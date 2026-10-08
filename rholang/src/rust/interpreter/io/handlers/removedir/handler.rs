// Trait-exempt `fs_remove_dir` handler method on `FsProcesses`.
//
// This is the entry point the runtime dispatcher calls once the
// trait-exempt stub (`SystemProcesses::fs_remove_dir_stub` in
// `system_processes.rs`) is swapped out.  Composes everything in
// the sibling submodules:
//
//   * slice 5.136 — `per_entry_ack_seed` + `MAX_RECURSION_DEPTH`.
//   * slice 5.137 — reply builders (`ok_with_count`,
//     `err_with_count`, `ok_recursive_manifest`,
//     `err_with_manifest`, `early_err_for_remove_dir`) +
//     `RemoveKindAsWire`.
//   * slice 5.138 — walker syscall primitives
//     (`unlink_leaf_via_dirfd` via `helpers::unlink`,
//     `remove_dir_recursive` with depth cap).
//   * slice 5.139 — reply readers
//     (`extract_removedir_n_deleted`, cost-supplement wrappers).
//   * slice 5.140 — Consensus-recursive composer
//     `walk_and_unlink_recursive_with_journal`.
//
// # Reply shape (DD-RemoveDirReplyShape)
//
// Every code path returns `nDeleted` at position 1 (success) or
// position 3 (failure):
//
//   * Non-recursive: `[true, 1]` / `[false, code, msg, 0]`.
//   * Recursive Oracular: `[true, nDeleted]` /
//     `[false, code, msg, nDeletedBeforeError]`.
//   * Recursive Consensus:
//     `[true, nDeleted, [[path, kind], ...]]` /
//     `[false, code, msg, nDeletedBeforeError, [[path, kind], ...]]`
//     — manifest at position 2/4 is an implementation-side
//     channel for R5(b) follower re-execution; Dir.rho unwraps
//     to the uniform shape at the Rholang boundary.
//
// # Shape A invariant (Task 0.4)
//
// WAL entries record BUNDLE-RELATIVE paths (via
// `canon_wal_target = canonicalize_lexical(root, rel)` where
// `root` is the RAW Rholang canonRoot).  Syscalls use the
// on-disk absolute (via `root_registry.resolve_or_identity_gated_for_consensus`).
// This split lets a joining validator rewrite WAL entries
// through its own registry at boot while leader/follower append
// byte-identical bytes.

#![allow(dead_code)]

use std::path::PathBuf;

use models::rhoapi::{ListParWithRandom, Par};

use super::journal::walk_and_unlink_recursive_with_journal;
use super::reply::{
    early_err_for_remove_dir, err_with_count, err_with_manifest, fs_remove_dir_supplement_count,
    fs_remove_dir_supplement_count_from_previous, io_err_code_u32, ok_with_count,
};
use super::walk::remove_dir_recursive;
use crate::rust::interpreter::errors::{illegal_argument_error, InterpreterError};
use crate::rust::interpreter::io::costs::{self};
use crate::rust::interpreter::io::errors::{
    fserr_to_code, io_err_code, FSERR_BAD_ARG, FSERR_BUSY, FSERR_CODE_CONSENSUS_DIVERGENCE,
    FSERR_CONSENSUS_DIVERGENCE, FSERR_QUOTA_EXCEEDED,
};
use crate::rust::interpreter::io::handler_trait::{spawn_blocking_par, FsProcesses};
use crate::rust::interpreter::io::handlers::helpers::{
    ack_channel_hash, finalize_failure_journal_via_table, journal_path_mutation_single_via_table,
    target_dev_inode_at, unlink_leaf_via_dirfd, RemoveKind,
};
use crate::rust::interpreter::io::path::descend::safe_descend_verified;
use crate::rust::interpreter::io::path::{
    canonicalize_lexical, io_msg_scrub, quarantine_err_reply,
};
use crate::rust::interpreter::io::response::extract_err_code;
use crate::rust::interpreter::io::wal::{WalEntry, WalOp, WalOutcome};
use crate::rust::interpreter::io::ConsensusMode;
use crate::rust::interpreter::rho_type::{RhoBoolean, RhoString};

