// fs_release_all_for_holder — (holder) -> [true, nReleased]
//
// **Fourth and final Lock-family handler.**  Non-verifying.
// Deploy-end sweep target — called by `File.close` before
// dispatching `fs_close` so a File cap that still holds locks at
// close time doesn't strand them until deploy-end auto-release
// fires.
//
// # Reply shape
//
// `[true, nReleased]` — the u64 count combines released lock
// entries + cancelled waiters.  Rholang callers typically ignore
// the value (the method is called for its side effect, not its
// result), but having a meaningful count helps operator-visibility
// at the Rholang-stub level.
//
// # Cancel-first / release-second ordering (B1 fix discipline)
//
// `release_all_for_holder` internally wakes waiters on the
// per-file FIFO.  If we released FIRST and cancelled SECOND, a
// same-holder parked waiter would get admitted (via the release's
// internal wake) THEN cancelled — leaking a held lock that
// another same-holder path thought was cancelled.
//
// The correct ordering:
//
//   1. `cancel_all_waiters_for_holder` — remove any parked
//      waiters for this holder.  No wake-up signals fire for them.
//   2. `release_all_for_holder` — drop all held entries for this
//      holder; wake the next legitimate waiter in the FIFO (not
//      the just-cancelled same-holder ones).
//
// Same discipline as `WalDeployScope::drop`'s B1 fix (which
// cancels pre-ack waiters before dropping the WAL scope).
//
// # Why the holder arg is opaque Par
//
// `holder` is any Rholang value hashed to a stable 32-byte
// `HolderId` via `holder_id_of`.  No arity check in parse — the
// handler accepts any Par shape.  This matches the lock-acquire
// handlers' convention: the Rholang-level `File`/`LockToken` agent
// chooses the holder discriminator (typically the per-cap
// `this` GPrivate name).

use std::future::Future;
use std::pin::Pin;

use linkme::distributed_slice;
use models::rhoapi::Par;

use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::io::costs::fs_release_all_for_holder_cost;
use crate::rust::interpreter::io::errors::FSERR_BAD_ARG;
use crate::rust::interpreter::io::handler_trait::{
    dispatch_via_trait_owned, FsHandler, FsHandlerEntry, HandlerFamily, HandlerReply, SyscallCtx,
    FS_HANDLERS,
};
use crate::rust::interpreter::io::handlers::helpers::holder_id_of;
use crate::rust::interpreter::io::response::ok_u64;
use crate::rust::interpreter::system_processes::{BodyRefs, FixedChannels};

/// Zero-sized handler type.
pub struct FsReleaseAllForHolderHandler;

/// Parsed args.  `holder` is any Par — no shape check at parse.
pub struct FsReleaseAllForHolderArgs {
    holder: Par,
}

impl FsHandler for FsReleaseAllForHolderHandler {
    const NAME: &'static str = "fs_release_all_for_holder";
    const ARITY: usize = 2; // (holder, ack)
                            // Non-verifying: holder is opaque Par (per-cap `this` name);
                            // release / cancel counts are host-local, no cross-validator
                            // verify semantics.

    type Args = FsReleaseAllForHolderArgs;

    fn parse_content(args: &[Par]) -> Result<FsReleaseAllForHolderArgs, Box<HandlerReply>> {
        // `holder` is opaque Par (arbitrary Rholang value hashed to
        // a stable 32-byte HolderId via `holder_id_of`).  No type
        // check needed — any Par shape maps deterministically to a
        // HolderId.
        let [holder_par] = args else {
            return Err(HandlerReply::boxed_err(FSERR_BAD_ARG, "expected (Par)"));
        };
        Ok(FsReleaseAllForHolderArgs {
            holder: holder_par.clone(),
        })
    }

    fn pre_charge_cost() -> Cost { fs_release_all_for_holder_cost() }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsReleaseAllForHolderArgs,
    ) -> Pin<Box<dyn Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            let holder = holder_id_of(&args.holder);
            // Cancel-first / release-second ordering — see module
            // header on the B1 fix discipline.  Reversing the
            // order lets a same-holder parked waiter get admitted-
            // then-leaked.
            let cancelled = ctx
                .handles
                .lock_registry
                .cancel_all_waiters_for_holder(&holder);
            let released = ctx.handles.lock_registry.release_all_for_holder(&holder);
            HandlerReply::ok(ok_u64((released + cancelled) as u64))
        })
    }
}

/// `#[distributed_slice(FS_HANDLERS)]` registration.
#[distributed_slice(FS_HANDLERS)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::interpreter::rho_type::{RhoNumber, RhoString};

    /// Trait-level constants wire through.  ARITY = 2 (holder +
    /// ack); non-verifying.
    #[test]
    fn consts_wire_through() {
        assert_eq!(
            FsReleaseAllForHolderHandler::NAME,
            "fs_release_all_for_holder"
        );
        assert_eq!(FsReleaseAllForHolderHandler::ARITY, 2);
        assert!(!FsReleaseAllForHolderHandler::VERIFYING);
    }

    /// `parse_content` accepts a String holder.
    #[test]
    fn parse_content_accepts_string_holder() {
        let args = vec![RhoString::create_par("holder-cap".to_string())];
        assert!(FsReleaseAllForHolderHandler::parse_content(&args).is_ok());
    }

    /// `parse_content` accepts a GInt holder (any Par is a valid
    /// holder — hashed to HolderId at dispatch).
    #[test]
    fn parse_content_accepts_gint_holder() {
        let args = vec![RhoNumber::create_par(42)];
        assert!(FsReleaseAllForHolderHandler::parse_content(&args).is_ok());
    }

    /// `parse_content` accepts a bare default Par as holder.
    /// Documents the "any Par" acceptance explicitly.
    #[test]
    fn parse_content_accepts_default_par_holder() {
        let args = vec![Par::default()];
        assert!(FsReleaseAllForHolderHandler::parse_content(&args).is_ok());
    }

    /// `parse_content` rejects wrong arg count (0 or 2+).
    #[test]
    fn parse_content_rejects_wrong_arg_count() {
        let empty: Vec<Par> = vec![];
        let two = vec![
            RhoString::create_par("holder".to_string()),
            RhoString::create_par("extra".to_string()),
        ];
        assert!(FsReleaseAllForHolderHandler::parse_content(&empty).is_err());
        assert!(FsReleaseAllForHolderHandler::parse_content(&two).is_err());
    }

    /// `pre_charge_cost` delegates to
    /// `costs::fs_release_all_for_holder_cost`.
    #[test]
    fn pre_charge_cost_delegates_to_costs_helper() {
        let via_handler = FsReleaseAllForHolderHandler::pre_charge_cost();
        let via_helper = fs_release_all_for_holder_cost();
        assert_eq!(via_handler.value, via_helper.value);
        assert_eq!(via_handler.operation, via_helper.operation);
    }
}
