//! File I/O FIP static-provisioning config schema (Phase 7 slice 21).
//!
//! Parses the four `storage.{oracle,consensus}-static-{files,dirs}` maps
//! that assign logical paths to canonical host paths + modes.  Boot-time
//! tree validation (symlink / hard-link walk) and CLI-flag parsing land
//! in subsequent Phase 7 slices; this module covers the config-schema
//! surface only.
//!
//! # Spec references
//!
//! - §1258-1288 (Config block, CLI flags, load-time validation).
//! - §1245 (dir mode defaults: `"rw"` CLI, `"r"` config).
//! - §File modes (whitelist: `"r"`, `"r+"`, `"w"`, `"w+"`, `"a"`, `"a+"`;
//!   `"wx"` / `"w+x"` rejected in config per §1281).
//!
//! # Entry-value shapes
//!
//! Each map value is either a `{path, mode}` object OR a bare String
//! (path only, mode defaults):
//!
//! ```text
//! oracle-static-files = {
//!   "reports/q3/summary.csv": { "path": "/srv/data/q3.csv", "mode": "r+" },
//!   "config/theme.json":      "/etc/myapp/theme.json"        // bare
//! }
//! ```
//!
//! Bare-String form gets the class-appropriate default mode (`"r"` for
//! files and dirs — config is default-restrictive per spec §1245).

use std::collections::HashMap;
use std::path::PathBuf;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

/// One static-file provisioning entry — the value side of an
/// `oracle-static-files` / `consensus-static-files` map.
///
/// Deserializes from either `{path, mode}` object or bare String
/// (bare String defaults `mode = "r"`).  Mode is validated at
/// deserialization time against §File modes minus `"wx"` / `"w+x"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StaticFileEntry {
    pub path: PathBuf,
    pub mode: String,
}

/// One static-dir provisioning entry — the value side of an
/// `oracle-static-dirs` / `consensus-static-dirs` map.
///
/// Deserializes from either `{path, mode}` object or bare String
/// (bare String defaults `mode = "r"` per spec §1245).  Mode is
/// validated against `{"r", "rw"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StaticDirEntry {
    pub path: PathBuf,
    pub mode: String,
}

/// The whitelist of file modes allowed in config-block form.  Spec
/// §File modes lists eight; `"wx"` and `"w+x"` are rejected in
/// config (spec §1281: they require the file to not exist,
/// contradicting the config's existence check).
pub const CONFIG_FILE_MODES: &[&str] = &["r", "r+", "w", "w+", "a", "a+"];

/// The whitelist of dir modes.
pub const CONFIG_DIR_MODES: &[&str] = &["r", "rw"];

/// Default file mode when the config entry is a bare String.
pub const DEFAULT_CONFIG_FILE_MODE: &str = "r";

/// Default dir mode when the config entry is a bare String (spec
/// §1245: config is default-restrictive).
pub const DEFAULT_CONFIG_DIR_MODE: &str = "r";

/// Syntactic path-shape checks that a config-loader can run without
/// filesystem access (i.e., before the boot-time tree walk).  Returns
/// an error message on rejection; `Ok(())` on pass.
///
/// - Rejects relative paths (spec §1273): "Every configured path must
///   be absolute."
/// - Rejects empty strings.
/// - Rejects paths that aren't valid UTF-8 (in practice `PathBuf` was
///   constructed from a `String` here, so this is a defensive check).
pub fn validate_absolute_path(p: &PathBuf) -> Result<(), String> {
    if p.as_os_str().is_empty() {
        return Err("path is empty".into());
    }
    if !p.is_absolute() {
        return Err(format!(
            "path {:?} is not absolute; static provisioning requires absolute host paths (spec §1273)",
            p
        ));
    }
    if p.to_str().is_none() {
        return Err(format!(
            "path {:?} is not valid UTF-8; spec §1274 requires UTF-8 paths",
            p
        ));
    }
    Ok(())
}

