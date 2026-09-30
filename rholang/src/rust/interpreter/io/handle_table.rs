// File-descriptor table — slice 1 of the `handle_table` submodule
// tree.
//
// Per-runtime `Arc<Inner>` holding an `Arc<tokio::sync::RwLock<
// HashMap<u64, FileHandle>>>` mapping opaque `u64` fds to open
// `File` handles.  Fds are monotonic — the counter never rewinds
// — so a closed fd is never reused.  This preserves the invariant
// that a stale fd reliably observes `FSERR_CLOSED` rather than
// aliasing a later-opened file.
//
// # This slice + prior slices provide
//
//   - `FileHandle` — the per-fd metadata record (owning
//     `Arc<File>` OR a shadow-handle `None` for follower replay).
//   - `FileHandleTable` — the runtime-wide cloneable handle with
//     the private `Inner` allocator + map, PLUS `wal`, the
//     deploy-scope cells, and the payload trait-object slots.
//   - `new` / `default` / `insert` / `insert_at` / `remove` /
//     `raw_fd` / `with_mut` (slice 1).
//   - `wal: Wal` public field + WAL journaling access (slice 2).
//   - `current_deploy_scope` / `current_deploy_sig` cells + their
//     poison-aborted accessors (slice 2).
//   - `payload_store` / `payload_source_recorder` cells + their
//     `share_*` / getter methods (slice 3).
//   - `root_registry: RootIdentityRegistry` public field +
//     `share_root_registry` broadcast method (slice 4).
//   - Soft-checkpoint machinery (`snapshot_next_fd` /
//     `seed_next_fd_watermark` / `seed_next_fd_from_state_hash` /
//     `truncate_to`) + its `FD_ENTROPY_HEADROOM_BITS` const-
//     assert (slice 8).
//
// # Deferred to later `handle_table` slices
//
//   - `lock_registry: LockRegistry` + `share_lock_registry` —
//     gated on `LockRegistry`.
//   - `dir_handles: DirHandleTable` + `share_dir_handles` —
//     gated on `DirHandleTable`.
//   - `close_all_for_deploy` / `has_active_handles_sync` — gated
//     on `LockRegistry` for the symmetric sweep interface.
//
// Each deferred field / method group has its own natural
// dependency:  `lock_registry` waits for `lock.rs`'s
// `LockRegistry` slice; `dir_handles` waits for
// `dir_handle_table.rs`; sharing methods wait for
// `RootIdentityRegistry::share_from` in a future `path::identity`
// slice.  Landing the fd allocator alone now unblocks the
// FileHandle *construction* path in Wave 4 handlers even while
// the higher-order plumbing catches up.
//
// # Fd namespace: NOT unique across FileHandleTable +
//   DirHandleTable
//
// File fds and directory-stream fds (once `dir_handle_table.rs`
// lands) will live in **separate** tables and can share numeric
// values.  The Rholang layer routes each fd to the correct native
// (`fs_close` vs. `fs_entries_stream_close`, etc.) based on the
// URN that produced the fd — a fd from `fs_open` goes to file
// handlers, a fd from `entriesStreamOpen` goes to dir-stream
// handlers.  The Rust handler layer looks up in the corresponding
// table.
//
// A bug that routed a file fd to a dir-stream handler (or vice
// versa) would produce `FSERR_CLOSED` — the handler's own table
// lookup fails.  Sound at the boundary but subtle for anyone
// reading `fd: u64` in isolation.  Refactor guardrail: if a
// future slice unifies these into `HandleTable<T>` with a
// discriminator tag, migrate carefully — every URN dispatch site
// assumes the two tables are disjoint namespaces.

use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

// `tokio::sync::RwLock` rather than `std::sync::RwLock` — the
// load-bearing reason is that `std::sync::RwLockReadGuard` /
// `RwLockWriteGuard` are `!Send`, so a caller wanting to hold a
// guard across an `.await` boundary (which every `insert` /
// `remove` / `raw_fd` / `with_mut` caller does, since they're all
// `async fn`) requires tokio's variant.  A secondary benefit:
// `spawn_blocking` closures don't need to acquire the guard —
// callers snapshot the metadata they need via `with_mut` /
// `raw_fd` BEFORE the closure and hand the values in.  Tokio's
// `RwLock` also does not poison, so there's no `poison_abort`
// equivalent to route through here (contrast `Wal`'s std-lock
// design in `wal.rs`).
//
// The **deploy-scope cells** below (`current_deploy_scope`,
// `current_deploy_sig`) use `std::sync::RwLock` instead — they're
// updated at deploy-entry / deploy-drop from synchronous
// `WalDeployScope::new` / `Drop` code that isn't inside an async
// context.  Their accessors route through `poison_abort` to match
// the DD-FailClosedOnInvariantBreak discipline.
use tokio::sync::RwLock;

use super::errors::poison_abort;
use super::lock::DeployScope;
use super::mode::AccessMode;
use super::path::identity::RootIdentityRegistry;
use super::wal::{PayloadPersistence, PayloadSourceRecorder, Wal};
use super::ConsensusMode;

/// Number of low bits reserved as per-lifetime fd-allocation
/// headroom below the state-hash-derived watermark.  2^20 ≈ 1M
/// fds is safely above `MAX_OPEN_FDS = 1024` live fds and any
/// realistic open+close cycle count within a single
/// block-computation.  A `const` assertion below enforces that
/// `MAX_OPEN_FDS` cannot silently exceed this budget on a future
/// change.
///
/// # CONSENSUS-OBSERVABLE
///
/// This constant governs the state-hash → fd-watermark derivation
/// in [`FileHandleTable::seed_next_fd_from_state_hash`].  Fd
/// values live in Rholang tuplespace state (`File`-agent `fdP`
/// cells → on-chain via the state hash), so validators running a
/// different value here would compute a different watermark for
/// the same state hash → different fd allocation sequence →
/// different tuplespace state.  A partial-upgrade fleet would
/// silently fork.
///
/// Registered into `CONSENSUS_FOLD` at order 14 (below); the
/// fingerprint fold advertises the constant to peering handshakes
/// so a mismatched validator fails the `network_id` check at boot
/// rather than silently forking.
const FD_ENTROPY_HEADROOM_BITS: u32 = 20;

// Compile-time invariant guard.  If a future change raises
// `MAX_OPEN_FDS` past the entropy-headroom budget, the build
// fails here — flagging that `seed_next_fd_from_state_hash`'s
// aliasing protection must be revisited.  The 2× factor is a
// safety margin for open+close cycles vs. concurrent-live fds.
const _: () = assert!(
    (super::MAX_OPEN_FDS as u64) * 2 < 1u64 << FD_ENTROPY_HEADROOM_BITS,
    "MAX_OPEN_FDS exceeds FD_ENTROPY_HEADROOM_BITS budget; \
     the state-hash-derived watermark cannot guarantee aliasing prevention"
);

crate::register_consensus_constant!(order = 14, name = FD_ENTROPY_HEADROOM_BITS, u64_be);

