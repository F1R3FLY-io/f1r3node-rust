# Admission identity ownership

## Scope and status

This document defines the queued and active identity repair for `pr216-admission-backpressure`.
The production migration followed formal verification of the ownership contract.
The combined formal and native gate passed.
Focused lifecycle and producer regressions also passed after a deterministic fixture correction.
Dispatcher shutdown and retry progress remain separate, incomplete obligations.
It does not change Casper validity, voting, fork choice, certificates, block encoding, or settlement.

The previous producer paths insert a block hash into a shared set before enqueue.
The worker creates the cleanup guard only after dequeue.
Dropping a receiver with queued items releases byte reservations but leaves those set entries.
The existing worker guard also removes by hash, without checking an owner identity.
The latter is a missing protection, not evidence of an observed stale-worker race.

## Ownership contract

An admission identity is a private, non-cloneable guard for one block hash and one live ownership token.
The token has a unique allocation while any reference to that token exists.
No public operation clears an arbitrary block hash.

The queue creates the identity before admission.
Duplicate admission cannot replace an existing identity.
The queue transfers the identity and byte reservation together to the worker.
Rejected enqueue, queued cancellation, worker cancellation, panic, processing failure, and successful completion all use the same destructor.

The destructor removes the map entry only when the current token equals its token.
A previous token cannot clear a replacement identity.
This rule does not depend on a wrapping integer generation counter.

| Stage | Owner | Required release order |
|---|---|---|
| Queue admission | Nonblocking enqueue operation | Rejection drops the rejected item and its lease. |
| Early producer rejection | Local identity followed by local body | Drop the local body before the identity. |
| Queued body | Queue item | Drop the body before its lease. |
| Unpolled worker | Worker future containing the complete item | Drop the body before its lease. |
| Polled worker | Inner processing future and outer lease | Drop the inner future before the outer lease. |
| Completed processing | Compact result and outer lease | Release the lease before independent retry work. |

A lease contains the byte reservation and admission identity.
The destructor releases bytes before removing the identity.
An eventual retry wake must occur after both releases, not after a rejected temporary reservation alone.

The registry uses short, hash-sharded metadata critical sections.
It must not hold a registry lock across block loading, validation, storage, network calls, or asynchronous waits.
Independent block workers retain their existing concurrency limit.

## Formal correspondence

`AdmissionIdentityOwnership.tla` models concurrent attempts, duplicate keys, queue capacity, active workers, completion, cancellation, receiver closure, and early producer rejection.
The safe configuration uses two keys, three attempts, two queue slots, and two worker slots.
Three unsafe controls omit queued cleanup, admit duplicate ownership, or release by hash without checking the token.
These controls must violate `Inv_ExactLiveOwnership`.
The fourth control releases a rejected producer's identity before its body and must violate `Inv_PayloadCovered`.

The first model did not separate producer rejection from body destruction.
Review identified that missing transition boundary.
The expanded model passed 2,648 reachable states and all four named controls before the rejection repair.

`AdmissionIdentityOwnership.v` proves pointwise claim and release rules for arbitrary natural-number identities and keys.
The twelve proofs include arbitrary finite sequences of unrelated and stale releases, plus body-before-identity release rules.
Allocation identity refines the abstract token equality relation.
Private guard construction and Rust ownership enforce the unforgeable-token premise.

The byte-reservation proof remains a separate obligation.
Production-linked tests must compose identity ownership with the actual reservation and channel operations.
Native asynchronous tests must cover unpolled task cancellation, cancellation during processing, and receiver destruction.
Loom must import the actual registry and destructor with instrumented synchronization primitives.

The registry has sixteen hash shards in production.
Loom covers one- and two-shard instantiations with at most three threads and 1,000 branches per execution.
The `len` method reads shards separately and provides observational metrics, not a linearizable global snapshot.
Admission never uses that metric as a capacity decision.

The fixture inspection helper now reads the registry without draining or re-enqueuing payloads.
Its old implementation released reservations while retaining bodies and ignored failed re-admission.
Native tests require repeated inspection to preserve queue capacity, reserved bytes, and the original queued item.

The model does not yet establish dispatcher termination or retry-service liveness.
Those obligations remain in the [admission and recovery plan](admission-and-recovery-backpressure.md).
No encoded-byte bound establishes a decoded-heap or whole-process memory bound.
