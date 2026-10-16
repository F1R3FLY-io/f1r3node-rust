// fs_read — (fd, n) -> [true, bytes] | [false, FSERR_*, msg]
//
// FIRST LENGTH-PARAMETERIZED verifying handler.  Sequential read
// via `libc::read(fd, buf, n)`; advances OS-fd position by bytes
// returned.  Fifth verifying handler, third fd-based.
//
// # First-length-parameterized matters
//
// fs_read overrides `pre_charge_incremental` (the length-based
// cost method on the FsHandler trait, slice 4.6) to charge
// `fs_read_cost(n)` based on the REQUESTED byte count.  The
// framework calls `reserve_incremental_primitive(cost)` instead
// of `reserve_primitive(pre_charge_cost())` for this handler —
// first time that path fires in a migrated handler.
//
// Requested-count semantics (NOT returned-count): the pre-charge
// uses the caller-supplied `n`, charging the full requested
// quantity even if `libc::read` returns fewer bytes (EOF).  The
// `fs_read_cost` module docstring (slice 4.3) explains this
// closes a mispredicted-read amplification vector where a caller
// pre-seeks past EOF to request megabytes at zero cost.
//
// # Shadow position advance (LIVE at Wave 4)
//
// Unlike fs_size / fs_stat / fs_exists (WAL-journal-only),
// fs_read's `journal` method also advances the shadow position
// — a wire-visible-on-replay surface that is NOT deferred.
// Must land live under Wave 4 so follower + leader shadow
// positions stay converged.
//
// Position advance happens on all paths including VerifyDivergence
// (POSIX `libc::read` advanced OS fd position by bytes returned
// regardless of consensus outcome; shadow must sync).  The
// `path.is_divergence()` branch only affects WAL journal shape —
// position still advances.
//
// # Dependencies — read_impl_via_table + journal stubs
//
// Dispatch delegates to [`read_impl_via_table`] (slice 4.20
// companion in `io/handlers/helpers/read_impl.rs`) for the libc
// read + reply construction.  The journal method's WAL calls
// (`journal_read_via_table`, `journal_read_divergence_via_table`)
// are stubbed with Wave 6 target comments — same deferral as
// fs_size / fs_stat / fs_exists.  Position advance IS wired.

use std::future::Future;
use std::pin::Pin;

use linkme::distributed_slice;
use models::rhoapi::Par;

use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::io::costs::fs_read_cost;
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
pub struct FsReadHandler;

/// Parsed args for [`FsReadHandler`].  `n` is parsed as `i64`
/// (RhoNumber returns i64) and validated non-negative at parse
/// time — the handler then stores it as `u64` for the read call.
pub struct FsReadArgs {
    fd: u64,
    n: u64,
}

impl FsHandler for FsReadHandler {
    const NAME: &'static str = "fs_read";
    const ARITY: usize = 3; // (fd, n, ack)
    const VERIFYING: bool = true;

    type Args = FsReadArgs;

