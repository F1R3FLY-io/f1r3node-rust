# Cold-Store Compression Implementation

Status: experimental implementation. Production rollout and formal acceptance remain pending.
Date: 2026-11-26.

## Scope

This implements the codec and compatibility phase of the [storage-only compression proposal](storage-only-compression.md).
The [adapter](../../rspace++/src/rspace/history/cold_store_codec.rs) preserves the logical `KeyValueStore` interface.
The history repository installs read support before it creates readers, checkpoint writers, importers, and exporters.
Existing wrappers preserve their write setting and shared resource budget.

The writer compresses only continuation leaves.
The decoder accepts compressed records for all three leaf variants.
Data and join writes remain raw during this initial evaluation.
No canonical serializer, key format, state hash, peer message, pruning rule, or execution rule changes.

## Configuration

The existing node configuration accepts:

```hocon
storage {
  cold-store-compression = false
}
```

The default is false, including configurations that omit the field.
Set it to true only for controlled evaluation on disposable state.
Node setup applies the setting to consensus, evaluation, and reporting stores.
Library callers can use `RSpaceStore::with_cold_compression(true)` for an explicitly owned test store.

Both settings read raw and compressed rows.
Disabling writes does not expand existing compressed rows.
An older binary without this decoder cannot safely open a database that contains compressed rows.

## Physical Record V1

The header uses fixed-width little-endian bincode fields.
It occupies exactly 60 bytes.
Its magic cannot be a valid legacy enum tag of 0, 1, or 2.
The payload is one independent LZ4 block, without a dictionary or an additional size prefix.

| Offset | Size | Field | Accepted value |
| --- | --- | --- | --- |
| 0 | 8 | Magic | `46 31 43 4c 5a 34 00 00` |
| 8 | 1 | Version | `1` |
| 9 | 1 | Codec | `1`, LZ4 block |
| 10 | 2 | Flags | `0` |
| 12 | 8 | Canonical length | Exact length of `PersistedData` bytes |
| 20 | 8 | Payload length | Exact number of bytes after the header |
| 28 | 32 | Digest | Blake2b-256 of the complete canonical value |
| 60 | Variable | Payload | Compressed canonical value |

The decoder rejects unknown versions, codecs, and flags.
It checks lengths before allocating decoded output.
It requires exact decoded length and validates the complete canonical digest.
It then validates the legacy leaf envelope and existing leaf hash against the unchanged key.

Borrowed deserialization validates the outer canonical value without allocating its nested leaf bytes.
The hash input remains the leaf's encoded byte vector, excluding the `PersistedData` enum tag.
Both raw 32-byte hash keys and their existing bincode representation remain supported.
Trailing canonical bytes and corrupt recognized records return storage errors.

