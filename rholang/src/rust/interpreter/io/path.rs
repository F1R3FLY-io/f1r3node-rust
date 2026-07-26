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

    // Open the root itself.  We don't set O_NOFOLLOW on the root open —
    // if the caller passed a symlinked root, that's a configuration
    // choice.  Root is boot-time canonicalized by the static-provisioning
    // layer, so this is fine.
    let mut cur = open_dir(root, false)?;

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
            return Err(map_open_err(e));
        }
        Ok(OwnedFd::from_raw_fd(fd))
    }
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
}
