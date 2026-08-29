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

    /// DD-7b-2 (a) Option 2 (2026-08-29): return a cloned snapshot
    /// of the current hash → bytes map.  Used by the deploy-write
    /// reproduction helper (`capture_consensus_writes_by_replaying_deploy`)
    /// to drain everything a scratch replay wrote into the store.
    /// Cheap for the small maps this helper produces (a handful of
    /// Consensus writes per deploy); not intended for large-store
    /// enumeration on the production serving path.
    pub fn snapshot(&self) -> HashMap<[u8; 32], Vec<u8>> {
        self.map
            .read()
            .expect("payload store lock poisoned")
            .clone()
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
///
/// # Security posture (Phase 7b-2 review, 2026-08-27)
///
/// **Path traversal:** `path_for(hash)` joins `hex(hash)` which is
/// `[0-9a-f]{64}` only — no separator characters, cannot escape
/// `self.dir`.
///
/// **Symlink races:** an attacker with write access to `self.dir`
/// could plant a symlink to redirect writes elsewhere, but such an
/// attacker already owns the node's data directory (via the
/// broader `<data-dir>` control implied by the setup.rs boot
/// pipeline).  Pre-existing environmental assumption.
///
/// **Concurrent same-hash writes:** two deploys writing identical
/// bytes on Consensus caps produce the same hash → same file path
/// → interleaved `std::fs::write` calls with byte-identical
/// content.  A concurrent reader mid-write could see partial
/// bytes, but the joiner-side re-hash check (`serve_payload`
/// docstring in this file) rejects partial content, so the reader
/// just asks another peer.  No correctness bug.
///
/// **Sync IO in an async caller:** `insert(bytes)` calls
/// `std::fs::write` which blocks the caller thread for the
/// duration of the write.  Callers are typically async fs
/// handlers on a multi-threaded tokio runtime — a large write
/// (up to `MAX_PAYLOAD_BYTES = 64 MiB`) blocks a worker for
/// potentially hundreds of milliseconds on slow disk.  Consistent
/// with the existing fs-handler pattern (they use `nix::unistd::write`
/// synchronously); a future async migration of the whole fs
/// stack would move this behind `spawn_blocking`.
///
/// **Unbounded disk growth:** `insert` has no retention policy.
/// A Consensus deploy writing MAX_WAL_ENTRIES (65,536) × maximum
/// payload (64 MiB) can produce ~4 TiB of on-disk cache per
/// runtime lifetime.  Per-deploy cost accounting bounds this in
/// practice (any such deploy would exhaust the block's REV
/// budget).  DD-7b-1(y) retention (one snapshot cycle behind
/// earliest retained snapshot) is a follow-up task; until then,
/// operators should monitor `<data-dir>/wal_payload_store/` size.
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

/// Phase 7b-2 (2026-08-27): the serving-side write path.  A leader
/// validator's `journal_write` calls this after computing
/// `PayloadRef::hash(bytes)` so the bytes are stashed content-
/// addressed on disk.  Implemented on top of the existing `insert`
/// method — the trait method exists to bridge the rholang crate
/// (which owns the fs-write handlers) and the casper crate
/// (which owns the payload store) without introducing a circular
/// dependency.
impl rholang::rust::interpreter::io::wal::PayloadPersistence for DirectoryPayloadStore {
    fn persist(&self, bytes: &[u8]) -> Result<[u8; 32], String> {
        self.insert(bytes)
    }
}

/// Phase 7b-2 (2026-08-27): same trait impl for the in-memory
/// store, so tests can wire an in-process persistence backend
/// without touching disk.
impl rholang::rust::interpreter::io::wal::PayloadPersistence for InMemoryPayloadStore {
    fn persist(&self, bytes: &[u8]) -> Result<[u8; 32], String> {
        Ok(self.insert(bytes.to_vec()))
    }
}

/// DD-7b-2 (a) Option 2 (2026-08-29): block-storage-backed
/// implementation of `PayloadSourceRecorder`.  Wraps the
/// `BlockDagKeyValueStorage` handle (co-located with the DAG's
/// existing `deploy_index`) and forwards `record` calls to
/// `record_payload_source`.
///
/// Wired into every runtime's `FileHandleTable::payload_source_recorder`
/// via `RuntimeManager::set_payload_source_recorder` at boot;
/// symmetric on leader and follower so any node whose block
/// processing succeeded can serve the Option 2 tier at boot to a
/// later joiner.
///
/// # Debug output
///
/// Custom Debug impl avoids leaking the underlying
/// `BlockDagKeyValueStorage`'s (typically large) internal state
/// through recorder-diagnostic logs.
#[derive(Clone)]
pub struct BlockStorageBackedRecorder {
    /// `BlockDagKeyValueStorage` is itself `Clone` (all internal
    /// state is Arc-shared under `PlRwLock`), so we hold it by
    /// value and lean on that shape rather than adding an extra
    /// `Arc` indirection.
    storage: block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage,
}

impl BlockStorageBackedRecorder {
    pub fn new(
        storage: block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage,
    ) -> Self {
        Self { storage }
    }
}

impl std::fmt::Debug for BlockStorageBackedRecorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlockStorageBackedRecorder")
            .finish_non_exhaustive()
    }
}

