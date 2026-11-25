# catalog casper 053

[Inventory](inventory.md) | [PlantUML](catalog-casper-053.puml)

![catalog-casper-053](catalog-casper-053.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### UnheldParent

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Surface` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `SkipSettled` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

### CheckBalance

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `pk` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `rand` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### CloseBlockDeploy

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `initial_rand` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### PreChargeDeploy

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `charge_amount` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `pk` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `rand` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### RefundDeploy

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `refund_amount` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `rand` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### SlashDeploy

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `evidence` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `pk` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `initial_rand` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### SigDisposition

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `latest` | 2 component values | Sum of component inline sizes plus compiler padding | Sum of component-owned allocations |
| `latest_kept_rejection` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 2 component values. Sum of component-owned allocations |
| `first_carrier` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 3 component values. Sum of component-owned allocations |

### DeploysCheckpoint

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `pre_state_hash` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `post_state_hash` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `deploys` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `rejected_deploys` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `system_deploys` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `bonds` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `applied_from_scope` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `merge_base` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |
| `block_hash_leaves` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Sum of element-owned allocations. Element: None |

## Cross-Page References

- `CheckBalance: rand -> Blake2b512Random [catalog-crypto-001.svg]`
- `CloseBlockDeploy: initial_rand -> Blake2b512Random [catalog-crypto-001.svg]`
- `DeploysCheckpoint: bonds -> Bond [catalog-models-033.svg]`
- `DeploysCheckpoint: deploys -> ProcessedDeploy [catalog-models-032.svg]`
- `DeploysCheckpoint: merge_base -> BlockHash [catalog-models-027.svg]`
- `DeploysCheckpoint: post_state_hash -> StateHash [catalog-models-027.svg]`
- `DeploysCheckpoint: pre_state_hash -> StateHash [catalog-models-027.svg]`
- `DeploysCheckpoint: rejected_deploys -> RejectedDeploy [catalog-models-031.svg]`
- `DeploysCheckpoint: system_deploys -> ProcessedSystemDeploy [catalog-models-032.svg]`
- `PreChargeDeploy: rand -> Blake2b512Random [catalog-crypto-001.svg]`
- `RefundDeploy: rand -> Blake2b512Random [catalog-crypto-001.svg]`
- `SigDisposition: first_carrier -> BlockHash [catalog-models-027.svg]`
- `SigDisposition: latest_kept_rejection -> BlockHash [catalog-models-027.svg]`
- `SlashDeploy: evidence -> EquivocationEvidence [catalog-models-032.svg]`
- `SlashDeploy: initial_rand -> Blake2b512Random [catalog-crypto-001.svg]`
