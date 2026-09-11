// Observation family — read-only handlers that never mutate FS state.
//
// 9 handlers, most Phase-5-verifying (Consensus follower re-executes
// the syscall and compares its fresh reply's stable_hash against the
// leader's cached; a mismatch fires FSERR_CONSENSUS_DIVERGENCE):
//   - fs_stat       (verifying) — file/dir metadata; Consensus mode
//                    omits host-transient fields (mtime/ctime/atime,
//                    owner, group).
//   - fs_entries    (verifying) — sorted directory listing; two-event
//                    cost (setup + per-entry supplement).
//   - fs_exists     (verifying) — post-2026-09-04 ban lift; arity 4
//                    with cmode threaded.
//   - fs_size       (verifying) — fd-based size via fstat.
//   - fs_read       (verifying) — length-parameterized cost + WAL
//                    payload hash journaling; shadow-fd position
//                    advance on both leader + follower.
//   - fs_read_at    (verifying) — pread against fd + offset.
//   - fs_seek       (verifying) — updates shadow fd position on
//                    follower to match leader.
//   - fs_flush      (non-verifying) — fsync (data + metadata).
//   - fs_tell       (non-verifying) — shadow-fd position read.
//
// Wave-3 S3.13b (2026-09-10) — moved out of `handlers.rs`.  Imports
// sibling helpers via `super::handlers::X` (visibility: `pub(super)`).
//
// Family: `HandlerFamily::Observation` (see handler_trait.rs).

use std::path::PathBuf;

use models::rhoapi::Par;
use tokio::task::spawn_blocking;

use super::super::rho_type::{RhoNumber, RhoString};
use super::super::system_processes::{BodyRefs, FixedChannels};
use super::errors::*;
use super::handler_trait::{
    dispatch_via_trait_owned, FsHandler, FsHandlerEntry, HandlerFamily, HandlerReply, JournalPath,
    SyscallCtx, FS_HANDLERS,
};
use super::handlers::{
    entry_stat_row, fstatat_meta, journal_read_divergence_via_table, journal_read_via_table,
    journal_state_read_via_table, leaf_of, read_dir_capped, read_impl_via_table, resolve_cmode,
    spawn_blocking_par, MAX_ENTRIES,
};
use super::path::{io_msg_scrub, quarantine_err_reply, safe_descend_verified};
use super::response::{
    err, extract_ok_bytes, extract_ok_list_len, extract_ok_u64, ok_bare, ok_bool, ok_list, ok_par,
    ok_u64,
};
use super::stat::stat_record;
use super::wal::WalOp;
use super::{costs, ConsensusMode};

// -------------------------------------------------------------------
// fs_flush — (fd) -> [true]  (fsync: data + metadata)
// -------------------------------------------------------------------

pub struct FsFlushHandler;

pub struct FsFlushArgs {
    fd: u64,
}

impl FsHandler for FsFlushHandler {
    const NAME: &'static str = "fs_flush";
    const ARITY: usize = 2; // (fd, ack)
                            // VERIFYING defaults to false — fs_flush is a non-verifying
                            // handler.  See handlers.rs top-comment § "The 15 verifying
                            // handlers" for the verify matrix.

    type Args = FsFlushArgs;

    fn parse_content(args: &[Par]) -> Result<FsFlushArgs, Box<HandlerReply>> {
        let [fd_par] = args else {
            // Unreachable in practice: framework's ARITY check runs
            // first and rejects with `illegal_argument_error(NAME)`.
            // Kept for future-proofing (a caller path that bypasses
            // the arity check would land here).
            return Err(HandlerReply::boxed_err(FSERR_BAD_ARG, "expected u64"));
        };
        let fd = RhoNumber::unapply(fd_par)
            .ok_or_else(|| HandlerReply::boxed_err(FSERR_BAD_ARG, "expected u64"))?;
        Ok(FsFlushArgs { fd: fd as u64 })
    }

    fn pre_charge_cost() -> crate::rust::interpreter::accounting::costs::Cost {
        costs::fs_flush_cost()
    }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsFlushArgs,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            let file_arc = match ctx.handles.raw_fd(args.fd).await {
                Some(f) => f,
                None => {
                    return HandlerReply::err(FSERR_CLOSED, format!("unknown fd {}", args.fd));
                }
            };
            let r = spawn_blocking(move || {
                use std::os::fd::AsRawFd;
                let raw_fd = file_arc.as_raw_fd();
                // SAFETY: `raw_fd` is derived from `file_arc:
                // Arc<File>` whose lifetime spans this closure; the
                // fd is open for the syscall.  `fsync` accepts any
                // integer and returns -1 with errno on invalid fd
                // rather than UB.
                unsafe {
                    if libc::fsync(raw_fd) < 0 {
                        Err(std::io::Error::last_os_error())
                    } else {
                        Ok(())
                    }
                }
            })
            .await;
            match r {
                Err(je) => super::errors::join_err_abort(je),
                Ok(Err(e)) => HandlerReply::err(io_err_code(&e), io_msg_scrub(&e)),
                Ok(Ok(())) => HandlerReply::ok(ok_bare()),
            }
        })
    }
}

#[linkme::distributed_slice(FS_HANDLERS)]
static FS_FLUSH_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsFlushHandler as FsHandler>::NAME,
    arity: <FsFlushHandler as FsHandler>::ARITY,
    verifying: <FsFlushHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsFlushHandler>(fs, args)),
    urn_suffix: "flush",
    fixed_channel: FixedChannels::fs_flush,
    body_ref: BodyRefs::FS_FLUSH,
    family: HandlerFamily::Observation,
};

