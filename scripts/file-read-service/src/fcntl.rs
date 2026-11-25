// POSIX advisory-lock bracket for the sidecar reader.
//
// The node's `rholang::rust::interpreter::io::handlers::helpers::fcntl_lock`
// (companion module) takes an `F_WRLCK` around every
// pwrite/write syscall.  This sidecar takes an `F_RDLCK` on the
// same byte range before each `pread`.  Advisory locks are
// per-process + per-fd-process-tuple: both parties must honor the
// discipline.  A hostile reader bypasses the lock and can see
// torn bytes; the sidecar is the coordinated path.
//
// Mirror lives at
// `rholang/src/rust/interpreter/io/handlers/helpers/fcntl_lock.rs` —
// the node's F_WRLCK side of this discipline.  Any semantic change
// (lock-type encoding, drop semantics, zero-length policy) MUST
// land in both files together; cross-process coordination relies
// on both sides using identical fcntl discipline.
//
// # Why duplicate (not import from the node crate)
//
// Importing the rholang-side module would pull the entire node /
// rholang dep tree into the sidecar — slow builds, large binary.
// The fcntl primitive is tiny (~40 LOC) and operator-stable; a
// local copy is cheaper than the cross-crate dep.  The two
// implementations converge on identical POSIX semantics; both
// cover zero-length no-op + i64::MAX clamp paths.

use std::os::fd::RawFd;

/// Range-bounded advisory read lock, released on `Drop`.
pub struct FcntlReadLockGuard {
    fd: RawFd,
    l_start: libc::off_t,
    l_len: libc::off_t,
    locked: bool,
}

impl FcntlReadLockGuard {
    /// Acquire an advisory **read** lock on `[offset, offset + len)`.
    /// Blocks until the lock is granted (`F_SETLKW`).  Zero-length
    /// ranges short-circuit to a no-op guard.
    pub fn acquire(fd: RawFd, offset: u64, len: u64) -> Result<Self, std::io::Error> {
        if len == 0 {
            return Ok(FcntlReadLockGuard {
                fd,
                l_start: 0,
                l_len: 0,
                locked: false,
            });
        }
        let max_i64 = i64::MAX as u64;
        let safe_offset = offset.min(max_i64);
        let safe_len = len.min(max_i64 - safe_offset);
        let fl = libc::flock {
            l_type: libc::F_RDLCK as libc::c_short,
            l_whence: libc::SEEK_SET as libc::c_short,
            l_start: safe_offset as libc::off_t,
            l_len: safe_len as libc::off_t,
            l_pid: 0,
        };
        // SAFETY: `fl` is a valid `libc::flock` on the stack with
        // all required fields initialized.  `fd` is open for the
        // duration of this call (owned by the caller's File).
        let rc = unsafe { libc::fcntl(fd, libc::F_SETLKW, &fl) };
        if rc < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(FcntlReadLockGuard {
            fd,
            l_start: safe_offset as libc::off_t,
            l_len: safe_len as libc::off_t,
            locked: true,
        })
    }
}

impl Drop for FcntlReadLockGuard {
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
        // Release is non-blocking: we hold the lock.  Errors on
        // release are not actionable; fd close auto-releases.
        //
        // SAFETY: Same as acquire — `fl` is a valid stack-local
        // struct; `self.fd` is still open (Drop runs before the
        // owning `File` closes it in the caller's scope).
        let _ = unsafe { libc::fcntl(self.fd, libc::F_SETLK, &fl) };
    }
}
