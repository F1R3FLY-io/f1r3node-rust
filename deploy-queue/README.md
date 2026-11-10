# deploy-queue

A per-validator deploy queue. It runs as a separate process next to one validator, from the same image
(`/opt/docker/bin/deploy-queue`). Clients send deploys to the queue in place of the validator. The queue holds
each deploy durably and releases it at the pace the shard settles deploys. When clients offer more than the shard
can settle, the queue holds or refuses the excess, so the shard does not receive more work than it can finalize.

## Lifecycle of a deploy

| Queue state | Node status | Meaning |
|---|---|---|
| `queued` | `Pending` | Accepted. The deploy waits in the queue. |
| `submitted` | `Pending` | Released. The deploy is in the node's pool. |
| `in-block` | `Pending` | A block carries the deploy. The queue waits for the node's verdict. |
| `finalized`, `failed`, `expired` | same | The node's verdict, or `expired` when the window closed in the queue. |
| `rejected` | `Failed` | The node refused the deploy when the queue released it. |

Terminal states never change. The queue drops a terminal record one deploy lifespan after its verdict.

## Admission

The queue refuses a deploy with the node's own error when its signature, shard id, phlo price, expiration,
validity window or Rholang term is not valid, or when the queue already holds it.

The queue then checks capacity. It accepts a deploy only when all deploys ahead of it can settle before its
validity window closes, with one citable depth to spare. The settle rate is the lower of two measurements: the
rate at which released deploys settled over recent heights, and the release window divided by the median settle
delay. A deploy that does not fit gets `deploy_queue_full` with a retry-after: the time until a deploy signed now
would fit. The client signs the deploy again with a current `validAfterBlockNumber` before it retries.

| Refusal | HTTP | gRPC |
|---|---|---|
| Invalid, bad term, expired, duplicate | 400, 409 | `ServiceError`, as the node |
| `deploy_queue_full` | 503 + `Retry-After` | `RESOURCE_EXHAUSTED` + `retry-after` metadata |
| `node_unavailable` | 503 + `Retry-After` | `UNAVAILABLE` + `retry-after` metadata |

## Release

Deployers take turns in the ready list. Each deployer's deploys keep their intake order.

The release window limits how many released deploys are not settled yet, in the node's pool or in a block. The
queue judges the shard once per round, which is half the citable depth (`maxParentDepth`) in heights. A verdict is
late when it came more than one round after release. At the end of each round:

- Most timed verdicts were late: the window halves.
- No verdict came, deploys waited for room, and an older release is still not settled: a stall, so the window
  halves.
- Deploys waited for room and at least one verdict came in time: the window grows by one.

A deploy that lost a merge (`rejection-count` above zero) settles late because of a conflict with other deploys,
not a lack of capacity. Its verdict counts as neither late nor on time, but it rules out a stall. The window
starts at one deploy per height of a round, and its ceiling is two full blocks (`2 × maxUserDeploysPerBlock`).

Release stops while the queue is not connected, while the node's finality is more than one citable depth behind
its tip, and for a backoff after the node could not take a deploy (1 s, doubling up to 30 s). The queue does not
release a deploy that has less window left than the median inclusion delay plus one block. That deploy closes in
the queue, and the queue later records the node's answer for it, or `expired`. A released deploy that no block
carries within one citable depth goes to the node again.

## Node link

The queue follows the node's event stream (`/ws/events`) and does not poll. It uses `block-added`,
`block-finalised`, `deploy-verdicts` and `entered-running-state`. On `stream-lagged` it reads every released
deploy's status again. The node must publish `deploy-verdicts` and report `latestBlockNumber`, `deployLifespan`,
`maxUserDeploysPerBlock` and `maxParentDepth` in `/api/status`.

## Client API

The queue serves the node's client ports, so a client uses it as it would use the node.

- gRPC (`0.0.0.0:40401`): `casper.v1.DeployService`, where the queue answers `doDeploy` and the status of deploys
  it holds and sends all other calls to the node. Also `deployqueue.v1.DeployQueue.watchDeploys`, which streams
  each state change of the named deploys, and server reflection.
- HTTP (`0.0.0.0:40403`): `POST /api/deploy` and `/api/v1/deploy`, `GET /api/deploy-finalization-status/{sig}`
  (the node's body plus `queue_state` and `detail`), `GET /ws/deploys?sigs=<hex>,<hex>`, `GET /ws/events`,
  `GET /queue/status` (counts per state, release window and cuts, measured delays and rates) and
  `GET /queue/metrics` (Prometheus). Other paths go to the node.

## Running

```bash
deploy-queue --node-grpc http://validator:40401 --node-http http://validator:40403 \
  --data-dir /var/lib/deploy-queue
```

`--grpc-listen` and `--http-listen` change the client addresses. `RUST_LOG` sets the log filter. The queue has no
configuration file: it reads the shard's values from its node. It writes each accepted deploy to its LMDB store
before it answers, so a restart keeps all held deploys. The release window and the rates start again.

## Tests

```bash
cargo test --release -p deploy-queue
cargo clippy --release -p deploy-queue --all-targets
```

`tests/queue_against_a_node.rs` runs the engine, the event link and the node client against a stand-in node.
