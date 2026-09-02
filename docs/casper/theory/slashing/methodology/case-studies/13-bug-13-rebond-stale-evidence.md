# Case study #13 — Same-key rebond could inherit stale evidence

## 1 · Summary

Before the fix, an old slash record could target a new bond under the same key.
The receive path did not bind evidence to the current bond generation.
The fixed predicate checks the bond generation and the activation epoch independently.

## 2 · Discovery technique

**Primary**: Sage `epoch_churn_attack_model.sage` enumerated
lifecycle traces involving same-key rebond and emitted witnesses
where stale evidence from a previous bond was admissible.

**Corroborating**: TLA⁺ `AuthorizedSlashFlow.tla`
`Inv_StaleGenerationCannotSlashRebondedKey` exhausts finite rebond scenarios.
The invariant confirms that the generation check rejects stale evidence.

## 3 · Witness reproduction

The fixtures
[`casper/tests/slashing/epoch_evidence_rollover.rs`](../../../../../../casper/tests/slashing/epoch_evidence_rollover.rs)
and
[`casper/tests/slashing/rebonded_identity_boundary.rs`](../../../../../../casper/tests/slashing/rebonded_identity_boundary.rs)
encode the bond → slash → unbond → withdraw → rebond → submit-stale-
evidence scenario; pre-fix the stale evidence is accepted,
post-fix it is rejected.

## 4 · Classification trace

```
threat_class       = projection_risk → permitted_bug_fix
ledger_status      = confirmed_fixed_bug
action             = Keep epoch_evidence_rollover.rs +
                     rebonded_identity_boundary.rs + Kani generation and epoch harnesses
```

## 5 · Evidence stack

| Layer            | Artifact                                                                                                                           |
|------------------|------------------------------------------------------------------------------------------------------------------------------------|
| Rocq theorem     | T-9.13, T-9.11 (`BugFixSlashAuthorization.v` clauses)                                                                              |
| TLA⁺ invariant   | `AuthorizedSlashFlow.tla` `Inv_StaleGenerationCannotSlashRebondedKey`                                                               |
| Sage             | `epoch_churn_attack_model.sage`                                                                                                    |
| Kani harnesses   | `received_authorization_requires_matching_evidence_generation`, `received_authorization_requires_matching_canonical_generation`, `received_authorization_requires_current_epoch_on_bounded_domain`, `received_authorization_requires_evidence_epoch_on_bounded_domain` |
| Rust regression  | `epoch_evidence_rollover.rs`, `rebonded_identity_boundary.rs`, `stale_evidence_filtered.rs`                                        |
| Bug-fix manifest | [`../../design/09-bug-fixes-and-rationale.md §9.15`](../../design/09-bug-fixes-and-rationale.md)                                   |

**Stack depth: 5** (Rocq + TLA⁺ + Sage + Kani + Rust regression + design).

## 6 · Lessons for the methodology

1. **Validator identity ≠ validator key**. A validator lifetime uses the key and its monotonic bond generation.
   An epoch boundary alone does not change the validator lifetime.
2. **Stale-evidence attacks need lifecycle models**. The bug is invisible when a model omits withdraw and rebond transitions.
   `epoch_churn_attack_model.sage` includes the minimum lifecycle that can express the attack.
3. **Authorization predicates are *necessarily-conjunctive***. Each
   clause defends against a distinct attack; removing any clause
   exposes a distinct vulnerability. The Kani harnesses for the
   clause-necessity (one per clause) prove this exhaustively on the
   bounded domain.
