//! File I/O FIP static-provisioning config schema.
//!
//! Carves out `storage.{oracle,consensus}-static-{files,dirs}` as a
//! typed HOCON block so operator config can declare which host paths
//! are provisioned into the Fs cap bundle at genesis time.  Current
//! surface: typed deserialization (object form + bare-String
//! shorthand), mode-string whitelist validation, operator-ergonomic
//! typo detector, size-limits DoS guard.  Subsequent slices add:
//!
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
//! Mode-string validation runs at deserialization time (ops-3):
//! files whitelist against POSIX minus exclusive-create
//! (`r / r+ / w / w+ / a / a+`); dirs whitelist against `r / r+`.
//! Invalid modes produce a serde deserialization error naming the
//! allowed set.
//!
//! Path-content validation (ops-6a) runs at deserialization time
//! too: paths must be absolute UTF-8, and both path and mode
//! strings must not contain NUL / ASCII controls / C1 controls /
//! bidi overrides / BOM / line-paragraph separators (log-injection,
//! Rholang-lexer, and FFI-NUL-truncation hazards).  Boot-time tree
//! validation (symlink / hard-link walk) still lives in a
//! follow-up slice.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

/// The four spec-canonical bucket keys.  Used by
/// `check_provisioning_typos` to distinguish valid names from
/// operator typos.  `#[allow(dead_code)]` until the typo-check
/// helper is wired into the config-loader pipeline (ops-7/8); kept
/// here so a future `--check-config` CLI can invoke it without a
/// cross-crate dep.
#[allow(dead_code)]
pub(crate) const KNOWN_BUCKETS: &[&str] = &[
    "oracle-static-files",
    "oracle-static-dirs",
    "consensus-static-files",
    "consensus-static-dirs",
];

/// Cap on the total number of provisioning entries across all four
/// buckets.  A hostile or corrupted config file with millions of
/// entries would otherwise OOM the loader at parse time.  100k
/// comfortably exceeds any legitimate operator workload while
/// bounding worst-case memory to a few MB.
pub(crate) const MAX_PROVISIONING_ENTRIES: usize = 100_000;

/// Cap on the length of any logical-name key.  Legitimate logical
/// paths are file-system-shaped (multi-segment) but bounded; 4 KiB
/// covers deep hierarchies without allowing pathological inputs.
pub(crate) const MAX_LOGICAL_KEY_LEN: usize = 4096;

/// Accepted file-mode whitelist for config-block entries.  Spec
/// §File modes enumerates eight POSIX forms:
/// `r / r+ / w / w+ / wx / w+x / a / a+`.  The exclusive-create
/// pair `wx` / `w+x` is rejected in config form per spec §1281:
/// their semantics require the file to NOT exist at open time,
/// which contradicts the config's existence check at boot
/// validation.  The remaining six are accepted.
pub(crate) const CONFIG_FILE_MODES: &[&str] = &["r", "r+", "w", "w+", "a", "a+"];

/// Accepted dir-attenuation whitelist for config-block entries.
/// Spec §1245: a Dir cap is either read-only (`"r"`) or
/// read/write (`"r+"`).  No other values are accepted.
pub(crate) const CONFIG_DIR_MODES: &[&str] = &["r", "r+"];

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

/// Validate that `p` is a non-empty absolute UTF-8 path, per
/// spec §1273-1274: "Every configured path must be absolute" and
/// paths must be UTF-8 (necessary for the Rholang bundle-source
/// splice downstream, and for log-safe reporting).
///
/// The UTF-8 branch is defensive: in the current pipeline a
/// `PathBuf` is always constructed from a serde `String` and is
/// therefore always valid UTF-8.  The check is kept so a future
/// refactor that admits raw `OsString` doesn't silently bypass it.
pub(crate) fn validate_absolute_path(p: &Path) -> Result<(), String> {
    if p.as_os_str().is_empty() {
        return Err("path is empty".into());
    }
    if !p.is_absolute() {
        return Err(format!(
            "path {p:?} is not absolute; static provisioning requires absolute host paths (spec §1273)"
        ));
    }
    if p.to_str().is_none() {
        return Err(format!(
            "path {p:?} is not valid UTF-8; spec §1274 requires UTF-8 paths"
        ));
    }
    Ok(())
}

