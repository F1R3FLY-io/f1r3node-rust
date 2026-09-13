// Shared free-fn helpers for the fs handler families.
//
// # X-6f A-07 Phase 3 (2026-09-13) — handlers_helpers extraction
//
// Wave-3 S3.13b (2026-09-10) split the 27 trait-registered
// handlers into per-family modules (handlers_lifecycle.rs,
// handlers_lock.rs, handlers_mutation.rs, handlers_observation.rs,
// handlers_stream.rs).  The handlers.rs god-module retained ~30
// `pub(super)` shared helpers used by those family modules.
//
// This module (Phase 3 of A-07) extracts those shared helpers into
// a dedicated module.  Post-extraction, `handlers.rs` contains just
// the `FsProcesses` struct + constructor + cross-family primitives
// (spawn_blocking_par is now here too), plus test modules.
//
// # What lives here
//
// * Top-level primitives: `spawn_blocking_par`,
//   `consensus_divergence_reply`, `unlink_leaf_via_dirfd`,
//   `ack_channel_hash`, `per_entry_ack_seed`.
// * Consensus-observable constants:
//   `MAX_ENTRIES`, `MAX_WRITE_BYTES`, `MAX_RECURSION_DEPTH`
//   (all with `register_consensus_constant!` registrations that
//   fold into `CONSENSUS_FOLD`).
// * Enum: `RemoveKind` (shared by `unlink_leaf_via_dirfd` +
//   `fs_remove_dir` in handlers_removedir.rs).
// * Arg resolvers: `resolve_cmode`, `resolve_lock_mode`,
//   `holder_id_of`.
// * WAL journaling helpers: `journal_state_read_via_table`,
//   `journal_write_via_table`, `finalize_write_journal_via_table`,
//   `finalize_failure_journal_via_table`,
//   `journal_truncate_via_table`, `journal_path_mutation_two_via_table`,
//   `journal_path_mutation_single_via_table`,
//   `journal_read_via_table`, `journal_read_divergence_via_table`.
// * Impl helpers: `open_impl_via_table`, `read_impl_via_table`,
//   `write_impl_via_table`, `chown_impl`, `dev_inode_from_fd_via_table`.
// * Error/reply helpers: `lock_err_reply`, `reply_is_ok`,
//   `readdir_one_entry`.
// * Path/stat helpers: `leaf_of`, `fstatat_meta`,
//   `target_dev_inode_at`, `entry_stat_row`, `read_dir_capped`.
// * Test-only marker: `_use_access_mode`.
// * `errno_reset` cfg-gated variants (macOS + Linux + fallback).
//
// # Import discipline
//
// All function bodies moved verbatim; visibility unchanged
// (`pub(super)` throughout).  Sibling handler modules access them
// as `super::handlers_helpers::foo` (the existing
// `super::handlers::foo` paths still work because handlers.rs
// re-exports).

use std::path::PathBuf;

use models::rhoapi::Par;
use tokio::task::spawn_blocking;

use super::super::rho_type::{RhoBoolean, RhoString};
use super::errors::*;
use super::handle_table::{FileHandle, FileHandleTable};
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
