//! # No test in this repository may expect a panic
//!
//! ## Why this repository, which does not itself use cranelift
//!
//! `f1r3node`'s own `[profile.dev]` sets only `debug = true`, and CI runs
//! `cargo test --release` — both LLVM-backed, where `panic!` unwinds normally. The ban
//! is nevertheless repository-wide, for two independent reasons.
//!
//! **1. These crates are compiled by a workspace that DOES use cranelift.** The
//! sibling `mettail-rust` workspace takes `models`, `rholang`, `rspace_plus_plus` and
//! `crypto` as path dependencies (and `shared`, `comm`, `casper` transitively) and
//! builds them with `[profile.dev] codegen-backend = "cranelift"`. Under cg_clif a
//! panic raised in a crate compiled that way does not unwind reliably: it sails
//! through any `std::panic::catch_unwind` monomorphised in the same crate, and in the
//! proc-macro case `rustc` aborts with
//! `fatal runtime error: Rust cannot catch foreign exceptions` and prints **nothing**.
//!
//! **2. `catch_unwind` cannot see the faults this repository actually has.** Measured
//! here, twice over: a panic at an `extern "C"` frame runs the non-unwinding shim and
//! raises `SIGABRT`
//! (`rspace++/libs/rspace_rhotypes/tests/ffi_absent_required_child.rs`), and a native
//! stack overflow is a `SIGSEGV` the runtime turns into an abort
//! (`rholang/tests/stack_depth_gate.rs`, `casper/tests/deploy_depth_ceiling.rs`).
//! Neither is interceptable in-process. A test that reached for `catch_unwind` to
//! observe one of those was already broken; the only sound observer is another
//! process.
//!
//! ## The two constructs, and why they are not held to the same rule
//!
//! | construct | rule | why |
//! |---|---|---|
//! | `#[should_panic]` | **banned outright** | its only purpose is to expect a panic — there is no other use |
//! | `catch_unwind` | **allowlisted, with a reason per entry** | it has three distinct uses and only one is a test asserting a panic |
//!
//! The three uses of `catch_unwind`:
//!
//! 1. **a test asserting that something panics** — banned, and rewritten;
//! 2. **a harness** isolating a subject or capturing a message for a diagnostic — kept
//!    where it is genuinely not asserting a panic;
//! 3. **production code defending itself** — kept; it is not a test at all.
//!
//! ⚠ An allowlist without a per-entry reason is the same defect wearing a list. Every
//! entry in [`CATCH_UNWIND_ALLOWLIST`] therefore carries the argument for its own
//! existence, and the gate fails if an entry's occurrence count drifts — so a NEW
//! `catch_unwind` in an already-allowlisted file has to be argued rather than
//! inheriting the argument made for its neighbours.
//!
//! ## What replaces an expectation of a panic
//!
//! Three patterns, chosen per site:
//!
//! 1. **The condition is an internal invariant no input can violate** ⇒ assert the
//!    invariant holds, and prove by construction that the guard never fires. A
//!    *measured negative*. `RuntimeBudget::set_deploy_signatures_funded`'s empty-list
//!    refusal is one: `Cosigned`'s private `signers` field and its two refusing
//!    constructors make an empty slice unconstructible upstream, and
//!    `cost_accounting_spec.rs` asserts that upstream rather than the panic.
//! 2. **The condition IS reachable from input** ⇒ the panic is the defect. The code
//!    grows a `try_…` entry point returning `Result`/`Option`, the panicking form
//!    becomes a thin wrapper over it, and the test asserts the `Err`/`None`.
//!    `FlumeLimitedBuffer::try_drop_new` is one.
//!
//!    ★ Where the function ALREADY returns `Result`, there is no `try_…` form to
//!    grow — the error channel exists and is merely unused, and the repair is to
//!    use it. `RSpace::consume`, `RSpace::locked_install_internal` and
//!    `ReplayRSpace::consume` are that case: an arity mismatch between `channels`
//!    and `patterns` is a decidable negative expressible in the caller's own
//!    arguments (two independent repeated proto fields), and it now returns
//!    `Err(RSpaceError::BugFoundError(..))`. `storage_actions_test.rs` moved from
//!    pattern 3 to this one when the guard was converted; the argument, the sibling
//!    table and the play/replay agreement cell are in
//!    `rspace++/tests/consume_arity_refusal.rs`.
//! 3. **The abort is observable only out of process** ⇒ run the subject in a **child
//!    process** and decide on its exit status and stderr.
//!    `ffi_absent_required_child.rs` established the shape here;
//!    `ffi_consume_arity_reachability.rs`, `absent_required_child_reachability.rs`
//!    and `spatial_matcher_disposition.rs` now use it.
//!
//! ## Scope
//!
//! Every `.rs` file `git` tracks, minus `scratch*/`. `git ls-files` rather than a
//! directory walk so the gate's idea of "source" is exactly the repository's, and a
//! file that is merely present on disk (build output, an editor backup) cannot make it
//! red.
//!
//! ## Anti-vacuity
//!
//! A scanner that walked nothing, or that could not see the construct it bans, would
//! pass forever. Three separate cells prevent that: [`the_walk_reaches_the_repository`],
//! [`the_scanner_finds_a_planted_attribute`] and
//! [`the_scanner_ignores_comments_and_string_literals`].

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

