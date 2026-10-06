# Host-work budget verification

This package verifies deterministic, non-economic limits for consensus host work.
The limits control node resource use without changing Rholang resource charges.

The permanent [host-work specification](../../../docs/casper/theory/host-work-budget.md) defines the production units and lifecycle boundary.

The package contains a finite concurrent TLA+ model and unbounded Rocq proofs.
The two layers check different parts of the same contract.

## Terms

A **host-work unit** is a consensus-visible integer measure of node work.
It is not a token, phlogiston charge, wallet debit, or semantic reduction.

A **phase** identifies one host-work category.
The model contains the eight phases in the production schedule.

A **dimension** is one independent capacity counter inside a phase.
No dimension can subsidize another dimension through a weighted sum.

| Phase | Independent dimensions |
| --- | --- |
| Structural admission | Structural items and structural bytes |
| Pure reduction | Reduction steps and reduction term bytes |
| Primitive evaluation | Primitive calls and primitive input bytes |
| Substitution | Substitution bindings and substitution bytes |
| Authority discovery | Authority nodes and authority depth |
| Physical search | Search candidates and search state bytes |
| Witness decoding | Witness fields and witness bytes |
| Witness verification | Verification operations and verification bytes |

A **schedule** maps each independent dimension to its integer limit.
A checkpoint commits the schedule with the complete event set and dimension usage.

A **reservation** checks available capacity before a phase changes state.
The checked rule accepts work only when both conditions hold:

```math
u + w \le L \quad\land\quad u + w \le M
```

Here, $`u`$ is used work, $`w`$ is requested work, $`L`$ is the dimension limit, and $`M`$ is the counter maximum.

A **replay** applies the checkpoint schedule to the complete checkpoint event set.
Replay uses canonical event order and cumulative, per-dimension reservations.

## Concurrent model

[`HostWorkBudget.tla`](HostWorkBudget.tla) models two validators and two shards.
Each validator receives two concurrent branches in an independent order.

Each validator processes a canonical twenty-item workload.
The workload exercises all sixteen dimensions, both shards, successful reservations, and cumulative rejection.

One failed reservation rejects and rolls back the complete transactional deployment.
Each validator then creates a checkpoint and replays the checkpoint.
Validator actions can interleave in every order that TLC explores.
Independent dimension counters do not require one global reservation sequence.

A branch can complete work after another branch sees the first failure.
The sticky rejection flag still rejects the complete deployment.

The safe model checks these invariants:

| Invariant | Verified requirement |
| --- | --- |
| `Inv_ArrivalOrderResult` | Independent branch arrival order cannot change the selected result. |
| `Inv_CheckedReservation` | Accepted work equals reserved work and stays below each dimension limit. |
| `Inv_FailureNonMutation` | A failed reservation cannot change semantic state. |
| `Inv_ShardLocalAdmission` | Work on one shard cannot consume another shard budget. |
| `Inv_EconomicSeparation` | Host work cannot debit an economic balance. |
| `Inv_DecodeAllocationBound` | Decode allocation cannot occur before a successful reservation. |
| `Inv_TransactionalFailureRollback` | One failed dimension rolls back all transactional host-work state. |
| `Inv_EndVerdictScheduleIndependent` | Validator interleavings produce the same final verdict. |
| `Inv_CompleteEventVerdictAgreement` | The final verdict uses the complete intended event multiset. |
| `Inv_CheckpointCoherent` | A checkpoint records the exact schedule, complete event set, and attempted usage. |
| Replay invariants | Replay enforces cumulative limits and reproduces the final verdict. |

The finite instance uses seven counter states and sixty-four total work units.
This bound is sufficient to cross each configured failure boundary.

The model does not infer production capacity values.
Production must select limits from measured resource envelopes and protocol governance.

The checked safe instance produced these results:

| Metric | Result |
| --- | --- |
| Generated states | 543,145 |
| Distinct states | 182,329 |
| Complete search depth | 95 |
| Remaining states | 0 |
| Safety violations | 0 |

Each of the ten unsafe controls produced its required counterexample.

## Required unsafe controls

Each unsafe configuration changes one rule.
The verification gate requires TLC to find the named invariant violation.

| Configuration suffix | Defect | Required violation |
| --- | --- | --- |
| `arrival_order_result_unsafe` | Select the first delivered branch. | `Inv_ArrivalOrderResult` |
| `wrapping_arithmetic_unsafe` | Wrap an overflowing reservation counter. | `Inv_CheckedReservation` |
| `saturating_arithmetic_unsafe` | Saturate and accept an oversized reservation. | `Inv_CheckedReservation` |
| `mutation_before_reservation_unsafe` | Change phase state before the reservation result. | `Inv_FailureNonMutation` |
| `schedule_mismatch_unsafe` | Replay with a validator-local schedule. | `Inv_ReplayScheduleAgreement` |
| `replay_without_enforcement_unsafe` | Replay accepted work without reserving its units. | `Inv_ReplayEnforced` |
| `noncumulative_replay_unsafe` | Check each replay event against zero usage. | `Inv_ReplayCumulativeVerdictAgreement` |
| `cross_shard_shared_budget_unsafe` | Share one phase counter across shards. | `Inv_ShardLocalAdmission` |
| `economic_debit_unsafe` | Debit wallet state for host work. | `Inv_EconomicSeparation` |
| `decode_after_allocation_unsafe` | Allocate decoded state before reservation. | `Inv_DecodeAllocationBound` |

Wrapping and saturation are separate controls.
Both operations can hide an oversized request while the stored counter appears valid.

## Rocq proofs

[`HostWorkBudget.v`](../../rocq/host_work_budget/theories/HostWorkBudget.v) proves checked reservation properties over arbitrary natural numbers.
It proves counter bounds, phase bounds, overflow rejection, failure non-mutation, and economic separation.

[`HostWorkExecution.v`](../../rocq/host_work_budget/theories/HostWorkExecution.v) proves execution properties.
It uses a generic dimension type, so production can add dimensions without changing the theorem shape.

The module proves that reservations on different dimensions commute.
It also proves rollback, cumulative replay agreement, mismatch rejection, and independent-shard commutation.

The transactional theorem compares two orders of one complete event multiset.
When each order sees a failure, both orders reject and restore the same initial usage.

The Rocq proofs contain no admitted obligations or additional axioms.
The verification script prints their assumptions for independent inspection.

## Verification

Run the focused gate from the repository root:

```bash
scripts/check-host-work-budget-formal.sh
```

The gate uses an on-disk TLC state directory.
The shared runner limits TLC memory, swap use, and worker count.

The gate performs these actions:

1. Parse the TLA+ modules with SANY.
2. Exhaust the safe finite state space with TLC.
3. Require every unsafe control to violate its specified invariant.
4. Compile both Rocq modules.
5. Reject admitted proof escapes and unexpected axioms.

TLA+ provides the concurrent state-machine notation and TLC checker used here.
See the [TLA+ project](https://lamport.azurewebsites.net/tla/tla.html).

Rocq checks the unbounded deductive proofs.
See the [Rocq documentation](https://rocq-prover.org/docs/).
