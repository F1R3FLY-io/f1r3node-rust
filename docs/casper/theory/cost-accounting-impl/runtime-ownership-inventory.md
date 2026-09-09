# Runtime ownership inventory

## Status and scope

This inventory records the runtime owners inspected on September 7, 2026.
The baseline is commit `559eb07fac98da2e6392e3a84c93bac41558c86c` plus the current uncommitted feature changes.
It supports `pr216-runtime-ownership-inventory` and the subsequent cache-release and history-retention tasks.

The tables distinguish source observations from unproved resource guarantees.
The source inventory covers the named owner categories and their release boundaries.
This document does not establish a process memory bound or a completed verification of every allocation lifetime.
The required follow-up checks are explicit below.

A **value owner** keeps a Rust value alive.
A **lock guard** keeps a lock acquired until the guard drops.
A **snapshot** can retain shared values after another owner removes its own references.
A **cache bound** limits cache-owned entries or estimated bytes, not all caller-owned copies or process memory.

## Runtime and execution owners

| Object | Value owner | Release condition | Remaining verification |
| --- | --- | --- | --- |
| `RuntimeManager` | `MultiParentCasperImpl` and manager clones share `Arc` members. | The last manager and external member owner drop. | Trace shutdown and engine replacement through every outstanding task. |
| Execution runtime | A spawn operation creates a runtime with a new RSpace hot store. | The execution task and all cloned runtime handles drop. | Measure retained owners after success, rejection, cancellation, and panic. |
| Replay runtime | A spawn operation creates new replay maps and a new hot store. | The replay task and its cloned handles drop. | Check every replay retry and report path. |
| `RhoRuntimeImpl` | The runtime shares its reducer, budget, deployment context, and merge channels. | The last corresponding `Arc` owner drops. | Check observer and reducer references for cycles. |
| `StateBoundAdmission` | The value owns shared candidate identifiers, execution evidence, and mergeable values. | The admission value and all shared copies drop. | Track copies across admission, replay, and reporting. |
| `StateBoundExecution` | The value owns shared processed deployments and mergeable values. | The execution value and all shared copies drop. | Check result retention after block completion. |
| Replay permit | `ReplayLock` owns a semaphore with one permit. A caller owns the acquired permit. | The permit drops on return or cancellation. | Check cancellation while queued and while executing. |
| Consensus replay waiter | A scoped waiter increments the waiter count. | The waiter guard drops and decrements the count. | Retain tests for cancellation and report admission. |
| Exploratory permit | A separate semaphore limits exploratory execution. | The acquired permit drops. | Verify all endpoint error paths release the permit. |

The replay permit controls one shared local resource.
It does not serialize independent validators.
This inventory does not authorize changes to its scheduling policy.

Sources:
[runtime manager](../../../../casper/src/rust/util/rholang/runtime_manager.rs),
[Casper owner](../../../../casper/src/rust/engine/multi_parent_casper/types.rs), and
[runtime](../../../../rholang/src/rust/interpreter/rho_runtime.rs).

## Cache owners and limits

| Cache | Owner and current limit | Release operation | What the limit does not establish |
| --- | --- | --- | --- |
| Block index | Manager-shared map, order queue, writer mutex, and byte counter. Defaults: 128 entries and 64 MiB estimated bytes. | Eviction, replacement, clear, or last owner drop. | Caller-owned `BlockIndex` copies can retain shared state after eviction. |
| Active validators | Manager-shared map and order queue. Default: 256 entries. | Eviction or last owner drop. | Entry count does not bound validator-vector bytes. Concurrent miss admission needs a separate bound check. |
| Bonds | Manager-shared map and order queue. Default: 64 entries. | Eviction or last owner drop. | Entry count does not bound bond-vector bytes. Concurrent admission needs verification. |
| Bond generations | Manager-shared map and order queue. Uses the 64-entry bonds limit. | Eviction or last owner drop. | Entry count does not bound generation-map bytes. Concurrent admission needs verification. |
| Parent post-state | Manager-shared map and order queue. Default: 64 entries. | Eviction or last owner drop. | A merged state contains variable-size rejection, effect, and signature collections. |
| Replay evidence | One mutex protects entries, order, and byte accounting. Defaults: 192 entries and 32 MiB estimated bytes. | Eviction, replacement, clear, or last owner drop. | A cache read returns an owned copy outside the cache byte counter. |
| Replay publication temporary | `collect_replay_logs` builds an owned event vector before the publication limit check. | The publication call returns or moves the vector into an entry. | The 1,536-event admission limit does not prevent the earlier full vector allocation. |
| Hot-store history caches | Each hot store owns continuation, datum, and join caches with entry and item thresholds. | Bulk clear or last hot-store owner drop. | Item thresholds are not byte bounds. Concurrent counter reset and refill need verification. |
| Radix read cache | Each radix-tree instance owns decoded nodes through `Arc`. The trim threshold is 4,096 entries, with a 2,048-entry target. | Trimming, explicit clear, or tree destruction. | Readers can retain node handles after eviction. Separate history instances have separate caches. |
| Radix write cache | The current history tree owns serialized nodes pending commit. | Successful checkpoint processing clears the cache. Tree destruction also releases it. | The inspected write cache has no byte admission limit. A failed commit does not follow the successful clear path. |

