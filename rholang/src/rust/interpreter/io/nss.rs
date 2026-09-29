// UID / GID resolution via NSS (getpwnam_r / getgrnam_r).
//
// Distinguishes not-found (`Ok(None)` — user/group doesn't exist)
// from transient failure (`Err` — likely an LDAP outage or similar).
// The handler layer translates `Err` to `FSERR_IO` and `Ok(None)` to
// `FSERR_BAD_ARG` for `chown` (where an unknown name is a caller
// error).
//
// Per POSIX only `ENOENT` and `ESRCH` are treated as "not found";
// every other non-zero rc (EPERM from a capability-restricted
// shadow-passwd read, EIO from NSS backend, EAGAIN from a transient
// nsswitch failure, etc.) surfaces as `Err` so the caller sees
// `FSERR_IO` and can retry / alert rather than getting a misleading
// `FSERR_BAD_ARG "unknown user"`.
//
// # BLOCKING CALLS
//
// `getpwnam_r` / `getgrnam_r` / `getpwuid_r` / `getgrgid_r` invoke
// the configured `nsswitch.conf` backend, which may include LDAP or
// NIS.  On such systems these calls block on network I/O and can
// take arbitrarily long.  Callers on an async runtime MUST wrap
// invocations in `tokio::task::spawn_blocking` (or equivalent) to
// avoid stalling the executor.
//
// # unsafe hygiene
//
// Each NSS lookup is a safe `fn` with narrow `unsafe { libc::* }`
// blocks wrapping only the FFI call and (for reverse lookups) the
// subsequent `CStr::from_ptr` read.  Each unsafe block carries a
// SAFETY comment stating the caller-side precondition and the libc
// post-condition.

/// Initial NSS buffer size — matches glibc's typical default.  We
/// grow-and-retry on ERANGE up to [`NSS_BUF_MAX`], which handles
/// LDAP-integrated passwd entries with long shell paths or many
/// group memberships.
#[cfg(unix)]
const NSS_BUF_INITIAL: usize = 4096;

/// Ceiling on NSS buffer growth — protects against a pathological
/// input that would otherwise let a malicious backend force
/// unbounded allocation.  A 64 KiB passwd/group record is already
/// well beyond any realistic entry.
#[cfg(unix)]
const NSS_BUF_MAX: usize = 64 * 1024;

// libc::c_char is i8 on some targets (x86_64), u8 on others (aarch64
// Linux, riscv64, s390x).  Using `[0i8; N]` breaks the aarch64 build.

#[cfg(unix)]
fn nss_buf(size: usize) -> Vec<libc::c_char> { vec![0 as libc::c_char; size] }

/// Format a non-zero errno as a human-readable message via
/// `std::io::Error::from_raw_os_error` — surfaces "Permission
/// denied" instead of "rc=13" in the caller's error path.
#[cfg(unix)]
fn errno_msg(fn_name: &str, rc: libc::c_int) -> String {
    let err = std::io::Error::from_raw_os_error(rc);
    format!("{fn_name}: {err} (errno {rc})")
}

/// Returns `Ok(Some(uid))` if the user exists, `Ok(None)` if the
/// caller-supplied name is genuinely absent (ENOENT/ESRCH per POSIX),
/// or `Err(_)` for any transient failure.  Grows the internal buffer
/// on ERANGE up to [`NSS_BUF_MAX`].
#[cfg(unix)]
pub fn resolve_uid(name: &str) -> Result<Option<u32>, String> {
    use std::ffi::CString;
    let cname = CString::new(name).map_err(|e| e.to_string())?;
    let mut size = NSS_BUF_INITIAL;
    loop {
        // SAFETY: `libc::passwd` is a plain-old-data C struct;
        // zeroing is a well-defined initialization for it.
        let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
        let mut buf = nss_buf(size);
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        // SAFETY: `cname.as_ptr()` is a NUL-terminated CString
        // buffer owned by this scope and outlives the call.
        // `&mut pwd` and `buf.as_mut_ptr()` are unique mutable
        // references / pointers to storage that outlives the call.
        // `getpwnam_r` writes into `pwd` + `buf` and stores a
        // pointer into `result` (or leaves it null on not-found).
        let rc = unsafe {
            libc::getpwnam_r(
                cname.as_ptr(),
                &mut pwd,
                buf.as_mut_ptr(),
                buf.len(),
                &mut result,
            )
        };
        if rc == 0 {
            return if result.is_null() {
                Ok(None)
            } else {
                Ok(Some(pwd.pw_uid))
            };
        }
        if rc == libc::ENOENT || rc == libc::ESRCH {
            return Ok(None);
        }
        if rc == libc::ERANGE && size < NSS_BUF_MAX {
            size = (size * 2).min(NSS_BUF_MAX);
            continue;
        }
        return Err(errno_msg("getpwnam_r", rc));
    }
}

