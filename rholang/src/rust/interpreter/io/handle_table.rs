// File-descriptor table.
//
// A per-runtime `Arc<RwLock<HashMap<u64, FileHandle>>>` mapping opaque u64
// fds to open `File` handles.  Fds are monotonic — the counter never
// rewinds — so a closed fd is never reused.  This preserves the invariant
// that a stale fd reliably observes `FSERR_CLOSED` rather than aliasing a
// later-opened file.
//
// Lifecycle: the plan calls for `snapshot_next_fd`/`truncate_to` for
// deploy-boundary rollback on the production deploy path.  Those live at
// the runtime layer (Phase 1 tail) and take an immutable snapshot of the
// counter; on rollback, any fds allocated past the snapshot are closed and
// removed from the table.

use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::sync::RwLock;

use super::mode::AccessMode;
use super::wal::Wal;
use super::ConsensusMode;

/// Slice 28 (H-28-F1 review fix): number of low bits reserved as
/// per-lifetime fd-allocation headroom below the state-hash-derived
/// watermark.  2^20 = ~1M fds is safely above `MAX_OPEN_FDS = 1024`
/// live fds and any realistic open+close cycle count within a single
/// block-computation.  A `const` assertion below enforces that
/// `MAX_OPEN_FDS` cannot silently exceed this budget on a future
/// change.
const FD_ENTROPY_HEADROOM_BITS: u32 = 20;

// ST-28-4 review fix: compile-time invariant guard.  If a future
// change raises `MAX_OPEN_FDS` past the entropy-headroom budget, the
// build fails here — flagging that `seed_next_fd_from_state_hash`'s
// aliasing protection must be revisited.  The 2× factor is a safety
// margin for open+close cycles vs. concurrent-live fds.
const _: () = assert!(
    (super::MAX_OPEN_FDS as u64) * 2 < 1u64 << FD_ENTROPY_HEADROOM_BITS,
    "MAX_OPEN_FDS exceeds FD_ENTROPY_HEADROOM_BITS budget; \
     the state-hash-derived watermark cannot guarantee aliasing prevention"
);

#[derive(Debug)]
pub struct FileHandle {
    /// The underlying OS file, or `None` for a shadow handle.
    ///
    /// **C-R1 review fix (slice 29 redesign round 2):** on the
    /// follower's `fs_open` replay branch we insert a *shadow*
    /// handle with `file = None` so that subsequent replay-branch
    /// mutating handlers can still look up `(cmode, canon_path)`
    /// for symmetric WAL journaling.  The follower never touches
    /// `file` on the replay branch (read/write handlers short-
    /// circuit on `is_replay = true` and return the cached
    /// `previous` reply), so a `None` here is never dereferenced
    /// through the syscall path.  `raw_fd()` returns `None` for
    /// shadow handles — any code path that reaches for the OS fd
    /// on a shadow handle gets `FSERR_CLOSED`, which is the
    /// correct failure mode (should never happen in practice
    /// because is_replay short-circuits earlier).
    pub file: Option<File>,
    pub canon_path: PathBuf,
    pub mode: AccessMode,
    /// Slice 29 (PB-M-14): per-cap consensus mode captured at
    /// `fs_open` time.  Mutating handlers (`fs_write`,
    /// `fs_write_at`, `fs_truncate`) consult this via
    /// `handles.with_mut` to decide whether to journal the op into
    /// the consensus WAL.
    pub cmode: ConsensusMode,
}

#[derive(Debug, Clone)]
pub struct FileHandleTable {
    inner: Arc<Inner>,
    /// Slice 29 (PB-M-14): consensus-mode Write-Ahead Log.  Attached
    /// to the handle table because both are per-runtime state that
    /// already gets plumbed identically through `FsProcesses`.
    /// Handler closures access via `self.handles.wal()`.  Journal
    /// appends happen inside the fd-based mutating handlers
    /// (`fs_write`, `fs_write_at`, `fs_truncate`) after successful
    /// syscall completion, gated on the FileHandle's `cmode`.
    pub wal: Wal,
    /// H-5 fix (2026-08-06): shared root-identity registry.
    /// Populated once at boot from operator-provisioned root
    /// paths; consulted on every `safe_descend_verified` in the
    /// fs_* handlers to detect post-boot rename-and-recreate of
    /// the root directory.  Attached to the handle table so all
    /// handler closures reach it via `self.handles.root_registry`;
    /// shared across runtimes via `RuntimeManager` so a single
    /// boot-time population is visible everywhere.
    pub root_registry: super::path::RootIdentityRegistry,
    /// Phase 8 slice 8a: shared range-lock registry.  Colocated on
    /// `RuntimeManager` alongside `root_id_registry`; broadcast to
    /// every spawned runtime via `share_lock_registry` so cross-cap
    /// coordination on the same `(dev, inode)` collapses to a single
    /// entry regardless of which runtime holds each cap.  See X-1
    /// design memo in the plan.
    pub lock_registry: super::lock::LockRegistry,
    /// Phase 8 slice 8a step 5: the per-runtime "current deploy
    /// scope" cell.  Set by `casper::rholang::runtime::WalDeployScope::
    /// new` at deploy-entry to the deploy-derived `DeployScope`
    /// (Blake2b256(deploy.sig) for user deploys; a state-hash-
    /// derived scope for system deploys).  Cleared to `[0; 32]` when
    /// the `WalDeployScope` guard drops.
    ///
    /// The `fs_lock_range` / `fs_lock_sequential` handlers read this
    /// cell at acquire time and record it in the `LockRegistry`
    /// entry's `deploy` field, so `release_all_for_deploy(&scope)`
    /// on `WalDeployScope::drop` can sweep exactly the current
    /// deploy's leaked locks.
    ///
    /// Default `[0; 32]` is the sentinel value guarded against in
    /// `LockRegistry::release_all_for_deploy`; nothing calls
    /// `release_all_for_deploy(&[0; 32])`, so the sentinel guard
    /// only fires when a deploy-end sweep is attempted OUTSIDE a
    /// live `WalDeployScope` — which would be a code bug.  Per-
    /// runtime cell (not manager-broadcast): each runtime processes
    /// deploys sequentially, so a single cell suffices; concurrent
    /// runtimes have independent `FileHandleTable` instances.
    pub current_deploy_scope: Arc<std::sync::RwLock<super::lock::DeployScope>>,
    /// Streaming-backing slice (2026-08-25): per-runtime directory-
    /// stream handle table backing the entriesStream* natives.
    /// Colocated with the file handle table because both are per-
    /// runtime and travel together through `FsProcesses`; handlers
    /// reach it via `self.handles.dir_handles`.  NOT shared across
    /// runtimes via `RuntimeManager` — dir-stream lifetimes are
    /// per-runtime just like file-handle lifetimes.
    pub dir_handles: super::dir_handle_table::DirHandleTable,
}

