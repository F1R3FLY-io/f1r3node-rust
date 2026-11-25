# Memory and Storage Improvement Proposals

Status: draft designs for review. These proposals do not authorize implementation, migration, deployment, or a protocol change.
Date: 2026-11-25.

## Proposals

### Storage

| Proposal | Purpose | Main risk | Suggested order |
| --- | --- | --- | --- |
| [Storage-only compression](storage-only-compression.md) | Compress existing cold-leaf bytes without changing their canonical representation | Decode limits, CPU cost, and incomplete storage-path coverage | Benchmark first |
| [Storage-subtree deduplication](storage-subtree-deduplication.md) | Share identical encoded subtrees between different cold leaves | Shared-object lifetime, atomic publication, and reconstruction cost | Measure reuse, then compare against compression |

Compression and deduplication address different costs.
Compression reduces repetition within stored bytes.
Deduplication stores one copy of an identical subtree for several references.
Neither proposal changes which chain states the node retains.

### Memory

| Proposal | Memory target | Main risk | Suggested order |
| --- | --- | --- | --- |
| [Typed JSON boundaries](memory-typed-json-boundaries.md) | Temporary object trees in reports, events, and queue parsing | API compatibility and parser behavior | First, as independent boundary changes |
| [Shared events and compact replay](memory-event-replay.md) | Payload clones and retained per-client startup JSON strings | Publication order, replay overlap, and lag recovery | After deterministic handoff tests |
| [Sparse radix cache](memory-sparse-radix-cache.md) | Empty slots in decoded cached nodes | Canonical bytes, lookup cost, and conversion churn | After occupancy measurements |
| [Inline binary hashes](memory-inline-hashes.md) | Per-hash allocations in selected runtime caches | Key compatibility and accidental serialization changes | Start with private cache keys |

This order reflects implementation risk, not a measured ranking of whole-node savings.
JSON trees mainly affect boundary allocations.
Radix nodes and hash keys can affect retained heap without any client traffic.
Each proposal separates existing source facts from proposed changes and unmeasured benefits.

These designs do not include a compact runtime `Par` representation.
The [Par schema](../../models/src/main/protobuf/RhoTypes.proto) has many collection headers, but its live allocation count remains unknown.
The [hot store](../../rspace++/src/rspace/hot_store.rs) already shares datums and continuations through `Arc` and uses persistent checkpoint maps.
Further runtime sharing needs measurements that account for those existing optimizations.
Storage-subtree deduplication does not itself reduce decoded runtime objects.

## Measurement Baseline

The [current report](../erd/storage/leaf-measurements.md) describes the complete cold-store scan.
The capture ran from 2026-11-25T10:35:59Z to 10:37:45Z.
It scanned five nodes and compared their state at finalized block 1095.
MiB means 1,048,576 bytes.

