// The 28 native filesystem handlers (verified count as of A8-M-1
// retirement, 2026-09-03: 20 fs syscalls + 4 lock natives + 3 per-fd
// stream natives + 1 quarantine helper).
//
// Each handler:
//   1. Unapplies the incoming contract call to extract
//      `(produce, is_replay, previous_output, args)`.
//   2. Cost pre-charge at handler entry via `metering.reserve_primitive`
//      (or `reserve_incremental_primitive` for length-parameterized
//      helpers).  Consensus mode: `is_replay = true` STILL charges +
//      re-executes for the 15 Phase-5-verifying handlers (each
//      compares its fresh reply hash to the leader's cached hash and
//      fires FSERR_CONSENSUS_DIVERGENCE on mismatch — see
//      `verify_reply_hash_matches_cached`).  The 15 are: fs_write,
//      fs_write_at, fs_truncate, fs_chmod, fs_remove_file,
//      fs_remove_dir, fs_rename, fs_copy_file, fs_read, fs_read_at,
//      fs_stat, fs_entries, fs_size, fs_seek, fs_exists (the last
//      lifted from an explicit Consensus ban on 2026-09-04, when
//      SNAPSHOT_FORMAT_VERSION bumped 5 → 6).  Non-verifying handlers
//      (locks, quarantine, open/close/flush/tell, streaming-open)
//      tautologically echo `previous_output` on replay per the pre-
//      Phase-5 pattern.  See `docs/consensus-invariants.md` §"Per-op
//      re-execute behavior" for the current 15/28 verify matrix.
//   3. Path-taking leaf ops descend via `safe_descend_verified`
//      (H-5 rename-and-recreate check + H-P7-6 O_NOFOLLOW at every
//      step) and issue the leaf syscall as an `*at` call against the
//      returned dirfd — TOCTOU-immune.  See `path.rs::SafeParent`.
//   4. Syscalls dispatch in a `spawn_blocking` task so long-blocking
//      `fsync` / recursive walks never stall the reactor.
//   5. Reply shape: `[true, ...]` on success, `[false, code, msg]` on
//      failure.  DD-RemoveDirReplyShape (2026-09-03) unified
//      removeDir's replies with `nDeleted` at position 1/3 —
//      see the handler's header for the exception.
//
// Error messages are scrubbed via `io_msg_scrub` — we surface the
// `std::io::ErrorKind` classification but not the free-form message
// (which on some platforms includes the offending path, leaking the
// caller's root prefix).
//
// # X-6d A-07 (2026-09-12, branch-review-2026-09-11.md Track A) —
//   What lives in this file
//
// Wave-3 S3.13b split the 27 trait-registered handlers into per-
// family modules.  Post-split, this file contains:
//
//   1. Top-level exports + doc comments (this header).
//   2. `spawn_blocking_par` / `consensus_divergence_reply` /
//      `unlink_leaf_via_dirfd` — cross-family helpers used by both
//      the trait-registered handlers and fs_remove_dir.
//   3. Constants: `MAX_ENTRIES`, `MAX_WRITE_BYTES` + their
//      CONSENSUS_FOLD registrations.
//   4. `FsProcesses` struct + constructor + `is_contract_call`.
//   5. **`fs_remove_dir` method** (trait-exempt per wave-3-plan.md
//      § S3.11) + its exclusive helpers
//      (`finalize_failure_journal`, `journal_path_mutation_single`).
//   6. 30+ `pub(super)` shared helpers: WAL journaling entry points
//      (`journal_write_via_table`, `journal_read_via_table`,
//      `journal_state_read_via_table`, etc.), path/mode
//      resolvers (`resolve_cmode`, `resolve_lock_mode`,
//      `holder_id_of`, `leaf_of`), stat helpers
//      (`fstatat_meta`, `target_dev_inode_at`, `entry_stat_row`),
//      the `RemoveKind` enum, `chown_impl`, `readdir_one_entry`,
//      `reply_is_ok`.
//   7. Test module — pins for the above, plus fs_remove_dir E2E
//      coverage.
//
// # fs_remove_dir stays trait-exempt
//
// fs_remove_dir has (a) two structurally distinct dispatch modes
// (recursive vs non-recursive) with divergent WAL shapes, (b) an
// inline recursive-walk syscall loop that runs under a
// spawn_blocking closure holding the FsProcesses' lock registry
// clone, (c) a reply shape that carries an `nDeleted` count field
// per DD-RemoveDirReplyShape.  These make the trait's uniform
// dispatch shape a poor fit — see wave-3-plan.md § S3.11 for the
// full rationale.
//
// # Future extraction targets (deferred)
//
// - `handlers_removedir.rs` — pull fs_remove_dir + its 2 helper
//   methods (finalize_failure_journal, journal_path_mutation_single)
//   + its 8 exclusive free-fn helpers into a dedicated file.  Uses
//   the split-`impl FsProcesses`-across-files pattern.
// - `handlers_helpers.rs` — pull the 30+ pub(super) shared helpers
//   into a dedicated helpers module.  Reduces handlers.rs to just
//   the FsProcesses definition + fs_remove_dir.
//
// Both are safe refactors (all callers within io/); tracked as
// A-07 follow-ups.  This slice removed 687 lines of dead
// `_deleted_pre_wave3_*` code (11 methods, all `#[cfg(any())]`-
// gated); the further splits reduce noise but don't change
// semantics.

#[allow(unused_imports)]
use models::rhoapi::Par;

use super::super::contract_call::ContractCall;
use super::super::dispatch::RhoDispatch;
use super::super::metering::MeteredMachine;
use super::super::rho_runtime::RhoISpace;
// X-6f A-07 Phase 3 (2026-09-13): test modules below `use super::*;`
// to pull in the helpers via the re-export.  Bring `Par` in from
// models directly and the FSERR constants + reply helpers +
// safe_descend_verified from siblings so tests can name them
// without a per-module import.
#[allow(unused_imports)]
use super::errors::{FserrCode, FSERR_CONSENSUS_DIVERGENCE, FSERR_QUOTA_EXCEEDED};
use super::handle_table::FileHandleTable;
// X-6f A-07 Phase 3 (2026-09-13): re-export the extracted helpers so
// existing `super::handlers::X` call sites in the family modules
// keep working without a codebase-wide rename.  The canonical
// definitions live in `handlers_helpers.rs`; this glob re-export is
// the compat shim.  Sibling modules MAY prefer
// `super::handlers_helpers::X` for new code (more precise) — both
// paths resolve to the same symbol.
//
// # X-8 A-new-1 (2026-09-13, branch-review-2026-09-13.md) —
//   INTENTIONAL PERMANENT COMPAT
//
// This re-export is NOT a temporary bridge awaiting a sunset date.
// It is the permanent public surface of the fs handler helpers.
// Rationale:
//
// - `handlers.rs` is the module every fs-handler sibling has
//   historically imported from (`super::handlers::X`).  Rewriting
//   every sibling to import from `super::handlers_helpers::X`
//   would be a codebase-wide churn with zero semantic benefit —
//   the re-export makes both paths equivalent, and Rust resolves
//   them to the same symbol at link time (no runtime cost, no
//   duplication).
// - The two-path invariant is enforceable: any new symbol added
//   to `handlers_helpers.rs` is automatically visible via
//   `handlers::X` through this glob, so contributors don't have
//   to remember to add re-exports one-by-one.
// - No maintenance hazard exists: the split (handlers_helpers.rs
//   holding the definitions, handlers.rs re-exporting) is fine.
//   A future refactor that would REQUIRE removing this shim
//   would have to remove `handlers.rs` entirely — which is a
//   larger conversation than a lint would help with.
//
// **Do not remove this re-export.**  If you want to consolidate
// (e.g., merge handlers.rs into another file), that's a separate
// discussion; the shim itself is not tech debt.
pub use super::handlers_helpers::*;
#[allow(unused_imports)]
use super::lock::{HolderId, LockMode};
#[allow(unused_imports)]
use super::path::safe_descend_verified;
#[allow(unused_imports)]
use super::response::err;
#[allow(unused_imports)]
use super::{ConsensusMode, CMODE_CONSENSUS_STR, CMODE_ORACULAR_STR};

