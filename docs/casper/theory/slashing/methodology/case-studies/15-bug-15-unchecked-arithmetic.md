# Case study #15 — Sequence arithmetic used unchecked boundaries

## 1 · Summary

Pre-fix, sequence-number arithmetic in
`casper/src/rust/slashing_authorization.rs` and related call sites
used unchecked operations (`+ 1`, `- 1`, direct `i32` casts from
`u64`). At boundaries — `i32::MAX`, `u64::MAX`, `seq_num = 0`,
`epoch_length = 0` — the unchecked operations either panicked, wrapped
to negative values, or silently truncated. An adversary could feed
boundary-tickling sequence numbers to derail the authorization
decision. Post-fix, all sequence and epoch arithmetic uses the
`checked_*` family of helpers that saturate to `None` on overflow /
underflow / divide-by-zero.

## 2 · Discovery technique

**Primary**: Kani harnesses on `checked_base_seq`, `checked_next_seq`,
and `epoch_for_block_number` proved the boundary behavior on the
exhaustive primitive domain; the pre-fix implementations failed
the harnesses immediately.

**Corroborating**:

- libFuzzer target `slashing_arithmetic` exercised the same
  helpers under coverage-guided mutation on the unbounded `u64`
  / `i64` domain (Kani's bound is `i32`).
- Sage `bounded_arithmetic_model.sage` finding #8 enumerated the
  overflow scenarios and confirmed the pre-fix behavior diverges
  from exact arithmetic at the boundary.

## 3 · Witness reproduction

```
cargo kani -p casper --harness checked_base_seq_rejects_nonpositive
cargo fuzz run slashing_arithmetic -- -runs=10000
```

Both commands return success on the post-fix code; the pre-fix
code (recoverable via `git show db0b979^`) fails both.

## 4 · Classification trace

```
threat_class       = projection_risk → permitted_bug_fix
ledger_status      = confirmed_fixed_bug
action             = Keep Kani harnesses + libFuzzer target +
                     prop_t_9_7_seqnum_density.rs as a layered
                     defense
```

## 5 · Evidence stack

| Layer            | Artifact                                                                                                                                                                                                                                        |
|------------------|-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| Rocq theorem     | T-9.15 (`BugFixCheckedArithmetic.v` — implicit through Kani harness equivalence)                                                                                                                                                                |
| Kani harnesses   | `checked_base_seq_rejects_nonpositive`, `checked_base_seq_matches_positive_i32_predecessor`, `checked_next_seq_matches_i32_successor`, `epoch_for_block_number_rejects_invalid_domain`, `epoch_for_block_number_matches_bounded_floor_division` |
| libFuzzer target | `fuzz/fuzz_targets/slashing_arithmetic.rs`                                                                                                                                                                                                      |
| Sage             | `bounded_arithmetic_model.sage` finding #8                                                                                                                                                                                                      |
| Rust regression  | `prop_t_9_7_seqnum_density.rs` (post-fix density property)                                                                                                                                                                                      |
| Bug-fix manifest | [`../../design/09-bug-fixes-and-rationale.md §9.17`](../../design/09-bug-fixes-and-rationale.md)                                                                                                                                                |

**Stack depth: 4** (Kani + libFuzzer + Sage + Rust + design).

## 6 · Lessons for the methodology

1. **Kani is the right tool for per-function boundary
   verification**. Five Kani harnesses verify the three arithmetic
   helpers in seconds with exhaustive coverage on the bounded
   primitive domain; no other technique gives both per-function
   exhaustion and Rust-level fidelity.
2. **libFuzzer + Kani is a *complementary* pair**. Kani exhausts
   the bounded domain; libFuzzer extends to the unbounded domain
   under composition. Both layers are required for layered
   defense.
3. **Sequence number 1 is *not* a boundary; `seq_num ≤ 0` is**.
   The pre-fix boundary was `seq_num <= 1`, dropping sequence 1
   (the valid genesis-child). The methodology requires every
   boundary expression to have a docstring stating the
   inclusive/exclusive choice and its rationale.
