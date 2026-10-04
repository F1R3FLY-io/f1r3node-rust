# Consensus abstraction implementation plan

Status: Part A is implemented and committed locally. Part B includes native v2 admission, durable blocklace storage, tau output, proposal, synchronization, and committed Rholang execution.
The node has a separate Cordial consensus adapter. Full network acceptance and the remaining production gates are not complete.
See [Part A changes and verification](consensus-runtime-part-a.md) and [the Cordial integration profile](../../cordial/INTEGRATION-PROFILE.md).

The user approved fixed membership and Rholang execution after native tau commitment for the first integration.
PoR is removed from this integration at the user's request. Upstream work and a separate approval are required before it returns.
Speculative multi-parent execution remains disabled.
The initial durable adapter lives in `cordial/cordial-consensus` because it has no node or VM dependency.
The node adapter connects the execution bridge, transport, proposal, and application APIs. Its presence does not establish production readiness.

Prepared: 2026-09-29.

This plan has two delivery parts. Review Part A before enabling Part B in the node.
The milestones below retain the proposed sequence. Local commits now group the implemented changes by dependency and responsibility.

| Part | Required result | Primary acceptance test, proposed |
| --- | --- | --- |
| A — Decouple CBC Casper | `NodeRuntime` starts one `ConsensusRuntime`. A `CasperConsensusAdapter` owns Casper initialization, queues, protocol tasks, and concrete consensus types. | `casper_runtime_deploy_propose_finalize_recover` |
| B — Integrate Cordial Miners | `CordialConsensusAdapter` runs through the same runtime. It preserves native blocklace, validation, finality, equivocation, and tau ordering. | `cordial_runtime_validate_finalize_order_recover` |

These tests use real adapters. A fake adapter tests runtime mechanics, but cannot satisfy either delivery gate.

## 1. Verified starting point

| Repository | Reviewed revision | Local action |
| --- | --- | --- |
| `F1R3FLY-io/f1r3node-rust` | `2e7c8e906b0c48b0886f75481bf56efdc15fcc73` | Fast-forwarded clean `dev` from `28414c7489057527c9f707154c1e00589a872ac1`. `HEAD` and `origin/dev` match. |
| `iCog-Labs-Dev/cordial-f1r3node` | `45de8b8ac85238f9b26563ed525c301c6e03a36d` | Fetched `origin/dev` and read that revision. Preserved the existing feature branch and untracked files. |

The main workspace has no `consensus-api` or `consensus-runtime` crate at this revision. The Cordial workspace has five crates, including `cordial-app-runtime`.

The installed toolchains include the target workspace pin, `nightly-2026-02-09`. Cordial currently specifies floating `nightly`.

`cargo metadata --no-deps --offline --locked` succeeded for the target workspace. It confirmed the package names and existing test targets. Rust tests were not executed during this planning pass.

### Source map

All links below point to the reviewed revisions. Each implementation step names the corresponding source label.

