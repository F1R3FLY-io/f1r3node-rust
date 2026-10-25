//! PathMap native-algebra differential corpus (test-only).
//!
//! Every EPathMap operation recorded here delegates to a native PathMap call:
//! lattice join, meet, subtract and restrict, value removal, branch pruning,
//! path creation, focused-subtrie grafting and head dropping. The Rholang
//! collection methods reach the same calls (`rholang/src/rust/interpreter/
//! reduce.rs`): `union`, `graft` and `joinInto` through `try_join`, `diff`
//! through `try_subtract`, `intersection` through `try_meet`, and
//! `restriction` through `try_restrict_member_prefixes`.
//!
//! For each case the manifest records what the operation returned beside the
//! trie, the result's mode and length, its canonical keys in trie order (with
//! each map value), and its EPM1 snapshot, which is consensus-visible. The
//! manifest was blessed on PathMap 0.2.2 before the move to 0.3.4. The
//! default mode compares it with the committed manifest and names every case
//! whose record changed, so each behavioural difference of a PathMap release
//! can be classified on its own.
//!
//! The cases run in a child process of this test binary
//! (`native_algebra_corpus_child`), because a PathMap release may panic on
//! some of them and the workspace's panic policy forbids `catch_unwind` in
//! tests. The child prints each case before running it. When a child dies,
//! the parent records `panic <location> <message>` against the last case the
//! child printed and starts a new child at the next case.
//!
//! `PATHMAP_NATIVE_ALGEBRA_CORPUS_BLESS=1 cargo test -p models --test
//! pathmap_native_algebra_corpus` rewrites
//! `tests/fixtures/goldens/pathmap_native_algebra_corpus.txt`.

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, EPathMap, Expr, Par};
use models::rust::canonical_path::{decode_trie_path, encode_trie_path};
use models::rust::pathmap_integration::{par_to_path, CursorKind, RholangSetPathMap};
use models::rust::rhoapi_ext::EntryTrie;

const MANIFEST: &str = "pathmap_native_algebra_corpus.txt";
const BLESS: &str = "PATHMAP_NATIVE_ALGEBRA_CORPUS_BLESS";
const CHILD: &str = "native_algebra_corpus_child";
const FIRST_CASE: &str = "PATHMAP_NATIVE_ALGEBRA_CORPUS_FIRST_CASE";
const RECORD_MARK: &str = "@@corpus ";
const COMPLETE: &str = "complete";

fn expr(instance: ExprInstance) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(instance),
    }])
}

fn gint(value: i64) -> Par { expr(ExprInstance::GInt(value)) }

fn gstring(value: &str) -> Par { expr(ExprInstance::GString(value.to_owned())) }

fn list(items: Vec<Par>) -> Par {
    expr(ExprInstance::EListBody(EList {
        ps: items,
        locally_free: Vec::new(),
        connective_used: false,
        remainder: None,
    }))
}

fn ints(values: &[i64]) -> Par { list(values.iter().copied().map(gint).collect()) }

fn set(entries: Vec<Par>) -> EntryTrie {
    EPathMap::from_set_iter(entries, Vec::new(), false, None)
        .entry_trie()
        .clone()
}

fn map(entries: Vec<(Par, i64)>) -> EntryTrie {
    EPathMap::new_map(
        entries.into_iter().map(|(key, value)| (key, gint(value))),
        Vec::new(),
        false,
        None,
    )
    .entry_trie()
    .clone()
}

/// The trie with a value-free endpoint two bytes below the key of `member`.
fn with_dangling_path(trie: &EntryTrie, member: &Par) -> EntryTrie {
    let mut path = encode_trie_path(member);
    path.extend_from_slice(&[0xAA, 0xBB]);
    let mut dangling = trie.clone();
    assert!(
        dangling
            .create_path(&path)
            .expect("a nonempty trie has a mode"),
        "the dangling endpoint is new"
    );
    dangling
}

fn raw_keys(trie: &EntryTrie) -> Vec<Vec<u8>> {
    let mut keys = Vec::with_capacity(trie.len());
    if trie.map_trie().is_some() {
        trie.for_each_raw_map_entry(|key, _| keys.push(key.to_vec()))
            .expect("a map-mode trie visits map entries");
    } else {
        trie.for_each_raw_set_entry(|key| keys.push(key.to_vec()))
            .expect("a set-mode or empty trie visits set entries");
    }
    keys
}

