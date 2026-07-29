//! ★★ **The shape of every overflow guard in the blessed contracts, pinned at the SOURCE.**
//!
//! `casper/src/main/resources/NonNegativeNumber.rho`'s `add` used to detect addition overflow by
//! OBSERVING THE WRAP — `if (v + x >= v)` is false only once `v + x` has already wrapped round to
//! something smaller than `v`. `6ff46f8a` made Int `+` and `-` checked, so that sum now raises
//! instead of wrapping, and `e3a4494b` replaced the guard with the total form
//! `if (x <= 9223372036854775807 - v)`, which never evaluates the overflowing sum at all.
//!
//! ⚠ **Why a SOURCE pin and not only a behavioural one.** The behavioural pin lives in
//! `rholang/tests/rholang_numeric_eval_spec.rs`
//! (`nonnegativenumber_add_refuses_and_restores_the_balance_and_is_exact_at_the_boundary`), and it
//! drives the reducer directly because it has to: measured 2026-07-29, the `RhoSpec`-driven
//! genesis suites do not execute. Of the 22 of them, only `failing_result_collector_spec` collects
//! any assertion at all — it is the only test resource that performs no registry lookup — and
//! `RhoSpec::run_tests` iterates only the assertions it received, so a suite that reports nothing
//! PASSES. `NonNegativeNumberTest.rho`'s `test_fail_on_overflow` and `MakeMintTest.rho`'s
//! `test_overflow_deposit` are both written and neither runs. Until that harness is repaired, a
//! source-shape pin is the only thing standing between the blessed contracts and a future editor
//! who "simplifies" the rearranged guard straight back into the wrap-detect form.
//!
//! The scan below is DERIVED, not a hand-maintained list of forbidden strings: it extracts every
//! `if (…)` condition from the comment-and-string-aware stripped source and rejects any that
//! performs addition. [`the_condition_scanner_sees_code_and_not_comments_or_strings`] is the
//! non-vacuity floor for the scanner itself.

use casper::rust::genesis::contracts::embedded_rho;

/// `Int`'s maximum, `2^63 - 1`. Rholang has no named constant for it, which is why the blessed
/// contracts spell it out; this is the same literal they use.
const RHOLANG_INT_MAX_LITERAL: &str = "9223372036854775807";

/// Replace every comment byte with a space, leaving code in place at its original offsets.
///
/// Rholang comments are `//` to end-of-line and `/* … */`. Neither introduces a comment inside a
/// double-quoted string or a backtick-quoted URI, so those are tracked; `\` escapes inside a
/// string are honoured. Comment bytes become spaces rather than being deleted so that line and
/// column offsets are preserved for diagnostics.
///
/// `blank_literals` additionally blanks the *contents* of string literals and backtick URIs,
/// keeping their delimiters. The condition scanner needs that — `"… if (s + t > s)"` is text, not
/// a guard, and a scanner that reads it would report a guard that does not exist. The floors that
/// check "the code survived stripping" need the opposite, because the things they look for
/// (`contract this(@"add", …)`) are spelled with string literals. Hence one function, two modes.
fn blank(source: &str, blank_literals: bool) -> String {
    let bytes = source.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0usize;

    #[derive(PartialEq)]
    enum Mode {
        Code,
        LineComment,
        BlockComment,
        DoubleQuoted,
        Backtick,
    }
    let mut mode = Mode::Code;

    while i < bytes.len() {
        let b = bytes[i];
        let next = bytes.get(i + 1).copied();
        match mode {
            Mode::Code => match (b, next) {
                (b'/', Some(b'/')) => {
                    mode = Mode::LineComment;
                    out.push(b' ');
                    out.push(b' ');
                    i += 2;
                }
                (b'/', Some(b'*')) => {
                    mode = Mode::BlockComment;
                    out.push(b' ');
                    out.push(b' ');
                    i += 2;
                }
                (b'"', _) => {
                    mode = Mode::DoubleQuoted;
                    out.push(b);
                    i += 1;
                }
                (b'`', _) => {
                    mode = Mode::Backtick;
                    out.push(b);
                    i += 1;
                }
                _ => {
                    out.push(b);
                    i += 1;
                }
            },
            Mode::LineComment => {
                if b == b'\n' {
                    mode = Mode::Code;
                    out.push(b'\n');
                } else {
                    out.push(b' ');
                }
                i += 1;
            }
            Mode::BlockComment => {
                if (b, next) == (b'*', Some(b'/')) {
                    mode = Mode::Code;
                    out.push(b' ');
                    out.push(b' ');
                    i += 2;
                } else {
                    out.push(if b == b'\n' { b'\n' } else { b' ' });
                    i += 1;
                }
            }
            Mode::DoubleQuoted => {
                if b == b'\\' {
                    out.push(if blank_literals { b' ' } else { b });
                    if let Some(escaped) = next {
                        out.push(if blank_literals { b' ' } else { escaped });
                        i += 2;
                        continue;
                    }
                } else if b == b'"' {
                    out.push(b);
                    mode = Mode::Code;
                } else {
                    out.push(if blank_literals && b != b'\n' { b' ' } else { b });
                }
                i += 1;
            }
            Mode::Backtick => {
                if b == b'`' {
                    out.push(b);
                    mode = Mode::Code;
                } else {
                    out.push(if blank_literals && b != b'\n' { b' ' } else { b });
                }
                i += 1;
            }
        }
    }

    String::from_utf8(out).expect("blanking comments preserves UTF-8 boundaries")
}

