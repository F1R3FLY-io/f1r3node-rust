# catalog casper 055

[Inventory](inventory.md) | [PlantUML](catalog-casper-055.puml)

![catalog-casper-055](catalog-casper-055.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### ReplayCacheState

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `map` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `retained_bytes` | 1 scalar | 8 B | None |

### InMemoryReplayCache

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `state` | 1 wrapper slot | Wrapper implementation and payload layout required | Payload allocations and any lock/waiter allocations. Once and lazy payloads can be uninitialized |
| `max_entries` | 1 scalar | 8 B | None |
| `max_bytes` | 1 scalar | 8 B | None |

### ReplayFailure

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `InternalError.msg` | 0..n UTF-8 bytes when this variant is active | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `ReplayStatusMismatch.initial_failed` | 1 scalar when this variant is active | 1 B | None |
| `ReplayStatusMismatch.replay_failed` | 1 scalar when this variant is active | 1 B | None |
| `UnusedCOMMEvent.msg` | 0..n UTF-8 bytes when this variant is active | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `ReplayCostMismatch.initial_cost` | 1 scalar when this variant is active | 8 B | None |
| `ReplayCostMismatch.replay_cost` | 1 scalar when this variant is active | 8 B | None |
| `SystemDeployErrorMismatch.play_error` | 0..n UTF-8 bytes when this variant is active | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `SystemDeployErrorMismatch.replay_error` | 0..n UTF-8 bytes when this variant is active | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |

### MergeableStore

Cardinality: Alias, not a separate allocation. Count values of the target type.

Size: Same layout and allocations as target. Do not add alias bytes again.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### MergeableKey

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `state_hash` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `creator` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `seq_num` | 1 scalar | 4 B | None |

### ExploratoryDeployConfig

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `max_concurrent` | 1 scalar | 8 B | None |
| `phlo_limit` | 1 scalar | 8 B | None |
| `execution_timeout` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### ReplayLock

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `semaphore` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `consensus_waiters` | 1 atomic value | Target atomic layout required | None for the atomic value itself |
| `changed` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |

### ReplayPermit

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `permit` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |
| `changed` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |

## Cross-Page References

- `MergeableKey: state_hash -> StateHashSerde [catalog-models-027.svg]`
- `MergeableStore: value -> ByteVector [catalog-shared-002.svg]`
- `MergeableStore: value -> DeployMergeableData [catalog-rholang-028.svg]`
- `MergeableStore: value -> KeyValueTypedStoreImpl [catalog-shared-007.svg]`
- `ReplayCacheState: map -> ReplayCacheEntry [catalog-casper-054.svg]`
- `ReplayCacheState: map -> ReplayCacheKey [catalog-casper-054.svg]`
