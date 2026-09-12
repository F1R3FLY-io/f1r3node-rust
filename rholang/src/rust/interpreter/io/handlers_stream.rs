// Stream family — per-fd directory-entries streaming primitives.
//
// 3 non-verifying handlers backing `Dir.rho::entries()`:
//   - fs_entries_stream_open  — allocate a stream fd, `openat` +
//     `fdopendir` under `safe_descend_verified`.  Consensus caps
//     rejected (readdir order not stable across per-validator subdirs).
//   - fs_entries_stream_next  — yield one entry per call via
//     `readdir_one_entry`; two-branch cost (setup + per-entry
//     supplement) via `post_reply_supplement`.
//   - fs_entries_stream_close — release the stream fd; shadow-remove
//     on replay so leader/follower dir_handles tables converge.
//
// Wave-3 S3.13b (2026-09-10) — moved out of `handlers.rs` into this
// per-family module.  Trait impl bodies + FS_HANDLERS registrations
// unchanged from the pre-split state; imports adjusted to reference
// sibling helpers in `handlers.rs` via `super::handlers::X`
// (visibility: `pub(super)`).
//
// Framework helpers (FsHandler, HandlerReply, SyscallCtx,
// FsHandlerEntry, HandlerFamily, dispatch_via_trait_owned,
// FS_HANDLERS) live in `super::handler_trait`.
//
// Family: `HandlerFamily::Stream` (see handler_trait.rs).

use std::path::PathBuf;

use models::rhoapi::Par;
use tokio::task::spawn_blocking;

use super::super::rho_type::{RhoNumber, RhoString};
use super::super::system_processes::{BodyRefs, FixedChannels};
use super::dir_handle_table::{DirHandle, DirIter};
use super::errors::*;
use super::handler_trait::{
    dispatch_via_trait_owned, FsHandler, FsHandlerEntry, HandlerFamily, HandlerReply, SyscallCtx,
    FS_HANDLERS,
};
use super::handlers::{
    journal_state_read_via_table, readdir_one_entry, reply_is_ok, resolve_cmode, spawn_blocking_par,
};
use super::path::{
    canonicalize_lexical, io_msg_scrub, quarantine_err_reply, safe_descend_verified,
};
use super::response::{err, extract_ok_fd, ok_bare, ok_fd, Fd};
use super::wal::WalOp;
use super::{costs, ConsensusMode};

// -------------------------------------------------------------------
// fs_entries_stream_close — (fd) -> [true]  (S3.3)
//
// Non-verifying stream lifecycle.  Mirrors fs_close's Phase-2 fd-
// release pattern: on_replay_side_effect removes the follower's
// shadow dir_handle so the follower's dir_handles table converges
// to the leader's post-close state.
// -------------------------------------------------------------------

pub struct FsEntriesStreamCloseHandler;

pub struct FsEntriesStreamCloseArgs {
    fd: u64,
}

impl FsHandler for FsEntriesStreamCloseHandler {
    const NAME: &'static str = "fs_entries_stream_close";
    const ARITY: usize = 2; // (fd, ack)

    type Args = FsEntriesStreamCloseArgs;

    fn parse_content(args: &[Par]) -> Result<FsEntriesStreamCloseArgs, Box<HandlerReply>> {
        let [fd_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected GInt streamFd",
            ));
        };
        let fd = RhoNumber::unapply(fd_par)
            .ok_or_else(|| HandlerReply::boxed_err(FSERR_BAD_ARG, "expected GInt streamFd"))?;
        Ok(FsEntriesStreamCloseArgs { fd: fd as u64 })
    }

    fn pre_charge_cost() -> crate::rust::interpreter::accounting::costs::Cost {
        costs::fs_entries_stream_close_cost()
    }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsEntriesStreamCloseArgs,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            ctx.handles.dir_handles.remove(args.fd).await;
            HandlerReply::ok(ok_bare())
        })
    }

    fn on_replay_side_effect<'a>(
        ctx: SyscallCtx<'a>,
        raw_args: &'a [Par],
        _previous: &'a [Par],
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            if let [fd_par, ..] = raw_args {
                if let Some(fd) = RhoNumber::unapply(fd_par) {
                    ctx.handles.dir_handles.remove(fd as u64).await;
                }
            }
        })
    }
}

