//! Phase 8 range-lock registry (slice 8a MVP — `wait: false` only).
//!
//! Coordinates cross-cap range-lock state on a single node.  Keyed on
//! `(dev, inode)` so two fresh-mint `File` caps opened by different
//! Rholang callers over the same on-disk file collapse to a single
//! coordination entry (cross-cap contention becomes observable rather
//! than silently racing at the fd layer).
//!
//! See the plan's Phase 8 §X-1 memo for the design rationale; this
//! module is the substrate for `fs_lock_range` / `fs_release_lock`
//! natives + the `File.lockRange` / `LockToken` Rholang surface.
//!
//! Colocated on `RuntimeManager` alongside `RootIdentityRegistry` and
//! shared to every spawned runtime via `share_lock_registry` (mirrors
//! the H-5 broadcast pattern).
//!
//! ## Mode-differentiated semantics
//!
//! - **Consensus mode**: `LockRegistry` is a hard invariant.  `is_locked`
//!   gates `fs_remove_file` / `fs_remove_dir`; cross-cap contention
//!   returns `FSERR_BUSY` deterministically.
//! - **Oracular mode**: `LockRegistry` is a best-effort in-process
//!   coordination hint.  Serializes this node's own callers on the same
//!   physical file; does NOT prevent external processes from writing to
//!   the same inode.  `fs_remove_file` does not gate on `is_locked`
//!   under oracular (matches host semantics — you can `rm` a file while
//!   another process has it open).
//!
//! ## Wait:true (slice 8b, 2026-08-12)
//!
//! Slice 8a's MVP returned `Err(LockError::Busy)` immediately on any
//! conflict.  Slice 8b adds blocking acquisition via the Rig-protocol
//! (plan §X-2).  Two orthogonal pieces sit at this layer:
//!
//! 1. **Waiter queue** — each `(dev, inode)`'s `FileLockState` carries a
//!    FIFO `waiters: VecDeque<Waiter>`.  A caller invoking
//!    `try_acquire_range_wait(..., WaitPolicy::Wait)` on a conflict
//!    parks in the queue rather than failing.  Every release path
//!    (`release`, `release_all_for_holder`, `release_all_for_deploy`)
//!    scans the queue head-first and admits admissible waiters; the
//!    caller awaits a `tokio::sync::oneshot::Receiver` for the outcome.
//!
//! 2. **Rig-protocol synthesis (slice 8b sub-2, native handler layer)**
//!    — cancelled waiters emit a synthesized error Produce via
//!    `Produce::with_error()` + `update_produce`, mirroring
//!    `reduce.rs::produce_inner` line 369 (the OpenAI/Ollama pathway).
//!    That's a native-handler concern; `LockRegistry` itself only
//!    signals `Err(LockError::Cancelled)` through the oneshot and lets
//!    the native perform the Produce synthesis.
//!
//! ## Head-of-line admission (2026-08-12)
//!
//! Strict FIFO: when a release makes room, the head waiter is checked;
//! if it fits, admit + loop; if not, stop.  Downstream waiters do not
//! overtake even when they would fit.  This trades throughput for
//! writer-anti-starvation and matches plan §950's FIFO commitment.
//! Priority / fairness / hash-derived shuffling are candidate future
//! schedulers; the `AcquireOutcome` / `WaitPolicy` API surface hides
//! the choice.
//!
//! ## Consensus-committed constants
//!
//! `MAX_RANGES_PER_FILE` and `LOCK_ID_CEILING` both govern when
//! `fs_lock_range` returns `FSERR_QUOTA_EXCEEDED`.  Because that
//! reply is visible to Rholang callers, all validators must agree
//! on both constants — a validator running with a different value
//! would produce a divergent reply on the same call sequence,
//! forking consensus.  Treat these as hard-fork surface (catalog
//! item #12 in `snapshot.rs`'s module docstring); pinned by
//! `max_ranges_per_file_pinned_at_1024` and `lock_id_ceiling_pinned`
//! in this module's test suite.  A change is a coordinated network
//! upgrade, not a per-node tune-up.
//!
//! `LockId` VALUES themselves are per-runtime and NOT
//! consensus-observable — only the QuotaExceeded threshold is.
//!
//! ## Cross-cap forgery safety
//!
//! `LockId` is a guessable monotone `u64`; naively an attacker could
//! enumerate ids and call `fs_release_lock(id)` to release someone
//! else's lock.  This is blocked by construction at the URN layer:
//! slice 31's phase-scoped URN filter prevents user code from binding
//! `rho:io:fs:native:*` URNs, so `fs_release_lock` is only reachable
//! via the genesis-scope `LockToken` agent in `File.rho`.  Each
//! `LockToken` instance holds its `LockId` in an unforgeable `stateP`
//! `GPrivate` cell — a receiver only obtains the id by receiving the
//! token itself, which is the intended ocap semantic.  Attenuation
//! ("hold this token but don't release") uses the standard
//! forwarder-filter pattern (spec §Ocap patterns > Attenuation).

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use tokio::sync::oneshot;

/// Filesystem identity — `(st_dev, st_ino)` from `fstat(2)`.  Keying
/// on this collapses hard-linked aliases, bind-mount duplicates, and
/// symlink chains to a single lock entry.
pub type DevInode = (u64, u64);

/// Opaque tag identifying which deploy owns this lock, for
/// `release_all_for_deploy` auto-release at deploy-end (WalDeployScope
/// hook, MUST per X-4 / spec §Explicit locks).  Concretely, whatever
/// key `WalDeployScope` already uses — typically the deploy hash.
pub type DeployScope = [u8; 32];

/// Opaque per-runtime handle returned by `try_acquire` and passed back
/// to `release`.  Also carried inside the Rholang-side `LockToken`
/// agent's `stateP`.
///
/// Not consensus-observable: allocated by a monotone atomic counter
/// per-runtime; validators do not compare these across the wire.  The
/// Rig-protocol layer above (slice 8b) ensures deterministic outcomes;
/// LockIds are ephemeral labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LockId(pub u64);

/// Cap-scoped identity for `release_all_for_holder` cleanup on
/// `File.close`.  Derived from the File agent's per-instance `this`
/// GPrivate name at cap-mint time — unique per fresh-mint open.
///
/// **Not** derived from `stateP`: `stateP` is module-level bound in
/// File.rho's outer `new` clause and therefore shared across every
/// File-cap instance minted through the composed source.  Using
/// `*stateP` as the holder input would collapse every cap on a
/// given runtime into a single HolderId → cross-cap coordination
/// would silently degrade to a same-holder no-op.  `*this` is the
/// bundled dispatch channel from `agent File { ... }`'s desugaring
/// (`new this, private in { ... }` inside each per-instance
/// constructor invocation), so each fresh-mint cap has a distinct
/// `this` name and therefore a distinct HolderId.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HolderId {
    pub bytes: [u8; 32],
}

impl HolderId {
    pub fn from_bytes(bytes: [u8; 32]) -> Self { Self { bytes } }
}

/// Requested access mode.  Multiple readers of overlapping ranges
/// coexist; a writer conflicts with any overlapping reader OR writer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockMode {
    Read,
    Write,
}

/// One granted range lock.
#[derive(Debug, Clone)]
pub struct RangeEntry {
    pub id: LockId,
    pub offset: u64,
    pub length: u64,
    pub mode: LockMode,
    pub holder: HolderId,
    pub deploy: DeployScope,
}

/// Per-`(dev, inode)` lock state.  Two disjoint substructures:
///
/// - `ranges` — positional range locks (from `bytesAt`, `writeBytesAt`,
///   `readInto`, `readAtInto`, `writeFrom`, `writeFromAt`, and
///   explicit `lockRange`).
/// - `sequential_holder` — the whole-file lock held by an active
///   sequential stream (`chars`, `bytes`, `lines`, `readLine`,
///   `writeChars`, `writeBytes`, `writeLine`, `writeLines`,
///   `writeString`, `writeByteArray`).  Represented as a separate flag
///   rather than a full-range entry per §Slice-1 commitments — cheaper
///   for the common case where a file has either sequential-only or
///   positional-only traffic.
///
/// Both are checked and mutated under the same `RwLock` write guard,
/// so they never disagree.  Coexistence rule per FIP §1143 "a
/// sequential stream conflicts with any positional stream and vice
/// versa": a positional acquire requires `sequential_holder.is_none()`
/// AND no overlapping range; a sequential acquire requires both empty.
///
/// NOTE: MVP uses `Vec<RangeEntry>` scanned linearly on every
/// acquire/release.  Correct against the four operations at any N;
/// appropriate for the small-N contention profile expected in MVP
/// workloads.  Candidate future optimization: `BTreeMap<offset, ...>`
/// or segment tree once real workloads expose N large enough for the
/// scan cost to matter.  The API surface hides the representation, so
/// the swap is behind an implementation boundary.
#[derive(Debug, Default)]
pub struct FileLockState {
    pub ranges: Vec<RangeEntry>,
    pub sequential_holder: Option<SequentialEntry>,
    /// FIFO queue of `wait: true` acquires that hit a conflict.  Each
    /// release path scans the head; admissible waiters are promoted to
    /// `ranges` / `sequential_holder` and their `admit` senders are
    /// signalled.  Strict head-of-line: a non-admissible head blocks
    /// admission of subsequent waiters (writer-anti-starvation).
    ///
    /// A state with parked waiters is NOT evicted from the registry
    /// map even if `ranges` and `sequential_holder` are empty — the
    /// waiters need somewhere to live until admit or cancel.
    waiters: VecDeque<Waiter>,
}

/// Sequential-stream whole-file lock — one per `(dev, inode)`.
#[derive(Debug, Clone)]
pub struct SequentialEntry {
    pub id: LockId,
    pub holder: HolderId,
    pub deploy: DeployScope,
}

/// Per-`(dev, inode)` cap on concurrent range entries.  Bounds the
/// linear-scan cost of `try_acquire_range` and `is_locked` against
/// pathological workloads (many disjoint tiny locks on one file).
/// Matches `MAX_OPEN_FDS`'s scale so a runtime's aggregate lock count
/// is bounded by `MAX_OPEN_FDS × MAX_RANGES_PER_FILE`.  A hostile
/// deploy hitting this cap gets `FSERR_QUOTA_EXCEEDED` at the native
/// boundary and cannot amplify further.
pub const MAX_RANGES_PER_FILE: usize = 1024;

/// Per-`(dev, inode)` cap on parked `wait: true` acquirers (Phase 8
/// NB-3, 2026-09-02).  Symmetric with `MAX_RANGES_PER_FILE` — bounds
/// the `waiters: VecDeque<Waiter>` deque against a hostile deploy
/// that spams `try_acquire_range_wait(..., WaitPolicy::Wait)` on a
/// locked file.  Each `Waiter` allocates ~150 bytes (LockId +
/// WaitKind + HolderId + DeployScope + oneshot::Sender pair), so at
/// saturation with `MAX_OPEN_FDS × MAX_WAITERS_PER_FILE` the
/// runtime-wide waiter memory tops out at ~150 MiB.
///
/// A hostile deploy hitting this cap gets `FSERR_QUOTA_EXCEEDED` at
/// the native boundary — same code as the live-range cap so
/// callers do not need to differentiate.  Pre-existing bounds
/// (per-deploy cost budget + `cancel_all_waiters_for_deploy` on
/// `WalDeployScope::Drop` + per-block runtime respawn) already
/// bounded the practical worst case; this cap is defense-in-depth
/// against future cost-tuning changes that might inadvertently
/// lower per-call cost enough to allow massive waiter allocations.
///
/// **Hard-fork surface** — per `docs/consensus-invariants.md § 5
/// byte gates` in `f1r3node-rust`.  Changing this value or the
/// error code path in any shard-live deploy would produce divergent
/// WAL / reply bytes vs. validators still on the pre-change value.
pub const MAX_WAITERS_PER_FILE: usize = 1024;

/// Guard threshold on the LockId counter.  With 2⁶⁴ headroom the
/// wrap is astronomical (~584 years at 10⁹ acquires/sec), but a
/// wrap would collide new LockIds with stale `LockToken`s still in
/// RSpace — allowing a spurious release-after-release.  Refusing
/// acquisitions past `LOCK_ID_CEILING` closes that vector at
/// negligible cost.  Set to `u64::MAX - 2¹⁶` so we have plenty of
/// warning before hard failure.
pub const LOCK_ID_CEILING: u64 = u64::MAX - (1 << 16);

/// Errors surfaced through the native handlers as `FSERR_*` codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockError {
    /// Requested range conflicts with an existing holder.  Maps to
    /// `FSERR_BUSY` at the native boundary.
    Busy,
    /// `release` called with a `LockId` that isn't held (double
    /// release, release after `File.close` swept it, wait:true
    /// cancellation resolved first, etc.).  Maps to `FSERR_CLOSED`.
    Closed,
    /// Zero-length range or other malformed input.  Maps to
    /// `FSERR_BAD_ARG` at the native boundary.  A zero-length "lock"
    /// would protect nothing and never conflict with anything, so
    /// silently accepting it invites subtle race bugs — reject at the
    /// boundary instead.
    BadArg,
    /// Per-`(dev, inode)` range cap reached (`MAX_RANGES_PER_FILE`),
    /// or LockId counter approaching `LOCK_ID_CEILING`.  Maps to
    /// `FSERR_QUOTA_EXCEEDED` at the native boundary.
    QuotaExceeded,
    /// A `wait: true` acquire was cancelled while parked — either via
    /// explicit `cancel_wait`, via the deploy-end sweep
    /// (`cancel_all_waiters_for_deploy`), or because the `LockRegistry`
    /// was dropped with waiters still parked.  Maps to
    /// `FSERR_CANCELLED` at the native boundary.  Slice 8b sub-2 wires
    /// this to a synthesized-error Produce via `Produce::with_error()`
    /// so the follower's replay path sees the outcome deterministically
    /// (plan §X-2).
    Cancelled,
}

/// Whether a conflicting acquire fails fast or parks in the FIFO
/// waiter queue.  Slice 8a MVP always uses `Fail`; slice 8b's
/// `lockRange(..., {"wait": true})` opts into `Wait`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitPolicy {
    /// Return `Err(LockError::Busy)` on conflict (MVP).
    Fail,
    /// On conflict, mint a `LockId`, enqueue as a `Waiter` in the
    /// per-`(dev, inode)` FIFO queue, and return
    /// `AcquireOutcome::Parked { lock_id, admit }`.  The caller
    /// awaits `admit` for the eventual outcome.
    Wait,
}

