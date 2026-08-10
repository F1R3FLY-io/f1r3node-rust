# Rholang Token Cost Model

**Status:** implementation-aligned with D3 / DR-9 / OD-1 / OD-3

**Consensus unit:** one token for each committed `BillableKind::Comm` event

**Non-consensus telemetry:** per-operation weights and the cost-trace digest

This guide describes the token model implemented by the current node. The former
`phlo_limit × phlo_price` escrow, precharge, refund, and per-deploy exhaustion model has been
removed. Historical design records that discuss those mechanisms predate D3 and are not a
description of the current wire or runtime.

## 1. Terms

| Term | Meaning |
|---|---|
| **COMM event** | A token-consuming send or receive reduction. The reducer records it as `BillableKind::Comm`. |
| **Consensus cost** | The number of committed COMM events. Every COMM contributes exactly one, independent of its diagnostic weight. |
| **Demand** | The static per-signature COMM count, written `Delta_s` in source as `DemandEntry::known_lower_bound`. |
| **Supply** | The renewable token balance for signature `s`, stored on the unnameable supply channel produced by `SignatureChannel::from_sig(s)`. |
| **Diagnostic event** | A `Reduction`, `Primitive`, or `Substitution` event. It records useful work information but contributes zero to consensus cost. |
| **Accepted-byte envelope** | The configured maximum decoded protobuf size. It bounds network work performed before a deploy has a metered Rholang state. |

The mathematical consensus quantity for deploy `d` is:

```math
\operatorname{cost}(d)
=
\left|\left\{e \in \operatorname{committed}(d) \mid
\operatorname{kind}(e)=\mathtt{Comm}\right\}\right|.
```

`RuntimeBudget::reconcile_lane` implements this equation: a committed `Comm` adds one, while
`Reduction`, `Primitive`, and `Substitution` add zero. The event's `weight` remains diagnostic and
does not alter the count.

## 2. End-to-end accounting flow

| Stage | Input | Decision or result | Consensus role |
|---|---|---|---|
| Network admission | Encoded gRPC message | Reject when the declared protobuf length exceeds the configured decoder limit | Bounds pre-decode work; does not charge tokens |
| Normalization | Accepted Rholang source | Produce a normalized, stack-safe `Par`, or reject a parse error | No token charge; work is bounded by accepted bytes |
| Funding analysis | Normalized `Par` and deploy signature | Compute per-signature COMM demand and compare it with live supply | Determines deploy admission |
| Evaluation | Admitted deploy | Record COMM and diagnostic events; execute without a per-deploy OOP cap | Produces the actual COMM count |
| Replay | Block deploys and pre-state | Recompute admission, execution, and settlement | Rejects cost, admission, status, or post-state disagreement |
| Settlement | Admitted per-signature demand | Debit supply once at block close | Conserves the funded token supply |

### 2.1 Static funding gate

`rholang::interpreter::accounting::delta_sigma::demand` is a pure, iterative structural pass over
the fully normalized `Par`. It counts exactly the send and receive nodes that cause the reducer to
emit `BillableKind::Comm`. `New`, `Match`, `If`, primitive calls, and substitutions are traversed as
needed but do not add to consensus demand.

Under the current `s0` collapse, all counted COMMs are attributed to the deploy envelope's
signature. Compound signatures use the same Split/Join supply algebra and live cross-group residual
ledger on play and replay. The gate admits only a canonically ordered funded prefix; replay
recomputes that decision independently.

### 2.2 Runtime and settlement

An admitted user deploy runs **unmetered for liveness**: it has no former `phlo_limit` exhaustion
boundary. This is safe because the block-assembly gate has already established fundedness, and the
runtime continues to record every COMM required to compute `ProcessedDeploy.cost`.

Settlement is not a second charge. It is the single state transition that realizes the admitted
demand against the signature supply pool. Per-operation diagnostic weights neither mint supply nor
increase the debit.

## 3. Pre-COMM resource envelope

Protobuf decoding and source parsing occur before a Rholang COMM event exists, so billing them as
COMMs would change the approved calculus. They are bounded by input size instead:

