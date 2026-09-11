// Lock family — byte-range + sequential lock helpers.
//
// 4 non-verifying handlers (LockRegistry is host-local, not
// cross-validator state):
//   - fs_lock_range: byte-range lock acquire (r/w), arity-8 with
//     explicit wait: Bool.  Cross-cap coordination keyed on fd's
//     (dev, ino) via `dev_inode_from_fd_via_table`.
//   - fs_lock_sequential: whole-fd exclusive lock; arity-5 shape
//     like fs_lock_range minus offset/length/mode.
//   - fs_release_lock: pure lock-registry op; idempotent (releasing
//     a non-held lock returns FSERR_CLOSED via `lock_err_reply`).
//   - fs_release_all_for_holder: deploy-end sweep + File.close.
//     Cancel-first / release-second ordering (WalDeployScope::drop
//     B1 fix).
//
// Wave-3 S3.13b (2026-09-10) — moved out of `handlers.rs`.  Imports
// sibling helpers via `super::handlers::X` (visibility: `pub(super)`).
//
// Family: `HandlerFamily::Lock` (see handler_trait.rs).

use models::rhoapi::Par;

use super::super::rho_type::{RhoBoolean, RhoNumber, RhoString};
use super::super::system_processes::{BodyRefs, FixedChannels};
use super::costs;
use super::errors::*;
use super::handler_trait::{
    dispatch_via_trait_owned, FsHandler, FsHandlerEntry, HandlerFamily, HandlerReply, SyscallCtx,
    FS_HANDLERS,
};
use super::handlers::{
    dev_inode_from_fd_via_table, holder_id_of, lock_err_reply, resolve_cmode, resolve_lock_mode,
};
use super::lock::{AcquireOutcome, LockError, LockId, WaitPolicy};
use super::response::{ok_bare, ok_u64};

// -------------------------------------------------------------------
// fs_release_lock — (lock_id) -> [true]  (S3.2, 2026-09-08)
//
// Non-verifying, pure lock-registry op.  Idempotent: releasing a
// non-held lock returns FSERR_CLOSED (via `lock_err_reply`).
// -------------------------------------------------------------------

pub struct FsReleaseLockHandler;

pub struct FsReleaseLockArgs {
    lock_id: u64,
    holder: Par,
}

impl FsHandler for FsReleaseLockHandler {
    const NAME: &'static str = "fs_release_lock";
    // S4.7 follow-up (2026-09-11 hardening): arity 2 → 3 adding
    // `holder: Par` at slot 1.  Hard-fork surface bump — the URN
    // arity registration in fs_genesis.rs, the LockToken agent's
    // constructor + release method, every fsReleaseLock! call site
    // in File.rho, and the stream-lifetime lockCell tuple format
    // all move in lockstep.  See auto-memory
    // `fileio_wave4_security_followups.md` § Item 2.
    const ARITY: usize = 3; // (lock_id, holder, ack)

    type Args = FsReleaseLockArgs;

    fn parse_content(args: &[Par]) -> Result<FsReleaseLockArgs, Box<HandlerReply>> {
        let [id_par, holder_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (u64, holder)",
            ));
        };
        let lock_id = match RhoNumber::unapply(id_par) {
            Some(n) if n >= 0 => n as u64,
            _ => {
                return Err(HandlerReply::boxed_err(
                    FSERR_BAD_ARG,
                    "expected (u64, holder)",
                ))
            }
        };
        Ok(FsReleaseLockArgs {
            lock_id,
            holder: holder_par.clone(),
        })
    }

    fn pre_charge_cost() -> crate::rust::interpreter::accounting::costs::Cost {
        costs::fs_release_lock_cost()
    }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsReleaseLockArgs,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            let holder = holder_id_of(&args.holder);
            match ctx
                .handles
                .lock_registry
                .release(LockId::from(args.lock_id), &holder)
            {
                Ok(()) => HandlerReply::ok(ok_bare()),
                Err(le) => HandlerReply::Err(lock_err_reply(le)),
            }
        })
    }
}

#[linkme::distributed_slice(FS_HANDLERS)]
static FS_RELEASE_LOCK_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsReleaseLockHandler as FsHandler>::NAME,
    arity: <FsReleaseLockHandler as FsHandler>::ARITY,
    verifying: <FsReleaseLockHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsReleaseLockHandler>(fs, args)),
    urn_suffix: "releaseLock",
    fixed_channel: FixedChannels::fs_release_lock,
    body_ref: BodyRefs::FS_RELEASE_LOCK,
    family: HandlerFamily::Lock,
};

