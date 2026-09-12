//! T-06 (2026-09-11, wave-4 Cluster D+E Phase 5): shared test-only
//! helpers for fileio WAL replay testing.
//!
//! Extracted from the inline `assert_dir_trees_byte_identical` /
//! `translate_path` / `apply_wal_translated` helpers previously
//! defined inside `rholang/tests/fs_wal_spec.rs::tests`.  Making
//! them a real crate lets any workspace test binary (rholang
//! integration tests, casper pb_m_14 canaries, future joiner
//! harnesses) drive follower-side WAL replay against a tempdir
//! tree without reimplementing the applier plumbing.
//!
//! # Why a separate crate (T-06 rationale)
//!
//! Pre-T-06 the helpers lived inside the `tests` module of
//! `fs_wal_spec.rs` and were shared with sub-modules only via the
//! `#[path]`-included `mutation.rs` / `observation.rs` `super::*`
//! re-export path.  Any other test binary that wanted the same
//! plumbing had to copy-paste the helpers — a duplication hazard
//! flagged in the A6 review as F-13.  Extracting to a crate
//! centralizes the helpers so a future refactor of the applier's
//! `ResolvedWalPath` shape (say, to carry symlink-safety cookies)
//! touches ONE definition, not N test-binary copies.
//!
//! # Circular dev-dep note
//!
//! This crate depends on `rholang` (for `WalEntry` /
//! `ResolvedWalPath` / `apply_wal_to_fresh_tree`); `rholang` in
//! turn lists THIS crate under `[dev-dependencies]` for its own
//! test binaries.  Circular dev-dep cycles are supported by
//! cargo — the two graphs are compiled independently.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use rholang::rust::interpreter::io::wal::WalEntry;
use rholang::rust::interpreter::io::wal_applier::{apply_wal_to_fresh_tree, ResolvedWalPath};

/// Recursively compare two directory trees for byte-identical file
/// contents + identical relative directory structure.  Ignores
/// mtime, uid/gid, and any files listed in `ignore` (relative paths
/// from either root, or `foo/` prefixes to ignore entire subtrees).
///
/// Panics with a diagnostic message on the first divergence:
///
/// - Tree layout differs (a key present on one side but not the
///   other).
/// - File byte contents differ.
/// - Same relative path is a file on one side and a directory on
///   the other.
///
/// Symlinks / other kinds are unexpected in fileio-consensus trees
/// (boot-time validation rejects them) and are skipped silently.
pub fn assert_dir_trees_byte_identical(a_root: &Path, b_root: &Path, ignore: &[&str]) {
    fn collect(
        root: &Path,
        base: &Path,
        ignore: &[&str],
        out: &mut BTreeMap<PathBuf, Option<Vec<u8>>>,
    ) {
        for entry in std::fs::read_dir(root).expect("read_dir") {
            let entry = entry.expect("dir entry");
            let path = entry.path();
            let rel = path.strip_prefix(base).unwrap().to_path_buf();
            let name = rel.to_string_lossy().to_string();
            if ignore
                .iter()
                .any(|p| name == *p || name.starts_with(&format!("{p}/")))
            {
                continue;
            }
            let ft = entry.file_type().expect("file_type");
            if ft.is_dir() {
                out.insert(rel.clone(), None); // directory marker
                collect(&path, base, ignore, out);
            } else if ft.is_file() {
                let bytes = std::fs::read(&path).expect("read file");
                out.insert(rel, Some(bytes));
            }
            // Symlinks / other kinds are unexpected in fileio-consensus
            // trees (boot-time validation rejects them); skip silently
            // to keep the helper focused.
        }
    }
    let mut a_map = BTreeMap::new();
    let mut b_map = BTreeMap::new();
    collect(a_root, a_root, ignore, &mut a_map);
    collect(b_root, b_root, ignore, &mut b_map);
    assert_eq!(
        a_map.keys().collect::<Vec<_>>(),
        b_map.keys().collect::<Vec<_>>(),
        "tree layout differs: leader={:?}, follower={:?}",
        a_map.keys().collect::<Vec<_>>(),
        b_map.keys().collect::<Vec<_>>(),
    );
    for (rel, a_val) in &a_map {
        let b_val = b_map.get(rel).unwrap();
        match (a_val, b_val) {
            (None, None) => {} // both directories
            (Some(a_bytes), Some(b_bytes)) => {
                assert_eq!(
                    a_bytes,
                    b_bytes,
                    "byte divergence at {rel:?}: leader_len={}, follower_len={}",
                    a_bytes.len(),
                    b_bytes.len(),
                );
            }
            _ => panic!(
                "kind divergence at {rel:?} (leader={:?}, follower={:?})",
                a_val.as_ref().map(|_| "file"),
                b_val.as_ref().map(|_| "file"),
            ),
        }
    }
}

/// Rewrite an absolute path from `leader_root/rel` into the
/// `(follower_root, rel, None)` triple the TOCTOU-safe applier
/// hands to `safe_descend_verified` (S-1 hardening 2026-09-03).
///
/// Panics if the path isn't rooted under `leader_root` — that's a
/// WAL entry the applier can't handle safely (an out-of-tree
/// canon_path would mean the leader saw a symlink escape, which
/// boot-time validation forbids in the consensus-static trees this
/// helper targets).
pub fn translate_path(leader_root: &Path, follower_root: &Path, p: &Path) -> ResolvedWalPath {
    let rel = p.strip_prefix(leader_root).unwrap_or_else(|_| {
        panic!(
            "WAL entry path {p:?} is not rooted under leader_root {leader_root:?}; \
             test harness invariant violated"
        )
    });
    ResolvedWalPath {
        root: follower_root.to_path_buf(),
        rel: rel.to_path_buf(),
        expected_root_id: None,
    }
}

/// Test-only wrapper for `apply_wal_to_fresh_tree` that translates
/// leader-tree WAL paths onto a follower tree via `translate_path`.
/// Production joiners construct the resolver from the boot registry
/// (`resolve_wal_entry_root_rel`); this helper keeps the
/// `pb_m_14_*` and `fs_wal_spec` call sites terse.
///
/// Passes empty `allowed_roots` — the test fixtures use tempdirs
/// so operator-frozen consensus-static-root validation is not
/// applicable; production sites plumb the actual roots.
///
/// Panics if the applier returns an error (which it should never do
/// for well-formed test WALs — anything else is a test-harness bug
/// or a WAL-applier regression that these tests are designed to
/// catch).
pub fn apply_wal_translated(
    wal: &[WalEntry],
    payload_bytes: &HashMap<[u8; 32], Vec<u8>>,
    leader_root: &Path,
    follower_root: &Path,
) {
    apply_wal_to_fresh_tree(
        wal,
        payload_bytes,
        |p| translate_path(leader_root, follower_root, p),
        &[],
    )
    .expect("test-driven WAL apply must not produce ApplierError");
}