fn key_mask(trie: &EntryTrie) -> RholangSetPathMap {
    let mut mask = RholangSetPathMap::new();
    for key in raw_keys(trie) {
        mask.insert(key, ());
    }
    mask
}

/// Proper prefixes of the longest key, then each full key: the positions a
/// prefix operation can name inside, at, and between stored values.
fn prefixes(trie: &EntryTrie) -> Vec<Vec<u8>> {
    let keys = raw_keys(trie);
    let longest = keys.iter().max_by_key(|key| key.len()).cloned();
    let mut positions = Vec::with_capacity(keys.len() + longest.as_ref().map_or(0, Vec::len));
    if let Some(longest) = longest {
        for length in 1..longest.len() {
            positions.push(longest[..length].to_vec());
        }
    }
    for key in keys {
        if !positions.contains(&key) {
            positions.push(key);
        }
    }
    positions
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(text, "{byte:02x}").expect("write to String");
    }
    text
}

fn value_label(value: &Par) -> String {
    match value.first::<Expr>() {
        Ok(Some((
            Expr {
                expr_instance: Some(ExprInstance::GInt(value)),
                ..
            },
            _,
        ))) => value.to_string(),
        other => format!("{other:?}"),
    }
}

/// What one operation returned beside its trie, then the trie; or the typed
/// error it refused with.
type Outcome = Result<(Option<String>, EntryTrie), String>;

fn produced(trie: EntryTrie) -> Outcome { Ok((None, trie)) }

fn produced_or_refused<E: std::fmt::Debug>(result: Result<EntryTrie, E>) -> Outcome {
    result
        .map(|trie| (None, trie))
        .map_err(|error| format!("{error:?}"))
}

fn returned(value: impl std::fmt::Display, trie: EntryTrie) -> Outcome {
    Ok((Some(value.to_string()), trie))
}

fn describe(outcome: Outcome) -> String {
    let mut body = String::new();
    match outcome {
        Err(error) => writeln!(body, "  error {error}").expect("write to String"),
        Ok((value, trie)) => {
            if let Some(value) = value {
                writeln!(body, "  returned {value}").expect("write to String");
            }
            writeln!(body, "  mode {:?} len {}", trie.mode(), trie.len()).expect("write to String");
            let mut keys = String::new();
            if trie.map_trie().is_some() {
                trie.for_each_raw_map_entry(|key, value| {
                    write!(keys, " {}={}", hex(key), value_label(value)).expect("write to String");
                })
                .expect("a map-mode trie visits map entries");
            } else {
                trie.for_each_raw_set_entry(|key| {
                    write!(keys, " {}", hex(key)).expect("write to String");
                })
                .expect("a set-mode or empty trie visits set entries");
            }
            writeln!(body, "  keys{keys}").expect("write to String");
            writeln!(body, "  epm1 {}", hex(trie.trie_snapshot())).expect("write to String");
        }
    }
    body
}

/// Prints manifest lines from the child, each behind [`RECORD_MARK`] so the
/// parent can separate them from libtest's own output.
fn emit(text: &str) {
    let mut out = std::io::stdout().lock();
    for line in text.lines() {
        writeln!(out, "{RECORD_MARK}{line}").expect("write a corpus record to stdout");
    }
    out.flush().expect("flush the corpus records");
}

/// Enumerates the cases in a fixed order and runs every case from `first` on.
/// The cases before `first` ran in an earlier child.
struct Corpus {
    next: usize,
    first: usize,
}

impl Corpus {
    fn record(&mut self, case: String, operation: impl FnOnce() -> Outcome) {
        let index = self.next;
        self.next += 1;
        if index < self.first {
            return;
        }
        emit(&format!("case {case}"));
        emit(&describe(operation()));
    }

    fn binary(&mut self, left: &(&str, EntryTrie), right: &(&str, EntryTrie), map_mode: bool) {
        let (left_name, left) = left;
        let (right_name, right) = right;
        let named = |op: &str| format!("{op} {left_name} {right_name}");
        self.record(named("join"), || produced_or_refused(left.try_join(right)));
        self.record(named("meet"), || produced_or_refused(left.try_meet(right)));
        self.record(named("subtract"), || {
            produced_or_refused(left.try_subtract(right))
        });
        self.record(named("restrict"), || {
            produced_or_refused(left.try_restrict(right))
        });
        self.record(named("restrict_member_prefixes"), || {
            produced_or_refused(left.try_restrict_member_prefixes(right))
        });
        self.record(named("subtract_encoded_keys"), || {
            produced(left.subtract_encoded_keys(&key_mask(right)))
        });
        self.record(named("select_encoded_keys"), || {
            produced(left.select_encoded_keys(&key_mask(right)))
        });
        if !map_mode {
            self.record(named("extend_entries"), || {
                let mut extended = left.clone();
                extended.extend_entries(right);
                produced(extended)
            });
        }
    }

