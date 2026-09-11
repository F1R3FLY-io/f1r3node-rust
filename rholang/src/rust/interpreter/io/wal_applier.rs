// WAL fresh-tree applier (Phase 7b-2 item (c), 2026-08-28;
// hardened 2026-08-28 review pass; TOCTOU-hardened 2026-09-03 S-1).
//
// Reconstructs on-disk file state from a captured WAL slice + a
// hash → bytes payload sidecar.  Moved here from the fs_wal_spec.rs
// test module so the joiner-side sync driver (wal_payload_sync.rs)
// can invoke it once all pending payloads are resolved.
//
// # Callers
//
// **Production (Phase 7b-2 joiner):** applies the WAL to its own
// filesystem tree.  Each WAL entry's `path` is routed through
// `RootIdentityRegistry::resolve_wal_entry_root_rel`, which returns
// `(on_disk_root, rel_from_root, expected_root_id)` — the same
// tuple the leader-side handlers use.  The applier then descends
// via `safe_descend_verified` + `*at` syscalls, matching the
// handler-side TOCTOU discipline exactly.
//
// **Test (pb_m_14_*, wal_applier_skips_failure_outcome_entries):**
// uses separate leader/follower tempdirs for isolation and passes
// a translation closure that decomposes the leader WAL path
// (`leader_root/rel`) into `(follower_root, rel, None)` for
// safe-descent under the follower tree.  The `translate_path`
// helper stays in the test module — it's a test-harness artifact.
//
// # TOCTOU discipline (2026-09-03 S-1)
//
// Pre-S-1 the applier used absolute-path `std::fs::*` / `libc::chown`
// against the closure-derived path.  A local attacker (or a race
// with a legit local process) could rename/symlink-swap a path
// component between the closure evaluation and the syscall, causing
// the applier to write outside the on-disk root.  The `allowed_roots`
// check bounded the blast radius after the fact but did not close
// the race.
//
// Post-S-1 the applier follows the same discipline as the fs
// handlers: `safe_descend_verified(root, rel, expected_root_id)`
// yields a `SafeParent` (a dirfd + a leaf `CString`), and every
// mutation is a `*at` syscall against that dirfd.  Any intermediate
// component swap between descent and syscall fails cleanly at the
// `*at` boundary rather than escaping the root.
//
// # Path validation (defense-in-depth)
//
// The applier accepts an `allowed_roots: &[PathBuf]` argument.
// If non-empty, every WAL entry's path (and `extra_path` for
// Rename/CopyFile) must be under one of those roots — otherwise
// the applier returns `ApplierError::PathOutsideAllowedRoots`
// without touching disk.  Callers pass the joiner's configured
// `consensus-static-*` roots to bound the blast radius of a
// leader canonicalize bug or (theoretically) a Blake2b256
// forgery.  Pass `&[]` to skip validation (test callers, or
// production sites that haven't yet plumbed the provisioning
// config through — a documented gap the boot wire-in currently
// exercises).
//
// # Supported ops
//
//   * `Write` / `WriteAt` — carry absolute `offset` (position-
//     follow-up 2026-08-26) + `payload_ref: Hash(...)`; replayed
//     as seek-then-write against the sidecar bytes.
//   * `Truncate` — carries the new file length in `offset`.
//   * `Chmod` / `Chown` / `RemoveFile` / `RemoveDir` / `Rename` /
//     `CopyFile` — path-based mutations replayed directly (H-29-3
//     lift, 2026-08-26).
//   * Failure-outcome entries — skipped per H-6 (the leader never
//     mutated disk on Failure).
//   * Observation-only variants (`Read`, `ReadAt`, `Stat`,
//     `Entries`, `Size`, `EntriesStreamNext`) — no disk change.
//
// # Error handling (Result-based, 2026-08-28 hardening)
//
// Every failure path — missing sidecar entry, missing field on
// an entry, unsupported PayloadRef variant, byzantine NULL in
// path, out-of-allowed-roots path, NSS resolution failure, or
// syscall error — returns a specific `ApplierError` variant
// instead of panicking.  This lets the boot subscriber log +
// continue processing subsequent snapshots even when a single
// WAL slice trips a defensive check.  The panic-based signature
// of the pre-hardening version was a subscriber-killer: an
// applier panic unwound through `spawn_blocking` → `.expect(...)`
// → the async `while let` loop, taking the subscriber task
// with it.

use std::collections::HashMap;
use std::ffi::CString;
use std::path::{Path, PathBuf};

use super::path::{safe_descend_verified, QuarantineError, SafeParent};
use super::wal::{PayloadRef, WalEntry, WalOp, WalOutcome};

/// Result of decomposing a WAL entry's absolute path into the
/// `(on_disk_root, rel_from_root, expected_root_id)` triple the
/// applier hands to `safe_descend_verified`.
///
/// Production callers construct this via
/// `RootIdentityRegistry::resolve_wal_entry_root_rel`, which
/// consults the boot-populated registry for the on-disk root and
/// identity.  Test callers construct it directly from tempdir
/// roots + relative subpaths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedWalPath {
    pub root: PathBuf,
    pub rel: PathBuf,
    pub expected_root_id: Option<(u64, u64)>,
}

impl ResolvedWalPath {
    /// Convenience for tests: `<parent>/<file_name>` split with no
    /// identity check.  Callers that already know the tempdir root
    /// + relative filename should construct the struct directly
    /// (this fallback only works for depth-1 paths).
    pub fn identity_leaf_split(p: &Path) -> Self {
        ResolvedWalPath {
            root: p
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("/")),
            rel: p.file_name().map(PathBuf::from).unwrap_or_default(),
            expected_root_id: None,
        }
    }
}

