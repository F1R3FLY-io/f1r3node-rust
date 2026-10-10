// WAL fresh-tree applier — foundation types (PR #519) + pure
// syscall helpers (PR #521) + NSS lookup helpers (PR #522) +
// main dispatcher (this PR, final slice of the wal_applier
// submodule tree).
//
// PR #519 shipped the shared types the applier builds on:
//
//   - [`ResolvedWalPath`] — the `(on_disk_root, rel_from_root,
//     expected_root_id)` triple the applier hands to
//     [`super::path::safe_descend_verified`] (PR #517 landed the
//     explicit-identity variant that lets WAL replay sites
//     thread this triple directly instead of wrapping each entry
//     in a `Root`).
//   - [`ApplierError`] — the full fault surface the dispatcher
//     and its helpers return.  13 variants cover byzantine input
//     (`MissingPayloadRef`, `PathContainsNull`,
//     `UnsupportedPayloadRef`), internal-invariant violations
//     (`MissingOffset`, `MissingModeBits`), out-of-tree writes
//     (`PathOutsideAllowedRoots`), NSS failures
//     (`NssResolutionFailed`, `NssNotFound`), I/O failures
//     (`IoFailure`, `ChownFailed`), payload-sidecar misses
//     (`MissingSidecarEntry`), and safe-descent failures
//     (`SafeDescendFailed`).
//
// PR #521 added the pure syscall helpers the dispatcher calls:
// `format_quarantine`, `openat_leaf`, `pwrite_all`, `copy_at`,
// and `check_path_allowed`.
//
// PR #522 added the NSS lookup helpers used by the Chown
// branch (`resolve_uid` / `resolve_gid`) as thin adapters over
// [`super::nss`]'s shared libc FFI machinery.
//
// This slice (PR #523, CAPSTONE) adds the main
// [`apply_wal_to_fresh_tree`] dispatcher + `descend_entry`
// helper.  The dispatcher iterates `wal` entries, skips
// Failure-outcome entries (H-6), and dispatches each `WalOp`
// through the matching libc syscall family:
//
//   - Write/WriteAt → openat + pwrite_all + close
//   - Truncate      → openat + ftruncate + close
//   - Chmod         → fchmodat
//   - Chown         → resolve_uid/gid + fchownat
//   - RemoveFile    → unlinkat
//   - RemoveDir     → unlinkat(AT_REMOVEDIR)
//   - Rename        → renameat (both sides descend)
//   - CopyFile      → copy_at (both sides descend)
//   - Read/Stat/.../Exists → observation-only, no-op
//
// Every entry's path (and `extra_path` for Rename/CopyFile)
// routes through [`descend_entry`] which checks
// `allowed_roots`, resolves via `path_map` closure, then
// calls `safe_descend_verified`.  The resulting
// `SafeParent` dirfd is used for every `*at` syscall so the
// S-1 TOCTOU discipline holds end-to-end.
//
// This closes the wal_applier submodule tree.  Downstream
// consumers (yet-to-land joiner-side `wal_payload_sync` and
// test fixtures via `fileio-test-fixtures/`) can now invoke
// `apply_wal_to_fresh_tree` against a decoded WAL slice +
// resolved payload-bytes sidecar.
//
// # Why `ResolvedWalPath` and not `&Root` for replay sites
//
// Boot-path handlers already have a [`super::path::identity::Root`]
// on hand (constructed via `Root::capture` at boot and threaded
// via the per-runtime registry).  WAL replay resolves
// `(on_disk_root, rel, (dev, ino))` per entry from a boot-
// populated registry — the explicit triple lets the dispatcher
// thread raw path bytes + an optional identity straight through
// without wrapping each entry in a `Root`.  Test harnesses with
// operator-frozen tempdirs can pass `expected_root_id = None`
// to skip the H-5 check.  Production replay sites MUST pass
// `Some((dev, ino))` — `None` silently drops the rename-and-
// recreate defense (see PR #517's `safe_descend_verified`
// docstring).
//
// # TOCTOU discipline (same as handlers)
//
// Every mutation runs as a `*at` syscall against the dirfd
// returned by `safe_descend_verified`.  A component swap
// between descent and syscall cannot escape the on-disk root
// — the same discipline the leader-side handlers use for
// symlink-swap-immunity.
//
// # Path validation (defense-in-depth)
//
// The dispatcher accepts `allowed_roots: &[PathBuf]` — if
// non-empty, every WAL entry's `path` (and `extra_path` for
// Rename/CopyFile) must be under one of those roots or the
// dispatcher returns [`ApplierError::PathOutsideAllowedRoots`]
// without touching disk.  Pass `&[]` to skip validation (tests
// or production sites with no provisioning plumbing).  This
// check is in a yet-to-land slice; the error variant lives here
// so the type surface is complete.
//
// # Sidecar authentication model
//
// `payload_bytes` carries Write/WriteAt bodies keyed by their
// Blake2b256 hash.  Production joiners verify
// `Blake2b256(fetched_bytes) == entry.payload_ref` before
// installing them in `payload_bytes` — Blake2b256 pre-image
// resistance makes serving alternative bytes with the same hash
// cryptographically infeasible.  The sidecar bytes are NOT
// independently signed (unlike manifest entries post-H-4):
// security = leader trust + hash check + TLS transport hygiene.
//
// If sidecar transport ever moves to a shared cache (Redis, S3,
// pubsub fan-out) or a peer-to-peer redistribution overlay, the
// current "hash-check-only" discipline needs a signature
// binding (leader signs `(payload_ref, deploy_scope)`; joiners
// verify the signature before installing).  Rationale: shared
// caches weaken the "malicious peer can only serve bytes with
// the correct hash" guarantee.  Any such refactor MUST cite
// this note.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::path::descend::safe_descend_verified;
use super::path::{QuarantineError, SafeParent};
use super::wal::{PayloadRef, WalEntry, WalOp, WalOutcome};

/// Result of decomposing a WAL entry's absolute path into the
/// `(on_disk_root, rel_from_root, expected_root_id)` triple the
/// applier hands to [`super::path::safe_descend_verified`].
///
/// Production callers construct this via (yet-to-land)
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
    /// Convenience for tests: `<parent>/<file_name>` split with
    /// no identity check.  Callers that already know the tempdir
    /// root + relative filename should construct the struct
    /// directly (this fallback only works for depth-1 paths).
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
    /// Reserved for a future reducer slice that resolves deploy
    /// refs from on-chain deploy data.
    UnsupportedPayloadRef { entry_index: usize },
    /// The WAL entry is a write op but `payload_ref` is `None`.
    /// Invariant violation: the leader's `journal_write` must
    /// populate this field.
    MissingPayloadRef { entry_index: usize, op: WalOp },
    /// A Write/WriteAt/Truncate entry is missing its `offset`.
    /// Invariant violation.
    MissingOffset { entry_index: usize, op: WalOp },
    /// A Chmod entry is missing `mode_bits`.
    MissingModeBits { entry_index: usize },
    /// A Chown entry is missing its `owner` field.
    MissingOwner { entry_index: usize },
    /// A Rename/CopyFile entry is missing `extra_path`.
    MissingExtraPath { entry_index: usize, op: WalOp },
    /// The WAL entry's path contains a NULL byte, which the
    /// `safe_descend_verified` layer catches at `to_c` before any
    /// syscall.  Retained for callers that may synthesize path-
    /// level pre-checks; the primary path routes NULL through
    /// `SafeDescendFailed`.
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
    /// unprivileged hosts — see the Chown branch's comment, in
    /// a yet-to-land slice).
    ChownFailed {
        entry_index: usize,
        path: PathBuf,
        errno: i32,
    },
    /// `safe_descend_verified` failed for this entry's
    /// (on-disk-root, rel) — e.g., a symlink component was
    /// found, the root's boot-captured identity no longer
    /// matches, or the rel escaped the root.  The applier
    /// surfaces descent failures explicitly rather than papering
    /// over them with a downstream open error.
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