impl rholang::rust::interpreter::io::wal::PayloadSourceRecorder for BlockStorageBackedRecorder {
    fn record(&self, payload_hash: [u8; 32], deploy_sig: &[u8]) -> Result<(), String> {
        self.storage
            .record_payload_source(payload_hash, deploy_sig)
            .map_err(|e| format!("{e}"))
    }
}

/// Phase 7b-2 (2026-08-27): a bundled handle to a payload store
/// that lets the same underlying bytes be reached through TWO
/// trait objects — `PayloadPersistence` (the write path, called
/// from the interpreter's `journal_write`) and `PayloadLookup`
/// (the read path, called from the wire-message dispatch).
///
/// The bundle exists because Rust's trait-object system can't
/// automatically coerce `Arc<dyn PayloadPersistence>` into
/// `Arc<dyn PayloadLookup>` even when the concrete type
/// implements both, and the two traits live in different crates
/// (PayloadPersistence in rholang, PayloadLookup in casper) so
/// they can't share a supertrait.  Construction sites clone one
/// concrete `Arc<T>` twice and coerce each clone to the
/// appropriate trait object.
#[derive(Clone)]
pub struct PayloadStoreBundle {
    /// Write-side handle used by the interpreter's fs-write
    /// handlers via `FileHandleTable::payload_store`.
    pub persistence: Arc<dyn rholang::rust::interpreter::io::wal::PayloadPersistence>,
    /// Read-side handle used by the wire dispatch's
    /// `serve_payload` / `has_wal_payload_announcement` via
    /// `WalPayloadContext::payload_lookup`.
    pub lookup: Arc<dyn PayloadLookup>,
}

impl std::fmt::Debug for PayloadStoreBundle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PayloadStoreBundle").finish_non_exhaustive()
    }
}

impl PayloadStoreBundle {
    /// Build a bundle from a `DirectoryPayloadStore` (the boot
    /// pipeline's normal path).  Both trait objects point at the
    /// same underlying directory.
    pub fn from_directory(store: DirectoryPayloadStore) -> Self {
        let arc = Arc::new(store);
        Self {
            persistence: arc.clone() as Arc<dyn rholang::rust::interpreter::io::wal::PayloadPersistence>,
            lookup: arc as Arc<dyn PayloadLookup>,
        }
    }

