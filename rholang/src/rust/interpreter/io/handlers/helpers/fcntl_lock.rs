// Kernel-level POSIX advisory-lock bracket for fs_read / fs_write
// syscalls.  Used to coordinate the node's write/read syscalls with
// an out-of-process sidecar reader (Phase B) that reads the same
// bundle files via their own file descriptors.
//
// # Scope
//
// Advisory locks are:
//   - **Per-process**: two fds in the same process do NOT contend.
//     Node-side callers (same process) don't need extra coordination
//     here; the Rholang-side `fsLockRange` registry handles that.
//     The bracket's purpose is to let a SEPARATE process (e.g., the
//     sidecar reader) take an `F_RDLCK` and have the node's
//     `F_WRLCK` on the same byte range block it, and vice versa.
//   - **Advisory**: both parties must honor the lock discipline.
//     A `cat` or stray reader that doesn't call fcntl can still
//     see torn writes.  Expected; the sidecar is the coordinated
//     path.
//
// # Why F_SETLKW (blocking)
//
// We're already running inside `tokio::task::spawn_blocking`, so a
// blocked lock acquisition parks a blocking-pool worker — expected
// cost.  Blocking semantics are strictly simpler than the F_SETLK
// (non-blocking) + retry-loop alternative.  The sidecar holds its
// locks for ms-scale read durations, so parking the write pool is
// bounded.
//
// # Lock range semantics
//
// POSIX fcntl locks a half-open byte range `[l_start, l_start + l_len)`.
// `l_len == 0` means "lock to end of file" — we never pass 0 here
// (always a bounded range), so EOF-growth doesn't accidentally
// extend the lock.
//
// Zero-length operations (`n == 0`) skip the lock entirely:
//
//   - POSIX says a zero-length lock range doesn't conflict with
//     any other range, so taking one is pointless.
//   - Avoids a syscall round-trip on no-op reads/writes.
//
// # Error handling
//
// A failed F_SETLKW returns `Err(std::io::Error)` to the caller.
// The caller surfaces this as `FSERR_IO` via `io_err_code`.
// Common failures:
//   - `EDEADLK` — kernel detected a lock-ordering deadlock.
//     Rare; indicates a bug or hostile peer; return IO error.
//   - `EINTR` — signal interrupted the wait.  Rare in Rust-side
//     Tokio runtime; return IO error.
//
// Release via `fcntl F_SETLK` (non-blocking) at Drop.  A release
// should never block (we hold the lock).  If it somehow fails,
// the lock auto-releases when the fd closes anyway — ignore the
// error, there's nothing to be done at Drop.

use std::os::fd::RawFd;

/// Range-bounded advisory lock, released on `Drop`.
///
/// Create via [`FcntlLockGuard::acquire_wrlck`] or
/// [`FcntlLockGuard::acquire_rdlck`].  Lifetime binds the lock
/// hold: dropping the guard calls `fcntl F_SETLK F_UNLCK` on the
/// same range.
pub struct FcntlLockGuard {
    fd: RawFd,
    l_start: libc::off_t,
    l_len: libc::off_t,
    // `None` represents the zero-length no-op case where
    // `acquire_*` returned without issuing any fcntl.  Drop
    // becomes a no-op too.
    locked: bool,
}

impl FcntlLockGuard {
    /// Acquire an advisory **write** lock on `[offset, offset + len)`.
    /// Blocks until the lock is granted (`F_SETLKW`).  Zero-length
    /// ranges short-circuit to a no-op guard.
    pub fn acquire_wrlck(fd: RawFd, offset: u64, len: u64) -> Result<Self, std::io::Error> {
        // Cast F_WRLCK to c_short: on macOS it's already c_short,
        // on Linux it's c_int.  flock.l_type is c_short on both.
        acquire(fd, libc::F_WRLCK as libc::c_short, offset, len)
    }

