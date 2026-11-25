# catalog deploy queue 002

[Inventory](inventory.md) | [PlantUML](catalog-deploy-queue-002.puml)

![catalog-deploy-queue-002](catalog-deploy-queue-002.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### Effect

Cardinality: Alias, not a separate allocation. Count values of the target type.

Size: Same layout and allocations as target. Do not add alias bytes again.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 1 owned target | Pointer or fat pointer, depending on target type | I(target) and its owned allocations, plus allocator overhead. Zero-sized targets need no allocation |

### IntakeResult

Cardinality: Alias, not a separate allocation. Count values of the target type.

Size: Same layout and allocations as target. Do not add alias bytes again.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 1 of 2 alternatives | Maximum variant layout plus compiler tag and padding | Only the active Ok or Err payload contributes owned allocations |

### CreatedSample

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `number` | 1 scalar | 8 B | None |
| `creator` | 0..n UTF-8 bytes | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `admission` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |
| `took` | 1 scalar | 8 B | None |
| `left` | 1 scalar | 8 B | None |
| `at` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### Engine

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `store` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `node` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `commands` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `records` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `counts` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `ready` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `waiting` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `expiry` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `closed` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `closed_checking` | 0..n distinct elements | Container and hasher implementation required | Element storage, owned allocations, spare capacity and index overhead. No type-level count cap |
| `released_index` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `resend_index` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `decided_index` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `held_by_deployer` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `chain` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |
| `connected` | 1 scalar | 1 B | None |
| `health` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |
| `created` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `learned` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `held_for_finality` | 1 scalar | 1 B | None |
| `next_sequence` | 1 scalar | 8 B | None |
| `retry_at` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |
| `retry_delay` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `transitions` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `dirty` | 0..n distinct elements | Container and hasher implementation required | Element storage, owned allocations, spare capacity and index overhead. No type-level count cap |
| `removed` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `new_payloads` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `dropped_payloads` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `payload_bytes` | 1 scalar | 8 B | None |
| `intake_replies` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Sum of component-owned allocations |
| `effects` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |

### DeadlineIndex

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `deploys` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `per_deadline` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |

### Slot

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `releasable_from` | 1 scalar | 8 B | None |
| `last` | 1 scalar | 8 B | None |

### Congestion

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Validator` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `ValidatorIdle` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Shard` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Storage` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

### IntakeError

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `IllegalArgument.0` | 0..n UTF-8 bytes when this variant is active | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `DeployExpired.0` | 0..n UTF-8 bytes when this variant is active | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `InvalidDeployWindow.0` | 0..n UTF-8 bytes when this variant is active | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `Duplicate.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `QueueFull.retry_after_secs` | 1 scalar when this variant is active | 8 B | None |
| `QueueFull.cause` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `DeployerShareFull.held` | 1 scalar when this variant is active | 8 B | None |
| `DeployerShareFull.limit` | 1 scalar when this variant is active | 8 B | None |
| `DeployerShareFull.retry_after_secs` | 1 scalar when this variant is active | 8 B | None |
| `NodeUnavailable.retry_after_secs` | 1 scalar when this variant is active | 8 B | None |
| `Stopped` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Store.0` | 0..n UTF-8 bytes when this variant is active | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |

## Cross-Page References

- `CreatedSample: admission -> Admission [catalog-deploy-queue-003.svg]`
- `DeadlineIndex: deploys -> Sig [catalog-deploy-queue-006.svg]`
- `Engine: chain -> ChainSnapshot [catalog-deploy-queue-006.svg]`
- `Engine: closed -> Sig [catalog-deploy-queue-006.svg]`
- `Engine: closed_checking -> Sig [catalog-deploy-queue-006.svg]`
- `Engine: commands -> Command [catalog-deploy-queue-001.svg]`
- `Engine: counts -> QueueState [catalog-deploy-queue-007.svg]`
- `Engine: decided_index -> Sig [catalog-deploy-queue-006.svg]`
- `Engine: dirty -> Sig [catalog-deploy-queue-006.svg]`
- `Engine: dropped_payloads -> Sig [catalog-deploy-queue-006.svg]`
- `Engine: health -> Health [catalog-deploy-queue-005.svg]`
- `Engine: new_payloads -> Sig [catalog-deploy-queue-006.svg]`
- `Engine: node -> NodeHttp [catalog-deploy-queue-006.svg]`
- `Engine: ready -> Sig [catalog-deploy-queue-006.svg]`
- `Engine: records -> QueueRecord [catalog-deploy-queue-007.svg]`
- `Engine: records -> Sig [catalog-deploy-queue-006.svg]`
- `Engine: released_index -> Sig [catalog-deploy-queue-006.svg]`
- `Engine: removed -> Sig [catalog-deploy-queue-006.svg]`
- `Engine: resend_index -> Sig [catalog-deploy-queue-006.svg]`
- `Engine: store -> QueueStore [catalog-deploy-queue-007.svg]`
- `Engine: transitions -> Transition [catalog-deploy-queue-001.svg]`
- `Engine: waiting -> Sig [catalog-deploy-queue-006.svg]`
- `IntakeError: Duplicate.0 -> Sig [catalog-deploy-queue-006.svg]`
- `IntakeResult: value -> Sig [catalog-deploy-queue-006.svg]`