/// Per-fd metadata record.
///
/// # `file: Option<Arc<File>>` — Arc keepalive vs. shadow handles
///
/// The `Arc` wrapper lets `raw_fd()` hand out clones that keep
/// the underlying `File` alive across a `spawn_blocking(libc::
/// read/write/...)` call — the OS fd stays valid until every
/// closure clone drops, defeating the race where a concurrent
/// `remove(fd)` would otherwise close the fd out from under the
/// pending syscall.
///
/// The `Option::None` case is a **shadow handle** — inserted on
/// the follower's `fs_open` replay branch (Wave 4) so subsequent
/// replay-branch mutating handlers can still look up
/// `(cmode, canon_path)` via `with_mut` for symmetric WAL
/// journaling.  The follower never touches `file` on the replay
/// branch (read/write handlers short-circuit on `is_replay = true`
/// and return the cached `previous` reply), so a `None` here is
/// never dereferenced through the syscall path.  `raw_fd()`
/// returns `None` for shadow handles — any code path that reaches
/// for the OS fd on a shadow handle gets `FSERR_CLOSED`, which is
/// the correct failure mode if it ever happens.
///
/// # Metadata-coupling caveat
///
/// The Arc keepalive protects the OS fd only.  The `FileHandle`
/// ENTRY in the table can still be removed via `remove(fd)`
/// while a `spawn_blocking` closure holds an `Arc<File>` clone.
/// Once removed, any handler that reads `FileHandle` fields (like
/// `canon_path`, `mode`, `cmode`, `position`) via a fresh
/// `with_mut` / `raw_fd` lookup would see `None`.  Callers MUST
/// snapshot the metadata they need BEFORE the `spawn_blocking`,
/// or accept that a mid-flight remove races the update.  Under
/// the sequential-deploy invariant, only ONE syscall closure can
/// be in flight per fd at a time, so the race window doesn't
/// materialize under production wiring.
///
/// # Shape A invariant on `canon_path`
///
/// `canon_path` MUST be the RAW `canonicalize_lexical(rholang_canon_
/// root, rel)` — the Rholang-side `canonRoot` as the reducer
/// received it, NOT the output of
/// `RootIdentityRegistry::resolve_or_identity`.  Under Consensus-fs
/// Shape A, Consensus caps carry `canonRoot = BUNDLE_ROOT_PREFIX`
/// so this field is bundle-relative (e.g. `/@bundle/target`); the
/// registry rewrites for the syscall at every handler entry.
/// Downstream WAL journaling copies this field verbatim into
/// `entry.path`, so a Shape A violation here would silently record
/// per-validator absolute paths and break leader/follower WAL
/// byte-identity.
#[derive(Debug)]
pub struct FileHandle {
    /// Underlying OS file wrapped in `Arc`, or `None` for a
    /// shadow handle.
    pub file: Option<Arc<File>>,
    /// See the struct-level "Shape A invariant" section.
    pub canon_path: PathBuf,
    /// Access mode captured at `fs_open` time.
    pub mode: AccessMode,
    /// Per-cap consensus mode captured at `fs_open` time.
    /// Mutating handlers (`fs_write`, `fs_write_at`,
    /// `fs_truncate`) consult this via `handles.with_mut` to
    /// decide whether to journal the op into the consensus WAL.
    pub cmode: ConsensusMode,
    /// Shadow file-position.  Tracks the notional position the
    /// fd would be at after every sequential `fs_write` /
    /// `fs_read` / `fs_seek`.  Consensus symmetry: both leader
    /// and follower evolve `position` deterministically from the
    /// same sequence of contract-arg values + reply values, so
    /// `journal_write` reads identical `position` on both sides
    /// for the same syscall.
    ///
    /// Updates:
    ///  * `fs_open` non-append modes → `0` (POSIX default).
    ///  * `fs_open` append modes (`a` / `a+`) on Consensus caps →
    ///    `fs_open` rejects with `FSERR_BAD_ARG` (`O_APPEND`
    ///    moves the write offset atomically at each write;
    ///    without fstat-per-write plus a matching shadow-EOF
    ///    simulation on the follower, WAL offset cannot be
    ///    recorded correctly).
    ///  * Successful sequential `fs_write(n)` / `fs_read(n)` →
    ///    `position += n`.
    ///  * Successful `fs_seek` → `position = new_pos`.
    ///  * `fs_write_at` / `fs_read_at` / `fs_truncate` do NOT
    ///    move the fd position, so `position` is untouched.
    pub position: u64,
    /// Deploy scope this file was opened under.  Populated at
    /// `fs_open` time from the (yet-to-land) `current_deploy_
    /// scope` cell; consulted by the (yet-to-land)
    /// `close_all_for_deploy` sweep to reclaim files a deploy
    /// left open past its `WalDeployScope::drop`.  Sentinel
    /// `[0u8; 32]` for out-of-deploy opens (test paths only
    /// under normal operation).
    pub deploy: super::lock::DeployScope,
}

/// Per-runtime file-descriptor table.  Cloneable — clones share
/// the underlying `Arc<Inner>` via reference counting, so every
/// handler closure holds an independent handle onto the same
/// underlying map.
///
/// # Slice-2 shape
///
/// The plumbing fields (`root_registry`, `lock_registry`,
/// `dir_handles`, `payload_store`, `payload_source_recorder`)
/// that fileio's `FileHandleTable` carries are DEFERRED to later
/// slices — each is gated on a downstream module landing (or on a
/// `share_*` API needing a helper method that doesn't exist yet).
/// The slice-2 struct adds the two pieces whose downstream
/// dependencies HAVE landed: the `Wal` buffer (PR #490 / #493)
/// and the deploy-scope cells (`DeployScope` type + `poison_abort`
/// helper from PR #488 / #489).
#[derive(Debug, Clone, Default)]
pub struct FileHandleTable {
    inner: Arc<Inner>,
    /// Consensus-mode Write-Ahead Log.  Attached to the handle
    /// table because both are per-runtime state that gets plumbed
    /// identically through the reducer.  Handler closures access
    /// via `self.handles.wal.append_with_ack(...)` etc.  Journal
    /// appends happen inside the fd-based mutating handlers after
    /// successful syscall completion, gated on the `FileHandle`'s
    /// `cmode`.
    ///
    /// Public field (matches fileio's ergonomic idiom) — `Wal`'s
    /// own methods already route through `poison_abort` internally,
    /// so no discipline is bypassed by direct access.
    pub wal: Wal,
    /// Shared root-identity registry.  Populated once at boot from
    /// operator-provisioned root paths; consulted on every
    /// `safe_descend` (or its handler-side wrappers) to detect
    /// post-boot rename-and-recreate of the root directory (H-5
    /// defense).  Attached to the handle table so all handler
    /// closures reach it via `self.handles.root_registry`; shared
    /// across runtimes via `RuntimeManager` so a single boot-time
    /// population is visible everywhere (see
    /// [`share_root_registry`](Self::share_root_registry)).
    ///
    /// Public field (matches fileio's ergonomic idiom) — the
    /// registry's own methods already route through `poison_abort`
    /// internally (see `RootIdentityRegistry` docs), so no
    /// discipline is bypassed by direct access.
    pub root_registry: RootIdentityRegistry,
    /// The per-runtime "current deploy state" — `scope` + `sig`
    /// bundled under a **single** guard.
    ///
    /// # Structural invariant
    ///
    /// Both fields live under one `RwLock`.  The compound writers
    /// `set_current_deploy(scope, sig)` and
    /// `clear_current_deploy()` update both in a single guard
    /// scope, so consumers can NEVER observe a half-populated
    /// pair `(sentinel_scope, non_empty_sig)` or
    /// `(non_sentinel_scope, empty_sig)` even under concurrent
    /// reads.  This makes the "both populated OR both sentinel"
    /// contract *structural* rather than conventional-only.
    ///
    /// The per-field `set_current_deploy_scope` /
    /// `set_current_deploy_sig` setters exist for narrower
    /// callers (tests intentionally exercising partial-state
    /// semantics); production deploy-entry / -drop code SHOULD
    /// use the compound helpers.
    ///
    /// # Cell shape
    ///
    /// `std::sync::RwLock` (not tokio's async lock) — the cell is
    /// set from synchronous `WalDeployScope::new` / `Drop` code,
    /// not from an async context.  Accessors route through
    /// `poison_abort` per DD-FailClosedOnInvariantBreak.
    ///
    /// # Consumers
    ///
    ///   - `scope` (`Blake2b256(deploy.sig)` for user deploys):
    ///     read on lock acquire (Wave 4) and by
    ///     `LockRegistry::release_all_for_deploy` at deploy end.
    ///     Sentinel `[0u8; 32]` between deploys.
    ///   - `sig` (raw signature bytes): read on `journal_write`
    ///     (Wave 4) to index `(payload_hash → deploy_sig)` via
    ///     `PayloadSourceRecorder`.  Empty vec between deploys —
    ///     the record step skips when the cell is empty.
    ///
    /// # Why two fields instead of one hash
    ///
    /// The scope drives lock-sweep scoping; the sig drives the
    /// block-storage `deploy_index` walk (which keys on
    /// `DeployId = shared::ByteString`).  Recording the scope
    /// hash into the payload-source index would break the joiner
    /// chain `payload_hash → deploy_sig → block_hash → block
    /// bytes → ProcessedDeploy`.
    ///
    /// Per-runtime cell (not manager-broadcast): each runtime
    /// processes deploys sequentially, so a single cell suffices;
    /// concurrent runtimes have independent `FileHandleTable`
    /// instances.
    deploy: Arc<std::sync::RwLock<DeployState>>,
    /// Serving-side payload persistence backend (optional).
    /// Populated by the boot pipeline via `share_payload_store`
    /// from a manager-shared slot; tests that don't wire a store
    /// see `None` and the yet-to-land `journal_write` skips the
    /// persist step (matches pre-persistence behavior).
    ///
    /// Cell shape: `Arc<std::sync::RwLock<Option<Arc<dyn ...>>>>`
    /// — outer `Arc` so clones share the slot; `RwLock` so the
    /// boot pipeline can install (`share_*`) after construction;
    /// inner `Option<Arc<dyn ...>>` so an unset backend is
    /// distinguishable from a live one and the trait object is
    /// cheaply-cloneable.
    ///
    /// Private field — accessors below route through
    /// `poison_abort` (same discipline as the deploy cell).
    payload_store: Arc<std::sync::RwLock<Option<Arc<dyn PayloadPersistence>>>>,
    /// Serving-side payload-source recorder (optional).  Same
    /// cell shape + sharing pattern as `payload_store`; drives
    /// the second tier of the joiner-side payload-fetch chain
    /// (see the `PayloadSourceRecorder` trait docstring in
    /// `wal.rs`).
    ///
    /// Private field — accessors below route through
    /// `poison_abort`.
    payload_source_recorder: Arc<std::sync::RwLock<Option<Arc<dyn PayloadSourceRecorder>>>>,
}

