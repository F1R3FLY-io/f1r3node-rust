// Access modes and permission-string parsing.
//
// Two orthogonal concepts share this module:
//
//   1. Fopen mode strings (`"r"`, `"rw"`, `"w+"`, ...) → `AccessMode` +
//      `OpenIntent`.  Spec §File.openFile lists the eight valid forms.
//
//   2. Chmod permission strings (`"rwxr-xr-x"`) → u16 permission bits.
//      Symbolic-delta forms (`"u+x"`) and octal (`"0755"`) are rejected —
//      spec §Dir.chmod.

use std::fs::OpenOptions;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessMode {
    Read,
    Write,
    ReadWrite,
}

/// What to do with a file that already exists at `open` time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExistPolicy {
    /// `"r"`, `"rw"` — must exist.  `"wx"`, `"w+x"` — must NOT exist.
    Require,
    RequireAbsent,
    /// `"w"`, `"w+"` — create-or-truncate.
    CreateOrTruncate,
    /// `"a"`, `"a+"` — create-or-append.
    CreateOrAppend,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpenIntent {
    pub mode: AccessMode,
    pub policy: ExistPolicy,
    pub append: bool,
    pub truncate: bool,
}

/// Parse the eight canonical fopen-mode strings from §File.openFile.
pub fn parse_open_mode(s: &str) -> Option<OpenIntent> {
    let (mode, policy, append, truncate) = match s {
        "r" => (AccessMode::Read, ExistPolicy::Require, false, false),
        "rw" => (AccessMode::ReadWrite, ExistPolicy::Require, false, false),
        "w" => (
            AccessMode::Write,
            ExistPolicy::CreateOrTruncate,
            false,
            true,
        ),
        "w+" => (
            AccessMode::ReadWrite,
            ExistPolicy::CreateOrTruncate,
            false,
            true,
        ),
        "wx" => (AccessMode::Write, ExistPolicy::RequireAbsent, false, false),
        "w+x" => (
            AccessMode::ReadWrite,
            ExistPolicy::RequireAbsent,
            false,
            false,
        ),
        "a" => (AccessMode::Write, ExistPolicy::CreateOrAppend, true, false),
        "a+" => (
            AccessMode::ReadWrite,
            ExistPolicy::CreateOrAppend,
            true,
            false,
        ),
        _ => return None,
    };
    Some(OpenIntent {
        mode,
        policy,
        append,
        truncate,
    })
}

/// Translate an `OpenIntent` to `std::fs::OpenOptions`.  Kept for the
/// Phase-5 File-agent that may prefer the higher-level API; the native
/// layer uses `fopen_flags` and issues `openat` directly.
pub fn open_options(intent: OpenIntent) -> OpenOptions {
    let mut opts = OpenOptions::new();
    match intent.mode {
        AccessMode::Read => {
            opts.read(true);
        }
        AccessMode::Write => {
            opts.write(true);
        }
        AccessMode::ReadWrite => {
            opts.read(true).write(true);
        }
    }
    match intent.policy {
        ExistPolicy::Require => { /* default: fail if not exists */ }
        ExistPolicy::RequireAbsent => {
            opts.create_new(true);
        }
        ExistPolicy::CreateOrTruncate => {
            opts.create(true).truncate(intent.truncate);
        }
        ExistPolicy::CreateOrAppend => {
            opts.create(true).append(intent.append);
        }
    }
    opts
}

/// Translate `OpenIntent` to raw `openat(2)` flags plus the file-creation
/// mode used when `O_CREAT` is set.  Callers (see `path::safe_open`)
/// combine the returned flags with `O_NOFOLLOW|O_CLOEXEC` before issuing
/// the syscall.
pub fn fopen_flags(intent: OpenIntent) -> (libc::c_int, libc::mode_t) {
    let mut flags: libc::c_int = match intent.mode {
        AccessMode::Read => libc::O_RDONLY,
        AccessMode::Write => libc::O_WRONLY,
        AccessMode::ReadWrite => libc::O_RDWR,
    };
    match intent.policy {
        ExistPolicy::Require => { /* no O_CREAT — open fails if absent */ }
        ExistPolicy::RequireAbsent => {
            flags |= libc::O_CREAT | libc::O_EXCL;
        }
        ExistPolicy::CreateOrTruncate => {
            flags |= libc::O_CREAT;
            if intent.truncate {
                flags |= libc::O_TRUNC;
            }
        }
        ExistPolicy::CreateOrAppend => {
            flags |= libc::O_CREAT;
            if intent.append {
                flags |= libc::O_APPEND;
            }
        }
    }
    (flags, 0o644)
}

/// Parse `"rwxr-xr-x"` (9 chars) to u16 permission bits.
///
/// Returns `None` for any other shape — symbolic-delta (`"u+x"`) and octal
/// (`"0755"`) are explicitly rejected per §Dir.chmod.
pub fn parse_chmod_mode(s: &str) -> Option<u16> {
    let bytes = s.as_bytes();
    if bytes.len() != 9 {
        return None;
    }
    let mut bits: u16 = 0;
    // Order: user-r, user-w, user-x, group-r, group-w, group-x, other-r, other-w, other-x
    let expected = [b'r', b'w', b'x', b'r', b'w', b'x', b'r', b'w', b'x'];
    for (i, &b) in bytes.iter().enumerate() {
        if b == expected[i] {
            bits |= 1 << (8 - i);
        } else if b != b'-' {
            return None;
        }
    }
    Some(bits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_eight_fopen_modes() {
        for s in ["r", "rw", "w", "w+", "wx", "w+x", "a", "a+"] {
            assert!(parse_open_mode(s).is_some(), "failed for {s}");
        }
    }

    #[test]
    fn rejects_unknown_mode() {
        assert!(parse_open_mode("").is_none());
        assert!(parse_open_mode("rwx").is_none());
        assert!(parse_open_mode("R").is_none());
    }

    #[test]
    fn parses_chmod_mode() {
        assert_eq!(parse_chmod_mode("rwxr-xr-x"), Some(0o755));
        assert_eq!(parse_chmod_mode("rw-r--r--"), Some(0o644));
        assert_eq!(parse_chmod_mode("---------"), Some(0o000));
        assert_eq!(parse_chmod_mode("rwxrwxrwx"), Some(0o777));
    }

    #[test]
    fn rejects_symbolic_and_octal() {
        assert_eq!(parse_chmod_mode("u+x"), None);
        assert_eq!(parse_chmod_mode("0755"), None);
        assert_eq!(parse_chmod_mode("wxrwxrwxr"), None); // out-of-order
    }
}
