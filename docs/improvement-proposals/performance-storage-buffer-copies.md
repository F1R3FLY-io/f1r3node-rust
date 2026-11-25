# Remove Redundant Storage Buffer Copies

Status: draft proposal. No store implementation is changed.
Date: 2026-11-25.

## Objective

Move buffers that a caller already owns instead of cloning their payloads.
Evaluate presence-only reads separately because they can change error behavior.
The [performance acceptance conditions](performance-overview.md#shared-acceptance-conditions) apply.

## Current Path

The [key/value trait](../../shared/src/rust/store/key_value_store.rs) has three relevant default methods:

| Method | Current operation | Avoidable work |
| --- | --- | --- |
| `get_one` | Calls `get`, then clones the first `Option<ByteBuffer>` through `split_first` | A second allocation and copy of a present value |
| `put_if_absent` | Builds a candidate vector, clones it, then filters existing keys | Copies all candidate values, including values discarded by the filter |
| `contains` | Calls `get`, then tests whether each result is present | Reads and decodes values that the caller does not consume |

The [LMDB store](../../shared/src/rust/store/lmdb_key_value_store.rs) uses these defaults.
Its typed database decodes owned buffers through `SerdeBincode`.
Independent readers already use separate read transactions without a global database mutex.

The first two costs follow directly from ownership in the source.
Their node-wide impact depends on call frequency and value sizes.
The [operation probe](performance-overview.md#reproducible-operation-probe) isolates the extra payload copies, not LMDB throughput.

## Phase 1: Consume Owned Results

For `get_one`, consume the returned vector and take its first optional value.
Preserve `None` for an empty result vector.
Do not change the `get` contract or introduce a borrowed value outside its transaction lifetime.

For `put_if_absent`, consume the existing candidate vector when filtering.
Keep the same key order, presence results, selected entries, and call to `put`.
The keys needed for the presence query still need ownership under the current API.
Removing those key copies would be a separate interface change.

Keep batch transaction boundaries, durability settings, and serialization unchanged.
Preserve behavior for empty batches and duplicate keys.
Do not reinterpret this copy-removal patch as a concurrency fix.

## Phase 2: Evaluate Presence Reads

A raw existence check could avoid decoding a large value.
However, current `contains` propagates decode errors from `get`.
Replacing it with an unchecked raw lookup could turn corruption into a successful presence result.

Compare two explicit designs:

- Keep generic `contains` unchanged and add a narrowly owned key-presence operation for callers that accept its semantics.
- Use a validated encoding-aware presence path only if it preserves the existing decode-error contract.

Choose through caller review and benchmarks, not a silent trait override.
Avoid byte-offset assumptions about bincode encoding.
Measure whether validation still leaves a useful saving.

Corrupt present values must not become absent values or valid recovered state.
A content-addressed cold-store caller cannot assume that an existing key proves its payload remains readable.

## Excluded Write Changes

Current batch `put_if_absent` reads presence before calling `put`.
Those operations are not one atomic conditional-write transaction.
The store also has a separate atomic single-entry operation using `NO_OVERWRITE`.

Converting the batch helper to atomic writes needs a separate behavior decision.
Concurrent writers and repeated keys within one batch can produce different results.
Document duplicate-key precedence before considering that change.
Do not bundle it with buffer movement.

## Regression Plan

| Case | Required assertion |
| --- | --- |
| Missing, empty, and nonempty values | Exact distinction between absence and an empty byte buffer |
| One result and empty result vectors | Same `get_one` value and errors as the reference helper |
| All-present, all-absent, and mixed batches | Identical selected entries, order, and written bytes |
| Repeated keys within a batch | Unchanged current duplicate-key behavior |
| Store read, decode, and write errors | Same error propagation without partial helper-level reinterpretation |
| Large values | No extra payload clone in the moved-result path |
| Actual LMDB and an in-memory test store | Equivalent helper behavior across implementations |
| Checkpoint and state-transfer callers | Identical canonical values and resulting state roots |

Instrument a test payload or allocator to distinguish decoding from the removed copy.
The standalone probe does not replace tests against these methods.
Keep deliberate malformed-value fixtures local to disposable databases.

## Benchmark Plan

Use point reads and conditional batches with small values, 2 KiB values, and 1 MiB values.
Add a fixed continuation-leaf corpus to represent a real caller.
Separate warm-page and cold-page trials.
Vary batch size and the fraction of keys already present.

Record copied bytes, allocation count, allocated bytes, CPU time, and latency percentiles.
Record read/write transaction count and peak live memory.
For phase 2, record decoded bytes and corruption-test outcomes as separate results.

The expected phase-1 result is removal of the identified copies, not zero-copy LMDB access.
Unchanged I/O can still dominate large cold reads.
Reject an optimization that lengthens transaction lifetime to retain borrowed storage buffers.

## Delivery Decision

Implement phase 1 first as a small store-helper patch.
Accept it only with equivalent values, errors, and write behavior.
Defer phase 2 if preserving corruption semantics removes its benefit.
No migration is required, and rollback uses the previous binary.
