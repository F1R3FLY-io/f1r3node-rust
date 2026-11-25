# catalog casper 044

[Inventory](inventory.md) | [PlantUML](catalog-casper-044.puml)

![catalog-casper-044](catalog-casper-044.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### ForkChoiceInputs

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `max_number_of_parents` | 1 scalar | 4 B | None |
| `approved_block_number` | 1 scalar | 8 B | None |
| `latest_message_depth` | 1 scalar | 8 B | None |

### CaptureEndpoint

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `dag` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `blocks` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `authority` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `fork_choice` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### AttachmentError

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Unsupported` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `AlreadyAttached` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

### EventKind

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `LiveDerivation` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `EffectAttempt` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `EffectReturn` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `DetachedDerivation` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `PersistedObservation` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

### FloorOutcome

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Advance` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `NoAdvance` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `ContainmentHold` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `AbsenceHold` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `IncompatibilityHold` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Failed` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

### ObservationEvent

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `installation` | 1 scalar | 8 B | None |
| `sequence` | 1 scalar | 8 B | None |
| `elapsed_ns` | 1 scalar | 8 B | None |
| `kind` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `operation` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 scalar. None |
| `outcome` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |
| `hash` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 32 elements. Sum of element-owned allocations. Element: None |
| `height` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 scalar. None |
| `success` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 scalar. None |
| `snapshot_digest` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 32 elements. Sum of element-owned allocations. Element: None |

### Interval

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `installation` | 1 scalar | 8 B | None |
| `started` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `active` | 1 atomic value | Target atomic layout required | None for the atomic value itself |
| `sequence` | 1 atomic value | Target atomic layout required | None for the atomic value itself |
| `operations` | 1 atomic value | Target atomic layout required | None for the atomic value itself |
| `lost` | 1 atomic value | Target atomic layout required | None for the atomic value itself |
| `overflow` | 1 atomic value | Target atomic layout required | None for the atomic value itself |
| `sender` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### ObserverBinding

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `0` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |

## Cross-Page References

- `CaptureEndpoint: authority -> AuthorityInputs [catalog-casper-043.svg]`
- `CaptureEndpoint: blocks -> KeyValueBlockStore [catalog-block-storage-008.svg]`
- `CaptureEndpoint: dag -> BlockDagKeyValueStorage [catalog-block-storage-001.svg]`
