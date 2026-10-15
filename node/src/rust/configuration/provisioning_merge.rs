//! File I/O FIP provisioning merge + bundle projection.
//!
//! ops-7a scaffold: lands `project_bundle` only.  Subsequent
//! slices add:
//!
//!   * ops-7b — `cli_static_provisioning` module (CLI arg types +
//!     structured value parsing, no clap yet).
//!   * ops-7c — `merge_cli_into_config` + `merge_and_validate`
//!     top-level merge logic plus intra-CLI / cross-source dedup
//!     helpers.
//!
//! `project_bundle` is the final step of config loading before the
//! bundle hits `casper::genesis::contracts::fs_generator`: walks the
//! four provisioning maps, deterministically orders the output, and
//! runs `BundleEntry::try_new` on every entry as defense-in-depth
//! against programmatic-bypass of the ops-6a serde + ops-6e boot
//! validators.

use std::collections::HashMap;

use casper::rust::genesis::contracts::fs_genesis::{
    BundleConsensusMode, BundleEntry, BundleEntryKind,
};

use super::file_io_provisioning::{FileIoProvisioning, StaticDirEntry, StaticFileEntry};

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
}
