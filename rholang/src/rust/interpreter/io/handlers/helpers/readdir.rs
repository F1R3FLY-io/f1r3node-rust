// `readdir`-backed primitives for the Stream + Observation families.
//
// Three helpers:
//
//   - `errno_reset` — platform-conditional (Linux + macOS)
//     per-thread errno zeroing.  `readdir` returns NULL for both
//     EOF and error; disambiguating requires clearing errno
//     beforehand and checking afterward.
//   - `readdir_one_entry(dirp, cmode) -> Par` — advance a `DIR*`
//     one entry, skipping `.` / `..`, and return either a
//     `[true, entryRecord]` Par on a real entry OR `err_eos()` /
//     an err Par on EOF / error.  Called by
//     `fs_entries_stream_next`.
//   - `entry_stat_row(dir_fd, name, cmode) -> Par` — stat one
//     entry relative to a parent `dir_fd` via
//     `openat(O_NOFOLLOW)` + `metadata`.  Returns a full
//     `stat_record` on regular / directory / etc. entries OR an
//     `error_record` on symlink / unreadable entries.  Shared
//     between `fs_entries` (bulk, yet to land) and
//     `fs_entries_stream_next`.
//   - `reply_is_ok(reply) -> bool` — classifier for the
//     per-entry supplement charge in Stream + Observation
//     handlers: `true` if the reply head is `[true, ...]`
//     regardless of the tail.  Different from `extract_ok_u64`
//     (requires `[true, int]`) and `extract_ok_list_len`
//     (requires `[true, list]`); only inspects the head boolean.
//
// # SAFETY contracts
//
// Each unsafe block has an inline SAFETY comment covering: fd /
// pointer lifetimes, NUL-termination guarantees, caller-owned
// serialization requirements (DirHandle's `iter` Mutex serializes
// readdir on a single DIR*), and non-retention of borrowed
// pointers past syscall return.

use std::ffi::OsStr;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::Par;

use crate::rust::interpreter::io::errors::io_err_code;
use crate::rust::interpreter::io::path::io_msg_scrub;
use crate::rust::interpreter::io::response::{err, err_eos, ok_par};
use crate::rust::interpreter::io::stat::{error_record, stat_record};
use crate::rust::interpreter::io::ConsensusMode;
use crate::rust::interpreter::rho_type::RhoBoolean;

/// Zero the platform's per-thread errno location.  Linux and
/// macOS have different functions for the TLS address; other
/// targets are not supported.
///
/// # Safety
///
/// Writes zero to the platform's per-thread errno location.  Safe
/// on any thread; the TLS access is thread-local by construction.
#[cfg(target_os = "macos")]
pub unsafe fn errno_reset() { *libc::__error() = 0; }

/// See [`errno_reset`] macOS variant.
///
/// # Safety
///
/// Writes zero to the platform's per-thread errno location.  Safe
/// on any thread; the TLS access is thread-local by construction.
#[cfg(target_os = "linux")]
pub unsafe fn errno_reset() { *libc::__errno_location() = 0; }

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
compile_error!("io::handlers::helpers::readdir supports only macOS and Linux targets");

/// Advance `dirp` one entry, skipping `.` / `..`, and build the
/// reply Par.  Returns one of:
///
///   - `ok_par(entry_stat_row(..))` — `[true, entryRecord]` on a
///     real entry.
///   - `err_eos()` — `[false, "EOS"]` on clean EOF.
///   - `err(io_err_code, io_msg_scrub)` — `[false, FSERR, msg]`
///     on readdir error.
///
/// # Safety contract
///
/// `dirp` must be a live `libc::DIR*`.  The caller must hold the
/// enclosing [`DirHandle::iter`](super::super::super::dir_handle_table::DirHandle)
/// Mutex for the duration of the call — `readdir` is not thread-
/// safe without serialization on the same DIR*.
pub fn readdir_one_entry(dirp: *mut libc::DIR, cmode: ConsensusMode) -> Par {
    use std::os::unix::ffi::OsStringExt;
    loop {
        // SAFETY: `errno_reset` writes zero to the platform's
        // per-thread errno location; safe on any thread.  Used
        // to disambiguate EOF (readdir returns NULL + errno==0)
        // from error (NULL + errno!=0).
        unsafe { errno_reset() };
        // SAFETY: `dirp` is a live `libc::DIR*` per this function's
        // SAFETY contract; the enclosing `DirHandle::iter` Mutex
        // serializes readdir calls on this DIR*.
        let ent = unsafe { libc::readdir(dirp) };
        if ent.is_null() {
            let raw = std::io::Error::last_os_error().raw_os_error();
            if raw == Some(0) || raw.is_none() {
                return err_eos();
            }
            let e = std::io::Error::last_os_error();
            return err(io_err_code(&e), io_msg_scrub(&e));
        }
        // SAFETY: `ent` is non-null (checked above); readdir(3)
        // guarantees `d_name` is a NUL-terminated in-struct array
        // owned by the DIR*'s buffer.  Pointer is only used
        // through the immediately-following `CStr::from_ptr` +
        // `to_bytes` + copy; no lifetime escapes past the next
        // readdir call.
        let name_ptr = unsafe { (*ent).d_name.as_ptr() };
        // SAFETY: `name_ptr` came from a live dirent's `d_name`,
        // NUL-terminated by readdir(3).  `name_c` is used only
        // for the subsequent `to_bytes()` before dropping.
        let name_c = unsafe { std::ffi::CStr::from_ptr(name_ptr) };
        let name_bytes = name_c.to_bytes();
        if name_bytes == b"." || name_bytes == b".." {
            continue;
        }
        // SAFETY: `dirp` still live per contract; `libc::dirfd`
        // returns the underlying fd owned by the DIR* (do NOT
        // close — closedir handles the fd when the DIR* is
        // dropped).
        let dir_fd = unsafe { libc::dirfd(dirp) };
        let name_os = std::ffi::OsString::from_vec(name_bytes.to_vec());
        return ok_par(entry_stat_row(dir_fd, &name_os, cmode));
    }
}

