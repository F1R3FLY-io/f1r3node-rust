// Safe path descent (TOCTOU-hardened).
//
// The naive approach walks components with `symlink_metadata` checks
// and then lets the handler layer pass a resolved `PathBuf` to
// `std::fs::*`.  Between the check and the syscall an attacker with
// write access to any subdirectory can `rename(subdir, .tmp) &&
// symlink(/, subdir)` and escape the root — a check-then-syscall
// race the plan explicitly forbids.
//
// The `path::descend` submodule instead opens `root` as a dirfd and
// descends via
// `openat(dirfd, name, O_NOFOLLOW|O_DIRECTORY|O_RDONLY|O_CLOEXEC)`
// at every step.  Any symlink component (or race that inserts one)
// short-circuits with `ELOOP`, surfaced here as
// `QuarantineError::SymlinkComponent`.  Callers receive a
// `SafeParent { dirfd, leaf }` and issue every subsequent syscall
// via `*at` against that dirfd — so the resolution path used at
// check time is the same one used at operation time.  TOCTOU-immune.
//
// macOS + Linux only; the whole submodule is `#[cfg(unix)]`-gated
// (see `compile_error!` below) because the design depends on
// `openat` / `O_NOFOLLOW` etc., none of which have a
// consensus-safe non-unix analogue.
//
// # Layout
//
// - `path` (this module) — primitives: `QuarantineError`,
//   `SafeParent`, `io_msg_scrub`, `canonicalize_lexical`,
//   `quarantine_err_reply`.
// - `path::descend` — `safe_descend` + `safe_descend_verified`
//   (the TOCTOU-immune walk).  *(follow-up PR)*
// - `path::identity` — `RootIdentityRegistry` + `capture_root_identity`
//   (H-5 rename-and-recreate defense).  *(follow-up PR)*
// - `path::open` — `safe_open` + `safe_open_verified`.  *(follow-up PR)*

#[cfg(not(unix))]
compile_error!(
    "rholang::interpreter::io::path requires a unix target: the TOCTOU-immune \
     design depends on openat / O_NOFOLLOW / dirfd semantics with no \
     consensus-safe non-unix analogue."
);

pub mod descend;
pub mod identity;
pub mod open;

use std::ffi::CString;
use std::io;
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::{Component, Path, PathBuf};

use super::errors::{io_err_code, FserrCode, FSERR_BAD_ARG, FSERR_QUARANTINE};

#[derive(Debug, PartialEq, Eq)]
pub enum QuarantineError {
    /// Empty relative path.
    Empty,
    /// Relative path collapses to `.` — refers to the root itself,
    /// which is not a valid leaf for `open` / `stat` / `unlink`.
    RootSelf,
    /// Path attempts to escape the root (leading `/`, `..` component,
    /// or resolves upward past the root).
    EscapesRoot,
    /// A path component is (or became, during descent) a symlink.
    /// Any symlink in the descent path short-circuits with `ELOOP`
    /// from `openat(O_NOFOLLOW)`.
    SymlinkComponent,
    /// The registered root directory's `(dev, inode)` pair no longer
    /// matches the one captured at boot — the classic
    /// rename-and-recreate attack that `O_NOFOLLOW` does not close.
    /// See `path::identity` (follow-up PR) for the boot-time
    /// capture and per-descent verification path.
    RootIdentityChanged,
    /// Any other syscall failure during descent.  The `ErrorKind`
    /// lets `quarantine_err_reply` route to the spec-canonical FSERR
    /// (`AlreadyExists` → `FSERR_ALREADY_EXISTS`, `NotFound` →
    /// `FSERR_NOT_FOUND`, etc.) via `errors::io_err_code`; the
    /// `String` carries the scrubbed message from `io_msg_scrub`.
    IoError(io::ErrorKind, String),
}

/// A safely-resolved leaf position: a dirfd for the parent directory
/// and the leaf's name (as a `CString` ready for `*at` syscalls).
///
/// Callers issue every subsequent syscall via `*at` against the
/// dirfd — never by rebuilding the path — or the TOCTOU-immunity
/// is lost.
#[derive(Debug)]
pub struct SafeParent {
    pub dirfd: OwnedFd,
    pub leaf: CString,
}

impl SafeParent {
    pub fn as_raw_fd(&self) -> i32 { self.dirfd.as_raw_fd() }

