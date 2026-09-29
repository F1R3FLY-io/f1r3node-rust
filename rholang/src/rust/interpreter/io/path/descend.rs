// TOCTOU-immune walk from `root` to a leaf's parent.
//
// The canonical entry point for every path-taking handler in the io
// tree.  Given a `root` path and a relative `rel` string, returns a
// [`SafeParent`] that bundles a dirfd for the leaf's parent + the
// leaf's `CString` name.  Callers issue every subsequent syscall
// via `*at` against that dirfd — never by rebuilding the path — or
// the TOCTOU-immunity is lost.
//
// # How it works
//
// 1. Reject `rel` if it is empty, absolute, or contains `..`.
// 2. Open `root` as a dirfd with `O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC`.
//    `O_NOFOLLOW` on the root open closes the "swap root with a
//    symlink post-boot" attack.
// 3. Optionally verify the opened root's `(dev, inode)` pair against
//    the caller-supplied boot-captured expected pair (H-5:
//    rename-and-recreate defense).
// 4. Descend along `rel`'s components via
//    `openat(dirfd, name, O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)`
//    at every intermediate.  Any symlink component surfaces as
//    `SymlinkComponent`; any other syscall failure surfaces as
//    `IoError(kind, scrubbed_msg)`.
// 5. Return the last-parent dirfd + the leaf's `CString`.
//
// On macOS, `openat(O_DIRECTORY | O_NOFOLLOW)` against a symlink
// returns `ENOTDIR` rather than Linux's `ELOOP`.  The `is_symlink_at`
// helper disambiguates by `fstatat` so both hosts produce a
// consistent `SymlinkComponent` outcome.

use std::ffi::{CString, OsStr};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path};

use super::{io_msg_scrub, QuarantineError, SafeParent};

/// Descend from `root` along `rel` without following any symlink,
/// and return a handle to the leaf's parent + the leaf's basename.
///
/// Convenience for callers that don't have a boot-captured root
/// identity to verify against.  Equivalent to
/// `safe_descend_verified(root, rel, None)`.
///
/// See [`safe_descend_verified`] for the full precondition contract
/// (in particular, `root` MUST be canonicalized).
pub fn safe_descend(root: &Path, rel: &str) -> Result<SafeParent, QuarantineError> {
    safe_descend_verified(root, rel, None)
}

