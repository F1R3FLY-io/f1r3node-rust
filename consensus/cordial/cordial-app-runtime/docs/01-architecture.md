# Application Interface Layer

## Summary

The application interface layer is the boundary that lets real applications
consume Cordial Miners output without knowing blocklace, finality, tau, gRPC, or
`f1r3node` internals.

Its purpose is simple:

```text
finalized ordered Cordial output -> deterministic app events -> app state
```

This keeps the consensus protocol generic while giving applications a stable
place to run on top of it.

## Why This Layer Is Needed

Cordial Miners already produces finalized ordered block output. That is enough
for consensus, but not enough for application developers.

Applications need a clearer contract:

- what event was finalized
- which application the event belongs to
- who submitted it
- where it appeared in the finalized order
- whether the app accepted or rejected it
- how to replay the same history into the same app state

Without this layer, every application would need to understand adapter output
directly. That would couple apps to the protocol internals too early.

## Architectural Position

```text
f1r3node deploys / external app events
        |
        v
Cordial deploy ingress
        |
        v
Cordial Miners blocklace
        |
        v
finality + tau ordering
        |
        v
OrderedFinalizedOutput
        |
        v
Application interface layer
        |
        v
app-specific state machines
```

The application interface layer does not decide consensus. It only consumes
already-finalized ordered output.

### Overall Architecture

```mermaid
flowchart TB
    external["External users / apps"] --> deploys["f1r3node deploys or app events"]
    deploys --> ingress["Cordial deploy ingress"]
    ingress --> mirror["Cordial f1r3node adapter"]
    mirror --> blocklace["Cordial Miners blocklace"]
    blocklace --> finality["Finality + tau ordering"]
    finality --> ordered["OrderedFinalizedOutput"]
    ordered --> extractor["App event extractor"]
    extractor --> router["App router by app_id"]
    router --> runtime["Cordial app runtime"]
    runtime --> receipts["App receipts"]
    runtime --> snapshots["App state snapshots"]

    router --> marketplace["AI marketplace app"]
    router --> ledger["Payment ledger app"]
    router --> social["Social feed app"]
    router --> reputation["PoR / reputation app"]

    subgraph consensus["Consensus Layer"]
        blocklace
        finality
        ordered
    end

    subgraph adapter["Integration Layer"]
        ingress
        mirror
    end

    subgraph app_layer["Application Layer"]
        extractor
        router
        runtime
        receipts
        snapshots
        marketplace
        ledger
        social
        reputation
    end
```

The important boundary is between `OrderedFinalizedOutput` and the app event
extractor. Everything before that is consensus or node integration. Everything
after that is application execution.

### Interaction Sequence

```mermaid
sequenceDiagram
    participant User
    participant Node as f1r3node / deploy API
    participant Ingress as Cordial deploy ingress
    participant Adapter as Cordial adapter mirror
    participant Core as Cordial Miners core
    participant Runtime as App runtime
    participant App as App state machine

    User->>Node: Submit deploy / app event
    Node->>Ingress: Observe deploy at ingress boundary
    Ingress->>Ingress: Record signature and app envelope
    Node->>Adapter: Block containing deploy becomes visible
    Adapter->>Core: Insert mirrored block into blocklace
    Core->>Core: Compute finality and tau order
    Core-->>Adapter: OrderedFinalizedOutput
    Adapter->>Runtime: Extract finalized AppEvent stream
    Runtime->>Runtime: Check cursor and ordered_index
    Runtime->>App: validate(event)
    alt app event valid
        App->>App: apply(event)
        App-->>Runtime: Applied receipt + new snapshot
    else app event invalid
        App-->>Runtime: Rejected receipt
    end
    Runtime-->>User: App state / receipt is queryable
```

This sequence shows the main rule: app validation happens after consensus
ordering. Invalid app events are rejected by the app runtime, not removed from
the finalized Cordial history.

## Proposed Crate Boundary

This layer should live in its own crate:

```text
crates/cordial-app-runtime/
```

The intended crate split is:

```text
cordial-miners-core
  Pure consensus: blocklace, approval, finality, tau, pruning, PoR weighting.

cordial-f1r3node-adapter
  Live node integration: deploy ingress, block mirroring, ordered output export.

cordial-app-runtime
  Generic app execution layer over finalized ordered output.

future app crates
  Example applications such as AI marketplace, payment ledger, social feed.
```

This prevents application logic from leaking into consensus or adapter code.

## Generic App Event

The runtime should convert finalized ordered output into generic app events.

Initial shape:

```rust
pub struct AppEvent {
    pub app_id: String,
    pub event_type: String,
    pub payload: Vec<u8>,
    pub submitter: Vec<u8>,
    pub ordered_index: u64,
    pub block_hash: Vec<u8>,
    pub deploy_signature: Option<Vec<u8>>,
    pub finalized_anchor: Vec<u8>,
}
```

The event envelope should be stable and app-neutral. Application-specific
payloads are decoded by the app, not by consensus.