// -------------------------------------------------------------------
// fs_lock_range — (fd, off, len, mode, holder, cmode, wait)
//                -> [true, lock_id]  (S3.3)
//
// Non-verifying lock acquisition.  Arity-8 tightened by slice-8b
// sub-4 (2026-08-26).  Pre-refactor validated `wait_par` before
// the is_replay short-circuit; my framework unifies content parse
// post-is_replay.  Under determinism assumptions (leader and
// follower see identical args) both paths produce byte-identical
// replies — see wave-3 S3.2 commit message.
// -------------------------------------------------------------------

pub struct FsLockRangeHandler;

pub struct FsLockRangeArgs {
    fd: u64,
    offset: u64,
    length: u64,
    mode: super::lock::LockMode,
    holder: Par,
    wait_policy: WaitPolicy,
}

impl FsHandler for FsLockRangeHandler {
    const NAME: &'static str = "fs_lock_range";
    const ARITY: usize = 8; // (fd, off, len, mode, holder, cmode, wait, ack)

    type Args = FsLockRangeArgs;

    fn parse_content(args: &[Par]) -> Result<FsLockRangeArgs, Box<HandlerReply>> {
        let [fd_par, off_par, len_par, mode_par, holder_par, cmode_par, wait_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (u64, u64, u64>0, String\"r|w\", Par, String, Bool)",
            ));
        };
        // wait_par first — matches pre-refactor's specific error
        // message ordering.
        let wait = match RhoBoolean::unapply(wait_par) {
            Some(b) => b,
            None => {
                return Err(HandlerReply::boxed_err(
                    FSERR_BAD_ARG,
                    "fs_lock_range: wait must be Bool",
                ));
            }
        };
        // cmode validation.  The acquire outcome doesn't currently
        // branch on cmode (step 4 WAL + step 7 unlink gate will);
        // this rejects bad shapes fail-closed.
        if resolve_cmode(cmode_par).is_none() {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "cmode must be String \"oracular\" or \"consensus\"",
            ));
        }
        // Combined args validation — matches pre-refactor's
        // "expected (u64, u64, u64>0, String\"r|w\", Par, String, Bool)"
        // shape.  Any single failure surfaces the combined message.
        let combined_err = || {
            HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (u64, u64, u64>0, String\"r|w\", Par, String, Bool)",
            )
        };
        let fd = RhoNumber::unapply(fd_par).ok_or_else(combined_err)?;
        let off = RhoNumber::unapply(off_par).ok_or_else(combined_err)?;
        let len = RhoNumber::unapply(len_par).ok_or_else(combined_err)?;
        // mode_par must be a String AND resolve to a LockMode.
        RhoString::unapply(mode_par).ok_or_else(combined_err)?;
        let lm = resolve_lock_mode(mode_par).ok_or_else(combined_err)?;
        if !(off >= 0 && len > 0) {
            return Err(combined_err());
        }
        let policy = if wait {
            WaitPolicy::Wait
        } else {
            WaitPolicy::Fail
        };
        Ok(FsLockRangeArgs {
            // Slice 28 CRIT-2: fds are hash-derived u64 bit-patterns;
            // reinterpret without gating on `fd >= 0`.
            fd: fd as u64,
            offset: off as u64,
            length: len as u64,
            mode: lm,
            holder: holder_par.clone(),
            wait_policy: policy,
        })
    }

    fn pre_charge_cost() -> crate::rust::interpreter::accounting::costs::Cost {
        costs::fs_lock_range_cost()
    }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsLockRangeArgs,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            match dev_inode_from_fd_via_table(ctx.handles, args.fd).await {
                Ok(dev_inode) => {
                    let holder = holder_id_of(&args.holder);
                    let deploy = ctx.current_deploy_scope();
                    match ctx.handles.lock_registry.try_acquire_range_wait(
                        dev_inode,
                        args.offset,
                        args.length,
                        args.mode,
                        holder,
                        deploy,
                        args.wait_policy,
                    ) {
                        Ok(AcquireOutcome::Immediate(id)) => HandlerReply::ok(ok_u64(id.as_u64())),
                        Ok(AcquireOutcome::Parked { admit, .. }) => {
                            // S4.8 fix (2026-09-11): park the current
                            // reduction participant while awaiting the
                            // oneshot admit signal.  Without this, the
                            // deterministic_reduction driver's
                            // `frontier_ready` check would refuse to
                            // advance any other participant's intent
                            // (including the release that will wake this
                            // parked admit), causing the deploy to deadlock
                            // until eval-test-source timeout.  See
                            // `deterministic_reduction::park_external_during`
                            // docstring for the mechanism.
                            match crate::rust::interpreter::deterministic_reduction::park_external_during(admit).await {
                                Ok(Ok(id)) => HandlerReply::ok(ok_u64(id.as_u64())),
                                Ok(Err(le)) => HandlerReply::Err(lock_err_reply(le)),
                                Err(_recv_error) => {
                                    HandlerReply::Err(lock_err_reply(LockError::Cancelled))
                                }
                            }
                        }
                        Err(le) => HandlerReply::Err(lock_err_reply(le)),
                    }
                }
                Err((code, msg)) => HandlerReply::err(code, msg),
            }
        })
    }
}

