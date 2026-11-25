// Write-syscall primitive for fs_write / fs_write_at.
//
// Mirrors fileio's `handlers_helpers::write_impl_via_table` — looks
// up the shadow fd (returns FSERR_CLOSED on miss), MAX_WRITE_BYTES-
// gates the request (defense in depth; the handler's `pre_syscall`
// already gates before the WAL reserve, but a replay-side-execute
// path could reach here via a different code path), then issues
// `libc::write` (sequential, `offset = None`) or `libc::pwrite`
// (positional, `offset = Some(off)`) on tokio's blocking pool.
//
// # Return shape
//
// `-> Par` — the handler bodies wrap this directly in
// `HandlerReply::Ok(par)`.  Success → `ok_u64(n)` where `n` is the
// `ssize_t` return from the syscall.  Error → `err(io_err_code,
// io_msg_scrub)`.
//
// # fd-formatting discipline
//
// Unknown-fd FSERR message formats `fd` as **u64** (direct) —
// matches fileio's `write_impl_via_table` + the broader fs_truncate
// convention.  See `fs_truncate` module header for the cross-
// handler fd-formatting audit discipline.
//
// # Why not inline in each handler?
//
// Both fs_write and fs_write_at use the exact same syscall shape
// modulo the `offset` branch.  Centralizing here means a Wave 5
// deterministic-reduction integration touches one call site, not
// two, and keeps the MAX_WRITE_BYTES gate's defense-in-depth
// identical between the sequential and positional variants.

use std::os::fd::AsRawFd;

use models::rhoapi::Par;
use tokio::task::spawn_blocking;

use super::fcntl_lock::{current_position, FcntlLockGuard};
use crate::rust::interpreter::io::errors::{io_err_code, join_err_abort, FSERR_CLOSED};
use crate::rust::interpreter::io::handle_table::FileHandleTable;
use crate::rust::interpreter::io::path::io_msg_scrub;
use crate::rust::interpreter::io::response::{err, ok_u64};
use crate::rust::interpreter::io::MAX_WRITE_BYTES;

/// Write `bytes` to the shadow fd — `libc::write` on `offset=None`,
/// `libc::pwrite` on `offset=Some(off)`.  See module header for the
/// MAX_WRITE_BYTES gate + fd-formatting discipline.
pub async fn write_impl_via_table(
    handles: &FileHandleTable,
    fd: u64,
    bytes: Vec<u8>,
    offset: Option<u64>,
) -> Par {
    if bytes.len() as u64 > MAX_WRITE_BYTES {
        return err(
            crate::rust::interpreter::io::errors::FSERR_QUOTA_EXCEEDED,
            format!("write {} exceeds MAX_WRITE_BYTES", bytes.len()),
        );
    }
    let file_arc = match handles.raw_fd(fd).await {
        Some(f) => f,
        None => return err(FSERR_CLOSED, format!("unknown fd {fd}")),
    };
    let result = spawn_blocking(move || {
        let raw_fd = file_arc.as_raw_fd();
        // POSIX advisory-lock bracket: F_WRLCK on `[lock_offset,
        // lock_offset + bytes.len())` for the duration of the
        // syscall.  Lets a cooperating out-of-process reader (the
        // Phase-B sidecar) see consistent bytes by taking an
        // F_RDLCK on the same range.  Same-process callers do NOT
        // contend here — fcntl locks are per-process; Rholang-side
        // coordination lives in the in-memory `LockRegistry`.
        //
        // For positional writes, `lock_offset = off` directly.
        // For sequential writes, query the current fd position
        // via `lseek(fd, 0, SEEK_CUR)` — a non-mutating peek — so
        // the lock range matches what `libc::write` will actually
        // target.
        let lock_offset = match offset {
            Some(off) => off,
            None => match current_position(raw_fd) {
                Ok(p) => p,
                Err(e) => return Err(e),
            },
        };
        let _lock = match FcntlLockGuard::acquire_wrlck(raw_fd, lock_offset, bytes.len() as u64) {
            Ok(g) => g,
            Err(e) => return Err(e),
        };
        // SAFETY: `file_arc` (an `Arc<File>`) is moved into this
        // closure and keeps `raw_fd` open for the duration.  `bytes`
        // is a live `Vec<u8>` owned by this scope.  pwrite/write
        // read up to `bytes.len()` bytes from the pointer and return
        // the count written (or -1 on error).  The `_lock` guard
        // keeps the advisory write-lock held across the syscall
        // and releases it on drop.
        let n = unsafe {
            if let Some(off) = offset {
                libc::pwrite(raw_fd, bytes.as_ptr() as *const _, bytes.len(), off as i64)
            } else {
                libc::write(raw_fd, bytes.as_ptr() as *const _, bytes.len())
            }
        };
        if n < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(n as u64)
        }
    })
    .await;
    match result {
        Err(je) => join_err_abort(je),
        Ok(Err(e)) => err(io_err_code(&e), io_msg_scrub(&e)),
        Ok(Ok(n)) => ok_u64(n),
    }
}