// ═══════════════════════════════════════════════════════════════════════════════
// The needles
// ═══════════════════════════════════════════════════════════════════════════════

// ★ Assembled from fragments so this file's own CODE text contains neither literal.
// The gate is therefore clean under its own rule with no path exclusion — nothing
// here has to be trusted to keep excluding itself after a rename.
const PANIC_EXPECTING_ATTRIBUTE: &str = concat!("should", "_panic");
const UNWIND_INTERCEPTOR: &str = concat!("catch", "_unwind");

// ═══════════════════════════════════════════════════════════════════════════════
// The allowlist
// ═══════════════════════════════════════════════════════════════════════════════

/// One allowlisted file: `(path, occurrences, why it is allowed to be there)`.
type Allowed = (&'static str, usize, &'static str);

/// Every file permitted to name the unwind interceptor, with the exact number of
/// occurrences and the argument for each.
///
/// ⚠ The count is part of the entry. A new occurrence in an already-listed file
/// fails this gate, because the reason recorded here was made about the occurrences
/// that existed when it was written — not about whatever is added later.
const CATCH_UNWIND_ALLOWLIST: &[Allowed] = &[
    // ── use 3: production code defending itself ──────────────────────────────
    (
        "rholang/src/rust/interpreter/reduce.rs",
        1,
        "PRODUCTION, NOT A TEST. `spawn_detached` counts detached reduction tasks with \
         an INCREMENT-BEFORE-SPAWN counter and an RAII guard. The interceptor is \
         MANDATORY there: a panicking deploy must still push an `InterpreterError` \
         into the drive sink, or `is_failed` flips a failed deploy to success. It \
         asserts nothing about panicking — it converts a panic into the error the \
         `Err` arm two lines above produces.",
    ),
    // ── use 2: a harness that does not assert a panic ────────────────────────
    (
        "rholang/src/rust/interpreter/pretty_printer.rs",
        1,
        "TEST HARNESS, and it observes TEST outcomes rather than asserting that \
         production panics. `quietly` runs a body with the panic hook silenced and \
         returns `Result<T, String>`; its callers are DIFFERENTIAL and MUTATION \
         meta-tests — `driven` against `oracle_…` must take the SAME disposition, and \
         the representation table records which registry test a given mutation turns \
         red. In that role it is doing what libtest does, in-process, because the \
         enclosing test is a test ABOUT other tests. No caller passes only when a \
         panic occurs; the differential passes when the two forms agree, panic or not. \
         ⚠ Two of its callers DO pin a specific production panic (`Match::target` / \
         `New::p` absent, and the deliberate load-bearing panic in `Printer::cap` that \
         `the_capping_call_sites_are_reproduced` counts). Those are recorded as \
         follow-ups rather than rewritten here: converting them means either changing \
         the printer's `.expect` discipline for absent required fields — the #127 \
         shape axis, an open finding — or spawning one process per probe per trim \
         through a differential that compares renders, and neither is a test author's \
         unilateral call.",
    ),
    // ── the token, but not the construct ─────────────────────────────────────
    (
        "rspace++/libs/rspace_rhotypes/tests/ffi_absent_required_child.rs",
        1,
        "NOT A CALL. The token occurs once in the assertion message that states the \
         finding — the fault at the FFI boundary \
         is a non-unwinding panic that no in-process interceptor can catch, which is \
         precisely why this file measures it out of process. This file is the template \
         the other subprocess probes in both repositories follow.",
    ),
];