    fn parse_content(args: &[Par]) -> Result<FsReadArgs, Box<HandlerReply>> {
        let [fd_par, n_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (fd:GInt, n:GInt>=0)",
            ));
        };
        match (RhoNumber::unapply(fd_par), RhoNumber::unapply(n_par)) {
            (Some(fd), Some(n)) if n >= 0 => Ok(FsReadArgs {
                fd: fd as u64,
                n: n as u64,
            }),
            _ => Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (fd:GInt, n:GInt>=0)",
            )),
        }
    }

    /// Only called when `pre_charge_incremental` returns `None`;
    /// fs_read always overrides.  Provided to satisfy the trait
    /// — the framework uses the incremental path.
    fn pre_charge_cost() -> Cost { fs_read_cost(0) }

    /// Charge via the length-parameterized helper.  Caller-
    /// controlled `n` on `raw_args[1]`.
    ///
    /// Byte count is REQUESTED (`n`), not returned — matches the
    /// `fs_read_cost` module docstring (slice 4.3): closes the
    /// mispredicted-read amplification vector where a caller
    /// pre-seeks past EOF to request megabytes at zero cost.
    ///
    /// Non-parseable or negative `n_par` yields `0` requested
    /// bytes, still burning `FS_SYSCALL_CONST` for the dispatch
    /// (via `fs_read_cost`'s base).  Deterministic across leader
    /// + replay — both see the same `raw_args` bytes.
    fn pre_charge_incremental(raw_args: &[Par]) -> Option<Cost> {
        let requested_bytes: u64 = raw_args
            .get(1)
            .and_then(RhoNumber::unapply)
            .and_then(|n| u64::try_from(n).ok())
            .unwrap_or(0);
        Some(fs_read_cost(requested_bytes))
    }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsReadArgs,
    ) -> Pin<Box<dyn Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            // Full dispatch body delegates to the shared helper —
            // libc::read + bounds check + reply construction.
            let par = read_impl_via_table(ctx.handles, args.fd, args.n, None).await;
            // Same HandlerReply::ok wrapping as fs_quarantine /
            // fs_exists / fs_stat — inherits the telemetry hazard
            // (future is_ok() consumer would misreport err Pars).
            // Documented at fs_quarantine's module header; the
            // Wave 6 follow-up threads HandlerReply through
            // spawn_blocking_par's cousin (`read_impl_via_table`
            // would gain an `-> HandlerReply` return type).
            HandlerReply::ok(par)
        })
    }

    /// Verifying-handler cmode resolution — fd-based (same shape
    /// as fs_seek / fs_size).  Reads the fd's shadow cmode.
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

    /// Dual-purpose hook: shadow-position advance (LIVE at Wave 4)
    /// + WAL journaling (STUB — Wave 6).
    ///
    /// # Shadow position advance
    ///
    /// POSIX `libc::read` advances the OS-fd position by the
    /// bytes actually returned — on all paths including
    /// VerifyDivergence (the fresh syscall ran on the follower's
    /// own fd regardless of consensus outcome).  The handler
    /// MUST advance its shadow position to match or shadow +
    /// OS state drift silently across subsequent reads.
    ///
    /// `path.state_source_reply()` provides the fresh syscall
    /// reply on Leader / VerifySuccess / VerifyDivergence and the
    /// `previous` cached reply on OracularEcho — both carry the
    /// `ok_bytes(bytes)` payload, so `extract_ok_bytes` + length
    /// gives the correct advance count on every path.
    ///
    /// # WAL journaling (Wave 6)
    ///
    /// Fileio additionally calls:
    ///
    ///   - `journal_read_divergence_via_table(ctx.handles, fd,
    ///     offset=None, ctx.ack)` on `is_divergence()` paths.
    ///   - `journal_read_via_table(ctx.handles, fd, bytes,
    ///     offset=None, ctx.ack)` on success / oracular-echo
    ///     paths.
    ///
    /// Both helpers live in `handlers_helpers.rs` on fileio and
    /// have NOT been ported to dev.  Under Wave 4 + 5 these are
    /// no-ops; verifying path still works via
    /// `verify_reply_hash_matches_cached` (dispatcher step 7).
    /// Wave 6 lands the helpers.
    fn journal<'a>(
        ctx: SyscallCtx<'a>,
        raw_args: &'a [Par],
        path: JournalPath<'a>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            let [fd_par, ..] = raw_args else {
                return;
            };
            let Some(fd_i64) = RhoNumber::unapply(fd_par) else {
                return;
            };
            let fd_u = fd_i64 as u64;

            // Extract bytes from the state-source reply.  Order
            // matters relative to the WAL journal (Wave 6): the
            // journal reads the PRE-advance shadow position from
            // FileHandle, so WAL append happens BEFORE the
            // advance below.  Currently a no-op; the ordering is
            // preserved so the Wave 6 slice can land its journal
            // calls between the extract and the advance without
            // touching this ordering structure.
            let bytes_slot = extract_ok_bytes(std::slice::from_ref(path.state_source_reply()));
            let advance_n = bytes_slot.as_ref().map(|b| b.len() as u64).unwrap_or(0);

            // Wave 6 WAL journaling target (stubbed):
            //
            //   if path.is_divergence() {
            //       let _ = journal_read_divergence_via_table(
            //           ctx.handles, fd_u, None, ctx.ack
            //       ).await;
            //   } else if let Some(bytes) = &bytes_slot {
            //       let _ = journal_read_via_table(
            //           ctx.handles, fd_u, bytes, None, ctx.ack
            //       ).await;
            //   }
            //
            // Dead reference to keep `path.is_divergence` surfaced
            // in the dispatch body — Wave 6 slice drops this when
            // the WAL calls light up above.
            let _was_divergence = path.is_divergence();

            // Shadow position advance (LIVE at Wave 4).  Matches
            // pre-trait fs_read: `h.position = h.position +
            // bytes.len()` with saturating semantics against u64
            // overflow on a pathological multi-exbibyte read.
            if advance_n > 0 {
                let _ = ctx
                    .handles
                    .with_mut(fd_u, |h| {
                        h.position = h.position.saturating_add(advance_n);
                    })
                    .await;
            }
        })
    }
}

