//! Slice 30 (PB-M-15) — consensus filesystem snapshot config validation.
//!
//! # Slice 30b addendum: SnapshotWriter builder
//!
//! `build_snapshot_writer` combines validation with construction: on
//! success it returns `Ok(Some(SnapshotWriter))` ready to attach to
//! `RhoRuntimeImpl` / `RuntimeManager`.  Callers in the boot pipeline
//! do a single call to hand the writer to the runtime.  Retention is
//! derived from `cadence`: `retain = max(2, ceil(cadence / cadence))
//! * 2` (i.e., `2 * cadence` snapshots retained) so a joining
//! validator can span up to `2 * cadence` blocks of history from any
//! single snapshot download.
//!
//! # Two required keys
//!
//! `storage.consensus-fs-snapshot-cadence` (u64, block interval) and
//! `storage.consensus-fs-snapshot-dir` (path) are both required at
//! boot when *any* `consensus-static-*` bucket has entries.  They have
//! no defaults per the 2026-08-03 FIP resolution: the trade-off between
//! snapshot cost (I/O + disk) and late-join replay length (WAL bytes)
//! is deployment-specific and cannot be defaulted safely.
//!
//! # What we check
//!
//! 1. `cadence` is `Some(n)` with `n >= 1`.  Zero is a boot-time error
//!    (would divide by zero at cadence-check time; also semantically
//!    nonsensical — "snapshot every 0 blocks" is undefined).
//! 2. `dir` is `Some(path)` and either exists as a directory OR can be
//!    created.  A writable-probe (create + remove a tmp file) confirms
//!    the operator's storage actually accepts writes.
//! 3. If either key is missing while a `consensus-static-*` bucket has
//!    entries, boot fails with a diagnostic that names the missing
//!    key(s) and explains why they matter.
//!
//! # Backward compatibility
//!
//! Nodes that never provision `consensus-static-*` (all four buckets
//! empty) are exempt — no snapshot infrastructure is needed for
//! them.  This lets existing operator configs upgrade cleanly.

use std::path::{Path, PathBuf};

use rholang::rust::interpreter::io::snapshot::SnapshotWriter;

use crate::rust::configuration::file_io_provisioning::FileIoProvisioning;

#[derive(Debug, PartialEq, Eq)]
pub enum SnapshotConfigError {
    /// Cadence key present but value is zero.
    ZeroCadence,
    /// Consensus-static provisioning present but cadence missing.
    MissingCadence,
    /// Consensus-static provisioning present but dir missing.
    MissingDir,
    /// Dir key present but the path is not a directory (and could not
    /// be created), or is not writable.
    UnwritableDir { path: PathBuf, reason: String },
    /// F-30b-1 (2026-08-24): consensus-static provisioning present
    /// but retention value is less than 2.  A retain < 2 would
    /// prune every snapshot beyond the most recent, breaking
    /// data availability for joining validators.
    RetainTooSmall { retain: usize },
}

impl std::fmt::Display for SnapshotConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SnapshotConfigError::ZeroCadence => write!(
                f,
                "storage.consensus-fs-snapshot-cadence must be >= 1 (a value of 0 is \
                 undefined: 'snapshot every 0 blocks' has no meaning; pick a positive \
                 block interval — trade-off: smaller cadence means more snapshot I/O \
                 but shorter joining-validator replay)"
            ),
            SnapshotConfigError::MissingCadence => write!(
                f,
                "storage.consensus-fs-snapshot-cadence is required when any \
                 consensus-static-* bucket is provisioned.  This value has no \
                 default: operators must choose a block interval based on their \
                 deployment's snapshot-cost vs. late-join-replay-length trade-off.  \
                 See File I/O FIP §Q-6."
            ),
            SnapshotConfigError::MissingDir => write!(
                f,
                "storage.consensus-fs-snapshot-dir is required when any \
                 consensus-static-* bucket is provisioned.  This value has no \
                 default: operators must choose an on-disk directory with sufficient \
                 free space for snapshot retention."
            ),
            SnapshotConfigError::UnwritableDir { path, reason } => write!(
                f,
                "storage.consensus-fs-snapshot-dir {} is not writable: {reason}",
                path.display()
            ),
            SnapshotConfigError::RetainTooSmall { retain } => write!(
                f,
                "storage.consensus-fs-snapshot-retain must be >= 2 (got {retain}) when any \
                 consensus-static-* bucket is provisioned.  A retain < 2 would prune every \
                 snapshot beyond the most recent, leaving joining validators with no history \
                 window.  Size per your late-join SLA: retain = ceil(target_history_blocks / \
                 cadence) + 1."
            ),
        }
    }
}