These are separate cache limits, not an aggregate node memory limit.
No observation in this table proves that a cache caused a reported out-of-memory failure.

Sources:
[cache limits and publication](../../../../casper/src/rust/util/rholang/runtime_manager.rs),
[replay cache](../../../../casper/src/rust/util/rholang/replay_cache.rs), and
[hot store](../../../../rspace++/src/rspace/hot_store.rs).

The radix-cache observations come from
[radix tree](../../../../rspace++/src/rspace/history/radix_tree.rs) and
[radix history](../../../../rspace++/src/rspace/history/instances/radix_history.rs).

## RSpace and persistent storage owners

| Object | Value owner | Release or replacement condition | Safety boundary |
| --- | --- | --- | --- |
| Active RSpace hot store | `ArcSwap` publishes the current store. Each `load_full` caller owns another reference. | A store replacement releases only the publisher's old reference. | In-flight callers can retain the old store. Replacement alone does not prove quiescence. |
| Replay hot store | The replay space publishes a shared store through its lock. | Reset, checkpoint, or revert replaces the published store. | Existing cloned references remain valid owners. |
| Live data and continuations | Five sharded persistent maps own data, continuations, installed continuations, joins, and installed joins. | Removal, clear, replacement, or last snapshot drop. | Persistent continuations and installed processes have intentional longer lifetimes. |
| Soft checkpoint | `SoftCheckpoint` owns persistent-map roots, an event log, and produce counters. | Revert consumes the checkpoint, or the checkpoint drops. | Shared map nodes survive while any snapshot or active map references them. |
| Event and replay indices | RSpace owns event logs and produce counters. Replay also owns its event multiset and consume index. | Take, reset, rig, clear, checkpoint, or last owner drop, according to the operation. | Verify each index separately. Clearing one index does not prove release of another. |
| Current history | A history repository owns a shared current-history wrapper. Reset can create a new wrapper. | The last wrapper owner drops. | This releases a handle, not necessarily persistent records. |
| Root repository | History repositories share the root repository and its backing store. | Handle drop releases local ownership. Explicit storage policy governs records. | Do not delete roots required by replay, recovery, or delayed peers. |
| Cold store and import/export handles | History repositories share backing stores and importer/exporter handles. | The last handle and active operation drop. | Rust destruction does not constitute disk compaction or historical garbage collection. |

Sources:
[RSpace setup](../../../../rspace++/src/rspace/rspace/setup.rs),
[checkpoint operations](../../../../rspace++/src/rspace/rspace/ispace_impl.rs),
[replay space](../../../../rspace++/src/rspace/replay_rspace.rs), and
[history repository](../../../../rspace++/src/rspace/history/history_repository_impl.rs).

### Root and history consumers

A **root marker** records a root hash in the root store.
A **history reader** owns a root-specific history instance and shared access to the cold store.
A root marker, a local handle, and complete stored state are different objects.

`RootsStore::contains_root` checks for the root marker.
It does not traverse the history trie or validate all referenced cold-store records.
The root-store interface provides no root-deletion operation or reader lease.
Do not interpret a successful marker lookup as proof of complete state or safe concurrent reclamation.

| Consumer | Root or history selected | Local owner and release | Retention requirement |
| --- | --- | --- | --- |
| Runtime creation | The source space's current history root. | A spawned space owns a reset repository, new hot store, and reader. These owners drop with the space. | The selected root's data must remain available during execution. |
| Reset and checkpoint | An explicit reset root or a newly committed checkpoint root. | The space replaces its repository and hot-store handles. Earlier readers retain their own handles. | Replacement must not invalidate data still needed by an earlier reader. |
| Block replay and reporting | The block's pre-state, intermediate checkpoints, and computed post-state. | The replay or reporting task owns its runtime and results. | Required roots must survive through replay completion or recovery of missing state. |
| Block-index construction | The block pre-state used to classify events and state changes. | A local reader supplies event-index construction. The returned index owns derived data. | Root data must remain readable while the index is computed, including cache misses. |
| DAG merge | The selected merge base. | Merge computation shares a root-specific reader between its local operations. | The base must remain readable until conflict resolution and state construction finish. |
| Mergeable-value conversion | The execution pre-state. | `convert_number_channels_to_diff` owns one reader and an initial-value map for the call. | All referenced initial values must remain readable throughout conversion. |
| Query endpoints | The requested state hash. | Data, continuation, and exploratory calls create temporary runtimes or readers. Returned values have caller-owned lifetimes. | The query must have available state or report an explicit failure. Cache eviction does not satisfy this requirement. |
| State exporter | The requested traversal path and shared history, value, and root stores. | Export operations own key lists and loaded response bytes. The exporter retains shared store handles. | A delayed peer can need historical data independently of the current runtime root. |
| Runtime state requester | A missing root associated with one or more waiting blocks. | Its actor owns pending roots, owner hashes, paths, retry state, an importer, and the `has_root` closure. | Actor retirement and owner release must not erase the last required recovery obligation. |
| Startup state requester | The restore root and forward-horizon roots. | Stream processors share request state, pending roots, path maps, importer handles, and progress state. | Completion must distinguish imported root markers from the required usable state. |

