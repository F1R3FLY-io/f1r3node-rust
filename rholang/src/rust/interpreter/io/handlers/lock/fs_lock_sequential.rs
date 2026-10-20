// fs_lock_sequential — (fd, holder, cmode, wait)
//                      → [true, lockId]
//                      | [false, FSERR, msg]
//
// Second Lock-family handler.  Whole-file sequential (exclusive)
// lock acquire — the entire fd becomes locked; no range slots.
// Non-verifying; LockId consensus-observable (same determinism
// discipline as fs_lock_range).
//
// # Why no range slots
//
// Sequential locks are a mutex over the entire file identity
// (dev, inode).  Rholang callers that need range-based locking
// use `fs_lock_range` with explicit `(offset, length, mode)`;
// `fs_lock_sequential` is the file-granularity shortcut.
//
// # Conflict semantics
//
// Under `WaitPolicy::Fail`, an attempt against a currently-held
// sequential lock OR any held range on the same inode returns
// `LockError::Busy`.  The exact rules live in
// `LockRegistry::sequential_conflicts` — same source for leader
// and follower, so conflict detection is deterministic.
//
// # Wait policy
//
// Same split as `fs_lock_range` (PR #642):
//
//   - `WaitPolicy::Fail` → `try_acquire_sequential`, returns
//     `Result<LockId, LockError>`.
//   - `WaitPolicy::Wait` → `try_acquire_sequential_wait`, returns
//     `Result<AcquireOutcome, LockError>` with Immediate / Parked
//     variants.
//
// # park_external_during deferral
//
// Same Wave 5 deferral as `fs_lock_range` — `admit.await` runs
// directly without `park_external_during`.  See family mod
// docstring.

use std::future::Future;
use std::pin::Pin;

use linkme::distributed_slice;
use models::rhoapi::Par;

use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::io::costs::fs_lock_sequential_cost;
use crate::rust::interpreter::io::errors::FSERR_BAD_ARG;
use crate::rust::interpreter::io::handler_trait::{
    dispatch_via_trait_owned, FsHandler, FsHandlerEntry, HandlerFamily, HandlerReply, SyscallCtx,
    FS_HANDLERS,
};
use crate::rust::interpreter::io::handlers::helpers::{
    dev_inode_from_fd_via_table, holder_id_of, lock_err_reply,
};
use crate::rust::interpreter::io::lock::{AcquireOutcome, LockError, WaitPolicy};
use crate::rust::interpreter::io::resolve_cmode;
use crate::rust::interpreter::io::response::ok_u64;
use crate::rust::interpreter::rho_type::{RhoBoolean, RhoNumber};
use crate::rust::interpreter::system_processes::{BodyRefs, FixedChannels};

/// Zero-sized handler type.
pub struct FsLockSequentialHandler;

/// Parsed args — simpler than fs_lock_range (no offset / length /
/// mode slots).  `holder` is kept as the original Par (hashed via
/// `holder_id_of` at dispatch time).
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
        // wait_par first — same specific-error-ordering as
        // fs_lock_range.
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
            // CRIT-2 fd-bit-pattern discipline — see fs_close /
            // fs_lock_range.
            fd: fd as u64,
            holder: holder_par.clone(),
            wait_policy: policy,
        })
    }

    fn pre_charge_cost() -> Cost { fs_lock_sequential_cost() }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsLockSequentialArgs,
    ) -> Pin<Box<dyn Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            match dev_inode_from_fd_via_table(ctx.handles, args.fd).await {
                Err((code, msg)) => HandlerReply::err(code, msg),
                Ok(dev_inode) => {
                    let holder = holder_id_of(&args.holder);
                    let deploy = ctx.current_deploy_scope();
                    // Dev splits sequential acquire into two
                    // methods (same as range) — branch on policy
                    // and call the appropriate one.
                    match args.wait_policy {
                        WaitPolicy::Fail => {
                            match ctx
                                .handles
                                .lock_registry
                                .try_acquire_sequential(dev_inode, holder, deploy)
                            {
                                Ok(id) => HandlerReply::ok(ok_u64(id.as_u64())),
                                Err(le) => HandlerReply::Err(lock_err_reply(le)),
                            }
                        }
                        WaitPolicy::Wait => {
                            match ctx
                                .handles
                                .lock_registry
                                .try_acquire_sequential_wait(dev_inode, holder, deploy)
                            {
                                Ok(AcquireOutcome::Immediate(id)) => {
                                    HandlerReply::ok(ok_u64(id.as_u64()))
                                }
                                Ok(AcquireOutcome::Parked { admit, .. }) => {
                                    // Wave 5 scope: park_external_during.
                                    // See family mod docstring.
                                    match admit.await {
                                        Ok(Ok(id)) => HandlerReply::ok(ok_u64(id.as_u64())),
                                        Ok(Err(le)) => HandlerReply::Err(lock_err_reply(le)),
                                        Err(_recv) => {
                                            HandlerReply::Err(lock_err_reply(LockError::Cancelled))
                                        }
                                    }
                                }
                                Err(le) => HandlerReply::Err(lock_err_reply(le)),
                            }
                        }
                    }
                }
            }
        })
    }
}

