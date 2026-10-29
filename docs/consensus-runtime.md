# Consensus runtime

This change implements the lifecycle boundary for issue #624. CBC Casper remains the default protocol. Cordial integration belongs to issue #625.

## Repository layout

```text
casper/                 Existing default consensus engine
consensus/
  api/                  Shared consensus interface
  runtime/              Shared lifecycle and supervision
node/                   Process host and Casper application adapter
```

The existing `casper/` crate stays at the repository root. The shared crates retain the package names `consensus-api` and `consensus-runtime`.
Other protocol crates will sit directly under `consensus/`, such as `consensus/cordial-miners/`. There is no intermediate `protocols/` directory.
The Cordial crate is outside the current #624 change and will be added through #625.

## Ownership

| Module | Responsibility |
| --- | --- |
| `consensus/api` | Protocol descriptors, capabilities, opaque requests, status, and adapter contract |
| `consensus/runtime` | Bounded admission, deadlines, readiness, task supervision, and shutdown |
| `node/src/rust/runtime` | Process configuration, transport, server listeners, and process shutdown |
| `node/src/rust/consensus/factory.rs` | Selection of the compiled protocol factory |
| `node/src/rust/consensus/ingress.rs` | Conversion of transport packets into opaque runtime requests |
| `node/src/rust/consensus/manifest.rs` | Serialized protocol and chain identity |
| `node/src/rust/consensus/casper` | Native startup, recovery, storage checks, workers, and application APIs |

The host receives a boxed `ApplicationProvider`. The provider creates public HTTP, administrative HTTP, external gRPC, and internal gRPC routes.
The host binds these routes to its listeners. It does not inspect a protocol enum or receive an `EngineCell`.

Casper owns its existing deploy, proposal, block-query, and reporting APIs. Deploy and proposal writes enter the bounded consensus handle.
Native query services remain inside the Casper adapter. Streaming queries belong to their response streams, so request cancellation releases the query future.

The packet bridge preserves the peer identity, packet kind, and payload bytes. Only the selected adapter decodes the native payload.

The manifest schema contains protocol, version, network, shard, and genesis identity. Casper validates existing LMDB stores before opening writable resources.
Casper records the manifest before it reports readiness.

Casper validates the configured validator key before it derives capabilities, binds the observer, or opens stores.
Invalid hexadecimal, incorrect key lengths, and invalid secp256k1 scalars return a signing error without key material.
An absent key preserves read-only operation.

Manifest publication does not replace an existing manifest. Concurrent publishers must agree on the full chain identity.
A legacy genesis block can establish genesis identity when the native genesis entry is absent.
A later checkpoint requires the native genesis entry. A checkpoint hash cannot replace a genesis hash.
The adapter rejects an unidentified checkpoint without changing the store.
This change does not provide an automatic migration for that case. Retain the store and obtain a verified recovery source before retrying startup.

The Casper adapter also owns the optional soak observer. The observer binds during preparation and stops before store shutdown.
Dropping a prepared adapter releases its observer socket. The host does not receive the native observer controller.

## Request memory admission

The runtime bounds both request count and retained request bytes. Default limits are:

| Traffic | Queued requests | Queued and active bytes |
| --- | --- | --- |
| Commands | 128 | 64 MiB |
| Packets | 256 | 256 MiB |

`RuntimeConfig.command_byte_capacity` and `packet_byte_capacity` set independent budgets. Command traffic cannot consume the packet budget.
The shared runtime defaults to a 16 MiB per-payload limit. Node assembly derives its limit from configured receive limits, currently 256 MiB by default.
Byte budgets are independent of those receive ceilings. Each budget must be positive and fit the semaphore permit range.
A higher receive ceiling does not prevent startup. Requests whose retained buffers exceed their byte budget receive `QueueFull`, even when their payload length passes validation.

Admission charges buffer capacity, not only payload length. Packet charges also include peer identifier, host, and packet-kind buffer capacities.
Admission does not wait for permits. Exhausting a count or byte limit returns `QueueFull` through the existing overload mapping.
Commands without payloads still consume queue entries but need no payload-byte permits.