// Compile-time witness that `ApplierError: Send + Sync` — required
// for propagation across `tokio::spawn_blocking` boundaries in the
// (yet-to-land) joiner-side subscriber task.  Hoisted to module
// scope so every `cargo build` catches a regression, not only
// `cargo test`.  Same pattern as `SnapshotError`'s witness.
const _APPLIER_ERROR_IS_SEND_SYNC: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ApplierError>();
};

// ===========================================================
// Pure syscall helpers (slice 2) — used by the dispatcher
// ===========================================================
//
// Each helper is `pub(super)` so the dispatcher can call it;
// no `#[allow(dead_code)]` is needed now that slice 4
// (the dispatcher) wires every helper to a call site.  A
// future refactor that orphans a helper surfaces as a
// standard dead-code warning.

/// Render a [`QuarantineError`] into a short operator-facing
/// string suitable for the `reason` field of
/// [`ApplierError::SafeDescendFailed`].  Keeps the shape stable
/// across platforms (`std::io::Error` renders include errno text
/// which varies between Linux and macOS; the `(kind, msg)` tuple
/// in `IoError` is already scrubbed by `io_msg_scrub` at the
/// `path` module boundary).
pub(super) fn format_quarantine(qe: &QuarantineError) -> String {
    match qe {
        QuarantineError::Empty => "empty rel".to_string(),
        QuarantineError::RootSelf => "rel resolves to root itself".to_string(),
        QuarantineError::EscapesRoot => "rel escapes root".to_string(),
        QuarantineError::SymlinkComponent => "symlink component in path".to_string(),
        QuarantineError::RootIdentityChanged => "root identity changed post-boot".to_string(),
        QuarantineError::IoError(kind, msg) => format!("{kind:?}: {msg}"),
    }
}

/// `openat` on `parent`'s dirfd using `parent`'s leaf name,
/// returning an [`OwnedFd`] on success or [`ApplierError::IoFailure`]
/// with the full WAL-entry context on failure.  `O_NOFOLLOW` is
/// unconditionally ORed into `flags` to preserve the S-1 TOCTOU
/// discipline — a leaf-level symlink is rejected here rather than
/// followed into whatever the attacker populated.
///
/// # Why `OwnedFd`
///
/// Returning an owning handle (vs. a raw `c_int`) makes close-on-
/// every-exit-path structural rather than conventional:
///
///   - Normal control flow: `OwnedFd` drops at scope end, closing
///     the fd exactly once.
///   - Panic unwinding (e.g., allocator OOM during `pwrite_all`):
///     `OwnedFd`'s Drop still runs, closing the fd.
///   - Early `?` returns: same.
///
/// With the raw-fd shape, the dispatcher had to pair every
/// `openat_leaf` call with an explicit `unsafe { libc::close(fd) }`
/// before any error propagation.  A panic between open and close
/// would leak the fd (recovered on process exit, but not clean).
/// The `OwnedFd` shape eliminates two `unsafe` close blocks at
/// each call site and makes panic + error paths structurally
/// equivalent.
pub(super) fn openat_leaf(
    entry_index: usize,
    op: WalOp,
    parent: &SafeParent,
    dst: &Path,
    flags: libc::c_int,
    mode: libc::mode_t,
) -> Result<std::os::fd::OwnedFd, ApplierError> {
    use std::os::fd::FromRawFd;
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
    // SAFETY: `openat` returned a non-negative `fd` (checked
    // above), so this is a fresh kernel-side open file descriptor
    // this function owns exclusively.  Transferring ownership to
    // `OwnedFd` means Drop closes it on every exit path.
    Ok(unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) })
}