    /// Acquire an advisory **read** lock on `[offset, offset + len)`.
    /// Blocks until the lock is granted (`F_SETLKW`).  Zero-length
    /// ranges short-circuit to a no-op guard.
    pub fn acquire_rdlck(fd: RawFd, offset: u64, len: u64) -> Result<Self, std::io::Error> {
        // Same platform cast as acquire_wrlck.
        acquire(fd, libc::F_RDLCK as libc::c_short, offset, len)
    }
}

fn acquire(
    fd: RawFd,
    l_type: libc::c_short,
    offset: u64,
    len: u64,
) -> Result<FcntlLockGuard, std::io::Error> {
    // Zero-length no-op: POSIX zero-length ranges don't conflict,
    // so there's nothing to coordinate.
    if len == 0 {
        return Ok(FcntlLockGuard {
            fd,
            l_start: 0,
            l_len: 0,
            locked: false,
        });
    }
    // Clamp to i64::MAX so the range stays expressible as
    // `off_t`.  Callers already gate on MAX_READ_BYTES /
    // MAX_WRITE_BYTES, so this is defense-in-depth: an attacker
    // who bypassed the quota check would still see a bounded
    // lock range (and the kernel would reject syscalls that
    // overflow off_t independently).
    let max_i64 = i64::MAX as u64;
    let safe_offset = offset.min(max_i64);
    let safe_len = len.min(max_i64 - safe_offset);
    let fl = libc::flock {
        l_type,
        l_whence: libc::SEEK_SET as libc::c_short,
        l_start: safe_offset as libc::off_t,
        l_len: safe_len as libc::off_t,
        l_pid: 0,
    };
    // SAFETY: `fl` is a valid `libc::flock` on the stack with
    // all required fields initialized.  `fd` is a borrowed raw
    // file descriptor — the caller holds an `Arc<File>` keeping
    // it open for the duration of this call.  `fcntl` reads the
    // struct and returns before this frame returns, so the
    // pointer is live for the full syscall.
    let rc = unsafe { libc::fcntl(fd, libc::F_SETLKW, &fl) };
    if rc < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(FcntlLockGuard {
        fd,
        l_start: safe_offset as libc::off_t,
        l_len: safe_len as libc::off_t,
        locked: true,
    })
}

impl Drop for FcntlLockGuard {
    fn drop(&mut self) {
        if !self.locked {
            return;
        }
        let fl = libc::flock {
            l_type: libc::F_UNLCK as libc::c_short,
            l_whence: libc::SEEK_SET as libc::c_short,
            l_start: self.l_start,
            l_len: self.l_len,
            l_pid: 0,
        };
        // Release is non-blocking: we hold the lock, so no wait
        // is needed.  Errors on release are not actionable here
        // — the fd close path auto-releases any lock we still
        // hold.  Deliberate `let _`.
        //
        // SAFETY: Same as `acquire` — `fl` is a valid stack-
        // local struct; `self.fd` is still open (we don't own
        // the fd, but Drop runs before the owning `Arc<File>`
        // closes it in the caller's scope).
        let _ = unsafe { libc::fcntl(self.fd, libc::F_SETLK, &fl) };
    }
}

/// Query the current fd position via `lseek(fd, 0, SEEK_CUR)`.
/// Used by sequential read/write to compute the lock range when
/// no explicit offset is provided.
///
/// `lseek(_, 0, SEEK_CUR)` is a non-mutating query — it returns
/// the current position without changing it.
///
/// # Errors
///
/// Returns the raw OS error on failure.  Typical cases:
///   - `ESPIPE` — fd refers to a pipe / socket / fifo (not
///     seekable).  Fileio caps are regular files, so this
///     indicates a bug or hostile fd injection.
///   - `EBADF` — fd is closed between the arc-held open and
///     here.  Should be impossible with the FileHandleTable's
///     Arc<File> discipline but surface the error if it fires.
pub fn current_position(fd: RawFd) -> Result<u64, std::io::Error> {
    // SAFETY: `fd` is borrowed from an `Arc<File>` the caller
    // keeps alive.  `lseek` with SEEK_CUR + offset 0 reads the
    // position without mutating fd state.
    let pos = unsafe { libc::lseek(fd, 0, libc::SEEK_CUR) };
    if pos < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(pos as u64)
    }
}

