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

use std::path::PathBuf;

use models::rhoapi::Par;
use tokio::task::spawn_blocking;

use super::super::contract_call::ContractCall;
use super::super::dispatch::RhoDispatch;
use super::super::metering::MeteredMachine;
use super::super::rho_runtime::RhoISpace;
use super::super::rho_type::{RhoBoolean, RhoString};
use super::errors::{poison_abort, *};
use super::handle_table::{FileHandle, FileHandleTable};
// S3.13b (2026-09-10): all handler-framework imports migrated out
// with the family split.  The trait-exempt `fs_remove_dir` body is
// self-contained (no trait framework references).
use super::lock::{HolderId, LockError, LockMode};
use super::mode::{fopen_flags, parse_open_mode, AccessMode};
use super::path::{
    canonicalize_lexical, io_msg_scrub, quarantine_err_reply, safe_descend_verified, SafeParent,
};
use super::response::*;
use super::stat::{error_record, stat_record};
use super::wal::{PayloadRef, WalEntry, WalOp, WalOutcome};
use super::{ConsensusMode, CMODE_CONSENSUS_STR, CMODE_ORACULAR_STR};

/// Slice 30c H-R3 integration: compute the ack channel's Blake2b256
/// hash the same way rspace computes `channel_hash` for a produce
/// event.  The result is the sidecar key on `Wal::append_with_ack`;
/// the same hash appears in the deploy_log's `ProduceEvent::channels_hash`
/// when the handler publishes its reply, so the log-order drain
/// can match them.
pub(super) fn ack_channel_hash(ack: &Par) -> [u8; 32] {
    let h = rspace_plus_plus::rspace::hashing::stable_hash_provider::hash(ack).bytes();
    // M-9 fix (2026-08-06): fail-hard in release, not just debug.
    // Pre-fix used `debug_assert_eq!` + `.min(32)` which would
    // silently zero-pad in release if a future Blake2b256
    // provider swap produced shorter output — hash collisions on
    // the placeholder sentinel `[0u8; 32]` would misroute
    // log-order drain.  Panicking loudly at the mismatch site
    // surfaces the misconfiguration at first call rather than
    // as a downstream consensus divergence.
    assert_eq!(
        h.len(),
        32,
        "Blake2b256 must produce 32-byte digest; got {} — the WAL ack sidecar \
         hard-depends on a fixed 32-byte hash width",
        h.len()
    );
    let mut out = [0u8; 32];
    out.copy_from_slice(&h);
    out
}

/// H-29-3 lift slice 2 (2026-08-26): derive a unique per-entry ack
/// hash for a recursive-removeDir manifest entry.  Rholang callers
/// see one ack channel for the whole `fsRemoveDir!(...)` call, but
/// the leader and follower each append MANY WAL entries (one per
/// tree leaf).  To keep the WAL's `ack_hashes` sidecar meaningful
/// (log-order drain, per-entry outcome finalize), each entry needs
/// its own sidecar key.  Both sides derive the same key from the
/// shared ack channel + entry's canonical path:
///
/// ```text
/// Blake2b256(ack_channel_hash(ack) || 0xFE || path_bytes)
/// ```
///
/// The `0xFE` separator prevents accidental collision with the
/// standard `ack_channel_hash(ack)` used for single-entry ops
/// (which never sees a path-suffix domain byte).  Determinism is
/// symmetric: leader and follower see the same ack Par (via rig
/// replay) and the same canonical path (via the reply-manifest
/// lookup + `canonicalize_lexical`).
pub(super) fn per_entry_ack_seed(ack: &Par, path: &std::path::Path) -> [u8; 32] {
    let base = ack_channel_hash(ack);
    let path_bytes = path.as_os_str().as_encoded_bytes();
    let mut buf = Vec::with_capacity(base.len() + 1 + path_bytes.len());
    buf.extend_from_slice(&base);
    buf.push(0xFE);
    buf.extend_from_slice(path_bytes);
    let h = crypto::rust::hash::blake2b256::Blake2b256::hash(buf);
    assert_eq!(h.len(), 32, "Blake2b256 must produce 32-byte digest");
    let mut out = [0u8; 32];
    out.copy_from_slice(&h);
    out
}

/// RQ-2 (2026-09-04) centralized wrapper for the
/// `spawn_blocking(|| -> Par { ... }).await.unwrap_or_else(|_je|
/// err(FSERR_IO, "spawn_blocking task failed"))` pattern that
/// appears at 10+ handler sites.
///
/// T-20 (2026-09-11, DD-FailClosedOnInvariantBreak): JoinError arm
/// changed from returning `err(FSERR_IO, ...)` (soft reply that
/// burned budget but masked the underlying bug) to `join_err_abort`
/// which panics with `JOIN_ERR_ABORT_PREFIX`, unwinding through
/// the async runtime to the deploy scope which rejects the block.
/// See `design-decisions.md § DD-FailClosedOnInvariantBreak`.
///
/// X-2 / G-01 (2026-09-11, branch-review-2026-09-11.md): the
/// `spawn_blocking` JoinHandle await is wrapped in
/// `park_external_during` so the current reduction participant is
/// removed from the session's participant set while the syscall
/// runs on the blocking pool.  Without this, a two-deploy scenario
/// (D1 in `spawn_blocking(safe_open_verified(...))`, D2 doing an
/// RSpace consume) deadlocks: D1's participant stays `Running`, so
/// `frontier_ready()` sees a mix of `Running` + `Waiting` and never
/// fires the driver — but the driver is the only thing that would
/// service D2's admit, and D1's spawn_blocking never completes
/// until the driver advances.  The park_external_during Drop guard
/// unparks on both happy path and panic unwind (T-20's
/// `join_err_abort` panic path).
///
/// Only wraps the "closure returns Par directly" pattern.  Sites
/// where the closure returns `Result<T, E>` and the outer match
/// dispatches on both variants stay inline — the extraction would
/// force awkward generic-over-Result-arm parameterization for
/// negligible LOC savings.  Those inline sites use the same
/// `join_err_abort` helper on the `Err(_)` arm AND the same
/// `park_external_during` wrapper on the `spawn_blocking(...).await`.
///
/// The pre-T-20 `spawn_blocking_par_with_fallback` companion (which
/// let callers produce a custom Par on JoinError, e.g.
/// DD-RemoveDirReplyShape'd early-errors) was removed as part of
/// this slice — any panic reaches abort regardless of caller
/// shape.
// S3.13b (2026-09-10) — `pub(super)` for sibling family modules
// (`handlers_stream.rs` etc.) that share this spawn_blocking wrapper.
pub(super) async fn spawn_blocking_par<F>(f: F) -> Par
where F: FnOnce() -> Par + Send + 'static {
    match crate::rust::interpreter::deterministic_reduction::park_external_during(spawn_blocking(f))
        .await
    {
        Ok(par) => par,
        Err(je) => super::errors::join_err_abort(je),
    }
}