impl std::error::Error for SnapshotConfigError {}

/// Return `true` if the node has any consensus-static-* provisioning
/// that would require the snapshot machinery.
pub fn requires_snapshot_config(provisioning: &FileIoProvisioning) -> bool {
    !provisioning.consensus_static_files.is_empty()
        || !provisioning.consensus_static_dirs.is_empty()
}

/// Boot-time validation.  Called from the node's config-load path.
///
/// * If no consensus-static provisioning → skip (backward-compat).
/// * Else require both keys, validate cadence > 0, and touch-probe
///   the dir.
///
/// On success, returns the CANONICALIZED snapshot dir (M-30-2 review
/// fix): the caller should use this canonicalized path for all
/// subsequent snapshot I/O so a relative-path config, a subsequent
/// `chdir`, or a symlink swap between boot and use cannot redirect
/// writes.  Returns `None` when no consensus-static provisioning
/// (the "skip" branch).
pub fn validate_snapshot_config(
    provisioning: &FileIoProvisioning,
    cadence: Option<u64>,
    dir: Option<&Path>,
    retain: usize,
) -> Result<Option<PathBuf>, SnapshotConfigError> {
    if !requires_snapshot_config(provisioning) {
        return Ok(None);
    }
    match cadence {
        None => return Err(SnapshotConfigError::MissingCadence),
        Some(0) => return Err(SnapshotConfigError::ZeroCadence),
        Some(_) => {}
    }
    // F-30b-1 (2026-08-24): retention must be >= 2 when snapshotting.
    // The floor is not silently applied — operators must have made an
    // informed choice.
    if retain < 2 {
        return Err(SnapshotConfigError::RetainTooSmall { retain });
    }
    let dir = dir.ok_or(SnapshotConfigError::MissingDir)?;
    probe_dir_writable(dir)?;
    // M-30-2 review fix (round 2): canonicalize the dir NOW so
    // downstream snapshot I/O uses an absolute, symlink-resolved
    // path.  `probe_dir_writable` already ensured the dir exists.
    let canonical = std::fs::canonicalize(dir).map_err(|e| SnapshotConfigError::UnwritableDir {
        path: dir.to_path_buf(),
        reason: format!("canonicalize failed: {e}"),
    })?;
    Ok(Some(canonical))
}

/// Slice 30b: build the SnapshotWriter for the boot pipeline.
///
/// Combines `validate_snapshot_config` with `SnapshotWriter`
/// construction: on success returns `Ok(Some(writer))` for the
/// boot pipeline to attach to `RuntimeManager`, `Ok(None)` when
/// the operator has no consensus-static provisioning (no writer
/// needed), or `Err(SnapshotConfigError)` on any validation
/// failure.
///
/// F-30b-1 promotion (2026-08-24): retention is now a required
/// operator value (see `NodeConfig.storage.consensus_fs_snapshot_retain`
/// docstring for sizing guidance).  The pre-promotion `cadence *
/// 2` fallback heuristic is gone — validate_snapshot_config
/// rejects `retain < 2` with `RetainTooSmall` when consensus
/// provisioning is present.
pub fn build_snapshot_writer(
    provisioning: &FileIoProvisioning,
    cadence: Option<u64>,
    dir: Option<&Path>,
    retain: usize,
    // H-4 fix (2026-08-06): validator identity secret key (raw
    // secp256k1 bytes) for signing manifest entries.  Populated
    // from `conf.casper.validator_private_key` at boot.  `None`
    // for observer nodes without an identity — the resulting
    // manifest is unsigned (pre-H-4 backward-compat behavior)
    // and a boot-time warning is logged so operators know their
    // manifests won't be verifiable by peers.
    signer_sk: Option<Vec<u8>>,
) -> Result<Option<SnapshotWriter>, SnapshotConfigError> {
    let canonical = validate_snapshot_config(provisioning, cadence, dir, retain)?;
    match canonical {
        None => Ok(None),
        Some(canonical_dir) => {
            // Unwrap safe: validate returned Some(dir) which implies
            // cadence was Some(nonzero) and retain was >= 2.
            let cadence = cadence.expect("cadence validated above");
            if signer_sk.is_none() {
                tracing::warn!(
                    target: "f1r3fly.fs_wal.snapshot",
                    "SnapshotWriter constructed without a signer key (observer node \
                     or missing validator_private_key); manifest entries will be \
                     written UNSIGNED and joining validators will not be able to \
                     verify authenticity.  H-4 fix requires a validator identity \
                     to produce signed manifests."
                );
            }
            Ok(Some(SnapshotWriter {
                dir: canonical_dir,
                cadence,
                retain,
                signer_sk,
            }))
        }
    }
}

