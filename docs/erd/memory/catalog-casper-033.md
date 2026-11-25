# catalog casper 033

[Inventory](inventory.md) | [PlantUML](catalog-casper-033.puml)

![catalog-casper-033](catalog-casper-033.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### StateLineage

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Meet.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `Disconnected` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

### FloorOfView

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Advance.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `NoAdvance` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `ContainmentHold.derived` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `AbsenceHold.missing` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `IncompatibilityHold.detail` | 0..n UTF-8 bytes when this variant is active | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |

### WitnessMemo

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `verdicts` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `committees` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `evaluations` | 1 scalar | 8 B | None |

### CanonicalDispositions

Cardinality: Alias, not a separate allocation. Count values of the target type.

Size: Same layout and allocations as target. Do not add alias bytes again.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |

### RetryGateBasis

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Open` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `NoDisposition` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `NoKeptRejection` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `RecordAboveFloor.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### FloorContext

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `floor` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `floor_state` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `settled_floors` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `frontier` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `parents` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `dispositions` | 1 wrapper slot | Wrapper implementation and payload layout required | Payload allocations and any lock/waiter allocations. Once and lazy payloads can be uninitialized |
| `effect_memo` | 1 wrapper slot | Wrapper implementation and payload layout required | Payload allocations and any lock/waiter allocations. Once and lazy payloads can be uninitialized |

### BundleEntryKind

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `File` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Dir` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

### BundleConsensusMode

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Oracular` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Consensus` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

## Cross-Page References

- `CanonicalDispositions: value -> SigDisposition [catalog-casper-053.svg]`
- `FloorContext: floor -> Floor [catalog-casper-032.svg]`
- `FloorContext: floor_state -> StateHash [catalog-models-027.svg]`
- `FloorContext: frontier -> Floor [catalog-casper-032.svg]`
- `FloorContext: parents -> BlockHash [catalog-models-027.svg]`
- `FloorContext: settled_floors -> Floor [catalog-casper-032.svg]`
- `FloorOfView: AbsenceHold.missing -> BlockHash [catalog-models-027.svg]`
- `FloorOfView: Advance.0 -> Floor [catalog-casper-032.svg]`
- `FloorOfView: ContainmentHold.derived -> Floor [catalog-casper-032.svg]`
- `RetryGateBasis: RecordAboveFloor.0 -> BlockHash [catalog-models-027.svg]`
- `StateLineage: Meet.0 -> BlockHash [catalog-models-027.svg]`
- `WitnessMemo: committees -> BlockHash [catalog-models-027.svg]`
- `WitnessMemo: verdicts -> BlockHash [catalog-models-027.svg]`