/// RQ-2 (2026-09-04) centralized builder for the Consensus-mode
/// follower-side "reply-hash mismatch" divergence reply.  Handlers
/// that verify their fresh syscall reply against the leader's
/// cached reply via `verify_reply_hash_matches_cached` produce this
/// reply on the `Err(reason)` arm; it carries FSERR_CONSENSUS_DIVERGENCE
/// (code 13, § docs/consensus-invariants.md § FSERR_CODE_*) and a
/// stable message shape that a monitoring layer can grep against.
///
/// Only wraps the `err(FSERR_CONSENSUS_DIVERGENCE, ...)` shape (12
/// call sites).  Sites that need `err_with_count` /
/// `err_with_manifest` for DD-RemoveDirReplyShape stay inline —
/// their reply shape carries the pre-divergence deletion count /
/// manifest that a caller downstream might inspect.
pub(super) fn consensus_divergence_reply(
    handler_name: &str,
    reason: impl std::fmt::Display,
) -> Par {
    err(
        FSERR_CONSENSUS_DIVERGENCE,
        format!("{handler_name} follower re-execute diverges from leader: {reason}"),
    )
}

/// TOCTOU-immune unlink of a manifest entry via openat chain from
/// a pinned `SafeParent` (2026-09-02, post-security-review S-1).
///
/// The recursive Consensus removeDir walker produces a manifest of
/// `(rel_path, kind)` tuples sorted post-order (children before
/// their containing directory).  Applying the manifest correctly
/// requires the unlink to happen against the pinned dirfd chain
/// obtained by descending from `parent`, not against an absolute
/// path resolved by the kernel from cwd — otherwise a swap of any
/// intermediate directory component between manifest-collection
/// and unlink would land the syscall on attacker-controlled bytes.
///
/// This mirrors the Oracular `remove_dir_recursive`'s
/// `openat`/`unlinkat` chained descent.  Under Shape A + D3 the
/// caller's manifest is derived from a walk of the target subtree,
/// so intermediate directories still exist at unlink time and the
/// descent chain always resolves.
///
/// Empty `rel_path` unlinks the target itself
/// (`unlinkat(parent.as_raw_fd(), parent.leaf_ptr(), flags)`).
/// Non-empty `rel_path`:
///   1. `openat(parent.as_raw_fd(), parent.leaf_ptr(), O_DIRECTORY|
///      O_NOFOLLOW|O_CLOEXEC)` to pin the target dirfd.
///   2. For each intermediate component: `openat(cur_fd, component,
///      O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC)`.
///   3. `unlinkat(last_intermediate_fd, leaf, kind_flags)`.
///
/// Each intermediate `OwnedFd` closes on scope exit via Drop.
///
/// T-10 (Mi-9, 2026-09-08): shared leaf-unlink helper used by
/// `fs_remove_file`, non-recursive `fs_remove_dir` (oracular +
/// consensus), and the empty-rel branch of `unlink_manifest_entry`.
///
/// Pre-fix, three call sites open-coded the same pattern:
///     let rc = unsafe { libc::unlinkat(parent.as_raw_fd(),
///                                       parent.leaf_ptr(), flags) };
///     if rc == 0 { Ok(()) } else { Err(last_os_error()) }
/// with a hand-picked `flags` argument.  Extracting the helper
/// documents the SAFETY invariant once and lets callers pass a
/// `RemoveKind` rather than a raw libc flag.
///
/// See `unlink_manifest_entry` for the multi-component variant.
pub(super) fn unlink_leaf_via_dirfd(parent: &SafeParent, kind: RemoveKind) -> std::io::Result<()> {
    let flags = match kind {
        RemoveKind::File => 0,
        RemoveKind::Dir => libc::AT_REMOVEDIR,
    };
    // SAFETY: `parent.as_raw_fd()` is an open dirfd owned by the
    // caller's `SafeParent`; `parent.leaf_ptr()` is a NUL-
    // terminated CString buffer owned by the same `SafeParent`
    // and outlives this call.  `unlinkat` reads both and does not
    // retain either past return.
    let rc = unsafe { libc::unlinkat(parent.as_raw_fd(), parent.leaf_ptr(), flags) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Cap on `fs_entries` output size — prevents a malicious caller pointing
/// the native at a million-entry directory and OOMing the node.
pub const MAX_ENTRIES: usize = 65_536;

/// Cap on `fs_write` payload — symmetric with `MAX_READ_BYTES`.
pub const MAX_WRITE_BYTES: u64 = 64 * 1024 * 1024;

// M-35 (2026-09-08, A4-S-3): register handlers.rs consensus constants
// with the fingerprint fold.  Orders 2 and 5 — preserved from the
// pre-M-35 manual fold order to avoid rolling the fingerprint golden
// hex.
crate::register_consensus_constant!(order = 2, name = MAX_WRITE_BYTES, u64_be);
crate::register_consensus_constant!(order = 5, name = MAX_ENTRIES, u64_be);

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

/// Resolve the Rholang-supplied cmode string into a `ConsensusMode`.
/// Slice 26 + C-26-F1 review fix: `fs_stat` / `fs_entries` / `fs_chown`
/// now take the per-cap consensus mode as a REQUIRED positional arg
/// (the library agents peek their `cmodeP` cell and forward the exact
/// string).  Any Par shape other than `String("oracular")` /
/// `String("consensus")` returns `None`; callers surface
/// `FSERR_BAD_ARG` and refuse to proceed.
///
/// Fail-closed rationale: the pre-fix behavior silently defaulted to
/// `self.mode` (Oracular by default), which is a downgrade for
/// Consensus caps.  Under the URN-filter gap (MVP #5 / slice 31) a
/// user deploy could omit the cmode entirely and get chown / host
/// metadata for free.  Under FIPS threat modeling, a per-cap mode arg
/// that fails to parse must reject the call, not silently pick the
/// weaker mode.
///
/// String constants live in `io/mod.rs` (`CMODE_ORACULAR_STR` /
/// `CMODE_CONSENSUS_STR`) and are re-exported by
/// `casper::genesis::contracts::fs_genesis::BundleConsensusMode` so the
/// composer and the resolver agree byte-for-byte.  A drift-assertion
/// test in `fs_genesis.rs` pins the pair.
pub(super) fn resolve_cmode(par: &Par) -> Option<ConsensusMode> {
    match RhoString::unapply(par).as_deref() {
        Some(s) if s == CMODE_CONSENSUS_STR => Some(ConsensusMode::Consensus),
        Some(s) if s == CMODE_ORACULAR_STR => Some(ConsensusMode::Oracular),
        _ => None,
    }
}

/// Phase 8 slice 8a — parse the Rholang-supplied lock-mode string
/// into a `LockMode`.  Accepts exactly `"r"` and `"w"` per FIP
/// §Explicit locks; any other shape returns `None` and the caller
/// surfaces `FSERR_BAD_ARG`.  Fail-closed mirrors `resolve_cmode`.
pub(super) fn resolve_lock_mode(par: &Par) -> Option<LockMode> {
    match RhoString::unapply(par).as_deref() {
        Some("r") => Some(LockMode::Read),
        Some("w") => Some(LockMode::Write),
        _ => None,
    }
}

/// Phase 8 slice 8a — derive a stable 32-byte `HolderId` from an
/// opaque Rholang Par (per convention: the caller-cap's per-instance
/// `this` GPrivate
/// name).  Uses the same Blake2b256 stable-hash provider that rspace
/// uses for channel identity, so equal-Par callers hash to the same
/// bytes across runtimes deterministically.
pub(super) fn holder_id_of(par: &Par) -> HolderId {
    let h = rspace_plus_plus::rspace::hashing::stable_hash_provider::hash(par).bytes();
    // Same 32-byte hard-dep as `ack_channel_hash`.  A Blake2b256
    // provider swap producing shorter output would silently
    // collide HolderIds; fail loudly at first call instead.
    assert_eq!(
        h.len(),
        32,
        "Blake2b256 must produce 32-byte digest; got {} — HolderId hard-depends on \
         a fixed 32-byte hash width",
        h.len()
    );
    let mut out = [0u8; 32];
    out.copy_from_slice(&h);
    HolderId::from_bytes(out)
}

/// Phase 8 slice 8a — map a `LockError` from the LockRegistry to
/// the FSERR reply shape.
/// Free-function form of `FsProcesses::journal_state_read` for
/// wave-3 trait impls to call via `ctx.handles.wal`.  Behavior
/// identical to the `&self` wrapper — same no-op-on-Oracular
/// short-circuit, same reply-hash + outcome encoding, same
/// `Wal::append_with_ack` key.
pub(super) fn journal_state_read_via_table(
    handles: &FileHandleTable,
    cmode: ConsensusMode,
    op: WalOp,
    path: PathBuf,
    reply: &Par,
    ack: &Par,
    length: Option<u64>,
) {
    if cmode != ConsensusMode::Consensus {
        return;
    }
    let reply_hash: [u8; 32] = {
        let h = rspace_plus_plus::rspace::hashing::stable_hash_provider::hash(reply).bytes();
        assert_eq!(
            h.len(),
            32,
            "M-5: stable_hash must produce a 32-byte Blake2b256"
        );
        let mut buf = [0u8; 32];
        buf.copy_from_slice(&h);
        buf
    };
    let outcome = if let Some(code_str) = extract_err_code(std::slice::from_ref(reply)) {
        WalOutcome::Failure {
            code: fserr_to_code(&code_str),
        }
    } else {
        WalOutcome::Success
    };
    let _ = handles.wal.append_with_ack(
        WalEntry {
            op,
            path,
            extra_path: None,
            offset: None,
            length,
            payload_ref: Some(PayloadRef::Hash(reply_hash)),
            mode_bits: None,
            owner: None,
            group: None,
            outcome,
        },
        ack_channel_hash(ack),
    );
}

/// Free-function form of `FsProcesses::journal_write` for wave-3
/// trait impls (FsWriteHandler, FsWriteAtHandler).  Semantic
/// behavior identical to the `&self` wrapper.  Fd-based cmode
/// lookup; self-guards on Oracular.  Persists payload to the
/// payload store + records payload_source_index BEFORE appending
/// the WAL entry.
pub(super) async fn journal_write_via_table(
    handles: &FileHandleTable,
    fd: u64,
    bytes: &[u8],
    offset: Option<u64>,
    ack: &Par,
) -> Result<bool, ()> {
    let wal_meta = handles
        .with_mut(fd, |h| (h.cmode, h.canon_path.clone(), h.position))
        .await;
    match wal_meta {
        Some((ConsensusMode::Consensus, canon_path, position)) => {
            let (op, resolved_offset) = match offset {
                Some(off) => (WalOp::WriteAt, Some(off)),
                None => (WalOp::Write, Some(position)),
            };
            // Phase 7b-2: persist payload BEFORE WAL append so a
            // joining validator's fetch protocol sees the bytes as
            // soon as the WAL entry lands.  Fail-open — log warn.
            if let Some(store) = handles.payload_store() {
                if let Err(e) = store.persist(bytes) {
                    tracing::warn!(
                        target: "f1r3fly.fs_wal.payload_store",
                        error = %e,
                        "payload store persist failed on Consensus write; \
                         joiners will need to fetch from another peer"
                    );
                }
            }
            let PayloadRef::Hash(payload_hash) = PayloadRef::hash(bytes) else {
                unreachable!("PayloadRef::hash always returns Hash variant")
            };
            // DD-7b-2 Option 2: record payload_hash → deploy_sig
            // mapping.  Fail-open.
            if let Some(recorder) = handles.payload_source_recorder() {
                let sig =
                    poison_abort(handles.current_deploy_sig.read(), "current_deploy_sig").clone();
                if !sig.is_empty() {
                    if let Err(e) = recorder.record(payload_hash, &sig) {
                        tracing::warn!(
                            target: "f1r3fly.fs_wal.payload_source_index",
                            error = %e,
                            "payload_source recorder record failed on \
                             Consensus write; joiners will need to fall back \
                             to peer fetch for this payload hash"
                        );
                    }
                }
            }
            handles
                .wal
                .append_with_ack(
                    WalEntry {
                        op,
                        path: canon_path,
                        extra_path: None,
                        offset: resolved_offset,
                        length: Some(bytes.len() as u64),
                        payload_ref: Some(PayloadRef::Hash(payload_hash)),
                        mode_bits: None,
                        owner: None,
                        group: None,
                        outcome: WalOutcome::Success,
                    },
                    ack_channel_hash(ack),
                )
                .map(|()| true)
        }
        _ => Ok(false),
    }
}

/// Free-function form of `FsProcesses::finalize_write_journal` for
/// wave-3 trait impls.  Patches the pre-appended WAL entry's
/// `length` + `payload_ref` to reflect actual bytes written on a
/// partial write.  Re-persists the truncated slice to the payload
/// store (fail-open).
pub(super) fn finalize_write_journal_via_table(
    handles: &FileHandleTable,
    requested_bytes: &[u8],
    actual_n: u64,
    ack: &Par,
) {
    let n = (actual_n as usize).min(requested_bytes.len());
    let actual_slice = &requested_bytes[..n];
    if let Some(store) = handles.payload_store() {
        if let Err(e) = store.persist(actual_slice) {
            tracing::warn!(
                target: "f1r3fly.fs_wal.payload_store",
                error = %e,
                "payload store persist failed on partial-write finalize"
            );
        }
    }
    let _ = handles
        .wal
        .update_partial_write_by_ack_hash(ack_channel_hash(ack), actual_slice);
}

/// Free-function form of `FsProcesses::write_impl` for wave-3
/// trait impls.  spawn_blocking libc::write (offset=None) or
/// libc::pwrite (offset=Some).  MAX_WRITE_BYTES gate at entry.
pub(super) async fn write_impl_via_table(
    handles: &FileHandleTable,
    fd: u64,
    bytes: Vec<u8>,
    offset: Option<u64>,
) -> Par {
    if bytes.len() as u64 > MAX_WRITE_BYTES {
        return err(
            FSERR_QUOTA_EXCEEDED,
            format!("write {} exceeds MAX_WRITE_BYTES", bytes.len()),
        );
    }
    let file_arc = match handles.raw_fd(fd).await {
        Some(f) => f,
        None => return err(FSERR_CLOSED, format!("unknown fd {fd}")),
    };
    // X-2 / G-01: park_external_during so the reduction driver can
    // advance other participants while this syscall runs on the
    // blocking pool.
    let result = crate::rust::interpreter::deterministic_reduction::park_external_during(
        spawn_blocking(move || {
            use std::os::fd::AsRawFd;
            let raw_fd = file_arc.as_raw_fd();
            // SAFETY: `file_arc` (an `Arc<File>`) is moved into this
            // closure and keeps `raw_fd` open for the duration.
            // `bytes` is a live `Vec<u8>` owned by this scope.
            // pwrite/write read up to `bytes.len()` bytes from the
            // pointer and return the count written (or -1 on error).
            let n = unsafe {
                if let Some(off) = offset {
                    libc::pwrite(raw_fd, bytes.as_ptr() as *const _, bytes.len(), off as i64)
                } else {
                    libc::write(raw_fd, bytes.as_ptr() as *const _, bytes.len())
                }
            };
            if n < 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(n as u64)
            }
        }),
    )
    .await;
    match result {
        Err(je) => crate::rust::interpreter::io::errors::join_err_abort(je),
        Ok(Err(e)) => err(io_err_code(&e), io_msg_scrub(&e)),
        Ok(Ok(n)) => ok_u64(n),
    }
}

/// Free-function form of `FsProcesses::finalize_failure_journal`
/// for wave-3 trait impls (S3.7+).  Patches the pre-appended
/// WAL entry's outcome from Success placeholder to
/// `Failure { code }`.  See journal_truncate / journal_path_
/// mutation_single for the pre-append side.
pub(super) fn finalize_failure_journal_via_table(handles: &FileHandleTable, code: u32, ack: &Par) {
    let _ = handles
        .wal
        .update_outcome_by_ack_hash(ack_channel_hash(ack), WalOutcome::Failure { code });
}

/// Free-function form of `FsProcesses::journal_truncate` for wave-3
/// trait impls (S3.7+).  Fd-based cmode lookup; self-guards on
/// Oracular.  Returns `Err(())` on WAL-cap exhaustion.
pub(super) async fn journal_truncate_via_table(
    handles: &FileHandleTable,
    fd: u64,
    n: u64,
    ack: &Par,
) -> Result<bool, ()> {
    let wal_meta = handles
        .with_mut(fd, |h| (h.cmode, h.canon_path.clone()))
        .await;
    match wal_meta {
        Some((ConsensusMode::Consensus, canon_path)) => handles
            .wal
            .append_with_ack(
                WalEntry {
                    op: WalOp::Truncate,
                    path: canon_path,
                    extra_path: None,
                    offset: Some(n),
                    length: None,
                    payload_ref: None,
                    mode_bits: None,
                    owner: None,
                    group: None,
                    outcome: WalOutcome::Success,
                },
                ack_channel_hash(ack),
            )
            .map(|()| true),
        _ => Ok(false),
    }
}

/// Free-function form of `FsProcesses::journal_path_mutation_two`
/// for wave-3 trait impls (S3.9+).  Two-endpoint mutation
/// (Rename, CopyFile) — `from_canon_path` goes in `path`,
/// `to_canon_path` in `extra_path`.  Cmode arg, self-guards on
/// Oracular.
#[allow(clippy::result_unit_err)]
pub(super) async fn journal_path_mutation_two_via_table(
    handles: &FileHandleTable,
    cmode: ConsensusMode,
    op: WalOp,
    from_canon_path: PathBuf,
    to_canon_path: PathBuf,
    ack: &Par,
) -> Result<bool, ()> {
    if cmode != ConsensusMode::Consensus {
        return Ok(false);
    }
    handles
        .wal
        .append_with_ack(
            WalEntry {
                op,
                path: from_canon_path,
                extra_path: Some(to_canon_path),
                offset: None,
                length: None,
                payload_ref: None,
                mode_bits: None,
                owner: None,
                group: None,
                outcome: WalOutcome::Success,
            },
            ack_channel_hash(ack),
        )
        .map(|()| true)
}

/// Free-function form of `FsProcesses::journal_path_mutation_single`
/// for wave-3 trait impls (S3.7+).  Cmode passed by arg (not fd-
/// based).  Self-guards on Oracular.  Returns `Err(())` on WAL-cap
/// exhaustion.
#[allow(clippy::result_unit_err, clippy::too_many_arguments)]
pub(super) async fn journal_path_mutation_single_via_table(
    handles: &FileHandleTable,
    cmode: ConsensusMode,
    op: WalOp,
    canon_path: PathBuf,
    mode_bits: Option<u32>,
    owner: Option<String>,
    group: Option<String>,
    ack: &Par,
) -> Result<bool, ()> {
    if cmode != ConsensusMode::Consensus {
        return Ok(false);
    }
    handles
        .wal
        .append_with_ack(
            WalEntry {
                op,
                path: canon_path,
                extra_path: None,
                offset: None,
                length: None,
                payload_ref: None,
                mode_bits,
                owner,
                group,
                outcome: WalOutcome::Success,
            },
            ack_channel_hash(ack),
        )
        .map(|()| true)
}

/// Free-function form of `FsProcesses::open_impl` for wave-3
/// FsOpenHandler.  Semantic behavior identical to the `&self`
/// wrapper: parse mode → reject Consensus+O_APPEND → resolve
/// or_identity → safe_open_verified → fstat regular-file check →
/// insert FileHandle → emit ok_fd or err.
pub(super) async fn open_impl_via_table(
    handles: &FileHandleTable,
    root: String,
    rel: String,
    mode: String,
    cmode: ConsensusMode,
) -> Par {
    let intent = match parse_open_mode(&mode) {
        Some(i) => i,
        None => return err(FSERR_BAD_ARG, format!("unknown fopen mode {mode:?}")),
    };
    // Consensus + O_APPEND is rejected: append writes get atomically
    // retargeted to file-end by the kernel; the shadow-position model
    // doesn't extend cleanly.  Rather than ship a WAL followers can't
    // replay, reject at open.
    if cmode == ConsensusMode::Consensus && intent.append {
        return err(
            FSERR_BAD_ARG,
            "append modes (\"a\", \"a+\") are not supported on Consensus caps — \
             use a non-append mode plus fs_seek(SEEK_END) if append semantics \
             are required, or open the cap as Oracular",
        );
    }
    let root_pb = PathBuf::from(&root);
    // Shape A: resolve legacy Oracular through identity fall-through,
    // Consensus through per-runtime RootIdentityRegistry to the
    // validator's on-disk staging dir + boot-captured (dev, inode).
    // X-6c M-04: gated for Consensus to refuse dispatch on unregistered
    // logical roots — fs_open is the entry point for every fd-based
    // Consensus session, so the guard here inherits down to all
    // fd-based ops that follow.
    let (root_pb, expected_root_id) = match handles
        .root_registry
        .resolve_or_identity_gated_for_consensus(&root_pb, cmode)
    {
        Ok(v) => v,
        Err((c, m)) => return err(c, m),
    };
    let intent_copy = intent;
    let rel_for_open = rel.clone();
    // X-2 / G-01: park_external_during so the reduction driver can
    // advance other participants while safe_open_verified runs on
    // the blocking pool.
    let opened = crate::rust::interpreter::deterministic_reduction::park_external_during(
        spawn_blocking(move || {
            let (flags, mode_bits) = fopen_flags(intent_copy);
            super::path::safe_open_verified(
                &root_pb,
                &rel_for_open,
                flags,
                mode_bits,
                expected_root_id,
            )
        }),
    )
    .await;
    let file = match opened {
        Err(je) => crate::rust::interpreter::io::errors::join_err_abort(je),
        Ok(Err(qe)) => {
            let (code, msg) = quarantine_err_reply(&qe);
            return err(code, msg);
        }
        Ok(Ok(f)) => f,
    };
    // Non-regular-file rejection.  Since we already have the fd
    // (opened with O_NOFOLLOW), there's no TOCTOU here.
    let meta = match file.metadata() {
        Ok(m) => m,
        Err(e) => return err(io_err_code(&e), io_msg_scrub(&e)),
    };
    if !meta.file_type().is_file() {
        return err(FSERR_UNSUPPORTED, "not a regular file");
    }
    let deploy = *poison_abort(
        handles.current_deploy_scope.read(),
        "current_deploy_scope RwLock",
    );
    let handle = FileHandle {
        file: Some(std::sync::Arc::new(file)),
        canon_path: canonicalize_lexical(&root, &rel),
        mode: intent.mode,
        cmode,
        position: 0,
        deploy,
    };
    match handles.insert(handle).await {
        Ok(fd) => ok_fd(Fd::from(fd)),
        Err(()) => err(FSERR_QUOTA_EXCEEDED, "per-runtime fd cap reached"),
    }
}

/// Free-function form of `FsProcesses::read_impl` for wave-3 trait
/// impls (FsReadHandler, FsReadAtHandler).  Semantic behavior
/// identical to the `&self` wrapper.
pub(super) async fn read_impl_via_table(
    handles: &FileHandleTable,
    fd: u64,
    n: u64,
    offset: Option<u64>,
) -> Par {
    if n > super::MAX_READ_BYTES {
        return err(
            FSERR_QUOTA_EXCEEDED,
            format!("read {n} exceeds MAX_READ_BYTES"),
        );
    }
    let file_arc = match handles.raw_fd(fd).await {
        Some(f) => f,
        None => return err(FSERR_CLOSED, format!("unknown fd {fd}")),
    };
    // X-2 / G-01: park_external_during so the reduction driver can
    // advance other participants while pread/read runs on the
    // blocking pool.
    let result = crate::rust::interpreter::deterministic_reduction::park_external_during(
        spawn_blocking(move || {
            use std::os::fd::AsRawFd;
            let raw_fd = file_arc.as_raw_fd();
            let mut buf = vec![0u8; n as usize];
            // SAFETY: `file_arc` (an `Arc<File>`) is moved into this
            // closure and keeps `raw_fd` open for the duration.  `buf`
            // is a live heap allocation of `n` bytes owned by this
            // scope.  pread/read write into `buf` up to `n` bytes and
            // return the count written (or -1 on error); we truncate
            // `buf` to the returned length.
            let got = unsafe {
                if let Some(off) = offset {
                    libc::pread(raw_fd, buf.as_mut_ptr() as *mut _, n as usize, off as i64)
                } else {
                    libc::read(raw_fd, buf.as_mut_ptr() as *mut _, n as usize)
                }
            };
            if got < 0 {
                Err(std::io::Error::last_os_error())
            } else {
                buf.truncate(got as usize);
                Ok(buf)
            }
        }),
    )
    .await;
    match result {
        Err(je) => crate::rust::interpreter::io::errors::join_err_abort(je),
        Ok(Err(e)) => err(io_err_code(&e), io_msg_scrub(&e)),
        Ok(Ok(bytes)) => ok_bytes(bytes),
    }
}

/// Free-function form of `FsProcesses::journal_read` for wave-3
/// trait impls.  Semantic behavior identical to the `&self` wrapper.
pub(super) async fn journal_read_via_table(
    handles: &FileHandleTable,
    fd: u64,
    bytes: &[u8],
    offset: Option<u64>,
    ack: &Par,
) -> Result<bool, ()> {
    let wal_meta = handles
        .with_mut(fd, |h| (h.cmode, h.canon_path.clone(), h.position))
        .await;
    match wal_meta {
        Some((ConsensusMode::Consensus, canon_path, position)) => {
            let (op, resolved_offset) = match offset {
                Some(off) => (WalOp::ReadAt, Some(off)),
                None => (WalOp::Read, Some(position)),
            };
            handles
                .wal
                .append_with_ack(
                    WalEntry {
                        op,
                        path: canon_path,
                        extra_path: None,
                        offset: resolved_offset,
                        length: Some(bytes.len() as u64),
                        payload_ref: Some(PayloadRef::hash(bytes)),
                        mode_bits: None,
                        owner: None,
                        group: None,
                        outcome: WalOutcome::Success,
                    },
                    ack_channel_hash(ack),
                )
                .map(|()| true)
        }
        _ => Ok(false),
    }
}

/// Free-function form of `FsProcesses::journal_read_divergence`
/// for wave-3 trait impls.
pub(super) async fn journal_read_divergence_via_table(
    handles: &FileHandleTable,
    fd: u64,
    offset: Option<u64>,
    ack: &Par,
) -> bool {
    let wal_meta = handles
        .with_mut(fd, |h| (h.cmode, h.canon_path.clone()))
        .await;
    match wal_meta {
        Some((ConsensusMode::Consensus, canon_path)) => {
            let op = match offset {
                Some(_) => WalOp::ReadAt,
                None => WalOp::Read,
            };
            let _ = handles.wal.append_with_ack(
                WalEntry {
                    op,
                    path: canon_path,
                    extra_path: None,
                    offset,
                    length: None,
                    payload_ref: None,
                    mode_bits: None,
                    owner: None,
                    group: None,
                    outcome: WalOutcome::Failure {
                        code: fserr_to_code(FSERR_CONSENSUS_DIVERGENCE.as_str()),
                    },
                },
                ack_channel_hash(ack),
            );
            true
        }
        _ => false,
    }
}

/// Free-function form of the pre-wave-3 `FsProcesses::dev_inode_from_fd`
/// method.  Takes `&FileHandleTable` directly so both the
/// `FsProcesses::fs_lock_*` wrappers (retiring at S3.12) and the
/// wave-3 `FsLockRangeHandler` / `FsLockSequentialHandler` trait
/// impls can share one implementation via `SyscallCtx::handles`.
///
/// Semantic behavior identical to the pre-wave-3 method — see its
/// docstring at the (now-thin) `FsProcesses::dev_inode_from_fd`
/// wrapper for the concurrent-close race analysis.
pub(super) async fn dev_inode_from_fd_via_table(
    handles: &FileHandleTable,
    fd: u64,
) -> Result<(u64, u64), (super::errors::FserrCode, String)> {
    let Some(file_arc) = handles.raw_fd(fd).await else {
        return Err((FSERR_CLOSED, "fd unknown or shadow handle".to_string()));
    };
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        let raw = file_arc.as_raw_fd();
        // SAFETY: `file_arc` (an `Arc<File>`) keeps the underlying
        // fd open for the lifetime of this scope, so `raw` is a
        // valid open fd.  `libc::stat` is POD — zeroed init is a
        // valid bit pattern.  fstat writes into `st` and doesn't
        // retain either pointer.
        unsafe {
            let mut st: libc::stat = std::mem::zeroed();
            if libc::fstat(raw, &mut st) < 0 {
                let e = std::io::Error::last_os_error();
                return Err((FSERR_IO, io_msg_scrub(&e)));
            }
            #[allow(clippy::unnecessary_cast)]
            Ok((st.st_dev as u64, st.st_ino as u64))
        }
    }
    #[cfg(not(unix))]
    {
        let _ = file_arc;
        Ok((0, 0))
    }
}

