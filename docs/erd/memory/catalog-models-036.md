# catalog models 036

[Inventory](inventory.md) | [PlantUML](catalog-models-036.puml)

![catalog-models-036](catalog-models-036.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### HostWorkReservationError

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `UsageOverflow.dimension` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `UsageOverflow.limit` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `UsageOverflow.usage` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `UsageOverflow.requested` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `LimitExceeded.dimension` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `LimitExceeded.limit` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `LimitExceeded.usage` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `LimitExceeded.requested` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `BudgetRejected.dimension` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `BudgetRejected.usage` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `BudgetRejected.requested` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### ParMap

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `ps` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `connective_used` | 1 scalar | 1 B | None |
| `locally_free` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: None |
| `remainder` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |

### ParMapTypeMapper

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| No declared fields | 0 field payloads | Compiler layout required | No field-owned allocations |

### ParSet

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `ps` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `connective_used` | 1 scalar | 1 B | None |
| `locally_free` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: None |
| `remainder` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |

### ParSetTypeMapper

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| No declared fields | 0 field payloads | Compiler layout required | No field-owned allocations |

### ParToSExpr

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| No declared fields | 0 field payloads | Compiler layout required | No field-owned allocations |

### Tag

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `NewVar` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Symbol.0` | 1 scalar when this variant is active | 1 B | None |
| `VarRef.0` | 1 scalar when this variant is active | 1 B | None |
| `Arity.0` | 1 scalar when this variant is active | 1 B | None |

### SExpr

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Symbol.0` | 0..n UTF-8 bytes when this variant is active | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `List.0` | 0..n elements when this variant is active | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |

## Cross-Page References

- `HostWorkReservationError: BudgetRejected.dimension -> HostWorkDimension [catalog-models-035.svg]`
- `HostWorkReservationError: BudgetRejected.requested -> HostWorkUnits [catalog-models-035.svg]`
- `HostWorkReservationError: BudgetRejected.usage -> HostWorkUsage [catalog-models-035.svg]`
- `HostWorkReservationError: LimitExceeded.dimension -> HostWorkDimension [catalog-models-035.svg]`
- `HostWorkReservationError: LimitExceeded.limit -> HostWorkLimit [catalog-models-035.svg]`
- `HostWorkReservationError: LimitExceeded.requested -> HostWorkUnits [catalog-models-035.svg]`
- `HostWorkReservationError: LimitExceeded.usage -> HostWorkUsage [catalog-models-035.svg]`
- `HostWorkReservationError: UsageOverflow.dimension -> HostWorkDimension [catalog-models-035.svg]`
- `HostWorkReservationError: UsageOverflow.limit -> HostWorkLimit [catalog-models-035.svg]`
- `HostWorkReservationError: UsageOverflow.requested -> HostWorkUnits [catalog-models-035.svg]`
- `HostWorkReservationError: UsageOverflow.usage -> HostWorkUsage [catalog-models-035.svg]`
- `ParMap: ps -> SortedParMap [catalog-models-040.svg]`
- `ParSet: ps -> SortedParHashSet [catalog-models-040.svg]`
