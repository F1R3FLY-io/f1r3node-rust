# Heartbeat Proposal Amplification Bound Claim

## Status

This claim is mandatory for heartbeat recovery changes.

Revised: the original formulation bounded amplification by silencing
non-leaders at high lag. Artifact replay of the CI finality stalls refuted
that mechanism: leader-only recovery is what held the stalls open.
Non-leaders logged "waiting for selected recovery leader" 81 and 114 times
in run 32284324989, 288 and 289 times in run 32397055615 (the 851-second
stall), while a single proposer — who cannot rebuild the mutual
justification a certification clique requires — proposed alone. The bound
is real; it is enforced by cadence and width, never by proposer exclusion.

## Claim

A recent self-proposal must stop all routine heartbeat proposals until the
configured cooldown ends.

Deploy grace must not bypass the self-proposal cooldown; grace widens lag
caps only.

Stale-LFB recovery is open to every bonded validator and paced solely by
the stale-recovery interval on both the LFB's age and the validator's own
silence: at most one recovery proposal per validator per interval.

The pending-deploy backstop is open to every bonded validator on the same
interval pacing: user work is never gated behind a single proposer.

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

- `node/src/rust/instances/heartbeat_proposer.rs::deploy_grace_does_not_bypass_self_propose_cooldown`
- `node/src/rust/instances/heartbeat_proposer.rs::a_cooldown_hot_validator_defers_routine_lanes_but_never_recovery`
- `node/src/rust/instances/heartbeat_proposer.rs::high_lag_pending_deploy_backstop_fires_for_non_leaders_too`
- `node/src/rust/instances/heartbeat_proposer.rs::high_lag_pending_deploy_backstop_allows_leader`
- `node/src/rust/instances/heartbeat_proposer.rs::high_lag_stale_recovery_fires_for_every_validator`
- `node/src/rust/instances/heartbeat_proposer.rs::a_stalled_idle_non_leader_must_get_its_recovery_proposal`
- `node/src/rust/instances/heartbeat_proposer.rs::stale_recovery_breaks_the_empty_frontier_deadlock`

## Counter-evidence to the leader-only formulation

- CI stall instances i1 (run 32284324989, 239 s) and i5 (run 32397055615,
  851 s): the stand-down counts above, measured from the shards' own logs.
- `casper/tests/finalized_floor/oracle_stall_replay_spec.rs`: exact oracle
  replays of both instances from committed fixtures — certification failed
  for want of mutual witnessing while one leader proposed.

## Gate rule

A routine-lane cooldown bypass refutes this claim.

A validator silenced during a stall — any bonded validator whose
stale-recovery interval has elapsed under a stale LFB and that still may
not propose — refutes this claim.

More than one empty proposal per validator per stale-recovery interval
during a stall refutes this claim.
