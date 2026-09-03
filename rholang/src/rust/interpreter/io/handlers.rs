// The 22 native filesystem handlers.
//
// Each handler:
//   1. Unapplies the incoming contract call to extract
//      `(produce, is_replay, previous_output, args)`.
//   2. On replay, immediately re-sends `previous_output` — filesystem
//      calls are non-deterministic and must not be re-issued.  Consistent
//      with `gpt4`/`dalle3`/`ollama_chat`: no cost charged on the replay
//      branch (cost already accounted at capture time by the leader).
//   3. Otherwise validates arguments, quarantines any path via
//      `path::safe_descend`, dispatches to the syscall in a
//      `spawn_blocking` task (so long-blocking `fsync`/`copy` never
//      stalls the reactor), and builds the `[true, ...]` /
//      `[false, code, msg]` reply.
//
// Path safety: every path-taking handler takes `(rootCanon, rel)` and
// descends via `openat + O_NOFOLLOW` at each step.  The leaf operation
// is issued as an `*at` syscall against the resolved parent dirfd, so
// the resolution path used for the safety check is the exact same path
// used for the operation — TOCTOU-immune.
//
// Error messages are scrubbed via `io_msg_scrub` — we surface the
// `std::io::ErrorKind` classification but not the free-form message
// (which on some platforms includes the offending path, leaking the
// caller's root prefix).

use std::path::PathBuf;

use models::rhoapi::{ListParWithRandom, Par};
use tokio::task::spawn_blocking;

use super::super::contract_call::ContractCall;
use super::super::dispatch::RhoDispatch;
use super::super::errors::{illegal_argument_error, InterpreterError};
use super::super::metering::MeteredMachine;
use super::super::rho_runtime::RhoISpace;
use super::super::rho_type::{RhoBoolean, RhoByteArray, RhoNumber, RhoString};
use super::dir_handle_table::{DirHandle, DirIter};
use super::errors::*;
use super::handle_table::{FileHandle, FileHandleTable};
// C-R1 review fix: `extract_ok_fd` is used from fs_open's is_replay
// branch to reconstruct the leader's returned fd for shadow-handle
// insertion.
use super::lock::{AcquireOutcome, HolderId, LockError, LockId, LockMode, WaitPolicy};
use super::mode::{fopen_flags, parse_open_mode, AccessMode};
use super::path::{
    canonicalize_lexical, io_msg_scrub, quarantine_err_reply, safe_descend_verified,
    safe_open_verified, SafeParent,
};
use super::response::*;
use super::stat::{error_record, stat_record};
use super::verify::verify_reply_hash_matches_cached;
use super::wal::{PayloadRef, WalEntry, WalOp, WalOutcome};
use super::{costs, ConsensusMode, CMODE_CONSENSUS_STR, CMODE_ORACULAR_STR};

/// Slice 30c H-R3 integration: compute the ack channel's Blake2b256
/// hash the same way rspace computes `channel_hash` for a produce
/// event.  The result is the sidecar key on `Wal::append_with_ack`;
/// the same hash appears in the deploy_log's `ProduceEvent::channels_hash`
/// when the handler publishes its reply, so the log-order drain
/// can match them.
fn ack_channel_hash(ack: &Par) -> [u8; 32] {
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
fn per_entry_ack_seed(ack: &Par, path: &std::path::Path) -> [u8; 32] {
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
/// # Safety
/// Uses libc directly.  Caller supplies a `SafeParent` obtained
/// via `safe_descend_verified` and a `rel_path` derived from a
/// `collect_recursive_manifest` walk of the target subtree.
unsafe fn unlink_manifest_entry(
    parent: &SafeParent,
    rel_path: &std::path::Path,
    kind: RemoveKind,
) -> std::io::Result<()> {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    let flags = match kind {
        RemoveKind::File => 0,
        RemoveKind::Dir => libc::AT_REMOVEDIR,
    };
    if rel_path.as_os_str().is_empty() {
        let rc = libc::unlinkat(parent.as_raw_fd(), parent.leaf_ptr(), flags);
        return if rc == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        };
    }
    // Only accept Normal components — reject `.`, `..`, absolute
    // roots, and Windows prefixes.  Defense-in-depth: the walker
    // that feeds this function filters "." and ".." via
    // std::fs::read_dir, but a future refactor that swapped
    // walkers could reintroduce them; the openat chain below would
    // then happily traverse "..".
    let mut components: Vec<&std::ffi::OsStr> = Vec::new();
    for c in rel_path.components() {
        match c {
            std::path::Component::Normal(n) => components.push(n),
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("rel_path contains non-Normal component: {c:?}"),
                ));
            }
        }
    }
    if components.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "rel_path yielded no components",
        ));
    }
    // Pin the target dirfd.
    let target_fd = libc::openat(
        parent.as_raw_fd(),
        parent.leaf_ptr(),
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
    );
    if target_fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut cur_fd = OwnedFd::from_raw_fd(target_fd);
    // Descend through intermediate components.
    for intermediate in &components[..components.len() - 1] {
        let cname = std::ffi::CString::new(intermediate.as_bytes()).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "nul in path component")
        })?;
        let next_fd = libc::openat(
            cur_fd.as_raw_fd(),
            cname.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        );
        if next_fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        cur_fd = OwnedFd::from_raw_fd(next_fd);
    }
    // Final unlink of the leaf from the pinned parent dirfd.
    let leaf_name = components.last().expect("components non-empty");
    let leaf_c = std::ffi::CString::new(leaf_name.as_bytes())
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "nul in leaf"))?;
    let rc = libc::unlinkat(cur_fd.as_raw_fd(), leaf_c.as_ptr(), flags);
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Post DD-RemoveDirReplyShape (2026-09-03): parse the recursive-
/// removeDir reply manifest into `(PathBuf, RemoveKind)` tuples.
///
/// Reply shapes (Consensus recursive only; other paths have no
/// manifest and this returns an empty Vec):
///   * `[true, nDeleted, [[path, kind], ...]]` — success.
///   * `[false, code, msg, nDeletedBeforeError, [[path, kind], ...]]`
///     — partial success followed by failure; the inner list
///     contains only what was successfully deleted before the
///     failing entry.
///
/// Returns an empty Vec for any other shape (any 4-element non-
/// Consensus-recursive reply, or malformed input).
///
/// Kept as `#[allow(dead_code)]` because the R5(b) follower reads
/// its own manifest via `collect_recursive_manifest` rather than
/// consuming the leader's, and cost accounting reads `nDeleted`
/// directly from the reply via `extract_removedir_n_deleted`.
/// The manifest still ships in the Consensus recursive reply as
/// an implementation-side channel; keeping this parser lets
/// diagnostics + future consumers extract it without re-walking.
#[allow(dead_code)]
fn extract_removedir_manifest(previous: &[Par]) -> Vec<(std::path::PathBuf, RemoveKind)> {
    let head = match previous.first() {
        Some(h) => h,
        None => return Vec::new(),
    };
    let expr = match head.exprs.first() {
        Some(e) => e,
        None => return Vec::new(),
    };
    let outer = match &expr.expr_instance {
        Some(models::rhoapi::expr::ExprInstance::EListBody(l)) => l,
        _ => return Vec::new(),
    };
    // Manifest lives at index 2 (success) or 4 (failure) post
    // DD-RemoveDirReplyShape.  Detect via the head bool.
    let ok_par = match outer.ps.first() {
        Some(p) => p,
        None => return Vec::new(),
    };
    let manifest_par = match RhoBoolean::unapply(ok_par) {
        Some(true) => match outer.ps.get(2) {
            Some(p) => p,
            None => return Vec::new(),
        },
        Some(false) => match outer.ps.get(4) {
            Some(p) => p,
            None => return Vec::new(),
        },
        None => return Vec::new(),
    };
    let manifest_expr = match manifest_par.exprs.first() {
        Some(e) => e,
        None => return Vec::new(),
    };
    let manifest_list = match &manifest_expr.expr_instance {
        Some(models::rhoapi::expr::ExprInstance::EListBody(l)) => l,
        _ => return Vec::new(),
    };
    let mut out = Vec::with_capacity(manifest_list.ps.len());
    for entry_par in &manifest_list.ps {
        let entry_expr = match entry_par.exprs.first() {
            Some(e) => e,
            None => continue,
        };
        let entry_list = match &entry_expr.expr_instance {
            Some(models::rhoapi::expr::ExprInstance::EListBody(l)) => l,
            _ => continue,
        };
        let path_par = match entry_list.ps.first() {
            Some(p) => p,
            None => continue,
        };
        let kind_par = match entry_list.ps.get(1) {
            Some(p) => p,
            None => continue,
        };
        let path = match RhoString::unapply(path_par) {
            Some(s) => std::path::PathBuf::from(s),
            None => continue,
        };
        let kind = match RhoString::unapply(kind_par).as_deref() {
            Some("file") => RemoveKind::File,
            Some("dir") => RemoveKind::Dir,
            _ => continue,
        };
        out.push((path, kind));
    }
    out
}

/// DD-RemoveDirReplyShape (2026-09-03): success reply carrying
/// `nDeleted` at position 1.  Used by `fs_remove_dir` for
/// non-recursive success (`[true, 1]`) and Oracular recursive
/// success (`[true, n]`).  See design-decisions.md.
fn ok_with_count(n_deleted: u64) -> Par {
    list_par_2(bool_par_true(), RhoNumber::create_par(n_deleted as i64))
}

/// DD-RemoveDirReplyShape (2026-09-03): failure reply carrying
/// `nDeletedBeforeError` at position 3.  Used by `fs_remove_dir`
/// for non-recursive failure (n=0) and Oracular recursive failure
/// (n = count-before-error).  See design-decisions.md.
fn err_with_count(code: &str, msg: impl Into<String>, n_deleted: u64) -> Par {
    let items = vec![
        bool_par_false(),
        RhoString::create_par(code.to_string()),
        RhoString::create_par(msg.into()),
        RhoNumber::create_par(n_deleted as i64),
    ];
    list_par_from(items)
}

/// DD-RemoveDirReplyShape (2026-09-03): success reply for a
/// recursive removeDir on a Consensus cap.  Shape:
/// `[true, nDeleted, [[path, kind], ...]]`.  `nDeleted` at
/// position 1 (uniform with all other removeDir success shapes);
/// the manifest at position 2 is the implementation-side channel
/// for R5(b) follower re-execution.  Dir.rho unwraps to
/// `[true, nDeleted]` at the Rholang boundary.
fn ok_recursive_manifest(deleted: &[(std::path::PathBuf, RemoveKind)]) -> Par {
    let inner: Vec<Par> = deleted
        .iter()
        .map(|(path, kind)| {
            let path_s = path.to_string_lossy().into_owned();
            list_par_2(
                RhoString::create_par(path_s),
                RhoString::create_par(kind.as_wire().to_string()),
            )
        })
        .collect();
    let items = vec![
        bool_par_true(),
        RhoNumber::create_par(deleted.len() as i64),
        list_par_from(inner),
    ];
    list_par_from(items)
}

/// DD-RemoveDirReplyShape (2026-09-03): early-failure reply picker
/// for `fs_remove_dir`.  Non-recursive OR Oracular → 4-element
/// `[false, code, msg, 0]`.  Consensus recursive → 5-element
/// `[false, code, msg, 0, []]` (empty manifest, no deletions
/// before this early error).  Used at pre-walk failure sites in
/// both the leader and follower spawn_blocking closures where the
/// walk didn't get far enough to have a partial manifest to report.
fn early_err_for_remove_dir(
    recursive: bool,
    cmode: ConsensusMode,
    code: &str,
    msg: impl Into<String>,
) -> Par {
    if recursive && cmode == ConsensusMode::Consensus {
        err_with_manifest(code, msg, &[])
    } else {
        err_with_count(code, msg, 0)
    }
}

/// DD-RemoveDirReplyShape (2026-09-03): failure reply for a
/// recursive removeDir on a Consensus cap.  Shape:
/// `[false, code, msg, nDeletedBeforeError, [[path, kind], ...]]`.
/// `nDeletedBeforeError` at position 3; manifest at position 4.
/// Dir.rho unwraps to `[false, code, msg, nDeletedBeforeError]`
/// at the Rholang boundary.
fn err_with_manifest(
    code: &str,
    msg: impl Into<String>,
    deleted: &[(std::path::PathBuf, RemoveKind)],
) -> Par {
    let inner: Vec<Par> = deleted
        .iter()
        .map(|(path, kind)| {
            let path_s = path.to_string_lossy().into_owned();
            list_par_2(
                RhoString::create_par(path_s),
                RhoString::create_par(kind.as_wire().to_string()),
            )
        })
        .collect();
    let items = vec![
        bool_par_false(),
        RhoString::create_par(code.to_string()),
        RhoString::create_par(msg.into()),
        RhoNumber::create_par(deleted.len() as i64),
        list_par_from(inner),
    ];
    list_par_from(items)
}

/// Small internal helper: `Par::default()` with a single `EList`
/// expression carrying `items`.
fn list_par_from(items: Vec<Par>) -> Par {
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::{EList, Expr};
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EListBody(EList {
            ps: items,
            locally_free: shared::rust::BitSet::default(),
            connective_used: false,
            remainder: None,
        })),
    }])
}

fn list_par_2(a: Par, b: Par) -> Par { list_par_from(vec![a, b]) }

fn bool_par_true() -> Par { Par::default().with_exprs(vec![RhoBoolean::create_expr(true)]) }

fn bool_par_false() -> Par { Par::default().with_exprs(vec![RhoBoolean::create_expr(false)]) }

/// Compose `io_err_code(e) → fserr_to_code(...)` for the WAL
/// `WalOutcome::Failure { code }` slot.  Consumed by handlers that
/// need the numeric FSERR code without a string round-trip.
fn io_err_code_u32(e: &std::io::Error) -> u32 { fserr_to_code(io_err_code(e)) }

/// Post DD-RemoveDirReplyShape (2026-09-03): read `nDeleted` from
/// a removeDir reply Par.  Reads position 1 (success) or position 3
/// (failure) — both indices carry the count uniformly across every
/// removeDir code path (non-recursive, recursive Oracular, recursive
/// Consensus).  Returns 0 for any malformed / non-list reply so
/// cost accounting fails safe.
///
/// Superseded the pre-DD-RemoveDirReplyShape branch-on-
/// (recursive, cmode) helpers `fs_remove_dir_supplement_count` +
/// `_from_previous` which had to derive the count from the manifest
/// (Consensus recursive only) or hard-code it (non-recursive = 1,
/// Oracular recursive = 0).  Post-shape-change the reply itself is
/// the canonical count source on every code path — including
/// Oracular recursive, which now bills per-entry symmetrically
/// with Consensus recursive.
fn extract_removedir_n_deleted(reply: &Par) -> u64 {
    let expr = match reply.exprs.first() {
        Some(e) => e,
        None => return 0,
    };
    let outer = match &expr.expr_instance {
        Some(models::rhoapi::expr::ExprInstance::EListBody(l)) => l,
        _ => return 0,
    };
    let ok_par = match outer.ps.first() {
        Some(p) => p,
        None => return 0,
    };
    let n_par = match RhoBoolean::unapply(ok_par) {
        Some(true) => match outer.ps.get(1) {
            Some(p) => p,
            None => return 0,
        },
        Some(false) => match outer.ps.get(3) {
            Some(p) => p,
            None => return 0,
        },
        None => return 0,
    };
    match RhoNumber::unapply(n_par) {
        Some(n) if n >= 0 => n as u64,
        _ => 0,
    }
}

/// Leader-path cost supplement count — read from fresh reply Par.
/// Post DD-RemoveDirReplyShape: single helper reads `nDeleted`
/// directly from the reply on every code path.
fn fs_remove_dir_supplement_count(
    _parsed: &Option<(String, String, bool)>,
    _cmode: ConsensusMode,
    reply: &Par,
) -> u64 {
    extract_removedir_n_deleted(reply)
}

/// Follower-path counterpart — read from `previous[0]`.  Same
/// helper; the split is preserved so callers can't accidentally
/// slice-of-Par vs. Par-ref-swap.
fn fs_remove_dir_supplement_count_from_previous(
    _parsed: &Option<(String, String, bool)>,
    _cmode: ConsensusMode,
    previous: &[Par],
) -> u64 {
    match previous.first() {
        Some(reply) => extract_removedir_n_deleted(reply),
        None => 0,
    }
}

/// Cap on `fs_entries` output size — prevents a malicious caller pointing
/// the native at a million-entry directory and OOMing the node.
pub const MAX_ENTRIES: usize = 65_536;

