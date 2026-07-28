//! ★ The source policy that makes the intern-store sibling enumeration
//! EXECUTABLE: **a test that reaches the process-wide intern store must hold
//! one of its file's store guards.**
//!
//! # Why this exists
//!
//! `cargo test` runs a binary's tests as threads in ONE process; `cargo nextest`
//! forks a process per test. The EPathMap intern store is a process-global
//! (`TRIE_INTERN`, `models/src/rust/pathmap_crate_type_mapper.rs`), so under
//! nextest each test sees a private empty store and under `cargo test` it sees
//! whatever its neighbours have already put there. A test that reads store
//! state without excluding its neighbours is therefore **green under one
//! harness and red under the other** — and f1r3node's CI runs `cargo test`
//! (`.github/workflows/ci.yml`, the per-crate matrix), which is the harness
//! that fails.
//!
//! `epathmap_wrapper_cell.rs` shipped exactly that: it asserted
//! `intern_store_len_for_test() == 0` — "nothing in this process has ever
//! interned" — while six of its own tests interned without taking the file's
//! `STORE_LOCK`. The observed left-hand value moved run to run (5, 27, 35, 64),
//! because it was reporting the schedule rather than the call under test.
//!
//! # Why a POLICY rather than one more fixed assertion
//!
//! The file already had a guard. The guard was not the missing piece — the
//! missing piece was that *the guard had not been applied to every sibling*,
//! and nothing anywhere reported the omission. Fixing the six call sites
//! without this test would leave the seventh free to be added silently, which
//! is how this defect class has recurred: adding a guard is not the same as
//! enumerating the siblings, and the enumeration is only durable if a machine
//! keeps re-deriving it.
//!
//! # The rule
//!
//! For every `models/tests/*.rs` that DEFINES a store guard (i.e. declares
//! `fn store_guard(`), every `#[test]` function in that file whose body can
//! reach the store ([`STORE_REACHING_TOKENS`]) must also take a guard
//! ([`GUARD_TOKENS`]).
//!
//! Files that define no guard are deliberately out of scope: their assertions
//! must be *relational* (invariant under any store state) rather than
//! observations of store state. `epathmap_spliced_event_bytes.rs` is the
//! worked example — it clears the store without any guard, and every assertion
//! it makes ("spliced == direct", "these two hash alike") holds whatever the
//! store contains, so it has nothing to serialize against. Extending the rule
//! to guardless files would demand a guard those files genuinely do not need.
//!
//! # Why it cannot pass vacuously
//!
//! Four independent floors, all exercised below:
//!
//! 1. the scan must have visited a plausible number of test files, so a broken
//!    root or directory filter fails instead of finding nothing;
//! 2. it must have recognised a plausible number of `#[test]` functions, so a
//!    broken function/brace scanner fails instead of finding nothing;
//! 3. it must have found a plausible number of *store-reaching* tests, so a
//!    broken token list fails instead of exempting everything;
//! 4. the detector is run against a fixture carrying known-bad and known-good
//!    shapes, so its JUDGEMENT is checked rather than assumed — including the
//!    string-literal and comment cases that a naive brace scanner gets wrong.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Policy
// ---------------------------------------------------------------------------

/// Calls that can reach the process-wide intern store.
///
/// `.intern()` is the wrapper's cell-or-store rendezvous; `interned_epathmap`
/// delegates to it. The three `*_for_test` seams read or write the store
/// directly. Any of them inside a test body makes that test a neighbour whose
/// scheduling is visible to every store observation in the same binary.
const STORE_REACHING_TOKENS: &[&str] = &[
    ".intern()",
    "interned_epathmap(",
    "inject_intern_entry_for_test(",
    "clear_intern_store_for_test(",
    "intern_store_len_for_test(",
    "intern_store_touches_for_test(",
];

/// The guards a store-reaching test may hold. EXCLUSIVE (`store_guard`) is
/// required to *observe* store state; SHARED (`interning_guard`) suffices to
/// merely intern. Which one is right is a judgement the author makes; that
/// SOME guard is held is the part a machine can check.
const GUARD_TOKENS: &[&str] = &["store_guard()", "interning_guard()"];

/// Marks a file as subject to the rule.
const GUARD_DEFINITION: &str = "fn store_guard(";

// ── Anti-vacuity floors ────────────────────────────────────────────────────
//
// Each is set well below the present count, so ordinary growth never trips
// them, but a scanner that silently stops working does.