/// Shared per-runtime state for the fs native handlers.  Cloned into
/// each handler closure via `ProcessContext`.
///
/// Phase 9 slice 9b: `metering` is the per-deploy `MeteredMachine`
/// shared with the reducer.  Handler entries emit
/// `metering.reserve_primitive(costs::fs_X())` before doing any
/// work, so a deploy that exhausts its budget is rejected at the
/// syscall boundary rather than mid-flight.  See
/// `rholang/src/rust/interpreter/io/costs.rs` for the weight table
/// and `rholang/tests/fileio_cost_spec.rs` for the golden-value
/// regression pins.
#[derive(Clone)]
pub struct FsProcesses {
    pub dispatcher: RhoDispatch,
    pub space: RhoISpace,
    pub handles: FileHandleTable,
    pub mode: ConsensusMode,
    pub metering: MeteredMachine,
}

impl FsProcesses {
    pub fn new(
        dispatcher: RhoDispatch,
        space: RhoISpace,
        handles: FileHandleTable,
        mode: ConsensusMode,
        metering: MeteredMachine,
    ) -> Self {
        FsProcesses {
            dispatcher,
            space,
            handles,
            mode,
            metering,
        }
    }

    pub(super) fn is_contract_call(&self) -> ContractCall {
        ContractCall {
            space: self.space.clone(),
            dispatcher: self.dispatcher.clone(),
        }
    }

    // ---------------------------------------------------------------
    // H-29-3 stopgap lift (2026-08-26): path-based mutation journal
    // helpers.  Every path-based Consensus-cap mutation
    // (`fs_chmod`, `fs_chown`, `fs_rename`, `fs_copy_file`,
    // `fs_remove_file`, `fs_remove_dir`) now journals to the WAL
    // BEFORE the syscall runs, matching the `journal_write` /
    // `journal_truncate` pattern.  Leader/follower symmetry is
    // straightforward for these 1-op mutations: the WAL entry is
    // fully derivable from the caller-supplied args (canon_path,
    // mode_bits, owner/group, extra_path) — both sides journal
    // from identical args, both hit `MAX_WAL_ENTRIES` at the same
    // moment, both finalize `Failure { code }` on syscall error.
    //
    // Cmode is passed as an argument (not from a FileHandle) since
    // these are path-based, not fd-based.  The helpers no-op on
    // Oracular caps (return `Ok(false)`) so the caller doesn't need
    // to gate on cmode itself.
    //
    // All six helpers return `Err(())` on WAL-cap exhaustion so the
    // caller can translate to FSERR_QUOTA_EXCEEDED and short-circuit
    // symmetrically on both leader and follower.
    // ---------------------------------------------------------------
}

// =====================================================================
// FsHandler trait migrations (wave 3, per FIPS/.../wave-3-plan.md)
// =====================================================================
//
// Each migrated handler contributes:
//   1. A `pub struct FsXHandler;` marker type (no fields).
//   2. `impl FsHandler for FsXHandler` — parse_content, cost, dispatch.
//   3. `#[linkme::distributed_slice(FS_HANDLERS)] static FS_X_ENTRY`
//      — one entry per handler in the distributed slice consumed by
//      rho_runtime.rs (post-S3.12).
//
// The corresponding `pub async fn fs_x(&self, ...)` method inside the
// `impl FsProcesses` block above collapses to a one-line
// `dispatch_via_trait::<FsXHandler>(self, contract_args).await`
// wrapper, kept for `rho_runtime.rs` compatibility until the S3.12
// switchover.

// ---------------------------------------------------------------------
// Helpers — pure fns (no self) called from spawn_blocking closures.
// ---------------------------------------------------------------------

// ---------------------------------------------------------------------
// Slice 26 review-fix tests: `resolve_cmode` (MT-26-1, ST-26-2).
// ---------------------------------------------------------------------

#[cfg(test)]
mod cmode_tests {
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::Expr;
    use models::rust::utils::{new_boundvar_par, new_gbool_par, new_gint_par, new_gstring_par};

    use super::*;

