# Heartbeat Proposal Amplification Bound Claim

## Status

This claim is mandatory for heartbeat recovery changes.

## Claim

A recent self-proposal must stop all routine heartbeat proposals until the configured cooldown ends.

Deploy grace must not bypass the self-proposal cooldown.

When finality lag exceeds the deploy recovery limit, only the deterministic recovery leader can propose a pending-deploy backstop.

When finality lag exceeds the moderate recovery limit, only the deterministic recovery leader can propose stale-LFB recovery.

The selected recovery leader must retain a bounded high-lag recovery path after the cooldown ends.

A temporally idle validator must retain the empty-frontier deadlock escape after the stale recovery interval ends.

## Formal statement

```text
recent_self(v) => not routine_proposal(v)
high_deploy_lag and not leader(v) => not pending_deploy_backstop(v)
high_stale_lag and not leader(v) => not stale_lfb_recovery(v)
high_deploy_lag and leader(v) and cooldown_elapsed(v) => pending_deploy_backstop(v)
idle(v) and stale_lfb and recovery_interval_elapsed(v) => recovery_escape(v)
```

## Evidence

- `node/src/rust/instances/heartbeat_proposer.rs::deploy_grace_does_not_bypass_self_propose_cooldown`
- `node/src/rust/instances/heartbeat_proposer.rs::high_lag_pending_deploy_backstop_is_leader_only`
- `node/src/rust/instances/heartbeat_proposer.rs::high_lag_pending_deploy_backstop_allows_leader`
- `node/src/rust/instances/heartbeat_proposer.rs::high_lag_stale_recovery_is_leader_only`
- `node/src/rust/instances/heartbeat_proposer.rs::stale_recovery_breaks_the_empty_frontier_deadlock`

## Gate rule

A cooldown bypass or a non-leader high-lag proposal refutes this claim.

Loss of both bounded recovery paths also refutes this claim.
