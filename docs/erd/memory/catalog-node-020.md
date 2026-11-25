# catalog node 020

[Inventory](inventory.md) | [PlantUML](catalog-node-020.puml)

![catalog-node-020](catalog-node-020.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### ProposeQueueEntry

Cardinality: Alias, not a separate allocation. Count values of the target type.

Size: Same layout and allocations as target. Do not add alias bytes again.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 4 component values | Sum of component inline sizes plus compiler padding | Sum of component-owned allocations |

### ProposerInstance

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `propose_requests_queue_rx` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `propose_requests_queue_tx` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `proposer` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `state` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `propose_queue_pending` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `propose_queue_max_pending` | 1 scalar | 8 B | None |

### ReplRuntime

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| No declared fields | 0 field payloads | Compiler layout required | No field-owned allocations |

### ReadParams

Cardinality: Alias, not a separate allocation. Count values of the target type.

Size: Same layout and allocations as target. Do not add alias bytes again.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 4 component values | Sum of component inline sizes plus compiler padding | Sum of component-owned allocations |

### RhoTrieTraverser

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| No declared fields | 0 field payloads | Compiler layout required | No field-owned allocations |

### APIServers

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `repl` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `propose` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `deploy` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `lsp` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### ProposerQueueEntry

Cardinality: Alias, not a separate allocation. Count values of the target type.

Size: Same layout and allocations as target. Do not add alias bytes again.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 4 component values | Sum of component inline sizes plus compiler padding | Sum of component-owned allocations |

### CasperLoop

Cardinality: Alias, not a separate allocation. Count values of the target type.

Size: Same layout and allocations as target. Do not add alias bytes again.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |

## Cross-Page References

- `APIServers: deploy -> DeployGrpcServiceV1Impl [catalog-node-002.svg]`
- `APIServers: lsp -> LspGrpcServiceImpl [catalog-node-003.svg]`
- `APIServers: propose -> ProposeGrpcServiceV1Impl [catalog-node-003.svg]`
- `APIServers: repl -> ReplGrpcServiceImpl [catalog-node-003.svg]`
- `CasperLoop: value -> CasperError [catalog-casper-030.svg]`
- `ProposeQueueEntry: value -> ProposerResult [catalog-casper-018.svg]`
- `ProposerInstance: proposer -> ProductionProposer [catalog-casper-018.svg]`
- `ProposerInstance: state -> ProposerState [catalog-casper-049.svg]`
- `ProposerQueueEntry: value -> ProposerResult [catalog-casper-018.svg]`
- `ReadParams: value -> ParMap [catalog-models-036.svg]`
