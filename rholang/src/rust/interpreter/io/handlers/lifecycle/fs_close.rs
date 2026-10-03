// fs_close — (fd) -> [true] | [false, FSERR_BAD_ARG, msg]
//
// Non-verifying lifecycle handler.  Releases a FileHandleTable
// entry on the leader path AND removes the follower's shadow
// FileHandle on the `is_replay` branch (Phase-2 fd-release
// discipline, consensus-observable per `docs/consensus-invariants.md`).
//
// # Phase-2 fd-release (on_replay_side_effect)
//
// Pre-Phase-2 the follower's shadow was metadata-only; Phase-2
// (fileio timestamp 2026-09-01) backs it with a real OS fd under
// Consensus caps, so failing to release on replay leaks OS fds.
// The replay hook opportunistically parses the fd arg and removes
// the shadow — matching pre-trait fs_close behavior:
//
//   - An unparseable fd_par on replay silently no-ops.  The
//     framework still echoes `previous` (leader's cached reply)
//     so leader / follower agree on wire bytes; the only
//     observable difference is a stale shadow entry on the
//     follower (which the deploy-scope sweep eventually cleans up).
//   - This matches [`FsHandler::on_replay_side_effect`]'s discipline:
//     opportunistic parsing on raw args, no `FSERR_BAD_ARG` reply
//     divergence (the leader's cached reply is authoritative).
//
// # HandlerReply ok/err wrapping
//
// Non-blocking async body (no spawn_blocking) — this handler just
// calls `ctx.handles.remove(fd).await` which is a short HashMap
// operation.  Returns `HandlerReply::ok(ok_bare())` unconditionally
// (close is idempotent; removing an unknown fd returns `false` but
// is NOT an error at the wire level — matches pre-trait behavior).
// The telemetry hazard flagged on `fs_quarantine` doesn't apply
// here: this handler has no error path to the Rholang caller.

use std::future::Future;
use std::pin::Pin;

use linkme::distributed_slice;
use models::rhoapi::Par;

use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::io::costs::fs_close_cost;
use crate::rust::interpreter::io::errors::FSERR_BAD_ARG;
use crate::rust::interpreter::io::handler_trait::{
    dispatch_via_trait_owned, FsHandler, FsHandlerEntry, HandlerFamily, HandlerReply, SyscallCtx,
    FS_HANDLERS,
};
use crate::rust::interpreter::io::response::ok_bare;
use crate::rust::interpreter::rho_type::RhoNumber;
use crate::rust::interpreter::system_processes::{BodyRefs, FixedChannels};

/// Zero-sized handler type.
pub struct FsCloseHandler;

/// Parsed args for [`FsCloseHandler`].  `fd` is a u64 bit-pattern
/// — fds on this platform are hash-derived u64 values, so the
/// sign bit carries information and must NOT be gated out via
/// `fd >= 0`.  Reinterpret the Rholang GInt via `fd as u64`.
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
        // Fds are hash-derived u64 bit-patterns; the sign bit
        // carries information, so reinterpret via `fd as u64`
        // rather than gating on `fd >= 0`.  Pin against a future
        // refactor that treated the sign bit as a validity marker.
        Ok(FsCloseArgs { fd: fd as u64 })
    }

    fn pre_charge_cost() -> Cost { fs_close_cost() }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsCloseArgs,
    ) -> Pin<Box<dyn Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            // remove returns `bool` (true = fd was present, false
            // = idempotent no-op).  We discard the bool: close of
            // an unknown fd is NOT a wire-level error — matches
            // pre-trait fs_close behavior.
            let _ = ctx.handles.remove(args.fd).await;
            HandlerReply::ok(ok_bare())
        })
    }

    fn on_replay_side_effect<'a>(
        ctx: SyscallCtx<'a>,
        raw_args: &'a [Par],
        _previous: &'a [Par],
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        // Phase-2 fd-release: opportunistically parse the fd_par
        // and remove the shadow.  Matches pre-trait behavior —
        // an unparseable fd_par on replay silently no-ops (the
        // leader's `previous` reply is still echoed by the
        // framework).  See handler_trait::fs_handler §
        // on_replay_side_effect for why we don't parse-then-fail
        // on replay.
        //
        // The `fd as u64` reinterpretation MUST match
        // `parse_content`'s above (fds are hash-derived u64 bit-
        // patterns; sign bit carries information).  A regression
        // that gated `fd >= 0` on one path but not the other
        // would create leader/follower drift: the leader-path
        // dispatch would reject fds the follower-path replay
        // happily removes (or vice versa).  Grep-audit pin:
        // search for `fd as u64` + ensure every fs_* handler's
        // fd-handling sites agree.
        Box::pin(async move {
            if let [fd_par, ..] = raw_args {
                if let Some(fd) = RhoNumber::unapply(fd_par) {
                    let _ = ctx.handles.remove(fd as u64).await;
                }
            }
        })
    }
}