impl FsProcesses {
    /// Handler for `fs_remove_dir(rootCanon, rel, recursive, cmode,
    /// ack)`.  Trait-exempt (vs. the uniform 27 trait-registered
    /// handlers in sibling families) per the submodule docstring —
    /// two-mode dispatch, inline recursive-walk syscall loop, and
    /// a reply shape carrying `nDeleted`.
    ///
    /// # Dispatch cases
    ///
    /// 1. **cmode parse failure** → `[false, FSERR_BAD_ARG, msg, 0]`
    ///    produced + returned.
    /// 2. **args parse failure** → `[false, FSERR_BAD_ARG, msg, 0]`
    ///    produced + returned (recursive unknown, generic 4-element
    ///    shape).
    /// 3. **is_replay** (follower):
    ///    a. **Non-recursive Consensus** — re-execute via
    ///   `safe_descend_verified` + journal + lock check +
    ///   `unlink_leaf_via_dirfd`; `verify_reply_hash_matches_cached`;
    ///   on divergence emit CONSENSUS_DIVERGENCE + finalize
    ///   the WAL row.
    ///    b. **Non-recursive Oracular** — journal (no-op for
    ///   Oracular) + H-6 finalize-from-cached.
    ///    c. **Recursive Consensus** — R5(b) re-execute via
    ///   `walk_and_unlink_recursive_with_journal` + verify;
    ///   on divergence emit 5-element CONSENSUS_DIVERGENCE
    ///   (per-entry WAL placeholders left as-is — they reflect
    ///   follower's actual syscalls, which may have succeeded
    ///   on the follower's own subdir).
    ///    d. Fall-through (recursive Oracular, None-parsed) → cost
    ///   supplement from `previous` + produce + return.
    /// 4. **Leader**:
    ///    - `safe_descend_verified` under the Shape A on-disk root.
    ///    - Lock check: Consensus + locked → FSERR_BUSY early-fail.
    ///      Oracular + locked → warn log with `n_holders`, proceed.
    ///    - Non-recursive: single `WAL append` + `unlink_leaf_via_dirfd`.
    ///    - Recursive Oracular: `remove_dir_recursive(depth=0)` with
    ///      inline unlinks; count returned in the reply.
    ///    - Recursive Consensus:
    ///      `walk_and_unlink_recursive_with_journal`.
    ///    - Cost supplement from the fresh reply + produce.
    ///
    /// Panics via `join_err_abort` on `spawn_blocking` JoinError
    /// (DD-FailClosedOnInvariantBreak) — pre-T-20 soft-reply
    /// fallbacks are dead per fileio T-20 (2026-09-11).  See
    /// `spawn_blocking_par` docstring.
    pub async fn fs_remove_dir(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        self.metering
            .reserve_primitive(costs::fs_remove_dir_cost(0))?;
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_remove_dir"));
        };
        let [root_par, rel_par, recursive_par, cmode_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_remove_dir"));
        };
        let cmode = match super::super::super::resolve_cmode(cmode_par) {
            Some(m) => m,
            None => {
                let out = vec![err_with_count(
                    FSERR_BAD_ARG,
                    "cmode must be String \"oracular\" or \"consensus\"",
                    0,
                )];
                produce(&out, ack).await?;
                return Ok(out);
            }
        };
        let parsed = match (
            RhoString::unapply(root_par),
            RhoString::unapply(rel_par),
            RhoBoolean::unapply(recursive_par),
        ) {
            (Some(root), Some(rel), Some(recursive)) => Some((root, rel, recursive)),
            _ => None,
        };
        if is_replay {
            if let Some((root, rel, recursive)) = &parsed {
                if !recursive && cmode == ConsensusMode::Consensus {
                    // Non-recursive Consensus follower re-execute.
                    let canon_path = match canonicalize_lexical(root, rel) {
                        Ok(p) => p,
                        Err(qe) => {
                            let (c, m) = quarantine_err_reply(&qe);
                            let out = vec![err_with_count(c, m, 0)];
                            produce(&out, ack).await?;
                            return Ok(out);
                        }
                    };
                    if journal_path_mutation_single_via_table(
                        &self.handles,
                        cmode,
                        WalOp::RemoveDir,
                        canon_path,
                        None,
                        None,
                        None,
                        ack,
                    )
                    .await
                    .is_err()
                    {
                        let out = vec![err_with_count(FSERR_QUOTA_EXCEEDED, "WAL cap exceeded", 0)];
                        produce(&out, ack).await?;
                        return Ok(out);
                    }
                    let raw_root_pb = PathBuf::from(root);
                    let (on_disk_root_pb, expected_root_id) = match self
                        .handles
                        .root_registry
                        .resolve_or_identity_gated_for_consensus(&raw_root_pb, cmode)
                    {
                        Ok(v) => v,
                        Err((c, m)) => {
                            let out = vec![early_err_for_remove_dir(*recursive, cmode, c, m)];
                            produce(&out, ack).await?;
                            return Ok(out);
                        }
                    };
                    let rel_owned = rel.to_string();
                    let lock_registry = self.handles.lock_registry.clone();
                    let fresh_reply = spawn_blocking_par(move || -> Par {
                        let parent = match safe_descend_verified(
                            &on_disk_root_pb,
                            &rel_owned,
                            expected_root_id,
                        ) {
                            Ok(p) => p,
                            Err(qe) => {
                                let (c, m) = quarantine_err_reply(&qe);
                                return err_with_count(c, m, 0);
                            }
                        };
                        let target_dev_inode = target_dev_inode_at(&parent);
                        let target_is_locked = target_dev_inode
                            .map(|di| lock_registry.is_locked(di, (0, u64::MAX)))
                            .unwrap_or(false);
                        if target_is_locked {
                            return err_with_count(
                                FSERR_BUSY,
                                "cannot remove: lock held on target (dev, inode)",
                                0,
                            );
                        }
                        match unlink_leaf_via_dirfd(&parent, RemoveKind::Dir) {
                            Ok(()) => ok_with_count(1),
                            Err(e) => err_with_count(io_err_code(&e), io_msg_scrub(&e), 0),
                        }
                    })
                    .await;
                    let supp_n =
                        fs_remove_dir_supplement_count_from_previous(&parsed, cmode, &previous);
                    self.metering.reserve_incremental_primitive(
                        costs::fs_remove_dir_per_entry_supplement_cost(supp_n),
                    )?;
                    match crate::rust::interpreter::io::verify::verify_reply_hash_matches_cached(
                        &fresh_reply,
                        &previous,
                    ) {
                        Ok(()) => {
                            if let Some(code_str) =
                                extract_err_code(std::slice::from_ref(&fresh_reply))
                            {
                                finalize_failure_journal_via_table(
                                    &self.handles,
                                    fserr_to_code(&code_str),
                                    ack,
                                );
                            }
                            let out = vec![fresh_reply];
                            produce(&out, ack).await?;
                            return Ok(out);
                        }
                        Err(reason) => {
                            let divergence_reply = err_with_count(
                                FSERR_CONSENSUS_DIVERGENCE,
                                format!(
                                    "fs_remove_dir follower re-execute diverges from leader: \
                                     {reason}",
                                ),
                                0,
                            );
                            finalize_failure_journal_via_table(
                                &self.handles,
                                FSERR_CODE_CONSENSUS_DIVERGENCE,
                                ack,
                            );
                            let out = vec![divergence_reply];
                            produce(&out, ack).await?;
                            return Ok(out);
                        }
                    }
                }
                if !recursive {
                    // Non-recursive Oracular: no-op journal + H-6
                    // finalize from cached.  canonicalize_lexical
                    // Err is treated as "skip the journal" since
                    // Oracular journaling is already a no-op; the
                    // finalize-from-cached step runs regardless.
                    if let Ok(canon_path) = canonicalize_lexical(root, rel) {
                        let _ = journal_path_mutation_single_via_table(
                            &self.handles,
                            cmode,
                            WalOp::RemoveDir,
                            canon_path,
                            None,
                            None,
                            None,
                            ack,
                        )
                        .await;
                    }
                    if let Some(code_str) = extract_err_code(&previous) {
                        finalize_failure_journal_via_table(
                            &self.handles,
                            fserr_to_code(&code_str),
                            ack,
                        );
                    }
                } else if cmode == ConsensusMode::Consensus {
                    // R5(b) recursive Consensus follower re-execute.
                    let raw_root_pb = PathBuf::from(root);
                    let (on_disk_root_pb, expected_root_id) = match self
                        .handles
                        .root_registry
                        .resolve_or_identity_gated_for_consensus(&raw_root_pb, cmode)
                    {
                        Ok(v) => v,
                        Err((c, m)) => {
                            let out = vec![early_err_for_remove_dir(*recursive, cmode, c, m)];
                            produce(&out, ack).await?;
                            return Ok(out);
                        }
                    };
                    let canon_wal_target = match canonicalize_lexical(root, rel) {
                        Ok(p) => p,
                        Err(qe) => {
                            let (c, m) = quarantine_err_reply(&qe);
                            let out = vec![early_err_for_remove_dir(*recursive, cmode, c, m)];
                            produce(&out, ack).await?;
                            return Ok(out);
                        }
                    };
                    let rel_owned = rel.to_string();
                    let lock_registry = self.handles.lock_registry.clone();
                    let wal_handle = self.handles.wal.clone();
                    let ack_clone = ack.clone();
                    let fresh_reply = spawn_blocking_par(move || -> Par {
                        let parent = match safe_descend_verified(
                            &on_disk_root_pb,
                            &rel_owned,
                            expected_root_id,
                        ) {
                            Ok(p) => p,
                            Err(qe) => {
                                let (c, m) = quarantine_err_reply(&qe);
                                return err_with_manifest(c, m, &[]);
                            }
                        };
                        let target_dev_inode = target_dev_inode_at(&parent);
                        let target_is_locked = target_dev_inode
                            .map(|di| lock_registry.is_locked(di, (0, u64::MAX)))
                            .unwrap_or(false);
                        if target_is_locked {
                            return err_with_manifest(
                                FSERR_BUSY,
                                "cannot remove: lock held on target (dev, inode)",
                                &[],
                            );
                        }
                        walk_and_unlink_recursive_with_journal(
                            &parent,
                            &canon_wal_target,
                            &ack_clone,
                            &wal_handle,
                        )
                    })
                    .await;
                    let supp_n = fs_remove_dir_supplement_count(&parsed, cmode, &fresh_reply);
                    self.metering.reserve_incremental_primitive(
                        costs::fs_remove_dir_per_entry_supplement_cost(supp_n),
                    )?;
                    match crate::rust::interpreter::io::verify::verify_reply_hash_matches_cached(
                        &fresh_reply,
                        &previous,
                    ) {
                        Ok(()) => {
                            let out = vec![fresh_reply];
                            produce(&out, ack).await?;
                            return Ok(out);
                        }
                        Err(reason) => {
                            let divergence_reply = err_with_manifest(
                                FSERR_CONSENSUS_DIVERGENCE,
                                format!(
                                    "fs_remove_dir recursive follower re-execute \
                                     diverges from leader: {reason}",
                                ),
                                &[],
                            );
                            let out = vec![divergence_reply];
                            produce(&out, ack).await?;
                            return Ok(out);
                        }
                    }
                }
            }
            // Fall-through: non-recursive Oracular (post-journal),
            // recursive Oracular, or None-parsed.  Cost supplement
            // from `previous` + produce.
            let supp_n = fs_remove_dir_supplement_count_from_previous(&parsed, cmode, &previous);
            self.metering.reserve_incremental_primitive(
                costs::fs_remove_dir_per_entry_supplement_cost(supp_n),
            )?;
            produce(&previous, ack).await?;
            return Ok(previous);
        }

