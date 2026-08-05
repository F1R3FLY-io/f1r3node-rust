//! File I/O FIP static-provisioning CLI-flag parsing (Phase 7 slice 22).
//!
//! Parses the four `--*-static-{file,dir}` flags into structured
//! `(logical_name, entry)` tuples using the same schema as
//! `file_io_provisioning`'s config-file surface, with two differences:
//!
//! 1. **Dir default mode is `"rw"`** on CLI (per spec §1252 and
//!    implementation-plan.md §Phase 7 default-modes bullet: `"rw"` for
//!    `--*-static-dir`, `"r"` for `*-static-dirs` in config).  The
//!    rationale is default-permissive on CLI (interactive operator
//!    usage) vs default-restrictive in config (deployment YAML).
//!    File default remains `"r"` on both surfaces.
//!
//! 2. Value syntax is a single string `<logical-name>=<value>` where
//!    `<value>` is either:
//!    - a bare JSON string  → path with default mode, e.g.
//!      `--oracle-static-dir '"logs/"="/var/log/rnode"'`
//!    - a JSON object       → `{path, mode}` explicit, e.g.
//!      `--oracle-static-file '"cfg.json"={"path":"/etc/cfg.json","mode":"r+"}'`
//!
//! The logical-name LHS is a JSON-quoted string (spec examples all
//! quote it because multi-segment logical paths contain `/`); bare
//! identifiers on the LHS are also accepted for the common
//! single-segment case.
//!
//! `split_name_value` is JSON-aware for the LHS: when the arg begins
//! with `"`, the split point is the `=` immediately following the
//! matching close quote — so a logical name containing an escaped `=`
//! (e.g., `"a=b"=/x`) parses correctly (M-22-1).
//!
//! Merging of CLI entries with config-file entries + duplicate
//! detection lands in slice 24.  Bundle handoff to genesis is
//! slice 25.

use std::path::{Component, Path, PathBuf};

use crate::rust::configuration::file_io_provisioning::{
    validate_absolute_path, StaticDirEntry, StaticFileEntry, CONFIG_DIR_MODES, CONFIG_FILE_MODES,
    DEFAULT_CONFIG_FILE_MODE, MAX_LOGICAL_KEY_LEN,
};

/// Default dir mode on the CLI-flag surface.  Diverges from
/// `DEFAULT_CONFIG_DIR_MODE = "r"` per spec §1252.
pub(crate) const DEFAULT_CLI_DIR_MODE: &str = "rw";

/// Default file mode on the CLI-flag surface.  Same as config
/// (`"r"`); duplicated as its own constant for symmetry / future
/// divergence.
pub(crate) const DEFAULT_CLI_FILE_MODE: &str = DEFAULT_CONFIG_FILE_MODE;

/// One CLI static-file entry — the tuple `(logical_name, entry)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliStaticFileArg {
    pub logical_name: String,
    pub entry: StaticFileEntry,
}

/// One CLI static-dir entry — `(logical_name, entry)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliStaticDirArg {
    pub logical_name: String,
    pub entry: StaticDirEntry,
}

/// Reject NUL and ASCII control characters (M-22-3).  These have no
/// legitimate use in a logical name or filesystem path we would ever
/// static-provision, and admitting them opens log-injection (control
/// chars end up in operator-facing `tracing::warn!` output), NUL-
/// truncation on FFI boundaries, and visual-confusable hazards.
fn reject_forbidden_chars(kind: &str, value: &str) -> Result<(), String> {
    if let Some((i, c)) = value
        .char_indices()
        .find(|(_, c)| *c == '\0' || (*c as u32) < 0x20 || *c == '\u{7f}')
    {
        return Err(format!(
            "{kind} contains forbidden control character U+{:04X} at byte {i}",
            c as u32
        ));
    }
    Ok(())
}

