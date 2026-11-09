//! File I/O FIP boot-time validation.
//!
//! Top-level entry point: `validate_provisioning_boot`.  Given a
//! parsed `FileIoProvisioning`, runs the full invariant pipeline
//! (size cap, syntactic defense-in-depth, canonicity, cross-bucket
//! logical-name uniqueness (H-3), bucket disjointness (PB-M-16),
//! per-entry tree walk) and returns every violation batched into a
//! `Vec<FileIoConfigError>` (plan §370 posture: one boot pass
//! surfaces every problem).
//!
//! Remaining work:
//!
//!   * ops-7 — CLI-merge path (populates the three `Duplicate*`
//!     variants from `provisioning_merge`); re-uses
//!     `normalize_for_compare` + `sort_key` from here.
//!   * ops-8 — wire `validate_provisioning_boot` into the node
//!     boot pipeline between config parsing and genesis handoff.
//!
//! Invariants (all batched into `Vec<FileIoConfigError>` so one
//! boot-failure report surfaces every violation, per plan §370):
//!
//!   * **No symlinks** on any configured path — entry or
//!     ancestor.  A symlink could redirect FS access to a
//!     location the config does not disclose.
//!   * **No hard-linked regular files** (Unix `nlink() > 1`) —
//!     two "different" paths pointing at the same inode defeat
//!     PB-M-16 disjointness and per-cap mode enforcement.
//!   * **Canonical absolute paths** — on macOS `/tmp` is a
//!     symlink to `/private/tmp`, so a config declaring
//!     `/tmp/foo` is really talking about `/private/tmp/foo`.
//!   * **Bucket disjointness (PB-M-16)** — the same path cannot
//!     be both consensus-replicated and oracle-local; prefix
//!     overlap across buckets is also rejected.
//!   * **Logical-name uniqueness across buckets (H-3)** —
//!     `Fs.rho::bMap` is a single-namespace keyed-by-logical-name
//!     map; a duplicate key would produce a Rholang map where
//!     second-write silently discards one entry.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::file_io_provisioning::{
    reject_forbidden_chars, validate_absolute_path, validate_size_limits, FileIoProvisioning,
    StaticDirEntry, StaticFileEntry, CONFIG_DIR_MODES, CONFIG_FILE_MODES,
};

/// The four spec-canonical buckets, as `&'static str` labels so
/// errors can name the offending bucket without allocating.
pub(crate) const BUCKET_ORACLE_FILE: &str = "oracle-static-files";
pub(crate) const BUCKET_ORACLE_DIR: &str = "oracle-static-dirs";
pub(crate) const BUCKET_CONSENSUS_FILE: &str = "consensus-static-files";
pub(crate) const BUCKET_CONSENSUS_DIR: &str = "consensus-static-dirs";

/// One provisioning-config violation.  Every variant carries
/// enough context to point the operator at the offending config
/// line (bucket + logical name) plus the underlying host detail
/// (path, nlink, canonical resolution, etc.).
///
/// The 15 boot variants are emitted by `validate_provisioning_boot`;
/// the 3 `Duplicate*` variants are emitted by the CLI-merge layer
/// (ops-7).
#[derive(Debug, PartialEq, Eq)]
pub enum FileIoConfigError {
    /// The declared path itself is a symlink.
    IsSymlink {
        bucket: &'static str,
        logical_name: String,
        path: PathBuf,
    },

    /// An ancestor directory on the declared path is a symlink.
    /// `prefix` is the ancestor that's actually a symlink; `canonical`
    /// is where the OS resolves it to (`None` on a dangling symlink
    /// or permission denied on the target).  Points the operator
    /// at the fix ("supply this canonical path instead").
    AbsolutePrefixSymlink {
        bucket: &'static str,
        logical_name: String,
        path: PathBuf,
        prefix: PathBuf,
        canonical: Option<PathBuf>,
    },

    /// A regular file with `nlink() > 1` — the same inode is
    /// reachable via a different path, defeating disjointness.
    HardLinked {
        bucket: &'static str,
        logical_name: String,
        path: PathBuf,
        nlink: u64,
    },

    /// The declared path does not exist on the host filesystem.
    PathNotFound {
        bucket: &'static str,
        logical_name: String,
        path: PathBuf,
    },

    /// `symlink_metadata` or a descendant walk failed for a reason
    /// other than absence — permission denied, I/O error, etc.
    StatFailed {
        bucket: &'static str,
        logical_name: String,
        path: PathBuf,
        error: String,
    },

    /// Same absolute path appears in both an `oracle-static-*`
    /// bucket and a `consensus-static-*` bucket.  A single
    /// filesystem entity cannot be both consensus-replicated and
    /// oracle-local (PB-M-16).
    BucketOverlapSamePath {
        path: PathBuf,
        bucket_a: &'static str,
        logical_name_a: String,
        bucket_b: &'static str,
        logical_name_b: String,
    },

    /// A dir path in one bucket contains a file/dir path declared
    /// in the OTHER bucket (prefix overlap).
    BucketOverlapPrefix {
        outer_bucket: &'static str,
        outer_logical_name: String,
        outer_path: PathBuf,
        inner_bucket: &'static str,
        inner_logical_name: String,
        inner_path: PathBuf,
    },

    /// The declared entry kind (file/dir) doesn't match the on-disk
    /// entity.  Declaring a file that's actually a directory (or
    /// vice versa) is almost certainly an operator error and would
    /// misroute every subsequent openFile/openDir call.
    KindMismatch {
        bucket: &'static str,
        logical_name: String,
        path: PathBuf,
        declared: &'static str,
        actual: &'static str,
    },

    /// The declared path is not lexically canonical — contains
    /// `.` or `..` components.  Two different lexical spellings of
    /// the same on-disk entity would defeat PB-M-16 disjointness
    /// (`/srv/data/./cfg.json` and `/srv/data/cfg.json` compare
    /// unequal as `PathBuf` yet name the same file).
    NonCanonicalPath {
        bucket: &'static str,
        logical_name: String,
        path: PathBuf,
        reason: &'static str,
    },

    /// Boot-time defense-in-depth: the declared path failed
    /// `validate_absolute_path` (ops-6a's syntactic check).  Fires
    /// only if the caller constructed a `FileIoProvisioning`
    /// programmatically, bypassing serde-time validation.
    NotAbsolute {
        bucket: &'static str,
        logical_name: String,
        path: PathBuf,
    },

    /// Boot-time defense-in-depth: the declared mode failed the
    /// ops-3 whitelist.  Same programmatic-bypass rationale as
    /// `NotAbsolute`.
    InvalidMode {
        bucket: &'static str,
        logical_name: String,
        mode: String,
    },

    /// Logical name / path / mode contains a control character or
    /// invisible glyph (NUL, C0/C1 control, DEL, BOM, RTL override,
    /// LS/PS).  The CLI parser (ops-9) and HOCON deserializer
    /// (ops-6a) enforce at entry; this variant catches programmatic
    /// construction that bypasses both.
    ForbiddenChars {
        bucket: &'static str,
        logical_name: String,
        field: &'static str,
        detail: String,
    },