/// Deserialize either a `{path, mode}` object or a bare String.
/// Common inner impl reused by `StaticFileEntry` and `StaticDirEntry`.
///
/// `default_mode` is the mode substituted when the entry is bare-String.
/// `mode_whitelist` is the set of accepted mode strings; anything else
/// yields a deserialization error.
fn deserialize_static_entry<'de, D>(
    deserializer: D,
    default_mode: &str,
    mode_whitelist: &[&str],
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
    let (path, mode) = match wire {
        Wire::Object { path, mode } => (path, mode),
        Wire::Bare(s) => (PathBuf::from(s), default_mode.to_string()),
    };

    validate_absolute_path(&path).map_err(D::Error::custom)?;

    if !mode_whitelist.contains(&mode.as_str()) {
        return Err(D::Error::custom(format!(
            "invalid mode {:?}; allowed modes are {:?}",
            mode, mode_whitelist
        )));
    }

    Ok((path, mode))
}

impl<'de> Deserialize<'de> for StaticFileEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: Deserializer<'de> {
        let (path, mode) =
            deserialize_static_entry(deserializer, DEFAULT_CONFIG_FILE_MODE, CONFIG_FILE_MODES)?;
        Ok(StaticFileEntry { path, mode })
    }
}

impl<'de> Deserialize<'de> for StaticDirEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: Deserializer<'de> {
        let (path, mode) =
            deserialize_static_entry(deserializer, DEFAULT_CONFIG_DIR_MODE, CONFIG_DIR_MODES)?;
        Ok(StaticDirEntry { path, mode })
    }
}