`AdapterContext` receives `AdmittedRequest<ConsensusCommand>` and `AdmittedRequest<PacketRequest>` envelopes.
An adapter calls `into_parts()` inside its processing task and retains the returned permit until that task releases its request resources.
The permit must survive any scheduling wait and native processing. A caller timeout alone must not release the permit.
Completion, rejected dispatch, enqueue failure, panic, cancellation, and queue destruction release their owned permits.

These budgets bound admitted request buffers. They do not bound total process memory, transport buffers, native decoding allocations, or response buffers.
They do not replace deploy-pool admission or change deploy-pool ownership.

## Command deadlines

Each `Submit`, `Propose`, and `Finalized` command carries a monotonic deadline from `tokio::time::Instant`.
The runtime sets this deadline before queue admission and uses the same deadline to wait for the reply.
Time in the command queue and time waiting for an adapter task both count toward the request timeout.

Every adapter must call `ConsensusCommand::try_start()` immediately before command processing, after any scheduling or concurrency wait.
The check rejects an expired command with `DeadlineExceeded`, even if the caller has not yet observed its timeout.
It also skips commands whose callers have dropped their reply receivers. Rejection affects only that request and does not stop the adapter.
Casper performs this check inside its command task, before dispatch to the native operation.

The check defines the execution start boundary. After that boundary, the operation can finish after the deadline, including after waits inside native processing.
A caller timeout does not cancel or roll back an operation that has started.
For `Submit` and `Propose`, `DeadlineExceeded` therefore means **outcome unknown**, not proof that no state changed.
Check the operation result before retrying a write. The shared interface does not provide request deduplication or guarantee safe write retries.
`Finalized` is a read and can be retried, but a later response can report newer finality.

## Packet deadlines

`PacketRequest` carries the same monotonic request deadline used for the caller response timeout.
Every adapter calls `PacketRequest::try_start()` after scheduling waits and immediately before native processing.
The check skips expired packets and packets whose reply receiver has closed. Skipping a request does not stop the adapter.

A packet operation that already started can finish after its caller times out. The runtime does not provide packet deduplication or automatic retries.

## Shutdown and cleanup

The supervisor owns two separate timeout budgets. `drain_timeout` allows active work to finish. `cleanup_timeout` allows resource cleanup after execution stops.
Both default to 30 seconds. A requested shutdown can therefore use up to 60 seconds across these phases.
These are cooperative asynchronous timeouts. They cannot interrupt blocking code that does not yield.

An adapter with asynchronous cleanup must send an owned `CleanupTask` through `AdapterContext.cleanup` before its first startup wait.
The cleanup task must retain the resources and task handles needed after the execution future is dropped.
Adapters without asynchronous resources can drop the registration sender without sending a task.

The supervisor drops the execution future before it starts registered cleanup.
Cleanup runs after normal completion, execution failure, startup failure, panic, or drain timeout. The cleanup budget starts when cleanup begins.
Casper registers cleanup before launch. Its cleanup aborts and joins managed task scopes, stops the observer, and then calls the store manager shutdown.
The normal path first drains active packet and command requests and then stops native workers.
An execution error skips that graceful drain. The supervisor still runs registered cleanup after the execution future stops.
Graceful drain and forced cleanup intentionally use different rules. The drain budget bounds completion waits, and cleanup aborts remaining tasks.
Task scopes retain their join handles if a graceful shutdown wait is canceled, so cleanup can still join those tasks.

`TaskScope::spawn` returns `Ok(())` when it accepts a task, or `ConsensusError::Stopped` after the scope closes.
Rejection drops the supplied future without polling it. Acceptance does not guarantee completion because shutdown can abort an accepted task.
`join_next` returns `Some(name)` for a successful task and an error for a failed or aborted task.
It waits when the scope is open and empty. It returns `None` when the scope is closed and empty.
Closing a scope wakes an existing join waiter. Concurrent join and shutdown calls serialize access to task completion.

The native Casper callback also returns a scheduling result. Genesis startup propagates rejection of its required approval task.
Deploy submission logs rejected auto-propose scheduling and preserves the result of the deploy that was already accepted.

