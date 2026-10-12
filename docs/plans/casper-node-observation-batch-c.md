# Batch C: publication writers and consistency contract

**Status:** TASK-019-5 research is complete. Maintainer review and Batch C implementation approval remain pending.

**Research date:** 2026-09-23. The user authorized this research before completion of TASK-019-3.

**Source revision:** `03d7f1b27544b2c5a93b664d8684b24ea16cf3e9` on `feature/casper-node-observation`.
The comparison revision is `dev` at `6d6d4fed6f84baa0913d8e87f32a7ffe2e0ca59a`.
The concurrent edit to `shared/src/rust/store/soak_snapshot.rs` is outside this writer inventory.

This document records source inspection, not executed crash qualification or an accepted correctness claim.
It supplies the writer inventory, intermediate states, and proposed observation contract for implementation review.

## Scope and evidence

The inventory includes every named database in the production block and directed acyclic graph (DAG) environments.
It also includes deploy storage, the dependency buffer, and runtime state needed to interpret publication after restart.
Test fixtures, detached scratch stores, and in-memory stores do not establish production durability.

The inventory follows database registration, store mutators, and their production callers.
The [store mapping](../../casper/src/rust/storage/rnode_key_value_store_manager.rs) defines the environment boundaries.
The [LMDB backend](../../shared/src/rust/store/lmdb_key_value_store.rs) defines the transaction boundaries.
The [typed adapter](../../shared/src/rust/store/key_value_typed_store_impl.rs) supplies serialization and typed operations.
LMDB means Lightning Memory-Mapped Database.

Each backend `put`, `delete`, or `put_one_if_absent` call commits its own write transaction.
A batch inside one call can be atomic without making adjacent calls atomic.
`batched_put` can combine stores only when their LMDB environment handles match.
Its fallback writes stores separately. This revision has no production caller of that helper.
Transaction completion alone does not establish durability after host power loss.

## Complete block and DAG store inventory

The following table covers all 15 registered databases in `blockstorage` and `dagstorage`.
Rows identify logical keys, durable mutators, callers, and relevant ordering.
Source references below the table resolve each writer family.

