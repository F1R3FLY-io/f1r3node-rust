// Phase 7b-1 snapshot chunk serving handler.
//
// Server-side counterpart of the (yet-to-land client-side)
// `SnapshotChunkRetriever`.  Reads a snapshot from the local
// disk cache, chunks it, and builds a `SnapshotChunkResponse`
// for the requested `(block_hash, chunk_index)` pair —
// including an inclusion proof against the anchored Merkle
// root.
//
// Layered against the retriever: this module produces
// responses; the retriever (slice 3.6, yet-to-land) verifies
// them.  Both operate on the same primitives from
// [`rholang::interpreter::io::snapshot_chunk`].
//
// Callers: the wire-message handler (yet-to-land in slice 3.5
// wire glue) routes an incoming `GetSnapshotChunkRequest` here
// after verifying the requesting peer + rate limits.  A
// successful [`serve_chunk`] returns the response proto to
// send back; error paths let the caller decide whether to
// reply with a NoSnapshotAvailable-style signal or stay
// silent.

use std::path::Path;

use models::rust::casper::protocol::casper_message::{
    HasSnapshot, MerkleProofStep, SnapshotChunkResponse,
};
use prost::bytes::Bytes;
use rholang::rust::interpreter::io::snapshot::read_snapshot_bytes;
use rholang::rust::interpreter::io::snapshot_chunk::{
    build_merkle_proof, chunk_snapshot, snapshot_merkle_root,
};

/// Errors from [`serve_chunk`].  Distinguishes "can't help"
/// (peer should ask someone else) from "hard failure" (config
/// bug worth logging at warn).
#[derive(Debug, PartialEq, Eq)]
pub enum ServeError {
    /// This node has no cached anchor for the requested block —
    /// most likely the peer's request references a block we
    /// haven't finalized yet.  Cheap "no thanks".
    UnknownBlockHash,
    /// `chunk_index` is out of range for this snapshot's chunk
    /// count.  The peer is confused about which snapshot they're
    /// fetching.
    IndexOutOfRange { requested: u32, chunk_count: u32 },
    /// Disk read of the snapshot file failed.  Log at warn —
    /// the anchor exists but the payload doesn't.  Operator
    /// should investigate (rare; typically means a storage
    /// failure or a hand-deleted snapshot).
    DiskReadFailed(String),
    /// The snapshot file's bytes hash to something DIFFERENT
    /// from the anchored atomic root OR the recomputed Merkle
    /// root disagrees with the anchor.  Byzantine local storage
    /// or a version-skewed on-disk file.  Log at warn.
    AtomicRootMismatch,
}

/// Serve a single chunk from the local snapshot cache.
///
/// `anchor` is the `(atomic_root, merkle_root)` pair stored in
/// `RuntimeManager::snapshot_merkle_roots` (yet-to-land) at the
/// finalized block hash the request references.  `snapshot_dir`
/// is the writer's on-disk directory (matches
/// `SnapshotWriter::dir`).
///
/// # Defense in depth
///
/// The server performs TWO integrity checks beyond trusting
/// the on-disk bytes:
///
///   1. `read_snapshot_bytes` rechecks
///      `Blake2b256(bytes) == atomic_root` and returns
///      [`ServeError::AtomicRootMismatch`] on tampering.
///   2. The Merkle root is recomputed from fresh chunking and
///      compared against the anchored `merkle_root` — a
///      mismatch means the anchor was written for a different
///      snapshot (or the file was tampered with between the
///      atomic-root check and chunking).  Refuse rather than
///      send a chunk that won't verify at the requester.
///
/// Both checks are consensus-aligned: the authority is the
/// locally-anchored `(atomic_root, merkle_root)` from block
/// finalization.  A server returning mismatched bytes with a
/// correct-looking echoed hash would get caught by the
/// requester's rehash + proof check (see
/// `SnapshotChunkResponseProto` docstring on the ADVISORY
/// echo).  The server-side checks here close the gap earlier.
pub fn serve_chunk(
    block_hash: &[u8],
    chunk_index: u32,
    anchor: ([u8; 32], [u8; 32]),
    snapshot_dir: &Path,
) -> Result<SnapshotChunkResponse, ServeError> {
    let (atomic_root, merkle_root) = anchor;
    // Read the on-disk snapshot bytes.  `read_snapshot_bytes`
    // rechecks Blake2b256(bytes) == atomic_root and returns
    // RootMismatch on tampering.
    let bytes = read_snapshot_bytes(snapshot_dir, &atomic_root).map_err(|e| match e {
        rholang::rust::interpreter::io::snapshot::SnapshotError::RootMismatch { .. } => {
            ServeError::AtomicRootMismatch
        }
        other => ServeError::DiskReadFailed(format!("{other:?}")),
    })?;
    // Chunk locally.  CPU work but bounded at 4 MiB × N chunks;
    // typical snapshots run 1-8 chunks so it's fast.
    let chunks = chunk_snapshot(&bytes);
    if chunk_index as usize >= chunks.len() {
        return Err(ServeError::IndexOutOfRange {
            requested: chunk_index,
            chunk_count: chunks.len() as u32,
        });
    }
    // Recompute the Merkle root as a defense-in-depth check:
    // if the anchored merkle_root disagrees with what fresh
    // chunking produces from the same bytes, something has
    // gone wrong upstream (probably the anchor was written for
    // a different snapshot).  Better to refuse than send a
    // chunk that won't verify at the requester.
    let hashes: Vec<[u8; 32]> = chunks.iter().map(|c| c.hash).collect();
    let recomputed_merkle_root = snapshot_merkle_root(&hashes);
    if recomputed_merkle_root != merkle_root {
        return Err(ServeError::AtomicRootMismatch);
    }
    let chunk = &chunks[chunk_index as usize];
    let proof = build_merkle_proof(&hashes, chunk_index).expect("in-range index has a proof");
    Ok(SnapshotChunkResponse {
        block_hash: Bytes::copy_from_slice(block_hash),
        chunk_index,
        chunk_bytes: Bytes::copy_from_slice(&chunk.bytes),
        chunk_hash: Bytes::copy_from_slice(&chunk.hash),
        merkle_root: Bytes::copy_from_slice(&merkle_root),
        chunk_count: chunks.len() as u32,
        merkle_proof: proof
            .siblings
            .into_iter()
            .map(|(sibling_hash, is_sibling_right)| MerkleProofStep {
                sibling_hash: Bytes::copy_from_slice(&sibling_hash),
                is_sibling_right,
            })
            .collect(),
    })
}

