# Storage Cardinality and Size

This assessment covers all 38 logical database names in the [storage inventory](README.md).
It also covers filesystem entities and logical contract records.
[Observed sizes](observed.md) report counts and page usage from the local shard.

## Units and Encoding

`K` and `V` are logical key and value byte lengths before the LMDB adapter.
The node's [LMDB adapter](../../../shared/src/rust/store/lmdb_key_value_store.rs) encodes each byte vector with bincode.
Each vector adds an 8-byte length prefix. Thus an adapter row uses `K + V + 16` encoded bytes.
This does not include LMDB page or record overhead.

The [typed store](../../../shared/src/rust/store/key_value_typed_store_impl.rs) usually adds an inner bincode encoding first.
A typed 32-byte hash therefore uses 48 physical key bytes: 32 data bytes and two 8-byte lengths.
Queue `meta` is an exception. It uses raw `Bytes` keys and values without this adapter.

Definitions for the tables below:

- `B`: published DAG records. `E`: stored parent-child index rows. `D`: retained deploy signatures.
- `H`: 32-byte block or state hash, where that invariant applies. `S`: actual signature length. `A`: validator-key length.
- `P`: protobuf payload byte length. `C`: compressed record byte length, including its uncompressed-length prefix.
- `bc(T)`: actual bincode encoding length of value `T`, including custom serde behavior.
- `N`: elements in the record's vector. `L`: a byte-string length. `O`: 1 when an optional value is present, otherwise 0.

Formulas below include the outer adapter prefixes unless marked raw.
They assume current encodings and valid 32-byte hashes. Genesis sender fields can be empty.
Signatures are not assumed to have one fixed length.
The fixed-int bincode format uses 8-byte vector/string lengths, 4-byte enum tags, 1-byte option tags and booleans, and 8-byte i64/u64 values.
Custom serializers can change these rules. `TokenAmount`, for example, encodes a decimal string rather than a fixed 32-byte integer.

## Blocks and DAG

| Named database | Cardinality and growth | Encoded bytes per row |
| --- | --- | --- |
| `blocks` | Zero or one retained body per block hash. Body pruning can leave metadata without a body | `48 + C` |
| `blocks-approved` | Zero or one approved-genesis record, key byte 42 | `17 + P` |
| `reporting-cache` | Zero or one cached report per queried block hash | `48 + C`, using the report codec |
| `transaction` | Registered only. No production writer found at this revision | Unknown schema. Do not assign a fabricated row size |
| `block-metadata` | One per published DAG record. Recovery checks its count against publication totals | `56 + bc(BlockMetadata)` |
| `latest-messages` | At most one latest message per stored validator identity | `A + 64` |
| `dag-membership-v1` | One per published DAG record, count `B` | **65**: hash 32, sequence 8, height 8, flag 1, adapter 16 |
| `dag-height-v1` | One per published DAG record, count `B` | **64**: height/hash key 40, sequence 8, adapter 16 |
| `dag-children-v1` | One per indexed parent-child identity, count `E` | **88** for normal 32-byte parent and child. Nonstandard parent identities use a tagged key and a variable collision-record value |
| `dag-sequence-v1` | One per published DAG record, count `B` | `A + 61`, or 126 for a 65-byte sender |
| `dag-equivocations-v1` | Subset of sender-sequence rows for invalid equivocations | `A + 61` |
| `genesis-hash` | Zero or one row | **71** |
| `dag-history-state-v1` | At most five reserved keys in this revision | `len(key) + len(encoded value) + 16`. `dag.progress` uses 54..110 total bytes with valid hashes |
| `dag-gc-retry-v1` | One per retained GC retry candidate | **56** |
| `floor-index` | Zero or one cached derived floor per block | **96** |
| `frontier-index` | Zero or one cached frontier per block | **96** |
| `mergeable-channel-cache` | One per retained state/creator/sequence key. Each value has zero or more deploy change groups | `24 + bc(MergeableKey) + bc(Vec<DeployMergeableData>)`. The typed byte-vector key adds an inner 8-byte length |

`BlockMetadata` depends on parent, justification, and validator counts.
With normal hashes, its bincode payload is:

```text
99 + sender_bytes + 40 * parent_count
   + sum(48 + justification_validator_bytes)
   + sum(16 + bonded_validator_bytes + decimal_stake_digits)
   + merge_base_bytes + 40 * has_equivocation
```