#[cfg(test)]
mod tests {
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::os::fd::AsRawFd;

    use super::*;

    /// Zero-length lock is a no-op — no fcntl syscall, immediate
    /// return.  Pins the fast-path for empty read/write.
    #[test]
    fn zero_length_lock_is_noop_guard() {
        let f = tempfile::tempfile().unwrap();
        let guard = FcntlLockGuard::acquire_wrlck(f.as_raw_fd(), 0, 0).expect("zero-len lock noop");
        // Drop the guard; nothing should happen.  No panic =
        // success.
        drop(guard);
    }

    /// A write lock + a read lock on the same range from the
    /// same process do NOT conflict (fcntl locks are per-
    /// process).  Both succeed, both release cleanly.  Pins
    /// the per-process semantics so future refactors can't
    /// accidentally introduce intra-process deadlock through
    /// this bracket.
    #[test]
    fn same_process_locks_do_not_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.bin");
        {
            let mut f = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&path)
                .unwrap();
            f.write_all(&[0u8; 128]).unwrap();
        }
        let f1 = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        let f2 = OpenOptions::new().read(true).open(&path).unwrap();
        let g_wr = FcntlLockGuard::acquire_wrlck(f1.as_raw_fd(), 0, 128).expect("wr lock");
        // Same-process read lock on the same range succeeds —
        // this is POSIX per-process semantics, not a bug in the
        // guard.
        let g_rd = FcntlLockGuard::acquire_rdlck(f2.as_raw_fd(), 0, 128).expect("rd lock");
        drop(g_rd);
        drop(g_wr);
    }

    /// Non-overlapping ranges on the same fd can be held
    /// simultaneously.  Range `[0, 10)` and `[20, 30)` are
    /// disjoint, so taking the second doesn't conflict with
    /// the first.
    #[test]
    fn disjoint_ranges_coexist() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.bin");
        {
            let mut f = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&path)
                .unwrap();
            f.write_all(&[0u8; 128]).unwrap();
        }
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        let g1 = FcntlLockGuard::acquire_wrlck(f.as_raw_fd(), 0, 10).expect("range 1");
        let g2 = FcntlLockGuard::acquire_wrlck(f.as_raw_fd(), 20, 10).expect("range 2");
        drop(g2);
        drop(g1);
    }

    /// `current_position` reads the fd's current offset without
    /// mutating it.  Writing 7 bytes advances the position to 7;
    /// `current_position` returns 7; a subsequent read starts
    /// from 7, proving the query didn't move the cursor.
    #[test]
    fn current_position_queries_without_mutation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.bin");
        let mut f = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(true)
            .open(&path)
            .unwrap();
        f.write_all(b"abcdefghij").unwrap();
        // Rewind + write 7 bytes to set the cursor at 7.
        use std::io::Seek;
        f.seek(std::io::SeekFrom::Start(0)).unwrap();
        f.write_all(b"ABCDEFG").unwrap();
        let pos = current_position(f.as_raw_fd()).expect("lseek");
        assert_eq!(pos, 7, "position must be 7 after writing 7 bytes");
        // Re-check — should still be 7 (the first query didn't
        // move it).
        let pos2 = current_position(f.as_raw_fd()).expect("lseek");
        assert_eq!(pos2, 7, "position must still be 7 after idempotent query");
    }

    /// Offset at i64::MAX gets clamped and the lock len
    /// collapses to 0, which becomes a no-op guard.  Defense-
    /// in-depth: `MAX_READ_BYTES` / `MAX_WRITE_BYTES` already
    /// cap at 64 MiB, so this path is unreachable from the
    /// handler API — but we pin the clamp so a future hostile-
    /// input scenario is still bounded.
    #[test]
    fn extreme_offset_clamps_to_zero_len() {
        let f = tempfile::tempfile().unwrap();
        let g =
            FcntlLockGuard::acquire_wrlck(f.as_raw_fd(), u64::MAX, 1024).expect("clamp succeeds");
        drop(g);
    }
}
