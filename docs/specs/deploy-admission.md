# Deploy admission specification

## Admission

Count every fresh pending signature against capacity, regardless of its selection position.
Fresh entries exceed the earliest valid-after boundary and have not expired by time.
Fresh entries have no terminal record, canonical appearance, or rejected-buffer entry.
Future entries consume capacity.
Accept a new deploy only when its validity height is at most the next block height plus three.
The deploy lifespan and time-expiry checks still apply.
Keep previously stored future entries in queue accounting, even when they exceed the admission horizon.
The proposer retains `(valid_after_block_number, time_stamp, sig)` selection order.

Keep at most eight demand samples from accepted local blocks, in sender sequence order.
Sample only blocks that leave fresh local work eligible for that block waiting after their first inclusions.
Future entries alone do not qualify a block for sampling.
Ignore repeated or older sequences, peer blocks, idle blocks, and failed proposals.
Count distinct first inclusions of fresh local signatures, including failed executions.
Exclude recovery entries and signatures included before this block.
Record zero when eligible fresh work waits but the block includes none.

Missing own blocks do not create samples or cause time-based decay.

With samples, capacity is `min(3 * shard_cap, floor(3 * sum(inclusions) / sample_count))`.
Without samples, capacity is `3 * min(8, shard_cap)`.
For zero measured capacity and an empty fresh queue, allow one probe if the shard cap is positive.
A zero shard cap always disables admission.
Accept a fresh candidate only when occupancy is below capacity.
Capacity reductions preserve accepted work.
Use integer arithmetic without overflow.

Check duplicates before overload under the reservation lock.
Reserve capacity before persistence.
If the deploy remains eligible, change its marker to pending after a successful write.
Release an uncommitted reservation after a write error or unwinding.
Defer exclusion of a reserved entry until commit or rollback, with one release per entry.

Refresh queue bounds, pool metadata, rejected-buffer metadata, DAG generation, and lifecycle revision.
Retry when the pool, rejected buffer, DAG, or reservation state changes incompatibly with the snapshot.
Use lifecycle revisions for cache invalidation between requests.
Do not retry solely because concurrent terminal writes change lifecycle revisions.
Terminal writes permanently exclude signatures, so an earlier queue snapshot can overcount but cannot create spare capacity.
Recheck the candidate's terminal and canonical status before reservation.
Read storage and prepare snapshot updates outside the reservation-cache mutex.
Preserve active reservations when reconciling stored entries. Reject updates based on an obsolete reservation revision.
Release the reservation-cache mutex before deploy persistence.
Use at most three refresh attempts, without sleep or backoff.
Release attempt guards before the next attempt.
After refresh exhaustion, return `DeployError::AdmissionBusy` without persistence.

Return `DeployError::PoolBusy` for overload.
Save no deploy and send no heartbeat or proposal signal after refusal.
HTTP returns status 503 with `deploy_pool_busy` or `deploy_admission_busy`.
HTTP returns status 409 with `duplicate_deploy` for a known signature.
HTTP returns status 400 with `invalid_deploy_window` for an invalid validity height or an expired admission window.
gRPC retains the existing `DeployResponse` service error envelope.
gRPC treats `InvalidFreshness` as an expected refusal and logs it at debug level.
Clients must resubmit refused requests.
Unrelated storage errors propagate as errors.

## Metadata

Read raw records from one consistent LMDB snapshot.
Use a shared write revision for each named LMDB database to detect committed writes, including independent handles.
Account for batch writes and conditional mutations that change stored data.
Do not advance the revision for a failed transaction or a conditional mutation that changes nothing.
Conservatively invalidate named database revisions after writes through unnamed handles in the same environment.
If the revision changes during the read, invalidate the cached revision.
The method returns the consistent snapshot.
Reconcile storage again on the next call.
Preserve metadata generation when entries remain unchanged.

Reuse decoded metadata only when its raw key and value match the verified Blake2b-256 fingerprint.
Decode changed records with the existing quarantine rules.
Controlled mutations invalidate affected fingerprints.
Removal and reconciliation discard fingerprints for absent records.
Compact an oversized fingerprint map and release its allocation when empty.
Retain metadata and fingerprints without retaining complete deploy bodies.

## Expiry

Before deletion, save an unknown signature as an open lifecycle row with its validity height and no events.
The next lifecycle evaluation reads the persisted open row.
Preserve existing rows and terminal records.
Write new lifecycle rows in a batch.
If the lifecycle write fails, log the failure and retain deploy copies without stopping proposal preparation.

Persisted open rows remain available after restart.
Evaluation uses the existing adopted floor and citation horizon.
With disabled depth checks, an unknown expiry remains pending under the existing lifecycle rule.

## Verification

Regression tests cover arrival order, future entries, concurrent reservations, capacity reduction, zero-service recovery, rollback, and duplicate priority.
Tests cover bounded snapshot retries, contention recovery, external writes, changed records, quarantine, reopen, fingerprint reuse, and map cleanup.
Tests also cover API errors, expiry recording, warm scheduling, restart, peer inclusion, duplicate drops, and write failure.
Historical performance and heartbeat measurements require separate evidence for each tested revision.

See the [design](../designs/deploy-admission.md).