`RSpaceHistoryReaderImpl` shares its target history through `Arc`.
`RadixHistory::reset` creates a new radix tree backed by the same store, rather than cloning the old tree's caches.
Successful history processing also constructs a fresh tree for the returned history.
These operations reduce cache sharing between histories. They do not remove persistent records.

The runtime state requester limits tracked roots to 256 and owners per root to 1,024.
Its command and item channels have capacities of 256 and 64 messages.
The requester drops local tracking after accepted completion or release of the last owner.
A stalled root enters retry backoff rather than automatic deletion.
Those count limits do not establish a byte bound for paths, incoming chunks, or stored imported data.

Node setup captures a `RuntimeManager` clone inside the requester's `has_root` closure.
The actor can therefore retain that manager while the actor remains alive.
Its spawn function returns channel senders, not a task-completion handle.
The actor exits after its inbox ends. Sender and nested-task ownership must be included in shutdown tests.

Sources:
[root store](../../../../rspace++/src/rspace/history/roots_store.rs),
[history reader](../../../../rspace++/src/rspace/history/instances/rspace_history_reader_impl.rs),
[block index](../../../../casper/src/rust/merging/block_index.rs),
[DAG merger](../../../../casper/src/rust/merging/dag_merger.rs),
[runtime manager](../../../../casper/src/rust/util/rholang/runtime_manager.rs),
[state exporter](../../../../rspace++/src/rspace/state/instances/rspace_exporter_store.rs),
[state importer](../../../../rspace++/src/rspace/state/instances/rspace_importer_store.rs),
[runtime requester](../../../../casper/src/rust/engine/runtime_state_requester.rs), and
[startup horizon requester](../../../../casper/src/rust/engine/lfs_horizon_requester.rs).

### Import-validity finding

The [state-import analysis](../finalized-floor/state-import-validity.md) records both requester paths, pinned dev provenance, canonical cursors, and the required formal correspondence.

The runtime requester imports supplied history and cold-store items before any content validation in that path.
Its concrete importer writes the supplied keys and bytes directly.
The requester then records the root marker and checks marker presence through `has_root`.
The forward-horizon requester uses the same marker-based completion assumption.

The plan-agent review found no preceding validation in the runtime message route.
The startup tuple-space requester explicitly invokes the existing state validator before writes.
That validator checks hashes and traversal membership, but its interface does not accept the peer's response cursor.
Reusing the validator alone therefore does not establish correct terminal-cursor handling.

The planned repair must reuse canonical exporter traversal and validate the complete page before writes or successful progress.
Root publication must follow validated closure, not marker presence or a peer-declared terminal flag alone.
Invalid input must preserve existing data and the outstanding retry owner.
These requirements restore an existing validation contract. They do not require different Casper voting or wire semantics.

The runtime requester's recording-importer tests do not cover this boundary.
The [real-store regressions](../../../../casper/src/rust/engine/runtime_state_import_tests.rs) now use the actual importer, real root lookup, and independent history, cold, and root stores.
Their nonempty fixture uses the real exporter, normal channel hashing, and typed channel lookup.
Task `pr216-state-import-validity` tracks the native reproduction, provenance, concurrent formal model, repair, and conformance checks.
These regressions do not establish the historical cause of a CI failure.

### Real-store regression results

The pre-repair run reproduced five failures on September 7, 2026.
Both controls passed.
Each destination started with an independent, valid current root.
The rejection checks compare all three stores and retain the original root, cursor, and retry owner.

