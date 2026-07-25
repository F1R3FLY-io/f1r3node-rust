# PR #114 (`fix/merge-recovery-finalization`) — commit-level absorption & FV-coverage analysis

---
doc_type: discovery
discovered_by: claude-session-07b4ccc6
date: 2026-07-18
relevance: [PR-114, PR-129, sealed-floor-merge-squashed, fv-campaign]
supersedes: file-level remainder inventory (2026-07-16, PR #127 phase-1 basis)
---

## Finding

Commit-level analysis of the eight head commits of `fix/merge-recovery-finalization`
(PR #114, `0f84eb38..9ec1096e`) against `sealed-floor-merge-squashed` at `298a476f`
(post dev `32a45d02` + master `35ce65f1` merges). Four parallel read-only agents
compared mechanisms content-level (none of the SHAs are ancestors of HEAD) and
checked the FV surface (Rocq, TLA+, Kani, proptests, gate scripts) for coverage.

### Absorbed — nothing lost if PR #114's branch is dropped (5 of 8)

| Commit | Subject | Evidence at HEAD |
|---|---|---|
| `0f84eb38` | Stale invalid justification validation | `neglected_invalid_block` two-pass w/ `slash_targets` at `validate.rs:1346-1458`; Rocq T-9.13 `BugFixSlashAuthorization.v`, `AuthorizedSlashFlow.tla`, Kani harnesses, both regression tests; traceability rows confirmed. Only loss: one corrected doc-comment line (`validate.rs:20-21` still has stale wording). |
| `f584e9e9` | Inclusion-leader gating | Superseded by `deploy_inclusion_progress()` (`block_creator.rs:1604`) + richer `BranchDeployInfo`; unit + integration tests present. |
| `bec6325f` | Bounded non-leader fallback | All consts/fns present and extended (adaptive backpressure-aware cap `adaptive_fallback_ordinary_deploy_cap` at `:1858` that the branch lacks). |
| `c58fcae5` | Canonical-admission starvation fix | `stranded_count` path intact (`block_creator.rs:105,1803-1822,1931-1938`); exact starvation-scenario tests present. |
| `c32cfee9` | Canonical support + in-scope recovery | Content-identical at HEAD (`prefer_deploy_support_main_parent`, `finalized_ancestor_deploy_sigs`, `FinalityLagStats`, adaptive caps, 20 admission metrics, caps 32→128). Recovery covered by Rocq `Recovery.v` (T-NDA) + `recovery_no_double_apply.rs`; the parent-promotion heuristic itself is unit-test-only (acceptable). |

### NOT absorbed — port targets for the new branch (3 of 8)

**1. `9ec1096e` "Fix merge recovery finalization scope" — highest priority: proof↔code divergence.**
All four mechanisms absent at HEAD:
- `block_in_floor_merge_scope`: swaps `is_in_main_chain` → `is_dag_ancestor` in
  `compute_parents_post_state`'s visible-block filter. HEAD (`interpreter_util.rs:1040,1081`)
  still implements the PRE-fix predicate, while HEAD's own formal artifacts prove the
  POST-fix behavior: `finalized_floor/theories/Selection.v` maps `anc_of` → `is_dag_ancestor`,
  and `finalized-floor-verification.md:219,230` assert DAG-ancestry scope. The FV safety
  argument is ahead of the code until this lands. Sits directly on the floor-divergence /
  `ComputedPreStateMismatch` surface.
- `rejected_buffer_has_recoverable_deploys` / `local_rejected_buffer_has_recoverable_deploys`:
  canonical-won-aware recovery gating (HEAD still uses bare `buffer.non_empty()` at
  `snapshot.rs:363-368`, `block_creator.rs:2347-2352`). Matches `DeployLifecycle.tla`
  `PurgeRejectedBuf=TRUE` intent — again modeled, not implemented.
- Expired rejected-buffer purge from storage+buffer in `prepare_user_deploys_with_policy`.
- `RecoveryDeferred` propose-outcome taxonomy (propose_result/proposer/block_api/
  heartbeat_proposer) — legitimate non-leader deferral no longer logged as warn/bug.

**2. `6012c6a3` "Bound deploy admission by encoded bytes" — absent + uncovered.**
HEAD took this commit's `ORDINARY_DEPLOY_PROPOSAL_CAP=128` raise but none of the byte
machinery that motivated it: no `select_deploys_for_block(…, byte_budget)`,
`deploy_encoded_len`, `DeploySelection`, 2 MiB / 512 KiB (backpressure) budgets,
one-oversize-deploy progress guarantee, or the 5 byte metrics. Only defense against
count-within-cap / serialized-size-blowout blocks. Already flagged as an open port
decision in `docs/validation/fv-campaign-gap-analysis.md:47-52,106`.

**3. `6981b37a` "Bound DAG pressure during finality recovery" — absent + uncovered.**
Three mechanisms, none at HEAD; HEAD's heartbeat caps bound only LFB *lag*, never DAG
*width*:
- `EmptyFrontierPressure` empty-frontier backpressure across 5 propose paths + full
  config knob `empty-frontier-max-unfinalized-blocks = 64` (casper_conf, defaults.conf,
  CLI option, config_mapper).
- `prune_dag_covered_parents` parent-set compaction in `compute_snapshot`.
- `compute_parents_post_state` fast-path narrowing (`is_in_main_chain` → `is_dag_ancestor`
  + empty-deploys guard) with deliberately reversed regression assertion — related to but
  distinct from 9ec1096e's scope predicate; HEAD keeps the older broader fast path
  (`interpreter_util.rs:874-880`).
Rocq `Recovery.v` explicitly declares this capacity dimension out of scope ("orthogonal
to this safety property"), so there is no formal coverage on either side.

## Implications

1. PR #114 must NOT be merged wholesale — 5/8 head commits would collide with
   equal-or-better HEAD implementations. It also must not be closed-and-forgotten:
   three commits carry unique, load-bearing content.
2. Port order for the new branch (off the sealed-floor/dev line):
   `9ec1096e` first (closes the proof↔code gap the FV campaign already asserts),
   then `6012c6a3`, then `6981b37a`. Roughly ~1,500 lines total including tests.
3. FV follow-ups for the port branch: byte-bound and width-bound have no formal
   artifacts anywhere — decide whether to extend `DeployLifecycle.tla` (admission
   bounds) and add a width-pressure model, or accept unit-test-only coverage with
   an explicit gap note in `fv-campaign-gap-analysis.md`.
4. Minor: port `0f84eb38`'s corrected module doc-comment to `validate.rs:20-21`.
5. Full per-commit agent reports are in the session transcript (2026-07-18);
   this doc is the durable summary.

## Next steps

- /multi-review on PR #114 to finalize its review context (comment posted to the PR
  mirrors this analysis).
- Create the port branch off dev after review; PR #114 can then be closed with the
  port branch referenced, or kept as history per the maintainer's preference
  (see project memory: do not close #114/#118 unilaterally).
