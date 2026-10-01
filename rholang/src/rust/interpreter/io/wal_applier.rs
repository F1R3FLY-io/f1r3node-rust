// WAL fresh-tree applier — foundation types (PR #519) + pure
// syscall helpers (PR #521) + NSS lookup helpers (this PR,
// slice 3 of the wal_applier submodule tree).
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
// This slice (PR #522) adds the NSS lookup helpers used by
// the Chown branch: `resolve_uid` (name → uid) and
// `resolve_gid` (name → gid).  Both are thin adapters over
// the libc FFI machinery in [`super::nss`]
// ([`resolve_uid_detailed`] / [`resolve_gid_detailed`]) — the
// shared machinery lives in `nss.rs` so future NSS tweaks
// (ERANGE discipline, buffer ceiling, additional errno
// handling) touch one site.  The adapter's job is just to
// map the shared `Result<Option<u32>, i32>` surface onto the
// applier's structured [`ApplierError`] variants.
//
// Subsequent slices add the main [`apply_wal_to_fresh_tree`]
// dispatcher.
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

use std::path::{Path, PathBuf};

use super::path::{QuarantineError, SafeParent};
use super::wal::WalOp;

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
// `#[allow(dead_code)]` applies to each helper individually
// because the sole caller (the dispatcher) lands in a yet-to-
// land slice.  Removing the allow once the dispatcher is in
// place will catch any orphaned helper that nothing wires up.
// Tests in this file DO exercise each helper, so test builds
// don't need the allow (cfg-test visibility suffices).

/// Render a [`QuarantineError`] into a short operator-facing
/// string suitable for the `reason` field of
/// [`ApplierError::SafeDescendFailed`].  Keeps the shape stable
/// across platforms (`std::io::Error` renders include errno text
/// which varies between Linux and macOS; the `(kind, msg)` tuple
/// in `IoError` is already scrubbed by `io_msg_scrub` at the
/// `path` module boundary).
#[allow(dead_code)]
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
/// returning the raw fd on success or [`ApplierError::IoFailure`]
/// with the full WAL-entry context on failure.  `O_NOFOLLOW` is
/// unconditionally ORed into `flags` to preserve the S-1 TOCTOU
/// discipline — a leaf-level symlink is rejected here rather
/// than followed into whatever the attacker populated.
///
/// Callers own the returned fd and MUST close it (typically via
/// `std::fs::File::from_raw_fd` + Drop or an explicit
/// `libc::close`) before returning.
#[allow(dead_code)]
pub(super) fn openat_leaf(
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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

    use std::os::fd::FromRawFd;

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
    /// confirm the returned fd is usable for a subsequent write.
    #[test]
    fn openat_leaf_creates_and_returns_usable_fd() {
        let tmp = tempfile::tempdir().unwrap();
        let parent = safe_descend_verified(tmp.path(), "new.bin", None).unwrap();

        let fd = openat_leaf(
            0,
            WalOp::Write,
            &parent,
            Path::new("new.bin"),
            libc::O_RDWR | libc::O_CREAT,
            0o644,
        )
        .expect("openat_leaf ok");

        // Wrap in File so Drop closes the fd; also tests the fd is
        // actually writable.
        // SAFETY: `openat_leaf` returned a fresh fd we own; wrapping
        // in `File::from_raw_fd` transfers ownership so Drop closes it.
        let mut f = unsafe { std::fs::File::from_raw_fd(fd) };
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
}