#[linkme::distributed_slice(FS_HANDLERS)]
static FS_ENTRIES_STREAM_CLOSE_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsEntriesStreamCloseHandler as FsHandler>::NAME,
    arity: <FsEntriesStreamCloseHandler as FsHandler>::ARITY,
    verifying: <FsEntriesStreamCloseHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| {
        Box::pin(dispatch_via_trait_owned::<FsEntriesStreamCloseHandler>(
            fs, args,
        ))
    },
    urn_suffix: "entriesStreamClose",
    fixed_channel: FixedChannels::fs_entries_stream_close,
    body_ref: BodyRefs::FS_ENTRIES_STREAM_CLOSE,
    family: HandlerFamily::Stream,
};

// -------------------------------------------------------------------
// fs_entries_stream_open — (root, rel, cmode) -> [true, streamFd]  (S3.4)
//
// Non-verifying stream lifecycle.  Complex is_replay:
// shadow-inserts a DirHandle at the leader's cached fd so
// downstream replay-branch handlers (stream_next / _close) can
// look up (cmode, canon_path) from the DirHandleTable.  Consensus
// caps are banned Phase-2 (readdir order not stable across
// per-validator subdirs); banned reply is [false, FSERR_UNSUPPORTED,
// ...] on the leader, so the replay path sees no `[true, fd]` and
// skips the shadow install.
// -------------------------------------------------------------------

pub struct FsEntriesStreamOpenHandler;

pub struct FsEntriesStreamOpenArgs {
    root: String,
    rel: String,
    cmode: ConsensusMode,
}

impl FsHandler for FsEntriesStreamOpenHandler {
    const NAME: &'static str = "fs_entries_stream_open";
    const ARITY: usize = 4; // (root, rel, cmode, ack)

    type Args = FsEntriesStreamOpenArgs;

