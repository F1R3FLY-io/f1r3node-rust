// Unlink primitives — `RemoveKind` discriminator + two helpers:
//
//   - `target_dev_inode_at(parent) -> Option<(u64, u64)>` — read
//     `(dev, inode)` for a leaf via `fstatat(AT_SYMLINK_NOFOLLOW)`
//     against the parent's dirfd.  Used by `fs_remove_file` /
//     `fs_remove_dir` to query the LockRegistry's unlink gate
//     (consensus-observable: `is_locked((dev, inode), (0, u64::MAX))`
//     under Consensus = `FSERR_BUSY`).
//   - `unlink_leaf_via_dirfd(parent, kind) -> io::Result<()>` —
//     `unlinkat` on the leaf via the parent's dirfd.  `kind`
//     chooses the flag: `File` → 0, `Dir` → `AT_REMOVEDIR`.
//
// # Why dirfd-based
//
// Both helpers take a [`SafeParent`](super::super::super::path::SafeParent)
// (from `safe_descend_verified`) and operate at the leaf relative
// to the parent's open dirfd.  This keeps the H-5 defense complete
// — the TOCTOU window between path lookup and the syscall is
// closed by the parent dirfd staying open across the call.  Path-
// string re-walks would reopen the attack surface.
//
// # No fail-closed on target_dev_inode_at
//
// `target_dev_inode_at` returns `None` if `fstatat` fails (e.g.,
// leaf doesn't exist, EACCES, etc.).  The caller (unlink gate)
// treats `None` as "not locked" — but the subsequent `unlinkat`
// will surface the appropriate error (ENOENT / EACCES / ...).
// Fail-open here is correct: a stat failure means the lock
// registry can't answer the question, but the unlink itself will
// hit the same underlying constraint and produce the right wire
// error.
//
// # RemoveKind
//
// Used by:
//
//   - `fs_remove_file` → `RemoveKind::File` (unlinkat flags = 0).
//   - `fs_remove_dir` (yet to land, trait-exempt per plan) →
//     `RemoveKind::Dir` (unlinkat flags = `AT_REMOVEDIR`).

use crate::rust::interpreter::io::path::SafeParent;

/// Kind marker for an `unlinkat` call — selects between regular-
/// file unlink (flags = 0) and directory unlink (flags =
/// `AT_REMOVEDIR`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoveKind {
    File,
    Dir,
}

/// Read `(dev, inode)` for the leaf under `parent`'s dirfd via
/// `fstatat(AT_SYMLINK_NOFOLLOW)`.  Returns `None` on fstatat
/// failure (treated as "no identity known" by the unlink gate —
/// the subsequent unlinkat will surface the underlying error).
pub fn target_dev_inode_at(parent: &SafeParent) -> Option<(u64, u64)> {
    // SAFETY: `parent` (a `SafeParent`) owns an open dirfd that
    // stays valid for `parent`'s lifetime; `parent.leaf_ptr()`
    // returns a NUL-terminated CString ptr owned by the same
    // `SafeParent`.  `libc::stat` is POD — zeroed is a valid
    // initializer.  `fstatat` writes into `sb` and does not retain
    // either pointer past the call.
    unsafe {
        let mut sb: libc::stat = std::mem::zeroed();
        if libc::fstatat(
            parent.as_raw_fd(),
            parent.leaf_ptr(),
            &mut sb,
            libc::AT_SYMLINK_NOFOLLOW,
        ) == 0
        {
            #[allow(clippy::unnecessary_cast)]
            Some((sb.st_dev as u64, sb.st_ino as u64))
        } else {
            None
        }
    }
}

/// `unlinkat` on the leaf under `parent`'s dirfd.  `kind` selects
/// the flag (0 for file, `AT_REMOVEDIR` for directory).  Returns
/// the libc error on failure.
pub fn unlink_leaf_via_dirfd(parent: &SafeParent, kind: RemoveKind) -> std::io::Result<()> {
    let flags = match kind {
        RemoveKind::File => 0,
        RemoveKind::Dir => libc::AT_REMOVEDIR,
    };
    // SAFETY: `parent.as_raw_fd()` is an open dirfd owned by the
    // caller's `SafeParent`; `parent.leaf_ptr()` is a NUL-
    // terminated CString buffer owned by the same `SafeParent`
    // and outlives this call.  `unlinkat` reads both and does not
    // retain either past return.
    let rc = unsafe { libc::unlinkat(parent.as_raw_fd(), parent.leaf_ptr(), flags) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `RemoveKind` has exactly the two variants expected by the
    /// unlinkat flag mapping.  Pin against a drift that added a
    /// third variant without updating `unlink_leaf_via_dirfd`'s
    /// match (which would be a compile error, so this is just a
    /// shape pin).
    #[test]
    fn remove_kind_variants_are_file_and_dir() {
        let file = RemoveKind::File;
        let dir = RemoveKind::Dir;
        assert_ne!(file, dir);
        // Exhaustive match on the two variants — if a third is
        // added, this breaks before it reaches dispatch sites.
        for k in [file, dir] {
            match k {
                RemoveKind::File | RemoveKind::Dir => {}
            }
        }
    }

    /// Runtime behavior of `target_dev_inode_at` and
    /// `unlink_leaf_via_dirfd` requires a `SafeParent` fixture
    /// (temp dir + descent).  Deferred to integration tests —
    /// the surfaces are consensus-safety-critical and need a real
    /// filesystem to exercise.  The two helpers' logic is a thin
    /// wrapper around libc syscalls; the pre-trait fileio tests
    /// cover the behavior and migrate forward unchanged.
    #[test]
    fn runtime_tests_deferred_to_integration() {
        // Compile-time witness that both helpers can be referenced
        // from a `SafeParent`-shaped context.  A regression that
        // broke the module's export surface would trip here.
        let _: fn(&SafeParent) -> Option<(u64, u64)> = target_dev_inode_at;
        let _: fn(&SafeParent, RemoveKind) -> std::io::Result<()> = unlink_leaf_via_dirfd;
    }
}
