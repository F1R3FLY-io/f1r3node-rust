# catalog casper 021

[Inventory](inventory.md) | [PlantUML](catalog-casper-021.puml)

![catalog-casper-021](catalog-casper-021.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### SignatureWrapper

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `0` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### ApproveBlockProtocolFactory

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| No declared fields | 0 field payloads | Compiler layout required | No field-owned allocations |

### ApproveBlockProtocolImpl

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `required_sigs` | 1 scalar | 4 B | None |
| `start` | 1 scalar | 8 B | None |
| `duration` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `interval` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `sigs` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `last_approved_block` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `event_log` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |
| `transport` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `connections_cell` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 shared strong reference. One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `conf` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 shared strong reference. One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `trusted_validators` | 0..n distinct elements | Container and hasher implementation required | Element storage, owned allocations, spare capacity and index overhead. No type-level count cap |
| `candidate` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `candidate_hash` | 0..n UTF-8 bytes | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `sig_data` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: None |

### BlockApproverProtocol

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `validator_id` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `deploy_timestamp` | 1 scalar | 8 B | None |
| `vaults` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `bonds_bytes` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `minimum_bond` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `maximum_bond` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `epoch_length` | 1 scalar | 4 B | None |
| `quarantine_length` | 1 scalar | 4 B | None |
| `number_of_active_validators` | 1 scalar | 4 B | None |
| `fault_tolerance_threshold_ppm` | 1 scalar | 8 B | None |
| `max_parent_depth` | 1 scalar | 4 B | None |
| `deploy_lifespan` | 1 scalar | 8 B | None |
| `min_phlo_price` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `max_phlo_limit` | 1 scalar | 8 B | None |
| `required_sigs` | 1 scalar | 4 B | None |
| `pos_multi_sig_public_keys` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `pos_multi_sig_quorum` | 1 scalar | 4 B | None |
| `native_token_name` | 0..n UTF-8 bytes | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `native_token_symbol` | 0..n UTF-8 bytes | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `native_token_decimals` | 1 scalar | 4 B | None |
| `fs_bundle` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `consensus_fs_snapshot_cadence` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 scalar. None |
| `transport` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `conf` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |

### CasperLaunchImpl

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
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
| `casper_shard_conf` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `ingest` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `conf` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `trim_state` | 1 scalar | 1 B | None |
| `disable_state_exporter` | 1 scalar | 1 B | None |
| `heartbeat_signal_ref` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `fs_bundle` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `state_items_tx` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |

### NoopEngine

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| No declared fields | 0 field payloads | Compiler layout required | No field-owned allocations |

### EngineCell

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `inner` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `observer` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 shared strong reference. One shared allocation holds the target, counters and alignment. Count it once across all handles |

### EngineWithCasper

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `casper` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |

## Cross-Page References

- `ApproveBlockProtocolImpl: candidate -> ApprovedBlockCandidate [catalog-models-029.svg]`
- `ApproveBlockProtocolImpl: conf -> RPConf [catalog-comm-004.svg]`
- `ApproveBlockProtocolImpl: connections_cell -> ConnectionsCell [catalog-comm-004.svg]`
- `ApproveBlockProtocolImpl: event_log -> F1r3flyEvents [catalog-shared-005.svg]`
- `ApproveBlockProtocolImpl: last_approved_block -> ApprovedBlock [catalog-models-030.svg]`
- `BlockApproverProtocol: bonds_bytes -> TokenAmount [catalog-shared-009.svg]`
- `BlockApproverProtocol: conf -> RPConf [catalog-comm-004.svg]`
- `BlockApproverProtocol: fs_bundle -> BundleEntry [catalog-casper-034.svg]`
- `BlockApproverProtocol: maximum_bond -> TokenAmount [catalog-shared-009.svg]`
- `BlockApproverProtocol: min_phlo_price -> TokenAmount [catalog-shared-009.svg]`
- `BlockApproverProtocol: minimum_bond -> TokenAmount [catalog-shared-009.svg]`
- `BlockApproverProtocol: validator_id -> ValidatorIdentity [catalog-casper-058.svg]`
- `BlockApproverProtocol: vaults -> Vault [catalog-casper-036.svg]`
- `CasperLaunchImpl: block_dag_storage -> BlockDagKeyValueStorage [catalog-block-storage-001.svg]`
- `CasperLaunchImpl: block_store -> KeyValueBlockStore [catalog-block-storage-008.svg]`
- `CasperLaunchImpl: casper_shard_conf -> CasperShardConf [catalog-casper-020.svg]`
- `CasperLaunchImpl: conf -> CasperConf [catalog-casper-020.svg]`
- `CasperLaunchImpl: connections_cell -> ConnectionsCell [catalog-comm-004.svg]`
- `CasperLaunchImpl: deploy_storage -> KeyValueDeployStorage [catalog-block-storage-008.svg]`
- `CasperLaunchImpl: estimator -> Estimator [catalog-casper-031.svg]`
- `CasperLaunchImpl: event_publisher -> F1r3flyEvents [catalog-shared-005.svg]`
- `CasperLaunchImpl: fs_bundle -> BundleEntry [catalog-casper-034.svg]`
- `CasperLaunchImpl: heartbeat_signal_ref -> HeartbeatSignalRef [catalog-casper-036.svg]`
- `CasperLaunchImpl: ingest -> IngestHandle [catalog-casper-007.svg]`
- `CasperLaunchImpl: last_approved_block -> ApprovedBlock [catalog-models-030.svg]`
- `CasperLaunchImpl: rejected_deploy_buffer -> KeyValueRejectedDeployBuffer [catalog-block-storage-008.svg]`
- `CasperLaunchImpl: rp_conf_ask -> RPConf [catalog-comm-004.svg]`
- `CasperLaunchImpl: rspace_state_manager -> RSpaceStateManager [catalog-rspacepp-018.svg]`
- `CasperLaunchImpl: runtime_manager -> RuntimeManager [catalog-casper-056.svg]`
- `CasperLaunchImpl: state_items_tx -> StoreItemsMessage [catalog-models-033.svg]`
- `EngineCell: observer -> ObserverController [catalog-casper-045.svg]`
