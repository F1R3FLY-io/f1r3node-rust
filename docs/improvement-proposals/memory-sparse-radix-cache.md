# Sparse In-Memory Radix Cache Nodes

Status: draft proposal. Cache occupancy and net heap savings are not yet measured.
Date: 2026-11-25.

## Objective

Reduce memory retained by decoded radix nodes when most of their 256 slots are empty.
Keep the existing radix encoding, node hashes, history roots, and storage operations unchanged.
Begin with the read cache rather than replacing the public mutable node type.

The [memory evaluation contract](README.md#memory-evaluation-contract) applies.
This is an in-memory representation change, not storage compression or historical-state pruning.

## Current Representation

The [radix implementation](../../rspace++/src/rspace/history/radix_tree.rs) defines `Node = Vec<Item>`.
`empty_node()` allocates 256 `EmptyItem` values.
Each slot has the complete enum layout, including space for alternatives containing two byte vectors.
An empty slot has no nested vector allocation, but still occupies its inline enum space.

The custom disk encoder already skips empty slots.
Compact disk records therefore expand into dense vectors when decoded.
The read cache stores `Arc<Node>` values and requests trimming above 4,096 entries toward 2,048 entries.
These are entry thresholds, not a strict concurrent heap limit.
Callers can keep shared nodes alive after cache eviction.

`load_node_arc()` shares a cached node.
`load_node()` instead clones its dense vector and occupied payloads.
Mutation paths also clone nodes before replacing slots.
The first phase targets retained cache memory, not every mutation allocation.

## Proposed Boundary

Introduce a private immutable `CachedNode` representation inside the radix implementation.
Keep the existing public `Node`, `Item`, `encode`, `try_decode`, and `hash_node` contracts initially.
Do not derive a new storage codec from the cache representation.

The prototype has two alternatives:

| Alternative | Contents | Intended use |
| --- | --- | --- |
| Sparse | A 256-bit presence bitmap and packed occupied `Item` values | Nodes with enough empty slots to offset bitmap overhead |
| Dense | An immutable 256-slot array | Nodes where packed storage costs more or performs worse |

Use an exactly sized boxed slice for packed entries to avoid spare vector capacity.
Keep occupied entries in ascending slot order.
The bitmap uses four `u64` words and requires 32 bytes before surrounding metadata.
An empty node can use a shared immutable empty representation if measurements justify it.

For slot `i`, its presence bit determines whether an entry exists.
The number of set bits before `i` gives the packed entry index.
Missing slots behave exactly like `EmptyItem`.
The lookup returns borrowed data and does not allocate.

Select sparse or dense storage from measured total layout and lookup cost.
Do not select a universal occupancy threshold from disk statistics.
The first prototype can retain the existing occupied `Item` payloads to isolate the empty-slot saving.
Inline hashes and compact prefixes are separate changes.

## Read and Mutation Paths

Decode with the existing checked codec before converting the result into `CachedNode`.
Move occupied items into the packed representation where possible.
Do not clone them solely to populate the cache.

Add an internal borrowed lookup interface for read-only traversal.
Route read-only consumers through that interface so cache hits do not recreate a dense vector.
Keep a compatibility conversion for callers that require the old `Node` or `Arc<Node>` type.
Count those conversions in benchmarks because they can reduce or eliminate the benefit.

For mutation, materialize the legacy mutable node only when needed.
Apply the existing update algorithm and compute bytes with the existing encoder.
Freeze the resulting node into its chosen cache representation after canonical encoding.
Do not retain both dense and sparse copies in separate caches.

`save_node()` compares an existing cached value with the proposed node to detect collisions.
Preserve that comparison through logical slot equality, not representation equality.
Sparse and dense forms of the same node must compare equal.

The implementation also has diagnostic and test paths using serde or bincode with `Node`.
Keep those paths on the legacy type during this phase.
Do not accidentally replace the custom radix format with derived serialization.

## Memory Model

Let `I` be `size_of::<Item>()`, `k` the occupied-slot count, and `P` the occupied items' owned payload bytes.
Ignore allocator rounding only for this initial comparison:

```text
legacy node = vector header + 256 * I + P
sparse node = representation header + 32-byte bitmap + k * I + P
```

Add the `Arc` allocation, cache key, map overhead, and allocator size classes to both measured totals.
The gross empty-slot saving is `(256 - k) * I` before the new metadata cost.
Dense nodes can lose memory efficiency under a sparse-only design.
The adaptive representation prevents that choice when measured layout shows a loss.

For one cache population, sum costs over unique node allocations.
Do not multiply node bytes by every `Arc` reference or cloned cache owner.
Measure nodes retained by callers after eviction separately.
Cache capacity does not reveal the actual occupancy distribution.

## Compatibility and Failure Behavior

- Preserve exact canonical output bytes for each logical node.
- Preserve child order, prefix bytes, empty roots, and leaf versus pointer tags.
- Preserve insertion, replacement, deletion, and path-compression behavior.
- Preserve typed missing-node errors and the existing explicit missing-root tolerance.
- Preserve [pruning protection](../../rspace++/src/rspace/history/prune_guard.rs) and [checkpoint publication](../../rspace++/src/rspace/history/history_repository_impl.rs).
- Preserve cache bounds and clear operations without introducing new disk deletion or root-retention rules.

Malformed input must follow the existing decoder contract.
The current decoder can accept entries in a different order and normalize them into indexed slots.
Compare the new path with that existing behavior, not with an assumed byte-identical re-encoding of every accepted input.
Canonical stored fixtures must still reproduce their original bytes exactly.

## Validation

| Test group | Required evidence |
| --- | --- |
| Occupancy | Empty, singleton, sparse, near-threshold, and fully occupied nodes retain identical logical slots |
| Bitmap boundaries | Slots 0, 63, 64, 127, 128, 191, 192, and 255 map to the correct packed entries |
| Entry kinds | Leaf and pointer entries preserve empty and maximum-length prefixes and complete payloads |
| Codec | Old and new paths produce identical canonical bytes and hashes for the same logical node |
| Decoder behavior | Duplicate slots, truncated input, unordered entries, and malformed lengths retain reviewed outcomes |
| Updates | Generated insert, replace, and delete sequences produce the same bytes and roots after every step |
| Representation changes | Sparse-to-dense and dense-to-sparse conversion preserve logical equality |
| Cache lifetime | Concurrent reads, cache clearing, and eviction cannot invalidate retained shared nodes |
| Missing state | Pruned descendants remain errors rather than empty subtrees |
| End to end | Genesis, checkpoint reload, state transfer, replay, merge recovery, and finalization retain baseline behavior |

Use the legacy algorithm as a differential oracle with identical ordered inputs.
Run counter and bridge-style workloads on disposable shards.
Require exactly-once state effects and consistent finalized state, not identical live proposal timing.
Apply existing [correctness evidence requirements](../cbc-verification-tiers.md) to the eventual implementation.

## Benchmark and Delivery

Measure occupied-slot histograms by trie depth and unique cached node allocation.
Record cache owner count, hit rate, retained bytes, conversion counts, and post-eviction references.
Compare cold loads, repeated reads, mutation-heavy checkpoints, and branch replay separately.
Include dense synthetic nodes so the benchmark does not favor the sparse design by construction.

1. Add occupancy instrumentation and a bounded offline benchmark.
2. Implement the private representation with conversion tests.
3. Compare canonical output against the legacy implementation.
4. Move read-only cache consumers to borrowed lookup.
5. Measure retained heap, peak conversion memory, CPU time, and finalization latency.
6. Validate disposable-shard compatibility before selecting the representation by default.

Keep the old mutable algorithm until a separate benchmark justifies changing it.
Reject the proposal if conversion churn or cache-hit latency outweighs the retained-memory saving.
Rollback is a binary change with no database migration.