| Ingress | Default bound | Enforcement point |
|---|---:|---|
| External and internal API gRPC | 16 MiB | Each generated Deploy, Propose, REPL, LSP, and reflection service calls `max_decoding_message_size` |
| Peer protocol unary gRPC | 256 KiB | The generated `TransportLayerServer` decoder is bounded before the TLS request interceptor |
| Peer streamed payload | 256 MiB reconstructed message | Chunking plus the stream circuit's `max_stream_message_size` check |

Tonic checks the gRPC message's declared length before reserving its protobuf body buffer or calling
prost. An oversized unary message returns gRPC `OutOfRange`, and the service handler is not invoked.
The black-box gates are:

- `node/tests/grpc_message_size_spec.rs` for public API deploy ingress;
- `comm/tests/transport/message_size_spec.rs` for peer transport ingress.

The decoder, normalizer, and generated term traversals are pushdown-automaton or worklist driven, so
nesting depth does not consume native call stack and no recursion-depth acceptance limit is needed.
The byte envelope bounds the amount of unauthenticated representation admitted to those stack-safe
machines. It is a size policy, not a hidden shape policy: equally sized shallow and deep terms face
the same network threshold.

## 4. Diagnostic operation weights

The functions in `rholang/src/rust/interpreter/accounting/costs.rs` still estimate physical work for
telemetry, profiling, and regression analysis. Examples include operand-size-sensitive BigInt and
BigRat arithmetic, encoded-length equality checks, collection operations, string conversion, and
substitution. These values are recorded on diagnostic events but have the following invariant:

```text
diagnostic weight changes  =>  consensus COMM count unchanged
```

This separation permits better performance models without silently changing deploy funding or
forking replay. Any future proposal to promote a diagnostic weight into consensus accounting is a
new protocol decision and must be entered in the consensus change register.

## 5. Worked examples

### 5.1 One send

```rholang
@0!(1)
```

The normalized term contains one token-consuming send, so its static demand and completed runtime
consensus cost are both one. Encoding, normalization, and the payload's integer size do not add
COMM tokens.

### 5.2 Arithmetic without communication

```rholang
new return in { return!(1 + 2 * 3) }
```

The arithmetic produces diagnostic primitive weights. The send on `return` contributes the one
consensus COMM token; the multiplication and addition contribute zero consensus tokens.

### 5.3 Deep but byte-bounded input

A deeply nested term is accepted or rejected according to syntax, signature, funding, and the same
message-byte limit as a shallow term. Traversal depth itself is not an admission criterion. The
iterative decoder and normalizer use heap worklists rather than native recursion.

## 6. Failure semantics

| Failure | Result |
|---|---|
| Protobuf exceeds ingress bound | Reject before decoding or handler dispatch with gRPC `OutOfRange` |
| Source does not parse or normalize | Reject the deploy; no COMM tokens are consumed |
| Static demand is not funded | Reject at block assembly; replay recomputes the same decision |
| User evaluation fails | Roll back that deploy's tuple-space effects while retaining deterministic status and cost evidence |
| Replay observes different cost, status, admission, or post-state | Reject the block |

`OutOfPhlogistonsError` and finite `RuntimeBudget` construction remain useful for isolated internal
tests and diagnostic machinery, but they are not the live accepted-user-deploy policy after OD-1.

## 7. Source-of-truth map

| Concern | Authoritative source |
|---|---|
| Approved model | `docs/theory/cost-accounting-impl/d3-replace-phlo-with-tokens.md` |
| Static demand and supply algebra | `rholang/src/rust/interpreter/accounting/delta_sigma.rs` |
| Runtime reconciliation | `rholang/src/rust/interpreter/accounting/mod.rs` |
| COMM emission | `rholang/src/rust/interpreter/reduce.rs` and `metering.rs` |
| Accepted deploy execution | `casper/src/rust/rholang/runtime.rs` |
| API byte bound | `node/src/rust/api/grpc_package.rs` |
| Peer byte bound | `comm/src/rust/transport/grpc_transport_receiver.rs` |
| Operator defaults | `node/src/main/resources/defaults.conf` |
| Consensus classification | `docs/consensus/consensus-change-register.md` |
| Resource analysis | `docs/theory/cost-accounting-threat-model.md` |