/// Every failure mode the applier can surface.  Callers pattern-
/// match to distinguish "byzantine input" (log + skip) from
/// "internal invariant violation" (surface to operator + halt).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplierError {
    /// The WAL entry's `payload_ref` hash is not present in the
    /// sidecar.  In production this indicates the joiner-side
    /// fetch driver returned `is_complete()` but a race dropped
    /// bytes between there and here; in tests, the driver
    /// mis-populated the sidecar.
    MissingSidecarEntry {
        entry_index: usize,
        hash_hex: String,
    },
    /// The WAL entry carries `PayloadRef::DeployRef { ... }`
    /// which the applier cannot yet reconstruct locally.
    /// Reserved for a future Phase 7b-2 reducer slice that
    /// resolves deploy refs from on-chain deploy data.
    UnsupportedPayloadRef { entry_index: usize },
    /// The WAL entry is a write op but `payload_ref` is `None`.
    /// Invariant violation: the leader's `journal_write` must
    /// populate this field.
    MissingPayloadRef { entry_index: usize, op: WalOp },
    /// A Write/WriteAt/Truncate entry is missing its `offset`.
    /// Invariant violation post-position-follow-up (2026-08-26).
    MissingOffset { entry_index: usize, op: WalOp },
    /// A Chmod entry is missing `mode_bits`.
    MissingModeBits { entry_index: usize },
    /// A Chown entry is missing its `owner` field.
    MissingOwner { entry_index: usize },
    /// A Rename/CopyFile entry is missing `extra_path`.
    MissingExtraPath { entry_index: usize, op: WalOp },
    /// The WAL entry's path contains a NULL byte, which the
    /// `safe_descend_verified` layer catches at `to_c` before any
    /// syscall.  Post-S-1 this variant is retained for callers
    /// that may synthesize path-level pre-checks; the primary
    /// path routes NULL through `SafeDescendFailed`.
    PathContainsNull { entry_index: usize },
    /// The WAL entry's path is not under any of the caller-
    /// supplied `allowed_roots`.  Defense-in-depth: blocks a
    /// hypothetical leader bug (or forged snapshot) that would
    /// otherwise write outside the joiner's consensus-static
    /// roots.  Never reachable when `allowed_roots` is empty.
    PathOutsideAllowedRoots { entry_index: usize, path: PathBuf },
    /// `getpwnam_r` / `getgrnam_r` returned a non-zero errno
    /// (not ERANGE — that is retried internally with a bigger
    /// buffer).  Almost always indicates a system-level NSS
    /// problem rather than a WAL bug.
    NssResolutionFailed { name: String, errno: i32 },
    /// `getpwnam_r` / `getgrnam_r` returned success but the
    /// result pointer is NULL — i.e., the name resolved to no
    /// entry.  Operator responsibility to keep NSS consistent
    /// across validators.
    NssNotFound { name: String },
    /// A `std::fs` op or a libc syscall returned an error.
    IoFailure {
        entry_index: usize,
        op: WalOp,
        path: PathBuf,
        message: String,
    },
    /// Chown's `libc::fchownat` returned a non-zero rc that is
    /// not EPERM (EPERM is treated as a no-op success for
    /// unprivileged hosts — see the Chown branch's comment).
    ChownFailed {
        entry_index: usize,
        path: PathBuf,
        errno: i32,
    },
    /// `safe_descend_verified` failed for this entry's
    /// (on-disk-root, rel) — e.g., a symlink component was found,
    /// the root's boot-captured identity no longer matches, or the
    /// rel escaped the root.  Post-S-1 the applier surfaces
    /// descent failures explicitly rather than papering over them
    /// with a downstream open error.
    SafeDescendFailed {
        entry_index: usize,
        root: PathBuf,
        rel: PathBuf,
        reason: String,
    },
}

impl std::fmt::Display for ApplierError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApplierError::MissingSidecarEntry {
                entry_index,
                hash_hex,
            } => write!(
                f,
                "WAL entry {entry_index}: hash {hash_hex} missing from payload sidecar"
            ),
            ApplierError::UnsupportedPayloadRef { entry_index } => write!(
                f,
                "WAL entry {entry_index}: DeployRef payload_ref not yet supported"
            ),
            ApplierError::MissingPayloadRef { entry_index, op } => {
                write!(f, "WAL entry {entry_index}: {op:?} without payload_ref")
            }
            ApplierError::MissingOffset { entry_index, op } => {
                write!(f, "WAL entry {entry_index}: {op:?} without offset")
            }
            ApplierError::MissingModeBits { entry_index } => {
                write!(f, "WAL entry {entry_index}: Chmod without mode_bits")
            }
            ApplierError::MissingOwner { entry_index } => {
                write!(f, "WAL entry {entry_index}: Chown without owner")
            }
            ApplierError::MissingExtraPath { entry_index, op } => {
                write!(f, "WAL entry {entry_index}: {op:?} without extra_path")
            }
            ApplierError::PathContainsNull { entry_index } => {
                write!(f, "WAL entry {entry_index}: path contains a NULL byte")
            }
            ApplierError::PathOutsideAllowedRoots { entry_index, path } => write!(
                f,
                "WAL entry {entry_index}: path {path:?} is not under any allowed root"
            ),
            ApplierError::NssResolutionFailed { name, errno } => {
                write!(f, "NSS lookup for {name:?} failed with errno {errno}")
            }
            ApplierError::NssNotFound { name } => {
                write!(f, "NSS lookup for {name:?} returned no entry")
            }
            ApplierError::IoFailure {
                entry_index,
                op,
                path,
                message,
            } => write!(
                f,
                "WAL entry {entry_index}: {op:?} at {path:?} failed: {message}"
            ),
            ApplierError::ChownFailed {
                entry_index,
                path,
                errno,
            } => write!(
                f,
                "WAL entry {entry_index}: chown {path:?} failed with errno {errno}"
            ),
            ApplierError::SafeDescendFailed {
                entry_index,
                root,
                rel,
                reason,
            } => write!(
                f,
                "WAL entry {entry_index}: safe_descend {root:?} / {rel:?} failed: {reason}"
            ),
        }
    }
}

impl std::error::Error for ApplierError {}

