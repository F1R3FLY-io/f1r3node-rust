//! File I/O FIP provisioning merge + bundle projection.
//!
//! Public surface:
//!
//!   * `merge_cli_into_config` — fold `Vec<CliStatic{File,Dir}Arg>`
//!     into a `FileIoProvisioning`, running intra-CLI dedup +
//!     cross-source (config vs CLI) dedup on logical names and
//!     on normalized paths.  All conflicts batched into a sorted
//!     `Vec<FileIoConfigError>`.
//!   * `merge_and_validate` — call `merge_cli_into_config`, then
//!     run `validate_provisioning_boot` on the merged result,
//!     combining errors from both stages for a single boot-
//!     failure report (plan §370).
//!   * `project_bundle` — project a validated
//!     `FileIoProvisioning` into the `Vec<BundleEntry>` shape
//!     `casper::genesis::contracts::fs_generator` consumes.
//!
//! Merge rules (plan §362):
//!
//!   * **Same logical name in both config and CLI** within a bucket:
//!     - Identical `(path, mode)` → silently deduped.
//!     - Different definitions → `DuplicateLogicalNameAcrossSources`;
//!       hard reject (config entry REMOVED, CLI entry NOT inserted).
//!       Boot fails on any error; no silent precedence.
//!   * **Same logical name repeated in CLI** within a bucket (clap
//!     allows repetition):
//!     - Identical repeats → silently deduped.
//!     - Differing repeats → `DuplicateLogicalNameInCli`; first
//!       occurrence wins.  Intra-source: operator's ordering
//!       intent is meaningful; different rule from cross-source.
//!   * **Same absolute path across config + CLI** within a bucket,
//!     under different logical names → `DuplicatePathAcrossSources`.
//!     Path comparison uses `normalize_for_compare` so `/etc/foo`
//!     and `/etc/./foo` compare equal before `NonCanonicalPath`
//!     fires.  Checked on ALL post-intra-CLI entries including
//!     those that will later lose the logical-name hard-reject.

use std::collections::{HashMap, HashSet};

use casper::rust::genesis::contracts::fs_genesis::{
    BundleConsensusMode, BundleEntry, BundleEntryKind,
};

use super::boot_validation::{
    normalize_for_compare, sort_key, validate_provisioning_boot, FileIoConfigError,
    BUCKET_CONSENSUS_DIR, BUCKET_CONSENSUS_FILE, BUCKET_ORACLE_DIR, BUCKET_ORACLE_FILE,
};
use super::commandline::cli_static_provisioning::{CliStaticDirArg, CliStaticFileArg};
use super::file_io_provisioning::{
    FileIoProvisioning, StaticDirEntry, StaticFileEntry, MAX_PROVISIONING_ENTRIES,
};

/// Fold CLI-provided entries into a base `FileIoProvisioning`.
/// Returns the merged provisioning and a batched + sorted
/// `Vec<FileIoConfigError>` describing every merge-time
/// duplicate-detection failure.  The merged provisioning is always
/// produced (never aborted) so downstream validation can accumulate
/// additional errors and the operator sees the full picture.
///
/// Pre-merge size cap: if total input entries (config + raw CLI,
/// before dedup) exceed `MAX_PROVISIONING_ENTRIES`, emits
/// `SizeLimitExceeded` and skips the full merge — bounds pre-boot
/// cost against a hostile unbounded-CLI-arg attacker.
pub fn merge_cli_into_config(
    mut config: FileIoProvisioning,
    cli_oracle_files: Vec<CliStaticFileArg>,
    cli_oracle_dirs: Vec<CliStaticDirArg>,
    cli_consensus_files: Vec<CliStaticFileArg>,
    cli_consensus_dirs: Vec<CliStaticDirArg>,
) -> (FileIoProvisioning, Vec<FileIoConfigError>) {
    let mut errors: Vec<FileIoConfigError> = Vec::new();

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

    errors.sort_by_key(sort_key);
    (config, errors)
}

/// Top-level boot integration.  Merges CLI-provided entries into
/// the config via `merge_cli_into_config`, then runs
/// `validate_provisioning_boot` on the merged result.  Errors
/// from both stages combine into one sorted
/// `Vec<FileIoConfigError>` — operator sees every violation in a
/// single boot-failure report.  The final concatenated vec is
/// re-sorted so discriminant classes interleave correctly.
///
/// ops-8 wires this from the actual boot pipeline; until then,
/// this function has no non-test callers (same posture as
/// `validate_provisioning_boot` through ops-6e/7a).
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
    errors.sort_by_key(sort_key);
    if errors.is_empty() {
        Ok(merged)
    } else {
        Err(errors)
    }
}