    fn s(v: &str) -> Par { new_gstring_par(v.to_string(), Vec::new(), false) }
    fn nil() -> Par { Par::default() }
    fn i(n: i64) -> Par { new_gint_par(n, Vec::new(), false) }
    fn b(x: bool) -> Par { new_gbool_par(x, Vec::new(), false) }
    fn bytes() -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GByteArray(vec![0u8, 1, 2])),
        }])
    }

    #[test]
    fn resolve_cmode_accepts_oracular_lowercase() {
        assert_eq!(resolve_cmode(&s("oracular")), Some(ConsensusMode::Oracular));
    }

    #[test]
    fn resolve_cmode_accepts_consensus_lowercase() {
        assert_eq!(
            resolve_cmode(&s("consensus")),
            Some(ConsensusMode::Consensus)
        );
    }

    // ST-26-2: case-sensitivity — capitalized / uppercase forms must
    // NOT be accepted.  A caller passing `"Consensus"` (mismatched
    // convention) MUST get rejected, not silently downgraded.
    #[test]
    fn resolve_cmode_rejects_capitalized() {
        assert_eq!(resolve_cmode(&s("Oracular")), None);
        assert_eq!(resolve_cmode(&s("Consensus")), None);
        assert_eq!(resolve_cmode(&s("CONSENSUS")), None);
    }

    // ST-26-2: whitespace-padded / trailing-space variants also
    // rejected.
    #[test]
    fn resolve_cmode_rejects_whitespace() {
        assert_eq!(resolve_cmode(&s(" oracular")), None);
        assert_eq!(resolve_cmode(&s("consensus ")), None);
        assert_eq!(resolve_cmode(&s("\toracular")), None);
        assert_eq!(resolve_cmode(&s("consensus\n")), None);
    }

    // MT-26-1: non-String Par shapes must all fall to None so the
    // handler surfaces FSERR_BAD_ARG.  Under the pre-fix fallback
    // behavior each of these would have silently defaulted to
    // Oracular.
    #[test]
    fn resolve_cmode_rejects_nil() {
        assert_eq!(resolve_cmode(&nil()), None);
    }

    #[test]
    fn resolve_cmode_rejects_int() {
        assert_eq!(resolve_cmode(&i(0)), None);
        assert_eq!(resolve_cmode(&i(1)), None);
    }

    #[test]
    fn resolve_cmode_rejects_bool() {
        assert_eq!(resolve_cmode(&b(true)), None);
        assert_eq!(resolve_cmode(&b(false)), None);
    }

    #[test]
    fn resolve_cmode_rejects_bytearray() {
        assert_eq!(resolve_cmode(&bytes()), None);
    }

    #[test]
    fn resolve_cmode_rejects_empty_string() {
        assert_eq!(resolve_cmode(&s("")), None);
    }

    #[test]
    fn resolve_cmode_rejects_unknown_string() {
        assert_eq!(resolve_cmode(&s("bogus")), None);
        assert_eq!(resolve_cmode(&s("oracle")), None);
        assert_eq!(resolve_cmode(&s("cons")), None);
    }

    #[test]
    fn resolve_cmode_rejects_boundvar_par() {
        // BoundVar par (an unbound Rholang variable position) must
        // also fall to None — RhoString::unapply returns None for it.
        let bv = new_boundvar_par(0, Vec::new(), false);
        assert_eq!(resolve_cmode(&bv), None);
    }

    // NT-26-3: pin `ConsensusMode::default()` so a future refactor
    // flipping the default would trip a test rather than silently
    // change every fallback direction.
    #[test]
    fn consensus_mode_default_is_consensus() {
        assert_eq!(ConsensusMode::default(), ConsensusMode::Consensus);
    }

    // Drift assertion for the shared constants (M-26-3): the
    // handler-side and composer-side constants MUST match byte-for-
    // byte or the composed source becomes unroutable.
    #[test]
    fn cmode_string_constants_are_stable() {
        assert_eq!(CMODE_ORACULAR_STR, "oracular");
        assert_eq!(CMODE_CONSENSUS_STR, "consensus");
    }

    // -- Phase 8 slice 8a — lock-native helpers -------------------------

    #[test]
    fn resolve_lock_mode_accepts_r_and_w() {
        assert_eq!(resolve_lock_mode(&s("r")), Some(LockMode::Read));
        assert_eq!(resolve_lock_mode(&s("w")), Some(LockMode::Write));
    }

    #[test]
    fn resolve_lock_mode_rejects_capitalized_and_padded() {
        assert_eq!(resolve_lock_mode(&s("R")), None);
        assert_eq!(resolve_lock_mode(&s("W")), None);
        assert_eq!(resolve_lock_mode(&s(" r")), None);
        assert_eq!(resolve_lock_mode(&s("r ")), None);
    }

    #[test]
    fn resolve_lock_mode_rejects_other_strings() {
        assert_eq!(resolve_lock_mode(&s("")), None);
        assert_eq!(resolve_lock_mode(&s("rw")), None);
        assert_eq!(resolve_lock_mode(&s("read")), None);
        assert_eq!(resolve_lock_mode(&s("write")), None);
        assert_eq!(resolve_lock_mode(&s("x")), None);
    }

    #[test]
    fn resolve_lock_mode_rejects_non_string_par() {
        // Fail-closed on any Par shape other than a String.  Mirrors
        // `resolve_cmode`'s discipline — a caller passing an Int or
        // Bool must not be silently downgraded to a default mode.
        assert_eq!(resolve_lock_mode(&nil()), None);
        assert_eq!(resolve_lock_mode(&i(0)), None);
        assert_eq!(resolve_lock_mode(&b(true)), None);
        assert_eq!(resolve_lock_mode(&bytes()), None);
    }

    #[test]
    fn holder_id_of_is_deterministic() {
        // Equal Pars must hash to the same HolderId across calls —
        // this is what makes `release_all_for_holder` work across
        // deploys.  A drift here would break File.close's ability to
        // release the specific cap's locks.
        let a = s("holder-1");
        let b = s("holder-1");
        assert_eq!(holder_id_of(&a), holder_id_of(&b));
    }

    #[test]
    fn holder_id_of_distinguishes_different_pars() {
        assert_ne!(holder_id_of(&s("holder-1")), holder_id_of(&s("holder-2")));
        assert_ne!(holder_id_of(&s("holder")), holder_id_of(&nil()));
        assert_ne!(holder_id_of(&i(1)), holder_id_of(&i(2)));
    }

    #[test]
    fn holder_id_of_hash_width_contract() {
        // The module docstring + runtime assertion require Blake2b256
        // → 32 bytes.  Pinned here so a provider swap producing a
        // shorter digest is caught at test time rather than at first
        // runtime call.
        let h = holder_id_of(&s("any-par"));
        assert_eq!(h.bytes.len(), 32);
    }

    // ---------------------------------------------------------------
    // Step-5 review Gap 2: pin fs_lock_range / fs_lock_sequential
    // handlers read scope from `self.handles.current_deploy_scope`
    // (the per-runtime cell set by WalDeployScope at deploy entry)
    // and NOT from `DeployScope::default()` (the pre-step-5
    // placeholder that was removed in step 5).  A regression that
    // reverted to the placeholder would silently break the auto-
    // release sweep: acquires would record the sentinel `[0; 32]`
    // scope, and any release_all_for_deploy sweep from a
    // WalDeployScope::drop would fail to clear them (scope
    // mismatch); or worse, a manual release_all_for_deploy(&[0; 32])
    // call would nuke every stray sentinel entry — now guarded by
    // the assert! in commit 6f537099, so this scenario would panic
    // loudly rather than silently corrupt state.
    // ---------------------------------------------------------------

    /// **Gap 2a**: pin fs_lock_range's scope-read.
    ///
    /// Wave-3 S3.3 (2026-09-08) update: the anchor moved from the
    /// (now 4-line) `pub async fn fs_lock_range` wrapper to
    /// `impl FsHandler for FsLockRangeHandler`.  The invariant is
    /// unchanged: the acquire path MUST read scope via
    /// `current_deploy_scope` (either the pre-wave-3
    /// `self.current_deploy_scope()` or the wave-3
    /// `ctx.current_deploy_scope()`, both of which read the
    /// `handles.current_deploy_scope` cell).
    #[test]
    fn fs_lock_range_reads_current_deploy_scope() {
        // S3.13b (2026-09-10): Lock family moved to handlers_lock.rs.
        let src = include_str!("handlers_lock.rs");
        let anchor = src
            .find("impl FsHandler for FsLockRangeHandler")
            .expect("handlers_lock.rs missing FsLockRangeHandler impl block");
        let window = &src[anchor..std::cmp::min(anchor + 8000, src.len())];
        assert!(
            window.contains("current_deploy_scope"),
            "step 5 regression: FsLockRangeHandler must read scope from \
             ctx.current_deploy_scope() (which reads \
             self.handles.current_deploy_scope — the per-runtime cell \
             WalDeployScope publishes at deploy entry)"
        );
        assert!(
            !window.contains("DeployScope::default()"),
            "step 5 regression: FsLockRangeHandler must NOT fall back \
             to DeployScope::default() — that pre-step-5 placeholder \
             path was removed in step 5.  Under step-5 semantics, an \
             acquire outside a live WalDeployScope reads the sentinel \
             [0; 32] cell value; a release_all_for_deploy call using \
             the default would trip the sentinel-guard assert! in \
             release_all_for_deploy (commit 6f537099)."
        );
    }

    /// **Gap 2b**: pin fs_lock_sequential's scope-read.
    /// See `fs_lock_range_reads_current_deploy_scope` for the wave-3
    /// anchor-relocation rationale.
    #[test]
    fn fs_lock_sequential_reads_current_deploy_scope() {
        // S3.13b (2026-09-10): Lock family moved to handlers_lock.rs.
        let src = include_str!("handlers_lock.rs");
        let anchor = src
            .find("impl FsHandler for FsLockSequentialHandler")
            .expect("handlers_lock.rs missing FsLockSequentialHandler impl block");
        let window = &src[anchor..std::cmp::min(anchor + 6000, src.len())];
        assert!(
            window.contains("current_deploy_scope"),
            "step 5 regression: FsLockSequentialHandler must read scope \
             from ctx.current_deploy_scope() (which reads \
             self.handles.current_deploy_scope)"
        );
        assert!(
            !window.contains("DeployScope::default()"),
            "step 5 regression: FsLockSequentialHandler must NOT fall \
             back to DeployScope::default() — see \
             fs_lock_range_reads_current_deploy_scope for rationale"
        );
    }

    // ---------------------------------------------------------------
    // Step 6 (2026-08-13) — mode-differentiated unlink gate tests.
    // ---------------------------------------------------------------

    /// Verify `target_dev_inode_at` returns Some((dev, ino)) for a
    /// real file, and None for a nonexistent one.  Small, direct
    /// unit test of the new helper introduced in step 6.
    #[test]
    fn target_dev_inode_at_reads_existing_file() {
        use std::io::Write;
        let tmpdir = tempfile::tempdir().expect("mktemp");
        let file_path = tmpdir.path().join("target.txt");
        {
            let mut f = std::fs::File::create(&file_path).expect("create");
            f.write_all(b"hi").expect("write");
        }
        // Use safe_descend_verified to get a SafeParent for the leaf.
        let parent =
            safe_descend_verified(tmpdir.path(), "target.txt", None).expect("safe_descend");
        let dev_inode = target_dev_inode_at(&parent);
        assert!(
            dev_inode.is_some(),
            "target_dev_inode_at must return Some for an existing file"
        );
        let (dev, ino) = dev_inode.unwrap();
        assert!(dev > 0, "dev must be non-zero on real fs");
        assert!(ino > 0, "ino must be non-zero on real fs");
    }

    /// `target_dev_inode_at` returns None on a nonexistent leaf.
    /// Callers treat None as "not locked" and let the subsequent
    /// unlink surface the appropriate error.
    #[test]
    fn target_dev_inode_at_returns_none_for_missing_leaf() {
        let tmpdir = tempfile::tempdir().expect("mktemp");
        let parent =
            safe_descend_verified(tmpdir.path(), "nonexistent.txt", None).expect("safe_descend");
        assert_eq!(target_dev_inode_at(&parent), None);
    }

    /// Step 6 review Gap 2: `target_dev_inode_at` uses
    /// `AT_SYMLINK_NOFOLLOW`, so a symlink leaf reports the LINK's
    /// own inode — NOT the target's.  Matches unlinkat's "remove the
    /// directory entry" semantics: unlinkat on a symlink removes the
    /// link, not what it points at.  Pin ensures a regression that
    /// drops the flag (or switches to `AT_EMPTY_PATH` / follows the
    /// link) would silently target the wrong entity — locks on the
    /// TARGET file would spuriously gate an unlink of the SYMLINK,
    /// and vice versa.
    #[test]
    fn target_dev_inode_at_reports_link_inode_not_target_inode() {
        use std::io::Write;
        let tmpdir = tempfile::tempdir().expect("mktemp");
        // Create the target file.
        let target_path = tmpdir.path().join("target.txt");
        {
            let mut f = std::fs::File::create(&target_path).expect("create target");
            f.write_all(b"hello").expect("write");
        }
        // Create a symlink to it in the same directory.
        let link_path = tmpdir.path().join("link.txt");
        std::os::unix::fs::symlink(&target_path, &link_path).expect("symlink");

        // Independently discover the target's inode via metadata (follows link).
        let target_meta = std::fs::metadata(&target_path).expect("stat target");
        use std::os::unix::fs::MetadataExt;
        let target_ino = target_meta.ino();

        // And the link's own inode via symlink_metadata (does NOT follow).
        let link_meta = std::fs::symlink_metadata(&link_path).expect("symlink_metadata");
        let link_ino = link_meta.ino();

        // Sanity: they must differ on a real filesystem.
        assert_ne!(
            target_ino, link_ino,
            "test precondition: target file and its symlink must have distinct inodes"
        );

        // Now via target_dev_inode_at:
        let parent = safe_descend_verified(tmpdir.path(), "link.txt", None).expect("safe_descend");
        let observed = target_dev_inode_at(&parent).expect("stat");
        assert_eq!(
            observed.1, link_ino,
            "target_dev_inode_at MUST report the LINK's inode (AT_SYMLINK_NOFOLLOW), \
             not the target's — a regression would let unlinkat operate on the \
             symlink while the LockRegistry query targets the wrong entity"
        );
        assert_ne!(
            observed.1, target_ino,
            "regression guard: if this equals target_ino, the flag has been dropped \
             or replaced by an option that follows the symlink"
        );
    }

    /// Step 6 review Gap 1: end-to-end composition test.  Verifies
    /// the building blocks used by the fs_remove_file / fs_remove_dir
    /// gate (target_dev_inode_at + LockRegistry::is_locked +
    /// LockRegistry::count_locks) compose correctly against a REAL
    /// filesystem, not just against unit-test fixtures.  Catches
    /// regressions where (a) target_dev_inode_at returns a different
    /// (dev, ino) than what LockRegistry entries key on, or (b)
    /// is_locked/count_locks look up under a different key shape.
    ///
    /// Doesn't invoke the handler itself — the source-scan pins
    /// (fs_remove_{file,dir}_has_step6_gate) verify the handler wires
    /// these building blocks; this test verifies the wiring
    /// terminates in the right filesystem entity.
    #[test]
    fn step6_gate_composition_against_real_filesystem() {
        use std::io::Write;
        // `HolderId` + `LockMode` are already imported via `use super::*`
        // in this module.  `LockRegistry` isn't in that import set, so
        // reach for it via its parent path.
        type LockRegistry = crate::rust::interpreter::io::lock::LockRegistry;
        let tmpdir = tempfile::tempdir().expect("mktemp");
        let file_path = tmpdir.path().join("target.txt");
        {
            let mut f = std::fs::File::create(&file_path).expect("create target");
            f.write_all(b"payload").expect("write");
        }
        // Handler flow step 1: safe_descend to the leaf.
        let parent = safe_descend_verified(tmpdir.path(), "target.txt", None)
            .expect("safe_descend on the real leaf");
        // Handler flow step 2: fstatat for (dev, ino).
        let dev_inode = target_dev_inode_at(&parent).expect("stat existing leaf");
        // Handler flow step 3: seed the LockRegistry with a lock on
        // that inode (simulates a live File cap holding a lock).
        let lock_registry = LockRegistry::new();
        let holder = HolderId::from_bytes([0x11u8; 32]);
        let deploy: [u8; 32] = [0x22u8; 32];
        lock_registry
            .try_acquire_range(dev_inode, 0, 100, LockMode::Write, holder.clone(), deploy)
            .expect("acquire");
        // Handler flow step 4: is_locked whole-file query — MUST report true.
        assert!(
            lock_registry.is_locked(dev_inode, (0, u64::MAX)),
            "is_locked composition: real-fs (dev, ino) key + acquire on \
             same key MUST report locked — otherwise the fs_remove_* gate \
             would spuriously admit unlinks of locked files"
        );
        // Handler flow step 5: count_locks — MUST report exact 1 (spec
        // §Mode-differentiated `{N} holder(s)` message).
        assert_eq!(
            lock_registry.count_locks(dev_inode),
            1,
            "count_locks composition: real-fs (dev, ino) key must yield \
             correct holder count for the Oracular log-warn message"
        );
        // Handler flow step 6: after release, gate MUST return false so
        // the unlink proceeds under Consensus (theoretical, H-29-3
        // blocks) or without a warn under Oracular.
        let n_released = lock_registry.release_all_for_holder(&holder);
        assert_eq!(n_released, 1);
        assert!(
            !lock_registry.is_locked(dev_inode, (0, u64::MAX)),
            "post-release: is_locked must report unlocked so the gate lets \
             the unlink proceed"
        );
        assert_eq!(lock_registry.count_locks(dev_inode), 0);
    }

    /// Pin fs_remove_file's step-6 gate: verify the handler calls
    /// `target_dev_inode_at` + `lock_registry.is_locked` AND
    /// dispatches on `cmode` inside the spawn_blocking closure
    /// (rather than the pre-step-6 early return).
    ///
    /// S3.12b (2026-09-09) re-anchor: `pub async fn fs_remove_file`
    /// was retired.  Live logic lives in `impl FsHandler for
    /// FsRemoveFileHandler`'s `dispatch` body; anchor there.
    #[test]
    fn fs_remove_file_has_step6_gate() {
        // S3.13b (2026-09-10): Mutation family moved to handlers_mutation.rs.
        let src = include_str!("handlers_mutation.rs");
        let fn_start = src
            .find("impl FsHandler for FsRemoveFileHandler")
            .expect("handlers_mutation.rs missing FsRemoveFileHandler trait impl");
        // 20KB window covers the extended step-6 body (grew after
        // S3.10 migrated into the trait impl, which pulled in
        // additional pre_syscall + journal hook bodies).
        let window = &src[fn_start..std::cmp::min(fn_start + 20000, src.len())];
        assert!(
            window.contains("target_dev_inode_at(&parent)"),
            "step 6 regression: fs_remove_file must call target_dev_inode_at \
             to resolve the target's (dev, inode) for the LockRegistry query"
        );
        assert!(
            window.contains("lock_registry.is_locked"),
            "step 6 regression: fs_remove_file must query \
             lock_registry.is_locked on the target to gate Consensus \
             unlinks per spec §Mode-differentiated invariants"
        );
        assert!(
            window.contains("ConsensusMode::Consensus")
                && window.contains("ConsensusMode::Oracular"),
            "step 6 regression: fs_remove_file must dispatch on cmode \
             inside spawn_blocking (Consensus locked → FSERR_BUSY; \
             Oracular locked → log-warn + proceed)"
        );
        assert!(
            window.contains("target: \"f1r3fly.fs.oracular\""),
            "step 6 regression: fs_remove_file's Oracular branch must \
             log-warn on locked-file delete for operator observability"
        );
    }

    /// Pin fs_remove_dir's step-6 gate — same structure as
    /// fs_remove_file's pin.  See that test's docstring for
    /// rationale.
    #[test]
    fn fs_remove_dir_has_step6_gate() {
        // X-6e A-07 Phase 2 (2026-09-13): fs_remove_dir moved to
        // handlers_removedir.rs.  Source-scan target updated
        // accordingly.
        let src = include_str!("handlers_removedir.rs");
        let fn_start = src
            .find("pub async fn fs_remove_dir")
            .expect("handlers_removedir.rs missing fs_remove_dir definition");
        // 30KB window: fs_remove_dir grew past 20KB in Phase 4
        // (2026-09-02) after the non-recursive Consensus follower
        // re-execute branch landed, adding a second spawn_blocking
        // body plus its lock-check + syscall + verify tail.
        let window = &src[fn_start..std::cmp::min(fn_start + 30000, src.len())];
        assert!(
            window.contains("target_dev_inode_at(&parent)"),
            "step 6 regression: fs_remove_dir must call target_dev_inode_at"
        );
        assert!(
            window.contains("lock_registry.is_locked"),
            "step 6 regression: fs_remove_dir must query \
             lock_registry.is_locked on the target"
        );
        assert!(
            window.contains("ConsensusMode::Consensus")
                && window.contains("ConsensusMode::Oracular"),
            "step 6 regression: fs_remove_dir must dispatch on cmode \
             inside spawn_blocking"
        );
        assert!(
            window.contains("target: \"f1r3fly.fs.oracular\""),
            "step 6 regression: fs_remove_dir's Oracular branch must \
             log-warn on locked-directory delete"
        );
    }

    // ---------------------------------------------------------------
    // Slice 8b sub-2 (2026-08-12) — `wait: true` native-handler
    // parking + Rig-protocol synth-error dispatch.  Source-scan pins
    // that the arity-flexible parse + WaitPolicy dispatch + admit-
    // await + Cancelled fallback are all present in each native.
    // Behavioral coverage is at the sub-5 integration-test layer
    // (file_dir_check.rs).
    // ---------------------------------------------------------------

    #[test]
    fn fs_lock_range_accepts_arity_8_with_wait_bool() {
        // Pins the sub-2 arity extension.  Regressions that revert to
        // arity-7-only would trip file_dir_check under sub-4 once
        // File.rho passes 8 args.
        //
        // Wave-3 S3.3 (2026-09-08): anchor moved from the (now 4-
        // line) `pub async fn fs_lock_range` wrapper to
        // `impl FsHandler for FsLockRangeHandler`.  The arity check
        // is now the trait's `ARITY` constant + framework arity
        // check — plus the destructure inside `parse_content`.  The
        // `wait_par`-at-slot-7 invariant is preserved by:
        //   1. `const ARITY: usize = 8` on the handler struct.
        //   2. `parse_content`'s `[fd_par, off_par, len_par,
        //      mode_par, holder_par, cmode_par, wait_par]` slice
        //      destructure (framework strips ack at index 7).
        //   3. `RhoBoolean::unapply(wait_par)` inside parse_content.
        // S3.13b (2026-09-10): Lock family moved to handlers_lock.rs.
        let src = include_str!("handlers_lock.rs");
        let anchor = src
            .find("impl FsHandler for FsLockRangeHandler")
            .expect("handlers_lock.rs missing FsLockRangeHandler impl block");
        let window = &src[anchor..std::cmp::min(anchor + 8000, src.len())];
        assert!(
            window.contains("const ARITY: usize = 8"),
            "sub-2 regression: FsLockRangeHandler must have ARITY = 8"
        );
        assert!(
            window
                .contains("[fd_par, off_par, len_par, mode_par, holder_par, cmode_par, wait_par]"),
            "sub-2 regression: FsLockRangeHandler::parse_content must \
             destructure 7 pre-ack args with `wait_par` at slot 6 \
             (framework strips ack at slot 7)"
        );
        assert!(
            window.contains("RhoBoolean::unapply(") && window.contains("wait_par"),
            "sub-2 regression: FsLockRangeHandler must parse wait as \
             RhoBoolean"
        );
        assert!(
            window.contains("WaitPolicy::Wait") && window.contains("WaitPolicy::Fail"),
            "sub-2 regression: FsLockRangeHandler must dispatch to \
             WaitPolicy based on wait: Bool"
        );
        assert!(
            window.contains("try_acquire_range_wait"),
            "sub-2 regression: FsLockRangeHandler must use the wait-aware \
             LockRegistry method"
        );
        assert!(
            window.contains("AcquireOutcome::Parked")
                && (window.contains("admit.await")
                    || window.contains("park_external_during(admit).await")),
            "sub-2 regression: FsLockRangeHandler must await the Parked \
             admission oneshot (directly or via \
             `park_external_during` — the S4.8 wrapper that parks the \
             reduction participant while awaiting an external event)"
        );
        // S4.8 (2026-09-11): the park_external wrapper is load-bearing
        // — without it, the deterministic_reduction driver deadlocks
        // waiting for the parked participant to submit its next
        // RSpace intent.  Pin the wrapper's presence so a refactor
        // that reverted to bare `admit.await` would be caught before
        // shipping.
        assert!(
            window.contains("park_external_during"),
            "S4.8 regression: FsLockRangeHandler must wrap `admit.await` \
             in `park_external_during` so the deterministic_reduction \
             driver's frontier_ready check can advance while this \
             participant is externally parked on the oneshot admit."
        );
        assert!(
            window.contains("LockError::Cancelled"),
            "sub-2 regression: FsLockRangeHandler must surface Cancelled \
             on oneshot RecvError (registry drop / no signal)"
        );
    }

    #[test]
    fn fs_lock_sequential_accepts_arity_5_with_wait_bool() {
        // Wave-3 S3.3 anchor relocation: see
        // `fs_lock_range_accepts_arity_8_with_wait_bool` for the
        // rationale.  Sequential arity is 5 in the pre-refactor sense
        // (fd, holder, cmode, wait, ack); the trait's ARITY = 5 and
        // parse_content destructures 4 pre-ack args.
        // S3.13b (2026-09-10): Lock family moved to handlers_lock.rs.
        let src = include_str!("handlers_lock.rs");
        let anchor = src
            .find("impl FsHandler for FsLockSequentialHandler")
            .expect("handlers_lock.rs missing FsLockSequentialHandler impl block");
        let window = &src[anchor..std::cmp::min(anchor + 6000, src.len())];
        assert!(
            window.contains("const ARITY: usize = 5"),
            "sub-2 regression: FsLockSequentialHandler must have ARITY = 5"
        );
        assert!(
            window.contains("[fd_par, holder_par, cmode_par, wait_par]"),
            "sub-2 regression: FsLockSequentialHandler::parse_content \
             must destructure 4 pre-ack args with `wait_par` at slot 3"
        );
        assert!(
            window.contains("RhoBoolean::unapply(") && window.contains("wait_par"),
            "sub-2 regression: FsLockSequentialHandler must parse wait \
             as RhoBoolean"
        );
        assert!(
            window.contains("try_acquire_sequential_wait"),
            "sub-2 regression: FsLockSequentialHandler must use the \
             wait-aware LockRegistry method"
        );
        assert!(
            window.contains("AcquireOutcome::Parked")
                && (window.contains("admit.await")
                    || window.contains("park_external_during(admit).await")),
            "sub-2 regression: FsLockSequentialHandler must await the \
             Parked admission oneshot (directly or via \
             `park_external_during` — the S4.8 wrapper that parks the \
             reduction participant while awaiting an external event)"
        );
        assert!(
            window.contains("park_external_during"),
            "S4.8 regression: FsLockSequentialHandler must wrap \
             `admit.await` in `park_external_during` so the \
             deterministic_reduction driver's frontier_ready check \
             can advance while this participant is externally parked."
        );
    }

    // Retired 2026-08-26 (Phase 8 arity tightening, commit 5e8f3e2a0):
    // `fs_lock_range_legacy_arity_7_defaults_wait_false` and
    // `fs_lock_sequential_legacy_arity_4_defaults_wait_false` pinned
    // the transitional shim that accepted arity-7/4 calls with
    // wait defaulted to false.  Sub-4 retired the shim; every
    // File.rho caller now passes arity 8/5 explicitly.  The
    // inverse invariant (shim is NOT present) is now pinned by
    // `fileio_cost_spec::lock_range_and_sequential_handlers_reject_arity_shim`.

    /// **Sub-6 review round-2 source-scan pin (BL-1)**:
    /// fs_release_all_for_holder MUST invoke
    /// `cancel_all_waiters_for_holder` BEFORE `release_all_for_holder`.
    /// Reverse order (release-first) is a same-holder cross-kind
    /// admission-then-leak bug: parked wait:true range with same
    /// holder as an about-to-be-released sequential holder gets
    /// admitted by release's internal wake_waiters, then cancel finds
    /// nothing to sweep, leaking the admitted range attached to a
    /// closed cap.  Mirrors the B1 fix on WalDeployScope::drop.
    #[test]
    fn fs_release_all_for_holder_cancels_before_releases() {
        // S3.13b (2026-09-10): Lock family moved to handlers_lock.rs.
        let src = include_str!("handlers_lock.rs");
        // S3.11 (2026-09-09): after the FsHandler trait migration, the
        // live cancel/release calls now live in `impl FsHandler for
        // FsReleaseAllForHolderHandler`'s `dispatch` body.  Anchor the
        // pin on the trait impl body (the parked pre-refactor body
        // was removed at S3.12b — retirement of dead wrappers).
        let fn_start = src
            .find("impl FsHandler for FsReleaseAllForHolderHandler")
            .expect("handlers_lock.rs missing FsReleaseAllForHolderHandler trait impl");
        let window = &src[fn_start..std::cmp::min(fn_start + 3000, src.len())];
        let cancel_pos = window
            .find("cancel_all_waiters_for_holder(&holder)")
            .expect("cancel_all_waiters_for_holder call not found");
        let release_pos = window
            .find("release_all_for_holder(&holder)")
            .expect("release_all_for_holder call not found");
        assert!(
            cancel_pos < release_pos,
            "sub-6 review round-2 BL-1 regression: \
             cancel_all_waiters_for_holder MUST precede \
             release_all_for_holder — same ordering as WalDeployScope::\
             drop's B1 fix.  Reversing allows same-holder waiters to be \
             admitted-then-leaked via release's internal wake_waiters."
        );
    }

    /// DD-7b-2 (a) Option 2 (2026-08-29): `journal_write`'s
    /// Consensus branch must call `payload_source_recorder.record(...)`
    /// after computing the payload hash — this populates the
    /// `payload_hash → deploy_sig` index a joining validator's
    /// boot-time reducer walks to reproduce write bytes from
    /// block-stored deploys.  Symmetric on leader and follower
    /// (both go through this handler on their respective play/replay
    /// branches).  A refactor that dropped the recorder call would
    /// silently disable the Option 2 tier for this validator; the
    /// leader-side index would stop populating, and any joiner
    /// that hits this validator as its Option 2 source would fall
    /// back to peer fetch on every unresolved hash.
    #[test]
    fn journal_write_records_payload_source_on_consensus_writes() {
        let src = include_str!("handlers.rs");
        let fn_start = src
            .find("async fn journal_write(")
            .expect("journal_write must exist");
        // Bound to the immediate function body — under 200 lines
        // today; 8 KiB is generous.
        let end = std::cmp::min(fn_start + 8192, src.len());
        let window = &src[fn_start..end];
        assert!(
            window.contains("payload_source_recorder"),
            "journal_write must consult the payload_source_recorder slot on \
             the Consensus branch.  Dropping the call silently disables the \
             DD-7b-2 (a) Option 2 index population; joiners lose the \
             block-storage-backed reproduction tier."
        );
        assert!(
            window.contains("recorder.record("),
            "journal_write must call `recorder.record(payload_hash, &sig)` \
             after computing the write's Blake2b256 hash — this is the \
             actual index-populating call, distinct from the payload_store \
             persist step above it."
        );
        assert!(
            window.contains("current_deploy_sig"),
            "journal_write must read the WalDeployScope-plumbed \
             `current_deploy_sig` cell; without it, the recorder would be \
             called with an empty sig (skipping the record step by the \
             non-empty guard below) and the index would never populate."
        );
        assert!(
            window.contains("if !sig.is_empty()"),
            "journal_write must guard the recorder call on non-empty sig — \
             system deploys have no sig and their writes cannot be \
             reproduced via the ProcessedDeploy chain; recording under an \
             empty sig would create dead index entries `lookup_by_deploy_id` \
             never resolves."
        );
    }
}

