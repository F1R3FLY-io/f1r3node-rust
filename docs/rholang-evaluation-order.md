# Rholang Evaluation Order and Consensus Determinism

> Last updated: 2026-04-14

## Rholang Is Non-Deterministic By Design

Rholang parallel composition (`A | B`) means A and B can execute in any order,
and different orderings may produce different observable results. This is a
feature — it models real concurrent systems where processes execute independently
without a global clock.

For example, `@x!(1) | @x!(2) | for(@v <- x) { ... }` can match either
`1` or `2` to the consumer. Both are valid executions.

## Blockchain Consensus Requires Reproducibility

When a proposer evaluates a deploy, it produces a specific state hash, cost,
and event log. Every validator must replay that deploy and produce the EXACT
same result. The non-determinism is resolved at the implementation level:

1. **Evaluation order**: The `eval(Par)` term vector ordering (sends first,
   receives second) determines stable branch source paths and task registration
2. **Candidate matching**: `Random.shuffle` with a deploy-derived seed selects
   the same candidate deterministically
3. **Replay oracle**: `ReplayRSpace` uses the play event log to force the
   exact same COMMs during replay, regardless of evaluation order

## Implementation: FuturesUnordered Branch Draining

The Rust node uses `tokio::spawn` for parallel Par branch evaluation, matching
Scala's `parTraverse`, and drains the branch tasks with `FuturesUnordered` so
completion order does not impose a sequential join barrier. Per-channel locks
use `tokio::sync::Mutex` which guarantees FIFO ordering for waiters, matching
Scala's cats-effect `Semaphore`.

Both `RSpace` and `ReplayRSpace` have per-channel two-phase locks, matching
Scala's `RSpaceOps` which provides locks to both via inheritance.

## Scala Empirical Analysis

Instrumented logging on the Scala node (`RSpaceOps.scala`, `RSpace.scala`)
confirmed across 4 independent runs:

- Both produces ALWAYS store before the consume fires COMM for the join
  contract `@0!!(0) | @1!!(1) | for (_ <- @0 & _ <- @1) { 0 }`
- The sends-first term vector ordering combined with Monix ForkJoinPool
  task registration produces consistent operation ordering
- The pattern holds across 20+ parallel repetitions per run on threads 27-44

## Known Limitations

- **RCHAIN-3917**: Scala acknowledges non-deterministic cost for some contracts
  involving persistent operations with same-channel joins. The Scala test for
  randomly generated contracts is `ignore`d.
- **RCHAIN-4032**: `SpaceMatcher.extractDataCandidates` causes unmatched COMMs
  with overlapping join patterns. Joins like `for(<- x & <- x)` are explicitly
  blocked with `ReceiveOnSameChannelsError`.

## Cost Accounting (Source Tokens)

The reducer reserves source-token events before executing metered Rholang work.
Parser failures consume zero tokens because no metered source state exists yet.
RSpace is not a metering wrapper; it only records tuple-space state, matching,
cleanup, and replay logs. This keeps play/replay costs independent from which
parallel task happens to trigger a COMM.

Metering uses explicit work frames (`MeteredMachine`) rather than recursive
charging. Each live billable frame is keyed by deploy id, branch-derived source
path, redex id, and local index, then drained in canonical order before
atomically reserving tokens from `RuntimeBudget`. This preserves maximum branch
parallelism: spawned Par tasks do not serialize on evaluation, only on the short
budget reservation CAS.

## References

- Scala `Reduce.scala` line 256-289: term vector construction
- Scala `RSpaceOps.scala` lines 107-158: per-channel Semaphore locks
- Scala `CostAccountingSpec.scala` line 303-317: determinism test with `.par`
- `cost-accounted-rho.pdf`: Future architecture for in-language cost accounting
