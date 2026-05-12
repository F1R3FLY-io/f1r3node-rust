# Case study #11 — Detector traversal was partial and duplicate-child sensitive

## 1 · Summary

Pre-fix, the equivocation detector's BFS-style traversal of a
validator's justifications had two defects: (1) on a missing
latest-message pointer, the traversal returned `∅` (fatal exit
disguised as no-evidence); (2) when two distinct justification edges
pointed at the same child block, the cardinality count
double-counted, falsely reporting `≥ 2 distinct children`.
Post-fix, the detector is *total* (missing pointers are
non-contributing) and *canonical* (duplicate paths to the same
child are deduplicated before cardinality).

## 2 · Discovery technique

**Primary**: Hypothesis `hypothesis_assumption_minimization.py`
generated minimal witnesses where the detector's behavior diverged
from the expected mathematical specification; the minimization
produced two distinct witness shapes corresponding to the two
defects.

**Corroborating**:

- Sage `theorem_assumption_counterexamples.sage` showed that
  removing the "missing pointers are non-contributing" assumption
  allowed any equivocation to escape detection through a single
  malformed view.
- TLA⁺ `EquivocationDetector.tla` `Inv_FixedDetectorTotal` and
  `Inv_DuplicateChildNeedsDistinctChildren` exhausted finite
  instances of both defects.

## 3 · Witness reproduction

```
cargo test -p casper --test mod -- slashing::pre_fix_bug_11
```

The fixture
[`casper/tests/slashing/pre_fix_bug_11.rs`](../../../../../casper/tests/slashing/pre_fix_bug_11.rs)
encodes both defects in adjacent test cases.

## 4 · Classification trace

```
threat_class       = projection_risk → permitted_bug_fix
                     (the partial traversal was a projection of the
                      mathematical detector that diverged on real
                      inputs; the fix restores the projection's
                      faithfulness)
ledger_status      = confirmed_fixed_bug
action             = Keep pre_fix_bug_11.rs + multiple post-fix anchors
                     (prop_t_9_11_*.rs, uc_101..108_detector_*.rs)
```

## 5 · Evidence stack

| Layer            | Artifact                                                                                                                                                                                                                                                                                                                           |
|------------------|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| Rocq theorem     | T-9.11 (`BugFixDetectorPartiality.v` — detector totality, permutation invariance, bisim under complete pointers)                                                                                                                                                                                                                   |
| TLA⁺ invariants  | `Inv_FixedDetectorTotal`, `Inv_MissingPointerNonContributing`, `Inv_DuplicateChildNeedsDistinctChildren`, `Inv_TwoDistinctChildrenDetect`                                                                                                                                                                                          |
| Sage             | `theorem_assumption_counterexamples.sage`                                                                                                                                                                                                                                                                                          |
| Hypothesis       | `hypothesis_assumption_minimization.rs`                                                                                                                                                                                                                                                                                            |
| Rust regression  | `pre_fix_bug_11.rs`, `prop_t_9_11_detector_totality.rs`, `prop_t_9_11_detector_permutation_invariance.rs`, `prop_t_9_11_detector_bisim_under_complete_pointers.rs`                                                                                                                                                                 |
| Detector tests   | `uc_101_detector_missing_nested_pointer.rs`, `uc_102_detector_order_independence.rs`, `uc_103_detector_preconditioned_bisim.rs`, `uc_104_detector_no_unsafe_lookup.rs`, `uc_105_detector_detected_hash_order.rs`, `uc_106_detector_two_child_order.rs`, `uc_107_detector_validator_churn.rs`, `uc_108_detector_duplicate_child.rs` |
| Bug-fix manifest | [`../../design/09-bug-fixes-and-rationale.md §9.13`](../../design/09-bug-fixes-and-rationale.md)                                                                                                                                                                                                                                   |

**Stack depth: 5+** (Rocq + TLA⁺ + Sage + Hypothesis + Rust × 10).

## 6 · Lessons for the methodology

1. **Two defects, one fix**. The methodology's decomposition into
   T-9.11 covers *both* the partiality and the duplicate-child
   sensitivity; the bug-fix manifest entry §9.13 documents both as
   one bug because they share the same root cause (the
   pre-fix BFS lacked canonicalization).
2. **Eight detector tests (uc_101..108) target distinct edge
   cases**. The methodology's *uniform-coverage* pattern (one test
   per edge case) applies here — each test targets one of the
   distinct pointer / child / order shapes the fix must handle.
3. **Hypothesis assumption-minimization is the right tool for
   "what's the smallest counterexample?"**. Where proptest would
   shrink a random input, Hypothesis can shrink an *assumption* —
   the test was *“what is the smallest view that breaks the
   detector?”* and the answer was a 2-pointer view, immediately
   reproducible.