/// Reject NUL, ASCII C0 controls (U+0000–U+001F), DEL (U+007F),
/// C1 controls (U+0080–U+009F), bidirectional-override marks,
/// line/paragraph separators, and BOM.  Operator-config values
/// pass through `tracing::warn!` output, FFI boundaries, and the
/// Rholang bundle-source splice; a control char in any of those
/// contexts is a log-injection, NUL-truncation, visual-confusable,
/// or lexer-break hazard.
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
    if cp < 0x20 || cp == 0x7F {
        return true;
    }
    if (0x80..=0x9F).contains(&cp) {
        return true;
    }
    matches!(
        cp,
        0x200E | 0x200F
        | 0x202A..=0x202E
        | 0x2028 | 0x2029
        | 0xFEFF
        | 0x2066..=0x2069
    )
}

/// Shared deserialize-either-shape helper.  Returns the raw
/// `(path, mode)` pair with `default_mode` substituted when the
/// entry is bare-String.  Runs three validations in sequence:
///
///   1. `validate_absolute_path` on the parsed path (spec §1273).
///   2. `reject_forbidden_chars` on the path's UTF-8 string.
///   3. `reject_forbidden_chars` on the mode string.
///   4. Mode whitelist (`mode_whitelist`).
///
/// Any failure produces a `D::Error::custom` describing the
/// offending value.
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

    if let Some(s) = path.to_str() {
        reject_forbidden_chars("static-provisioning path", s).map_err(D::Error::custom)?;
    }

    reject_forbidden_chars("static-provisioning mode", &mode).map_err(D::Error::custom)?;

    if !mode_whitelist.contains(&mode.as_str()) {
        return Err(D::Error::custom(format!(
            "invalid mode {mode:?}; allowed modes are {mode_whitelist:?}"
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

/// Operator-ergonomic typo detector.  Because `FileIoProvisioning`
/// is designed to `#[serde(flatten)]` into the surrounding `Storage`
/// config (ops-5), any unrecognized `storage {}` key silently
/// deserializes as an empty map — an operator who writes
/// `oracel-static-files { ... }` (typo) discovers the mistake only
/// when every runtime `openFile` returns `FSERR_UNSUPPORTED`.
///
/// Scans line-by-line rather than walking the parsed `hocon::Hocon`
/// tree to avoid coupling to that crate's API surface.  Skips
/// comment lines (`#` and `//`).  Detects typos by proximity:
///
///   (a) tokens containing `static-file` or `static-dir` that
///       aren't in `KNOWN_BUCKETS` (catches prefix typos like
///       `oracel-static-files` and suffix typos like
///       `consensus-static-dirsx`).
///   (b) tokens starting with `oracle-` or `consensus-` that
///       aren't one of the four canonical buckets (catches
///       middle-of-word typos like `consensus-satic-dirs` that
///       don't contain either substring).
///
/// Returns a de-duplicated, sorted vector of warning strings.  The
/// typo-check helper is exposed as a library function for a future
/// `--check-config` CLI (ops-7/8 wire-up); not yet called by the
/// boot path, hence the module-level `#[allow(dead_code)]`.
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

/// Enforce the two size caps on a parsed `FileIoProvisioning`.
/// Called after deserialization; returns `Err` with a human-readable
/// diagnostic if either cap is exceeded.  Intended to be called by
/// the config-loader after `resolve()` in a follow-up slice;
/// `#[allow(dead_code)]` until that wire-up lands.
///
///   1. Total entries across all four buckets ≤ MAX_PROVISIONING_ENTRIES.
///   2. Each logical-name key length ≤ MAX_LOGICAL_KEY_LEN.
#[allow(dead_code)]
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
                     truncated preview: `{}...`",
                    k.len(),
                    // Char-boundary-safe truncate: byte-slicing at a
                    // raw byte offset panics if the offset lands
                    // mid-UTF-8-codepoint.  `chars().take()` walks
                    // codepoints so a hostile key with multi-byte
                    // chars produces a clean error instead of a crash.
                    k.chars().take(80).collect::<String>()
                ));
            }
        }
    }
    Ok(())
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

    /// Every accepted file mode in the whitelist (`r / r+ / w / w+
    /// / a / a+`) deserializes.  Catches a whitelist typo or an
    /// accidental removal.
    #[test]
    fn every_whitelisted_file_mode_parses() {
        for mode in ["r", "r+", "w", "w+", "a", "a+"] {
            let hocon = format!(
                r#"
                oracle-static-files {{
                  "n" = {{ path = "/p", mode = "{mode}" }}
                }}
                "#
            );
            let cfg = parse_provisioning(&hocon)
                .unwrap_or_else(|e| panic!("mode {mode:?} should parse but got {e}"));
            assert_eq!(
                cfg.oracle_static_files.get("n").map(|e| e.mode.as_str()),
                Some(mode)
            );
        }
    }

    /// Exclusive-create file modes (`wx`, `w+x`) are rejected in
    /// config form per spec §1281 — their semantics require the
    /// file to NOT exist, contradicting the boot-time existence
    /// check.
    #[test]
    fn exclusive_create_file_modes_rejected() {
        for mode in ["wx", "w+x"] {
            let hocon = format!(
                r#"
                oracle-static-files {{
                  "n" = {{ path = "/p", mode = "{mode}" }}
                }}
                "#
            );
            let err = parse_provisioning(&hocon)
                .err()
                .unwrap_or_else(|| panic!("mode {mode:?} should reject"));
            assert!(
                err.contains("invalid mode") && err.contains(mode),
                "error message must name the invalid mode; got {err:?}"
            );
        }
    }

    /// Arbitrary non-POSIX mode strings on files are rejected
    /// (`"rwx"`, `"xyz"`, empty, etc.).  Pins the whitelist as a
    /// closed set.
    #[test]
    fn non_posix_file_modes_rejected() {
        for mode in ["rwx", "xyz", "", "rw", "R", "r++"] {
            let hocon = format!(
                r#"
                oracle-static-files {{
                  "n" = {{ path = "/p", mode = "{mode}" }}
                }}
                "#
            );
            assert!(
                parse_provisioning(&hocon).is_err(),
                "mode {mode:?} should reject but parsed"
            );
        }
    }

    /// Both accepted dir attenuation keywords deserialize.
    #[test]
    fn every_whitelisted_dir_mode_parses() {
        for mode in ["r", "r+"] {
            let hocon = format!(
                r#"
                consensus-static-dirs {{
                  "d/" = {{ path = "/p", mode = "{mode}" }}
                }}
                "#
            );
            let cfg = parse_provisioning(&hocon)
                .unwrap_or_else(|e| panic!("dir mode {mode:?} should parse but got {e}"));
            assert_eq!(
                cfg.consensus_static_dirs.get("d/").map(|e| e.mode.as_str()),
                Some(mode)
            );
        }
    }

    /// Any mode outside the dir-attenuation whitelist is rejected
    /// — including POSIX file modes that aren't valid Dir
    /// attenuation keywords (`"w"`, `"a+"`, etc.).
    #[test]
    fn non_dir_attenuation_dir_modes_rejected() {
        for mode in ["w", "w+", "a", "a+", "wx", "w+x", "rw", "rwx", ""] {
            let hocon = format!(
                r#"
                oracle-static-dirs {{
                  "d/" = {{ path = "/p", mode = "{mode}" }}
                }}
                "#
            );
            assert!(
                parse_provisioning(&hocon).is_err(),
                "dir mode {mode:?} should reject but parsed"
            );
        }
    }

    /// Relative paths are rejected at deserialization.  Covers
    /// bare-String and object-form entries, files and dirs.
    #[test]
    fn relative_path_rejected() {
        for hocon in [
            r#"oracle-static-files { "n" = "etc/foo" }"#,
            r#"oracle-static-files { "n" = { path = "etc/foo", mode = "r" } }"#,
            r#"oracle-static-dirs  { "d/" = "etc/foo" }"#,
            r#"consensus-static-dirs { "d/" = { path = "etc/foo", mode = "r+" } }"#,
        ] {
            let err = parse_provisioning(hocon)
                .err()
                .unwrap_or_else(|| panic!("relative path should reject but parsed: {hocon}"));
            assert!(
                err.contains("is not absolute"),
                "error must name the invariant; got {err:?}"
            );
        }
    }

    /// Empty paths are rejected at deserialization.
    #[test]
    fn empty_path_rejected() {
        let err = parse_provisioning(r#"oracle-static-files { "n" = "" }"#)
            .expect_err("empty path should reject");
        assert!(
            err.contains("path is empty"),
            "error must name the empty-path invariant; got {err:?}"
        );
    }

    /// NUL and ASCII control characters in the path value are
    /// rejected (log-injection + FFI-NUL-truncation hazards).
    /// Uses HOCON `\u` escapes so the control char reaches the
    /// deserializer without breaking HOCON syntax.
    #[test]
    fn control_char_in_path_rejected() {
        for esc in ["\\u0000", "\\u001B", "\\u007F", "\\u000A"] {
            let hocon = format!(r#"oracle-static-files {{ "n" = "/a/{esc}b" }}"#);
            let err = parse_provisioning(&hocon)
                .err()
                .unwrap_or_else(|| panic!("control-char path {esc} should reject"));
            assert!(
                err.contains("forbidden control character")
                    && err.contains("static-provisioning path"),
                "error must name the invariant and the field; got {err:?}"
            );
        }
    }

    /// Bidirectional-override characters in the path are rejected
    /// (visual-confusable hazard in operator-facing logs).
    #[test]
    fn bidi_override_in_path_rejected() {
        let hocon = format!(
            "oracle-static-files {{ \"n\" = \"/a/{}b\" }}",
            '\u{202E}' // RIGHT-TO-LEFT OVERRIDE
        );
        let err = parse_provisioning(&hocon).expect_err("bidi-override path should reject");
        assert!(
            err.contains("forbidden control character"),
            "error must name the invariant; got {err:?}"
        );
    }

    /// Control characters in the mode string are rejected.  The
    /// mode whitelist already rejects most such strings as "not a
    /// POSIX mode", but a mode of `"r\0"` would also smuggle a NUL
    /// through FFI boundaries if the whitelist check happened first.
    /// Pins the ordering: forbidden-char check runs before whitelist.
    #[test]
    fn control_char_in_mode_rejected() {
        let hocon = r#"oracle-static-files { "n" = { path = "/p", mode = "r " } }"#;
        let err = parse_provisioning(hocon).expect_err("control-char mode should reject");
        assert!(
            err.contains("forbidden control character") && err.contains("static-provisioning mode"),
            "error must name the invariant and the field; got {err:?}"
        );
    }

    /// `validate_absolute_path` happy path on a plain absolute UTF-8
    /// path.  Pins that the three gates (empty, non-absolute,
    /// non-UTF-8) all pass on a clean input.
    #[test]
    fn validate_absolute_path_accepts_absolute_utf8() {
        assert!(validate_absolute_path(Path::new("/etc/myapp/full.json")).is_ok());
    }

    /// `reject_forbidden_chars` happy path on a plain ASCII mode
    /// string.  Pins that the matcher doesn't false-positive on
    /// legitimate values.
    #[test]
    fn reject_forbidden_chars_accepts_ascii_mode() {
        for mode in ["r", "r+", "w", "w+", "a", "a+"] {
            assert!(reject_forbidden_chars("mode", mode).is_ok());
        }
    }

    /// Known bucket names produce no warnings.  Pins that the
    /// detector doesn't flag the canonical keys as typos.
    #[test]
    fn typo_detector_passes_clean_config() {
        let warnings = check_provisioning_typos(
            r#"
            oracle-static-files   { "n" = "/a" }
            oracle-static-dirs    { "d/" = "/b" }
            consensus-static-files { "n" = "/c" }
            consensus-static-dirs { "d/" = "/d" }
            "#,
        );
        assert!(
            warnings.is_empty(),
            "clean config should produce no warnings, got {warnings:?}"
        );
    }

    /// Prefix typos (`oracel-...`) are flagged.
    #[test]
    fn typo_detector_flags_prefix_typo() {
        let warnings = check_provisioning_typos(
            r#"
            oracel-static-files { "n" = "/a" }
            "#,
        );
        assert_eq!(warnings.len(), 1, "expected one warning, got {warnings:?}");
        assert!(
            warnings[0].contains("oracel-static-files"),
            "warning must name the offending token: {}",
            warnings[0]
        );
    }

    /// Suffix typos (`consensus-static-dirsx`) are flagged.
    #[test]
    fn typo_detector_flags_suffix_typo() {
        let warnings = check_provisioning_typos(
            r#"
            consensus-static-dirsx { "d/" = "/a" }
            "#,
        );
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("consensus-static-dirsx"));
    }

    /// Middle-of-word typos (`consensus-satic-dirs`) are flagged
    /// via the `starts_with("consensus-")` heuristic (the token
    /// doesn't contain `static-dir` so the first heuristic misses).
    #[test]
    fn typo_detector_flags_middle_typo() {
        let warnings = check_provisioning_typos(
            r#"
            consensus-satic-dirs { "d/" = "/a" }
            "#,
        );
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("consensus-satic-dirs"));
    }

    /// Comment lines (`#` and `//`) are skipped — an operator can
    /// paste examples into comments without tripping the detector.
    #[test]
    fn typo_detector_skips_comment_lines() {
        let warnings = check_provisioning_typos(
            r#"
            # oracel-static-files { "n" = "/a" }
            // consensus-satic-dirs { "d/" = "/b" }
            "#,
        );
        assert!(
            warnings.is_empty(),
            "comment-wrapped typos should be ignored, got {warnings:?}"
        );
    }

    /// Duplicate occurrences of the same typo are de-duplicated to
    /// a single warning so a config with ten entries under one
    /// typo'd bucket doesn't produce ten identical warnings.
    #[test]
    fn typo_detector_deduplicates_repeat_tokens() {
        let warnings = check_provisioning_typos(
            r#"
            oracel-static-files {
              "a" = "/a"
              "b" = "/b"
            }
            oracel-static-files { "c" = "/c" }
            "#,
        );
        assert_eq!(
            warnings.len(),
            1,
            "repeated typo should produce one warning, got {warnings:?}"
        );
    }

    /// An empty config passes size validation.
    #[test]
    fn size_limits_accept_empty_config() {
        let cfg = FileIoProvisioning::default();
        validate_size_limits(&cfg).expect("empty config passes");
    }

    /// A config with (MAX+1) entries fails the total cap.
    #[test]
    fn size_limits_reject_total_exceeding_cap() {
        let mut cfg = FileIoProvisioning::default();
        for i in 0..=MAX_PROVISIONING_ENTRIES {
            cfg.oracle_static_files
                .insert(format!("k{i}"), StaticFileEntry {
                    path: PathBuf::from("/p"),
                    mode: "r".into(),
                });
        }
        let err = validate_size_limits(&cfg).expect_err("over-cap should fail");
        assert!(
            err.contains("exceeds cap") && err.contains(&MAX_PROVISIONING_ENTRIES.to_string()),
            "error must name the cap: {err}"
        );
    }

    /// A hostile key over the cap containing multi-byte UTF-8
    /// produces a clean error instead of a byte-slice panic.
    /// Defense-in-depth: a naive `&k[..80]` would panic if byte
    /// 80 lands mid-codepoint; the char-walking truncate avoids
    /// that.
    #[test]
    fn size_limits_reject_handles_multibyte_utf8_safely() {
        let mut cfg = FileIoProvisioning::default();
        // Each "é" is 2 bytes; MAX+1 codepoints * 2 bytes = well
        // over the byte cap.  Also, byte-offset 80 lands between
        // the two bytes of the 41st "é" codepoint — exactly the
        // raw-slice panic case.
        let hostile_key: String = "é".repeat(MAX_LOGICAL_KEY_LEN);
        cfg.oracle_static_dirs.insert(hostile_key, StaticDirEntry {
            path: PathBuf::from("/p"),
            mode: "r".into(),
        });
        let err = validate_size_limits(&cfg).expect_err("over-cap key should fail cleanly");
        assert!(err.contains("cap of"), "error must name the cap, got {err}");
    }

    /// Flatten-into-Storage wire-up: parsing a full `Storage`
    /// HOCON block carries the four provisioning buckets through
    /// `#[serde(flatten)]`.  Pins ops-5's field addition.
    #[test]
    fn storage_flatten_carries_provisioning_through() {
        use crate::rust::configuration::model::Storage;

        let hocon = r#"
            data-dir = "/var/lib/rnode"
            oracle-static-files {
              "cfg" = "/etc/myapp/cfg.json"
            }
            consensus-static-dirs {
              "shared/" = { path = "/srv/shared", mode = "r+" }
            }
        "#;
        let storage: Storage = hocon::HoconLoader::new()
            .load_str(hocon)
            .expect("load")
            .resolve()
            .expect("resolve");
        assert_eq!(storage.data_dir, PathBuf::from("/var/lib/rnode"));
        assert_eq!(
            storage
                .file_io_provisioning
                .oracle_static_files
                .get("cfg")
                .map(|e| e.mode.as_str()),
            Some("r")
        );
        assert_eq!(
            storage
                .file_io_provisioning
                .consensus_static_dirs
                .get("shared/")
                .map(|e| (e.path.as_path(), e.mode.as_str())),
            Some((std::path::Path::new("/srv/shared"), "r+"))
        );
        // Unprovisioned buckets stay empty.
        assert!(storage.file_io_provisioning.oracle_static_dirs.is_empty());
        assert!(storage
            .file_io_provisioning
            .consensus_static_files
            .is_empty());
    }

    /// `Storage` without any provisioning entries parses unchanged
    /// — operators who haven't opted into static provisioning get
    /// the default-empty `FileIoProvisioning`.
    #[test]
    fn storage_without_provisioning_parses_with_empty_default() {
        use crate::rust::configuration::model::Storage;

        let storage: Storage = hocon::HoconLoader::new()
            .load_str(r#"data-dir = "/var/lib/rnode""#)
            .expect("load")
            .resolve()
            .expect("resolve");
        assert_eq!(storage.data_dir, PathBuf::from("/var/lib/rnode"));
        assert_eq!(storage.file_io_provisioning, FileIoProvisioning::default());
    }

    /// A single key longer than MAX_LOGICAL_KEY_LEN fails even if
    /// the entry total is well under cap.
    #[test]
    fn size_limits_reject_key_longer_than_cap() {
        let mut cfg = FileIoProvisioning::default();
        let long_key = "x".repeat(MAX_LOGICAL_KEY_LEN + 1);
        cfg.consensus_static_dirs.insert(long_key, StaticDirEntry {
            path: PathBuf::from("/p"),
            mode: "r".into(),
        });
        let err = validate_size_limits(&cfg).expect_err("over-long key should fail");
        assert!(
            err.contains("consensus-static-dirs") && err.contains("cap of"),
            "error must name the bucket + cap: {err}"
        );
    }
}