The fixed evidence is the [saved report](https://github.com/machieke/f1r3node-rust/blob/31062bc508789356c08ce46f3dffdaf987ecfa43/docs/erd/storage/leaf-measurements.md)
(local Git: `31062bc508789356c08ce46f3dffdaf987ecfa43:docs/erd/storage/leaf-measurements.md`).
The [saved histograms](https://github.com/machieke/f1r3node-rust/blob/31062bc508789356c08ce46f3dffdaf987ecfa43/docs/erd/storage/leaves-observed.json)
(local Git: `31062bc508789356c08ce46f3dffdaf987ecfa43:docs/erd/storage/leaves-observed.json`) contain the underlying counts.

| Observation | Value | Interpretation |
| --- | --- | --- |
| All cold-leaf rows | 89,136 | Physical copies across five replicas and retained versions |
| Continuation rows | 41,718 | 225,964,113 encoded key/value bytes, or 215.496 MiB |
| Continuation elements | 44,697 | Waiting-continuation records inside those leaves |
| Nested Par occurrences in continuations | 1,739,994 | Occurrences, not distinct subtree contents |
| Continuation share of cold key/value bytes | 91.94% | Does not include database page overhead |
| Continuation leaves at the selected finalized root | 2,848 per node | Same decoded statistics on all five nodes |

Each current `Par` has ten vector-length prefixes and one boolean in its bincode representation.
These fields require 81 bytes before vector contents.
Multiplication by the measured Par count gives 140,939,514 bytes, or 134.410 MiB.
This structural cost is 62.37% of continuation key/value bytes.
It is not a measured compression saving or a count of removable bytes.

The outer wrappers and physical keys account for only 3,520,680 continuation bytes, or 1.56%.
The calculation subtracts encoded continuation-element bytes from encoded continuation-row bytes.
See the [Par schema](../../models/src/main/protobuf/RhoTypes.proto), [serializer](../../rspace++/src/rspace/serializers/serializers.rs), and [size rules](../erd/storage/sizing.md).

The scan did not measure compression ratios, distinct subtree hashes, or reuse between different leaves.
The saved JSON contains aggregate counts, not the canonical payloads required for those benchmarks.
A new bounded capture or a controlled fixture corpus is necessary.
Replica copies must not count as deduplication opportunities within one node.

## Shared Contract

In the storage proposals, canonical bytes mean the exact bytes exposed by the existing logical cold store.
The physical storage representation can change only below that interface.

Let `U` be the existing bincode encoding of `PersistedData`.
Let `L` be the existing bincode encoding of its selected leaf structure.
The cold-leaf content hash is `Blake2b256(L)`, not `Blake2b256(U)`.
The `PersistedData` enum tag is outside that existing hash input.
See [leaf hashing and checkpoint writes](../../rspace++/src/rspace/history/history_repository_impl.rs) and [state-import verification](../../rspace++/src/rspace/state/rspace_importer.rs).

Both designs require these invariants:

- A logical read returns exactly `U`, byte for byte.
- Leaf hashes, trie roots, channel hashes, and trace hashes remain unchanged.
- Continuation order, multiplicity, patterns, guards, random state, and trace data remain unchanged.
- State transfer exposes the existing canonical bytes, not physical compression frames or subtree references.
- Raw-hash and bincode-hash keys retain their current lookup precedence.
- A corrupt stored record returns an error, not an absent or empty continuation.
- Storage changes do not change deploy execution, charging, merge recovery, or finalization rules.

These are proposed acceptance conditions, not completed correctness claims.
An implementation needs byte-level, restart, state-transfer, and consensus regression evidence before release.

## Evaluation Order

1. Benchmark lossless compression on a fixed canonical corpus.
2. Measure subtree reuse within each node and retained-root population.
3. Compare deduplication against compression, including all metadata and read costs.
4. Prototype each physical format behind a disabled-by-default local storage option.
5. Run mixed-format recovery and compatibility tests on disposable shards.

The corpus must include user traffic, large continuations, guards, joins, and retained branch history.
The current snapshot alone does not establish production savings.
Numerical performance thresholds remain open until the baseline benchmark exists.

See the [storage diagrams](../erd/storage/README.md) for the current structure.

## Memory Source Baseline

The memory proposals use the following source snapshot, not a heap capture.
Document-relative source links in each proposal provide navigation to the current checkout.
These pinned references preserve the inspected implementations:

- [Trace response conversion](https://github.com/machieke/f1r3node-rust/blob/d6e53417848947e6c60c348c883734a4cbea6651/node/src/rust/web/reporting_routes.rs)
  (local Git: `d6e53417848947e6c60c348c883734a4cbea6651:node/src/rust/web/reporting_routes.rs`).
- [WebSocket conversion and replay](https://github.com/machieke/f1r3node-rust/blob/d6e53417848947e6c60c348c883734a4cbea6651/node/src/rust/web/events_info.rs)
  (local Git: `d6e53417848947e6c60c348c883734a4cbea6651:node/src/rust/web/events_info.rs`).
- [Event publication and ownership](https://github.com/machieke/f1r3node-rust/blob/d6e53417848947e6c60c348c883734a4cbea6651/shared/src/rust/shared/f1r3fly_events.rs)
  (local Git: `d6e53417848947e6c60c348c883734a4cbea6651:shared/src/rust/shared/f1r3fly_events.rs`).
- [Queue event parsing](https://github.com/machieke/f1r3node-rust/blob/d6e53417848947e6c60c348c883734a4cbea6651/deploy-queue/src/events.rs)
  (local Git: `d6e53417848947e6c60c348c883734a4cbea6651:deploy-queue/src/events.rs`).
- [Radix node representation and caches](https://github.com/machieke/f1r3node-rust/blob/d6e53417848947e6c60c348c883734a4cbea6651/rspace%2B%2B/src/rspace/history/radix_tree.rs)
  (local Git: `d6e53417848947e6c60c348c883734a4cbea6651:rspace++/src/rspace/history/radix_tree.rs`).
- [RSpace hash wrapper](https://github.com/machieke/f1r3node-rust/blob/d6e53417848947e6c60c348c883734a4cbea6651/rspace%2B%2B/src/rspace/hashing/blake2b256_hash.rs)
  (local Git: `d6e53417848947e6c60c348c883734a4cbea6651:rspace++/src/rspace/hashing/blake2b256_hash.rs`).

The [memory catalog](../erd/memory/README.md) and [size rules](../erd/memory/sizing.md) describe layouts and known owner limits.
The cold-store measurement baseline does not establish live heap counts.
Its retained versions and replica copies must not become multipliers for a single process's memory estimate.

## Memory Evaluation Contract

The following conditions are proposed acceptance criteria, not completed implementation claims.
No proposal changes deploy scheduling, canonical inclusion, finalization, or chain-state retention.
No proposal increases concurrency, cache limits, or timeout values to hide a regression.

### Compatibility

- Keep external JSON schemas, byte encodings, and protocol versions unchanged unless a separately reviewed behavior change is necessary.
- Keep canonical serialized values, physical keys, consensus hashes, and state roots unchanged.
- Preserve order, multiplicity, errors, cancellation, and ownership lifetimes.
- Preserve lag recovery, bounded queues, replay admission, root pins, and pruning coordination.
- Apply the existing [correctness evidence requirements](../cbc-verification-tiers.md) to affected implementation files.

A smaller in-memory representation is not permission to change serde output.
A cache optimization is not permission to discard unresolved state or change missing-state errors into empty values.

### Measurement

Use a Rust or Bash benchmark against a fixed revision and reproducible synthetic fixtures.
Use disposable shards for end-to-end traffic.
Do not profile, restart, or mutate the current shard as an implicit part of this documentation work.
Any future live capture needs separate operational approval.

For each benchmark, record:

- The revision, build profile, compiler, architecture, allocator, and workload.
- Live owner counts, container capacities, and unique shared allocation counts.
- Retained heap before and after the workload, peak live heap, total allocated bytes, and allocation count.
- Process resident memory, allocator-retained pages, and file-backed pages as separate measurements where available.
- CPU time, throughput, and latency at the 50th, 95th, and 99th percentiles.
- Cache hits, conversions, event lag, and client count where applicable.
- Finalized state correctness, deploy completion, finalization progress, and node restart counts for shard tests.

Count each shared allocation once, including its reference counters and alignment.
Account for transient overlap during conversion and for callers retaining values after cache eviction.
Measure node and deploy-queue processes separately.
Use bounded metric labels such as component, representation, and result.
Do not export contract contents, signatures, hashes, or client identifiers as metric labels.

Compare one change at a time before testing combinations.
Do not add independent percentage savings because the same allocation can appear in multiple proposals.
Run repeated warmed and cold trials with the same resource limits.
Set numerical acceptance thresholds from the baseline before evaluating the candidate.

The result should name measured savings, uncertainty, latency costs, and regressions.
Layout estimates alone do not justify a default change.
Successful rollback should require only the previous binary because these memory proposals do not alter stored formats.
