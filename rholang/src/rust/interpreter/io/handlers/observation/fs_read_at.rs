// fs_read_at — (fd, off, n) -> [true, bytes] | [false, FSERR_*, msg]
//
// Verifying observation, length-parameterized cost.  Same shape
// as fs_read with an added offset arg; uses `libc::pread` under
// the hood (does NOT advance OS fd position per POSIX pread
// semantics).  Sixth verifying handler, fourth fd-based, second
// length-parameterized.
//
// # Key difference from fs_read: NO shadow-position advance
//
// POSIX `libc::pread(fd, buf, n, off)` performs the read at the
// supplied offset WITHOUT modifying the fd's current position.
// This is the whole point of `pread` vs. `read` — concurrent
// positional reads from the same fd don't race for the fd's
// position.
//
// Consequence: fs_read_at's `journal` method DOES journal the
// read (WAL stub, same deferral as fs_read) but does NOT advance
// the shadow position.  See
// [`verify::FdPositionMutator`](crate::rust::interpreter::io::verify::FdPositionMutator)
// — pread is deliberately excluded from the enum of
// position-mutating handlers.  Pin: a regression that added
// position advance to this handler would silently drift shadow
// from the actual OS fd state.
//
// # Shared read_impl_via_table (slice 4.20)
//
// Dispatch is a one-line delegation to
// [`read_impl_via_table(handles, fd, n, Some(off))`](
// crate::rust::interpreter::io::handlers::helpers::read_impl_via_table).
// The helper branches on `offset: Some(off)` to use `libc::pread`
// instead of `libc::read`.  The quota-ordering + fd-lookup +
// FSERR fan-out are shared with fs_read.
//
// # Journal: same WAL stub + offset threaded through
//
// Wave 6 target: `journal_read_via_table(handles, fd, bytes,
// Some(off), ack)` on success paths; `journal_read_divergence_via_table(
// handles, fd, Some(off), ack)` on VerifyDivergence.  Same
// deferral as fs_read; the offset is threaded through so the
// Wave 6 slice can land journaling without touching this
// handler's dispatch structure.

use std::future::Future;
use std::pin::Pin;

use linkme::distributed_slice;
use models::rhoapi::Par;

use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::io::costs::fs_read_at_cost;
use crate::rust::interpreter::io::errors::FSERR_BAD_ARG;
use crate::rust::interpreter::io::handler_trait::{
    dispatch_via_trait_owned, FsHandler, FsHandlerEntry, HandlerFamily, HandlerReply, JournalPath,
    SyscallCtx, FS_HANDLERS,
};
use crate::rust::interpreter::io::handlers::helpers::read_impl_via_table;
use crate::rust::interpreter::io::response::extract_ok_bytes;
use crate::rust::interpreter::io::ConsensusMode;
use crate::rust::interpreter::rho_type::RhoNumber;
use crate::rust::interpreter::system_processes::{BodyRefs, FixedChannels};

/// Zero-sized handler type.
pub struct FsReadAtHandler;

/// Parsed args for [`FsReadAtHandler`].  `off` + `n` both stored
/// as `u64` after a parse-time non-negative check.
pub struct FsReadAtArgs {
    fd: u64,
    off: u64,
    n: u64,
}

impl FsHandler for FsReadAtHandler {
    const NAME: &'static str = "fs_read_at";
    const ARITY: usize = 4; // (fd, off, n, ack)
    const VERIFYING: bool = true;

    type Args = FsReadAtArgs;

