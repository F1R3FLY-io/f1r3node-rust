// Phase 7b-1 (2026-08-27): snapshot chunk-fetch Merkle chunker.
//
// Joining validators need to obtain the actual WAL bytes referenced
// by a snapshot's on-chain-committed root.  Rather than pulling the
// entire snapshot atomically (unbounded latency + memory pressure),
// Phase 7b-1 splits each snapshot blob into fixed-size 4 MiB chunks
// and derives a Merkle root over per-chunk Blake2b256 hashes.
//
// The design memo (implementation-plan.md:374-377, Option C) picks
// this layered approach because:
//   * PB-M-15's `WalSnapshotWrite` finalization effect already
//     commits the snapshot root on-chain via
//     `record_finalization_effect(..., WalSnapshotWrite)`; making
//     THAT root the Merkle root over chunk hashes gives us a
//     verified-once anchor.
//   * Per-chunk verification lets a joiner accept partial progress
//     (each 4 MiB chunk verifies against the anchored root
//     independently) and reject a byzantine chunk without
//     discarding the whole download.
//   * The chunker is fully deterministic + symlink-free + Blake2b-
//     hashed, matching the WAL encoding's own hash discipline.
//
// This file provides the pure-utility chunker + Merkle-root
// derivation + verify primitives.  The network-layer
// `SnapshotChunkRetriever` (paralleling `BlockRetriever` at
// `casper/src/rust/engine/block_retriever.rs`) is a follow-up slice
// that consumes these primitives.
//
// # Chunk size
//
// `CHUNK_SIZE = 4 * 1024 * 1024` (4 MiB).  Chosen to match the
// existing block-fetch max-message shape (Casper wire messages cap
// around 32 MiB for large blocks; 4 MiB gives 8 chunks per max
// message under typical MTU/serialization overhead) AND to keep
// per-request round-trip latency bounded at ~4 MiB / peer_bw.
//
// Changing `CHUNK_SIZE` is a **hard fork of the snapshot-chunk
// protocol** — the Merkle root over chunk hashes depends on the
// chunk boundary, so a network-wide change would produce different
// on-chain roots for the same underlying snapshot bytes.  Pinned
// by `chunk_size_pinned_at_4_mib`.
//
// # Merkle tree shape
//
// Binary Merkle tree over per-chunk Blake2b256 hashes, with
// domain-separated leaf and interior nodes.  For N chunks:
//   * Leaves       = `Blake2b256(LEAF_DOMAIN || chunk_bytes)`
//   * Interior     = `Blake2b256(INTERIOR_DOMAIN || left || right)`
//   * Odd-count siblings: the trailing lone hash is promoted to the
//     next level unchanged (no duplicate-and-hash).  This matches
//     the shape used by Bitcoin/Ethereum block Merkle trees and
//     avoids the second-preimage attack on duplicated leaves.
//   * Root = the single hash at the top after log2(N) reductions.
//   * Empty snapshot (N=0): root = [0u8; 32] sentinel.  Won't
//     collide with any real Blake2b256 output (preimage
//     resistance) *and* differs by construction from any
//     domain-tagged hash (a domain-tagged hash's input starts with
//     `LEAF_DOMAIN` / `INTERIOR_DOMAIN`, not the empty byte).
//
// The `LEAF_DOMAIN` / `INTERIOR_DOMAIN` byte prefixes are the
// standard second-preimage-resistance construction (NEAR,
// Certificate Transparency, RFC 6962) — a leaf hash and a promoted
// interior hash come from disjoint input spaces, so no two
// structurally-different snapshots can share a root through the
// promote-unchanged step even in the (infeasible) preimage-
// collision case.  Without the prefix, an attacker who could
// produce a chunk whose bytes hash to the same 32-byte value as
// an interior node would forge a shorter snapshot with the same
// root; the prefix makes those input spaces disjoint by
// construction.
//
// Changing any of (chunk order, interior hash formula, domain
// bytes, odd-tail treatment, empty sentinel) is a hard fork —
// pinned by `merkle_tree_shape_pinned_by_golden_hex`.

use crypto::rust::hash::blake2b256::Blake2b256;

/// Chunk size for snapshot fetch.  4 MiB per chunk.  See file-level
/// docstring for the rationale.  Hard-fork surface.
pub const CHUNK_SIZE: usize = 4 * 1024 * 1024;

