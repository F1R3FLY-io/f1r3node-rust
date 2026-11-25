# catalog casper 019

[Inventory](inventory.md) | [PlantUML](catalog-casper-019.puml)

![catalog-casper-019](catalog-casper-019.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### ProductionStakeChecker

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `validator` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |

### ProductionHeightChecker

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `validator` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |

### ProductionBlockCreator

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `deploy_storage` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `rejected_deploy_buffer` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `runtime_manager` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `block_store` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `finality_progress` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### ProductionBlockValidator

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| No declared fields | 0 field payloads | Compiler layout required | No field-owned allocations |

### ProductionProposeEffectHandler

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `block_store` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `transport` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `connections_cell` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `conf` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `event_publisher` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### DeployError

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `ParsingError.0` | 0..n UTF-8 bytes when this variant is active | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `MissingUser` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `UnknownSignatureAlgorithm.0` | 0..n UTF-8 bytes when this variant is active | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `SignatureVerificationFailed` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `DuplicateDeploy.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `SeedReuse.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `InvalidFreshness` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `PhloLimitAboveCap.phlo_limit` | 1 scalar when this variant is active | 8 B | None |
| `PhloLimitAboveCap.max_phlo_limit` | 1 scalar when this variant is active | 8 B | None |
| `RecordTooLarge.record_bytes` | 1 scalar when this variant is active | 8 B | None |
| `RecordTooLarge.limit` | 1 scalar when this variant is active | 8 B | None |
| `PoolBusy.resource` | 1 borrowed reference when this variant is active | Pointer or fat pointer. Referent layout required | No owned allocation. Referent: 1 value |
| `PoolBusy.required` | 1 scalar when this variant is active | 8 B | None |
| `PoolBusy.limit` | 1 scalar when this variant is active | 8 B | None |
| `AdmissionBusy` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

### CasperSnapshot

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `dag` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `last_finalized_block` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `parents` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `justifications` | 0..n distinct elements | Container and hasher implementation required | Element storage, owned allocations, spare capacity and index overhead. No type-level count cap |
| `invalid_blocks` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `deploys_in_scope` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `rejected_in_scope` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `seeds_in_scope` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `max_block_num` | 1 scalar | 8 B | None |
| `max_seq_nums` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `on_chain_state` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `chain_id` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### OnChainCasperState

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `shard_conf` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `bonds_map` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `active_validators` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |

## Cross-Page References

- `CasperSnapshot: chain_id -> BlockHash [catalog-models-027.svg]`
- `CasperSnapshot: dag -> KeyValueDagRepresentation [catalog-block-storage-001.svg]`
- `CasperSnapshot: invalid_blocks -> BlockHash [catalog-models-027.svg]`
- `CasperSnapshot: justifications -> Justification [catalog-models-031.svg]`
- `CasperSnapshot: last_finalized_block -> BlockHash [catalog-models-027.svg]`
- `CasperSnapshot: parents -> BlockMessage [catalog-models-031.svg]`
- `DeployError: DuplicateDeploy.0 -> DeployId [catalog-block-storage-001.svg]`
- `DeployError: SeedReuse.0 -> DeployId [catalog-block-storage-001.svg]`
- `OnChainCasperState: shard_conf -> CasperShardConf [catalog-casper-020.svg]`
- `ProductionBlockCreator: block_store -> KeyValueBlockStore [catalog-block-storage-008.svg]`
- `ProductionBlockCreator: deploy_storage -> KeyValueDeployStorage [catalog-block-storage-008.svg]`
- `ProductionBlockCreator: rejected_deploy_buffer -> KeyValueRejectedDeployBuffer [catalog-block-storage-008.svg]`
- `ProductionBlockCreator: runtime_manager -> RuntimeManager [catalog-casper-056.svg]`
- `ProductionHeightChecker: validator -> ValidatorIdentity [catalog-casper-058.svg]`
- `ProductionProposeEffectHandler: block_store -> KeyValueBlockStore [catalog-block-storage-008.svg]`
- `ProductionProposeEffectHandler: conf -> RPConf [catalog-comm-004.svg]`
- `ProductionProposeEffectHandler: connections_cell -> ConnectionsCell [catalog-comm-004.svg]`
- `ProductionProposeEffectHandler: event_publisher -> F1r3flyEvents [catalog-shared-005.svg]`
- `ProductionStakeChecker: validator -> ValidatorIdentity [catalog-casper-058.svg]`
