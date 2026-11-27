# CbC Evidence: node/src/rust/instances/heartbeat_proposer.rs

- **Status:** discharged
- **Adapter:** agentic
- **Commit:** 3fd707102
- **Verified:** 2026-08-21T05:21:58Z

Claim:

> # Heartbeat Proposal Amplification Bound Claim

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

```json
{
  "artifact": {
    "path": "node/src/rust/instances/heartbeat_proposer.rs",
    "commit": "3fd707102",
    "id": "node-src-rust-instances-heartbeat-proposer-rs"
  },
  "claim": "# Heartbeat Proposal Amplification Bound Claim\n\n## Status\n\nThis claim is mandatory for heartbeat recovery changes.\n\n## Claim\n\nA recent self-proposal must stop all routine heartbeat proposals until the configured cooldown ends.\n\nDeploy grace must not bypass the self-proposal cooldown.\n\nWhen finality lag exceeds the deploy recovery limit, only the deterministic recovery leader can propose a pending-deploy backstop.\n\nWhen finality lag exceeds the moderate recovery limit, only the deterministic recovery leader can propose stale-LFB recovery.\n\nThe selected recovery leader must retain a bounded high-lag recovery path after the cooldown ends.\n\nA temporally idle validator must retain the empty-frontier deadlock escape after the stale recovery interval ends.\n\n## Formal statement\n\n```text\nrecent_self(v) => not routine_proposal(v)\nhigh_deploy_lag and not leader(v) => not pending_deploy_backstop(v)\nhigh_stale_lag and not leader(v) => not stale_lfb_recovery(v)\nhigh_deploy_lag and leader(v) and cooldown_elapsed(v) => pending_deploy_backstop(v)\nidle(v) and stale_lfb and recovery_interval_elapsed(v) => recovery_escape(v)\n```\n\n## Evidence\n\n- `node/src/rust/instances/heartbeat_proposer.rs::deploy_grace_does_not_bypass_self_propose_cooldown`\n- `node/src/rust/instances/heartbeat_proposer.rs::high_lag_pending_deploy_backstop_is_leader_only`\n- `node/src/rust/instances/heartbeat_proposer.rs::high_lag_pending_deploy_backstop_allows_leader`\n- `node/src/rust/instances/heartbeat_proposer.rs::high_lag_stale_recovery_is_leader_only`\n- `node/src/rust/instances/heartbeat_proposer.rs::stale_recovery_breaks_the_empty_frontier_deadlock`\n\n## Gate rule\n\nA cooldown bypass or a non-leader high-lag proposal refutes this claim.\n\nLoss of both bounded recovery paths also refutes this claim.",
  "adapter": "agentic",
  "status": "discharged",
  "evidence": {
    "kind": "proof",
    "ref": "verify-heartbeat-proposal-bound.sh",
    "counterexample": null,
    "detail": "agentic: LLM-proposed annotations, prover-discharged",
    "proposal": ""
  },
  "waiver": null,
  "verified_at": "2026-08-21T05:21:58Z"
}
```
