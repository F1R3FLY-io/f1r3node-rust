# Cold-Store Compression Construction Proof

The [proof source](theories/MainTheorem.v) establishes abstract adapter properties with Coq's kernel.
It supports the pending [compression claim](../../../docs/claims/rspace-cold-store-compression.md).
It is not a proof of the complete Rust implementation.

## Representation Boundary

`Bytes` is a list of natural numbers. It represents an arbitrary finite canonical sequence.
`Compressed value verified` records a canonical sequence and the result of an abstract integrity check.
It does not contain or execute an LZ4 byte stream.
The encoder constructor models the required identity contract of a correct compression library.
The verified flag abstracts the production header, length, digest, and leaf-key checks.
`Raw` represents trusted, structurally valid canonical bytes, not a payload whose hash was checked on every read.
Generic writes validate supplied bytes; checkpoint writes reuse the key computed by their private producer.
That producer's Rust implementation is a trusted boundary, not a theorem in this model.

The proof therefore depends on a correspondence that the Rust tests exercise but do not prove:
successful physical decoding must return exactly the represented canonical sequence.
Coq's `None` means no accepted value. It does not distinguish a missing row from a rejected record.
Production Rust must preserve that distinction through `Result`, as the corruption tests require.

## Theorems

| Theorems | Established property |
| --- | --- |
| `canonical_roundtrip` | Abstract encoding followed by decoding preserves the canonical sequence |
| `corrupt_frame_refused`, `unknown_format_refused`, `successful_compressed_read_verified` | Only accepted representations produce a decoded value |
| `physical_choice_preserves_state`, `write_preserves_other_keys` | Physical representation does not change logical state or unrelated keys |
| `canonical_export_independent`, `canonical_observer_independent` | Canonical exports and any deterministic observer of those exports ignore physical representation |
| `changed_row_preserved`, `different_value_preserved`, `deletion_requires_matching_value`, `stable_matching_row_deleted` | Compare-and-delete preserves replacements and requires the expected logical value |
| `acquire_preserves_bounds`, `release_preserves_bounds` | Abstract reservation transitions preserve configured bounds |
| `acquire_release_exact` | Releasing the bytes from one successful acquisition restores its prior state |
| `arbitrary_trace_preserves_bounds` | Any finite trace of the abstract budget transitions preserves initial bounds |

The construction arguments quantify over arbitrary sequence lengths, keys, capacities, and finite trace lengths.
They are not limited to the small state space of the [TLA+ model](../../tlaplus/cold_store_compression/README.md).
However, the budget theorem does not prove Rust permit ownership, arbitrary concurrent allocator behavior, or the accuracy of the encoder-workspace estimate.
The release transition uses natural-number subtraction. Exact accounting relies on releasing the reservation owned by a permit.
The TLA+ accounting invariant and Rust operation-trace tests check that additional condition in their tested scopes.

## Checking and Trust

The [evidence script](../../../scripts/ci/check-cold-store-compression.sh) runs `coqc`, `coqchk`, and `Print Assumptions` for all 16 theorems.
Each assumption report must say `Closed under the global context`.
The source contains no admitted theorem or declared axiom.
The normal Coq invariant gate also builds this project and audits these theorems.

Trusted dependencies include the Coq kernel, the Rust compiler and standard library, the pinned serializers and codec, the hash function, and atomic backend operations.
Root equality and canonical import/export behavior are tested against real RSpace code; this proof does not implement RSpace or LMDB.
No liveness, power-loss recovery, cryptographic authenticity, production performance, or complete-node memory bound is claimed.
Maintainer acceptance and any required implementation-refinement evidence remain pending.