/// Announce a snapshot's shape to peers.  Called when a joiner
/// asks `HasSnapshotRequest`; the server replies with
/// [`HasSnapshot`] iff it has a cached anchor the local file
/// matches on BOTH `atomic_root` and `merkle_root`.
///
/// Returns the shape (merkle_root + chunk_count) suitable for
/// serialization into `HasSnapshotProto`.  The merkle_root
/// field is ADVISORY per the proto docstring — the joiner MUST
/// re-verify against its locally-anchored root (block
/// finalization) regardless of what we send here.
///
/// # Symmetric defense-in-depth with [`serve_chunk`]
///
/// Performs the SAME two integrity checks [`serve_chunk`] does:
///
///   1. `read_snapshot_bytes` rechecks
///      `Blake2b256(bytes) == atomic_root`.
///   2. The Merkle root is recomputed from fresh chunking and
///      compared against the anchored `merkle_root`.
///
/// A server MUST NOT announce availability of a snapshot it
/// can't actually serve.  If the anchor's `merkle_root` is
/// corrupt (even with a valid `atomic_root`), a subsequent
/// `serve_chunk` would reject — announcing in that case would
/// cost the joiner a wasted round-trip plus a peer-switch.
/// Catching here closes the loop earlier.
///
/// # Cost
///
/// Re-chunks the on-disk bytes to get `chunk_count` AND the
/// recomputed root.  Keeps this stateless and avoids caching
/// yet-another derived value on `RuntimeManager`.  Bounded by
/// MAX_PAYLOAD_BYTES / CHUNK_SIZE — a few MiB of CPU work per
/// announcement.  If announcement volume ever becomes a hot
/// path, cache `(chunk_count, merkle_root)` alongside the
/// anchor in a future slice.
pub fn has_snapshot_announcement(
    block_hash: &[u8],
    anchor: ([u8; 32], [u8; 32]),
    snapshot_dir: &Path,
) -> Result<HasSnapshot, ServeError> {
    let (atomic_root, merkle_root) = anchor;
    let bytes = read_snapshot_bytes(snapshot_dir, &atomic_root).map_err(|e| match e {
        rholang::rust::interpreter::io::snapshot::SnapshotError::RootMismatch { .. } => {
            ServeError::AtomicRootMismatch
        }
        other => ServeError::DiskReadFailed(format!("{other:?}")),
    })?;
    let chunks = chunk_snapshot(&bytes);
    // Symmetric with serve_chunk: refuse to announce a
    // snapshot whose stored merkle_root disagrees with what
    // fresh chunking of the atomic-root-verified bytes
    // produces.
    let hashes: Vec<[u8; 32]> = chunks.iter().map(|c| c.hash).collect();
    let recomputed_merkle_root = snapshot_merkle_root(&hashes);
    if recomputed_merkle_root != merkle_root {
        return Err(ServeError::AtomicRootMismatch);
    }
    Ok(HasSnapshot {
        block_hash: Bytes::copy_from_slice(block_hash),
        merkle_root: Bytes::copy_from_slice(&merkle_root),
        chunk_count: chunks.len() as u32,
    })
}

