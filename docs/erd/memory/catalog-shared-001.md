# catalog shared 001

[Inventory](inventory.md) | [PlantUML](catalog-shared-001.puml)

![catalog-shared-001](catalog-shared-001.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### WorkKind

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Traversal` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Metadata` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Oracle` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Clique` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Signature` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Allocation` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

### WorkLimits

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `operations` | 1 scalar | 8 B | None |
| `allocated_bytes` | 1 scalar | 8 B | None |
| `clique_expansions` | 1 scalar | 8 B | None |
| `recursion_depth` | 1 scalar | 8 B | None |

### WorkUsage

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `traversal` | 1 scalar | 8 B | None |
| `metadata` | 1 scalar | 8 B | None |
| `oracle` | 1 scalar | 8 B | None |
| `clique` | 1 scalar | 8 B | None |
| `signature` | 1 scalar | 8 B | None |
| `allocation` | 1 scalar | 8 B | None |
| `operations` | 1 scalar | 8 B | None |
| `allocated_bytes` | 1 scalar | 8 B | None |
| `clique_expansions` | 1 scalar | 8 B | None |
| `maximum_depth` | 1 scalar | 8 B | None |

### NoopWork

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| No declared fields | 0 field payloads | Compiler layout required | No field-owned allocations |

### BudgetState

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `total` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `paths` | WORK_PATHS elements | WORK_PATHS * I(element), inline array | Sum of element-owned allocations. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `failure` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 0..n UTF-8 bytes. capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |

### Budget

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `limits` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `deadline` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `cancelled` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `metadata_bytes` | 1 scalar | 8 B | None |
| `body_bytes` | 1 scalar | 8 B | None |
| `state` | 1 wrapper slot | Wrapper implementation and payload layout required | Payload allocations and any lock/waiter allocations. Once and lazy payloads can be uninitialized |

### CheckedWork

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `budget` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `path` | 1 scalar | 8 B | None |

### GrpcServer

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `server_future` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |
| `port` | 1 scalar | 2 B | None |
| `shutdown_tx` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |

## Cross-Page References

No resolved cross-page references.
