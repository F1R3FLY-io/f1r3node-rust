# Casper Node Observation: Batch B Review

**Status:** Batch B1 confirmed and implemented locally on 2026-09-21. Its claim remains pending. Batch B2 and Batch C still require confirmation.

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

## Batch B2 constructor and trait review

The proposed handle route is `start` to `NodeRuntime`, then `setup_node_program`, then an explicitly configured `EngineCell`.

At engine installation, the cell would attach a narrow observer handle to supported Casper instances. Unsupported trait implementations must remain unsupported.

The default `EngineCell::init` must allocate no observer state. Attachment must not invoke the finalizer, compute a production snapshot, or request validator identity.

An instance must reject conflicting handle attachment. Its observation coverage begins at successful attachment, not at an invented earlier startup time.

The production instance needs a disabled observer field. The constructor review found six explicit test fixtures that would also need that field:

- `casper/tests/helper/test_node.rs`
- `casper/tests/batch1/multi_parent_casper_bonding_spec.rs`
- `casper/tests/api/pending_deploys_test.rs`
- `casper/tests/api/deploy_finalization_status_test.rs`
- `casper/tests/api/last_finalized_api_test.rs`
- `casper/tests/api/bonded_status_api_test.rs`

The workspace structural search also found the constructor literal in `casper/src/rust/casper.rs`.

The proposed wiring extends beyond the original Batch B list:

- `node/src/rust/runtime/node_runtime.rs`
- `node/src/rust/runtime/setup.rs`
- `casper/src/rust/engine/engine_cell.rs`
- `casper/src/rust/casper.rs`
- `casper/src/rust/engine/multi_parent_casper/types.rs`
- `casper/src/rust/engine/multi_parent_casper/dispatch.rs`
- The six fixture files above.

Source navigation returned no language-server references for the production type. Structural searches supplied the constructor inventory, not proof of exhaustive semantic coverage.

### Evaluation requirements before Batch B2 approval

A reference path must differ from the measured path. Repeating `tips_with_latest_messages` does not supply independent reference coverage.

The floor comparison must distinguish immutable restore seeds from optimization caches. Removing a required restore seed would change the input.

The exact oracle decision, original fault-tolerance result, and display projection require separate fields. Each value must identify its actual input snapshot.

Counters must increment at actual traversal and clique-search operations. Missing counters must remain unavailable.

The source review adds `casper/src/rust/util/clique.rs` to the work-bound review. Traversal helpers require equivalent inspection before the final instrumentation list.

A live finalizer decision is not a detached comparison result. A derivation, an attempted effect, and persisted finalization require distinct records.

Record overflow or observer failure must not block consensus or invent a successful observation. The affected observation must be incomplete.

Batch B2 needs a final file list, limits, reference algorithm, claim changes, and tests after Batch B1 review. This document does not authorize Batch B2.

## Batch C and campaign boundaries

The cross-environment storage finding also constrains Batch C. Batch C still needs the complete writer inventory and publication consistency contract.

Do not infer occurrence identity from deploy signatures. Actual PR #216 merge remains necessary for occurrence-level recovery qualification.

No node capability, campaign guard, candidate image, workload pin, or cloud budget changes in this proposal.

Merge, image publication, candidate repinning, preflight, full baselines, and later stability approval remain separate actions.

## Batch B1 confirmation record

The user confirmed the nine-file Batch B1 scope, the pending claim, and the four high-weight mandatory tags on 2026-09-21.

The [Batch B1 work log](../work-logs/casper-node-observation-batch-b1.md) records the implementation, the local results, and the registered claim `CLAIM-CASPER-NODE-OBSERVATION-002`.

The confirmation does not authorize Batch B2, Batch C, merges, claim acceptance, or cloud launches. Commits and pushes require their separate consent.
