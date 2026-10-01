// WAL fresh-tree applier — foundation types (slice 1 of the
// wal_applier submodule tree).
//
// This slice (PR #518 of the triage) ships the shared types the
// subsequent applier slices build on:
//
//   - [`ResolvedWalPath`] — the `(on_disk_root, rel_from_root,
//     expected_root_id)` triple the applier hands to
//     [`super::path::safe_descend_verified`] (PR #517 landed the
//     explicit-identity variant that lets WAL replay sites
//     thread this triple directly instead of wrapping each entry
//     in a `Root`).
//   - [`ApplierError`] — the full fault surface the dispatcher
//     and its helpers return.  13 variants cover byzantine input
//     (`MissingPayloadRef`, `PathContainsNull`,
//     `UnsupportedPayloadRef`), internal-invariant violations
//     (`MissingOffset`, `MissingModeBits`), out-of-tree writes
//     (`PathOutsideAllowedRoots`), NSS failures
//     (`NssResolutionFailed`, `NssNotFound`), I/O failures
//     (`IoFailure`, `ChownFailed`), payload-sidecar misses
//     (`MissingSidecarEntry`), and safe-descent failures
//     (`SafeDescendFailed`).
//
// Subsequent slices add the pure syscall helpers (`pwrite_all`,
// `copy_at`, `openat_leaf`, `check_path_allowed`,
// `format_quarantine`), the NSS lookup helpers (`resolve_uid`,
// `resolve_gid`), and finally the main
// [`apply_wal_to_fresh_tree`] dispatcher.
//
// # Why `ResolvedWalPath` and not `&Root` for replay sites
//
// Boot-path handlers already have a [`super::path::identity::Root`]
// on hand (constructed via `Root::capture` at boot and threaded
// via the per-runtime registry).  WAL replay resolves
// `(on_disk_root, rel, (dev, ino))` per entry from a boot-
// populated registry — the explicit triple lets the dispatcher
// thread raw path bytes + an optional identity straight through
// without wrapping each entry in a `Root`.  Test harnesses with
// operator-frozen tempdirs can pass `expected_root_id = None`
// to skip the H-5 check.  Production replay sites MUST pass
// `Some((dev, ino))` — `None` silently drops the rename-and-
// recreate defense (see PR #517's `safe_descend_verified`
// docstring).
//
// # TOCTOU discipline (same as handlers)
//
// Every mutation runs as a `*at` syscall against the dirfd
// returned by `safe_descend_verified`.  A component swap
// between descent and syscall cannot escape the on-disk root
// — the same discipline the leader-side handlers use for
// symlink-swap-immunity.
//
// # Path validation (defense-in-depth)
//
// The dispatcher accepts `allowed_roots: &[PathBuf]` — if
// non-empty, every WAL entry's `path` (and `extra_path` for
// Rename/CopyFile) must be under one of those roots or the
// dispatcher returns [`ApplierError::PathOutsideAllowedRoots`]
// without touching disk.  Pass `&[]` to skip validation (tests
// or production sites with no provisioning plumbing).  This
// check is in a yet-to-land slice; the error variant lives here
// so the type surface is complete.
//
// # Sidecar authentication model
//
// `payload_bytes` carries Write/WriteAt bodies keyed by their
// Blake2b256 hash.  Production joiners verify
// `Blake2b256(fetched_bytes) == entry.payload_ref` before
// installing them in `payload_bytes` — Blake2b256 pre-image
// resistance makes serving alternative bytes with the same hash
// cryptographically infeasible.  The sidecar bytes are NOT
// independently signed (unlike manifest entries post-H-4):
// security = leader trust + hash check + TLS transport hygiene.
//
// If sidecar transport ever moves to a shared cache (Redis, S3,
// pubsub fan-out) or a peer-to-peer redistribution overlay, the
// current "hash-check-only" discipline needs a signature
// binding (leader signs `(payload_ref, deploy_scope)`; joiners
// verify the signature before installing).  Rationale: shared
// caches weaken the "malicious peer can only serve bytes with
// the correct hash" guarantee.  Any such refactor MUST cite
// this note.

use std::path::{Path, PathBuf};

use super::wal::WalOp;