    /// The same logical name appears in more than one bucket (any
    /// pair — oracle × consensus, oracle-files × oracle-dirs, etc.).
    /// `Fs.rho::bMap` is a single keyed-by-logical-name namespace,
    /// so a duplicate key would produce a Rholang map where
    /// second-write wins, silently discarding one entry.
    LogicalNameConflictAcrossBuckets {
        logical_name: String,
        members: Vec<(&'static str, PathBuf)>,
    },

    /// Total provisioning-entry count exceeds
    /// `MAX_PROVISIONING_ENTRIES` (ops-4 cap).  Batched here so
    /// the rest of the report isn't discarded, but the tree walk
    /// short-circuits after this to avoid runaway cost.
    SizeLimitExceeded { message: String },

    /// The recursive walk of a provisioned directory exceeded the
    /// per-boot cap on visited descendants.  Emits once and stops;
    /// further descendants are silently skipped for this entry.
    WalkLimitExceeded {
        bucket: &'static str,
        logical_name: String,
        path: PathBuf,
        limit: usize,
    },

    /// ops-7 CLI merge: the same logical name appears in both the
    /// config-file surface and the CLI surface within the same
    /// bucket, with different `(path, mode)` definitions.
    /// Identical duplicates are silently deduped.
    DuplicateLogicalNameAcrossSources {
        bucket: &'static str,
        logical_name: String,
        config_path: PathBuf,
        config_mode: String,
        cli_path: PathBuf,
        cli_mode: String,
    },

    /// ops-7 CLI merge: the CLI surface contains multiple
    /// repetitions of the same `--*-static-*` flag with the same
    /// logical name but non-identical `(path, mode)` payloads.
    /// Identical repeated entries are silently deduped.
    DuplicateLogicalNameInCli {
        bucket: &'static str,
        logical_name: String,
        count: usize,
    },

    /// ops-7 CLI merge: the same absolute host path appears in both
    /// the config-file surface and the CLI surface within the same
    /// bucket, under DIFFERENT logical names.  Operator forgot
    /// they'd already provisioned this path.
    DuplicatePathAcrossSources {
        bucket: &'static str,
        path: PathBuf,
        config_logical_name: String,
        cli_logical_name: String,
    },
}

impl std::fmt::Display for FileIoConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IsSymlink {
                bucket,
                logical_name,
                path,
            } => write!(
                f,
                "[{bucket}] `{logical_name}` -> {path:?} is a symbolic link; \
                 static-provisioning targets must be direct paths (spec §Boot validation)"
            ),
            Self::AbsolutePrefixSymlink {
                bucket,
                logical_name,
                path,
                prefix,
                canonical,
            } => match canonical {
                Some(c) => write!(
                    f,
                    "[{bucket}] `{logical_name}` -> {path:?} traverses symlink at {prefix:?} \
                     (resolves to {c:?}); supply the canonical path in the config to avoid \
                     ambiguous redirection"
                ),
                None => write!(
                    f,
                    "[{bucket}] `{logical_name}` -> {path:?} traverses symlink at {prefix:?} \
                     (canonical resolution failed — dangling symlink or unreadable target); \
                     supply the canonical path in the config"
                ),
            },
            Self::HardLinked {
                bucket,
                logical_name,
                path,
                nlink,
            } => write!(
                f,
                "[{bucket}] `{logical_name}` -> {path:?} is a regular file with nlink={nlink} > 1; \
                 hard-linked inodes defeat per-cap mode enforcement (PB-M-16)"
            ),
            Self::PathNotFound {
                bucket,
                logical_name,
                path,
            } => write!(
                f,
                "[{bucket}] `{logical_name}` -> {path:?} does not exist on the host filesystem"
            ),
            Self::StatFailed {
                bucket,
                logical_name,
                path,
                error,
            } => write!(
                f,
                "[{bucket}] `{logical_name}` -> {path:?} could not be stat'd: {error}"
            ),
            Self::BucketOverlapSamePath {
                path,
                bucket_a,
                logical_name_a,
                bucket_b,
                logical_name_b,
            } => write!(
                f,
                "path {path:?} appears in both [{bucket_a}] as `{logical_name_a}` and \
                 [{bucket_b}] as `{logical_name_b}`; a filesystem entity cannot be both \
                 consensus-replicated and oracle-local (PB-M-16)"
            ),
            Self::BucketOverlapPrefix {
                outer_bucket,
                outer_logical_name,
                outer_path,
                inner_bucket,
                inner_logical_name,
                inner_path,
            } => write!(
                f,
                "[{outer_bucket}] `{outer_logical_name}` -> {outer_path:?} contains \
                 [{inner_bucket}] `{inner_logical_name}` -> {inner_path:?}; \
                 cross-bucket prefix overlap is forbidden (PB-M-16)"
            ),
            Self::KindMismatch {
                bucket,
                logical_name,
                path,
                declared,
                actual,
            } => write!(
                f,
                "[{bucket}] `{logical_name}` -> {path:?} was declared as a {declared} \
                 but the host filesystem has a {actual} at that path"
            ),
            Self::NonCanonicalPath {
                bucket,
                logical_name,
                path,
                reason,
            } => write!(
                f,
                "[{bucket}] `{logical_name}` -> {path:?} is not lexically canonical ({reason}); \
                 supply an absolute path with no `.` or `..` components (PB-M-16 disjointness \
                 check requires canonical spellings)"
            ),
            Self::NotAbsolute {
                bucket,
                logical_name,
                path,
            } => write!(
                f,
                "[{bucket}] `{logical_name}` -> {path:?} is not an absolute path \
                 (spec §1273 requires absolute paths)"
            ),
            Self::InvalidMode {
                bucket,
                logical_name,
                mode,
            } => write!(
                f,
                "[{bucket}] `{logical_name}` mode {mode:?} is not in the spec whitelist"
            ),
            Self::ForbiddenChars {
                bucket,
                logical_name,
                field,
                detail,
            } => write!(f, "[{bucket}] `{logical_name}` {field}: {detail}"),
            Self::LogicalNameConflictAcrossBuckets {
                logical_name,
                members,
            } => {
                let member_str: Vec<String> = members
                    .iter()
                    .map(|(b, p)| format!("[{b}] -> {p:?}"))
                    .collect();
                write!(
                    f,
                    "logical name `{logical_name}` appears in multiple buckets: {}; \
                     Fs.rho's bMap is a single-namespace map keyed by logical name, so \
                     a duplicate key would produce a Rholang map where second-write \
                     silently discards one entry (and pre-H-3 panicked in \
                     format_bundle_for_rholang at genesis composition)",
                    member_str.join(", ")
                )
            }
            Self::SizeLimitExceeded { message } => write!(f, "{message}"),
            Self::WalkLimitExceeded {
                bucket,
                logical_name,
                path,
                limit,
            } => write!(
                f,
                "[{bucket}] `{logical_name}` -> {path:?} tree walk exceeded per-boot cap \
                 of {limit} descendants; remaining descendants skipped"
            ),
            Self::DuplicateLogicalNameAcrossSources {
                bucket,
                logical_name,
                config_path,
                config_mode,
                cli_path,
                cli_mode,
            } => write!(
                f,
                "[{bucket}] `{logical_name}` is declared in BOTH config \
                 ({config_path:?}, mode={config_mode:?}) and CLI \
                 ({cli_path:?}, mode={cli_mode:?}) with different definitions; \
                 remove one source or reconcile the definitions"
            ),
            Self::DuplicateLogicalNameInCli {
                bucket,
                logical_name,
                count,
            } => write!(
                f,
                "[{bucket}] CLI declares `{logical_name}` {count} times with \
                 differing `(path, mode)` payloads; keep only one"
            ),
            Self::DuplicatePathAcrossSources {
                bucket,
                path,
                config_logical_name,
                cli_logical_name,
            } => write!(
                f,
                "[{bucket}] host path {path:?} appears in both config \
                 (as `{config_logical_name}`) and CLI (as `{cli_logical_name}`); \
                 the same host path may not be provisioned from both sources"
            ),
        }
    }
}

/// Kind of the declared entry — a file or a directory.  Determines
/// whether the tree walker (ops-6d) recurses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryKind {
    File,
    Dir,
}

/// A flattened view of one provisioning entry used by the
/// validators (ops-6d/6e).  Keeps borrowed `&str` / `&Path`
/// references to the owning `FileIoProvisioning` so the walk is
/// alloc-free beyond the output `Vec`.
#[derive(Debug)]
pub(crate) struct FlatEntry<'a> {
    pub bucket: &'static str,
    pub logical_name: &'a str,
    pub path: &'a Path,
    pub mode: &'a str,
    pub kind: EntryKind,
    pub is_consensus: bool,
}

/// Flatten the four provisioning maps into a single iterable view,
/// deterministically ordered by `(bucket, logical_name)`.
///
/// Rust's `HashMap` iteration order is randomized per build, so a
/// naive flatten would surface errors in a different order on
/// different runs — defeating `cargo test`'s deterministic
/// expectations and operator pattern-matching on error output.
/// Sorting here lets every downstream validator (ops-6d/6e) emit a
/// reproducible error `Vec`.
pub(crate) fn flatten<'a>(cfg: &'a FileIoProvisioning) -> Vec<FlatEntry<'a>> {
    let mut out: Vec<FlatEntry<'a>> = Vec::new();
    let extend_files = |bucket: &'static str,
                        is_consensus: bool,
                        map: &'a HashMap<String, StaticFileEntry>,
                        out: &mut Vec<FlatEntry<'a>>| {
        for (name, entry) in map {
            out.push(FlatEntry {
                bucket,
                logical_name: name.as_str(),
                path: entry.path.as_path(),
                mode: entry.mode.as_str(),
                kind: EntryKind::File,
                is_consensus,
            });
        }
    };
    let extend_dirs = |bucket: &'static str,
                       is_consensus: bool,
                       map: &'a HashMap<String, StaticDirEntry>,
                       out: &mut Vec<FlatEntry<'a>>| {
        for (name, entry) in map {
            out.push(FlatEntry {
                bucket,
                logical_name: name.as_str(),
                path: entry.path.as_path(),
                mode: entry.mode.as_str(),
                kind: EntryKind::Dir,
                is_consensus,
            });
        }
    };
    extend_files(
        BUCKET_ORACLE_FILE,
        false,
        &cfg.oracle_static_files,
        &mut out,
    );
    extend_dirs(BUCKET_ORACLE_DIR, false, &cfg.oracle_static_dirs, &mut out);
    extend_files(
        BUCKET_CONSENSUS_FILE,
        true,
        &cfg.consensus_static_files,
        &mut out,
    );
    extend_dirs(
        BUCKET_CONSENSUS_DIR,
        true,
        &cfg.consensus_static_dirs,
        &mut out,
    );
    out.sort_by(|a, b| {
        a.bucket
            .cmp(b.bucket)
            .then_with(|| a.logical_name.cmp(b.logical_name))
    });
    out
}

/// Reject `.` and `..` components.  Returns `Some(reason)` when
/// the path is non-canonical; `None` when clean.
///
/// Uses string-level inspection because Rust's `Path::components()`
/// silently drops `Component::CurDir` — a components-only check
/// would miss `/foo/./bar`.  `PathBuf` equality is byte-level, not
/// component-level, so `/foo/./bar` and `/foo/bar` compare unequal
/// despite naming the same file and would defeat the bucket-
/// disjointness check (ops-6e) without rejection here.  `..` DOES
/// round-trip as `Component::ParentDir`, but the same string check
/// is kept for symmetry and clearer operator-facing messages.
///
/// Trailing `/` and `//` are intentionally NOT rejected because
/// `Path` equality normalizes them (pinned by the trailing-slash
/// overlap test in ops-6e).
pub(crate) fn non_canonical_reason(p: &Path) -> Option<&'static str> {
    let s = p.to_string_lossy();
    let s = s.as_ref();
    if s == ".." || s.starts_with("../") || s.ends_with("/..") || s.contains("/../") {
        return Some("contains `..` component");
    }
    if s == "." || s.starts_with("./") || s.ends_with("/.") || s.contains("/./") {
        return Some("contains `.` component");
    }
    None
}

