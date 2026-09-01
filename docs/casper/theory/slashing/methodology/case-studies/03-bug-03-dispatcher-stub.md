# Case study #3 — Generic slash dispatcher stub

## 1 · Summary

Pre-fix, only `AdmissibleEquivocation` and `IgnorableEquivocation`
flowed through the record-creation path in
`multi_parent_casper_impl.rs`. The other 15 slashable variants
(`JustificationRegression`, `InvalidBondsCache`, `NeglectedInvalidBlock`,
etc.) were merely *marked invalid* in the DAG; record creation was
silently skipped. Slash enforcement relied on a later proposer's
`prepare_slashing_deploys` happening to re-surface the offender —
unreliable under adversarial proposer rotation. Post-fix, every
`is_slashable()` variant dispatches through the same record path.

## 2 · Discovery technique

**Primary**: code-walking review of the dispatcher catch-all arm
(`engine/multi_parent_casper/validation_dispatcher.rs:502-512`), which carried the TODO
*“Slash block for status except InvalidUnslashableBlock - OLD”*.

**Corroborating**: Sage differential model surfaced 15 distinct
divergences (one per slashable variant) when comparing pre-fix Rust
against the post-fix oracle. Each divergence is a Sage finding
classified `permitted_bug_fix`.

## 3 · Witness reproduction

```
cargo test -p casper --test mod -- slashing::pre_fix_bug_3
```

The fixture
[`casper/tests/slashing/pre_fix_bug_3.rs`](../../../../../../casper/tests/slashing/pre_fix_bug_3.rs)
encodes a `JustificationRegression` scenario; pre-fix the offender's
record is never created; post-fix the dispatcher creates the
record and the slash proceeds.

## 4 · Classification trace

```
threat_class       = permitted_bug_fix
ledger_status      = confirmed_fixed_bug
action             = Keep pre_fix_bug_3.rs + post-fix anchors
                     (integration_t_invalid_*.rs files for each
                      slashable variant)
```

## 5 · Evidence stack

| Layer             | Artifact                                                                                                                                                                                                                                                                                                                                                                                                             |
|-------------------|----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| Rocq theorem      | Universal terminal persistence and exact evidence eligibility in `BugFixDispatcher.v`                                                                                                                                                                                                                                                                                                                               |
| Rust regression   | `pre_fix_bug_3.rs`, `prop_t_9_3_catchall_records.rs`                                                                                                                                                                                                                                                                                                                                                                 |
| Integration tests | `integration_t_invalid_block_hash_records.rs`, `integration_t_invalid_block_number.rs`, `integration_t_invalid_bonds_cache.rs`, `integration_t_invalid_follows.rs`, `integration_t_invalid_parents.rs`, `integration_t_invalid_repeat_deploy.rs`, `integration_t_invalid_sequence_number.rs`, `integration_t_invalid_shard_id.rs`, `integration_t_invalid_transaction.rs`, `integration_t_contains_future_deploy.rs` |
| Bug-fix manifest  | [`../../design/09-bug-fixes-and-rationale.md §9.4`](../../design/09-bug-fixes-and-rationale.md)                                                                                                                                                                                                                                                                                                                      |
| TLA+ model        | `CertifiedRejectionDependency.tla` safe model and five required unsafe controls                                                                                                                                                                                                                                                                                                                                      |

**Stack depth: 4** (Rocq + Rust regression + integration anchors +
design).

## 6 · Lessons for the methodology

1. **Separate terminal state from economic evidence.** Every certified
   rejection needs a durable terminal record. Only objective equivocation can
   change the economic evidence store.
2. **Test every taxonomy member.** The slashing suite verifies persistence and
   evidence eligibility for all 29 rejection reasons.
3. **Cross-fix interactions matter**. Bug #3 interacts with Bug #1
   (both touch the dispatcher) and with Bug #2 (the dispatcher
   inserts into the tracker that Bug #2 races). The bug-fix manifest
   §9.11 documents the interactions; the methodology requires every
   bug fix to consider downstream interactions.
