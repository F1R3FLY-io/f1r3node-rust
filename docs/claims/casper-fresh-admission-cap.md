# Claim: deploy admission follows the backlog while finality advances

```yaml
claim_id: CLAIM-CASPER-FRESH-ADMISSION-CAP-001
artifacts:
  - casper/src/rust/blocks/proposer/block_creator.rs   # admission caps, FinalityProgress, OrdinaryCapSource, and their tests
  - casper/src/rust/blocks/proposer/proposer.rs        # ProductionBlockCreator keeps the FinalityProgress across proposals
status: pending
adapter: agentic
mechanization: none
references:
  - https://github.com/F1R3FLY-io/f1r3node-rust/issues/24   # test_load deploys not finalized within 45 s
  - https://github.com/F1R3FLY-io/f1r3node-rust/issues/24#issuecomment-6094239136   # the non-leader fallback cap of 8 is the mechanism
  - GitHub Actions run 38017311024                          # weekend soak preflight: high-phase failure with cap 8
  - https://github.com/F1R3FLY-io/f1r3node-rust/pull/695    # hotfix PR
```

## Context

Soak 38017311024 failed `test_load` in the high phase. While unfinalized user deploys are in scope, only the deploy-inclusion leader admits ordinary deploys at the normal cap. Every other validator takes the fallback path, and `adaptive_fallback_ordinary_deploy_cap` gives it 8 deploys for each block. The cap rises to 16 at an oldest fresh deploy age of 120 s and to 32 at 300 s. Both ages are above the 45 s finalization window of `test_load`.

In the high phase, validator2 held 26, 33, 25, 17, and 9 fresh local deploys and selected 8 each time. Its oldest fresh deploy aged from 7.5 s to 18.0 s, and one deploy did not finalize within 45 s.

The first change let the cap follow the backlog without backpressure. A local `test_load` on that build still failed the high phase with 4 unfinalized deploys. Under load, the LFB lag of a healthy 3-validator shard is about 4 blocks. Soft backpressure therefore held every validator at 8 while the backlog was 12 to 20. The lag measures the depth of the finality cone, not a finality fault. The stall signal in this claim separates a deep cone from an LFB that does not advance.

## Definitions

- **Non-leader:** a validator for which `allow_deploy_inclusion` is false. `ordinary_admission_policy` gives it the fresh-admission fallback.
- **Leader:** the deploy-inclusion leader. Its ordinary cap comes from `adaptive_normal_ordinary_deploy_cap`.
- **Backlog:** `FreshLocalDeployStats.count`, the fresh deploys in local storage that the validator can admit.
- **Soft backpressure:** LFB lag from `FINALITY_LAG_SOFT_BACKPRESSURE_BLOCKS` (4) to 7.
- **Hard backpressure:** LFB lag of at least `FINALITY_LAG_HARD_BACKPRESSURE_BLOCKS` (8).
- **Stalled LFB:** `FinalityLagStats.stalled` is true. `FinalityProgress` sets it when the LFB height did not advance during the last `FINALITY_STALL_PROPOSALS` (2) proposals of this validator.
- **Base cap:** the value of `adaptive_fallback_ordinary_deploy_cap` for the same inputs.
- **Shard cap:** `max-user-deploys-per-block` from the shard configuration.

## Claim statements

**S1. Backlog cap.** Without backpressure, the fresh-admission cap of a non-leader is `max(base cap, min(backlog, 32, shard cap))`.

**S2. Bounds.** The backlog rule never gives more than 32 without backpressure, never more than `SOFT_BACKPRESSURE_MAX_ORDINARY_DEPLOY_CAP` (16) under soft backpressure, and never more than the shard cap.

**S3. Backpressure.**
- **S3a. Soft, LFB advancing.** The fresh-admission cap of a non-leader is `max(8, min(backlog, 16, shard cap))`.
- **S3b. Soft, LFB stalled.** The fresh-admission cap of a non-leader is 8, whatever the backlog.
- **S3c. Hard.** Every admission path gives 4: the non-leader fresh-admission cap, the in-scope recovery cap, and the leader's ordinary cap. The backlog, the stall signal, and the role do not change this.

**S4. Small backlogs are unchanged.** A backlog of 8 or fewer gives the base cap.

**S5. Age escalation without backpressure is unchanged.** Without backpressure, the base cap rises to 16 at 120 s and to 32 at 300 s. S1 takes the larger of the base cap and the backlog cap. Under backpressure the base cap is 8 or 4, whatever the age.

**S6. Other admission paths.**
- The in-scope recovery cap from `in_scope_recovery_fallback` does not follow the in-scope backlog.
- Without backpressure, the leader's ordinary cap is the shard cap.
- Under soft backpressure, the leader's ordinary cap is `min(shard cap, 16)` while the LFB advances, and 8 while it is stalled.
- Stale in-scope work still clamps the leader to 8, and signature-stale in-scope work still clamps it to 4.

**S7. Stall signal.**
- `FinalityProgress::observe` reports a stall after 2 consecutive proposals without an LFB advance, and the first advance clears it. A lower LFB height counts as no advance.
- `ProductionBlockCreator` keeps one `FinalityProgress` for the life of the node. It passes the tracker through `create_with_progress`. The node builds the proposer once at startup.
- `create` passes a fresh tracker, so its stall signal is always false. Only tests call `create`.

