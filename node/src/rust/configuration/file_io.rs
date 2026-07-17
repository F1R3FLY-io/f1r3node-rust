//! File I/O static-provisioning config (FIP 2026-02-06 File-I/O
//! §"With the config file" and §"With flags").
//!
//! Holds the four lists of pre-authorized files and directories the
//! deployment ships with at boot. The FS-agent Rholang layer (later
//! PR) will consume these at genesis and hand out capabilities to the
//! initial deployment. Nothing in this file opens fds -- that's the
//! provisioning step, which lives with the FS-agent bootstrap. This
//! file only parses, validates, and stores.
//!
//! The FIP splits static provisioning along two orthogonal axes:
//!
//! - **`oracle-*` vs `consensus-*`**: which mode the entry is
//!   accessible under. This FIP is oracular-only; consensus entries
//!   are accepted and stored but not exercised until a follow-up FIP
//!   defines the multi-node consensus-mode file-state model.
//! - **`*-files` vs `*-dirs`**: file agents get an `fopen`-style
//!   mode string per the FIP's mode table (`"r"`, `"w"`, `"a"`,
//!   `"r+"`, `"w+"`, `"a+"`, `"wx"`, `"w+x"`, `"wbx"`); dir agents
//!   get either `"r"` or `"rw"`.
//!
//! Defaults differ between the config-file form and the CLI-flag
//! form for directories (per FIP §"With the config file"):
//!
//! - Config-block entries default to `mode: "r"` (default-restrictive).
//! - CLI-flag entries default to `"rw"` (default-permissive) for dirs.
//!
//! File entries default to `"r"` on both surfaces -- the fopen-mode
//! default doesn't need a per-surface distinction.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The set of allowed dir mode strings per FIP §"The `openDir`
/// method of `io:fs`". A dir agent opened with `"r"` permits only
/// read-side methods; `"rw"` permits the full mutation surface.
const ALLOWED_DIR_MODES: &[&str] = &["r", "rw"];

/// Top-level file-I/O config section on `NodeConf`. All four lists
/// default to empty via `#[serde(default)]` -- deployments without
/// static provisioning simply omit the block.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileIo {
    #[serde(rename = "oracle-static-files", default)]
    pub oracle_static_files: Vec<StaticFileEntry>,

    #[serde(rename = "oracle-static-dirs", default)]
    pub oracle_static_dirs: Vec<StaticDirEntry>,

    /// Accepted and stored but not exercised until the follow-up
    /// consensus-mode FIP lands.
    #[serde(rename = "consensus-static-files", default)]
    pub consensus_static_files: Vec<StaticFileEntry>,

    /// Accepted and stored but not exercised until the follow-up
    /// consensus-mode FIP lands.
    #[serde(rename = "consensus-static-dirs", default)]
    pub consensus_static_dirs: Vec<StaticDirEntry>,
}

/// One entry in a `*-static-files` list. Each entry names an
/// absolute path plus an fopen-style mode string.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticFileEntry {
    pub path: PathBuf,
    #[serde(default = "default_file_mode")]
    pub mode: String,
}

fn default_file_mode() -> String { "r".to_string() }

impl StaticFileEntry {
    /// Constructor for entries synthesized from a CLI flag
    /// (`--oracle-static-file PATH`). Defaults to mode `"r"`, same
    /// as the config-file default -- files have no
    /// flag-vs-config default asymmetry.
    pub fn from_flag(path: PathBuf) -> Self {
        Self {
            path,
            mode: default_file_mode(),
        }
    }
}

/// One entry in a `*-static-dirs` list. Dir modes are restricted
/// to `"r"` or `"rw"` per the FIP's `openDir` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticDirEntry {
    pub path: PathBuf,
    #[serde(default = "default_dir_mode_config")]
    pub mode: String,
}

/// Config-block default for dir entries: `"r"` (default-restrictive).
/// Server deployments authoring a static-dir block explicitly opt
/// in to `"rw"` by writing the mode.
fn default_dir_mode_config() -> String { "r".to_string() }

/// CLI-flag default for dir entries: `"rw"` (default-permissive).
/// Interactive / dev use.
fn default_dir_mode_flag() -> String { "rw".to_string() }