/// Bundled `(scope, sig)` for the current deploy.  Private —
/// exposed through `FileHandleTable`'s accessor methods only.
#[derive(Debug, Default, Clone)]
struct DeployState {
    scope: DeployScope,
    sig: Vec<u8>,
}

#[derive(Debug)]
struct Inner {
    table: RwLock<HashMap<u64, FileHandle>>,
    /// Monotonic per-runtime fd allocator.  Starts at 1
    /// (fd = 0 is reserved as the "unset" sentinel at the
    /// Rholang boundary — a caller receiving fd = 0 knows the
    /// allocation failed even before checking the outer
    /// `Result`).
    next_fd: AtomicU64,
}

impl Default for Inner {
    fn default() -> Self {
        Inner {
            table: RwLock::new(HashMap::new()),
            next_fd: AtomicU64::new(1),
        }
    }
}

impl FileHandleTable {
    /// Construct an empty handle table with an fd counter
    /// starting at 1.
    pub fn new() -> Self { Self::default() }

    /// Insert a fresh handle, returning the freshly-allocated
    /// monotonic fd.
    ///
    /// # Errors
    ///
    /// Returns `Err(())` when:
    ///   - the per-runtime fd cap (`super::MAX_OPEN_FDS`) is
    ///     reached, OR
    ///   - `next_fd` would wrap past `u64::MAX`.
    ///
    /// The handler layer translates either to
    /// `FSERR_QUOTA_EXCEEDED`.  The `u64::MAX` wrap guard
    /// prevents a lifetime allocation of ~2^32 open/close
    /// cycles combined with a (subsequent-slice) high-watermark
    /// seed from wrapping to `fd = 0` (which would alias any
    /// stale reference at low fd values).
    #[allow(clippy::result_unit_err)]
    pub async fn insert(&self, handle: FileHandle) -> Result<u64, ()> {
        let mut table = self.inner.table.write().await;
        if table.len() >= super::MAX_OPEN_FDS {
            return Err(());
        }
        // Fetch-then-check on a monotonic counter — we bail out
        // before the increment would overflow.  Serialized under
        // the write lock, so `Relaxed` ordering is sufficient
        // (the lock's acquire/release edges provide the
        // synchronization that would otherwise need SeqCst).
        let current = self.inner.next_fd.load(Ordering::Relaxed);
        if current == u64::MAX {
            return Err(());
        }
        let fd = self.inner.next_fd.fetch_add(1, Ordering::Relaxed);
        if fd == u64::MAX {
            // Post-fetch defensive check.  In principle unreachable
            // under the write lock (`current != u64::MAX` and no
            // concurrent mutator between the load and the
            // fetch_add), but kept as belt-and-suspenders — if the
            // lock discipline ever regresses, this pins the counter
            // at `u64::MAX` so subsequent `insert` calls fail fast.
            self.inner.next_fd.store(u64::MAX, Ordering::Relaxed);
            return Err(());
        }
        // Structural invariant: `insert_at`'s `fetch_max` on
        // `next_fd` ensures the allocator never reissues a fd
        // that a prior `insert_at` populated.  `debug_assert!`
        // catches a regression in the invariant loudly in debug
        // builds while release stays zero-cost.
        debug_assert!(
            !table.contains_key(&fd),
            "FileHandleTable::insert allocator reissued a fd \
             (fd={fd}) already populated by an earlier `insert_at` \
             — the `insert_at` `fetch_max(next_fd, fd + 1)` \
             invariant has regressed"
        );
        table.insert(fd, handle);
        Ok(fd)
    }

    /// Insert a handle at a **specific** fd, bypassing the
    /// monotonic allocator.  Used by the (Wave 4) follower's
    /// `fs_open` replay branch — the leader's fd (extracted from
    /// the cached `previous` reply) is passed here so the
    /// follower's fd table indexes the same numeric key as the
    /// leader's, enabling symmetric WAL journaling in subsequent
    /// replay-branch mutating handlers.
    ///
    /// # Structural non-collision invariant
    ///
    /// Advances `next_fd` via `fetch_max(fd + 1)` so a subsequent
    /// `insert()` allocation CANNOT reissue the fd this call
    /// populated.  This is a structural guarantee at the module
    /// boundary — no follow-up "seed the counter from the state
    /// hash" pass is required to keep `insert` and `insert_at`
    /// non-overlapping on the same table.
    ///
    /// (The eventual soft-checkpoint slice's `seed_next_fd_from_
    /// state_hash` still has a role — it establishes a fresh
    /// per-runtime watermark at boot/reset for aliasing-
    /// prevention against stale tuplespace references from prior
    /// process lifetimes.  Independent from this invariant.)
    ///
    /// Returns `true` on success, `false` if the fd slot was
    /// already occupied — which would indicate a follower state-
    /// derivation bug and MUST NOT overwrite silently.
    pub async fn insert_at(&self, fd: u64, handle: FileHandle) -> bool {
        let mut table = self.inner.table.write().await;
        if table.contains_key(&fd) {
            return false;
        }
        table.insert(fd, handle);
        // Advance the monotone counter past this fd so the next
        // `insert()` can't collide.  `fetch_max` is idempotent —
        // if `next_fd` was already past `fd + 1`, this is a
        // no-op.  Saturates at `u64::MAX` if `fd == u64::MAX`
        // (that specific fd's allocation stays valid; the next
        // `insert()` will trip the wrap guard).
        let advance_to = fd.saturating_add(1);
        self.inner.next_fd.fetch_max(advance_to, Ordering::Relaxed);
        true
    }

    /// Remove and drop the handle at `fd`.  Dropping the
    /// `FileHandle` releases the `Arc<File>` reference; the OS
    /// fd closes when the last outstanding clone drops (which
    /// closes any TOCTOU window for a concurrent `raw_fd` clone
    /// held by a `spawn_blocking` closure — see the field-level
    /// docstring on `FileHandle::file`).
    ///
    /// Returns `true` if the fd was present, `false` otherwise.
    /// Idempotent — closing an unknown fd is a no-op returning
    /// `false`.
    pub async fn remove(&self, fd: u64) -> bool {
        let mut table = self.inner.table.write().await;
        table.remove(&fd).is_some()
    }

    /// Look up an `Arc<File>` clone for a given logical fd
    /// handle.  Used by the `spawn_blocking` closures so they
    /// can hand a valid OS fd into libc syscalls without holding
    /// the tokio `RwLock` across the syscall.
    ///
    /// Returns `None` if the fd is absent OR the handle is a
    /// shadow (`file: None`, follower's `fs_open` replay branch).
    /// Shadow handles are inserted only on the follower's replay
    /// path; the read/write syscall paths short-circuit on
    /// `is_replay = true` before reaching `raw_fd`, so a `None`
    /// here from a shadow handle should never be observed in
    /// practice — but the fail-closed return (translated to
    /// `FSERR_CLOSED` upstream) is correct if it ever is.
    ///
    /// The returned value is an `Arc<File>` clone (not a raw
    /// `i32`) so the caller can move it into a `spawn_blocking`
    /// closure and derive the raw fd inside via `.as_raw_fd()`.
    /// The Arc keeps the underlying `File` alive for the
    /// closure's lifetime even if a concurrent `remove(fd)`
    /// drops the table's own Arc — closing the TOCTOU window a
    /// bare-`i32` return would leave open.
    #[cfg(unix)]
    pub async fn raw_fd(&self, fd: u64) -> Option<Arc<File>> {
        let table = self.inner.table.read().await;
        table.get(&fd).and_then(|h| h.file.clone())
    }

    /// Run `f` against the handle at `fd` under a write lock.
    /// Returns `None` if the fd is absent, otherwise `Some(f's
    /// return value)`.
    ///
    /// # Snapshot-before-spawn discipline
    ///
    /// Callers that follow this with a `spawn_blocking` closure
    /// MUST snapshot the fields they need INSIDE this call
    /// (moved-out via `.clone()` or bit-copy), NOT re-read them
    /// from a fresh `with_mut` after the closure returns — the
    /// second lookup could race with a `remove(fd)` and observe
    /// `None`.
    pub async fn with_mut<F, R>(&self, fd: u64, f: F) -> Option<R>
    where F: FnOnce(&mut FileHandle) -> R {
        let mut table = self.inner.table.write().await;
        table.get_mut(&fd).map(f)
    }

    // --- Deploy-scope cells ----------------------------------------

