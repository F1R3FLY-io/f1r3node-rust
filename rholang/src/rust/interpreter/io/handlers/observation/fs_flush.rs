// fs_flush — (fd) -> [true] | [false, FSERR_CLOSED, msg] | [false, FSERR_IO, msg]
//
// Non-verifying observation handler.  Runs `libc::fsync(raw_fd)`
// inside a tokio blocking task so the fsync (which can block for
// many milliseconds on spinning disks) doesn't stall the reactor.
// Returns `[true]` on success; `FSERR_CLOSED` if the fd is unknown
// to the FileHandleTable; `FSERR_IO` (via `io_err_code`) if fsync
// itself failed.
//
// # Why fs_flush first among observation handlers
//
// Simplest observation handler — no path resolution, no cmode
// gating, no WAL journaling, no shadow-position tracking, no
// verify matrix.  Serves as the shape pin for subsequent
// observation handlers (fs_tell, fs_seek) that follow the same
// `raw_fd → spawn_blocking(libc) → io_err_code` pattern.
//
// # Why non-verifying
//
// `fsync` is a durability hint, not a value-returning observation
// — the reply is binary success/failure, so there's no "value" to
// verify on replay.  Follower on-disk-vs-page-cache state
// converges via WAL + snapshot replay, not by re-executing
// `fsync` on the follower.  The dispatcher's step-4 is_replay
// short-circuit tautologically echoes the leader's cached reply
// on replay — the follower NEVER runs this handler's dispatch
// body on `is_replay = true`.
//
// # park_external_during deferral
//
// Fileio wraps the `spawn_blocking` call through
// `deterministic_reduction::park_external_during(...)` so the
// reduction driver can advance other participants while the fsync
// runs.  That `deterministic_reduction.rs` module is 1,437 LOC on
// fileio and has not yet been ported to dev — same Wave 5 deferral
// as `spawn_blocking_par` (slice 4.8).  Under Wave 4 + 5 the raw
// `tokio::task::spawn_blocking` call runs without reduction-driver
// parking; consensus-observable behavior is unaffected (fsync is
// not a wire-format event).  When `park_external_during` lands,
// wrap this call site through it — one-line change.
//
// # JoinError discipline
//
// Panics inside the blocking task propagate as JoinError, which
// this handler surfaces via [`join_err_abort`] — same fail-closed
// discipline as [`spawn_blocking_par`](super::super::super::handler_trait::spawn_blocking_par)
// but inlined here because the closure returns `io::Result<()>`,
// not `Par` (so `spawn_blocking_par`'s `-> Par` shape doesn't fit).

use std::future::Future;
use std::os::fd::AsRawFd;
use std::pin::Pin;

use linkme::distributed_slice;
use models::rhoapi::Par;
use tokio::task::spawn_blocking;

use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::io::costs::fs_flush_cost;
use crate::rust::interpreter::io::errors::{
    io_err_code, join_err_abort, FSERR_BAD_ARG, FSERR_CLOSED,
};
use crate::rust::interpreter::io::handler_trait::{
    dispatch_via_trait_owned, FsHandler, FsHandlerEntry, HandlerFamily, HandlerReply, SyscallCtx,
    FS_HANDLERS,
};
use crate::rust::interpreter::io::path::io_msg_scrub;
use crate::rust::interpreter::io::response::ok_bare;
use crate::rust::interpreter::rho_type::RhoNumber;
use crate::rust::interpreter::system_processes::{BodyRefs, FixedChannels};

/// Zero-sized handler type.
pub struct FsFlushHandler;

/// Parsed args for [`FsFlushHandler`].  `fd` is a u64 bit-pattern
/// — the same discipline as [`FsCloseHandler`](super::super::lifecycle::fs_close::FsCloseHandler),
/// pinned by that handler's u64-bit-pattern test.
pub struct FsFlushArgs {
    fd: u64,
}

impl FsHandler for FsFlushHandler {
    const NAME: &'static str = "fs_flush";
    const ARITY: usize = 2; // (fd, ack)

    type Args = FsFlushArgs;

    fn parse_content(args: &[Par]) -> Result<FsFlushArgs, Box<HandlerReply>> {
        let [fd_par] = args else {
            // Framework-dispatched path: unreachable in practice —
            // the dispatcher's step-2 ARITY check runs first and
            // rejects with `illegal_argument_error(NAME)`.  Direct
            // test-call path: `parse_content_rejects_wrong_arg_count`
            // exercises this arm by calling the method with
            // non-ARITY slices.  Both futures depend on this arm
            // existing.
            return Err(HandlerReply::boxed_err(FSERR_BAD_ARG, "expected GInt fd"));
        };
        let fd = RhoNumber::unapply(fd_par)
            .ok_or_else(|| HandlerReply::boxed_err(FSERR_BAD_ARG, "expected GInt fd"))?;
        // `fd as u64` reinterpretation matches fs_close discipline.
        Ok(FsFlushArgs { fd: fd as u64 })
    }

