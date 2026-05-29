# Slashing Decision Records

This file records protocol decisions whose alternatives were considered during
the 2026-05 vulnerability-resolution pass. It is not a work log; it is the
stable rationale for the selected semantics.

## DR-1 — Validator Lifetime Identity

**Decision.** Slashing evidence is scoped to an epoch-scoped validator lifetime.
For the implemented Rust rule, evidence is authorized only when:

```text
authorized(hash, v, e) ≜
  invalidEvidence[hash] = (v, e, …)
  ∧ currentEpoch = e
  ∧ parentPreStateBond(v) > 0
```

where `v` is the validator public key and `e` is the target activation epoch.
The current implementation derives `e` from block numbers and `epochLength`;
the bond check is evaluated against the block's actual parent pre-state.

**Rationale.** A raw public key is not enough to distinguish an old validator
lifetime from a later same-key rebond. Epoch scoping prevents stale evidence
from slashing a later lifetime.

**Alternatives considered.**

| Alternative | Consequence |
| --- | --- |
| Permanent key retirement | Stronger and simpler, but operationally stricter because a withdrawn key can never be reused. |
| Slash old offenses after rebond | Preserves old raw-key semantics, but allows stale evidence to slash new stake and was rejected as unsafe. |
| Full PoS `bondEpochs` state | More precise activation-lifetime tracking, but requires a larger Rholang state migration. This remains the preferred future refinement if epoch scoping is considered too conservative. |

## DR-2 — Slash Candidate Source

**Decision.** Proposers derive slash candidates from the authorized invalid
evidence index rather than only `invalid_latest_messages`.

**Rationale.** Invalid blocks are inserted as invalid and do not necessarily
become latest messages. Using only invalid latest messages can leave valid
evidence recorded but never proposed for slashing.

**Alternatives considered.**

| Alternative | Consequence |
| --- | --- |
| Record-store driven candidates | Requires `EquivocationRecord` to carry invalid block hashes for all slashable statuses. Useful future cleanup, but larger migration. |
| Minimal invalid-latest patch | Smaller code change, but retains the coupling between slash liveness and latest-message maintenance. |

## DR-3 — Received Slash Deploy Authorization

**Decision.** A received slash deploy is valid only if it is locally authorized
before replay. The issuer must be the block sender, the invalid hash must be a
known invalid block, the target epoch must match the evidence epoch and current
epoch, the offender must be currently bonded, and a block may include at most
one slash deploy per `(validator, epoch)` target.

**Alternatives considered.**

| Alternative | Consequence |
| --- | --- |
| Keep PoS deployer-slash fallback | Allows unknown invalid hashes to slash the deployer and makes authorization implicit in Rholang replay. Rejected because block validation must reject unauthorized slash deploys before state transition. |
| Trust proposer-generated slash deploys | Insufficient for received blocks because adversarial proposers choose block bodies. |

## DR-4 — Duplicate Justifications

**Decision.** Blocks with duplicate justification validators are invalid before
detector projection.

**Rationale.** The detector projects justifications into a map keyed by
validator. Rejecting duplicates makes projection deterministic and prevents
order-sensitive evidence visibility.

**Alternatives considered.**

| Alternative | Consequence |
| --- | --- |
| Keep first duplicate | Deterministic but silently accepts malformed evidence. |
| Keep last duplicate | Matches some map-collection behavior but preserves adversarial order dependence. |

## DR-5 — Checked Sequence Arithmetic

**Decision.** Sequence arithmetic used by slashing evidence must be checked.
`seq − 1` is skipped for the legacy `EquivocationRecord` path if it would
underflow, and proposer `seq + 1` must fit in `i32`.

**Alternatives considered.**

| Alternative | Consequence |
| --- | --- |
| Wrapping arithmetic | Can corrupt record keys and differ between debug and release behavior. |
| Saturating arithmetic | Avoids panic but aliases boundary values into real record keys. |

## DR ↔ Bug ↔ Theorem cross-reference

Each Decision Record was motivated by a specific bug class and is
discharged by a specific theorem. The table below makes the
linkage explicit so a reader can move bidirectionally between the
operational decision (this file), the bug taxonomy (§09), and the
formal claim (verification §9 / MainTheorem.v).

