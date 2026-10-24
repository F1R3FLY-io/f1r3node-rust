# API Reference

## Ports

| Port | Protocol | Service |
|------|----------|---------|
| 40401 | gRPC | DeployService (external) |
| 40402 | gRPC | ProposeService (internal) |
| 40403 | HTTP | Public REST API |
| 40405 | HTTP | Admin REST API |

---

## Error Responses

All HTTP endpoints return errors as structured JSON. Every non-2xx response body conforms to this schema:

```json
{
  "error": "block_not_found",
  "message": "Block not found: 3bfdf56f"
}
```

| Field | Description |
|-------|-------------|
| `error` | Machine-readable error kind. Stable across node versions — safe to switch on in client code. |
| `message` | Human-readable description. May change between releases. |

### Status codes

| Code | Meaning | Common `error` values |
|------|---------|----------------------|
| `400` | Client sent invalid input | `invalid_request_body`, `invalid_path_parameter`, `invalid_query_parameter`, `invalid_hash`, `illegal_argument`, `rholang_bad_term`, `readonly_node_required`, `validator_node_required`|
| `404` | Requested resource not found | `block_not_found`, `deploy_not_found`, `endpoint_not_found` |
| `405` | HTTP method not allowed for this path | `method_not_allowed` |
| `422` | Input is valid but execution failed | `rholang_execution_error`, `out_of_phlogistons`, `user_abort`, `aggregate_error` |
| `409` | Expected state conflict (empty mempool) | `no_new_deploys` |
| `500` | Node-side failure | `interpreter_internal_error`, `runtime_error`, `replay_failure`, `signing_error`, `kv_store_error`, `history_error`, `system_runtime_error`, `stream_error`, `lock_error`, `other_error`, `unknown_error` |
| `502` | Upstream or peer communication failure | `comm_error`, `external_service_error` |

### Read-only-only endpoints on validators

The following endpoints run exploratory deploys internally and are only available on read-only nodes. On validator nodes they return `400 readonly_node_required`:

- `POST /api/explore-deploy`
- `POST /api/explore-deploy-by-block-hash`
- `POST /api/estimate-cost`
- `GET /api/balance/{address}`
- `GET /api/registry/{uri}`
- `GET /api/validators`
- `GET /api/validator/{pubkey}`
- `GET /api/epoch/rewards`

```json
HTTP/1.1 400 Bad Request
Content-Type: application/json

{"error": "readonly_node_required", "message": "Exploratory deploy requires a read-only node"}
```

Route these requests to a read-only node (typically port 40453). `GET /api/status` exposes `isValidator` and `isReadOnly` so clients can pick a target dynamically.

---

## HTTP REST API (port 40403)

### `GET /api/status`

Node identity, network membership, and operational state. Available on all node types.

**Parameters:** None

```bash
curl http://localhost:40403/api/status
```

```json
{
  "version": {"api": "1", "node": "F1r3node Rust 0.4.10 ()"},
  "address": "rnode://1e780e5d...@rnode.bootstrap?protocol=40400&discovery=40404",
  "networkId": "testnet",
  "shardId": "root",
  "peers": 4,
  "nodes": 4,
  "minPhloPrice": 1,
  "peerList": [
    {"address": "rnode://...", "nodeId": "a5aec03d...", "host": "rnode.validator3", "protocolPort": 40400, "discoveryPort": 40404, "isConnected": true}
  ],
  "nativeTokenName": "F1R3CAP",
  "nativeTokenSymbol": "F1R3",
  "nativeTokenDecimals": 8,
  "lastFinalizedBlockNumber": 28,
  "isValidator": true,
  "isReadOnly": false,
  "isReady": true,
  "currentEpoch": 2,
  "epochLength": 10
}
```

| Field | Type | Description |
|-------|------|-------------|
| `version` | object | API and node version |
| `address` | string | Node's rnode:// address |
| `networkId` | string | Network identifier |
| `shardId` | string | Shard identifier |
| `peers` | int | Connected peer count |
| `nodes` | int | Discovered node count |
| `minPhloPrice` | int | Minimum phlogiston price for deploys — the chain-adopted floor once casper is up; until then the local genesis-seed value. `isReady: false` marks every value in this payload provisional |
| `peerList` | array | Detailed peer info with connection status |
| `nativeTokenName` | string | Full token name from genesis |
| `nativeTokenSymbol` | string | Token ticker symbol |
| `nativeTokenDecimals` | int | Decimal places (dust per token = 10^decimals) |
| `lastFinalizedBlockNumber` | int | LFB block number. `-1` if casper not yet initialized |
| `isValidator` | bool | `true` if the node has a configured validator key; this does not confirm current bond status |
| `isReadOnly` | bool | `true` if running in read-only mode |
| `isReady` | bool | `true` after engine enters Running state |
| `currentEpoch` | int | `lastFinalizedBlockNumber / epochLength` |
| `epochLength` | int | Blocks per epoch, from genesis configuration |

---

### `GET /api/ready`

Readiness probe. Returns HTTP 200 with `{"ready": true}` once the Casper engine has entered the Running state and the node can serve deploys and exploratory deploys. Returns HTTP 503 with `{"ready": false}` while Casper is still initializing (genesis ceremony, or last-finalized-state sync on an observer).

Unlike `GET /api/status`, which answers 200 throughout startup, this endpoint fails until the node is deploy-ready, so it works directly as a container health check (`docker compose up --wait`, `depends_on: condition: service_healthy`) and as a Kubernetes `httpGet` readiness probe without needing `jq`.

A node running the genesis ceremony (minutes to hours) reports `unhealthy` for the whole window, because it cannot serve deploys yet. Operators spanning a long genesis raise `--wait-timeout` (Docker Compose) or probe `failureThreshold` / `start_period` (Kubernetes, Docker `HEALTHCHECK`) accordingly.

---

### Block Endpoints

All block endpoints support `?view=full|summary`. Single-item endpoints default to **full** (includes deploys). List endpoints default to **summary** (block headers only, deploys omitted).

#### `GET /api/block/{hash}`

Get a block by hash or hex prefix. Default view: full.