    /// Read the current deploy scope.  Returns `[0u8; 32]` (the
    /// sentinel) when no `WalDeployScope` is live.
    pub fn current_deploy_scope(&self) -> DeployScope {
        poison_abort(self.deploy.read(), "FileHandleTable.deploy").scope
    }

    /// Set ONLY the current deploy scope.  Prefer
    /// `set_current_deploy(scope, sig)` at deploy-entry — the
    /// compound writer maintains the "both populated OR both
    /// sentinel" invariant structurally.  This per-field setter
    /// exists for narrower callers (tests that intentionally
    /// exercise partial state).
    pub fn set_current_deploy_scope(&self, scope: DeployScope) {
        poison_abort(self.deploy.write(), "FileHandleTable.deploy").scope = scope;
    }

    /// Read the current deploy signature (raw bytes).  Returns an
    /// empty vec when no `WalDeployScope` is live — that's the
    /// sentinel the (yet-to-land) `journal_write` path checks to
    /// decide whether to skip the payload-source-recorder step.
    pub fn current_deploy_sig(&self) -> Vec<u8> {
        poison_abort(self.deploy.read(), "FileHandleTable.deploy")
            .sig
            .clone()
    }

    /// Set ONLY the current deploy signature.  Same "prefer the
    /// compound writer" caveat as `set_current_deploy_scope`.
    pub fn set_current_deploy_sig(&self, sig: Vec<u8>) {
        poison_abort(self.deploy.write(), "FileHandleTable.deploy").sig = sig;
    }

    /// Set BOTH deploy cells atomically under a single guard.
    /// Called by the (yet-to-land) `WalDeployScope::new` at
    /// deploy-entry — the two fields transition together so
    /// consumers cannot observe a `(scope, sig)` pair that
    /// straddles the deploy boundary.
    pub fn set_current_deploy(&self, scope: DeployScope, sig: Vec<u8>) {
        let mut guard = poison_abort(self.deploy.write(), "FileHandleTable.deploy");
        guard.scope = scope;
        guard.sig = sig;
    }

    /// Reset BOTH deploy cells to their sentinel state atomically
    /// under a single guard.  Called by the (yet-to-land)
    /// `WalDeployScope::drop`.  Consumers reading during (or
    /// concurrent with) this call see either the pre-clear
    /// `(populated, populated)` or the post-clear `(sentinel,
    /// sentinel)` — never a straddled pair.
    pub fn clear_current_deploy(&self) {
        let mut guard = poison_abort(self.deploy.write(), "FileHandleTable.deploy");
        guard.scope = [0u8; 32];
        guard.sig.clear();
    }

    // --- Payload trait-object plumbing -----------------------------

    /// Attach (or clear, with `None`) the manager-shared payload
    /// persistence backend.  Called from the boot pipeline via
    /// the runtime manager after `FileHandleTable::new`; the
    /// interior `RwLock` gives interior mutability so the write
    /// is visible through every already-cloned handle without
    /// requiring `&mut self`.
    ///
    /// After attachment, the (yet-to-land) `journal_write` on
    /// every mutating fs handler for a Consensus cap will call
    /// `store.persist(bytes)` to stash the write payload content-
    /// addressed on disk for peer-fetch by joiners.
    pub fn share_payload_store(&self, shared: Option<Arc<dyn PayloadPersistence>>) {
        *poison_abort(self.payload_store.write(), "FileHandleTable.payload_store") = shared;
    }

    /// Read the currently-installed persistence backend, if any.
    /// The returned `Option<Arc<dyn ...>>` is cheap to clone (Arc
    /// refcount bump).  Called by `journal_write` on every write
    /// to decide whether to persist.
    pub fn payload_store(&self) -> Option<Arc<dyn PayloadPersistence>> {
        poison_abort(self.payload_store.read(), "FileHandleTable.payload_store").clone()
    }

    /// Attach (or clear, with `None`) the manager-shared payload-
    /// source recorder — the second tier of the joiner-side
    /// payload-fetch chain (see the `PayloadSourceRecorder` trait
    /// docstring in `wal.rs`).  Same sharing pattern as
    /// `share_payload_store`.
    pub fn share_payload_source_recorder(&self, shared: Option<Arc<dyn PayloadSourceRecorder>>) {
        *poison_abort(
            self.payload_source_recorder.write(),
            "FileHandleTable.payload_source_recorder",
        ) = shared;
    }

    /// Read the currently-installed payload-source recorder, if
    /// any.  Called by `journal_write` on every Consensus-cap
    /// write to decide whether to record
    /// `(payload_hash → deploy_sig)` into the block-storage
    /// side-index.
    pub fn payload_source_recorder(&self) -> Option<Arc<dyn PayloadSourceRecorder>> {
        poison_abort(
            self.payload_source_recorder.read(),
            "FileHandleTable.payload_source_recorder",
        )
        .clone()
    }

    // --- Root-identity registry sharing ----------------------------

    /// Attach the manager-shared root-identity registry so all
    /// runtimes spawned from this manager see the same
    /// `logical → Root` map.  Called from the boot pipeline
    /// (`RuntimeManager::spawn_runtime` /
    /// `spawn_replay_runtime`) — same broadcast pattern as
    /// [`share_payload_store`](Self::share_payload_store).
    ///
    /// Takes `&self` (not `&mut self`) — the underlying
    /// [`RootIdentityRegistry::share_from`] is `&self` (interior
    /// mutability via the outer `RwLock`), so the boot pipeline
    /// can install after already-spawned runtimes have cloned
    /// the handle table.
    ///
    /// # Load-bearing invariant: reducer-clone visibility
    ///
    /// `rho_runtime::create_rho_runtime` clones the
    /// `FileHandleTable` (and therefore this field) to hand a
    /// copy to the reducer BEFORE the boot pipeline calls
    /// `share_root_registry`.  The two-layer indirection inside
    /// `RootIdentityRegistry` (see its struct-level docstring)
    /// ensures the reducer's earlier clone shares the OUTER slot
    /// with the runtime's clone — swapping the INNER Arc via
    /// `share_from` is visible through both.  A field-replacement
    /// approach here (`self.root_registry = shared`) would only
    /// update the runtime's outer field and leave the reducer's
    /// clone bound to its old inner — the PB-M-14 canary
    /// regression fileio's investigation surfaced.
    ///
    /// The two properties this call preserves:
    ///   1. **Reducer-clone visibility**: the reducer's earlier
    ///      clone points at the SAME outer slot Arc as `self`;
    ///      the inner swap is observed by both.
    ///   2. **Late-registration propagation**: after the swap,
    ///      every clone routes through `shared`'s inner Arc — a
    ///      later `register` on the manager writes to the shared
    ///      inner and every runtime sees it.
    pub fn share_root_registry(&self, shared: RootIdentityRegistry) {
        self.root_registry.share_from(&shared);
    }

    // --- Soft-checkpoint (slice 8) ---------------------------------

    /// Snapshot the next-fd counter for deploy-boundary rollback.
    /// Callers pair this with [`truncate_to`](Self::truncate_to)
    /// on `RhoRuntimeImpl` soft-checkpoint: the snapshot is taken
    /// pre-deploy, and revert closes every fd allocated past it.
    pub fn snapshot_next_fd(&self) -> u64 { self.inner.next_fd.load(Ordering::Relaxed) }

    /// Raise the fd counter to at least `watermark + 1` before
    /// the next allocation.  Called on every block boundary via
    /// runtime reset so a post-restart runtime cannot allocate
    /// fd values that alias stale references still present in
    /// the tuplespace.
    ///
    /// # Aliasing scenario averted
    ///
    /// Pre-restart, Deploy A opened a File with fd = 42 and
    /// stashed the cap in the tuplespace.  Node restarts; the
    /// fresh `FileHandleTable::next_fd` starts at 1.  A
    /// subsequent Deploy B opens 41 unrelated files; the 42nd
    /// new open would allocate fd = 42.  Deploy A's stashed cap,
    /// invoked later via `fsRead(42, ...)`, would now read from
    /// Deploy B's file.  Seeding the watermark from a value
    /// larger than any pre-restart fd prevents this.
    ///
    /// # Monotonic + idempotent
    ///
    /// Multiple calls with the same or lower watermark are
    /// no-ops; the counter never rewinds.  This preserves the
    /// pre-existing "closed fd is never re-issued" invariant.
    ///
    /// Uses a compare-and-swap loop rather than `fetch_max`
    /// because `AtomicU64::fetch_max` isn't stable everywhere the
    /// tree builds today — the CAS is a portable equivalent.
    pub fn seed_next_fd_watermark(&self, watermark: u64) {
        let target = watermark.saturating_add(1);
        let mut current = self.inner.next_fd.load(Ordering::Relaxed);
        while current < target {
            match self.inner.next_fd.compare_exchange_weak(
                current,
                target,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(observed) => current = observed,
            }
        }
    }