/// H5 (coverage-review 2026-09-03): unit tests for
/// `extract_removedir_n_deleted`.  The helper is the sole cost-
/// supplement source post-DD-RemoveDirReplyShape; a silent
/// regression to always-return-0 would recreate the Oracular DoS
/// this landing exists to fix, and no integration test would
/// catch it under normal (happy-path) inputs.  These edge-case
/// pins exhaust every failure branch of the extractor + verify
/// the count-extraction is correct at every valid shape.
#[cfg(test)]
mod remove_dir_n_deleted_extractor_tests {
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::{EList, Expr};
    use models::rust::utils::{new_gbool_par, new_gint_par, new_gstring_par};
    use shared::rust::BitSet;

    use super::super::handlers_removedir::extract_removedir_n_deleted;
    use super::*;

    fn s(v: &str) -> Par { new_gstring_par(v.to_string(), Vec::new(), false) }
    fn i(n: i64) -> Par { new_gint_par(n, Vec::new(), false) }
    fn b(x: bool) -> Par { new_gbool_par(x, Vec::new(), false) }
    fn list(items: Vec<Par>) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps: items,
                locally_free: BitSet::default(),
                connective_used: false,
                remainder: None,
            })),
        }])
    }

    /// Empty Par → 0 (fail-safe on any malformed input).
    #[test]
    fn empty_par_returns_zero() {
        assert_eq!(extract_removedir_n_deleted(&Par::default()), 0);
    }

    /// Non-list Par (bare bool) → 0.
    #[test]
    fn non_list_par_returns_zero() {
        assert_eq!(extract_removedir_n_deleted(&b(true)), 0);
    }

    /// Empty list `[]` → 0 (no elements at position 1).
    #[test]
    fn empty_list_returns_zero() {
        assert_eq!(extract_removedir_n_deleted(&list(vec![])), 0);
    }

    /// `[true]` — success shape but missing count at position 1 → 0.
    #[test]
    fn success_missing_count_returns_zero() {
        assert_eq!(extract_removedir_n_deleted(&list(vec![b(true)])), 0);
    }

    /// `[true, "not an int"]` — non-Int at position 1 → 0.
    #[test]
    fn success_non_int_count_returns_zero() {
        assert_eq!(
            extract_removedir_n_deleted(&list(vec![b(true), s("not an int")])),
            0
        );
    }

    /// `[true, -5]` — negative count → 0 (fail-safe).
    #[test]
    fn success_negative_count_returns_zero() {
        assert_eq!(extract_removedir_n_deleted(&list(vec![b(true), i(-5)])), 0);
    }

    /// `[true, 5]` — non-recursive / Oracular success shape → 5.
    #[test]
    fn success_2_element_returns_count() {
        assert_eq!(extract_removedir_n_deleted(&list(vec![b(true), i(5)])), 5);
    }

    /// `[true, 5, manifest]` — Consensus recursive success shape → 5.
    /// Manifest at position 2 is ignored by this extractor.
    #[test]
    fn success_3_element_with_manifest_returns_count_only() {
        let manifest = list(vec![]);
        assert_eq!(
            extract_removedir_n_deleted(&list(vec![b(true), i(5), manifest])),
            5
        );
    }

    /// `[true, 0]` — success with zero deletions (theoretical; the
    /// leader emits 1 for non-recursive) → 0.
    #[test]
    fn success_zero_count_returns_zero() {
        assert_eq!(extract_removedir_n_deleted(&list(vec![b(true), i(0)])), 0);
    }

    /// `[false, "c", "m"]` — failure shape missing count at position
    /// 3 → 0.
    #[test]
    fn failure_missing_count_returns_zero() {
        assert_eq!(
            extract_removedir_n_deleted(&list(vec![b(false), s("c"), s("m")])),
            0
        );
    }

    /// `[false, "c", "m", 7]` — non-recursive / Oracular failure
    /// shape → 7.
    #[test]
    fn failure_4_element_returns_count() {
        assert_eq!(
            extract_removedir_n_deleted(&list(vec![b(false), s("c"), s("m"), i(7)])),
            7
        );
    }

    /// `[false, "c", "m", 7, manifest]` — Consensus recursive
    /// failure shape → 7.  Manifest at position 4 ignored.
    #[test]
    fn failure_5_element_with_manifest_returns_count_only() {
        let manifest = list(vec![]);
        assert_eq!(
            extract_removedir_n_deleted(&list(vec![b(false), s("c"), s("m"), i(7), manifest])),
            7
        );
    }

    /// `[false, "c", "m", -1]` — negative failure count → 0.
    #[test]
    fn failure_negative_count_returns_zero() {
        assert_eq!(
            extract_removedir_n_deleted(&list(vec![b(false), s("c"), s("m"), i(-1)])),
            0
        );
    }

    /// Head is neither `true` nor `false` (e.g. an Int) → 0.
    #[test]
    fn head_non_bool_returns_zero() {
        assert_eq!(extract_removedir_n_deleted(&list(vec![i(1), i(5)])), 0);
    }

    /// `[true, i64::MAX]` — upper-bound acceptance (well beyond any
    /// physical FS but exercises the u64 cast path).
    #[test]
    fn success_i64_max_returns_i64_max_as_u64() {
        assert_eq!(
            extract_removedir_n_deleted(&list(vec![b(true), i(i64::MAX)])),
            i64::MAX as u64
        );
    }
}