pub(super) fn lock_err_reply(le: LockError) -> Par {
    match le {
        LockError::Busy => err(FSERR_BUSY, "range lock unavailable"),
        LockError::Closed => err(FSERR_CLOSED, "lock id not held"),
        LockError::BadArg => err(FSERR_BAD_ARG, "invalid lock argument"),
        LockError::QuotaExceeded => err(
            FSERR_QUOTA_EXCEEDED,
            "range-lock cap exceeded for this file",
        ),
        // Slice-8b: `wait: true` acquire cancelled while parked
        // (explicit cancel_wait / deploy-end sweep / registry drop).
        // Sub-2 additionally synthesizes a Produce::with_error() for
        // deterministic replay per plan §X-2; the string reply body
        // still maps through this helper.
        LockError::Cancelled => err(FSERR_CANCELLED, "wait:true lock acquisition cancelled"),
        // NB-7 (2026-09-02): cross-deploy mutual-wait deadlock
        // detected at enqueue time — the requested wait:true acquire
        // would close a cycle in the cross-deploy wait-for graph, so
        // it is refused eagerly without allocating a Waiter struct.
        LockError::Deadlock => err(
            FSERR_DEADLOCK,
            "wait:true lock acquisition would close cross-deploy wait-for cycle",
        ),
    }
}