#[cfg(test)]
mod tests {
    use rholang::rust::interpreter::io::snapshot::write_snapshot;
    use rholang::rust::interpreter::io::snapshot_chunk::{
        verify_chunk_hash, verify_merkle_proof, MerkleProof, SnapshotChunk,
    };
    use rholang::rust::interpreter::io::wal::{PayloadRef, WalEntry, WalOp, WalOutcome};

    use super::*;

    fn mk_entry(tag: &str) -> WalEntry {
        WalEntry {
            op: WalOp::Write,
            path: std::path::PathBuf::from(format!("/{tag}")),
            extra_path: None,
            offset: Some(0),
            length: Some(tag.len() as u64),
            payload_ref: Some(PayloadRef::hash(tag.as_bytes())),
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        }
    }

    /// Server's response verifies end-to-end: rehash chunk_bytes
    /// against chunk_hash, walk the inclusion proof against the
    /// anchored merkle_root, and confirm the response's
    /// metadata (chunk_count, merkle_root) matches the anchor.
    #[test]
    fn serve_chunk_response_verifies_against_anchored_root() {
        let dir = tempfile::tempdir().unwrap();
        let entries = vec![mk_entry("a"), mk_entry("b"), mk_entry("c")];
        let (_path, atomic_root, merkle_root) = write_snapshot(dir.path(), &entries).unwrap();
        let block_hash = vec![0xAB; 32];

        let response = serve_chunk(&block_hash, 0, (atomic_root, merkle_root), dir.path())
            .expect("serve_chunk must succeed on well-formed anchor");

        // Echo fields match request + anchor.
        assert_eq!(response.block_hash.as_ref(), block_hash.as_slice());
        assert_eq!(response.chunk_index, 0);
        assert_eq!(response.merkle_root.as_ref(), merkle_root.as_slice());

        // chunk_hash matches Blake2b256(chunk_bytes).  Verifies
        // the server populated the ADVISORY echo correctly.
        let synthetic = SnapshotChunk {
            index: response.chunk_index,
            bytes: response.chunk_bytes.to_vec(),
            hash: {
                let mut h = [0u8; 32];
                h.copy_from_slice(&response.chunk_hash);
                h
            },
        };
        assert!(verify_chunk_hash(&synthetic));

        // Inclusion proof verifies against the response's own
        // merkle_root (which matches the anchor per the above
        // assertion, so this also verifies against the anchored
        // root).
        let anchored = {
            let mut r = [0u8; 32];
            r.copy_from_slice(&response.merkle_root);
            r
        };
        let proof = MerkleProof {
            index: response.chunk_index,
            siblings: response
                .merkle_proof
                .iter()
                .map(|s| {
                    let mut h = [0u8; 32];
                    h.copy_from_slice(&s.sibling_hash);
                    (h, s.is_sibling_right)
                })
                .collect(),
        };
        assert!(verify_merkle_proof(&anchored, &synthetic.hash, &proof));
    }

    /// Out-of-range chunk_index yields IndexOutOfRange with the
    /// requested value AND the actual chunk count — so the
    /// client can learn the correct count without an extra
    /// round-trip.
    #[test]
    fn serve_chunk_rejects_out_of_range_index() {
        let dir = tempfile::tempdir().unwrap();
        let entries = vec![mk_entry("x")];
        let (_path, atomic_root, merkle_root) = write_snapshot(dir.path(), &entries).unwrap();
        let err = serve_chunk(&[0x00; 32], 99, (atomic_root, merkle_root), dir.path())
            .expect_err("out-of-range must error");
        match err {
            ServeError::IndexOutOfRange {
                requested,
                chunk_count,
            } => {
                assert_eq!(requested, 99);
                assert!(chunk_count >= 1);
            }
            other => panic!("expected IndexOutOfRange, got {other:?}"),
        }
    }

