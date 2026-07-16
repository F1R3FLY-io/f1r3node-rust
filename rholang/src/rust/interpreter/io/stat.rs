//! Encoders for the FIP `stat` / `entries` record shape.
//!
//! FIP §"[[TODO]] entries record shape" (Part 1 TODO 5) defines the
//! per-entry record. This module builds it as a Rholang `Map` with
//! string keys and typed values so both the standalone `nativeStat`
//! primitive and the per-entry loop in `nativeEntries` share one
//! source of truth for the field set and encoding.
//!
//! Field set emitted here:
//!
//! - `name`  : String — basename only (never contains `/`).
//! - `kind`  : String — one of `"file"`, `"dir"`, `"symlink"`, `"other"`.
//! - `size`  : Int    — bytes; present for regular files only.
//! - `mode`  : String — 9-char `"rwxrwxrwx"` style.
//! - `owner` : String — user name from NSS reverse-lookup; Unix only. Omitted if the uid has no NSS entry.
//! - `group` : String — group name from NSS reverse-lookup; Unix only. Omitted if the gid has no NSS entry.
//! - `mtime` : Int    — Unix epoch seconds.
//! - `ctime` : Int    — Unix epoch seconds.
//! - `atime` : Int    — Unix epoch seconds.
//!
//! Consensus-vs-oracular filtering (the FIP TODO 5 promise that
//! `mtime`/`ctime`/`atime`/`owner`/`group` are omitted in consensus
//! mode) is the agent layer's job, not the native primitive's. The
//! native layer emits every field it can compute; the agent layer
//! strips the per-mode ones before handing to the user.

use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use models::rhoapi::Par;

use crate::rust::interpreter::rho_type::{RhoNumber, RhoString};

/// Format a Unix mode's permission bits as a 9-char `"rwxr-xr-x"`
/// string. Higher bits (setuid/setgid/sticky) are ignored per the
/// FIP §"[[TODO]] entries record shape" note that those bits are
/// not exposed.
pub fn mode_string(mode_bits: u32) -> String {
    let bits = mode_bits & 0o777;
    let mut s = String::with_capacity(9);
    for shift in [6, 3, 0] {
        let triad = (bits >> shift) & 0b111;
        s.push(if triad & 0b100 != 0 { 'r' } else { '-' });
        s.push(if triad & 0b010 != 0 { 'w' } else { '-' });
        s.push(if triad & 0b001 != 0 { 'x' } else { '-' });
    }
    s
}

fn epoch_secs(t: std::io::Result<SystemTime>) -> Option<i64> {
    t.ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64)
}

/// Build the record for a file/dir/symlink at `basename` with the
/// given metadata. Uses `symlink_metadata` conventions -- callers
/// pass the metadata they want reflected (follow-symlink or not).
pub fn stat_record(basename: &str, meta: &std::fs::Metadata) -> HashMap<Par, Par> {
    let ft = meta.file_type();
    let kind = if ft.is_file() {
        "file"
    } else if ft.is_dir() {
        "dir"
    } else if ft.is_symlink() {
        "symlink"
    } else {
        "other"
    };

    let mut m: HashMap<Par, Par> = HashMap::new();
    m.insert(
        RhoString::create_par("name".to_string()),
        RhoString::create_par(basename.to_string()),
    );
    m.insert(
        RhoString::create_par("kind".to_string()),
        RhoString::create_par(kind.to_string()),
    );

    if ft.is_file() {
        m.insert(
            RhoString::create_par("size".to_string()),
            RhoNumber::create_par(meta.len() as i64),
        );
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        use super::nss;

        m.insert(
            RhoString::create_par("mode".to_string()),
            RhoString::create_par(mode_string(meta.permissions().mode())),
        );

        // owner/group per FIP §Entries record shape TODO 8. NSS
        // reverse-lookup may fail (uid orphaned from /etc/passwd,
        // NSS backend down, non-UTF-8 name); in that case the
        // field is omitted rather than fabricating a fallback,
        // matching the convention `mtime`/`ctime`/`atime` follow
        // for unavailable timestamps.
        if let Some(name) = nss::user_name(meta.uid()) {
            m.insert(
                RhoString::create_par("owner".to_string()),
                RhoString::create_par(name),
            );
        }
        if let Some(name) = nss::group_name(meta.gid()) {
            m.insert(
                RhoString::create_par("group".to_string()),
                RhoString::create_par(name),
            );
        }
    }
    #[cfg(not(unix))]
    {
        // FIP targets macOS + Linux only, but keep the code portable
        // enough that a Windows dev build still compiles cleanly. On
        // non-Unix hosts, emit the readonly-vs-writable bit as an
        // approximation. `owner`/`group` are omitted -- there's no
        // POSIX-compatible identity to report.
        let m_str = if meta.permissions().readonly() {
            "r--r--r--"
        } else {
            "rw-rw-rw-"
        };
        m.insert(
            RhoString::create_par("mode".to_string()),
            RhoString::create_par(m_str.to_string()),
        );
    }

    if let Some(t) = epoch_secs(meta.modified()) {
        m.insert(
            RhoString::create_par("mtime".to_string()),
            RhoNumber::create_par(t),
        );
    }
    if let Some(t) = epoch_secs(meta.created()) {
        m.insert(
            RhoString::create_par("ctime".to_string()),
            RhoNumber::create_par(t),
        );
    }
    if let Some(t) = epoch_secs(meta.accessed()) {
        m.insert(
            RhoString::create_par("atime".to_string()),
            RhoNumber::create_par(t),
        );
    }

    m
}

