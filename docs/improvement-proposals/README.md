# Storage Improvement Proposals

Status: draft designs for review. These proposals do not authorize implementation, migration, deployment, or a protocol change.
Date: 2026-11-25.

## Proposals

| Proposal | Purpose | Main risk | Suggested order |
| --- | --- | --- | --- |
| [Storage-only compression](storage-only-compression.md) | Compress existing cold-leaf bytes without changing their canonical representation | Decode limits, CPU cost, and incomplete storage-path coverage | Benchmark first |
| [Storage-subtree deduplication](storage-subtree-deduplication.md) | Share identical encoded subtrees between different cold leaves | Shared-object lifetime, atomic publication, and reconstruction cost | Measure reuse, then compare against compression |

Compression and deduplication address different costs.
Compression reduces repetition within stored bytes.
Deduplication stores one copy of an identical subtree for several references.
Neither proposal changes which chain states the node retains.

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

In these proposals, canonical bytes mean the exact bytes exposed by the existing logical cold store.
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