/// `models/tests/` holds ~30 integration test files.
const MIN_TEST_FILES_SCANNED: usize = 15;
/// Those files hold several hundred `#[test]` functions between them.
const MIN_TEST_FUNCTIONS_FOUND: usize = 80;
/// The two store suites alone contribute well over a dozen store-reaching
/// tests. If this floor is not met, the token list has stopped matching and
/// the policy would be exempting everything it is supposed to police.
const MIN_STORE_REACHING_TESTS: usize = 10;

// ---------------------------------------------------------------------------
// Source scanning
// ---------------------------------------------------------------------------

/// One `#[test]` function recovered from a source file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct TestFn {
    name: String,
    body: String,
}

/// Strip Rust comments and the CONTENTS of string/char literals, preserving
/// byte offsets so brace matching stays aligned with the original text.
///
/// Both are load-bearing. `debug.starts_with("EPathMap { ps: [")` and
/// `format!("{name}: drift")` put unbalanced braces inside literals, and a
/// commented-out `.intern()` must not count as reaching the store. Replacing
/// rather than deleting keeps every index valid, so the caller can slice the
/// ORIGINAL source with offsets found in the blanked copy.
fn blank_comments_and_literals(source: &str) -> String {
    let bytes = source.as_bytes();
    // Same length as the input: every byte is copied or replaced, never dropped.
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0usize;

    // Replacement that cannot match any token and cannot be a brace.
    const BLANK: u8 = b' ';

    while i < bytes.len() {
        let rest = &bytes[i..];

        // Line comment.
        if rest.starts_with(b"//") {
            while i < bytes.len() && bytes[i] != b'\n' {
                out.push(BLANK);
                i += 1;
            }
            continue;
        }

        // Block comment (Rust nests them).
        if rest.starts_with(b"/*") {
            let mut depth = 0usize;
            while i < bytes.len() {
                if bytes[i..].starts_with(b"/*") {
                    depth += 1;
                    out.push(BLANK);
                    out.push(BLANK);
                    i += 2;
                } else if bytes[i..].starts_with(b"*/") {
                    depth -= 1;
                    out.push(BLANK);
                    out.push(BLANK);
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    out.push(if bytes[i] == b'\n' { b'\n' } else { BLANK });
                    i += 1;
                }
            }
            continue;
        }

        // Raw string: r"..." / r#"..."# / r##"..."## …
        if rest.starts_with(b"r\"") || rest.starts_with(b"r#") {
            let mut j = i + 1;
            let mut hashes = 0usize;
            while j < bytes.len() && bytes[j] == b'#' {
                hashes += 1;
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'"' {
                // Blank r, the hashes and the opening quote.
                for _ in i..=j {
                    out.push(BLANK);
                }
                i = j + 1;
                // Scan to the matching `"` followed by `hashes` `#`s.
                loop {
                    if i >= bytes.len() {
                        break;
                    }
                    if bytes[i] == b'"' {
                        let closes = bytes[i + 1..]
                            .iter()
                            .take(hashes)
                            .filter(|b| **b == b'#')
                            .count();
                        if closes == hashes {
                            for _ in 0..=hashes {
                                out.push(BLANK);
                            }
                            i += hashes + 1;
                            break;
                        }
                    }
                    out.push(if bytes[i] == b'\n' { b'\n' } else { BLANK });
                    i += 1;
                }
                continue;
            }
        }

        // Ordinary string literal.
        if bytes[i] == b'"' {
            out.push(BLANK);
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    out.push(BLANK);
                    if i + 1 < bytes.len() {
                        out.push(if bytes[i + 1] == b'\n' { b'\n' } else { BLANK });
                    }
                    i += 2;
                    continue;
                }
                if bytes[i] == b'"' {
                    out.push(BLANK);
                    i += 1;
                    break;
                }
                out.push(if bytes[i] == b'\n' { b'\n' } else { BLANK });
                i += 1;
            }
            continue;
        }

        // Char literal — `'a'`, `'\n'`, `'\''`. Lifetimes (`'static`) have no
        // closing quote, so they fall through to the byte copy below.
        if bytes[i] == b'\'' {
            let is_escaped_char = bytes.get(i + 1) == Some(&b'\\');
            let close = if is_escaped_char { i + 3 } else { i + 2 };
            if bytes.get(close) == Some(&b'\'') {
                for _ in i..=close {
                    out.push(BLANK);
                }
                i = close + 1;
                continue;
            }
        }

        out.push(bytes[i]);
        i += 1;
    }

    String::from_utf8(out).expect("blanking preserves ASCII structure byte-for-byte")
}

