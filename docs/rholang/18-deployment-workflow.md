# Deployment Workflow

How to deploy Rholang contracts to the F1R3FLY Rust shard.

## Deploy Lifecycle

1. **Write** -- create a `.rho` file
2. **Deploy** -- submit the contract with phlogiston limit and deployer key
3. **Propose** -- validator includes the deploy in a block
4. **Finalize** -- block reaches finality through consensus
5. **Query** -- read results via exploratory deploy or data-at-name

## CLI Commands

These are subcommands of the `node` binary, which talks to a **running** node
over gRPC. (`rholang-cli` is a different tool: it evaluates a `.rho` file
locally and takes no subcommands — see [`rholang/README.md`](../../rholang/README.md).)

### Deploy a Contract

```bash
node deploy \
  --phlo-limit 100000 \
  --phlo-price 1 \
  --valid-after-block <N> \
  --private-key $PRIVATE_KEY \
  --shard-id root \
  contract.rho
```

Parameters:
- positional: path to the `.rho` file
- `--private-key` (or `--private-key-path`): deployer's key
- `--phlo-limit`: maximum phlogiston to spend
- `--phlo-price`: price per phlogiston unit (typically 1)
- `--valid-after-block`: the deploy is valid for `deploy-lifespan` blocks after this height
- `--shard-id`: target shard

Send the deploy to a **bonded validator**. A node that cannot propose accepts
the deploy and never includes it: the queue is node-local and deploys are not
gossiped.

### Propose a Block

```bash
node propose
```

With the heartbeat proposer enabled — the default — validators propose on their
own and this is only needed on a shard that has it switched off.

### Check Finalization

```bash
node is-finalized --hash $BLOCK_HASH
```

Returns whether the block is finalized. To follow a single deploy rather than a
block, prefer `GET /api/deploy-finalization-status/{sig}`, which reports the
canonical verdict for that deploy.

### Exploratory Deploy

Execute a read-only contract without creating a block. Useful for querying
state. Only a **read-only** node serves this; any other node answers
`400 readonly_node_required`.

```bash
curl -X POST http://localhost:40453/api/explore-deploy \
  -H 'Content-Type: application/json' \
  -d '{"term": "new ret in { ret!(42) }"}'
```

## HTTP API

The node exposes an HTTP API (default port 40403).

### Deploy

The deploy must be signed, and the deploy fields are nested under `data`:

```bash
curl -X POST http://localhost:40413/api/deploy \
  -H 'Content-Type: application/json' \
  -d '{
    "data": {
      "term": "new stdout(`rho:io:stdout`) in { stdout!(\"hello\") }",
      "timestamp": 1700000000000,
      "phloPrice": 1,
      "phloLimit": 100000,
      "validAfterBlockNumber": 0,
      "shardId": "root"
    },
    "deployer": "04abc...",
    "signature": "3044...",
    "sigAlgorithm": "secp256k1"
  }'
```

Signing by hand is awkward; `node deploy` or a client library is the usual
route. See [the API reference](../node/api-reference.md) for the full schema
and status codes. Post to a bonded validator's HTTP port, not to a bootstrap or
read-only node.

### Get Deploy Status

```bash
curl http://localhost:40403/api/deploy/$DEPLOY_ID
curl http://localhost:40403/api/deploy/$DEPLOY_ID?view=summary
```

**Views:**
- **`full`** (default): all fields — `deployId`, `blockHash`, `blockNumber`, `timestamp`, `cost`, `errored`, `isFinalized`, `deployer`, `term`, `systemDeployError`, `phloPrice`, `phloLimit`, `sigAlgorithm`, `validAfterBlockNumber`, `transfers`
- **`summary`**: core fields only — `deployId`, `blockHash`, `blockNumber`, `timestamp`, `cost`, `errored`, `isFinalized`. For lightweight polling.

**Transfers:** The `transfers` field is `null` on validator nodes (block replay unavailable) and a populated array on readonly nodes. `null` means transfers can't be extracted on this node type — query a readonly node for transfer details.

### Exploratory Deploy

```bash
curl -X POST http://localhost:40403/api/explore-deploy \
  -H 'Content-Type: application/json' \
  -d '{"term": "new ret(`rho:io:stdout`) in { ret!(42) }"}'
```

Response includes the phlogiston cost.

### Get Data at Name by Block Hash

```bash
curl -X POST http://localhost:40403/api/data-at-name-by-block-hash \
  -H 'Content-Type: application/json' \
  -d '{"par": {"unforgeables": [{"g_private_body": {"id": "..."}}]}, "blockHash": "abc123...", "usePreStateHash": false}'
```

## gRPC API

The node exposes gRPC services for programmatic access:

- `DeployService.doDeploy` -- submit a deploy
- `DeployService.getBlock` -- get block by hash
- `DeployService.findDeploy` -- find deploy by ID
- `ProposeService.propose` -- propose a block
- `DeployService.getDataAtName` -- query data on a channel

Python client (`pyf1r3fly`) wraps these for integration testing.

## WebSocket Events

The node streams block lifecycle events via WebSocket:

```
ws://localhost:40403/ws/events
```

Event types:
- `block-created` -- new block proposed
- `block-added` -- block added to DAG
- `block-finalised` -- block reached finality
- Genesis ceremony events
- Node lifecycle events

Events published during startup are buffered and replayed when clients connect.

## Deploy Result

After a deploy is included in a finalized block, the result contains:

| Field | Description |
|-------|-------------|
| `cost` | Phlogiston consumed |
| `errored` | Whether the deploy produced an error |
| `systemDeployError` | System-level error message (if any) |
| `blockNumber` | Block containing the deploy |

## Common Patterns

### Deploy and Wait for Result

```bash
# 1. Deploy (to a bonded validator)
DEPLOY_ID=$(node deploy --phlo-limit 100000 --phlo-price 1 \
  --valid-after-block $VABN --private-key $KEY --shard-id root contract.rho)

# 2. Only if the heartbeat proposer is disabled on this shard
node propose

# 3. Check result
curl http://localhost:40413/api/deploy/$DEPLOY_ID?view=summary
```

### Query State After Deploy

Use exploratory deploy to read state without creating a new block:

```rho
// query.rho -- read the registry entry set by a previous deploy
new lookup(`rho:registry:lookup`), stdout(`rho:io:stdout`) in {
  new ret in {
    lookup!(`rho:id:my_service_uri`, *ret) |
    for (service <- ret) {
      new result in {
        service!({"action": "status"}, *result) |
        for (@status <- result) {
          stdout!(status)
        }
      }
    }
  }
}
```

### Generate Keys

```bash
cargo run --bin rholang-cli -- generate-key-pair --save
```

This creates a keypair file that can be used for deploys.

## Phlogiston Tips

- Start with `--phlo-limit 100000` for simple contracts
- Use `1000000` for complex contracts (registry operations, vault transfers)
- Check the deploy result's `cost` field to see actual consumption
- If you get `OutOfPhlogistonsError`, increase the limit
- See [Cost Model](13-cost-model.md) for detailed cost tables
