// Mutation family — path- or fd-based syscalls that mutate FS state.
//
// 8 verifying handlers (fs_remove_dir is a 9th mutation handler but
// stays trait-exempt in `handlers.rs` — see wave-3-plan.md § S3.11).
// All 8 pre-append a Success placeholder WAL entry in `pre_syscall`,
// finalize the entry (partial-write patch or Failure flip) in
// `journal`, and re-execute under Consensus follower verify:
//   - fs_write / fs_write_at: byte-payload mutation; length-para-
//     meterized cost; `write_impl_via_table` runs libc::pwrite on
//     the shadow fd.  fs_write advances shadow position on all 4
//     paths; fs_write_at doesn't.
//   - fs_truncate: fd + n; constant cost; libc::ftruncate.
//   - fs_chmod: mode-bits mutation; safe_descend_verified + chmod.
//   - fs_chown: owner/group mutation.  Consensus caps rejected
//     (host uid/gid state is not deterministic across validators).
//   - fs_rename / fs_copy_file: 2-path handlers using
//     `journal_path_mutation_two_via_table`.
//   - fs_remove_file: 1-path handler with lock-registry gate
//     (Consensus + locked → FSERR_BUSY; Oracular + locked → log-warn
//     + proceed).
//
// Wave-3 S3.13b (2026-09-10) — moved out of `handlers.rs`.  Imports
// sibling helpers via `super::handlers::X` (visibility: `pub(super)`).
//
// Family: `HandlerFamily::Mutation` (see handler_trait.rs).

use std::path::PathBuf;

use models::rhoapi::Par;
use tokio::task::spawn_blocking;

use super::super::rho_type::{RhoByteArray, RhoNumber, RhoString};
use super::super::system_processes::{BodyRefs, FixedChannels};
use super::errors::*;
use super::handler_trait::{
    dispatch_via_trait_owned, FsHandler, FsHandlerEntry, HandlerFamily, HandlerReply, JournalPath,
    SyscallCtx, FS_HANDLERS,
};
use super::handlers::{
    chown_impl, finalize_failure_journal_via_table, finalize_write_journal_via_table,
    journal_path_mutation_single_via_table, journal_path_mutation_two_via_table,
    journal_truncate_via_table, journal_write_via_table, resolve_cmode, spawn_blocking_par,
    target_dev_inode_at, unlink_leaf_via_dirfd, write_impl_via_table, RemoveKind, MAX_WRITE_BYTES,
};
use super::path::{
    canonicalize_lexical, io_msg_scrub, quarantine_err_reply, safe_descend_verified,
    safe_open_verified,
};
use super::response::{err, extract_err_code, extract_ok_u64, ok_bare, ok_u64};
use super::wal::WalOp;
use super::{costs, ConsensusMode};

// -------------------------------------------------------------------
// fs_truncate — (fd, n) -> [true]  (S3.7, 2026-09-09)
//
// Verifying mutation, constant cost.  Cmode fd-based.  Pre-appends
// WAL entry via `journal_truncate_via_table` in `pre_syscall`;
// finalizes via `finalize_failure_journal_via_table` in `journal`
// on syscall error or verify-divergence.
// -------------------------------------------------------------------

pub struct FsTruncateHandler;

pub struct FsTruncateArgs {
    fd: u64,
    n: u64,
}

impl FsHandler for FsTruncateHandler {
    const NAME: &'static str = "fs_truncate";
    const ARITY: usize = 3; // (fd, n, ack)
    const VERIFYING: bool = true;

    type Args = FsTruncateArgs;

