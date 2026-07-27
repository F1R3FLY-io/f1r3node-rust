//! G1 — the source policy: **no RAII type inside a lazily-initialised `static`**.
//!
//! # Why this exists
//!
//! Rust runs no destructors for `static`s. Putting a type whose entire contract is `Drop`
//! into a `lazy_static!` / `Lazy` / `LazyLock` / `OnceLock` / `OnceCell` therefore does not
//! "clean up at program exit"; it never cleans up at all. That is not a hypothetical: this
//! workspace shipped
//!
//! ```ignore
//! lazy_static! {
//!     // "Automatic cleanup when TempDir is dropped (at program exit)."
//!     static ref SHARED_LMDB_ENV: (PathBuf, TempDir) = { /* ... */ };
//! }
//! ```
//!
//! in two diverged copies, and it leaked 285 directories / 824 MB of `tmpfs` in 145 seconds of
//! a single test run before anyone noticed.
//!
//! No off-the-shelf clippy lint covers this shape. `clippy::let_underscore_lock`,
//! `clippy::mem_forget` and friends all look at expressions; the defect is a *placement*: a
//! `Drop`-carrying type in storage that is never dropped. So the policy is enforced here, in
//! source, as a test.
//!
//! # What it does
//!
//! Walks every `.rs` file in the workspace, finds every lazily-initialised `static`, and fails
//! if the static's type-or-initialiser mentions a denylisted RAII type. Known cases live in
//! [`ALLOWLIST`], keyed by file **and** symbol, each carrying a one-line justification — so a
//! known case is a *recorded decision* rather than a silent omission, and a new one cannot be
//! introduced without editing this file and writing down why.
//!
//! The scan is deterministic (sorted output), reads nothing but the source tree, and holds no
//! shared state, so it is safe under parallel execution.
//!
//! # Why it cannot pass vacuously
//!
//! Three independent guards, all in [`policy_holds_for_the_whole_workspace`] and
//! [`detector_flags_the_shapes_it_claims_to_flag`]:
//!
//! 1. the walk must have visited a plausible number of files, so a broken root or a broken
//!    directory filter fails instead of finding nothing;
//! 2. the detector must have recognised a plausible number of lazily-initialised statics, so a
//!    broken region finder fails instead of finding nothing;
//! 3. every [`ALLOWLIST`] entry must still match something, so entries cannot rot into
//!    permanent blanket exemptions for code that has since moved or been deleted;
//! 4. and the detector is run against a fixture with known-bad and known-good shapes, so its
//!    *judgement* is checked rather than assumed.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------------------
// Policy
// ---------------------------------------------------------------------------------------

/// Types whose contract is `Drop`. Matched as whole identifiers, so `Env` catches `heed::Env`
/// without catching `LmdbEnvConfig` or `EnvFilter`.
const RAII_TYPES: &[&str] = &[
    "TempDir",
    "NamedTempFile",
    "File",
    "TcpListener",
    "UdpSocket",
    "UnixListener",
    "Child",
    "Runtime",
    "Env",
];

/// Container types that defer initialisation to first use, and are therefore never dropped
/// when they appear in `static` position.
const LAZY_CONTAINERS: &[&str] = &["Lazy", "LazyLock", "OnceLock", "OnceCell"];

