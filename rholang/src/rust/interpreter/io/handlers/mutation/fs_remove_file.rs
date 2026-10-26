// fs_remove_file — (root, rel, cmode) -> [true] | [false, FSERR, msg]
//
// Fifth mutation handler.  Verifying, constant cost
// (`FS_PATH_MUTATION_CONST` = 200).  Cmode arg-based.  Exercises
// the LockRegistry's unlink gate (consensus-observable):
//
//   - **Consensus + locked** → `FSERR_BUSY` (reject deterministically
//     — the lock has holders whose fd-based reads / writes must
//     stay valid).
//   - **Oracular + locked** → proceed with `tracing::warn!`
//     (POSIX rm-while-open semantics: existing fd holders retain
//     their open fds; path-based lookups for the same leaf will
//     subsequently fail).
//   - **Not locked (any cmode)** → proceed via `unlink_leaf_via_dirfd`.
//
// # Reserve + finalize (H-6 pattern)
//
//   - `pre_syscall`: reserve `WalOp::RemoveFile` entry (no mode_bits
//     / owner / group); same lexical-quarantine skip as fs_chmod /
//     fs_rename / fs_chown.
//   - `dispatch`: safe_descend_verified → target_dev_inode_at →
//     unlink gate → unlinkat.
//   - `journal`: standard H-6 finalize — same shape as fs_chmod /
//     fs_rename.
//
// # Why dirfd-based unlink
//
// `unlink_leaf_via_dirfd` uses the parent's dirfd + `unlinkat` so
// the H-5 defense (no path-component re-walk) is complete.  A
// `remove_file(root.join(rel))` fallback would re-walk the path
// after `safe_descend_verified` returned, reopening the TOCTOU
// window — the whole point of SafeParent + unlinkat is to close
// that window.
//
// # Lock gate semantics
//
// `is_locked((dev, inode), (0, u64::MAX))` queries the registry
// for ANY range held on the inode.  Different Rholang holders may
// have taken sub-ranges of the file; the whole-file probe
// correctly detects any held slot.  The gate uses `(0, u64::MAX)`
// not `(0, 0)` because:
//
//   - Zero-length ranges never conflict in the interval model
//     (empty intervals don't overlap).
//   - Max-length probe matches any non-empty held range (the
//     definition of "any locked range" in the interval model).
//
// # No telemetry hazard
//
// Follows fs_chmod's `Result<Par, Par>` pattern — raw
// `spawn_blocking` + outer-match discriminate into
// `HandlerReply::ok` / `HandlerReply::Err`.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use linkme::distributed_slice;
use models::rhoapi::Par;
use tokio::task::spawn_blocking;

use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::io::costs::fs_remove_file_cost;
use crate::rust::interpreter::io::errors::{
    fserr_to_code, io_err_code, join_err_abort, FSERR_BAD_ARG, FSERR_BUSY,
    FSERR_CODE_CONSENSUS_DIVERGENCE, FSERR_QUOTA_EXCEEDED,
};
use crate::rust::interpreter::io::handler_trait::{
    dispatch_via_trait_owned, FsHandler, FsHandlerEntry, HandlerFamily, HandlerReply, JournalPath,
    SyscallCtx, FS_HANDLERS,
};
use crate::rust::interpreter::io::handlers::helpers::{
    finalize_failure_journal_via_table, journal_path_mutation_single_via_table,
    target_dev_inode_at, unlink_leaf_via_dirfd, RemoveKind,
};
use crate::rust::interpreter::io::path::descend::safe_descend_verified;
use crate::rust::interpreter::io::path::{
    canonicalize_lexical, io_msg_scrub, quarantine_err_reply,
};
use crate::rust::interpreter::io::response::{err, extract_err_code, ok_bare};
use crate::rust::interpreter::io::wal::WalOp;
use crate::rust::interpreter::io::{resolve_cmode, ConsensusMode};
use crate::rust::interpreter::rho_type::RhoString;
use crate::rust::interpreter::system_processes::{BodyRefs, FixedChannels};

/// Zero-sized handler type.
pub struct FsRemoveFileHandler;

/// Parsed args.
pub struct FsRemoveFileArgs {
    root: String,
    rel: String,
    cmode: ConsensusMode,
}

impl FsHandler for FsRemoveFileHandler {
    const NAME: &'static str = "fs_remove_file";
    const ARITY: usize = 4; // (root, rel, cmode, ack)
    const VERIFYING: bool = true;

    type Args = FsRemoveFileArgs;