/// Build a stat-row Par for one entry inside `dir_fd`.  Opens the
/// entry via `openat(O_NOFOLLOW | O_CLOEXEC)`, calls `metadata`,
/// and returns either a full `stat_record` (regular / directory /
/// etc.) or an `error_record` (symlink, unreadable, invalid
/// filename).
///
/// # Safety contract
///
/// `dir_fd` must be a live open dirfd for the duration of the call.
/// The owning `DirHandle::iter` Mutex (held by `readdir_one_entry`'s
/// caller) keeps the DIR* and its underlying dirfd alive across
/// this call.
pub fn entry_stat_row(dir_fd: libc::c_int, name: &OsStr, mode: ConsensusMode) -> Par {
    use std::os::fd::FromRawFd;
    use std::os::unix::ffi::OsStrExt;
    let display = name.to_string_lossy().into_owned();
    let cname = match std::ffi::CString::new(name.as_bytes()) {
        Ok(c) => c,
        Err(_) => return error_record(&display, "invalid filename"),
    };
    // SAFETY: `dir_fd` is a caller-supplied open dirfd (per
    // contract); `cname` is a locally-owned NUL-terminated
    // CString that outlives the openat call.  On success,
    // `File::from_raw_fd` takes ownership of the fresh fd so
    // `Drop` closes it on every exit path.
    unsafe {
        let fd = libc::openat(
            dir_fd,
            cname.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        );
        if fd < 0 {
            let e = std::io::Error::last_os_error();
            return error_record(&display, &io_msg_scrub(&e));
        }
        let file = std::fs::File::from_raw_fd(fd);
        match file.metadata() {
            Ok(m) => stat_record(&display, &m, mode),
            Err(e) => error_record(&display, &io_msg_scrub(&e)),
        }
    }
}

/// Enumerate `dir_fd` into a sorted-by-caller `Vec<OsString>`
/// (`.` / `..` skipped), capped at `max`.  Returns `(names,
/// hit_cap)` where `hit_cap = true` indicates the caller should
/// surface `FSERR_QUOTA_EXCEEDED` (the enumeration was truncated).
///
/// # Why fdopendir takes ownership
///
/// `fdopendir(dir_fd)` succeeds → the DIR* owns `dir_fd`; a later
/// `closedir(dir)` releases the fd.  On fdopendir failure the
/// caller's fd stays live and this function closes it manually
/// before returning the error.
///
/// # Safety contract
///
/// `dir_fd` must be a caller-supplied open dirfd that this
/// function may take ownership of (via fdopendir on success +
/// manual close on failure).  The caller must NOT close `dir_fd`
/// after this call returns — the DIR*'s drop handles it.
///
/// # Determinism note
///
/// The returned `Vec<OsString>` reflects `readdir` order, which
/// is filesystem-dependent and NOT stable across validators.
/// `fs_entries` sorts the slice BEFORE building reply rows so
/// its reply bytes are deterministic.  Callers that need
/// deterministic order MUST sort post-call.
pub fn read_dir_capped(
    dir_fd: libc::c_int,
    max: usize,
) -> std::io::Result<(Vec<std::ffi::OsString>, bool)> {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    // SAFETY: `dir_fd` is a caller-supplied open dirfd per
    // contract; `fdopendir` takes ownership on success (closedir
    // handles the fd on drop).  On failure we manually close it.
    // `readdir` / `CStr::from_ptr` on the returned DIR* are
    // single-threaded here (the caller holds no shared pointer),
    // and each dirent buffer is copied before the next readdir
    // call.
    unsafe {
        let dir = libc::fdopendir(dir_fd);
        if dir.is_null() {
            let e = std::io::Error::last_os_error();
            libc::close(dir_fd);
            return Err(e);
        }
        let mut names: Vec<OsString> = Vec::new();
        let mut hit_cap = false;
        loop {
            // Reset errno; readdir returns NULL on both EOF and
            // error.  Must clear before the call + check after.
            errno_reset();
            let ent = libc::readdir(dir);
            if ent.is_null() {
                let e = std::io::Error::last_os_error();
                if e.raw_os_error() == Some(0) {
                    break; // Clean EOF.
                }
                libc::closedir(dir);
                return Err(e);
            }
            let name_ptr = (*ent).d_name.as_ptr();
            let name_c = std::ffi::CStr::from_ptr(name_ptr);
            let name_bytes = name_c.to_bytes();
            if name_bytes == b"." || name_bytes == b".." {
                continue;
            }
            if names.len() >= max {
                hit_cap = true;
                break;
            }
            names.push(OsString::from_vec(name_bytes.to_vec()));
        }
        libc::closedir(dir);
        Ok((names, hit_cap))
    }
}