// -------------------------------------------------------------------
// fs_tell — (fd) -> [true, pos]  (S3.2, 2026-09-08)
//
// Non-verifying (see verify.rs::FdPositionMutator + F-3 comment).
// The dispatch body preserves the compile-time link to
// `FdPositionMutator::ALL` that the pre-migration handler kept on
// its is_replay branch — the reference is now inside dispatch's
// non-replay path but still in handlers.rs's compilation unit.
// -------------------------------------------------------------------

pub struct FsTellHandler;

pub struct FsTellArgs {
    fd: u64,
}

impl FsHandler for FsTellHandler {
    const NAME: &'static str = "fs_tell";
    const ARITY: usize = 2; // (fd, ack)

    type Args = FsTellArgs;

    fn parse_content(args: &[Par]) -> Result<FsTellArgs, Box<HandlerReply>> {
        let [fd_par] = args else {
            return Err(HandlerReply::boxed_err(FSERR_BAD_ARG, "expected u64"));
        };
        let fd = RhoNumber::unapply(fd_par)
            .ok_or_else(|| HandlerReply::boxed_err(FSERR_BAD_ARG, "expected u64"))?;
        Ok(FsTellArgs { fd: fd as u64 })
    }

    fn pre_charge_cost() -> crate::rust::interpreter::accounting::costs::Cost {
        costs::fs_tell_cost()
    }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsTellArgs,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = HandlerReply> + Send + 'a>> {
        // F-3 (2026-09-04) compile-time link — see
        // verify.rs::FdPositionMutator.  The const-eval guard in
        // verify.rs is module-scope so it fires regardless of any
        // reference here; this line is a click-through aid keeping
        // FdPositionMutator visible from the fs_tell dispatch body.
        let _fd_position_mutator_invariant = super::verify::FdPositionMutator::ALL;
        Box::pin(async move {
            let file_arc = match ctx.handles.raw_fd(args.fd).await {
                Some(f) => f,
                None => {
                    return HandlerReply::err(FSERR_CLOSED, format!("unknown fd {}", args.fd));
                }
            };
            let r = spawn_blocking(move || {
                use std::os::fd::AsRawFd;
                let raw_fd = file_arc.as_raw_fd();
                // SAFETY: `raw_fd` derives from `file_arc: Arc<File>`
                // whose lifetime spans this closure; fd is open for
                // the call.  `lseek(SEEK_CUR, 0)` is idempotent and
                // returns the current offset (or -1 with errno).
                unsafe {
                    let pos = libc::lseek(raw_fd, 0, libc::SEEK_CUR);
                    if pos < 0 {
                        Err(std::io::Error::last_os_error())
                    } else {
                        Ok(pos as u64)
                    }
                }
            })
            .await;
            match r {
                Err(je) => super::errors::join_err_abort(je),
                Ok(Err(e)) => HandlerReply::err(io_err_code(&e), io_msg_scrub(&e)),
                Ok(Ok(pos)) => HandlerReply::ok(ok_u64(pos)),
            }
        })
    }
}

#[linkme::distributed_slice(FS_HANDLERS)]
static FS_TELL_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsTellHandler as FsHandler>::NAME,
    arity: <FsTellHandler as FsHandler>::ARITY,
    verifying: <FsTellHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsTellHandler>(fs, args)),
    urn_suffix: "tell",
    fixed_channel: FixedChannels::fs_tell,
    body_ref: BodyRefs::FS_TELL,
    family: HandlerFamily::Observation,
};

// -------------------------------------------------------------------
// fs_size — (fd) -> [true, nBytes]  (S3.5, 2026-09-09)
//
// First verifying handler.  Cmode is fd-based (looked up from the
// file handle table's shadow).  Consensus follower re-executes fstat
// and verifies fresh reply's hash against leader's cached hash.
// Journal keyed on the fd's shadow (cmode, canon_path).
// -------------------------------------------------------------------

pub struct FsSizeHandler;

pub struct FsSizeArgs {
    fd: u64,
}

impl FsHandler for FsSizeHandler {
    const NAME: &'static str = "fs_size";
    const ARITY: usize = 2; // (fd, ack)
    const VERIFYING: bool = true;

    type Args = FsSizeArgs;

    fn parse_content(args: &[Par]) -> Result<FsSizeArgs, Box<HandlerReply>> {
        let [fd_par] = args else {
            return Err(HandlerReply::boxed_err(FSERR_BAD_ARG, "expected u64"));
        };
        let fd = RhoNumber::unapply(fd_par)
            .ok_or_else(|| HandlerReply::boxed_err(FSERR_BAD_ARG, "expected u64"))?;
        Ok(FsSizeArgs { fd: fd as u64 })
    }

