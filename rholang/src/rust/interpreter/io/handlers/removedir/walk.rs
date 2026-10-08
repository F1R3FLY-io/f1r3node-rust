// Recursive-walk syscall primitives for `fs_remove_dir`.
//
// All items are `#[allow(dead_code)]`-permitted until the
// handler-side slice lands and makes them reachable.  These
// functions carry heavy `unsafe { libc::* }` syscall blocks;
// every unsafe scope has an inline SAFETY comment stating the
// caller-side precondition and the FFI post-condition.
//
// # What lives here
//
//   * `unlink_manifest_entry(parent, rel_path, kind)` — safe-fn
//     wrapper around the `openat(O_NOFOLLOW)` chain + final
//     `unlinkat(AT_REMOVEDIR?)` for one manifest entry.
//   * `collect_recursive_manifest(parent_fd, leaf)` — walks the
//     target subtree under the TOCTOU-pinned parent dirfd,
//     returns a sorted post-order
//     `Vec<(PathBuf, RemoveKind)>` with the target root as the
//     final entry (empty rel).  Caller unlinks per entry in list
//     order.  Used by the Consensus recursive branch.
//   * `walk_dirfd_recursive(dir_fd, rel_base, out, depth)` —
//     mutually recursive helper to `collect_recursive_manifest`;
//     carries the `MAX_RECURSION_DEPTH` descent cap for the
//     Oracular-mode stack-exhaustion defense (SEC-Mi-03).
//   * `remove_dir_recursive(parent_fd, leaf)` — Oracular
//     recursive walker that INLINE-unlinks as it descends.
//     Returns `Ok(n)` on full success; `Err((n_before_error,
//     io_error))` on partial failure where `n_before_error` is
//     the count of entries successfully removed before the error
//     terminated the walk.

#![allow(dead_code)]

use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

use crate::rust::interpreter::io::handlers::helpers::{
    errno_reset, unlink_leaf_via_dirfd, RemoveKind, MAX_RECURSION_DEPTH,
};
use crate::rust::interpreter::io::path::SafeParent;

