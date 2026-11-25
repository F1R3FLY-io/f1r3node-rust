// HOCON config schema for the file-read sidecar.
//
// Mirrors the subset of `node::rust::configuration::file_io_provisioning`
// the sidecar needs.  Supported buckets served over HTTP:
//
//   * `oracle-static-files`   — operator-local, no WAL.
//   * `oracle-static-dirs`    — operator-local, no WAL.
//   * `consensus-static-dirs` — operator-declared on-disk path
//     that the node WAL-replicates writes into.  Sidecar serves
//     from the SAME `entry.path` the node registers (see
//     `register_consensus_bundle_roots_core`).  Co-located per-node
//     sidecar gets a locally-consistent view that advances as the
//     node's WAL replay materialises bytes.
//
// `consensus-static-files` are accepted at parse but routed to a
// 503: writes stage through `fs_bundle_staging` before an atomic
// rename, so the logical→on-disk mapping isn't stable from HOCON
// alone and the sidecar would need the node's registry to resolve.
//
// # Why not depend on `node::file_io_provisioning` directly
//
// Importing the node crate would pull the entire node binary's
// dep tree (hundreds of crates) into a sidecar whose job is just
// "serve bytes with an fcntl bracket."  Local duplication is
// cheaper and operator-visible drift is bounded by the schema's
// stability (two bucket keys, two value shapes).
//
// # Supported entry forms
//
//   * `name = "/absolute/path"`           — defaults mode = "r"
//   * `name = { path = "/p", mode = "r" }` — explicit mode
//
// Mode validation is deferred to the node (which operates the WAL
// side); the sidecar only reads bytes, so an operator-declared
// mode doesn't affect correctness.  The sidecar logs a warn for
// malformed modes but doesn't reject.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use eyre::{Context, Result};
use serde::Deserialize;

/// Root config shape.  All four buckets are optional (default
/// empty) so a config with only Oracular entries parses cleanly.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct SidecarConfig {
    /// Static-file entries served by the sidecar.
    #[serde(default, rename = "oracle-static-files")]
    pub oracle_static_files: HashMap<String, EntryValue>,

    /// Static-dir entries served by the sidecar (with a `?sub_rel`
    /// query component).
    #[serde(default, rename = "oracle-static-dirs")]
    pub oracle_static_dirs: HashMap<String, EntryValue>,

    /// Consensus-mode files — accepted at parse but routed to a
    /// 503 response because the sidecar can't resolve
    /// `/@bundle/<X>` to a per-validator on-disk path without
    /// talking to the node's `RootIdentityRegistry`.
    #[serde(default, rename = "consensus-static-files")]
    pub consensus_static_files: HashMap<String, EntryValue>,

    /// Consensus-mode dirs — same posture as consensus-static-files.
    #[serde(default, rename = "consensus-static-dirs")]
    pub consensus_static_dirs: HashMap<String, EntryValue>,
}

/// Per-entry value: either a bare absolute-path String or a
/// `{ path, mode }` object.  Serde's untagged enum tries each
/// variant in order and takes the first that matches.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum EntryValue {
    /// Bare-String form: operator-ergonomic shorthand for the
    /// common read-only default.
    Bare(PathBuf),
    /// Object form with explicit mode.
    Full {
        path: PathBuf,
        #[serde(default)]
        mode: Option<String>,
    },
}

impl EntryValue {
    /// The host-side absolute path for this entry.
    pub fn path(&self) -> &Path {
        match self {
            EntryValue::Bare(p) => p.as_path(),
            EntryValue::Full { path, .. } => path.as_path(),
        }
    }
}

/// Parse a HOCON config file into a `SidecarConfig`.
///
/// The HOCON crate's `load_file` loads + resolves substitutions +
/// deserializes.  Any schema violation (unknown bucket, malformed
/// entry shape) surfaces as an error.
///
/// # Errors
///
///   * File not found or unreadable.
///   * HOCON parse error (malformed syntax).
///   * Schema violation (wrong types, missing required fields).
pub fn load_from_path(path: &Path) -> Result<SidecarConfig> {
    let loaded = hocon::HoconLoader::new()
        .load_file(path)
        .with_context(|| format!("load HOCON config from {}", path.display()))?;
    let cfg: SidecarConfig = loaded
        .resolve()
        .with_context(|| format!("deserialize HOCON config at {}", path.display()))?;
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn write_hocon(dir: &tempfile::TempDir, body: &str) -> PathBuf {
        let path = dir.path().join("config.conf");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(body.as_bytes()).unwrap();
        path
    }

    #[test]
    fn empty_config_parses_to_empty_buckets() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_hocon(&dir, "{}");
        let cfg = load_from_path(&path).unwrap();
        assert!(cfg.oracle_static_files.is_empty());
        assert!(cfg.oracle_static_dirs.is_empty());
        assert!(cfg.consensus_static_files.is_empty());
        assert!(cfg.consensus_static_dirs.is_empty());
    }

    #[test]
    fn bare_string_form_parses_to_bare_variant() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_hocon(
            &dir,
            r#"
            oracle-static-files = {
              theme = "/etc/theme.json"
            }
            "#,
        );
        let cfg = load_from_path(&path).unwrap();
        assert_eq!(cfg.oracle_static_files.len(), 1);
        let entry = cfg.oracle_static_files.get("theme").unwrap();
        assert_eq!(entry.path(), Path::new("/etc/theme.json"));
        assert!(matches!(entry, EntryValue::Bare(_)));
    }

    #[test]
    fn object_form_parses_to_full_variant() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_hocon(
            &dir,
            r#"
            oracle-static-files = {
              theme = { path = "/etc/theme.json", mode = "r+" }
            }
            "#,
        );
        let cfg = load_from_path(&path).unwrap();
        let entry = cfg.oracle_static_files.get("theme").unwrap();
        assert_eq!(entry.path(), Path::new("/etc/theme.json"));
        match entry {
            EntryValue::Full { mode, .. } => assert_eq!(mode.as_deref(), Some("r+")),
            _ => panic!("expected Full variant"),
        }
    }

    #[test]
    fn mixed_buckets_parse_independently() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_hocon(
            &dir,
            r#"
            oracle-static-files = { f = "/etc/f" }
            oracle-static-dirs = { d = "/srv/d" }
            consensus-static-files = { cf = "/var/cf" }
            consensus-static-dirs = { cd = "/var/cd" }
            "#,
        );
        let cfg = load_from_path(&path).unwrap();
        assert_eq!(cfg.oracle_static_files.len(), 1);
        assert_eq!(cfg.oracle_static_dirs.len(), 1);
        assert_eq!(cfg.consensus_static_files.len(), 1);
        assert_eq!(cfg.consensus_static_dirs.len(), 1);
    }

    #[test]
    fn missing_file_surfaces_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("absent.conf");
        let err = load_from_path(&path).unwrap_err();
        assert!(
            err.to_string().contains("load HOCON config"),
            "error must name the load step; got {err}"
        );
    }

    /// Schema violation: a bucket value given as a scalar instead
    /// of the required map surfaces as a deserialization error.
    #[test]
    fn schema_violation_surfaces_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_hocon(
            &dir,
            r#"
            oracle-static-files = "this is a bare scalar, not a map"
            "#,
        );
        let result = load_from_path(&path);
        assert!(
            result.is_err(),
            "a scalar in place of the oracle-static-files map must fail \
             deserialization"
        );
    }
}