| Parameter | Location | Required | Description |
|-----------|----------|----------|-------------|
| `hash` | path | yes | Full 64-char hex block hash, or a hex prefix of at least 6 characters for prefix lookup |
| `view` | query | no | `full` (default) or `summary` |

```bash
curl http://localhost:40403/api/block/3bfdf56f...
curl "http://localhost:40403/api/block/3bfdf56f...?view=summary"
```

Full response includes `blockInfo` (header with `isFinalized`) + `deploys` array. Summary omits `deploys`.

Each deploy includes `parameters` when its signed data contains named values.
The JSON format matches [`data.parameters`](#deploy-parameters) in deploy submissions.
The node omits empty parameter arrays.
Full block lists and trace reports use the same parameter format.
Summary block responses omit deploy data, including parameters.

| Status | Condition |
|--------|-----------|
| `200` | Block found |
| `400` | Hash shorter than 6 chars or contains non-hex characters (`invalid_hash`) |
| `404` | No block matching the hash or prefix (`block_not_found`) |
| `500` | Node-side failure |

#### `GET /api/last-finalized-block`

Get the last finalized block. Default view: full.

| Parameter | Location | Required | Description |
|-----------|----------|----------|-------------|
| `view` | query | no | `full` (default) or `summary` |

```bash
curl http://localhost:40403/api/last-finalized-block
curl "http://localhost:40403/api/last-finalized-block?view=summary"
```

#### `GET /api/blocks/{depth}`

Get recent blocks by depth. Default view: summary.

| Parameter | Location | Required | Description |
|-----------|----------|----------|-------------|
| `depth` | path | yes | Number of most-recent blocks to return; clamped to the configured maximum |
| `view` | query | no | `summary` (default) or `full` |

```bash
curl http://localhost:40403/api/blocks/5
curl "http://localhost:40403/api/blocks/5?view=full"
```

#### `GET /api/blocks/{start}/{end}`

Get blocks by height range. Default view: summary.

| Parameter | Location | Required | Description |
|-----------|----------|----------|-------------|
| `start` | path | yes | Start block height (inclusive) |
| `end` | path | yes | End block height (inclusive); clamped to the configured maximum range |
| `view` | query | no | `summary` (default) or `full` |

```bash
curl http://localhost:40403/api/blocks/100/110
```

#### `GET /api/blocks`

Get the most recent block. Default view: summary.

| Parameter | Location | Required | Description |
|-----------|----------|----------|-------------|
| `view` | query | no | `summary` (default) or `full` |

```bash
curl http://localhost:40403/api/blocks
```

#### `GET /api/is-finalized/{hash}`

Check if a block is finalized. Returns `true` or `false`.

| Parameter | Location | Required | Description |
|-----------|----------|----------|-------------|
| `hash` | path | yes | Full 64-char hex block hash |

```bash
curl http://localhost:40403/api/is-finalized/3bfdf56f...
```

---

### Deploy Endpoints

#### `POST /api/deploy`

Submit a signed deploy to the network. Validator nodes only.

**Protocol 6 (DR-66).** A protocol-6 node accepts only offered-funded deploys. This route takes a body-only deploy, so a node that runs Casper refuses it with `400` (`illegal_argument`, message `new deploys require offered-funded cost accounting`). Submit an offered-funded envelope with [`POST /api/deploy/offered`](#post-apideployoffered) or with gRPC `doDeploy`. The [`node deploy`](#command-line-deploy) command signs an owner-direct offer.

**Request body:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `data.term` | string | yes | Rholang source code |
| `data.timestamp` | int | yes | Deploy timestamp (ms since epoch) |
| `data.phloPrice` | int | yes | Phlogiston price per unit |
| `data.phloLimit` | int | yes | Maximum phlogiston to consume |
| `data.validAfterBlockNumber` | int | yes | Deploy valid after this block number |
| `data.shardId` | string | yes | Target shard (e.g. `"root"`) |
| `data.parameters` | array | no | Named Rholang values. The default is an empty array. |
| `deployer` | string | yes | Deployer public key (hex) |
| `signature` | string | yes | Deploy signature (hex) |
| `sigAlgorithm` | string | yes | Signature algorithm (`"secp256k1"`) |

```bash
curl -X POST http://localhost:40413/api/deploy \
  -H 'Content-Type: application/json' \
  -d '{
    "data": {
      "term": "new stdout(`rho:io:stdout`) in { stdout!(42) }",
      "timestamp": 1700000000000,
      "phloPrice": 10,
      "phloLimit": 100000,
      "validAfterBlockNumber": 0,
      "shardId": "root"
    },
    "deployer": "04abc...",
    "signature": "3044...",
    "sigAlgorithm": "secp256k1"
  }'
```

| Status | Condition |
|--------|-----------|
| `200` | Deploy accepted; body is the deploy ID (hex string) |
| `400` | Malformed body or invalid field value: read-only node, unbonded validator, wrong shard ID, forbidden key, phlo price below minimum, deploy expired, body-only deploy on a protocol-6 node (`invalid_request_body`, `illegal_argument`, `rholang_bad_term`) |
| `422` | Term valid but execution failed (`rholang_execution_error`, `out_of_phlogistons`, `user_abort`) |
| `500` | Node-side failure (`interpreter_internal_error`, `replay_failure`, `signing_error`) |
| `502` | Peer communication failure (`comm_error`) |

Deploy admission checks the node's validator against the last finalized block's committee. Proposal checks the selected parent's committee. These sets can differ during the finality gap after a bond change.

##### Deploy parameters

A deploy parameter supplies a typed value through `rho:deploy:param:<name>`.
The deploy signature covers parameter names, values, and array order.
The node rejects changes to these fields after signing.

```json
{
  "parameters": [
    {"name": "items", "value": {"type": "list", "value": [
      {"type": "int", "value": -42},
      {"type": "nil"}
    ]}}
  ]
}
```

```rholang
new items(`rho:deploy:param:items`), stdout(`rho:io:stdout`) in {
  stdout!(*items)
}
```

| JSON `type` | JSON `value` |
|-------------|--------------|
| `bool` | A Boolean. |
| `int` | A signed 64-bit integer. |
| `string` | A string. |
| `bytes` | An array of integers from 0 through 255. |
| `tuple`, `list`, `set` | An array of typed values. |
| `map` | An array of objects with typed `key` and `value` fields. |
| `nil` | Omit the `value` field. |
| `uri` | A URI string, used as a Rholang value. |

Lists and tuples preserve element order.
Sets remove duplicate values and use Rholang ordering.
Maps use Rholang key ordering and reject duplicate keys, including equivalent set keys.
A URI parameter does not grant access to a system process.

Parameter names must be unique and match `[A-Za-z_][A-Za-z0-9_]*`.
Each name can contain at most 256 bytes.
A deploy can contain at most 50 parameters, 32 nesting levels, and 10,000 values.
The total data budget is 1 MiB, including names, string data, byte data, and eight bytes per value.
Transport limits also apply.

Deploys without parameters retain their protobuf bytes and signatures.
JSON output omits an empty parameter array.

The deploy storage and rejected buffer can read existing records without parameters.
Both stores write every new record as protobuf with the `F1R3DEP` prefix and version byte `0x01`.
This storage format also applies to deploys without parameters.
Nodes that support only the old storage format cannot read these new records.
Reading a legacy record does not rewrite that record.

The `deploy.storage.legacy_decode` counter records each attempt to decode legacy bincode data in either store.
The counter includes repeated reads and failed attempts.
Retain legacy support until active nodes show no legacy decode attempts over an agreed observation period.
A zero increase without store reads does not prove that legacy records are absent.

The earlier Scala implementation used protobuf field 13 for parameters.
This Rust schema uses field 13 for `expirationTimestamp` and field 22 for parameters.
Primitive value fields retain their original numbers, including `sint64` for integers.

To migrate a client from that Scala schema:

1. Generate the client types from this repository's protobuf files.
2. Encode parameters in field 22 with the recursive value schema.
3. Apply the name, size, and nesting limits above.
4. Sign the deploy again with its new protobuf bytes.

The Scala implementation permitted larger byte parameters.
Clients must apply the total data budget before they sign a deploy for this schema.
Nodes that use the earlier schema cannot validate these parameter signatures.
All validators must use a compatible schema before the network accepts deploy parameters.

#### `POST /api/deploy/offered`

Submit an offered-funded deploy (protocol 6). Validator nodes only.

An offered-funded deploy is a signed envelope. It holds the deploy body, the signed funding intent and the signatures. The funding intent names the vaults that pay, the maximum REV that each vault can pay, and the resource schedule and price that the signers accept. The REV ceiling of the offer is $`\text{phloLimit} \times \text{phloPrice} + 1`$ atomic REV, where 1 is the fixed deploy fee.

The node checks the offer before it queues it. The offer must use the offered-funded format and sign the schedule of the adopted genesis resource policy at its own price. It must also target this shard, be inside the block lifespan, and have a price at or above the adopted minimum. The proposer then compares the linear cost estimate of the offer with the vault balances at the block pre-state (DR-122). The proposer does not include an offer that the vaults cannot cover. That offer has no effect and no charge.

**Request body:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `deployDataProtoBase64` | string | yes | Canonical standard base64 of the canonical protobuf encoding of the `DeployDataProto`. Its `authorizationV61.formatVersion` is the offered-funded format |

```bash
curl -X POST http://localhost:40413/api/deploy/offered \
  -H 'Content-Type: application/json' \
  -d '{ "deployDataProtoBase64": "CgRuZXcg…" }'
```

| Status | Condition |
|--------|-----------|
| `200` | Offer accepted. The body is `"Success!\nDeployId is: <hex>"`, where the deploy ID is the 32-byte envelope identity |
| `400` | Invalid or noncanonical base64 or protobuf, read-only node, inactive genesis policy, schedule or price that differs from the signed terms, price below the adopted minimum, or deploy outside the block lifespan (`invalid_request_body`, `illegal_argument`) |
| `409` | The offer is already known (`duplicate_deploy`) |

#### `GET /api/deploy/offered/{deploy_id}/receipt`

Return the settlement receipt of an offered deploy that a block holds. `deploy_id` is the hex encoding of the 32-byte deploy ID. The node checks the stored receipt against the committed cost evidence of the deploy before it returns the receipt. The body is `null` when the deploy is not an offered deploy.

| Field | Type | Description |
|-------|------|-------------|
| `deployId` | string | The deploy ID (hex) |
| `blockHash` | string | The block that holds the deploy (hex) |
| `phloUsed` | int | The phlo that the run used |
| `freshPhlo` | int | The part of `phloUsed` that no existing prepaid receipt covers |
| `retainedPhlo` | int | The phlo of newly retained resources, a separate acquisition |
| `phloLimit` | int | The signed phlo limit |
| `phloPrice` | int | The signed phlo price |
| `feeRev` | string | The fixed deploy fee in atomic REV |
| `revSpent` | string | $`(\text{freshPhlo} - \text{ownerPhlo} + \text{retainedPhlo}) \times \text{phloPrice} + \text{ownerRev} + \text{feeRev}`$ |
| `revCeiling` | string | $`\text{phloLimit} \times \text{phloPrice} + \text{feeRev}`$ |
| `purses` | array | One entry for each paying vault: `address` (hex), `resourceRev`, `feeRev` and `postBalance`, all as decimal strings. The owner vaults of stored processes that the deploy used are included, and a vault that pays as a source and as an owner has one entry |

The REV amounts are decimal strings, because they are 128-bit values.

`ownerPhlo` and `ownerRev` are the phlo and the REV that the owners of stored processes paid for this deploy (DR-135). They are fields of the committed cost evidence, not of the receipt. Each owner pays at the price that its stored region recorded, so `revSpent` can exceed `revCeiling` by `ownerRev`. The signed limit bounds only the work that the caller pays: $`\text{phloUsed} - \text{ownerPhlo} + \text{retainedPhlo} \le \text{phloLimit}`$, so `phloUsed` can exceed `phloLimit` by at most `ownerPhlo` (DR-135 item 13). Without owner charges, `revSpent` is $`(\text{freshPhlo} + \text{retainedPhlo}) \times \text{phloPrice} + \text{feeRev}`$.

| Status | Condition |
|--------|-----------|
| `200` | The receipt, or `null` for a deploy that is not offered |
| `400` | The deploy ID is not 32 bytes of hex (`invalid_hash`) |
| `404` | No block holds the deploy (`deploy_not_found`) |
| `500` | The stored receipt differs from the committed cost evidence |

#### Command-line deploy

`node deploy` signs an owner-direct offer and submits it through gRPC `doDeploy`. In an owner-direct offer, the signer's own vault pays for every resource class of the signer's own lane. The positional arguments are the same as on dev: the phlo limit, the phlo price, the valid-after block number, the private key or the key file, the Rholang file and the shard ID. The source caps and the total exposure of the offer are its REV ceiling, so the signer accepts the same maximum cost as a dev deploy with the same limit and price.

| Option | Description |
|--------|-------------|
| `--resource-policy <hex>` | The hexadecimal genesis resource policy of the shard, as in `casper.genesis-block-data.resource-policy`. The offer signs its schedule at the given price. Without this option, the offer signs the default resource schedule of the shipped configurations for the given shard |

The default resource schedule has four resource classes: compute, introduction, transfer and trace, each at weight 1. It settles in atomic REV at 8 decimals under the native compatibility rule. The shipped configurations (`docker/conf`, `run-local/conf` and the Helm chart) carry its policy for the `root` shard.

#### `GET /api/deploy/{deploy_id}`

Get deploy execution details by deploy ID.

| Parameter | Location | Required | Description |
|-----------|----------|----------|-------------|
| `deploy_id` | path | yes | Hex-encoded deploy ID (deploy signature) |
| `view` | query | no | `full` (default) or `summary` |

```bash
curl http://localhost:40403/api/deploy/abc123...
curl "http://localhost:40403/api/deploy/abc123...?view=summary"
```

**Full response fields:**

| Field | Type | Description |
|-------|------|-------------|
| `deployId` | string | Deploy signature ID |
| `blockHash` | string | Containing block hash |
| `blockNumber` | int | Containing block number |
| `timestamp` | int | Block timestamp |
| `cost` | int | Phlogiston consumed |
| `errored` | bool | Whether execution failed |
| `isFinalized` | bool | Whether the containing block is finalized |
| `deployer` | string | Deployer public key (full only) |
| `term` | string | Rholang source (full only) |
| `systemDeployError` | string | System deploy error message (full only) |
| `phloPrice` | int | Phlo price (full only) |
| `phloLimit` | int | Phlo limit (full only) |
| `sigAlgorithm` | string | Signature algorithm (full only) |
| `validAfterBlockNumber` | int | Valid-after constraint (full only) |
| `transfers` | array/null | Transfer list or null on validators (full only) |
| `parameters` | array | Named Rholang values from the signed deploy. The node omits this field in summary responses and when no parameters exist. |

**Summary** returns only: `deployId`, `blockHash`, `blockNumber`, `timestamp`, `cost`, `errored`, `isFinalized`.

| Status | Condition |
|--------|-----------|
| `200` | Deploy found |
| `400` | Deploy ID is not valid hex (`invalid_hash`) |
| `404` | No deploy with this ID found in any finalized block (`deploy_not_found`) |
| `500` | Node-side failure |

#### `GET /api/deploy-finalization-status/{deploy_sig_hex}`

Query the canonical-state finalization status of a deploy by its signature.

Prefer this over polling `is-finalized` on the containing block. A block can finalize while some of its deploys' effects are dropped during a merge — polling by block hash would return `true` misleadingly.

| Parameter | Location | Required | Description |
|-----------|----------|----------|-------------|
| `deploy_sig_hex` | path | yes | Hex-encoded deploy signature (with or without `0x` prefix) |

```bash
curl http://localhost:40403/api/deploy-finalization-status/3044...
```

```json
{"state": "Finalized", "rejectionCount": 0, "latestBlockHash": "3bfdf56f..."}
```

Possible `state` values: `Finalized`, `Failed`, `Pending`, `Expired`.

| Field | Type | Description |
|-------|------|-------------|
| `state` | string | Deploy finalization state |
| `rejectionCount` | int | Number of times the deploy was rejected during finalization |
| `latestBlockHash` | string/null | Hex block hash where the deploy was last seen; `null` if never included |

| Status | Condition |
|--------|-----------|
| `200` | Status determined |
| `400` | Signature is not valid hex (`invalid_hash`) |
| `500` | Node-side failure |

#### `GET /api/pending-deploys`

Bulk snapshot of deploys currently queued in the node's local proposer pools: `deploy_storage` (submitted, not yet proposed) and `rejected_deploy_buffer` (recovering after a merge conflict). Each entry carries an `isRejected` flag so consumers can distinguish fresh deploys from recovery-backlog deploys.

This HTTP response does not include deploy parameters.
After block inclusion, use the full deploy or block response to read parameters.

The queue is **node-local**: deploys never gossip between nodes, so an observer (read-only) node always answers `{"deploys": [], "totalAvailable": 0}` — it rejects `doDeploy`, so it never holds pending deploys. Route this request to a validator. For deploy status that is consistent across nodes, use `GET /api/deploy-finalization-status/{sig}` instead, which is DAG-derived.

Already-included, future (`validAfterBlockNumber` ahead of the tip), and expired deploys are filtered out; a signature sitting in both pools is reported once with `isRejected: true`.

**Offered deploys (protocol 6, DR-66).** On a protocol-6 node the pools hold offered-funded envelopes, and the snapshot lists them under the same window, expiry, scope and duplicate rules. An envelope entry uses the same fields: `phloPrice` and `phloLimit` are the signed offer values, `deployer` is the public key of the primary signer, and `sig` is the hex encoding of the 32-byte deploy ID (the envelope identity), which the offered routes take as `deploy_id`.

**Parameters:**

| Parameter | Location | Required | Description |
|-----------|----------|----------|-------------|
| `deployer` | query | no | Hex-encoded deployer public key; omit to return all pending deploys |

```bash
curl http://localhost:40403/api/pending-deploys
curl http://localhost:40403/api/pending-deploys?deployer=04abc...
```

```json
{
  "deploys": [
    {
      "term": "@1!(1)",
      "timestamp": 1700000000000,
      "phloPrice": 1,
      "phloLimit": 90000,
      "validAfterBlockNumber": 0,
      "shardId": "",
      "deployer": "04abc...",
      "sig": "3044...",
      "sigAlgorithm": "secp256k1",
      "expirationTimestamp": null,
      "isRejected": false
    }
  ],
  "totalAvailable": 1
}
```

| Field | Type | Description |
|-------|------|-------------|
| `deploys` | array | Pending deploys, sorted by `(timestamp, sig)` |
| `totalAvailable` | int | Count before cap truncation. If `totalAvailable > deploys.length`, more entries exist than returned |
| `deploys[].isRejected` | bool | `true` = rejected-recovery buffer (merge conflict), `false` = fresh in deploy_storage |

Responses are capped at 1000 entries; compare `totalAvailable` with the array length to detect truncation.

| Status | Condition |
|--------|-----------|
| `200` | Snapshot taken |
| `400` | Deployer key is not valid hex (`invalid_public_key`) |
| `500` | Node-side failure (including bootstrapping node, Casper not yet initialised) |

#### `GET /api/prepare-deploy`

Get the next sequence number for the node's validator (or `-1` if not a validator). Use `seqNumber` as `validAfterBlockNumber` in the deploy data — it is the validator's next expected sequence number, not the deployer's.

**Parameters:** None

```bash
curl http://localhost:40403/api/prepare-deploy
```

```json
{"names": [], "seqNumber": 20}
```

#### `POST /api/prepare-deploy`

Same as GET, but additionally pre-generates unforgeable private names for a given deployer and timestamp. Equivalent to gRPC `previewPrivateNames`.

**Request body:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `deployer` | string | yes | Deployer public key (hex) |
| `timestamp` | int | yes | Deploy timestamp (ms since epoch) |
| `nameQty` | int | yes | Number of unforgeable names to generate (max 1024) |

```bash
curl -X POST http://localhost:40403/api/prepare-deploy \
  -H 'Content-Type: application/json' \
  -d '{"deployer": "04abc...", "timestamp": 1700000000000, "nameQty": 2}'
```

```json
{
  "names": ["a1b2c3...", "d4e5f6..."],
  "seqNumber": 20
}
```

The `names` array contains hex-encoded unforgeable names that will be produced by the deployer at the given timestamp. Clients can use these to pre-sign contracts that create private channels before deploying.

| Status | Condition |
|--------|-----------|
| `200` | Sequence number and (optionally) names returned |
| `400` | Malformed body or invalid deployer hex (`invalid_request_body`, `invalid_hash`) |
| `500` | Node-side failure (`runtime_error`) |

---

### Exploratory Deploy

Execute Rholang code in read-only mode. No block is created, no phlogiston is consumed. **Read-only nodes only.**

#### `POST /api/explore-deploy`

Execute against the last finalized block (LFB) post-state. Unfinalized DAG-tip state is not visible.

**Request body:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `term` | string | yes | Rholang source code |

```bash
curl -X POST http://localhost:40453/api/explore-deploy \
  -H 'Content-Type: application/json' \
  -d '{"term": "new ret in { ret!(42) }"}'
```

| Status | Condition |
|--------|-----------|
| `200` | Execution succeeded; returns channel data |
| `400` | Malformed body, invalid Rholang, or node is not read-only (`invalid_request_body`, `rholang_bad_term`, `readonly_node_required`) |
| `422` | Term valid but execution failed (`rholang_execution_error`, `out_of_phlogistons`, `user_abort`) |
| `500` | Node-side failure (`interpreter_internal_error`) |
| `502` | External service failure (`external_service_error`) |
| `503` | Exploratory capacity is occupied (`observer_busy`); carries `Retry-After` |
| `504` | Execution exceeded the configured deadline (`exploratory_timeout`) |

The phlogiston limit is the authoritative execution bound. The wall-clock deadline is best-effort because timeout observation requires interpreter execution to yield, and capacity remains occupied until cancelled work terminates.

#### `POST /api/explore-deploy-by-block-hash`

Execute against a specific block's post-state.

**Request body:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `term` | string | yes | Rholang source code |
| `blockHash` | string | yes | Block hash to execute against (must not be empty) |
| `usePreStateHash` | bool | no | Use pre-state instead of post-state (default: false) |

```bash
curl -X POST http://localhost:40453/api/explore-deploy-by-block-hash \
  -H 'Content-Type: application/json' \
  -d '{"term": "new ret in { ret!(42) }", "blockHash": "3bfdf56f...", "usePreStateHash": false}'
```

| Status | Condition |
|--------|-----------|
| `200` | Execution succeeded; returns channel data |
| `400` | Malformed body, invalid Rholang, invalid block hash, or node is not read-only (`invalid_request_body`, `rholang_bad_term`, `invalid_hash`, `readonly_node_required`) |
| `404` | Specified block not found (`block_not_found`) |
| `422` | Term valid but execution failed (`rholang_execution_error`, `out_of_phlogistons`, `user_abort`) |
| `500` | Node-side failure (`interpreter_internal_error`) |
| `502` | External service failure (`external_service_error`) |
| `503` | Exploratory capacity is occupied (`observer_busy`); carries `Retry-After` |
| `504` | Execution exceeded the configured deadline (`exploratory_timeout`) |

---

### Data Query

#### `POST /api/data-at-name-by-block-hash`

Query data at a Rholang name in a specific block's post-state.

**Request body:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `par` | object | yes | Rholang `Par` (protobuf JSON) identifying the channel |
| `blockHash` | string | yes | Block hash to query against |
| `usePreStateHash` | bool | no | Use pre-state instead of post-state (default: false) |

```bash
curl -X POST http://localhost:40453/api/data-at-name-by-block-hash \
  -H 'Content-Type: application/json' \
  -d '{
    "par": {"unforgeables": [{"g_private_body": {"id": "..."}}]},
    "blockHash": "3bfdf56f...",
    "usePreStateHash": false
  }'
```

| Status | Condition |
|--------|-----------|
| `200` | Data found |
| `400` | Malformed body or invalid block hash (`invalid_request_body`, `invalid_hash`) |
| `404` | Specified block not found (`block_not_found`) |
| `500` | Node-side failure (`interpreter_internal_error`) |

---

### Reporting

Block execution trace. **Read-only nodes only** (requires block report replay). Available when the node is started with reporting enabled.

#### `GET /reporting/trace`

Full per-deploy execution trace for a block — every produce, consume, and COMM event for every user deploy and system deploy. Use for debugging orphan sends and auditing contract execution.

| Parameter | Location | Required | Description |
|-----------|----------|----------|-------------|
| `blockHash` | query | yes | Full 64-char hex block hash |
| `forceReplay` | query | no | If `true`, discard any cached trace and re-replay the block (default: `false`) |

```bash
curl "http://localhost:40453/reporting/trace?blockHash=3bfdf56f...&forceReplay=false"
```

```json
{
  "report": { "deploys": [ ... ], "systemDeploys": [ ... ] }
}
```

**Offered deploys (protocol 6, DR-70).** The report of an offered deploy has the same event batches as the report of a dev user deploy: the batch of the registry bootstrap for the first deploy (`REPORT_PHASE_UNSPECIFIED`), then the events of the user run (`REPORT_PHASE_USER`). It has no `PRECHARGE` or `REFUND` batch, because the cost settlement replaces them. Its entry also carries `offered`:

- `nativeCostEvidence`: the committed native cost evidence of the deploy, in base64.
- `receipt`: the settlement receipt. It has the same data as the receipt route `/deploy/offered/{deploy_id}/receipt`. The REV amounts are decimal strings, because they are 128-bit values.

The node extracts transfers from the USER batch, as for any user deploy. System deploys keep their event batches.

```json
{
  "deployInfo": { "sig": "…", "cost": 412, "errored": false, … },
  "report": [ { "phase": "REPORT_PHASE_USER", "events": [ … ] } ],
  "offered": {
    "nativeCostEvidence": "AAEC…",
    "receipt": {
      "phloUsed": 412,
      "freshPhlo": 412,
      "retainedPhlo": 0,
      "phloLimit": 100000,
      "phloPrice": 1,
      "feeRev": "1",
      "revSpent": "413",
      "revCeiling": "100001",
      "purses": [
        { "address": "AbC…", "resourceRev": "412", "feeRev": "1", "postBalance": 9999587 }
      ]
    }
  }
}
```

| Status | Condition |
|--------|-----------|
| `200` | Trace report returned |
| `400` | `blockHash` query parameter is missing, empty, or contains non-hex characters; or node is not read-only (`invalid_query_parameter`, `invalid_hash`, `readonly_node_required`) |
| `404` | Specified block not found (`block_not_found`) |
| `500` | Node-side failure (`replay_failure`, `kv_store_error`, `lock_error`, `unknown_error`) |

---

### High-Level Query Endpoints

Convenience endpoints wrapping exploratory deploy or genesis config. Unless noted, **read-only nodes only**.

#### `GET /api/balance/{address}`

Vault balance for a wallet address.

| Parameter | Location | Required | Description |
|-----------|----------|----------|-------------|
| `address` | path | yes | REV wallet address (Base58-encoded, starts with `1111`) |
| `block_hash` | query | no | Block hash to query against (defaults to LFB) |

```bash
curl http://localhost:40453/api/balance/11112BpS5mG8...
```

```json
{"address": "11112BpS5mG8...", "balance": 1000000, "blockNumber": 42, "blockHash": "3bfdf56f..."}
```

| Status | Condition |
|--------|-----------|
| `200` | Balance returned |
| `400` | Invalid block hash or node is not read-only (`invalid_hash`, `readonly_node_required`) |
| `404` | Specified block not found (`block_not_found`) |
| `422` | Exploratory deploy execution failed (`rholang_execution_error`, `out_of_phlogistons`) |
| `500` | Node-side failure (`interpreter_internal_error`) |

#### `GET /api/registry/{uri}`

Registry lookup. Returns the registered data unwrapped from the registry's `(true, data)` tuple.

| Parameter | Location | Required | Description |
|-----------|----------|----------|-------------|
| `uri` | path | yes | Registry URI (e.g. `rho:id:abc...`) |
| `block_hash` | query | no | Block hash to query against (defaults to LFB) |

```bash
curl http://localhost:40453/api/registry/rho:id:abc...
```

```json
{"uri": "rho:id:abc...", "data": [<RhoExpr>], "blockNumber": 42, "blockHash": "3bfdf56f..."}
```

| Status | Condition |
|--------|-----------|
| `200` | Data returned |
| `400` | Invalid block hash or node is not read-only (`invalid_hash`, `readonly_node_required`) |
| `404` | Specified block not found (`block_not_found`) |
| `422` | Exploratory deploy execution failed (`rholang_execution_error`, `out_of_phlogistons`) |
| `500` | Node-side failure (`interpreter_internal_error`) |

#### `GET /api/validators`

Active validator set from the PoS contract (`getBonds`).

| Parameter | Location | Required | Description |
|-----------|----------|----------|-------------|
| `block_hash` | query | no | Block hash to query against (defaults to LFB) |

```bash
curl http://localhost:40453/api/validators
```

```json
{
  "validators": [
    {"publicKey": "04837a4cff...", "stake": 1000},
    {"publicKey": "04fa70d7be...", "stake": 1000},
    {"publicKey": "0457febafc...", "stake": 1000}
  ],
  "totalStake": 3000,
  "blockNumber": 30,
  "blockHash": "6bb5892d..."
}
```

| Status | Condition |
|--------|-----------|
| `200` | Validator set returned |
| `400` | Invalid block hash or node is not read-only (`invalid_hash`, `readonly_node_required`) |
| `404` | Specified block not found (`block_not_found`) |
| `422` | Exploratory deploy execution failed (`rholang_execution_error`) |
| `500` | Node-side failure (`interpreter_internal_error`) |

#### `GET /api/validator/{pubkey}`

Status of a specific validator — whether bonded and current stake. Queries the PoS contract (`getBonds`).

| Parameter | Location | Required | Description |
|-----------|----------|----------|-------------|
| `pubkey` | path | yes | Validator secp256k1 public key as a 65-byte uncompressed hex string |
| `block_hash` | query | no | Block hash to query against (defaults to LFB) |

```bash
curl http://localhost:40453/api/validator/04837a4cff...
```

Bonded validator:
```json
{"publicKey": "04837a4cff...", "isBonded": true, "stake": 1000, "blockNumber": 4, "blockHash": "7701282c..."}
```

Unknown key:
```json
{"publicKey": "04aaaa...", "isBonded": false, "stake": null, "blockNumber": 4, "blockHash": "7701282c..."}
```

| Status | Condition |
|--------|-----------|
| `200` | Status returned |
| `400` | Invalid public key or block hash (`illegal_argument`, `invalid_hash`, `readonly_node_required`) |
| `404` | Specified block not found (`block_not_found`) |
| `422` | Exploratory deploy execution failed (`rholang_execution_error`) |
| `500` | Node-side failure (`interpreter_internal_error`) |

#### `GET /api/bond-status/{pubkey}`

Check if a public key is bonded. Uses `BlockAPI::bond_status` directly — **available on all node types**, no exploratory deploy required.

| Parameter | Location | Required | Description |
|-----------|----------|----------|-------------|
| `pubkey` | path | yes | Validator secp256k1 public key as a 65-byte uncompressed hex string |

```bash
curl http://localhost:40403/api/bond-status/04837a4cff...
```

```json
{"publicKey": "04837a4cff...", "isBonded": true}
```

| Status | Condition |
|--------|-----------|
| `200` | Status returned |
| `400` | Invalid public key format (`illegal_argument`) |
| `500` | Node-side failure (`runtime_error`) |

#### `GET /api/epoch`

Current epoch info. Uses cached genesis config — no exploratory deploy required. **Available on all node types.**

| Parameter | Location | Required | Description |
|-----------|----------|----------|-------------|
| `block_hash` | query | no | Block hash to derive epoch from (defaults to LFB) |

```bash
curl http://localhost:40403/api/epoch
```

```json
{
  "currentEpoch": 3,
  "epochLength": 10,
  "quarantineLength": 10,
  "blocksUntilNextEpoch": 10,
  "lastFinalizedBlockNumber": 30,
  "blockHash": "6bb5892d..."
}
```

| Status | Condition |
|--------|-----------|
| `200` | Epoch info returned |
| `400` | Invalid block hash (`invalid_hash`) |
| `404` | Specified block not found (`block_not_found`) |
| `500` | Node-side failure (`runtime_error`) |

#### `GET /api/epoch/rewards`

Current epoch rewards from the PoS contract (`getCurrentEpochRewards`). Returns a map of validator public keys to their accumulated rewards.

If the node is desynced from the network, the Rholang execution may fail with an arithmetic error (e.g. overflow in reward calculation). This returns `422 rholang_execution_error`, not `400`.

| Parameter | Location | Required | Description |
|-----------|----------|----------|-------------|
| `block_hash` | query | no | Block hash to query against (defaults to LFB) |

```bash
curl http://localhost:40453/api/epoch/rewards
```

```json
{
  "rewards": {
    "ExprMap": {
      "data": {
        "04837a4cff...": {"ExprInt": {"data": 0}},
        "04fa70d7be...": {"ExprInt": {"data": 0}}
      }
    }
  },
  "blockNumber": 3,
  "blockHash": "2ee3df7f..."
}
```

| Status | Condition |
|--------|-----------|
| `200` | Rewards returned |
| `400` | Invalid block hash or node is not read-only (`invalid_hash`, `readonly_node_required`) |
| `404` | Specified block not found (`block_not_found`) |
| `422` | Execution failed, e.g. arithmetic overflow due to node desync (`rholang_execution_error`) |
| `500` | Node-side failure (`interpreter_internal_error`) |

#### `POST /api/estimate-cost`

Estimate phlogiston cost of Rholang code without committing. Runs exploratory deploy and returns only the cost.

The estimate runs without owner funding, so it reports the whole run, including the work that the owners of stored processes pay. A caller's signed limit needs to cover only the caller's own part (DR-135 item 13).

**Request body:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `term` | string | yes | Rholang source code |
| `deployer` | string | no | Hex-encoded 65-byte uncompressed secp256k1 public key (`04`-prefixed). For identity-dependent terms such as REV vault transfers, pass the deployer public key; without it the term executes under an ephemeral identity and the returned cost can be significantly lower than the real deploy cost. |

| Parameter | Location | Required | Description |
|-----------|----------|----------|-------------|
| `block_hash` | query | no | Block hash to execute against (defaults to LFB) |

```bash
curl -X POST http://localhost:40453/api/estimate-cost \
  -H 'Content-Type: application/json' \
  -d '{"term": "new ret in { ret!(42) }", "deployer": "04..."}'
```

```json
{"cost": 204, "blockNumber": 3, "blockHash": "2ee3df7f...", "deployerIdentity": "provided"}
```

**Response fields:**

| Field | Type | Description |
|-------|------|-------------|
| `cost` | number | Estimated phlogiston (gas) cost |
| `blockNumber` | number | Block number the estimate ran against |
| `blockHash` | string | Block hash the estimate ran against |
| `deployerIdentity` | string | Which identity produced the estimate: `"provided"` (the caller-supplied `deployer` key was used) or `"ephemeral"` (no `deployer` was passed, so the term ran under a process-wide random key). `"ephemeral"` may significantly underestimate the real deploy cost for identity-dependent terms |

| Status | Condition |
|--------|-----------|
| `200` | Cost estimated |
 | `400` | Malformed body, invalid Rholang, invalid block hash, invalid deployer key, or node is not read-only (`invalid_request_body`, `illegal_argument`, `rholang_bad_term`, `invalid_hash`, `readonly_node_required`) |
| `404` | Specified block not found (`block_not_found`) |
| `422` | Term valid but execution failed (`rholang_execution_error`, `out_of_phlogistons`) |
| `500` | Node-side failure (`interpreter_internal_error`) |

---

### Admin API (port 40405)

#### `POST /api/propose`

Propose a new block containing pending deploys. Validator nodes only.

**Parameters:** None

```bash
curl -X POST http://localhost:40405/api/propose
```

| Status | Condition |
|--------|-----------|
| `200` | Propose result message (success block hash) |
| `400` | Read-only node (`readonly_node_required`) |
| `409` | No new deploys to propose (`no_new_deploys`) |
| `500` | Node-side propose failure (`unknown_error`, `replay_failure`) |

---

## gRPC API

### DeployService (port 40401)

| Method | Request | Response | Description |
|--------|---------|----------|-------------|
| `doDeploy` | `DeployDataProto` | `DeployResponse` | Submit a signed deploy to a validator in the last finalized block's committee. Validates phlo price, shard ID, signature, expiration. Triggers auto-propose if enabled. A proto whose `authorizationV61.formatVersion` is the offered-funded format goes to offered admission (as `POST /api/deploy/offered`); a protocol-6 node refuses a body-only deploy (DR-66) |
| `getBlock` | `BlockQuery` | `BlockResponse` | Get block by hash. Returns `BlockInfo` (header + deploys). Transfers enriched on readonly |
| `getBlocks` | `BlocksQuery` | `stream BlockInfoResponse` | Get recent blocks by depth. Streaming. Returns `LightBlockInfo` (headers only) |
| `showMainChain` | `BlocksQuery` | `stream BlockInfoResponse` | Walk the main chain path from tip. Streaming. Returns `LightBlockInfo` |
| `getBlocksByHeights` | `BlocksQueryByHeight` | `stream BlockInfoResponse` | Get blocks in a height range. Streaming. Clamped by `api_max_blocks_limit` |
| `lastFinalizedBlock` | `LastFinalizedBlockQuery` | `LastFinalizedBlockResponse` | Get the last finalized block. Returns full `BlockInfo` with deploys |
| `isFinalized` | `IsFinalizedQuery` | `IsFinalizedResponse` | Check if a block is finalized. Returns bool |
| `findDeploy` | `FindDeployQuery` | `FindDeployResponse` | Find block containing a deploy. Retries up to 80x with 100ms intervals while deploy propagates through DAG |
| `getDataAtName` | `DataAtNameByBlockQuery` | `RhoDataResponse` | Query data at a Rholang name in a specific block's post-state. Takes `Par` + block hash + `usePreStateHash` |
| `listenForContinuationAtName` | `ContinuationAtNameQuery` | `ContinuationAtNameResponse` | Find processes waiting to receive on given channel names. Returns matching patterns and continuation bodies |
| `exploratoryDeploy` | `ExploratoryDeployQuery` | `ExploratoryDeployResponse` | Execute Rholang read-only. No block created, no phlo consumed. Returns result `Par`s, block context, and cost. Readonly only |
| `bondStatus` | `BondStatusQuery` | `BondStatusResponse` | Check if a public key is bonded. Validates that the key is a 65-byte uncompressed secp256k1 point; returns error on invalid input. HTTP: `GET /api/bond-status/{pubkey}` |
| `previewPrivateNames` | `PrivateNamePreviewQuery` | `PrivateNamePreviewResponse` | Generate unforgeable names from deployer key + timestamp. Allows clients to compute signatures over names before deploying. Max 1024 names |
| `getEventByHash` | `ReportQuery` | `EventInfoResponse` | Get full block execution trace — every COMM/produce/consume event per deploy and system deploy. Takes block hash + `forceReplay` flag. Used for debugging and auditing. An offered deploy reports its user-run events in a USER batch, and its `DeployInfoWithEventData.offered` also holds the native cost evidence and the settlement receipt (DR-70) |
| `visualizeDag` | `VisualizeDagQuery` | `stream VisualizeBlocksResponse` | DAG visualization in DOT format. Takes depth + startBlockNumber + showJustificationLines |
| `machineVerifiableDag` | `MachineVerifyQuery` | `MachineVerifyResponse` | Machine-parseable DAG representation |
| `status` | `google.protobuf.Empty` | `StatusResponse` | Node status — version, address, peers, network, native token metadata, LFB number, isValidator, isReadOnly, isReady, epoch |
| `getPendingDeploys` | `PendingDeploysQuery` | `PendingDeploysResponse` | Bulk list of the node-local pending-deploy queue (deploy_storage + rejected-recovery buffer), optionally filtered by `deployerPubkey`. Capped at 1000 entries; `totalAvailable` reports the pre-cap count. Observers always answer empty. HTTP: `GET /api/pending-deploys` |

### ProposeService (port 40402)

| Method | Request | Response | Description |
|--------|---------|----------|-------------|
| `propose` | `ProposeQuery` | `ProposeResponse` | Propose a new block. `isAsync`: if true returns immediately, otherwise waits for result. Validator only |
| `proposeResult` | `ProposeResultQuery` | `ProposeResultResponse` | Get latest propose result. Blocks until current proposal completes if one is in progress |

Proto definitions: `models/src/main/protobuf/DeployServiceV1.proto`, `ProposeServiceV1.proto`, `DeployServiceCommon.proto`.

---

## Transfer Behavior

| Node type | `transfers` on deploy responses | `TransfersAvailable` WebSocket event |
|-----------|--------------------------------|--------------------------------------|
| **Readonly** | Populated array | Emitted after block report cache warming |
| **Validator** | `null` (omitted) | Not emitted |

`null` means transfers cannot be extracted on this node type (block replay requires readonly mode). An empty array `[]` means the deploy had no transfers.

An offered deploy (protocol 6) reports the events of its user run in a USER batch, so the node extracts its transfers as for any user deploy (DR-70). The cost settlement is not a transfer of the deploy and does not appear in `transfers`.

---

## View Parameters

| Endpoint type | Default | Opt-in |
|---|---|---|
| Single item (`/block/{hash}`, `/last-finalized-block`, `/deploy/{id}`) | `full` | `?view=summary` |
| Lists (`/blocks`, `/blocks/{depth}`, `/blocks/{start}/{end}`) | `summary` | `?view=full` |

---

## WebSocket Events

See [websocket-events.md](websocket-events.md) for the full event catalog, payload format, and startup replay behavior.

```bash
wscat -c ws://localhost:40403/ws/events
```