/// Sentinel root for an empty snapshot (0-chunk case).  A
/// well-formed non-empty snapshot cannot produce this root because
/// Blake2b256 is preimage-resistant AND every real leaf/interior
/// hash is domain-tagged (see `LEAF_DOMAIN` / `INTERIOR_DOMAIN`
/// below).
pub const EMPTY_SNAPSHOT_ROOT: [u8; 32] = [0u8; 32];

/// Domain-separation byte prepended to chunk bytes before hashing
/// to produce a leaf hash.  Distinguishes leaf inputs from interior
/// inputs so no two structurally-different snapshots can share a
/// root through the promote-unchanged step — see the "Merkle tree
/// shape" section of the file-level docstring.
///
/// Hard-fork surface: changing this byte flips every snapshot root
/// on-network.
const LEAF_DOMAIN: u8 = 0x00;

/// Domain-separation byte prepended to the left || right
/// concatenation before hashing to produce an interior node hash.
/// See `LEAF_DOMAIN`.  Hard-fork surface.
const INTERIOR_DOMAIN: u8 = 0x01;

/// A single 4 MiB chunk of a snapshot, plus its Blake2b256 hash.
/// Owned bytes so callers can freely move chunks across threads /
/// serialize to the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotChunk {
    /// Which chunk this is (0..chunk_count).  Wire messages carry
    /// (snapshot_root, chunk_index) as the request key.
    pub index: u32,
    /// Chunk bytes.  Length is `CHUNK_SIZE` for all but possibly
    /// the final chunk (which may be shorter).
    pub bytes: Vec<u8>,
    /// `Blake2b256(LEAF_DOMAIN || bytes)` — the domain-tagged leaf
    /// hash.  Precomputed at chunk time so wire verification skips
    /// a rehash on the send path.
    pub hash: [u8; 32],
}

/// Split a snapshot blob into fixed-size chunks + per-chunk
/// Blake2b256 hashes.  Deterministic across validators for
/// identical input.  An empty input produces a zero-chunk output.
pub fn chunk_snapshot(bytes: &[u8]) -> Vec<SnapshotChunk> {
    let n_chunks = bytes.len().div_ceil(CHUNK_SIZE);
    let mut out = Vec::with_capacity(n_chunks);
    for i in 0..n_chunks {
        let start = i * CHUNK_SIZE;
        let end = (start + CHUNK_SIZE).min(bytes.len());
        let chunk_bytes = bytes[start..end].to_vec();
        let hash = hash_leaf(&chunk_bytes);
        out.push(SnapshotChunk {
            index: u32::try_from(i).expect("snapshot chunk count exceeds u32::MAX"),
            bytes: chunk_bytes,
            hash,
        });
    }
    out
}

/// Compute the Merkle root over the per-chunk hashes.  Deterministic
/// across validators for identical input.  The empty case returns
/// `EMPTY_SNAPSHOT_ROOT`.
pub fn snapshot_merkle_root(chunk_hashes: &[[u8; 32]]) -> [u8; 32] {
    if chunk_hashes.is_empty() {
        return EMPTY_SNAPSHOT_ROOT;
    }
    let mut level: Vec<[u8; 32]> = chunk_hashes.to_vec();
    while level.len() > 1 {
        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        let mut i = 0;
        while i + 1 < level.len() {
            next.push(hash_interior(&level[i], &level[i + 1]));
            i += 2;
        }
        // Odd tail: promote unchanged (no duplicate-and-hash).
        if i < level.len() {
            next.push(level[i]);
        }
        level = next;
    }
    level[0]
}

/// Verify a chunk against its claimed hash.  Returns `true` if
/// `chunk.hash == Blake2b256(LEAF_DOMAIN || chunk.bytes)`.  Callers
/// verify this on every received chunk BEFORE adding it to the
/// reconstructed snapshot.
///
/// The comparison is not constant-time — Blake2b256 is O(n) in
/// input length and `==` on `[u8; 32]` short-circuits.  Not a
/// meaningful gap: the expected hash is on-chain, so an attacker
/// learns nothing from timing that they don't already know.
pub fn verify_chunk_hash(chunk: &SnapshotChunk) -> bool { hash_leaf(&chunk.bytes) == chunk.hash }

/// Merkle inclusion proof for a single chunk.  Enough to verify
/// that a chunk with hash `chunk_hash` at index `index` participates
/// in the Merkle tree rooted at `root`.
///
/// `siblings` is bottom-up (leaf-level first, root's-child last).
/// Each entry is `(sibling_hash, is_right)` where `is_right`
/// indicates whether the sibling is the RIGHT child (in which case
/// we concatenate `current || sibling`) or LEFT (`sibling || current`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MerkleProof {
    pub index: u32,
    pub siblings: Vec<([u8; 32], bool)>,
}

