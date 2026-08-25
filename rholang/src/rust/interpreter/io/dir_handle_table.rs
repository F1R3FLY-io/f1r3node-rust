// Directory-stream handle table — parallel to `FileHandleTable`, backing
// the `entriesStreamOpen` / `entriesStreamNext` / `entriesStreamClose`
// natives (streaming-backing slice, 2026-08-25).
//
// A per-runtime `Arc<RwLock<HashMap<u64, Arc<DirHandle>>>>` mapping
// opaque u64 stream fds to `libc::DIR*` iterators.  Fds are monotonic
// and derived from a state-hash-seeded watermark — same aliasing-
// prevention pattern as `FileHandleTable` (PB-M-13 / slice 28).
//
// Why `Arc<DirHandle>` instead of storing `DirHandle` directly (as
// `FileHandleTable` does with `FileHandle`): every `readdir` runs in
// `spawn_blocking`, so the caller `.await`s the result while the
// per-handle iter mutex is held.  Wrapping each handle in an `Arc`
// lets the handler acquire a cheap clone under a brief read lock on
// the outer table, then acquire the handle's own mutex outside the
// table lock — different-fd traffic proceeds in parallel while same-
// fd calls serialize (which is required — POSIX `readdir` on the
// same `DIR*` from multiple threads is undefined).
//
// Why raw `libc::DIR*` and not `tokio::fs::ReadDir`: `tokio::fs::
// read_dir(path)` internally calls `std::fs::read_dir(path)` which
// calls `opendir(3)` on the joined path — a symlink-following open.
// The pre-existing bulk `fs_entries` handler explicitly uses `openat`
// + `O_NOFOLLOW` off a `safe_descend`-resolved dirfd for TOCTOU-
// immunity against symlink-swap attacks (see `handlers.rs::
// fs_entries`).  Streaming is meant to supersede bulk for large
// directories, so its safety story must match.  We open the dir via
// the same `safe_descend_verified` + `openat` path used by
// `fs_entries`, then wrap the fd in `fdopendir` for iteration.  The
// per-handle Mutex serializes `readdir` calls; each call runs inside
// `spawn_blocking`.
//
// The plan calls for `MAX_OPEN_FDS = 1024` to be shared between file
// and dir handles.  This module reuses the same constant but tracks
// dir-stream lifetimes in an independent HashMap — a file fd and a
// dir fd may collide numerically (they live in disjoint tables, and
// the Rholang layer routes each to the matching native, so numeric
// collision is not a soundness concern).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::sync::{Mutex, RwLock};

use super::lock::DeployScope;
use super::ConsensusMode;

/// Owned `libc::DIR*` iterator over a directory.
///
/// Constructed via `DirIter::from_dir_fd(fd)` (which calls `fdopendir`
/// and takes ownership of the fd; on failure the fd is closed before
/// the error is returned so callers do not double-close).  Drop calls
/// `closedir` which also closes the underlying kernel dirp fd.
///
/// The `*mut libc::DIR` pointer is not `Send` by default; the
/// `unsafe impl` below asserts serial single-thread access, which the
/// enclosing `Mutex<Option<DirIter>>` in `DirHandle` enforces.  POSIX
/// guarantees `readdir(3)` is thread-safe *across distinct* `DIR*`
/// streams but leaves same-stream concurrent access undefined; the
/// Mutex is the load-bearing invariant.
///
/// `readdir` also stashes state inside libc; passing the pointer
/// across a `spawn_blocking` boundary is safe as long as no other
/// thread touches it in the meantime — the outer Mutex guarantees
/// that.  The pointer round-trips via `usize` cast because raw
/// pointers are not `Send`; the guard proves the address stays live
/// for the closure's lifetime.
pub struct DirIter {
    dirp: *mut libc::DIR,
}

// SAFETY: the enclosing `Mutex<Option<DirIter>>` guarantees exclusive
// access — POSIX allows serial single-thread `readdir` on a `DIR*`.
unsafe impl Send for DirIter {}