| Label | Source | Relevance |
| --- | --- | --- |
| F1 | [NodeRuntime](https://github.com/F1R3FLY-io/f1r3node-rust/blob/2e7c8e906b0c48b0886f75481bf56efdc15fcc73/node/src/rust/runtime/node_runtime.rs#L158) | Creates the block retriever, unpacks Casper values, launches Casper, and schedules its tasks. |
| F2 | [Node setup](https://github.com/F1R3FLY-io/f1r3node-rust/blob/2e7c8e906b0c48b0886f75481bf56efdc15fcc73/node/src/rust/runtime/setup.rs#L60) | Returns the large tuple of application and Casper values. Also starts tasks during setup. |
| F3 | [Casper engine](https://github.com/F1R3FLY-io/f1r3node-rust/blob/2e7c8e906b0c48b0886f75481bf56efdc15fcc73/casper/src/rust/engine/engine.rs) | Native initialization and `CasperMessage` handling. |
| F4 | [Casper packet handler](https://github.com/F1R3FLY-io/f1r3node-rust/blob/2e7c8e906b0c48b0886f75481bf56efdc15fcc73/casper/src/rust/util/comm/casper_packet_handler.rs) | Existing wire decode and native dispatch. |
| F5 | [Node instances](https://github.com/F1R3FLY-io/f1r3node-rust/tree/2e7c8e906b0c48b0886f75481bf56efdc15fcc73/node/src/rust/instances) | Block processor, proposer, and heartbeat task ownership. |
| F6 | [Casper smoke test](https://github.com/F1R3FLY-io/f1r3node-rust/blob/2e7c8e906b0c48b0886f75481bf56efdc15fcc73/casper/tests/batch1/multi_parent_casper_smoke_spec.rs) | Existing deploy and block-production baseline. |
| F7 | [Casper finalization tests](https://github.com/F1R3FLY-io/f1r3node-rust/blob/2e7c8e906b0c48b0886f75481bf56efdc15fcc73/casper/tests/batch1/multi_parent_casper_finalization_spec.rs) | Round-robin finalization and monotonic last-finalized-block behavior. |
| F8 | [Casper communication tests](https://github.com/F1R3FLY-io/f1r3node-rust/blob/2e7c8e906b0c48b0886f75481bf56efdc15fcc73/casper/tests/batch1/multi_parent_casper_communication_spec.rs) | Missing-block requests and recovery. |
| F9 | [Current CI](https://github.com/F1R3FLY-io/f1r3node-rust/blob/2e7c8e906b0c48b0886f75481bf56efdc15fcc73/.github/workflows/ci.yml) | Locked release tests use nextest. Casper tests run in two partitions. |
| C1 | [Cordial core](https://github.com/iCog-Labs-Dev/cordial-f1r3node/tree/45de8b8ac85238f9b26563ed525c301c6e03a36d/crates/cordial-miners-core/src) | Native types, blocklace, validation, weighted finality, and ordering. |
| C2 | [Cordial startup bridge](https://github.com/iCog-Labs-Dev/cordial-f1r3node/blob/45de8b8ac85238f9b26563ed525c301c6e03a36d/crates/cordial-f1r3node-adapter/src/runtime_bridge.rs) | The current factory returns `LegacyStubAdapter` or `CordialStubAdapter`. |
| C3 | [Cordial ingress](https://github.com/iCog-Labs-Dev/cordial-f1r3node/blob/45de8b8ac85238f9b26563ed525c301c6e03a36d/crates/cordial-f1r3node-adapter/src/live_ingress.rs) | Pending predecessors, persistence, snapshots, and trusted observer paths. |
| C4 | [Cordial proposer](https://github.com/iCog-Labs-Dev/cordial-f1r3node/blob/45de8b8ac85238f9b26563ed525c301c6e03a36d/crates/cordial-f1r3node-adapter/src/proposer.rs) | Predecessor selection, execution, signing, and publication ports. |
| C5 | [Cordial persistence](https://github.com/iCog-Labs-Dev/cordial-f1r3node/tree/45de8b8ac85238f9b26563ed525c301c6e03a36d/crates/cordial-f1r3space-adapter/src/lmdb_store) | Store opening, repository operations, and recovery. |
| C6 | [Existing adapter conformance](https://github.com/iCog-Labs-Dev/cordial-f1r3node/blob/45de8b8ac85238f9b26563ed525c301c6e03a36d/crates/cordial-f1r3node-adapter/tests/conformance.rs) | Final leaders, equivocation, and prefix scenarios. These use a mock verifier and Casper-shaped adapter. |
| C7 | [Four-node core simulation](https://github.com/iCog-Labs-Dev/cordial-f1r3node/blob/45de8b8ac85238f9b26563ed525c301c6e03a36d/crates/cordial-miners-core/tests/test_four_node_convergence.rs) | Reordered delivery and convergence. Its fixture disables content-hash and signature checks. |
| C8 | [Real execution tests](https://github.com/iCog-Labs-Dev/cordial-f1r3node/blob/45de8b8ac85238f9b26563ed525c301c6e03a36d/crates/cordial-f1r3space-adapter/tests/test_e2e_execution.rs) | Real Rholang execution. Tests are ignored by default and require explicit execution. |
| C9 | [Cordial adapter manifest](https://github.com/iCog-Labs-Dev/cordial-f1r3node/blob/45de8b8ac85238f9b26563ed525c301c6e03a36d/crates/cordial-f1r3node-adapter/Cargo.toml) | Uses sibling `f1r3node` path dependencies. It is not directly integrated into this Rust workspace. |

## 2. Implementation decisions for review

### Ownership

`NodeRuntime` will own the process. `ConsensusRuntime` will own the selected consensus adapter's lifecycle and task supervision.

The consensus adapter will own protocol state and rules. Application modules will own execution facilities and public server assembly.

| Current responsibility | Proposed owner | Required result |
| --- | --- | --- |
| Transport listeners, discovery, HTTP/gRPC servers, process signals | `NodeRuntime` | These modules run independently of the selected consensus rules. |
| Startup state, task supervision, bounded ingress, health, drain, stop | `ConsensusRuntime` | Both adapters use the same lifecycle interface. |
| `CasperLaunch`, `EngineCell`, `BlockRetriever`, approved-block state | `CasperConsensusAdapter` | No concrete values escape into `NodeRuntime`. |
| Casper proposer, block queues, heartbeat, fork-choice maintenance, merge GC | `CasperConsensusAdapter` | Native behavior stays inside the adapter's supervised task tree. |
| Native blocklace, predecessors, validator view, evidence, finality, tau | `CordialConsensusAdapter` | The host never reconstructs these rules. |
| Existing Casper HTTP/gRPC response types | Casper compatibility module | Existing clients retain their responses. Cordial receives explicit supported routes. |
| Rholang runtime construction and resource lifetime | Application assembly and execution adapter | Both F1R3 integrations use real Rholang execution. The shared consensus interface has no `RuntimeManager` getter. |
| Protocol store schema, indexes, evidence, recovery | Each consensus adapter | Protocol directories stay separate. Shared facilities provide resource management. |

The initial adapters will live under `node/src/rust/consensus/`. This permits gradual extraction from node modules without a `casper -> node -> casper` dependency cycle.

The Casper algorithms stay in the `casper` crate. A later crate extraction can move adapter assembly when its node dependencies are removed.

### Small shared interface

Add two crates: `consensus-api` and `consensus-runtime`. Start with primitive types inside `consensus-api`. Split them into another crate only when a real consumer requires it.

Neither crate may depend on `node`, `casper`, `models`, `rholang`, RSpace, or concrete consensus stores.

Use opaque identifiers and bounded payload bytes at ingress. The adapter owns native decoding and validation. Do not require a shared `Block`, DAG height, snapshot, or fork-choice method.

The following is a design sketch. Supporting types, error details, and tests belong in A2 and A3.

```rust
#[async_trait::async_trait]
pub trait ConsensusAdapter: Send + 'static {
    async fn run(
        self: Box<Self>,
        context: AdapterContext,
    ) -> Result<(), ConsensusError>;
}

pub struct ConsensusRuntime {
    handle: ConsensusHandle,
    supervisor: tokio::task::JoinHandle<Result<(), ConsensusError>>,
}

pub enum ConsensusCommand {
    Submit(SubmitRequest),
    Propose(ProposeRequest),
}

pub enum AdmissionOutcome {
    Accepted(ObjectId),
    Duplicate(ObjectId),
    Deferred { object: ObjectId, missing: Vec<ObjectId> },
    Rejected { object: Option<ObjectId>, reason: RejectReason },
}
```

`AdapterContext` supplies bounded receivers, network access, lifecycle signals, and a status publisher. The factory injects native stores and execution dependencies into the adapter.

`ConsensusHandle` provides submit, propose, status, and shutdown coordination. Requests carry reply channels and request identifiers. Shutdown uses a separate signal, so full ingress cannot block it.

Packet enqueue success means the runtime accepted work. It does not mean the protocol admitted a block. Admission results arrive through correlated results or events.

The runtime supervisor does not serialize all Casper work into one synchronous loop. Preserve current processor concurrency and proposer serialization within bounded tasks.

### Startup and shutdown ordering

1. Validate configuration and the data directory identity.
2. Prepare the selected adapter and its packet route.
3. Start transport and discovery with that route installed.
4. Start the consensus supervisor and native bootstrap work.
5. Keep packet processing active while bootstrap or synchronization waits for peers.
6. Publish `Ready` only when the adapter satisfies its native readiness condition.
7. On shutdown, stop new proposals and application submissions.
8. Drain admitted work within the configured deadline.
9. Flush required state, stop protocol tasks, and close resources.
10. Stop transport after protocol work no longer requires it.

A startup failure must stop every task already created. A critical task failure must reach the node supervisor.

Peer discovery remains active during synchronization. The adapter handles the protocol-specific peer wait. A single awaited initialization call must not block ingress.

### Compatibility and scope limits

- Keep CBC Casper as the default consensus. Keep Rholang as the current F1R3 execution runtime.
- Preserve Casper packet types, protobufs, signature rules, genesis, and store layouts in Part A.
- Map existing `routing::Packet` values to internal neutral inputs. An internal envelope does not require a new Casper wire format.
- Give Cordial a distinct versioned packet namespace in Part B.
- Retain native CBC traits below its adapter. Cordial must not implement `MultiParentCasper` to enter the runtime.
- Treat fork-choice, finality thresholds, merge rules, and slashing changes as separate protocol changes.
- Add execution ports where Cordial actually needs them. A full VM or storage rewrite is outside this refactor.
- Keep local API capabilities separate from chain-critical rules. An optional query flag changing must not invalidate an otherwise compatible store.

Part A exposes Casper's native finalized progress. It does not promise a universal ordered block stream from an arbitrary DAG traversal.

An `ORDERED_COMMIT_OUTPUT` capability requires a tested protocol-to-application ordering rule. Casper may report finalized progress without this optional capability.

For Cordial, tau supplies the native ordered prefix. The adapter must validate that each published suffix extends that prefix.

## 3. Part A — Decouple CBC Casper from NodeRuntime

Part A is complete when the Casper acceptance test passes through `ConsensusRuntime`, existing Casper tests pass, and `NodeRuntime` contains no Casper lifecycle logic.

### A1. Establish the Casper behavior baseline

**Change:** Record the current behavior before moving code. Add a runtime harness that exercises production assembly with temporary directories and controlled transport.

**Files:** `node/tests/consensus_casper.rs`, `node/tests/support/`, and narrowly scoped test hooks near F1–F2. Reuse F6–F8 scenarios.

`casper/tests/helper` is private to its integration test target. Do not assume another crate can import it. Share only necessary fixtures through `test-utils` if needed.

**Suggested commits:**

```text
test(node): characterize Casper startup proposal and recovery
test(node): characterize Casper task failure and shutdown
```

**Verification:** Exercise standalone and joining modes. Submit a real signed deploy and propose through the current node entry points. Record post-state, finalization, and readiness.

Exercise validator and read-only configurations. Test failed initialization, full queues, and shutdown during initialization. Record existing failures separately from refactor regressions.

**Exit gate:** The baseline tests run against the current path. The fixtures contain enough validator participation to advance finalization beyond genesis.

### A2. Define the shared lifecycle contract

**Change:** Add `consensus-api` with lifecycle states, capabilities, neutral packet metadata, request/reply types, and library errors.

**Files:** Root `Cargo.toml`, `Cargo.lock`, `consensus-api/Cargo.toml`, and `consensus-api/src/{lib,error,input,status}.rs`.

Define size limits, queue-full behavior, request cancellation, and error meaning. Separate unsupported operations from temporary unavailability.

**Suggested commit:**

```text
feat(consensus-api): define protocol-neutral lifecycle and command types
```

**Verification:** Compile a boxed adapter with the proposed interface. Test valid and invalid transitions. Test bounded decoding inputs and typed capability errors.

**Exit gate:** `cargo tree -p consensus-api` contains no protocol or VM dependency. No native block type appears in the public interface.

### A3. Implement ConsensusRuntime supervision

**Change:** Add the runtime, handle, task registry, and deterministic test adapter. Supervise task errors, initialization, readiness, and shutdown.

**Files:** `consensus-runtime/`, root manifests, and `consensus-runtime/tests/lifecycle.rs`.

Register every child task, including tasks created inside helper constructors. Dropping a `JoinHandle` must not leave untracked work running.

**Suggested commits:**

```text
feat(consensus-runtime): add bounded command routing and task supervision
feat(consensus-runtime): add readiness and coordinated shutdown
```

Each commit includes focused tests for the behavior it adds.

**Verification:** Test queue saturation, closed reply channels, initialization failure, task panic, unexpected task completion, and deadline expiry.

Test shutdown with a full queue. Test that peer-dependent startup continues to process packets. Verify that all child tasks stop after failure or drain.

**Exit gate:** Runtime tests pass without Casper or Cordial dependencies. The fake adapter is used only for these lifecycle tests.

### A4. Encapsulate Casper assembly

**Change:** Introduce `CasperConsensusAdapter`. Move construction details from F1–F2 into the adapter's assembly module.

Move `BlockRetriever`, requested-block tracking, approved-block state, `EngineCell`, `CasperLaunch`, processor queues, and proposer state together.

**Files:** New `node/src/rust/consensus/{mod,factory}.rs`, `node/src/rust/consensus/casper/{mod,assembly}.rs`, F1, and F2.

Keep an internal `CasperResources` aggregate during the move. Only the Casper assembly module may construct or access it.

**Suggested commits:**

```text
refactor(node): encapsulate Casper construction in consensus adapter
refactor(node): expose prepared consensus and application services
```

Replace the setup tuple with a named `PreparedNode` result. It contains the neutral consensus runtime and application server modules.

**Verification:** Run A1 against the extracted construction path. Compare genesis identity, configuration values, execution dependencies, and opened store names.

**Exit gate:** Setup no longer exposes Casper queues and engine cells to `NodeRuntime`. Construction has not changed consensus or storage semantics.

### A5. Move Casper tasks and ingress into the adapter

**Change:** Transfer native task ownership from F1 and F5. Use F4 for existing Casper decoding behind the adapter route.

Move block processing, proposal processing, heartbeat, dependency retrieval, stale fork-choice checks, and merge GC. Include the runtime-state requester and queue-metric sampler.

Audit report-prewarming and readiness tasks currently started in F2. Place application tasks in supervised application modules and consensus tasks in the adapter.

**Files:** `node/src/rust/consensus/casper/{tasks,ingress}.rs`, F1–F5, and `casper/src/rust/engine/runtime_state_requester.rs` if its task handle needs exposure.

**Suggested commits:**

```text
refactor(node): supervise Casper protocol tasks through ConsensusRuntime
refactor(node): route existing Casper packets through consensus ingress
```

**Verification:** Reuse the missing-block scenarios in F8. Send duplicate, malformed, invalid-signature, and child-before-parent packets through the route.

Assert native admission results remain unchanged. Compare queue limits and failure behavior with A1. Confirm heartbeat and GC obey existing configuration.

**Exit gate:** `NodeRuntime` no longer calls `casper_launch.launch()` or schedules Casper loops. Native block queue items remain inside the adapter.

### A6. Migrate node callers and protect data directory identity

**Change:** Route submit, propose, status, and health operations through `ConsensusHandle`. Isolate remaining Casper response conversion in a compatibility module.

**Files:** `node/src/rust/consensus/casper/api_compat.rs`, `node/src/rust/runtime/{setup,node_runtime,api_servers,servers_instances}.rs`, affected node API modules, and `node/src/rust/consensus/manifest.rs`.

Move `BlockReportAPI` construction and use behind application server assembly. `NodeRuntime` must not receive that concrete value as a task argument.

Add a sidecar manifest for chain/genesis identity, protocol ID, protocol contract version, and schema version. Write it atomically after identifying the store.

For an existing Casper directory, verify native genesis and store metadata before recording `cbc-casper`. Reject an ambiguous directory without relabeling it.

**Suggested commits:**

```text
refactor(node): route consensus requests through ConsensusHandle
refactor(node): isolate Casper API compatibility from NodeRuntime
feat(node): validate consensus identity before opening protocol stores
```

**Verification:** Compare existing HTTP/gRPC response fixtures. Test read-only proposal refusal and syncing status. Test new, legacy Casper, corrupt, and mismatched directories.

Assert manifest mismatch fails before protocol writes. Retain existing execution receipts and replay behavior without routing speculative execution twice.

**Exit gate:** `NodeRuntime` depends on neutral lifecycle values. Casper compatibility code stays in named modules. Existing Casper data remains readable.

### A7. Prove the Casper delivery and remove temporary wiring

**Change:** Make the runtime path the normal Casper path. Remove the temporary comparison path after the behavior comparison passes.

**Files:** `node/tests/consensus_casper.rs`, `node/tests/support/`, the runtime/adapter modules, and CI test registration.

**Suggested commits:**

```text
test(node): verify Casper consensus runtime end to end
refactor(node): remove legacy Casper lifecycle assembly
```

Keep the characterization scenarios. Run them against the final production entry point after removing duplicate assembly.

**Primary test:** `casper_runtime_deploy_propose_finalize_recover`.

1. Start enough configured validators to satisfy the fixture's native quorum.
2. Start nodes through the production factory and `ConsensusRuntime`.
3. Submit a deterministic signed Rholang deploy through the handle or public route.
4. Propose and disseminate through adapter entry points.
5. Assert receiving nodes validate the real block and agree on its post-state.
6. Produce support blocks until native finalization advances beyond genesis.
7. Assert finalized progress is monotonic and agrees after synchronization.
8. Stop a node, close its stores, and reopen the same temporary directory.
9. Assert the finalized point and deploy status survive recovery.
10. Shut down all nodes and assert their supervised task registries are empty.

The multi-node test must use the same genesis, deterministic deploy bytes, and controlled time. Compare protocol results, not log wording.

Add focused companion tests for missing history, invalid blocks, read-only nodes, task failure, and existing-client response compatibility.

**Part A acceptance:** The primary test, focused companion tests, existing Casper suites, and dependency checks pass. Casper remains the default.

**Rollback:** Revert an unmerged extraction step while its comparison path exists. After cutover, use the previous compatible Casper release and store backup.

## 4. Part B — Integrate the native Cordial consensus adapter

Start Part B after Part A passes. Keep the Part A acceptance test mandatory throughout Part B.

### B1. Integrate the maintained Cordial modules

**Local implementation status, 2026-10-02:** The core and application runtime remain imported from `45de8b8ac85238f9b26563ed525c301c6e03a36d`.
PoR is removed at the user's request. The active import inventory contains 107 files.
`scripts/check_cordial_import.py` verifies provenance and rejects accidental PoR registration.
The source import and the implemented integration layers are now grouped into local semantic commits.

The approved v2 corrections add a content-hash domain, total predecessor ordering, mandatory received-block admission, and nine signed regressions.
Native approval, finality, and tau implementations remain unchanged.
The earlier verification run passed 452 core tests and nine application runtime tests, with no ignored doctests.
See [the source report](../../cordial/SOURCE.md) for historical runs and current verification evidence.

`cordial-consensus` now owns chain-bound packets, durable native admission, pending dependency recovery, persisted tau output, and ingress lifecycle integration.
The execution journal now stores receipts, state roots, deploy identities, and the next execution index.
`cordial-rholang` validates signed deploys and executes only committed output through the real VM.
The runtime executes pending work before readiness and after packet admission.
The new tests cover reverse-order ingress, RSpace reopen, duplicate deploys, rejected deploys, failed execution, and missing execution state.
The node now connects selection, authenticated transport, proposal, and synchronization. Resource supervision under load and full network acceptance remain open.
The official Casper result remains 122 passed out of 124, with two resource-guard errors.

**Change:** Bring the reviewed Cordial implementation into the target build with explicit source provenance and one dependency set.

Recommended approach: import the required crates under `cordial/` as maintained workspace members. Record the source revision and retain licenses, native tests, and required assets.

Import the core, application runtime, execution/storage bridge, and reusable integration modules. Keep PoR, observer tools, and Casper-compatibility demos outside this integration.

Register the standalone core and application crates first. Register each bridge only when its adapted dependencies and tests pass.

This source import is a proposal for review. A pinned external dependency is an alternative if maintainers retain ownership in the separate repository.

**Files:** Root manifests, `cordial/*/Cargo.toml`, `cordial/SOURCE.md`, imported source/tests, and the core dependency-check script.

C9 uses sibling `f1r3node` paths. Adapt them to this workspace. Check `heed` 0.20 versus 0.22, `rand` 0.8 versus 0.9, and the pinned compiler.

Do not force one version when native types or behavior are incompatible. Keep an internal version where necessary and adapt the exposed types deliberately.

The upstream execution bridge enables `casper/test-utils` in a normal dependency.
The new local `cordial-rholang` bridge does not enable that feature. Its tests construct real genesis through production methods.

**Suggested commits:**

```text
build(consensus): import pinned Cordial core and standalone tests
build(cordial): integrate execution and storage bridges with native tests
```

**Verification:** Build on the target toolchain. Run imported core and application tests. Compare with the pinned source baseline.

**Exit gate:** A clean checkout builds without an undeclared sibling repository. Source provenance and test assets are retained. Cordial core remains independent of Casper.

### B2. Define Cordial chain configuration and authenticated ingress

**Change:** Register native Cordial types and a versioned packet namespace. Inject an authoritative validator and weight configuration.

**Files:** `node/src/rust/consensus/cordial/{mod,config,wire,ingress}.rs`, C1 crypto/validation modules, and Cordial adapter tests.

Specify chain identity, trusted genesis, membership, leader selection, wavelength, fault assumptions, and validation mode. These values must agree across nodes and restarts.

Use real hash and signature verification. Validate against the configured validator view, never an untrusted block's claimed bonds.

Define deterministic transport encoding. Test predecessor permutations and native content identity. Keep established hash semantics unless a separately reviewed protocol correction is required.

Document how chain/version separation is authenticated. Changing signed bytes requires a versioned protocol decision, not a silent adapter conversion.

**Suggested commits:**

```text
feat(cordial): add chain configuration and authenticated packet ingress
test(cordial): verify canonical identity and cross-chain rejection
```

**Verification:** Accept valid signed native blocks. Reject altered payloads, invalid signatures, unknown validators, invalid versions, and mismatched chain context.

Require strict predecessor and equivocation checks for the selected production mode. Observer-only `AlreadyValidatedVerifier` paths cannot admit P2P traffic.

**Exit gate:** The adapter validates native objects with all required checks enabled. No `CordialStubAdapter` or fake Casper block is used.

### B3. Implement native admission, dependency recovery, and persistence

**Change:** Drive blocklace admission through the adapter. Translate results into accepted, duplicate, deferred, and rejected outcomes.

**Files:** `node/src/rust/consensus/cordial/{admission,recovery,store}.rs`, C3–C5 reusable modules, and store tests.

Bound pending objects by count and bytes. Bound dependency requests, retries, and per-peer load. Retry children when predecessors arrive.

Persist admitted objects and required metadata before publishing them to live readers. Preserve verified conflicting objects in evidence storage when native admission rejects them.

Use one authoritative mutable consensus state. A live mirror and a separate driver must not produce independent finality decisions.

Treat execution storage and consensus metadata as separate durability domains when necessary. Use a journal and idempotent recovery across them.

**Suggested commits:**

```text
feat(cordial): add bounded native admission and dependency recovery
feat(cordial): persist consensus state and recover durable admissions
```

**Verification:** Test child-before-predecessor delivery, duplicates, retry exhaustion, restart, evidence recovery, and injected storage errors.

Test crashes before and after each durable transition. Corruption required by committed output must fail recovery or trigger explicit repair.

The adapter must not report readiness after silently omitting committed history.

**Exit gate:** Native objects, evidence, and progress survive recovery. Live state never advertises an admission that failed its durable step.

### B4. Connect finality, equivocation, and ordered output

**Change:** Call native approval, ratification, weighted finality, and tau functions from C1. Preserve their preconditions and weight snapshots.

**Files:** `node/src/rust/consensus/cordial/{progress,evidence}.rs`, native finality/ordering modules, and a durable output journal.

Keep equivocation detection, exclusion from approval, permanent ejection, and slashing as distinct actions. The adapter must not invent a shared global blacklist.

Keep the approved validator set and weights fixed. PoR transitions and dynamic membership are outside this delivery.

Persist the output record sequence and producer cursor atomically. Track each consumer acknowledgement separately.

Delivery is at least once. A consumer deduplicates by stable record identity and applies effects atomically with its acknowledgement where possible.

This permits replay after a crash without applying an application effect twice. Cursor persistence alone cannot guarantee exactly-once external effects.

**Suggested commits:**

```text
feat(cordial): publish native finalized tau progress
feat(cordial): preserve evidence and durable output acknowledgements
```

**Verification:** Compare adapter results with direct native functions on identical valid blocklaces. Run tau and equivocation property tests.

Test duplicate delivery, reordered input, growing views, persisted weight transitions, consumer replay, and crash recovery of the output journal.

**Exit gate:** Tau output extends the previous committed prefix. Evidence and exclusion semantics match the native protocol under equivalent views.

### B5. Connect proposal, synchronization, and real execution

**Local partial implementation:** The execution journal, real VM bridge, and runtime execution connection are present.
Proposal and peer synchronization are also implemented. B5 still requires network acceptance, economic verification, process-crash recovery checks, and cross-process execution agreement.

**Change:** Use native predecessor selection and the existing proposer ports. Connect actual transport and the F1R3 Rholang execution adapter.

**Files:** `node/src/rust/consensus/cordial/{proposal,sync,execution}.rs`, C4–C5, and the execution bridge tests from C8.

Validate a locally proposed object through the same strict rules used for received objects before durable admission and broadcast.

Define the execution starting state for each proposal. Define how ordered final output reaches application execution and reporting.

Specify speculative execution versus committed application effects. Persist receipts and roots. Never execute the same committed application effect twice during recovery.

Provide bootstrap trust, object retrieval, bounded catch-up, and deterministic validator configuration. Test that an observer cannot propose as a validator.

**Suggested commits:**

```text
feat(cordial): connect native proposal and peer synchronization
feat(cordial): integrate Rholang execution and receipt recovery
```

**Verification:** Propose using real keys. Verify local output at another node. Execute a real deploy and compare receipts and state roots.

Run the ignored execution tests explicitly. Test failed deploys, missing execution state, proposal cancellation, and a restart after execution but before admission.

**Exit gate:** A real Cordial node can join, propose, validate, execute, finalize, and recover through the shared runtime.

### B6. Enable explicit protocol selection and supported routes

**Change:** Register the Cordial adapter in the production factory. Enable explicit `consensus.protocol = cordial-miners` selection.

**Files:** `node/src/rust/consensus/factory.rs`, node configuration/CLI, manifests, status routes, and manifest tests.

Add a `cordial-miners` Cargo feature for node registration. Its absence must produce a clear configuration error when Cordial is requested.

One process runs one selected protocol for its chain. A binary can contain both factories. Runtime selection does not mean switching a running chain's consensus.

Keep Casper as the default for new directories and verified Casper stores. An existing Cordial store requires explicit Cordial selection.

Expose protocol ID, version, capabilities, readiness, and progress age. Return typed unsupported errors for Casper-specific queries without native Cordial equivalents.

**Suggested commits:**

```text
feat(node): register selectable Cordial consensus adapter
feat(node): enforce protocol capabilities and store compatibility
```

**Verification:** Test both explicit selectors, the Casper default, missing build support, unknown selectors, and all protocol/store mismatch combinations.

Test that capability presence remains distinct from current readiness. Temporary loss of peers must not change the declared interface.

**Exit gate:** Exactly one adapter starts. Unsupported routes cannot return fabricated Casper data. A store cannot silently change protocols.

### B7. Prove the Cordial delivery and keep Casper green

**Change:** Add the native runtime acceptance test and a process-level cluster test. Require both adapters in continuous integration.

**Files:** `node/tests/consensus_cordial.rs`, `node/tests/support/`, `node/tests/consensus_protocol_selection.rs`, and CI configuration.

**Suggested commits:**

```text
test(node): verify native Cordial consensus runtime end to end
test(consensus): require Casper and Cordial runtime acceptance gates
```

**Primary test:** `cordial_runtime_validate_finalize_order_recover`.

1. Configure four validators with fixed test keys and a documented valid quorum configuration.
2. Start the real Cordial adapter through the production factory and `ConsensusRuntime`.
3. Submit a payload and construct properly signed native blocks through the proposer.
4. Deliver objects in different orders, including duplicates and a child before its predecessor.
5. Assert missing predecessors produce bounded requests and deferred admission.
6. Complete dependency delivery and assert pending objects become admitted once.
7. Advance enough native waves to obtain a final leader and nonempty tau output.
8. Assert synchronized honest nodes agree on the finalized output prefix and execution results.
9. Extend the blocklace and assert the earlier output remains a byte-for-byte prefix.
10. Restart a node from its temporary directory and assert evidence, progress, and consumer state recover.
11. Shut down the cluster and assert all supervised tasks stop.

Use a second named scenario, `cordial_runtime_preserves_equivocation_semantics`, for the adversarial branch.

Create two correctly signed conflicting objects from one validator. Preserve the proof even if one object is rejected from the admitted blocklace.

Compare approval and ordering with native rules after equivalent evidence arrives. Assert an acknowledged equivocation cannot contribute the support prohibited by those rules.

Do not assert that every equivocation prevents all honest finality. Progress depends on the remaining honest quorum and the native protocol assumptions.

Also require `cordial_runtime_rejects_invalid_signature`, `cordial_runtime_replays_output_without_reapplying_effects`, and the manifest mismatch tests.

**Part B acceptance:** Cordial's primary test and companion tests pass. Part A remains green with and without the Cordial feature.

**Rollback:** Disable Cordial registration for new launches. Preserve Cordial data for a compatible Cordial release. Never open it with Casper.

## 5. Test commands and continuous integration

Commands below use the workspace's `rtk` convention. Run commands from the target repository unless the text says otherwise.

### Existing Casper regression commands

These test targets exist at the reviewed revision. They establish the baseline before A2.

```bash
rtk cargo test --locked --release -p casper --test mod multi_parent_casper_smoke_spec
rtk cargo test --locked --release -p casper --test mod multi_parent_casper_finalization_spec
rtk cargo test --locked --release -p casper --test mod multi_parent_casper_communication_spec
rtk cargo test --locked --release -p node
```

Check the test count. A mistyped filter that runs zero tests does not pass a milestone.

### Part A gates, available after their files are implemented

```bash
rtk cargo test --locked -p consensus-api
rtk cargo test --locked -p consensus-runtime
rtk cargo test --locked --release -p node --test consensus_casper
rtk cargo test --locked --release -p casper
rtk cargo test --locked --release -p node
```

### Part B gates, available after import and adapter implementation

```bash
rtk cargo test --locked --release -p cordial-miners-core
rtk cargo test --locked --release -p cordial-app-runtime
rtk cargo test --locked --release -p cordial-consensus
rtk cargo test --locked --release -p cordial-rholang
rtk cargo test --locked --release -p node --features cordial-miners --test consensus_cordial
rtk cargo test --locked --release -p node --features cordial-miners --test consensus_casper
rtk cargo test --locked --release -p node --features cordial-miners --test consensus_protocol_selection
```

Retain relevant imported adapter recovery, proposal, and output tests as bridge regression coverage. Existing mock conformance cannot replace runtime acceptance.

The commands assume B1 retains the package names. If an import renames a package, update this plan and CI in that commit.

### CI jobs to add

| Job | Required evidence |
| --- | --- |
| `consensus-runtime-contract` | Shared lifecycle, limits, cancellation, and failure tests pass without native protocol dependencies. |
| `casper-runtime` | Full `consensus_casper` target passes with real execution and persistence. |
| `cordial-runtime` | Full `consensus_cordial` target passes with real signatures and native protocol rules. |
| `consensus-selection` | Defaults, feature availability, capabilities, and manifest checks pass. |
| `consensus-process` | Real transport tests pass for each protocol with isolated data directories and loopback ports. |

Use the existing locked release/nextest pattern from F9 for broad suites. Keep doctests where applicable.

The two named acceptance targets must run nonzero tests. Do not allow their absence to pass CI. Required tests cannot remain ignored.

### Process-level test for each implementation

Start enough actual node processes to satisfy the configured quorum. Use temporary directories and ports allocated by the test harness.

Test cold start, joining, proposal, peer delivery, finality, restart, and termination for Casper. Repeat the workflow with Cordial-native packet and output checks.

For Cordial, use four validators for the documented one-fault scenario. Add partition/heal and conflicting-object delivery after the honest scenario passes.

A process test must wait for observable progress with a deadline. Fixed sleeps and a successful status response are insufficient evidence of finality.

## 6. Review gates and first implementation unit

Each row A1–A7 and B1–B7 is an implementation unit. A unit can contain several small commits listed above.

Before starting a unit, confirm its prerequisite gate. Implement its focused tests with the change. Present the diff and results for review.

Each commit must build with the existing default configuration. Include required module registration, manifest changes, lockfile updates, and focused tests in that commit.

Keep code movement separate from behavior changes where both commits remain buildable. Record any required native protocol correction as a separate prerequisite.

Suggested branch sequence: `feature/consensus-casper-runtime`, then `feature/consensus-cordial-adapter`. Both ultimately target `dev`.

Commit messages in this document are proposed messages. No commits or pushes were created during planning.

The first implementation unit is **A1**. It adds the Casper characterization harness and establishes executable evidence before the extraction.

Review these decisions before implementation:

1. Use two shared crates and keep initial native adapters in node assembly modules.
2. Preserve existing Casper wire and store formats throughout Part A.
3. Treat Casper finalized progress and Cordial tau output according to their native contracts.
4. Import maintained Cordial modules with provenance, or select the pinned-dependency alternative before B1.
5. Require both runtime acceptance tests and the process tests before enabling Cordial for use.

## 7. Completion evidence

| Check | Part A | Part B |
| --- | --- | --- |
| `NodeRuntime` owns no native consensus queues or launchers | Required | Remains required |
| Shared crates avoid protocol and VM dependencies | Required | Remains required |
| Existing Casper tests and public behavior | Pass | Still pass |
| Named Casper runtime acceptance test | Pass | Pass with either build configuration |
| Cordial native blocklace and validation | Not enabled | Pass native and runtime tests |
| Cordial finality, equivocation, and tau ordering | Not enabled | Pass direct-core comparison and recovery tests |
| Real Rholang execution | Preserved | Verified through the Cordial execution bridge |
| Protocol/store mismatch | Rejected | Rejected for both protocols |
| Shutdown and task ownership | All Casper tasks accounted for | All Cordial tasks accounted for |
| Real process test | Casper passes | Both pass |

Record the commit SHA, exact command, test count, result, and remaining failures for every completed unit.

If a native baseline fails, preserve the reproducer. Handle the fix as an explicit prerequisite. Do not disable protocol checks to pass integration.
