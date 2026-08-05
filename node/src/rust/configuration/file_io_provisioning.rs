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
use std::path::{Path, PathBuf};

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
pub(crate) const CONFIG_FILE_MODES: &[&str] = &["r", "r+", "w", "w+", "a", "a+"];

/// The whitelist of dir modes (spec §1245).
pub(crate) const CONFIG_DIR_MODES: &[&str] = &["r", "rw"];

/// Default file mode when the config entry is a bare String
/// (spec §File modes: `Default: "r"`).
pub(crate) const DEFAULT_CONFIG_FILE_MODE: &str = "r";

/// Default dir mode when the config entry is a bare String
/// (spec §1245: config is default-restrictive).
pub(crate) const DEFAULT_CONFIG_DIR_MODE: &str = "r";

/// The four spec-canonical bucket keys.  Used by
/// `check_provisioning_typos` to distinguish valid names from
/// operator typos.  `#[allow(dead_code)]`: the typo-check helper
/// is exposed as a library function for a future `--check-config`
/// CLI (slice 22 follow-up); not yet called by the boot path.
#[allow(dead_code)]
pub(crate) const KNOWN_BUCKETS: &[&str] = &[
    "oracle-static-files",
    "oracle-static-dirs",
    "consensus-static-files",
    "consensus-static-dirs",
];

/// Cap on the total number of provisioning entries across all four
/// buckets.  A hostile or corrupted config file with millions of
/// entries would otherwise OOM the loader at parse time (M-21-2).
/// 100k should comfortably exceed any legitimate operator workload
/// while bounding worst-case memory to O(few MB).
pub(crate) const MAX_PROVISIONING_ENTRIES: usize = 100_000;

/// Cap on the length of any logical-name key.  Legitimate logical
/// paths are file-system-shaped (multi-segment) but bounded; 4 KiB
/// covers deep hierarchies without allowing pathological inputs.
pub(crate) const MAX_LOGICAL_KEY_LEN: usize = 4096;

