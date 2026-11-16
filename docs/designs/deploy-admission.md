# Deploy admission

## Retained resource limits

An explicit `casper.deploy-admission` object selects retained resource accounting.
The object requires `max-records`, `max-bytes`, and `max-inflight`.
No new numeric default is selected. An absent object keeps the existing policy for comparison.
In this mode, local block observations do not update the previous adaptive capacity estimator.
The record ceiling remains `max-records` as the pool fills and drains.

The resource guard counts every verified pool record and each unwritten reservation once per signature.
Canonical inclusion, expiry, rejection, and terminal verdicts do not release stored resources.
A successful removal releases resources when the next metadata snapshot reconciles the pool.
A failed removal keeps the resource charge.
Restart recovery reconstructs occupancy from all retained records, even above the configured limits.
Lower limits stop new intake without deleting accepted work.

The retained byte limit is strict, including an otherwise empty pool.
A record larger than the entire byte limit receives `deploy_too_large`, with HTTP status 413.
Temporary aggregate exhaustion keeps the existing HTTP 503 `deploy_pool_busy` response.
Duplicate detection precedes both refusals.
Declared phlo and local service samples do not change these resource limits.
The guard does not change the proposer's block limit or stop block processing and finalization.

The first patch covers retained records, reservations, configuration, and refusal classification.
One shared semaphore limits submission concurrency before transport decoding and signature verification.
Compatibility with the current dev and queue event contract requires a later transfer and validation.

## Existing comparison policy

Issue #710 shows sustained intake above local block capacity.
The deploy pool retains included deploys until their terminal verdict.
Its total size therefore does not measure fresh pending work.

Admission bounds all fresh pending signatures, including future deploys.
An earlier timestamp cannot bypass a full queue.
An eight-sample window measures first inclusions in accepted local blocks that leave fresh, eligible work waiting.
Each sample uses the block's selection candidates. Arrivals after selection do not count as unserved demand for that block.
Three blocks of measured inclusion capacity define the queue budget.
The estimate does not guarantee execution when service or consensus conditions change.

Admission also tracks encoded record bytes and declared phlo limits for fresh entries and active reservations.
Successful local inclusions teach the mean resource demand per included deploy over at most eight positive observations.
The count budget scales these means into byte and phlo budgets.
The byte budget includes one mean-sized record of headroom for small encoding-size variations.
The byte budget also uses three existing proposer byte budgets as its normal ceiling.
Before resource observations exist, admission uses this byte ceiling and the existing count budget.
A positive count budget permits one resource probe when the fresh queue is empty.
This probe can exceed a learned resource budget, as the proposer already permits one oversized selection.
The probe prevents a change in deploy size or declared phlo limit from permanently blocking progress.
The declared phlo limit is an execution allowance, not measured CPU time or memory use.
Encoded bytes exclude LMDB page overhead and do not measure resident memory.
Resource reductions preserve accepted entries. The reservation lock protects all resource accounting with the signature count.
This policy does not change protobuf fields, stored record formats, or the external queue controller.

Before the first sample, capacity is `3 * min(8, shard_cap)`.
Idle blocks do not reduce capacity.
Blocks that drain eligible work do not reduce capacity.
Their actual inclusions can support a gradual increase without entering the sample window.
Each increase is at most one currently estimated block, rounded up, with a minimum step of one.
An increased budget cannot exceed three times the observed inclusion count or three configured block limits.
A new sample with remaining eligible demand clears this recovery budget and uses the existing sample window.
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

The retained mode also requires an explicit `max-inflight` limit.
One semaphore covers HTTP and both gRPC deploy submission services.
Each submission acquires a permit before JSON or protobuf decoding. Acquisition never waits for capacity.
At this boundary, overload takes priority over duplicate detection because the deploy has not been decoded.
Other endpoints and peer block processing do not acquire these permits.
HTTP overload returns status 503 with `deploy_admission_busy`.
gRPC overload uses the existing `DeployResponse` service error envelope.
Blocking deploy work owns a permit until completion, including after client cancellation.
This limit bounds concurrent submissions, not their execution cost or finalization delay.

See the [specification](../specs/deploy-admission.md) and [correctness claims](../claims/deploy-admission.md).