pub(super) fn leaf_of(rel: &str) -> String {
    std::path::Path::new(rel)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| rel.to_string())
}

/// Fetch a `std::fs::Metadata` for the leaf named by `parent`.  Opens
/// the leaf via openat + O_NOFOLLOW off the parent dirfd, then reads
/// metadata.  A symlink leaf yields `ELOOP` — the caller decides how to
/// surface that.
pub(super) fn fstatat_meta(parent: &SafeParent) -> std::io::Result<std::fs::Metadata> {
    use std::os::fd::FromRawFd;
    // SAFETY: `parent` (a `SafeParent`) owns an open dirfd for its
    // lifetime, and `parent.leaf_ptr()` is a NUL-terminated CString
    // ptr owned by the same `SafeParent`.  On openat success,
    // `File::from_raw_fd` takes ownership of the fresh fd so Drop
    // closes it on every exit path.
    unsafe {
        let fd = libc::openat(
            parent.as_raw_fd(),
            parent.leaf_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        );
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let file = std::fs::File::from_raw_fd(fd);
        file.metadata()
    }
}

/// Phase 8 slice 8a step 6 — fstatat the leaf under `parent`, returning
/// `(dev, inode)` for the LockRegistry query in the remove handlers.
///
/// Uses `AT_SYMLINK_NOFOLLOW` so the returned identity matches the
/// filesystem entity that `unlinkat` would remove (the link itself,
/// not the target it points at).  A symlink leaf yields the link's
/// own inode — consistent with unlinkat's "remove the directory
/// entry" semantics.
///
/// Returns `None` on any stat error (target doesn't exist, permission
/// denied, etc.).  Callers treat `None` as "not locked" — the
/// subsequent unlink attempt will surface the appropriate error to
/// the Rholang caller.  Doesn't allocate; unlike `fstatat_meta`
/// (which opens the file to build a `Metadata`), this uses `libc::
/// fstatat` directly for the two u64s we need.
pub(super) fn target_dev_inode_at(parent: &SafeParent) -> Option<(u64, u64)> {
    // SAFETY: `parent` (a `SafeParent`) owns an open dirfd that
    // stays valid for `parent`'s lifetime; `parent.leaf_ptr()`
    // returns a NUL-terminated CString ptr owned by the same
    // `SafeParent`.  `libc::stat` is POD — zeroed is a valid
    // initializer.  fstatat writes into `sb` and doesn't retain
    // either pointer.
    unsafe {
        let mut sb: libc::stat = std::mem::zeroed();
        if libc::fstatat(
            parent.as_raw_fd(),
            parent.leaf_ptr(),
            &mut sb,
            libc::AT_SYMLINK_NOFOLLOW,
        ) == 0
        {
            #[allow(clippy::unnecessary_cast)]
            Some((sb.st_dev as u64, sb.st_ino as u64))
        } else {
            None
        }
    }
}

