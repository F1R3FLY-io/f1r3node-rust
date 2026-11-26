# CbC Evidence: node/src/rust/instances/heartbeat_proposer.rs

- **Status:** pending
- **Adapter:** manual
- **Commit:** 2267799f938e0e5ef8664d1eaa18c26becaadc72
- **Verified:** 2026-11-26T22:34:47Z

Claim:

> # Heartbeat Proposal Amplification Bound Claim
>
> ## Status
>
> This claim is mandatory for heartbeat recovery changes.
>
> The original claim limited recovery to one leader at high lag. Replay of
> two finality stalls showed that recovery needed proposals from other bonded
> validators. In run 32284324989, two validators logged 81 and 114 waits for
> the recovery leader. In run 32397055615, two validators logged 288 and 289
> waits during an 851-second stall.
>
> The recovery interval and the empty-frontier width cap bound proposals.
>
> ## Claim
>
> A recent self-proposal must stop all routine heartbeat proposals until the
> configured cooldown ends.
>
> Deploy admission sends one wake after a new reservation if the wake flag
> is on and the heartbeat signal exists. The wake flag is on when autopropose
> is off or the node has no proposal trigger. A failed or duplicate admission
> sends no wake. The receiver can combine wake signals.
>
> The heartbeat can schedule an earlier check when only the cooldown blocks
> the pending-deploy lane. This requires a known age for the validator's last
> block and no proposal due from another lane. The timer delay stays within
> the normal check interval. The new check evaluates the proposal conditions
> again.
>
> Deploy grace increases lag caps. The self-proposal cooldown still applies.
>
> Every bonded validator can propose through stale-LFB recovery. The recovery
> interval applies to the age of the LFB and the time since the validator's
> last proposal. Each validator can make at most one recovery proposal in the
> interval.
>
> Every bonded validator can use the pending-deploy backstop under the same
> interval rule.
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
> - F1 and F2 model checks and Rust bindings in `formal/tlaplus/heartbeat_wake/README.md`.
> - `casper/src/rust/engine/casper_launch.rs::a_node_wakes_the_heartbeat_on_deploys_only_without_autopropose`
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
> - The shard logs give the wait counts for stalls i1 (run 32284324989, 239 s)
>   and i5 (run 32397055615, 851 s).
> - `casper/tests/finalized_floor/oracle_stall_replay_spec.rs` replays both
>   stalls from committed test inputs. The validators could not form the
>   mutual justifications needed for certification while one leader proposed.
>
> ## Gate rule
>
> A routine-lane cooldown bypass refutes this claim.
>
> During a stall, a bonded validator can propose after its stale-recovery
> interval elapses while the LFB is stale. Preventing that proposal refutes
> this claim.
>
> More than one empty proposal per validator per stale-recovery interval
> during a stall refutes this claim.