| Case | Required behavior | Observed result before repair |
| --- | --- | --- |
| Valid nonempty exporter page | Import complete state and read joins through the normal channel interface. | Passed. |
| Unknown response path | Preserve stores and the original retry obligation. | Passed, including redispatch after a controlled clock advance. |
| Invalid history hash | Reject the page before any stored data changes. | Failed. The import replaced data, published the root marker, and retired recovery. |
| Invalid cold value | Preserve the valid value already stored under that key. | Failed. The import overwrote that value and retired recovery. |
| Missing referenced cold value | Keep the root unpublished and retain recovery. | Failed. The import published the root marker and retired recovery. |
| Forged continuation | Preserve the requested cursor and reject unvalidated progress. | Failed. The import changed the cursor and counted the page as progress. |
| Re-export imported state | Supply the same cold data to a later peer. | Failed. The exporter returned no cold item. |

The re-export failure has a separate key-format cause.
The [importer](../../../../rspace++/src/rspace/state/instances/rspace_importer_store.rs) writes cold values under raw hash bytes.
The [exporter](../../../../rspace++/src/rspace/state/instances/rspace_exporter_store.rs) requests those values with bincode-serialized hash keys.
The [history reader](../../../../rspace++/src/rspace/history/instances/rspace_history_reader_impl.rs) supports both representations.
Thus imported joins remain locally readable, but re-export omits them.
The test demonstrates both observations with the same nonempty state.

The focused evidence directory is `target/verification/state-import/red-channel.IFk7Cq`.
The native command returned the expected failure code `101`, with five failures and two passes in 0.01 seconds after compilation.
Strict Casper Clippy, test-file formatting, and recorded input-hash checks passed.
The systemd invocation was `4bc16d48263e4252876541fa2c99e058`.
The scope limited memory to 5 GiB, disabled swap, and limited execution to one CPU.

The reviewed assertions also reject a retry-attempt reset without validated progress.
These seven cases do not establish multipage closure, concurrent-consumer safety, or forward-horizon correctness.
Those checks remain prerequisites for repair acceptance.
No production import behavior changed for this reproduction.

The [existing model boundary](../../../../formal/tlaplus/finalized_floor/README.md) already excludes authenticated page transfer and complete-root publication.
That document explicitly states that current Rust network import does not satisfy the model's state-availability premise.
These regressions demonstrate that excluded implementation boundary.
They do not invalidate a proof that claimed to cover page transfer, because no such coverage was claimed there.

Sources:
[runtime routing](../../../../casper/src/rust/engine/running.rs),
[startup validation call](../../../../casper/src/rust/engine/lfs_tuple_space_requester.rs),
[state validator](../../../../rspace++/src/rspace/state/rspace_importer.rs), and
[canonical export pages](../../../../rspace++/src/rspace/state/exporters/rspace_exporter_items.rs).

## Reduction and accounting owners

| Object | Value owner | Release condition | Required correspondence check |
| --- | --- | --- | --- |
| Reduction session | Contexts, participant guards, and a driver task share the session. | The last context, guard, and driver reference drop. | Include the asynchronously spawned driver, not only the caller future. |
| Evaluation guard | The session owns an optional read guard on its coordinator boundary. | The quiescent completion path takes and drops the guard. | Prove that cancelled participants and pending intents cannot retain the boundary indefinitely. |
| Participant and intent maps | Session state owns participant states, operation payloads, and result channels. | Completion removes the participant and its waiting intent. The driver consumes executable intents. | Test cancellation at each transfer boundary. |
| Scoped child handle | `ScopedJoinHandle` owns a Tokio join handle. | Drop requests task abort. | Abort is not synchronous destruction. Observe child completion before claiming complete release. |
| Runtime budget | Reducer forks share counters, attempt queues, reconciliation state, and authority state. | Reset clears per-deploy contents. Last owner drop releases the allocation. | Reset assumes no in-flight deployment operation. Verify that the coordinator enforces this assumption. |
| Cost and authority evidence | Budget queues and maps own attempts, byte events, introductions, reservations, and reconciliation results. | Reconciliation transfers ownership. Reset clears contents. Published evidence has its own owner. | A cleared vector can retain capacity. Preserve evidence required by settlement and replay. |

Sources:
[reduction coordinator](../../../../rholang/src/rust/interpreter/deterministic_reduction.rs) and
[accounting runtime](../../../../rholang/src/rust/interpreter/accounting/mod.rs).

The runtime constructor stores a weak reducer reference in the dispatcher's `OnceLock`.
This back-reference does not create a strong reducer-dispatcher ownership cycle.
The accounting observer retains the shared budget, not the reducer itself.
These inspected edges do not prove that every external service or spawned task releases its owners correctly.

Sources:
[runtime construction](../../../../rholang/src/rust/interpreter/rho_runtime.rs) and
[dispatcher](../../../../rholang/src/rust/interpreter/dispatch.rs).

## Reports and response owners

A report has three separate ownership phases: request admission, replay, and response delivery.
A limit on replay concurrency does not bound allocations in the other phases.

