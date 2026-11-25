# catalog node 017

[Inventory](inventory.md) | [PlantUML](catalog-node-017.puml)

![catalog-node-017](catalog-node-017.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### OpenAIConf

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `enabled` | 1 scalar | 1 B | None |
| `api_key` | 0..n UTF-8 bytes | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `validate_api_key` | 1 scalar | 1 B | None |
| `validation_timeout_sec` | 1 scalar | 8 B | None |

### Profile

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `name` | 1 borrowed reference | Pointer or fat pointer. Referent layout required | No owned allocation. Referent: 1 value |
| `data_dir` | 2 component values | Sum of component inline sizes plus compiler padding | Sum of component-owned allocations |

### Command

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Run.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `Keygen.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `LastFinalizedBlock` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `IsFinalized.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `Repl` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Eval.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `Deploy.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `ShowBlock.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `ShowBlocks.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `Vdag.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `Mvdag` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `ListenDataAtName.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `ListenContAtName.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `FindDeploy.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `Propose.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `BondStatus.0` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `Status` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

### BytesIn

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Num.0` | 1 scalar when this variant is active | 4 B | None |
| `Str.0` | 0..n UTF-8 bytes when this variant is active | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |

### GLOBAL_REPORTER

Cardinality: One slot per linked process instance. Lazy payload can be uninitialized.

Size: Declared slot or lazy wrapper layout plus initialized owned allocations.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 1 wrapper slot | Wrapper implementation and payload layout required | Payload allocations and any lock/waiter allocations. Once and lazy payloads can be uninitialized |

### NewPrometheusReporter

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `prometheus_handle` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `upkeep` | 1 wrapper slot | Wrapper implementation and payload layout required | Payload allocations and any lock/waiter allocations. Once and lazy payloads can be uninitialized |

### PrometheusUpkeep

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `reporter` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |

### UpkeepWorker

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `users` | 1 scalar | 8 B | None |
| `stop` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `thread` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |

## Cross-Page References

- `Command: BondStatus.0 -> BondStatusOptions [catalog-node-014.svg]`
- `Command: Deploy.0 -> DeployOptions [catalog-node-013.svg]`
- `Command: Eval.0 -> EvalOptions [catalog-node-013.svg]`
- `Command: FindDeploy.0 -> FindDeployOptions [catalog-node-014.svg]`
- `Command: IsFinalized.0 -> IsFinalizedOptions [catalog-node-014.svg]`
- `Command: Keygen.0 -> KeygenOptions [catalog-node-013.svg]`
- `Command: ListenContAtName.0 -> ContAtNameOptions [catalog-node-014.svg]`
- `Command: ListenDataAtName.0 -> DataAtNameOptions [catalog-node-014.svg]`
- `Command: Propose.0 -> ProposeOptions [catalog-node-014.svg]`
- `Command: Run.0 -> RunOptions [catalog-node-013.svg]`
- `Command: ShowBlock.0 -> ShowBlockOptions [catalog-node-013.svg]`
- `Command: ShowBlocks.0 -> ShowBlocksOptions [catalog-node-014.svg]`
- `Command: Vdag.0 -> VisualizeDagOptions [catalog-node-014.svg]`
