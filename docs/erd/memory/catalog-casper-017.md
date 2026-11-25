# catalog casper 017

[Inventory](inventory.md) | [PlantUML](catalog-casper-017.puml)

![catalog-casper-017](catalog-casper-017.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### ProposeID

Cardinality: Alias, not a separate allocation. Count values of the target type.

Size: Same layout and allocations as target. Do not add alias bytes again.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### ProposeResult

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `propose_status` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### ProposeStatus

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Success.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `Failure.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### ProposeSuccess

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `result` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### ProposeFailure

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `NoNewDeploys` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `InternalDeployError` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `BugError` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `MissingBlock.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `FloorDistanceHold` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `CheckConstraintsFailure.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### CheckProposeConstraintsResult

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Success` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Failure.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### CheckProposeConstraintsFailure

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `NotBonded` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `NotEnoughNewBlocks` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `TooFarAheadOfLastFinalized` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `HistoryIncomplete` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

### BlockCreatorResult

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `NoNewDeploys` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Created.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `Created.1` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `Created.2` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `Created.3` | 0..1 present payload when this variant is active | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |

## Cross-Page References

- `BlockCreatorResult: Created.0 -> BlockMessage [catalog-models-031.svg]`
- `BlockCreatorResult: Created.3 -> DeployAdmission [catalog-shared-004.svg]`
- `ProposeSuccess: result -> ValidBlock [catalog-casper-006.svg]`
