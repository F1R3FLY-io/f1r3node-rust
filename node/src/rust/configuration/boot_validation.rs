//! File I/O FIP boot-time validation.
//!
//! Enumerates every spec-mandated boot-time invariant for the
//! static-provisioning config as a typed error, plus the
//! operator-facing `Display` impl, plus the flatten / canonical-
//! path / deterministic-sort scaffolding downstream validators
//! use.  Validator wiring that emits these errors lands across
//! subsequent slices:
//!
//!   * ops-6d — per-entry validators (symlink, hard-link,
//!     kind-mismatch, forbidden-chars, canonical-path).
//!   * ops-6e — cross-source + bucket-disjointness validators
//!     + top-level `validate_provisioning_boot` entry point.
//!   * ops-7  — CLI-merge path (populates the three
//!     `Duplicate*` variants from `provisioning_merge`).
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

#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::file_io_provisioning::{FileIoProvisioning, StaticDirEntry, StaticFileEntry};

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
/// The 15 ops-6 variants are emitted by `validate_provisioning_boot`
/// (ops-6c..6e); the 3 `Duplicate*` variants are emitted by the
/// CLI-merge layer (ops-7).
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
}
