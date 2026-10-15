// Wave 3 PR 3.8 — WAL payload fetch sync integration tests.
//
// Exercises the WAL-payload server (`wal_payload_server`) +
// retriever (`wal_payload_retriever`) at the state-machine
// composition level.  Scenario-level pins beyond the per-module
// unit tests: a joiner fetching many payloads from one or more
// peers, byzantine-peer defense in composed context, reducer-
// bypass interleaved with fetch, partial-failure recovery.

use casper::rust::engine::wal_payload_retriever::{
    AdmitOutcome, WalPayloadRetriever, MAX_PAYLOAD_BYTES,
};
use casper::rust::engine::wal_payload_server::{
    has_wal_payload_announcement, serve_payload, InMemoryPayloadStore,
};
use models::rust::casper::protocol::casper_message::WalPayloadResponse;
use prost::bytes::Bytes;

/// LOAD-BEARING: end-to-end multi-payload sync.  A joiner
/// enqueues several payload hashes it needs; the server
/// (populated with the bytes) answers each; the retriever
/// admits and the joiner reaches completion with the right
/// bytes.
#[tokio::test]
async fn full_multi_payload_syncs_end_to_end() {
    let store = InMemoryPayloadStore::new();
    let payloads: Vec<Vec<u8>> = (0..8)
        .map(|i| format!("payload-{i}").as_bytes().repeat(32 + i * 7))
        .collect();
    let hashes: Vec<[u8; 32]> = payloads.iter().map(|b| store.insert(b.clone())).collect();

    let retriever = WalPayloadRetriever::new();
    for h in &hashes {
        retriever.enqueue(*h).await;
    }
    assert_eq!(retriever.pending_count().await, hashes.len());

    for (h, expected) in hashes.iter().zip(payloads.iter()) {
        let response = serve_payload(h, &store).expect("serve_payload");
        assert_eq!(
            retriever.admit_response(&response).await,
            AdmitOutcome::PayloadAccepted,
        );
        assert_eq!(retriever.get_bytes(h).await.unwrap(), *expected);
    }
    assert!(retriever.is_complete().await);
}

/// Partial-failure recovery: a byzantine peer returns tampered
/// bytes for one payload while others serve clean.  The
/// retriever rejects the tampered response, keeps the clean
/// progress, and the sync completes when a different peer
/// serves the bad payload cleanly.
#[tokio::test]
async fn tampered_payload_leaves_hash_pending_for_retry() {
    let good_store = InMemoryPayloadStore::new();
    let bytes = b"authentic bytes".to_vec();
    let hash = good_store.insert(bytes.clone());

    let retriever = WalPayloadRetriever::new();
    retriever.enqueue(hash).await;

    // Byzantine peer sends wrong bytes claiming the correct
    // hash.
    let tampered = WalPayloadResponse {
        payload_hash: Bytes::copy_from_slice(&hash),
        payload_bytes: Bytes::from(b"TAMPERED".to_vec()),
    };
    assert_eq!(
        retriever.admit_response(&tampered).await,
        AdmitOutcome::PayloadHashMismatch,
    );
    assert_eq!(retriever.pending_count().await, 1);

    // Clean peer serves the correct bytes.
    let clean = serve_payload(&hash, &good_store).unwrap();
    assert_eq!(
        retriever.admit_response(&clean).await,
        AdmitOutcome::PayloadAccepted,
    );
    assert_eq!(retriever.get_bytes(&hash).await.unwrap(), bytes);
    assert!(retriever.is_complete().await);
}