/// Split a CLI value into `(lhs, rhs)` at the `=` separator.
///
/// JSON-aware for the LHS: if the arg begins with `"`, the closing
/// quote is located (respecting `\"` escapes) and the `=` immediately
/// after (allowing whitespace) is the split point.  Otherwise, the
/// first `=` in the arg is the split point.
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
        // `i` indexes the closing `"` within `rest`.
        let end_quote_abs = leading_ws + i;
        // Skip whitespace between the close quote and the `=`.
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
/// Rejects empty names (both surfaces), oversize names past
/// `MAX_LOGICAL_KEY_LEN` (S-22-1), and NUL / control characters in
/// the decoded name (M-22-3, M-22-4).
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
/// explicit `mode` from an object form, or `None` for bare-String
/// form (caller supplies the surface-appropriate default).
///
/// Rejects JSON `null`/`true`/`false`/array shapes (S-22-5) and
/// unquoted bare paths containing `=` (M-22-1), which almost always
/// indicate an operator typo (missed quote/brace).  Object form uses
/// `#[serde(deny_unknown_fields)]` (M-22-2) so a misspelled
/// `"nmode"` doesn't silently fall to the mode default.
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
            "value {trimmed:?} must be a JSON string or `{{path, mode}}` object; scalars/arrays aren't accepted"
        ))
    } else if trimmed.contains('=') {
        Err(format!(
            "unquoted bare-path value {trimmed:?} contains `=`; wrap the value in quotes or use object form to avoid ambiguity (M-22-1)"
        ))
    } else {
        // Unquoted bare path — accepted for operator ergonomics.
        Ok((PathBuf::from(trimmed), None))
    }
}

