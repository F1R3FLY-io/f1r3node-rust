# Typed JSON Boundaries Without Intermediate Trees

Status: draft proposal. Allocation savings and latency effects are not yet measured.
Date: 2026-11-25.

## Objective

Remove intermediate JSON object trees from trace responses, WebSocket events, and deploy-queue event parsing.
Keep JSON as the external format.
Reduce temporary allocations without changing consensus data or retained chain state.

The [memory evaluation contract](README.md#memory-evaluation-contract) applies.
This proposal targets request-time peak memory and allocation traffic, not the main retained interpreter heap.
It does not replace JSON with a binary client protocol.

## Current Paths

| Path | Current conversion | Avoidable work |
| --- | --- | --- |
| [Trace response](../../node/src/rust/web/reporting_routes.rs) | `BlockEventInfoSerde -> Value -> response bytes` | A second object tree contains copied fields, strings, and map entries |
| [WebSocket event](../../node/src/rust/web/events_info.rs) | `F1r3flyEvent -> Value -> envelope Value -> String` | Each subscriber builds an object tree before sending text |
| [Queue event](../../deploy-queue/src/events.rs) | `text -> Envelope<Value> -> typed payload -> NodeEvent` | The parser materializes fields that the queue does not use |

The trace handler already has a [typed serializable report](../../node/src/rust/api/serde_types/block_event_info.rs).
Its `to_value` call borrows that report, so conversion can hold both representations.
The final response buffer is a separate allocation.

The queue parses a complete `BlockPayload` for `block-finalised` but retains only `block_number`.
Unknown event types also pass through a generic payload tree before the queue ignores them.
The queue runs separately from the node, so report its savings separately.

## Proposed Design

### Trace Responses

Change the report variant to contain `BlockEventInfoSerde`, not `serde_json::Value`.
Pass that typed value directly to the existing response serializer.
Keep the `type` discriminator, field names, optional fields, byte encodings, and response status mapping.
Keep the documented OpenAPI response contract compatible.

Do not add response streaming in the first change.
Streaming changes error timing, buffering, and cancellation behavior.
The existing complete response buffer remains part of peak memory.

The current conversion replaces a serialization failure with `null`.
The implementation needs an explicit decision for that failure path.
Prefer the existing structured error response before response headers are sent, rather than a successful response with missing report data.
Test this behavior separately from successful schema equivalence.

The [report API](../../casper/src/rust/api/block_report_api.rs) owns replay admission and cached-report behavior.
Do not change its semaphore, replay limits, cache retention, or historical-state handling.
Its replay permit does not establish a memory bound for all later response serialization.

### WebSocket Events

Add a borrowed serializer view over `F1r3flyEvent`.
The view writes `event`, `schema-version`, and `payload` directly.
Match each event variant to its existing event name and typed payload.
Do not serialize the internally tagged enum inside `payload`, which would duplicate the event discriminator.

Keep the shared event type unchanged in this first proposal.
The view belongs at the WebSocket boundary, not in the consensus publisher.
Keep the handshake and lag-marker schemas unchanged.

Serialize directly into the text buffer consumed by the socket.
Do not introduce an unbounded serialized-event cache.
Shared event ownership and startup deduplication belong to the [event replay proposal](memory-event-replay.md).

### Queue Event Parsing

Use an envelope with a borrowed `RawValue` payload as the first prototype.
Enable the existing `serde_json` dependency's `raw_value` feature only where required.
Read the discriminator, then deserialize the selected typed payload from its original byte slice.
No borrowed field may escape the input frame into the asynchronous command queue.

This approach scans JSON syntax before typed parsing, but avoids a complete generic object tree.
It does not assume that the discriminator precedes the payload in the input.
An adjacent-tagged enum is an alternative only if allocation measurements confirm that its parser avoids equivalent buffering.

For the initial implementation, preserve the fields and types required by the current parser.
For `block-finalised`, a custom visitor can validate unused fields without retaining their strings or deploy list.
Retain required-field checks, numeric range checks, nested deploy validation, and the accepted unknown-field behavior.
Do not weaken validation by replacing the payload immediately with a height-only structure.

Preserve `Running`, `Ignored`, verdicts, and lag events.
Missing or null payloads must follow the current event-specific behavior.
Compare duplicate-key handling explicitly because a generic JSON tree can collapse duplicates before typed decoding.
Any intentional rejection change needs separate review and a named regression test.
Preserve input-size, nesting, and numeric limits, including limits on payloads that the queue ignores.
Raw payload capture is not a substitute for those validation checks.

## Memory Model

Let `T` be the typed report's owned memory, `J` its generic JSON tree, and `W` its response bytes.
The existing path can overlap these allocations.
The proposed path removes `J`, but retains `T`, `W`, and serializer scratch memory.
Do not infer `J` from the wire length alone.
Map nodes, allocated keys, string capacities, and numeric values have different costs.

WebSocket savings depend on payload size, event rate, and subscriber count.
Queue savings depend on ignored payload fields and concurrent frame processing.
Freed temporary allocations might not immediately reduce resident memory because the allocator can retain pages.

## Validation

| Test group | Required evidence |
| --- | --- |
| Trace schema | Old and new serializers produce equal parsed JSON for empty and populated reports |
| Trace details | Byte encodings, optional values, large amounts, nested events, and array order remain exact |
| Trace failures | Existing API errors retain their status mapping, and serialization failure cannot become silent success |
| Event coverage | Every event variant retains its name, payload, schema version, and omission rules |
| Stream behavior | Handshake, lag markers, disconnects, and socket errors retain their behavior |
| Queue equivalence | Both parsers produce the same `NodeEvent` for all accepted fixtures |
| Parser failures | Malformed JSON, missing fields, nulls, duplicate keys, numeric overflow, and invalid nested records have reviewed outcomes |
| Resource limits | Deep nesting, oversized frames, and ignored payloads cannot bypass existing limits |
| Compatibility | Payload-before-discriminator input, unknown event types, and unknown fields remain supported as specified |
| Lifetimes | No borrowed input outlives its frame, and cancellation releases temporary buffers |

JSON object-key order is not a semantic equivalence assertion.
Preserve exact bytes where a consumer or existing test requires byte-level stability.
Keep the old implementation as a test oracle during the transition, not as a second production serialization pass.

## Benchmark and Delivery

Benchmark trace responses, WebSocket serialization, and queue parsing independently.
Use small, typical, and large fixtures, including ignored events with large valid payloads.
Measure allocation count, total allocated bytes, peak live bytes, throughput, and tail latency.
Vary subscriber count and concurrent report requests without increasing existing admission limits.

1. Capture existing output and parser behavior in fixtures.
2. Implement typed trace responses and the borrowed event serializer separately.
3. Prototype raw-payload queue parsing with equivalent validation.
4. Compare each change against the same baseline workload.
5. Run client compatibility tests and disposable-shard finalization tests.

Accept a change only when allocation reductions exceed measurement noise and latency remains within the agreed budget.
Keep changes independent so a parser regression does not block a safe response-serialization improvement.
Rollback requires a binary change only, with no state migration.
