# Runtime Contract

## Summary

This note documents the first implemented `cordial-app-runtime` processing
contract.

The runtime now owns an in-memory deterministic bridge from finalized app
events to application receipts and snapshots:

```text
caller-provided finalized AppEvent stream
  -> AppRuntime
  -> CordialApp validate/apply
  -> AppReceipt storage + AppCursor advancement + AppSnapshot access
```

The caller is still responsible for providing events in finalized Cordial order.
The runtime preserves that order exactly; it does not sort, reorder, fetch, or
derive consensus output.

## Implemented Scope

The implementation lives in:

```text
crates/cordial-app-runtime/src/runtime.rs
```

It adds:

- event processing through `AppRuntime::process_event`
- batch processing through `AppRuntime::process_events`
- routing by `AppId`
- duplicate protection by `AppEventId`
- applied and rejected receipt recording
- runtime cursor advancement
- receipt lookup and ordered receipt iteration
- per-app snapshot lookup
- stable snapshot listing across registered apps

Behavior tests live in:

```text
crates/cordial-app-runtime/tests/test_runtime_processing.rs
```

## Runtime-Owned State

`AppRuntime` now owns these in-memory structures:

| Field | Role |
|------|------|
| `apps` | Registered deterministic application state machines, keyed by `AppId` |
| `processed_event_ids` | Duplicate protection for event replay safety |
| `receipts` | Stored receipts keyed by `AppEventId` |
| `receipt_order` | Caller-provided finalized processing order for receipt iteration |
| `cursor` | Last processed finalized event position |

The state is intentionally in-memory for this slice. Durable persistence,
restart recovery, and snapshot loading are follow-up work.

## Processing Rules

For each `AppEvent`, `AppRuntime::process_event` follows this order:

1. Check whether the `event_id` was already processed.
2. Find the registered app for `app_id`.
3. Call `CordialApp::validate`.
4. If validation succeeds, call `CordialApp::apply`.
5. If validation rejects, create a rejected receipt and do not call `apply`.
6. Record the receipt for applied or rejected app decisions.
7. Advance the runtime cursor to the event's `finalized_anchor` and
   `ordered_index`.

The runtime never changes the caller-provided order. If callers pass events in
tau order, receipts iterate in tau order. If callers pass a different order,
the runtime preserves that exact order.

## Outcome Semantics

| Case | Runtime result | Receipt stored? | Cursor advances? |
|------|----------------|-----------------|------------------|
| Registered app accepts event | `Ok(Applied receipt)` | Yes | Yes |
| Registered app validation rejects event | `Ok(Rejected receipt)` | Yes | Yes |
| Unknown `app_id` | `Err(AppError::UnknownApp)` | No | No |
| Duplicate `event_id` | `Err(AppError::DuplicateEvent)` | No | No |
| App `apply` fails | `Err(AppError::ApplyFailed)` | No | No |

The key app-layer rule is that validation failure is not a consensus failure.
The event remains part of finalized history, and the runtime records a rejected
receipt for auditability.

Apply failure is different: it means the app could not deterministically produce
its state transition after validation. The runtime surfaces the error and does
not record a partial receipt or cursor movement for that event.

## Cursor Semantics

The runtime cursor is:

```rust
pub struct AppCursor {
    pub finalized_anchor: Vec<u8>,
    pub ordered_index: u64,
}
```

It records the last event that reached an app decision: either applied or
rejected. The cursor follows finalized history, not app acceptance.

That means a rejected app event still advances the cursor. This preserves the
append-only history rule:

```text
finalized event exists
  -> app rejects it
  -> rejection receipt is stored
  -> cursor advances past it
```

The current cursor is runtime-wide. Per-app durable cursors may be added when
persistence and restart semantics are designed.

## Receipt Semantics

Receipts are keyed by `AppEventId` and can be queried with:

```rust
AppRuntime::receipt(&event_id)
```

Receipts can also be iterated with:

```rust
AppRuntime::receipts()
```

Iteration follows `receipt_order`, which records the caller-provided finalized
event order. This avoids leaking map iteration order into app-visible audit
output.

For applied events, the runtime normalizes the receipt identity fields from the
event itself:

- `app_id`
- `event_id`
- `ordered_index`
- `status`

The app still provides app-specific receipt data:

- `state_root`
- `message`

For rejected events, the runtime creates the receipt and keeps the app snapshot
state root unchanged.

## Snapshot Semantics

Applications expose snapshots through `CordialApp::snapshot`.

The runtime exposes:

```rust
AppRuntime::snapshot(&app_id)
AppRuntime::snapshots()
```

`snapshot(&app_id)` returns one app snapshot or `UnknownApp`.
`snapshots()` returns snapshots for all registered apps in stable `AppId` order.

Snapshots are opaque to the generic runtime. App-specific payload decoding,
state roots, and snapshot formats belong to concrete app crates.

## Why Stable Collections

The runtime uses `BTreeMap` and `BTreeSet` instead of `HashMap` and `HashSet`
where deterministic ordering can matter.

This matters because app runtime replay must be reproducible. Given the same
ordered event prefix, every node should reproduce the same receipts and
snapshots.

`BTreeMap` and `BTreeSet` provide:

- stable key order during iteration
- deterministic behavior across runs and machines
- easier replay tests and debugging
- no dependency on randomized hash iteration order

`HashMap` and `HashSet` are often faster for raw lookup, but Rust's standard
hash collections intentionally do not provide stable iteration order. That is a
poor fit for a replay boundary where ordering might leak into receipts,
snapshots, logs, or tests.

The runtime also keeps an explicit `receipt_order` vector because receipt
iteration must follow finalized event order, not sorted event ID order.

## Multi-App Isolation

The same event stream can contain events for many applications. The runtime
routes each event by `app_id` and only invokes the matching app.

Batch processing returns one result per input event:

```rust
AppRuntime::process_events(events)
```

This lets callers observe a failure for one app while continuing to process
later events for unrelated apps. A failed event does not poison the whole
runtime.

## Replay Contract

For the same registered apps and the same ordered event prefix:

```text
same AppEvent sequence
  -> same process_event/process_events results
  -> same stored receipts
  -> same cursor
  -> same snapshots
```

The tests prove this for in-memory replay. Durable replay from persisted
history and snapshots is a later persistence-layer issue.

## Out Of Scope

This runtime slice does not implement:

- extraction from `OrderedFinalizedOutput`
- direct `f1r3node` or gRPC integration
- blocklace, finality, tau ordering, or PoR logic
- durable persistence
- app-specific payload decoding
- concrete app behavior such as marketplace, payment ledger, or social feed
  semantics

Those remain separate layers so `cordial-app-runtime` stays app-neutral.

## Verification

The implemented slice is covered by:

```text
cargo fmt -p cordial-app-runtime -- --check
cargo test -p cordial-app-runtime
cargo clippy -p cordial-app-runtime --all-targets --all-features --no-deps -- -D warnings
```