    fn pre_charge_cost() -> crate::rust::interpreter::accounting::costs::Cost {
        costs::fs_size_cost()
    }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsSizeArgs,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            match ctx.handles.raw_fd(args.fd).await {
                Some(file_arc) => {
                    let r = spawn_blocking(move || {
                        use std::os::fd::AsRawFd;
                        let raw_fd = file_arc.as_raw_fd();
                        // SAFETY: `libc::stat` is a POD C struct that
                        // `zeroed()` can validly initialize (integer
                        // fields).  `raw_fd` derives from `file_arc:
                        // Arc<File>` whose lifetime spans this
                        // closure.  `fstat` reads `&mut sb` without
                        // retaining it past the call.
                        unsafe {
                            let mut sb: libc::stat = std::mem::zeroed();
                            if libc::fstat(raw_fd, &mut sb) < 0 {
                                Err(std::io::Error::last_os_error())
                            } else {
                                Ok(sb.st_size as u64)
                            }
                        }
                    })
                    .await;
                    match r {
                        Err(je) => super::errors::join_err_abort(je),
                        Ok(Err(e)) => HandlerReply::err(io_err_code(&e), io_msg_scrub(&e)),
                        Ok(Ok(n)) => HandlerReply::ok(ok_u64(n)),
                    }
                }
                None => HandlerReply::err(FSERR_CLOSED, format!("unknown fd {}", args.fd)),
            }
        })
    }

    fn resolve_replay_cmode<'a>(
        ctx: SyscallCtx<'a>,
        raw_args: &'a [Par],
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<ConsensusMode>> + Send + 'a>>
    {
        // fd-based cmode: read the fd's shadow.  Matches pre-refactor
        // `handles.with_mut(fd, |h| h.cmode)`.  A missing shadow or
        // bad fd_par produces None → framework Oracular tautological
        // path.
        Box::pin(async move {
            let [fd_par, ..] = raw_args else {
                return None;
            };
            let fd = RhoNumber::unapply(fd_par)?;
            ctx.handles.with_mut(fd as u64, |h| h.cmode).await
        })
    }

    fn journal<'a>(
        ctx: SyscallCtx<'a>,
        raw_args: &'a [Par],
        path: JournalPath<'a>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        let reply = path.produce_reply();
        // Look up (cmode, canon_path) from the fd's shadow.  If the
        // fd is unknown, no journaling.
        Box::pin(async move {
            let [fd_par, ..] = raw_args else {
                return;
            };
            let Some(fd) = RhoNumber::unapply(fd_par) else {
                return;
            };
            if let Some((cmode, path)) = ctx
                .handles
                .with_mut(fd as u64, |h| (h.cmode, h.canon_path.clone()))
                .await
            {
                journal_state_read_via_table(
                    ctx.handles,
                    cmode,
                    WalOp::Size,
                    path,
                    reply,
                    ctx.ack,
                    None,
                );
            }
        })
    }
}

#[linkme::distributed_slice(FS_HANDLERS)]
static FS_SIZE_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsSizeHandler as FsHandler>::NAME,
    arity: <FsSizeHandler as FsHandler>::ARITY,
    verifying: <FsSizeHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsSizeHandler>(fs, args)),
    urn_suffix: "size",
    fixed_channel: FixedChannels::fs_size,
    body_ref: BodyRefs::FS_SIZE,
    family: HandlerFamily::Observation,
};

// -------------------------------------------------------------------
// fs_stat — (root, rel, cmode) -> [true, record]  (S3.5, 2026-09-09)
//
// Verifying observation.  Cmode is arg-based (raw_args[2]).
// Consensus follower re-executes fstatat + verifies.  Journal
// keyed on cmode + PathBuf(root).join(rel).
// -------------------------------------------------------------------

pub struct FsStatHandler;

pub struct FsStatArgs {
    root: String,
    rel: String,
    cmode: ConsensusMode,
}

impl FsHandler for FsStatHandler {
    const NAME: &'static str = "fs_stat";
    const ARITY: usize = 4; // (root, rel, cmode, ack)
    const VERIFYING: bool = true;

    type Args = FsStatArgs;

    fn parse_content(args: &[Par]) -> Result<FsStatArgs, Box<HandlerReply>> {
        let [root_par, rel_par, cmode_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (String, String, String)",
            ));
        };
        // Cmode validation first — matches pre-refactor's specific
        // "cmode must be String \"oracular\" or \"consensus\"" reply.
        let cmode = match resolve_cmode(cmode_par) {
            Some(m) => m,
            None => {
                return Err(HandlerReply::boxed_err(
                    FSERR_BAD_ARG,
                    "cmode must be String \"oracular\" or \"consensus\"",
                ));
            }
        };
        let (root, rel) = match (RhoString::unapply(root_par), RhoString::unapply(rel_par)) {
            (Some(r), Some(l)) => (r, l),
            _ => {
                return Err(HandlerReply::boxed_err(
                    FSERR_BAD_ARG,
                    "expected (String, String, String)",
                ));
            }
        };
        Ok(FsStatArgs { root, rel, cmode })
    }

    fn pre_charge_cost() -> crate::rust::interpreter::accounting::costs::Cost {
        costs::fs_stat_cost()
    }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsStatArgs,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            let leaf_name = leaf_of(&args.rel);
            let root_pb = PathBuf::from(&args.root);
            let (root_pb, expected_root_id) =
                ctx.handles.root_registry.resolve_or_identity(&root_pb);
            let rel = args.rel;
            let cmode = args.cmode;
            let par = spawn_blocking_par(move || -> Par {
                let parent = match safe_descend_verified(&root_pb, &rel, expected_root_id) {
                    Ok(p) => p,
                    Err(qe) => {
                        let (code, msg) = quarantine_err_reply(&qe);
                        return err(code, msg);
                    }
                };
                match fstatat_meta(&parent) {
                    Ok(m) => ok_par(stat_record(&leaf_name, &m, cmode)),
                    Err(e) => err(io_err_code(&e), io_msg_scrub(&e)),
                }
            })
            .await;
            HandlerReply::Ok(par)
        })
    }

    fn resolve_replay_cmode<'a>(
        _ctx: SyscallCtx<'a>,
        raw_args: &'a [Par],
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<ConsensusMode>> + Send + 'a>>
    {
        // Arg-based cmode: raw_args[2] is `cmode_par`.
        Box::pin(async move {
            let [_, _, cmode_par] = raw_args else {
                return None;
            };
            resolve_cmode(cmode_par)
        })
    }

    fn journal<'a>(
        ctx: SyscallCtx<'a>,
        raw_args: &'a [Par],
        path: JournalPath<'a>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        let reply = path.produce_reply();
        Box::pin(async move {
            let [root_par, rel_par, cmode_par] = raw_args else {
                return;
            };
            let Some(cmode) = resolve_cmode(cmode_par) else {
                return;
            };
            let (Some(root), Some(rel)) =
                (RhoString::unapply(root_par), RhoString::unapply(rel_par))
            else {
                return;
            };
            let mut path = PathBuf::from(root);
            if !rel.is_empty() {
                path.push(&rel);
            }
            journal_state_read_via_table(
                ctx.handles,
                cmode,
                WalOp::Stat,
                path,
                reply,
                ctx.ack,
                None,
            );
        })
    }
}