    fn parse_content(args: &[Par]) -> Result<FsTruncateArgs, Box<HandlerReply>> {
        let [fd_par, n_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (u64, u64)",
            ));
        };
        match (RhoNumber::unapply(fd_par), RhoNumber::unapply(n_par)) {
            (Some(fd), Some(n)) if n >= 0 => Ok(FsTruncateArgs {
                fd: fd as u64,
                n: n as u64,
            }),
            _ => Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (u64, u64)",
            )),
        }
    }

    fn pre_charge_cost() -> crate::rust::interpreter::accounting::costs::Cost {
        costs::fs_truncate_cost()
    }

    fn pre_syscall<'a>(
        ctx: SyscallCtx<'a>,
        args: &'a FsTruncateArgs,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), Box<HandlerReply>>> + Send + 'a>,
    > {
        // C-29-F1: pre-append WAL entry on BOTH leader and follower.
        // MAX_TRUNCATE_BYTES gate first so oversize truncates don't
        // consume a WAL slot for calls that will error out.
        Box::pin(async move {
            if args.n > super::MAX_TRUNCATE_BYTES {
                return Ok(());
            }
            if journal_truncate_via_table(ctx.handles, args.fd, args.n, ctx.ack)
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
        args: FsTruncateArgs,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            if args.n > super::MAX_TRUNCATE_BYTES {
                return HandlerReply::err(
                    FSERR_QUOTA_EXCEEDED,
                    format!("truncate {} exceeds MAX_TRUNCATE_BYTES", args.n),
                );
            }
            let file_arc = match ctx.handles.raw_fd(args.fd).await {
                Some(f) => f,
                // Pre-refactor fs_truncate destructured `fd: u64` from
                // the parsed tuple `Some((fd as u64, n as u64))`, so
                // the format is UNSIGNED.  Different from fs_seek /
                // fs_size where fd is `i64` from RhoNumber::unapply
                // and format is signed.  Preserve pre-refactor
                // byte-identity — no `as i64` cast here.
                None => {
                    return HandlerReply::err(FSERR_CLOSED, format!("unknown fd {}", args.fd));
                }
            };
            let n = args.n;
            let r = spawn_blocking(move || {
                use std::os::fd::AsRawFd;
                let raw_fd = file_arc.as_raw_fd();
                // SAFETY: `raw_fd` is derived from `file_arc: Arc<File>`
                // whose lifetime spans the `spawn_blocking` closure;
                // the fd remains open for the duration of the syscall.
                // `ftruncate` takes an open fd and a signed length;
                // negative return means errno is set.
                unsafe {
                    if libc::ftruncate(raw_fd, n as i64) < 0 {
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
        _raw_args: &'a [Par],
        path: JournalPath<'a>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        // H-6 finalize: patch the pre-appended Success placeholder to
        // Failure(code) on syscall error or verify-divergence.  Success
        // path (no err payload) leaves the placeholder as Success.
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

#[linkme::distributed_slice(FS_HANDLERS)]
static FS_TRUNCATE_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsTruncateHandler as FsHandler>::NAME,
    arity: <FsTruncateHandler as FsHandler>::ARITY,
    verifying: <FsTruncateHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsTruncateHandler>(fs, args)),
    urn_suffix: "truncate",
    fixed_channel: FixedChannels::fs_truncate,
    body_ref: BodyRefs::FS_TRUNCATE,
    family: HandlerFamily::Mutation,
};

// -------------------------------------------------------------------
// fs_chmod — (root, rel, bits, cmode) -> [true]  (S3.7, 2026-09-09)
//
// Verifying path-mutation.  Cmode arg-based.  Pre-appends WAL
// entry via `journal_path_mutation_single_via_table(WalOp::Chmod,
// mode_bits=Some(bits))` in `pre_syscall`.
// -------------------------------------------------------------------

pub struct FsChmodHandler;

pub struct FsChmodArgs {
    root: String,
    rel: String,
    bits: u32,
    cmode: ConsensusMode,
}

impl FsHandler for FsChmodHandler {
    const NAME: &'static str = "fs_chmod";
    const ARITY: usize = 5; // (root, rel, bits, cmode, ack)
    const VERIFYING: bool = true;

    type Args = FsChmodArgs;

    fn parse_content(args: &[Par]) -> Result<FsChmodArgs, Box<HandlerReply>> {
        let [root_par, rel_par, mode_par, cmode_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (String, String, u64<=0o7777, String)",
            ));
        };
        // Cmode validation first — matches pre-refactor specific
        // "cmode must be..." error message.
        let cmode = match resolve_cmode(cmode_par) {
            Some(m) => m,
            None => {
                return Err(HandlerReply::boxed_err(
                    FSERR_BAD_ARG,
                    "cmode must be String \"oracular\" or \"consensus\"",
                ));
            }
        };
        // Combined args parse — pre-refactor emits the combined
        // "expected (String, String, u64<=0o7777, String)" on any
        // single failure.
        let combined_err = || {
            HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (String, String, u64<=0o7777, String)",
            )
        };
        let root = RhoString::unapply(root_par).ok_or_else(combined_err)?;
        let rel = RhoString::unapply(rel_par).ok_or_else(combined_err)?;
        let bits = RhoNumber::unapply(mode_par).ok_or_else(combined_err)?;
        if !(0..=0o7777).contains(&bits) {
            return Err(combined_err());
        }
        Ok(FsChmodArgs {
            root,
            rel,
            bits: bits as u32,
            cmode,
        })
    }

    fn pre_charge_cost() -> crate::rust::interpreter::accounting::costs::Cost {
        costs::fs_chmod_cost()
    }

    fn pre_syscall<'a>(
        ctx: SyscallCtx<'a>,
        args: &'a FsChmodArgs,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), Box<HandlerReply>>> + Send + 'a>,
    > {
        Box::pin(async move {
            let canon_path = canonicalize_lexical(&args.root, &args.rel);
            if journal_path_mutation_single_via_table(
                ctx.handles,
                args.cmode,
                WalOp::Chmod,
                canon_path,
                Some(args.bits),
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
        args: FsChmodArgs,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            let root_pb = PathBuf::from(&args.root);
            let (root_pb, expected_root_id) =
                ctx.handles.root_registry.resolve_or_identity(&root_pb);
            let rel = args.rel;
            let bits = args.bits as libc::mode_t;
            let par = spawn_blocking_par(move || -> Par {
                let parent = match safe_descend_verified(&root_pb, &rel, expected_root_id) {
                    Ok(p) => p,
                    Err(qe) => {
                        let (c, m) = quarantine_err_reply(&qe);
                        return err(c, m);
                    }
                };
                // SAFETY: `parent` is a `DirfdRoot` from
                // `safe_descend_verified`; the enclosed dirfd is open
                // for the lifetime of `parent` (RAII), and
                // `parent.leaf_ptr()` returns a NUL-terminated
                // `*const c_char` valid for the same lifetime.
                // `fchmodat` reads both without retaining either past
                // the call.
                let rc = unsafe {
                    libc::fchmodat(
                        parent.as_raw_fd(),
                        parent.leaf_ptr(),
                        bits,
                        libc::AT_SYMLINK_NOFOLLOW,
                    )
                };
                if rc == 0 {
                    ok_bare()
                } else {
                    let e = std::io::Error::last_os_error();
                    // ENOTSUP / EOPNOTSUPP → FSERR_UNSUPPORTED (Linux
                    // and some fs don't honor AT_SYMLINK_NOFOLLOW on
                    // chmod; report UNSUPPORTED rather than silently
                    // following).
                    let code = if e.raw_os_error() == Some(libc::ENOTSUP)
                        || e.raw_os_error() == Some(libc::EOPNOTSUPP)
                    {
                        FSERR_UNSUPPORTED
                    } else {
                        io_err_code(&e)
                    };
                    err(code, io_msg_scrub(&e))
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
            let [_, _, _, cmode_par] = raw_args else {
                return None;
            };
            resolve_cmode(cmode_par)
        })
    }

    fn journal<'a>(
        ctx: SyscallCtx<'a>,
        _raw_args: &'a [Par],
        path: JournalPath<'a>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
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

#[linkme::distributed_slice(FS_HANDLERS)]
static FS_CHMOD_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsChmodHandler as FsHandler>::NAME,
    arity: <FsChmodHandler as FsHandler>::ARITY,
    verifying: <FsChmodHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsChmodHandler>(fs, args)),
    urn_suffix: "chmod",
    fixed_channel: FixedChannels::fs_chmod,
    body_ref: BodyRefs::FS_CHMOD,
    family: HandlerFamily::Mutation,
};

// -------------------------------------------------------------------
// fs_chown — (root, rel, owner, group, cmode) -> [true]  (S3.7)
//
// NON-verifying path-mutation.  Consensus BANNED at parse_content
// with FSERR_UNSUPPORTED (post-2026-09-02 S-2 security review —
// NSS mapping is host-local).  Oracular-only journal call is a
// no-op (journal_path_mutation_single_via_table self-guards).
// -------------------------------------------------------------------

pub struct FsChownHandler;

pub struct FsChownArgs {
    root: String,
    rel: String,
    owner: Option<String>,
    group: Option<String>,
    cmode: ConsensusMode,
}

impl FsHandler for FsChownHandler {
    const NAME: &'static str = "fs_chown";
    const ARITY: usize = 6; // (root, rel, owner, group, cmode, ack)
                            // NON-verifying — the Consensus ban means no verify path ever
                            // fires (Consensus caps rejected at parse_content).

    type Args = FsChownArgs;

    fn parse_content(args: &[Par]) -> Result<FsChownArgs, Box<HandlerReply>> {
        let [root_par, rel_par, owner_par, group_par, cmode_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (String, String, String|Nil, String|Nil, String)",
            ));
        };
        // C-26-F1: fail-closed on unrecognized cmode.
        let cmode = match resolve_cmode(cmode_par) {
            Some(m) => m,
            None => {
                return Err(HandlerReply::boxed_err(
                    FSERR_BAD_ARG,
                    "cmode must be String \"oracular\" or \"consensus\"",
                ));
            }
        };
        // Post-2026-09-02 S-2 review ban: fs_chown + Consensus is
        // UNSUPPORTED (NSS mapping is host-local, silently divergent).
        if cmode == ConsensusMode::Consensus {
            return Err(HandlerReply::boxed_err(
                FSERR_UNSUPPORTED,
                "fs_chown: Consensus mode not supported — NSS mapping (owner/group \
                 name to uid/gid) is host-local and can differ across validators, \
                 producing silent on-disk divergence that the reply-hash verify \
                 cannot detect.  Use Oracular mode, or lift this ban by capturing \
                 resolved uid/gid in the WAL entry with shard-wide NSS coordination.",
            ));
        }
        let (root, rel) = match (RhoString::unapply(root_par), RhoString::unapply(rel_par)) {
            (Some(r), Some(l)) => (r, l),
            _ => {
                return Err(HandlerReply::boxed_err(
                    FSERR_BAD_ARG,
                    "expected (String, String, String|Nil, String|Nil, String)",
                ));
            }
        };
        Ok(FsChownArgs {
            root,
            rel,
            owner: RhoString::unapply(owner_par),
            group: RhoString::unapply(group_par),
            cmode,
        })
    }

    fn pre_charge_cost() -> crate::rust::interpreter::accounting::costs::Cost {
        costs::fs_chown_cost()
    }

    fn pre_syscall<'a>(
        ctx: SyscallCtx<'a>,
        args: &'a FsChownArgs,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), Box<HandlerReply>>> + Send + 'a>,
    > {
        // Post-S-2 ban above rules out Consensus, so this call is
        // Oracular-only and journal_path_mutation_single_via_table
        // no-ops (returns Ok(false)).  Kept for structural parity
        // with the other path-mutation handlers.
        Box::pin(async move {
            let canon_path = canonicalize_lexical(&args.root, &args.rel);
            if journal_path_mutation_single_via_table(
                ctx.handles,
                args.cmode,
                WalOp::Chown,
                canon_path,
                None,
                args.owner.clone(),
                args.group.clone(),
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
        args: FsChownArgs,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            let root_pb = PathBuf::from(&args.root);
            let (root_pb, expected_root_id) =
                ctx.handles.root_registry.resolve_or_identity(&root_pb);
            let par =
                chown_impl(&root_pb, args.rel, args.owner, args.group, expected_root_id).await;
            HandlerReply::Ok(par)
        })
    }

    fn journal<'a>(
        ctx: SyscallCtx<'a>,
        _raw_args: &'a [Par],
        path: JournalPath<'a>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        // fs_chown is non-verifying; framework never invokes
        // VerifyDivergence path.  finalize_failure_journal fires on
        // Leader / OracularEcho if reply has an err code.
        Box::pin(async move {
            let reply = path.produce_reply();
            if let Some(code_str) = extract_err_code(std::slice::from_ref(reply)) {
                finalize_failure_journal_via_table(ctx.handles, fserr_to_code(&code_str), ctx.ack);
            }
        })
    }
}

#[linkme::distributed_slice(FS_HANDLERS)]
static FS_CHOWN_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsChownHandler as FsHandler>::NAME,
    arity: <FsChownHandler as FsHandler>::ARITY,
    verifying: <FsChownHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsChownHandler>(fs, args)),
    urn_suffix: "chown",
    fixed_channel: FixedChannels::fs_chown,
    body_ref: BodyRefs::FS_CHOWN,
    family: HandlerFamily::Mutation,
};

// -------------------------------------------------------------------
// fs_write — (fd, bytes) -> [true, nWritten]  (S3.8, 2026-09-09)
//
// Verifying mutation, length-parameterized cost.  Cmode fd-based.
// Pre-appends WAL entry (with position from fd shadow as offset
// for sequential Write) via `journal_write_via_table` in
// `pre_syscall`.  On partial write, patches WAL entry's length +
// payload_ref via `finalize_write_journal_via_table`.  Shadow
// position advances by actually-written bytes on all 4 paths
// (POSIX libc::write advances OS fd; shadow must sync).
// -------------------------------------------------------------------

pub struct FsWriteHandler;

pub struct FsWriteArgs {
    fd: u64,
    bytes: Vec<u8>,
}

impl FsHandler for FsWriteHandler {
    const NAME: &'static str = "fs_write";
    const ARITY: usize = 3; // (fd, bytes, ack)
    const VERIFYING: bool = true;

    type Args = FsWriteArgs;

    fn parse_content(args: &[Par]) -> Result<FsWriteArgs, Box<HandlerReply>> {
        let [fd_par, bytes_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (u64, ByteArray)",
            ));
        };
        match (RhoNumber::unapply(fd_par), RhoByteArray::unapply(bytes_par)) {
            (Some(fd), Some(bytes)) => Ok(FsWriteArgs {
                fd: fd as u64,
                bytes,
            }),
            _ => Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (u64, ByteArray)",
            )),
        }
    }

    fn pre_charge_cost() -> crate::rust::interpreter::accounting::costs::Cost {
        costs::fs_write_cost(0)
    }

    fn pre_charge_incremental(
        raw_args: &[Par],
    ) -> Option<crate::rust::interpreter::accounting::costs::Cost> {
        // Cost based on bytes.len() from raw_args[1] (bytes_par).
        let requested_bytes: u64 = raw_args
            .get(1)
            .and_then(RhoByteArray::unapply)
            .map(|b| b.len() as u64)
            .unwrap_or(0);
        Some(costs::fs_write_cost(requested_bytes))
    }

    fn pre_syscall<'a>(
        ctx: SyscallCtx<'a>,
        args: &'a FsWriteArgs,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), Box<HandlerReply>>> + Send + 'a>,
    > {
        Box::pin(async move {
            // MAX_WRITE_BYTES gate first — oversize writes must NOT
            // consume a WAL slot (M-R3 review round 2).
            if args.bytes.len() as u64 > MAX_WRITE_BYTES {
                return Err(HandlerReply::boxed_err(
                    FSERR_QUOTA_EXCEEDED,
                    format!("write {} exceeds MAX_WRITE_BYTES", args.bytes.len()),
                ));
            }
            // Pre-append WAL entry (fd-based cmode; self-guards for
            // Oracular).  Err → WAL cap → early exit with
            // FSERR_QUOTA_EXCEEDED.
            if journal_write_via_table(ctx.handles, args.fd, &args.bytes, None, ctx.ack)
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
        args: FsWriteArgs,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            let par = write_impl_via_table(ctx.handles, args.fd, args.bytes, None).await;
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
            let [fd_par, bytes_par, ..] = raw_args else {
                return;
            };
            let Some(fd_i64) = RhoNumber::unapply(fd_par) else {
                return;
            };
            let fd_u = fd_i64 as u64;
            let bytes_opt = RhoByteArray::unapply(bytes_par);
            let state_source = path.state_source_reply();
            // Gated `n_opt` — matches pre-refactor's `if let Some(n)
            // = extract_ok_u64(...)` gate.  On error reply,
            // extract_ok_u64 returns None → NO finalize_write_journal
            // (would incorrectly patch WAL length to 0) and NO shadow
            // advance (nothing to sync — the syscall didn't move OS
            // fd position).
            let n_opt = extract_ok_u64(std::slice::from_ref(state_source));
            if path.is_divergence() {
                finalize_failure_journal_via_table(
                    ctx.handles,
                    FSERR_CODE_CONSENSUS_DIVERGENCE,
                    ctx.ack,
                );
            } else {
                // Success paths (Leader / VerifySuccess /
                // OracularEcho).  Finalize partial write only if the
                // reply carries an ok_u64 n AND n < requested bytes
                // (patches WAL entry's length + payload_ref).
                if let Some(n) = n_opt {
                    if let Some(bytes) = bytes_opt.as_ref() {
                        if n < bytes.len() as u64 {
                            finalize_write_journal_via_table(ctx.handles, bytes, n, ctx.ack);
                        }
                    }
                }
                // Finalize failure if the reply carries an err code
                // (independent of the partial-write check — the reply
                // is either ok or err, never both).
                let reply = path.produce_reply();
                if let Some(code_str) = extract_err_code(std::slice::from_ref(reply)) {
                    finalize_failure_journal_via_table(
                        ctx.handles,
                        fserr_to_code(&code_str),
                        ctx.ack,
                    );
                }
            }
            // Shadow position advance by n on all 4 paths (POSIX
            // libc::write moved the OS-fd position by that count).
            // Gated on Some(n) — no advance on error reply.
            if let Some(n) = n_opt {
                if n > 0 {
                    let _ = ctx
                        .handles
                        .with_mut(fd_u, |h| h.position = h.position.saturating_add(n))
                        .await;
                }
            }
        })
    }
}