    fn pre_charge_cost() -> Cost { fs_flush_cost() }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsFlushArgs,
    ) -> Pin<Box<dyn Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            // Look up the Arc<File> for the fd.  `raw_fd` returns
            // `None` for unknown fds OR shadow handles (follower's
            // replay-only entry with `file: None`).  Shadow handles
            // never reach this dispatch path (the dispatcher's
            // step-4 is_replay short-circuit echoes `previous`
            // before calling dispatch on non-verifying handlers).
            let file_arc = match ctx.handles.raw_fd(args.fd).await {
                Some(f) => f,
                None => {
                    return HandlerReply::err(FSERR_CLOSED, format!("unknown fd {}", args.fd));
                }
            };
            // Blocking fsync off the reactor.  park_external_during
            // is deferred (see module docstring); the raw
            // spawn_blocking call runs without reduction-driver
            // parking.  JoinError propagates to join_err_abort —
            // same fail-closed discipline as spawn_blocking_par.
            let r = spawn_blocking(move || {
                let raw_fd = file_arc.as_raw_fd();
                // SAFETY: `raw_fd` is derived from `file_arc:
                // Arc<File>` whose lifetime spans this closure; the
                // fd is open for the syscall.  `fsync` accepts any
                // integer and returns -1 with errno on invalid fd
                // rather than UB.
                unsafe {
                    if libc::fsync(raw_fd) < 0 {
                        Err(std::io::Error::last_os_error())
                    } else {
                        Ok(())
                    }
                }
            })
            .await;
            match r {
                Err(je) => join_err_abort(je),
                Ok(Err(e)) => HandlerReply::err(io_err_code(&e), io_msg_scrub(&e)),
                Ok(Ok(())) => HandlerReply::ok(ok_bare()),
            }
        })
    }
}

/// `#[distributed_slice(FS_HANDLERS)]` registration.
#[distributed_slice(FS_HANDLERS)]
static FS_FLUSH_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsFlushHandler as FsHandler>::NAME,
    arity: <FsFlushHandler as FsHandler>::ARITY,
    verifying: <FsFlushHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsFlushHandler>(fs, args)),
    urn_suffix: "flush",
    fixed_channel: FixedChannels::fs_flush,
    body_ref: BodyRefs::FS_FLUSH,
    family: HandlerFamily::Observation,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::interpreter::rho_type::RhoString;

    /// Trait-level constants wire through.
    #[test]
    fn consts_wire_through() {
        assert_eq!(FsFlushHandler::NAME, "fs_flush");
        assert_eq!(FsFlushHandler::ARITY, 2);
        assert!(!FsFlushHandler::VERIFYING);
    }

    /// `parse_content` success path — positive fd.
    #[test]
    fn parse_content_accepts_positive_fd() {
        let args = vec![RhoNumber::create_par(42)];
        let parsed = FsFlushHandler::parse_content(&args)
            .ok()
            .expect("positive fd parses");
        assert_eq!(parsed.fd, 42u64);
    }

    /// Same u64 bit-pattern discipline as fs_close.  Pin here so
    /// grep-audit for "fd as u64" catches every fd-taking handler.
    #[test]
    fn parse_content_accepts_negative_fd_as_u64_bitpattern() {
        let args = vec![RhoNumber::create_par(-1)];
        let parsed = FsFlushHandler::parse_content(&args)
            .ok()
            .expect("negative fd parses as u64");
        assert_eq!(parsed.fd, u64::MAX);
    }

    /// `parse_content` rejects wrong arg count.
    #[test]
    fn parse_content_rejects_wrong_arg_count() {
        let empty: Vec<Par> = vec![];
        let two = vec![RhoNumber::create_par(1), RhoNumber::create_par(2)];
        assert!(FsFlushHandler::parse_content(&empty).is_err());
        assert!(FsFlushHandler::parse_content(&two).is_err());
    }

    /// `parse_content` rejects a non-GInt fd arg.
    #[test]
    fn parse_content_rejects_non_int_fd() {
        let args = vec![RhoString::create_par("not a fd".to_string())];
        assert!(FsFlushHandler::parse_content(&args).is_err());
    }

    /// `pre_charge_cost` delegates to `costs::fs_flush_cost`.
    #[test]
    fn pre_charge_cost_delegates_to_costs_helper() {
        let via_handler = FsFlushHandler::pre_charge_cost();
        let via_helper = fs_flush_cost();
        assert_eq!(via_handler.value, via_helper.value);
        assert_eq!(via_handler.operation, via_helper.operation);
    }
}
