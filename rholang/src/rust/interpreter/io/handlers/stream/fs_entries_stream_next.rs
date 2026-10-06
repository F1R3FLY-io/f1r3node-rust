// fs_entries_stream_next — (streamFd) -> [true, entryRecord]
//                                       | [false, "EOS"]
//                                       | [false, FSERR, msg]
//
// Third and final Stream-family handler.  Non-verifying stream
// advance.  **First handler on dev to activate
// `post_reply_supplement`** — the two-event cost accounting slot
// that pairs with `pre_charge_cost` to produce `setup + per-entry`
// cost billing.
//
// # Two-event cost
//
// Matches bulk `fs_entries`:
//
//   - `pre_charge_cost()` = `FS_ENTRIES_SETUP` = 50 (setup).
//   - `post_reply_supplement(reply)` = per-entry supplement with
//     `n = 1` on `[true, entryRecord]` or `n = 0` on EOS / error.
//     Uses `reserve_incremental_primitive` (framework auto-
//     dispatches) because `n=0` legitimately produces a zero-weight
//     cost — the non-incremental `reserve_primitive` would return
//     a `BugFoundError` on a zero cost.
//
// # Phase-2 ban (upstream)
//
// Consensus caps are rejected by `fs_entries_stream_open` at parse
// time (PR #637), so under normal flow no Consensus stream fd ever
// reaches `_next`.  The handler keeps `journal_state_read_via_table`
// calls in place anyway for structural parity + future ban lift
// (would need shard-committed collation order).
//
// # DIR* access + MutexGuard lifetime
//
// `DirHandle.iter: Mutex<Option<DirIter>>` serializes concurrent
// `next` calls on the same stream.  The dispatch body:
//
//   1. Fetches an `Arc<DirHandle>` via `dir_handles.get(fd)`.
//   2. Holds a lock on `handle.iter` across the `spawn_blocking`
//      boundary.
//   3. Passes the DIR* address via `usize` (raw pointers are
//      not `Send`); the MutexGuard keeps the address live
//      across the `.await`.
//
// Shadow handles (from `on_replay_side_effect`) carry
// `iter: Mutex<None>` — the `None` arm of the match returns
// `FSERR_CLOSED` ("shadow stream handle has no iterator").
//
// # HandlerReply wrapping
//
// Returns `HandlerReply::Ok(reply_par)` on BOTH success and error
// paths — the framework's `post_reply_supplement` classifier
// (`reply_is_ok`) inspects the Par's head boolean, not the Rust
// wrapper variant.  Matches the pre-trait wire shape.  Future
// consumers of `HandlerReply::is_ok` (reserved for metrics / WAL-
// Failure journaling per the trait docstring) would need the Err
// variant for error replies; filed as a structural concern on
// `fs_quarantine`.
//
// # Journal-on-replay (structural parity)
//
// `on_replay_side_effect` journals the cached `previous` reply if
// the fd's shadow carries `cmode = Consensus`.  Under the Phase-2
// ban this is unreachable (no Consensus shadow dir fds), but the
// journal call keeps structural parity so a future ban lift
// doesn't require handler-level changes — only the parse-time
// rejection drops.

use std::future::Future;
use std::pin::Pin;

use linkme::distributed_slice;
use models::rhoapi::Par;

use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::io::costs::{
    fs_entries_stream_next_cost, fs_entries_stream_per_entry_supplement_cost,
};
use crate::rust::interpreter::io::errors::{FSERR_BAD_ARG, FSERR_CLOSED};
use crate::rust::interpreter::io::handler_trait::{
    dispatch_via_trait_owned, spawn_blocking_par, FsHandler, FsHandlerEntry, HandlerFamily,
    HandlerReply, SyscallCtx, FS_HANDLERS,
};
use crate::rust::interpreter::io::handlers::helpers::{
    journal_state_read_via_table, readdir_one_entry, reply_is_ok,
};
use crate::rust::interpreter::io::response::err;
use crate::rust::interpreter::io::wal::WalOp;
use crate::rust::interpreter::rho_type::RhoNumber;
use crate::rust::interpreter::system_processes::{BodyRefs, FixedChannels};

/// Zero-sized handler type.
pub struct FsEntriesStreamNextHandler;

/// Parsed args.
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

    fn pre_charge_cost() -> Cost { fs_entries_stream_next_cost() }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsEntriesStreamNextArgs,
    ) -> Pin<Box<dyn Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            // Hold the Arc so we can re-consult (cmode, canon_path)
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
                        // Pass the DIR* address across the
                        // `spawn_blocking` boundary via `usize`
                        // (raw pointers are not `Send`).  The
                        // MutexGuard `iter_lock` is held across the
                        // `.await` below, keeping the address live
                        // for the duration of the blocking call.
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
            // Journal the leader's reply if the handle is
            // Consensus.  `journal_state_read_via_table` self-
            // guards on Oracular (no-op), so this call is cheap
            // under the Phase-2 ban (Oracular-only in practice).
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
            // Wrap into `HandlerReply::Ok` on BOTH success and
            // error paths — see module header on HandlerReply
            // wrapping discipline.
            HandlerReply::Ok(reply_par)
        })
    }

    fn on_replay_side_effect<'a>(
        ctx: SyscallCtx<'a>,
        raw_args: &'a [Par],
        previous: &'a [Par],
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        // Journal the cached reply if the fd's shadow reports
        // Consensus.  Under the Phase-2 ban (fs_entries_stream_open
        // rejects Consensus at parse) this branch is unreachable
        // in normal flow.  Kept for structural parity.
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

    fn post_reply_supplement(reply: &[Par]) -> Option<Cost> {
        // Two-branch supplement — see module header on the
        // two-event cost shape.  Uses
        // `reserve_incremental_primitive` (framework auto-
        // dispatches) because `n=0` legitimately produces a
        // zero-weight cost that `reserve_primitive` would reject.
        let n = if reply_is_ok(reply) { 1 } else { 0 };
        Some(fs_entries_stream_per_entry_supplement_cost(n))
    }
}