    fn parse_content(args: &[Par]) -> Result<FsEntriesStreamOpenArgs, Box<HandlerReply>> {
        let [root_par, rel_par, cmode_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (String root, String rel)",
            ));
        };
        // cmode validation first — matches pre-refactor error-message
        // ordering (cmode check runs after arity+cmode-Par extraction).
        let cmode = match resolve_cmode(cmode_par) {
            Some(m) => m,
            None => {
                return Err(HandlerReply::boxed_err(
                    FSERR_BAD_ARG,
                    "cmode must be String \"oracular\" or \"consensus\"",
                ));
            }
        };
        // Phase 2 ban (2026-09-01): Consensus + entriesStream* is
        // UNSUPPORTED.  Reject at open time (see original handler
        // docstring for the readdir-order rationale).
        if cmode == ConsensusMode::Consensus {
            return Err(HandlerReply::boxed_err(
                FSERR_UNSUPPORTED,
                "entriesStream* is not supported on Consensus caps — readdir order \
                 is fs-dependent and not stable across per-validator subdirs.  Use \
                 `fs_entries` (sorted, deterministic across validators) instead.",
            ));
        }
        // String args extraction (post-cmode check per pre-refactor
        // control flow — a bogus cmode with valid root/rel produces
        // the "cmode must be String..." error, not "expected (String
        // root, String rel)").
        let (root, rel) = match (RhoString::unapply(root_par), RhoString::unapply(rel_par)) {
            (Some(r), Some(l)) => (r, l),
            _ => {
                return Err(HandlerReply::boxed_err(
                    FSERR_BAD_ARG,
                    "expected (String root, String rel)",
                ));
            }
        };
        Ok(FsEntriesStreamOpenArgs { root, rel, cmode })
    }

    fn pre_charge_cost() -> crate::rust::interpreter::accounting::costs::Cost {
        costs::fs_entries_stream_open_cost()
    }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsEntriesStreamOpenArgs,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            let root_pb = PathBuf::from(&args.root);
            let (root_pb, expected_root_id) =
                ctx.handles.root_registry.resolve_or_identity(&root_pb);
            let rel_for_open = args.rel.clone();
            // safe_descend + openat + fdopendir in a blocking task.
            // Err returns a Box<Par> so the Result stays pointer-sized.
            //
            // X-2 / G-01: park_external_during so the reduction driver
            // can advance other participants while the descent + open
            // + fdopendir runs on the blocking pool.
            let opened = crate::rust::interpreter::deterministic_reduction::park_external_during(
                spawn_blocking(move || -> Result<DirIter, Box<Par>> {
                    let parent =
                        match safe_descend_verified(&root_pb, &rel_for_open, expected_root_id) {
                            Ok(p) => p,
                            Err(qe) => {
                                let (code, msg) = quarantine_err_reply(&qe);
                                return Err(Box::new(err(code, msg)));
                            }
                        };
                    // SAFETY: `parent` is a `DirfdRoot` from
                    // `safe_descend_verified`; the dirfd is open for
                    // `parent`'s lifetime and `parent.leaf_ptr()` returns
                    // a NUL-terminated `*const c_char` valid for the same
                    // lifetime.  `openat` does not retain either past the
                    // call.
                    let dir_fd = unsafe {
                        libc::openat(
                            parent.as_raw_fd(),
                            parent.leaf_ptr(),
                            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                        )
                    };
                    if dir_fd < 0 {
                        let e = std::io::Error::last_os_error();
                        return Err(Box::new(err(io_err_code(&e), io_msg_scrub(&e))));
                    }
                    // L-3 pattern (fs_entries): F_DUPFD_CLOEXEC on the fd
                    // handed to fdopendir so the DIR*'s underlying fd
                    // carries CLOEXEC atomically.  Close the original.
                    //
                    // SAFETY: `dir_fd` is a freshly-opened open fd from
                    // the `openat` above; the `< 0` check preceded us.
                    // `fcntl(F_DUPFD_CLOEXEC, ...)` reads the fd flags
                    // and returns a new fd (or -1 on failure).
                    let read_fd = unsafe { libc::fcntl(dir_fd, libc::F_DUPFD_CLOEXEC, 0) };
                    // SAFETY: same `dir_fd` from `openat` above; still
                    // open (we haven't closed or dup'd-with-transfer it).
                    // We ignore the return value — even if close fails,
                    // the fd is gone.
                    unsafe { libc::close(dir_fd) };
                    if read_fd < 0 {
                        let e = std::io::Error::last_os_error();
                        return Err(Box::new(err(io_err_code(&e), io_msg_scrub(&e))));
                    }
                    DirIter::from_dir_fd(read_fd)
                        .map_err(|e| Box::new(err(io_err_code(&e), io_msg_scrub(&e))))
                }),
            )
            .await
            .unwrap_or_else(|je| crate::rust::interpreter::io::errors::join_err_abort(je));
            match opened {
                Ok(iter) => {
                    let deploy = ctx.current_deploy_scope();
                    let handle = DirHandle::new(
                        iter,
                        canonicalize_lexical(&args.root, &args.rel),
                        args.cmode,
                        deploy,
                    );
                    match ctx.handles.dir_handles.insert(handle).await {
                        // A-3 (2026-09-03): emit via ok_fd(Fd::from(...)).
                        Ok(fd) => HandlerReply::ok(ok_fd(Fd::from(fd))),
                        Err(()) => HandlerReply::err(
                            FSERR_QUOTA_EXCEEDED,
                            "per-runtime dir-stream fd cap reached",
                        ),
                    }
                }
                Err(e_par) => HandlerReply::Err(*e_par),
            }
        })
    }

    fn on_replay_side_effect<'a>(
        ctx: SyscallCtx<'a>,
        raw_args: &'a [Par],
        previous: &'a [Par],
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        // Shadow-insert at the leader's fd so subsequent replay-
        // branch handlers can look up cmode / canon_path.  Only fires
        // for Oracular streams; Consensus opens are rejected leader-
        // side so `extract_ok_fd(previous)` returns None and no
        // shadow gets inserted.
        Box::pin(async move {
            if let Some(fd) = extract_ok_fd(previous) {
                if let [root_par, rel_par, cmode_par, ..] = raw_args {
                    if let (Some(root), Some(rel)) =
                        (RhoString::unapply(root_par), RhoString::unapply(rel_par))
                    {
                        // Fall back to Consensus on bogus cmode —
                        // matches fs_open's C-R1 fallback rationale
                        // (a bogus cmode with a [true, fd] cached
                        // reply is definitionally a bug; fail-closed
                        // to the more restrictive mode).
                        let cmode = resolve_cmode(cmode_par).unwrap_or(ConsensusMode::Consensus);
                        let deploy = ctx.current_deploy_scope();
                        let shadow =
                            DirHandle::shadow(canonicalize_lexical(&root, &rel), cmode, deploy);
                        let _ = ctx.handles.dir_handles.insert_at(fd.as_u64(), shadow).await;
                    }
                }
            }
        })
    }
}

