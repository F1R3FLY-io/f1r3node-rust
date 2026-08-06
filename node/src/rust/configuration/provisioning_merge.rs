//! Merge CLI-provided provisioning entries into the config-file
//! surface (Phase 7 slice 24).
//!
//! Slice 22 delivered CLI parsers producing `Vec<CliStatic{File,Dir}Arg>`.
//! Slice 21 delivered the `FileIoProvisioning` config-struct.  This
//! module produces a single merged `FileIoProvisioning` from both
//! sources plus a batched `Vec<FileIoConfigError>` of merge-time
//! duplicate-detection failures.
//!
//! Merge rules (plan §362, per 2026-08-03 slice-24 review):
//!
//! - **Same logical name in both config and CLI** within a bucket:
//!   - Identical `(path, mode)` → silently deduped, no error.
//!   - Different definitions → `DuplicateLogicalNameAcrossSources`.
//!     **Hard reject (M-24-2 option c):** on conflict, the config
//!     entry is REMOVED from the merged map and the CLI entry is
//!     NOT inserted.  Boot fails on any error; the operator must
//!     resolve one source before re-running.  No silent precedence.
//!
//! - **Same logical name declared multiple times in CLI** within a
//!   bucket (slice 22 allows repetition):
//!   - Identical repeats → silently deduped.
//!   - Differing repeats → `DuplicateLogicalNameInCli`; first
//!     occurrence wins.  (Intra-source; different rule from
//!     cross-source because within one source the operator's
//!     ordering intent is meaningful.)
//!
//! - **Same absolute host path from both config and CLI** within a
//!   bucket, under different logical names → `DuplicatePathAcrossSources`.
//!   Path comparison uses lexical normalization (`Path::components`
//!   drops `.` components) so `/etc/foo` and `/etc/./foo` compare
//!   equal even before slice 23's `NonCanonicalPath` check fires.
//!   ALL CLI entries are checked, including those dropped by the
//!   logical-name hard-reject (M-24-4).
//!
//! ## TODO for slice 25
//!
//! `merge_and_validate` returns `FileIoProvisioning`.  Plan §369
//! mandates the genesis handoff bundle as
//! `Vec<(logicalName, canonPath, kind, mode, consensusMode)>`.  Slice
//! 25 must synthesize (a) `consensusMode = Consensus | Oracular` from
//! the bucket the entry lives in (`consensus-static-*` vs
//! `oracle-static-*`), (b) `kind = File | Dir` from the map type
//! (`oracle_static_files` vs `oracle_static_dirs`).  None of that
//! plumbing exists yet.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use casper::rust::genesis::contracts::fs_genesis::{
    BundleConsensusMode, BundleEntry, BundleEntryKind,
};

use super::boot_validation::{
    sort_key, validate_provisioning_boot, FileIoConfigError, BUCKET_CONSENSUS_DIR,
    BUCKET_CONSENSUS_FILE, BUCKET_ORACLE_DIR, BUCKET_ORACLE_FILE,
};
use super::commandline::cli_static_provisioning::{CliStaticDirArg, CliStaticFileArg};
use super::file_io_provisioning::{
    FileIoProvisioning, StaticDirEntry, StaticFileEntry, MAX_PROVISIONING_ENTRIES,
};

/// Lexical normalization for path *comparison* (M-24-1).  Drops `.`
/// components via `Path::components()` — Rust silently normalizes
/// those in the components iterator, so `.collect::<PathBuf>()`
/// returns a canonical spelling.  `..` components are preserved and
/// handled by slice 23's `non_canonical_reason` post-merge.  Not
/// used for insertion (we insert operator's original spelling so
/// their error messages match what they wrote), only for
/// dedup-equality comparisons.
pub(crate) fn normalize_for_compare(p: &Path) -> PathBuf {
    // Use components() to drop `.` and collapse `//` / trailing `/`.
    // Note: this preserves `..` components — slice 23's canonicity
    // check catches those separately.
    p.components().collect()
}

/// Merge CLI-parsed provisioning entries into a base `FileIoProvisioning`.
/// Returns the merged provisioning + all merge-time errors batched
/// and sorted (H-24-1: deterministic operator-facing output).
///
/// Pre-merge size cap (M-24-3): if total input entries exceed
/// `MAX_PROVISIONING_ENTRIES`, emits `SizeLimitExceeded` and skips
/// the full merge — bounds pre-boot cost against a hostile
/// unbounded-CLI-arg attacker.
///
/// The merged provisioning is always produced (never `None`) so
/// that downstream boot validation can run against it and
/// accumulate additional errors — the operator sees the full
/// picture.
pub fn merge_cli_into_config(
    mut config: FileIoProvisioning,
    cli_oracle_files: Vec<CliStaticFileArg>,
    cli_oracle_dirs: Vec<CliStaticDirArg>,
    cli_consensus_files: Vec<CliStaticFileArg>,
    cli_consensus_dirs: Vec<CliStaticDirArg>,
) -> (FileIoProvisioning, Vec<FileIoConfigError>) {
    let mut errors: Vec<FileIoConfigError> = Vec::new();

    // M-24-3: bound total cost.  Add config entries + CLI arg counts
    // (raw, before dedup).  If we're already over the cap, don't
    // spend O(N) or O(N²) walking the input — return the cap error
    // and let the caller surface it.
    let total = config.oracle_static_files.len()
        + config.oracle_static_dirs.len()
        + config.consensus_static_files.len()
        + config.consensus_static_dirs.len()
        + cli_oracle_files.len()
        + cli_oracle_dirs.len()
        + cli_consensus_files.len()
        + cli_consensus_dirs.len();
    if total > MAX_PROVISIONING_ENTRIES {
        errors.push(FileIoConfigError::SizeLimitExceeded {
            message: format!(
                "total static-provisioning entries (config + CLI) = {total}; exceeds cap of \
                 {MAX_PROVISIONING_ENTRIES}; refuse to merge to bound pre-boot cost"
            ),
        });
        return (config, errors);
    }

    merge_file_bucket(
        BUCKET_ORACLE_FILE,
        &mut config.oracle_static_files,
        cli_oracle_files,
        &mut errors,
    );
    merge_dir_bucket(
        BUCKET_ORACLE_DIR,
        &mut config.oracle_static_dirs,
        cli_oracle_dirs,
        &mut errors,
    );
    merge_file_bucket(
        BUCKET_CONSENSUS_FILE,
        &mut config.consensus_static_files,
        cli_consensus_files,
        &mut errors,
    );
    merge_dir_bucket(
        BUCKET_CONSENSUS_DIR,
        &mut config.consensus_static_dirs,
        cli_consensus_dirs,
        &mut errors,
    );

    // H-24-1: deterministic error order regardless of HashMap
    // iteration randomization.
    errors.sort_by_key(sort_key);

    (config, errors)
}

/// Top-level boot integration (slice 24 deliverable).  Merges
/// CLI-provided entries into the config, then runs
/// `validate_provisioning_boot` against the merged result.  All
/// errors from BOTH stages are combined into a single sorted
/// `Vec<FileIoConfigError>` — the operator sees every violation in
/// one boot-failure report (plan §370).  Final combined vec is
/// sorted (H-24-1) so cluster log-diffing works.
///
/// Slice 25 will call this from the actual boot pipeline and pass
/// the returned `FileIoProvisioning` to the `FsGenesis` bundle
/// constructor.  Until then, this function has no non-test callers
/// (mirrors slice 23's `validate_provisioning_boot`).
pub fn merge_and_validate(
    config: FileIoProvisioning,
    cli_oracle_files: Vec<CliStaticFileArg>,
    cli_oracle_dirs: Vec<CliStaticDirArg>,
    cli_consensus_files: Vec<CliStaticFileArg>,
    cli_consensus_dirs: Vec<CliStaticDirArg>,
) -> Result<FileIoProvisioning, Vec<FileIoConfigError>> {
    let (merged, mut errors) = merge_cli_into_config(
        config,
        cli_oracle_files,
        cli_oracle_dirs,
        cli_consensus_files,
        cli_consensus_dirs,
    );
    if let Err(mut validation_errors) = validate_provisioning_boot(&merged) {
        errors.append(&mut validation_errors);
    }
    // H-24-1: re-sort after appending validation errors.  Merge
    // errors were sorted inside merge_cli_into_config; validation
    // errors were sorted inside validate_provisioning_boot; the
    // concatenation must be re-sorted so discriminant classes
    // interleave correctly.
    errors.sort_by_key(sort_key);
    if errors.is_empty() {
        Ok(merged)
    } else {
        Err(errors)
    }
}