**S8. Cap source.** The admission policy reports why it chose each cap: `ordinary_cap_source` for the ordinary cap and `in_scope_recovery_cap_source` for the in-scope recovery cap. The values are normal, base, backlog, soft-backlog, soft-stalled, stale, and hard. Both go to gauges (`block-creator.deploy-admission.cap-source` and `block-creator.deploy-admission.in-scope-recovery-cap-source`) and to the admission log lines.

## Seam premises (documented, not proven)

- **Duplicate inclusion.** A larger cap for a non-leader can put more deploys into blocks that other validators also build. The existing in-scope filters and the merge's keep-one rule handle duplicates. The bounds of 32 and 16 limit the extra block size.
- **Merge contention.** Larger blocks can cause more merge rejections. The rejected-deploy recovery path re-proposes them. The soak measures this effect, and this claim does not prove it.
- **Feedback under slow storage.** Larger blocks take longer to create and validate. If finality then falls behind, the lag grows, and hard backpressure at lag 8 stops the growth. The soak shows whether the hard tier acts. The cap-source gauge records it.
- **Restart.** The tracker is in memory. After a restart it reports no stall until it sees 2 proposals. During that time, S3c still bounds the cap at lag 8 or more.
- **Proposal count, not time.** The stall window counts this validator's proposals, not wall-clock time. A validator that proposes rarely detects a stall later.

## Discharge plan

1. Each statement has a unit test in `block_creator.rs` (`mod tests`). The tests call the admission functions with fixed values. **Done 2026-10-10.**
   - S1: `non_leader_fresh_admission_cap_follows_its_backlog_without_backpressure`
   - S2: `non_leader_fresh_admission_cap_is_bounded_by_the_fallback_maximum_and_the_shard_cap` and `the_soft_backpressure_backlog_cap_is_never_above_the_shard_cap`
   - S3a: `soft_backpressure_with_an_advancing_lfb_lets_the_non_leader_cap_follow_its_backlog_up_to_16`
   - S3b: `a_stalled_lfb_under_backpressure_holds_the_non_leader_cap_whatever_the_backlog`
   - S3c: `hard_finality_backpressure_gives_cap_4_on_every_path_whatever_the_backlog_or_progress`
   - S4: `small_non_leader_backlog_keeps_the_base_fallback_cap`
   - S5: `age_escalation_still_raises_the_non_leader_cap_above_a_smaller_backlog` and `adaptive_fallback_scales_with_age_and_finality_backpressure`
   - S6: `in_scope_recovery_cap_does_not_follow_the_in_scope_backlog`, `deploy_inclusion_leader_keeps_the_normal_cap_whatever_the_fallback_backlog`, and `soft_backpressure_gives_the_leader_16_while_the_lfb_advances_and_8_while_it_is_stalled`
   - S7: `finality_progress_reports_a_stall_after_two_proposals_without_an_lfb_advance` and `a_proposers_lag_stats_report_a_stall_after_two_proposals_at_the_same_lfb`
   - S8: `the_admission_policy_reports_the_source_of_its_ordinary_cap` and `the_admission_policy_reports_the_in_scope_recovery_cap_source_separately`
2. Each test was checked with a mutation of the rule that it covers, and each mutation made the test fail. **Done 2026-10-10.**
3. Three older tests changed their soft-backpressure input to a stalled LFB, because S3a changed the meaning of "soft backpressure":
   - `adaptive_fallback_scales_with_age_and_finality_backpressure`
   - `a_stalled_lfb_under_backpressure_holds_the_non_leader_cap_whatever_the_backlog` (renamed from `finality_backpressure_holds_the_non_leader_cap_whatever_the_backlog`)
   - `deploy_inclusion_leader_keeps_the_normal_cap_whatever_the_fallback_backlog`

   All other admission tests pass without change. **Done 2026-10-10.**
4. Local evidence: `test_load` with the subprocess provider on a release build of `1f3131e1c` passed (1 passed in 545.9 s). **Done 2026-10-10.**

   | Phase | Inclusion p95 | Finalization p95 | Unfinalized | Same test before S3a (prune fix only) |
   |---|---|---|---|---|
   | high | 7.2 s | 22.5 s | 0 | 10.3 s, 33.4 s, 4 unfinalized |
   | sustained | 8.2 s | 24.1 s | 0 | 8.2 s, 24.2 s, 0 unfinalized |

5. Soak evidence: run a pre-flight and a soak on the hotfix branch. Compare them with soak 37875034099 (10 of 50 failed) and the master soak 37996403003. **Open.** Compare these values:
   - the high-phase inclusion p95
   - the `block-creator.ordinary-deploys.deferred` counts
   - the cap-source gauges, especially the time in the hard tier
   - the merge rejections
   - the `test_load` failure rate.
6. Record the evidence in `docs/casper/cbc-evidence/` for `block_creator.rs` and `proposer.rs` and cite this claim id. **Open, after the soak.** The maintainer decides acceptance.