/// Common validation applied to both file and dir CLI entries.
/// Delegates absolute/UTF-8 checks to slice 21's `validate_absolute_path`,
/// then adds CLI-specific hardening:
///  - Reject `..` components (S-22-3): they defeat lexical dedup in
///    slice 24 and offer no legitimate value.
///  - Reject NUL/control chars in the path string (M-22-3).
fn validate(path: &Path, mode: &str, mode_whitelist: &[&str]) -> Result<(), String> {
    validate_absolute_path(path)?;
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(format!(
            "path {path:?} contains `..` component; static provisioning requires a lexically-canonical path"
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

/// clap value parser for `--oracle-static-file` / `--consensus-static-file`.
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

/// clap value parser for `--oracle-static-dir` / `--consensus-static-dir`.
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

    // ------------------- FILE parser -------------------

    #[test]
    fn file_object_form_with_quoted_name() {
        let arg = r#""reports/q3/summary.csv"={"path":"/srv/data/q3.csv","mode":"r+"}"#;
        let cli = parse_cli_static_file(arg).expect("parse");
        assert_eq!(cli.logical_name, "reports/q3/summary.csv");
        assert_eq!(cli.entry.path, PathBuf::from("/srv/data/q3.csv"));
        assert_eq!(cli.entry.mode, "r+");
    }

    #[test]
    fn file_bare_json_string_defaults_to_r() {
        let arg = r#""config/theme.json"="/etc/myapp/theme.json""#;
        let cli = parse_cli_static_file(arg).expect("parse");
        assert_eq!(cli.logical_name, "config/theme.json");
        assert_eq!(cli.entry.path, PathBuf::from("/etc/myapp/theme.json"));
        assert_eq!(cli.entry.mode, DEFAULT_CLI_FILE_MODE);
        // S-22-9: couple to constant AND literal so a future change of
        // DEFAULT_CLI_FILE_MODE doesn't silently pass this test.
        assert_eq!(cli.entry.mode, "r");
    }

    #[test]
    fn file_unquoted_bare_path() {
        let arg = "cfg.json=/etc/cfg.json";
        let cli = parse_cli_static_file(arg).expect("parse");
        assert_eq!(cli.logical_name, "cfg.json");
        assert_eq!(cli.entry.path, PathBuf::from("/etc/cfg.json"));
        assert_eq!(cli.entry.mode, DEFAULT_CLI_FILE_MODE);
    }

    #[test]
    fn file_rejects_relative_path() {
        let err = parse_cli_static_file(r#""cfg"={"path":"rel/path","mode":"r"}"#)
            .expect_err("relative path must fail");
        assert!(err.contains("not absolute"), "unexpected: {err}");
    }

    #[test]
    fn file_rejects_wx_mode() {
        let err = parse_cli_static_file(r#""log"={"path":"/var/log/x","mode":"wx"}"#)
            .expect_err("wx must fail");
        assert!(err.contains("invalid mode"), "unexpected: {err}");
        // S-22-10: error message must include the offending mode.
        assert!(err.contains("wx"), "message should include 'wx': {err}");
    }

    #[test]
    fn file_rejects_w_plus_x_mode() {
        let err = parse_cli_static_file(r#""log"={"path":"/var/log/x","mode":"w+x"}"#)
            .expect_err("w+x must fail");
        assert!(err.contains("invalid mode"), "unexpected: {err}");
        assert!(err.contains("w+x"), "message should include 'w+x': {err}");
    }

    #[test]
    fn file_rejects_dir_mode_rw() {
        // "rw" is a dir mode, not a file mode.
        let err = parse_cli_static_file(r#""cfg"={"path":"/etc/cfg","mode":"rw"}"#)
            .expect_err("rw must fail for file");
        assert!(err.contains("invalid mode"), "unexpected: {err}");
    }

    #[test]
    fn file_accepts_all_valid_file_modes() {
        for mode in CONFIG_FILE_MODES {
            let arg = format!(r#""cfg"={{"path":"/tmp/cfg","mode":"{mode}"}}"#);
            let cli = parse_cli_static_file(&arg)
                .unwrap_or_else(|e| panic!("mode {mode} should parse; got {e}"));
            assert_eq!(cli.entry.mode, *mode);
        }
    }

    #[test]
    fn file_rejects_missing_equals() {
        let err = parse_cli_static_file("just-a-name").expect_err("missing = must fail");
        assert!(err.contains("expected"), "unexpected: {err}");
        // S-22-10: message should include the offending input.
        assert!(
            err.contains("just-a-name"),
            "message should echo input: {err}"
        );
    }

    #[test]
    fn file_rejects_malformed_json_object() {
        let err = parse_cli_static_file(r#""cfg"={"path":"/x", not-json}"#)
            .expect_err("malformed JSON must fail");
        assert!(
            err.contains("valid") || err.contains("object"),
            "unexpected: {err}"
        );
    }

    // ------------------- DIR parser -------------------

    #[test]
    fn dir_bare_json_string_defaults_to_rw_on_cli() {
        // Spec §1252: CLI dir default is "rw" (unlike config's "r").
        let arg = r#""reports/archive/"="/srv/data/reports-archive""#;
        let cli = parse_cli_static_dir(arg).expect("parse");
        assert_eq!(cli.logical_name, "reports/archive/");
        assert_eq!(cli.entry.path, PathBuf::from("/srv/data/reports-archive"));
        assert_eq!(cli.entry.mode, DEFAULT_CLI_DIR_MODE);
        assert_eq!(cli.entry.mode, "rw"); // couples to constant + literal
    }

    #[test]
    fn dir_object_form_accepts_explicit_r() {
        let arg = r#""readonly-dir/"={"path":"/etc/readonly","mode":"r"}"#;
        let cli = parse_cli_static_dir(arg).expect("parse");
        assert_eq!(cli.entry.mode, "r");
    }

    #[test]
    fn dir_rejects_file_mode() {
        // "r+" is a file mode, not a dir mode.
        let err = parse_cli_static_dir(r#""out/"={"path":"/var/out","mode":"r+"}"#)
            .expect_err("r+ must fail for dir");
        assert!(err.contains("invalid mode"), "unexpected: {err}");
    }

    #[test]
    fn dir_rejects_relative_path() {
        let err =
            parse_cli_static_dir(r#""logs/"="rel/logs""#).expect_err("relative path must fail");
        assert!(err.contains("not absolute"), "unexpected: {err}");
    }

    #[test]
    fn dir_unquoted_bare_path() {
        let arg = "logs=/var/log/rnode";
        let cli = parse_cli_static_dir(arg).expect("parse");
        assert_eq!(cli.logical_name, "logs");
        assert_eq!(cli.entry.path, PathBuf::from("/var/log/rnode"));
        assert_eq!(cli.entry.mode, DEFAULT_CLI_DIR_MODE);
    }

    /// S-22-4: full cross-mode matrix — every file-only mode is
    /// rejected by the dir parser; every dir-only mode is rejected by
    /// the file parser.  The shared mode `"r"` is exempt.
    #[test]
    fn cross_mode_rejection_matrix() {
        for &m in CONFIG_FILE_MODES {
            if CONFIG_DIR_MODES.contains(&m) {
                continue;
            }
            let arg = format!(r#""d/"={{"path":"/tmp","mode":"{m}"}}"#);
            parse_cli_static_dir(&arg)
                .expect_err(&format!("file-only mode {m} must fail on dir parser"));
        }
        for &m in CONFIG_DIR_MODES {
            if CONFIG_FILE_MODES.contains(&m) {
                continue;
            }
            let arg = format!(r#""cfg"={{"path":"/tmp","mode":"{m}"}}"#);
            parse_cli_static_file(&arg)
                .expect_err(&format!("dir-only mode {m} must fail on file parser"));
        }
    }

    // ------------------- shared helpers -------------------

    #[test]
    fn logical_name_json_unescapes() {
        let arg = r#""name\/with\/slashes"="/abs""#;
        let cli = parse_cli_static_file(arg).expect("parse");
        // JSON unescape converts \/ to /.
        assert_eq!(cli.logical_name, "name/with/slashes");
    }

    #[test]
    fn empty_value_rejected() {
        let err = parse_cli_static_file(r#""cfg"="#).expect_err("empty value must fail");
        assert!(err.contains("empty"), "unexpected: {err}");
    }

    #[test]
    fn empty_logical_name_rejected() {
        let err = parse_cli_static_file(r#"="/abs""#).expect_err("empty name must fail");
        assert!(err.contains("empty"), "unexpected: {err}");
    }

    #[test]
    fn equals_in_json_value_survives_split() {
        // JSON value may contain `=` characters (e.g., in a URL-like
        // path); split_name_value splits on the FIRST `=` only.
        let arg = r#""cfg"={"path":"/path/with=equals","mode":"r"}"#;
        let cli = parse_cli_static_file(arg).expect("parse");
        assert_eq!(cli.entry.path, PathBuf::from("/path/with=equals"));
    }

    // ------------------- Must-fix additions -------------------

    /// M-22-1: JSON-aware split.  A logical name that itself contains
    /// an `=` via a JSON escape must parse correctly (previously the
    /// first `=` byte cleaved inside the LHS's quotes).
    #[test]
    fn logical_name_may_contain_escaped_equals() {
        let arg = r#""a=b"=/abs"#;
        let cli = parse_cli_static_file(arg).expect("parse");
        assert_eq!(cli.logical_name, "a=b");
        assert_eq!(cli.entry.path, PathBuf::from("/abs"));
    }

    /// M-22-1: unquoted bare-path RHS containing `=` is now rejected
    /// (it almost always means an operator forgot to quote or brace).
    #[test]
    fn unquoted_bare_path_with_equals_is_rejected() {
        let err = parse_cli_static_file("foo=/etc/passwd=extra")
            .expect_err("bare path with `=` must be rejected");
        assert!(
            err.contains("contains `=`") || err.contains("wrap the value"),
            "unexpected: {err}"
        );
    }

    /// M-22-2: object form with an unknown field (typo of `mode`)
    /// must fail — silently defaulting to `"rw"` (dir surface)
    /// would upgrade read-only intent to read-write.
    #[test]
    fn object_with_unknown_field_is_rejected() {
        let err = parse_cli_static_dir(r#""logs/"={"path":"/var/log","mmode":"r"}"#)
            .expect_err("misspelled `mmode` must fail");
        assert!(
            err.contains("mmode") || err.contains("unknown"),
            "unexpected: {err}"
        );
    }

    /// M-22-3: NUL byte in JSON-escaped logical name is rejected.
    #[test]
    fn nul_in_logical_name_rejected() {
        let err = parse_cli_static_file(r#""admin\u0000forged"=/abs"#)
            .expect_err("NUL in logical name must be rejected");
        assert!(err.contains("control"), "unexpected: {err}");
    }

    /// M-22-3: newline in JSON-escaped logical name is rejected
    /// (log-injection defence).
    #[test]
    fn newline_in_logical_name_rejected() {
        let err = parse_cli_static_file(r#""admin\n[ERR] forged"=/etc/x"#)
            .expect_err("newline in logical name must be rejected");
        assert!(err.contains("control"), "unexpected: {err}");
    }

    /// M-22-3: NUL byte in path (via JSON escape) is rejected.
    #[test]
    fn nul_in_path_rejected() {
        let err = parse_cli_static_file(r#""cfg"={"path":"/etc/\u0000null","mode":"r"}"#)
            .expect_err("NUL in path must be rejected");
        assert!(err.contains("control"), "unexpected: {err}");
    }

    /// M-22-4: empty JSON-quoted logical name is now rejected
    /// (previously silently accepted as `""`).
    #[test]
    fn empty_json_quoted_logical_name_rejected() {
        let err = parse_cli_static_file(r#"""=/abs"#)
            .expect_err("empty quoted logical name must be rejected");
        assert!(err.contains("empty"), "unexpected: {err}");
    }

    /// M-22-6: object form missing required `path` field.
    #[test]
    fn object_missing_path_is_rejected() {
        let err =
            parse_cli_static_file(r#""cfg"={"mode":"r"}"#).expect_err("missing path must fail");
        assert!(
            err.contains("path") || err.contains("missing"),
            "unexpected: {err}"
        );
    }

    /// M-22-6: object form missing required `mode` field.
    #[test]
    fn object_missing_mode_is_rejected() {
        let err =
            parse_cli_static_file(r#""cfg"={"path":"/x"}"#).expect_err("missing mode must fail");
        assert!(
            err.contains("mode") || err.contains("missing"),
            "unexpected: {err}"
        );
    }

    // ------------------- Should-fix additions -------------------

    /// S-22-1: logical name larger than `MAX_LOGICAL_KEY_LEN` is
    /// rejected on the CLI surface (parity with config surface's
    /// `validate_size_limits`).
    #[test]
    fn oversize_logical_name_rejected() {
        let big = "a".repeat(MAX_LOGICAL_KEY_LEN + 1);
        let arg = format!("{big}=/abs");
        let err = parse_cli_static_file(&arg).expect_err("oversize logical name must be rejected");
        assert!(
            err.contains("MAX_LOGICAL_KEY_LEN") || err.contains("exceeds"),
            "unexpected: {err}"
        );
    }

    /// S-22-1: boundary — exactly `MAX_LOGICAL_KEY_LEN` bytes is OK.
    #[test]
    fn logical_name_at_limit_ok() {
        let big = "a".repeat(MAX_LOGICAL_KEY_LEN);
        let arg = format!("{big}=/abs");
        parse_cli_static_file(&arg).expect("name at limit should parse");
    }

    /// S-22-3: path containing a `..` component is rejected
    /// (defeats lexical dedup in slice 24).
    #[test]
    fn path_with_dotdot_rejected() {
        let err = parse_cli_static_file(r#""cfg"={"path":"/etc/../etc/passwd","mode":"r"}"#)
            .expect_err("`..` in path must fail");
        assert!(err.contains(".."), "unexpected: {err}");
    }

    /// S-22-5: JSON null value is rejected.
    #[test]
    fn json_null_value_rejected() {
        let err = parse_cli_static_file(r#""cfg"=null"#).expect_err("null value must be rejected");
        assert!(
            err.contains("scalars") || err.contains("string"),
            "unexpected: {err}"
        );
    }

    /// S-22-5: JSON true value is rejected.
    #[test]
    fn json_true_value_rejected() {
        let err = parse_cli_static_file(r#""cfg"=true"#).expect_err("true value must be rejected");
        assert!(
            err.contains("scalars") || err.contains("string"),
            "unexpected: {err}"
        );
    }

    /// S-22-5: JSON array value is rejected.
    #[test]
    fn json_array_value_rejected() {
        let err =
            parse_cli_static_file(r#""cfg"=["/abs"]"#).expect_err("array value must be rejected");
        assert!(
            err.contains("scalars") || err.contains("array") || err.contains("string"),
            "unexpected: {err}"
        );
    }

    /// S-22-8: empty `path` in object form is rejected.
    #[test]
    fn empty_object_path_rejected() {
        let err = parse_cli_static_file(r#""cfg"={"path":"","mode":"r"}"#)
            .expect_err("empty object path must be rejected");
        assert!(
            err.contains("empty") || err.contains("not absolute"),
            "unexpected: {err}"
        );
    }

    /// S-22-8: empty `mode` in object form is rejected (empty string
    /// isn't in the whitelist).
    #[test]
    fn empty_object_mode_rejected() {
        let err = parse_cli_static_file(r#""cfg"={"path":"/x","mode":""}"#)
            .expect_err("empty object mode must be rejected");
        assert!(err.contains("invalid mode"), "unexpected: {err}");
    }

    /// S-22-8: path with spaces is accepted (object form, quoted).
    #[test]
    fn path_with_spaces_accepted() {
        let arg = r#""cfg"={"path":"/etc/my dir/x","mode":"r"}"#;
        let cli = parse_cli_static_file(arg).expect("parse");
        assert_eq!(cli.entry.path, PathBuf::from("/etc/my dir/x"));
    }

    /// S-22-8: 3 KiB path is accepted (parity with slice 21's
    /// `syntactically_odd_paths_still_parse`).
    #[test]
    fn very_long_path_accepted() {
        let long = "/tmp/".to_string() + &"a".repeat(3000);
        let arg = format!(r#""cfg"={{"path":"{long}","mode":"r"}}"#);
        let cli = parse_cli_static_file(&arg).expect("parse");
        assert_eq!(cli.entry.path, PathBuf::from(&long));
    }

    /// S-22-8: logical name equal to a mode string parses normally
    /// (no bizarre "mode inference").
    #[test]
    fn logical_name_shaped_like_mode_string_ok() {
        let arg = r#""rw"=/etc/x"#;
        let cli = parse_cli_static_file(arg).expect("parse");
        assert_eq!(cli.logical_name, "rw");
        assert_eq!(cli.entry.mode, DEFAULT_CLI_FILE_MODE); // default, not "rw"
    }

    /// N-22-2: mode comparison is strictly case-sensitive.  A future
    /// case-insensitive switch would silently accept `"WX"` → `"wx"`.
    #[test]
    fn uppercase_mode_rejected() {
        let err = parse_cli_static_file(r#""cfg"={"path":"/x","mode":"R"}"#)
            .expect_err("uppercase mode must be rejected");
        assert!(err.contains("invalid mode"), "unexpected: {err}");
    }

    // ------------------- split_name_value unit tests -------------------

    /// N-22-7: pin down the split contract on empty LHS.
    #[test]
    fn split_empty_lhs() {
        assert_eq!(split_name_value("=/abs").unwrap(), ("", "/abs"));
    }

    /// N-22-7: pin down the split contract on empty RHS.
    #[test]
    fn split_empty_rhs() {
        assert_eq!(split_name_value("cfg=").unwrap(), ("cfg", ""));
    }

    /// N-22-7: bare "=" alone.
    #[test]
    fn split_just_equals() {
        assert_eq!(split_name_value("=").unwrap(), ("", ""));
    }

    /// N-22-7: missing `=` altogether.
    #[test]
    fn split_missing_equals_errors() {
        assert!(split_name_value("no-equals-here").is_err());
    }

    /// N-22-7: unterminated quoted LHS errors.
    #[test]
    fn split_unterminated_quote_errors() {
        let err = split_name_value(r#""unterminated=/abs"#).unwrap_err();
        assert!(err.contains("unterminated"), "unexpected: {err}");
    }

    /// N-22-7: quoted LHS with escaped `=` inside doesn't split at the
    /// interior `=`.
    #[test]
    fn split_json_aware_skips_escaped_equals() {
        let (lhs, rhs) = split_name_value(r#""a=b"=/x"#).unwrap();
        assert_eq!(lhs, r#""a=b""#);
        assert_eq!(rhs, "/x");
    }
}
