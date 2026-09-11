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

use std::path::PathBuf;

use models::rhoapi::{ListParWithRandom, Par};
use tokio::task::spawn_blocking;

use super::super::contract_call::ContractCall;
use super::super::dispatch::RhoDispatch;
use super::super::errors::{illegal_argument_error, InterpreterError};
use super::super::metering::MeteredMachine;
use super::super::rho_runtime::RhoISpace;
use super::super::rho_type::{RhoBoolean, RhoNumber, RhoString};
use super::errors::*;
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

/// RQ-2 (2026-09-04) centralized wrapper for the
/// `spawn_blocking(|| -> Par { ... }).await.unwrap_or_else(|_je|
/// err(FSERR_IO, "spawn_blocking task failed"))` pattern that
/// appears at 10+ handler sites.  The uniform mapping from a
/// panicked / cancelled blocking task to `FSERR_IO` is now in one
/// place; a future refactor of the fallback shape (e.g., adding
/// telemetry, distinguishing panic from cancellation) touches
/// exactly one function.
///
/// Only wraps the "closure returns Par directly" pattern.  Sites
/// where the closure returns `Result<T, E>` and the outer match
/// dispatches on both variants stay inline — the extraction would
/// force awkward generic-over-Result-arm parameterization for
/// negligible LOC savings.
///
/// T-17 (2026-09-08): callers whose JoinError fallback is a custom
/// Par (e.g., `err_with_count(FSERR_IO, ..., 0)` or an
/// `early_err_for_remove_dir(...)` per DD-RemoveDirReplyShape) use
/// `spawn_blocking_par_with_fallback` instead — same shape, custom
/// fallback closure.
// S3.13b (2026-09-10) — `pub(super)` for sibling family modules
// (`handlers_stream.rs` etc.) that share this spawn_blocking wrapper.
pub(super) async fn spawn_blocking_par<F>(f: F) -> Par
where F: FnOnce() -> Par + Send + 'static {
    spawn_blocking(f)
        .await
        .unwrap_or_else(|_je| err(FSERR_IO, "spawn_blocking task failed"))
}