/// Does `reply` start with `[true, ...]`?  Used by the two-event
/// cost supplement classifier: `n = 1` on `[true, entryRecord]`,
/// `n = 0` on `[false, ...]` (EOS or error).
///
/// Only inspects the head boolean — the tail shape doesn't
/// matter for this classification (contrast with `extract_ok_u64`
/// / `extract_ok_list_len` which validate specific tail shapes).
pub fn reply_is_ok(reply: &[Par]) -> bool {
    let Some(head) = reply.first() else {
        return false;
    };
    let Some(expr) = head.exprs.first() else {
        return false;
    };
    let Some(ExprInstance::EListBody(list)) = expr.expr_instance.as_ref() else {
        return false;
    };
    let Some(ok_par) = list.ps.first() else {
        return false;
    };
    RhoBoolean::unapply(ok_par) == Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::interpreter::io::response::{err, err_eos, ok_bare};
    use crate::rust::interpreter::rho_type::{RhoNumber, RhoString};

    /// `reply_is_ok` returns `true` on an `ok_bare()` reply
    /// (`[true]` — head boolean true, no tail).
    #[test]
    fn reply_is_ok_accepts_ok_bare() {
        let r = [ok_bare()];
        assert!(reply_is_ok(&r));
    }

    /// `reply_is_ok` returns `true` on `[true, entryRecord]`
    /// shape — the actual Stream `next` success reply.
    #[test]
    fn reply_is_ok_accepts_ok_with_tail() {
        let r = [ok_par(RhoString::create_par("entry".to_string()))];
        assert!(reply_is_ok(&r));
    }

    /// `reply_is_ok` returns `false` on `err_eos()` reply
    /// (`[false, "EOS"]`).
    #[test]
    fn reply_is_ok_rejects_eos() {
        let r = [err_eos()];
        assert!(!reply_is_ok(&r));
    }

    /// `reply_is_ok` returns `false` on an error reply
    /// (`[false, FSERR, msg]`).
    #[test]
    fn reply_is_ok_rejects_err() {
        use crate::rust::interpreter::io::errors::FSERR_IO;
        let r = [err(FSERR_IO, "disk error")];
        assert!(!reply_is_ok(&r));
    }

    /// `reply_is_ok` returns `false` on an empty reply slice.
    #[test]
    fn reply_is_ok_rejects_empty_slice() {
        let r: Vec<Par> = vec![];
        assert!(!reply_is_ok(&r));
    }

    /// `reply_is_ok` returns `false` on a non-list head (not a
    /// well-formed reply shape).
    #[test]
    fn reply_is_ok_rejects_non_list_head() {
        let r = [RhoNumber::create_par(42)];
        assert!(!reply_is_ok(&r));
    }

    /// Compile-time witness that `errno_reset` has the expected
    /// `unsafe fn()` signature on supported platforms.  Also
    /// pins the no-arg + no-return shape against a future
    /// refactor that added params.
    #[test]
    fn errno_reset_signature_is_unsafe_fn_no_arg_no_return() { let _: unsafe fn() = errno_reset; }

    /// Runtime behavior of `readdir_one_entry` and
    /// `entry_stat_row` requires a `DIR*` + dirfd fixture
    /// (`fdopendir` on a tempdir).  Deferred to integration
    /// tests — the pre-trait fileio tests cover the behavior
    /// and migrate forward unchanged.
    #[test]
    fn runtime_tests_deferred_to_integration() {
        // Compile-time witnesses that the exported helpers can
        // be referenced from their expected call sites.  A
        // regression that broke the module's export surface
        // would trip here before downstream handler compiles.
        let _: fn(*mut libc::DIR, ConsensusMode) -> Par = readdir_one_entry;
        let _: fn(libc::c_int, &OsStr, ConsensusMode) -> Par = entry_stat_row;
    }
}
