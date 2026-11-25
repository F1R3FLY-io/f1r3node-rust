# catalog casper 013

[Inventory](inventory.md) | [PlantUML](catalog-casper-013.puml)

![catalog-casper-013](catalog-casper-013.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### OfInterestVerdict

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Fresh` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `AlreadyProcessed` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `WrongShard` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `WrongVersion` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `OldUnsolicited` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

### SenderAuthorization

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Authorized` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Unauthorized` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Unknown` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

### Intake

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `block_store` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `dag` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `view` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `approved_block` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### Strike

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `UnauthorizedSender` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Malformed` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `HashMismatch` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `InvalidSender` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

### StrikeOutcome

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Counted` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Ignored.until_ms` | 1 scalar when this variant is active | 8 B | None |

### PeerReputation

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `view` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `strikes` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `ignored_until` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |

### ReceiptRefusal

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Invalid.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `Overloaded` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `AlreadyKnown` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `KnownCheck.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### HashSlot

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `users` | 1 scalar | 8 B | None |
| `lock` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |

## Cross-Page References

- `Intake: approved_block -> BlockMessage [catalog-models-031.svg]`
- `Intake: block_store -> KeyValueBlockStore [catalog-block-storage-008.svg]`
- `Intake: dag -> BlockDagKeyValueStorage [catalog-block-storage-001.svg]`
- `Intake: view -> IngestView [catalog-casper-012.svg]`
- `PeerReputation: ignored_until -> PeerNode [catalog-comm-003.svg]`
- `PeerReputation: strikes -> PeerNode [catalog-comm-003.svg]`
- `PeerReputation: view -> IngestView [catalog-casper-012.svg]`
- `ReceiptRefusal: Invalid.0 -> IntegrityFailure [catalog-casper-008.svg]`
- `ReceiptRefusal: KnownCheck.0 -> CasperError [catalog-casper-030.svg]`