/// Recorded exemptions: `(workspace-relative path, symbol, justification)`.
///
/// Every entry must still match a real finding — an entry that matches nothing fails the test,
/// so this list cannot silently outlive the code it describes.
const ALLOWLIST: &[(&str, &str, &str)] = &[
    (
        "rspace++/libs/rspace_rhotypes/src/lib.rs",
        "RT",
        "Never-dropped tokio Runtime in a cdylib: leaks its worker threads and their fds on \
         every dlopen/dlclose cycle. Separately filed; out of scope for the scratch-directory \
         work that introduced this gate. Not a scratch directory, so the two-layer cleanup \
         does not apply.",
    ),
    (
        "rspace++/src/rspace/shared/env_cache.rs",
        "ENV_CACHE",
        "Holds `Weak<Env>`, not `Env`. The cache deliberately does not own the environments, \
         so nothing here is kept alive by the static and there is no destructor to miss; the \
         owning `Arc<Env>` lives in the caller's `LmdbDirStoreManager`.",
    ),
    (
        "block-storage/tests/block_dag_storage_test.rs",
        "RUNTIME",
        "Never-dropped tokio Runtime in a test binary. Its worker threads and fds die with the \
         process, which for a per-test-binary runtime is the intended lifetime; the leak is \
         bounded by one runtime per process and does not touch the filesystem.",
    ),
    (
        "casper/tests/batch2/lmdb_key_value_store_spec.rs",
        "RUNTIME",
        "Never-dropped tokio Runtime in a test binary; same bounded, process-lifetime \
         rationale as block-storage's.",
    ),
    (
        "casper/tests/util/in_memory_key_value_store_spec.rs",
        "RUNTIME",
        "Never-dropped tokio Runtime in a test binary; same bounded, process-lifetime \
         rationale as block-storage's.",
    ),
];

/// Directory names never descended into.
const SKIP_DIRS: &[&str] = &["target", ".git", ".jj", "node_modules", ".cargo"];

/// Floor on files visited. The workspace has over a thousand `.rs` files; if the walk finds
/// dramatically fewer, the root or the filter is broken and the test must say so rather than
/// pass by finding nothing to complain about.
const MIN_FILES_SCANNED: usize = 500;

/// Floor on lazily-initialised statics recognised. Same reasoning, one level down: this
/// catches a region finder that has stopped finding regions. The workspace currently has 63
/// across 1052 files; the floor leaves room for churn while still being far above the zero a
/// broken detector would report.
const MIN_LAZY_STATICS_FOUND: usize = 40;

// ---------------------------------------------------------------------------------------
// Model
// ---------------------------------------------------------------------------------------

/// One lazily-initialised `static`, with enough context to judge and to report it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct LazyStatic {
    /// Workspace-relative path, so messages and allowlist keys are stable.
    file: String,
    symbol: String,
    line: usize,
    /// Declaration text with comments and string literals blanked out.
    declaration: String,
}

impl LazyStatic {
    /// Denylisted RAII types mentioned anywhere in this static's type or initialiser.
    fn raii_types(&self) -> Vec<&'static str> {
        let identifiers = identifiers_in(&self.declaration);
        RAII_TYPES
            .iter()
            .copied()
            .filter(|raii| identifiers.contains(*raii))
            .collect()
    }
}

// ---------------------------------------------------------------------------------------
// Lexing: blank out comments and string literals
// ---------------------------------------------------------------------------------------

/// Replace every comment and every string/character literal with spaces, preserving newlines.
///
/// Both halves matter. Blanking **comments** stops prose from tripping the denylist — the
/// module documentation of the very fix this gate guards discusses `TempDir` at length, and a
/// scanner that read comments would flag it. Blanking **string literals** stops a literal
/// containing `{`, `}` or `;` from corrupting the brace-depth tracking below.
fn blank_comments_and_literals(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let mut index = 0usize;

    /// Push `count` blanks, keeping any newlines so line numbers survive.
    fn blank(out: &mut String, chars: &[char], from: usize, to: usize) {
        for &character in &chars[from..to] {
            out.push(if character == '\n' { '\n' } else { ' ' });
        }
    }

    while index < chars.len() {
        let character = chars[index];
        let next = chars.get(index + 1).copied();

        // Line comment.
        if character == '/' && next == Some('/') {
            let start = index;
            while index < chars.len() && chars[index] != '\n' {
                index += 1;
            }
            blank(&mut out, &chars, start, index);
            continue;
        }

        // Block comment, which nests in Rust.
        if character == '/' && next == Some('*') {
            let start = index;
            let mut depth = 0usize;
            while index < chars.len() {
                if chars[index] == '/' && chars.get(index + 1) == Some(&'*') {
                    depth += 1;
                    index += 2;
                } else if chars[index] == '*' && chars.get(index + 1) == Some(&'/') {
                    depth -= 1;
                    index += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    index += 1;
                }
            }
            blank(&mut out, &chars, start, index);
            continue;
        }

        // Raw string: r"..." / r#"..."# / br#"..."#.
        if let Some(hashes) = raw_string_opening(&chars, index) {
            let start = index;
            index = skip_raw_string(&chars, index, hashes);
            blank(&mut out, &chars, start, index);
            continue;
        }

        // Ordinary string or byte string.
        if character == '"' || (character == 'b' && next == Some('"')) {
            let start = index;
            if character == 'b' {
                index += 1;
            }
            index += 1; // opening quote
            while index < chars.len() {
                match chars[index] {
                    '\\' => index += 2,
                    '"' => {
                        index += 1;
                        break;
                    }
                    _ => index += 1,
                }
            }
            blank(&mut out, &chars, start, index);
            continue;
        }

        // Character literal, distinguished from a lifetime by what follows the quote.
        if character == '\'' && is_character_literal(&chars, index) {
            let start = index;
            index += 1;
            while index < chars.len() {
                match chars[index] {
                    '\\' => index += 2,
                    '\'' => {
                        index += 1;
                        break;
                    }
                    _ => index += 1,
                }
            }
            blank(&mut out, &chars, start, index);
            continue;
        }

        out.push(character);
        index += 1;
    }

    out
}

