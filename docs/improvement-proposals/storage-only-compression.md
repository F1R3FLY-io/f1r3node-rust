# Storage-Only Compression of Canonical Cold Leaves

Status: draft proposal. No implementation, migration, or performance claim is included.
Date: 2026-11-25.

## Objective

Reduce cold-store disk usage without changing canonical bytes, content hashes, state roots, or peer messages.
Start with `ContinuationsLeaf`, which dominates the [measured cold-store bytes](README.md#measurement-baseline).
The reader must still support all three leaf variants.

This proposal does not replace bincode, alter the Par schema, or remove historical state.
It does not promise smaller decoded interpreter objects or lower execution memory.

## Current Path

The [serializer](../../rspace++/src/rspace/serializers/serializers.rs) encodes each waiting continuation with bincode.
It sorts the encoded records and encodes their vector.
The [checkpoint writer](../../rspace++/src/rspace/history/history_repository_impl.rs) hashes the leaf encoding and stores a `PersistedData` value.
The [LMDB adapter](../../shared/src/rust/store/lmdb_key_value_store.rs) adds its byte-vector encoding.
LMDB means Lightning Memory-Mapped Database.

The [history reader](../../rspace++/src/rspace/history/instances/rspace_history_reader_impl.rs) decodes the stored value.
State export and import also access the cold store.
Changing only the history reader would leave those paths incompatible.

## Proposed Boundary

Introduce a cold-store codec below the logical `KeyValueStore` interface.
Install one shared adapter when the [history repository](../../rspace++/src/rspace/history/history_repository.rs) receives its cold store.
Give that adapter to checkpoint writes, readers, importers, and exporters.
Do not apply compression to every LMDB database.

```mermaid
flowchart LR
    A["Existing canonical serializer"] --> B["Canonical PersistedData bytes U"]
    B --> C["Cold-store codec"]
    C --> D["Raw bytes or versioned compressed record"]
    D --> E["Existing LMDB adapter"]
    E --> F["Cold database"]
    F --> G["Bounded decode and integrity check"]
    G --> H["Exact original bytes U"]
    H --> I["Runtime reader or canonical state export"]
```

The [shared contract](README.md#shared-contract) defines `U` and the existing leaf-hash input.
The codec must not feed compressed bytes into any consensus hash.

## Physical Format

Use a versioned record with an unambiguous discriminator.
Choose a magic prefix that cannot start a valid legacy `PersistedData` value.
The current fixed-width enum tags are 0, 1, and 2.
Reserve a distinct prefix and test the distinction against every legacy variant.
The final layout remains an implementation decision.

| Field | Purpose |
| --- | --- |
| Magic and format version | Distinguish the physical format from legacy bytes |
| Codec identifier and flags | Select one supported decoder without format guessing |
| Canonical byte length | Bound allocation and verify the exact decoded length |
| Stored payload length | Reject truncated records and trailing bytes |
| Digest of canonical `U` | Protect the complete value, including the leaf-kind tag |
| Compressed payload | Recover exactly the original `U` |

Use the existing `lz4_flex` dependency for the first benchmark.
The [block store](../../block-storage/src/rust/key_value_block_store.rs) already uses that library.
Do not copy an unbounded decompression path or assume block-format compatibility.
The cold-store record needs its own version and limits.

Keep the legacy representation when compression does not save space after record overhead.
A compression-size threshold and minimum saving remain benchmark parameters.
Do not create a new compression dictionary or cross-record dependency in this proposal.

## Writes and Reads

The proposed write sequence is:

1. Accept the existing canonical value and unchanged logical key.
2. Validate the supported leaf kind and existing leaf hash.
3. Attempt compression within the configured work and allocation budgets.
4. Select compressed storage only when its complete record is smaller.
5. Commit the physical value through the existing storage transaction.

An optimization-budget limit can select raw storage before the write.
An invalid canonical value or storage failure must return an error.
A valid legacy value above the compression limit remains raw.
The storage optimization must not introduce a new consensus validity limit.

The proposed read sequence is:

1. Detect legacy bytes or a supported physical record.
2. Check the header and all lengths before allocation.
3. Decode into a bounded buffer.
4. Check exact output length and complete input consumption.
5. Verify the canonical-value digest and existing leaf hash.
6. Return the original canonical bytes.

Reject unsupported versions, invalid lengths, corrupt payloads, and excessive allocations.
Do not retry a recognized but corrupt compressed record as legacy data.
Do not mask corruption as a missing key, empty leaf, or successful read from a lower-priority key.

Limits must cover stored bytes, decoded bytes, concurrent decoders, and aggregate scratch memory.
Release decoder buffers or cap their retained capacity.
A per-record limit alone cannot prevent memory pressure from concurrent readers.
Determine limits from existing valid records and adversarial fixtures before enabling compressed writes.

## Storage Interface Coverage

The adapter must preserve the [store contract](../../shared/src/rust/store/key_value_store.rs).

| Operation | Required behavior |
| --- | --- |
| `get`, `get_one` | Return canonical values and propagate corruption |
| `put`, `put_one`, `put_if_absent`, `put_one_if_absent` | Encode values without changing key identity or insertion semantics |
| `iterate`, `iterate_while`, `scan_page`, `to_map` | Expose canonical values through the logical interface |
| `delete_if_value` | Compare canonical bytes and delete atomically against the matching physical record |
| `delete`, `contains`, `non_empty` | Preserve existing key semantics and error behavior |
| `clone_box` | Preserve the codec and its shared resource limits |
| `as_any` and optimized batches | Prevent downcast or direct-LMDB paths from bypassing the codec |
| `size_bytes` and metrics | Distinguish physical bytes from canonical bytes |

Equivalent raw and compressed records can represent the same value.
Conditional deletion must not depend on deterministic compressor output.
Do not replace an atomic compare-and-delete with separate logical reads and unconditional deletes.

The [exporter](../../rspace++/src/rspace/state/instances/rspace_exporter_store.rs) must return canonical `PersistedData` bytes.
The [import validator](../../rspace++/src/rspace/state/rspace_importer.rs) must retain its existing hash and leaf-kind checks.
The [import store](../../rspace++/src/rspace/state/instances/rspace_importer_store.rs) can compress only after those checks.
Keep the existing raw-hash-first key lookup order.
Do not combine this change with key-format migration.

## Migration and Rollback

Introduce read support before enabling compressed writes.
Existing raw rows remain valid, and new raw and compressed rows can coexist.
Start with compression disabled and enable it only in controlled tests.
Integrate the option into existing node configuration rather than adding a separate configuration system.

Background migration is a later phase, not a startup prerequisite.
Use bounded pages, short transactions, and atomic compare-and-replace operations.
Record progress only after the corresponding batch commits.
Preserve keys and do not alter trie roots or retained-root selection.
If the compare-and-replace operation is unavailable, defer online migration.

Disabling compressed writes does not make an old binary compatible with compressed rows.
Rollback requires verified expansion of every compressed row into the legacy representation.
Prefer an offline conversion with the shard stopped and a consistent backup available.
Compare canonical bytes, leaf hashes, and root readability before using the old binary.
Do not restore only an old cold database into newer history databases.

Compression can reduce live pages without shrinking an existing LMDB file.
Report file allocation, live pages, and canonical bytes separately.
File compaction is a separate maintenance operation.

## Validation

| Test group | Required evidence |
| --- | --- |
| Exact round trip | `decode(encode(U)) == U` for every captured row and generated valid fixture |
| Leaf variants | Empty and populated DataLeaf, ContinuationsLeaf, and JoinsLeaf values |
| Continuation semantics | Multiple records, duplicates, persistence, peeks, guards, random state, and native continuations remain exact |
| Failure behavior | Truncation, bad lengths, extra bytes, unknown codec, damaged payload, and expansion attacks return errors |
| Store operations | All adapter methods, concurrent insertion, conditional deletion, and both key formats preserve behavior |
| Crash and restart | Interrupted writes and migration batches leave readable committed state |
| State transfer | Raw and compressed nodes exchange identical canonical state in both directions |
| Consensus regression | Identical checkpoint roots, replay results, deploy verdicts, and finalized histories |
| Load regression | Counter updates and bridge-style locks execute once and reach finalization without new restarts |

Use disposable shards for mixed-format and load tests.
Do not use the current running shard as a migration experiment.
Include genesis, state-sync bootstrap, retained historical roots, and rollback in the test matrix.
Compare deterministic replay outputs with identical ordered inputs.
For live shards, require consistent finalized state and continued progress rather than identical proposal timing.

## Benchmark and Metrics

Compare raw storage and compression on the same canonical corpus.
Measure each node separately before reporting replica totals.
Use both fresh databases and migrated databases to expose page-fragmentation effects.

- Measure complete encoded bytes, live LMDB pages, and allocated file bytes.
- Measure compression and decompression latency at p50, p95, and p99.
- Measure checkpoint, replay, state-transfer, and finalized-deploy latency.
- Measure CPU time, peak scratch memory, process memory, and reader concurrency.
- Count raw fallbacks, codec failures, limit failures, and bytes saved after headers.
- Test incompressible data and largest valid leaves, not only the current continuation-heavy snapshot.

Here, p50, p95, and p99 are the 50th, 95th, and 99th percentiles.
Use bounded metric labels such as leaf kind, codec, and result.
Do not label metrics with leaf hashes or export contract payloads.
Set rollout thresholds from baseline results before enabling the feature.
Do not report the 62.37% structural cost as an expected compression ratio.

## Delivery and Open Decisions

1. Build a bounded offline benchmark using existing canonical bytes.
2. Specify and test the record discriminator, codec, and decoder budgets.
3. Implement the shared adapter with compressed writes disabled.
4. Validate restart, state transfer, and unchanged consensus outputs.
5. Measure controlled mixed-format shards under user traffic.
6. Evaluate migration and rollback before any default change.

Open decisions include codec parameters, size thresholds, resource budgets, adapter placement, and migration transaction support.
State-root equality and byte equality are release conditions.
Storage savings alone do not justify CPU, latency, or memory regressions.
