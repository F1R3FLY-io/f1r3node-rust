# Deterministic parallel reduction

This model covers the interpreter-to-RSpace linearization boundary. Parallel branches submit typed intents, a complete frontier is frozen, and transitively overlapping channel or cost-authority footprints commit in causal order. Components with disjoint channels and disjoint linear-authority regions may commit together. Checkpoints are admitted only after the frontier is quiescent.

The minimal counterexample is two sends competing with one receive on the same channel. Sorting visible candidates is insufficient because an earlier match cannot consider a candidate that has not arrived. The safe model waits for the complete frontier and leaves value `2` after value `1` wins the canonical communication. A send on `y` shares a linear cost-authority region with the `x` component and must not execute as disjoint work. A send on `z` has an independent channel and authority and witnesses retained concurrency.

Run the safe model with:

```console
tlc -config MC_DeterministicParallelReduction.cfg DeterministicParallelReduction.tla
```

Run the bounded symbolic cross-witnesses with:

```console
scripts/check-cost-accounted-rho-apalache.sh --filter deterministic
```

The five unsafe controls respectively demonstrate arrival-order commitment, non-canonical conflict commitment, checkpointing with work in flight, global serialization that discards disjoint parallelism, and classifying operations with a shared purse region as independent merely because their channels differ. Dedicated Apalache configurations select one violated invariant per control so the symbolic trace identifies the exact defect rather than an earlier auxiliary invariant.

`EvaluationBoundary.tla` separately models structured cancellation. Root cancellation requests child cancellation. Each child then aborts without a mutation. The shared epoch permit remains held until all children terminate. Checkpoint, reset, rollback, and replay boundaries require the exclusive permit. Its unsafe control detaches the children and releases the permit. The resulting trace checkpoints while child mutations remain possible.

`ReductionDriverLifecycle.tla` models driver ownership across participant submission, synchronous work, asynchronous work, cancellation, completion, and repeated batches. Submission claims an inline driver in the same state transition that completes a frontier. The driver polls the batch once. A ready poll commits without a Tokio scheduler boundary. A pending poll transfers the same batch to a spawned driver before the participant can yield. A driver releases ownership before it delivers results. Result delivery can then start the next frontier without reentry into the completed driver.

The safe configurations exhaust all states for two rounds across three participants. They check synchronous and asynchronous paths, exact operation conservation, one driver owner, waiter ownership, trace uniqueness, per-participant order, eventual quiescence, and retained incomplete-frontier concurrency. The claim control separates submission from ownership and reaches a ready frontier with no driver. The transfer control permits a pending inline poll without ownership transfer and reaches a cancellation-sensitive driver state.

Run the lifecycle models with:

```console
tlc -config MC_ReductionDriverLifecycle.cfg ReductionDriverLifecycle.tla
tlc -config MC_ReductionDriverLifecycle_inline.cfg ReductionDriverLifecycle.tla
```

`SingleParticipantFastPath.tla` models the direct path after all other participants terminate. Direct and scheduled singleton commitment produce the same result. Direct ownership requires exactly one live participant and eventually resolves. The unsafe control permits direct ownership with two live participants and violates that requirement.

Run the singleton models with:

```console
tlc -config MC_SingleParticipantFastPath.cfg SingleParticipantFastPath.tla
tlc -config MC_SingleParticipantFastPath_unsafe.cfg SingleParticipantFastPath.tla
```
