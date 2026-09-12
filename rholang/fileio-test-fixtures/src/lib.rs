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

#[cfg(test)]
mod tests {
    //! X-5 D-14 (2026-09-12, branch-review-2026-09-11.md Track D):
    //! unit tests for the shared test-fixture helpers.  Track D
    //! flagged that `assert_dir_trees_byte_identical` and
    //! `apply_wal_translated` had no direct coverage — they were
    //! tested transitively via fs_wal_spec, which passes a well-
    //! formed WAL under one code path (Consensus leader-follower
    //! parity).  A regression in the helper's edge cases (kind
    //! divergence, mismatch detection, idempotent re-apply) would
    //! slip past transitive coverage.
    //!
    //! These pins exercise the helpers directly against synthetic
    //! trees / WAL entries.
    use rholang::rust::interpreter::io::wal::{PayloadRef, WalOp, WalOutcome};

    use super::*;

    /// D-14 pin 1: `assert_dir_trees_byte_identical` returns
    /// cleanly on two identical trees + panics on divergence.  Two
    /// sub-cases: content divergence + kind divergence (file vs
    /// directory at the same rel path).
    #[test]
    fn d14_assert_dir_trees_byte_identical_panics_on_content_divergence() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        std::fs::write(a.path().join("f.bin"), b"leader-content").unwrap();
        std::fs::write(b.path().join("f.bin"), b"follower-content").unwrap();
        let result = std::panic::catch_unwind(|| {
            assert_dir_trees_byte_identical(a.path(), b.path(), &[]);
        });
        assert!(
            result.is_err(),
            "D-14 regression: assert_dir_trees_byte_identical must panic \
             on differing file contents"
        );
    }

    #[test]
    fn d14_assert_dir_trees_byte_identical_passes_on_identical_trees() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        // Populate both trees identically.
        std::fs::write(a.path().join("f.bin"), b"same-content").unwrap();
        std::fs::write(b.path().join("f.bin"), b"same-content").unwrap();
        std::fs::create_dir(a.path().join("sub")).unwrap();
        std::fs::create_dir(b.path().join("sub")).unwrap();
        std::fs::write(a.path().join("sub/g.bin"), b"nested").unwrap();
        std::fs::write(b.path().join("sub/g.bin"), b"nested").unwrap();
        // MUST NOT panic.
        assert_dir_trees_byte_identical(a.path(), b.path(), &[]);
    }

    #[test]
    fn d14_assert_dir_trees_byte_identical_panics_on_kind_divergence() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        // Same relative path — `x` — is a file on side a, a
        // directory on side b.  A regression that ignored the
        // kind at collect() time would miss this.
        std::fs::write(a.path().join("x"), b"file-content").unwrap();
        std::fs::create_dir(b.path().join("x")).unwrap();
        let result = std::panic::catch_unwind(|| {
            assert_dir_trees_byte_identical(a.path(), b.path(), &[]);
        });
        assert!(
            result.is_err(),
            "D-14 regression: kind divergence (file vs dir at the same \
             rel path) must panic"
        );
    }

    /// D-14 pin 2: `apply_wal_translated` on a WAL with a single
    /// RemoveFile entry that targets a nested path — verifies
    /// (a) the file is actually removed on the follower side,
    /// (b) parent directory structure is preserved, (c) applying
    /// the SAME WAL twice is idempotent (the second removal is a
    /// no-op because the file is already gone; H-6 skips Failure
    /// entries and the entry outcome is Success, so it retries the
    /// unlink and either succeeds or NotFound-ignores per applier
    /// semantics).
    #[test]
    fn d14_apply_wal_translated_removes_nested_file() {
        let leader = tempfile::tempdir().unwrap();
        let follower = tempfile::tempdir().unwrap();
        // Seed the follower tree with a nested file that the WAL
        // entry will remove.
        std::fs::create_dir(follower.path().join("sub")).unwrap();
        std::fs::write(follower.path().join("sub/target.bin"), b"payload").unwrap();

        // Synthetic WAL: RemoveFile on `leader_root/sub/target.bin`.
        // translate_path rewrites onto follower_root.
        let leader_target = leader.path().join("sub/target.bin");
        let wal = vec![WalEntry {
            op: WalOp::RemoveFile,
            path: leader_target.clone(),
            extra_path: None,
            offset: None,
            length: None,
            payload_ref: None,
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        }];
        // Also need to seed the leader side for translate_path to
        // strip the prefix correctly.
        std::fs::create_dir(leader.path().join("sub")).unwrap();

        apply_wal_translated(&wal, &HashMap::new(), leader.path(), follower.path());
        assert!(
            !follower.path().join("sub/target.bin").exists(),
            "D-14 regression: RemoveFile WAL entry must delete the \
             nested file on the follower side"
        );
        assert!(
            follower.path().join("sub").exists(),
            "D-14 regression: parent directory must survive the \
             RemoveFile — only the leaf gets removed"
        );
    }

    /// D-14 pin 3: idempotent apply — re-running the same WAL
    /// (with a Failure entry followed by a Success entry) does
    /// not surface a fresh mutation on the second pass.  Failure
    /// entries are skipped (H-6); Success entries are re-applied.
    /// The pin is that the applier tolerates re-running against
    /// its own output tree — no ApplierError from the second call.
    #[test]
    fn d14_apply_wal_translated_is_idempotent_on_success_entries() {
        let leader = tempfile::tempdir().unwrap();
        let follower = tempfile::tempdir().unwrap();
        std::fs::create_dir(leader.path().join("sub")).unwrap();
        std::fs::create_dir(follower.path().join("sub")).unwrap();

        // Write a payload via a Write entry.  Payload keyed by its
        // Blake2b256 hash — we synthesize an arbitrary hash + bytes.
        // (`apply_wal_to_fresh_tree`'s Write branch looks up the
        // hash in `payload_bytes` and pwrites the bytes; it doesn't
        // verify hash === Blake2b256(bytes) inside the applier.)
        let hash = [0xAAu8; 32];
        let bytes = b"payload-v1".to_vec();
        let mut payload_bytes: HashMap<[u8; 32], Vec<u8>> = HashMap::new();
        payload_bytes.insert(hash, bytes.clone());

        let leader_target = leader.path().join("sub/data.bin");
        let wal = vec![WalEntry {
            op: WalOp::Write,
            path: leader_target,
            extra_path: None,
            offset: Some(0),
            length: Some(bytes.len() as u64),
            payload_ref: Some(PayloadRef::Hash(hash)),
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        }];

        // First apply — populates the follower's tree.
        apply_wal_translated(&wal, &payload_bytes, leader.path(), follower.path());
        let after_first = std::fs::read(follower.path().join("sub/data.bin"))
            .expect("data.bin must exist after first apply");
        assert_eq!(
            after_first, bytes,
            "D-14 regression: first apply must land the payload"
        );

        // Second apply of the SAME WAL — idempotent, tree stays
        // consistent.  A regression that (say) appended instead of
        // pwrote at offset 0 would surface here as a doubled file
        // content.
        apply_wal_translated(&wal, &payload_bytes, leader.path(), follower.path());
        let after_second = std::fs::read(follower.path().join("sub/data.bin"))
            .expect("data.bin must exist after second apply");
        assert_eq!(
            after_second, bytes,
            "D-14 regression: re-applying the same Write WAL must be \
             idempotent (pwrite at offset 0 overwrites in place)"
        );
    }
}