/// Outcome of a `try_acquire_range_wait` / `try_acquire_sequential_wait`
/// call.  Wraps the immediate-success path and the parked path.
///
/// Under `WaitPolicy::Fail`, only `Immediate` is ever returned; a
/// conflict short-circuits to `Err(LockError::Busy)`.
#[derive(Debug)]
pub enum AcquireOutcome {
    /// Acquired immediately.  Behaves exactly like the pre-slice-8b
    /// `Ok(LockId)` return.
    Immediate(LockId),
    /// Parked in the waiter queue.  `lock_id` is the id that WILL be
    /// granted on admission (also the handle for `cancel_wait`).
    /// `admit` resolves to:
    ///   - `Ok(Ok(lock_id))` when a release path admits this waiter,
    ///   - `Ok(Err(LockError::Cancelled))` when the deploy-end sweep
    ///     or an explicit `cancel_wait` fires,
    ///   - `Err(_)` (oneshot RecvError) if the `LockRegistry` is
    ///     dropped without signalling — the caller should treat this
    ///     as `Cancelled` too (see `ParkedHandle::wait` helper).
    Parked {
        lock_id: LockId,
        admit: oneshot::Receiver<Result<LockId, LockError>>,
    },
}

/// One parked wait entry.  Private — the waiter's identity is only
/// visible externally as its `LockId` (used by `cancel_wait`).
#[derive(Debug)]
struct Waiter {
    lock_id: LockId,
    kind: WaitKind,
    holder: HolderId,
    deploy: DeployScope,
    /// Signalled with `Ok(lock_id)` on admission or
    /// `Err(LockError::Cancelled)` on cancel.  Dropping the sender
    /// (registry drop / waiter removal without signal) surfaces to
    /// the receiver as `Err(RecvError)` which the caller treats as
    /// Cancelled.
    admit: oneshot::Sender<Result<LockId, LockError>>,
}

/// What kind of lock a parked waiter is trying to take.
#[derive(Debug)]
enum WaitKind {
    Range {
        offset: u64,
        length: u64,
        mode: LockMode,
    },
    Sequential,
}

/// Range-lock registry — shared across every runtime spawned from a
/// single `RuntimeManager` via `share_lock_registry` (mirrors the
/// H-5 `RootIdentityRegistry` broadcast pattern).
///
/// `RwLock` gives contention-free concurrent reads for the `is_locked`
/// query on the unlink gate hot path.  Writes serialize acquire /
/// release / sweep — expected low volume vs. read path.
///
/// `next_lock_id` is a monotone `AtomicU64` — LockIds are ephemeral
/// per-runtime handles, not consensus-observable.
#[derive(Debug, Clone, Default)]
pub struct LockRegistry {
    inner: Arc<RwLock<HashMap<DevInode, FileLockState>>>,
    next_lock_id: Arc<AtomicU64>,
}

impl LockRegistry {
    pub fn new() -> Self { Self::default() }

    /// Mint the next `LockId`.  Skips zero (sentinel-friendly) and
    /// refuses past `LOCK_ID_CEILING` to prevent 2⁶⁴-wrap collisions
    /// with stale `LockToken`s still in RSpace.
    fn mint_id(&self) -> Result<LockId, LockError> {
        let raw = self.next_lock_id.fetch_add(1, Ordering::SeqCst);
        if raw >= LOCK_ID_CEILING {
            // Roll back so we don't advance past the ceiling forever
            // (multiple threads racing here is fine — they all see the
            // same over-ceiling condition and all return QuotaExceeded).
            self.next_lock_id.store(LOCK_ID_CEILING, Ordering::SeqCst);
            return Err(LockError::QuotaExceeded);
        }
        Ok(LockId(raw.wrapping_add(1)))
    }

    /// Try to acquire a positional range lock.  Returns `Ok(LockId)`
    /// on success; `Err(LockError::Busy)` on conflict (either a
    /// sequential-stream holder exists on this `(dev, inode)` or an
    /// overlapping range conflicts per read-vs-write rules);
    /// `Err(LockError::BadArg)` on zero-length range;
    /// `Err(LockError::QuotaExceeded)` if per-file range cap or
    /// LockId ceiling is hit.
    ///
    /// ## Same-holder re-entry (2026-08-12, closing X-1 §Explicit locks gap)
    ///
    /// Overlapping ranges from the *same* holder never conflict, regardless
    /// of mode.  Spec §Explicit locks: `lockRange` "Composes with implicit
    /// locks acquired by concurrent positional calls."  A File cap that
    /// holds an explicit `lockRange(0, 1024, "w")` must be able to do
    /// positional I/O inside that range through the same cap — its inline
    /// auto-acquire (with the same holder) would otherwise trip a W-vs-W
    /// self-conflict and return `FSERR_BUSY`, defeating the compositional
    /// promise.  The File-agent dispatch loop's `stateP`-linear-receive
    /// already serializes two intra-cap operations at the Rholang layer, so
    /// there is no kernel-fd race for this rule to guard against; the rule
    /// just lets the `LockRegistry` reflect the same "no self-conflict"
    /// semantic Rholang callers observe.
    ///
    /// Scope: **positional-vs-positional only**.  Sequential acquisition
    /// (see `try_acquire_sequential`) stays strict — a File cap holding a
    /// range lock cannot also open a sequential stream through itself,
    /// matching FIP §1143's "one active sequential stream per File" and
    /// "a sequential stream conflicts with any positional stream and vice
    /// versa."  Same-holder read-vs-write is granted (holder already has
    /// full cap authority; `lockRange` is for cross-holder coordination,
    /// not intra-cap access-tier enforcement).  Each same-holder acquire
    /// still mints a fresh `LockId`; each `release(id)` removes exactly
    /// the one entry with that id.
    pub fn try_acquire_range(
        &self,
        dev_inode: DevInode,
        offset: u64,
        length: u64,
        mode: LockMode,
        holder: HolderId,
        deploy: DeployScope,
    ) -> Result<LockId, LockError> {
        match self.try_acquire_range_wait(
            dev_inode,
            offset,
            length,
            mode,
            holder,
            deploy,
            WaitPolicy::Fail,
        )? {
            AcquireOutcome::Immediate(id) => Ok(id),
            AcquireOutcome::Parked { .. } => {
                unreachable!("WaitPolicy::Fail never parks — parking is a Wait-only outcome")
            }
        }
    }

    /// Slice-8b wait-aware variant of `try_acquire_range`.
    ///
    /// - `WaitPolicy::Fail` → identical to `try_acquire_range`: returns
    ///   `Immediate(id)` on success, `Err(Busy)` on conflict.
    /// - `WaitPolicy::Wait` → on conflict, mints a `LockId`, enqueues
    ///   a `Waiter` at the tail of the per-`(dev, inode)` FIFO queue,
    ///   and returns `Parked { lock_id, admit }`.  The caller awaits
    ///   `admit` for the eventual outcome (`Ok(id)` on admission via
    ///   any release path, `Err(Cancelled)` on cancel or registry drop).
    ///
    /// `Err(BadArg)` on zero-length range and `Err(QuotaExceeded)` on
    /// `MAX_RANGES_PER_FILE` cap are still returned eagerly under both
    /// policies — those aren't conflicts that could resolve by waiting.
    pub fn try_acquire_range_wait(
        &self,
        dev_inode: DevInode,
        offset: u64,
        length: u64,
        mode: LockMode,
        holder: HolderId,
        deploy: DeployScope,
        wait_mode: WaitPolicy,
    ) -> Result<AcquireOutcome, LockError> {
        if length == 0 {
            // A zero-length lock protects nothing and never conflicts;
            // silently accepting invites subtle race bugs.  Reject.
            return Err(LockError::BadArg);
        }
        let mut guard = self.inner.write().expect("lock registry poisoned");
        let state = guard.entry(dev_inode).or_default();
        // MAX_RANGES_PER_FILE bounds LIVE ranges only, not parked
        // waiters — parked waiters have no allocated range slot yet.
        // A hostile deploy that spams `wait: true` parks is still
        // bounded by cancel_all_waiters_for_deploy at deploy-end.
        if state.ranges.len() >= MAX_RANGES_PER_FILE {
            return Err(LockError::QuotaExceeded);
        }
        if range_conflicts(state, offset, length, mode, &holder) {
            match wait_mode {
                WaitPolicy::Fail => return Err(LockError::Busy),
                WaitPolicy::Wait => {
                    // NB-3 (2026-09-02): per-file parked-waiter cap.
                    // Prevents a hostile deploy from allocating
                    // unbounded Waiter structs (~150 bytes each) by
                    // spamming wait:true on a locked file.  Same
                    // FSERR_QUOTA_EXCEEDED code as the live-range
                    // cap above; callers do not need to differentiate.
                    // Hard-fork surface — see MAX_WAITERS_PER_FILE
                    // docstring.
                    if state.waiters.len() >= MAX_WAITERS_PER_FILE {
                        return Err(LockError::QuotaExceeded);
                    }
                    let id = self.mint_id()?;
                    let (tx, rx) = oneshot::channel();
                    state.waiters.push_back(Waiter {
                        lock_id: id,
                        kind: WaitKind::Range {
                            offset,
                            length,
                            mode,
                        },
                        holder,
                        deploy,
                        admit: tx,
                    });
                    return Ok(AcquireOutcome::Parked {
                        lock_id: id,
                        admit: rx,
                    });
                }
            }
        }
        let id = self.mint_id()?;
        state.ranges.push(RangeEntry {
            id,
            offset,
            length,
            mode,
            holder,
            deploy,
        });
        Ok(AcquireOutcome::Immediate(id))
    }

    /// Try to acquire the whole-file sequential lock.  Returns
    /// `Ok(LockId)` only if `sequential_holder.is_none()` AND
    /// `ranges.is_empty()` per the coexistence rule; `Err(Busy)`
    /// otherwise; `Err(QuotaExceeded)` on LockId ceiling.
    pub fn try_acquire_sequential(
        &self,
        dev_inode: DevInode,
        holder: HolderId,
        deploy: DeployScope,
    ) -> Result<LockId, LockError> {
        match self.try_acquire_sequential_wait(dev_inode, holder, deploy, WaitPolicy::Fail)? {
            AcquireOutcome::Immediate(id) => Ok(id),
            AcquireOutcome::Parked { .. } => {
                unreachable!("WaitPolicy::Fail never parks — parking is a Wait-only outcome")
            }
        }
    }

    /// Slice-8b wait-aware variant of `try_acquire_sequential`.
    /// Same semantics as `try_acquire_range_wait` but for the
    /// whole-file sequential lock.  Sequential does NOT participate
    /// in the same-holder skip rule (see `try_acquire_range_wait`
    /// docstring); a File cap holding any lock cannot open a
    /// sequential stream through itself.
    pub fn try_acquire_sequential_wait(
        &self,
        dev_inode: DevInode,
        holder: HolderId,
        deploy: DeployScope,
        wait_mode: WaitPolicy,
    ) -> Result<AcquireOutcome, LockError> {
        let mut guard = self.inner.write().expect("lock registry poisoned");
        let state = guard.entry(dev_inode).or_default();
        if sequential_conflicts(state) {
            match wait_mode {
                WaitPolicy::Fail => return Err(LockError::Busy),
                WaitPolicy::Wait => {
                    // NB-3 (2026-09-02): symmetric per-file parked-
                    // waiter cap.  See MAX_WAITERS_PER_FILE docstring
                    // for the memory-bound + hard-fork rationale.
                    // Applied to sequential-wait as well as range-
                    // wait because both share the same
                    // `state.waiters` deque.
                    if state.waiters.len() >= MAX_WAITERS_PER_FILE {
                        return Err(LockError::QuotaExceeded);
                    }
                    let id = self.mint_id()?;
                    let (tx, rx) = oneshot::channel();
                    state.waiters.push_back(Waiter {
                        lock_id: id,
                        kind: WaitKind::Sequential,
                        holder,
                        deploy,
                        admit: tx,
                    });
                    return Ok(AcquireOutcome::Parked {
                        lock_id: id,
                        admit: rx,
                    });
                }
            }
        }
        let id = self.mint_id()?;
        state.sequential_holder = Some(SequentialEntry { id, holder, deploy });
        Ok(AcquireOutcome::Immediate(id))
    }

    /// Release a specific lock by id.  Returns `Ok(())` if the id was
    /// held (either as a range or the sequential holder), `Err(Closed)`
    /// if not.  Evicts the `(dev, inode)` entry from the map if both
    /// substructures become empty — closes the inode-reuse safety gap.
    pub fn release(&self, lock_id: LockId) -> Result<(), LockError> {
        let mut guard = self.inner.write().expect("lock registry poisoned");
        let mut touched_key: Option<DevInode> = None;
        let mut released = false;
        for (dev_inode, state) in guard.iter_mut() {
            if let Some(pos) = state.ranges.iter().position(|e| e.id == lock_id) {
                state.ranges.remove(pos);
                released = true;
            } else if state.sequential_holder.as_ref().map(|s| s.id) == Some(lock_id) {
                state.sequential_holder = None;
                released = true;
            }
            if released {
                touched_key = Some(*dev_inode);
                break;
            }
        }
        if let Some(k) = touched_key {
            if let Some(state) = guard.get_mut(&k) {
                wake_waiters(state);
                if state_is_empty(state) {
                    guard.remove(&k);
                }
            }
        }
        if released {
            Ok(())
        } else {
            Err(LockError::Closed)
        }
    }

    /// Release every lock held by `holder`.  Called from `File.close`
    /// — clears both positional ranges and the sequential flag if
    /// they belong to this cap.  Locks held on the same `(dev, inode)`
    /// via other caps are unaffected.  Returns the number of locks
    /// released (for diagnostics; caller may ignore).
    pub fn release_all_for_holder(&self, holder: &HolderId) -> usize {
        let mut guard = self.inner.write().expect("lock registry poisoned");
        let mut released = 0usize;
        let mut evict: Vec<DevInode> = Vec::new();
        for (dev_inode, state) in guard.iter_mut() {
            let before = state.ranges.len();
            state.ranges.retain(|e| &e.holder != holder);
            released += before - state.ranges.len();
            if state.sequential_holder.as_ref().map(|s| &s.holder) == Some(holder) {
                state.sequential_holder = None;
                released += 1;
            }
            // Wake any waiters (from OTHER holders) that now fit.
            // This holder's OWN parked waiters are separately swept by
            // `cancel_all_waiters_for_holder` — the caller (typically
            // `fs_release_all_for_holder` on File.close) invokes both.
            // Concerns kept separate for the same reason as the
            // release_all_for_deploy / cancel_all_waiters_for_deploy
            // split.
            wake_waiters(state);
            if state_is_empty(state) {
                evict.push(*dev_inode);
            }
        }
        for k in evict {
            guard.remove(&k);
        }
        released
    }

    /// Sweep every parked waiter belonging to `holder` — signals each
    /// `Err(LockError::Cancelled)` and returns the count cancelled.
    /// Called from `fs_release_all_for_holder` (File.close path) so
    /// the closed cap's parked wait:true acquires resolve
    /// deterministically with a Cancelled reply instead of hanging in
    /// the queue attached to a dead cap.
    ///
    /// Symmetrical with `cancel_all_waiters_for_deploy` but keyed on
    /// `HolderId` (per-cap) rather than `DeployScope` (per-deploy).
    /// A holder can span multiple `(dev, inode)` entries; the sweep
    /// visits all.
    pub fn cancel_all_waiters_for_holder(&self, holder: &HolderId) -> usize {
        let mut guard = self.inner.write().expect("lock registry poisoned");
        let mut cancelled = 0usize;
        let mut evict: Vec<DevInode> = Vec::new();
        for (dev_inode, state) in guard.iter_mut() {
            let (matching, keep): (VecDeque<Waiter>, VecDeque<Waiter>) =
                state.waiters.drain(..).partition(|w| &w.holder == holder);
            state.waiters = keep;
            for waiter in matching {
                let _ = waiter.admit.send(Err(LockError::Cancelled));
                cancelled += 1;
            }
            if state_is_empty(state) {
                evict.push(*dev_inode);
            }
        }
        for k in evict {
            guard.remove(&k);
        }
        cancelled
    }

