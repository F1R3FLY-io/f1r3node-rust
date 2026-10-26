// Phase 7b-2 WAL payload serving handler.
//
// Server-side counterpart of the (yet-to-land) `WalPayloadRetriever`
// (sibling in this slice).  Looks up payload bytes by their
// Blake2b256 hash in a pluggable backing store and builds a
// [`WalPayloadResponse`].
//
// The backing store is trait-abstracted ([`PayloadLookup`]) so
// this module doesn't couple the network path to any particular
// storage implementation.  Reference impl in this slice:
//
//   * [`InMemoryPayloadStore`] — a simple
//     `HashMap<[u8; 32], Vec<u8>>` wrapped in an `RwLock`.  Used
//     for tests and for early integration where the payload cache
//     is transient (e.g., a serving validator that keeps recent
//     write payloads in RAM for a bounded window before eviction).
//
// Future slices will add a `DirectoryPayloadStore` (operator-
// provisioned content case) and a `BlockStorageBackedRecorder`
// (DD-7b-2 Option 2 payload-source index); both depend on
// casper-level block-storage machinery that lives outside the
// Wave 3 scope.
//
// Callers: the wire-message handler (yet-to-land sync-driver
// slice) routes an incoming `GetWalPayloadRequest` here after
// verifying the requesting peer.  A successful [`serve_payload`]
// returns the response proto to send back; error paths let the
// caller decide whether to reply with a NoPayloadAvailable-style
// signal or stay silent.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crypto::rust::hash::blake2b256::Blake2b256;
use models::rust::casper::protocol::casper_message::{HasWalPayload, WalPayloadResponse};
use prost::bytes::Bytes;
use tracing::debug;

/// Errors from [`serve_payload`].
#[derive(Debug, PartialEq, Eq)]
pub enum ServeError {
    /// This node has no bytes cached for the requested payload
    /// hash.  Cheap "no thanks"; peer should ask someone else.
    UnknownPayload,
    /// The backing store returned bytes but they don't hash to
    /// the requested `payload_hash`.  Log at warn — either the
    /// store is corrupted or a caller stashed bytes under the
    /// wrong key.  Better to refuse than send bytes that won't
    /// verify.
    PayloadHashMismatch,
    /// Backing store I/O failed (e.g., disk read error).  Rare;
    /// operator should investigate.
    BackingStoreFailed(String),
}

/// Trait for a payload-hash → bytes lookup.  Every impl is
/// content-addressed: `get(h)` returns the bytes iff
/// `h == Blake2b256(bytes)`.  Implementations SHOULD NOT verify
/// the hash themselves ([`serve_payload`] does that as a
/// defense-in-depth check on every call).
pub trait PayloadLookup: Send + Sync {
    /// Return the bytes for `payload_hash`, or `None` if unknown.
    fn get(&self, payload_hash: &[u8; 32]) -> Result<Option<Vec<u8>>, String>;
}

/// Serve a single payload from the backing store.
///
/// `payload_hash` must be 32 bytes; anything else returns
/// [`ServeError::UnknownPayload`] (the byzantine caller has
/// malformed the request).  On success, produces a
/// [`WalPayloadResponse`] that the requester will re-verify by
/// rehashing against the ORIGINAL request's hash (per the proto
/// ADVISORY-echo docstring).
///
/// # Defense in depth
///
/// Rehashes the retrieved bytes and confirms they match the
/// requested hash.  A backing-store bug that returned wrong
/// bytes would otherwise result in the joiner rejecting our
/// response — better to detect + refuse locally and avoid
/// wasting bandwidth.
pub fn serve_payload<L: PayloadLookup + ?Sized>(
    payload_hash: &[u8],
    lookup: &L,
) -> Result<WalPayloadResponse, ServeError> {
    let hash = match slice_to_hash(payload_hash) {
        Some(h) => h,
        None => return Err(ServeError::UnknownPayload),
    };
    let bytes = match lookup.get(&hash) {
        Ok(Some(b)) => b,
        Ok(None) => return Err(ServeError::UnknownPayload),
        Err(e) => return Err(ServeError::BackingStoreFailed(e)),
    };
    let actual = hash_bytes(&bytes);
    if actual != hash {
        debug!(
            target: "f1r3fly.casper.wal_payload_server",
            requested = hex::encode(hash),
            actual = hex::encode(actual),
            "backing store returned bytes that don't hash to the requested key"
        );
        return Err(ServeError::PayloadHashMismatch);
    }
    Ok(WalPayloadResponse {
        payload_hash: Bytes::copy_from_slice(&hash),
        payload_bytes: Bytes::from(bytes),
    })
}

