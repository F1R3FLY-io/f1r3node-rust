# catalog models 031

[Inventory](inventory.md) | [PlantUML](catalog-models-031.puml)

![catalog-models-031](catalog-models-031.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### ApprovedBlockRequest

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `identifier` | 0..n UTF-8 bytes | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `trim_state` | 1 scalar | 1 B | None |

### BlockHashMessage

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `block_hash` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `block_creator` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### BlockMessage

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `block_hash` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `header` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `body` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `justifications` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `sender` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `seq_num` | 1 scalar | 4 B | None |
| `sig` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `sig_algorithm` | 0..n UTF-8 bytes | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `shard_id` | 0..n UTF-8 bytes | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `extra_bytes` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### Header

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `parents_hash_list` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `timestamp` | 1 scalar | 8 B | None |
| `version` | 1 scalar | 8 B | None |
| `extra_bytes` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `chain_id` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### BlockCommitment

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `header` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `pre_state_hash` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `post_state_hash` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `bonds_root` | 32 elements | 32 B. 32 * I(element), inline array | Sum of element-owned allocations. Element: None |
| `justifications_root` | 32 elements | 32 B. 32 * I(element), inline array | Sum of element-owned allocations. Element: None |
| `block_number` | 1 scalar | 8 B | None |
| `rejected_root` | 32 elements | 32 B. 32 * I(element), inline array | Sum of element-owned allocations. Element: None |
| `applied_from_scope_root` | 32 elements | 32 B. 32 * I(element), inline array | Sum of element-owned allocations. Element: None |
| `merge_base` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `body_extra_bytes` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `sender` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `sig_algorithm` | 0..n UTF-8 bytes | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `seq_num` | 1 scalar | 4 B | None |
| `shard_id` | 0..n UTF-8 bytes | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `extra_bytes` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `deploys_root` | 32 elements | 32 B. 32 * I(element), inline array | Sum of element-owned allocations. Element: None |
| `sig` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### RejectedDeploy

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `sig` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `duplicate` | 1 scalar | 1 B | None |
| `carrier` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### Body

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `state` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `deploys` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `rejected_deploys` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `system_deploys` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `extra_bytes` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `applied_from_scope` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `merge_base` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### Justification

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `validator` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `latest_block_hash` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

## Cross-Page References

- `BlockCommitment: body_extra_bytes -> ByteString [catalog-models-027.svg]`
- `BlockCommitment: extra_bytes -> ByteString [catalog-models-027.svg]`
- `BlockCommitment: merge_base -> ByteString [catalog-models-027.svg]`
- `BlockCommitment: post_state_hash -> ByteString [catalog-models-027.svg]`
- `BlockCommitment: pre_state_hash -> ByteString [catalog-models-027.svg]`
- `BlockCommitment: sender -> ByteString [catalog-models-027.svg]`
- `BlockCommitment: sig -> ByteString [catalog-models-027.svg]`
- `BlockHashMessage: block_creator -> ByteString [catalog-models-027.svg]`
- `BlockHashMessage: block_hash -> ByteString [catalog-models-027.svg]`
- `BlockMessage: block_hash -> ByteString [catalog-models-027.svg]`
- `BlockMessage: extra_bytes -> ByteString [catalog-models-027.svg]`
- `BlockMessage: sender -> ByteString [catalog-models-027.svg]`
- `BlockMessage: sig -> ByteString [catalog-models-027.svg]`
- `Body: applied_from_scope -> ByteString [catalog-models-027.svg]`
- `Body: deploys -> ProcessedDeploy [catalog-models-032.svg]`
- `Body: extra_bytes -> ByteString [catalog-models-027.svg]`
- `Body: merge_base -> ByteString [catalog-models-027.svg]`
- `Body: state -> F1r3flyState [catalog-models-032.svg]`
- `Body: system_deploys -> ProcessedSystemDeploy [catalog-models-032.svg]`
- `Header: chain_id -> ByteString [catalog-models-027.svg]`
- `Header: extra_bytes -> ByteString [catalog-models-027.svg]`
- `Header: parents_hash_list -> ByteString [catalog-models-027.svg]`
- `Justification: latest_block_hash -> ByteString [catalog-models-027.svg]`
- `Justification: validator -> ByteString [catalog-models-027.svg]`
- `RejectedDeploy: carrier -> ByteString [catalog-models-027.svg]`
- `RejectedDeploy: sig -> ByteString [catalog-models-027.svg]`
