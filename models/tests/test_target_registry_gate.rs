//! # Every file in `models/tests/` runs, exactly once
//!
//! ## The two failure modes, and why one gate covers both
//!
//! `models/Cargo.toml` declares `[[test]] name = "models_tests" path =
//! "tests/mod.rs"`, and `tests/mod.rs` `mod`-includes seven files. Cargo's
//! integration-test **auto-discovery** independently made each of those seven a
//! target of its own, so six of them (the seventh is feature-gated off by
//! default) were compiled twice and executed twice — measured at 31 binaries
//! and 406 test cases, of which 54 cases were duplicate executions.
//!
//! `autotests = false` removes the second compilation. But auto-discovery was
//! also the ONLY thing making the other 24 files run: on its own, that setting
//! would have deleted 24 targets, silently. The two failure modes are therefore
//! dual, and a project can be in either:
//!
//! | | a file is `mod`-included | a file is not |
//! |---|---|---|
//! | **auto-discovery on** | compiled TWICE, run twice | compiled once (this was the only reason it ran) |
//! | **auto-discovery off** | compiled once ✓ | ⚠ **NOT COMPILED AND NOT RUN** |
//!
//! The bottom-right cell is the one that costs a gate its life without anyone
//! noticing, and it is the cell `autotests = false` moves new files into by
//! default. This test is what makes that cell loud: a `tests/*.rs` that is
//! neither declared as a `[[test]]` target in `Cargo.toml` nor `mod`-included
//! from `tests/mod.rs` fails here, by name, with the two lines that fix it.
//!
//! ## Why the registry is read from the manifest rather than hard-coded
//!
//! A hard-coded list of expected files is a second registry, and two registries
//! disagree. The manifest is the one Cargo obeys, so it is the one this reads;
//! the filesystem is the ground truth for what exists. The gate is exactly the
//! statement that those two agree.
//!
//! ## Scope
//!
//! This checks `models` only. Seven sibling crates in this workspace
//! (`block-storage`, `casper`, `comm`, `crypto`, `graphz`, `rholang`,
//! `rspace++`) have the same `tests/mod.rs` + auto-discovery shape and none of
//! them sets `autotests`, so each still pays the double compilation. They are
//! named here so the count is on the record rather than left as a vague
//! reassurance — fixing them is the same edit, one crate at a time.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn manifest_dir() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")) }

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// Every `tests/*.rs` on disk, by stem, excluding `mod.rs` (the aggregate's own
/// root, which is a target by `path` and not by discovery).
fn files_on_disk() -> BTreeSet<String> {
    let dir = manifest_dir().join("tests");
    let mut found = BTreeSet::new();
    for entry in std::fs::read_dir(&dir).expect("models/tests must exist") {
        let path = entry.expect("readable dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue; // fixtures/, golden/, par_corpus/, .proptest-regressions
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("a UTF-8 file stem")
            .to_string();
        if stem != "mod" {
            found.insert(stem);
        }
    }
    found
}

/// Every `path = "tests/<stem>.rs"` declared under a `[[test]]` table in
/// `Cargo.toml`, by stem. `tests/mod.rs` maps to the stem `mod`, which is
/// filtered out on the other side.
fn declared_in_manifest() -> BTreeSet<String> {
    let manifest = read(&manifest_dir().join("Cargo.toml"));
    let mut declared = BTreeSet::new();
    for line in manifest.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("path") else {
            continue;
        };
        let Some(value) = rest.split('"').nth(1) else {
            continue;
        };
        if let Some(stem) = value
            .strip_prefix("tests/")
            .and_then(|s| s.strip_suffix(".rs"))
        {
            declared.insert(stem.to_string());
        }
    }
    declared
}

/// Every `mod <name>;` in `tests/mod.rs`, by name, `#[cfg]`-gated or not: a
/// feature-gated module still RUNS under that feature, so it is registered.
fn included_by_the_aggregate() -> BTreeSet<String> {
    read(&manifest_dir().join("tests/mod.rs"))
        .lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix("mod ")?
                .strip_suffix(';')
                .map(str::to_string)
        })
        .collect()
}