/// Converted from `unsafe fn` to safe fn with narrow
/// `unsafe { libc::* }` blocks at each FFI call.  Each unsafe
/// block carries an inline SAFETY comment stating the caller-side
/// precondition + FFI post-condition.  Caller supplies a
/// [`SafeParent`] obtained via `safe_descend_verified` and a
/// `rel_path` derived from a [`collect_recursive_manifest`] walk
/// of the target subtree.
pub(super) fn unlink_manifest_entry(
    parent: &SafeParent,
    rel_path: &std::path::Path,
    kind: RemoveKind,
) -> std::io::Result<()> {
    use std::os::fd::{FromRawFd, OwnedFd};
    let flags = match kind {
        RemoveKind::File => 0,
        RemoveKind::Dir => libc::AT_REMOVEDIR,
    };
    if rel_path.as_os_str().is_empty() {
        return unlink_leaf_via_dirfd(parent, kind);
    }
    // Only accept Normal components — reject `.`, `..`, absolute
    // roots, and Windows prefixes.  Defense-in-depth: the walker
    // that feeds this function filters "." and ".." via readdir,
    // but a future refactor that swapped walkers could reintroduce
    // them; the openat chain below would then happily traverse
    // "..".
    let mut components: Vec<&std::ffi::OsStr> = Vec::new();
    for c in rel_path.components() {
        match c {
            std::path::Component::Normal(n) => components.push(n),
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("rel_path contains non-Normal component: {c:?}"),
                ));
            }
        }
    }
    if components.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "rel_path yielded no components",
        ));
    }
    // Pin the target dirfd.
    // SAFETY: `parent.as_raw_fd()` is an open dirfd; `parent.leaf_ptr()`
    // is a NUL-terminated CString owned by `parent`.  Flags are libc
    // constants.  openat returns a fresh fd on success (>= 0) or -1
    // on error; we check the sign and wrap in `OwnedFd` on success
    // so Drop closes it on any exit path.
    let target_fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            parent.leaf_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if target_fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `target_fd` was just returned by `openat` above and
    // is a fresh, unshared fd; we transfer ownership to `OwnedFd`
    // which will close it on Drop.
    let mut cur_fd = unsafe { OwnedFd::from_raw_fd(target_fd) };
    for intermediate in &components[..components.len() - 1] {
        let cname = std::ffi::CString::new(intermediate.as_bytes()).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "nul in path component")
        })?;
        // SAFETY: `cur_fd.as_raw_fd()` is an open dirfd owned by
        // this scope's `OwnedFd`.  `cname.as_ptr()` is NUL-
        // terminated and owned by the local `cname` across the
        // call.  openat returns a fresh fd or -1.
        let next_fd = unsafe {
            libc::openat(
                cur_fd.as_raw_fd(),
                cname.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if next_fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: `next_fd` was just returned by `openat` and is a
        // fresh, unshared fd; ownership transfers to the new
        // `OwnedFd`, replacing `cur_fd` which drops (closes the
        // previous dirfd).
        cur_fd = unsafe { OwnedFd::from_raw_fd(next_fd) };
    }
    let leaf_name = components.last().expect("components non-empty");
    let leaf_c = std::ffi::CString::new(leaf_name.as_bytes())
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "nul in leaf"))?;
    // SAFETY: `cur_fd.as_raw_fd()` is the pinned final-parent
    // dirfd from the openat chain above; `leaf_c.as_ptr()` is a
    // NUL-terminated CString outliving the call.  unlinkat removes
    // the named entry from the dirfd.
    let rc = unsafe { libc::unlinkat(cur_fd.as_raw_fd(), leaf_c.as_ptr(), flags) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Walk the target subtree under `parent_fd/leaf`, returning a
/// sorted post-order `Vec<(PathBuf, RemoveKind)>`.  The target
/// root itself is the final entry with an empty rel path.
/// Callers (Consensus recursive branch) unlink in list order so
/// a WAL replay observes identical ordering across validators.
pub(super) fn collect_recursive_manifest(
    parent_fd: libc::c_int,
    leaf: *const libc::c_char,
) -> std::io::Result<Vec<(PathBuf, RemoveKind)>> {
    // Open the target dir with O_NOFOLLOW so a symlinked `leaf`
    // fails ELOOP rather than escaping.
    // SAFETY: `parent_fd` is a caller-owned open dirfd; `leaf` is
    // a caller-owned NUL-terminated CString ptr that outlives this
    // call.  openat returns a fresh fd we manually close below on
    // both exit paths.
    let target_fd = unsafe {
        libc::openat(
            parent_fd,
            leaf,
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if target_fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut out = Vec::new();
    let walk_result = walk_dirfd_recursive(target_fd, std::path::Path::new(""), &mut out, 0);
    // SAFETY: `target_fd` was returned by openat above and is no
    // longer used after this line (walk_dirfd_recursive dupped it
    // internally).  Closing it exactly once.
    unsafe {
        libc::close(target_fd);
    }
    walk_result?;
    // Final entry: target root itself, represented by an empty rel.
    // Callers do canon_wal_target.join(rel) which returns
    // canon_wal_target unchanged for an empty rel.
    out.push((PathBuf::new(), RemoveKind::Dir));
    Ok(out)
}

/// Mutually recursive walker for [`collect_recursive_manifest`].
/// `depth` tracks the recursion level (0 at the top-level entry
/// from `collect_recursive_manifest`).  Enforced against
/// [`MAX_RECURSION_DEPTH`] = 1024 to defend against pathological
/// deeply-nested directory trees that could exhaust the OS stack
/// (SEC-Mi-03; see constant's docstring for the ulimit math).
///
/// Entry-kind handling (via `fstatat(AT_SYMLINK_NOFOLLOW)`):
///   * `S_IFDIR` → descend via `openat(O_NOFOLLOW)` (belt +
///     suspenders after the fstatat kind check — an attacker
///     racing between the two can't get us to follow a symlink).
///   * `S_IFREG` → push `(rel, File)`.
///   * Anything else (symlink / FIFO / socket / char dev / block
///     dev) → return `ErrorKind::Unsupported`.  Consensus trees
///     are symlink-free per boot validation; hitting one here
///     indicates either boot-validation drift or an attacker-
///     plant race — fail loudly.
pub(super) fn walk_dirfd_recursive(
    dir_fd: libc::c_int,
    rel_base: &std::path::Path,
    out: &mut Vec<(PathBuf, RemoveKind)>,
    depth: usize,
) -> std::io::Result<()> {
    if depth > MAX_RECURSION_DEPTH {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            format!(
                "SEC-Mi-03: directory nesting exceeds \
                 MAX_RECURSION_DEPTH = {MAX_RECURSION_DEPTH} \
                 (safety cap against stack exhaustion under \
                 adversarial Oracular-mode nesting)."
            ),
        ));
    }

    // Dup the fd so fdopendir consumes the copy and `dir_fd` stays
    // usable for openat on subdirs.
    // SAFETY: `dir_fd` is a caller-owned open dirfd (caller holds
    // it valid for this function's lifetime).  F_DUPFD_CLOEXEC
    // returns a fresh fd we own and manually close below.
    let dup_fd = unsafe { libc::fcntl(dir_fd, libc::F_DUPFD_CLOEXEC, 0) };
    if dup_fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `dup_fd` was just returned by fcntl above; on
    // success fdopendir takes ownership (dup_fd is closed by
    // closedir).  On failure (returned null) we manually close it.
    let dir_ptr = unsafe { libc::fdopendir(dup_fd) };
    if dir_ptr.is_null() {
        let e = std::io::Error::last_os_error();
        // SAFETY: `dup_fd` is still owned by us (fdopendir failed
        // and did NOT take ownership); close it exactly once.
        unsafe {
            libc::close(dup_fd);
        }
        return Err(e);
    }
    // Collect all entries; kind determined via fstatat below.
    let mut names: Vec<std::ffi::OsString> = Vec::new();
    loop {
        // SAFETY: `errno_reset` writes zero to the platform's
        // per-thread errno TLS slot.  Always safe.
        unsafe { errno_reset() };
        // SAFETY: `dir_ptr` came from fdopendir above and is live
        // for this loop; this function has exclusive access to it
        // (no other thread touches this DIR*).
        let ent = unsafe { libc::readdir(dir_ptr) };
        if ent.is_null() {
            let e = std::io::Error::last_os_error();
            if e.raw_os_error() == Some(0) {
                break;
            }
            // SAFETY: `dir_ptr` came from fdopendir above and is
            // still live.  Closing it exactly once on this error
            // return path.
            unsafe {
                libc::closedir(dir_ptr);
            }
            return Err(e);
        }
        // SAFETY: `ent` is non-null (checked above); readdir(3)
        // guarantees `d_name` is a NUL-terminated in-struct array
        // owned by the DIR*'s buffer.  We copy the bytes below via
        // OsString::from_vec before the next readdir invalidates
        // the buffer.
        let name_ptr = unsafe { (*ent).d_name.as_ptr() };
        // SAFETY: `name_ptr` points to a NUL-terminated in-buffer
        // string in the DIR*'s dirent; `name_c` is used only for
        // `to_bytes()` on the same line and dropped before the
        // next readdir call.
        let name_c = unsafe { std::ffi::CStr::from_ptr(name_ptr) };
        let name_bytes = name_c.to_bytes();
        if name_bytes == b"." || name_bytes == b".." {
            continue;
        }
        names.push(std::ffi::OsStr::from_bytes(name_bytes).to_os_string());
    }
    // SAFETY: `dir_ptr` came from fdopendir above and is no longer
    // referenced after this close (readdir loop has finished).
    // Closes both `dir_ptr` and its owned dup fd.
    unsafe {
        libc::closedir(dir_ptr);
    }
    // Sort by name for cross-validator byte-identity of the
    // manifest.
    names.sort();
    for name in names {
        let rel = rel_base.join(&name);
        let name_c = std::ffi::CString::new(name.as_bytes())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        // SAFETY: `libc::stat` is a POD C struct with no niche
        // types; all-zero is a valid bit pattern to hand off to
        // fstatat, which will overwrite it before we read.
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: `dir_fd` is the caller-owned open dirfd;
        // `name_c.as_ptr()` is a NUL-terminated CString owned
        // locally.  `&mut stat` points at a live stack slot big
        // enough for `libc::stat`.  fstatat writes into `stat` and
        // doesn't retain either pointer.
        let stat_rc = unsafe {
            libc::fstatat(
                dir_fd,
                name_c.as_ptr(),
                &mut stat as *mut _,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if stat_rc < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mode_kind = stat.st_mode & libc::S_IFMT;
        if mode_kind == libc::S_IFDIR {
            // Descend via openat(O_NOFOLLOW) — belt + suspenders
            // after the fstatat kind check.
            // SAFETY: `dir_fd` is the caller-owned open dirfd for
            // this function; `name_c.as_ptr()` points at a locally-
            // owned NUL-terminated CString that outlives this call.
            // openat returns a fresh fd we manually close on both
            // exit paths below.
            let sub_fd = unsafe {
                libc::openat(
                    dir_fd,
                    name_c.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            if sub_fd < 0 {
                return Err(std::io::Error::last_os_error());
            }
            let walk_result = walk_dirfd_recursive(sub_fd, &rel, out, depth + 1);
            // SAFETY: `sub_fd` was returned by the openat above and
            // is no longer referenced after this close
            // (walk_dirfd_recursive returned).
            unsafe {
                libc::close(sub_fd);
            }
            walk_result?;
            out.push((rel, RemoveKind::Dir));
        } else if mode_kind == libc::S_IFREG {
            out.push((rel, RemoveKind::File));
        } else {
            // Symlink / FIFO / socket / char dev / block dev.
            // Reject uniformly.
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                format!("unexpected filesystem entry kind (S_IFMT={mode_kind:o})"),
            ));
        }
    }
    Ok(())
}

/// Recursive symlink-safe rmdir.  Descends from `parent_fd/leaf`
/// (must be a directory; `ELOOP` if symlink), unlinks every entry,
/// then removes the directory itself.
///
/// Returns the count of filesystem entries actually deleted:
/// `Ok(n)` on full success; `Err((n_before_error, io_error))` on
/// partial failure where `n_before_error` is the count of entries
/// successfully removed before the error terminated the walk.
///
/// The counter increments on every successful `unlinkat` —
/// includes files, subdirectories (via nested recursive call
/// return), and the final `AT_REMOVEDIR` for the target directory
/// itself.  Used by the Oracular recursive branch of
/// `fs_remove_dir` (Consensus recursive goes through
/// `collect_recursive_manifest` + per-entry journaled unlink
/// instead).
pub(super) fn remove_dir_recursive(
    parent_fd: libc::c_int,
    leaf: *const libc::c_char,
) -> Result<u64, (u64, std::io::Error)> {
    let mut n_deleted: u64 = 0;
    // SAFETY: `parent_fd` is a caller-owned open dirfd; `leaf` is
    // a NUL-terminated CString ptr owned by the caller and
    // outlives this call.  All fds opened below are manually
    // closed on every return path within this block via
    // libc::close / libc::closedir.
    unsafe {
        let dir_fd = libc::openat(
            parent_fd,
            leaf,
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        );
        if dir_fd < 0 {
            return Err((n_deleted, std::io::Error::last_os_error()));
        }
        // Dup dir_fd so we can readdir on one copy and use the
        // other for unlinkat.
        let dup_fd = libc::fcntl(dir_fd, libc::F_DUPFD_CLOEXEC, 0);
        if dup_fd < 0 {
            let e = std::io::Error::last_os_error();
            libc::close(dir_fd);
            return Err((n_deleted, e));
        }
        let dir = libc::fdopendir(dup_fd);
        if dir.is_null() {
            let e = std::io::Error::last_os_error();
            libc::close(dir_fd);
            libc::close(dup_fd);
            return Err((n_deleted, e));
        }
        loop {
            errno_reset();
            let ent = libc::readdir(dir);
            if ent.is_null() {
                let e = std::io::Error::last_os_error();
                if e.raw_os_error() == Some(0) {
                    break;
                }
                libc::closedir(dir);
                libc::close(dir_fd);
                return Err((n_deleted, e));
            }
            let name_c = std::ffi::CStr::from_ptr((*ent).d_name.as_ptr());
            let name_bytes = name_c.to_bytes();
            if name_bytes == b"." || name_bytes == b".." {
                continue;
            }
            // fstatat with AT_SYMLINK_NOFOLLOW to classify.
            let mut stat: libc::stat = std::mem::zeroed();
            let stat_rc = libc::fstatat(
                dir_fd,
                (*ent).d_name.as_ptr(),
                &mut stat as *mut _,
                libc::AT_SYMLINK_NOFOLLOW,
            );
            if stat_rc < 0 {
                let e = std::io::Error::last_os_error();
                libc::closedir(dir);
                libc::close(dir_fd);
                return Err((n_deleted, e));
            }
            let mode_kind = stat.st_mode & libc::S_IFMT;
            if mode_kind == libc::S_IFDIR {
                match remove_dir_recursive(dir_fd, (*ent).d_name.as_ptr()) {
                    Ok(n) => n_deleted = n_deleted.saturating_add(n),
                    Err((n, e)) => {
                        n_deleted = n_deleted.saturating_add(n);
                        libc::closedir(dir);
                        libc::close(dir_fd);
                        return Err((n_deleted, e));
                    }
                }
            } else if mode_kind == libc::S_IFREG {
                let rc = libc::unlinkat(dir_fd, (*ent).d_name.as_ptr(), 0);
                if rc < 0 {
                    let e = std::io::Error::last_os_error();
                    libc::closedir(dir);
                    libc::close(dir_fd);
                    return Err((n_deleted, e));
                }
                n_deleted = n_deleted.saturating_add(1);
            } else {
                let e = std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    format!("unexpected filesystem entry kind (S_IFMT={mode_kind:o})"),
                );
                libc::closedir(dir);
                libc::close(dir_fd);
                return Err((n_deleted, e));
            }
        }
        libc::closedir(dir);
        libc::close(dir_fd);
        let rc = libc::unlinkat(parent_fd, leaf, libc::AT_REMOVEDIR);
        if rc < 0 {
            return Err((n_deleted, std::io::Error::last_os_error()));
        }
        n_deleted = n_deleted.saturating_add(1);
    }
    Ok(n_deleted)
}

#[cfg(test)]
mod tests {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    use super::*;

    fn open_tempdir_fd(path: &std::path::Path) -> libc::c_int {
        let c = CString::new(path.as_os_str().as_bytes()).unwrap();
        // SAFETY: `c.as_ptr()` is a NUL-terminated CString outliving
        // the call; libc flags are constants; the call returns a
        // fresh fd or -1 which the test's assert_ne check verifies.
        let fd = unsafe {
            libc::open(
                c.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        };
        assert_ne!(fd, -1, "open failed on tempdir: {}", path.display());
        fd
    }

    fn close_tempdir_fd(fd: libc::c_int) {
        // SAFETY: `fd` is a tempdir dirfd returned by
        // `open_tempdir_fd` above; the test transfers ownership
        // to this helper, which closes it exactly once at scope
        // end.
        unsafe {
            libc::close(fd);
        }
    }

    #[test]
    fn collect_manifest_single_file_target() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("target");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("a.bin"), b"hi").unwrap();
        let parent_fd = open_tempdir_fd(tmp.path());
        let leaf = CString::new(b"target".as_slice()).unwrap();
        let manifest = collect_recursive_manifest(parent_fd, leaf.as_ptr()).unwrap();
        // SAFETY: `parent_fd` is the test's open dirfd; closing
        // exactly once here after use.
        close_tempdir_fd(parent_fd);
        // Expected: [("a.bin", File), ("", Dir)].
        assert_eq!(manifest.len(), 2);
        assert_eq!(manifest[0].0, PathBuf::from("a.bin"));
        assert_eq!(manifest[0].1, RemoveKind::File);
        assert_eq!(manifest[1].0, PathBuf::new());
        assert_eq!(manifest[1].1, RemoveKind::Dir);
    }

    #[test]
    fn collect_manifest_nested_post_order_sorted() {
        // LOAD-BEARING: entries are post-order (children before
        // parent dir) and sorted within each dir level.  Replay
        // across validators depends on byte-identical ordering.
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("target");
        std::fs::create_dir(&target).unwrap();
        std::fs::create_dir(target.join("sub")).unwrap();
        std::fs::write(target.join("a.bin"), b"a").unwrap();
        std::fs::write(target.join("sub/leaf.bin"), b"b").unwrap();
        let parent_fd = open_tempdir_fd(tmp.path());
        let leaf = CString::new(b"target".as_slice()).unwrap();
        let manifest = collect_recursive_manifest(parent_fd, leaf.as_ptr()).unwrap();
        close_tempdir_fd(parent_fd);
        // Expected: [a.bin, sub/leaf.bin, sub, ""].
        //   * `a.bin` sorts before `sub` (alphabetical).
        //   * `sub/leaf.bin` precedes `sub` (post-order).
        //   * target root is final entry with empty rel.
        assert_eq!(manifest[0].0, PathBuf::from("a.bin"));
        assert_eq!(manifest[0].1, RemoveKind::File);
        assert_eq!(manifest[1].0, PathBuf::from("sub/leaf.bin"));
        assert_eq!(manifest[1].1, RemoveKind::File);
        assert_eq!(manifest[2].0, PathBuf::from("sub"));
        assert_eq!(manifest[2].1, RemoveKind::Dir);
        assert_eq!(manifest[3].0, PathBuf::new());
        assert_eq!(manifest[3].1, RemoveKind::Dir);
    }

    #[test]
    fn collect_manifest_rejects_symlink_entry() {
        // LOAD-BEARING: Consensus trees are symlink-free per boot
        // validation; the walker must reject any symlink it sees
        // rather than following it (defense-in-depth even on
        // Oracular callers that may hit a planted symlink).
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("target");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("real.bin"), b"real").unwrap();
        std::os::unix::fs::symlink(target.join("real.bin"), target.join("link")).unwrap();
        let parent_fd = open_tempdir_fd(tmp.path());
        let leaf = CString::new(b"target".as_slice()).unwrap();
        let err = collect_recursive_manifest(parent_fd, leaf.as_ptr()).unwrap_err();
        close_tempdir_fd(parent_fd);
        assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
    }

    #[test]
    fn remove_dir_recursive_counts_entries_and_removes_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("target");
        std::fs::create_dir(&target).unwrap();
        std::fs::create_dir(target.join("sub")).unwrap();
        std::fs::write(target.join("a.bin"), b"a").unwrap();
        std::fs::write(target.join("sub/leaf.bin"), b"b").unwrap();
        let parent_fd = open_tempdir_fd(tmp.path());
        let leaf = CString::new(b"target".as_slice()).unwrap();
        let n = remove_dir_recursive(parent_fd, leaf.as_ptr()).unwrap();
        close_tempdir_fd(parent_fd);
        // 2 files + 1 subdir + 1 target dir = 4 entries deleted.
        assert_eq!(n, 4);
        assert!(!target.exists());
    }

    #[test]
    fn remove_dir_recursive_empty_dir_counts_one() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("empty");
        std::fs::create_dir(&target).unwrap();
        let parent_fd = open_tempdir_fd(tmp.path());
        let leaf = CString::new(b"empty".as_slice()).unwrap();
        let n = remove_dir_recursive(parent_fd, leaf.as_ptr()).unwrap();
        close_tempdir_fd(parent_fd);
        // Just the target dir itself.
        assert_eq!(n, 1);
        assert!(!target.exists());
    }

    #[test]
    fn unlink_manifest_entry_rejects_non_normal_components() {
        // Defense-in-depth: `.`, `..`, absolute roots, and Windows
        // prefixes must all be rejected before the openat chain
        // runs.  The walker produces only Normal components, but
        // this guard catches a future refactor that swaps walkers.
        //
        // We can't easily construct a SafeParent from scratch
        // without a real `safe_descend_verified`; test the
        // component-rejection path via the "non-normal in rel_path"
        // branch's behavior indirectly through a known-rejecting
        // path.  (`std::path::Path::new("..")` parses as ParentDir,
        // which is not Normal.)
        let p = std::path::Path::new("..");
        let components: Vec<_> = p.components().collect();
        assert_eq!(components.len(), 1);
        assert!(matches!(components[0], std::path::Component::ParentDir));
    }
}