impl DirIter {
    /// Wrap a directory fd in a `DIR*` iterator.  Takes ownership of
    /// `dir_fd` — do NOT close it separately.  On `fdopendir` failure
    /// the fd is closed before the error is returned.
    pub fn from_dir_fd(dir_fd: libc::c_int) -> std::io::Result<Self> {
        let dirp = unsafe { libc::fdopendir(dir_fd) };
        if dirp.is_null() {
            let e = std::io::Error::last_os_error();
            unsafe { libc::close(dir_fd) };
            return Err(e);
        }
        Ok(Self { dirp })
    }

    /// Raw pointer for callers driving `readdir` inside `spawn_blocking`.
    /// The caller must hold the enclosing Mutex guard for the duration
    /// of every use.
    pub fn as_ptr(&self) -> *mut libc::DIR { self.dirp }
}

impl std::fmt::Debug for DirIter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DirIter")
            .field("dirp", &(self.dirp as usize))
            .finish()
    }
}

impl Drop for DirIter {
    fn drop(&mut self) {
        // closedir closes the underlying dirp fd as a side-effect,
        // matching fdopendir's ownership contract.
        unsafe { libc::closedir(self.dirp) };
    }
}

/// Number of low bits reserved as per-lifetime fd-allocation headroom
/// below the state-hash-derived watermark.  Mirrors the constant of
/// the same name in `handle_table.rs`; kept independent so a future
/// tuning here (e.g., a much smaller MAX_OPEN_DIR_STREAMS) doesn't
/// silently reduce the file-fd budget.  See `handle_table.rs`
/// FD_ENTROPY_HEADROOM_BITS docs for the entropy-budget rationale.
const FD_ENTROPY_HEADROOM_BITS: u32 = 20;

// Compile-time invariant guard.  If a future change raises
// `MAX_OPEN_FDS` past the entropy-headroom budget, the build fails
// here — flagging that `seed_next_fd_from_state_hash`'s aliasing
// protection must be revisited.  The 2× factor is a safety margin
// for open+close cycles vs. concurrent-live fds.
const _: () = assert!(
    (super::MAX_OPEN_FDS as u64) * 2 < 1u64 << FD_ENTROPY_HEADROOM_BITS,
    "MAX_OPEN_FDS exceeds FD_ENTROPY_HEADROOM_BITS budget; \
     the state-hash-derived watermark cannot guarantee aliasing prevention"
);

/// One live directory-stream handle.
#[derive(Debug)]
pub struct DirHandle {
    /// The underlying `libc::DIR*` iterator, or `None` for a shadow
    /// handle inserted on the follower's `entriesStreamOpen` replay
    /// branch.
    ///
    /// The per-handle Mutex lets a `spawn_blocking(readdir)` call
    /// proceed while holding only this handle's lock — not the outer
    /// table `RwLock`.  Concurrent callers of `entriesStreamNext(fd)`
    /// for the same fd serialize on this mutex (which is required —
    /// POSIX leaves same-`DIR*` concurrent `readdir` undefined);
    /// calls for *different* fds proceed in parallel.
    ///
    /// The follower's replay branch never touches this field — the
    /// `is_replay = true` short-circuit returns the cached `previous`
    /// reply before reaching `readdir`.  A shadow handle carries
    /// enough metadata (`canon_path`, `cmode`, `deploy`) for the
    /// replay-branch handler to look up (cmode, canon_path) for
    /// symmetric WAL journaling.
    pub iter: Mutex<Option<DirIter>>,
    /// Canonical host path of the directory the stream iterates.
    pub canon_path: PathBuf,
    /// Consensus vs. oracular mode captured at open time.  The
    /// `entriesStreamNext` handler consults this to decide whether
    /// to journal each yielded entry into the consensus WAL.
    pub cmode: ConsensusMode,
    /// Deploy scope this stream was opened under.  Populated at open
    /// time from `handles.current_deploy_scope`; consulted by
    /// `close_all_for_deploy` to sweep streams the deploy left open
    /// past its `WalDeployScope::drop`.  Sentinel `[0; 32]` for
    /// out-of-deploy opens (test paths only under normal operation).
    pub deploy: DeployScope,
}

