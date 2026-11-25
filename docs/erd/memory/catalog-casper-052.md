# catalog casper 052

[Inventory](inventory.md) | [PlantUML](catalog-casper-052.puml)

![catalog-casper-052](catalog-casper-052.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### ServiceResult

Cardinality: Alias, not a separate allocation. Count values of the target type.

Size: Same layout and allocations as target. Do not add alias bytes again.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 1 of 2 alternatives | Maximum variant layout plus compiler tag and padding | Only the active Ok or Err payload contributes owned allocations |

### LAST

Cardinality: One slot per linked process instance. Lazy payload can be uninitialized.

Size: Declared slot or lazy wrapper layout plus initialized owned allocations.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### DagOperations

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| No declared fields | 0 field payloads | Compiler layout required | No field-owned allocations |

### ReverseOrderedBlockMetadata

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `0` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### GcProgress

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Yielded.deleted` | 1 scalar when this variant is active | 8 B | None |
| `Yielded.blocks_deleted` | 1 scalar when this variant is active | 8 B | None |
| `Complete.deleted` | 1 scalar when this variant is active | 8 B | None |
| `Complete.blocks_deleted` | 1 scalar when this variant is active | 8 B | None |

### GcSweep

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `phase` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `pending_len` | 1 scalar | 8 B | None |

### Pass

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `dag` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `floor` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `conf` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `target` | 1 scalar | 8 B | None |
| `discovery_start` | 1 scalar | 8 B | None |
| `discovery_cursor` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 0..n elements. capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: None |
| `retry_cursor` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 0..n elements. capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: None |
| `active_latest` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `restore_floor` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 scalar. None |

### Phase

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Idle` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Discover.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `FindMinimum.pass` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `FindMinimum.cursor` | 0..1 present payload when this variant is active | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 0..n elements. capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: None |
| `FindMinimum.minimum` | 0..1 present payload when this variant is active | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 scalar. None |
| `FindMinimum.has_below_floor` | 1 scalar when this variant is active | 1 B | None |
| `Delete.pass` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `Delete.moved_past` | 0..1 present payload when this variant is active | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 0..n distinct elements. Element storage, owned allocations, spare capacity and index overhead. No type-level count cap |
| `Delete.cursor` | 0..1 present payload when this variant is active | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 0..n elements. capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: None |

## Cross-Page References

- `Pass: active_latest -> BlockHash [catalog-models-027.svg]`
- `Pass: conf -> CasperShardConf [catalog-casper-020.svg]`
- `Pass: dag -> KeyValueDagRepresentation [catalog-block-storage-001.svg]`
- `Pass: floor -> Floor [catalog-casper-032.svg]`
- `Phase: Delete.moved_past -> BlockHash [catalog-models-027.svg]`
- `ReverseOrderedBlockMetadata: 0 -> BlockMetadata [catalog-models-027.svg]`
