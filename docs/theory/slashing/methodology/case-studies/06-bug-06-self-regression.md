# Case study #6 — Self-regression slips through

## 1 · Summary

Pre-fix, the validator at `validate.rs:875-898` had a filter that
dropped self-regression detection (a validator citing its own past
block as its latest message *out of sequence-number order*). The
filter masked a class of slashable equivocations where the validator
attempts to "amend" its own history by pointing at an older block
than its actual latest. Post-fix, self-regression is detected as
equivocation.

## 2 · Discovery technique

**Primary**: Sage `differential_bisimilarity_model.sage` produced
divergence witnesses where the Rust path classified self-regression
as `Valid` but the oracle classified it as a slashable equivocation.

**Corroborating**: Hypothesis state-machine search for sequences
involving self-citation of earlier blocks; the witness was minimized
to a 3-action sequence.

## 3 · Witness reproduction

```
cargo test -p casper --test mod -- slashing::pre_fix_bug_6
```

The fixture
[`casper/tests/slashing/pre_fix_bug_6.rs`](../../../../../casper/tests/slashing/pre_fix_bug_6.rs)
encodes the canonical self-regression scenario: validator `v0`
signs blocks `b1, b2` at sequence numbers `1, 2`; then signs `b3`
at sequence 3 with `b1` as the latest message instead of `b2`. The
post-fix path classifies `b3` as equivocation.

## 4 · Classification trace

```
threat_class       = permitted_bug_fix
ledger_status      = confirmed_fixed_bug
action             = Keep pre_fix_bug_6.rs + post-fix anchors
                     uc_06_self_regression.rs +
                     uc_37_self_regression_dag_level.rs
```

## 5 · Evidence stack

| Layer            | Artifact                                                                                   |
|------------------|--------------------------------------------------------------------------------------------|
| Rocq theorem     | T-9.6 + T-9.4 (transfer-failure related; cross-fix interaction)                            |
| Rust regression  | `pre_fix_bug_6.rs`, `prop_t_9_6_self_regression.rs`, `uc_06_self_regression.rs`            |
| DAG-level test   | `uc_37_self_regression_dag_level.rs`                                                       |
| Bug-fix manifest | [`../../design/09-bug-fixes-and-rationale.md §9.7`](../../design/09-bug-fixes-and-rationale.md) |

**Stack depth: 3** (Rocq + Rust regression + design).

## 6 · Lessons for the methodology

1. **Filters dropped during refactor are silent defects**. The
   self-regression filter was present in the Scala original but
   was dropped during the Rust port; no failing test surfaced the
   gap because no test exercised self-regression. The methodology
   requires every dropped check to be paired with an explicit
   *"removed because X"* comment.
2. **DAG-level tests catch what block-level tests miss**. The
   `uc_37_self_regression_dag_level.rs` test exercises the DAG
   topology that self-regression requires; a block-level test
   alone would have allowed the bug to recur in a topology variant.
3. **Self-regression is a *temporal* equivocation**. The
   methodology's threat model distinguishes spatial equivocation
   (two blocks at the same sequence) from temporal equivocation
   (two latest-message claims for the same prefix); both are
   slashable.