/// Deterministic sort key for `FileIoConfigError`.  Orders by
/// `(discriminant, bucket, logical_name, path)` so the batched
/// error report an operator sees is reproducible across
/// `HashMap`-randomized runs.
///
/// Discriminant choice groups related errors together in the
/// output: size cap (0) first, then syntactic path errors (1-3),
/// then bucket-disjointness (4-5), then host-filesystem errors
/// (6-12), then CLI-merge errors (13-15), then catch-alls (16-17).
///
/// Exposed at `pub(crate)` so ops-7's CLI-merge layer can sort its
/// combined error output before returning.
pub(crate) fn sort_key(err: &FileIoConfigError) -> (u8, String, String, String) {
    let empty = String::new();
    match err {
        FileIoConfigError::SizeLimitExceeded { message } => {
            (0, empty.clone(), empty.clone(), message.clone())
        }
        FileIoConfigError::NotAbsolute {
            bucket,
            logical_name,
            path,
        } => (
            1,
            bucket.to_string(),
            logical_name.clone(),
            path.to_string_lossy().into_owned(),
        ),
        FileIoConfigError::NonCanonicalPath {
            bucket,
            logical_name,
            path,
            ..
        } => (
            2,
            bucket.to_string(),
            logical_name.clone(),
            path.to_string_lossy().into_owned(),
        ),
        FileIoConfigError::InvalidMode {
            bucket,
            logical_name,
            ..
        } => (3, bucket.to_string(), logical_name.clone(), empty),
        FileIoConfigError::BucketOverlapSamePath {
            path,
            bucket_a,
            logical_name_a,
            ..
        } => (
            4,
            bucket_a.to_string(),
            logical_name_a.clone(),
            path.to_string_lossy().into_owned(),
        ),
        FileIoConfigError::BucketOverlapPrefix {
            outer_bucket,
            outer_logical_name,
            outer_path,
            ..
        } => (
            5,
            outer_bucket.to_string(),
            outer_logical_name.clone(),
            outer_path.to_string_lossy().into_owned(),
        ),
        FileIoConfigError::PathNotFound {
            bucket,
            logical_name,
            path,
        } => (
            6,
            bucket.to_string(),
            logical_name.clone(),
            path.to_string_lossy().into_owned(),
        ),
        FileIoConfigError::IsSymlink {
            bucket,
            logical_name,
            path,
        } => (
            7,
            bucket.to_string(),
            logical_name.clone(),
            path.to_string_lossy().into_owned(),
        ),
        FileIoConfigError::AbsolutePrefixSymlink {
            bucket,
            logical_name,
            path,
            ..
        } => (
            8,
            bucket.to_string(),
            logical_name.clone(),
            path.to_string_lossy().into_owned(),
        ),
        FileIoConfigError::HardLinked {
            bucket,
            logical_name,
            path,
            ..
        } => (
            9,
            bucket.to_string(),
            logical_name.clone(),
            path.to_string_lossy().into_owned(),
        ),
        FileIoConfigError::KindMismatch {
            bucket,
            logical_name,
            path,
            ..
        } => (
            10,
            bucket.to_string(),
            logical_name.clone(),
            path.to_string_lossy().into_owned(),
        ),
        FileIoConfigError::StatFailed {
            bucket,
            logical_name,
            path,
            ..
        } => (
            11,
            bucket.to_string(),
            logical_name.clone(),
            path.to_string_lossy().into_owned(),
        ),
        FileIoConfigError::WalkLimitExceeded {
            bucket,
            logical_name,
            path,
            ..
        } => (
            12,
            bucket.to_string(),
            logical_name.clone(),
            path.to_string_lossy().into_owned(),
        ),
        FileIoConfigError::DuplicateLogicalNameAcrossSources {
            bucket,
            logical_name,
            ..
        } => (13, bucket.to_string(), logical_name.clone(), empty),
        FileIoConfigError::DuplicateLogicalNameInCli {
            bucket,
            logical_name,
            ..
        } => (14, bucket.to_string(), logical_name.clone(), empty),
        FileIoConfigError::DuplicatePathAcrossSources { bucket, path, .. } => (
            15,
            bucket.to_string(),
            empty,
            path.to_string_lossy().into_owned(),
        ),
        FileIoConfigError::ForbiddenChars {
            bucket,
            logical_name,
            field,
            ..
        } => (
            16,
            bucket.to_string(),
            logical_name.clone(),
            field.to_string(),
        ),
        FileIoConfigError::LogicalNameConflictAcrossBuckets { logical_name, .. } => {
            (17, empty.clone(), logical_name.clone(), empty)
        }
    }
}

