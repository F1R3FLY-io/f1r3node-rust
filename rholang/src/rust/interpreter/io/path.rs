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
    IoError(String),
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
/// Also used by Phase 8 slice 8a's `stat_leaf_dev_inode` helper to
/// key the `LockRegistry` on the physical file identity a Rholang
/// `File` cap addresses.
pub fn fstat_dev_inode(fd: i32) -> Result<(u64, u64), QuarantineError> {
    #[cfg(unix)]
    {
        unsafe {
            let mut st: libc::stat = std::mem::zeroed();
            if libc::fstat(fd, &mut st) < 0 {
                return Err(QuarantineError::IoError(
                    std::io::Error::last_os_error().to_string(),
                ));
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

/// Phase 8 slice 8a helper: safely resolve `(root, rel)` and return
/// the leaf's `(st_dev, st_ino)`.  Composes `safe_descend_verified`
/// (H-5 root-identity check) with `openat(O_NOFOLLOW)` on the leaf
/// and `fstat` on the resulting fd — the fd is scoped to this call
/// (dropped on return) so we're strictly querying identity, not
/// consuming an fd slot.
///
/// The (dev, inode) return value is used to key the `LockRegistry`
/// per Phase 8 §X-1 memo: keying on physical file identity collapses
/// two fresh-mint `File` caps opened over the same on-disk file to
/// a single lock-coordination entry.
///
/// Symlinks at the leaf reject with `QuarantineError::SymlinkComponent`
/// per the FIP's §Non-regular-file rejection rule.
pub fn stat_leaf_dev_inode(
    root: &Path,
    rel: &str,
    expected_root_id: Option<(u64, u64)>,
) -> Result<(u64, u64), QuarantineError> {
    let parent = safe_descend_verified(root, rel, expected_root_id)?;
    #[cfg(unix)]
    {
        unsafe {
            let flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
            let fd = libc::openat(parent.as_raw_fd(), parent.leaf_ptr(), flags);
            if fd < 0 {
                let e = std::io::Error::last_os_error();
                // A symlink at the leaf under O_NOFOLLOW returns ELOOP
                // on Linux and ENOTDIR on macOS.  Map to SymlinkComponent
                // uniformly, matching openat_dir's behavior.
                let raw = e.raw_os_error();
                if raw == Some(libc::ELOOP) || raw == Some(libc::ENOTDIR) {
                    return Err(QuarantineError::SymlinkComponent);
                }
                return Err(map_open_err(e));
            }
            let owned = OwnedFd::from_raw_fd(fd);
            fstat_dev_inode(owned.as_raw_fd())
        }
    }
    #[cfg(not(unix))]
    {
        // Non-unix: no dev/ino, matches capture_root_identity's stub.
        let _ = parent;
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
/// The map is keyed by the boot-canonicalized root path
/// (`std::fs::canonicalize`'d).  Handlers look up by the
/// `canonRoot` string they receive from the Fs.rho bundle,
/// which is byte-identical to the boot-canonicalized path
/// (see `format_bundle_for_rholang`).
#[derive(Debug, Clone, Default)]
pub struct RootIdentityRegistry {
    inner: std::sync::Arc<
        std::sync::RwLock<std::collections::HashMap<std::path::PathBuf, (u64, u64)>>,
    >,
}

impl RootIdentityRegistry {
    pub fn new() -> Self { Self::default() }

    /// Record a root's boot-time identity.  Idempotent — a repeat
    /// register with the same value is a no-op; a repeat with a
    /// different value overwrites (last-write-wins, which should
    /// not happen in practice since boot populates once).
    pub fn register(&self, root: std::path::PathBuf, id: (u64, u64)) {
        let mut guard = self.inner.write().expect("root-identity registry poisoned");
        guard.insert(root, id);
    }

    /// Look up a root's expected identity.  Returns `None` for
    /// unregistered paths — callers pass `None` through to
    /// `safe_descend_verified`, which then skips the check.
    pub fn get(&self, root: &std::path::Path) -> Option<(u64, u64)> {
        let guard = self.inner.read().expect("root-identity registry poisoned");
        guard.get(root).copied()
    }

    /// Count of registered roots.  For diagnostics only.
    pub fn len(&self) -> usize {
        let guard = self.inner.read().expect("root-identity registry poisoned");
        guard.len()
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
        .map_err(|e| QuarantineError::IoError(e.to_string()))?;
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
        _ => QuarantineError::IoError(io_msg_scrub(&e)),
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
    use super::errors::{FSERR_BAD_ARG, FSERR_IO, FSERR_QUARANTINE};
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
        QuarantineError::IoError(m) => (FSERR_IO, m.clone()),
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

    // -- Phase 8 slice 8a — stat_leaf_dev_inode ------------------------

    #[test]
    fn stat_leaf_dev_inode_returns_pair_for_regular_file() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let leaf = root.join("data.bin");
        fs::write(&leaf, b"hello").unwrap();
        // Sanity: the pair should match a direct fs::metadata call.
        let expected = fs::metadata(&leaf).unwrap();
        let (dev, ino) =
            stat_leaf_dev_inode(&root, "data.bin", None).expect("regular file must resolve");
        use std::os::unix::fs::MetadataExt;
        assert_eq!(dev, expected.dev());
        assert_eq!(ino, expected.ino());
    }

    #[test]
    fn stat_leaf_dev_inode_rejects_symlink_at_leaf() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let target = root.join("target.bin");
        fs::write(&target, b"x").unwrap();
        symlink(&target, root.join("link.bin")).unwrap();
        // openat(O_NOFOLLOW) at the leaf returns ELOOP (Linux) /
        // ENOTDIR (macOS); both map uniformly to SymlinkComponent.
        assert_qe_dev_inode(
            stat_leaf_dev_inode(&root, "link.bin", None),
            QuarantineError::SymlinkComponent,
        );
    }

    #[test]
    fn stat_leaf_dev_inode_rejects_escape() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        assert_qe_dev_inode(
            stat_leaf_dev_inode(&root, "../escape.txt", None),
            QuarantineError::EscapesRoot,
        );
    }

    #[test]
    fn stat_leaf_dev_inode_reports_io_on_missing_leaf() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        // No file at `data.bin` — openat returns ENOENT which maps
        // to IoError via `map_open_err`.
        let result = stat_leaf_dev_inode(&root, "data.bin", None);
        assert!(
            matches!(&result, Err(QuarantineError::IoError(_))),
            "missing leaf must surface as IoError; got: {:?}",
            result.as_ref().err()
        );
    }

    #[test]
    fn stat_leaf_dev_inode_detects_root_identity_drift() {
        // H-5 inheritance: if the caller passes the expected boot-id
        // and the on-disk (dev, inode) has drifted, safe_descend_verified
        // catches it BEFORE we ever try to open the leaf.
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        fs::write(root.join("data.bin"), b"x").unwrap();
        // Deliberately-wrong expected identity.
        let bogus_id = (u64::MAX, u64::MAX);
        assert_qe_dev_inode(
            stat_leaf_dev_inode(&root, "data.bin", Some(bogus_id)),
            QuarantineError::RootIdentityChanged,
        );
    }

    fn assert_qe_dev_inode(actual: Result<(u64, u64), QuarantineError>, expected: QuarantineError) {
        match actual {
            Ok(pair) => panic!("expected {expected:?}, got Ok({pair:?})"),
            Err(e) => assert_eq!(e, expected),
        }
    }
}