/// Every `if (…)` condition in `source`, with balanced parentheses, comments already blanked.
fn if_conditions(source: &str) -> Vec<String> {
    let blanked = blank(source, true);
    let bytes = blanked.as_bytes();
    let mut found = Vec::new();
    let mut search_from = 0usize;

    while let Some(offset) = blanked[search_from..].find("if") {
        let kw_start = search_from + offset;
        search_from = kw_start + 2;

        // `if` must be a whole token: not the tail of `notify`, not the head of `iffy`.
        let preceded_by_ident = kw_start
            .checked_sub(1)
            .is_some_and(|p| bytes[p].is_ascii_alphanumeric() || bytes[p] == b'_');
        if preceded_by_ident {
            continue;
        }
        let mut cursor = kw_start + 2;
        if bytes
            .get(cursor)
            .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
        {
            continue;
        }
        while bytes.get(cursor).is_some_and(|c| c.is_ascii_whitespace()) {
            cursor += 1;
        }
        if bytes.get(cursor) != Some(&b'(') {
            // A receive guard (`for (x <- ch if cond)`) or an `if` whose condition is not
            // parenthesised; neither is a parenthesised condition, so there is nothing to slice.
            continue;
        }

        let open = cursor;
        let mut depth = 0usize;
        let mut end = None;
        while cursor < bytes.len() {
            match bytes[cursor] {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(cursor);
                        break;
                    }
                }
                _ => {}
            }
            cursor += 1;
        }
        let close = end.expect("an `if (` in a blessed contract has a matching `)`");
        found.push(blanked[open + 1..close].trim().to_string());
        search_from = close;
    }

    found
}

/// ★ THE NON-VACUITY FLOOR for the scanner. Without this, a stripper that blanked *everything*
/// would make every "no `if` condition adds" assertion below trivially true.
#[test]
fn the_condition_scanner_sees_code_and_not_comments_or_strings() {
    let fixture = r#"
/* header: if (a + b >= a) lives in a block comment and must not be seen */
new c in {
  // if (v + x >= v) is the old idiom, named in a line comment
  c!("a string containing // and if (s + t > s)") |
  c!(`rho:id:if(u+w>=u)`) |
  if (p + q >= p) { Nil } else { Nil } |
  if (x <= 9223372036854775807 - v) { Nil } else { Nil } |
  for (@z <- c if z > 0) { Nil }
}
"#;
    let conditions = if_conditions(fixture);
    assert_eq!(
        conditions,
        vec![
            "p + q >= p".to_string(),
            format!("x <= {RHOLANG_INT_MAX_LITERAL} - v"),
        ],
        "★ the scanner must see exactly the two CODE conditions — not the block comment, not the \
         line comment, not the string, not the URI, and not the receive guard",
    );

    let adding: Vec<&String> = conditions.iter().filter(|c| c.contains('+')).collect();
    assert_eq!(
        adding.len(),
        1,
        "★ FLOOR: the scanner must be able to CATCH an adding condition, or its use below is \
         vacuous; got {conditions:?}",
    );
}