/// Apply a captured WAL slice to a filesystem tree.
///
/// See module docstring for supported ops, path_map semantics,
/// path validation, and error variants.
///
/// `path_map` receives the WAL entry's `path` (or `extra_path`)
/// and returns a `ResolvedWalPath` — the `(on_disk_root,
/// rel_from_root, expected_root_id)` triple the applier hands to
/// `safe_descend_verified`.  Every mutation runs as a `*at`
/// syscall against the descended dirfd, so a component swap
/// between descent and syscall cannot escape the on-disk root
/// (S-1 TOCTOU discipline, 2026-09-03).
pub fn apply_wal_to_fresh_tree<F>(
    wal: &[WalEntry],
    payload_bytes: &HashMap<[u8; 32], Vec<u8>>,
    path_map: F,
    allowed_roots: &[PathBuf],
) -> Result<(), ApplierError>
where
    F: Fn(&Path) -> ResolvedWalPath,
{
    for (i, entry) in wal.iter().enumerate() {
        if matches!(entry.outcome, WalOutcome::Failure { .. }) {
            continue; // H-6: leader never mutated disk on Failure
        }
        match entry.op {
            WalOp::Write | WalOp::WriteAt => {
                let hash = match entry.payload_ref {
                    Some(PayloadRef::Hash(h)) => h,
                    Some(PayloadRef::DeployRef { .. }) => {
                        return Err(ApplierError::UnsupportedPayloadRef { entry_index: i })
                    }
                    None => {
                        return Err(ApplierError::MissingPayloadRef {
                            entry_index: i,
                            op: entry.op,
                        })
                    }
                };
                let bytes =
                    payload_bytes
                        .get(&hash)
                        .ok_or_else(|| ApplierError::MissingSidecarEntry {
                            entry_index: i,
                            hash_hex: hex::encode(hash),
                        })?;
                let off = entry.offset.ok_or(ApplierError::MissingOffset {
                    entry_index: i,
                    op: entry.op,
                })?;
                let (parent, dst) =
                    descend_entry(i, entry.op, &entry.path, &path_map, allowed_roots)?;
                let fd = openat_leaf(
                    i,
                    entry.op,
                    &parent,
                    &dst,
                    libc::O_WRONLY | libc::O_CREAT | libc::O_CLOEXEC,
                    0o644,
                )?;
                let write_res = pwrite_all(fd, bytes, off);
                // SAFETY: `fd` was returned by `openat_leaf` above and
                // has not been closed elsewhere; we hold sole ownership
                // and close it exactly once here.  `close` does not
                // retain the fd.
                unsafe { libc::close(fd) };
                write_res.map_err(|e| ApplierError::IoFailure {
                    entry_index: i,
                    op: entry.op,
                    path: dst,
                    message: format!("pwrite: {e}"),
                })?;
            }
            WalOp::Truncate => {
                let n = entry.offset.ok_or(ApplierError::MissingOffset {
                    entry_index: i,
                    op: entry.op,
                })?;
                let (parent, dst) =
                    descend_entry(i, entry.op, &entry.path, &path_map, allowed_roots)?;
                let fd = openat_leaf(
                    i,
                    entry.op,
                    &parent,
                    &dst,
                    libc::O_WRONLY | libc::O_CLOEXEC,
                    0,
                )?;
                // SAFETY: `fd` is a fresh writable fd just returned by
                // `openat_leaf`; `ftruncate` operates purely on the
                // kernel-side file object and does not touch userspace
                // memory.
                let rc = unsafe { libc::ftruncate(fd, n as libc::off_t) };
                let ftrunc_err = if rc < 0 {
                    Some(std::io::Error::last_os_error())
                } else {
                    None
                };
                // SAFETY: `fd` was returned by `openat_leaf` above and
                // has not been closed elsewhere; we hold sole ownership
                // and close it exactly once here.
                unsafe { libc::close(fd) };
                if let Some(e) = ftrunc_err {
                    return Err(ApplierError::IoFailure {
                        entry_index: i,
                        op: entry.op,
                        path: dst,
                        message: format!("ftruncate: {e}"),
                    });
                }
            }
            // Observation-only — nothing to reconstruct on disk.
            WalOp::Read
            | WalOp::ReadAt
            | WalOp::Stat
            | WalOp::Entries
            | WalOp::Size
            | WalOp::EntriesStreamNext
            | WalOp::Exists => {}
            WalOp::Chmod => {
                let bits = entry
                    .mode_bits
                    .ok_or(ApplierError::MissingModeBits { entry_index: i })?;
                let (parent, dst) =
                    descend_entry(i, entry.op, &entry.path, &path_map, allowed_roots)?;
                // SAFETY: `parent` owns an open dirfd and a NUL-
                // terminated `CString` leaf for its lifetime, so
                // `as_raw_fd()` and `leaf_ptr()` are valid for the
                // duration of this call.  `fchmodat` only reads the
                // leaf name pointer and does not retain it.
                let rc = unsafe {
                    libc::fchmodat(
                        parent.as_raw_fd(),
                        parent.leaf_ptr(),
                        bits as libc::mode_t,
                        0,
                    )
                };
                if rc != 0 {
                    let e = std::io::Error::last_os_error();
                    return Err(ApplierError::IoFailure {
                        entry_index: i,
                        op: entry.op,
                        path: dst,
                        message: format!("fchmodat: {e}"),
                    });
                }
            }
            WalOp::Chown => {
                let owner = entry
                    .owner
                    .as_ref()
                    .ok_or(ApplierError::MissingOwner { entry_index: i })?;
                let group = entry.group.as_deref();
                let uid = if owner.is_empty() {
                    u32::MAX
                } else {
                    resolve_uid(owner)?
                };
                let gid = match group {
                    None | Some("") => u32::MAX,
                    Some(g) => resolve_gid(g)?,
                };
                let (parent, dst) =
                    descend_entry(i, entry.op, &entry.path, &path_map, allowed_roots)?;
                // SAFETY: `parent` owns an open dirfd and a NUL-
                // terminated `CString` leaf for its lifetime, so
                // `as_raw_fd()` and `leaf_ptr()` are valid here.
                // `fchownat` only reads the leaf name pointer and does
                // not retain it; `AT_SYMLINK_NOFOLLOW` matches leader
                // discipline (never traverse a symlink leaf).
                let rc = unsafe {
                    libc::fchownat(
                        parent.as_raw_fd(),
                        parent.leaf_ptr(),
                        uid,
                        gid,
                        libc::AT_SYMLINK_NOFOLLOW,
                    )
                };
                if rc != 0 {
                    let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
                    // Unprivileged hosts (typical CI) can't chown
                    // to arbitrary owners.  EPERM is treated as a
                    // no-op success — tests should use current-
                    // user names to avoid this; production
                    // joiners run as the node service user and
                    // MUST have the perms to replay the leader's
                    // chown ops.
                    if errno != libc::EPERM {
                        return Err(ApplierError::ChownFailed {
                            entry_index: i,
                            path: dst,
                            errno,
                        });
                    }
                }
            }
            WalOp::RemoveFile => {
                let (parent, dst) =
                    descend_entry(i, entry.op, &entry.path, &path_map, allowed_roots)?;
                // SAFETY: `parent` owns an open dirfd and a NUL-
                // terminated `CString` leaf for its lifetime, so
                // `as_raw_fd()` and `leaf_ptr()` are valid here.
                // `unlinkat` only reads the leaf name pointer.
                let rc = unsafe { libc::unlinkat(parent.as_raw_fd(), parent.leaf_ptr(), 0) };
                if rc != 0 {
                    let e = std::io::Error::last_os_error();
                    return Err(ApplierError::IoFailure {
                        entry_index: i,
                        op: entry.op,
                        path: dst,
                        message: format!("unlinkat: {e}"),
                    });
                }
            }
            WalOp::RemoveDir => {
                let (parent, dst) =
                    descend_entry(i, entry.op, &entry.path, &path_map, allowed_roots)?;
                // SAFETY: `parent` owns an open dirfd and a NUL-
                // terminated `CString` leaf for its lifetime, so
                // `as_raw_fd()` and `leaf_ptr()` are valid here.
                // `unlinkat(AT_REMOVEDIR)` only reads the leaf name
                // pointer.
                let rc = unsafe {
                    libc::unlinkat(parent.as_raw_fd(), parent.leaf_ptr(), libc::AT_REMOVEDIR)
                };
                if rc != 0 {
                    let e = std::io::Error::last_os_error();
                    return Err(ApplierError::IoFailure {
                        entry_index: i,
                        op: entry.op,
                        path: dst,
                        message: format!("unlinkat(AT_REMOVEDIR): {e}"),
                    });
                }
            }
            WalOp::Rename => {
                let extra = entry
                    .extra_path
                    .as_ref()
                    .ok_or(ApplierError::MissingExtraPath {
                        entry_index: i,
                        op: entry.op,
                    })?;
                let (from_parent, from_dst) =
                    descend_entry(i, entry.op, &entry.path, &path_map, allowed_roots)?;
                let (to_parent, to_dst) =
                    descend_entry(i, entry.op, extra, &path_map, allowed_roots)?;
                // SAFETY: both `from_parent` and `to_parent` own open
                // dirfds and NUL-terminated `CString` leaves for their
                // lifetimes, so all four accessors are valid here.
                // `renameat` only reads the leaf name pointers and
                // does not retain them.
                let rc = unsafe {
                    libc::renameat(
                        from_parent.as_raw_fd(),
                        from_parent.leaf_ptr(),
                        to_parent.as_raw_fd(),
                        to_parent.leaf_ptr(),
                    )
                };
                if rc != 0 {
                    let e = std::io::Error::last_os_error();
                    return Err(ApplierError::IoFailure {
                        entry_index: i,
                        op: entry.op,
                        path: from_dst,
                        message: format!("renameat → {to_dst:?}: {e}"),
                    });
                }
            }
            WalOp::CopyFile => {
                let extra = entry
                    .extra_path
                    .as_ref()
                    .ok_or(ApplierError::MissingExtraPath {
                        entry_index: i,
                        op: entry.op,
                    })?;
                let (from_parent, from_dst) =
                    descend_entry(i, entry.op, &entry.path, &path_map, allowed_roots)?;
                let (to_parent, to_dst) =
                    descend_entry(i, entry.op, extra, &path_map, allowed_roots)?;
                copy_at(&from_parent, &to_parent).map_err(|e| ApplierError::IoFailure {
                    entry_index: i,
                    op: entry.op,
                    path: from_dst.clone(),
                    message: format!("copy → {to_dst:?}: {e}"),
                })?;
            }
        }
    }
    Ok(())
}

