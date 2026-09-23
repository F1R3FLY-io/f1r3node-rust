# Casper Node Observation: Batch B Review

**Status:** B2 planning steps 1 through 4 are complete. B2 and Batch C implementation remain unapproved. The Batch A and B1 claims remain pending.

**Branch:** `feature/casper-node-observation`.

**Reviewed base:** `d021a1d53686098fc8719564c29160a1ad21bb04`.

**Consumer:** TASK-017-12. This work remains separate from the harness branch and PR #436.

The [original plan](https://github.com/F1R3FLY-io/f1r3node-rust/blob/7b8865aaa49fdec69b5b3dea8a239c06b4d399b6/docs/plans/casper-node-interface-prerequisite.md) proposed twelve Batch B files.

The constructor and storage review found additional requirements. The twelve-file list does not cover bounded storage reads or complete runtime wiring.

Batch A was authorized and implemented. Its claim remains pending. The original plan's pending-confirmation statement no longer applies to Batch A.

The current continuation also corrects a Batch A shutdown defect. The [shutdown work log](../work-logs/casper-node-observer-shutdown-review.md) records that separate change.

## Source findings

These findings describe the reviewed source. They do not establish live candidate qualification or exhaustive publication-writer coverage.

| Source and symbol | Finding | Required boundary |
| --- | --- | --- |
| `casper/src/rust/casper.rs::hash_set_casper` | The constructor adopts threshold, parent depth, deploy lifespan, and minimum Phlo price from chain state. | Capture the running instance's adopted values and their source state. Do not substitute startup configuration. |
| `casper/src/rust/engine/casper_launch.rs::create_casper` | The direct startup path calls `hash_set_casper`. | Cover direct startup. |
| `casper/src/rust/engine/initializing.rs::create_casper_and_transition_to_running` | The joining path calls the same constructor before the running-state transition. | Cover delayed initialization. |
| `casper/src/rust/engine/genesis_ceremony_master.rs::create_casper_from_storage` | The ceremony path also calls the constructor. | Cover ceremony completion. |
| `casper/src/rust/engine/engine.rs::transition_to_running` | The transition installs the running engine through `EngineCell::set`. | An explicit observer handle can follow engine installation. |
| `casper/src/rust/engine/engine_cell.rs` | The cell currently owns only the engine. | Default construction must remain observer-free. |
| `casper/src/rust/engine/multi_parent_casper/dispatch.rs` | `last_finalized_block` invokes the real finalizer. | A diagnostic request must not call this method as a read-only query. |
| `casper/src/rust/engine/multi_parent_casper/snapshot.rs::compute_snapshot` | Snapshot construction runs floor evaluation and updates production caches. | A diagnostic request must not use this method as detached capture. |
| `block-storage/src/rust/dag/block_dag_key_value_storage.rs::get_representation_internal` | The representation shares metadata, floor, frontier, lifecycle, and carrier stores. | Copying the representation does not detach those stores. |
| `block-storage/src/rust/dag/block_dag_key_value_storage.rs::insert_internal` | The insertion generation changes during block insertion. | The generation is not a revision for every authority input. |
| `record_directly_finalized` and `propagate_ft_to_finalized_blocks` in the same file | These methods update finalization metadata without incrementing the insertion generation. | Equal insertion generations do not prove stable capture. |
| `KeyValueDagRepresentation::put_cached_floor` and `put_cached_frontier` | These methods write stores without the DAG global lock. | The global lock alone does not freeze cache rows. |
| `casper/src/rust/storage/rnode_key_value_store_manager.rs::rnode_db_mapping` | Blocks and DAG metadata occupy different LMDB environments. | One DAG transaction does not capture both environments. |
| `shared/src/rust/store/lmdb_key_value_store.rs` | Existing reads decode owned values before callers can inspect their lengths. | Observer limits need a bounded read path before allocation. |
| `shared/src/rust/store/key_value_typed_store_impl.rs` | Collection reads the complete underlying map. | Do not call collection before checking capture limits. |
| `block-storage/src/rust/key_value_block_store.rs::bytes_to_block_proto` | The decoder resizes a buffer using the encoded decompressed length. | Check compressed and decompressed limits before observer decoding. |
| `block-storage/src/rust/dag/block_metadata_store.rs::new` | Reconstruction converts collection failure into an empty input. | Observer construction must reject failure rather than certify empty state. |
| `casper/src/rust/finality/floor.rs::floor_of_block` | Evaluation reads and writes floor and frontier caches. | Each evaluation needs separate scratch stores. |
| `casper/src/rust/safety/clique_oracle.rs::ft_witnessed_exact` | Early returns can occur before clique calculation. | Uncomputed clique values remain absent, not zero. |
| `casper/src/rust/util/clique.rs::expand_max_weight` | Clique search recurses synchronously. | An asynchronous timeout alone does not bound this work. |

LMDB means Lightning Memory-Mapped Database. The storage manager uses separate environments for several durable stores.

The Batch A configuration digest has scope `batch-a-public-config-v1`. It does not identify the complete adopted authority configuration.

## Proposed next implementation: Batch B1

Batch B1 supplies bounded, detached storage data. It adds no socket operation, evaluator, finalizer observer, or live capability declaration.

This smaller batch establishes the consistency boundary before consensus-path instrumentation. Batch B2 remains a separate implementation approval.

### Capture contract

1. Require explicit limits for bytes, records, blocks, validators, edges, decode expansion, and capture work.
2. Reject invalid limits before store access.
3. Acquire the DAG read guard with a finite acquisition limit.
4. Open read transactions for every participating LMDB environment.
5. Record each transaction identity with its environment identity.
6. Read bounded raw values before typed decoding or decompression.
7. Copy the required DAG state while its existing guards remain held.
8. Recheck every participating environment after capture.
9. Reject capture if a transaction identity changed or a required input cannot be read.
10. Release all production guards and transactions before evaluation or response serialization.

The proposed consistency check requires monotonic transaction identities. It must establish a common interval for all accepted values.

Equal insertion generations, equal row counts, or two matching samples are insufficient. An unavailable transaction-identity check must return unsupported.

The implementation must verify the pinned LMDB library's transaction and thread rules. It must not add an unchecked assumption about overlapping transactions.

A changed environment causes one rejected capture. Batch B1 proposes no automatic retry loop and no pause of production writers.

This read contract does not make separate node writes atomic. A consistent read can expose an intermediate publication state.

The capture initially covers a complete held DAG within explicit limits. A larger DAG is unsupported until a separately specified bounded-window contract exists.

A block outside the held DAG differs from a missing row for a held block. Capture must retain that distinction.

A required read failure is incomplete evidence. It must not become an empty snapshot, a missing-block assertion, or a successful finality hold.

Canonical encoding must sort unordered collections and preserve ordered parent lists. The digest must cover availability states, cache seeds, schema, limits, and coverage.

Scratch construction must allocate new metadata, block, floor, and frontier stores. It must not retain a production store handle.

Lifecycle and carrier stores are outside this authority capture. Scratch construction must prevent their empty substitutes from becoming durable-work observations.

Native I/O latency remains a separate assumption. Operation bounds and deadline checks do not prove an operating-system scheduling deadline.

### Exact Batch B1 source and test files

| File | Proposed change |
| --- | --- |
| `shared/src/rust/store/soak_snapshot.rs` | Add read-only, bounded LMDB capture helpers and transaction-identity checks. |
| `shared/src/rust/store/mod.rs` | Register the capture helper. |
| `shared/tests/soak_snapshot.rs` | Test bounds, transaction changes, unsupported backends, and unchanged stored bytes. |
| `block-storage/src/rust/dag/soak_snapshot.rs` | Add detached data, canonical identity, availability states, and independent scratch construction. |
| `block-storage/src/rust/dag/mod.rs` | Register the detached snapshot module. |
| `block-storage/src/rust/dag/block_dag_key_value_storage.rs` | Expose the checked capture boundary under existing DAG guards. |
| `block-storage/src/rust/dag/block_metadata_store.rs` | Add narrow capture access and checked scratch construction. |
| `block-storage/src/rust/key_value_block_store.rs` | Add observer-only bounded block decoding and capture access. |
| `block-storage/tests/soak_snapshot.rs` | Test independence, concurrent changes, availability, limits, and malformed encodings. |

No dependency, lockfile, storage format, or production write-path change is proposed. Additional files require an updated plan.

The normal block decoder retains its existing behavior. Observer-specific limits must not become new block-validity rules.

### Batch B1 claim and attribute proposal

Register `CLAIM-CASPER-NODE-OBSERVATION-002` in `docs/claims/casper-node-authority-snapshot.md` before implementation. Keep its verification status pending.

The claim inventory must include all nine files above. Its scope is bounded detached capture, not authority equivalence or live qualification.

Propose `cbc=mandatory cbc-weight=high` for these four new files:

- `shared/src/rust/store/soak_snapshot.rs`
- `shared/tests/soak_snapshot.rs`
- `block-storage/src/rust/dag/soak_snapshot.rs`
- `block-storage/tests/soak_snapshot.rs`

These tags require human ratification. This review does not apply them.

The DAG storage file already has a mandatory tag. Its existing `CLAIM-FINALITY-002` carrier obligations remain in force.

The default per-artifact record lookup did not find a DAG storage record. A missing record must not become a waiver or discharge.

Create or reconcile pending records at these paths after approval:

- `docs/cbc-evidence/shared-src-rust-store-soak-snapshot-rs.md`
- `docs/cbc-evidence/shared-tests-soak-snapshot-rs.md`
- `docs/cbc-evidence/block-storage-src-rust-dag-soak-snapshot-rs.md`
- `docs/cbc-evidence/block-storage-tests-soak-snapshot-rs.md`
- `docs/cbc-evidence/block-storage-src-rust-dag-block-dag-key-value-storage-rs.md`

Retain prior evidence identities when a current record changes. Keep bulk evidence outside Git and compact run records under `docs/cbc-evidence/runs/`.

### Batch B1 acceptance checks

- Reject excessive raw lengths before allocation and excessive decompressed lengths before buffer growth.
- Reject malformed encodings and oversized internal collection lengths.
- Detect finalization and cache changes that leave the insertion generation unchanged.
- Reject unsupported storage backends instead of substituting independent reads.
- Reject changes between environment capture and validation, including a value changed back to its original bytes.
- Preserve captured bytes after live metadata, block, and cache mutations.
- Verify that two scratch views share no mutable stores.
- Preserve parent ordering and distinguish absent data from failed reads.
- Compare production store bytes before and after successful and rejected capture.
- Run existing DAG, carrier, block-store, and storage-backend regressions.

Passing these tests does not qualify the live authority profile. Source-bound verification and explicit acceptance remain separate gates.

## Batch B2 planning steps 1 through 4

The user authorized these planning steps on 2026-09-23.
This authorization permits planning before TASK-019-4 acceptance.
Implementation still requires accepted Batch A and B1 claims, the Step 5 claim record, and ratified tags.

The source review uses `cef1f4b721b8109019459f11c53d49df00eb68f9`.
Steps 1 through 4 are complete as a design.
The proposed implementation has not started.

### Step 1: Final implementation file list

This list defines the implementation boundary.
Changes outside this list require a revised scope.
Claim files, tags, and evidence records belong to Step 5 and the later verification step.

| File | Planned responsibility |
| --- | --- |
| `node/src/rust/runtime/node_runtime.rs` | Pass the enabled observer handle through startup and shutdown. |
| `node/src/rust/runtime/setup.rs` | Select ordinary or explicitly observed engine construction. |
| `node/src/rust/soak_observer.rs` | Add the authority request, bounded response, and attachment status. |
| `node/tests/soak_observer.rs` | Test protocol limits, unsupported states, shutdown, and request identity. |
| `casper/src/rust/mod.rs` | Register the observer module. |
| `casper/src/rust/soak_observer.rs` (new) | Define attachment, authority inputs, records, and the bounded event queue. |
| `casper/src/rust/soak_observer/evaluation.rs` (new) | Capture once and run the measured evaluation on scratch stores. |
| `casper/src/rust/soak_observer/reference.rs` (new) | Evaluate the independent floor and oracle reference. |
| `casper/src/rust/casper.rs` | Add the default unsupported trait method and initialize the empty attachment field. |
| `casper/src/rust/engine/engine_cell.rs` | Attach supported instances during engine installation and record coverage. |
| `casper/src/rust/engine/multi_parent_casper/types.rs` | Store the initially empty attachment cell. |
| `casper/src/rust/engine/multi_parent_casper/dispatch.rs` | Supply narrow capture access and adopted public authority inputs. |
| `casper/src/rust/engine/multi_parent_casper/finalization_runner.rs` | Emit bounded derivation and effect records without additional consensus work. |
| `casper/src/rust/finality/floor.rs` | Add metered evaluation entry points and charge floor, lineage, and signature operations. |
| `casper/src/rust/safety/clique_oracle.rs` | Expose measured exact and original results with metered oracle operations. |
| `casper/src/rust/util/clique.rs` | Bound synchronous clique search before expansion and allocation. |
| `shared/src/rust/dag/mod.rs` | Register the work meter. |
| `shared/src/rust/dag/observation_work.rs` (new) | Define checked work, memory, depth, and deadline accounting with a no-op implementation. |
| `block-storage/src/rust/dag/block_dag_key_value_storage.rs` | Add metered main-parent and DAG traversal variants for detached evaluation. |
| `block-storage/tests/soak_snapshot.rs` | Verify scratch traversal bounds and unchanged production storage. |
| `casper/tests/soak_observer.rs` (new) | Test attachment, reference comparison, counters, limits, and event loss. |
| `casper/tests/helper/test_node.rs` | Initialize the empty attachment field. |
| `casper/tests/batch1/multi_parent_casper_bonding_spec.rs` | Initialize the empty attachment field. |
| `casper/tests/api/pending_deploys_test.rs` | Initialize the empty attachment field. |
| `casper/tests/api/deploy_finalization_status_test.rs` | Initialize the empty attachment field. |
| `casper/tests/api/last_finalized_api_test.rs` | Initialize the empty attachment field. |
| `casper/tests/api/bonded_status_api_test.rs` | Initialize the empty attachment field. |

The structural search found one production constructor literal and the six fixture literals above.
The constructor routes are direct startup, delayed initialization, and genesis ceremony completion.
All routes reach `hash_set_casper` and engine installation.

The review also inspected `estimator.rs`, `util/dag_operations.rs`, and `shared/src/rust/dag/dag_ops.rs`.
B2 evaluates floor and oracle results directly. It does not call the estimator or claim a tips comparison.
Those estimator traversal files therefore require no B2 change.

The reachable storage traversals include `main_parent_chain`, `is_in_main_chain`, and `is_dag_ancestor`.
Their internal loops require metering. Counting their return values would miss work and allocation.

The proposed implementation adds no dependency, storage format, or production consensus limit.
Existing entry points use a generic no-op meter. Observer entry points use the checked meter.
The implementation must preserve existing results and errors when observation is disabled.

### Step 2: Attachment and coverage

The route is `start` → `NodeRuntime` → `setup_node_program` → `EngineCell` → the installed Casper instance.
Ordinary startup retains `EngineCell::init()` and creates no observer handle, queue, task, or capture endpoint.

Each Casper instance contains an empty `OnceLock<ObserverBinding>`.
An empty cell allocates no observer state.
The enabled observer creates one controller and supplies its handle to explicit engine construction.

The trait attachment method defaults to `Unsupported`.
The implementation exposes only the DAG store, block store, adopted public configuration, and approved block identity.
It exposes no validator identity, runtime manager, finalizer, production snapshot method, or unrestricted Casper reference.

The controller owns a narrow capture endpoint.
The endpoint contains storage handles and immutable public values, without a reference back to the Casper instance.
Detached results retain none of these live handles.

| Installation state | Observer result |
| --- | --- |
| Observation is disabled. | Install the engine without attachment work. |
| The engine has no Casper instance. | Install the engine and report `awaiting_casper`. |
| The instance supports attachment and its cell is empty. | Attach once and start a new coverage interval. |
| The instance already has an attachment, including the same handle. | Reject the second attachment and mark the observer unavailable for that installation. |
| The trait implementation is unsupported. | Install the engine and report `unsupported`. |
| Engine replacement removes or changes the instance. | Close the old interval and select only the new endpoint. |

Attachment occurs in the existing engine installation critical section.
Attachment performs bounded bookkeeping only.
It does not read stores, await queue capacity, serialize responses, or invoke application callbacks.

Attachment failure affects observer availability, not engine installation or consensus.
The installation result records the attachment failure.
It does not report an earlier interval as coverage for the new engine.

Each interval names the process incarnation, instance identifier, installation sequence, and attachment time.
Requests bind to one interval.
Replacement during capture makes the request incomplete, with `instance_changed`.

Successful attachment defines the earliest observed event.
Startup time and earlier finalization events remain outside coverage.
Shutdown closes the interval and cancels observer work.

The event queue uses fixed capacity and nonblocking insertion.
Each attempted insertion consumes a sequence number.
Overflow increments a loss counter and marks the interval incomplete.
Counter overflow also ends complete coverage.

### Step 3: Detached evaluation and independent reference

The new operation is `authority_snapshot`.
Its request contains explicit capture limits, evaluation limits, target hashes, requested body hashes, and requested comparisons.
Existing peer, incarnation, challenge, executable, configuration, session, and frame checks remain required.

The authority input includes the adopted integer threshold, parent depth, deploy lifespan, minimum Phlo price, and approved block hash.
The input also names the approved post-state hash from which the constructor adopted those values.
Startup configuration cannot replace adopted values.

The authority digest uses a new versioned scope.
It includes the B1 snapshot digest, adopted values, approved state identity, request selections, and evaluation limits.
The Batch A public configuration digest remains a separate identity.

1. Validate the request and its limits before accessing stores.
2. Select one attached instance and record its coverage interval.
3. Run one B1 capture with the explicit body selection.
4. Reject incomplete capture or an instance change.
5. Release all production guards before constructing scratch views.
6. Create separate scratch views for exact evaluation and original fault-tolerance evaluation.
7. Run the measured floor and oracle paths with checked work meters.
8. Run the independent reference against immutable captured data.
9. Compare only results with the same authority input digest and complete required inputs.
10. Serialize the bounded result after all production guards are released.

Evaluation never requests missing blocks from peers or reads live stores to fill a gap.
Missing required body coverage makes the dependent result unavailable.
A captured absent body and a body excluded from the request have different reasons.

The measured floor entry point is `floor_of_view`.
It uses the captured last finalized block and captured latest-message map.
Per-block floor requests instead use that block's ordered parents and signed justifications.
These are different input scopes and receive different evaluation digests.

The exact oracle uses `ft_witnessed_exact` with the adopted integer threshold.
The original result uses `ft_witnessed` with the same explicit latest-message map on a separate scratch view.
The oracle committee comes from the target's main parent, or from the target when it has no parent.

#### Reference algorithm

The reference reads immutable maps and bodies directly.
It calls no production estimator, floor, oracle, clique, or traversal helper.
Shared types, canonical encoding, and the work meter do not supply consensus decisions.

1. Validate held metadata, parent order, referenced heights, and the required body coverage.
2. Construct independent main-parent, state-parent, and justification maps.
3. Derive each target's committee from the captured metadata.
4. Determine agreeing validators through independent main-parent membership checks.
5. Apply both directional justification tests for each candidate validator pair.
6. Enumerate validator subsets and select the maximum-weight clique.
7. Evaluate the exact threshold with checked integer arithmetic.
8. Derive per-block floors in increasing parent order.
9. Search each parent's main chain from its head to find its highest witnessed frontier.
10. Combine frontiers with inherited floors and apply the state-containment rules.
11. Apply the captured current-floor advancement and hold rules for a view request.

The directional tests walk self-justifications with the source algorithm's exclusive stopping boundary.
They preserve missing-history behavior and both validator directions.
The subset search includes isolated singleton cliques.
It uses neither Bron–Kerbosch search nor its pruning or caches.

The exact decision first requires positive total stake and a strict agreeing majority.
It then evaluates `2*q*den >= S*(den+num)` for floor decisions.
The record preserves `gt` separately when a caller explicitly requests strict comparison.
Arithmetic overflow yields an unavailable result, never a wrapped decision.

The floor reference derives state parents from recorded merge bases or a sole parent.
A multi-parent block without a recorded merge base is not a guessed lineage.
Containment uses nonfailed deploy signatures and `applied_from_scope` signatures above the state-lineage meet.

Candidate ordering uses descending height and descending hash.
Each inherited floor requires containment or the existing common-parent and pure-cut alternative.
The reference preserves that alternative as source behavior. It does not claim a new proof of its safety.

For view evaluation, the reference checks validator ownership of latest-message testimony.
It reproduces undecidable-tip filtering and the threshold-dependent hold behavior.
Advance, no advance, containment hold, absence hold, and incompatibility remain distinct outcomes.

#### Cache and input limits

B1 records floor and frontier values without restore-seed provenance.
The measured path preserves all captured rows.
The reference ignores optimization caches only when all required ancestry reaches a captured genesis.

A truncated history can require immutable restore seeds.
Without independently identified seeds, the reference reports `restore_seed_provenance_unavailable`.
It must not remove required seeds or use an unexplained cached answer as independent evidence.

The API display value has another input gap.
The API subtracts normalized initial fault using the live equivocation tracker.
B1 does not capture that tracker.
B2 therefore reports `equivocation_snapshot_unavailable` for the display projection.

Persisted fault tolerance from finalized metadata can still be reported as its own captured value.
It cannot replace the display projection or the original oracle result.
Adding a complete display capture or restore-seed contract requires a separate scope review.

#### Work bounds

Every request supplies positive limits.
The following ceilings are proposed for the observer only.
They are design limits for review, not changes to consensus configuration.

| Resource | Proposed ceiling |
| --- | --- |
| Captured blocks / validators / edges | 4,096 / 64 / 65,536. |
| Captured raw bytes / decompressed body bytes | 64 MiB total / 8 MiB per body. |
| Evaluation allocation accounting | 256 MiB across all scratch views and reference data. |
| Evaluation work | 2,000,000 charged operations across all paths. |
| Clique search expansions / recursion depth | 100,000 / 64. |
| Exhaustive reference committee | 16 validators. |
| Event queue / maximum encoded event | 256 records / 4 KiB. |
| Response frame | The existing 1 MiB maximum. |
| Request deadline | The remaining session deadline, with a maximum of 30 seconds. |

All smaller request limits apply.
Capture retains its existing read, decode, lock, and work limits.
Checked arithmetic must validate aggregate sizes before copying or allocating scratch data.

Metered loops check work and deadline before each operation.
Clique metering covers graph construction, expansion, pivot scans, intersections, candidate copies, and recursion.
Traversal metering covers each visited node, examined edge, metadata access, lineage step, and signature-set operation.

A generic no-op meter preserves ordinary execution.
The observer meter can stop synchronous work directly.
An asynchronous timeout alone cannot stop synchronous clique search.
A detached blocking task that continues after timeout is unacceptable.

One evaluation can run per observer.
An overlapping request receives `busy` without an unbounded waiting queue.
Budget exhaustion produces partial counters and an unavailable result.
Allocation accounting bounds charged payloads and collection capacity, not total process memory or operating-system latency.

### Step 4: Record schema and evidence meaning

Each result uses an explicit availability union: `available`, `unavailable`, `failed`, or `not_requested`.
An unavailable result has a reason and no fabricated value.
A result names the input digest actually used, or an explicit reason why that digest is absent.

| Field | Required contents |
| --- | --- |
| `identity` | Source revision, executable digest, process incarnation, instance identifier, and request identity. |
| `coverage` | Attachment interval, sequence range, loss count, and completeness. |
| `inputs` | B1 digest, authority digest, evaluation digest, input scope, limits, and capture transaction identities. |
| `oracle_decision` | Boolean decision, comparator, exact threshold numerator and denominator, and input digest. |
| `oracle_witness` | Total stake, agreeing stake, clique weight, and early-return reason with separate availability states. |
| `original_fault_tolerance` | Original `f32` bits, algorithm identifier, explicit latest-message scope, and input digest. |
| `display_projection` | Projected `f32` bits, persisted-or-live source, initial-fault input, and input digest when available. |
| `persisted_fault_tolerance` | The metadata value and capture digest, without a claim that it came from this oracle evaluation. |
| `floor_result` | Typed floor outcome, target, current floor, derived floor, and input digest. |
| `reference_comparison` | Reference algorithm version, compared input digest, both outcomes, and match, mismatch, or unavailable. |
| `work` | Per-path traversal, oracle, clique, signature, allocation, and budget counters with availability and completeness. |
| `event` | Event kind, sequence, operation correlation when known, and outcome. |

An exact early return can leave clique weight uncomputed.
That weight is unavailable, not zero.
A measured zero counter means instrumentation was active and observed no operations.
A missing hook means unavailable.

Counters increment at the operation sites identified in Step 1.
They do not estimate work from output sizes or elapsed time.
Exact, original, and reference evaluations retain separate counter sets and one aggregate budget.

Floating-point bit patterns preserve sentinels and exceptional values.
An optional numeric rendering does not replace those bits.
The exact decision is never reconstructed from the display projection.

#### Event distinctions

| Event kind | Evidence boundary |
| --- | --- |
| `detached_derivation` | A result from scratch evaluation. It applies no production effect. |
| `live_derivation` | A floor outcome already computed by the live finalizer after attachment. |
| `effect_attempt` | The live finalizer is about to request its existing finalization effect. |
| `effect_return` | The effect call returned success or failure. Partial persistence can precede a failure. |
| `persisted_observation` | B1 capture observed finalized metadata in its recorded transaction interval. |

Live hooks enqueue fixed-size facts from existing local values.
They perform no capture, serialization, disk access, blocking send, or extra oracle call on the consensus path.
Uncaptured live inputs have an unavailable snapshot digest.

An effect callback and a published event do not prove metadata persistence.
The storage path can persist completed effects before it returns an error.
A later capture records the persisted state separately, without assigning unsupported causation to an earlier attempt.

Dropped records make the affected interval incomplete.
Later records expose the sequence gap and loss count.
The consumer cannot turn missing events into successful effects.

### Implementation and verification gate

Step 5 remains pending.
It must register `CLAIM-CASPER-NODE-OBSERVATION-003`, ratify mandatory tags, and create pending artifact records before implementation.

The implementation checks must cover every attachment state, all three constructor routes, limits, event loss, and unchanged production bytes.
Reference tests must retain disagreements from incorrect threshold, traversal, clique, cache, and containment controls.
Uninstrumented counters and unavailable display or restore inputs must remain unavailable in those tests.

B2 does not qualify a live authority profile while required inputs remain unavailable.
Batch A and B1 acceptance remains under TASK-019-4.
This planning authorization does not accept either claim or authorize B2 implementation.

## Batch C and campaign boundaries

The cross-environment storage finding also constrains Batch C.
The [Batch C research](casper-node-observation-batch-c.md) records the writer inventory and publication consistency contract.
Maintainer review and implementation approval remain pending.

Do not infer occurrence identity from deploy signatures. Actual PR #216 merge remains necessary for occurrence-level recovery qualification.

This planning update changes no node capability, campaign guard, candidate image, workload pin, or cloud budget.

Merge, image publication, candidate repinning, preflight, full baselines, and later stability approval remain separate actions.

## Batch B1 confirmation record

The user confirmed the nine-file Batch B1 scope, the pending claim, and the four high-weight mandatory tags on 2026-09-21.

The [Batch B1 work log](../work-logs/casper-node-observation-batch-b1.md) records the implementation, the local results, and the registered claim `CLAIM-CASPER-NODE-OBSERVATION-002`.

The B1 confirmation does not authorize B2 implementation, Batch C implementation, merges, claim acceptance, or cloud launches. Commits and pushes require their separate consent.

## B1 continuation review

The review at `2ccc4ae0ac1045232c247ecd76925e13fabd0ade` found reader-budget and decompression defects. Local corrections address those defects within the nine-file B1 scope.

The [continuation work log](../work-logs/casper-node-observation-batch-b1.md#continuation-review-on-2026-09-21) records failing controls, passing regressions, and unresolved findings.

The [B1 completion review](../work-logs/casper-node-observation-batch-b1.md#b1-completion-review) records subsequent corrections for nested bounds, work accounting, scratch blocks, and canonical identity.

Formal claim acceptance remains pending. Do not use the current capture as an accepted B2 prerequisite.

The earlier local test counts do not establish those properties. This review does not approve B2 or C, discharge the capture claim, or qualify a live adapter.