    /// Derive a deterministic per-restart watermark from a
    /// 32-byte state hash and seed `next_fd` from it.  Called
    /// on every block boundary via runtime reset.
    ///
    /// # Consensus commitment
    ///
    /// Fd values are consensus-observable (stored in Rholang
    /// tuplespace state as `u64` inside `File`-agent `fdP`
    /// cells; go on-chain via the tuplespace state hash).  This
    /// derivation is therefore an implicit consensus commitment
    /// — every validator resetting to the same state hash
    /// computes the same watermark, so replay of captured fd
    /// values on followers matches the leader's allocation.  Any
    /// future change to the derivation (bytes used, shift
    /// amount, hashing algorithm) is a hard fork.
    ///
    /// # Entropy derivation
    ///
    /// Take the first 8 bytes of `hash` as a big-endian `u64` and
    /// mask off the low `FD_ENTROPY_HEADROOM_BITS` bits.  The
    /// masked bits are reserved as per-lifetime allocation
    /// headroom — a single runtime lifetime can allocate up to
    /// `1 << FD_ENTROPY_HEADROOM_BITS` fds before the counter
    /// would enter the next watermark's range.
    ///
    /// Entropy budget: 64 − 20 = 44 bits of state-hash entropy
    /// in the watermark.  Birthday collision at ~2^22 (~4 million)
    /// blocks → ~130 years at 1 block/sec.  Headroom budget:
    /// 2^20 ≈ 1M fd allocations per runtime lifetime, well above
    /// any realistic per-block open/close pattern
    /// (`MAX_OPEN_FDS = 1024` live fds → ~1000 open+close cycles
    /// per block is the realistic upper bound).
    ///
    /// # Short-hash guard
    ///
    /// `debug_assert!` fires if a caller passes a hash shorter
    /// than 8 bytes — state hashes are always 32 bytes, but a
    /// future refactor passing a truncated value would silently
    /// reduce entropy and disable the aliasing protection.
    pub fn seed_next_fd_from_state_hash(&self, hash: &[u8]) {
        // Release-time assert (not `debug_assert!`) — the check
        // is a single length comparison on a rare boot-only path
        // and closes the "silent entropy reduction" hole a
        // truncated hash would open.  State hashes are always 32
        // bytes in practice; a shorter one is a code bug that
        // MUST fail loudly rather than silently pad + reduce
        // watermark entropy.
        assert!(
            hash.len() >= 8,
            "seed_next_fd_from_state_hash requires at least 8 bytes of hash; \
             got {} — a shorter hash silently reduces entropy and disables aliasing protection",
            hash.len()
        );
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&hash[..8]);
        let hi = u64::from_be_bytes(buf);
        // Mask off the low headroom bits so a full runtime
        // lifetime cannot overflow into the next watermark's
        // range.
        let watermark = hi & !((1u64 << FD_ENTROPY_HEADROOM_BITS) - 1);
        self.seed_next_fd_watermark(watermark);
    }

    /// Roll back to a prior `snapshot_next_fd` value, dropping
    /// every fd allocated past it.  Called by
    /// `RhoRuntimeImpl::revert_to_soft_checkpoint` alongside
    /// `Wal::truncate_to`.
    ///
    /// Note: `next_fd` is monotonic even across rollback — the
    /// counter does NOT rewind.  That is the invariant that
    /// prevents fd aliasing across deploys.  A rolled-back
    /// deploy's next-attempt allocation gets a fresh fd value,
    /// not a reused one.
    pub async fn truncate_to(&self, snapshot: u64) {
        let mut table = self.inner.table.write().await;
        table.retain(|&fd, _| fd < snapshot);
    }
}

// Compile-time witnesses.  `FileHandleTable` is stored on the
// runtime and cloned into every handler closure — any refactor
// that broke `Send + Sync` would surface here at build time
// instead of at some unrelated tokio-spawn site.  `FileHandle`
// itself must also be `Send + Sync` because it is guarded by a
// tokio `RwLock` (which requires `T: Send`).
const _FILE_HANDLE_TABLE_IS_SEND_SYNC: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<FileHandleTable>();
};
const _FILE_HANDLE_IS_SEND_SYNC: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<FileHandle>();
};

#[cfg(test)]
mod tests {
    use super::*;

    /// Test helper — a shadow handle (no OS fd), suitable for
    /// filling the table without actually opening files.  The
    /// bulk of the slice-1 tests use these to exercise the
    /// allocator and map without racing against the process fd
    /// limit.
    fn shadow_handle(deploy_byte: u8) -> FileHandle {
        FileHandle {
            file: None,
            canon_path: PathBuf::from("/tmp/shadow"),
            mode: AccessMode::Read,
            cmode: ConsensusMode::Consensus,
            position: 0,
            deploy: [deploy_byte; 32],
        }
    }