The implementation uses the locked `lz4_flex` version, 0.11.6.
Its [block decoder](https://docs.rs/lz4_flex/0.11.6/lz4_flex/block/fn.decompress_into.html) consumes the supplied input block.
Regression tests also cover trailing payload bytes and short decoded output with otherwise matching zero padding.

## Initial Limits

These are conservative experimental parameters, not workload-derived production settings.
MiB means 1,048,576 bytes.

| Parameter | Value | Effect |
| --- | --- | --- |
| Minimum canonical size for compression | 1,024 bytes | Smaller values remain raw |
| Required saving after the header | 64 bytes | Less profitable values remain raw |
| Largest compressed physical record | 16 MiB | Larger recognized records return an error |
| Largest canonical value in a compressed record | 16 MiB | Header check precedes decoded-buffer allocation |
| Concurrent compression/decompression operations | 2 per process | Shared by all adapter instances and clones |
| Accounted active scratch | 64 MiB per process | Input and output reservations share one budget |
| Encoder workspace allowance | 1 MiB per active encoder | Additional allowance for the locked library's workspace |
| Conditional-delete attempts | 8 | Persistent representation races return a contention error |

Oversized valid legacy records remain readable and writable as raw values.
The compression limit does not become a new valid-state limit.
An encoder uses raw storage when no immediate scratch reservation is available.
Decoder calls wait for scratch without holding a codec permit needed by another decoder.
Permits cover synchronous codec work and release on every return path.
Raw canonical validation does not acquire a permit; the two-operation limit does not bound raw hashing concurrency.

Returned values and completed write batches belong to callers, not the scratch budget.
The underlying store also allocates physical input buffers before codec inspection.
Thus, the budget does not cap complete batch memory, arbitrary legacy-row sizes, or process resident memory.
The encoder releases its input before it discards unused output capacity.
This ordering leaves the input reservation available for a possible output reallocation.

Do not raise format limits locally without compatibility review.
Every binary that reads those records must support their decoded size.
Recheck workspace accounting when updating the compression dependency.

## Interface and Transactions

Point reads, iteration, scans, and maps return canonical values.
Presence queries retain decode-error propagation.
Page cursors and backend ordering remain unchanged.
`size_bytes` reports physical storage rather than expanded canonical bytes.
For LMDB, it uses this database's branch, leaf, and overflow pages.
It excludes environment metadata, free pages, and other databases.
The fallible `try_size_bytes` method reports statistics failures.
The infallible trait method returns zero with a warning and failure metric when statistics are unavailable.
Diagnostic printing reports counts and sizes without exposing contract bytes.

Writes validate and encode the complete batch before the existing backend transaction.
`put_one_if_absent` retains the backend's atomic insertion operation.
Batch `put_if_absent` retains its existing read-then-write semantics.
This change does not claim atomic conditional insertion for an entire batch.

Conditional deletion first decodes the physical row and compares canonical bytes.
It then uses the backend's atomic compare-and-delete against that exact physical row.
A concurrent representation change triggers a retry, not an unconditional delete or a false mismatch.
A different canonical value remains intact.

The wrapper exposes itself through `as_any`, not its LMDB backend.
Optimized callers therefore cannot downcast through it and write unencoded values accidentally.
The history and roots stores remain unwrapped, preserving their existing atomic checkpoint publication path.

## Metrics

The metric names start with `rspace.cold-codec`.

| Suffix | Meaning |
| --- | --- |
| `encode.seconds`, `decode.seconds` | Codec operation latency, including validation |
| `raw` | Raw selections, with a bounded `reason` label |
| `encoded.canonical-bytes` | Canonical bytes from successful compression attempts |
| `encoded.physical-bytes` | Complete selected compressed records, including headers |
| `encoded.saved-bytes` | Difference between canonical values and selected records |
| `decoded.bytes` | Canonical bytes recovered from compressed records |
| `failures` | Failed encode, decode, or size calls, with an `operation` label |
| `limit-failures` | Physical, decoded, or scratch limit refusals |
| `scratch.bytes`, `active` | Current shared reservations and operation count |

Encoding metrics count attempts, not committed disk savings.
A later storage error or conditional-insert conflict can prevent publication.
These metrics exclude LMDB page allocation and file fragmentation.
They contain no leaf hashes, signatures, or contract contents.

## Validation

The [codec tests](../../rspace++/src/rspace/history/cold_store_codec/tests.rs) cover:

- Every legacy leaf tag, empty and populated values, and both key formats.
- Raw fallback for small, incompressible, oversized, disabled, and budget-constrained writes.
- Invalid keys, tags, lengths, digests, compressed payloads, and trailing input.
- Exact output lengths, arbitrary canonical bytes, and malformed physical records.
- Store iteration, paging, presence checks, cloning, insertion, and conditional deletion.
- Concurrent insertion, representation replacement, budget sharing, and reservation release.

The [integration tests](../../rspace++/tests/cold_store_compression.rs) cover:

- Equal checkpoint roots and canonical exports with raw and compressed storage.
- Mixed-format state transfer in both directions through the existing import validator.
- Raw-key priority, with no serialized-key fallback after corruption.
- Retained roots and mixed rows after an LMDB reopen.
- An aborted LMDB write transaction that preserves the previous committed row.
- Replay of persisted multi-channel continuations with peeks and unchanged effects.

Run the focused and existing regression suites from the repository root:

```bash
scripts/ci/run-tests.sh cargo test --locked -p rspace_plus_plus --lib --test cold_store_compression -j 2
scripts/ci/run-tests.sh cargo test --locked -p rspace_plus_plus --test export_import_tests --test replay_rspace_tests -j 2
scripts/ci/run-tests.sh cargo test --locked -p rspace_plus_plus --test mod --example cold_store_compression -j 2
scripts/ci/run-tests.sh cargo test --locked -p node --lib cold_store_compression_tests -j 2
```

The 2026-11-26 run passed 175 library tests, with one existing ignored test.
It also passed all five new integration tests, 12 existing export/import tests, and 24 existing replay tests.
The broader `mod` integration target passed 177 tests, with three existing timing-test skips.
All three benchmark-corpus tests passed.
The node configuration regression and `cargo check --locked -p node --lib -j 2` passed.
Node builds used protoc 27.3 because the system protoc 3.6.1 does not support the existing proto3 optional fields.
Clippy passed for the RSpace library, tests, and examples with warnings denied.

The [correctness claim record](../claims/rspace-cold-store-compression.md) separates regression evidence from formal acceptance.
The [formal and live validation record](../cbc-evidence/cold-store-compression-2026-11-26.md) records the model, proof, and shard checks.
The positive TLC model passed, and all five deliberate defects produced their expected counterexamples.
Coq checked 16 abstract adapter theorems, with no admitted results or axioms.
Two new operation-trace tests compare the production adapter against independent store and reservation models.
These checks do not establish a complete Rust refinement proof.
A transaction-abort test is not a power-loss or process-kill test.
Generic RSpace fixtures do not establish live Rholang or network finalization performance.
The disposable mixed-format shard finalized 100 counter deploys and 100 synthetic lock-record deploys.
Its late observer returned exact totals at a block finalized by all validators, including after a compressed-write-disable restart.
The initial run reached finalized height 79 without an automatic restart or OOM kill.
Offline inspection of its stopped databases passed after a diagnostic-tool fix.
A second fresh-genesis run completed the corrected combined harness with exit code zero, including final state verification at height 68 and stopped-store inspection.
See the validation record for the source-bound results, recorded failure, and scope limits.
The existing CbC gate still reports missing evidence for the changed history-repository factory.
No existing verification policy or waiver is changed by this implementation.

## Offline Benchmark

The [benchmark](../../rspace++/examples/cold_store_compression.rs) uses isolated memory stores.
It does not open a node database, modify a live shard, or perform a migration.
It measures legacy unwrapped storage, validated raw storage, and compression separately.
The legacy baseline exposes the new integrity-check cost instead of hiding it in both comparison paths.

```bash
cargo run --locked --release -p rspace_plus_plus --example cold_store_compression -- --synthetic
cargo run --locked --release -p rspace_plus_plus --example cold_store_compression -- --corpus CORPUS_DIRECTORY
```

Each corpus file contains one canonical value.
Its filename is the hexadecimal logical key followed by `.bin`.
The directory must contain only regular `.bin` files.
The benchmark verifies key/value integrity and exact round trips before reporting success.

The tool limits the corpus to 256 rows, 64 MiB per file, and 120 seconds between bounded operations.
It retains only one input row and its comparison stores at a time.
Each mode uses eight iterations per row and reports p50, p95, and p99 wall-clock latency.
With eight samples, p95 and p99 both select the slowest observation.
Those percentiles are smoke-test measurements, not production tail-latency estimates.

Complete row sizes include bincode's outer key and value wrappers.
They exclude LMDB pages, filesystem allocation, and replication.
Synthetic payloads test repetitive and incompressible bytes, not complete contract workload distributions.
The earlier histogram report cannot reconstruct its original canonical payload corpus.

### Synthetic Results

Five release-mode runs completed on 2026-11-26 on an Intel Xeon W-2225 host.
Each run covered 30 rows with eight repetitions per storage mode.
The Rust toolchain was nightly-2026-02-09.
The test lock excluded other repository tests during these measurements.
The host was not dedicated to this benchmark.

The table reports the median of the five per-run p50 read latencies, in microseconds.
Byte counts include key and value wrappers.
The repeated fixtures contain zero bytes; random fixtures use a fixed seed.

| Continuation payload | Raw row bytes | Selected row bytes | Legacy read us | Validated raw read us | Compression-enabled read us |
| --- | ---: | ---: | ---: | ---: | ---: |
| 64 KiB repeated | 65,596 | 385 | 2.7 | 69.5 | 169.5 |
| 64 KiB random | 65,596 | 65,596 | 2.6 | 63.4 | 69.5 |
| 1 MiB repeated | 1,048,636 | 4,240 | 339.2 | 1,054.1 | 2,474.2 |
| 1 MiB random | 1,048,636 | 1,048,636 | 340.0 | 1,051.9 | 1,044.0 |

For the 1 MiB repeated fixture, median write times were 0.49 us for legacy storage, 996.5 us for validated raw storage, and 2,312.1 us with compression enabled.
The timer excludes the caller's input clone.
The legacy memory-store write mostly moves an owned buffer; it does not represent an LMDB commit.

Each complete run took 315 to 349 ms inside the benchmark.
GNU `time -v` reported 0.27 to 0.30 seconds of user CPU time and 0.03 to 0.05 seconds of system CPU time.
Peak resident memory was 11,020 to 11,136 KiB.
These process totals include fixture generation and all three modes, not only codec scratch.

The result confirms raw fallback for incompressible input and savings for highly repetitive input.
It also exposes substantial integrity-check overhead against unwrapped memory storage.
Disabling compressed writes does not remove that overhead because the compatibility reader still validates legacy values.
Do not treat the default-off setting as proof of unchanged runtime performance.
Representative LMDB and shard latency measurements remain necessary before deployment.

## Rollback and Remaining Gates

There is no automatic migration, startup rewrite, or in-place rollback command.
The existing store interface has no atomic compare-and-replace operation for online migration.
Keep that phase deferred rather than emulate it with an unsafe read and unconditional write.

For an old-binary rollback, use a stopped shard and a consistent full-state backup.
Expand every compressed row with a reviewed offline converter before the old binary opens that state.
Verify canonical bytes, leaf hashes, retained roots, and the absence of compressed rows.
Disabling the write setting alone is not sufficient.
Do not restore only an older cold database beside newer history and roots databases.

The following release gates remain separate from this implementation:

- Resolution of the remaining formal proof obligations and named maintainer acceptance for the new codec claims.
- Representative canonical-corpus compression and CPU measurements.
- Fresh and migrated LMDB page usage, file allocation, and fragmentation measurements.
- Mixed-version shards and older retained-root recovery beyond the tested same-binary, mixed-format shard.
- Real bridge contracts and longer counter/lock workloads beyond the tested synthetic records.
- Process-kill recovery, an offline rollback converter, and a verified rollback rehearsal.
- A latency and memory budget that accounts for integrity checks even with compressed writes disabled.

No default-on change is justified by the synthetic benchmark alone.
Existing direct-LMDB analysis tools need format awareness before inspecting compressed physical values.