/// `#[distributed_slice(FS_HANDLERS)]` registration.
#[distributed_slice(FS_HANDLERS)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::interpreter::rho_type::RhoString;

    fn mk_cmode_par(s: &str) -> Par { RhoString::create_par(s.to_string()) }

    fn valid_args(wait: bool) -> Vec<Par> {
        vec![
            RhoNumber::create_par(42),                   // fd
            RhoString::create_par("holder".to_string()), // holder
            mk_cmode_par("consensus"),                   // cmode
            RhoBoolean::create_par(wait),                // wait
        ]
    }

    /// Trait-level constants wire through.  ARITY = 5 (simpler
    /// than fs_lock_range's 8); non-verifying.
    #[test]
    fn consts_wire_through() {
        assert_eq!(FsLockSequentialHandler::NAME, "fs_lock_sequential");
        assert_eq!(FsLockSequentialHandler::ARITY, 5);
        assert!(!FsLockSequentialHandler::VERIFYING);
    }

    /// `parse_content` success for both wait values.
    #[test]
    fn parse_content_accepts_both_wait_values() {
        for wait in [false, true] {
            let parsed = FsLockSequentialHandler::parse_content(&valid_args(wait))
                .ok()
                .unwrap_or_else(|| panic!("wait={wait} parses"));
            assert_eq!(parsed.fd, 42u64);
            let expected = if wait {
                WaitPolicy::Wait
            } else {
                WaitPolicy::Fail
            };
            assert_eq!(parsed.wait_policy, expected);
        }
    }

    /// LOAD-BEARING: fd parses as u64 bit-pattern (sign bit = data).
    #[test]
    fn parse_content_accepts_negative_fd_as_u64_bitpattern() {
        let mut args = valid_args(false);
        args[0] = RhoNumber::create_par(-1);
        let parsed = FsLockSequentialHandler::parse_content(&args)
            .ok()
            .expect("negative fd parses");
        assert_eq!(parsed.fd, u64::MAX);
    }

    /// LOAD-BEARING: non-Bool wait → specific message.
    #[test]
    fn parse_content_rejects_non_bool_wait_with_specific_message() {
        let mut args = valid_args(false);
        args[3] = RhoString::create_par("true".to_string());
        let reply = *FsLockSequentialHandler::parse_content(&args)
            .err()
            .expect("rejects");
        match reply {
            HandlerReply::Err(_) => {}
            HandlerReply::Ok(_) => panic!("expected Err"),
        }
    }

    /// LOAD-BEARING: bad cmode → specific message.
    #[test]
    fn parse_content_rejects_bad_cmode_with_specific_message() {
        let mut args = valid_args(false);
        args[2] = mk_cmode_par("Consensus"); // uppercase
        let reply = *FsLockSequentialHandler::parse_content(&args)
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
        let three = valid_args(false).into_iter().take(3).collect::<Vec<_>>();
        let mut five = valid_args(false);
        five.push(RhoString::create_par("extra".to_string()));
        assert!(FsLockSequentialHandler::parse_content(&three).is_err());
        assert!(FsLockSequentialHandler::parse_content(&five).is_err());
    }

    /// `parse_content` rejects a non-GInt fd (e.g., String).
    #[test]
    fn parse_content_rejects_non_int_fd() {
        let mut args = valid_args(false);
        args[0] = RhoString::create_par("not-a-fd".to_string());
        assert!(FsLockSequentialHandler::parse_content(&args).is_err());
    }

    /// `pre_charge_cost` delegates to
    /// `costs::fs_lock_sequential_cost`.
    #[test]
    fn pre_charge_cost_delegates_to_costs_helper() {
        let via_handler = FsLockSequentialHandler::pre_charge_cost();
        let via_helper = fs_lock_sequential_cost();
        assert_eq!(via_handler.value, via_helper.value);
        assert_eq!(via_handler.operation, via_helper.operation);
    }
}