/// Iterate the flattened entry set, running the per-entry
/// validators (`check_absolute_prefix` + `check_entry_tree`) on
/// each.  Batches every error into the shared accumulator rather
/// than failing fast — the operator sees every violation on one
/// boot pass (plan §370).
pub(crate) fn check_entries(entries: &[FlatEntry<'_>], errors: &mut Vec<FileIoConfigError>) {
    for e in entries {
        check_absolute_prefix(e, errors);
        check_entry_tree(e, errors);
    }
}

/// Walk every ancestor of `entry.path` from root down to (but not
/// including) the entry itself; `symlink_metadata` each.  If any
/// ancestor is a symlink, emit `AbsolutePrefixSymlink` and stop.
/// The shallowest symlink wins because it usually captures the
/// root cause (e.g., `/tmp -> /private/tmp` on macOS).
///
/// Ancestor stat errors other than `NotFound` emit `StatFailed`;
/// a silent skip here could hide a symlink behind a permission-
/// denied ancestor.  `NotFound` stays silent because
/// `check_entry_tree` reports `PathNotFound` for the entry itself.
fn check_absolute_prefix(entry: &FlatEntry<'_>, errors: &mut Vec<FileIoConfigError>) {
    let mut ancestors: Vec<&Path> = entry.path.ancestors().skip(1).collect();
    ancestors.reverse();
    for anc in ancestors {
        if anc.as_os_str().is_empty() {
            continue;
        }
        match fs::symlink_metadata(anc) {
            Ok(md) if md.file_type().is_symlink() => {
                let canonical = fs::canonicalize(anc).ok();
                errors.push(FileIoConfigError::AbsolutePrefixSymlink {
                    bucket: entry.bucket,
                    logical_name: entry.logical_name.to_string(),
                    path: entry.path.to_path_buf(),
                    prefix: anc.to_path_buf(),
                    canonical,
                });
                return;
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => {
                errors.push(FileIoConfigError::StatFailed {
                    bucket: entry.bucket,
                    logical_name: entry.logical_name.to_string(),
                    path: anc.to_path_buf(),
                    error: format!("ancestor stat failed: {e}"),
                });
                return;
            }
        }
    }
}

/// Stat the entry itself.  Dispatch: symlink -> `IsSymlink`;
/// regular file -> `check_hardlink`; directory -> `walk_dir`;
/// kind-mismatch or other (device/socket/fifo) -> `KindMismatch`
/// with the on-disk kind in `actual`.
fn check_entry_tree(entry: &FlatEntry<'_>, errors: &mut Vec<FileIoConfigError>) {
    let md = match fs::symlink_metadata(entry.path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            errors.push(FileIoConfigError::PathNotFound {
                bucket: entry.bucket,
                logical_name: entry.logical_name.to_string(),
                path: entry.path.to_path_buf(),
            });
            return;
        }
        Err(e) => {
            errors.push(FileIoConfigError::StatFailed {
                bucket: entry.bucket,
                logical_name: entry.logical_name.to_string(),
                path: entry.path.to_path_buf(),
                error: e.to_string(),
            });
            return;
        }
    };
    if md.file_type().is_symlink() {
        errors.push(FileIoConfigError::IsSymlink {
            bucket: entry.bucket,
            logical_name: entry.logical_name.to_string(),
            path: entry.path.to_path_buf(),
        });
        return;
    }
    match (
        entry.kind,
        md.file_type().is_file(),
        md.file_type().is_dir(),
    ) {
        (EntryKind::File, true, _) => {
            check_hardlink(entry, &md, entry.path, errors);
        }
        (EntryKind::Dir, _, true) => {
            walk_dir(entry, entry.path, errors);
        }
        (EntryKind::File, false, true) => {
            errors.push(FileIoConfigError::KindMismatch {
                bucket: entry.bucket,
                logical_name: entry.logical_name.to_string(),
                path: entry.path.to_path_buf(),
                declared: "file",
                actual: "directory",
            });
        }
        (EntryKind::Dir, true, false) => {
            errors.push(FileIoConfigError::KindMismatch {
                bucket: entry.bucket,
                logical_name: entry.logical_name.to_string(),
                path: entry.path.to_path_buf(),
                declared: "directory",
                actual: "file",
            });
        }
        _ => {
            errors.push(FileIoConfigError::KindMismatch {
                bucket: entry.bucket,
                logical_name: entry.logical_name.to_string(),
                path: entry.path.to_path_buf(),
                declared: match entry.kind {
                    EntryKind::File => "file",
                    EntryKind::Dir => "directory",
                },
                actual: "other (device/socket/fifo)",
            });
        }
    }
}

/// Per-entry cap on how many descendants a single provisioned dir
/// may walk.  Beyond this, `WalkLimitExceeded` fires once and the
/// walk stops — a partial report is better than a boot-time hang
/// on a genuinely huge (or hostile) tree.
pub(crate) const MAX_WALK_DESCENDANTS: usize = 1_000_000;

/// Iterative subtree walker (converted from recursion to avoid
/// stack overflow on deep trees).  For every entry: symlink ->
/// `IsSymlink`; regular file -> `check_hardlink`; subdir -> push
/// onto work stack; other (FIFO / socket / device) ->
/// `KindMismatch` with `actual = "other (device/socket/fifo)"`.
fn walk_dir(entry: &FlatEntry<'_>, root: &Path, errors: &mut Vec<FileIoConfigError>) {
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    let mut visited: usize = 0;
    while let Some(dir) = stack.pop() {
        let iter = match fs::read_dir(&dir) {
            Ok(i) => i,
            Err(e) => {
                errors.push(FileIoConfigError::StatFailed {
                    bucket: entry.bucket,
                    logical_name: entry.logical_name.to_string(),
                    path: dir.clone(),
                    error: e.to_string(),
                });
                continue;
            }
        };
        for de in iter {
            visited += 1;
            if visited > MAX_WALK_DESCENDANTS {
                errors.push(FileIoConfigError::WalkLimitExceeded {
                    bucket: entry.bucket,
                    logical_name: entry.logical_name.to_string(),
                    path: root.to_path_buf(),
                    limit: MAX_WALK_DESCENDANTS,
                });
                return;
            }
            let de = match de {
                Ok(d) => d,
                Err(e) => {
                    errors.push(FileIoConfigError::StatFailed {
                        bucket: entry.bucket,
                        logical_name: entry.logical_name.to_string(),
                        path: dir.clone(),
                        error: e.to_string(),
                    });
                    continue;
                }
            };
            let child = de.path();
            let md = match fs::symlink_metadata(&child) {
                Ok(m) => m,
                Err(e) => {
                    errors.push(FileIoConfigError::StatFailed {
                        bucket: entry.bucket,
                        logical_name: entry.logical_name.to_string(),
                        path: child,
                        error: e.to_string(),
                    });
                    continue;
                }
            };
            let ft = md.file_type();
            if ft.is_symlink() {
                errors.push(FileIoConfigError::IsSymlink {
                    bucket: entry.bucket,
                    logical_name: entry.logical_name.to_string(),
                    path: child,
                });
            } else if ft.is_file() {
                check_hardlink(entry, &md, &child, errors);
            } else if ft.is_dir() {
                stack.push(child);
            } else {
                errors.push(FileIoConfigError::KindMismatch {
                    bucket: entry.bucket,
                    logical_name: entry.logical_name.to_string(),
                    path: child,
                    declared: "regular descendant of provisioned dir",
                    actual: "other (device/socket/fifo)",
                });
            }
        }
    }
}

/// `nlink() > 1` check.  Unix-only via `MetadataExt::nlink`; on
/// non-Unix hosts the check is a no-op (hard-link semantics aren't
/// consistent enough across Windows FSs to gate boot on).
#[cfg(unix)]
fn check_hardlink(
    entry: &FlatEntry<'_>,
    md: &fs::Metadata,
    path: &Path,
    errors: &mut Vec<FileIoConfigError>,
) {
    use std::os::unix::fs::MetadataExt;
    let nlink = md.nlink();
    if nlink > 1 {
        errors.push(FileIoConfigError::HardLinked {
            bucket: entry.bucket,
            logical_name: entry.logical_name.to_string(),
            path: path.to_path_buf(),
            nlink,
        });
    }
}

#[cfg(not(unix))]
fn check_hardlink(
    _entry: &FlatEntry<'_>,
    _md: &fs::Metadata,
    _path: &Path,
    _errors: &mut Vec<FileIoConfigError>,
) {
}

/// Lexical path normalization for comparison only.  Collects
/// components to drop `.` and collapse `//` / trailing `/`.
/// Preserves `..` components — `non_canonical_reason` catches
/// those separately.  Exposed at `pub(crate)` so ops-7's CLI-merge
/// layer can reuse it when deduplicating host-path collisions.
pub(crate) fn normalize_for_compare(p: &Path) -> PathBuf { p.components().collect() }

/// H-3 fix: Fs.rho's `bMap` is a single logical-name namespace
/// shared across all four buckets.  Any name appearing in `> 1`
/// entry would produce a Rholang map where second-write silently
/// wins — pre-H-3 this surfaced as a panic in
/// `format_bundle_for_rholang` on EVERY validator, a network-wide
/// genesis DoS.  Reject at boot with one error per colliding name
/// listing every (bucket, path) pair for operator diagnosis.
fn check_cross_source_logical_name_conflict(
    entries: &[FlatEntry<'_>],
    errors: &mut Vec<FileIoConfigError>,
) {
    let mut by_name: HashMap<&str, Vec<&FlatEntry<'_>>> = HashMap::new();
    for e in entries {
        by_name.entry(e.logical_name).or_default().push(e);
    }
    let mut names: Vec<&&str> = by_name
        .iter()
        .filter(|(_, vs)| vs.len() > 1)
        .map(|(n, _)| n)
        .collect();
    names.sort();
    for name in names {
        let mut vs = by_name[*name].clone();
        vs.sort_by_key(|e| e.bucket);
        errors.push(FileIoConfigError::LogicalNameConflictAcrossBuckets {
            logical_name: (*name).to_string(),
            members: vs
                .iter()
                .map(|e| (e.bucket, e.path.to_path_buf()))
                .collect(),
        });
    }
}

/// PB-M-16 cross-bucket disjointness.  Detects same-path duplicates
/// and prefix overlaps between oracle and consensus buckets.
/// Same-bucket overlaps are legal (a user may legitimately declare
/// a directory and a file inside it in the same bucket).
///
/// Same-path comparison uses `normalize_for_compare` so
/// `/etc/foo` and `/etc/./foo` are recognized as SamePath (raw
/// `PathBuf` equality is byte-level, so the un-normalized check
/// would classify the pair as Prefix).  The emitted error carries
/// the operator's original path spelling so the fix is locatable.
fn check_bucket_disjointness(entries: &[FlatEntry<'_>], errors: &mut Vec<FileIoConfigError>) {
    let mut oracle: Vec<&FlatEntry<'_>> = Vec::new();
    let mut consensus: Vec<&FlatEntry<'_>> = Vec::new();
    for e in entries {
        if e.is_consensus {
            consensus.push(e);
        } else {
            oracle.push(e);
        }
    }
    for o in &oracle {
        let o_norm = normalize_for_compare(o.path);
        for c in &consensus {
            let c_norm = normalize_for_compare(c.path);
            if o_norm == c_norm {
                errors.push(FileIoConfigError::BucketOverlapSamePath {
                    path: o.path.to_path_buf(),
                    bucket_a: o.bucket,
                    logical_name_a: o.logical_name.to_string(),
                    bucket_b: c.bucket,
                    logical_name_b: c.logical_name.to_string(),
                });
                continue;
            }
            if let Some((outer, inner)) = prefix_pair(o, c) {
                errors.push(FileIoConfigError::BucketOverlapPrefix {
                    outer_bucket: outer.bucket,
                    outer_logical_name: outer.logical_name.to_string(),
                    outer_path: outer.path.to_path_buf(),
                    inner_bucket: inner.bucket,
                    inner_logical_name: inner.logical_name.to_string(),
                    inner_path: inner.path.to_path_buf(),
                });
            }
        }
    }
}

/// Return `Some((outer, inner))` if one entry's path is a strict
/// path-component prefix of the other's, else `None`.  A path is
/// a prefix of another only when the other is inside its directory
/// tree — string-prefix isn't enough (`/foo` is not a component
/// prefix of `/foobar`).  `Path::starts_with` does component-level
/// matching.
fn prefix_pair<'a>(
    a: &'a FlatEntry<'a>,
    b: &'a FlatEntry<'a>,
) -> Option<(&'a FlatEntry<'a>, &'a FlatEntry<'a>)> {
    if b.path.starts_with(a.path) && b.path != a.path {
        Some((a, b))
    } else if a.path.starts_with(b.path) && a.path != b.path {
        Some((b, a))
    } else {
        None
    }
}

/// Boot-time validation entry point.  Runs the full validation
/// pipeline against a parsed `FileIoProvisioning`; batches every
/// violation into a single `Vec<FileIoConfigError>` (plan §370);
/// returns `Ok(())` iff no violations.
///
/// Pipeline:
///
///   1. Size cap: `validate_size_limits`.  On breach, surface the
///      error but still run syntactic checks below.  Skip the tree
///      walk + bucket disjointness to bound cost (a hostile 10M-
///      entry config shouldn't ever hit the real filesystem).
///   2. Defense-in-depth syntactic re-checks: absolute path,
///      canonical path, mode whitelist, forbidden chars (on
///      logical name, path, mode).  Catches programmatic
///      construction that bypassed ops-6a's serde-time checks
///      (test harnesses, ops-7's CLI merge, etc.).  Entries that
///      fail any syntactic check are FLAGGED but then EXCLUDED
///      from downstream disjointness + tree walk to avoid cascade
///      errors.
///   3. Cross-bucket logical-name conflict (H-3).
///   4. Bucket disjointness (PB-M-16).
///   5. Per-entry tree walk (ancestor symlinks, entry symlink,
///      hard links, kind mismatch, hostile-tree descendant cap).
///   6. Deterministic sort via `sort_key` (M-23-1).
pub fn validate_provisioning_boot(cfg: &FileIoProvisioning) -> Result<(), Vec<FileIoConfigError>> {
    let mut errors: Vec<FileIoConfigError> = Vec::new();

    let size_ok = match validate_size_limits(cfg) {
        Ok(()) => true,
        Err(message) => {
            errors.push(FileIoConfigError::SizeLimitExceeded { message });
            false
        }
    };

    let all_entries = flatten(cfg);
    let mut valid_entries: Vec<FlatEntry<'_>> = Vec::with_capacity(all_entries.len());
    for e in all_entries {
        let mut skip = false;
        if validate_absolute_path(e.path).is_err() {
            errors.push(FileIoConfigError::NotAbsolute {
                bucket: e.bucket,
                logical_name: e.logical_name.to_string(),
                path: e.path.to_path_buf(),
            });
            skip = true;
        }
        if let Some(reason) = non_canonical_reason(e.path) {
            errors.push(FileIoConfigError::NonCanonicalPath {
                bucket: e.bucket,
                logical_name: e.logical_name.to_string(),
                path: e.path.to_path_buf(),
                reason,
            });
            skip = true;
        }
        let whitelist: &[&str] = match e.kind {
            EntryKind::File => CONFIG_FILE_MODES,
            EntryKind::Dir => CONFIG_DIR_MODES,
        };
        if !whitelist.contains(&e.mode) {
            errors.push(FileIoConfigError::InvalidMode {
                bucket: e.bucket,
                logical_name: e.logical_name.to_string(),
                mode: e.mode.to_string(),
            });
            skip = true;
        }
        if let Err(detail) = reject_forbidden_chars("logical_name", e.logical_name) {
            errors.push(FileIoConfigError::ForbiddenChars {
                bucket: e.bucket,
                logical_name: e.logical_name.to_string(),
                field: "logical_name",
                detail,
            });
            skip = true;
        }
        if let Some(s) = e.path.to_str() {
            if let Err(detail) = reject_forbidden_chars("path", s) {
                errors.push(FileIoConfigError::ForbiddenChars {
                    bucket: e.bucket,
                    logical_name: e.logical_name.to_string(),
                    field: "path",
                    detail,
                });
                skip = true;
            }
        }
        if let Err(detail) = reject_forbidden_chars("mode", e.mode) {
            errors.push(FileIoConfigError::ForbiddenChars {
                bucket: e.bucket,
                logical_name: e.logical_name.to_string(),
                field: "mode",
                detail,
            });
            skip = true;
        }
        if !skip {
            valid_entries.push(e);
        }
    }

    check_cross_source_logical_name_conflict(&valid_entries, &mut errors);

    if size_ok {
        check_bucket_disjointness(&valid_entries, &mut errors);
        check_entries(&valid_entries, &mut errors);
    }

    errors.sort_by_key(sort_key);

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every variant's `Display` output includes the bucket label,
    /// logical name (when the variant has one), and at least one
    /// substring that identifies the invariant.  Operator reads
    /// this on boot-failure; dropping any of these would make the
    /// error unlocatable in a 100-entry provisioning config.
    #[test]
    fn every_variant_display_includes_expected_substrings() {
        let cases: Vec<(FileIoConfigError, Vec<&str>)> = vec![
            (
                FileIoConfigError::IsSymlink {
                    bucket: BUCKET_ORACLE_FILE,
                    logical_name: "cfg".into(),
                    path: PathBuf::from("/etc/cfg"),
                },
                vec!["oracle-static-files", "cfg", "symbolic link"],
            ),
            (
                FileIoConfigError::AbsolutePrefixSymlink {
                    bucket: BUCKET_ORACLE_DIR,
                    logical_name: "share".into(),
                    path: PathBuf::from("/tmp/share"),
                    prefix: PathBuf::from("/tmp"),
                    canonical: Some(PathBuf::from("/private/tmp")),
                },
                vec!["oracle-static-dirs", "share", "symlink at", "resolves to"],
            ),
            (
                FileIoConfigError::AbsolutePrefixSymlink {
                    bucket: BUCKET_ORACLE_DIR,
                    logical_name: "share".into(),
                    path: PathBuf::from("/x/share"),
                    prefix: PathBuf::from("/x"),
                    canonical: None,
                },
                vec!["canonical resolution failed"],
            ),
            (
                FileIoConfigError::HardLinked {
                    bucket: BUCKET_CONSENSUS_FILE,
                    logical_name: "n".into(),
                    path: PathBuf::from("/p"),
                    nlink: 2,
                },
                vec!["consensus-static-files", "nlink=2", "PB-M-16"],
            ),
            (
                FileIoConfigError::PathNotFound {
                    bucket: BUCKET_ORACLE_FILE,
                    logical_name: "missing".into(),
                    path: PathBuf::from("/no/such"),
                },
                vec!["missing", "does not exist"],
            ),
            (
                FileIoConfigError::StatFailed {
                    bucket: BUCKET_ORACLE_DIR,
                    logical_name: "d".into(),
                    path: PathBuf::from("/p"),
                    error: "permission denied".into(),
                },
                vec!["could not be stat'd", "permission denied"],
            ),
            (
                FileIoConfigError::BucketOverlapSamePath {
                    path: PathBuf::from("/p"),
                    bucket_a: BUCKET_ORACLE_FILE,
                    logical_name_a: "a".into(),
                    bucket_b: BUCKET_CONSENSUS_FILE,
                    logical_name_b: "b".into(),
                },
                vec!["oracle-static-files", "consensus-static-files", "PB-M-16"],
            ),
            (
                FileIoConfigError::BucketOverlapPrefix {
                    outer_bucket: BUCKET_ORACLE_DIR,
                    outer_logical_name: "outer".into(),
                    outer_path: PathBuf::from("/data"),
                    inner_bucket: BUCKET_CONSENSUS_FILE,
                    inner_logical_name: "inner".into(),
                    inner_path: PathBuf::from("/data/cfg"),
                },
                vec!["contains", "prefix overlap", "PB-M-16"],
            ),
            (
                FileIoConfigError::KindMismatch {
                    bucket: BUCKET_ORACLE_FILE,
                    logical_name: "n".into(),
                    path: PathBuf::from("/p"),
                    declared: "file",
                    actual: "directory",
                },
                vec!["declared as a file", "has a directory"],
            ),
            (
                FileIoConfigError::NonCanonicalPath {
                    bucket: BUCKET_ORACLE_FILE,
                    logical_name: "n".into(),
                    path: PathBuf::from("/a/./b"),
                    reason: "contains `.` component",
                },
                vec!["not lexically canonical", "contains `.` component"],
            ),
            (
                FileIoConfigError::NotAbsolute {
                    bucket: BUCKET_ORACLE_FILE,
                    logical_name: "n".into(),
                    path: PathBuf::from("relative"),
                },
                vec!["is not an absolute path", "spec §1273"],
            ),
            (
                FileIoConfigError::InvalidMode {
                    bucket: BUCKET_ORACLE_FILE,
                    logical_name: "n".into(),
                    mode: "rwx".into(),
                },
                vec!["mode", "rwx", "not in the spec whitelist"],
            ),
            (
                FileIoConfigError::ForbiddenChars {
                    bucket: BUCKET_ORACLE_FILE,
                    logical_name: "n".into(),
                    field: "path",
                    detail: "U+0000 at byte 3".into(),
                },
                vec!["path", "U+0000 at byte 3"],
            ),
            (
                FileIoConfigError::LogicalNameConflictAcrossBuckets {
                    logical_name: "shared".into(),
                    members: vec![
                        (BUCKET_ORACLE_FILE, PathBuf::from("/a")),
                        (BUCKET_ORACLE_DIR, PathBuf::from("/b")),
                    ],
                },
                vec![
                    "shared",
                    "appears in multiple buckets",
                    "oracle-static-files",
                    "oracle-static-dirs",
                ],
            ),
            (
                FileIoConfigError::SizeLimitExceeded {
                    message: "exceeded cap of 100000 entries".into(),
                },
                vec!["exceeded cap of 100000 entries"],
            ),
            (
                FileIoConfigError::WalkLimitExceeded {
                    bucket: BUCKET_ORACLE_DIR,
                    logical_name: "d".into(),
                    path: PathBuf::from("/data"),
                    limit: 1_000_000,
                },
                vec!["tree walk exceeded", "1000000 descendants"],
            ),
            (
                FileIoConfigError::DuplicateLogicalNameAcrossSources {
                    bucket: BUCKET_ORACLE_FILE,
                    logical_name: "n".into(),
                    config_path: PathBuf::from("/a"),
                    config_mode: "r".into(),
                    cli_path: PathBuf::from("/b"),
                    cli_mode: "r+".into(),
                },
                vec!["declared in BOTH config", "and CLI"],
            ),
            (
                FileIoConfigError::DuplicateLogicalNameInCli {
                    bucket: BUCKET_ORACLE_FILE,
                    logical_name: "n".into(),
                    count: 2,
                },
                vec!["CLI declares", "2 times"],
            ),
            (
                FileIoConfigError::DuplicatePathAcrossSources {
                    bucket: BUCKET_ORACLE_FILE,
                    path: PathBuf::from("/p"),
                    config_logical_name: "a".into(),
                    cli_logical_name: "b".into(),
                },
                vec!["appears in both config", "and CLI"],
            ),
        ];

        for (err, substrings) in cases {
            let s = err.to_string();
            for sub in &substrings {
                assert!(
                    s.contains(sub),
                    "Display of {err:?} missing substring {sub:?}; got {s:?}"
                );
            }
        }
    }

    /// Bucket constants match the spec-canonical HOCON keys exactly.
    /// Any divergence would break operator-facing error messages
    /// (the error says `[oracle-static-files]` and the config has
    /// a matching key).
    #[test]
    fn bucket_constants_match_hocon_keys() {
        assert_eq!(BUCKET_ORACLE_FILE, "oracle-static-files");
        assert_eq!(BUCKET_ORACLE_DIR, "oracle-static-dirs");
        assert_eq!(BUCKET_CONSENSUS_FILE, "consensus-static-files");
        assert_eq!(BUCKET_CONSENSUS_DIR, "consensus-static-dirs");
    }

    // ---------- flatten / FlatEntry ----------

    fn empty_cfg() -> FileIoProvisioning {
        FileIoProvisioning {
            oracle_static_files: HashMap::new(),
            oracle_static_dirs: HashMap::new(),
            consensus_static_files: HashMap::new(),
            consensus_static_dirs: HashMap::new(),
        }
    }

    /// `flatten` emits one `FlatEntry` per entry across all four
    /// buckets, with `kind` and `is_consensus` set correctly per
    /// bucket.  Pins the mapping bucket -> (kind, is_consensus):
    /// oracle-files = (File, false), oracle-dirs = (Dir, false),
    /// consensus-files = (File, true), consensus-dirs = (Dir, true).
    #[test]
    fn flatten_covers_all_four_buckets() {
        let mut cfg = empty_cfg();
        cfg.oracle_static_files
            .insert("of".into(), StaticFileEntry {
                path: PathBuf::from("/x/of"),
                mode: "r".into(),
            });
        cfg.oracle_static_dirs.insert("od".into(), StaticDirEntry {
            path: PathBuf::from("/x/od"),
            mode: "r".into(),
        });
        cfg.consensus_static_files
            .insert("cf".into(), StaticFileEntry {
                path: PathBuf::from("/x/cf"),
                mode: "r".into(),
            });
        cfg.consensus_static_dirs
            .insert("cd".into(), StaticDirEntry {
                path: PathBuf::from("/x/cd"),
                mode: "r".into(),
            });
        let out = flatten(&cfg);
        assert_eq!(out.len(), 4);
        let by_bucket: HashMap<&str, &FlatEntry<'_>> = out.iter().map(|e| (e.bucket, e)).collect();
        assert!(matches!(
            by_bucket[BUCKET_ORACLE_FILE].kind,
            EntryKind::File
        ));
        assert!(!by_bucket[BUCKET_ORACLE_FILE].is_consensus);
        assert!(matches!(by_bucket[BUCKET_ORACLE_DIR].kind, EntryKind::Dir));
        assert!(!by_bucket[BUCKET_ORACLE_DIR].is_consensus);
        assert!(matches!(
            by_bucket[BUCKET_CONSENSUS_FILE].kind,
            EntryKind::File
        ));
        assert!(by_bucket[BUCKET_CONSENSUS_FILE].is_consensus);
        assert!(matches!(
            by_bucket[BUCKET_CONSENSUS_DIR].kind,
            EntryKind::Dir
        ));
        assert!(by_bucket[BUCKET_CONSENSUS_DIR].is_consensus);
    }

    /// `flatten` sorts by `(bucket, logical_name)`.  `HashMap`
    /// iteration order is randomized per build; without the sort,
    /// downstream validators would emit errors in a different
    /// order on different runs.  Pins the ordering invariant that
    /// every downstream validator (ops-6d/6e) depends on.
    #[test]
    fn flatten_output_is_deterministically_ordered() {
        let mut cfg = empty_cfg();
        for name in ["z", "a", "m", "b"] {
            cfg.oracle_static_files
                .insert(name.into(), StaticFileEntry {
                    path: PathBuf::from(format!("/x/{name}")),
                    mode: "r".into(),
                });
        }
        let out1 = flatten(&cfg);
        let out2 = flatten(&cfg);
        let names1: Vec<&str> = out1.iter().map(|e| e.logical_name).collect();
        let names2: Vec<&str> = out2.iter().map(|e| e.logical_name).collect();
        assert_eq!(names1, names2, "flatten must be deterministic across calls");
        assert_eq!(
            names1,
            vec!["a", "b", "m", "z"],
            "flatten must sort by logical_name within a bucket"
        );
    }

    // ---------- non_canonical_reason ----------

    /// `non_canonical_reason` rejects `.` and `..` components but
    /// accepts canonical absolute paths, roots, and
    /// `PathBuf`-normalized spellings (trailing slash, double slash).
    #[test]
    fn non_canonical_reason_pin_behavior() {
        assert_eq!(non_canonical_reason(Path::new("/foo/bar")), None);
        assert_eq!(non_canonical_reason(Path::new("/")), None);
        assert!(non_canonical_reason(Path::new("/foo/./bar")).is_some());
        assert!(non_canonical_reason(Path::new("/foo/../bar")).is_some());
        assert!(non_canonical_reason(Path::new("./bar")).is_some());
        assert!(non_canonical_reason(Path::new("../bar")).is_some());
        assert!(non_canonical_reason(Path::new("/foo/.")).is_some());
        assert!(non_canonical_reason(Path::new("/foo/..")).is_some());
        // Trailing / and // normalize via Path::components — accepted.
        assert_eq!(non_canonical_reason(Path::new("/foo/bar/")), None);
        assert_eq!(non_canonical_reason(Path::new("/foo//bar")), None);
    }

    /// The reason string names which component triggered rejection
    /// so the operator-facing error message can point at the fix.
    #[test]
    fn non_canonical_reason_names_the_offending_component() {
        assert!(non_canonical_reason(Path::new("/a/./b"))
            .expect("`.` must reject")
            .contains("`.` component"));
        assert!(non_canonical_reason(Path::new("/a/../b"))
            .expect("`..` must reject")
            .contains("`..` component"));
    }

    // ---------- sort_key ----------

    /// `sort_key` groups error variants by category via its first
    /// tuple element (the discriminant).  Pins the groups' relative
    /// order so operator-facing batched reports stay stable:
    /// size cap first, then path-syntactic errors, then bucket-
    /// disjointness, then host-filesystem errors, then CLI-merge,
    /// then catch-alls.
    #[test]
    fn sort_key_discriminant_order_is_pinned() {
        let size = FileIoConfigError::SizeLimitExceeded {
            message: "overflow".into(),
        };
        let not_abs = FileIoConfigError::NotAbsolute {
            bucket: BUCKET_ORACLE_FILE,
            logical_name: "n".into(),
            path: PathBuf::from("rel"),
        };
        let overlap = FileIoConfigError::BucketOverlapSamePath {
            path: PathBuf::from("/p"),
            bucket_a: BUCKET_ORACLE_FILE,
            logical_name_a: "a".into(),
            bucket_b: BUCKET_CONSENSUS_FILE,
            logical_name_b: "b".into(),
        };
        let not_found = FileIoConfigError::PathNotFound {
            bucket: BUCKET_ORACLE_FILE,
            logical_name: "n".into(),
            path: PathBuf::from("/no/such"),
        };
        let dup_cli = FileIoConfigError::DuplicateLogicalNameInCli {
            bucket: BUCKET_ORACLE_FILE,
            logical_name: "n".into(),
            count: 2,
        };
        let name_conflict = FileIoConfigError::LogicalNameConflictAcrossBuckets {
            logical_name: "shared".into(),
            members: vec![],
        };

        let disc = |e: &FileIoConfigError| sort_key(e).0;
        assert_eq!(disc(&size), 0);
        assert_eq!(disc(&not_abs), 1);
        assert_eq!(disc(&overlap), 4);
        assert_eq!(disc(&not_found), 6);
        assert_eq!(disc(&dup_cli), 14);
        assert_eq!(disc(&name_conflict), 17);
    }

    /// Within a discriminant class, `sort_key` orders by
    /// `(bucket, logical_name, path)`.  Pins deterministic
    /// intra-class ordering so operators see errors for the same
    /// bucket adjacent in the report.
    #[test]
    fn sort_key_within_class_orders_by_bucket_then_name() {
        let a = FileIoConfigError::PathNotFound {
            bucket: BUCKET_ORACLE_FILE,
            logical_name: "b".into(),
            path: PathBuf::from("/p/b"),
        };
        let b = FileIoConfigError::PathNotFound {
            bucket: BUCKET_ORACLE_FILE,
            logical_name: "a".into(),
            path: PathBuf::from("/p/a"),
        };
        let c = FileIoConfigError::PathNotFound {
            bucket: BUCKET_CONSENSUS_FILE,
            logical_name: "a".into(),
            path: PathBuf::from("/p/a"),
        };
        let mut errs = [&a, &b, &c];
        errs.sort_by_key(|e| sort_key(e));
        // consensus-static-files sorts first alphabetically; within
        // oracle-static-files, logical_name "a" sorts before "b".
        assert_eq!(errs[0].to_string(), c.to_string());
        assert_eq!(errs[1].to_string(), b.to_string());
        assert_eq!(errs[2].to_string(), a.to_string());
    }

    // ---------- Per-entry validators (ops-6d) ----------

    use tempfile::TempDir;

    /// `TempDir::path()` on macOS lives under `/var/folders/...`,
    /// but `/var` is a symlink to `/private/var` — so uncanonicalized
    /// paths would trip the (correct) absolute-prefix-symlink
    /// diagnostic in every test.  Real config files supply canonical
    /// paths per that diagnostic's exact recommendation.
    fn td_root(td: &TempDir) -> PathBuf { fs::canonicalize(td.path()).unwrap() }

    fn make_file(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, b"x").unwrap();
    }

    fn run_entry_check(
        bucket: &'static str,
        kind: EntryKind,
        path: &Path,
    ) -> Vec<FileIoConfigError> {
        let entry = FlatEntry {
            bucket,
            logical_name: "n",
            path,
            mode: "r",
            kind,
            is_consensus: false,
        };
        let mut errors = Vec::new();
        check_absolute_prefix(&entry, &mut errors);
        check_entry_tree(&entry, &mut errors);
        errors
    }

    /// Happy path: a regular file at a canonical absolute path
    /// produces no errors.
    #[test]
    fn regular_file_passes_entry_check() {
        let td = TempDir::new().unwrap();
        let path = td_root(&td).join("cfg.json");
        make_file(&path);
        let errs = run_entry_check(BUCKET_ORACLE_FILE, EntryKind::File, &path);
        assert!(errs.is_empty(), "expected no errors; got {errs:?}");
    }

    /// Happy path: a regular directory at a canonical absolute path
    /// produces no errors.
    #[test]
    fn regular_dir_passes_entry_check() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let path = root.join("share");
        fs::create_dir_all(&path).unwrap();
        make_file(&path.join("child.json"));
        let errs = run_entry_check(BUCKET_ORACLE_DIR, EntryKind::Dir, &path);
        assert!(errs.is_empty(), "expected no errors; got {errs:?}");
    }

    /// A missing path produces `PathNotFound`.
    #[test]
    fn missing_path_rejected() {
        let td = TempDir::new().unwrap();
        let path = td_root(&td).join("no_such_file");
        let errs = run_entry_check(BUCKET_ORACLE_FILE, EntryKind::File, &path);
        assert!(
            errs.iter()
                .any(|e| matches!(e, FileIoConfigError::PathNotFound { .. })),
            "expected PathNotFound; got {errs:?}"
        );
    }

    /// Declared file but on-disk directory -> `KindMismatch`.
    #[test]
    fn kind_mismatch_file_declared_but_dir_on_disk() {
        let td = TempDir::new().unwrap();
        let path = td_root(&td).join("ambiguous");
        fs::create_dir_all(&path).unwrap();
        let errs = run_entry_check(BUCKET_ORACLE_FILE, EntryKind::File, &path);
        assert!(
            errs.iter().any(|e| matches!(
                e,
                FileIoConfigError::KindMismatch { declared, actual, .. }
                    if *declared == "file" && *actual == "directory"
            )),
            "expected KindMismatch declared=file actual=directory; got {errs:?}"
        );
    }

    /// Declared directory but on-disk file -> `KindMismatch`.
    #[test]
    fn kind_mismatch_dir_declared_but_file_on_disk() {
        let td = TempDir::new().unwrap();
        let path = td_root(&td).join("regular.txt");
        make_file(&path);
        let errs = run_entry_check(BUCKET_ORACLE_DIR, EntryKind::Dir, &path);
        assert!(
            errs.iter().any(|e| matches!(
                e,
                FileIoConfigError::KindMismatch { declared, actual, .. }
                    if *declared == "directory" && *actual == "file"
            )),
            "expected KindMismatch declared=directory actual=file; got {errs:?}"
        );
    }

    /// Entry is a symlink -> `IsSymlink`.
    #[cfg(unix)]
    #[test]
    fn entry_as_symlink_rejected() {
        use std::os::unix::fs::symlink;
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let target = root.join("target.json");
        make_file(&target);
        let link = root.join("link.json");
        symlink(&target, &link).unwrap();
        let errs = run_entry_check(BUCKET_ORACLE_FILE, EntryKind::File, &link);
        assert!(
            errs.iter()
                .any(|e| matches!(e, FileIoConfigError::IsSymlink { .. })),
            "expected IsSymlink; got {errs:?}"
        );
    }

    /// Ancestor is a symlink -> `AbsolutePrefixSymlink` naming the
    /// ancestor and its canonical resolution.
    #[cfg(unix)]
    #[test]
    fn ancestor_symlink_rejected_with_canonical() {
        use std::os::unix::fs::symlink;
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let real = root.join("real");
        fs::create_dir_all(&real).unwrap();
        let link = root.join("alias");
        symlink(&real, &link).unwrap();
        let file = link.join("cfg.json");
        make_file(&real.join("cfg.json"));
        let errs = run_entry_check(BUCKET_ORACLE_FILE, EntryKind::File, &file);
        assert!(
            errs.iter().any(|e| matches!(
                e,
                FileIoConfigError::AbsolutePrefixSymlink { prefix, canonical: Some(c), .. }
                    if prefix == &link && c == &real
            )),
            "expected AbsolutePrefixSymlink with canonical resolution; got {errs:?}"
        );
    }

    /// A dangling ancestor symlink yields `AbsolutePrefixSymlink`
    /// with `canonical: None`.
    #[cfg(unix)]
    #[test]
    fn ancestor_symlink_dangling_reports_no_canonical() {
        use std::os::unix::fs::symlink;
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let link = root.join("alias");
        symlink(root.join("does_not_exist"), &link).unwrap();
        let file = link.join("cfg.json");
        let errs = run_entry_check(BUCKET_ORACLE_FILE, EntryKind::File, &file);
        assert!(
            errs.iter()
                .any(|e| matches!(e, FileIoConfigError::AbsolutePrefixSymlink {
                    canonical: None,
                    ..
                })),
            "expected AbsolutePrefixSymlink with canonical=None; got {errs:?}"
        );
    }

    /// A hard-linked regular file (`nlink > 1`) yields `HardLinked`.
    #[cfg(unix)]
    #[test]
    fn hardlinked_file_rejected() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let a = root.join("a.json");
        let b = root.join("b.json");
        make_file(&a);
        fs::hard_link(&a, &b).unwrap();
        let errs = run_entry_check(BUCKET_ORACLE_FILE, EntryKind::File, &a);
        assert!(
            errs.iter().any(|e| matches!(
                e,
                FileIoConfigError::HardLinked { nlink, .. } if *nlink >= 2
            )),
            "expected HardLinked with nlink >= 2; got {errs:?}"
        );
    }

    /// A symlinked descendant inside a provisioned dir yields
    /// `IsSymlink`.  Pins the subtree-walk contract: the walker
    /// surfaces symlinks found anywhere beneath the entry root,
    /// not just at the top level.
    #[cfg(unix)]
    #[test]
    fn symlink_descendant_of_dir_rejected() {
        use std::os::unix::fs::symlink;
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let share = root.join("share");
        fs::create_dir_all(&share).unwrap();
        make_file(&share.join("real.json"));
        symlink(share.join("real.json"), share.join("alias.json")).unwrap();
        let errs = run_entry_check(BUCKET_ORACLE_DIR, EntryKind::Dir, &share);
        assert!(
            errs.iter()
                .any(|e| matches!(e, FileIoConfigError::IsSymlink { .. })),
            "expected IsSymlink inside walked dir; got {errs:?}"
        );
    }

    /// Hard-linked descendant inside a walked dir yields
    /// `HardLinked`.
    #[cfg(unix)]
    #[test]
    fn hardlinked_descendant_of_dir_rejected() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let share = root.join("share");
        fs::create_dir_all(&share).unwrap();
        let a = share.join("a.json");
        let b = share.join("b.json");
        make_file(&a);
        fs::hard_link(&a, &b).unwrap();
        let errs = run_entry_check(BUCKET_ORACLE_DIR, EntryKind::Dir, &share);
        assert!(
            errs.iter().any(|e| matches!(
                e,
                FileIoConfigError::HardLinked { nlink, .. } if *nlink >= 2
            )),
            "expected HardLinked inside walked dir; got {errs:?}"
        );
    }

    /// An empty walked directory produces no errors.
    #[test]
    fn empty_walked_dir_passes() {
        let td = TempDir::new().unwrap();
        let share = td_root(&td).join("empty");
        fs::create_dir_all(&share).unwrap();
        let errs = run_entry_check(BUCKET_ORACLE_DIR, EntryKind::Dir, &share);
        assert!(errs.is_empty(), "expected no errors; got {errs:?}");
    }

    // ---------- Top-level validate_provisioning_boot (ops-6e) ----------

    fn add_oracle_file(cfg: &mut FileIoProvisioning, name: &str, path: PathBuf) {
        cfg.oracle_static_files
            .insert(name.into(), StaticFileEntry {
                path,
                mode: "r".into(),
            });
    }
    fn add_oracle_dir(cfg: &mut FileIoProvisioning, name: &str, path: PathBuf) {
        cfg.oracle_static_dirs.insert(name.into(), StaticDirEntry {
            path,
            mode: "r".into(),
        });
    }
    fn add_consensus_file(cfg: &mut FileIoProvisioning, name: &str, path: PathBuf) {
        cfg.consensus_static_files
            .insert(name.into(), StaticFileEntry {
                path,
                mode: "r".into(),
            });
    }
    #[test]
    fn empty_config_passes() {
        assert!(validate_provisioning_boot(&empty_cfg()).is_ok());
    }

    #[test]
    fn single_regular_file_passes() {
        let td = TempDir::new().unwrap();
        let path = td_root(&td).join("cfg.json");
        make_file(&path);
        let mut cfg = empty_cfg();
        add_oracle_file(&mut cfg, "cfg", path);
        assert!(validate_provisioning_boot(&cfg).is_ok());
    }

    #[test]
    fn single_regular_dir_passes() {
        let td = TempDir::new().unwrap();
        let share = td_root(&td).join("share");
        fs::create_dir_all(&share).unwrap();
        let mut cfg = empty_cfg();
        add_oracle_dir(&mut cfg, "share", share);
        assert!(validate_provisioning_boot(&cfg).is_ok());
    }

    // ---------- PB-M-16 bucket disjointness ----------

    #[test]
    fn same_path_in_both_buckets_rejected() {
        let td = TempDir::new().unwrap();
        let path = td_root(&td).join("f.json");
        make_file(&path);
        let mut cfg = empty_cfg();
        add_oracle_file(&mut cfg, "a", path.clone());
        add_consensus_file(&mut cfg, "b", path);
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, FileIoConfigError::BucketOverlapSamePath { .. })),
            "expected BucketOverlapSamePath; got {errs:?}"
        );
    }

    #[test]
    fn oracle_dir_containing_consensus_file_rejected() {
        let td = TempDir::new().unwrap();
        let outer = td_root(&td).join("data");
        fs::create_dir_all(&outer).unwrap();
        let inner = outer.join("cfg.json");
        make_file(&inner);
        let mut cfg = empty_cfg();
        add_oracle_dir(&mut cfg, "data", outer);
        add_consensus_file(&mut cfg, "cfg", inner);
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, FileIoConfigError::BucketOverlapPrefix { .. })),
            "expected BucketOverlapPrefix; got {errs:?}"
        );
    }

    /// A dir and a file inside it, both in the SAME bucket, is a
    /// legitimate operator pattern (dir cap + explicit file cap at
    /// a different mode).  Must not emit `BucketOverlapPrefix`.
    #[test]
    fn same_bucket_prefix_overlap_allowed() {
        let td = TempDir::new().unwrap();
        let outer = td_root(&td).join("data");
        fs::create_dir_all(&outer).unwrap();
        let inner = outer.join("cfg.json");
        make_file(&inner);
        let mut cfg = empty_cfg();
        add_oracle_dir(&mut cfg, "data", outer);
        add_oracle_file(&mut cfg, "cfg", inner);
        assert!(
            validate_provisioning_boot(&cfg).is_ok(),
            "same-bucket overlap must pass"
        );
    }

    /// `/foo` is NOT a component-prefix of `/foobar`; raw string-
    /// prefix matching would misclassify this as overlap.  Pins
    /// that `Path::starts_with` component-level semantics are
    /// honored.
    #[test]
    fn string_prefix_only_does_not_overlap() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let foo_dir = root.join("foo");
        fs::create_dir_all(&foo_dir).unwrap();
        let foobar_file = root.join("foobar");
        make_file(&foobar_file);
        let mut cfg = empty_cfg();
        add_oracle_dir(&mut cfg, "foo", foo_dir);
        add_consensus_file(&mut cfg, "foobar", foobar_file);
        assert!(
            validate_provisioning_boot(&cfg).is_ok(),
            "string-prefix only must not overlap"
        );
    }

    // ---------- H-3: logical-name conflict across buckets ----------

    #[test]
    fn same_name_across_oracle_file_and_oracle_dir_rejected() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let f = root.join("f");
        let d = root.join("d");
        make_file(&f);
        fs::create_dir_all(&d).unwrap();
        let mut cfg = empty_cfg();
        add_oracle_file(&mut cfg, "shared", f);
        add_oracle_dir(&mut cfg, "shared", d);
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        assert!(
            errs.iter().any(|e| matches!(
                e,
                FileIoConfigError::LogicalNameConflictAcrossBuckets { logical_name, .. }
                    if logical_name == "shared"
            )),
            "expected LogicalNameConflictAcrossBuckets; got {errs:?}"
        );
    }

    #[test]
    fn same_name_across_three_buckets_emits_single_error_with_all_members() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let f1 = root.join("f1");
        let f2 = root.join("f2");
        let d1 = root.join("d1");
        make_file(&f1);
        make_file(&f2);
        fs::create_dir_all(&d1).unwrap();
        let mut cfg = empty_cfg();
        add_oracle_file(&mut cfg, "k", f1);
        add_consensus_file(&mut cfg, "k", f2);
        add_oracle_dir(&mut cfg, "k", d1);
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        let conflicts: Vec<_> = errs
            .iter()
            .filter_map(|e| match e {
                FileIoConfigError::LogicalNameConflictAcrossBuckets {
                    logical_name,
                    members,
                } => Some((logical_name, members)),
                _ => None,
            })
            .collect();
        assert_eq!(conflicts.len(), 1, "expected exactly one conflict error");
        assert_eq!(conflicts[0].0, "k");
        assert_eq!(conflicts[0].1.len(), 3, "all 3 members must be reported");
    }

    // ---------- Non-canonical path ----------

    #[test]
    fn dot_component_rejected_as_non_canonical() {
        let td = TempDir::new().unwrap();
        let path = td_root(&td).join("./cfg.json");
        let mut cfg = empty_cfg();
        add_oracle_file(&mut cfg, "cfg", path);
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        assert!(
            errs.iter().any(|e| matches!(
                e,
                FileIoConfigError::NonCanonicalPath { reason, .. }
                    if reason.contains("`.`")
            )),
            "expected NonCanonicalPath for `.`; got {errs:?}"
        );
    }

    #[test]
    fn dotdot_component_rejected_as_non_canonical() {
        let mut cfg = empty_cfg();
        add_oracle_file(&mut cfg, "cfg", PathBuf::from("/etc/../etc/cfg"));
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        assert!(
            errs.iter().any(|e| matches!(
                e,
                FileIoConfigError::NonCanonicalPath { reason, .. }
                    if reason.contains("`..`")
            )),
            "expected NonCanonicalPath for `..`; got {errs:?}"
        );
    }

    // ---------- Defense-in-depth: programmatic bypass ----------

    /// Programmatic FileIoProvisioning construction can bypass
    /// ops-6a's serde-time absolute-path check.  Boot layer
    /// catches it.
    #[test]
    fn programmatic_relative_path_flagged_as_not_absolute() {
        let mut cfg = empty_cfg();
        add_oracle_file(&mut cfg, "cfg", PathBuf::from("relative/path"));
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, FileIoConfigError::NotAbsolute { .. })),
            "expected NotAbsolute; got {errs:?}"
        );
    }

    /// Same programmatic-bypass defense for mode whitelist.
    #[test]
    fn programmatic_invalid_mode_flagged() {
        let td = TempDir::new().unwrap();
        let path = td_root(&td).join("cfg.json");
        make_file(&path);
        let mut cfg = empty_cfg();
        cfg.oracle_static_files
            .insert("cfg".into(), StaticFileEntry {
                path,
                mode: "rwx".into(),
            });
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, FileIoConfigError::InvalidMode { .. })),
            "expected InvalidMode; got {errs:?}"
        );
    }

    // ---------- Batching + determinism ----------

    /// Multiple distinct violations in one config all surface in
    /// the error Vec.  Plan §370 posture: operator sees every
    /// problem on one boot pass.
    #[test]
    fn multiple_errors_batched() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let shared = root.join("shared");
        make_file(&shared);
        let mut cfg = empty_cfg();
        add_oracle_file(&mut cfg, "x", PathBuf::from("relative"));
        add_oracle_file(&mut cfg, "y", root.join("no_such"));
        add_oracle_file(&mut cfg, "same", shared.clone());
        add_consensus_file(&mut cfg, "other", shared);
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        assert!(errs
            .iter()
            .any(|e| matches!(e, FileIoConfigError::NotAbsolute { .. })));
        assert!(errs
            .iter()
            .any(|e| matches!(e, FileIoConfigError::PathNotFound { .. })));
        assert!(errs
            .iter()
            .any(|e| matches!(e, FileIoConfigError::BucketOverlapSamePath { .. })));
    }

    /// Error output must be byte-equal across identical calls —
    /// `HashMap` iteration is randomized by default, so a leak
    /// through the deterministic-sort would fail intermittently.
    /// Three back-to-back runs give high confidence in one CI
    /// invocation.
    #[test]
    fn boot_validation_error_order_is_deterministic() {
        let mut cfg = empty_cfg();
        for i in 0..8 {
            add_oracle_file(
                &mut cfg,
                &format!("miss-{i}"),
                PathBuf::from(format!("/no/such/f{i}")),
            );
            add_oracle_dir(
                &mut cfg,
                &format!("miss-d-{i}"),
                PathBuf::from(format!("/no/such/d{i}")),
            );
        }
        let errs1 = validate_provisioning_boot(&cfg).unwrap_err();
        let errs2 = validate_provisioning_boot(&cfg).unwrap_err();
        let errs3 = validate_provisioning_boot(&cfg).unwrap_err();
        assert_eq!(errs1, errs2, "run 1 vs 2 must agree");
        assert_eq!(errs2, errs3, "run 2 vs 3 must agree");
        assert!(!errs1.is_empty());
    }

    /// HOCON -> `FileIoProvisioning` -> `validate_provisioning_boot`
    /// integration: a HOCON config that parses cleanly but has a
    /// cross-bucket name collision gets caught at boot.  Pins the
    /// end-to-end pipeline.
    #[test]
    fn hocon_parse_then_validate_integration() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let f = root.join("f");
        let d = root.join("d");
        make_file(&f);
        fs::create_dir_all(&d).unwrap();
        let hocon = format!(
            r#"
            oracle-static-files {{ "shared" = "{}" }}
            consensus-static-dirs {{ "shared" = "{}" }}
            "#,
            f.display(),
            d.display()
        );
        let cfg: FileIoProvisioning = hocon::HoconLoader::new()
            .load_str(&hocon)
            .expect("hocon load")
            .resolve()
            .expect("hocon resolve");
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        assert!(
            errs.iter().any(|e| matches!(
                e,
                FileIoConfigError::LogicalNameConflictAcrossBuckets { .. }
            )),
            "expected LogicalNameConflictAcrossBuckets; got {errs:?}"
        );
    }

    // ---------- normalize_for_compare ----------

    #[test]
    fn normalize_for_compare_strips_dot_and_double_slash() {
        assert_eq!(
            normalize_for_compare(Path::new("/a/b")),
            PathBuf::from("/a/b")
        );
        assert_eq!(
            normalize_for_compare(Path::new("/a/./b")),
            PathBuf::from("/a/b")
        );
        assert_eq!(
            normalize_for_compare(Path::new("/a//b")),
            PathBuf::from("/a/b")
        );
        assert_eq!(
            normalize_for_compare(Path::new("/a/b/")),
            PathBuf::from("/a/b")
        );
    }
}
