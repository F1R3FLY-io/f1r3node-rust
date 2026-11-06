> Last updated: 2026-11-06

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

**Configuration and resident state.** `casper.dag-hot-window-depth` must be positive and defaults to `1024`.
For finalized height `F`, window depth `W`, and maximum published valid height `H`, the resident band is inclusive: `max(0, F-W)..H`.
Invalid blocks cannot raise `H`.
The window bounds retained history by height, not by block count.
Unfinalized blocks above `F` remain resident through `H`.
Eviction removes resident indices without deleting cold metadata, membership, height, or child records from durable storage.

**`KeyValueDagRepresentation`.** `get_representation()` captures resident collections, latest messages, the last finalized block, maximum valid height, and publication sequence under a shared lock.
Clones share the persistent `imbl` collections.
Historical queries use durable indices and the captured publication sequence, so later block insertions remain invisible to an older view.
Queries read mutable finality metadata from storage.
The representation does not capture a historical snapshot of every metadata field.

**Query contract.** `lookup`, `contains`, `children`, `block_number`, `main_parent`, `is_finalized`, and historical traversals return `Result` values.
Cold records remain queryable after eviction, and storage failures propagate instead of becoming missing blocks or partial traversal results.
`scan_membership_page` and `scan_height_range` provide paged historical access within the captured visibility boundary.
Resident map sizes do not describe total DAG history.

**Publication and finalization.** `BlockDagKeyValueStorage` coordinates readers and writers with a `parking_lot::RwLock`.
`insert(block, mode)` persists metadata, history indices, applicable latest-message pointers, and publication progress in one atomic transaction before publishing resident state.
`record_directly_finalized(hash, ft_value, effect)` persists finality metadata and finalized progress before refreshing the resident window.
`BlockMetadataStore` maintains resident block numbers, main parents, self-justifications, adjacency, and finalized-status caches over durable metadata.

**Restart and index migration.** Startup resumes legacy metadata indexing in pages of at most 1,024 records, with each page and its cursor committed atomically.
After migration, restart checks committed table counts and finalized metadata, then rebuilds the resident band through paged height scans.
It does not rebuild the complete historical DAG in memory.
Migration work can still cover the complete metadata store.
The page limit bounds each batch, not total migration time.

**Diagnostics and test access.** `resident_counts()` reports resident cache cardinalities.
Production callers use query methods rather than resident maps, whose visibility is restricted to the crate.
`DagFixture`, `detached_fixture()`, and `fixture_mut()` require tests or the `test-internals` feature.
Fixture mutation requires a detached view and cannot modify a view backed by durable publication.

**Database compatibility.** DAG index migration changes storage indices, not block hashes or signatures.
It does not convert an existing chain to the upstream Merkle block-hash scheme v2.
Stored blocks must independently satisfy the current block verification rules.

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

**`KeyValueDeployStorage`** -- Stores `Signed<DeployData>` indexed by deploy signature. Methods: `add()`, `remove()`, `read_all()`, `any()`, `non_empty()`. Deploy index maps deploy signature to block hash.

**`KeyValueRejectedDeployBuffer`** -- Keeps deploys that a multi-parent merge rejected, so that this node can propose them again. It uses the same record format as `KeyValueDeployStorage`.

### Invalid records in the local deploy stores

The local deploy stores, `deploy_storage` and `rejected_deploy_buffer`, quarantine records with invalid encoding or signatures. Other records stay available.

