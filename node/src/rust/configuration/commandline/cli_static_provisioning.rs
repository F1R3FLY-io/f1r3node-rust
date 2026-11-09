//! File I/O FIP static-provisioning CLI value-parsing types.
//!
//! Types + parsers for the four `--*-static-{file,dir}` CLI flags.
//! ops-9 wires these into `clap` as value-parsers; this slice
//! lands just the types + the string-level parsing so the merge
//! layer in ops-7c can consume `Vec<CliStatic{File,Dir}Arg>` from
//! the eventual clap plumbing or any other call site.
//!
//! Value syntax (per flag): `<logical-name>=<value>` where
//! `<value>` is either:
//!
//!   * A JSON-quoted bare path with default mode:
//!     `--oracle-static-dir '"logs/"="/var/log/rnode"'`
//!   * A JSON object with explicit mode:
//!     `--oracle-static-file '"cfg.json"={"path":"/etc/cfg.json","mode":"r+"}'`
//!   * An unquoted bare path for operator ergonomics
//!     (`--oracle-static-file cfg=/etc/cfg.json`).
//!
//! The logical-name LHS is a JSON-quoted string (spec examples all
//! quote it because multi-segment logical paths contain `/`); bare
//! identifiers on the LHS are also accepted for the single-segment
//! case.  `split_name_value` is JSON-aware for the LHS: when the
//! arg begins with `"`, the split point is the `=` immediately
//! following the matching close quote — so a logical name
//! containing an escaped `=` (e.g., `"a=b"=/x`) parses correctly.
//!
//! Defaults diverge between surfaces:
//!
//!   * Dir: config default `"r"` (default-restrictive deployment
//!     YAML); CLI default `"r+"` (interactive operator opts into
//!     write by default).
//!   * File: `"r"` on both surfaces.

use std::path::{Component, Path, PathBuf};

use crate::rust::configuration::file_io_provisioning::{
    reject_forbidden_chars, validate_absolute_path, StaticDirEntry, StaticFileEntry,
    CONFIG_DIR_MODES, CONFIG_FILE_MODES, DEFAULT_CONFIG_FILE_MODE, MAX_LOGICAL_KEY_LEN,
};

/// CLI-surface default for `--*-static-dir`.  Diverges from
/// `DEFAULT_CONFIG_DIR_MODE = "r"` per spec §1252: interactive
/// operator usage opts into read/write by default; deployment YAML
/// stays default-restrictive.
pub(crate) const DEFAULT_CLI_DIR_MODE: &str = "r+";

/// CLI-surface default for `--*-static-file`.  Same as config.
pub(crate) const DEFAULT_CLI_FILE_MODE: &str = DEFAULT_CONFIG_FILE_MODE;

/// One CLI static-file entry — `(logical_name, StaticFileEntry)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliStaticFileArg {
    pub logical_name: String,
    pub entry: StaticFileEntry,
}

/// One CLI static-dir entry — `(logical_name, StaticDirEntry)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliStaticDirArg {
    pub logical_name: String,
    pub entry: StaticDirEntry,
}

/// Split a CLI value into `(lhs, rhs)` at the `=` separator.
///
/// JSON-aware for the LHS: if the arg begins with `"`, the closing
/// quote is located (respecting `\"` escapes) and the `=`
/// immediately after (allowing whitespace) is the split point.
/// Otherwise, the first `=` in the arg is the split point.  This
/// matters because a logical name containing an escaped `=` would
/// otherwise be split at the wrong offset.
fn split_name_value(arg: &str) -> Result<(&str, &str), String> {
    let leading_ws = arg.len() - arg.trim_start().len();
    let rest = &arg[leading_ws..];
    if rest.starts_with('"') {
        let bytes = rest.as_bytes();
        let mut i = 1;
        let mut escaped = false;
        loop {
            if i >= bytes.len() {
                return Err(format!("unterminated quoted logical name in {arg:?}"));
            }
            let b = bytes[i];
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                break;
            }
            i += 1;
        }
        let end_quote_abs = leading_ws + i;
        let mut j = end_quote_abs + 1;
        let arg_bytes = arg.as_bytes();
        while j < arg_bytes.len() && arg_bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if j >= arg_bytes.len() || arg_bytes[j] != b'=' {
            return Err(format!("expected `=` after quoted logical name in {arg:?}"));
        }
        Ok((&arg[..=end_quote_abs], &arg[j + 1..]))
    } else {
        let eq = arg
            .find('=')
            .ok_or_else(|| format!("expected `<name>=<value>`, got {arg:?}"))?;
        let (lhs, rhs) = arg.split_at(eq);
        Ok((lhs, &rhs[1..]))
    }
}

