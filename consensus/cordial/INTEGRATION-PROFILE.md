# Cordial integration profile and current progress

Status: local implementation, not production-qualified. The node has an explicit Cordial adapter. Casper remains the default.
Focused execution tests pass, including independent stores and competing receives. Full network acceptance and broader concurrency checks remain open.

## Approved scope

The user approved a fixed validator set for the first Cordial integration.
Rholang execution must occur after native tau ordering commits an object.
PoR is excluded from this integration. It can return after separate upstream development and approval.
Speculative multi-parent execution remains disabled.

The new `cordial-consensus` crate implements chain authentication, durable native admission, persisted output, an execution journal, and a lifecycle adapter.
It does not depend on Casper, Rholang, or the node crate.
The imported core and application runtime retain their separate responsibilities.
The new `cordial-rholang` crate connects committed output to the existing Rholang runtime.
This bridge currently imports `RuntimeManager` from Casper. That dependency remains outside the native protocol and shared runtime crates.

## Chain identity and packet authentication

`ChainSpec` contains the network name, shard name, execution genesis hash, fixed validators and weights, and wavelength.
Validator keys use compressed Secp256k1 encoding. Duplicate keys, other key encodings, and zero weights are rejected.

The adapter sorts validator keys before it computes the chain fingerprint.
The fingerprint includes the profile identifier and all chain parameters.
Each native block contains this fingerprint inside its signed payload.
Changing the network, shard, execution genesis, membership, weights, or wavelength invalidates the packet for this chain.

The profile uses a round-robin leader schedule over the sorted validator keys.
The schedule and payload format belong to `cordial-fixed-committee-ordered-rholang-v1`.
Changing these semantics requires a new profile identifier, even if the serialized configuration remains unchanged.

The wavelength bounds are input limits, not proof that every accepted configuration satisfies the desired production fault model.
The integration tests use wavelength three. Maintainers must review the intended quorum and leader configuration before production registration.

Initial blocks contain no application data. Later payloads are still opaque bytes in this integration layer.
The execution bridge validates signed Rholang deploys and derives VM context from committed records.
The execution genesis hash identifies the initial state. Node assembly must supply that state in RSpace before execution starts.

The block route is `cordial/block/v2`. Decoding rejects trailing bytes, unsupported versions, invalid signatures, and noncanonical predecessor order.
Limits are 1 MiB per packet, 256 KiB per application payload, and 1,024 predecessors.

Block authentication is separate from TLS peer authentication.
The node adapter sends native packets through the existing authenticated transport and peer discovery.
The active runtime exchanges bounded history pages through `cordial/sync/v1`.
Pages contain at most 32 packets and remain below the packet byte limit.
Periodic requests retry dropped gossip and recover late peers.
Each connected peer has a bounded synchronization cursor entry.

## Durable admission and recovery

`DurableBlocklace` owns one LMDB environment and one native in-memory blocklace.
An exclusive directory lease prevents two owners from opening the same store.
The versioned manifest binds the store to the exact normalized chain configuration and profile.
The store rejects unidentified directories and incompatible manifests.

Admission follows this sequence:

1. Decode and authenticate the signed native object.
2. Run mandatory native received-block validation.
3. Store the object and admission counter in one LMDB transaction.
4. Publish the admitted object in the live blocklace after the transaction succeeds.
5. Retry pending children through the same mandatory validation path.

Deferred objects persist across restart. Defaults limit pending state to 256 objects and 4 MiB.
Duplicates do not consume another pending slot.
When a dependency arrives, the store retries the pending objects and releases completed entries.
Permanently invalid pending objects are removed.

Both valid conflicting branches can persist. The store does not replace native equivocation semantics with a first-arrival rule.
Recovery authenticates and validates every stored object before it becomes live state.
Recovery rejects gaps, inconsistent counters, invalid records, and inconsistent output.

These tests use real temporary LMDB environments. They include actual map exhaustion, not a simulated successful write.
They test close and reopen recovery. They do not yet establish recovery from process termination at every write boundary or host power loss.

## Committed output

The store calls the unchanged native `weighted_tau` implementation.
It persists the ordered identities and output counter in one transaction.
The new output must extend the stored prefix. A conflicting prefix causes a fatal error.

Admission and output publication use separate transactions.
A restart can therefore find durable objects without their latest output records.
The lifecycle adapter calls `advance_output` before readiness to close this recovery gap.