#[linkme::distributed_slice(FS_HANDLERS)]
static FS_LOCK_RANGE_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsLockRangeHandler as FsHandler>::NAME,
    arity: <FsLockRangeHandler as FsHandler>::ARITY,
    verifying: <FsLockRangeHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsLockRangeHandler>(fs, args)),
    urn_suffix: "lockRange",
    fixed_channel: FixedChannels::fs_lock_range,
    body_ref: BodyRefs::FS_LOCK_RANGE,
    family: HandlerFamily::Lock,
};

// -------------------------------------------------------------------
// fs_lock_sequential — (fd, holder, cmode, wait)
//                     -> [true, lock_id]  (S3.3)
//
// Non-verifying sequential-lock acquisition.  Similar shape to
// fs_lock_range but no offset/length/mode slots.
// -------------------------------------------------------------------

pub struct FsLockSequentialHandler;

pub struct FsLockSequentialArgs {
    fd: u64,
    holder: Par,
    wait_policy: WaitPolicy,
}

impl FsHandler for FsLockSequentialHandler {
    const NAME: &'static str = "fs_lock_sequential";
    const ARITY: usize = 5; // (fd, holder, cmode, wait, ack)

    type Args = FsLockSequentialArgs;

    fn parse_content(args: &[Par]) -> Result<FsLockSequentialArgs, Box<HandlerReply>> {
        let [fd_par, holder_par, cmode_par, wait_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (u64, Par, String, Bool)",
            ));
        };
        let wait = match RhoBoolean::unapply(wait_par) {
            Some(b) => b,
            None => {
                return Err(HandlerReply::boxed_err(
                    FSERR_BAD_ARG,
                    "fs_lock_sequential: wait must be Bool",
                ));
            }
        };
        if resolve_cmode(cmode_par).is_none() {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "cmode must be String \"oracular\" or \"consensus\"",
            ));
        }
        let fd = RhoNumber::unapply(fd_par).ok_or_else(|| {
            HandlerReply::boxed_err(FSERR_BAD_ARG, "expected (u64, Par, String, Bool)")
        })?;
        let policy = if wait {
            WaitPolicy::Wait
        } else {
            WaitPolicy::Fail
        };
        Ok(FsLockSequentialArgs {
            // Slice 28 CRIT-2: fds are hash-derived u64 bit-patterns;
            // reinterpret without gating on `fd >= 0`.
            fd: fd as u64,
            holder: holder_par.clone(),
            wait_policy: policy,
        })
    }

    fn pre_charge_cost() -> crate::rust::interpreter::accounting::costs::Cost {
        costs::fs_lock_sequential_cost()
    }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsLockSequentialArgs,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            match dev_inode_from_fd_via_table(ctx.handles, args.fd).await {
                Ok(dev_inode) => {
                    let holder = holder_id_of(&args.holder);
                    let deploy = ctx.current_deploy_scope();
                    match ctx.handles.lock_registry.try_acquire_sequential_wait(
                        dev_inode,
                        holder,
                        deploy,
                        args.wait_policy,
                    ) {
                        Ok(AcquireOutcome::Immediate(id)) => HandlerReply::ok(ok_u64(id.as_u64())),
                        Ok(AcquireOutcome::Parked { admit, .. }) => {
                            // S4.8 fix (2026-09-11): mirror of the
                            // fs_lock_range wrap — park the participant
                            // while awaiting the sequential-lock admit.
                            match crate::rust::interpreter::deterministic_reduction::park_external_during(admit).await {
                                Ok(Ok(id)) => HandlerReply::ok(ok_u64(id.as_u64())),
                                Ok(Err(le)) => HandlerReply::Err(lock_err_reply(le)),
                                Err(_recv_error) => {
                                    HandlerReply::Err(lock_err_reply(LockError::Cancelled))
                                }
                            }
                        }
                        Err(le) => HandlerReply::Err(lock_err_reply(le)),
                    }
                }
                Err((code, msg)) => HandlerReply::err(code, msg),
            }
        })
    }
}

