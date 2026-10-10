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
use std::path::{Path, PathBuf};
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

/// Directory-backed store.  Bytes live under `<dir>/<hex(hash)>`.
/// Matches the on-disk shape of the snapshot dir; fits the
/// operator-provisioned content case cleanly.
///
/// # Security posture
///
/// **Path traversal:** [`path_for`](Self::path_for) joins
/// `hex(hash)` which is `[0-9a-f]{64}` only — no separator
/// characters, cannot escape `self.dir`.
///
/// **Symlink races:** an attacker with write access to `self.dir`
/// could plant a symlink to redirect writes elsewhere, but such
/// an attacker already owns the node's data directory (via the
/// broader `<data-dir>` control implied by the setup.rs boot
/// pipeline).  Pre-existing environmental assumption.
///
/// **Concurrent same-hash writes:** two deploys writing identical
/// bytes on Consensus caps produce the same hash → same file path
/// → interleaved `std::fs::write` calls with byte-identical
/// content.  A concurrent reader mid-write could see partial
/// bytes, but the joiner-side re-hash check (see
/// [`serve_payload`]) rejects partial content, so the reader just
/// asks another peer.  No correctness bug.
///
/// **Sync IO in an async caller:** [`insert`](Self::insert) calls
/// `std::fs::write` which blocks the caller thread for the
/// duration of the write.  Callers are typically async fs
/// handlers on a multi-threaded tokio runtime — a large write
/// (up to `MAX_PAYLOAD_BYTES = 64 MiB`) blocks a worker for
/// potentially hundreds of milliseconds on slow disk.  Consistent
/// with the existing fs-handler pattern (they use
/// `nix::unistd::write` synchronously); a future async migration
/// of the whole fs stack would move this behind `spawn_blocking`.
///
/// **Unbounded disk growth:** [`insert`](Self::insert) has no
/// retention policy.  A Consensus deploy writing MAX_WAL_ENTRIES
/// (65,536) × maximum payload (64 MiB) can produce ~4 TiB of
/// on-disk cache per runtime lifetime.  Per-deploy cost
/// accounting bounds this in practice (any such deploy would
/// exhaust the block's REV budget).  Snapshot-retention-driven
/// pruning via [`prune_payload_store`] is the operational
/// mitigation; until an automated eviction policy lands, operators
/// should monitor `<data-dir>/wal_payload_store/` size.
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

/// Bridges the casper-crate directory payload store to the
/// rholang-crate WAL journaling path.  A leader validator's
/// `journal_write` calls [`persist`] after computing
/// `PayloadRef::hash(bytes)` so the bytes are stashed
/// content-addressed on disk for later peer fetches.
impl rholang::rust::interpreter::io::wal::PayloadPersistence for DirectoryPayloadStore {
    fn persist(&self, bytes: &[u8]) -> Result<[u8; 32], String> { self.insert(bytes) }
}

/// A bundled handle to a payload store that lets the same
/// underlying bytes be reached through TWO trait objects —
/// [`rholang::rust::interpreter::io::wal::PayloadPersistence`]
/// (the write path, called from the interpreter's `journal_write`)
/// and [`PayloadLookup`] (the read path, called from the
/// wire-message dispatch).
///
/// The bundle exists because Rust's trait-object system can't
/// automatically coerce `Arc<dyn PayloadPersistence>` into
/// `Arc<dyn PayloadLookup>` even when the concrete type
/// implements both, and the two traits live in different crates
/// (`PayloadPersistence` in rholang, `PayloadLookup` in casper)
/// so they can't share a supertrait.  Construction sites clone
/// one concrete `Arc<T>` twice and coerce each clone to the
/// appropriate trait object.
#[derive(Clone)]
pub struct PayloadStoreBundle {
    /// Write-side handle used by the interpreter's fs-write
    /// handlers via `FileHandleTable::payload_store`.
    pub persistence: Arc<dyn rholang::rust::interpreter::io::wal::PayloadPersistence>,
    /// Read-side handle used by the wire dispatch's
    /// [`serve_payload`] / [`has_wal_payload_announcement`] via
    /// the (future) `WalPayloadContext::payload_lookup`.
    pub lookup: Arc<dyn PayloadLookup>,
}

impl std::fmt::Debug for PayloadStoreBundle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PayloadStoreBundle").finish_non_exhaustive()
    }
}

impl PayloadStoreBundle {
    /// Build a bundle from a [`DirectoryPayloadStore`] (the boot
    /// pipeline's normal path).  Both trait objects point at the
    /// same underlying directory.
    pub fn from_directory(store: DirectoryPayloadStore) -> Self {
        let arc = Arc::new(store);
        Self {
            persistence: arc.clone()
                as Arc<dyn rholang::rust::interpreter::io::wal::PayloadPersistence>,
            lookup: arc as Arc<dyn PayloadLookup>,
        }
    }

