# FV Campaign Gap Analysis — porting the #114 verification campaign onto dev

Status: Phase A (artifact port) staged on `fv/merge-recovery-fv-campaign`.
Companion to [merge-recovery-validation-plan.md](merge-recovery-validation-plan.md).

## Purpose

PR #114 (`fix/merge-recovery-finalization`) carries both production merge-recovery
corrections and a formal-verification campaign, but is too large and too
conflicted to merge as-is. The production corrections reach `dev` through
re-derived ports (#118, contained in #122). The FV campaign does not. This
branch ports the FV campaign as a follow-on, and this document records the gap
analysis that scoped it.

## PR lineage findings

- `#90` (`sealed-floor-merge-wip`) is a strict ancestor of `#114`
  (`fix/merge-recovery-finalization`). The **entire FV campaign lives in #90**;
  the two tips have zero differences under `formal/`, `docs/theory/`, and
  `scripts/check-*-ALL.sh`.
- `#114` adds exactly 7 production commits on top of #90:
  `7bfb3c55`, `0f84eb38`, `f584e9e9`, `bec6325f`, `c32cfee9`, `c58fcae5`,
  `6012c6a3`.
- `#122` (`fix/dem-merge-recovery-addl-pre`) **contains #118**
  (`fix/dev-merge-recovery-validation`, merged in at `3d269c95`) plus four
  additional correction commits. Merging #122 lands #118's ports automatically.
- Neither #122 nor #118 nor #123 nor #124 touches any FV path.
- `dev` already carries the slashing FV campaign (from `analysis/slashing`,
  latest `94e57733`). That commit is in #114's history, so #114's slashing
  artifacts are strict extensions of dev's — no clobber risk.

## Defect matrix — #114's claimed fixes vs. what #122 ports

Verified at symbol level against the `fix/dem-merge-recovery-addl-pre` tree.

| # | Fix (#114 commit) | In #122 tree? | Evidence |
|---|---|---|---|
| 1 | Finalized-floor merge base, floor/frontier indexes | ✅ ported | #118 phase 2 (`57838f25`); `d82fbc54` registers LMDB stores |
| 2 | Rejected-deploy buffering / recovery | ✅ ported | #118 phases 1–3 |
| 3 | Sealed-floor deploy retention until FINALIZED | ✅ ported | #118 phase 3 (`c363f552`) |
| 4 | Deploy-inclusion leadership (`f584e9e9`) | ✅ ported | `deploy_inclusion_leader` in tree |
| 5 | Bounded non-leader fallback admission (`bec6325f`) | ✅ ported | `NON_LEADER_FALLBACK_ORDINARY_DEPLOY_CAP` in tree |
| 6 | Finality-lag backpressure + admission metrics (`c32cfee9`) | ✅ ported | `BLOCK_CREATOR_DEPLOY_ADMISSION_BACKPRESSURE_METRIC` in tree |
| 7 | Canonical admission starvation / stranded in-scope recovery (`c58fcae5`) | ✅ ported | `..._STRANDED_IN_SCOPE_METRIC` in tree |
| 8 | Stale invalid-justification validation (`0f84eb38`) | ✅ ported | epoch-scoped slash matching + regression test in `casper/src/rust/validate.rs` |
| 9 | Counter-deploy merge recovery finalization (`7bfb3c55`) | ✅ re-derived | `frontier_chase_max_lag` is now a config value (`casper_conf.rs`); `binary_data_is_single_number` in `merging/dag_merger.rs` |
| 10 | **Byte-bounded deploy admission (`6012c6a3`)** | ❌ **not ported** | no byte-budget/encoded-bytes admission symbols anywhere in the #122 tree |

**Open defect gap:** `6012c6a3` "Bound deploy admission by encoded bytes"
(+292 lines in `block_creator.rs`, 9 metrics incl.
`BLOCK_CREATOR_DEPLOY_ADMISSION_BYTE_CAP_HIT_METRIC`,
`USER_DEPLOY_BACKPRESSURE_BYTE_PROPOSAL_BUDGET`). Needs its own port decision:
either fold into the #90 rework or port as a small standalone PR.

## FV campaign inventory (this branch, Phase A)

Ported verbatim from the #114/#90 tip: 122 new files, 39 extended
(38 slashing artifacts + `.gitignore` for `.localtools`).

| Subsystem | Rocq | TLA+ | Z3 | Sage | Wolfram | Dossier |
|---|---|---|---|---|---|---|
| finalized-floor | ✚ | ✚ | ✚ | ✚ | ✚ | `docs/theory/finalized-floor/finalized-floor-verification.md` |
| fork-choice (LMD-GHOST) | ✚ | ✚ | ✚ | ✚ | ✚ | `docs/theory/fork-choice/fork-choice-verification.md` |
| merge-algebra | ✚ | — | ✚ | — | — | `docs/theory/merge-algebra/merge-algebra-verification.md` |
| deploy-lifecycle | — | ✚ | — | — | — | (gated via `check-deploy-lifecycle-ALL.sh`) |
| slashing | ext | ext | — | ext | — | `docs/theory/slashing/slashing-verification.md` (ext) |

Gate scripts (local-only by design — #114 made **no CI workflow changes**):
`scripts/check-{finalized-floor,fork-choice,merge-algebra,slashing,deploy-lifecycle}-ALL.sh`.

## Phase plan

1. **Phase A (this branch)** — pure FV artifacts: `formal/`, `docs/theory/`,
   gate scripts, `.gitignore`. Additive; only overlap is the slashing
   extension. Mergeable independently of code state, but see caveat below.
2. **Phase B (after #122/#123/#124 land)** — FV-derived Rust tests:
   `casper/tests/fork_choice/` property tests (Batch 1),
   `casper/tests/batch2/validate_test.rs` (Batches 2–3), and the small src
   seams they exercise (`finality/floor.rs` helpers, `block_creator.rs`,
   `finalization_runner.rs`). These compile only against the ported code —
   which #122 provides.
3. **Phase C** — refresh dossier source-code citations against post-merge dev
   (dossiers cite Rust line numbers; #114 itself needed a drift-refresh
   commit, `5160ba43`), then run all five gate scripts and record results.

### Phase A caveat — gate scripts reference Phase B tests

The gate scripts invoke `cargo test` targets that arrive in Phase B
(`finalized_floor::` proptests, `fork_choice::` proptests,
`batch2::validate_test`, `loom_frontier_floor_cache`). Until Phase B lands,
those steps fail (some are marked fail-soft). The scripts are ported verbatim
for traceability rather than trimmed.

## Dependency on #90 rework

The FV campaign's proofs/models describe the sealed-floor semantics
implemented in #90. #122/#118 port re-derived versions of that code onto dev;
the reworked #90 is expected to merge just ahead of this branch. Before
merging this branch, confirm the #90 rework did not change verified semantics
(floor selection, merge algebra combine rules, fork-choice tie-breaks,
FtThreshold decision rule) — if it did, the affected proofs and dossiers must
be re-checked, not just re-cited.

## Unresolved items

- Port decision for `6012c6a3` (byte-bounded admission) — see defect matrix.
- Decision: keep gate scripts local-only (per #114's intent) or wire into CI.
- #118 remains open as the tracker for the "unsolved remainder" validation
  matrix; #122 subsumes its code. Close or repurpose after #122 merges.