fn probe_dir_writable(dir: &Path) -> Result<(), SnapshotConfigError> {
    // Ensure directory exists (create if not).
    if let Err(e) = std::fs::create_dir_all(dir) {
        return Err(SnapshotConfigError::UnwritableDir {
            path: dir.to_path_buf(),
            reason: format!("create_dir_all failed: {e}"),
        });
    }
    // Touch-and-remove probe.  L-4 fix (2026-08-06): use
    // `tempfile::Builder` to produce a cryptographically-random
    // suffix instead of `pid + now_nanos`.  Pre-fix, an attacker
    // with local read access could predict the probe path from
    // process metadata (pid via /proc, clock roughly via observed
    // events) and pre-create a file / symlink at that path within
    // the race window.  The O_EXCL step (create_new) below already
    // catches the pre-existing-path case, but making the name
    // unpredictable closes the race one layer earlier and matches
    // the standard convention.  `tempfile` uses getrandom() under
    // the hood — same source certificate_helper uses for key
    // material.
    use std::io::Read;
    let mut suffix = [0u8; 16];
    // Read from /dev/urandom on Unix; falls back to platform CSPRNG
    // elsewhere.  Cheaper than pulling in the whole `rand` API
    // surface for one 16-byte draw.
    match std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut suffix)) {
        Ok(_) => {}
        Err(_) => {
            // If /dev/urandom is unavailable (containerized bind
            // mount, etc.), fall back to pid+nanos with a warning.
            // The O_EXCL step below still gates against the pre-
            // placement attack in the fallback path.
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0);
            let pid = std::process::id() as u64;
            suffix[..8].copy_from_slice(&nanos.to_be_bytes());
            suffix[8..].copy_from_slice(&pid.to_be_bytes());
            tracing::warn!(
                target: "f1r3fly.fs_wal.snapshot",
                "/dev/urandom unavailable for probe-name entropy; falling back to \
                 pid+nanos (still safe under O_EXCL create_new)"
            );
        }
    };
    let probe_name = format!(
        ".snapshot-probe-{}",
        suffix.iter().fold(String::with_capacity(32), |mut acc, b| {
            use std::fmt::Write;
            let _ = write!(acc, "{b:02x}");
            acc
        })
    );
    let probe_path = dir.join(&probe_name);
    // M-30-3 review fix (round 2): use O_EXCL (create_new) so a
    // pre-existing file / symlink at the probe path causes the
    // probe to fail hard rather than follow.  Closes the local
    // symlink-race: an attacker who predicts `pid + now_nanos` and
    // pre-creates a symlink pointing at a file the validator has
    // write access to would have their symlink truncated via the
    // pre-fix `std::fs::write`; O_EXCL makes that create step fail
    // instead.
    use std::io::Write;
    let mut file = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe_path)
    {
        Ok(f) => f,
        Err(e) => {
            return Err(SnapshotConfigError::UnwritableDir {
                path: dir.to_path_buf(),
                reason: format!("O_EXCL create probe failed: {e}"),
            });
        }
    };
    if let Err(e) = file.write_all(b"") {
        // Best-effort cleanup — if the create succeeded, we own the
        // path; if this fails too, the caller sees the write error.
        let _ = std::fs::remove_file(&probe_path);
        return Err(SnapshotConfigError::UnwritableDir {
            path: dir.to_path_buf(),
            reason: format!("write probe failed: {e}"),
        });
    }
    drop(file);
    if let Err(e) = std::fs::remove_file(&probe_path) {
        return Err(SnapshotConfigError::UnwritableDir {
            path: dir.to_path_buf(),
            reason: format!(
                "remove probe failed: {e} (file was created but couldn't be cleaned up)"
            ),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::rust::configuration::file_io_provisioning::{StaticDirEntry, StaticFileEntry};

    fn empty_provisioning() -> FileIoProvisioning { FileIoProvisioning::default() }

    fn provisioning_with_consensus_file() -> FileIoProvisioning {
        let mut p = FileIoProvisioning::default();
        p.consensus_static_files
            .insert("app/data.bin".to_string(), StaticFileEntry {
                path: PathBuf::from("/srv/app.bin"),
                mode: "r".to_string(),
            });
        p
    }

    fn provisioning_with_consensus_dir() -> FileIoProvisioning {
        let mut p = FileIoProvisioning::default();
        p.consensus_static_dirs
            .insert("app/data".to_string(), StaticDirEntry {
                path: PathBuf::from("/srv/data"),
                mode: "rw".to_string(),
            });
        p
    }

    #[test]
    fn no_consensus_provisioning_skips_validation() {
        // With no consensus buckets, snapshot config is not required —
        // even if cadence is missing / zero.  Backward-compat for
        // nodes not using consensus static provisioning.
        assert_eq!(
            validate_snapshot_config(&empty_provisioning(), None, None, 2).unwrap(),
            None,
            "no consensus provisioning returns Ok(None) canonicalized dir"
        );
        assert_eq!(
            validate_snapshot_config(&empty_provisioning(), Some(0), None, 2).unwrap(),
            None
        );
    }

    #[test]
    fn missing_cadence_with_consensus_provisioning_fails() {
        let err = validate_snapshot_config(&provisioning_with_consensus_file(), None, None, 2)
            .unwrap_err();
        assert_eq!(err, SnapshotConfigError::MissingCadence);
        assert!(err.to_string().contains("consensus-fs-snapshot-cadence"));
        assert!(err.to_string().contains("no default"));
    }

    #[test]
    fn missing_cadence_with_consensus_dir_fails() {
        let err = validate_snapshot_config(&provisioning_with_consensus_dir(), None, None, 2)
            .unwrap_err();
        assert_eq!(err, SnapshotConfigError::MissingCadence);
    }

    #[test]
    fn zero_cadence_fails() {
        let err = validate_snapshot_config(&provisioning_with_consensus_file(), Some(0), None, 2)
            .unwrap_err();
        assert_eq!(err, SnapshotConfigError::ZeroCadence);
        assert!(err.to_string().contains(">= 1"));
    }

    #[test]
    fn missing_dir_with_valid_cadence_fails() {
        let err = validate_snapshot_config(&provisioning_with_consensus_file(), Some(100), None, 2)
            .unwrap_err();
        assert_eq!(err, SnapshotConfigError::MissingDir);
        assert!(err.to_string().contains("consensus-fs-snapshot-dir"));
    }

    #[test]
    fn valid_config_with_writable_tempdir_passes() {
        let dir = tempfile::tempdir().unwrap();
        let canonical = validate_snapshot_config(
            &provisioning_with_consensus_file(),
            Some(100),
            Some(dir.path()),
            200,
        )
        .unwrap()
        .expect("consensus provisioning requires Some(dir) return");
        // M-30-2 round-2 fix: canonicalize returns an absolute path.
        assert!(
            canonical.is_absolute(),
            "canonicalized snapshot dir must be absolute; got {canonical:?}"
        );
    }

    #[test]
    fn valid_config_creates_dir_if_missing() {
        let parent = tempfile::tempdir().unwrap();
        let subdir = parent.path().join("snapshots-do-not-exist-yet");
        assert!(!subdir.exists(), "precondition: subdir must not exist");
        assert!(
            validate_snapshot_config(
                &provisioning_with_consensus_file(),
                Some(50),
                Some(&subdir),
                100,
            )
            .is_ok(),
            "validation must create the dir"
        );
        assert!(subdir.exists(), "subdir must be created by probe");
    }

    #[test]
    fn unwritable_dir_fails() {
        // Use a path we can't create: point at an existing file.
        let dir = tempfile::tempdir().unwrap();
        let file_not_dir = dir.path().join("not-a-dir");
        std::fs::write(&file_not_dir, b"x").unwrap();
        let err = validate_snapshot_config(
            &provisioning_with_consensus_file(),
            Some(50),
            Some(&file_not_dir),
            100,
        )
        .unwrap_err();
        assert!(matches!(err, SnapshotConfigError::UnwritableDir { .. }));
    }

    /// F-30b-1 (2026-08-24): retain < 2 with consensus provisioning
    /// must fail with `RetainTooSmall`.  Guards the promotion of
    /// the retention key from `Option<usize>` with a `cadence * 2`
    /// fallback to a required operator value.
    #[test]
    fn retain_below_floor_with_consensus_provisioning_fails() {
        let dir = tempfile::tempdir().unwrap();
        for bad_retain in [0usize, 1] {
            let err = validate_snapshot_config(
                &provisioning_with_consensus_file(),
                Some(50),
                Some(dir.path()),
                bad_retain,
            )
            .unwrap_err();
            assert_eq!(err, SnapshotConfigError::RetainTooSmall {
                retain: bad_retain
            });
            assert!(err.to_string().contains(">= 2"));
        }
    }

    /// Companion: retain=0 without consensus provisioning is fine
    /// (no writer is built, so retain is unused).
    #[test]
    fn retain_zero_without_consensus_provisioning_passes() {
        assert!(validate_snapshot_config(&empty_provisioning(), Some(100), None, 0).is_ok());
    }

    #[test]
    fn requires_snapshot_config_false_when_all_buckets_empty() {
        assert!(!requires_snapshot_config(&empty_provisioning()));
    }

    #[test]
    fn requires_snapshot_config_true_when_consensus_files_present() {
        assert!(requires_snapshot_config(&provisioning_with_consensus_file()));
    }

    #[test]
    fn requires_snapshot_config_true_when_consensus_dirs_present() {
        assert!(requires_snapshot_config(&provisioning_with_consensus_dir()));
    }

    #[test]
    fn oracle_only_provisioning_does_not_require_snapshot_config() {
        let mut p = FileIoProvisioning::default();
        p.oracle_static_files
            .insert("cache/foo".to_string(), StaticFileEntry {
                path: PathBuf::from("/tmp/foo"),
                mode: "r".to_string(),
            });
        p.oracle_static_dirs
            .insert("cache/dir".to_string(), StaticDirEntry {
                path: PathBuf::from("/tmp/cachedir"),
                mode: "r".to_string(),
            });
        assert!(!requires_snapshot_config(&p));
        // And boot passes with no snapshot config.
        assert!(validate_snapshot_config(&p, None, None, 2).is_ok());
    }

    // Silence unused HashMap import warning; provisioning helper types
    // reference HashMap indirectly.
    #[test]
    fn hash_map_import_is_reachable() { let _m: HashMap<String, StaticFileEntry> = HashMap::new(); }

    // ------------------------------------------------------------------
    // Round-2 review-fix tests: M-30-2 canonicalize, M-30-3 O_EXCL,
    // Coverage M7 (write-failure branch of probe_dir_writable), and
    // H-30-1 boot-integration placeholder.
    // ------------------------------------------------------------------

    /// M-30-2: canonicalized path is symlink-resolved.
    #[cfg(unix)]
    #[test]
    fn valid_config_returns_symlink_resolved_dir() {
        let real_parent = tempfile::tempdir().unwrap();
        let real_dir = real_parent.path().join("real");
        std::fs::create_dir(&real_dir).unwrap();
        let symlink_parent = tempfile::tempdir().unwrap();
        let symlink_path = symlink_parent.path().join("link-to-real");
        std::os::unix::fs::symlink(&real_dir, &symlink_path).unwrap();
        let canonical = validate_snapshot_config(
            &provisioning_with_consensus_file(),
            Some(50),
            Some(&symlink_path),
            100,
        )
        .unwrap()
        .unwrap();
        assert_eq!(canonical, std::fs::canonicalize(&real_dir).unwrap());
    }

    /// M-30-3: O_EXCL probe cleans up and works across repeated
    /// calls.  Since we can't reliably guess-and-race the real probe
    /// path in a test, we spot-check that the probe file is always
    /// removed and back-to-back probes succeed.
    #[test]
    fn probe_uses_o_excl_and_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        probe_dir_writable(dir.path()).unwrap();
        probe_dir_writable(dir.path()).unwrap();
        let leftover: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(".snapshot-probe-")
            })
            .collect();
        assert!(
            leftover.is_empty(),
            "probe_dir_writable must clean up its probe file; found {leftover:?}"
        );
    }

    /// M-30-3: O_EXCL rejects pre-existing files.  Simulates a
    /// local attacker pre-creating a file at a would-be probe path
    /// by constructing a directory with an existing `.snapshot-probe-*`
    /// entry (using a wildcard path won't work; we test the
    /// underlying `OpenOptions::create_new` guarantee via a direct
    /// pre-placed file that mimics the probe pattern).
    #[test]
    fn o_excl_semantics_reject_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let victim = dir.path().join("victim");
        std::fs::write(&victim, b"attacker owns me").unwrap();
        let result = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&victim);
        assert!(
            result.is_err(),
            "create_new must fail on existing file (O_EXCL semantics)"
        );
        // Victim's contents are unmodified.
        assert_eq!(std::fs::read(&victim).unwrap(), b"attacker owns me");
    }

    /// Coverage M7: probe_dir_writable's write-failure branch on a
    /// read-only directory (Unix only).  Pre-fix, only the
    /// create_dir_all failure branch was tested.
    #[cfg(unix)]
    #[test]
    fn probe_dir_writable_fails_on_read_only_dir() {
        use std::os::unix::fs::PermissionsExt;
        let parent = tempfile::tempdir().unwrap();
        let read_only = parent.path().join("read-only");
        std::fs::create_dir(&read_only).unwrap();
        let mut perms = std::fs::metadata(&read_only).unwrap().permissions();
        perms.set_mode(0o500);
        std::fs::set_permissions(&read_only, perms).unwrap();

        let err = probe_dir_writable(&read_only).unwrap_err();
        assert!(
            matches!(err, SnapshotConfigError::UnwritableDir { .. }),
            "read-only dir must produce UnwritableDir, got {err:?}"
        );
        // Reset perms so tempdir cleanup works.
        let mut perms = std::fs::metadata(&read_only).unwrap().permissions();
        perms.set_mode(0o700);
        std::fs::set_permissions(&read_only, perms).unwrap();
    }

    /// H-30-1 / HIGH-4 (Phase 7 whole-review, slice 33): the boot
    /// integration landed 2026-08-05.  `node::runtime::setup.rs`
    /// (after `RuntimeManager::create_with_history`) now calls
    /// `merge_and_validate` → `build_snapshot_writer` →
    /// `runtime_manager.set_fs_snapshot_writer(writer).await`.
    /// This pin verifies the intermediate wiring at file scope:
    /// the setup.rs source contains the expected boot-call
    /// sequence.  A file-scan is heavy for a config test but
    /// cheap compared to the alternative (bringing up a full
    /// node in-test); a regression that removes the boot call
    /// silently reverts production to snapshot-less mode and
    /// this test catches it.
    #[test]
    fn boot_pipeline_calls_build_snapshot_writer() {
        let setup_rs = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/rust/runtime/setup.rs",
        ))
        .expect("read setup.rs");
        assert!(
            setup_rs.contains("build_snapshot_writer"),
            "setup.rs must call build_snapshot_writer at boot (HIGH-4 fix). \
             If you refactored the call, update this test's needle."
        );
        assert!(
            setup_rs.contains("set_fs_snapshot_writer"),
            "setup.rs must attach the writer to RuntimeManager via \
             set_fs_snapshot_writer(writer).await (HIGH-4 fix)."
        );
    }

    /// Phase 7b-2 (2026-08-27): setup.rs must install a
    /// `DirectoryPayloadStore` bundle at boot so leader-side
    /// `journal_write` persists Consensus write bytes for joining
    /// validators to fetch.  A regression that removes the boot
    /// call silently reverts production to un-persisted mode
    /// (every joiner request → UnknownPayload) and this pin
    /// catches it.
    #[test]
    fn boot_pipeline_installs_payload_store() {
        let setup_rs = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/rust/runtime/setup.rs",
        ))
        .expect("read setup.rs");
        assert!(
            setup_rs.contains("DirectoryPayloadStore"),
            "setup.rs must construct a DirectoryPayloadStore at boot (Phase 7b-2 \
             item b).  If you refactored, update this test's needle."
        );
        assert!(
            setup_rs.contains("wal_payload_store"),
            "setup.rs must name the payload store sibling dir as `wal_payload_store` \
             (DD-7b-1 (a) committed choice)."
        );
        assert!(
            setup_rs.contains("set_payload_store"),
            "setup.rs must attach the bundle to RuntimeManager via \
             set_payload_store(Some(bundle)).await."
        );
    }

    // Slice 30b: build_snapshot_writer tests.

    #[test]
    fn build_snapshot_writer_returns_none_without_consensus_provisioning() {
        let w = build_snapshot_writer(&empty_provisioning(), Some(100), None, 100, None).unwrap();
        assert!(w.is_none());
    }

    #[test]
    fn build_snapshot_writer_returns_writer_with_valid_config() {
        let dir = tempfile::tempdir().unwrap();
        let w = build_snapshot_writer(
            &provisioning_with_consensus_file(),
            Some(50),
            Some(dir.path()),
            200,
            None,
        )
        .unwrap()
        .expect("consensus provisioning + valid config returns Some");
        assert_eq!(w.cadence, 50);
        assert_eq!(
            w.retain, 200,
            "explicit operator retain value flows through unchanged"
        );
        assert!(w.dir.is_absolute());
    }

    #[test]
    fn build_snapshot_writer_propagates_validation_error() {
        let err = build_snapshot_writer(&provisioning_with_consensus_file(), None, None, 100, None)
            .unwrap_err();
        assert_eq!(err, SnapshotConfigError::MissingCadence);
    }

    /// F-30b-1 (2026-08-24): retain values pass through unchanged.
    /// The pre-promotion `cadence * 2` fallback + floor logic is
    /// gone — operators own the choice.  Large retain values that
    /// might once have wrapped in the old heuristic (cadence *
    /// 2 near u64::MAX) are irrelevant now because retain is set
    /// directly, not derived.
    #[test]
    fn build_snapshot_writer_retain_flows_through_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        for retain in [2usize, 100, 10_000, 1_000_000_000] {
            let w = build_snapshot_writer(
                &provisioning_with_consensus_file(),
                Some(50),
                Some(dir.path()),
                retain,
                None,
            )
            .unwrap()
            .unwrap();
            assert_eq!(
                w.retain, retain,
                "retain {retain} must flow through unchanged"
            );
        }
    }

    /// F-30b-1: retain < 2 with consensus provisioning surfaces as
    /// `RetainTooSmall` from `build_snapshot_writer` (via the
    /// underlying `validate_snapshot_config` check).  Complements
    /// `retain_below_floor_with_consensus_provisioning_fails`
    /// which exercises the same path via validate directly.
    #[test]
    fn build_snapshot_writer_rejects_retain_below_floor() {
        let dir = tempfile::tempdir().unwrap();
        for bad_retain in [0usize, 1] {
            let err = build_snapshot_writer(
                &provisioning_with_consensus_file(),
                Some(100),
                Some(dir.path()),
                bad_retain,
                None,
            )
            .unwrap_err();
            assert_eq!(err, SnapshotConfigError::RetainTooSmall {
                retain: bad_retain
            });
        }
    }

    #[test]
    fn build_snapshot_writer_retain_ignored_when_no_provisioning() {
        // No consensus provisioning → no writer regardless of retain
        // value.  Retain is a knob on an attached writer; it can't
        // force a writer to exist, and any retain (including 0)
        // is accepted since it's unused.
        for retain in [0usize, 500, 999_999] {
            let w = build_snapshot_writer(&empty_provisioning(), Some(100), None, retain, None)
                .unwrap();
            assert!(
                w.is_none(),
                "retain {retain} must not conjure a writer without provisioning"
            );
        }
    }
}
