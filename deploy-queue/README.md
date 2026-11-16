# deploy-queue

A per-validator deploy queue. It runs as a separate process next to one validator, from the same image
(`/opt/docker/bin/deploy-queue`). Clients send deploys to the queue in place of the validator. The queue holds
each deploy durably and releases it as fast as the validator takes deploys into its blocks. When clients offer
more than the validator can take before the deploys' windows close, the queue refuses the excess with a
retry-after, so a deploy it accepts lands within its deploy lifespan.

The states a client reads, the refusals and the client endpoints are in
[Deploy queue client API](../docs/node/deploy-queue.md). This file describes how the queue decides.

## Validator and shard

Deploys never gossip: only the validator a queue fronts can include that queue's deploys. The queue learns
everything else from the node's event stream:

- **The validator.** Each `block-created` event carries the block's deploys and the block creator's
  `deploy-admission`: the most fresh deploys the block could take (`cap`), what set the cap, and whether the node
  lowered it because finality trails the tip (`backpressure`). The queue learns from a block only when this queue
  had deploys in the validator's pool as it was built; a block built from an empty pool says nothing about what
  the validator takes. A block that left this queue's deploys behind took what it could, whatever the event says
  about its cap. A block that took everything it was offered was limited only by demand, so it could have taken
  its cap. The validator's intake is the lower quartile of what its blocks could take over the last citable depth
  (`maxParentDepth` heights), times how often it creates a block: an admitted deploy waits through many blocks
  while the node swings its cap with finality, so admission counts on the lower caps rather than their average.
- **The shard.** Every `block-added` event, from any creator: how fast heights come, and the validator's share of
  the blocks. The node's own `backpressure` says the shard's finality is holding the validator back.

`GET /queue/status` reports both: `validator` (own blocks, block share, intake per height, pool target, latest
cap and its source, saturated) and `shard` (seconds per height, backpressure, finality lag).

## Admission

The queue refuses a deploy with the node's own error when its signature, shard id, phlo price, expiration,
validity window or Rholang term is not valid, when its `validAfterBlockNumber` is more than `maxParentDepth`
above the tip, or when the queue already holds it.

The queue then checks capacity. Release takes the earliest deadline (last includable block) first, so a deploy
waits behind every deploy in the validator's pool and every queued deploy with an earlier deadline. The queue
accepts a deploy only when, at the validator's intake, it is taken into a block before its validity window
closes, with one citable depth to spare, and no queued deploy with a later deadline that lands in time now would
stop landing in time. The window is the shard's `deployLifespan`, read from the node, so the line the queue holds
scales with it. Before the validator has built a block from this queue's deploys, the intake is one deploy per
height. Deploys sharing a deadline share their valid-after block, so the check walks the distinct deadlines
ahead, not every deploy held.

A deploy that does not fit gets `deploy_queue_full` with a retry-after, the time until a deploy signed now would
fit, and a cause: the shard when the node is holding its validator back for finality or the queue holds release
for finality (see Release), the idle validator when it has created no block over a citable depth while the shard
moved on (the queue then takes no deploy until it creates blocks again), the validator otherwise (it takes fewer
deploys than arrive).

One deployer may hold at most an equal share of the queue's capacity that keeps room for one more deployer:
`capacity / (other deployers + 2)`, and never less than one deploy. The capacity is what the validator takes in
a fresh window, less the citable depth. A deployer over its share gets `deploy_queue_full` too.

## Release

The queue keeps the validator's pool holding what its next block will take: the cap its block creator last
applied to this queue's deploys, and one deploy until a cap is known. Deploys beyond that wait in the queue, in
deadline order and within their deployer's share, rather than in the pool. A deploy carried by a block no longer counts against the pool; its
verdict is the shard's to reach, so how long finality takes does not slow release.

Release stops while the queue is not connected and for a backoff after the node could not take a deploy (1 s,
doubling up to 30 s). Release also stops, and intake refuses every deploy with `deploy_queue_full` and the shard as
its cause, while the tip leads the last finalized block by more than `maxParentDepth`. Deploys released then only
deepen the unfinalized work every validator must merge. Both resume when a `block-finalised` event brings the lag
back within `maxParentDepth`. `GET /queue/status` reports the hold as `finality_hold` with the lag. A shard whose tip
and finality both stand still needs no hold: no block takes the pooled deploys, so release adds none. The node cannot take a deploy while its validator is not bonded (`503
validator_not_bonded`); the deploy waits in the queue and expires there if the validator is not bonded before its
window closes. The queue does not release a deploy whose window closes before the validator's next block is due
(the heights between its blocks). That deploy closes in the queue, and the queue later records the node's answer
for it, or `expired`. A released deploy that no block carries within one citable depth goes to the node again.

## Node link

The queue follows the node's event stream (`/ws/events`) and does not poll. It uses `block-created`,
`block-added`, `block-finalised`, `deploy-verdicts` and `entered-running-state`. On `stream-lagged`, on an event
it cannot parse, and on every reconnect it reads every released deploy's status again. The node must publish
`deploy-verdicts` and `deploy-admission` on `block-created`, and report `latestBlockNumber`, `deployLifespan`,
`maxUserDeploysPerBlock` and a bounded `maxParentDepth` in `/api/status`; the queue does not connect to a node
whose `maxParentDepth` is unbounded.

## Running

```bash
deploy-queue --node-grpc http://validator:40401 --node-http http://validator:40403 \
  --data-dir /var/lib/deploy-queue
```

`--grpc-listen` and `--http-listen` change the client addresses. `RUST_LOG` sets the log filter. The queue has no
configuration file: it reads the shard's values from its node. It commits the changes of each batch of waiting
requests to its LMDB store in one transaction before it answers, broadcasts or sends anything for them, so a
restart keeps all held deploys. What it measured of the validator and shard starts again. The store format
changed with the persisted last-sent height: a data directory from an earlier build does not load.

## Tests

```bash
cargo test --release -p deploy-queue
cargo clippy --release -p deploy-queue --all-targets
```

`tests/queue_against_a_node.rs` runs the engine, the event link and the node client against a stand-in node.
