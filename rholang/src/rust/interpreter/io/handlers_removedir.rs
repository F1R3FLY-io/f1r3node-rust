// Trait-exempt `fs_remove_dir` handler + all its exclusive helpers.
//
// Wave-3 S3.13b split the 27 trait-registered handlers into per-
// family modules.  `fs_remove_dir` stayed in `handlers.rs`
// (trait-exempt per wave-3-plan.md § S3.11) because it has:
//
//   * Two structurally distinct dispatch modes (recursive vs
//     non-recursive) with divergent WAL shapes.
//   * An inline recursive-walk syscall loop that runs under a
//     `spawn_blocking` closure holding the FsProcesses' lock
//     registry clone.
//   * A reply shape carrying an `nDeleted` count field per
//     DD-RemoveDirReplyShape.
//
// These traits made the trait's uniform dispatch shape a poor fit.
//
// This module (X-6e A-07 Phase 2, 2026-09-12) extracts
// fs_remove_dir + its exclusive helpers out of the 4000-line
// `handlers.rs` god-module.  The `impl FsProcesses` block below is
// a "split impl" — Rust allows multiple `impl Foo { ... }` blocks
// for the same struct across different modules in the same crate.
//
// # What lives here
//
// * `impl FsProcesses { fs_remove_dir, finalize_failure_journal,
//   journal_path_mutation_single }` — the trait-exempt handler
//   method + its 2 exclusive helper methods.
// * `impl RemoveKind { as_wire }` — the wire-string projection
//   method (only fs_remove_dir needs it; RemoveKind itself lives
//   in handlers.rs because `unlink_leaf_via_dirfd` shares it).
// * ~15 free-fn helpers (`err_with_count`, `err_with_manifest`,
//   `ok_recursive_manifest`, `extract_removedir_n_deleted`, etc.)
//   — all previously module-private to handlers.rs, now module-
//   private to handlers_removedir.  Zero external callers.
// * 4 recursive-walk fns (`collect_recursive_manifest`,
//   `walk_and_unlink_recursive_with_journal`, `walk_dirfd_recursive`,
//   `remove_dir_recursive`).

use std::path::PathBuf;

use models::rhoapi::{ListParWithRandom, Par};

use super::super::errors::{illegal_argument_error, InterpreterError};
use super::super::rho_type::{RhoBoolean, RhoNumber, RhoString};
use super::errors::*;
use super::handlers::{
    ack_channel_hash, per_entry_ack_seed, resolve_cmode, spawn_blocking_par, target_dev_inode_at,
    unlink_leaf_via_dirfd, FsProcesses, RemoveKind, MAX_RECURSION_DEPTH,
};
use super::path::{
    canonicalize_lexical, io_msg_scrub, quarantine_err_reply, safe_descend_verified, SafeParent,
};
use super::response::*;
use super::verify::verify_reply_hash_matches_cached;
use super::wal::{WalEntry, WalOp, WalOutcome};
use super::{costs, ConsensusMode};

// M-9 fix (2026-08-06): read errno to distinguish clean EOF from
// error in readdir loops.  Same helper as handlers.rs; a local
// copy avoids adding a pub(super) helper just for this module.
#[cfg(target_os = "macos")]
unsafe fn errno_reset() { *libc::__error() = 0; }
#[cfg(target_os = "linux")]
unsafe fn errno_reset() { *libc::__errno_location() = 0; }