/// Build a stat/error record for one entry inside `dir_fd`.  Opens the
/// entry via openat + O_NOFOLLOW; regular/directory entries produce a
/// full `stat_record`, symlinks and unreadable entries produce an
/// `error_record` (spec §Dir.entries: per-entry error becomes a row).
pub(super) fn entry_stat_row(
    dir_fd: libc::c_int,
    name: &std::ffi::OsStr,
    mode: ConsensusMode,
) -> Par {
    use std::os::fd::FromRawFd;
    use std::os::unix::ffi::OsStrExt;
    let display = name.to_string_lossy().into_owned();
    let cname = match std::ffi::CString::new(name.as_bytes()) {
        Ok(c) => c,
        Err(_) => return error_record(&display, "invalid filename"),
    };
    // SAFETY: `dir_fd` is a caller-supplied open dirfd; `cname`
    // is a locally-owned NUL-terminated CString that outlives the
    // openat call.  On success, `File::from_raw_fd` takes ownership
    // of the fresh fd so Drop closes it on every exit path.
    unsafe {
        let fd = libc::openat(
            dir_fd,
            cname.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        );
        if fd < 0 {
            let e = std::io::Error::last_os_error();
            return error_record(&display, &io_msg_scrub(&e));
        }
        let file = std::fs::File::from_raw_fd(fd);
        match file.metadata() {
            Ok(m) => stat_record(&display, &m, mode),
            Err(e) => error_record(&display, &io_msg_scrub(&e)),
        }
    }
}