/// Syntactic path-shape checks that a config-loader can run without
/// filesystem access (i.e., before the boot-time tree walk).  Returns
/// an error message on rejection; `Ok(())` on pass.
///
/// - Rejects relative paths (spec §1273): "Every configured path must
///   be absolute."
/// - Rejects empty strings.
/// - Defensive UTF-8 check: in the current code path a `PathBuf` is
///   always constructed from a serde String and therefore always
///   valid UTF-8, so this branch is unreachable in practice.  Kept
///   so a future refactor that adds a raw-`OsString` path
///   constructor doesn't silently bypass the check (m-21-4).
pub(crate) fn validate_absolute_path(p: &Path) -> Result<(), String> {
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

/// Reject NUL, ASCII control characters (C0 U+0000-U+001F, DEL U+007F),
/// C1 controls (U+0080-U+009F), bidirectional-override marks and other
/// invisible characters that are hazardous in operator config: log
/// injection (control chars in `tracing::warn!` output), NUL truncation
/// at FFI boundaries, visual confusables (RTL overrides), and Rholang
/// lexer breakage (`\r`, form feed) when the value is spliced into the
/// composed FsGenesis source (C-25-2 slice-25 review fix).
///
/// Called by slice 22's CLI parser (originally), by slice 21's HOCON
/// deserializer (added 2026-08-03 slice-25 review C-25-2), and by
/// slice 25's `BundleEntry::try_new` as defense-in-depth.
pub(crate) fn reject_forbidden_chars(kind: &str, value: &str) -> Result<(), String> {
    if let Some((i, c)) = value.char_indices().find(|(_, c)| is_forbidden(*c)) {
        return Err(format!(
            "{kind} contains forbidden control character U+{:04X} at byte {i}",
            c as u32
        ));
    }
    Ok(())
}

fn is_forbidden(c: char) -> bool {
    let cp = c as u32;
    // NUL + C0 controls (0x00-0x1F) + DEL (0x7F)
    if cp < 0x20 || cp == 0x7F {
        return true;
    }
    // C1 controls (0x80-0x9F)
    if (0x80..=0x9F).contains(&cp) {
        return true;
    }
    // Bidirectional-override / invisible chars — visual-confusable
    // hazard in operator-facing log output, and some (LS/PS) will
    // break the Rholang lexer's line handling.
    matches!(
        cp,
        0x200E | 0x200F        // LRM, RLM
        | 0x202A..=0x202E      // LRE, RLE, PDF, LRO, RLO
        | 0x2028 | 0x2029      // LINE / PARAGRAPH SEPARATOR
        | 0xFEFF               // BOM / ZWNBSP
        | 0x2066..=0x2069      // LRI, RLI, FSI, PDI
    )
}

/// Scan a raw HOCON config text for keys that look like static-
/// provisioning bucket names (`*-static-*`) but aren't one of the
/// four canonical `KNOWN_BUCKETS`.  Returns a `Vec<String>` of
/// human-readable warnings (empty on clean input) so the caller can
/// surface them via `tracing::warn!` or a boot-log line.
///
/// M-21-1 review fix.  Because `FileIoProvisioning` is
/// `#[serde(flatten)]` into `Storage`, any unrecognized `storage {}`
/// key silently deserializes as an empty map — an operator who
/// writes `oracel-static-files { ... }` (typo) discovers the mistake
/// only when every runtime `openFile` returns `FSERR_UNSUPPORTED`.
/// This validator is the operator-facing safety net.
///
/// Scans line-by-line rather than walking the parsed `hocon::Hocon`
/// tree to avoid coupling to that crate's API surface.  Skips
/// comment lines (`#` and `//`).  Detects typos by proximity: any
/// token containing `static-file` or `static-dir` outside the
/// KNOWN_BUCKETS whitelist is flagged.
///
/// TODO: wire this into the config-loader pipeline so warnings are
/// emitted at boot; currently exposed as a library helper so
/// operators can invoke it via a `--check-config` CLI (slice 22).
/// `#[allow(dead_code)]` until that CLI wire-up lands.
#[allow(dead_code)]
pub(crate) fn check_provisioning_typos(config_hocon_text: &str) -> Vec<String> {
    let mut warnings: Vec<String> = Vec::new();
    for line in config_hocon_text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') || trimmed.starts_with("//") {
            continue;
        }
        // Split on non-key characters (whitespace, `=`, `{`, `:`,
        // `.`, quotes) to isolate individual identifier-shaped tokens.
        for token in line
            .split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
            .filter(|t| !t.is_empty())
        {
            // Two heuristics for "looks like a static-provisioning
            // bucket name":
            //   (a) contains `static-file` or `static-dir` — catches
            //       prefix typos like `oracel-static-files` and
            //       suffix typos like `consensus-static-dirsx`.
            //   (b) starts with `oracle-` or `consensus-` — catches
            //       middle-of-word typos like `consensus-satic-dirs`
            //       that don't contain either substring.
            let looks_like_bucket = token.contains("static-file")
                || token.contains("static-dir")
                || (token.starts_with("oracle-")
                    && token != "oracle-static-files"
                    && token != "oracle-static-dirs")
                || (token.starts_with("consensus-")
                    && token != "consensus-static-files"
                    && token != "consensus-static-dirs");
            if looks_like_bucket && !KNOWN_BUCKETS.contains(&token) {
                warnings.push(format!(
                    "unrecognized static-provisioning key `{token}` in config; \
                     expected one of {KNOWN_BUCKETS:?}. \
                     Entries under this key will be silently ignored — \
                     openFile / openDir calls for those logical names will \
                     return FSERR_UNSUPPORTED at runtime."
                ));
            }
        }
    }
    warnings.sort();
    warnings.dedup();
    warnings
}