/// Every `#[test]` function in `source`, with its body.
///
/// Works for plain `#[test] fn …` and for the `#[test] fn …` forms inside a
/// `proptest! { … }` block, because it keys off the attribute rather than the
/// surrounding item.
fn test_functions(source: &str) -> Vec<TestFn> {
    let blanked = blank_comments_and_literals(source);
    let bytes = blanked.as_bytes();
    let mut found = Vec::new();
    let mut search_from = 0usize;

    while let Some(rel) = blanked[search_from..].find("#[test]") {
        let attr_at = search_from + rel;
        search_from = attr_at + "#[test]".len();

        // The next `fn` after the attribute begins the function.
        let Some(fn_rel) = blanked[search_from..].find("fn ") else {
            break;
        };
        let fn_at = search_from + fn_rel + "fn ".len();

        let name: String = blanked[fn_at..]
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if name.is_empty() {
            continue;
        }

        // Brace-match the body from the first `{` at or after the signature.
        let Some(open_rel) = blanked[fn_at..].find('{') else {
            break;
        };
        let open_at = fn_at + open_rel;
        let mut depth = 0usize;
        let mut end_at = open_at;
        for (offset, byte) in bytes[open_at..].iter().enumerate() {
            match byte {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        end_at = open_at + offset + 1;
                        break;
                    }
                },
                _ => {},
            }
        }
        if end_at <= open_at {
            continue; // unbalanced — leave it to the compiler to complain
        }

        // Slice the ORIGINAL source: offsets are preserved by the blanking, and
        // token matching wants the real text minus comments/literals, which is
        // exactly the blanked copy.
        found.push(TestFn {
            name,
            body: blanked[open_at..end_at].to_string(),
        });
        search_from = end_at;
    }

    found
}

fn reaches_store(body: &str) -> bool {
    STORE_REACHING_TOKENS.iter().any(|t| body.contains(t))
}

fn holds_a_guard(body: &str) -> bool {
    GUARD_TOKENS.iter().any(|t| body.contains(t))
}

fn tests_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests")
}

/// `models/tests/*.rs`, sorted for deterministic output.
fn test_sources() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(tests_dir())
        .expect("models/tests must be readable")
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "rs"))
        .collect();
    files.sort();
    files
}

// ---------------------------------------------------------------------------
// The policy test
// ---------------------------------------------------------------------------

#[test]
fn every_store_reaching_test_in_a_guarded_file_holds_a_guard() {
    let mut files_scanned = 0usize;
    let mut test_fns_found = 0usize;
    let mut store_reaching = 0usize;
    let mut guarded_files: BTreeSet<String> = BTreeSet::new();
    let mut violations: Vec<String> = Vec::new();

    for path in test_sources() {
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("reading {} failed: {e}", path.display()));
        files_scanned += 1;

        let functions = test_functions(&source);
        test_fns_found += functions.len();

        let file_defines_a_guard = blank_comments_and_literals(&source).contains(GUARD_DEFINITION);
        let file_name = path
            .file_name()
            .expect("a test source has a file name")
            .to_string_lossy()
            .to_string();
        if file_defines_a_guard {
            guarded_files.insert(file_name.clone());
        }

        for function in functions {
            if !reaches_store(&function.body) {
                continue;
            }
            store_reaching += 1;
            if file_defines_a_guard && !holds_a_guard(&function.body) {
                violations.push(format!("{file_name}::{}", function.name));
            }
        }
    }

    // Floors 1-3: the scan must have actually done something. Checked BEFORE
    // the policy verdict, so "found nothing" fails loudly instead of passing.
    assert!(
        files_scanned >= MIN_TEST_FILES_SCANNED,
        "scanned only {files_scanned} test files (floor {MIN_TEST_FILES_SCANNED}) — the \
         directory walk is broken, so this policy is checking nothing"
    );
    assert!(
        test_fns_found >= MIN_TEST_FUNCTIONS_FOUND,
        "recognised only {test_fns_found} #[test] functions (floor {MIN_TEST_FUNCTIONS_FOUND}) — \
         the function scanner is broken, so this policy is checking nothing"
    );
    assert!(
        store_reaching >= MIN_STORE_REACHING_TESTS,
        "found only {store_reaching} store-reaching tests (floor {MIN_STORE_REACHING_TESTS}) — \
         STORE_REACHING_TOKENS has stopped matching, so this policy is exempting everything"
    );
    assert!(
        !guarded_files.is_empty(),
        "no test file declares `{GUARD_DEFINITION}` — either the guards were removed (in which \
         case the store observations they protect are now unsound) or this policy has lost \
         track of them"
    );

    assert!(
        violations.is_empty(),
        "these tests reach the process-wide intern store without holding their file's guard:\n  \
         {}\n\n\
         Under `cargo test` every test in a binary shares one process, so an unguarded intern \
         makes the store's contents depend on the schedule — and any test in the same file that \
         OBSERVES store state then reads its neighbours instead of itself. Take \
         `interning_guard()` (shared — you only intern) or `store_guard()` (exclusive — you \
         observe store state). Guarded files: {:?}",
        violations.join("\n  "),
        guarded_files
    );
}