/// `#[distributed_slice(FS_HANDLERS)]` registration.
#[distributed_slice(FS_HANDLERS)]
static FS_READ_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsReadHandler as FsHandler>::NAME,
    arity: <FsReadHandler as FsHandler>::ARITY,
    verifying: <FsReadHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsReadHandler>(fs, args)),
    urn_suffix: "read",
    fixed_channel: FixedChannels::fs_read,
    body_ref: BodyRefs::FS_READ,
    family: HandlerFamily::Observation,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::interpreter::io::costs::FS_SYSCALL_CONST;
    use crate::rust::interpreter::rho_type::RhoString;

    /// Trait-level constants wire through.
    #[test]
    fn consts_wire_through() {
        assert_eq!(FsReadHandler::NAME, "fs_read");
        assert_eq!(FsReadHandler::ARITY, 3);
        assert!(FsReadHandler::VERIFYING);
    }

    /// Happy path: positive fd + positive n parse.
    #[test]
    fn parse_content_accepts_positive_fd_and_n() {
        let args = vec![RhoNumber::create_par(42), RhoNumber::create_par(1024)];
        let parsed = FsReadHandler::parse_content(&args).ok().expect("parses");
        assert_eq!(parsed.fd, 42u64);
        assert_eq!(parsed.n, 1024u64);
    }

    /// n == 0 is accepted (zero-byte read is legitimate — EOF
    /// probe).
    #[test]
    fn parse_content_accepts_zero_n() {
        let args = vec![RhoNumber::create_par(42), RhoNumber::create_par(0)];
        let parsed = FsReadHandler::parse_content(&args).ok().expect("parses");
        assert_eq!(parsed.n, 0u64);
    }

    /// LOAD-BEARING: negative `n` rejected at parse.  Pin against
    /// a future refactor that silently accepted negative via
    /// `n as u64` bit-pattern (which would wrap to a huge value
    /// and immediately trip the quota check — wrong error class).
    #[test]
    fn parse_content_rejects_negative_n() {
        let args = vec![RhoNumber::create_par(42), RhoNumber::create_par(-1)];
        assert!(FsReadHandler::parse_content(&args).is_err());
    }

    /// u64 bit-pattern discipline on `fd` — same as fs_close /
    /// fs_flush / fs_tell / fs_size.  Grep-audit continuation.
    #[test]
    fn parse_content_accepts_negative_fd_as_u64_bitpattern() {
        let args = vec![RhoNumber::create_par(-1), RhoNumber::create_par(0)];
        let parsed = FsReadHandler::parse_content(&args).ok().expect("parses");
        assert_eq!(parsed.fd, u64::MAX);
    }

    #[test]
    fn parse_content_rejects_wrong_arg_count() {
        assert!(FsReadHandler::parse_content(&[]).is_err());
        assert!(FsReadHandler::parse_content(&[RhoNumber::create_par(1)]).is_err());
    }

    #[test]
    fn parse_content_rejects_non_int_args() {
        let non_int_fd = vec![
            RhoString::create_par("not a fd".to_string()),
            RhoNumber::create_par(0),
        ];
        let non_int_n = vec![
            RhoNumber::create_par(0),
            RhoString::create_par("not a count".to_string()),
        ];
        assert!(FsReadHandler::parse_content(&non_int_fd).is_err());
        assert!(FsReadHandler::parse_content(&non_int_n).is_err());
    }

    /// `pre_charge_cost` returns `fs_read_cost(0)` — the base.
    #[test]
    fn pre_charge_cost_returns_base() {
        let via_handler = FsReadHandler::pre_charge_cost();
        let via_helper = fs_read_cost(0);
        assert_eq!(via_handler.value, via_helper.value);
        assert_eq!(via_handler.operation, via_helper.operation);
    }

    /// `pre_charge_incremental` reads `n` from `raw_args[1]` and
    /// charges `fs_read_cost(n)`.
    #[test]
    fn pre_charge_incremental_reads_n_from_raw_args() {
        let args = vec![RhoNumber::create_par(0), RhoNumber::create_par(100)];
        let cost = FsReadHandler::pre_charge_incremental(&args).expect("incremental");
        let expected = fs_read_cost(100);
        assert_eq!(cost.value, expected.value);
        assert_eq!(cost.operation, expected.operation);
    }

    /// LOAD-BEARING: non-parseable or negative `n_par` yields
    /// `fs_read_cost(0)` — still charges the base
    /// (`FS_SYSCALL_CONST`) but doesn't fail the pre-charge.
    /// Deterministic across leader + replay even when raw_args
    /// are malformed.
    #[test]
    fn pre_charge_incremental_handles_bad_n_as_zero() {
        // Non-parseable (String in n slot).
        let bad_n = vec![
            RhoNumber::create_par(0),
            RhoString::create_par("x".to_string()),
        ];
        let cost = FsReadHandler::pre_charge_incremental(&bad_n).expect("Some");
        assert_eq!(cost.value, FS_SYSCALL_CONST);

        // Negative n — u64::try_from fails + unwrap_or(0).
        let neg_n = vec![RhoNumber::create_par(0), RhoNumber::create_par(-1)];
        let cost = FsReadHandler::pre_charge_incremental(&neg_n).expect("Some");
        assert_eq!(cost.value, FS_SYSCALL_CONST);
    }

    /// Short raw_args (missing n) → `fs_read_cost(0)`.
    /// Framework arity check (dispatcher step 2) catches this
    /// before dispatch; the pre-charge call happens AFTER arity
    /// so short raw_args shouldn't surface here.  Pin as
    /// defense-in-depth.
    #[test]
    fn pre_charge_incremental_handles_short_raw_args() {
        let short = vec![RhoNumber::create_par(0)];
        let cost = FsReadHandler::pre_charge_incremental(&short).expect("Some");
        assert_eq!(cost.value, FS_SYSCALL_CONST);
    }
}