A drain timeout reports `ShutdownTimeout`. A cleanup timeout reports `CleanupTimeout`.
If execution and cleanup both fail, `ShutdownFailed` preserves both errors. Any failure produces `Phase::Failed`, rather than a clean stopped status.
If cleanup itself times out before tasks stop, explicit storage shutdown might not run. The runtime reports this failure and drops the remaining cleanup resources.
Use `ConsensusRuntime::shutdown().await` for managed shutdown. Dropping the runtime still aborts the supervisor and does not await asynchronous cleanup.

## Build selection

The node enables the `cbc-casper` Cargo feature by default. The configuration selector remains `consensus.protocol = "cbc-casper"`.
Unknown configuration values fail during parsing. A recognized protocol without a compiled factory returns `UnavailableProtocol` before logging, TLS setup, or transport startup.

Use the following command to check the host without the Casper adapter:

```sh
cargo test --locked --release -p node --no-default-features \
  --test consensus_factory --test consensus_application \
  --test consensus_ingress --test consensus_boundaries
```

The feature gates the adapter, its API modules, and its factory registration. The node still depends on the Casper crate for existing configuration and CLI types.
This boundary does not replace the execution system or define a universal block, snapshot, or execution record.

## Verification

Run the shared lifecycle tests:

```sh
cargo test --locked --release -p consensus-api -p consensus-runtime
```

Run the host and Casper compatibility tests:

```sh
cargo test --locked --release -p node --lib \
  --test consensus_application --test consensus_ingress \
  --test consensus_factory --test consensus_boundaries --test consensus_casper
```

The Casper acceptance test uses HTTP routes and a loopback gRPC server. It covers deploys, proposals, finality, native packet delivery, shutdown, and store recovery.
Additional tests cover initialization failure and store identity rejection. These focused checks do not replace the full network acceptance suite.
The observer lifecycle test checks socket cleanup after preparation cancellation and runtime shutdown.
The peer-wait shutdown test waits for a packet acknowledgement after launch, while the test connection set remains empty.
Its shutdown deadline includes the configured drain and cleanup budgets. A separate test cancels startup immediately.
Readiness tests check both APIs through preparation, bootstrap, readiness, draining, shutdown, and failure.
Manifest tests cover concurrent publication and legacy checkpoints with and without a genesis entry.

## Related designs

The lifecycle names remain `ConsensusRuntime` and `ConsensusAdapter`, as decision D7 proposes in [PR #627](https://github.com/F1R3FLY-io/f1r3node-rust/pull/627).
`ReplicationMedium`, neutral finality and membership types, shared deploy-pool ownership, and the execution port remain separate work.
Cordial integration remains in issue #625.

[PR #766](https://github.com/F1R3FLY-io/f1r3node-rust/pull/766) directs readers to the current adaptive-execution draft in [PR #714](https://github.com/F1R3FLY-io/f1r3node-rust/pull/714).
That proposal requires ratification. TASK-027-4 adds the control-update channel that `AdapterContext` does not provide today.
Adaptive controls must preserve block validity and replicated state. This lifecycle change does not implement the adaptive controller.

The adapter capability flags describe consensus operations. They are separate from the proposed client capability names in PR #743.
The proposed version axes, mechanism implementation kind, and manifest extensions remain follow-up design work.

## Operator release notes

- An invalid configured validator key stops startup with an error. An absent key still permits read-only operation.
- Rejection of required genesis-task scheduling fails initialization. Rejected optional auto-propose scheduling preserves an already accepted deploy.
- Owners must await `ConsensusRuntime::shutdown()` for managed cleanup. Dropping the runtime aborts the supervisor without awaiting cleanup.
- Request admission can report overload at the byte limit before the count limit. API clients retain the existing overload response mapping.
- Application errors returned as HTTP 500 use a generic message, including native Casper errors restored across the runtime boundary. Server logs retain diagnostic details.
- A timed-out write or packet operation can have an unknown outcome after processing starts. A timeout does not establish a safe retry.
