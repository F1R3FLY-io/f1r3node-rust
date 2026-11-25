# catalog casper 022

[Inventory](inventory.md) | [PlantUML](catalog-casper-022.puml)

![catalog-casper-022](catalog-casper-022.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### GenesisCeremonyMaster

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `approve_protocol` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `transport_layer` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `rp_conf_ask` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### GenesisValidator

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `ingest` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `casper_shard_conf` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `validator_id` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `block_approver` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `transport_layer` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `rp_conf_ask` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `connections_cell` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `last_approved_block` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `event_publisher` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `engine_cell` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `block_store` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `block_dag_storage` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `deploy_storage` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `rejected_deploy_buffer` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `rspace_state_manager` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `runtime_manager` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `estimator` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `seen_candidates` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `approved_block_pull_last` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `heartbeat_signal_ref` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `state_items_tx` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |

### SeenCandidates

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `set` | 0..n distinct elements | Container and hasher implementation required | Element storage, owned allocations, spare capacity and index overhead. No type-level count cap |
| `fifo` | 0..n elements | Container implementation required | Element storage, owned allocations, spare capacity or per-node links. No type-level count cap |
| `max_entries` | 1 scalar | 8 B | None |

### Initializing

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `transport_layer` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `rp_conf_ask` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `connections_cell` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `last_approved_block` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `block_store` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `block_dag_storage` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `deploy_storage` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `rejected_deploy_buffer` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `rspace_state_manager` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `ingest` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `casper_shard_conf` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `validator_id` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |
| `the_init` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `block_message_rx` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `tuple_space_rx` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `block_message_tx` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `tuple_space_tx` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `block_message_queue_pending` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `tuple_space_queue_pending` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `trim_state` | 1 scalar | 1 B | None |
| `disable_state_exporter` | 1 scalar | 1 B | None |
| `start_requester` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `init_started_at` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `no_approved_block_retries` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `restore_failures` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `event_publisher` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `engine_cell` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `runtime_manager` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `estimator` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `heartbeat_signal_ref` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `state_items_tx` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |
| `floor_cache_tx` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `floor_cache_rx` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `block_pruned_tx` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `block_pruned_rx` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `stored_approved_block` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |

### BlockRequesterWrapper

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `transport_layer` | 1 borrowed reference | Pointer or fat pointer. Referent layout required | No owned allocation. Referent: 1 value |
| `connections_cell` | 1 borrowed reference | Pointer or fat pointer. Referent layout required | No owned allocation. Referent: 1 value |
| `rp_conf_ask` | 1 borrowed reference | Pointer or fat pointer. Referent layout required | No owned allocation. Referent: 1 value |
| `block_store` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `validate_block_fn` | 1 owned target | Pointer or fat pointer, depending on target type | I(target) and its owned allocations, plus allocator overhead. Zero-sized targets need no allocation |

### TupleSpaceRequester

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `transport_layer` | 1 borrowed reference | Pointer or fat pointer. Referent layout required | No owned allocation. Referent: 1 value |
| `rp_conf_ask` | 1 borrowed reference | Pointer or fat pointer. Referent layout required | No owned allocation. Referent: 1 value |

### HorizonRequester

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `transport_layer` | 1 borrowed reference | Pointer or fat pointer. Referent layout required | No owned allocation. Referent: 1 value |
| `rp_conf_ask` | 1 borrowed reference | Pointer or fat pointer. Referent layout required | No owned allocation. Referent: 1 value |

### ReqStatus

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Init` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Requested` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Received` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

## Cross-Page References

- `BlockRequesterWrapper: block_store -> KeyValueBlockStore [catalog-block-storage-008.svg]`
- `BlockRequesterWrapper: connections_cell -> ConnectionsCell [catalog-comm-004.svg]`
- `BlockRequesterWrapper: rp_conf_ask -> RPConf [catalog-comm-004.svg]`
- `BlockRequesterWrapper: validate_block_fn -> BlockMessage [catalog-models-031.svg]`
- `GenesisCeremonyMaster: approve_protocol -> ApproveBlockProtocolImpl [catalog-casper-021.svg]`
- `GenesisCeremonyMaster: rp_conf_ask -> RPConf [catalog-comm-004.svg]`
- `GenesisValidator: block_approver -> BlockApproverProtocol [catalog-casper-021.svg]`
- `GenesisValidator: block_dag_storage -> BlockDagKeyValueStorage [catalog-block-storage-001.svg]`
- `GenesisValidator: block_store -> KeyValueBlockStore [catalog-block-storage-008.svg]`
- `GenesisValidator: casper_shard_conf -> CasperShardConf [catalog-casper-020.svg]`
- `GenesisValidator: connections_cell -> ConnectionsCell [catalog-comm-004.svg]`
- `GenesisValidator: deploy_storage -> KeyValueDeployStorage [catalog-block-storage-008.svg]`
- `GenesisValidator: engine_cell -> EngineCell [catalog-casper-021.svg]`
- `GenesisValidator: estimator -> Estimator [catalog-casper-031.svg]`
- `GenesisValidator: event_publisher -> F1r3flyEvents [catalog-shared-005.svg]`
- `GenesisValidator: heartbeat_signal_ref -> HeartbeatSignalRef [catalog-casper-036.svg]`
- `GenesisValidator: ingest -> IngestHandle [catalog-casper-007.svg]`
- `GenesisValidator: last_approved_block -> ApprovedBlock [catalog-models-030.svg]`
- `GenesisValidator: rejected_deploy_buffer -> KeyValueRejectedDeployBuffer [catalog-block-storage-008.svg]`
- `GenesisValidator: rp_conf_ask -> RPConf [catalog-comm-004.svg]`
- `GenesisValidator: rspace_state_manager -> RSpaceStateManager [catalog-rspacepp-018.svg]`
- `GenesisValidator: runtime_manager -> RuntimeManager [catalog-casper-056.svg]`
- `GenesisValidator: state_items_tx -> StoreItemsMessage [catalog-models-033.svg]`
- `GenesisValidator: validator_id -> ValidatorIdentity [catalog-casper-058.svg]`
- `HorizonRequester: rp_conf_ask -> RPConf [catalog-comm-004.svg]`
- `Initializing: block_dag_storage -> BlockDagKeyValueStorage [catalog-block-storage-001.svg]`
- `Initializing: block_message_rx -> BlockMessage [catalog-models-031.svg]`
- `Initializing: block_message_tx -> BlockMessage [catalog-models-031.svg]`
- `Initializing: block_pruned_rx -> BlockPrunedResponse [catalog-models-028.svg]`
- `Initializing: block_pruned_tx -> BlockPrunedResponse [catalog-models-028.svg]`
- `Initializing: block_store -> KeyValueBlockStore [catalog-block-storage-008.svg]`
- `Initializing: casper_shard_conf -> CasperShardConf [catalog-casper-020.svg]`
- `Initializing: connections_cell -> ConnectionsCell [catalog-comm-004.svg]`
- `Initializing: deploy_storage -> KeyValueDeployStorage [catalog-block-storage-008.svg]`
- `Initializing: engine_cell -> EngineCell [catalog-casper-021.svg]`
- `Initializing: estimator -> Estimator [catalog-casper-031.svg]`
- `Initializing: event_publisher -> F1r3flyEvents [catalog-shared-005.svg]`
- `Initializing: floor_cache_rx -> FloorCacheResponse [catalog-models-030.svg]`
- `Initializing: floor_cache_tx -> FloorCacheResponse [catalog-models-030.svg]`
- `Initializing: heartbeat_signal_ref -> HeartbeatSignalRef [catalog-casper-036.svg]`
- `Initializing: ingest -> IngestHandle [catalog-casper-007.svg]`
- `Initializing: last_approved_block -> ApprovedBlock [catalog-models-030.svg]`
- `Initializing: rejected_deploy_buffer -> KeyValueRejectedDeployBuffer [catalog-block-storage-008.svg]`
- `Initializing: rp_conf_ask -> RPConf [catalog-comm-004.svg]`
- `Initializing: rspace_state_manager -> RSpaceStateManager [catalog-rspacepp-018.svg]`
- `Initializing: runtime_manager -> RuntimeManager [catalog-casper-056.svg]`
- `Initializing: state_items_tx -> StoreItemsMessage [catalog-models-033.svg]`
- `Initializing: stored_approved_block -> ApprovedBlock [catalog-models-030.svg]`
- `Initializing: the_init -> CasperError [catalog-casper-030.svg]`
- `Initializing: tuple_space_rx -> StoreItemsMessage [catalog-models-033.svg]`
- `Initializing: tuple_space_tx -> StoreItemsMessage [catalog-models-033.svg]`
- `Initializing: validator_id -> ValidatorIdentity [catalog-casper-058.svg]`
- `SeenCandidates: fifo -> BlockHash [catalog-models-027.svg]`
- `SeenCandidates: set -> BlockHash [catalog-models-027.svg]`
- `TupleSpaceRequester: rp_conf_ask -> RPConf [catalog-comm-004.svg]`
