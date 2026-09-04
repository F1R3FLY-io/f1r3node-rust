# Consensus Cross-View Determinism Claim

## Status

This claim is mandatory and pending complete code-level discharge.

## Claim

Two honest nodes must select the same recovery leader when their finalized bonded validator sets are equal.

Local last finalized block hashes must not change the selected leader.

Local non-finalized DAG contents must not change the selected leader.

Validator enumeration order must not change the selected leader.

## Formal statement

For bonded set `V` and local views `a` and `b`:

```text
V(a) = V(b) => leader(V(a), a) = leader(V(b), b)
```

## Evidence

- `formal/tlaplus/recovery_leader/RecoveryLeader.tla`
- `formal/tlaplus/recovery_leader/MC_RecoveryLeader.cfg`
- `formal/tlaplus/recovery_leader/MC_RecoveryLeader_view_dependent_pre_fix.cfg`
- `node/src/rust/instances/heartbeat_proposer.rs::lag_recovery_leader_is_stable_across_local_dag_and_lfb_views`

## Required code bridge

A generated property test must vary validator order and both local LFB values.

The generated test must hold the bonded validator set constant.

## Gate rule

Any view-dependent leader counterexample refutes this claim and blocks completion.