    /// Raw pointer to the NUL-terminated leaf name, suitable for `*at`
    /// syscalls.  The pointer is valid for the lifetime of `&self`
    /// (borrowed from `self.leaf.as_ptr()`); callers MUST NOT retain
    /// it past the `&self` borrow, or the underlying `CString` may
    /// have dropped and the pointer dangles.
    pub fn leaf_ptr(&self) -> *const libc::c_char { self.leaf.as_ptr() }
}

/// Scrub a `std::io::Error` down to a stable, consensus-safe
/// classification string.
///
/// # Why not `format!("{}", e.kind())`
///
/// `std::io::ErrorKind`'s `Display` impl is defined by the standard
/// library and its exact wording is not stability-committed across
/// Rust minor versions.  If validators run different Rust versions,
/// a stdlib wording tweak (e.g., "entity not found" → "not found")
/// would silently produce different msg bytes for the same
/// underlying kind — and the msg goes into the `[false, code, msg]`
/// reply `Par` which the WAL entry hashes.  Consensus fork.
///
/// This function maps each `ErrorKind` to a hardcoded string that
/// we control.  Adding a new mapping or changing an existing one
/// IS a consensus surface change — treat as a coordinated protocol
/// update, not a mechanical refactor.
///
/// Unknown / future `ErrorKind` variants fall back to the generic
/// `"io error"` so a Rust upgrade that adds new variants can't
/// silently leak a stdlib-derived string.
pub fn io_msg_scrub(e: &std::io::Error) -> String { io_kind_wire(e.kind()).to_string() }

/// The consensus-observable string for each `ErrorKind`.  See
/// [`io_msg_scrub`] for why this is hardcoded rather than deferring
/// to stdlib `Display`.
fn io_kind_wire(kind: std::io::ErrorKind) -> &'static str {
    use std::io::ErrorKind::*;
    match kind {
        NotFound => "not found",
        PermissionDenied => "permission denied",
        AlreadyExists => "already exists",
        InvalidInput => "invalid input",
        InvalidData => "invalid data",
        Unsupported => "unsupported",
        // Everything else (Other, WouldBlock, TimedOut, Interrupted,
        // etc.) collapses to a generic string.  A caller that needs
        // finer classification should add a mapping HERE (and pin
        // it in the test below); adding new mappings is a
        // consensus surface change.
        _ => "io error",
    }
}

/// Lexically normalize `PathBuf::from(root).join(rel)` so equivalent
/// `rel` forms produce identical `PathBuf`s.  Removes `.` components
/// (`Component::CurDir`) and relies on `Path::components()` to
/// collapse duplicate separators (`//` → `/`).  Does NOT resolve
/// symlinks — that's `canonicalize`'s job and requires disk I/O.
/// This is a pure lexical rewrite suitable for consensus WAL entries
/// where the canonical string must be deterministic per-input
/// independent of host state.
///
/// Fail-closed on escape attempts:
/// - `rel` containing `..` (a `Component::ParentDir`) →
///   `Err(QuarantineError::EscapesRoot)`.
/// - `rel` starting with `/` (absolute) → `Err(QuarantineError::EscapesRoot)`.
///
/// This is defense-in-depth — `safe_descend` (follow-up PR) is the
/// load-bearing gate, but making this function fallible closes the
/// escape hatch permanently if a caller bypasses it.
pub fn canonicalize_lexical(root: &str, rel: &str) -> Result<PathBuf, QuarantineError> {
    let rel_path = Path::new(rel);
    for component in rel_path.components() {
        match component {
            Component::ParentDir | Component::RootDir => {
                return Err(QuarantineError::EscapesRoot);
            }
            // `Prefix` is Windows-only; the module is unix-gated, so
            // it never occurs in practice.  `CurDir` / `Normal` are
            // fine at this stage — `CurDir` is dropped during
            // normalization below.
            _ => {}
        }
    }
    let joined = PathBuf::from(root).join(rel_path);
    let mut normalized = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::CurDir => {} // skip `.`
            other => normalized.push(other),
        }
    }
    Ok(normalized)
}