    fn parse_content(args: &[Par]) -> Result<FsReadAtArgs, Box<HandlerReply>> {
        let [fd_par, off_par, n_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (fd:GInt, off:GInt>=0, n:GInt>=0)",
            ));
        };
        match (
            RhoNumber::unapply(fd_par),
            RhoNumber::unapply(off_par),
            RhoNumber::unapply(n_par),
        ) {
            // LOAD-BEARING double guard: `off >= 0` AND `n >= 0`.
            // Without the off >= 0 guard, a negative off would
            // reinterpret as u64::MAX via `off as u64`, then
            // libc::pread with a negative off_t argument would
            // return EINVAL — WRONG error class (would surface as
            // FSERR_IO instead of FSERR_BAD_ARG).  Fd is NOT
            // guarded (sign bit is a legitimate hash-derived
            // bit-pattern — see fs_close discipline).
            (Some(fd), Some(off), Some(n)) if off >= 0 && n >= 0 => Ok(FsReadAtArgs {
                fd: fd as u64,
                off: off as u64,
                n: n as u64,
            }),
            _ => Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (fd:GInt, off:GInt>=0, n:GInt>=0)",
            )),
        }
    }

    fn pre_charge_cost() -> Cost { fs_read_at_cost(0) }

    /// Length-parameterized cost — `n_par` is at slot 2 for
    /// fs_read_at (fd, off, n) — one slot further than fs_read's
    /// (fd, n).  A regression that read `raw_args[1]` instead would
    /// pre-charge on the offset value, not the byte count —
    /// consensus-observable cost drift.
    fn pre_charge_incremental(raw_args: &[Par]) -> Option<Cost> {
        let requested_bytes: u64 = raw_args
            .get(2)
            .and_then(RhoNumber::unapply)
            .and_then(|n| u64::try_from(n).ok())
            .unwrap_or(0);
        Some(fs_read_at_cost(requested_bytes))
    }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsReadAtArgs,
    ) -> Pin<Box<dyn Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            // Thin delegation to the shared read helper.
            // `offset = Some(args.off)` dispatches to `libc::pread`
            // (positional, no OS-fd-position mutation).
            let par = read_impl_via_table(ctx.handles, args.fd, args.n, Some(args.off)).await;
            // Same HandlerReply::ok wrapping as fs_quarantine /
            // fs_exists / fs_stat / fs_read — inherits the
            // telemetry hazard chain.  Documented at fs_quarantine's
            // module header.
            HandlerReply::ok(par)
        })
    }

    /// Fd-based cmode resolution — same shape as fs_seek /
    /// fs_size / fs_read.  Reads the fd's shadow cmode via
    /// `with_mut`.
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

    /// WAL journal only — NO shadow-position advance.
    ///
    /// Unlike fs_read (which advances by bytes returned on all
    /// paths including VerifyDivergence), fs_read_at uses pread
    /// which does NOT move the OS fd position per POSIX.  See
    /// [`verify::FdPositionMutator`](crate::rust::interpreter::io::verify::FdPositionMutator)
    /// — pread is deliberately excluded from the enum.  A
    /// regression that added position advance to this handler
    /// would silently drift shadow position from the actual OS
    /// fd state (which pread did NOT change).
    ///
    /// WAL journaling itself is **stubbed** (Wave 6 scope) — same
    /// deferral as fs_read.  The offset extraction + guard logic
    /// stays live so the Wave 6 slice can land the WAL calls
    /// without touching the structure.  Wave 6 targets:
    ///
    ///   - `journal_read_via_table(handles, fd, bytes, Some(off),
    ///     ack)` on Leader / VerifySuccess / OracularEcho.
    ///   - `journal_read_divergence_via_table(handles, fd,
    ///     Some(off), ack)` on VerifyDivergence.
    fn journal<'a>(
        ctx: SyscallCtx<'a>,
        raw_args: &'a [Par],
        path: JournalPath<'a>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            let [fd_par, off_par, ..] = raw_args else {
                return;
            };
            let (Some(fd_i64), Some(off_i64)) =
                (RhoNumber::unapply(fd_par), RhoNumber::unapply(off_par))
            else {
                return;
            };
            // off < 0 on replay = malformed args.  Journal skips
            // (matches pre-trait: an unparseable / negative off
            // on replay silently no-ops; framework still echoes
            // `previous`).
            if off_i64 < 0 {
                return;
            }
            let _fd_u = fd_i64 as u64;
            let _off_u = off_i64 as u64;
            let _state_source = path.state_source_reply();
            let _bytes_slot = extract_ok_bytes(std::slice::from_ref(_state_source));
            // Wave 6 WAL journaling target (stubbed):
            //
            //   if path.is_divergence() {
            //       let _ = journal_read_divergence_via_table(
            //           ctx.handles, _fd_u, Some(_off_u), ctx.ack
            //       ).await;
            //   } else if let Some(bytes) = &_bytes_slot {
            //       let _ = journal_read_via_table(
            //           ctx.handles, _fd_u, bytes, Some(_off_u), ctx.ack
            //       ).await;
            //   }
            //
            // Dead reference to keep `path.is_divergence` surfaced
            // in the dispatch body — Wave 6 slice drops this when
            // the WAL calls light up above.  Suppress unused
            // bindings via let _ above.
            let _was_divergence = path.is_divergence();
            // Suppress unused ctx — Wave 6 journal calls take
            // ctx.handles + ctx.ack.
            let _ = ctx;
        })
    }
}

