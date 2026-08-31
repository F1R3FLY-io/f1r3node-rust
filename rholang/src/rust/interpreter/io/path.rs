// Safe path descent (TOCTOU-hardened).
//
// The prior version walked components with `symlink_metadata` checks and
// then let the handler layer pass a resolved `PathBuf` to `std::fs::*`.
// Between the check and the syscall an attacker with write access to any
// subdirectory could `rename(subdir, .tmp) && symlink(/, subdir)` and
// escape the root — the "check-then-syscall" window the plan explicitly
// says to collapse.
//
// This version instead opens `root` as a dirfd and descends via
// `openat(dirfd, name, O_NOFOLLOW|O_DIRECTORY|O_RDONLY|O_CLOEXEC)` at
// every step.  Any symlink component (or race that inserts one)
// short-circuits with `ELOOP`, translated to `QuarantineError::
// SymlinkComponent`.  The returned `SafeParent` holds a dirfd to the
// leaf's parent plus the leaf's `CString` name; handlers then use `*at`
// syscalls (`fstatat`, `fchmodat`, `unlinkat`, `renameat`, `openat`)
// against that dirfd, so the resolution path used at check time is the
// same one used at operation time — TOCTOU-immune.
//
// macOS + Linux (per FIP scope).  All syscalls used are POSIX; no
// dependence on Linux-only `openat2`.

use std::ffi::{CString, OsStr};
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path};

#[derive(Debug, PartialEq, Eq)]
pub enum QuarantineError {
    Empty,
    RootSelf,
    EscapesRoot,
    SymlinkComponent,
    /// H-5 fix (2026-08-06): the root directory this operator
    /// provisioned at boot has been replaced by a directory with
    /// a different (dev, inode) pair — the classic
    /// rename-and-recreate attack that H-P7-6's `O_NOFOLLOW`
    /// explicitly does NOT close.  A boot-registered root
    /// identity (`RootIdentityRegistry` populated in
    /// `node/setup.rs::create_casper_infrastructure`) is
    /// consulted at every `safe_descend`; mismatch surfaces this
    /// error instead of silently descending into the attacker's
    /// tree.  Operator diagnostic: the root path resolves but
    /// its underlying inode no longer matches boot — check for
    /// out-of-band `mv` / recreate / rebind on the provisioned
    /// path.
    RootIdentityChanged,
    /// `IoError(kind, scrubbed_msg)` — the scrubbed
    /// (`io_msg_scrub`) display and the classifier that lets
    /// `quarantine_err_reply` route AlreadyExists to
    /// `FSERR_ALREADY_EXISTS`, NotFound to `FSERR_NOT_FOUND`, etc.
    /// (via `io_err_code`).  Pre-slice-10b this variant carried
    /// only the string, collapsing every syscall failure to
    /// `FSERR_IO` and stripping a caller's ability to do
    /// create-if-not-exists via `wx` mode.  Callers without a
    /// natural `io::ErrorKind` (e.g., `CString::NulError`) may
    /// synthesize `io::ErrorKind::Other`, which `io_err_code`
    /// maps back to `FSERR_IO` — preserving legacy behavior for
    /// those sites while unlocking specific codes for real
    /// syscall errors.
    IoError(io::ErrorKind, String),
}

/// A safely-resolved leaf position: a dirfd for the parent directory and
/// the leaf's name (as a `CString` ready for `*at` syscalls).
pub struct SafeParent {
    pub dirfd: OwnedFd,
    pub leaf: CString,
}

impl SafeParent {
    pub fn as_raw_fd(&self) -> i32 { self.dirfd.as_raw_fd() }
    pub fn leaf_ptr(&self) -> *const libc::c_char { self.leaf.as_ptr() }
}

/// Descend from `root` along `rel` without following any symlink, and
/// return a handle to the leaf's parent + the leaf's basename.
///
/// The returned `SafeParent.dirfd` is a fresh file descriptor owned by
/// the caller.  Every subsequent syscall MUST be issued via `*at`
/// against this dirfd (never via a rebuilt path string) or the
/// TOCTOU-immunity is lost.
pub fn safe_descend(root: &Path, rel: &str) -> Result<SafeParent, QuarantineError> {
    safe_descend_verified(root, rel, None)
}

