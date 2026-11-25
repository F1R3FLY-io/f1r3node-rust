# catalog rspacepp 006

[Inventory](inventory.md) | [PlantUML](catalog-rspacepp-006.puml)

![catalog-rspacepp-006](catalog-rspacepp-006.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### RootRepository

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `roots_store` | 1 owned target | Pointer or fat pointer, depending on target type | I(target) and its owned allocations, plus allocator overhead. Zero-sized targets need no allocation |

### RootsStoreInstances

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| No declared fields | 0 field payloads | Compiler layout required | No field-owned allocations |

### RootsStoreInstance

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `store` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |

### ShardedMap

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `shards` | 1 owned target | Pointer or fat pointer, depending on target type | I(target) and its owned allocations, plus allocator overhead. Zero-sized targets need no allocation |

### HotStoreState

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `continuations` | 1 owned target | Pointer or fat pointer, depending on target type | I(target) and its owned allocations, plus allocator overhead. Zero-sized targets need no allocation |
| `installed_continuations` | 1 owned target | Pointer or fat pointer, depending on target type | I(target) and its owned allocations, plus allocator overhead. Zero-sized targets need no allocation |
| `data` | 1 owned target | Pointer or fat pointer, depending on target type | I(target) and its owned allocations, plus allocator overhead. Zero-sized targets need no allocation |
| `joins` | 1 owned target | Pointer or fat pointer, depending on target type | I(target) and its owned allocations, plus allocator overhead. Zero-sized targets need no allocation |
| `installed_joins` | 1 owned target | Pointer or fat pointer, depending on target type | I(target) and its owned allocations, plus allocator overhead. Zero-sized targets need no allocation |

### InMemHotStore

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `data` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `continuations` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `installed_continuations` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `joins` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `installed_joins` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `history_cache_continuations` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `history_cache_datums` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `history_cache_joins` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `history_cache_cont_items` | 1 atomic value | Target atomic layout required | None for the atomic value itself |
| `history_cache_data_items` | 1 atomic value | Target atomic layout required | None for the atomic value itself |
| `history_cache_joins_items` | 1 atomic value | Target atomic layout required | None for the atomic value itself |
| `checkpoint_lock` | 1 wrapper slot | Wrapper implementation and payload layout required | Payload allocations and any lock/waiter allocations. Once and lazy payloads can be uninitialized |
| `history_reader_base` | 1 owned target | Pointer or fat pointer, depending on target type | I(target) and its owned allocations, plus allocator overhead. Zero-sized targets need no allocation |

### LAST_EMIT_AT_MS

Cardinality: One slot per linked process instance. Lazy payload can be uninitialized.

Size: Declared slot or lazy wrapper layout plus initialized owned allocations.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 1 atomic value | Target atomic layout required | None for the atomic value itself |

### LAST_EMIT_AT_MS

Cardinality: One slot per linked process instance. Lazy payload can be uninitialized.

Size: Declared slot or lazy wrapper layout plus initialized owned allocations.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 1 atomic value | Target atomic layout required | None for the atomic value itself |

## Cross-Page References

- `HotStoreState: continuations -> WaitingContinuation [catalog-rspacepp-010.svg]`
- `HotStoreState: data -> Datum [catalog-rspacepp-009.svg]`
- `HotStoreState: installed_continuations -> WaitingContinuation [catalog-rspacepp-010.svg]`
- `InMemHotStore: continuations -> WaitingContinuation [catalog-rspacepp-010.svg]`
- `InMemHotStore: data -> Datum [catalog-rspacepp-009.svg]`
- `InMemHotStore: history_cache_continuations -> WaitingContinuation [catalog-rspacepp-010.svg]`
- `InMemHotStore: history_cache_datums -> Datum [catalog-rspacepp-009.svg]`
- `InMemHotStore: installed_continuations -> WaitingContinuation [catalog-rspacepp-010.svg]`
- `ShardedMap: shards -> V [catalog-casper-043.svg]`