| Object | Value owner | Release condition | Current boundary |
| --- | --- | --- | --- |
| Queued report input | `block_report_with_permit` owns a full block and a Casper handle before it waits for the report semaphore. | The request returns, fails, or its future drops. | The semaphore limits active report generation, not the number or bytes of queued blocks. |
| Report queue metric | `ReportQueueMetricGuard` belongs to the waiting request. | The guard drops after acquisition, an error, or cancellation. | A correct queue metric does not establish bounded queue memory. |
| Reporting runtime | `RhoReporterCasper::trace` creates the runtime after it acquires the shared replay permit. | The trace future and all child owners drop. | Node setup supplies `RuntimeManager::replay_lock()`. This is not an independent reporting replay lock. |
| Replay inputs and results | The trace owns a DAG snapshot, cloned executable deployments, invalid-block data, and report results. | The trace drops inputs and transfers its results to report construction. | These allocations are separate from runtime cache estimates. |
| Reporting events | `ReportingRspace` shares soft events, report batches, and the current phase through three mutexes. | `get_report` transfers batches with `mem::take`. A checkpoint clears internal vectors. | The returned batches remain alive in the caller. Vector clearing can retain capacity. |
| Report storage conversion | The store operation owns report values, protobuf bytes, and compressed bytes during conversion. | Each conversion temporary drops after its last use. | Compression does not bound simultaneous uncompressed copies. |
| Cached report result | A cache read decompresses and decodes an owned `BlockEventInfo`. | The requesting caller drops or transfers the result. | Cache hits bypass the generation semaphore. They still allocate decoded values. |
| HTTP response | The trace handler converts its report to a serialization wrapper, a JSON value, and response bytes. | Each temporary drops after conversion. Response bytes remain owned until consumption or drop. | The generation permit has already dropped before response construction and delivery. |

`ReportingRspace::reset` and `rig_and_reset` delegate to the replay space.
Do not assume that these operations also drain reporting vectors.
`get_report` and `create_checkpoint` contain the explicit reporting-vector release operations.

The inspected main and administrative HTTP routers configure request-body limits, not concurrent report-count or response-byte limits.
Both servers use `axum::serve` without an additional application concurrency layer at their construction sites.
This observation does not establish the capacity of an external proxy or the operating system.
The gRPC per-connection limit does not govern these HTTP handlers.

These source observations establish retained owners, not a measured memory failure.
A regression must control the report permit and observe allocation ownership before any change to request admission.
The regression must also cover cancellation, duplicate requests, cached responses, and slow response delivery.
Moving the block read after the permit would address only queued input ownership, not the other allocation phases.

Sources:
[report API](../../../../casper/src/rust/api/block_report_api.rs),
[report replay](../../../../casper/src/rust/reporting_casper.rs),
[reporting space](../../../../rspace++/src/rspace/reporting_rspace.rs),
[report store](../../../../casper/src/rust/report_store.rs),
[HTTP handler](../../../../node/src/rust/web/reporting_routes.rs),
[HTTP routes](../../../../node/src/rust/web/routes.rs), and
[server construction](../../../../node/src/rust/runtime/servers_instances.rs).

## Finalization receipts and mergeable evidence

A finalization receipt records completion of one durable finalization effect.
Receipt cleanup must preserve recovery information until the effects cursor establishes completion.
This inventory records ownership only. It does not revise the finalization or retention rules.

| Object | Value owner | Release condition | Current boundary |
| --- | --- | --- | --- |
| Effect receipt | The ledger store owns the persistent receipt. | Receipt compaction deletes completed receipt keys. | Dropping a ledger handle does not delete receipts. |
| Compaction scan | `FinalizationReceiptCompaction` owns a ledger clone, cursor, optional key iterator, and optional error. | The scan finishes or drops. | An error remains in the scan and prevents further progress through that scan instance. |
| Receipt iterator | The iterator owns one round's finalized-block set and generates effect keys incrementally. | The iterator consumes its blocks, resets after cursor advancement, or drops. | The 256-key page limits a deletion batch, not the size of the loaded round manifest. |
| Compaction worker | Each `spawn_blocking` closure owns the scan for one page. | The closure completes and returns the scan, or drops its result after the receiver disappears. | Dropping an awaiting future does not prove that an already executing blocking page has stopped. |
| Mergeable sweep | Node setup shares `GcSweep` through an asynchronous mutex captured by the collection loop. | The loop, active pass, and final shared owner drop. | The sweep persists between passes but starts empty after process restart. |
| Pending mergeable hashes | The sweep owns hashes that it cannot yet delete. | A successful eligible deletion attempt removes a hash. Storage errors retain it. | There is no fixed pending-count limit in this sweep. Permanently ineligible hashes can remain indefinitely. |
| Collection pass | The pass owns its DAG snapshot, common-ancestor set, and selected deletion hashes. | The pass finishes or drops. | Candidate and snapshot memory remain separate from runtime cache bounds. |

