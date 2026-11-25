# catalog block storage 001

[Inventory](inventory.md) | [PlantUML](catalog-block-storage-001.puml)

![catalog-block-storage-001](catalog-block-storage-001.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### DeployId

Cardinality: Alias, not a separate allocation. Count values of the target type.

Size: Same layout and allocations as target. Do not add alias bytes again.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### DagResidentCounts

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `blocks` | 1 scalar | 8 B | None |
| `child_keys` | 1 scalar | 8 B | None |
| `heights` | 1 scalar | 8 B | None |
| `finalized` | 1 scalar | 8 B | None |

### InsertMode

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Normal` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Equivocation.sibling` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `Approved` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `SettledHistory` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

### KeyValueDagRepresentation

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `history` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 shared strong reference. One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `view_sequence` | 1 scalar | 8 B | None |
| `view_max_height` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 scalar. None |
| `pruning_cutoff` | 1 scalar | 8 B | None |
| `dag_set` | 0..n distinct elements | Container and hasher implementation required | Element storage, owned allocations, spare capacity and index overhead. No type-level count cap |
| `latest_messages_map` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `child_map` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `height_map` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `block_number_map` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `main_parent_map` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `self_justification_map` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `last_finalized_block_hash` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `finalized_blocks_set` | 0..n distinct elements | Container and hasher implementation required | Element storage, owned allocations, spare capacity and index overhead. No type-level count cap |
| `block_metadata_index` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `floor_index` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `frontier_index` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `lifecycle` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `carrier_index` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |

### BlockDagKeyValueStorage

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `persistence` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 shared strong reference. One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `global_lock` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `latest_messages_index` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `block_metadata_index` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `floor_index` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `frontier_index` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `lifecycle` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `carrier_index` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `dag_generation` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `ft_lower_bound` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `genesis_hash_index` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `insert_observer` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |

### SoakCaptureAccess

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `_global` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `metadata` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `storage` | 1 borrowed reference | Pointer or fat pointer. Referent layout required | No owned allocation. Referent: 1 value |

### DagStateCopy

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `sequence` | 1 scalar | 8 B | None |
| `max_height` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 scalar. None |
| `dag_set` | 0..n distinct elements | Container and hasher implementation required | Element storage, owned allocations, spare capacity and index overhead. No type-level count cap |
| `child_map` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `height_map` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `block_number_map` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `main_parent_map` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `self_justification_map` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `last_finalized_block` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 2 component values. Sum of component-owned allocations |
| `finalized_block_set` | 0..n distinct elements | Container and hasher implementation required | Element storage, owned allocations, spare capacity and index overhead. No type-level count cap |

### BlockMetadataStore

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `store` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `dag_state` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `persistence` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 shared strong reference. One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `hot_window` | 1 scalar | 8 B | None |

## Cross-Page References

- `BlockDagKeyValueStorage: carrier_index -> CarrierIndex [catalog-block-storage-002.svg]`
- `BlockDagKeyValueStorage: floor_index -> BlockHashSerde [catalog-models-027.svg]`
- `BlockDagKeyValueStorage: floor_index -> KeyValueTypedStoreImpl [catalog-shared-007.svg]`
- `BlockDagKeyValueStorage: frontier_index -> BlockHashSerde [catalog-models-027.svg]`
- `BlockDagKeyValueStorage: frontier_index -> KeyValueTypedStoreImpl [catalog-shared-007.svg]`
- `BlockDagKeyValueStorage: genesis_hash_index -> BlockHashSerde [catalog-models-027.svg]`
- `BlockDagKeyValueStorage: genesis_hash_index -> KeyValueTypedStoreImpl [catalog-shared-007.svg]`
- `BlockDagKeyValueStorage: insert_observer -> BlockHash [catalog-models-027.svg]`
- `BlockDagKeyValueStorage: latest_messages_index -> BlockHashSerde [catalog-models-027.svg]`
- `BlockDagKeyValueStorage: latest_messages_index -> KeyValueTypedStoreImpl [catalog-shared-007.svg]`
- `BlockDagKeyValueStorage: latest_messages_index -> ValidatorSerde [catalog-models-040.svg]`
- `BlockDagKeyValueStorage: lifecycle -> DeployLifecycleTables [catalog-block-storage-004.svg]`
- `BlockDagKeyValueStorage: persistence -> DagPersistence [catalog-block-storage-004.svg]`
- `BlockMetadataStore: dag_state -> DagState [catalog-block-storage-002.svg]`
- `BlockMetadataStore: persistence -> DagPersistence [catalog-block-storage-004.svg]`
- `BlockMetadataStore: store -> BlockHashSerde [catalog-models-027.svg]`
- `BlockMetadataStore: store -> BlockMetadata [catalog-models-027.svg]`
- `BlockMetadataStore: store -> KeyValueTypedStoreImpl [catalog-shared-007.svg]`
- `DagStateCopy: block_number_map -> BlockHash [catalog-models-027.svg]`
- `DagStateCopy: child_map -> BlockHash [catalog-models-027.svg]`
- `DagStateCopy: dag_set -> BlockHash [catalog-models-027.svg]`
- `DagStateCopy: finalized_block_set -> BlockHash [catalog-models-027.svg]`
- `DagStateCopy: height_map -> BlockHash [catalog-models-027.svg]`
- `DagStateCopy: last_finalized_block -> BlockHash [catalog-models-027.svg]`
- `DagStateCopy: main_parent_map -> BlockHash [catalog-models-027.svg]`
- `DagStateCopy: self_justification_map -> BlockHash [catalog-models-027.svg]`
- `InsertMode: Equivocation.sibling -> BlockHash [catalog-models-027.svg]`
- `KeyValueDagRepresentation: block_number_map -> BlockHash [catalog-models-027.svg]`
- `KeyValueDagRepresentation: carrier_index -> CarrierIndex [catalog-block-storage-002.svg]`
- `KeyValueDagRepresentation: child_map -> BlockHash [catalog-models-027.svg]`
- `KeyValueDagRepresentation: dag_set -> BlockHash [catalog-models-027.svg]`
- `KeyValueDagRepresentation: finalized_blocks_set -> BlockHash [catalog-models-027.svg]`
- `KeyValueDagRepresentation: floor_index -> BlockHashSerde [catalog-models-027.svg]`
- `KeyValueDagRepresentation: floor_index -> KeyValueTypedStoreImpl [catalog-shared-007.svg]`
- `KeyValueDagRepresentation: frontier_index -> BlockHashSerde [catalog-models-027.svg]`
- `KeyValueDagRepresentation: frontier_index -> KeyValueTypedStoreImpl [catalog-shared-007.svg]`
- `KeyValueDagRepresentation: height_map -> BlockHash [catalog-models-027.svg]`
- `KeyValueDagRepresentation: history -> DagHistoryIndex [catalog-block-storage-003.svg]`
- `KeyValueDagRepresentation: last_finalized_block_hash -> BlockHash [catalog-models-027.svg]`
- `KeyValueDagRepresentation: latest_messages_map -> BlockHash [catalog-models-027.svg]`
- `KeyValueDagRepresentation: lifecycle -> DeployLifecycleTables [catalog-block-storage-004.svg]`
- `KeyValueDagRepresentation: main_parent_map -> BlockHash [catalog-models-027.svg]`
- `KeyValueDagRepresentation: self_justification_map -> BlockHash [catalog-models-027.svg]`