        // Leader path.
        let reply = match parsed.clone() {
            Some((root, rel, recursive)) => {
                let raw_root_pb = PathBuf::from(&root);
                let (on_disk_root_pb, expected_root_id) = match self
                    .handles
                    .root_registry
                    .resolve_or_identity_gated_for_consensus(&raw_root_pb, cmode)
                {
                    Ok(v) => v,
                    Err((c, m)) => {
                        let out = vec![early_err_for_remove_dir(recursive, cmode, c, m)];
                        produce(&out, ack).await?;
                        return Ok(out);
                    }
                };
                let canon_wal_target = match canonicalize_lexical(&root, &rel) {
                    Ok(p) => p,
                    Err(qe) => {
                        let (c, m) = quarantine_err_reply(&qe);
                        let out = vec![early_err_for_remove_dir(recursive, cmode, c, m)];
                        produce(&out, ack).await?;
                        return Ok(out);
                    }
                };
                let lock_registry = self.handles.lock_registry.clone();
                let ack_clone = ack.clone();
                let wal = self.handles.wal.clone();
                spawn_blocking_par(move || -> Par {
                    let parent =
                        match safe_descend_verified(&on_disk_root_pb, &rel, expected_root_id) {
                            Ok(p) => p,
                            Err(qe) => {
                                let (c, m) = quarantine_err_reply(&qe);
                                return early_err_for_remove_dir(recursive, cmode, c, m);
                            }
                        };
                    let target_dev_inode = target_dev_inode_at(&parent);
                    let target_is_locked = target_dev_inode
                        .map(|di| lock_registry.is_locked(di, (0, u64::MAX)))
                        .unwrap_or(false);
                    if cmode == ConsensusMode::Consensus && target_is_locked {
                        return early_err_for_remove_dir(
                            recursive,
                            cmode,
                            FSERR_BUSY,
                            "cannot remove: lock held on target (dev, inode)",
                        );
                    }
                    if cmode == ConsensusMode::Oracular && target_is_locked {
                        if let Some((dev, ino)) = target_dev_inode {
                            let n_holders = lock_registry.n_holders((dev, ino));
                            tracing::warn!(
                                target: "f1r3fly.fs.oracular",
                                dev = dev,
                                ino = ino,
                                n_holders = n_holders,
                                "oracular removeDir of locked directory (dev={dev}, ino={ino}) \
                                 — {n_holders} holder(s) will observe subsequent errors on \
                                 path-based calls; fd-based calls remain valid until close",
                            );
                        }
                    }
                    if !recursive {
                        // Non-recursive leader path.
                        if cmode == ConsensusMode::Consensus {
                            let e = wal.append_with_ack(
                                WalEntry {
                                    op: WalOp::RemoveDir,
                                    path: canon_wal_target.clone(),
                                    extra_path: None,
                                    offset: None,
                                    length: None,
                                    payload_ref: None,
                                    mode_bits: None,
                                    owner: None,
                                    group: None,
                                    outcome: WalOutcome::Success,
                                },
                                ack_channel_hash(&ack_clone),
                            );
                            if e.is_err() {
                                return err_with_count(FSERR_QUOTA_EXCEEDED, "WAL cap exceeded", 0);
                            }
                        }
                        match unlink_leaf_via_dirfd(&parent, RemoveKind::Dir) {
                            Ok(()) => return ok_with_count(1),
                            Err(e) => {
                                if cmode == ConsensusMode::Consensus {
                                    let _ = wal.update_outcome_by_ack_hash(
                                        ack_channel_hash(&ack_clone),
                                        WalOutcome::Failure {
                                            code: io_err_code_u32(&e),
                                        },
                                    );
                                }
                                return err_with_count(io_err_code(&e), io_msg_scrub(&e), 0);
                            }
                        }
                    }
                    // Recursive leader path.
                    if cmode == ConsensusMode::Oracular {
                        match remove_dir_recursive(parent.as_raw_fd(), parent.leaf_ptr(), 0) {
                            Ok(n) => ok_with_count(n),
                            Err((n_before, e)) => {
                                err_with_count(io_err_code(&e), io_msg_scrub(&e), n_before)
                            }
                        }
                    } else {
                        walk_and_unlink_recursive_with_journal(
                            &parent,
                            &canon_wal_target,
                            &ack_clone,
                            &wal,
                        )
                    }
                })
                .await
            }
            None => err_with_count(FSERR_BAD_ARG, "expected (String, String, Bool, String)", 0),
        };
        let supp_n = fs_remove_dir_supplement_count(&parsed, cmode, &reply);
        self.metering.reserve_incremental_primitive(
            costs::fs_remove_dir_per_entry_supplement_cost(supp_n),
        )?;
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }
}
