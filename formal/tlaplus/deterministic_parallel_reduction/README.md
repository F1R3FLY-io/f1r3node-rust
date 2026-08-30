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

`EvaluationBoundary.tla` separately models cancellation ownership. The evaluation keeps a shared epoch permit until every detached child has completed. Checkpoint, reset, rollback, and replay boundaries require the exclusive permit. Its unsafe control releases the permit when the root future is cancelled and produces a checkpoint while detached child mutations are still live.
