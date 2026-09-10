# Heartbeat-only shards alternate between a dense and a sparse block regime

**Status.** Operational observation, recorded 2026-09-10. Evidence for ledger entries D-03 and D-06. Not a decision.

**Source.** Operator report from a heartbeat-only shard on `dev`, three validators and one bootstrap node, 2.5 days. Run identifiers were not supplied. This record was not reproduced by its author.

## Finding

A heartbeat-only shard with three validators and one bootstrap node ran for 2.5 days with no user deploys. It showed two block regimes.

| Regime | Blocks per height | Block processing time | Stability |
| --- | --- | --- | --- |
| Dense | 3, one from each validator | Grew from 192 ms to 1107 ms over 2.5 days | Cost grows with chain length |
| Alternating | 1 or 2 | Low, about one fifth of the dense cost | Stable regardless of chain length |

The shard started in the dense regime. It switched to the alternating regime without an info-level log event and later switched back. Run identifiers were not supplied with the report, and this session did not reproduce the numbers.

## Mechanism

The dense regime is the frontier-follow lane of the heartbeat proposer, lane 2 in [Consensus Protocol section 8](../CONSENSUS_PROTOCOL.md). A validator proposes whenever it observes new parents. With `check-interval = 5 seconds` and `self-propose-cooldown = 3 seconds` in `node/src/main/resources/defaults.conf`, every validator sees the other two blocks on each tick and proposes again. Each height gets three blocks.

The processing cost comes from parent selection. The estimator walks to the lowest universal common ancestor of the latest messages, the lowest block through which every history passes (`casper/src/rust/util/dag_operations.rs`). The walk is bounded by the approved block and by a 1000-deep latest-message filter in `casper/src/rust/estimator.rs`, not by the last finalized block. In the dense regime no height holds a single block, so the walk continues to the last single-block height. That height recedes while the regime persists, and the cost grows with chain length.

The switch is an emergent effect of three throttles in `defaults.conf`, not a designed mode:

- `frontier-chase-max-lag = 20` throttles frontier-follow once a validator is more than 20 blocks ahead of the LFB.
- `empty-frontier-max-unfinalized-blocks = 12` stops empty proposals once more than 12 unfinalized blocks sit above the LFB.
- The 3-second cooldown against the 5-second tick lets one validator skip a tick when phases drift.

Any of these breaks the lock-step. Heights fall to one or two blocks, the ancestor walk finds a single-block height within one or two steps, and the cost drops. When finalization catches up, frontier-follow resumes and the dense regime returns.

Only the backpressure activation logs at info level, in `node/src/rust/instances/heartbeat_proposer.rs`. The lag-cap and cooldown throttles log their reason at debug level. A silent switch at info level points at those two throttles.

## Implications

- [D-06](./decision-ledger/06-heartbeat-recovery-leadership.md) open question 2 asked whether removing the frontier-follow lane changes cadence on a shard with no deploys and no stall. It does. The dense cadence is that lane. PR #216 rule R-HEARTBEAT-WORK removes the lane, so the dense regime cannot form under that rule.
- [D-03](./decision-ledger/03-fork-choice-certified-context.md) sub-decision 3.3 asks for a work bound on the LCA walk as a function of floor distance. This run shows the unbounded walk in production numbers. The PR #216 certified context uses the floor as the backstop, which bounds the walk to floor distance.
- The regime switch is undocumented. Operators who tune cadence need to know that block cost, not only block count, depends on which throttle is active.
