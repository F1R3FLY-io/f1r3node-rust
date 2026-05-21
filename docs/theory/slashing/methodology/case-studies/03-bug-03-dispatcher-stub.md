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
(`multi_parent_casper_impl.rs:1090-1099`), which carried the TODO
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
[`casper/tests/slashing/pre_fix_bug_3.rs`](../../../../../casper/tests/slashing/pre_fix_bug_3.rs)
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
| Rocq theorem      | `t_9_3_dispatch_complete` (`BugFixDispatcher.v:41`)                                                                                                                                                                                                                                                                                                                                                                  |
| Rust regression   | `pre_fix_bug_3.rs`, `prop_t_9_3_catchall_records.rs`                                                                                                                                                                                                                                                                                                                                                                 |
| Integration tests | `integration_t_invalid_block_hash_records.rs`, `integration_t_invalid_block_number.rs`, `integration_t_invalid_bonds_cache.rs`, `integration_t_invalid_follows.rs`, `integration_t_invalid_parents.rs`, `integration_t_invalid_repeat_deploy.rs`, `integration_t_invalid_sequence_number.rs`, `integration_t_invalid_shard_id.rs`, `integration_t_invalid_transaction.rs`, `integration_t_contains_future_deploy.rs` |
| Bug-fix manifest  | [`../../design/09-bug-fixes-and-rationale.md §9.4`](../../design/09-bug-fixes-and-rationale.md)                                                                                                                                                                                                                                                                                                                      |
| Diagram           | [Diagram 05 — Generic invalid-block dispatch (post-fix)](../../diagrams/05-seq-invalid-block-dispatch-fixed.svg)                                                                                                                                                                                                                                                                                                     |

**Stack depth: 4** (Rocq + Rust regression + integration anchors +
design).

## 6 · Lessons for the methodology

1. **A catch-all that does nothing is a *latent* bug**. The pre-fix
   dispatcher matched every slashable variant but only acted on two
   of them. The methodology requires every `match` arm to either
   have an explicit action or an explicit "no action with
   rationale" comment.
2. **Every variant in a taxonomy needs an integration test**. The
   slashing test suite has one `integration_t_invalid_*.rs` file per
   slashable variant; this is the methodology's *uniform-coverage*
   pattern that prevents the next dispatcher stub from going
   unnoticed.
3. **Cross-fix interactions matter**. Bug #3 interacts with Bug #1
   (both touch the dispatcher) and with Bug #2 (the dispatcher
   inserts into the tracker that Bug #2 races). The bug-fix manifest
   §9.11 documents the interactions; the methodology requires every
   bug fix to consider downstream interactions.
