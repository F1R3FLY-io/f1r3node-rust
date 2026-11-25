# catalog casper 025

[Inventory](inventory.md) | [PlantUML](catalog-casper-025.puml)

![catalog-casper-025](catalog-casper-025.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### TupleSpaceStreamProcessor

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `request_ops` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `state_importer` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `st` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `request_tx` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### FinalizedBondCache

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `validator` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |
| `bonded` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |

### DivergenceMonitor

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `state` | 1 wrapper slot | Wrapper implementation and payload layout required | Payload allocations and any lock/waiter allocations. Once and lazy payloads can be uninitialized |
| `diverged` | 1 atomic value | Target atomic layout required | None for the atomic value itself |

### HoldStreak

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `pinned_lfb` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |
| `refusals` | 1 scalar | 8 B | None |
| `first_derived_number` | 1 scalar | 8 B | None |
| `latest_derived_number` | 1 scalar | 8 B | None |

### FinalizationContext

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `observer` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `block_dag_storage` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `block_store` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `runtime_manager` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `event_publisher` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `divergence_monitor` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `deploy_storage` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `finalized_bond_cache` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `shard_conf` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### CycleFuture

Cardinality: Alias, not a separate allocation. Count values of the target type.

Size: Same layout and allocations as target. Do not add alias bytes again.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### Cycle

Cardinality: Alias, not a separate allocation. Count values of the target type.

Size: Same layout and allocations as target. Do not add alias bytes again.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 1 owned target | Pointer or fat pointer, depending on target type | I(target) and its owned allocations, plus allocator overhead. Zero-sized targets need no allocation |

### Finalizer

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `wake` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `requested` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `in_cycle` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `completed` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `_stop` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

## Cross-Page References

- `FinalizationContext: block_dag_storage -> BlockDagKeyValueStorage [catalog-block-storage-001.svg]`
- `FinalizationContext: block_store -> KeyValueBlockStore [catalog-block-storage-008.svg]`
- `FinalizationContext: deploy_storage -> KeyValueDeployStorage [catalog-block-storage-008.svg]`
- `FinalizationContext: event_publisher -> F1r3flyEvents [catalog-shared-005.svg]`
- `FinalizationContext: observer -> ObserverBinding [catalog-casper-044.svg]`
- `FinalizationContext: runtime_manager -> RuntimeManager [catalog-casper-056.svg]`
- `FinalizationContext: shard_conf -> CasperShardConf [catalog-casper-020.svg]`
- `HoldStreak: pinned_lfb -> BlockHash [catalog-models-027.svg]`
- `TupleSpaceStreamProcessor: st -> ST [catalog-casper-024.svg]`
- `TupleSpaceStreamProcessor: st -> StatePartPath [catalog-casper-024.svg]`
