# catalog rspacepp 013

[Inventory](inventory.md) | [PlantUML](catalog-rspacepp-013.puml)

![catalog-rspacepp-013](catalog-rspacepp-013.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### ReportingProduce

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `channel` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `data` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### ReportingConsume

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `channels` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `patterns` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `continuation` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `peeks` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: None |

### ReportingComm

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `consume` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `produces` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |

### ReportPhase

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Unspecified` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Precharge` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `User` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Refund` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

### ReportBatch

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `phase` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `events` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |

### ReportingRspace

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `replay_rspace` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `report` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `soft_report` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `current_phase` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |

### ReportingLogger

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `report` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `soft_report` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |

### RhoEvent

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `RhoComm.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `RhoProduce.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `RhoConsume.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

## Cross-Page References

- `ReportBatch: events -> ReportingEvent [catalog-rspacepp-012.svg]`
- `ReportingLogger: soft_report -> ReportingEvent [catalog-rspacepp-012.svg]`
- `ReportingRspace: replay_rspace -> ReplayRSpace [catalog-rspacepp-012.svg]`
- `ReportingRspace: soft_report -> ReportingEvent [catalog-rspacepp-012.svg]`
- `RhoEvent: RhoComm.0 -> RhoComm [catalog-rspacepp-014.svg]`
- `RhoEvent: RhoConsume.0 -> RhoConsume [catalog-rspacepp-014.svg]`
- `RhoEvent: RhoProduce.0 -> RhoProduce [catalog-rspacepp-014.svg]`