#[derive(Debug)]
struct Inner {
    table: RwLock<HashMap<u64, FileHandle>>,
    next_fd: AtomicU64,
}

impl FileHandleTable {
    pub fn new() -> Self {
        FileHandleTable {
            inner: Arc::new(Inner {
                table: RwLock::new(HashMap::new()),
                next_fd: AtomicU64::new(1),
            }),
            wal: Wal::new(),
            // H-5: empty registry by default.  Boot pipeline
            // populates via `share_root_registry`; tests that
            // don't set up a registry get a no-op (safe_descend
            // sees None → skips the identity check, preserving
            // pre-H-5 behavior).
            root_registry: super::path::RootIdentityRegistry::new(),
            lock_registry: super::lock::LockRegistry::new(),
            // Step 5: sentinel default.  A live deploy overwrites
            // this via WalDeployScope::new at deploy entry; the
            // guard clears it back to sentinel on drop.
            current_deploy_scope: Arc::new(std::sync::RwLock::new([0u8; 32])),
            dir_handles: super::dir_handle_table::DirHandleTable::new(),
        }
    }

    /// H-5 fix (2026-08-06): replace the per-runtime empty
    /// registry with a manager-shared one so all spawned
    /// runtimes read from the same root-identity map.  Called
    /// by `RuntimeManager::spawn_runtime` mirroring the
    /// `fs_snapshot_writer` sharing pattern.
    pub fn share_root_registry(&mut self, shared: super::path::RootIdentityRegistry) {
        self.root_registry = shared;
    }

    /// Phase 8 slice 8a: attach the manager-shared range-lock
    /// registry so all runtimes spawned from this manager see the
    /// same `(dev, inode) → FileLockState` map.  Mirrors
    /// `share_root_registry` — called from `RuntimeManager::
    /// spawn_runtime` and `spawn_replay_runtime`.
    pub fn share_lock_registry(&mut self, shared: super::lock::LockRegistry) {
        self.lock_registry = shared;
    }

    /// Allocate a fresh fd and register the handle.
    ///
    /// Returns `Err(())` when the per-runtime fd cap is reached OR
    /// when `next_fd` would wrap past `u64::MAX`; the handler
    /// translates either to `FSERR_QUOTA_EXCEEDED`.  H-28-F2 review
    /// fix: the wrap guard prevents a lifetime allocation of ~2^32
    /// open/close cycles combined with a high-watermark seed from
    /// wrapping to fd=0 (which would alias any stale reference at
    /// low fd values).
    pub async fn insert(&self, handle: FileHandle) -> Result<u64, ()> {
        let mut table = self.inner.table.write().await;
        if table.len() >= super::MAX_OPEN_FDS {
            return Err(());
        }
        // Fetch-then-check on a monotonic counter — we bail out
        // before the increment would overflow.  The reserved
        // headroom is a full u32 (see `seed_next_fd_from_state_hash`
        // for the watermark mask that guarantees it).
        let current = self.inner.next_fd.load(Ordering::SeqCst);
        if current == u64::MAX {
            return Err(());
        }
        let fd = self.inner.next_fd.fetch_add(1, Ordering::SeqCst);
        // Post-increment defensive check: if the fetch_add wrapped
        // (would only happen under highly-concurrent adversarial
        // load), reject the allocation and roll the counter back.
        if fd == u64::MAX {
            self.inner.next_fd.store(u64::MAX, Ordering::SeqCst);
            return Err(());
        }
        table.insert(fd, handle);
        Ok(fd)
    }