#[linkme::distributed_slice(FS_HANDLERS)]
static FS_ENTRIES_STREAM_OPEN_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsEntriesStreamOpenHandler as FsHandler>::NAME,
    arity: <FsEntriesStreamOpenHandler as FsHandler>::ARITY,
    verifying: <FsEntriesStreamOpenHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| {
        Box::pin(dispatch_via_trait_owned::<FsEntriesStreamOpenHandler>(
            fs, args,
        ))
    },
    urn_suffix: "entriesStreamOpen",
    fixed_channel: FixedChannels::fs_entries_stream_open,
    body_ref: BodyRefs::FS_ENTRIES_STREAM_OPEN,
    family: HandlerFamily::Stream,
};

// -------------------------------------------------------------------
// fs_entries_stream_next — (streamFd) -> [true, entryRecord]  (S3.4)
//
// Non-verifying stream advance.  Two-branch cost (setup +
// per-entry supplement) matches bulk fs_entries.  WAL journaling
// on both leader and replay paths for Consensus caps — but under
// the Phase-2 ban no Consensus stream fd ever reaches here in
// normal flow.  Journaling kept as load-bearing structural parity
// for a future ban lift.
// -------------------------------------------------------------------

pub struct FsEntriesStreamNextHandler;

pub struct FsEntriesStreamNextArgs {
    fd: u64,
}

impl FsHandler for FsEntriesStreamNextHandler {
    const NAME: &'static str = "fs_entries_stream_next";
    const ARITY: usize = 2; // (streamFd, ack)

    type Args = FsEntriesStreamNextArgs;

