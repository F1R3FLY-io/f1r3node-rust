# Decision 96 in order-then-execute: what an uncovered block does

Lane A2, 2026-11-03. Escalation memo: consensus and state root.

## The gap

Decision 96 makes a block invalid when its proposer cannot cover the close's PoS charge. In v6 a block is ordered before it executes. `check_structure` cannot test the cover, because the cover depends on the block's settlement. Each executor learns of the problem only when it runs the close. At that point the block is already in the order.

Today an execute error stops the engine's step. For example, `require_no_slashes` returns an error. An uncovered block therefore has no valid record. Each honest executor fails on it in the same way, and the chain stops at that block.

## Options

1. **Void block.** An uncovered block executes as an empty block: no envelope runs, there is no close, and the post-state root is the pre-state root. Play and replay both find the shortfall and give the void record. Evidence names the proposer (slashing comes with B6). Cost: one record outcome, a branch in `execute` and `replay`, and tests. Estimate 0.5 day. The proposer loses the block's fees, and the clients' envelopes go back to the pool.
2. **Close without the charge.** The envelopes stand. The close runs, and the proposer pays what it holds. The shortfall goes unpaid and is recorded as evidence. This contradicts Decision 96 ("invalid") and leaves work unpaid.
3. **Proposer pre-check only.** A proposer does not propose when its speculative execution shows a shortfall. This does not cover a dishonest proposer, so option 1 or 2 is still necessary.

## Recommendation

Option 1, with option 3 as a courtesy check in `create_block`. Only option 1 keeps "invalid" and lets the chain continue.

## Built now (lane A2)

`stops::close_covered` runs after the close in `execute` and in `replay`. A shortfall is an error in both, which is the stop described above. Lane B's FX-16 records the cover. Until a ruling, the check gives no void record.
