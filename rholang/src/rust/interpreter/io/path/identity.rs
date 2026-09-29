// Boot-time root capture + registry for the H-5 rename-and-recreate
// defense.
//
// # `Root` newtype
//
// `Root` is the ONLY way to hand a root path to
// [`super::descend::safe_descend`].  Constructing one goes through
// [`Root::capture`], which:
// - `.canonicalize()`s the input, resolving any symlinks in the
//   parent chain that would otherwise silently bypass the walk's
//   final-component `O_NOFOLLOW`;
// - opens the resolved directory with `O_DIRECTORY | O_NOFOLLOW`
//   and `fstat`s the resulting fd to capture the `(dev, inode)`
//   pair — TOCTOU-immune, mirroring the descend-side check.  See
//   [`Root::capture`]'s docstring for why this closes the two-
//   syscall race a naive `canonicalize + metadata` sequence would
//   have.  (The H-5 defense: an attacker who does
//   `mv legit legit.bak && mkdir legit && populate` produces a
//   fresh inode; the captured identity no longer matches.)
//
// Making canonicalization + identity capture a single ceremony that
// safe_descend statically requires closes the "root parent
// canonicalization" gap the PR 1.2 review flagged.
//
// # `RootIdentityRegistry`
//
// Populated once at boot from operator-provisioned root paths, then
// queried on every handler dispatch.  Maps a Rholang-side logical
// path (the `canonRoot` the caller hands us) to the `Root` the
// handler should descend into.  Wraps the inner map in an
// `Arc<RwLock<_>>` so per-syscall reads don't contend; boot-time
// writes take the write lock briefly.
//
// This is a Wave-1 minimal shape.  Shape-A (logical ≠ on-disk),
// share-across-runtime-clones (two-layer indirection), and the
// Consensus-mode gate that refuses unregistered dispatch all land
// in later waves; this PR ships the primitive.

use std::collections::HashMap;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use super::descend::{fstat_dev_inode, open_dir};
use super::{io_msg_scrub, QuarantineError};

/// A canonicalized on-disk root + its captured `(dev, inode)`
/// identity.
///
/// The ONLY way [`super::descend::safe_descend`] accepts a root:
/// callers must construct one via [`Root::capture`] at boot, then
/// hand out `&Root` clones from a registry.  This makes the
/// "root MUST be canonicalized" invariant compiler-enforced and
/// gives safe_descend a stable identity to verify against for
/// the H-5 defense.
#[derive(Debug, Clone)]
pub struct Root {
    canonical: PathBuf,
    identity: (u64, u64),
}

