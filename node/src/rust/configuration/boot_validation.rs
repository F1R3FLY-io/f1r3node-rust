//! File I/O FIP boot-time validation (ops-6b scaffold slice).
//!
//! Enumerates every spec-mandated boot-time invariant for the
//! static-provisioning config as a typed error, plus the
//! operator-facing `Display` impl.  Validator wiring that emits
//! these errors lands across subsequent slices:
//!
//!   * ops-6c — flatten helpers + sort_key.
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

use std::path::PathBuf;

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
}
