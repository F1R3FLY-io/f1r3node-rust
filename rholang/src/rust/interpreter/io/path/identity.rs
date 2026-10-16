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
use crate::rust::interpreter::io::errors::poison_abort;

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
/// Later slices that register logical→on-disk mappings (Shape-A
/// `register_with_remap`) normalize upstream; direct users of
/// `register` / `get` are on the hook for consistency.
///
/// # Thread safety
///
/// Thread-safe.  Reads (frequent, per-syscall) don't contend;
/// writes (rare, boot-time only) take a write lock briefly.
/// Every guard acquisition routes through `poison_abort` per
/// DD-FailClosedOnInvariantBreak.
///
/// # Two-layer indirection (`Arc<RwLock<Arc<RwLock<Inner>>>>`)
///
/// The nested-Arc layout is load-bearing for the
/// `RuntimeManager` broadcast pattern.  `Clone` copies the OUTER
/// `Arc<RwLock<_>>` slot; [`share_from`](Self::share_from)
/// atomically swaps the INNER `Arc<RwLock<Inner>>` inside the
/// slot so:
///
///   1. **Reducer-clone visibility**: `create_rho_runtime` clones
///      the enclosing `FileHandleTable` (which owns a registry
///      clone) to hand a copy to the reducer BEFORE the boot
///      pipeline installs the manager-shared registry via
///      `share_from`.  Both clones share the OUTER slot, so
///      swapping the inner Arc is visible through both.  A
///      field-replacement design would only update the runtime's
///      outer field — the reducer's earlier clone would keep
///      its own disjoint inner and never see boot-time
///      registrations.  Handler paths through the reducer's
///      clone would resolve against an empty backing → every
///      Shape A `/@bundle/...` `canonRoot` would fall through
///      to `NotFound` at open time.
///   2. **Late-registration propagation**: after `share_from`,
///      ALL handles (manager, runtime, reducer's clone, any
///      future clone) point at the SAME inner Arc.  A subsequent
///      `register` on the manager writes to the inner Arc's map
///      — visible everywhere.
#[derive(Debug, Default, Clone)]
pub struct RootIdentityRegistry {
    slot: Arc<RwLock<Arc<RwLock<Inner>>>>,
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
        let backing = self.current_backing();
        let mut guard = poison_abort(backing.write(), "RootIdentityRegistry.inner");
        guard.entries.insert(logical, root);
    }

    /// Look up the [`Root`] for a logical path.  Returns `None`
    /// for unregistered logicals.
    pub fn get(&self, logical: &Path) -> Option<Root> {
        let backing = self.current_backing();
        let guard = poison_abort(backing.read(), "RootIdentityRegistry.inner");
        guard.entries.get(logical).cloned()
    }

    /// Handler-pattern convenience: look up `logical` and return
    /// `(on_disk_root, expected_root_id)` such that the handler
    /// can pass `on_disk_root` to [`super::descend::safe_descend_verified`]
    /// and `expected_root_id` as its identity argument.  Falls
    /// through to `(logical.to_path_buf(), None)` for unregistered
    /// logical roots — matches the pre-Shape-A behavior where the
    /// handler treated the caller-supplied path as the on-disk
    /// path directly and skipped the H-5 identity check.
    ///
    /// # Shape-A gating (yet to land)
    ///
    /// This ungated variant is safe for Oracular callers (where
    /// the test harness or an off-ledger integration constructs a
    /// cap over an arbitrary host path).  Consensus callers under
    /// Shape A must reject unregistered logicals — a `None` return
    /// then means the boot registration failed or the cap is
    /// misconfigured.  A gated `resolve_or_identity_gated_for_consensus`
    /// variant will land with the handler slice that first needs
    /// Consensus-cap gating (observation / mutation families).
    pub fn resolve_or_identity(&self, logical: &Path) -> (PathBuf, Option<(u64, u64)>) {
        match self.get(logical) {
            Some(r) => (r.path().to_path_buf(), Some(r.identity())),
            None => (logical.to_path_buf(), None),
        }
    }

    /// Count of registered roots.  Diagnostics only.
    pub fn len(&self) -> usize {
        let backing = self.current_backing();
        let guard = poison_abort(backing.read(), "RootIdentityRegistry.inner");
        guard.entries.len()
    }

    pub fn is_empty(&self) -> bool { self.len() == 0 }

    /// Atomically point `self`'s backing at `other`'s backing so
    /// subsequent reads and writes through `self` (and every
    /// clone that shares `self`'s outer slot Arc) route through
    /// `other`'s inner map.
    ///
    /// See the struct-level "Two-layer indirection" section for
    /// the load-bearing rationale (reducer-clone visibility +
    /// late-registration propagation).
    ///
    /// # Compact behavior
    ///
    /// A subsequent `register` on EITHER `self` or `other` writes
    /// to the shared inner map — after `share_from`, they are
    /// semantically the same registry backing.  Calling
    /// `share_from` a second time with a DIFFERENT `other`
    /// atomically swaps to the new inner (the prior inner is
    /// dropped when its last owning Arc goes away).
    ///
    /// # Serialize with `register` at the boot pipeline
    ///
    /// `share_from` and a concurrent `register` on affected
    /// handles produce a lost-update on the losing side:
    /// `register` snapshots the current inner Arc via
    /// `current_backing`, `share_from` swaps the slot to a
    /// different inner, `register`'s write lands in the ORPHANED
    /// inner (dropped once its refcount reaches zero), and a
    /// subsequent `get` reads the NEW inner and misses the
    /// registration.
    ///
    /// The boot pipeline MUST serialize the two: register on the
    /// manager, then broadcast via `share_from`, THEN begin per-
    /// syscall reads through the shared handles.  All production
    /// call sites do this by construction (boot is a single-
    /// threaded phase); the caveat is here for anyone writing
    /// tests or a future refactor that reorders the boot phases.
    /// Structural enforcement (widening `register`'s lock to
    /// contend with `share_from`) is possible but would slow the
    /// hot path for a race that can't fire under intended usage.
    pub fn share_from(&self, other: &RootIdentityRegistry) {
        let src = poison_abort(other.slot.read(), "RootIdentityRegistry.slot").clone();
        *poison_abort(self.slot.write(), "RootIdentityRegistry.slot") = src;
    }

    /// Snapshot the current backing (`Arc<RwLock<Inner>>`) —
    /// after `share_from`, this reflects the shared inner.  The
    /// returned Arc lets the caller take a further guard without
    /// re-touching the outer slot on every access.
    fn current_backing(&self) -> Arc<RwLock<Inner>> {
        poison_abort(self.slot.read(), "RootIdentityRegistry.slot").clone()
    }
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

    // --- share_from --------------------------------------------------

    /// `share_from` makes the shared registry's registrations
    /// visible on the receiver.  Basic sanity: register on A,
    /// share B ← A, verify B.get sees the registration.
    #[test]
    fn share_from_exposes_shared_registrations_on_receiver() {
        let tmp = TempDir::new().unwrap();
        let root = Root::capture(tmp.path()).unwrap();

        let manager = RootIdentityRegistry::new();
        let logical = PathBuf::from("/@bundle/shared");
        manager.register(logical.clone(), root.clone());

        let runtime = RootIdentityRegistry::new();
        assert!(
            runtime.get(&logical).is_none(),
            "pre-share: runtime is empty"
        );

        runtime.share_from(&manager);
        assert!(
            runtime.get(&logical).is_some(),
            "post-share: runtime sees the manager's registration"
        );
    }

    /// The load-bearing test: `share_from` propagates LATE
    /// registrations to prior-cloned handles.  This is the exact
    /// PB-M-14 canary regression the two-layer indirection fixes.
    ///
    /// Setup mirrors the production sequence:
    ///   1. `manager` created.
    ///   2. `reducer_clone` cloned FROM the runtime's post-share
    ///      handle (which shares the manager's inner via
    ///      `share_from`).
    ///   3. Manager registers AFTER share_from.
    ///   4. reducer_clone MUST see the registration.
    ///
    /// A pre-fix single-layer design would have every
    /// `.clone()` produce a disjoint inner map — the reducer
    /// clone captured at step 2 would never see the step-3
    /// registration.
    #[test]
    fn share_from_late_registration_propagates_to_prior_clones() {
        let tmp = TempDir::new().unwrap();
        let root = Root::capture(tmp.path()).unwrap();

        // Step 1: manager is created (empty).
        let manager = RootIdentityRegistry::new();

        // Step 2: a runtime is created and given the manager's
        // backing via share_from.  A downstream consumer (the
        // "reducer") captures a clone of the runtime's handle.
        let runtime = RootIdentityRegistry::new();
        runtime.share_from(&manager);
        let reducer_clone = runtime.clone();

        // Step 3: the manager registers a Root (this happens
        // AFTER the reducer_clone was captured).
        let logical = PathBuf::from("/@bundle/late");
        manager.register(logical.clone(), root.clone());

        // Step 4: the reducer_clone MUST see the manager's
        // late registration.  A single-layer design would fail
        // this — reducer_clone would hold a captured-empty
        // inner.
        assert!(
            reducer_clone.get(&logical).is_some(),
            "late registration on the manager MUST propagate to \
             a reducer clone captured after share_from"
        );
    }

    /// After `share_from`, writes to EITHER side land in the
    /// same shared backing.  Pins the "share_from unifies both
    /// endpoints" semantic (not just a one-way copy).
    #[test]
    fn share_from_is_bidirectional_after_swap() {
        let tmp = TempDir::new().unwrap();
        let root = Root::capture(tmp.path()).unwrap();

        let a = RootIdentityRegistry::new();
        let b = RootIdentityRegistry::new();
        b.share_from(&a);

        // Register through `b` (the receiver).
        let logical_b = PathBuf::from("/@bundle/from-b");
        b.register(logical_b.clone(), root.clone());
        assert!(a.get(&logical_b).is_some(), "a must see b's registration");

        // Register through `a` (the source).
        let logical_a = PathBuf::from("/@bundle/from-a");
        a.register(logical_a.clone(), root.clone());
        assert!(b.get(&logical_a).is_some(), "b must see a's registration");
    }

    /// A second `share_from` with a DIFFERENT source atomically
    /// swaps the backing — subsequent reads see the new source's
    /// entries, not the old.
    #[test]
    fn share_from_replaces_prior_backing() {
        let tmp = TempDir::new().unwrap();
        let root = Root::capture(tmp.path()).unwrap();

        let source_a = RootIdentityRegistry::new();
        source_a.register(PathBuf::from("/@bundle/a-key"), root.clone());

        let source_b = RootIdentityRegistry::new();
        source_b.register(PathBuf::from("/@bundle/b-key"), root.clone());

        let receiver = RootIdentityRegistry::new();
        receiver.share_from(&source_a);
        assert!(receiver.get(Path::new("/@bundle/a-key")).is_some());
        assert!(receiver.get(Path::new("/@bundle/b-key")).is_none());

        // Swap the backing.
        receiver.share_from(&source_b);
        assert!(
            receiver.get(Path::new("/@bundle/a-key")).is_none(),
            "post-swap: prior backing's entries are gone"
        );
        assert!(receiver.get(Path::new("/@bundle/b-key")).is_some());
    }

    /// `share_from(&self)` on the same registry is a no-op (self-
    /// share).  Pins that a defensive `handles.share_root_registry
    /// (handles.root_registry())` at the boot pipeline doesn't
    /// accidentally destroy state.
    #[test]
    fn share_from_self_is_noop() {
        let tmp = TempDir::new().unwrap();
        let root = Root::capture(tmp.path()).unwrap();
        let reg = RootIdentityRegistry::new();
        let logical = PathBuf::from("/@bundle/self-share");
        reg.register(logical.clone(), root);

        reg.share_from(&reg);
        assert!(reg.get(&logical).is_some());
    }
}