    fn unary(&mut self, name: &str, trie: &EntryTrie, replacement: &EntryTrie) {
        let keys = raw_keys(trie);
        let mut absent = encode_trie_path(&gint(99));
        if keys.contains(&absent) {
            absent = encode_trie_path(&gint(98));
        }
        for key in keys.iter().chain(std::iter::once(&absent)) {
            self.record(format!("remove_encoded_entry {name} {}", hex(key)), || {
                let mut removed = trie.clone();
                let present = removed.remove_encoded_entry(key);
                returned(present, removed)
            });
        }
        for prefix in prefixes(trie) {
            self.record(
                format!("remove_branches_at {name} {}", hex(&prefix)),
                || {
                    let mut pruned = trie.clone();
                    let changed = pruned.remove_branches_at(&prefix);
                    returned(changed, pruned)
                },
            );
            self.record(format!("remove_subtrie_at {name} {}", hex(&prefix)), || {
                let mut removed = trie.clone();
                let changed = removed.remove_subtrie_at(&prefix);
                returned(changed, removed)
            });
            self.record(format!("subtrie {name} {}", hex(&prefix)), || {
                produced(trie.subtrie(&prefix))
            });
            self.record(format!("create_path {name} {}", hex(&prefix)), || {
                let mut created = trie.clone();
                match created.create_path(&prefix) {
                    Ok(new) => returned(new, created),
                    Err(error) => Err(format!("{error:?}")),
                }
            });
        }
        for count in 1..=2 {
            self.record(format!("drop_head {name} {count}"), || {
                produced(trie.drop_head(count))
            });
        }
        if trie.map_trie().is_some() {
            self.record(format!("remove_greatest_map_entry {name}"), || {
                let mut greatest = trie.clone();
                let removed = greatest
                    .remove_greatest_map_entry()
                    .map_err(|error| format!("{error:?}"))?;
                returned(removed.is_some(), greatest)
            });
        } else {
            self.record(format!("remove_greatest_entry {name}"), || {
                let mut greatest = trie.clone();
                let removed = greatest.remove_greatest_entry().is_some();
                returned(removed, greatest)
            });
        }
        self.record(format!("replace_subtrie {name} root"), || {
            let mut replaced = trie.clone();
            replaced
                .replace_subtrie(&[], CursorKind::Prefix, replacement)
                .map_err(|error| format!("{error:?}"))?;
            produced(replaced)
        });
        if let Some(first) = keys.first() {
            // The reducer's cursor naming the first member: its per-element
            // segments and kind (`EZipper.current_path` and `cursor_kind`).
            let member = decode_trie_path(first).expect("stored keys are canonical encodings");
            let cursor = par_to_path(&member);
            let kind = CursorKind::of(&member);
            self.record(
                format!("replace_subtrie {name} {} {kind:?}", hex(first)),
                || {
                    let mut replaced = trie.clone();
                    replaced
                        .replace_subtrie(&cursor, kind, replacement)
                        .map_err(|error| format!("{error:?}"))?;
                    produced(replaced)
                },
            );
        }
    }
}