// ==================================================================
// Free-fn helpers (module-private).
// ==================================================================

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
pub(super) fn extract_removedir_n_deleted(reply: &Par) -> u64 {
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
    // SAFETY: `parent_fd` is a caller-owned open dirfd; `leaf` is
    // a caller-owned NUL-terminated CString ptr that outlives this
    // call.  Returns a fresh fd we manually close below on both
    // exit paths.
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
    let walk_result = walk_dirfd_recursive(target_fd, std::path::Path::new(""), &mut out, 0);
    // SAFETY: `target_fd` was returned by openat above and is no
    // longer used after this line (walk_dirfd_recursive dupped it
    // internally).  Closing it exactly once.
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

fn walk_dirfd_recursive(
    dir_fd: libc::c_int,
    rel_base: &std::path::Path,
    out: &mut Vec<(PathBuf, RemoveKind)>,
    depth: usize,
) -> std::io::Result<()> {
    if depth > MAX_RECURSION_DEPTH {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            format!(
                "SEC-Mi-03: directory nesting exceeds \
                 MAX_RECURSION_DEPTH = {MAX_RECURSION_DEPTH} \
                 (safety cap against stack exhaustion under \
                 adversarial Oracular-mode nesting).  See \
                 branch-review-2026-09-11.md Mi-03."
            ),
        ));
    }
    use std::os::unix::ffi::OsStrExt;

    // Dup the fd so fdopendir consumes the copy and dir_fd stays
    // usable for openat on subdirs.  F_DUPFD_CLOEXEC per the same
    // rationale as fs_entries (L-3 fix, 2026-08-06).
    // SAFETY: `dir_fd` is a caller-owned open dirfd (caller holds
    // it valid for this function's lifetime).  F_DUPFD_CLOEXEC
    // returns a fresh fd we own and manually close below.
    let dup_fd = unsafe { libc::fcntl(dir_fd, libc::F_DUPFD_CLOEXEC, 0) };
    if dup_fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `dup_fd` was just returned by fcntl above; on success
    // fdopendir takes ownership (dup_fd is closed by closedir).  On
    // failure (returned null) we manually close it below.
    let dir_ptr = unsafe { libc::fdopendir(dup_fd) };
    if dir_ptr.is_null() {
        let e = std::io::Error::last_os_error();
        // SAFETY: `dup_fd` is still owned by us (fdopendir failed
        // and did NOT take ownership); close it exactly once here.
        unsafe {
            libc::close(dup_fd);
        }
        return Err(e);
    }
    // Collect all entries; kind determined via fstatat below.
    let mut names: Vec<std::ffi::OsString> = Vec::new();
    loop {
        // SAFETY: `errno_reset` writes zero to the platform's
        // per-thread errno TLS slot.  Always safe.
        unsafe {
            errno_reset();
        }
        // SAFETY: `dir_ptr` came from fdopendir above and is live
        // for this loop; this function has exclusive access to it
        // (no other thread touches this DIR*).
        let ent = unsafe { libc::readdir(dir_ptr) };
        if ent.is_null() {
            let e = std::io::Error::last_os_error();
            if e.raw_os_error() == Some(0) {
                break;
            }
            // SAFETY: `dir_ptr` came from fdopendir above and is
            // still live at this point.  Closing it exactly once
            // on this error return path.
            unsafe {
                libc::closedir(dir_ptr);
            }
            return Err(e);
        }
        // SAFETY: `ent` is non-null (checked above); readdir(3)
        // guarantees `d_name` is a NUL-terminated in-struct array
        // owned by the DIR* buffer.  We copy the bytes below via
        // OsString::from_vec before the next readdir invalidates
        // the buffer.
        let name_ptr = unsafe { (*ent).d_name.as_ptr() };
        // SAFETY: `name_ptr` points to a NUL-terminated in-buffer
        // string in the DIR*'s dirent; `name_c` is used only for
        // `to_bytes()` on the same line and dropped before the next
        // readdir call.
        let name_c = unsafe { std::ffi::CStr::from_ptr(name_ptr) };
        let name_bytes = name_c.to_bytes();
        if name_bytes == b"." || name_bytes == b".." {
            continue;
        }
        names.push(std::ffi::OsStr::from_bytes(name_bytes).to_os_string());
    }
    // SAFETY: `dir_ptr` came from fdopendir above and is no longer
    // referenced after this close (readdir loop has finished).
    // Closes both `dir_ptr` and its owned dup fd.
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
        // SAFETY: `libc::stat` is a POD C struct with no niche
        // types; all-zero is a valid bit pattern to hand off to
        // fstatat, which will overwrite it before we read.
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: `dir_fd` is the caller-owned open dirfd;
        // `name_c.as_ptr()` is a NUL-terminated CString owned
        // locally.  `&mut stat` points at a live stack slot big
        // enough for `libc::stat`.  fstatat writes into `stat` and
        // doesn't retain either pointer.
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
            // SAFETY: `dir_fd` is the caller-owned open dirfd for
            // this function; `name_c.as_ptr()` points at a locally-
            // owned NUL-terminated CString that outlives this call.
            // openat returns a fresh fd we manually close on both
            // exit paths below.
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
            let walk_result = walk_dirfd_recursive(sub_fd, &rel, out, depth + 1);
            // SAFETY: `sub_fd` was returned by the openat above and
            // is no longer referenced after this close (walk_
            // dirfd_recursive returned).  Closing it exactly once.
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
    // SAFETY: `parent_fd` is a caller-owned open dirfd (see
    // callers: pinned via safe_descend or from the outer recursive
    // call).  `leaf` is a NUL-terminated CString ptr owned by the
    // caller and outlives this call.  All fds opened below are
    // manually closed on every return path within this block via
    // libc::close / libc::closedir.  readdir/unlinkat/dirfd/close/
    // closedir/fcntl/fdopendir/CStr::from_ptr sites all follow the
    // idiomatic pattern (see similar comment block in
    // walk_dirfd_recursive above).
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

