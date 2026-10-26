// fs_copy_file — (fromRoot, fromRel, toRoot, toRel, cmode)
//                → [true, nBytes]
//                | [false, FSERR, msg]
//
// Sixth mutation handler and second two-endpoint path mutation
// (after fs_rename, PR #634).  Verifying, constant cost
// (`FS_PATH_MUTATION_CONST` = 200).  Byte-count reply via `ok_u64`
// — distinguishes this from fs_rename's bare-ok reply shape.
//
// # Reserve + finalize (H-6 pattern)
//
//   - `pre_syscall`: reserve a `WalOp::CopyFile` entry carrying
//     `from_canon` in `path` and `to_canon` in `extra_path`.
//     Same lexical-quarantine skip as fs_chmod / fs_rename.
//   - `dispatch`: resolve BOTH logical roots → `safe_open_verified`
//     on source (`O_RDONLY`) and dest (`O_WRONLY | O_CREAT |
//     O_TRUNC`, mode `0o644`) → `std::io::copy(&mut src, &mut dst)`
//     → `ok_u64(bytes_copied)`.
//   - `journal`: standard H-6 finalize — same shape as fs_rename.
//
// # Why safe_open_verified on BOTH endpoints
//
// Previous slices used `safe_descend_verified` + per-syscall
// `unlinkat` / `fchmodat` / `renameat` to keep the dirfd across
// the entire operation.  Copy-file is different: the actual
// copy loop (`std::io::copy`) runs through Rust's `File` API
// which needs owned `std::fs::File` handles.  `safe_open_verified`
// gives us that — it opens with `O_NOFOLLOW | O_CLOEXEC` forced
// so the H-5 defense stays intact for both endpoints.
//
// # Create-truncate on destination
//
// `O_CREAT | O_TRUNC` with mode `0o644` matches fileio pre-trait
// behavior: a copy to an existing path TRUNCATES then overwrites;
// a copy to a nonexistent path CREATES with 0o644.  Rholang
// callers that need different semantics (e.g., fail-if-exists)
// should open the destination explicitly via `fs_open("w+x", …)`
// and pass fds — but copy_file's happy path is the "copy to
// a staging path" workflow where create-or-truncate is the
// expected semantic.
//
// # No telemetry hazard
//
// Follows the `Result<Par, Par>` pattern from fs_chmod / fs_rename
// — raw `tokio::task::spawn_blocking` + outer-match discriminate
// into `HandlerReply::ok` / `HandlerReply::Err`.
// `clippy::result_large_err` allowed with the same rationale as
// fs_chmod (boxing the inline-Par Err adds an alloc per failed
// syscall without improving hazard-avoidance semantics).
//
// # Shape-A root gating (deferred)
//
// Uses the ungated `resolve_or_identity` for both endpoints —
// same deferral as fs_chmod / fs_rename / fs_chown / fs_remove_file.
// Coordinated migration when `resolve_or_identity_gated_for_consensus`
// lands.
//
// # std::io::copy under spawn_blocking
//
// `std::io::copy` reads source + writes destination in a loop
// (8 KiB chunks per the stdlib default).  Running under
// `spawn_blocking` keeps the tokio reactor free during what can
// be a long-running syscall sequence.  Partial-copy on error
// returns the bytes-written-so-far; we surface the error with
// `io_err_code` + `io_msg_scrub` rather than returning the
// partial count — matches fileio pre-trait behavior.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use linkme::distributed_slice;
use models::rhoapi::Par;
use tokio::task::spawn_blocking;