This is an encoded-record formula, not a heap formula.
The [metadata fields](../../../models/src/rust/block_metadata.rs), [serde adapters](../../../shared/src/rust/mod.rs), and [token serializer](../../../shared/src/rust/token_amount.rs) determine it.

`DagProgress` contains two optional fields and uses 26..82 payload bytes.
`DiscoveryState` uses `3 + 8*has_claimed + 8*has_completed + has_cursor*(8 + cursor_length)` payload bytes.
The `gc.retry-cursor` value is raw cursor bytes, not a bincode `Option`.
The effective-anchor and restore-floor values are encoded i64 heights.
See [publication](../../../block-storage/src/rust/dag/dag_persistence.rs), [history keys](../../../block-storage/src/rust/dag/dag_history_index.rs), and [GC records](../../../block-storage/src/rust/dag/dag_gc_store.rs).

## Deploy Records

| Named database | Cardinality and retention | Encoded bytes per row |
| --- | --- | --- |
| `deploy_storage` | One per pool signature. Defaults: at most 512 records and 16 MiB of charged encoded deploy data | `S + 56 + P` for the current checked codec |
| `rejected_deploy_buffer` | One per retained rejected signature. Retry and expiry cleanup apply. Do not apply the pool's 512-row cap to this table | `S + 56 + P` |
| `deploy_storage_seeds` | Zero or one holder per name seed in the pool's claim table | `K + S + 16` |
| `rejected_deploy_buffer_seeds` | Zero or one holder per name seed in the rejection buffer's claim table | `K + S + 16` |
| `deploy_storage_quarantine` | One per quarantined original key. Not an active pool record | `K + original_value_bytes + 16` |
| `rejected_deploy_buffer_quarantine` | One per quarantined original key | `K + original_value_bytes + 16` |
| `deploy-lifecycle-events` | One per open signature. Zero or more events per row. Removed at terminal settlement | `S + 33 + 8*has_valid_after + 53*N` |
| `deploy-lifecycle-terminal` | At most one immutable verdict per signature. Grows with decided deploy history | `S + 80` with a 32-byte latest block hash. A missing hash makes the record 32 bytes shorter |
| `carrier-index` | One row per retained signature. Zero or more carrier blocks. Pruned by height window | `S + 32 + 48*N` |
| `carrier-index-seeds` | One row per retained seed. Zero or more signature/block carriers | `seed_length + 32 + sum(56 + carrier_signature_length)` |
| `carrier-index-meta` | At most four reserved keys | `key_string_length + 32` |

The checked deploy codec adds an 8-byte format marker and a 32-byte fingerprint before the protobuf payload.
The logical pool and seed relationships are not SQL foreign keys.
Crash recovery and quarantine can create temporary differences between table counts.
See [deploy storage](../../../block-storage/src/rust/deploy/key_value_deploy_storage.rs), [checked codec](../../../block-storage/src/rust/deploy/deploy_codec.rs), [lifecycle types](../../../block-storage/src/rust/dag/deploy_lifecycle_types.rs), and [carrier index](../../../block-storage/src/rust/dag/carrier_index.rs).

## Tuple Space and Queue

| Named database | Cardinality and retention | Encoded bytes per row |
| --- | --- | --- |
| `rspace-history` | One per retained distinct radix-node hash. Multiple roots share nodes | `48 + sum(34 + prefix_length)` over nonempty slots |
| `eval-history` | Same schema for the evaluator environment | Same formula as `rspace-history` |
| `rspace-roots` | One marker per recorded root plus zero or one current-root pointer | **51** per root marker, **60** for the pointer |
| `eval-roots` | Same root schema in the evaluator environment | **51** or **60** |
| `rspace-cold` | One per retained leaf hash. Data, continuation, or join payload | `56 + bc(PersistedData)` |
| `eval-cold` | Same cold schema in the evaluator environment | `56 + bc(PersistedData)` |
| `rspace-channels` | Legacy compatibility mapping. Not an additional current writer | Legacy schema not assigned a current record-size formula |
| `records` | One queue record per retained signature. Some terminal records remain after payload deletion | `S + 16 + bc(QueueRecord)` |
| `payloads` | Zero or one signed deploy payload per queue record. Removed on terminal decision | `S + P + 16` |
| `meta` | One queue-format row once initialized | **10 raw bytes**: key `format` is 6 bytes and version is 4 bytes |