/// Parse the LHS logical-name.  Accepts a JSON-quoted string
/// (`"path/with/slashes"`) or a bare identifier (`config-theme`).
///
/// Rejects empty names, oversize names past `MAX_LOGICAL_KEY_LEN`,
/// and forbidden chars in the decoded name (same codepoint set the
/// HOCON surface rejects — see `reject_forbidden_chars` in
/// `file_io_provisioning`).
fn parse_logical_name(lhs: &str) -> Result<String, String> {
    let trimmed = lhs.trim();
    if trimmed.is_empty() {
        return Err("logical name is empty".into());
    }
    let decoded = if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        serde_json::from_str::<String>(trimmed)
            .map_err(|e| format!("logical name {trimmed:?} isn't a valid JSON string: {e}"))?
    } else {
        trimmed.to_string()
    };
    if decoded.is_empty() {
        return Err("logical name decodes to empty string".into());
    }
    if decoded.len() > MAX_LOGICAL_KEY_LEN {
        return Err(format!(
            "logical name is {} bytes; exceeds MAX_LOGICAL_KEY_LEN ({MAX_LOGICAL_KEY_LEN})",
            decoded.len()
        ));
    }
    reject_forbidden_chars("logical name", &decoded)?;
    Ok(decoded)
}

/// Parse the RHS value.  Returns `(path, Option<mode>)` — an
/// explicit `mode` from object form, or `None` for bare-String
/// form (caller supplies the surface-appropriate default).
///
/// Rejects JSON `null`/`true`/`false`/array shapes and unquoted
/// bare paths containing `=` (almost always an operator typo:
/// missed quote/brace).  Object form uses `#[serde(deny_unknown_fields)]`
/// so a misspelled `"nmode"` doesn't silently fall to the default.
fn parse_value(rhs: &str) -> Result<(PathBuf, Option<String>), String> {
    let trimmed = rhs.trim();
    if trimmed.is_empty() {
        return Err("value is empty".into());
    }
    if trimmed.starts_with('{') {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            path: String,
            mode: String,
        }
        let w: Wire = serde_json::from_str(trimmed)
            .map_err(|e| format!("value {trimmed:?} isn't a valid `{{path, mode}}` object: {e}"))?;
        Ok((PathBuf::from(w.path), Some(w.mode)))
    } else if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        let s: String = serde_json::from_str(trimmed)
            .map_err(|e| format!("value {trimmed:?} isn't a valid JSON string: {e}"))?;
        Ok((PathBuf::from(s), None))
    } else if trimmed.starts_with('[')
        || trimmed == "null"
        || trimmed == "true"
        || trimmed == "false"
    {
        Err(format!(
            "value {trimmed:?} must be a JSON string or `{{path, mode}}` object; \
             scalars/arrays aren't accepted"
        ))
    } else if trimmed.contains('=') {
        Err(format!(
            "unquoted bare-path value {trimmed:?} contains `=`; wrap the value in quotes \
             or use object form to avoid ambiguity"
        ))
    } else {
        Ok((PathBuf::from(trimmed), None))
    }
}

/// Common validation applied to both file and dir CLI entries.
/// Delegates absolute/UTF-8 checks to ops-6a's
/// `validate_absolute_path`, then adds CLI-specific hardening:
///
///   * Reject `..` components.  They defeat lexical dedup in the
///     merge layer (ops-7c) and offer no legitimate value.
///   * Reject forbidden chars in the path string (shared gate with
///     the HOCON surface).
///   * Reject modes outside the surface whitelist.
fn validate(path: &Path, mode: &str, mode_whitelist: &[&str]) -> Result<(), String> {
    validate_absolute_path(path)?;
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(format!(
            "path {path:?} contains `..` component; static provisioning requires \
             a lexically-canonical path"
        ));
    }
    let s = path.to_str().expect("validate_absolute_path proved UTF-8");
    reject_forbidden_chars("path", s)?;
    if !mode_whitelist.contains(&mode) {
        return Err(format!(
            "invalid mode {mode:?}; allowed modes are {mode_whitelist:?}"
        ));
    }
    Ok(())
}

