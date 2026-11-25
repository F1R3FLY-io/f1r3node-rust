# catalog rholang 004

[Inventory](inventory.md) | [PlantUML](catalog-rholang-004.puml)

![catalog-rholang-004](catalog-rholang-004.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### Compiler

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| No declared fields | 0 field payloads | Compiler layout required | No field-owned allocations |

### FreeContext

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `level` | 1 scalar | 8 B | None |
| `typ` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `source_span` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### Shadowed

Cardinality: Alias, not a separate allocation. Count values of the target type.

Size: Same layout and allocations as target. Do not add alias bytes again.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 3 component values | Sum of component inline sizes plus compiler padding | Sum of component-owned allocations |

### FreeMap

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `next_level` | 1 scalar | 8 B | None |
| `level_bindings` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `wildcards` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `connectives` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |

### IdContextSpan

Cardinality: Alias, not a separate allocation. Count values of the target type.

Size: Same layout and allocations as target. Do not add alias bytes again.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 3 component values | Sum of component inline sizes plus compiler padding | Sum of component-owned allocations |

### IdContextPos

Cardinality: Alias, not a separate allocation. Count values of the target type.

Size: Same layout and allocations as target. Do not add alias bytes again.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 3 component values | Sum of component inline sizes plus compiler padding | Sum of component-owned allocations |

### VarSort

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `ProcSort` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `NameSort` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

### ProcVisitInputs

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `par` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `bound_map_chain` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `free_map` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

## Cross-Page References

- `ProcVisitInputs: bound_map_chain -> BoundMapChain [catalog-rholang-003.svg]`
