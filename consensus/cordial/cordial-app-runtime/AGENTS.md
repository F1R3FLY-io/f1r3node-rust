# Cordial App Runtime Agent Guide

This crate is the application interface layer above Cordial Miners finalized
ordered output. It is intentionally app-neutral: applications should depend on
this crate instead of depending directly on blocklace internals, tau ordering,
gRPC, or `f1r3node` adapter details.

## Read First

Before changing this crate, read:

- `crates/cordial-app-runtime/docs/index.md`
- `crates/cordial-app-runtime/docs/01-architecture.md`
- `crates/cordial-app-runtime/src/lib.rs`
- `crates/cordial-app-runtime/src/runtime.rs`
- `docs/cordial-miners/integration/19-application-interface-layer.md`

## Core Design

The runtime boundary is:

```text
finalized ordered Cordial output
  -> deterministic AppEvent stream
  -> app-specific state machines
  -> AppReceipt + AppSnapshot
```

The crate should answer:

```text
Given finalized ordered events, how does an application deterministically apply
them and expose receipts/snapshots?
```

It should not answer:

```text
Which blocks are finalized?
What is the tau order?
How does f1r3node store or execute Rholang?
How are PoR weights calculated?
```

## Ownership Boundary

`cordial-app-runtime` owns:

- app-neutral event vocabulary: `AppId`, `AppEventId`, `AppEvent`
- replay/cursor vocabulary: `AppCursor`, `AppSnapshot`
- app-layer output vocabulary: `AppReceipt`, `AppReceiptStatus`
- app runtime errors: `AppError`
- the `CordialApp` deterministic state-machine trait
- the generic `AppRuntime` registry and event-processing behavior

`cordial-app-runtime` does not own:

- blocklace data structures
- approval, ratification, finality, or tau-ordering logic
- live deploy ingestion
- `f1r3node` networking, storage, or RSpace/Rholang runtime integration
- PoR reputation math or weight calculation
- concrete applications such as an AI marketplace, payment ledger, or social
  feed

## Current Implementation State

The crate currently has the vocabulary and registry scaffold:

- `event.rs`: app IDs, event IDs, finalized event envelope, cursor, snapshot
- `receipt.rs`: app receipts and applied/rejected status
- `error.rs`: app-runtime error variants
- `runtime.rs`: `CordialApp` trait and an in-memory `AppRuntime` registry
- `lib.rs`: public re-exports

`AppRuntime` currently registers apps only. Event processing, duplicate
protection, cursor advancement, receipt storage, and replay tests are the next
implementation slice.

## Next Implementation Slice

Keep the first functional step small and deterministic:

1. Add in-memory event processing to `AppRuntime`.
2. Route each `AppEvent` by `app_id`.
3. Reject duplicate `event_id` values deterministically.
4. Apply events in caller-provided finalized order.
5. Emit an `AppReceipt` for applied and rejected app events.
6. Advance an `AppCursor` only according to the finalized ordered stream.
7. Expose read-only access to stored receipts and snapshots.
8. Add replay tests proving the same ordered event prefix produces the same
   receipts and snapshots.

Prefer APIs that make invalid states explicit with `Result<_, AppError>` rather
than panics or silent overwrites.

## Runtime Semantics

Important behavior rules:

- Consensus validity and app validity are separate.
- Invalid app events remain part of finalized Cordial history.
- An app-level validation failure should produce a rejected receipt, not remove
  or reorder the event.
- A failure in one app should not block unrelated apps from processing their own
  events.
- App-specific payload decoding belongs in concrete app crates, not in this
  generic runtime.
- Given the same finalized ordered event sequence, every node must be able to
  reproduce the same app receipts and snapshots.

## Determinism Rules

- Preserve finalized order exactly; do not sort finalized input events unless an
  API explicitly receives unordered input and documents how it canonicalizes it.
- Use stable maps such as `BTreeMap`/`BTreeSet` when iteration order can leak
  into receipts, snapshots, or tests.
- Treat `ordered_index` and `event_id` as part of replay safety.
- Do not use wall-clock time, randomness, networking, background tasks, or
  process-local state in deterministic event application.
- Keep payloads opaque bytes in the generic runtime.

## Testing Guidance

Prefer external behavior tests under `crates/cordial-app-runtime/tests/` for the
runtime contract. Keep unit tests in `src/` only when they are narrow helper
tests.

Useful test cases:

- registering one app and multiple apps
- unknown `app_id`
- duplicate `event_id`
- valid event produces an applied receipt
- app validation failure produces a rejected receipt
- app apply failure is surfaced deterministically
- cursor advances in finalized order
- replaying the same prefix produces identical receipts/snapshots
- unrelated app failure does not block another app

Run at least:

```text
cargo fmt -p cordial-app-runtime -- --check
cargo test -p cordial-app-runtime
cargo clippy -p cordial-app-runtime --all-targets -- -D warnings
```

## Relationship To PoR

PoR and the app runtime are parallel layers.

```text
uniform/bond mode -> OrderedFinalizedOutput -> cordial-app-runtime
PoR-weighted mode -> OrderedFinalizedOutput -> cordial-app-runtime
```

The app runtime should consume finalized ordered output regardless of whether
Cordial Miners used uniform weights, bond weights, or PoR-derived weights.
Reputation updates may later be represented as app events or as a
protocol-adjacent application, but they must never retroactively rewrite an
already-finalized app event order.

## Keep Out For Now

Do not add these to the generic runtime unless a specific issue asks for them:

- AI marketplace business rules
- payment ledger semantics
- social feed semantics
- Rholang execution
- LMDB/RSpace persistence
- HTTP/gRPC APIs
- PoR committee or leader-selection logic
- Cordial Miners consensus changes

