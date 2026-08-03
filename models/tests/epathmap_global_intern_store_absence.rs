//! Source-policy gate for the removed process-global EPathMap intern store.

use std::fs;
use std::path::{Path, PathBuf};

const FORBIDDEN_STORE_SYMBOLS: &[&str] = &[
    "TRIE_INTERN",
    "interned_epathmap",
    "inject_intern_entry_for_test",
    "clear_intern_store_for_test",
    "intern_store_len_for_test",
    "intern_store_touches_for_test",
];

fn rust_sources(root: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(root).unwrap_or_else(|error| {
        panic!(
            "failed to read production source directory {}: {error}",
            root.display()
        )
    }) {
        let path = entry.expect("source directory entry").path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn process_global_epathmap_intern_store_remains_absent() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut sources = Vec::new();
    rust_sources(&root, &mut sources);
    sources.sort();
    assert!(
        sources.len() >= 40,
        "production-source scan found only {} Rust files under {}; refusing a vacuous pass",
        sources.len(),
        root.display()
    );

    let mut hits = Vec::new();
    for path in sources {
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        for symbol in FORBIDDEN_STORE_SYMBOLS {
            if source.contains(symbol) {
                hits.push(format!("{}: {symbol}", path.display()));
            }
        }
    }

    assert!(
        hits.is_empty(),
        "the removed process-global EPathMap intern store has returned:\n{}",
        hits.join("\n")
    );
}
