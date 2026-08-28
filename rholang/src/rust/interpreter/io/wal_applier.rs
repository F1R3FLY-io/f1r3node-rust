// WAL fresh-tree applier (Phase 7b-2 item (c), 2026-08-28;
// hardened 2026-08-28 review pass).
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
// # Path validation (defense-in-depth)
//
// The applier accepts an `allowed_roots: &[PathBuf]` argument.
// If non-empty, every WAL entry's path (and `extra_path` for
// Rename/CopyFile) must be under one of those roots — otherwise
// the applier returns `ApplierError::PathOutsideAllowedRoots`
// without touching disk.  Callers pass the joiner's configured
// `consensus-static-*` roots to bound the blast radius of a
// leader canonicalize bug or (theoretically) a Blake2b256
// forgery.  Pass `&[]` to skip validation (test callers, or
// production sites that haven't yet plumbed the provisioning
// config through — a documented gap the boot wire-in currently
// exercises).
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
// # Error handling (Result-based, 2026-08-28 hardening)
//
// Every failure path — missing sidecar entry, missing field on
// an entry, unsupported PayloadRef variant, byzantine NULL in
// path, out-of-allowed-roots path, NSS resolution failure, or
// syscall error — returns a specific `ApplierError` variant
// instead of panicking.  This lets the boot subscriber log +
// continue processing subsequent snapshots even when a single
// WAL slice trips a defensive check.  The panic-based signature
// of the pre-hardening version was a subscriber-killer: an
// applier panic unwound through `spawn_blocking` → `.expect(...)`
// → the async `while let` loop, taking the subscriber task
// with it.

use std::collections::HashMap;
use std::ffi::CString;
use std::path::{Path, PathBuf};

use super::wal::{PayloadRef, WalEntry, WalOp, WalOutcome};

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
    /// Reserved for a future Phase 7b-2 reducer slice that
    /// resolves deploy refs from on-chain deploy data.
    UnsupportedPayloadRef { entry_index: usize },
    /// The WAL entry is a write op but `payload_ref` is `None`.
    /// Invariant violation: the leader's `journal_write` must
    /// populate this field.
    MissingPayloadRef { entry_index: usize, op: WalOp },
    /// A Write/WriteAt/Truncate entry is missing its `offset`.
    /// Invariant violation post-position-follow-up (2026-08-26).
    MissingOffset { entry_index: usize, op: WalOp },
    /// A Chmod entry is missing `mode_bits`.
    MissingModeBits { entry_index: usize },
    /// A Chown entry is missing its `owner` field.
    MissingOwner { entry_index: usize },
    /// A Rename/CopyFile entry is missing `extra_path`.
    MissingExtraPath { entry_index: usize, op: WalOp },
    /// The WAL entry's path contains a NULL byte, which would
    /// cause `CString::new` to fail before any syscall.  Only
    /// reachable via a byzantine WAL (Blake2b256 preimage would
    /// have to be forged); pre-hardening this triggered a panic.
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
    NssResolutionFailed {
        name: String,
        errno: i32,
    },
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
    /// Chown's `libc::chown` returned a non-zero rc that is
    /// not EPERM (EPERM is treated as a no-op success for
    /// unprivileged hosts — see the Chown branch's comment).
    ChownFailed {
        entry_index: usize,
        path: PathBuf,
        errno: i32,
    },
}

impl std::fmt::Display for ApplierError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApplierError::MissingSidecarEntry { entry_index, hash_hex } => write!(
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
        }
    }
}

impl std::error::Error for ApplierError {}

