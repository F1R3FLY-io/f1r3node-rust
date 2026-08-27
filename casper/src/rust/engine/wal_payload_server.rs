// Phase 7b-2 WAL payload serving handler (2026-08-27).
//
// Server-side counterpart of `WalPayloadRetriever`.  Looks up
// payload bytes by their Blake2b256 hash in a pluggable backing
// store and builds a `WalPayloadResponse`.
//
// The backing store is trait-abstracted (`PayloadLookup`) so this
// module doesn't couple the network path to any particular
// storage implementation.  Reference impls in this file:
//
//   * `InMemoryPayloadStore` — a simple `HashMap<[u8; 32], Vec<u8>>`
//     wrapped in an `RwLock`.  Used for tests and for early
//     integration where the payload cache is transient (e.g., a
//     serving validator that keeps recent write payloads in RAM
//     for a bounded window before eviction).
//   * `DirectoryPayloadStore` — reads bytes from
//     `<dir>/<hex(hash)>` files.  Fits the "operator-provisioned
//     content" case (Phase 7b-2 write-payload determinism reducer)
//     and matches how the snapshot dir is laid out.
//
// Callers: the wire-message handler routes an incoming
// `GetWalPayloadRequest` here after verifying the requesting peer.
// A successful `serve_payload` returns the response proto to send
// back; error paths let the caller decide whether to reply with a
// NoPayloadAvailable-style signal or stay silent.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use crypto::rust::hash::blake2b256::Blake2b256;
use models::rust::casper::protocol::casper_message::{HasWalPayload, WalPayloadResponse};
use prost::bytes::Bytes;
use tracing::debug;

/// Errors from `serve_payload`.
#[derive(Debug)]
pub enum ServeError {
    /// This node has no bytes cached for the requested payload
    /// hash.  Cheap "no thanks"; peer should ask someone else.
    UnknownPayload,
    /// The backing store returned bytes but they don't hash to the
    /// requested payload_hash.  Log at warn — either the store is
    /// corrupted or a caller stashed bytes under the wrong key.
    /// Better to refuse than to send bytes that won't verify.
    PayloadHashMismatch,
    /// Backing store I/O failed (e.g., disk read error).  Rare;
    /// operator should investigate.
    BackingStoreFailed(String),
}

/// Trait for a payload-hash → bytes lookup.  Every impl is
/// content-addressed: `get(h)` returns the bytes iff `h ==
/// Blake2b256(bytes)`.  Implementations SHOULD NOT verify the hash
/// (`serve_payload` does that as a defense-in-depth check on
/// every call).
pub trait PayloadLookup: Send + Sync {
    /// Return the bytes for `payload_hash`, or None if unknown.
    fn get(&self, payload_hash: &[u8; 32]) -> Result<Option<Vec<u8>>, String>;
}

/// Serve a single payload from the backing store.
///
/// `payload_hash` must be 32 bytes; anything else returns
/// `UnknownPayload` (the byzantine caller has malformed the
/// request).  On success, produces a `WalPayloadResponse` that the
/// requester will re-verify by rehashing.
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
    // Defense-in-depth: rehash the retrieved bytes and confirm
    // they match the requested hash.  A backing-store bug that
    // returned wrong bytes would otherwise result in the joiner
    // rejecting our response — better to detect + refuse locally.
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

/// Build a `HasWalPayload` announcement for a payload hash we can
/// serve.  Callers use this to reply to a broadcast
/// `HasWalPayloadRequest`.  Returns None if the payload is
/// unknown.
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
    Ok(HasWalPayload {
        payload_hash: Bytes::copy_from_slice(&hash),
        payload_size: bytes.len() as u32,
    })
}

/// In-memory reference implementation.  Fits tests and any serving
/// path that keeps recent write payloads in RAM.  Bytes MUST be
/// content-addressed (`insert(bytes)` computes the hash and uses
/// that as the key).
///
/// **Lock choice (review-fix 2026-08-27):** uses `std::sync::RwLock`
/// rather than `tokio::sync::RwLock`.  Rationale: `PayloadLookup::get`
/// is a synchronous trait method called from within the async wire
/// handler; using `tokio::sync::RwLock::blocking_read` from inside
/// a tokio runtime blocks the executor thread (documented tokio
/// footgun).  A std lock is fine here because we never hold the
/// guard across an `.await` — every access is a scoped read or
/// write within a single function.
#[derive(Debug, Clone, Default)]
pub struct InMemoryPayloadStore {
    map: Arc<RwLock<HashMap<[u8; 32], Vec<u8>>>>,
}

