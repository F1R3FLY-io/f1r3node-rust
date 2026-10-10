# Claim: a non-leader's fresh-admission cap follows its local backlog

```yaml
claim_id: CLAIM-CASPER-FRESH-ADMISSION-CAP-001
artifacts:
  - casper/src/rust/blocks/proposer/block_creator.rs   # fresh_admission_fallback and its tests
status: pending
adapter: agentic
mechanization: none
references:
  - https://github.com/F1R3FLY-io/f1r3node-rust/issues/24   # test_load deploys not finalized within 45 s
  - https://github.com/F1R3FLY-io/f1r3node-rust/issues/24#issuecomment-6094239136   # the non-leader fallback cap of 8 is the mechanism
  - GitHub Actions run 38017311024                          # weekend soak preflight: high-phase failure with cap 8
```

## Context

Soak 38017311024 failed `test_load` in the high phase. While unfinalized user deploys are in scope, only the deploy-inclusion leader admits ordinary deploys at the normal cap. Every other validator takes the fallback path, and `adaptive_fallback_ordinary_deploy_cap` gives it 8 deploys for each block. The cap rises to 16 at an oldest fresh deploy age of 120 s and to 32 at 300 s. Both ages are above the 45 s finalization window of `test_load`.

In the high phase, validator2 held 26, 33, 25, 17, and 9 fresh local deploys and selected 8 each time. Its oldest fresh deploy aged from 7.5 s to 18.0 s, and one deploy did not finalize within 45 s.

## Definitions

- **Non-leader:** a validator for which `allow_deploy_inclusion` is false, so `ordinary_admission_policy` uses the fresh-admission fallback.
- **Backlog:** `FreshLocalDeployStats.count`, the fresh deploys in local storage that the validator can admit.
- **Backpressure:** LFB lag of at least `FINALITY_LAG_SOFT_BACKPRESSURE_BLOCKS` (4).
- **Base cap:** the value of `adaptive_fallback_ordinary_deploy_cap` for the same inputs.
- **Shard cap:** `max-user-deploys-per-block` from the shard configuration.

## Claim statements

**S1. Backlog cap.** Without backpressure, the fresh-admission cap of a non-leader is `max(base cap, min(backlog, 32, shard cap))`.

**S2. Bounds.** The fresh-admission cap is never above 32 from the backlog rule, and never above the shard cap.

**S3. Backpressure is unchanged.** With backpressure, the fresh-admission cap is the base cap: 8 at lag 4 to 7, and 4 at lag 8 or more, whatever the backlog.

**S4. Small backlogs are unchanged.** A backlog of 8 or fewer gives the base cap.

**S5. Age escalation is unchanged.** The base cap still rises to 16 at 120 s and to 32 at 300 s. S1 takes the larger of the base cap and the backlog cap.

**S6. Other admission paths are unchanged.**
- The in-scope recovery cap from `in_scope_recovery_fallback` does not follow the in-scope backlog.
- The leader's normal cap from `adaptive_normal_ordinary_deploy_cap` does not change.

## Seam premises (documented, not proven)

- **Duplicate inclusion.** A larger cap for a non-leader can put more deploys into blocks that other validators also build. The existing in-scope filters and the merge's keep-one rule handle duplicates. The bound of 32 limits the extra block size.
- **Merge contention.** Larger non-leader blocks can cause more merge rejections. The rejected-deploy recovery path re-proposes them. The soak measures this effect, and this claim does not prove it.

## Discharge plan

1. S1 to S6 each have a unit test in `block_creator.rs` (`mod tests`). The tests call `fresh_admission_fallback`, `in_scope_recovery_fallback`, and `ordinary_admission_policy` with fixed values. **Done 2026-10-10** (commits 05350ba52, 53fe719be, 7e592d1fb, 6891fee26, f8ffc2b84):
   - S1: `non_leader_fresh_admission_cap_follows_its_backlog_without_backpressure`
   - S2: `non_leader_fresh_admission_cap_is_bounded_by_the_fallback_maximum_and_the_shard_cap`
   - S3: `finality_backpressure_holds_the_non_leader_cap_whatever_the_backlog`
   - S4: `small_non_leader_backlog_keeps_the_base_fallback_cap`
   - S5: `age_escalation_still_raises_the_non_leader_cap_above_a_smaller_backlog`
   - S6: `in_scope_recovery_cap_does_not_follow_the_in_scope_backlog` and `deploy_inclusion_leader_keeps_the_normal_cap_whatever_the_fallback_backlog`
2. Each test was checked with a mutation of the rule that it covers, and each mutation made the test fail. **Done 2026-10-10.**
3. The existing admission tests in `block_creator.rs` pass without change: 59 tests in the module. **Done 2026-10-10.**
4. Soak evidence: run a soak on the hotfix branch. Compare it with soak 37875034099 (10 of 50 failed) and the master soak 37996403003. **Open.** Compare these values:
   - the high-phase inclusion p95
   - the `block-creator.ordinary-deploys.deferred` counts
   - the merge rejections
   - the `test_load` failure rate.
5. Record the evidence in `docs/casper/cbc-evidence/` for `block_creator.rs` and cite this claim id. **Open, after the soak.** The maintainer decides acceptance.