/// T-17 (2026-09-08, Mi-3 fix): companion to `spawn_blocking_par`
/// for sites whose JoinError arm isn't the plain
/// `err(FSERR_IO, "spawn_blocking task failed")` — e.g., the
/// DD-RemoveDirReplyShape sites that need `err_with_count` or
/// `early_err_for_remove_dir` with the recursive/cmode-picked
/// reply shape.
///
/// The fallback closure runs synchronously on the awaiting task
/// (no spawn_blocking hop), so it's the right place for cheap
/// computations that use captured state (cmode / recursive /
/// ack context).
async fn spawn_blocking_par_with_fallback<F, G>(f: F, on_join_err: G) -> Par
where
    F: FnOnce() -> Par + Send + 'static,
    G: FnOnce() -> Par,
{
    match spawn_blocking(f).await {
        Ok(par) => par,
        Err(_je) => on_join_err(),
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

/// T-09 (Mi-1, 2026-09-08): converted from `unsafe fn` to safe fn
/// with narrow `unsafe { libc::* }` blocks at each FFI call.  Every
/// unsafe block has an inline SAFETY comment stating the caller-
/// side precondition and the FFI post-condition.  Caller supplies
/// a `SafeParent` obtained via `safe_descend_verified` and a
/// `rel_path` derived from a `collect_recursive_manifest` walk of
/// the target subtree.
fn unlink_manifest_entry(
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
        return unlink_leaf_via_dirfd(parent, kind);
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
    // SAFETY: `parent.as_raw_fd()` is an open dirfd; `parent.leaf_
    // ptr()` is a NUL-terminated CString owned by `parent`.  Flags
    // are libc constants.  `openat` returns a fresh fd on success
    // (>= 0) or -1 on error; we check the sign and wrap in
    // `OwnedFd` on success so Drop closes it on any exit path.
    let target_fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            parent.leaf_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if target_fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `target_fd` was just returned by `openat` above and
    // is a fresh, unshared fd; we transfer ownership to `OwnedFd`
    // which will close it on Drop.  No other reference to
    // `target_fd` exists in this scope.
    let mut cur_fd = unsafe { OwnedFd::from_raw_fd(target_fd) };
    // Descend through intermediate components.
    for intermediate in &components[..components.len() - 1] {
        let cname = std::ffi::CString::new(intermediate.as_bytes()).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "nul in path component")
        })?;
        // SAFETY: `cur_fd.as_raw_fd()` is an open dirfd owned by
        // this scope's `OwnedFd`.  `cname.as_ptr()` is a NUL-
        // terminated CString owned by this scope's `cname` and
        // outlives the call.  `openat` returns a fresh fd or -1;
        // we check and re-wrap in `OwnedFd`.
        let next_fd = unsafe {
            libc::openat(
                cur_fd.as_raw_fd(),
                cname.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if next_fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: `next_fd` was just returned by `openat` and is a
        // fresh, unshared fd; ownership transfers to the new
        // `OwnedFd`, replacing `cur_fd` which drops (closes the
        // previous dirfd).
        cur_fd = unsafe { OwnedFd::from_raw_fd(next_fd) };
    }
    // Final unlink of the leaf from the pinned parent dirfd.
    let leaf_name = components.last().expect("components non-empty");
    let leaf_c = std::ffi::CString::new(leaf_name.as_bytes())
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "nul in leaf"))?;
    // SAFETY: `cur_fd.as_raw_fd()` is the pinned final-parent
    // dirfd from the openat chain above; `leaf_c.as_ptr()` is a
    // NUL-terminated CString outliving the call.  `unlinkat`
    // removes the named entry from the dirfd.
    let rc = unsafe { libc::unlinkat(cur_fd.as_raw_fd(), leaf_c.as_ptr(), flags) };
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
fn extract_removedir_manifest(previous: &[Par]) -> Vec<(PathBuf, RemoveKind)> {
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
            Some(s) => PathBuf::from(s),
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
fn err_with_count(code: super::errors::FserrCode, msg: impl Into<String>, n_deleted: u64) -> Par {
    let items = vec![
        bool_par_false(),
        RhoString::create_par(code.as_str().to_string()),
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
fn ok_recursive_manifest(deleted: &[(PathBuf, RemoveKind)]) -> Par {
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
    code: super::errors::FserrCode,
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
    code: super::errors::FserrCode,
    msg: impl Into<String>,
    deleted: &[(PathBuf, RemoveKind)],
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
        RhoString::create_par(code.as_str().to_string()),
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
fn io_err_code_u32(e: &std::io::Error) -> u32 { fserr_to_code(io_err_code(e).as_str()) }

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

    /// RQ-2 (2026-09-04) centralized reader for the per-runtime
    /// current-deploy-scope cell.  Used at every handler that tags
    /// fd-table entries / DirHandle shadows / lock holders with the
    /// deploy scope for cross-cap coordination + deploy-end sweep.
    ///
    /// The scope is a `[u8; 32]` — set at deploy entry by
    /// `WalDeployScope::new_with_lock_sweep` (in the casper crate)
    /// and cleared back to `[0; 32]` at deploy end.  Reads are
    /// consistent within a single handler invocation because the
    /// scope cell is only written twice per deploy (entry + exit).
    ///
    /// Wraps the `RwLock::read().expect(...)` pattern in one place
    /// so a future refactor of the deploy-scope storage (e.g., an
    /// atomic swap, a per-thread cell) doesn't need to touch every
    /// handler.  Pre-RQ-2 this pattern was inlined at 6 sites, each
    /// with an identical `expect("current_deploy_scope RwLock
    /// poisoned")` message.
    #[cfg(any())]
    fn _deleted_pre_wave3_current_deploy_scope(&self) -> [u8; 32] {
        *self
            .handles
            .current_deploy_scope
            .read()
            .expect("current_deploy_scope RwLock poisoned")
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
    // Wave-3 S3.8 (2026-09-09): retired now that fs_write /
    // fs_write_at migrated to the FsHandler trait.  Free-fn form
    // `journal_write_via_table` is what the trait impls call via
    // `ctx.handles`.
    #[cfg(any())]
    async fn _deleted_pre_wave3_journal_write(
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
    // Wave-3 S3.6 (2026-09-09) — the `&self` wrappers
    // `journal_read`, `journal_read_divergence`, `read_impl` retired
    // now that fs_read / fs_read_at / fs_seek migrated to the
    // FsHandler trait.  Trait impls call the free-fn forms
    // (`journal_read_via_table`, `journal_read_divergence_via_table`,
    // `read_impl_via_table`) at the module bottom via `ctx.handles`.
    #[cfg(any())]
    async fn _deleted_pre_wave3_journal_read(
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
    #[cfg(any())]
    async fn _deleted_pre_wave3_journal_read_divergence(
        &self,
        fd: u64,
        offset: Option<u64>,
        ack: &Par,
    ) -> bool {
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
    #[cfg(any())]
    fn _deleted_pre_wave3_finalize_write_journal(
        &self,
        requested_bytes: &[u8],
        actual_n: u64,
        ack: &Par,
    ) {
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

    // Wave-3 S3.7 (2026-09-09): retired now that fs_truncate migrated
    // to the FsHandler trait.  Free-fn form `journal_truncate_via_
    // table` is what the trait impl calls via `ctx.handles`.
    #[cfg(any())]
    async fn _deleted_pre_wave3_journal_truncate(
        &self,
        fd: u64,
        n: u64,
        ack: &Par,
    ) -> Result<bool, ()> {
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
    #[cfg(any())]
    async fn _deleted_pre_wave3_journal_path_mutation_two(
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
    /// A no-op if `cmode != Consensus`.  fs_stat / fs_entries /
    /// fs_exists take cmode as an arg (fs_exists's cmode slot was
    /// added in the 2026-09-04 ban-lift slice, bumping arity 3 →
    /// 4); fs_size looks up the FileHandle's cmode via the fd.
    #[cfg(any())]
    fn _deleted_pre_wave3_journal_state_read(
        &self,
        cmode: ConsensusMode,
        op: WalOp,
        path: PathBuf,
        reply: &Par,
        ack: &Par,
        length: Option<u64>,
    ) {
        journal_state_read_via_table(&self.handles, cmode, op, path, reply, ack, length)
    }

    #[cfg(any())]
    async fn _deleted_pre_wave3_open_impl(
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
        let deploy = self.current_deploy_scope();
        let handle = FileHandle {
            // A4-M-1 fix (2026-09-04): wrap in Arc so `raw_fd()` can
            // hand out clones that outlive concurrent remove(fd).
            file: Some(std::sync::Arc::new(file)),
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
            // A-3 (2026-09-03): emit via `ok_fd(Fd::from(...))` to
            // enforce the fd-vs-quantity newtype invariant at the
            // emission boundary; wire format is byte-identical to
            // the pre-A-3 `ok_u64(fd)` shape.
            Ok(fd) => ok_fd(Fd::from(fd)),
            Err(()) => err(FSERR_QUOTA_EXCEEDED, "per-runtime fd cap reached"),
        }
    }

    #[cfg(any())]
    async fn _deleted_pre_wave3_read_impl(&self, fd: u64, n: u64, offset: Option<u64>) -> Par {
        read_impl_via_table(&self.handles, fd, n, offset).await
    }

    #[cfg(any())]
    async fn _deleted_pre_wave3_write_impl(
        &self,
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
        // A4-M-1 fix (2026-09-04): Arc<File> moved into the closure.
        let file_arc = match self.handles.raw_fd(fd).await {
            Some(f) => f,
            None => return err(FSERR_CLOSED, format!("unknown fd {fd}")),
        };
        // Redesign note: WAL journaling for Consensus caps happens in
        // `fs_write` / `fs_write_at` BEFORE this function is called,
        // so both leader and follower populate identical WALs
        // (C-29-F1 review fix).  Do NOT append here.
        let result = spawn_blocking(move || {
            use std::os::fd::AsRawFd;
            let raw_fd = file_arc.as_raw_fd();
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
    // removeDir — (rootCanon, rel, recursive: Bool, cmode) ->
    // DD-RemoveDirReplyShape (2026-09-03): every code path returns
    // `nDeleted` at position 1 (success) or position 3 (failure).
    //   Non-recursive: [true, 1] / [false, code, msg, 0]
    //   Recursive Oracular: [true, nDeleted] / [false, code, msg,
    //     nDeletedBeforeError]
    //   Recursive Consensus: [true, nDeleted, [[path, kind], ...]] /
    //     [false, code, msg, nDeletedBeforeError, [[path, kind], ...]]
    //     — manifest at position 2/4 is an implementation-side
    //     channel for R5(b) follower re-execution; `Dir.rho`'s
    //     `removeDir` method unwraps to the uniform shape at the
    //     Rholang boundary.
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
                    let fresh_reply = spawn_blocking_par_with_fallback(
                        move || -> Par {
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
                            match unlink_leaf_via_dirfd(&parent, RemoveKind::Dir) {
                                // DD-RemoveDirReplyShape: non-recursive success
                                // deletes exactly one entry (the target itself).
                                Ok(()) => ok_with_count(1),
                                // DD-RemoveDirReplyShape: non-recursive failure
                                // → 0 entries deleted before the error.
                                Err(e) => err_with_count(io_err_code(&e), io_msg_scrub(&e), 0),
                            }
                        },
                        || err_with_count(FSERR_IO, "spawn_blocking task failed", 0),
                    )
                    .await;
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
                    let fresh_reply = spawn_blocking_par_with_fallback(
                        move || -> Par {
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
                            // S-2 (2026-09-03): symlink-safe walker via
                            // parent-dirfd + O_NOFOLLOW; canon_target is no
                            // longer used to enumerate children.
                            // RQ-1 (2026-09-03): walk + per-entry journal +
                            // per-entry unlink loop extracted to
                            // `walk_and_unlink_recursive_with_journal`
                            // — identical code to the leader-branch call
                            // below (line ~4300).
                            walk_and_unlink_recursive_with_journal(
                                &parent,
                                &canon_wal_target,
                                &ack_clone,
                                &wal_handle,
                            )
                        },
                        // DD-RemoveDirReplyShape: spawn_blocking task
                        // failure on recursive Consensus path — 5-element
                        // with empty manifest.
                        || err_with_manifest(FSERR_IO, "spawn_blocking task failed", &[]),
                    )
                    .await;
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
            // Post DD-RemoveDirReplyShape (2026-09-03): per-entry
            // cost supplement (follower fall-through path).  Both
            // sides derive the count via `extract_removedir_n_deleted`
            // reading position 1 (success) or 3 (failure) of the
            // reply Par.  Uniform across all code paths — no more
            // branch-on-(recursive, cmode) logic needed.
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
                spawn_blocking_par_with_fallback(
                    move || -> Par {
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
                        // S-2 (2026-09-03): canon_target removed — the
                        // recursive Consensus walker now uses parent's dirfd
                        // + openat(O_NOFOLLOW) instead of a canonical path
                        // enumeration.
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
                                    return err_with_count(
                                        FSERR_QUOTA_EXCEEDED,
                                        "WAL cap exceeded",
                                        0,
                                    );
                                }
                            }
                            match unlink_leaf_via_dirfd(&parent, RemoveKind::Dir) {
                                // DD-RemoveDirReplyShape: non-recursive success
                                // deletes exactly one entry (the target itself).
                                Ok(()) => return ok_with_count(1),
                                Err(e) => {
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
                            }
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
                            //
                            // S-2 (2026-09-03): symlink-safe walker via
                            // parent-dirfd + O_NOFOLLOW; canon_target is no
                            // longer used to enumerate children.
                            // RQ-1 (2026-09-03): walk + per-entry journal +
                            // per-entry unlink loop extracted to
                            // `walk_and_unlink_recursive_with_journal` —
                            // identical code to the follower-branch call
                            // above (line ~4080).
                            walk_and_unlink_recursive_with_journal(
                                &parent,
                                &canon_wal_target,
                                &ack_clone,
                                &wal,
                            )
                        }
                    },
                    // DD-RemoveDirReplyShape: shape picker based on
                    // (recursive, cmode) so the reply-hash verify path
                    // stays symmetric with the follower.
                    || {
                        early_err_for_remove_dir(
                            recursive,
                            cmode,
                            FSERR_IO,
                            "spawn_blocking task failed",
                        )
                    },
                )
                .await
            }
            None => {
                // DD-RemoveDirReplyShape: parsed=None means args are
                // wrong-shape; recursive is unknown so pick the generic
                // 4-element count-carrying failure.
                err_with_count(FSERR_BAD_ARG, "expected (String, String, Bool, String)", 0)
            }
        };
        // Post DD-RemoveDirReplyShape (2026-09-03): per-entry cost
        // supplement is derived directly from the reply Par on every
        // code path (non-recursive, recursive Oracular, recursive
        // Consensus).  `extract_removedir_n_deleted` reads position 1
        // (success) or position 3 (failure) — uniform across all paths.
        // `reserve_incremental_primitive` tolerates n=0 without a
        // BugFoundError, so early quarantine / lock-held failures that
        // emit `nBefore=0` don't trip cost accounting.
        let supp_n = fs_remove_dir_supplement_count(&parsed, cmode, &reply);
        self.metering.reserve_incremental_primitive(
            costs::fs_remove_dir_per_entry_supplement_cost(supp_n),
        )?;
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }
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
                let sig = handles
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
    let result = spawn_blocking(move || {
        use std::os::fd::AsRawFd;
        let raw_fd = file_arc.as_raw_fd();
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
    let (root_pb, expected_root_id) = handles.root_registry.resolve_or_identity(&root_pb);
    let intent_copy = intent;
    let rel_for_open = rel.clone();
    let opened = spawn_blocking(move || {
        let (flags, mode_bits) = fopen_flags(intent_copy);
        super::path::safe_open_verified(&root_pb, &rel_for_open, flags, mode_bits, expected_root_id)
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
    // Non-regular-file rejection.  Since we already have the fd
    // (opened with O_NOFOLLOW), there's no TOCTOU here.
    let meta = match file.metadata() {
        Ok(m) => m,
        Err(e) => return err(io_err_code(&e), io_msg_scrub(&e)),
    };
    if !meta.file_type().is_file() {
        return err(FSERR_UNSUPPORTED, "not a regular file");
    }
    let deploy = *handles
        .current_deploy_scope
        .read()
        .expect("current_deploy_scope RwLock poisoned");
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
    let result = spawn_blocking(move || {
        use std::os::fd::AsRawFd;
        let raw_fd = file_arc.as_raw_fd();
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
/// TOCTOU-safe recursive-rmdir walker for the Consensus branch.
/// Descends from `parent_fd` into `leaf` (must be a directory;
/// ELOOP if symlink) via `openat(O_NOFOLLOW)`, walks children via
/// `fdopendir` + `fstatat(AT_SYMLINK_NOFOLLOW)`, and returns the
/// sorted post-order manifest of `(relative_path, kind)` tuples.
///
/// Symlink safety: post S-2 security review fix (2026-09-03) —
/// prior version used `std::fs::read_dir(absolute_path)` which
/// follows symlinks and let attacker-planted symlinks under a
/// Consensus root leak arbitrary directory names into the on-chain
/// WAL manifest + cause cross-validator divergence.  Now every
/// descent goes through `openat(..., O_NOFOLLOW)` and every kind-
/// probe uses `fstatat(..., AT_SYMLINK_NOFOLLOW)`.  Any non-file
/// / non-dir entry (symlink, fifo, socket, device) returns
/// `Unsupported` — same failure mode as boot-time validation.
///
/// Ordering: children first, then the containing directory itself
/// (post-order).  Sibling entries sorted by name for cross-validator
/// byte-identity of the manifest (readdir order is fs-specific).
///
/// Sibling: `remove_dir_recursive` below (used by the Oracular
/// branch) — same openat+fdopendir idiom.  The two walkers are not
/// merged because the Oracular walker inlines the unlink loop
/// while the Consensus walker returns a manifest that the caller
/// unlinks + journals per entry.
fn collect_recursive_manifest(
    parent_fd: libc::c_int,
    leaf: *const libc::c_char,
) -> std::io::Result<Vec<(PathBuf, RemoveKind)>> {
    // Open the target dir with O_NOFOLLOW so a symlinked `leaf`
    // fails ELOOP rather than escaping.
    let target_fd = unsafe {
        libc::openat(
            parent_fd,
            leaf,
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if target_fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut out = Vec::new();
    let walk_result = walk_dirfd_recursive(target_fd, std::path::Path::new(""), &mut out);
    unsafe {
        libc::close(target_fd);
    }
    walk_result?;
    // Final entry: target_root itself, represented by an empty
    // relative path.  Callers do canon_wal_target.join(rel) which
    // returns canon_wal_target unchanged for an empty rel.
    out.push((PathBuf::new(), RemoveKind::Dir));
    Ok(out)
}

/// RQ-1 (2026-09-03) extracted helper — the recursive-Consensus
/// removeDir walk + per-entry journal + per-entry unlink loop
/// that was duplicated between the leader and the follower
/// branches of `fs_remove_dir`.  Both call sites walk the same
/// manifest, journal the same WAL rows keyed on the same
/// per-entry ack hash, unlink via the same TOCTOU-immune dirfd
/// chain, and return the same reply Par shape.  The extraction
/// preserves byte-for-byte semantics on both sides (pinned by
/// `recursive_remove_dir_wal_is_byte_identical_on_leader_and_
/// follower` in `fs_wal_spec.rs`).
///
/// # Ordering guarantees preserved
///
/// - `collect_recursive_manifest` returns sorted post-order
///   (children before parent, alphabetical within a directory),
///   so the WAL journal emission order + reply manifest order
///   are deterministic and identical across validators walking
///   equivalent subdirs under Shape A.
/// - Each entry's WAL row is appended BEFORE its unlink fires
///   (append-first discipline).  A WAL-cap failure aborts the
///   walk BEFORE any further filesystem mutation.
/// - Per-entry ack seed is derived from the WAL path (bundle-
///   relative), so leader and follower produce identical hash
///   keys → identical Wal sidecar routing.
///
/// # Failure semantics
///
/// - Empty manifest impossible: `collect_recursive_manifest`
///   always emits at least the target-root sentinel entry.
/// - WAL-cap exhaustion returns
///   `err_with_manifest(FSERR_QUOTA_EXCEEDED, ..., &deleted)` —
///   `deleted` reflects the entries successfully removed BEFORE
///   the cap hit, matching the DD-RemoveDirReplyShape contract.
/// - Per-entry unlink failure updates the WAL row's outcome to
///   `Failure { code }` via `update_outcome_by_ack_hash` before
///   returning the truncated deleted list.
///
/// # Non-goals
///
/// - Does NOT do the outer safe_descend / lock-check /
///   spawn_blocking wrapping.  Callers pre-descend and pass the
///   pinned `SafeParent`.
/// - Does NOT compute the cost supplement.  Callers charge it
///   AFTER inspecting the returned reply.
fn walk_and_unlink_recursive_with_journal(
    parent: &SafeParent,
    canon_wal_target: &std::path::Path,
    ack_clone: &Par,
    wal_handle: &crate::rust::interpreter::io::wal::Wal,
) -> Par {
    let manifest = match collect_recursive_manifest(parent.as_raw_fd(), parent.leaf_ptr()) {
        Ok(m) => m,
        Err(e) => {
            return err_with_manifest(io_err_code(&e), io_msg_scrub(&e), &[]);
        }
    };
    let mut deleted: Vec<(PathBuf, RemoveKind)> = Vec::new();
    for (rel_path, kind) in manifest {
        // Empty rel_path marks the target root itself (final
        // post-order entry).  `Path::join("")` appends a trailing
        // separator whose serialized bytes would differ from the
        // un-joined target — special-case to preserve WAL byte-
        // for-byte identity (latent bug caught by 2026-09-02
        // security review: `PathBuf::eq` is component-wise and
        // hid the discrepancy, but `encode_wal_slice` byte
        // compare surfaces it).
        let wal_path = if rel_path.as_os_str().is_empty() {
            canon_wal_target.to_path_buf()
        } else {
            canon_wal_target.join(&rel_path)
        };
        let op = match kind {
            RemoveKind::File => WalOp::RemoveFile,
            RemoveKind::Dir => WalOp::RemoveDir,
        };
        // per_entry_ack seeded on WAL path (bundle-relative) so
        // leader + follower produce identical per-entry ack hashes.
        let per_entry_ack = per_entry_ack_seed(ack_clone, &wal_path);
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
        // TOCTOU-immune unlink via pinned dirfd chain from
        // `parent` (post-security-review S-1, 2026-09-02).
        let unlink_rc = unlink_manifest_entry(parent, &rel_path, kind);
        match unlink_rc {
            Ok(()) => {
                deleted.push((rel_path, kind));
            }
            Err(e) => {
                let code_u32 = io_err_code_u32(&e);
                let _ = wal_handle.update_outcome_by_ack_hash(per_entry_ack, WalOutcome::Failure {
                    code: code_u32,
                });
                return err_with_manifest(io_err_code(&e), io_msg_scrub(&e), &deleted);
            }
        }
    }
    ok_recursive_manifest(&deleted)
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
fn walk_dirfd_recursive(
    dir_fd: libc::c_int,
    rel_base: &std::path::Path,
    out: &mut Vec<(PathBuf, RemoveKind)>,
) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;

    // Dup the fd so fdopendir consumes the copy and dir_fd stays
    // usable for openat on subdirs.  F_DUPFD_CLOEXEC per the same
    // rationale as fs_entries (L-3 fix, 2026-08-06).
    let dup_fd = unsafe { libc::fcntl(dir_fd, libc::F_DUPFD_CLOEXEC, 0) };
    if dup_fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let dir_ptr = unsafe { libc::fdopendir(dup_fd) };
    if dir_ptr.is_null() {
        let e = std::io::Error::last_os_error();
        unsafe {
            libc::close(dup_fd);
        }
        return Err(e);
    }
    // Collect all entries; kind determined via fstatat below.
    let mut names: Vec<std::ffi::OsString> = Vec::new();
    loop {
        unsafe {
            errno_reset();
        }
        let ent = unsafe { libc::readdir(dir_ptr) };
        if ent.is_null() {
            let e = std::io::Error::last_os_error();
            if e.raw_os_error() == Some(0) {
                break;
            }
            unsafe {
                libc::closedir(dir_ptr);
            }
            return Err(e);
        }
        let name_ptr = unsafe { (*ent).d_name.as_ptr() };
        let name_c = unsafe { std::ffi::CStr::from_ptr(name_ptr) };
        let name_bytes = name_c.to_bytes();
        if name_bytes == b"." || name_bytes == b".." {
            continue;
        }
        names.push(std::ffi::OsStr::from_bytes(name_bytes).to_os_string());
    }
    unsafe {
        libc::closedir(dir_ptr);
    }
    // Sort by name for cross-validator byte-identity of the manifest.
    names.sort();
    for name in names {
        let rel = rel_base.join(&name);
        // fstatat with AT_SYMLINK_NOFOLLOW: rejects symlinks by
        // returning their symlink stat (S_IFLNK) rather than
        // following.
        let name_c = std::ffi::CString::new(name.as_bytes())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        let stat_rc = unsafe {
            libc::fstatat(
                dir_fd,
                name_c.as_ptr(),
                &mut stat as *mut _,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if stat_rc < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mode_kind = stat.st_mode & libc::S_IFMT;
        if mode_kind == libc::S_IFDIR {
            // Descend via openat(O_NOFOLLOW) — belt+suspenders
            // after the fstatat kind check (an attacker racing
            // between the two can't get us to follow a symlink).
            let sub_fd = unsafe {
                libc::openat(
                    dir_fd,
                    name_c.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            if sub_fd < 0 {
                return Err(std::io::Error::last_os_error());
            }
            let walk_result = walk_dirfd_recursive(sub_fd, &rel, out);
            unsafe {
                libc::close(sub_fd);
            }
            walk_result?;
            out.push((rel, RemoveKind::Dir));
        } else if mode_kind == libc::S_IFREG {
            out.push((rel, RemoveKind::File));
        } else {
            // Symlink (S_IFLNK), FIFO (S_IFIFO), socket (S_IFSOCK),
            // char device (S_IFCHR), block device (S_IFBLK).
            // Reject uniformly.  Consensus trees are supposed to be
            // symlink-free per boot validation; hitting one here
            // indicates either boot-validation drift or an
            // attacker-plant race, both worth failing loudly.
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                format!("unexpected filesystem entry kind (S_IFMT={:o})", mode_kind),
            ));
        }
    }
    Ok(())
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

    /// `spawn_blocking_par`'s panic-fallback must produce a Par
    /// byte-identical to `err(FSERR_IO, "spawn_blocking task failed")`.
    /// This was the string 10 handler sites used to hardcode inline;
    /// a future refactor that changed the message (e.g., adding a
    /// task-id suffix, translating the wording) would silently drift
    /// the consensus surface for any deploy that trips a blocking-
    /// task panic.
    #[tokio::test]
    async fn spawn_blocking_par_panic_fallback_matches_pre_wrapper_err() {
        let via_wrapper = spawn_blocking_par(|| -> Par { panic!("simulated task panic") }).await;
        let via_pre_wrapper = err(FSERR_IO, "spawn_blocking task failed");
        assert_eq!(
            via_wrapper, via_pre_wrapper,
            "RQ-2 wire drift: spawn_blocking_par's JoinError fallback MUST produce \
             `err(FSERR_IO, \"spawn_blocking task failed\")` byte-identical to the \
             10 pre-wrapper inline sites.  Any deploy that trips a blocking-task \
             panic would see a divergent reply Par → consensus divergence."
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

    /// T-17 review-fix (C1, 2026-09-08): companion to
    /// `spawn_blocking_par_panic_fallback_matches_pre_wrapper_err`
    /// covering the `_with_fallback` variant.  A regression that
    /// swapped the Ok/Err branches inside
    /// `spawn_blocking_par_with_fallback` would pass the happy-path
    /// pin below but produce the closure's Par instead of the
    /// fallback on JoinError — this pin catches that.
    ///
    /// Uses a distinct sentinel FSERR-string so a wire-format drift
    /// in the DD-RemoveDirReplyShape sites (which pass
    /// `err_with_count` / `err_with_manifest` fallbacks) surfaces
    /// via the same class of pin.
    #[tokio::test]
    async fn spawn_blocking_par_with_fallback_panic_calls_on_join_err() {
        // S4.9 (2026-09-11): test sentinel via explicit FserrCode
        // construction (see FSERR_TEST usage above).
        let via_wrapper = spawn_blocking_par_with_fallback(
            || -> Par { panic!("simulated task panic") },
            || err(FserrCode("FSERR_T17_TEST"), "fallback sentinel"),
        )
        .await;
        let expected = err(FserrCode("FSERR_T17_TEST"), "fallback sentinel");
        assert_eq!(
            via_wrapper, expected,
            "T-17 wire drift: spawn_blocking_par_with_fallback MUST invoke the \
             on_join_err closure and forward its Par on JoinError.  A regression \
             that swapped Ok/Err branches, dropped the fallback, or reshaped its \
             return would silently break the 3 DD-RemoveDirReplyShape migrated \
             call sites."
        );
    }

    /// T-17 review-fix (C1, 2026-09-08): happy-path companion.
    /// `spawn_blocking_par_with_fallback` MUST forward the
    /// closure's Par unchanged when the task completes normally
    /// (i.e., the fallback closure is NOT invoked).  Verified by
    /// using a sentinel fallback distinct from the happy return —
    /// a regression that always invoked the fallback would produce
    /// the wrong Par.
    #[tokio::test]
    async fn spawn_blocking_par_with_fallback_happy_path_forwards_closure_par() {
        // S4.9 (2026-09-11): test sentinels via explicit FserrCode
        // construction (see FSERR_TEST usage above).
        let payload = err(FserrCode("FSERR_T17_HAPPY"), "closure sentinel");
        let expected = payload.clone();
        let via_wrapper = spawn_blocking_par_with_fallback(
            move || payload,
            || {
                err(
                    FserrCode("FSERR_T17_FALLBACK"),
                    "wrong: fallback fired on happy path",
                )
            },
        )
        .await;
        assert_eq!(
            via_wrapper, expected,
            "T-17 wire drift: spawn_blocking_par_with_fallback MUST forward the \
             closure's Par unchanged on the Ok branch.  A regression that always \
             invoked the fallback would surface as the wrong FSERR string."
        );
    }

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