    /// LOAD-BEARING: a bogus merkle_root anchor is detected
    /// before any chunk is served.  Prevents the server from
    /// sending chunks the client cannot verify (which would
    /// waste the client's bandwidth and force a peer-switch).
    #[test]
    fn serve_chunk_rejects_wrong_merkle_anchor() {
        let dir = tempfile::tempdir().unwrap();
        let entries = vec![mk_entry("y")];
        let (_path, atomic_root, _real_merkle) = write_snapshot(dir.path(), &entries).unwrap();
        let bogus_merkle = [0xFFu8; 32];
        let err = serve_chunk(&[0x00; 32], 0, (atomic_root, bogus_merkle), dir.path())
            .expect_err("wrong merkle anchor must error");
        assert_eq!(err, ServeError::AtomicRootMismatch);
    }

    /// LOAD-BEARING: tampered atomic_root (bytes-vs-anchor
    /// mismatch) is caught by `read_snapshot_bytes` before
    /// chunking.
    #[test]
    fn serve_chunk_rejects_wrong_atomic_root() {
        let dir = tempfile::tempdir().unwrap();
        let entries = vec![mk_entry("z")];
        let (_path, _real_atomic, merkle_root) = write_snapshot(dir.path(), &entries).unwrap();
        let bogus_atomic = [0xEEu8; 32];
        let err = serve_chunk(&[0x00; 32], 0, (bogus_atomic, merkle_root), dir.path())
            .expect_err("wrong atomic anchor must error");
        // read_snapshot_bytes returns a disk-read error when
        // the atomic_root key is absent (no file at that path).
        // Not AtomicRootMismatch — that would require the file
        // to EXIST but hash wrong.
        assert!(matches!(err, ServeError::DiskReadFailed(_)));
    }

    /// `has_snapshot_announcement` returns the chunk_count the
    /// joiner would need to fetch all pieces, keyed on the
    /// anchored (atomic_root, merkle_root) pair.
    #[test]
    fn has_snapshot_announcement_returns_shape() {
        let dir = tempfile::tempdir().unwrap();
        let entries = vec![mk_entry("a"), mk_entry("b")];
        let (_path, atomic_root, merkle_root) = write_snapshot(dir.path(), &entries).unwrap();
        let block_hash = vec![0xCC; 32];

        let announcement =
            has_snapshot_announcement(&block_hash, (atomic_root, merkle_root), dir.path())
                .expect("has_snapshot_announcement must succeed");
        assert_eq!(announcement.block_hash.as_ref(), block_hash.as_slice());
        assert_eq!(announcement.merkle_root.as_ref(), merkle_root.as_slice());
        assert!(announcement.chunk_count >= 1);
    }

    /// `has_snapshot_announcement` reports the same chunk_count
    /// `serve_chunk` would catalog — pins the server-side
    /// consistency between the lookup and fetch paths.
    #[test]
    fn has_snapshot_announcement_chunk_count_matches_serve_chunk() {
        let dir = tempfile::tempdir().unwrap();
        let entries = vec![mk_entry("a"), mk_entry("b"), mk_entry("c")];
        let (_path, atomic_root, merkle_root) = write_snapshot(dir.path(), &entries).unwrap();

        let announcement =
            has_snapshot_announcement(&[0x00; 32], (atomic_root, merkle_root), dir.path())
                .expect("announcement must succeed");

        let response = serve_chunk(&[0x00; 32], 0, (atomic_root, merkle_root), dir.path())
            .expect("serve_chunk must succeed");

        assert_eq!(announcement.chunk_count, response.chunk_count);
    }

    /// LOAD-BEARING: `has_snapshot_announcement` fails on a
    /// tampered atomic_root (storage integrity).  The server
    /// MUST NOT announce availability of a snapshot it can't
    /// actually serve.
    #[test]
    fn has_snapshot_announcement_rejects_wrong_atomic_root() {
        let dir = tempfile::tempdir().unwrap();
        let entries = vec![mk_entry("x")];
        let (_path, _real_atomic, merkle_root) = write_snapshot(dir.path(), &entries).unwrap();
        let bogus_atomic = [0xEEu8; 32];
        let err = has_snapshot_announcement(&[0x00; 32], (bogus_atomic, merkle_root), dir.path())
            .expect_err("wrong atomic anchor must fail announcement");
        assert!(matches!(err, ServeError::DiskReadFailed(_)));
    }

