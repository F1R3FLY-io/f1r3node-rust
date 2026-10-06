// Wave 3 PR 3.8 — snapshot chunk-fetch sync integration tests.
//
// Exercises the server (`snapshot_chunk_server`) + retriever
// (`snapshot_chunk_retriever`) primitives at the state-machine
// composition level — the kind of sequence a yet-to-land
// sync-driver slice would run end-to-end over the comm layer.
//
// These tests use ONLY the public API: no module-internal test
// fixtures.  They pin the composed behavior across multi-step
// scenarios (full snapshot download, partial failure recovery,
// byzantine-peer defense in composed context).

use casper::rust::engine::snapshot_chunk_retriever::{
    AdmitOutcome, SnapshotChunkRetriever, SnapshotTarget, MAX_CHUNK_BYTES, MAX_MERKLE_PROOF_STEPS,
};
use casper::rust::engine::snapshot_chunk_server::{has_snapshot_announcement, serve_chunk};
use models::rust::casper::protocol::casper_message::{MerkleProofStep, SnapshotChunkResponse};
use prost::bytes::Bytes;
use rholang::rust::interpreter::io::snapshot::write_snapshot;
use rholang::rust::interpreter::io::wal::{PayloadRef, WalEntry, WalOp, WalOutcome};

/// Build a WAL entry with a padded path for controlling the
/// serialized size.  Each padding byte adds roughly 1 byte to
/// the on-disk snapshot.
fn mk_padded_entry(tag: &str, path_pad_bytes: usize) -> WalEntry {
    let mut p = String::with_capacity(path_pad_bytes + tag.len() + 1);
    p.push('/');
    p.push_str(tag);
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

/// Scenario fixture: build a multi-chunk snapshot on disk and
/// return `(block_hash, atomic_root, merkle_root, chunk_count,
/// snapshot_dir)`.  The `tag` byte is XOR'd into entry
/// identifiers so different tags produce distinct snapshots
/// with distinct roots.  Caller drops `_dir` at scope end to
/// clean up.
fn multi_chunk_snapshot_fixture(tag: u8) -> (Vec<u8>, [u8; 32], [u8; 32], u32, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    // ~2 KiB per entry × 8192 entries ≈ 16 MiB → ~4 chunks.
    // Tag goes into the entry name so different fixtures
    // produce different snapshots.
    let entries: Vec<WalEntry> = (0..8192)
        .map(|i| mk_padded_entry(&format!("e-{tag:02x}-{i}"), 2048))
        .collect();
    let (_path, atomic_root, merkle_root) = write_snapshot(dir.path(), &entries).unwrap();
    let announcement =
        has_snapshot_announcement(&[tag; 32], (atomic_root, merkle_root), dir.path())
            .expect("announcement");
    assert!(
        announcement.chunk_count >= 2,
        "fixture must produce multi-chunk snapshot (got {})",
        announcement.chunk_count
    );
    (
        vec![tag; 32],
        atomic_root,
        merkle_root,
        announcement.chunk_count,
        dir,
    )
}

/// LOAD-BEARING: end-to-end multi-chunk snapshot sync.  A fresh
/// joiner starts with a `SnapshotTarget` pinned from a
/// `HasSnapshot` announcement, fetches every chunk via
/// `serve_chunk`, admits each response, and finally reconstructs
/// the full snapshot bytes.  Pins the composed server→retriever
/// pipeline at the scenario level.
#[tokio::test]
async fn full_multi_chunk_snapshot_syncs_end_to_end() {
    let (block_hash, atomic_root, merkle_root, chunk_count, dir) =
        multi_chunk_snapshot_fixture(0xA0);

    // Joiner-side: pin target from the server's announcement.
    let target = SnapshotTarget {
        block_hash: block_hash.clone(),
        merkle_root,
        chunk_count,
    };
    let retriever = SnapshotChunkRetriever::new(target);
    assert_eq!(retriever.pending_count().await, chunk_count as usize);

    // Fetch each chunk in order (sync-driver would run this
    // concurrently; sequential here keeps the test readable).
    for i in 0..chunk_count {
        let response =
            serve_chunk(&block_hash, i, (atomic_root, merkle_root), dir.path()).expect("serve");
        assert_eq!(
            retriever.admit_response(&response).await,
            AdmitOutcome::ChunkAccepted,
            "chunk {i} must admit cleanly"
        );
    }
    assert!(retriever.is_complete().await);

    // Reconstructed bytes match the on-disk snapshot.
    let assembled = retriever.assemble().await.expect("complete → Some");
    let expected = std::fs::read(rholang::rust::interpreter::io::snapshot::snapshot_path(
        dir.path(),
        &atomic_root,
    ))
    .expect("read snapshot");
    assert_eq!(assembled, expected);
}

/// LOAD-BEARING: partial-failure recovery.  A byzantine peer
/// returns a tampered chunk for one index while other peers
/// serve the rest cleanly.  The retriever accepts the clean
/// chunks, rejects the tampered one, and remains pending on
/// that index — a sync-driver would then re-request from a
/// different peer.  Pins that the state machine doesn't
/// discard progress on partial byzantine input.
#[tokio::test]
async fn partial_byzantine_response_leaves_tampered_index_pending() {
    let (block_hash, atomic_root, merkle_root, chunk_count, dir) =
        multi_chunk_snapshot_fixture(0xA1);

    let target = SnapshotTarget {
        block_hash: block_hash.clone(),
        merkle_root,
        chunk_count,
    };
    let retriever = SnapshotChunkRetriever::new(target);

    // Admit a tampered response for chunk 1 (byzantine peer).
    let mut tampered = serve_chunk(&block_hash, 1, (atomic_root, merkle_root), dir.path()).unwrap();
    let mut b = tampered.chunk_bytes.to_vec();
    b[0] ^= 0x01;
    tampered.chunk_bytes = Bytes::from(b);
    assert_eq!(
        retriever.admit_response(&tampered).await,
        AdmitOutcome::ChunkHashMismatch,
    );
    assert!(retriever.pending_indices().await.contains(&1));

    // Admit clean responses for every OTHER chunk.
    for i in 0..chunk_count {
        if i == 1 {
            continue;
        }
        let response = serve_chunk(&block_hash, i, (atomic_root, merkle_root), dir.path()).unwrap();
        assert_eq!(
            retriever.admit_response(&response).await,
            AdmitOutcome::ChunkAccepted,
        );
    }

    // Chunk 1 is still pending; the rest are not.
    assert_eq!(retriever.pending_indices().await, vec![1]);
    assert!(!retriever.is_complete().await);

    // Clean re-serve of chunk 1 admits — progress on the other
    // chunks is preserved.
    let clean = serve_chunk(&block_hash, 1, (atomic_root, merkle_root), dir.path()).unwrap();
    assert_eq!(
        retriever.admit_response(&clean).await,
        AdmitOutcome::ChunkAccepted,
    );
    assert!(retriever.is_complete().await);
}

/// A joiner pinned on a stale `merkle_root` rejects responses
/// from a server with a DIFFERENT snapshot.  Pins the
/// cross-target isolation: an attacker that serves well-formed
/// chunks for a different snapshot cannot corrupt the joiner's
/// view.
#[tokio::test]
async fn stale_target_rejects_wrong_snapshot_responses() {
    let (block_hash_a, atomic_a, merkle_a, chunk_count_a, dir_a) =
        multi_chunk_snapshot_fixture(0xA2);
    let (_block_hash_b, atomic_b, merkle_b, _chunk_count_b, dir_b) =
        multi_chunk_snapshot_fixture(0xA3);
    // Ensure the two snapshots differ.
    assert_ne!(atomic_a, atomic_b);
    assert_ne!(merkle_a, merkle_b);

    let target = SnapshotTarget {
        block_hash: block_hash_a.clone(),
        merkle_root: merkle_a,
        chunk_count: chunk_count_a,
    };
    let retriever = SnapshotChunkRetriever::new(target);

    // A byzantine peer serves a chunk from snapshot B with
    // block_hash claimed as A.
    let mut bogus = serve_chunk(&block_hash_a, 0, (atomic_b, merkle_b), dir_b.path()).unwrap();
    // Response's merkle_root is B's; retriever pinned A's.
    assert_eq!(
        retriever.admit_response(&bogus).await,
        AdmitOutcome::MerkleRootMismatch,
    );

    // Even with the block_hash spoofed to A's but merkle_root
    // from B — same rejection.
    bogus.block_hash = Bytes::copy_from_slice(&block_hash_a);
    assert_eq!(
        retriever.admit_response(&bogus).await,
        AdmitOutcome::MerkleRootMismatch,
    );

    // Clean serve from A's snapshot still admits.
    let clean = serve_chunk(&block_hash_a, 0, (atomic_a, merkle_a), dir_a.path()).unwrap();
    assert_eq!(
        retriever.admit_response(&clean).await,
        AdmitOutcome::ChunkAccepted,
    );
}

/// Byzantine CPU-exhaustion defense in a composed context: a
/// peer flooding oversized `chunk_bytes` or oversized
/// `merkle_proof` payloads gets rejected at the security caps,
/// well before any hashing or proof-walking runs.  Pins the
/// cheap-first ordering holds when the retriever is loaded
/// with real pending state (not just a fresh retriever).
#[tokio::test]
async fn security_caps_reject_before_hashing_mid_sync() {
    let (block_hash, atomic_root, merkle_root, chunk_count, dir) =
        multi_chunk_snapshot_fixture(0xA4);

    let target = SnapshotTarget {
        block_hash: block_hash.clone(),
        merkle_root,
        chunk_count,
    };
    let retriever = SnapshotChunkRetriever::new(target);

    // Admit chunk 0 cleanly.
    let r0 = serve_chunk(&block_hash, 0, (atomic_root, merkle_root), dir.path()).unwrap();
    assert_eq!(
        retriever.admit_response(&r0).await,
        AdmitOutcome::ChunkAccepted,
    );

    // Byzantine peer floods an oversized chunk_bytes response
    // targeting an untracked chunk_index.
    let bogus_oversized = SnapshotChunkResponse {
        block_hash: Bytes::copy_from_slice(&block_hash),
        chunk_index: chunk_count + 42, // untracked
        chunk_bytes: Bytes::from(vec![0u8; MAX_CHUNK_BYTES + 1]),
        chunk_hash: Bytes::copy_from_slice(&[0u8; 32]),
        merkle_root: Bytes::copy_from_slice(&merkle_root),
        chunk_count,
        merkle_proof: vec![],
    };
    // UnknownRequest fires FIRST (even before the oversized
    // cap) — the retriever's PR #555 review-fix.
    assert_eq!(
        retriever.admit_response(&bogus_oversized).await,
        AdmitOutcome::UnknownRequest,
    );

    // Byzantine peer floods an oversized merkle_proof for a
    // TRACKED chunk_index — this one hits the proof-length cap.
    let proof_dos = SnapshotChunkResponse {
        block_hash: Bytes::copy_from_slice(&block_hash),
        chunk_index: 1,
        chunk_bytes: Bytes::copy_from_slice(&[0u8; 100]),
        chunk_hash: Bytes::copy_from_slice(&[0u8; 32]),
        merkle_root: Bytes::copy_from_slice(&merkle_root),
        chunk_count,
        merkle_proof: (0..(MAX_MERKLE_PROOF_STEPS + 10))
            .map(|_| MerkleProofStep {
                sibling_hash: Bytes::copy_from_slice(&[0u8; 32]),
                is_sibling_right: false,
            })
            .collect(),
    };
    assert_eq!(
        retriever.admit_response(&proof_dos).await,
        AdmitOutcome::MerkleProofInvalid,
    );

    // Clean chunks still admit after the byzantine floods.
    for i in 1..chunk_count {
        let r = serve_chunk(&block_hash, i, (atomic_root, merkle_root), dir.path()).unwrap();
        assert_eq!(
            retriever.admit_response(&r).await,
            AdmitOutcome::ChunkAccepted,
        );
    }
    assert!(retriever.is_complete().await);
}

/// Single-chunk snapshot fast-path: a small snapshot that fits
/// in one chunk still exercises the full server + retriever
/// pipeline with a zero-step inclusion proof.  Pins that the
/// retriever doesn't accidentally require >0 proof steps.
#[tokio::test]
async fn single_chunk_snapshot_syncs_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let entries = vec![mk_padded_entry("x", 100)];
    let (_path, atomic_root, merkle_root) = write_snapshot(dir.path(), &entries).unwrap();
    let block_hash = vec![0xB0; 32];

    let announcement =
        has_snapshot_announcement(&block_hash, (atomic_root, merkle_root), dir.path()).unwrap();
    assert_eq!(announcement.chunk_count, 1);

    let target = SnapshotTarget {
        block_hash: block_hash.clone(),
        merkle_root,
        chunk_count: announcement.chunk_count,
    };
    let retriever = SnapshotChunkRetriever::new(target);

    let response = serve_chunk(&block_hash, 0, (atomic_root, merkle_root), dir.path()).unwrap();
    assert!(response.merkle_proof.is_empty()); // single-chunk tree, no siblings
    assert_eq!(
        retriever.admit_response(&response).await,
        AdmitOutcome::ChunkAccepted,
    );
    assert!(retriever.is_complete().await);
}

