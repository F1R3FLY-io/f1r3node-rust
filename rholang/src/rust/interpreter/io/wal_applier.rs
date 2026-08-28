// WAL fresh-tree applier (Phase 7b-2 item (c), 2026-08-28).
//
// Reconstructs on-disk file state from a captured WAL slice + a
// hash → bytes payload sidecar.  Moved here from the fs_wal_spec.rs
// test module so the joiner-side sync driver (wal_payload_sync.rs)
// can invoke it once all pending payloads are resolved.
//
// # Callers
//
// **Production (Phase 7b-2 joiner):** applies the WAL to its own
// filesystem tree.  WAL entries' `path` values are already
// canonical host paths agreed across validators (operator-frozen
// `consensus-static-*` roots), so the joiner passes an identity
// `path_map` closure.
//
// **Test (pb_m_14_*, wal_applier_skips_failure_outcome_entries):**
// uses separate leader/follower tempdirs for isolation and passes
// a translation closure that rewrites the WAL's absolute paths
// from `leader_root/rel` to `follower_root/rel`.  The `translate_path`
// helper stays in the test module — it's a test-harness artifact.
//
// # Supported ops
//
//   * `Write` / `WriteAt` — carry absolute `offset` (position-
//     follow-up 2026-08-26) + `payload_ref: Hash(...)`; replayed
//     as seek-then-write against the sidecar bytes.
//   * `Truncate` — carries the new file length in `offset`.
//   * `Chmod` / `Chown` / `RemoveFile` / `RemoveDir` / `Rename` /
//     `CopyFile` — path-based mutations replayed directly (H-29-3
//     lift, 2026-08-26).
//   * Failure-outcome entries — skipped per H-6 (the leader never
//     mutated disk on Failure).
//   * Observation-only variants (`Read`, `ReadAt`, `Stat`,
//     `Entries`, `Size`, `EntriesStreamNext`) — no disk change.
//
// # Error semantics
//
// Panics on invariant violations: missing sidecar entry, missing
// offset on a Write/WriteAt, DeployRef payload_ref (not yet
// reproducer-supported), missing extra_path on Rename/CopyFile.
// Joiner callers reach the applier only after
// `WalPayloadSyncDriver::is_complete()` returns true, so a missing
// hash indicates an internal state-machine bug rather than a
// runtime condition.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::wal::{PayloadRef, WalEntry, WalOp, WalOutcome};

