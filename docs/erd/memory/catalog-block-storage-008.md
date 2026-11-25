# catalog block storage 008

[Inventory](inventory.md) | [PlantUML](catalog-block-storage-008.puml)

![catalog-block-storage-008](catalog-block-storage-008.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### PreparedInsertResult

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Inserted` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `AlreadyPresent` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Changed` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `SeedHeld.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `Refused.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### DeployPreparationError

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `InvalidSignature` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `UnsupportedAlgorithm.0` | 0..n UTF-8 bytes when this variant is active | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `Serialization.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### KeyValueDeployStorage

Cardinality: Alias, not a separate allocation. Count values of the target type.

Size: Same layout and allocations as target. Do not add alias bytes again.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### KeyValueRejectedDeployBuffer

Cardinality: Alias, not a separate allocation. Count values of the target type.

Size: Same layout and allocations as target. Do not add alias bytes again.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### BlockDecodeLimits

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `max_compressed_bytes` | 1 scalar | 8 B | None |
| `max_decompressed_bytes` | 1 scalar | 8 B | None |
| `max_expansion_ratio` | 1 scalar | 8 B | None |

### KeyValueBlockStore

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `store` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `store_approved_block` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `approved_block_key` | 1 elements | 1 B. 1 * I(element), inline array | Sum of element-owned allocations. Element: None |
| `pending` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |

### BLOCK_PROTO_DECOMPRESS_BUFFER

Cardinality: One slot per accessing thread. Live payload can be uninitialized.

Size: Declared slot or lazy wrapper layout plus initialized owned allocations.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 1 wrapper slot | Wrapper implementation and payload layout required | Payload allocations and any lock/waiter allocations. Once and lazy payloads can be uninitialized |

### DEPLOY_SIG_DECOMPRESS_BUFFER

Cardinality: One slot per accessing thread. Live payload can be uninitialized.

Size: Declared slot or lazy wrapper layout plus initialized owned allocations.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 1 wrapper slot | Wrapper implementation and payload layout required | Payload allocations and any lock/waiter allocations. Once and lazy payloads can be uninitialized |

## Cross-Page References

- `DeployPreparationError: Serialization.0 -> KvStoreError [catalog-shared-007.svg]`
- `KeyValueBlockStore: pending -> PendingBlocks [catalog-block-storage-010.svg]`
- `KeyValueDeployStorage: value -> DeployStorageName [catalog-block-storage-007.svg]`
- `KeyValueDeployStorage: value -> KeyValueDeployStore [catalog-block-storage-007.svg]`
- `KeyValueRejectedDeployBuffer: value -> KeyValueDeployStore [catalog-block-storage-007.svg]`
- `KeyValueRejectedDeployBuffer: value -> RejectedDeployBufferName [catalog-block-storage-007.svg]`
- `PreparedInsertResult: Refused.0 -> PoolRefusal [catalog-block-storage-007.svg]`