    /// Build a bundle from an in-memory store (test / dev-mode
    /// path).  Both trait objects point at the same underlying
    /// `HashMap` guarded by a std `RwLock`.
    pub fn from_in_memory(store: InMemoryPayloadStore) -> Self {
        let arc = Arc::new(store);
        Self {
            persistence: arc.clone() as Arc<dyn rholang::rust::interpreter::io::wal::PayloadPersistence>,
            lookup: arc as Arc<dyn PayloadLookup>,
        }
    }
}

/// Phase 7b-2 retention (DD-7b-1 (y), 2026-08-27): delete any
/// content-addressed payload files in `payload_dir` whose hex-
/// hash filename is NOT in `keep`.  Files whose name does not
/// decode as a 64-char hex string are left untouched (defensive:
/// operators may have leftover tmp files, symlinks, README
/// snippets, etc.).  Symlinks are skipped for the same reason
/// `prune_snapshot_dir` skips them — attacker-planted symlinks to
/// unrelated targets should not get followed.
///
/// The `keep` set typically comes from
/// `rholang::rust::interpreter::io::snapshot::scan_retained_payload_hashes`
/// which unions the hashes sidecars across all retained
/// snapshots.
///
/// Returns the number of files removed.  Individual `remove_file`
/// failures are logged, not propagated — retention is bounded by
/// future passes anyway.
pub fn prune_payload_store(
    payload_dir: &Path,
    keep: &std::collections::HashSet<[u8; 32]>,
) -> std::io::Result<usize> {
    let read_dir = match std::fs::read_dir(payload_dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e),
    };
    let mut removed = 0;
    for entry in read_dir.flatten() {
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        // Skip symlinks + non-regular entries (dirs, sockets, etc.).
        if file_type.is_symlink() || file_type.is_dir() {
            continue;
        }
        let name = match path.file_name().and_then(|s| s.to_str()) {
            Some(n) => n,
            None => continue,
        };
        // Content-addressed filenames are exactly 64 hex chars.
        // Anything else is operator ephemera; leave alone.
        if name.len() != 64 || !name.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        let hash = match hex::decode(name) {
            Ok(v) if v.len() == 32 => {
                let mut buf = [0u8; 32];
                buf.copy_from_slice(&v);
                buf
            }
            _ => continue,
        };
        if keep.contains(&hash) {
            continue;
        }
        match std::fs::remove_file(&path) {
            Ok(()) => removed += 1,
            Err(e) => tracing::warn!(
                target: "f1r3fly.fs_wal.payload_store",
                path = %path.display(),
                error = %e,
                "prune_payload_store: failed to remove non-retained payload; continuing"
            ),
        }
    }
    Ok(removed)
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

    // -----------------------------------------------------------
    // Phase 7b-2 retention (DD-7b-1 (y), 2026-08-27) tests.
    // -----------------------------------------------------------

    /// `prune_payload_store` deletes payload files whose
    /// hex-hash filename is not in the keep set.
    #[test]
    fn prune_payload_store_removes_non_retained() {
        let dir = tempfile::tempdir().unwrap();
        let store = DirectoryPayloadStore::new(dir.path().to_path_buf());
        let keep_bytes = b"keep this".to_vec();
        let drop_bytes = b"drop this".to_vec();
        let keep_h = store.insert(&keep_bytes).unwrap();
        let drop_h = store.insert(&drop_bytes).unwrap();
        let mut keep_set = std::collections::HashSet::new();
        keep_set.insert(keep_h);
        let removed = prune_payload_store(dir.path(), &keep_set).unwrap();
        assert_eq!(removed, 1);
        // Keep still there; drop is gone.
        assert!(store.get(&keep_h).unwrap().is_some());
        assert!(store.get(&drop_h).unwrap().is_none());
    }

    /// `prune_payload_store` leaves non-hex-named files alone —
    /// operators may drop READMEs or `.gitkeep` in the dir; they
    /// must not disappear on retention.
    #[test]
    fn prune_payload_store_ignores_non_hex_named_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README"), b"docs").unwrap();
        std::fs::write(dir.path().join("some-junk.txt"), b"junk").unwrap();
        // A 64-char filename that's NOT valid hex (contains 'g').
        let bad_hex = "g".repeat(64);
        std::fs::write(dir.path().join(&bad_hex), b"not hex").unwrap();
        // Empty keep set → prune EVERY hex-64 file.  None of the
        // above should qualify.
        let empty: std::collections::HashSet<[u8; 32]> = std::collections::HashSet::new();
        let removed = prune_payload_store(dir.path(), &empty).unwrap();
        assert_eq!(removed, 0);
        assert!(dir.path().join("README").exists());
        assert!(dir.path().join("some-junk.txt").exists());
        assert!(dir.path().join(&bad_hex).exists());
    }

    /// `prune_payload_store` on a non-existent dir returns Ok(0)
    /// so a boot-time retention pass before the payload dir has
    /// been created is a graceful no-op.
    #[test]
    fn prune_payload_store_missing_dir_is_ok_zero() {
        let missing = std::path::Path::new("/tmp/does-not-exist-payload-prune");
        let empty: std::collections::HashSet<[u8; 32]> = std::collections::HashSet::new();
        assert_eq!(prune_payload_store(missing, &empty).unwrap(), 0);
    }

    /// `prune_payload_store` skips symlinks — defense in depth
    /// against an attacker planting `<hex(hash)> -> /etc/passwd`
    /// in the payload dir.  We do NOT follow such links to
    /// unlink; the whole file gets left alone.
    #[cfg(unix)]
    #[test]
    fn prune_payload_store_skips_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let store = DirectoryPayloadStore::new(dir.path().to_path_buf());
        let payload = b"real".to_vec();
        let h_real = store.insert(&payload).unwrap();
        // Create a symlink at another hex-64 hash name pointing
        // at the real file.  If prune followed the link + unlinked,
        // the target would vanish and we'd break the payload store.
        let fake_hash_name = "0".repeat(64);
        let symlink_path = dir.path().join(&fake_hash_name);
        std::os::unix::fs::symlink(store.path_for(&h_real), &symlink_path).unwrap();
        // Empty keep set → prune every regular hex-64 file.
        // The symlink hash name is hex-64, but symlink filter
        // skips it.  The real file's hex name is NOT in keep so
        // it gets removed.
        let empty: std::collections::HashSet<[u8; 32]> = std::collections::HashSet::new();
        let _ = prune_payload_store(dir.path(), &empty).unwrap();
        // Symlink itself still present.
        let meta = std::fs::symlink_metadata(&symlink_path).unwrap();
        assert!(meta.file_type().is_symlink());
    }

    /// DD-7b-2 (a) Option 2 primitive pin (2026-08-29):
    /// `InMemoryPayloadStore::snapshot()` returns a cloned view of
    /// every inserted (hash, bytes) pair.  The
    /// `capture_consensus_writes_by_replaying_deploy` helper drains
    /// its capturing store via this method — a missing entry here
    /// (or a mutation-aliased return) would silently drop write
    /// bytes the helper is supposed to reproduce.
    #[test]
    fn in_memory_payload_store_snapshot_returns_all_inserted_entries() {
        let store = InMemoryPayloadStore::new();
        let a = b"first".to_vec();
        let b = b"second".to_vec();
        let c = b"third".to_vec();
        let ha = store.insert(a.clone());
        let hb = store.insert(b.clone());
        let hc = store.insert(c.clone());

        let snap = store.snapshot();
        assert_eq!(snap.len(), 3);
        assert_eq!(snap.get(&ha), Some(&a));
        assert_eq!(snap.get(&hb), Some(&b));
        assert_eq!(snap.get(&hc), Some(&c));

        // Snapshot is a clone: mutating it doesn't affect the
        // store, and mutating the store after doesn't affect the
        // returned snapshot.  Guards against a future refactor
        // returning a shared reference that leaks store internals.
        let mut mutated = snap.clone();
        mutated.remove(&ha);
        assert_eq!(store.snapshot().len(), 3, "store must be unaffected");
        let d = b"fourth".to_vec();
        let _ = store.insert(d.clone());
        assert_eq!(snap.len(), 3, "prior snapshot must be a stable clone");
    }

    /// DD-7b-2 (a) Option 2 primitive pin (2026-08-29): empty store
    /// snapshot is empty (not None, not a placeholder).  The
    /// deploy-write reproduction helper returns this shape for
    /// deploys that did no Consensus writes; callers must
    /// distinguish "no writes" from "helper failed" via the
    /// `Result` variant, not by inspecting the map.
    #[test]
    fn in_memory_payload_store_snapshot_is_empty_when_store_is_empty() {
        let store = InMemoryPayloadStore::new();
        let snap = store.snapshot();
        assert!(snap.is_empty(), "expected empty map, got {snap:?}");
    }

    // ---------------------------------------------------------------
    // DD-7b-2 (a) Option 2 (2026-08-29): behavioral tests for
    // `BlockStorageBackedRecorder`.  Complement the shape-scan
    // pin `journal_write_records_payload_source_on_consensus_writes`
    // by exercising the actual trait-through-storage path.
    // ---------------------------------------------------------------

    async fn make_dag_storage(
    ) -> block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage {
        use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
        let mut kvm = InMemoryStoreManager::new();
        block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage::new(&mut kvm)
            .await
            .expect("in-memory DAG storage")
    }

    /// `BlockStorageBackedRecorder::record` writes through to the
    /// underlying `BlockDagKeyValueStorage`'s `payload_source_index`
    /// so a subsequent `lookup_payload_source` returns the recorded
    /// sig.  A refactor that made `record` a no-op (e.g., forgot to
    /// call `record_payload_source`) would trip here.
    #[tokio::test]
    async fn block_storage_backed_recorder_writes_through_to_index() {
        use rholang::rust::interpreter::io::wal::PayloadSourceRecorder;
        let storage = make_dag_storage().await;
        let recorder = BlockStorageBackedRecorder::new(storage.clone());

        let payload_hash = [0x33u8; 32];
        let deploy_sig: Vec<u8> = vec![0x77, 0x88, 0x99];
        recorder
            .record(payload_hash, &deploy_sig)
            .expect("record must succeed on fresh storage");

        let got = storage
            .lookup_payload_source(&payload_hash)
            .expect("lookup must not error")
            .expect("recorder.record must have written");
        assert_eq!(got, deploy_sig);
    }

    /// The recorder's `Arc<dyn PayloadSourceRecorder>` trait object
    /// preserves the write-through path.  Confirms the coercion in
    /// `RuntimeManager::spawn_runtime` /
    /// `FileHandleTable::share_payload_source_recorder` doesn't
    /// insert any wrapper that swallows writes.  A regression that
    /// wrapped the recorder in a NoOp adapter (e.g., during a
    /// feature-flag rollback) would fire here.
    #[tokio::test]
    async fn recorder_write_through_survives_arc_dyn_erasure() {
        use rholang::rust::interpreter::io::wal::PayloadSourceRecorder;
        let storage = make_dag_storage().await;
        let recorder: std::sync::Arc<dyn PayloadSourceRecorder> = std::sync::Arc::new(
            BlockStorageBackedRecorder::new(storage.clone()),
        );

        let payload_hash = [0x44u8; 32];
        let deploy_sig: Vec<u8> = vec![0xAA, 0xBB];
        recorder
            .record(payload_hash, &deploy_sig)
            .expect("record via Arc<dyn PayloadSourceRecorder> must succeed");

        let got = storage
            .lookup_payload_source(&payload_hash)
            .expect("lookup")
            .expect("recorder must have written through the trait object");
        assert_eq!(got, deploy_sig);
    }
}

