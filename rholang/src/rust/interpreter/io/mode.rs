// Access modes and permission-string parsing.
//
// Two orthogonal concepts share this module:
//
//   1. Fopen mode strings (`"r"`, `"r+"`, `"w+"`, ...) → `AccessMode` +
//      `OpenIntent`.  Spec §File modes lists the eight valid forms.
//
//   2. Chmod permission strings (`"rwxr-xr-x"`) → `libc::mode_t`
//      permission bits.  Symbolic-delta forms (`"u+x"`), octal
//      (`"0755"`), and special bits (setuid/setgid/sticky via `s`/`t`
//      chars) are all rejected — spec §Dir.chmod.

use std::fs::OpenOptions;

/// Default file-creation permission bits when `O_CREAT` is used
/// without a caller-supplied mode.  Matches POSIX shell's default.
pub const DEFAULT_CREATE_MODE: libc::mode_t = 0o644;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessMode {
    Read,
    Write,
    ReadWrite,
}

/// What to do with a file that already exists at `open` time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExistPolicy {
    /// `"r"`, `"r+"` — file must already exist; open fails otherwise.
    Require,
    /// `"wx"`, `"w+x"` — file must NOT exist; open fails if it does.
    /// Maps to `O_CREAT | O_EXCL` / `create_new(true)`, which is
    /// TOCTOU-safe.
    RequireAbsent,
    /// `"w"`, `"w+"` — create-if-absent, always truncate.
    CreateOrTruncate,
    /// `"a"`, `"a+"` — create-if-absent, always append.
    CreateOrAppend,
}

/// Parsed `open` intent.  Fields are the minimal orthogonal set:
/// truncate and append behavior are determined by `policy`
/// (`CreateOrTruncate` implies truncate, `CreateOrAppend` implies
/// append) so callers cannot construct an inconsistent state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpenIntent {
    pub mode: AccessMode,
    pub policy: ExistPolicy,
}

/// Parse the eight canonical fopen-mode strings from §File.openFile.
///
/// `"rw"` was previously accepted as a synonym for `"r+"`.  Spec §File
/// modes enumerates exactly 8 forms (r / w / a / r+ / w+ / wx / w+x /
/// a+); accepting `"rw"` was a spec-shrinking alias.  Removed since no
/// shipping network encodes it and the alias forced every reader of
/// the FIP to also read the code to know which mode strings actually
/// parsed.
pub fn parse_open_mode(s: &str) -> Option<OpenIntent> {
    let (mode, policy) = match s {
        "r" => (AccessMode::Read, ExistPolicy::Require),
        "r+" => (AccessMode::ReadWrite, ExistPolicy::Require),
        "w" => (AccessMode::Write, ExistPolicy::CreateOrTruncate),
        "w+" => (AccessMode::ReadWrite, ExistPolicy::CreateOrTruncate),
        "wx" => (AccessMode::Write, ExistPolicy::RequireAbsent),
        "w+x" => (AccessMode::ReadWrite, ExistPolicy::RequireAbsent),
        "a" => (AccessMode::Write, ExistPolicy::CreateOrAppend),
        "a+" => (AccessMode::ReadWrite, ExistPolicy::CreateOrAppend),
        _ => return None,
    };
    Some(OpenIntent { mode, policy })
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
            opts.create(true).truncate(true);
        }
        ExistPolicy::CreateOrAppend => {
            opts.create(true).append(true);
        }
    }
    opts
}

/// Translate `OpenIntent` to raw `openat(2)` flags plus the file-
/// creation mode used when `O_CREAT` is set.
///
/// # Safety-neutral output
///
/// The returned flags are the intent-to-flags translation *only* —
/// no TOCTOU or symlink hardening is applied here.  A caller that
/// hands these flags directly to `openat` (bypassing
/// `path::safe_open`) can open a file *through* a symlink, defeating
/// the restricted-root guarantee.
///
/// The canonical call site is `path::safe_open`, which walks via
/// `safe_descend` (H-5 identity check + descent-level `O_NOFOLLOW`)
/// then combines these flags with `O_NOFOLLOW | O_CLOEXEC` on the
/// leaf open.
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
            flags |= libc::O_CREAT | libc::O_TRUNC;
        }
        ExistPolicy::CreateOrAppend => {
            flags |= libc::O_CREAT | libc::O_APPEND;
        }
    }
    (flags, DEFAULT_CREATE_MODE)
}

