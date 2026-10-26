// fs_entries_stream_close — (fd) -> [true]
//
// Non-verifying stream lifecycle handler.  Mirrors `fs_close`'s
// Phase-2 fd-release pattern against the directory-entries stream
// table ([`DirHandleTable`](super::super::super::dir_handle_table)):
//
//   - Leader path: `ctx.handles.dir_handles.remove(fd).await`
//     releases the stream entry.  Idempotent — removing an unknown
//     fd returns `false` at the table level but is NOT a wire-level
//     error (matches `fs_close` and pre-trait `fs_entries_stream_close`
//     behavior).
//   - Replay path: `on_replay_side_effect` opportunistically parses
//     the fd arg and removes the follower's shadow dir_handle so
//     leader / follower `dir_handles` tables converge.  An
//     unparseable `fd_par` on replay silently no-ops — the framework
//     still echoes `previous` (leader's cached reply) so the wire
//     bytes stay in sync.
//
// # First Stream-family handler
//
// Opens the Stream family on dev.  Chosen first because it has the
// smallest dependency surface in the family: no WAL journaling, no
// `safe_descend_verified`, no cost supplement, no cmode gating.
// The sibling `fs_entries_stream_open` / `_next` handlers that
// land later carry those pieces.
//
// # Why fd parses as unsigned
//
// Fds are hash-derived u64 bit-patterns (see `fs_close` for the
// full discussion).  The sign bit carries information, so
// `parse_content` reinterprets via `fd as u64` rather than gating
// on `fd >= 0` — a tightening to `>= 0` would silently reject half
// the fd space.  The replay hook does the same cast on `raw_args`
// so leader / follower agree on which fd to remove.

use std::future::Future;
use std::pin::Pin;

use linkme::distributed_slice;
use models::rhoapi::Par;

use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::io::costs::fs_entries_stream_close_cost;
use crate::rust::interpreter::io::errors::FSERR_BAD_ARG;
use crate::rust::interpreter::io::handler_trait::{
    dispatch_via_trait_owned, FsHandler, FsHandlerEntry, HandlerFamily, HandlerReply, SyscallCtx,
    FS_HANDLERS,
};
use crate::rust::interpreter::io::response::ok_bare;
use crate::rust::interpreter::rho_type::RhoNumber;
use crate::rust::interpreter::system_processes::{BodyRefs, FixedChannels};

/// Zero-sized handler type.
pub struct FsEntriesStreamCloseHandler;

/// Parsed args.  `fd` is a u64 bit-pattern — see module docstring.
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

    fn pre_charge_cost() -> Cost { fs_entries_stream_close_cost() }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsEntriesStreamCloseArgs,
    ) -> Pin<Box<dyn Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            // Discard the bool return — close of an unknown stream
            // fd is NOT a wire-level error (idempotent).  Matches
            // fs_close + pre-trait fs_entries_stream_close behavior.
            let _ = ctx.handles.dir_handles.remove(args.fd).await;
            HandlerReply::ok(ok_bare())
        })
    }

    fn on_replay_side_effect<'a>(
        ctx: SyscallCtx<'a>,
        raw_args: &'a [Par],
        _previous: &'a [Par],
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        // Opportunistic parse — unparseable raw args on replay are
        // a no-op (not a FSERR_BAD_ARG divergence).  See
        // handler_trait::fs_handler § on_replay_side_effect for why.
        Box::pin(async move {
            if let [fd_par, ..] = raw_args {
                if let Some(fd) = RhoNumber::unapply(fd_par) {
                    let _ = ctx.handles.dir_handles.remove(fd as u64).await;
                }
            }
        })
    }
}

/// `#[distributed_slice(FS_HANDLERS)]` registration — populated at
/// link time, consumed by the yet-to-land `rho_runtime.rs` wiring
/// that walks `FS_HANDLERS.iter()`.
#[distributed_slice(FS_HANDLERS)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::interpreter::io::handle_table::FileHandleTable;
    use crate::rust::interpreter::rho_type::RhoString;

    /// Trait-level constants wire through.
    #[test]
    fn consts_wire_through() {
        assert_eq!(FsEntriesStreamCloseHandler::NAME, "fs_entries_stream_close");
        assert_eq!(FsEntriesStreamCloseHandler::ARITY, 2);
        assert!(!FsEntriesStreamCloseHandler::VERIFYING);
    }

    /// `parse_content` success path returns the fd as u64.
    #[test]
    fn parse_content_accepts_positive_fd() {
        let args = vec![RhoNumber::create_par(42)];
        let parsed = FsEntriesStreamCloseHandler::parse_content(&args)
            .ok()
            .expect("positive fd parses");
        assert_eq!(parsed.fd, 42u64);
    }

    /// LOAD-BEARING: a negative GInt (fd with the sign bit set)
    /// parses as a u64 bit-pattern.  Mirrors the `fs_close` pin —
    /// stream fds share the same hash-derived u64 bit-pattern
    /// discipline, so a future refactor that gated `fd >= 0` on one
    /// but not the other would create leader/follower drift.
    #[test]
    fn parse_content_accepts_negative_fd_as_u64_bitpattern() {
        let args = vec![RhoNumber::create_par(-1)];
        let parsed = FsEntriesStreamCloseHandler::parse_content(&args)
            .ok()
            .expect("negative fd parses as u64");
        assert_eq!(parsed.fd, u64::MAX);
    }

    /// `parse_content` rejects wrong arg count.
    #[test]
    fn parse_content_rejects_wrong_arg_count() {
        let empty: Vec<Par> = vec![];
        let two = vec![RhoNumber::create_par(1), RhoNumber::create_par(2)];
        assert!(FsEntriesStreamCloseHandler::parse_content(&empty).is_err());
        assert!(FsEntriesStreamCloseHandler::parse_content(&two).is_err());
    }

    /// `parse_content` rejects a non-GInt arg.
    #[test]
    fn parse_content_rejects_non_int_fd() {
        let args = vec![RhoString::create_par("not a fd".to_string())];
        assert!(FsEntriesStreamCloseHandler::parse_content(&args).is_err());
    }

    /// `pre_charge_cost` delegates to `costs::fs_entries_stream_close_cost`.
    /// Golden value pinned at the costs module (the alias-to-
    /// fs_close_cost documented at `fs_entries_stream_close_is_fs_close`
    /// in `io/costs.rs` tests).
    #[test]
    fn pre_charge_cost_delegates_to_costs_helper() {
        let via_handler = FsEntriesStreamCloseHandler::pre_charge_cost();
        let via_helper = fs_entries_stream_close_cost();
        assert_eq!(via_handler.value, via_helper.value);
        assert_eq!(via_handler.operation, via_helper.operation);
    }

    /// `DirHandleTable::remove` on an unknown fd returns `false`
    /// — the dispatch body relies on this idempotency.  Pin here so
    /// a future tightening (reject unknown fd) surfaces as a
    /// dispatch-test failure before it changes consensus-observable
    /// behavior.  Same contract as `fs_close`'s FileHandleTable pin.
    #[tokio::test]
    async fn dir_handle_table_remove_unknown_fd_is_idempotent() {
        let handles = FileHandleTable::new();
        let was_present = handles.dir_handles.remove(0xdeadbeef).await;
        assert!(
            !was_present,
            "unknown stream fd must return false (not panic / not an error)"
        );
    }
}