/// Result of decomposing a WAL entry's absolute path into the
/// `(on_disk_root, rel_from_root, expected_root_id)` triple the
/// applier hands to [`super::path::safe_descend_verified`].
///
/// Production callers construct this via (yet-to-land)
/// `RootIdentityRegistry::resolve_wal_entry_root_rel`, which
/// consults the boot-populated registry for the on-disk root and
/// identity.  Test callers construct it directly from tempdir
/// roots + relative subpaths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedWalPath {
    pub root: PathBuf,
    pub rel: PathBuf,
    pub expected_root_id: Option<(u64, u64)>,
}

impl ResolvedWalPath {
    /// Convenience for tests: `<parent>/<file_name>` split with
    /// no identity check.  Callers that already know the tempdir
    /// root + relative filename should construct the struct
    /// directly (this fallback only works for depth-1 paths).
    pub fn identity_leaf_split(p: &Path) -> Self {
        ResolvedWalPath {
            root: p
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("/")),
            rel: p.file_name().map(PathBuf::from).unwrap_or_default(),
            expected_root_id: None,
        }
    }
}

/// Every failure mode the applier can surface.  Callers pattern-
/// match to distinguish "byzantine input" (log + skip) from
/// "internal invariant violation" (surface to operator + halt).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplierError {
    /// The WAL entry's `payload_ref` hash is not present in the
    /// sidecar.  In production this indicates the joiner-side
    /// fetch driver returned `is_complete()` but a race dropped
    /// bytes between there and here; in tests, the driver
    /// mis-populated the sidecar.
    MissingSidecarEntry {
        entry_index: usize,
        hash_hex: String,
    },
    /// The WAL entry carries `PayloadRef::DeployRef { ... }`
    /// which the applier cannot yet reconstruct locally.
    /// Reserved for a future reducer slice that resolves deploy
    /// refs from on-chain deploy data.
    UnsupportedPayloadRef { entry_index: usize },
    /// The WAL entry is a write op but `payload_ref` is `None`.
    /// Invariant violation: the leader's `journal_write` must
    /// populate this field.
    MissingPayloadRef { entry_index: usize, op: WalOp },
    /// A Write/WriteAt/Truncate entry is missing its `offset`.
    /// Invariant violation.
    MissingOffset { entry_index: usize, op: WalOp },
    /// A Chmod entry is missing `mode_bits`.
    MissingModeBits { entry_index: usize },
    /// A Chown entry is missing its `owner` field.
    MissingOwner { entry_index: usize },
    /// A Rename/CopyFile entry is missing `extra_path`.
    MissingExtraPath { entry_index: usize, op: WalOp },
    /// The WAL entry's path contains a NULL byte, which the
    /// `safe_descend_verified` layer catches at `to_c` before any
    /// syscall.  Retained for callers that may synthesize path-
    /// level pre-checks; the primary path routes NULL through
    /// `SafeDescendFailed`.
    PathContainsNull { entry_index: usize },
    /// The WAL entry's path is not under any of the caller-
    /// supplied `allowed_roots`.  Defense-in-depth: blocks a
    /// hypothetical leader bug (or forged snapshot) that would
    /// otherwise write outside the joiner's consensus-static
    /// roots.  Never reachable when `allowed_roots` is empty.
    PathOutsideAllowedRoots { entry_index: usize, path: PathBuf },
    /// `getpwnam_r` / `getgrnam_r` returned a non-zero errno
    /// (not ERANGE — that is retried internally with a bigger
    /// buffer).  Almost always indicates a system-level NSS
    /// problem rather than a WAL bug.
    NssResolutionFailed { name: String, errno: i32 },
    /// `getpwnam_r` / `getgrnam_r` returned success but the
    /// result pointer is NULL — i.e., the name resolved to no
    /// entry.  Operator responsibility to keep NSS consistent
    /// across validators.
    NssNotFound { name: String },
    /// A `std::fs` op or a libc syscall returned an error.
    IoFailure {
        entry_index: usize,
        op: WalOp,
        path: PathBuf,
        message: String,
    },
    /// Chown's `libc::fchownat` returned a non-zero rc that is
    /// not EPERM (EPERM is treated as a no-op success for
    /// unprivileged hosts — see the Chown branch's comment, in
    /// a yet-to-land slice).
    ChownFailed {
        entry_index: usize,
        path: PathBuf,
        errno: i32,
    },
    /// `safe_descend_verified` failed for this entry's
    /// (on-disk-root, rel) — e.g., a symlink component was
    /// found, the root's boot-captured identity no longer
    /// matches, or the rel escaped the root.  The applier
    /// surfaces descent failures explicitly rather than papering
    /// over them with a downstream open error.
    SafeDescendFailed {
        entry_index: usize,
        root: PathBuf,
        rel: PathBuf,
        reason: String,
    },
}

