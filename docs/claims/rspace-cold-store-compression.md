# Claim: Cold-Store Compression Preserves Canonical State

```yaml
claim_id: CLAIM-RSPACE-COLD-COMPRESSION-001
artifacts:
  - rspace++/src/rspace/history/cold_store_codec.rs
  - rspace++/src/rspace/history/cold_store_codec/tests.rs
  - rspace++/src/rspace/history/history_repository.rs
  - rspace++/src/rspace/rspace.rs
  - rspace++/tests/cold_store_compression.rs
status: pending
task: TASK-032-1
mechanization: partial
evidence: docs/cbc-evidence/cold-store-compression-2026-11-26.md
```

## Scope

The user requested implementation of the [compression proposal](../improvement-proposals/storage-only-compression.md) on 2026-11-26.
The [implementation record](../improvement-proposals/storage-only-compression-implementation.md) specifies the physical format and experimental limits.
This claim covers the adapter and its integration, not the existing consensus algorithm or the complete LMDB implementation.

## Properties

1. A successful logical read returns the exact canonical value encoded by a valid physical record.
2. Physical compression does not change leaf hashes, keys, exported canonical values, or resulting checkpoint roots.
3. Corrupt recognized records return errors rather than absence, empty state, or lower-priority fallback.
4. Active compression and decompression reservations respect the shared operation and accounted scratch limits.
5. A successful conditional deletion removes only a physical row whose canonical value matches the expected value.
6. A concurrent representation replacement cannot cause an unconditional deletion of a different value.
7. Readers, checkpoint writers, importers, and exporters share the codec boundary within a history repository.
8. Valid legacy values above the compression limit remain raw without a new state-validity limit.

## Assumptions and Limits

The pinned bincode layout, LZ4 implementation, Blake2b-256 integrity properties, and atomic backend operations are trusted dependencies.
The resource claim covers codec reservations, not all allocations by callers, batch collections, or the backend.
Raw canonical validation does not acquire a codec permit. Its hashing work is outside this concurrency bound.
Successful decoding verifies byte identity through lengths, the canonical digest, and the leaf hash.
This does not provide authenticity against an attacker who can rewrite both values and their trusted references.

The adapter does not make batch conditional insertion atomic.
Its conditional-delete retry limit can produce a contention error under persistent replacement.
The adapter does not alter root retention or guarantee that external direct-store tools understand the new physical format.

## Evidence and Remaining Obligations

The [codec tests](../../rspace++/src/rspace/history/cold_store_codec/tests.rs) exercise the production adapter, corruption cases, budget transitions, and deterministic replacement races.
The [integration tests](../../rspace++/tests/cold_store_compression.rs) compare checkpoint roots, exported values, restart behavior, and replay outputs.
Existing RSpace library, import/export, and replay suites provide regression coverage.

The [TLA+ model](../../formal/tlaplus/cold_store_compression/README.md) checks corruption refusal, canonical export, conditional deletion, and shared budget accounting.
Its five deliberate defects must produce the expected counterexamples.
The [Coq proof](../../formal/rocq/cold_store_compression/README.md) establishes 16 abstract representation, deletion, and budget theorems without admitted results or axioms.
Two operation-trace tests compare the production adapter with independent logical-store and reservation models.

| Property | Refutation and binding | Construction status |
| --- | --- | --- |
| 1. Exact logical reads | Codec round trips, malformed records, logical-store traces | Abstract round-trip and accepted-read proofs; Rust byte-parser refinement remains open |
| 2. Canonical identity | Root, replay, export, and import comparisons | Abstract logical-state and deterministic-observer proofs; RSpace and hash implementations are trusted |
| 3. Corruption refusal | Integrity negative control and production error tests | Abstract invalid-record refusal; precise Rust error propagation is tested, not proved |
| 4. Bounded codec work | Three-worker budget model, release control, reservation traces | Arbitrary finite abstract budget traces preserve bounds; exact Rust ownership and allocator accounting remain assumptions |
| 5. Matching conditional deletion | Atomic-delete model and operation traces | Deletion requires the expected canonical value under atomic physical comparison |
| 6. Replacement preservation | Deterministic races and unsafe-delete counterexample | Changed physical rows and different logical values are preserved |
| 7. Shared codec boundary | Checkpoint, transfer, replay, and restart integration tests | Factory and caller integration are not mechanically proved |
| 8. Oversized legacy compatibility | Production oversized-raw regression | No construction proof of the Rust size checks |

These checks are not a complete Rust refinement proof.
The new format and concurrency properties still require named maintainer review and resolution of applicable proof gaps.
No new waiver or completed verification status is claimed.
Existing guard-parity evidence does not automatically cover this new codec.