## App Trait

Each application should implement a deterministic state machine.

Initial trait sketch:

```rust
pub trait CordialApp {
    fn app_id(&self) -> &str;

    fn validate(&self, event: &AppEvent) -> Result<(), AppError>;

    fn apply(&mut self, event: AppEvent) -> Result<AppReceipt, AppError>;

    fn snapshot(&self) -> AppSnapshot;
}
```

The important rule is that `apply` must be deterministic. Given the same
finalized event sequence, every node should produce the same app state.

## Runtime Responsibilities

The runtime should be responsible for:

- reading finalized ordered output
- extracting app events from ordered blocks or deploy payloads
- routing events by `app_id`
- applying events in finalized tau order
- storing a processed cursor
- producing app receipts
- exposing app snapshots
- replaying state from ordered history plus snapshots

The runtime should not:

- choose leaders
- decide finality
- mutate tau order
- hide invalid app events from history
- depend on a specific application such as the AI marketplace

## Processing Contract

The application runtime should provide the following guarantees to apps:

1. Events are delivered in finalized Cordial tau order.
2. Each event has a stable `ordered_index`.
3. Events are delivered at least once internally, but applied exactly once per
   app state cursor.
4. Invalid app events are recorded as app-layer rejections, not removed from
   consensus history.
5. App state can be replayed from the same ordered event prefix.
6. App state never rewrites a finalized prefix.

## Rejection Model

Consensus validity and app validity are separate.

A deploy may be valid for consensus but invalid for an application. For example,
an AI marketplace event may try to accept a task that does not exist.

In that case:

```text
consensus output: event remains finalized
app runtime: emits rejection receipt
app state: unchanged except for receipt / audit log
```

This preserves auditability and avoids rewriting finalized history.

## App Receipts

Every applied or rejected event should produce a receipt.

Initial shape:

```rust
pub struct AppReceipt {
    pub app_id: String,
    pub ordered_index: u64,
    pub event_id: String,
    pub status: AppReceiptStatus,
    pub state_root: Option<Vec<u8>>,
    pub message: Option<String>,
}

pub enum AppReceiptStatus {
    Applied,
    Rejected,
}
```

Receipts give beta applications a visible audit trail without requiring full
production indexing immediately.

## Cursor And Replay

The runtime needs a durable cursor:

```rust
pub struct AppCursor {
    pub finalized_anchor: Vec<u8>,
    pub ordered_index: u64,
}
```

This lets the runtime answer:

- which finalized prefix has been applied
- where replay should resume after restart
- whether a new ordered output is an append-only extension

This cursor should align with the existing `OrderedFinalizedOutput` prefix
contract.

## Multi-App Isolation

The same finalized ordered stream may contain events for many applications.

Events should be routed by `app_id`:

```text
ai.marketplace
payments.ledger
social.feed
por.reputation
identity.registry
```

Each app owns its own state, validation rules, and receipts. A failure in one
app must not block unrelated apps from processing their own events.

## AI Marketplace Fit

The AI marketplace should be treated as a first application built on top of the
generic runtime, not as the runtime itself.

Possible event types:

```text
TaskPosted
OfferSubmitted
ResultSubmitted
ResultAccepted
ProviderRated
DisputeOpened
DisputeResolved
```

This application is a strong fit because it naturally uses:

- deterministic ordering
- finalized commitments
- provider reputation
- audit trails
- multi-party workflows

## PoR Relationship

PoR weighting and the app runtime can be developed in parallel.

The app runtime consumes finalized ordered output regardless of how that output
was produced:

```text
uniform/bond mode -> OrderedFinalizedOutput -> app runtime
PoR mode          -> OrderedFinalizedOutput -> app runtime
```

Later, reputation updates may themselves be modeled as app events or as a
special protocol-adjacent application.

The key constraint is that reputation snapshots must not retroactively rewrite
already-finalized app event order.

## Open Design Questions

- Should app event envelopes be encoded directly inside deploy terms, deploy
  metadata, or a Cordial-specific payload wrapper?
- Should `AppEvent` use raw bytes only, or include typed JSON for early beta
  tooling?
- Should app receipts be part of consensus output, adapter output, or runtime
  persistence only?
- What storage backend should the first app runtime use: in-memory, LMDB, or
  f1r3node/RSpace-backed storage?
- Should PoR reputation updates be a normal app or a privileged protocol app?

## First Implementation Slice

The first implementation should stay small:

1. Add `crates/cordial-app-runtime`.
2. Define `AppEvent`, `AppReceipt`, `AppCursor`, `CordialApp`, and `AppRuntime`.
3. Add an in-memory runtime that applies events in order.
4. Add tests for deterministic replay, duplicate cursor protection, and
   app-layer rejection receipts.
5. Add a tiny example app, such as an in-memory task board.

Only after that should we build the AI marketplace-specific crate.

The in-memory runtime-processing slice is now implemented and documented in
[02-runtime-contract.md](./02-runtime-contract.md).