impl DirHandle {
    /// Construct a real (leader / oracular) handle wrapping an open
    /// `DIR*` iterator.
    pub fn new(
        iter: DirIter,
        canon_path: PathBuf,
        cmode: ConsensusMode,
        deploy: DeployScope,
    ) -> Self {
        Self {
            iter: Mutex::new(Some(iter)),
            canon_path,
            cmode,
            deploy,
        }
    }

    /// Construct a shadow handle for the follower's replay branch.
    /// No underlying `DIR*` — the replay path never iterates.
    pub fn shadow(canon_path: PathBuf, cmode: ConsensusMode, deploy: DeployScope) -> Self {
        Self {
            iter: Mutex::new(None),
            canon_path,
            cmode,
            deploy,
        }
    }
}

/// Per-runtime table of live directory-stream handles.
#[derive(Debug, Clone)]
pub struct DirHandleTable {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    table: RwLock<HashMap<u64, Arc<DirHandle>>>,
    next_fd: AtomicU64,
}

impl DirHandleTable {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                table: RwLock::new(HashMap::new()),
                next_fd: AtomicU64::new(1),
            }),
        }
    }

    /// Allocate a fresh fd and register the handle.
    ///
    /// Returns `Err(())` when the per-runtime fd cap is reached OR when
    /// `next_fd` would wrap past `u64::MAX`; upstream handlers translate
    /// either to `FSERR_QUOTA_EXCEEDED`.  Mirrors `FileHandleTable::
    /// insert` — same monotonicity + wrap-guard invariants; see that
    /// method's docstring for the aliasing-safety rationale.
    pub async fn insert(&self, handle: DirHandle) -> Result<u64, ()> {
        let mut table = self.inner.table.write().await;
        if table.len() >= super::MAX_OPEN_FDS {
            return Err(());
        }
        let current = self.inner.next_fd.load(Ordering::SeqCst);
        if current == u64::MAX {
            return Err(());
        }
        let fd = self.inner.next_fd.fetch_add(1, Ordering::SeqCst);
        if fd == u64::MAX {
            self.inner.next_fd.store(u64::MAX, Ordering::SeqCst);
            return Err(());
        }
        table.insert(fd, Arc::new(handle));
        Ok(fd)
    }

    /// Insert a handle at a specific fd, bypassing the monotonic
    /// allocator.  Used by the follower's `entriesStreamOpen` replay
    /// branch to shadow the leader's fd allocation — same contract as
    /// `FileHandleTable::insert_at` (see that method for the state-
    /// derivation rationale).  Returns `true` on success, `false` if
    /// the slot was already occupied.
    pub async fn insert_at(&self, fd: u64, handle: DirHandle) -> bool {
        let mut table = self.inner.table.write().await;
        if table.contains_key(&fd) {
            return false;
        }
        table.insert(fd, Arc::new(handle));
        true
    }

    /// Remove and drop the handle at `fd`.  Returns `true` if the fd
    /// was present.  Idempotent.  Dropping the `Arc<DirHandle>` drops
    /// the `ReadDir`, releasing the underlying kernel dirp handle.
    pub async fn remove(&self, fd: u64) -> bool {
        let mut table = self.inner.table.write().await;
        table.remove(&fd).is_some()
    }

    /// Look up an existing handle.  Returns a cloned `Arc` so the
    /// caller can drive `iter.lock().await` outside the table lock.
    /// `None` if `fd` is absent.
    pub async fn get(&self, fd: u64) -> Option<Arc<DirHandle>> {
        let table = self.inner.table.read().await;
        table.get(&fd).cloned()
    }

    /// Snapshot the next-fd counter for deploy-boundary rollback.
    pub fn snapshot_next_fd(&self) -> u64 { self.inner.next_fd.load(Ordering::SeqCst) }

    /// Raise the fd counter to at least `watermark + 1` before the
    /// next allocation.  Idempotent + monotonic.  Called by
    /// `RhoRuntimeImpl::reset` on block boundaries so a post-restart
    /// runtime cannot allocate fd values that alias stale references
    /// still present in the tuplespace.  Same contract as
    /// `FileHandleTable::seed_next_fd_watermark`.
    pub fn seed_next_fd_watermark(&self, watermark: u64) {
        let target = watermark.saturating_add(1);
        let mut current = self.inner.next_fd.load(Ordering::SeqCst);
        while current < target {
            match self.inner.next_fd.compare_exchange_weak(
                current,
                target,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => break,
                Err(observed) => current = observed,
            }
        }
    }

    /// Derive a deterministic per-restart watermark from a 32-byte
    /// state hash and seed `next_fd` from it.  Same 8-byte prefix +
    /// low-bit-mask derivation as `FileHandleTable::
    /// seed_next_fd_from_state_hash` — see that method's docstring
    /// for the entropy budget + consensus-commitment discussion.
    ///
    /// Dir-stream fds are consensus-observable in the same sense
    /// file fds are: a Rholang cap holding a stream fd can pass it
    /// back into `entriesStreamNext`, so the numeric value flows
    /// through the tuplespace and must be reproducible byte-for-byte
    /// on the follower.  Any change to the derivation (bytes used,
    /// mask width, hash algorithm) is a hard fork.
    pub fn seed_next_fd_from_state_hash(&self, hash: &[u8]) {
        debug_assert!(
            hash.len() >= 8,
            "seed_next_fd_from_state_hash requires at least 8 bytes of hash; \
             got {} — a shorter hash silently reduces entropy and disables aliasing protection",
            hash.len()
        );
        let mut buf = [0u8; 8];
        let n = hash.len().min(8);
        buf[..n].copy_from_slice(&hash[..n]);
        let hi = u64::from_be_bytes(buf);
        let watermark = hi & !((1u64 << FD_ENTROPY_HEADROOM_BITS) - 1);
        self.seed_next_fd_watermark(watermark);
    }

    /// Roll back to the snapshot, closing every fd allocated past it.
    /// `next_fd` is monotonic — the counter is NOT rewound.  Used from
    /// the deploy-boundary rollback path (Step 4 of the streaming
    /// slice) to sweep streams a failed / reverted deploy left open.
    pub async fn truncate_to(&self, snapshot: u64) {
        let mut table = self.inner.table.write().await;
        table.retain(|&fd, _| fd < snapshot);
    }

    /// Close every stream fd owned by `scope`.  Mirrors
    /// `LockRegistry::release_all_for_deploy` — called from
    /// `WalDeployScope::Drop` to sweep streams the caller neither
    /// closed explicitly nor consumed to EOS.  Returns the number
    /// of fds swept for diagnostics.
    ///
    /// Sentinel `[0; 32]` is a hard error: `WalDeployScope::Drop`
    /// only calls this with a Blake2b256-derived scope, which never
    /// collides with the sentinel.  A sentinel call would sweep
    /// every stream opened outside a deploy (test scaffolding), so
    /// fail loudly on the assumption that it indicates a code bug.
    pub async fn close_all_for_deploy(&self, scope: &DeployScope) -> usize {
        assert!(
            *scope != [0u8; 32],
            "DirHandleTable::close_all_for_deploy called with sentinel scope [0; 32]; \
             callers must pass a Blake2b256-derived deploy scope"
        );
        let mut table = self.inner.table.write().await;
        let before = table.len();
        table.retain(|_, h| &h.deploy != scope);
        before - table.len()
    }
}

