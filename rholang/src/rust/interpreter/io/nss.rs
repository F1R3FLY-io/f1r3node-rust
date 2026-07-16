//! Name Service Switch lookups for user/group names ↔ uid/gid.
//!
//! Wraps the POSIX thread-safe `getpwnam_r` / `getgrnam_r` /
//! `getpwuid_r` / `getgrgid_r` calls in a small safe API. Used by
//! the File I/O layer for `nativeChown` (name → id) and for the
//! `stat`/`entries` record's `owner`/`group` fields (id → name).
//!
//! Missing users/groups return `None` rather than surfacing an
//! error, matching the FIP §Entries record shape convention:
//! fields that can't be resolved are simply omitted.
//!
//! The buffer for the strings that the `passwd` / `group` struct
//! fields point into starts at 512 bytes and doubles up to 64 KiB
//! on ERANGE, per the POSIX pattern. Systems with unusual /etc/passwd
//! entries (very long GECOS fields, huge group member lists) would
//! trip the ceiling; that's rare enough in practice that we prefer
//! bounded memory over unbounded growth.

use std::ffi::{CStr, CString};

const INITIAL_BUF: usize = 512;
const MAX_BUF: usize = 64 * 1024;

/// Resolve a user name to a uid. `None` if the user does not
/// exist, if `name` contains an interior null byte, or if the
/// lookup fails for any other reason.
pub fn resolve_uid(name: &str) -> Option<u32> {
    let c_name = CString::new(name).ok()?;
    let mut buf = vec![0u8; INITIAL_BUF];
    loop {
        // SAFETY: getpwnam_r requires a zero-initialized `passwd`
        // and a mutable pointer for the result. We give it both.
        // The strings inside `pwd` point into `buf`, which we keep
        // alive until we've copied out the uid.
        let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        let rc = unsafe {
            libc::getpwnam_r(
                c_name.as_ptr(),
                &mut pwd,
                buf.as_mut_ptr() as *mut libc::c_char,
                buf.len(),
                &mut result,
            )
        };
        if rc == 0 {
            // POSIX: rc==0 with result==NULL means "not found".
            if result.is_null() {
                return None;
            }
            return Some(pwd.pw_uid);
        }
        if rc == libc::ERANGE && buf.len() < MAX_BUF {
            buf.resize(buf.len() * 2, 0);
            continue;
        }
        return None;
    }
}

/// Resolve a group name to a gid. `None` on any failure or if the
/// group does not exist.
pub fn resolve_gid(name: &str) -> Option<u32> {
    let c_name = CString::new(name).ok()?;
    let mut buf = vec![0u8; INITIAL_BUF];
    loop {
        // SAFETY: same shape as resolve_uid.
        let mut grp: libc::group = unsafe { std::mem::zeroed() };
        let mut result: *mut libc::group = std::ptr::null_mut();
        let rc = unsafe {
            libc::getgrnam_r(
                c_name.as_ptr(),
                &mut grp,
                buf.as_mut_ptr() as *mut libc::c_char,
                buf.len(),
                &mut result,
            )
        };
        if rc == 0 {
            if result.is_null() {
                return None;
            }
            return Some(grp.gr_gid);
        }
        if rc == libc::ERANGE && buf.len() < MAX_BUF {
            buf.resize(buf.len() * 2, 0);
            continue;
        }
        return None;
    }
}

/// Reverse-lookup a uid to a user name. `None` if the uid has no
/// entry in the passwd database.
pub fn user_name(uid: u32) -> Option<String> {
    let mut buf = vec![0u8; INITIAL_BUF];
    loop {
        // SAFETY: same shape as resolve_uid.
        let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        let rc = unsafe {
            libc::getpwuid_r(
                uid,
                &mut pwd,
                buf.as_mut_ptr() as *mut libc::c_char,
                buf.len(),
                &mut result,
            )
        };
        if rc == 0 {
            if result.is_null() {
                return None;
            }
            // SAFETY: pwd.pw_name is a pointer into buf, valid while
            // buf is alive. We copy the bytes out before buf drops.
            let cstr = unsafe { CStr::from_ptr(pwd.pw_name) };
            return cstr.to_str().ok().map(str::to_owned);
        }
        if rc == libc::ERANGE && buf.len() < MAX_BUF {
            buf.resize(buf.len() * 2, 0);
            continue;
        }
        return None;
    }
}

/// Reverse-lookup a gid to a group name. `None` if the gid has no
/// entry in the group database.
pub fn group_name(gid: u32) -> Option<String> {
    let mut buf = vec![0u8; INITIAL_BUF];
    loop {
        // SAFETY: same shape as resolve_uid.
        let mut grp: libc::group = unsafe { std::mem::zeroed() };
        let mut result: *mut libc::group = std::ptr::null_mut();
        let rc = unsafe {
            libc::getgrgid_r(
                gid,
                &mut grp,
                buf.as_mut_ptr() as *mut libc::c_char,
                buf.len(),
                &mut result,
            )
        };
        if rc == 0 {
            if result.is_null() {
                return None;
            }
            // SAFETY: grp.gr_name is a pointer into buf, valid while
            // buf is alive. Copy out before drop.
            let cstr = unsafe { CStr::from_ptr(grp.gr_name) };
            return cstr.to_str().ok().map(str::to_owned);
        }
        if rc == libc::ERANGE && buf.len() < MAX_BUF {
            buf.resize(buf.len() * 2, 0);
            continue;
        }
        return None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every Unix system has a `root` user with uid 0. Round-trip
    /// through both directions of the lookup to confirm the FFI
    /// wiring works and CStr copy-out doesn't drop early.
    #[test]
    fn root_user_round_trips() {
        assert_eq!(resolve_uid("root"), Some(0));
        assert_eq!(user_name(0).as_deref(), Some("root"));
    }

    /// The primary group for uid 0 is `root` on Linux and `wheel`
    /// on macOS; both have gid 0. We check the gid, then round-trip
    /// its name back.
    #[test]
    fn gid_zero_round_trips() {
        let name = group_name(0).expect("gid 0 must have a name");
        assert_eq!(resolve_gid(&name), Some(0));
    }

    #[test]
    fn unknown_user_returns_none() {
        // A pseudo-random unlikely-to-exist username.
        assert!(resolve_uid("this-user-does-not-exist-42").is_none());
    }

    #[test]
    fn unknown_group_returns_none() {
        assert!(resolve_gid("this-group-does-not-exist-42").is_none());
    }

    #[test]
    fn unknown_uid_returns_none() {
        // A uid unlikely to appear on any test host. Skip if
        // somehow it exists; the point is to exercise the
        // null-result path.
        assert!(user_name(4_000_000_000).is_none());
    }

    #[test]
    fn interior_null_in_name_returns_none() {
        assert!(resolve_uid("root\0x").is_none());
    }
}