/// Common prologue for every op: check `allowed_roots` on the raw
/// WAL entry path (bundle-relative under Shape A), run the closure
/// to resolve to the on-disk absolute, then `safe_descend_verified`
/// to obtain the SafeParent dirfd.  Returns the descended parent +
/// the joined on-disk path for error messages.
///
/// Ordering rationale (S4.4, 2026-09-10): WAL entries under Shape A
/// carry bundle-relative paths (e.g. `/@bundle/target`), and node
/// setup registers `BUNDLE_ROOT_PREFIX` (`/@bundle`) as a
/// consensus-static root plus the operator's absolute per-validator
/// paths.  Checking the RAW `entry_path` against `allowed_roots`
/// matches on the bundle-relative prefix directly, without depending
/// on how the registry resolves it to a per-validator on-disk
/// subdir.  Checking the resolved on-disk root would require the
/// operator to register per-validator absolute paths whose exact
/// lexical shape matches the registry's output — brittle across
/// canonicalization variants.  See node::runtime::setup where
/// `runtime_manager.register_consensus_static_root(BUNDLE_ROOT_
/// PREFIX)` documents this contract explicitly.
fn descend_entry<F>(
    entry_index: usize,
    op: WalOp,
    entry_path: &Path,
    path_map: &F,
    allowed_roots: &[PathBuf],
) -> Result<(SafeParent, PathBuf), ApplierError>
where
    F: Fn(&Path) -> ResolvedWalPath,
{
    if !allowed_roots.is_empty() {
        check_path_allowed(entry_index, entry_path, allowed_roots)?;
    }
    let resolved = path_map(entry_path);
    let rel_str = resolved.rel.to_string_lossy().into_owned();
    let dst = resolved.root.join(&resolved.rel);
    let parent = safe_descend_verified(&resolved.root, &rel_str, resolved.expected_root_id)
        .map_err(|qe| ApplierError::SafeDescendFailed {
            entry_index,
            root: resolved.root.clone(),
            rel: resolved.rel.clone(),
            reason: format_quarantine(&qe),
        })?;
    // Silence "unused op" warning on paths that skip IoFailure
    // wrapping — retained for future error variants that carry op.
    let _ = op;
    Ok((parent, dst))
}

fn format_quarantine(qe: &QuarantineError) -> String {
    match qe {
        QuarantineError::Empty => "empty rel".to_string(),
        QuarantineError::RootSelf => "rel resolves to root itself".to_string(),
        QuarantineError::EscapesRoot => "rel escapes root".to_string(),
        QuarantineError::SymlinkComponent => "symlink component in path".to_string(),
        QuarantineError::RootIdentityChanged => "root identity changed post-boot".to_string(),
        QuarantineError::IoError(kind, msg) => format!("{kind:?}: {msg}"),
    }
}

fn openat_leaf(
    entry_index: usize,
    op: WalOp,
    parent: &SafeParent,
    dst: &Path,
    flags: libc::c_int,
    mode: libc::mode_t,
) -> Result<libc::c_int, ApplierError> {
    // SAFETY: `parent` owns an open dirfd and a NUL-terminated
    // `CString` leaf for its lifetime, so `as_raw_fd()` and
    // `leaf_ptr()` are valid across the call.  `openat` only reads
    // the leaf name pointer; `O_NOFOLLOW` preserves the S-1 TOCTOU
    // discipline (a leaf-level symlink is rejected here rather than
    // followed).
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            parent.leaf_ptr(),
            flags | libc::O_NOFOLLOW,
            mode as libc::c_uint,
        )
    };
    if fd < 0 {
        let e = std::io::Error::last_os_error();
        return Err(ApplierError::IoFailure {
            entry_index,
            op,
            path: dst.to_path_buf(),
            message: format!("openat: {e}"),
        });
    }
    Ok(fd)
}

fn pwrite_all(fd: libc::c_int, bytes: &[u8], off: u64) -> std::io::Result<()> {
    let mut written: usize = 0;
    while written < bytes.len() {
        // SAFETY: `fd` is a caller-owned open fd valid for the entire
        // call.  `bytes.as_ptr().add(written)` is in-bounds for the
        // slice because the loop guard ensures `written < bytes.len()`,
        // and the length passed is `bytes.len() - written`, so the
        // whole read range lies within the slice.  `pwrite` only
        // reads the buffer; it does not retain the pointer.
        let n = unsafe {
            libc::pwrite(
                fd,
                bytes.as_ptr().add(written) as *const _,
                bytes.len() - written,
                (off + written as u64) as libc::off_t,
            )
        };
        if n < 0 {
            let e = std::io::Error::last_os_error();
            if e.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(e);
        }
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WriteZero,
                "pwrite returned 0",
            ));
        }
        written += n as usize;
    }
    Ok(())
}

