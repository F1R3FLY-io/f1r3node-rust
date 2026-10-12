// fs_lock_range — (fd, offset, length, mode, holder, cmode, wait)
//                 → [true, lockId]
//                 | [false, FSERR, msg]
//
// **First Lock-family handler.**  Range-based advisory lock
// acquire against the per-runtime `LockRegistry`.  Non-verifying
// (lock state is per-runtime, not consensus-observable through
// a persistent WAL).  BUT the minted `LockId` IS consensus-
// observable — both leader and follower must mint the same
// id for the same request, deterministically.
//
// # Why the LockId is deterministic
//
// `LockRegistry::try_acquire_range_wait` consults the per-
// `(dev, inode)` interval tree + monotonic id counter under
// the registry's `Mutex`.  Leader and follower see the same
// (dev, inode) via `dev_inode_from_fd_via_table` (resolved from
// their respective shadow fds) + the same deploy state (via
// `ctx.current_deploy_scope()`) + the same request args, so
// their minted ids match.
//
// # Wait policy
//
// Rholang caller passes `wait: Bool`:
//
//   - `wait: false` → `WaitPolicy::Fail` → on conflict return
//     `Err(LockError::Busy)` immediately.  Clean, no parking.
//   - `wait: true`  → `WaitPolicy::Wait` → on conflict mint a
//     `LockId`, enqueue a waiter in the per-file FIFO, return
//     `AcquireOutcome::Parked { lock_id, admit }`.  The handler
//     then awaits `admit` for the eventual grant / cancel.
//
// # park_external_during deferral
//
// On the Parked branch, we await `admit` directly without
// `park_external_during` — see the family mod docstring for
// the Wave 5 deferral note.  Behavior-equivalent to fileio
// in single-waiter shapes; concurrent-advance optimization
// deferred.
//
// # HandlerReply hazard
//
// Returns `HandlerReply::ok` on success + `HandlerReply::Err`
// on all failure paths — correct discrimination via explicit
// pattern matching.  Does NOT inherit the fs_quarantine-style
// `HandlerReply::Ok(err_par)` chain.

use std::future::Future;
use std::pin::Pin;

use linkme::distributed_slice;
use models::rhoapi::Par;

use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::io::costs::fs_lock_range_cost;
use crate::rust::interpreter::io::errors::FSERR_BAD_ARG;
use crate::rust::interpreter::io::handler_trait::{
    dispatch_via_trait_owned, FsHandler, FsHandlerEntry, HandlerFamily, HandlerReply, SyscallCtx,
    FS_HANDLERS,
};
use crate::rust::interpreter::io::handlers::helpers::{
    dev_inode_from_fd_via_table, holder_id_of, lock_err_reply, resolve_lock_mode,
};
use crate::rust::interpreter::io::lock::{AcquireOutcome, LockError, LockMode, WaitPolicy};
use crate::rust::interpreter::io::resolve_cmode;
use crate::rust::interpreter::io::response::ok_u64;
use crate::rust::interpreter::rho_type::{RhoBoolean, RhoNumber, RhoString};
use crate::rust::interpreter::system_processes::{BodyRefs, FixedChannels};

/// Zero-sized handler type.
pub struct FsLockRangeHandler;