/// EINTR-tolerant positioned-write loop.  Writes the entire
/// `bytes` slice at absolute offset `off` into `fd`, retrying on
/// short writes and `EINTR`.  Returns [`WriteZero`] if
/// `pwrite(2)` reports 0 progress — treated as a fatal failure
/// (nothing can rescue a kernel that stops accepting writes
/// without an error).
///
/// Caller owns `fd`'s lifetime; this helper neither opens nor
/// closes.  Not `pub(super)` — a yet-to-land dispatcher slice
/// is the only intended caller, but exposed to tests via the
/// `#[cfg(test)] mod tests` boundary.
///
/// [`WriteZero`]: std::io::ErrorKind::WriteZero
pub(super) fn pwrite_all(fd: libc::c_int, bytes: &[u8], off: u64) -> std::io::Result<()> {
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

/// [`copy_at`]'s read/write buffer size.  64 KiB matches typical
/// kernel readahead on both Linux and macOS, so a single
/// `read(2)` usually fills the buffer in one hop for medium
/// files.  Named here so a future tuning (e.g., jumping to
/// 1 MiB after kernel readahead-size changes) is a one-line
/// edit instead of a search-and-replace.
const COPY_BUF_LEN: usize = 64 * 1024;

/// Portable file-to-file copy via two openat'd fds + a read/write
/// loop.  Avoids `libc::sendfile` / `copy_file_range` for macOS
/// compatibility.  The destination is created with 0o644 and
/// truncated — matches `std::fs::copy` semantics closely enough
/// for WAL replay (leader's Chmod entries adjust perms after the
/// fact).
///
/// Both source and destination are opened with `O_NOFOLLOW` so
/// a symlink leaf on either side is rejected rather than
/// followed (S-1 TOCTOU discipline for CopyFile replay).  Both
/// fds are closed on all exit paths via an `FdGuard` RAII.
///
/// The read/write buffer is [`COPY_BUF_LEN`] bytes.  Matches
/// typical kernel readahead on both Linux and macOS so a single
/// `read(2)` system call usually fills the buffer in one hop
/// for medium files; larger files iterate (pinned by
/// `copy_at_large_file_round_trips_through_multi_iteration_loop`).
pub(super) fn copy_at(from_parent: &SafeParent, to_parent: &SafeParent) -> std::io::Result<()> {
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

    let mut buf = [0u8; COPY_BUF_LEN];
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

/// Defense-in-depth path validation.  Returns `Ok(())` if
/// `path` starts with any entry in `allowed_roots`;
/// [`ApplierError::PathOutsideAllowedRoots`] otherwise.
///
/// # The empty-allowed-roots contract
///
/// Called with an empty `allowed_roots`, this function returns
/// `Err` for every path (because `[].iter().any(_)` is `false`).
/// The "pass `&[]` to skip validation" semantics live at the
/// dispatcher: the dispatcher gates this call with
/// `if !allowed_roots.is_empty()` and only invokes
/// `check_path_allowed` when at least one root is configured.
///
/// This split keeps the function pure (no "if empty, pass"
/// hidden branch) and keeps the "do I need to validate at all?"
/// decision at a single caller site.
pub(super) fn check_path_allowed(
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

// ===========================================================
// NSS lookup helpers (slice 3) — Chown branch supports
// ===========================================================
//
// The Chown branch of the dispatcher receives `owner` and
// `group` as textual names (via the WAL entry's
// `Option<String>` fields) and must resolve them to numeric
// `(uid, gid)` for `libc::fchownat`.
//
// Thin adapters over `super::nss::resolve_uid_detailed` /
// `resolve_gid_detailed` — the core libc FFI machinery
// (ERANGE grow-and-retry, SAFETY-commented syscall shape,
// reentrant `_r` variants, buffer ceiling) lives in `nss.rs`
// so a future NSS tweak touches one site.  The adapter's job
// is just to map the shared `Result<Option<u32>, i32>`
// surface onto `ApplierError`:
//
//   - `Ok(Some(uid))` → `Ok(uid)`.
//   - `Ok(None)` → `Err(NssNotFound)` — genuine miss (null
//     result_ptr, ENOENT, ESRCH).
//   - `Err(errno)` → `Err(NssResolutionFailed { errno })` —
//     any other failure (EINVAL for NUL in input, ERANGE at
//     the `NSS_BUF_MAX` ceiling, EIO/EAGAIN/etc.).  The
//     EINVAL mapping (vs. previously overloading NssNotFound
//     for NUL inputs) is the semantic fix the consolidation
//     carries along: "invalid input" and "name resolved to
//     no entry" are distinct conditions with different
//     operator-facing remediation.

/// Resolve a user name to its numeric uid via
/// [`super::nss::resolve_uid_detailed`], mapping onto the
/// applier's structured [`ApplierError`] surface.
pub(super) fn resolve_uid(name: &str) -> Result<u32, ApplierError> {
    match super::nss::resolve_uid_detailed(name) {
        Ok(Some(uid)) => Ok(uid),
        Ok(None) => Err(ApplierError::NssNotFound {
            name: name.to_string(),
        }),
        Err(errno) => Err(ApplierError::NssResolutionFailed {
            name: name.to_string(),
            errno,
        }),
    }
}

/// Resolve a group name to its numeric gid via
/// [`super::nss::resolve_gid_detailed`], mapping onto the
/// applier's structured [`ApplierError`] surface.  Same shape
/// as [`resolve_uid`].
pub(super) fn resolve_gid(name: &str) -> Result<u32, ApplierError> {
    match super::nss::resolve_gid_detailed(name) {
        Ok(Some(gid)) => Ok(gid),
        Ok(None) => Err(ApplierError::NssNotFound {
            name: name.to_string(),
        }),
        Err(errno) => Err(ApplierError::NssResolutionFailed {
            name: name.to_string(),
            errno,
        }),
    }
}

// ===========================================================
// Main dispatcher (slice 4 — CAPSTONE)
// ===========================================================

/// Apply a captured WAL slice to a filesystem tree.
///
/// See the module docstring for supported ops, `path_map`
/// semantics, path validation, and error variants.
///
/// `path_map` receives the WAL entry's `path` (or `extra_path`
/// for Rename / CopyFile) and returns a [`ResolvedWalPath`] —
/// the `(on_disk_root, rel_from_root, expected_root_id)`
/// triple the applier hands to [`safe_descend_verified`].
/// Every mutation runs as a `*at` syscall against the
/// descended dirfd, so a component swap between descent and
/// syscall cannot escape the on-disk root (S-1 TOCTOU
/// discipline).
///
/// # H-6 Failure-outcome skip
///
/// WAL entries whose outcome is `Failure { .. }` are skipped
/// unconditionally.  The leader never mutated disk on
/// Failure, so replaying a Failure entry on the follower
/// would introduce a divergence.
///
/// # Chown's EPERM-is-ok posture
///
/// `fchownat` with `AT_SYMLINK_NOFOLLOW` returning `EPERM`
/// is treated as a no-op success — unprivileged hosts
/// (typical CI, test harnesses) can't chown to arbitrary
/// owners.  Tests should use current-user names to avoid this
/// branch; production joiners run as the node service user
/// and MUST have the perms to replay the leader's chown ops.
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
                let owned_fd = openat_leaf(
                    i,
                    entry.op,
                    &parent,
                    &dst,
                    libc::O_WRONLY | libc::O_CREAT | libc::O_CLOEXEC,
                    0o644,
                )?;
                use std::os::fd::AsRawFd;
                let write_res = pwrite_all(owned_fd.as_raw_fd(), bytes, off);
                // `owned_fd` drops at scope end → close.  Panic
                // during `pwrite_all` or error-propagation via `?`
                // below both close the fd via Drop.
                drop(owned_fd);
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
                let owned_fd = openat_leaf(
                    i,
                    entry.op,
                    &parent,
                    &dst,
                    libc::O_WRONLY | libc::O_CLOEXEC,
                    0,
                )?;
                use std::os::fd::AsRawFd;
                // SAFETY: `owned_fd` keeps the fd open for the
                // duration of this block; `ftruncate` operates
                // purely on the kernel-side file object and does
                // not touch userspace memory.  The owning `OwnedFd`
                // Drop closes on every exit path.
                let rc = unsafe { libc::ftruncate(owned_fd.as_raw_fd(), n as libc::off_t) };
                let ftrunc_err = if rc < 0 {
                    Some(std::io::Error::last_os_error())
                } else {
                    None
                };
                // `owned_fd` drops at scope end → close.  Panic
                // or error-propagation both close the fd via Drop.
                drop(owned_fd);
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
                // Chown.owner contract: the leader-side journaling
                // (yet-to-land in handlers.rs) MUST populate
                // `owner` for every Chown entry — either with the
                // name string (triggers NSS resolve) or with
                // `Some("")` meaning "leave uid unchanged" (short-
                // circuits NSS to the POSIX `u32::MAX` sentinel).
                // `None` is a leader-side invariant violation; we
                // reject with `MissingOwner` so the subscriber
                // surfaces the bug rather than silently applying a
                // zero-field chown.  `group` is more permissive:
                // both `None` and `Some("")` route to the "leave
                // gid unchanged" sentinel.
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

/// Common prologue for every op: check `allowed_roots` on the
/// raw WAL entry path (bundle-relative), run the closure to
/// resolve to the on-disk absolute, then
/// [`safe_descend_verified`] to obtain the [`SafeParent`]
/// dirfd.  Returns the descended parent + the joined on-disk
/// path for error messages.
///
/// # Ordering rationale
///
/// WAL entries carry bundle-relative paths (e.g.
/// `/@bundle/target`).  Node setup registers `BUNDLE_ROOT_PREFIX`
/// (`/@bundle`) as a consensus-static root plus the operator's
/// absolute per-validator paths.  Checking the RAW `entry_path`
/// against `allowed_roots` matches on the bundle-relative
/// prefix directly, without depending on how the registry
/// resolves it to a per-validator on-disk subdir.  Checking the
/// resolved on-disk root would require the operator to register
/// per-validator absolute paths whose exact lexical shape
/// matches the registry's output — brittle across
/// canonicalization variants.
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

#[cfg(test)]
mod tests {
    use super::super::wal::WalOp;
    use super::*;

    #[test]
    fn resolved_wal_path_identity_leaf_split_depth_one_path() {
        let p = Path::new("/tmp/leaf.txt");
        let r = ResolvedWalPath::identity_leaf_split(p);
        assert_eq!(r.root, PathBuf::from("/tmp"));
        assert_eq!(r.rel, PathBuf::from("leaf.txt"));
        assert_eq!(
            r.expected_root_id, None,
            "test-helper always skips identity"
        );
    }

    /// Edge case: a path with no parent (e.g., root-level) falls
    /// back to `"/"` for root — matches fileio's convenience
    /// semantics.  Depth-1 paths with no filename component
    /// aren't a realistic WAL input but keep the helper total.
    #[test]
    fn resolved_wal_path_identity_leaf_split_root_level_falls_back() {
        let p = Path::new("/");
        let r = ResolvedWalPath::identity_leaf_split(p);
        assert_eq!(r.root, PathBuf::from("/"));
        assert_eq!(r.rel, PathBuf::new());
        assert_eq!(r.expected_root_id, None);
    }

    /// `ResolvedWalPath: Clone + PartialEq + Eq` — pinned via a
    /// trivial roundtrip so a derive-removal regression surfaces.
    #[test]
    fn resolved_wal_path_derived_traits() {
        let a = ResolvedWalPath {
            root: PathBuf::from("/root"),
            rel: PathBuf::from("rel"),
            expected_root_id: Some((1, 2)),
        };
        let b = a.clone();
        assert_eq!(a, b);
    }

    /// LOAD-BEARING: every `ApplierError` variant has a `Display`
    /// impl that renders operator-useful content.  Rather than
    /// enumerate 13 separate tests, pin one representative per
    /// shape and let the match-exhaustiveness check catch new
    /// variants at compile time (adding a variant without
    /// extending the Display match fails the build).
    #[test]
    fn applier_error_display_covers_every_variant() {
        let cases: Vec<ApplierError> = vec![
            ApplierError::MissingSidecarEntry {
                entry_index: 7,
                hash_hex: "deadbeef".into(),
            },
            ApplierError::UnsupportedPayloadRef { entry_index: 1 },
            ApplierError::MissingPayloadRef {
                entry_index: 2,
                op: WalOp::Write,
            },
            ApplierError::MissingOffset {
                entry_index: 3,
                op: WalOp::Truncate,
            },
            ApplierError::MissingModeBits { entry_index: 4 },
            ApplierError::MissingOwner { entry_index: 5 },
            ApplierError::MissingExtraPath {
                entry_index: 6,
                op: WalOp::Rename,
            },
            ApplierError::PathContainsNull { entry_index: 8 },
            ApplierError::PathOutsideAllowedRoots {
                entry_index: 9,
                path: PathBuf::from("/etc/passwd"),
            },
            ApplierError::NssResolutionFailed {
                name: "alice".into(),
                errno: 11,
            },
            ApplierError::NssNotFound {
                name: "ghost".into(),
            },
            ApplierError::IoFailure {
                entry_index: 10,
                op: WalOp::Write,
                path: PathBuf::from("/tmp/x"),
                message: "no space left on device".into(),
            },
            ApplierError::ChownFailed {
                entry_index: 11,
                path: PathBuf::from("/tmp/y"),
                errno: 1,
            },
            ApplierError::SafeDescendFailed {
                entry_index: 12,
                root: PathBuf::from("/root"),
                rel: PathBuf::from("deep/path"),
                reason: "symlink component".into(),
            },
        ];
        for case in &cases {
            let s = format!("{case}");
            assert!(!s.is_empty(), "Display produced empty string for {case:?}");
            assert!(
                !s.contains("<unknown>"),
                "Display produced fallback-looking text for {case:?}: {s}"
            );
        }
    }

    /// Spot-check that specific variants surface the identifying
    /// field in their Display output.  Catches a lazy
    /// `write!(f, "applier error")` refactor that would still
    /// pass the "not empty" check above.
    #[test]
    fn applier_error_display_includes_identifying_field() {
        let s = format!("{}", ApplierError::MissingSidecarEntry {
            entry_index: 42,
            hash_hex: "cafebabe".into(),
        });
        assert!(s.contains("42"), "entry_index embedded: {s}");
        assert!(s.contains("cafebabe"), "hash_hex embedded: {s}");

        let s = format!("{}", ApplierError::PathOutsideAllowedRoots {
            entry_index: 1,
            path: PathBuf::from("/etc/passwd"),
        });
        assert!(s.contains("/etc/passwd"), "path embedded: {s}");

        let s = format!("{}", ApplierError::NssNotFound {
            name: "unicorn".into(),
        });
        assert!(s.contains("unicorn"), "nss name embedded: {s}");
    }

    /// `ApplierError: std::error::Error` — pinned so a future
    /// refactor that forgot the `impl Error` block (e.g., when
    /// adding variants and typing `impl Display` without the
    /// matching Error) fails this test.
    #[test]
    fn applier_error_implements_std_error_trait() {
        fn assert_error<T: std::error::Error>() {}
        assert_error::<ApplierError>();
    }

    // ---------------------------------------------------------------
    // Pure syscall helpers (slice 2)
    // ---------------------------------------------------------------

    use super::super::path::descend::safe_descend_verified;
    use super::super::path::QuarantineError;

    /// LOAD-BEARING: pin the `format_quarantine` arm for every
    /// `QuarantineError` variant.  A new `QuarantineError` variant
    /// would silently take the catch-all arm (if we had one) or
    /// fail to compile (match exhaustiveness) — we have the latter
    /// posture here.  The content check ensures the output isn't a
    /// placeholder like "unknown".
    #[test]
    fn format_quarantine_covers_every_variant() {
        let cases: Vec<QuarantineError> = vec![
            QuarantineError::Empty,
            QuarantineError::RootSelf,
            QuarantineError::EscapesRoot,
            QuarantineError::SymlinkComponent,
            QuarantineError::RootIdentityChanged,
            QuarantineError::IoError(std::io::ErrorKind::NotFound, "example path missing".into()),
        ];
        for case in &cases {
            let s = format_quarantine(case);
            assert!(!s.is_empty(), "empty format for {case:?}");
            assert!(!s.contains("unknown"), "placeholder-looking text: {s}");
        }
    }

    /// IoError arm embeds both the kind debug and the scrubbed
    /// message — pinned so a future refactor that dropped either
    /// surfaces here instead of silently losing operator-facing
    /// diagnostics.
    #[test]
    fn format_quarantine_io_error_embeds_kind_and_message() {
        let s = format_quarantine(&QuarantineError::IoError(
            std::io::ErrorKind::PermissionDenied,
            "scrubbed-permission-denied".into(),
        ));
        assert!(s.contains("PermissionDenied"), "ErrorKind embedded: {s}");
        assert!(
            s.contains("scrubbed-permission-denied"),
            "message embedded: {s}"
        );
    }

    /// `check_path_allowed(&[])` returns `Err` for every path —
    /// the "skip validation on empty allow-list" semantics is
    /// enforced by the dispatcher's `if !allowed_roots.is_empty()`
    /// gate, NOT by this function.  Pin the raw-function contract
    /// so a future refactor that moves the gate into the function
    /// surfaces here (and the dispatcher's gate becomes
    /// dead-code).
    #[test]
    fn check_path_allowed_empty_allowed_roots_rejects_every_path() {
        let out = check_path_allowed(0, Path::new("/etc/passwd"), &[]);
        assert!(
            matches!(out, Err(ApplierError::PathOutsideAllowedRoots { .. })),
            "empty allowed_roots must REJECT; the skip is the caller's responsibility"
        );
    }

    /// Path under a listed root passes.
    #[test]
    fn check_path_allowed_path_under_allowed_root_passes() {
        let roots = vec![PathBuf::from("/opt/validator")];
        check_path_allowed(0, Path::new("/opt/validator/data/x"), &roots)
            .expect("under-allowed-root path must pass");
    }

    /// Path outside every listed root surfaces
    /// `PathOutsideAllowedRoots` with the full path for
    /// operator-facing diagnostics.
    #[test]
    fn check_path_allowed_outside_root_returns_structured_error() {
        let roots = vec![
            PathBuf::from("/opt/validator"),
            PathBuf::from("/var/lib/validator"),
        ];
        let out = check_path_allowed(42, Path::new("/etc/passwd"), &roots);
        match out {
            Err(ApplierError::PathOutsideAllowedRoots { entry_index, path }) => {
                assert_eq!(entry_index, 42);
                assert_eq!(path, PathBuf::from("/etc/passwd"));
            }
            other => panic!("expected PathOutsideAllowedRoots, got {other:?}"),
        }
    }

    /// `path.starts_with(root)` matches on COMPLETE components —
    /// pinned so a `/opt/validator-staging` path is NOT treated as
    /// under `/opt/validator`.  Load-bearing security property:
    /// without the complete-component check, a validator provisioned
    /// at `/opt/validator` could receive writes to its sibling-dir
    /// `/opt/validator-attacker`.
    #[test]
    fn check_path_allowed_path_prefix_match_is_component_wise() {
        let roots = vec![PathBuf::from("/opt/validator")];
        let out = check_path_allowed(1, Path::new("/opt/validator-attacker/x"), &roots);
        assert!(
            matches!(out, Err(ApplierError::PathOutsideAllowedRoots { .. })),
            "component-wise prefix match MUST reject sibling-with-shared-prefix path"
        );
    }

    /// `pwrite_all` writes the entire payload at the specified
    /// offset, atomically via positioned writes (not via seek-
    /// then-write).  Pin: writing 128 bytes at offset 0 produces
    /// exactly those bytes with no residual zeroes.
    #[test]
    fn pwrite_all_writes_entire_payload_at_offset_zero() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let file = tmp.as_file();
        use std::os::fd::AsRawFd;
        let fd = file.as_raw_fd();

        let payload: Vec<u8> = (0..128).map(|i| i as u8).collect();
        pwrite_all(fd, &payload, 0).expect("pwrite_all ok");

        let back = std::fs::read(tmp.path()).unwrap();
        assert_eq!(back, payload);
    }

    /// `pwrite_all` writes at an offset past end-of-file,
    /// creating a sparse-hole prefix.  Pin: writing N bytes at
    /// offset K produces a file of length K+N with zeros in
    /// `[0, K)` and `payload` in `[K, K+N)`.
    #[test]
    fn pwrite_all_at_offset_creates_sparse_prefix() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        use std::os::fd::AsRawFd;
        let fd = tmp.as_file().as_raw_fd();

        let payload = b"TAIL";
        pwrite_all(fd, payload, 10).expect("pwrite_all ok");

        let back = std::fs::read(tmp.path()).unwrap();
        assert_eq!(back.len(), 14);
        assert_eq!(&back[..10], &[0u8; 10]);
        assert_eq!(&back[10..], payload);
    }

    /// `pwrite_all` with an empty payload is a no-op — the loop
    /// never executes.  Pin so a future "always issue at least
    /// one pwrite" refactor would surface via file growth here.
    #[test]
    fn pwrite_all_empty_payload_is_no_op() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        use std::os::fd::AsRawFd;
        let fd = tmp.as_file().as_raw_fd();

        pwrite_all(fd, &[], 100).expect("pwrite_all ok");

        let back = std::fs::read(tmp.path()).unwrap();
        assert!(back.is_empty(), "empty payload should NOT extend the file");
    }

    /// LOAD-BEARING: `copy_at` reproduces the source's bytes at
    /// the destination.  End-to-end via `safe_descend_verified` on
    /// both sides so this exercises the actual TOCTOU-safe path a
    /// Rename/CopyFile dispatcher branch would use.
    #[test]
    fn copy_at_reproduces_source_bytes_at_destination() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("src.bin"), b"hello world").unwrap();

        let from = safe_descend_verified(tmp.path(), "src.bin", None).unwrap();
        let to = safe_descend_verified(tmp.path(), "dst.bin", None).unwrap();
        copy_at(&from, &to).expect("copy_at ok");

        let back = std::fs::read(tmp.path().join("dst.bin")).unwrap();
        assert_eq!(back, b"hello world");
    }

    /// `copy_at` with an empty source produces an empty
    /// destination — the read loop exits on `n == 0` from the
    /// first `read` call.  Pins the "truncated-to-empty"
    /// behavior.
    #[test]
    fn copy_at_empty_source_produces_empty_destination() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("src.bin"), b"").unwrap();

        let from = safe_descend_verified(tmp.path(), "src.bin", None).unwrap();
        let to = safe_descend_verified(tmp.path(), "dst.bin", None).unwrap();
        copy_at(&from, &to).expect("copy_at ok on empty source");

        let back = std::fs::read(tmp.path().join("dst.bin")).unwrap();
        assert!(back.is_empty());
    }

    /// `copy_at` with a payload larger than the 64 KiB buffer
    /// exercises the multi-iteration read/write loop.  Pin: a
    /// 256 KiB file (4 iterations) round-trips byte-identically.
    #[test]
    fn copy_at_large_file_round_trips_through_multi_iteration_loop() {
        let tmp = tempfile::tempdir().unwrap();
        let big: Vec<u8> = (0..256 * 1024).map(|i| (i * 7 + 13) as u8).collect();
        std::fs::write(tmp.path().join("src.bin"), &big).unwrap();

        let from = safe_descend_verified(tmp.path(), "src.bin", None).unwrap();
        let to = safe_descend_verified(tmp.path(), "dst.bin", None).unwrap();
        copy_at(&from, &to).expect("copy_at ok on large file");

        let back = std::fs::read(tmp.path().join("dst.bin")).unwrap();
        assert_eq!(back.len(), big.len());
        assert_eq!(back, big);
    }

    /// `copy_at` truncates an existing destination before writing
    /// — pins the `O_TRUNC` flag so a longer stale destination
    /// doesn't leave trailing bytes after a shorter overwrite.
    #[test]
    fn copy_at_truncates_existing_destination() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("src.bin"), b"short").unwrap();
        std::fs::write(tmp.path().join("dst.bin"), b"much-longer-stale").unwrap();

        let from = safe_descend_verified(tmp.path(), "src.bin", None).unwrap();
        let to = safe_descend_verified(tmp.path(), "dst.bin", None).unwrap();
        copy_at(&from, &to).expect("copy_at ok");

        let back = std::fs::read(tmp.path().join("dst.bin")).unwrap();
        assert_eq!(back, b"short", "destination truncated to source length");
    }

    /// `openat_leaf` with `O_CREAT | O_RDWR` creates a new file
    /// under a safely-descended parent.  Pin the happy path +
    /// confirm the returned `OwnedFd` is usable for a subsequent
    /// write (via `File::from(owned_fd)` which transfers ownership
    /// without an extra `unsafe`).
    #[test]
    fn openat_leaf_creates_and_returns_usable_fd() {
        let tmp = tempfile::tempdir().unwrap();
        let parent = safe_descend_verified(tmp.path(), "new.bin", None).unwrap();

        let owned_fd = openat_leaf(
            0,
            WalOp::Write,
            &parent,
            Path::new("new.bin"),
            libc::O_RDWR | libc::O_CREAT,
            0o644,
        )
        .expect("openat_leaf ok");

        // `OwnedFd → File` is a safe transfer; File's Drop closes
        // the fd.
        let mut f = std::fs::File::from(owned_fd);
        use std::io::Write;
        f.write_all(b"content").unwrap();
        drop(f);

        let back = std::fs::read(tmp.path().join("new.bin")).unwrap();
        assert_eq!(back, b"content");
    }

    /// `openat_leaf` with a symlink leaf fails cleanly via
    /// `O_NOFOLLOW` — load-bearing S-1 TOCTOU property.  Pin:
    /// an attacker-planted leaf symlink does NOT get followed
    /// into the target; `openat_leaf` returns `IoFailure` with
    /// an `openat:` prefix in the message.
    #[test]
    fn openat_leaf_rejects_leaf_symlink_via_nofollow() {
        let tmp = tempfile::tempdir().unwrap();
        // Create a decoy target the symlink would point at.
        std::fs::write(tmp.path().join("target.bin"), b"attacker").unwrap();
        // Plant a symlink at the leaf position.
        std::os::unix::fs::symlink(tmp.path().join("target.bin"), tmp.path().join("link.bin"))
            .unwrap();
        // Descend to the symlink (descent is a parent operation,
        // so the leaf symlink isn't rejected here — openat_leaf is
        // the layer that rejects it).
        let parent = safe_descend_verified(tmp.path(), "link.bin", None).unwrap();

        let out = openat_leaf(
            7,
            WalOp::Write,
            &parent,
            Path::new("link.bin"),
            libc::O_RDWR,
            0,
        );
        match out {
            Err(ApplierError::IoFailure {
                entry_index,
                op,
                path,
                message,
            }) => {
                assert_eq!(entry_index, 7);
                assert_eq!(op, WalOp::Write);
                assert_eq!(path, PathBuf::from("link.bin"));
                assert!(
                    message.starts_with("openat:"),
                    "message surfaces the openat context: {message}"
                );
            }
            other => panic!("expected IoFailure, got {other:?}"),
        }
    }

    /// `openat_leaf` on a missing path with no `O_CREAT` fails
    /// as `IoFailure` carrying the full WAL-entry context.  Pin
    /// the (entry_index, op, path) propagation so a future
    /// refactor that lost one of those fields surfaces here.
    #[test]
    fn openat_leaf_missing_file_without_create_surfaces_full_context() {
        let tmp = tempfile::tempdir().unwrap();
        let parent = safe_descend_verified(tmp.path(), "ghost.bin", None).unwrap();

        let out = openat_leaf(
            3,
            WalOp::Chmod,
            &parent,
            Path::new("ghost.bin"),
            libc::O_RDONLY,
            0,
        );
        match out {
            Err(ApplierError::IoFailure {
                entry_index,
                op,
                path,
                message: _,
            }) => {
                assert_eq!(entry_index, 3);
                assert_eq!(op, WalOp::Chmod);
                assert_eq!(path, PathBuf::from("ghost.bin"));
            }
            other => panic!("expected IoFailure, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // NSS lookup helpers (slice 3)
    // ---------------------------------------------------------------

    /// `root` resolves to uid 0 on every POSIX system — the one
    /// portable fixture that pins the happy-path lookup
    /// end-to-end across Linux + macOS without a conditional.
    #[test]
    fn resolve_uid_known_name_root_resolves_to_zero() {
        assert_eq!(resolve_uid("root").unwrap(), 0);
    }

    /// A clearly-nonexistent name surfaces [`ApplierError::NssNotFound`]
    /// with the input preserved — operator diagnostic surface.
    /// Using a UUID-like string makes a collision with a real
    /// user ID vanishingly unlikely.
    #[test]
    fn resolve_uid_unknown_name_returns_nss_not_found() {
        let bogus = "nss-ghost-9f3e7d8b-wal-applier-test";
        match resolve_uid(bogus) {
            Err(ApplierError::NssNotFound { name }) => assert_eq!(name, bogus),
            other => panic!("expected NssNotFound, got {other:?}"),
        }
    }

    /// A name containing a NUL byte fails the `CString::new`
    /// pre-check in `nss::resolve_uid_detailed`, surfacing as
    /// [`ApplierError::NssResolutionFailed { errno: EINVAL }`].
    /// Pins the "invalid input" semantic — distinct from "name
    /// not found" per the consolidation refactor.  A future
    /// refactor that routes NUL-containing input through the
    /// syscall (and thus into UB territory) surfaces here.
    #[test]
    fn resolve_uid_name_with_null_byte_returns_resolution_failed_einval() {
        let bad = "root\0injected";
        match resolve_uid(bad) {
            Err(ApplierError::NssResolutionFailed { name, errno }) => {
                assert_eq!(name, bad);
                assert_eq!(errno, libc::EINVAL);
            }
            other => panic!("expected NssResolutionFailed(EINVAL) on NUL, got {other:?}"),
        }
    }

    /// Portable group lookup: `daemon` exists on both Linux and
    /// macOS.  We assert `Ok(_)` rather than pinning the numeric
    /// gid because the value differs across distros (Linux gid=1
    /// on most, macOS gid=1).  The pin is "the lookup machinery
    /// works end-to-end"; the specific number is a system
    /// configuration detail.
    #[test]
    fn resolve_gid_known_name_daemon_resolves() {
        let out = resolve_gid("daemon");
        assert!(
            matches!(out, Ok(_)),
            "`daemon` group lookup should succeed on any standard \
             POSIX system; got {out:?}"
        );
    }

    #[test]
    fn resolve_gid_unknown_name_returns_nss_not_found() {
        let bogus = "nss-ghost-group-9f3e7d8b-wal-applier-test";
        match resolve_gid(bogus) {
            Err(ApplierError::NssNotFound { name }) => assert_eq!(name, bogus),
            other => panic!("expected NssNotFound, got {other:?}"),
        }
    }

    #[test]
    fn resolve_gid_name_with_null_byte_returns_resolution_failed_einval() {
        let bad = "wheel\0injected";
        match resolve_gid(bad) {
            Err(ApplierError::NssResolutionFailed { name, errno }) => {
                assert_eq!(name, bad);
                assert_eq!(errno, libc::EINVAL);
            }
            other => panic!("expected NssResolutionFailed(EINVAL) on NUL, got {other:?}"),
        }
    }

    /// Empty name — `CString::new("")` succeeds (empty CStr is
    /// valid), so the syscall runs with an empty C string.  POSIX
    /// `getpwnam_r` returns success with a null `result_ptr` for
    /// an empty name (no matching entry), surfacing as
    /// [`ApplierError::NssNotFound`].  Pins the "empty name →
    /// clean error, not panic" path.
    #[test]
    fn resolve_uid_empty_name_returns_nss_not_found() {
        match resolve_uid("") {
            Err(ApplierError::NssNotFound { name }) => assert!(name.is_empty()),
            other => panic!("expected NssNotFound on empty, got {other:?}"),
        }
    }

    /// Symmetric with the uid test — pin that gid lookup also
    /// routes empty-name through the "clean error, not panic"
    /// path.  Code paths are shared (both adapt over
    /// `nss::*_detailed`), but the symmetric pin catches a
    /// future refactor that diverged just one of the two
    /// adapters.
    #[test]
    fn resolve_gid_empty_name_returns_nss_not_found() {
        match resolve_gid("") {
            Err(ApplierError::NssNotFound { name }) => assert!(name.is_empty()),
            other => panic!("expected NssNotFound on empty, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // apply_wal_to_fresh_tree dispatcher (slice 4 — capstone)
    // ---------------------------------------------------------------

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
    /// WAL entry's canonical host path.  LOAD-BEARING end-to-end
    /// pin: synthetic Write entry, identity path_map, verify the
    /// payload lands at the expected offset.
    #[test]
    fn identity_path_map_writes_at_wal_entry_path() {
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("target.bin");
        std::fs::write(&dst, vec![0u8; 8]).unwrap();

        let payload = b"data".to_vec();
        let (entry, h) = write_entry_at(&dst, 2, &payload);
        let mut sidecar: HashMap<[u8; 32], Vec<u8>> = HashMap::new();
        sidecar.insert(h, payload.clone());

        apply_wal_to_fresh_tree(&[entry], &sidecar, ResolvedWalPath::identity_leaf_split, &[
        ])
        .unwrap();

        let got = std::fs::read(&dst).unwrap();
        assert_eq!(&got[..2], &[0, 0]);
        assert_eq!(&got[2..2 + payload.len()], payload.as_slice());
    }

    /// H-6: `Failure`-outcome entries are skipped even when their
    /// sidecar bytes are missing — the applier must not attempt a
    /// write the leader never performed.
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
            ResolvedWalPath::identity_leaf_split,
            &[],
        )
        .unwrap();

        // Byte state unchanged.
        assert_eq!(std::fs::read(&dst).unwrap(), vec![0xAA; 8]);
    }

    /// The caller-supplied `path_map` closure redirects writes: a
    /// closure that translates `src_root/f.bin` → `dst_root/f.bin`
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

    /// Missing sidecar entry returns `MissingSidecarEntry` rather
    /// than panicking — a pre-hardening panic would kill the boot
    /// subscriber task.
    #[test]
    fn missing_sidecar_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("t.bin");
        std::fs::write(&dst, vec![0u8; 8]).unwrap();
        let payload = b"missing".to_vec();
        let (entry, h) = write_entry_at(&dst, 0, &payload);
        let sidecar: HashMap<[u8; 32], Vec<u8>> = HashMap::new();
        let err =
            apply_wal_to_fresh_tree(&[entry], &sidecar, ResolvedWalPath::identity_leaf_split, &[
            ])
            .expect_err("missing sidecar must Err");
        assert_eq!(err, ApplierError::MissingSidecarEntry {
            entry_index: 0,
            hash_hex: hex::encode(h),
        });
        assert_eq!(std::fs::read(&dst).unwrap(), vec![0u8; 8]);
    }

    /// A `DeployRef` payload_ref is well-formed but not-yet-
    /// reproducible.  Dispatcher surfaces `UnsupportedPayloadRef`.
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
            ResolvedWalPath::identity_leaf_split,
            &[],
        )
        .expect_err("DeployRef must Err");
        assert_eq!(err, ApplierError::UnsupportedPayloadRef { entry_index: 0 });
    }

    /// A path outside every `allowed_roots` entry returns
    /// `PathOutsideAllowedRoots` — defense-in-depth against a
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
            ResolvedWalPath::identity_leaf_split,
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

    /// Empty `allowed_roots` disables validation at the dispatcher
    /// level — the `check_path_allowed` call is skipped by
    /// `descend_entry`'s `if !allowed_roots.is_empty()` gate.
    /// LOAD-BEARING: pins the integration between the gate and the
    /// pure function (which per PR #521 rejects empty inputs; the
    /// skip is caller-driven).
    #[test]
    fn empty_allowed_roots_skips_validation() {
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("t.bin");
        std::fs::write(&dst, vec![0u8; 8]).unwrap();
        let payload = b"ok".to_vec();
        let (entry, h) = write_entry_at(&dst, 0, &payload);
        let mut sidecar: HashMap<[u8; 32], Vec<u8>> = HashMap::new();
        sidecar.insert(h, payload.clone());
        apply_wal_to_fresh_tree(&[entry], &sidecar, ResolvedWalPath::identity_leaf_split, &[
        ])
        .unwrap();
        let got = std::fs::read(&dst).unwrap();
        assert_eq!(&got[..2], payload.as_slice());
    }

    /// A NULL byte inside a Chown path is caught by
    /// `safe_descend_verified`'s `to_c` step and surfaces as
    /// `SafeDescendFailed`.  Chown resolves NSS names BEFORE
    /// descent, so we use an empty owner/group pair (short-
    /// circuits NSS) to exercise the descent-side NULL check.
    #[test]
    fn chown_path_with_null_byte_returns_safe_descend_failed() {
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
            ResolvedWalPath::identity_leaf_split,
            &[],
        )
        .expect_err("path with NULL must Err");
        assert!(
            matches!(err, ApplierError::SafeDescendFailed { entry_index: 0, .. }),
            "got {err:?}"
        );
    }

    /// Empty owner + empty group short-circuits to
    /// `(u32::MAX, u32::MAX)` sentinels (POSIX "leave uid/gid
    /// unchanged").  No NSS lookup fires, so even hosts without
    /// NSS entries for these names succeed.
    #[test]
    fn chown_empty_owner_and_group_short_circuits_nss() {
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("chownable.bin");
        std::fs::write(&dst, vec![0u8; 4]).unwrap();
        let entry = WalEntry {
            op: WalOp::Chown,
            path: dst,
            extra_path: None,
            offset: None,
            length: None,
            payload_ref: None,
            mode_bits: None,
            owner: Some(String::new()),
            group: Some(String::new()),
            outcome: WalOutcome::Success,
        };
        apply_wal_to_fresh_tree(
            &[entry],
            &HashMap::new(),
            ResolvedWalPath::identity_leaf_split,
            &[],
        )
        .unwrap();
    }

    /// Chown with a nonexistent owner name surfaces `NssNotFound`
    /// (not a panic).  Pin against the reentrant `getpwnam_r`
    /// path via the `nss::resolve_uid_detailed` adapter.
    #[test]
    fn chown_nonexistent_owner_returns_nss_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("chownable.bin");
        std::fs::write(&dst, vec![0u8; 4]).unwrap();
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
            ResolvedWalPath::identity_leaf_split,
            &[],
        )
        .expect_err("nonexistent owner must Err");
        assert!(
            matches!(err, ApplierError::NssNotFound { .. }),
            "got {err:?}"
        );
    }

    /// A Rename entry missing `extra_path` returns
    /// `MissingExtraPath` — invariant violation surfaced rather
    /// than panicking.
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
            ResolvedWalPath::identity_leaf_split,
            &[],
        )
        .expect_err("missing extra_path must Err");
        assert_eq!(err, ApplierError::MissingExtraPath {
            entry_index: 0,
            op: WalOp::Rename,
        });
    }

    /// Truncate without offset returns `MissingOffset`.
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
            ResolvedWalPath::identity_leaf_split,
            &[],
        )
        .expect_err("missing offset on Truncate must Err");
        assert_eq!(err, ApplierError::MissingOffset {
            entry_index: 0,
            op: WalOp::Truncate,
        });
    }

    /// Truncate on a nonexistent file surfaces `IoFailure` (via
    /// the `openat_leaf` ENOENT path), not a panic.
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
            ResolvedWalPath::identity_leaf_split,
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

    /// LOAD-BEARING S-1 TOCTOU pin: a symlink component along the
    /// on-disk path is rejected by `safe_descend_verified`'s
    /// `openat(O_NOFOLLOW)` step, surfacing as `SafeDescendFailed`
    /// rather than silently traversing into the symlink target.
    #[test]
    fn symlink_intermediate_component_returns_safe_descend_failed() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let evil = tempfile::tempdir().unwrap();
        // /root/sub is a symlink to /evil (the attacker's tree).
        symlink(evil.path(), root.path().join("sub")).unwrap();
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

    /// LOAD-BEARING: full-flow H-5 rename-and-recreate attack
    /// detection.  Capture the real root identity, perform the
    /// attack between capture and apply, verify the applier
    /// surfaces `SafeDescendFailed` AND that the attacker-seeded
    /// bytes stay unmutated.  Uses my branch's `Root::capture +
    /// identity()` API rather than fileio's free-function
    /// `capture_root_identity`.
    #[test]
    fn wal_applier_identity_check_rejects_renamed_root() {
        use super::super::path::identity::Root;

        let staging = tempfile::tempdir().unwrap();
        let root_path = staging.path().join("legit-root");
        std::fs::create_dir(&root_path).unwrap();
        std::fs::write(root_path.join("target.bin"), vec![0u8; 8]).unwrap();

        // Boot: capture the real identity of the legit root.
        let boot_id = Root::capture(&root_path)
            .expect("boot-time capture ok")
            .identity();

        // Sanity: the applier accepts the entry BEFORE the attack.
        let payload = b"good".to_vec();
        let (entry, h) = write_entry_at(&root_path.join("target.bin"), 0, &payload);
        let mut sidecar: HashMap<[u8; 32], Vec<u8>> = HashMap::new();
        sidecar.insert(h, payload.clone());
        let root_pb = root_path.clone();
        apply_wal_to_fresh_tree(
            &[entry.clone()],
            &sidecar,
            |_p| ResolvedWalPath {
                root: root_pb.clone(),
                rel: PathBuf::from("target.bin"),
                expected_root_id: Some(boot_id),
            },
            &[],
        )
        .expect("pre-attack apply must succeed against the legit root");
        // O_WRONLY | O_CREAT (not TRUNC) → tail of the 8-byte seed
        // remains after the 4-byte payload.
        let contents_after = std::fs::read(root_path.join("target.bin")).unwrap();
        assert_eq!(&contents_after[..4], b"good");

        // Attack: rename the legit root aside, then recreate a
        // fresh dir with the same name + attacker content.
        let sidelined = staging.path().join("legit-root.bak");
        std::fs::rename(&root_path, &sidelined).unwrap();
        std::fs::create_dir(&root_path).unwrap();
        std::fs::write(root_path.join("target.bin"), b"attacker-seed").unwrap();

        // Apply again with the ORIGINAL boot_id.  The applier
        // MUST reject at safe_descend_verified with the identity
        // mismatch — attacker-seeded content stays untouched.
        let attack_payload = b"attacker-overwrite".to_vec();
        let (attack_entry, attack_h) =
            write_entry_at(&root_path.join("target.bin"), 0, &attack_payload);
        sidecar.insert(attack_h, attack_payload);
        let err = apply_wal_to_fresh_tree(
            &[attack_entry],
            &sidecar,
            |_p| ResolvedWalPath {
                root: root_pb.clone(),
                rel: PathBuf::from("target.bin"),
                expected_root_id: Some(boot_id),
            },
            &[],
        )
        .expect_err("post-rename apply MUST reject");
        assert!(
            matches!(err, ApplierError::SafeDescendFailed { entry_index: 0, .. }),
            "post-rename apply must surface as SafeDescendFailed; got {err:?}"
        );
        assert_eq!(
            std::fs::read(root_path.join("target.bin")).unwrap(),
            b"attacker-seed".to_vec(),
            "applier must not mutate a swapped root"
        );
    }

    /// Mismatched `expected_root_id` (synthetic
    /// `Some((u64::MAX, u64::MAX))`) is rejected by
    /// `safe_descend_verified` and surfaces as
    /// `SafeDescendFailed`.  Companion to the rename-and-recreate
    /// test above — this one forces the mismatch synthetically
    /// (no attack staging needed).
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
        let err = apply_wal_to_fresh_tree(
            &[entry],
            &sidecar,
            |_p| ResolvedWalPath {
                root: root_pb.clone(),
                rel: PathBuf::from("t.bin"),
                expected_root_id: Some((u64::MAX, u64::MAX)),
            },
            &[],
        )
        .expect_err("mismatched root identity must Err");
        assert!(
            matches!(err, ApplierError::SafeDescendFailed { entry_index: 0, .. }),
            "got {err:?}"
        );
        assert_eq!(std::fs::read(&dst).unwrap(), vec![0u8; 8]);
    }
}