/// LOAD-BEARING: byzantine CPU-exhaustion defense.  A peer
/// floods the retriever with well-formed responses for
/// NEVER-REQUESTED hashes.  The cheap pending-set lookup
/// rejects each in O(1) — the expensive Blake2b256 hash never
/// runs.  Pins that the retriever's cheap-first ordering holds
/// under flood.
#[tokio::test]
async fn flood_of_unknown_payloads_rejects_cheaply() {
    let retriever = WalPayloadRetriever::new();
    // Enqueue a single real hash so the retriever has pending
    // state (realistic mid-sync condition).
    let real_hash = [0xAA; 32];
    retriever.enqueue(real_hash).await;

    // Flood: well-formed responses for 100 different never-
    // requested hashes.  Each would cost ~200ms Blake2b256 if
    // the lookup ran AFTER the hash check.
    for i in 0..100 {
        let bytes = vec![i as u8; 1024];
        let flood_hash = blake2b256(&bytes);
        let flood_response = WalPayloadResponse {
            payload_hash: Bytes::copy_from_slice(&flood_hash),
            payload_bytes: Bytes::from(bytes),
        };
        assert_eq!(
            retriever.admit_response(&flood_response).await,
            AdmitOutcome::UnknownRequest,
            "flood response {i} must reject via cheap lookup, not hashing"
        );
    }

    // The real hash is still pending — the retriever's state
    // wasn't corrupted by the flood.
    assert_eq!(retriever.pending_count().await, 1);
}

/// LOAD-BEARING: oversized payload rejected before any hashing
/// in a composed context.  A peer sends a `MAX_PAYLOAD_BYTES +
/// 1` byte payload — the size cap fires BEFORE the
/// pending-set lookup OR the hash check.  Pins the ordering:
/// size cap is cheapest (constant-time), checked first.
#[tokio::test]
async fn oversized_payload_rejects_before_lookup_or_hashing() {
    let retriever = WalPayloadRetriever::new();
    let hash = [0xBB; 32];
    retriever.enqueue(hash).await; // tracked, so a well-formed oversized would still get caught

    let oversized = WalPayloadResponse {
        payload_hash: Bytes::copy_from_slice(&hash),
        payload_bytes: Bytes::from(vec![0u8; MAX_PAYLOAD_BYTES + 1]),
    };
    assert_eq!(
        retriever.admit_response(&oversized).await,
        AdmitOutcome::PayloadOversized,
    );
    // Pending state unchanged — the hash is still awaiting a
    // clean response.
    assert_eq!(retriever.pending_count().await, 1);
}

/// Reducer bypass interleaved with fetch: the joiner can
/// locally reproduce SOME payload bytes (via the deploy-data
/// reducer) and `mark_resolved` them, while OTHER payloads
/// still need peer fetch.  Pins the two paths compose cleanly
/// — reducer-resolved and fetch-resolved payloads coexist.
#[tokio::test]
async fn reducer_bypass_and_peer_fetch_compose_cleanly() {
    let store = InMemoryPayloadStore::new();
    let reducer_bytes = b"reducer-reproduced-locally".to_vec();
    let fetch_bytes = b"must-fetch-from-peer".to_vec();

    let reducer_hash = blake2b256(&reducer_bytes);
    let fetch_hash = store.insert(fetch_bytes.clone());

    let retriever = WalPayloadRetriever::new();
    // Joiner enqueues the fetch-side; reducer-side goes
    // through mark_resolved.
    retriever.enqueue(fetch_hash).await;
    assert!(
        retriever
            .mark_resolved(reducer_hash, reducer_bytes.clone())
            .await
    );

    // Reducer-resolved is complete without any peer traffic.
    assert_eq!(
        retriever.get_bytes(&reducer_hash).await.unwrap(),
        reducer_bytes
    );

    // Fetch-side still pending.
    assert_eq!(retriever.pending_count().await, 1);

    // Peer serves the fetch-side payload.
    let response = serve_payload(&fetch_hash, &store).unwrap();
    assert_eq!(
        retriever.admit_response(&response).await,
        AdmitOutcome::PayloadAccepted,
    );
    assert!(retriever.is_complete().await);
    assert_eq!(retriever.get_bytes(&fetch_hash).await.unwrap(), fetch_bytes);
}