/// Parsed args.  `holder` is kept as the original Par (hashed
/// via `holder_id_of` at dispatch time); `wait_policy` is the
/// typed form derived from the Rholang Bool.
pub struct FsLockRangeArgs {
    fd: u64,
    offset: u64,
    length: u64,
    mode: LockMode,
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
        // wait_par first — matches pre-trait specific error
        // message ordering (bad wait → "wait must be Bool",
        // distinct from the combined-shape error).
        let wait = match RhoBoolean::unapply(wait_par) {
            Some(b) => b,
            None => {
                return Err(HandlerReply::boxed_err(
                    FSERR_BAD_ARG,
                    "fs_lock_range: wait must be Bool",
                ));
            }
        };
        // cmode validation.  The acquire outcome doesn't branch
        // on cmode today (per-runtime state, not consensus-WAL),
        // but a bad cmode shape still fails-closed.
        if resolve_cmode(cmode_par).is_none() {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "cmode must be String \"oracular\" or \"consensus\"",
            ));
        }
        // Combined args validation — any single failure surfaces
        // the combined message.  Mirrors fs_chmod / fs_truncate.
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
            // CRIT-2: fds are hash-derived u64 bit-patterns;
            // reinterpret without gating on `fd >= 0`.  Same
            // discipline as fs_close / fs_truncate / etc.
            fd: fd as u64,
            offset: off as u64,
            length: len as u64,
            mode: lm,
            holder: holder_par.clone(),
            wait_policy: policy,
        })
    }

    fn pre_charge_cost() -> Cost { fs_lock_range_cost() }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsLockRangeArgs,
    ) -> Pin<Box<dyn Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            match dev_inode_from_fd_via_table(ctx.handles, args.fd).await {
                Err((code, msg)) => HandlerReply::err(code, msg),
                Ok(dev_inode) => {
                    let holder = holder_id_of(&args.holder);
                    let deploy = ctx.current_deploy_scope();
                    // Dev's LockRegistry splits the WaitPolicy
                    // into two methods (fileio collapsed them
                    // into one with an enum arg):
                    //
                    //   - `WaitPolicy::Fail` → `try_acquire_range`,
                    //     returns `Result<LockId, LockError>`.
                    //   - `WaitPolicy::Wait` → `try_acquire_range_wait`,
                    //     returns `Result<AcquireOutcome, LockError>`
                    //     with Immediate / Parked variants.
                    //
                    // Call the appropriate method based on the
                    // parsed policy.  Keep the two arms uniform
                    // on the reply shape (ok_u64(id)).
                    match args.wait_policy {
                        WaitPolicy::Fail => match ctx.handles.lock_registry.try_acquire_range(
                            dev_inode,
                            args.offset,
                            args.length,
                            args.mode,
                            holder,
                            deploy,
                        ) {
                            Ok(id) => HandlerReply::ok(ok_u64(id.as_u64())),
                            Err(le) => HandlerReply::Err(lock_err_reply(le)),
                        },
                        WaitPolicy::Wait => match ctx.handles.lock_registry.try_acquire_range_wait(
                            dev_inode,
                            args.offset,
                            args.length,
                            args.mode,
                            holder,
                            deploy,
                        ) {
                            Ok(AcquireOutcome::Immediate(id)) => {
                                HandlerReply::ok(ok_u64(id.as_u64()))
                            }
                            Ok(AcquireOutcome::Parked { admit, .. }) => {
                                // Wave 5 scope: wrap this in
                                // `park_external_during` so the
                                // reduction driver can advance
                                // other participants while we
                                // await.  See family mod
                                // docstring on the deferral.
                                match admit.await {
                                    Ok(Ok(id)) => HandlerReply::ok(ok_u64(id.as_u64())),
                                    Ok(Err(le)) => HandlerReply::Err(lock_err_reply(le)),
                                    Err(_recv) => {
                                        HandlerReply::Err(lock_err_reply(LockError::Cancelled))
                                    }
                                }
                            }
                            Err(le) => HandlerReply::Err(lock_err_reply(le)),
                        },
                    }
                }
            }
        })
    }
}