impl std::fmt::Display for ApplierError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApplierError::MissingSidecarEntry {
                entry_index,
                hash_hex,
            } => write!(
                f,
                "WAL entry {entry_index}: hash {hash_hex} missing from payload sidecar"
            ),
            ApplierError::UnsupportedPayloadRef { entry_index } => write!(
                f,
                "WAL entry {entry_index}: DeployRef payload_ref not yet supported"
            ),
            ApplierError::MissingPayloadRef { entry_index, op } => {
                write!(f, "WAL entry {entry_index}: {op:?} without payload_ref")
            }
            ApplierError::MissingOffset { entry_index, op } => {
                write!(f, "WAL entry {entry_index}: {op:?} without offset")
            }
            ApplierError::MissingModeBits { entry_index } => {
                write!(f, "WAL entry {entry_index}: Chmod without mode_bits")
            }
            ApplierError::MissingOwner { entry_index } => {
                write!(f, "WAL entry {entry_index}: Chown without owner")
            }
            ApplierError::MissingExtraPath { entry_index, op } => {
                write!(f, "WAL entry {entry_index}: {op:?} without extra_path")
            }
            ApplierError::PathContainsNull { entry_index } => {
                write!(f, "WAL entry {entry_index}: path contains a NULL byte")
            }
            ApplierError::PathOutsideAllowedRoots { entry_index, path } => write!(
                f,
                "WAL entry {entry_index}: path {path:?} is not under any allowed root"
            ),
            ApplierError::NssResolutionFailed { name, errno } => {
                write!(f, "NSS lookup for {name:?} failed with errno {errno}")
            }
            ApplierError::NssNotFound { name } => {
                write!(f, "NSS lookup for {name:?} returned no entry")
            }
            ApplierError::IoFailure {
                entry_index,
                op,
                path,
                message,
            } => write!(
                f,
                "WAL entry {entry_index}: {op:?} at {path:?} failed: {message}"
            ),
            ApplierError::ChownFailed {
                entry_index,
                path,
                errno,
            } => write!(
                f,
                "WAL entry {entry_index}: chown {path:?} failed with errno {errno}"
            ),
            ApplierError::SafeDescendFailed {
                entry_index,
                root,
                rel,
                reason,
            } => write!(
                f,
                "WAL entry {entry_index}: safe_descend {root:?} / {rel:?} failed: {reason}"
            ),
        }
    }
}

impl std::error::Error for ApplierError {}

// Compile-time witness that `ApplierError: Send + Sync` — required
// for propagation across `tokio::spawn_blocking` boundaries in the
// (yet-to-land) joiner-side subscriber task.  Hoisted to module
// scope so every `cargo build` catches a regression, not only
// `cargo test`.  Same pattern as `SnapshotError`'s witness.
const _APPLIER_ERROR_IS_SEND_SYNC: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ApplierError>();
};

#[cfg(test)]
mod tests {
    use super::super::wal::WalOp;
    use super::*;

    #[test]
    fn resolved_wal_path_identity_leaf_split_depth_one_path() {
        let p = Path::new("/tmp/leaf.txt");
        let r = ResolvedWalPath::identity_leaf_split(p);
        assert_eq!(r.root, PathBuf::from("/tmp"));
        assert_eq!(r.rel, PathBuf::from("leaf.txt"));
        assert_eq!(
            r.expected_root_id, None,
            "test-helper always skips identity"
        );
    }

    /// Edge case: a path with no parent (e.g., root-level) falls
    /// back to `"/"` for root — matches fileio's convenience
    /// semantics.  Depth-1 paths with no filename component
    /// aren't a realistic WAL input but keep the helper total.
    #[test]
    fn resolved_wal_path_identity_leaf_split_root_level_falls_back() {
        let p = Path::new("/");
        let r = ResolvedWalPath::identity_leaf_split(p);
        assert_eq!(r.root, PathBuf::from("/"));
        assert_eq!(r.rel, PathBuf::new());
        assert_eq!(r.expected_root_id, None);
    }

