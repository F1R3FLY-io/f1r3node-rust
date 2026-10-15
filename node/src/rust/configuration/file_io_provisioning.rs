//! File I/O FIP static-provisioning config schema — type-definition
//! surface only (ops-port slice 1).
//!
//! Carves out `storage.{oracle,consensus}-static-{files,dirs}` as a
//! typed HOCON block so operator config can declare which host paths
//! are provisioned into the Fs cap bundle at genesis time.  This
//! slice is **types + default-deserialization only**.  Subsequent
//! slices add:
//!
//!   * Bare-String entry-value form (one-line path assignment).
//!   * Mode validation at deserialization time (whitelist against
//!     POSIX file modes / Dir attenuation keywords).
//!   * `check_provisioning_typos` operator-ergonomic typo detector.
//!   * `validate_size_limits` DoS guard on map cardinality.
//!   * Boot-time canonicalization + tree validation (symlink /
//!     hard-link walk) in `boot_validation`.
//!   * Merge-with-defaults + CLI overrides in `provisioning_merge`.
//!   * Boot wiring that threads this config into
//!     `GenesisParameters.fs_bundle` + `RootIdentityRegistry`.
//!
//! # Spec references (future slices)
//!
//!   * §1258-1288 (Config block, CLI flags, load-time validation).
//!   * §1245 (Dir mode defaults).
//!   * §File modes (POSIX whitelist).
//!
//! # Mode strings
//!
//! Static entries use the same mode vocabulary as the Fs/Dir caps:
//! POSIX file modes for files (`"r"`, `"r+"`, `"w"`, `"w+"`, `"a"`,
//! `"a+"`), and `"r"` / `"r+"` for Dir attenuation.  Validation of
//! that vocabulary is deferred to the mode-validation slice.
//!
//! # Minimum-slice posture
//!
//! `#[derive(Default, Deserialize, Serialize)]` with `#[serde(rename
//! = "...")]` kebab-case field names.  No custom deserialize impl
//! yet; config entries must use the object form `{path = "...",
//! mode = "..."}`.  An empty HOCON block (or an absent block)
//! parses to `FileIoProvisioning::default()` with four empty maps.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// One static-file provisioning entry — the value side of an
/// `oracle-static-files` / `consensus-static-files` map.
///
/// MVP: object form only (`{path = "...", mode = "..."}`).  A later
/// slice adds bare-String form that defaults `mode = "r"`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct StaticFileEntry {
    pub path: PathBuf,
    pub mode: String,
}

/// One static-dir provisioning entry — the value side of an
/// `oracle-static-dirs` / `consensus-static-dirs` map.
///
/// MVP: object form only (`{path = "...", mode = "..."}`).  A later
/// slice adds bare-String form that defaults `mode = "r"`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct StaticDirEntry {
    pub path: PathBuf,
    pub mode: String,
}

/// Static-provisioning block: four maps split by consensus axis
/// (oracle / consensus) × kind axis (files / dirs).  Each map keys
/// a Rholang-level logical name (what callers pass to
/// `Fs.openFile` / `Fs.openDir`) to an on-disk entry.
///
/// All four maps default to empty so a node config that doesn't
/// provision any static entries parses unchanged.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct FileIoProvisioning {
    #[serde(default, rename = "oracle-static-files")]
    pub oracle_static_files: HashMap<String, StaticFileEntry>,

    #[serde(default, rename = "oracle-static-dirs")]
    pub oracle_static_dirs: HashMap<String, StaticDirEntry>,

    #[serde(default, rename = "consensus-static-files")]
    pub consensus_static_files: HashMap<String, StaticFileEntry>,

    #[serde(default, rename = "consensus-static-dirs")]
    pub consensus_static_dirs: HashMap<String, StaticDirEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_provisioning(hocon_text: &str) -> Result<FileIoProvisioning, String> {
        hocon::HoconLoader::new()
            .load_str(hocon_text)
            .map_err(|e| format!("load: {e}"))?
            .resolve()
            .map_err(|e| format!("resolve: {e}"))
    }

    /// An empty / missing block parses to all-empty maps.  Covers
    /// the default-posture case: an operator who hasn't opted into
    /// static provisioning gets `FileIoProvisioning::default()`.
    #[test]
    fn empty_block_yields_all_empty_maps() {
        let cfg = parse_provisioning("").expect("empty parses");
        assert!(cfg.oracle_static_files.is_empty());
        assert!(cfg.oracle_static_dirs.is_empty());
        assert!(cfg.consensus_static_files.is_empty());
        assert!(cfg.consensus_static_dirs.is_empty());
    }

    /// Object-form file entry parses: `{path = ..., mode = ...}`
    /// deserializes into `StaticFileEntry`.  Bare-String form is
    /// not yet supported and is deferred to a follow-up slice.
    #[test]
    fn object_form_file_entry_parses() {
        let cfg = parse_provisioning(
            r#"
            oracle-static-files {
              "config/theme.json" = { path = "/etc/myapp/theme.json", mode = "r" }
            }
            "#,
        )
        .expect("parse");
        let entry = cfg
            .oracle_static_files
            .get("config/theme.json")
            .expect("entry present");
        assert_eq!(entry.path, PathBuf::from("/etc/myapp/theme.json"));
        assert_eq!(entry.mode, "r");
    }

    /// Object-form dir entry parses with each of the two canonical
    /// Dir attenuation keywords.
    #[test]
    fn object_form_dir_entry_parses_r_and_rplus() {
        let cfg_r = parse_provisioning(
            r#"
            oracle-static-dirs {
              "shared/" = { path = "/srv/shared", mode = "r" }
            }
            "#,
        )
        .expect("parse r-mode");
        assert_eq!(
            cfg_r.oracle_static_dirs.get("shared/").map(|d| &d.mode),
            Some(&"r".to_string())
        );

        let cfg_rp = parse_provisioning(
            r#"
            consensus-static-dirs {
              "writable/" = { path = "/srv/writable", mode = "r+" }
            }
            "#,
        )
        .expect("parse r+-mode");
        assert_eq!(
            cfg_rp
                .consensus_static_dirs
                .get("writable/")
                .map(|d| &d.mode),
            Some(&"r+".to_string())
        );
    }

    /// All four buckets populate independently.  Catches a schema
    /// regression where kebab-case rename drops a bucket (e.g., a
    /// typo in `#[serde(rename = ...)]`).
    #[test]
    fn all_four_buckets_populate_independently() {
        let cfg = parse_provisioning(
            r#"
            oracle-static-files    { "of"  = { path = "/a", mode = "r" } }
            oracle-static-dirs     { "od/" = { path = "/b", mode = "r" } }
            consensus-static-files { "cf"  = { path = "/c", mode = "r" } }
            consensus-static-dirs  { "cd/" = { path = "/d", mode = "r" } }
            "#,
        )
        .expect("parse");
        assert_eq!(cfg.oracle_static_files.len(), 1);
        assert_eq!(cfg.oracle_static_dirs.len(), 1);
        assert_eq!(cfg.consensus_static_files.len(), 1);
        assert_eq!(cfg.consensus_static_dirs.len(), 1);
    }
}
