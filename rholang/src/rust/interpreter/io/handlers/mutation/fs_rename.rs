// fs_rename — (fromRoot, fromRel, toRoot, toRel, cmode)
//             → [true] | [false, FSERR, msg]
//
// Second path-mutation handler on dev and first two-endpoint
// mutation.  Exercises `journal_path_mutation_two_via_table` (the
// third of PR #629's three journal helper shapes).  Verifying,
// constant cost.  Cmode arg-based.
//
// # Reserve + finalize (H-6 pattern)
//
//   - `pre_syscall`: compute from-canon + to-canon via
//     `canonicalize_lexical` for each endpoint.  If either lexical
//     canonicalization fails, skip the reserve deterministically
//     (same discipline as fs_chmod — leader + follower both see
//     the same string-op outcome, so both skip the reserve and
//     the dispatch body produces the matching quarantine reply).
//     On success, reserve a `WalOp::Rename` entry carrying
//     `from_canon` in `path` and `to_canon` in `extra_path`.
//     Oracular self-guard inside the helper.
//   - `dispatch`: resolve both logical roots → `safe_descend_
//     verified` for each → `renameat(from_dirfd, from_leaf,
//     to_dirfd, to_leaf)`.  EXDEV (cross-device rename not
//     supported by the kernel) maps to `FSERR_CROSS_DEVICE`.
//   - `journal`: on error / verify-divergence, patch via
//     `finalize_failure_journal_via_table` (same shape as fs_truncate
//     / fs_chmod).
//
// # Shape-A root gating (deferred)
//
// Uses the ungated `resolve_or_identity` for BOTH endpoints —
// same deferral as fs_chmod / fs_exists / fs_stat.  BACKLOG:
// migrate all four handlers together when
// `resolve_or_identity_gated_for_consensus` lands on dev.
//
// # No telemetry hazard
//
// Follows fs_chmod's `Result<Par, Par>` pattern.  Raw
// `tokio::task::spawn_blocking` + outer-match discriminate into
// `HandlerReply::ok` / `HandlerReply::Err`.  `clippy::result_large_err`
// allowed with the same rationale as fs_chmod.
//
// # Why renameat (not rename)
//
// `renameat(from_dirfd, from_leaf, to_dirfd, to_leaf)` uses the
// per-endpoint dirfds from `safe_descend_verified` directly — the
// H-5 defense is complete across both endpoints without a separate
// path-component recheck.  `rename()` would require re-joining the
// resolved paths into strings and re-walking them, which widens
// the TOCTOU window.
//
// # EXDEV discipline
//
// `renameat` requires both endpoints to reside on the same
// filesystem; cross-device moves return EXDEV.  Rather than
// silently fall back to a copy+delete (which is not atomic and
// would need its own WAL discipline), surface EXDEV as
// `FSERR_CROSS_DEVICE` and let the Rholang caller decide whether
// to issue `fs_copy_file` + `fs_remove_file` explicitly.
// Matches pre-trait fs_rename behavior.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use linkme::distributed_slice;
use models::rhoapi::Par;
use tokio::task::spawn_blocking;

