//! File I/O FIP boot-time validation (Phase 7 slice 23).
//!
//! Fails the node's launch if any static-provisioning entry violates
//! the spec's boot-time invariants:
//!
//! - **No symlinks** anywhere along a configured path — neither the
//!   entry itself nor any ancestor.  A symlink on the path could
//!   redirect FS access to a location the operator did not intend
//!   and the config file does not disclose.
//! - **No hard-linked regular files** (Unix `nlink() > 1`) — two
//!   config entries pointing at "different" paths could refer to the
//!   same inode, defeating the disjointness invariant PB-M-16 relies
//!   on and making per-cap mode enforcement ineffective.
//! - **Absolute-prefix symlink diagnostic** — on macOS `/tmp` is a
//!   symlink to `/private/tmp`, so a config declaring `/tmp/foo` is
//!   really talking about `/private/tmp/foo`.  Detect and instruct
//!   the operator to supply the canonical path.
//! - **Bucket disjointness (PB-M-16)** — the same path cannot be
//!   both consensus-replicated and oracle-local.  Reject same-path
//!   in both buckets, and reject prefix overlap (a dir declared in
//!   one bucket containing a path declared in the other).
//!
//! All errors are batched into a `Vec<FileIoConfigError>` so the
//! operator sees every violation in one boot-failure report (plan
//! §370 design constraint).  Slice 24 will merge CLI-provided
//! entries into the same validator before invoking it; for now the
//! validator runs against the config-file surface only.

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

/// One provisioning-config violation.  Every variant carries enough
/// context to point the operator at the offending config line
/// (bucket + logical name) plus the underlying host detail (path,
/// nlink, canonical resolution).
#[derive(Debug, PartialEq, Eq)]
pub enum FileIoConfigError {
    /// The declared path itself is a symlink.
    IsSymlink {
        bucket: &'static str,
        logical_name: String,
        path: PathBuf,
    },

    /// An ancestor directory on the declared path is a symlink.
    /// The `prefix` is the ancestor that's actually a symlink; the
    /// `canonical` is where the OS resolves it to (`None` if the
    /// resolution failed — e.g., dangling symlink or permission
    /// denied on the target).  Points the operator at the fix
    /// ("supply this canonical path instead").
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
    /// the same on-disk entity would otherwise defeat the
    /// disjointness check (PB-M-16): a config declaring
    /// `/srv/data/./cfg.json` and `/srv/data/cfg.json` in different
    /// buckets would appear disjoint despite naming the same file.
    /// Operators must supply lexically canonical absolute paths.
    NonCanonicalPath {
        bucket: &'static str,
        logical_name: String,
        path: PathBuf,
        reason: &'static str,
    },

    /// Boot-time defense-in-depth: the declared path failed
    /// `validate_absolute_path` (slice 21's syntactic check).
    /// Fires only if the caller constructed a `FileIoProvisioning`
    /// programmatically, bypassing serde-time validation.
    NotAbsolute {
        bucket: &'static str,
        logical_name: String,
        path: PathBuf,
    },

    /// Boot-time defense-in-depth: the declared mode failed the
    /// slice-21 whitelist.  Same rationale as `NotAbsolute`.
    InvalidMode {
        bucket: &'static str,
        logical_name: String,
        mode: String,
    },

    /// C-25-2 slice-25 review fix: logical name / path / mode
    /// contains a control character or invisible glyph (NUL, C0/C1
    /// control, DEL, BOM, RTL override, LS/PS).  Slice 22 CLI
    /// enforces at parse; slice 21 HOCON enforces at deserialize
    /// (via `reject_forbidden_chars`); this variant catches
    /// programmatic construction that bypasses both.
    ForbiddenChars {
        bucket: &'static str,
        logical_name: String,
        field: &'static str,
        detail: String,
    },

    /// C-25-1 slice-25 review fix (also PB-M-16 tightening): the
    /// same logical name appears in an `oracle-static-*` bucket AND
    /// a `consensus-static-*` bucket.  Fs.rho's `bMap` is a single
    /// keyed-by-logical-name namespace; two buckets emitting the
    /// same key would produce a Rholang map where second-write
    /// wins, silently discarding one entry.  Reject at boot.
    ///
    /// H-3 fix (2026-08-06): the pre-fix variant
    /// (`LogicalNameConflictAcrossOracleConsensus`) only detected
    /// oracle × consensus name collisions.  Same-side, cross-bucket
    /// collisions (e.g., `oracle-static-files{"shared": ...}` +
    /// `oracle-static-dirs{"shared": ...}`) silently passed
    /// validation and then panicked in
    /// `format_bundle_for_rholang` at genesis composition —
    /// network-wide DoS.  This variant now covers ANY pair of
    /// buckets in the flat entry set; `members` lists every
    /// (bucket, path) pair for the colliding name.
    LogicalNameConflictAcrossBuckets {
        logical_name: String,
        members: Vec<(&'static str, PathBuf)>,
    },

    /// Total provisioning-entry count exceeds
    /// `MAX_PROVISIONING_ENTRIES` (slice 21 cap).  Batched here so
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

    /// Slice 24 merge: the same logical name appears in both the
    /// config-file surface and the CLI surface within the same
    /// bucket, with different `(path, mode)` definitions.  Operator
    /// must pick one source to avoid ambiguity.  Identical
    /// duplicates (same path + same mode) are silently deduped and
    /// do not surface as an error.
    DuplicateLogicalNameAcrossSources {
        bucket: &'static str,
        logical_name: String,
        config_path: PathBuf,
        config_mode: String,
        cli_path: PathBuf,
        cli_mode: String,
    },

    /// Slice 24 merge: the CLI surface contains multiple
    /// repetitions of the same `--*-static-*` flag with the same
    /// logical name but non-identical `(path, mode)` payloads.
    /// (Slice 22 accumulated all repetitions; slice 24 dedups here
    /// with an explicit error on conflict.)  Identical repeated
    /// entries are silently deduped.
    DuplicateLogicalNameInCli {
        bucket: &'static str,
        logical_name: String,
        count: usize,
    },

    /// Slice 24 merge: the same absolute host path appears in both
    /// the config-file surface and the CLI surface within the same
    /// bucket, under DIFFERENT logical names.  Almost always an
    /// operator error (they forgot they'd already provisioned this
    /// path).  Plan §362 mandates the error; operators suppress by
    /// removing one of the two spellings.
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
/// whether the tree walker recurses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryKind {
    File,
    Dir,
}

/// A flattened view of one provisioning entry used by the validator.
#[derive(Debug)]
pub(crate) struct FlatEntry<'a> {
    pub bucket: &'static str,
    pub logical_name: &'a str,
    pub path: &'a Path,
    pub mode: &'a str,
    pub kind: EntryKind,
    pub is_consensus: bool,
}

/// Flatten the four provisioning maps into a single iterable view.
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
    // M-23-1 determinism: sort by (bucket, logical_name) so the
    // downstream cross-bucket loop iterates in a fixed order and
    // any error-`Vec` construction is reproducible across runs
    // (HashMap iteration is randomized by default in Rust).
    out.sort_by(|a, b| {
        a.bucket
            .cmp(b.bucket)
            .then_with(|| a.logical_name.cmp(b.logical_name))
    });
    out
}