    /// Build a bundle from an in-memory store (test / dev-mode
    /// path).  Both trait objects point at the same underlying
    /// `HashMap` guarded by a std `RwLock`.
    pub fn from_in_memory(store: InMemoryPayloadStore) -> Self {
        let arc = Arc::new(store);
        Self {
            persistence: arc.clone()
                as Arc<dyn rholang::rust::interpreter::io::wal::PayloadPersistence>,
            lookup: arc as Arc<dyn PayloadLookup>,
        }
    }
}

/// Delete any content-addressed payload files in `payload_dir`
/// whose hex-hash filename is NOT in `keep`.  Files whose name
/// does not decode as a 64-char hex string are left untouched
/// (defensive: operators may have leftover tmp files, symlinks,
/// README snippets, etc.).  Symlinks are skipped for the same
/// reason snapshot-dir pruning skips them — attacker-planted
/// symlinks to unrelated targets should not get followed.
///
/// The `keep` set typically comes from the snapshot layer's
/// retained-payload-hashes enumeration (union of the hashes
/// sidecars across all retained snapshots).
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
        if file_type.is_symlink() || file_type.is_dir() {
            continue;
        }
        let name = match path.file_name().and_then(|s| s.to_str()) {
            Some(n) => n,
            None => continue,
        };
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

/// Trap-check that the `Path` argument compiles into public API.
/// Callers pass a `&Path` down as `dir` when constructing a
/// [`DirectoryPayloadStore`]; keep the alias so a future refactor
/// doesn't accidentally lose the ergonomic constructor shape.
#[allow(dead_code)]
fn _shape_check(p: &Path) -> DirectoryPayloadStore { DirectoryPayloadStore::new(p.to_path_buf()) }

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

    #[test]
    fn directory_store_round_trips_through_lookup() {
        let dir = tempfile::tempdir().unwrap();
        let store = DirectoryPayloadStore::new(dir.path().to_path_buf());
        let bytes = b"disk-backed payload".to_vec();
        let hash = store.insert(&bytes).expect("insert");
        let fetched = store.get(&hash).unwrap().expect("round trip");
        assert_eq!(fetched, bytes);
    }

    #[test]
    fn directory_store_lookup_returns_none_for_unknown_hash() {
        let dir = tempfile::tempdir().unwrap();
        let store = DirectoryPayloadStore::new(dir.path().to_path_buf());
        let result = store.get(&[0xFF; 32]).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn directory_store_persist_hashes_the_bytes() {
        use rholang::rust::interpreter::io::wal::PayloadPersistence;
        let dir = tempfile::tempdir().unwrap();
        let store = DirectoryPayloadStore::new(dir.path().to_path_buf());
        let bytes = b"persisted via trait".to_vec();
        let hash = store.persist(&bytes).expect("persist");
        assert_eq!(hash, Blake2b256::hash(bytes.clone()).as_slice());
        let fetched = store.get(&hash).unwrap().expect("fetched");
        assert_eq!(fetched, bytes);
    }

    #[test]
    fn payload_store_bundle_shares_bytes_across_trait_objects() {
        let in_mem = InMemoryPayloadStore::new();
        let bundle = PayloadStoreBundle::from_in_memory(in_mem);
        let bytes = b"shared through both trait objects".to_vec();
        let hash = bundle.persistence.persist(&bytes).expect("persist");
        let fetched = bundle.lookup.get(&hash).unwrap().expect("round trip");
        assert_eq!(fetched, bytes);
    }

    #[test]
    fn prune_payload_store_removes_non_retained_files() {
        let dir = tempfile::tempdir().unwrap();
        let store = DirectoryPayloadStore::new(dir.path().to_path_buf());
        let keep = store.insert(b"kept bytes").expect("insert keep");
        let drop = store.insert(b"drop bytes").expect("insert drop");
        let keep_set: std::collections::HashSet<[u8; 32]> = [keep].into_iter().collect();
        let removed = prune_payload_store(dir.path(), &keep_set).expect("prune");
        assert_eq!(removed, 1);
        assert!(store.get(&keep).unwrap().is_some());
        assert!(store.get(&drop).unwrap().is_none());
    }

    #[test]
    fn prune_payload_store_leaves_non_hex_files_alone() {
        let dir = tempfile::tempdir().unwrap();
        let store = DirectoryPayloadStore::new(dir.path().to_path_buf());
        let keep = store.insert(b"content").expect("insert");
        std::fs::write(dir.path().join("README.md"), b"operator notes").unwrap();
        std::fs::write(dir.path().join("tempfile.tmp"), b"junk").unwrap();
        let keep_set: std::collections::HashSet<[u8; 32]> = [keep].into_iter().collect();
        let removed = prune_payload_store(dir.path(), &keep_set).expect("prune");
        assert_eq!(removed, 0);
        assert!(dir.path().join("README.md").exists());
        assert!(dir.path().join("tempfile.tmp").exists());
    }

    #[test]
    fn prune_payload_store_tolerates_missing_dir() {
        let dir = tempfile::tempdir().unwrap();
        let ghost = dir.path().join("never-created");
        let removed = prune_payload_store(&ghost, &Default::default()).expect("prune");
        assert_eq!(removed, 0);
    }
}