/// H-5 fix (2026-08-06): variant of `safe_descend` that verifies the
/// opened root's `(dev, inode)` pair against a boot-captured
/// expected value.  Closes the rename-and-recreate attack that
/// H-P7-6's `O_NOFOLLOW` explicitly does not: an attacker with
/// write access to the root's parent (`$HOME/data`, for a node
/// running as an unprivileged user) can `mv /legit /legit.bak &&
/// mkdir /legit && populate` — the new `/legit` is a real
/// directory with a different inode.  `O_NOFOLLOW` allows opening
/// real dirs, so the pre-fix descent lands in the attacker's tree
/// and every subsequent syscall reads/writes attacker files
/// silently.
///
/// Post-fix: with an `expected_root_id: Some((dev, inode))` in
/// hand (boot-captured via `capture_root_identity`), a mismatch
/// after opening surfaces `QuarantineError::RootIdentityChanged`.
/// Callers pass `None` (or invoke `safe_descend`) if identity
/// verification is not applicable — e.g., internal helpers that
/// operate on already-verified fds, or code paths that don't
/// have a boot-registered root.
///
/// Explicitly deferred: the (dev, inode) pair captured at boot
/// assumes the filesystem preserves it across reboots (true for
/// standard local FSes: ext4/xfs/apfs/etc.).  On some networked
/// or in-memory FSes inode numbers may not be stable across
/// remounts — for those, operators should compose with the
/// documented read-only bind-mount mitigation.
pub fn safe_descend_verified(
    root: &Path,
    rel: &str,
    expected_root_id: Option<(u64, u64)>,
) -> Result<SafeParent, QuarantineError> {
    if rel.is_empty() {
        return Err(QuarantineError::Empty);
    }
    let rel_path = Path::new(rel);
    if rel_path.is_absolute() {
        return Err(QuarantineError::EscapesRoot);
    }

    // Collect Normal components; reject `..`; skip `.`.
    let mut comps: Vec<&OsStr> = Vec::new();
    for c in rel_path.components() {
        match c {
            Component::CurDir => continue,
            Component::ParentDir => return Err(QuarantineError::EscapesRoot),
            Component::Normal(n) => comps.push(n),
            Component::RootDir | Component::Prefix(_) => {
                return Err(QuarantineError::EscapesRoot);
            }
        }
    }
    if comps.is_empty() {
        return Err(QuarantineError::RootSelf);
    }

    // Open the root itself.  Slice 30c H-P7-6 fix: set O_NOFOLLOW
    // on the root open so a post-boot symlink-swap attack fails
    // cleanly.
    //
    // Attack it closes: attacker replaces the boot-canonicalized
    // root path with a symlink post-boot, e.g., after the
    // provisioning validator has recorded the canonical path but
    // before or between syscalls.  Without O_NOFOLLOW, subsequent
    // `open_dir(root, false)` would follow the symlink into an
    // attacker-controlled tree.  With O_NOFOLLOW, the open fails
    // with ELOOP (Linux) or ENOTDIR (macOS) which maps to
    // `QuarantineError::SymlinkComponent`.
    //
    // Consistency with boot invariant: the provisioning validator
    // (node/src/rust/configuration/boot_validation.rs) rejects any
    // provisioned path that is a symlink at boot, so a legitimately
    // provisioned root can never be a symlink.  Enforcing
    // O_NOFOLLOW on the root open just extends that invariant to
    // every subsequent syscall for the lifetime of the process.
    //
    // What this DOESN'T close: rename-and-recreate.  If an attacker
    // does `mv /legit /legit.bak && mkdir /legit && populate` with
    // attacker files, the new `/legit` is a real directory with a
    // different inode.  O_NOFOLLOW allows opening real directories,
    // so this attack succeeds.  A full defense requires the
    // provisioning layer to record the boot-time `(dev, inode)`
    // pair and this function to verify via `fstat` after open —
    // documented in the H-P7-6 follow-up.  Practical mitigation
    // today: operators should mount `consensus-static-*` /
    // `oracle-static-*` roots on filesystems the node user does
    // not have write access to (e.g., read-only bind mounts).
    let cur = open_dir(root, true)?;

    // H-5 fix (2026-08-06): after opening the root, verify its
    // (dev, inode) matches the boot-captured expected pair.
    // Detects rename-and-recreate: `mv /legit /legit.bak && mkdir
    // /legit && populate` produces a new /legit with a DIFFERENT
    // inode.  O_NOFOLLOW (H-P7-6) allows opening real dirs, so
    // the descent would silently land in the attacker's tree
    // without this check.
    if let Some(expected) = expected_root_id {
        let actual = fstat_dev_inode(cur.as_raw_fd())?;
        if actual != expected {
            return Err(QuarantineError::RootIdentityChanged);
        }
    }

    let mut cur = cur;
    let leaf_name = comps.last().unwrap();
    for intermediate in &comps[..comps.len() - 1] {
        let name = to_c(intermediate)?;
        cur = openat_dir(&cur, name.as_ptr(), true)?;
    }
    let leaf = to_c(leaf_name)?;

    Ok(SafeParent { dirfd: cur, leaf })
}

/// H-5 fix (2026-08-06): capture a directory's identity as a
/// `(dev, inode)` pair via `stat(2)`.  Called at boot from
/// `node::setup::create_casper_infrastructure` for each
/// operator-provisioned root path; the resulting pair is stored
/// in the shared `RootIdentityRegistry` and consumed on every
/// `safe_descend_verified` call.
///
/// Returns `io::Error` on stat failure (e.g., permission denied
/// on the parent, path vanished between boot-validate and
/// registry-populate).  Caller decides whether to skip
/// registration or fail boot on error.
pub fn capture_root_identity(root: &Path) -> std::io::Result<(u64, u64)> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let md = std::fs::metadata(root)?;
        Ok((md.dev(), md.ino()))
    }
    #[cfg(not(unix))]
    {
        // Windows: no dev/ino equivalent; return (0, 0) sentinel
        // and log-warn.  H-5's attack model presupposes POSIX
        // rename-and-recreate; the Windows equivalent (moving a
        // directory across filesystems) is a different threat
        // surface.
        tracing::warn!(
            target: "f1r3fly.fs_wal.safe_descend",
            "capture_root_identity: (dev, inode) unavailable on non-Unix; \
             root identity verification is a no-op"
        );
        Ok((0, 0))
    }
}

/// H-5 fix (2026-08-06): fstat a raw fd for its `(dev, inode)`
/// pair.  Used by `safe_descend_verified` post-open to compare
/// against the boot-captured expected value.  Independent of the
/// `stat`-by-path helper (`capture_root_identity`) so we compare
/// what we just OPENED, not a subsequent by-path lookup that
/// could resolve to a different inode under an active attack.
///
/// Also used by Phase 8 slice 8a's `fs_lock_range` /
/// `fs_lock_sequential` handlers to key the `LockRegistry` on the
/// physical file identity of the caller's fd (post-review-2 fix:
/// keys on `fstat(fd).(dev, ino)` so cross-cap coordination is
/// oracular-correct even when the underlying path is remapped
/// externally).
pub fn fstat_dev_inode(fd: i32) -> Result<(u64, u64), QuarantineError> {
    #[cfg(unix)]
    {
        unsafe {
            let mut st: libc::stat = std::mem::zeroed();
            if libc::fstat(fd, &mut st) < 0 {
                let e = std::io::Error::last_os_error();
                return Err(QuarantineError::IoError(e.kind(), e.to_string()));
            }
            // st.st_dev / st.st_ino widths differ across platforms
            // (u32 vs u64).  Widen uniformly to u64 for comparison.
            #[allow(clippy::unnecessary_cast)]
            Ok((st.st_dev as u64, st.st_ino as u64))
        }
    }
    #[cfg(not(unix))]
    {
        Ok((0, 0))
    }
}