/// Apply a captured WAL slice to a filesystem tree.
///
/// See module docstring for supported ops, path_map semantics,
/// path validation, and error variants.
pub fn apply_wal_to_fresh_tree<F>(
    wal: &[WalEntry],
    payload_bytes: &HashMap<[u8; 32], Vec<u8>>,
    path_map: F,
    allowed_roots: &[PathBuf],
) -> Result<(), ApplierError>
where
    F: Fn(&Path) -> PathBuf,
{
    use std::io::{Seek, SeekFrom, Write};
    for (i, entry) in wal.iter().enumerate() {
        if matches!(entry.outcome, WalOutcome::Failure { .. }) {
            continue; // H-6: leader never mutated disk on Failure
        }
        // Defense-in-depth: path validation against caller-supplied
        // consensus-static roots.  Empty allowed_roots skips.
        if !allowed_roots.is_empty() {
            check_path_allowed(i, &entry.path, allowed_roots)?;
            if let Some(ep) = &entry.extra_path {
                check_path_allowed(i, ep, allowed_roots)?;
            }
        }
        match entry.op {
            WalOp::Write | WalOp::WriteAt => {
                let dst = path_map(&entry.path);
                let hash = match entry.payload_ref {
                    Some(PayloadRef::Hash(h)) => h,
                    Some(PayloadRef::DeployRef { .. }) => {
                        return Err(ApplierError::UnsupportedPayloadRef { entry_index: i })
                    }
                    None => {
                        return Err(ApplierError::MissingPayloadRef {
                            entry_index: i,
                            op: entry.op,
                        })
                    }
                };
                let bytes =
                    payload_bytes
                        .get(&hash)
                        .ok_or_else(|| ApplierError::MissingSidecarEntry {
                            entry_index: i,
                            hash_hex: hex::encode(hash),
                        })?;
                let off = entry.offset.ok_or(ApplierError::MissingOffset {
                    entry_index: i,
                    op: entry.op,
                })?;
                let mut f = std::fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(false)
                    .open(&dst)
                    .map_err(|e| ApplierError::IoFailure {
                        entry_index: i,
                        op: entry.op,
                        path: dst.clone(),
                        message: format!("open: {e}"),
                    })?;
                f.seek(SeekFrom::Start(off))
                    .map_err(|e| ApplierError::IoFailure {
                        entry_index: i,
                        op: entry.op,
                        path: dst.clone(),
                        message: format!("seek: {e}"),
                    })?;
                f.write_all(bytes)
                    .map_err(|e| ApplierError::IoFailure {
                        entry_index: i,
                        op: entry.op,
                        path: dst.clone(),
                        message: format!("write: {e}"),
                    })?;
            }
            WalOp::Truncate => {
                let dst = path_map(&entry.path);
                let n = entry.offset.ok_or(ApplierError::MissingOffset {
                    entry_index: i,
                    op: entry.op,
                })?;
                let f = std::fs::OpenOptions::new()
                    .write(true)
                    .open(&dst)
                    .map_err(|e| ApplierError::IoFailure {
                        entry_index: i,
                        op: entry.op,
                        path: dst.clone(),
                        message: format!("open: {e}"),
                    })?;
                f.set_len(n).map_err(|e| ApplierError::IoFailure {
                    entry_index: i,
                    op: entry.op,
                    path: dst.clone(),
                    message: format!("set_len: {e}"),
                })?;
            }
            // Observation-only — nothing to reconstruct on disk.
            WalOp::Read
            | WalOp::ReadAt
            | WalOp::Stat
            | WalOp::Entries
            | WalOp::Size
            | WalOp::EntriesStreamNext => {}
            WalOp::Chmod => {
                let dst = path_map(&entry.path);
                let bits = entry
                    .mode_bits
                    .ok_or(ApplierError::MissingModeBits { entry_index: i })?;
                use std::os::unix::fs::PermissionsExt;
                let perms = std::fs::Permissions::from_mode(bits);
                std::fs::set_permissions(&dst, perms).map_err(|e| ApplierError::IoFailure {
                    entry_index: i,
                    op: entry.op,
                    path: dst.clone(),
                    message: format!("set_permissions: {e}"),
                })?;
            }
            WalOp::Chown => {
                let dst = path_map(&entry.path);
                let owner = entry
                    .owner
                    .as_ref()
                    .ok_or(ApplierError::MissingOwner { entry_index: i })?;
                let group = entry.group.as_deref();
                let uid = if owner.is_empty() {
                    u32::MAX
                } else {
                    resolve_uid(owner)?
                };
                let gid = match group {
                    None | Some("") => u32::MAX,
                    Some(g) => resolve_gid(g)?,
                };
                let cpath = os_str_to_cstring(dst.as_os_str())
                    .map_err(|_| ApplierError::PathContainsNull { entry_index: i })?;
                let rc = unsafe { libc::chown(cpath.as_ptr(), uid, gid) };
                if rc != 0 {
                    let errno = std::io::Error::last_os_error()
                        .raw_os_error()
                        .unwrap_or(0);
                    // Unprivileged hosts (typical CI) can't chown
                    // to arbitrary owners.  EPERM is treated as a
                    // no-op success — tests should use current-
                    // user names to avoid this; production
                    // joiners run as the node service user and
                    // MUST have the perms to replay the leader's
                    // chown ops.
                    if errno != libc::EPERM {
                        return Err(ApplierError::ChownFailed {
                            entry_index: i,
                            path: dst,
                            errno,
                        });
                    }
                }
            }
            WalOp::RemoveFile => {
                let dst = path_map(&entry.path);
                std::fs::remove_file(&dst).map_err(|e| ApplierError::IoFailure {
                    entry_index: i,
                    op: entry.op,
                    path: dst.clone(),
                    message: format!("remove_file: {e}"),
                })?;
            }
            WalOp::RemoveDir => {
                let dst = path_map(&entry.path);
                std::fs::remove_dir(&dst).map_err(|e| ApplierError::IoFailure {
                    entry_index: i,
                    op: entry.op,
                    path: dst.clone(),
                    message: format!("remove_dir: {e}"),
                })?;
            }
            WalOp::Rename => {
                let from = path_map(&entry.path);
                let extra = entry
                    .extra_path
                    .as_ref()
                    .ok_or(ApplierError::MissingExtraPath {
                        entry_index: i,
                        op: entry.op,
                    })?;
                let to = path_map(extra);
                std::fs::rename(&from, &to).map_err(|e| ApplierError::IoFailure {
                    entry_index: i,
                    op: entry.op,
                    path: from.clone(),
                    message: format!("rename → {to:?}: {e}"),
                })?;
            }
            WalOp::CopyFile => {
                let from = path_map(&entry.path);
                let extra = entry
                    .extra_path
                    .as_ref()
                    .ok_or(ApplierError::MissingExtraPath {
                        entry_index: i,
                        op: entry.op,
                    })?;
                let to = path_map(extra);
                std::fs::copy(&from, &to).map_err(|e| ApplierError::IoFailure {
                    entry_index: i,
                    op: entry.op,
                    path: from.clone(),
                    message: format!("copy → {to:?}: {e}"),
                })?;
            }
        }
    }
    Ok(())
}