```json
{
  "artifact": {
    "path": "node/src/rust/instances/heartbeat_proposer.rs",
    "commit": "2267799f938e0e5ef8664d1eaa18c26becaadc72",
    "id": "node-src-rust-instances-heartbeat-proposer-rs"
  },
  "claim": "# Heartbeat Proposal Amplification Bound Claim\n\n## Status\n\nThis claim is mandatory for heartbeat recovery changes.\n\nThe original claim limited recovery to one leader at high lag. Replay of\ntwo finality stalls showed that recovery needed proposals from other bonded\nvalidators. In run 32284324989, two validators logged 81 and 114 waits for\nthe recovery leader. In run 32397055615, two validators logged 288 and 289\nwaits during an 851-second stall.\n\nThe recovery interval and the empty-frontier width cap bound proposals.\n\n## Claim\n\nA recent self-proposal must stop all routine heartbeat proposals until the\nconfigured cooldown ends.\n\nDeploy admission sends one wake after a new reservation if the wake flag\nis on and the heartbeat signal exists. The wake flag is on when autopropose\nis off or the node has no proposal trigger. A failed or duplicate admission\nsends no wake. The receiver can combine wake signals.\n\nThe heartbeat can schedule an earlier check when only the cooldown blocks\nthe pending-deploy lane. This requires a known age for the validator's last\nblock and no proposal due from another lane. The timer delay stays within\nthe normal check interval. The new check evaluates the proposal conditions\nagain.\n\nDeploy grace increases lag caps. The self-proposal cooldown still applies.\n\nEvery bonded validator can propose through stale-LFB recovery. The recovery\ninterval applies to the age of the LFB and the time since the validator's\nlast proposal. Each validator can make at most one recovery proposal in the\ninterval.\n\nEvery bonded validator can use the pending-deploy backstop under the same\ninterval rule.\n\nEmpty heartbeat proposals stop at the empty-frontier width cap, except\nthat a temporally idle validator retains one proposal per stale-recovery\ninterval through the cap (the consensus-deadlock escape).\n\n## Formal statement\n\n```text\nrecent_self(v) => not routine_proposal(v)\nstale_lfb and recovery_interval_elapsed(v) and not backpressure => stale_lfb_recovery(v)\nhigh_deploy_lag and pending(v) and recovery_interval_elapsed(v) and cooldown_elapsed(v) => pending_deploy_backstop(v)\nproposals_per_interval(v) <= 1 during a stall\nidle(v) and stale_lfb and recovery_interval_elapsed(v) => recovery_escape(v)\n```\n\n## Evidence\n\n- F1 and F2 model checks and Rust bindings in `formal/tlaplus/heartbeat_wake/README.md`.\n- `casper/src/rust/engine/casper_launch.rs::a_node_wakes_the_heartbeat_on_deploys_only_without_autopropose`\n- `casper/tests/engine/casper_launch_spec.rs::only_a_new_admitted_deploy_wakes_an_enabled_heartbeat`\n- `node/src/rust/instances/heartbeat_proposer.rs::cooldown_recheck_tests`\n- `node/src/rust/instances/heartbeat_proposer.rs::deploy_grace_does_not_bypass_self_propose_cooldown`\n- `node/src/rust/instances/heartbeat_proposer.rs::a_cooldown_hot_validator_defers_routine_lanes_but_never_recovery`\n- `node/src/rust/instances/heartbeat_proposer.rs::high_lag_pending_deploy_backstop_fires_for_non_leaders_too`\n- `node/src/rust/instances/heartbeat_proposer.rs::high_lag_pending_deploy_backstop_allows_leader`\n- `node/src/rust/instances/heartbeat_proposer.rs::high_lag_stale_recovery_fires_for_every_validator`\n- `node/src/rust/instances/heartbeat_proposer.rs::a_stalled_idle_non_leader_must_get_its_recovery_proposal`\n- `node/src/rust/instances/heartbeat_proposer.rs::stale_recovery_breaks_the_empty_frontier_deadlock`\n\n## Counter-evidence to the leader-only formulation\n\n- The shard logs give the wait counts for stalls i1 (run 32284324989, 239 s)\n  and i5 (run 32397055615, 851 s).\n- `casper/tests/finalized_floor/oracle_stall_replay_spec.rs` replays both\n  stalls from committed test inputs. The validators could not form the\n  mutual justifications needed for certification while one leader proposed.\n\n## Gate rule\n\nA routine-lane cooldown bypass refutes this claim.\n\nDuring a stall, a bonded validator can propose after its stale-recovery\ninterval elapses while the LFB is stale. Preventing that proposal refutes\nthis claim.\n\nMore than one empty proposal per validator per stale-recovery interval\nduring a stall refutes this claim.\n",
  "adapter": "manual",
  "status": "pending",
  "evidence": {
    "kind": "tla-rocq-rust",
    "ref": "formal/tlaplus/heartbeat_wake/README.md",
    "detail": "TLC checked 82,945 distinct states and found the three expected counterexample traces. The Rocq gate checked seven theorems for the local rules. The Rust binding check passed 221,184 lane decision cases. The B1 wake-enablement test passed. The full amplification claim remains pending."
  },
  "waiver": null,
  "verified_at": "2026-11-26T22:34:47Z",
  "verification_id": "e3ae68ad-7aa3-4070-9e43-930b416c275f",
  "inputs": {
    "artifact": "node/src/rust/instances/heartbeat_proposer.rs",
    "source_sha256": "edd5aa018a48170b79bd665a6fb1c2fe37049385c2fe3de137538f871925a3a0",
    "claim": {
      "path": "docs/claims/heartbeat-proposal-amplification-bound.md",
      "sha256": "ed82fc7ecb52b015a8eecb425e467329b6005fd72a5017f713941b5f7d7e740e"
    }
  },
  "evidence_files": {
    "formal/tlaplus/heartbeat_wake/README.md": "ec9abb8e5f5e8a4cc670fe97fff42409eb58eb051cf7ad8110961ae8e61ec2ba"
  }
}
```