// -------------------------------------------------------------------
// RQ-2 review-follow-up wire-identity pins (2026-09-04).
//
// Each of the 3 wrappers introduced by RQ-2 (spawn_blocking_par +
// consensus_divergence_reply + current_deploy_scope) is a single
// source of truth for a wire-visible reply shape or a load-bearing
// invariant.  A future refactor that silently changed one wrapper's
// output (e.g., a different FSERR string, a different reply arity)
// would produce a consensus-observable byte-drift.  These pins
// exercise the wrapper via a synthetic input and compare against
// the exact byte shape the pre-RQ-2 inlined pattern would emit.
// -------------------------------------------------------------------

#[cfg(test)]
mod rq2_wrapper_pins {
    use super::*;

    /// T-20 (2026-09-11, DD-FailClosedOnInvariantBreak):
    /// `spawn_blocking_par` MUST panic (deploy abort) when the
    /// blocking task panics.  Pre-T-20 this produced an FSERR_IO
    /// reply; post-T-20 the panic propagates so the deploy scope
    /// rejects the block.
    ///
    /// The panic message carries the canonical
    /// `JOIN_ERR_ABORT_PREFIX` so operational log scanning can
    /// grep for this hazard class.  A regression that suppressed
    /// the panic or dropped the prefix would silently mask real
    /// syscall bugs — same failure mode this test guards.
    #[tokio::test]
    async fn spawn_blocking_par_panics_on_join_err_per_dd_failclosed() {
        let result = std::panic::AssertUnwindSafe(async {
            spawn_blocking_par(|| -> Par { panic!("simulated task panic") }).await
        });
        let outcome = futures::FutureExt::catch_unwind(result).await;
        let payload = outcome.expect_err(
            "T-20: spawn_blocking_par MUST panic on JoinError (deploy \
             abort), not produce an FSERR_IO reply",
        );
        let msg = payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| {
                payload
                    .downcast_ref::<&'static str>()
                    .map(|s| s.to_string())
            })
            .unwrap_or_default();
        assert!(
            msg.starts_with(crate::rust::interpreter::io::errors::JOIN_ERR_ABORT_PREFIX),
            "T-20: JoinError abort must panic with \
             `JOIN_ERR_ABORT_PREFIX` for log-scan alerting; got: {msg:?}"
        );
        assert!(
            msg.contains("simulated task panic"),
            "T-20: the underlying panic payload must be preserved \
             in the abort banner for operator triage; got: {msg:?}"
        );
    }

    /// `spawn_blocking_par`'s happy path passes the closure's Par
    /// through unchanged.  Guards against a future "sanitize the
    /// reply" refactor that would silently reshape valid returns.
    #[tokio::test]
    async fn spawn_blocking_par_happy_path_forwards_closure_par() {
        // S4.9 (2026-09-11): test-sentinel code — deliberately NOT
        // a spec-canonical FSERR_* constant.  Constructed via
        // `FserrCode(...)` at the call site to satisfy the newtype
        // gate while retaining an arbitrary sentinel for the wire-
        // drift check.
        let payload = err(FserrCode("FSERR_TEST"), "sentinel payload");
        let expected = payload.clone();
        let via_wrapper = spawn_blocking_par(move || payload).await;
        assert_eq!(
            via_wrapper, expected,
            "RQ-2 wire drift: spawn_blocking_par MUST forward the closure's Par \
             unchanged.  A regression that reshaped the reply would break every \
             non-panicking migrated call site."
        );
    }

    // T-20 (2026-09-11, DD-FailClosedOnInvariantBreak): the T-17
    // `spawn_blocking_par_with_fallback` wrapper was removed as
    // part of this slice; its 3 DD-RemoveDirReplyShape call sites
    // migrated to plain `spawn_blocking_par` (panic aborts, no
    // per-call custom fallback).  The two pre-T-20 pins for that
    // wrapper (`_panic_calls_on_join_err` +
    // `_happy_path_forwards_closure_par`) are deleted here — the
    // remaining `spawn_blocking_par_panics_on_join_err_per_dd_
    // failclosed` covers both wrappers' JoinError behaviour, and
    // the happy-path forwarding pin (`spawn_blocking_par_
    // happy_path_forwards_closure_par` below) covers the Ok arm.

    /// `consensus_divergence_reply` must produce the exact
    /// `err(FSERR_CONSENSUS_DIVERGENCE, format!("<name> follower \
    /// re-execute diverges from leader: <reason>"))` shape that the
    /// 12 pre-wrapper inline sites used.  A monitoring layer greps
    /// the message string; drift would silently break both
    /// consensus wire format AND grep-based alerting.
    #[test]
    fn consensus_divergence_reply_matches_pre_wrapper_format() {
        for handler in &["fs_read", "fs_write", "fs_chmod", "fs_remove_dir"] {
            let reason = "hash mismatch (fresh=abc123, cached=def456)";
            let via_wrapper = consensus_divergence_reply(handler, reason);
            let via_pre_wrapper = err(
                FSERR_CONSENSUS_DIVERGENCE,
                format!("{handler} follower re-execute diverges from leader: {reason}"),
            );
            assert_eq!(
                via_wrapper, via_pre_wrapper,
                "RQ-2 wire drift: consensus_divergence_reply must produce a Par \
                 byte-identical to the pre-wrapper `err(FSERR_CONSENSUS_DIVERGENCE, \
                 format!(...))` shape at every one of the 12 migrated sites.  \
                 handler={handler}, reason={reason}",
            );
        }
    }
}