/// If a raw-string literal starts at `index`, the number of `#` in its delimiter.
fn raw_string_opening(chars: &[char], index: usize) -> Option<usize> {
    let mut cursor = index;
    if chars.get(cursor) == Some(&'b') {
        cursor += 1;
    }
    if chars.get(cursor) != Some(&'r') {
        return None;
    }
    // `r` must start an identifier boundary, otherwise it is part of a longer name.
    if index > 0 && is_identifier_char(chars[index - 1]) {
        return None;
    }
    cursor += 1;
    let mut hashes = 0usize;
    while chars.get(cursor) == Some(&'#') {
        hashes += 1;
        cursor += 1;
    }
    if chars.get(cursor) == Some(&'"') {
        Some(hashes)
    } else {
        None
    }
}

/// Index just past the end of the raw string starting at `index`.
fn skip_raw_string(chars: &[char], index: usize, hashes: usize) -> usize {
    let mut cursor = index;
    while chars.get(cursor) != Some(&'"') {
        cursor += 1;
    }
    cursor += 1;
    loop {
        if cursor >= chars.len() {
            return cursor;
        }
        if chars[cursor] == '"' {
            let closing = (1..=hashes).all(|offset| chars.get(cursor + offset) == Some(&'#'));
            if closing {
                return cursor + 1 + hashes;
            }
        }
        cursor += 1;
    }
}

/// Tell a character literal from a lifetime. `'a` is a lifetime; `'a'` is a literal.
fn is_character_literal(chars: &[char], index: usize) -> bool {
    match chars.get(index + 1) {
        Some('\\') => true,
        Some(_) => chars.get(index + 2) == Some(&'\''),
        None => false,
    }
}

fn is_identifier_char(character: char) -> bool { character.is_alphanumeric() || character == '_' }

/// Every identifier in `text`, for whole-word denylist matching.
fn identifiers_in(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut current = String::new();
    for character in text.chars() {
        if is_identifier_char(character) {
            current.push(character);
        } else if !current.is_empty() {
            found.insert(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        found.insert(current);
    }
    found
}

// ---------------------------------------------------------------------------------------
// Region finding
// ---------------------------------------------------------------------------------------

/// True when `chars[index..]` starts the keyword `keyword` at an identifier boundary.
///
/// The leading-`'` exclusion is what keeps `&'static str` and every `'static` lifetime bound
/// out of the results.
fn keyword_at(chars: &[char], index: usize, keyword: &str) -> bool {
    let keyword: Vec<char> = keyword.chars().collect();
    if index + keyword.len() > chars.len() {
        return false;
    }
    if chars[index..index + keyword.len()] != keyword[..] {
        return false;
    }
    if index > 0 {
        let previous = chars[index - 1];
        if is_identifier_char(previous) || previous == '\'' {
            return false;
        }
    }
    // The keyword must also end at a boundary, so `staticky` is not `static`.
    !matches!(chars.get(index + keyword.len()), Some(&following) if is_identifier_char(following))
}

/// Index just past the `;` that ends the declaration starting at `from`, tracking nesting so
/// that a `;` inside `[u8; 4]` or inside an initialiser block does not end it early.
fn end_of_declaration(chars: &[char], from: usize) -> usize {
    let mut depth = 0i32;
    let mut cursor = from;
    while cursor < chars.len() {
        match chars[cursor] {
            '{' | '(' | '[' => depth += 1,
            '}' | ')' | ']' => depth -= 1,
            ';' if depth <= 0 => return cursor + 1,
            _ => {}
        }
        cursor += 1;
    }
    chars.len()
}

/// Index just past the `}` closing the block whose `{` is at `from`.
fn end_of_block(chars: &[char], from: usize) -> usize {
    let mut depth = 0i32;
    let mut cursor = from;
    while cursor < chars.len() {
        match chars[cursor] {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return cursor + 1;
                }
            }
            _ => {}
        }
        cursor += 1;
    }
    chars.len()
}