| Database and environment | Key and value | Writer and production path | Boundary and possible intermediate state |
|---|---|---|---|
| `blocks`, `blockstorage` | Block hash to compressed block body. | `KeyValueBlockStore::put` and `put_block_message`. Receive, propose, genesis, and restore paths write bodies. | One body transaction precedes admission on normal receive and propose paths. A stored body does not prove admission. |
| `blocks-approved`, `dagstorage` | Key `[42]` to approved block. | `put_approved_block`. Genesis initialization and approved-state restore publish this register. | A separate transaction updates the approved register. Restore has its own ordering and completion checks. |
| `block-metadata`, `dagstorage` | Block hash to metadata and finalization fields. | Admission and finalization use `BlockMetadataStore::add`, `record_finalized`, and `propagate_ft_to_finalized_blocks`. Approved insertion and legacy migration also write metadata. | Addition follows carrier and event writes. Finalization batches flags separately from fault-tolerance updates, and memory can change before persistence succeeds. |
| `latest-messages`, `dagstorage` | Validator to latest hash. | `BlockDagKeyValueStorage::insert_internal`. Normal and invalid insertion update sender entries and applicable newly bonded validators. | The latest-message batch follows metadata. `SettledHistory` insertion skips this update. |
| `invalid-blocks`, `dagstorage` | Block hash to invalid metadata. | `insert_internal` through invalid-block admission. | The invalid row follows metadata and precedes latest-message updates. |
| `equivocation-tracker`, `dagstorage` | Validator and sequence to detected block hashes. | `EquivocationTrackerStore::add` through `access_equivocations_tracker`. Equivocation detection and validation dispatch write rows. | The DAG write guard protects these production updates. Each store call commits separately from block admission. |
| `floor-index`, `dagstorage` | Block hash to cached floor hash. | `put_cached_floor`. `floor_of_block`, restore cache import, and approved-state seed installation write rows. | These writes do not require the global DAG write guard. Floor and frontier updates use separate calls. |
| `frontier-index`, `dagstorage` | Block hash to cached finalized frontier. | `put_cached_frontier`. Floor derivation and restore cache import write rows. | A floor row can exist before its frontier row. Cache presence does not establish restore completion. |
| `deploy-lifecycle-events`, `dagstorage` | Deploy signature to validity data and open lifecycle events. | `append_event_once` and `append_events` during valid insertion. Terminal publication deletes the open row. | Valid insertion writes events before metadata, while invalid insertion records no lifecycle events. Event deletion follows a separate terminal-record transaction. |
| `deploy-lifecycle-terminal`, `dagstorage` | Deploy signature to frozen terminal verdict. | `DeployLifecycleStore::put_terminal_if_absent` through `write_terminal` and `observe_block`. | The lifecycle write guard serializes callers. A terminal row can coexist with an undeleted open event row. |
| `carrier-index`, `dagstorage` | Deploy signature to carrier heights and hashes. | `CarrierIndex::record_once` during insertion, including invalid and settled blocks. `prune_below` runs through lifecycle observation. | Insertion precedes metadata. Pruning rewrites or deletes individual rows before it advances the prune cursor. |
| `carrier-index-meta`, `dagstorage` | Coverage watermark and prune cursor. | `ensure_carrier_watermark`, `record_carrier_coverage_from`, and `prune_below`. Startup, genesis, restore, and lifecycle observation write metadata. | A watermark records coverage, not a backfill. Existing DAG data can predate coverage. |
| `last-finalized-block`, `dagstorage` | Legacy key `1` to a hash or migration marker. | `LastFinalizedKeyValueStorage::put` and `migrate_lfb` during startup. | Migration updates metadata in chunks before writing the completion marker. Current finalization derives its state from metadata. |
| `genesis-hash`, `dagstorage` | Key `genesis` to shard genesis hash. | `record_genesis_hash` during genesis and truncated restore. | The guarded register accepts the same value again and rejects a different value. It is separate from approved-state publication. |
| `mergeable-channel-cache`, `dagstorage` | State hash, creator, and sequence to mergeable-channel data. | `RuntimeManager::save_mergeable_channels`, `put_mergeable_entry_bytes`, and `delete_mergeable_channels`. Execution, replay, restore, and garbage collection write this cache. | Checkpoint and mergeable-cache writes have separate commits. `ensure_mergeable_entry` can replay and write when an entry is absent. |

Writer source references:

- [Block store](../../block-storage/src/rust/key_value_block_store.rs), [block processor](../../casper/src/rust/blocks/block_processor.rs), and [proposer](../../casper/src/rust/blocks/proposer/proposer.rs).
- [DAG storage](../../block-storage/src/rust/dag/block_dag_key_value_storage.rs) and [metadata store](../../block-storage/src/rust/dag/block_metadata_store.rs).
- [Equivocation store](../../block-storage/src/rust/dag/equivocation_tracker_store.rs), [detector](../../casper/src/rust/equivocation_detector.rs), and [validation dispatch](../../casper/src/rust/engine/multi_parent_casper/validation_dispatcher.rs).
- [Floor derivation](../../casper/src/rust/finality/floor.rs), [lifecycle store](../../block-storage/src/rust/dag/deploy_lifecycle_types.rs), [lifecycle evaluation](../../casper/src/rust/finality/deploy_lifecycle.rs), and [carrier index](../../block-storage/src/rust/dag/carrier_index.rs).
- [Legacy migration](../../block-storage/src/rust/finality/last_finalized_key_value_storage.rs) and [startup setup](../../node/src/rust/runtime/setup.rs).
- [Genesis engine](../../casper/src/rust/engine/engine.rs), [restore engine](../../casper/src/rust/engine/initializing.rs), and [block restore](../../casper/src/rust/engine/lfs_block_requester.rs).
- [Runtime manager](../../casper/src/rust/util/rholang/runtime_manager.rs) and [mergeable-cache collection](../../casper/src/rust/util/mergeable_channels_gc.rs).

No production block-body deletion route was found in this revision.
The exposed `EquivocationTrackerStore::add_all` operation also writes a batch, but the reviewed production paths use `add`.
Future callers of public store handles require inventory renewal.

## Adjacent durable writers

