// Built-at-startup index mapping each logical name to a resolved
// entry.  Separates "file" from "dir" so the handler can refuse
// `?sub_rel` on a File entry and require it on a Dir entry.
//
// Buckets routed into the servable `oracular` map:
//
//   * `oracle-static-files`  — operator-declared bytes, no WAL.
//   * `oracle-static-dirs`   — operator-declared bytes, no WAL.
//   * `consensus-static-dirs` — operator-declared on-disk path
//     that the node WAL-replicates writes into.  The sidecar can
//     resolve directly because `register_consensus_bundle_roots`
//     uses `entry.path` verbatim (see setup.rs:186).  Each
//     validator writes the SAME bytes to its own copy, so the
//     per-node sidecar serves a locally-consistent view.
//
// Buckets that land in `skip` with a 503 diagnostic:
//
//   * `consensus-static-files` — writes stage through
//     `fs_bundle_staging` before the atomic rename, so the
//     logical→on-disk mapping is not stable from HOCON alone.
//     The sidecar would need the node's registry to resolve.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::config::SidecarConfig;

/// Resolved entry kind: file vs dir, host-side path.
#[derive(Debug, Clone)]
pub enum Entry {
    File { path: PathBuf },
    Dir { path: PathBuf },
}

impl Entry {
    pub fn path(&self) -> &std::path::Path {
        match self {
            Entry::File { path } => path.as_path(),
            Entry::Dir { path } => path.as_path(),
        }
    }
}

/// Reason an entry is unavailable for sidecar reads.
#[derive(Debug, Clone)]
pub enum SkipReason {
    /// Consensus-mode entry.  Served as a 503 with the specific
    /// diagnostic the plan calls out.
    ConsensusMode,
}

/// Index built once at sidecar startup.  Immutable afterwards.
pub struct FileReadIndex {
    oracular: HashMap<String, Entry>,
    skip: HashMap<String, SkipReason>,
}

impl FileReadIndex {
    /// Build an index from a parsed `SidecarConfig`.  Oracular
    /// entries become `Entry::{File,Dir}`; consensus entries land
    /// in the skip map.
    pub fn build(cfg: &SidecarConfig) -> Self {
        let mut oracular: HashMap<String, Entry> = HashMap::new();
        let mut skip: HashMap<String, SkipReason> = HashMap::new();

        for (name, entry) in &cfg.oracle_static_files {
            oracular.insert(name.clone(), Entry::File {
                path: entry.path().to_path_buf(),
            });
        }
        for (name, entry) in &cfg.oracle_static_dirs {
            oracular.insert(name.clone(), Entry::Dir {
                path: entry.path().to_path_buf(),
            });
        }
        for (name, entry) in &cfg.consensus_static_dirs {
            oracular.insert(name.clone(), Entry::Dir {
                path: entry.path().to_path_buf(),
            });
        }
        for name in cfg.consensus_static_files.keys() {
            skip.insert(name.clone(), SkipReason::ConsensusMode);
        }
        FileReadIndex { oracular, skip }
    }

    pub fn lookup(&self, logical: &str) -> Lookup<'_> {
        if let Some(entry) = self.oracular.get(logical) {
            Lookup::Found(entry)
        } else if let Some(reason) = self.skip.get(logical) {
            Lookup::Skipped(reason)
        } else {
            Lookup::NotFound
        }
    }
}

/// Three-way result of a logical-name lookup.
pub enum Lookup<'a> {
    Found(&'a Entry),
    Skipped(&'a SkipReason),
    NotFound,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::config::EntryValue;

    fn mk_file(path: &str) -> EntryValue { EntryValue::Bare(PathBuf::from(path)) }

    #[test]
    fn empty_config_builds_empty_index() {
        let cfg = SidecarConfig::default();
        let idx = FileReadIndex::build(&cfg);
        assert!(matches!(idx.lookup("anything"), Lookup::NotFound));
    }

    #[test]
    fn oracle_file_resolves_to_file_entry() {
        let mut cfg = SidecarConfig::default();
        cfg.oracle_static_files
            .insert("theme".into(), mk_file("/etc/theme.json"));
        let idx = FileReadIndex::build(&cfg);
        match idx.lookup("theme") {
            Lookup::Found(Entry::File { path }) => {
                assert_eq!(path, &PathBuf::from("/etc/theme.json"));
            }
            _ => panic!("expected Found(File)"),
        }
    }

    #[test]
    fn oracle_dir_resolves_to_dir_entry() {
        let mut cfg = SidecarConfig::default();
        cfg.oracle_static_dirs
            .insert("share".into(), mk_file("/srv/share"));
        let idx = FileReadIndex::build(&cfg);
        match idx.lookup("share") {
            Lookup::Found(Entry::Dir { path }) => {
                assert_eq!(path, &PathBuf::from("/srv/share"));
            }
            _ => panic!("expected Found(Dir)"),
        }
    }

    #[test]
    fn consensus_static_file_routes_to_skip() {
        let mut cfg = SidecarConfig::default();
        cfg.consensus_static_files
            .insert("state".into(), mk_file("/var/state"));
        let idx = FileReadIndex::build(&cfg);
        match idx.lookup("state") {
            Lookup::Skipped(SkipReason::ConsensusMode) => {}
            _ => panic!("expected Skipped(ConsensusMode)"),
        }
    }

    #[test]
    fn consensus_static_dir_resolves_to_dir_entry() {
        let mut cfg = SidecarConfig::default();
        cfg.consensus_static_dirs
            .insert("root".into(), mk_file("/var/lib/rnode/f1r3drive"));
        let idx = FileReadIndex::build(&cfg);
        match idx.lookup("root") {
            Lookup::Found(Entry::Dir { path }) => {
                assert_eq!(path, &PathBuf::from("/var/lib/rnode/f1r3drive"));
            }
            _ => panic!(
                "expected Found(Dir) for operator-declared consensus-static-dirs; \
                 node setup registers entry.path verbatim, so the sidecar can \
                 serve it the same as oracle-static-dirs"
            ),
        }
    }

    #[test]
    fn unknown_logical_returns_not_found() {
        let mut cfg = SidecarConfig::default();
        cfg.oracle_static_files
            .insert("known".into(), mk_file("/etc/k"));
        let idx = FileReadIndex::build(&cfg);
        assert!(matches!(idx.lookup("unknown"), Lookup::NotFound));
    }
}