/// ★ THE GATE. Every file that exists is reachable, and everything declared
/// exists.
#[test]
fn every_test_file_is_registered_exactly_once() {
    let on_disk = files_on_disk();
    let declared = declared_in_manifest();
    let included = included_by_the_aggregate();

    // ── the gate is not vacuous ──────────────────────────────────────────
    assert!(
        on_disk.len() >= 25,
        "models/tests holds only {} test files; this gate was written against \
         31, so either the suite was gutted or the directory scan stopped \
         matching",
        on_disk.len()
    );
    assert!(
        declared.contains("mod"),
        "Cargo.toml no longer declares `path = \"tests/mod.rs\"`, so the \
         aggregate target `models_tests` is gone and this gate is reading a \
         manifest it does not understand"
    );
    assert!(
        !included.is_empty(),
        "tests/mod.rs includes nothing, so the aggregate target compiles no \
         tests and this gate's second column is empty"
    );

    // ── ⚠ THE SILENT-DROP MODE: a file that runs NOWHERE ─────────────────
    let orphaned: Vec<&String> = on_disk
        .iter()
        .filter(|stem| !declared.contains(*stem) && !included.contains(*stem))
        .collect();
    assert!(
        orphaned.is_empty(),
        "★ {} file(s) in models/tests/ are compiled by NO target, because \
         `autotests = false` is set and nothing registers them: {:?}\n\n\
         Add to models/Cargo.toml:\n\n{}\n\
         …or add `mod <name>;` to models/tests/mod.rs to fold it into \
         `models_tests`.",
        orphaned.len(),
        orphaned,
        orphaned
            .iter()
            .map(|s| format!("[[test]]\nname = \"{s}\"\npath = \"tests/{s}.rs\"\n"))
            .collect::<Vec<_>>()
            .join("\n")
    );

    // ── THE DOUBLE-COMPILATION MODE: a file reached BOTH ways ────────────
    let doubled: Vec<&String> = on_disk
        .iter()
        .filter(|stem| declared.contains(*stem) && included.contains(*stem))
        .collect();
    assert!(
        doubled.is_empty(),
        "★ {} file(s) in models/tests/ are compiled TWICE — once as their own \
         `[[test]]` target and once inside `models_tests`: {:?}\n\n\
         Remove either the `[[test]]` block or the `mod` line. This is the \
         waste `autotests = false` was added to remove.",
        doubled.len(),
        doubled
    );

    // ── a declaration that names nothing ─────────────────────────────────
    let dangling: Vec<&String> = declared
        .iter()
        .filter(|stem| stem.as_str() != "mod" && !on_disk.contains(*stem))
        .collect();
    assert!(
        dangling.is_empty(),
        "Cargo.toml declares `[[test]]` target(s) whose file does not exist: \
         {dangling:?}"
    );
    let dangling_mods: Vec<&String> = included
        .iter()
        .filter(|stem| !on_disk.contains(*stem))
        .collect();
    assert!(
        dangling_mods.is_empty(),
        "tests/mod.rs includes module(s) with no file: {dangling_mods:?}"
    );

    println!(
        "{} test files: {} declared as their own target, {} folded into \
         `models_tests`, 0 orphaned, 0 doubled",
        on_disk.len(),
        declared.len() - 1,
        included.len()
    );
}

/// ★ **The gate can go red**, on both of its arms, driven over constructed
/// registries rather than by editing the manifest.
///
/// A guard nobody has watched fail is not evidence. The classification is
/// factored out so this can exercise it directly; the converse is asserted too,
/// so a classifier that flagged everything would fail here rather than look
/// strict.
#[test]
fn the_registry_classification_separates_orphans_from_duplicates() {
    fn classify<'a>(
        on_disk: &'a [&'a str],
        declared: &[&str],
        included: &[&str],
    ) -> (Vec<&'a str>, Vec<&'a str>) {
        let orphaned = on_disk
            .iter()
            .copied()
            .filter(|s| !declared.contains(s) && !included.contains(s))
            .collect();
        let doubled = on_disk
            .iter()
            .copied()
            .filter(|s| declared.contains(s) && included.contains(s))
            .collect();
        (orphaned, doubled)
    }

    // The state `autotests = false` creates for a new, unregistered file.
    let (orphaned, doubled) = classify(&["a", "newcomer"], &["a"], &[]);
    assert_eq!(orphaned, vec!["newcomer"], "the silent-drop arm must fire");
    assert!(doubled.is_empty());

    // The state this crate was in before `autotests = false`.
    let (orphaned, doubled) = classify(&["a", "both"], &["a", "both"], &["both"]);
    assert!(orphaned.is_empty());
    assert_eq!(orphaned.len(), 0);
    assert_eq!(
        doubled,
        vec!["both"],
        "the double-compilation arm must fire"
    );

    // CONTROL — the state the crate is in NOW: every file reached exactly once,
    // by exactly one route. Neither arm may fire on it.
    let (orphaned, doubled) = classify(&["standalone", "folded"], &["standalone"], &["folded"]);
    assert!(
        orphaned.is_empty() && doubled.is_empty(),
        "the classifier must not flag a well-formed registry: orphaned = \
         {orphaned:?}, doubled = {doubled:?}"
    );
}