/// The four static-provisioning maps that live under `storage {}` in
/// the config block.  Each map is optional (defaults to empty) so a
/// node config that doesn't provision any static files parses cleanly.
///
/// Kept as a bundle struct so `Storage` can `#[serde(flatten)]` it and
/// keep field names spec-canonical (`oracle-static-files`, ...).
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
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

    #[test]
    fn empty_block_yields_all_empty_maps() {
        let cfg = parse_provisioning("").expect("empty parses");
        assert!(cfg.oracle_static_files.is_empty());
        assert!(cfg.oracle_static_dirs.is_empty());
        assert!(cfg.consensus_static_files.is_empty());
        assert!(cfg.consensus_static_dirs.is_empty());
    }

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

    #[test]
    fn bare_string_dir_entry_defaults_to_read_mode() {
        // Spec §1245 — config-block dir defaults to "r".
        let cfg = parse_provisioning(
            r#"
            oracle-static-dirs {
              "reports/archive/" = "/srv/data/reports-archive"
            }
            "#,
        )
        .expect("parse");
        let entry = cfg
            .oracle_static_dirs
            .get("reports/archive/")
            .expect("entry present");
        assert_eq!(entry.mode, "r");
    }

    #[test]
    fn dir_entry_accepts_rw_mode() {
        let cfg = parse_provisioning(
            r#"
            oracle-static-dirs {
              "output/" = { path = "/var/spool/myapp/out", mode = "rw" }
            }
            "#,
        )
        .expect("parse");
        assert_eq!(cfg.oracle_static_dirs.get("output/").unwrap().mode, "rw");
    }

    #[test]
    fn dir_entry_rejects_non_dir_mode() {
        let err = parse_provisioning(
            r#"
            oracle-static-dirs {
              "output/" = { path = "/var/spool/myapp/out", mode = "r+" }
            }
            "#,
        )
        .expect_err("r+ isn't a dir mode");
        assert!(err.contains("invalid mode"), "unexpected error: {err}");
    }

    #[test]
    fn file_entry_rejects_wx_mode() {
        // Spec §1281: "wx" and "w+x" contradict the existence check
        // and are rejected in config-block form.
        let err = parse_provisioning(
            r#"
            oracle-static-files {
              "log.txt" = { path = "/var/log/myapp.log", mode = "wx" }
            }
            "#,
        )
        .expect_err("wx must be rejected in config");
        assert!(err.contains("invalid mode"), "unexpected error: {err}");
    }

    #[test]
    fn file_entry_rejects_w_plus_x_mode() {
        let err = parse_provisioning(
            r#"
            oracle-static-files {
              "log.txt" = { path = "/var/log/myapp.log", mode = "w+x" }
            }
            "#,
        )
        .expect_err("w+x must be rejected in config");
        assert!(err.contains("invalid mode"), "unexpected error: {err}");
    }

    #[test]
    fn all_valid_file_modes_accepted() {
        for mode in CONFIG_FILE_MODES {
            let text = format!(
                r#"
                oracle-static-files {{
                  "file.txt" = {{ path = "/tmp/file.txt", mode = "{mode}" }}
                }}
                "#
            );
            let cfg = parse_provisioning(&text)
                .unwrap_or_else(|e| panic!("mode {mode} should be accepted; error: {e}"));
            assert_eq!(cfg.oracle_static_files.get("file.txt").unwrap().mode, *mode);
        }
    }

    #[test]
    fn relative_path_rejected() {
        let err = parse_provisioning(
            r#"
            oracle-static-files {
              "cfg" = { path = "relative/path.json", mode = "r" }
            }
            "#,
        )
        .expect_err("relative path must be rejected");
        assert!(err.contains("not absolute"), "unexpected error: {err}");
    }

    #[test]
    fn bare_string_relative_path_rejected() {
        let err = parse_provisioning(
            r#"
            oracle-static-dirs {
              "logs/" = "relative/path"
            }
            "#,
        )
        .expect_err("relative bare-string path must be rejected");
        assert!(err.contains("not absolute"), "unexpected error: {err}");
    }

    #[test]
    fn consensus_and_oracle_are_separate_namespaces() {
        // Same logical name in both buckets is legal — they populate
        // different principal groups.
        let cfg = parse_provisioning(
            r#"
            oracle-static-files {
              "cfg.json" = { path = "/etc/oracle/cfg.json", mode = "r" }
            }
            consensus-static-files {
              "cfg.json" = { path = "/etc/consensus/cfg.json", mode = "r" }
            }
            "#,
        )
        .expect("parse");
        assert_eq!(
            cfg.oracle_static_files.get("cfg.json").unwrap().path,
            PathBuf::from("/etc/oracle/cfg.json")
        );
        assert_eq!(
            cfg.consensus_static_files.get("cfg.json").unwrap().path,
            PathBuf::from("/etc/consensus/cfg.json")
        );
    }

    #[test]
    fn multiple_entries_per_bucket_all_parse() {
        let cfg = parse_provisioning(
            r#"
            oracle-static-files {
              "a" = "/abs/a"
              "b" = "/abs/b"
              "c" = { path = "/abs/c", mode = "r+" }
            }
            "#,
        )
        .expect("parse");
        assert_eq!(cfg.oracle_static_files.len(), 3);
        assert_eq!(cfg.oracle_static_files.get("a").unwrap().mode, "r");
        assert_eq!(cfg.oracle_static_files.get("b").unwrap().mode, "r");
        assert_eq!(cfg.oracle_static_files.get("c").unwrap().mode, "r+");
    }

    #[test]
    fn empty_path_rejected() {
        let err = parse_provisioning(
            r#"
            oracle-static-files {
              "cfg" = ""
            }
            "#,
        )
        .expect_err("empty path must be rejected");
        assert!(
            err.contains("empty") || err.contains("not absolute"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn unknown_mode_rejected() {
        let err = parse_provisioning(
            r#"
            oracle-static-files {
              "cfg" = { path = "/tmp/cfg", mode = "banana" }
            }
            "#,
        )
        .expect_err("banana isn't a mode");
        assert!(err.contains("invalid mode"), "unexpected error: {err}");
    }
}