/// Project a merged `FileIoProvisioning` into the tuple-vec shape
/// that `casper::genesis::contracts::fs_generator` consumes (Phase 7
/// slice 25 deliverable).
///
/// Walks all four buckets, synthesizing `BundleEntryKind` from the
/// map type (File vs Dir) and preserving the operator's logical
/// name, path, mode, and (slice 26) `consensus_mode`.  The
/// `consensus_mode` field is derived from the bucket the entry
/// came from — `oracle-static-*` → `BundleConsensusMode::Oracular`,
/// `consensus-static-*` → `BundleConsensusMode::Consensus`.  Fs.rho
/// threads this through into File/Dir agent state on mint; File/
/// Dir chown/stat/entries methods forward it to the native handler.
///
/// Output is sorted by (bucket-order, logical_name) for
/// deterministic downstream Rholang composition — the composed
/// FsGenesis source must be byte-identical across every validator's
/// genesis-block computation.  Bucket order: oracle-files, oracle-
/// dirs, consensus-files, consensus-dirs.  (The final Rholang
/// serialization sorts again by logical name across all buckets,
/// so this initial ordering is defense-in-depth.)
pub fn project_bundle(cfg: &FileIoProvisioning) -> Vec<BundleEntry> {
    let mut out: Vec<BundleEntry> = Vec::with_capacity(
        cfg.oracle_static_files.len()
            + cfg.oracle_static_dirs.len()
            + cfg.consensus_static_files.len()
            + cfg.consensus_static_dirs.len(),
    );
    // H-25-3 slice-25 review fix: route through BundleEntry::try_new
    // so the projection re-validates upstream invariants (UTF-8,
    // absolute path, no forbidden chars).  If validation fires
    // here, an upstream layer (slice 21 HOCON, slice 22 CLI, slice
    // 23 boot) failed to enforce — panic rather than emit a bundle
    // that would crash the Rholang lexer or silently open a
    // different file at deploy time.
    let extend_files = |bucket: &'static str,
                        cmode: BundleConsensusMode,
                        map: &HashMap<String, StaticFileEntry>,
                        out: &mut Vec<BundleEntry>| {
        for (name, entry) in map {
            let e = BundleEntry::try_new(
                name.clone(),
                entry.path.clone(),
                BundleEntryKind::File,
                entry.mode.clone(),
                cmode,
            )
            .unwrap_or_else(|err| {
                panic!(
                    "project_bundle: BundleEntry::try_new rejected \
                     [{bucket}] `{name}`: {err}.  Upstream should have caught."
                )
            });
            out.push(e);
        }
    };
    let extend_dirs = |bucket: &'static str,
                       cmode: BundleConsensusMode,
                       map: &HashMap<String, StaticDirEntry>,
                       out: &mut Vec<BundleEntry>| {
        for (name, entry) in map {
            let e = BundleEntry::try_new(
                name.clone(),
                entry.path.clone(),
                BundleEntryKind::Dir,
                entry.mode.clone(),
                cmode,
            )
            .unwrap_or_else(|err| {
                panic!(
                    "project_bundle: BundleEntry::try_new rejected \
                     [{bucket}] `{name}`: {err}.  Upstream should have caught."
                )
            });
            out.push(e);
        }
    };
    extend_files(
        "oracle-static-files",
        BundleConsensusMode::Oracular,
        &cfg.oracle_static_files,
        &mut out,
    );
    extend_dirs(
        "oracle-static-dirs",
        BundleConsensusMode::Oracular,
        &cfg.oracle_static_dirs,
        &mut out,
    );
    extend_files(
        "consensus-static-files",
        BundleConsensusMode::Consensus,
        &cfg.consensus_static_files,
        &mut out,
    );
    extend_dirs(
        "consensus-static-dirs",
        BundleConsensusMode::Consensus,
        &cfg.consensus_static_dirs,
        &mut out,
    );
    // M-P7-4 review fix: tie-break on `consensus_mode` so entries
    // with identical `logical_name` (which upstream
    // `check_cross_source_logical_name_conflict` normally rejects
    // BEFORE this fn runs, but which could slip through if that
    // check is ever weakened) produce a deterministic order
    // independent of HashMap iteration.  A second `.then_with` on
    // canon_path adds defense-in-depth for the hypothetical
    // same-name-same-cmode case.
    out.sort_by(|a, b| {
        a.logical_name
            .cmp(&b.logical_name)
            .then_with(|| a.consensus_mode.cmp(&b.consensus_mode))
            .then_with(|| a.canon_path.cmp(&b.canon_path))
    });
    out
}

/// File-bucket merge.  Structured the same way for dirs; the two
/// helpers stay separate because `StaticFileEntry` and
/// `StaticDirEntry` are distinct types (deliberately — so a File
/// entry can't be moved into a Dir map or vice versa).
///
/// Order matters: path-dedup runs on ALL CLI entries (before
/// logical-name dedup drops anything, M-24-4) so that a CLI entry
/// which loses on name still surfaces a path-collision error if
/// its path also collided with a config-side alias.
fn merge_file_bucket(
    bucket: &'static str,
    config_map: &mut HashMap<String, StaticFileEntry>,
    cli_args: Vec<CliStaticFileArg>,
    errors: &mut Vec<FileIoConfigError>,
) {
    // Step 1: intra-CLI dedup.  Group by logical_name.  Keep first
    // occurrence; emit DuplicateLogicalNameInCli on differing repeats.
    let cli_dedup = intra_cli_dedup_files(bucket, cli_args, errors);

    // Step 2: cross-source path dedup on the FULL post-intra-CLI
    // map (before logical-name hard-reject drops entries).  This is
    // the M-24-4 fix: a CLI entry that will later lose on logical
    // name still gets its path-collision reported.
    cross_source_path_dedup_files_map(bucket, config_map, &cli_dedup, errors);

    // Step 3: cross-source logical-name dedup with HARD REJECT
    // (M-24-2 option c): on conflict, remove the config entry from
    // the merged map and don't insert the CLI entry.  No silent
    // precedence; boot fails on any error.
    let cli_to_insert =
        cross_source_logical_name_dedup_files(bucket, config_map, cli_dedup, errors);

    // Step 4: fold surviving CLI entries into the config map.
    for (name, entry) in cli_to_insert {
        config_map.insert(name, entry);
    }
}

fn merge_dir_bucket(
    bucket: &'static str,
    config_map: &mut HashMap<String, StaticDirEntry>,
    cli_args: Vec<CliStaticDirArg>,
    errors: &mut Vec<FileIoConfigError>,
) {
    let cli_dedup = intra_cli_dedup_dirs(bucket, cli_args, errors);
    cross_source_path_dedup_dirs_map(bucket, config_map, &cli_dedup, errors);
    let cli_to_insert = cross_source_logical_name_dedup_dirs(bucket, config_map, cli_dedup, errors);
    for (name, entry) in cli_to_insert {
        config_map.insert(name, entry);
    }
}

// -------------------- intra-CLI dedup --------------------

fn intra_cli_dedup_files(
    bucket: &'static str,
    args: Vec<CliStaticFileArg>,
    errors: &mut Vec<FileIoConfigError>,
) -> HashMap<String, StaticFileEntry> {
    let mut out: HashMap<String, StaticFileEntry> = HashMap::new();
    let mut counts: HashMap<String, usize> = HashMap::new();
    // Track which names have conflicting entries; only emit once
    // per conflicting name (N-24-2: HashSet is the right shape).
    let mut reported: HashSet<String> = HashSet::new();
    for arg in args {
        let CliStaticFileArg {
            logical_name,
            entry,
        } = arg;
        // SAFETY invariant: counts.entry(name).or_insert(0) creates
        // an entry if absent, so counts.get(&name) is guaranteed
        // Some (N-24-3).  Keep this line adjacent to the get() below
        // — any refactor that separates them must preserve the
        // invariant or replace unwrap with expect.
        *counts.entry(logical_name.clone()).or_insert(0) += 1;
        match out.get(&logical_name) {
            None => {
                out.insert(logical_name, entry);
            }
            Some(existing) if *existing == entry => {
                // Identical repeat — silently deduped.
            }
            Some(_) => {
                if reported.insert(logical_name.clone()) {
                    errors.push(FileIoConfigError::DuplicateLogicalNameInCli {
                        bucket,
                        logical_name: logical_name.clone(),
                        // SAFETY: counts.entry above guaranteed presence.
                        count: *counts.get(&logical_name).unwrap(),
                    });
                }
                // First-wins: do not overwrite `out`.
            }
        }
    }
    // For conflicting names, the count in the error is the count at
    // detection time.  Update to the final count for correctness.
    // Bucket-scoped: only mutate errors emitted by THIS call.
    for e in errors.iter_mut() {
        if let FileIoConfigError::DuplicateLogicalNameInCli {
            bucket: b,
            logical_name,
            count,
        } = e
        {
            if *b == bucket {
                if let Some(final_count) = counts.get(logical_name) {
                    *count = *final_count;
                }
            }
        }
    }
    out
}

fn intra_cli_dedup_dirs(
    bucket: &'static str,
    args: Vec<CliStaticDirArg>,
    errors: &mut Vec<FileIoConfigError>,
) -> HashMap<String, StaticDirEntry> {
    let mut out: HashMap<String, StaticDirEntry> = HashMap::new();
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut reported: HashSet<String> = HashSet::new();
    for arg in args {
        let CliStaticDirArg {
            logical_name,
            entry,
        } = arg;
        // SAFETY: see intra_cli_dedup_files.
        *counts.entry(logical_name.clone()).or_insert(0) += 1;
        match out.get(&logical_name) {
            None => {
                out.insert(logical_name, entry);
            }
            Some(existing) if *existing == entry => {}
            Some(_) => {
                if reported.insert(logical_name.clone()) {
                    errors.push(FileIoConfigError::DuplicateLogicalNameInCli {
                        bucket,
                        logical_name: logical_name.clone(),
                        // SAFETY: counts.entry above guaranteed presence.
                        count: *counts.get(&logical_name).unwrap(),
                    });
                }
            }
        }
    }
    for e in errors.iter_mut() {
        if let FileIoConfigError::DuplicateLogicalNameInCli {
            bucket: b,
            logical_name,
            count,
        } = e
        {
            if *b == bucket {
                if let Some(final_count) = counts.get(logical_name) {
                    *count = *final_count;
                }
            }
        }
    }
    out
}

// -------------------- cross-source logical-name dedup --------------------

