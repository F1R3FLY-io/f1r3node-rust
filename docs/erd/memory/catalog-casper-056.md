# catalog casper 056

[Inventory](inventory.md) | [PlantUML](catalog-casper-056.puml)

![catalog-casper-056](catalog-casper-056.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### ConsensusReplayWaiter

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `0` | 1 borrowed reference | Pointer or fat pointer. Referent layout required | No owned allocation. Referent: 1 value |

### RuntimeManager

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `space` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `replay_space` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `history_repo` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `mergeable_store` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `mergeable_tags` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `block_index_cache` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `active_validators_cache` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `bonds_cache` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `quarantined_cache` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `parents_post_state_cache` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `replay_cache` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 shared strong reference. One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `replay_lock` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `exploratory_deploy_semaphore` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `exploratory_deploy_phlo_limit` | 1 scalar | 8 B | None |
| `exploratory_deploy_execution_timeout` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `external_services` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `fs_snapshot_writer` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `snapshot_merkle_roots` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `payload_store` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `consensus_static_roots` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `root_registry` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `shared_lock_registry` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### ParentsPostStateCacheKey

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `sorted_parent_hashes` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `snapshot_lfb_hash` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `sorted_latest_messages` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Sum of component-owned allocations |
| `buffer_populated` | 1 scalar | 1 B | None |

### MergedPreState

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `state` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `rejected_user` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `applied_from_scope` | 0..n distinct elements | Container and hasher implementation required | Element storage, owned allocations, spare capacity and index overhead. No type-level count cap |
| `merge_base` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |

### ParentsPostStateCacheVal

Cardinality: Alias, not a separate allocation. Count values of the target type.

Size: Same layout and allocations as target. Do not add alias bytes again.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### SystemDeployEnum

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Slash.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `Close.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### SystemDeployResult

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `PlaySucceeded.state_hash` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `PlaySucceeded.processed_system_deploy` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `PlaySucceeded.mergeable_channels` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `PlaySucceeded.result` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `PlayFailed.processed_system_deploy` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### SystemDeployUserError

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `error_message` | 0..n UTF-8 bytes | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |

## Cross-Page References

- `ConsensusReplayWaiter: 0 -> ReplayLock [catalog-casper-055.svg]`
- `MergedPreState: merge_base -> BlockHash [catalog-models-027.svg]`
- `MergedPreState: rejected_user -> RejectedDeploy [catalog-models-031.svg]`
- `MergedPreState: state -> StateHash [catalog-models-027.svg]`
- `ParentsPostStateCacheKey: snapshot_lfb_hash -> BlockHash [catalog-models-027.svg]`
- `ParentsPostStateCacheKey: sorted_latest_messages -> BlockHash [catalog-models-027.svg]`
- `ParentsPostStateCacheKey: sorted_parent_hashes -> BlockHash [catalog-models-027.svg]`
- `RuntimeManager: active_validators_cache -> BoundedCache [catalog-casper-050.svg]`
- `RuntimeManager: active_validators_cache -> StateHash [catalog-models-027.svg]`
- `RuntimeManager: block_index_cache -> BlockHash [catalog-models-027.svg]`
- `RuntimeManager: block_index_cache -> BlockIndex [catalog-casper-037.svg]`
- `RuntimeManager: block_index_cache -> BoundedCache [catalog-casper-050.svg]`
- `RuntimeManager: bonds_cache -> Bond [catalog-models-033.svg]`
- `RuntimeManager: bonds_cache -> BoundedCache [catalog-casper-050.svg]`
- `RuntimeManager: bonds_cache -> StateHash [catalog-models-027.svg]`
- `RuntimeManager: external_services -> ExternalServices [catalog-rholang-006.svg]`
- `RuntimeManager: fs_snapshot_writer -> SnapshotWriter [catalog-rholang-024.svg]`
- `RuntimeManager: history_repo -> RhoHistoryRepository [catalog-rholang-040.svg]`
- `RuntimeManager: mergeable_store -> MergeableStore [catalog-casper-055.svg]`
- `RuntimeManager: mergeable_tags -> MergeType [catalog-rspacepp-011.svg]`
- `RuntimeManager: parents_post_state_cache -> BoundedCache [catalog-casper-050.svg]`
- `RuntimeManager: payload_store -> PayloadStoreBundle [catalog-casper-029.svg]`
- `RuntimeManager: quarantined_cache -> Bond [catalog-models-033.svg]`
- `RuntimeManager: quarantined_cache -> BoundedCache [catalog-casper-050.svg]`
- `RuntimeManager: quarantined_cache -> StateHash [catalog-models-027.svg]`
- `RuntimeManager: replay_cache -> InMemoryReplayCache [catalog-casper-055.svg]`
- `RuntimeManager: replay_lock -> ReplayLock [catalog-casper-055.svg]`
- `RuntimeManager: replay_space -> ReplayRSpace [catalog-rspacepp-012.svg]`
- `RuntimeManager: root_registry -> RootIdentityRegistry [catalog-rholang-023.svg]`
- `RuntimeManager: shared_lock_registry -> SharedLockRegistry [catalog-rholang-022.svg]`
- `RuntimeManager: space -> RSpace [catalog-rspacepp-014.svg]`
- `SystemDeployEnum: Close.0 -> CloseBlockDeploy [catalog-casper-053.svg]`
- `SystemDeployEnum: Slash.0 -> SlashDeploy [catalog-casper-053.svg]`
- `SystemDeployResult: PlayFailed.processed_system_deploy -> ProcessedSystemDeploy [catalog-models-032.svg]`
- `SystemDeployResult: PlaySucceeded.mergeable_channels -> NumberChannelsEndVal [catalog-rspacepp-011.svg]`
- `SystemDeployResult: PlaySucceeded.processed_system_deploy -> ProcessedSystemDeploy [catalog-models-032.svg]`
- `SystemDeployResult: PlaySucceeded.state_hash -> StateHash [catalog-models-027.svg]`