/// Build the inclusion proof for `index` in a snapshot with the
/// given `chunk_hashes` sequence.  Returns None if `index` is out
/// of range.
pub fn build_merkle_proof(chunk_hashes: &[[u8; 32]], index: u32) -> Option<MerkleProof> {
    let index_usize = usize::try_from(index).ok()?;
    if chunk_hashes.is_empty() || index_usize >= chunk_hashes.len() {
        return None;
    }
    let mut siblings = Vec::new();
    let mut level: Vec<[u8; 32]> = chunk_hashes.to_vec();
    let mut idx = index_usize;
    while level.len() > 1 {
        // Sibling of idx at this level.
        let is_idx_left = idx % 2 == 0;
        let sib_idx = if is_idx_left { idx + 1 } else { idx - 1 };
        if sib_idx < level.len() {
            // sibling is on the RIGHT if the target is left-of-pair.
            siblings.push((level[sib_idx], is_idx_left));
        }
        // Fold up the next level.
        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        let mut i = 0;
        while i + 1 < level.len() {
            next.push(hash_interior(&level[i], &level[i + 1]));
            i += 2;
        }
        if i < level.len() {
            next.push(level[i]);
        }
        idx /= 2;
        level = next;
    }
    Some(MerkleProof { index, siblings })
}

/// Verify that `chunk_hash` at `proof.index` participates in the
/// Merkle tree rooted at `root`.  Constant work per proof step.
pub fn verify_merkle_proof(root: &[u8; 32], chunk_hash: &[u8; 32], proof: &MerkleProof) -> bool {
    let mut current = *chunk_hash;
    for (sibling, sibling_is_right) in &proof.siblings {
        current = if *sibling_is_right {
            hash_interior(&current, sibling)
        } else {
            hash_interior(sibling, &current)
        };
    }
    current == *root
}

/// Domain-tagged leaf hash: `Blake2b256(LEAF_DOMAIN || bytes)`.
///
/// TODO(follow-up): the underlying `Blake2b256::hash(Vec<u8>)` API
/// forces a `.to_vec()` copy on every call (see the internal
/// helper below).  Fixing the crypto crate's API to accept
/// `&[u8]` would eliminate a full 4 MiB copy per chunk hash on the
/// send path.  Tracked separately since it crosses the crate
/// boundary.
fn hash_leaf(bytes: &[u8]) -> [u8; 32] {
    let mut input = Vec::with_capacity(1 + bytes.len());
    input.push(LEAF_DOMAIN);
    input.extend_from_slice(bytes);
    hash_owned(input)
}

/// Domain-tagged interior hash:
/// `Blake2b256(INTERIOR_DOMAIN || left || right)`.
fn hash_interior(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut input = [0u8; 1 + 32 + 32];
    input[0] = INTERIOR_DOMAIN;
    input[1..33].copy_from_slice(left);
    input[33..].copy_from_slice(right);
    hash_owned(input.to_vec())
}