fn run_cases(first: usize) {
    let wide: Vec<i64> = (0..12).collect();
    let wide_even: Vec<i64> = (0..12).step_by(2).collect();

    let empty = ("{}", set(Vec::new()));
    let one = ("{1}", set(vec![gint(1)]));
    let bare_and_list = (
        "{1,2,3,[1,2]}",
        set(vec![gint(1), gint(2), gint(3), ints(&[1, 2])]),
    );
    let list_1_2 = ("{[1,2]}", set(vec![ints(&[1, 2])]));
    let list_chain = (
        "{[1],[1,2],[1,2,3]}",
        set(vec![ints(&[1]), ints(&[1, 2]), ints(&[1, 2, 3])]),
    );
    let bare_chain = (
        "{7,[7],[7,7]}",
        set(vec![gint(7), ints(&[7]), ints(&[7, 7])]),
    );
    let list_7_7 = ("{[7,7]}", set(vec![ints(&[7, 7])]));
    let strings = (
        "{\"a\",\"ab\",\"abc\"}",
        set(vec![gstring("a"), gstring("ab"), gstring("abc")]),
    );
    let string_ab = ("{\"ab\"}", set(vec![gstring("ab")]));
    let wide_set = ("{0..11}", set(wide.iter().copied().map(gint).collect()));
    let wide_even_set = (
        "{0,2..10}",
        set(wide_even.iter().copied().map(gint).collect()),
    );
    let nested = ("{[[1]],[1]}", set(vec![list(vec![ints(&[1])]), ints(&[1])]));
    let bare_and_list_copy = ("{1,2,3,[1,2]}'", set(bare_and_list.1.entries_owned()));
    let dangling = (
        "{1,2,3,[1,2]}+1.aabb",
        with_dangling_path(&bare_and_list.1, &gint(1)),
    );

    let map_empty = ("{:}", map(Vec::new()));
    let map_bare_and_list = (
        "{1:10,2:20,3:30,[1,2]:12}",
        map(vec![
            (gint(1), 10),
            (gint(2), 20),
            (gint(3), 30),
            (ints(&[1, 2]), 12),
        ]),
    );
    let map_list_1_2 = ("{[1,2]:99}", map(vec![(ints(&[1, 2]), 99)]));
    let map_list_1_2_same = ("{[1,2]:12}", map(vec![(ints(&[1, 2]), 12)]));
    let map_wide = (
        "{0..11:10i}",
        map(wide.iter().map(|&key| (gint(key), key * 10)).collect()),
    );
    let map_wide_even = (
        "{0,2..10:10i}",
        map(wide_even.iter().map(|&key| (gint(key), key * 10)).collect()),
    );
    let map_copy = (
        "{1:10,2:20,3:30,[1,2]:12}'",
        map(vec![
            (gint(1), 10),
            (gint(2), 20),
            (gint(3), 30),
            (ints(&[1, 2]), 12),
        ]),
    );
    let map_dangling = (
        "{1:10,2:20,3:30,[1,2]:12}+1.aabb",
        with_dangling_path(&map_bare_and_list.1, &gint(1)),
    );

    let mut corpus = Corpus { next: 0, first };
    for (left, right) in [
        (&bare_and_list, &list_1_2),
        (&list_1_2, &bare_and_list),
        (&list_chain, &list_1_2),
        (&list_1_2, &list_chain),
        (&bare_chain, &list_7_7),
        (&list_7_7, &bare_chain),
        (&one, &bare_and_list),
        (&bare_and_list, &one),
        (&strings, &string_ab),
        (&string_ab, &strings),
        (&wide_set, &wide_even_set),
        (&wide_even_set, &wide_set),
        (&nested, &list_chain),
        (&list_chain, &nested),
        (&bare_and_list, &empty),
        (&empty, &bare_and_list),
        (&bare_and_list, &bare_and_list),
        (&bare_and_list, &bare_and_list_copy),
        (&dangling, &bare_and_list),
        (&bare_and_list, &dangling),
        (&dangling, &list_1_2),
        (&list_1_2, &dangling),
    ] {
        corpus.binary(left, right, false);
    }
    for (left, right) in [
        (&map_bare_and_list, &map_list_1_2),
        (&map_list_1_2, &map_bare_and_list),
        (&map_bare_and_list, &map_list_1_2_same),
        (&map_list_1_2_same, &map_bare_and_list),
        (&map_wide, &map_wide_even),
        (&map_wide_even, &map_wide),
        (&map_bare_and_list, &map_empty),
        (&map_empty, &map_bare_and_list),
        (&map_bare_and_list, &map_copy),
        (&map_dangling, &map_bare_and_list),
        (&map_bare_and_list, &map_dangling),
    ] {
        corpus.binary(left, right, true);
    }
    for (left, right) in [
        (&bare_and_list, &map_bare_and_list),
        (&map_bare_and_list, &bare_and_list),
    ] {
        corpus.binary(left, right, true);
    }
    for operand in [
        &one,
        &bare_and_list,
        &list_chain,
        &bare_chain,
        &strings,
        &wide_set,
        &nested,
        &dangling,
    ] {
        corpus.unary(operand.0, &operand.1, &list_1_2.1);
    }
    for operand in [&map_bare_and_list, &map_wide, &map_dangling] {
        corpus.unary(operand.0, &operand.1, &map_list_1_2.1);
    }
}