/// Floor 4: the detector's JUDGEMENT, against shapes whose verdict is known —
/// including the two a naive scanner gets wrong (a brace inside a string
/// literal, and a commented-out store call).
#[test]
fn detector_flags_the_shapes_it_claims_to_flag() {
    const FIXTURE: &str = r####"
        fn store_guard() -> RwLockWriteGuard<'static, ()> { unimplemented!() }

        #[test]
        fn bad_interns_without_any_guard() {
            let map = fixture();
            let _ = map.intern();
        }

        #[test]
        fn good_takes_the_shared_guard() {
            let _guard = interning_guard();
            let _ = map.intern();
        }

        #[test]
        fn good_takes_the_exclusive_guard() {
            let _guard = store_guard();
            assert_eq!(intern_store_touches_for_test(), 0);
        }

        #[test]
        fn good_does_not_touch_the_store_at_all() {
            assert_eq!(map.encode_to_vec(), other.encode_to_vec());
        }

        #[test]
        fn good_brace_inside_a_string_literal_does_not_truncate_the_body() {
            // A naive brace matcher ends the body at the `}` in this literal and
            // never sees the intern below — a FALSE PASS.
            assert!(debug.starts_with("EPathMap { ps: ["));
            let _guard = interning_guard();
            let _ = map.intern();
        }

        #[test]
        fn good_commented_out_intern_is_not_a_store_reach() {
            // let _ = map.intern();
            assert!(true);
        }
    "####;

    let functions = test_functions(FIXTURE);
    let names: Vec<&str> = functions.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "bad_interns_without_any_guard",
            "good_takes_the_shared_guard",
            "good_takes_the_exclusive_guard",
            "good_does_not_touch_the_store_at_all",
            "good_brace_inside_a_string_literal_does_not_truncate_the_body",
            "good_commented_out_intern_is_not_a_store_reach",
        ],
        "the scanner must recover every #[test] function, in order"
    );

    let verdict = |name: &str| -> (bool, bool) {
        let f = functions
            .iter()
            .find(|f| f.name == name)
            .unwrap_or_else(|| panic!("fixture function {name} must be recovered"));
        (reaches_store(&f.body), holds_a_guard(&f.body))
    };

    assert_eq!(
        verdict("bad_interns_without_any_guard"),
        (true, false),
        "an unguarded intern must be flagged — this is the shape the policy exists for"
    );
    assert_eq!(
        verdict("good_takes_the_shared_guard"),
        (true, true),
        "an intern under the SHARED guard is compliant"
    );
    assert_eq!(
        verdict("good_takes_the_exclusive_guard"),
        (true, true),
        "a store OBSERVATION under the EXCLUSIVE guard is compliant"
    );
    assert_eq!(
        verdict("good_does_not_touch_the_store_at_all"),
        (false, false),
        "a test that never reaches the store needs no guard"
    );
    assert_eq!(
        verdict("good_brace_inside_a_string_literal_does_not_truncate_the_body"),
        (true, true),
        "an opening brace inside a string literal must not end the body early — otherwise the \
         scanner silently stops seeing the rest of every such test"
    );
    assert_eq!(
        verdict("good_commented_out_intern_is_not_a_store_reach"),
        (false, false),
        "a commented-out store call must not count as reaching the store"
    );

    // And the policy loop's own verdict on the fixture: exactly one violation.
    let flagged: Vec<&str> = functions
        .iter()
        .filter(|f| reaches_store(&f.body) && !holds_a_guard(&f.body))
        .map(|f| f.name.as_str())
        .collect();
    assert_eq!(
        flagged,
        vec!["bad_interns_without_any_guard"],
        "the fixture must produce exactly one violation"
    );
}