// ═══════════════════════════════════════════════════════════════════════════════
// The scanner
// ═══════════════════════════════════════════════════════════════════════════════

/// One occurrence of a banned or allowlisted construct.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Occurrence {
    path: String,
    line: usize,
    text: String,
}

/// Strip line comments, block comments, char literals and — optionally — string
/// literals from one file's text, preserving line structure so line numbers stay
/// exact.
///
/// Comments are always removed: this gate must be describable in prose, and every
/// rewritten site carries a comment explaining what it used to be.
///
/// `keep_strings` is the difference between the two needles:
///
/// * the panic-expecting **attribute** is looked for with strings REMOVED, so a
///   fixture that merely names one is not mistaken for one;
/// * the unwind interceptor is looked for with strings KEPT, because a macro that
///   EMITS one has committed to it exactly as much as a crate that writes one.
///
/// ⚠ Known and deliberate limitation: string state is reset at each newline, so the
/// continuation lines of a multi-line string literal are scanned as code. That errs
/// toward REPORTING, which is the correct failure direction for a gate — a false
/// positive is loud and is answered by an allowlist entry (two of the four entries
/// above are exactly that), whereas carrying string state across lines would let a
/// stray quote silently blind the scanner for the rest of a file.
fn strip(source: &str, keep_strings: bool) -> Vec<String> {
    let mut out = Vec::with_capacity(source.lines().count());
    let mut in_block_comment = false;
    for raw_line in source.lines() {
        let chars: Vec<char> = raw_line.chars().collect();
        let mut kept = String::with_capacity(raw_line.len());
        let mut i = 0;
        let mut in_string = false;
        let mut raw_hashes: Option<usize> = None;
        while i < chars.len() {
            if in_block_comment {
                if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    in_block_comment = false;
                    i += 2;
                } else {
                    i += 1;
                }
                continue;
            }
            if let Some(hashes) = raw_hashes {
                if chars[i] == '"'
                    && chars[i + 1..].iter().take(hashes).all(|c| *c == '#')
                    && chars.len() >= i + 1 + hashes
                {
                    raw_hashes = None;
                    i += 1 + hashes;
                    continue;
                }
                if keep_strings {
                    kept.push(chars[i]);
                }
                i += 1;
                continue;
            }
            if in_string {
                if chars[i] == '\\' {
                    if keep_strings {
                        kept.push(chars[i]);
                        if let Some(c) = chars.get(i + 1) {
                            kept.push(*c);
                        }
                    }
                    i += 2;
                    continue;
                }
                if chars[i] == '"' {
                    in_string = false;
                    i += 1;
                    continue;
                }
                if keep_strings {
                    kept.push(chars[i]);
                }
                i += 1;
                continue;
            }
            // Not in any comment or literal.
            if chars[i] == '/' && chars.get(i + 1) == Some(&'/') {
                break; // line comment (including `///` and `//!`)
            }
            if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                in_block_comment = true;
                i += 2;
                continue;
            }
            // A CHAR LITERAL, so `'"'` cannot be mistaken for the start of a string.
            // A lifetime (`'static`, `'_`) is not a literal and falls through.
            if chars[i] == '\'' {
                let width = match chars.get(i + 1) {
                    Some('\\') => 4, // '\n' — quote, backslash, escapee, quote
                    Some(_) => 3,    // 'x'
                    None => 0,
                };
                if width > 0 && chars.get(i + width - 1) == Some(&'\'') {
                    i += width;
                    continue;
                }
            }
            if chars[i] == 'r' {
                // `r"…"` or `r#"…"#`
                let mut hashes = 0;
                let mut j = i + 1;
                while chars.get(j) == Some(&'#') {
                    hashes += 1;
                    j += 1;
                }
                if chars.get(j) == Some(&'"') {
                    raw_hashes = Some(hashes);
                    i = j + 1;
                    continue;
                }
            }
            if chars[i] == '"' {
                in_string = true;
                i += 1;
                continue;
            }
            kept.push(chars[i]);
            i += 1;
        }
        out.push(kept);
    }
    out
}