| DR   | Bug    | Headline theorem                                                | Rocq alias                                  |
|------|--------|-----------------------------------------------------------------|---------------------------------------------|
| DR-1 | #13    | T-9.12 — Stale evidence cannot slash a same-key rebond         | `main_T9_12_stale_evidence_not_authorized`  |
| DR-2 | #14    | T-LivenessGap — Authorized invalid-block evidence index path   | `deploy_epoch_matches_target`               |
| DR-3 | #12    | T-9.13 — Unknown / unauthorized slash deploys are no-ops       | `main_T9_13_unknown_slash_evidence_noop`    |
| DR-3 | (corollary) | T-Auth — Auth-token check rejects invalid tokens          | `main_TAuth_invalid_token_noop`, `main_TAuth_valid_token_equiv` |
| DR-4 | #16    | T-9.15 — Duplicate justifications rejected before projection   | `main_T9_15_duplicate_justifications_rejected` |
| DR-5 | #15    | T-9.14 — Checked sequence arithmetic at boundary               | `main_T9_14_checked_pred_positive`          |

DR-1 through DR-5 each cover one of the Rust-source-confirmed bug
classes #12..#16. The Rocq aliases above all live in
`formal/rocq/slashing/theories/MainTheorem.v` and resolve to the corresponding
underlying lemmas in the relevant `BugFix*.v` files
(e.g. `BugFixSlashAuthorization.v`, `ValidatorLifetime.v`,
`BugFixSeqArithmetic.v`, `BugFixDuplicateJustifications.v`).

## DR-6 — Removal of the Rust↔Scala bisimilarity (2026-05-29)

**Decision.** Remove the Rust↔Scala bisimilarity development: the Rocq module
`formal/rocq/slashing/theories/Bisimulation.v`, the T-13/T-14/T-15 bisimilarity
components of `MainTheorem.v` (§5–§8: `main_T13_slash_bisim`, `main_T14_*`,
`main_T15_*`, `main_bisimilarity_theorem`, `main_bisimilarity_strong`,
`pipeline_step`/`t_15_pipeline_step_preserves_R`), the five Rust property tests
that mirrored them (`prop_t_13a_bonds_bisim`, `prop_t_13b_records_bisim`,
`prop_t_13c_forkchoice_bisim`, `prop_t_14_weak_barbed_equiv`,
`prop_t_15_bisim_under_workload`), and the corresponding build-manifest and
documentation entries.

**Rationale.** The migration to the cost-accounted-rho architecture means the
Rust slashing implementation no longer has a corresponding Scala implementation
to be bisimilar *to* — the two architectures are no longer comparable. The
bisimilarity's purpose (finding Rust/Scala divergences during the port) is
complete. Git history preserves the removed proofs.

**Preserved (explicitly NOT removed).** The headline safety theorem
`main_slashing_algorithm_correct` and all T-1..T-12 / T-9.x detection,
slash-effect, two-level-closure, and bug-fix theorems are independent of the
bisimilarity and remain (in `PoSContract.v`, `EquivocationDetector.v`,
`TwoLevelSlashing.v`, the `BugFix*.v` modules). The slashing Rocq build closes
with zero admissions/axioms after the removal (verified).

**Distinct from — and NOT to be confused with — the triple-bisimilarity suite.**
The *triple*-bisimilarity differential tests (`triple_bisim_driver.rs`,
`prop_t_triple_bisim_{dispatch,forkchoice,records}.rs`; methodology doc
`methodology/differential-and-metamorphic/03-triple-bisimilarity.md`) run the
same trace through **three** implementations — the Rust **harness** (Tier 3),
the Rocq-derived **oracle** (Tier 2), and the **production** adapter (Tier 1) —
with **no Scala leg**. They check Rust↔Rocq↔production agreement and remain a
valuable cross-implementation check (more so under the cost-accounted rework).
**They are KEPT.** (One of them, `prop_t_triple_bisim_dispatch.rs`, was briefly
removed in error during this pass and restored.)

**Alternatives considered.**

| Alternative | Consequence |
| --- | --- |
| Re-scope the bisimilarity to relate Rust to the *spec's* model | Rejected: the spec's model is realized by the very Rust implementation under test; a Rust↔spec "bisimilarity" collapses to the existing simulation/refinement results already covered by the triple-bisim oracle tier and the cost-accounted-rho translation-faithfulness proofs. |
| Keep the Rust↔Scala proofs as historical reference | Rejected: dead proofs over a removed counterpart are maintenance debt; git history is the reference. |