Stored output is not an execution receipt. The execution journal separately tracks the next unacknowledged output record.
Each receipt contains the object identity, output index, pre-state, post-state, deploy identities, and execution results.
One LMDB transaction persists the receipt, deploy index, and execution cursor.
Recovery verifies the receipt sequence, state-root chain, deploy index, and correspondence with committed output.
A real LMDB map-exhaustion test verifies that failed acknowledgment leaves no partial receipt or deploy index.

The store manifest now uses schema two. Schema-one stores are rejected. No existing data directory is silently migrated.

## Committed Rholang execution

`CommittedRholang` consumes only persisted native output through `DurableBlocklace::next_execution`.
It never executes a newly admitted object before native ordering commits that object.

The version-one protobuf batch contains a signed block timestamp and at most 64 signed deploys.
The native object signature authenticates the timestamp and batch bytes. Each deploy retains its own signature.
The batch has a total limit of 10,000,000 phlo. The application payload limit also applies.
The bridge checks signature encoding, signature validity, shard, timestamps, expiration, valid-after index, and charge overflow.

The VM block number and sequence come from the committed output index.
The VM timestamp comes from the authenticated batch. The VM sender is the actual object creator.
The bridge converts the compressed native validator key into the VM public-key encoding.
The batch timestamp is not a wall-clock attestation. A reviewed chain-clock policy remains part of production validation.

Malformed batches and invalid deploys produce explicit rejection receipts without changing application state.
Duplicate deploys do not execute again. Failed deploys retain their VM cost and failure result.
Infrastructure failures leave the execution cursor unchanged and stop the runtime.

External services are disabled. There is no exactly-once guarantee for arbitrary external effects.
The bridge does not issue `CloseBlock` or other system deploys. Fee settlement and fixed-profile economic behavior need separate verification.
The ordered context and replay tests do not prove determinism for every concurrent Rholang program.
Cross-process execution agreement remains a production acceptance requirement.

## Shared runtime integration

`CordialIngressAdapter::prepare` returns the existing `PreparedConsensus` type.
Preparation does not open a store. Starting the runtime opens the store, recovers native state, and publishes pending output.
`prepare_with_executor` also drains pending execution before readiness and after successful packet admission.
The Rholang executor checks chain identity and the current RSpace root even when no output awaits execution.
The adapter owns this work. Execution does not run as a detached task.

The adapter then receives bounded packet requests through `ConsensusRuntime`.
The runtime rejects packets above 1 MiB before enqueueing them. A smaller configured limit remains effective.
Invalid packets produce typed input errors without stopping the runtime.
Pending-capacity exhaustion produces `QueueFull`.
Storage or state-integrity failures stop the adapter and mark runtime health as failed.
Shutdown rejects queued requests and releases the store.

The ingress-only adapter advertises no optional capabilities. Submit, proposal, and Casper-style finalized queries return `UnsupportedCapability`.
With an executor, readiness requires completion of recovered execution work. Without an executor, readiness describes only local consensus recovery.
Neither mode claims that the node synchronized with peers or supports proposal and submission APIs.

`CordialNode::prepare` supplies the active node adapter.
A configured validator advertises proposal support. A validator with an application executor also advertises deploy submission.
Observers reject submission and proposal requests. All Cordial nodes reject Casper-style last-finalized-block queries.

The proposer reconstructs the highest quorum-supported native prefix and uses native predecessor selection.
It stores the proposal before publication. Restart continues the persisted validator history.
The submit path validates signed deploys and stores a bounded durable pool.
Proposal admission removes selected pool entries only after the proposal is durable.
Execution acknowledgment removes matching pending entries with the execution receipt transaction.
Duplicate committed deploys never execute again.

## Node configuration and APIs

Select the adapter explicitly in the node configuration:

```hocon
consensus {
  protocol = "cordial-miners"
  cordial {
    chain-file = "/absolute/path/chain.json"
    validator-key-file = "/absolute/path/validator.key"
    tick-ms = 1000
  }
}
openai.enabled = false
```

Omit `validator-key-file` for an observer. A validator key file contains one hexadecimal Secp256k1 secret and requires owner-only permissions.
Do not publish validator key files or include them in test evidence exports.
The transport network must match the chain configuration.