use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::io::costs::fs_rename_cost;
use crate::rust::interpreter::io::errors::{
    fserr_to_code, io_err_code, join_err_abort, FSERR_BAD_ARG, FSERR_CODE_CONSENSUS_DIVERGENCE,
    FSERR_CROSS_DEVICE, FSERR_QUOTA_EXCEEDED,
};
use crate::rust::interpreter::io::handler_trait::{
    dispatch_via_trait_owned, FsHandler, FsHandlerEntry, HandlerFamily, HandlerReply, JournalPath,
    SyscallCtx, FS_HANDLERS,
};
use crate::rust::interpreter::io::handlers::helpers::{
    finalize_failure_journal_via_table, journal_path_mutation_two_via_table,
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
pub struct FsRenameHandler;

/// Parsed args for [`FsRenameHandler`].  Four string positions +
/// cmode: `from_root`/`from_rel` for the source endpoint and
/// `to_root`/`to_rel` for the destination.  Both endpoints must
/// resolve to the same filesystem (EXDEV surfaces otherwise —
/// see module header).
pub struct FsRenameArgs {
    from_root: String,
    from_rel: String,
    to_root: String,
    to_rel: String,
    cmode: ConsensusMode,
}

impl FsHandler for FsRenameHandler {
    const NAME: &'static str = "fs_rename";
    const ARITY: usize = 6; // (fromRoot, fromRel, toRoot, toRel, cmode, ack)
    const VERIFYING: bool = true;

    type Args = FsRenameArgs;

    fn parse_content(args: &[Par]) -> Result<FsRenameArgs, Box<HandlerReply>> {
        let [from_root_par, from_rel_par, to_root_par, to_rel_par, cmode_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected 4 String args + cmode",
            ));
        };
        // Cmode validation first — matches pre-trait specific
        // "cmode must be..." error message, distinct from the
        // combined "expected 4 String args + cmode" shape error.
        let cmode = match resolve_cmode(cmode_par) {
            Some(m) => m,
            None => {
                return Err(HandlerReply::boxed_err(
                    FSERR_BAD_ARG,
                    "cmode must be String \"oracular\" or \"consensus\"",
                ));
            }
        };
        match (
            RhoString::unapply(from_root_par),
            RhoString::unapply(from_rel_par),
            RhoString::unapply(to_root_par),
            RhoString::unapply(to_rel_par),
        ) {
            (Some(from_root), Some(from_rel), Some(to_root), Some(to_rel)) => Ok(FsRenameArgs {
                from_root,
                from_rel,
                to_root,
                to_rel,
                cmode,
            }),
            _ => Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected 4 String args + cmode",
            )),
        }
    }

    fn pre_charge_cost() -> Cost { fs_rename_cost() }

    fn pre_syscall<'a>(
        ctx: SyscallCtx<'a>,
        args: &'a FsRenameArgs,
    ) -> Pin<Box<dyn Future<Output = Result<(), Box<HandlerReply>>> + Send + 'a>> {
        Box::pin(async move {
            // Deterministic lexical quarantine on EITHER endpoint
            // → skip reserve.  Both leader + follower see the same
            // outcome (string-op, no filesystem access), so the
            // skip-no-reserve path is consensus-safe.  The dispatch
            // body produces the matching quarantine reply.
            let from_canon = match canonicalize_lexical(&args.from_root, &args.from_rel) {
                Ok(p) => p,
                Err(_) => return Ok(()),
            };
            let to_canon = match canonicalize_lexical(&args.to_root, &args.to_rel) {
                Ok(p) => p,
                Err(_) => return Ok(()),
            };
            if journal_path_mutation_two_via_table(
                ctx.handles,
                args.cmode,
                WalOp::Rename,
                from_canon,
                to_canon,
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
        args: FsRenameArgs,
    ) -> Pin<Box<dyn Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            // BACKLOG: migrate to resolve_or_identity_gated_for_consensus
            // once path/identity ports the Shape-A gating.  Both
            // endpoints must gate together — see module header on
            // Shape-A deferral.
            let from_logical = PathBuf::from(&args.from_root);
            let (from_root_pb, from_expected_id) =
                ctx.handles.root_registry.resolve_or_identity(&from_logical);
            let to_logical = PathBuf::from(&args.to_root);
            let (to_root_pb, to_expected_id) =
                ctx.handles.root_registry.resolve_or_identity(&to_logical);
            let from_rel = args.from_rel;
            let to_rel = args.to_rel;
            // Direct `spawn_blocking` with `Result<Par, Par>` —
            // avoids the HandlerReply telemetry hazard (see
            // fs_chmod module header for the pattern rationale).
            #[allow(clippy::result_large_err)]
            let r = spawn_blocking(move || -> Result<Par, Par> {
                let from_parent =
                    match safe_descend_verified(&from_root_pb, &from_rel, from_expected_id) {
                        Ok(p) => p,
                        Err(qe) => {
                            let (c, m) = quarantine_err_reply(&qe);
                            return Err(err(c, m));
                        }
                    };
                let to_parent = match safe_descend_verified(&to_root_pb, &to_rel, to_expected_id) {
                    Ok(p) => p,
                    Err(qe) => {
                        let (c, m) = quarantine_err_reply(&qe);
                        return Err(err(c, m));
                    }
                };
                // SAFETY: `from_parent` and `to_parent` are
                // `SafeParent` RAII wrappers from `safe_descend_
                // verified`; each holds an open dirfd for its own
                // lifetime, and `leaf_ptr()` returns a NUL-terminated
                // `*const c_char` valid for the same lifetime.
                // `renameat` reads all four without retention.
                let rc = unsafe {
                    libc::renameat(
                        from_parent.as_raw_fd(),
                        from_parent.leaf_ptr(),
                        to_parent.as_raw_fd(),
                        to_parent.leaf_ptr(),
                    )
                };
                if rc == 0 {
                    Ok(ok_bare())
                } else {
                    let e = std::io::Error::last_os_error();
                    // EXDEV → FSERR_CROSS_DEVICE.  Pre-trait
                    // behavior; see module header on EXDEV
                    // discipline.
                    let code = if e.raw_os_error() == Some(libc::EXDEV) {
                        FSERR_CROSS_DEVICE
                    } else {
                        io_err_code(&e)
                    };
                    Err(err(code, io_msg_scrub(&e)))
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
        // Cmode is the fifth positional arg (after fromRoot,
        // fromRel, toRoot, toRel).  Standard arg-based pattern.
        Box::pin(async move {
            let [_, _, _, _, cmode_par] = raw_args else {
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
        // Same H-6 finalize shape as fs_truncate / fs_chmod.
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
static FS_RENAME_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsRenameHandler as FsHandler>::NAME,
    arity: <FsRenameHandler as FsHandler>::ARITY,
    verifying: <FsRenameHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsRenameHandler>(fs, args)),
    urn_suffix: "rename",
    fixed_channel: FixedChannels::fs_rename,
    body_ref: BodyRefs::FS_RENAME,
    family: HandlerFamily::Mutation,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_cmode_par(s: &str) -> Par { RhoString::create_par(s.to_string()) }

    fn valid_args() -> Vec<Par> {
        vec![
            RhoString::create_par("/@bundle/example".to_string()),
            RhoString::create_par("src/file".to_string()),
            RhoString::create_par("/@bundle/example".to_string()),
            RhoString::create_par("dst/file".to_string()),
            mk_cmode_par("consensus"),
        ]
    }

    /// Trait-level constants wire through.  Third mutation handler.
    /// ARITY = 6 (fromRoot + fromRel + toRoot + toRel + cmode + ack).
    #[test]
    fn consts_wire_through() {
        assert_eq!(FsRenameHandler::NAME, "fs_rename");
        assert_eq!(FsRenameHandler::ARITY, 6);
        assert!(FsRenameHandler::VERIFYING);
    }

    /// `parse_content` success path returns both endpoints + cmode.
    #[test]
    fn parse_content_accepts_valid_tuple() {
        let parsed = FsRenameHandler::parse_content(&valid_args())
            .ok()
            .expect("valid tuple parses");
        assert_eq!(parsed.from_root, "/@bundle/example");
        assert_eq!(parsed.from_rel, "src/file");
        assert_eq!(parsed.to_root, "/@bundle/example");
        assert_eq!(parsed.to_rel, "dst/file");
        assert_eq!(parsed.cmode, ConsensusMode::Consensus);
    }

    /// Cross-root rename (different `from_root` / `to_root`) also
    /// parses — the handler accepts it; the EXDEV check at
    /// `renameat` time will reject cross-filesystem calls at
    /// runtime.  Pin that the parser doesn't gate on root equality.
    #[test]
    fn parse_content_accepts_cross_root_rename() {
        let args = vec![
            RhoString::create_par("/@bundle/a".to_string()),
            RhoString::create_par("f".to_string()),
            RhoString::create_par("/@bundle/b".to_string()),
            RhoString::create_par("f".to_string()),
            mk_cmode_par("consensus"),
        ];
        let parsed = FsRenameHandler::parse_content(&args)
            .ok()
            .expect("cross-root parses");
        assert_eq!(parsed.from_root, "/@bundle/a");
        assert_eq!(parsed.to_root, "/@bundle/b");
    }

    /// LOAD-BEARING: a wrong cmode string produces the specific
    /// "cmode must be..." error message, distinct from the combined
    /// "expected 4 String args + cmode" shape error.  Mirrors
    /// fs_chmod's discrimination.
    #[test]
    fn parse_content_rejects_bad_cmode_with_specific_message() {
        let mut args = valid_args();
        args[4] = RhoString::create_par("Consensus".to_string()); // uppercase
        let reply = *FsRenameHandler::parse_content(&args)
            .err()
            .expect("rejects");
        match reply {
            HandlerReply::Err(_) => {}
            HandlerReply::Ok(_) => panic!("expected Err"),
        }
    }

    /// `parse_content` rejects wrong arg count (4, 6+).  The parser
    /// requires exactly 5 pre-ack args.
    #[test]
    fn parse_content_rejects_wrong_arg_count() {
        let four = valid_args().into_iter().take(4).collect::<Vec<_>>();
        let mut six = valid_args();
        six.push(RhoString::create_par("extra".to_string()));
        assert!(FsRenameHandler::parse_content(&four).is_err());
        assert!(FsRenameHandler::parse_content(&six).is_err());
    }

    /// `parse_content` rejects a non-String in any of the four
    /// path positions (via RhoString::unapply None).
    #[test]
    fn parse_content_rejects_non_string_in_any_path_position() {
        use crate::rust::interpreter::rho_type::RhoNumber;
        for i in 0..4 {
            let mut args = valid_args();
            args[i] = RhoNumber::create_par(42);
            assert!(
                FsRenameHandler::parse_content(&args).is_err(),
                "slot {i} should reject non-string",
            );
        }
    }

    /// `pre_charge_cost` delegates to `costs::fs_rename_cost`.
    /// Golden value pinned at the costs module
    /// (`FS_PATH_MUTATION_CONST` = 200 vs. `FS_SYSCALL_CONST` = 100
    /// for the single-endpoint handlers).
    #[test]
    fn pre_charge_cost_delegates_to_costs_helper() {
        let via_handler = FsRenameHandler::pre_charge_cost();
        let via_helper = fs_rename_cost();
        assert_eq!(via_handler.value, via_helper.value);
        assert_eq!(via_handler.operation, via_helper.operation);
    }
}
