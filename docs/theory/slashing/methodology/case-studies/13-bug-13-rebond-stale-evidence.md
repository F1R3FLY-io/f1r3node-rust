# Case study #13 — Same-key rebond could inherit stale evidence

## 1 · Summary

Pre-fix, a validator who was slashed, unbonded, withdrew, and then
rebonded with the *same key* in a later epoch could be slashed
again on the *old* evidence — the receive path did not check that
the evidence's epoch matched the post-rebond activation epoch.
Post-fix, the authorization predicate's
`slash_evidence_epoch_matches_target` clause rejects stale evidence.

## 2 · Discovery technique

**Primary**: Sage `epoch_churn_attack_model.sage` enumerated
lifecycle traces involving same-key rebond and emitted witnesses
where stale evidence from a previous bond was admissible.

**Corroborating**: TLA⁺ `AuthorizedSlashFlow.tla`
`Inv_RebondRejectsStaleEvidence` exhausted finite rebond scenarios
and confirmed the post-fix authorization predicate is sufficient.

## 3 · Witness reproduction

The fixtures
[`casper/tests/slashing/epoch_evidence_rollover.rs`](../../../../../casper/tests/slashing/epoch_evidence_rollover.rs)
and
[`casper/tests/slashing/rebonded_identity_boundary.rs`](../../../../../casper/tests/slashing/rebonded_identity_boundary.rs)
encode the bond → slash → unbond → withdraw → rebond → submit-stale-
evidence scenario; pre-fix the stale evidence is accepted,
post-fix it is rejected.

## 4 · Classification trace

```
threat_class       = projection_risk → permitted_bug_fix
ledger_status      = confirmed_fixed_bug
action             = Keep epoch_evidence_rollover.rs +
                     rebonded_identity_boundary.rs + Kani harness
                     received_authorization_requires_evidence_epoch_on_bounded_domain
```

## 5 · Evidence stack

| Layer            | Artifact                                                                                                                           |
|------------------|------------------------------------------------------------------------------------------------------------------------------------|
| Rocq theorem     | T-9.13, T-9.11 (`BugFixSlashAuthorization.v` clauses)                                                                              |
| TLA⁺ invariant   | `AuthorizedSlashFlow.tla` `Inv_RebondRejectsStaleEvidence`                                                                         |
| Sage             | `epoch_churn_attack_model.sage`                                                                                                    |
| Kani harnesses   | `received_authorization_requires_evidence_epoch_on_bounded_domain`, `slash_evidence_epoch_matches_target_matches_epoch_projection` |
| Rust regression  | `epoch_evidence_rollover.rs`, `rebonded_identity_boundary.rs`, `stale_evidence_filtered.rs`                                        |
| Bug-fix manifest | [`../../design/09-bug-fixes-and-rationale.md §9.15`](../../design/09-bug-fixes-and-rationale.md)                                   |

**Stack depth: 5** (Rocq + TLA⁺ + Sage + Kani + Rust regression + design).

## 6 · Lessons for the methodology

1. **Validator identity ≠ validator key**. A validator's *identity*
   is its bond epoch + key; the same key in a new epoch is a new
   identity. The methodology's `epoch_churn_attack_model` is the
   canonical search engine for identity-confusion bugs.
2. **Stale-evidence attacks need temporal models**. The bug is
   invisible in any model that abstracts away epochs;
   `epoch_churn_attack_model.sage` was the *minimum* model that
   could express the attack.
3. **Authorization predicates are *necessarily-conjunctive***. Each
   clause defends against a distinct attack; removing any clause
   exposes a distinct vulnerability. The Kani harnesses for the
   clause-necessity (one per clause) prove this exhaustively on the
   bounded domain.