/// Portable file-to-file copy via two openat'd fds + a read/write
/// loop.  Avoids `libc::sendfile` / `copy_file_range` for macOS
/// compatibility (per FIP scope).  The destination is created
/// with 0o644 and truncated — matches `std::fs::copy` semantics
/// closely enough for WAL replay (leader's Chmod entries adjust
/// perms after the fact).
fn copy_at(from_parent: &SafeParent, to_parent: &SafeParent) -> std::io::Result<()> {
    // SAFETY: `from_parent` owns an open dirfd and a NUL-terminated
    // `CString` leaf for its lifetime, so `as_raw_fd()` and
    // `leaf_ptr()` are valid across the call.  `openat` only reads
    // the leaf name pointer; `O_NOFOLLOW` refuses a symlink leaf,
    // preserving S-1 TOCTOU discipline.
    let from_fd = unsafe {
        libc::openat(
            from_parent.as_raw_fd(),
            from_parent.leaf_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0,
        )
    };
    if from_fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    struct FdGuard(libc::c_int);
    impl Drop for FdGuard {
        fn drop(&mut self) {
            // SAFETY: type invariant — `self.0` is a valid open fd
            // owned by this guard (constructed only from a fresh
            // `openat` result that returned >= 0), closed exactly
            // once here in `drop`.
            unsafe { libc::close(self.0) };
        }
    }
    let _from_guard = FdGuard(from_fd);
    // SAFETY: `to_parent` owns an open dirfd and a NUL-terminated
    // `CString` leaf for its lifetime, so `as_raw_fd()` and
    // `leaf_ptr()` are valid across the call.  `openat` only reads
    // the leaf name pointer; `O_NOFOLLOW` refuses a symlink leaf,
    // preserving S-1 TOCTOU discipline.
    let to_fd = unsafe {
        libc::openat(
            to_parent.as_raw_fd(),
            to_parent.leaf_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o644,
        )
    };
    if to_fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let _to_guard = FdGuard(to_fd);

    let mut buf = [0u8; 64 * 1024];
    loop {
        // SAFETY: `from_fd` is kept open by `_from_guard` for the
        // full scope of this loop.  `buf` is a live stack array;
        // `buf.as_mut_ptr()` is valid for writes of `buf.len()`
        // bytes.  `read` writes at most `buf.len()` bytes and does
        // not retain the pointer.
        let n = unsafe { libc::read(from_fd, buf.as_mut_ptr() as *mut _, buf.len()) };
        if n < 0 {
            let e = std::io::Error::last_os_error();
            if e.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(e);
        }
        if n == 0 {
            return Ok(());
        }
        let mut written = 0usize;
        while written < n as usize {
            // SAFETY: `to_fd` is kept open by `_to_guard` for the
            // full scope of this loop.  `n` is >= 0 (the `n < 0`
            // and `n == 0` cases returned above) and n <= buf.len()
            // because `read` reports how many bytes it wrote into
            // buf.  The loop guard ensures `written < n`, so
            // `buf.as_ptr().add(written)` is in bounds and the
            // read range `n - written` also lies within the slice.
            // `write` only reads the buffer; it does not retain the
            // pointer.
            let w = unsafe {
                libc::write(
                    to_fd,
                    buf.as_ptr().add(written) as *const _,
                    n as usize - written,
                )
            };
            if w < 0 {
                let e = std::io::Error::last_os_error();
                if e.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(e);
            }
            if w == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "write returned 0",
                ));
            }
            written += w as usize;
        }
    }
}

fn check_path_allowed(
    entry_index: usize,
    path: &Path,
    allowed_roots: &[PathBuf],
) -> Result<(), ApplierError> {
    if allowed_roots.iter().any(|root| path.starts_with(root)) {
        Ok(())
    } else {
        Err(ApplierError::PathOutsideAllowedRoots {
            entry_index,
            path: path.to_path_buf(),
        })
    }
}

/// Thread-safe `getpwnam` — uses `getpwnam_r` under the hood so
/// concurrent applier invocations (or concurrent NSS lookups
/// elsewhere in the process) don't corrupt each other's `passwd`
/// pointers.  Grows the caller-provided buffer on ERANGE up to a
/// reasonable ceiling (16 MiB) so long entries still succeed.
fn resolve_uid(name: &str) -> Result<u32, ApplierError> {
    let cname = CString::new(name.as_bytes()).map_err(|_| ApplierError::NssNotFound {
        name: name.to_string(),
    })?;
    let mut buf_len: usize = 1024;
    let ceiling: usize = 16 * 1024 * 1024;
    loop {
        let mut buf: Vec<libc::c_char> = vec![0; buf_len];
        // SAFETY: `libc::passwd` is a POD C struct — every field is
        // an integer or raw pointer — so an all-zero bit pattern is
        // a valid representation.  We only read fields after
        // `getpwnam_r` returns success and `result_ptr` is non-null.
        let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
        let mut result_ptr: *mut libc::passwd = std::ptr::null_mut();
        // SAFETY: `cname` is a NUL-terminated `CString` alive for the
        // whole call (the local binding above outlives this scope).
        // `&mut pwd` is a valid unique pointer to a live `passwd`.
        // `buf.as_mut_ptr()` is valid for writes of `buf.len()` bytes.
        // `&mut result_ptr` points to a live `*mut passwd` local.
        // `getpwnam_r` populates `pwd` and `result_ptr`; it does not
        // retain any of these pointers past the call.
        let rc = unsafe {
            libc::getpwnam_r(
                cname.as_ptr(),
                &mut pwd,
                buf.as_mut_ptr(),
                buf.len(),
                &mut result_ptr,
            )
        };
        if rc == libc::ERANGE {
            if buf_len >= ceiling {
                return Err(ApplierError::NssResolutionFailed {
                    name: name.to_string(),
                    errno: rc,
                });
            }
            buf_len = (buf_len * 2).min(ceiling);
            continue;
        }
        if rc != 0 {
            return Err(ApplierError::NssResolutionFailed {
                name: name.to_string(),
                errno: rc,
            });
        }
        if result_ptr.is_null() {
            return Err(ApplierError::NssNotFound {
                name: name.to_string(),
            });
        }
        return Ok(pwd.pw_uid);
    }
}

