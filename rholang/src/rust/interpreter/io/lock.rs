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
//! ## Wait:true (deferred to slice 8b)
//!
//! Every acquire in this MVP returns immediately with either a `LockId`
//! or `LockError::Busy`.  Blocking acquisition is slice 8b via the
//! Rig-protocol: leader synthesizes an error Produce on cancel/timeout
//! via `Produce::with_error()` (mirrors `reduce.rs::produce_inner` line
//! 369, the OpenAI/Ollama pathway).  See plan §X-2 for the design.
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

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

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
/// `File.close`.  Derived from the File-agent's `stateP` GPrivate
/// name at cap-mint time — unique per fresh-mint open.
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
        if length == 0 {
            // A zero-length lock protects nothing and never conflicts;
            // silently accepting invites subtle race bugs.  Reject.
            return Err(LockError::BadArg);
        }
        let mut guard = self.inner.write().expect("lock registry poisoned");
        let state = guard.entry(dev_inode).or_default();
        if state.sequential_holder.is_some() {
            return Err(LockError::Busy);
        }
        if state.ranges.len() >= MAX_RANGES_PER_FILE {
            return Err(LockError::QuotaExceeded);
        }
        for entry in &state.ranges {
            if !ranges_overlap((entry.offset, entry.length), (offset, length)) {
                continue;
            }
            // Two overlapping reads coexist (spec §1143 "Multiple readers
            // of overlapping ranges coexist").
            if mode == LockMode::Read && entry.mode == LockMode::Read {
                continue;
            }
            // Same-holder overlapping acquires coexist (see docstring).
            if entry.holder == holder {
                continue;
            }
            return Err(LockError::Busy);
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
        Ok(id)
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
        let mut guard = self.inner.write().expect("lock registry poisoned");
        let state = guard.entry(dev_inode).or_default();
        if state.sequential_holder.is_some() || !state.ranges.is_empty() {
            return Err(LockError::Busy);
        }
        let id = self.mint_id()?;
        state.sequential_holder = Some(SequentialEntry { id, holder, deploy });
        Ok(id)
    }

    /// Release a specific lock by id.  Returns `Ok(())` if the id was
    /// held (either as a range or the sequential holder), `Err(Closed)`
    /// if not.  Evicts the `(dev, inode)` entry from the map if both
    /// substructures become empty — closes the inode-reuse safety gap.
    pub fn release(&self, lock_id: LockId) -> Result<(), LockError> {
        let mut guard = self.inner.write().expect("lock registry poisoned");
        let mut evict_key: Option<DevInode> = None;
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
                if state.ranges.is_empty() && state.sequential_holder.is_none() {
                    evict_key = Some(*dev_inode);
                }
                break;
            }
        }
        if let Some(k) = evict_key {
            guard.remove(&k);
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
            if state.ranges.is_empty() && state.sequential_holder.is_none() {
                evict.push(*dev_inode);
            }
        }
        for k in evict {
            guard.remove(&k);
        }
        released
    }

    /// Release every lock owned by `deploy`.  Called from the
    /// `WalDeployScope::end` auto-release hook (MUST per X-4 / spec
    /// §Explicit locks).  Returns the number of locks released.
    ///
    /// # Sentinel guard: `[0; 32]` is reserved as the slice-8a step-4
    /// placeholder for "no real DeployScope wired yet."  Step 5's
    /// natives (`fs_lock_range` / `fs_lock_sequential`) pass this
    /// placeholder while step 6 is unimplemented.  Calling
    /// `release_all_for_deploy(&[0; 32])` before step 6 wires real
    /// deploy identities would sweep EVERY currently-held lock —
    /// masking a bug as a working sweep.  The debug-assert below
    /// turns that into a loud test failure so step 6 must land the
    /// real DeployScope before enabling the auto-release hook.
    /// Post-step-6 this guard can be removed (or repurposed to
    /// reject any all-zero scope as caller error).
    pub fn release_all_for_deploy(&self, deploy: &DeployScope) -> usize {
        debug_assert!(
            deploy != &[0u8; 32],
            "release_all_for_deploy called with the [0; 32] sentinel — this is \
             slice-8a step-4's placeholder DeployScope.  Step 6 must wire real \
             per-deploy identities before enabling any auto-release hook, or \
             this sweep will nuke every held lock in the registry."
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
            if state.ranges.is_empty() && state.sequential_holder.is_none() {
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

    /// Diagnostic count of currently-tracked `(dev, inode)` entries.
    pub fn tracked_files(&self) -> usize {
        let guard = self.inner.read().expect("lock registry poisoned");
        guard.len()
    }

    /// Diagnostic count of currently-held locks (positional + sequential).
    pub fn held_locks(&self) -> usize {
        let guard = self.inner.read().expect("lock registry poisoned");
        guard
            .values()
            .map(|s| s.ranges.len() + s.sequential_holder.iter().count())
            .sum()
    }

    /// Test-only: seed the LockId counter so tests can trigger the
    /// `LOCK_ID_CEILING` guard without doing 10¹⁹ acquisitions.
    #[cfg(test)]
    fn set_next_lock_id_for_testing(&self, v: u64) { self.next_lock_id.store(v, Ordering::SeqCst); }
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
    fn release_all_for_deploy_zero_sentinel_panics_in_debug() {
        // Step-4 placeholder guard: sweeping on `[0; 32]` would
        // release every held lock in the registry, because natives
        // (step 5) currently pass this placeholder as their deploy
        // scope until step 6 wires real per-deploy identities.  This
        // test pins the guard so a premature step-6 partial-wire that
        // accidentally invokes `release_all_for_deploy(&[0; 32])`
        // fails loudly here rather than silently nuking locks.
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
}
