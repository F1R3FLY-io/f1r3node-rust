// Lifecycle family — file/cap creation, close, quarantine.
//
// 3 non-verifying handlers:
//   - fs_open: allocate a FileHandle via `open_impl_via_table` under
//     safe_open_verified.  Non-verifying but with an intricate
//     `on_replay_side_effect` that installs a shadow FileHandle at
//     the leader's cached fd — Phase-2 backs the shadow with a REAL
//     libc::open on Consensus + non-append modes so downstream fd-
//     based re-execute ops (fs_size / fs_read / etc.) run on the
//     follower's own subdir file.
//   - fs_close: pure fd-release + Phase-2 shadow-remove on replay.
//   - fs_quarantine: safe_descend_verified inside spawn_blocking;
//     echoes the caller-supplied joined path on success.
//
// Wave-3 S3.13b (2026-09-10) — moved out of `handlers.rs`.
//
// Family: `HandlerFamily::Lifecycle` (see handler_trait.rs).

use std::path::PathBuf;

use models::rhoapi::Par;
use tokio::task::spawn_blocking;

use super::super::rho_type::{RhoNumber, RhoString};
use super::super::system_processes::{BodyRefs, FixedChannels};
use super::errors::*;
use super::handle_table::FileHandle;
use super::handler_trait::{
    dispatch_via_trait_owned, FsHandler, FsHandlerEntry, HandlerFamily, HandlerReply, SyscallCtx,
    FS_HANDLERS,
};
use super::handlers::{open_impl_via_table, resolve_cmode, spawn_blocking_par};
use super::mode::{fopen_flags, parse_open_mode, AccessMode};
use super::path::{canonicalize_lexical, quarantine_err_reply, safe_descend_verified};
use super::response::{err, extract_ok_fd, ok_bare, ok_string};
use super::{costs, ConsensusMode};

// -------------------------------------------------------------------
// fs_close — (fd) -> [true]  (S3.2, 2026-09-08)
//
// Non-verifying but with a Phase-2 fd-release side effect on the
// `is_replay` branch.  Pre-Phase-2 the follower's shadow was
// metadata-only; Phase-2 (2026-09-01) backs it with a real OS fd,
// so failing to release on replay leaks OS fds.
// -------------------------------------------------------------------

pub struct FsCloseHandler;

pub struct FsCloseArgs {
    fd: u64,
}

impl FsHandler for FsCloseHandler {
    const NAME: &'static str = "fs_close";
    const ARITY: usize = 2; // (fd, ack)

    type Args = FsCloseArgs;

    fn parse_content(args: &[Par]) -> Result<FsCloseArgs, Box<HandlerReply>> {
        let [fd_par] = args else {
            return Err(HandlerReply::boxed_err(FSERR_BAD_ARG, "expected GInt fd"));
        };
        let fd = RhoNumber::unapply(fd_par)
            .ok_or_else(|| HandlerReply::boxed_err(FSERR_BAD_ARG, "expected GInt fd"))?;
        // Slice 28 (post-2026-08-06 CRIT-2 fix): fds are hash-
        // derived u64 bit-patterns; the sign bit carries
        // information, so reinterpret via `fd as u64` rather than
        // gating on `fd >= 0`.
        Ok(FsCloseArgs { fd: fd as u64 })
    }

    fn pre_charge_cost() -> crate::rust::interpreter::accounting::costs::Cost {
        costs::fs_close_cost()
    }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsCloseArgs,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            ctx.handles.remove(args.fd).await;
            HandlerReply::ok(ok_bare())
        })
    }

    fn on_replay_side_effect<'a>(
        ctx: SyscallCtx<'a>,
        raw_args: &'a [Par],
        _previous: &'a [Par],
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        // Phase-2 fd-release: opportunistically parse the fd_par
        // and remove the shadow.  Matches pre-migration behavior
        // — an unparseable fd_par on replay silently no-ops (the
        // leader's `previous` reply is still echoed by the
        // framework).  See handler_trait.rs § on_replay_side_effect
        // for why we don't parse-then-fail on replay.
        Box::pin(async move {
            if let [fd_par, ..] = raw_args {
                if let Some(fd) = RhoNumber::unapply(fd_par) {
                    ctx.handles.remove(fd as u64).await;
                }
            }
        })
    }
}