/// ★★ `NonNegativeNumber.rho`'s `add` guards BEFORE adding, and no `if` in the file adds.
#[test]
fn nonnegativenumber_guards_before_adding_never_by_observing_a_wrap() {
    let source = embedded_rho::NON_NEGATIVE_NUMBER;
    let blanked = blank(source, false);

    // FLOOR: the stripper kept the code and dropped the header comment.
    assert!(
        blanked.contains(r#"contract this(@"add", @x, success)"#),
        "★ FLOOR: `add`'s definition must survive comment blanking",
    );
    assert!(
        blanked.contains(r#"contract this(@"sub", @x, success)"#),
        "★ FLOOR: `sub`'s definition must survive comment blanking",
    );
    assert!(
        !blanked.contains("lastNonce"),
        "★ FLOOR: `lastNonce` appears only in the header BLOCK COMMENT, so seeing it means the \
         stripper is not stripping",
    );

    let conditions = if_conditions(source);

    // FLOOR: this file really does have guards to inspect.
    assert!(
        conditions.len() >= 4,
        "★ FLOOR: NonNegativeNumber.rho has at least four parenthesised `if` conditions \
         (add's outer, add's guard, sub's outer, sub's guard, the initializer's); got \
         {conditions:?}",
    );

    // ★ THE SUBJECT. Not one guard in this contract may compute a sum in order to decide whether
    // that sum is representable.
    let adding: Vec<&String> = conditions.iter().filter(|c| c.contains('+')).collect();
    assert!(
        adding.is_empty(),
        "★★ an `if` condition in NonNegativeNumber.rho performs ADDITION: {adding:?}\n\
         Int `+` is checked, so an overflowing sum RAISES rather than wrapping — a guard that has \
         to evaluate `v + x` in order to learn whether `v + x` is representable cannot work. \
         Guard BEFORE the arithmetic, as `add` and `sub` both now do.",
    );

    // ★ `add` uses the ruled total form...
    assert!(
        conditions
            .iter()
            .any(|c| c.replace(' ', "") == format!("x<={RHOLANG_INT_MAX_LITERAL}-v")),
        "★ `add` must guard with `x <= {RHOLANG_INT_MAX_LITERAL} - v` (RULED 2026-07-29); \
         got {conditions:?}",
    );
    // ...and `sub`, its sibling, was already written the same way: decide, then compute. With
    // `0 <= x <= v` the difference `v - x` lies in `[0, v]`, so it can neither underflow nor
    // overflow, and `sub`'s false branch restores `v` exactly as `add`'s does.
    assert!(
        conditions.iter().any(|c| c.replace(' ', "") == "x<=v"),
        "★ `sub` must keep guarding with `x <= v`; got {conditions:?}",
    );
}

/// ★★ `MakeMint.rho`'s `deposit` is DOWNSTREAM of `NonNegativeNumber`'s `add` — it must not grow
/// a wrap detect of its own.
///
/// `MakeMint.rho:133-136` calls `balance!("add", amount, *addSuccessCh)` and branches on the
/// boolean that comes back, so the one fix at `NonNegativeNumber.rho:22` covers it. This cell
/// pins that it stays that way.
#[test]
fn makemint_deposit_delegates_its_overflow_check_and_adds_in_no_condition() {
    let source = embedded_rho::MAKE_MINT;
    let blanked = blank(source, false);

    // FLOOR: `deposit` and its delegation to `add` are present.
    assert!(
        blanked.contains(r#"contract thisPurse(@"deposit", @amount, @src, success)"#),
        "★ FLOOR: `deposit`'s definition must survive comment blanking",
    );
    assert!(
        blanked.contains(r#"balance!("add", amount, *addSuccessCh)"#),
        "★ `deposit` must DELEGATE its overflow check to NonNegativeNumber's `add` rather than \
         doing its own",
    );

    let conditions = if_conditions(source);
    assert!(
        conditions.len() >= 4,
        "★ FLOOR: MakeMint.rho has several parenthesised `if` conditions; got {conditions:?}",
    );

    let adding: Vec<&String> = conditions.iter().filter(|c| c.contains('+')).collect();
    assert!(
        adding.is_empty(),
        "★★ an `if` condition in MakeMint.rho performs ADDITION: {adding:?} — the overflow check \
         belongs to NonNegativeNumber's `add`, which answers `false` without evaluating the sum",
    );
}
