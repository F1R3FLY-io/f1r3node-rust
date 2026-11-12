# deploy-queue

A per-validator deploy queue. It runs as a separate process next to one validator, from the same image
(`/opt/docker/bin/deploy-queue`). Clients send deploys to the queue in place of the validator. The queue holds
each deploy durably and releases it at the pace the shard settles deploys. When clients offer more than the shard
can settle, the queue holds or refuses the excess, so the shard does not receive more work than it can finalize.

The states a client reads, the refusals and the client endpoints are in
[Deploy queue client API](../docs/node/deploy-queue.md). This file describes how the queue decides.

## Admission

The queue refuses a deploy with the node's own error when its signature, shard id, phlo price, expiration,
validity window or Rholang term is not valid, when its `validAfterBlockNumber` is more than `maxParentDepth`
above the tip, or when the queue already holds it.

The queue then checks capacity. Release takes the earliest deadline (last includable block) first, so a deploy
waits behind every released deploy and every queued deploy with an earlier deadline. The queue accepts a deploy
only when it settles before its validity window closes, with one citable depth to spare, and no queued deploy
with a later deadline that settles in time now would stop settling in time. An accepted deploy therefore never
expires in the queue while the settle rate holds. The settle rate is the lower of two measurements: the rate at
which released deploys settled over recent heights, and the release window divided by the median settle delay.
A deploy that does not fit gets `deploy_queue_full` with a retry-after: the time until a deploy signed now would
fit.

One deployer may hold at most an equal share of the queue's capacity that keeps room for one more deployer:
`capacity / (other deployers + 2)`, and never less than one deploy. The capacity is what settles at the settle
rate in a fresh window, less the citable depth. A deployer over its share gets `deploy_queue_full` too.

## Release

The ready list is ordered by deadline. The release window limits how many released deploys are not settled yet, in the node's pool or in a block. The
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
its tip, and for a backoff after the node could not take a deploy (1 s, doubling up to 30 s). The node cannot
take a deploy while its validator is not bonded (`503 validator_not_bonded`); the deploy waits in the queue and
expires there if the validator is not bonded before its window closes. The queue does not
release a deploy that has less window left than the median inclusion delay plus one block. That deploy closes in
the queue, and the queue later records the node's answer for it, or `expired`. A released deploy that no block
carries within one citable depth goes to the node again.

## Node link

The queue follows the node's event stream (`/ws/events`) and does not poll. It uses `block-added`,
`block-finalised`, `deploy-verdicts` and `entered-running-state`. On `stream-lagged`, on an event it cannot
parse, and on every reconnect it reads every released deploy's status again. The node must publish
`deploy-verdicts` and report `latestBlockNumber`, `deployLifespan`, `maxUserDeploysPerBlock` and a bounded
`maxParentDepth` in `/api/status`; the queue does not connect to a node whose `maxParentDepth` is unbounded.

## Running

```bash
deploy-queue --node-grpc http://validator:40401 --node-http http://validator:40403 \
  --data-dir /var/lib/deploy-queue
```

`--grpc-listen` and `--http-listen` change the client addresses. `RUST_LOG` sets the log filter. The queue has no
configuration file: it reads the shard's values from its node. It commits the changes of each batch of waiting
requests to its LMDB store in one transaction before it answers, broadcasts or sends anything for them, so a
restart keeps all held deploys. The release window and the rates start again. The store format changed with the
persisted last-sent height: a data directory from an earlier build does not load.

## Tests

```bash
cargo test --release -p deploy-queue
cargo clippy --release -p deploy-queue --all-targets
```

`tests/queue_against_a_node.rs` runs the engine, the event link and the node client against a stand-in node.