/// The symbol name declared by a region that begins at the `static` keyword.
fn symbol_of(chars: &[char], static_index: usize) -> String {
    let mut cursor = static_index + "static".len();
    loop {
        while cursor < chars.len() && chars[cursor].is_whitespace() {
            cursor += 1;
        }
        // `static ref NAME` (lazy_static) and `static mut NAME` both have a modifier here.
        if keyword_at(chars, cursor, "ref") {
            cursor += "ref".len();
            continue;
        }
        if keyword_at(chars, cursor, "mut") {
            cursor += "mut".len();
            continue;
        }
        break;
    }
    let start = cursor;
    while cursor < chars.len() && is_identifier_char(chars[cursor]) {
        cursor += 1;
    }
    chars[start..cursor].iter().collect()
}

fn line_of(chars: &[char], index: usize) -> usize {
    chars[..index].iter().filter(|&&c| c == '\n').count() + 1
}

/// Find every lazily-initialised `static` in one file's source.
///
/// Two shapes are recognised:
///
/// * `lazy_static! { ... }` (and `lazy_static::lazy_static! { ... }`), where **every**
///   `static ref` inside the block is lazily initialised by construction; and
/// * a plain `static NAME: C<..> = ..;` whose declaration mentions one of [`LAZY_CONTAINERS`],
///   including `static`s declared inside a function body.
fn find_lazy_statics(relative_path: &str, source: &str) -> Vec<LazyStatic> {
    let blanked = blank_comments_and_literals(source);
    let chars: Vec<char> = blanked.chars().collect();
    let mut found = Vec::new();
    let mut lazy_static_blocks: Vec<(usize, usize)> = Vec::new();

    // Pass 1 — `lazy_static!` macro blocks.
    let mut cursor = 0usize;
    while cursor < chars.len() {
        if keyword_at(&chars, cursor, "lazy_static") {
            let mut probe = cursor + "lazy_static".len();
            // Tolerate `lazy_static::lazy_static!`.
            if chars.get(probe) == Some(&':') && chars.get(probe + 1) == Some(&':') {
                cursor = probe + 2;
                continue;
            }
            while probe < chars.len() && chars[probe].is_whitespace() {
                probe += 1;
            }
            if chars.get(probe) == Some(&'!') {
                probe += 1;
                while probe < chars.len() && chars[probe].is_whitespace() {
                    probe += 1;
                }
                if chars.get(probe) == Some(&'{') {
                    let block_end = end_of_block(&chars, probe);
                    lazy_static_blocks.push((probe, block_end));
                    // Every `static` inside the block is a lazily-initialised static.
                    let mut inner = probe;
                    while inner < block_end {
                        if keyword_at(&chars, inner, "static") {
                            let declaration_end = end_of_declaration(&chars, inner).min(block_end);
                            found.push(LazyStatic {
                                file: relative_path.to_string(),
                                symbol: symbol_of(&chars, inner),
                                line: line_of(&chars, inner),
                                declaration: chars[inner..declaration_end].iter().collect(),
                            });
                            inner = declaration_end;
                            continue;
                        }
                        inner += 1;
                    }
                    cursor = block_end;
                    continue;
                }
            }
        }
        cursor += 1;
    }

    // Pass 2 — plain `static` declarations naming a lazy container.
    let mut cursor = 0usize;
    while cursor < chars.len() {
        if keyword_at(&chars, cursor, "static")
            && !lazy_static_blocks
                .iter()
                .any(|&(start, end)| cursor >= start && cursor < end)
        {
            let declaration_end = end_of_declaration(&chars, cursor);
            let declaration: String = chars[cursor..declaration_end].iter().collect();
            let identifiers = identifiers_in(&declaration);
            if LAZY_CONTAINERS
                .iter()
                .any(|container| identifiers.contains(*container))
            {
                found.push(LazyStatic {
                    file: relative_path.to_string(),
                    symbol: symbol_of(&chars, cursor),
                    line: line_of(&chars, cursor),
                    declaration,
                });
            }
            cursor = declaration_end;
            continue;
        }
        cursor += 1;
    }

    found.sort();
    found
}

