# Case study #10 — PoS withdrawal transfer-failure FIXME

## 1 · Summary

Companion to Bug #4 (slash-side transfer failure), Bug #10 is on
the **withdrawal** side. Pre-fix, the Rholang `payWithdraw` flow at
`PoS.rhox:615-651` removed the validator's `withdrawers` entry
*before* the `posVault.transfer` had completed; if the transfer
failed, the validator's entry was already gone but the transfer
had not credited the receiver. The validator's funds were
effectively orphaned. Post-fix, `computeRemove` is success-gated:
the entry is removed only after the transfer succeeds, and on
failure the entry remains intact for retry.

## 2 · Discovery technique

**Primary**: FIXME audit pass surfaced the
`FIXME handle transfer failing case` comment on the withdrawal
path; the audit's hypothesis was that the same Bug #4 pattern
existed on the withdrawal side.

**Corroborating**:

- **Rocq** `BugFixWithdrawTransferFailure.v` mechanized three
  theorems (T-9.10, T-9.10′, T-9.10″): atomicity of the withdrawer-
  removal-and-transfer pair, total-funds conservation, and
  eventual-payment-under-fair-retry.
- **TLA⁺** `WithdrawFlow.tla` exhausted finite withdrawal flows
  with `Inv_TotalFundsConserved` and `Inv_WithdrawalRetryable`.
- **Hypothesis** `multi_epoch_state_machine.py` corroborated the
  retry semantics on randomized lifecycle traces.

## 3 · Witness reproduction

```
cargo test -p casper --test mod -- slashing::pre_fix_bug_10
```

The fixture
[`casper/tests/slashing/pre_fix_bug_10.rs`](../../../../../casper/tests/slashing/pre_fix_bug_10.rs)
encodes the canonical post-quarantine withdrawal with a forced
transfer failure; pre-fix the validator's entry is removed but
funds are lost; post-fix the entry remains and the withdrawal
retries.

## 4 · Classification trace

```
threat_class       = permitted_bug_fix (FIXME annotation)
ledger_status      = confirmed_fixed_bug
action             = Keep pre_fix_bug_10.rs + post-fix anchors
                     prop_t_9_10_withdraw_safety.rs +
                     hypothesis_bundle_evidence_state_machine.rs
```

## 5 · Evidence stack

| Layer            | Artifact                                                                                                                       |
|------------------|--------------------------------------------------------------------------------------------------------------------------------|
| Rocq theorems    | T-9.10, T-9.10′, T-9.10″ (`BugFixWithdrawTransferFailure.v`)                                                                    |
| TLA⁺ model       | `WithdrawFlow.tla` with `Inv_TotalFundsConserved`, `Inv_WithdrawalRetryable`, `Inv_AllRemovedValidatorsPaid`, `Inv_FairWithdrawerProgress` |
| Hypothesis       | `hypothesis_bundle_evidence_state_machine.rs`                                                                                   |
| Rust regression  | `pre_fix_bug_10.rs`, `prop_t_9_10_withdraw_safety.rs`                                                                          |
| Rholang fix      | `casper/src/main/resources/PoS.rhox` (`payWithdraw` success-gated `computeRemove`)                                              |
| Bug-fix manifest | [`../../design/09-bug-fixes-and-rationale.md §9.12`](../../design/09-bug-fixes-and-rationale.md)                              |
| Diagram          | [Diagram 11 — Withdrawal transfer-failure fix](../../diagrams/11-seq-withdrawal-flow-fix.svg)                                  |

**Stack depth: 5** (Rocq × 3 + TLA⁺ + Hypothesis + Rust + design + visual).

## 6 · Lessons for the methodology

1. **Cross-fix interactions matter for audit completeness**. Bug #10
   is the *withdrawal* sibling of Bug #4 (slash side). The audit
   that found Bug #4 should have generalized; that it took a
   separate pass to find Bug #10 is a methodology lesson — every
   FIXME audit should consider sibling code paths.
2. **The three-theorem decomposition is canonical for resource
   conservation**. T-9.10 (atomicity), T-9.10′ (conservation),
   T-9.10″ (eventual payment) decompose the load-bearing property
   into independently provable parts. The methodology prefers this
   decomposition over a single monolithic theorem.
3. **Rholang fixes need both Rocq and TLA⁺**. Rocq mechanizes the
   *transition*; TLA⁺ exhausts the *state space*. Together they
   give both unbounded mathematical evidence and finite-bound
   operational evidence.