/// Verified variant of [`safe_descend`].  If `expected_root_id` is
/// `Some((dev, inode))`, the opened root's `(dev, inode)` pair is
/// checked against the expected value after opening; a mismatch
/// surfaces `QuarantineError::RootIdentityChanged`.
///
/// This is the H-5 rename-and-recreate defense: an attacker with
/// write access to the root's parent can `mv /legit /legit.bak &&
/// mkdir /legit && populate` — the new `/legit` is a real directory
/// with a different inode.  `O_NOFOLLOW` allows opening real dirs,
/// so the descent would silently land in the attacker's tree
/// without this check.
///
/// The boot-time capture of the expected `(dev, inode)` pair lives
/// in the `path::identity` submodule (Wave 1 PR 1.3).
///
/// # Preconditions
///
/// **`root` MUST be a fully canonicalized absolute path.**
/// `open_dir` applies `O_NOFOLLOW` only to the *final* component
/// of `root`; if any intermediate directory in `root`'s path is a
/// symlink, it is silently followed.  An attacker who controls the
/// parent of the provisioned root can then redirect the whole tree
/// and bypass every downstream check.  Callers must invoke
/// `.canonicalize()` at boot time and hand the result to this
/// function; do not accept a raw operator string.
///
/// The follow-up `path::identity` module (Wave 1 PR 1.3) introduces
/// a `Root` newtype whose constructor performs this canonicalization,
/// so the precondition becomes compiler-enforceable.  Until then,
/// this contract is a documented caller responsibility.
///
/// # TOCTOU-immunity contract
///
/// The `SafeParent.dirfd` returned by this function is bound to the
/// exact path resolved here.  Every subsequent syscall against the
/// leaf MUST be an `*at` variant referencing that dirfd (`fchmodat`,
/// `fchownat`, `renameat`, `unlinkat`, `fstatat`, `openat`, ...).
/// Rebuilding the path string and using a non-`*at` syscall
/// re-opens the TOCTOU window.
pub fn safe_descend_verified(
    root: &Path,
    rel: &str,
    expected_root_id: Option<(u64, u64)>,
) -> Result<SafeParent, QuarantineError> {
    if rel.is_empty() {
        return Err(QuarantineError::Empty);
    }
    let rel_path = Path::new(rel);
    // Belt-and-suspenders: reject leading `/` here on the raw
    // `rel`, and reject `Component::RootDir` again during
    // component iteration below.  Both layers are cheap and the
    // intent is clearer with the checks stated at each stage.
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

    // Open the root itself with `O_NOFOLLOW` so a post-boot
    // symlink-swap on the root path fails cleanly (ELOOP on Linux,
    // ENOTDIR on macOS — both map to `SymlinkComponent`).  What
    // this does NOT close: rename-and-recreate.  That's the H-5
    // defense below; a caller supplies `expected_root_id` from a
    // boot-captured `(dev, inode)` and we verify after open.
    let cur = open_dir(root, true)?;

    // H-5: verify the opened root's identity matches the boot-
    // captured expected pair.  Detects rename-and-recreate.
    if let Some(expected) = expected_root_id {
        let actual = fstat_dev_inode(cur.as_raw_fd())?;
        if actual != expected {
            return Err(QuarantineError::RootIdentityChanged);
        }
    }

    let mut cur = cur;
    let leaf_name = comps.last().expect("comps non-empty (checked above)");
    for intermediate in &comps[..comps.len() - 1] {
        let name = to_c(intermediate)?;
        cur = openat_dir(&cur, name.as_ptr(), true)?;
    }
    let leaf = to_c(leaf_name)?;

    Ok(SafeParent { dirfd: cur, leaf })
}

/// Fetch a directory's `(dev, inode)` pair via `fstat(2)`.  Used by
/// [`safe_descend_verified`] for the H-5 identity check and by the
/// eventual `path::identity` submodule for boot-time capture.
///
/// `pub(super)` — visible to other `path::*` submodules but not
/// externally.  Callers in other io modules should go through the
/// identity registry (Wave 1 PR 1.3) rather than opening fds
/// directly.
pub(super) fn fstat_dev_inode(fd: i32) -> Result<(u64, u64), QuarantineError> {
    // SAFETY: `libc::stat` is a POD C struct — `zeroed()` is a
    // valid initialization (all fields are integer types).  The
    // caller passes a real integer `fd`; `fstat` accepts any
    // integer and returns -1 with errno on invalid fd rather than
    // UB.  `&mut st` outlives the FFI call.
    unsafe {
        let mut st: libc::stat = std::mem::zeroed();
        if libc::fstat(fd, &mut st) < 0 {
            let e = io::Error::last_os_error();
            return Err(QuarantineError::IoError(e.kind(), io_msg_scrub(&e)));
        }
        // st_dev / st_ino widths differ across platforms
        // (u32 on some, u64 on others).  Widen uniformly to u64.
        #[allow(clippy::unnecessary_cast)]
        Ok((st.st_dev as u64, st.st_ino as u64))
    }
}