impl Root {
    /// Canonicalize `path`, open the resolved directory, and
    /// capture its `(dev, inode)` identity via `fstat` on the
    /// open fd.
    ///
    /// # Why open + fstat, not metadata
    ///
    /// A naive `canonicalize + metadata` sequence has a two-syscall
    /// race window: an attacker who wins the race can swap the
    /// resolved path between the two calls, and the captured
    /// identity would then reflect the attacker's inode.  The H-5
    /// check on later descends would compare against that already-
    /// poisoned baseline and pass.
    ///
    /// The `open + fstat` pattern (mirroring `safe_descend`'s per-
    /// descent check) closes the race: the fd binds to a specific
    /// inode at open time; `fstat` on that fd reads its metadata
    /// regardless of what happens to the path afterward.
    ///
    /// # Errors
    ///
    /// - `IoError(NotFound, _)` — path doesn't exist or a component
    ///   is unreadable (from `canonicalize`).
    /// - `IoError(_, _)` — the open fails for a non-symlink reason
    ///   (`ENOTDIR` if `path` resolves to a non-directory, `EACCES`
    ///   on permission denied, etc.).  Because `open_dir` uses
    ///   `O_DIRECTORY`, capture of a regular-file path fails
    ///   cleanly here rather than at first descend.
    /// - `SymlinkComponent` — post-canonicalize, the resolved path
    ///   was somehow (racily) a symlink.  `canonicalize` should
    ///   have unwound them; if this fires the underlying tree is
    ///   actively hostile.
    ///
    /// # Residual threat: intermediate symlink after capture
    ///
    /// Canonicalize resolves symlinks at capture time.  If an
    /// intermediate directory in the canonical path is *replaced*
    /// with a symlink between capture and a subsequent
    /// `safe_descend`, `open_dir`'s `O_NOFOLLOW` only guards the
    /// final component of `root`, so an intermediate swap can
    /// still redirect the descent.  Practical mitigation:
    /// operators MUST provision roots on filesystems where the
    /// node user does not have write access to the root's parent
    /// chain (e.g., read-only bind mounts, root-owned parents).
    pub fn capture(path: impl AsRef<Path>) -> Result<Root, QuarantineError> {
        let canonical = path
            .as_ref()
            .canonicalize()
            .map_err(|e| QuarantineError::IoError(e.kind(), io_msg_scrub(&e)))?;
        // open + fstat — TOCTOU-immune identity capture that also
        // enforces "is a directory" via `O_DIRECTORY` (the second
        // arg `true` enables `O_NOFOLLOW`; canonicalize already
        // resolved symlinks, but defense-in-depth is cheap).
        let fd = open_dir(&canonical, true)?;
        let identity = fstat_dev_inode(fd.as_raw_fd())?;
        Ok(Root {
            canonical,
            identity,
        })
    }

    /// The canonicalized on-disk path.
    pub fn path(&self) -> &Path { &self.canonical }

    /// The `(dev, inode)` pair captured at construction — the H-5
    /// defense's expected value for [`super::descend::safe_descend`]
    /// to compare against.
    pub fn identity(&self) -> (u64, u64) { self.identity }
}

/// Registry mapping Rholang-side logical roots to on-disk [`Root`]s.
///
/// Populated at boot from `node::setup::create_casper_infrastructure`
/// (once cross-crate wiring lands), then queried by every path-taking
/// handler to resolve a caller-supplied logical path to the on-disk
/// `Root` it should descend into.
///
/// # Key matching is byte-for-byte
///
/// Logical keys are stored in a `HashMap<PathBuf, Root>` and matched
/// via `PathBuf`'s byte-level `Hash` / `PartialEq`.
/// `PathBuf::from("/@bundle/example")` and
/// `PathBuf::from("/@bundle/./example")` are DIFFERENT keys and will
/// not cross-lookup.  Callers must supply the exact registered form.
/// Later waves that register logical→on-disk mappings (Shape-A)
/// normalize upstream; direct users of `register` / `get` are on
/// the hook for consistency.
///
/// # Thread safety
///
/// Thread-safe via an internal `RwLock`.  Reads (frequent,
/// per-syscall) don't contend; writes (rare, boot-time only) take
/// the write lock briefly.  `Clone` shares the backing map
/// (`Arc`-cloned), so a registry handed to a reducer clone stays
/// coherent with the manager's copy.
#[derive(Debug, Default, Clone)]
pub struct RootIdentityRegistry {
    inner: Arc<RwLock<Inner>>,
}

#[derive(Debug, Default)]
struct Inner {
    entries: HashMap<PathBuf, Root>,
}

impl RootIdentityRegistry {
    pub fn new() -> Self { Self::default() }

    /// Register a logical root → on-disk [`Root`] mapping.  Boot
    /// populates once; subsequent handler-path queries use [`get`].
    /// Idempotent: repeat register with the same logical key
    /// overwrites (last-write-wins).  In practice boot populates
    /// exactly once per logical root; overwrite semantics matter
    /// mostly for tests that build up a registry incrementally.
    ///
    /// [`get`]: RootIdentityRegistry::get
    pub fn register(&self, logical: PathBuf, root: Root) {
        // TODO: replace `.unwrap()` with `poison_abort` from
        // `io::errors` when that helper lands (deferred from
        // Wave 0 PR 0.1).  A poisoned lock here means a boot-time
        // writer panicked; propagating the panic is the correct
        // behavior, but `poison_abort` will make the message
        // consistent with the rest of the io tree.
        let mut guard = self.inner.write().unwrap();
        guard.entries.insert(logical, root);
    }