    fn parse_content(args: &[Par]) -> Result<FsEntriesStreamNextArgs, Box<HandlerReply>> {
        let [fd_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected GInt streamFd",
            ));
        };
        let fd = RhoNumber::unapply(fd_par)
            .ok_or_else(|| HandlerReply::boxed_err(FSERR_BAD_ARG, "expected GInt streamFd"))?;
        Ok(FsEntriesStreamNextArgs { fd: fd as u64 })
    }

    fn pre_charge_cost() -> crate::rust::interpreter::accounting::costs::Cost {
        costs::fs_entries_stream_next_cost()
    }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsEntriesStreamNextArgs,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            // Hold onto the Arc so we can re-consult (cmode, canon_path)
            // for the post-dispatch journal call.
            let handle_opt = ctx.handles.dir_handles.get(args.fd).await;
            let reply_par = if let Some(handle) = handle_opt.as_ref() {
                let cmode = handle.cmode;
                let iter_lock = handle.iter.lock().await;
                match iter_lock.as_ref() {
                    None => err(
                        FSERR_CLOSED,
                        "shadow stream handle has no iterator (leader path only)",
                    ),
                    Some(iter) => {
                        // Pass the DIR* address across spawn_blocking
                        // via usize (raw pointers are not Send).  The
                        // MutexGuard `iter_lock` is held across the
                        // .await below, keeping the address live.
                        let dirp_addr = iter.as_ptr() as usize;
                        spawn_blocking_par(move || -> Par {
                            let dirp = dirp_addr as *mut libc::DIR;
                            readdir_one_entry(dirp, cmode)
                        })
                        .await
                    }
                }
            } else {
                err(FSERR_CLOSED, "stream fd closed or unknown")
            };
            // Journal the leader's reply if the handle is Consensus.
            // `journal_state_read_via_table` self-guards on
            // Consensus — no-op for Oracular caps.  Under the Phase-2
            // ban this WAL-append is unreachable in normal flow.
            if let Some(handle) = handle_opt.as_ref() {
                let n = if reply_is_ok(std::slice::from_ref(&reply_par)) {
                    1
                } else {
                    0
                };
                journal_state_read_via_table(
                    ctx.handles,
                    handle.cmode,
                    WalOp::EntriesStreamNext,
                    handle.canon_path.clone(),
                    &reply_par,
                    ctx.ack,
                    Some(n),
                );
            }
            // Wrap the Par into HandlerReply.  Success reply is
            // [true, entryRecord]; EOS is [false, "EOS"]; error is
            // [false, code, msg].  The framework's post_reply_supplement
            // classifies via `reply_is_ok` — both variants funnel to
            // HandlerReply::Ok because the bytes are what matter for
            // the framework's supplement calc, not the enum tag.
            HandlerReply::Ok(reply_par)
        })
    }

    fn on_replay_side_effect<'a>(
        ctx: SyscallCtx<'a>,
        raw_args: &'a [Par],
        previous: &'a [Par],
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        // Journal the cached reply if the fd's shadow reports a
        // Consensus cmode.  Under the Phase-2 ban this branch is
        // unreachable in normal flow (no Consensus stream fds
        // exist).  Kept for structural parity.
        Box::pin(async move {
            if let [fd_par, ..] = raw_args {
                if let Some(fd) = RhoNumber::unapply(fd_par) {
                    if let Some(handle) = ctx.handles.dir_handles.get(fd as u64).await {
                        if let Some(reply_par) = previous.first() {
                            let n = if reply_is_ok(previous) { 1 } else { 0 };
                            journal_state_read_via_table(
                                ctx.handles,
                                handle.cmode,
                                WalOp::EntriesStreamNext,
                                handle.canon_path.clone(),
                                reply_par,
                                ctx.ack,
                                Some(n),
                            );
                        }
                    }
                }
            }
        })
    }

    fn post_reply_supplement(
        reply: &[Par],
    ) -> Option<crate::rust::interpreter::accounting::costs::Cost> {
        // Two-branch supplement matches pre-refactor:
        //   n = 1 on `[true, entryRecord]`
        //   n = 0 on EOS `[false, "EOS"]` or error `[false, code, msg]`.
        // Uses `reserve_incremental_primitive` (framework auto-
        // dispatches) because the n=0 case legitimately produces a
        // zero-weight cost — `reserve_primitive` returns BugFoundError
        // on zero.
        let n = if reply_is_ok(reply) { 1 } else { 0 };
        Some(costs::fs_entries_stream_per_entry_supplement_cost(n))
    }
}

#[linkme::distributed_slice(FS_HANDLERS)]
static FS_ENTRIES_STREAM_NEXT_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsEntriesStreamNextHandler as FsHandler>::NAME,
    arity: <FsEntriesStreamNextHandler as FsHandler>::ARITY,
    verifying: <FsEntriesStreamNextHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| {
        Box::pin(dispatch_via_trait_owned::<FsEntriesStreamNextHandler>(
            fs, args,
        ))
    },
    urn_suffix: "entriesStreamNext",
    fixed_channel: FixedChannels::fs_entries_stream_next,
    body_ref: BodyRefs::FS_ENTRIES_STREAM_NEXT,
    family: HandlerFamily::Stream,
};
