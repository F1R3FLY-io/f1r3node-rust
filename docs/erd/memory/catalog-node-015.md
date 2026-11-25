# catalog node 015

[Inventory](inventory.md) | [PlantUML](catalog-node-015.puml)

![catalog-node-015](catalog-node-015.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### StaticFileEntry

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `path` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `mode` | 0..n UTF-8 bytes | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |

### StaticDirEntry

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `path` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `mode` | 0..n UTF-8 bytes | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |

### Wire

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Object.path` | 1 value when this variant is active | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `Object.mode` | 0..n UTF-8 bytes when this variant is active | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `Bare.0` | 0..n UTF-8 bytes when this variant is active | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |

### FileIoProvisioning

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `oracle_static_files` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `oracle_static_dirs` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `consensus_static_files` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |
| `consensus_static_dirs` | 0..n key/value pairs | Container and hasher implementation required | n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap |

### NodeConf

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `standalone` | 1 scalar | 1 B | None |
| `autopropose` | 1 scalar | 1 B | None |
| `protocol_server` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `protocol_client` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `peers_discovery` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `api_server` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `storage` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `tls` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `casper` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `metrics` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `logging` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `dev_mode` | 1 scalar | 1 B | None |
| `dev` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `openai` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `soak_observer` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |

### SoakObserverConfig

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `directory` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `source_revision` | 0..n UTF-8 bytes | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `approved_request_sha256` | 0..n UTF-8 bytes | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `peer_pid` | 1 scalar | 4 B | None |
| `peer_start_ticks` | 1 scalar | 8 B | None |
| `session_timeout_ms` | 1 scalar | 8 B | None |
| `max_sessions` | 1 scalar | 4 B | None |

### ProtocolServer

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `network_id` | 0..n UTF-8 bytes | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `host` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 0..n UTF-8 bytes. capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `allow_private_addresses` | 1 scalar | 1 B | None |
| `use_random_ports` | 1 scalar | 1 B | None |
| `dynamic_ip` | 1 scalar | 1 B | None |
| `no_upnp` | 1 scalar | 1 B | None |
| `port` | 1 scalar | 2 B | None |
| `grpc_max_recv_message_size` | 1 scalar | 4 B | None |
| `grpc_max_recv_stream_message_size` | 1 scalar | 4 B | None |
| `max_message_consumers` | 1 scalar | 4 B | None |
| `disable_state_exporter` | 1 scalar | 1 B | None |

### ProtocolClient

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `network_id` | 0..n UTF-8 bytes | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `bootstrap` | 0..n UTF-8 bytes | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `disable_lfs` | 1 scalar | 1 B | None |
| `batch_max_connections` | 1 scalar | 4 B | None |
| `network_timeout` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `grpc_max_recv_message_size` | 1 scalar | 4 B | None |
| `grpc_stream_chunk_size` | 1 scalar | 4 B | None |

## Cross-Page References

- `NodeConf: api_server -> ApiServer [catalog-node-016.svg]`
- `NodeConf: casper -> CasperConf [catalog-casper-020.svg]`
- `NodeConf: dev -> DevConf [catalog-node-016.svg]`
- `NodeConf: logging -> LoggingConfig [catalog-shared-010.svg]`
- `NodeConf: metrics -> Metrics [catalog-node-016.svg]`
- `NodeConf: openai -> OpenAIConf [catalog-node-017.svg]`
- `NodeConf: peers_discovery -> PeersDiscovery [catalog-node-016.svg]`
- `NodeConf: storage -> Storage [catalog-node-016.svg]`
- `NodeConf: tls -> TlsConf [catalog-node-016.svg]`