#[linkme::distributed_slice(FS_HANDLERS)]
static FS_STAT_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsStatHandler as FsHandler>::NAME,
    arity: <FsStatHandler as FsHandler>::ARITY,
    verifying: <FsStatHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsStatHandler>(fs, args)),
    urn_suffix: "stat",
    fixed_channel: FixedChannels::fs_stat,
    body_ref: BodyRefs::FS_STAT,
    family: HandlerFamily::Observation,
};

// -------------------------------------------------------------------
// fs_exists — (root, rel, cmode) -> [true, Bool]  (S3.5, 2026-09-09)
//
// Verifying observation.  Same shape as fs_stat — arg-based cmode,
// journal keyed on cmode + path.  Distinguishes descent-quarantine
// vs descent-IoError: quarantine surfaces `err(code, msg)`;
// IoError yields `ok_bool(false)`.
// -------------------------------------------------------------------

pub struct FsExistsHandler;

pub struct FsExistsArgs {
    root: String,
    rel: String,
    // cmode not stored in Args — dispatch doesn't use it (fstatat's
    // ok/err drives ok_bool).  Journal + replay_cmode re-resolve
    // from raw_args.
}

impl FsHandler for FsExistsHandler {
    const NAME: &'static str = "fs_exists";
    const ARITY: usize = 4; // (root, rel, cmode, ack)
    const VERIFYING: bool = true;

    type Args = FsExistsArgs;

    fn parse_content(args: &[Par]) -> Result<FsExistsArgs, Box<HandlerReply>> {
        let [root_par, rel_par, cmode_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (String, String, String)",
            ));
        };
        if resolve_cmode(cmode_par).is_none() {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "cmode must be String \"oracular\" or \"consensus\"",
            ));
        }
        let (root, rel) = match (RhoString::unapply(root_par), RhoString::unapply(rel_par)) {
            (Some(r), Some(l)) => (r, l),
            _ => {
                return Err(HandlerReply::boxed_err(
                    FSERR_BAD_ARG,
                    "expected (String, String, String)",
                ));
            }
        };
        Ok(FsExistsArgs { root, rel })
    }

    fn pre_charge_cost() -> crate::rust::interpreter::accounting::costs::Cost {
        costs::fs_exists_cost()
    }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsExistsArgs,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            let root_pb = PathBuf::from(&args.root);
            let (root_pb, expected_root_id) =
                ctx.handles.root_registry.resolve_or_identity(&root_pb);
            let rel = args.rel;
            let par = spawn_blocking_par(move || -> Par {
                let parent = match safe_descend_verified(&root_pb, &rel, expected_root_id) {
                    Ok(p) => p,
                    Err(qe) => {
                        use super::path::QuarantineError::*;
                        return match qe {
                            EscapesRoot | SymlinkComponent | RootIdentityChanged => {
                                let (c, m) = quarantine_err_reply(&qe);
                                err(c, m)
                            }
                            Empty | RootSelf => {
                                let (c, m) = quarantine_err_reply(&qe);
                                err(c, m)
                            }
                            IoError(_, _) => ok_bool(false),
                        };
                    }
                };
                let ok = fstatat_meta(&parent).is_ok();
                ok_bool(ok)
            })
            .await;
            HandlerReply::Ok(par)
        })
    }

    fn resolve_replay_cmode<'a>(
        _ctx: SyscallCtx<'a>,
        raw_args: &'a [Par],
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<ConsensusMode>> + Send + 'a>>
    {
        Box::pin(async move {
            let [_, _, cmode_par] = raw_args else {
                return None;
            };
            resolve_cmode(cmode_par)
        })
    }

    fn journal<'a>(
        ctx: SyscallCtx<'a>,
        raw_args: &'a [Par],
        path: JournalPath<'a>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        let reply = path.produce_reply();
        Box::pin(async move {
            let [root_par, rel_par, cmode_par] = raw_args else {
                return;
            };
            let Some(cmode) = resolve_cmode(cmode_par) else {
                return;
            };
            let (Some(root), Some(rel)) =
                (RhoString::unapply(root_par), RhoString::unapply(rel_par))
            else {
                return;
            };
            let mut path = PathBuf::from(root);
            if !rel.is_empty() {
                path.push(&rel);
            }
            journal_state_read_via_table(
                ctx.handles,
                cmode,
                WalOp::Exists,
                path,
                reply,
                ctx.ack,
                None,
            );
        })
    }
}

#[linkme::distributed_slice(FS_HANDLERS)]
static FS_EXISTS_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsExistsHandler as FsHandler>::NAME,
    arity: <FsExistsHandler as FsHandler>::ARITY,
    verifying: <FsExistsHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsExistsHandler>(fs, args)),
    urn_suffix: "exists",
    fixed_channel: FixedChannels::fs_exists,
    body_ref: BodyRefs::FS_EXISTS,
    family: HandlerFamily::Observation,
};

