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
//! # Entry-value shapes
//!
//! Each map value is either a `{path, mode}` object OR a bare String
//! (path only, mode defaults to `"r"`):
//!
//! ```text
//! oracle-static-files = {
//!   "config/full": { "path": "/etc/myapp/full.json", "mode": "r+" },
//!   "config/bare": "/etc/myapp/theme.json"           // defaults mode = "r"
//! }
//! ```
//!
//! Bare-String form reflects the common case (default-restrictive
//! read-only provisioning); object form is required when the
//! operator opts into write access.
//!
//! Mode-string validation is **deferred** to a follow-up slice.
//! Any string value is accepted at parse time; semantic validation
//! happens when the entry is threaded through Fs.rho / Dir.rho at
//! runtime.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Deserializer, Serialize};

/// Default mode substituted when a static-file entry is bare-String.
/// Spec §File modes default: `"r"`.
pub(crate) const DEFAULT_CONFIG_FILE_MODE: &str = "r";

/// Default mode substituted when a static-dir entry is bare-String.
/// Spec §1245: config is default-restrictive.
pub(crate) const DEFAULT_CONFIG_DIR_MODE: &str = "r";

/// One static-file provisioning entry — the value side of an
/// `oracle-static-files` / `consensus-static-files` map.
///
/// Deserializes from either `{path, mode}` object or bare String
/// (bare → `mode = "r"`).  See `deserialize_static_entry` for the
/// untagged-enum impl shared with `StaticDirEntry`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StaticFileEntry {
    pub path: PathBuf,
    pub mode: String,
}

/// One static-dir provisioning entry — the value side of an
/// `oracle-static-dirs` / `consensus-static-dirs` map.
///
/// Deserializes from either `{path, mode}` object or bare String
/// (bare → `mode = "r"`).  See `deserialize_static_entry` for the
/// untagged-enum impl shared with `StaticFileEntry`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StaticDirEntry {
    pub path: PathBuf,
    pub mode: String,
}

/// Shared deserialize-either-shape helper.  Returns the raw
/// `(path, mode)` pair with `default_mode` substituted when the
/// entry is bare-String.
///
/// Mode / path validation lands in a follow-up slice; this helper
/// is intentionally the thinnest wrapper that supports the two
/// wire shapes.
fn deserialize_static_entry<'de, D>(
    deserializer: D,
    default_mode: &str,
) -> Result<(PathBuf, String), D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Wire {
        Object { path: PathBuf, mode: String },
        Bare(String),
    }

    let wire = Wire::deserialize(deserializer)?;
    Ok(match wire {
        Wire::Object { path, mode } => (path, mode),
        Wire::Bare(s) => (PathBuf::from(s), default_mode.to_string()),
    })
}

impl<'de> Deserialize<'de> for StaticFileEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: Deserializer<'de> {
        let (path, mode) = deserialize_static_entry(deserializer, DEFAULT_CONFIG_FILE_MODE)?;
        Ok(StaticFileEntry { path, mode })
    }
}

impl<'de> Deserialize<'de> for StaticDirEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: Deserializer<'de> {
        let (path, mode) = deserialize_static_entry(deserializer, DEFAULT_CONFIG_DIR_MODE)?;
        Ok(StaticDirEntry { path, mode })
    }
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

    /// Bare-String file entry defaults `mode = "r"`.  Pins the
    /// common-case shorthand: an operator writes a path only and
    /// gets read-only provisioning.
    #[test]
    fn bare_string_file_entry_defaults_to_read_mode() {
        let cfg = parse_provisioning(
            r#"
            oracle-static-files {
              "config/theme.json" = "/etc/myapp/theme.json"
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

    /// Bare-String dir entry defaults `mode = "r"` (spec §1245:
    /// config is default-restrictive; operators must opt into
    /// `"r+"` via the object form).
    #[test]
    fn bare_string_dir_entry_defaults_to_read_mode() {
        let cfg = parse_provisioning(
            r#"
            consensus-static-dirs {
              "shared/" = "/srv/shared"
            }
            "#,
        )
        .expect("parse");
        let entry = cfg
            .consensus_static_dirs
            .get("shared/")
            .expect("entry present");
        assert_eq!(entry.path, PathBuf::from("/srv/shared"));
        assert_eq!(entry.mode, "r");
    }

    /// Mixed object + bare forms in the same bucket parse
    /// independently.  Catches a regression where the untagged
    /// enum match arm order misclassifies one shape as the other.
    #[test]
    fn object_and_bare_forms_coexist_in_one_bucket() {
        let cfg = parse_provisioning(
            r#"
            oracle-static-files {
              "full" = { path = "/a/full", mode = "r+" }
              "bare" = "/a/bare"
            }
            "#,
        )
        .expect("parse");
        let full = cfg.oracle_static_files.get("full").expect("full present");
        assert_eq!(full.mode, "r+");
        let bare = cfg.oracle_static_files.get("bare").expect("bare present");
        assert_eq!(bare.path, PathBuf::from("/a/bare"));
        assert_eq!(bare.mode, "r");
    }

    /// Object-form file entry parses: `{path = ..., mode = ...}`
    /// deserializes into `StaticFileEntry`.  Preserved from ops-1;
    /// pins that the explicit mode isn't discarded by the untagged
    /// enum's bare-String arm.
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
