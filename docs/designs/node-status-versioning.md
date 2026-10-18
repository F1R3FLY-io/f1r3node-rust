# Node Status Versioning and Capabilities

- **Status:** Proposed 2026-10-15 for maintainer review. No code changes yet. TASK-013-8 implements this record after approval.
- **Epic:** EPIC-013 (release process). Related: EPIC-014 TASK-014-10 (test net faucet).
- **Governing text:** [`docs/release-process.md`](../release-process.md) Sections 5.1 to 5.5 (proposed)
- **Source proposal:** F1r3Faucet [`docs/designs/node-compatibility.md`](https://github.com/F1R3FLY-io/F1r3Faucet/blob/dev/docs/designs/node-compatibility.md), Sections 3.2, 3.3, and 3.5
- **Code:** `node/src/rust/api/web_api.rs` (`ApiStatus`, `VersionInfo`), `node/src/rust/api/deploy_grpc_service_v1.rs` (`status`), `models/src/main/protobuf/DeployServiceCommon.proto` (`Status`, `VersionInfo`)

## 1. Purpose

Clients such as F1r3Faucet, wallets, and explorers must find out from a node whether they can use it. Today the status response gives the build version and a fixed API version. It does not show capabilities, the consensus model, or the genesis.

This record specifies four additions to the status response of the HTTP API and the gRPC API:

1. The client API versions that the node serves
2. A capability list with stable names
3. The consensus model
4. The genesis identifier

## 2. Current state

| Item | HTTP `/api/status` | gRPC `status` |
|------|--------------------|---------------|
| API version | `version.api`, JSON string, fixed to `"1"` (`web_api.rs`) | `VersionInfo.api`, field 1, `string`, fixed to `"1"` (`deploy_grpc_service_v1.rs`) |
| Build version | `version.node` | `VersionInfo.node`, field 2, `string` |
| Capabilities | None | None |
| Consensus model | None | None |
| Genesis | None | None |

`Status` uses protobuf fields 1 to 17. `VersionInfo` uses fields 1 and 2.

## 3. Proposed fields

All fields are additive. Under release-process Section 5.1 they need a PATCH increase before 1.0.0 (decision V2).

| JSON name | Protobuf | Type | Value |
|-----------|----------|------|-------|
| `version.apiVersion` | `VersionInfo.apiVersion = 3` | integer (`uint32`) | The newest client API version that the node serves. Today `1` |
| `version.apiSupported` | `VersionInfo.apiSupported = 4` | list of integers (`repeated uint32`) | Every client API version that the node serves, in increasing order. Today `[1]` |
| `capabilities` | `Status.capabilities = 18` | list of strings (`repeated string`) | Capability names (Section 4), in sorted order |
| `consensusModel` | `Status.consensusModel = 19` | string | The `consensus_model` identifier of the release process. Today `cbc-casper` |
| `genesisId` | `Status.genesisId = 20` | string | Hex hash of the approved genesis block. Empty until Casper starts |

### 3.0 Protocol fields (proposed 2026-10-18)

Release-process Sections 5.6 to 5.8 add change classes for a running shard. Clients and operators must see the protocol that a shard runs and any activation that is coming. These fields are also additive.

| JSON name | Protobuf | Type | Value |
|-----------|----------|------|-------|
| `protocolVersion` | `Status.protocolVersion = 21` | integer (`uint32`) | The running Casper protocol version. The node reads it from the approved block, not from its configuration (Casper decision ledger, D-01). Today `1`, the value of `CASPER_PROTOCOL_VERSION` |
| `genesisProfile` | `Status.genesisProfile = 22` | string | The profile that the genesis policy selects, for example an order-then-execute profile of protocol 7. Empty for the default profile |
| `pendingActivations` | `Status.pendingActivations = 23` | list of messages | Each activation that the shard state records and that has not occurred yet: the new rule set and the epoch or block height. Empty today. Only an activation mechanism under decision V7 can fill it |

A client that sees a pending activation can warn its user before the shard changes rules. The field numbers follow the fields of the table above. They are proposals, like the other fields of this section.

### 3.1 The `version.api` field (decision D1)

The F1r3Faucet proposal asks for an integer `version.api`. The field is a `string` in the protobuf message and in the JSON response. A type change breaks every client that decodes the field. Under release-process Section 5.2, that change itself would need client API version 2.

This record therefore proposes:

- `version.api` keeps its type and stays the decimal form of `version.apiVersion`, for example `"1"`. It is deprecated.
- Clients read `version.apiVersion` and `version.apiSupported`.
- A later client API version 2 can remove `version.api` when the overlap window (decision V3) ends.

**Decision D1** for the maintainers: accept this additive form, or change `version.api` to an integer inside a client API version 2.

### 3.2 Genesis identifier

`genesisId` is the block hash of the approved block that the node started from (`casper.get_approved_block()`). A shard that restarts with a fresh genesis gets a new `genesisId`, even when its `shardId` stays the same. A client treats a new `genesisId` as a new shard.

Before Casper starts, `genesisId` is an empty string. A client must not treat an empty value as a new shard.

### 3.3 One source for both APIs

The HTTP and gRPC handlers build the status separately today. The implementation adds one function that returns the version, capability, consensus, and genesis values. Both handlers call it, so the two APIs cannot report different values.

## 4. Capabilities

A capability is a stable, lowercase, dot-separated name. A node lists a capability only when the feature works on that node. The names are decision V4, shared with F1r3Faucet. The proposal starts with these names:

| Name | Meaning |
|------|---------|
| `deploy.expirationTimestamp` | Deploys accept an expiration timestamp |
| `deploy.parameters` | Deploys accept typed parameters |
| `query.balance` | The node answers vault balance queries |
| `query.deployFinalizationStatus` | The node reports the finalization status of a deploy |
| `vault.system` | `rho:vault:system` resolves |
| `vault.alias.rchain` | The deprecated `rho:rchain:revVault` and `rho:rchain:pos` resolve (pull request #733) |
| `vault.alias.f1r3fly` | `rho:f1r3fly:revVault`, `rho:f1r3fly:pos`, and `rho:f1r3fly:multiSigRevVault` resolve (pull request #733) |
| `consensus.cbc-casper` | The shard runs F1r3fly CbC Casper |

A registry alias is part of genesis state. The `vault.alias.*` capabilities must therefore report what the genesis of the shard registered, not what the binary contains. The implementation reads this from the registry of the running shard or from the genesis configuration. A node of a new release on an old shard must not report an alias that its shard lacks.

Removal of a capability is a breaking change (release-process Section 5.3).

## 5. Acceptance criteria

1. `/api/status` and gRPC `status` return `apiVersion`, `apiSupported`, `capabilities`, `consensusModel`, and `genesisId` with the same values.
2. `version.api` stays a string with the decimal value of `apiVersion`.
3. `genesisId` equals the hash of the approved block, and it is empty before Casper starts.
4. Each `vault.alias.*` capability appears only when the shard registry resolves the alias.
5. A client built against the current status response still decodes the new response. A test decodes the new protobuf message with the old message definition.
6. The OpenAPI schema (`ToSchema`) and the API documentation show the new fields.

## 6. Decisions

| ID | Decision | Owner |
|----|----------|-------|
| D1 | Additive `apiVersion` and `apiSupported`, or an integer `version.api` in client API version 2 (Section 3.1) | f1r3node-rust maintainers |
| V4 | Capability names (Section 4) | f1r3node-rust and F1r3Faucet maintainers |
| D2 | Add the protocol fields of Section 3.0. `pendingActivations` stays empty until decision V7 ratifies an activation mechanism. | f1r3node-rust maintainers |

Decisions V1 to V3 and V5 to V9 are in release-process Section 5.5.
