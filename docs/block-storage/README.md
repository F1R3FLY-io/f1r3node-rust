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

**`BlockDagKeyValueStorage`** -- Live mutable DAG with global `Mutex`:
- `get_representation()` -- Atomic snapshot (acquires lock)
- `insert(block, invalid, approved)` -- Add block with metadata updates
- `record_directly_finalized(hash, ft_value, effect)` -- Async finalization with cached FT
- `propagate_ft_to_finalized_blocks(ft_value)` -- Update all finalized blocks with lower cached FT

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

**`KeyValueDeployStorage`** -- Stores `Signed<DeployData>` indexed by deploy signature. Methods: `add()`, `remove()`, `read_all()`, `any()`, `non_empty()`. Deploy index maps deploy signature to block hash.

**`KeyValueRejectedDeployBuffer`** -- Keeps deploys that a multi-parent merge rejected, so that this node can propose them again. It uses the same record format as `KeyValueDeployStorage`.

### Undecodable records in the local deploy stores

The two local deploy stores, `deploy_storage` and `rejected_deploy_buffer`, quarantine a record that they cannot decode. The other records stay available.

- **Scan behavior.** When `read_all()` or `any()` finds an undecodable key or value, the node moves the raw bytes, unchanged, to a sibling LMDB database. The databases are `deploy_storage_quarantine` and `rejected_deploy_buffer_quarantine`. Both are in the `deploystorage` environment. The scan then continues with the remaining records. Block preparation, the pending-deploy check, and `list_pending_deploys` use these scans, so one damaged record does not stop them. If the predicate of `any()` returns an error, the node first quarantines the damaged records that it found and then returns the error.
- **Backend failures.** An LMDB read or write failure is not a decode failure. It stops the operation and returns the error, as before. A failure to write to the quarantine database also stops the scan and keeps the damaged record in the live store.
- **Unsupported format versions.** A record in a format that this node version cannot read is quarantined like a damaged record. The node does not delete it.
- **Reopen and recovery.** At each store open, the node tries to decode every quarantined record again. It moves each readable record back to the live store if the live store does not have that key. If the live store has the same bytes under that key, the node removes the quarantined copy. If the live store has different bytes under that key, the node keeps both copies and logs a warning. Thus, a downgrade followed by an upgrade to a version that reads the format loses no records. A record that stays unreadable stays in quarantine and is not scanned again. The client that submitted that deploy can submit it again.
- **Key collisions.** The node never overwrites a quarantined record. If a damaged live record has a key that the quarantine already holds with different bytes, the live record stays in place and each scan skips it with a warning.
- **Interrupted move.** A move writes to the quarantine first and then deletes the live record. If the process stops between the two steps, the record is in both databases. The next scan finds identical bytes in the quarantine and completes the delete.
- **Locking.** `read_all()` and `any()` delete records from the live store. Call them under the same lock as `add()` and `remove()`. Casper holds both stores behind a mutex, which satisfies this requirement.
- **Operator cleanup.** The `WARN` lines and the counters below show when records enter the quarantine. The quarantine databases hold only records that the node does not read. If you do not need a quarantined record, you can delete it from its quarantine database while the node is stopped. Do not delete records from `deploy_storage` or `rejected_deploy_buffer`.
- **Diagnostics.** Each quarantined record produces one `WARN` line from `block_storage::rust::deploy::deploy_record_quarantine`. The line contains the store name, the first 32 key bytes in hex, the key length, the value length, and an error text cut to 160 characters. It never contains the deploy contents or a private key. A scan logs at most 10 records and then one summary line. A store open that finds quarantined records logs one `WARN` line with the restored, removed-duplicate, kept-on-conflict, and remaining counts. The counters `deploy_store.quarantined_records` and `deploy_store.restored_records` have a `store` label.
- **Scope.** Block storage and chain-state stores keep their present error handling.

**`KeyValueRejectedDeployBuffer`** uses the `KeyValueDeployStore` implementation in a separate LMDB store. The buffer holds deploys that a merge rejected and that this validator proposes again. The buffer has no size limit. Each block removes entries whose validity window closed or that expired.

The two stores share the `deploystorage` LMDB environment with a map size of 1 GB. The `deploy.store.size` gauge uses the `store` label and shows the number of entries at the last full read. The node logs a warning when the rejected deploy buffer holds more than 1,000 entries.

## Tests

`block_dag_storage_test.rs` (proptest integration), `key_value_block_store.rs` (proptest unit), `casper_buffer_key_value_storage.rs` (tokio async), `doubly_linked_dag_operations.rs` (DAG unit tests).

**See also:** [block-storage/ crate](../../block-storage/)

[← Back to docs index](../README.md)