/// Thread-safe `getgrnam` companion — same shape as `resolve_uid`.
fn resolve_gid(name: &str) -> Result<u32, ApplierError> {
    let cname = CString::new(name.as_bytes()).map_err(|_| ApplierError::NssNotFound {
        name: name.to_string(),
    })?;
    let mut buf_len: usize = 1024;
    let ceiling: usize = 16 * 1024 * 1024;
    loop {
        let mut buf: Vec<libc::c_char> = vec![0; buf_len];
        // SAFETY: `libc::group` is a POD C struct — every field is
        // an integer or raw pointer — so an all-zero bit pattern is
        // a valid representation.  We only read fields after
        // `getgrnam_r` returns success and `result_ptr` is non-null.
        let mut grp: libc::group = unsafe { std::mem::zeroed() };
        let mut result_ptr: *mut libc::group = std::ptr::null_mut();
        // SAFETY: `cname` is a NUL-terminated `CString` alive for the
        // whole call.  `&mut grp` is a valid unique pointer to a live
        // `group`.  `buf.as_mut_ptr()` is valid for writes of
        // `buf.len()` bytes.  `&mut result_ptr` points to a live
        // `*mut group` local.  `getgrnam_r` populates `grp` and
        // `result_ptr`; it does not retain any of these pointers.
        let rc = unsafe {
            libc::getgrnam_r(
                cname.as_ptr(),
                &mut grp,
                buf.as_mut_ptr(),
                buf.len(),
                &mut result_ptr,
            )
        };
        if rc == libc::ERANGE {
            if buf_len >= ceiling {
                return Err(ApplierError::NssResolutionFailed {
                    name: name.to_string(),
                    errno: rc,
                });
            }
            buf_len = (buf_len * 2).min(ceiling);
            continue;
        }
        if rc != 0 {
            return Err(ApplierError::NssResolutionFailed {
                name: name.to_string(),
                errno: rc,
            });
        }
        if result_ptr.is_null() {
            return Err(ApplierError::NssNotFound {
                name: name.to_string(),
            });
        }
        return Ok(grp.gr_gid);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::rust::interpreter::io::wal::PayloadRef;

    fn write_entry_at(dst: &Path, off: u64, payload: &[u8]) -> (WalEntry, [u8; 32]) {
        let PayloadRef::Hash(h) = PayloadRef::hash(payload) else {
            unreachable!()
        };
        let entry = WalEntry {
            op: WalOp::WriteAt,
            path: dst.to_path_buf(),
            extra_path: None,
            offset: Some(off),
            length: Some(payload.len() as u64),
            payload_ref: Some(PayloadRef::Hash(h)),
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        };
        (entry, h)
    }

    /// Identity path_map — production joiners apply directly to the
    /// WAL entry's canonical host path.  This unit exercises the
    /// identity path with a synthetic Write entry so a future
    /// regression in the identity closure surfaces here without
    /// waiting for the full pb_m_14_* integration tests to run.
    #[test]
    fn identity_path_map_writes_at_wal_entry_path() {
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("target.bin");
        std::fs::write(&dst, vec![0u8; 8]).unwrap();

        let payload = b"data".to_vec();
        let (entry, h) = write_entry_at(&dst, 2, &payload);
        let mut sidecar: HashMap<[u8; 32], Vec<u8>> = HashMap::new();
        sidecar.insert(h, payload.clone());

        apply_wal_to_fresh_tree(
            &[entry],
            &sidecar,
            |p| ResolvedWalPath::identity_leaf_split(p),
            &[],
        )
        .unwrap();

        let got = std::fs::read(&dst).unwrap();
        assert_eq!(&got[..2], &[0, 0]);
        assert_eq!(&got[2..2 + payload.len()], payload.as_slice());
    }

    /// H-6: `Failure`-outcome entries are skipped even when their
    /// sidecar bytes are missing — the applier must not attempt a
    /// write the leader never performed.  Regression pin lives at
    /// tests-level too (`wal_applier_skips_failure_outcome_entries`
    /// in fs_wal_spec.rs); the unit here is defensive against the
    /// applier being reused from other contexts.
    #[test]
    fn skips_failure_outcome_entries_without_touching_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("target.bin");
        std::fs::write(&dst, vec![0xAA; 8]).unwrap();

        let bogus_hash = [0u8; 32];
        let failure_entry = WalEntry {
            op: WalOp::WriteAt,
            path: dst.clone(),
            extra_path: None,
            offset: Some(0),
            length: Some(4),
            payload_ref: Some(PayloadRef::Hash(bogus_hash)),
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Failure { code: 5 },
        };
        // Empty sidecar — a Failure entry must not touch it.
        let sidecar: HashMap<[u8; 32], Vec<u8>> = HashMap::new();
        apply_wal_to_fresh_tree(
            &[failure_entry],
            &sidecar,
            |p| ResolvedWalPath::identity_leaf_split(p),
            &[],
        )
        .unwrap();

        // Byte state unchanged.
        assert_eq!(std::fs::read(&dst).unwrap(), vec![0xAA; 8]);
    }

    /// Applier respects the caller-supplied `path_map`.  A closure
    /// that redirects all writes into a translated tempdir subtree
    /// leaves the WAL entry's original path untouched.
    #[test]
    fn path_map_closure_redirects_writes() {
        let src_root = tempfile::tempdir().unwrap();
        let dst_root = tempfile::tempdir().unwrap();
        std::fs::write(src_root.path().join("f.bin"), vec![0u8; 8]).unwrap();
        std::fs::write(dst_root.path().join("f.bin"), vec![0u8; 8]).unwrap();

        let payload = b"xy".to_vec();
        let (entry, h) = write_entry_at(&src_root.path().join("f.bin"), 0, &payload);
        let mut sidecar: HashMap<[u8; 32], Vec<u8>> = HashMap::new();
        sidecar.insert(h, payload.clone());

        let src = src_root.path().to_path_buf();
        let dst = dst_root.path().to_path_buf();
        apply_wal_to_fresh_tree(
            &[entry],
            &sidecar,
            |p| {
                let rel = p.strip_prefix(&src).unwrap();
                ResolvedWalPath {
                    root: dst.clone(),
                    rel: rel.to_path_buf(),
                    expected_root_id: None,
                }
            },
            &[],
        )
        .unwrap();

        assert_eq!(
            std::fs::read(src_root.path().join("f.bin")).unwrap(),
            vec![0u8; 8]
        );
        let got = std::fs::read(dst_root.path().join("f.bin")).unwrap();
        assert_eq!(&got[..2], payload.as_slice());
    }

    // ---------------------------------------------------------------
    // 2026-08-28 hardening pins.  Every ApplierError variant that is
    // reachable via a well-formed WAL entry OR a byzantine WAL entry
    // has a runtime pin so future refactors that break the error
    // path (e.g., re-introducing a panic) fail HERE rather than
    // silently killing the boot subscriber.
    // ---------------------------------------------------------------

    /// Missing sidecar entry returns a specific error variant.
    /// Prior panic-based version killed the boot subscriber.
    #[test]
    fn missing_sidecar_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("t.bin");
        std::fs::write(&dst, vec![0u8; 8]).unwrap();
        let payload = b"missing".to_vec();
        let (entry, h) = write_entry_at(&dst, 0, &payload);
        // Empty sidecar — hash h is not present.
        let sidecar: HashMap<[u8; 32], Vec<u8>> = HashMap::new();
        let err = apply_wal_to_fresh_tree(
            &[entry],
            &sidecar,
            |p| ResolvedWalPath::identity_leaf_split(p),
            &[],
        )
        .expect_err("missing sidecar must Err");
        assert_eq!(err, ApplierError::MissingSidecarEntry {
            entry_index: 0,
            hash_hex: hex::encode(h),
        });
        // File unchanged since the applier failed before writing.
        assert_eq!(std::fs::read(&dst).unwrap(), vec![0u8; 8]);
    }

    /// A `DeployRef` payload_ref is a well-formed but not-yet-
    /// reproducible variant.  Applier reports UnsupportedPayloadRef;
    /// does not panic.
    #[test]
    fn deploy_ref_payload_ref_returns_unsupported() {
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("t.bin");
        std::fs::write(&dst, vec![0u8; 8]).unwrap();
        let entry = WalEntry {
            op: WalOp::WriteAt,
            path: dst.clone(),
            extra_path: None,
            offset: Some(0),
            length: Some(4),
            payload_ref: Some(PayloadRef::DeployRef {
                block_hash: [0; 32],
                deploy_index: 0,
                arg_index: 0,
            }),
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        };
        let err = apply_wal_to_fresh_tree(
            &[entry],
            &HashMap::new(),
            |p| ResolvedWalPath::identity_leaf_split(p),
            &[],
        )
        .expect_err("DeployRef must Err");
        assert_eq!(err, ApplierError::UnsupportedPayloadRef { entry_index: 0 });
    }

    /// A path outside every `allowed_roots` entry returns
    /// PathOutsideAllowedRoots — defense-in-depth against a
    /// leader canonicalize bug or a forged snapshot.
    #[test]
    fn path_outside_allowed_roots_returns_error() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let outside_target = outside.path().join("evil.bin");
        std::fs::write(&outside_target, vec![0u8; 8]).unwrap();

        let payload = b"attacker".to_vec();
        let (entry, h) = write_entry_at(&outside_target, 0, &payload);
        let mut sidecar: HashMap<[u8; 32], Vec<u8>> = HashMap::new();
        sidecar.insert(h, payload.clone());

        let allowed = vec![root.path().to_path_buf()];
        let err = apply_wal_to_fresh_tree(
            &[entry],
            &sidecar,
            |p| ResolvedWalPath::identity_leaf_split(p),
            &allowed,
        )
        .expect_err("out-of-root must Err");
        assert!(
            matches!(err, ApplierError::PathOutsideAllowedRoots {
                entry_index: 0,
                ..
            }),
            "got {err:?}"
        );
        // Outside file untouched.
        assert_eq!(std::fs::read(&outside_target).unwrap(), vec![0u8; 8]);
    }

    /// Empty `allowed_roots` disables validation — the applier
    /// applies to any path.  Explicit pin: production callsites
    /// currently pass `&[]` until provisioning is plumbed.
    #[test]
    fn empty_allowed_roots_skips_validation() {
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("t.bin");
        std::fs::write(&dst, vec![0u8; 8]).unwrap();
        let payload = b"ok".to_vec();
        let (entry, h) = write_entry_at(&dst, 0, &payload);
        let mut sidecar: HashMap<[u8; 32], Vec<u8>> = HashMap::new();
        sidecar.insert(h, payload.clone());
        // allowed_roots empty AND dst is a tempdir path outside
        // any "consensus-static" prefix — validation must not fire.
        apply_wal_to_fresh_tree(
            &[entry],
            &sidecar,
            |p| ResolvedWalPath::identity_leaf_split(p),
            &[],
        )
        .unwrap();
        let got = std::fs::read(&dst).unwrap();
        assert_eq!(&got[..2], payload.as_slice());
    }

    /// A NULL byte inside a chown target path is caught by
    /// `safe_descend_verified`'s `to_c` step (post-S-1
    /// 2026-09-03) and surfaces as `SafeDescendFailed`.  Pre-
    /// hardening this triggered a `.unwrap()` panic that killed
    /// the boot subscriber loop; pre-S-1 it surfaced as
    /// `PathContainsNull` via the applier's own `os_str_to_cstring`
    /// pre-check.
    ///
    /// Chown resolves NSS names BEFORE descent, so a nonexistent
    /// owner must NOT be used here (that would fire `NssNotFound`
    /// first).  Use a name that exists on virtually every host
    /// (`root`) so we exercise the descent-side NULL check.
    #[test]
    fn chown_path_with_null_byte_returns_error() {
        use std::os::unix::ffi::OsStrExt;

        let bad_path = PathBuf::from(std::ffi::OsStr::from_bytes(b"/tmp/has\0null"));
        let entry = WalEntry {
            op: WalOp::Chown,
            path: bad_path,
            extra_path: None,
            offset: None,
            length: None,
            payload_ref: None,
            mode_bits: None,
            owner: Some(String::new()),
            group: Some(String::new()),
            outcome: WalOutcome::Success,
        };
        let err = apply_wal_to_fresh_tree(
            &[entry],
            &HashMap::new(),
            |p| ResolvedWalPath::identity_leaf_split(p),
            &[],
        )
        .expect_err("path with NULL must Err");
        assert!(
            matches!(err, ApplierError::SafeDescendFailed { entry_index: 0, .. }),
            "got {err:?}"
        );
    }

    /// Empty owner + empty group short-circuits to (u32::MAX,
    /// u32::MAX) sentinels (i.e., "no change" in POSIX chown
    /// semantics).  Regression pin for the NSS-avoidance path.
    #[test]
    fn chown_empty_owner_and_group_short_circuits_nss() {
        // Empty owner + empty group means "u32::MAX for both",
        // which is POSIX "leave uid/gid unchanged".  No NSS lookup
        // fires; applier returns Ok even without NSS entries for
        // any name.
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("chownable.bin");
        std::fs::write(&dst, vec![0u8; 4]).unwrap();
        let entry = WalEntry {
            op: WalOp::Chown,
            path: dst.clone(),
            extra_path: None,
            offset: None,
            length: None,
            payload_ref: None,
            mode_bits: None,
            owner: Some(String::new()),
            group: Some(String::new()),
            outcome: WalOutcome::Success,
        };
        // Should return Ok on any host, no NSS involvement.
        apply_wal_to_fresh_tree(
            &[entry],
            &HashMap::new(),
            |p| ResolvedWalPath::identity_leaf_split(p),
            &[],
        )
        .unwrap();
    }

    /// Chown NSS lookup with a nonexistent owner name surfaces as
    /// NssNotFound (not a panic).  Pin against the reentrant
    /// getpwnam_r path.
    #[test]
    fn chown_nonexistent_owner_returns_nss_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("chownable.bin");
        std::fs::write(&dst, vec![0u8; 4]).unwrap();
        // A name that (almost) certainly resolves to nothing.
        let entry = WalEntry {
            op: WalOp::Chown,
            path: dst,
            extra_path: None,
            offset: None,
            length: None,
            payload_ref: None,
            mode_bits: None,
            owner: Some("no-such-user-in-nss-4f8d3a2e".to_string()),
            group: None,
            outcome: WalOutcome::Success,
        };
        let err = apply_wal_to_fresh_tree(
            &[entry],
            &HashMap::new(),
            |p| ResolvedWalPath::identity_leaf_split(p),
            &[],
        )
        .expect_err("nonexistent owner must Err");
        assert!(
            matches!(err, ApplierError::NssNotFound { .. }),
            "got {err:?}"
        );
    }

    /// A Rename/CopyFile entry missing extra_path returns the
    /// MissingExtraPath variant.  Invariant violation surfaced
    /// rather than panicking.
    #[test]
    fn rename_without_extra_path_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        std::fs::write(&a, b"a").unwrap();
        let entry = WalEntry {
            op: WalOp::Rename,
            path: a,
            extra_path: None,
            offset: None,
            length: None,
            payload_ref: None,
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        };
        let err = apply_wal_to_fresh_tree(
            &[entry],
            &HashMap::new(),
            |p| ResolvedWalPath::identity_leaf_split(p),
            &[],
        )
        .expect_err("missing extra_path must Err");
        assert_eq!(err, ApplierError::MissingExtraPath {
            entry_index: 0,
            op: WalOp::Rename,
        });
    }

    /// Truncate without offset returns MissingOffset (previously
    /// panicked via `.expect(...)`).
    #[test]
    fn truncate_without_offset_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("t.bin");
        std::fs::write(&dst, vec![0u8; 16]).unwrap();
        let entry = WalEntry {
            op: WalOp::Truncate,
            path: dst,
            extra_path: None,
            offset: None,
            length: None,
            payload_ref: None,
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        };
        let err = apply_wal_to_fresh_tree(
            &[entry],
            &HashMap::new(),
            |p| ResolvedWalPath::identity_leaf_split(p),
            &[],
        )
        .expect_err("missing offset on Truncate must Err");
        assert_eq!(err, ApplierError::MissingOffset {
            entry_index: 0,
            op: WalOp::Truncate,
        });
    }

    /// An IO failure (e.g., open of a nonexistent parent dir for
    /// truncate) surfaces as IoFailure — not a panic.
    #[test]
    fn truncate_missing_target_returns_io_failure() {
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("does-not-exist.bin");
        let entry = WalEntry {
            op: WalOp::Truncate,
            path: dst.clone(),
            extra_path: None,
            offset: Some(0),
            length: None,
            payload_ref: None,
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        };
        let err = apply_wal_to_fresh_tree(
            &[entry],
            &HashMap::new(),
            |p| ResolvedWalPath::identity_leaf_split(p),
            &[],
        )
        .expect_err("truncate on missing file must Err");
        assert!(
            matches!(err, ApplierError::IoFailure {
                entry_index: 0,
                op: WalOp::Truncate,
                ..
            }),
            "got {err:?}"
        );
    }

    /// S-1 TOCTOU pin (2026-09-03): a symlink component along the
    /// on-disk path is rejected by `safe_descend_verified`'s
    /// `openat(O_NOFOLLOW)` step, surfacing as `SafeDescendFailed`
    /// rather than silently traversing into the symlink target.
    /// Pre-S-1 the applier's `std::fs::*` calls followed the
    /// symlink, letting a race with a local process redirect the
    /// write outside the on-disk root.
    #[test]
    fn symlink_intermediate_component_returns_safe_descend_failed() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let evil = tempfile::tempdir().unwrap();
        // /root/sub is a symlink to /evil (the attacker's tree).
        symlink(evil.path(), root.path().join("sub")).unwrap();
        // Target the applier at /root/sub/target.bin — a Write op
        // that pre-S-1 would follow the symlink and land in /evil.
        let target = root.path().join("sub").join("target.bin");
        let payload = b"hello".to_vec();
        let (entry, h) = write_entry_at(&target, 0, &payload);
        let mut sidecar: HashMap<[u8; 32], Vec<u8>> = HashMap::new();
        sidecar.insert(h, payload.clone());

        let root_pb = root.path().to_path_buf();
        let err = apply_wal_to_fresh_tree(
            &[entry],
            &sidecar,
            |p| {
                let rel = p.strip_prefix(&root_pb).unwrap();
                ResolvedWalPath {
                    root: root_pb.clone(),
                    rel: rel.to_path_buf(),
                    expected_root_id: None,
                }
            },
            &[],
        )
        .expect_err("symlink component must Err");
        assert!(
            matches!(err, ApplierError::SafeDescendFailed { entry_index: 0, .. }),
            "got {err:?}"
        );
        // The attacker's tree is untouched.
        assert!(!evil.path().join("target.bin").exists());
    }

    /// S-1 TOCTOU pin (2026-09-03): a mismatched `expected_root_id`
    /// (as would happen under the H-5 rename-and-recreate attack)
    /// is rejected by `safe_descend_verified` and surfaces as
    /// `SafeDescendFailed`.  Pre-S-1 the applier had no identity
    /// check at all — a swapped root's contents were silently
    /// mutated.
    #[test]
    fn mismatched_root_identity_returns_safe_descend_failed() {
        let root = tempfile::tempdir().unwrap();
        let dst = root.path().join("t.bin");
        std::fs::write(&dst, vec![0u8; 8]).unwrap();

        let payload = b"data".to_vec();
        let (entry, h) = write_entry_at(&dst, 0, &payload);
        let mut sidecar: HashMap<[u8; 32], Vec<u8>> = HashMap::new();
        sidecar.insert(h, payload.clone());

        let root_pb = root.path().to_path_buf();
        // Synthesize a bogus identity — root's real (dev, inode)
        // will not match, forcing RootIdentityChanged.
        let err = apply_wal_to_fresh_tree(
            &[entry],
            &sidecar,
            |_p| ResolvedWalPath {
                root: root_pb.clone(),
                rel: std::path::PathBuf::from("t.bin"),
                expected_root_id: Some((u64::MAX, u64::MAX)),
            },
            &[],
        )
        .expect_err("mismatched root identity must Err");
        assert!(
            matches!(err, ApplierError::SafeDescendFailed { entry_index: 0, .. }),
            "got {err:?}"
        );
        // File unchanged.
        assert_eq!(std::fs::read(&dst).unwrap(), vec![0u8; 8]);
    }
}
