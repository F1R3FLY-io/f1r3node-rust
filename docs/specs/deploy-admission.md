# Deploy admission specification

## Admission

Let `ahead` count fresh deploys before the candidate in `(valid_after_block_number, time_stamp, sig)` order.
Fresh deploys have not expired by the next block or by time.
Fresh deploys have no terminal record, visible inclusion, or rejected-buffer entry.
Future deploys count only when their selection keys precede the candidate.

Keep at most eight demand samples from accepted local blocks, in sender sequence order.
Demand excludes deploys whose validity height does not yet permit inclusion.
Ignore repeated or older sequences, peer blocks, idle blocks, and failed proposals.
Count actual first inclusions of fresh local signatures, including failed executions.
Exclude recovery and signatures included before this block.
Record zero when fresh work waits but the block includes none.

With samples, capacity is `floor(3 * sum(inclusions) / sample_count)`.
Bound capacity by three times the shard cap.
Without samples, capacity is `3 * min(8, shard_cap)`.
For zero measured capacity and an empty fresh queue, allow one probe if the shard cap is positive.
Otherwise, accept only when `ahead + 1 <= capacity`.
Use integer arithmetic without overflow.

Check duplicates before overload under the reservation lock.
Stop the pool scan when enough predecessors establish refusal.
For refusal, the reported queue position is a lower bound.
Return `DeployError::PoolBusy` for overload.
Save no deploy and send no heartbeat or proposal signal after refusal.
HTTP returns status 503 and error kind `deploy_pool_busy`.
gRPC retains the existing `DeployResponse` service error envelope.
The error message tells the client to retry or use another validator.

## Expiry

Before deletion, save an unknown signature as an open lifecycle row with its validity height and no events.
Queue the signature for the active lifecycle register.
Preserve existing rows and terminal records.
If the lifecycle write fails, return the error before deletion.
Schedule rebuild reads persisted open rows after restart.
Evaluation uses the existing floor and citability bound.
With disabled depth checks, an unknown expiry remains pending under the existing lifecycle rule.

## Verification

Tests cover positions 24 and 25 at service eight, order, exclusions, parallel reservations, cold start, zero service, and recovery.
Tests cover API errors, expiry recording, warm scheduling, restart, peer inclusion, duplicate drops, and write failure.
See the [design](../designs/deploy-admission.md).