/// `#[distributed_slice(FS_HANDLERS)]` registration.
#[distributed_slice(FS_HANDLERS)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::interpreter::io::response::{err_eos, ok_bare};
    use crate::rust::interpreter::rho_type::RhoString;

    /// Trait-level constants wire through.
    #[test]
    fn consts_wire_through() {
        assert_eq!(FsEntriesStreamNextHandler::NAME, "fs_entries_stream_next");
        assert_eq!(FsEntriesStreamNextHandler::ARITY, 2);
        assert!(!FsEntriesStreamNextHandler::VERIFYING);
    }

    /// `parse_content` success path returns the fd as u64.
    #[test]
    fn parse_content_accepts_positive_fd() {
        let args = vec![RhoNumber::create_par(42)];
        let parsed = FsEntriesStreamNextHandler::parse_content(&args)
            .ok()
            .expect("positive fd parses");
        assert_eq!(parsed.fd, 42u64);
    }

    /// LOAD-BEARING: negative GInt (sign bit set) parses as u64
    /// bit-pattern — same stream-fd discipline as
    /// `fs_entries_stream_close`.  A regression that gated
    /// `fd >= 0` on either handler but not the other would
    /// create leader/follower drift.
    #[test]
    fn parse_content_accepts_negative_fd_as_u64_bitpattern() {
        let args = vec![RhoNumber::create_par(-1)];
        let parsed = FsEntriesStreamNextHandler::parse_content(&args)
            .ok()
            .expect("negative fd parses as u64");
        assert_eq!(parsed.fd, u64::MAX);
    }

    /// `parse_content` rejects wrong arg count (0 or 2+).
    #[test]
    fn parse_content_rejects_wrong_arg_count() {
        let empty: Vec<Par> = vec![];
        let two = vec![RhoNumber::create_par(1), RhoNumber::create_par(2)];
        assert!(FsEntriesStreamNextHandler::parse_content(&empty).is_err());
        assert!(FsEntriesStreamNextHandler::parse_content(&two).is_err());
    }

    /// `parse_content` rejects a non-GInt arg (e.g., a String).
    #[test]
    fn parse_content_rejects_non_int_fd() {
        let args = vec![RhoString::create_par("not a fd".to_string())];
        assert!(FsEntriesStreamNextHandler::parse_content(&args).is_err());
    }

    /// `pre_charge_cost` delegates to
    /// `costs::fs_entries_stream_next_cost` (= 50, matches
    /// `fs_entries_stream_open` setup).
    #[test]
    fn pre_charge_cost_delegates_to_costs_helper() {
        let via_handler = FsEntriesStreamNextHandler::pre_charge_cost();
        let via_helper = fs_entries_stream_next_cost();
        assert_eq!(via_handler.value, via_helper.value);
        assert_eq!(via_handler.operation, via_helper.operation);
    }

    /// LOAD-BEARING: `post_reply_supplement` on an `[true, entry]`
    /// reply charges the n=1 supplement.  Pin against a
    /// regression that always returned n=0 (which would miss
    /// per-entry cost on real reads).
    #[test]
    fn post_reply_supplement_charges_n_1_for_ok_entry() {
        let reply = [ok_bare()]; // `[true]` — reply_is_ok = true
        let got = FsEntriesStreamNextHandler::post_reply_supplement(&reply)
            .expect("supplement returns Some");
        let want = fs_entries_stream_per_entry_supplement_cost(1);
        assert_eq!(got.value, want.value);
        assert_eq!(got.operation, want.operation);
    }

    /// LOAD-BEARING: `post_reply_supplement` on an EOS reply
    /// charges the n=0 supplement (zero-weight cost; framework
    /// auto-dispatches reserve_incremental_primitive which
    /// accepts zero).
    #[test]
    fn post_reply_supplement_charges_n_0_for_eos() {
        let reply = [err_eos()];
        let got = FsEntriesStreamNextHandler::post_reply_supplement(&reply)
            .expect("supplement returns Some");
        let want = fs_entries_stream_per_entry_supplement_cost(0);
        assert_eq!(got.value, want.value);
    }

    /// LOAD-BEARING: `post_reply_supplement` on an err reply
    /// charges the n=0 supplement (same as EOS).
    #[test]
    fn post_reply_supplement_charges_n_0_for_err() {
        use crate::rust::interpreter::io::errors::FSERR_IO;
        let reply = [err(FSERR_IO, "disk error")];
        let got = FsEntriesStreamNextHandler::post_reply_supplement(&reply)
            .expect("supplement returns Some");
        let want = fs_entries_stream_per_entry_supplement_cost(0);
        assert_eq!(got.value, want.value);
    }
}
