# catalog casper 008

[Inventory](inventory.md) | [PlantUML](catalog-casper-008.puml)

![catalog-casper-008](catalog-casper-008.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### Stores

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `dag` | 1 borrowed reference | Pointer or fat pointer. Referent layout required | No owned allocation. Referent: 1 value |
| `block_store` | 1 borrowed reference | Pointer or fat pointer. Referent layout required | No owned allocation. Referent: 1 value |

### IntakeLanes

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `awaited` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `local` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `unsolicited` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### IntakeJob

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Delivered.0` | 1 owned target when this variant is active | Pointer or fat pointer, depending on target type | I(target) and its owned allocations, plus allocator overhead. Zero-sized targets need no allocation |
| `Local.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### Owner

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `state` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `view` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `block_store` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `dag` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `connections` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `local` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `outbound` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `work` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `in_validation` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `width` | 1 scalar | 8 B | None |
| `state_root_fetch_tx` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |

### IntegrityFailure

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Malformed` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `HashMismatch` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

### CheckedBlock

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `0` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### BodyAt

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Disk` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Pending` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

### HeldMeta

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `hash` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `sender` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `height` | 1 scalar | 8 B | None |
| `deps` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `parents` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `justification_validators` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `deferred` | 1 scalar | 1 B | None |
| `body` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `size` | 1 scalar | 8 B | None |
| `trusted` | 1 scalar | 1 B | None |
| `bonded_at_anchor` | 1 scalar | 1 B | None |
| `solicited` | 1 scalar | 1 B | None |
| `pushed_by` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |

## Cross-Page References

- `CheckedBlock: 0 -> BlockMessage [catalog-models-031.svg]`
- `HeldMeta: deps -> BlockHash [catalog-models-027.svg]`
- `HeldMeta: hash -> BlockHash [catalog-models-027.svg]`
- `HeldMeta: parents -> BlockHash [catalog-models-027.svg]`
- `HeldMeta: pushed_by -> PeerNode [catalog-comm-003.svg]`
- `IntakeJob: Delivered.0 -> Delivery [catalog-casper-007.svg]`
- `IntakeJob: Local.0 -> BlockHash [catalog-models-027.svg]`
- `IntakeLanes: awaited -> Delivery [catalog-casper-007.svg]`
- `IntakeLanes: local -> BlockHash [catalog-models-027.svg]`
- `IntakeLanes: unsolicited -> Delivery [catalog-casper-007.svg]`
- `Owner: block_store -> KeyValueBlockStore [catalog-block-storage-008.svg]`
- `Owner: connections -> ConnectionsCell [catalog-comm-004.svg]`
- `Owner: dag -> BlockDagKeyValueStorage [catalog-block-storage-001.svg]`
- `Owner: in_validation -> BlockHash [catalog-models-027.svg]`
- `Owner: local -> BlockHash [catalog-models-027.svg]`
- `Owner: outbound -> Outbound [catalog-casper-007.svg]`
- `Owner: state -> IngestState [catalog-casper-012.svg]`
- `Owner: state_root_fetch_tx -> Blake2b256Hash [catalog-rspacepp-002.svg]`
- `Owner: view -> IngestView [catalog-casper-012.svg]`
- `Owner: work -> BlockHash [catalog-models-027.svg]`
- `Owner: work -> Work [catalog-casper-009.svg]`
- `Stores: block_store -> KeyValueBlockStore [catalog-block-storage-008.svg]`
- `Stores: dag -> BlockDagKeyValueStorage [catalog-block-storage-001.svg]`