    /// Look up the [`Root`] for a logical path.  Returns `None`
    /// for unregistered logicals.
    pub fn get(&self, logical: &Path) -> Option<Root> {
        let guard = self.inner.read().unwrap();
        guard.entries.get(logical).cloned()
    }

    /// Count of registered roots.  Diagnostics only.
    pub fn len(&self) -> usize {
        let guard = self.inner.read().unwrap();
        guard.entries.len()
    }

    pub fn is_empty(&self) -> bool { self.len() == 0 }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;

    use tempfile::TempDir;

    use super::*;

    // --- Root::capture --------------------------------------------

    /// A `..`-containing input canonicalizes to the resolved
    /// absolute path.  Pins the "root must be canonicalized"
    /// invariant that PR 1.2 review flagged: constructing a Root
    /// with a non-canonical input yields a canonical one.
    #[test]
    fn root_capture_canonicalizes_dot_dot_paths() {
        let tmp = TempDir::new().unwrap();
        let sub = tmp.path().join("sub");
        fs::create_dir(&sub).unwrap();
        // `sub/../sub` resolves to `sub`.
        let root = Root::capture(sub.join("..").join("sub")).unwrap();
        assert_eq!(root.path(), sub.canonicalize().unwrap());
    }

    /// A symlink in the parent chain is resolved by `canonicalize`
    /// — the captured path is the real target, not the symlink.
    #[test]
    fn root_capture_resolves_symlinks_in_parent_chain() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("target");
        fs::create_dir(&target).unwrap();
        let link = tmp.path().join("link");
        symlink(&target, &link).unwrap();
        // Capture via the symlink — should resolve to `target`.
        let root = Root::capture(&link).unwrap();
        assert_eq!(root.path(), target.canonicalize().unwrap());
    }

    /// A nonexistent path fails capture — no silent fallback.
    /// `canonicalize` returns `NotFound`, which surfaces as
    /// `QuarantineError::IoError(NotFound, _)`.
    #[test]
    fn root_capture_fails_on_nonexistent_path() {
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("does-not-exist");
        match Root::capture(&missing) {
            Err(QuarantineError::IoError(kind, _)) => {
                assert_eq!(kind, std::io::ErrorKind::NotFound);
            }
            other => panic!("expected IoError(NotFound), got {other:?}"),
        }
    }

    /// A path pointing at a regular file (not a directory) fails
    /// capture — `open_dir`'s `O_DIRECTORY` surfaces `ENOTDIR`
    /// cleanly at capture time, not at first descend.
    #[test]
    fn root_capture_fails_on_regular_file_input() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("f.txt");
        std::fs::write(&file, b"x").unwrap();
        match Root::capture(&file) {
            Err(QuarantineError::IoError(_, _)) => {
                // ENOTDIR from `O_DIRECTORY` — the exact ErrorKind
                // (`NotADirectory`) isn't stable across Rust
                // versions, so just pin that it's an IoError.
            }
            other => panic!("expected IoError for file input, got {other:?}"),
        }
    }

    /// A broken symlink fails capture at `canonicalize` — the
    /// target doesn't exist, so `canonicalize` returns `NotFound`.
    #[test]
    fn root_capture_fails_on_broken_symlink() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("missing-target");
        let link = tmp.path().join("broken-link");
        symlink(&target, &link).unwrap();
        match Root::capture(&link) {
            Err(QuarantineError::IoError(kind, _)) => {
                assert_eq!(
                    kind,
                    std::io::ErrorKind::NotFound,
                    "canonicalize should report NotFound for broken symlink"
                );
            }
            other => panic!("expected IoError(NotFound) for broken symlink, got {other:?}"),
        }
    }

    /// Capturing the same path twice yields the same identity
    /// (barring an actual rename-and-recreate between the calls,
    /// which is what the H-5 defense catches at descend time).
    #[test]
    fn root_capture_of_same_path_yields_same_identity() {
        let tmp = TempDir::new().unwrap();
        let root_dir = tmp.path().join("r");
        fs::create_dir(&root_dir).unwrap();
        let a = Root::capture(&root_dir).unwrap();
        let b = Root::capture(&root_dir).unwrap();
        assert_eq!(a.identity(), b.identity());
        assert_eq!(a.path(), b.path());
    }

    /// The captured `(dev, inode)` pair has a nonzero `inode` on
    /// every supported filesystem — sanity check that we're
    /// actually reading real metadata, not a `zeroed` default.
    #[test]
    fn root_capture_produces_nonzero_inode() {
        let tmp = TempDir::new().unwrap();
        let root = Root::capture(tmp.path()).unwrap();
        let (_dev, ino) = root.identity();
        assert!(ino > 0, "inode should be nonzero on a real filesystem");
    }

    // --- RootIdentityRegistry -------------------------------------

    #[test]
    fn registry_is_empty_by_default() {
        let reg = RootIdentityRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn registry_register_and_get_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let root = Root::capture(tmp.path()).unwrap();
        let reg = RootIdentityRegistry::new();
        let logical = PathBuf::from("/@bundle/example");
        reg.register(logical.clone(), root.clone());

        let looked_up = reg.get(&logical).expect("registered logical resolves");
        assert_eq!(looked_up.path(), root.path());
        assert_eq!(looked_up.identity(), root.identity());
    }

    #[test]
    fn registry_get_returns_none_for_unregistered() {
        let reg = RootIdentityRegistry::new();
        assert!(reg.get(Path::new("/@bundle/nope")).is_none());
    }

    #[test]
    fn registry_len_reflects_registrations() {
        let tmp = TempDir::new().unwrap();
        let root = Root::capture(tmp.path()).unwrap();
        let reg = RootIdentityRegistry::new();
        assert_eq!(reg.len(), 0);
        reg.register(PathBuf::from("/@bundle/a"), root.clone());
        assert_eq!(reg.len(), 1);
        reg.register(PathBuf::from("/@bundle/b"), root);
        assert_eq!(reg.len(), 2);
    }

    /// Repeat register with the same logical key is idempotent:
    /// the latest write wins.  In practice boot registers once
    /// per logical root; overwrite semantics matter for tests
    /// building up a registry incrementally.
    #[test]
    fn registry_repeat_register_is_last_write_wins() {
        let tmp = TempDir::new().unwrap();
        let root_a = Root::capture(tmp.path()).unwrap();
        let sub = tmp.path().join("sub");
        fs::create_dir(&sub).unwrap();
        let root_b = Root::capture(&sub).unwrap();

        let reg = RootIdentityRegistry::new();
        let logical = PathBuf::from("/@bundle/x");
        reg.register(logical.clone(), root_a.clone());
        reg.register(logical.clone(), root_b.clone());
        assert_eq!(reg.len(), 1, "second register replaces, not appends");

        let got = reg.get(&logical).unwrap();
        assert_eq!(got.path(), root_b.path(), "last write wins");
    }

    /// `Clone` shares the backing map — a mutation via one handle
    /// is visible via the other.  Load-bearing for handing a
    /// registry to a reducer clone; both must see boot-time
    /// registrations.
    #[test]
    fn registry_clone_shares_backing_map() {
        let tmp = TempDir::new().unwrap();
        let root = Root::capture(tmp.path()).unwrap();

        let reg_a = RootIdentityRegistry::new();
        let reg_b = reg_a.clone();

        let logical = PathBuf::from("/@bundle/shared");
        reg_a.register(logical.clone(), root.clone());
        assert!(
            reg_b.get(&logical).is_some(),
            "clone must see registration made via original"
        );
    }
}