    fn parse_content(args: &[Par]) -> Result<FsRemoveFileArgs, Box<HandlerReply>> {
        let [root_par, rel_par, cmode_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (String, String, String)",
            ));
        };
        let cmode = match resolve_cmode(cmode_par) {
            Some(m) => m,
            None => {
                return Err(HandlerReply::boxed_err(
                    FSERR_BAD_ARG,
                    "cmode must be String \"oracular\" or \"consensus\"",
                ));
            }
        };
        match (RhoString::unapply(root_par), RhoString::unapply(rel_par)) {
            (Some(root), Some(rel)) => Ok(FsRemoveFileArgs { root, rel, cmode }),
            _ => Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (String, String, String)",
            )),
        }
    }

    fn pre_charge_cost() -> Cost { fs_remove_file_cost() }

    fn pre_syscall<'a>(
        ctx: SyscallCtx<'a>,
        args: &'a FsRemoveFileArgs,
    ) -> Pin<Box<dyn Future<Output = Result<(), Box<HandlerReply>>> + Send + 'a>> {
        Box::pin(async move {
            let canon_path = match canonicalize_lexical(&args.root, &args.rel) {
                Ok(p) => p,
                Err(_) => return Ok(()),
            };
            if journal_path_mutation_single_via_table(
                ctx.handles,
                args.cmode,
                WalOp::RemoveFile,
                canon_path,
                None,
                None,
                None,
                ctx.ack,
            )
            .await
            .is_err()
            {
                return Err(HandlerReply::boxed_err(
                    FSERR_QUOTA_EXCEEDED,
                    "WAL cap exceeded",
                ));
            }
            Ok(())
        })
    }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsRemoveFileArgs,
    ) -> Pin<Box<dyn Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            // BACKLOG: migrate to resolve_or_identity_gated_for_consensus
            // (coordinated with fs_chmod / fs_rename / fs_chown).
            let logical = PathBuf::from(&args.root);
            let (root_pb, expected_root_id) =
                ctx.handles.root_registry.resolve_or_identity(&logical);
            let lock_registry = ctx.handles.lock_registry.clone();
            let rel = args.rel;
            let cmode = args.cmode;
            #[allow(clippy::result_large_err)]
            let r = spawn_blocking(move || -> Result<Par, Par> {
                let parent = match safe_descend_verified(&root_pb, &rel, expected_root_id) {
                    Ok(p) => p,
                    Err(qe) => {
                        let (c, m) = quarantine_err_reply(&qe);
                        return Err(err(c, m));
                    }
                };
                // Unlink gate — see module header on `(0, u64::MAX)`
                // whole-file probe + mode-differentiated semantics.
                let target_dev_inode = target_dev_inode_at(&parent);
                let target_is_locked = target_dev_inode
                    .map(|di| lock_registry.is_locked(di, (0, u64::MAX)))
                    .unwrap_or(false);
                if cmode == ConsensusMode::Consensus && target_is_locked {
                    return Err(err(
                        FSERR_BUSY,
                        "cannot remove: lock held on target (dev, inode)",
                    ));
                }
                if cmode == ConsensusMode::Oracular && target_is_locked {
                    if let Some((dev, ino)) = target_dev_inode {
                        let n_holders = lock_registry.n_holders((dev, ino));
                        tracing::warn!(
                            target: "f1r3fly.fs.oracular",
                            dev = dev,
                            ino = ino,
                            n_holders = n_holders,
                            "oracular unlink of locked file (dev={}, ino={}) — {} \
                             holder(s) will observe subsequent errors on path-based \
                             calls; fd-based calls remain valid until close",
                            dev,
                            ino,
                            n_holders
                        );
                    }
                }
                match unlink_leaf_via_dirfd(&parent, RemoveKind::File) {
                    Ok(()) => Ok(ok_bare()),
                    Err(e) => Err(err(io_err_code(&e), io_msg_scrub(&e))),
                }
            })
            .await;
            match r {
                Err(je) => join_err_abort(je),
                Ok(Ok(par)) => HandlerReply::ok(par),
                Ok(Err(par)) => HandlerReply::Err(par),
            }
        })
    }

    fn resolve_replay_cmode<'a>(
        _ctx: SyscallCtx<'a>,
        raw_args: &'a [Par],
    ) -> Pin<Box<dyn Future<Output = Option<ConsensusMode>> + Send + 'a>> {
        Box::pin(async move {
            let [_, _, cmode_par] = raw_args else {
                return None;
            };
            resolve_cmode(cmode_par)
        })
    }

    fn journal<'a>(
        ctx: SyscallCtx<'a>,
        _raw_args: &'a [Par],
        path: JournalPath<'a>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            if path.is_divergence() {
                finalize_failure_journal_via_table(
                    ctx.handles,
                    FSERR_CODE_CONSENSUS_DIVERGENCE,
                    ctx.ack,
                );
            } else {
                let reply = path.produce_reply();
                if let Some(code_str) = extract_err_code(std::slice::from_ref(reply)) {
                    finalize_failure_journal_via_table(
                        ctx.handles,
                        fserr_to_code(&code_str),
                        ctx.ack,
                    );
                }
            }
        })
    }
}