A radix node has 256 possible slots. Each nonempty slot has a two-byte header, a prefix of at most 127 bytes, and a 32-byte hash.
Its compact encoded payload is at most 41,216 bytes by that format bound.
This bound does not imply every such node is reachable or valid for a particular history key.
See [radix encoding](../../../rspace++/src/rspace/history/radix_tree.rs) and [root records](../../../rspace++/src/rspace/history/roots_store.rs).

The queue payload budget defaults to 8 GiB. Its map limit is 16 GiB.
The node's LMDB environment map limits are separate from both used pages and file length.
See [queue records](../../../deploy-queue/src/record.rs) and [queue storage](../../../deploy-queue/src/store.rs).

For `QueueRecord`, the bincode payload is:

```text
57 + deployer_bytes
   + 8 * (has_submitted_height + has_last_sent_height + has_decided_height)
   + has_latest_hash * (8 + hash_length)
   + has_detail * (8 + detail_UTF8_bytes)
```

The base includes five option tags. It excludes the key and the outer LMDB adapter.

## Files and Logical Entities

| Entity | Cardinality | Data size |
| --- | --- | --- |
| LMDB environment | One directory per mapped environment, with multiple named databases | `data.mdb` file length, allocated disk blocks, and live table pages are different measurements. `lock.mdb` is separate |
| WAL snapshot | Zero or more retained content-addressed snapshots | Version byte + u32 entry count + sum of custom entry encodings: `5 + sum(entry_bytes)` |
| `WalEntry` and its result/reference alternatives | One per recorded filesystem operation in a snapshot | Custom tagged format. Paths, results, inline values, and references vary. Use the actual snapshot encoder |
| Payload-hash sidecar | Zero or one `.hashes` file per successfully written snapshot | `4 + 32*N` for N distinct referenced payload hashes |
| Manifest | Zero or one `manifest.jsonl` per snapshot directory | Sum of UTF-8 JSON line lengths, including newlines. One entry per recorded snapshot or empty sentinel |
| Payload file | Zero or one file per retained content hash | Raw payload length. Filesystem block allocation can be larger |
| Snapshot chunk | `ceil(snapshot_bytes / 4 MiB)` nonempty chunks | At most 4 MiB of data per chunk, plus transfer metadata. Chunks are not additional persistent copies by definition |
| Detached DAG capture | Zero or more explicit diagnostic exports | Format- and capture-dependent. Not a serialization of the full heap |
| Known peers | One optional saved file per node, zero or more unique peer lines | Sum of encoded address-line bytes and separators |
| TLS certificate and private key | One configured identity pair per node | PEM file byte lengths. Contents must not be exported in this inventory |
| Node configuration and genesis inputs | One selected configuration and zero or more configured input files | File byte lengths. Operational inputs, not growing consensus tables |
| Diagnostic logs | Files or runtime log streams according to logging configuration | Retention and rotation configuration determine totals, not the entity schema |
| Rholang `Par`, datum, continuation, and join | Zero or more recursive values per channel or history leaf | Variable encoded AST and tuple-space payload. Persisted inside cold leaves, not separate LMDB tables |
| Registry, versioned registry, PoS maps, vaults, and purses | Contract-defined keys and records in tuple space | Measure encoded tuple-space values and retained history. Do not double-count these as new tables |

The purse single-value invariant is a contract invariant for a purse channel.
It is not a guarantee that only one historical leaf contains that purse state.
Contract map growth, history versions, trie sharing, and garbage collection determine retained bytes.
See [filesystem diagrams](filesystem.svg), [contract state](contract-state.svg), and the [snapshot codec](../../../rholang/src/rust/interpreter/io/snapshot.rs).

## Physical Accounting

The read-only probe reports exact row counts and live B-tree page usage within one read transaction per environment.
It samples only the first 256 rows per table, with a two-second sampling budget per environment.
Sample means are not population means, and sample byte totals are not whole-table totals unless the sample is complete.
Empty tables have zero rows. An absent database is reported separately, not as a measured empty table.

Do not add decoded heap bytes to encoded storage bytes as though they were one allocation.
Do not sum replica counts to estimate unique chain entities.
Do not equate map size, data-file length, allocated disk blocks, live table pages, or RSS.
LMDB free pages, environment metadata, and page slack explain differences between these measurements.