    #[tokio::test]
    async fn new_table_starts_empty_with_fd_counter_at_1() {
        let table = FileHandleTable::new();
        assert!(table.inner.table.read().await.is_empty());
        assert_eq!(table.inner.next_fd.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn insert_returns_monotonic_fds_starting_at_1() {
        let table = FileHandleTable::new();
        let fd1 = table.insert(shadow_handle(0x01)).await.unwrap();
        let fd2 = table.insert(shadow_handle(0x02)).await.unwrap();
        let fd3 = table.insert(shadow_handle(0x03)).await.unwrap();
        assert_eq!(fd1, 1);
        assert_eq!(fd2, 2);
        assert_eq!(fd3, 3);
    }

    #[tokio::test]
    async fn insert_returns_err_past_max_open_fds() {
        let table = FileHandleTable::new();
        // Fill to the cap using shadow handles.
        for i in 0..super::super::MAX_OPEN_FDS {
            table
                .insert(shadow_handle(i as u8))
                .await
                .unwrap_or_else(|()| panic!("insert {i} unexpectedly failed"));
        }
        // The next insert trips the cap.
        assert_eq!(table.insert(shadow_handle(0xFF)).await, Err(()));
    }

    #[tokio::test]
    async fn remove_returns_true_on_present_fd_false_otherwise() {
        let table = FileHandleTable::new();
        let fd = table.insert(shadow_handle(0x42)).await.unwrap();
        assert!(table.remove(fd).await);
        // Idempotent — removing again returns false.
        assert!(!table.remove(fd).await);
        // Unrelated fds return false too.
        assert!(!table.remove(9999).await);
    }

    /// A removed fd is NOT reused — the monotonic allocator
    /// advances past it.  This is the load-bearing "stale fd
    /// reliably observes `FSERR_CLOSED`" invariant that the
    /// module docstring calls out.
    #[tokio::test]
    async fn removed_fd_is_not_reused_by_subsequent_insert() {
        let table = FileHandleTable::new();
        let fd1 = table.insert(shadow_handle(0x01)).await.unwrap();
        assert_eq!(fd1, 1);
        assert!(table.remove(fd1).await);
        // The next insert allocates fd = 2, not fd = 1.
        let fd2 = table.insert(shadow_handle(0x02)).await.unwrap();
        assert_eq!(fd2, 2);
    }

    #[tokio::test]
    async fn insert_at_places_handle_at_specific_fd() {
        let table = FileHandleTable::new();
        assert!(table.insert_at(42, shadow_handle(0xAA)).await);
        // Present at fd = 42.
        let deploy_readback = table
            .with_mut(42, |h| h.deploy)
            .await
            .expect("fd 42 must be present");
        assert_eq!(deploy_readback, [0xAA; 32]);
    }

    /// `insert_at` must NOT overwrite an occupied slot — this
    /// guards against a follower state-derivation bug where the
    /// same leader fd was replayed twice.
    #[tokio::test]
    async fn insert_at_rejects_collision() {
        let table = FileHandleTable::new();
        assert!(table.insert_at(7, shadow_handle(0x01)).await);
        assert!(!table.insert_at(7, shadow_handle(0x02)).await);
        // Original handle survives.
        let deploy_readback = table.with_mut(7, |h| h.deploy).await.unwrap();
        assert_eq!(deploy_readback, [0x01; 32]);
    }

    /// `insert_at` advances `next_fd` via `fetch_max(fd + 1)`
    /// so a subsequent `insert()` allocation CANNOT reissue the
    /// fd this call populated.  Structural non-collision
    /// invariant — no follow-up "seed the counter" pass is
    /// required to keep `insert` and `insert_at` disjoint.
    #[tokio::test]
    async fn insert_at_advances_monotonic_counter_past_placed_fd() {
        let table = FileHandleTable::new();
        assert!(table.insert_at(1000, shadow_handle(0x01)).await);
        // next_fd is now 1001 — advanced past the placed fd.
        assert_eq!(table.inner.next_fd.load(Ordering::Relaxed), 1001);
        // A subsequent insert() allocates 1001, NOT 1 — the
        // allocator can never reissue an insert_at'd fd.
        let fd = table.insert(shadow_handle(0x02)).await.unwrap();
        assert_eq!(fd, 1001);
    }

    /// `insert_at` is idempotent on its counter advance —
    /// repeated calls with the same or lower fd don't rewind
    /// the counter (since `fetch_max` is monotonic-max).
    #[tokio::test]
    async fn insert_at_below_current_counter_does_not_rewind() {
        let table = FileHandleTable::new();
        // Bump the counter past 100.
        assert!(table.insert_at(500, shadow_handle(0x01)).await);
        assert_eq!(table.inner.next_fd.load(Ordering::Relaxed), 501);
        // A below-counter insert_at doesn't rewind.
        assert!(table.insert_at(100, shadow_handle(0x02)).await);
        assert_eq!(table.inner.next_fd.load(Ordering::Relaxed), 501);
    }

    /// The reviewer-flagged silent-overwrite hazard, pinned.
    /// Pre-fix: `insert_at(1)` followed by `insert()` would
    /// silently overwrite the shadow handle at fd = 1.  Post-
    /// fix: the `insert_at`'s `fetch_max(fd + 1)` advances the
    /// counter to 2, so `insert()` allocates fd = 2 (or higher),
    /// and the shadow at fd = 1 survives.
    #[tokio::test]
    async fn insert_after_insert_at_does_not_overwrite() {
        let table = FileHandleTable::new();
        // Follower's shadow-leader replay-branch insert.
        assert!(table.insert_at(1, shadow_handle(0xAA)).await);
        // A subsequent independent `insert()` MUST NOT reissue
        // fd = 1.  Pre-fix, this would silently overwrite the
        // shadow handle.
        let fresh_fd = table.insert(shadow_handle(0xBB)).await.unwrap();
        assert_ne!(fresh_fd, 1, "insert must not reissue insert_at'd fd");
        // The shadow at fd = 1 survives — verified by reading
        // back the distinctive deploy byte.
        let shadow_deploy = table.with_mut(1, |h| h.deploy).await.unwrap();
        assert_eq!(shadow_deploy, [0xAA; 32]);
        // The fresh handle lives at fresh_fd with its own deploy.
        let fresh_deploy = table.with_mut(fresh_fd, |h| h.deploy).await.unwrap();
        assert_eq!(fresh_deploy, [0xBB; 32]);
    }

    #[tokio::test]
    async fn raw_fd_returns_none_for_absent_fd_and_shadow_handle() {
        let table = FileHandleTable::new();
        // Absent fd → None.
        assert!(table.raw_fd(999).await.is_none());
        // Present but shadow (file: None) → None.
        let fd = table.insert(shadow_handle(0x01)).await.unwrap();
        assert!(table.raw_fd(fd).await.is_none());
    }

    /// `raw_fd` returns `Some(Arc<File>)` when the handle owns
    /// a real OS fd.  The Arc-clone contract is what makes the
    /// `spawn_blocking` closure safe against a concurrent
    /// `remove(fd)`: pin that a clone survives a remove of the
    /// original table entry, closing the TOCTOU window a bare
    /// `i32` return would leave open (bare fd → concurrent
    /// remove drops `File` → OS fd closes → unrelated `open(2)`
    /// reuses the integer → pending syscall targets wrong fd).
    #[tokio::test]
    #[cfg(unix)]
    async fn raw_fd_arc_clone_survives_concurrent_remove() {
        use std::io::Write;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.txt");
        let file = File::create(&path).unwrap();

        let table = FileHandleTable::new();
        let fd = table
            .insert(FileHandle {
                file: Some(Arc::new(file)),
                canon_path: path.clone(),
                mode: AccessMode::Write,
                cmode: ConsensusMode::Oracular,
                position: 0,
                deploy: [0u8; 32],
            })
            .await
            .unwrap();

        // Grab an Arc<File> clone (the raw_fd contract).
        let clone = table.raw_fd(fd).await.expect("raw_fd must return Some");
        // Concurrently remove the table entry.  The table's own
        // Arc<File> drops; only our `clone` keeps the File alive.
        assert!(table.remove(fd).await);
        // The clone MUST still be usable — `try_clone()` on the
        // still-open OS fd succeeds, and we can write through
        // the duplicate.  A regression that closed the OS fd on
        // table removal would fail either `try_clone()` (EBADF)
        // or the subsequent `write_all` (EBADF).
        let mut dup = clone
            .try_clone()
            .expect("File::try_clone must succeed while Arc clone is live");
        dup.write_all(b"post-remove")
            .expect("write must succeed on the surviving fd");
    }

    #[tokio::test]
    async fn with_mut_mutates_handle_and_persists() {
        let table = FileHandleTable::new();
        let fd = table.insert(shadow_handle(0x01)).await.unwrap();

        // Advance position via the mutation callback.
        let returned = table
            .with_mut(fd, |h| {
                h.position += 42;
                h.position
            })
            .await
            .unwrap();
        assert_eq!(returned, 42);

        // Re-read to confirm the mutation persisted.
        let readback = table.with_mut(fd, |h| h.position).await.unwrap();
        assert_eq!(readback, 42);
    }

    #[tokio::test]
    async fn with_mut_returns_none_for_absent_fd() {
        let table = FileHandleTable::new();
        assert!(table.with_mut(9999, |_| ()).await.is_none());
    }

    /// Clone-shares-underlying-table invariant.  Handler closures
    /// receive cloned handles; a mutation through one clone must
    /// be visible through the other.
    #[tokio::test]
    async fn clone_shares_underlying_table() {
        let a = FileHandleTable::new();
        let b = a.clone();
        let fd = a.insert(shadow_handle(0x42)).await.unwrap();
        // Visible via the sibling clone.
        let deploy = b.with_mut(fd, |h| h.deploy).await.unwrap();
        assert_eq!(deploy, [0x42; 32]);
    }

    // --- Slice 2: Wal + deploy-scope cells -------------------------

    #[test]
    fn wal_field_starts_empty_and_is_shared_across_clones() {
        use super::super::wal::{PayloadRef, WalEntry, WalOp, WalOutcome};

        let a = FileHandleTable::new();
        assert!(a.wal.is_empty());

        // Cloning FileHandleTable shares the Arc<...> inside Wal
        // (Wal itself is Arc-backed).  A journal append through
        // `a` is visible through `b`.
        let b = a.clone();
        let entry = WalEntry {
            op: WalOp::Write,
            path: PathBuf::from("/tmp/x"),
            extra_path: None,
            offset: None,
            length: Some(1),
            payload_ref: Some(PayloadRef::hash(b"x")),
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        };
        a.wal.append(entry).unwrap();
        assert_eq!(b.wal.len(), 1);
    }

    #[test]
    fn deploy_scope_starts_at_sentinel() {
        let table = FileHandleTable::new();
        assert_eq!(table.current_deploy_scope(), [0u8; 32]);
    }

    #[test]
    fn deploy_sig_starts_empty() {
        let table = FileHandleTable::new();
        assert!(table.current_deploy_sig().is_empty());
    }

    #[test]
    fn set_and_read_current_deploy_scope_roundtrips() {
        let table = FileHandleTable::new();
        let scope = [0xAAu8; 32];
        table.set_current_deploy_scope(scope);
        assert_eq!(table.current_deploy_scope(), scope);
    }

    #[test]
    fn set_and_read_current_deploy_sig_roundtrips() {
        let table = FileHandleTable::new();
        let sig = b"raw-deploy-sig-bytes".to_vec();
        table.set_current_deploy_sig(sig.clone());
        assert_eq!(table.current_deploy_sig(), sig);
    }

    /// The clones share the underlying `Arc<RwLock<...>>` cells —
    /// a set through one clone is visible through another.  Load-
    /// bearing for the (yet-to-land) `WalDeployScope::new` +
    /// handler-side reads across independently-cloned handles.
    #[test]
    fn deploy_scope_and_sig_are_shared_across_clones() {
        let a = FileHandleTable::new();
        let b = a.clone();
        let scope = [0xBBu8; 32];
        a.set_current_deploy_scope(scope);
        a.set_current_deploy_sig(b"sig".to_vec());
        assert_eq!(b.current_deploy_scope(), scope);
        assert_eq!(b.current_deploy_sig(), b"sig".to_vec());
    }

    #[test]
    fn clear_current_deploy_resets_both_cells_to_sentinel() {
        let table = FileHandleTable::new();
        table.set_current_deploy_scope([0x11u8; 32]);
        table.set_current_deploy_sig(vec![0x22, 0x33]);
        table.clear_current_deploy();
        assert_eq!(table.current_deploy_scope(), [0u8; 32]);
        assert!(table.current_deploy_sig().is_empty());
    }

    /// The compound writer sets both cells under a single guard.
    /// A subsequent read of either field sees the paired value.
    #[test]
    fn set_current_deploy_updates_both_cells_atomically() {
        let table = FileHandleTable::new();
        let scope = [0x77u8; 32];
        let sig = b"paired-sig".to_vec();
        table.set_current_deploy(scope, sig.clone());
        assert_eq!(table.current_deploy_scope(), scope);
        assert_eq!(table.current_deploy_sig(), sig);
    }

    // --- Slice 3: payload trait-object plumbing --------------------

    /// Test-only `PayloadPersistence` fake that records every
    /// `persist` invocation and returns Blake2b256 via the trait
    /// contract (matches `MockPersistence` in `wal.rs`).
    #[derive(Debug, Default)]
    struct RecordingPersistence {
        seen: std::sync::Mutex<Vec<Vec<u8>>>,
    }

    impl PayloadPersistence for RecordingPersistence {
        fn persist(&self, bytes: &[u8]) -> Result<[u8; 32], String> {
            self.seen.lock().unwrap().push(bytes.to_vec());
            match super::super::wal::PayloadRef::hash(bytes) {
                super::super::wal::PayloadRef::Hash(h) => Ok(h),
                _ => unreachable!(),
            }
        }
    }

    /// Test-only `PayloadSourceRecorder` fake.
    #[derive(Debug, Default)]
    struct RecordingRecorder {
        seen: std::sync::Mutex<Vec<([u8; 32], Vec<u8>)>>,
    }

    impl PayloadSourceRecorder for RecordingRecorder {
        fn record(&self, payload_hash: [u8; 32], deploy_sig: &[u8]) -> Result<(), String> {
            self.seen
                .lock()
                .unwrap()
                .push((payload_hash, deploy_sig.to_vec()));
            Ok(())
        }
    }

    #[test]
    fn payload_store_starts_unset() {
        let table = FileHandleTable::new();
        assert!(table.payload_store().is_none());
    }

    #[test]
    fn payload_source_recorder_starts_unset() {
        let table = FileHandleTable::new();
        assert!(table.payload_source_recorder().is_none());
    }

    /// `share_payload_store` attaches a backend that a subsequent
    /// `payload_store()` read observes.  End-to-end round-trip via
    /// the trait — invokes `.persist(bytes)` through the returned
    /// `Arc<dyn ...>` and confirms the fake recorded the bytes.
    #[test]
    fn share_payload_store_then_persist_roundtrips_through_trait() {
        let table = FileHandleTable::new();
        let backend = Arc::new(RecordingPersistence::default());
        table.share_payload_store(Some(backend.clone() as Arc<dyn PayloadPersistence>));

        let installed = table.payload_store().expect("must be set");
        let bytes = b"consensus-write-payload";
        let hash = installed.persist(bytes).expect("persist ok");
        // Verify the hash matches the contract (Blake2b256).
        match super::super::wal::PayloadRef::hash(bytes) {
            super::super::wal::PayloadRef::Hash(expected) => assert_eq!(hash, expected),
            _ => unreachable!(),
        }
        // The fake recorded the bytes.
        let seen = backend.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0], bytes.to_vec());
    }

