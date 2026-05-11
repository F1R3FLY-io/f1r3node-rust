# 03 · Arithmetic & projection models

## 1 · Family motivation

Bounded arithmetic is a *non-obvious* source of bugs in a system
whose mathematical specification uses unbounded integers. The slashing
specification reasons about validator counts, stake amounts, sequence
numbers, epoch indices, and block numbers as integers; the Rust
implementation projects these onto `i32`, `i64`, and `u64`. A
projection that wraps, overflows, or silently truncates at the
boundary is a *projection risk*; one that allows an adversary to
manipulate the boundary is a *projection bug* (Bug #15).

## 2 · Models in this family

| Model                                                                                                                        | Searches                                                                                   |
|------------------------------------------------------------------------------------------------------------------------------|--------------------------------------------------------------------------------------------|
| [`arithmetic_envelope_model.sage`](../../../../../formal/sage/slashing/arithmetic_envelope_model.sage)                       | Fixed-width safe envelopes `initialVault + validators * maxBond ≤ limit`                   |
| [`bounded_arithmetic_model.sage`](../../../../../formal/sage/slashing/bounded_arithmetic_model.sage)                         | Compares exact arithmetic against checked / wrapping / saturating bounded projections      |
| [`implementation_projection_risk_model.sage`](../../../../../formal/sage/slashing/implementation_projection_risk_model.sage) | Searches for parameter shifts that move the projection boundary into the production domain |

## 3 · Representative witness

```json
{
  "kind": "bounded_arithmetic_witness",
  "operation": "i64_add",
  "operand_a": 9223372036854775807,
  "operand_b": 1,
  "exact_result": 9223372036854775808,
  "checked_result": null,
  "wrapping_result": -9223372036854775808,
  "saturating_result": 9223372036854775807,
  "production_uses": "checked",
  "matches_production": true
}
```

Reading: the canonical `i64::MAX + 1` overflow. The exact result
exceeds `i64::MAX`. The production code uses `checked_add`, which
returns `None` (matches `null` in the witness); the wrapping
alternative would silently invert the sign, an attack vector
exploited in Bug #15.

## 4 · Promotion targets

| Witness shape                        | Kani harness                                                   | Rocq theorem     | Rust regression                     |
|--------------------------------------|----------------------------------------------------------------|------------------|-------------------------------------|
| `i64` overflow (signed)              | `checked_next_seq_matches_i32_successor`                       | T-9.13           | (Kani is the per-function evidence) |
| `u64` overflow (unsigned)            | `checked_next_seq` (saturating on `u64 → i32`)                 | T-9.13           | (same)                              |
| `i32` predecessor at 0               | `checked_base_seq_rejects_nonpositive`                         | (boundary)       | `pre_fix_bug_*.rs` for Bug #15      |
| Epoch division by zero               | `epoch_for_block_number_rejects_invalid_domain`                | T-9.14           | `prop_t_9_7_seqnum_density.rs`      |
| Vault envelope `total_funds ≤ limit` | (separate Rocq invariant in `BugFixWithdrawTransferFailure.v`) | T-9.10″          | `prop_t_9_10_withdraw_safety.rs`    |
| Projection-boundary shift exploit    | (no single harness; covered by composition)                    | (assumption_cex) | `projection_risk_regressions.rs`    |

## 5 · Related findings

In [`formal/sage/slashing/FINDINGS.md`](../../../../../formal/sage/slashing/FINDINGS.md):

- **#8** Bounded arithmetic diverges from exact arithmetic at the
  overflow boundary.
- **#23** Arithmetic envelopes prove the vault total is bounded.
- **#15** (cross-ref) Sequence arithmetic used unchecked boundaries
  before the Bug #15 fix.

## 6 · Methodology note

This family is the canonical example of **complementary tools**:
Sage demonstrates *that* a boundary exists; Kani proves *which side*
of the boundary the production code lives on. Neither alone gives
the full picture.
