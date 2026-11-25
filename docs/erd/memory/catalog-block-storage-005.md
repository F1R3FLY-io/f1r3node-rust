# catalog block storage 005

[Inventory](inventory.md) | [PlantUML](catalog-block-storage-005.puml)

![catalog-block-storage-005](catalog-block-storage-005.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### Availability

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Present.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `Absent` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

### BlockBody

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Held.0` | 0..n elements when this variant is active | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: None |
| `NotHeld` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

### DetachedBlock

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `metadata` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `floor` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `frontier` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### Coverage

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `held_blocks` | 1 scalar | 8 B | None |
| `complete_held_dag` | 1 scalar | 1 B | None |
| `requested_bodies` | 1 scalar | 8 B | None |

### CapturePhase

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `GuardsHeld` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `StateCopied` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `RowsRead` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Validated` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `GuardsReleased` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

### SnapshotData

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `schema_version` | 1 scalar | 4 B | None |
| `scope` | 1 borrowed reference | Pointer or fat pointer. Referent layout required | No owned allocation. Referent: 1 value |
| `limits` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `coverage` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `insertion_generation` | 1 scalar | 8 B | None |
| `view_sequence` | 1 scalar | 8 B | None |
| `transactions` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `usage` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `work` | 1 scalar | 8 B | None |
| `dag_set` | 0..n distinct elements | Container and hasher implementation required | Element storage, owned allocations, spare capacity and index overhead. No type-level count cap |
| `child_map` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `height_map` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `block_number_map` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `main_parent_map` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `self_justification_map` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `last_finalized_block` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 2 component values. Sum of component-owned allocations |
| `finalized_block_set` | 0..n distinct elements | Container and hasher implementation required | Element storage, owned allocations, spare capacity and index overhead. No type-level count cap |
| `latest_messages` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `invalid_blocks` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `blocks` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `bodies` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |

### DetachedDagSnapshot

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `data` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `canonical` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: None |
| `digest` | 32 elements | 32 B. 32 * I(element), inline array | Sum of element-owned allocations. Element: None |

### ScratchView

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `representation` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `blocks` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `excluded_stores` | 1 borrowed reference | Pointer or fat pointer. Referent layout required | No owned allocation. Referent: 0..n elements |

## Cross-Page References

- `DetachedBlock: floor -> BlockHash [catalog-models-027.svg]`
- `DetachedBlock: frontier -> BlockHash [catalog-models-027.svg]`
- `DetachedBlock: metadata -> BlockMetadata [catalog-models-027.svg]`
- `ScratchView: blocks -> KeyValueBlockStore [catalog-block-storage-008.svg]`
- `ScratchView: representation -> KeyValueDagRepresentation [catalog-block-storage-001.svg]`
- `SnapshotData: block_number_map -> BlockHash [catalog-models-027.svg]`
- `SnapshotData: blocks -> BlockHash [catalog-models-027.svg]`
- `SnapshotData: bodies -> BlockHash [catalog-models-027.svg]`
- `SnapshotData: child_map -> BlockHash [catalog-models-027.svg]`
- `SnapshotData: dag_set -> BlockHash [catalog-models-027.svg]`
- `SnapshotData: finalized_block_set -> BlockHash [catalog-models-027.svg]`
- `SnapshotData: height_map -> BlockHash [catalog-models-027.svg]`
- `SnapshotData: invalid_blocks -> BlockHash [catalog-models-027.svg]`
- `SnapshotData: invalid_blocks -> BlockMetadata [catalog-models-027.svg]`
- `SnapshotData: last_finalized_block -> BlockHash [catalog-models-027.svg]`
- `SnapshotData: latest_messages -> BlockHash [catalog-models-027.svg]`
- `SnapshotData: limits -> CaptureLimits [catalog-block-storage-004.svg]`
- `SnapshotData: main_parent_map -> BlockHash [catalog-models-027.svg]`
- `SnapshotData: self_justification_map -> BlockHash [catalog-models-027.svg]`
- `SnapshotData: transactions -> TransactionIdentity [catalog-shared-008.svg]`
- `SnapshotData: usage -> ReadUsage [catalog-shared-009.svg]`
