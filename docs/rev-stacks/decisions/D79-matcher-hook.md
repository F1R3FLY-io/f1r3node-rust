# Decision 79: hook points for the funded-receive filter (for lane C)

Lane A2, on `patch/D76`. Decision 79: a stored receive whose wallet cannot pay its cell is not a match candidate. The cell is tested against the wallet's pre-state balance minus what the block holds, and replay re-checks every logged COMM. This note names the places where the filter goes, and the data that the executor supplies. Lane C owns the filter.

## Where guards are filtered today

The cross-channel `where` guard is a veto on a match whose spatial binds all succeeded: `Match::check_commit(&K, &[A]) -> bool` (`rspace++/src/rspace/match.rs`), implemented by `Matcher::check_commit` (`rholang/src/rust/interpreter/matcher/match.rs`). `K` is the `TaggedContinuation`, so the hook already sees the stored seal record (`seal::of_continuation`). A veto rolls the match back and the next candidate is tried, as for a spatial miss.

| Path | Call | Covers |
|---|---|---|
| Play, a send that meets stored receives | `space_matcher.rs`, `extract_first_match`, line 161 | Each waiting continuation in turn. A veto tries the next one. |
| Play, a receive that meets stored data | `rspace/ops_consume.rs`, line 79 | The new receive. On a veto, the receive is stored. |
| Replay, a send | `replay_rspace.rs`, line 1298, through `extract_first_match` | As in play. |
| Replay, a receive | `replay_rspace.rs`, `locked_consume` → `get_comm_and_consume_candidates` → `run_matcher_consume` | **No `check_commit` call today.** Replay rebuilds the COMM from the trace and asserts that the trace holds it. A `where` guard on a receive that meets stored data is not re-checked in replay either. |

Decision 79 therefore needs a second hook on the replay consume path, after `run_matcher_consume` finds the candidates and before the COMM is accepted. A play and a replay that disagree there must make the block invalid, not store the receive. A trace COMM that the filter refuses means the proposer matched an unfunded receive.

The filter belongs on the stored receive, which is `K` on the send paths. On the receive paths the new receive runs under the current record, and its own cell is the sender's side of Decision 76. That side is charged to the running record and checked at the close of the run (Decision 48), not in the matcher.

## What the filter needs, and where it comes from

1. **The receive's wallet and price:** in `K` (the stored `SealRecord`: owner and origin price). A receive with no record is genesis code, charged to the client (Decision 81), so it is always a candidate.
2. **The wallet's pre-state balance:** read from history at the block pre-state, not from the run's hot store. The run's own writes must not change the test. `RevStacks` over a history reader at `pre_root` gives it, and `StoreReader::at(&runtime_manager, pre_root)` (`v6/gate.rs`) already reads balances at the pre-state for the gate.
3. **What the block holds on that wallet:** the gate's commitment table, `GateOutcome::table.committed(&wallet)` (`v6/gate.rs`). Today the executor passes each run only the reservations of its own paying wallets (`hold` → `set_reservation`, `executor.rs`). The filter needs the whole table.

## Proposal for the interface

The executor builds one read-only oracle per block and installs it on each run's runtime, as it installs the reservations now:

```rust
pub trait FundedReceive: Send + Sync {
    fn can_pay(&self, owner: &WalletOwner, phlo_price: u64) -> bool;
}
```

Its value is `pre_balance(owner) - table.committed(wallet(owner)) >= phlo_price` (one cell at the origin price), with all inputs fixed for the block. The same oracle, built from the recorded gate, goes on the replay runtimes. `Matcher` gets the oracle through a task-local or a field set per run, the way `seal_records` is set today. `Matcher` then vetoes in `check_commit` when `of_continuation(k)` names an owner and `!can_pay(owner, price)`. That is one function for play and replay.

Lane A2 builds the executor side (the oracle and its installation) when lane C confirms the trait. It does not depend on FX-12's other parts.