/// Reject `.` and `..` components (H-23-2 canonicalization gap).
/// Returns `Some(reason)` if the path is non-canonical, `None` if
/// clean.
///
/// Uses string-level inspection because Rust's `Path::components()`
/// silently drops `Component::CurDir` — so a components-only check
/// would miss `/foo/./bar`.  `PathBuf` equality is byte-level, not
/// component-level, so `/foo/./bar` and `/foo/bar` compare unequal
/// and would defeat the disjointness check without rejection here.
/// (`..` DOES round-trip as `Component::ParentDir`, but we use the
/// same string check for symmetry and clearer error messages.)
///
/// Trailing `/` and `//` are NOT rejected because `Path` equality
/// normalizes them (verified by the trailing-slash overlap test).
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

/// Deterministic sort key for `FileIoConfigError` (M-23-1).  Orders
/// by (discriminant, bucket, logical_name, path) so the report
/// operators see is stable across HashMap-randomized runs.
///
/// Exposed at `pub(crate)` so slice 24's merge layer can sort its
/// combined error output before returning (H-24-1).
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

/// M-25-7 slice-25 review fix: cross-source logical-name conflict.
/// Fs.rho's `bMap` has a single logical-name namespace shared by
/// both `oracle-static-*` and `consensus-static-*` buckets.  If two
/// entries carry the same logical name across the oracle/consensus
/// divide, the emitted Rholang map has a duplicate key and
/// second-write silently wins, discarding one entry.  Reject at
/// boot rather than allow silent data loss.
///
/// H-3 fix (2026-08-06): pre-fix, this check tracked oracle and
/// consensus names in *separate* HashMap-per-side buffers.  On the
/// SAME side, two entries with the same `logical_name` in
/// different buckets (e.g., `oracle-static-files{"shared":...}`
/// and `oracle-static-dirs{"shared":...}`) silently overwrote in
/// the HashMap — only one entry survived boot validation.  The
/// other entry then re-appeared during `project_bundle`'s flat
/// walk of all four buckets, so the projected `Vec<BundleEntry>`
/// contained BOTH.  `format_bundle_for_rholang` sorts by
/// `logical_name` and asserts adjacent entries have distinct
/// names — the assert-panic fires deterministically on EVERY
/// validator, causing a network-wide genesis DoS on any shard
/// whose operators accidentally cross-bucket-shadowed a name.
///
/// Post-fix: the check uses a single `HashMap<logical_name,
/// Vec<&FlatEntry>>` keyed by name across ALL four buckets.  Any
/// name appearing in `> 1` entry emits `LogicalNameConflictAcross
/// Buckets` regardless of which bucket pair collided.  Boot fails
/// LOUDLY at the validator's local config-check step with a
/// clear operator-facing message, instead of failing with a
/// panic at genesis-composition time on every peer.
fn check_cross_source_logical_name_conflict(
    entries: &[FlatEntry<'_>],
    errors: &mut Vec<FileIoConfigError>,
) {
    use std::collections::HashMap;
    let mut by_name: HashMap<&str, Vec<&FlatEntry<'_>>> = HashMap::new();
    for e in entries {
        by_name.entry(e.logical_name).or_default().push(e);
    }
    // Deterministic emit: sort by name; within a collision, sort
    // members by bucket for stable operator-facing output.
    let mut names: Vec<&&str> = by_name
        .iter()
        .filter(|(_, vs)| vs.len() > 1)
        .map(|(n, _)| n)
        .collect();
    names.sort();
    for name in names {
        let mut vs = by_name[*name].clone();
        vs.sort_by_key(|e| e.bucket);
        // Emit one error per collision (the entire member list is
        // included in the emitted variant so a single operator
        // report describes the full ambiguity).
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
/// and prefix overlaps between oracle and consensus buckets.  Same-
/// bucket overlaps are legal (a user may legitimately declare a
/// directory and a file inside it in the same bucket).
fn check_bucket_disjointness(entries: &[FlatEntry<'_>], errors: &mut Vec<FileIoConfigError>) {
    use super::provisioning_merge::normalize_for_compare;

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
        // M-P7-3 review fix: compare via lexical-normalized paths so
        // `/etc/foo` vs `/etc/./foo` are recognized as SamePath
        // (pre-fix classified as Prefix due to raw-byte `!=` between
        // PathBufs that Path::components() treats as equal).  Kept
        // the original path in the emitted error so operator sees
        // what they wrote.  `NonCanonicalPath` rejects `.`/`..`
        // upstream, so this normalization is defense-in-depth.
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
/// path-component prefix of the other's, else `None`.  A path is a
/// prefix of another only when the other is inside its directory
/// tree — string-prefix isn't enough (`/foo` is not a component
/// prefix of `/foobar`).
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

/// Walk each entry and emit tree-shape errors.  For file entries,
/// stat the single path; for dir entries, stat and recursively
/// walk descendants.  The absolute-prefix symlink check is done
/// once per entry against each ancestor from root to parent.
fn check_entries(entries: &[FlatEntry<'_>], errors: &mut Vec<FileIoConfigError>) {
    for e in entries {
        check_absolute_prefix(e, errors);
        check_entry_tree(e, errors);
    }
}

/// For every ancestor of `entry.path` from the root down to (but
/// not including) the entry itself, `symlink_metadata` it.  If any
/// ancestor is a symlink, emit `AbsolutePrefixSymlink`.  We stop at
/// the first symlink ancestor — the diagnostic points the operator
/// at the shallowest one, which usually captures the root cause
/// (e.g., `/tmp` on macOS).
///
/// Ancestor stat errors other than `NotFound` now emit `StatFailed`
/// (H-23-3): previously silent, which could hide a symlink behind a
/// permission-denied ancestor.  `NotFound` is still silent because
/// `check_entry_tree` will report `PathNotFound` for the entry
/// itself.
fn check_absolute_prefix(entry: &FlatEntry<'_>, errors: &mut Vec<FileIoConfigError>) {
    let mut ancestors: Vec<&Path> = entry.path.ancestors().skip(1).collect();
    ancestors.reverse(); // root-down so the shallowest symlink wins
    for anc in ancestors {
        if anc.as_os_str().is_empty() {
            continue;
        }
        match fs::symlink_metadata(anc) {
            Ok(md) if md.file_type().is_symlink() => {
                // S-23-1: canonical is None if canonicalize fails
                // (dangling symlink or unreadable target).
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
            Ok(_) => {} // regular dir, continue walking
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // A missing ancestor will trip the per-entry check
                // below; no separate diagnostic here.
                return;
            }
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

/// Stat the entry itself; if it's a symlink error out.  If it's a
/// file, check hard-links.  If it's a dir, recurse.
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
            // Not file, not dir, not symlink — likely a device
            // node, socket, FIFO, etc.  Not part of any spec use
            // case; report as kind mismatch with actual="other".
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

/// Iterative dir walker (M-23-4: converted from recursion to avoid
/// stack overflow on deep trees).  For every entry: if a symlink →
/// error; if a regular file → hardlink check; if a subdir → push
/// onto work stack.  Emits `KindMismatch` for non-file/non-dir/
/// non-symlink children (S-23-5: previously silently ignored,
/// inconsistent with top-level behavior).
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
                // S-23-5: FIFO / socket / device — emit
                // KindMismatch("other") for consistency with the
                // top-level `check_entry_tree` behavior.
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
/// non-Unix hosts the check is a no-op (hard links are less
/// consequential on Windows because most FS backends don't allow
/// them across mount points).
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
    // No-op on non-Unix; hard-link semantics aren't consistent
    // enough across Windows FSs to gate boot on.
}

/// Boot-time validation entry point.  Runs bucket-disjointness and
/// per-entry tree walk; batches every violation into a single
/// `Vec<FileIoConfigError>`.  Returns `Ok(())` iff no violations.
///
/// Validation order:
///   1. Size cap (M-23-3): abort further tree walking if the total
///      entry count exceeds `MAX_PROVISIONING_ENTRIES`; still return
///      the accumulated syntactic errors below.
///   2. Defense-in-depth (M-23-2): re-check every path's absolute-
///      ness, mode whitelist, and lexical canonicity.  Redundant
///      with slice 21's serde-time checks, but catches programmatic
///      construction (test harnesses, slice-24 CLI merger, etc.)
///      that bypasses serde.  Non-canonical / non-absolute /
///      invalid-mode entries are FLAGGED but then EXCLUDED from
///      downstream disjointness + tree-walk to avoid cascade errors.
///   3. Bucket disjointness (PB-M-16).
///   4. Per-entry tree walk (ancestor symlinks, entry symlink, hard
///      links, kind mismatch).
///   5. Sort errors (M-23-1) for deterministic operator output.
///
/// Intended call site: the node's boot pipeline, after config
/// parsing and slice-24's CLI/config merge, before genesis handoff
/// to `Fs!?(...)`.  Slice 23 wires it against the config-file
/// surface only; slice 24 extends the input to include CLI entries.
pub fn validate_provisioning_boot(cfg: &FileIoProvisioning) -> Result<(), Vec<FileIoConfigError>> {
    let mut errors: Vec<FileIoConfigError> = Vec::new();

    // (1) Size cap.  On breach, still surface subsequent syntactic
    // errors — but skip the tree walk to bound cost.
    let size_ok = match validate_size_limits(cfg) {
        Ok(()) => true,
        Err(message) => {
            errors.push(FileIoConfigError::SizeLimitExceeded { message });
            false
        }
    };

    // (2) Defense-in-depth syntactic re-checks + canonicity.  Build
    // a filtered entry set for downstream tree/disjointness passes.
    let all_entries = flatten(cfg);
    let mut valid_entries: Vec<FlatEntry<'_>> = Vec::with_capacity(all_entries.len());
    for e in all_entries {
        let mut skip = false;
        if let Err(msg) = validate_absolute_path(e.path) {
            let _ = msg; // message thrown away; NotAbsolute variant carries the shape.
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
        // C-25-2: defense-in-depth forbidden-char re-check on
        // logical name, path, and mode.
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

    // M-25-7: cross-bucket logical-name conflict.  Fs.rho bMap has
    // one namespace; same key in oracle + consensus buckets =
    // silent-overwrite hazard.
    check_cross_source_logical_name_conflict(&valid_entries, &mut errors);

    if size_ok {
        // (3) Bucket disjointness — only on the pre-validated set.
        check_bucket_disjointness(&valid_entries, &mut errors);
        // (4) Per-entry tree walk.
        check_entries(&valid_entries, &mut errors);
    }

    // (5) Deterministic error order.
    errors.sort_by_key(sort_key);

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use tempfile::TempDir;

    use super::*;

    // -------------------- helpers --------------------

    fn empty_cfg() -> FileIoProvisioning {
        FileIoProvisioning {
            oracle_static_files: HashMap::new(),
            oracle_static_dirs: HashMap::new(),
            consensus_static_files: HashMap::new(),
            consensus_static_dirs: HashMap::new(),
        }
    }

    fn make_file(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, b"x").unwrap();
    }

    /// Returns the tempdir's *canonicalized* path.  macOS's `/var`
    /// is a symlink to `/private/var`, and `TempDir` returns paths
    /// under `/var/folders/...` — so uncanonicalized paths would
    /// trip the (correct) absolute-prefix-symlink diagnostic in
    /// every test.  Real config files supply canonical paths per
    /// the diagnostic's exact recommendation.
    fn td_root(td: &TempDir) -> PathBuf { fs::canonicalize(td.path()).unwrap() }

    fn add_oracle_file(cfg: &mut FileIoProvisioning, name: &str, path: PathBuf) {
        cfg.oracle_static_files
            .insert(name.into(), StaticFileEntry {
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

    fn add_oracle_dir(cfg: &mut FileIoProvisioning, name: &str, path: PathBuf) {
        cfg.oracle_static_dirs.insert(name.into(), StaticDirEntry {
            path,
            mode: "r".into(),
        });
    }

    fn add_consensus_dir(cfg: &mut FileIoProvisioning, name: &str, path: PathBuf) {
        cfg.consensus_static_dirs
            .insert(name.into(), StaticDirEntry {
                path,
                mode: "r".into(),
            });
    }

    // -------------------- empty / happy-path --------------------

    #[test]
    fn empty_config_passes() {
        let cfg = empty_cfg();
        assert!(validate_provisioning_boot(&cfg).is_ok());
    }

    #[test]
    fn single_regular_file_passes() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let p = root.as_path().join("cfg.json");
        make_file(&p);
        let mut cfg = empty_cfg();
        add_oracle_file(&mut cfg, "cfg", p);
        assert!(
            validate_provisioning_boot(&cfg).is_ok(),
            "regular file must pass"
        );
    }

    #[test]
    fn single_regular_dir_passes() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let mut cfg = empty_cfg();
        add_oracle_dir(&mut cfg, "d", root.as_path().to_path_buf());
        assert!(
            validate_provisioning_boot(&cfg).is_ok(),
            "regular dir must pass"
        );
    }

    #[test]
    fn dir_with_regular_descendants_passes() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        make_file(&root.as_path().join("a.txt"));
        make_file(&root.as_path().join("sub/b.txt"));
        make_file(&root.as_path().join("sub/c/d.txt"));
        let mut cfg = empty_cfg();
        add_oracle_dir(&mut cfg, "d", root.as_path().to_path_buf());
        assert!(validate_provisioning_boot(&cfg).is_ok());
    }

    // -------------------- IsSymlink --------------------

    #[cfg(unix)]
    #[test]
    fn symlink_as_entry_rejected() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let target = root.as_path().join("target.txt");
        make_file(&target);
        let link = root.as_path().join("link.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let mut cfg = empty_cfg();
        add_oracle_file(&mut cfg, "cfg", link);
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, FileIoConfigError::IsSymlink { .. })),
            "expected IsSymlink; got {errs:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_descendant_of_dir_rejected() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let target = root.as_path().join("real.txt");
        make_file(&target);
        let link = root.as_path().join("link.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let mut cfg = empty_cfg();
        add_oracle_dir(&mut cfg, "d", root.as_path().to_path_buf());
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        assert!(
            errs.iter().any(|e| matches!(
                e,
                FileIoConfigError::IsSymlink { path, .. } if path == &link
            )),
            "expected IsSymlink at {link:?}; got {errs:?}"
        );
    }

    // -------------------- HardLinked --------------------

    #[cfg(unix)]
    #[test]
    fn hardlinked_file_rejected() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let a = root.as_path().join("a.txt");
        make_file(&a);
        let b = root.as_path().join("b.txt");
        fs::hard_link(&a, &b).unwrap();
        let mut cfg = empty_cfg();
        add_oracle_file(&mut cfg, "a", a.clone());
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        assert!(
            errs.iter().any(|e| matches!(
                e,
                FileIoConfigError::HardLinked { path, nlink, .. } if path == &a && *nlink >= 2
            )),
            "expected HardLinked at {a:?}; got {errs:?}"
        );
    }

    // -------------------- AbsolutePrefixSymlink --------------------

    #[cfg(unix)]
    #[test]
    fn ancestor_symlink_diagnostic() {
        // /td/actual/  <- real dir
        // /td/link -> actual   <- symlinked ancestor
        // config declares /td/link/cfg.txt (via the symlink)
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let actual = root.as_path().join("actual");
        fs::create_dir(&actual).unwrap();
        let cfg_path_actual = actual.join("cfg.txt");
        make_file(&cfg_path_actual);
        let link = root.as_path().join("link");
        std::os::unix::fs::symlink(&actual, &link).unwrap();
        let via_link = link.join("cfg.txt");
        let mut cfg = empty_cfg();
        add_oracle_file(&mut cfg, "cfg", via_link.clone());
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        assert!(
            errs.iter().any(|e| matches!(
                e,
                FileIoConfigError::AbsolutePrefixSymlink { prefix, .. } if prefix == &link
            )),
            "expected AbsolutePrefixSymlink at {link:?}; got {errs:?}"
        );
    }

    // -------------------- PathNotFound --------------------

    #[test]
    fn missing_path_rejected() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let p = root.as_path().join("does-not-exist.txt");
        let mut cfg = empty_cfg();
        add_oracle_file(&mut cfg, "missing", p.clone());
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        assert!(
            errs.iter().any(|e| matches!(
                e,
                FileIoConfigError::PathNotFound { path, .. } if path == &p
            )),
            "expected PathNotFound at {p:?}; got {errs:?}"
        );
    }

    // -------------------- BucketOverlapSamePath --------------------

    #[test]
    fn same_path_in_both_buckets_rejected() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let p = root.as_path().join("shared.txt");
        make_file(&p);
        let mut cfg = empty_cfg();
        add_oracle_file(&mut cfg, "o-name", p.clone());
        add_consensus_file(&mut cfg, "c-name", p.clone());
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        assert!(
            errs.iter().any(|e| matches!(
                e,
                FileIoConfigError::BucketOverlapSamePath { path, .. } if path == &p
            )),
            "expected BucketOverlapSamePath at {p:?}; got {errs:?}"
        );
    }

    // -------------------- BucketOverlapPrefix --------------------

    #[test]
    fn oracle_dir_containing_consensus_file_rejected() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let outer = root.as_path().to_path_buf();
        let inner = outer.join("sub/leaf.txt");
        make_file(&inner);
        let mut cfg = empty_cfg();
        add_oracle_dir(&mut cfg, "outer", outer.clone());
        add_consensus_file(&mut cfg, "inner", inner.clone());
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        assert!(
            errs.iter().any(|e| matches!(
                e,
                FileIoConfigError::BucketOverlapPrefix { outer_path, inner_path, .. }
                    if outer_path == &outer && inner_path == &inner
            )),
            "expected BucketOverlapPrefix outer={outer:?} inner={inner:?}; got {errs:?}"
        );
    }

    #[test]
    fn consensus_dir_containing_oracle_file_rejected() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let outer = root.as_path().to_path_buf();
        let inner = outer.join("nested/leaf.txt");
        make_file(&inner);
        let mut cfg = empty_cfg();
        add_consensus_dir(&mut cfg, "outer", outer.clone());
        add_oracle_file(&mut cfg, "inner", inner.clone());
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        assert!(
            errs.iter().any(|e| matches!(
                e,
                FileIoConfigError::BucketOverlapPrefix { outer_path, inner_path, .. }
                    if outer_path == &outer && inner_path == &inner
            )),
            "expected BucketOverlapPrefix outer={outer:?} inner={inner:?}; got {errs:?}"
        );
    }

    /// PB-M-16 note: same-bucket overlap is legal.  Only cross-
    /// bucket overlap is rejected.
    #[test]
    fn same_bucket_prefix_overlap_allowed() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let outer = root.as_path().to_path_buf();
        let inner = outer.join("child.txt");
        make_file(&inner);
        let mut cfg = empty_cfg();
        add_oracle_dir(&mut cfg, "outer", outer);
        add_oracle_file(&mut cfg, "inner", inner);
        // Prefix-overlap within one bucket is permitted (the tree
        // walker will see the file both as a descendant of the dir
        // and as an entry itself, but no BucketOverlapPrefix fires).
        let res = validate_provisioning_boot(&cfg);
        assert!(res.is_ok(), "same-bucket overlap should pass: {res:?}");
    }

    /// String-prefix (not path-component prefix) must NOT trip
    /// BucketOverlapPrefix.  `/foo` is not a component prefix of
    /// `/foobar`.
    #[test]
    fn string_prefix_only_does_not_overlap() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let a = root.as_path().join("foo");
        let b = root.as_path().join("foobar");
        fs::create_dir(&a).unwrap();
        fs::create_dir(&b).unwrap();
        let mut cfg = empty_cfg();
        add_oracle_dir(&mut cfg, "o", a);
        add_consensus_dir(&mut cfg, "c", b);
        let res = validate_provisioning_boot(&cfg);
        assert!(
            res.is_ok(),
            "string-prefix `foo`/`foobar` must not overlap: {res:?}"
        );
    }

    // -------------------- Batching --------------------

    /// Multiple violations in one config should surface together,
    /// not one at a time (plan §370 design constraint).
    #[test]
    fn multiple_errors_batched() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let missing = root.as_path().join("missing.txt");
        let shared = root.as_path().join("shared.txt");
        make_file(&shared);
        let mut cfg = empty_cfg();
        add_oracle_file(&mut cfg, "missing", missing.clone());
        add_oracle_file(&mut cfg, "shared-o", shared.clone());
        add_consensus_file(&mut cfg, "shared-c", shared.clone());
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        let has_missing = errs
            .iter()
            .any(|e| matches!(e, FileIoConfigError::PathNotFound { .. }));
        let has_overlap = errs
            .iter()
            .any(|e| matches!(e, FileIoConfigError::BucketOverlapSamePath { .. }));
        assert!(
            has_missing && has_overlap,
            "both errors must appear; got {errs:?}"
        );
    }

    // -------------------- Display coverage --------------------

    // -------------------- KindMismatch --------------------

    #[test]
    fn file_declared_but_dir_on_disk_rejected() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let d = root.join("actually-a-dir");
        fs::create_dir(&d).unwrap();
        let mut cfg = empty_cfg();
        add_oracle_file(&mut cfg, "wrong", d.clone());
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        assert!(
            errs.iter().any(|e| matches!(
                e,
                FileIoConfigError::KindMismatch { path, declared, actual, .. }
                    if path == &d && *declared == "file" && *actual == "directory"
            )),
            "expected KindMismatch file→dir at {d:?}; got {errs:?}"
        );
    }

    #[test]
    fn dir_declared_but_file_on_disk_rejected() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let p = root.join("actually-a-file.txt");
        make_file(&p);
        let mut cfg = empty_cfg();
        add_oracle_dir(&mut cfg, "wrong", p.clone());
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        assert!(
            errs.iter().any(|e| matches!(
                e,
                FileIoConfigError::KindMismatch { path, declared, actual, .. }
                    if path == &p && *declared == "directory" && *actual == "file"
            )),
            "expected KindMismatch dir→file at {p:?}; got {errs:?}"
        );
    }

    // -------------------- Display coverage --------------------

    /// Error `Display` messages must contain the bucket + logical
    /// name so an operator can find the offending config line.
    #[test]
    fn error_messages_include_bucket_and_name() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let p = root.as_path().join("nope.txt");
        let mut cfg = empty_cfg();
        add_consensus_file(&mut cfg, "logs/missing", p);
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        let msg = format!("{}", errs[0]);
        assert!(
            msg.contains("consensus-static-files"),
            "bucket in msg: {msg}"
        );
        assert!(msg.contains("logs/missing"), "name in msg: {msg}");
    }

    // -------------------- Review-driven additions --------------------
    // (2026-08-03 slice-23 review fixes)

    // ---------- Must-fix coverage ----------

    /// M-23-6: `StatFailed` on ancestor (permission-denied dir).
    /// The ancestor stat error path was previously silent — now
    /// emits StatFailed per H-23-3.
    #[cfg(unix)]
    #[test]
    fn permission_denied_ancestor_yields_stat_failed_or_notfound() {
        use std::os::unix::fs::PermissionsExt;
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let locked = root.as_path().join("locked");
        fs::create_dir(&locked).unwrap();
        let inside = locked.join("target.txt");
        make_file(&inside);
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let mut cfg = empty_cfg();
        add_oracle_file(&mut cfg, "x", inside);
        let res = validate_provisioning_boot(&cfg);
        // Restore permissions before assertion so cleanup works.
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        // Root can bypass 0o000; tolerate Ok if running as root.
        if let Err(errs) = res {
            assert!(
                errs.iter().any(|e| matches!(
                    e,
                    FileIoConfigError::StatFailed { .. } | FileIoConfigError::PathNotFound { .. }
                )),
                "expected StatFailed or PathNotFound; got {errs:?}"
            );
        }
    }

    /// M-23-7: hard-linked file *inside* a walked dir.  Distinct
    /// code branch (walk_dir → check_hardlink) from the direct
    /// file-entry path exercised by `hardlinked_file_rejected`.
    #[cfg(unix)]
    #[test]
    fn hardlinked_file_inside_walked_dir_rejected() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let a = root.as_path().join("a.txt");
        make_file(&a);
        let b = root.as_path().join("b.txt");
        fs::hard_link(&a, &b).unwrap();
        let mut cfg = empty_cfg();
        add_oracle_dir(&mut cfg, "d", root.clone());
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        let hard_count = errs
            .iter()
            .filter(|e| matches!(e, FileIoConfigError::HardLinked { .. }))
            .count();
        assert!(
            hard_count >= 2,
            "expected >=2 HardLinked (both a and b); got {errs:?}"
        );
    }

    /// M-23-8: dir+dir same-path bucket overlap.
    #[test]
    fn dir_dir_same_path_in_both_buckets_rejected() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let mut cfg = empty_cfg();
        add_oracle_dir(&mut cfg, "o", root.clone());
        add_consensus_dir(&mut cfg, "c", root.clone());
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        assert!(
            errs.iter().any(|e| matches!(
                e,
                FileIoConfigError::BucketOverlapSamePath { path, .. } if path == &root
            )),
            "expected BucketOverlapSamePath for dir/dir; got {errs:?}"
        );
    }

    // -------------------- H-3 regression pins (2026-08-06) --------------------
    //
    // Same-side, cross-bucket name collision.  Pre-H-3, the check
    // used `HashMap<name, entry>` on each side, so
    // oracle-static-files{"shared":...} + oracle-static-dirs{"shared":...}
    // silently overwrote in the HashMap and validation passed.
    // `project_bundle` then included BOTH (flat walk of all 4 buckets),
    // and `format_bundle_for_rholang` panicked on the adjacent
    // duplicate — deterministic on every validator, network-wide
    // genesis DoS.  Post-H-3 each such collision emits
    // `LogicalNameConflictAcrossBuckets` at boot.

    #[test]
    fn same_name_across_oracle_file_and_oracle_dir_rejected() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let file_path = root.join("data.bin");
        make_file(&file_path);
        let dir_path = root.join("dir");
        fs::create_dir_all(&dir_path).unwrap();
        let mut cfg = empty_cfg();
        add_oracle_file(&mut cfg, "shared", file_path);
        add_oracle_dir(&mut cfg, "shared", dir_path);
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        assert!(
            errs.iter().any(|e| matches!(
                e,
                FileIoConfigError::LogicalNameConflictAcrossBuckets { logical_name, members }
                    if logical_name == "shared" && members.len() == 2
            )),
            "H-3 regression: same name in oracle-file and oracle-dir must emit \
             LogicalNameConflictAcrossBuckets; got {errs:?}"
        );
    }

    #[test]
    fn same_name_across_consensus_file_and_consensus_dir_rejected() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let file_path = root.join("data.bin");
        make_file(&file_path);
        let dir_path = root.join("dir");
        fs::create_dir_all(&dir_path).unwrap();
        let mut cfg = empty_cfg();
        add_consensus_file(&mut cfg, "shared", file_path);
        add_consensus_dir(&mut cfg, "shared", dir_path);
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        assert!(
            errs.iter().any(|e| matches!(
                e,
                FileIoConfigError::LogicalNameConflictAcrossBuckets { logical_name, .. }
                    if logical_name == "shared"
            )),
            "H-3 regression: same name in consensus-file and consensus-dir must \
             emit LogicalNameConflictAcrossBuckets; got {errs:?}"
        );
    }

    /// The pre-H-3 covered case (oracle-file × consensus-file with
    /// same name) MUST still emit under the widened check.
    #[test]
    fn same_name_across_oracle_file_and_consensus_file_still_rejected() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let oracle_path = root.join("oracle.bin");
        let consensus_path = root.join("consensus.bin");
        make_file(&oracle_path);
        make_file(&consensus_path);
        let mut cfg = empty_cfg();
        add_oracle_file(&mut cfg, "shared", oracle_path);
        add_consensus_file(&mut cfg, "shared", consensus_path);
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        assert!(
            errs.iter().any(|e| matches!(
                e,
                FileIoConfigError::LogicalNameConflictAcrossBuckets { logical_name, .. }
                    if logical_name == "shared"
            )),
            "pre-H-3 oracle × consensus name collision must still be caught; got {errs:?}"
        );
    }

    /// Triple-bucket collision (same name in three different buckets)
    /// must emit ONE error whose `members` lists all three entries.
    #[test]
    fn same_name_across_three_buckets_emits_single_error_with_all_members() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let f1 = root.join("f1");
        let f2 = root.join("f2");
        let d = root.join("d");
        make_file(&f1);
        make_file(&f2);
        fs::create_dir_all(&d).unwrap();
        let mut cfg = empty_cfg();
        add_oracle_file(&mut cfg, "shared", f1);
        add_consensus_file(&mut cfg, "shared", f2);
        add_oracle_dir(&mut cfg, "shared", d);
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        let conflict_errs: Vec<_> = errs
            .iter()
            .filter_map(|e| match e {
                FileIoConfigError::LogicalNameConflictAcrossBuckets {
                    logical_name,
                    members,
                } if logical_name == "shared" => Some(members),
                _ => None,
            })
            .collect();
        assert_eq!(
            conflict_errs.len(),
            1,
            "three-bucket collision must produce exactly one error"
        );
        assert_eq!(
            conflict_errs[0].len(),
            3,
            "the single error must list all three colliding members"
        );
    }

    /// M-23-8: mixed-kind same-path (oracle file + consensus dir).
    #[test]
    fn mixed_kind_same_path_oracle_file_consensus_dir_rejected() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let mut cfg = empty_cfg();
        add_oracle_file(&mut cfg, "o", root.clone());
        add_consensus_dir(&mut cfg, "c", root.clone());
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        assert!(
            errs.iter().any(|e| matches!(
                e,
                FileIoConfigError::BucketOverlapSamePath { path, .. } if path == &root
            )),
            "expected BucketOverlapSamePath for mixed kinds; got {errs:?}"
        );
    }

    /// M-23-8: mixed-kind same-path (oracle dir + consensus file).
    #[test]
    fn mixed_kind_same_path_oracle_dir_consensus_file_rejected() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let p = root.as_path().join("x.txt");
        make_file(&p);
        let mut cfg = empty_cfg();
        add_oracle_dir(&mut cfg, "o", p.clone());
        add_consensus_file(&mut cfg, "c", p.clone());
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        assert!(
            errs.iter().any(|e| matches!(
                e,
                FileIoConfigError::BucketOverlapSamePath { path, .. } if path == &p
            )),
            "expected BucketOverlapSamePath for reversed mixed kinds; got {errs:?}"
        );
    }

    /// M-23-1: error order is deterministic across runs.  HashMap
    /// iteration is randomized by default; the validator sorts
    /// entries in `flatten` and errors in the final step so cluster
    /// log-diffing works.
    #[test]
    fn error_order_is_stable_across_repeated_runs() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        // Build a multi-error config: several missing paths + several overlaps.
        let mut cfg = empty_cfg();
        for i in 0..5 {
            let p = root.as_path().join(format!("missing-{i}.txt"));
            add_oracle_file(&mut cfg, &format!("m{i}"), p);
        }
        let shared = root.as_path().join("shared.txt");
        make_file(&shared);
        for i in 0..3 {
            add_oracle_file(&mut cfg, &format!("shared-o-{i}"), shared.clone());
            add_consensus_file(&mut cfg, &format!("shared-c-{i}"), shared.clone());
        }
        let first = validate_provisioning_boot(&cfg).unwrap_err();
        for _ in 0..9 {
            let next = validate_provisioning_boot(&cfg).unwrap_err();
            assert_eq!(
                format!("{first:?}"),
                format!("{next:?}"),
                "error order must be deterministic across runs"
            );
        }
    }

    // ---------- Non-canonical path (H-23-2) ----------

    #[test]
    fn dot_component_rejected_as_non_canonical() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let p = root.as_path().join("./cfg.txt");
        let mut cfg = empty_cfg();
        add_oracle_file(&mut cfg, "cfg", p.clone());
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        assert!(
            errs.iter().any(|e| matches!(
                e,
                FileIoConfigError::NonCanonicalPath { reason, .. }
                    if reason.contains(".")
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
                    if reason.contains("..")
            )),
            "expected NonCanonicalPath for `..`; got {errs:?}"
        );
    }

    /// H-23-2: non-canonical path is EXCLUDED from downstream
    /// disjointness/tree-walk (else we'd double-report).  Verify
    /// that a non-canonical entry only surfaces `NonCanonicalPath`,
    /// not `PathNotFound` or tree-walk errors on the same entry.
    #[test]
    fn non_canonical_excluded_from_downstream_checks() {
        let mut cfg = empty_cfg();
        add_oracle_file(&mut cfg, "cfg", PathBuf::from("/etc/./cfg"));
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        assert_eq!(errs.len(), 1, "expected exactly one error; got {errs:?}");
        assert!(matches!(
            &errs[0],
            FileIoConfigError::NonCanonicalPath { .. }
        ));
    }

    // ---------- Defense-in-depth (M-23-2) ----------

    #[test]
    fn programmatic_relative_path_flagged_as_not_absolute() {
        let mut cfg = empty_cfg();
        add_oracle_file(&mut cfg, "cfg", PathBuf::from("relative/path.txt"));
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, FileIoConfigError::NotAbsolute { .. })),
            "expected NotAbsolute; got {errs:?}"
        );
    }

    #[test]
    fn programmatic_invalid_mode_flagged() {
        let mut cfg = empty_cfg();
        cfg.oracle_static_files
            .insert("cfg".into(), StaticFileEntry {
                path: PathBuf::from("/etc/cfg"),
                mode: "wx".into(), // rejected by CONFIG_FILE_MODES whitelist
            });
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        assert!(
            errs.iter().any(|e| matches!(
                e,
                FileIoConfigError::InvalidMode { mode, .. } if mode == "wx"
            )),
            "expected InvalidMode for wx; got {errs:?}"
        );
    }

    // ---------- Size-cap wiring (M-23-3) ----------

    #[test]
    fn size_limit_exceeded_surfaces_and_short_circuits_walk() {
        // Fabricate > MAX_PROVISIONING_ENTRIES via direct insertion.
        // We only need `oracle_static_files` to breach the cap.
        use crate::rust::configuration::file_io_provisioning::MAX_PROVISIONING_ENTRIES;
        let mut cfg = empty_cfg();
        for i in 0..=MAX_PROVISIONING_ENTRIES {
            cfg.oracle_static_files
                .insert(format!("k{i}"), StaticFileEntry {
                    path: PathBuf::from(format!("/tmp/cap-test-{i}")),
                    mode: "r".into(),
                });
        }
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, FileIoConfigError::SizeLimitExceeded { .. })),
            "expected SizeLimitExceeded; got first {:?}",
            errs.first()
        );
        // Confirm the walk was short-circuited: no PathNotFound errors
        // for the fabricated /tmp/cap-test-* paths.
        let path_not_found_count = errs
            .iter()
            .filter(|e| matches!(e, FileIoConfigError::PathNotFound { .. }))
            .count();
        assert_eq!(
            path_not_found_count, 0,
            "size cap must short-circuit the tree walk; got {path_not_found_count} PathNotFound"
        );
    }

    // ---------- Should-fix additions ----------

    /// S-23-2: shallowest-symlink-wins invariant with 2+ ancestors.
    #[cfg(unix)]
    #[test]
    fn shallowest_ancestor_symlink_wins_when_multiple() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let real1 = root.as_path().join("real1");
        fs::create_dir(&real1).unwrap();
        let real2 = real1.join("real2");
        fs::create_dir(&real2).unwrap();
        make_file(&real2.join("leaf.txt"));
        // link1 -> real1 (shallow)
        let link1 = root.as_path().join("link1");
        std::os::unix::fs::symlink(&real1, &link1).unwrap();
        // link2 -> real2, but placed inside real1 not link1 (both ancestors are symlinks
        // when the entry is /root/link1/link2/leaf.txt because /root/link1 is a symlink
        // AND /root/link1/link2 resolves through that symlink to /root/real1/link2,
        // which we make into a symlink too).
        let link2_inside_real1 = real1.join("link2");
        std::os::unix::fs::symlink(&real2, &link2_inside_real1).unwrap();
        let via_link = link1.join("link2").join("leaf.txt");
        let mut cfg = empty_cfg();
        add_oracle_file(&mut cfg, "cfg", via_link);
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        let sym_errs: Vec<_> = errs
            .iter()
            .filter_map(|e| match e {
                FileIoConfigError::AbsolutePrefixSymlink { prefix, .. } => Some(prefix.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            sym_errs.len(),
            1,
            "expected exactly one AbsolutePrefixSymlink (shallowest wins); got {sym_errs:?}"
        );
        assert_eq!(sym_errs[0], link1, "shallowest ancestor must win");
    }

    /// S-23-3: `flatten()` direct test — all four buckets end up in
    /// the output with correct metadata.
    #[test]
    fn flatten_covers_all_four_buckets() {
        let mut cfg = empty_cfg();
        add_oracle_file(&mut cfg, "of", PathBuf::from("/x/of"));
        add_oracle_dir(&mut cfg, "od", PathBuf::from("/x/od"));
        add_consensus_file(&mut cfg, "cf", PathBuf::from("/x/cf"));
        add_consensus_dir(&mut cfg, "cd", PathBuf::from("/x/cd"));
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

    /// S-23-3: `prefix_pair()` direct unit tests.
    #[test]
    fn prefix_pair_component_boundary_cases() {
        let a = FlatEntry {
            bucket: BUCKET_ORACLE_DIR,
            logical_name: "a",
            path: Path::new("/foo"),
            mode: "r",
            kind: EntryKind::Dir,
            is_consensus: false,
        };
        let b = FlatEntry {
            bucket: BUCKET_CONSENSUS_FILE,
            logical_name: "b",
            path: Path::new("/foo/bar/baz"),
            mode: "r",
            kind: EntryKind::File,
            is_consensus: true,
        };
        let c = FlatEntry {
            bucket: BUCKET_CONSENSUS_DIR,
            logical_name: "c",
            path: Path::new("/foobar"),
            mode: "r",
            kind: EntryKind::Dir,
            is_consensus: true,
        };
        let d = FlatEntry {
            bucket: BUCKET_ORACLE_FILE,
            logical_name: "d",
            path: Path::new("/foo"),
            mode: "r",
            kind: EntryKind::File,
            is_consensus: false,
        };
        // b is inside a → (outer=a, inner=b)
        let (outer, inner) = prefix_pair(&a, &b).expect("a is prefix of b");
        assert_eq!(outer.logical_name, "a");
        assert_eq!(inner.logical_name, "b");
        // Reversed: (b, a) → outer=a, inner=b still.
        let (outer, inner) = prefix_pair(&b, &a).expect("a is prefix of b (reversed args)");
        assert_eq!(outer.logical_name, "a");
        assert_eq!(inner.logical_name, "b");
        // String-prefix only: /foo vs /foobar → None.
        assert!(prefix_pair(&a, &c).is_none());
        // Identical paths → None (same-path is a separate error).
        assert!(prefix_pair(&a, &d).is_none());
    }

    /// S-23-4: empty walked dir passes cleanly.
    #[test]
    fn empty_walked_dir_passes() {
        let td = TempDir::new().unwrap();
        let mut cfg = empty_cfg();
        add_oracle_dir(&mut cfg, "empty", td_root(&td));
        assert!(validate_provisioning_boot(&cfg).is_ok());
    }

    /// S-23-6: broken symlink ancestor → canonical=None fallback.
    #[cfg(unix)]
    #[test]
    fn broken_ancestor_symlink_reported_with_no_canonical() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let dead = root.as_path().join("dangling");
        std::os::unix::fs::symlink(root.as_path().join("nowhere"), &dead).unwrap();
        let mut cfg = empty_cfg();
        add_oracle_file(&mut cfg, "x", dead.join("leaf.txt"));
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        assert!(
            errs.iter().any(|e| matches!(
                e,
                FileIoConfigError::AbsolutePrefixSymlink { prefix, canonical, .. }
                    if prefix == &dead && canonical.is_none()
            )),
            "expected AbsolutePrefixSymlink with canonical=None; got {errs:?}"
        );
    }

    /// S-23-7 + S-23-5: FIFO/socket → KindMismatch("other") at top
    /// level AND inside a walked dir.  Uses mkfifo via std::Command
    /// to avoid a libc/nix dev-dep.
    #[cfg(unix)]
    #[test]
    fn fifo_declared_as_file_reports_other_kind() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let fifo = root.as_path().join("f.fifo");
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo binary must be available in test env");
        if !status.success() {
            // Skip test if the platform lacks mkfifo; still counts as passing.
            return;
        }
        let mut cfg = empty_cfg();
        add_oracle_file(&mut cfg, "x", fifo);
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        assert!(
            errs.iter().any(|e| matches!(
                e,
                FileIoConfigError::KindMismatch { actual, .. }
                    if actual.contains("other")
            )),
            "expected KindMismatch(other) for FIFO; got {errs:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn fifo_inside_walked_dir_reports_other_kind() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let fifo = root.as_path().join("nested.fifo");
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo binary must be available in test env");
        if !status.success() {
            return;
        }
        let mut cfg = empty_cfg();
        add_oracle_dir(&mut cfg, "d", root.clone());
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        assert!(
            errs.iter().any(|e| matches!(
                e,
                FileIoConfigError::KindMismatch { actual, .. }
                    if actual.contains("other")
            )),
            "expected KindMismatch(other) for FIFO inside dir; got {errs:?}"
        );
    }

    /// S-23-8: Display coverage — every variant.
    #[test]
    fn every_variant_display_includes_expected_substrings() {
        let cases: Vec<(FileIoConfigError, Vec<&str>)> = vec![
            (
                FileIoConfigError::IsSymlink {
                    bucket: "b",
                    logical_name: "n".into(),
                    path: "/p".into(),
                },
                vec!["symbolic link", "b", "n", "/p"],
            ),
            (
                FileIoConfigError::AbsolutePrefixSymlink {
                    bucket: "b",
                    logical_name: "n".into(),
                    path: "/p".into(),
                    prefix: "/q".into(),
                    canonical: Some("/r".into()),
                },
                vec!["traverses symlink", "/q", "/r"],
            ),
            (
                FileIoConfigError::AbsolutePrefixSymlink {
                    bucket: "b",
                    logical_name: "n".into(),
                    path: "/p".into(),
                    prefix: "/q".into(),
                    canonical: None,
                },
                vec!["traverses symlink", "canonical resolution failed"],
            ),
            (
                FileIoConfigError::HardLinked {
                    bucket: "b",
                    logical_name: "n".into(),
                    path: "/p".into(),
                    nlink: 3,
                },
                vec!["nlink=3", "PB-M-16"],
            ),
            (
                FileIoConfigError::PathNotFound {
                    bucket: "b",
                    logical_name: "n".into(),
                    path: "/p".into(),
                },
                vec!["does not exist"],
            ),
            (
                FileIoConfigError::StatFailed {
                    bucket: "b",
                    logical_name: "n".into(),
                    path: "/p".into(),
                    error: "permission denied".into(),
                },
                vec!["could not be stat'd", "permission denied"],
            ),
            (
                FileIoConfigError::BucketOverlapSamePath {
                    path: "/p".into(),
                    bucket_a: "oa",
                    logical_name_a: "na".into(),
                    bucket_b: "cb",
                    logical_name_b: "nb".into(),
                },
                vec!["appears in both", "oa", "cb", "PB-M-16"],
            ),
            (
                FileIoConfigError::BucketOverlapPrefix {
                    outer_bucket: "ob",
                    outer_logical_name: "on".into(),
                    outer_path: "/o".into(),
                    inner_bucket: "ib",
                    inner_logical_name: "in".into(),
                    inner_path: "/o/x".into(),
                },
                vec!["contains", "cross-bucket prefix overlap"],
            ),
            (
                FileIoConfigError::KindMismatch {
                    bucket: "b",
                    logical_name: "n".into(),
                    path: "/p".into(),
                    declared: "file",
                    actual: "directory",
                },
                vec!["declared as a file", "has a directory"],
            ),
            (
                FileIoConfigError::NonCanonicalPath {
                    bucket: "b",
                    logical_name: "n".into(),
                    path: "/p".into(),
                    reason: "contains `..` component",
                },
                vec!["not lexically canonical", "`..` component"],
            ),
            (
                FileIoConfigError::NotAbsolute {
                    bucket: "b",
                    logical_name: "n".into(),
                    path: "rel/p".into(),
                },
                vec!["not an absolute path"],
            ),
            (
                FileIoConfigError::InvalidMode {
                    bucket: "b",
                    logical_name: "n".into(),
                    mode: "wx".into(),
                },
                vec!["\"wx\"", "not in the spec whitelist"],
            ),
            (
                FileIoConfigError::SizeLimitExceeded {
                    message: "cap breach".into(),
                },
                vec!["cap breach"],
            ),
            (
                FileIoConfigError::WalkLimitExceeded {
                    bucket: "b",
                    logical_name: "n".into(),
                    path: "/p".into(),
                    limit: 1_000_000,
                },
                vec!["exceeded per-boot cap", "1000000"],
            ),
        ];
        for (err, needles) in cases {
            let msg = format!("{err}");
            for n in needles {
                assert!(msg.contains(n), "variant {err:?} missing `{n}` in `{msg}`");
            }
        }
    }

    /// S-23-9: HOCON → validator integration.  Proves the schema →
    /// validator handoff doesn't break under future refactors.
    #[test]
    fn hocon_parse_then_validate_integration() {
        let text = r#"
            oracle-static-files = {
              "cfg": "/etc/does-not-exist"
            }
        "#;
        let parsed: FileIoProvisioning = hocon::HoconLoader::new()
            .load_str(text)
            .unwrap()
            .resolve()
            .unwrap();
        let errs = validate_provisioning_boot(&parsed).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, FileIoConfigError::PathNotFound { .. })),
            "expected PathNotFound from HOCON-parsed missing entry; got {errs:?}"
        );
    }

    /// S-23-10: multiple same-path bucket overlaps → all reported.
    #[test]
    fn multiple_same_path_bucket_overlaps_all_reported() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let p1 = root.as_path().join("p1.txt");
        let p2 = root.as_path().join("p2.txt");
        make_file(&p1);
        make_file(&p2);
        let mut cfg = empty_cfg();
        add_oracle_file(&mut cfg, "a-o", p1.clone());
        add_consensus_file(&mut cfg, "a-c", p1);
        add_oracle_file(&mut cfg, "b-o", p2.clone());
        add_consensus_file(&mut cfg, "b-c", p2);
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        let overlaps = errs
            .iter()
            .filter(|e| matches!(e, FileIoConfigError::BucketOverlapSamePath { .. }))
            .count();
        assert_eq!(overlaps, 2, "expected 2 overlaps; got {errs:?}");
    }

    /// S-23-11: trailing-slash disjointness — `/x` vs `/x/` normalizes
    /// (Rust's `Path` components collapse trailing slash).
    #[test]
    fn trailing_slash_detected_as_same_path_overlap() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let with_slash = format!("{}/", root.as_path().display());
        let mut cfg = empty_cfg();
        add_oracle_dir(&mut cfg, "o", root.clone());
        add_consensus_dir(&mut cfg, "c", PathBuf::from(&with_slash));
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, FileIoConfigError::BucketOverlapSamePath { .. })),
            "expected trailing-slash form to overlap with non-slash form; got {errs:?}"
        );
    }

    /// S-23-12: pin the priority — symlink-as-entry fires `IsSymlink`,
    /// not `KindMismatch`, when disk kind (via the symlink target)
    /// disagrees with the declared kind.  Regression against future
    /// refactor that moves the kind check ahead of the symlink check.
    #[cfg(unix)]
    #[test]
    fn symlink_wins_over_kind_mismatch_priority_pin() {
        let td = TempDir::new().unwrap();
        let root = td_root(&td);
        let target_dir = root.as_path().join("target");
        fs::create_dir(&target_dir).unwrap();
        let link = root.as_path().join("link");
        std::os::unix::fs::symlink(&target_dir, &link).unwrap();
        // Declare `link` as a file — via symlink, resolves to a dir.
        // The IsSymlink check must preempt KindMismatch.
        let mut cfg = empty_cfg();
        add_oracle_file(&mut cfg, "x", link);
        let errs = validate_provisioning_boot(&cfg).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, FileIoConfigError::IsSymlink { .. })),
            "expected IsSymlink to preempt KindMismatch; got {errs:?}"
        );
        assert!(
            !errs
                .iter()
                .any(|e| matches!(e, FileIoConfigError::KindMismatch { .. })),
            "KindMismatch must NOT fire on a symlink entry; got {errs:?}"
        );
    }

    /// non_canonical_reason is a helper — pin its behavior directly.
    #[test]
    fn non_canonical_reason_pin_behavior() {
        assert_eq!(non_canonical_reason(Path::new("/foo/bar")), None);
        assert_eq!(non_canonical_reason(Path::new("/")), None);
        assert!(non_canonical_reason(Path::new("/foo/./bar")).is_some());
        assert!(non_canonical_reason(Path::new("/foo/../bar")).is_some());
        // Trailing slash normalizes via Path::components — accepted.
        assert_eq!(non_canonical_reason(Path::new("/foo/bar/")), None);
        // Double slash normalizes too.
        assert_eq!(non_canonical_reason(Path::new("/foo//bar")), None);
    }
}
