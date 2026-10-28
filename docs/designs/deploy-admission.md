# Deploy admission

Issue #710 shows sustained intake above local block capacity.
The deploy pool retains included deploys until their terminal verdict.
Its total size therefore does not measure fresh pending work.

Admission bounds all fresh pending signatures, including future deploys.
An earlier timestamp cannot bypass a full queue.
An eight-sample window measures first inclusions in accepted local blocks with fresh demand.
Three blocks of measured inclusion capacity define the queue budget.
The estimate does not guarantee execution when service or consensus conditions change.

Before the first sample, capacity is `3 * min(8, shard_cap)`.
Idle blocks do not reduce capacity.
A block with eligible demand and no first inclusion contributes zero.
A zero estimate permits one probe after fresh pending work drains, if the shard cap is positive.

A reservation occupies capacity before persistence.
A failed write or unwinding releases the reservation.
Inclusion or expiry during a write defers release until the write completes or rolls back.
Capacity reductions preserve previously accepted work.
Duplicate checks keep priority over full-queue refusals.

Admission checks pool metadata, rejected-buffer metadata, DAG generation, and lifecycle revision.
State changes during all three attempts produce a retryable contention error without saving the candidate.
Consistent storage snapshots and verified-record fingerprints reduce repeated metadata decoding.
Environment-wide LMDB revisions can still cause reconciliation after unrelated writes.

Expiry removal first saves an open lifecycle row.
The lifecycle register checks the adopted floor and citation horizon before an expiry verdict.
Another block can therefore finalize a dropped local copy.
Persisted rows support restart recovery without a storage migration.

See the [specification](../specs/deploy-admission.md) and [correctness claims](../claims/deploy-admission.md).