use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::io::costs::fs_copy_file_cost;
use crate::rust::interpreter::io::errors::{
    fserr_to_code, io_err_code, join_err_abort, FSERR_BAD_ARG, FSERR_CODE_CONSENSUS_DIVERGENCE,
    FSERR_QUOTA_EXCEEDED,
};
use crate::rust::interpreter::io::handler_trait::{
    dispatch_via_trait_owned, FsHandler, FsHandlerEntry, HandlerFamily, HandlerReply, JournalPath,
    SyscallCtx, FS_HANDLERS,
};
use crate::rust::interpreter::io::handlers::helpers::{
    finalize_failure_journal_via_table, journal_path_mutation_two_via_table,
};
use crate::rust::interpreter::io::path::open::safe_open_verified;
use crate::rust::interpreter::io::path::{
    canonicalize_lexical, io_msg_scrub, quarantine_err_reply,
};
use crate::rust::interpreter::io::response::{err, extract_err_code, ok_u64};
use crate::rust::interpreter::io::wal::WalOp;
use crate::rust::interpreter::io::{resolve_cmode, ConsensusMode};
use crate::rust::interpreter::rho_type::RhoString;
use crate::rust::interpreter::system_processes::{BodyRefs, FixedChannels};

/// Zero-sized handler type.
pub struct FsCopyFileHandler;

/// Parsed args.  Same shape as fs_rename — two endpoints + cmode.
pub struct FsCopyFileArgs {
    from_root: String,
    from_rel: String,
    to_root: String,
    to_rel: String,
    cmode: ConsensusMode,
}

impl FsHandler for FsCopyFileHandler {
    const NAME: &'static str = "fs_copy_file";
    const ARITY: usize = 6; // (fromRoot, fromRel, toRoot, toRel, cmode, ack)
    const VERIFYING: bool = true;

    type Args = FsCopyFileArgs;