/// `#[distributed_slice(FS_HANDLERS)]` registration.
#[distributed_slice(FS_HANDLERS)]
static FS_READ_AT_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsReadAtHandler as FsHandler>::NAME,
    arity: <FsReadAtHandler as FsHandler>::ARITY,
    verifying: <FsReadAtHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsReadAtHandler>(fs, args)),
    urn_suffix: "readAt",
    fixed_channel: FixedChannels::fs_read_at,
    body_ref: BodyRefs::FS_READ_AT,
    family: HandlerFamily::Observation,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::interpreter::io::costs::FS_SYSCALL_CONST;
    use crate::rust::interpreter::rho_type::RhoString;

    /// Trait-level constants wire through.  URN suffix pinned at
    /// registration — camelCase `"readAt"` matches fileio.
    #[test]
    fn consts_wire_through() {
        assert_eq!(FsReadAtHandler::NAME, "fs_read_at");
        assert_eq!(FsReadAtHandler::ARITY, 4);
        assert!(FsReadAtHandler::VERIFYING);
    }

    /// Happy path.
    #[test]
    fn parse_content_accepts_valid_fd_off_n() {
        let args = vec![
            RhoNumber::create_par(42),
            RhoNumber::create_par(1000),
            RhoNumber::create_par(2048),
        ];
        let parsed = FsReadAtHandler::parse_content(&args).ok().expect("parses");
        assert_eq!(parsed.fd, 42u64);
        assert_eq!(parsed.off, 1000u64);
        assert_eq!(parsed.n, 2048u64);
    }

    /// Zero off + zero n accepted (both are legitimate boundary
    /// values — pread at position 0 with 0 bytes is a valid probe).
    #[test]
    fn parse_content_accepts_zero_off_and_n() {
        let args = vec![
            RhoNumber::create_par(42),
            RhoNumber::create_par(0),
            RhoNumber::create_par(0),
        ];
        let parsed = FsReadAtHandler::parse_content(&args).ok().expect("parses");
        assert_eq!(parsed.off, 0u64);
        assert_eq!(parsed.n, 0u64);
    }

    /// LOAD-BEARING: negative `off` rejected at parse.  Pin
    /// against a regression that let `off as u64` wrap to
    /// u64::MAX → libc::pread EINVAL → FSERR_IO (wrong error
    /// class).
    #[test]
    fn parse_content_rejects_negative_off() {
        let args = vec![
            RhoNumber::create_par(42),
            RhoNumber::create_par(-1),
            RhoNumber::create_par(100),
        ];
        assert!(FsReadAtHandler::parse_content(&args).is_err());
    }

    /// LOAD-BEARING: negative `n` rejected at parse (same as
    /// fs_read).
    #[test]
    fn parse_content_rejects_negative_n() {
        let args = vec![
            RhoNumber::create_par(42),
            RhoNumber::create_par(0),
            RhoNumber::create_par(-1),
        ];
        assert!(FsReadAtHandler::parse_content(&args).is_err());
    }

    /// u64 bit-pattern discipline on fd — grep-audit continuation.
    #[test]
    fn parse_content_accepts_negative_fd_as_u64_bitpattern() {
        let args = vec![
            RhoNumber::create_par(-1),
            RhoNumber::create_par(0),
            RhoNumber::create_par(0),
        ];
        let parsed = FsReadAtHandler::parse_content(&args).ok().expect("parses");
        assert_eq!(parsed.fd, u64::MAX);
    }

    #[test]
    fn parse_content_rejects_wrong_arg_count() {
        assert!(FsReadAtHandler::parse_content(&[]).is_err());
        assert!(FsReadAtHandler::parse_content(&[
            RhoNumber::create_par(1),
            RhoNumber::create_par(2)
        ])
        .is_err());
    }

    #[test]
    fn parse_content_rejects_non_int_args() {
        let non_int_off = vec![
            RhoNumber::create_par(0),
            RhoString::create_par("x".to_string()),
            RhoNumber::create_par(0),
        ];
        assert!(FsReadAtHandler::parse_content(&non_int_off).is_err());
    }

    #[test]
    fn pre_charge_cost_returns_base() {
        let via_handler = FsReadAtHandler::pre_charge_cost();
        let via_helper = fs_read_at_cost(0);
        assert_eq!(via_handler.value, via_helper.value);
        assert_eq!(via_handler.operation, via_helper.operation);
    }

    /// LOAD-BEARING slot pin: `n_par` is at `raw_args[2]` for
    /// fs_read_at (fd, off, n), not `[1]` like fs_read.  A
    /// regression that read `[1]` would pre-charge on the offset
    /// value — consensus-observable cost drift.
    #[test]
    fn pre_charge_incremental_reads_n_from_slot_2_not_slot_1() {
        let args = vec![
            RhoNumber::create_par(0),   // fd
            RhoNumber::create_par(999), // off (NOT the byte count)
            RhoNumber::create_par(100), // n (THE byte count)
        ];
        let cost = FsReadAtHandler::pre_charge_incremental(&args).expect("incremental");
        let expected = fs_read_at_cost(100);
        assert_eq!(
            cost.value, expected.value,
            "pre_charge_incremental must read `n` from slot 2 \
             (raw_args[2]), not the offset at slot 1.  A refactor \
             that charged on offset would make large-offset reads \
             overcharge against the deploy budget."
        );
    }

    /// Non-parseable or negative `n_par` → 0 requested bytes +
    /// base `FS_SYSCALL_CONST`.  Same defensive discipline as
    /// fs_read.
    #[test]
    fn pre_charge_incremental_handles_bad_n_as_zero() {
        let bad_n = vec![
            RhoNumber::create_par(0),
            RhoNumber::create_par(0),
            RhoString::create_par("x".to_string()),
        ];
        let cost = FsReadAtHandler::pre_charge_incremental(&bad_n).expect("Some");
        assert_eq!(cost.value, FS_SYSCALL_CONST);

        let neg_n = vec![
            RhoNumber::create_par(0),
            RhoNumber::create_par(0),
            RhoNumber::create_par(-1),
        ];
        let cost = FsReadAtHandler::pre_charge_incremental(&neg_n).expect("Some");
        assert_eq!(cost.value, FS_SYSCALL_CONST);
    }

    /// Short raw_args (< 3 slots) → base cost.  Defense in depth.
    #[test]
    fn pre_charge_incremental_handles_short_raw_args() {
        let short = vec![RhoNumber::create_par(0), RhoNumber::create_par(0)];
        let cost = FsReadAtHandler::pre_charge_incremental(&short).expect("Some");
        assert_eq!(cost.value, FS_SYSCALL_CONST);
    }
}