| Database or state | Writer and caller | Publication relevance |
|---|---|---|
| `deploy_storage`, `deploystorage` | `KeyValueDeployStorage::add`, `add_if_absent`, `remove`, and `remove_by_sig`. Admission reserves a signature. Lifecycle and proposer cleanup remove signatures. | Pool membership has several writers. A missing pool row alone does not prove terminal publication. |
| `rejected_deploy_buffer`, `deploystorage` | `KeyValueRejectedDeployBuffer::add`, `remove`, and `remove_by_sig`. Parent-state computation buffers recoverable owner-held deploys. Proposer expiry, settlement, and refund cleanup remove entries. | The buffer stores signed deploys by signature. Separate pool and buffer operations can leave intermediate membership states. |
| `parents-map`, `casperbuffer` | `add_relation`, `put_pendant`, `remove`, and `enforce_limits`. Receive, admission, startup reconciliation, and buffer limits change dependency rows. | Some removal paths update parents before deleting rows. Admission uses process locks across separate DAG and buffer transactions. |
| `rspace-cold`, `rspace/cold` | History checkpoints write leaf data. State import writes raw data items. | Leaf data can persist before history and root publication. |
| `rspace-history`, `rspace/history` | Radix-tree `commit` writes history nodes. State import writes history items. | A checkpoint or restore can persist an incomplete sequence across environments. |
| `rspace-roots`, `rspace/history` | `RootsStore::record_root` writes the root entry, then the current-root pointer. Root reset, initialization, and state import can change the pointer. | Separate calls remain separate transactions even when history and roots share an environment. |

The legacy configuration separates history, roots, and cold data into `rspace/casper/v2` environments.
It also registers `rspace-channels`. No production caller of that legacy database name was found.
An implementation must report the actual environment layout and reject an unsupported legacy configuration.

The evaluator databases, reporting cache, and transaction index do not supply the consensus publication tuple defined here.
They remain outside this contract. Their presence cannot substitute for missing consensus storage evidence.

Writer source references:

- [Deploy store](../../block-storage/src/rust/deploy/key_value_deploy_storage.rs) and [rejected buffer](../../block-storage/src/rust/deploy/key_value_rejected_deploy_buffer.rs).
- [Admission](../../casper/src/rust/engine/multi_parent_casper/block_admission.rs) and [block creation](../../casper/src/rust/blocks/proposer/block_creator.rs).
- [Parent-state computation](../../casper/src/rust/util/rholang/interpreter_util.rs), especially `compute_parents_post_state` and its guarded `add` call.
- [Dependency buffer](../../block-storage/src/rust/casperbuffer/casper_buffer_key_value_storage.rs), [combined admission](../../block-storage/src/rust/dag/buffer_dag_transition.rs), and [startup reconciliation](../../casper/src/rust/engine/casper_launch.rs).
- [History repository](../../rspace++/src/rspace/history/history_repository_impl.rs), [root store](../../rspace++/src/rspace/history/roots_store.rs), and [radix tree](../../rspace++/src/rspace/history/radix_tree.rs).
- [State importer](../../rspace++/src/rspace/state/instances/rspace_importer_store.rs), [tuple-space restore](../../casper/src/rust/engine/lfs_tuple_space_requester.rs), and [horizon restore](../../casper/src/rust/engine/lfs_horizon_requester.rs).

## Publication sequences and exposed states

### Normal block admission

The receive and propose paths store the body before DAG admission.
`insert` takes the global DAG write guard. The combined admission helper takes that guard and the dependency-buffer guard.
The public `insert_internal` method relies on its caller for that guard.

The durable sequence inside insertion is:

1. Write carrier rows for body deploys.
2. Write lifecycle events for a valid block.
3. Write block metadata.
4. Write the invalid row when applicable.
5. Write latest messages unless the mode is `SettledHistory`.
6. Write finalization flags when the mode is `Approved`.

Each list item can contain multiple store calls.
The DAG generation increments after metadata insertion. It does not identify every store mutation or survive process restart.
The combined admission helper then removes dependency-buffer state.
The helper name `atomic_insert_then_buffer` describes process exclusion, not a transaction across both environments.

