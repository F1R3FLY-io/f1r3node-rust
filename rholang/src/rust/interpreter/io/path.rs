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
    let mut cur = open_dir(root, true)?;

    let leaf_name = comps.last().unwrap();
    for intermediate in &comps[..comps.len() - 1] {
        let name = to_c(intermediate)?;
        cur = openat_dir(&cur, name.as_ptr(), true)?;
    }
    let leaf = to_c(leaf_name)?;

    Ok(SafeParent { dirfd: cur, leaf })
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
pub fn io_msg_scrub(e: &std::io::Error) -> String { format!("{:?}", e.kind()) }

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
}