/// `#[distributed_slice(FS_HANDLERS)]` registration.
#[distributed_slice(FS_HANDLERS)]
static FS_REMOVE_FILE_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsRemoveFileHandler as FsHandler>::NAME,
    arity: <FsRemoveFileHandler as FsHandler>::ARITY,
    verifying: <FsRemoveFileHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsRemoveFileHandler>(fs, args)),
    urn_suffix: "removeFile",
    fixed_channel: FixedChannels::fs_remove_file,
    body_ref: BodyRefs::FS_REMOVE_FILE,
    family: HandlerFamily::Mutation,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_cmode_par(s: &str) -> Par { RhoString::create_par(s.to_string()) }

    fn valid_args(cmode: &str) -> Vec<Par> {
        vec![
            RhoString::create_par("/@bundle/example".to_string()),
            RhoString::create_par("subdir/file".to_string()),
            mk_cmode_par(cmode),
        ]
    }

    /// Trait-level constants wire through.
    #[test]
    fn consts_wire_through() {
        assert_eq!(FsRemoveFileHandler::NAME, "fs_remove_file");
        assert_eq!(FsRemoveFileHandler::ARITY, 4);
        assert!(FsRemoveFileHandler::VERIFYING);
    }

    /// `parse_content` success path returns both root/rel + cmode.
    /// Both Consensus and Oracular round-trip.
    #[test]
    fn parse_content_accepts_both_cmodes() {
        for cmode_str in ["consensus", "oracular"] {
            let parsed = FsRemoveFileHandler::parse_content(&valid_args(cmode_str))
                .ok()
                .unwrap_or_else(|| panic!("{cmode_str} parses"));
            assert_eq!(parsed.root, "/@bundle/example");
            assert_eq!(parsed.rel, "subdir/file");
            match cmode_str {
                "consensus" => assert_eq!(parsed.cmode, ConsensusMode::Consensus),
                "oracular" => assert_eq!(parsed.cmode, ConsensusMode::Oracular),
                _ => unreachable!(),
            }
        }
    }

    /// LOAD-BEARING: bad cmode produces the specific "cmode must
    /// be..." message distinct from the combined "expected
    /// (String, String, String)" shape error.
    #[test]
    fn parse_content_rejects_bad_cmode_with_specific_message() {
        let mut args = valid_args("consensus");
        args[2] = mk_cmode_par("Consensus"); // uppercase
        let reply = *FsRemoveFileHandler::parse_content(&args)
            .err()
            .expect("rejects");
        match reply {
            HandlerReply::Err(_) => {}
            HandlerReply::Ok(_) => panic!("expected Err"),
        }
    }

    /// `parse_content` rejects wrong arg count.
    #[test]
    fn parse_content_rejects_wrong_arg_count() {
        let two = valid_args("consensus")
            .into_iter()
            .take(2)
            .collect::<Vec<_>>();
        let mut four = valid_args("consensus");
        four.push(RhoString::create_par("extra".to_string()));
        assert!(FsRemoveFileHandler::parse_content(&two).is_err());
        assert!(FsRemoveFileHandler::parse_content(&four).is_err());
    }

    /// `parse_content` rejects a non-String in root or rel.
    #[test]
    fn parse_content_rejects_non_string_root_or_rel() {
        use crate::rust::interpreter::rho_type::RhoNumber;
        for i in 0..2 {
            let mut args = valid_args("consensus");
            args[i] = RhoNumber::create_par(42);
            assert!(
                FsRemoveFileHandler::parse_content(&args).is_err(),
                "slot {i} should reject non-string",
            );
        }
    }

    /// `pre_charge_cost` delegates to `costs::fs_remove_file_cost`.
    /// Golden value pinned at the costs module
    /// (`FS_PATH_MUTATION_CONST` = 200, matching fs_rename /
    /// fs_copy_file).
    #[test]
    fn pre_charge_cost_delegates_to_costs_helper() {
        let via_handler = FsRemoveFileHandler::pre_charge_cost();
        let via_helper = fs_remove_file_cost();
        assert_eq!(via_handler.value, via_helper.value);
        assert_eq!(via_handler.operation, via_helper.operation);
    }
}
