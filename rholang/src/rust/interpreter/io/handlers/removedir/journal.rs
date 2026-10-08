// Consensus-recursive composer: walk + unlink + journal.
//
// Ties slice 5.138's `collect_recursive_manifest` + `unlink_manifest_entry`
// to slice 5.137's reply builders + the WAL + the per-entry
// ack-seed primitive (slice 5.136).
//
// # What this function does
//
//   1. `collect_recursive_manifest(parent.as_raw_fd(), parent.leaf_ptr())`
//      walks the target subtree under the TOCTOU-pinned
//      `SafeParent` and returns a sorted post-order manifest.
//   2. For each entry (child-first, target-root-last):
//        * Compute `wal_path = canon_wal_target.join(rel_path)`
//          with an empty-rel special case so WAL byte-identity
//          holds across validators walking equivalent subdirs
//          under Shape A.
//        * Derive the per-entry ack seed via `per_entry_ack_seed`
//          so leader + follower see identical sidecar keys.
//        * `wal_handle.append_with_ack(entry, per_entry_ack)` —
//          append-first discipline so a WAL-cap failure aborts
//          the walk BEFORE any further filesystem mutation.  On
//          failure return `err_with_manifest(FSERR_QUOTA_EXCEEDED,
//          …, &deleted)`.
//        * `unlink_manifest_entry(parent, &rel_path, kind)` —
//          TOCTOU-immune unlink via pinned dirfd chain.  On
//          success add to `deleted`; on failure patch the WAL
//          row's outcome to `Failure { code }` and return
//          `err_with_manifest(io_err_code, io_msg, &deleted)`.
//   3. On full success return `ok_recursive_manifest(&deleted)`.
//
// # Both-sides byte-identity
//
// Both leader and follower re-execute this function on their OWN
// per-validator subdirs (Shape A / D3 discipline).  Equivalent
// subdirs → identical manifests → identical WAL entries →
// identical per-entry ack hashes → identical sidecar routing →
// identical reply Pars.
//
// # Non-goals
//
// - Does NOT do the outer `safe_descend` / lock-check /
//   `spawn_blocking` wrapping.  Callers pre-descend and pass the
//   pinned `SafeParent`.
// - Does NOT compute the cost supplement.  Callers charge it
//   AFTER inspecting the returned reply via
//   `extract_removedir_n_deleted` (slice 5.139).

use std::path::PathBuf;

use models::rhoapi::Par;

use super::reply::{err_with_manifest, io_err_code_u32, ok_recursive_manifest};
use super::walk::{collect_recursive_manifest, unlink_manifest_entry};
use crate::rust::interpreter::io::errors::{io_err_code, FSERR_QUOTA_EXCEEDED};
use crate::rust::interpreter::io::handlers::helpers::{per_entry_ack_seed, RemoveKind};
use crate::rust::interpreter::io::path::{io_msg_scrub, SafeParent};
use crate::rust::interpreter::io::wal::{Wal, WalEntry, WalOp, WalOutcome};

/// Walk + unlink + journal a Consensus-recursive `fs_remove_dir`.
///
/// # Ordering guarantees
///
/// - [`collect_recursive_manifest`] returns sorted post-order
///   (children before parent, alphabetical within a directory),
///   so the WAL journal emission order + reply manifest order are
///   deterministic and identical across validators walking
///   equivalent subdirs under Shape A.
/// - Each entry's WAL row is appended BEFORE its unlink fires
///   (append-first discipline).  A WAL-cap failure aborts the
///   walk BEFORE any further filesystem mutation.
/// - Per-entry ack seed is derived from the WAL path
///   (bundle-relative under Shape A), so leader + follower
///   produce identical hash keys → identical `Wal` sidecar
///   routing.
///
/// # Failure semantics
///
/// - Empty manifest impossible: `collect_recursive_manifest`
///   always emits at least the target-root sentinel entry.
/// - WAL-cap exhaustion → `err_with_manifest(FSERR_QUOTA_EXCEEDED,
///   …, &deleted)` where `deleted` reflects the entries
///   successfully removed BEFORE the cap hit.
/// - Per-entry unlink failure → patch WAL row's outcome to
///   `Failure { code }` via `update_outcome_by_ack_hash` before
///   returning the truncated deleted list wrapped in
///   `err_with_manifest(io_err_code(&e), io_msg_scrub(&e),
///   &deleted)`.
///
/// Returns the reply `Par` (success or partial-failure manifest).
/// The caller charges the cost supplement based on
/// `extract_removedir_n_deleted(reply)`.
pub(super) fn walk_and_unlink_recursive_with_journal(
    parent: &SafeParent,
    canon_wal_target: &std::path::Path,
    ack_clone: &Par,
    wal_handle: &Wal,
) -> Par {
    let manifest = match collect_recursive_manifest(parent.as_raw_fd(), parent.leaf_ptr()) {
        Ok(m) => m,
        Err(e) => {
            return err_with_manifest(io_err_code(&e), io_msg_scrub(&e), &[]);
        }
    };
    let mut deleted: Vec<(PathBuf, RemoveKind)> = Vec::new();
    for (rel_path, kind) in manifest {
        // Empty rel_path marks the target root (final post-order
        // entry).  `Path::join("")` appends a trailing separator
        // whose serialized bytes would differ from the un-joined
        // target — special-case to preserve WAL byte-for-byte
        // identity.  `PathBuf::eq` is component-wise so the issue
        // hides under direct comparison; `encode_wal_slice`'s
        // byte compare surfaces it.
        let wal_path = if rel_path.as_os_str().is_empty() {
            canon_wal_target.to_path_buf()
        } else {
            canon_wal_target.join(&rel_path)
        };
        let op = match kind {
            RemoveKind::File => WalOp::RemoveFile,
            RemoveKind::Dir => WalOp::RemoveDir,
        };
        // Per-entry ack seeded on WAL path (bundle-relative) so
        // leader + follower produce identical per-entry ack
        // hashes.
        let per_entry_ack = per_entry_ack_seed(ack_clone, &wal_path);
        if wal_handle
            .append_with_ack(
                WalEntry {
                    op,
                    path: wal_path,
                    extra_path: None,
                    offset: None,
                    length: None,
                    payload_ref: None,
                    mode_bits: None,
                    owner: None,
                    group: None,
                    outcome: WalOutcome::Success,
                },
                per_entry_ack,
            )
            .is_err()
        {
            return err_with_manifest(
                FSERR_QUOTA_EXCEEDED,
                "WAL cap exceeded during recursive removeDir",
                &deleted,
            );
        }
        // TOCTOU-immune unlink via pinned dirfd chain from
        // `parent`.
        match unlink_manifest_entry(parent, &rel_path, kind) {
            Ok(()) => {
                deleted.push((rel_path, kind));
            }
            Err(e) => {
                let code_u32 = io_err_code_u32(&e);
                let _ = wal_handle.update_outcome_by_ack_hash(per_entry_ack, WalOutcome::Failure {
                    code: code_u32,
                });
                return err_with_manifest(io_err_code(&e), io_msg_scrub(&e), &deleted);
            }
        }
    }
    ok_recursive_manifest(&deleted)
}