The compactor deletes receipt keys before it advances the durable compaction cursor.
A failed scan does not serve as a successful cleanup result.
Its replacement reconstructs work from durable cursors and the round record.
These facts do not establish a bound on retained ledger history.

The mergeable collector retains candidates when a storage operation returns an error.
If the block lookup returns no block, the current path removes the pending hash without deleting mergeable data.
That case needs an explicit storage-consistency test before any repair or change to deletion policy.
The collector's current safety predicate remains unchanged.

Sources:
[finalization ledger](../../../../block-storage/src/rust/finality/finalization_ledger.rs),
[receipt pages](../../../../block-storage/src/rust/finality/finalization_ledger/recovery_pages.rs),
[mergeable collector](../../../../casper/src/rust/util/mergeable_channels_gc.rs), and
[node setup](../../../../node/src/rust/runtime/setup.rs).

## Engine replacement and task shutdown

`EngineCell::get` returns a cloned engine handle after it releases the cell lock.
Engine replacement drops the cell's retired handle and registration after publication and cleanup.
An in-flight request can still own the old engine or its Casper handle.
Cell replacement therefore does not establish destruction of the old runtime.

Node shutdown aborts and drains the initializer and critical-task `JoinSet` values, with a 30-second outer timeout.
Some critical tasks only await a separately spawned server handle.
The two HTTP servers run inside separate `spawn_blocking` closures with dedicated runtimes.
Their supervisor wrappers contain no explicit child shutdown request.

The drain establishes completion of the directly supervised tasks when it returns successfully.
It does not establish completion of every nested server task or an already executing blocking operation.
The timeout branch also does not establish complete destruction of those tasks.
Process termination can release resources later, but that is different from an ownership guarantee at `NodeRuntime::main` return.

A shutdown regression must observe the actual server or worker owner, not only the wrapper's completion.
It must start the child before cancellation and retain a controlled cleanup path so the test cannot leave a server running.
It must include normal shutdown, startup failure, a critical-task error, and cancellation during a blocking operation.
No end-to-end shutdown failure has been reproduced as part of this inventory update.

Sources:
[engine cell](../../../../casper/src/rust/engine/engine_cell.rs),
[node runtime](../../../../node/src/rust/runtime/node_runtime.rs),
[runtime supervision](../../../../node/src/rust/runtime/runtime_supervision.rs), and
[server construction](../../../../node/src/rust/runtime/servers_instances.rs).

## Cache lock-order finding

Source inspection found an opposing lock order in five cache families.
Cache hits retain a DashMap shard read guard while requesting the order mutex.
Eviction retains the order mutex while requesting a shard write guard.

A reader and an evictor can request the same key:

1. The reader acquires the shard read guard.
2. The evictor acquires the order mutex.
3. The evictor requests the shard write guard.
4. The reader requests the order mutex.

Neither operation can release its first guard until its second acquisition completes.
This is a source-level deadlock path, not an attribution of a particular CI timeout.

Affected methods are `get_active_validators`, `compute_bonds`, `compute_bond_generations`, `get_cached_parents_post_state`, and `get_or_compute_block_index`.
The block-index fast path remains outside its writer mutex.
The writer-locked recheck also retains its read guard through the order update, but writers cannot evict concurrently through that same mutex.

Cloning a cached value does not release its guard.
Shadowing a guard binding also does not release the original guard before the enclosing scope ends.

The plan-agent review selected an explicit guard-release boundary before the order update.
The repair now releases each shard guard after copying the value and before touching the order queue.
The parent-cache path completes its copy statement before the order update.
This repair leaves cache values, eviction policy, byte accounting, and consensus rules unchanged.
It does not add serialization.

The read-only comparison found the same patterns in the MeTTaIL feature worktree.
That worktree lacks the bond-generation cache and contains no corresponding completed guard-release helper.
Pinned `dev` commit `0f5d2b7414786cd27b8a686ca636485c35a5edce` contains the same four shared cache-family patterns.
This branch also uses the pattern in its bond-generation cache and block-index recheck.
The bug is not evidence that this branch changed Casper's protocol.