/// Convenience: pull the basename off a full path, defaulting to
/// the empty string if the path has no final component (e.g., `/`).
pub fn basename_of(path: &Path) -> String {
    path.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_string_covers_the_permission_grid() {
        assert_eq!(mode_string(0o000), "---------");
        assert_eq!(mode_string(0o777), "rwxrwxrwx");
        assert_eq!(mode_string(0o755), "rwxr-xr-x");
        assert_eq!(mode_string(0o644), "rw-r--r--");
        assert_eq!(mode_string(0o400), "r--------");
        // Higher bits (setuid/setgid/sticky) are ignored per the FIP.
        assert_eq!(mode_string(0o4755), "rwxr-xr-x");
    }

    #[test]
    fn stat_record_of_a_file_has_the_expected_shape() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"hello").unwrap();
        let meta = std::fs::metadata(tmp.path()).unwrap();
        let rec = stat_record(&basename_of(tmp.path()), &meta);
        let name_key = RhoString::create_par("name".to_string());
        let kind_key = RhoString::create_par("kind".to_string());
        let size_key = RhoString::create_par("size".to_string());
        let mode_key = RhoString::create_par("mode".to_string());
        assert!(rec.contains_key(&name_key));
        assert_eq!(
            RhoString::unapply(rec.get(&kind_key).unwrap()).as_deref(),
            Some("file")
        );
        assert_eq!(RhoNumber::unapply(rec.get(&size_key).unwrap()), Some(5));
        assert!(rec.contains_key(&mode_key));
    }

    #[test]
    fn stat_record_of_a_dir_omits_size() {
        let tmp = tempfile::tempdir().unwrap();
        let meta = std::fs::metadata(tmp.path()).unwrap();
        let rec = stat_record(&basename_of(tmp.path()), &meta);
        assert_eq!(
            RhoString::unapply(rec.get(&RhoString::create_par("kind".to_string())).unwrap())
                .as_deref(),
            Some("dir")
        );
        assert!(!rec.contains_key(&RhoString::create_par("size".to_string())));
    }

    #[test]
    #[cfg(unix)]
    fn stat_record_includes_owner_and_group_on_unix() {
        // The test process has a real uid/gid backed by /etc/passwd
        // and /etc/group entries on any CI/dev host we target.
        // If NSS reverse-lookup produces None (unlikely but possible),
        // the field is legitimately omitted and this test is
        // over-strict; if we ever hit that, downgrade to "assert
        // owner is either present or the uid is missing from NSS."
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let meta = std::fs::metadata(tmp.path()).unwrap();
        let rec = stat_record(&basename_of(tmp.path()), &meta);
        assert!(
            rec.contains_key(&RhoString::create_par("owner".to_string())),
            "owner field expected for a file owned by the test uid"
        );
        assert!(
            rec.contains_key(&RhoString::create_par("group".to_string())),
            "group field expected for a file owned by the test gid"
        );
    }
}
