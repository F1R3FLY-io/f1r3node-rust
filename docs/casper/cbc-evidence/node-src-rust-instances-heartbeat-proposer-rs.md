# CbC Evidence: node/src/rust/instances/heartbeat_proposer.rs

- **Status:** pending
- **Adapter:** manual
- **Commit:** ce70d8ee1fa24742888dc86be45aea8a123ee056
- **Verified:** 2026-11-26T21:42:26Z

Claim:

> # Heartbeat Proposal Amplification Bound Claim
>
> ## Status
>
> This claim is mandatory for heartbeat recovery changes.
>
> Revised: the original formulation bounded amplification by silencing
> non-leaders at high lag. Artifact replay of the CI finality stalls refuted
> that mechanism: leader-only recovery is what held the stalls open.
> Non-leaders logged "waiting for selected recovery leader" 81 and 114 times
> in run 32284324989, 288 and 289 times in run 32397055615 (the 851-second
> stall), while a single proposer — who cannot rebuild the mutual
> justification a certification clique requires — proposed alone. The bound
> is real; it is enforced by cadence and width, never by proposer exclusion.
>
> ## Claim
>
> A recent self-proposal must stop all routine heartbeat proposals until the
> configured cooldown ends.
>
> A newly admitted deploy can wake the heartbeat when the API has no propose
> function. A failed or duplicate admission does not send a deploy wake.
> When only the cooldown blocks the pending-deploy lane, the heartbeat can
> schedule an earlier check. The check evaluates the same proposal conditions.
>
> Deploy grace must not bypass the self-proposal cooldown; grace widens lag
> caps only.
>
> Stale-LFB recovery is open to every bonded validator and paced solely by
> the stale-recovery interval on both the LFB's age and the validator's own
> silence: at most one recovery proposal per validator per interval.
>
> The pending-deploy backstop is open to every bonded validator on the same
> interval pacing: user work is never gated behind a single proposer.
>
> Empty heartbeat proposals stop at the empty-frontier width cap, except
> that a temporally idle validator retains one proposal per stale-recovery
> interval through the cap (the consensus-deadlock escape).
>
> ## Formal statement
>
> ```text
> recent_self(v) => not routine_proposal(v)
> stale_lfb and recovery_interval_elapsed(v) and not backpressure => stale_lfb_recovery(v)
> high_deploy_lag and pending(v) and recovery_interval_elapsed(v) and cooldown_elapsed(v) => pending_deploy_backstop(v)
> proposals_per_interval(v) <= 1 during a stall
> idle(v) and stale_lfb and recovery_interval_elapsed(v) => recovery_escape(v)
> ```
>
> ## Evidence
>
> - `casper/tests/engine/casper_launch_spec.rs::only_a_new_admitted_deploy_wakes_an_enabled_heartbeat`
> - `node/src/rust/instances/heartbeat_proposer.rs::cooldown_recheck_tests`
> - `node/src/rust/instances/heartbeat_proposer.rs::deploy_grace_does_not_bypass_self_propose_cooldown`
> - `node/src/rust/instances/heartbeat_proposer.rs::a_cooldown_hot_validator_defers_routine_lanes_but_never_recovery`
> - `node/src/rust/instances/heartbeat_proposer.rs::high_lag_pending_deploy_backstop_fires_for_non_leaders_too`
> - `node/src/rust/instances/heartbeat_proposer.rs::high_lag_pending_deploy_backstop_allows_leader`
> - `node/src/rust/instances/heartbeat_proposer.rs::high_lag_stale_recovery_fires_for_every_validator`
> - `node/src/rust/instances/heartbeat_proposer.rs::a_stalled_idle_non_leader_must_get_its_recovery_proposal`
> - `node/src/rust/instances/heartbeat_proposer.rs::stale_recovery_breaks_the_empty_frontier_deadlock`
>
> ## Counter-evidence to the leader-only formulation
>
> - CI stall instances i1 (run 32284324989, 239 s) and i5 (run 32397055615,
>   851 s): the stand-down counts above, measured from the shards' own logs.
> - `casper/tests/finalized_floor/oracle_stall_replay_spec.rs`: exact oracle
>   replays of both instances from committed fixtures — certification failed
>   for want of mutual witnessing while one leader proposed.
>
> ## Gate rule
>
> A routine-lane cooldown bypass refutes this claim.
>
> A validator silenced during a stall — any bonded validator whose
> stale-recovery interval has elapsed under a stale LFB and that still may
> not propose — refutes this claim.
>
> More than one empty proposal per validator per stale-recovery interval
> during a stall refutes this claim.