// -------------------------------------------------------------------
// fs_read — (fd, n) -> [true, bytes]  (S3.6, 2026-09-09)
//
// Verifying observation with length-parameterized cost.  Cmode
// fd-based (fs_size shape).  Journal writes to WAL via journal_
// read_via_table (offset=None for sequential Read).  Shadow
// position advances by bytes.len() on both leader and verify
// paths — including verify-failure (POSIX libc::read advances
// OS fd position by bytes returned; shadow must sync).
// -------------------------------------------------------------------

pub struct FsReadHandler;

pub struct FsReadArgs {
    fd: u64,
    n: u64,
}

impl FsHandler for FsReadHandler {
    const NAME: &'static str = "fs_read";
    const ARITY: usize = 3; // (fd, n, ack)
    const VERIFYING: bool = true;

    type Args = FsReadArgs;

    fn parse_content(args: &[Par]) -> Result<FsReadArgs, Box<HandlerReply>> {
        let [fd_par, n_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (fd:GInt, n:GInt>=0)",
            ));
        };
        match (RhoNumber::unapply(fd_par), RhoNumber::unapply(n_par)) {
            (Some(fd), Some(n)) if n >= 0 => Ok(FsReadArgs {
                fd: fd as u64,
                n: n as u64,
            }),
            _ => Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (fd:GInt, n:GInt>=0)",
            )),
        }
    }

    fn pre_charge_cost() -> crate::rust::interpreter::accounting::costs::Cost {
        // Only used when pre_charge_incremental returns None.  fs_read
        // always overrides.
        costs::fs_read_cost(0)
    }

    fn pre_charge_incremental(
        raw_args: &[Par],
    ) -> Option<crate::rust::interpreter::accounting::costs::Cost> {
        // Charge based on REQUESTED byte count (n_par).  Deterministic
        // across leader and replay.  A non-parseable or negative n_par
        // yields 0 requested bytes, still burning FS_SYSCALL_CONST for
        // the dispatch (via fs_read_cost's base).
        let requested_bytes: u64 = raw_args
            .get(1)
            .and_then(RhoNumber::unapply)
            .and_then(|n| u64::try_from(n).ok())
            .unwrap_or(0);
        Some(costs::fs_read_cost(requested_bytes))
    }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsReadArgs,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            let par = read_impl_via_table(ctx.handles, args.fd, args.n, None).await;
            HandlerReply::Ok(par)
        })
    }

    fn resolve_replay_cmode<'a>(
        ctx: SyscallCtx<'a>,
        raw_args: &'a [Par],
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<ConsensusMode>> + Send + 'a>>
    {
        Box::pin(async move {
            let [fd_par, ..] = raw_args else {
                return None;
            };
            let fd = RhoNumber::unapply(fd_par)?;
            ctx.handles.with_mut(fd as u64, |h| h.cmode).await
        })
    }

    fn journal<'a>(
        ctx: SyscallCtx<'a>,
        raw_args: &'a [Par],
        path: JournalPath<'a>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            let [fd_par, ..] = raw_args else {
                return;
            };
            let Some(fd_i64) = RhoNumber::unapply(fd_par) else {
                return;
            };
            let fd_u = fd_i64 as u64;
            let state_source = path.state_source_reply();
            let bytes_slot = extract_ok_bytes(std::slice::from_ref(state_source));
            let advance_n = bytes_slot.as_ref().map(|b| b.len() as u64).unwrap_or(0);
            // Order matters: journal_read reads the PRE-read shadow
            // position from FileHandle before we advance below.
            // Mirrors pre-refactor's write-then-advance sequence.
            if path.is_divergence() {
                // Verify-failure — journal a Failure entry with
                // FSERR_CODE_CONSENSUS_DIVERGENCE.  Shadow still
                // advances by fresh bytes (POSIX libc::read
                // advanced OS fd position; keeping shadow in sync).
                let _ = journal_read_divergence_via_table(ctx.handles, fd_u, None, ctx.ack).await;
            } else if let Some(bytes) = &bytes_slot {
                // Success / Oracular-echo — journal bytes with
                // payload_ref = Hash(bytes).
                let _ = journal_read_via_table(ctx.handles, fd_u, bytes, None, ctx.ack).await;
            }
            if advance_n > 0 {
                let _ = ctx
                    .handles
                    .with_mut(fd_u, |h| h.position = h.position.saturating_add(advance_n))
                    .await;
            }
        })
    }
}

#[linkme::distributed_slice(FS_HANDLERS)]
static FS_READ_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsReadHandler as FsHandler>::NAME,
    arity: <FsReadHandler as FsHandler>::ARITY,
    verifying: <FsReadHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsReadHandler>(fs, args)),
    urn_suffix: "read",
    fixed_channel: FixedChannels::fs_read,
    body_ref: BodyRefs::FS_READ,
    family: HandlerFamily::Observation,
};

// -------------------------------------------------------------------
// fs_read_at — (fd, off, n) -> [true, bytes]  (S3.6, 2026-09-09)
//
// Verifying observation, length-parameterized cost.  Same shape as
// fs_read but with an offset arg.  Uses libc::pread (does NOT
// advance OS fd position per POSIX) — so journal writes with the
// caller-supplied offset, and shadow position is NOT advanced.
// -------------------------------------------------------------------

pub struct FsReadAtHandler;

pub struct FsReadAtArgs {
    fd: u64,
    off: u64,
    n: u64,
}

impl FsHandler for FsReadAtHandler {
    const NAME: &'static str = "fs_read_at";
    const ARITY: usize = 4; // (fd, off, n, ack)
    const VERIFYING: bool = true;

    type Args = FsReadAtArgs;

