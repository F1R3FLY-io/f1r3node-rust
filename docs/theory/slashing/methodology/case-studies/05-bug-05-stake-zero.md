# Case study #5 — Stake-0 silent classification

## 1 · Summary

Pre-fix, the equivocation detector at
`equivocation_detector.rs:184` checked `if stake > 0` to skip stake-
zero validators; this filter silently dropped equivocation
classifications for validators whose bond had been reduced to zero
(e.g. by an earlier slash). The result was that a once-slashed
validator could continue to equivocate without producing further
records. Post-fix, the detector treats stake-zero validators as
*already-slashed* (passive state) but does not silently drop their
classifications.

## 2 · Discovery technique

**Primary**: Sage `weighted_closure_model.sage` enumerated stake
vectors with zero-stake validators and found that a zero-stake
direct offender (Sage finding #3) was admitted by the model but
filtered by the production detector. The model output classified
the divergence as `candidate_boundary`; investigation revealed the
Rust filter was hiding a real classification.

**Corroborating**: Hypothesis state machine search for lifecycle
sequences ending in stake-zero validators corroborated the
classification gap.

## 3 · Witness reproduction

```
cargo test -p casper --test mod -- slashing::pre_fix_bug_5
```

The fixture
[`casper/tests/slashing/pre_fix_bug_5.rs`](../../../../../casper/tests/slashing/pre_fix_bug_5.rs)
encodes a scenario where validator `v0` has bond zero (e.g. post-
slash) and signs two distinct blocks; pre-fix the detector returns
`Status::Valid` silently; post-fix the detector returns the
appropriate equivocation classification.

## 4 · Classification trace

```
threat_class       = permitted_bug_fix
                     (the `if stake > 0` guard was a documented
                      Scala-inherited defensive check that turned
                      out to suppress evidence)
ledger_status      = confirmed_fixed_bug
action             = Keep pre_fix_bug_5.rs + post-fix anchor
                     prop_t_9_5_active_has_positive_bond.rs
```

## 5 · Evidence stack

| Layer            | Artifact                                                                              |
|------------------|---------------------------------------------------------------------------------------|
| Rocq theorem     | `t_9_5_active_has_positive_bond` (`BugFixStakeZero.v` — implicit through PoS invariants) |
| Sage model       | `weighted_closure_model.sage`; finding #3                                              |
| Rust regression  | `pre_fix_bug_5.rs`, `prop_t_9_5_active_has_positive_bond.rs`                            |
| Bug-fix manifest | [`../../design/09-bug-fixes-and-rationale.md §9.6`](../../design/09-bug-fixes-and-rationale.md) |

**Stack depth: 4** (Rocq + Sage + Rust regression + design).

## 6 · Lessons for the methodology

1. **"Defensive" filters can hide bugs**. A `if x > 0` guard that
   silently skips an interesting case is a *failure to record
   negative evidence*. The methodology prefers explicit `match`
   arms (with logged outcomes) over silent guards.
2. **Sage's exact-integer arithmetic surfaces boundary cases that
   randomized testing misses**. Stake zero is exactly the kind of
   boundary that `proptest` is unlikely to sample (its strategies
   default to small positive integers); Sage enumerates the
   boundary explicitly.
3. **Classification is observable behavior, not internal state**.
   The detector's *return value* is the classification; silently
   skipping a classification changes that return value and is
   therefore an observable defect, not an internal optimization.