    /// LOAD-BEARING symmetric defense: `has_snapshot_announcement`
    /// recomputes the Merkle root from fresh chunking AND rejects
    /// a corrupt merkle_root in the anchor — closing the
    /// "atomic_root valid but merkle_root wrong" edge case that
    /// would otherwise cost the joiner a wasted round-trip plus
    /// a peer-switch.  Mirrors `serve_chunk`'s own merkle-anchor
    /// check.
    #[test]
    fn has_snapshot_announcement_rejects_wrong_merkle_anchor() {
        let dir = tempfile::tempdir().unwrap();
        let entries = vec![mk_entry("x")];
        let (_path, atomic_root, _real_merkle) = write_snapshot(dir.path(), &entries).unwrap();
        let bogus_merkle = [0xFFu8; 32];
        let err = has_snapshot_announcement(&[0x00; 32], (atomic_root, bogus_merkle), dir.path())
            .expect_err("wrong merkle anchor must fail announcement");
        assert_eq!(err, ServeError::AtomicRootMismatch);
    }

    // --- Multi-chunk coverage ---------------------------------
    //
    // The small-snapshot tests above produce single-chunk
    // outputs (empty inclusion proofs).  Build a multi-chunk
    // snapshot by padding each entry's path to a few KiB and
    // using enough entries to exceed CHUNK_SIZE (4 MiB).  This
    // exercises the real multi-step inclusion-proof walk on
    // both `serve_chunk` and the end-to-end verify path.

    /// Build a WalEntry with a padded path field, used to force
    /// multi-chunk snapshots in the test below.
    fn mk_padded_entry(tag: &str, path_pad_bytes: usize) -> WalEntry {
        let mut p = String::with_capacity(path_pad_bytes + tag.len() + 1);
        p.push('/');
        p.push_str(tag);
        // Pad with ASCII '.' bytes so the serialized length is
        // predictably large.
        for _ in 0..path_pad_bytes {
            p.push('.');
        }
        WalEntry {
            op: WalOp::Write,
            path: std::path::PathBuf::from(p),
            extra_path: None,
            offset: Some(0),
            length: Some(tag.len() as u64),
            payload_ref: Some(PayloadRef::hash(tag.as_bytes())),
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        }
    }

    /// LOAD-BEARING end-to-end integration pin for the multi-
    /// chunk code path.  Builds a snapshot large enough to
    /// require multiple chunks (so `merkle_proof` has >0
    /// siblings), serves an interior chunk, verifies the
    /// chunk's inclusion proof walks correctly against the
    /// anchored Merkle root.  The small-snapshot tests above
    /// cover the single-chunk / empty-proof path; this test
    /// covers the real multi-step walk.
    #[test]
    fn serve_chunk_multi_chunk_proof_verifies_against_anchored_root() {
        let dir = tempfile::tempdir().unwrap();
        // Target: at least 3 chunks → inclusion proof for an
        // interior chunk has >= 1 sibling at each level.
        // ~2 KiB per entry × 8192 entries ≈ 16 MiB → ~4 chunks.
        let path_pad = 2048;
        let n_entries = 8192;
        let entries: Vec<WalEntry> = (0..n_entries)
            .map(|i| mk_padded_entry(&format!("e{i}"), path_pad))
            .collect();
        let (_path, atomic_root, merkle_root) = write_snapshot(dir.path(), &entries).unwrap();

        // Serve chunk 1 (interior — not head, not tail — so the
        // inclusion proof has siblings on both left + right
        // sides of the tree walk).
        let block_hash = vec![0xAB; 32];
        let interior_index = 1;
        let response = serve_chunk(
            &block_hash,
            interior_index,
            (atomic_root, merkle_root),
            dir.path(),
        )
        .expect("serve_chunk must succeed");

        // Multi-chunk: chunk_count >= 2 AND proof has >= 1 step.
        assert!(
            response.chunk_count >= 2,
            "test fixture must produce multiple chunks"
        );
        assert!(
            !response.merkle_proof.is_empty(),
            "multi-chunk response must carry a non-empty inclusion proof"
        );

        // End-to-end verification: rehash chunk + walk proof
        // against anchored root.
        let synthetic = SnapshotChunk {
            index: response.chunk_index,
            bytes: response.chunk_bytes.to_vec(),
            hash: {
                let mut h = [0u8; 32];
                h.copy_from_slice(&response.chunk_hash);
                h
            },
        };
        assert!(verify_chunk_hash(&synthetic));

        let anchored = {
            let mut r = [0u8; 32];
            r.copy_from_slice(&response.merkle_root);
            r
        };
        let proof = MerkleProof {
            index: response.chunk_index,
            siblings: response
                .merkle_proof
                .iter()
                .map(|s| {
                    let mut h = [0u8; 32];
                    h.copy_from_slice(&s.sibling_hash);
                    (h, s.is_sibling_right)
                })
                .collect(),
        };
        assert!(
            verify_merkle_proof(&anchored, &synthetic.hash, &proof),
            "multi-chunk inclusion proof must verify against anchored merkle_root"
        );
    }
}
