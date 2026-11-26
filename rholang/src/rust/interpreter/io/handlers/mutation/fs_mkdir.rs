// fs_mkdir — (root, rel, mode, parents, cmode) -> [true] | [false, FSERR, msg]
//
// Directory creation handler.  Verifying mutation, constant cost
// (`FS_PATH_MUTATION_CONST` = 200).  Cmode arg-based.
//
// # `parents` semantics (mirrors Node.js `fs.mkdir({recursive})`)
//
//   - `parents = false`: strict mkdir of the leaf.  Missing
//     intermediate parent → `FSERR_NOT_FOUND`.  Pre-existing leaf →
//     `FSERR_ALREADY_EXISTS`.  Mode is applied atomically via
//     `mkdirat` (POSIX `S_IRWXU & ~umask` semantics modified by the
//     explicit mode argument).
//   - `parents = true`: idempotent, create intermediates.  Pre-
//     existing directory → success without mode change.  Falls back
//     to `std::fs::create_dir_all` for the recursive create; the
//     explicit mode is applied via `chmod` on the leaf only (Node
//     semantics — intermediates get umask-adjusted default perms).
//
// # TOCTOU note
//
// `parents = false` uses `safe_descend_verified` + `mkdirat` so the
// H-5 identity defense + the H-6 TOCTOU-immune leaf write both hold.
// `parents = true` uses `create_dir_all` under the (already-
// identity-verified) `root_pb`, trading a tightly-bounded TOCTOU
// window for the ergonomics of the recursive create.  The window is
// bounded by `resolve_or_identity_gated_for_consensus`'s dev/inode
// pinning on the root; an attacker who swapped a directory under
// root between the identity check and `create_dir_all` would hit the
// trusted-root policy anyway (the dir fd chain stays within the
// known root).
//
// # Reserve + finalize (H-6 pattern)
//
//   - `pre_syscall`: reserve `WalOp::Mkdir` with `mode_bits =
//     Some(mode)` and `length` encoding the `parents` flag
//     (`Some(1)` ↔ true, `None` ↔ false).  Lexical quarantine
//     fail-skip matches fs_chmod / fs_rename.
//   - `dispatch`: `resolve_or_identity_gated_for_consensus` + the
//     per-branch mkdir body.
//   - `journal`: standard H-6 finalize — same shape as fs_chmod.

use std::fs::Permissions;
use std::future::Future;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::pin::Pin;

use linkme::distributed_slice;
use models::rhoapi::Par;
use tokio::task::spawn_blocking;

use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::io::costs::fs_mkdir_cost;
use crate::rust::interpreter::io::errors::{
    fserr_to_code, io_err_code, join_err_abort, FSERR_BAD_ARG, FSERR_CODE_CONSENSUS_DIVERGENCE,
    FSERR_QUARANTINE, FSERR_QUOTA_EXCEEDED,
};
use crate::rust::interpreter::io::handler_trait::{
    dispatch_via_trait_owned, FsHandler, FsHandlerEntry, HandlerFamily, HandlerReply, JournalPath,
    SyscallCtx, FS_HANDLERS,
};
use crate::rust::interpreter::io::handlers::helpers::{
    finalize_failure_journal_via_table, journal_mkdir_via_table,
};
use crate::rust::interpreter::io::path::descend::safe_descend_verified;
use crate::rust::interpreter::io::path::{
    canonicalize_lexical, io_msg_scrub, quarantine_err_reply,
};
use crate::rust::interpreter::io::response::{err, extract_err_code, ok_bare};
use crate::rust::interpreter::io::{resolve_cmode, ConsensusMode};
use crate::rust::interpreter::rho_type::{RhoBoolean, RhoNumber, RhoString};
use crate::rust::interpreter::system_processes::{BodyRefs, FixedChannels};

/// Zero-sized handler type.
pub struct FsMkdirHandler;

/// Parsed args.
pub struct FsMkdirArgs {
    root: String,
    rel: String,
    mode: u32,
    parents: bool,
    cmode: ConsensusMode,
}

impl FsHandler for FsMkdirHandler {
    const NAME: &'static str = "fs_mkdir";
    const ARITY: usize = 6; // (root, rel, mode, parents, cmode, ack)
    const VERIFYING: bool = true;

