# Deploy admission specification

## Explicit retained-resource policy

The retained-resource policy is the default.
Use provisional pool limits of 512 records and 16777216 encoded bytes.
Treat this profile as an initial engineering choice, without a measured optimum or a whole-node memory guarantee.
Do not derive the profile from transport message limits, proposer block limits, or comparison-policy samples.
Express the retained byte budget as integer bytes.
Apply the existing three-height arrival horizon to both policies.
For a nonempty DAG, calculate the next height as the highest stored block number plus one, without another increment.
Accept a new validity height only through that next height plus three, subject to the other freshness checks.
Configuration can override each field through the embedded default object.
Require `max-records` and `max-bytes` in `casper.deploy-admission`. Reject unknown fields in this object.
Configure submission concurrency separately through `api-server.deploy-max-inflight`.
Leave that setting null by default. Apply an additional concurrency ceiling only when explicitly configured.
Reject a zero concurrency limit or a value above the semaphore limit before starting the API servers.
An explicit null concurrency limit disables ingress limiting independently of the retained policy.
Use retained defaults when the object is absent. Select the existing comparison policy only with an explicit null object.
Do not derive these limits from service samples, declared phlo, or the proposer block limit.
With sufficient byte capacity, an explicit record limit of 128 must admit 128 distinct deploys before any proposal.
The next distinct deploy must receive a pool-busy refusal without persistence.
Deleting one stored record must permit one replacement after metadata reconciliation.

Count all verified pool records, including expired, included, rejected, and terminal copies until their removal.
Count unwritten reservations against the same limits before persistence.
Reconcile independent storage changes through the existing metadata generations and reservation revisions.
Count a stored signature and its active reservation once.
If another handle persists a reserved signature, preserve its resource charge when the original attempt rolls back.
Do not release stored resources because a freshness window or terminal status changes.
Reconstruct all occupancy after restart, even above the configured limits.
Lower limits preserve accepted records and stop new admission until sufficient resources become available.

Reject a candidate that exceeds the entire byte budget, including in an empty pool.
Return HTTP 413 with `deploy_too_large` for this refusal.
Start the gRPC service error with `deploy_too_large:` for the same permanent refusal.
Return HTTP 503 with `deploy_pool_busy` when the aggregate count or byte budget is exhausted.
Keep duplicate detection ahead of both resource refusals.
An expired write remains charged until the storage removal succeeds and metadata reconciliation observes the removal.
Keep block processing, proposal creation, and finalization independent of intake refusal.

This policy limits encoded pool records. It does not bound LMDB page allocation, total node RSS, or contract execution cost.
Exclude storage keys and page overhead from encoded-value accounting.
Preserve restored or independently written records above these limits, and refuse new intake until space becomes available.
Share one submission semaphore across HTTP and both gRPC API servers.
Acquire without waiting before JSON or protobuf decoding. Reject excess submissions through the existing admission-busy response.
An ingress refusal precedes signature and duplicate checks. Retries must preserve the same signed deploy.
Keep gRPC refusals in the existing service error envelope, including refusals before protobuf decoding.
Hold the permit until blocking work completes, even when the request future is canceled.
Release the permit after rejection, completion, or unwinding. Do not charge other endpoints or peer traffic.
When ingress limiting is disabled, preserve the previous transport decoding execution placement.
Publish retained record and byte occupancy, their limits, and current submission concurrency.
Mark `pending_ahead` as unavailable in retained mode through `pending-ahead-measured = 0` and reset its value to zero.
Use checked resource arithmetic. Block new intake after an accounting invariant failure without deleting stored records.
Validation against the current dev and external queue remains a later step.

## Existing comparison policy

Count every fresh pending signature against capacity, regardless of its selection position.
Fresh entries exceed the earliest valid-after boundary and have not expired by time.
Fresh entries have no terminal record, canonical appearance, or rejected-buffer entry.
Future entries consume capacity.
The deploy lifespan and time-expiry checks still apply.
Keep previously stored future entries in queue accounting, even when they exceed the admission horizon.
The proposer retains `(valid_after_block_number, time_stamp, sig)` selection order.

Keep at most eight demand samples from accepted local blocks, in sender sequence order.
Sample only blocks that leave fresh local work eligible for that block waiting after their first inclusions.
Capture eligible candidates during block selection. Count their actual inclusions after successful local proposal acceptance.
Do not sample the pool at insertion time or count arrivals after selection.
Future entries alone do not qualify a block for sampling.
Ignore repeated or older sequences, peer blocks, idle blocks, and failed proposals.
Count distinct first inclusions of fresh local signatures, including failed executions.
Exclude recovery entries and signatures included before this block.
Record zero when eligible fresh work waits but the block includes none.

Missing own blocks do not create samples or cause time-based decay.

With samples, capacity is `min(3 * shard_cap, floor(3 * sum(inclusions) / sample_count))`.
Without samples, capacity is `3 * min(8, shard_cap)`.
The following recovery rule can raise this baseline without adding drained blocks to the sample window.
Let `B` be the larger of the baseline and recovery budgets before the configured block-limit clamp.
For a drained block with `I` actual first inclusions, update recovery only if `3 * I > B`.
Set the recovery budget to `min(3 * I, B + max(1, ceil(B / 3)))`, using saturating arithmetic.
Capacity is the larger budget, capped at `3 * shard_cap`.
A block with remaining eligible demand clears recovery before updating the eight-sample window.
Repeated sequences, older sequences, and idle blocks cannot increase recovery.
For zero measured capacity and an empty fresh queue, allow one probe if the shard cap is positive.
A zero shard cap always disables admission.
Accept a fresh candidate only when occupancy is below capacity.
Capacity reductions preserve accepted work.
Use integer arithmetic without overflow.

Do not derive byte or declared-phlo budgets from comparison-policy samples.
Keep this policy count-only for comparison with the retained resource guard.

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
