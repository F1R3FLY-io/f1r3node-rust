> Last updated: 2026-04-19

# Crate: block-storage

**Path**: `block-storage/`

Block persistence, DAG state management, casper buffer, and deploy indexing.

## Block Store

**`KeyValueBlockStore`**:
- Two stores: blocks and approved_blocks
- LZ4 compression with varint length prefix (Java-compatible)
- `get(hash)`, `put(hash, block)`, `contains(hash)`
- `get_approved_block()` / `put_approved_block()` -- Singleton approved block

## DAG Storage

**`KeyValueDagRepresentation`** -- Immutable DAG snapshot (O(1) clones via `imbl`):
- `dag_set`, `latest_messages_map`, `child_map`, `height_map`
- `invalid_blocks_set`, `last_finalized_block`, `finalized_blocks_set`
- Queries: `lookup`, `children`, `parents_unsafe`, `latest_messages`, `topo_sort`, `main_parent_chain`, `ancestors`, `descendants`, `non_finalized_blocks`

**`BlockDagKeyValueStorage`** -- Live mutable DAG with a global read/write lock:
- `get_representation()` -- Atomic snapshot (acquires lock)
- `insert(block, mode)` -- Add a normal, invalid, or approved block with metadata updates
- `record_directly_finalized_atomic(hash, ft_value, effect)` -- Compare-and-append an immutable finalization round, project it in order, and run its receipted effect
- `reconcile_finalization_projection()` -- Resume committed metadata projection from the durable cursor
- `pending_finalization_effect_records()` -- Return only the unfinished contiguous effect suffix

**`FinalizationLedger`** -- Crash-consistent local finalization publication:
- Atomically bootstraps an immutable approved-genesis anchor, revision-zero head, and three recovery cursors
- Treats an exact approved-genesis retry after any head advancement as a write-free identity assertion
- Rejects conflicting genesis identity, same-hash immutable-metadata drift, partial bootstrap, unrooted history, chain corruption, and cursor corruption
- Persists an immutable local finalization witness before its successor round,
  binding the exact local predecessor, target state, frozen eligible
  latest-message map, supporting closure, authority-context digest, exact FTT,
  and finalized manifest
- Atomically stores an immutable successor round and its durable head, with the
  record bound to the persisted witness digest
- Hash-chains exact sorted finalized manifests and rejects stale, unrelated, equal-height, and regressive successors
- Persists ordered projection, contiguous effect-completion, and receipt-compaction cursors
- Receipts deploy removal, cosigner removal, runtime-cache eviction, and finalized-event publication independently
- Audits the complete hash chain on reopen and uses constant-time anchor/head checks on ordinary duplicate-genesis insertion
- Treats revision, record digest, and witness digest as node-local audit
  identity; live synchronization never imports them as consensus authority

Admission schema version `9` is the first schema with the rooted finalization
ledger. Schema `8` stores are not silently upgraded because an inferred root
would not be independently auditable. Start from a fresh protocol-v5 genesis or
use an explicit verified migration.

**`BlockMetadataStore`** -- Per-block metadata with in-memory DAG state:
- Uses `imbl` persistent collections (HashSet, OrdMap, HashMap) for structural sharing
- `add(metadata)`, `record_finalized(directly, indirectly, ft_value)`, `contains(hash)`
- `update_ft_if_higher(hashes, ft_value)` -- Batch update cached FT for blocks below threshold
- `finalized_block_hashes()` -- Returns all finalized block hashes from in-memory set
- **DAG metadata caches**: In-memory indices avoid repeated LMDB deserialization on hot paths:
  - `block_number_map`: BlockHash → block_num
  - `main_parent_map`: BlockHash → parent BlockHash
  - `self_justification_map`: BlockHash → justification BlockHash
  - `finalized_block_set`: Bounded HashSet (cap 50k, prunes to 25k) of finalized blocks

## Casper Buffer

**`CasperBufferKeyValueStorage`** -- Tracks unprocessed block dependencies:
- Two `DashMap`s: child_to_parent, parent_to_child adjacency lists
- `BlockDependencyDag` (doubly-linked DAG) rebuilt from persistent store on startup
- `add_relation(parent, child)`, `put_pendant(block)`, `remove(hash)`, `get_pendants()`

## Finality Storage

**`LastFinalizedStorage` trait**: `put(hash)`, `get()`, `get_or_else(default)`
- `LastFinalizedKeyValueStorage` (persistent)
- `LastFinalizedMemoryStorage` (in-memory)

## Deploy Storage

**`KeyValueDeployStorage`** -- Stores `Signed<DeployData>` indexed by deploy signature. Methods: `add()`, `remove()`, `read_all()`, `non_empty()`. Deploy index maps deploy signature to block hash.

## Tests

`block_dag_storage_test.rs` (example and property integration),
`finalization_ledger.rs` (restart, corruption, idempotence, and parallel append
unit tests), `key_value_block_store.rs` (property unit),
`casper_buffer_key_value_storage.rs` (Tokio async), and
`doubly_linked_dag_operations.rs` (DAG unit tests). The corresponding TLA+,
Rocq, and Loom evidence is cataloged in
[`finalization-atomicity-and-recovery.md`](../theory/finalized-floor/finalization-atomicity-and-recovery.md).

**See also:** [block-storage/ crate](../../block-storage/)

[← Back to docs index](../README.md)