#[allow(clippy::too_many_lines)]
pub(super) fn read_dir_capped(
    dir_fd: libc::c_int,
    max: usize,
) -> std::io::Result<(Vec<std::ffi::OsString>, bool)> {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    // SAFETY: `dir_fd` is a caller-supplied open dirfd; fdopendir
    // takes ownership on success (dir_fd is closed by closedir).
    // On failure we manually close it.  readdir/CStr::from_ptr on
    // the DIR* are single-threaded here (caller-owned) and each
    // dirent buffer is copied before the next readdir call.
    unsafe {
        let dir = libc::fdopendir(dir_fd);
        if dir.is_null() {
            let e = std::io::Error::last_os_error();
            libc::close(dir_fd);
            return Err(e);
        }
        let mut names: Vec<OsString> = Vec::new();
        let mut hit_cap = false;
        loop {
            // Reset errno; readdir returns NULL on both EOF and error.
            errno_reset();
            let ent = libc::readdir(dir);
            if ent.is_null() {
                let e = std::io::Error::last_os_error();
                if e.raw_os_error() == Some(0) {
                    break; // Clean EOF.
                }
                libc::closedir(dir);
                return Err(e);
            }
            let name_ptr = (*ent).d_name.as_ptr();
            let name_c = std::ffi::CStr::from_ptr(name_ptr);
            let name_bytes = name_c.to_bytes();
            if name_bytes == b"." || name_bytes == b".." {
                continue;
            }
            if names.len() >= max {
                hit_cap = true;
                break;
            }
            names.push(OsString::from_vec(name_bytes.to_vec()));
        }
        libc::closedir(dir);
        Ok((names, hit_cap))
    }
}

