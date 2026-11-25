# Memory Cardinality and Size

Every entity in the [type inventory](inventory.md) has an assessment beside its rendered diagram.
Each catalog page gives entity cardinality, size rules, and a field-by-field table.
[sizing.json](sizing.json) contains the same assessments for automated analysis.
There are 1,868 declarations in this revision. This is not a count of live objects.

The [ranked size overview](../size-overview.md) uses measured storage cardinalities and encoded record sizes.
It does not assign fabricated totals to unmeasured heap entities.

## Interpretation

Cardinality has two meanings here:

- **Relationship cardinality:** the number of values in a field of one owner. An optional value has zero or one payload. A vector has zero or more elements.
- **Instance cardinality:** the number of live objects in a process. Most declarations do not establish this count. A metric, allocator capture, or owner inspection is required.

The assessment separates these size layers:

1. **Inline layout:** `I(T) = size_of::<T>()`, including compiler padding. This excludes pointed-to allocations.
2. **Owned allocations:** container capacity, nested values, index overhead, allocator overhead, and shared allocations counted once.
3. **Encoded data:** bincode, protobuf, or a custom format. This is not the Rust object layout.
4. **Physical storage:** LMDB pages and allocated filesystem blocks. See [storage sizing](../storage/sizing.md).

All sizes use bytes. KiB, MiB, and GiB use powers of 1024.
The generator records its target architecture and pointer width in the JSON output.
The current generator target is x86_64 with 8-byte pointers.
Its `String` and `Vec` headers are 24 bytes. These are headers, not total object costs.
These measurements do not establish the layout of a different node build or architecture.

For a vector, allocated element storage is `capacity * I(element)`.
Add allocations owned by live elements and allocator overhead.
For a string, add its byte capacity, not its character count.
Maps require keys, values, spare capacity, buckets or tree nodes, and control metadata.
Neither `Vec` nor a map type establishes an application-level upper bound.

`Arc` and `Rc` handles can refer to the same allocation.
Count that allocation once, including counters and alignment.
Do not multiply the target size by the number of references.
An alias does not create a second allocation.
Enum alternatives and protobuf `oneof` members are exclusive.
Do not add the payload sizes of all alternatives.

## Known Owner Limits

These are source defaults, not measurements or universal limits on every instance of a type.
Configuration and constructor arguments can change them.

| Owner or entity | Cardinality or retention rule | Size rule | Source |
| --- | --- | --- | --- |
| `PendingBlocks` | At most 8,192 blocks and reservations, at most 2,048 per sender by default | 256 MiB encoded-byte budget. Map, decoded block, and allocator costs are additional | [Pending blocks](../../../block-storage/src/rust/pending_blocks.rs) |
| Deploy admission pool | At most 512 records by default | 16 MiB encoded-byte budget. This is not a process heap limit | [Deploy storage](../../../block-storage/src/rust/deploy/key_value_deploy_storage.rs) |
| Resident DAG | Height window relative to finality, plus newer blocks. Not a fixed count of blocks | One height can contain many branches. Count blocks, edges, latest messages, and metadata separately | [DAG representation](../../../block-storage/src/rust/dag/block_dag_key_value_storage.rs) |
| Block index cache | At most 512 entries per configured production cache | Entry-count limit. Shared cached values can have variable size | [Runtime manager](../../../casper/src/rust/util/rholang/runtime_manager.rs), [scope limit](../../../casper/src/rust/util/rholang/interpreter_util.rs) |
| Parent post-state cache | At most 64 entries | Entry-count limit, not a byte limit | [Runtime manager](../../../casper/src/rust/util/rholang/runtime_manager.rs) |
| Active validator cache | At most 256 entries | Each value contains a variable validator collection | [Runtime manager](../../../casper/src/rust/util/rholang/runtime_manager.rs) |
| Bonds and quarantine caches | At most 64 entries per cache | Values vary with validator count | [Runtime manager](../../../casper/src/rust/util/rholang/runtime_manager.rs) |
| Production replay cache | At most 192 entries and 32 MiB of charged retained bytes | Admission also limits event-log length to 1,536 entries. Charging is an estimate, not allocator accounting | [Replay cache](../../../casper/src/rust/util/rholang/replay_cache.rs), [construction](../../../casper/src/rust/util/rholang/runtime_manager.rs) |
| `BoundedCache<K,V>` | At most its constructor capacity | LRU entry limit. `Arc<V>` allocations are shared | [Bounded cache](../../../casper/src/rust/util/bounded_cache.rs) |
| Queue records | One record per retained signature. Terminal retention and admission policy apply | Records are resident. Deploy payloads remain on disk until read for sending | [Queue store](../../../deploy-queue/src/store.rs) |
| Queue payloads | One payload per held signature, removed after a terminal decision | 8 GiB payload admission budget. The 16 GiB LMDB map is address space, not resident RAM | [Queue store](../../../deploy-queue/src/store.rs) |
| Hot-store datums, continuations, joins | Variable by active channels, contracts, and reduction workload | Recursive values, indexes, and checkpoints. No fixed total follows from the schema | [Hot store](../../../rspace++/src/rspace/hot_store.rs) |

## Evidence and Unknowns

[Runtime observations](../storage/observed.md) record current resident-DAG metrics and exact database row counts.
These measurements describe the named local shard at the recorded time, not all deployments.
Metrics and different LMDB environments are sampled at different times.

No heap profiler was attached, and no node code was changed.
The node does not expose a live instance counter for every declaration.
The catalog therefore does not claim exact live counts or total heap bytes for all 1,868 entries.

Unknown layouts are explicit. In particular, generic parameters, trait objects, custom wrappers, custom allocators, and third-party internals require their concrete implementations.
The syntax-based rules recognize standard container names. A custom type with the same name needs manual review.
The generator does not resolve Rust imports, aliases recursively, compiler layout attributes, or arbitrary macros.
It does not use bincode rules to estimate custom `Serialize` implementations.

For a capacity estimate, combine a measured owner count with its concrete inline layout and retained allocations.
Include temporary replay and validation objects, allocator fragmentation, stacks, and file-backed resident pages.
Container memory usage cannot be divided reliably among entity types without further instrumentation.

## Protobuf Formula

`v(x)` is the unsigned varint width, from 1 to 10 bytes for a u64.
A present field includes its tag. A length-delimited payload adds `v(length)`.
Signed `int32` values can use 10 bytes when negative. Zigzag `sint32` uses at most 5 bytes.
Repeated numeric fields can be packed. The catalog gives both packed and unpacked formulas instead of assuming a field option.
Duplicate wire occurrences and unknown fields are outside the generated message's normalized encoding model.

Use the concrete message's `encoded_len()` for an exact protobuf value size.
Use the actual storage codec for disk records. See [storage sizing](../storage/sizing.md).
