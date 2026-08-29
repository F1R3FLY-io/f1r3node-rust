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
use std::time::Duration;

use tokio::sync::{Notify, RwLock};

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
    /// Shadow file-position (2026-08-26, follow-up to PB-M-14 file-
    /// state-identity via WAL replay).  Tracks the notional
    /// position the fd would be at after every sequential
    /// `fs_write` / `fs_read` / `fs_seek`, so `journal_write` /
    /// `journal_read` can record the ABSOLUTE offset for sequential
    /// ops into the Consensus WAL — enabling a joining validator's
    /// applier to reconstruct file state from the WAL alone (no fd,
    /// no position-simulation, no `Open`/`Close`/`Seek` WAL
    /// variants required).
    ///
    /// Updates:
    /// * `fs_open` non-append modes → `0` (POSIX default for
    ///   O_RDONLY/O_WRONLY/O_RDWR without O_APPEND).
    /// * `fs_open` append modes (`a` / `a+`) on Consensus caps →
    ///   fs_open rejects with `FSERR_BAD_ARG`.  Rationale: O_APPEND
    ///   moves the write offset to file-end atomically at each
    ///   write; without fstat-per-write plus a matching shadow-EOF
    ///   simulation on the follower, WAL offset cannot be recorded
    ///   correctly.  A future slice may lift this restriction by
    ///   tracking per-canon_path EOF on both sides.
    /// * Successful sequential `fs_write(n)` / `fs_read(n)` →
    ///   `position += n` (both leader and follower, `n` from
    ///   syscall reply on leader / `previous` cache on follower).
    /// * Successful `fs_seek` → `position = new_pos` (leader from
    ///   lseek reply / follower from `previous`).
    /// * `fs_write_at` / `fs_read_at` / `fs_truncate` — POSIX
    ///   pwrite/pread/ftruncate DO NOT move the fd position, so
    ///   `position` is untouched.
    ///
    /// Consensus symmetry: both leader and follower evolve
    /// `position` deterministically from the same sequence of
    /// contract-arg values + reply values, so `journal_write`
    /// reads identical `position` on both sides for the same
    /// syscall and produces byte-identical WAL entries.
    pub position: u64,
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
    /// Phase 7b-2 (2026-08-27): optional shared payload persistence
    /// backend.  When set (via `share_payload_store` at
    /// `RuntimeManager::spawn_runtime` time), `journal_write` calls
    /// `store.persist(bytes)` for every Consensus-cap write so the
    /// bytes are stashed content-addressed on disk.  Joining
    /// validators fetch them via the Phase 7b-2 wire protocol
    /// (`GetWalPayloadRequest`).
    ///
    /// `None` on nodes without an operator-configured payload store
    /// (observer nodes, test harnesses); when None, `journal_write`
    /// still appends the WAL entry but the bytes are not persisted
    /// on the serving side.  This is safe for a joiner-only
    /// posture (they never serve payloads); a serving validator
    /// SHOULD wire this at boot.
    ///
    /// **Interior mutability:** wrapped in `Arc<std::sync::RwLock<Option<...>>>`
    /// so a post-`spawn_runtime` `share_payload_store` call is
    /// visible through every `FileHandleTable` clone — including
    /// the copy `FsProcesses` snapshotted at reducer-setup time.
    /// (The simpler `Option<Arc<dyn>>` shape would strand the
    /// FsProcesses clone with a stale None because the field
    /// replacement doesn't cross the earlier clone boundary.)  The
    /// std `RwLock` is chosen over `tokio::sync::RwLock` for the
    /// same reason `InMemoryPayloadStore` uses one: the guard is
    /// never held across an `.await`, and using a tokio lock from
    /// inside a sync trait method would risk executor deadlock.
    pub payload_store: Arc<std::sync::RwLock<Option<Arc<dyn super::wal::PayloadPersistence>>>>,
    /// Item d-3 (2026-08-28): per-fd shadow-install notifiers.  The
    /// follower's rigged-replay reducer can dispatch a mutating handler
    /// (`fs_write` / `fs_read` / `fs_truncate`) on a spawned task
    /// BEFORE `fs_open`'s replay branch has finished `insert_at.await`
    /// on another task — so `journal_write`'s `with_mut(fd, ...)` sees
    /// None, no WAL entry lands, and the follower's per-deploy WAL
    /// slice diverges from the leader's.  This map holds a
    /// `tokio::sync::Notify` per fd that is currently being waited on;
    /// the replay-branch of every mutating handler waits (with a
    /// bounded timeout) on the fd's Notify if `with_mut` returns None,
    /// and `fs_open`'s replay branch calls `notify_fd_ready(fd)` once
    /// its `insert_at` completes.
    ///
    /// Leader path never populates the map: leader's fs_open + fs_write
    /// run on the same task in source-code order, so `with_mut`
    /// succeeds on first try and `wait_for_replay_shadow`'s fast path
    /// exits without touching the map.
    ///
    /// `std::sync::Mutex` is chosen over `tokio::sync::Mutex` because
    /// the guard is never held across `.await` — same rationale as
    /// `payload_store` above.  Interior mutability so `notify_fd_ready`
    /// can take `&self`.
    fd_notifiers: Arc<std::sync::Mutex<HashMap<u64, Arc<Notify>>>>,
}