// ---------- per-bucket merge composition ----------

fn merge_file_bucket(
    bucket: &'static str,
    config_map: &mut HashMap<String, StaticFileEntry>,
    cli_args: Vec<CliStaticFileArg>,
    errors: &mut Vec<FileIoConfigError>,
) {
    let cli_dedup = intra_cli_dedup_files(bucket, cli_args, errors);
    cross_source_path_dedup_files_map(bucket, config_map, &cli_dedup, errors);
    let cli_to_insert =
        cross_source_logical_name_dedup_files(bucket, config_map, cli_dedup, errors);
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

// ---------- intra-CLI dedup ----------

/// Group CLI file entries by logical name.  Keep the first
/// occurrence; emit `DuplicateLogicalNameInCli` once per
/// conflicting name.  Identical repeats are silently deduped.
/// The emitted `count` is the final-count-after-walk (the first
/// pass stores occurrence-at-detection; a second pass over the
/// bucket-scoped error slice patches to the final total).
fn intra_cli_dedup_files(
    bucket: &'static str,
    args: Vec<CliStaticFileArg>,
    errors: &mut Vec<FileIoConfigError>,
) -> HashMap<String, StaticFileEntry> {
    let mut out: HashMap<String, StaticFileEntry> = HashMap::new();
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut reported: HashSet<String> = HashSet::new();
    for arg in args {
        let CliStaticFileArg {
            logical_name,
            entry,
        } = arg;
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
                        count: *counts.get(&logical_name).expect("just incremented"),
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
                        count: *counts.get(&logical_name).expect("just incremented"),
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

// ---------- cross-source logical-name dedup (hard reject) ----------

/// On conflict: emit `DuplicateLogicalNameAcrossSources` AND remove
/// the config entry.  Rationale: boot fails on any error; leaving
/// the config entry in place would silently privilege one source,
/// and there's no defensible rule for picking either.  If the
/// operator resolves by removing one source, the surviving entry
/// gets inserted normally on the next run.
///
/// Returns the map of CLI entries to insert (CLI-only entries +
/// silently-deduped identical entries that don't need
/// re-insertion).
fn cross_source_logical_name_dedup_files(
    bucket: &'static str,
    config: &mut HashMap<String, StaticFileEntry>,
    cli: HashMap<String, StaticFileEntry>,
    errors: &mut Vec<FileIoConfigError>,
) -> HashMap<String, StaticFileEntry> {
    let mut to_insert: HashMap<String, StaticFileEntry> = HashMap::new();
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

// ---------- cross-source path dedup ----------

/// O(N+M) path-collision check: builds a lexically-normalized
/// index of the config side, then probes each CLI entry.  Takes
/// the FULL post-intra-CLI dedup map (not just the eventual
/// cli_to_insert), so path collisions on entries that will later
/// lose the logical-name hard-reject still surface.
fn cross_source_path_dedup_files_map(
    bucket: &'static str,
    config: &HashMap<String, StaticFileEntry>,
    cli: &HashMap<String, StaticFileEntry>,
    errors: &mut Vec<FileIoConfigError>,
) {
    let cfg_by_path: HashMap<std::path::PathBuf, &String> = config
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

fn cross_source_path_dedup_dirs_map(
    bucket: &'static str,
    config: &HashMap<String, StaticDirEntry>,
    cli: &HashMap<String, StaticDirEntry>,
    errors: &mut Vec<FileIoConfigError>,
) {
    let cfg_by_path: HashMap<std::path::PathBuf, &String> = config
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

/// Project a merged `FileIoProvisioning` into the tuple-vec shape
/// `casper::genesis::contracts::fs_generator` consumes.
///
/// Walks all four buckets, synthesizing `BundleEntryKind` from the
/// map type (File vs Dir) and `BundleConsensusMode` from the bucket
/// family (`oracle-static-*` -> `Oracular`,
/// `consensus-static-*` -> `Consensus`).  Fs.rho threads
/// `consensus_mode` through into File/Dir agent state on mint;
/// File/Dir chown/stat/entries methods forward it to the native
/// handler to decide whether host-transient fields (`mtime`,
/// `atime`, `ctime`, `owner`, `group`) appear in stat records and
/// whether `chown` succeeds.
///
/// Routes through `BundleEntry::try_new` so every projected entry
/// re-runs the upstream invariants (UTF-8, absolute path, no
/// forbidden chars).  If `try_new` fires here, ops-6a (HOCON),
/// ops-9 (CLI, future), or ops-6e (boot) failed to enforce —
/// panic rather than emit a bundle that would crash the Rholang
/// lexer or silently open a different file at deploy time.
///
/// Output is deterministically sorted by
/// `(logical_name, consensus_mode, canon_path)`.  Downstream
/// Rholang serialization sorts again, so this initial ordering is
/// defense-in-depth against a cross-validator genesis-byte
/// divergence.
pub fn project_bundle(cfg: &FileIoProvisioning) -> Vec<BundleEntry> {
    let mut out: Vec<BundleEntry> = Vec::with_capacity(
        cfg.oracle_static_files.len()
            + cfg.oracle_static_dirs.len()
            + cfg.consensus_static_files.len()
            + cfg.consensus_static_dirs.len(),
    );
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
    out.sort_by(|a, b| {
        a.logical_name
            .cmp(&b.logical_name)
            .then_with(|| a.consensus_mode.cmp(&b.consensus_mode))
            .then_with(|| a.canon_path.cmp(&b.canon_path))
    });
    out
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn empty_cfg() -> FileIoProvisioning {
        FileIoProvisioning {
            oracle_static_files: HashMap::new(),
            oracle_static_dirs: HashMap::new(),
            consensus_static_files: HashMap::new(),
            consensus_static_dirs: HashMap::new(),
        }
    }

    #[test]
    fn empty_cfg_projects_to_empty_bundle() {
        let bundle = project_bundle(&empty_cfg());
        assert!(bundle.is_empty());
    }

    /// All four buckets project to the correct
    /// (kind, consensus_mode) pair.
    #[test]
    fn all_four_buckets_project_with_correct_kind_and_cmode() {
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
        let bundle = project_bundle(&cfg);
        assert_eq!(bundle.len(), 4);
        let by_name: HashMap<&str, &BundleEntry> = bundle
            .iter()
            .map(|e| (e.logical_name.as_str(), e))
            .collect();
        assert_eq!(by_name["of"].kind, BundleEntryKind::File);
        assert_eq!(by_name["of"].consensus_mode, BundleConsensusMode::Oracular);
        assert_eq!(by_name["od"].kind, BundleEntryKind::Dir);
        assert_eq!(by_name["od"].consensus_mode, BundleConsensusMode::Oracular);
        assert_eq!(by_name["cf"].kind, BundleEntryKind::File);
        assert_eq!(by_name["cf"].consensus_mode, BundleConsensusMode::Consensus);
        assert_eq!(by_name["cd"].kind, BundleEntryKind::Dir);
        assert_eq!(by_name["cd"].consensus_mode, BundleConsensusMode::Consensus);
    }

    /// Output must be deterministically sorted by logical_name
    /// across all four buckets.  Pins the invariant downstream
    /// Rholang composition depends on.
    #[test]
    fn output_is_sorted_by_logical_name() {
        let mut cfg = empty_cfg();
        for name in ["z", "a", "m", "b"] {
            cfg.oracle_static_files
                .insert(name.into(), StaticFileEntry {
                    path: PathBuf::from(format!("/x/{name}")),
                    mode: "r".into(),
                });
        }
        let bundle = project_bundle(&cfg);
        let names: Vec<&str> = bundle.iter().map(|e| e.logical_name.as_str()).collect();
        assert_eq!(names, vec!["a", "b", "m", "z"]);
    }

    /// Three back-to-back projections of the same cfg produce
    /// byte-identical bundles.  HashMap iteration is randomized;
    /// without the deterministic sort, cross-validator genesis-
    /// byte divergence is immediate.
    #[test]
    fn projection_is_deterministic_across_runs() {
        let mut cfg = empty_cfg();
        for (i, name) in ["a", "b", "c", "d"].iter().enumerate() {
            cfg.oracle_static_files
                .insert((*name).into(), StaticFileEntry {
                    path: PathBuf::from(format!("/x/{i}")),
                    mode: "r".into(),
                });
            cfg.consensus_static_dirs
                .insert(format!("{name}-d"), StaticDirEntry {
                    path: PathBuf::from(format!("/x/d{i}")),
                    mode: "r".into(),
                });
        }
        let b1 = project_bundle(&cfg);
        let b2 = project_bundle(&cfg);
        let b3 = project_bundle(&cfg);
        assert_eq!(b1, b2);
        assert_eq!(b2, b3);
    }

    /// Mode strings carry through to the projected bundle entry
    /// unchanged.  No defaulting at the projection layer — the
    /// ops-6a / ops-6e layers are responsible for mode validation.
    #[test]
    fn modes_pass_through_unchanged() {
        let mut cfg = empty_cfg();
        cfg.oracle_static_files.insert("f".into(), StaticFileEntry {
            path: PathBuf::from("/x/f"),
            mode: "r+".into(),
        });
        cfg.consensus_static_dirs
            .insert("d".into(), StaticDirEntry {
                path: PathBuf::from("/x/d"),
                mode: "r+".into(),
            });
        let bundle = project_bundle(&cfg);
        let f = bundle.iter().find(|e| e.logical_name == "f").unwrap();
        let d = bundle.iter().find(|e| e.logical_name == "d").unwrap();
        assert_eq!(f.mode, "r+");
        assert_eq!(d.mode, "r+");
    }

    /// Programmatic bypass that would crash the Rholang lexer
    /// (e.g., NUL in logical_name) is caught by `BundleEntry::try_new`
    /// and panics the projection — upstream invariant failure.
    #[test]
    #[should_panic(expected = "Upstream should have caught")]
    fn projection_panics_on_programmatic_bypass() {
        let mut cfg = empty_cfg();
        cfg.oracle_static_files
            .insert("has\0nul".into(), StaticFileEntry {
                path: PathBuf::from("/x/f"),
                mode: "r".into(),
            });
        let _ = project_bundle(&cfg);
    }

    // ---------- merge_cli_into_config / merge_and_validate ----------

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

    /// Empty inputs produce an empty merged provisioning with no
    /// errors.
    #[test]
    fn merge_empty_inputs_passes() {
        let (merged, errors) = merge_cli_into_config(empty_cfg(), vec![], vec![], vec![], vec![]);
        assert!(errors.is_empty());
        assert!(merged.oracle_static_files.is_empty());
        assert!(merged.oracle_static_dirs.is_empty());
        assert!(merged.consensus_static_files.is_empty());
        assert!(merged.consensus_static_dirs.is_empty());
    }

    /// CLI entry with a logical name absent from config is folded
    /// into the merged map.
    #[test]
    fn cli_only_entry_is_inserted() {
        let (merged, errors) = merge_cli_into_config(
            empty_cfg(),
            vec![cli_file("cfg", "/etc/cfg", "r")],
            vec![],
            vec![],
            vec![],
        );
        assert!(errors.is_empty());
        assert_eq!(merged.oracle_static_files.len(), 1);
        assert_eq!(
            merged.oracle_static_files.get("cfg").unwrap().path,
            PathBuf::from("/etc/cfg")
        );
    }

    /// Identical config + CLI entry: silent dedup, no error, no
    /// change.
    #[test]
    fn identical_config_and_cli_silently_deduped() {
        let mut config = empty_cfg();
        config
            .oracle_static_files
            .insert("cfg".into(), StaticFileEntry {
                path: PathBuf::from("/etc/cfg"),
                mode: "r".into(),
            });
        let (merged, errors) = merge_cli_into_config(
            config,
            vec![cli_file("cfg", "/etc/cfg", "r")],
            vec![],
            vec![],
            vec![],
        );
        assert!(errors.is_empty(), "got {errors:?}");
        assert_eq!(merged.oracle_static_files.len(), 1);
    }

    /// Differing config + CLI entry under the same logical name:
    /// hard reject.  Error emitted; config entry REMOVED; CLI entry
    /// NOT inserted.  No silent precedence.
    #[test]
    fn conflicting_config_and_cli_hard_rejects_both() {
        let mut config = empty_cfg();
        config
            .oracle_static_files
            .insert("cfg".into(), StaticFileEntry {
                path: PathBuf::from("/etc/cfg"),
                mode: "r".into(),
            });
        let (merged, errors) = merge_cli_into_config(
            config,
            vec![cli_file("cfg", "/etc/other", "r+")],
            vec![],
            vec![],
            vec![],
        );
        assert_eq!(errors.len(), 1);
        assert!(matches!(
            errors[0],
            FileIoConfigError::DuplicateLogicalNameAcrossSources { .. }
        ));
        assert!(
            !merged.oracle_static_files.contains_key("cfg"),
            "config entry must be removed on hard reject"
        );
    }

    /// Same logical name repeated in CLI with identical payloads:
    /// silent dedup.
    #[test]
    fn identical_cli_repeats_silently_deduped() {
        let (merged, errors) = merge_cli_into_config(
            empty_cfg(),
            vec![
                cli_file("cfg", "/etc/cfg", "r"),
                cli_file("cfg", "/etc/cfg", "r"),
            ],
            vec![],
            vec![],
            vec![],
        );
        assert!(errors.is_empty(), "got {errors:?}");
        assert_eq!(merged.oracle_static_files.len(), 1);
    }

    /// Same logical name repeated in CLI with differing payloads:
    /// emit `DuplicateLogicalNameInCli` once with the final count;
    /// first occurrence wins in the merged map.
    #[test]
    fn differing_cli_repeats_emit_single_error_with_final_count() {
        let (merged, errors) = merge_cli_into_config(
            empty_cfg(),
            vec![
                cli_file("cfg", "/etc/a", "r"),
                cli_file("cfg", "/etc/b", "r"),
                cli_file("cfg", "/etc/c", "r"),
            ],
            vec![],
            vec![],
            vec![],
        );
        let cli_errors: Vec<_> = errors
            .iter()
            .filter_map(|e| match e {
                FileIoConfigError::DuplicateLogicalNameInCli { count, .. } => Some(*count),
                _ => None,
            })
            .collect();
        assert_eq!(cli_errors.len(), 1);
        assert_eq!(cli_errors[0], 3, "final count must be total occurrences");
        assert_eq!(
            merged.oracle_static_files.get("cfg").unwrap().path,
            PathBuf::from("/etc/a"),
            "first-occurrence must win"
        );
    }

    /// Config + CLI entries pointing at the same path under
    /// different logical names: `DuplicatePathAcrossSources` fires.
    #[test]
    fn cross_source_path_collision_rejected() {
        let mut config = empty_cfg();
        config
            .oracle_static_files
            .insert("a".into(), StaticFileEntry {
                path: PathBuf::from("/etc/shared"),
                mode: "r".into(),
            });
        let (_merged, errors) = merge_cli_into_config(
            config,
            vec![cli_file("b", "/etc/shared", "r")],
            vec![],
            vec![],
            vec![],
        );
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, FileIoConfigError::DuplicatePathAcrossSources { .. })),
            "expected DuplicatePathAcrossSources; got {errors:?}"
        );
    }

    /// Path collision via lexical-non-canonical spelling (`/etc/./x`
    /// vs `/etc/x`) still fires `DuplicatePathAcrossSources` because
    /// `normalize_for_compare` collapses the `.` component.
    #[test]
    fn cross_source_path_collision_via_dot_component() {
        let mut config = empty_cfg();
        config
            .oracle_static_files
            .insert("a".into(), StaticFileEntry {
                path: PathBuf::from("/etc/x"),
                mode: "r".into(),
            });
        let (_merged, errors) = merge_cli_into_config(
            config,
            vec![cli_file("b", "/etc/./x", "r")],
            vec![],
            vec![],
            vec![],
        );
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, FileIoConfigError::DuplicatePathAcrossSources { .. })),
            "normalize_for_compare must collapse `.` so same-path fires; got {errors:?}"
        );
    }

    /// Even an entry that will later lose logical-name hard-reject
    /// still gets its path collision reported.  M-24-4: path
    /// dedup runs on the FULL post-intra-CLI set before the
    /// logical-name hard-reject drops anything.
    #[test]
    fn path_collision_reported_before_logical_name_hard_reject() {
        let mut config = empty_cfg();
        config
            .oracle_static_files
            .insert("existing_name".into(), StaticFileEntry {
                path: PathBuf::from("/etc/shared"),
                mode: "r".into(),
            });
        // CLI uses the same logical name AND the same path, with
        // a different mode -> both the name-conflict and the
        // path-conflict should fire.
        let (_merged, errors) = merge_cli_into_config(
            config,
            vec![cli_file("existing_name", "/etc/shared", "r+")],
            vec![],
            vec![],
            vec![],
        );
        assert!(
            errors.iter().any(|e| matches!(
                e,
                FileIoConfigError::DuplicateLogicalNameAcrossSources { .. }
            )),
            "logical-name hard-reject must fire; got {errors:?}"
        );
        // Same path with same name: path dedup check compares
        // names-equal and skips (not a cross-source collision).
    }

    /// Dir buckets go through the same merge path.
    #[test]
    fn dir_bucket_merge_also_hard_rejects() {
        let mut config = empty_cfg();
        config
            .oracle_static_dirs
            .insert("share".into(), StaticDirEntry {
                path: PathBuf::from("/srv/share"),
                mode: "r".into(),
            });
        let (merged, errors) = merge_cli_into_config(
            config,
            vec![],
            vec![cli_dir("share", "/srv/other", "r+")],
            vec![],
            vec![],
        );
        assert!(
            errors.iter().any(|e| matches!(
                e,
                FileIoConfigError::DuplicateLogicalNameAcrossSources { .. }
            )),
            "got {errors:?}"
        );
        assert!(!merged.oracle_static_dirs.contains_key("share"));
    }

    /// Size cap: raw input total (config + CLI, pre-dedup)
    /// exceeding MAX_PROVISIONING_ENTRIES short-circuits the full
    /// merge.
    #[test]
    fn size_cap_short_circuits_merge() {
        // Build a CLI vec just over the cap.
        let cli: Vec<CliStaticFileArg> = (0..(MAX_PROVISIONING_ENTRIES + 1))
            .map(|i| cli_file(&format!("n{i}"), &format!("/x/{i}"), "r"))
            .collect();
        let (merged, errors) = merge_cli_into_config(empty_cfg(), cli, vec![], vec![], vec![]);
        assert_eq!(errors.len(), 1);
        assert!(matches!(
            errors[0],
            FileIoConfigError::SizeLimitExceeded { .. }
        ));
        // Merge short-circuited; config untouched.
        assert!(merged.oracle_static_files.is_empty());
    }

    /// Error output must be deterministic across repeated calls
    /// (sort_key stable, iteration order controlled).
    #[test]
    fn merge_errors_are_deterministic() {
        let args = vec![
            cli_file("a", "/x/a1", "r"),
            cli_file("a", "/x/a2", "r"),
            cli_file("b", "/x/b1", "r"),
            cli_file("b", "/x/b2", "r"),
        ];
        let (_m1, e1) = merge_cli_into_config(empty_cfg(), args.clone(), vec![], vec![], vec![]);
        let (_m2, e2) = merge_cli_into_config(empty_cfg(), args.clone(), vec![], vec![], vec![]);
        let (_m3, e3) = merge_cli_into_config(empty_cfg(), args, vec![], vec![], vec![]);
        assert_eq!(e1, e2);
        assert_eq!(e2, e3);
    }

    /// `merge_and_validate` combines merge errors with boot-
    /// validation errors in a single sorted vec.
    #[test]
    fn merge_and_validate_combines_errors() {
        let result = merge_and_validate(
            empty_cfg(),
            // Programmatic bypass: relative path reaches the
            // validation stage (merge accepts it; boot catches it).
            vec![CliStaticFileArg {
                logical_name: "cfg".into(),
                entry: StaticFileEntry {
                    path: PathBuf::from("relative"),
                    mode: "r".into(),
                },
            }],
            vec![],
            vec![],
            vec![],
        );
        let errs = result.expect_err("relative path must fail validation");
        assert!(
            errs.iter()
                .any(|e| matches!(e, FileIoConfigError::NotAbsolute { .. })),
            "boot-validation NotAbsolute must surface; got {errs:?}"
        );
    }

    /// `merge_and_validate` returns `Ok(merged)` on clean inputs.
    #[test]
    fn merge_and_validate_passes_clean_cfg() {
        // Use absolute paths to clean the validation path.
        let result = merge_and_validate(empty_cfg(), vec![], vec![], vec![], vec![])
            .expect("empty is clean");
        assert!(result.oracle_static_files.is_empty());
    }
}