fn hash_owned(input: Vec<u8>) -> [u8; 32] {
    let h = Blake2b256::hash(input);
    assert_eq!(h.len(), 32, "Blake2b256 must produce 32-byte digest");
    let mut out = [0u8; 32];
    out.copy_from_slice(&h);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hard-fork surface pin: chunk size stays at 4 MiB.  A future
    /// change would produce different Merkle roots on-network — pin
    /// here so any accidental edit trips CI.
    #[test]
    fn chunk_size_pinned_at_4_mib() {
        assert_eq!(CHUNK_SIZE, 4 * 1024 * 1024);
    }

    #[test]
    fn empty_snapshot_chunks_to_empty_vec() {
        let chunks = chunk_snapshot(&[]);
        assert!(chunks.is_empty());
        assert_eq!(snapshot_merkle_root(&[]), EMPTY_SNAPSHOT_ROOT);
    }

    #[test]
    fn sub_chunk_snapshot_produces_single_chunk() {
        let bytes = vec![0xAAu8; 1024]; // well under 4 MiB
        let chunks = chunk_snapshot(&bytes);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].index, 0);
        assert_eq!(chunks[0].bytes, bytes);
        assert!(verify_chunk_hash(&chunks[0]));
        // Single-chunk Merkle root == the chunk's hash.
        let root = snapshot_merkle_root(&[chunks[0].hash]);
        assert_eq!(root, chunks[0].hash);
    }

    #[test]
    fn chunk_boundary_at_exactly_chunk_size() {
        let bytes = vec![0xBBu8; CHUNK_SIZE];
        let chunks = chunk_snapshot(&bytes);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].bytes.len(), CHUNK_SIZE);
    }

    #[test]
    fn chunk_boundary_at_chunk_size_plus_one_produces_two_chunks() {
        let mut bytes = vec![0xCCu8; CHUNK_SIZE];
        bytes.push(0xDD);
        let chunks = chunk_snapshot(&bytes);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].bytes.len(), CHUNK_SIZE);
        assert_eq!(chunks[1].bytes.len(), 1);
        assert_eq!(chunks[1].bytes[0], 0xDD);
        // Both chunks self-verify.
        assert!(verify_chunk_hash(&chunks[0]));
        assert!(verify_chunk_hash(&chunks[1]));
    }

    #[test]
    fn tamper_detection_via_verify_chunk_hash() {
        let bytes = vec![0xEEu8; 128];
        let mut chunk = chunk_snapshot(&bytes).into_iter().next().unwrap();
        assert!(verify_chunk_hash(&chunk));
        // Flip one byte in the payload; hash comparison should now fail.
        chunk.bytes[0] ^= 0x01;
        assert!(!verify_chunk_hash(&chunk));
    }

    #[test]
    fn merkle_root_deterministic_for_identical_input() {
        let bytes = vec![0x77u8; CHUNK_SIZE * 3 + 500];
        let chunks_a = chunk_snapshot(&bytes);
        let chunks_b = chunk_snapshot(&bytes);
        let hashes_a: Vec<[u8; 32]> = chunks_a.iter().map(|c| c.hash).collect();
        let hashes_b: Vec<[u8; 32]> = chunks_b.iter().map(|c| c.hash).collect();
        assert_eq!(hashes_a, hashes_b);
        assert_eq!(
            snapshot_merkle_root(&hashes_a),
            snapshot_merkle_root(&hashes_b)
        );
    }

    #[test]
    fn merkle_root_differs_on_content_change() {
        let mut bytes = vec![0x00u8; CHUNK_SIZE + 100];
        let chunks = chunk_snapshot(&bytes);
        let hashes: Vec<[u8; 32]> = chunks.iter().map(|c| c.hash).collect();
        let root_a = snapshot_merkle_root(&hashes);
        // Flip a byte in chunk 1 (second chunk).
        bytes[CHUNK_SIZE + 50] ^= 0x01;
        let chunks_b = chunk_snapshot(&bytes);
        let hashes_b: Vec<[u8; 32]> = chunks_b.iter().map(|c| c.hash).collect();
        let root_b = snapshot_merkle_root(&hashes_b);
        assert_ne!(root_a, root_b);
    }

    #[test]
    fn merkle_proof_verifies_for_every_chunk_index() {
        // Odd chunk count (5 chunks: 4 full + 700-byte tail) so
        // the "promote tail" case fires at level 0 — the case a
        // proof for an odd-position leaf must handle.
        let bytes = vec![0x33u8; CHUNK_SIZE * 4 + 700];
        let chunks = chunk_snapshot(&bytes);
        assert_eq!(chunks.len(), 5, "test setup: expected odd chunk count");
        let hashes: Vec<[u8; 32]> = chunks.iter().map(|c| c.hash).collect();
        let root = snapshot_merkle_root(&hashes);
        for chunk in &chunks {
            let proof = build_merkle_proof(&hashes, chunk.index).expect("proof for valid index");
            assert!(
                verify_merkle_proof(&root, &chunk.hash, &proof),
                "proof for chunk {} did not verify",
                chunk.index,
            );
        }
    }

    #[test]
    fn merkle_proof_rejects_wrong_chunk() {
        // Distinct-per-chunk payload so no two chunks share a hash.
        let mut bytes = Vec::with_capacity(CHUNK_SIZE * 4);
        for byte in [0xA1u8, 0xB2, 0xC3, 0xD4] {
            bytes.extend(std::iter::repeat_n(byte, CHUNK_SIZE));
        }
        let chunks = chunk_snapshot(&bytes);
        let hashes: Vec<[u8; 32]> = chunks.iter().map(|c| c.hash).collect();
        let root = snapshot_merkle_root(&hashes);
        let proof = build_merkle_proof(&hashes, 1).unwrap();
        // Verify proof for chunk 1 with chunk 0's hash — must reject.
        let wrong_hash = chunks[0].hash;
        assert!(
            !verify_merkle_proof(&root, &wrong_hash, &proof),
            "proof for chunk 1 must NOT verify with chunk 0's hash",
        );
    }

    #[test]
    fn merkle_proof_rejects_bogus_root() {
        let bytes = vec![0x55u8; CHUNK_SIZE * 3];
        let chunks = chunk_snapshot(&bytes);
        let hashes: Vec<[u8; 32]> = chunks.iter().map(|c| c.hash).collect();
        let proof = build_merkle_proof(&hashes, 0).unwrap();
        let bogus_root = [0xFFu8; 32];
        assert!(
            !verify_merkle_proof(&bogus_root, &chunks[0].hash, &proof),
            "proof must NOT verify against a bogus root",
        );
    }

    #[test]
    fn build_merkle_proof_out_of_range_returns_none() {
        let bytes = vec![0x66u8; CHUNK_SIZE * 2];
        let chunks = chunk_snapshot(&bytes);
        let hashes: Vec<[u8; 32]> = chunks.iter().map(|c| c.hash).collect();
        assert!(build_merkle_proof(&hashes, 99).is_none());
        // Empty snapshot: proof unbuilt regardless of index.
        assert!(build_merkle_proof(&[], 0).is_none());
    }

    /// Golden-hex pin on the Merkle root for a deterministic
    /// 3-chunk-and-a-bit input.  Guards against a refactor that
    /// changes the tree shape (interior hash formula, odd-tail
    /// treatment, empty sentinel).  A regression would flip this
    /// hash — visible in CI before deployment.
    #[test]
    fn merkle_tree_shape_pinned_by_golden_hex() {
        // Deterministic input: 3 full chunks of a fixed byte + a
        // 100-byte tail chunk.  Chunk 0 = 0xAA * 4MiB;
        // chunk 1 = 0xBB * 4MiB; chunk 2 = 0xCC * 4MiB;
        // chunk 3 = 0xDD * 100.  Predictable.
        let mut bytes = Vec::with_capacity(CHUNK_SIZE * 3 + 100);
        bytes.extend(std::iter::repeat_n(0xAAu8, CHUNK_SIZE));
        bytes.extend(std::iter::repeat_n(0xBBu8, CHUNK_SIZE));
        bytes.extend(std::iter::repeat_n(0xCCu8, CHUNK_SIZE));
        bytes.extend(std::iter::repeat_n(0xDDu8, 100));
        let chunks = chunk_snapshot(&bytes);
        assert_eq!(chunks.len(), 4);
        let hashes: Vec<[u8; 32]> = chunks.iter().map(|c| c.hash).collect();
        let root = snapshot_merkle_root(&hashes);
        let hex = root.iter().fold(String::with_capacity(64), |mut acc, b| {
            use std::fmt::Write;
            let _ = write!(acc, "{b:02x}");
            acc
        });
        // Regenerate via cargo test --lib -- merkle_tree_shape_pinned_by_golden_hex --nocapture
        // ONLY when intentionally hard-forking the chunker.
        const EXPECTED: &str = "2321490882adf57b4d77e30e6a4d67f2832bf0c5e0af17466729273f366fa852";
        assert_eq!(
            hex, EXPECTED,
            "Merkle-root shape changed.  If intentional, bump the hard-fork \
             surface catalog and update EXPECTED; else find and revert the \
             chunker edit."
        );
    }

    /// Cross-structure disambiguation: two snapshots that differ in
    /// chunk count (and therefore tree shape) produce distinct
    /// roots.  Without domain separation between leaves and
    /// interior nodes, a 3-chunk snapshot `[a, b, X]` and a 4-chunk
    /// snapshot `[a, b, c, d]` where `X == hash(c || d)` would
    /// share a root — the promote-unchanged step makes leaf and
    /// interior hashes indistinguishable at the parent level.  The
    /// `LEAF_DOMAIN` / `INTERIOR_DOMAIN` prefixes make the leaf and
    /// interior input spaces disjoint, so this collision is
    /// impossible by construction.  Pin the behavior end-to-end.
    #[test]
    fn different_chunk_counts_produce_distinct_roots() {
        let two_chunks = vec![0x11u8; CHUNK_SIZE * 2];
        let three_chunks = vec![0x11u8; CHUNK_SIZE * 3];
        let root_2 = snapshot_merkle_root(
            &chunk_snapshot(&two_chunks)
                .iter()
                .map(|c| c.hash)
                .collect::<Vec<_>>(),
        );
        let root_3 = snapshot_merkle_root(
            &chunk_snapshot(&three_chunks)
                .iter()
                .map(|c| c.hash)
                .collect::<Vec<_>>(),
        );
        assert_ne!(root_2, root_3);
    }
}