/// Chunk-count divergence: a byzantine server advertises a
/// correct `merkle_root` but inflates `chunk_count` by 1 (or
/// deflates it).  The retriever accepts chunks against the
/// pinned chunk_count, rejects any response claiming a
/// different count.  Pins the chunk_count check.
#[tokio::test]
async fn chunk_count_mismatch_rejected() {
    let (block_hash, atomic_root, merkle_root, chunk_count, dir) =
        multi_chunk_snapshot_fixture(0xA5);

    let target = SnapshotTarget {
        block_hash: block_hash.clone(),
        merkle_root,
        chunk_count,
    };
    let retriever = SnapshotChunkRetriever::new(target);

    // Byzantine server: build a response claiming an inflated
    // chunk_count.
    let mut lying = serve_chunk(&block_hash, 0, (atomic_root, merkle_root), dir.path()).unwrap();
    lying.chunk_count = chunk_count + 1;
    assert_eq!(
        retriever.admit_response(&lying).await,
        AdmitOutcome::ChunkCountMismatch,
    );

    // Clean responses still admit — the retriever hasn't been
    // corrupted by the lying response.
    let clean = serve_chunk(&block_hash, 0, (atomic_root, merkle_root), dir.path()).unwrap();
    assert_eq!(
        retriever.admit_response(&clean).await,
        AdmitOutcome::ChunkAccepted,
    );
}

/// Interior-chunk inclusion-proof walk: on a multi-chunk
/// snapshot, serving an interior chunk (not head, not tail)
/// produces a non-trivial proof.  Pins that the composed
/// pipeline walks the proof correctly for the hardest case.
#[tokio::test]
async fn interior_chunk_proof_walks_cleanly() {
    let (block_hash, atomic_root, merkle_root, chunk_count, dir) =
        multi_chunk_snapshot_fixture(0xA6);
    assert!(chunk_count >= 3, "need multi-chunk for an interior index");

    let target = SnapshotTarget {
        block_hash: block_hash.clone(),
        merkle_root,
        chunk_count,
    };
    let retriever = SnapshotChunkRetriever::new(target);

    // Pick an interior index — not 0, not chunk_count - 1.
    let interior = chunk_count / 2;
    let response = serve_chunk(
        &block_hash,
        interior,
        (atomic_root, merkle_root),
        dir.path(),
    )
    .unwrap();
    assert!(
        !response.merkle_proof.is_empty(),
        "interior chunk must carry a non-empty inclusion proof"
    );
    assert_eq!(
        retriever.admit_response(&response).await,
        AdmitOutcome::ChunkAccepted,
    );
}