    fn parse_content(args: &[Par]) -> Result<FsReadAtArgs, Box<HandlerReply>> {
        let [fd_par, off_par, n_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (fd:GInt, off:GInt>=0, n:GInt>=0)",
            ));
        };
        match (
            RhoNumber::unapply(fd_par),
            RhoNumber::unapply(off_par),
            RhoNumber::unapply(n_par),
        ) {
            (Some(fd), Some(off), Some(n)) if off >= 0 && n >= 0 => Ok(FsReadAtArgs {
                fd: fd as u64,
                off: off as u64,
                n: n as u64,
            }),
            _ => Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (fd:GInt, off:GInt>=0, n:GInt>=0)",
            )),
        }
    }

    fn pre_charge_cost() -> crate::rust::interpreter::accounting::costs::Cost {
        costs::fs_read_at_cost(0)
    }

    fn pre_charge_incremental(
        raw_args: &[Par],
    ) -> Option<crate::rust::interpreter::accounting::costs::Cost> {
        // n_par is at slot 2 for fs_read_at (fd, off, n).
        let requested_bytes: u64 = raw_args
            .get(2)
            .and_then(RhoNumber::unapply)
            .and_then(|n| u64::try_from(n).ok())
            .unwrap_or(0);
        Some(costs::fs_read_at_cost(requested_bytes))
    }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsReadAtArgs,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            let par = read_impl_via_table(ctx.handles, args.fd, args.n, Some(args.off)).await;
            HandlerReply::Ok(par)
        })
    }

    fn resolve_replay_cmode<'a>(
        ctx: SyscallCtx<'a>,
        raw_args: &'a [Par],
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<ConsensusMode>> + Send + 'a>>
    {
        Box::pin(async move {
            let [fd_par, ..] = raw_args else {
                return None;
            };
            let fd = RhoNumber::unapply(fd_par)?;
            ctx.handles.with_mut(fd as u64, |h| h.cmode).await
        })
    }

    fn journal<'a>(
        ctx: SyscallCtx<'a>,
        raw_args: &'a [Par],
        path: JournalPath<'a>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            let [fd_par, off_par, ..] = raw_args else {
                return;
            };
            let (Some(fd_i64), Some(off_i64)) =
                (RhoNumber::unapply(fd_par), RhoNumber::unapply(off_par))
            else {
                return;
            };
            if off_i64 < 0 {
                return;
            }
            let fd_u = fd_i64 as u64;
            let off_u = off_i64 as u64;
            let state_source = path.state_source_reply();
            let bytes_slot = extract_ok_bytes(std::slice::from_ref(state_source));
            if path.is_divergence() {
                let _ = journal_read_divergence_via_table(ctx.handles, fd_u, Some(off_u), ctx.ack)
                    .await;
            } else if let Some(bytes) = &bytes_slot {
                let _ =
                    journal_read_via_table(ctx.handles, fd_u, bytes, Some(off_u), ctx.ack).await;
            }
            // fs_read_at (pread) does NOT advance shadow position
            // per POSIX (see FdPositionMutator in verify.rs — pread
            // deliberately excluded).
        })
    }
}

#[linkme::distributed_slice(FS_HANDLERS)]
static FS_READ_AT_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsReadAtHandler as FsHandler>::NAME,
    arity: <FsReadAtHandler as FsHandler>::ARITY,
    verifying: <FsReadAtHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsReadAtHandler>(fs, args)),
    urn_suffix: "readAt",
    fixed_channel: FixedChannels::fs_read_at,
    body_ref: BodyRefs::FS_READ_AT,
    family: HandlerFamily::Observation,
};

// -------------------------------------------------------------------
// fs_seek — (fd, off, whence) -> [true, pos]  (S3.6, 2026-09-09)
//
// Verifying observation, constant cost.  Cmode fd-based.  Does NOT
// journal to WAL — but journal hook is where shadow position
// advance lives (fires on Leader / VerifySuccess / OracularEcho
// paths; skipped on VerifyDivergence).
// -------------------------------------------------------------------

pub struct FsSeekHandler;

pub struct FsSeekArgs {
    fd: u64,
    off: i64,
    whence: libc::c_int,
}

impl FsHandler for FsSeekHandler {
    const NAME: &'static str = "fs_seek";
    const ARITY: usize = 4; // (fd, off, whence, ack)
    const VERIFYING: bool = true;

    type Args = FsSeekArgs;