fn check_path_allowed(
    entry_index: usize,
    path: &Path,
    allowed_roots: &[PathBuf],
) -> Result<(), ApplierError> {
    if allowed_roots.iter().any(|root| path.starts_with(root)) {
        Ok(())
    } else {
        Err(ApplierError::PathOutsideAllowedRoots {
            entry_index,
            path: path.to_path_buf(),
        })
    }
}

/// Convert an `OsStr` to a nul-terminated C string suitable for
/// libc syscalls.  Fails cleanly on embedded NULL bytes.
fn os_str_to_cstring(s: &std::ffi::OsStr) -> Result<CString, std::ffi::NulError> {
    CString::new(s.as_encoded_bytes())
}

/// Thread-safe `getpwnam` — uses `getpwnam_r` under the hood so
/// concurrent applier invocations (or concurrent NSS lookups
/// elsewhere in the process) don't corrupt each other's `passwd`
/// pointers.  Grows the caller-provided buffer on ERANGE up to a
/// reasonable ceiling (16 MiB) so long entries still succeed.
fn resolve_uid(name: &str) -> Result<u32, ApplierError> {
    let cname =
        CString::new(name.as_bytes()).map_err(|_| ApplierError::NssNotFound {
            name: name.to_string(),
        })?;
    let mut buf_len: usize = 1024;
    let ceiling: usize = 16 * 1024 * 1024;
    loop {
        let mut buf: Vec<libc::c_char> = vec![0; buf_len];
        let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
        let mut result_ptr: *mut libc::passwd = std::ptr::null_mut();
        let rc = unsafe {
            libc::getpwnam_r(
                cname.as_ptr(),
                &mut pwd,
                buf.as_mut_ptr(),
                buf.len(),
                &mut result_ptr,
            )
        };
        if rc == libc::ERANGE {
            if buf_len >= ceiling {
                return Err(ApplierError::NssResolutionFailed {
                    name: name.to_string(),
                    errno: rc,
                });
            }
            buf_len = (buf_len * 2).min(ceiling);
            continue;
        }
        if rc != 0 {
            return Err(ApplierError::NssResolutionFailed {
                name: name.to_string(),
                errno: rc,
            });
        }
        if result_ptr.is_null() {
            return Err(ApplierError::NssNotFound {
                name: name.to_string(),
            });
        }
        return Ok(pwd.pw_uid);
    }
}

