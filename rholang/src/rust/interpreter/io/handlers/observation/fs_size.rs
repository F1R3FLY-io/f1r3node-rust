// fs_size — (fd) -> [true, n_bytes] | [false, FSERR_*, msg]
//
// Verifying observation handler.  Returns the fd's current size
// via `libc::fstat(raw_fd)`.  Second verifying handler in the
// migration (after fs_seek, slice 4.16); shares the same cmode-
// resolution + verifying-replay plumbing.
//
// # Cmode source — fd-based
//
// `resolve_replay_cmode` reads the fd's shadow cmode via
// `with_mut(fd, |h| h.cmode)` — same shape as fs_seek's.  The
// per-fd cmode was captured at `fs_open` time and lives on the
// `FileHandle` for the fd's lifetime; `fs_size` doesn't take a
// cmode arg, so this is the only source.
//
// # WAL state-read journaling (Wave 6 scope)
//
// Fileio's fs_size calls `journal_state_read_via_table(handles,
// cmode, WalOp::Size, canon_path, reply, ack, None)` inside its
// `journal` method.  That helper (in `handlers_helpers.rs` on
// fileio) writes a `WalOp::Size` entry recording the leader's
// observation so the follower can anchor its replay against the
// same WAL event.
//
// `journal_state_read_via_table` has NOT been ported to dev yet
// — it's part of the handlers_helpers slice that fs_open +
// fs_stat + other verifying handlers also need.  Under Wave 4
// + 5 with NoopMetering, WAL journaling is a dormant surface
// (the WAL exists but no cost-accounted-rho replay reconciliation
// consumes it yet).  The verifying path still works via
// `verify_reply_hash_matches_cached` (dispatcher step 7) —
// follower re-executes `fstat` and verifies its fresh reply hash
// against the leader's cached reply hash directly.
//
// This handler's `journal` method is a documented no-op for Wave
// 4 + 5.  Wave 6 lands the helper + flips this to the full
// journal call.
//
// # park_external_during deferral
//
// Same deferral as sibling handlers — raw
// `tokio::task::spawn_blocking`.  Wave 5 scope.

use std::future::Future;
use std::os::fd::AsRawFd;
use std::pin::Pin;

use linkme::distributed_slice;
use models::rhoapi::Par;
use tokio::task::spawn_blocking;

use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::io::costs::fs_size_cost;
use crate::rust::interpreter::io::errors::{
    io_err_code, join_err_abort, FSERR_BAD_ARG, FSERR_CLOSED,
};
use crate::rust::interpreter::io::handler_trait::{
    dispatch_via_trait_owned, FsHandler, FsHandlerEntry, HandlerFamily, HandlerReply, JournalPath,
    SyscallCtx, FS_HANDLERS,
};
use crate::rust::interpreter::io::path::io_msg_scrub;
use crate::rust::interpreter::io::response::ok_u64;
use crate::rust::interpreter::io::ConsensusMode;
use crate::rust::interpreter::rho_type::RhoNumber;
use crate::rust::interpreter::system_processes::{BodyRefs, FixedChannels};

/// Zero-sized handler type.
pub struct FsSizeHandler;

/// Parsed args for [`FsSizeHandler`].
pub struct FsSizeArgs {
    fd: u64,
}

impl FsHandler for FsSizeHandler {
    const NAME: &'static str = "fs_size";
    const ARITY: usize = 2; // (fd, ack)
    const VERIFYING: bool = true;

    type Args = FsSizeArgs;

    fn parse_content(args: &[Par]) -> Result<FsSizeArgs, Box<HandlerReply>> {
        let [fd_par] = args else {
            // Framework-dispatched path: unreachable in practice —
            // dispatcher's step-2 ARITY check runs first.  Direct
            // test-call path: `parse_content_rejects_wrong_arg_count`
            // exercises this arm.
            return Err(HandlerReply::boxed_err(FSERR_BAD_ARG, "expected GInt fd"));
        };
        let fd = RhoNumber::unapply(fd_par)
            .ok_or_else(|| HandlerReply::boxed_err(FSERR_BAD_ARG, "expected GInt fd"))?;
        Ok(FsSizeArgs { fd: fd as u64 })
    }