/// Cap on `fs_write` payload — symmetric with `MAX_READ_BYTES`.
pub const MAX_WRITE_BYTES: u64 = 64 * 1024 * 1024;

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

    fn is_contract_call(&self) -> ContractCall {
        ContractCall {
            space: self.space.clone(),
            dispatcher: self.dispatcher.clone(),
        }
    }

    /// Redesign helper: journal a Write / WriteAt to the WAL from
    /// data fully derivable from args (fd + bytes + offset).  Called
    /// from `fs_write` and `fs_write_at` BEFORE the `is_replay`
    /// short-circuit so leader and follower populate identical WALs
    /// (C-29-F1 review fix).
    ///
    /// Returns:
    ///   * `Ok(true)`  — fd is a Consensus cap and entry was appended.
    ///   * `Ok(false)` — fd is not Consensus (Oracular or unknown), no-op.
    ///   * `Err(())`   — WAL is at cap (`MAX_WAL_ENTRIES`); caller must
    ///     translate to `FSERR_QUOTA_EXCEEDED` and NOT proceed with the
    ///     syscall so leader/follower stay symmetric (both hit the same
    ///     cap moment).
    ///
    /// Note on partial writes (M-29-3 trade-off): we record the
    /// REQUESTED byte length + a hash of the REQUESTED payload.  On a
    /// partial-write the actual on-disk state is n<len; the FIP
    /// documents this as a caller-responsibility retry pattern.
    /// Recording requested-bytes keeps the WAL fully derivable from
    /// contract args, which is what makes leader/follower symmetric
    /// on the `is_replay` short-circuit path (the follower does NOT
    /// re-issue the syscall and therefore does not know `n`).
    async fn journal_write(
        &self,
        fd: u64,
        bytes: &[u8],
        offset: Option<u64>,
        ack: &Par,
    ) -> Result<bool, ()> {
        // For sequential Write (offset=None from caller), pull the
        // fd's shadow position — that's the absolute offset the
        // subsequent libc::write will land at.  Both leader and
        // follower evolve `position` deterministically from the same
        // sequence of contract-arg values (see FileHandle::position
        // docstring), so this read is symmetric.  For WriteAt, the
        // caller supplied the explicit offset.
        //
        // Position-follow-up (2026-08-26): a WAL entry with
        // `offset=Some(pos)` for sequential Write is what unblocks
        // the fresh-tree applier (`apply_wal_to_fresh_tree` in
        // `fs_wal_spec.rs`) to reconstruct file state from the WAL
        // alone.  Prior to this, sequential Write recorded
        // `offset=None` and the applier had to panic on it.
        let wal_meta = self
            .handles
            .with_mut(fd, |h| (h.cmode, h.canon_path.clone(), h.position))
            .await;
        match wal_meta {
            Some((ConsensusMode::Consensus, canon_path, position)) => {
                let (op, resolved_offset) = match offset {
                    Some(off) => (WalOp::WriteAt, Some(off)),
                    None => (WalOp::Write, Some(position)),
                };
                // Phase 7b-2 (2026-08-27): stash the write payload
                // content-addressed on disk BEFORE appending the
                // WAL entry so a joining validator's fetch protocol
                // sees the bytes as soon as the WAL entry lands.
                // Failure is logged but not fatal — the joiner-side
                // fetch protocol will find the bytes on other
                // serving peers (or fall back to the reducer once
                // wired).  We do the persist unconditionally on
                // Consensus caps whenever a store is attached; a
                // downstream retention pass evicts stale bytes on
                // snapshot-cycle boundaries.
                if let Some(store) = self.handles.payload_store() {
                    if let Err(e) = store.persist(bytes) {
                        tracing::warn!(
                            target: "f1r3fly.fs_wal.payload_store",
                            error = %e,
                            "payload store persist failed on Consensus write; \
                             joiners will need to fetch from another peer"
                        );
                    }
                }
                // DD-7b-2 (a) Option 2 (2026-08-29): record the
                // payload_hash → deploy_sig mapping into the
                // block-storage-backed persistent index.  Chained
                // through the existing deploy_sig → block_hash
                // map (deploy_index in block_dag_key_value_storage),
                // this lets a joiner reconstruct write bytes from
                // block-stored deploys via
                // capture_consensus_writes_by_replaying_deploy —
                // the second tier of apply_wal_slice_after_fetch's
                // reducer below the local PayloadLookup.
                //
                // Symmetric on leader (fs_write path) AND follower
                // (replay path via journal_write's replay-branch
                // caller); WalDeployScope sets current_deploy_sig
                // on both sides so any node whose block processing
                // succeeded can serve the Option 2 tier.  An empty
                // sig (system deploys, between-deploy handler
                // calls) skips — see FileHandleTable::
                // current_deploy_sig docstring.  M-2 review
                // discipline: fail-open; log Err at warn instead
                // of aborting the deploy so a broken index doesn't
                // reject Consensus writes leader-side.
                let PayloadRef::Hash(payload_hash) = PayloadRef::hash(bytes) else {
                    unreachable!("PayloadRef::hash always returns Hash variant")
                };
                if let Some(recorder) = self.handles.payload_source_recorder() {
                    let sig = self
                        .handles
                        .current_deploy_sig
                        .read()
                        .expect("current_deploy_sig lock poisoned")
                        .clone();
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
                self.handles
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
                            // H-6 fix (2026-08-06): optimistic
                            // Success placeholder; the leader's
                            // finalize_failure_journal below
                            // updates to Failure on syscall error.
                            outcome: WalOutcome::Success,
                        },
                        ack_channel_hash(ack),
                    )
                    .map(|()| true)
            }
            _ => Ok(false),
        }
    }

    /// Slice 32 (PB-M-14 read-hash): journal a Read/ReadAt to the WAL.
    /// Called AFTER a successful read (leader path) OR from the
    /// `is_replay` branch after extracting the cached bytes (follower
    /// path) — both sides append the SAME entry (same op, path,
    /// offset, length, and `Hash(bytes)` payload) so the WAL is
    /// byte-identical across leader and follower.
    ///
    /// The hash is over the RETURNED bytes (post-truncate to the
    /// actual read length), not the requested length — mirrors how
    /// fs_read's reply carries `ok_bytes(bytes)` with the actual
    /// truncated length after `buf.truncate(got as usize)`.
    ///
    /// Under PB-M-14 semantics, a joining validator replaying the
    /// deploy against reconstructed state must observe the same
    /// bytes on `fs_read`.  A mismatch (hash of freshly-read bytes
    /// != WAL entry's hash) indicates disk state divergence between
    /// leader and follower — the read-verify path (implemented
    /// symmetrically via `journal_read` on both sides) catches this
    /// at WAL-root-comparison time rather than as a silent tuplespace
    /// fork downstream.
    async fn journal_read(
        &self,
        fd: u64,
        bytes: &[u8],
        offset: Option<u64>,
        ack: &Par,
    ) -> Result<bool, ()> {
        // Sequential Read journals with shadow-position as absolute
        // offset — same rationale as sequential Write; joining
        // validators can now verify a Read against reconstructed
        // state at the correct file position.  See journal_write
        // for the position-follow-up (2026-08-26) design note.
        //
        // journal_read is called AFTER the syscall completes
        // successfully — at which point the handler has NOT yet
        // advanced FileHandle.position.  So the position read here
        // reflects the PRE-read position, which is exactly the
        // absolute offset the leader's libc::read consumed bytes
        // from.  The handler then advances position by
        // `bytes.len()` after this call.
        let wal_meta = self
            .handles
            .with_mut(fd, |h| (h.cmode, h.canon_path.clone(), h.position))
            .await;
        match wal_meta {
            Some((ConsensusMode::Consensus, canon_path, position)) => {
                let (op, resolved_offset) = match offset {
                    Some(off) => (WalOp::ReadAt, Some(off)),
                    None => (WalOp::Read, Some(position)),
                };
                self.handles
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
                            // Reads are journaled AFTER a successful
                            // syscall (see docstring above); the
                            // outcome is always Success.  Failed
                            // reads short-circuit before this call.
                            outcome: WalOutcome::Success,
                        },
                        ack_channel_hash(ack),
                    )
                    .map(|()| true)
            }
            _ => Ok(false),
        }
    }

    /// Phase 2 (Consensus re-execute + verify, 2026-09-01):
    /// sibling helper to `journal_read` for the divergence path.
    /// `journal_read` hardcodes `WalOutcome::Success` (reads are
    /// only journaled AFTER a successful syscall on the leader
    /// path); a Consensus follower that detects fs_read /
    /// fs_read_at re-execute divergence needs to journal a Failure
    /// entry with `FSERR_CODE_CONSENSUS_DIVERGENCE`.
    ///
    /// Field shape mirrors `journal_read`'s WalEntry with two
    /// deltas: `payload_ref: None` and `length: None` because the
    /// divergence-err reply carries no bytes to hash.  Follower's
    /// divergent WAL entry inherently doesn't match the leader's
    /// (leader never emits a CONSENSUS_DIVERGENCE outcome); block
    /// validation catches the divergence via RSpace rig's produce
    /// comparator on the ack channel.
    ///
    /// Returns `true` if the entry was appended (fd was a
    /// Consensus-cap shadow); `false` otherwise (unregistered fd
    /// or Oracular shadow — the latter never enters this path in
    /// normal flow since the handler's dispatch routes Oracular to
    /// the tautological branch above).
    ///
    /// # Coverage note (2026-09-01)
    ///
    /// The `_ => false` branch is defense-in-depth: fs_read /
    /// fs_read_at dispatch upstream on `jmode != Consensus`, so
    /// this helper is only reached with a Consensus shadow in
    /// well-formed execution.  Direct testing of the fallthrough
    /// would require exposing this method `pub(crate)` and
    /// constructing a full `FsProcesses` in a unit test —
    /// disproportionate scaffolding for a branch that mirrors
    /// `journal_read`'s identically-shaped Consensus guard (which
    /// IS exercised via the Oracular is_replay tautological path).
    /// A future caller that hits this branch through a NEW
    /// dispatch site would need its own coverage pin.
    async fn journal_read_divergence(&self, fd: u64, offset: Option<u64>, ack: &Par) -> bool {
        let wal_meta = self
            .handles
            .with_mut(fd, |h| (h.cmode, h.canon_path.clone()))
            .await;
        match wal_meta {
            Some((ConsensusMode::Consensus, canon_path)) => {
                let op = match offset {
                    Some(_) => WalOp::ReadAt,
                    None => WalOp::Read,
                };
                self.handles
                    .wal
                    .append_with_ack(
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
                                code: FSERR_CODE_CONSENSUS_DIVERGENCE,
                            },
                        },
                        ack_channel_hash(ack),
                    )
                    .is_ok()
            }
            _ => false,
        }
    }

    /// Redesign helper: journal a Truncate to the WAL from data
    /// fully derivable from args (fd + n).  Called from `fs_truncate`
    /// BEFORE the `is_replay` short-circuit (C-29-F1 review fix).
    /// Return semantics identical to `journal_write`.
    /// Slice 30c M-29-3 fix: finalize a previously-reserved write
    /// WAL entry with the ACTUAL bytes written.  Called by
    /// `fs_write` / `fs_write_at` on partial writes (n < requested).
    ///
    /// Semantics:
    /// - Locates the entry with matching ack_hash (the placeholder
    ///   appended by `journal_write` pre-syscall).
    /// - Updates only `length` and `payload_ref` — preserves
    ///   `op`, `path`, `offset`, `outcome` from the placeholder.
    /// - On leader: `n` comes from the syscall reply.
    /// - On follower: `n` comes from the cached `previous` reply.
    /// Both sides derive the same `n` from `bytes` (same args, same
    /// deterministic reply) and produce a byte-identical final entry.
    ///
    /// M-7 fix (2026-08-06): removed the fd-relookup path that
    /// silently no-op'd if the fd was closed between
    /// `journal_write` and this call.  The placeholder is keyed
    /// by `ack_hash` (a fresh unforgeable, unique per syscall)
    /// which cannot be aliased away — the placeholder was just
    /// appended in the same handler.  The WAL method
    /// (`update_partial_write_by_ack_hash`) is a no-op if no
    /// entry matches, which is the correct behavior for
    /// non-Consensus caps (they never appended a placeholder).
    ///
    /// Full-length writes (n == requested) don't call this — the
    /// pre-syscall placeholder already has the correct content.
    /// Failed writes (error reply) go through
    /// `finalize_failure_journal` instead (H-6).
    fn finalize_write_journal(&self, requested_bytes: &[u8], actual_n: u64, ack: &Par) {
        let n = (actual_n as usize).min(requested_bytes.len());
        let actual_slice = &requested_bytes[..n];
        // Phase 7b-2 (2026-08-27): re-persist under the truncated
        // slice's hash.  On full-length writes `n == requested`
        // and the pre-syscall persist already covered the same
        // bytes (idempotent).  On partial-write `n < requested`,
        // the WAL entry's `payload_ref` is updated to point at the
        // truncated slice's hash — the payload store must have
        // those bytes too, or the joiner side will fail to fetch.
        // Failure is logged but not fatal.
        if let Some(store) = self.handles.payload_store() {
            if let Err(e) = store.persist(actual_slice) {
                tracing::warn!(
                    target: "f1r3fly.fs_wal.payload_store",
                    error = %e,
                    "payload store persist failed on partial-write finalize"
                );
            }
        }
        let _ = self
            .handles
            .wal
            .update_partial_write_by_ack_hash(ack_channel_hash(ack), actual_slice);
    }

    /// H-6 fix (2026-08-06): flip a reserved WAL entry's outcome
    /// to `Failure { code }` when the leader's syscall reply
    /// carries an error.  Symmetric across leader (`code` from
    /// the error reply the syscall just returned) and follower
    /// (`code` extracted from the cached `previous` reply).
    ///
    /// All other fields (op, path, offset, length, payload_ref)
    /// are preserved so replay consumers can see WHAT the leader
    /// asked for and WHY they should skip it.
    ///
    /// Followers reading a `Failure` entry MUST NOT apply the
    /// mutation to reconstructed state — the leader never wrote
    /// anything, so the follower's reconstructed state stays
    /// consistent by also not writing.
    ///
    /// No-op if no entry matches `ack_hash` (e.g., the syscall
    /// was on a non-Consensus cap, so `journal_write` /
    /// `journal_truncate` returned early with no reservation).
    fn finalize_failure_journal(&self, code: u32, ack: &Par) {
        let _ = self
            .handles
            .wal
            .update_outcome_by_ack_hash(ack_channel_hash(ack), WalOutcome::Failure { code });
    }

    async fn journal_truncate(&self, fd: u64, n: u64, ack: &Par) -> Result<bool, ()> {
        let wal_meta = self
            .handles
            .with_mut(fd, |h| (h.cmode, h.canon_path.clone()))
            .await;
        match wal_meta {
            Some((ConsensusMode::Consensus, canon_path)) => self
                .handles
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
                        // H-6: optimistic placeholder; the
                        // leader's finalize_failure_journal
                        // updates to Failure on syscall error.
                        outcome: WalOutcome::Success,
                    },
                    ack_channel_hash(ack),
                )
                .map(|()| true),
            _ => Ok(false),
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

    /// Journal a path-based mutation entry with a single
    /// canonical path (used by chmod/chown/removeFile).  Returns
    /// `Ok(true)` on Consensus append, `Ok(false)` on Oracular
    /// no-op, `Err(())` on WAL-cap exhaustion.  Payload fields
    /// (`mode_bits`, `owner`, `group`) are supplied by the caller
    /// for op-specific data; `extra_path` and `length` are
    /// unpopulated for single-path ops.
    #[allow(clippy::result_unit_err)]
    /// # Shape A invariant (Task 0.4, 2026-08-31)
    ///
    /// `canon_path` MUST be derived from the RAW Rholang canonRoot
    /// via `canonicalize_lexical(raw_root, rel)` — NOT from a root
    /// that has been passed through `RootIdentityRegistry::
    /// resolve_or_identity`.  Under Consensus-fs Shape A, Consensus
    /// caps register `canonRoot = BUNDLE_ROOT_PREFIX`
    /// (bundle-relative); the resolver rewrites it to a
    /// per-validator on-disk absolute for the syscall step, but the
    /// WAL entry must record the bundle-relative form so leader and
    /// follower produce byte-identical WAL entries and the joiner's
    /// applier can rewrite via its own registry at boot.  A journal
    /// site that accidentally hands the RESOLVED root here would
    /// silently record per-validator absolute paths and break both
    /// leader/follower WAL byte-identity and joiner-side
    /// applicability.  See handlers.rs::fs_remove_dir for the
    /// working pattern (raw_root_pb, on_disk_root_pb, canon_wal_target).
    async fn journal_path_mutation_single(
        &self,
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
        self.handles
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

    /// Journal a two-path mutation entry (Rename, CopyFile).
    /// Same shape as `journal_path_mutation_single` but populates
    /// `extra_path` with the destination.
    ///
    /// # Shape A invariant (Task 0.4, 2026-08-31)
    ///
    /// BOTH `from_canon_path` and `to_canon_path` MUST be derived
    /// from the RAW Rholang canonRoot via
    /// `canonicalize_lexical(raw_root, rel)` — same discipline as
    /// `journal_path_mutation_single`.  fs_rename and fs_copy_file's
    /// current call sites use raw roots from parsed args (never
    /// touching `resolve_or_identity` on the pair), so this
    /// invariant holds today.
    #[allow(clippy::result_unit_err)]
    async fn journal_path_mutation_two(
        &self,
        cmode: ConsensusMode,
        op: WalOp,
        from_canon_path: PathBuf,
        to_canon_path: PathBuf,
        ack: &Par,
    ) -> Result<bool, ()> {
        if cmode != ConsensusMode::Consensus {
            return Ok(false);
        }
        self.handles
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

    /// M-5 fix (2026-08-06): journal a state-read reply on a
    /// Consensus cap.  Called AFTER the syscall completes (both
    /// leader and follower paths — the follower extracts the
    /// same reply from the cached `previous`, hashes it, and
    /// journals identical bytes).
    ///
    /// - `op`: `WalOp::Stat` / `WalOp::Entries` / `WalOp::Size`.
    /// - `path`: the canonical target (root + rel joined for
    ///   path-based ops; canon_path from FileHandle for fd-based).
    /// - `reply`: the just-produced reply Par.  Hashed via
    ///   `stable_hash_provider::hash` for a canonical Blake2b256.
    /// - Outcome derived from the reply: `[true, ...]` → Success,
    ///   `[false, code, ...]` → Failure { code = fserr_to_code(code) }.
    ///
    /// A no-op if `cmode != Consensus`.  fs_stat / fs_entries
    /// take cmode as an arg; fs_size looks up the FileHandle's
    /// cmode via the fd.  fs_exists deliberately excluded —
    /// its arity (3) has no cmode signal; a follow-up would
    /// require an arity bump.
    fn journal_state_read(
        &self,
        cmode: ConsensusMode,
        op: WalOp,
        path: std::path::PathBuf,
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
        let _ = self.handles.wal.append_with_ack(
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

    // -------------------------------------------------------------------
    // open — (rootCanon, rel, mode) -> [true, fd] | [false, code, msg]
    // -------------------------------------------------------------------
    pub async fn fs_open(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        // Phase 9 slice 9b-ii: charge the fs_open weight at handler
        // entry.  Placed BEFORE the argument-shape check on purpose:
        // the charge is a pure function of the handler identity, so
        // leader and replay both hit it regardless of arg validity —
        // giving byte-identical `BillableTokenEvent::Primitive` logs
        // across validators (a load-bearing consensus invariant under
        // D3).  On budget exhaustion `?` propagates
        // `OutOfPhlogistonsError`; the deploy is rejected before any
        // syscall runs.  See rholang/src/rust/interpreter/io/costs.rs
        // for the weight table and rholang/tests/fileio_cost_spec.rs
        // for the golden-value regression pins.
        self.metering.reserve_primitive(costs::fs_open_cost())?;
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_open"));
        };
        // Slice 29 (PB-M-14): `(root, rel, mode, cmode, ack)` — the
        // cmode arg is stashed in the returned `FileHandle` so
        // subsequent mutating handlers can journal to the consensus
        // WAL when the cap is `Consensus`.  Same fail-closed
        // semantics as the slice-26 cmode threading: invalid cmode
        // → `FSERR_BAD_ARG`.
        let [root_par, rel_par, mode_par, cmode_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_open"));
        };
        if is_replay {
            // C-R1 review fix (slice 29 round 2): the leader's fs_open
            // populates a FileHandle with (cmode, canon_path); on the
            // follower we must populate a *shadow* handle at the same
            // fd so subsequent replay-branch mutating handlers'
            // `journal_write` / `journal_truncate` can look up
            // `(cmode, canon_path)` and append an identical WAL entry.
            // Pre-fix: follower's fs_open short-circuited without
            // inserting into the fd table → follower's journal_write
            // no-op'd on unknown fd → leader/follower WAL divergence.
            //
            // 2026-08-30 review-follow-up: fd sites use
            // `extract_ok_fd` (bit-preserving reinterpret) not
            // `extract_ok_u64` (reject-negative).  Fds seeded from
            // state-hash entropy commonly exceed i64::MAX and
            // Rholang's GInt wraps to negative — see the docstring
            // on `extract_ok_fd` for the PB-M-14 canary rationale.
            //
            // Phase 2 prerequisite (Consensus re-execute + verify,
            // 2026-09-01): under Consensus cmode, the shadow's `file`
            // slot now backs a REAL `libc::open` against the
            // follower's own subdir file (via the Shape A resolver).
            // This is load-bearing for downstream fd-based Consensus
            // re-execute ops (fs_size / fs_read / fs_read_at /
            // fs_write / fs_write_at / fs_truncate) — they call
            // `libc::fstat` / `read` / `write` / `ftruncate` on the
            // shadow's `file`, so a `file: None` shadow would surface
            // as `FSERR_CLOSED` on the fresh syscall and trip spurious
            // `FSERR_CONSENSUS_DIVERGENCE`.  Oracular caps keep the
            // pre-Phase-2 metadata-only shadow because Oracle-mode fs
            // state isn't reproducible on the follower.
            if let Some(fd) = extract_ok_fd(&previous) {
                // Only Consensus caps need shadow handles for
                // journaling — Oracular writes don't append to the
                // WAL either way.  But we insert regardless so any
                // future handler consulting FileHandle metadata on
                // the replay branch sees consistent state.
                if let (Some(root), Some(rel), Some(mode_str)) = (
                    RhoString::unapply(root_par),
                    RhoString::unapply(rel_par),
                    RhoString::unapply(mode_par),
                ) {
                    // resolve_cmode: fail-closed on bogus values, but a
                    // bogus cmode with a `[true, fd]` cached reply is
                    // definitionally a bug (leader wouldn't have
                    // returned success on a rejected cmode).  Fall
                    // back to `Consensus` (most restrictive) rather
                    // than skip the insert so any divergence surfaces
                    // via WAL comparison rather than silently masking.
                    let cmode = resolve_cmode(cmode_par).unwrap_or(ConsensusMode::Consensus);
                    let intent = parse_open_mode(&mode_str);
                    // Phase 2: under Consensus, attempt the real
                    // open against the follower's own subdir file.
                    // Uses the SAME code path as the leader's
                    // open_impl (resolve_or_identity +
                    // safe_open_verified) so the follower's shadow's
                    // File wraps a real OS fd pointing at
                    // <follower_subdir>/<rel>.  On failure (fs drift,
                    // permission mismatch, missing follower-side
                    // file), fall back to `file: None` — the
                    // divergence will surface on the FIRST downstream
                    // Consensus fd op as `FSERR_CONSENSUS_DIVERGENCE`
                    // (fresh FSERR_CLOSED vs cached `[true, ...]`
                    // reply → hash mismatch).  Consensus + O_APPEND
                    // is rejected leader-side (see open_impl below);
                    // leader never returns [true, fd] on that
                    // combination so `extract_ok_fd` wouldn't have
                    // succeeded here.  Guard defensively regardless.
                    let file: Option<std::fs::File> = if cmode == ConsensusMode::Consensus {
                        match intent {
                            Some(intent) if !intent.append => {
                                let root_pb = PathBuf::from(&root);
                                let (root_pb, expected_root_id) =
                                    self.handles.root_registry.resolve_or_identity(&root_pb);
                                let rel_for_open = rel.clone();
                                let intent_copy = intent;
                                let opened = spawn_blocking(move || {
                                    let (flags, mode_bits) = fopen_flags(intent_copy);
                                    super::path::safe_open_verified(
                                        &root_pb,
                                        &rel_for_open,
                                        flags,
                                        mode_bits,
                                        expected_root_id,
                                    )
                                })
                                .await;
                                match opened {
                                    Ok(Ok(f)) => Some(f),
                                    _ => None,
                                }
                            }
                            _ => None,
                        }
                    } else {
                        None
                    };
                    // Same canon_path derivation as `open_impl`
                    // (C-29-1 fix) — must be byte-identical so WAL
                    // paths match across leader/follower.
                    let deploy = *self
                        .handles
                        .current_deploy_scope
                        .read()
                        .expect("current_deploy_scope RwLock poisoned");
                    let shadow = FileHandle {
                        file,
                        // M-R2: same lexical normalization as the leader's
                        // open_impl so follower's WAL entries match.
                        canon_path: canonicalize_lexical(&root, &rel),
                        mode: intent.map(|i| i.mode).unwrap_or(AccessMode::Read),
                        cmode,
                        // Shadow position starts at 0 — POSIX default for
                        // O_RDONLY/O_WRONLY/O_RDWR without O_APPEND.  The
                        // leader's open_impl below rejects Consensus +
                        // append modes at the args-check, so on the
                        // replay branch we never see a shadow handle
                        // whose real fd was O_APPEND.
                        position: 0,
                        // Deploy-end sweep (2026-09-02): shadow handles
                        // also carry the deploy scope so the follower's
                        // sweep symmetrically drops shadows the leader
                        // dropped.
                        deploy,
                    };
                    // Ignore the return value: if the slot is already
                    // occupied (shouldn't happen on a fresh follower),
                    // the pre-existing handle wins.  Any real
                    // divergence surfaces later via WAL mismatch.
                    let _ = self.handles.insert_at(fd.as_u64(), shadow).await;
                }
            }
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let cmode = match resolve_cmode(cmode_par) {
            Some(m) => m,
            None => {
                let out = vec![err(
                    FSERR_BAD_ARG,
                    "cmode must be String \"oracular\" or \"consensus\"",
                )];
                produce(&out, ack).await?;
                return Ok(out);
            }
        };
        let reply = match (
            RhoString::unapply(root_par),
            RhoString::unapply(rel_par),
            RhoString::unapply(mode_par),
        ) {
            (Some(root), Some(rel), Some(mode_str)) => {
                self.open_impl(root, rel, mode_str, cmode).await
            }
            _ => err(FSERR_BAD_ARG, "expected (String, String, String, String)"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    async fn open_impl(
        &self,
        root: String,
        rel: String,
        mode: String,
        cmode: ConsensusMode,
    ) -> Par {
        let intent = match parse_open_mode(&mode) {
            Some(i) => i,
            None => return err(FSERR_BAD_ARG, format!("unknown fopen mode {mode:?}")),
        };
        // Consensus caps + O_APPEND is not supported (see
        // `FileHandle::position` docstring).  O_APPEND writes are
        // atomically retargeted to file-end by the kernel; the
        // shadow-position model that lets sequential writes record
        // deterministic absolute offsets in the WAL doesn't extend
        // cleanly to O_APPEND without per-canon_path EOF simulation
        // on the follower.  Rather than silently produce a WAL that
        // followers can't replay, reject at open time.  Consensus
        // authors should use non-append modes + explicit `fs_seek`
        // if they need append-like behavior.
        if cmode == ConsensusMode::Consensus && intent.append {
            return err(
                FSERR_BAD_ARG,
                "append modes (\"a\", \"a+\") are not supported on Consensus caps — \
                 use a non-append mode plus fs_seek(SEEK_END) if append semantics \
                 are required, or open the cap as Oracular",
            );
        }
        let root_pb = PathBuf::from(&root);
        // Shape A (2026-08-31): route the caller's `root` through
        // the per-runtime `RootIdentityRegistry`.  For legacy
        // (Oracular) bundles the resolver's fall-through returns
        // the input path unchanged; for Consensus bundles under
        // Shape A, the emitted logical `/@bundle/...` root remaps
        // to the validator's on-disk staging dir + the boot-
        // captured `(dev, inode)` identity.  Passing both to
        // `safe_open_verified` preserves the H-5 rename-and-
        // recreate defense at open time (previously fs_open
        // silently skipped identity verification — a pre-existing
        // gap surfaced by Shape A's landing).
        let (root_pb, expected_root_id) = self.handles.root_registry.resolve_or_identity(&root_pb);
        let intent_copy = intent;
        // C-29-1 review fix: keep `rel` accessible for canon_path
        // construction below.  Clone into the blocking closure and
        // retain the original for later use.
        let rel_for_open = rel.clone();
        // openat descent + safe_open in a blocking task — sync fs.
        let opened = spawn_blocking(move || {
            let (flags, mode_bits) = fopen_flags(intent_copy);
            super::path::safe_open_verified(
                &root_pb,
                &rel_for_open,
                flags,
                mode_bits,
                expected_root_id,
            )
        })
        .await;
        let file = match opened {
            Err(_join_err) => return err(FSERR_IO, "spawn_blocking task failed"),
            Ok(Err(qe)) => {
                let (code, msg) = quarantine_err_reply(&qe);
                return err(code, msg);
            }
            Ok(Ok(f)) => f,
        };
        // Reject non-regular files via fstat on the opened fd.  Because
        // we already have the fd (opened with O_NOFOLLOW), there's no
        // TOCTOU here.
        let meta = match file.metadata() {
            Ok(m) => m,
            Err(e) => return err(io_err_code(&e), io_msg_scrub(&e)),
        };
        if !meta.file_type().is_file() {
            return err(FSERR_UNSUPPORTED, "not a regular file");
        }
        // C-29-1 review fix: include the resolved `rel` in the
        // canonical path so WAL entries can distinguish files under
        // the same canonRoot.  Pre-fix the `.join("")` no-op dropped
        // `rel` entirely, causing every WAL entry to record only the
        // canonRoot — replay had no way to tell which file to apply
        // the payload to.
        let deploy = *self
            .handles
            .current_deploy_scope
            .read()
            .expect("current_deploy_scope RwLock poisoned");
        let handle = FileHandle {
            file: Some(file),
            // M-R2 round-2 fix: lexically normalize so `a/b.txt` and
            // `./a/b.txt` produce byte-identical canon_paths, keeping
            // WAL entries stable across equivalent rel forms.
            canon_path: canonicalize_lexical(&root, &rel),
            mode: intent.mode,
            cmode,
            // Shadow position starts at 0 for all supported modes.
            // Non-append modes (r/rw/w/w+/wx/w+x) all leave the fd at
            // position 0 after open.  Append modes are rejected above
            // for Consensus; for Oracular they're allowed but no WAL
            // consumer reads `position`, so 0 is a safe default (the
            // kernel handles O_APPEND retargeting at write time, and
            // our sequential-write path doesn't consult `position`
            // when the WAL journal is a no-op).
            position: 0,
            // Deploy-end sweep (2026-09-02): capture the scope so
            // FileHandleTable::close_all_for_deploy can identify this
            // file as belonging to the ending deploy.  Read from the
            // per-runtime current_deploy_scope cell, populated by
            // WalDeployScope::new_with_lock_sweep at deploy entry.
            deploy,
        };
        match self.handles.insert(handle).await {
            Ok(fd) => ok_u64(fd),
            Err(()) => err(FSERR_QUOTA_EXCEEDED, "per-runtime fd cap reached"),
        }
    }

    // -------------------------------------------------------------------
    // close — (fd) -> [true]
    // -------------------------------------------------------------------
    pub async fn fs_close(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        // Phase 9 slice 9b-ii: charge fs_close weight at handler entry.
        // See fs_open for the rationale on placement before unapply.
        self.metering.reserve_primitive(costs::fs_close_cost())?;
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_close"));
        };
        let [fd_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_close"));
        };
        if is_replay {
            // Phase 2 fd-release (Consensus re-execute + verify,
            // 2026-09-01): the follower's fs_open replay branch now
            // installs a shadow whose `file` slot backs a REAL OS
            // fd under Consensus (needed for downstream fd-based
            // re-execute — fs_size / fs_read / fs_write / etc.).
            // Pre-Phase-2 the shadow was metadata-only (`file:
            // None`) so this branch could produce cached reply
            // without touching the fd table; now it MUST release
            // the shadow at close time, or the follower would
            // accumulate OS fds up to MAX_OPEN_FDS across the
            // runtime's lifetime — a validator processing many
            // blocks with Consensus fs traffic would eventually
            // hit FSERR_QUOTA_EXCEEDED on fs_open replay.  The
            // remove is a pure side-effect: it doesn't touch the
            // reply Par (still the cached leader reply) nor the
            // WAL (fs_close doesn't journal).  Symmetric with the
            // leader path below.
            if let Some(fd) = RhoNumber::unapply(fd_par) {
                self.handles.remove(fd as u64).await;
            }
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match RhoNumber::unapply(fd_par) {
            Some(fd) => {
                // Slice 28 (post-2026-08-06 CRIT-2 fix): fds are
                // hash-derived u64 bit-patterns; the sign bit
                // carries information, so we reinterpret the GInt
                // via `fd as u64` rather than gating on `fd >= 0`.
                self.handles.remove(fd as u64).await;
                ok_bare()
            }
            _ => err(FSERR_BAD_ARG, "expected GInt fd"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // read — (fd, n) -> [true, bytes]
    //
    // Slice 32 (PB-M-14 read-hash): on Consensus caps, `journal_read`
    // appends a `Read` WAL entry whose `payload_ref = Hash(bytes)`
    // AFTER the leader's syscall completes successfully.  The
    // `is_replay = true` follower branch symmetrically re-extracts
    // the bytes from `previous` and appends the SAME entry, so the
    // per-deploy WAL is byte-identical across leader and follower.
    // The follower does NOT re-execute the syscall — the tuplespace
    // `previous` cache already supplies the correct return value.
    // Non-Consensus caps skip journaling on both sides.
    // -------------------------------------------------------------------
    pub async fn fs_read(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_read"));
        };
        let [fd_par, n_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_read"));
        };
        // Phase 9 slice 9b-iv: charge fs_read cost based on the
        // REQUESTED byte count.  Per costs.rs::fs_read_cost docstring,
        // the requested count is what's charged (not actually-returned)
        // so an EOF-truncated read still burns the intended bytes,
        // closing the pre-seek-past-EOF cost-amplification vector.
        // The count is a deterministic function of `n_par` (a user-
        // supplied GInt), which is identical across leader and replay.
        // A non-parseable or negative n_par yields 0 requested bytes,
        // which still burns FS_SYSCALL_CONST = 100 for the dispatch.
        let requested_bytes: u64 = RhoNumber::unapply(n_par)
            .and_then(|n| u64::try_from(n).ok())
            .unwrap_or(0);
        // Cost-helper audit (2026-08-26): `fs_read_cost` docstring
        // requires `reserve_incremental_primitive` because
        // `requested_bytes` can legitimately be 0 (EOF-truncated
        // read, or non-parseable n_par).  Current base
        // `FS_SYSCALL_CONST = 100` guarantees positivity, so the
        // switch is a no-op for today's inputs; defense-in-depth
        // against a future coefficient change that could drop the
        // base to 0.
        self.metering
            .reserve_incremental_primitive(costs::fs_read_cost(requested_bytes))?;
        // Phase 2 (Consensus re-execute + verify, 2026-09-01):
        // dispatch is_replay on the fd's cmode.  Oracular / no-
        // shadow → unchanged Phase-0 tautological path (cached
        // reply, journal + advance shadow position by cached
        // bytes' length).  Consensus follower → re-execute
        // libc::read via the shadow's real fd (installed by
        // fs_open's Phase-2 real-open), verify vs cached, journal
        // fresh + advance shadow on match, emit divergence-err +
        // Failure WAL on mismatch.  Same shape as fs_read_at with
        // the addition that fs_read (sequential) advances the
        // shadow position; fs_read_at (positional) doesn't.
        let jmode: Option<ConsensusMode> = match RhoNumber::unapply(fd_par) {
            Some(fd) => self.handles.with_mut(fd as u64, |h| h.cmode).await,
            None => None,
        };
        if is_replay && jmode != Some(ConsensusMode::Consensus) {
            // Oracular / no-shadow follower — unchanged Phase-0 path.
            if let (Some(fd), Some(bytes)) =
                (RhoNumber::unapply(fd_par), extract_ok_bytes(&previous))
            {
                // Order matters: journal_read reads the PRE-read
                // shadow position from FileHandle; then the
                // subsequent with_mut advances position by
                // bytes.len().  If we advanced first, journal_read
                // would record the post-read offset which is
                // wrong.  Mirrors the leader path below.
                let fd_u = fd as u64;
                let _ = self.journal_read(fd_u, &bytes, None, ack).await;
                let n = bytes.len() as u64;
                let _ = self
                    .handles
                    .with_mut(fd_u, |h| h.position = h.position.saturating_add(n))
                    .await;
            }
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        // Fresh syscall — leader (always) or Consensus follower.
        // read_impl calls libc::read (offset = None) against the
        // shadow's real raw_fd.  The follower's own libc::read
        // advances the OS-level fd position by the returned byte
        // count; the shadow position advance below keeps our
        // in-process shadow in sync with the OS position, matching
        // what the leader's play run did on its own OS fd.
        let fresh_reply = match (RhoNumber::unapply(fd_par), RhoNumber::unapply(n_par)) {
            (Some(fd), Some(n)) if n >= 0 => self.read_impl(fd as u64, n as u64, None).await,
            _ => err(FSERR_BAD_ARG, "expected (fd:GInt, n:GInt>=0)"),
        };
        if is_replay {
            // Consensus follower — Phase 2 re-execute + verify.
            match verify_reply_hash_matches_cached(&fresh_reply, &previous) {
                Ok(()) => {
                    // Match — journal fresh bytes + advance shadow.
                    // Bytes hash to the same Blake2b256 as leader's
                    // WAL entry (verify succeeded → Pars are byte-
                    // identical), so WAL byte-identity is preserved.
                    if let (Some(fd), Some(bytes)) = (
                        RhoNumber::unapply(fd_par),
                        extract_ok_bytes(std::slice::from_ref(&fresh_reply)),
                    ) {
                        let fd_u = fd as u64;
                        let _ = self.journal_read(fd_u, &bytes, None, ack).await;
                        let n = bytes.len() as u64;
                        let _ = self
                            .handles
                            .with_mut(fd_u, |h| h.position = h.position.saturating_add(n))
                            .await;
                    }
                    let out = vec![fresh_reply];
                    produce(&out, ack).await?;
                    Ok(out)
                }
                Err(reason) => {
                    // Divergence — emit divergence-err + Failure WAL
                    // via the shared journal_read_divergence helper
                    // (offset=None → WalOp::Read).  Shadow position:
                    // advance by the FRESH read's byte count (the
                    // follower's OS-level libc::read already
                    // advanced the OS position by that count, so
                    // this keeps shadow-vs-OS position in sync on
                    // the follower).  Subsequent ops in the failing
                    // deploy don't affect correctness — the deploy
                    // will be rejected at check_replay_data — but
                    // matching OS position is cleaner than skipping
                    // the advance.
                    let divergence_reply = err(
                        FSERR_CONSENSUS_DIVERGENCE,
                        format!("fs_read follower re-execute diverges from leader: {reason}",),
                    );
                    if let Some(fd) = RhoNumber::unapply(fd_par) {
                        let fd_u = fd as u64;
                        let _ = self.journal_read_divergence(fd_u, None, ack).await;
                        if let Some(bytes) = extract_ok_bytes(std::slice::from_ref(&fresh_reply)) {
                            let n = bytes.len() as u64;
                            let _ = self
                                .handles
                                .with_mut(fd_u, |h| h.position = h.position.saturating_add(n))
                                .await;
                        }
                    }
                    let out = vec![divergence_reply];
                    produce(&out, ack).await?;
                    Ok(out)
                }
            }
        } else {
            // Leader path — journal fresh bytes + advance shadow.
            if let Some(bytes) = extract_ok_bytes(std::slice::from_ref(&fresh_reply)) {
                if let Some(fd) = RhoNumber::unapply(fd_par) {
                    let fd_u = fd as u64;
                    let _ = self.journal_read(fd_u, &bytes, None, ack).await;
                    let n = bytes.len() as u64;
                    let _ = self
                        .handles
                        .with_mut(fd_u, |h| h.position = h.position.saturating_add(n))
                        .await;
                }
            }
            let out = vec![fresh_reply];
            produce(&out, ack).await?;
            Ok(out)
        }
    }

    // -------------------------------------------------------------------
    // readAt — (fd, offset, n) -> [true, bytes]
    //
    // Slice 32 (PB-M-14 read-hash): see `fs_read` docstring.  Same
    // leader/follower journal_read pattern, with offset populated.
    // -------------------------------------------------------------------
    pub async fn fs_read_at(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_read_at"));
        };
        let [fd_par, off_par, n_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_read_at"));
        };
        // Phase 9 slice 9b-iv: charge fs_read_at cost based on the
        // REQUESTED byte count.  See fs_read for rationale.
        let requested_bytes: u64 = RhoNumber::unapply(n_par)
            .and_then(|n| u64::try_from(n).ok())
            .unwrap_or(0);
        self.metering
            .reserve_incremental_primitive(costs::fs_read_at_cost(requested_bytes))?;
        // Phase 2 (Consensus re-execute + verify, 2026-09-01):
        // Look up the fd's cmode so we can dispatch is_replay
        // to Oracular (unchanged Phase-0 tautological path) vs
        // Consensus (re-execute + verify).  A None cmode (missing
        // shadow, e.g., fd was closed or Rholang passed a bad fd)
        // folds into the Oracular branch — pre-Phase-2 behavior
        // for the tautological path, matching fs_size's dispatch.
        let jmode: Option<ConsensusMode> = match RhoNumber::unapply(fd_par) {
            Some(fd) => self.handles.with_mut(fd as u64, |h| h.cmode).await,
            None => None,
        };
        if is_replay && jmode != Some(ConsensusMode::Consensus) {
            // Oracular / no-shadow follower — unchanged Phase-0 path.
            // journal_read self-guards on Consensus so this is a WAL
            // no-op for Oracular; kept for structural parity.
            if let (Some(fd), Some(off), Some(bytes)) = (
                RhoNumber::unapply(fd_par),
                RhoNumber::unapply(off_par),
                extract_ok_bytes(&previous),
            ) {
                if off >= 0 {
                    let _ = self
                        .journal_read(fd as u64, &bytes, Some(off as u64), ack)
                        .await;
                }
            }
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        // Fresh syscall — leader (always) or Consensus follower
        // (Phase-2 re-execute).  read_impl calls libc::pread against
        // the shadow's real raw_fd (populated on the follower by
        // fs_open's Phase-2 real-open under Consensus).
        let fresh_reply = match (
            RhoNumber::unapply(fd_par),
            RhoNumber::unapply(off_par),
            RhoNumber::unapply(n_par),
        ) {
            (Some(fd), Some(off), Some(n)) if off >= 0 && n >= 0 => {
                self.read_impl(fd as u64, n as u64, Some(off as u64)).await
            }
            _ => err(FSERR_BAD_ARG, "expected (fd:GInt, off:GInt>=0, n:GInt>=0)"),
        };
        if is_replay {
            // Consensus follower — Phase 2 re-execute + verify.
            match verify_reply_hash_matches_cached(&fresh_reply, &previous) {
                Ok(()) => {
                    // Match — journal the FRESH bytes.  Since verify
                    // succeeded, stable_hash(fresh_reply) ==
                    // stable_hash(cached_reply) → the [true, bytes]
                    // Pars are byte-identical → the raw bytes hash to
                    // the same Blake2b256 the leader's WAL recorded,
                    // preserving WAL byte-identity post-verification.
                    if let (Some(fd), Some(off), Some(bytes)) = (
                        RhoNumber::unapply(fd_par),
                        RhoNumber::unapply(off_par),
                        extract_ok_bytes(std::slice::from_ref(&fresh_reply)),
                    ) {
                        if off >= 0 {
                            let _ = self
                                .journal_read(fd as u64, &bytes, Some(off as u64), ack)
                                .await;
                        }
                    }
                    let out = vec![fresh_reply];
                    produce(&out, ack).await?;
                    Ok(out)
                }
                Err(reason) => {
                    // Divergence — journal a Failure WAL entry via
                    // the shared helper.  See journal_read_divergence
                    // for the field-shape rationale (payload_ref +
                    // length are None because the divergence-err
                    // reply carries no bytes).
                    let divergence_reply = err(
                        FSERR_CONSENSUS_DIVERGENCE,
                        format!("fs_read_at follower re-execute diverges from leader: {reason}",),
                    );
                    if let (Some(fd), Some(off)) =
                        (RhoNumber::unapply(fd_par), RhoNumber::unapply(off_par))
                    {
                        if off >= 0 {
                            let _ = self
                                .journal_read_divergence(fd as u64, Some(off as u64), ack)
                                .await;
                        }
                    }
                    let out = vec![divergence_reply];
                    produce(&out, ack).await?;
                    Ok(out)
                }
            }
        } else {
            // Leader path — journal fresh bytes, produce fresh reply.
            if let Some(bytes) = extract_ok_bytes(std::slice::from_ref(&fresh_reply)) {
                if let (Some(fd), Some(off)) =
                    (RhoNumber::unapply(fd_par), RhoNumber::unapply(off_par))
                {
                    if off >= 0 {
                        let _ = self
                            .journal_read(fd as u64, &bytes, Some(off as u64), ack)
                            .await;
                    }
                }
            }
            let out = vec![fresh_reply];
            produce(&out, ack).await?;
            Ok(out)
        }
    }

    async fn read_impl(&self, fd: u64, n: u64, offset: Option<u64>) -> Par {
        if n > super::MAX_READ_BYTES {
            return err(
                FSERR_QUOTA_EXCEEDED,
                format!("read {n} exceeds MAX_READ_BYTES"),
            );
        }
        // We can't move a `&mut FileHandle` into spawn_blocking (it lives
        // behind an RwLock owned by `handles`).  Instead: take the fd's
        // raw fd, do the syscall on a blocking task, and let `File` be
        // reconstructed from the handle table on the next call.  We use
        // libc::pread directly so we don't need &mut File.
        let raw_fd = match self.handles.raw_fd(fd).await {
            Some(rfd) => rfd,
            None => return err(FSERR_CLOSED, format!("unknown fd {fd}")),
        };
        let result = spawn_blocking(move || {
            let mut buf = vec![0u8; n as usize];
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
        })
        .await;
        match result {
            Err(_join_err) => err(FSERR_IO, "spawn_blocking task failed"),
            Ok(Err(e)) => err(io_err_code(&e), io_msg_scrub(&e)),
            Ok(Ok(bytes)) => ok_bytes(bytes),
        }
    }

    // -------------------------------------------------------------------
    // write — (fd, bytes) -> [true, nWritten]
    // -------------------------------------------------------------------
    pub async fn fs_write(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_write"));
        };
        let [fd_par, bytes_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_write"));
        };
        // Parse args deterministically for both leader and follower.
        let parsed = match (RhoNumber::unapply(fd_par), RhoByteArray::unapply(bytes_par)) {
            (Some(fd), Some(bytes)) => Some((fd as u64, bytes)),
            _ => None,
        };
        // Phase 9 slice 9b-iv: charge fs_write cost based on the
        // requested byte count (bytes.len()).  Deterministic across
        // leader and replay: `bytes_par` is a user-supplied
        // GByteArray argument, identical on both sides.  Charge fires
        // BEFORE the WAL-cap check and journal_write below so a
        // MAX_WRITE_BYTES-oversized request still burns the dispatch
        // cost + linear-per-byte cost the user intended to spend
        // (matches the pre-charge boundary discipline in
        // costs.rs::fs_read_cost).
        let requested_bytes: u64 = parsed
            .as_ref()
            .map(|(_, bytes)| bytes.len() as u64)
            .unwrap_or(0);
        // Cost-helper audit (2026-08-26): mirrors fs_read audit fix —
        // `requested_bytes` can be 0 (empty write) so use incremental.
        self.metering
            .reserve_incremental_primitive(costs::fs_write_cost(requested_bytes))?;
        // M-R3 review fix (round 2): enforce MAX_WRITE_BYTES BEFORE
        // journaling so an oversized write cannot consume a WAL slot
        // for a call that will error out.  Mirror fs_truncate's
        // pre-check-then-journal ordering (which was already correct).
        if let Some((_, bytes)) = &parsed {
            if bytes.len() as u64 > MAX_WRITE_BYTES {
                let out = vec![err(
                    FSERR_QUOTA_EXCEEDED,
                    format!("write {} exceeds MAX_WRITE_BYTES", bytes.len()),
                )];
                produce(&out, ack).await?;
                return Ok(out);
            }
        }
        // C-29-F1 review fix: journal to WAL on BOTH leader and
        // follower — before the `is_replay` short-circuit — so both
        // sides populate identical WALs.  Populated purely from
        // args (path via fd lookup, payload hash of requested
        // bytes).  A cap-exceeded return here is deterministic
        // (both sides hit the same cap moment) and produces a
        // symmetric FSERR_QUOTA_EXCEEDED reply that is cached in
        // `previous` on the leader and replayed on the follower.
        if let Some((fd, bytes)) = &parsed {
            if self.journal_write(*fd, bytes, None, ack).await.is_err() {
                let out = vec![err(FSERR_QUOTA_EXCEEDED, "WAL cap exceeded")];
                produce(&out, ack).await?;
                return Ok(out);
            }
        }
        // Phase 3 (Consensus re-execute + verify, 2026-09-01):
        // Under D2 (deploy source re-evaluation for bytes), the
        // follower's `parsed` bytes are byte-identical to leader's
        // — same Rholang deploy term, same GByteArray arg.  The
        // Consensus follower re-executes the real `libc::write` via
        // its shadow's real fd (installed by fs_open's Phase-2
        // real-open), verifies the fresh reply's stable_hash matches
        // leader's cached, and finalizes the WAL entry from the
        // fresh reply on match (byte-identical to leader's finalize
        // by construction) or flips to Failure { CONSENSUS_DIVERGENCE }
        // on mismatch.  Oracular follower is unchanged from Phase-0
        // H-6 tautological finalize path.
        let jmode: Option<ConsensusMode> = match &parsed {
            Some((fd, _)) => self.handles.with_mut(*fd, |h| h.cmode).await,
            None => None,
        };
        if is_replay && jmode != Some(ConsensusMode::Consensus) {
            // Oracular / no-shadow follower — Phase-0 H-6 shape.
            if let (Some((fd, bytes)), Some(n)) = (&parsed, extract_ok_u64(&previous)) {
                if n < bytes.len() as u64 {
                    self.finalize_write_journal(bytes, n, ack);
                }
                // Position-follow-up (2026-08-26): advance shadow
                // position by ACTUAL bytes written (sequential
                // write, so pwrite semantics don't apply here —
                // this is fs_write, not fs_write_at).  Mirrors the
                // leader path below so both sides evolve position
                // identically.  A partial write (n < requested)
                // advances by n; a failed write (previous is an
                // error reply) advances by 0 (n=None from
                // extract_ok_u64).
                let fd_copy = *fd;
                let _ = self
                    .handles
                    .with_mut(fd_copy, |h| h.position = h.position.saturating_add(n))
                    .await;
            }
            // H-6 fix (2026-08-06): if the leader's cached reply
            // was an error, follower flips the placeholder to
            // Failure { code }.  Symmetric with the leader-path
            // finalize below: same code → same WAL entry.
            if let Some(code_str) = extract_err_code(&previous) {
                self.finalize_failure_journal(fserr_to_code(&code_str), ack);
            }
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        // Fresh syscall — leader (always) or Consensus follower
        // (Phase 3 re-execute).  Real `libc::write` via the shadow's
        // real raw_fd.  On the follower under D3 per-validator
        // subdirs, this writes to the follower's OWN subdir file
        // (making the file bytes match the leader's — the load-
        // bearing outcome for Canary 1's Phase-3 completion signal).
        let fresh_reply = match parsed.clone() {
            Some((fd, bytes)) => self.write_impl(fd, bytes, None).await,
            None => err(FSERR_BAD_ARG, "expected (u64, ByteArray)"),
        };
        if is_replay {
            // Consensus follower — Phase 3 verify.
            match verify_reply_hash_matches_cached(&fresh_reply, &previous) {
                Ok(()) => {
                    // Match — reply is byte-identical to cached, so
                    // the fresh finalize is symmetric with what the
                    // leader did.  Finalize partial-write / failure
                    // from FRESH; advance shadow by FRESH n.  WAL
                    // byte-identity preserved by construction of
                    // verify OK (Pars byte-identical → same n → same
                    // finalize_write_journal update).
                    if let (Some((fd, bytes)), Some(n)) =
                        (&parsed, extract_ok_u64(std::slice::from_ref(&fresh_reply)))
                    {
                        if n < bytes.len() as u64 {
                            self.finalize_write_journal(bytes, n, ack);
                        }
                        let fd_copy = *fd;
                        let _ = self
                            .handles
                            .with_mut(fd_copy, |h| h.position = h.position.saturating_add(n))
                            .await;
                    }
                    if let Some(code_str) = extract_err_code(std::slice::from_ref(&fresh_reply)) {
                        self.finalize_failure_journal(fserr_to_code(&code_str), ack);
                    }
                    let out = vec![fresh_reply];
                    produce(&out, ack).await?;
                    Ok(out)
                }
                Err(reason) => {
                    // Divergence — flip pre-appended Success
                    // placeholder to Failure { CONSENSUS_DIVERGENCE }.
                    // Advance shadow by FRESH n (if the fresh syscall
                    // did write some bytes) to keep shadow-vs-OS
                    // position in sync on the follower — same
                    // rationale as fs_read's divergence-path shadow
                    // advance.  Deploy fails via RSpace rig regardless,
                    // so subsequent ops are moot; sync is for
                    // invariant cleanliness.
                    if let (Some((fd, _)), Some(m)) =
                        (&parsed, extract_ok_u64(std::slice::from_ref(&fresh_reply)))
                    {
                        let fd_copy = *fd;
                        let _ = self
                            .handles
                            .with_mut(fd_copy, |h| h.position = h.position.saturating_add(m))
                            .await;
                    }
                    let divergence_reply = err(
                        FSERR_CONSENSUS_DIVERGENCE,
                        format!(
                            "fs_write follower re-execute diverges from leader: \
                             {reason}",
                        ),
                    );
                    self.finalize_failure_journal(FSERR_CODE_CONSENSUS_DIVERGENCE, ack);
                    let out = vec![divergence_reply];
                    produce(&out, ack).await?;
                    Ok(out)
                }
            }
        } else {
            // Leader path.  Slice 30c M-29-3 + H-6 finalize.
            if let (Some((fd, bytes)), Some(n)) =
                (&parsed, extract_ok_u64(std::slice::from_ref(&fresh_reply)))
            {
                if n < bytes.len() as u64 {
                    tracing::warn!(
                        target: "f1r3fly.fs_wal",
                        fd = fd,
                        requested = bytes.len(),
                        actual = n,
                        "partial write on Consensus cap; finalizing WAL entry with actual bytes"
                    );
                    self.finalize_write_journal(bytes, n, ack);
                }
                let fd_copy = *fd;
                let _ = self
                    .handles
                    .with_mut(fd_copy, |h| h.position = h.position.saturating_add(n))
                    .await;
            }
            if let Some(code_str) = extract_err_code(std::slice::from_ref(&fresh_reply)) {
                self.finalize_failure_journal(fserr_to_code(&code_str), ack);
            }
            let out = vec![fresh_reply];
            produce(&out, ack).await?;
            Ok(out)
        }
    }

    // -------------------------------------------------------------------
    // writeAt — (fd, offset, bytes) -> [true, nWritten]
    // -------------------------------------------------------------------
    pub async fn fs_write_at(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_write_at"));
        };
        let [fd_par, off_par, bytes_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_write_at"));
        };
        let parsed = match (
            RhoNumber::unapply(fd_par),
            RhoNumber::unapply(off_par),
            RhoByteArray::unapply(bytes_par),
        ) {
            (Some(fd), Some(off), Some(bytes)) if off >= 0 => Some((fd as u64, off as u64, bytes)),
            _ => None,
        };
        // Phase 9 slice 9b-iv: charge fs_write_at cost based on the
        // requested byte count.  See fs_write for rationale.
        let requested_bytes: u64 = parsed
            .as_ref()
            .map(|(_, _, bytes)| bytes.len() as u64)
            .unwrap_or(0);
        self.metering
            .reserve_incremental_primitive(costs::fs_write_at_cost(requested_bytes))?;
        // M-R3 review fix (round 2): MAX_WRITE_BYTES check BEFORE
        // journaling; see fs_write for rationale.
        if let Some((_, _, bytes)) = &parsed {
            if bytes.len() as u64 > MAX_WRITE_BYTES {
                let out = vec![err(
                    FSERR_QUOTA_EXCEEDED,
                    format!("write {} exceeds MAX_WRITE_BYTES", bytes.len()),
                )];
                produce(&out, ack).await?;
                return Ok(out);
            }
        }
        // C-29-F1 review fix: journal to WAL on both leader and follower
        // before the `is_replay` short-circuit (see `fs_write` for the
        // full rationale).
        if let Some((fd, off, bytes)) = &parsed {
            if self
                .journal_write(*fd, bytes, Some(*off), ack)
                .await
                .is_err()
            {
                let out = vec![err(FSERR_QUOTA_EXCEEDED, "WAL cap exceeded")];
                produce(&out, ack).await?;
                return Ok(out);
            }
        }
        // Phase 3 (Consensus re-execute + verify, 2026-09-01):
        // Same shape as fs_write but positional — no shadow position
        // advance on either side (pwrite doesn't advance OS-fd
        // position).  Oracular follower unchanged (H-6 finalize
        // path).  Consensus follower re-executes libc::pwrite via
        // shadow's real fd + verify + finalize from fresh reply on
        // match / divergence-err on mismatch.
        let jmode: Option<ConsensusMode> = match &parsed {
            Some((fd, _, _)) => self.handles.with_mut(*fd, |h| h.cmode).await,
            None => None,
        };
        if is_replay && jmode != Some(ConsensusMode::Consensus) {
            // Oracular / no-shadow follower — Phase-0 H-6 shape.
            if let (Some((_fd, _off, bytes)), Some(n)) = (&parsed, extract_ok_u64(&previous)) {
                if n < bytes.len() as u64 {
                    self.finalize_write_journal(bytes, n, ack);
                }
            }
            if let Some(code_str) = extract_err_code(&previous) {
                self.finalize_failure_journal(fserr_to_code(&code_str), ack);
            }
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        // Fresh syscall — leader (always) or Consensus follower.
        // libc::pwrite at the specified offset via shadow's real
        // raw_fd.  pwrite does NOT advance the OS-fd position (per
        // POSIX), so no position tracking needed on either side.
        let fresh_reply = match parsed.clone() {
            Some((fd, off, bytes)) => self.write_impl(fd, bytes, Some(off)).await,
            None => err(FSERR_BAD_ARG, "expected (u64, u64, ByteArray)"),
        };
        if is_replay {
            // Consensus follower — Phase 3 verify.
            match verify_reply_hash_matches_cached(&fresh_reply, &previous) {
                Ok(()) => {
                    if let (Some((_fd, _off, bytes)), Some(n)) =
                        (&parsed, extract_ok_u64(std::slice::from_ref(&fresh_reply)))
                    {
                        if n < bytes.len() as u64 {
                            self.finalize_write_journal(bytes, n, ack);
                        }
                    }
                    if let Some(code_str) = extract_err_code(std::slice::from_ref(&fresh_reply)) {
                        self.finalize_failure_journal(fserr_to_code(&code_str), ack);
                    }
                    let out = vec![fresh_reply];
                    produce(&out, ack).await?;
                    Ok(out)
                }
                Err(reason) => {
                    let divergence_reply = err(
                        FSERR_CONSENSUS_DIVERGENCE,
                        format!(
                            "fs_write_at follower re-execute diverges from leader: \
                             {reason}",
                        ),
                    );
                    self.finalize_failure_journal(FSERR_CODE_CONSENSUS_DIVERGENCE, ack);
                    let out = vec![divergence_reply];
                    produce(&out, ack).await?;
                    Ok(out)
                }
            }
        } else {
            // Leader path — H-6 finalize.
            if let (Some((fd, off, bytes)), Some(n)) =
                (&parsed, extract_ok_u64(std::slice::from_ref(&fresh_reply)))
            {
                if n < bytes.len() as u64 {
                    tracing::warn!(
                        target: "f1r3fly.fs_wal",
                        fd = fd,
                        offset = off,
                        requested = bytes.len(),
                        actual = n,
                        "partial write_at on Consensus cap; finalizing WAL entry"
                    );
                    self.finalize_write_journal(bytes, n, ack);
                }
            }
            if let Some(code_str) = extract_err_code(std::slice::from_ref(&fresh_reply)) {
                self.finalize_failure_journal(fserr_to_code(&code_str), ack);
            }
            let out = vec![fresh_reply];
            produce(&out, ack).await?;
            Ok(out)
        }
    }

    async fn write_impl(&self, fd: u64, bytes: Vec<u8>, offset: Option<u64>) -> Par {
        if bytes.len() as u64 > MAX_WRITE_BYTES {
            return err(
                FSERR_QUOTA_EXCEEDED,
                format!("write {} exceeds MAX_WRITE_BYTES", bytes.len()),
            );
        }
        let raw_fd = match self.handles.raw_fd(fd).await {
            Some(rfd) => rfd,
            None => return err(FSERR_CLOSED, format!("unknown fd {fd}")),
        };
        // Redesign note: WAL journaling for Consensus caps happens in
        // `fs_write` / `fs_write_at` BEFORE this function is called,
        // so both leader and follower populate identical WALs
        // (C-29-F1 review fix).  Do NOT append here.
        let result = spawn_blocking(move || {
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
        })
        .await;
        match result {
            Err(_join_err) => err(FSERR_IO, "spawn_blocking task failed"),
            Ok(Err(e)) => err(io_err_code(&e), io_msg_scrub(&e)),
            Ok(Ok(n)) => ok_u64(n),
        }
    }

    // -------------------------------------------------------------------
    // seek — (fd, offset, whence) -> [true, newPos]
    // -------------------------------------------------------------------
    pub async fn fs_seek(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        // Phase 9 slice 9b-ii: charge fs_seek weight at handler entry.
        // See fs_open for the rationale on placement before unapply.
        self.metering.reserve_primitive(costs::fs_seek_cost())?;
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_seek"));
        };
        let [fd_par, off_par, whence_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_seek"));
        };
        if is_replay {
            // Position-follow-up (2026-08-26): the follower must
            // update its shadow position from the leader's cached
            // reply so subsequent sequential fs_write / fs_read
            // journal the correct absolute offset.  Extract the
            // new position from `previous` (leader cached
            // ok_u64(new_pos) on success; error replies leave
            // position unchanged, mirroring POSIX lseek's
            // failure-doesn't-move-position semantics).
            //
            // Phase 3 prerequisite (Consensus re-execute + verify,
            // 2026-09-01): under Consensus, the follower's shadow
            // fd is now backed by a REAL OS fd (installed by
            // fs_open's Phase-2 real-open).  Phase 3's fs_write
            // re-execute advances that OS-fd position via real
            // libc::write; Phase 2's fs_read re-execute reads from
            // it.  For the OS-fd position to track the leader's
            // seek movements, the Consensus follower must ALSO
            // call libc::lseek on the real fd — not just update
            // the shadow tracker.  Otherwise OS position drifts
            // from shadow, and subsequent fs_read re-executes hit
            // wrong offsets and trip spurious CONSENSUS_DIVERGENCE.
            //
            // Oracular follower keeps the shadow-only path
            // (Oracle-mode fd is metadata-only shadow, no real OS
            // fd to seek).  jmode lookup below dispatches.
            if let Some(fd) = RhoNumber::unapply(fd_par) {
                let fd_u = fd as u64;
                let cmode_opt = self.handles.with_mut(fd_u, |h| h.cmode).await;
                if cmode_opt == Some(ConsensusMode::Consensus) {
                    // Real lseek on the shadow's OS fd to keep OS
                    // position tracking leader's play.  Re-execute
                    // the same syscall the leader made — args are
                    // deterministic from the same Rholang deploy.
                    if let (Some(off), Some(w)) =
                        (RhoNumber::unapply(off_par), RhoString::unapply(whence_par))
                    {
                        let whence_code = match w.as_str() {
                            "set" if off >= 0 => Some(libc::SEEK_SET),
                            "cur" => Some(libc::SEEK_CUR),
                            "end" => Some(libc::SEEK_END),
                            _ => None,
                        };
                        if let (Some(whence), Some(raw_fd)) =
                            (whence_code, self.handles.raw_fd(fd_u).await)
                        {
                            let _ =
                                spawn_blocking(move || unsafe { libc::lseek(raw_fd, off, whence) })
                                    .await;
                        }
                    }
                }
            }
            if let (Some(fd), Some(new_pos)) =
                (RhoNumber::unapply(fd_par), extract_ok_u64(&previous))
            {
                let fd_u = fd as u64;
                let _ = self.handles.with_mut(fd_u, |h| h.position = new_pos).await;
            }
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match (
            RhoNumber::unapply(fd_par),
            RhoNumber::unapply(off_par),
            RhoString::unapply(whence_par),
        ) {
            (Some(fd), Some(off), Some(w)) => {
                let whence_code = match w.as_str() {
                    "set" if off >= 0 => Some(libc::SEEK_SET),
                    "cur" => Some(libc::SEEK_CUR),
                    "end" => Some(libc::SEEK_END),
                    _ => None,
                };
                match whence_code {
                    None => err(FSERR_BAD_ARG, "expected whence in {set,cur,end}"),
                    Some(whence) => match self.handles.raw_fd(fd as u64).await {
                        None => err(FSERR_CLOSED, format!("unknown fd {fd}")),
                        Some(raw_fd) => {
                            let r = spawn_blocking(move || unsafe {
                                let pos = libc::lseek(raw_fd, off, whence);
                                if pos < 0 {
                                    Err(std::io::Error::last_os_error())
                                } else {
                                    Ok(pos as u64)
                                }
                            })
                            .await;
                            match r {
                                Err(_je) => err(FSERR_IO, "spawn_blocking task failed"),
                                Ok(Err(e)) => err(io_err_code(&e), io_msg_scrub(&e)),
                                Ok(Ok(pos)) => {
                                    // Position-follow-up: sync shadow
                                    // to real position.  Follower
                                    // mirrors this via the
                                    // extract_ok_u64(&previous) path
                                    // in the is_replay branch above.
                                    let fd_u = fd as u64;
                                    let _ = self.handles.with_mut(fd_u, |h| h.position = pos).await;
                                    ok_u64(pos)
                                }
                            }
                        }
                    },
                }
            }
            _ => err(FSERR_BAD_ARG, "expected (u64, i64, String)"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // tell — (fd) -> [true, pos]
    // -------------------------------------------------------------------
    pub async fn fs_tell(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        // Phase 9 slice 9b-ii: charge fs_tell weight at handler entry.
        // See fs_open for the rationale on placement before unapply.
        self.metering.reserve_primitive(costs::fs_tell_cost())?;
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_tell"));
        };
        let [fd_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_tell"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match RhoNumber::unapply(fd_par) {
            Some(fd) => {
                let raw_fd = match self.handles.raw_fd(fd as u64).await {
                    Some(r) => r,
                    None => {
                        let out = vec![err(FSERR_CLOSED, format!("unknown fd {fd}"))];
                        produce(&out, ack).await?;
                        return Ok(out);
                    }
                };
                let r = spawn_blocking(move || unsafe {
                    let pos = libc::lseek(raw_fd, 0, libc::SEEK_CUR);
                    if pos < 0 {
                        Err(std::io::Error::last_os_error())
                    } else {
                        Ok(pos as u64)
                    }
                })
                .await;
                match r {
                    Err(_je) => err(FSERR_IO, "spawn_blocking task failed"),
                    Ok(Err(e)) => err(io_err_code(&e), io_msg_scrub(&e)),
                    Ok(Ok(pos)) => ok_u64(pos),
                }
            }
            _ => err(FSERR_BAD_ARG, "expected u64"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // size — (fd) -> [true, nBytes]
    // -------------------------------------------------------------------
    pub async fn fs_size(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        // Phase 9 slice 9b-ii: charge fs_size weight at handler entry.
        // See fs_open for the rationale on placement before unapply.
        self.metering.reserve_primitive(costs::fs_size_cost())?;
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_size"));
        };
        let [fd_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_size"));
        };
        // M-5: look up FileHandle cmode + canon_path so both
        // is_replay and leader paths can journal symmetrically.
        // If the fd isn't valid at lookup time, the syscall
        // will produce FSERR_CLOSED and journal-cmode is None
        // (no journaling happens).
        let (jmode, jpath): (Option<ConsensusMode>, Option<PathBuf>) =
            match RhoNumber::unapply(fd_par) {
                Some(fd) => {
                    let meta = self
                        .handles
                        .with_mut(fd as u64, |h| (h.cmode, h.canon_path.clone()))
                        .await;
                    match meta {
                        Some((c, p)) => (Some(c), Some(p)),
                        None => (None, None),
                    }
                }
                None => (None, None),
            };
        // Phase 2 (Consensus re-execute + verify, 2026-09-01):
        // Oracular follower unchanged from M-5's Phase-0 behavior —
        // consumes the leader's cached reply, since Oracle-mode state
        // isn't reproducible on the follower's own fs.  Consensus
        // follower now re-executes fstat against its own shadow fd
        // and verifies the fresh reply's stable_hash matches the
        // leader's cached reply hash extracted from `previous`.  See
        // auto-memory `fileio_wal_replay_verification_gap.md` and
        // fs_stat's Phase-1 refactor above for the design pattern.
        if is_replay && jmode != Some(ConsensusMode::Consensus) {
            // Oracular follower (or unresolved cmode — the fd shadow
            // wasn't installed, matches pre-Phase-2 tautological path).
            if let (Some(mode), Some(p)) = (jmode, jpath.clone()) {
                if let Some(reply_par) = previous.first() {
                    self.journal_state_read(mode, WalOp::Size, p, reply_par, ack, None);
                }
            }
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        // Fresh syscall reply — leader (always) and Consensus follower.
        // Pre-Phase-2 the leader path used an early-return on
        // raw_fd = None (before journal); folded into the fresh-
        // reply match arm below.  The pre-Phase-2 non-journal
        // behavior is preserved by the (jmode, jpath) guard on
        // `journal_state_read` — a missing shadow-fd yields
        // jmode = None so the journal call is a no-op regardless.
        let fresh_reply = match RhoNumber::unapply(fd_par) {
            Some(fd) => match self.handles.raw_fd(fd as u64).await {
                Some(raw_fd) => {
                    let r = spawn_blocking(move || unsafe {
                        let mut sb: libc::stat = std::mem::zeroed();
                        if libc::fstat(raw_fd, &mut sb) < 0 {
                            Err(std::io::Error::last_os_error())
                        } else {
                            Ok(sb.st_size as u64)
                        }
                    })
                    .await;
                    match r {
                        Err(_je) => err(FSERR_IO, "spawn_blocking task failed"),
                        Ok(Err(e)) => err(io_err_code(&e), io_msg_scrub(&e)),
                        Ok(Ok(n)) => ok_u64(n),
                    }
                }
                None => err(FSERR_CLOSED, format!("unknown fd {fd}")),
            },
            _ => err(FSERR_BAD_ARG, "expected u64"),
        };
        if is_replay {
            // Consensus follower — Phase 2 re-execute + verify.
            match verify_reply_hash_matches_cached(&fresh_reply, &previous) {
                Ok(()) => {
                    if let (Some(mode), Some(p)) = (jmode, jpath) {
                        self.journal_state_read(mode, WalOp::Size, p, &fresh_reply, ack, None);
                    }
                    let out = vec![fresh_reply];
                    produce(&out, ack).await?;
                    Ok(out)
                }
                Err(reason) => {
                    let divergence_reply = err(
                        FSERR_CONSENSUS_DIVERGENCE,
                        format!("fs_size follower re-execute diverges from leader: {reason}",),
                    );
                    if let (Some(mode), Some(p)) = (jmode, jpath) {
                        self.journal_state_read(mode, WalOp::Size, p, &divergence_reply, ack, None);
                    }
                    let out = vec![divergence_reply];
                    produce(&out, ack).await?;
                    Ok(out)
                }
            }
        } else {
            // Leader path — journal fresh reply, produce it.
            if let (Some(mode), Some(p)) = (jmode, jpath) {
                self.journal_state_read(mode, WalOp::Size, p, &fresh_reply, ack, None);
            }
            let out = vec![fresh_reply];
            produce(&out, ack).await?;
            Ok(out)
        }
    }

    // -------------------------------------------------------------------
    // truncate — (fd, n) -> [true]
    // -------------------------------------------------------------------
    pub async fn fs_truncate(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        // Phase 9 slice 9b-ii: charge fs_truncate weight at handler entry.
        // See fs_open for the rationale on placement before unapply.
        self.metering.reserve_primitive(costs::fs_truncate_cost())?;
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_truncate"));
        };
        let [fd_par, n_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_truncate"));
        };
        let parsed = match (RhoNumber::unapply(fd_par), RhoNumber::unapply(n_par)) {
            (Some(fd), Some(n)) if n >= 0 => Some((fd as u64, n as u64)),
            _ => None,
        };
        // C-29-F1 review fix: journal to WAL on both leader and
        // follower before the `is_replay` short-circuit.  We do the
        // MAX_TRUNCATE_BYTES check first so an oversize truncate does
        // not consume a WAL slot for a call that will error out.
        if let Some((fd, n)) = &parsed {
            if *n <= super::MAX_TRUNCATE_BYTES && self.journal_truncate(*fd, *n, ack).await.is_err()
            {
                let out = vec![err(FSERR_QUOTA_EXCEEDED, "WAL cap exceeded")];
                produce(&out, ack).await?;
                return Ok(out);
            }
        }
        // Phase 3 (Consensus re-execute + verify, 2026-09-01):
        // Oracular follower unchanged — consumes cached reply and
        // finalizes the pre-appended placeholder based on the
        // cached error code.  Consensus follower now re-executes
        // ftruncate via its shadow's real fd (installed by fs_open's
        // Phase-2 real-open), verifies the fresh reply's stable_hash
        // matches the leader's cached-reply hash, and either keeps
        // the pre-appended Success placeholder (on match) or flips
        // it to Failure { FSERR_CODE_CONSENSUS_DIVERGENCE } and
        // emits a divergence-err reply (on mismatch).  See
        // fileio_wal_replay_verification_gap.md for the design.
        let jmode: Option<ConsensusMode> = match &parsed {
            Some((fd, _)) => self.handles.with_mut(*fd, |h| h.cmode).await,
            None => None,
        };
        if is_replay && jmode != Some(ConsensusMode::Consensus) {
            // Oracular / no-shadow follower — H-6 Phase-0 shape.
            if let Some(code_str) = extract_err_code(&previous) {
                self.finalize_failure_journal(fserr_to_code(&code_str), ack);
            }
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        // Fresh syscall — leader (always) or Consensus follower.
        // H-6 refactor: compute the reply without any early-return
        // so failure-finalize gets a single call site below.  The
        // FSERR_CLOSED "unknown fd" branch that previously returned
        // eagerly now folds into `reply` naturally.
        let fresh_reply = match parsed {
            Some((fd, n)) => {
                if n > super::MAX_TRUNCATE_BYTES {
                    err(
                        FSERR_QUOTA_EXCEEDED,
                        format!("truncate {n} exceeds MAX_TRUNCATE_BYTES"),
                    )
                } else {
                    match self.handles.raw_fd(fd).await {
                        Some(raw_fd) => {
                            let r = spawn_blocking(move || unsafe {
                                if libc::ftruncate(raw_fd, n as i64) < 0 {
                                    Err(std::io::Error::last_os_error())
                                } else {
                                    Ok(())
                                }
                            })
                            .await;
                            match r {
                                Err(_je) => err(FSERR_IO, "spawn_blocking task failed"),
                                Ok(Err(e)) => err(io_err_code(&e), io_msg_scrub(&e)),
                                Ok(Ok(())) => ok_bare(),
                            }
                        }
                        None => err(FSERR_CLOSED, format!("unknown fd {fd}")),
                    }
                }
            }
            None => err(FSERR_BAD_ARG, "expected (u64, u64)"),
        };
        if is_replay {
            // Consensus follower — Phase 3 re-execute + verify.
            match verify_reply_hash_matches_cached(&fresh_reply, &previous) {
                Ok(()) => {
                    // Match — reply is byte-identical to cached.
                    // H-6 finalize path applies to fresh reply too
                    // (in case both leader and follower saw the
                    // same syscall error, e.g., ENOSPC agreed on
                    // both sides).  Under the verify-succeeded
                    // branch the fresh code equals the cached code,
                    // so the finalize below is equivalent to the
                    // Oracular branch's cached-based finalize.
                    if let Some(code_str) = extract_err_code(std::slice::from_ref(&fresh_reply)) {
                        self.finalize_failure_journal(fserr_to_code(&code_str), ack);
                    }
                    let out = vec![fresh_reply];
                    produce(&out, ack).await?;
                    Ok(out)
                }
                Err(reason) => {
                    // Divergence — flip the pre-appended placeholder
                    // to Failure { CONSENSUS_DIVERGENCE } and emit a
                    // divergence-err reply.  RSpace rig catches the
                    // divergent produce and rejects the block.
                    let divergence_reply = err(
                        FSERR_CONSENSUS_DIVERGENCE,
                        format!("fs_truncate follower re-execute diverges from leader: {reason}",),
                    );
                    self.finalize_failure_journal(FSERR_CODE_CONSENSUS_DIVERGENCE, ack);
                    let out = vec![divergence_reply];
                    produce(&out, ack).await?;
                    Ok(out)
                }
            }
        } else {
            // Leader path — H-6 finalize on syscall error.
            if let Some(code_str) = extract_err_code(std::slice::from_ref(&fresh_reply)) {
                self.finalize_failure_journal(fserr_to_code(&code_str), ack);
            }
            let out = vec![fresh_reply];
            produce(&out, ack).await?;
            Ok(out)
        }
    }

    // -------------------------------------------------------------------
    // flush — (fd) -> [true]  (fsync: data + metadata)
    // -------------------------------------------------------------------
    pub async fn fs_flush(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        // Phase 9 slice 9b-ii: charge fs_flush weight at handler entry.
        // See fs_open for the rationale on placement before unapply.
        self.metering.reserve_primitive(costs::fs_flush_cost())?;
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_flush"));
        };
        let [fd_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_flush"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match RhoNumber::unapply(fd_par) {
            Some(fd) => {
                let raw_fd = match self.handles.raw_fd(fd as u64).await {
                    Some(r) => r,
                    None => {
                        let out = vec![err(FSERR_CLOSED, format!("unknown fd {fd}"))];
                        produce(&out, ack).await?;
                        return Ok(out);
                    }
                };
                let r = spawn_blocking(move || unsafe {
                    if libc::fsync(raw_fd) < 0 {
                        Err(std::io::Error::last_os_error())
                    } else {
                        Ok(())
                    }
                })
                .await;
                match r {
                    Err(_je) => err(FSERR_IO, "spawn_blocking task failed"),
                    Ok(Err(e)) => err(io_err_code(&e), io_msg_scrub(&e)),
                    Ok(Ok(())) => ok_bare(),
                }
            }
            _ => err(FSERR_BAD_ARG, "expected u64"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // stat — (rootCanon, rel) -> [true, record]
    // Uses fstatat(AT_SYMLINK_NOFOLLOW) via safe descent.
    // -------------------------------------------------------------------
    pub async fn fs_stat(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        // Phase 9 slice 9b-ii: charge fs_stat weight at handler entry.
        // See fs_open for the rationale on placement before unapply.
        self.metering.reserve_primitive(costs::fs_stat_cost())?;
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_stat"));
        };
        // Slice 26: `(root, rel, cmode, ack)`.  `cmode` is
        // `"oracular"` / `"consensus"` and controls whether the
        // record omits host-transient fields.  On an unrecognized
        // cmode string we fall back to `self.mode` (Oracular by
        // default) — this preserves behavior for any caller that
        // hasn't been updated yet.
        let [root_par, rel_par, cmode_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_stat"));
        };
        // M-5 fix (2026-08-06): resolve cmode BEFORE the is_replay
        // short-circuit so both leader + follower can journal
        // symmetrically.  A bad cmode still short-circuits with
        // FSERR_BAD_ARG the same way as before (that path
        // doesn't journal — it's a parse error, not a filesystem
        // observation).
        let mode = match resolve_cmode(cmode_par) {
            Some(m) => m,
            None => {
                let out = vec![err(
                    FSERR_BAD_ARG,
                    "cmode must be String \"oracular\" or \"consensus\"",
                )];
                produce(&out, ack).await?;
                return Ok(out);
            }
        };
        // Precompute journal path (used by both branches).
        let journal_path: Option<PathBuf> =
            match (RhoString::unapply(root_par), RhoString::unapply(rel_par)) {
                (Some(root), Some(rel)) => {
                    let mut p = PathBuf::from(root);
                    if !rel.is_empty() {
                        p.push(&rel);
                    }
                    Some(p)
                }
                _ => None,
            };
        // Phase 1 (Consensus re-execute + verify, 2026-09-01):
        // Oracular follower is unchanged from M-5's Phase-0 behavior
        // — it consumes the leader's cached reply, since Oracle-mode
        // state isn't reproducible on the follower's own fs.  The
        // Consensus follower now re-executes the syscall against its
        // own fs and verifies the fresh reply's stable_hash matches
        // the leader's cached reply hash extracted from `previous`.
        // See auto-memory `fileio_wal_replay_verification_gap.md`.
        if is_replay && mode != ConsensusMode::Consensus {
            // Oracular follower — Phase-0 tautological branch.
            // `journal_state_read` self-guards on Consensus so the
            // call here is a WAL no-op today, kept for structural
            // parity with the Consensus branch's Success path.
            if let Some(p) = journal_path.clone() {
                if let Some(reply_par) = previous.first() {
                    self.journal_state_read(mode, WalOp::Stat, p, reply_par, ack, None);
                }
            }
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        // Fresh syscall reply — required by BOTH the leader (always)
        // and the Consensus follower (Phase 1 re-execute + verify).
        // Under Shape A, `resolve_or_identity` remaps a Consensus
        // bundle root (e.g., `/@bundle/target`) to this validator's
        // on-disk subdir before descent; unregistered roots fall
        // through to identity resolution (Oracular / raw absolute).
        let fresh_reply = match (RhoString::unapply(root_par), RhoString::unapply(rel_par)) {
            (Some(root), Some(rel)) => {
                let leaf_name = leaf_of(&rel);
                let root_pb = PathBuf::from(root);
                let (root_pb, expected_root_id) =
                    self.handles.root_registry.resolve_or_identity(&root_pb);
                spawn_blocking(move || -> Par {
                    let parent = match safe_descend_verified(&root_pb, &rel, expected_root_id) {
                        Ok(p) => p,
                        Err(qe) => {
                            let (code, msg) = quarantine_err_reply(&qe);
                            return err(code, msg);
                        }
                    };
                    match fstatat_meta(&parent) {
                        Ok(m) => ok_par(stat_record(&leaf_name, &m, mode)),
                        Err(e) => err(io_err_code(&e), io_msg_scrub(&e)),
                    }
                })
                .await
                .unwrap_or_else(|_je| err(FSERR_IO, "spawn_blocking task failed"))
            }
            _ => err(FSERR_BAD_ARG, "expected (String, String, String)"),
        };
        if is_replay {
            // Consensus follower — Phase 1 re-execute + verify.
            // `verify_reply_hash_matches_cached` compares
            // stable_hash(fresh_reply) against
            // stable_hash(previous.first()); RSpace guarantees
            // `previous.first()` is the leader's play-time reply
            // verbatim, so its hash equals the `PayloadRef::Hash`
            // the leader's `journal_state_read` wrote at play time.
            match verify_reply_hash_matches_cached(&fresh_reply, &previous) {
                Ok(()) => {
                    // Match — journal the FRESH reply.  Its hash is
                    // byte-identical to the leader's WAL entry hash,
                    // preserving the leader/follower WAL byte-identity
                    // property post-verification (no longer tautological).
                    if let Some(p) = journal_path {
                        self.journal_state_read(mode, WalOp::Stat, p, &fresh_reply, ack, None);
                    }
                    let out = vec![fresh_reply];
                    produce(&out, ack).await?;
                    Ok(out)
                }
                Err(reason) => {
                    // Divergence (D1 = Option A per
                    // `fileio_wal_replay_verification_gap.md`):
                    // divergent DEPLOY fails; block still proceeds.
                    // `journal_state_read` auto-derives the WAL entry's
                    // outcome to `Failure { code: FSERR_CODE_CONSENSUS_
                    // DIVERGENCE }` from the reply's error slot.
                    let divergence_reply = err(
                        FSERR_CONSENSUS_DIVERGENCE,
                        format!("fs_stat follower re-execute diverges from leader: {reason}",),
                    );
                    if let Some(p) = journal_path {
                        self.journal_state_read(mode, WalOp::Stat, p, &divergence_reply, ack, None);
                    }
                    let out = vec![divergence_reply];
                    produce(&out, ack).await?;
                    Ok(out)
                }
            }
        } else {
            // Leader path — journal fresh reply, produce it.
            if let Some(p) = journal_path {
                self.journal_state_read(mode, WalOp::Stat, p, &fresh_reply, ack, None);
            }
            let out = vec![fresh_reply];
            produce(&out, ack).await?;
            Ok(out)
        }
    }

    // -------------------------------------------------------------------
    // exists — (rootCanon, rel) -> [true, Bool]
    // -------------------------------------------------------------------
    pub async fn fs_exists(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        // Phase 9 slice 9b-ii: charge fs_exists weight at handler entry.
        // See fs_open for the rationale on placement before unapply.
        self.metering.reserve_primitive(costs::fs_exists_cost())?;
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_exists"));
        };
        let [root_par, rel_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_exists"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match (RhoString::unapply(root_par), RhoString::unapply(rel_par)) {
            (Some(root), Some(rel)) => {
                let root_pb = PathBuf::from(root);
                let (root_pb, expected_root_id) =
                    self.handles.root_registry.resolve_or_identity(&root_pb);
                spawn_blocking(move || -> Par {
                    let parent = match safe_descend_verified(&root_pb, &rel, expected_root_id) {
                        Ok(p) => p,
                        Err(qe) => {
                            // Not-found or symlink → exists is false.  A
                            // quarantine failure (escape/absolute/etc.)
                            // is a caller error, surface as bad arg.
                            use super::path::QuarantineError::*;
                            return match qe {
                                EscapesRoot | SymlinkComponent | RootIdentityChanged => {
                                    let (c, m) = quarantine_err_reply(&qe);
                                    err(c, m)
                                }
                                Empty | RootSelf => {
                                    let (c, m) = quarantine_err_reply(&qe);
                                    err(c, m)
                                }
                                IoError(_, _) => ok_bool(false),
                            };
                        }
                    };
                    let ok = fstatat_meta(&parent).is_ok();
                    ok_bool(ok)
                })
                .await
                .unwrap_or_else(|_je| err(FSERR_IO, "spawn_blocking task failed"))
            }
            _ => err(FSERR_BAD_ARG, "expected (String, String)"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // entries — (rootCanon, rel) -> [true, [record, ...]]
    // Sorted lex by name; capped at MAX_ENTRIES; per-entry stat error
    // becomes a row with an `error` field.
    // -------------------------------------------------------------------
    pub async fn fs_entries(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        // Phase 9 slice 9b-iv: charge fs_entries SETUP cost at entry.
        // Weight = 50 (the base term, `fs_entries_cost(0)`).  The
        // per-entry supplement (FS_ENTRIES_PER_ENTRY * n_entries)
        // fires as a second `reserve_primitive` call once `n` is
        // knowable — see the two-branch post-reply supplement charges
        // below.  Both branches emit the same two-event sequence
        // with matching weights so the D3 canonical event log is
        // byte-identical across leader and follower.
        self.metering.reserve_primitive(costs::fs_entries_cost(0))?;
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_entries"));
        };
        // Slice 26: `(root, rel, cmode, ack)`.
        let [root_par, rel_par, cmode_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_entries"));
        };
        // M-5: resolve cmode before is_replay so both leader
        // and follower journal symmetrically.
        let mode = match resolve_cmode(cmode_par) {
            Some(m) => m,
            None => {
                let out = vec![err(
                    FSERR_BAD_ARG,
                    "cmode must be String \"oracular\" or \"consensus\"",
                )];
                produce(&out, ack).await?;
                return Ok(out);
            }
        };
        let journal_path: Option<PathBuf> =
            match (RhoString::unapply(root_par), RhoString::unapply(rel_par)) {
                (Some(root), Some(rel)) => {
                    let mut p = PathBuf::from(root);
                    if !rel.is_empty() {
                        p.push(&rel);
                    }
                    Some(p)
                }
                _ => None,
            };
        // Phase 2 (Consensus re-execute + verify, 2026-09-01):
        // Oracular follower is unchanged (Phase-0 tautological path
        // — consumes cached reply, charges per-entry supplement
        // from cached).  Consensus follower re-executes the whole
        // readdir + sort + cap pipeline and verifies the fresh
        // reply's stable_hash matches the leader's.  See
        // fileio_wal_replay_verification_gap.md for the design.
        if is_replay && mode != ConsensusMode::Consensus {
            // Slice 9b-iv follow-up: per-entry supplement charge on
            // the replay branch.  `n` comes from the leader's cached
            // `[true, [row1, ..., rowN]]` reply via
            // `extract_ok_list_len(&previous)`; an error reply or
            // shape mismatch yields `None`, matched by the leader
            // branch charging 0 for the same shape (see below).
            let n_entries = extract_ok_list_len(&previous).unwrap_or(0);
            // Cost-helper audit fix (2026-08-26): use
            // `reserve_incremental_primitive` — its early-return on
            // zero cost avoids `BugFoundError` when n_entries = 0
            // (empty directory).  Pre-fix, an empty-dir fs_entries
            // populated `EvaluateResult.errors` and skipped the WAL
            // journal that fires below.  Companion to the
            // streaming-slice Step 3 fix at handlers.rs:2786 which
            // dodges the same hazard for `fs_entries_stream_next`.
            self.metering.reserve_incremental_primitive(
                costs::fs_entries_per_entry_supplement_cost(n_entries),
            )?;
            if let Some(p) = journal_path.clone() {
                if let Some(reply_par) = previous.first() {
                    self.journal_state_read(mode, WalOp::Entries, p, reply_par, ack, None);
                }
            }
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        // Fresh syscall — leader (always) or Consensus follower
        // (Phase 2 re-execute).  Under Shape A, the follower's
        // re-execute reads the follower's own subdir (via
        // resolve_or_identity + safe_descend_verified).  Sort +
        // cap make the reply deterministic given identical dir
        // state, so verify hashing works uniformly.
        let fresh_reply = match (RhoString::unapply(root_par), RhoString::unapply(rel_par)) {
            (Some(root), Some(rel)) => {
                let root_pb = PathBuf::from(root);
                let (root_pb, expected_root_id) =
                    self.handles.root_registry.resolve_or_identity(&root_pb);
                spawn_blocking(move || -> Par {
                    let parent = match safe_descend_verified(&root_pb, &rel, expected_root_id) {
                        Ok(p) => p,
                        Err(qe) => {
                            let (code, msg) = quarantine_err_reply(&qe);
                            return err(code, msg);
                        }
                    };
                    // Open the target directory (safely, via openat +
                    // O_NOFOLLOW|O_DIRECTORY off the parent dirfd).
                    let dir_fd = unsafe {
                        libc::openat(
                            parent.as_raw_fd(),
                            parent.leaf_ptr(),
                            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                        )
                    };
                    if dir_fd < 0 {
                        let e = std::io::Error::last_os_error();
                        return err(io_err_code(&e), io_msg_scrub(&e));
                    }
                    // L-3 fix (2026-08-06): use F_DUPFD_CLOEXEC so the
                    // duplicated fd carries FD_CLOEXEC atomically.  Plain
                    // libc::dup would produce a fd WITHOUT CLOEXEC; a
                    // concurrent exec (from any thread) would leak the
                    // dir fd into the child process.  F_DUPFD_CLOEXEC
                    // closes the race by setting CLOEXEC in the same
                    // syscall.
                    let read_fd = unsafe { libc::fcntl(dir_fd, libc::F_DUPFD_CLOEXEC, 0) };
                    if read_fd < 0 {
                        let e = std::io::Error::last_os_error();
                        unsafe { libc::close(dir_fd) };
                        return err(io_err_code(&e), io_msg_scrub(&e));
                    }
                    let entries = read_dir_capped(read_fd, MAX_ENTRIES);
                    match entries {
                        Err(e) => {
                            unsafe { libc::close(dir_fd) };
                            err(io_err_code(&e), io_msg_scrub(&e))
                        }
                        Ok((mut names, hit_cap)) => {
                            if hit_cap {
                                unsafe { libc::close(dir_fd) };
                                return err(
                                    FSERR_QUOTA_EXCEEDED,
                                    format!(
                                        "entries exceeds MAX_ENTRIES={MAX_ENTRIES}; use \
                                         entriesStream for large directories",
                                    ),
                                );
                            }
                            names.sort();
                            let rows: Vec<Par> = names
                                .into_iter()
                                .map(|name| entry_stat_row(dir_fd, &name, mode))
                                .collect();
                            unsafe { libc::close(dir_fd) };
                            ok_list(rows)
                        }
                    }
                })
                .await
                .unwrap_or_else(|_je| err(FSERR_IO, "spawn_blocking task failed"))
            }
            _ => err(FSERR_BAD_ARG, "expected (String, String)"),
        };
        // Slice 9b-iv follow-up: per-entry supplement charge from
        // the fresh reply.  Applies to leader (always) and
        // Consensus follower (Phase 2 re-execute).  On successful
        // Phase-2 verify, fresh reply has the same list length as
        // cached (the reply Par is bytewise identical when the hash
        // matches), so the charge matches the leader's — canonical
        // event log stays byte-identical.  On divergence, the
        // deploy is rejected via RSpace rig anyway, so any cost
        // divergence is a symptom of the underlying state
        // divergence rather than an independent consensus concern.
        let n_entries = extract_ok_list_len(std::slice::from_ref(&fresh_reply)).unwrap_or(0);
        self.metering.reserve_incremental_primitive(
            costs::fs_entries_per_entry_supplement_cost(n_entries),
        )?;
        if is_replay {
            // Consensus follower — Phase 2 re-execute + verify.
            match verify_reply_hash_matches_cached(&fresh_reply, &previous) {
                Ok(()) => {
                    if let Some(p) = journal_path {
                        self.journal_state_read(mode, WalOp::Entries, p, &fresh_reply, ack, None);
                    }
                    let out = vec![fresh_reply];
                    produce(&out, ack).await?;
                    return Ok(out);
                }
                Err(reason) => {
                    let divergence_reply = err(
                        FSERR_CONSENSUS_DIVERGENCE,
                        format!("fs_entries follower re-execute diverges from leader: {reason}",),
                    );
                    if let Some(p) = journal_path {
                        self.journal_state_read(
                            mode,
                            WalOp::Entries,
                            p,
                            &divergence_reply,
                            ack,
                            None,
                        );
                    }
                    let out = vec![divergence_reply];
                    produce(&out, ack).await?;
                    return Ok(out);
                }
            }
        }
        // Leader path (is_replay = false).
        let reply = fresh_reply;
        // M-5: leader journals from fresh reply.
        if let Some(p) = journal_path {
            self.journal_state_read(mode, WalOp::Entries, p, &reply, ack, None);
        }
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // rename — (fromRootCanon, fromRel, toRootCanon, toRel) -> [true]
    // Uses renameat between two safely-descended parents.
    // -------------------------------------------------------------------
    pub async fn fs_rename(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        // Phase 9 slice 9b-iii: charge fs_rename weight at handler entry.
        // See fs_open for the rationale on placement before unapply.
        // Path-mutation constant (2x FS_SYSCALL_CONST: two-endpoint work).
        self.metering.reserve_primitive(costs::fs_rename_cost())?;
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_rename"));
        };
        // H-29-3 lift (2026-08-26): Consensus caps journal a Rename
        // entry pre-syscall with `path` = from-canon-path,
        // `extra_path` = to-canon-path.
        let [from_root_par, from_rel_par, to_root_par, to_rel_par, cmode_par, ack] =
            args.as_slice()
        else {
            return Err(illegal_argument_error("fs_rename"));
        };
        let cmode = match resolve_cmode(cmode_par) {
            Some(m) => m,
            None => {
                let out = vec![err(
                    FSERR_BAD_ARG,
                    "cmode must be String \"oracular\" or \"consensus\"",
                )];
                produce(&out, ack).await?;
                return Ok(out);
            }
        };
        let parsed = match (
            RhoString::unapply(from_root_par),
            RhoString::unapply(from_rel_par),
            RhoString::unapply(to_root_par),
            RhoString::unapply(to_rel_par),
        ) {
            (Some(from_root), Some(from_rel), Some(to_root), Some(to_rel)) => {
                Some((from_root, from_rel, to_root, to_rel))
            }
            _ => None,
        };
        if let Some((from_root, from_rel, to_root, to_rel)) = &parsed {
            let from_canon = canonicalize_lexical(from_root, from_rel);
            let to_canon = canonicalize_lexical(to_root, to_rel);
            if self
                .journal_path_mutation_two(cmode, WalOp::Rename, from_canon, to_canon, ack)
                .await
                .is_err()
            {
                let out = vec![err(FSERR_QUOTA_EXCEEDED, "WAL cap exceeded")];
                produce(&out, ack).await?;
                return Ok(out);
            }
        }
        // Phase 4 (Consensus re-execute + verify, 2026-09-02):
        // Path-based mutation with TWO endpoints (from + to).  Under
        // Consensus, the follower re-executes renameat against its
        // own subdir via the Shape A resolver applied to BOTH roots,
        // verifies fresh vs cached, and finalizes accordingly.
        // Oracular unchanged (H-6 tautological finalize).
        //
        // Atomicity under D3: renameat is POSIX-atomic within a
        // filesystem; under per-validator subdirs each side operates
        // on its own copy of the from/to pair, so atomicity holds
        // per-side.  Cross-device rename (EXDEV) is symmetric across
        // sides because per-validator subdirs live on the same FS
        // as the source bundle (D3 constructs them via std::fs
        // operations on the same mount).
        if is_replay && cmode != ConsensusMode::Consensus {
            // Oracular follower — Phase-0 H-6 shape.
            if let Some(code_str) = extract_err_code(&previous) {
                self.finalize_failure_journal(fserr_to_code(&code_str), ack);
            }
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        // Fresh syscall — leader (always) or Consensus follower.
        let fresh_reply = match parsed {
            Some((from_root, from_rel, to_root, to_rel)) => {
                let from_root_pb = PathBuf::from(from_root);
                let to_root_pb = PathBuf::from(to_root);
                let (from_root_pb, from_expected_id) = self
                    .handles
                    .root_registry
                    .resolve_or_identity(&from_root_pb);
                let (to_root_pb, to_expected_id) =
                    self.handles.root_registry.resolve_or_identity(&to_root_pb);
                spawn_blocking(move || -> Par {
                    let from_parent =
                        match safe_descend_verified(&from_root_pb, &from_rel, from_expected_id) {
                            Ok(p) => p,
                            Err(qe) => {
                                let (c, m) = quarantine_err_reply(&qe);
                                return err(c, m);
                            }
                        };
                    let to_parent =
                        match safe_descend_verified(&to_root_pb, &to_rel, to_expected_id) {
                            Ok(p) => p,
                            Err(qe) => {
                                let (c, m) = quarantine_err_reply(&qe);
                                return err(c, m);
                            }
                        };
                    let rc = unsafe {
                        libc::renameat(
                            from_parent.as_raw_fd(),
                            from_parent.leaf_ptr(),
                            to_parent.as_raw_fd(),
                            to_parent.leaf_ptr(),
                        )
                    };
                    if rc == 0 {
                        ok_bare()
                    } else {
                        let e = std::io::Error::last_os_error();
                        let code = if e.raw_os_error() == Some(libc::EXDEV) {
                            FSERR_CROSS_DEVICE
                        } else {
                            io_err_code(&e)
                        };
                        err(code, io_msg_scrub(&e))
                    }
                })
                .await
                .unwrap_or_else(|_je| err(FSERR_IO, "spawn_blocking task failed"))
            }
            None => err(FSERR_BAD_ARG, "expected 4 String args + cmode"),
        };
        if is_replay {
            // Consensus follower — Phase 4 re-execute + verify.
            match verify_reply_hash_matches_cached(&fresh_reply, &previous) {
                Ok(()) => {
                    if let Some(code_str) = extract_err_code(std::slice::from_ref(&fresh_reply)) {
                        self.finalize_failure_journal(fserr_to_code(&code_str), ack);
                    }
                    let out = vec![fresh_reply];
                    produce(&out, ack).await?;
                    Ok(out)
                }
                Err(reason) => {
                    let divergence_reply = err(
                        FSERR_CONSENSUS_DIVERGENCE,
                        format!("fs_rename follower re-execute diverges from leader: {reason}",),
                    );
                    self.finalize_failure_journal(FSERR_CODE_CONSENSUS_DIVERGENCE, ack);
                    let out = vec![divergence_reply];
                    produce(&out, ack).await?;
                    Ok(out)
                }
            }
        } else {
            // Leader path — H-6 finalize on syscall error.
            if let Some(code_str) = extract_err_code(std::slice::from_ref(&fresh_reply)) {
                self.finalize_failure_journal(fserr_to_code(&code_str), ack);
            }
            let out = vec![fresh_reply];
            produce(&out, ack).await?;
            Ok(out)
        }
    }

    // -------------------------------------------------------------------
    // copyFile — (fromRootCanon, fromRel, toRootCanon, toRel) -> [true, nBytes]
    // Uses safe_open_verified on both sides + std::io::copy on File objects.
    // -------------------------------------------------------------------
    pub async fn fs_copy_file(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        // Phase 9 slice 9b-iii: charge fs_copy_file weight at handler entry.
        // See fs_open for the rationale on placement before unapply.
        // Path-mutation constant (2x FS_SYSCALL_CONST: two-endpoint work).
        self.metering
            .reserve_primitive(costs::fs_copy_file_cost())?;
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_copy_file"));
        };
        // H-29-3 lift (2026-08-26): Consensus caps journal a CopyFile
        // entry pre-syscall.  Applier-side reconstruction reads the
        // source file from the follower's reconstructed tree — the
        // source's bytes are already established by prior WAL entries.
        let [from_root_par, from_rel_par, to_root_par, to_rel_par, cmode_par, ack] =
            args.as_slice()
        else {
            return Err(illegal_argument_error("fs_copy_file"));
        };
        let cmode = match resolve_cmode(cmode_par) {
            Some(m) => m,
            None => {
                let out = vec![err(
                    FSERR_BAD_ARG,
                    "cmode must be String \"oracular\" or \"consensus\"",
                )];
                produce(&out, ack).await?;
                return Ok(out);
            }
        };
        let parsed = match (
            RhoString::unapply(from_root_par),
            RhoString::unapply(from_rel_par),
            RhoString::unapply(to_root_par),
            RhoString::unapply(to_rel_par),
        ) {
            (Some(from_root), Some(from_rel), Some(to_root), Some(to_rel)) => {
                Some((from_root, from_rel, to_root, to_rel))
            }
            _ => None,
        };
        if let Some((from_root, from_rel, to_root, to_rel)) = &parsed {
            let from_canon = canonicalize_lexical(from_root, from_rel);
            let to_canon = canonicalize_lexical(to_root, to_rel);
            if self
                .journal_path_mutation_two(cmode, WalOp::CopyFile, from_canon, to_canon, ack)
                .await
                .is_err()
            {
                let out = vec![err(FSERR_QUOTA_EXCEEDED, "WAL cap exceeded")];
                produce(&out, ack).await?;
                return Ok(out);
            }
        }
        // Phase 4 (Consensus re-execute + verify, 2026-09-02):
        // Path-based mutation, two endpoints, reply carries a u64
        // byte count `[true, n]` in addition to the boolean.  Under
        // Consensus the follower re-executes the copy against its
        // own subdir via the Shape A resolver applied to BOTH roots
        // and verifies the fresh reply (including n) matches cached.
        //
        // H-5 identity migration (F2 residual from Phase 0 Stage 2,
        // 2026-09-02): fs_copy_file previously called `safe_open`
        // (unverified) directly on both endpoints — bypassing H-5's
        // (dev, inode) check and undefended against rename-and-
        // recreate against the operator's staged root.  This slice
        // migrates both endpoints to `safe_open_verified` and passes
        // the `expected_root_id` from `resolve_or_identity`, closing
        // F2's fs_copy_file half (fs_open half landed with the
        // Phase-0 F2 fix).
        //
        // Byte-count divergence surface: under D3 per-validator
        // subdirs, byte-count symmetry holds because both sides
        // re-execute every prior WAL op against their own copy of
        // the source — the source contents at copy time are
        // deterministic on the deploy sequence.  Divergence only
        // fires if an external process modified the source between
        // leader play and follower re-execute (a real inconsistency
        // the CONSENSUS_DIVERGENCE surface is meant to catch).
        if is_replay && cmode != ConsensusMode::Consensus {
            // Oracular follower — Phase-0 H-6 shape.
            if let Some(code_str) = extract_err_code(&previous) {
                self.finalize_failure_journal(fserr_to_code(&code_str), ack);
            }
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        // Fresh syscall — leader (always) or Consensus follower.
        let fresh_reply = match parsed {
            Some((from_root, from_rel, to_root, to_rel)) => {
                let from_root_pb = PathBuf::from(from_root);
                let to_root_pb = PathBuf::from(to_root);
                let (from_root_pb, from_expected_id) = self
                    .handles
                    .root_registry
                    .resolve_or_identity(&from_root_pb);
                let (to_root_pb, to_expected_id) =
                    self.handles.root_registry.resolve_or_identity(&to_root_pb);
                spawn_blocking(move || -> Par {
                    let mut src = match safe_open_verified(
                        &from_root_pb,
                        &from_rel,
                        libc::O_RDONLY,
                        0,
                        from_expected_id,
                    ) {
                        Ok(f) => f,
                        Err(qe) => {
                            let (c, m) = quarantine_err_reply(&qe);
                            return err(c, m);
                        }
                    };
                    let mut dst = match safe_open_verified(
                        &to_root_pb,
                        &to_rel,
                        libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC,
                        0o644,
                        to_expected_id,
                    ) {
                        Ok(f) => f,
                        Err(qe) => {
                            let (c, m) = quarantine_err_reply(&qe);
                            return err(c, m);
                        }
                    };
                    match std::io::copy(&mut src, &mut dst) {
                        Ok(n) => ok_u64(n),
                        Err(e) => err(io_err_code(&e), io_msg_scrub(&e)),
                    }
                })
                .await
                .unwrap_or_else(|_je| err(FSERR_IO, "spawn_blocking task failed"))
            }
            None => err(FSERR_BAD_ARG, "expected 4 String args + cmode"),
        };
        if is_replay {
            // Consensus follower — Phase 4 re-execute + verify.
            match verify_reply_hash_matches_cached(&fresh_reply, &previous) {
                Ok(()) => {
                    if let Some(code_str) = extract_err_code(std::slice::from_ref(&fresh_reply)) {
                        self.finalize_failure_journal(fserr_to_code(&code_str), ack);
                    }
                    let out = vec![fresh_reply];
                    produce(&out, ack).await?;
                    Ok(out)
                }
                Err(reason) => {
                    let divergence_reply = err(
                        FSERR_CONSENSUS_DIVERGENCE,
                        format!("fs_copy_file follower re-execute diverges from leader: {reason}",),
                    );
                    self.finalize_failure_journal(FSERR_CODE_CONSENSUS_DIVERGENCE, ack);
                    let out = vec![divergence_reply];
                    produce(&out, ack).await?;
                    Ok(out)
                }
            }
        } else {
            // Leader path — H-6 finalize on syscall error.
            if let Some(code_str) = extract_err_code(std::slice::from_ref(&fresh_reply)) {
                self.finalize_failure_journal(fserr_to_code(&code_str), ack);
            }
            let out = vec![fresh_reply];
            produce(&out, ack).await?;
            Ok(out)
        }
    }

    // -------------------------------------------------------------------
    // removeFile — (rootCanon, rel) -> [true]
    // -------------------------------------------------------------------
    pub async fn fs_remove_file(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        // Phase 9 slice 9b-iii: charge fs_remove_file weight at handler entry.
        // See fs_open for the rationale on placement before unapply.
        // Path-mutation constant (2x FS_SYSCALL_CONST: two-endpoint work).
        self.metering
            .reserve_primitive(costs::fs_remove_file_cost())?;
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_remove_file"));
        };
        // H-29-3 lift (2026-08-26): Consensus caps now unlink after
        // WAL-journaling.  Consensus+locked still returns FSERR_BUSY
        // per plan §Mode-differentiated invariants (see below).
        // Argument shape: `(root, rel, cmode, ack)`.
        let [root_par, rel_par, cmode_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_remove_file"));
        };
        let cmode = match resolve_cmode(cmode_par) {
            Some(m) => m,
            None => {
                let out = vec![err(
                    FSERR_BAD_ARG,
                    "cmode must be String \"oracular\" or \"consensus\"",
                )];
                produce(&out, ack).await?;
                return Ok(out);
            }
        };
        let parsed = match (RhoString::unapply(root_par), RhoString::unapply(rel_par)) {
            (Some(root), Some(rel)) => Some((root, rel)),
            _ => None,
        };
        // Pre-syscall WAL journal (fully derived from args).
        if let Some((root, rel)) = &parsed {
            let canon_path = canonicalize_lexical(root, rel);
            if self
                .journal_path_mutation_single(
                    cmode,
                    WalOp::RemoveFile,
                    canon_path,
                    None,
                    None,
                    None,
                    ack,
                )
                .await
                .is_err()
            {
                let out = vec![err(FSERR_QUOTA_EXCEEDED, "WAL cap exceeded")];
                produce(&out, ack).await?;
                return Ok(out);
            }
        }
        // Phase 4 (Consensus re-execute + verify, 2026-09-02):
        // Path-based mutation.  Under Consensus, the follower now
        // re-executes unlinkat against its own subdir file via the
        // Shape A resolver, verifies fresh reply vs cached, and
        // finalizes accordingly.  Oracular unchanged (H-6
        // tautological finalize path).
        //
        // Idempotency concern (per plan Risk R2, largely dissolved
        // by D3): under per-validator subdirs, the follower's own
        // copy of the file exists at pre-play state when the
        // follower's re-execute runs (leader's play only touched
        // leader's subdir).  Re-unlinkat succeeds on both sides
        // symmetrically.  A prior deploy in the SAME block that
        // removed the same file would fail on both sides with
        // FSERR_NOT_FOUND — symmetric error, no divergence.
        if is_replay && cmode != ConsensusMode::Consensus {
            // Oracular follower — Phase-0 H-6 shape.
            if let Some(code_str) = extract_err_code(&previous) {
                self.finalize_failure_journal(fserr_to_code(&code_str), ack);
            }
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        // Fresh syscall — leader (always) or Consensus follower.
        // Phase 8 slice 8a step 6 (2026-08-13): mode-differentiated
        // unlink gate.  Consensus+locked returns FSERR_BUSY per plan
        // §Mode-differentiated invariants ("fs_remove_file consults
        // LockRegistry at handler entry and refuses if any lock is
        // held on (dev, inode)").  Oracular+locked proceeds with the
        // unlink but log-warns for observability.
        let fresh_reply = match parsed {
            Some((root, rel)) => {
                let root_pb = PathBuf::from(root);
                let (root_pb, expected_root_id) =
                    self.handles.root_registry.resolve_or_identity(&root_pb);
                let lock_registry = self.handles.lock_registry.clone();
                spawn_blocking(move || -> Par {
                    let parent = match safe_descend_verified(&root_pb, &rel, expected_root_id) {
                        Ok(p) => p,
                        Err(qe) => {
                            let (c, m) = quarantine_err_reply(&qe);
                            return err(c, m);
                        }
                    };
                    // Step 6: fstatat the target for (dev, inode) so we
                    // can query the LockRegistry.  `AT_SYMLINK_NOFOLLOW`
                    // matches unlinkat's semantics.
                    let target_dev_inode = target_dev_inode_at(&parent);
                    let target_is_locked = target_dev_inode
                        .map(|di| lock_registry.is_locked(di, (0, u64::MAX)))
                        .unwrap_or(false);
                    if cmode == ConsensusMode::Consensus && target_is_locked {
                        // Plan §Mode-differentiated invariants:
                        // Consensus refuses on any held lock.
                        // Closes the "unlink-while-locked → lock
                        // survives on now-orphan inode" race.
                        return err(
                            FSERR_BUSY,
                            "cannot remove: lock held on target (dev, inode)",
                        );
                    }
                    if cmode == ConsensusMode::Oracular && target_is_locked {
                        // Plan §Mode-differentiated invariants:
                        // Oracular does NOT gate on locks (host FS
                        // semantics — `rm` works even while another
                        // process has the file open).  Log-warn for
                        // observability.
                        if let Some((dev, ino)) = target_dev_inode {
                            let n_holders = lock_registry.count_locks((dev, ino));
                            tracing::warn!(
                                target: "f1r3fly.fs.oracular",
                                dev = dev,
                                ino = ino,
                                n_holders = n_holders,
                                "oracular unlink of locked file (dev={}, ino={}) — {} \
                                 holder(s) will observe subsequent errors on path-based \
                                 calls; fd-based calls remain valid until close",
                                dev,
                                ino,
                                n_holders
                            );
                        }
                    }
                    let rc = unsafe { libc::unlinkat(parent.as_raw_fd(), parent.leaf_ptr(), 0) };
                    if rc == 0 {
                        ok_bare()
                    } else {
                        let e = std::io::Error::last_os_error();
                        err(io_err_code(&e), io_msg_scrub(&e))
                    }
                })
                .await
                .unwrap_or_else(|_je| err(FSERR_IO, "spawn_blocking task failed"))
            }
            None => err(FSERR_BAD_ARG, "expected (String, String, String)"),
        };
        if is_replay {
            // Consensus follower — Phase 4 re-execute + verify.
            match verify_reply_hash_matches_cached(&fresh_reply, &previous) {
                Ok(()) => {
                    if let Some(code_str) = extract_err_code(std::slice::from_ref(&fresh_reply)) {
                        self.finalize_failure_journal(fserr_to_code(&code_str), ack);
                    }
                    let out = vec![fresh_reply];
                    produce(&out, ack).await?;
                    Ok(out)
                }
                Err(reason) => {
                    let divergence_reply = err(
                        FSERR_CONSENSUS_DIVERGENCE,
                        format!(
                            "fs_remove_file follower re-execute diverges from leader: \
                             {reason}",
                        ),
                    );
                    self.finalize_failure_journal(FSERR_CODE_CONSENSUS_DIVERGENCE, ack);
                    let out = vec![divergence_reply];
                    produce(&out, ack).await?;
                    Ok(out)
                }
            }
        } else {
            // Leader path — H-6 finalize on syscall error.
            if let Some(code_str) = extract_err_code(std::slice::from_ref(&fresh_reply)) {
                self.finalize_failure_journal(fserr_to_code(&code_str), ack);
            }
            let out = vec![fresh_reply];
            produce(&out, ack).await?;
            Ok(out)
        }
    }

    // -------------------------------------------------------------------
    // removeDir — (rootCanon, rel, recursive: Bool, cmode) ->
    //   Non-recursive: [true] / [false, code, msg]
    //   Recursive Oracular: [true] / [false, code, msg]
    //   Recursive Consensus: [true, [[path, kind], ...]] / [false, code, msg,
    //     [[path, kind], ...]]  (manifest of deleted entries in unlink order)
    //
    // H-29-3 lift slice 2 (2026-08-26): Consensus recursive removeDir
    // walks the tree in sorted post-order, emits one WAL entry per
    // unlinked leaf (RemoveFile) or directory (RemoveDir), and packs
    // the manifest into the reply so the follower can journal
    // byte-identical entries on the is_replay branch.  Non-recursive
    // Consensus emits a single RemoveDir entry.  Oracular semantics
    // are unchanged.
    // -------------------------------------------------------------------
    pub async fn fs_remove_dir(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        self.metering
            .reserve_primitive(costs::fs_remove_dir_cost(0))?;
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_remove_dir"));
        };
        let [root_par, rel_par, recursive_par, cmode_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_remove_dir"));
        };
        let cmode = match resolve_cmode(cmode_par) {
            Some(m) => m,
            None => {
                // DD-RemoveDirReplyShape: bad-cmode 4-element shape
                // (recursive is unknown here; the generic count-
                // carrying failure form is safe for Dir.rho unwrap).
                let out = vec![err_with_count(
                    FSERR_BAD_ARG,
                    "cmode must be String \"oracular\" or \"consensus\"",
                    0,
                )];
                produce(&out, ack).await?;
                return Ok(out);
            }
        };
        let parsed = match (
            RhoString::unapply(root_par),
            RhoString::unapply(rel_par),
            RhoBoolean::unapply(recursive_par),
        ) {
            (Some(root), Some(rel), Some(recursive)) => Some((root, rel, recursive)),
            _ => None,
        };
        if is_replay {
            // Phase 4 (Consensus re-execute + verify, 2026-09-02):
            // Both non-recursive AND recursive Consensus followers
            // re-execute against their own subdir via the Shape A
            // resolver and verify fresh vs cached — non-recursive
            // via a single unlinkat(AT_REMOVEDIR), recursive via
            // R5(b) collect_recursive_manifest returning relative
            // paths so both sides walk their own subdirs and
            // produce byte-identical WAL + reply manifests.
            // Non-recursive Oracular keeps the H-6 tautological
            // finalize path.  Recursive Oracular walks locally
            // (no WAL journaling) — unchanged.
            if let Some((root, rel, recursive)) = &parsed {
                if !recursive && cmode == ConsensusMode::Consensus {
                    // Phase 4 non-recursive Consensus re-execute.
                    let canon_path = canonicalize_lexical(root, rel);
                    if self
                        .journal_path_mutation_single(
                            cmode,
                            WalOp::RemoveDir,
                            canon_path,
                            None,
                            None,
                            None,
                            ack,
                        )
                        .await
                        .is_err()
                    {
                        // DD-RemoveDirReplyShape: non-recursive
                        // Consensus WAL-cap failure — 4-element
                        // count-carrying failure with n=0.
                        let out = vec![err_with_count(FSERR_QUOTA_EXCEEDED, "WAL cap exceeded", 0)];
                        produce(&out, ack).await?;
                        return Ok(out);
                    }
                    // Fresh syscall via Shape A resolver + lock-check
                    // gate (same as leader's non-recursive path).
                    let raw_root_pb = PathBuf::from(root);
                    let (on_disk_root_pb, expected_root_id) =
                        self.handles.root_registry.resolve_or_identity(&raw_root_pb);
                    let rel_owned = rel.to_string();
                    let lock_registry = self.handles.lock_registry.clone();
                    let fresh_reply = spawn_blocking(move || -> Par {
                        let parent = match safe_descend_verified(
                            &on_disk_root_pb,
                            &rel_owned,
                            expected_root_id,
                        ) {
                            Ok(p) => p,
                            Err(qe) => {
                                let (c, m) = quarantine_err_reply(&qe);
                                // DD-RemoveDirReplyShape: quarantine
                                // failure in follower non-recursive
                                // Consensus branch.
                                return err_with_count(c, m, 0);
                            }
                        };
                        let target_dev_inode = target_dev_inode_at(&parent);
                        let target_is_locked = target_dev_inode
                            .map(|di| lock_registry.is_locked(di, (0, u64::MAX)))
                            .unwrap_or(false);
                        // Consensus + locked → FSERR_BUSY (symmetric
                        // with leader; shared LockRegistry across
                        // spawned runtimes means both sides observe
                        // the same lock state).
                        if target_is_locked {
                            return err_with_count(
                                FSERR_BUSY,
                                "cannot remove: lock held on target (dev, inode)",
                                0,
                            );
                        }
                        let rc = unsafe {
                            libc::unlinkat(
                                parent.as_raw_fd(),
                                parent.leaf_ptr(),
                                libc::AT_REMOVEDIR,
                            )
                        };
                        if rc == 0 {
                            // DD-RemoveDirReplyShape: non-recursive success
                            // deletes exactly one entry (the target itself).
                            ok_with_count(1)
                        } else {
                            let e = std::io::Error::last_os_error();
                            // DD-RemoveDirReplyShape: non-recursive failure
                            // → 0 entries deleted before the error.
                            err_with_count(io_err_code(&e), io_msg_scrub(&e), 0)
                        }
                    })
                    .await
                    .unwrap_or_else(|_je| {
                        err_with_count(FSERR_IO, "spawn_blocking task failed", 0)
                    });
                    // Verify + finalize.
                    let supp_n =
                        fs_remove_dir_supplement_count_from_previous(&parsed, cmode, &previous);
                    self.metering.reserve_incremental_primitive(
                        costs::fs_remove_dir_per_entry_supplement_cost(supp_n),
                    )?;
                    match verify_reply_hash_matches_cached(&fresh_reply, &previous) {
                        Ok(()) => {
                            if let Some(code_str) =
                                extract_err_code(std::slice::from_ref(&fresh_reply))
                            {
                                self.finalize_failure_journal(fserr_to_code(&code_str), ack);
                            }
                            let out = vec![fresh_reply];
                            produce(&out, ack).await?;
                            return Ok(out);
                        }
                        Err(reason) => {
                            // DD-RemoveDirReplyShape: non-recursive
                            // Consensus divergence — 4-element failure.
                            let divergence_reply = err_with_count(
                                FSERR_CONSENSUS_DIVERGENCE,
                                format!(
                                    "fs_remove_dir follower re-execute diverges from leader: \
                                     {reason}",
                                ),
                                0,
                            );
                            self.finalize_failure_journal(FSERR_CODE_CONSENSUS_DIVERGENCE, ack);
                            let out = vec![divergence_reply];
                            produce(&out, ack).await?;
                            return Ok(out);
                        }
                    }
                }
                if !recursive {
                    // Non-recursive Oracular: journal single RemoveDir
                    // entry (no-op for Oracular via journal_path_mutation_
                    // single) + H-6 finalize from cached.
                    let canon_path = canonicalize_lexical(root, rel);
                    let _ = self
                        .journal_path_mutation_single(
                            cmode,
                            WalOp::RemoveDir,
                            canon_path,
                            None,
                            None,
                            None,
                            ack,
                        )
                        .await;
                    if let Some(code_str) = extract_err_code(&previous) {
                        self.finalize_failure_journal(fserr_to_code(&code_str), ack);
                    }
                } else if cmode == ConsensusMode::Consensus {
                    // R5(b) recursive Consensus re-execute (2026-09-02).
                    // Follower walks its OWN per-validator subdir via
                    // Shape A resolver + safe_descend_verified, does
                    // real per-entry unlinks, journals per-entry WAL
                    // with bundle-relative paths, and produces its own
                    // reply with the relative-path manifest.  Both
                    // leader and follower produce byte-identical WAL
                    // + reply when their subdirs contain the same
                    // tree (Shape A / D3 discipline).  Divergence
                    // surfaces via verify_reply_hash_matches_cached
                    // → CONSENSUS_DIVERGENCE reply → RSpace rig
                    // catches at check_replay_data.
                    //
                    // On divergence we do NOT flip the per-entry WAL
                    // placeholders to Failure { CONSENSUS_DIVERGENCE }
                    // because they reflect follower's ACTUAL syscalls
                    // (which may have succeeded on the follower's own
                    // subdir).  The reply-hash divergence is the
                    // canonical divergence signal; downstream
                    // consumers hash the reply, not the WAL.
                    let raw_root_pb = PathBuf::from(root);
                    let (on_disk_root_pb, expected_root_id) =
                        self.handles.root_registry.resolve_or_identity(&raw_root_pb);
                    let canon_wal_target = canonicalize_lexical(root, rel);
                    let rel_owned = rel.to_string();
                    let lock_registry = self.handles.lock_registry.clone();
                    let wal_handle = self.handles.wal.clone();
                    let ack_clone = ack.clone();
                    let fresh_reply = spawn_blocking(move || -> Par {
                        let parent = match safe_descend_verified(
                            &on_disk_root_pb,
                            &rel_owned,
                            expected_root_id,
                        ) {
                            Ok(p) => p,
                            Err(qe) => {
                                let (c, m) = quarantine_err_reply(&qe);
                                // DD-RemoveDirReplyShape: recursive Consensus
                                // quarantine failure — 5-element with empty
                                // manifest (walk didn't start).
                                return err_with_manifest(c, m, &[]);
                            }
                        };
                        let target_dev_inode = target_dev_inode_at(&parent);
                        let target_is_locked = target_dev_inode
                            .map(|di| lock_registry.is_locked(di, (0, u64::MAX)))
                            .unwrap_or(false);
                        if target_is_locked {
                            return err_with_manifest(
                                FSERR_BUSY,
                                "cannot remove: lock held on target (dev, inode)",
                                &[],
                            );
                        }
                        let canon_target =
                            canonicalize_lexical(&on_disk_root_pb.to_string_lossy(), &rel_owned);
                        let manifest = match collect_recursive_manifest(&canon_target) {
                            Ok(m) => m,
                            Err(e) => {
                                return err_with_manifest(io_err_code(&e), io_msg_scrub(&e), &[]);
                            }
                        };
                        let mut deleted: Vec<(std::path::PathBuf, RemoveKind)> = Vec::new();
                        for (rel_path, kind) in manifest {
                            // Empty rel_path marks the target root itself
                            // (final post-order entry).  `Path::join("")`
                            // appends a trailing separator; the resulting
                            // PathBuf compares equal to the un-joined one
                            // via `PathBuf::eq` (component-wise) but its
                            // serialized bytes differ.  Follower must
                            // special-case to match the leader's WAL
                            // byte-for-byte (latent bug caught by
                            // security review 2026-09-02: `assert_eq!(l,
                            // f)` in tests uses PathBuf::eq and hid the
                            // discrepancy, but `encode_wal_slice` byte
                            // compare would surface it).
                            let wal_path = if rel_path.as_os_str().is_empty() {
                                canon_wal_target.clone()
                            } else {
                                canon_wal_target.join(&rel_path)
                            };
                            let op = match kind {
                                RemoveKind::File => WalOp::RemoveFile,
                                RemoveKind::Dir => WalOp::RemoveDir,
                            };
                            let per_entry_ack = per_entry_ack_seed(&ack_clone, &wal_path);
                            if wal_handle
                                .append_with_ack(
                                    WalEntry {
                                        op,
                                        path: wal_path,
                                        extra_path: None,
                                        offset: None,
                                        length: None,
                                        payload_ref: None,
                                        mode_bits: None,
                                        owner: None,
                                        group: None,
                                        outcome: WalOutcome::Success,
                                    },
                                    per_entry_ack,
                                )
                                .is_err()
                            {
                                return err_with_manifest(
                                    FSERR_QUOTA_EXCEEDED,
                                    "WAL cap exceeded during recursive removeDir",
                                    &deleted,
                                );
                            }
                            // TOCTOU-immune unlink via pinned dirfd chain
                            // from `parent` (post-security-review S-1,
                            // 2026-09-02).  Prior to this fix, the
                            // recursive Consensus branches used
                            // libc_unlink(AT_FDCWD, absolute_path) which
                            // resolved intermediate components from cwd
                            // by name and lost the dirfd guarantee the
                            // Oracular remove_dir_recursive already had.
                            let unlink_rc =
                                unsafe { unlink_manifest_entry(&parent, &rel_path, kind) };
                            match unlink_rc {
                                Ok(()) => {
                                    deleted.push((rel_path, kind));
                                }
                                Err(e) => {
                                    let code_u32 = io_err_code_u32(&e);
                                    let _ = wal_handle.update_outcome_by_ack_hash(
                                        per_entry_ack,
                                        WalOutcome::Failure { code: code_u32 },
                                    );
                                    return err_with_manifest(
                                        io_err_code(&e),
                                        io_msg_scrub(&e),
                                        &deleted,
                                    );
                                }
                            }
                        }
                        ok_recursive_manifest(&deleted)
                    })
                    .await
                    .unwrap_or_else(|_je| {
                        // DD-RemoveDirReplyShape: spawn_blocking task
                        // failure on recursive Consensus path — 5-element
                        // with empty manifest.
                        err_with_manifest(FSERR_IO, "spawn_blocking task failed", &[])
                    });
                    let supp_n = fs_remove_dir_supplement_count(&parsed, cmode, &fresh_reply);
                    self.metering.reserve_incremental_primitive(
                        costs::fs_remove_dir_per_entry_supplement_cost(supp_n),
                    )?;
                    match verify_reply_hash_matches_cached(&fresh_reply, &previous) {
                        Ok(()) => {
                            let out = vec![fresh_reply];
                            produce(&out, ack).await?;
                            return Ok(out);
                        }
                        Err(reason) => {
                            // DD-RemoveDirReplyShape: recursive Consensus
                            // divergence — 5-element failure.
                            let divergence_reply = err_with_manifest(
                                FSERR_CONSENSUS_DIVERGENCE,
                                format!(
                                    "fs_remove_dir recursive follower re-execute \
                                     diverges from leader: {reason}",
                                ),
                                &[],
                            );
                            let out = vec![divergence_reply];
                            produce(&out, ack).await?;
                            return Ok(out);
                        }
                    }
                }
            }
            // H-29-3 lift follow-up (2026-08-27): per-entry cost
            // supplement (follower path).  Both sides derive the
            // count from the reply's manifest (via
            // extract_removedir_manifest) for Consensus recursive,
            // or from `recursive`/`cmode`/reply-shape for the
            // non-recursive and Oracular cases.  Same helper as
            // the leader path below; keeps the D3 event log
            // byte-identical.
            //
            // R5(b) note (2026-09-02): the recursive Consensus branch
            // returns from its own arm above (with cost supplement
            // computed from fresh reply).  This fall-through path
            // covers non-recursive Oracular + recursive Oracular +
            // any None-parsed cases.
            let supp_n = fs_remove_dir_supplement_count_from_previous(&parsed, cmode, &previous);
            self.metering.reserve_incremental_primitive(
                costs::fs_remove_dir_per_entry_supplement_cost(supp_n),
            )?;
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        // Leader path.
        let reply = match parsed.clone() {
            Some((root, rel, recursive)) => {
                // Task 0.4 / Shape A (2026-08-31): capture BOTH the
                // raw Rholang canonRoot (bundle-relative for Consensus
                // caps) AND the resolver's on-disk root separately.
                //   - `raw_root_pb` / `canon_wal_target` — used for
                //     WAL entry.path emission (Shape A invariant:
                //     WAL entries carry bundle-relative paths).
                //   - `on_disk_root_pb` — passed to safe_descend_verified
                //     for the actual filesystem descent.
                // Pre-0.4 the code shadowed `root_pb` with the resolver
                // output and then reused it for `canon_target`, so the
                // leader-side WAL RemoveDir entry recorded the
                // RESOLVED absolute path instead of the raw
                // bundle-relative one — the follower-side symmetric
                // journaling at the top of this handler uses the raw
                // root, so under Shape A the two sides would have
                // recorded divergent path bytes for the same logical
                // action.  No PB-M-14 canary exercises Consensus
                // RemoveDir today, so the divergence was latent.
                let raw_root_pb = PathBuf::from(&root);
                let (on_disk_root_pb, expected_root_id) =
                    self.handles.root_registry.resolve_or_identity(&raw_root_pb);
                let canon_wal_target = canonicalize_lexical(&root, &rel);
                let lock_registry = self.handles.lock_registry.clone();
                let ack_clone = ack.clone();
                let wal = self.handles.wal.clone();
                spawn_blocking(move || -> Par {
                    let parent =
                        match safe_descend_verified(&on_disk_root_pb, &rel, expected_root_id) {
                            Ok(p) => p,
                            Err(qe) => {
                                let (c, m) = quarantine_err_reply(&qe);
                                // DD-RemoveDirReplyShape: pre-walk quarantine
                                // failure — shape picker picks 4- vs 5-element
                                // based on (recursive, cmode).
                                return early_err_for_remove_dir(recursive, cmode, c, m);
                            }
                        };
                    let target_dev_inode = target_dev_inode_at(&parent);
                    let target_is_locked = target_dev_inode
                        .map(|di| lock_registry.is_locked(di, (0, u64::MAX)))
                        .unwrap_or(false);
                    // Consensus + locked → FSERR_BUSY (unchanged from
                    // slice 1 removeFile pattern).  Oracular + locked
                    // proceeds with a log-warn.
                    if cmode == ConsensusMode::Consensus && target_is_locked {
                        return early_err_for_remove_dir(
                            recursive,
                            cmode,
                            FSERR_BUSY,
                            "cannot remove: lock held on target (dev, inode)",
                        );
                    }
                    if cmode == ConsensusMode::Oracular && target_is_locked {
                        if let Some((dev, ino)) = target_dev_inode {
                            let n_holders = lock_registry.count_locks((dev, ino));
                            tracing::warn!(
                                target: "f1r3fly.fs.oracular",
                                dev = dev,
                                ino = ino,
                                n_holders = n_holders,
                                "oracular removeDir of locked directory (dev={}, ino={}) \
                                 — {} holder(s) will observe subsequent errors on \
                                 path-based calls; fd-based calls remain valid until close",
                                dev,
                                ino,
                                n_holders
                            );
                        }
                    }
                    // Task 0.4 / Shape A + R5(b) (2026-09-02):
                    // recursive manifest emission below walks the
                    // on-disk tree via
                    // `collect_recursive_manifest(&canon_target)`
                    // where canon_target is the on-disk absolute
                    // path.  The walker returns RELATIVE paths
                    // (relative to canon_target); callers apply
                    // `canon_wal_target.join(rel)` for bundle-
                    // relative WAL entries and `canon_target.join(rel)`
                    // for on-disk syscalls.  This closes the pre-
                    // R5(b) Shape A gap where absolute per-validator
                    // paths in the recursive manifest wouldn't
                    // resolve on a joiner via the registry.
                    let canon_target =
                        canonicalize_lexical(&on_disk_root_pb.to_string_lossy(), &rel);
                    if !recursive {
                        // Non-recursive: single unlinkat(AT_REMOVEDIR).
                        if cmode == ConsensusMode::Consensus {
                            let e = wal.append_with_ack(
                                WalEntry {
                                    op: WalOp::RemoveDir,
                                    // Shape A: WAL records the raw
                                    // bundle-relative path so leader
                                    // and follower append identical
                                    // bytes; syscall below uses
                                    // canon_target (on-disk absolute).
                                    path: canon_wal_target.clone(),
                                    extra_path: None,
                                    offset: None,
                                    length: None,
                                    payload_ref: None,
                                    mode_bits: None,
                                    owner: None,
                                    group: None,
                                    outcome: WalOutcome::Success,
                                },
                                ack_channel_hash(&ack_clone),
                            );
                            if e.is_err() {
                                // Non-recursive Consensus WAL cap.
                                return err_with_count(FSERR_QUOTA_EXCEEDED, "WAL cap exceeded", 0);
                            }
                        }
                        let rc = unsafe {
                            libc::unlinkat(
                                parent.as_raw_fd(),
                                parent.leaf_ptr(),
                                libc::AT_REMOVEDIR,
                            )
                        };
                        if rc == 0 {
                            // DD-RemoveDirReplyShape: non-recursive success
                            // deletes exactly one entry (the target itself).
                            return ok_with_count(1);
                        }
                        let e = std::io::Error::last_os_error();
                        if cmode == ConsensusMode::Consensus {
                            let _ = wal.update_outcome_by_ack_hash(
                                ack_channel_hash(&ack_clone),
                                WalOutcome::Failure {
                                    code: io_err_code_u32(&e),
                                },
                            );
                        }
                        return err_with_count(io_err_code(&e), io_msg_scrub(&e), 0);
                    }
                    // Recursive path.
                    if cmode == ConsensusMode::Oracular {
                        // Oracular: existing readdir-loop unlinker, no
                        // WAL, count-carrying reply per
                        // DD-RemoveDirReplyShape (2026-09-03).  The
                        // walker now returns (n_deleted) on success
                        // and (n_before_error, io_error) on partial
                        // failure so we can bill per-entry cost
                        // symmetrically with Consensus recursive.
                        match remove_dir_recursive(parent.as_raw_fd(), parent.leaf_ptr()) {
                            Ok(n) => ok_with_count(n),
                            Err((n_before, e)) => {
                                err_with_count(io_err_code(&e), io_msg_scrub(&e), n_before)
                            }
                        }
                    } else {
                        // Consensus + recursive: sorted-post-order walk
                        // yielding RELATIVE paths (R5(b), 2026-09-02),
                        // per-entry journal + unlink, reply carries
                        // manifest of successfully-deleted entries as
                        // relative paths.  Under Shape A, both leader
                        // and follower walk their OWN per-validator
                        // subdir → byte-identical relative manifests
                        // → byte-identical WAL (via canon_wal_target
                        // .join(rel)) → byte-identical replies.
                        let manifest = match collect_recursive_manifest(&canon_target) {
                            Ok(m) => m,
                            Err(e) => {
                                // DD-RemoveDirReplyShape: manifest-walk
                                // failure on recursive Consensus (walk
                                // didn't start → empty deleted list).
                                return err_with_manifest(io_err_code(&e), io_msg_scrub(&e), &[]);
                            }
                        };
                        let mut deleted: Vec<(std::path::PathBuf, RemoveKind)> = Vec::new();
                        for (rel_path, kind) in manifest {
                            // wal_path is bundle-relative (Shape A
                            // invariant) for cross-validator WAL byte-
                            // identity.  Empty rel_path marks the target
                            // root itself (final post-order entry);
                            // Path::join with an empty component
                            // appends a trailing separator, so special-
                            // case that to preserve the target's path
                            // spelling in the WAL.
                            let wal_path = if rel_path.as_os_str().is_empty() {
                                canon_wal_target.clone()
                            } else {
                                canon_wal_target.join(&rel_path)
                            };
                            let op = match kind {
                                RemoveKind::File => WalOp::RemoveFile,
                                RemoveKind::Dir => WalOp::RemoveDir,
                            };
                            // per_entry_ack seeded on WAL path
                            // (bundle-relative) so leader + follower
                            // produce identical per-entry ack hashes.
                            let per_entry_ack = per_entry_ack_seed(&ack_clone, &wal_path);
                            if wal
                                .append_with_ack(
                                    WalEntry {
                                        op,
                                        path: wal_path,
                                        extra_path: None,
                                        offset: None,
                                        length: None,
                                        payload_ref: None,
                                        mode_bits: None,
                                        owner: None,
                                        group: None,
                                        outcome: WalOutcome::Success,
                                    },
                                    per_entry_ack,
                                )
                                .is_err()
                            {
                                return err_with_manifest(
                                    FSERR_QUOTA_EXCEEDED,
                                    "WAL cap exceeded during recursive removeDir",
                                    &deleted,
                                );
                            }
                            // TOCTOU-immune unlink via pinned dirfd chain
                            // from `parent` (post-security-review S-1,
                            // 2026-09-02).  Under the trust model
                            // (Consensus mode assumes the node is the
                            // only process on the machine) this is
                            // defense-in-depth; also matches the
                            // Oracular remove_dir_recursive's shape.
                            let unlink_rc =
                                unsafe { unlink_manifest_entry(&parent, &rel_path, kind) };
                            match unlink_rc {
                                Ok(()) => {
                                    deleted.push((rel_path, kind));
                                }
                                Err(e) => {
                                    let code_u32 = io_err_code_u32(&e);
                                    let _ = wal.update_outcome_by_ack_hash(
                                        per_entry_ack,
                                        WalOutcome::Failure { code: code_u32 },
                                    );
                                    return err_with_manifest(
                                        io_err_code(&e),
                                        io_msg_scrub(&e),
                                        &deleted,
                                    );
                                }
                            }
                        }
                        ok_recursive_manifest(&deleted)
                    }
                })
                .await
                .unwrap_or_else(|_je| {
                    // DD-RemoveDirReplyShape: shape picker based on
                    // (recursive, cmode) so the reply-hash verify path
                    // stays symmetric with the follower.
                    early_err_for_remove_dir(
                        recursive,
                        cmode,
                        FSERR_IO,
                        "spawn_blocking task failed",
                    )
                })
            }
            None => {
                // DD-RemoveDirReplyShape: parsed=None means args are
                // wrong-shape; recursive is unknown so pick the generic
                // 4-element count-carrying failure.
                err_with_count(FSERR_BAD_ARG, "expected (String, String, Bool, String)", 0)
            }
        };
        // H-29-3 lift follow-up (2026-08-27): per-entry cost
        // supplement.  Non-recursive → 1 attempted entry.  Consensus
        // recursive → manifest length (leader from fresh reply /
        // follower from `previous` — both derive the same count via
        // extract_removedir_manifest).  Oracular recursive → 0 (no
        // manifest available; both sides skip based on args-visible
        // cmode + recursive).  reserve_incremental_primitive tolerates
        // n=0 without a BugFoundError.
        let supp_n = fs_remove_dir_supplement_count(&parsed, cmode, &reply);
        self.metering.reserve_incremental_primitive(
            costs::fs_remove_dir_per_entry_supplement_cost(supp_n),
        )?;
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // chmod — (rootCanon, rel, modeBits) -> [true]
    // fchmodat(AT_SYMLINK_NOFOLLOW) — spec-mandated symlink safety.
    // -------------------------------------------------------------------
    pub async fn fs_chmod(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        // Phase 9 slice 9b-ii: charge fs_chmod weight at handler entry.
        // See fs_open for the rationale on placement before unapply.
        self.metering.reserve_primitive(costs::fs_chmod_cost())?;
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_chmod"));
        };
        // H-29-3 lift (2026-08-26): Consensus caps now journal to
        // the WAL BEFORE the syscall runs (same pattern as
        // `journal_write` / `journal_truncate`).  Argument shape
        // is `(root, rel, bits, cmode, ack)`.
        let [root_par, rel_par, mode_par, cmode_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_chmod"));
        };
        let cmode = match resolve_cmode(cmode_par) {
            Some(m) => m,
            None => {
                let out = vec![err(
                    FSERR_BAD_ARG,
                    "cmode must be String \"oracular\" or \"consensus\"",
                )];
                produce(&out, ack).await?;
                return Ok(out);
            }
        };
        // Parse the remaining args deterministically for both
        // sides.  Parsing failure → no journal (nothing was going
        // to happen on the leader either) + FSERR_BAD_ARG reply.
        let parsed = match (
            RhoString::unapply(root_par),
            RhoString::unapply(rel_par),
            RhoNumber::unapply(mode_par),
        ) {
            (Some(root), Some(rel), Some(bits)) if (0..=0o7777).contains(&bits) => {
                Some((root, rel, bits as u32))
            }
            _ => None,
        };
        // Journal pre-syscall on BOTH leader and follower — mirror
        // of fs_write's C-29-F1 pattern.  The WAL entry is fully
        // derived from args (canon_path from lexical join, mode
        // bits from parse) so both sides append identical bytes.
        if let Some((root, rel, bits)) = &parsed {
            let canon_path = canonicalize_lexical(root, rel);
            if self
                .journal_path_mutation_single(
                    cmode,
                    WalOp::Chmod,
                    canon_path,
                    Some(*bits),
                    None,
                    None,
                    ack,
                )
                .await
                .is_err()
            {
                let out = vec![err(FSERR_QUOTA_EXCEEDED, "WAL cap exceeded")];
                produce(&out, ack).await?;
                return Ok(out);
            }
        }
        // Phase 4 (Consensus re-execute + verify, 2026-09-02):
        // Path-based mutation.  Under Consensus, the follower now
        // re-executes fchmodat against its own subdir file via the
        // Shape A resolver, verifies the fresh reply's stable_hash
        // matches leader's cached, and either keeps the pre-appended
        // Success placeholder (on match) or flips it to Failure {
        // FSERR_CODE_CONSENSUS_DIVERGENCE } (on mismatch).  Oracular
        // follower is unchanged from the Phase-0 H-6 tautological
        // finalize path.  Same shape as fs_truncate's Phase-3
        // refactor since chmod's reply is `[true]` (no numeric
        // payload).
        if is_replay && cmode != ConsensusMode::Consensus {
            // Oracular follower — Phase-0 H-6 shape.
            if let Some(code_str) = extract_err_code(&previous) {
                self.finalize_failure_journal(fserr_to_code(&code_str), ack);
            }
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        // Fresh syscall — leader (always) or Consensus follower.
        // safe_descend_verified + fchmodat via the follower's own
        // resolved on-disk root (Shape A per-validator remap under
        // Consensus; identity fall-through under Oracular).
        let fresh_reply = match parsed {
            Some((root, rel, bits)) => {
                let root_pb = PathBuf::from(root);
                let (root_pb, expected_root_id) =
                    self.handles.root_registry.resolve_or_identity(&root_pb);
                let bits = bits as libc::mode_t;
                spawn_blocking(move || -> Par {
                    let parent = match safe_descend_verified(&root_pb, &rel, expected_root_id) {
                        Ok(p) => p,
                        Err(qe) => {
                            let (c, m) = quarantine_err_reply(&qe);
                            return err(c, m);
                        }
                    };
                    let rc = unsafe {
                        libc::fchmodat(
                            parent.as_raw_fd(),
                            parent.leaf_ptr(),
                            bits,
                            libc::AT_SYMLINK_NOFOLLOW,
                        )
                    };
                    if rc == 0 {
                        ok_bare()
                    } else {
                        let e = std::io::Error::last_os_error();
                        // ENOTSUP means the platform doesn't honor
                        // AT_SYMLINK_NOFOLLOW on chmod (Linux, some
                        // filesystems).  In that case there's no
                        // symlink-safe chmod primitive; report
                        // UNSUPPORTED so the caller sees the failure
                        // rather than silently following.
                        let code = if e.raw_os_error() == Some(libc::ENOTSUP)
                            || e.raw_os_error() == Some(libc::EOPNOTSUPP)
                        {
                            FSERR_UNSUPPORTED
                        } else {
                            io_err_code(&e)
                        };
                        err(code, io_msg_scrub(&e))
                    }
                })
                .await
                .unwrap_or_else(|_je| err(FSERR_IO, "spawn_blocking task failed"))
            }
            None => err(
                FSERR_BAD_ARG,
                "expected (String, String, u64<=0o7777, String)",
            ),
        };
        if is_replay {
            // Consensus follower — Phase 4 re-execute + verify.
            match verify_reply_hash_matches_cached(&fresh_reply, &previous) {
                Ok(()) => {
                    // Match: fresh reply is byte-identical to cached.
                    // H-6 finalize path applies to fresh symmetrically
                    // — if both sides saw the same syscall error, both
                    // finalize with the same code.
                    if let Some(code_str) = extract_err_code(std::slice::from_ref(&fresh_reply)) {
                        self.finalize_failure_journal(fserr_to_code(&code_str), ack);
                    }
                    let out = vec![fresh_reply];
                    produce(&out, ack).await?;
                    Ok(out)
                }
                Err(reason) => {
                    let divergence_reply = err(
                        FSERR_CONSENSUS_DIVERGENCE,
                        format!("fs_chmod follower re-execute diverges from leader: {reason}",),
                    );
                    self.finalize_failure_journal(FSERR_CODE_CONSENSUS_DIVERGENCE, ack);
                    let out = vec![divergence_reply];
                    produce(&out, ack).await?;
                    Ok(out)
                }
            }
        } else {
            // Leader path — H-6 finalize on syscall error.
            if let Some(code_str) = extract_err_code(std::slice::from_ref(&fresh_reply)) {
                self.finalize_failure_journal(fserr_to_code(&code_str), ack);
            }
            let out = vec![fresh_reply];
            produce(&out, ack).await?;
            Ok(out)
        }
    }

    // -------------------------------------------------------------------
    // chown — (rootCanon, rel, owner, group, cmode) -> [true]
    // Oracular: fchownat(AT_SYMLINK_NOFOLLOW), no WAL.
    // Consensus: BANNED with FSERR_UNSUPPORTED at handler entry
    // (post-2026-09-02 S-2 security review).  Reason: WAL captures
    // owner/group as caller-supplied String values, NSS-mapping to
    // uid/gid is host-local, and two validators with different NSS
    // configs would produce silent on-disk uid divergence that the
    // reply-hash Consensus verify cannot catch.  Lifting the ban
    // requires capturing resolved (uid, gid) in the WAL entry with
    // shard-wide NSS coordination — see the in-handler ban comment.
    // -------------------------------------------------------------------
    pub async fn fs_chown(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        // Phase 9 slice 9b-ii: charge fs_chown weight at handler entry.
        self.metering.reserve_primitive(costs::fs_chown_cost())?;
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_chown"));
        };
        // Slice 26: `(root, rel, owner, group, cmode, ack)`.
        let [root_par, rel_par, owner_par, group_par, cmode_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_chown"));
        };
        // C-26-F1 review fix: fail-closed on unrecognized cmode.
        let cmode = match resolve_cmode(cmode_par) {
            Some(m) => m,
            None => {
                let out = vec![err(
                    FSERR_BAD_ARG,
                    "cmode must be String \"oracular\" or \"consensus\"",
                )];
                produce(&out, ack).await?;
                return Ok(out);
            }
        };
        // Phase 4 ban (2026-09-02, post-security-review S-2):
        // fs_chown with cmode=Consensus is UNSUPPORTED.  Reason:
        // WAL captures owner/group as caller-supplied String values
        // (e.g., "bob"), and NSS-mapping ("bob" → uid) is host-
        // local — two validators with different /etc/passwd
        // entries would land different uids on-disk for the same
        // deploy without any signal to the consensus layer.  The
        // Consensus verify pattern (compare fresh vs cached reply
        // hash) doesn't catch this: fchownat's reply is `[true]`
        // regardless of the uid it actually stamped.
        //
        // Lifting the ban requires either (a) capturing the resolved
        // (uid, gid) in the WAL entry (not the display strings) and
        // requiring operators to coordinate NSS mappings shard-
        // wide, or (b) shipping fs_chown as an Oracular-only cap
        // and blocking Consensus per this gate.  Chose (b) —
        // matches `entriesStream* + Consensus` ban rationale
        // (feature is unsafe under naive expectations; reject at
        // handler entry with a specific error code).  Callers can
        // still use fs_chown under Oracular for per-node local
        // ownership changes.
        //
        // Pre-2026-09-02 the handler journaled Chown + called
        // fchownat under Consensus with cached-reply consumption
        // on replay — a Phase-0 tautological pattern that silently
        // masked NSS divergence.  This gate closes that surface.
        if cmode == ConsensusMode::Consensus {
            let out = vec![err(
                FSERR_UNSUPPORTED,
                "fs_chown: Consensus mode not supported — NSS mapping (owner/group \
                 name to uid/gid) is host-local and can differ across validators, \
                 producing silent on-disk divergence that the reply-hash verify \
                 cannot detect.  Use Oracular mode, or lift this ban by capturing \
                 resolved uid/gid in the WAL entry with shard-wide NSS coordination.",
            )];
            produce(&out, ack).await?;
            return Ok(out);
        }
        // Parse args deterministically for both sides.
        let parsed = match (RhoString::unapply(root_par), RhoString::unapply(rel_par)) {
            (Some(root), Some(rel)) => Some((
                root,
                rel,
                RhoString::unapply(owner_par),
                RhoString::unapply(group_par),
            )),
            _ => None,
        };
        // Journal pre-syscall on both sides (H-29-3 lift, 2026-08-26).
        // Post-S-2 ban above, this branch is Oracular-only:
        // `journal_path_mutation_single` returns Ok(false) for
        // cmode != Consensus, so no WAL entry is actually appended —
        // the call is kept for symmetry with the other path-mutation
        // handlers and in case the ban is ever lifted with proper
        // NSS coordination.
        if let Some((root, rel, owner_opt, group_opt)) = &parsed {
            let canon_path = canonicalize_lexical(root, rel);
            if self
                .journal_path_mutation_single(
                    cmode,
                    WalOp::Chown,
                    canon_path,
                    None,
                    owner_opt.clone(),
                    group_opt.clone(),
                    ack,
                )
                .await
                .is_err()
            {
                let out = vec![err(FSERR_QUOTA_EXCEEDED, "WAL cap exceeded")];
                produce(&out, ack).await?;
                return Ok(out);
            }
        }
        if is_replay {
            if let Some(code_str) = extract_err_code(&previous) {
                self.finalize_failure_journal(fserr_to_code(&code_str), ack);
            }
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match parsed {
            Some((root, rel, owner_opt, group_opt)) => {
                let root_pb = PathBuf::from(root);
                let (root_pb, expected_root_id) =
                    self.handles.root_registry.resolve_or_identity(&root_pb);
                chown_impl(&root_pb, rel, owner_opt, group_opt, expected_root_id).await
            }
            None => err(
                FSERR_BAD_ARG,
                "expected (String, String, String|Nil, String|Nil, String)",
            ),
        };
        if let Some(code_str) = extract_err_code(std::slice::from_ref(&reply)) {
            self.finalize_failure_journal(fserr_to_code(&code_str), ack);
        }
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // quarantine — (rootCanon, rel) -> [true, canonPath]
    // Standalone safety check that also returns a diagnostic display path
    // (procfs magic-link resolution of the parent dirfd + leaf).  Note:
    // the returned canonPath echoes back the (already caller-known)
    // resolved path; other handlers do NOT accept caller-supplied
    // canonPaths.
    // -------------------------------------------------------------------
    pub async fn fs_quarantine(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        // Phase 9 slice 9b-ii: charge fs_quarantine weight at handler entry.
        // See fs_open for the rationale on placement before unapply.
        self.metering
            .reserve_primitive(costs::fs_quarantine_cost())?;
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_quarantine"));
        };
        let [root_par, rel_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_quarantine"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match (RhoString::unapply(root_par), RhoString::unapply(rel_par)) {
            (Some(root), Some(rel)) => {
                let root_pb = PathBuf::from(&root);
                let (root_pb, expected_root_id) =
                    self.handles.root_registry.resolve_or_identity(&root_pb);
                spawn_blocking(move || -> Par {
                    match safe_descend_verified(&root_pb, &rel, expected_root_id) {
                        Ok(_) => {
                            // Return the caller-supplied joined path;
                            // safe_descend already verified it doesn't
                            // escape.  This is deterministic (no
                            // canonicalize call, so no host drift).
                            ok_string(root_pb.join(&rel).to_string_lossy().into_owned())
                        }
                        Err(qe) => {
                            let (c, m) = quarantine_err_reply(&qe);
                            err(c, m)
                        }
                    }
                })
                .await
                .unwrap_or_else(|_je| err(FSERR_IO, "spawn_blocking task failed"))
            }
            _ => err(FSERR_BAD_ARG, "expected (String, String)"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // entriesStream — (rootCanon, rel) -> [true, streamFd]
    // Placeholder: returns FSERR_UNSUPPORTED.  The backing streaming
    // primitive (a per-runtime dir-handle table analogous to
    // FileHandleTable, with `next(fd)` / `close(fd)` operators) is
    // scoped for Phase 1 tail-end but not yet implemented; Phase 4
    // wires the agent-side EntryStream on top of it.
    // -------------------------------------------------------------------
    pub async fn fs_entries_stream(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        // Phase 9 slice 9b-iv: charge fs_entries_stream SETUP cost only.
        // Weight = 50 (the base term).  Per-entry cost
        // (FS_ENTRIES_PER_ENTRY * n_entries) is deferred to a
        // follow-up slice because it requires post-syscall counting
        // on the leader branch and matching entry extraction from
        // `previous` on the replay branch — a two-branch charge
        // pattern rather than the single entry-point charge used
        // by the other length-parameterized handlers.
        self.metering
            .reserve_primitive(costs::fs_entries_stream_cost(0))?;
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_entries_stream"));
        };
        let [_root, _rel, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_entries_stream"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = err(
            FSERR_UNSUPPORTED,
            "entriesStream backing not yet implemented (Phase 1 tail-end)",
        );
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // Streaming-backing slice (2026-08-25) — three natives implementing
    // per-fd directory-entries streaming.  Companion to (eventually
    // replacing, once Dir.rho swaps) the bulk `entriesStream` stub
    // above.
    //
    // Safety pattern mirrors bulk `fs_entries`: `entriesStreamOpen` does
    // `safe_descend_verified` + `openat(O_DIRECTORY|O_NOFOLLOW|
    // O_CLOEXEC)` to get a TOCTOU-immune dirfd, then wraps it in
    // `fdopendir` for iteration.  The per-stream `DirHandle` lives in
    // `self.handles.dir_handles` (a `DirHandleTable` parallel to the
    // file handle table) and holds the `DIR*` behind a `Mutex`.
    //
    // D3 WAL wiring is Step 3 of this slice — the three handlers below
    // implement the oracular / non-journaled path.  Under Consensus
    // mode today, `entriesStreamNext` yields records but does NOT
    // append them to the WAL; consensus-mode replay parity across
    // validators works because the reply is replay-cached via
    // `non_deterministic_ops()`, but there is no independent
    // durability substrate yet.  Step 3 lands `WalOp::EntriesStreamNext`
    // and the symmetric leader/follower journal hooks.
    // -------------------------------------------------------------------

    /// `entriesStreamOpen(root, rel, cmode, ack)`.
    ///
    /// Args: `(String root, String rel, String cmode, ack)`.
    ///
    /// Success: `[true, streamFd]` where `streamFd` is an opaque u64
    /// allocated by `DirHandleTable`.  The fd is monotonic + state-
    /// hash-seeded (slice-28 aliasing prevention), so leader and
    /// follower produce the same fd for the same deploy call site.
    ///
    /// Failure: `[false, code, msg]` — usual FSERR_QUARANTINE /
    /// FSERR_BAD_ARG / FSERR_QUOTA_EXCEEDED / FSERR_IO shapes.
    ///
    /// Follower `is_replay = true`: shadow-insert at the leader's fd
    /// (extracted via `extract_ok_fd`) so future replay-branch
    /// handlers can look up `(cmode, canon_path)` symmetrically.  Same
    /// contract as `fs_open`'s C-R1 shadow-handle logic.
    pub async fn fs_entries_stream_open(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        // Charge setup cost.  Weight = fs_entries_stream_cost(0) =
        // FS_ENTRIES_SETUP = 50 — same as bulk `fs_entries` setup so
        // the two variants are cost-comparable at open.  The alias
        // `fs_entries_stream_open_cost` exists to satisfy the per-
        // handler naming pin in `fileio_cost_spec`.
        self.metering
            .reserve_primitive(costs::fs_entries_stream_open_cost())?;
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_entries_stream_open"));
        };
        let [root_par, rel_par, cmode_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_entries_stream_open"));
        };
        if is_replay {
            // Shadow-insert at the leader's fd so subsequent replay-
            // branch handlers (entriesStreamNext, entriesStreamClose)
            // can look up cmode / canon_path from the DirHandleTable.
            //
            // 2026-08-30 review-follow-up: use `extract_ok_fd` (bit-
            // preserving reinterpret) for the same rationale as
            // fs_open — see `extract_ok_fd`'s docstring.
            //
            // Under the Phase 2 ban (2026-09-01), Consensus-mode
            // opens are rejected leader-side (see below), so
            // `extract_ok_fd` on a Consensus-cap open returns None
            // (cached reply is `[false, FSERR_UNSUPPORTED, ...]`)
            // and no shadow gets inserted.  Only Oracular streams
            // reach this shadow-install path.  Kept as metadata-only
            // shadow (matches the pre-Phase-2 Phase-0 shape) — no
            // real-open needed since Oracular streams don't
            // re-execute on the follower.
            if let Some(fd) = extract_ok_fd(&previous) {
                if let (Some(root), Some(rel)) =
                    (RhoString::unapply(root_par), RhoString::unapply(rel_par))
                {
                    // Fall back to Consensus on bogus cmode — matches
                    // fs_open's C-R1 fallback rationale (a bogus cmode
                    // with a [true, fd] cached reply is definitionally
                    // a bug; fail-closed to the more restrictive mode).
                    let cmode = resolve_cmode(cmode_par).unwrap_or(ConsensusMode::Consensus);
                    let deploy = *self
                        .handles
                        .current_deploy_scope
                        .read()
                        .expect("current_deploy_scope RwLock poisoned");
                    let shadow =
                        DirHandle::shadow(canonicalize_lexical(&root, &rel), cmode, deploy);
                    // Ignore the return: on a fresh follower the slot
                    // is empty; on a repeat call the existing handle
                    // wins.  Real divergence surfaces later.
                    let _ = self
                        .handles
                        .dir_handles
                        .insert_at(fd.as_u64(), shadow)
                        .await;
                }
            }
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        // Leader path.
        let cmode = match resolve_cmode(cmode_par) {
            Some(m) => m,
            None => {
                let out = vec![err(
                    FSERR_BAD_ARG,
                    "cmode must be String \"oracular\" or \"consensus\"",
                )];
                produce(&out, ack).await?;
                return Ok(out);
            }
        };
        // Phase 2 ban (Consensus re-execute + verify, 2026-09-01):
        // Consensus + entriesStream* is UNSUPPORTED.  Reason:
        // `readdir` iteration order is fs-dependent and not
        // guaranteed to be stable across per-validator subdirs (D3);
        // two validators with independently-created copies of the
        // same logical directory could yield entries in different
        // orders, tripping spurious CONSENSUS_DIVERGENCE.  Bulk
        // `fs_entries` avoids this because it sorts.  Under Phase-0
        // this hazard was invisible (follower consumed cached reply);
        // Phase 2's re-execute exposes it.  Rather than ship a
        // Consensus-cap primitive with a non-obvious readdir-order
        // correctness constraint operators must satisfy externally,
        // reject at open time and direct users to `fs_entries`.
        //
        // Lifting the ban requires either DirIter-level canonical
        // ordering (buffer + sort, but that breaks the streaming
        // semantic + per-Next journaling) or a spec change making
        // operator responsibility for readdir-order stability
        // explicit.  Slotted for Phase 4/5.
        if cmode == ConsensusMode::Consensus {
            let out = vec![err(
                FSERR_UNSUPPORTED,
                "entriesStream* is not supported on Consensus caps — readdir order \
                 is fs-dependent and not stable across per-validator subdirs.  Use \
                 `fs_entries` (sorted, deterministic across validators) instead.",
            )];
            produce(&out, ack).await?;
            return Ok(out);
        }
        let (root, rel) = match (RhoString::unapply(root_par), RhoString::unapply(rel_par)) {
            (Some(r), Some(l)) => (r, l),
            _ => {
                let out = vec![err(FSERR_BAD_ARG, "expected (String root, String rel)")];
                produce(&out, ack).await?;
                return Ok(out);
            }
        };
        let root_pb = PathBuf::from(&root);
        let (root_pb, expected_root_id) = self.handles.root_registry.resolve_or_identity(&root_pb);
        let rel_for_open = rel.clone();
        // safe_descend + openat + fdopendir in a blocking task —
        // mirrors bulk fs_entries' opening syscall sequence exactly
        // so TOCTOU-immunity + O_NOFOLLOW semantics carry through.
        //
        // Err returns a `Box<Par>` (not bare `Par`) so the enclosing
        // Result stays pointer-sized — clippy `result_large_err` flags
        // a naked ~296-byte error variant.
        let opened = spawn_blocking(move || -> Result<DirIter, Box<Par>> {
            let parent = match safe_descend_verified(&root_pb, &rel_for_open, expected_root_id) {
                Ok(p) => p,
                Err(qe) => {
                    let (code, msg) = quarantine_err_reply(&qe);
                    return Err(Box::new(err(code, msg)));
                }
            };
            let dir_fd = unsafe {
                libc::openat(
                    parent.as_raw_fd(),
                    parent.leaf_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            if dir_fd < 0 {
                let e = std::io::Error::last_os_error();
                return Err(Box::new(err(io_err_code(&e), io_msg_scrub(&e))));
            }
            // L-3 pattern (fs_entries): F_DUPFD_CLOEXEC on the fd
            // handed to fdopendir so the DIR*'s underlying fd
            // carries CLOEXEC atomically.  Close the original.
            let read_fd = unsafe { libc::fcntl(dir_fd, libc::F_DUPFD_CLOEXEC, 0) };
            unsafe { libc::close(dir_fd) };
            if read_fd < 0 {
                let e = std::io::Error::last_os_error();
                return Err(Box::new(err(io_err_code(&e), io_msg_scrub(&e))));
            }
            DirIter::from_dir_fd(read_fd)
                .map_err(|e| Box::new(err(io_err_code(&e), io_msg_scrub(&e))))
        })
        .await
        .unwrap_or_else(|_je| Err(Box::new(err(FSERR_IO, "spawn_blocking task failed"))));
        let reply = match opened {
            Ok(iter) => {
                let deploy = *self
                    .handles
                    .current_deploy_scope
                    .read()
                    .expect("current_deploy_scope RwLock poisoned");
                let handle = DirHandle::new(iter, canonicalize_lexical(&root, &rel), cmode, deploy);
                match self.handles.dir_handles.insert(handle).await {
                    Ok(fd) => ok_u64(fd),
                    Err(()) => err(
                        FSERR_QUOTA_EXCEEDED,
                        "per-runtime dir-stream fd cap reached",
                    ),
                }
            }
            Err(e_par) => *e_par,
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    /// `entriesStreamNext(streamFd, ack)`.
    ///
    /// Args: `(GInt streamFd, ack)`.  `cmode` is captured in the
    /// DirHandle at open time, not re-passed here — matches the fd-
    /// based natives (fs_read, fs_write, ...) that also look up
    /// cmode from the handle.
    ///
    /// Success: `[true, entryRecord]` — one entry, same
    /// `stat_record`-shaped Par that bulk `fs_entries` emits per row.
    /// End of stream: `[false, "EOS"]` — 2-element terminator,
    /// distinguishable from error `[false, code, msg]` shapes.
    /// Error: `[false, code, msg]` — FSERR_CLOSED / FSERR_IO / etc.
    ///
    /// Cost: per-call setup `fs_entries_stream_cost(0)` at entry, plus
    /// a per-entry supplement of `fs_entries_stream_per_entry_supplement_cost(1)`
    /// after a successful entry (0 on EOS or error).  Two-branch shape
    /// mirrors `fs_entries` so the D3 canonical event log is byte-
    /// identical across leader and follower on both success and EOS
    /// paths.
    pub async fn fs_entries_stream_next(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        // Per-call setup portion of the two-branch charge.  Per-entry
        // supplement is charged post-reply via
        // `fs_entries_stream_per_entry_supplement_cost` (n=1 on
        // `[true, ...]`, n=0 on EOS / error).  The alias
        // `fs_entries_stream_next_cost` exists to satisfy the per-
        // handler naming pin in `fileio_cost_spec`.
        self.metering
            .reserve_primitive(costs::fs_entries_stream_next_cost())?;
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_entries_stream_next"));
        };
        let [fd_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_entries_stream_next"));
        };
        // Note (2026-09-01 Phase 2 ban): Consensus + entriesStream*
        // is rejected at entriesStreamOpen, so under normal flow no
        // Consensus stream fd ever reaches this handler.  Oracular
        // caps use the Phase-0 tautological replay path (follower
        // consumes cached reply); the leader path below runs the
        // real `readdir` on the DIR*.  If a future change lifts the
        // ban, this handler needs Phase-2 re-execute + verify
        // treatment (see git history for the pre-ban shape).
        if is_replay {
            // Per-entry supplement charge on the replay branch: n = 1
            // when the cached reply is `[true, entryRecord]`, else n = 0
            // (EOS or error).  `reply_is_ok` only checks the head is
            // `true` — dedicated helper vs. `extract_ok_u64` which
            // additionally requires the payload to be an int.
            let n = if reply_is_ok(&previous) { 1 } else { 0 };
            self.metering.reserve_incremental_primitive(
                costs::fs_entries_stream_per_entry_supplement_cost(n),
            )?;
            // Journal the cached reply if the fd's shadow reports a
            // Consensus cmode.  Under the Phase 2 ban this branch is
            // unreachable in normal flow (no Consensus stream fds
            // exist).  `journal_state_read` self-guards on Consensus
            // → this call is a WAL no-op for Oracular; kept for
            // structural parity with the leader path's journal call
            // and as a load-bearing defense-in-depth site if a future
            // change lifts the ban without re-adding Phase-2 wiring
            // here.
            if let Some(fd) = RhoNumber::unapply(fd_par) {
                if let Some(handle) = self.handles.dir_handles.get(fd as u64).await {
                    if let Some(reply_par) = previous.first() {
                        self.journal_state_read(
                            handle.cmode,
                            WalOp::EntriesStreamNext,
                            handle.canon_path.clone(),
                            reply_par,
                            ack,
                            Some(n),
                        );
                    }
                }
            }
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        // Leader path.
        let fd = match RhoNumber::unapply(fd_par) {
            Some(n) => n as u64,
            None => {
                let out = vec![err(FSERR_BAD_ARG, "expected GInt streamFd")];
                produce(&out, ack).await?;
                return Ok(out);
            }
        };
        // Hold onto the Arc so we can re-consult (cmode, canon_path)
        // for the post-reply journal call without a second table
        // lookup.  `handle_opt` is dropped at the end of this fn, so
        // the Arc keeps the DirHandle alive for the whole path.
        let handle_opt = self.handles.dir_handles.get(fd).await;
        let reply = if let Some(handle) = handle_opt.as_ref() {
            let cmode = handle.cmode;
            let iter_lock = handle.iter.lock().await;
            match iter_lock.as_ref() {
                None => err(
                    FSERR_CLOSED,
                    "shadow stream handle has no iterator (leader path only)",
                ),
                Some(iter) => {
                    // Pass the DIR* address across spawn_blocking
                    // via usize cast (raw pointers are not Send).
                    // The MutexGuard `iter_lock` is held across the
                    // .await below, keeping the address live for
                    // the closure's whole lifetime.
                    let dirp_addr = iter.as_ptr() as usize;
                    spawn_blocking(move || -> Par {
                        let dirp = dirp_addr as *mut libc::DIR;
                        readdir_one_entry(dirp, cmode)
                    })
                    .await
                    .unwrap_or_else(|_je| err(FSERR_IO, "spawn_blocking task failed"))
                }
            }
        } else {
            err(FSERR_CLOSED, "stream fd closed or unknown")
        };
        // Post-reply supplement charge — same two-branch shape as
        // fs_entries.  n = 1 on `[true, ...]`, else 0.  Uses
        // `reserve_incremental_primitive` (not `reserve_primitive`)
        // because the n=0 case (EOS or error) legitimately produces
        // a zero-weight cost — `reserve_primitive` returns
        // `BugFoundError` on zero, which would silently poison the
        // deploy's EvaluateResult.errors.
        let n = if reply_is_ok(std::slice::from_ref(&reply)) {
            1
        } else {
            0
        };
        self.metering
            .reserve_incremental_primitive(costs::fs_entries_stream_per_entry_supplement_cost(n))?;
        // Journal the leader's reply if the handle is a Consensus cap.
        // `journal_state_read` skips Oracular caps internally.  Under
        // the Phase 2 ban this WAL-append is unreachable in normal
        // flow, since Consensus stream opens are rejected at
        // entriesStreamOpen — but kept as a load-bearing site for the
        // future case if the ban is lifted with re-execute wiring.
        if let Some(handle) = handle_opt.as_ref() {
            self.journal_state_read(
                handle.cmode,
                WalOp::EntriesStreamNext,
                handle.canon_path.clone(),
                &reply,
                ack,
                Some(n),
            );
        }
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    /// `entriesStreamClose(streamFd, ack)`.
    ///
    /// Args: `(GInt streamFd, ack)`.  Idempotent — closing an already-
    /// removed fd is a no-op that still returns `[true]`, matching
    /// `fs_close`'s shape.  Dropping the `DirHandle` runs `DirIter`'s
    /// `closedir` which releases the underlying kernel dirp fd.
    ///
    /// Cost: `fs_close_cost()` — same class as `fs_close`; the work is
    /// a hashmap remove + `closedir` syscall.
    pub async fn fs_entries_stream_close(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        // Aliased to `fs_close_cost` semantically (fd release +
        // closedir).  The alias `fs_entries_stream_close_cost` exists
        // to satisfy the per-handler naming pin in `fileio_cost_spec`.
        self.metering
            .reserve_primitive(costs::fs_entries_stream_close_cost())?;
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_entries_stream_close"));
        };
        let [fd_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_entries_stream_close"));
        };
        if is_replay {
            // Also drop the shadow handle so the follower's dir_handles
            // table converges to the leader's post-close state.
            if let Some(fd) = RhoNumber::unapply(fd_par) {
                self.handles.dir_handles.remove(fd as u64).await;
            }
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match RhoNumber::unapply(fd_par) {
            Some(fd) => {
                self.handles.dir_handles.remove(fd as u64).await;
                ok_bare()
            }
            None => err(FSERR_BAD_ARG, "expected GInt streamFd"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // Phase 8 slice 8a — range-lock natives.
    //
    // These acquire and release entries in `self.handles.lock_registry`,
    // the RuntimeManager-broadcast `LockRegistry` (X-1 design memo).
    // The lock table is keyed on `(dev, inode)` so cross-cap
    // coordination on the same physical file collapses to one entry
    // regardless of which fresh-mint `File` cap holds it (slice 27).
    //
    // Wait:false only for MVP.  Blocking acquisition (wait:true) is
    // slice 8b via Rig-protocol; every acquire here returns immediately
    // with either `[true, lock_id]` or `[false, FSERR_BUSY, ...]`.
    //
    // WAL journaling of `LockAcquire` / `LockRelease` entries is step 4
    // of slice 8a — deferred here.  The natives resolve the acquire
    // outcome but do not yet append WAL entries.  Under consensus mode
    // they will need to (per X-1 §4); under oracular they will not
    // (per §Mode-differentiated invariants — oracular locks are
    // in-process hints, not consensus state).
    //
    // Deploy-end auto-release (MUST per X-4 / spec §Explicit locks)
    // is wired at `casper::rholang::runtime::WalDeployScope`'s Drop
    // in the casper crate (step 5, 2026-08-13).  The RAII guard
    // constructed at deploy-entry sets `handles.current_deploy_scope`
    // to a Blake2b256-derived scope; on Drop, the guard calls
    // `lock_registry.release_all_for_deploy(&scope)` before clearing
    // the scope cell back to the `[0; 32]` sentinel.  These handlers
    // read the scope from `handles.current_deploy_scope` at acquire
    // time, so a leaked lock (caller neither released nor closed
    // File before deploy end) gets swept transparently at deploy end.
    //
    // Replay semantics: on `is_replay = true` these natives echo
    // `previous` and do NOT touch `LockRegistry`.  Follower registry
    // state diverges from the leader's, but that divergence is never
    // consensus-observable because every reply is captured — no
    // consensus-observable code path consults `LockRegistry` outside
    // the replay-cached natives.  When step 4 adds WAL journaling of
    // `LockAcquire` / `LockRelease` entries, the follower's state
    // MUST be reconstituted from the WAL during replay (mirror slice
    // 29's `journal_write` / `finalize_write_journal` pattern) so
    // that step 7's consensus-mode unlink gate (`is_locked` in
    // `fs_remove_file` / `fs_remove_dir`) sees the same state on
    // leader and follower.  Under oracular mode the LockRegistry is
    // best-effort per §Mode-differentiated invariants, so follower
    // state doesn't matter there either way.
    // -------------------------------------------------------------------

    /// Acquire a positional range lock on the file behind `fd`.
    ///
    /// Args: `(fd: u64, offset: u64, length: u64, mode: String("r"|"w"),
    /// holder: Par, cmode: String, ack)`
    ///
    /// **Fd-based keying — critical for oracular correctness.**  The
    /// lock keys on `fstat(fd).(st_dev, st_ino)` — the physical file
    /// `fd` points at — NOT on any current-path resolution.  Under
    /// oracular mode a caller's fd may point at an inode different
    /// from whatever's currently at the original path (external
    /// process could have `mv`d the file); keying on the fd's inode
    /// keeps the lock consistent with the reads/writes it protects
    /// (which also operate through `fd`).  Under consensus mode the
    /// path is stable so either keying would be correct — but fd is
    /// uniformly right.
    ///
    /// - `holder` is opaque Par (per convention: the caller-cap's
    ///   per-instance `this` GPrivate name, NOT the module-level
    ///   `stateP`) hashed to a stable 32-byte `HolderId` — used by
    ///   `File.close`'s `release_all_for_holder` sweep.  See `HolderId`
    ///   docstring in `lock.rs` for the "why per-instance" rationale
    ///   — passing a shared module-level name here collapses every
    ///   cap into one holder and breaks cross-cap coordination.
    /// - `cmode` is validated but not currently branched on at acquire
    ///   time; step 4 (WAL) and step 7 (unlink gate) will.
    ///
    /// Reply: `[true, lock_id: Int]` on success, `[false, code, msg]`
    /// on error (FSERR_CLOSED if fd unknown or shadow handle,
    /// FSERR_BUSY, FSERR_BAD_ARG, FSERR_QUOTA_EXCEEDED, FSERR_IO on
    /// fstat failure).
    pub async fn fs_lock_range(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        // Phase 9 slice 9b-ii: charge fs_lock_range weight at handler entry.
        // See fs_open for the rationale on placement before unapply.
        self.metering
            .reserve_primitive(costs::fs_lock_range_cost())?;
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_lock_range"));
        };
        // Slice-8b sub-4 arity tightening (2026-08-26): accept only
        // arity 8 (fd, off, len, mode, holder, cmode, wait, ack)
        // with `wait: Bool` at slot 7.  The legacy arity-7 (no
        // `wait`) branch was retired now that every File.rho caller
        // threads an explicit wait argument.  A stray arity-7
        // invocation falls to `illegal_argument_error` — mirrors
        // the sub-2 commit's transitional-shim removal plan.
        let (fd_par, off_par, len_par, mode_par, holder_par, cmode_par, wait, ack) =
            match args.as_slice() {
                [fd, off, len, mode, holder, cmode, wait_par, ack] => {
                    match RhoBoolean::unapply(wait_par) {
                        Some(b) => (fd, off, len, mode, holder, cmode, b, ack),
                        None => {
                            let out = vec![err(FSERR_BAD_ARG, "fs_lock_range: wait must be Bool")];
                            produce(&out, ack).await?;
                            return Ok(out);
                        }
                    }
                }
                _ => return Err(illegal_argument_error("fs_lock_range")),
            };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        // cmode validation: rejects bad shapes even though acquire
        // outcome doesn't currently branch on it — step 4 (WAL) and
        // step 7 (unlink gate) will.  Fail-closed matches the pattern
        // of every other cmode-taking native.
        let _cmode = match resolve_cmode(cmode_par) {
            Some(m) => m,
            None => {
                let out = vec![err(
                    FSERR_BAD_ARG,
                    "cmode must be String \"oracular\" or \"consensus\"",
                )];
                produce(&out, ack).await?;
                return Ok(out);
            }
        };
        let policy = if wait {
            WaitPolicy::Wait
        } else {
            WaitPolicy::Fail
        };
        let reply = match (
            RhoNumber::unapply(fd_par),
            RhoNumber::unapply(off_par),
            RhoNumber::unapply(len_par),
            RhoString::unapply(mode_par),
            resolve_lock_mode(mode_par),
        ) {
            // Slice 28 CRIT-2 pattern: fds are hash-derived u64 bit
            // patterns; the sign bit carries information, so `fd as
            // u64` is the reinterpret and we do NOT gate on `fd >= 0`.
            // (Same fix as `fs_close` line 600.  Pre-fix: ~50% of
            // seeded fds had the high bit set, so lock acquires
            // failed intermittently with FSERR_BAD_ARG.  Repro via
            // `fileio_examples_spec::fileio_lockrange_cross_cap_busy_then_release`.)
            (Some(fd), Some(off), Some(len), Some(_), Some(lm)) if off >= 0 && len > 0 => {
                match self.dev_inode_from_fd(fd as u64).await {
                    Ok(dev_inode) => {
                        let holder = holder_id_of(holder_par);
                        // Step 5: read the per-runtime "current deploy
                        // scope" cell set by WalDeployScope::new at
                        // deploy entry.  Sentinel [0; 32] means "no
                        // deploy in flight" (test or genesis path);
                        // that's safe here — acquire doesn't validate
                        // the scope, only release_all_for_deploy does.
                        let deploy = *self
                            .handles
                            .current_deploy_scope
                            .read()
                            .expect("current_deploy_scope RwLock poisoned");
                        match self.handles.lock_registry.try_acquire_range_wait(
                            dev_inode, off as u64, len as u64, lm, holder, deploy, policy,
                        ) {
                            Ok(AcquireOutcome::Immediate(id)) => ok_u64(id.0),
                            Ok(AcquireOutcome::Parked { admit, .. }) => {
                                // Await admission (release-triggered
                                // wake) or cancellation (deploy-end
                                // sweep via WalDeployScope::drop —
                                // sub-3, or explicit cancel_wait).
                                //
                                // Runtime concern (slice-8b sub-3):
                                // this await lives inside the deploy's
                                // eval future.  If nothing signals the
                                // oneshot, the eval future hangs.
                                // Sub-3's WalDeployScope::drop invokes
                                // `cancel_all_waiters_for_deploy` at
                                // deploy end to guarantee no waiter is
                                // leaked past deploy boundary — see
                                // that commit for the full lifecycle.
                                match admit.await {
                                    Ok(Ok(id)) => ok_u64(id.0),
                                    Ok(Err(le)) => lock_err_reply(le),
                                    // Sender dropped without a signal
                                    // (registry drop or unusual state
                                    // sequence) — surface as Cancelled
                                    // per the AcquireOutcome::Parked
                                    // docstring.
                                    Err(_recv_error) => lock_err_reply(LockError::Cancelled),
                                }
                            }
                            Err(le) => lock_err_reply(le),
                        }
                    }
                    Err((code, msg)) => err(code, msg),
                }
            }
            _ => err(
                FSERR_BAD_ARG,
                "expected (u64, u64, u64>0, String\"r|w\", Par, String, Bool)",
            ),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    /// Acquire the whole-file sequential lock on the file behind `fd`.
    ///
    /// Called by sequential-stream constructors (`chars`, `bytes`,
    /// `lines`, `readLine`, `writeChars`, `writeBytes`, `writeLine`,
    /// `writeLines`, `writeString`, `writeByteArray`) before they
    /// begin producing.  Enforces "one active sequential stream per
    /// File" (FIP §1132) at the physical file level rather than at
    /// the cap level, so cross-cap sequential streams on the same
    /// inode also conflict per §1182 — using fd-based keying (see
    /// `fs_lock_range` docstring for the oracular-correctness
    /// rationale).
    ///
    /// Args: `(fd: u64, holder: Par, cmode: String, ack)`
    /// Reply: `[true, lock_id: Int]` or `[false, code, msg]`.
    pub async fn fs_lock_sequential(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        // Phase 9 slice 9b-ii: charge fs_lock_sequential weight at handler entry.
        // See fs_open for the rationale on placement before unapply.
        self.metering
            .reserve_primitive(costs::fs_lock_sequential_cost())?;
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_lock_sequential"));
        };
        // Slice-8b sub-4 arity tightening (2026-08-26): accept only
        // arity 5 (fd, holder, cmode, wait, ack).  The legacy arity-4
        // branch was retired now that every File.rho caller threads
        // an explicit wait argument.
        let (fd_par, holder_par, cmode_par, wait, ack) = match args.as_slice() {
            [fd, holder, cmode, wait_par, ack] => match RhoBoolean::unapply(wait_par) {
                Some(b) => (fd, holder, cmode, b, ack),
                None => {
                    let out = vec![err(FSERR_BAD_ARG, "fs_lock_sequential: wait must be Bool")];
                    produce(&out, ack).await?;
                    return Ok(out);
                }
            },
            _ => return Err(illegal_argument_error("fs_lock_sequential")),
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let _cmode = match resolve_cmode(cmode_par) {
            Some(m) => m,
            None => {
                let out = vec![err(
                    FSERR_BAD_ARG,
                    "cmode must be String \"oracular\" or \"consensus\"",
                )];
                produce(&out, ack).await?;
                return Ok(out);
            }
        };
        let policy = if wait {
            WaitPolicy::Wait
        } else {
            WaitPolicy::Fail
        };
        let reply = match RhoNumber::unapply(fd_par) {
            // Slice 28 CRIT-2 pattern: fds are hash-derived u64 bit
            // patterns; sign bit carries information, so we reinterpret
            // via `fd as u64` without gating on `fd >= 0`.  Same fix
            // as `fs_close` / `fs_lock_range`.  Pre-fix: ~50% of seeded
            // fds had the high bit set → sequential-lock acquires (all
            // stream producers, writeByteArray, etc.) failed
            // intermittently with FSERR_BAD_ARG under real bundle
            // openFile.
            Some(fd) => match self.dev_inode_from_fd(fd as u64).await {
                Ok(dev_inode) => {
                    let holder = holder_id_of(holder_par);
                    // Step 5: read per-runtime "current deploy scope" cell.
                    let deploy = *self
                        .handles
                        .current_deploy_scope
                        .read()
                        .expect("current_deploy_scope RwLock poisoned");
                    match self
                        .handles
                        .lock_registry
                        .try_acquire_sequential_wait(dev_inode, holder, deploy, policy)
                    {
                        Ok(AcquireOutcome::Immediate(id)) => ok_u64(id.0),
                        Ok(AcquireOutcome::Parked { admit, .. }) => match admit.await {
                            Ok(Ok(id)) => ok_u64(id.0),
                            Ok(Err(le)) => lock_err_reply(le),
                            Err(_recv_error) => lock_err_reply(LockError::Cancelled),
                        },
                        Err(le) => lock_err_reply(le),
                    }
                }
                Err((code, msg)) => err(code, msg),
            },
            _ => err(FSERR_BAD_ARG, "expected (u64, Par, String, Bool)"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    /// Resolve a Rholang-supplied fd to its `(st_dev, st_ino)` pair
    /// via `fstat(2)`.  Common core of `fs_lock_range` /
    /// `fs_lock_sequential` (post-review-2 fix, 2026-08-12): both
    /// natives key their LockRegistry entry on the physical file the
    /// fd points at, not on any current-path resolution, so cross-cap
    /// coordination is oracular-correct.
    ///
    /// Returns `Err((FSERR_CLOSED, ...))` if the fd is unknown to the
    /// handle table or is a shadow handle (replay-only), and
    /// `Err((FSERR_IO, ...))` on fstat failure.
    ///
    /// # Concurrent close race — documented as caller-bug behavior
    ///
    /// `raw_fd` returns the raw OS fd as a bare `i32` and drops the
    /// handle-table lock.  Between the return and the `fstat` below,
    /// a concurrent `fs_close(fd)` on the SAME cap could drop the
    /// tokio `File`, close the OS fd, and let a subsequent `open` in
    /// another handler reuse that fd number for a different inode —
    /// making `fstat` stat the wrong file.
    ///
    /// This race is only exploitable by a caller doing incoherent
    /// concurrent close-and-lock on their OWN cap: under fresh-mint
    /// semantics each cap owns its own fd; other caps have distinct
    /// fds and can't close it.  Under H-7's play/replay isolation
    /// each has its own `FileHandleTable`, so no cross-runtime race
    /// either.  Under D3's `FuturesUnordered` two Par branches of a
    /// single deploy sharing the same cap could race — but "close
    /// while locking" is user-code incoherence, not a security
    /// vulnerability.  Outcome: either `FSERR_CLOSED` (if raw_fd
    /// missed too) or a `(dev, ino)` for whatever file happens to
    /// hold that raw fd at the moment of fstat.  Neither leaks
    /// authority — the caller closed their own fd; if the lock
    /// registry ends up keyed on a different inode, only that
    /// caller sees the confusion, and their subsequent reads/writes
    /// on the (now closed) fd will fail with `FSERR_CLOSED` anyway.
    async fn dev_inode_from_fd(&self, fd: u64) -> Result<(u64, u64), (&'static str, String)> {
        let Some(raw) = self.handles.raw_fd(fd).await else {
            return Err((FSERR_CLOSED, "fd unknown or shadow handle".to_string()));
        };
        #[cfg(unix)]
        {
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
            let _ = raw;
            Ok((0, 0))
        }
    }

    /// Release a previously-acquired lock by id.  Both positional
    /// (`fs_lock_range`) and sequential (`fs_lock_sequential`) locks
    /// release through this native — the id space is unified.
    ///
    /// Args: `(lock_id: u64, ack)`
    /// Reply: `[true]` on success, `[false, FSERR_CLOSED, msg]` if
    /// the id isn't held (double release, cap-close swept it first,
    /// etc.  Idempotent behavior mirrors `File.close` /
    /// `Stream.close`).
    pub async fn fs_release_lock(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        // Phase 9 slice 9b-ii: charge fs_release_lock weight at handler entry.
        // See fs_open for the rationale on placement before unapply.
        self.metering
            .reserve_primitive(costs::fs_release_lock_cost())?;
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_release_lock"));
        };
        let [id_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_release_lock"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match RhoNumber::unapply(id_par) {
            Some(n) if n >= 0 => match self.handles.lock_registry.release(LockId(n as u64)) {
                Ok(()) => ok_bare(),
                Err(le) => lock_err_reply(le),
            },
            _ => err(FSERR_BAD_ARG, "expected (u64)"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    /// Sweep every positional and sequential lock owned by `holder` from
    /// the `LockRegistry`.  Called by `File.close` before dispatching
    /// `fs_close`, so a File cap that still holds locks at close time
    /// doesn't strand them until deploy-end auto-release fires.
    ///
    /// Args: `(holder: Par, ack)` — holder is opaque Par (per
    /// convention: the caller-cap's per-instance `this` GPrivate name,
    /// NOT `stateP`) hashed to a stable 32-byte `HolderId`, matching
    /// the derivation used at acquire time.  See `HolderId` docstring
    /// in `lock.rs` for why per-instance and not per-module.
    ///
    /// Reply: `[true, released_count: Int]`.  Zero released is not an
    /// error — a cap that never acquired anything sweeps zero.  This
    /// native is deliberately best-effort: it can never fail on
    /// well-typed input, mirroring the "close is always safe to call"
    /// invariant of `File.close` / `Stream.close`.  Subsequent
    /// `lockToken!release()` on now-orphaned tokens returns
    /// `[false, FSERR_CLOSED, ...]` through `fs_release_lock`'s
    /// unknown-id path — the caller sees a clean idempotent error.
    ///
    /// Cross-cap safety: locks held on the same `(dev, inode)` via
    /// *other* File caps are unaffected — the sweep is scoped by
    /// `HolderId` equality, and each fresh-mint `File.openFile` cap
    /// derives a distinct `HolderId` from its own `stateP`.
    pub async fn fs_release_all_for_holder(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        // Phase 9 slice 9b-ii: charge fs_release_all_for_holder weight at handler entry.
        // See fs_open for the rationale on placement before unapply.
        self.metering
            .reserve_primitive(costs::fs_release_all_for_holder_cost())?;
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_release_all_for_holder"));
        };
        let [holder_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_release_all_for_holder"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let holder = holder_id_of(holder_par);
        // Slice-8b sub-6 review round-2 (2026-08-12): cancel-first,
        // release-second — SAME ordering as WalDeployScope::drop
        // (B1 fix).  Rationale: `release_all_for_holder` internally
        // calls `wake_waiters(state)` after removing this holder's
        // held entries.  If this same holder has a parked waiter
        // (concrete: cap held sequential + parked wait:true range),
        // the wake path admits it — sequential-vs-positional exclusion
        // no longer blocks (sequential just got released; ranges
        // empty), and same-holder-skip trivially permits self.
        // The subsequently-run cancel_all_waiters_for_holder then
        // finds nothing to cancel; the just-admitted entry LEAKS
        // attached to a now-closed cap.  Reversed order (cancel first,
        // then release) kills the parked waiter before wake_waiters
        // can promote it.  Mirrors the B1 fix pattern exactly.
        let cancelled = self
            .handles
            .lock_registry
            .cancel_all_waiters_for_holder(&holder);
        let released = self.handles.lock_registry.release_all_for_holder(&holder);
        let out = vec![ok_u64((released + cancelled) as u64)];
        produce(&out, ack).await?;
        Ok(out)
    }
}

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
fn resolve_cmode(par: &Par) -> Option<ConsensusMode> {
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
fn resolve_lock_mode(par: &Par) -> Option<LockMode> {
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
fn holder_id_of(par: &Par) -> HolderId {
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
fn lock_err_reply(le: LockError) -> Par {
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

fn leaf_of(rel: &str) -> String {
    std::path::Path::new(rel)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| rel.to_string())
}

/// Fetch a `std::fs::Metadata` for the leaf named by `parent`.  Opens
/// the leaf via openat + O_NOFOLLOW off the parent dirfd, then reads
/// metadata.  A symlink leaf yields `ELOOP` — the caller decides how to
/// surface that.
fn fstatat_meta(parent: &SafeParent) -> std::io::Result<std::fs::Metadata> {
    use std::os::fd::FromRawFd;
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
fn target_dev_inode_at(parent: &SafeParent) -> Option<(u64, u64)> {
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
fn entry_stat_row(dir_fd: libc::c_int, name: &std::ffi::OsStr, mode: ConsensusMode) -> Par {
    use std::os::fd::FromRawFd;
    use std::os::unix::ffi::OsStrExt;
    let display = name.to_string_lossy().into_owned();
    let cname = match std::ffi::CString::new(name.as_bytes()) {
        Ok(c) => c,
        Err(_) => return error_record(&display, "invalid filename"),
    };
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
fn read_dir_capped(
    dir_fd: libc::c_int,
    max: usize,
) -> std::io::Result<(Vec<std::ffi::OsString>, bool)> {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
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
enum RemoveKind {
    File,
    Dir,
}

impl RemoveKind {
    fn as_wire(&self) -> &'static str {
        match self {
            RemoveKind::File => "file",
            RemoveKind::Dir => "dir",
        }
    }
}

/// H-29-3 lift slice 2 (2026-08-26) + R5(b) update (2026-09-02):
/// sorted post-order walk that collects (relative_path, kind)
/// tuples for a recursive Consensus removeDir.  Both leader and
/// follower re-execute this walk on their OWN per-validator
/// subdirs; the paths returned are relative to `target_root` so
/// leader and follower produce byte-identical manifests +
/// byte-identical WAL entries when their subdirs contain the
/// same tree (which they must under Shape A / D3 discipline).
///
/// The final entry represents `target_root` itself, encoded as
/// `PathBuf::new()` (empty relative path).  Callers apply it via
/// `canon_wal_target.join(rel)` which returns `canon_wal_target`
/// unchanged when `rel` is empty (`Path::join` semantics), so the
/// target dir's WAL entry carries the requested removeDir path
/// (bundle-relative under Shape A, absolute under identity
/// resolution).
///
/// Uses `std::fs::read_dir` because consensus trees reject
/// symlinks at boot; the lexical-sort requirement makes std::fs
/// the ergonomic choice over raw readdir.  If a symlink or
/// non-file/non-dir entry is encountered here, returns Unsupported
/// — same failure mode as boot-time validation.
///
/// Ordering: children first, then the containing directory itself
/// (post-order).  Sibling entries are sorted by `file_name()`.
fn collect_recursive_manifest(
    target_root: &std::path::Path,
) -> std::io::Result<Vec<(std::path::PathBuf, RemoveKind)>> {
    fn walk(
        dir: &std::path::Path,
        rel_base: &std::path::Path,
        out: &mut Vec<(std::path::PathBuf, RemoveKind)>,
    ) -> std::io::Result<()> {
        let mut entries: Vec<(std::ffi::OsString, std::path::PathBuf, std::fs::FileType)> =
            std::fs::read_dir(dir)?
                .map(|r| r.and_then(|e| e.file_type().map(|ft| (e.file_name(), e.path(), ft))))
                .collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, path, ft) in entries {
            let rel = rel_base.join(&name);
            if ft.is_dir() {
                walk(&path, &rel, out)?;
                out.push((rel, RemoveKind::Dir));
            } else if ft.is_file() {
                out.push((rel, RemoveKind::File));
            } else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    format!("unexpected filesystem entry kind at {path:?}"),
                ));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(target_root, std::path::Path::new(""), &mut out)?;
    // Final entry: target_root itself, represented by an empty
    // relative path.  Callers do canon_wal_target.join(rel) which
    // returns canon_wal_target unchanged for an empty rel.
    out.push((std::path::PathBuf::new(), RemoveKind::Dir));
    Ok(out)
}

/// Recursive symlink-safe rmdir.  Descends from `parent` into `leaf`
/// (must be a directory; ELOOP if symlink), unlinks every entry, then
/// removes the directory itself.
/// Recursive removeDir walker used by the Oracular recursive branch
/// of `fs_remove_dir`.  Post DD-RemoveDirReplyShape (2026-09-03),
/// returns the count of filesystem entries actually deleted:
/// `Ok(n)` on full success; `Err((n_before_error, io_error))` on
/// partial failure where `n_before_error` is the count of entries
/// successfully removed before the error terminated the walk.
///
/// The counter increments on every successful `unlinkat` — includes
/// files, subdirectories (via nested recursive call return), and
/// the final `AT_REMOVEDIR` for the target directory itself.
fn remove_dir_recursive(
    parent_fd: libc::c_int,
    leaf: *const libc::c_char,
) -> Result<u64, (u64, std::io::Error)> {
    let mut n_deleted: u64 = 0;
    unsafe {
        let dir_fd = libc::openat(
            parent_fd,
            leaf,
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        );
        if dir_fd < 0 {
            return Err((n_deleted, std::io::Error::last_os_error()));
        }
        // Dup dir_fd so we can readdir on one copy and use the other for
        // unlinkat.  L-3 fix (2026-08-06): F_DUPFD_CLOEXEC — see the
        // fs_entries site for rationale.
        let dup_fd = libc::fcntl(dir_fd, libc::F_DUPFD_CLOEXEC, 0);
        if dup_fd < 0 {
            let e = std::io::Error::last_os_error();
            libc::close(dir_fd);
            return Err((n_deleted, e));
        }
        let dir = libc::fdopendir(dup_fd);
        if dir.is_null() {
            let e = std::io::Error::last_os_error();
            libc::close(dir_fd);
            libc::close(dup_fd);
            return Err((n_deleted, e));
        }
        loop {
            errno_reset();
            let ent = libc::readdir(dir);
            if ent.is_null() {
                let e = std::io::Error::last_os_error();
                if e.raw_os_error() == Some(0) {
                    break;
                }
                libc::closedir(dir);
                libc::close(dir_fd);
                return Err((n_deleted, e));
            }
            let name_ptr = (*ent).d_name.as_ptr();
            let name_c = std::ffi::CStr::from_ptr(name_ptr);
            let name_bytes = name_c.to_bytes();
            if name_bytes == b"." || name_bytes == b".." {
                continue;
            }
            // Try file first; if it's a directory, recurse.
            let file_rc = libc::unlinkat(dir_fd, name_ptr, 0);
            if file_rc == 0 {
                n_deleted += 1;
                continue;
            }
            let e = std::io::Error::last_os_error();
            if e.raw_os_error() == Some(libc::EISDIR) || e.raw_os_error() == Some(libc::EPERM) {
                match remove_dir_recursive(dir_fd, name_ptr) {
                    Ok(inner_n) => {
                        n_deleted = n_deleted.saturating_add(inner_n);
                    }
                    Err((inner_n, inner_e)) => {
                        libc::closedir(dir);
                        libc::close(dir_fd);
                        return Err((n_deleted.saturating_add(inner_n), inner_e));
                    }
                }
                continue;
            }
            libc::closedir(dir);
            libc::close(dir_fd);
            return Err((n_deleted, e));
        }
        libc::closedir(dir);
        libc::close(dir_fd);
        // Finally remove the directory itself.
        if libc::unlinkat(parent_fd, leaf, libc::AT_REMOVEDIR) < 0 {
            return Err((n_deleted, std::io::Error::last_os_error()));
        }
        n_deleted += 1;
        Ok(n_deleted)
    }
}

async fn chown_impl(
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
    spawn_blocking(move || -> Par {
        let parent = match safe_descend_verified(&root_pb, &rel, expected_root_id) {
            Ok(p) => p,
            Err(qe) => {
                let (c, m) = quarantine_err_reply(&qe);
                return err(c, m);
            }
        };
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
    .unwrap_or_else(|_je| err(FSERR_IO, "spawn_blocking task failed"))
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
fn readdir_one_entry(dirp: *mut libc::DIR, cmode: ConsensusMode) -> Par {
    use std::os::unix::ffi::OsStringExt;
    loop {
        unsafe { errno_reset() };
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
        let name_ptr = unsafe { (*ent).d_name.as_ptr() };
        let name_c = unsafe { std::ffi::CStr::from_ptr(name_ptr) };
        let name_bytes = name_c.to_bytes();
        if name_bytes == b"." || name_bytes == b".." {
            continue;
        }
        // dirfd(3) returns the underlying fd for `openat`-based
        // per-entry stat — matches bulk fs_entries' pattern.
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
fn reply_is_ok(reply: &[Par]) -> bool {
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
    #[test]
    fn fs_lock_range_reads_current_deploy_scope() {
        let src = include_str!("handlers.rs");
        let fn_start = src
            .find("pub async fn fs_lock_range")
            .expect("handlers.rs missing fs_lock_range definition");
        // Window bumped 3KB → 6KB (slice 8b sub-2, 2026-08-12): the
        // wait:true parking + await + admit dispatch added ~2KB to
        // the fn body, pushing the current_deploy_scope read past
        // the old 3KB horizon.
        let window = &src[fn_start..std::cmp::min(fn_start + 6000, src.len())];
        assert!(
            window.contains("current_deploy_scope"),
            "step 5 regression: fs_lock_range must read scope from \
             self.handles.current_deploy_scope — the per-runtime cell \
             WalDeployScope publishes at deploy entry"
        );
        assert!(
            !window.contains("DeployScope::default()"),
            "step 5 regression: fs_lock_range must NOT fall back to \
             DeployScope::default() — that pre-step-5 placeholder path \
             was removed in step 5.  Under step-5 semantics, an acquire \
             outside a live WalDeployScope reads the sentinel [0; 32] \
             cell value; a release_all_for_deploy call using the default \
             would trip the sentinel-guard assert! in release_all_for_\
             deploy (commit 6f537099)."
        );
    }

    /// **Gap 2b**: pin fs_lock_sequential's scope-read.
    #[test]
    fn fs_lock_sequential_reads_current_deploy_scope() {
        let src = include_str!("handlers.rs");
        let fn_start = src
            .find("pub async fn fs_lock_sequential")
            .expect("handlers.rs missing fs_lock_sequential definition");
        // Window bumped 3KB → 5KB (slice 8b sub-2): wait:true
        // dispatch added ~1KB to the fn body.
        let window = &src[fn_start..std::cmp::min(fn_start + 5000, src.len())];
        assert!(
            window.contains("current_deploy_scope"),
            "step 5 regression: fs_lock_sequential must read scope from \
             self.handles.current_deploy_scope"
        );
        assert!(
            !window.contains("DeployScope::default()"),
            "step 5 regression: fs_lock_sequential must NOT fall back to \
             DeployScope::default() — see fs_lock_range test above for \
             rationale"
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
    #[test]
    fn fs_remove_file_has_step6_gate() {
        let src = include_str!("handlers.rs");
        let fn_start = src
            .find("pub async fn fs_remove_file")
            .expect("handlers.rs missing fs_remove_file definition");
        // 10KB window covers the extended step-6 body (grew slightly
        // after Gap 3 follow-up added count_locks wiring + updated
        // message).
        let window = &src[fn_start..std::cmp::min(fn_start + 10000, src.len())];
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
        let src = include_str!("handlers.rs");
        let fn_start = src
            .find("pub async fn fs_remove_dir")
            .expect("handlers.rs missing fs_remove_dir definition");
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
        // File.rho passes 8 args.  Window bumped to 8KB — the
        // wait:true parking path adds ~2KB to the fn body over the
        // pre-slice-8b baseline.
        let src = include_str!("handlers.rs");
        let fn_start = src
            .find("pub async fn fs_lock_range")
            .expect("handlers.rs missing fs_lock_range definition");
        let window = &src[fn_start..std::cmp::min(fn_start + 8000, src.len())];
        assert!(
            window.contains("[fd, off, len, mode, holder, cmode, wait_par, ack]"),
            "sub-2 regression: fs_lock_range must accept the 8-arg form \
             with `wait_par` at slot 7"
        );
        // Note: `RhoBoolean::unapply(\n    wait_par,\n)` under
        // rustfmt — search for the pieces separately.
        assert!(
            window.contains("RhoBoolean::unapply(") && window.contains("wait_par"),
            "sub-2 regression: fs_lock_range must parse wait as RhoBoolean"
        );
        assert!(
            window.contains("WaitPolicy::Wait") && window.contains("WaitPolicy::Fail"),
            "sub-2 regression: fs_lock_range must dispatch to \
             WaitPolicy based on wait: Bool"
        );
        assert!(
            window.contains("try_acquire_range_wait"),
            "sub-2 regression: fs_lock_range must use the wait-aware \
             LockRegistry method"
        );
        assert!(
            window.contains("AcquireOutcome::Parked") && window.contains("admit.await"),
            "sub-2 regression: fs_lock_range must await the Parked \
             admission oneshot"
        );
        assert!(
            window.contains("LockError::Cancelled"),
            "sub-2 regression: fs_lock_range must surface Cancelled \
             on oneshot RecvError (registry drop / no signal)"
        );
    }

    #[test]
    fn fs_lock_sequential_accepts_arity_5_with_wait_bool() {
        let src = include_str!("handlers.rs");
        let fn_start = src
            .find("pub async fn fs_lock_sequential")
            .expect("handlers.rs missing fs_lock_sequential definition");
        let window = &src[fn_start..std::cmp::min(fn_start + 5000, src.len())];
        assert!(
            window.contains("[fd, holder, cmode, wait_par, ack]"),
            "sub-2 regression: fs_lock_sequential must accept the 5-arg \
             form with `wait_par` at slot 4"
        );
        assert!(
            window.contains("RhoBoolean::unapply(") && window.contains("wait_par"),
            "sub-2 regression: fs_lock_sequential must parse wait as RhoBoolean"
        );
        assert!(
            window.contains("try_acquire_sequential_wait"),
            "sub-2 regression: fs_lock_sequential must use the wait-aware \
             LockRegistry method"
        );
        assert!(
            window.contains("AcquireOutcome::Parked") && window.contains("admit.await"),
            "sub-2 regression: fs_lock_sequential must await the Parked \
             admission oneshot"
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
        let src = include_str!("handlers.rs");
        let fn_start = src
            .find("pub async fn fs_release_all_for_holder")
            .expect("handlers.rs missing fs_release_all_for_holder definition");
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