/// What one child invocation printed and whether it exited successfully.
struct ChildRun {
    ok: bool,
    stdout: String,
    stderr: String,
}

fn run_child(first: usize) -> ChildRun {
    let exe = std::env::current_exe().expect("pathmap_native_algebra_corpus: current_exe");
    let out = Command::new(exe)
        // `--nocapture` so the records and the panic message reach the pipes
        // verbatim instead of being folded into libtest's captured summary.
        .args(["--ignored", "--exact", CHILD, "--nocapture"])
        .env(FIRST_CASE, first.to_string())
        .stdin(Stdio::null())
        .output()
        .expect("pathmap_native_algebra_corpus: run the corpus child");
    ChildRun {
        ok: out.status.success(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// The child's panic as `<location> <message>`. A location inside a registry
/// crate keeps only the crate-relative part, and any other absolute location
/// only its file name, so the manifest carries no host path.
fn panic_summary(stderr: &str) -> String {
    let mut lines = stderr.lines();
    match lines
        .by_ref()
        .find_map(|line| line.split_once("panicked at "))
    {
        Some((_, at)) => {
            let location = at.trim_end_matches(':');
            let location = match location.rsplit_once("/registry/src/") {
                Some((_, registry)) => registry
                    .split_once('/')
                    .map_or(registry, |(_, crate_path)| crate_path),
                None if location.starts_with('/') => {
                    location.rsplit('/').next().unwrap_or(location)
                }
                None => location,
            };
            format!("panic {location} {}", lines.next().unwrap_or("").trim())
        }
        None => format!("died {}", stderr.lines().last().unwrap_or("").trim()),
    }
}

/// Runs the corpus in child processes and returns its manifest. A child that
/// dies leaves its last printed case without a body: that case's operation
/// did not return, so the parent records how the child died and resumes at
/// the next case.
fn build_corpus() -> String {
    let mut manifest = String::new();
    let mut first = 0usize;
    loop {
        let child = run_child(first);
        let mut printed = 0usize;
        let mut open_case = false;
        let mut complete = false;
        for line in child.stdout.lines() {
            let Some((_, record)) = line.split_once(RECORD_MARK) else {
                continue;
            };
            if record == COMPLETE {
                complete = true;
                continue;
            }
            open_case = record.starts_with("case ");
            printed += usize::from(open_case);
            manifest.push_str(record);
            manifest.push('\n');
        }
        if child.ok {
            assert!(
                complete,
                "the corpus child exited without running the corpus (first case {first}):\n{}\n{}",
                child.stdout, child.stderr
            );
            return manifest;
        }
        assert!(
            open_case,
            "the corpus child died outside a case (first case {first}):\n{}",
            child.stderr
        );
        writeln!(manifest, "  {}", panic_summary(&child.stderr)).expect("write to String");
        first += printed;
    }
}

fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("goldens")
        .join(MANIFEST)
}

#[test]
#[ignore = "child process of the native-algebra corpus; driven via PATHMAP_NATIVE_ALGEBRA_CORPUS_FIRST_CASE"]
fn native_algebra_corpus_child() {
    let Ok(first) = std::env::var(FIRST_CASE) else {
        println!("{CHILD}: no {FIRST_CASE}, not a child invocation, nothing to do");
        return;
    };
    run_cases(first.parse().expect("the first case index is a number"));
    emit(COMPLETE);
}

#[test]
fn native_algebra_results_match_the_blessed_pathmap_corpus() {
    let actual = build_corpus();
    assert!(
        actual == build_corpus(),
        "the corpus is deterministic across child processes"
    );
    let path = manifest_path();
    if std::env::var_os(BLESS).is_some() {
        std::fs::write(&path, &actual).expect("write the corpus manifest");
        println!("BLESSED {} ({} bytes)", path.display(), actual.len());
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "missing corpus manifest {}: {error}; bless it with {BLESS}=1",
            path.display()
        )
    });
    let changed: Vec<&str> = expected
        .split("\ncase ")
        .zip(actual.split("\ncase "))
        .filter(|(expected, actual)| expected != actual)
        .map(|(expected, _)| expected.lines().next().unwrap_or(""))
        .collect();
    assert!(
        expected == actual,
        "{} corpus cases changed (first lines shown; {} expected bytes, {} actual):\n{}",
        changed.len(),
        expected.len(),
        actual.len(),
        changed.join("\n")
    );
}
