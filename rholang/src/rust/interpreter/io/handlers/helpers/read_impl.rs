// Sequential + positional read via `libc::read` / `libc::pread`.
// Shared between fs_read (sequential, `offset: None`) and
// fs_read_at (positional, `offset: Some(off)`).
//
// # Reply shape
//
//   - `ok_bytes(bytes)` — success, bytes read (may be shorter than
//     requested on EOF).
//   - `err(FSERR_QUOTA_EXCEEDED, msg)` — `n > MAX_READ_BYTES`.
//     Checked BEFORE spawning the blocking task + before fd
//     lookup, so quota exhaustion returns the same reply even on
//     an unknown fd.  Matches pre-trait ordering.
//   - `err(FSERR_CLOSED, msg)` — fd absent from the handle table
//     OR shadow handle (no `file: Some(_)`).
//   - `err(io_err_code, io_msg_scrub)` — libc::read / libc::pread
//     failure.
//
// # Buffer allocation
//
// Allocates `vec![0u8; n]` BEFORE the syscall, then truncates to
// the returned byte count.  No zero-init optimization — the
// kernel overwrites the full range on success (truncated to
// `got`), but on partial-read-then-error we might leak zeros
// past the truncation point.  Current form matches fileio;
// `MaybeUninit`-based read would be a cross-slice performance
// change out of scope.
//
// # park_external_during deferral
//
// Same deferral as the per-handler sibling calls — raw
// `tokio::task::spawn_blocking` without reduction-driver parking.
// Wave 5 scope.

use std::os::fd::AsRawFd;

use models::rhoapi::Par;
use tokio::task::spawn_blocking;

use crate::rust::interpreter::io::errors::{
    io_err_code, join_err_abort, FSERR_CLOSED, FSERR_QUOTA_EXCEEDED,
};
use crate::rust::interpreter::io::handle_table::FileHandleTable;
use crate::rust::interpreter::io::path::io_msg_scrub;
use crate::rust::interpreter::io::response::{err, ok_bytes};
use crate::rust::interpreter::io::MAX_READ_BYTES;

/// Sequential + positional read helper.
///
/// `offset = None` → `libc::read(fd, buf, n)` (sequential; advances
/// the OS-fd position by the bytes returned).  `offset = Some(off)`
/// → `libc::pread(fd, buf, n, off)` (positional; does NOT advance
/// the OS-fd position).  The handler layer tracks shadow-position
/// advance separately via `JournalPath::state_source_reply`.
pub async fn read_impl_via_table(
    handles: &FileHandleTable,
    fd: u64,
    n: u64,
    offset: Option<u64>,
) -> Par {
    if n > MAX_READ_BYTES {
        return err(
            FSERR_QUOTA_EXCEEDED,
            format!("read {n} exceeds MAX_READ_BYTES"),
        );
    }
    let file_arc = match handles.raw_fd(fd).await {
        Some(f) => f,
        None => return err(FSERR_CLOSED, format!("unknown fd {fd}")),
    };
    let result = spawn_blocking(move || {
        let raw_fd = file_arc.as_raw_fd();
        let mut buf = vec![0u8; n as usize];
        // SAFETY: `file_arc` (an `Arc<File>`) is moved into this
        // closure and keeps `raw_fd` open for the duration.  `buf`
        // is a live heap allocation of `n` bytes owned by this
        // scope.  pread/read write into `buf` up to `n` bytes and
        // return the count written (or -1 on error); we truncate
        // `buf` to the returned length after a sign check.
        let got = unsafe {
            if let Some(off) = offset {
                libc::pread(raw_fd, buf.as_mut_ptr() as *mut _, n as usize, off as i64)
            } else {
                libc::read(raw_fd, buf.as_mut_ptr() as *mut _, n as usize)
            }
        };
        if got < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            buf.truncate(got as usize);
            Ok(buf)
        }
    })
    .await;
    match result {
        Err(je) => join_err_abort(je),
        Ok(Err(e)) => err(io_err_code(&e), io_msg_scrub(&e)),
        Ok(Ok(bytes)) => ok_bytes(bytes),
    }
}

#[cfg(test)]
mod tests {
    use prost::Message;

    use super::*;
    use crate::rust::interpreter::io::errors::FSERR_QUOTA_EXCEEDED;

    /// LOAD-BEARING: `n > MAX_READ_BYTES` MUST produce
    /// `err(FSERR_QUOTA_EXCEEDED, ...)` BEFORE fd lookup.  Pin the
    /// ordering so a future refactor that moved the check after
    /// `raw_fd(fd)` would surface a different reply for the
    /// `(unknown_fd, oversized_n)` corner case (FSERR_CLOSED
    /// instead of FSERR_QUOTA_EXCEEDED) — consensus-observable.
    ///
    /// Uses `MAX_READ_BYTES + 1` on an empty FileHandleTable (any
    /// fd would be unknown).  The quota check runs first, so the
    /// reply is QUOTA_EXCEEDED regardless of fd state.
    #[tokio::test]
    async fn quota_check_runs_before_fd_lookup() {
        let handles = FileHandleTable::new();
        let reply = read_impl_via_table(&handles, 0xdeadbeef, MAX_READ_BYTES + 1, None).await;

        // Reply shape pin — must be an `err(FSERR_QUOTA_EXCEEDED,
        // ...)` Par, NOT an `err(FSERR_CLOSED, ...)` Par.
        let expected = err(
            FSERR_QUOTA_EXCEEDED,
            format!("read {} exceeds MAX_READ_BYTES", MAX_READ_BYTES + 1),
        );
        assert_eq!(reply.encode_to_vec(), expected.encode_to_vec());
    }

    /// `n == MAX_READ_BYTES` is the boundary — exactly MAX is
    /// accepted (quota check uses `>`, not `>=`).  Fd still
    /// unknown so reply is `FSERR_CLOSED`, proving the quota
    /// check didn't reject the boundary value.
    #[tokio::test]
    async fn exactly_max_bytes_passes_quota_check() {
        let handles = FileHandleTable::new();
        let reply = read_impl_via_table(&handles, 0xdeadbeef, MAX_READ_BYTES, None).await;
        let expected = err(FSERR_CLOSED, "unknown fd 3735928559".to_string());
        assert_eq!(reply.encode_to_vec(), expected.encode_to_vec());
    }

    /// Unknown fd → `FSERR_CLOSED`.
    #[tokio::test]
    async fn unknown_fd_returns_fserr_closed() {
        let handles = FileHandleTable::new();
        let reply = read_impl_via_table(&handles, 0, 1024, None).await;
        let expected = err(FSERR_CLOSED, "unknown fd 0".to_string());
        assert_eq!(reply.encode_to_vec(), expected.encode_to_vec());
    }

    /// Offset-variant quota check runs at the same site —
    /// `fs_read_at` (yet to land) MUST get the same ordering.
    #[tokio::test]
    async fn quota_check_also_fires_for_positional_read() {
        let handles = FileHandleTable::new();
        let reply = read_impl_via_table(&handles, 0, MAX_READ_BYTES + 1, Some(0)).await;
        let expected = err(
            FSERR_QUOTA_EXCEEDED,
            format!("read {} exceeds MAX_READ_BYTES", MAX_READ_BYTES + 1),
        );
        assert_eq!(reply.encode_to_vec(), expected.encode_to_vec());
    }
}
