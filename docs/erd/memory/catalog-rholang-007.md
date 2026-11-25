# catalog rholang 007

[Inventory](inventory.md) | [PlantUML](catalog-rholang-007.puml)

![catalog-rholang-007](catalog-rholang-007.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### HostWorkBudgetState

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `limits` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `usage` | HOST_WORK_DIMENSION_COUNT elements | HOST_WORK_DIMENSION_COUNT * I(element), inline array | Sum of element-owned allocations. Element: None for the atomic value itself |
| `rejected` | 1 atomic value | Target atomic layout required | None for the atomic value itself |
| `rejection` | 1 wrapper slot | Wrapper implementation and payload layout required | Payload allocations and any lock/waiter allocations. Once and lazy payloads can be uninitialized |

### HostWorkBudget

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `state` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |

### EvaluateResult

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `cost` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `errors` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `mergeable` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |

### InterpreterImpl

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `c` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `merge_chs` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |

### ConsensusFoldEntry

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `order` | 1 scalar | 4 B | None |
| `name` | 1 borrowed reference | Pointer or fat pointer. Referent layout required | No owned allocation. Referent: 1 value |
| `encode` | 1 function pointer | 8 B. Function pointer on the generator target | None for the pointer itself |

### DirIter

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `dirp` | 1 pointer, possibly null | Pointer or fat pointer. Pointee layout required | Ownership and lifetime are not established by a raw pointer |

### DirHandle

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `iter` | 1 wrapper slot | Wrapper implementation and payload layout required | Payload allocations and any lock/waiter allocations. Once and lazy payloads can be uninitialized |
| `canon_path` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `cmode` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `deploy` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### DirHandleTable

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `inner` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |

## Cross-Page References

- `DirHandle: cmode -> ConsensusMode [catalog-rholang-022.svg]`
- `DirHandle: deploy -> DeployScope [catalog-rholang-020.svg]`
- `DirHandleTable: inner -> Inner [catalog-rholang-008.svg]`
- `EvaluateResult: cost -> Cost [catalog-rholang-001.svg]`
- `EvaluateResult: errors -> InterpreterError [catalog-rholang-006.svg]`
- `EvaluateResult: mergeable -> MergeType [catalog-rspacepp-011.svg]`
- `HostWorkBudgetState: limits -> HostWorkLimits [catalog-models-035.svg]`
- `HostWorkBudgetState: rejection -> HostWorkReservationError [catalog-models-036.svg]`
- `InterpreterImpl: c -> _cost [catalog-rholang-001.svg]`
- `InterpreterImpl: merge_chs -> MergeType [catalog-rspacepp-011.svg]`