    fn parse_content(args: &[Par]) -> Result<FsSeekArgs, Box<HandlerReply>> {
        let [fd_par, off_par, whence_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (u64, i64, String)",
            ));
        };
        // First tier: all three args must parse.  Mismatched error
        // message per pre-refactor.
        let combined_err = || HandlerReply::boxed_err(FSERR_BAD_ARG, "expected (u64, i64, String)");
        let fd = RhoNumber::unapply(fd_par).ok_or_else(combined_err)?;
        let off = RhoNumber::unapply(off_par).ok_or_else(combined_err)?;
        let w = RhoString::unapply(whence_par).ok_or_else(combined_err)?;
        // Second tier: whence must be in {set,cur,end}, and "set"
        // additionally requires off >= 0.  Distinct pre-refactor
        // error message.
        let whence = match w.as_str() {
            "set" if off >= 0 => libc::SEEK_SET,
            "cur" => libc::SEEK_CUR,
            "end" => libc::SEEK_END,
            _ => {
                return Err(HandlerReply::boxed_err(
                    FSERR_BAD_ARG,
                    "expected whence in {set,cur,end}",
                ));
            }
        };
        Ok(FsSeekArgs {
            fd: fd as u64,
            off,
            whence,
        })
    }

    fn pre_charge_cost() -> crate::rust::interpreter::accounting::costs::Cost {
        costs::fs_seek_cost()
    }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsSeekArgs,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            let file_arc = match ctx.handles.raw_fd(args.fd).await {
                Some(f) => f,
                // Pre-refactor fs_seek formatted `fd` as signed i64
                // (RhoNumber::unapply returns i64).  Preserve byte-
                // identity by casting `args.fd: u64` back through
                // `as i64` before formatting.  See wave-3 S3.5
                // review F2 for the equivalent drift in
                // fs_flush/fs_tell/fs_size (deferred).
                None => {
                    return HandlerReply::err(
                        FSERR_CLOSED,
                        format!("unknown fd {}", args.fd as i64),
                    );
                }
            };
            let r = spawn_blocking(move || {
                use std::os::fd::AsRawFd;
                let raw_fd = file_arc.as_raw_fd();
                // SAFETY: `raw_fd` derives from `file_arc: Arc<File>`
                // whose lifetime spans this closure; fd is open for
                // the call.  `lseek` accepts any integer fd and
                // returns -1 with errno on invalid fd or offset.
                unsafe {
                    let pos = libc::lseek(raw_fd, args.off, args.whence);
                    if pos < 0 {
                        Err(std::io::Error::last_os_error())
                    } else {
                        Ok(pos as u64)
                    }
                }
            })
            .await;
            match r {
                Err(je) => super::errors::join_err_abort(je),
                Ok(Err(e)) => HandlerReply::err(io_err_code(&e), io_msg_scrub(&e)),
                Ok(Ok(pos)) => HandlerReply::ok(ok_u64(pos)),
            }
        })
    }

    fn resolve_replay_cmode<'a>(
        ctx: SyscallCtx<'a>,
        raw_args: &'a [Par],
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<ConsensusMode>> + Send + 'a>>
    {
        Box::pin(async move {
            let [fd_par, ..] = raw_args else {
                return None;
            };
            let fd = RhoNumber::unapply(fd_par)?;
            ctx.handles.with_mut(fd as u64, |h| h.cmode).await
        })
    }

    fn journal<'a>(
        ctx: SyscallCtx<'a>,
        raw_args: &'a [Par],
        path: JournalPath<'a>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        // fs_seek doesn't journal to WAL; the "journal" hook here
        // is purely for shadow position advance.  Skip on divergence
        // (pre-refactor's `Err(reason) =>` branch does not touch
        // shadow).  On Leader / VerifySuccess / OracularEcho: set
        // shadow position to the reply's ok_u64.
        Box::pin(async move {
            if path.is_divergence() {
                return;
            }
            let [fd_par, ..] = raw_args else {
                return;
            };
            let Some(fd_i64) = RhoNumber::unapply(fd_par) else {
                return;
            };
            let state_source = path.state_source_reply();
            if let Some(new_pos) = extract_ok_u64(std::slice::from_ref(state_source)) {
                let _ = ctx
                    .handles
                    .with_mut(fd_i64 as u64, |h| h.position = new_pos)
                    .await;
            }
        })
    }
}

#[linkme::distributed_slice(FS_HANDLERS)]
static FS_SEEK_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsSeekHandler as FsHandler>::NAME,
    arity: <FsSeekHandler as FsHandler>::ARITY,
    verifying: <FsSeekHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsSeekHandler>(fs, args)),
    urn_suffix: "seek",
    fixed_channel: FixedChannels::fs_seek,
    body_ref: BodyRefs::FS_SEEK,
    family: HandlerFamily::Observation,
};
// -------------------------------------------------------------------
// fs_entries — (root, rel, cmode) -> [true, [row1, ..., rowN]]  (S3.9)
//
// Verifying observation with TWO-EVENT cost accounting:
// `pre_charge_cost() = fs_entries_cost(0)` = FS_ENTRIES_SETUP
// (base 50, reserve_primitive) — event 1.  `post_reply_supplement`
// = fs_entries_per_entry_supplement_cost(n_entries) via
// reserve_incremental_primitive — event 2.  Both leader and
// follower emit the same 2-event sequence to preserve the D3
// canonical event log fold bytes.
// -------------------------------------------------------------------

pub struct FsEntriesHandler;

pub struct FsEntriesArgs {
    root: String,
    rel: String,
    cmode: ConsensusMode,
}

impl FsHandler for FsEntriesHandler {
    const NAME: &'static str = "fs_entries";
    const ARITY: usize = 4; // (root, rel, cmode, ack)
    const VERIFYING: bool = true;

    type Args = FsEntriesArgs;