/// Translate `QuarantineError` to the `(code, message)` pair for the
/// `[false, code, msg]` reply shape.  `code` is a `FserrCode` (the
/// newtype from `errors`) so the compiler rejects raw-string misuse
/// at every call site — see `errors::FserrCode`.
pub fn quarantine_err_reply(e: &QuarantineError) -> (FserrCode, String) {
    match e {
        QuarantineError::Empty => (FSERR_BAD_ARG, "empty relative path".into()),
        QuarantineError::RootSelf => (FSERR_BAD_ARG, "path resolves to root itself".into()),
        QuarantineError::EscapesRoot => (FSERR_QUARANTINE, "path escapes root".into()),
        QuarantineError::SymlinkComponent => {
            (FSERR_QUARANTINE, "symlink in path components".into())
        }
        QuarantineError::RootIdentityChanged => (
            FSERR_QUARANTINE,
            "provisioned root's (dev, inode) does not match boot-captured \
             identity — possible rename-and-recreate attack (H-5); check \
             for out-of-band mv/rebind on the provisioned path"
                .into(),
        ),
        QuarantineError::IoError(kind, m) => {
            // Route via `io_err_code` so AlreadyExists / NotFound /
            // PermissionDenied / Unsupported reach their spec-canonical
            // FSERR codes instead of collapsing to FSERR_IO.  Kinds
            // without a dedicated FSERR (e.g. `Other`) fall back to
            // FSERR_IO via `io_err_code`'s default arm.
            let synthetic = io::Error::from(*kind);
            (io_err_code(&synthetic), m.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::interpreter::io::errors::{
        FSERR_ALREADY_EXISTS, FSERR_IO, FSERR_NOT_FOUND, FSERR_PERM, FSERR_UNSUPPORTED,
    };

    // --- canonicalize_lexical -------------------------------------

    #[test]
    fn canonicalize_lexical_removes_cur_dir_components() {
        assert_eq!(
            canonicalize_lexical("/root", "./a/./b.txt").unwrap(),
            PathBuf::from("/root/a/b.txt")
        );
    }

    #[test]
    fn canonicalize_lexical_collapses_double_separators() {
        assert_eq!(
            canonicalize_lexical("/root", "a//b.txt").unwrap(),
            PathBuf::from("/root/a/b.txt")
        );
    }

    #[test]
    fn canonicalize_lexical_plain_paths_are_unchanged() {
        assert_eq!(
            canonicalize_lexical("/root", "a/b.txt").unwrap(),
            PathBuf::from("/root/a/b.txt")
        );
    }

    /// The determinism property: equivalent `rel` forms produce the
    /// SAME `PathBuf`.
    #[test]
    fn canonicalize_lexical_two_equivalent_forms_agree() {
        let a = canonicalize_lexical("/root", "a/b.txt").unwrap();
        let b = canonicalize_lexical("/root", "./a/b.txt").unwrap();
        let c = canonicalize_lexical("/root", "a//b.txt").unwrap();
        assert_eq!(a, b);
        assert_eq!(a, c);
    }

    /// Trailing slash on `root` must not affect the output.  If
    /// `PathBuf::join`'s handling of the trailing slash differed
    /// from a bare `root`, WAL entries would drift depending on
    /// how the caller happened to spell the root.
    #[test]
    fn canonicalize_lexical_trailing_slash_on_root_normalizes() {
        assert_eq!(
            canonicalize_lexical("/root/", "a").unwrap(),
            canonicalize_lexical("/root", "a").unwrap()
        );
    }

    /// Empty `rel` returns just the root.  `safe_descend` rejects
    /// this upstream (with `QuarantineError::Empty`), but pin the
    /// pass-through so a future refactor that changes the behavior
    /// here surfaces at CI rather than at runtime.
    #[test]
    fn canonicalize_lexical_empty_rel_returns_root() {
        assert_eq!(
            canonicalize_lexical("/root", "").unwrap(),
            PathBuf::from("/root")
        );
    }

    // --- canonicalize_lexical: fail-closed on escape ---------------

    /// A `..` component in `rel` is rejected outright rather than
    /// silently normalized.  Defense-in-depth for a caller that
    /// bypasses `safe_descend`'s upstream gate.
    #[test]
    fn canonicalize_lexical_rejects_parent_dir() {
        assert_eq!(
            canonicalize_lexical("/root", "../etc/passwd"),
            Err(QuarantineError::EscapesRoot)
        );
        assert_eq!(
            canonicalize_lexical("/root", "a/../b"),
            Err(QuarantineError::EscapesRoot)
        );
        assert_eq!(
            canonicalize_lexical("/root", "a/b/.."),
            Err(QuarantineError::EscapesRoot)
        );
    }

    /// A `rel` that starts with `/` is absolute and (via
    /// `PathBuf::join`) would replace the root entirely — reject
    /// as escape.
    #[test]
    fn canonicalize_lexical_rejects_absolute_rel() {
        assert_eq!(
            canonicalize_lexical("/root", "/etc/passwd"),
            Err(QuarantineError::EscapesRoot)
        );
        assert_eq!(
            canonicalize_lexical("/root", "/"),
            Err(QuarantineError::EscapesRoot)
        );
    }

    // --- io_msg_scrub / io_kind_wire -------------------------------

    /// Consensus-observable wire strings are hardcoded, NOT derived
    /// from `std::io::ErrorKind::Display` — changing the stdlib's
    /// wording between Rust versions must not silently fork the
    /// network.  Each mapping is a consensus surface; adding or
    /// changing one is a coordinated protocol update.
    #[test]
    fn io_msg_scrub_returns_hardcoded_wire_strings() {
        use std::io::{Error, ErrorKind};
        assert_eq!(io_msg_scrub(&Error::from(ErrorKind::NotFound)), "not found");
        assert_eq!(
            io_msg_scrub(&Error::from(ErrorKind::PermissionDenied)),
            "permission denied"
        );
        assert_eq!(
            io_msg_scrub(&Error::from(ErrorKind::AlreadyExists)),
            "already exists"
        );
        assert_eq!(
            io_msg_scrub(&Error::from(ErrorKind::InvalidInput)),
            "invalid input"
        );
        assert_eq!(
            io_msg_scrub(&Error::from(ErrorKind::InvalidData)),
            "invalid data"
        );
        assert_eq!(
            io_msg_scrub(&Error::from(ErrorKind::Unsupported)),
            "unsupported"
        );
        // Unknown / future kinds collapse to a generic string,
        // decoupled from stdlib version.
        assert_eq!(io_msg_scrub(&Error::from(ErrorKind::Other)), "io error");
        assert_eq!(
            io_msg_scrub(&Error::from(ErrorKind::WouldBlock)),
            "io error"
        );
        assert_eq!(io_msg_scrub(&Error::from(ErrorKind::TimedOut)), "io error");
        assert_eq!(
            io_msg_scrub(&Error::from(ErrorKind::Interrupted)),
            "io error"
        );
    }

    // --- quarantine_err_reply -------------------------------------

    /// Each `QuarantineError` variant maps to the spec-canonical
    /// `FserrCode`.  Pinning the mapping here prevents a routing
    /// regression from silently returning the wrong error code to
    /// callers.
    #[test]
    fn quarantine_err_reply_routes_variants_to_canonical_fserr() {
        let (code, _) = quarantine_err_reply(&QuarantineError::Empty);
        assert_eq!(code, FSERR_BAD_ARG);

        let (code, _) = quarantine_err_reply(&QuarantineError::RootSelf);
        assert_eq!(code, FSERR_BAD_ARG);

        let (code, _) = quarantine_err_reply(&QuarantineError::EscapesRoot);
        assert_eq!(code, FSERR_QUARANTINE);

        let (code, _) = quarantine_err_reply(&QuarantineError::SymlinkComponent);
        assert_eq!(code, FSERR_QUARANTINE);

        let (code, _) = quarantine_err_reply(&QuarantineError::RootIdentityChanged);
        assert_eq!(code, FSERR_QUARANTINE);
    }

    /// `IoError` variants delegate to `errors::io_err_code`, which
    /// maps each `ErrorKind` to its spec-canonical FSERR.  Pin the
    /// full routing table so a regression in either
    /// `quarantine_err_reply` or `io_err_code` surfaces here.
    #[test]
    fn quarantine_err_reply_io_variants_route_via_io_err_code() {
        let cases: &[(std::io::ErrorKind, super::FserrCode)] = &[
            (std::io::ErrorKind::NotFound, FSERR_NOT_FOUND),
            (std::io::ErrorKind::PermissionDenied, FSERR_PERM),
            (std::io::ErrorKind::AlreadyExists, FSERR_ALREADY_EXISTS),
            (std::io::ErrorKind::InvalidInput, FSERR_BAD_ARG),
            (std::io::ErrorKind::InvalidData, FSERR_BAD_ARG),
            (std::io::ErrorKind::Unsupported, FSERR_UNSUPPORTED),
            (std::io::ErrorKind::Other, FSERR_IO),
        ];
        for (kind, expected) in cases {
            let err = QuarantineError::IoError(*kind, "msg".into());
            let (code, msg) = quarantine_err_reply(&err);
            assert_eq!(
                code, *expected,
                "IoError({kind:?}) should route to {expected:?}"
            );
            assert_eq!(msg, "msg");
        }
    }
}