/// Parse one `--*-static-file` CLI value into a `CliStaticFileArg`.
/// Intended as a clap value-parser (ops-9), also directly usable
/// from unit tests + any ad-hoc caller.
pub fn parse_cli_static_file(arg: &str) -> Result<CliStaticFileArg, String> {
    let (lhs, rhs) = split_name_value(arg)?;
    let logical_name = parse_logical_name(lhs)?;
    let (path, mode_opt) = parse_value(rhs)?;
    let mode = mode_opt.unwrap_or_else(|| DEFAULT_CLI_FILE_MODE.to_string());
    validate(&path, &mode, CONFIG_FILE_MODES)?;
    Ok(CliStaticFileArg {
        logical_name,
        entry: StaticFileEntry { path, mode },
    })
}

/// Parse one `--*-static-dir` CLI value into a `CliStaticDirArg`.
pub fn parse_cli_static_dir(arg: &str) -> Result<CliStaticDirArg, String> {
    let (lhs, rhs) = split_name_value(arg)?;
    let logical_name = parse_logical_name(lhs)?;
    let (path, mode_opt) = parse_value(rhs)?;
    let mode = mode_opt.unwrap_or_else(|| DEFAULT_CLI_DIR_MODE.to_string());
    validate(&path, &mode, CONFIG_DIR_MODES)?;
    Ok(CliStaticDirArg {
        logical_name,
        entry: StaticDirEntry { path, mode },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- happy paths ----------

    #[test]
    fn bare_string_value_defaults_file_mode_to_r() {
        let a = parse_cli_static_file(r#""cfg.json"=/etc/cfg.json"#).unwrap();
        assert_eq!(a.logical_name, "cfg.json");
        assert_eq!(a.entry.path, PathBuf::from("/etc/cfg.json"));
        assert_eq!(a.entry.mode, "r");
    }

    #[test]
    fn bare_string_value_defaults_dir_mode_to_rplus() {
        let a = parse_cli_static_dir(r#""logs/"=/var/log/rnode"#).unwrap();
        assert_eq!(a.logical_name, "logs/");
        assert_eq!(a.entry.path, PathBuf::from("/var/log/rnode"));
        assert_eq!(a.entry.mode, "r+");
    }

    #[test]
    fn object_value_sets_file_mode_explicitly() {
        let a =
            parse_cli_static_file(r#""cfg.json"={"path":"/etc/cfg.json","mode":"r+"}"#).unwrap();
        assert_eq!(a.entry.mode, "r+");
    }

    #[test]
    fn object_value_sets_dir_mode_to_r_readonly() {
        let a = parse_cli_static_dir(r#""readonly/"={"path":"/srv/readonly","mode":"r"}"#).unwrap();
        assert_eq!(a.entry.mode, "r");
    }

    #[test]
    fn bare_identifier_lhs_without_quotes_accepted() {
        let a = parse_cli_static_file("cfg=/etc/cfg.json").unwrap();
        assert_eq!(a.logical_name, "cfg");
    }

    #[test]
    fn unquoted_bare_path_rhs_accepted() {
        let a = parse_cli_static_file("cfg=/etc/cfg.json").unwrap();
        assert_eq!(a.entry.path, PathBuf::from("/etc/cfg.json"));
    }

    // ---------- JSON-aware LHS split ----------

    #[test]
    fn logical_name_with_escaped_equals_parses_correctly() {
        let a = parse_cli_static_file(r#""a=b"=/x/y"#).unwrap();
        assert_eq!(a.logical_name, "a=b");
        assert_eq!(a.entry.path, PathBuf::from("/x/y"));
    }

    #[test]
    fn multi_segment_logical_name_parses() {
        let a = parse_cli_static_file(r#""config/theme.json"=/etc/theme.json"#).unwrap();
        assert_eq!(a.logical_name, "config/theme.json");
    }

    // ---------- rejections: name LHS ----------

    #[test]
    fn empty_logical_name_rejected() {
        let err = parse_cli_static_file(r#"""=/x"#).expect_err("empty name should reject");
        assert!(err.contains("empty"), "got {err:?}");
    }

    #[test]
    fn missing_equals_rejected() {
        let err = parse_cli_static_file("just_name").expect_err("missing `=` should reject");
        assert!(err.contains("expected `<name>=<value>`"), "got {err:?}");
    }

    #[test]
    fn unterminated_quoted_name_rejected() {
        let err =
            parse_cli_static_file(r#""oops=/x"#).expect_err("unterminated quote should reject");
        assert!(err.contains("unterminated"), "got {err:?}");
    }

    #[test]
    fn oversized_logical_name_rejected() {
        let big = "x".repeat(MAX_LOGICAL_KEY_LEN + 1);
        let arg = format!("{big}=/x");
        let err = parse_cli_static_file(&arg).expect_err("oversized name should reject");
        assert!(err.contains("MAX_LOGICAL_KEY_LEN"), "got {err:?}");
    }

    // ---------- rejections: value RHS ----------

    #[test]
    fn relative_path_rejected() {
        let err =
            parse_cli_static_file("cfg=relative/path").expect_err("relative path should reject");
        assert!(err.contains("not absolute"), "got {err:?}");
    }

    #[test]
    fn parent_dir_component_rejected() {
        let err =
            parse_cli_static_file("cfg=/etc/../cfg").expect_err("`..` component should reject");
        assert!(err.contains("`..` component"), "got {err:?}");
    }

    #[test]
    fn invalid_file_mode_rejected() {
        let err = parse_cli_static_file(r#"cfg={"path":"/etc/cfg","mode":"rwx"}"#)
            .expect_err("invalid mode should reject");
        assert!(
            err.contains("invalid mode") && err.contains("rwx"),
            "got {err:?}"
        );
    }

    #[test]
    fn invalid_dir_mode_rejected() {
        let err = parse_cli_static_dir(r#"d={"path":"/srv/d","mode":"w"}"#)
            .expect_err("non-dir-attenuation mode should reject");
        assert!(err.contains("invalid mode"), "got {err:?}");
    }

    #[test]
    fn unknown_field_in_object_rejected() {
        let err = parse_cli_static_file(r#"cfg={"path":"/x","nmode":"r"}"#)
            .expect_err("unknown field should reject");
        assert!(
            err.to_lowercase().contains("unknown") || err.contains("nmode"),
            "got {err:?}"
        );
    }

    #[test]
    fn null_value_rejected() {
        let err = parse_cli_static_file("cfg=null").expect_err("null should reject");
        assert!(err.contains("must be a JSON string"), "got {err:?}");
    }

    #[test]
    fn unquoted_value_with_equals_rejected() {
        let err =
            parse_cli_static_file("cfg=/x=/y").expect_err("unquoted `=` in value should reject");
        assert!(err.contains("contains `=`"), "got {err:?}");
    }

    // ---------- forbidden chars ----------

    /// Control chars inside a JSON-quoted logical name are
    /// rejected.  serde_json's string parser fires first on raw
    /// controls (U+0000-U+001F); our own `reject_forbidden_chars`
    /// is the fallback for bare-identifier LHS.  Either path
    /// produces a diagnostic naming the hazard.
    #[test]
    fn control_char_in_logical_name_rejected_via_quoted() {
        let arg = format!("\"a{}b\"=/x", '\u{0007}');
        let err = parse_cli_static_file(&arg).expect_err("control char should reject");
        assert!(
            err.to_lowercase().contains("control character"),
            "got {err:?}"
        );
    }

    /// Bare-identifier LHS reaches our own `reject_forbidden_chars`
    /// path, surfacing the shared diagnostic string.
    #[test]
    fn control_char_in_bare_logical_name_rejected() {
        let arg = format!("a{}b=/x", '\u{0007}');
        let err = parse_cli_static_file(&arg).expect_err("control char should reject");
        assert!(err.contains("forbidden control character"), "got {err:?}");
    }

    #[test]
    fn control_char_in_path_rejected() {
        let arg = format!("cfg=/a{}b", '\u{0001}');
        let err = parse_cli_static_file(&arg).expect_err("control char in path should reject");
        assert!(err.contains("forbidden control character"), "got {err:?}");
    }
}
