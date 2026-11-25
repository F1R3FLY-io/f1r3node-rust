# Shared Events and Compact Startup Replay

Status: draft proposal. The publication protocol and memory savings require validation.
Date: 2026-11-25.

## Objective

Remove complete JSON strings from per-connection replay bookkeeping.
Share immutable event payloads across the startup buffer and event subscribers.
Preserve startup replay, live event order, lag reporting, and the external WebSocket schema.

The [memory evaluation contract](README.md#memory-evaluation-contract) applies.
This proposal changes event delivery internals, not consensus ordering or deploy inclusion.
It complements [typed JSON boundaries](memory-typed-json-boundaries.md), but does not require that serializer change.

## Current Retention

The [event bus](../../shared/src/rust/shared/f1r3fly_events.rs) stores owned `F1r3flyEvent` values in its startup buffer.
It requests a broadcast capacity of 100 events.
Publication clones the payload into the startup buffer while startup remains open.
Broadcast receivers also receive cloned event values.

The [WebSocket handler](../../node/src/rust/web/events_info.rs) clones the startup vector for each connection.
It serializes each startup event and inserts the complete string into `HashSet<String>`.
An entry disappears only when the same serialized value arrives through the live stream.
Events published before subscription normally have no such live copy.
Their strings can remain until the connection ends.

The startup buffer is intended to freeze after `seal_startup()` but has no explicit byte cap.
The current flag check and buffer append use separate synchronization steps.
A scalar-boundary implementation must establish publication ordering instead of assuming that seal and append form one atomic operation.
This is not evidence of continuing growth after startup publishers finish and sealing completes.
It is a per-client duplication problem and a separate prolonged-startup retention risk.

## Proposed Representation

Use one immutable published-event allocation shared through `Arc`.
Each allocation contains a process-local sequence number and one `F1r3flyEvent` payload.
The startup buffer and broadcast channel retain references to that allocation.
Receivers clone the reference, not the nested payload.

The sequence number identifies a publication, not its content.
Two publications with identical payloads receive different numbers.
The number stays internal and does not change public event schemas.
It is not a durable cursor and does not span node restarts.

Initially, retain typed events rather than both typed events and cached serialized text.
Caching both representations could offset the saving.
A shared text representation needs a separate benchmark and explicit byte accounting.

## Publication Protocol

A scalar replay boundary is safe only when buffered startup events form a complete publication prefix.
An atomic counter alone does not establish that property.
Concurrent publishers could reserve numbers and send them in a different order.

Use one short synchronous critical section for these operations:

1. Assign the next publication number with checked arithmetic.
2. Append the shared event when startup remains open.
3. Send that same shared event through the broadcast channel.

Perform `seal_startup()` under the same synchronization.
After sealing, no later publication may enter the startup prefix.
Serialization and socket writes remain outside the critical section.
Never await network progress while holding the publication lock.

Do not silently wrap sequence numbers or continue after an inconsistent publication state.
Use an explicit event-subsystem error path and record the failure without exposing payload contents.
Preserve the existing success behavior when no live receivers exist.
Startup events remain available for later replay.

The existing post-startup path skips the startup mutex.
Serializing publication through a lock therefore adds a contention risk.
Measure that cost before accepting this protocol.
Any later fast path needs an equivalent ordering argument and concurrency tests.

## Subscription and Handoff

The proposed connection sequence is:

1. Subscribe to the live channel before taking the startup snapshot.
2. Snapshot startup references and the last included sequence under the publication lock.
3. Send the existing handshake.
4. Replay the snapshot in sequence order.
5. Release the snapshot references when replay completes.
6. Skip live events whose sequence is at or below the snapshot boundary.
7. Send later live events through the existing serializer.

For an empty snapshot, use an explicit absent boundary.
The boundary is the last startup event in this snapshot, not the latest event in the live channel.
Otherwise, a late subscriber could discard live events that were never replayed.

When subscription occurs during startup, replay and live delivery can overlap.
The boundary removes only this overlap.
A later identical payload has a newer sequence and remains deliverable.
Startup publications after the snapshot also remain deliverable through the live channel.

If replay serialization fails, terminate the connection rather than claiming that the unsent event was replayed.
Keep this failure behavior explicit in compatibility review.
Do not leave a boundary that silently suppresses an undelivered event.

Keep broadcast lag markers and their skipped-event counts.
A slow subscriber can still lose live events while replay takes place.
This proposal does not promise lossless delivery beyond the existing channel retention.
Do not add an unbounded catch-up queue to hide lag.

## Bounds and Lifetime

Let `C` be the number of connected clients.
Current deduplication strings can retain approximately `C * S_json`, plus string and hash-set overhead.
Here, `S_json` is the retained startup JSON text per client, not the typed payload size.

The proposal retains shared startup payloads once, plus per-client sequence metadata after replay.
During replay, each client still holds a snapshot of shared references and its current output buffer.
Measure those transient costs and the shared allocation headers.
Disconnected clients must release all snapshot and broadcast references.

A channel capacity limits event count, not event bytes.
This proposal does not introduce a startup eviction policy or change the live channel capacity.
A future startup byte limit needs an explicit incomplete-replay signal and client recovery behavior.
A truncated startup prefix cannot use this scalar-boundary protocol without another correctness argument.

## Validation

| Test group | Required evidence |
| --- | --- |
| Late subscription | All buffered startup events replay without retaining their JSON strings afterward |
| Startup overlap | A publication between subscription and snapshot appears once, not zero or twice |
| Snapshot boundary | A publication after the snapshot remains visible in the live stream |
| Seal race | Concurrent publish and seal operations produce one complete, ordered startup prefix |
| Concurrent publishers | Sequence order matches channel publication order |
| Identical payloads | Distinct publications with equal contents both remain visible |
| Empty and failed replay | Empty buffers work, and a failed send or serialization cannot suppress unsent work silently |
| Lag and reconnect | Existing lag markers survive, reconnect replays startup, and live delivery resumes |
| Ownership | Dropping clients releases their references without invalidating other subscribers |
| Sequence limits | Near-overflow fixtures never reuse a sequence within an active stream |
| Client integration | Queue resynchronization and [runtime event consumers](../../node/src/rust/runtime/setup.rs) retain their behavior |

Use deterministic synchronization barriers around publish, snapshot, and seal operations.
Sleep-based race tests alone are insufficient.
Do not claim general ordering correctness from a small stress test.

## Benchmark and Delivery

Compare one client with multiple slow and fast clients using the same event corpus.
Measure memory before subscription, during replay, after replay, and after disconnect.
Record unique payload bytes, retained references, allocation counts, publication lock time, and lag frequency.
Include large verdict and transfer events, not only small startup notifications.

1. Capture existing event output and add publication and handoff regression cases.
2. Introduce shared payload ownership without changing public serialization.
3. Add ordered publication identifiers and scalar replay bookkeeping.
4. Validate lag handling and queue recovery on a disposable shard.
5. Accept the change only after retained memory improves without unacceptable publication contention.

Rollback requires no data migration or peer upgrade coordination.
Existing WebSocket connections close during a binary restart and reconnect through the existing protocol.