impl Default for DirHandleTable {
    fn default() -> Self { Self::new() }
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_shadow(cmode: ConsensusMode, deploy: DeployScope) -> DirHandle {
        DirHandle::shadow(PathBuf::from("/root/dir"), cmode, deploy)
    }

    /// Open `path` via the same syscall pattern the streaming handler
    /// uses (`open(O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC)` + `fdopendir`)
    /// so tests exercise the real DIR* iteration path — not a
    /// tokio-shortcut that would follow symlinks.
    fn mk_real(path: &std::path::Path) -> DirHandle {
        use std::os::unix::ffi::OsStrExt;
        let cpath = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        let dir_fd = unsafe {
            libc::open(
                cpath.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        assert!(dir_fd >= 0, "open dir: {}", std::io::Error::last_os_error());
        let iter = DirIter::from_dir_fd(dir_fd).unwrap();
        DirHandle::new(
            iter,
            path.to_path_buf(),
            ConsensusMode::Oracular,
            [0xAAu8; 32],
        )
    }

    /// insert allocates monotonically starting at 1.
    #[tokio::test]
    async fn insert_allocates_monotonically_from_one() {
        let table = DirHandleTable::new();
        let fd1 = table
            .insert(mk_shadow(ConsensusMode::Oracular, [0xAAu8; 32]))
            .await
            .unwrap();
        let fd2 = table
            .insert(mk_shadow(ConsensusMode::Oracular, [0xAAu8; 32]))
            .await
            .unwrap();
        assert_eq!(fd1, 1);
        assert_eq!(fd2, 2);
    }

    /// insert past MAX_OPEN_FDS returns Err; remove + insert recovers.
    #[tokio::test]
    async fn insert_at_cap_returns_err_and_recovers_on_remove() {
        let table = DirHandleTable::new();
        let mut fds = Vec::with_capacity(super::super::MAX_OPEN_FDS);
        for _ in 0..super::super::MAX_OPEN_FDS {
            fds.push(
                table
                    .insert(mk_shadow(ConsensusMode::Oracular, [0xAAu8; 32]))
                    .await
                    .expect("insert under cap should succeed"),
            );
        }
        let over = table
            .insert(mk_shadow(ConsensusMode::Oracular, [0xAAu8; 32]))
            .await;
        assert!(
            over.is_err(),
            "insert past MAX_OPEN_FDS must return Err(()) → FSERR_QUOTA_EXCEEDED"
        );
        assert!(table.remove(fds.pop().unwrap()).await);
        let recovered = table
            .insert(mk_shadow(ConsensusMode::Oracular, [0xAAu8; 32]))
            .await
            .expect("insert after remove should succeed");
        let max_prev = *fds.iter().max().unwrap();
        assert!(
            recovered > max_prev,
            "next_fd must be monotonic across remove/insert"
        );
    }

    /// next_fd is monotonic across remove — a closed fd is never aliased.
    #[tokio::test]
    async fn next_fd_is_monotonic_across_remove() {
        let table = DirHandleTable::new();
        let fd1 = table
            .insert(mk_shadow(ConsensusMode::Oracular, [0xAAu8; 32]))
            .await
            .unwrap();
        assert!(table.remove(fd1).await);
        let fd2 = table
            .insert(mk_shadow(ConsensusMode::Oracular, [0xAAu8; 32]))
            .await
            .unwrap();
        assert!(fd2 > fd1, "next_fd must not rewind after remove");
    }

    /// insert_at places a shadow handle at the exact fd requested.
    #[tokio::test]
    async fn insert_at_places_shadow_handle_at_specified_fd() {
        let table = DirHandleTable::new();
        assert!(
            table
                .insert_at(
                    42,
                    DirHandle::shadow(
                        PathBuf::from("/root/dir"),
                        ConsensusMode::Consensus,
                        [0xAAu8; 32]
                    )
                )
                .await,
            "insert_at into an empty slot must succeed"
        );
        let dh = table.get(42).await.expect("handle at fd=42");
        assert_eq!(dh.cmode, ConsensusMode::Consensus);
        assert_eq!(dh.canon_path, PathBuf::from("/root/dir"));
        // Shadow handle: iter is None.
        assert!(dh.iter.lock().await.is_none());
    }

    /// insert_at into an occupied slot returns false and does NOT
    /// overwrite — silent overwrite would mask a follower state-
    /// derivation bug.
    #[tokio::test]
    async fn insert_at_occupied_slot_returns_false() {
        let table = DirHandleTable::new();
        table
            .insert_at(
                42,
                DirHandle::shadow(
                    PathBuf::from("/first"),
                    ConsensusMode::Oracular,
                    [0xAAu8; 32],
                ),
            )
            .await;
        assert!(
            !table
                .insert_at(
                    42,
                    DirHandle::shadow(
                        PathBuf::from("/second"),
                        ConsensusMode::Consensus,
                        [0xBBu8; 32]
                    )
                )
                .await,
            "insert_at into an occupied slot must return false"
        );
        let dh = table.get(42).await.expect("original handle survives");
        assert_eq!(dh.canon_path, PathBuf::from("/first"));
    }

    /// get returns None for an unknown fd.
    #[tokio::test]
    async fn get_returns_none_for_unknown_fd() {
        let table = DirHandleTable::new();
        assert!(table.get(999).await.is_none());
    }

    /// seed_next_fd_watermark raises next_fd monotonically.
    #[tokio::test]
    async fn seed_watermark_raises_next_fd() {
        let table = DirHandleTable::new();
        let fd1 = table
            .insert(mk_shadow(ConsensusMode::Oracular, [0xAAu8; 32]))
            .await
            .unwrap();
        assert_eq!(fd1, 1);
        table.seed_next_fd_watermark(1000);
        let fd2 = table
            .insert(mk_shadow(ConsensusMode::Oracular, [0xAAu8; 32]))
            .await
            .unwrap();
        assert!(
            fd2 > 1000,
            "next_fd must be raised past watermark; got {fd2}"
        );
    }

    /// seed_next_fd_watermark ignores same-or-smaller values.
    #[tokio::test]
    async fn seed_watermark_is_monotonic_no_op_on_smaller() {
        let table = DirHandleTable::new();
        table.seed_next_fd_watermark(10000);
        let after_seed = table.snapshot_next_fd();
        assert!(after_seed > 10000);
        table.seed_next_fd_watermark(5);
        assert_eq!(table.snapshot_next_fd(), after_seed);
        table.seed_next_fd_watermark(10000);
        assert_eq!(table.snapshot_next_fd(), after_seed);
    }

    /// seed_next_fd_from_state_hash is deterministic across tables.
    #[test]
    fn seed_from_state_hash_is_deterministic() {
        let hash = [0x12u8, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0].repeat(4);
        let t1 = DirHandleTable::new();
        let t2 = DirHandleTable::new();
        t1.seed_next_fd_from_state_hash(&hash);
        t2.seed_next_fd_from_state_hash(&hash);
        assert_eq!(
            t1.snapshot_next_fd(),
            t2.snapshot_next_fd(),
            "same state hash must yield same watermark"
        );
    }

    /// seed_next_fd_from_state_hash reserves the low
    /// FD_ENTROPY_HEADROOM_BITS as per-lifetime allocation headroom.
    #[test]
    fn seed_from_state_hash_reserves_low_bits_for_headroom() {
        let all_ones = vec![0xffu8; 32];
        let t = DirHandleTable::new();
        t.seed_next_fd_from_state_hash(&all_ones);
        let next = t.snapshot_next_fd();
        let watermark = next - 1;
        let low_mask = (1u64 << FD_ENTROPY_HEADROOM_BITS) - 1;
        assert_eq!(
            watermark & low_mask,
            0,
            "watermark's low {FD_ENTROPY_HEADROOM_BITS} bits must be zero (headroom); \
             got watermark {watermark:#018x}"
        );
    }

    /// Different state hashes produce disjoint fd ranges — the core
    /// cross-restart aliasing-prevention invariant.
    #[tokio::test]
    async fn different_state_hashes_produce_disjoint_fd_ranges() {
        let hash_a: Vec<u8> = std::iter::repeat_n(0x00u8, 32).collect();
        let t1 = DirHandleTable::new();
        t1.seed_next_fd_from_state_hash(&hash_a);
        let mut used_by_t1 = std::collections::HashSet::new();
        for _ in 0..100 {
            used_by_t1.insert(
                t1.insert(mk_shadow(ConsensusMode::Oracular, [0xAAu8; 32]))
                    .await
                    .unwrap(),
            );
        }
        let hash_b: Vec<u8> = std::iter::repeat_n(0xffu8, 32).collect();
        let t2 = DirHandleTable::new();
        t2.seed_next_fd_from_state_hash(&hash_b);
        for _ in 0..100 {
            let fd = t2
                .insert(mk_shadow(ConsensusMode::Oracular, [0xAAu8; 32]))
                .await
                .unwrap();
            assert!(
                !used_by_t1.contains(&fd),
                "fd {fd} allocated by t2 aliases a fd from t1's lifetime"
            );
        }
    }

    /// insert at u64::MAX returns Err — wrap guard.
    #[tokio::test]
    async fn insert_returns_err_when_next_fd_at_u64_max() {
        let table = DirHandleTable::new();
        table.seed_next_fd_watermark(u64::MAX - 1);
        assert_eq!(table.snapshot_next_fd(), u64::MAX);
        let res = table
            .insert(mk_shadow(ConsensusMode::Oracular, [0xAAu8; 32]))
            .await;
        assert!(res.is_err(), "insert at u64::MAX must return Err");
        assert_eq!(table.snapshot_next_fd(), u64::MAX);
    }

    /// seed_next_fd_from_state_hash panics in debug when passed a
    /// short hash — guards against a future refactor silently
    /// disabling the aliasing protection.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "requires at least 8 bytes of hash")]
    fn seed_from_short_hash_debug_asserts() {
        let short_hash = vec![0xffu8, 0x00, 0x00, 0x00];
        let t = DirHandleTable::new();
        t.seed_next_fd_from_state_hash(&short_hash);
    }

    /// truncate_to closes every fd past the snapshot; pre-snapshot
    /// fds survive.
    #[tokio::test]
    async fn truncate_to_snapshot_leaves_pre_snapshot_intact() {
        let table = DirHandleTable::new();
        let fd_pre = table
            .insert(mk_shadow(ConsensusMode::Oracular, [0xAAu8; 32]))
            .await
            .unwrap();
        let snap = table.snapshot_next_fd();
        let fd_post1 = table
            .insert(mk_shadow(ConsensusMode::Oracular, [0xAAu8; 32]))
            .await
            .unwrap();
        let fd_post2 = table
            .insert(mk_shadow(ConsensusMode::Oracular, [0xAAu8; 32]))
            .await
            .unwrap();
        assert!(fd_pre < snap && fd_post1 >= snap && fd_post2 >= snap);
        table.truncate_to(snap).await;
        assert!(
            table.get(fd_pre).await.is_some(),
            "pre-snapshot fd survives"
        );
        assert!(table.get(fd_post1).await.is_none());
        assert!(table.get(fd_post2).await.is_none());
    }

    /// close_all_for_deploy sweeps handles whose deploy matches the
    /// requested scope; other-deploy handles are untouched.
    #[tokio::test]
    async fn close_all_for_deploy_sweeps_only_matching_deploy() {
        let table = DirHandleTable::new();
        let scope_a: DeployScope = [0xAAu8; 32];
        let scope_b: DeployScope = [0xBBu8; 32];
        let fd_a1 = table
            .insert(mk_shadow(ConsensusMode::Oracular, scope_a))
            .await
            .unwrap();
        let fd_a2 = table
            .insert(mk_shadow(ConsensusMode::Oracular, scope_a))
            .await
            .unwrap();
        let fd_b = table
            .insert(mk_shadow(ConsensusMode::Consensus, scope_b))
            .await
            .unwrap();
        let n = table.close_all_for_deploy(&scope_a).await;
        assert_eq!(n, 2, "both scope_a streams swept");
        assert!(table.get(fd_a1).await.is_none());
        assert!(table.get(fd_a2).await.is_none());
        assert!(
            table.get(fd_b).await.is_some(),
            "scope_b stream must survive"
        );
    }

    /// close_all_for_deploy panics on the sentinel scope — that's
    /// the same fail-loud invariant WalDeployScope::new_with_lock_sweep
    /// enforces at construction.
    #[tokio::test]
    #[should_panic(expected = "sentinel scope")]
    async fn close_all_for_deploy_panics_on_sentinel_scope() {
        let table = DirHandleTable::new();
        let _ = table.close_all_for_deploy(&[0u8; 32]).await;
    }

    /// A real handle's DIR* iterator can be advanced via
    /// `libc::readdir` under the per-handle Mutex.  Exercises the
    /// actual streaming path (not just the metadata slots) so a
    /// future refactor to the iter storage doesn't silently break
    /// per-call iteration.  Passes the raw pointer via `usize` cast
    /// to satisfy `Send` — the enclosing Mutex guard keeps it live.
    #[tokio::test]
    async fn real_handle_iter_can_be_advanced() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a"), b"").unwrap();
        std::fs::write(dir.path().join("b"), b"").unwrap();
        let table = DirHandleTable::new();
        let fd = table.insert(mk_real(dir.path())).await.unwrap();
        let dh = table.get(fd).await.unwrap();
        let iter_lock = dh.iter.lock().await;
        let iter = iter_lock.as_ref().expect("real handle has iter");
        let names = read_all_names(iter);
        assert_eq!(names, vec!["a".to_string(), "b".to_string()]);
    }

    /// Distinct opens of the same directory yield distinct fds with
    /// independent iterators — the dir analog of the file table's
    /// "distinct opens yield distinct cursors" invariant.
    #[tokio::test]
    async fn distinct_opens_yield_independent_iterators() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a"), b"").unwrap();
        std::fs::write(dir.path().join("b"), b"").unwrap();
        let table = DirHandleTable::new();
        let fd1 = table.insert(mk_real(dir.path())).await.unwrap();
        let fd2 = table.insert(mk_real(dir.path())).await.unwrap();
        assert_ne!(fd1, fd2);
        // Advance fd1 to completion.
        let dh1 = table.get(fd1).await.unwrap();
        {
            let it = dh1.iter.lock().await;
            let it = it.as_ref().unwrap();
            let names = read_all_names(it);
            assert_eq!(names.len(), 2, "fd1 saw both entries");
        }
        // fd2's iterator has not been advanced — must still see both.
        let dh2 = table.get(fd2).await.unwrap();
        let it = dh2.iter.lock().await;
        let it = it.as_ref().unwrap();
        let names = read_all_names(it);
        assert_eq!(names.len(), 2, "fd2 iterator must be independent of fd1");
    }

