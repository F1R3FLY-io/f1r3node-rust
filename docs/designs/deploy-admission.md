# Deploy admission

## Retained resource limits

Retained resource accounting is the default admission policy.
The `casper.deploy-admission` object requires `max-records` and `max-bytes`.
The provisional pool defaults are 512 records and 16 MiB of encoded values.
These values are initial resource budgets, not a measured optimum or a latency guarantee.
They are engineering policy choices. Throughput and retained-memory measurements have not calibrated this profile.
The record limit bounds per-signature accounting. The byte limit bounds encoded values. The submission limit bounds concurrent intake work.
No formula derives these values from the HTTP limit, the proposer cap, or the previous count estimator.
An absent configuration uses these defaults. An explicit `casper.deploy-admission = null` selects the previous policy for comparison.
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
Encoded values exclude storage keys, LMDB page overhead, and other node allocations.
Recovery and independent storage writes can exceed these intake limits. Such excess stops new intake without deleting accepted records.

The default byte budget is exactly 16777216 bytes.
Existing HTTP and API gRPC configuration values use `16M`, which the transport byte parser reads as 16000000 bytes.
Transport limits measure request bodies or messages. They do not define the retained byte budget.
Use integer bytes for `casper.deploy-admission.max-bytes` to avoid differences between configuration parsers.

Both policies share a three-height admission horizon beyond the next block height.
For a nonempty DAG, use the highest stored block number plus one as the next height.
The horizon does not remove previously stored future deploys from queue accounting.
Three heights are an inherited policy choice, not a measured optimum or a conversion from local inclusion capacity.

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
The three-block multiplier, eight-sample window, and initial eight-deploy estimate are uncalibrated policy choices from the comparison implementation.

The comparison policy does not impose byte or declared-phlo budgets.
Local service samples change only the count capacity.
The declared phlo limit is an execution allowance, not measured CPU time or memory use.
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

Admission checks pool metadata, rejected-buffer metadata, DAG generation, and lifecycle revision.
Pool, rejected-buffer, DAG, or reservation changes can require another snapshot attempt.
Three unsuccessful attempts produce a retryable contention error without saving the candidate.
This retry limit bounds snapshot work. It does not establish three as an optimal retry count or retry a refused client request.
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

## Submission concurrency and metrics

The independent `api-server.deploy-max-inflight` setting defaults to null, which disables this additional ingress limit.
An explicit positive value enables the limit without changing the retained policy.
Select that value from the deployment's resources and observed intake pressure. No concurrency default has been calibrated.
The limit can also protect the count-only comparison policy.
One semaphore covers HTTP and both gRPC deploy submission services.
Each submission acquires a permit before JSON or protobuf decoding. Acquisition never waits for capacity.
At this boundary, overload takes priority over duplicate detection because the deploy has not been decoded.
Other endpoints and peer block processing do not acquire these permits.
HTTP overload returns status 503 with `deploy_admission_busy`.
gRPC overload uses the existing `DeployResponse` service error envelope.
Blocking deploy work owns a permit until completion, including after client cancellation.
This limit bounds concurrent submissions, not their execution cost or finalization delay.
When this limit is disabled, transport decoding retains its previous execution placement.

Retained occupancy metrics report `deploy.admission.retained.records` and `deploy.admission.retained.bytes`.
These gauges include unwritten reservations and stored records from the latest reconciliation.
Matching `records-limit` and `bytes-limit` gauges report the configured ceilings.
The `deploy.admission.inflight` and `deploy.admission.inflight-limit` gauges report submission concurrency.
The `pending-ahead-measured` gauge is zero in retained mode and one in comparison mode.
The existing `pending_ahead` gauge is reset to zero when its measurement is unavailable.
That zero does not assert that no pending work exists.
Checked resource arithmetic blocks new intake if accounting invariants fail.
It does not wrap counters, panic during cleanup, or authorize deletion of stored deploys.

See the [specification](../specs/deploy-admission.md) and [correctness claims](../claims/deploy-admission.md).
