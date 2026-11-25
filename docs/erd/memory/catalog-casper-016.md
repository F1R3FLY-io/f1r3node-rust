# catalog casper 016

[Inventory](inventory.md) | [PlantUML](catalog-casper-016.puml)

![catalog-casper-016](catalog-casper-016.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### DeployInclusionStaleness

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `stale` | 1 scalar | 1 B | None |
| `block_or_time_stale` | 1 scalar | 1 B | None |
| `signature_stale` | 1 scalar | 1 B | None |
| `missing_deploy_metadata` | 1 scalar | 1 B | None |

### OrdinaryCapSource

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Normal` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Base` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Backlog` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `SoftBacklog` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `SoftStalled` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Stale` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Hard` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

### FreshAdmissionFallback

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `allowed` | 1 scalar | 1 B | None |
| `cap` | 1 scalar | 8 B | None |
| `backpressure` | 1 scalar | 1 B | None |
| `source` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### DeploySelection

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `deploys` | 0..n distinct elements | Container and hasher implementation required | Element storage, owned allocations, spare capacity and index overhead. No type-level count cap |
| `strategy` | 1 borrowed reference | Pointer or fat pointer. Referent layout required | No owned allocation. Referent: 1 value |
| `selected_bytes` | 1 scalar | 8 B | None |
| `deferred_bytes` | 1 scalar | 8 B | None |
| `count_capped` | 1 scalar | 1 B | None |
| `byte_capped` | 1 scalar | 1 B | None |

### FinalityLagStats

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `dag_tip` | 1 scalar | 8 B | None |
| `last_finalized_block` | 1 scalar | 8 B | None |
| `lag` | 1 scalar | 8 B | None |
| `stalled` | 1 scalar | 1 B | None |

### FinalityProgress

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `last_finalized_block` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 scalar. None |
| `unadvanced_proposals` | 1 scalar | 4 B | None |

### ProposalShape

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `AsSnapshot.0` | 0..1 present payload when this variant is active | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |
| `Witness.snapshot` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `Witness.floor_ctx` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### UnfinalizedConeBounds

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `last_finalized_block_number` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 scalar. None |
| `earliest_block_number` | 1 scalar | 8 B | None |

## Cross-Page References

- `DeploySelection: deploys -> Signed [catalog-crypto-003.svg]`
- `ProposalShape: AsSnapshot.0 -> FloorContext [catalog-casper-033.svg]`
- `ProposalShape: Witness.floor_ctx -> FloorContext [catalog-casper-033.svg]`
- `ProposalShape: Witness.snapshot -> CasperSnapshot [catalog-casper-019.svg]`