/// Every occurrence of `needle` in `source`'s code text, attributed to `path`.
fn scan_source(path: &str, source: &str, needle: &str, keep_strings: bool) -> Vec<Occurrence> {
    strip(source, keep_strings)
        .into_iter()
        .enumerate()
        .filter(|(_, code)| code.contains(needle))
        .map(|(index, code)| Occurrence {
            path: path.to_string(),
            line: index + 1,
            text: code.trim().to_string(),
        })
        .collect()
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the shared crate has a workspace parent")
        .to_path_buf()
}

/// Every `.rs` path `git` tracks, minus `scratch*/`.
fn tracked_rust_files() -> Vec<String> {
    let output = Command::new("git")
        .args(["-c", "core.fsmonitor=false", "ls-files", "-z", "--", "*.rs"])
        .current_dir(repo_root())
        .output()
        .expect("`git ls-files` runs in the repository root");
    assert!(
        output.status.success(),
        "`git ls-files` failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|path| !path.is_empty() && !path.starts_with("scratch"))
        .map(str::to_string)
        .collect()
}

/// Scan the whole repository for one needle.
fn scan_repository(needle: &str, keep_strings: bool) -> (Vec<Occurrence>, usize) {
    let root = repo_root();
    let mut found = Vec::new();
    let mut walked = 0usize;
    for relative in tracked_rust_files() {
        let Ok(source) = fs::read_to_string(root.join(&relative)) else {
            continue;
        };
        walked += 1;
        found.extend(scan_source(&relative, &source, needle, keep_strings));
    }
    (found, walked)
}

// ═══════════════════════════════════════════════════════════════════════════════
// The gate
// ═══════════════════════════════════════════════════════════════════════════════

/// ★★★ No tracked source may carry a panic-expecting test attribute.
#[test]
fn no_tracked_source_expects_a_panic() {
    let (found, walked) = scan_repository(PANIC_EXPECTING_ATTRIBUTE, false);
    assert!(
        found.is_empty(),
        "★★★ {} panic-expecting attribute(s) in tracked source, across {walked} files.\n\n\
         See this file's module documentation for why the ban is repository-wide even \
         though CI runs an LLVM-backed profile, and for the three patterns that \
         replace one.\n\n{}",
        found.len(),
        found
            .iter()
            .map(|o| format!("  {}:{}: {}", o.path, o.line, o.text))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// ★★★ Every unwind interceptor is on the allowlist, at the recorded count.
#[test]
fn every_unwind_interceptor_is_allowlisted_with_a_reason() {
    let (found, walked) = scan_repository(UNWIND_INTERCEPTOR, true);

    let mut observed: BTreeMap<&str, Vec<&Occurrence>> = BTreeMap::new();
    for occurrence in &found {
        observed
            .entry(occurrence.path.as_str())
            .or_default()
            .push(occurrence);
    }

    let allowed: BTreeMap<&str, (usize, &str)> = CATCH_UNWIND_ALLOWLIST
        .iter()
        .map(|(path, count, reason)| (*path, (*count, *reason)))
        .collect();

    for (path, count, reason) in CATCH_UNWIND_ALLOWLIST {
        assert!(
            reason.len() >= 80,
            "the allowlist entry for `{path}` has no real reason recorded (\"{reason}\"). \
             An allowlist without a per-entry reason is the same defect wearing a list."
        );
        assert!(*count > 0, "`{path}` is allowlisted for zero occurrences");
    }

    let mut problems: Vec<String> = Vec::new();

    for (path, occurrences) in &observed {
        match allowed.get(path) {
            None => problems.push(format!(
                "  ★ NOT ALLOWLISTED — {path} ({} occurrence(s)):\n{}",
                occurrences.len(),
                occurrences
                    .iter()
                    .map(|o| format!("      line {}: {}", o.line, o.text))
                    .collect::<Vec<_>>()
                    .join("\n")
            )),
            Some((expected, _)) if *expected != occurrences.len() => problems.push(format!(
                "  ★ COUNT DRIFTED — {path}: allowlisted for {expected}, found {}. The \
                 recorded reason was written about the occurrences that existed then; a \
                 new one must be argued, not inherited.\n{}",
                occurrences.len(),
                occurrences
                    .iter()
                    .map(|o| format!("      line {}: {}", o.line, o.text))
                    .collect::<Vec<_>>()
                    .join("\n")
            )),
            Some(_) => {}
        }
    }

    for (path, _) in &allowed {
        if !observed.contains_key(path) {
            problems.push(format!(
                "  ★ STALE ENTRY — {path} is allowlisted but no longer contains the \
                 construct. Delete the entry so the list keeps describing the tree."
            ));
        }
    }

    assert!(
        problems.is_empty(),
        "★★★ the unwind-interceptor allowlist does not describe the tree (walked \
         {walked} files):\n\n{}\n\n\
         Each entry must state which of the three uses it is — a test asserting a \
         panic (which must be REWRITTEN, not listed), a harness that does not assert \
         one, or production code defending itself.",
        problems.join("\n\n")
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Anti-vacuity — the gate must be able to go RED
// ═══════════════════════════════════════════════════════════════════════════════

/// The walk really covers the repository.
#[test]
fn the_walk_reaches_the_repository() {
    let (_, walked) = scan_repository(PANIC_EXPECTING_ATTRIBUTE, false);
    assert!(
        walked > 500,
        "expected to walk more than 500 tracked source files, walked {walked} — the \
         gate is scanning an empty or truncated tree and proves nothing"
    );
    let files = tracked_rust_files();
    assert!(
        files
            .iter()
            .any(|p| p == "shared/tests/panic_expectation_gate.rs"),
        "the walk did not reach this file, so its idea of `tracked source` is not this \
         repository's"
    );
}

/// ★ THE RED PROOF. Planted in a synthetic buffer, the attribute is reported — with
/// the file name and the line.
///
/// This is the executable form of "show it red". It cannot be done by planting a real
/// attribute in the tree, because the whole point of the gate is that one must not be
/// there; a synthetic buffer proves the same thing about the same scanner and stays
/// proven on every future run rather than once.
#[test]
fn the_scanner_finds_a_planted_attribute() {
    let planted = format!(
        "#[test]\n#[{}(expected = \"boom\")]\nfn plants_a_violation() {{ panic!(\"boom\") }}\n",
        PANIC_EXPECTING_ATTRIBUTE
    );
    let found = scan_source(
        "synthetic/planted_violation.rs",
        &planted,
        PANIC_EXPECTING_ATTRIBUTE,
        false,
    );
    assert_eq!(
        found.len(),
        1,
        "the planted attribute was not reported: {found:?}"
    );
    assert_eq!(found[0].path, "synthetic/planted_violation.rs");
    assert_eq!(
        found[0].line, 2,
        "the reported line must be the attribute's"
    );

    // The `#[tokio::test]` spelling this repository uses is reported too.
    let asynchronous = format!(
        "#[tokio::test]\n#[{}(expected = \"RUST ERROR\")]\nasync fn a() {{}}\n",
        PANIC_EXPECTING_ATTRIBUTE
    );
    assert_eq!(
        scan_source(
            "synthetic/async.rs",
            &asynchronous,
            PANIC_EXPECTING_ATTRIBUTE,
            false
        )
        .len(),
        1,
        "an async panic-expecting test must be reported too"
    );

    // …and the interceptor needle finds a planted interceptor.
    let interceptor = format!(
        "fn f() {{ let _ = std::panic::{}(|| ()); }}\n",
        UNWIND_INTERCEPTOR
    );
    let found = scan_source(
        "synthetic/interceptor.rs",
        &interceptor,
        UNWIND_INTERCEPTOR,
        true,
    );
    assert_eq!(
        found.len(),
        1,
        "the planted interceptor was not reported: {found:?}"
    );
}

/// …and the scanner does NOT report a construct that is merely mentioned.
///
/// Without this the gate would be unusable: this very file explains the ban in prose,
/// and every rewritten site records what it used to be.
#[test]
fn the_scanner_ignores_comments_and_string_literals() {
    let mentions = format!(
        "//! This module used to use `#[{attr}]`.\n\
         /// Formerly `#[{attr}(expected = \"x\")]`; see the gate.\n\
         // #[{attr}]\n\
         /* #[{attr}] */\n\
         fn f() {{ let _info = \"ignore,{attr}\"; }}\n",
        attr = PANIC_EXPECTING_ATTRIBUTE
    );
    let found = scan_source(
        "synthetic/mentions.rs",
        &mentions,
        PANIC_EXPECTING_ATTRIBUTE,
        false,
    );
    assert!(
        found.is_empty(),
        "the scanner reported a MENTION as a violation, which would make the ban \
         undocumentable: {found:?}"
    );

    let block = format!(
        "/*\n  #[{attr}]\n  still a comment\n*/\nfn g() {{}}\n",
        attr = PANIC_EXPECTING_ATTRIBUTE
    );
    assert!(
        scan_source(
            "synthetic/block.rs",
            &block,
            PANIC_EXPECTING_ATTRIBUTE,
            false
        )
        .is_empty(),
        "a multi-line block comment leaked"
    );

    let raw = format!(
        "fn h() {{ let _ = r#\"#[{attr}]\"#; }}\n",
        attr = PANIC_EXPECTING_ATTRIBUTE
    );
    assert!(
        scan_source("synthetic/raw.rs", &raw, PANIC_EXPECTING_ATTRIBUTE, false).is_empty(),
        "a raw string leaked into attribute detection"
    );

    // ★ A CHAR LITERAL holding a quote must not put the scanner into string state for
    // the rest of the line — otherwise a violation after one would be invisible.
    let quoted_char = format!(
        "fn q(c: char) {{ if c == '\"' {{}} }}\n#[{attr}]\nfn after() {{}}\n",
        attr = PANIC_EXPECTING_ATTRIBUTE
    );
    assert_eq!(
        scan_source(
            "synthetic/quoted_char.rs",
            &quoted_char,
            PANIC_EXPECTING_ATTRIBUTE,
            false
        )
        .len(),
        1,
        "a char literal containing a double quote blinded the scanner"
    );

    // …and an interceptor inside a KEPT string is still seen.
    let emitted = format!(
        "fn e() {{ out.push_str(r#\"std::panic::{}\"#); }}\n",
        UNWIND_INTERCEPTOR
    );
    assert_eq!(
        scan_source("synthetic/emitted.rs", &emitted, UNWIND_INTERCEPTOR, true).len(),
        1,
        "an EMITTED interceptor must still be seen"
    );
}
