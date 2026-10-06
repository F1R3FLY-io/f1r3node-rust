//! Rholang source-composition utilities.
//!
//! Shared between `casper::genesis::contracts::fs_genesis` (production
//! genesis-deploy composition) and File I/O test harnesses that stitch
//! library sources into a shared `new` scope for integration tests.
//!
//! Historically these lived as two hand-synced copies (M-P6-4 caught a
//! divergence between them, M-14 tracked the maintenance-cost concern).
//! Consolidated here 2026-08-11 (M-14 resolution, Option B): rholang is
//! the deepest crate that both call sites depend on; casper depends on
//! rholang; test code lives in rholang; no circular dependency.

/// Extract the body between the top-level `new ... in {` and the
/// closing `}` of a `.rho` library file.
///
/// Lexically-aware scanner: skips (a) `//` line comments (to end of
/// line), (b) `/* ... */` block comments, (c) `"..."` string
/// literals with `\` escape handling, and (d) `` `...` `` URI
/// literals.  Brace-matches to find the CLOSING `}` that pairs with
/// the top-level `new ... in {` opener — more robust than a
/// byte-level `rfind('}')` approach, which silently truncates on
/// stray `}` inside string literals or trailing comments.
///
/// Panics if the source doesn't contain a top-level `in {` or if
/// braces are unbalanced.  Both are programmer errors in the
/// hand-authored `.rho` library sources this operates on.
pub fn lib_body(src: &str) -> &str {
    let bytes = src.as_bytes();
    let n = bytes.len();
    let mut i = 0usize;

    // Scanner state.  When advancing, we always land at the next
    // "top-level" byte (i.e., not inside a comment / string / URI).
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut in_string = false;
    let mut in_uri = false;

    // Locate the top-level `new ... in {` opener.
    let mut open_pos: Option<usize> = None;
    while i < n {
        let c = bytes[i];
        if in_line_comment {
            if c == b'\n' {
                in_line_comment = false;
            }
            i += 1;
            continue;
        }
        if in_block_comment {
            if c == b'*' && i + 1 < n && bytes[i + 1] == b'/' {
                in_block_comment = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if in_string {
            if c == b'\\' && i + 1 < n {
                i += 2;
                continue;
            }
            if c == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if in_uri {
            if c == b'`' {
                in_uri = false;
            }
            i += 1;
            continue;
        }
        // Top-level dispatch.
        if c == b'/' && i + 1 < n && bytes[i + 1] == b'/' {
            in_line_comment = true;
            i += 2;
            continue;
        }
        if c == b'/' && i + 1 < n && bytes[i + 1] == b'*' {
            in_block_comment = true;
            i += 2;
            continue;
        }
        if c == b'"' {
            in_string = true;
            i += 1;
            continue;
        }
        if c == b'`' {
            in_uri = true;
            i += 1;
            continue;
        }
        // Look for `in {` at top level.
        if c == b'i'
            && i + 3 < n
            && bytes[i + 1] == b'n'
            && bytes[i + 2] == b' '
            && bytes[i + 3] == b'{'
        {
            // Sanity: ensure `in` is a keyword not the tail of an ident.
            let prev_ok = i == 0 || !is_ident_char(bytes[i - 1]);
            if prev_ok {
                open_pos = Some(i + 4); // one past the `{`
                break;
            }
        }
        i += 1;
    }
    let body_start = open_pos.expect("library source must contain a top-level `in {`");

    // Walk from body_start, tracking brace depth.  The FIRST `}` that
    // brings depth back to 0 is the closing `}` of the `new ... in {`.
    let mut depth: i64 = 1;
    let mut j = body_start;
    // Reset lexer state.
    in_line_comment = false;
    in_block_comment = false;
    in_string = false;
    in_uri = false;
    while j < n {
        let c = bytes[j];
        if in_line_comment {
            if c == b'\n' {
                in_line_comment = false;
            }
            j += 1;
            continue;
        }
        if in_block_comment {
            if c == b'*' && j + 1 < n && bytes[j + 1] == b'/' {
                in_block_comment = false;
                j += 2;
                continue;
            }
            j += 1;
            continue;
        }
        if in_string {
            if c == b'\\' && j + 1 < n {
                j += 2;
                continue;
            }
            if c == b'"' {
                in_string = false;
            }
            j += 1;
            continue;
        }
        if in_uri {
            if c == b'`' {
                in_uri = false;
            }
            j += 1;
            continue;
        }
        if c == b'/' && j + 1 < n && bytes[j + 1] == b'/' {
            in_line_comment = true;
            j += 2;
            continue;
        }
        if c == b'/' && j + 1 < n && bytes[j + 1] == b'*' {
            in_block_comment = true;
            j += 2;
            continue;
        }
        if c == b'"' {
            in_string = true;
            j += 1;
            continue;
        }
        if c == b'`' {
            in_uri = true;
            j += 1;
            continue;
        }
        if c == b'{' {
            depth += 1;
        } else if c == b'}' {
            depth -= 1;
            if depth == 0 {
                return &src[body_start..j];
            }
        }
        j += 1;
    }
    panic!("library source must have a balanced closing `}}` for the top-level `new ... in {{`");
}

fn is_ident_char(b: u8) -> bool { matches!(b, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_') }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lib_body_extracts_simple_body() {
        let src = "new x in { x!(1) }";
        assert_eq!(lib_body(src), " x!(1) ");
    }

    #[test]
    fn lib_body_skips_line_comment_in_marker() {
        // The FIRST `in {` is inside a `//` comment; the SECOND is real.
        let src = "// docs: `new x in { ... }`\nnew x in { x!(2) }";
        assert_eq!(lib_body(src), " x!(2) ");
    }

    #[test]
    fn lib_body_skips_block_comment_in_marker() {
        let src = "/* new x in { hidden } */\nnew x in { x!(3) }";
        assert_eq!(lib_body(src), " x!(3) ");
    }

    #[test]
    fn lib_body_handles_brace_inside_string_literal() {
        // If we used rfind('}'), the `}` inside "hi}" would truncate the body.
        let src = r#"new x in { x!("hi}") }"#;
        assert_eq!(lib_body(src), r#" x!("hi}") "#);
    }

    #[test]
    fn lib_body_handles_escaped_quote_in_string() {
        let src = r#"new x in { x!("say \"}\"") }"#;
        assert_eq!(lib_body(src), r#" x!("say \"}\"") "#);
    }

    #[test]
    fn lib_body_handles_backtick_uri_with_brace() {
        // Backtick-quoted URIs shouldn't shift the brace-matcher.
        let src = "new x(`rho:io:fs:1.0.0/{weird}`) in { x!(4) }";
        assert_eq!(lib_body(src), " x!(4) ");
    }

    #[test]
    fn lib_body_handles_nested_braces() {
        let src = "new x in { match 1 { 1 => x!(5) } }";
        assert_eq!(lib_body(src), " match 1 { 1 => x!(5) } ");
    }

    #[test]
    fn lib_body_handles_trailing_content_past_close_brace() {
        // A hypothetical future maintainer adds a `// trailing } comment`
        // after the closing brace.  rfind('}') would break on this;
        // the depth-tracker doesn't.
        let src = "new x in { x!(6) }\n// trailing }";
        assert_eq!(lib_body(src), " x!(6) ");
    }

    #[test]
    fn lib_body_rejects_in_inside_identifier() {
        // A hypothetical library where the byte sequence `in {` appears
        // as the tail of an identifier (e.g., `sortin {`) must NOT be
        // matched.  Prev-char check enforces `in` is a keyword.
        let src = "new x in { new sortin { 1 } }";
        // With the identifier check, we still find the OUTER `in {`.
        assert_eq!(lib_body(src), " new sortin { 1 } ");
    }
}