// ---------------------------------------------------------------------------------------
// Walking
// ---------------------------------------------------------------------------------------

fn workspace_root() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("shared/ has a parent")
        .to_path_buf();
    assert!(
        root.join("Cargo.toml").is_file(),
        "workspace root {} has no Cargo.toml — the walk would find nothing",
        root.display()
    );
    root
}

fn collect_rust_files(root: &Path, directory: &Path, into: &mut Vec<(String, PathBuf)>) {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        match entry.file_type() {
            Ok(file_type) if file_type.is_dir() => {
                if !SKIP_DIRS.contains(&name.as_str()) {
                    collect_rust_files(root, &path, into);
                }
            }
            Ok(file_type) if file_type.is_file() && name.ends_with(".rs") => {
                let relative = path
                    .strip_prefix(root)
                    .expect("walked path is under the root")
                    .to_string_lossy()
                    .into_owned();
                into.push((relative, path));
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------------------
// The gate
// ---------------------------------------------------------------------------------------

#[test]
fn policy_holds_for_the_whole_workspace() {
    let root = workspace_root();
    let mut files = Vec::new();
    collect_rust_files(&root, &root, &mut files);
    files.sort();

    assert!(
        files.len() >= MIN_FILES_SCANNED,
        "only {} .rs files were found under {} — expected at least {}. The walk is broken, and \
         a broken walk would pass this gate by finding nothing.",
        files.len(),
        root.display(),
        MIN_FILES_SCANNED
    );

    let mut all_lazy_statics = Vec::new();
    for (relative, path) in &files {
        let source = match fs::read_to_string(path) {
            Ok(source) => source,
            Err(_) => continue,
        };
        all_lazy_statics.extend(find_lazy_statics(relative, &source));
    }

    assert!(
        all_lazy_statics.len() >= MIN_LAZY_STATICS_FOUND,
        "only {} lazily-initialised statics were recognised across {} files — expected at least \
         {}. The region finder is broken, and a broken region finder would pass this gate by \
         finding nothing.",
        all_lazy_statics.len(),
        files.len(),
        MIN_LAZY_STATICS_FOUND
    );

    let mut violations = Vec::new();
    let mut matched_allowlist_entries = BTreeSet::new();

    for lazy_static in &all_lazy_statics {
        let raii = lazy_static.raii_types();
        if raii.is_empty() {
            continue;
        }
        match ALLOWLIST.iter().position(|(file, symbol, _)| {
            *file == lazy_static.file && *symbol == lazy_static.symbol
        }) {
            Some(index) => {
                matched_allowlist_entries.insert(index);
            }
            None => violations.push(format!(
                "  {}:{} — static `{}` holds {:?} in a lazily-initialised static.\n      \
                 Rust never drops statics, so its destructor will never run.",
                lazy_static.file, lazy_static.line, lazy_static.symbol, raii
            )),
        }
    }

    let stale: Vec<String> = ALLOWLIST
        .iter()
        .enumerate()
        .filter(|(index, _)| !matched_allowlist_entries.contains(index))
        .map(|(_, (file, symbol, _))| format!("  {file} :: {symbol}"))
        .collect();

    assert!(
        stale.is_empty(),
        "ALLOWLIST entries that no longer match anything — delete them, or the exemption \
         outlives the code it was written for:\n{}",
        stale.join("\n")
    );

    assert!(
        violations.is_empty(),
        "RAII types found inside lazily-initialised statics:\n{}\n\nEither move the value into \
         something whose lifetime is a scope, give it a cleanup mechanism that does not depend \
         on `Drop` (see `shared::rust::test_scratch`), or add it to ALLOWLIST in {} with a \
         justification.",
        violations.join("\n"),
        file!()
    );
}

// ---------------------------------------------------------------------------------------
// The detector's own judgement
// ---------------------------------------------------------------------------------------

/// The gate above only proves "nothing unexpected was found". That is exactly the shape of a
/// check that cannot fail, so the detector's judgement is pinned separately against a fixture
/// with known-bad and known-good shapes.
#[test]
fn detector_flags_the_shapes_it_claims_to_flag() {
    const FIXTURE: &str = r####"
        use tempfile::TempDir;

        // A comment mentioning TempDir must not, by itself, trip anything.
        lazy_static! {
            /// Doc comment mentioning Runtime and File.
            static ref LEAKED_DIR: (PathBuf, TempDir) = make_it();
            static ref CLEAN_COUNTER: AtomicU64 = AtomicU64::new(0);
        }

        static LEAKED_RUNTIME: OnceLock<Runtime> = OnceLock::new();
        static CLEAN_QUERY: OnceLock<Par> = OnceLock::new();
        static CLEAN_PLAIN: AtomicUsize = AtomicUsize::new(0);
        static CLEAN_ARRAY: [u8; 4] = [0; 4];
        static CLEAN_MESSAGE: &'static str = "a TempDir in a string literal is not a type";

        fn scoped() {
            static NESTED_LEAK: Lazy<File> = Lazy::new(|| File::create("x").unwrap());
            let genuinely_scoped: TempDir = TempDir::new().unwrap();
            drop(genuinely_scoped);
        }
    "####;

    let found = find_lazy_statics("fixture.rs", FIXTURE);
    let names: Vec<&str> = found.iter().map(|entry| entry.symbol.as_str()).collect();

    for expected in [
        "LEAKED_DIR",
        "CLEAN_COUNTER",
        "LEAKED_RUNTIME",
        "CLEAN_QUERY",
        "NESTED_LEAK",
    ] {
        assert!(
            names.contains(&expected),
            "the detector missed the lazily-initialised static `{expected}`; found {names:?}"
        );
    }
    for not_expected in ["CLEAN_PLAIN", "CLEAN_ARRAY", "CLEAN_MESSAGE"] {
        assert!(
            !names.contains(&not_expected),
            "`{not_expected}` is not lazily initialised and must not be reported; found {names:?}"
        );
    }

    let flagged: Vec<&str> = found
        .iter()
        .filter(|entry| !entry.raii_types().is_empty())
        .map(|entry| entry.symbol.as_str())
        .collect();

    assert_eq!(
        flagged,
        ["LEAKED_DIR", "LEAKED_RUNTIME", "NESTED_LEAK"],
        "the detector must flag exactly the RAII-in-static cases and nothing else"
    );
}

/// The comment- and literal-blanking pass is what stops this very file, and the module
/// documentation of `shared::rust::test_scratch`, from tripping the denylist. Pin it.
#[test]
fn prose_and_string_literals_never_trip_the_denylist() {
    const FIXTURE: &str = r####"
        /// This doc comment explains at length why a TempDir in a static is wrong,
        /// and mentions Runtime, File and TcpListener while doing so.
        /* A block comment naming NamedTempFile and Child. */
        static DISCUSSED_BUT_CLEAN: OnceLock<PathBuf> = OnceLock::new();

        static ALSO_CLEAN: OnceLock<String> = OnceLock::new(); // trailing note about UdpSocket
    "####;

    let found = find_lazy_statics("fixture.rs", FIXTURE);
    assert_eq!(
        found.len(),
        2,
        "expected both statics to be recognised: {found:?}"
    );
    for entry in &found {
        assert!(
            entry.raii_types().is_empty(),
            "`{}` was flagged because of prose, not code: {:?}",
            entry.symbol,
            entry.raii_types()
        );
    }
}
