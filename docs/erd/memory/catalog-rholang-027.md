# catalog rholang 027

[Inventory](inventory.md) | [PlantUML](catalog-rholang-027.puml)

![catalog-rholang-027](catalog-rholang-027.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### Indexed

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `index` | 1 scalar | 8 B | None |

### ParCount

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `sends` | 1 scalar | 8 B | None |
| `receives` | 1 scalar | 8 B | None |
| `news` | 1 scalar | 8 B | None |
| `exprs` | 1 scalar | 8 B | None |
| `matches` | 1 scalar | 8 B | None |
| `unforgeables` | 1 scalar | 8 B | None |
| `bundles` | 1 scalar | 8 B | None |

### StepBudget

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `0` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |

### SpatialMatcherContext

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `free_map` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `budget` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### SizeRange

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `min` | 1 scalar | 8 B | None |
| `max` | 1 scalar | 8 B | None |

### SubsetCursor

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `len` | 1 scalar | 8 B | None |
| `min` | 1 scalar | 8 B | None |
| `max` | 1 scalar | 8 B | None |
| `empty` | 1 scalar | 1 B | None |
| `positions` | 0..n elements | Container implementation required | Element storage, owned allocations, spare capacity or per-node links. No type-level count cap |

### SubPars

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `par` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `done` | 1 scalar | 1 B | None |
| `cursors` | 7 elements | 7 * I(element), inline array | Sum of element-owned allocations. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `started` | 1 scalar | 1 B | None |
| `budget` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### RholangMergingLogic

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| No declared fields | 0 field payloads | Compiler layout required | No field-owned allocations |

## Cross-Page References

No resolved cross-page references.