/// Same shape as `resolve_uid`.  Grows on ERANGE up to
/// [`NSS_BUF_MAX`].
#[cfg(unix)]
pub fn resolve_gid(name: &str) -> Result<Option<u32>, String> {
    use std::ffi::CString;
    let cname = CString::new(name).map_err(|e| e.to_string())?;
    let mut size = NSS_BUF_INITIAL;
    loop {
        // SAFETY: `libc::group` is a plain-old-data C struct;
        // zeroing is a well-defined initialization for it.
        let mut grp: libc::group = unsafe { std::mem::zeroed() };
        let mut buf = nss_buf(size);
        let mut result: *mut libc::group = std::ptr::null_mut();
        // SAFETY: same as `resolve_uid`; substitute `getgrnam_r`
        // for `getpwnam_r` and `libc::group` for `libc::passwd`.
        let rc = unsafe {
            libc::getgrnam_r(
                cname.as_ptr(),
                &mut grp,
                buf.as_mut_ptr(),
                buf.len(),
                &mut result,
            )
        };
        if rc == 0 {
            return if result.is_null() {
                Ok(None)
            } else {
                Ok(Some(grp.gr_gid))
            };
        }
        if rc == libc::ENOENT || rc == libc::ESRCH {
            return Ok(None);
        }
        if rc == libc::ERANGE && size < NSS_BUF_MAX {
            size = (size * 2).min(NSS_BUF_MAX);
            continue;
        }
        return Err(errno_msg("getgrnam_r", rc));
    }
}

/// Non-unix stub: this platform has no NSS backend, so we cannot
/// answer "does this user exist?" either way.  Returns `Err` (which
/// callers translate to `FSERR_IO`) rather than `Ok(None)` (which
/// would incorrectly claim the user doesn't exist).
#[cfg(not(unix))]
pub fn resolve_uid(_name: &str) -> Result<Option<u32>, String> {
    Err("NSS lookups not supported on this platform".into())
}

/// Same rationale as the `resolve_uid` non-unix stub.
#[cfg(not(unix))]
pub fn resolve_gid(_name: &str) -> Result<Option<u32>, String> {
    Err("NSS lookups not supported on this platform".into())
}

/// Reverse lookup: uid → username.  Used by `stat` under
/// `ConsensusMode::Oracular` where the name is a display field —
/// dropping it on lookup failure is graceful.
///
/// # Error collapsing
///
/// This helper collapses BOTH "not found" AND "transient NSS backend
/// failure" into `None`.  Callers that need to distinguish those
/// cases (for example, to alert on NSS downtime rather than silently
/// omit the field) must use a different helper — none exists today;
/// add one when a caller arrives that needs it.
#[cfg(unix)]
pub fn uid_to_name(uid: u32) -> Option<String> {
    let mut size = NSS_BUF_INITIAL;
    loop {
        // SAFETY: `libc::passwd` zeroing — same rationale as
        // `resolve_uid`.
        let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
        let mut buf = nss_buf(size);
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        // SAFETY: `&mut pwd` and `buf.as_mut_ptr()` point to unique
        // storage that outlives the call.  `getpwuid_r` populates
        // `pwd` + `buf` on success and stores a pointer into
        // `result`.
        let rc =
            unsafe { libc::getpwuid_r(uid, &mut pwd, buf.as_mut_ptr(), buf.len(), &mut result) };
        if rc == 0 && !result.is_null() {
            // SAFETY: `getpwuid_r` on success populates
            // `pwd.pw_name` with a pointer into `buf` (still alive
            // in this scope); the pointer references a
            // NUL-terminated C string.  `CStr::from_ptr` reads
            // that string safely.
            let cstr = unsafe { std::ffi::CStr::from_ptr(pwd.pw_name) };
            return cstr.to_str().ok().map(|s| s.to_string());
        }
        if rc == libc::ERANGE && size < NSS_BUF_MAX {
            size = (size * 2).min(NSS_BUF_MAX);
            continue;
        }
        return None;
    }
}

