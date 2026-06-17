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
  "isValidator": false,
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
| `minPhloPrice` | int | Minimum phlogiston price for deploys |
| `peerList` | array | Detailed peer info with connection status |
| `nativeTokenName` | string | Full token name from genesis |
| `nativeTokenSymbol` | string | Token ticker symbol |
| `nativeTokenDecimals` | int | Decimal places (dust per token = 10^decimals) |
| `lastFinalizedBlockNumber` | int | LFB block number. `-1` if casper not yet initialized |
| `isValidator` | bool | `true` if the node can propose blocks |
| `isReadOnly` | bool | `true` if running in read-only mode |
| `isReady` | bool | `true` after engine enters Running state |
| `currentEpoch` | int | `lastFinalizedBlockNumber / epochLength` |
| `epochLength` | int | Blocks per epoch, from genesis configuration |

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

**Request body:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `data.term` | string | yes | Rholang source code |
| `data.timestamp` | int | yes | Deploy timestamp (ms since epoch) |
| `data.phloPrice` | int | yes | Phlogiston price per unit |
| `data.phloLimit` | int | yes | Maximum phlogiston to consume |
| `data.validAfterBlockNumber` | int | yes | Deploy valid after this block number |
| `data.shardId` | string | yes | Target shard (e.g. `"root"`) |
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
| `400` | Malformed body or invalid field value: read-only node, wrong shard ID, forbidden key, phlo price below minimum, deploy expired (`invalid_request_body`, `illegal_argument`, `rholang_bad_term`) |
| `422` | Term valid but execution failed (`rholang_execution_error`, `out_of_phlogistons`, `user_abort`) |
| `500` | Node-side failure (`interpreter_internal_error`, `replay_failure`, `signing_error`) |
| `502` | Peer communication failure (`comm_error`) |

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

Execute against the latest block state.

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

#### `POST /api/explore-deploy-by-block-hash`

Execute against a specific block's post-state.

**Request body:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `term` | string | yes | Rholang source code |
| `blockHash` | string | yes | Block hash to execute against |
| `usePreStateHash` | bool | no | Use pre-state instead of post-state (default: false). **Note:** currently ignored by the server; always uses post-state |

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

**Request body:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `term` | string | yes | Rholang source code |

| Parameter | Location | Required | Description |
|-----------|----------|----------|-------------|
| `block_hash` | query | no | Block hash to execute against (defaults to LFB) |

```bash
curl -X POST http://localhost:40453/api/estimate-cost \
  -H 'Content-Type: application/json' \
  -d '{"term": "new ret in { ret!(42) }"}'
```

```json
{"cost": 204, "blockNumber": 3, "blockHash": "2ee3df7f..."}
```

| Status | Condition |
|--------|-----------|
| `200` | Cost estimated |
| `400` | Malformed body, invalid Rholang, invalid block hash, or node is not read-only (`invalid_request_body`, `rholang_bad_term`, `invalid_hash`, `readonly_node_required`) |
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
| `doDeploy` | `DeployDataProto` | `DeployResponse` | Submit a signed deploy. Validates phlo price, shard ID, signature, expiration. Triggers auto-propose if enabled |
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
| `getEventByHash` | `ReportQuery` | `EventInfoResponse` | Get full block execution trace — every COMM/produce/consume event per deploy and system deploy. Takes block hash + `forceReplay` flag. Used for debugging and auditing |
| `visualizeDag` | `VisualizeDagQuery` | `stream VisualizeBlocksResponse` | DAG visualization in DOT format. Takes depth + startBlockNumber + showJustificationLines |
| `machineVerifiableDag` | `MachineVerifyQuery` | `MachineVerifyResponse` | Machine-parseable DAG representation |
| `status` | `google.protobuf.Empty` | `StatusResponse` | Node status — version, address, peers, network, native token metadata, LFB number, isValidator, isReadOnly, isReady, epoch |

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