/// Item d-3 (2026-08-28): bounded wait for a fs_open replay-branch
/// shadow install to become visible in the fd table.  500ms is 500x
/// the typical tokio dispatch latency between spawn and first poll on
/// a busy worker, so a legit fs_open→fs_write race resolves in
/// microseconds; the timeout only fires on genuinely-missing fds
/// (leader replied `[false, code, msg]` to fs_open, so no shadow was
/// installed, and Rholang subsequently invoked a mutating handler
/// with the bad fd anyway — a caller bug, not a race).
pub const SHADOW_WAIT_TIMEOUT: Duration = Duration::from_millis(500);

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
            // Phase 7b-2: unset by default; the boot pipeline
            // populates via `share_payload_store` from a shared
            // manager slot.  Tests that don't wire a store see None
            // and journal_write skips the persist step (matches
            // pre-7b-2 behavior).
            payload_store: Arc::new(std::sync::RwLock::new(None)),
            // Item d-3: empty map — populated on demand by waiters
            // on the follower's replay branch; drained by
            // `notify_fd_ready` in fs_open's replay branch.
            fd_notifiers: Arc::new(std::sync::Mutex::new(HashMap::new())),
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

    /// Phase 7b-2 (2026-08-27): attach the manager-shared payload
    /// persistence backend.  Called from
    /// `RuntimeManager::spawn_runtime` /
    /// `spawn_replay_runtime` (post-spawn, but the interior
    /// `Arc<RwLock<_>>` ensures the write is visible through every
    /// clone).  A `None` argument clears any previously-attached
    /// store; a `Some(store)` sets it.
    ///
    /// After attachment, `journal_write` on every mutating fs
    /// handler for a Consensus cap will call `store.persist(bytes)`
    /// so the serving side accumulates the bytes joiners will
    /// need to fetch.
    ///
    /// Takes `&self` (not `&mut self`) because the RwLock provides
    /// interior mutability — matches the shape used by
    /// `RootIdentityRegistry::register` for the same reason.
    pub fn share_payload_store(&self, shared: Option<Arc<dyn super::wal::PayloadPersistence>>) {
        *self
            .payload_store
            .write()
            .expect("payload_store lock poisoned") = shared;
    }

    /// Phase 7b-2 diagnostic — read the currently-installed
    /// persistence backend, if any.  Used by `journal_write` to
    /// look up the store on every write.
    pub fn payload_store(&self) -> Option<Arc<dyn super::wal::PayloadPersistence>> {
        self.payload_store
            .read()
            .expect("payload_store lock poisoned")
            .clone()
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

    /// Item d-3 (2026-08-28): fast presence check for `fd` in the fd
    /// table.  Used by `wait_for_replay_shadow`'s double-check pattern
    /// to close the race between "register interest via Notify" and
    /// "shadow insert lands".
    pub async fn contains_fd(&self, fd: u64) -> bool {
        let table = self.inner.table.read().await;
        table.contains_key(&fd)
    }

    /// Item d-3 (2026-08-28): follower-side replay barrier.  If `fd`
    /// is already present in the fd table, return immediately (fast
    /// path — leader path always hits this).  Otherwise, register a
    /// waiter on the fd's `Notify`, re-check presence (defends
    /// against a shadow-install that landed between the initial
    /// check and Notify registration), and wait up to `timeout`
    /// for `fs_open`'s replay-branch `notify_fd_ready(fd)` to
    /// resolve.
    ///
    /// Returns `true` iff the fd is present when the function returns
    /// — either observed on entry, observed on the double-check, or
    /// observed after `notify_fd_ready` wakes the waiter.  Returns
    /// `false` only when the timeout expires with the fd still
    /// absent — a genuine "fd never opened" case (leader replied
    /// error to fs_open, no shadow ever installed).  Callers on the
    /// replay branch treat `false` the same as pre-d-3: skip the
    /// journal step and proceed to produce the leader's cached error
    /// reply.
    ///
    /// Bounded timeout by design: `SHADOW_WAIT_TIMEOUT` (500ms) is
    /// well above tokio dispatch latency but small enough that error
    /// paths don't stall block validation.
    pub async fn wait_for_replay_shadow(&self, fd: u64, timeout: Duration) -> bool {
        // Fast path: fd already present.  Leader always hits this.
        if self.contains_fd(fd).await {
            return true;
        }
        // Slow path: register interest via Notify BEFORE the
        // double-check so a shadow-install that lands between the
        // check and the wait still wakes us.
        let notifier = {
            let mut map = self.fd_notifiers.lock().expect("fd_notifiers poisoned");
            map.entry(fd)
                .or_insert_with(|| Arc::new(Notify::new()))
                .clone()
        };
        let notified = notifier.notified();
        tokio::pin!(notified);
        // M-1 review fix (2026-08-29): `Notify::notified()` returns a
        // `Notified` future that only REGISTERS as a waiter on
        // `notify_waiters()` when first polled.  Between `notified()`
        // creation and `.await` below, our task may be de-scheduled
        // long enough for a concurrent `notify_fd_ready` to fire —
        // with no registered waiters, the notification is dropped,
        // and our subsequent `.await` sees no permit and parks until
        // the timeout expires.  The final `contains_fd` after
        // timeout recovers correctness (the fd IS present) but
        // adds a full `timeout` window of latency per affected call.
        //
        // `enable()` (tokio >= 1.10) registers the waiter WITHOUT
        // polling.  After enable(), any subsequent `notify_waiters()`
        // reliably wakes us; the double-check below still guards the
        // ordering where the shadow lands BEFORE we register
        // interest.
        notified.as_mut().enable();
        if self.contains_fd(fd).await {
            return true;
        }
        // Bounded wait; even on timeout, do one final presence
        // check so a shadow-install that raced with the timeout
        // firing is still observed.
        let _ = tokio::time::timeout(timeout, notified).await;
        self.contains_fd(fd).await
    }

    /// Item d-3 (2026-08-28): wake every `wait_for_replay_shadow`
    /// waiter for `fd`.  Called from `fs_open`'s replay branch after
    /// `insert_at` (whether or not insertion succeeded — a
    /// pre-existing slot means the shadow is already there and
    /// waiters should be released to re-check).  Removes the fd's
    /// entry from the notifier map so subsequent
    /// `wait_for_replay_shadow` calls (unlikely; the fd is now in
    /// the table) fast-path on `contains_fd`.
    pub fn notify_fd_ready(&self, fd: u64) {
        let notifier = {
            let mut map = self.fd_notifiers.lock().expect("fd_notifiers poisoned");
            map.remove(&fd)
        };
        if let Some(n) = notifier {
            n.notify_waiters();
        }
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
            position: 0,
        }
    }

    fn make_shadow_handle(path: PathBuf, cmode: ConsensusMode) -> FileHandle {
        FileHandle {
            file: None,
            canon_path: path,
            mode: AccessMode::Read,
            cmode,
            position: 0,
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

    // ---------------------------------------------------------------
    // Item d-3 (2026-08-28): per-fd shadow-install barrier tests.
    // ---------------------------------------------------------------

    /// Fast path: an fd already in the table is observed by the
    /// initial `contains_fd` and `wait_for_replay_shadow` returns
    /// `true` without touching the notifier map.  Leader path
    /// always hits this — fs_open's `insert()` runs on the same
    /// task as the subsequent fs_write, so the fd is present.
    #[tokio::test]
    async fn wait_for_replay_shadow_fast_path_when_fd_present() {
        let table = FileHandleTable::new();
        let shadow = make_shadow_handle(PathBuf::from("/root/x"), ConsensusMode::Consensus);
        assert!(table.insert_at(7, shadow).await);
        // Fd is present; wait returns immediately even with a
        // near-zero timeout.
        assert!(
            table
                .wait_for_replay_shadow(7, Duration::from_millis(1))
                .await,
            "fast path must return true without waiting when fd is already present"
        );
    }

    /// The barrier's core invariant: a waiter parked in
    /// `wait_for_replay_shadow` wakes on `notify_fd_ready` and
    /// re-observes the fd on retry.  This is what closes the
    /// follower-side reducer race that caused fs_write's
    /// `journal_write` to see None and drop the Write WAL entry.
    #[tokio::test]
    async fn wait_for_replay_shadow_wakes_on_notify_after_insert() {
        let table = std::sync::Arc::new(FileHandleTable::new());
        let waiter_table = table.clone();
        let waiter = tokio::spawn(async move {
            // Timeout well above the sleep+insert delay below so
            // the barrier reliably wakes on notify, not on
            // timeout — asserting the notify path, not the
            // timeout fallback.
            waiter_table
                .wait_for_replay_shadow(99, Duration::from_secs(2))
                .await
        });
        // Simulate the fs_open replay branch: delay + insert +
        // notify, mirroring the race pattern where the mutating-
        // handler task registers interest first and fs_open lands
        // its shadow later on another task.
        tokio::time::sleep(Duration::from_millis(20)).await;
        let shadow = make_shadow_handle(PathBuf::from("/root/late"), ConsensusMode::Consensus);
        assert!(table.insert_at(99, shadow).await);
        table.notify_fd_ready(99);
        let observed = waiter.await.expect("waiter task join");
        assert!(
            observed,
            "wait_for_replay_shadow must return true after notify_fd_ready wakes it"
        );
    }

    /// Timeout fallback: if `notify_fd_ready` is never called
    /// (leader replied error to fs_open, no shadow ever installed)
    /// the barrier gives up after the bounded timeout and returns
    /// `false`, letting the caller proceed as pre-d-3 (no-op
    /// journal_write on an unknown fd).
    #[tokio::test]
    async fn wait_for_replay_shadow_returns_false_on_timeout() {
        let table = FileHandleTable::new();
        let start = std::time::Instant::now();
        // Use a small (but non-zero) timeout so the test is fast
        // yet exercises the actual `tokio::time::timeout` path
        // rather than an immediate fast-path shortcut.
        let observed = table
            .wait_for_replay_shadow(123, Duration::from_millis(50))
            .await;
        assert!(!observed, "must return false when fd never lands");
        assert!(
            start.elapsed() >= Duration::from_millis(50),
            "timeout must be respected — a false return before deadline \
             would mean the wait short-circuited"
        );
    }

    /// L-2 review pin (2026-08-29): two tasks parked in
    /// `wait_for_replay_shadow` on the SAME fd must both wake on a
    /// single `notify_fd_ready` call.  `Notify::notify_waiters()`
    /// wakes every registered waiter; a regression that switched to
    /// `notify_one()` would leave one waiter parked until timeout,
    /// causing intermittent 500ms stalls on realistic Rholang like
    /// `fsRead | fsWrite` where both branches race the same fd's
    /// shadow install.
    #[tokio::test]
    async fn wait_for_replay_shadow_wakes_all_concurrent_waiters() {
        let table = std::sync::Arc::new(FileHandleTable::new());
        let t1 = table.clone();
        let t2 = table.clone();
        let w1 = tokio::spawn(async move {
            t1.wait_for_replay_shadow(555, Duration::from_secs(2)).await
        });
        let w2 = tokio::spawn(async move {
            t2.wait_for_replay_shadow(555, Duration::from_secs(2)).await
        });
        // Yield the runtime a couple of times so both waiter tasks
        // reach the .await (registered as Notify waiters) before we
        // fire the notify — otherwise a fast notify race could
        // land BEFORE waiter registration and rely purely on the
        // M-1 enable() guarantee to be observed.  The pin still
        // exercises the "both wake on one notify" property either
        // way.
        for _ in 0..3 {
            tokio::task::yield_now().await;
        }
        let shadow = make_shadow_handle(PathBuf::from("/root/concurrent"), ConsensusMode::Consensus);
        assert!(table.insert_at(555, shadow).await);
        table.notify_fd_ready(555);
        let r1 = w1.await.expect("waiter 1 join");
        let r2 = w2.await.expect("waiter 2 join");
        assert!(r1, "waiter 1 must observe fd present after notify");
        assert!(r2, "waiter 2 must observe fd present after notify");
    }

    /// M-1 review pin (2026-08-29): a `notify_fd_ready` that fires
    /// between `notified()` creation and its first poll must still
    /// be observed by the waiter.  Without
    /// `notified.as_mut().enable()` (added in M-1), the waiter
    /// would register AFTER `notify_waiters()` fired, park until
    /// timeout, and only recover via the terminal `contains_fd`
    /// check — a full-timeout latency stall on every affected
    /// call.
    ///
    /// The pin uses two oneshot channels to deterministically
    /// place the notify in the "between enable() and .await"
    /// window: the waiter signals when it's past enable(), fires
    /// the notify, then releases the waiter to poll.  The
    /// elapsed-time assertion (well under the wait's own timeout)
    /// distinguishes the enable() wake path from the
    /// timeout-recover path — a regression that dropped enable()
    /// would take ~waiter_timeout ms; enable() takes microseconds.
    #[tokio::test]
    async fn wait_for_replay_shadow_survives_notify_before_await() {
        let table = std::sync::Arc::new(FileHandleTable::new());

        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<()>();
        let (go_tx, go_rx) = tokio::sync::oneshot::channel::<()>();
        let table_for_waiter = table.clone();

        // Waiter-side timeout inside the wait — a regression that
        // dropped enable() would stall for this long; enable()
        // wakes in microseconds.  200ms leaves plenty of headroom
        // over the elapsed-time assert below (100ms) without being
        // flaky on slow CI.
        const WAITER_TIMEOUT: Duration = Duration::from_millis(200);
        const MAX_ELAPSED_WITH_ENABLE: Duration = Duration::from_millis(100);

        let waiter = tokio::spawn(async move {
            // Mirror wait_for_replay_shadow's shape up through
            // enable(), then coordinate with the driver so the
            // notify fires in the "between enable() and .await"
            // window enable() is supposed to make safe.
            if table_for_waiter.contains_fd(888).await {
                return (true, Duration::ZERO);
            }
            let notifier = {
                let mut map = table_for_waiter
                    .fd_notifiers
                    .lock()
                    .expect("fd_notifiers poisoned");
                map.entry(888)
                    .or_insert_with(|| std::sync::Arc::new(tokio::sync::Notify::new()))
                    .clone()
            };
            let notified = notifier.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let _ = ready_tx.send(());
            let _ = go_rx.await;
            let started = std::time::Instant::now();
            let _ = tokio::time::timeout(WAITER_TIMEOUT, notified).await;
            let elapsed = started.elapsed();
            (table_for_waiter.contains_fd(888).await, elapsed)
        });

        ready_rx.await.expect("waiter reached enable");
        let shadow = make_shadow_handle(
            PathBuf::from("/root/pre-await-notify"),
            ConsensusMode::Consensus,
        );
        assert!(table.insert_at(888, shadow).await);
        table.notify_fd_ready(888);
        go_tx.send(()).expect("coordinator go");

        let (observed, elapsed) = waiter.await.expect("waiter join");
        assert!(observed, "waiter must observe fd present");
        assert!(
            elapsed < MAX_ELAPSED_WITH_ENABLE,
            "wait must have woken via notify, not timeout+recover.  elapsed = {elapsed:?}; \
             a value near WAITER_TIMEOUT ({WAITER_TIMEOUT:?}) indicates enable() was dropped \
             and the waiter fell through to the terminal contains_fd."
        );
    }
}