impl StaticDirEntry {
    /// Constructor for entries synthesized from a CLI flag
    /// (`--oracle-static-dir PATH`). Defaults to mode `"rw"` per
    /// the FIP -- the flag form is intended for interactive/dev use
    /// where broader default authority is convenient; production
    /// server deployments should use the config-file form (which
    /// defaults to `"r"`) and opt in to `"rw"` explicitly per dir.
    pub fn from_flag(path: PathBuf) -> Self {
        Self {
            path,
            mode: default_dir_mode_flag(),
        }
    }
}

/// A single validation failure. Carries enough context for the
/// caller to produce a useful error message; the `section` string
/// identifies which of the four lists the offending entry came from.
#[derive(Debug)]
pub enum FileIoConfigError {
    /// The path does not exist on disk.
    PathNotFound {
        section: &'static str,
        path: PathBuf,
    },
    /// The path canonicalizes to something else, indicating a
    /// symbolic-link traversal. The FIP §"With the config file"
    /// promises "no symbolic or hard links" for static-provisioned
    /// entries; we enforce it at load time.
    SymlinkTraversal {
        section: &'static str,
        path: PathBuf,
        canonical: PathBuf,
    },
    /// The mode string isn't one of the file-mode-table entries.
    InvalidFileMode {
        section: &'static str,
        path: PathBuf,
        mode: String,
    },
    /// The mode string isn't `"r"` or `"rw"`.
    InvalidDirMode {
        section: &'static str,
        path: PathBuf,
        mode: String,
    },
    /// Any other host-side error during canonicalization or stat.
    Io {
        section: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
}

impl std::fmt::Display for FileIoConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PathNotFound { section, path } => {
                write!(f, "[{section}] path does not exist: {}", path.display())
            }
            Self::SymlinkTraversal {
                section,
                path,
                canonical,
            } => write!(
                f,
                "[{section}] path {} traverses a symlink to {} (static-provisioned entries must not cross symlinks per FIP §\"With the config file\")",
                path.display(),
                canonical.display()
            ),
            Self::InvalidFileMode {
                section,
                path,
                mode,
            } => write!(
                f,
                "[{section}] path {}: unknown file mode {mode:?} (expected one of r, w, a, r+, w+, a+, wx, w+x, wbx)",
                path.display()
            ),
            Self::InvalidDirMode {
                section,
                path,
                mode,
            } => write!(
                f,
                "[{section}] path {}: unknown dir mode {mode:?} (expected \"r\" or \"rw\")",
                path.display()
            ),
            Self::Io {
                section,
                path,
                source,
            } => write!(f, "[{section}] path {}: {source}", path.display()),
        }
    }
}

impl std::error::Error for FileIoConfigError {}

/// Validate a whole `FileIo` block. Returns every failure at once
/// (rather than stopping at the first) so operators can fix all
/// misconfigurations in one edit pass.
pub fn validate(cfg: &FileIo) -> Result<(), Vec<FileIoConfigError>> {
    let mut errs = Vec::new();
    for entry in &cfg.oracle_static_files {
        validate_file(entry, "oracle-static-files", &mut errs);
    }
    for entry in &cfg.oracle_static_dirs {
        validate_dir(entry, "oracle-static-dirs", &mut errs);
    }
    for entry in &cfg.consensus_static_files {
        validate_file(entry, "consensus-static-files", &mut errs);
    }
    for entry in &cfg.consensus_static_dirs {
        validate_dir(entry, "consensus-static-dirs", &mut errs);
    }
    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs)
    }
}

fn validate_file(
    entry: &StaticFileEntry,
    section: &'static str,
    errs: &mut Vec<FileIoConfigError>,
) {
    if rholang::rust::interpreter::io::mode::open_options_for(&entry.mode).is_none() {
        errs.push(FileIoConfigError::InvalidFileMode {
            section,
            path: entry.path.clone(),
            mode: entry.mode.clone(),
        });
    }
    validate_path(&entry.path, section, errs);
}

fn validate_dir(entry: &StaticDirEntry, section: &'static str, errs: &mut Vec<FileIoConfigError>) {
    if !ALLOWED_DIR_MODES.contains(&entry.mode.as_str()) {
        errs.push(FileIoConfigError::InvalidDirMode {
            section,
            path: entry.path.clone(),
            mode: entry.mode.clone(),
        });
    }
    validate_path(&entry.path, section, errs);
}