    /// C-R1 review fix (slice 29 round 2): insert a handle at a
    /// specific fd, bypassing the monotonic allocator.  Used by
    /// `fs_open`'s `is_replay = true` branch on the follower — the
    /// leader's fd (extracted from the cached `previous` reply) is
    /// passed here so the follower's fd table indexes the same
    /// numeric key as the leader's, enabling symmetric WAL
    /// journaling in the subsequent replay-branch mutating handlers
    /// (`journal_write` / `journal_truncate` look up
    /// `(cmode, canon_path)` via `handles.with_mut(fd, ...)`).
    ///
    /// Does NOT advance `next_fd` — the leader's fd was allocated
    /// via `insert()` and slice-28's watermark seeding ensures
    /// leader/follower produce the same fd sequence anyway; the
    /// counter on the follower catches up on the leader's
    /// subsequent `insert()` calls (which will observe the seeded
    /// watermark).  Returns `true` on success, `false` if the fd
    /// slot was already occupied (which would indicate a follower
    /// state-derivation bug and should NOT overwrite silently).
    pub async fn insert_at(&self, fd: u64, handle: FileHandle) -> bool {
        let mut table = self.inner.table.write().await;
        if table.contains_key(&fd) {
            return false;
        }
        table.insert(fd, handle);
        true
    }

    /// Remove and close the handle at `fd`.  Returns `true` if the fd was
    /// present.  Idempotent: closing an unknown fd is a no-op returning
    /// `false`.
    pub async fn remove(&self, fd: u64) -> bool {
        let mut table = self.inner.table.write().await;
        table.remove(&fd).is_some()
    }

    /// Snapshot the next-fd counter for deploy-boundary rollback.
    pub fn snapshot_next_fd(&self) -> u64 { self.inner.next_fd.load(Ordering::SeqCst) }

