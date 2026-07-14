---
task: merge-recovery-validation-port
branch: fix/dev-merge-recovery-validation
claimed_by: claude-session-07b4ccc6
started: 2026-07-13T20:00:00Z
handoff_status: in_progress
---

# Merge Recovery Corrections Port — Work Log

## Context

The branch validates the corrections developed on `fix/merge-recovery-finalization`
(preserved tip `backup/test-asi-chain-validations-f584e9e` = `f584e9e9`) against a
`dev` baseline (`e71dd897`). Commit `394ecf80` added the intentionally-red
validation suite; see `docs/validation/merge-recovery-validation-plan.md`.

## Phase 1 — minimal port (committed as `7b92f8c7`)

A prior session ported simplified re-derivations of the corrections. Result:
validation items 1–8 green (mergeable reconstruction, finalizer effect, deploy
ordering, unbonded/zero-stake equivocation, stale justification, slash matching,
inclusion leadership). Items 9–10 (two/three-writer convergence, under load)
stayed red with two failure modes:

- `unrecovered writes` — keep-one losers never re-proposed; the simplified
  `canonical_won_sigs`/leadership gate lacked the source branch's
  recovered-deploy-leader machinery.
- `NoNewDeploys` mid-round — the test harness lacked the `allow_empty_blocks`
  heartbeat mode the source branch's `TestNode` has.
- Under load, an FS-monotonicity regression (`finalized state regressed`)
  reproduced the original RCA (`RCA-asi-devnet-finality-halt`) — the merge base
  was not the node-deterministic finalized floor.

## Phase 2 — faithful subsystem port (this session, in progress)

Wholesale file swaps from `f584e9e9` (verified lossless where dev's blob exists
in source history; surgical patches where dev drifted):

**Swapped (lossless or reviewed):** `block_creator.rs`, `interpreter_util.rs`,
`dag_merger.rs`, `conflict_set_merger.rs`, `deploy_chain_index.rs`,
`finalizer.rs`, `clique_oracle.rs` (adds `FtThreshold`),
`metrics_constants.rs`, `system_deploy_user_error.rs`,
`block_dag_key_value_storage.rs` (adds floor/frontier indexes + newly-bonded
placeholder registration), `rholang_merging_logic.rs`,
`merging_logic.rs`/`event_log_index.rs` (rspace++, checked IntegerAdd overflow),
`proof_of_stake.rs` (ppm field + conversion helper). New file:
`casper/src/rust/finality/floor.rs` (finalized-floor derivation).

**Surgical patches:** `casper.rs` (CasperShardConf.fault_tolerance_threshold_ppm),
`casper_launch.rs` (ppm conversion at shard-conf build), `finalization_runner.rs`
(FtThreshold from ppm), `approve_block_protocol.rs`/`block_approver_protocol.rs`
(ppm: 0 — inert; on-chain render NOT ported, see below), `proto_util.rs`
(`slashed_block_senders`), `block_api.rs` (async 7-arg
`compute_parents_post_state`), test harness `allow_empty_blocks` port, and
mechanical call-site updates across ~12 test files.

**Deliberately NOT ported (follow-up):** on-chain θ-ppm provenance —
`standard_deploys.rs` rendering `faultToleranceThreshold` into the genesis PoS
contract, `initializing.rs` reading it back, and node-config plumbing. On this
branch the exact θ ppm is derived locally from the configured f32
(single conversion point in `casper_launch`), which is consistent across nodes
with identical configs. Genesis hashes are unchanged.

## Phase 3 — engine snapshot builders + suite reconciliation (2026-07-14)

Phase 2 committed as `57838f25`; master merged as `b19e79e0` (fixes the yanked
`spin` deny failure via lockfile refresh). The pre-push gate then surfaced 95
casper failures — one primary panic under the shared LMDB fixture lock poisoned
~80 downstream tests.

