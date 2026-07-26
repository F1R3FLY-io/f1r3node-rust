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
    unsafe {
        let mut pwd: libc::passwd = std::mem::zeroed();
        let mut buf = nss_buf();
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        let rc = libc::getpwnam_r(
            cname.as_ptr(),
            &mut pwd,
            buf.as_mut_ptr(),
            buf.len(),
            &mut result,
        );
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
}

/// Same shape as `resolve_uid`.
#[cfg(unix)]
pub fn resolve_gid(name: &str) -> Result<Option<u32>, String> {
    use std::ffi::CString;
    let cname = CString::new(name).map_err(|e| e.to_string())?;
    unsafe {
        let mut grp: libc::group = std::mem::zeroed();
        let mut buf = nss_buf();
        let mut result: *mut libc::group = std::ptr::null_mut();
        let rc = libc::getgrnam_r(
            cname.as_ptr(),
            &mut grp,
            buf.as_mut_ptr(),
            buf.len(),
            &mut result,
        );
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
    unsafe {
        let mut pwd: libc::passwd = std::mem::zeroed();
        let mut buf = nss_buf();
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        let rc = libc::getpwuid_r(uid, &mut pwd, buf.as_mut_ptr(), buf.len(), &mut result);
        if rc == 0 && !result.is_null() {
            let cstr = std::ffi::CStr::from_ptr(pwd.pw_name);
            cstr.to_str().ok().map(|s| s.to_string())
        } else {
            None
        }
    }
}

#[cfg(unix)]
pub fn gid_to_name(gid: u32) -> Option<String> {
    unsafe {
        let mut grp: libc::group = std::mem::zeroed();
        let mut buf = nss_buf();
        let mut result: *mut libc::group = std::ptr::null_mut();
        let rc = libc::getgrgid_r(gid, &mut grp, buf.as_mut_ptr(), buf.len(), &mut result);
        if rc == 0 && !result.is_null() {
            let cstr = std::ffi::CStr::from_ptr(grp.gr_name);
            cstr.to_str().ok().map(|s| s.to_string())
        } else {
            None
        }
    }
}

#[cfg(not(unix))]
pub fn uid_to_name(_uid: u32) -> Option<String> { None }

#[cfg(not(unix))]
pub fn gid_to_name(_gid: u32) -> Option<String> { None }