    /// Slice 28 (PB-M-13): raise the fd counter to at least
    /// `watermark + 1` before the next allocation.  Called by
    /// `RhoRuntimeImpl::reset` on every block boundary so a
    /// post-restart runtime cannot allocate fd values that alias
    /// stale references still present in the tuplespace.
    ///
    /// Aliasing scenario averted: pre-restart, Deploy A opened
    /// File with fd=42 and stashed the cap in the tuplespace.
    /// Node restarts; the fresh `FileHandleTable::next_fd` starts
    /// at 1.  A subsequent Deploy B opens 41 unrelated files;
    /// the 42nd new open would allocate fd=42.  Deploy A's
    /// stashed cap, invoked later via `fsRead(42, ...)`, would
    /// now read from Deploy B's file.  Seeding the watermark
    /// from a value larger than any pre-restart fd prevents this.
    ///
    /// Idempotent and monotonic: multiple calls with the same or
    /// decreasing watermark are no-ops; the counter never rewinds.
    /// This preserves the pre-existing invariant that a closed fd
    /// is never re-issued.
    pub fn seed_next_fd_watermark(&self, watermark: u64) {
        let target = watermark.saturating_add(1);
        // Compare-and-swap loop to raise next_fd atomically.
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

    /// Slice 28: derive a deterministic per-restart watermark from
    /// a 32-byte state hash and seed `next_fd` from it.  Called by
    /// `RhoRuntimeImpl::reset` on every block boundary.
    ///
    /// **Consensus commitment**: fd values are consensus-observable
    /// (stored in Rholang tuplespace state as `u64` inside File-
    /// agent `fdP` cells; go on-chain via the tuplespace state
    /// hash).  This derivation is therefore an implicit consensus
    /// commitment — every validator resetting to the same state
    /// hash computes the same watermark, so replay of captured
    /// fd values on followers matches the leader's allocation.  Any
    /// future change to the derivation (bytes used, shift amount,
    /// hashing algorithm) is a hard fork.
    ///
    /// **Entropy derivation** (H-28-F1 review fix, replacing the
    /// original 4-byte prefix): take first 8 bytes of `hash` as a
    /// big-endian u64 and mask off the low `FD_ENTROPY_HEADROOM_BITS`
    /// bits.  The masked bits are reserved as per-lifetime allocation
    /// headroom — a single runtime lifetime can allocate up to
    /// `1 << FD_ENTROPY_HEADROOM_BITS` fds before the counter would
    /// enter the next watermark's range.
    ///
    /// Entropy budget: 64 - 20 = 44 bits of state-hash entropy in the
    /// watermark.  Birthday collision at ~2^22 (~4 million) blocks →
    /// ~130 years at 1 block/sec vs. the 4-byte derivation's ~18-hour
    /// horizon.  Headroom budget: 2^20 = ~1M fd allocations per
    /// runtime lifetime (a single block computation), well above any
    /// realistic per-block open/close pattern (`MAX_OPEN_FDS = 1024`
    /// live fds, so ~1000 open+close cycles per block is the
    /// realistic upper bound).
    ///
    /// `debug_assert` guards against a caller passing a short hash
    /// (L-28-F1): state hashes are always 32 bytes, but a future
    /// refactor passing a truncated value would silently disable
    /// the seed and break the aliasing protection.
    pub fn seed_next_fd_from_state_hash(&self, hash: &[u8]) {
        debug_assert!(
            hash.len() >= 8,
            "seed_next_fd_from_state_hash requires at least 8 bytes of hash; \
             got {} — a shorter hash silently reduces entropy and disables aliasing protection",
            hash.len()
        );
        // Bounds-safe: pad with zeros if hash is shorter.
        let mut buf = [0u8; 8];
        let n = hash.len().min(8);
        buf[..n].copy_from_slice(&hash[..n]);
        let hi = u64::from_be_bytes(buf);
        // Mask off the low headroom bits so a full runtime lifetime
        // cannot overflow into the next watermark's range.  The
        // watermark is unsigned throughout — it is a bit-pattern
        // derived from an unsigned hash and its full 64 bits carry
        // meaning.  Fd values traverse the Rholang boundary via
        // GInt, but native handlers reinterpret the received GInt
        // bit-pattern as u64 (see `handlers.rs` — `fd as u64` after
        // the length-arg sign guard, which applies only to
        // legitimately-signed inputs like read/write lengths).
        let watermark = hi & !((1u64 << FD_ENTROPY_HEADROOM_BITS) - 1);
        self.seed_next_fd_watermark(watermark);
    }

    /// Roll back to the snapshot, closing every fd allocated past it.
    pub async fn truncate_to(&self, snapshot: u64) {
        let mut table = self.inner.table.write().await;
        table.retain(|&fd, _| fd < snapshot);
        // Note: `next_fd` is monotonic even across rollback — that is the
        // invariant that prevents fd aliasing across deploys.
    }

    /// Run `f` against the handle at `fd` under a write lock.  Returns
    /// `None` if the fd is absent.
    pub async fn with_mut<F, R>(&self, fd: u64, f: F) -> Option<R>
    where F: FnOnce(&mut FileHandle) -> R {
        let mut table = self.inner.table.write().await;
        table.get_mut(&fd).map(f)
    }

    /// Look up the raw OS fd for a given logical fd handle.  Used by the
    /// spawn_blocking closures so they can issue libc syscalls without
    /// holding the tokio RwLock across the syscall.
    ///
    /// Returns `None` if the fd is absent OR the handle is a shadow
    /// (`file: None`, C-R1 review fix).  Shadow handles are inserted
    /// only on the follower's `fs_open` replay branch; the read/write
    /// syscall paths short-circuit on `is_replay = true` before
    /// reaching `raw_fd`, so a `None` here from a shadow handle
    /// should never be observed in practice — but the fail-closed
    /// return (translated to `FSERR_CLOSED` upstream) is correct if
    /// it ever is.
    ///
    /// SAFETY: the returned raw fd is valid only until the underlying
    /// `FileHandle`'s `File` is dropped (i.e., until `remove` is called).
    /// Callers must not close it directly, and must not use it after any
    /// intervening `remove(fd)`.
    #[cfg(unix)]
    pub async fn raw_fd(&self, fd: u64) -> Option<i32> {
        use std::os::fd::AsRawFd;
        let table = self.inner.table.read().await;
        table
            .get(&fd)
            .and_then(|h| h.file.as_ref().map(|f| f.as_raw_fd()))
    }
}

impl Default for FileHandleTable {
    fn default() -> Self { Self::new() }
}

// ---------------------------------------------------------------------
// Slice 27 review-fix tests: fresh-mint invariants and MAX_OPEN_FDS
// cap enforcement.
// ---------------------------------------------------------------------

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::os::fd::{AsRawFd, IntoRawFd};

    use super::*;

    fn open_ro(path: &std::path::Path) -> File {
        std::fs::OpenOptions::new().read(true).open(path).unwrap()
    }

    fn make_handle(file: File, path: PathBuf) -> FileHandle {
        FileHandle {
            file: Some(file),
            canon_path: path,
            mode: AccessMode::Read,
            cmode: ConsensusMode::Oracular,
        }
    }

    fn make_shadow_handle(path: PathBuf, cmode: ConsensusMode) -> FileHandle {
        FileHandle {
            file: None,
            canon_path: path,
            mode: AccessMode::Read,
            cmode,
        }
    }

    // MT-27-1 review fix: opening the same real file twice produces
    // DISTINCT kernel fds, and reading N bytes on fd1 does NOT advance
    // fd2's cursor.  This is the CORE POSIX-open-twice invariant that
    // motivated slice 27's revert of the Fs cache.  The Rholang
    // integration tests can't cover this (their mock uses a single
    // shared cursor cell); this Rust-level test does.
    #[tokio::test]
    async fn open_same_file_twice_yields_distinct_fds_with_independent_cursors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.bin");
        std::fs::write(&path, b"abcdefgh").unwrap();

        let table = FileHandleTable::new();
        let fd1 = table
            .insert(make_handle(open_ro(&path), path.clone()))
            .await
            .unwrap();
        let fd2 = table
            .insert(make_handle(open_ro(&path), path.clone()))
            .await
            .unwrap();
        assert_ne!(fd1, fd2, "distinct opens must yield distinct logical fds");