/// See `uid_to_name` — same error-collapsing behavior applies.
#[cfg(unix)]
pub fn gid_to_name(gid: u32) -> Option<String> {
    let mut size = NSS_BUF_INITIAL;
    loop {
        // SAFETY: `libc::group` zeroing — same rationale as
        // `resolve_gid`.
        let mut grp: libc::group = unsafe { std::mem::zeroed() };
        let mut buf = nss_buf(size);
        let mut result: *mut libc::group = std::ptr::null_mut();
        // SAFETY: same as `uid_to_name`; substitute `getgrgid_r`
        // and `libc::group`.
        let rc =
            unsafe { libc::getgrgid_r(gid, &mut grp, buf.as_mut_ptr(), buf.len(), &mut result) };
        if rc == 0 && !result.is_null() {
            // SAFETY: same as `uid_to_name`; `grp.gr_name` points
            // into `buf` and is NUL-terminated on success.
            let cstr = unsafe { std::ffi::CStr::from_ptr(grp.gr_name) };
            return cstr.to_str().ok().map(|s| s.to_string());
        }
        if rc == libc::ERANGE && size < NSS_BUF_MAX {
            size = (size * 2).min(NSS_BUF_MAX);
            continue;
        }
        return None;
    }
}

#[cfg(not(unix))]
pub fn uid_to_name(_uid: u32) -> Option<String> { None }

#[cfg(not(unix))]
pub fn gid_to_name(_gid: u32) -> Option<String> { None }

#[cfg(test)]
mod tests {
    use super::*;

    /// `CString::new` rejects any input containing a NUL byte — the
    /// error must be reported before any FFI call happens.  This
    /// test is hermetic (no dependency on system passwd state).
    #[cfg(unix)]
    #[test]
    fn resolve_uid_rejects_names_containing_nul() {
        let err = resolve_uid("with\0nul").expect_err("interior NUL must fail");
        assert!(!err.is_empty(), "error string should be non-empty");
    }

    #[cfg(unix)]
    #[test]
    fn resolve_gid_rejects_names_containing_nul() {
        assert!(resolve_gid("with\0nul").is_err());
    }

    /// An empty username must not surface as a legitimate uid.
    /// Well-behaved backends return "not found" (`Ok(None)`); some
    /// eagerly reject with a distinct error, which is also fine.
    /// The invariant this test pins is: no code path returns
    /// `Ok(Some(uid))` for the empty string.
    #[cfg(unix)]
    #[test]
    fn resolve_uid_never_fabricates_uid_for_empty_name() {
        match resolve_uid("") {
            Ok(None) | Err(_) => {}
            Ok(Some(uid)) => panic!("empty username surfaced uid {uid}"),
        }
    }

    // --- system-dependent smoke tests -----------------------------
    //
    // Run manually with `cargo test -- --ignored`.  Depend on the
    // host having a `root` account with uid 0 — true for essentially
    // every Unix, but not guaranteed.

    #[cfg(unix)]
    #[test]
    #[ignore]
    fn resolve_uid_smoke_root_is_uid_0() {
        assert_eq!(resolve_uid("root").expect("no NSS failure"), Some(0));
    }

    #[cfg(unix)]
    #[test]
    #[ignore]
    fn uid_to_name_smoke_uid_0_is_root() {
        assert_eq!(uid_to_name(0).as_deref(), Some("root"));
    }
}
