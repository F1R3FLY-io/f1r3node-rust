// TOCTOU-immune file open at a leaf under a canonicalized [`Root`].
//
// Given a [`Root`] + a relative `rel`, walks the root via
// [`super::descend::safe_descend`] (which enforces the H-5
// identity check and O_NOFOLLOW-at-every-intermediate), then opens
// the leaf via `openat` against the returned parent dirfd with
// `flags | O_NOFOLLOW | O_CLOEXEC` and the caller-supplied
// creation `mode`.
//
// # Why force O_NOFOLLOW on the leaf
//
// Every handler that opens a *file* wants to fail on a symlink
// leaf — following would let an attacker redirect the open into
// arbitrary files.  Callers that legitimately need "open this
// exact symlink as a symlink" would need `O_PATH` (Linux-only),
// which isn't a use case for this module today.
//
// # Divergence from `safe_descend`
//
// [`safe_descend`] *accepts* a symlink at the leaf position and
// returns a [`SafeParent`] pointing at its parent — handlers like
// `unlinkat` legitimately want to operate on a symlink leaf.
// `safe_open` is stricter because opening a file handle to a
// symlink target would defeat the point of restricted-root
// enforcement; the two functions serve different call sites.

use std::fs::File;
use std::os::fd::FromRawFd;
use std::path::Path;

use super::descend::{map_open_err, safe_descend, safe_descend_verified};
use super::identity::Root;
use super::QuarantineError;