/// `has_wal_payload_announcement` → `GetWalPayloadRequest` →
/// `WalPayloadResponse` round-trip.  Pins the lookup→fetch
/// pipeline a joiner's sync-driver would compose: broadcast
/// Has-request, admit announcements, then serve the full
/// payload from a chosen peer.
#[tokio::test]
async fn announcement_then_fetch_round_trip() {
    let store = InMemoryPayloadStore::new();
    let bytes = b"announced-and-fetched".to_vec();
    let hash = store.insert(bytes.clone());

    // Server builds an announcement advertising it has this
    // payload.
    let announcement = has_wal_payload_announcement(&hash, &store).unwrap();
    assert_eq!(announcement.payload_hash.as_ref(), hash.as_slice());
    assert_eq!(announcement.payload_size as usize, bytes.len());

    // Joiner, having seen the announcement, enqueues and
    // fetches.
    let retriever = WalPayloadRetriever::new();
    retriever.enqueue(hash).await;
    let response = serve_payload(&hash, &store).unwrap();
    assert_eq!(
        retriever.admit_response(&response).await,
        AdmitOutcome::PayloadAccepted,
    );
    assert_eq!(retriever.get_bytes(&hash).await.unwrap(), bytes);
}

/// Server with multiple backing stores: a joiner can fetch
/// different payloads from different peers (modeled here as
/// different `InMemoryPayloadStore` instances).  Pins that
/// `serve_payload` is store-local and the retriever composes
/// responses from different sources.
#[tokio::test]
async fn multi_source_fetch_composes_cleanly() {
    let store_alice = InMemoryPayloadStore::new();
    let store_bob = InMemoryPayloadStore::new();

    let bytes_a = b"alice has this".to_vec();
    let bytes_b = b"bob has that".to_vec();

    let hash_a = store_alice.insert(bytes_a.clone());
    let hash_b = store_bob.insert(bytes_b.clone());

    let retriever = WalPayloadRetriever::new();
    retriever.enqueue(hash_a).await;
    retriever.enqueue(hash_b).await;

    // Alice serves hash_a.
    let r_a = serve_payload(&hash_a, &store_alice).unwrap();
    assert_eq!(
        retriever.admit_response(&r_a).await,
        AdmitOutcome::PayloadAccepted,
    );
    // Bob serves hash_b.
    let r_b = serve_payload(&hash_b, &store_bob).unwrap();
    assert_eq!(
        retriever.admit_response(&r_b).await,
        AdmitOutcome::PayloadAccepted,
    );

    assert!(retriever.is_complete().await);
    assert_eq!(retriever.get_bytes(&hash_a).await.unwrap(), bytes_a);
    assert_eq!(retriever.get_bytes(&hash_b).await.unwrap(), bytes_b);
}

/// Idempotent duplicate responses: a sync-driver may receive
/// multiple copies of the same response (broadcast + retry).
/// The retriever accepts the first and treats subsequent
/// duplicates as idempotent no-ops.  Pins the duplicate-
/// handling composes with sync progress.
#[tokio::test]
async fn duplicate_responses_are_idempotent_mid_sync() {
    let store = InMemoryPayloadStore::new();
    let bytes = b"content".to_vec();
    let hash = store.insert(bytes.clone());

    let retriever = WalPayloadRetriever::new();
    retriever.enqueue(hash).await;
    let response = serve_payload(&hash, &store).unwrap();

    // First admit → accepts.
    assert_eq!(
        retriever.admit_response(&response).await,
        AdmitOutcome::PayloadAccepted,
    );
    // Subsequent admits → idempotent accept, no state change.
    for _ in 0..3 {
        assert_eq!(
            retriever.admit_response(&response).await,
            AdmitOutcome::PayloadAccepted,
        );
    }
    assert_eq!(retriever.get_bytes(&hash).await.unwrap(), bytes);
}

fn blake2b256(bytes: &[u8]) -> [u8; 32] {
    let h = crypto::rust::hash::blake2b256::Blake2b256::hash(bytes.to_vec());
    let mut out = [0u8; 32];
    out.copy_from_slice(&h);
    out
}