    fn parse_content(args: &[Par]) -> Result<FsEntriesArgs, Box<HandlerReply>> {
        let [root_par, rel_par, cmode_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (String, String)",
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
        let (root, rel) = match (RhoString::unapply(root_par), RhoString::unapply(rel_par)) {
            (Some(r), Some(l)) => (r, l),
            _ => {
                return Err(HandlerReply::boxed_err(
                    FSERR_BAD_ARG,
                    "expected (String, String)",
                ));
            }
        };
        Ok(FsEntriesArgs { root, rel, cmode })
    }

    fn pre_charge_cost() -> crate::rust::interpreter::accounting::costs::Cost {
        // Setup weight — base 50 (FS_ENTRIES_SETUP).  Per-entry
        // supplement fires via `post_reply_supplement` after the
        // reply is known.
        costs::fs_entries_cost(0)
    }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsEntriesArgs,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            let root_pb = PathBuf::from(&args.root);
            let (root_pb, expected_root_id) =
                ctx.handles.root_registry.resolve_or_identity(&root_pb);
            let rel = args.rel;
            let cmode = args.cmode;
            let par = spawn_blocking_par(move || -> Par {
                let parent = match safe_descend_verified(&root_pb, &rel, expected_root_id) {
                    Ok(p) => p,
                    Err(qe) => {
                        let (code, msg) = quarantine_err_reply(&qe);
                        return err(code, msg);
                    }
                };
                // SAFETY: `parent` is a `DirfdRoot` RAII wrapper
                // from `safe_descend_verified`; its dirfd is open
                // for `parent`'s lifetime and `parent.leaf_ptr()`
                // returns a NUL-terminated `*const c_char` valid
                // for the same lifetime.  `openat` reads both
                // without retention.
                let dir_fd = unsafe {
                    libc::openat(
                        parent.as_raw_fd(),
                        parent.leaf_ptr(),
                        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                    )
                };
                if dir_fd < 0 {
                    let e = std::io::Error::last_os_error();
                    return err(io_err_code(&e), io_msg_scrub(&e));
                }
                // L-3: F_DUPFD_CLOEXEC keeps CLOEXEC set atomically.
                //
                // SAFETY: `dir_fd` is a freshly-opened open fd
                // from the `openat` above (post-`< 0` check).
                // `fcntl(F_DUPFD_CLOEXEC, 0)` returns a new fd (or
                // -1 with errno) without invalidating `dir_fd`.
                let read_fd = unsafe { libc::fcntl(dir_fd, libc::F_DUPFD_CLOEXEC, 0) };
                if read_fd < 0 {
                    let e = std::io::Error::last_os_error();
                    // SAFETY: `dir_fd` is still open (fcntl above
                    // failed; no dup created).  We must close it.
                    unsafe { libc::close(dir_fd) };
                    return err(io_err_code(&e), io_msg_scrub(&e));
                }
                let entries = read_dir_capped(read_fd, MAX_ENTRIES);
                match entries {
                    Err(e) => {
                        // SAFETY: `dir_fd` open for the duration
                        // of `read_dir_capped` (unaffected by
                        // read errors).
                        unsafe { libc::close(dir_fd) };
                        err(io_err_code(&e), io_msg_scrub(&e))
                    }
                    Ok((mut names, hit_cap)) => {
                        if hit_cap {
                            // SAFETY: `dir_fd` open; return path
                            // must release before propagating.
                            unsafe { libc::close(dir_fd) };
                            return err(
                                FSERR_QUOTA_EXCEEDED,
                                format!(
                                    "entries exceeds MAX_ENTRIES={MAX_ENTRIES}; use \
                                     entriesStreamOpen / _Next / _Close for large \
                                     directories",
                                ),
                            );
                        }
                        names.sort();
                        let rows: Vec<Par> = names
                            .into_iter()
                            .map(|name| entry_stat_row(dir_fd, &name, cmode))
                            .collect();
                        // SAFETY: `dir_fd` was still open through
                        // the `entry_stat_row` iteration; last use
                        // — close it now to release the fd.
                        unsafe { libc::close(dir_fd) };
                        ok_list(rows)
                    }
                }
            })
            .await;
            HandlerReply::Ok(par)
        })
    }

    fn resolve_replay_cmode<'a>(
        _ctx: SyscallCtx<'a>,
        raw_args: &'a [Par],
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<ConsensusMode>> + Send + 'a>>
    {
        Box::pin(async move {
            let [_, _, cmode_par] = raw_args else {
                return None;
            };
            resolve_cmode(cmode_par)
        })
    }

    fn journal<'a>(
        ctx: SyscallCtx<'a>,
        raw_args: &'a [Par],
        path: JournalPath<'a>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        // State-read journal (fs_stat / fs_size / fs_exists shape).
        // No pre-append + finalize pattern for fs_entries — just
        // journal the reply's stable_hash with WalOp::Entries.
        Box::pin(async move {
            let [root_par, rel_par, cmode_par] = raw_args else {
                return;
            };
            let Some(cmode) = resolve_cmode(cmode_par) else {
                return;
            };
            let (Some(root), Some(rel)) =
                (RhoString::unapply(root_par), RhoString::unapply(rel_par))
            else {
                return;
            };
            let mut path_buf = PathBuf::from(root);
            if !rel.is_empty() {
                path_buf.push(&rel);
            }
            let reply = path.produce_reply();
            journal_state_read_via_table(
                ctx.handles,
                cmode,
                WalOp::Entries,
                path_buf,
                reply,
                ctx.ack,
                None,
            );
        })
    }

    fn post_reply_supplement(
        reply: &[Par],
    ) -> Option<crate::rust::interpreter::accounting::costs::Cost> {
        // Per-entry supplement — fires after dispatch (leader path)
        // OR after Oracular echo (via `previous` reply slice).  n =
        // 0 on error / EOS / bad-shape reply.  Uses
        // reserve_incremental_primitive (framework auto-dispatches)
        // because n=0 legitimately produces zero-weight cost.
        let n_entries = extract_ok_list_len(reply).unwrap_or(0);
        Some(costs::fs_entries_per_entry_supplement_cost(n_entries))
    }
}

#[linkme::distributed_slice(FS_HANDLERS)]
static FS_ENTRIES_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsEntriesHandler as FsHandler>::NAME,
    arity: <FsEntriesHandler as FsHandler>::ARITY,
    verifying: <FsEntriesHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsEntriesHandler>(fs, args)),
    urn_suffix: "entries",
    fixed_channel: FixedChannels::fs_entries,
    body_ref: BodyRefs::FS_ENTRIES,
    family: HandlerFamily::Observation,
};
