# catalog node 002

[Inventory](inventory.md) | [PlantUML](catalog-node-002.puml)

![catalog-node-002](catalog-node-002.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### EvalRequest

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Generated Rust layout plus owned allocations. Encoded size is the sum of present field encodings.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `program #1` | 0..1 encoded value for these proto3 schemas | Generated Rust field layout required. Wire size is not heap size | Wire when present: tag + v(L) + L bytes, where L is the byte length. Defaults can be omitted. tag = v((field_number << 3) &#124; wire_type) |
| `printUnmatchedSendsOnly #2` | 0..1 encoded value for these proto3 schemas | Generated Rust field layout required. Wire size is not heap size | Wire when present: tag + 1 B. Defaults can be omitted. tag = v((field_number << 3) &#124; wire_type) |
| `language #3` | 0..1 encoded value for these proto3 schemas | Generated Rust field layout required. Wire size is not heap size | Wire when present: tag + v(L) + L bytes, where L is the byte length. Defaults can be omitted. tag = v((field_number << 3) &#124; wire_type) |

### ReplResponse

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Generated Rust layout plus owned allocations. Encoded size is the sum of present field encodings.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `output #1` | 0..1 encoded value for these proto3 schemas | Generated Rust field layout required. Wire size is not heap size | Wire when present: tag + v(L) + L bytes, where L is the byte length. Defaults can be omitted. tag = v((field_number << 3) &#124; wire_type) |

### AdminWebApiImpl

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `trigger_propose_f_opt` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 shared strong reference. One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `proposer_state_ref_opt` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 shared strong reference. One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `engine_cell` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |

### DeployGrpcServiceV1Impl

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `deploy_limiter` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `api_max_blocks_limit` | 1 scalar | 4 B | None |
| `trigger_propose_f` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 shared strong reference. One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `dev_mode` | 1 scalar | 1 B | None |
| `network_id` | 0..n UTF-8 bytes | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `shard_id` | 0..n UTF-8 bytes | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `min_phlo_price` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `native_token_name` | 0..n UTF-8 bytes | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `native_token_symbol` | 0..n UTF-8 bytes | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `native_token_decimals` | 1 scalar | 4 B | None |
| `has_validator_key` | 1 scalar | 1 B | None |
| `is_node_read_only` | 1 scalar | 1 B | None |
| `engine_cell` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `block_report_api` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `transfer_unforgeable` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `key_value_block_store` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `rp_conf_cell` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `connections_cell` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `node_discovery` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |
| `epoch_length` | 1 scalar | 4 B | None |
| `is_ready` | 1 shared strong reference | Pointer or fat pointer, depending on target type | One shared allocation holds the target, counters and alignment. Count it once across all handles |

### DeployIngressService

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `inner` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `limiter` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### RE_TOP_LEVEL_FREE_VARS

Cardinality: One slot per linked process instance. Lazy payload can be uninitialized.

Size: Declared slot or lazy wrapper layout plus initialized owned allocations.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 1 wrapper slot | Wrapper implementation and payload layout required | Payload allocations and any lock/waiter allocations. Once and lazy payloads can be uninitialized |

### RE_TOP_LEVEL_WILDCARDS

Cardinality: One slot per linked process instance. Lazy payload can be uninitialized.

Size: Declared slot or lazy wrapper layout plus initialized owned allocations.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 1 wrapper slot | Wrapper implementation and payload layout required | Payload allocations and any lock/waiter allocations. Once and lazy payloads can be uninitialized |

### RE_TOP_LEVEL_CONNECTIVES

Cardinality: One slot per linked process instance. Lazy payload can be uninitialized.

Size: Declared slot or lazy wrapper layout plus initialized owned allocations.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 1 wrapper slot | Wrapper implementation and payload layout required | Payload allocations and any lock/waiter allocations. Once and lazy payloads can be uninitialized |

## Cross-Page References

- `AdminWebApiImpl: engine_cell -> EngineCell [catalog-casper-021.svg]`
- `AdminWebApiImpl: proposer_state_ref_opt -> ProposerState [catalog-casper-049.svg]`
- `AdminWebApiImpl: trigger_propose_f_opt -> ProposeFunction [catalog-casper-039.svg]`
- `DeployGrpcServiceV1Impl: block_report_api -> BlockReportAPI [catalog-casper-004.svg]`
- `DeployGrpcServiceV1Impl: connections_cell -> ConnectionsCell [catalog-comm-004.svg]`
- `DeployGrpcServiceV1Impl: deploy_limiter -> DeployRequestLimiter [catalog-node-003.svg]`
- `DeployGrpcServiceV1Impl: engine_cell -> EngineCell [catalog-casper-021.svg]`
- `DeployGrpcServiceV1Impl: key_value_block_store -> KeyValueBlockStore [catalog-block-storage-008.svg]`
- `DeployGrpcServiceV1Impl: min_phlo_price -> TokenAmount [catalog-shared-009.svg]`
- `DeployGrpcServiceV1Impl: rp_conf_cell -> RPConfCell [catalog-comm-004.svg]`
- `DeployGrpcServiceV1Impl: trigger_propose_f -> ProposeFunction [catalog-casper-039.svg]`
- `DeployIngressService: limiter -> DeployRequestLimiter [catalog-node-003.svg]`