#[linkme::distributed_slice(FS_HANDLERS)]
static FS_LOCK_SEQUENTIAL_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsLockSequentialHandler as FsHandler>::NAME,
    arity: <FsLockSequentialHandler as FsHandler>::ARITY,
    verifying: <FsLockSequentialHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| {
        Box::pin(dispatch_via_trait_owned::<FsLockSequentialHandler>(
            fs, args,
        ))
    },
    urn_suffix: "lockSequential",
    fixed_channel: FixedChannels::fs_lock_sequential,
    body_ref: BodyRefs::FS_LOCK_SEQUENTIAL,
    family: HandlerFamily::Lock,
};

// -------------------------------------------------------------------
// fs_release_all_for_holder — (holder) -> [true, nReleased]  (S3.11)
//
// Non-verifying lock helper.  Deploy-end sweep target.  Called by
// File.close before dispatching fs_close so a File cap that still
// holds locks at close time doesn't strand them until deploy-end
// auto-release fires.
//
// Cancel-first / release-second ordering — same as
// WalDeployScope::drop's B1 fix.  Rationale: `release_all_for_
// holder` internally wakes waiters; reversing order would let a
// same-holder parked waiter get admitted-then-leaked.
// -------------------------------------------------------------------

pub struct FsReleaseAllForHolderHandler;

pub struct FsReleaseAllForHolderArgs {
    holder: Par,
}

impl FsHandler for FsReleaseAllForHolderHandler {
    const NAME: &'static str = "fs_release_all_for_holder";
    const ARITY: usize = 2; // (holder, ack)
                            // Non-verifying: holder is opaque Par (per-cap `this` name);
                            // release/cancel counts are host-local, no cross-validator
                            // verify semantics.

    type Args = FsReleaseAllForHolderArgs;

    fn parse_content(args: &[Par]) -> Result<FsReleaseAllForHolderArgs, Box<HandlerReply>> {
        // `holder` is opaque Par (arbitrary Rholang value hashed to a
        // stable 32-byte HolderId).  No type check needed — any Par
        // shape maps deterministically to a HolderId.
        let [holder_par] = args else {
            return Err(HandlerReply::boxed_err(FSERR_BAD_ARG, "expected (Par)"));
        };
        Ok(FsReleaseAllForHolderArgs {
            holder: holder_par.clone(),
        })
    }

    fn pre_charge_cost() -> crate::rust::interpreter::accounting::costs::Cost {
        costs::fs_release_all_for_holder_cost()
    }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsReleaseAllForHolderArgs,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            let holder = holder_id_of(&args.holder);
            // Slice-8b sub-6 review round-2 (2026-08-12): cancel-first
            // / release-second ordering — same as WalDeployScope::drop
            // B1 fix.  `release_all_for_holder` internally wakes
            // waiters; reversing the order would let a same-holder
            // parked waiter get admitted then leaked when cancel
            // subsequently finds nothing to cancel.
            let cancelled = ctx
                .handles
                .lock_registry
                .cancel_all_waiters_for_holder(&holder);
            let released = ctx.handles.lock_registry.release_all_for_holder(&holder);
            HandlerReply::ok(ok_u64((released + cancelled) as u64))
        })
    }
}

#[linkme::distributed_slice(FS_HANDLERS)]
static FS_RELEASE_ALL_FOR_HOLDER_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsReleaseAllForHolderHandler as FsHandler>::NAME,
    arity: <FsReleaseAllForHolderHandler as FsHandler>::ARITY,
    verifying: <FsReleaseAllForHolderHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| {
        Box::pin(dispatch_via_trait_owned::<FsReleaseAllForHolderHandler>(
            fs, args,
        ))
    },
    urn_suffix: "releaseAllForHolder",
    fixed_channel: FixedChannels::fs_release_all_for_holder,
    body_ref: BodyRefs::FS_RELEASE_ALL_FOR_HOLDER,
    family: HandlerFamily::Lock,
};