/// Hard reject on conflict (M-24-2 option c).  Returns the map of
/// CLI entries to insert into `config` (i.e., CLI-only entries that
/// don't collide with any config entry, plus silently-deduped
/// identical entries which don't need re-insertion).
///
/// On `DuplicateLogicalNameAcrossSources`: emit the error AND
/// REMOVE the config entry from `config` so neither side survives
/// in the merged map.  Rationale: boot fails on any error; leaving
/// the config entry in place would silently privilege one source,
/// and slice 24 doesn't have a defensible reason to pick either.
/// If the operator resolves by removing one source, the surviving
/// entry gets inserted normally on the next run.
fn cross_source_logical_name_dedup_files(
    bucket: &'static str,
    config: &mut HashMap<String, StaticFileEntry>,
    cli: HashMap<String, StaticFileEntry>,
    errors: &mut Vec<FileIoConfigError>,
) -> HashMap<String, StaticFileEntry> {
    let mut to_insert: HashMap<String, StaticFileEntry> = HashMap::new();
    // Collect conflicts first, then mutate — avoids borrow-checker
    // issues from mutating `config` while iterating errors.
    let mut to_remove: Vec<String> = Vec::new();
    for (name, cli_entry) in cli {
        match config.get(&name) {
            None => {
                to_insert.insert(name, cli_entry);
            }
            Some(cfg_entry) if *cfg_entry == cli_entry => {
                // Identical — silent dedup, config already holds it.
            }
            Some(cfg_entry) => {
                errors.push(FileIoConfigError::DuplicateLogicalNameAcrossSources {
                    bucket,
                    logical_name: name.clone(),
                    config_path: cfg_entry.path.clone(),
                    config_mode: cfg_entry.mode.clone(),
                    cli_path: cli_entry.path.clone(),
                    cli_mode: cli_entry.mode.clone(),
                });
                to_remove.push(name); // hard reject: remove config entry too
            }
        }
    }
    for name in to_remove {
        config.remove(&name);
    }
    to_insert
}

fn cross_source_logical_name_dedup_dirs(
    bucket: &'static str,
    config: &mut HashMap<String, StaticDirEntry>,
    cli: HashMap<String, StaticDirEntry>,
    errors: &mut Vec<FileIoConfigError>,
) -> HashMap<String, StaticDirEntry> {
    let mut to_insert: HashMap<String, StaticDirEntry> = HashMap::new();
    let mut to_remove: Vec<String> = Vec::new();
    for (name, cli_entry) in cli {
        match config.get(&name) {
            None => {
                to_insert.insert(name, cli_entry);
            }
            Some(cfg_entry) if *cfg_entry == cli_entry => {}
            Some(cfg_entry) => {
                errors.push(FileIoConfigError::DuplicateLogicalNameAcrossSources {
                    bucket,
                    logical_name: name.clone(),
                    config_path: cfg_entry.path.clone(),
                    config_mode: cfg_entry.mode.clone(),
                    cli_path: cli_entry.path.clone(),
                    cli_mode: cli_entry.mode.clone(),
                });
                to_remove.push(name);
            }
        }
    }
    for name in to_remove {
        config.remove(&name);
    }
    to_insert
}

// -------------------- cross-source path dedup --------------------

/// M-24-5: O(N+M) path-collision check using a HashMap keyed on
/// LEXICALLY NORMALIZED paths (M-24-1 canonicalization asymmetry
/// fix).  Uses `normalize_for_compare` so `/etc/foo` and
/// `/etc/./foo` compare equal even before slice 23's
/// `NonCanonicalPath` fires.
///
/// M-24-4: takes the FULL post-intra-CLI dedup map (not just
/// cli_to_insert), so path collisions on entries that will later
/// lose the logical-name hard-reject are also reported.
fn cross_source_path_dedup_files_map(
    bucket: &'static str,
    config: &HashMap<String, StaticFileEntry>,
    cli: &HashMap<String, StaticFileEntry>,
    errors: &mut Vec<FileIoConfigError>,
) {
    // Build config-side index once: normalized_path -> config_logical_name.
    let cfg_by_path: HashMap<PathBuf, &String> = config
        .iter()
        .map(|(name, entry)| (normalize_for_compare(entry.path.as_path()), name))
        .collect();
    // Deterministic emit order: iterate CLI entries in name order.
    let mut cli_names: Vec<&String> = cli.keys().collect();
    cli_names.sort();
    for cli_name in cli_names {
        let cli_entry = &cli[cli_name];
        let key = normalize_for_compare(cli_entry.path.as_path());
        if let Some(cfg_name) = cfg_by_path.get(&key) {
            if cfg_name.as_str() != cli_name.as_str() {
                errors.push(FileIoConfigError::DuplicatePathAcrossSources {
                    bucket,
                    path: cli_entry.path.clone(),
                    config_logical_name: (*cfg_name).clone(),
                    cli_logical_name: cli_name.clone(),
                });
            }
        }
    }
}