/// Build a [`HasWalPayload`] announcement for a payload hash we
/// can serve.  Callers use this to reply to a broadcast
/// `HasWalPayloadRequest`.  Returns [`ServeError::UnknownPayload`]
/// if the hash is unknown.
///
/// # Symmetric defense-in-depth with [`serve_payload`]
///
/// Fetches the bytes to populate `payload_size` AND verifies
/// `Blake2b256(bytes) == requested_hash`.  A server MUST NOT
/// announce availability of a payload it can't actually serve.
/// If the backing store has corrupt bytes keyed under the
/// requested hash, a subsequent [`serve_payload`] would reject —
/// announcing in that case would cost the joiner a wasted round-
/// trip plus a peer-switch.  Catching here closes the loop
/// earlier.
pub fn has_wal_payload_announcement<L: PayloadLookup + ?Sized>(
    payload_hash: &[u8],
    lookup: &L,
) -> Result<HasWalPayload, ServeError> {
    let hash = match slice_to_hash(payload_hash) {
        Some(h) => h,
        None => return Err(ServeError::UnknownPayload),
    };
    let bytes = match lookup.get(&hash) {
        Ok(Some(b)) => b,
        Ok(None) => return Err(ServeError::UnknownPayload),
        Err(e) => return Err(ServeError::BackingStoreFailed(e)),
    };
    let actual = hash_bytes(&bytes);
    if actual != hash {
        return Err(ServeError::PayloadHashMismatch);
    }
    Ok(HasWalPayload {
        payload_hash: Bytes::copy_from_slice(&hash),
        payload_size: bytes.len() as u32,
    })
}

/// In-memory reference implementation of [`PayloadLookup`].
/// Fits tests and any serving path that keeps recent write
/// payloads in RAM.  Bytes MUST be content-addressed —
/// [`InMemoryPayloadStore::insert`] computes the hash and uses
/// that as the key.
///
/// # Lock choice
///
/// Uses `std::sync::RwLock` rather than `tokio::sync::RwLock`.
/// Rationale: [`PayloadLookup::get`] is a synchronous trait
/// method called from within the async wire handler; using
/// `tokio::sync::RwLock::blocking_read` from inside a tokio
/// runtime blocks the executor thread (documented tokio
/// footgun).  A std lock is fine here because we never hold the
/// guard across an `.await` — every access is a scoped read or
/// write within a single function.
#[derive(Debug, Clone, Default)]
pub struct InMemoryPayloadStore {
    map: Arc<RwLock<HashMap<[u8; 32], Vec<u8>>>>,
}

impl InMemoryPayloadStore {
    pub fn new() -> Self { Self::default() }

    /// Content-addressed insert: computes `Blake2b256(bytes)`
    /// and stores under that key.  Returns the computed hash so
    /// callers can echo it into a WAL entry.
    pub fn insert(&self, bytes: Vec<u8>) -> [u8; 32] {
        let h = hash_bytes(&bytes);
        self.map
            .write()
            .expect("payload store lock poisoned")
            .insert(h, bytes);
        h
    }

    /// Insert with a caller-supplied hash.  Debug-asserts that
    /// the hash matches the bytes.  Panics on mismatch in debug
    /// builds (a programming error); silently accepts in release
    /// (the [`serve_payload`] rehash check would still catch
    /// it).
    pub fn insert_with_hash(&self, hash: [u8; 32], bytes: Vec<u8>) {
        debug_assert_eq!(hash, hash_bytes(&bytes), "hash/bytes mismatch");
        self.map
            .write()
            .expect("payload store lock poisoned")
            .insert(hash, bytes);
    }

    /// Number of entries stored.
    pub fn len(&self) -> usize { self.map.read().expect("payload store lock poisoned").len() }

    pub fn is_empty(&self) -> bool {
        self.map
            .read()
            .expect("payload store lock poisoned")
            .is_empty()
    }
}

impl PayloadLookup for InMemoryPayloadStore {
    fn get(&self, payload_hash: &[u8; 32]) -> Result<Option<Vec<u8>>, String> {
        let g = self.map.read().map_err(|e| format!("lock poisoned: {e}"))?;
        Ok(g.get(payload_hash).cloned())
    }
}

/// Bridges the casper-crate payload store to the rholang-crate
/// WAL journaling path.  A leader validator's `journal_write`
/// calls [`persist`] after computing `PayloadRef::hash(bytes)` so
/// the bytes are stashed content-addressed for later peer
/// fetches.
impl rholang::rust::interpreter::io::wal::PayloadPersistence for InMemoryPayloadStore {
    fn persist(&self, bytes: &[u8]) -> Result<[u8; 32], String> { Ok(self.insert(bytes.to_vec())) }
}

