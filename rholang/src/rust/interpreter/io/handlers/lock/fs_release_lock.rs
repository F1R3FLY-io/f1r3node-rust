// fs_release_lock — (lock_id, holder) -> [true]
//                                       | [false, FSERR, msg]
//
// Third Lock-family handler.  Pure LockRegistry op, no filesystem
// syscalls.  Non-verifying.  Idempotent at the Rholang-reply level
// (releasing a non-held lock surfaces `FSERR_CLOSED` via
// `lock_err_reply`) — matches pre-trait behavior.
//
// # Holder-identity check (S4.7 hardening)
//
// The handler takes TWO args (plus ack): the `lock_id` to release
// AND the `holder` Par whose HolderId must match the lock's
// recorded holder.  Prevents a Rholang caller that doesn't hold
// the lock from releasing it on behalf of another holder.
// `LockRegistry::release` enforces this — returns
// `LockError::Closed` if the holder mismatches.
//
// Pre-S4.7-hardening ARITY was 2 (just lock_id + ack); after
// hardening it's 3.  The URN arity registration in fs_genesis.rs,
// the LockToken agent's release method, and every
// `fsReleaseLock!` call site in `File.rho` all move in lockstep
// at hard-fork time.  This slice ports the post-hardening shape.
//
// # Dispatch — pure LockRegistry op
//
// No `dev_inode_from_fd_via_table` call (unlike acquire); the
// LockRegistry indexes releases by `LockId` directly.  No
// `park_external_during` (release doesn't park).  No WAL
// journaling (lock state is per-runtime).
//
// # LockId wire shape
//
// Rholang passes the LockId as a `u64` (GInt with `n >= 0`
// enforced at parse — unlike fds, LockIds are allocator-produced
// and strictly non-negative in `[0, i64::MAX]` per the
// `LockRegistry` contract).

use std::future::Future;
use std::pin::Pin;

use linkme::distributed_slice;
use models::rhoapi::Par;

use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::io::costs::fs_release_lock_cost;
use crate::rust::interpreter::io::errors::FSERR_BAD_ARG;
use crate::rust::interpreter::io::handler_trait::{
    dispatch_via_trait_owned, FsHandler, FsHandlerEntry, HandlerFamily, HandlerReply, SyscallCtx,
    FS_HANDLERS,
};
use crate::rust::interpreter::io::handlers::helpers::{holder_id_of, lock_err_reply};
use crate::rust::interpreter::io::lock::{LockError, LockId};
use crate::rust::interpreter::io::response::ok_bare;
use crate::rust::interpreter::rho_type::RhoNumber;
use crate::rust::interpreter::system_processes::{BodyRefs, FixedChannels};

/// Zero-sized handler type.
pub struct FsReleaseLockHandler;

/// Parsed args.
pub struct FsReleaseLockArgs {
    lock_id: u64,
    holder: Par,
}

impl FsHandler for FsReleaseLockHandler {
    const NAME: &'static str = "fs_release_lock";
    // S4.7 hardening: arity 2 → 3 adding `holder: Par` at slot 1.
    // Hard-fork surface bump — see module header.
    const ARITY: usize = 3; // (lock_id, holder, ack)

    type Args = FsReleaseLockArgs;

    fn parse_content(args: &[Par]) -> Result<FsReleaseLockArgs, Box<HandlerReply>> {
        let [id_par, holder_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (u64, holder)",
            ));
        };
        // LockIds are allocator-produced non-negatives (per
        // registry contract: `[0, i64::MAX]`).  Enforce `n >= 0`
        // at parse — a negative GInt is a Rholang caller bug.
        //
        // Note: differs from fd parsing (fds reinterpret the sign
        // bit as a data bit since they're hash-derived u64 bit-
        // patterns).  LockIds have a tighter range per the
        // allocator contract; silently accepting the sign bit
        // would mint a LockId outside the allocator's
        // claimable range.
        let lock_id = match RhoNumber::unapply(id_par) {
            Some(n) if n >= 0 => n as u64,
            _ => {
                return Err(HandlerReply::boxed_err(
                    FSERR_BAD_ARG,
                    "expected (u64, holder)",
                ));
            }
        };
        Ok(FsReleaseLockArgs {
            lock_id,
            holder: holder_par.clone(),
        })
    }

    fn pre_charge_cost() -> Cost { fs_release_lock_cost() }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsReleaseLockArgs,
    ) -> Pin<Box<dyn Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            let holder = holder_id_of(&args.holder);
            // Dev's `LockId::try_from(u64)` enforces the
            // allocator contract `[0, i64::MAX]`.  Our parse
            // already enforced `n >= 0`, but we're going through
            // `as u64` so the sign check at parse was in `i64`;
            // the try_from re-checks the `[0, i64::MAX]` bound.
            // On out-of-range (impossible in practice given the
            // parse constraint, but defense-in-depth) surface
            // `LockError::BadArg` via `lock_err_reply`.
            let lock_id = match LockId::try_from(args.lock_id) {
                Ok(id) => id,
                Err(_) => return HandlerReply::Err(lock_err_reply(LockError::BadArg)),
            };
            match ctx.handles.lock_registry.release(lock_id, &holder) {
                Ok(()) => HandlerReply::ok(ok_bare()),
                Err(le) => HandlerReply::Err(lock_err_reply(le)),
            }
        })
    }
}