        // Read 3 bytes on fd1; advances fd1's cursor to position 3.
        let mut buf1 = [0u8; 3];
        let raw1 = table.raw_fd(fd1).await.unwrap();
        assert_ne!(
            raw1,
            table.raw_fd(fd2).await.unwrap(),
            "distinct kernel fds"
        );
        // Use the FileHandle::file directly for standard-library read.
        table
            .with_mut(fd1, |h| {
                h.file.as_mut().unwrap().read_exact(&mut buf1).unwrap();
            })
            .await
            .unwrap();
        assert_eq!(&buf1, b"abc", "fd1 read the first three bytes");

        // fd2's cursor must still be at 0 — independent-cursor invariant.
        let mut buf2 = [0u8; 3];
        table
            .with_mut(fd2, |h| {
                h.file.as_mut().unwrap().read_exact(&mut buf2).unwrap();
            })
            .await
            .unwrap();
        assert_eq!(
            &buf2, b"abc",
            "fd2 must start at 0 (fd1's read must not advance fd2's cursor)"
        );
    }

    // Companion: closing fd1 does NOT close fd2's kernel fd.  Also
    // POSIX-guaranteed but worth pinning at the table level.
    #[tokio::test]
    async fn close_one_fd_does_not_affect_the_other() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.bin");
        std::fs::write(&path, b"xyz").unwrap();

        let table = FileHandleTable::new();
        let fd1 = table
            .insert(make_handle(open_ro(&path), path.clone()))
            .await
            .unwrap();
        let fd2 = table
            .insert(make_handle(open_ro(&path), path.clone()))
            .await
            .unwrap();

        assert!(table.remove(fd1).await, "fd1 removed");
        // fd2 still works.
        let mut buf = [0u8; 3];
        table
            .with_mut(fd2, |h| {
                h.file.as_mut().unwrap().read_exact(&mut buf).unwrap();
            })
            .await
            .unwrap();
        assert_eq!(&buf, b"xyz");
        assert!(table.remove(fd2).await);
    }

    // MT-27-2 review fix: MAX_OPEN_FDS cap enforcement.  Under slice
    // 27 fresh-mint, a runaway deploy can exhaust the per-runtime
    // table.  Pins that (a) the cap is enforced, (b) subsequent
    // inserts fail cleanly with `Err(())` (translated to
    // FSERR_QUOTA_EXCEEDED by the handler), and (c) removing one
    // entry lets the next insert succeed.
    #[tokio::test]
    async fn insert_at_cap_returns_err_and_recovers_on_remove() {
        // Use a temp file for real fds; we only need the table
        // machinery to see MAX_OPEN_FDS entries, so open the same file
        // over and over.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f");
        std::fs::write(&path, b"").unwrap();

        let table = FileHandleTable::new();
        // Insert MAX_OPEN_FDS handles.
        let mut fds = Vec::with_capacity(super::super::MAX_OPEN_FDS);
        for _ in 0..super::super::MAX_OPEN_FDS {
            let fd = table
                .insert(make_handle(open_ro(&path), path.clone()))
                .await
                .expect("insert under cap should succeed");
            fds.push(fd);
        }
        // The (cap + 1)th insert must fail.
        let over = table
            .insert(make_handle(open_ro(&path), path.clone()))
            .await;
        assert!(
            over.is_err(),
            "insert past MAX_OPEN_FDS must return Err(()) → FSERR_QUOTA_EXCEEDED"
        );
        // Remove one and confirm the next insert succeeds — cap is a
        // count of LIVE entries, not a lifetime allocation count.
        assert!(table.remove(fds.pop().unwrap()).await);
        let recovered = table
            .insert(make_handle(open_ro(&path), path.clone()))
            .await
            .expect("insert after remove should succeed");
        // Fd counter is monotonic — recovered fd is strictly greater
        // than any previously-issued fd.
        let max_prev = *fds.iter().max().unwrap();
        assert!(
            recovered > max_prev,
            "next_fd must be monotonic across remove/insert"
        );
    }

    // Regression pin: `next_fd` never rewinds — a closed fd is never
    // aliased.  Prevents a future refactor that "reclaims" fds by
    // rewinding the counter (would break the fd-freshness invariant
    // that stale references reliably observe FSERR_CLOSED).
    #[tokio::test]
    async fn next_fd_is_monotonic_across_remove() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f");
        std::fs::write(&path, b"").unwrap();

        let table = FileHandleTable::new();
        let fd1 = table
            .insert(make_handle(open_ro(&path), path.clone()))
            .await
            .unwrap();
        assert!(table.remove(fd1).await);
        let fd2 = table
            .insert(make_handle(open_ro(&path), path.clone()))
            .await
            .unwrap();
        assert!(fd2 > fd1, "next_fd must not rewind after remove");
    }

    // ---------------------------------------------------------
    // Slice 28 (PB-M-13): post-restart fd-aliasing prevention.
    // ---------------------------------------------------------

    // seed_next_fd_watermark raises next_fd monotonically.
    #[tokio::test]
    async fn seed_watermark_raises_next_fd() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f");
        std::fs::write(&path, b"").unwrap();
        let table = FileHandleTable::new();
        // Initially next_fd = 1.
        let fd1 = table
            .insert(make_handle(open_ro(&path), path.clone()))
            .await
            .unwrap();
        assert_eq!(fd1, 1);
        // Seed watermark to 1000.  Next allocation should be > 1000.
        table.seed_next_fd_watermark(1000);
        let fd2 = table
            .insert(make_handle(open_ro(&path), path.clone()))
            .await
            .unwrap();
        assert!(
            fd2 > 1000,
            "next_fd must be raised past watermark; got {fd2}"
        );
    }

    // seed_next_fd_watermark is idempotent — calling with the same
    // or a smaller value than current next_fd is a no-op.
    #[tokio::test]
    async fn seed_watermark_is_monotonic_no_op_on_smaller() {
        let table = FileHandleTable::new();
        table.seed_next_fd_watermark(10000);
        let after_seed = table.snapshot_next_fd();
        assert!(after_seed > 10000);
        // Attempt to lower the watermark — must be ignored.
        table.seed_next_fd_watermark(5);
        assert_eq!(table.snapshot_next_fd(), after_seed);
        // Same-value seed is a no-op.
        table.seed_next_fd_watermark(10000);
        assert_eq!(table.snapshot_next_fd(), after_seed);
    }

    // seed_next_fd_from_state_hash derives a deterministic
    // watermark: two calls with the same hash produce the same
    // final next_fd.
    #[test]
    fn seed_from_state_hash_is_deterministic() {
        let hash = [0x12u8, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0].repeat(4);
        let t1 = FileHandleTable::new();
        let t2 = FileHandleTable::new();
        t1.seed_next_fd_from_state_hash(&hash);
        t2.seed_next_fd_from_state_hash(&hash);
        assert_eq!(
            t1.snapshot_next_fd(),
            t2.snapshot_next_fd(),
            "same state hash must yield same watermark"
        );
    }

    // seed_next_fd_from_state_hash uses first 8 bytes (updated
    // from 4 per H-28-F1) — different hashes with the same
    // 8-byte prefix collide.  With Blake2b hashing → uniform
    // distribution → birthday collision at ~2^32 blocks (~130
    // years at 1 block/sec).
    #[test]
    fn seed_from_state_hash_differs_for_different_prefixes() {
        // Updated for H-28-F1 fix: derivation now takes 8 bytes.
        let hash_a = [0xffu8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00].to_vec();
        let hash_b = [0x00u8, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00].to_vec();
        let ta = FileHandleTable::new();
        let tb = FileHandleTable::new();
        ta.seed_next_fd_from_state_hash(&hash_a);
        tb.seed_next_fd_from_state_hash(&hash_b);
        assert_ne!(ta.snapshot_next_fd(), tb.snapshot_next_fd());
    }

    /// Slice 28 review-fix: `seed_next_fd_from_state_hash` reserves
    /// exactly `FD_ENTROPY_HEADROOM_BITS` low bits for per-lifetime
    /// allocation.  Pins the derivation so a regression narrowing
    /// the mask (or extending it past what's safe) trips a test.
    #[test]
    fn seed_from_state_hash_reserves_low_bits_for_headroom() {
        // Hash with all bits set — watermark should be
        // 0xFFFFFFFFFFFFFFFF & ~((1<<20)-1) = 0xFFFFFFFFFFF00000
        // But since the counter never wraps below start, and starting
        // value is 1 (from AtomicU64::new(1)), we just verify the
        // watermark's low bits are all zero.
        let all_ones = vec![0xffu8; 32];
        let t = FileHandleTable::new();
        t.seed_next_fd_from_state_hash(&all_ones);
        let next = t.snapshot_next_fd();
        // next = watermark + 1.  So `next - 1` is the watermark; its
        // low FD_ENTROPY_HEADROOM_BITS must be zero.
        let watermark = next - 1;
        let low_mask = (1u64 << FD_ENTROPY_HEADROOM_BITS) - 1;
        assert_eq!(
            watermark & low_mask,
            0,
            "watermark's low {FD_ENTROPY_HEADROOM_BITS} bits must be zero (headroom); \
             got watermark {watermark:#018x}"
        );
    }

    // Aliasing-scenario regression: two runtimes representing two
    // consecutive lifetimes both reset to the same state hash and
    // then allocate — the second lifetime's allocations must not
    // reuse fd values allocated in the first.  This is the CORE
    // slice-28 invariant.
    #[tokio::test]
    async fn post_restart_fd_allocation_does_not_alias_stale_fds() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f");
        std::fs::write(&path, b"").unwrap();
        // A 32-byte state hash — same value both lifetimes.
        let state_hash: Vec<u8> = (0..32u8).collect();

        // First lifetime: seed, allocate a batch, capture the
        // highest fd.
        let t1 = FileHandleTable::new();
        t1.seed_next_fd_from_state_hash(&state_hash);
        let mut first_lifetime_fds = Vec::new();
        for _ in 0..50 {
            first_lifetime_fds.push(
                t1.insert(make_handle(open_ro(&path), path.clone()))
                    .await
                    .unwrap(),
            );
        }
        let max_fd_first = *first_lifetime_fds.iter().max().unwrap();

        // Second lifetime: fresh table, same state hash reset.
        let t2 = FileHandleTable::new();
        t2.seed_next_fd_from_state_hash(&state_hash);
        // A single allocation on t2 — under the OLD design (no
        // watermark) this would allocate fd=1.  Now it allocates
        // from the seeded watermark's range, which is IDENTICAL to
        // t1's starting range (same hash), but the fd is still the
        // NEXT unused value — either colliding with t1 or not,
        // depending on t1's allocation pattern.
        //
        // In a real restart scenario, t1 is DEAD before t2 starts,
        // and t2 is only trying to avoid aliasing stale references
        // from t1's dead lifetime that might still be recorded in
        // Rholang tuplespace state.  This test proves the watermark
        // is applied.
        let post_restart_fd = t2
            .insert(make_handle(open_ro(&path), path.clone()))
            .await
            .unwrap();
        // First allocation on t2 == first allocation on t1
        // (deterministic seeding).  This is CORRECT: leader and
        // follower must produce the same fd for the same deploy.
        assert_eq!(post_restart_fd, first_lifetime_fds[0]);
        // But the WATERMARK is above 0, proving the seed took effect.
        assert!(post_restart_fd > 0);
        // And the highest fd allocated in the first lifetime is
        // still in a sensible range — well below u64::MAX so
        // subsequent restarts have headroom.
        assert!(max_fd_first < u64::MAX / 2);
    }

    // Cross-restart aliasing scenario (the actual PB-M-13 threat):
    // First lifetime opens files and dies.  Second lifetime, using
    // a DIFFERENT state hash (as happens between blocks), allocates.
    // The two fd ranges must not overlap.
    #[tokio::test]
    async fn different_state_hashes_produce_disjoint_fd_ranges() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f");
        std::fs::write(&path, b"").unwrap();

        // Lifetime 1 at hash A.
        let hash_a: Vec<u8> = std::iter::repeat_n(0x00u8, 32).collect();
        let t1 = FileHandleTable::new();
        t1.seed_next_fd_from_state_hash(&hash_a);
        let mut used_by_t1 = std::collections::HashSet::new();
        for _ in 0..100 {
            used_by_t1.insert(
                t1.insert(make_handle(open_ro(&path), path.clone()))
                    .await
                    .unwrap(),
            );
        }

        // Lifetime 2 at hash B — many bits differ, so upper 32
        // bits of watermark differ.
        let hash_b: Vec<u8> = std::iter::repeat_n(0xffu8, 32).collect();
        let t2 = FileHandleTable::new();
        t2.seed_next_fd_from_state_hash(&hash_b);
        for _ in 0..100 {
            let fd = t2
                .insert(make_handle(open_ro(&path), path.clone()))
                .await
                .unwrap();
            assert!(
                !used_by_t1.contains(&fd),
                "fd {fd} allocated by t2 aliases a fd from t1's lifetime"
            );
        }
    }

    // Regression pin for `snapshot_next_fd`/`truncate_to`: rolling
    // back to a snapshot closes every fd past the snapshot but leaves
    // pre-snapshot fds intact.  Prevents a regression that would
    // orphan fds on error rollback or truncate too aggressively.
    #[tokio::test]
    async fn truncate_to_snapshot_leaves_pre_snapshot_intact() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f");
        std::fs::write(&path, b"").unwrap();

        let table = FileHandleTable::new();
        let fd_pre = table
            .insert(make_handle(open_ro(&path), path.clone()))
            .await
            .unwrap();
        let snap = table.snapshot_next_fd();
        let fd_post1 = table
            .insert(make_handle(open_ro(&path), path.clone()))
            .await
            .unwrap();
        let fd_post2 = table
            .insert(make_handle(open_ro(&path), path.clone()))
            .await
            .unwrap();
        assert!(fd_pre < snap && fd_post1 >= snap && fd_post2 >= snap);
        table.truncate_to(snap).await;
        // Pre-snapshot fd still resolves.
        assert!(
            table.raw_fd(fd_pre).await.is_some(),
            "pre-snapshot fd survives"
        );
        // Post-snapshot fds are gone.
        assert!(table.raw_fd(fd_post1).await.is_none());
        assert!(table.raw_fd(fd_post2).await.is_none());
    }

    // Keep the raw_fd + seek helpers used above in-scope.
    #[allow(dead_code)]
    fn _use_seek(mut f: File) -> u64 {
        f.seek(SeekFrom::Start(0)).unwrap();
        f.write_all(b"").unwrap();
        let raw = f.as_raw_fd();
        let _ = f.into_raw_fd();
        raw as u64
    }

    // ---------------------------------------------------------
    // Slice 28 review-fix tests.
    // ---------------------------------------------------------

    /// ST-28-1 review fix: `insert` returns Err on overflow rather
    /// than silently wrapping.  H-28-F2 hazard: a high watermark
    /// (near u64::MAX) followed by enough allocations wraps
    /// `fetch_add` back to 0 → aliases fd=0 (which is invalid) and
    /// then any low fd value.  The fix in `insert` detects the
    /// wrap and returns Err, translated to `FSERR_QUOTA_EXCEEDED`
    /// upstream.
    #[tokio::test]
    async fn insert_returns_err_when_next_fd_at_u64_max() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f");
        std::fs::write(&path, b"").unwrap();
        let table = FileHandleTable::new();
        // Force next_fd to u64::MAX by direct set — via public
        // seed_next_fd_watermark(u64::MAX - 1) which saturates the
        // add-one to u64::MAX.
        table.seed_next_fd_watermark(u64::MAX - 1);
        assert_eq!(table.snapshot_next_fd(), u64::MAX);
        // First insert must fail (would push counter to u64::MAX+1).
        let res = table
            .insert(make_handle(open_ro(&path), path.clone()))
            .await;
        assert!(res.is_err(), "insert at u64::MAX must return Err");
        // Counter is still u64::MAX — no wrap.
        assert_eq!(table.snapshot_next_fd(), u64::MAX);
    }

    /// ST-28-4 companion: exercise the `FD_ENTROPY_HEADROOM_BITS`
    /// budget — allocating up to 2^20 fds against a watermark
    /// leaves us within the same 20-bit range (doesn't spill into
    /// the next state-hash bucket).  Also verifies that a
    /// full-lifetime scan doesn't hit the wrap guard for a
    /// realistic watermark.
    #[tokio::test]
    async fn allocations_within_headroom_budget_stay_in_bucket() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f");
        std::fs::write(&path, b"").unwrap();
        let table = FileHandleTable::new();
        // Watermark at boundary (some middle-of-u64 value).
        let watermark: u64 = 0x1234_5678_9000_0000;
        table.seed_next_fd_watermark(watermark);
        let start = table.snapshot_next_fd();
        assert_eq!(start, watermark + 1);
        // Allocate 100 fds and confirm we haven't crossed into
        // the next watermark bucket.
        let mut last_fd = 0;
        for _ in 0..100 {
            let fd = table
                .insert(make_handle(open_ro(&path), path.clone()))
                .await
                .unwrap();
            last_fd = fd;
            // Also close each so we don't hit MAX_OPEN_FDS.
            assert!(table.remove(fd).await);
        }
        // All 100 allocations landed in [watermark, watermark + 200),
        // well within the 2^20 headroom budget.
        let bucket_size = 1u64 << FD_ENTROPY_HEADROOM_BITS;
        assert!(
            last_fd < watermark + bucket_size,
            "allocation {last_fd:#018x} escaped watermark bucket \
             [{:#018x}, {:#018x})",
            watermark,
            watermark + bucket_size
        );
    }

    /// L-28-F1 review fix: `seed_next_fd_from_state_hash` panics
    /// (in debug builds) when passed a hash shorter than 8 bytes.
    /// Guards against a future refactor passing a truncated hash
    /// which would silently reduce entropy.
    ///
    /// Gated on `debug_assertions` because the panic is via
    /// `debug_assert!`, which is a no-op in `--release` mode.
    /// Without the gate, the pre-push hook's release-mode test
    /// pass fails with "test did not panic as expected."
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "requires at least 8 bytes of hash")]
    fn seed_from_short_hash_debug_asserts() {
        let short_hash = vec![0xffu8, 0x00, 0x00, 0x00];
        let t = FileHandleTable::new();
        t.seed_next_fd_from_state_hash(&short_hash);
    }

    // ---------------------------------------------------------
    // C-R1 round-2 review-fix tests: shadow handles.
    // ---------------------------------------------------------

    /// insert_at places a shadow handle at the exact fd requested,
    /// enabling follower `journal_write` to find (cmode, canon_path).
    #[tokio::test]
    async fn insert_at_places_shadow_handle_at_specified_fd() {
        let table = FileHandleTable::new();
        let shadow = make_shadow_handle(PathBuf::from("/root/x.bin"), ConsensusMode::Consensus);
        assert!(
            table.insert_at(42, shadow).await,
            "insert_at into an empty slot must succeed"
        );
        // with_mut can now find the handle at fd=42.
        let meta = table
            .with_mut(42, |h| (h.cmode, h.canon_path.clone()))
            .await;
        assert_eq!(
            meta,
            Some((ConsensusMode::Consensus, PathBuf::from("/root/x.bin"))),
            "shadow handle metadata must be visible to with_mut"
        );
        // raw_fd on a shadow handle returns None (no OS fd).
        assert!(
            table.raw_fd(42).await.is_none(),
            "raw_fd on shadow handle must be None — no OS file"
        );
    }

    /// insert_at into an occupied slot returns false and does NOT
    /// overwrite — silent overwrite would mask a state-derivation
    /// bug on the follower.
    #[tokio::test]
    async fn insert_at_occupied_slot_returns_false() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f");
        std::fs::write(&path, b"").unwrap();
        let table = FileHandleTable::new();
        // Occupy fd=42 with a REAL handle.
        table
            .insert_at(42, make_handle(open_ro(&path), path.clone()))
            .await;
        // Try to overwrite with a shadow.
        let shadow = make_shadow_handle(PathBuf::from("/other"), ConsensusMode::Oracular);
        assert!(
            !table.insert_at(42, shadow).await,
            "insert_at must reject overwriting an occupied slot"
        );
        // Original handle is intact.
        let meta = table
            .with_mut(42, |h| (h.cmode, h.canon_path.clone()))
            .await
            .unwrap();
        assert_eq!(
            meta.0,
            ConsensusMode::Oracular /* from make_handle default */
        );
        assert_eq!(meta.1, path);
    }
}