/// H-5 fix (2026-08-06): shared registry mapping each
/// operator-provisioned root path to its boot-captured
/// `(dev, inode)` identity.  Populated once at boot; consulted
/// on every `safe_descend_verified` to detect
/// rename-and-recreate attacks post-boot.
///
/// Thread-safe: reads are frequent (every syscall), writes
/// happen once at boot.  `RwLock` gives us contention-free
/// concurrent reads.
///
/// # Shape A extension (2026-08-31)
///
/// Each entry now carries an `on_disk_root` in addition to the
/// identity pair.  For the historical "logical == on_disk" case
/// (every registration prior to Shape A) the field is a copy of
/// the registration key, so `register(canon, id)` continues to
/// mean "the caller's `canonRoot` string IS the on-disk absolute
/// path".  Callers can also `register_with_remap(logical,
/// on_disk, id)` to record a logical→on-disk remap — used under
/// D3 to give each validator its own subdirectory copy of a
/// Consensus-cap file while keeping the bundle-baked `canonRoot`
/// string identical across validators (a requirement for
/// genesis-block hash consensus).
///
/// Handlers should call `resolve(logical)` — returns the
/// on-disk absolute + identity, or `None` for unregistered
/// logical roots (fall-through: caller uses the logical path
/// as the on-disk path, matching pre-Shape-A behavior).  The
/// legacy `get(root)` method is kept for callers that only need
/// the identity by-on-disk-path (rare).
///
/// The map is keyed by the logical root as the Rholang side
/// sees it (`canonRoot` from Fs.rho's `bMap` — which under
/// Shape A becomes bundle-relative in the composed source but
/// remains identity-registered for legacy callers).
#[derive(Debug, Clone, Default)]
pub struct RootIdentityRegistry {
    inner: std::sync::Arc<std::sync::RwLock<RegistryInner>>,
}

/// Per-registration record: the on-disk absolute root the
/// handler should hand to `safe_descend_verified`, plus the
/// (dev, inode) identity captured at boot for the H-5
/// rename-and-recreate check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredRoot {
    pub on_disk_root: std::path::PathBuf,
    pub identity: (u64, u64),
}

#[derive(Debug, Default)]
struct RegistryInner {
    entries: std::collections::HashMap<std::path::PathBuf, RegisteredRoot>,
}

impl RootIdentityRegistry {
    pub fn new() -> Self { Self::default() }

    /// Legacy registration: the caller's `canonRoot` IS the
    /// on-disk absolute path (logical == on_disk).  Idempotent
    /// with respect to the value; a repeat register with a
    /// different identity overwrites (last-write-wins), which
    /// should not happen in practice since boot populates once.
    pub fn register(&self, root: std::path::PathBuf, id: (u64, u64)) {
        self.register_with_remap(root.clone(), root, id);
    }

    /// Shape A registration: the Rholang-side logical root
    /// (`logical`) may differ from the on-disk absolute root
    /// (`on_disk`).  Used by per-validator harness setup so
    /// each validator can hold a distinct on-disk copy of a
    /// Consensus-cap file while both validators' bundles agree
    /// on the logical name.  For legacy callers, `logical ==
    /// on_disk` and this collapses to identity behavior.
    pub fn register_with_remap(
        &self,
        logical: std::path::PathBuf,
        on_disk: std::path::PathBuf,
        id: (u64, u64),
    ) {
        let mut guard = self
            .inner
            .write()
            .expect("root-identity registry poisoned");
        guard.entries.insert(
            logical,
            RegisteredRoot {
                on_disk_root: on_disk,
                identity: id,
            },
        );
    }

    /// Look up a root's expected identity by ON-DISK path.
    /// Kept for backward compat with pre-Shape-A callers that
    /// already have the on-disk absolute in hand (e.g., the
    /// legacy `.get(&root_pb)` pattern in handlers where
    /// `root_pb` is the Rholang-side `canonRoot`).  Prefer
    /// `resolve` for new code: it returns the on-disk path AND
    /// the identity, which is what every handler needs.
    ///
    /// Behaviorally identical to pre-Shape-A `get` in the
    /// logical == on_disk case (every current registration).
    pub fn get(&self, root: &std::path::Path) -> Option<(u64, u64)> {
        let guard = self
            .inner
            .read()
            .expect("root-identity registry poisoned");
        guard.entries.get(root).map(|r| r.identity)
    }

    /// Shape A lookup: given a logical root (the string
    /// Rholang code hands us as `canonRoot`), return the
    /// on-disk absolute path the handler should syscall
    /// against + the boot-captured identity.  Returns `None`
    /// for unregistered logical roots — callers fall through to
    /// treating the logical path as the on-disk path (the
    /// pre-Shape-A behavior).
    pub fn resolve(&self, logical: &std::path::Path) -> Option<RegisteredRoot> {
        let guard = self
            .inner
            .read()
            .expect("root-identity registry poisoned");
        guard.entries.get(logical).cloned()
    }

    /// Convenience wrapper for the handler pattern: returns
    /// `(on_disk_root, expected_root_id)` such that the handler
    /// can pass `on_disk_root` to `safe_descend_verified` and
    /// `expected_root_id` as the identity argument.  Falls
    /// through to `(logical.to_owned(), None)` for unregistered
    /// logical roots — matches pre-Shape-A behavior exactly.
    pub fn resolve_or_identity(
        &self,
        logical: &std::path::Path,
    ) -> (std::path::PathBuf, Option<(u64, u64)>) {
        match self.resolve(logical) {
            Some(r) => (r.on_disk_root, Some(r.identity)),
            None => (logical.to_path_buf(), None),
        }
    }