/// Enforce the two size caps (M-21-2) on a parsed
/// `FileIoProvisioning`.  Called after deserialization; returns
/// `Err` with a human-readable diagnostic if either cap is
/// exceeded.  Intended to be called by the config-loader after
/// `resolve()`.
pub(crate) fn validate_size_limits(cfg: &FileIoProvisioning) -> Result<(), String> {
    let total = cfg.oracle_static_files.len()
        + cfg.oracle_static_dirs.len()
        + cfg.consensus_static_files.len()
        + cfg.consensus_static_dirs.len();
    if total > MAX_PROVISIONING_ENTRIES {
        return Err(format!(
            "total static-provisioning entries {total} exceeds cap of \
             {MAX_PROVISIONING_ENTRIES}; refuse to load rather than risk OOM"
        ));
    }
    let key_batches: [(&str, Vec<&String>); 4] = [
        (
            "oracle-static-files",
            cfg.oracle_static_files.keys().collect(),
        ),
        (
            "oracle-static-dirs",
            cfg.oracle_static_dirs.keys().collect(),
        ),
        (
            "consensus-static-files",
            cfg.consensus_static_files.keys().collect(),
        ),
        (
            "consensus-static-dirs",
            cfg.consensus_static_dirs.keys().collect(),
        ),
    ];
    for (bucket, keys) in &key_batches {
        for k in keys {
            if k.len() > MAX_LOGICAL_KEY_LEN {
                return Err(format!(
                    "logical name in `{bucket}` has length {} > cap of {MAX_LOGICAL_KEY_LEN}; \
                     truncated to first 80 chars: `{}...`",
                    k.len(),
                    &k[..80.min(k.len())]
                ));
            }
        }
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

    // C-25-2 slice-25 review fix: reject control chars in the path
    // string.  Previously enforced by slice 22's CLI parser but not
    // by HOCON deserialization — leaving an operator-config path
    // for control chars to reach `rholang_string_escape` and either
    // crash the Rholang lexer or produce ambiguous log output.
    if let Some(s) = path.to_str() {
        reject_forbidden_chars("static-provisioning path", s).map_err(D::Error::custom)?;
    }

    reject_forbidden_chars("static-provisioning mode", &mode).map_err(D::Error::custom)?;

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
        // Mi-21-5 review fix: couple the assertion to the constant so a
        // future change to DEFAULT_CONFIG_FILE_MODE trips this test.
        assert_eq!(entry.mode, DEFAULT_CONFIG_FILE_MODE);
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

    // ==================================================================
    // Review-fix additions (slice-21 whole-slice review)
    // ==================================================================

    /// M-21-3: round-trip through the full `Storage` struct where
    /// `FileIoProvisioning` is `#[serde(flatten)]`.  Ensures the
    /// flatten wiring is exercised — a future refactor that
    /// accidentally double-nests the maps under a
    /// `file-io-provisioning` sub-key would trip this test.
    #[test]
    fn round_trips_through_storage_flatten() {
        use crate::rust::configuration::model::Storage;

        let text = r#"
            data-dir = "/var/lib/rnode"
            oracle-static-files {
              "cfg.json" = "/etc/cfg.json"
            }
            consensus-static-dirs {
              "logs/" = { path = "/var/log/rnode", mode = "rw" }
            }
        "#;
        let storage: Storage = hocon::HoconLoader::new()
            .load_str(text)
            .expect("load")
            .resolve()
            .expect("resolve");
        assert_eq!(storage.data_dir, PathBuf::from("/var/lib/rnode"));
        assert_eq!(
            storage
                .file_io_provisioning
                .oracle_static_files
                .get("cfg.json")
                .unwrap()
                .path,
            PathBuf::from("/etc/cfg.json")
        );
        assert_eq!(
            storage
                .file_io_provisioning
                .consensus_static_dirs
                .get("logs/")
                .unwrap()
                .mode,
            "rw"
        );
        assert!(storage.file_io_provisioning.oracle_static_dirs.is_empty());
        assert!(storage
            .file_io_provisioning
            .consensus_static_files
            .is_empty());
    }

    /// M-21-4: object form requires the `mode` field.  A missing
    /// `mode` must fail — locking in the "no `#[serde(default)]` on
    /// mode" contract so a future add wouldn't silently change
    /// schema semantics.
    #[test]
    fn object_form_requires_mode_field() {
        let err = parse_provisioning(
            r#"
            oracle-static-files {
              "cfg" = { path = "/tmp/cfg" }
            }
            "#,
        )
        .expect_err("missing mode should fail");
        // hocon+serde-untagged doesn't include the field name in the
        // error — it just reports "data did not match any variant of
        // untagged enum Wire".  Match on that stable phrasing.
        assert!(
            err.contains("did not match any variant")
                || err.contains("mode")
                || err.contains("Wire"),
            "unexpected error: {err}"
        );
    }

    /// M-21-5a: `path` as wrong type (Int) rejected.
    #[test]
    fn wrong_type_path_int_rejected() {
        let err = parse_provisioning(
            r#"
            oracle-static-files {
              "cfg" = { path = 42, mode = "r" }
            }
            "#,
        )
        .expect_err("Int path should fail");
        assert!(!err.is_empty(), "must produce a deserialize error");
    }

    /// M-21-5b: `mode` as wrong type (Bool) rejected.
    #[test]
    fn wrong_type_mode_bool_rejected() {
        let err = parse_provisioning(
            r#"
            oracle-static-files {
              "cfg" = { path = "/tmp/cfg", mode = true }
            }
            "#,
        )
        .expect_err("Bool mode should fail");
        assert!(!err.is_empty(), "must produce a deserialize error");
    }

    /// M-21-5c: entry as an Int (neither ObjectForm nor bare-String).
    #[test]
    fn wrong_type_entry_int_rejected() {
        let err = parse_provisioning(
            r#"
            oracle-static-files {
              "cfg" = 42
            }
            "#,
        )
        .expect_err("Int entry should fail");
        assert!(!err.is_empty(), "must produce a deserialize error");
    }

    /// m-21-6: partial-map coverage — only one of four buckets set.
    #[test]
    fn one_bucket_set_others_default_empty() {
        let cfg = parse_provisioning(
            r#"
            oracle-static-dirs {
              "logs/" = "/var/log/rnode"
            }
            "#,
        )
        .expect("parse");
        assert!(cfg.oracle_static_files.is_empty());
        assert_eq!(cfg.oracle_static_dirs.len(), 1);
        assert!(cfg.consensus_static_files.is_empty());
        assert!(cfg.consensus_static_dirs.is_empty());
    }

    /// m-21-7: `Default` impl assertion (distinct from parse-empty).
    #[test]
    fn default_impl_yields_all_empty_maps() {
        let cfg = FileIoProvisioning::default();
        assert!(cfg.oracle_static_files.is_empty());
        assert!(cfg.oracle_static_dirs.is_empty());
        assert!(cfg.consensus_static_files.is_empty());
        assert!(cfg.consensus_static_dirs.is_empty());
    }

    /// m-21-5: serialize round-trip via serde_json (HOCON has no
    /// serializer).  ObjectForm-only on the write side; bare-String
    /// input degrades to Object output but the semantics are
    /// preserved.
    #[test]
    fn serialize_round_trips_through_json() {
        let original = parse_provisioning(
            r#"
            oracle-static-files {
              "cfg.json" = "/etc/cfg.json"
              "log.txt" = { path = "/var/log/x.log", mode = "a+" }
            }
            oracle-static-dirs {
              "logs/" = { path = "/var/log/", mode = "rw" }
            }
            "#,
        )
        .expect("parse");

        let json = serde_json::to_string(&original).expect("serialize");
        let restored: FileIoProvisioning = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(
            restored.oracle_static_files.get("cfg.json").unwrap().path,
            PathBuf::from("/etc/cfg.json")
        );
        assert_eq!(
            restored.oracle_static_files.get("cfg.json").unwrap().mode,
            DEFAULT_CONFIG_FILE_MODE
        );
        assert_eq!(
            restored.oracle_static_files.get("log.txt").unwrap().mode,
            "a+"
        );
        assert_eq!(restored.oracle_static_dirs.get("logs/").unwrap().mode, "rw");
    }

    /// m-21-4: `validate_absolute_path` UTF-8 branch is unreachable
    /// via the serde-String path.  Exercise it directly with a
    /// non-UTF-8 `PathBuf` (Unix-only) so a future refactor that
    /// bypasses the check doesn't go undetected.
    #[cfg(unix)]
    #[test]
    fn validate_absolute_path_rejects_non_utf8() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        // 0xFF-0xFE isn't valid UTF-8.  Prefixed with `/` so we get
        // past the absolute-path check to the UTF-8 check.
        let bad: PathBuf = {
            let mut bytes = vec![b'/'];
            bytes.extend_from_slice(&[0xFF, 0xFE]);
            PathBuf::from(OsString::from_vec(bytes))
        };
        let err = validate_absolute_path(&bad).expect_err("non-UTF-8 must be rejected");
        assert!(err.contains("UTF-8"), "unexpected error: {err}");
    }

    /// Mi-21-6: syntactically-accepted paths — long paths, unusual
    /// characters.  Existence + safety checks are deferred to the
    /// tree-walk in slice 23.
    #[test]
    fn syntactically_odd_paths_still_parse() {
        // 3 KiB path (under the 4 KiB per-key cap; the path value
        // itself has no cap in this schema).
        let long_path = format!("/{}", "a".repeat(3000));
        let text = format!(
            r#"
            oracle-static-files {{
              "cfg" = {{ path = "{long_path}", mode = "r" }}
            }}
            "#
        );
        let cfg = parse_provisioning(&text).expect("long absolute path parses");
        assert_eq!(
            cfg.oracle_static_files
                .get("cfg")
                .unwrap()
                .path
                .as_os_str()
                .len(),
            3001
        );
    }

    // ------------------------------------------------------------------
    // M-21-1 review fix: `check_provisioning_typos` validator.
    // ------------------------------------------------------------------

    #[test]
    fn typo_check_clean_config_produces_no_warnings() {
        let text = r#"
            storage {
              data-dir = "/var/lib/rnode"
              oracle-static-files { "cfg.json" = "/etc/cfg.json" }
              consensus-static-dirs { "logs/" = "/var/log/rnode" }
            }
        "#;
        assert!(check_provisioning_typos(text).is_empty());
    }

    #[test]
    fn typo_check_flags_misspelled_bucket_name() {
        let text = r#"
            storage {
              # operator meant `oracle-static-files`
              oracel-static-files { "cfg.json" = "/etc/cfg.json" }
            }
        "#;
        let warnings = check_provisioning_typos(text);
        assert_eq!(
            warnings.len(),
            1,
            "expected exactly 1 warning; got {warnings:?}"
        );
        assert!(warnings[0].contains("oracel-static-files"));
        assert!(warnings[0].contains("FSERR_UNSUPPORTED"));
    }

    #[test]
    fn typo_check_flags_several_typos() {
        let text = r#"
            storage {
              oracel-static-files { }
              consensus-satic-dirs { }
              consensus-static-dirsx { }
            }
        "#;
        let warnings = check_provisioning_typos(text);
        assert_eq!(warnings.len(), 3, "expected 3 warnings; got {warnings:?}");
    }

    #[test]
    fn typo_check_ignores_comment_lines() {
        let text = r#"
            storage {
              # oracel-static-files { "hidden" = "/x" }
              // consensus-satic-dirs { }
            }
        "#;
        assert!(check_provisioning_typos(text).is_empty());
    }

    // ------------------------------------------------------------------
    // M-21-2 review fix: `validate_size_limits`.
    // ------------------------------------------------------------------

    #[test]
    fn size_limits_pass_on_normal_config() {
        let cfg = parse_provisioning(
            r#"
            oracle-static-files {
              "a" = "/abs/a"
              "b" = "/abs/b"
            }
            "#,
        )
        .expect("parse");
        validate_size_limits(&cfg).expect("normal config passes cap check");
    }

    #[test]
    fn size_limits_reject_too_many_entries() {
        let mut cfg = FileIoProvisioning::default();
        for i in 0..=MAX_PROVISIONING_ENTRIES {
            cfg.oracle_static_files
                .insert(format!("key_{i}"), StaticFileEntry {
                    path: PathBuf::from("/x"),
                    mode: "r".to_string(),
                });
        }
        let err = validate_size_limits(&cfg).expect_err("must exceed cap");
        assert!(err.contains("exceeds cap"), "unexpected: {err}");
    }

    #[test]
    fn size_limits_reject_overlong_key() {
        let mut cfg = FileIoProvisioning::default();
        let huge_key: String = "a".repeat(MAX_LOGICAL_KEY_LEN + 1);
        cfg.oracle_static_files.insert(huge_key, StaticFileEntry {
            path: PathBuf::from("/x"),
            mode: "r".to_string(),
        });
        let err = validate_size_limits(&cfg).expect_err("must exceed key length cap");
        assert!(err.contains("> cap"), "unexpected: {err}");
    }
}