    /// Release every lock owned by `deploy`.  Called from the
    /// `WalDeployScope::end` auto-release hook (MUST per X-4 / spec
    /// §Explicit locks).  Returns the number of locks released.
    ///
    /// # Sentinel guard: `[0; 32]` is reserved as the slice-8a step-4
    /// placeholder for "no real DeployScope wired yet."  Step 5's
    /// natives (`fs_lock_range` / `fs_lock_sequential`) pass this
    /// placeholder while step 5 was unimplemented.  Calling
    /// `release_all_for_deploy(&[0; 32])` outside a live
    /// `WalDeployScope` would sweep EVERY currently-held lock with a
    /// sentinel-scope entry — under normal step-5 operation there
    /// should be none (every acquire under a live WalDeployScope
    /// records the real deploy scope), but a pre-step-5 partial-wire
    /// bug could leave stray sentinel-scoped entries.  The assert
    /// turns that into a loud panic in release builds too — defense-
    /// in-depth promoted from debug_assert during the step-5 review
    /// (2026-08-13).  Small runtime cost (one comparison per sweep)
    /// vs. catching a "silently nuke every lock" regression.
    pub fn release_all_for_deploy(&self, deploy: &DeployScope) -> usize {
        assert!(
            deploy != &[0u8; 32],
            "release_all_for_deploy called with the [0; 32] sentinel — this is \
             the pre-step-5 placeholder DeployScope.  A live WalDeployScope \
             derives a non-sentinel scope via Blake2b256; the sentinel guard \
             fires only when a caller invokes release_all_for_deploy outside \
             a WalDeployScope guard, which would sweep every stray sentinel-\
             scoped entry."
        );
        let mut guard = self.inner.write().expect("lock registry poisoned");
        let mut released = 0usize;
        let mut evict: Vec<DevInode> = Vec::new();
        for (dev_inode, state) in guard.iter_mut() {
            let before = state.ranges.len();
            state.ranges.retain(|e| &e.deploy != deploy);
            released += before - state.ranges.len();
            if state.sequential_holder.as_ref().map(|s| &s.deploy) == Some(deploy) {
                state.sequential_holder = None;
                released += 1;
            }
            // After removing this deploy's held locks, wake any
            // waiters (from OTHER deploys) that now fit.  Waiters
            // belonging to THIS deploy are separately cancelled by
            // `cancel_all_waiters_for_deploy` (slice-8b sub-3 wires
            // both together in the WalDeployScope::drop hook).
            wake_waiters(state);
            if state_is_empty(state) {
                evict.push(*dev_inode);
            }
        }
        for k in evict {
            guard.remove(&k);
        }
        released
    }

    /// Consensus-mode `fs_remove_file` / `fs_remove_dir` unlink gate.
    /// Returns true if any lock (positional or sequential) is held on
    /// `(dev, inode)` that overlaps the queried range.  Callers under
    /// consensus mode surface `FSERR_BUSY` on true; oracular callers
    /// skip this check entirely and log-warn instead.
    ///
    /// For a whole-file query (delete or truncate-to-zero), pass
    /// `range = (0, u64::MAX)`.
    pub fn is_locked(&self, dev_inode: DevInode, range: (u64, u64)) -> bool {
        let guard = self.inner.read().expect("lock registry poisoned");
        let Some(state) = guard.get(&dev_inode) else {
            return false;
        };
        if state.sequential_holder.is_some() {
            return true;
        }
        state
            .ranges
            .iter()
            .any(|e| ranges_overlap((e.offset, e.length), range))
    }

    /// Count locks held on `dev_inode` (positional ranges +
    /// sequential-holder flag).  Returns 0 for a non-tracked inode.
    ///
    /// Used by the step-6 Oracular unlink log-warn to include the
    /// spec-mandated `{N} holder(s)` count (spec §Mode-differentiated
    /// invariants).  The count is holder-agnostic — a single holder
    /// with 3 same-holder-skip range acquires (per Prep A) reports
    /// 3 here, not 1.  Sequential holder counts as +1 if present.
    ///
    /// Read-lock scoped, cheap: single HashMap lookup + one
    /// arithmetic combination.  Suitable for hot-path use in the
    /// unlink handlers.
    pub fn count_locks(&self, dev_inode: DevInode) -> usize {
        let guard = self.inner.read().expect("lock registry poisoned");
        let Some(state) = guard.get(&dev_inode) else {
            return 0;
        };
        state.ranges.len() + state.sequential_holder.iter().count()
    }

    /// Diagnostic count of currently-tracked `(dev, inode)` entries.
    pub fn tracked_files(&self) -> usize {
        let guard = self.inner.read().expect("lock registry poisoned");
        guard.len()
    }

    /// Diagnostic count of currently-held locks (positional + sequential).
    /// Does NOT include parked waiters — a waiter has no allocated
    /// range slot until it admits.
    pub fn held_locks(&self) -> usize {
        let guard = self.inner.read().expect("lock registry poisoned");
        guard
            .values()
            .map(|s| s.ranges.len() + s.sequential_holder.iter().count())
            .sum()
    }

    /// Diagnostic count of currently-parked waiters across all
    /// `(dev, inode)` entries.  Useful for tests + telemetry.
    pub fn parked_waiters(&self) -> usize {
        let guard = self.inner.read().expect("lock registry poisoned");
        guard.values().map(|s| s.waiters.len()).sum()
    }

    /// Cancel a specific parked waiter by its `LockId`.  Returns
    /// `true` if a waiter was found and cancelled (its admit oneshot
    /// received `Err(LockError::Cancelled)`), `false` if no parked
    /// waiter with that id exists (already admitted, already
    /// cancelled, or never parked — the id may correspond to a
    /// currently-held lock, which `cancel_wait` does NOT release; use
    /// `release` for that).
    ///
    /// Idempotent: repeat calls after cancellation are `false` no-ops.
    /// A cancellation does NOT wake other waiters — removing a parked
    /// (non-holding) entry doesn't free any resource for others.
    pub fn cancel_wait(&self, lock_id: LockId) -> bool {
        let mut guard = self.inner.write().expect("lock registry poisoned");
        let mut touched_key: Option<DevInode> = None;
        let mut cancelled = false;
        for (dev_inode, state) in guard.iter_mut() {
            if let Some(pos) = state.waiters.iter().position(|w| w.lock_id == lock_id) {
                let waiter = state.waiters.remove(pos).expect("position was valid");
                // Ignore send error: receiver already dropped means
                // the caller isn't listening (task was aborted, etc.);
                // the cancellation still succeeds from the registry's
                // perspective — the waiter is gone.
                let _ = waiter.admit.send(Err(LockError::Cancelled));
                cancelled = true;
                touched_key = Some(*dev_inode);
                break;
            }
        }
        if let Some(k) = touched_key {
            if let Some(state) = guard.get_mut(&k) {
                if state_is_empty(state) {
                    guard.remove(&k);
                }
            }
        }
        cancelled
    }

    /// Sweep every parked waiter belonging to `deploy` — signals each
    /// `Err(LockError::Cancelled)` and returns the count cancelled.
    /// Called from the `WalDeployScope::drop` hook alongside
    /// `release_all_for_deploy` to guarantee no waiter is leaked past
    /// deploy end (slice 8b sub-3).
    ///
    /// # Sentinel guard: `[0; 32]` is reserved as the pre-step-5
    /// placeholder DeployScope.  A production caller running under a
    /// live `WalDeployScope` derives a non-sentinel scope via
    /// Blake2b256; the sentinel guard fires only when a caller invokes
    /// `cancel_all_waiters_for_deploy` outside a WalDeployScope guard,
    /// which would sweep every stray sentinel-scoped waiter.  Mirrors
    /// the assert on `release_all_for_deploy`.
    pub fn cancel_all_waiters_for_deploy(&self, deploy: &DeployScope) -> usize {
        assert!(
            deploy != &[0u8; 32],
            "cancel_all_waiters_for_deploy called with the [0; 32] sentinel — this \
             is the pre-step-5 placeholder DeployScope.  A live WalDeployScope \
             derives a non-sentinel scope via Blake2b256; the sentinel guard fires \
             only when a caller invokes cancel_all_waiters_for_deploy outside a \
             WalDeployScope guard, which would sweep every stray sentinel-scoped \
             waiter."
        );
        let mut guard = self.inner.write().expect("lock registry poisoned");
        let mut cancelled = 0usize;
        let mut evict: Vec<DevInode> = Vec::new();
        for (dev_inode, state) in guard.iter_mut() {
            // Partition: keep non-matching, drain matching → cancel.
            let (matching, keep): (VecDeque<Waiter>, VecDeque<Waiter>) =
                state.waiters.drain(..).partition(|w| &w.deploy == deploy);
            state.waiters = keep;
            for waiter in matching {
                let _ = waiter.admit.send(Err(LockError::Cancelled));
                cancelled += 1;
            }
            if state_is_empty(state) {
                evict.push(*dev_inode);
            }
        }
        for k in evict {
            guard.remove(&k);
        }
        cancelled
    }

    /// Test-only: seed the LockId counter so tests can trigger the
    /// `LOCK_ID_CEILING` guard without doing 10¹⁹ acquisitions.
    #[cfg(test)]
    fn set_next_lock_id_for_testing(&self, v: u64) { self.next_lock_id.store(v, Ordering::SeqCst); }
}

/// Predicate: does an incoming positional acquire of
/// `(offset, length, mode)` by `holder` conflict with any currently-
/// held lock in `state`?  Mirrors the check in
/// `try_acquire_range_wait` exactly — factored out so `wake_waiters`
/// can re-check admissibility without duplicating the logic.
///
/// Conflict rules (spec §1143 + X-1 same-holder skip):
///   - Any active sequential holder conflicts with any positional acquire.
///   - Overlapping reads coexist (R vs R never conflicts).
///   - Same-holder overlapping acquires coexist (positional-vs-positional).
///   - Otherwise, W vs R / W vs W on overlapping ranges conflicts.
fn range_conflicts(
    state: &FileLockState,
    offset: u64,
    length: u64,
    mode: LockMode,
    holder: &HolderId,
) -> bool {
    if state.sequential_holder.is_some() {
        return true;
    }
    for entry in &state.ranges {
        if !ranges_overlap((entry.offset, entry.length), (offset, length)) {
            continue;
        }
        if mode == LockMode::Read && entry.mode == LockMode::Read {
            continue;
        }
        if &entry.holder == holder {
            continue;
        }
        return true;
    }
    false
}

/// Predicate: does an incoming sequential acquire conflict with any
/// currently-held lock in `state`?  Sequential requires the state
/// entirely empty (no ranges, no sequential_holder) per FIP §1143's
/// coexistence rule.  Sequential does NOT use the same-holder skip.
fn sequential_conflicts(state: &FileLockState) -> bool {
    state.sequential_holder.is_some() || !state.ranges.is_empty()
}

/// Strict head-of-line FIFO wake pass.  Called after any release path
/// that removes a held lock from `state`.  Walks the waiter queue
/// from the front and:
///   - if the head is admissible (no conflicts with current holders),
///     pops it, promotes it to a held lock, and signals its admit
///     sender with `Ok(lock_id)`;
///   - if the admit sender's receiver has been dropped (caller's task
///     was aborted between park and admit), rolls back the promotion
///     — otherwise the "held" lock would be stranded.  Continues to
///     the next waiter in that case (the rolled-back state is the
///     same as it would have been if the aborted caller had never
///     parked);
///   - if the head is NOT admissible, stops.  Downstream waiters do
///     NOT overtake — strict FIFO / writer-anti-starvation.
///
/// Idempotent when `waiters` is empty.  Safe to call after any state
/// mutation; a no-op if nothing has changed.
fn wake_waiters(state: &mut FileLockState) {
    while let Some(head) = state.waiters.front() {
        // Check admissibility using the same rules as the direct
        // acquire path.
        let admissible = match head.kind {
            WaitKind::Range {
                offset,
                length,
                mode,
            } => {
                state.ranges.len() < MAX_RANGES_PER_FILE
                    && !range_conflicts(state, offset, length, mode, &head.holder)
            }
            WaitKind::Sequential => !sequential_conflicts(state),
        };
        if !admissible {
            break;
        }
        // Pop before promoting so a rollback can just re-check the
        // (now different) new head next iteration.
        let waiter = state.waiters.pop_front().expect("front just observed");
        let lock_id = waiter.lock_id;
        match waiter.kind {
            WaitKind::Range {
                offset,
                length,
                mode,
            } => {
                state.ranges.push(RangeEntry {
                    id: lock_id,
                    offset,
                    length,
                    mode,
                    holder: waiter.holder.clone(),
                    deploy: waiter.deploy,
                });
                if waiter.admit.send(Ok(lock_id)).is_err() {
                    // Receiver already gone (caller task cancelled
                    // locally).  Roll back the promotion so the slot
                    // returns to the free pool for the next waiter.
                    state.ranges.pop();
                }
            }
            WaitKind::Sequential => {
                let previous = state.sequential_holder.replace(SequentialEntry {
                    id: lock_id,
                    holder: waiter.holder.clone(),
                    deploy: waiter.deploy,
                });
                debug_assert!(
                    previous.is_none(),
                    "sequential_holder must be empty before admit — guarded by \
                     sequential_conflicts()"
                );
                if waiter.admit.send(Ok(lock_id)).is_err() {
                    // Same rollback as Range case.
                    state.sequential_holder = None;
                }
            }
        }
    }
}

/// A `FileLockState` is "empty" (safe to evict from the registry
/// map) only when it has no held locks AND no parked waiters.  A
/// state with parked waiters MUST NOT be evicted — dropping the
/// `Waiter`'s `admit` sender would signal cancel to the caller even
/// though nobody called `cancel_wait`, and the waiter would silently
/// disappear from the queue.
fn state_is_empty(state: &FileLockState) -> bool {
    state.ranges.is_empty() && state.sequential_holder.is_none() && state.waiters.is_empty()
}

/// Two half-open intervals `[o1, o1+l1)` and `[o2, o2+l2)` overlap iff
/// `o1 < o2 + l2` AND `o2 < o1 + l1`.  Zero-length ranges do not
/// overlap anything (defensive — the natives should reject
/// zero-length before calling here, but the invariant is cheap).
///
/// Uses saturating add to avoid overflow on `u64::MAX` end-of-file
/// sentinel ranges used by the sequential-flag whole-file query.
fn ranges_overlap(a: (u64, u64), b: (u64, u64)) -> bool {
    if a.1 == 0 || b.1 == 0 {
        return false;
    }
    let a_end = a.0.saturating_add(a.1);
    let b_end = b.0.saturating_add(b.1);
    a.0 < b_end && b.0 < a_end
}

