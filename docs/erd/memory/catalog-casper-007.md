# catalog casper 007

[Inventory](inventory.md) | [PlantUML](catalog-casper-007.puml)

![catalog-casper-007](catalog-casper-007.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### SharedCasper

Cardinality: Alias, not a separate allocation. Count values of the target type.

Size: Same layout and allocations as target. Do not add alias bytes again.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |

### Delivery

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `block` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `from` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### Outbound

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Request.hash` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `Request.peer` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `Broadcast.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### Attached

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `casper` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `intake` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |

### AttachedSlot

Cardinality: Alias, not a separate allocation. Count values of the target type.

Size: Same layout and allocations as target. Do not add alias bytes again.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |

### IngestHandle

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `view` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `awaited` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `unsolicited` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `inputs` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `attached` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `block_store` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `dag` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### IngestPorts

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `awaited` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `unsolicited` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `inputs` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `inserted` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### IngestSetup

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `block_store` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `dag` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `transport` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `connections` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `conf` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `validation_width` | 1 scalar | 8 B | None |
| `max_number_of_parents` | 1 scalar | 4 B | None |
| `state_root_fetch_tx` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |
| `state_root_results` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |

## Cross-Page References

- `Attached: intake -> Intake [catalog-casper-013.svg]`
- `Delivery: block -> CheckedBlock [catalog-casper-008.svg]`
- `Delivery: from -> PeerNode [catalog-comm-003.svg]`
- `IngestHandle: block_store -> KeyValueBlockStore [catalog-block-storage-008.svg]`
- `IngestHandle: dag -> BlockDagKeyValueStorage [catalog-block-storage-001.svg]`
- `IngestHandle: inputs -> IngestInput [catalog-casper-012.svg]`
- `IngestHandle: view -> IngestView [catalog-casper-012.svg]`
- `IngestPorts: inputs -> IngestInput [catalog-casper-012.svg]`
- `IngestPorts: inserted -> BlockHash [catalog-models-027.svg]`
- `IngestSetup: block_store -> KeyValueBlockStore [catalog-block-storage-008.svg]`
- `IngestSetup: conf -> RPConf [catalog-comm-004.svg]`
- `IngestSetup: connections -> ConnectionsCell [catalog-comm-004.svg]`
- `IngestSetup: dag -> BlockDagKeyValueStorage [catalog-block-storage-001.svg]`
- `IngestSetup: state_root_fetch_tx -> Blake2b256Hash [catalog-rspacepp-002.svg]`
- `IngestSetup: state_root_results -> Blake2b256Hash [catalog-rspacepp-002.svg]`
- `IngestSetup: state_root_results -> StateFetchResult [catalog-casper-012.svg]`
- `Outbound: Broadcast.0 -> BlockHash [catalog-models-027.svg]`
- `Outbound: Request.hash -> BlockHash [catalog-models-027.svg]`
- `Outbound: Request.peer -> PeerNode [catalog-comm-003.svg]`
