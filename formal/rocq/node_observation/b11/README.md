# B11 canonical snapshot encoding

This project proves roundtrip decoding and injectivity for the complete snapshot wire schema. The model emits bytes, including tags and length prefixes.

The Rust binding calls the production `DetachedDagSnapshot::seal` function. It exports the resulting snapshot fields and bytes as separate Rocq definitions.

Rocq checks their equality with an independent encoder. Each case also proves validity and roundtrip decoding.

## Schema correspondence

`Schema.v` follows `SnapshotData::encode`, `CanonicalEncoder::metadata`, and the final work field in `DetachedDagSnapshot::seal`.

| Field group | Wire representation |
| --- | --- |
| Schema | Length-prefixed `schema` tag, then four-byte unsigned version |
| Scope | Eight-byte length, then UTF-8 bytes |
| Limits | `limits` tag, twelve eight-byte integers, then four-byte nanoseconds |
| Read usage | `read usage` tag, then three eight-byte integers |
| Coverage | `coverage` tag, eight-byte count, one-byte Boolean, eight-byte count |
| Generation | `generation` tag, then eight-byte integer |
| Transactions | `transactions` tag, count, then environment bytes, reference count, transaction identifier, and optional validation identifier |
| DAG set | `dag_set` tag, count, then length-prefixed hashes |
| Child map | `child_map` tag, count, then hash and counted child hashes |
| Height map | `height_map` tag, count, then signed height bits and counted hashes |
| Block number map | `block_number_map` tag, count, then hash and signed block number bits |
| Parent maps | Separate `main_parent_map` and `self_justification_map` tags, each followed by counted hash pairs |
| Last finalized block | `last_finalized_block` tag, then optional hash and signed height bits |
| Finalized set | `finalized_block_set` tag, count, then hashes |
| Latest messages | `latest_messages` tag, count, then validator and hash pairs |
| Invalid blocks | `invalid_blocks` tag, count, then key and complete metadata |
| Detached blocks | `blocks` tag, count, then key, complete metadata, optional floor, and optional frontier |
| Bodies | `bodies` tag, count, then key and optional raw body bytes |
| Work | `work` tag, then the final eight-byte work count |

All integer bytes use big-endian order. Signed integers use their two's-complement bits. Counts and byte lengths use eight bytes.

Metadata contains the block hash, ordered parents, sender, ordered justifications, weights, block number, sequence number, three flags, float bits, and merge base.

Block numbers and weights use eight bytes. Sequence numbers and float bits use four bytes. Float encoding preserves the exact `f32::to_bits` result.

Optional fields use zero for absence and one before a present value. Present empty bytes differ from absence.

Collections follow their Rust `BTreeMap` or `BTreeSet` iteration order. Parent, justification, and transaction vectors retain their sequence order.

The limits model contains eleven configured integer limits, duration seconds, and duration nanoseconds. The usage model contains bytes, records, and operations.

## Construction guarantees

`MainTheorem.v` exports eight theorems:

- `canonical_integer_roundtrip`
- `canonical_metadata_roundtrip`
- `canonical_snapshot_roundtrip`
- `canonical_metadata_injective`
- `canonical_snapshot_injective`
- `canonical_ordered_parents_preserved`
- `canonical_availability_distinct`
- `canonical_collection_permutation`

The roundtrip theorems preserve an arbitrary trailing suffix. Injectivity follows from decoding, without a hash assumption.

Validity requires each scalar to fit its width, each raw byte to be below 256, and each count to fit eight bytes.

The collection theorem requires sorted inputs, an asymmetric order, decidable equality, and equal elements up to permutation. Rust collection ordering remains a library assumption. Integer conversion assumes a supported Rust target with at most 64-bit `usize` values.

The model treats floating-point values as bit patterns. Its equality distinguishes NaN payloads and signed zero.

The wire proof covers successful snapshots. It does not establish capture limits, resource accounting, store consistency, or error handling.

SHA-256 remains outside the injectivity theorem. The Rust tests check the digest input. No collision-freedom theorem is claimed.

## Rust correspondence checks

The fixture suite includes 75 cases. These cases cover all snapshot fields and every metadata field in both metadata locations.

The cases include signed extremes, a NaN payload, signed zero, UTF-8, embedded zero bytes, empty values, availability states, and ordered vectors.

Each changed field must change production bytes. A separate test changes collection insertion order and requires identical bytes.

Six Rocq controls prove rejection of wrong endianness, a changed tag, an omitted field, truncated work, a trailing byte, and a changed coverage flag.

The construction proofs apply to arbitrary valid model values. The executable cases provide finite Rust correspondence evidence, not a universal machine-checked refinement of Rust.

The source manifest binds the cases to the workspace Rust files, Cargo manifests, lockfile, toolchain, and Cargo configuration.

## Reproduction

Use Rust with the workspace system dependencies and Rocq 9.1.1. The Rust phase requires cached Cargo dependencies because it uses `--offline`.

1. Export production cases:

   ```sh
   bash scripts/ci/check-node-canonical-wire.sh export target/b11-run
   ```

2. Verify the proofs and correspondence:

   ```sh
   opam exec -- bash scripts/ci/check-node-canonical-wire.sh verify target/b11-run
   ```

Use a new output directory for each export. Verification retains source digests, case digests, compiler versions, proof logs, and assumption output.

The phases can run on separate hosts. Copy the complete output directory between hosts, and use identical source files.

For an archived checkout without Git metadata, set `B11_SOURCE_LIST` to a file list from the originating checkout. Verification checks each listed file digest.

The combined formal gate builds this namespace and checks all eight exports. The combined binding driver exports the production cases.

The `rocq-build` CI job requires the binding job and verifies its source-bound cases before completing the formal receipt.

The evidence package refresh remains separate. Historical isolated verification does not establish verification of the integrated source revision.

Both node claims retain pending status until their remaining evidence and maintainer review requirements are complete.
