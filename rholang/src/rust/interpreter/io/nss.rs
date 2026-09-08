// UID / GID resolution via NSS (getpwnam_r / getgrnam_r).
//
// Distinguishes not-found (`Ok(None)` — user/group doesn't exist) from
// transient failure (`Err` — likely an LDAP outage or similar).  The
// handler layer translates `Err` to `FSERR_IO` and `Ok(None)` to
// `FSERR_BAD_ARG` for `chown` (where an unknown name is a caller error).
//
// Per plan §Risks and open questions, only `ENOENT` and `ESRCH` are
// treated as "not found" per POSIX getpwnam_r; every other non-zero rc
// (EPERM from a capability-restricted shadow-passwd read, EIO from NSS
// backend, EAGAIN from a transient nsswitch failure, etc.) surfaces as
// `Err` so the caller sees `FSERR_IO` and can retry / alert rather than
// getting a misleading `FSERR_BAD_ARG "unknown user"`.
//
// T-11 (2026-09-08): each NSS lookup is now a safe fn with a
// narrow `unsafe { libc::* }` block wrapping ONLY the FFI call.
// The pre-refactor shape wrapped ~20 lines of safe buffer
// allocation, zeroing, and result decoding inside a giant unsafe
// block — the actual unsafe operation was the single libc call.
// Each unsafe block carries a SAFETY comment stating the caller-
// side precondition and the libc post-condition.

// libc::c_char is i8 on some targets (x86_64), u8 on others (aarch64
// Linux, riscv64, s390x).  Using `[0i8; N]` breaks the aarch64 build.
// The alias below picks the right type per target.
#[cfg(unix)]
type NssBuf = [libc::c_char; 4096];

#[cfg(unix)]
fn nss_buf() -> NssBuf { [0 as libc::c_char; 4096] }

/// Returns `Ok(Some(uid))` if the user exists, `Ok(None)` if the caller-
/// supplied name is genuinely absent (ENOENT/ESRCH per POSIX), or
/// `Err(_)` for any transient failure.
#[cfg(unix)]
pub fn resolve_uid(name: &str) -> Result<Option<u32>, String> {
    use std::ffi::CString;
    let cname = CString::new(name).map_err(|e| e.to_string())?;
    // SAFETY: `libc::passwd` is a plain-old-data C struct; zeroing
    // is a well-defined initialization for it.
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut buf = nss_buf();
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: `cname.as_ptr()` is a NUL-terminated CString buffer
    // owned by this scope and outlives the call.  `&mut pwd` and
    // `buf.as_mut_ptr()` are unique mutable references / pointers
    // to stack-allocated storage that outlives the call.
    // `getpwnam_r` writes into `pwd` + `buf` and stores a pointer
    // into `result` (or leaves it null on not-found).
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
        if result.is_null() {
            Ok(None)
        } else {
            Ok(Some(pwd.pw_uid))
        }
    } else if rc == libc::ENOENT || rc == libc::ESRCH {
        Ok(None)
    } else {
        Err(format!("getpwnam_r rc={rc}"))
    }
}

/// Same shape as `resolve_uid`.
#[cfg(unix)]
pub fn resolve_gid(name: &str) -> Result<Option<u32>, String> {
    use std::ffi::CString;
    let cname = CString::new(name).map_err(|e| e.to_string())?;
    // SAFETY: `libc::group` is a plain-old-data C struct; zeroing
    // is a well-defined initialization for it.
    let mut grp: libc::group = unsafe { std::mem::zeroed() };
    let mut buf = nss_buf();
    let mut result: *mut libc::group = std::ptr::null_mut();
    // SAFETY: same as `resolve_uid`; substitute `getgrnam_r` for
    // `getpwnam_r` and `libc::group` for `libc::passwd`.
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
        if result.is_null() {
            Ok(None)
        } else {
            Ok(Some(grp.gr_gid))
        }
    } else if rc == libc::ENOENT || rc == libc::ESRCH {
        Ok(None)
    } else {
        Err(format!("getgrnam_r rc={rc}"))
    }
}

#[cfg(not(unix))]
pub fn resolve_uid(_name: &str) -> Result<Option<u32>, String> {
    Err("NSS lookups not supported on this platform".into())
}

#[cfg(not(unix))]
pub fn resolve_gid(_name: &str) -> Result<Option<u32>, String> {
    Err("NSS lookups not supported on this platform".into())
}

/// Reverse lookup: uid → username.  Used for `stat` record building.
#[cfg(unix)]
pub fn uid_to_name(uid: u32) -> Option<String> {
    // SAFETY: `libc::passwd` zeroing — same rationale as
    // `resolve_uid`.
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut buf = nss_buf();
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: `&mut pwd` and `buf.as_mut_ptr()` point to unique
    // stack storage that outlives the call.  `getpwuid_r`
    // populates `pwd` + `buf` on success and stores a pointer
    // into `result`.
    let rc = unsafe { libc::getpwuid_r(uid, &mut pwd, buf.as_mut_ptr(), buf.len(), &mut result) };
    if rc == 0 && !result.is_null() {
        // SAFETY: `getpwuid_r` on success populates `pwd.pw_name`
        // with a pointer into `buf` (still alive in this scope);
        // the pointer references a NUL-terminated C string.
        // `CStr::from_ptr` reads that string safely.
        let cstr = unsafe { std::ffi::CStr::from_ptr(pwd.pw_name) };
        cstr.to_str().ok().map(|s| s.to_string())
    } else {
        None
    }
}

#[cfg(unix)]
pub fn gid_to_name(gid: u32) -> Option<String> {
    // SAFETY: `libc::group` zeroing — same rationale as
    // `resolve_gid`.
    let mut grp: libc::group = unsafe { std::mem::zeroed() };
    let mut buf = nss_buf();
    let mut result: *mut libc::group = std::ptr::null_mut();
    // SAFETY: same as `uid_to_name`; substitute `getgrgid_r` and
    // `libc::group`.
    let rc = unsafe { libc::getgrgid_r(gid, &mut grp, buf.as_mut_ptr(), buf.len(), &mut result) };
    if rc == 0 && !result.is_null() {
        // SAFETY: same as `uid_to_name`; `grp.gr_name` points
        // into `buf` and is NUL-terminated on success.
        let cstr = unsafe { std::ffi::CStr::from_ptr(grp.gr_name) };
        cstr.to_str().ok().map(|s| s.to_string())
    } else {
        None
    }
}

#[cfg(not(unix))]
pub fn uid_to_name(_uid: u32) -> Option<String> { None }

#[cfg(not(unix))]
pub fn gid_to_name(_gid: u32) -> Option<String> { None }
