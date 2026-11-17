# Deploy admission

## Pool limits

The deploy pool has two limits: `casper.deploy-admission.max-records` and `casper.deploy-admission.max-bytes`.
The defaults are 512 records and 16777216 bytes of encoded records.
These values are resource ceilings, not a measured optimum or a latency guarantee.
Adaptive submission control is the job of the external deploy queue, not of the node.

The deploy store keeps the totals.
The first admission check computes the record count and the encoded byte total from the stored keys and value lengths, without decoding.
Opening the store and reading records do not scan for the totals.
After that, every insert and every deletion updates the totals under one store lock.
Admission checks the limits and inserts the record under that same lock, so concurrent submissions cannot share the last slot.
A deletion releases its charge only for a record that the store actually removed.
A record that the quarantine moves out of the live store also releases its charge.

Every stored record counts, including a deploy that a block already carries and a deploy with a terminal verdict.
The finalizer removes those copies, and each removal releases its charge.
After a restart, the totals include all stored records, even above lowered limits.
Lower limits stop new intake. They do not delete stored records.

A duplicate signature receives `duplicate_deploy` before any limit check.
A record larger than `max-bytes` receives `deploy_too_large`, HTTP 413. It cannot be accepted at any occupancy.
At the defaults, the transport limits refuse such a request first. The refusal applies when `max-bytes` is set below them.
A full record count or byte total receives `deploy_pool_busy`, HTTP 503. The client can retry the same signed deploy.
Encoded records exclude storage keys, LMDB page overhead, and other node allocations.
The limits do not change the proposer's block limit and never stop block processing or finalization.

## Admission order

1. The API refuses a closed validity window or a past expiration timestamp with `deploy_expired`.
2. The API refuses a `validAfterBlockNumber` above the next block number plus `max-parent-depth` with `invalid_deploy_window`.
3. Admission refuses a signature that a block, the pool, or the rejected-deploy buffer already holds.
4. Admission checks the limits without writing, so a refused deploy leaves no lifecycle row.
5. Admission opens the deploy's lifecycle row with its `validAfterBlockNumber`.
6. Admission inserts the record if the pool record and the limits are unchanged.

The next block number is `latest_block_number()`, the highest stored block number plus one.
The proposer's expiry filter uses the same number.
The future-height bound is the depth across which the shard merges parents.
A client that reads the height from any honest node of the shard names no height above it.

If the pool or rejected-buffer record for the signature changes during admission, admission tries again.
After three attempts it returns `deploy_admission_busy` without storing the deploy.

## Expiry outcome

The lifecycle row opens before the pool insert.
The finalizer settles every open row.
A deploy that no block carries settles `Expired` when finality passes its window end plus the citability horizon.
This does not depend on the local validator creating blocks.
A crash between the row and the insert leaves a row without a pool copy, which settles `Expired`.
A row left by a deploy that lost a concurrent insert settles to the deploy's actual outcome.

## Submission concurrency and metrics

The independent `api-server.deploy-max-inflight` setting is null by default, which disables this limit.
One semaphore covers HTTP and both gRPC deploy submission services.
Each submission takes a permit before JSON or protobuf decoding, without waiting.
Other endpoints and peer block processing take no permit.
An overloaded submission receives `deploy_admission_busy`: HTTP 503, or the gRPC `DeployResponse` service error envelope.
Blocking deploy work keeps its permit until it completes, including after client cancellation.

The `deploy.store.size` and `deploy.store.bytes` gauges report each deploy store's totals.
The `deploy-admission.refused` counter counts limit refusals.
The `deploy.admission.inflight` and `deploy.admission.inflight-limit` gauges report submission concurrency.

See the [specification](../specs/deploy-admission.md) and [correctness claims](../claims/deploy-admission.md).