/// Open `<root>/<rel>` for the caller-supplied `flags` and (for
/// `O_CREAT`) `mode`, with TOCTOU-immune descent to the leaf's
/// parent + `openat` against that parent's dirfd.  Forces
/// `O_NOFOLLOW | O_CLOEXEC` on the leaf open regardless of what
/// the caller passes (see the module doc).
///
/// Returns a `File` bound to the freshly-opened fd — its `Drop`
/// closes the fd, so callers get RAII lifetime management for
/// free.
///
/// # Errors
///
/// - Any descent-phase error surfaces the same way it does from
///   [`safe_descend`]: `Empty`, `EscapesRoot`, `RootSelf`,
///   `SymlinkComponent`, `RootIdentityChanged`,
///   `IoError(kind, msg)`.
/// - Leaf-phase errors map via [`map_open_err`]:
///   `ELOOP` → `SymlinkComponent` (leaf is a symlink; O_NOFOLLOW
///   fires); everything else → `IoError(kind, scrubbed_msg)` so
///   `NotFound` / `AlreadyExists` / `PermissionDenied` / etc.
///   reach their spec-canonical FSERR via
///   `quarantine_err_reply`.
pub fn safe_open(
    root: &Root,
    rel: &str,
    flags: libc::c_int,
    mode: libc::mode_t,
) -> Result<File, QuarantineError> {
    let parent = safe_descend(root, rel)?;
    // SAFETY: `parent` is a `SafeParent` holding an `OwnedFd`
    // (dirfd valid for the borrow) + a `CString` leaf name
    // (NUL-terminated, valid for the borrow).  `parent.leaf_ptr()`
    // returns a pointer with the same lifetime as `&parent`; both
    // outlive this call.  `openat` reads them without retention.
    // On success `fd` is a fresh open fd; `File::from_raw_fd`
    // takes ownership so `Drop` closes it.
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

/// Shape-A variant of [`safe_open`] that threads an
/// `expected_root_id: Option<(u64, u64)>` through
/// [`safe_descend_verified`] — the identity-check gated form.
///
/// `None` → ungated behavior (pre-Shape-A compat; identity check
/// is skipped at descent).  `Some((dev, ino))` → full M-04
/// identity check at every intermediate component.
///
/// Takes `&Path` for the root (not `&Root`), so the caller can
/// pass the on-disk root resolved by
/// `RootIdentityRegistry::resolve_or_identity`.
///
/// All other semantics (`O_NOFOLLOW | O_CLOEXEC` forced, descent-
/// phase error mapping, leaf-phase `ELOOP` → `SymlinkComponent`)
/// match [`safe_open`].  Used by fs_open's `open_impl_via_table`
/// — the handler-layer wrapper that resolves cmode + registry
/// before calling here.
pub fn safe_open_verified(
    root: &Path,
    rel: &str,
    flags: libc::c_int,
    mode: libc::mode_t,
    expected_root_id: Option<(u64, u64)>,
) -> Result<File, QuarantineError> {
    let parent = safe_descend_verified(root, rel, expected_root_id)?;
    // SAFETY: `parent` is a `SafeParent` RAII wrapper with an open
    // dirfd for its lifetime; `parent.leaf_ptr()` returns a NUL-
    // terminated `*const c_char` valid for the same lifetime.
    // `openat` reads them without retention.  On success `fd` is a
    // fresh open fd — `File::from_raw_fd` takes ownership (its
    // `Drop` closes the fd).
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{Read, Write};
    use std::os::unix::fs::symlink;

    use tempfile::TempDir;

    use super::*;

    fn assert_err(actual: Result<File, QuarantineError>, expected: QuarantineError) {
        match actual {
            Ok(_) => panic!("expected {expected:?}, got Ok"),
            Err(e) => assert_eq!(e, expected),
        }
    }

    // --- Happy path -----------------------------------------------

    #[test]
    fn safe_open_reads_existing_file() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("f.txt"), b"hello").unwrap();
        let root = Root::capture(tmp.path()).unwrap();

        let mut file = safe_open(&root, "f.txt", libc::O_RDONLY, 0).expect("open succeeds");
        let mut buf = String::new();
        file.read_to_string(&mut buf).unwrap();
        assert_eq!(buf, "hello");
    }

    /// `O_CREAT | O_EXCL` on a nonexistent path creates the file
    /// with the requested mode.  Pins the standard "atomic
    /// create-if-not-exists" idiom that `wx` mode maps to.
    #[test]
    fn safe_open_creates_new_file_with_o_creat_o_excl() {
        let tmp = TempDir::new().unwrap();
        let root = Root::capture(tmp.path()).unwrap();

        let mut file = safe_open(
            &root,
            "new.txt",
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
            0o644,
        )
        .expect("create-new succeeds");
        file.write_all(b"created").unwrap();
        drop(file);

        // Verify the file exists and has the expected content.
        let read_back = fs::read_to_string(tmp.path().join("new.txt")).unwrap();
        assert_eq!(read_back, "created");
    }

    #[test]
    fn safe_open_can_descend_into_subdirectory() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("d")).unwrap();
        fs::write(tmp.path().join("d/leaf.txt"), b"nested").unwrap();
        let root = Root::capture(tmp.path()).unwrap();

        let mut file = safe_open(&root, "d/leaf.txt", libc::O_RDONLY, 0).expect("descend + open");
        let mut buf = String::new();
        file.read_to_string(&mut buf).unwrap();
        assert_eq!(buf, "nested");
    }

    // --- Error routing --------------------------------------------

    /// `O_RDONLY` on a nonexistent path surfaces
    /// `IoError(NotFound, _)` — the openat itself fails after
    /// descent succeeds.  Pin the classification so
    /// `quarantine_err_reply` routes to `FSERR_NOT_FOUND`.
    #[test]
    fn safe_open_missing_file_surfaces_not_found() {
        let tmp = TempDir::new().unwrap();
        let root = Root::capture(tmp.path()).unwrap();

        match safe_open(&root, "nope.txt", libc::O_RDONLY, 0) {
            Err(QuarantineError::IoError(kind, _)) => {
                assert_eq!(kind, std::io::ErrorKind::NotFound);
            }
            other => panic!("expected IoError(NotFound), got {other:?}"),
        }
    }

    /// `O_CREAT | O_EXCL` on an existing path surfaces
    /// `IoError(AlreadyExists, _)`.  Pin the classification so
    /// `quarantine_err_reply` routes to `FSERR_ALREADY_EXISTS`
    /// — the code `wx` mode's create-if-not-exists idiom relies
    /// on.
    #[test]
    fn safe_open_creat_excl_on_existing_surfaces_already_exists() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("f.txt"), b"x").unwrap();
        let root = Root::capture(tmp.path()).unwrap();

        match safe_open(
            &root,
            "f.txt",
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
            0o644,
        ) {
            Err(QuarantineError::IoError(kind, _)) => {
                assert_eq!(kind, std::io::ErrorKind::AlreadyExists);
            }
            other => panic!("expected IoError(AlreadyExists), got {other:?}"),
        }
    }

    // --- Symlink defense ------------------------------------------

    /// The leaf being a symlink surfaces `SymlinkComponent` —
    /// `O_NOFOLLOW` is forced on the leaf open regardless of what
    /// flags the caller passed.  This differs from `safe_descend`,
    /// which accepts a leaf-symlink and returns a `SafeParent`
    /// (its callers can `unlinkat` a symlink).
    #[test]
    fn safe_open_rejects_symlink_at_leaf_position() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("target.txt"), b"secret").unwrap();
        symlink(tmp.path().join("target.txt"), tmp.path().join("link.txt")).unwrap();
        let root = Root::capture(tmp.path()).unwrap();

        assert_err(
            safe_open(&root, "link.txt", libc::O_RDONLY, 0),
            QuarantineError::SymlinkComponent,
        );
    }

    /// Intermediate symlink still surfaces `SymlinkComponent`
    /// during the descent phase (before we ever reach the leaf
    /// open).  Verifies the descent-level defense flows through
    /// `safe_open` correctly.
    #[test]
    fn safe_open_rejects_symlink_intermediate() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("target");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("f.txt"), b"x").unwrap();
        symlink(&target, tmp.path().join("link")).unwrap();
        let root = Root::capture(tmp.path()).unwrap();

        assert_err(
            safe_open(&root, "link/f.txt", libc::O_RDONLY, 0),
            QuarantineError::SymlinkComponent,
        );
    }

    // --- Descent-level error propagation --------------------------

    /// `rel` with `..` is rejected by `safe_descend` before the
    /// open ever runs.  Pin that safe_open surfaces the descent-
    /// level error unchanged.
    #[test]
    fn safe_open_propagates_escapes_root() {
        let tmp = TempDir::new().unwrap();
        let root = Root::capture(tmp.path()).unwrap();
        assert_err(
            safe_open(&root, "../etc/passwd", libc::O_RDONLY, 0),
            QuarantineError::EscapesRoot,
        );
    }

    #[test]
    fn safe_open_propagates_empty() {
        let tmp = TempDir::new().unwrap();
        let root = Root::capture(tmp.path()).unwrap();
        assert_err(
            safe_open(&root, "", libc::O_RDONLY, 0),
            QuarantineError::Empty,
        );
    }

    /// `rel` that resolves to the root itself (`.` / `./`) is
    /// rejected by `safe_descend` with `RootSelf` — there's no leaf
    /// to open.  Pin that `safe_open` surfaces this unchanged.
    #[test]
    fn safe_open_propagates_root_self() {
        let tmp = TempDir::new().unwrap();
        let root = Root::capture(tmp.path()).unwrap();
        assert_err(
            safe_open(&root, ".", libc::O_RDONLY, 0),
            QuarantineError::RootSelf,
        );
    }

    /// A descent-phase `IoError` (a missing intermediate directory
    /// fails inside `safe_descend`, before the leaf open is ever
    /// attempted) surfaces through `safe_open` with the descent-
    /// phase kind unchanged.  Complements
    /// `safe_open_missing_file_surfaces_not_found`, which exercises
    /// the *leaf*-phase `IoError` path.
    #[test]
    fn safe_open_propagates_descent_phase_io_error() {
        let tmp = TempDir::new().unwrap();
        let root = Root::capture(tmp.path()).unwrap();
        match safe_open(&root, "missing_dir/f.txt", libc::O_RDONLY, 0) {
            Err(QuarantineError::IoError(kind, _)) => {
                assert_eq!(kind, std::io::ErrorKind::NotFound);
            }
            other => panic!("expected IoError(NotFound) from descent phase, got {other:?}"),
        }
    }

    /// A file with mode `000` surfaces `IoError(PermissionDenied, _)`
    /// on `O_RDONLY` — pins the `FSERR_PERM` classification route.
    #[test]
    #[cfg(unix)]
    fn safe_open_no_read_permission_surfaces_permission_denied() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("locked.txt");
        fs::write(&path, b"secret").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
        let root = Root::capture(tmp.path()).unwrap();

        // Skip when running as root — root bypasses DAC read checks.
        // Cheap probe: try reading; if it succeeds we're root and the
        // permission bit is meaningless.
        if fs::File::open(&path).is_ok() {
            return;
        }

        match safe_open(&root, "locked.txt", libc::O_RDONLY, 0) {
            Err(QuarantineError::IoError(kind, _)) => {
                assert_eq!(kind, std::io::ErrorKind::PermissionDenied);
            }
            other => panic!("expected IoError(PermissionDenied), got {other:?}"),
        }
    }

    // --- H-5 defense flows through safe_open ----------------------

    /// The identity check runs (via `safe_descend`) even when the
    /// caller uses `safe_open` — rename-and-recreate of the root
    /// surfaces `RootIdentityChanged`, not a silent open into the
    /// attacker's tree.
    #[test]
    fn safe_open_rejects_rename_and_recreate() {
        let tmp = TempDir::new().unwrap();
        let root_path = tmp.path().join("legit");
        fs::create_dir(&root_path).unwrap();
        fs::write(root_path.join("f.txt"), b"real").unwrap();
        let root = Root::capture(&root_path).unwrap();

        // Rename-and-recreate.
        let backup = tmp.path().join("legit.bak");
        fs::rename(&root_path, &backup).unwrap();
        fs::create_dir(&root_path).unwrap();
        fs::write(root_path.join("f.txt"), b"attacker").unwrap();

        assert_err(
            safe_open(&root, "f.txt", libc::O_RDONLY, 0),
            QuarantineError::RootIdentityChanged,
        );
    }

    /// `File`'s `Drop` closes the fd — pin that safe_open's
    /// return value participates in RAII (regression scenario: a
    /// refactor that wraps the fd in a non-owning type would
    /// silently leak fds under load).
    #[test]
    fn safe_open_returned_file_drops_fd() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("f.txt"), b"x").unwrap();
        let root = Root::capture(tmp.path()).unwrap();

        // Open and drop many times; each iteration drops before
        // the next open, so at most 1 fd is held at once — the
        // test would pass even under a tiny `ulimit -n`.  What it
        // catches is a regression that *fails* to drop: without
        // Drop closing the fd, iteration N would hold N open fds
        // and hit EMFILE well before 1000 (macOS default
        // `ulimit -n` is 256; Linux is typically 1024).
        for _ in 0..1000 {
            let _f = safe_open(&root, "f.txt", libc::O_RDONLY, 0).unwrap();
        }
    }
}
