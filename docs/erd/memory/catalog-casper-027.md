# catalog casper 027

[Inventory](inventory.md) | [PlantUML](catalog-casper-027.puml)

![catalog-casper-027](catalog-casper-027.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### PendingRoot

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `started` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `outstanding` | 0..n distinct elements | Container and hasher implementation required | Element storage, owned allocations, spare capacity and index overhead. No type-level count cap |
| `chunks_imported` | 1 scalar | 8 B | None |
| `sent_at` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `resend_after` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### Core

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `pending` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `queued` | 0..n elements | Container implementation required | Element storage, owned allocations, spare capacity or per-node links. No type-level count cap |
| `path_to_root` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `fetched_total` | 1 scalar | 8 B | None |
| `importer` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `has_root` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `outcomes` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Sum of component-owned allocations |

### BootstrapChunkSender

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `transport` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `rp_conf` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### StateRequesterHandles

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `fetch_tx` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `items_tx` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `results` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### ChunkRequestState

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `chunk_index` | 1 scalar | 4 B | None |
| `last_request_ms` | 1 scalar | 8 B | None |
| `initial_request_ms` | 1 scalar | 8 B | None |
| `peers_tried` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: None |
| `retry_count` | 1 scalar | 4 B | None |
| `bytes` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 0..n elements. capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: None |

### SnapshotTarget

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `block_hash` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: None |
| `merkle_root` | 32 elements | 32 B. 32 * I(element), inline array | Sum of element-owned allocations. Element: None |
| `chunk_count` | 1 scalar | 4 B | None |

### AdmitOutcome

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `ChunkAccepted` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `UnknownRequest` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `WrongTarget` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `MerkleRootMismatch` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `ChunkHashMismatch` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `MerkleProofInvalid` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `ChunkCountMismatch` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

### SnapshotChunkRetriever

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `target` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `chunks` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |

## Cross-Page References

- `BootstrapChunkSender: rp_conf -> RPConf [catalog-comm-004.svg]`
- `Core: has_root -> HasRootFn [catalog-casper-023.svg]`
- `Core: outcomes -> Blake2b256Hash [catalog-rspacepp-002.svg]`
- `Core: outcomes -> StateFetchResult [catalog-casper-012.svg]`
- `Core: path_to_root -> Blake2b256Hash [catalog-rspacepp-002.svg]`
- `Core: path_to_root -> StatePartPath [catalog-casper-024.svg]`
- `Core: pending -> Blake2b256Hash [catalog-rspacepp-002.svg]`
- `Core: queued -> Blake2b256Hash [catalog-rspacepp-002.svg]`
- `PendingRoot: outstanding -> StatePartPath [catalog-casper-024.svg]`
- `StateRequesterHandles: fetch_tx -> Blake2b256Hash [catalog-rspacepp-002.svg]`
- `StateRequesterHandles: items_tx -> StoreItemsMessage [catalog-models-033.svg]`
- `StateRequesterHandles: results -> Blake2b256Hash [catalog-rspacepp-002.svg]`
- `StateRequesterHandles: results -> StateFetchResult [catalog-casper-012.svg]`