/// Parse `"rwxr-xr-x"` (9 chars) to `libc::mode_t` permission bits
/// (0..=0o777).
///
/// Returns `None` for any other shape:
/// - Symbolic-delta forms (`"u+x"`) — rejected per §Dir.chmod.
/// - Octal forms (`"0755"`) — rejected per §Dir.chmod.
/// - Special bits (setuid `s`, setgid `s`, sticky `t`) — rejected;
///   Rholang's capability-security model does not expose them.
pub fn parse_chmod_mode(s: &str) -> Option<libc::mode_t> {
    let bytes = s.as_bytes();
    if bytes.len() != 9 {
        return None;
    }
    let mut bits: libc::mode_t = 0;
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
        for s in ["r", "w", "w+", "wx", "w+x", "a", "a+", "r+"] {
            assert!(parse_open_mode(s).is_some(), "failed for {s}");
        }
    }

    /// Assert the full `OpenIntent` produced by each canonical mode
    /// string — pins that the parser's (mode, policy) pairing is
    /// exactly what §File modes prescribes.
    #[test]
    fn parses_each_mode_to_expected_intent() {
        use AccessMode::*;
        use ExistPolicy::*;
        let cases: &[(&str, AccessMode, ExistPolicy)] = &[
            ("r", Read, Require),
            ("r+", ReadWrite, Require),
            ("w", Write, CreateOrTruncate),
            ("w+", ReadWrite, CreateOrTruncate),
            ("wx", Write, RequireAbsent),
            ("w+x", ReadWrite, RequireAbsent),
            ("a", Write, CreateOrAppend),
            ("a+", ReadWrite, CreateOrAppend),
        ];
        for (s, expected_mode, expected_policy) in cases {
            let intent = parse_open_mode(s).unwrap_or_else(|| panic!("failed to parse {s}"));
            assert_eq!(intent.mode, *expected_mode, "mode mismatch for {s}");
            assert_eq!(intent.policy, *expected_policy, "policy mismatch for {s}");
        }
    }

    /// `"rw"` must not parse as a file mode.  A regression that
    /// reintroduces it as an alias for `r+` fails here.
    #[test]
    fn rw_is_not_a_valid_file_mode() {
        assert!(parse_open_mode("rw").is_none());
    }

    #[test]
    fn rejects_unknown_mode() {
        assert!(parse_open_mode("").is_none());
        assert!(parse_open_mode("rwx").is_none());
        assert!(parse_open_mode("R").is_none());
    }

    /// Table-driven pin for the syscall-facing `fopen_flags` — asserts
    /// the exact `openat(2)` flag bitmap for each of the 8 canonical
    /// mode strings.  This is where a bit-mask bug would hide, so it
    /// gets its own explicit table.
    #[test]
    fn fopen_flags_matches_expected_bitmap_for_each_mode() {
        let cases: &[(&str, libc::c_int)] = &[
            ("r", libc::O_RDONLY),
            ("r+", libc::O_RDWR),
            ("w", libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC),
            ("w+", libc::O_RDWR | libc::O_CREAT | libc::O_TRUNC),
            ("wx", libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL),
            ("w+x", libc::O_RDWR | libc::O_CREAT | libc::O_EXCL),
            ("a", libc::O_WRONLY | libc::O_CREAT | libc::O_APPEND),
            ("a+", libc::O_RDWR | libc::O_CREAT | libc::O_APPEND),
        ];
        for (s, expected_flags) in cases {
            let intent = parse_open_mode(s).unwrap();
            let (flags, mode) = fopen_flags(intent);
            assert_eq!(
                flags, *expected_flags,
                "flag mismatch for {s}: expected 0x{:x}, got 0x{:x}",
                *expected_flags, flags,
            );
            assert_eq!(
                mode, DEFAULT_CREATE_MODE,
                "creation mode must be DEFAULT_CREATE_MODE for {s}"
            );
        }
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

    /// Special bits (setuid / setgid / sticky) are not expressible in
    /// this parser — the canonical 9-char string only carries the
    /// r/w/x/- alphabet.  Any `s` or `t` character in a position that
    /// would normally hold `x` is rejected.
    #[test]
    fn rejects_setuid_setgid_sticky() {
        assert_eq!(parse_chmod_mode("rwsr-xr-x"), None, "setuid rejected");
        assert_eq!(parse_chmod_mode("rwxr-sr-x"), None, "setgid rejected");
        assert_eq!(parse_chmod_mode("rwxr-xr-t"), None, "sticky rejected");
    }
}