#[cfg(test)]
mod tests {
    use super::*;

    fn holder(byte: u8) -> HolderId { HolderId::from_bytes([byte; 32]) }
    fn deploy(byte: u8) -> DeployScope { [byte; 32] }

    // -- ranges_overlap ---------------------------------------------------

    #[test]
    fn overlap_disjoint_returns_false() {
        assert!(!ranges_overlap((0, 100), (100, 100)));
        assert!(!ranges_overlap((100, 100), (0, 100)));
    }

    #[test]
    fn overlap_touching_boundary_returns_false() {
        // [0, 100) and [100, 200) share no bytes.
        assert!(!ranges_overlap((0, 100), (100, 100)));
    }

    #[test]
    fn overlap_partial_returns_true() {
        assert!(ranges_overlap((0, 100), (50, 100)));
        assert!(ranges_overlap((50, 100), (0, 100)));
    }

    #[test]
    fn overlap_contained_returns_true() {
        assert!(ranges_overlap((0, 1000), (100, 10)));
        assert!(ranges_overlap((100, 10), (0, 1000)));
    }

    #[test]
    fn overlap_zero_length_never_matches() {
        assert!(!ranges_overlap((0, 0), (0, 100)));
        assert!(!ranges_overlap((0, 100), (0, 0)));
    }

    #[test]
    fn overlap_end_of_file_sentinel_saturates() {
        // Whole-file query [0, u64::MAX) vs. any positive range.
        assert!(ranges_overlap((0, u64::MAX), (5000, 100)));
        // No overflow panic on u64::MAX offset.
        assert!(ranges_overlap((u64::MAX - 100, 200), (0, u64::MAX)));
    }

    // -- try_acquire_range ------------------------------------------------

    #[test]
    fn two_disjoint_reads_coexist() {
        let reg = LockRegistry::new();
        let a = reg
            .try_acquire_range((1, 42), 0, 100, LockMode::Read, holder(1), deploy(1))
            .expect("first acquire");
        let b = reg
            .try_acquire_range((1, 42), 200, 100, LockMode::Read, holder(2), deploy(1))
            .expect("second acquire");
        assert_ne!(a, b);
        assert_eq!(reg.held_locks(), 2);
    }

    #[test]
    fn two_overlapping_reads_coexist() {
        let reg = LockRegistry::new();
        reg.try_acquire_range((1, 42), 0, 100, LockMode::Read, holder(1), deploy(1))
            .unwrap();
        reg.try_acquire_range((1, 42), 50, 100, LockMode::Read, holder(2), deploy(1))
            .expect("overlapping reads must coexist");
    }

    #[test]
    fn reader_vs_writer_overlapping_conflicts() {
        let reg = LockRegistry::new();
        reg.try_acquire_range((1, 42), 0, 100, LockMode::Read, holder(1), deploy(1))
            .unwrap();
        let err = reg
            .try_acquire_range((1, 42), 50, 100, LockMode::Write, holder(2), deploy(1))
            .unwrap_err();
        assert_eq!(err, LockError::Busy);
    }

    #[test]
    fn writer_vs_writer_overlapping_conflicts() {
        let reg = LockRegistry::new();
        reg.try_acquire_range((1, 42), 0, 100, LockMode::Write, holder(1), deploy(1))
            .unwrap();
        let err = reg
            .try_acquire_range((1, 42), 50, 100, LockMode::Write, holder(2), deploy(1))
            .unwrap_err();
        assert_eq!(err, LockError::Busy);
    }

    #[test]
    fn different_dev_inode_is_independent() {
        let reg = LockRegistry::new();
        reg.try_acquire_range((1, 42), 0, 100, LockMode::Write, holder(1), deploy(1))
            .unwrap();
        // Same range, different inode: independent state.
        reg.try_acquire_range((1, 43), 0, 100, LockMode::Write, holder(1), deploy(1))
            .expect("different inode must not conflict");
    }

    // -- same-holder re-entry --------------------------------------------
    //
    // Closes the X-1 §Explicit locks compositional-promise gap: a File cap
    // that holds an explicit `lockRange` must be able to do positional I/O
    // in that range through the same cap without tripping its own W-vs-W
    // conflict.  Rule: overlapping ranges from the same holder never
    // conflict, regardless of mode.  Scope: positional-vs-positional only
    // (sequential stays strict per FIP §1143).

    #[test]
    fn same_holder_overlapping_writes_coexist() {
        let reg = LockRegistry::new();
        let a = reg
            .try_acquire_range((1, 42), 0, 1024, LockMode::Write, holder(1), deploy(1))
            .expect("explicit lockRange must succeed");
        // Same holder inline-acquires a sub-range at "w" — models
        // File.writeBytesAt(50, 20) inside the outer explicit lockRange.
        let b = reg
            .try_acquire_range((1, 42), 50, 20, LockMode::Write, holder(1), deploy(1))
            .expect("same-holder overlapping W must coexist (X-1 compositional promise)");
        assert_ne!(a, b, "same-holder re-entry mints a fresh LockId");
        assert_eq!(reg.held_locks(), 2);
    }

    #[test]
    fn same_holder_read_then_write_overlapping_coexists() {
        let reg = LockRegistry::new();
        // Same holder, R lock, then a W in the same range.  The holder
        // has full cap authority; lockRange is for cross-holder coordination,
        // not intra-cap access-tier enforcement.
        reg.try_acquire_range((1, 42), 0, 1024, LockMode::Read, holder(1), deploy(1))
            .unwrap();
        reg.try_acquire_range((1, 42), 50, 20, LockMode::Write, holder(1), deploy(1))
            .expect("same-holder R-then-W must coexist");
    }

    #[test]
    fn different_holder_overlapping_still_conflicts_when_outer_is_writer() {
        let reg = LockRegistry::new();
        // Same-holder rule must NOT relax cross-holder coordination.
        reg.try_acquire_range((1, 42), 0, 1024, LockMode::Write, holder(1), deploy(1))
            .unwrap();
        let err = reg
            .try_acquire_range((1, 42), 50, 20, LockMode::Write, holder(2), deploy(1))
            .unwrap_err();
        assert_eq!(err, LockError::Busy, "different holder must still conflict");
    }

    #[test]
    fn same_holder_range_release_frees_only_matching_entry() {
        let reg = LockRegistry::new();
        let outer = reg
            .try_acquire_range((1, 42), 0, 1024, LockMode::Write, holder(1), deploy(1))
            .unwrap();
        let inner = reg
            .try_acquire_range((1, 42), 50, 20, LockMode::Write, holder(1), deploy(1))
            .unwrap();
        // Releasing the inner acquisition must NOT drop the outer explicit
        // lock — cross-holder writes still conflict on the outer range.
        reg.release(inner).unwrap();
        assert_eq!(reg.held_locks(), 1, "outer lock survives inner release");
        let err = reg
            .try_acquire_range((1, 42), 100, 50, LockMode::Write, holder(2), deploy(1))
            .unwrap_err();
        assert_eq!(err, LockError::Busy);
        // Releasing the outer now frees the range for other holders.
        reg.release(outer).unwrap();
        reg.try_acquire_range((1, 42), 100, 50, LockMode::Write, holder(2), deploy(1))
            .expect("after outer release, other holder must succeed");
    }

    #[test]
    fn same_holder_sequential_still_conflicts_with_own_range() {
        let reg = LockRegistry::new();
        // FIP §1143: sequential streams conflict with any positional stream
        // and vice versa — including from the same cap.  Same-holder skip
        // is scoped to positional-vs-positional; it does not relax
        // sequential exclusion.
        reg.try_acquire_range((1, 42), 0, 100, LockMode::Read, holder(1), deploy(1))
            .unwrap();
        let err = reg
            .try_acquire_sequential((1, 42), holder(1), deploy(1))
            .unwrap_err();
        assert_eq!(err, LockError::Busy);
    }

    #[test]
    fn sequential_after_same_holder_range_still_conflicts() {
        // Mirror of `same_holder_sequential_still_conflicts_with_own_range`
        // (fold-in edge test from Prep A review): a positional acquire
        // followed by a sequential attempt from the same holder must return
        // Busy, matching FIP §1143.  Guards against a future refactor
        // extending same-holder-skip to sequential.
        let reg = LockRegistry::new();
        reg.try_acquire_range((1, 42), 0, 100, LockMode::Write, holder(1), deploy(1))
            .unwrap();
        // The mirror-case W range instead of the R range from the
        // pre-existing test; both must fail sequential acquisition.
        let err = reg
            .try_acquire_sequential((1, 42), holder(1), deploy(1))
            .unwrap_err();
        assert_eq!(err, LockError::Busy);
    }

    #[test]
    fn same_holder_range_hits_max_ranges_cap() {
        // Fold-in edge test from Prep A review: MAX_RANGES_PER_FILE
        // (line 264 in try_acquire_range) fires BEFORE the same-holder
        // skip.  A future refactor reordering the checks could silently
        // let a single cap consume unbounded entries; this pin catches it.
        let reg = LockRegistry::new();
        for i in 0..MAX_RANGES_PER_FILE as u64 {
            reg.try_acquire_range((1, 42), i * 100, 50, LockMode::Read, holder(1), deploy(1))
                .expect("acquires up to the cap must succeed");
        }
        // Same-holder overlapping-writer would normally be admitted by
        // the same-holder-skip; but at the cap it must fire QuotaExceeded
        // before the skip check even runs.
        let err = reg
            .try_acquire_range((1, 42), 0, 50, LockMode::Write, holder(1), deploy(1))
            .unwrap_err();
        assert_eq!(err, LockError::QuotaExceeded);
    }

    // -- sequential-flag coexistence -------------------------------------

    #[test]
    fn sequential_blocks_positional() {
        let reg = LockRegistry::new();
        reg.try_acquire_sequential((1, 42), holder(1), deploy(1))
            .unwrap();
        let err = reg
            .try_acquire_range((1, 42), 0, 100, LockMode::Read, holder(2), deploy(1))
            .unwrap_err();
        assert_eq!(err, LockError::Busy);
    }

    #[test]
    fn positional_blocks_sequential() {
        let reg = LockRegistry::new();
        reg.try_acquire_range((1, 42), 500, 10, LockMode::Read, holder(1), deploy(1))
            .unwrap();
        let err = reg
            .try_acquire_sequential((1, 42), holder(2), deploy(1))
            .unwrap_err();
        assert_eq!(err, LockError::Busy);
    }

    #[test]
    fn two_sequentials_conflict() {
        let reg = LockRegistry::new();
        reg.try_acquire_sequential((1, 42), holder(1), deploy(1))
            .unwrap();
        assert_eq!(
            reg.try_acquire_sequential((1, 42), holder(2), deploy(1))
                .unwrap_err(),
            LockError::Busy
        );
    }

    // -- release ---------------------------------------------------------

    #[test]
    fn release_removes_positional() {
        let reg = LockRegistry::new();
        let id = reg
            .try_acquire_range((1, 42), 0, 100, LockMode::Write, holder(1), deploy(1))
            .unwrap();
        reg.release(id).unwrap();
        // Overlapping write now succeeds.
        reg.try_acquire_range((1, 42), 50, 100, LockMode::Write, holder(2), deploy(1))
            .expect("released — next acquire must succeed");
    }

    #[test]
    fn release_removes_sequential() {
        let reg = LockRegistry::new();
        let id = reg
            .try_acquire_sequential((1, 42), holder(1), deploy(1))
            .unwrap();
        reg.release(id).unwrap();
        reg.try_acquire_sequential((1, 42), holder(2), deploy(1))
            .expect("released — next sequential must succeed");
    }

    #[test]
    fn release_twice_returns_closed() {
        let reg = LockRegistry::new();
        let id = reg
            .try_acquire_range((1, 42), 0, 100, LockMode::Read, holder(1), deploy(1))
            .unwrap();
        reg.release(id).unwrap();
        assert_eq!(reg.release(id).unwrap_err(), LockError::Closed);
    }

    #[test]
    fn release_unknown_id_returns_closed() {
        let reg = LockRegistry::new();
        assert_eq!(reg.release(LockId(9999)).unwrap_err(), LockError::Closed);
    }

    #[test]
    fn release_evicts_empty_dev_inode_entry() {
        let reg = LockRegistry::new();
        let id = reg
            .try_acquire_range((1, 42), 0, 100, LockMode::Read, holder(1), deploy(1))
            .unwrap();
        assert_eq!(reg.tracked_files(), 1);
        reg.release(id).unwrap();
        assert_eq!(
            reg.tracked_files(),
            0,
            "empty state must evict — inode-reuse safety"
        );
    }

    #[test]
    fn release_does_not_evict_if_other_locks_present() {
        let reg = LockRegistry::new();
        let a = reg
            .try_acquire_range((1, 42), 0, 100, LockMode::Read, holder(1), deploy(1))
            .unwrap();
        reg.try_acquire_range((1, 42), 200, 100, LockMode::Read, holder(2), deploy(1))
            .unwrap();
        reg.release(a).unwrap();
        assert_eq!(reg.tracked_files(), 1);
    }

    // -- release_all_for_holder ------------------------------------------

    #[test]
    fn release_all_for_holder_sweeps_positional_and_sequential() {
        let reg = LockRegistry::new();
        reg.try_acquire_range((1, 42), 0, 100, LockMode::Read, holder(1), deploy(1))
            .unwrap();
        reg.try_acquire_range((1, 42), 200, 100, LockMode::Read, holder(1), deploy(1))
            .unwrap();
        reg.try_acquire_sequential((1, 43), holder(1), deploy(1))
            .unwrap();
        // Different holder — should survive the sweep.
        reg.try_acquire_range((1, 42), 400, 100, LockMode::Read, holder(2), deploy(1))
            .unwrap();
        let released = reg.release_all_for_holder(&holder(1));
        assert_eq!(released, 3);
        assert_eq!(reg.held_locks(), 1, "other holder's lock survives");
    }

    #[test]
    fn release_all_for_holder_evicts_now_empty_entries() {
        let reg = LockRegistry::new();
        reg.try_acquire_sequential((1, 42), holder(1), deploy(1))
            .unwrap();
        assert_eq!(reg.tracked_files(), 1);
        reg.release_all_for_holder(&holder(1));
        assert_eq!(reg.tracked_files(), 0);
    }

