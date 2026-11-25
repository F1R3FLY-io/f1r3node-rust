# catalog casper 024

[Inventory](inventory.md) | [PlantUML](catalog-casper-024.puml)

![catalog-casper-024](catalog-casper-024.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### Requests

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `wanted` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `queue` | 0..n elements | Container implementation required | Element storage, owned allocations, spare capacity or per-node links. No type-level count cap |
| `in_flight` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `done` | 1 scalar | 8 B | None |

### ValidPage

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `path` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `wanted` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `history_items` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `data_items` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### HorizonSync

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `request_ops` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `has_root` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `state_importer` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `roots` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `walk` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `requests` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `request_timeout` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `max_request_timeout` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### ReqStatus

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Init` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Requested` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Received` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Done` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

### StatePartPath

Cardinality: Alias, not a separate allocation. Count values of the target type.

Size: Same layout and allocations as target. Do not add alias bytes again.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Sum of component-owned allocations |

### StoreItems

Cardinality: Alias, not a separate allocation. Count values of the target type.

Size: Same layout and allocations as target. Do not add alias bytes again.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Sum of component-owned allocations |

### ValidChunk

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `history_items` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `data_items` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `complete` | 1 scalar | 1 B | None |

### ST

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `d` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |

## Cross-Page References

- `HorizonSync: has_root -> HasRootFn [catalog-casper-023.svg]`
- `HorizonSync: roots -> Blake2b256Hash [catalog-rspacepp-002.svg]`
- `HorizonSync: walk -> StateWalk [catalog-rspacepp-018.svg]`
- `Requests: in_flight -> PathTimer [catalog-casper-023.svg]`
- `Requests: wanted -> Wanted [catalog-casper-023.svg]`
- `StatePartPath: value -> Blake2b256Hash [catalog-rspacepp-002.svg]`
- `StoreItems: value -> Blake2b256Hash [catalog-rspacepp-002.svg]`
- `ValidPage: wanted -> Wanted [catalog-casper-023.svg]`
