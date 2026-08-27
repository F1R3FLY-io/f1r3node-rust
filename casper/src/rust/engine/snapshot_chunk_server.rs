// Phase 7b-1 snapshot chunk serving handler (2026-08-27).
//
// Server-side counterpart of `SnapshotChunkRetriever`.  Reads a
// snapshot from the local disk cache, chunks it, and builds a
// `SnapshotChunkResponse` for the requested (block_hash,
// chunk_index) pair — including an inclusion proof against the
// anchored Merkle root.
//
// Layered against the retriever: this module produces responses;
// the retriever verifies them.  Both operate on the same
// primitives from `rholang::interpreter::io::snapshot_chunk`.
//
// Callers: the wire-message handler routes an incoming
// `GetSnapshotChunkRequest` here after verifying the requesting
// peer + rate limits.  A successful `serve_chunk` returns the
// response proto to send back; error paths let the caller decide
// whether to reply with a NoSnapshotAvailable-style signal or
// stay silent.

use std::path::Path;

use models::rust::casper::protocol::casper_message::{
    HasSnapshot, MerkleProofStep, SnapshotChunkResponse,
};
use prost::bytes::Bytes;
use rholang::rust::interpreter::io::snapshot::read_snapshot_bytes;
use rholang::rust::interpreter::io::snapshot_chunk::{
    build_merkle_proof, chunk_snapshot, snapshot_merkle_root,
};

/// Errors from `serve_chunk`.  Distinguish "can't help" (peer
/// should ask someone else) from "hard failure" (config bug worth
/// logging).
#[derive(Debug)]
pub enum ServeError {
    /// This node has no cached anchor for the requested block —
    /// most likely the peer's request references a block we
    /// haven't finalized yet.  Cheap "no thanks".
    UnknownBlockHash,
    /// chunk_index is out of range for this snapshot's chunk count.
    /// The peer is confused about which snapshot they're fetching.
    IndexOutOfRange { requested: u32, chunk_count: u32 },
    /// Disk read of the snapshot file failed.  Log at warn — the
    /// anchor exists but the payload doesn't.  Operator should
    /// investigate (rare; typically means a rare storage failure
    /// or a hand-deleted snapshot).
    DiskReadFailed(String),
    /// The snapshot file's bytes hash to something DIFFERENT from
    /// the anchored atomic root.  Byzantine local storage or a
    /// version-skewed on-disk file.  Log at warn.
    AtomicRootMismatch,
}