    /// Fold-in edge test from Prep A step-4 review (2026-08-12,
    /// finalized 2026-08-13): verify the sweep count is exact when
    /// same-holder OVERLAPPING acquires are present.  Under Prep A's
    /// same-holder-skip rule, a cap can acquire multiple overlapping
    /// range locks with the same holder — each mints a fresh
    /// RangeEntry.  Sweep must count them ALL, not just one.  A
    /// regression that stopped scanning at the first same-holder
    /// match would return an undercount + strand entries.  The
    /// pre-existing `release_all_for_holder_sweeps_positional_and_
    /// sequential` test at line 853 exercises non-overlapping
    /// same-holder ranges; this adds the overlapping case.
    #[test]
    fn release_all_for_holder_sweeps_all_same_holder_overlapping_ranges() {
        let reg = LockRegistry::new();
        // 3 overlapping same-holder acquires (Prep A rule permits).
        reg.try_acquire_range((1, 42), 0, 1024, LockMode::Write, holder(1), deploy(1))
            .unwrap();
        reg.try_acquire_range((1, 42), 100, 100, LockMode::Write, holder(1), deploy(1))
            .unwrap();
        reg.try_acquire_range((1, 42), 500, 200, LockMode::Read, holder(1), deploy(1))
            .unwrap();
        // Non-overlapping different-holder acquire — must survive sweep.
        reg.try_acquire_range((1, 42), 2000, 100, LockMode::Read, holder(2), deploy(1))
            .unwrap();
        assert_eq!(reg.held_locks(), 4);
        let released = reg.release_all_for_holder(&holder(1));
        assert_eq!(
            released, 3,
            "sweep MUST return exact count of same-holder entries, \
             including OVERLAPPING acquires from the same-holder-skip rule"
        );
        assert_eq!(
            reg.held_locks(),
            1,
            "different-holder lock must survive the sweep"
        );
    }

    // -- release_all_for_deploy ------------------------------------------

    #[test]
    fn release_all_for_deploy_sweeps_across_dev_inodes() {
        let reg = LockRegistry::new();
        reg.try_acquire_range((1, 42), 0, 100, LockMode::Read, holder(1), deploy(1))
            .unwrap();
        reg.try_acquire_range((1, 43), 0, 100, LockMode::Read, holder(2), deploy(1))
            .unwrap();
        reg.try_acquire_range((1, 44), 0, 100, LockMode::Read, holder(3), deploy(2))
            .unwrap();
        let released = reg.release_all_for_deploy(&deploy(1));
        assert_eq!(released, 2);
        assert_eq!(reg.held_locks(), 1, "deploy(2)'s lock survives");
    }

    // -- is_locked --------------------------------------------------------

    #[test]
    fn is_locked_reports_overlapping_range() {
        let reg = LockRegistry::new();
        reg.try_acquire_range((1, 42), 500, 100, LockMode::Read, holder(1), deploy(1))
            .unwrap();
        assert!(reg.is_locked((1, 42), (550, 10)));
        assert!(!reg.is_locked((1, 42), (0, 100)));
    }

    #[test]
    fn is_locked_reports_sequential_as_true_for_any_query() {
        let reg = LockRegistry::new();
        reg.try_acquire_sequential((1, 42), holder(1), deploy(1))
            .unwrap();
        assert!(reg.is_locked((1, 42), (0, 1)));
        assert!(reg.is_locked((1, 42), (u64::MAX - 1, 1)));
    }

    #[test]
    fn is_locked_returns_false_on_untracked_inode() {
        let reg = LockRegistry::new();
        assert!(!reg.is_locked((1, 42), (0, u64::MAX)));
    }

    #[test]
    fn is_locked_whole_file_query_uses_sentinel_range() {
        let reg = LockRegistry::new();
        reg.try_acquire_range((1, 42), 500, 100, LockMode::Read, holder(1), deploy(1))
            .unwrap();
        // Simulates fs_remove_file's whole-file check.
        assert!(reg.is_locked((1, 42), (0, u64::MAX)));
    }

    // -- count_locks (step 6 spec-message follow-up) -----------------------

    /// Step 6 review Gap 3: LockRegistry::count_locks(dev_inode) returns
    /// the exact holder-count that fs_remove_file / fs_remove_dir splice
    /// into the Oracular "{N} holder(s)" log-warn.
    #[test]
    fn count_locks_returns_zero_for_untracked_inode() {
        let reg = LockRegistry::new();
        assert_eq!(reg.count_locks((1, 42)), 0);
    }

    #[test]
    fn count_locks_sums_ranges_and_sequential() {
        let reg = LockRegistry::new();
        // 3 range acquires (some same-holder overlapping under Prep A).
        reg.try_acquire_range((1, 42), 0, 100, LockMode::Write, holder(1), deploy(1))
            .unwrap();
        reg.try_acquire_range((1, 42), 50, 20, LockMode::Write, holder(1), deploy(1))
            .unwrap();
        reg.try_acquire_range((1, 42), 200, 50, LockMode::Read, holder(2), deploy(1))
            .unwrap();
        assert_eq!(reg.count_locks((1, 42)), 3);
        // Different inode: independent.
        assert_eq!(reg.count_locks((1, 43)), 0);
        // Sequential on OTHER inode.
        reg.try_acquire_sequential((1, 43), holder(3), deploy(1))
            .unwrap();
        assert_eq!(reg.count_locks((1, 43)), 1);
    }

    #[test]
    fn count_locks_decreases_after_release() {
        let reg = LockRegistry::new();
        let a = reg
            .try_acquire_range((1, 42), 0, 100, LockMode::Read, holder(1), deploy(1))
            .unwrap();
        let b = reg
            .try_acquire_range((1, 42), 200, 100, LockMode::Read, holder(2), deploy(1))
            .unwrap();
        assert_eq!(reg.count_locks((1, 42)), 2);
        reg.release(a).unwrap();
        assert_eq!(reg.count_locks((1, 42)), 1);
        reg.release(b).unwrap();
        // Entry evicted when last lock released → count is 0.
        assert_eq!(reg.count_locks((1, 42)), 0);
    }

    // -- LockId monotonicity ---------------------------------------------

    #[test]
    fn lock_ids_are_unique_and_monotone_within_a_registry() {
        let reg = LockRegistry::new();
        let a = reg
            .try_acquire_range((1, 42), 0, 10, LockMode::Read, holder(1), deploy(1))
            .unwrap();
        let b = reg
            .try_acquire_range((1, 42), 20, 10, LockMode::Read, holder(1), deploy(1))
            .unwrap();
        let c = reg
            .try_acquire_sequential((1, 43), holder(1), deploy(1))
            .unwrap();
        assert!(a.0 < b.0);
        assert!(b.0 < c.0);
    }

    #[test]
    fn lock_id_zero_is_never_minted() {
        // Sentinel-friendly: LockId(0) reserved as a potential Option<>-less
        // "no lock" marker at future call sites.
        let reg = LockRegistry::new();
        for _ in 0..10 {
            let id = reg
                .try_acquire_range((1, 42), 0, 1, LockMode::Read, holder(1), deploy(1))
                .unwrap();
            assert_ne!(id.0, 0);
            reg.release(id).unwrap();
        }
    }

    // -- broadcast Arc semantics -----------------------------------------

    #[test]
    fn clone_shares_state() {
        // Verifies the broadcast pattern: a runtime holding a cloned
        // LockRegistry sees the same locks as its parent.
        let a = LockRegistry::new();
        let b = a.clone();
        let id = a
            .try_acquire_range((1, 42), 0, 100, LockMode::Read, holder(1), deploy(1))
            .unwrap();
        assert!(
            b.is_locked((1, 42), (0, 100)),
            "cloned handle sees parent's lock"
        );
        b.release(id).unwrap();
        assert!(
            !a.is_locked((1, 42), (0, 100)),
            "release via clone visible to parent"
        );
    }

    #[test]
    fn clone_observes_concurrent_sweep() {
        // Sweep via one handle is visible through another — matches
        // deploy-end auto-release semantics under the broadcast pattern.
        let a = LockRegistry::new();
        let b = a.clone();
        a.try_acquire_range((1, 42), 0, 100, LockMode::Read, holder(1), deploy(1))
            .unwrap();
        a.try_acquire_sequential((1, 43), holder(1), deploy(1))
            .unwrap();
        assert_eq!(b.held_locks(), 2);
        let released = b.release_all_for_deploy(&deploy(1));
        assert_eq!(released, 2);
        assert_eq!(a.held_locks(), 0);
        assert_eq!(a.tracked_files(), 0);
    }

    // -- zero-length and boundary validation -----------------------------

    #[test]
    fn zero_length_range_rejected_as_bad_arg() {
        let reg = LockRegistry::new();
        let err = reg
            .try_acquire_range((1, 42), 100, 0, LockMode::Read, holder(1), deploy(1))
            .unwrap_err();
        assert_eq!(err, LockError::BadArg);
        // And no state was mutated (the (dev, inode) entry was not
        // added).
        assert_eq!(reg.tracked_files(), 0);
    }

    #[test]
    fn zero_length_is_locked_query_returns_false() {
        // Symmetric on the read path — an empty range can't possibly
        // be locked, and the query must not panic.
        let reg = LockRegistry::new();
        reg.try_acquire_range((1, 42), 0, 100, LockMode::Read, holder(1), deploy(1))
            .unwrap();
        assert!(!reg.is_locked((1, 42), (50, 0)));
    }

    #[test]
    fn u64_max_offset_saturates_without_panic() {
        // Range whose end saturates to u64::MAX.  Should not panic;
        // should be treated as extending to the end-of-space.
        let reg = LockRegistry::new();
        let id = reg
            .try_acquire_range(
                (1, 42),
                u64::MAX - 100,
                200,
                LockMode::Write,
                holder(1),
                deploy(1),
            )
            .unwrap();
        // A whole-file query still overlaps with a saturated range.
        assert!(reg.is_locked((1, 42), (0, u64::MAX)));
        reg.release(id).unwrap();
    }

    // -- per-file range cap ---------------------------------------------

    #[test]
    fn max_ranges_per_file_cap_enforced() {
        let reg = LockRegistry::new();
        for i in 0..MAX_RANGES_PER_FILE {
            reg.try_acquire_range(
                (1, 42),
                (i as u64) * 10,
                1,
                LockMode::Read,
                holder(1),
                deploy(1),
            )
            .unwrap();
        }
        let err = reg
            .try_acquire_range(
                (1, 42),
                (MAX_RANGES_PER_FILE as u64) * 10,
                1,
                LockMode::Read,
                holder(1),
                deploy(1),
            )
            .unwrap_err();
        assert_eq!(err, LockError::QuotaExceeded);
    }

    #[test]
    fn cap_recovers_after_release() {
        let reg = LockRegistry::new();
        let mut ids = Vec::with_capacity(MAX_RANGES_PER_FILE);
        for i in 0..MAX_RANGES_PER_FILE {
            ids.push(
                reg.try_acquire_range(
                    (1, 42),
                    (i as u64) * 10,
                    1,
                    LockMode::Read,
                    holder(1),
                    deploy(1),
                )
                .unwrap(),
            );
        }
        // At cap now — release one and re-acquire.
        reg.release(ids[0]).unwrap();
        reg.try_acquire_range(
            (1, 42),
            (MAX_RANGES_PER_FILE as u64) * 10,
            1,
            LockMode::Read,
            holder(1),
            deploy(1),
        )
        .expect("released slot must accept a new acquire");
    }

    #[test]
    fn cap_is_per_dev_inode_not_global() {
        // MAX_RANGES_PER_FILE on inode A doesn't limit inode B.
        let reg = LockRegistry::new();
        for i in 0..MAX_RANGES_PER_FILE {
            reg.try_acquire_range(
                (1, 42),
                (i as u64) * 10,
                1,
                LockMode::Read,
                holder(1),
                deploy(1),
            )
            .unwrap();
        }
        // Different inode — should still admit.
        reg.try_acquire_range((1, 43), 0, 1, LockMode::Read, holder(1), deploy(1))
            .expect("cap is per-(dev, inode), not global");
    }

    // -- NB-3 (2026-09-02): MAX_WAITERS_PER_FILE cap --------------------
    //
    // Bounds the parked-waiter deque against a hostile deploy that
    // spams wait:true on a locked file.  Hard-fork surface — see
    // MAX_WAITERS_PER_FILE docstring for the memory-bound math +
    // rationale.  Same FSERR_QUOTA_EXCEEDED code as the live-range
    // cap so callers do not need to differentiate.