    /// Count of registered roots.  For diagnostics only.
    pub fn len(&self) -> usize {
        let guard = self
            .inner
            .read()
            .expect("root-identity registry poisoned");
        guard.entries.len()
    }

    pub fn is_empty(&self) -> bool { self.len() == 0 }
}

/// Descend to the leaf itself as a fresh `File` handle, using the caller-
/// supplied `flags` (masked by `O_NOFOLLOW|O_CLOEXEC` automatically).
///
/// `flags` is the raw open(2) flags — e.g. `O_RDONLY`, `O_WRONLY|O_CREAT`,
/// etc.  `mode` is the file-creation permission for `O_CREAT`; ignored
/// otherwise.  Returns the descended `File` and its dirfd's canonical
/// display path (for error messages / stat records; the dirfd itself is
/// what matters for correctness).
pub fn safe_open(
    root: &Path,
    rel: &str,
    flags: libc::c_int,
    mode: libc::mode_t,
) -> Result<File, QuarantineError> {
    let parent = safe_descend(root, rel)?;
    unsafe {
        let full_flags = flags | libc::O_NOFOLLOW | libc::O_CLOEXEC;
        let fd = libc::openat(
            parent.as_raw_fd(),
            parent.leaf_ptr(),
            full_flags,
            mode as libc::c_uint,
        );
        if fd < 0 {
            let e = std::io::Error::last_os_error();
            return Err(map_open_err(e));
        }
        Ok(File::from_raw_fd(fd))
    }
}

fn open_dir(path: &Path, nofollow: bool) -> Result<OwnedFd, QuarantineError> {
    let cpath = CString::new(path.as_os_str().as_bytes())
        .map_err(|e| QuarantineError::IoError(io::ErrorKind::InvalidInput, e.to_string()))?;
    unsafe {
        let mut flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC;
        if nofollow {
            flags |= libc::O_NOFOLLOW;
        }
        let fd = libc::open(cpath.as_ptr(), flags);
        if fd < 0 {
            let e = std::io::Error::last_os_error();
            // Slice 30c H-P7-6 fix: mirror `openat_dir`'s ELOOP-vs-
            // ENOTDIR disambiguation on the root open.  macOS returns
            // ENOTDIR when `open(O_DIRECTORY|O_NOFOLLOW)` hits a
            // symlink (the symlink itself isn't a directory); Linux
            // returns ELOOP.  Both mean the same thing at the safety
            // layer: the requested path is a symlink and we refused
            // to follow it.  Surface as `SymlinkComponent` for a
            // consistent operator-facing error regardless of host OS.
            let raw = e.raw_os_error();
            if raw == Some(libc::ELOOP)
                || (nofollow && raw == Some(libc::ENOTDIR) && is_symlink_at_path(&cpath))
            {
                return Err(QuarantineError::SymlinkComponent);
            }
            return Err(map_open_err(e));
        }
        Ok(OwnedFd::from_raw_fd(fd))
    }
}

/// H-P7-6 helper: absolute-path variant of `is_symlink_at`.  Used
/// on the ENOTDIR fallback path in `open_dir` to distinguish
/// symlink-swap-attack from a genuine "expected a directory, got
/// a regular file" error.
unsafe fn is_symlink_at_path(cpath: &CString) -> bool {
    let mut sb: libc::stat = std::mem::zeroed();
    // AT_FDCWD + AT_SYMLINK_NOFOLLOW = lstat semantics.
    if libc::fstatat(
        libc::AT_FDCWD,
        cpath.as_ptr(),
        &mut sb,
        libc::AT_SYMLINK_NOFOLLOW,
    ) != 0
    {
        return false;
    }
    (sb.st_mode & libc::S_IFMT) == libc::S_IFLNK
}

fn openat_dir(
    parent: &OwnedFd,
    name: *const libc::c_char,
    nofollow: bool,
) -> Result<OwnedFd, QuarantineError> {
    unsafe {
        let mut flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC;
        if nofollow {
            flags |= libc::O_NOFOLLOW;
        }
        let fd = libc::openat(parent.as_raw_fd(), name, flags);
        if fd < 0 {
            let e = std::io::Error::last_os_error();
            // On macOS, `openat(O_DIRECTORY|O_NOFOLLOW)` against a symlink
            // yields `ENOTDIR` rather than `ELOOP` — the symlink itself
            // isn't a directory.  Disambiguate by fstatat'ing the leaf
            // and reporting SymlinkComponent if it's actually a symlink.
            let raw = e.raw_os_error();
            if raw == Some(libc::ELOOP)
                || (nofollow
                    && raw == Some(libc::ENOTDIR)
                    && is_symlink_at(parent.as_raw_fd(), name))
            {
                return Err(QuarantineError::SymlinkComponent);
            }
            return Err(map_open_err(e));
        }
        Ok(OwnedFd::from_raw_fd(fd))
    }
}

unsafe fn is_symlink_at(dir_fd: libc::c_int, name: *const libc::c_char) -> bool {
    let mut sb: libc::stat = std::mem::zeroed();
    if libc::fstatat(dir_fd, name, &mut sb, libc::AT_SYMLINK_NOFOLLOW) != 0 {
        return false;
    }
    (sb.st_mode & libc::S_IFMT) == libc::S_IFLNK
}

fn to_c(name: &OsStr) -> Result<CString, QuarantineError> {
    CString::new(name.as_bytes()).map_err(|_| QuarantineError::EscapesRoot)
}

