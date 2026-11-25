# catalog models 033

[Inventory](inventory.md) | [PlantUML](catalog-models-033.puml)

![catalog-models-033](catalog-models-033.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### ProduceEvent

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `channels_hash` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `hash` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `persistent` | 1 scalar | 1 B | None |
| `times_repeated` | 1 scalar | 4 B | None |
| `is_deterministic` | 1 scalar | 1 B | None |
| `output_value` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `failed` | 1 scalar | 1 B | None |

### ConsumeEvent

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `channels_hashes` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `hash` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `persistent` | 1 scalar | 1 B | None |

### CommEvent

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `consume` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `produces` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `peeks` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |

### Bond

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `validator` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `stake` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### StoreNodeKey

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `hash` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `index` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |

### StoreItemsMessageRequest

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `start_path` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Sum of component-owned allocations |
| `skip` | 1 scalar | 4 B | None |
| `take` | 1 scalar | 4 B | None |

### StoreItemsMessage

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `start_path` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Sum of component-owned allocations |
| `last_path` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Sum of component-owned allocations |
| `history_items` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Sum of component-owned allocations |
| `data_items` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Sum of component-owned allocations |

### MessageClass

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Pull` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Push` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

## Cross-Page References

- `Bond: stake -> TokenAmount [catalog-shared-009.svg]`
- `Bond: validator -> ByteString [catalog-models-027.svg]`
- `CommEvent: peeks -> Peek [catalog-models-032.svg]`
- `ConsumeEvent: channels_hashes -> ByteString [catalog-models-027.svg]`
- `ConsumeEvent: hash -> ByteString [catalog-models-027.svg]`
- `ProduceEvent: channels_hash -> ByteString [catalog-models-027.svg]`
- `ProduceEvent: hash -> ByteString [catalog-models-027.svg]`
- `ProduceEvent: output_value -> ByteString [catalog-models-027.svg]`
- `StoreItemsMessage: data_items -> Blake2b256Hash [catalog-rspacepp-002.svg]`
- `StoreItemsMessage: data_items -> ByteString [catalog-models-027.svg]`
- `StoreItemsMessage: history_items -> Blake2b256Hash [catalog-rspacepp-002.svg]`
- `StoreItemsMessage: history_items -> ByteString [catalog-models-027.svg]`
- `StoreItemsMessage: last_path -> Blake2b256Hash [catalog-rspacepp-002.svg]`
- `StoreItemsMessage: last_path -> Byte [catalog-shared-002.svg]`
- `StoreItemsMessage: start_path -> Blake2b256Hash [catalog-rspacepp-002.svg]`
- `StoreItemsMessage: start_path -> Byte [catalog-shared-002.svg]`
- `StoreItemsMessageRequest: start_path -> Blake2b256Hash [catalog-rspacepp-002.svg]`
- `StoreItemsMessageRequest: start_path -> Byte [catalog-shared-002.svg]`
- `StoreNodeKey: hash -> Blake2b256Hash [catalog-rspacepp-002.svg]`
- `StoreNodeKey: index -> Byte [catalog-shared-002.svg]`