    /// range-wait path: 1024 parked waiters, the 1025th trips
    /// QuotaExceeded before allocating a Waiter struct.
    #[test]
    fn max_waiters_per_file_cap_enforced_on_range_wait() {
        let reg = LockRegistry::new();
        // Acquire the whole file so every subsequent wait:true
        // request parks.
        reg.try_acquire_range((1, 42), 0, u64::MAX, LockMode::Write, holder(1), deploy(1))
            .expect("initial full-file write lock must succeed");
        // Fill the waiter deque to capacity.  Each waiter uses a
        // distinct holder to avoid same-holder skip-rule shortcuts.
        for _ in 0..MAX_WAITERS_PER_FILE {
            expect_parked(
                reg.try_acquire_range_wait(
                    (1, 42),
                    0,
                    100,
                    LockMode::Read,
                    holder(2),
                    deploy(1),
                    WaitPolicy::Wait,
                )
                .expect("wait park up to cap must succeed"),
            );
        }
        assert_eq!(reg.parked_waiters(), MAX_WAITERS_PER_FILE);
        // The next wait:true request must be rejected before allocating
        // another Waiter struct.
        let err = reg
            .try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Read,
                holder(0),
                deploy(1),
                WaitPolicy::Wait,
            )
            .expect_err("range-wait over cap must return QuotaExceeded");
        assert_eq!(err, LockError::QuotaExceeded);
        // Deque length unchanged — proves the cap fired BEFORE the
        // Waiter got pushed.
        assert_eq!(reg.parked_waiters(), MAX_WAITERS_PER_FILE);
    }

    /// sequential-wait path: symmetric with the range-wait test.  Both
    /// acquire paths share the same `state.waiters` deque, so the cap
    /// applies uniformly.
    #[test]
    fn max_waiters_per_file_cap_enforced_on_sequential_wait() {
        let reg = LockRegistry::new();
        // Any lock on the file makes sequential-conflicts true.
        reg.try_acquire_range((1, 42), 0, 100, LockMode::Read, holder(1), deploy(1))
            .expect("initial range lock must succeed");
        for _ in 0..MAX_WAITERS_PER_FILE {
            expect_parked(
                reg.try_acquire_sequential_wait((1, 42), holder(2), deploy(1), WaitPolicy::Wait)
                    .expect("sequential wait park up to cap must succeed"),
            );
        }
        assert_eq!(reg.parked_waiters(), MAX_WAITERS_PER_FILE);
        let err = reg
            .try_acquire_sequential_wait((1, 42), holder(0), deploy(1), WaitPolicy::Wait)
            .expect_err("sequential-wait over cap must return QuotaExceeded");
        assert_eq!(err, LockError::QuotaExceeded);
        assert_eq!(reg.parked_waiters(), MAX_WAITERS_PER_FILE);
    }

    /// The waiter cap is per-`(dev, inode)`, not global: filling one
    /// file's deque doesn't block wait:true on a different inode.
    #[test]
    fn waiter_cap_is_per_dev_inode_not_global() {
        let reg = LockRegistry::new();
        reg.try_acquire_range((1, 42), 0, u64::MAX, LockMode::Write, holder(1), deploy(1))
            .expect("inode A initial lock");
        for _ in 0..MAX_WAITERS_PER_FILE {
            expect_parked(
                reg.try_acquire_range_wait(
                    (1, 42),
                    0,
                    100,
                    LockMode::Read,
                    holder(2),
                    deploy(1),
                    WaitPolicy::Wait,
                )
                .unwrap(),
            );
        }
        // Different inode — still admits an immediate acquire (no
        // waiter needed) even though inode A's deque is full.
        reg.try_acquire_range((1, 43), 0, 100, LockMode::Read, holder(1), deploy(1))
            .expect("waiter cap is per-(dev, inode), not global");
    }

    /// After sweeping the deploy, the deque frees up and subsequent
    /// wait:true acquires can park again.  Verifies the cap recovers
    /// symmetrically with the sweep behavior.
    #[test]
    fn waiter_cap_recovers_after_deploy_sweep() {
        let reg = LockRegistry::new();
        reg.try_acquire_range((1, 42), 0, u64::MAX, LockMode::Write, holder(1), deploy(1))
            .expect("initial lock");
        for _ in 0..MAX_WAITERS_PER_FILE {
            expect_parked(
                reg.try_acquire_range_wait(
                    (1, 42),
                    0,
                    100,
                    LockMode::Read,
                    holder(2),
                    deploy(2),
                    WaitPolicy::Wait,
                )
                .unwrap(),
            );
        }
        // Cap reached — verify the guard fires.
        assert_eq!(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Read,
                holder(0),
                deploy(2),
                WaitPolicy::Wait,
            )
            .expect_err("must be QuotaExceeded"),
            LockError::QuotaExceeded
        );
        // Sweep the waiters' deploy — cap recovers.
        let n_cancelled = reg.cancel_all_waiters_for_deploy(&deploy(2));
        assert_eq!(n_cancelled, MAX_WAITERS_PER_FILE);
        assert_eq!(reg.parked_waiters(), 0);
        // New wait:true acquires can park again.
        expect_parked(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Read,
                holder(0),
                deploy(3),
                WaitPolicy::Wait,
            )
            .expect("wait must park cleanly after sweep"),
        );
    }

    // -- no-op sweeps ---------------------------------------------------

    #[test]
    fn release_all_for_holder_unknown_returns_zero() {
        let reg = LockRegistry::new();
        reg.try_acquire_range((1, 42), 0, 100, LockMode::Read, holder(1), deploy(1))
            .unwrap();
        // Unknown holder — no-op sweep, existing lock survives.
        let released = reg.release_all_for_holder(&holder(99));
        assert_eq!(released, 0);
        assert_eq!(reg.held_locks(), 1);
    }

    #[test]
    fn release_all_for_deploy_unknown_returns_zero() {
        let reg = LockRegistry::new();
        reg.try_acquire_range((1, 42), 0, 100, LockMode::Read, holder(1), deploy(1))
            .unwrap();
        let released = reg.release_all_for_deploy(&deploy(99));
        assert_eq!(released, 0);
        assert_eq!(reg.held_locks(), 1);
    }

    #[test]
    #[should_panic(expected = "release_all_for_deploy called with the [0; 32] sentinel")]
    fn release_all_for_deploy_zero_sentinel_panics() {
        // Sentinel guard: sweeping on `[0; 32]` would release every
        // sentinel-scoped entry in the registry.  Under step-5 normal
        // operation there should be none (every acquire under a live
        // WalDeployScope records the real deploy scope via
        // FileHandleTable::current_deploy_scope), but a caller
        // invoking release_all_for_deploy(&[0; 32]) directly — outside
        // a WalDeployScope guard — would sweep stray sentinel entries.
        //
        // Guard promoted from debug_assert to assert during step-5
        // review (2026-08-13) so the panic fires in release builds
        // too; test renamed from `_panics_in_debug` accordingly.
        let reg = LockRegistry::new();
        reg.try_acquire_range((1, 42), 0, 100, LockMode::Read, holder(1), [0u8; 32])
            .unwrap();
        reg.release_all_for_deploy(&[0u8; 32]);
    }

    #[test]
    fn release_all_for_deploy_targets_by_deploy_not_holder() {
        // Under D3 a File cap outlives its originating deploy, so one
        // HolderId can hold locks in multiple deploys.  Sweeping by
        // deploy must not disturb the same holder's locks in a
        // different deploy.
        let reg = LockRegistry::new();
        reg.try_acquire_range((1, 42), 0, 10, LockMode::Read, holder(1), deploy(1))
            .unwrap();
        reg.try_acquire_range((1, 42), 20, 10, LockMode::Read, holder(1), deploy(2))
            .unwrap();
        assert_eq!(reg.held_locks(), 2);
        reg.release_all_for_deploy(&deploy(1));
        assert_eq!(
            reg.held_locks(),
            1,
            "holder(1)'s deploy(2) lock must survive"
        );
    }

    // -- consensus-committed constant pins (hard-fork surface) -----------

    #[test]
    fn max_ranges_per_file_pinned_at_1024() {
        // Catalog item #12 in snapshot.rs's hard-fork surface docstring.
        // Consensus-observable: governs when fs_lock_range returns
        // FSERR_QUOTA_EXCEEDED.  A drift here diverges validator replies.
        assert_eq!(MAX_RANGES_PER_FILE, 1024);
    }

    #[test]
    fn lock_id_ceiling_pinned() {
        // Catalog item #12.  LockId values themselves are per-runtime
        // and not consensus-observable, but the QuotaExceeded threshold
        // IS.  Pinned at u64::MAX - 2^16 = 18446744073709486079.
        assert_eq!(LOCK_ID_CEILING, u64::MAX - (1 << 16));
        assert_eq!(LOCK_ID_CEILING, 18_446_744_073_709_486_079);
    }

    // -- ceiling trigger -------------------------------------------------

    #[test]
    fn ceiling_returns_quota_exceeded() {
        let reg = LockRegistry::new();
        // Seed the counter right at the ceiling so the next
        // fetch_add returns ceiling.
        reg.set_next_lock_id_for_testing(LOCK_ID_CEILING);
        let err = reg
            .try_acquire_range((1, 42), 0, 10, LockMode::Read, holder(1), deploy(1))
            .unwrap_err();
        assert_eq!(err, LockError::QuotaExceeded);
        // Also: sequential acquire fails through the same ceiling.
        let err2 = reg
            .try_acquire_sequential((1, 43), holder(1), deploy(1))
            .unwrap_err();
        assert_eq!(err2, LockError::QuotaExceeded);
    }

    #[test]
    fn ceiling_holds_across_concurrent_racers() {
        // Simulates two concurrent racers past the ceiling: both must
        // receive QuotaExceeded; neither must receive an Ok with a
        // wrapped LockId.  The store-on-overflow in mint_id is
        // idempotent across racers.
        let reg = LockRegistry::new();
        // Seed slightly under the ceiling so the first fetch_add
        // succeeds; the next two step past.
        reg.set_next_lock_id_for_testing(LOCK_ID_CEILING - 1);
        let ok = reg
            .try_acquire_range((1, 42), 0, 10, LockMode::Read, holder(1), deploy(1))
            .expect("last slot must admit");
        assert_eq!(ok.0, LOCK_ID_CEILING);
        // Every subsequent acquire fails.
        for _ in 0..5 {
            let err = reg
                .try_acquire_range((1, 43), 0, 10, LockMode::Read, holder(1), deploy(1))
                .unwrap_err();
            assert_eq!(err, LockError::QuotaExceeded);
        }
    }

    #[test]
    fn just_below_ceiling_admits_normally() {
        // Regression: LOCK_ID_CEILING itself is a valid returned
        // LockId (fetch_add at CEILING-1 → LockId(CEILING)).  The
        // NEXT acquire is what fails.
        let reg = LockRegistry::new();
        reg.set_next_lock_id_for_testing(LOCK_ID_CEILING - 1);
        let id = reg
            .try_acquire_range((1, 42), 0, 10, LockMode::Read, holder(1), deploy(1))
            .unwrap();
        assert_eq!(id.0, LOCK_ID_CEILING);
    }

    // -- cap-full sequential coexistence ---------------------------------

    #[test]
    fn cap_full_positional_still_blocks_sequential() {
        // Defensive against a future refactor that reorders the
        // sequential-holder check vs. the cap check inside
        // try_acquire_range / try_acquire_sequential.  Even with the
        // range table saturated, a sequential acquire must return Busy
        // (positional-blocks-sequential), NOT QuotaExceeded — the
        // coexistence rule wins over the cap.
        let reg = LockRegistry::new();
        for i in 0..MAX_RANGES_PER_FILE {
            reg.try_acquire_range(
                (1, 42),
                (i as u64) * 10,
                1,
                LockMode::Read,
                holder(1),
                deploy(1),
            )
            .unwrap();
        }
        let err = reg
            .try_acquire_sequential((1, 42), holder(2), deploy(1))
            .unwrap_err();
        assert_eq!(err, LockError::Busy);
    }

    // -- slice 8b: wait-queue infrastructure ------------------------------
    //
    // Tests for `WaitPolicy::Wait`, the FIFO waiter queue, head-of-line
    // admission, `cancel_wait`, and `cancel_all_waiters_for_deploy`.
    // See plan §X-2 Slice 8b concrete implementation steps §1-3.

    /// Helper: assert the AcquireOutcome is Immediate and unwrap its id.
    fn expect_immediate(o: AcquireOutcome) -> LockId {
        match o {
            AcquireOutcome::Immediate(id) => id,
            AcquireOutcome::Parked { .. } => panic!("expected Immediate, got Parked"),
        }
    }

    /// Helper: assert the AcquireOutcome is Parked and unwrap its (id, rx).
    fn expect_parked(o: AcquireOutcome) -> (LockId, oneshot::Receiver<Result<LockId, LockError>>) {
        match o {
            AcquireOutcome::Parked { lock_id, admit } => (lock_id, admit),
            AcquireOutcome::Immediate(_) => panic!("expected Parked, got Immediate"),
        }
    }

    #[test]
    fn wait_policy_fail_returns_immediate_or_busy_matching_pre_8b_behavior() {
        // Pins that the WaitPolicy::Fail branch reproduces the exact
        // pre-slice-8b semantics.  Regressions to the wait-queue code
        // paths must not affect the fail-fast return shape.
        let reg = LockRegistry::new();
        expect_immediate(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Write,
                holder(1),
                deploy(1),
                WaitPolicy::Fail,
            )
            .unwrap(),
        );
        let err = reg
            .try_acquire_range_wait(
                (1, 42),
                50,
                100,
                LockMode::Write,
                holder(2),
                deploy(1),
                WaitPolicy::Fail,
            )
            .unwrap_err();
        assert_eq!(err, LockError::Busy);
        assert_eq!(reg.parked_waiters(), 0);
    }

    #[tokio::test]
    async fn waiter_admitted_after_conflicting_holder_releases() {
        // A → W(0, 100), B parks on W(0, 100) with Wait; release A → B admits.
        let reg = LockRegistry::new();
        let a = expect_immediate(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Write,
                holder(1),
                deploy(1),
                WaitPolicy::Fail,
            )
            .unwrap(),
        );
        let (b_id, b_rx) = expect_parked(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Write,
                holder(2),
                deploy(2),
                WaitPolicy::Wait,
            )
            .unwrap(),
        );
        assert_eq!(reg.parked_waiters(), 1);
        assert_eq!(reg.held_locks(), 1); // only A is held
        reg.release(a).unwrap();
        // B's admit should now fire with Ok(b_id).
        let admitted = b_rx.await.expect("admit sender must not drop");
        assert_eq!(admitted, Ok(b_id));
        assert_eq!(reg.parked_waiters(), 0);
        assert_eq!(reg.held_locks(), 1); // B now holds
    }

    #[tokio::test]
    async fn three_waiters_admit_fifo_after_release() {
        // A holds W(0, 100).  B, C, D each park on W(0, 100) in
        // order.  Release A → B admits; release B → C admits; etc.
        let reg = LockRegistry::new();
        let a = expect_immediate(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Write,
                holder(1),
                deploy(1),
                WaitPolicy::Fail,
            )
            .unwrap(),
        );
        let (b_id, b_rx) = expect_parked(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Write,
                holder(2),
                deploy(2),
                WaitPolicy::Wait,
            )
            .unwrap(),
        );
        let (c_id, c_rx) = expect_parked(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Write,
                holder(3),
                deploy(3),
                WaitPolicy::Wait,
            )
            .unwrap(),
        );
        let (d_id, d_rx) = expect_parked(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Write,
                holder(4),
                deploy(4),
                WaitPolicy::Wait,
            )
            .unwrap(),
        );
        assert_eq!(reg.parked_waiters(), 3);
        assert_eq!(reg.held_locks(), 1);
        // Release A → B admits.
        reg.release(a).unwrap();
        assert_eq!(b_rx.await.unwrap(), Ok(b_id));
        assert_eq!(reg.parked_waiters(), 2);
        // Release B → C admits.
        reg.release(b_id).unwrap();
        assert_eq!(c_rx.await.unwrap(), Ok(c_id));
        assert_eq!(reg.parked_waiters(), 1);
        // Release C → D admits.
        reg.release(c_id).unwrap();
        assert_eq!(d_rx.await.unwrap(), Ok(d_id));
        assert_eq!(reg.parked_waiters(), 0);
        assert_eq!(reg.held_locks(), 1);
    }

    #[tokio::test]
    async fn admit_stops_at_first_non_admissible_head_after_release() {
        // Strict head-of-line: after a release, wake_waiters scans the
        // queue front-first; the first non-admissible head halts the
        // admission wave even if downstream waiters would fit against
        // the current holder set.
        //
        // Scenario: A holds W(0, 500).  Three waiters all conflict at
        // park time — B: W(0, 100), C: W(200, 100), D: W(400, 100).
        // Release A → B admits.  C would fit against just-admitted
        // B (disjoint range), but strict-FIFO doesn't overtake — the
        // implementation admits C in the same wake pass because C's
        // check is against the post-admit state (which now holds B),
        // and C is disjoint from B.
        //
        // NB: strict-FIFO here means "don't overtake a NON-admissible
        // head", not "admit at most one per release" — the wake loop
        // cascades while heads keep being admissible.  This test
        // pins that CASCADE and the wake loop's stop-at-conflict.
        let reg = LockRegistry::new();
        let a = expect_immediate(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                500,
                LockMode::Write,
                holder(1),
                deploy(1),
                WaitPolicy::Fail,
            )
            .unwrap(),
        );
        let (b_id, b_rx) = expect_parked(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Write,
                holder(2),
                deploy(2),
                WaitPolicy::Wait,
            )
            .unwrap(),
        );
        let (c_id, c_rx) = expect_parked(
            reg.try_acquire_range_wait(
                (1, 42),
                200,
                100,
                LockMode::Write,
                holder(3),
                deploy(3),
                WaitPolicy::Wait,
            )
            .unwrap(),
        );
        let (d_id, d_rx) = expect_parked(
            reg.try_acquire_range_wait(
                (1, 42),
                400,
                100,
                LockMode::Write,
                holder(4),
                deploy(4),
                WaitPolicy::Wait,
            )
            .unwrap(),
        );
        // Release A → cascade wake: all three heads are disjoint from
        // each other, so all admit in FIFO order in one wake pass.
        reg.release(a).unwrap();
        assert_eq!(b_rx.await.unwrap(), Ok(b_id));
        assert_eq!(c_rx.await.unwrap(), Ok(c_id));
        assert_eq!(d_rx.await.unwrap(), Ok(d_id));
        assert_eq!(reg.parked_waiters(), 0);
        assert_eq!(reg.held_locks(), 3);
    }

    #[tokio::test]
    async fn admit_wave_stops_at_conflicting_head() {
        // Cascade admission stops as soon as a head can't fit — even
        // if a later waiter WOULD fit against the (now-augmented)
        // holder set.  This is the strict-FIFO anti-starvation
        // property.
        //
        // Scenario: A holds W(0, 100).  B parks W(0, 100), C parks
        // W(50, 100) [conflicts with B's would-be admit], D parks
        // W(400, 100) [disjoint from everything].
        //
        // Release A → B admits.  C is head-of-queue now, conflicts
        // with just-admitted B → wake halts.  D would fit but is
        // BEHIND C in the queue → stays parked.
        let reg = LockRegistry::new();
        let a = expect_immediate(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Write,
                holder(1),
                deploy(1),
                WaitPolicy::Fail,
            )
            .unwrap(),
        );
        let (b_id, b_rx) = expect_parked(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Write,
                holder(2),
                deploy(2),
                WaitPolicy::Wait,
            )
            .unwrap(),
        );
        // Get C to actually park: it has to conflict at park time
        // (A's W(0, 100)).  C's range (50, 100) overlaps A → conflict → park.
        let (_c_id, mut c_rx) = expect_parked(
            reg.try_acquire_range_wait(
                (1, 42),
                50,
                100,
                LockMode::Write,
                holder(3),
                deploy(3),
                WaitPolicy::Wait,
            )
            .unwrap(),
        );
        // D at (400, 100) — DOESN'T conflict with A → would go
        // Immediate under the current impl (fresh acquires that
        // don't conflict with holders don't respect the parked
        // queue).  To simulate the strict-FIFO case, we'd need D to
        // conflict at park time.  Give D a range that overlaps A
        // (so it parks) but doesn't overlap B (so it WOULD admit
        // after B if not for head-of-line).  Not achievable in one
        // range: any range overlapping A's (0, 100) also overlaps
        // B's (0, 100) — they're identical.
        //
        // So this test just pins that the wave stops at C.  D would
        // Immediate-acquire so we skip it.
        reg.release(a).unwrap();
        assert_eq!(b_rx.await.unwrap(), Ok(b_id));
        // C stayed parked because C's (50, 100) conflicts with B's
        // just-admitted (0, 100).
        assert!(matches!(
            c_rx.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        assert_eq!(reg.parked_waiters(), 1);
    }

    #[tokio::test]
    async fn cancel_wait_removes_parked_and_signals_cancelled() {
        let reg = LockRegistry::new();
        let _a = expect_immediate(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Write,
                holder(1),
                deploy(1),
                WaitPolicy::Fail,
            )
            .unwrap(),
        );
        let (b_id, b_rx) = expect_parked(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Write,
                holder(2),
                deploy(2),
                WaitPolicy::Wait,
            )
            .unwrap(),
        );
        assert_eq!(reg.parked_waiters(), 1);
        assert!(reg.cancel_wait(b_id));
        assert_eq!(reg.parked_waiters(), 0);
        let outcome = b_rx.await.expect("sender still lives long enough to send");
        assert_eq!(outcome, Err(LockError::Cancelled));
        // Repeat cancel is a no-op.
        assert!(!reg.cancel_wait(b_id));
    }

    #[tokio::test]
    async fn cancel_wait_on_unknown_id_returns_false() {
        let reg = LockRegistry::new();
        assert!(!reg.cancel_wait(LockId(999)));
    }

    #[tokio::test]
    async fn cancel_wait_does_not_wake_other_waiters() {
        // A holds W.  B and C park.  Cancelling C (behind B) leaves
        // B parked — cancellation of a non-head waiter doesn't free
        // any resource, so head admission is unchanged.
        let reg = LockRegistry::new();
        let _a = expect_immediate(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Write,
                holder(1),
                deploy(1),
                WaitPolicy::Fail,
            )
            .unwrap(),
        );
        let (_b_id, mut b_rx) = expect_parked(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Write,
                holder(2),
                deploy(2),
                WaitPolicy::Wait,
            )
            .unwrap(),
        );
        let (c_id, _c_rx) = expect_parked(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Write,
                holder(3),
                deploy(3),
                WaitPolicy::Wait,
            )
            .unwrap(),
        );
        assert!(reg.cancel_wait(c_id));
        assert!(matches!(
            b_rx.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        assert_eq!(reg.parked_waiters(), 1);
    }

    #[tokio::test]
    async fn cancel_all_waiters_for_deploy_sweeps_matching_only() {
        // A holds under deploy(9).  B parks under deploy(2), C parks
        // under deploy(3), D parks under deploy(2).  Sweep deploy(2)
        // → B and D cancel, C remains.
        let reg = LockRegistry::new();
        let _a = expect_immediate(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Write,
                holder(1),
                deploy(9),
                WaitPolicy::Fail,
            )
            .unwrap(),
        );
        let (_b_id, b_rx) = expect_parked(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Write,
                holder(2),
                deploy(2),
                WaitPolicy::Wait,
            )
            .unwrap(),
        );
        let (_c_id, mut c_rx) = expect_parked(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Write,
                holder(3),
                deploy(3),
                WaitPolicy::Wait,
            )
            .unwrap(),
        );
        let (_d_id, d_rx) = expect_parked(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Write,
                holder(4),
                deploy(2),
                WaitPolicy::Wait,
            )
            .unwrap(),
        );
        assert_eq!(reg.parked_waiters(), 3);
        let cancelled = reg.cancel_all_waiters_for_deploy(&deploy(2));
        assert_eq!(cancelled, 2);
        assert_eq!(reg.parked_waiters(), 1);
        assert_eq!(b_rx.await.unwrap(), Err(LockError::Cancelled));
        assert_eq!(d_rx.await.unwrap(), Err(LockError::Cancelled));
        // C is still parked.
        assert!(matches!(
            c_rx.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
    }

    #[test]
    fn cancel_all_waiters_for_deploy_zero_sentinel_panics() {
        // Mirrors release_all_for_deploy sentinel guard.
        let reg = LockRegistry::new();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            reg.cancel_all_waiters_for_deploy(&[0u8; 32])
        }));
        assert!(result.is_err(), "expected panic on [0; 32] sentinel");
    }

    #[tokio::test]
    async fn release_all_for_holder_wakes_admissible_waiter_of_other_holder() {
        // A (holder 1) holds W(0, 100).  B (holder 2) parks W(0, 100).
        // A closes → release_all_for_holder(1) sweeps A's ranges + wakes
        // B in the same call.
        let reg = LockRegistry::new();
        let _a = expect_immediate(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Write,
                holder(1),
                deploy(1),
                WaitPolicy::Fail,
            )
            .unwrap(),
        );
        let (b_id, b_rx) = expect_parked(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Write,
                holder(2),
                deploy(2),
                WaitPolicy::Wait,
            )
            .unwrap(),
        );
        let released = reg.release_all_for_holder(&holder(1));
        assert_eq!(released, 1);
        assert_eq!(b_rx.await.unwrap(), Ok(b_id));
        assert_eq!(reg.held_locks(), 1);
    }

    #[tokio::test]
    async fn release_all_for_deploy_wakes_admissible_waiter_of_other_deploy() {
        // Same as above but by deploy scope.
        let reg = LockRegistry::new();
        let _a = expect_immediate(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Write,
                holder(1),
                deploy(1),
                WaitPolicy::Fail,
            )
            .unwrap(),
        );
        let (b_id, b_rx) = expect_parked(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Write,
                holder(2),
                deploy(2),
                WaitPolicy::Wait,
            )
            .unwrap(),
        );
        let released = reg.release_all_for_deploy(&deploy(1));
        assert_eq!(released, 1);
        assert_eq!(b_rx.await.unwrap(), Ok(b_id));
    }

    #[tokio::test]
    async fn parked_state_not_evicted_after_conflicting_holder_release() {
        // Regression pin for the state_is_empty predicate: a state
        // with parked waiters (but no held locks after release) must
        // NOT be evicted — dropping the map entry would drop the
        // waiter's admit sender and spuriously signal Cancelled.
        //
        // Scenario: A holds Sequential.  B parks Sequential.  Release
        // A — B is now admissible so it's promoted immediately.  So
        // this scenario doesn't test eviction skip.  Better: cancel_wait
        // removes the last waiter; then state IS evicted.
        //
        // Test eviction-skip via: A holds W.  B parks W.  Cancel B via
        // cancel_wait.  State should be evicted (empty: no ranges, no
        // sequential, no waiters).  Contrast: while B is parked but A
        // is not yet released, state has A held + B parked, obviously
        // not evictable.  The key case: if A releases while B is still
        // parked (via cancel_wait race), we don't strand.
        //
        // Simplest: acquire, cancel-park nothing left → state evicted
        // once cancelled.  This test focuses on the "waiter keeps
        // state alive" invariant while it's parked.
        let reg = LockRegistry::new();
        assert_eq!(reg.tracked_files(), 0);
        let _a = expect_immediate(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Write,
                holder(1),
                deploy(1),
                WaitPolicy::Fail,
            )
            .unwrap(),
        );
        assert_eq!(reg.tracked_files(), 1);
        let (_b_id, _b_rx) = expect_parked(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Write,
                holder(2),
                deploy(2),
                WaitPolicy::Wait,
            )
            .unwrap(),
        );
        assert_eq!(reg.tracked_files(), 1);
        // Release A: state has B parked → B gets admitted → state
        // now holds B alone.  tracked_files still 1.
        reg.release(_a).unwrap();
        assert_eq!(reg.tracked_files(), 1);
        assert_eq!(reg.held_locks(), 1);
    }

    #[tokio::test]
    async fn eviction_after_cancel_removes_last_waiter() {
        let reg = LockRegistry::new();
        let a = expect_immediate(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Write,
                holder(1),
                deploy(1),
                WaitPolicy::Fail,
            )
            .unwrap(),
        );
        let (b_id, _b_rx) = expect_parked(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Write,
                holder(2),
                deploy(2),
                WaitPolicy::Wait,
            )
            .unwrap(),
        );
        // Cancel B first.
        assert!(reg.cancel_wait(b_id));
        assert_eq!(reg.tracked_files(), 1); // A still held
                                            // Now release A.  ranges empty, sequential none, waiters
                                            // empty → evict.
        reg.release(a).unwrap();
        assert_eq!(reg.tracked_files(), 0);
    }

    #[tokio::test]
    async fn dropped_receiver_before_admit_rolls_back_promotion() {
        // If a caller's task is aborted between park and admit, the
        // oneshot receiver is dropped.  When the release path admits
        // that waiter, its `admit.send` returns Err → wake_waiters
        // must roll back the promotion so the range slot returns to
        // the free pool for the NEXT waiter.
        let reg = LockRegistry::new();
        let a = expect_immediate(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Write,
                holder(1),
                deploy(1),
                WaitPolicy::Fail,
            )
            .unwrap(),
        );
        let (_b_id, b_rx) = expect_parked(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Write,
                holder(2),
                deploy(2),
                WaitPolicy::Wait,
            )
            .unwrap(),
        );
        let (c_id, c_rx) = expect_parked(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Write,
                holder(3),
                deploy(3),
                WaitPolicy::Wait,
            )
            .unwrap(),
        );
        // Drop B's receiver (simulate task abort).
        drop(b_rx);
        // Release A.  wake_waiters admits B → send fails → rollback
        // → then admits C.
        reg.release(a).unwrap();
        assert_eq!(c_rx.await.unwrap(), Ok(c_id));
        assert_eq!(reg.held_locks(), 1);
        assert_eq!(reg.parked_waiters(), 0);
    }

    #[tokio::test]
    async fn wait_true_immediate_when_no_conflict() {
        // wait:true on an unconflicted acquire behaves exactly like
        // wait:false — returns Immediate, doesn't park.
        let reg = LockRegistry::new();
        let out = reg
            .try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Write,
                holder(1),
                deploy(1),
                WaitPolicy::Wait,
            )
            .unwrap();
        expect_immediate(out);
        assert_eq!(reg.parked_waiters(), 0);
        assert_eq!(reg.held_locks(), 1);
    }

    #[tokio::test]
    async fn sequential_waiter_admits_after_ranges_drain() {
        // A holds R(0, 100).  B parks Sequential (needs empty state).
        // Release A → B admits.
        let reg = LockRegistry::new();
        let a = expect_immediate(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Read,
                holder(1),
                deploy(1),
                WaitPolicy::Fail,
            )
            .unwrap(),
        );
        let (b_id, b_rx) = expect_parked(
            reg.try_acquire_sequential_wait((1, 42), holder(2), deploy(2), WaitPolicy::Wait)
                .unwrap(),
        );
        reg.release(a).unwrap();
        assert_eq!(b_rx.await.unwrap(), Ok(b_id));
    }

    #[tokio::test]
    async fn same_holder_wait_true_still_uses_same_holder_skip() {
        // Same-holder Prep-A skip rule applies under Wait too — a
        // holder's own overlapping range shouldn't self-park.
        let reg = LockRegistry::new();
        let _a = expect_immediate(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Write,
                holder(1),
                deploy(1),
                WaitPolicy::Fail,
            )
            .unwrap(),
        );
        // Same holder, overlapping — should Immediate (skip rule), not Parked.
        let out = reg
            .try_acquire_range_wait(
                (1, 42),
                50,
                100,
                LockMode::Write,
                holder(1),
                deploy(1),
                WaitPolicy::Wait,
            )
            .unwrap();
        expect_immediate(out);
        assert_eq!(reg.parked_waiters(), 0);
        assert_eq!(reg.held_locks(), 2);
    }

    #[tokio::test]
    async fn waiter_admit_rolls_back_range_if_max_ranges_would_exceed() {
        // Regression pin: if a release fires wake_waiters against a
        // state where ranges is at MAX-1, admission checks
        // len < MAX_RANGES_PER_FILE and won't overflow.  Constructed
        // scenario: fill up to MAX-1 with same-holder non-overlapping
        // ranges under holder A, park a different-holder overlapping
        // waiter, release one of A's → waiter admits (now
        // count == MAX-1 → MAX after admit, fits by 1 slot).
        //
        // This is a smoke check that wake_waiters respects the
        // per-file cap.
        let reg = LockRegistry::new();
        for i in 0..MAX_RANGES_PER_FILE as u64 {
            reg.try_acquire_range_wait(
                (1, 42),
                i * 10,
                5,
                LockMode::Write,
                holder(1),
                deploy(1),
                WaitPolicy::Fail,
            )
            .unwrap();
        }
        // Now cap is hit.  A same-holder wait would skip; use a
        // different holder overlapping first entry.
        let out = reg.try_acquire_range_wait(
            (1, 42),
            0,
            5,
            LockMode::Write,
            holder(2),
            deploy(2),
            WaitPolicy::Wait,
        );
        // Cap is full — even wait:true is QuotaExceeded (no slot).
        assert_eq!(out.err(), Some(LockError::QuotaExceeded));
    }

    // -- sub-6 review-fix: cancel_all_waiters_for_holder -----------------

    #[tokio::test]
    async fn cancel_all_waiters_for_holder_sweeps_matching_only() {
        // Sub-6 review-fix B2: File.close's fs_release_all_for_holder
        // needs a symmetrical waiter-sweep so parked wait:true acquires
        // by the closed cap resolve deterministically (Cancelled)
        // instead of leaking in the queue with a stale HolderId.
        let reg = LockRegistry::new();
        // Pre-holder to force parking.
        let pre = deploy(0xEE);
        reg.try_acquire_range((1, 42), 0, 100, LockMode::Write, holder(9), pre)
            .expect("pre acquire");
        // Holder A parks 2 waiters, holder B parks 1.
        let (_a1, a1_rx) = expect_parked(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Write,
                holder(1),
                deploy(1),
                WaitPolicy::Wait,
            )
            .unwrap(),
        );
        let (_a2, a2_rx) = expect_parked(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Write,
                holder(1),
                deploy(1),
                WaitPolicy::Wait,
            )
            .unwrap(),
        );
        let (_b1, mut b1_rx) = expect_parked(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Write,
                holder(2),
                deploy(2),
                WaitPolicy::Wait,
            )
            .unwrap(),
        );
        assert_eq!(reg.parked_waiters(), 3);
        let cancelled = reg.cancel_all_waiters_for_holder(&holder(1));
        assert_eq!(cancelled, 2);
        assert_eq!(reg.parked_waiters(), 1);
        assert_eq!(a1_rx.await.unwrap(), Err(LockError::Cancelled));
        assert_eq!(a2_rx.await.unwrap(), Err(LockError::Cancelled));
        // B's waiter still parked.
        assert!(matches!(
            b1_rx.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test]
    async fn cancel_all_waiters_for_holder_zero_returns_no_matches_gracefully() {
        // Called on a holder with no parked waiters — no-op, returns 0.
        let reg = LockRegistry::new();
        assert_eq!(reg.cancel_all_waiters_for_holder(&holder(99)), 0);
    }

    /// **Sub-6 review round-2 regression pin (BL-1)**:
    /// fs_release_all_for_holder MUST cancel-first-release-second —
    /// same ordering as WalDeployScope::drop (B1 fix).  Concrete
    /// failure mode this test exercises: same holder holds sequential
    /// AND parks wait:true range.  release_all_for_holder first would
    /// wake the range waiter (sequential is gone → sequential_conflicts
    /// false; ranges empty → range_conflicts false); waiter admits →
    /// held range tied to a now-closed cap → leaks.
    ///
    /// The registry-level primitives support BOTH orderings; this
    /// test uses the primitives DIRECTLY in the correct order to
    /// simulate the fixed native.  The handler-level fix is at
    /// handlers.rs:fs_release_all_for_holder; the source-scan pin
    /// there catches regressions to release-first.
    #[tokio::test]
    async fn cancel_then_release_avoids_same_holder_cross_kind_admission_leak() {
        let reg = LockRegistry::new();
        // Same holder acquires sequential AND parks wait:true range.
        reg.try_acquire_sequential((1, 42), holder(1), deploy(1))
            .expect("sequential acquire");
        let (_wait_id, wait_rx) = expect_parked(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Read,
                holder(1),
                deploy(1),
                WaitPolicy::Wait,
            )
            .unwrap(),
        );
        assert_eq!(reg.held_locks(), 1);
        assert_eq!(reg.parked_waiters(), 1);
        // Cancel-first-release-second (matches the fixed native).
        let cancelled = reg.cancel_all_waiters_for_holder(&holder(1));
        let released = reg.release_all_for_holder(&holder(1));
        assert_eq!(cancelled, 1);
        assert_eq!(released, 1);
        // Waiter got Cancelled (not admitted).
        assert_eq!(wait_rx.await.unwrap(), Err(LockError::Cancelled));
        // Registry is empty — no leaked range entry.
        assert_eq!(
            reg.held_locks(),
            0,
            "sub-6 round-2 BL-1 regression: same-holder cross-kind \
             wait must be cancelled, not admitted-then-leaked"
        );
        assert_eq!(reg.parked_waiters(), 0);
        assert_eq!(reg.tracked_files(), 0);
    }

    /// Companion: demonstrate the PRE-FIX bug when using release-first
    /// order — the waiter IS admitted and leaks.  Documents the bug
    /// shape unambiguously so a future refactor that reverses to
    /// release-first is identifiable via test output.
    #[tokio::test]
    async fn release_first_admits_same_holder_waiter_documenting_bug_shape() {
        let reg = LockRegistry::new();
        reg.try_acquire_sequential((1, 42), holder(1), deploy(1))
            .expect("sequential acquire");
        let (wait_id, wait_rx) = expect_parked(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Read,
                holder(1),
                deploy(1),
                WaitPolicy::Wait,
            )
            .unwrap(),
        );
        // Release-first (BUG order): wake_waiters sees empty state,
        // admits waiter into held range.
        let released = reg.release_all_for_holder(&holder(1));
        let cancelled = reg.cancel_all_waiters_for_holder(&holder(1));
        assert_eq!(released, 1);
        // Cancel found nothing — waiter was already admitted.
        assert_eq!(cancelled, 0);
        // The waiter's admit oneshot received Ok — it thinks it holds.
        assert_eq!(wait_rx.await.unwrap(), Ok(wait_id));
        // AND the registry has a live range entry.  If this were the
        // real close() path, the entry would leak until deploy-end.
        assert_eq!(reg.held_locks(), 1);
    }

    /// **Phase 8 review follow-up N4 (2026-08-26).**  Registry drop
    /// must surface `Err(LockError::Cancelled)` (or the underlying
    /// `RecvError` mapped to Cancelled by callers) to every parked
    /// waiter.  Pins the behavior documented in:
    ///   * `LockRegistry` doc lines 47-50 ("signals
    ///     `Err(LockError::Cancelled)` through the oneshot")
    ///   * `Waiter.admit` doc line 308-311 ("Dropping the sender
    ///     (registry drop / waiter removal without signal) surfaces
    ///     to `ParkedHandle.wait` as `Cancelled`")
    ///
    /// The mechanism is Rust's `Sender::drop` → `Receiver::recv`
    /// returns `Err(RecvError)`.  `LockRegistry` has no explicit
    /// `impl Drop`; the guarantee comes from the `Arc<RwLock<...>>`
    /// dropping its inner HashMap, which drops every `Waiter`,
    /// which drops the `admit: oneshot::Sender`.  A regression that
    /// adds a `Drop` for LockRegistry that swaps out the inner
    /// state (e.g., `mem::take` into a static) without also draining
    /// waiter senders would leak parked deploys — this pin catches
    /// that class of change.
    #[tokio::test]
    async fn registry_drop_surfaces_recverror_to_parked_waiter() {
        let reg = LockRegistry::new();
        let _held = reg
            .try_acquire_range((1, 42), 0, 100, LockMode::Write, holder(1), deploy(1))
            .expect("initial acquire");
        let (_wait_id, wait_rx) = expect_parked(
            reg.try_acquire_range_wait(
                (1, 42),
                0,
                100,
                LockMode::Write,
                holder(2),
                deploy(2),
                WaitPolicy::Wait,
            )
            .unwrap(),
        );
        // Drop the registry — the inner HashMap (holding the Waiter,
        // which owns the oneshot::Sender for `wait_rx`) is dropped.
        drop(reg);
        // Receiver observes `RecvError` — callers translate this to
        // `LockError::Cancelled` at the ParkedHandle::wait layer
        // (see the docstring at Waiter.admit line 308-311).  Here
        // we assert the raw signal.
        assert!(
            wait_rx.await.is_err(),
            "registry drop must surface RecvError to parked waiter — \
             regression scenario: a future `impl Drop for LockRegistry` \
             that mem::swaps the inner state out (without draining \
             waiter senders first) would leave the sender alive and \
             the receiver hung.  Current implementation has no `impl \
             Drop`; the guarantee comes from Arc/HashMap propagation."
        );
    }

    /// **Phase 8 review follow-up: randomized tokio-schedule stress
    /// test (2026-08-26).**  Spawns N cooperating tokio tasks that
    /// each drive a random acquire → hold → release sequence
    /// against the LockRegistry under contention, then asserts the
    /// clean-state invariant after all tasks drain: no held locks,
    /// no parked waiters, no orphan handle IDs.
    ///
    /// Purpose: shakes out concurrency bugs that fixed-scenario
    /// tests miss — interleavings where a release admits waiters
    /// in an unexpected order, where cancellation races with
    /// admission, where the wake-loop terminates early under load,
    /// etc.  Complements the 80+ fixed-scenario tests above.
    ///
    /// Determinism: the tokio scheduler chooses task interleaving
    /// non-deterministically, but the property being checked —
    /// "post-drain state is empty" — is scheduler-independent.  A
    /// regression that leaks a range entry or waiter under contention
    /// would trip on some fraction of runs.  For CI-stability we
    /// run 8 iterations with 8 tasks each; a real property-test
    /// harness would run hundreds.
    ///
    /// Not a `#[cfg(loom)]` variant — loom exhaustively explores
    /// interleavings but would require porting to loom primitives
    /// (loom::sync::RwLock, loom::sync::atomic::AtomicU64, etc.).
    /// That's a follow-up if this simpler test surfaces something
    /// concerning.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn randomized_concurrent_acquire_release_drains_clean() {
        use std::sync::atomic::{AtomicU64, Ordering as AOrdering};

        // Deterministic per-iteration seed (deploy_scope byte) so
        // each iteration exercises a different scheduling shape
        // but the outer test is reproducible.
        const ITERATIONS: u64 = 8;
        const WORKERS_PER_ITER: usize = 8;
        const OPS_PER_WORKER: usize = 12;
        // Small file-id space so contention is realistic.
        const FILE_IDS: u64 = 3;
        // Small range space so overlaps happen.
        const RANGE_MAX_OFFSET: u64 = 32;
        const RANGE_MAX_LEN: u64 = 8;

        for iter in 0..ITERATIONS {
            let reg = Arc::new(LockRegistry::new());
            // Simple LCG for reproducible per-op choices without
            // pulling in rand as a workspace dep just for this pin.
            let seed = Arc::new(AtomicU64::new(
                0x9e37_79b9_7f4a_7c15u64 ^ iter.wrapping_mul(31),
            ));
            let mut handles = Vec::with_capacity(WORKERS_PER_ITER);

            for worker_id in 0..WORKERS_PER_ITER {
                let reg = reg.clone();
                let seed = seed.clone();
                let holder = holder(worker_id as u8 + 1);
                let deploy = deploy(worker_id as u8 + 1);
                handles.push(tokio::spawn(async move {
                    // Held-lock stack per worker so drain returns to
                    // clean state at the end.
                    let mut held: Vec<LockId> = Vec::new();
                    for _ in 0..OPS_PER_WORKER {
                        // LCG step; bits chosen for offset, length,
                        // mode, dev_inode, op-kind.
                        let s = seed.fetch_add(0x9e37_79b9_7f4a_7c15u64, AOrdering::Relaxed);
                        let dev_inode = (1u64, s % FILE_IDS);
                        let offset = (s >> 3) % RANGE_MAX_OFFSET;
                        let length = 1 + ((s >> 8) % RANGE_MAX_LEN);
                        let mode = if (s >> 14) & 1 == 0 {
                            LockMode::Read
                        } else {
                            LockMode::Write
                        };
                        let op_kind = (s >> 15) & 0b11;
                        match op_kind {
                            0 | 1 => {
                                // 50% acquire (wait:false).
                                if let Ok(id) = reg.try_acquire_range(
                                    dev_inode,
                                    offset,
                                    length,
                                    mode,
                                    holder.clone(),
                                    deploy,
                                ) {
                                    held.push(id);
                                }
                            }
                            2 => {
                                // 25% acquire wait:true (may park).
                                match reg.try_acquire_range_wait(
                                    dev_inode,
                                    offset,
                                    length,
                                    mode,
                                    holder.clone(),
                                    deploy,
                                    WaitPolicy::Wait,
                                ) {
                                    Ok(AcquireOutcome::Immediate(id)) => held.push(id),
                                    Ok(AcquireOutcome::Parked { admit, .. }) => {
                                        // Give the scheduler up to
                                        // 25ms to admit us; else
                                        // drop the receiver, which
                                        // eventually surfaces as
                                        // rollback via release
                                        // paths.
                                        let with_timeout = tokio::time::timeout(
                                            std::time::Duration::from_millis(25),
                                            admit,
                                        )
                                        .await;
                                        if let Ok(Ok(Ok(id))) = with_timeout {
                                            held.push(id);
                                        }
                                    }
                                    Err(_) => {}
                                }
                            }
                            _ => {
                                // 25% release (if we hold anything).
                                if let Some(id) = held.pop() {
                                    let _ = reg.release(id);
                                }
                            }
                        }
                        // Yield to encourage interleaving.
                        tokio::task::yield_now().await;
                    }
                    // Drain any leftover held locks so the worker
                    // exits clean.
                    for id in held.drain(..) {
                        let _ = reg.release(id);
                    }
                }));
            }

            for h in handles {
                h.await.expect("worker task did not panic");
            }
            // Final sweep: cancel any lingering waiters (parked
            // wait:true acquires whose 25ms timeout hit) by
            // simulating deploy-end sweep for every holder.
            for wid in 0..WORKERS_PER_ITER {
                reg.cancel_all_waiters_for_deploy(&deploy(wid as u8 + 1));
                reg.release_all_for_deploy(&deploy(wid as u8 + 1));
            }
            // Poll-until-quiescent so any wake_waiters chain that
            // needs many task-scheduling hops to complete is given
            // enough runway.  Bounded max (100 iterations × 1ms =
            // 100ms) so a genuine deadlock still trips the test's
            // outer tokio timeout rather than hanging indefinitely.
            // Pre-fix was a hard-coded 4 yields — under CI parallelism
            // that could produce rare flakes when wake_waiters chains
            // were longer than 4 hops.
            for _ in 0..100 {
                if reg.held_locks() == 0 && reg.parked_waiters() == 0 {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }
            assert_eq!(
                reg.held_locks(),
                0,
                "iter {iter}: held_locks should be 0 after full drain; \
                 a non-zero count indicates a range entry was inserted \
                 without a matching release/rollback path.  Regression \
                 investigation: check wake_waiters admission → range \
                 rollback on Sender dropped, and release_all_for_deploy \
                 sweep."
            );
            assert_eq!(
                reg.parked_waiters(),
                0,
                "iter {iter}: parked_waiters should be 0 after full drain; \
                 a non-zero count indicates a Waiter was enqueued without \
                 a matching admit/cancel path.  Regression investigation: \
                 check cancel_all_waiters_for_deploy vs. \
                 cancel_all_waiters_for_holder path coverage."
            );
        }
    }
}