`chain.json` contains `chain: ChainSpec` and `genesis: GenesisSettings`.
The node requires a nonzero pinned execution genesis root.
Use the generator to derive a pin from a draft configuration. The generator refuses to overwrite an existing file.

```bash
rtk cargo run --locked --offline --release -p node --example cordial_genesis -- \
  --input chain-draft.json --output chain.json
```

The root manifest pins the protocol, storage version, committee, execution genesis, and genesis settings.
An exclusive root lease prevents simultaneous owners. Cordial rejects Casper stores and changed chain configurations.
Execution uses `cordial-execution/`. Native consensus uses `cordial-consensus/`.

| Interface | Supported operation |
| --- | --- |
| gRPC `doDeploy` | Submit a signed `DeployDataProto`. Return its Cordial deploy identifier. |
| gRPC `propose` | Request a synchronous native proposal. Return its native object identifier. |
| HTTP `POST /api/deploy` | Submit binary `DeployDataProto` bytes. |
| HTTP `GET /api/status` | Read local readiness and admitted, committed, and executed counts. |
| HTTP `GET /api/cordial/output?start=0&limit=32` | Read a bounded committed prefix page. |
| HTTP `GET /api/cordial/objects/{id}` | Read the encoded native object. |
| HTTP `GET /api/cordial/receipts/{index}` | Read a durable execution receipt and decoded result. |
| HTTP `POST /api/cordial/data` | Read a Rholang channel at an executed state root. |
| Admin HTTP `POST /api/propose` | Request a synchronous native proposal. |

Unsupported Casper queries return gRPC `Unimplemented`. Unsupported HTTP operations return status 501.
Local readiness means recovery completed and the adapter accepts work. It does not assert network quorum or recent finality progress.

The current adapter performs native calculations and LMDB operations synchronously inside its task.
One expensive operation cannot be interrupted by the runtime deadline.
Before production registration, add bounded admission work and supervised blocking-work ownership without detached writes after shutdown.

## Remaining production gates

| Gate | Remaining work |
| --- | --- |
| Node selection | Implemented. Three focused tests cover selection, manifest safety, deploy execution, and restart without Casper stores. |
| Real transport | Implemented. Real-node TLS acceptance is pending. |
| Proposal | Implemented. Native proposal and persisted-history tests pass. Network acceptance is pending. |
| Execution | Verify economic behavior, independent-process execution, interrupted execution recovery, and concurrent Rholang determinism. |
| Application APIs | Implemented. HTTP deploy and receipt checks pass. Full gRPC and network checks are pending. |
| Resource control | Bound history traversal and pending retries. Supervise blocking work and prove shutdown behavior under load. |
| Evidence | Expose durable equivocation evidence without enabling unapproved PoR membership changes. |
| Fault recovery | Test process termination, disk faults, corrupted journals, and restart at admission, output, and execution boundaries. |
| Network acceptance | Run real multi-process Cordial startup, deploy, proposal, finality, execution, late join, partition recovery, and restart scenarios. |
| Casper preservation | Complete the two resource-limited official network cases on this host when sufficient memory is available. |

Do not deploy this Cordial profile to a production chain until these gates pass.
Native tests and ingress tests cannot replace the full network acceptance test.

## Verification commands

Run from `f1r3node-rust`:

```bash
rtk proxy python3 scripts/check_cordial_import.py
rtk cargo test --locked --offline --release -j 1 -p cordial-consensus
rtk cargo test --locked --offline --release -j 1 -p cordial-rholang
rtk cargo test --locked --offline --release -j 1 -p cordial-miners-core
rtk cargo test --locked --offline --release -j 1 -p cordial-app-runtime
rtk cargo test --locked --offline --release -j 1 -p consensus-api -p consensus-runtime
rtk cargo test --locked --offline --release -j 1 -p node --test consensus_cordial
```

The core suite contains socket tests. A sandbox socket denial requires local-network permission, not a skipped test.

## Local commit organization

The local commits separate the native import, storage, runtime, execution bridge, and node integration.
Each implementation commit includes its focused tests. The full network harness remains outside these verified acceptance claims.

```text
feat(cordial): import pinned native core with v2 admission hardening
feat(cordial): persist chain-bound blocklace and execution journal
feat(cordial): add supervised proposal and synchronization runtime
feat(cordial): execute committed deploys on an isolated deterministic VM
feat(node): integrate Cordial consensus adapter and native APIs
```
