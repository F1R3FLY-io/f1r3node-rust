# Deploy admission

Issue #710 shows sustained intake above local block capacity.
The deploy pool retains included deploys until their terminal verdict.
Its total size therefore does not measure fresh pending work.

Admission bounds all fresh pending signatures, including future deploys.
An earlier timestamp cannot bypass a full queue.
An eight-sample window measures first inclusions in accepted local blocks that leave fresh, eligible work waiting.
Three blocks of measured inclusion capacity define the queue budget.
The estimate does not guarantee execution when service or consensus conditions change.

Before the first sample, capacity is `3 * min(8, shard_cap)`.
Idle blocks do not reduce capacity.
Blocks that drain eligible work do not reduce capacity.
Future deploys alone do not establish eligible demand.
A block with eligible demand and no first inclusion contributes zero.
A zero estimate permits one probe after fresh pending work drains, if the shard cap is positive.

A reservation occupies capacity before persistence.
A failed write or unwinding releases the reservation.
Inclusion or expiry during a write defers release until the write completes or rolls back.
Capacity reductions preserve previously accepted work.
Duplicate checks keep priority over full-queue refusals.
New deploys must have a validity height no later than the next block height plus three.
This admission horizon does not remove previously accepted future deploys from queue accounting.
The height horizon is a policy bound, not a measured optimum or a conversion from local inclusion capacity.

Admission checks pool metadata, rejected-buffer metadata, DAG generation, and lifecycle revision.
Pool, rejected-buffer, DAG, or reservation changes can require another snapshot attempt.
Three unsuccessful attempts produce a retryable contention error without saving the candidate.
Terminal records are permanent exclusions. Their concurrent writes can only make an earlier queue snapshot overcount pending work.
Lifecycle changes invalidate cached snapshots for later requests without independently restarting the current attempt.
The candidate's terminal and canonical status is checked again before reservation.
Storage reads, snapshot filtering, and deploy persistence run outside the reservation-cache mutex.
Reservation revisions prevent an older snapshot from replacing a completed write or rollback.
Consistent storage snapshots and verified-record fingerprints reduce repeated metadata decoding.
Named LMDB databases have separate shared write revisions, including across independent handles.
Writes through unnamed handles conservatively invalidate named databases in the same environment.

Expiry removal first saves an open lifecycle row.
New expiry rows are written in a batch. A failed write keeps the deploy copies and does not stop proposal preparation.
The lifecycle register checks the adopted floor and citation horizon before an expiry verdict.
Another block can therefore finalize a dropped local copy.
Persisted rows support restart recovery without a storage migration.

See the [specification](../specs/deploy-admission.md) and [correctness claims](../claims/deploy-admission.md).