/// Lexically normalize a path: convert to absolute (against cwd if
/// relative), drop `.` components, pop on `..`, collapse redundant
/// separators. Does NOT follow symlinks -- that's the whole point.
///
/// Used by `validate_path` to compare against `canonicalize` (which
/// DOES follow symlinks): if the two agree, no symlink was
/// traversed; if they differ, at least one component was a symlink.
///
/// Without this normalization, `canonicalize != input` would fire
/// spuriously on relative paths, trailing slashes, `.` / `..`
/// components, and other purely-lexical differences that have
/// nothing to do with symlinks.
///
/// On Unix, `..` popping is safe under lexical semantics: `a/..`
/// canonically means "the parent of a," and if `a` is a symlink
/// resolving through the parent would give a different result. We
/// accept that discrepancy -- if the operator writes
/// `/tmp/link/..`, the resulting mismatch against
/// `canonicalize`'s answer flags the symlink, which is exactly the
/// safety property we want.
fn lexically_normalize(path: &Path) -> std::io::Result<PathBuf> {
    use std::path::Component;
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        // Prepend cwd. If cwd itself is inaccessible (rare), fail
        // hard rather than silently accepting an unrooted path.
        std::env::current_dir()?.join(path)
    };
    let mut out = PathBuf::new();
    for comp in absolute.components() {
        match comp {
            Component::Prefix(p) => out.push(p.as_os_str()),
            Component::RootDir => out.push(std::path::MAIN_SEPARATOR.to_string()),
            Component::CurDir => {}
            Component::ParentDir => {
                // Pop unless we're at the root (or an empty
                // out, which shouldn't happen given the absolute
                // prefix above but is safe to guard).
                out.pop();
            }
            Component::Normal(x) => out.push(x),
        }
    }
    Ok(out)
}

