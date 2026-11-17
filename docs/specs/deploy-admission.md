# Deploy admission specification

## Pool limits

Use pool limits of 512 records and 16777216 encoded bytes by default.
Read them from `casper.deploy-admission.max-records` and `casper.deploy-admission.max-bytes`.
A missing field takes its default. Reject unknown fields in this object.
Express the byte limit as integer bytes.
Do not derive the limits from transport limits, the proposer block limit, or declared phlo.

Keep the record count and the encoded byte total in the deploy store.
Compute both totals from the stored keys and value lengths at the first limit check, without decoding.
Do not scan for the totals when the store opens or when it reads records.
After that, update both totals under one store lock for each insert and each deletion.
Release the charge of a deletion only for a key that the deletion removed.
Release the charge of a record that the quarantine moves out of the live store.
Count every stored record: fresh, included, rejected, expired, and terminal copies, until their removal.
After a restart, count all stored records, even above lowered limits.
Lower limits stop new admission. They do not delete stored records.

Under the store lock, in this order:

1. Return `Changed` if the stored record for the signature differs from the admission snapshot.
2. Return `AlreadyPresent` for a verified stored record.
3. Refuse a record larger than `max-bytes` with `TooLarge`, at any occupancy.
4. Refuse with `Full` when the record count is at `max-records`.
5. Refuse with `Full` when the byte total plus the record exceeds `max-bytes`.
6. Insert the record if absent, and add it to both totals.

With sufficient byte capacity, a record limit of N must admit N distinct deploys before any proposal.
Deploy N + 1 must receive a pool-busy refusal without persistence.
Removing one stored record must permit one replacement.
Concurrent submissions for the last free slot must admit exactly one.

This policy limits encoded pool records.
It does not bound storage keys, LMDB page allocation, total node RSS, or contract execution cost.

## Validity window

Use `latest_block_number()`, the highest stored block number plus one, as the next block number.
Use the same number for the API expiry check and the proposer's expiry filter.
Refuse a closed validity window or a past expiration timestamp with `deploy_expired`.
Refuse a `validAfterBlockNumber` above the next block number plus `max-parent-depth` with `invalid_deploy_window`.

## Admission

Refuse a signature that a block, the pool, or the rejected-deploy buffer already holds with `duplicate_deploy`.
Check the limits before the lifecycle write, so that a refused deploy opens no lifecycle row.
Open the lifecycle row with the deploy's `validAfterBlockNumber` before the pool insert.
Retry when the pool or rejected-buffer record for the signature changes during admission.
After three attempts, return `deploy_admission_busy` without persistence.
Save no deploy and send no heartbeat or proposal signal after a refusal.

## Responses

| Condition | HTTP | gRPC service error prefix | Retry |
|---|---|---|---|
| Known signature | 409 `duplicate_deploy` | — | No |
| Record larger than `max-bytes` | 413 `deploy_too_large` | `deploy_too_large:` | No |
| Record or byte limit full | 503 `deploy_pool_busy` | `deploy_pool_busy:` | Yes, same signed deploy |
| Admission contention, or submission limit reached | 503 `deploy_admission_busy` | `deploy_admission_busy:` | Yes, same signed deploy |
| Closed validity window or expiration timestamp | 400 `deploy_expired` | `deploy_expired:` | No |
| Validity height above the horizon | 400 `invalid_deploy_window` | `invalid_deploy_window:` | Yes, after the chain advances |

gRPC keeps the existing `DeployResponse` service error envelope and logs these refusals at debug level.
Unrelated storage errors propagate as errors.

## Expiry

The finalizer settles every open lifecycle row.
A row whose deploy no block carries settles `Expired` past its window end plus the citability horizon.
Preserve existing rows and terminal records. Admission never reopens a row behind a terminal record.
Persisted rows remain available after restart.

## Submission concurrency

Leave `api-server.deploy-max-inflight` null by default, which disables the limit.
Reject zero or a value above the semaphore limit before starting the API servers.
Share one semaphore across HTTP and both gRPC deploy services.
Acquire without waiting before JSON or protobuf decoding.
Hold the permit until blocking work completes, even when the request future is canceled.
Release it after rejection, completion, or unwinding. Other endpoints and peer traffic take no permit.

## Metrics

Publish `deploy.store.size` and `deploy.store.bytes` for each deploy store on every change.
Count limit refusals in `deploy-admission.refused`.
Publish `deploy.admission.inflight` and `deploy.admission.inflight-limit`.

See the [design](../designs/deploy-admission.md).