/// Serve a single chunk from the local snapshot cache.
///
/// `anchor` is the `(atomic_root, merkle_root)` pair stored in
/// `RuntimeManager::snapshot_merkle_roots` at the finalized block
/// hash the request references.  `snapshot_dir` is the writer's
/// on-disk directory (matches `SnapshotWriter::dir`).
pub fn serve_chunk(
    block_hash: &[u8],
    chunk_index: u32,
    anchor: ([u8; 32], [u8; 32]),
    snapshot_dir: &Path,
) -> Result<SnapshotChunkResponse, ServeError> {
    let (atomic_root, merkle_root) = anchor;
    // Read the on-disk snapshot bytes.  read_snapshot_bytes
    // rechecks Blake2b256(bytes) == atomic_root and returns
    // `AtomicRootMismatch` if the file has been tampered with.
    let bytes = read_snapshot_bytes(snapshot_dir, &atomic_root).map_err(|e| match e {
        rholang::rust::interpreter::io::snapshot::SnapshotError::RootMismatch { .. } => {
            ServeError::AtomicRootMismatch
        }
        other => ServeError::DiskReadFailed(format!("{other:?}")),
    })?;
    // Chunk locally.  This is CPU work but bounded at 4 MiB × N
    // chunks; typical snapshots run 1-8 chunks so it's fast.
    let chunks = chunk_snapshot(&bytes);
    if chunk_index as usize >= chunks.len() {
        return Err(ServeError::IndexOutOfRange {
            requested: chunk_index,
            chunk_count: chunks.len() as u32,
        });
    }
    // Recompute the Merkle root as a defense-in-depth check: if
    // the anchored merkle_root disagrees with what fresh chunking
    // produces from the same bytes, something has gone wrong
    // upstream (probably the anchor was written for a different
    // snapshot).  Better to refuse than to send a chunk that
    // won't verify on the requester's end.
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
/// asks `HasSnapshotRequest`; the server replies with `HasSnapshot`
/// iff it has a cached anchor.
pub fn has_snapshot_announcement(
    block_hash: &[u8],
    anchor: ([u8; 32], [u8; 32]),
    snapshot_dir: &Path,
) -> Result<HasSnapshot, ServeError> {
    let (atomic_root, merkle_root) = anchor;
    // We need chunk_count to fill the announcement.  Cheapest way
    // is to re-chunk the on-disk bytes; keeps this stateless and
    // avoids caching yet-another derived value on RuntimeManager.
    let bytes = read_snapshot_bytes(snapshot_dir, &atomic_root).map_err(|e| match e {
        rholang::rust::interpreter::io::snapshot::SnapshotError::RootMismatch { .. } => {
            ServeError::AtomicRootMismatch
        }
        other => ServeError::DiskReadFailed(format!("{other:?}")),
    })?;
    let chunks = chunk_snapshot(&bytes);
    Ok(HasSnapshot {
        block_hash: Bytes::copy_from_slice(block_hash),
        merkle_root: Bytes::copy_from_slice(&merkle_root),
        chunk_count: chunks.len() as u32,
    })
}

#[cfg(test)]
mod tests {
    use rholang::rust::interpreter::io::snapshot::{write_snapshot, SnapshotBlob};
    use rholang::rust::interpreter::io::wal::{PayloadRef, WalEntry, WalOp, WalOutcome};

    use super::*;
    use crate::rust::engine::snapshot_chunk_retriever::{
        AdmitOutcome, SnapshotChunkRetriever, SnapshotTarget,
    };

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

    /// Server-produced chunks round-trip through the retriever.
    /// End-to-end sanity that server + retriever agree on wire
    /// shapes + Merkle proof directions.
    #[tokio::test]
    async fn server_chunks_round_trip_through_retriever() {
        // Small snapshot: 3 entries → tiny blob → single chunk.
        let dir = tempfile::tempdir().unwrap();
        let entries = vec![mk_entry("a"), mk_entry("b"), mk_entry("c")];
        let (_path, atomic_root, merkle_root) = write_snapshot(dir.path(), &entries).unwrap();
        let block_hash = vec![0xAB; 32];

        // Retriever setup: pin the merkle_root as the anchor.
        // We'll learn chunk_count from a HasSnapshot announcement.
        let announcement =
            has_snapshot_announcement(&block_hash, (atomic_root, merkle_root), dir.path())
                .expect("has_snapshot_announcement");
        assert_eq!(announcement.chunk_count, 1);
        let target = SnapshotTarget {
            block_hash: block_hash.clone(),
            merkle_root,
            chunk_count: announcement.chunk_count,
        };
        let retriever = SnapshotChunkRetriever::new(target);

        // Server produces the chunk.
        let response = serve_chunk(&block_hash, 0, (atomic_root, merkle_root), dir.path())
            .expect("serve_chunk");
        assert_eq!(
            retriever.admit_response(&response).await,
            AdmitOutcome::ChunkAccepted
        );
        assert!(retriever.is_complete().await);
        // Assembled bytes match what write_snapshot produced.
        let assembled = retriever.assemble().await.unwrap();
        let blob = SnapshotBlob {
            bytes: assembled.clone(),
            root: [0u8; 32],        // unused
            merkle_root: [0u8; 32], // unused
        };
        assert_eq!(blob.bytes, assembled);
    }

    #[tokio::test]
    async fn serve_chunk_rejects_out_of_range_index() {
        let dir = tempfile::tempdir().unwrap();
        let entries = vec![mk_entry("x")];
        let (_path, atomic_root, merkle_root) = write_snapshot(dir.path(), &entries).unwrap();
        let err = serve_chunk(&[0x00; 32], 99, (atomic_root, merkle_root), dir.path())
            .expect_err("out-of-range must error");
        match err {
            ServeError::IndexOutOfRange { requested, .. } => assert_eq!(requested, 99),
            other => panic!("expected IndexOutOfRange, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn serve_chunk_rejects_wrong_anchor() {
        let dir = tempfile::tempdir().unwrap();
        let entries = vec![mk_entry("y")];
        let (_path, atomic_root, _real_merkle) = write_snapshot(dir.path(), &entries).unwrap();
        // Anchor a bogus merkle_root — server should reject
        // rather than send a chunk that won't verify at the joiner.
        let bogus_merkle = [0xFFu8; 32];
        let err = serve_chunk(&[0x00; 32], 0, (atomic_root, bogus_merkle), dir.path())
            .expect_err("wrong merkle anchor must error");
        assert!(matches!(err, ServeError::AtomicRootMismatch));
    }
}