fn hash_bytes(bytes: &[u8]) -> [u8; 32] {
    let h = Blake2b256::hash(bytes.to_vec());
    assert_eq!(h.len(), 32, "Blake2b256 must produce 32-byte digest");
    let mut out = [0u8; 32];
    out.copy_from_slice(&h);
    out
}

fn slice_to_hash(slice: &[u8]) -> Option<[u8; 32]> {
    if slice.len() != 32 {
        return None;
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(slice);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_memory_insert_round_trips_through_lookup() {
        let store = InMemoryPayloadStore::new();
        let bytes = b"hello world".to_vec();
        let hash = store.insert(bytes.clone());
        let fetched = store.get(&hash).unwrap().expect("round trip");
        assert_eq!(fetched, bytes);
    }

    #[test]
    fn in_memory_lookup_returns_none_for_unknown_hash() {
        let store = InMemoryPayloadStore::new();
        let result = store.get(&[0xFF; 32]).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn serve_payload_happy_path() {
        let store = InMemoryPayloadStore::new();
        let bytes = b"payload bytes".to_vec();
        let hash = store.insert(bytes.clone());
        let response = serve_payload(&hash, &store).expect("serve_payload");
        assert_eq!(response.payload_hash.as_ref(), hash.as_slice());
        assert_eq!(response.payload_bytes.as_ref(), bytes.as_slice());
    }

    #[test]
    fn serve_payload_unknown_returns_unknown_payload() {
        let store = InMemoryPayloadStore::new();
        let err = serve_payload(&[0xEE; 32], &store).expect_err("must fail");
        assert_eq!(err, ServeError::UnknownPayload);
    }

    #[test]
    fn serve_payload_non_32_byte_hash_returns_unknown_payload() {
        let store = InMemoryPayloadStore::new();
        let err = serve_payload(&[0x11; 16], &store).expect_err("must fail");
        assert_eq!(err, ServeError::UnknownPayload);
    }

    /// LOAD-BEARING defense-in-depth: a backing-store bug (bytes
    /// stashed under a wrong key via `insert_with_hash` release-
    /// mode silent accept) is caught by `serve_payload`'s
    /// defensive rehash BEFORE the response is sent.
    #[test]
    fn serve_payload_rejects_hash_mismatch_from_backing_store() {
        let store = InMemoryPayloadStore::new();
        // Stash bytes under a wrong key, bypassing the usual
        // content-addressed insert.
        let bogus_hash = [0xDD; 32];
        store
            .map
            .write()
            .unwrap()
            .insert(bogus_hash, b"different bytes".to_vec());
        let err = serve_payload(&bogus_hash, &store).expect_err("must fail");
        assert_eq!(err, ServeError::PayloadHashMismatch);
    }

    #[test]
    fn has_wal_payload_announcement_returns_shape() {
        let store = InMemoryPayloadStore::new();
        let bytes = vec![0u8; 1024];
        let hash = store.insert(bytes.clone());
        let announcement = has_wal_payload_announcement(&hash, &store).expect("announcement");
        assert_eq!(announcement.payload_hash.as_ref(), hash.as_slice());
        assert_eq!(announcement.payload_size as usize, bytes.len());
    }

    #[test]
    fn has_wal_payload_announcement_unknown_returns_unknown_payload() {
        let store = InMemoryPayloadStore::new();
        let err = has_wal_payload_announcement(&[0xCC; 32], &store).expect_err("must fail");
        assert_eq!(err, ServeError::UnknownPayload);
    }

    /// LOAD-BEARING: `has_wal_payload_announcement` performs the
    /// same defensive rehash as `serve_payload`.  Server MUST
    /// NOT announce availability of a payload it can't actually
    /// serve.
    #[test]
    fn has_wal_payload_announcement_rejects_hash_mismatch() {
        let store = InMemoryPayloadStore::new();
        let bogus_hash = [0xDD; 32];
        store
            .map
            .write()
            .unwrap()
            .insert(bogus_hash, b"different bytes".to_vec());
        let err = has_wal_payload_announcement(&bogus_hash, &store).expect_err("must fail");
        assert_eq!(err, ServeError::PayloadHashMismatch);
    }

    /// Backing store I/O errors (lock poison, future disk store
    /// errors) propagate as [`ServeError::BackingStoreFailed`].
    #[test]
    fn serve_payload_propagates_backing_store_errors() {
        struct AlwaysFails;
        impl PayloadLookup for AlwaysFails {
            fn get(&self, _: &[u8; 32]) -> Result<Option<Vec<u8>>, String> {
                Err("disk read failed".to_string())
            }
        }
        let err = serve_payload(&[0xAB; 32], &AlwaysFails).expect_err("must fail");
        assert!(matches!(err, ServeError::BackingStoreFailed(_)));
    }
}