- **Write checks.** Before a write, the codec checks the parameters, supported signature algorithm, and signature. Prepared writes retain the checked bytes until persistence.
- **Read checks.** Store open does not scan live records. Reads check the fingerprint of current-format records and verify signatures of historical records. The decoded signature must match the record key. Reads still deserialize deploys.
- **Record format.** Checked writes store `F1R3DEP\x02`, a 32-byte Blake2b-256 fingerprint, and the protobuf payload in one record. The fingerprint covers the payload. It detects corruption and does not replace signature verification at the write boundary. Raw writes that bypass this boundary are unsupported. No separate verification database or fingerprint cache is required.
- **Migration.** Reads verify signatures of `F1R3DEP\x01` and legacy bincode records before upgrading them. A batch replaces only records whose bytes still match the read snapshot. Concurrent replacements and deletions remain intact. Migration runs after the read transaction closes. A migration write failure preserves the verified data and requires verification on a later read.
- **Scan behavior.** An invalid record moves to `deploy_storage_quarantine` or `rejected_deploy_buffer_quarantine` with its raw bytes unchanged. Both databases use the `deploystorage` environment. The scan continues with valid records. If an `any()` predicate fails, the scan first quarantines the invalid records already found, then returns the predicate error.
- **Backend failures.** Live-store reads, admission writes, and quarantine failures stop the operation and return the error. A failed quarantine write keeps the damaged record in the live store. Migration writes use the fallback described above.
- **Unsupported format versions.** A record in a format that this node version cannot read is quarantined like a damaged record. The node does not delete it.
- **Reopen and recovery.** At each store open, the node checks quarantined records again using the format-dependent checks above. Restoration inserts only absent keys. Identical live bytes permit removal of the quarantined duplicate. Different live bytes preserve both copies. Invalid records remain in quarantine.
- **Older binaries.** Earlier binaries cannot read version 2 records. A rollback requires compatible decoding or an explicit data conversion. Existing verification-index files are unused and are not deleted automatically.
- **Key collisions.** The node never overwrites a quarantined record. If a damaged live record has a key that the quarantine already holds with different bytes, the live record stays in place and each scan skips it with a warning.
- **Interrupted move.** A move writes to the quarantine first and then deletes the live record. If the process stops between the two steps, the record is in both databases. The next scan finds identical bytes in the quarantine and completes the delete.
- **Concurrent access.** A quarantine move preserves bytes before an atomic compare-and-delete of the live record. A concurrent replacement remains available for subsequent verification. The deploy pool needs no outer mutex.
- **Admission and recovery.** Admission prepares the candidate once before retries and before acquiring the rejected-buffer mutex. Under that mutex, admission compares current bytes with inspected snapshots and uses conditional insertion. A changed record requires a new inspection outside the mutex. Recovery skips invalid candidates and writes the remaining records in one batch. The `buffer_added` event follows successful persistence.
- **Operator cleanup.** The `WARN` lines and the counters below show when records enter the quarantine. The quarantine databases hold only records that the node does not read. If you do not need a quarantined record, you can delete it from its quarantine database while the node is stopped. Do not delete records from `deploy_storage` or `rejected_deploy_buffer`.
- **Diagnostics.** Record warnings identify the store and failure reason: `decode`, `checksum`, or `signature`. They include at most 32 key bytes, byte lengths, and 160 error characters. They exclude deploy contents and private keys. A scan logs at most 10 records, then a summary when needed. Store open reports restoration, duplicate removal, conflicts, and remaining quarantined records.
- **Counters.** Both quarantine counters have a `store` label. `deploy_store.quarantined_records` also has a `reason` label.
- **Scope.** Block storage and chain-state stores keep their present error handling.

**`KeyValueRejectedDeployBuffer`** uses the `KeyValueDeployStore` implementation in a separate LMDB store. The buffer holds deploys that a merge rejected and that this validator proposes again. The buffer has no size limit. Each block removes entries whose validity window closed or that expired.

The two stores share the `deploystorage` LMDB environment with a map size of 1 GB. The `deploy.store.size` gauge uses the `store` label and shows the number of entries at the last full read. The node logs a warning when the rejected deploy buffer holds more than 1,000 entries.

## Tests

`block_dag_storage_test.rs` (proptest integration), `key_value_block_store.rs` (proptest unit), `casper_buffer_key_value_storage.rs` (tokio async), `doubly_linked_dag_operations.rs` (DAG unit tests).

**See also:** [block-storage/ crate](../../block-storage/)

[← Back to docs index](../README.md)