    fn parse_content(args: &[Par]) -> Result<FsCopyFileArgs, Box<HandlerReply>> {
        let [from_root_par, from_rel_par, to_root_par, to_rel_par, cmode_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected 4 String args + cmode",
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
        match (
            RhoString::unapply(from_root_par),
            RhoString::unapply(from_rel_par),
            RhoString::unapply(to_root_par),
            RhoString::unapply(to_rel_par),
        ) {
            (Some(from_root), Some(from_rel), Some(to_root), Some(to_rel)) => Ok(FsCopyFileArgs {
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

    fn pre_charge_cost() -> Cost { fs_copy_file_cost() }

    fn pre_syscall<'a>(
        ctx: SyscallCtx<'a>,
        args: &'a FsCopyFileArgs,
    ) -> Pin<Box<dyn Future<Output = Result<(), Box<HandlerReply>>> + Send + 'a>> {
        Box::pin(async move {
            // Deterministic lexical quarantine on EITHER endpoint
            // → skip reserve (same discipline as fs_rename).
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
                WalOp::CopyFile,
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
        args: FsCopyFileArgs,
    ) -> Pin<Box<dyn Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            // BACKLOG: coordinated migration with the other seven
            // path-based handlers waiting on
            // resolve_or_identity_gated_for_consensus.
            let from_logical = PathBuf::from(&args.from_root);
            let (from_root_pb, from_expected_id) =
                ctx.handles.root_registry.resolve_or_identity(&from_logical);
            let to_logical = PathBuf::from(&args.to_root);
            let (to_root_pb, to_expected_id) =
                ctx.handles.root_registry.resolve_or_identity(&to_logical);
            let from_rel = args.from_rel;
            let to_rel = args.to_rel;
            #[allow(clippy::result_large_err)]
            let r = spawn_blocking(move || -> Result<Par, Par> {
                let mut src = match safe_open_verified(
                    &from_root_pb,
                    &from_rel,
                    libc::O_RDONLY,
                    0,
                    from_expected_id,
                ) {
                    Ok(f) => f,
                    Err(qe) => {
                        let (c, m) = quarantine_err_reply(&qe);
                        return Err(err(c, m));
                    }
                };
                let mut dst = match safe_open_verified(
                    &to_root_pb,
                    &to_rel,
                    libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC,
                    0o644,
                    to_expected_id,
                ) {
                    Ok(f) => f,
                    Err(qe) => {
                        let (c, m) = quarantine_err_reply(&qe);
                        return Err(err(c, m));
                    }
                };
                match std::io::copy(&mut src, &mut dst) {
                    Ok(n) => Ok(ok_u64(n)),
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
        // Cmode is the fifth positional arg (after from{Root,Rel}
        // + to{Root,Rel}).  Standard arg-based pattern.
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
        // Same H-6 finalize shape as fs_rename.
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
static FS_COPY_FILE_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsCopyFileHandler as FsHandler>::NAME,
    arity: <FsCopyFileHandler as FsHandler>::ARITY,
    verifying: <FsCopyFileHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsCopyFileHandler>(fs, args)),
    urn_suffix: "copyFile",
    fixed_channel: FixedChannels::fs_copy_file,
    body_ref: BodyRefs::FS_COPY_FILE,
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

    /// Trait-level constants wire through.  Six positional args
    /// (two endpoints + cmode + ack); verifying.
    #[test]
    fn consts_wire_through() {
        assert_eq!(FsCopyFileHandler::NAME, "fs_copy_file");
        assert_eq!(FsCopyFileHandler::ARITY, 6);
        assert!(FsCopyFileHandler::VERIFYING);
    }

    /// `parse_content` success path returns both endpoints + cmode.
    #[test]
    fn parse_content_accepts_valid_tuple() {
        let parsed = FsCopyFileHandler::parse_content(&valid_args())
            .ok()
            .expect("valid tuple parses");
        assert_eq!(parsed.from_root, "/@bundle/example");
        assert_eq!(parsed.from_rel, "src/file");
        assert_eq!(parsed.to_root, "/@bundle/example");
        assert_eq!(parsed.to_rel, "dst/file");
        assert_eq!(parsed.cmode, ConsensusMode::Consensus);
    }

    /// Cross-root copy parses (no root-equality constraint at
    /// parse — unlike fs_rename which can hit EXDEV, copy_file
    /// crosses devices by construction).  Mirrors fs_rename's
    /// `parse_content_accepts_cross_root_rename`.
    #[test]
    fn parse_content_accepts_cross_root_copy() {
        let args = vec![
            RhoString::create_par("/@bundle/a".to_string()),
            RhoString::create_par("f".to_string()),
            RhoString::create_par("/@bundle/b".to_string()),
            RhoString::create_par("f".to_string()),
            mk_cmode_par("consensus"),
        ];
        let parsed = FsCopyFileHandler::parse_content(&args)
            .ok()
            .expect("cross-root parses");
        assert_eq!(parsed.from_root, "/@bundle/a");
        assert_eq!(parsed.to_root, "/@bundle/b");
    }

    /// LOAD-BEARING: bad cmode produces specific "cmode must
    /// be..." error.
    #[test]
    fn parse_content_rejects_bad_cmode_with_specific_message() {
        let mut args = valid_args();
        args[4] = mk_cmode_par("Consensus"); // uppercase
        let reply = *FsCopyFileHandler::parse_content(&args)
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
        let four = valid_args().into_iter().take(4).collect::<Vec<_>>();
        let mut six = valid_args();
        six.push(RhoString::create_par("extra".to_string()));
        assert!(FsCopyFileHandler::parse_content(&four).is_err());
        assert!(FsCopyFileHandler::parse_content(&six).is_err());
    }

    /// `parse_content` rejects a non-String in any of the 4 path
    /// positions.
    #[test]
    fn parse_content_rejects_non_string_in_any_path_position() {
        use crate::rust::interpreter::rho_type::RhoNumber;
        for i in 0..4 {
            let mut args = valid_args();
            args[i] = RhoNumber::create_par(42);
            assert!(
                FsCopyFileHandler::parse_content(&args).is_err(),
                "slot {i} should reject non-string",
            );
        }
    }

    /// `pre_charge_cost` delegates to `costs::fs_copy_file_cost`.
    /// Golden value pinned at the costs module
    /// (`FS_PATH_MUTATION_CONST` = 200, matching fs_rename /
    /// fs_remove_file).
    #[test]
    fn pre_charge_cost_delegates_to_costs_helper() {
        let via_handler = FsCopyFileHandler::pre_charge_cost();
        let via_helper = fs_copy_file_cost();
        assert_eq!(via_handler.value, via_helper.value);
        assert_eq!(via_handler.operation, via_helper.operation);
    }
}