/// Kind marker for a `RecursiveRemoveManifest` entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RemoveKind {
    File,
    Dir,
}

/// Recurse into `dir_fd`, collecting `(rel_base/child_name, kind)`
/// tuples into `out` in sorted post-order.  Called by
/// `collect_recursive_manifest`; caller owns `dir_fd` and is
/// responsible for closing it.
///
/// This function dup-then-fdopendir on `dir_fd` (dup so caller's
/// fd stays valid for openat on subdirs), reads all entries via
/// readdir, sorts them by name for determinism, then processes
/// each entry:
/// - fstatat(AT_SYMLINK_NOFOLLOW) → determine kind.
/// - S_IFREG → push as file.
/// - S_IFDIR → openat(O_NOFOLLOW) → recurse → close → push as dir.
/// - Anything else (S_IFLNK, S_IFIFO, S_IFSOCK, S_IFCHR, S_IFBLK)
///   → return Unsupported.
///
/// # X-3 / SEC-Mi-03 (2026-09-12, branch-review-2026-09-11.md)
///
/// `depth` tracks the recursion level (0 at the top-level entry
/// from `walk_and_unlink_recursive_with_journal`).  Enforced
/// against `MAX_RECURSION_DEPTH` = 1024 to defend against
/// pathological deeply-nested directory trees that could exhaust
/// the OS stack (typically ~2MB on Linux → ~1000+ frames per
/// exhaustion under this function's ~2KB-per-frame local
/// allocations).
///
/// Consensus mode is protected by the threat model
/// (consensus-managed trees have no adversarial writer per
/// `fileio_consensus_no_writer_threat_model.md`); Oracular mode
/// is the surface this cap defends.
pub(crate) const MAX_RECURSION_DEPTH: usize = 1024;