    /// Drain `iter` via readdir, skipping "." and "..", returning
    /// sorted names.  Serial single-thread access is sound because
    /// the caller holds the enclosing Mutex guard.
    fn read_all_names(iter: &DirIter) -> Vec<String> {
        use std::os::unix::ffi::OsStringExt;
        let dirp = iter.as_ptr();
        let mut names: Vec<String> = Vec::new();
        loop {
            // POSIX readdir returns NULL on both EOF and error;
            // distinguishing them requires clearing errno beforehand.
            unsafe { errno_reset() };
            let ent = unsafe { libc::readdir(dirp) };
            if ent.is_null() {
                let raw = std::io::Error::last_os_error().raw_os_error();
                assert!(
                    raw == Some(0) || raw.is_none(),
                    "readdir returned NULL with errno {raw:?}"
                );
                break;
            }
            let name_ptr = unsafe { (*ent).d_name.as_ptr() };
            let name_c = unsafe { std::ffi::CStr::from_ptr(name_ptr) };
            let name_bytes = name_c.to_bytes();
            if name_bytes == b"." || name_bytes == b".." {
                continue;
            }
            let name = std::ffi::OsString::from_vec(name_bytes.to_vec())
                .to_string_lossy()
                .into_owned();
            names.push(name);
        }
        names.sort();
        names
    }

    #[cfg(target_os = "macos")]
    unsafe fn errno_reset() { *libc::__error() = 0; }

    #[cfg(target_os = "linux")]
    unsafe fn errno_reset() { *libc::__errno_location() = 0; }
}
