# catalog casper 030

[Inventory](inventory.md) | [PlantUML](catalog-casper-030.puml)

![catalog-casper-030](catalog-casper-030.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### WalPayloadTickStop

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `signal` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |

### WalPayloadTickHandle

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `join_handle` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `stop` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### EnumerateStats

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `resolved_locally` | 1 scalar | 8 B | None |
| `enqueued_for_fetch` | 1 scalar | 8 B | None |

### BootApplyReport

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `enumerated` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `sidecar_populated` | 1 scalar | 8 B | None |
| `wal_entries` | 1 scalar | 8 B | None |

### BootApplyError

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `PayloadFetchTimeout.pending_count` | 1 scalar when this variant is active | 8 B | None |
| `MissingResolvedHash.hash_hex` | 0..n UTF-8 bytes when this variant is active | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `ApplierFailed.message` | 0..n UTF-8 bytes when this variant is active | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `ApplierPanic.message` | 0..n UTF-8 bytes when this variant is active | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |

### EquivocationDetector

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| No declared fields | 0 field payloads | Compiler layout required | No field-owned allocations |

### CanonicalChildCache

Cardinality: Alias, not a separate allocation. Count values of the target type.

Size: Same layout and allocations as target. Do not add alias bytes again.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |

### CasperError

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `InterpreterError.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `KvStoreError.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `RuntimeError.0` | 0..n UTF-8 bytes when this variant is active | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `SystemRuntimeError.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `SigningError.0` | 0..n UTF-8 bytes when this variant is active | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `ReplayFailure.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `CommError.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `HistoryError.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `StreamError.0` | 0..n UTF-8 bytes when this variant is active | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `LockError.0` | 0..n UTF-8 bytes when this variant is active | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `SlashAuth.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `BlockNotHeld.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `BlockNotHeld.1` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `IncompatibleFinalizedFork.0` | 0..n UTF-8 bytes when this variant is active | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `FloorDistanceExceeded.distance` | 1 scalar when this variant is active | 8 B | None |
| `FloorDistanceExceeded.cap` | 1 scalar when this variant is active | 8 B | None |
| `FloorDistanceExceeded.floor_block` | 1 scalar when this variant is active | 8 B | None |
| `FloorDistanceExceeded.parents` | 1 scalar when this variant is active | 8 B | None |
| `Other.0` | 0..n UTF-8 bytes when this variant is active | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |

## Cross-Page References

- `CanonicalChildCache: value -> BlockHash [catalog-models-027.svg]`
- `CasperError: BlockNotHeld.0 -> BlockHash [catalog-models-027.svg]`
- `CasperError: BlockNotHeld.1 -> MissingBlockContext [catalog-shared-007.svg]`
- `CasperError: CommError.0 -> CommError [catalog-comm-003.svg]`
- `CasperError: HistoryError.0 -> HistoryError [catalog-rspacepp-001.svg]`
- `CasperError: InterpreterError.0 -> InterpreterError [catalog-rholang-006.svg]`
- `CasperError: KvStoreError.0 -> KvStoreError [catalog-shared-007.svg]`
- `CasperError: ReplayFailure.0 -> ReplayFailure [catalog-casper-055.svg]`
- `CasperError: SlashAuth.0 -> SlashAuthError [catalog-casper-043.svg]`
- `CasperError: SystemRuntimeError.0 -> SystemDeployPlatformFailure [catalog-casper-057.svg]`
