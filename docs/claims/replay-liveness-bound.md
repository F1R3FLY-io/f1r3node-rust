# Replay Liveness Bound Claim

## Status

The empty-store hot-loop and singleton task-chain claims are modeled. General indexed replay equivalence remains pending.

## Claim

A persistent-contract replay must not clone the complete recorded `COMM` multiset for every empty-store fire.

For `N` empty-store fires, recorded `COMM` clones and matcher runs must be at most linear in `N`.

The optimized replay path must select the same recorded `COMM` as the full replay path when store data exists.

A singleton recursive evaluation must not retain one Tokio task for each recursion step.

The inline evaluator must yield to the scheduler at a fixed work interval.

## Formal statement

```text
empty_store_clone_work(N) <= N
empty_store_matcher_work(N) <= N
indexed_selection(trace, store) = full_selection(trace, store)
live_recursive_tasks(N) = 0
yield_work(N) <= N
```

## Evidence

- `formal/tlaplus/replay_liveness/ReplayHotLoop.tla`
- `formal/tlaplus/replay_liveness/MC_ReplayHotLoop.cfg`
- `formal/tlaplus/replay_liveness/MC_ReplayHotLoop_quadratic_pre_fix.cfg`
- `rspace++/src/rspace/replay_rspace.rs::locked_consume`
- `rholang/src/rust/interpreter/reduce.rs::DebruijnInterpreter::eval_inner`
- `rholang/tests/single_term_recursion_spec.rs`

## Required code bridge

Add deterministic counters for cloned candidates and matcher evaluations.

Add a 100,000-fire persistent-contract replay test.

Assert a linear operation bound instead of a wall-clock threshold.

Generate replay states with multiple recorded candidates.

Compare indexed selection with full-scan selection for each generated state.

Retain exact-commit convergence evidence below the 10 GB standard integration ceiling.

## Gate rule

A superlinear operation count or selection mismatch refutes this claim and blocks completion.