// ==================================================================
// `impl RemoveKind` — `as_wire` wire-string projection.
// ==================================================================

impl RemoveKind {
    fn as_wire(&self) -> &'static str {
        match self {
            RemoveKind::File => "file",
            RemoveKind::Dir => "dir",
        }
    }
}

// ==================================================================
// `impl FsProcesses` — the trait-exempt `fs_remove_dir` method
// plus its 2 exclusive helper methods (finalize_failure_journal,
// journal_path_mutation_single).
// ==================================================================

impl FsProcesses {
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
                    // X-6c M-04 gated variant.
                    let raw_root_pb = PathBuf::from(root);
                    let (on_disk_root_pb, expected_root_id) = match self
                        .handles
                        .root_registry
                        .resolve_or_identity_gated_for_consensus(&raw_root_pb, cmode)
                    {
                        Ok(v) => v,
                        Err((c, m)) => {
                            let out = vec![early_err_for_remove_dir(*recursive, cmode, c, m)];
                            produce(&out, ack).await?;
                            return Ok(out);
                        }
                    };
                    let rel_owned = rel.to_string();
                    let lock_registry = self.handles.lock_registry.clone();
                    // T-20 (2026-09-11, DD-FailClosedOnInvariantBreak):
                    // migrated from `spawn_blocking_par_with_fallback`.
                    // The pre-T-20 fallback `err_with_count(FSERR_IO,
                    // ...)` is dead — a JoinError now aborts the deploy
                    // via `join_err_abort`, so no reply shape is
                    // returned on panic.
                    let fresh_reply = spawn_blocking_par(move || -> Par {
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
                    })
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
                    // X-6c M-04 gated variant.
                    let raw_root_pb = PathBuf::from(root);
                    let (on_disk_root_pb, expected_root_id) = match self
                        .handles
                        .root_registry
                        .resolve_or_identity_gated_for_consensus(&raw_root_pb, cmode)
                    {
                        Ok(v) => v,
                        Err((c, m)) => {
                            let out = vec![early_err_for_remove_dir(*recursive, cmode, c, m)];
                            produce(&out, ack).await?;
                            return Ok(out);
                        }
                    };
                    let canon_wal_target = canonicalize_lexical(root, rel);
                    let rel_owned = rel.to_string();
                    let lock_registry = self.handles.lock_registry.clone();
                    let wal_handle = self.handles.wal.clone();
                    let ack_clone = ack.clone();
                    // T-20 (2026-09-11, DD-FailClosedOnInvariantBreak):
                    // migrated from `spawn_blocking_par_with_fallback`.
                    // Panic aborts the deploy via `join_err_abort`; the
                    // pre-T-20 `err_with_manifest(FSERR_IO, ..., &[])`
                    // fallback is dead.
                    let fresh_reply = spawn_blocking_par(move || -> Par {
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
                    })
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
                // X-6c M-04 gated variant.
                let raw_root_pb = PathBuf::from(&root);
                let (on_disk_root_pb, expected_root_id) = match self
                    .handles
                    .root_registry
                    .resolve_or_identity_gated_for_consensus(&raw_root_pb, cmode)
                {
                    Ok(v) => v,
                    Err((c, m)) => {
                        let out = vec![early_err_for_remove_dir(recursive, cmode, c, m)];
                        produce(&out, ack).await?;
                        return Ok(out);
                    }
                };
                let canon_wal_target = canonicalize_lexical(&root, &rel);
                let lock_registry = self.handles.lock_registry.clone();
                let ack_clone = ack.clone();
                let wal = self.handles.wal.clone();
                // T-20 (2026-09-11, DD-FailClosedOnInvariantBreak):
                // migrated from `spawn_blocking_par_with_fallback`.
                // Panic aborts the deploy via `join_err_abort`; the
                // pre-T-20 `early_err_for_remove_dir(FSERR_IO, ...)`
                // fallback is dead.
                spawn_blocking_par(move || -> Par {
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
                                return err_with_count(FSERR_QUOTA_EXCEEDED, "WAL cap exceeded", 0);
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
                })
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