#[linkme::distributed_slice(FS_HANDLERS)]
static FS_WRITE_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsWriteHandler as FsHandler>::NAME,
    arity: <FsWriteHandler as FsHandler>::ARITY,
    verifying: <FsWriteHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsWriteHandler>(fs, args)),
    urn_suffix: "write",
    fixed_channel: FixedChannels::fs_write,
    body_ref: BodyRefs::FS_WRITE,
    family: HandlerFamily::Mutation,
};

// -------------------------------------------------------------------
// fs_write_at — (fd, off, bytes) -> [true, nWritten]  (S3.8)
//
// Verifying mutation with offset.  Same shape as fs_write; NO
// shadow position advance (POSIX libc::pwrite doesn't advance
// OS-fd position).
// -------------------------------------------------------------------

pub struct FsWriteAtHandler;

pub struct FsWriteAtArgs {
    fd: u64,
    off: u64,
    bytes: Vec<u8>,
}

impl FsHandler for FsWriteAtHandler {
    const NAME: &'static str = "fs_write_at";
    const ARITY: usize = 4; // (fd, off, bytes, ack)
    const VERIFYING: bool = true;

    type Args = FsWriteAtArgs;

    fn parse_content(args: &[Par]) -> Result<FsWriteAtArgs, Box<HandlerReply>> {
        let [fd_par, off_par, bytes_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (u64, u64, ByteArray)",
            ));
        };
        match (
            RhoNumber::unapply(fd_par),
            RhoNumber::unapply(off_par),
            RhoByteArray::unapply(bytes_par),
        ) {
            (Some(fd), Some(off), Some(bytes)) if off >= 0 => Ok(FsWriteAtArgs {
                fd: fd as u64,
                off: off as u64,
                bytes,
            }),
            _ => Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (u64, u64, ByteArray)",
            )),
        }
    }

    fn pre_charge_cost() -> crate::rust::interpreter::accounting::costs::Cost {
        costs::fs_write_at_cost(0)
    }

    fn pre_charge_incremental(
        raw_args: &[Par],
    ) -> Option<crate::rust::interpreter::accounting::costs::Cost> {
        // bytes_par is at slot 2 for fs_write_at (fd, off, bytes).
        let requested_bytes: u64 = raw_args
            .get(2)
            .and_then(RhoByteArray::unapply)
            .map(|b| b.len() as u64)
            .unwrap_or(0);
        Some(costs::fs_write_at_cost(requested_bytes))
    }

    fn pre_syscall<'a>(
        ctx: SyscallCtx<'a>,
        args: &'a FsWriteAtArgs,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), Box<HandlerReply>>> + Send + 'a>,
    > {
        Box::pin(async move {
            if args.bytes.len() as u64 > MAX_WRITE_BYTES {
                return Err(HandlerReply::boxed_err(
                    FSERR_QUOTA_EXCEEDED,
                    format!("write {} exceeds MAX_WRITE_BYTES", args.bytes.len()),
                ));
            }
            if journal_write_via_table(ctx.handles, args.fd, &args.bytes, Some(args.off), ctx.ack)
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
        args: FsWriteAtArgs,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            let par = write_impl_via_table(ctx.handles, args.fd, args.bytes, Some(args.off)).await;
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
        // fs_write_at doesn't advance shadow (pwrite semantics).
        // Otherwise same as fs_write: finalize partial + finalize
        // failure.  Gated on Some(n) so an error reply doesn't
        // patch the WAL entry's length to 0.
        Box::pin(async move {
            let [_, _, bytes_par, ..] = raw_args else {
                return;
            };
            let bytes_opt = RhoByteArray::unapply(bytes_par);
            let state_source = path.state_source_reply();
            let n_opt = extract_ok_u64(std::slice::from_ref(state_source));
            if path.is_divergence() {
                finalize_failure_journal_via_table(
                    ctx.handles,
                    FSERR_CODE_CONSENSUS_DIVERGENCE,
                    ctx.ack,
                );
            } else {
                if let Some(n) = n_opt {
                    if let Some(bytes) = bytes_opt.as_ref() {
                        if n < bytes.len() as u64 {
                            finalize_write_journal_via_table(ctx.handles, bytes, n, ctx.ack);
                        }
                    }
                }
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

#[linkme::distributed_slice(FS_HANDLERS)]
static FS_WRITE_AT_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsWriteAtHandler as FsHandler>::NAME,
    arity: <FsWriteAtHandler as FsHandler>::ARITY,
    verifying: <FsWriteAtHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsWriteAtHandler>(fs, args)),
    urn_suffix: "writeAt",
    fixed_channel: FixedChannels::fs_write_at,
    body_ref: BodyRefs::FS_WRITE_AT,
    family: HandlerFamily::Mutation,
};

// -------------------------------------------------------------------
// fs_rename — (fromRoot, fromRel, toRoot, toRel, cmode) -> [true]
//                                                        (S3.9)
//
// Verifying mutation, path-based with TWO endpoints.  cmode
// arg-based.  Pre-appends WAL entry via
// `journal_path_mutation_two_via_table(WalOp::Rename)` — from-canon
// in `path`, to-canon in `extra_path`.  EXDEV → FSERR_CROSS_DEVICE.
// -------------------------------------------------------------------

pub struct FsRenameHandler;

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

    fn pre_charge_cost() -> crate::rust::interpreter::accounting::costs::Cost {
        costs::fs_rename_cost()
    }

    fn pre_syscall<'a>(
        ctx: SyscallCtx<'a>,
        args: &'a FsRenameArgs,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), Box<HandlerReply>>> + Send + 'a>,
    > {
        Box::pin(async move {
            let from_canon = canonicalize_lexical(&args.from_root, &args.from_rel);
            let to_canon = canonicalize_lexical(&args.to_root, &args.to_rel);
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
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            let from_root_pb = PathBuf::from(&args.from_root);
            let to_root_pb = PathBuf::from(&args.to_root);
            let (from_root_pb, from_expected_id) =
                ctx.handles.root_registry.resolve_or_identity(&from_root_pb);
            let (to_root_pb, to_expected_id) =
                ctx.handles.root_registry.resolve_or_identity(&to_root_pb);
            let from_rel = args.from_rel;
            let to_rel = args.to_rel;
            let par = spawn_blocking_par(move || -> Par {
                let from_parent =
                    match safe_descend_verified(&from_root_pb, &from_rel, from_expected_id) {
                        Ok(p) => p,
                        Err(qe) => {
                            let (c, m) = quarantine_err_reply(&qe);
                            return err(c, m);
                        }
                    };
                let to_parent = match safe_descend_verified(&to_root_pb, &to_rel, to_expected_id) {
                    Ok(p) => p,
                    Err(qe) => {
                        let (c, m) = quarantine_err_reply(&qe);
                        return err(c, m);
                    }
                };
                // SAFETY: both `from_parent` and `to_parent` are
                // `DirfdRoot` RAII wrappers from `safe_descend_
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
                    ok_bare()
                } else {
                    let e = std::io::Error::last_os_error();
                    let code = if e.raw_os_error() == Some(libc::EXDEV) {
                        FSERR_CROSS_DEVICE
                    } else {
                        io_err_code(&e)
                    };
                    err(code, io_msg_scrub(&e))
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
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        // H-6 finalize pattern — same as fs_chmod / fs_truncate.
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

#[linkme::distributed_slice(FS_HANDLERS)]
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

// -------------------------------------------------------------------
// fs_copy_file — (fromRoot, fromRel, toRoot, toRel, cmode)
//                                        -> [true, nBytes]  (S3.9)
//
// Verifying mutation, path-based with TWO endpoints.  Same shape as
// fs_rename but reply carries a u64 byte count.  H-5 identity
// migration: uses safe_open_verified on both endpoints.
// -------------------------------------------------------------------

pub struct FsCopyFileHandler;

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

    fn pre_charge_cost() -> crate::rust::interpreter::accounting::costs::Cost {
        costs::fs_copy_file_cost()
    }

    fn pre_syscall<'a>(
        ctx: SyscallCtx<'a>,
        args: &'a FsCopyFileArgs,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), Box<HandlerReply>>> + Send + 'a>,
    > {
        Box::pin(async move {
            let from_canon = canonicalize_lexical(&args.from_root, &args.from_rel);
            let to_canon = canonicalize_lexical(&args.to_root, &args.to_rel);
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
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            let from_root_pb = PathBuf::from(&args.from_root);
            let to_root_pb = PathBuf::from(&args.to_root);
            let (from_root_pb, from_expected_id) =
                ctx.handles.root_registry.resolve_or_identity(&from_root_pb);
            let (to_root_pb, to_expected_id) =
                ctx.handles.root_registry.resolve_or_identity(&to_root_pb);
            let from_rel = args.from_rel;
            let to_rel = args.to_rel;
            let par = spawn_blocking_par(move || -> Par {
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
                        return err(c, m);
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
                        return err(c, m);
                    }
                };
                match std::io::copy(&mut src, &mut dst) {
                    Ok(n) => ok_u64(n),
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
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        // H-6 finalize — same as fs_rename.  No shadow state to
        // advance.
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

#[linkme::distributed_slice(FS_HANDLERS)]
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

// -------------------------------------------------------------------
// fs_remove_file — (root, rel, cmode) -> [true]  (S3.10)
//
// Verifying path-mutation.  Consensus+locked: FSERR_BUSY.
// Oracular+locked: proceeds with log-warn (POSIX rm-while-open
// semantics).  Uses unlink_leaf_via_dirfd via SafeParent.
// -------------------------------------------------------------------

pub struct FsRemoveFileHandler;

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

    fn pre_charge_cost() -> crate::rust::interpreter::accounting::costs::Cost {
        costs::fs_remove_file_cost()
    }

    fn pre_syscall<'a>(
        ctx: SyscallCtx<'a>,
        args: &'a FsRemoveFileArgs,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), Box<HandlerReply>>> + Send + 'a>,
    > {
        Box::pin(async move {
            let canon_path = canonicalize_lexical(&args.root, &args.rel);
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
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            let root_pb = PathBuf::from(&args.root);
            let (root_pb, expected_root_id) =
                ctx.handles.root_registry.resolve_or_identity(&root_pb);
            let lock_registry = ctx.handles.lock_registry.clone();
            let rel = args.rel;
            let cmode = args.cmode;
            let par = spawn_blocking_par(move || -> Par {
                let parent = match safe_descend_verified(&root_pb, &rel, expected_root_id) {
                    Ok(p) => p,
                    Err(qe) => {
                        let (c, m) = quarantine_err_reply(&qe);
                        return err(c, m);
                    }
                };
                // Step 6: mode-differentiated unlink gate.
                let target_dev_inode = target_dev_inode_at(&parent);
                let target_is_locked = target_dev_inode
                    .map(|di| lock_registry.is_locked(di, (0, u64::MAX)))
                    .unwrap_or(false);
                if cmode == ConsensusMode::Consensus && target_is_locked {
                    return err(
                        FSERR_BUSY,
                        "cannot remove: lock held on target (dev, inode)",
                    );
                }
                if cmode == ConsensusMode::Oracular && target_is_locked {
                    if let Some((dev, ino)) = target_dev_inode {
                        let n_holders = lock_registry.count_locks((dev, ino));
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
                    Ok(()) => ok_bare(),
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
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        // H-6 finalize — same as fs_chmod / fs_rename / fs_copy_file.
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

#[linkme::distributed_slice(FS_HANDLERS)]
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