    fn pre_charge_cost() -> Cost { fs_size_cost() }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsSizeArgs,
    ) -> Pin<Box<dyn Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            let file_arc = match ctx.handles.raw_fd(args.fd).await {
                Some(f) => f,
                None => {
                    // u64 fd formatting — matches fs_close / fs_flush
                    // / fs_tell discipline.  (See fs_seek's module
                    // header for the cross-handler fd-formatting
                    // audit — fs_seek uses i64 for pre-trait byte
                    // identity; fs_size uses u64 per its own pre-
                    // trait format.)
                    return HandlerReply::err(FSERR_CLOSED, format!("unknown fd {}", args.fd));
                }
            };
            let r = spawn_blocking(move || {
                let raw_fd = file_arc.as_raw_fd();
                // SAFETY: `libc::stat` is a POD C struct that
                // `zeroed()` can validly initialize (integer
                // fields).  `raw_fd` derives from `file_arc:
                // Arc<File>` whose lifetime spans this closure.
                // `fstat` reads `&mut sb` without retaining it
                // past the call.
                unsafe {
                    let mut sb: libc::stat = std::mem::zeroed();
                    if libc::fstat(raw_fd, &mut sb) < 0 {
                        Err(std::io::Error::last_os_error())
                    } else {
                        Ok(sb.st_size as u64)
                    }
                }
            })
            .await;
            match r {
                Err(je) => join_err_abort(je),
                Ok(Err(e)) => HandlerReply::err(io_err_code(&e), io_msg_scrub(&e)),
                Ok(Ok(n)) => HandlerReply::ok(ok_u64(n)),
            }
        })
    }

    /// Verifying-handler cmode resolution — fd-based.  Same shape
    /// as fs_seek's: reads the fd's shadow `cmode` field via
    /// `with_mut`.  `None` → dispatcher's Oracular echo path.
    fn resolve_replay_cmode<'a>(
        ctx: SyscallCtx<'a>,
        raw_args: &'a [Par],
    ) -> Pin<Box<dyn Future<Output = Option<ConsensusMode>> + Send + 'a>> {
        Box::pin(async move {
            let [fd_par, ..] = raw_args else {
                return None;
            };
            let fd = RhoNumber::unapply(fd_par)?;
            ctx.handles.with_mut(fd as u64, |h| h.cmode).await
        })
    }

    /// WAL state-read journal — Wave 6 scope.
    ///
    /// Fileio calls `journal_state_read_via_table(handles,
    /// cmode, WalOp::Size, canon_path, reply, ack, None)` here
    /// to record the leader's observation.  That helper lives
    /// in `handlers_helpers.rs` on fileio and has NOT been
    /// ported to dev — it's part of the handlers_helpers slice
    /// that other verifying handlers (fs_stat, fs_exists, fs_read,
    /// etc.) also need.
    ///
    /// Under Wave 4 + 5 (NoopMetering), WAL journaling is a
    /// dormant surface — the verifying path still works via
    /// `verify_reply_hash_matches_cached` (dispatcher step 7).
    /// This method stays as a no-op until the helper slice lands.
    fn journal<'a>(
        _ctx: SyscallCtx<'a>,
        _raw_args: &'a [Par],
        _path: JournalPath<'a>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        // Wave 4 + 5: no-op.
        // Wave 6: journal_state_read_via_table(ctx.handles, cmode,
        // WalOp::Size, canon_path, path.produce_reply(), ctx.ack,
        // None) — see module docstring.
        Box::pin(async {})
    }
}

/// `#[distributed_slice(FS_HANDLERS)]` registration.
#[distributed_slice(FS_HANDLERS)]
static FS_SIZE_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsSizeHandler as FsHandler>::NAME,
    arity: <FsSizeHandler as FsHandler>::ARITY,
    verifying: <FsSizeHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsSizeHandler>(fs, args)),
    urn_suffix: "size",
    fixed_channel: FixedChannels::fs_size,
    body_ref: BodyRefs::FS_SIZE,
    family: HandlerFamily::Observation,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::interpreter::rho_type::RhoString;

    /// Trait-level constants wire through — including VERIFYING=true.
    #[test]
    fn consts_wire_through() {
        assert_eq!(FsSizeHandler::NAME, "fs_size");
        assert_eq!(FsSizeHandler::ARITY, 2);
        assert!(FsSizeHandler::VERIFYING, "fs_size is a verifying handler");
    }

    #[test]
    fn parse_content_accepts_positive_fd() {
        let args = vec![RhoNumber::create_par(42)];
        let parsed = FsSizeHandler::parse_content(&args).ok().expect("parses");
        assert_eq!(parsed.fd, 42u64);
    }

    /// Grep-audit u64 bit-pattern pin.
    #[test]
    fn parse_content_accepts_negative_fd_as_u64_bitpattern() {
        let args = vec![RhoNumber::create_par(-1)];
        let parsed = FsSizeHandler::parse_content(&args).ok().expect("parses");
        assert_eq!(parsed.fd, u64::MAX);
    }

    #[test]
    fn parse_content_rejects_wrong_arg_count() {
        assert!(FsSizeHandler::parse_content(&[]).is_err());
        assert!(FsSizeHandler::parse_content(&[
            RhoNumber::create_par(1),
            RhoNumber::create_par(2)
        ])
        .is_err());
    }

    #[test]
    fn parse_content_rejects_non_int_fd() {
        let args = vec![RhoString::create_par("not a fd".to_string())];
        assert!(FsSizeHandler::parse_content(&args).is_err());
    }

    #[test]
    fn pre_charge_cost_delegates_to_costs_helper() {
        let via_handler = FsSizeHandler::pre_charge_cost();
        let via_helper = fs_size_cost();
        assert_eq!(via_handler.value, via_helper.value);
        assert_eq!(via_handler.operation, via_helper.operation);
    }
}
