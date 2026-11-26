# Heartbeat Proposal Amplification Bound Claim

## Status

This claim is mandatory for heartbeat recovery changes.

The original claim limited recovery to one leader at high lag. Replay of
two finality stalls showed that recovery needed proposals from other bonded
validators. In run 32284324989, two validators logged 81 and 114 waits for
the recovery leader. In run 32397055615, two validators logged 288 and 289
waits during an 851-second stall.

The recovery interval and the empty-frontier width cap bound proposals.

## Claim

A recent self-proposal must stop all routine heartbeat proposals until the
configured cooldown ends.

Deploy admission sends one wake after a new reservation if the wake flag
is on and the heartbeat signal exists. The wake flag is on when autopropose
is off or the node has no proposal trigger. A failed or duplicate admission
sends no wake. The receiver can combine wake signals.

The heartbeat can schedule an earlier check when only the cooldown blocks
the pending-deploy lane. This requires a known age for the validator's last
block and no proposal due from another lane. The timer delay stays within
the normal check interval. The new check evaluates the proposal conditions
again.

Deploy grace increases lag caps. The self-proposal cooldown still applies.

Every bonded validator can propose through stale-LFB recovery. The recovery
interval applies to the age of the LFB and the time since the validator's
last proposal. Each validator can make at most one recovery proposal in the
interval.

Every bonded validator can use the pending-deploy backstop under the same
interval rule.

Empty heartbeat proposals stop at the empty-frontier width cap, except
that a temporally idle validator retains one proposal per stale-recovery
interval through the cap (the consensus-deadlock escape).

## Formal statement

```text
recent_self(v) => not routine_proposal(v)
stale_lfb and recovery_interval_elapsed(v) and not backpressure => stale_lfb_recovery(v)
high_deploy_lag and pending(v) and recovery_interval_elapsed(v) and cooldown_elapsed(v) => pending_deploy_backstop(v)
proposals_per_interval(v) <= 1 during a stall
idle(v) and stale_lfb and recovery_interval_elapsed(v) => recovery_escape(v)
```

## Evidence

- F1 and F2 model checks and Rust bindings in `formal/tlaplus/heartbeat_wake/README.md`.
- `casper/src/rust/engine/casper_launch.rs::a_node_wakes_the_heartbeat_on_deploys_only_without_autopropose`
- `casper/tests/engine/casper_launch_spec.rs::only_a_new_admitted_deploy_wakes_an_enabled_heartbeat`
- `node/src/rust/instances/heartbeat_proposer.rs::cooldown_recheck_tests`
- `node/src/rust/instances/heartbeat_proposer.rs::deploy_grace_does_not_bypass_self_propose_cooldown`
- `node/src/rust/instances/heartbeat_proposer.rs::a_cooldown_hot_validator_defers_routine_lanes_but_never_recovery`
- `node/src/rust/instances/heartbeat_proposer.rs::high_lag_pending_deploy_backstop_fires_for_non_leaders_too`
- `node/src/rust/instances/heartbeat_proposer.rs::high_lag_pending_deploy_backstop_allows_leader`
- `node/src/rust/instances/heartbeat_proposer.rs::high_lag_stale_recovery_fires_for_every_validator`
- `node/src/rust/instances/heartbeat_proposer.rs::a_stalled_idle_non_leader_must_get_its_recovery_proposal`
- `node/src/rust/instances/heartbeat_proposer.rs::stale_recovery_breaks_the_empty_frontier_deadlock`

## Counter-evidence to the leader-only formulation

- The shard logs give the wait counts for stalls i1 (run 32284324989, 239 s)
  and i5 (run 32397055615, 851 s).
- `casper/tests/finalized_floor/oracle_stall_replay_spec.rs` replays both
  stalls from committed test inputs. The validators could not form the
  mutual justifications needed for certification while one leader proposed.

## Gate rule

A routine-lane cooldown bypass refutes this claim.

During a stall, a bonded validator can propose after its stale-recovery
interval elapses while the LFB is stale. Preventing that proposal refutes
this claim.

More than one empty proposal per validator per stale-recovery interval
during a stall refutes this claim.