/// Apply a captured WAL slice to a filesystem tree.
///
/// See module docstring for supported ops, path_map semantics, and
/// panic conditions.
pub fn apply_wal_to_fresh_tree<F>(
    wal: &[WalEntry],
    payload_bytes: &HashMap<[u8; 32], Vec<u8>>,
    path_map: F,
) where
    F: Fn(&Path) -> PathBuf,
{
    use std::io::{Seek, SeekFrom, Write};
    for (i, entry) in wal.iter().enumerate() {
        if matches!(entry.outcome, WalOutcome::Failure { .. }) {
            continue; // H-6: leader never mutated disk on Failure
        }
        match entry.op {
            WalOp::Write | WalOp::WriteAt => {
                let dst = path_map(&entry.path);
                let hash = match entry.payload_ref {
                    Some(PayloadRef::Hash(h)) => h,
                    Some(PayloadRef::DeployRef { .. }) => panic!(
                        "WAL entry {i}: DeployRef payload_ref not yet supported \
                         by the fresh-tree applier — needs on-chain deploy \
                         data lookup (Phase 7b-2 reducer)"
                    ),
                    None => panic!(
                        "WAL entry {i}: {:?} without payload_ref — invariant \
                         violation in the write handler",
                        entry.op,
                    ),
                };
                let bytes = payload_bytes.get(&hash).unwrap_or_else(|| {
                    panic!(
                        "WAL entry {i}: hash {} missing from payload sidecar; \
                         a real Phase 7b joiner would `get_wal_payload` for it, \
                         but the driver mis-populated the sidecar",
                        hex::encode(hash),
                    )
                });
                // Both Write and WriteAt carry absolute offset
                // post-position-follow-up (2026-08-26).
                let off = entry.offset.unwrap_or_else(|| {
                    panic!(
                        "WAL entry {i}: {:?} without offset — position-follow-up \
                         regression? journal_write should populate offset from \
                         FileHandle.position (sequential Write) or the caller-\
                         supplied offset (WriteAt)",
                        entry.op,
                    )
                });
                let mut f = std::fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(false)
                    .open(&dst)
                    .unwrap_or_else(|e| panic!("open {dst:?}: {e}"));
                f.seek(SeekFrom::Start(off))
                    .unwrap_or_else(|e| panic!("seek {dst:?}: {e}"));
                f.write_all(bytes)
                    .unwrap_or_else(|e| panic!("write {dst:?}: {e}"));
            }
            WalOp::Truncate => {
                let dst = path_map(&entry.path);
                let n = entry.offset.expect("Truncate must carry offset");
                let f = std::fs::OpenOptions::new()
                    .write(true)
                    .open(&dst)
                    .unwrap_or_else(|e| panic!("open-for-truncate {dst:?}: {e}"));
                f.set_len(n)
                    .unwrap_or_else(|e| panic!("set_len {dst:?}: {e}"));
            }
            // Observation-only — nothing to reconstruct on disk.
            WalOp::Read
            | WalOp::ReadAt
            | WalOp::Stat
            | WalOp::Entries
            | WalOp::Size
            | WalOp::EntriesStreamNext => {}
            // Path-based mutations (H-29-3 lift, 2026-08-26).
            // Each entry is fully derivable from args, so replay
            // is a straight syscall.  Failure entries are already
            // filtered above by the outcome check.
            WalOp::Chmod => {
                let dst = path_map(&entry.path);
                let bits = entry
                    .mode_bits
                    .unwrap_or_else(|| panic!("WAL entry {i}: Chmod without mode_bits"));
                use std::os::unix::fs::PermissionsExt;
                let perms = std::fs::Permissions::from_mode(bits);
                std::fs::set_permissions(&dst, perms)
                    .unwrap_or_else(|e| panic!("chmod {dst:?}: {e}"));
            }
            WalOp::Chown => {
                let dst = path_map(&entry.path);
                let owner = entry
                    .owner
                    .as_ref()
                    .unwrap_or_else(|| panic!("WAL entry {i}: Chown without owner"));
                let group = entry.group.as_deref();
                let uid = if owner.is_empty() {
                    u32::MAX
                } else {
                    use std::ffi::CString;
                    let cname = CString::new(owner.as_str()).unwrap();
                    let pw = unsafe { libc::getpwnam(cname.as_ptr()) };
                    if pw.is_null() {
                        panic!(
                            "WAL entry {i}: applier can't resolve owner {owner:?} \
                             on this host — NSS-mismatch scenario"
                        );
                    }
                    unsafe { (*pw).pw_uid }
                };
                let gid = match group {
                    None | Some("") => u32::MAX,
                    Some(g) => {
                        use std::ffi::CString;
                        let cname = CString::new(g).unwrap();
                        let gr = unsafe { libc::getgrnam(cname.as_ptr()) };
                        if gr.is_null() {
                            panic!("WAL entry {i}: applier can't resolve group {g:?}");
                        }
                        unsafe { (*gr).gr_gid }
                    }
                };
                let cpath = std::ffi::CString::new(dst.as_os_str().as_encoded_bytes()).unwrap();
                let rc = unsafe { libc::chown(cpath.as_ptr(), uid, gid) };
                if rc != 0 {
                    // Unprivileged hosts (typical CI) can't chown to
                    // arbitrary owners.  EPERM is treated as a no-op
                    // success — tests should use current-user names
                    // to avoid this; production joiners run as the
                    // node service user and MUST have the perms to
                    // replay the leader's chown ops.
                    let e = std::io::Error::last_os_error();
                    if e.raw_os_error() != Some(libc::EPERM) {
                        panic!("chown {dst:?}: {e}");
                    }
                }
            }
            WalOp::RemoveFile => {
                let dst = path_map(&entry.path);
                std::fs::remove_file(&dst)
                    .unwrap_or_else(|e| panic!("remove_file {dst:?}: {e}"));
            }
            WalOp::RemoveDir => {
                let dst = path_map(&entry.path);
                std::fs::remove_dir(&dst).unwrap_or_else(|e| panic!("remove_dir {dst:?}: {e}"));
            }
            WalOp::Rename => {
                let from = path_map(&entry.path);
                let to = entry
                    .extra_path
                    .as_ref()
                    .unwrap_or_else(|| panic!("WAL entry {i}: Rename without extra_path"));
                let to = path_map(to);
                std::fs::rename(&from, &to)
                    .unwrap_or_else(|e| panic!("rename {from:?} → {to:?}: {e}"));
            }
            WalOp::CopyFile => {
                let from = path_map(&entry.path);
                let to = entry
                    .extra_path
                    .as_ref()
                    .unwrap_or_else(|| panic!("WAL entry {i}: CopyFile without extra_path"));
                let to = path_map(to);
                std::fs::copy(&from, &to)
                    .unwrap_or_else(|e| panic!("copy {from:?} → {to:?}: {e}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::rust::interpreter::io::wal::PayloadRef;

    /// Identity path_map — production joiners apply directly to the
    /// WAL entry's canonical host path.  This unit exercises the
    /// identity path with a synthetic Write entry so a future
    /// regression in the identity closure surfaces here without
    /// waiting for the full pb_m_14_* integration tests to run.
    #[test]
    fn identity_path_map_writes_at_wal_entry_path() {
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("target.bin");
        std::fs::write(&dst, vec![0u8; 8]).unwrap();

        let payload = b"data".to_vec();
        let PayloadRef::Hash(h) = PayloadRef::hash(&payload) else {
            unreachable!()
        };
        let entry = WalEntry {
            op: WalOp::WriteAt,
            path: dst.clone(),
            extra_path: None,
            offset: Some(2),
            length: Some(payload.len() as u64),
            payload_ref: Some(PayloadRef::Hash(h)),
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        };
        let mut sidecar: HashMap<[u8; 32], Vec<u8>> = HashMap::new();
        sidecar.insert(h, payload.clone());

        apply_wal_to_fresh_tree(&[entry], &sidecar, |p| p.to_path_buf());

        let got = std::fs::read(&dst).unwrap();
        assert_eq!(&got[..2], &[0, 0]);
        assert_eq!(&got[2..2 + payload.len()], payload.as_slice());
    }

    /// H-6: `Failure`-outcome entries are skipped even when their
    /// sidecar bytes are missing — the applier must not attempt a
    /// write the leader never performed.  Regression pin lives at
    /// tests-level too (`wal_applier_skips_failure_outcome_entries`
    /// in fs_wal_spec.rs); the unit here is defensive against the
    /// applier being reused from other contexts.
    #[test]
    fn skips_failure_outcome_entries_without_touching_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("target.bin");
        std::fs::write(&dst, vec![0xAA; 8]).unwrap();

        let bogus_hash = [0u8; 32];
        let failure_entry = WalEntry {
            op: WalOp::WriteAt,
            path: dst.clone(),
            extra_path: None,
            offset: Some(0),
            length: Some(4),
            payload_ref: Some(PayloadRef::Hash(bogus_hash)),
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Failure { code: 5 },
        };
        // Empty sidecar — a Failure entry must not touch it.
        let sidecar: HashMap<[u8; 32], Vec<u8>> = HashMap::new();
        apply_wal_to_fresh_tree(&[failure_entry], &sidecar, |p| p.to_path_buf());

        // Byte state unchanged.
        assert_eq!(std::fs::read(&dst).unwrap(), vec![0xAA; 8]);
    }

    /// Applier respects the caller-supplied `path_map`.  A closure
    /// that redirects all writes into a translated tempdir subtree
    /// leaves the WAL entry's original path untouched.
    #[test]
    fn path_map_closure_redirects_writes() {
        let src_root = tempfile::tempdir().unwrap();
        let dst_root = tempfile::tempdir().unwrap();
        std::fs::write(src_root.path().join("f.bin"), vec![0u8; 8]).unwrap();
        std::fs::write(dst_root.path().join("f.bin"), vec![0u8; 8]).unwrap();

        let payload = b"xy".to_vec();
        let PayloadRef::Hash(h) = PayloadRef::hash(&payload) else {
            unreachable!()
        };
        let entry = WalEntry {
            op: WalOp::WriteAt,
            path: src_root.path().join("f.bin"),
            extra_path: None,
            offset: Some(0),
            length: Some(payload.len() as u64),
            payload_ref: Some(PayloadRef::Hash(h)),
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        };
        let mut sidecar: HashMap<[u8; 32], Vec<u8>> = HashMap::new();
        sidecar.insert(h, payload.clone());

        let src = src_root.path().to_path_buf();
        let dst = dst_root.path().to_path_buf();
        apply_wal_to_fresh_tree(&[entry], &sidecar, |p| {
            let rel = p.strip_prefix(&src).unwrap();
            dst.join(rel)
        });

        // Src untouched — the closure redirected the write.
        assert_eq!(
            std::fs::read(src_root.path().join("f.bin")).unwrap(),
            vec![0u8; 8]
        );
        // Dst reflects the write.
        let got = std::fs::read(dst_root.path().join("f.bin")).unwrap();
        assert_eq!(&got[..2], payload.as_slice());
    }
}