Valid admission subsequently evaluates lifecycle state, removes newly terminal signatures from the deploy pool, publishes an event, and updates finalization.
A duplicate metadata check can return without replaying the earlier insertion sequence.
An interrupted prefix therefore needs explicit recovery evidence. Re-insertion alone does not establish repair.

### Finalization and lifecycle

`record_directly_finalized` discovers pending ancestors under the DAG guard, then runs its effect callback outside that guard.
Completed callback rounds enter `effect_applied`. Finalization later persists the selected metadata rows and separately propagates fault tolerance.
Callback failure or the retry limit can follow durable writes for earlier completed rounds.
A callback can also perform some effects before it returns an error.
No durable receipt in this path proves that each callback effect ran exactly once.

The production callback removes in-memory block-index entries and publishes finalization logs and events.
Those effects are distinct from durable lifecycle terminal records and deploy-pool removal.
The lifecycle schedule rebuilds from open event rows after restart.
A terminal write and deletion of its open event row remain separate transactions.

Proposer cleanup has additional removal routes, including expiry, settled recovery, selected recovered deploys, and refund failure.
Selected recovered deploys can leave the pool while their rejected-buffer copy remains.
Parent-state computation can add rejected-buffer entries before the resulting proposal is published.
It logs some missing-carrier and buffer-write failures without establishing successful buffer custody.

### Genesis, restore, and startup

Genesis initialization writes a body, inserts approved DAG metadata, and writes the approved register.
The restore handler can insert approved DAG metadata before it requests the approved runtime state.
Restore also imports bodies, mergeable entries, floor seeds, history nodes, leaf data, and roots through separate calls.
A present approved DAG row therefore does not prove that restore completed.

Startup can migrate the legacy finalization register, initialize carrier coverage, reconcile dependency-buffer entries, and rebuild lifecycle schedules.
These actions are writers or derived-state rebuilds. They are not a general publication recovery transaction.
`BlockMetadataStore::new` can replace a failed metadata read with an empty in-memory state after logging a warning.
Raw database evidence and in-memory state must remain separate in a recovery report.

## Proposed consistency contract

### Meaning of a successful observation

A successful observation identifies one node incarnation, source revision, configuration, request, and bounded capture interval.
It records every included database, environment identity, transaction identity, key range, and completeness result.
It separates in-memory values from raw durable rows.
It reports truncation, missing bodies, invalid decoding, unsupported stores, and changed environments as incomplete or unsupported results.
It never replaces those failures with an empty successful inventory.

A consistent read can expose a committed prefix of a publication sequence.
For example, it can contain a body without metadata, a terminal row with open events, or metadata without a later latest-message update.
Other writer routes can interleave with that prefix.
The observed tuple must retain the writer route and sequence context before a checker attributes an inconsistency to a failure.

The existing B1 capture does not meet the complete Batch C inventory.
B1 process guards also do not cover every writer in this document.
A future live capture needs complete writer exclusion or a validated transaction-identity interval across all included environments.
Transaction identities from different environments must not be treated as one shared sequence.
A detached copy alone does not prove that its source rows came from one stable interval.

A stable interval establishes read consistency. It does not establish atomic publication or a complete effect.
Fault tolerance in metadata is a stored projection, not an original finalization certificate.
The consistency result must preserve this distinction.

### Meaning of recovery evidence

Recovery evidence includes raw inventories before termination and after restart.
The external controller records process termination, the old and new incarnation identities, and the exact data directory used.
Where required, a stopped-store inspection precedes startup writers and supplies the raw crash state.
The first live observation after restart cannot by itself separate crash residue from startup repair.

The publication tuple includes body presence, metadata, latest messages, invalid rows, floor and frontier data, and approved or genesis registers.
It also includes equivocations, lifecycle rows, carrier coverage, deploy custody, dependency state, mergeable data, and the referenced runtime state closure.
The tuple uses each block's declared post-state hash.
The mutable current-root pointer cannot substitute for that hash because execution, replay, and restore can change the pointer.

The observer reads existing state without replay, cache population, repair, finalization, or deploy removal.
Missing state closure produces an explicit incomplete result.
The observer must not call read-like helpers that create roots or reconstruct mergeable entries.
An optional transport journal records evidence delivery only. It never becomes consensus authority or a recovery ledger.

