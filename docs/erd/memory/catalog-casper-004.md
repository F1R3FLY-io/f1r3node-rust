# catalog casper 004

[Inventory](inventory.md) | [PlantUML](catalog-casper-004.puml)

![catalog-casper-004](catalog-casper-004.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### ProposeRefusal

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `NotBonded` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `NotEnoughNewBlocks` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `TooFarAheadOfLastFinalized` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `HistoryIncomplete` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `MissingBlock` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `FloorDistanceHold` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

### ProposeRefusedError

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `refusal` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `message` | 0..n UTF-8 bytes | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |

### ReportQueueMetricGuard

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| No declared fields | 0 field payloads | Compiler layout required | No field-owned allocations |

### ReportPermit

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `RejectIfHeld` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Wait` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

### BlockReportError

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `CasperNotInitialized` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `ReadOnlyRequired` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `BlockNotFound.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `StateUnavailable.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `ExcessivePhloLimit.block` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `ExcessivePhloLimit.cap` | 1 scalar when this variant is active | 8 B | None |
| `Busy` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `ReplayFailed.0` | 0..n UTF-8 bytes when this variant is active | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `PostStateMismatch.block` | 0..n UTF-8 bytes when this variant is active | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `PostStateMismatch.computed` | 0..n UTF-8 bytes when this variant is active | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `PostStateMismatch.recorded` | 0..n UTF-8 bytes when this variant is active | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `BlockInfoError.0` | 0..n UTF-8 bytes when this variant is active | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `StoreError.0` | 0..n UTF-8 bytes when this variant is active | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `SemaphoreError.0` | 0..n UTF-8 bytes when this variant is active | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |

### ApiErr

Cardinality: Alias, not a separate allocation. Count values of the target type.

Size: Same layout and allocations as target. Do not add alias bytes again.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 1 of 2 alternatives | Maximum variant layout plus compiler tag and padding | Only the active Ok or Err payload contributes owned allocations |

### BlockReportAPI

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `reporting_casper` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `report_store` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `engine_cell` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `block_store` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `oracle` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `block_report_semaphore` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `report_transformer` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `dev_mode` | 1 scalar | 1 B | None |
| `report_cache_retention` | 1 scalar | 4 B | None |
| `report_prune_cursor` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |

### ApiErr

Cardinality: Alias, not a separate allocation. Count values of the target type.

Size: Same layout and allocations as target. Do not add alias bytes again.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 1 of 2 alternatives | Maximum variant layout plus compiler tag and padding | Only the active Ok or Err payload contributes owned allocations |

## Cross-Page References

- `BlockReportAPI: block_store -> KeyValueBlockStore [catalog-block-storage-008.svg]`
- `BlockReportAPI: engine_cell -> EngineCell [catalog-casper-021.svg]`
- `BlockReportAPI: oracle -> CliqueOracleImpl [catalog-casper-043.svg]`
- `BlockReportAPI: report_store -> ReportStore [catalog-casper-039.svg]`
- `BlockReportAPI: report_transformer -> ReportingProtoTransformer [catalog-casper-040.svg]`
- `BlockReportError: BlockNotFound.0 -> BlockHash [catalog-models-027.svg]`
- `BlockReportError: ExcessivePhloLimit.block -> BlockHash [catalog-models-027.svg]`
- `BlockReportError: StateUnavailable.0 -> BlockHash [catalog-models-027.svg]`