fn cross_source_path_dedup_dirs_map(
    bucket: &'static str,
    config: &HashMap<String, StaticDirEntry>,
    cli: &HashMap<String, StaticDirEntry>,
    errors: &mut Vec<FileIoConfigError>,
) {
    let cfg_by_path: HashMap<PathBuf, &String> = config
        .iter()
        .map(|(name, entry)| (normalize_for_compare(entry.path.as_path()), name))
        .collect();
    let mut cli_names: Vec<&String> = cli.keys().collect();
    cli_names.sort();
    for cli_name in cli_names {
        let cli_entry = &cli[cli_name];
        let key = normalize_for_compare(cli_entry.path.as_path());
        if let Some(cfg_name) = cfg_by_path.get(&key) {
            if cfg_name.as_str() != cli_name.as_str() {
                errors.push(FileIoConfigError::DuplicatePathAcrossSources {
                    bucket,
                    path: cli_entry.path.clone(),
                    config_logical_name: (*cfg_name).clone(),
                    cli_logical_name: cli_name.clone(),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    // -------------------- helpers --------------------

    fn empty_cfg() -> FileIoProvisioning { FileIoProvisioning::default() }

    fn cli_file(name: &str, path: &str, mode: &str) -> CliStaticFileArg {
        CliStaticFileArg {
            logical_name: name.into(),
            entry: StaticFileEntry {
                path: PathBuf::from(path),
                mode: mode.into(),
            },
        }
    }

    fn cli_dir(name: &str, path: &str, mode: &str) -> CliStaticDirArg {
        CliStaticDirArg {
            logical_name: name.into(),
            entry: StaticDirEntry {
                path: PathBuf::from(path),
                mode: mode.into(),
            },
        }
    }

    fn cfg_with_oracle_file(name: &str, path: &str, mode: &str) -> FileIoProvisioning {
        let mut cfg = empty_cfg();
        cfg.oracle_static_files
            .insert(name.into(), StaticFileEntry {
                path: PathBuf::from(path),
                mode: mode.into(),
            });
        cfg
    }

    // -------------------- happy paths --------------------

    #[test]
    fn empty_inputs_yield_empty_output_no_errors() {
        let (merged, errs) = merge_cli_into_config(empty_cfg(), vec![], vec![], vec![], vec![]);
        assert!(errs.is_empty());
        assert!(merged.oracle_static_files.is_empty());
        assert!(merged.oracle_static_dirs.is_empty());
        assert!(merged.consensus_static_files.is_empty());
        assert!(merged.consensus_static_dirs.is_empty());
    }

    #[test]
    fn cli_only_populates_all_four_buckets() {
        let (merged, errs) = merge_cli_into_config(
            empty_cfg(),
            vec![cli_file("of", "/x/of", "r")],
            vec![cli_dir("od", "/x/od", "rw")],
            vec![cli_file("cf", "/x/cf", "r+")],
            vec![cli_dir("cd", "/x/cd", "r")],
        );
        assert!(errs.is_empty());
        assert_eq!(merged.oracle_static_files.len(), 1);
        assert_eq!(merged.oracle_static_dirs.len(), 1);
        assert_eq!(merged.consensus_static_files.len(), 1);
        assert_eq!(merged.consensus_static_dirs.len(), 1);
        assert_eq!(
            merged.oracle_static_files["of"].path,
            PathBuf::from("/x/of")
        );
    }

    #[test]
    fn config_only_passes_through_untouched() {
        let cfg = cfg_with_oracle_file("keep", "/etc/keep", "r");
        let (merged, errs) = merge_cli_into_config(cfg, vec![], vec![], vec![], vec![]);
        assert!(errs.is_empty());
        assert_eq!(merged.oracle_static_files.len(), 1);
        assert_eq!(
            merged.oracle_static_files["keep"].path,
            PathBuf::from("/etc/keep")
        );
    }

    #[test]
    fn config_and_cli_distinct_names_both_present() {
        let cfg = cfg_with_oracle_file("cfg-only", "/etc/cfg", "r");
        let (merged, errs) = merge_cli_into_config(
            cfg,
            vec![cli_file("cli-only", "/etc/cli", "r")],
            vec![],
            vec![],
            vec![],
        );
        assert!(errs.is_empty());
        assert_eq!(merged.oracle_static_files.len(), 2);
        assert!(merged.oracle_static_files.contains_key("cfg-only"));
        assert!(merged.oracle_static_files.contains_key("cli-only"));
    }

    // -------------------- intra-CLI dedup --------------------

    #[test]
    fn intra_cli_identical_repeat_is_silent() {
        let (merged, errs) = merge_cli_into_config(
            empty_cfg(),
            vec![
                cli_file("dup", "/etc/x", "r"),
                cli_file("dup", "/etc/x", "r"),
            ],
            vec![],
            vec![],
            vec![],
        );
        assert!(errs.is_empty(), "identical repeat must be silent: {errs:?}");
        assert_eq!(merged.oracle_static_files.len(), 1);
    }

    #[test]
    fn intra_cli_conflicting_repeat_flagged_once() {
        let (merged, errs) = merge_cli_into_config(
            empty_cfg(),
            vec![
                cli_file("dup", "/etc/a", "r"),
                cli_file("dup", "/etc/b", "r"),
                cli_file("dup", "/etc/c", "r+"),
            ],
            vec![],
            vec![],
            vec![],
        );
        let dupe_errs: Vec<_> = errs
            .iter()
            .filter(|e| matches!(e, FileIoConfigError::DuplicateLogicalNameInCli { .. }))
            .collect();
        assert_eq!(
            dupe_errs.len(),
            1,
            "one error per conflicting name; got {errs:?}"
        );
        if let FileIoConfigError::DuplicateLogicalNameInCli {
            logical_name,
            count,
            ..
        } = dupe_errs[0]
        {
            assert_eq!(logical_name, "dup");
            assert_eq!(*count, 3);
        }
        // First occurrence wins.
        assert_eq!(
            merged.oracle_static_files["dup"].path,
            PathBuf::from("/etc/a")
        );
    }

    #[test]
    fn intra_cli_dedup_handles_mixed_dupes_and_uniques() {
        let (merged, errs) = merge_cli_into_config(
            empty_cfg(),
            vec![
                cli_file("a", "/etc/a", "r"),
                cli_file("b", "/etc/b", "r"),
                cli_file("a", "/etc/a", "r"), // silent
                cli_file("c", "/etc/c1", "r"),
                cli_file("c", "/etc/c2", "r"), // conflict
            ],
            vec![],
            vec![],
            vec![],
        );
        assert_eq!(errs.len(), 1, "only c is conflicting; got {errs:?}");
        assert_eq!(merged.oracle_static_files.len(), 3);
    }

    // -------------------- cross-source logical-name dedup --------------------

    #[test]
    fn config_and_cli_same_name_identical_entries_silent() {
        let cfg = cfg_with_oracle_file("shared", "/etc/x", "r");
        let (merged, errs) = merge_cli_into_config(
            cfg,
            vec![cli_file("shared", "/etc/x", "r")],
            vec![],
            vec![],
            vec![],
        );
        assert!(
            errs.is_empty(),
            "identical cross-source must be silent: {errs:?}"
        );
        assert_eq!(merged.oracle_static_files.len(), 1);
    }

    /// M-24-2 (option c) hard reject: on
    /// DuplicateLogicalNameAcrossSources, BOTH the config and CLI
    /// entries are removed from the merged map.  No silent
    /// precedence; boot fails, operator must resolve.
    #[test]
    fn config_and_cli_same_name_different_path_flagged_and_removed() {
        let cfg = cfg_with_oracle_file("conflict", "/etc/cfg-path", "r");
        let (merged, errs) = merge_cli_into_config(
            cfg,
            vec![cli_file("conflict", "/etc/cli-path", "r")],
            vec![],
            vec![],
            vec![],
        );
        assert!(errs.iter().any(|e| matches!(
            e,
            FileIoConfigError::DuplicateLogicalNameAcrossSources {
                logical_name,
                config_path,
                cli_path,
                ..
            } if logical_name == "conflict"
                && config_path == &PathBuf::from("/etc/cfg-path")
                && cli_path == &PathBuf::from("/etc/cli-path")
        )));
        // Hard reject: entry is REMOVED from the merged map so
        // neither side silently wins.
        assert!(
            !merged.oracle_static_files.contains_key("conflict"),
            "hard-reject: entry must be removed from merged map; got {:?}",
            merged.oracle_static_files
        );
    }

    #[test]
    fn config_and_cli_same_name_different_mode_flagged_and_removed() {
        let cfg = cfg_with_oracle_file("conflict", "/etc/x", "r");
        let (merged, errs) = merge_cli_into_config(
            cfg,
            vec![cli_file("conflict", "/etc/x", "r+")],
            vec![],
            vec![],
            vec![],
        );
        assert!(errs.iter().any(|e| matches!(
            e,
            FileIoConfigError::DuplicateLogicalNameAcrossSources {
                config_mode,
                cli_mode,
                ..
            } if config_mode == "r" && cli_mode == "r+"
        )));
        assert!(!merged.oracle_static_files.contains_key("conflict"));
    }

    // -------------------- cross-source path dedup --------------------

    #[test]
    fn same_path_different_names_config_and_cli_flagged() {
        let cfg = cfg_with_oracle_file("cfg-name", "/etc/shared", "r");
        let (_merged, errs) = merge_cli_into_config(
            cfg,
            vec![cli_file("cli-name", "/etc/shared", "r")],
            vec![],
            vec![],
            vec![],
        );
        assert!(
            errs.iter().any(|e| matches!(
                e,
                FileIoConfigError::DuplicatePathAcrossSources {
                    path,
                    config_logical_name,
                    cli_logical_name,
                    ..
                } if path == &PathBuf::from("/etc/shared")
                    && config_logical_name == "cfg-name"
                    && cli_logical_name == "cli-name"
            )),
            "expected DuplicatePathAcrossSources; got {errs:?}"
        );
    }

    /// Same path in config + CLI under the SAME logical name is
    /// caught by the logical-name dedup (silent if entries match,
    /// DuplicateLogicalNameAcrossSources if they differ); it must
    /// NOT also trigger DuplicatePathAcrossSources.
    #[test]
    fn same_path_same_name_does_not_double_report() {
        let cfg = cfg_with_oracle_file("name", "/etc/x", "r");
        let (_merged, errs) = merge_cli_into_config(
            cfg,
            vec![cli_file("name", "/etc/x", "r")],
            vec![],
            vec![],
            vec![],
        );
        assert!(
            errs.is_empty(),
            "identical same-name entry must be silent: {errs:?}"
        );
    }

    /// Same path within a single bucket, different logical names,
    /// BOTH from CLI (not cross-source) — plan §362 doesn't flag
    /// this; only cross-source is a merge concern.  Aliasing intent
    /// within one source is allowed.
    #[test]
    fn cli_only_aliasing_within_same_bucket_not_flagged() {
        let (_merged, errs) = merge_cli_into_config(
            empty_cfg(),
            vec![
                cli_file("a", "/etc/shared", "r"),
                cli_file("b", "/etc/shared", "r"),
            ],
            vec![],
            vec![],
            vec![],
        );
        assert!(
            errs.is_empty(),
            "single-source aliasing not a merge concern: {errs:?}"
        );
    }

    // -------------------- bucket disjointness (dir surface) --------------------

    #[test]
    fn dir_bucket_intra_cli_conflict_flagged() {
        let (_merged, errs) = merge_cli_into_config(
            empty_cfg(),
            vec![],
            vec![cli_dir("d", "/etc/d1", "rw"), cli_dir("d", "/etc/d2", "rw")],
            vec![],
            vec![],
        );
        assert!(errs.iter().any(|e| matches!(
            e,
            FileIoConfigError::DuplicateLogicalNameInCli { bucket, .. }
                if *bucket == BUCKET_ORACLE_DIR
        )));
    }

    #[test]
    fn dir_bucket_cross_source_name_conflict_flagged() {
        let mut cfg = empty_cfg();
        cfg.oracle_static_dirs.insert("d".into(), StaticDirEntry {
            path: PathBuf::from("/etc/d-cfg"),
            mode: "r".into(),
        });
        let (_merged, errs) = merge_cli_into_config(
            cfg,
            vec![],
            vec![cli_dir("d", "/etc/d-cli", "rw")],
            vec![],
            vec![],
        );
        assert!(errs.iter().any(|e| matches!(
            e,
            FileIoConfigError::DuplicateLogicalNameAcrossSources { bucket, .. }
                if *bucket == BUCKET_ORACLE_DIR
        )));
    }

    // -------------------- bucket isolation --------------------

    /// A logical name shared across buckets (e.g., same name in
    /// oracle-files and consensus-files) is legal — different
    /// buckets have separate namespaces.  Slice 23's bucket
    /// disjointness catches the underlying-path conflict; slice 24
    /// merges each bucket independently.
    #[test]
    fn same_logical_name_different_buckets_not_a_merge_conflict() {
        let mut cfg = empty_cfg();
        cfg.oracle_static_files.insert("n".into(), StaticFileEntry {
            path: PathBuf::from("/etc/o"),
            mode: "r".into(),
        });
        let (merged, errs) = merge_cli_into_config(
            cfg,
            vec![],
            vec![],
            vec![cli_file("n", "/etc/c", "r")],
            vec![],
        );
        assert!(
            errs.is_empty(),
            "cross-bucket same-name is not a merge concern: {errs:?}"
        );
        assert!(merged.oracle_static_files.contains_key("n"));
        assert!(merged.consensus_static_files.contains_key("n"));
    }

    // -------------------- error batching --------------------

    #[test]
    fn multiple_conflicts_batched_across_buckets() {
        let mut cfg = empty_cfg();
        cfg.oracle_static_files
            .insert("of".into(), StaticFileEntry {
                path: PathBuf::from("/etc/of-cfg"),
                mode: "r".into(),
            });
        cfg.consensus_static_dirs
            .insert("cd".into(), StaticDirEntry {
                path: PathBuf::from("/etc/cd-cfg"),
                mode: "r".into(),
            });
        let (_merged, errs) = merge_cli_into_config(
            cfg,
            vec![cli_file("of", "/etc/of-cli", "r")], // cross-source name conflict
            vec![
                cli_dir("od", "/etc/od-1", "rw"),
                cli_dir("od", "/etc/od-2", "rw"), // intra-cli conflict
            ],
            vec![],
            vec![cli_dir("cd", "/etc/cd-cli", "r")], // cross-source name conflict
        );
        let name_conflicts = errs
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    FileIoConfigError::DuplicateLogicalNameAcrossSources { .. }
                )
            })
            .count();
        let intra_cli = errs
            .iter()
            .filter(|e| matches!(e, FileIoConfigError::DuplicateLogicalNameInCli { .. }))
            .count();
        assert_eq!(
            name_conflicts, 2,
            "two cross-source conflicts; got {errs:?}"
        );
        assert_eq!(intra_cli, 1, "one intra-CLI conflict; got {errs:?}");
    }

    // -------------------- Display / message coverage --------------------

    #[test]
    fn duplicate_logical_name_across_sources_error_message() {
        let e = FileIoConfigError::DuplicateLogicalNameAcrossSources {
            bucket: BUCKET_ORACLE_FILE,
            logical_name: "n".into(),
            config_path: PathBuf::from("/c"),
            config_mode: "r".into(),
            cli_path: PathBuf::from("/l"),
            cli_mode: "r+".into(),
        };
        let msg = format!("{e}");
        assert!(msg.contains("oracle-static-files"));
        assert!(msg.contains("`n`"));
        assert!(msg.contains("config") && msg.contains("CLI"));
        assert!(msg.contains("/c") && msg.contains("/l"));
        assert!(msg.contains("\"r\"") && msg.contains("\"r+\""));
    }

    #[test]
    fn duplicate_logical_name_in_cli_error_message() {
        let e = FileIoConfigError::DuplicateLogicalNameInCli {
            bucket: BUCKET_CONSENSUS_DIR,
            logical_name: "d".into(),
            count: 5,
        };
        let msg = format!("{e}");
        assert!(msg.contains("consensus-static-dirs"));
        assert!(msg.contains("`d`"));
        assert!(msg.contains("5 times"));
    }

    #[test]
    fn duplicate_path_across_sources_error_message() {
        let e = FileIoConfigError::DuplicatePathAcrossSources {
            bucket: BUCKET_ORACLE_DIR,
            path: PathBuf::from("/etc/shared"),
            config_logical_name: "cfg-alias".into(),
            cli_logical_name: "cli-alias".into(),
        };
        let msg = format!("{e}");
        assert!(msg.contains("oracle-static-dirs"));
        assert!(msg.contains("/etc/shared"));
        assert!(msg.contains("cfg-alias") && msg.contains("cli-alias"));
    }

    // -------------------- merge_and_validate integration --------------------

    #[test]
    fn merge_and_validate_returns_ok_on_clean_input() {
        // Empty in, empty out.  boot_validation is a no-op on
        // empty provisioning (no paths to walk).
        let res = merge_and_validate(empty_cfg(), vec![], vec![], vec![], vec![]);
        let merged = res.expect("empty merge+validate must succeed");
        assert!(merged.oracle_static_files.is_empty());
    }

    #[test]
    fn merge_and_validate_combines_merge_and_validation_errors() {
        // Two distinct issues:
        //  (a) merge error: logical name "k" conflicts across
        //      config and CLI with different modes.  Under hard
        //      reject the "k" entry is dropped from the merged map.
        //  (b) validation error: a SEPARATE logical name "m"
        //      declared in config with a missing path.  Survives
        //      the merge and is caught by boot validation.
        let mut cfg = cfg_with_oracle_file("k", "/etc/k", "r");
        cfg.oracle_static_files.insert("m".into(), StaticFileEntry {
            path: PathBuf::from("/does/not/exist"),
            mode: "r".into(),
        });
        let errs = merge_and_validate(
            cfg,
            vec![cli_file("k", "/etc/k", "r+")],
            vec![],
            vec![],
            vec![],
        )
        .expect_err("both merge and validation errors should surface");
        let has_merge_err = errs.iter().any(|e| {
            matches!(
                e,
                FileIoConfigError::DuplicateLogicalNameAcrossSources { .. }
            )
        });
        let has_validation_err = errs
            .iter()
            .any(|e| matches!(e, FileIoConfigError::PathNotFound { .. }));
        assert!(
            has_merge_err && has_validation_err,
            "expected both merge + validation errors; got {errs:?}"
        );
    }

    #[test]
    fn merge_and_validate_returns_merged_ok_when_valid() {
        // Use a canonical existing path (temp dir).
        let td = tempfile::TempDir::new().unwrap();
        let root = std::fs::canonicalize(td.path()).unwrap();
        let f = root.join("cfg.json");
        std::fs::write(&f, b"x").unwrap();

        let cfg = empty_cfg();
        let cli_arg = CliStaticFileArg {
            logical_name: "cfg".into(),
            entry: StaticFileEntry {
                path: f.clone(),
                mode: "r".into(),
            },
        };
        let merged = merge_and_validate(cfg, vec![cli_arg], vec![], vec![], vec![])
            .expect("valid merge + valid file should return Ok");
        assert_eq!(merged.oracle_static_files.len(), 1);
        assert_eq!(merged.oracle_static_files["cfg"].path, f);
    }

    // ==================================================================
    // Review-driven additions (2026-08-03 slice 24 review)
    // ==================================================================

    // -------------------- MT-24-1: ordering determinism --------------------

    /// H-24-1: repeatedly calling merge_cli_into_config on the same
    /// multi-conflict input must produce byte-identical error Vec
    /// across N runs.  Without the final sort_by_key, HashMap
    /// iteration randomization would produce differently-ordered
    /// output between runs.
    #[test]
    fn merge_cli_into_config_error_order_is_stable_across_runs() {
        let build = || {
            let mut cfg = empty_cfg();
            for i in 0..5 {
                cfg.oracle_static_files
                    .insert(format!("k{i}"), StaticFileEntry {
                        path: PathBuf::from(format!("/etc/cfg-{i}")),
                        mode: "r".into(),
                    });
            }
            merge_cli_into_config(
                cfg,
                (0..5)
                    .map(|i| cli_file(&format!("k{i}"), &format!("/etc/cli-{i}"), "r"))
                    .collect(),
                vec![],
                vec![],
                vec![],
            )
            .1
        };
        let baseline = build();
        assert!(!baseline.is_empty());
        for _ in 0..50 {
            let next = build();
            assert_eq!(
                format!("{baseline:?}"),
                format!("{next:?}"),
                "merge_cli_into_config error order must be deterministic"
            );
        }
    }

    // -------------------- MT-24-2: sort_key covers new variants --------------------

    /// Ensure the sort_key discriminant integers for the three new
    /// variants (13, 14, 15) sort in the expected order relative to
    /// existing variants.
    #[test]
    fn new_variants_sort_after_existing_variants() {
        use crate::rust::configuration::boot_validation::sort_key as sk;
        // PathNotFound (discriminant 6) < DuplicateLogicalNameAcrossSources (13)
        //                              < DuplicateLogicalNameInCli (14)
        //                              < DuplicatePathAcrossSources (15).
        let mut errs = [
            FileIoConfigError::DuplicatePathAcrossSources {
                bucket: BUCKET_ORACLE_FILE,
                path: PathBuf::from("/x"),
                config_logical_name: "a".into(),
                cli_logical_name: "b".into(),
            },
            FileIoConfigError::DuplicateLogicalNameAcrossSources {
                bucket: BUCKET_ORACLE_FILE,
                logical_name: "z".into(),
                config_path: PathBuf::from("/x"),
                config_mode: "r".into(),
                cli_path: PathBuf::from("/x"),
                cli_mode: "r+".into(),
            },
            FileIoConfigError::PathNotFound {
                bucket: BUCKET_ORACLE_FILE,
                logical_name: "y".into(),
                path: PathBuf::from("/x"),
            },
            FileIoConfigError::DuplicateLogicalNameInCli {
                bucket: BUCKET_ORACLE_FILE,
                logical_name: "w".into(),
                count: 2,
            },
        ];
        errs.sort_by_key(sk);
        assert!(matches!(errs[0], FileIoConfigError::PathNotFound { .. }));
        assert!(matches!(
            errs[1],
            FileIoConfigError::DuplicateLogicalNameAcrossSources { .. }
        ));
        assert!(matches!(
            errs[2],
            FileIoConfigError::DuplicateLogicalNameInCli { .. }
        ));
        assert!(matches!(
            errs[3],
            FileIoConfigError::DuplicatePathAcrossSources { .. }
        ));
    }

    // -------------------- MT-24-3: merge_and_validate fully sorted --------------------

    /// H-24-1: merge_and_validate's combined error vec must be
    /// fully sorted — merge-error discriminant classes must
    /// interleave correctly with validation-error classes.
    #[test]
    fn merge_and_validate_combined_errors_fully_sorted() {
        use crate::rust::configuration::boot_validation::sort_key as sk;
        // Build a config with one missing path (validation error,
        // discriminant 6) AND a name conflict (merge error, 13).
        let mut cfg = cfg_with_oracle_file("k", "/etc/k", "r");
        cfg.oracle_static_files.insert("m".into(), StaticFileEntry {
            path: PathBuf::from("/does/not/exist-A"),
            mode: "r".into(),
        });
        cfg.oracle_static_files.insert("n".into(), StaticFileEntry {
            path: PathBuf::from("/does/not/exist-B"),
            mode: "r".into(),
        });
        let errs = merge_and_validate(
            cfg,
            vec![cli_file("k", "/etc/k", "r+")],
            vec![],
            vec![],
            vec![],
        )
        .expect_err("expected multiple errors");
        // Verify: the returned vec is sorted by sort_key.
        let keys: Vec<_> = errs.iter().map(sk).collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(
            keys, sorted,
            "merge_and_validate returned unsorted errors: {errs:?}"
        );
    }

    // -------------------- M-24-3: pre-merge size cap --------------------

    #[test]
    fn size_cap_short_circuits_before_merge() {
        use crate::rust::configuration::file_io_provisioning::MAX_PROVISIONING_ENTRIES;
        // Build a config just above the cap.
        let mut cfg = empty_cfg();
        for i in 0..=MAX_PROVISIONING_ENTRIES {
            cfg.oracle_static_files
                .insert(format!("k{i}"), StaticFileEntry {
                    path: PathBuf::from(format!("/etc/{i}")),
                    mode: "r".into(),
                });
        }
        // Add one CLI arg that would also conflict, to prove merge
        // doesn't run.
        let cli = vec![cli_file("k0", "/etc/other", "r+")];
        let (_, errs) = merge_cli_into_config(cfg, cli, vec![], vec![], vec![]);
        assert!(
            errs.iter()
                .any(|e| matches!(e, FileIoConfigError::SizeLimitExceeded { .. })),
            "expected SizeLimitExceeded; got {errs:?}"
        );
        // Merge was short-circuited: no DuplicateLogicalNameAcrossSources.
        assert!(
            !errs.iter().any(|e| matches!(
                e,
                FileIoConfigError::DuplicateLogicalNameAcrossSources { .. }
            )),
            "merge must be short-circuited by size cap; got {errs:?}"
        );
    }

    // -------------------- M-24-1: canonicalization equivalence --------------------

    /// `/etc/foo` (config) and `/etc/./foo` (CLI) refer to the same
    /// path.  Merge must treat them as equal for
    /// DuplicatePathAcrossSources detection despite byte-level
    /// PathBuf inequality.
    #[test]
    fn dot_component_normalized_for_path_dedup() {
        let cfg = cfg_with_oracle_file("cfg-name", "/etc/foo", "r");
        let (_merged, errs) = merge_cli_into_config(
            cfg,
            vec![cli_file("cli-name", "/etc/./foo", "r")],
            vec![],
            vec![],
            vec![],
        );
        assert!(
            errs.iter()
                .any(|e| matches!(e, FileIoConfigError::DuplicatePathAcrossSources { .. })),
            "expected path collision despite . component; got {errs:?}"
        );
    }

    // -------------------- M-24-4: losing-CLI-entry path still checked --------------------

    /// A CLI entry that loses on logical-name (hard reject) but
    /// whose path collides with a DIFFERENT config alias must
    /// still surface DuplicatePathAcrossSources.  Verifies that
    /// path-dedup runs BEFORE logical-name dedup drops entries.
    #[test]
    fn dropped_cli_entry_still_surfaces_path_collision() {
        let mut cfg = empty_cfg();
        cfg.oracle_static_files
            .insert("shared".into(), StaticFileEntry {
                path: PathBuf::from("/etc/A"),
                mode: "r".into(),
            });
        cfg.oracle_static_files
            .insert("other".into(), StaticFileEntry {
                path: PathBuf::from("/etc/B"),
                mode: "r".into(),
            });
        // CLI's "shared" loses on name (differing mode).  Its
        // path /etc/B collides with config's "other".
        let (_merged, errs) = merge_cli_into_config(
            cfg,
            vec![cli_file("shared", "/etc/B", "r+")],
            vec![],
            vec![],
            vec![],
        );
        assert!(
            errs.iter().any(|e| matches!(
                e,
                FileIoConfigError::DuplicateLogicalNameAcrossSources { .. }
            )),
            "expected name conflict; got {errs:?}"
        );
        assert!(
            errs.iter().any(|e| matches!(
                e,
                FileIoConfigError::DuplicatePathAcrossSources { path, .. }
                    if path == &PathBuf::from("/etc/B")
            )),
            "expected path collision to also surface; got {errs:?}"
        );
    }

    // -------------------- ST-24-1: dir-bucket path dedup --------------------

    #[test]
    fn dir_bucket_same_path_different_names_flagged() {
        let mut cfg = empty_cfg();
        cfg.oracle_static_dirs
            .insert("cfg-dir".into(), StaticDirEntry {
                path: PathBuf::from("/var/shared"),
                mode: "r".into(),
            });
        let (_merged, errs) = merge_cli_into_config(
            cfg,
            vec![],
            vec![cli_dir("cli-dir", "/var/shared", "r")],
            vec![],
            vec![],
        );
        assert!(
            errs.iter().any(|e| matches!(
                e,
                FileIoConfigError::DuplicatePathAcrossSources { bucket, .. }
                    if *bucket == BUCKET_ORACLE_DIR
            )),
            "expected dir-bucket path collision; got {errs:?}"
        );
    }

    // -------------------- ST-24-2: all 6 cross-bucket name pairs --------------------

    /// Same logical name across every pair of distinct buckets is
    /// legal — buckets are separate namespaces.
    #[test]
    fn cross_bucket_same_name_all_six_pairs_allowed() {
        // 6 pairs of the 4 buckets (order-independent).
        // We build a fresh cfg + CLI for each pair.
        let pairs = [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)];
        for (a, b) in pairs {
            let mut cfg = empty_cfg();
            let mut cli_of: Vec<CliStaticFileArg> = vec![];
            let mut cli_od: Vec<CliStaticDirArg> = vec![];
            let mut cli_cf: Vec<CliStaticFileArg> = vec![];
            let mut cli_cd: Vec<CliStaticDirArg> = vec![];
            // Populate bucket `a` in config, bucket `b` in CLI, both with name "n".
            populate_bucket(
                &mut cfg,
                &mut cli_of,
                &mut cli_od,
                &mut cli_cf,
                &mut cli_cd,
                a,
                true,
            );
            populate_bucket(
                &mut cfg,
                &mut cli_of,
                &mut cli_od,
                &mut cli_cf,
                &mut cli_cd,
                b,
                false,
            );
            let (_merged, errs) = merge_cli_into_config(cfg, cli_of, cli_od, cli_cf, cli_cd);
            assert!(
                errs.is_empty(),
                "pair ({a},{b}) — cross-bucket same-name must be allowed; got {errs:?}"
            );
        }
    }

    /// Helper: populate a config or CLI slot for one of the 4
    /// buckets by numeric index (0=oracle-file, 1=oracle-dir,
    /// 2=consensus-file, 3=consensus-dir).  Uses the same logical
    /// name "n" and a bucket-distinct path.
    fn populate_bucket(
        cfg: &mut FileIoProvisioning,
        cli_of: &mut Vec<CliStaticFileArg>,
        cli_od: &mut Vec<CliStaticDirArg>,
        cli_cf: &mut Vec<CliStaticFileArg>,
        cli_cd: &mut Vec<CliStaticDirArg>,
        bucket_idx: usize,
        into_config: bool,
    ) {
        let path = format!("/etc/b{bucket_idx}");
        match bucket_idx {
            0 => {
                if into_config {
                    cfg.oracle_static_files.insert("n".into(), StaticFileEntry {
                        path: PathBuf::from(path),
                        mode: "r".into(),
                    });
                } else {
                    cli_of.push(cli_file("n", &path, "r"));
                }
            }
            1 => {
                if into_config {
                    cfg.oracle_static_dirs.insert("n".into(), StaticDirEntry {
                        path: PathBuf::from(path),
                        mode: "r".into(),
                    });
                } else {
                    cli_od.push(cli_dir("n", &path, "r"));
                }
            }
            2 => {
                if into_config {
                    cfg.consensus_static_files
                        .insert("n".into(), StaticFileEntry {
                            path: PathBuf::from(path),
                            mode: "r".into(),
                        });
                } else {
                    cli_cf.push(cli_file("n", &path, "r"));
                }
            }
            3 => {
                if into_config {
                    cfg.consensus_static_dirs
                        .insert("n".into(), StaticDirEntry {
                            path: PathBuf::from(path),
                            mode: "r".into(),
                        });
                } else {
                    cli_cd.push(cli_dir("n", &path, "r"));
                }
            }
            _ => unreachable!(),
        }
    }

    // -------------------- ST-24-3: count accuracy in mixed scenarios --------------------

    #[test]
    fn intra_cli_count_mixed_identical_and_differing() {
        // 2 identical + 1 differing → count=3, one error.
        let (_merged, errs) = merge_cli_into_config(
            empty_cfg(),
            vec![
                cli_file("a", "/etc/a", "r"),
                cli_file("a", "/etc/a", "r"),
                cli_file("a", "/etc/other", "r"),
            ],
            vec![],
            vec![],
            vec![],
        );
        let dupe_errs: Vec<_> = errs
            .iter()
            .filter_map(|e| match e {
                FileIoConfigError::DuplicateLogicalNameInCli {
                    logical_name,
                    count,
                    ..
                } => Some((logical_name.clone(), *count)),
                _ => None,
            })
            .collect();
        assert_eq!(dupe_errs, vec![("a".to_string(), 3)]);
    }

    #[test]
    fn intra_cli_two_conflicting_names_correct_counts_per_name() {
        let (_merged, errs) = merge_cli_into_config(
            empty_cfg(),
            vec![
                cli_file("a", "/1", "r"),
                cli_file("a", "/2", "r"),
                cli_file("a", "/3", "r"),
                cli_file("b", "/1", "r"),
                cli_file("b", "/2", "r"),
            ],
            vec![],
            vec![],
            vec![],
        );
        let mut counts_by_name: std::collections::BTreeMap<String, usize> = Default::default();
        for e in &errs {
            if let FileIoConfigError::DuplicateLogicalNameInCli {
                logical_name,
                count,
                ..
            } = e
            {
                counts_by_name.insert(logical_name.clone(), *count);
            }
        }
        assert_eq!(counts_by_name.get("a"), Some(&3));
        assert_eq!(counts_by_name.get("b"), Some(&2));
    }

    // -------------------- ST-24-5: config-wins full-struct pin (obsolete) --------------------

    /// M-24-2 hard reject: NO config-wins semantics.  Pin the
    /// hard-reject invariant with a struct-level assertion — if
    /// StaticFileEntry gains a field, this test forces reviewers
    /// to think about whether hard-reject still applies.
    #[test]
    fn hard_reject_removes_config_entry_from_map() {
        let cfg = cfg_with_oracle_file("k", "/etc/cfg", "r");
        let (merged, errs) = merge_cli_into_config(
            cfg,
            vec![cli_file("k", "/etc/cli", "r")],
            vec![],
            vec![],
            vec![],
        );
        assert!(!errs.is_empty());
        assert!(
            !merged.oracle_static_files.contains_key("k"),
            "hard-reject invariant broken: `k` still present"
        );
    }

    // -------------------- ST-24-6: HOCON integration --------------------

    #[test]
    fn hocon_parse_then_merge_then_validate_integration() {
        let text = r#"
            oracle-static-files = {
              "shared": "/etc/does-not-exist-hocon"
            }
        "#;
        let cfg: FileIoProvisioning = hocon::HoconLoader::new()
            .load_str(text)
            .unwrap()
            .resolve()
            .unwrap();
        let errs = merge_and_validate(
            cfg,
            vec![cli_file("shared", "/etc/does-not-exist-hocon", "r+")],
            vec![],
            vec![],
            vec![],
        )
        .expect_err("expected merge conflict on differing modes");
        assert!(
            errs.iter().any(|e| matches!(
                e,
                FileIoConfigError::DuplicateLogicalNameAcrossSources { .. }
            )),
            "expected DuplicateLogicalNameAcrossSources; got {errs:?}"
        );
    }

    // -------------------- ST-24-7: symmetric label test --------------------

    /// The DuplicatePathAcrossSources labels always reflect source
    /// origin: config_logical_name always from config, cli_logical_name
    /// always from CLI — regardless of which side the arg-order
    /// happens to be on inside the module.
    #[test]
    fn duplicate_path_labels_reflect_source_not_arg_order() {
        // Swap conventional letter roles: config uses "b", CLI uses "a".
        let cfg = cfg_with_oracle_file("b", "/etc/shared", "r");
        let (_merged, errs) = merge_cli_into_config(
            cfg,
            vec![cli_file("a", "/etc/shared", "r")],
            vec![],
            vec![],
            vec![],
        );
        assert!(
            errs.iter().any(|e| matches!(
                e,
                FileIoConfigError::DuplicatePathAcrossSources {
                    config_logical_name, cli_logical_name, ..
                } if config_logical_name == "b" && cli_logical_name == "a"
            )),
            "labels swapped; got {errs:?}"
        );
    }

    // -------------------- ST-24-8: 4-cell matrix --------------------

    #[test]
    fn merge_and_validate_matrix_zero_merge_n_validation() {
        // Config has one missing path, no CLI, no merge conflicts.
        let cfg = cfg_with_oracle_file("m", "/does/not/exist", "r");
        let errs = merge_and_validate(cfg, vec![], vec![], vec![], vec![])
            .expect_err("validation-only error");
        assert!(
            errs.iter().all(|e| !matches!(
                e,
                FileIoConfigError::DuplicateLogicalNameAcrossSources { .. }
                    | FileIoConfigError::DuplicateLogicalNameInCli { .. }
                    | FileIoConfigError::DuplicatePathAcrossSources { .. }
            )),
            "no merge errors expected; got {errs:?}"
        );
        assert!(errs
            .iter()
            .any(|e| matches!(e, FileIoConfigError::PathNotFound { .. })));
    }

    #[test]
    fn merge_and_validate_matrix_n_merge_zero_validation() {
        // Merge conflict on non-filesystem-touching paths (both
        // config and CLI declare same-name-different-mode entries;
        // hard-reject removes them; no other entries in the config
        // to validate against filesystem).
        let cfg = cfg_with_oracle_file("k", "/etc/x", "r");
        let errs = merge_and_validate(
            cfg,
            vec![cli_file("k", "/etc/x", "r+")],
            vec![],
            vec![],
            vec![],
        )
        .expect_err("merge-only error");
        assert!(errs.iter().any(|e| matches!(
            e,
            FileIoConfigError::DuplicateLogicalNameAcrossSources { .. }
        )));
        // No PathNotFound because the conflicting entry was
        // removed from the merged map.
        assert!(!errs
            .iter()
            .any(|e| matches!(e, FileIoConfigError::PathNotFound { .. })));
    }

    // -------------------- ST-24-9: N-way path collisions --------------------

    /// Config has 2 aliases at same path, CLI adds a third at same
    /// path with a distinct name.  How many DuplicatePathAcrossSources
    /// errors surface?  Pin the current behavior: ONE — because the
    /// path-dedup uses `cfg_by_path: HashMap<PathBuf, &String>`
    /// which only holds one config name per path.
    #[test]
    fn n_way_path_collision_reports_once_per_cli_entry() {
        let mut cfg = empty_cfg();
        cfg.oracle_static_files
            .insert("cfg-a".into(), StaticFileEntry {
                path: PathBuf::from("/etc/shared"),
                mode: "r".into(),
            });
        cfg.oracle_static_files
            .insert("cfg-b".into(), StaticFileEntry {
                path: PathBuf::from("/etc/shared"),
                mode: "r".into(),
            });
        let (_merged, errs) = merge_cli_into_config(
            cfg,
            vec![cli_file("cli-c", "/etc/shared", "r")],
            vec![],
            vec![],
            vec![],
        );
        let path_errs = errs
            .iter()
            .filter(|e| matches!(e, FileIoConfigError::DuplicatePathAcrossSources { .. }))
            .count();
        // The O(N+M) HashMap-based path-dedup only stores one config
        // name per path, so we get ONE error naming whichever config
        // alias landed in the HashMap (deterministic sort disambiguates
        // in the emit path).  Pin the current behavior.
        assert_eq!(
            path_errs, 1,
            "expected exactly one path-collision error; got {errs:?}"
        );
    }

    // -------------------- N-24-6: PartialEq identity pin --------------------

    /// If a field is added to StaticFileEntry, the derived
    /// PartialEq will include it and merge's silent-dedup semantics
    /// may change unexpectedly.  Pin the identity contract.
    #[test]
    fn static_file_entry_equality_pin() {
        let a = StaticFileEntry {
            path: PathBuf::from("/x"),
            mode: "r".into(),
        };
        let b = StaticFileEntry {
            path: PathBuf::from("/x"),
            mode: "r".into(),
        };
        assert_eq!(
            a, b,
            "if StaticFileEntry grows a field, merge dedup semantics change; update this test deliberately"
        );
    }

    #[test]
    fn static_dir_entry_equality_pin() {
        let a = StaticDirEntry {
            path: PathBuf::from("/x"),
            mode: "r".into(),
        };
        let b = StaticDirEntry {
            path: PathBuf::from("/x"),
            mode: "r".into(),
        };
        assert_eq!(a, b);
    }

    // -------------------- N-24-9: Display same-path diff-mode --------------------

    // ==================================================================
    // Slice 25: bundle projection tests
    // ==================================================================

    #[test]
    fn project_bundle_empty_config_yields_empty_bundle() {
        assert!(project_bundle(&empty_cfg()).is_empty());
    }

    #[test]
    fn project_bundle_covers_all_four_buckets_with_correct_kinds() {
        let mut cfg = empty_cfg();
        cfg.oracle_static_files
            .insert("of".into(), StaticFileEntry {
                path: PathBuf::from("/etc/of"),
                mode: "r".into(),
            });
        cfg.oracle_static_dirs.insert("od".into(), StaticDirEntry {
            path: PathBuf::from("/etc/od"),
            mode: "rw".into(),
        });
        cfg.consensus_static_files
            .insert("cf".into(), StaticFileEntry {
                path: PathBuf::from("/etc/cf"),
                mode: "r+".into(),
            });
        cfg.consensus_static_dirs
            .insert("cd".into(), StaticDirEntry {
                path: PathBuf::from("/etc/cd"),
                mode: "r".into(),
            });
        let bundle = project_bundle(&cfg);
        assert_eq!(bundle.len(), 4);
        // Sorted by logical name: cd < cf < od < of
        assert_eq!(bundle[0].logical_name, "cd");
        assert!(matches!(bundle[0].kind, BundleEntryKind::Dir));
        assert_eq!(bundle[0].consensus_mode, BundleConsensusMode::Consensus);
        assert_eq!(bundle[1].logical_name, "cf");
        assert!(matches!(bundle[1].kind, BundleEntryKind::File));
        assert_eq!(bundle[1].consensus_mode, BundleConsensusMode::Consensus);
        assert_eq!(bundle[2].logical_name, "od");
        assert!(matches!(bundle[2].kind, BundleEntryKind::Dir));
        assert_eq!(bundle[2].consensus_mode, BundleConsensusMode::Oracular);
        assert_eq!(bundle[3].logical_name, "of");
        assert!(matches!(bundle[3].kind, BundleEntryKind::File));
        assert_eq!(bundle[3].consensus_mode, BundleConsensusMode::Oracular);
    }

    #[test]
    fn project_bundle_preserves_path_and_mode() {
        let mut cfg = empty_cfg();
        cfg.oracle_static_files
            .insert("cfg".into(), StaticFileEntry {
                path: PathBuf::from("/etc/myapp/theme.json"),
                mode: "r+".into(),
            });
        let bundle = project_bundle(&cfg);
        assert_eq!(bundle.len(), 1);
        assert_eq!(bundle[0].canon_path, PathBuf::from("/etc/myapp/theme.json"));
        assert_eq!(bundle[0].mode, "r+");
    }

    #[test]
    fn project_bundle_deterministic_across_repeated_runs() {
        // HashMap iteration is randomized; project_bundle sorts.
        let mut cfg = empty_cfg();
        for i in 0..10 {
            cfg.oracle_static_files
                .insert(format!("name-{i}"), StaticFileEntry {
                    path: PathBuf::from(format!("/etc/{i}")),
                    mode: "r".into(),
                });
        }
        let baseline = project_bundle(&cfg);
        for _ in 0..20 {
            assert_eq!(
                project_bundle(&cfg),
                baseline,
                "project_bundle must be deterministic"
            );
        }
    }

    #[test]
    fn display_shows_both_modes_when_paths_match_but_modes_differ() {
        let e = FileIoConfigError::DuplicateLogicalNameAcrossSources {
            bucket: BUCKET_ORACLE_FILE,
            logical_name: "n".into(),
            config_path: PathBuf::from("/same"),
            config_mode: "r".into(),
            cli_path: PathBuf::from("/same"),
            cli_mode: "r+".into(),
        };
        let msg = format!("{e}");
        assert!(
            msg.contains("\"r\"") && msg.contains("\"r+\""),
            "modes missing: {msg}"
        );
    }

    // MT-26-16 review fix: project_bundle mechanically produces two
    // entries when the same logical_name appears in both
    // oracle-static-* and consensus-static-* buckets.  The BOOT
    // validator (`check_cross_source_logical_name_conflict`) is
    // responsible for REJECTING this at merge time; projection
    // itself is a mechanical dump.  This test pins that mechanical
    // behavior — both entries flow through with distinct cmodes.
    #[test]
    fn project_bundle_same_name_across_oracle_consensus_yields_both_entries() {
        let mut cfg = empty_cfg();
        cfg.oracle_static_files
            .insert("shared".into(), StaticFileEntry {
                path: PathBuf::from("/etc/o"),
                mode: "r".into(),
            });
        cfg.consensus_static_files
            .insert("shared".into(), StaticFileEntry {
                path: PathBuf::from("/etc/c"),
                mode: "r".into(),
            });
        let bundle = project_bundle(&cfg);
        assert_eq!(
            bundle.len(),
            2,
            "project_bundle is mechanical; both entries must flow through"
        );
        // Same logical_name, distinct cmodes.  The pair is sorted by
        // logical_name so order after project_bundle is stable but
        // the sort key doesn't disambiguate cmodes — position within
        // ties is HashMap-iteration-order-dependent.  Verify both
        // cmodes appear exactly once, regardless of order.
        let modes: std::collections::BTreeSet<_> = bundle
            .iter()
            .map(|e| (e.logical_name.clone(), e.consensus_mode))
            .collect();
        assert!(modes.contains(&("shared".into(), BundleConsensusMode::Oracular)));
        assert!(modes.contains(&("shared".into(), BundleConsensusMode::Consensus)));
    }

    // H-21-COV-1 (Phase 7 whole-review, delivered 2026-08-06):
    // full-pipeline coverage from a real HOCON string, through
    // `merge_and_validate` (with actual filesystem validation
    // against tempdir paths), through `project_bundle`.  Existing
    // integration coverage (`hocon_parse_then_merge_then_validate_
    // integration`) only exercises the merge-conflict error path;
    // `project_bundle_*` bypasses HOCON parsing and validation
    // entirely by building `FileIoProvisioning` from struct
    // literals with non-existent `/etc/...` paths.  This test
    // closes the gap: operator writes HOCON -> parse produces a
    // real `FileIoProvisioning` -> merge_and_validate walks real
    // files on disk without error -> project_bundle yields a
    // Vec<BundleEntry> ready for `GenesisParameters.fs_bundle`
    // with correct cmodes derived from the operator's bucket
    // choice.
    //
    // Note: HOCON dir keys can't end in `/` (parser rejects), so
    // dir-bucket entries use non-slash-terminated logical names
    // here.  The Fs.rho bundle map treats keys as opaque strings
    // so this doesn't affect deploy-side semantics.
    #[test]
    fn hocon_parse_through_project_bundle_full_pipeline_happy_path() {
        let td = tempfile::TempDir::new().unwrap();
        let root = std::fs::canonicalize(td.path()).unwrap();

        // Seed one real file + one real dir per cmode-bucket.
        let ora_file = root.join("oracle-cfg.json");
        std::fs::write(&ora_file, b"{}").unwrap();
        let ora_dir = root.join("oracle-data");
        std::fs::create_dir(&ora_dir).unwrap();

        let con_file = root.join("consensus-genesis.rho");
        std::fs::write(&con_file, b"Nil").unwrap();
        let con_dir = root.join("consensus-shard-state");
        std::fs::create_dir(&con_dir).unwrap();

        // Realistic HOCON block spanning all four buckets.
        // Note: dir keys use spec §1245's trailing-slash convention
        // where legal, but HOCON strips the trailing `/` on the
        // parse side unless the key is quoted with the `/`
        // preserved -- so we use unslashed names here to keep the
        // assertion set matching what the parser produces.
        let hocon_text = format!(
            r#"
            oracle-static-files {{
              "app-config" = {{ path = "{ora_file}", mode = "r" }}
            }}
            oracle-static-dirs {{
              "app-data" = {{ path = "{ora_dir}", mode = "rw" }}
            }}
            consensus-static-files {{
              "genesis-src" = {{ path = "{con_file}", mode = "r" }}
            }}
            consensus-static-dirs {{
              "shard-state" = {{ path = "{con_dir}", mode = "rw" }}
            }}
            "#,
            ora_file = ora_file.display(),
            ora_dir = ora_dir.display(),
            con_file = con_file.display(),
            con_dir = con_dir.display(),
        );

        // Stage 1: HOCON parse.  Any lexer / deserialize failure
        // fails the test with a clear message.
        let cfg: FileIoProvisioning = hocon::HoconLoader::new()
            .load_str(&hocon_text)
            .expect("HOCON load")
            .resolve()
            .expect("HOCON resolve to FileIoProvisioning");

        // Pin the four-bucket parse before merge validates.
        assert_eq!(cfg.oracle_static_files.len(), 1);
        assert_eq!(cfg.oracle_static_dirs.len(), 1);
        assert_eq!(cfg.consensus_static_files.len(), 1);
        assert_eq!(cfg.consensus_static_dirs.len(), 1);

        // Stage 2: merge + boot validation against real filesystem.
        let merged = merge_and_validate(cfg, vec![], vec![], vec![], vec![])
            .expect("merge_and_validate with real tempdir files must succeed");

        // Stage 3: project into the shape genesis consumes.
        let bundle = project_bundle(&merged);
        assert_eq!(
            bundle.len(),
            4,
            "expected one entry per bucket; got {bundle:?}"
        );

        // Bundle is sorted by (bucket-index, logical_name) then
        // stably by cmode+canon_path (M-P7-4 tie-break).  Verify
        // by logical_name lookup rather than positional to keep
        // the assertion robust to sort-key drift.
        let by_name: std::collections::HashMap<&str, &BundleEntry> = bundle
            .iter()
            .map(|e| (e.logical_name.as_str(), e))
            .collect();

        let ora_f = by_name["app-config"];
        assert!(matches!(ora_f.kind, BundleEntryKind::File));
        assert_eq!(ora_f.consensus_mode, BundleConsensusMode::Oracular);
        assert_eq!(ora_f.canon_path, ora_file);
        assert_eq!(ora_f.mode, "r");

        let ora_d = by_name["app-data"];
        assert!(matches!(ora_d.kind, BundleEntryKind::Dir));
        assert_eq!(ora_d.consensus_mode, BundleConsensusMode::Oracular);
        assert_eq!(ora_d.canon_path, ora_dir);
        assert_eq!(ora_d.mode, "rw");

        let con_f = by_name["genesis-src"];
        assert!(matches!(con_f.kind, BundleEntryKind::File));
        assert_eq!(con_f.consensus_mode, BundleConsensusMode::Consensus);
        assert_eq!(con_f.canon_path, con_file);
        assert_eq!(con_f.mode, "r");

        let con_d = by_name["shard-state"];
        assert!(matches!(con_d.kind, BundleEntryKind::Dir));
        assert_eq!(con_d.consensus_mode, BundleConsensusMode::Consensus);
        assert_eq!(con_d.canon_path, con_dir);
        assert_eq!(con_d.mode, "rw");
    }

    // NT-26-17 review fix: empty-bucket variant coverage.
    #[test]
    fn project_bundle_only_two_buckets_populated() {
        let mut cfg = empty_cfg();
        cfg.oracle_static_files
            .insert("of".into(), StaticFileEntry {
                path: PathBuf::from("/o/f"),
                mode: "r".into(),
            });
        cfg.consensus_static_dirs
            .insert("cd".into(), StaticDirEntry {
                path: PathBuf::from("/c/d"),
                mode: "r".into(),
            });
        let bundle = project_bundle(&cfg);
        assert_eq!(bundle.len(), 2);
        // Sorted by logical_name: cd < of.
        assert_eq!(bundle[0].logical_name, "cd");
        assert_eq!(bundle[0].consensus_mode, BundleConsensusMode::Consensus);
        assert!(matches!(bundle[0].kind, BundleEntryKind::Dir));
        assert_eq!(bundle[1].logical_name, "of");
        assert_eq!(bundle[1].consensus_mode, BundleConsensusMode::Oracular);
        assert!(matches!(bundle[1].kind, BundleEntryKind::File));
    }
}
