# Replay Liveness Bound Claim

## Status

The empty-store hot-loop claim is modeled. General indexed replay equivalence remains pending.

## Claim

A persistent-contract replay must not clone the complete recorded `COMM` multiset for every empty-store fire.

For `N` empty-store fires, recorded `COMM` clones and matcher runs must be at most linear in `N`.

The optimized replay path must select the same recorded `COMM` as the full replay path when store data exists.

## Formal statement

```text
empty_store_clone_work(N) <= N
empty_store_matcher_work(N) <= N
indexed_selection(trace, store) = full_selection(trace, store)
```

## Evidence

- `formal/tlaplus/replay_liveness/ReplayHotLoop.tla`
- `formal/tlaplus/replay_liveness/MC_ReplayHotLoop.cfg`
- `formal/tlaplus/replay_liveness/MC_ReplayHotLoop_quadratic_pre_fix.cfg`
- `rspace++/src/rspace/replay_rspace.rs::locked_consume`

## Required code bridge

Add deterministic counters for cloned candidates and matcher evaluations.

Add a 100,000-fire persistent-contract replay test.

Assert a linear operation bound instead of a wall-clock threshold.

Generate replay states with multiple recorded candidates.

Compare indexed selection with full-scan selection for each generated state.

## Gate rule

A superlinear operation count or selection mismatch refutes this claim and blocks completion.