fn map_open_err(e: std::io::Error) -> QuarantineError {
    // ELOOP with O_NOFOLLOW means "final component was a symlink" — the
    // TOCTOU signal we care about.  Everything else is either a legitimate
    // I/O error or an escape attempt (ENOENT on `..`, etc.).
    match e.raw_os_error() {
        Some(libc::ELOOP) => QuarantineError::SymlinkComponent,
        // Carry the io::ErrorKind so `quarantine_err_reply` can route
        // AlreadyExists → FSERR_ALREADY_EXISTS, NotFound →
        // FSERR_NOT_FOUND, PermissionDenied → FSERR_PERM, etc.  Pre-
        // slice-10b this arm collapsed every non-ELOOP failure to
        // FSERR_IO, breaking the create-if-not-exists idiom under
        // `wx`/`w+x` open modes (the caller received FSERR_IO instead
        // of the mapped FSERR_ALREADY_EXISTS on collision).
        _ => QuarantineError::IoError(e.kind(), io_msg_scrub(&e)),
    }
}

/// Scrub `std::io::Error` display to avoid leaking canonical paths back
/// to a Rholang caller.  We keep the OS-level classification (which is
/// what a caller needs to distinguish `NotFound` from `PermissionDenied`)
/// and drop the free-form message.
///
/// L-2 fix (2026-08-06): use `{}` (Display) instead of `{:?}` (Debug).
/// Debug on `std::io::ErrorKind` today produces `NotFound`,
/// `PermissionDenied`, etc.  — safe.  But a future stdlib variant like
/// `Uncategorized(String)` (unstable but landing) would Debug-format the
/// inner String, potentially exfiltrating a host path back to Rholang.
/// Display is guaranteed to be a stable human-readable classification
/// with no internal-field spillage.
pub fn io_msg_scrub(e: &std::io::Error) -> String { format!("{}", e.kind()) }

/// M-R2 review fix (slice 29 round 2): lexically normalize
/// `PathBuf::from(root).join(rel)` so equivalent rel forms produce
/// identical `PathBuf`s.  Removes `.` components (`Component::CurDir`)
/// and relies on `Path::components()` to collapse duplicate separators
/// (`//` → `/`).  Does NOT resolve symlinks (that's canonicalize's job
/// and requires disk I/O) — this is a pure lexical rewrite suitable
/// for consensus WAL entries where the canonical string must be
/// deterministic per-input independent of host state.
///
/// Rejects `..` implicitly: `safe_descend` upstream already forbids
/// parent-references in `rel`, so `..` never reaches here.  If a future
/// caller bypassed that check, `..` would appear as
/// `Component::ParentDir` and pass through unchanged (still a
/// footgun — the safe_descend gate is the load-bearing check).
pub fn canonicalize_lexical(root: &str, rel: &str) -> std::path::PathBuf {
    use std::path::PathBuf;
    let joined = PathBuf::from(root).join(rel);
    let mut normalized = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::CurDir => {} // skip `.`
            other => normalized.push(other),
        }
    }
    normalized
}

#[cfg(test)]
mod path_tests {
    use super::*;

    #[test]
    fn canonicalize_lexical_removes_cur_dir_components() {
        assert_eq!(
            canonicalize_lexical("/root", "./a/./b.txt"),
            std::path::PathBuf::from("/root/a/b.txt")
        );
    }

    #[test]
    fn canonicalize_lexical_collapses_double_separators() {
        assert_eq!(
            canonicalize_lexical("/root", "a//b.txt"),
            std::path::PathBuf::from("/root/a/b.txt")
        );
    }

    #[test]
    fn canonicalize_lexical_plain_paths_are_unchanged() {
        assert_eq!(
            canonicalize_lexical("/root", "a/b.txt"),
            std::path::PathBuf::from("/root/a/b.txt")
        );
    }

    #[test]
    fn canonicalize_lexical_two_equivalent_forms_agree() {
        // The M-R2 property: equivalent rel forms produce the SAME PathBuf.
        let a = canonicalize_lexical("/root", "a/b.txt");
        let b = canonicalize_lexical("/root", "./a/b.txt");
        let c = canonicalize_lexical("/root", "a//b.txt");
        assert_eq!(a, b);
        assert_eq!(a, c);
    }
}

