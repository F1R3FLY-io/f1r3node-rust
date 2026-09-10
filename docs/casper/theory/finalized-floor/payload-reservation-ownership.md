# Block payload reservation ownership

## Scope

Block admission reserves encoded bytes against a local processing budget.
This reservation is not a token charge, a phlogiston limit, or a consensus validity rule.
It controls local queue and worker ownership.
The [backpressure plan](admission-and-recovery-backpressure.md) defines the complete repair scope.

The local reservation must remain held while the corresponding admitted payload remains owned.
Removing a block from the processing queue does not release its worker reservation.
Completing replay must not leave an unreserved block body in a result queue or worker continuation.

## Confirmed defect and repair

The previous worker cloned its block before processing.
The original remained in the worker while processing returned the second block through its outcome.
The worker sent that returned block to a 128-entry result queue.
It then released the admission reservation before its dependency scan.
Both the result queue and worker continuation could retain corresponding block bytes after release.

The repaired worker moves its block into processing.
The processing outcome contains only the processing status.
The external result contains the block hash and status, not the block body.
The worker retains its reservation until processing and result publication finish.
The sole production result consumer still drains results without using a block body.

The inspected local `dev` also sent full block results.
This change repairs shared result retention rather than restoring a different `dev` result type.
It does not change validation, votes, fork choice, or block encoding.

## Ownership contract

Let $`A`$ denote the finite set of processing attempts.
For attempt $`a`$, let $`s_a`$ denote its encoded size.
Let $`P`$ denote attempts with a live admitted payload, and let $`R`$ denote attempts with a held reservation.
Let $`C`$ denote the local byte capacity.
The contract requires:

```math
P \subseteq R, \qquad
\sum_{a\in P} s_a \le \sum_{a\in R} s_a \le C.
```

A successful reservation uses checked addition and an atomic compare-and-swap operation.
A failed reservation creates no owner.
Count-admission failure drops its rejected reservation.
Successful queue admission transfers the same reservation to the worker.
The reservation destructor releases its exact charge once.

The queue item declares its block before its reservation.
Rust drops those fields in declaration order when an intact queued item is destroyed.
The normal worker path moves the block into its processing future before it explicitly releases the reservation.
The separate cancellation and duplicate-identity audit must still verify the complete dispatcher lifecycle.

The formulas count one logical encoded payload per attempt.
They do not count physical clone multiplicity, decoded object expansion, allocator overhead, or cached storage values.
Removing the worker clone and result body repairs two specific ownership violations.
It does not prove a whole-node resident-memory ceiling.

## Formal model

`BlockPayloadOwnership.tla` separates reservation, staged admission, queue residence, active processing, result publication, result consumption, continuation release, and cancellation.
Its safe configuration permits three attempts, two simultaneous workers, two queued blocks, and two queued results.
The configuration explores sizes one and two against capacity three.

The safe model passed 10,893 reachable states.
The result-body control violated `Inv_ExactReservationCoverage` after reservation release with a live result payload.
The worker-tail control violated the same invariant with a live worker copy.
These are safety counterexamples, not claims that every historical test failure had this cause.

`PayloadReservationCoverage.v` proves 13 ownership results for arbitrary finite lists and operation sequences.
The operations reserve, move payload ownership, and release after the last payload owner.
The proof covers repeated release attempts, arbitrary owner counts, and the aggregate logical-payload ceiling.
Compilation and independent proof checking passed with closed assumptions.

The Rocq transition checks an abstract aggregate budget.
It does not prescribe a linear-time scan or a global production lock.
Production uses an atomic counter, and separate tests establish its reserve-and-release correspondence.

## Production-linked tests

The production CAS loop and reservation destructor reside in `block_processing_queue/admission_budget.rs`.
Both production and Loom compile that same source file.
The test adapter supplies Loom atomics and reference-counted pointers instead of the standard-library versions.
The metric callback does not participate in the ownership contract.

| Boundary | Evidence |
| --- | --- |
| Competing byte reservations | Actual CAS loop and destructor under Loom. |
| Queue-to-worker transfer | Same reservation survives transfer before release. |
| Release and reacquisition | An earlier reservation cannot decrement a later reservation after its own destructor completes. |
| Integer overflow | Competing maximum-size requests retain checked-add behavior. |
| Arbitrary reserve/release histories | Generated native tests compare the actual counter with the sum of held reservation charges. |
| Cancellation, panic, and receiver drop | Native tests verify that the actual reservation reaches zero after cleanup. |
| Completion type change | Node compilation checks the compact result through its production consumer. |

These tests do not establish stale duplicate-marker safety.
They also do not replace full worker-lifecycle tests for cancellation during replay, result backpressure, and dependency recovery.
Those obligations remain explicit in the task plan.

The [work log](../../../work-logs/task-pr216-admission-backpressure-2026-09-06.md) records source hashes, commands, limits, and current qualification status.