```json
{
  "artifact": {
    "path": "node/src/rust/instances/heartbeat_proposer.rs",
    "commit": "ce70d8ee1fa24742888dc86be45aea8a123ee056",
    "id": "node-src-rust-instances-heartbeat-proposer-rs"
  },
  "claim": "# Heartbeat Proposal Amplification Bound Claim\n\n## Status\n\nThis claim is mandatory for heartbeat recovery changes.\n\nRevised: the original formulation bounded amplification by silencing\nnon-leaders at high lag. Artifact replay of the CI finality stalls refuted\nthat mechanism: leader-only recovery is what held the stalls open.\nNon-leaders logged \"waiting for selected recovery leader\" 81 and 114 times\nin run 32284324989, 288 and 289 times in run 32397055615 (the 851-second\nstall), while a single proposer \u2014 who cannot rebuild the mutual\njustification a certification clique requires \u2014 proposed alone. The bound\nis real; it is enforced by cadence and width, never by proposer exclusion.\n\n## Claim\n\nA recent self-proposal must stop all routine heartbeat proposals until the\nconfigured cooldown ends.\n\nA newly admitted deploy can wake the heartbeat when the API has no propose\nfunction. A failed or duplicate admission does not send a deploy wake.\nWhen only the cooldown blocks the pending-deploy lane, the heartbeat can\nschedule an earlier check. The check evaluates the same proposal conditions.\n\nDeploy grace must not bypass the self-proposal cooldown; grace widens lag\ncaps only.\n\nStale-LFB recovery is open to every bonded validator and paced solely by\nthe stale-recovery interval on both the LFB's age and the validator's own\nsilence: at most one recovery proposal per validator per interval.\n\nThe pending-deploy backstop is open to every bonded validator on the same\ninterval pacing: user work is never gated behind a single proposer.\n\nEmpty heartbeat proposals stop at the empty-frontier width cap, except\nthat a temporally idle validator retains one proposal per stale-recovery\ninterval through the cap (the consensus-deadlock escape).\n\n## Formal statement\n\n```text\nrecent_self(v) => not routine_proposal(v)\nstale_lfb and recovery_interval_elapsed(v) and not backpressure => stale_lfb_recovery(v)\nhigh_deploy_lag and pending(v) and recovery_interval_elapsed(v) and cooldown_elapsed(v) => pending_deploy_backstop(v)\nproposals_per_interval(v) <= 1 during a stall\nidle(v) and stale_lfb and recovery_interval_elapsed(v) => recovery_escape(v)\n```\n\n## Evidence\n\n- `casper/tests/engine/casper_launch_spec.rs::only_a_new_admitted_deploy_wakes_an_enabled_heartbeat`\n- `node/src/rust/instances/heartbeat_proposer.rs::cooldown_recheck_tests`\n- `node/src/rust/instances/heartbeat_proposer.rs::deploy_grace_does_not_bypass_self_propose_cooldown`\n- `node/src/rust/instances/heartbeat_proposer.rs::a_cooldown_hot_validator_defers_routine_lanes_but_never_recovery`\n- `node/src/rust/instances/heartbeat_proposer.rs::high_lag_pending_deploy_backstop_fires_for_non_leaders_too`\n- `node/src/rust/instances/heartbeat_proposer.rs::high_lag_pending_deploy_backstop_allows_leader`\n- `node/src/rust/instances/heartbeat_proposer.rs::high_lag_stale_recovery_fires_for_every_validator`\n- `node/src/rust/instances/heartbeat_proposer.rs::a_stalled_idle_non_leader_must_get_its_recovery_proposal`\n- `node/src/rust/instances/heartbeat_proposer.rs::stale_recovery_breaks_the_empty_frontier_deadlock`\n\n## Counter-evidence to the leader-only formulation\n\n- CI stall instances i1 (run 32284324989, 239 s) and i5 (run 32397055615,\n  851 s): the stand-down counts above, measured from the shards' own logs.\n- `casper/tests/finalized_floor/oracle_stall_replay_spec.rs`: exact oracle\n  replays of both instances from committed fixtures \u2014 certification failed\n  for want of mutual witnessing while one leader proposed.\n\n## Gate rule\n\nA routine-lane cooldown bypass refutes this claim.\n\nA validator silenced during a stall \u2014 any bonded validator whose\nstale-recovery interval has elapsed under a stale LFB and that still may\nnot propose \u2014 refutes this claim.\n\nMore than one empty proposal per validator per stale-recovery interval\nduring a stall refutes this claim.\n",
  "adapter": "manual",
  "status": "pending",
  "evidence": {
    "kind": "rust-test-results",
    "ref": "scripts/verify-heartbeat-proposal-bound.sh",
    "detail": "The working-tree candidate passed 48 heartbeat tests on the dev base. This covers cooldown decisions, recheck scheduling and the existing recovery lanes. The admission integration tests passed separately. The source digest identifies the candidate. Formal discharge is pending."
  },
  "waiver": null,
  "verified_at": "2026-11-26T21:42:26Z",
  "verification_id": "815a4022-1725-4e28-b8ee-6c4d13577b4d",
  "inputs": {
    "artifact": "node/src/rust/instances/heartbeat_proposer.rs",
    "source_sha256": "edd5aa018a48170b79bd665a6fb1c2fe37049385c2fe3de137538f871925a3a0",
    "claim": {
      "path": "docs/claims/heartbeat-proposal-amplification-bound.md",
      "sha256": "e5c10f2b0bf6e8ee906c0665dfa5af41429ac77a39b7e7a54c12fe3bc9fa03a6"
    }
  },
  "evidence_files": {
    "scripts/verify-heartbeat-proposal-bound.sh": "ab2d10dce19da66d8d0c4965ebc7ff84ae5887f3a39f3706a3ab5b4650ea6065"
  }
}
```