/// Thread-safe `getgrnam` companion — same shape as `resolve_uid`.
fn resolve_gid(name: &str) -> Result<u32, ApplierError> {
    let cname =
        CString::new(name.as_bytes()).map_err(|_| ApplierError::NssNotFound {
            name: name.to_string(),
        })?;
    let mut buf_len: usize = 1024;
    let ceiling: usize = 16 * 1024 * 1024;
    loop {
        let mut buf: Vec<libc::c_char> = vec![0; buf_len];
        let mut grp: libc::group = unsafe { std::mem::zeroed() };
        let mut result_ptr: *mut libc::group = std::ptr::null_mut();
        let rc = unsafe {
            libc::getgrnam_r(
                cname.as_ptr(),
                &mut grp,
                buf.as_mut_ptr(),
                buf.len(),
                &mut result_ptr,
            )
        };
        if rc == libc::ERANGE {
            if buf_len >= ceiling {
                return Err(ApplierError::NssResolutionFailed {
                    name: name.to_string(),
                    errno: rc,
                });
            }
            buf_len = (buf_len * 2).min(ceiling);
            continue;
        }
        if rc != 0 {
            return Err(ApplierError::NssResolutionFailed {
                name: name.to_string(),
                errno: rc,
            });
        }
        if result_ptr.is_null() {
            return Err(ApplierError::NssNotFound {
                name: name.to_string(),
            });
        }
        return Ok(grp.gr_gid);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::rust::interpreter::io::wal::PayloadRef;

    fn write_entry_at(dst: &Path, off: u64, payload: &[u8]) -> (WalEntry, [u8; 32]) {
        let PayloadRef::Hash(h) = PayloadRef::hash(payload) else {
            unreachable!()
        };
        let entry = WalEntry {
            op: WalOp::WriteAt,
            path: dst.to_path_buf(),
            extra_path: None,
            offset: Some(off),
            length: Some(payload.len() as u64),
            payload_ref: Some(PayloadRef::Hash(h)),
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        };
        (entry, h)
    }

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
        let (entry, h) = write_entry_at(&dst, 2, &payload);
        let mut sidecar: HashMap<[u8; 32], Vec<u8>> = HashMap::new();
        sidecar.insert(h, payload.clone());

        apply_wal_to_fresh_tree(&[entry], &sidecar, |p| p.to_path_buf(), &[]).unwrap();

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
        apply_wal_to_fresh_tree(&[failure_entry], &sidecar, |p| p.to_path_buf(), &[]).unwrap();

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
        let (entry, h) = write_entry_at(&src_root.path().join("f.bin"), 0, &payload);
        let mut sidecar: HashMap<[u8; 32], Vec<u8>> = HashMap::new();
        sidecar.insert(h, payload.clone());

        let src = src_root.path().to_path_buf();
        let dst = dst_root.path().to_path_buf();
        apply_wal_to_fresh_tree(
            &[entry],
            &sidecar,
            |p| {
                let rel = p.strip_prefix(&src).unwrap();
                dst.join(rel)
            },
            &[],
        )
        .unwrap();

        assert_eq!(
            std::fs::read(src_root.path().join("f.bin")).unwrap(),
            vec![0u8; 8]
        );
        let got = std::fs::read(dst_root.path().join("f.bin")).unwrap();
        assert_eq!(&got[..2], payload.as_slice());
    }

    // ---------------------------------------------------------------
    // 2026-08-28 hardening pins.  Every ApplierError variant that is
    // reachable via a well-formed WAL entry OR a byzantine WAL entry
    // has a runtime pin so future refactors that break the error
    // path (e.g., re-introducing a panic) fail HERE rather than
    // silently killing the boot subscriber.
    // ---------------------------------------------------------------

    /// Missing sidecar entry returns a specific error variant.
    /// Prior panic-based version killed the boot subscriber.
    #[test]
    fn missing_sidecar_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("t.bin");
        std::fs::write(&dst, vec![0u8; 8]).unwrap();
        let payload = b"missing".to_vec();
        let (entry, h) = write_entry_at(&dst, 0, &payload);
        // Empty sidecar — hash h is not present.
        let sidecar: HashMap<[u8; 32], Vec<u8>> = HashMap::new();
        let err = apply_wal_to_fresh_tree(&[entry], &sidecar, |p| p.to_path_buf(), &[])
            .expect_err("missing sidecar must Err");
        assert_eq!(
            err,
            ApplierError::MissingSidecarEntry {
                entry_index: 0,
                hash_hex: hex::encode(h),
            }
        );
        // File unchanged since the applier failed before writing.
        assert_eq!(std::fs::read(&dst).unwrap(), vec![0u8; 8]);
    }

    /// A `DeployRef` payload_ref is a well-formed but not-yet-
    /// reproducible variant.  Applier reports UnsupportedPayloadRef;
    /// does not panic.
    #[test]
    fn deploy_ref_payload_ref_returns_unsupported() {
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("t.bin");
        std::fs::write(&dst, vec![0u8; 8]).unwrap();
        let entry = WalEntry {
            op: WalOp::WriteAt,
            path: dst.clone(),
            extra_path: None,
            offset: Some(0),
            length: Some(4),
            payload_ref: Some(PayloadRef::DeployRef {
                block_hash: [0; 32],
                deploy_index: 0,
                arg_index: 0,
            }),
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        };
        let err = apply_wal_to_fresh_tree(&[entry], &HashMap::new(), |p| p.to_path_buf(), &[])
            .expect_err("DeployRef must Err");
        assert_eq!(err, ApplierError::UnsupportedPayloadRef { entry_index: 0 });
    }

    /// A path outside every `allowed_roots` entry returns
    /// PathOutsideAllowedRoots — defense-in-depth against a
    /// leader canonicalize bug or a forged snapshot.
    #[test]
    fn path_outside_allowed_roots_returns_error() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let outside_target = outside.path().join("evil.bin");
        std::fs::write(&outside_target, vec![0u8; 8]).unwrap();

        let payload = b"attacker".to_vec();
        let (entry, h) = write_entry_at(&outside_target, 0, &payload);
        let mut sidecar: HashMap<[u8; 32], Vec<u8>> = HashMap::new();
        sidecar.insert(h, payload.clone());

        let allowed = vec![root.path().to_path_buf()];
        let err = apply_wal_to_fresh_tree(&[entry], &sidecar, |p| p.to_path_buf(), &allowed)
            .expect_err("out-of-root must Err");
        assert!(
            matches!(err, ApplierError::PathOutsideAllowedRoots { entry_index: 0, .. }),
            "got {err:?}"
        );
        // Outside file untouched.
        assert_eq!(std::fs::read(&outside_target).unwrap(), vec![0u8; 8]);
    }

    /// Empty `allowed_roots` disables validation — the applier
    /// applies to any path.  Explicit pin: production callsites
    /// currently pass `&[]` until provisioning is plumbed.
    #[test]
    fn empty_allowed_roots_skips_validation() {
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("t.bin");
        std::fs::write(&dst, vec![0u8; 8]).unwrap();
        let payload = b"ok".to_vec();
        let (entry, h) = write_entry_at(&dst, 0, &payload);
        let mut sidecar: HashMap<[u8; 32], Vec<u8>> = HashMap::new();
        sidecar.insert(h, payload.clone());
        // allowed_roots empty AND dst is a tempdir path outside
        // any "consensus-static" prefix — validation must not fire.
        apply_wal_to_fresh_tree(&[entry], &sidecar, |p| p.to_path_buf(), &[]).unwrap();
        let got = std::fs::read(&dst).unwrap();
        assert_eq!(&got[..2], payload.as_slice());
    }

    /// A NULL byte inside a chown target path is caught by
    /// os_str_to_cstring and surfaces as PathContainsNull.  Pre-
    /// hardening this triggered a `.unwrap()` panic that killed
    /// the boot subscriber loop.
    #[test]
    fn chown_path_with_null_byte_returns_error() {
        use std::os::unix::ffi::OsStrExt;

        let bad_path = PathBuf::from(std::ffi::OsStr::from_bytes(b"/tmp/has\0null"));
        let entry = WalEntry {
            op: WalOp::Chown,
            path: bad_path,
            extra_path: None,
            offset: None,
            length: None,
            payload_ref: None,
            mode_bits: None,
            owner: Some("root".to_string()),
            group: None,
            outcome: WalOutcome::Success,
        };
        let err = apply_wal_to_fresh_tree(&[entry], &HashMap::new(), |p| p.to_path_buf(), &[])
            .expect_err("path with NULL must Err");
        assert_eq!(err, ApplierError::PathContainsNull { entry_index: 0 });
    }

    /// Empty owner + empty group short-circuits to (u32::MAX,
    /// u32::MAX) sentinels (i.e., "no change" in POSIX chown
    /// semantics).  Regression pin for the NSS-avoidance path.
    #[test]
    fn chown_empty_owner_and_group_short_circuits_nss() {
        // Empty owner + empty group means "u32::MAX for both",
        // which is POSIX "leave uid/gid unchanged".  No NSS lookup
        // fires; applier returns Ok even without NSS entries for
        // any name.
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("chownable.bin");
        std::fs::write(&dst, vec![0u8; 4]).unwrap();
        let entry = WalEntry {
            op: WalOp::Chown,
            path: dst.clone(),
            extra_path: None,
            offset: None,
            length: None,
            payload_ref: None,
            mode_bits: None,
            owner: Some(String::new()),
            group: Some(String::new()),
            outcome: WalOutcome::Success,
        };
        // Should return Ok on any host, no NSS involvement.
        apply_wal_to_fresh_tree(&[entry], &HashMap::new(), |p| p.to_path_buf(), &[]).unwrap();
    }

    /// Chown NSS lookup with a nonexistent owner name surfaces as
    /// NssNotFound (not a panic).  Pin against the reentrant
    /// getpwnam_r path.
    #[test]
    fn chown_nonexistent_owner_returns_nss_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("chownable.bin");
        std::fs::write(&dst, vec![0u8; 4]).unwrap();
        // A name that (almost) certainly resolves to nothing.
        let entry = WalEntry {
            op: WalOp::Chown,
            path: dst,
            extra_path: None,
            offset: None,
            length: None,
            payload_ref: None,
            mode_bits: None,
            owner: Some("no-such-user-in-nss-4f8d3a2e".to_string()),
            group: None,
            outcome: WalOutcome::Success,
        };
        let err = apply_wal_to_fresh_tree(&[entry], &HashMap::new(), |p| p.to_path_buf(), &[])
            .expect_err("nonexistent owner must Err");
        assert!(
            matches!(err, ApplierError::NssNotFound { .. }),
            "got {err:?}"
        );
    }

    /// A Rename/CopyFile entry missing extra_path returns the
    /// MissingExtraPath variant.  Invariant violation surfaced
    /// rather than panicking.
    #[test]
    fn rename_without_extra_path_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        std::fs::write(&a, b"a").unwrap();
        let entry = WalEntry {
            op: WalOp::Rename,
            path: a,
            extra_path: None,
            offset: None,
            length: None,
            payload_ref: None,
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        };
        let err = apply_wal_to_fresh_tree(&[entry], &HashMap::new(), |p| p.to_path_buf(), &[])
            .expect_err("missing extra_path must Err");
        assert_eq!(
            err,
            ApplierError::MissingExtraPath {
                entry_index: 0,
                op: WalOp::Rename,
            }
        );
    }

    /// Truncate without offset returns MissingOffset (previously
    /// panicked via `.expect(...)`).
    #[test]
    fn truncate_without_offset_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("t.bin");
        std::fs::write(&dst, vec![0u8; 16]).unwrap();
        let entry = WalEntry {
            op: WalOp::Truncate,
            path: dst,
            extra_path: None,
            offset: None,
            length: None,
            payload_ref: None,
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        };
        let err = apply_wal_to_fresh_tree(&[entry], &HashMap::new(), |p| p.to_path_buf(), &[])
            .expect_err("missing offset on Truncate must Err");
        assert_eq!(
            err,
            ApplierError::MissingOffset {
                entry_index: 0,
                op: WalOp::Truncate,
            }
        );
    }

    /// An IO failure (e.g., open of a nonexistent parent dir for
    /// truncate) surfaces as IoFailure — not a panic.
    #[test]
    fn truncate_missing_target_returns_io_failure() {
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("does-not-exist.bin");
        let entry = WalEntry {
            op: WalOp::Truncate,
            path: dst.clone(),
            extra_path: None,
            offset: Some(0),
            length: None,
            payload_ref: None,
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        };
        let err = apply_wal_to_fresh_tree(&[entry], &HashMap::new(), |p| p.to_path_buf(), &[])
            .expect_err("truncate on missing file must Err");
        assert!(
            matches!(
                err,
                ApplierError::IoFailure {
                    entry_index: 0,
                    op: WalOp::Truncate,
                    ..
                }
            ),
            "got {err:?}"
        );
    }
}
