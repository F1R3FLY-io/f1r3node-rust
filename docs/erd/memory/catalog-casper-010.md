# catalog casper 010

[Inventory](inventory.md) | [PlantUML](catalog-casper-010.puml)

![catalog-casper-010](catalog-casper-010.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### FetchAction

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `LookupLocal.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `RequestBlock.hash` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `RequestBlock.peer` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `BroadcastHasBlockRequest.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `FetchStateRoot.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### NeedFacts

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `height_hint` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 scalar. None |
| `trusted` | 1 scalar | 1 B | None |
| `vouched` | 1 scalar | 1 B | None |

### NeedOrigin

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Cited` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Requested` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Announced` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

### Priority

Cardinality: Alias, not a separate allocation. Count values of the target type.

Size: Same layout and allocations as target. Do not add alias bytes again.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 3 component values | Sum of component inline sizes plus compiler padding | Sum of component-owned allocations |

### Stage

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `LocalLookup.issued` | 1 scalar when this variant is active | 1 B | None |
| `Network` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

### BlockNeed

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `facts` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `origin` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `stage` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `candidates` | 0..n elements | Container implementation required | Element storage, owned allocations, spare capacity or per-node links. No type-level count cap |
| `asked` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `announced` | 1 scalar | 1 B | None |
| `requery_cursor` | 1 scalar | 8 B | None |
| `attempts` | 1 scalar | 4 B | None |
| `next_due_ms` | 1 scalar | 8 B | None |
| `first_requested_ms` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 scalar. None |
| `last_requested_ms` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 scalar. None |
| `pushed_by` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |
| `seq` | 1 scalar | 8 B | None |

### RootNeed

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `attempts` | 1 scalar | 4 B | None |
| `next_due_ms` | 1 scalar | 8 B | None |
| `in_flight` | 1 scalar | 1 B | None |

### RemovedNeed

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `facts` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `origin` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `pushed_by` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |
| `first_requested_ms` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 scalar. None |

## Cross-Page References

- `BlockNeed: asked -> PeerNode [catalog-comm-003.svg]`
- `BlockNeed: candidates -> PeerNode [catalog-comm-003.svg]`
- `BlockNeed: pushed_by -> PeerNode [catalog-comm-003.svg]`
- `FetchAction: BroadcastHasBlockRequest.0 -> BlockHash [catalog-models-027.svg]`
- `FetchAction: FetchStateRoot.0 -> Blake2b256Hash [catalog-rspacepp-002.svg]`
- `FetchAction: LookupLocal.0 -> BlockHash [catalog-models-027.svg]`
- `FetchAction: RequestBlock.hash -> BlockHash [catalog-models-027.svg]`
- `FetchAction: RequestBlock.peer -> PeerNode [catalog-comm-003.svg]`
- `RemovedNeed: pushed_by -> PeerNode [catalog-comm-003.svg]`