Ported in phase 3 (lossless swaps from `f584e9e9` unless noted):
- `engine/multi_parent_casper/{snapshot,dispatch,types}.rs` — the snapshot
  builders feeding `deploys_in_scope`/`rejected_in_scope`/parents into the
  leadership gate (two last touched by "Fix merge recovery finalization for
  counter deploys")
- `engine/multi_parent_casper/{block_admission,validation_dispatcher}.rs` —
  sealed-floor deploy retention (deploys stay pending until FINALIZED, the
  loser-recovery mechanism) and `bonds_cache_from_floor` validation (fixed the
  `InvalidBondsCache` slashing failure); dev tracing-target rename reapplied
- `validate.rs` gains `bonds_cache_from_floor` (surgical);
  `MultiParentCasper::rejected_deploy_buffer_contains_sig` trait method
- Test reconciliation: `uc_112` (source = FV-audited no-unbonded-stamp
  version), `recovery_cycle_spec` + `block_creator_spec` (source's corrected
  harnesses; dev's block-expiry buffer-purge test superseded — recovered
  deploys are deliberately exempt from block-expiry, buffer hygiene is
  canonical-win purge + time expiry), misfire spec (snapshot.parents set —
  proposers never have empty parents), heartbeat-mode adaptations
  (`allow_empty_blocks`) in finalization round-robin, exploratory-deploy,
  merge, limited-parent-depth, single-parent, and bridge specs, and a
  leader-aware deploy2 packaging loop in the merge spec.

## Final state (full casper release suite)

736 passed / 8 failed / 31 ignored.

Expected-red (all verified to fail IDENTICALLY on the source branch
`f584e9e9` via worktree runs — they are the UNSOLVED remainder of
RCA-asi-devnet-finality-halt, which this validation branch tracks):
- `map_cell_convergence_spec::{two,three}_writers_converge[,_under_load]`
  (keep-one loser recovery incomplete upstream too)
- `map_cell_convergence_spec::unresolved_user_frontier_has_one_deploy_inclusion_leader`
- `recovery_cycle_spec::recovery_cycle_rejected_deploy_retries_while_source_is_visible`
- `runtime_manager_test::{bridge_query_survives_multi_parent_merge,stale_diff_application_corrupts_merged_state}`
  (known stale-diff merge defect, red on both branches)

Flaky (passes solo, fails occasionally under parallel load; pre-existing):
- `approve_block_protocol_test::should_continue_collecting_if_not_enough_signatures`

Notable wins vs the source branch: FS-monotonicity under load fixed, slashing
bonds-cache green, merge spec multi-parent test green (red on source), misfire
double-execution guards green (absent on source), clippy clean.

## Phase 4 — admission-starvation fixes + LMDB registry (2026-07-14, branch fix/dem-merge-recovery-addl-pre)

PR #118's CI failed beyond the expected-red set: ALL Heavy Pipeline smoke and
integration jobs. Two root causes, both fixed on `fix/dem-merge-recovery-addl-pre`
(= dev + merge of the validation branch + these fixes):

1. **Startup crash on every production node**: the floor/frontier DAG indexes
   were never registered in `rnode_key_value_store_manager.rs`'s LMDB db
   mapping — `LmdbDirStoreManager` fails closed on undeclared stores
   (`Key floor-index was not found`), while the in-memory test manager creates
   stores on demand, so no test ever caught it. Ported the source branch's
   registry fix (`8c7c8073`).
2. **Deploy-admission starvation** under the inclusion-leadership gate: ported
   the source branch's three new commits (`bec6325f` bounded non-leader
   admission fallback, `c32cfee9` inclusion recovery + canonical support,
   `c58fcae5` canonical admission starvation) via wholesale swaps of
   `block_creator.rs`, `snapshot.rs`, `metrics_constants.rs`,
   `block_creator_spec.rs`, plus `max_user_deploys_per_block` 32→128 at four
   config sites and upstream's panic→warn downgrade in
   `block_metadata_store.rs` (non-contiguous height map is diagnostic, not
   fatal).

Verification: workspace compiles; clippy `--all-targets -D warnings` clean;
full casper suite **740 passed / 6 failed** — `two_writers_converge` and
`three_writers_converge` flipped GREEN (red on both branches before);
remaining expected-red: under-load convergence, leadership spec,
recovery-cycle retries, stale-diff pair (+ the approve_block flake). Local
standalone smoke reproducing CI's check: node reaches Running and LFB #10
within budget (SMOKE PASS) — previously crashed at startup.

## Next steps

- Commit phase 3 (user-driven /quick-commit).
- Pushing requires `SKIP_TESTS=1` or crate-scoped `TEST_CRATES` while the
  expected-red set exists — consistent with the validation plan's
  intentionally-red CI posture. Alternative: leave casper out of pre-push
  and let CI document the red matrix.
- Follow-up candidates: on-chain θ-ppm provenance chain
  (standard_deploys/initializing/node config); the actual convergence
  recovery completion (the expected-red set) — new engineering beyond the
  source branch.
