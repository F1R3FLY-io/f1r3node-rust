# Case study #16 — Duplicate justifications made detector projection ambiguous

## 1 · Summary

Pre-fix, if a block's justifications list contained the same
validator twice (e.g. via `Validator(v) ↦ Hash(h₁)` followed by
`Validator(v) ↦ Hash(h₂)`), the detector's validator-projection
silently picked one of the two pointers (typically the last). This
created an ambiguous projection: two semantically-distinct blocks
could project to the same view, allowing equivocation to be hidden
behind a malformed justifications list. Post-fix, duplicate
justifications are rejected before any projection happens.

## 2 · Discovery technique

**Primary**: TLA⁺ `JustificationProjection.tla` model with
`Inv_DuplicateValidatorsRejected` invariant. The invariant says:
*“before any validator-key map projection happens, the
justifications list must have no duplicate validators”*. TLC found
the violating trace immediately on the pre-fix model.

**Corroborating**: Sage `differential_bisimilarity_model.sage`
emitted a witness where Rust and Rocq oracle disagreed on the
detector's classification for a duplicate-justifications view.

## 3 · Witness reproduction

The fixture
[`casper/tests/slashing/duplicate_neglect_edges.rs`](../../../../../casper/tests/slashing/duplicate_neglect_edges.rs)
encodes the canonical duplicate-validator scenario; pre-fix the
projection is ambiguous, post-fix the input is rejected before
projection.

## 4 · Classification trace

```
threat_class       = projection_risk → permitted_bug_fix
ledger_status      = confirmed_fixed_bug
action             = Keep duplicate_neglect_edges.rs +
                     post-fix anchors uc_108_detector_duplicate_child.rs
                     + TLA+ JustificationProjection model
```

## 5 · Evidence stack

| Layer            | Artifact                                                                                                                  |
|------------------|---------------------------------------------------------------------------------------------------------------------------|
| Rocq theorem     | T-9.16 (`BugFixDuplicateJustifications.v` — implicit through detector projection theorems)                                |
| TLA⁺ model       | `JustificationProjection.tla` `Inv_DuplicateValidatorsRejected`                                                              |
| Sage             | `differential_bisimilarity_model.sage` finding                                                                              |
| Rust regression  | `duplicate_neglect_edges.rs`, `uc_108_detector_duplicate_child.rs`, `delimiter_free_record_key_collision.rs`               |
| Bug-fix manifest | [`../../design/09-bug-fixes-and-rationale.md §9.18`](../../design/09-bug-fixes-and-rationale.md)                          |

**Stack depth: 4** (Rocq + TLA⁺ + Sage + Rust + design).

## 6 · Lessons for the methodology

1. **Projection ambiguity is a *silent* bug class**. The pre-fix
   code did not panic, did not log, did not return an error; it
   silently picked one of the duplicate entries. The methodology
   requires every projection to be *well-defined-or-explicit-
   reject* — never silent.
2. **TLA⁺ at the projection layer is essential**. The bug lives
   *between* the input format (justifications list) and the
   semantic model (validator → latest message map); a TLA⁺ model
   at exactly this projection layer is what catches it.
3. **"Cosmetic" duplicates are not cosmetic**. The duplicate-
   justifications case looks like a parsing redundancy; in fact it
   has downstream semantic effect. The methodology requires every
   parsing redundancy to be either rejected or canonicalized
   before downstream use.
