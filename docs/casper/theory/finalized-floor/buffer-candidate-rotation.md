# Buffer candidate rotation

## Status and scope

The buffer candidate index is implemented.
Combined focused qualification passed in `target/verification/buffer-durable-membership/run.oimpzM`.
The production retry services do not yet use the new index.
The [admission plan](admission-and-recovery-backpressure.md) defines that remaining integration work.

The index selects hashes for examination, not blocks for admission.
Each caller must still check durable membership, certificate dependencies, admitted metadata, duplicate ownership, quarantine, and available capacity.
No index result changes block validity or consensus authority.

## Membership and order

Candidate membership contains explicitly buffered blocks and referenced missing block roots.
It excludes typed certificate dependency keys.
The [durable membership contract](buffer-durable-membership.md) defines those identities and their persistence.

The process-local index uses a hash map with predecessor and successor links.
Each live candidate has one entry.
The index has no tombstones or historic entries.

New identities join the tail.
Duplicate insertion leaves the complete order unchanged.
Removal repairs adjacent links and removes the selected identity.
One examination returns the head and moves that head to the tail.

The buffer mutation guard protects membership changes.
A separate short index mutex protects link changes and selection.
The index mutex does not cover block loading, dependency validation, asynchronous waits, or validator execution.
Storage changes publish candidate changes only after the strict durable transaction succeeds.

Startup reconstructs live identities from durable metadata and sorts them once for a canonical initial order.
The cursor is not persisted.
The fairness guarantee applies within one running process, not across indefinitely repeated restarts.

## Early capacity exits

A scanner must rotate candidates when it examines them.
It must not rotate a full page and then abandon that page after its first candidate reaches capacity.

For two candidates, that old page pattern can restore the original order after every failed first attempt.
The second candidate then receives no attempt.
Single-candidate rotation prevents that repeated skip.

The shared pump must limit examinations per turn.
If concurrent removal shrinks the index during a turn, the pump must also prevent repeated examination within that turn.
A bounded visited set or equivalent turn state can provide this control.
The index alone does not implement the complete pump.

## Formal guarantees

[`BufferCandidateRotation.tla`](../../../../formal/tlaplus/block_admission/BufferCandidateRotation.tla) checks exact membership, unique entries, and bounded overtaking.
Insertion, removal, duplicate insertion, and examination are separate actions.
Its safe configuration checks four identities and weak fairness of examination.
All 1,641 reachable states and the temporal property passed.

Three unsafe controls must violate `Inv_NoOvertaking`.
They place new arrivals first, move duplicate identities, or rotate an unexamined page.
Each unsafe control produced the required counterexample.

[`BufferCandidateRotation.v`](../../../../formal/rocq/finalized_floor/theories/BufferCandidateRotation.v) proves nine closed theorems for arbitrary finite queues and operation histories.
New arrivals cannot increase an existing candidate's rank.
Removing another candidate cannot increase that rank.
Examining another head decreases the rank by one.
Thus, a surviving candidate cannot be bypassed for a complete rotation of its initial queue.

The bound counts examination operations, not elapsed time or successful admissions.
Progress requires continued pump scheduling.
Admission additionally requires available capacity, complete dependencies, quarantine expiry, and a block that fits the local resource limits.

## Production correspondence

The native index properties compare every operation against an independent FIFO reference.
They check both link directions, complete reachability, unique entries, membership, and exact order.
Additional properties cover continuous arrivals and duplicate updates.

The Loom target imports the actual production index.
Two producers can insert identities, repeat insertion, and retire another identity while the consumer examines candidates.
The existing candidate must receive service within its initial bound.
The final index must contain exactly the surviving identities.

The buffer's generated history tests check index membership after every graph mutation, failed transaction, and restart.
Named tests distinguish an ordinary `0xff` block hash from the separate certificate namespace.
These tests do not prove that every retry producer calls the index correctly.
The shared-pump tests must establish that integration separately.

## Resource bounds

Each index operation changes a constant number of links.
Hash-map lookup has expected constant cost for the fixed-size hash keys.
The index does not scan all candidates during selection or removal.
Startup uses a one-time sort of reconstructed identities.

Resident index metadata remains proportional to live candidate count.
This is not a fixed whole-buffer memory bound.
The intended scanner bound is a bounded number of temporary hashes and at most one loaded scanner body.
The current index provides the required incremental hash selection, not the complete scanner residency guarantee.

Use the [focused buffer gate](buffer-durable-membership.md#formal-correspondence) to run the proofs, models, native properties, and Loom target.