/// Translate `QuarantineError` to the (code, message) pair for the
/// `[false, code, msg]` reply shape.
pub fn quarantine_err_reply(e: &QuarantineError) -> (&'static str, String) {
    use super::errors::{io_err_code, FSERR_BAD_ARG, FSERR_QUARANTINE};
    match e {
        QuarantineError::Empty => (FSERR_BAD_ARG, "empty relative path".into()),
        QuarantineError::RootSelf => (FSERR_BAD_ARG, "path resolves to root itself".into()),
        QuarantineError::EscapesRoot => (FSERR_QUARANTINE, "path escapes root".into()),
        QuarantineError::SymlinkComponent => {
            (FSERR_QUARANTINE, "symlink in path components".into())
        }
        QuarantineError::RootIdentityChanged => (
            FSERR_QUARANTINE,
            "provisioned root's (dev, inode) does not match boot-captured identity — \
             possible rename-and-recreate attack (H-5); check for out-of-band mv/rebind \
             on the provisioned path"
                .into(),
        ),
        QuarantineError::IoError(kind, m) => {
            // Route via `io_err_code` so AlreadyExists / NotFound /
            // PermissionDenied / Unsupported reach their spec-canonical
            // FSERR codes instead of collapsing to FSERR_IO.  Kinds
            // without a dedicated FSERR (e.g. `Other`) fall back to
            // FSERR_IO via io_err_code's default arm.
            let synthetic = io::Error::from(*kind);
            (io_err_code(&synthetic), m.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Write;
    use std::os::unix::fs::symlink;

    use tempfile::TempDir;

    use super::*;

    fn assert_qe(actual: Result<SafeParent, QuarantineError>, expected: QuarantineError) {
        match actual {
            Ok(_) => panic!("expected {expected:?}, got Ok"),
            Err(e) => assert_eq!(e, expected),
        }
    }

    #[test]
    fn rejects_empty() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        assert_qe(safe_descend(&root, ""), QuarantineError::Empty);
    }

    #[test]
    fn rejects_parent_traversal() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        assert_qe(
            safe_descend(&root, "../escape.txt"),
            QuarantineError::EscapesRoot,
        );
    }

    #[test]
    fn rejects_absolute() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        assert_qe(
            safe_descend(&root, "/etc/passwd"),
            QuarantineError::EscapesRoot,
        );
    }

    #[test]
    fn rejects_root_self_after_collapse() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        assert_qe(safe_descend(&root, "."), QuarantineError::RootSelf);
    }

    #[test]
    fn allows_simple_descendant() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let file = root.join("sub/nested.txt");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, b"hi").unwrap();
        let parent = safe_descend(&root, "sub/nested.txt").unwrap();
        unsafe {
            let mut sb: libc::stat = std::mem::zeroed();
            let rc = libc::fstatat(
                parent.as_raw_fd(),
                parent.leaf_ptr(),
                &mut sb,
                libc::AT_SYMLINK_NOFOLLOW,
            );
            assert_eq!(rc, 0);
            assert_eq!(sb.st_size, 2);
        }
    }

    #[test]
    fn rejects_symlink_intermediate() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        fs::create_dir_all(root.join("real")).unwrap();
        let mut f = fs::File::create(root.join("real/leaf.txt")).unwrap();
        f.write_all(b"hi").unwrap();
        symlink(root.join("real"), root.join("link")).unwrap();
        assert_qe(
            safe_descend(&root, "link/leaf.txt"),
            QuarantineError::SymlinkComponent,
        );
    }

    #[test]
    fn safe_open_reads_file() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        fs::write(root.join("hi.txt"), b"contents").unwrap();
        let f = safe_open(&root, "hi.txt", libc::O_RDONLY, 0).unwrap();
        assert_eq!(f.metadata().unwrap().len(), 8);
    }

    #[test]
    fn safe_open_rejects_symlink_leaf() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        fs::write(root.join("real.txt"), b"contents").unwrap();
        symlink(root.join("real.txt"), root.join("link.txt")).unwrap();
        let err = safe_open(&root, "link.txt", libc::O_RDONLY, 0).unwrap_err();
        assert_eq!(err, QuarantineError::SymlinkComponent);
    }

    /// Slice 30c H-P7-6 fix regression pin: post-boot symlink-swap
    /// attack on the root path fails cleanly.  Pre-fix, `open_dir`
    /// on the root did NOT set O_NOFOLLOW — an attacker replacing
    /// the canonicalized root with a symlink post-boot would
    /// silently redirect every subsequent syscall to the symlink's
    /// target.  Post-fix, the root open uses O_NOFOLLOW so the
    /// swap surfaces as `SymlinkComponent`.
    #[test]
    fn safe_descend_rejects_root_replaced_with_symlink_post_boot() {
        // Simulate boot: canonicalize a real directory.
        let attacker_tree = TempDir::new().unwrap();
        std::fs::write(
            attacker_tree.path().join("gotcha.txt"),
            b"attacker-controlled",
        )
        .unwrap();

        let staging = TempDir::new().unwrap();
        let root_path = staging.path().join("legit-root");
        std::fs::create_dir(&root_path).unwrap();
        // Legitimate content that would live under this root.
        std::fs::write(root_path.join("legit.txt"), b"legit").unwrap();

        // Sanity: safe_descend works pre-attack.
        let ok = safe_descend(&root_path, "legit.txt");
        assert!(ok.is_ok(), "pre-attack descend should succeed");

        // Attack: remove the real directory, replace with a symlink
        // to the attacker tree.
        std::fs::remove_dir_all(&root_path).unwrap();
        symlink(attacker_tree.path(), &root_path).unwrap();

        // Post-attack: safe_descend must fail with SymlinkComponent
        // (H-P7-6 fix).  Pre-fix this would silently succeed against
        // `gotcha.txt` in the attacker tree.
        let post_attack = safe_descend(&root_path, "legit.txt");
        assert!(
            matches!(&post_attack, Err(QuarantineError::SymlinkComponent)),
            "post-boot root symlink-swap must surface as SymlinkComponent \
             (H-P7-6 fix); pre-fix this would have followed the symlink into \
             the attacker tree.  Got: {:?}",
            post_attack.as_ref().err()
        );

        // Companion: the ATTACKER file itself must not be reachable
        // via the compromised root path either — even if it exists
        // in the attacker tree, safe_descend can't traverse a
        // symlinked root.
        let attacker_path = safe_descend(&root_path, "gotcha.txt");
        assert!(
            matches!(&attacker_path, Err(QuarantineError::SymlinkComponent)),
            "attacker file must NOT be reachable via compromised root path; \
             got: {:?}",
            attacker_path.as_ref().err()
        );
    }

    /// H-5 regression: `safe_descend_verified` must reject a
    /// rename-and-recreate attack that `safe_descend`/H-P7-6 does
    /// NOT cover.
    ///
    /// Scenario: attacker moves the boot-provisioned directory
    /// aside (`mv /legit /legit.bak`) and creates a fresh empty
    /// directory with the same name (`mkdir /legit`) — the new
    /// directory is a real directory (not a symlink), so the
    /// O_NOFOLLOW check in `safe_descend` passes.  Only its
    /// `(dev, inode)` identity differs from what boot captured.
    ///
    /// With `expected_root_id = Some(boot_id)`,
    /// `safe_descend_verified` fstats the freshly opened dir and
    /// compares — the mismatch surfaces as
    /// `QuarantineError::RootIdentityChanged`.
    #[test]
    fn safe_descend_verified_rejects_rename_and_recreate() {
        let staging = TempDir::new().unwrap();
        let root_path = staging.path().join("legit-root");
        std::fs::create_dir(&root_path).unwrap();
        std::fs::write(root_path.join("legit.txt"), b"boot-content").unwrap();

        // Boot: capture the identity.
        let boot_id = capture_root_identity(&root_path).expect("boot-time stat ok");

        // Sanity: verified descend works with the correct id
        // against the original inode.
        let pre = safe_descend_verified(&root_path, "legit.txt", Some(boot_id));
        assert!(
            pre.is_ok(),
            "pre-attack verified descend should succeed; got {:?}",
            pre.as_ref().err()
        );

        // Attack: rename the real directory aside, then create a
        // fresh directory with the same name and populate with
        // attacker-controlled content of the same name.
        let sidelined = staging.path().join("legit-root.bak");
        std::fs::rename(&root_path, &sidelined).unwrap();
        std::fs::create_dir(&root_path).unwrap();
        std::fs::write(root_path.join("legit.txt"), b"attacker-content").unwrap();

        // Without verification the descend succeeds — the fresh
        // dir is a real dir, so H-P7-6's O_NOFOLLOW happily
        // opens it.  Pin that behavior so a regression can't
        // silently make the verified variant unnecessary.
        let unverified = safe_descend(&root_path, "legit.txt");
        assert!(
            unverified.is_ok(),
            "unverified safe_descend intentionally passes rename-and-recreate — \
             that's the H-5 gap.  Got: {:?}",
            unverified.as_ref().err()
        );

        // WITH verification: the (dev, inode) mismatch is
        // detected and the syscall is quarantined.
        let verified = safe_descend_verified(&root_path, "legit.txt", Some(boot_id));
        assert!(
            matches!(&verified, Err(QuarantineError::RootIdentityChanged)),
            "H-5: rename-and-recreate must be caught as RootIdentityChanged; \
             got: {:?}",
            verified.as_ref().err()
        );
    }

    // `stat_leaf_dev_inode` + its 5 tests removed in review-2 fix
    // (2026-08-12).  The fd-based `dev_inode_from_fd` in handlers.rs
    // (uses the already-pub `fstat_dev_inode` helper) is the
    // oracular-correct keyer; the path-based helper was semantically
    // wrong for LockRegistry keying under oracular file swap.

    /// Slice-10b bug-fix pin: `quarantine_err_reply` MUST route
    /// `QuarantineError::IoError(kind, _)` through `io_err_code(kind)`
    /// so real syscall errors (AlreadyExists / NotFound /
    /// PermissionDenied / Unsupported) reach their spec-canonical
    /// FSERR codes.  Pre-fix the router collapsed every IoError to
    /// `FSERR_IO`, breaking the create-if-not-exists idiom under
    /// `wx`/`w+x` (a caller checking for FSERR_ALREADY_EXISTS to
    /// distinguish "already there" from "genuinely broken" got
    /// FSERR_IO instead — indistinguishable from a disk failure).
    ///
    /// A regression that reverts the enum variant to `IoError(String)`
    /// or the reply arm to `(FSERR_IO, _)` trips one of these four
    /// arms.
    #[test]
    fn quarantine_err_reply_routes_io_error_kind_to_matching_fserr() {
        use super::super::errors::{
            FSERR_ALREADY_EXISTS, FSERR_BAD_ARG, FSERR_IO, FSERR_NOT_FOUND, FSERR_PERM,
            FSERR_UNSUPPORTED,
        };
        let cases: &[(io::ErrorKind, &str, &str)] = &[
            (io::ErrorKind::AlreadyExists, FSERR_ALREADY_EXISTS, "wx"),
            (io::ErrorKind::NotFound, FSERR_NOT_FOUND, "missing"),
            (io::ErrorKind::PermissionDenied, FSERR_PERM, "read-only fs"),
            (io::ErrorKind::Unsupported, FSERR_UNSUPPORTED, "no fifo"),
            (io::ErrorKind::InvalidInput, FSERR_BAD_ARG, "bad flag"),
            // `Other` is the fall-through: preserves FSERR_IO for
            // sites that don't have a natural kind (e.g., CString
            // NulError inside `open_dir`).
            (io::ErrorKind::Other, FSERR_IO, "some io error"),
        ];
        for (kind, expected_code, msg) in cases {
            let (got_code, got_msg) =
                quarantine_err_reply(&QuarantineError::IoError(*kind, (*msg).to_string()));
            assert_eq!(
                got_code, *expected_code,
                "quarantine_err_reply(IoError({kind:?})) should route to {expected_code}",
            );
            assert_eq!(&got_msg, msg, "message must be forwarded verbatim");
        }
    }

    /// Slice-10b bug-fix pin: `safe_open` on `O_CREAT | O_EXCL`
    /// with a pre-existing leaf must produce
    /// `QuarantineError::IoError(AlreadyExists, _)` — the shape
    /// that `quarantine_err_reply` routes to FSERR_ALREADY_EXISTS.
    /// Pre-fix `map_open_err` discarded the io::ErrorKind, and this
    /// arm silently degraded to FSERR_IO downstream.
    #[test]
    fn safe_open_o_excl_on_existing_reports_already_exists_kind() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        fs::write(root.join("existing.txt"), b"already here").unwrap();

        let err = safe_open(
            &root,
            "existing.txt",
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
            0o644,
        )
        .expect_err("O_CREAT|O_EXCL on existing must fail");
        match err {
            QuarantineError::IoError(io::ErrorKind::AlreadyExists, _) => {}
            other => panic!(
                "expected IoError(AlreadyExists, _), got {other:?} — the fix \
                 to preserve io::ErrorKind in QuarantineError::IoError has \
                 regressed and callers can no longer distinguish EEXIST from \
                 arbitrary IO failure"
            ),
        }
    }

    /// Slice-10b review pin (FIPS defense-in-depth): the scrubbed
    /// message that leaves `map_open_err` — and therefore reaches
    /// the Rholang reply tuple via `quarantine_err_reply` — must
    /// NOT contain any component of the caller's canonical path.
    /// Pre-fix `map_open_err` correctly called `io_msg_scrub` (which
    /// formats only the `io::ErrorKind` display), but a reasonable-
    /// looking refactor to `format!("{}", e)` or `e.to_string()`
    /// would leak the underlying path (stdlib's `io::Error`
    /// constructed via `Error::from_raw_os_error` combined with
    /// path context can emit `"No such file or directory: /foo/bar"`
    /// — the exact leakage the L-2 fix on `io_msg_scrub` intends
    /// to prevent).
    ///
    /// Provokes a real EEXIST via `safe_open(O_CREAT|O_EXCL)` on a
    /// pre-existing leaf and asserts the returned message does not
    /// contain the temp-directory basename.  Fails loudly if a
    /// future refactor swaps `io_msg_scrub` for a raw `Display`.
    #[test]
    fn safe_open_error_message_does_not_leak_canonical_path() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        // Ensure the tempdir path contains a segment we can grep for.
        let tmp_basename = root
            .file_name()
            .and_then(|n| n.to_str())
            .expect("tempdir path has a unicode basename")
            .to_owned();
        let leaf = "leaky-path-probe.txt";
        fs::write(root.join(leaf), b"already here").unwrap();

        let err = safe_open(
            &root,
            leaf,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
            0o644,
        )
        .expect_err("O_CREAT|O_EXCL on existing must fail");
        let (_code, msg) = quarantine_err_reply(&err);

        // Neither the tempdir basename nor the leaf filename should
        // appear in the reply.  A raw `e.to_string()` in place of
        // `io_msg_scrub` would let one of these through on hosts
        // whose libc annotates EEXIST with the offending path.
        assert!(
            !msg.contains(&tmp_basename),
            "FIPS: reply message must not leak the tempdir path.  \
             Got message: {msg:?}; tempdir basename: {tmp_basename:?}. \
             Check that `map_open_err` still calls `io_msg_scrub(&e)` \
             and not `e.to_string()` / `format!(\"{{}}\", e)`."
        );
        assert!(
            !msg.contains(leaf),
            "FIPS: reply message must not leak the leaf filename.  \
             Got message: {msg:?}; leaf name: {leaf:?}."
        );
    }

    /// Shape A (2026-08-31): the legacy `register(canon, id)` API
    /// records the caller's canon as BOTH the logical and the
    /// on-disk root, so `resolve(canon)` and `get(canon)` continue
    /// to return the identity that pre-Shape-A callers saw.
    /// Handlers that migrate to `resolve_or_identity(canon)` under
    /// this legacy registration observe `(canon, Some(id))` — the
    /// on-disk path handed to `safe_descend_verified` is the same
    /// path the Rholang side passed in, which is the pre-Shape-A
    /// behavior.  A regression that dropped the on_disk_root =
    /// canon default in `register` would trip this pin.
    #[test]
    fn root_registry_legacy_register_is_identity_remap() {
        let reg = RootIdentityRegistry::new();
        let canon = std::path::PathBuf::from("/tmp/legacy/target");
        reg.register(canon.clone(), (17, 42));

        assert_eq!(reg.get(&canon), Some((17, 42)), "legacy get() unchanged");

        let resolved = reg.resolve(&canon).expect("legacy register also visible via resolve");
        assert_eq!(resolved.on_disk_root, canon, "logical == on_disk under legacy register");
        assert_eq!(resolved.identity, (17, 42));

        let (on_disk, id) = reg.resolve_or_identity(&canon);
        assert_eq!(on_disk, canon);
        assert_eq!(id, Some((17, 42)));
    }

    /// Shape A (2026-08-31): `register_with_remap(logical, on_disk,
    /// id)` records a distinct logical→on-disk mapping.  Handlers
    /// resolving the logical root receive the on-disk absolute
    /// (which is what they'll pass to `safe_descend_verified`) +
    /// the boot identity.  `get(on_disk)` returns None — the
    /// registry is keyed by LOGICAL root, not on-disk.  A
    /// regression that keyed by on-disk would break the
    /// per-validator resolution model (two validators with the
    /// same logical root but different on-disks would clobber
    /// each other's entry).
    #[test]
    fn root_registry_remap_resolves_logical_to_on_disk() {
        let reg = RootIdentityRegistry::new();
        let logical = std::path::PathBuf::from("/@bundle/target");
        let on_disk = std::path::PathBuf::from("/tmp/validator-A/target");
        reg.register_with_remap(logical.clone(), on_disk.clone(), (7, 11));

        let resolved = reg.resolve(&logical).expect("remap must be resolvable by logical key");
        assert_eq!(resolved.on_disk_root, on_disk);
        assert_eq!(resolved.identity, (7, 11));

        let (r_on_disk, r_id) = reg.resolve_or_identity(&logical);
        assert_eq!(r_on_disk, on_disk, "handler gets the on-disk path to syscall against");
        assert_eq!(r_id, Some((7, 11)));

        assert!(
            reg.get(&on_disk).is_none(),
            "registry is keyed by LOGICAL root; the on-disk absolute is not itself a key"
        );
    }

    /// Shape A (2026-08-31): `resolve_or_identity` on an
    /// unregistered logical root falls through to the caller's
    /// original path with `None` identity — matches pre-Shape-A
    /// behavior exactly (handler-side `get() → None` + pass the
    /// caller's path to `safe_descend_verified` with `None` id).
    /// A regression that started synthesizing a made-up identity
    /// or returning an empty PathBuf would trip this pin.
    #[test]
    fn root_registry_resolve_or_identity_falls_through_for_unregistered() {
        let reg = RootIdentityRegistry::new();
        let unknown = std::path::PathBuf::from("/some/never-registered/path");

        let (on_disk, id) = reg.resolve_or_identity(&unknown);
        assert_eq!(on_disk, unknown, "fall-through returns the caller's own path");
        assert!(id.is_none(), "no identity available for unregistered logical roots");
        assert!(reg.resolve(&unknown).is_none());
    }
}
