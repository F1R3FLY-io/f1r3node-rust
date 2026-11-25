# catalog casper 047

[Inventory](inventory.md) | [PlantUML](catalog-casper-047.puml)

![catalog-casper-047](catalog-casper-047.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### TargetResult

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `target` | 0..n UTF-8 bytes | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `input_scope` | 1 borrowed reference | Pointer or fat pointer. Referent layout required | No owned allocation. Referent: 1 value |
| `comparator` | 1 borrowed reference | Pointer or fat pointer. Referent layout required | No owned allocation. Referent: 1 value |
| `threshold_numerator` | 1 scalar | 8 B | None |
| `threshold_denominator` | 1 scalar | 8 B | None |
| `oracle_decision` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `oracle_witness` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `original_fault_tolerance` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `display_projection` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `display_inputs` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |
| `persisted_fault_tolerance` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `reference_comparison` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### TransactionRecord

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `environment` | 0..n UTF-8 bytes | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `before_open` | 1 scalar | 8 B | None |
| `opened` | 1 scalar | 8 B | None |
| `validated` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 scalar. None |

### WorkReport

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `aggregate` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `preparation` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `measured` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `original` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `reference` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `fork_choice_bounded` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `fork_choice_reference` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `complete` | 1 scalar | 1 B | None |
| `failure` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 0..n UTF-8 bytes. capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |

### AuthorityResponse

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `scope` | 1 borrowed reference | Pointer or fat pointer. Referent layout required | No owned allocation. Referent: 1 value |
| `live_profile_qualified` | 1 scalar | 1 B | None |
| `coverage` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |
| `snapshot_digest` | 0..n UTF-8 bytes | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `authority_digest` | 0..n UTF-8 bytes | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `authority` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `request` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `held_blocks` | 1 scalar | 8 B | None |
| `requested_bodies` | 1 scalar | 8 B | None |
| `capture_work` | 1 scalar | 8 B | None |
| `capture_operations` | 1 scalar | 8 B | None |
| `capture_records` | 1 scalar | 8 B | None |
| `capture_bytes` | 1 scalar | 8 B | None |
| `transactions` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `targets` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `floor_result` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `floor_comparison` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `fork_choice` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `work` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `events` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `display_scope` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 borrowed reference. No owned allocation. Referent: 1 value |

### PartialWork

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `aggregate` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `preparation` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `measured` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `original` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |
| `reference` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |
| `fork_choice_bounded` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |
| `fork_choice_reference` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |
| `failure` | 0..n UTF-8 bytes | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |

### AuthorityFailure

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `reason` | 0..n UTF-8 bytes | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `work` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |

### EvaluationMode

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Bounded` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Reference` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

### LowerBoundRule

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `FinalizedFloor` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `ApprovedBlock` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

## Cross-Page References

- `AuthorityResponse: authority -> AuthorityInputs [catalog-casper-043.svg]`
- `AuthorityResponse: events -> ObservationEvent [catalog-casper-044.svg]`
- `AuthorityResponse: floor_comparison -> FloorComparison [catalog-casper-046.svg]`
- `AuthorityResponse: floor_comparison -> Value [catalog-casper-046.svg]`
- `AuthorityResponse: floor_result -> FloorValue [catalog-casper-046.svg]`
- `AuthorityResponse: floor_result -> Value [catalog-casper-046.svg]`
- `AuthorityResponse: fork_choice -> ForkChoiceObservation [catalog-casper-048.svg]`
- `AuthorityResponse: fork_choice -> Value [catalog-casper-046.svg]`
- `AuthorityResponse: request -> AuthorityRequest [catalog-casper-046.svg]`
- `PartialWork: aggregate -> WorkUsage [catalog-shared-001.svg]`
- `PartialWork: fork_choice_bounded -> WorkUsage [catalog-shared-001.svg]`
- `PartialWork: fork_choice_reference -> WorkUsage [catalog-shared-001.svg]`
- `PartialWork: measured -> WorkUsage [catalog-shared-001.svg]`
- `PartialWork: original -> WorkUsage [catalog-shared-001.svg]`
- `PartialWork: preparation -> WorkUsage [catalog-shared-001.svg]`
- `PartialWork: reference -> WorkUsage [catalog-shared-001.svg]`
- `TargetResult: display_inputs -> DisplayInputs [catalog-casper-045.svg]`
- `TargetResult: display_inputs -> Value [catalog-casper-046.svg]`
- `TargetResult: display_projection -> Value [catalog-casper-046.svg]`
- `TargetResult: oracle_decision -> Value [catalog-casper-046.svg]`
- `TargetResult: oracle_witness -> ExactOracleResult [catalog-casper-042.svg]`
- `TargetResult: oracle_witness -> Value [catalog-casper-046.svg]`
- `TargetResult: original_fault_tolerance -> Value [catalog-casper-046.svg]`
- `TargetResult: persisted_fault_tolerance -> Value [catalog-casper-046.svg]`
- `TargetResult: reference_comparison -> OracleComparison [catalog-casper-046.svg]`
- `TargetResult: reference_comparison -> Value [catalog-casper-046.svg]`
- `WorkReport: aggregate -> WorkUsage [catalog-shared-001.svg]`
- `WorkReport: fork_choice_bounded -> Value [catalog-casper-046.svg]`
- `WorkReport: fork_choice_bounded -> WorkUsage [catalog-shared-001.svg]`
- `WorkReport: fork_choice_reference -> Value [catalog-casper-046.svg]`
- `WorkReport: fork_choice_reference -> WorkUsage [catalog-shared-001.svg]`
- `WorkReport: measured -> WorkUsage [catalog-shared-001.svg]`
- `WorkReport: original -> Value [catalog-casper-046.svg]`
- `WorkReport: original -> WorkUsage [catalog-shared-001.svg]`
- `WorkReport: preparation -> WorkUsage [catalog-shared-001.svg]`
- `WorkReport: reference -> Value [catalog-casper-046.svg]`
- `WorkReport: reference -> WorkUsage [catalog-shared-001.svg]`