    type Args = FsMkdirArgs;

    fn parse_content(args: &[Par]) -> Result<FsMkdirArgs, Box<HandlerReply>> {
        let [root_par, rel_par, mode_par, parents_par, cmode_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (String, String, Int, Bool, String)",
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
        let combined_err = || {
            HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (String, String, Int<=0o7777, Bool, String)",
            )
        };
        let root = RhoString::unapply(root_par).ok_or_else(combined_err)?;
        let rel = RhoString::unapply(rel_par).ok_or_else(combined_err)?;
        let mode = RhoNumber::unapply(mode_par).ok_or_else(combined_err)?;
        if !(0..=0o7777).contains(&mode) {
            return Err(combined_err());
        }
        let parents = RhoBoolean::unapply(parents_par).ok_or_else(combined_err)?;
        Ok(FsMkdirArgs {
            root,
            rel,
            mode: mode as u32,
            parents,
            cmode,
        })
    }

    fn pre_charge_cost() -> Cost { fs_mkdir_cost() }

    fn pre_syscall<'a>(
        ctx: SyscallCtx<'a>,
        args: &'a FsMkdirArgs,
    ) -> Pin<Box<dyn Future<Output = Result<(), Box<HandlerReply>>> + Send + 'a>> {
        Box::pin(async move {
            let canon_path = match canonicalize_lexical(&args.root, &args.rel) {
                Ok(p) => p,
                Err(_) => return Ok(()),
            };
            if journal_mkdir_via_table(
                ctx.handles,
                args.cmode,
                canon_path,
                args.mode,
                args.parents,
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
        args: FsMkdirArgs,
    ) -> Pin<Box<dyn Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            // Shape-A gating: Consensus-mode mkdir against an
            // unregistered `/@bundle/<X>` short-circuits with
            // FSERR_UNSUPPORTED before any filesystem syscall.
            let logical = PathBuf::from(&args.root);
            let (root_pb, expected_root_id) = match ctx
                .handles
                .root_registry
                .resolve_or_identity_gated_for_consensus(&logical, args.cmode)
            {
                Ok(v) => v,
                Err((c, m)) => return HandlerReply::err(c, m),
            };
            let rel = args.rel;
            let mode = args.mode;
            let parents = args.parents;
            #[allow(clippy::result_large_err)]
            let r = spawn_blocking(move || -> Result<Par, Par> {
                // Lexical pre-check so parents=true (which bypasses
                // safe_descend_verified) still rejects `..` / absolute
                // / symlink-trap intermediates at the string layer.
                for c in Path::new(&rel).components() {
                    match c {
                        Component::Normal(_) | Component::CurDir => {}
                        _ => {
                            return Err(err(FSERR_QUARANTINE, "rel contains .. or absolute path"));
                        }
                    }
                }
                if parents {
                    // Recursive create under the (identity-verified)
                    // host root.  The explicit mode is applied via
                    // chmod on the leaf only — Node.js semantics;
                    // intermediates get umask-default perms.
                    let full_path = root_pb.join(&rel);
                    match std::fs::create_dir_all(&full_path) {
                        Ok(()) => {
                            let perm = Permissions::from_mode(mode);
                            match std::fs::set_permissions(&full_path, perm) {
                                Ok(()) => Ok(ok_bare()),
                                Err(e) => Err(err(io_err_code(&e), io_msg_scrub(&e))),
                            }
                        }
                        Err(e) => Err(err(io_err_code(&e), io_msg_scrub(&e))),
                    }
                } else {
                    // Strict mkdir: parent must exist, leaf must not.
                    let parent = match safe_descend_verified(&root_pb, &rel, expected_root_id) {
                        Ok(p) => p,
                        Err(qe) => {
                            let (c, m) = quarantine_err_reply(&qe);
                            return Err(err(c, m));
                        }
                    };
                    // SAFETY: `parent` holds an open dirfd for the
                    // call duration; `leaf_ptr()` is NUL-terminated
                    // and valid for `&parent`.  `mkdirat` reads both
                    // without retention.
                    let rc = unsafe {
                        libc::mkdirat(parent.as_raw_fd(), parent.leaf_ptr(), mode as libc::mode_t)
                    };
                    if rc == 0 {
                        Ok(ok_bare())
                    } else {
                        let e = std::io::Error::last_os_error();
                        Err(err(io_err_code(&e), io_msg_scrub(&e)))
                    }
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
static FS_MKDIR_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsMkdirHandler as FsHandler>::NAME,
    arity: <FsMkdirHandler as FsHandler>::ARITY,
    verifying: <FsMkdirHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsMkdirHandler>(fs, args)),
    urn_suffix: "mkdir",
    fixed_channel: FixedChannels::fs_mkdir,
    body_ref: BodyRefs::FS_MKDIR,
    family: HandlerFamily::Mutation,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_cmode_par(s: &str) -> Par { RhoString::create_par(s.to_string()) }

    fn valid_args(cmode: &str, parents: bool) -> Vec<Par> {
        vec![
            RhoString::create_par("/@bundle/example".to_string()),
            RhoString::create_par("subdir/new".to_string()),
            RhoNumber::create_par(0o755),
            RhoBoolean::create_par(parents),
            mk_cmode_par(cmode),
        ]
    }

    #[test]
    fn consts_wire_through() {
        assert_eq!(FsMkdirHandler::NAME, "fs_mkdir");
        assert_eq!(FsMkdirHandler::ARITY, 6);
        assert!(FsMkdirHandler::VERIFYING);
    }

    #[test]
    fn parse_content_accepts_both_cmodes_and_both_parents() {
        for cmode_str in ["consensus", "oracular"] {
            for parents in [true, false] {
                let parsed = FsMkdirHandler::parse_content(&valid_args(cmode_str, parents))
                    .ok()
                    .unwrap_or_else(|| panic!("{cmode_str}/{parents} parses"));
                assert_eq!(parsed.root, "/@bundle/example");
                assert_eq!(parsed.rel, "subdir/new");
                assert_eq!(parsed.mode, 0o755);
                assert_eq!(parsed.parents, parents);
                match cmode_str {
                    "consensus" => assert_eq!(parsed.cmode, ConsensusMode::Consensus),
                    "oracular" => assert_eq!(parsed.cmode, ConsensusMode::Oracular),
                    _ => unreachable!(),
                }
            }
        }
    }

    #[test]
    fn parse_content_rejects_bad_cmode_with_specific_message() {
        let mut args = valid_args("consensus", true);
        args[4] = mk_cmode_par("Consensus");
        let reply = *FsMkdirHandler::parse_content(&args).err().expect("rejects");
        match reply {
            HandlerReply::Err(_) => {}
            HandlerReply::Ok(_) => panic!("expected Err"),
        }
    }

    #[test]
    fn parse_content_rejects_wrong_arg_count() {
        let four = valid_args("consensus", true)
            .into_iter()
            .take(4)
            .collect::<Vec<_>>();
        let mut six = valid_args("consensus", true);
        six.push(RhoString::create_par("extra".to_string()));
        assert!(FsMkdirHandler::parse_content(&four).is_err());
        assert!(FsMkdirHandler::parse_content(&six).is_err());
    }

    #[test]
    fn parse_content_rejects_out_of_range_mode() {
        for bad_mode in [-1, 0o10000, i64::MAX] {
            let mut args = valid_args("consensus", true);
            args[2] = RhoNumber::create_par(bad_mode);
            assert!(
                FsMkdirHandler::parse_content(&args).is_err(),
                "mode {bad_mode} should reject",
            );
        }
    }

    #[test]
    fn parse_content_rejects_non_bool_parents() {
        let mut args = valid_args("consensus", true);
        args[3] = RhoNumber::create_par(1); // Int, not Bool
        assert!(FsMkdirHandler::parse_content(&args).is_err());
    }

    #[test]
    fn pre_charge_cost_delegates_to_costs_helper() {
        let via_handler = FsMkdirHandler::pre_charge_cost();
        let via_helper = fs_mkdir_cost();
        assert_eq!(via_handler.value, via_helper.value);
        assert_eq!(via_handler.operation, via_helper.operation);
    }
}