pub(super) async fn chown_impl(
    root: &std::path::Path,
    rel: String,
    owner: Option<String>,
    group: Option<String>,
    // H-5 fix (2026-08-06): expected (dev, inode) for the root
    // path — plumbed from the caller via
    // `self.handles.root_registry.resolve_or_identity(&root_pb)`.
    // `None` skips identity verification (used by test/fixture
    // paths without a boot-populated registry).
    expected_root_id: Option<(u64, u64)>,
) -> Par {
    use super::nss::{resolve_gid, resolve_uid};

    let uid = match owner {
        None => u32::MAX, // libc: -1 means "no change"
        Some(name) => match resolve_uid(&name) {
            Ok(Some(u)) => u,
            Ok(None) => return err(FSERR_BAD_ARG, format!("unknown user {name}")),
            Err(e) => return err(FSERR_IO, e),
        },
    };
    let gid = match group {
        None => u32::MAX,
        Some(name) => match resolve_gid(&name) {
            Ok(Some(g)) => g,
            Ok(None) => return err(FSERR_BAD_ARG, format!("unknown group {name}")),
            Err(e) => return err(FSERR_IO, e),
        },
    };
    let root_pb = root.to_path_buf();
    spawn_blocking_par(move || -> Par {
        let parent = match safe_descend_verified(&root_pb, &rel, expected_root_id) {
            Ok(p) => p,
            Err(qe) => {
                let (c, m) = quarantine_err_reply(&qe);
                return err(c, m);
            }
        };
        // SAFETY: `parent` is a `SafeParent` from
        // `safe_descend_verified` above; its dirfd stays open for
        // `parent`'s lifetime and `leaf_ptr()` returns a NUL-
        // terminated CString owned by the same `SafeParent`.
        // `fchownat` reads both and does not retain either.
        let rc = unsafe {
            libc::fchownat(
                parent.as_raw_fd(),
                parent.leaf_ptr(),
                uid,
                gid,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if rc == 0 {
            ok_bare()
        } else {
            let e = std::io::Error::last_os_error();
            err(io_err_code(&e), io_msg_scrub(&e))
        }
    })
    .await
}

/// Silence unused-import warning on AccessMode (used in Phase 5 by the
/// File-agent wiring).
#[allow(dead_code)]
fn _use_access_mode(_a: AccessMode) {}

/// Streaming-backing slice: drive a single `readdir` on `dirp`,
/// skipping `.` and `..`, and return either:
///   * `[true, entryRecord]` on a real entry (stat via `entry_stat_row`);
///   * `[false, "EOS"]` on clean end-of-directory;
///   * `[false, code, msg]` on `readdir` error or per-entry stat error.
///
/// Must run inside `spawn_blocking` — `readdir` + `openat` + `fstat`
/// are blocking syscalls.  The caller holds the enclosing `DirHandle`
/// Mutex guard for the whole duration, which is what makes the raw
/// `dirp` pointer safe to touch here (POSIX leaves same-`DIR*`
/// concurrent `readdir` undefined; the Mutex is the load-bearing
/// invariant that serializes access).
///
/// SAFETY: `dirp` must be a live `libc::DIR*` obtained via
/// `fdopendir` on a fd that has not been closed; the caller must
/// hold the enclosing `DirHandle::iter` Mutex guard for the
/// duration.
pub(super) fn readdir_one_entry(dirp: *mut libc::DIR, cmode: ConsensusMode) -> Par {
    use std::os::unix::ffi::OsStringExt;
    loop {
        // SAFETY: `errno_reset` writes zero to the platform's
        // per-thread errno location; safe on any thread.  Used to
        // disambiguate EOF (readdir returns NULL + errno==0) from
        // error (returns NULL + errno!=0).
        unsafe { errno_reset() };
        // SAFETY: `dirp` is a live `libc::DIR*` per this function's
        // top-level SAFETY contract; the enclosing DirHandle::iter
        // Mutex serializes readdir calls on this DIR*.
        let ent = unsafe { libc::readdir(dirp) };
        if ent.is_null() {
            let raw = std::io::Error::last_os_error().raw_os_error();
            if raw == Some(0) || raw.is_none() {
                // Clean EOF.
                return err_eos();
            }
            let e = std::io::Error::last_os_error();
            return err(io_err_code(&e), io_msg_scrub(&e));
        }
        // SAFETY: `ent` is non-null (checked above); readdir(3)
        // guarantees `d_name` is a NUL-terminated in-struct array
        // owned by the DIR* buffer.  Pointer is only used through
        // the immediately-following `CStr::from_ptr` and dropped
        // before the next readdir call, so no aliasing issue.
        let name_ptr = unsafe { (*ent).d_name.as_ptr() };
        // SAFETY: `name_ptr` came from a live dirent's `d_name`,
        // NUL-terminated by readdir(3); `name_c` is used only for
        // the subsequent `to_bytes()` before dropping — no lifetime
        // escapes past the next readdir.
        let name_c = unsafe { std::ffi::CStr::from_ptr(name_ptr) };
        let name_bytes = name_c.to_bytes();
        if name_bytes == b"." || name_bytes == b".." {
            continue;
        }
        // dirfd(3) returns the underlying fd for `openat`-based
        // per-entry stat — matches bulk fs_entries' pattern.
        // SAFETY: `dirp` is a live `libc::DIR*` per this function's
        // top-level SAFETY contract (caller holds the `DirHandle::
        // iter` Mutex).  `dirfd` returns the underlying fd, which
        // remains owned by the DIR* (do not close).
        let dir_fd = unsafe { libc::dirfd(dirp) };
        let name_os = std::ffi::OsString::from_vec(name_bytes.to_vec());
        return ok_par(entry_stat_row(dir_fd, &name_os, cmode));
    }
}

/// Streaming-backing slice: returns `true` if `reply` is a `[true, ...]`
/// shape (regardless of the tail).  Used by the entries-stream Next
/// handler to derive `n = 1` (successful entry) vs `n = 0` (EOS or
/// error) for the per-entry supplement charge — same two-branch
/// pattern as bulk `fs_entries`.
///
/// Not the same as `extract_ok_u64` (which requires `[true, int]`);
/// not the same as `extract_ok_list_len` (which requires
/// `[true, list]`); we only need to know if the head is `true`.
pub(super) fn reply_is_ok(reply: &[Par]) -> bool {
    use models::rhoapi::expr::ExprInstance;
    let Some(head) = reply.first() else {
        return false;
    };
    let Some(expr) = head.exprs.first() else {
        return false;
    };
    let Some(ExprInstance::EListBody(list)) = expr.expr_instance.as_ref() else {
        return false;
    };
    let Some(ok_par) = list.ps.first() else {
        return false;
    };
    RhoBoolean::unapply(ok_par) == Some(true)
}

/// Portable errno reset.  `readdir` returns NULL on both EOF and error;
/// distinguishing them requires clearing errno beforehand and checking
/// it after.  errno lives at platform-specific TLS addresses.
#[cfg(target_os = "macos")]
unsafe fn errno_reset() { *libc::__error() = 0; }

#[cfg(target_os = "linux")]
unsafe fn errno_reset() { *libc::__errno_location() = 0; }

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
unsafe fn errno_reset() {
    compile_error!("Unsupported platform for File I/O FIP native primitives");
}

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