impl InMemoryPayloadStore {
    pub fn new() -> Self { Self::default() }

    /// Content-addressed insert: computes `Blake2b256(bytes)` and
    /// stores under that key.  Returns the computed hash so callers
    /// can echo it into a WAL entry.
    pub fn insert(&self, bytes: Vec<u8>) -> [u8; 32] {
        let h = hash_bytes(&bytes);
        self.map.write().expect("payload store lock poisoned").insert(h, bytes);
        h
    }

    /// Insert with a caller-supplied hash.  Debug-asserts that the
    /// hash matches the bytes.  Panics on mismatch in debug builds
    /// (a programming error); silently accepts in release builds
    /// (the `serve_payload` rehash check would still catch it).
    pub fn insert_with_hash(&self, hash: [u8; 32], bytes: Vec<u8>) {
        debug_assert_eq!(hash, hash_bytes(&bytes), "hash/bytes mismatch");
        self.map.write().expect("payload store lock poisoned").insert(hash, bytes);
    }

    /// Number of entries stored.
    pub fn len(&self) -> usize {
        self.map.read().expect("payload store lock poisoned").len()
    }
    pub fn is_empty(&self) -> bool {
        self.map.read().expect("payload store lock poisoned").is_empty()
    }
}

impl PayloadLookup for InMemoryPayloadStore {
    fn get(&self, payload_hash: &[u8; 32]) -> Result<Option<Vec<u8>>, String> {
        let g = self.map.read().map_err(|e| format!("lock poisoned: {e}"))?;
        Ok(g.get(payload_hash).cloned())
    }
}

/// Directory-backed store.  Bytes live under `<dir>/<hex(hash)>`.
/// Matches the on-disk shape of the snapshot dir; fits the
/// operator-provisioned content case cleanly.
#[derive(Debug, Clone)]
pub struct DirectoryPayloadStore {
    dir: PathBuf,
}

impl DirectoryPayloadStore {
    pub fn new(dir: PathBuf) -> Self { Self { dir } }

    fn path_for(&self, hash: &[u8; 32]) -> PathBuf { self.dir.join(hex::encode(hash)) }

    /// Content-addressed write.  Creates the dir if needed.
    pub fn insert(&self, bytes: &[u8]) -> Result<[u8; 32], String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| format!("mkdir {:?}: {e}", self.dir))?;
        let h = hash_bytes(bytes);
        let p = self.path_for(&h);
        std::fs::write(&p, bytes).map_err(|e| format!("write {p:?}: {e}"))?;
        Ok(h)
    }
}

impl PayloadLookup for DirectoryPayloadStore {
    fn get(&self, payload_hash: &[u8; 32]) -> Result<Option<Vec<u8>>, String> {
        let p = self.path_for(payload_hash);
        match std::fs::read(&p) {
            Ok(b) => Ok(Some(b)),
            Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("read {p:?}: {e}")),
        }
    }
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