/// `#[distributed_slice(FS_HANDLERS)]` registration.
#[distributed_slice(FS_HANDLERS)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::interpreter::io::handle_table::FileHandleTable;
    use crate::rust::interpreter::rho_type::RhoString;

    /// Trait-level constants wire through.
    #[test]
    fn consts_wire_through() {
        assert_eq!(FsCloseHandler::NAME, "fs_close");
        assert_eq!(FsCloseHandler::ARITY, 2);
        assert!(!FsCloseHandler::VERIFYING);
    }

    /// `parse_content` success path returns the fd as u64.
    #[test]
    fn parse_content_accepts_positive_fd() {
        let args = vec![RhoNumber::create_par(42)];
        let parsed = FsCloseHandler::parse_content(&args)
            .ok()
            .expect("positive fd parses");
        assert_eq!(parsed.fd, 42u64);
    }

    /// LOAD-BEARING: a negative GInt (fd with the sign bit set)
    /// parses as a u64 bit-pattern, NOT rejected as invalid.
    /// Pin against a future refactor that gated on `fd >= 0` —
    /// which would silently reject half the fd space (fds are
    /// hash-derived u64 values, so the sign bit is a real data
    /// bit, not an invalid-marker).
    #[test]
    fn parse_content_accepts_negative_fd_as_u64_bitpattern() {
        let args = vec![RhoNumber::create_par(-1)];
        let parsed = FsCloseHandler::parse_content(&args)
            .ok()
            .expect("negative fd parses as u64");
        assert_eq!(parsed.fd, u64::MAX); // -1_i64 as u64
    }

    /// `parse_content` rejects wrong arg count.
    #[test]
    fn parse_content_rejects_wrong_arg_count() {
        let empty: Vec<Par> = vec![];
        let two = vec![RhoNumber::create_par(1), RhoNumber::create_par(2)];
        assert!(FsCloseHandler::parse_content(&empty).is_err());
        assert!(FsCloseHandler::parse_content(&two).is_err());
    }

    /// `parse_content` rejects a non-GInt arg (e.g., a String
    /// where an integer fd was expected).
    #[test]
    fn parse_content_rejects_non_int_fd() {
        let args = vec![RhoString::create_par("not a fd".to_string())];
        assert!(FsCloseHandler::parse_content(&args).is_err());
    }

    /// `pre_charge_cost` delegates to `costs::fs_close_cost`.
    /// Golden value pinned at the costs module.
    #[test]
    fn pre_charge_cost_delegates_to_costs_helper() {
        let via_handler = FsCloseHandler::pre_charge_cost();
        let via_helper = fs_close_cost();
        assert_eq!(via_handler.value, via_helper.value);
        assert_eq!(via_handler.operation, via_helper.operation);
    }

    /// `FileHandleTable::remove` on an unknown fd returns `false`
    /// — the dispatch body relies on this idempotency.  Pin that
    /// behavior here so a future tightening (reject unknown fd)
    /// surfaces as a dispatch-test failure before it changes
    /// consensus-observable behavior.
    #[tokio::test]
    async fn file_handle_table_remove_unknown_fd_is_idempotent() {
        let handles = FileHandleTable::new();
        let was_present = handles.remove(0xdeadbeef).await;
        assert!(
            !was_present,
            "unknown fd must return false (not panic / not an error)"
        );
    }
}