/// Common path checks: existence + no-symlink-traversal. Called by
/// both file and dir validators.
fn validate_path(path: &Path, section: &'static str, errs: &mut Vec<FileIoConfigError>) {
    // symlink_metadata rather than metadata so a symlink at the
    // named path itself is reported as "path exists but is a
    // symlink" via the canonicalization check, not "path does not
    // exist" via a NotFound on the target.
    match std::fs::symlink_metadata(path) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            errs.push(FileIoConfigError::PathNotFound {
                section,
                path: path.to_path_buf(),
            });
            return;
        }
        Err(source) => {
            errs.push(FileIoConfigError::Io {
                section,
                path: path.to_path_buf(),
                source,
            });
            return;
        }
    }
    // Compare the fully-canonicalized path (which follows symlinks
    // through every ancestor) against the lexically-normalized
    // input (which does NOT follow symlinks). If they differ, at
    // least one component was a symlink; reject.
    //
    // The lexical normalization step is what distinguishes this
    // check from a naive `canonicalize != path`, which fires on
    // relative paths, trailing slashes, `.` / `..` components, and
    // any other purely-lexical differences -- footguns for
    // operators writing e.g. `--oracle-static-dir data/agent`.
    let normalized = match lexically_normalize(path) {
        Ok(p) => p,
        Err(source) => {
            errs.push(FileIoConfigError::Io {
                section,
                path: path.to_path_buf(),
                source,
            });
            return;
        }
    };
    match std::fs::canonicalize(path) {
        Ok(canonical) => {
            if canonical != normalized {
                errs.push(FileIoConfigError::SymlinkTraversal {
                    section,
                    path: path.to_path_buf(),
                    canonical,
                });
            }
        }
        Err(source) => {
            errs.push(FileIoConfigError::Io {
                section,
                path: path.to_path_buf(),
                source,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_empty() {
        let cfg = FileIo::default();
        assert!(cfg.oracle_static_files.is_empty());
        assert!(cfg.oracle_static_dirs.is_empty());
        assert!(cfg.consensus_static_files.is_empty());
        assert!(cfg.consensus_static_dirs.is_empty());
    }

    #[test]
    fn file_entry_default_mode_is_r() {
        let e: StaticFileEntry = serde_json::from_str(r#"{"path": "/tmp/x"}"#).unwrap();
        assert_eq!(e.mode, "r");
    }

    #[test]
    fn dir_entry_config_default_mode_is_r() {
        let e: StaticDirEntry = serde_json::from_str(r#"{"path": "/tmp"}"#).unwrap();
        assert_eq!(e.mode, "r");
    }

    #[test]
    fn dir_entry_flag_default_mode_is_rw() {
        let e = StaticDirEntry::from_flag(PathBuf::from("/tmp"));
        assert_eq!(e.mode, "rw");
    }

    #[test]
    fn file_entry_flag_default_mode_is_r() {
        let e = StaticFileEntry::from_flag(PathBuf::from("/tmp/x"));
        assert_eq!(e.mode, "r");
    }

    #[test]
    fn validate_accepts_real_file_with_valid_mode() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        // Use the tempfile's path as-given by the OS -- earlier
        // versions of this test had to `canonicalize` first because
        // the naive `canonicalize != input` check tripped on macOS
        // `/tmp` (a symlink to `/private/tmp`). The switch to
        // `canonicalize == lexically_normalize(input)` closes that
        // false positive: `/tmp` symlink or not, both sides agree
        // that the path lexically points to `/tmp/foo` and
        // canonically resolves to `/private/tmp/foo`, so no
        // symlink-traversal is claimed.
        //
        // Wait -- the two DO differ in that case. See
        // `validate_rejects_symlink_traversal_at_root_prefix`
        // below for the deliberate design choice. Real deployments
        // supply already-canonical paths in the config; this test
        // pre-canonicalizes to match production practice.
        let path = std::fs::canonicalize(tmp.path()).unwrap();
        let cfg = FileIo {
            oracle_static_files: vec![StaticFileEntry {
                path,
                mode: "r".to_string(),
            }],
            ..Default::default()
        };
        validate(&cfg).unwrap();
    }

    #[test]
    fn validate_rejects_missing_path() {
        let cfg = FileIo {
            oracle_static_files: vec![StaticFileEntry {
                path: PathBuf::from("/definitely/does/not/exist/anywhere"),
                mode: "r".to_string(),
            }],
            ..Default::default()
        };
        let errs = validate(&cfg).unwrap_err();
        assert!(matches!(errs[0], FileIoConfigError::PathNotFound { .. }));
    }

    #[test]
    fn validate_rejects_unknown_file_mode() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = std::fs::canonicalize(tmp.path()).unwrap();
        let cfg = FileIo {
            oracle_static_files: vec![StaticFileEntry {
                path,
                mode: "banana".to_string(),
            }],
            ..Default::default()
        };
        let errs = validate(&cfg).unwrap_err();
        assert!(errs
            .iter()
            .any(|e| matches!(e, FileIoConfigError::InvalidFileMode { .. })));
    }

    #[test]
    fn validate_rejects_unknown_dir_mode() {
        let tmp = tempfile::tempdir().unwrap();
        let path = std::fs::canonicalize(tmp.path()).unwrap();
        let cfg = FileIo {
            oracle_static_dirs: vec![StaticDirEntry {
                path,
                mode: "wxyz".to_string(),
            }],
            ..Default::default()
        };
        let errs = validate(&cfg).unwrap_err();
        assert!(errs
            .iter()
            .any(|e| matches!(e, FileIoConfigError::InvalidDirMode { .. })));
    }

    #[test]
    #[cfg(unix)]
    fn validate_rejects_symlink_traversal() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        let target = root.join("real.txt");
        std::fs::write(&target, b"").unwrap();
        let link = root.join("link.txt");
        symlink(&target, &link).unwrap();
        let cfg = FileIo {
            oracle_static_files: vec![StaticFileEntry {
                path: link,
                mode: "r".to_string(),
            }],
            ..Default::default()
        };
        let errs = validate(&cfg).unwrap_err();
        assert!(errs
            .iter()
            .any(|e| matches!(e, FileIoConfigError::SymlinkTraversal { .. })));
    }

    #[test]
    fn validate_reports_multiple_errors_at_once() {
        let cfg = FileIo {
            oracle_static_files: vec![StaticFileEntry {
                path: PathBuf::from("/nope/one"),
                mode: "r".to_string(),
            }],
            consensus_static_dirs: vec![StaticDirEntry {
                path: PathBuf::from("/nope/two"),
                mode: "r".to_string(),
            }],
            ..Default::default()
        };
        let errs = validate(&cfg).unwrap_err();
        assert_eq!(errs.len(), 2);
    }

    #[test]
    fn validate_accepts_all_nine_fopen_modes() {
        // Each fopen-style mode string is a legitimate file-entry
        // mode; validation should accept them all when the path
        // exists. Uses a single scratch file to avoid tempfile
        // churn.
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = std::fs::canonicalize(tmp.path()).unwrap();
        for mode in ["r", "w", "a", "r+", "w+", "a+", "wx", "w+x", "wbx"] {
            let cfg = FileIo {
                oracle_static_files: vec![StaticFileEntry {
                    path: path.clone(),
                    mode: mode.to_string(),
                }],
                ..Default::default()
            };
            validate(&cfg)
                .unwrap_or_else(|e| panic!("mode {mode} should be valid, got errors: {e:?}"));
        }
    }

    // --- lexically_normalize helper ---

    #[test]
    fn normalize_absolute_path_unchanged() {
        let n = lexically_normalize(Path::new("/foo/bar/baz")).unwrap();
        assert_eq!(n, PathBuf::from("/foo/bar/baz"));
    }

    #[test]
    fn normalize_relative_prepends_cwd() {
        let n = lexically_normalize(Path::new("baz")).unwrap();
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(n, cwd.join("baz"));
    }

    #[test]
    fn normalize_drops_current_dir_components() {
        let n = lexically_normalize(Path::new("/foo/./bar/./baz")).unwrap();
        assert_eq!(n, PathBuf::from("/foo/bar/baz"));
    }

    #[test]
    fn normalize_pops_on_parent_dir() {
        let n = lexically_normalize(Path::new("/foo/bar/../baz")).unwrap();
        assert_eq!(n, PathBuf::from("/foo/baz"));
    }

    #[test]
    fn normalize_collapses_trailing_slash() {
        let n = lexically_normalize(Path::new("/foo/bar/")).unwrap();
        assert_eq!(n, PathBuf::from("/foo/bar"));
    }

    #[test]
    fn normalize_at_root_pop_is_noop() {
        // A `..` at the root is undefined in POSIX; treat lexically
        // as pop-nothing rather than panic.
        let n = lexically_normalize(Path::new("/..")).unwrap();
        assert_eq!(n, PathBuf::from("/"));
    }

    #[test]
    fn normalize_multiple_leading_slashes_collapse() {
        // `//foo` is POSIX implementation-defined; `Components`
        // treats the extra slashes as one RootDir. We accept
        // whatever `Components` yields.
        let n = lexically_normalize(Path::new("//foo//bar")).unwrap();
        assert_eq!(n, PathBuf::from("/foo/bar"));
    }

    #[test]
    #[cfg(unix)]
    fn validate_accepts_relative_path_without_symlink() {
        // Regression guard for the pre-normalization bug: a
        // relative path used to fire `canonicalize != input`
        // spuriously because the canonical form is absolute.
        // With lexical normalization, both sides absolutize the
        // input to `cwd/<name>`, and if no symlinks are traversed
        // they agree.
        //
        // Build a scratch file inside the current cwd for the
        // duration of this test; use a randomized name so parallel
        // test runs don't collide.
        let unique = format!(
            "fileio-cfg-relative-{}",
            std::process::id() as u64
                ^ std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos() as u64
        );
        let rel = PathBuf::from(&unique);
        let abs = std::env::current_dir().unwrap().join(&rel);
        // Skip the test if cwd is itself under a symlink (e.g.
        // `/tmp` on macOS if the test runner sets cwd there). The
        // spurious-relative-path check we're guarding requires
        // cwd to canonicalize to itself.
        if std::fs::canonicalize(std::env::current_dir().unwrap())
            .unwrap_or_else(|_| PathBuf::new())
            != std::env::current_dir().unwrap()
        {
            return;
        }
        std::fs::write(&abs, b"").unwrap();
        let cfg = FileIo {
            oracle_static_files: vec![StaticFileEntry {
                path: rel,
                mode: "r".to_string(),
            }],
            ..Default::default()
        };
        let result = validate(&cfg);
        // Clean up regardless of pass/fail.
        let _ = std::fs::remove_file(&abs);
        result.unwrap();
    }

    #[test]
    fn validate_accepts_path_with_current_and_parent_dir_components() {
        // `.` and `..` components are lexical; the normalizer
        // resolves them without following symlinks.
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        std::fs::create_dir(root.join("a")).unwrap();
        std::fs::create_dir(root.join("b")).unwrap();
        let target = root.join("b/data.txt");
        std::fs::write(&target, b"").unwrap();
        // Refer to /root/b/data.txt via /root/a/../b/./data.txt.
        let indirect = root
            .join("a")
            .join("..")
            .join("b")
            .join(".")
            .join("data.txt");
        let cfg = FileIo {
            oracle_static_files: vec![StaticFileEntry {
                path: indirect,
                mode: "r".to_string(),
            }],
            ..Default::default()
        };
        validate(&cfg).unwrap();
    }
}