    /// `share_payload_store(None)` clears a previously-attached
    /// backend — the getter returns `None`.
    #[test]
    fn share_payload_store_none_clears_installed_backend() {
        let table = FileHandleTable::new();
        table.share_payload_store(Some(
            Arc::new(RecordingPersistence::default()) as Arc<dyn PayloadPersistence>
        ));
        assert!(table.payload_store().is_some());
        table.share_payload_store(None);
        assert!(table.payload_store().is_none());
    }

    /// Round-trip test for the recorder — parallels the persist
    /// round-trip test above.
    #[test]
    fn share_payload_source_recorder_then_record_roundtrips_through_trait() {
        let table = FileHandleTable::new();
        let recorder = Arc::new(RecordingRecorder::default());
        table.share_payload_source_recorder(Some(
            recorder.clone() as Arc<dyn PayloadSourceRecorder>
        ));

        let installed = table.payload_source_recorder().expect("must be set");
        installed
            .record([0xAAu8; 32], b"deploy-sig")
            .expect("record ok");
        let seen = recorder.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0], ([0xAAu8; 32], b"deploy-sig".to_vec()));
    }

    #[test]
    fn share_payload_source_recorder_none_clears_installed_recorder() {
        let table = FileHandleTable::new();
        table.share_payload_source_recorder(Some(
            Arc::new(RecordingRecorder::default()) as Arc<dyn PayloadSourceRecorder>
        ));
        assert!(table.payload_source_recorder().is_some());
        table.share_payload_source_recorder(None);
        assert!(table.payload_source_recorder().is_none());
    }

    /// Both payload cells share `Arc<RwLock<...>>` semantics with
    /// clones — an attachment through one clone is visible via
    /// another.  Load-bearing for the RuntimeManager broadcast
    /// pattern (the boot pipeline calls `share_*` on a manager-
    /// held clone; every already-spawned runtime's clone MUST
    /// observe the attachment on the next read).
    #[test]
    fn payload_store_and_recorder_share_across_clones() {
        let a = FileHandleTable::new();
        let b = a.clone();
        let backend = Arc::new(RecordingPersistence::default());
        let recorder = Arc::new(RecordingRecorder::default());
        a.share_payload_store(Some(backend as Arc<dyn PayloadPersistence>));
        a.share_payload_source_recorder(Some(recorder as Arc<dyn PayloadSourceRecorder>));

        assert!(b.payload_store().is_some());
        assert!(b.payload_source_recorder().is_some());
    }

    /// Replacement semantics: `share_payload_store(Some(A))`
    /// followed by `share_payload_store(Some(B))` yields `B` on
    /// the next `payload_store()` read (last-write-wins).  Pins
    /// the "share_* semantically REPLACES rather than accumulates"
    /// contract that operator hot-swap of a backend relies on.
    #[test]
    fn share_payload_store_replaces_prior_backend() {
        let table = FileHandleTable::new();
        let a = Arc::new(RecordingPersistence::default());
        let b = Arc::new(RecordingPersistence::default());
        table.share_payload_store(Some(a.clone() as Arc<dyn PayloadPersistence>));
        table.share_payload_store(Some(b.clone() as Arc<dyn PayloadPersistence>));

        // Invoke through the installed backend and confirm ONLY
        // `b`'s fake recorded the call — `a` was replaced, not
        // stacked.
        table
            .payload_store()
            .expect("must be set")
            .persist(b"witness")
            .expect("persist ok");
        assert!(
            a.seen.lock().unwrap().is_empty(),
            "prior backend must not receive calls"
        );
        assert_eq!(b.seen.lock().unwrap().len(), 1);
    }

    /// Symmetric replacement pin for the recorder.
    #[test]
    fn share_payload_source_recorder_replaces_prior_recorder() {
        let table = FileHandleTable::new();
        let a = Arc::new(RecordingRecorder::default());
        let b = Arc::new(RecordingRecorder::default());
        table.share_payload_source_recorder(Some(a.clone() as Arc<dyn PayloadSourceRecorder>));
        table.share_payload_source_recorder(Some(b.clone() as Arc<dyn PayloadSourceRecorder>));

        table
            .payload_source_recorder()
            .expect("must be set")
            .record([0x11u8; 32], b"sig")
            .expect("record ok");
        assert!(
            a.seen.lock().unwrap().is_empty(),
            "prior recorder must not receive calls"
        );
        assert_eq!(b.seen.lock().unwrap().len(), 1);
    }

    // --- Slice 4: root_registry + share_root_registry --------------

    use tempfile::TempDir;

    use super::super::path::identity::Root;

    #[test]
    fn root_registry_starts_empty() {
        let table = FileHandleTable::new();
        assert!(table.root_registry.is_empty());
        assert_eq!(table.root_registry.len(), 0);
    }

    /// Basic sanity: `share_root_registry` installs a manager
    /// registry; the table's own root_registry accessor sees
    /// registrations made through the shared handle.
    #[test]
    fn share_root_registry_exposes_manager_registrations() {
        let tmp = TempDir::new().unwrap();
        let root = Root::capture(tmp.path()).unwrap();

        let manager = RootIdentityRegistry::new();
        let logical = PathBuf::from("/@bundle/target");
        manager.register(logical.clone(), root);

        let table = FileHandleTable::new();
        assert!(table.root_registry.get(&logical).is_none(), "pre-share");

        table.share_root_registry(manager);
        assert!(
            table.root_registry.get(&logical).is_some(),
            "post-share: table sees manager's registration"
        );
    }

    /// **Load-bearing** — mirrors PR #501's PB-M-14 regression
    /// scenario at the `FileHandleTable` layer:
    ///
    ///   1. Manager registry created empty.
    ///   2. Table created; `share_root_registry(manager)` called.
    ///   3. `reducer_clone` captured from the table AFTER step 2.
    ///   4. Manager registers AFTER step 3.
    ///   5. reducer_clone.root_registry MUST see the step-4
    ///      registration.
    ///
    /// A field-replacement design would fail step 5 because
    /// reducer_clone would still hold its own registry inner
    /// disjoint from the manager.  The two-layer indirection
    /// inside `RootIdentityRegistry` (see PR #501) makes both
    /// clones share the outer slot; the swap inside `share_from`
    /// is visible through both.
    #[test]
    fn share_root_registry_preserves_reducer_clone_visibility() {
        let tmp = TempDir::new().unwrap();
        let root = Root::capture(tmp.path()).unwrap();

        // Step 1: empty manager.
        let manager = RootIdentityRegistry::new();

        // Step 2: table shares from manager.
        let table = FileHandleTable::new();
        table.share_root_registry(manager.clone());

        // Step 3: reducer captures a clone of the table AFTER
        // the share_root_registry call.
        let reducer_clone = table.clone();

        // Step 4: manager registers AFTER reducer_clone was
        // captured.
        let logical = PathBuf::from("/@bundle/late");
        manager.register(logical.clone(), root);

        // Step 5: the reducer_clone's root_registry MUST see the
        // late registration.  A pre-fix field-replacement design
        // would fail this — the reducer_clone would hold a
        // disjoint inner Arc.
        assert!(
            reducer_clone.root_registry.get(&logical).is_some(),
            "reducer_clone MUST see manager's late registration \
             via the two-layer share_from indirection"
        );
    }

    /// `share_root_registry` follows the same across-clones
    /// visibility contract as `share_payload_store` — a share
    /// through one clone is visible via another.
    #[test]
    fn share_root_registry_broadcasts_across_prior_clones() {
        let tmp = TempDir::new().unwrap();
        let root = Root::capture(tmp.path()).unwrap();

        let a = FileHandleTable::new();
        let b = a.clone();

        let manager = RootIdentityRegistry::new();
        let logical = PathBuf::from("/@bundle/via-a");
        manager.register(logical.clone(), root);

        a.share_root_registry(manager);
        assert!(
            b.root_registry.get(&logical).is_some(),
            "b (cloned before share) must see manager registrations \
             after a.share_root_registry"
        );
    }

    // --- Slice 8: soft-checkpoint machinery ------------------------

    #[tokio::test]
    async fn snapshot_next_fd_returns_current_counter_value() {
        let table = FileHandleTable::new();
        assert_eq!(table.snapshot_next_fd(), 1);
        // After 3 inserts, next_fd advances to 4.
        for i in 0..3 {
            table.insert(shadow_handle(i as u8)).await.unwrap();
        }
        assert_eq!(table.snapshot_next_fd(), 4);
    }

    /// `seed_next_fd_watermark` raises the counter to
    /// `watermark + 1`.  A subsequent `insert()` allocates from
    /// there, not from the old counter position.
    #[tokio::test]
    async fn seed_next_fd_watermark_raises_counter() {
        let table = FileHandleTable::new();
        table.seed_next_fd_watermark(100);
        assert_eq!(table.snapshot_next_fd(), 101);
        let fd = table.insert(shadow_handle(0x01)).await.unwrap();
        assert_eq!(fd, 101, "insert allocates from the seeded watermark");
    }

    /// `seed_next_fd_watermark` is monotonic — a lower watermark
    /// does NOT rewind the counter.  Preserves the "closed fd is
    /// never re-issued" invariant across rollback.
    #[tokio::test]
    async fn seed_next_fd_watermark_is_monotonic() {
        let table = FileHandleTable::new();
        table.seed_next_fd_watermark(100);
        assert_eq!(table.snapshot_next_fd(), 101);
        // Attempt to rewind to a lower watermark — no-op.
        table.seed_next_fd_watermark(50);
        assert_eq!(table.snapshot_next_fd(), 101);
        // Same watermark — also no-op.
        table.seed_next_fd_watermark(100);
        assert_eq!(table.snapshot_next_fd(), 101);
    }

    /// `seed_next_fd_watermark(u64::MAX)` saturates safely at
    /// `u64::MAX` (the `.saturating_add(1)` guard) rather than
    /// wrapping to zero.  A subsequent `insert()` then trips
    /// the wrap guard from PR #495 and returns `Err(())`.
    #[tokio::test]
    async fn seed_next_fd_watermark_saturates_at_u64_max() {
        let table = FileHandleTable::new();
        table.seed_next_fd_watermark(u64::MAX);
        assert_eq!(table.snapshot_next_fd(), u64::MAX);
        // Subsequent insert trips the wrap guard.
        assert_eq!(table.insert(shadow_handle(0x00)).await, Err(()));
    }

    /// `seed_next_fd_from_state_hash` derives a deterministic
    /// watermark from the hash prefix.  Same hash → same
    /// watermark (consensus commitment across validators).
    #[tokio::test]
    async fn seed_next_fd_from_state_hash_is_deterministic() {
        let table_a = FileHandleTable::new();
        let table_b = FileHandleTable::new();
        // Distinctive 32-byte hash.
        let hash: [u8; 32] = [
            0xAB, 0xCD, 0xEF, 0x01, 0x23, 0x45, 0x67, 0x89, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00,
        ];
        table_a.seed_next_fd_from_state_hash(&hash);
        table_b.seed_next_fd_from_state_hash(&hash);
        assert_eq!(
            table_a.snapshot_next_fd(),
            table_b.snapshot_next_fd(),
            "two validators seeding from the same state hash MUST \
             derive identical watermarks — this is a consensus \
             commitment"
        );
    }

    /// The state-hash-derived watermark leaves at least
    /// `FD_ENTROPY_HEADROOM_BITS` of headroom before the next
    /// watermark's range.  Pin the low-bit mask: the resulting
    /// watermark's low 20 bits are zero, then `seed_next_fd_
    /// watermark` adds 1 (so bit 0 is set, but bits 1..20 stay
    /// clear).
    #[tokio::test]
    async fn seed_next_fd_from_state_hash_masks_headroom_bits() {
        let table = FileHandleTable::new();
        let hash: [u8; 32] = [
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        table.seed_next_fd_from_state_hash(&hash);
        // hi = u64::MAX (all 8 leading bytes are 0xFF).
        // watermark = u64::MAX & !((1 << 20) - 1) = u64::MAX ^ 0xFFFFF = 0xFFFF_FFFF_FFF0_0000.
        // seed_next_fd_watermark stores watermark + 1 = 0xFFFF_FFFF_FFF0_0001.
        assert_eq!(table.snapshot_next_fd(), 0xFFFF_FFFF_FFF0_0001);
    }

    /// `truncate_to` closes every fd allocated past the snapshot.
    /// The counter does NOT rewind — a subsequent `insert()`
    /// allocates from where the counter was before truncate.
    #[tokio::test]
    async fn truncate_to_drops_fds_past_snapshot_but_leaves_counter_monotonic() {
        let table = FileHandleTable::new();
        // Allocate 3 fds → 1, 2, 3.  Snapshot AFTER fd 2 = 3.
        let fd1 = table.insert(shadow_handle(0x01)).await.unwrap();
        let fd2 = table.insert(shadow_handle(0x02)).await.unwrap();
        let snapshot = table.snapshot_next_fd();
        assert_eq!(snapshot, 3);
        let fd3 = table.insert(shadow_handle(0x03)).await.unwrap();
        assert_eq!((fd1, fd2, fd3, table.snapshot_next_fd()), (1, 2, 3, 4));

        // Rollback: fd3 goes away; fd1 and fd2 survive.
        table.truncate_to(snapshot).await;
        assert!(table.with_mut(fd1, |_| ()).await.is_some());
        assert!(table.with_mut(fd2, |_| ()).await.is_some());
        assert!(table.with_mut(fd3, |_| ()).await.is_none());

        // Counter does NOT rewind.  A new insert allocates fd 4,
        // NOT fd 3 (which would alias the just-rolled-back fd).
        let fd4 = table.insert(shadow_handle(0x04)).await.unwrap();
        assert_eq!(fd4, 4, "counter monotonicity — no fd reuse across rollback");
    }

    /// `truncate_to(snapshot)` with `snapshot` at-or-past the
    /// current counter is a no-op (defensive stale-snapshot
    /// handling).
    #[tokio::test]
    async fn truncate_to_past_current_counter_is_noop() {
        let table = FileHandleTable::new();
        table.insert(shadow_handle(0x01)).await.unwrap();
        table.truncate_to(999).await;
        // Handle survives.
        assert!(table.with_mut(1, |_| ()).await.is_some());
    }
}