The [pinned upstream source](https://github.com/F1R3FLY-io/f1r3node-rust/blob/0f5d2b7414786cd27b8a686ca636485c35a5edce/casper/src/rust/util/rholang/runtime_manager.rs)
contains the common order helper, generic eviction helper, and four cache-hit implementations.
Local history identifies commit `1214adada` as the introduction of `evict_fifo_entry`.
This history observation does not establish the origin of every later cache path.

Required evidence precedes a repair:

- A deterministic production probe must observe guard ownership at the order-lock boundary.
- The probe must exercise all affected cache families without creating blocked native threads.
- A concurrent model must distinguish shard guards, order ownership, value copies, and block-index writer ownership.
- Unsafe controls must retain guards after cloning and expose the resulting wait cycle.
- Property tests must preserve returned values and order updates across the guard-release boundary.
- Loom checks must use separate locks. A single cache mutex would conceal this defect.

The existing parent-cache Loom tests use one mutex.
They test cache-key behavior and do not prove the production lock order safe.

### Production regression evidence

The native regression exercised all six affected hit paths through real `RuntimeManager` methods.
A test-only, thread-local observer ran immediately before the order-lock request.
The observer tried to acquire a mutable guard for the same cache entry without waiting.

Each affected path retained its shard guard.
The six release assertions failed as expected.
The no-held-guard control passed.
All seven tests completed in 0.02 seconds without blocked threads.

The cached-value and order-update assertions passed before the guard-release assertions failed.
The block-index recheck test also confirmed that its controlled insertion hook ran.
These tests establish the physical guard boundary, not the cause of a specific historical CI timeout.

Command: `cargo test --offline -p casper --lib cache_lock_tests:: -- --test-threads=1`.
Observed exit: `101`.
The scope used a 5 GiB memory limit, zero swap, one build job, one test thread, and a 100% CPU quota.
The invocation identifier was `e77abacd953b4d6d979c508fc607e770`.

| Input | SHA-256 |
| --- | --- |
| `runtime_manager.rs`, with test-only observers | `f19e83e99922789f63c214a6ddd6e2edbeaf0de3def7d604cccc328f1605dd85` |
| `runtime_cache_lock_tests.rs` | `dd1b38ebd40bc09b156ff9c8219c025fc9fed32921276e23761455967ad76e7f` |

The disposable transcript is `target/verification/runtime-cache-lock/red-20260907/test.log`.
This permanent summary records its result before artifact cleanup.

The same seven native tests passed after the guard-release repair.
They completed in 0.02 seconds under the same resource limits.
The green invocation identifier was `6ca59bc60af34d3cb5336ae5d1700846`.

### Formal model and correspondence

The [cache lock model](../../../../formal/tlaplus/finalized_floor/RuntimeCacheLockOrder.tla)
represents two readers, one evictor, one cached value, and separate physical locks.
The generic configuration has two independent readers.
The block-index configuration has a fast reader and a writer-locked recheck reader.

The fast reader does not acquire the block-index writer mutex.
Copying a value does not remove its owner from `shardReaders`.
The safe transition releases that guard explicitly before requesting the order mutex.

| Model invariant | Meaning | Implementation check |
| --- | --- | --- |
| `TypeOK` | Each owner and operation phase belongs to its declared set. | Rust types establish native owner types. The model checks its own state types. |
| `NoShardGuardAtTouchRequest` | A reader has no shard guard when it requests or holds the order mutex. | Six real cache-hit probes check this boundary. |
| `NoWaitForCycle` | The current lock-wait graph has no directed cycle. | Separate-lock Loom cases explore reader and evictor interleavings. |
| `ReturnedCopyMatchesRead` | A completed cache hit returns the value it observed under the read guard. | Native value assertions and generated ownership tests check returned copies. |
| `ShardMutualExclusion` | A shard writer cannot coexist with shard readers. | DashMap provides the native lock. Loom models this boundary with its read/write lock. |
| `GuardPhaseCoherence` | Each physical guard agrees with its owner's operation phase. | The native probes establish the critical phase mapping. The model checks all modeled phases. |
| `NoCompletedGuardOwner` | A completed modeled operation owns no modeled lock. | Native order access and Loom completion checks exercise release. |

Under weak fairness, `EventuallyCompleted` requires every finite modeled operation to finish.
Weak fairness requires an operation that stays enabled to eventually take a step.
It cannot enable a blocked lock acquisition or remove an existing wait cycle.

| Configuration | Distinct states | Result |
| --- | ---: | --- |
| Generic safe cache | 456 | Safety and finite-operation completion passed. |
| Block-index safe cache | 206 | Safety and finite-operation completion passed. |
| `HoldReadAcrossTouch` | 26 before refutation | `NoWaitForCycle` failed with a reader and evictor holding opposing locks. |
| `ShadowCloneWithoutDrop` | 30 before refutation | The same invariant failed after the fast reader copied its value but retained its guard. |

The complete safe state graphs each had depth 24.
The unsafe controls reached their cycles at depths five and six.
Both unsafe checker exits were `12`, not compilation errors or timeouts.

The pre-repair run checked these invariants and both controls before the production edit.
Its evidence directory is `target/verification/runtime-cache-lock/formal.kA4srX`.
The invocation identifier is `d68b6d25a936406695699cdc62574819`.
The checker used a 512 MiB Java heap inside a 2 GiB systemd scope with no swap and one CPU.

The [formal checker script](../../../../scripts/check-runtime-cache-lock-order.sh)
checks input hashes before it reports completion.
The script requires both safe passes and the two named unsafe counterexamples.

The [Loom checks](../../../../formal/loom/cost_accounting/tests/loom_runtime_cache_lock_order.rs)
use distinct shard, order, and optional writer locks.
DashMap itself is not Loom-instrumented.
The native probes connect the real guard lifetime to the modeled boundary.

These checks do not prove arbitrary-history Casper liveness.
They also do not prove cache capacity, miss computation, replacement policy, full order-queue semantics, or a node memory bound.
The block-index insertion hook demonstrates recheck reachability, not the full concurrent writer and byte-accounting protocol.

### Property and Loom coverage

The native properties generate cache keys, cached values, unique order queues, and stale order entries.
The properties check order updates, exact victim selection, returned copies after eviction, and empty-block index byte accounting.
Each property uses 256 generated cases in the recorded focused run.
Generated tests do not constitute exhaustive enumeration of every key, value, or execution history.

Both Loom cases completed with no preemption, permutation, or duration cutoff configured in their builders.
Each case contains two readers and one evictor.
The builders still operate within Loom's execution limits and the external process resource limit.
The completed runs provide bounded implementation-model evidence, not a proof about every production interleaving.

The local aggregate runner now registers the four model configurations, eleven native tests, and two Loom cases.
It requires the named unsafe invariant failures, not an arbitrary checker error.
See [the aggregate runner](../../../../scripts/check-finalized-floor-ALL.sh).
Formal verification remains a local gate. This repair does not add formal tools to GitHub CI.

### Final focused verification

| Check | Result | Disposable evidence |
| --- | --- | --- |
| Seven native examples and four generated properties | Eleven tests passed. Each property used 256 cases. | `target/verification/runtime-cache-lock/final.yXIy5V/native.log` |
| Two separate-lock Loom cases | Both passed. | `target/verification/runtime-cache-lock/conformance.PDzIhw/loom.log` |
| Casper library and test Clippy checks | Passed with `-D warnings`. | `target/verification/runtime-cache-lock/final.yXIy5V/clippy.log` |
| New Loom target Clippy check | Passed with `-D warnings`. | `target/verification/runtime-cache-lock/conformance.PDzIhw/clippy-loom.log` |
| Final-input model checks | Both safe instances passed. Both unsafe controls produced the required cycle. | `target/verification/runtime-cache-lock/formal.xcpEIg/` |
| New Rust-file formatting, shell syntax, and captured input hashes | Passed. | `target/verification/runtime-cache-lock/final.yXIy5V/` |

The final native tests completed in 1.20 seconds after compilation.
The two Loom cases completed in 0.47 seconds after compilation.
The final native invocation was `8687ba6f7d9b400db8f484daeb056469`.
The final model invocation was `1ab4d5b9dca5424e86a5e211e9d36faa`.

The model rerun used a 1 GiB scope.
The native and lint run used a 5 GiB scope.
Each scope disabled swap and limited execution to one CPU.
The combined owned memory caps, including the separate ongoing dispatcher check, did not exceed 8 GiB.

The full aggregate runner and the whole CI suite were not rerun for this local repair.
The aggregate script registration passed shell syntax checks.
All newly registered cache checks ran through their focused commands.
Other campaign tests and the upstream pruning decision remain open.

## Completion boundary and remaining coverage

The [block-heap lifecycle](block-heap-lifecycle.md) separates live values from allocator-retained pages.
Allocator reclamation cannot release a live cache entry, snapshot, task, or lock guard.
Its abstract reclamation envelope does not replace this ownership inventory.

The [buffer preservation review](../finalized-floor/buffer-pruning-preservation.md) covers admission and recovery ownership separately.
Its unresolved storage policy requires upstream review.
This inventory does not select that policy.

The source inventory is complete for the named owner categories.
The runtime-lifetime repair remains incomplete until the following tasks establish their required evidence.

| Required verification | Current task owner |
| --- | --- |
| Report queue, response, and receipt-manifest allocations under controlled concurrent requests. | `pr216-runtime-cache-release` |
| Child-task release during shutdown, cancellation, panic, and storage errors. | `pr216-runtime-cache-release` |
| Concurrent cache admission, counter resets, and post-eviction caller ownership. | `pr216-runtime-cache-release` |
| Root and history retention across the consumer paths listed above. | `pr216-root-history-retention` |
| Validated import, canonical cursor handling, and complete-root publication before recovery retirement. | `pr216-state-import-validity` |
| Aggregate live copies, persistent snapshots, and allocator-retained pages under a pinned concurrent workload. | `pr216-long-horizon-resource-measurement` |

The inventory identified the guard defect before its separate, formally checked repair.
No Casper architecture change resulted from this repair.