/// Trap-check that the `Path` argument compiles into public API.
/// Callers pass a `&Path` down as `dir` when constructing
/// DirectoryPayloadStore; keep the alias so a future refactor
/// doesn't accidentally lose the ergonomic constructor shape.
#[allow(dead_code)]
fn _shape_check(p: &Path) -> DirectoryPayloadStore { DirectoryPayloadStore::new(p.to_path_buf()) }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::engine::wal_payload_retriever::{AdmitOutcome, WalPayloadRetriever};

    #[tokio::test]
    async fn in_memory_round_trip_through_retriever() {
        let store = InMemoryPayloadStore::new();
        let payload = b"round-trip payload".to_vec();
        let h = store.insert(payload.clone());

        // Server produces a response.
        let response = tokio::task::spawn_blocking({
            let store = store.clone();
            let h = h;
            move || serve_payload(&h, &store).expect("serve_payload")
        })
        .await
        .unwrap();
        assert_eq!(response.payload_hash.as_ref(), &h);
        assert_eq!(response.payload_bytes.as_ref(), payload.as_slice());

        // Retriever accepts it.
        let retriever = WalPayloadRetriever::new();
        retriever.enqueue(h).await;
        assert_eq!(
            retriever.admit_response(&response).await,
            AdmitOutcome::PayloadAccepted
        );
        assert!(retriever.is_complete().await);
    }

    #[tokio::test]
    async fn serve_payload_unknown_hash() {
        let store = InMemoryPayloadStore::new();
        let bogus = [0xAAu8; 32];
        let store2 = store.clone();
        let err = tokio::task::spawn_blocking(move || serve_payload(&bogus, &store2).unwrap_err())
            .await
            .unwrap();
        assert!(matches!(err, ServeError::UnknownPayload));
    }

    #[tokio::test]
    async fn serve_payload_rejects_malformed_hash() {
        let store = InMemoryPayloadStore::new();
        let short = [0u8; 16];
        let store2 = store.clone();
        let err = tokio::task::spawn_blocking(move || serve_payload(&short, &store2).unwrap_err())
            .await
            .unwrap();
        assert!(matches!(err, ServeError::UnknownPayload));
    }

    #[tokio::test]
    async fn has_wal_payload_announcement_returns_size() {
        let store = InMemoryPayloadStore::new();
        let payload = b"1234567890".to_vec();
        let h = store.insert(payload.clone());
        let store2 = store.clone();
        let announcement = tokio::task::spawn_blocking(move || {
            has_wal_payload_announcement(&h, &store2).expect("announcement")
        })
        .await
        .unwrap();
        assert_eq!(announcement.payload_hash.as_ref(), &h);
        assert_eq!(announcement.payload_size, payload.len() as u32);
    }

    #[test]
    fn directory_store_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let store = DirectoryPayloadStore::new(dir.path().to_path_buf());
        let payload = b"on-disk payload".to_vec();
        let h = store.insert(&payload).expect("insert");
        let got = store.get(&h).unwrap().unwrap();
        assert_eq!(got, payload);
        // Unknown hash returns None.
        assert!(store.get(&[0xFFu8; 32]).unwrap().is_none());
    }

    #[test]
    fn directory_store_detects_tampered_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let store = DirectoryPayloadStore::new(dir.path().to_path_buf());
        let payload = b"pristine".to_vec();
        let h = store.insert(&payload).expect("insert");
        // Overwrite the file with different bytes without updating
        // the filename.  `serve_payload` should catch the mismatch.
        std::fs::write(dir.path().join(hex::encode(h)), b"tampered").unwrap();
        let err = serve_payload(&h, &store).expect_err("mismatch");
        assert!(matches!(err, ServeError::PayloadHashMismatch));
    }

    /// T-7: a `DirectoryPayloadStore` pointed at a directory that
    /// exists but where reads fail (unreadable file) surfaces the
    /// underlying IO error as `ServeError::BackingStoreFailed`
    /// rather than a silent None.  Simulated by chmod'ing a
    /// deposited payload file to unreadable and rehashing.  Only
    /// runs on Unix — file-permission semantics differ on Windows.
    #[cfg(unix)]
    #[test]
    fn directory_store_surfaces_io_errors() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let store = DirectoryPayloadStore::new(dir.path().to_path_buf());
        let payload = b"unreadable".to_vec();
        let h = store.insert(&payload).expect("insert");
        // Make the file unreadable.
        let p = dir.path().join(hex::encode(h));
        let mut perms = std::fs::metadata(&p).unwrap().permissions();
        perms.set_mode(0o000);
        std::fs::set_permissions(&p, perms).unwrap();
        let got = store.get(&h);
        // Restore perms so tempdir cleanup can delete.
        let mut restore = std::fs::metadata(&p).unwrap().permissions();
        restore.set_mode(0o600);
        let _ = std::fs::set_permissions(&p, restore);
        match got {
            Err(e) => assert!(
                e.contains("Permission denied") || e.contains("read"),
                "expected read error, got {e}"
            ),
            Ok(other) => panic!("expected io error, got {other:?}"),
        }
    }

    /// T-7: `serve_payload` surfaces backing-store errors as
    /// `ServeError::BackingStoreFailed` (not swallowed as
    /// UnknownPayload).
    #[test]
    fn serve_payload_surfaces_backing_store_error() {
        struct AlwaysFailStore;
        impl PayloadLookup for AlwaysFailStore {
            fn get(&self, _: &[u8; 32]) -> Result<Option<Vec<u8>>, String> {
                Err("simulated disk failure".to_string())
            }
        }
        let err = serve_payload(&[0u8; 32], &AlwaysFailStore).unwrap_err();
        assert!(matches!(err, ServeError::BackingStoreFailed(msg) if msg.contains("disk")));
    }
}
