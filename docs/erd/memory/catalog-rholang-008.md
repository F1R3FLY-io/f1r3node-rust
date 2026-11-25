# catalog rholang 008

[Inventory](inventory.md) | [PlantUML](catalog-rholang-008.puml)

![catalog-rholang-008](catalog-rholang-008.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### Inner

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `table` | 1 wrapper slot | Wrapper implementation and payload layout required | Payload allocations and any lock/waiter allocations. Once and lazy payloads can be uninitialized |
| `next_fd` | 1 atomic value | Target atomic layout required | None for the atomic value itself |

### FserrCode

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `0` | 1 borrowed reference | Pointer or fat pointer. Referent layout required | No owned allocation. Referent: 1 value |

### FileHandle

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `file` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 shared strong reference. One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `canon_path` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `mode` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `cmode` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `position` | 1 scalar | 8 B | None |
| `deploy` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### FileHandleTable

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `inner` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `wal` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `root_registry` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `lock_registry` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `dir_handles` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `deploy` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `payload_store` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `payload_source_recorder` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |

### DeployState

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `scope` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `sig` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: None |

### Inner

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `table` | 1 wrapper slot | Wrapper implementation and payload layout required | Payload allocations and any lock/waiter allocations. Once and lazy payloads can be uninitialized |
| `next_fd` | 1 atomic value | Target atomic layout required | None for the atomic value itself |

### HandlerFamily

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Mutation` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Observation` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Stream` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Lock` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Lifecycle` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

### FsHandlerEntry

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `name` | 1 borrowed reference | Pointer or fat pointer. Referent layout required | No owned allocation. Referent: 1 value |
| `arity` | 1 scalar | 8 B | None |
| `verifying` | 1 scalar | 1 B | None |
| `dispatch` | 1 function pointer | 8 B. Function pointer on the generator target | None for the pointer itself |
| `urn_suffix` | 1 borrowed reference | Pointer or fat pointer. Referent layout required | No owned allocation. Referent: 1 value |
| `fixed_channel` | 1 function pointer | 8 B. Function pointer on the generator target | None for the pointer itself |
| `body_ref` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `family` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

## Cross-Page References

- `DeployState: scope -> DeployScope [catalog-rholang-020.svg]`
- `FileHandle: cmode -> ConsensusMode [catalog-rholang-022.svg]`
- `FileHandle: deploy -> DeployScope [catalog-rholang-020.svg]`
- `FileHandle: mode -> AccessMode [catalog-rholang-022.svg]`
- `FileHandleTable: dir_handles -> DirHandleTable [catalog-rholang-007.svg]`
- `FileHandleTable: lock_registry -> SharedLockRegistry [catalog-rholang-022.svg]`
- `FileHandleTable: root_registry -> RootIdentityRegistry [catalog-rholang-023.svg]`
- `FileHandleTable: wal -> Wal [catalog-rholang-025.svg]`
- `FsHandlerEntry: body_ref -> BodyRef [catalog-rholang-043.svg]`
- `FsHandlerEntry: dispatch -> FsProcesses [catalog-rholang-009.svg]`
- `FsHandlerEntry: dispatch -> InterpreterError [catalog-rholang-006.svg]`
- `Inner: table -> DirHandle [catalog-rholang-007.svg]`