fn open_dir(path: &Path, nofollow: bool) -> Result<OwnedFd, QuarantineError> {
    let cpath = CString::new(path.as_os_str().as_bytes())
        .map_err(|e| QuarantineError::IoError(io::ErrorKind::InvalidInput, e.to_string()))?;
    // SAFETY: `cpath` is a locally-owned CString outliving this
    // block; its `.as_ptr()` returns a NUL-terminated `*const c_char`
    // valid for the block.  `libc::open` does not retain the
    // pointer.  On success `fd` is a fresh open fd, wrapped in
    // `OwnedFd` for RAII close on drop.
    unsafe {
        let mut flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC;
        if nofollow {
            flags |= libc::O_NOFOLLOW;
        }
        let fd = libc::open(cpath.as_ptr(), flags);
        if fd < 0 {
            let e = io::Error::last_os_error();
            // macOS returns ENOTDIR when `open(O_DIRECTORY | O_NOFOLLOW)`
            // hits a symlink (the symlink isn't a directory); Linux
            // returns ELOOP.  Both mean "the requested path is a
            // symlink and we refused to follow it".  Disambiguate by
            // `fstatat` (via `is_symlink_at_path`) so the operator-
            // facing outcome is `SymlinkComponent` on either host.
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

/// Absolute-path variant of `is_symlink_at`.  Used on the
/// ENOTDIR-fallback path in `open_dir` to distinguish a symlink-
/// swap attack from a genuine "expected a directory, got a
/// regular file" error.
///
/// # Safety
///
/// - `cpath` must be a valid, NUL-terminated `CString`.  This
///   function borrows it and passes `.as_ptr()` to `fstatat`; no
///   pointer is retained past the call.
/// - `libc::stat` is POD; `zeroed()` is a well-defined initializer.
unsafe fn is_symlink_at_path(cpath: &CString) -> bool {
    // SAFETY: bounds documented on the fn signature.
    unsafe {
        let mut sb: libc::stat = std::mem::zeroed();
        // AT_FDCWD + AT_SYMLINK_NOFOLLOW = lstat semantics on `cpath`.
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
}

fn openat_dir(
    parent: &OwnedFd,
    name: *const libc::c_char,
    // Currently always `true` at every call site — kept as a
    // parameter because the follow-up `safe_open` (Wave 1 PR 1.4)
    // opens the leaf and may pass `false` there.  Do NOT flip
    // this to `false` in intermediate-descent contexts: the
    // O_NOFOLLOW-at-every-step invariant is what makes the walk
    // TOCTOU-immune.
    nofollow: bool,
) -> Result<OwnedFd, QuarantineError> {
    // SAFETY: `parent` is a borrowed `OwnedFd` with an open dirfd
    // for the borrow's lifetime.  Caller passes `name` as a valid
    // NUL-terminated `*const c_char` whose lifetime covers this
    // call (`safe_descend_verified` owns the CString across the
    // openat).  `openat` returns a fresh fd (or -1) without
    // retaining `name`.
    unsafe {
        let mut flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC;
        if nofollow {
            flags |= libc::O_NOFOLLOW;
        }
        let fd = libc::openat(parent.as_raw_fd(), name, flags);
        if fd < 0 {
            let e = io::Error::last_os_error();
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

/// dirfd-relative variant of `is_symlink_at_path` — same ENOTDIR /
/// ELOOP disambiguation, but scoped to a parent dirfd.
///
/// # Safety
///
/// - `dir_fd` must be an open dirfd valid for the call.
/// - `name` must be a valid NUL-terminated `*const c_char` whose
///   lifetime covers the call.
/// - `libc::stat` is POD; `zeroed()` is well-defined.
unsafe fn is_symlink_at(dir_fd: libc::c_int, name: *const libc::c_char) -> bool {
    // SAFETY: bounds documented on the fn signature.
    unsafe {
        let mut sb: libc::stat = std::mem::zeroed();
        if libc::fstatat(dir_fd, name, &mut sb, libc::AT_SYMLINK_NOFOLLOW) != 0 {
            return false;
        }
        (sb.st_mode & libc::S_IFMT) == libc::S_IFLNK
    }
}

fn to_c(name: &OsStr) -> Result<CString, QuarantineError> {
    CString::new(name.as_bytes()).map_err(|_| {
        // A NUL byte in a path component is invalid input, not a
        // root escape — route through `IoError(InvalidInput, _)`
        // so `quarantine_err_reply` surfaces `FSERR_BAD_ARG` with
        // a message the caller can act on.
        QuarantineError::IoError(
            io::ErrorKind::InvalidInput,
            "NUL byte in path component".into(),
        )
    })
}

/// Classify an `open` / `openat` error.  `ELOOP` under `O_NOFOLLOW`
/// means "final component was a symlink" — the TOCTOU signal.
/// Every other kind is either a legitimate I/O error or a benign
/// escape (`ENOENT` on `..`, etc.); carry the `ErrorKind` so
/// `quarantine_err_reply` can route `AlreadyExists` →
/// `FSERR_ALREADY_EXISTS`, `NotFound` → `FSERR_NOT_FOUND`, etc.
fn map_open_err(e: std::io::Error) -> QuarantineError {
    match e.raw_os_error() {
        Some(libc::ELOOP) => QuarantineError::SymlinkComponent,
        _ => QuarantineError::IoError(e.kind(), io_msg_scrub(&e)),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;

    use tempfile::TempDir;

    use super::*;

    fn assert_err(actual: Result<SafeParent, QuarantineError>, expected: QuarantineError) {
        match actual {
            Ok(_) => panic!("expected {expected:?}, got Ok"),
            Err(e) => assert_eq!(e, expected),
        }
    }

    // --- Input validation (hermetic-ish; require only a root dir) --

    #[test]
    fn safe_descend_rejects_empty() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        assert_err(safe_descend(&root, ""), QuarantineError::Empty);
    }

    #[test]
    fn safe_descend_rejects_parent_traversal() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        assert_err(
            safe_descend(&root, "../escape.txt"),
            QuarantineError::EscapesRoot,
        );
        assert_err(safe_descend(&root, "a/../b"), QuarantineError::EscapesRoot);
        assert_err(safe_descend(&root, "a/b/.."), QuarantineError::EscapesRoot);
    }

    #[test]
    fn safe_descend_rejects_absolute() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        assert_err(
            safe_descend(&root, "/etc/passwd"),
            QuarantineError::EscapesRoot,
        );
    }

    /// `rel` that resolves to just root after collapse (e.g. `.` or
    /// `./`) yields `RootSelf` — there's no leaf to point at, so
    /// the handler layer's `open` / `stat` / `unlink` can't proceed.
    #[test]
    fn safe_descend_rejects_root_self_after_collapse() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        assert_err(safe_descend(&root, "."), QuarantineError::RootSelf);
        assert_err(safe_descend(&root, "./"), QuarantineError::RootSelf);
    }

    // --- Percent-encoded parent-traversal (SEC regression pin) -----

    /// URL-parser-vs-filesystem-parser bug class: an HTTP server or
    /// URL router that decodes `%2e%2e` after its path-safety check
    /// accepts the request into the "safe" prefix but hands the OS
    /// an escape.  Rust's `Path::components()` operates on raw byte
    /// sequences on Unix (`OsStr::as_bytes`); it does NOT decode
    /// percent sequences.  Pin that behavior: a `rel` containing
    /// `%2e%2e` is a single `Normal` component with a literal
    /// filename, not a `..`.  Callers that want percent-decoding
    /// must do it explicitly upstream — and then their decoded
    /// input goes through this reject-`..` gate again.
    #[test]
    fn safe_descend_treats_percent_encoded_parent_as_literal() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        // The literal filename "%2e%2e" doesn't exist, so we expect
        // an IoError(NotFound) — not a rejection at the component
        // level.  Crucially, the descent DOES NOT treat it as
        // `..` / `EscapesRoot`.
        match safe_descend(&root, "%2e%2e/etc/passwd") {
            Err(QuarantineError::EscapesRoot) => {
                panic!("%2e%2e was decoded to `..` — URL-decode bug class");
            }
            Err(QuarantineError::IoError(kind, _)) => {
                assert_eq!(kind, io::ErrorKind::NotFound);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    // --- Happy path ------------------------------------------------

    #[test]
    fn safe_descend_returns_safe_parent_for_existing_leaf() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        fs::create_dir(root.join("d")).unwrap();
        fs::write(root.join("d/leaf.txt"), b"x").unwrap();

        let sp = safe_descend(&root, "d/leaf.txt").expect("descend succeeds");
        assert_eq!(
            sp.leaf.to_str().unwrap(),
            "leaf.txt",
            "leaf CString must be the basename"
        );
        // The dirfd points at `d`; verify by fstatat'ing the leaf
        // against the returned dirfd.
        // SAFETY: sp.dirfd is a live OwnedFd; sp.leaf is a valid
        // NUL-terminated CString; both outlive the call.
        unsafe {
            let mut sb: libc::stat = std::mem::zeroed();
            let rc = libc::fstatat(sp.as_raw_fd(), sp.leaf_ptr(), &mut sb, 0);
            assert_eq!(rc, 0, "fstatat via SafeParent must find the leaf");
        }
    }

    // --- Symlink defense ------------------------------------------

    /// If any intermediate component is a symlink, descent must
    /// short-circuit with `SymlinkComponent` (regardless of what
    /// the symlink targets).  This is the O_NOFOLLOW guarantee.
    #[test]
    fn safe_descend_rejects_symlink_intermediate() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        // Create a real target dir with a file, and a symlink at
        // `root/link` pointing to the target dir.
        let target = root.join("target");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("leaf.txt"), b"x").unwrap();
        symlink(&target, root.join("link")).unwrap();

        assert_err(
            safe_descend(&root, "link/leaf.txt"),
            QuarantineError::SymlinkComponent,
        );
    }

    /// If the leaf itself is a symlink, descent still succeeds
    /// (the leaf resolution happens via `*at` on the parent dirfd,
    /// and it's the handler's decision what to do with a leaf that
    /// turns out to be a symlink).  Pin this so a well-intentioned
    /// "fix" that broadens O_NOFOLLOW to reject leaf symlinks
    /// during descent doesn't silently break `unlink`-a-symlink
    /// use cases.
    #[test]
    fn safe_descend_accepts_symlink_at_leaf_position() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        fs::write(root.join("real.txt"), b"x").unwrap();
        symlink(root.join("real.txt"), root.join("link.txt")).unwrap();

        let sp = safe_descend(&root, "link.txt").expect("leaf-symlink descend succeeds");
        assert_eq!(sp.leaf.to_str().unwrap(), "link.txt");
    }

    // --- H-5 root identity verification ---------------------------

    /// With a matching `expected_root_id`, descent proceeds normally.
    #[test]
    fn safe_descend_verified_accepts_matching_root_identity() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        fs::write(root.join("leaf.txt"), b"x").unwrap();

        // Capture the root's actual (dev, inode) via a probe open.
        let probe = open_dir(&root, true).unwrap();
        let expected = fstat_dev_inode(probe.as_raw_fd()).unwrap();
        drop(probe);

        let sp = safe_descend_verified(&root, "leaf.txt", Some(expected))
            .expect("matching identity descend succeeds");
        assert_eq!(sp.leaf.to_str().unwrap(), "leaf.txt");
    }

    /// A mismatched `expected_root_id` surfaces
    /// `QuarantineError::RootIdentityChanged` — simulating the H-5
    /// rename-and-recreate scenario where the root's inode has
    /// changed since boot.
    #[test]
    fn safe_descend_verified_rejects_mismatched_root_identity() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        fs::write(root.join("leaf.txt"), b"x").unwrap();

        // A guaranteed-wrong expected pair.
        let wrong = (u64::MAX, u64::MAX);
        assert_err(
            safe_descend_verified(&root, "leaf.txt", Some(wrong)),
            QuarantineError::RootIdentityChanged,
        );
    }

    /// Scenario pin for the actual H-5 attack shape: capture the
    /// root's real identity, then simulate a rename-and-recreate
    /// (`mv legit legit.bak && mkdir legit && populate`).  The new
    /// `legit` directory has a fresh inode; descent with the
    /// original captured identity must surface
    /// `RootIdentityChanged`.  Complements the synthetic
    /// `(u64::MAX, u64::MAX)` fast-check above with a realistic
    /// end-to-end reproduction of the attack the H-5 defense
    /// exists to close.
    #[test]
    fn safe_descend_verified_rejects_real_rename_and_recreate() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("legit");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("leaf.txt"), b"real").unwrap();
        let root = root.canonicalize().unwrap();

        // Capture the original root identity.
        let probe = open_dir(&root, true).unwrap();
        let orig_id = fstat_dev_inode(probe.as_raw_fd()).unwrap();
        drop(probe);

        // Rename-and-recreate: move the original away, create a
        // fresh directory in its place.  The fresh mkdir yields a
        // new inode on every supported filesystem.
        let backup = tmp.path().join("legit.bak");
        fs::rename(&root, &backup).unwrap();
        fs::create_dir(&root).unwrap();
        fs::write(root.join("leaf.txt"), b"attacker").unwrap();

        // Descent with the ORIGINAL identity must reject the fresh root.
        assert_err(
            safe_descend_verified(&root, "leaf.txt", Some(orig_id)),
            QuarantineError::RootIdentityChanged,
        );
    }

    /// Explicit pin for `expected_root_id = None` — the H-5 branch
    /// is skipped entirely.  Complements the H-5 tests by ensuring
    /// the two-branch behavior (Some → verify, None → skip) is
    /// exercised both ways.
    #[test]
    fn safe_descend_verified_with_none_identity_skips_h5_check() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        fs::write(root.join("leaf.txt"), b"x").unwrap();

        let sp = safe_descend_verified(&root, "leaf.txt", None)
            .expect("None identity means H-5 skipped, descent succeeds");
        assert_eq!(sp.leaf.to_str().unwrap(), "leaf.txt");
    }

    // --- Directory-at-leaf happy path -----------------------------

    /// If the leaf itself is a directory, descent still succeeds
    /// and returns a `SafeParent` pointing at the leaf's parent
    /// with the leaf's basename.  Handlers like `fs_remove_dir` /
    /// `fs_stat` on a subdirectory rely on this shape.
    #[test]
    fn safe_descend_accepts_directory_at_leaf_position() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        fs::create_dir(root.join("subdir")).unwrap();

        let sp = safe_descend(&root, "subdir").expect("dir-leaf descend succeeds");
        assert_eq!(sp.leaf.to_str().unwrap(), "subdir");

        // fstatat via SafeParent must report the leaf is a directory.
        // SAFETY: sp.dirfd is a live OwnedFd; sp.leaf is a valid
        // NUL-terminated CString; both outlive the call.
        unsafe {
            let mut sb: libc::stat = std::mem::zeroed();
            let rc = libc::fstatat(sp.as_raw_fd(), sp.leaf_ptr(), &mut sb, 0);
            assert_eq!(rc, 0);
            assert_eq!(
                sb.st_mode & libc::S_IFMT,
                libc::S_IFDIR,
                "leaf should be a directory"
            );
        }
    }

    // --- NUL-byte-in-component (invalid input) --------------------

    /// A NUL byte inside a path component is invalid input, not a
    /// root escape — the `CString::new` failure must route through
    /// `IoError(InvalidInput, _)` so the caller sees the right
    /// FSERR (`FSERR_BAD_ARG`) with a diagnostic message, not the
    /// misleading `FSERR_QUARANTINE "path escapes root"`.
    #[test]
    fn safe_descend_nul_in_component_surfaces_invalid_input() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        match safe_descend(&root, "a\0b") {
            Err(QuarantineError::IoError(kind, _)) => {
                assert_eq!(
                    kind,
                    io::ErrorKind::InvalidInput,
                    "NUL in component must classify as InvalidInput"
                );
            }
            other => panic!("expected IoError(InvalidInput), got {other:?}"),
        }
    }
}