    /// `ResolvedWalPath: Clone + PartialEq + Eq` — pinned via a
    /// trivial roundtrip so a derive-removal regression surfaces.
    #[test]
    fn resolved_wal_path_derived_traits() {
        let a = ResolvedWalPath {
            root: PathBuf::from("/root"),
            rel: PathBuf::from("rel"),
            expected_root_id: Some((1, 2)),
        };
        let b = a.clone();
        assert_eq!(a, b);
    }

    /// LOAD-BEARING: every `ApplierError` variant has a `Display`
    /// impl that renders operator-useful content.  Rather than
    /// enumerate 13 separate tests, pin one representative per
    /// shape and let the match-exhaustiveness check catch new
    /// variants at compile time (adding a variant without
    /// extending the Display match fails the build).
    #[test]
    fn applier_error_display_covers_every_variant() {
        let cases: Vec<ApplierError> = vec![
            ApplierError::MissingSidecarEntry {
                entry_index: 7,
                hash_hex: "deadbeef".into(),
            },
            ApplierError::UnsupportedPayloadRef { entry_index: 1 },
            ApplierError::MissingPayloadRef {
                entry_index: 2,
                op: WalOp::Write,
            },
            ApplierError::MissingOffset {
                entry_index: 3,
                op: WalOp::Truncate,
            },
            ApplierError::MissingModeBits { entry_index: 4 },
            ApplierError::MissingOwner { entry_index: 5 },
            ApplierError::MissingExtraPath {
                entry_index: 6,
                op: WalOp::Rename,
            },
            ApplierError::PathContainsNull { entry_index: 8 },
            ApplierError::PathOutsideAllowedRoots {
                entry_index: 9,
                path: PathBuf::from("/etc/passwd"),
            },
            ApplierError::NssResolutionFailed {
                name: "alice".into(),
                errno: 11,
            },
            ApplierError::NssNotFound {
                name: "ghost".into(),
            },
            ApplierError::IoFailure {
                entry_index: 10,
                op: WalOp::Write,
                path: PathBuf::from("/tmp/x"),
                message: "no space left on device".into(),
            },
            ApplierError::ChownFailed {
                entry_index: 11,
                path: PathBuf::from("/tmp/y"),
                errno: 1,
            },
            ApplierError::SafeDescendFailed {
                entry_index: 12,
                root: PathBuf::from("/root"),
                rel: PathBuf::from("deep/path"),
                reason: "symlink component".into(),
            },
        ];
        for case in &cases {
            let s = format!("{case}");
            assert!(!s.is_empty(), "Display produced empty string for {case:?}");
            assert!(
                !s.contains("<unknown>"),
                "Display produced fallback-looking text for {case:?}: {s}"
            );
        }
    }

    /// Spot-check that specific variants surface the identifying
    /// field in their Display output.  Catches a lazy
    /// `write!(f, "applier error")` refactor that would still
    /// pass the "not empty" check above.
    #[test]
    fn applier_error_display_includes_identifying_field() {
        let s = format!("{}", ApplierError::MissingSidecarEntry {
            entry_index: 42,
            hash_hex: "cafebabe".into(),
        });
        assert!(s.contains("42"), "entry_index embedded: {s}");
        assert!(s.contains("cafebabe"), "hash_hex embedded: {s}");

        let s = format!("{}", ApplierError::PathOutsideAllowedRoots {
            entry_index: 1,
            path: PathBuf::from("/etc/passwd"),
        });
        assert!(s.contains("/etc/passwd"), "path embedded: {s}");

        let s = format!("{}", ApplierError::NssNotFound {
            name: "unicorn".into(),
        });
        assert!(s.contains("unicorn"), "nss name embedded: {s}");
    }

    /// `ApplierError: std::error::Error` — pinned so a future
    /// refactor that forgot the `impl Error` block (e.g., when
    /// adding variants and typing `impl Display` without the
    /// matching Error) fails this test.
    #[test]
    fn applier_error_implements_std_error_trait() {
        fn assert_error<T: std::error::Error>() {}
        assert_error::<ApplierError>();
    }
}