/// `#[distributed_slice(FS_HANDLERS)]` registration.
#[distributed_slice(FS_HANDLERS)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::interpreter::rho_type::RhoString;

    fn valid_args() -> Vec<Par> {
        vec![
            RhoNumber::create_par(42), // lock_id
            RhoString::create_par("holder".to_string()),
        ]
    }

    /// Trait-level constants wire through.  Non-verifying;
    /// ARITY = 3 (lock_id + holder + ack).
    #[test]
    fn consts_wire_through() {
        assert_eq!(FsReleaseLockHandler::NAME, "fs_release_lock");
        assert_eq!(FsReleaseLockHandler::ARITY, 3);
        assert!(!FsReleaseLockHandler::VERIFYING);
    }

    /// `parse_content` success returns both lock_id and holder.
    #[test]
    fn parse_content_accepts_valid_tuple() {
        let parsed = FsReleaseLockHandler::parse_content(&valid_args())
            .ok()
            .expect("valid tuple parses");
        assert_eq!(parsed.lock_id, 42u64);
    }

    /// `parse_content` accepts lock_id = 0 (allocator can mint
    /// zero as the first LockId).
    #[test]
    fn parse_content_accepts_zero_lock_id() {
        let mut args = valid_args();
        args[0] = RhoNumber::create_par(0);
        let parsed = FsReleaseLockHandler::parse_content(&args)
            .ok()
            .expect("zero id parses");
        assert_eq!(parsed.lock_id, 0u64);
    }

    /// LOAD-BEARING: negative lock_id is REJECTED (unlike fds,
    /// which accept the sign bit as data).  LockIds are allocator-
    /// produced in `[0, i64::MAX]`; a negative GInt is a Rholang
    /// caller bug.  Pin against a regression that silently cast
    /// via `n as u64` (which would mint LockIds outside the
    /// allocator's claimable range).
    #[test]
    fn parse_content_rejects_negative_lock_id() {
        let mut args = valid_args();
        args[0] = RhoNumber::create_par(-1);
        assert!(FsReleaseLockHandler::parse_content(&args).is_err());
    }

    /// `parse_content` rejects wrong arg count (1 or 3+).
    #[test]
    fn parse_content_rejects_wrong_arg_count() {
        let one = valid_args().into_iter().take(1).collect::<Vec<_>>();
        let mut three = valid_args();
        three.push(RhoString::create_par("extra".to_string()));
        assert!(FsReleaseLockHandler::parse_content(&one).is_err());
        assert!(FsReleaseLockHandler::parse_content(&three).is_err());
    }

    /// `parse_content` rejects a non-GInt lock_id.
    #[test]
    fn parse_content_rejects_non_int_lock_id() {
        let mut args = valid_args();
        args[0] = RhoString::create_par("not-an-id".to_string());
        assert!(FsReleaseLockHandler::parse_content(&args).is_err());
    }

    /// `pre_charge_cost` delegates to `costs::fs_release_lock_cost`.
    #[test]
    fn pre_charge_cost_delegates_to_costs_helper() {
        let via_handler = FsReleaseLockHandler::pre_charge_cost();
        let via_helper = fs_release_lock_cost();
        assert_eq!(via_handler.value, via_helper.value);
        assert_eq!(via_handler.operation, via_helper.operation);
    }
}