#[linkme::distributed_slice(FS_HANDLERS)]
static FS_CLOSE_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsCloseHandler as FsHandler>::NAME,
    arity: <FsCloseHandler as FsHandler>::ARITY,
    verifying: <FsCloseHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsCloseHandler>(fs, args)),
    urn_suffix: "close",
    fixed_channel: FixedChannels::fs_close,
    body_ref: BodyRefs::FS_CLOSE,
    family: HandlerFamily::Lifecycle,
};

// -------------------------------------------------------------------
// fs_quarantine — (rootCanon, rel) -> [true, canonPath]  (S3.3)
//
// Non-verifying lifecycle helper: safe_descend_verified inside a
// spawn_blocking task, echoes the caller-supplied joined path on
// success (no canonicalize call → no host drift).
// -------------------------------------------------------------------

pub struct FsQuarantineHandler;

pub struct FsQuarantineArgs {
    root: String,
    rel: String,
}

impl FsHandler for FsQuarantineHandler {
    const NAME: &'static str = "fs_quarantine";
    const ARITY: usize = 3; // (rootCanon, rel, ack)

    type Args = FsQuarantineArgs;

    fn parse_content(args: &[Par]) -> Result<FsQuarantineArgs, Box<HandlerReply>> {
        let [root_par, rel_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (String, String)",
            ));
        };
        match (RhoString::unapply(root_par), RhoString::unapply(rel_par)) {
            (Some(root), Some(rel)) => Ok(FsQuarantineArgs { root, rel }),
            _ => Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (String, String)",
            )),
        }
    }

    fn pre_charge_cost() -> crate::rust::interpreter::accounting::costs::Cost {
        costs::fs_quarantine_cost()
    }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsQuarantineArgs,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            let root_pb = PathBuf::from(&args.root);
            let (root_pb, expected_root_id) =
                ctx.handles.root_registry.resolve_or_identity(&root_pb);
            let rel = args.rel;
            let par = spawn_blocking_par(move || -> Par {
                match safe_descend_verified(&root_pb, &rel, expected_root_id) {
                    Ok(_) => ok_string(root_pb.join(&rel).to_string_lossy().into_owned()),
                    Err(qe) => {
                        let (c, m) = quarantine_err_reply(&qe);
                        err(c, m)
                    }
                }
            })
            .await;
            // spawn_blocking_par returns a Par directly — wrap into
            // HandlerReply based on the pre-refactor semantic
            // (success = ok_string, failure = err(...)).  Both are
            // Par-shaped; the framework doesn't need to distinguish
            // here, but for consistency the trait's Ok/Err split is
            // preserved by using `HandlerReply::Ok(par)` for all
            // spawn_blocking replies (the byte-level split lives
            // inside the returned Par).
            HandlerReply::Ok(par)
        })
    }
}

#[linkme::distributed_slice(FS_HANDLERS)]
static FS_QUARANTINE_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsQuarantineHandler as FsHandler>::NAME,
    arity: <FsQuarantineHandler as FsHandler>::ARITY,
    verifying: <FsQuarantineHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsQuarantineHandler>(fs, args)),
    urn_suffix: "quarantine",
    fixed_channel: FixedChannels::fs_quarantine,
    body_ref: BodyRefs::FS_QUARANTINE,
    family: HandlerFamily::Lifecycle,
};

// -------------------------------------------------------------------
// fs_open — (root, rel, mode, cmode) -> [true, fd]  (S3.10)
//
// NON-verifying lifecycle.  Most intricate handler (~300 LOC in
// pre-refactor).  is_replay side-effect installs a shadow
// FileHandle at the leader's fd so downstream mutating handlers
// (fs_write / fs_truncate / etc.) can look up (cmode, canon_path)
// symmetrically.  Under Consensus + non-append mode, the shadow's
// `file` slot backs a REAL libc::open against the follower's own
// subdir file — load-bearing for Phase-2 fd-based re-execute ops.
// Consensus + O_APPEND is rejected leader-side (see dispatch).
// -------------------------------------------------------------------

pub struct FsOpenHandler;

pub struct FsOpenArgs {
    root: String,
    rel: String,
    mode: String,
    cmode: ConsensusMode,
}

impl FsHandler for FsOpenHandler {
    const NAME: &'static str = "fs_open";
    const ARITY: usize = 5; // (root, rel, mode, cmode, ack)

    type Args = FsOpenArgs;