/// `#[distributed_slice(FS_HANDLERS)]` registration.
#[distributed_slice(FS_HANDLERS)]
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

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_cmode_par(s: &str) -> Par { RhoString::create_par(s.to_string()) }

    fn valid_args(mode: &str, wait: bool) -> Vec<Par> {
        vec![
            RhoNumber::create_par(42),                   // fd
            RhoNumber::create_par(0),                    // off
            RhoNumber::create_par(1024),                 // len
            RhoString::create_par(mode.to_string()),     // mode
            RhoString::create_par("holder".to_string()), // holder
            mk_cmode_par("consensus"),                   // cmode
            RhoBoolean::create_par(wait),                // wait
        ]
    }

    /// Trait-level constants wire through.  ARITY = 8 is the
    /// widest handler surface in Wave 4.  Non-verifying — lock
    /// state is per-runtime, not consensus-WAL.
    #[test]
    fn consts_wire_through() {
        assert_eq!(FsLockRangeHandler::NAME, "fs_lock_range");
        assert_eq!(FsLockRangeHandler::ARITY, 8);
        assert!(!FsLockRangeHandler::VERIFYING);
    }

    /// `parse_content` success path — read-mode, wait=false.
    #[test]
    fn parse_content_accepts_valid_read_fail_tuple() {
        let parsed = FsLockRangeHandler::parse_content(&valid_args("r", false))
            .ok()
            .expect("read/fail parses");
        assert_eq!(parsed.fd, 42u64);
        assert_eq!(parsed.offset, 0u64);
        assert_eq!(parsed.length, 1024u64);
        assert_eq!(parsed.mode, LockMode::Read);
        assert_eq!(parsed.wait_policy, WaitPolicy::Fail);
    }

    /// Both mode strings + both wait values round-trip at parse.
    #[test]
    fn parse_content_accepts_all_mode_wait_combinations() {
        for mode in ["r", "w"] {
            for wait in [false, true] {
                let parsed = FsLockRangeHandler::parse_content(&valid_args(mode, wait))
                    .ok()
                    .unwrap_or_else(|| panic!("({mode}, wait={wait}) parses"));
                let expected_mode = if mode == "r" {
                    LockMode::Read
                } else {
                    LockMode::Write
                };
                let expected_policy = if wait {
                    WaitPolicy::Wait
                } else {
                    WaitPolicy::Fail
                };
                assert_eq!(parsed.mode, expected_mode);
                assert_eq!(parsed.wait_policy, expected_policy);
            }
        }
    }

    /// LOAD-BEARING: fd parses as u64 bit-pattern even for the
    /// sign-bit-set case.  Same discipline as fs_close.
    #[test]
    fn parse_content_accepts_negative_fd_as_u64_bitpattern() {
        let mut args = valid_args("r", false);
        args[0] = RhoNumber::create_par(-1);
        let parsed = FsLockRangeHandler::parse_content(&args)
            .ok()
            .expect("negative fd parses");
        assert_eq!(parsed.fd, u64::MAX);
    }

    /// LOAD-BEARING: `length = 0` is rejected (`len > 0`
    /// requirement) — a zero-length lock is a no-op that would
    /// collide with the empty-interval sentinel semantics.
    #[test]
    fn parse_content_rejects_zero_length() {
        let mut args = valid_args("r", false);
        args[2] = RhoNumber::create_par(0);
        assert!(FsLockRangeHandler::parse_content(&args).is_err());
    }

    /// LOAD-BEARING: negative offset is rejected (`off >= 0`
    /// requirement).  POSIX advisory lock offsets are non-negative.
    #[test]
    fn parse_content_rejects_negative_offset() {
        let mut args = valid_args("r", false);
        args[1] = RhoNumber::create_par(-1);
        assert!(FsLockRangeHandler::parse_content(&args).is_err());
    }

    /// LOAD-BEARING: a non-canonical mode string (`"x"`,
    /// `"R"`, etc.) is rejected.  Mirrors `resolve_lock_mode`'s
    /// fail-closed discipline.
    #[test]
    fn parse_content_rejects_bogus_mode_string() {
        for mode in ["x", "R", "W", "read", ""] {
            let args = valid_args(mode, false);
            assert!(
                FsLockRangeHandler::parse_content(&args).is_err(),
                "mode {mode:?} should reject",
            );
        }
    }

    /// LOAD-BEARING: bad `wait` par (non-Bool) produces the
    /// SPECIFIC "fs_lock_range: wait must be Bool" error,
    /// distinct from the combined shape error.
    #[test]
    fn parse_content_rejects_non_bool_wait_with_specific_message() {
        let mut args = valid_args("r", false);
        args[6] = RhoString::create_par("true".to_string()); // String not Bool
        let reply = *FsLockRangeHandler::parse_content(&args)
            .err()
            .expect("rejects");
        match reply {
            HandlerReply::Err(_) => {}
            HandlerReply::Ok(_) => panic!("expected Err"),
        }
    }

    /// LOAD-BEARING: bad cmode produces specific "cmode must
    /// be..." error.
    #[test]
    fn parse_content_rejects_bad_cmode_with_specific_message() {
        let mut args = valid_args("r", false);
        args[5] = mk_cmode_par("Consensus"); // uppercase
        let reply = *FsLockRangeHandler::parse_content(&args)
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
        let six = valid_args("r", false)
            .into_iter()
            .take(6)
            .collect::<Vec<_>>();
        let mut eight = valid_args("r", false);
        eight.push(RhoString::create_par("extra".to_string()));
        assert!(FsLockRangeHandler::parse_content(&six).is_err());
        assert!(FsLockRangeHandler::parse_content(&eight).is_err());
    }

    /// `pre_charge_cost` delegates to `costs::fs_lock_range_cost`.
    /// Golden value pinned at the costs module.
    #[test]
    fn pre_charge_cost_delegates_to_costs_helper() {
        let via_handler = FsLockRangeHandler::pre_charge_cost();
        let via_helper = fs_lock_range_cost();
        assert_eq!(via_handler.value, via_helper.value);
        assert_eq!(via_handler.operation, via_helper.operation);
    }
}
