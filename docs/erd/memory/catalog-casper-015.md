# catalog casper 015

[Inventory](inventory.md) | [PlantUML](catalog-casper-015.puml)

![catalog-casper-015](catalog-casper-015.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### BlockAnnouncer

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `transport` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `connections` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `conf` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `permits` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |

### Validation

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `block_store` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `dag` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `announcer` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `settled_admissions` | 1 atomic value | Target atomic layout required | None for the atomic value itself |
| `state_root_fetch_tx` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |

### PreparedUserDeploys

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `deploys` | 0..n distinct elements | Container and hasher implementation required | Element storage, owned allocations, spare capacity and index overhead. No type-level count cap |
| `effective_cap` | 1 scalar | 8 B | None |
| `cap_hit` | 1 scalar | 1 B | None |
| `selected_retry_count` | 1 scalar | 8 B | None |
| `selected_ordinary_count` | 1 scalar | 8 B | None |
| `selected_in_scope_recovery_count` | 1 scalar | 8 B | None |
| `selected_in_scope_recovery_sigs` | 0..n distinct elements | Container and hasher implementation required | Element storage, owned allocations, spare capacity and index overhead. No type-level count cap |
| `already_in_scope_count` | 1 scalar | 8 B | None |
| `selected_user_deploy_bytes` | 1 scalar | 8 B | None |
| `deferred_user_deploy_bytes` | 1 scalar | 8 B | None |
| `byte_cap_hit` | 1 scalar | 1 B | None |

### DeployAdmissionPolicy

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `allow_ordinary` | 1 scalar | 1 B | None |
| `ordinary_cap` | 1 scalar | 8 B | None |
| `allow_in_scope_recovery` | 1 scalar | 1 B | None |
| `in_scope_recovery_cap` | 1 scalar | 8 B | None |
| `reserve_tail` | 1 scalar | 1 B | None |
| `fallback` | 1 scalar | 1 B | None |
| `backpressure` | 1 scalar | 1 B | None |
| `ordinary_cap_source` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `in_scope_recovery_cap_source` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### BranchDeployInfo

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `block_hash` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `sender` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `block_number` | 1 scalar | 8 B | None |
| `timestamp` | 1 scalar | 8 B | None |
| `deploy_sig_count` | 1 scalar | 8 B | None |
| `new_sig_count` | 1 scalar | 8 B | None |
| `recycled_sig_count` | 1 scalar | 8 B | None |

### DeployInclusionProgress

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `leader` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |
| `latest_deploy` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |

### FreshLocalDeployStats

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `count` | 1 scalar | 8 B | None |
| `oldest_age_millis` | 1 scalar | 8 B | None |

### InScopeLocalDeployStats

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `count` | 1 scalar | 8 B | None |
| `oldest_age_millis` | 1 scalar | 8 B | None |
| `stranded_count` | 1 scalar | 8 B | None |

## Cross-Page References

- `BlockAnnouncer: conf -> RPConf [catalog-comm-004.svg]`
- `BlockAnnouncer: connections -> ConnectionsCell [catalog-comm-004.svg]`
- `BranchDeployInfo: block_hash -> BlockHash [catalog-models-027.svg]`
- `DeployAdmissionPolicy: in_scope_recovery_cap_source -> OrdinaryCapSource [catalog-casper-016.svg]`
- `DeployAdmissionPolicy: ordinary_cap_source -> OrdinaryCapSource [catalog-casper-016.svg]`
- `PreparedUserDeploys: deploys -> Signed [catalog-crypto-003.svg]`
- `Validation: block_store -> KeyValueBlockStore [catalog-block-storage-008.svg]`
- `Validation: dag -> BlockDagKeyValueStorage [catalog-block-storage-001.svg]`
- `Validation: state_root_fetch_tx -> Blake2b256Hash [catalog-rspacepp-002.svg]`
