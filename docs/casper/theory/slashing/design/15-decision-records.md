# Slashing Decision Records

This file records protocol decisions whose alternatives were considered during
the 2026-05 vulnerability-resolution pass. It is not a work log; it is the
stable rationale for the selected semantics.

## DR-1 — Validator Lifetime Identity

**Decision.** Validator lifetime identity is the monotonic PoS bond generation
`(validator, generation)`. Activation epoch remains a separate slash window.
For the implemented Rust rule, evidence is authorized only when:

```text
authorized(hash, v, g, e) ≜
  certifiedEvidence[hash] = (v, g, e, …)
  ∧ currentEpoch = e
  ∧ canonicalMergedPreStateGeneration(v) = g
  ∧ canonicalMergedPreStateBond(v) > 0
```

where `v` is the validator public key, `g` is its PoS bond generation, and `e`
is the target activation epoch. A generation changes only after a completed
withdrawal followed by a fresh bond. The implementation derives `e` from the
actual proposed block number and `epochLength`; bond and generation are loaded
together from the exact canonical merged pre-state root used by replay.

**Rationale.** A raw public key is not enough to distinguish an old validator
lifetime from a later same-key rebond, while an ordinary epoch boundary is not
a bond lifecycle transition. Generation identity prevents stale evidence from
slashing later stake without incorrectly manufacturing a new identity at every
epoch. The current-epoch condition independently prevents stale authorization
from being replayed outside its activation window.

**Alternatives considered.**

| Alternative | Consequence |
| --- | --- |
| Permanent key retirement | Stronger and simpler, but operationally stricter because a withdrawn key can never be reused. |
| Slash old offenses after rebond | Preserves old raw-key semantics, but allows stale evidence to slash new stake and was rejected as unsafe. |
| Epoch as validator incarnation | Confuses a scheduling window with validator custody and silently creates a new identity at every epoch boundary. |
| Monotonic PoS bond generation | Selected: exact lifetime identity committed in on-chain PoS state and serialized in certified block metadata. |

## DR-2 — Slash Candidate Source

**Decision.** Proposers derive slash candidates from the authorized invalid
evidence indexes rather than only `invalid_latest_messages`.

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
known invalid block or a canonical objective pair, the target epoch must match
every evidence epoch and the current epoch, the evidence generation must match
the canonical pre-state generation, the offender must be positively bonded at
that same root, and a block may include at most one slash deploy per
`(validator, generation)` target.

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
| DR-7 | #18    | T-9.13″ — Slash targets are fetched before authorization       | `main_T9_13_slash_target_is_dependency`, `main_T9_13_missing_local_evidence_waits`, `main_T9_13_tracker_witness_not_slash_evidence` |
| DR-8 | #19    | Objective evidence is canonical and lifetime-scoped            | `objective_equivocation_correct`            |

DR-1 through DR-5 cover the Rust-source-confirmed bug classes #12..#16; DR-7
covers the receiver-local availability race in #18; DR-8 applies DR-1's
lifetime identity to the arrival-order race in #19. The Rocq aliases live in
the slashing and finalized-floor `MainTheorem.v` capstones and resolve to the
corresponding underlying lemmas in the relevant `BugFix*.v` files
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

## DR-7 — Slash-Evidence Dependency Closure

**Decision.** Every successful slash deploy contributes its target invalid-block
hash to the containing block's dependency set. Direct block processing and
buffer resolution use the same readiness predicate. A DAG entry or invalid-block
index entry satisfies the dependency; equivocation-tracker membership alone does
not satisfy a slash-evidence dependency. The dependency projection is
deterministically deduplicated.

**Rationale.** Received-slash authorization needs immutable target metadata,
including sender, height, and invalid status. Tracker membership proves that a
validator has an equivocation record but does not materialize that target block.
Treating tracker-only state as ready made validity depend on receiver-local
arrival order: one node could authorize after seeing the target while another
could reject the same block as `UnauthorizedSlashDeploy`. Declaring the target as
a dependency makes missing evidence an availability state: buffer, fetch, then
run the same deterministic authorization predicate.

**Alternatives considered.**

| Alternative | Consequence |
| --- | --- |
| Authorize immediately and retry after rejection | Records an objectively valid block as invalid on receivers that have not yet materialized the evidence. |
| Accept equivocation-tracker membership as readiness | Bypasses the metadata required by authorization and preserves receiver-local behavior. |
| Add a slash-specific recovery path outside dependency handling | Duplicates readiness semantics and permits the direct and buffered paths to drift. |

## DR-8 — Canonical Objective Equivocation Evidence

**Decision.** Equal-sequence sibling observations are stored durably as an
ordered hash group keyed by `(validator, bond_generation, sequence)`.
Authorization first selects the active canonical pre-state generation, then
groups that set by activation epoch, and only then selects the two
lexicographically least hashes. A same-generation, same-epoch pair is objective
evidence without reference to either node's local invalid flag. Both hashes are
dependencies.

The affected generation is excluded from voting, including later descendants
in that generation. Old evidence does not permanently retire a later same-key
generation.
A structural cross-epoch collision still suppresses unary fallback for its own
`(validator, sequence)` group, because choosing the locally second sibling
would reintroduce arrival-order authority. It does not suppress an independent
unary fault at another sequence.

**Rationale.** Parallel validators can complete both sibling validations
against stale snapshots and can classify opposite siblings as locally invalid.
Objective proof must therefore be a relation over two immutable messages. DR-1
also forbids turning that relation into permanent raw-key retirement. Grouping
by generation and epoch before choosing a pair prevents a lexicographically
smaller old hash from hiding two current eligible siblings.

**Alternatives considered.**

| Alternative | Consequence |
| --- | --- |
| Unary locally invalid hash | Opposite arrival orders authorize opposite evidence and can split honest block validation. |
| Select the first two hashes before epoch grouping | A cross-epoch low hash can hide a valid same-epoch pair. |
| Permanent public-key voter exclusion | Simpler, but contradicts DR-1 and rejects a later same-key validator lifetime. |
| Retroactively invalidate both siblings | Rewrites accepted history and can invalidate descendants or finalized state. |
