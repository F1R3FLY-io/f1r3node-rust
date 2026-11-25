# catalog casper 026

[Inventory](inventory.md) | [PlantUML](catalog-casper-026.puml)

![catalog-casper-026](catalog-casper-026.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### ScopeSets

Cardinality: Alias, not a separate allocation. Count values of the target type.

Size: Same layout and allocations as target. Do not add alias bytes again.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 3 component values | Sum of component inline sizes plus compiler padding | Sum of component-owned allocations |

### MultiParentCasperImpl

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `observer` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `ingest` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `event_publisher` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `runtime_manager` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `estimator` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `block_store` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `block_dag_storage` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `deploy_storage` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `rejected_deploy_buffer` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `validator_id` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |
| `finalized_bond_cache` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `casper_shard_conf` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `approved_block` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `effective_anchor` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `divergence_monitor` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `finalizer` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `heartbeat_signal_ref` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `deploys_in_scope_cache` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `active_validators_cache` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `judged_twins` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |

### TwinJudgement

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Valid` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Invalid.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `Deferred.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### SnapshotChunkContext

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `sync_driver` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `snapshot_dir` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `snapshot_merkle_roots` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |

### WalPayloadContext

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `sync_driver` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `payload_lookup` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `tick_stop` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |

### LastFinalizedBlockNotFoundError

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| No declared fields | 0 field payloads | Compiler layout required | No field-owned allocations |

### Running

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `ingest` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `casper` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `approved_block` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `the_init` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `init_called` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `disable_state_exporter` | 1 scalar | 1 B | None |
| `transport` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `conf` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `state_items_tx` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |
| `snapshot_chunk_ctx` | 1 wrapper slot | Wrapper implementation and payload layout required | Payload allocations and any lock/waiter allocations. Once and lazy payloads can be uninitialized |
| `wal_payload_ctx` | 1 wrapper slot | Wrapper implementation and payload layout required | Payload allocations and any lock/waiter allocations. Once and lazy payloads can be uninitialized |
| `state_export_limiter` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### Chunk

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `root` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `start_path` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `last_path` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

## Cross-Page References

- `Chunk: last_path -> StatePartPath [catalog-casper-024.svg]`
- `Chunk: root -> Blake2b256Hash [catalog-rspacepp-002.svg]`
- `Chunk: start_path -> StatePartPath [catalog-casper-024.svg]`
- `MultiParentCasperImpl: approved_block -> BlockMessage [catalog-models-031.svg]`
- `MultiParentCasperImpl: block_dag_storage -> BlockDagKeyValueStorage [catalog-block-storage-001.svg]`
- `MultiParentCasperImpl: block_store -> KeyValueBlockStore [catalog-block-storage-008.svg]`
- `MultiParentCasperImpl: casper_shard_conf -> CasperShardConf [catalog-casper-020.svg]`
- `MultiParentCasperImpl: deploy_storage -> KeyValueDeployStorage [catalog-block-storage-008.svg]`
- `MultiParentCasperImpl: deploys_in_scope_cache -> BlockHash [catalog-models-027.svg]`
- `MultiParentCasperImpl: divergence_monitor -> DivergenceMonitor [catalog-casper-025.svg]`
- `MultiParentCasperImpl: estimator -> Estimator [catalog-casper-031.svg]`
- `MultiParentCasperImpl: event_publisher -> F1r3flyEvents [catalog-shared-005.svg]`
- `MultiParentCasperImpl: finalized_bond_cache -> FinalizedBondCache [catalog-casper-025.svg]`
- `MultiParentCasperImpl: finalizer -> Finalizer [catalog-casper-025.svg]`
- `MultiParentCasperImpl: heartbeat_signal_ref -> HeartbeatSignalRef [catalog-casper-036.svg]`
- `MultiParentCasperImpl: ingest -> IngestHandle [catalog-casper-007.svg]`
- `MultiParentCasperImpl: judged_twins -> BlockHash [catalog-models-027.svg]`
- `MultiParentCasperImpl: judged_twins -> BoundedCache [catalog-casper-050.svg]`
- `MultiParentCasperImpl: judged_twins -> InvalidBlock [catalog-casper-006.svg]`
- `MultiParentCasperImpl: observer -> ObserverBinding [catalog-casper-044.svg]`
- `MultiParentCasperImpl: rejected_deploy_buffer -> KeyValueRejectedDeployBuffer [catalog-block-storage-008.svg]`
- `MultiParentCasperImpl: runtime_manager -> RuntimeManager [catalog-casper-056.svg]`
- `MultiParentCasperImpl: validator_id -> ValidatorIdentity [catalog-casper-058.svg]`
- `Running: approved_block -> ApprovedBlock [catalog-models-030.svg]`
- `Running: conf -> RPConf [catalog-comm-004.svg]`
- `Running: ingest -> IngestHandle [catalog-casper-007.svg]`
- `Running: state_export_limiter -> StateExportLimiter [catalog-casper-028.svg]`
- `Running: state_items_tx -> StoreItemsMessage [catalog-models-033.svg]`
- `Running: the_init -> CasperError [catalog-casper-030.svg]`
- `SnapshotChunkContext: sync_driver -> SnapshotChunkSyncDriver [catalog-casper-028.svg]`
- `TwinJudgement: Deferred.0 -> BlockError [catalog-casper-006.svg]`
- `TwinJudgement: Invalid.0 -> InvalidBlock [catalog-casper-006.svg]`
- `WalPayloadContext: sync_driver -> WalPayloadSyncDriver [catalog-casper-029.svg]`
- `WalPayloadContext: tick_stop -> WalPayloadTickStop [catalog-casper-030.svg]`