### Proposed fault boundaries

The following boundaries require implementation review and exact source binding before use.
They are candidates, not installed hooks or approved crash tests.

| Boundary family | Candidate pause point | Evidence required |
|---|---|---|
| Block admission | After body commit and between carrier, event, metadata, invalid-row, and latest-message commits. | The block, writer route, attempt identity, exact completed calls, and held guards. |
| Dependency cleanup | After DAG insertion and before or after dependency-buffer removal. | Both environment identities and the raw dependency rows. |
| Terminal publication | After terminal commit, before event deletion, and before or after pool removal. | The signature, terminal bytes, remaining event bytes, and custody rows. |
| Finalization | Before and after callback execution, metadata commit, and fault-tolerance propagation. | The finalized set and completed effect identities, when production supplies them. |
| Runtime state | Between cold-data, history, root-entry, current-root, and mergeable-cache commits. | The referenced state hash and completed raw-store writes. |
| Restore and startup | Between migration chunks, approved metadata, state import, cache seeds, coverage writes, and completion. | The restore or migration phase and exact raw-store coverage. |

A fault request arms one bounded attempt for one node incarnation and deadline.
The node emits a reached receipt only when execution reaches the selected source boundary.
The controller must retain that receipt before it terminates the process.
An arm acknowledgment does not prove that execution reached the boundary.
An external request label does not create a durable production effect identity.

A bounded pause lease releases execution after timeout or controller loss.
Transport work must not introduce an unbounded wait while a production guard is held.
Implementation review must specify receipt delivery, lease release, cancellation, and lock order for each proposed hook.
A read-only observer permission must never arm a fault or terminate a process.
The external controller owns termination and restart proof.

## Unsupported capability and decision gates

The reviewed source has no general publication ledger, durable attempt receipt, or exact occurrence store.
It has no installed Batch C fault hooks.
The observer must return unsupported when a requested boundary or effect identity has no production representation.
It must not invent a ledger or change transaction boundaries to satisfy a qualification check.

[PR #216](https://github.com/F1R3FLY-io/f1r3node-rust/pull/216) was open during the research check.
Its head was `619beb4a4a7ad3f8967d4586daf0f5c552bd150e`, with target `dev` and no merge revision.
Occurrence-level recovery qualification therefore remains blocked.
Deploy signatures identify current custody and lifecycle rows. They do not identify exact retry occurrences or tombstones.
Carrier lists and rejection records cannot substitute for an implemented occurrence store.

[Decision D-07](../casper/design/decision-ledger/07-deploy-recovery-custody.md) records the unresolved difference between its ratification language and absent occurrence storage.
The ratifiers must resolve that difference before an occurrence contract can claim protocol authority.
After the dependency merges, the inventory must follow actual occurrence writers and keys at the merged revision.
The PR title or signature-level tests cannot establish those semantics.

[Decision D-05](../casper/design/decision-ledger/05-finalization-publication.md) supplies publication requirements.
The source sequences above do not establish all five D-05 invariants.
In particular, process locks do not prove restart atomicity, and current callback bookkeeping does not prove exactly-once effects.
These findings constrain qualification. They do not authorize protocol changes or a new recovery algorithm.

## Review and future validation

TASK-019-5 ends with this research artifact and its tracker entry.
Named maintainer review of the inventory and contract must precede Batch C implementation approval.
TASK-019-3 remains the B2 implementation dependency. TASK-019-4 retains ownership of the existing observation claims.

An implementation proposal must name every changed writer, capture adapter, hook, permission, and evidence field.
It must retain bounded row, byte, time, and work limits without treating those limits as proof of complete publication.
Its validation must cover successful and failed writes, concurrent writers, partial terminal publication, callback errors, interrupted restore, and startup migration.
Negative controls must distinguish read consistency, incomplete publication, missing state, wrong incarnation, and an unreached fault boundary.

Research checks covered store registration, production callers, transaction ordering, dependency status, local references, and changed prose.
No fault was armed, process terminated, store repaired, production source changed, or correctness claim accepted for this task.