    fn parse_content(args: &[Par]) -> Result<FsOpenArgs, Box<HandlerReply>> {
        let [root_par, rel_par, mode_par, cmode_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (String, String, String, String)",
            ));
        };
        // Cmode validation first — matches pre-refactor.
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
            RhoString::unapply(root_par),
            RhoString::unapply(rel_par),
            RhoString::unapply(mode_par),
        ) {
            (Some(root), Some(rel), Some(mode)) => Ok(FsOpenArgs {
                root,
                rel,
                mode,
                cmode,
            }),
            _ => Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (String, String, String, String)",
            )),
        }
    }

    fn pre_charge_cost() -> crate::rust::interpreter::accounting::costs::Cost {
        costs::fs_open_cost()
    }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsOpenArgs,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            let par =
                open_impl_via_table(ctx.handles, args.root, args.rel, args.mode, args.cmode).await;
            HandlerReply::Ok(par)
        })
    }

    fn on_replay_side_effect<'a>(
        ctx: SyscallCtx<'a>,
        raw_args: &'a [Par],
        previous: &'a [Par],
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        // C-R1: install a shadow FileHandle at the leader's fd so
        // downstream replay-branch mutating handlers can look up
        // (cmode, canon_path).  Phase-2: for Consensus caps, the
        // shadow's `file` slot backs a REAL libc::open via
        // safe_open_verified so downstream fd-based re-execute ops
        // (fs_size / fs_read / etc.) can run on the follower's own
        // subdir file.  Oracular caps keep the pre-Phase-2
        // metadata-only shadow (file: None).
        Box::pin(async move {
            let Some(fd) = extract_ok_fd(previous) else {
                return;
            };
            let [root_par, rel_par, mode_par, cmode_par] = raw_args else {
                return;
            };
            let (Some(root), Some(rel), Some(mode_str)) = (
                RhoString::unapply(root_par),
                RhoString::unapply(rel_par),
                RhoString::unapply(mode_par),
            ) else {
                return;
            };
            // Bogus cmode with cached [true, fd] is a leader bug;
            // fail-closed to Consensus (most restrictive).
            let cmode = resolve_cmode(cmode_par).unwrap_or(ConsensusMode::Consensus);
            let intent = parse_open_mode(&mode_str);
            // Phase-2: real-open under Consensus + non-append.
            // Under any other combination (Oracular, or Consensus +
            // append which is rejected leader-side), shadow's `file`
            // slot is None.
            let file: Option<std::sync::Arc<std::fs::File>> = if cmode == ConsensusMode::Consensus {
                match intent {
                    Some(intent) if !intent.append => {
                        let root_pb = PathBuf::from(&root);
                        let (root_pb, expected_root_id) =
                            ctx.handles.root_registry.resolve_or_identity(&root_pb);
                        let rel_for_open = rel.clone();
                        let intent_copy = intent;
                        let opened = spawn_blocking(move || {
                            let (flags, mode_bits) = fopen_flags(intent_copy);
                            super::path::safe_open_verified(
                                &root_pb,
                                &rel_for_open,
                                flags,
                                mode_bits,
                                expected_root_id,
                            )
                        })
                        .await;
                        match opened {
                            Ok(Ok(f)) => Some(std::sync::Arc::new(f)),
                            _ => None,
                        }
                    }
                    _ => None,
                }
            } else {
                None
            };
            let deploy = ctx.current_deploy_scope();
            let shadow = FileHandle {
                file,
                canon_path: canonicalize_lexical(&root, &rel),
                mode: intent.map(|i| i.mode).unwrap_or(AccessMode::Read),
                cmode,
                position: 0,
                deploy,
            };
            // Ignore return: slot already occupied on repeat call →
            // existing handle wins.  Real divergence surfaces later
            // via WAL comparison.
            let _ = ctx.handles.insert_at(fd.as_u64(), shadow).await;
        })
    }
}

#[linkme::distributed_slice(FS_HANDLERS)]
static FS_OPEN_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsOpenHandler as FsHandler>::NAME,
    arity: <FsOpenHandler as FsHandler>::ARITY,
    verifying: <FsOpenHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsOpenHandler>(fs, args)),
    urn_suffix: "open",
    fixed_channel: FixedChannels::fs_open,
    body_ref: BodyRefs::FS_OPEN,
    family: HandlerFamily::Lifecycle,
};
