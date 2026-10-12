// fs_truncate — (fd, n) -> [true] | [false, FSERR, msg]
//
// First mutation-family handler on dev.  Verifying, constant cost.
// Cmode fd-based (lookup via `with_mut` on the shadow handle).
//
// # Reserve + finalize (H-6 pattern)
//
//   - `pre_syscall`: reserve a `WalOp::Truncate` entry with
//     `WalOutcome::Success` placeholder.  Fires on BOTH leader and
//     follower (the follower is replaying the same deploy, so its
//     WAL grows in lock-step with the leader's).  Oversize-truncate
//     bail runs BEFORE the reserve so FSERR_QUOTA_EXCEEDED calls
//     don't consume a WAL slot.
//   - `dispatch`: `libc::ftruncate(raw_fd, n as i64)` on the shadow
//     fd under `tokio::task::spawn_blocking`.  Success → `ok_bare()`;
//     syscall error → `err(io_err_code, io_msg_scrub)`.
//   - `journal`: on error / verify-divergence, patch the reserved
//     entry's outcome via `finalize_failure_journal_via_table`.
//     Success leaves the placeholder as `Success`.
//
// # park_external_during deferral
//
// Same deferral as the observation-family siblings: raw
// `tokio::task::spawn_blocking` without reduction-driver parking.
// Wave 5 scope.
//
// # fd-formatting discipline (consensus-observable)
//
// Unknown-fd FSERR message formats `args.fd` as **u64** (direct)
// — matches pre-trait fs_truncate.  See `fs_seek`'s module header
// for the cross-handler audit table.  A future harmonize-all-to-u64
// refactor would be byte-identical for this handler but would
// drift fs_seek's unknown-fd message (which uses `as i64`).
//
// # MAX_TRUNCATE_BYTES gate
//
// Oversize-truncate (`n > MAX_TRUNCATE_BYTES`) returns
// `FSERR_QUOTA_EXCEEDED` BEFORE the WAL reserve so cap-exhausting
// loops of oversized truncates don't consume WAL slots.  The gate
// runs symmetrically in both `pre_syscall` (early return, no
// reserve) and `dispatch` (the reply) — both leader and follower
// agree on the "no reserve + oversize reply" path.

use std::future::Future;
use std::os::fd::AsRawFd;
use std::pin::Pin;

use linkme::distributed_slice;
use models::rhoapi::Par;
use tokio::task::spawn_blocking;

use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::io::costs::fs_truncate_cost;
use crate::rust::interpreter::io::errors::{
    fserr_to_code, io_err_code, join_err_abort, FSERR_BAD_ARG, FSERR_CLOSED,
    FSERR_CODE_CONSENSUS_DIVERGENCE, FSERR_QUOTA_EXCEEDED,
};
use crate::rust::interpreter::io::handler_trait::{
    dispatch_via_trait_owned, FsHandler, FsHandlerEntry, HandlerFamily, HandlerReply, JournalPath,
    SyscallCtx, FS_HANDLERS,
};
use crate::rust::interpreter::io::handlers::helpers::{
    finalize_failure_journal_via_table, journal_truncate_via_table,
};
use crate::rust::interpreter::io::path::io_msg_scrub;
use crate::rust::interpreter::io::response::{extract_err_code, ok_bare};
use crate::rust::interpreter::io::{ConsensusMode, MAX_TRUNCATE_BYTES};
use crate::rust::interpreter::rho_type::RhoNumber;
use crate::rust::interpreter::system_processes::{BodyRefs, FixedChannels};

/// Zero-sized handler type.
pub struct FsTruncateHandler;

/// Parsed args for [`FsTruncateHandler`].  Both fd and n are u64
/// — fd is a hash-derived bit-pattern (see `fs_close` on the sign-
/// bit discipline); n is a byte count (`n >= 0` enforced at parse
/// time so negative truncate lengths surface as `FSERR_BAD_ARG`).
pub struct FsTruncateArgs {
    fd: u64,
    n: u64,
}

impl FsHandler for FsTruncateHandler {
    const NAME: &'static str = "fs_truncate";
    const ARITY: usize = 3; // (fd, n, ack)
    const VERIFYING: bool = true;

    type Args = FsTruncateArgs;

    fn parse_content(args: &[Par]) -> Result<FsTruncateArgs, Box<HandlerReply>> {
        let [fd_par, n_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (u64, u64)",
            ));
        };
        match (RhoNumber::unapply(fd_par), RhoNumber::unapply(n_par)) {
            (Some(fd), Some(n)) if n >= 0 => Ok(FsTruncateArgs {
                fd: fd as u64,
                n: n as u64,
            }),
            _ => Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (u64, u64)",
            )),
        }
    }

    fn pre_charge_cost() -> Cost { fs_truncate_cost() }

    fn pre_syscall<'a>(
        ctx: SyscallCtx<'a>,
        args: &'a FsTruncateArgs,
    ) -> Pin<Box<dyn Future<Output = Result<(), Box<HandlerReply>>> + Send + 'a>> {
        // C-29-F1: pre-append WAL entry on BOTH leader and follower.
        // MAX_TRUNCATE_BYTES gate first so oversize truncates don't
        // consume a WAL slot for calls that will error out.
        Box::pin(async move {
            if args.n > MAX_TRUNCATE_BYTES {
                return Ok(());
            }
            if journal_truncate_via_table(ctx.handles, args.fd, args.n, ctx.ack)
                .await
                .is_err()
            {
                return Err(HandlerReply::boxed_err(
                    FSERR_QUOTA_EXCEEDED,
                    "WAL cap exceeded",
                ));
            }
            Ok(())
        })
    }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsTruncateArgs,
    ) -> Pin<Box<dyn Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            if args.n > MAX_TRUNCATE_BYTES {
                return HandlerReply::err(
                    FSERR_QUOTA_EXCEEDED,
                    format!("truncate {} exceeds MAX_TRUNCATE_BYTES", args.n),
                );
            }
            let file_arc = match ctx.handles.raw_fd(args.fd).await {
                Some(f) => f,
                // Pre-trait fs_truncate formats `fd` as **u64**
                // (direct).  See module header for the cross-handler
                // fd-formatting audit discipline.
                None => {
                    return HandlerReply::err(FSERR_CLOSED, format!("unknown fd {}", args.fd));
                }
            };
            let n = args.n;
            let r = spawn_blocking(move || {
                let raw_fd = file_arc.as_raw_fd();
                // SAFETY: `raw_fd` is derived from `file_arc:
                // Arc<File>` whose lifetime spans the `spawn_blocking`
                // closure; the fd remains open for the duration of
                // the syscall.  `ftruncate` takes an open fd and a
                // signed length; negative return means errno is set.
                unsafe {
                    if libc::ftruncate(raw_fd, n as i64) < 0 {
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

    fn journal<'a>(
        ctx: SyscallCtx<'a>,
        _raw_args: &'a [Par],
        path: JournalPath<'a>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        // H-6 finalize: patch the pre-appended Success placeholder
        // to Failure(code) on syscall error or verify-divergence.
        // Success path (no err payload) leaves the placeholder as
        // Success.
        Box::pin(async move {
            if path.is_divergence() {
                finalize_failure_journal_via_table(
                    ctx.handles,
                    FSERR_CODE_CONSENSUS_DIVERGENCE,
                    ctx.ack,
                );
            } else {
                let reply = path.produce_reply();
                if let Some(code_str) = extract_err_code(std::slice::from_ref(reply)) {
                    finalize_failure_journal_via_table(
                        ctx.handles,
                        fserr_to_code(&code_str),
                        ctx.ack,
                    );
                }
            }
        })
    }
}

/// `#[distributed_slice(FS_HANDLERS)]` registration.
#[distributed_slice(FS_HANDLERS)]
static FS_TRUNCATE_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsTruncateHandler as FsHandler>::NAME,
    arity: <FsTruncateHandler as FsHandler>::ARITY,
    verifying: <FsTruncateHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsTruncateHandler>(fs, args)),
    urn_suffix: "truncate",
    fixed_channel: FixedChannels::fs_truncate,
    body_ref: BodyRefs::FS_TRUNCATE,
    family: HandlerFamily::Mutation,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::interpreter::rho_type::RhoString;

    /// Trait-level constants wire through.  First Mutation-family
    /// handler, so VERIFYING = true (unlike the Lifecycle / Stream
    /// handlers landed so far).
    #[test]
    fn consts_wire_through() {
        assert_eq!(FsTruncateHandler::NAME, "fs_truncate");
        assert_eq!(FsTruncateHandler::ARITY, 3);
        assert!(FsTruncateHandler::VERIFYING);
    }

    /// `parse_content` success path returns `(fd, n)` as u64.
    #[test]
    fn parse_content_accepts_two_non_negative_gints() {
        let args = vec![RhoNumber::create_par(7), RhoNumber::create_par(1024)];
        let parsed = FsTruncateHandler::parse_content(&args)
            .ok()
            .expect("two u64 parse");
        assert_eq!(parsed.fd, 7u64);
        assert_eq!(parsed.n, 1024u64);
    }

    /// LOAD-BEARING: fd parses as u64 bit-pattern even when the
    /// signed GInt has the sign bit set.  Same discipline as
    /// `fs_close` / `fs_seek` / etc.  A future tightening to
    /// `fd >= 0` would silently reject half the fd space.
    #[test]
    fn parse_content_accepts_negative_fd_as_u64_bitpattern() {
        let args = vec![RhoNumber::create_par(-1), RhoNumber::create_par(0)];
        let parsed = FsTruncateHandler::parse_content(&args)
            .ok()
            .expect("negative fd parses as u64");
        assert_eq!(parsed.fd, u64::MAX);
        assert_eq!(parsed.n, 0u64);
    }

    /// LOAD-BEARING: a negative `n` is rejected at parse time.
    /// `libc::ftruncate` with a negative length is nominally
    /// EINVAL, but surfacing that as FSERR_BAD_ARG at the Rholang
    /// boundary is consensus-observable.  Pin against a future
    /// refactor that treated `n as u64` as the parse path (which
    /// would accept the sign-bit set as a huge positive).
    #[test]
    fn parse_content_rejects_negative_n() {
        let args = vec![RhoNumber::create_par(0), RhoNumber::create_par(-1)];
        assert!(FsTruncateHandler::parse_content(&args).is_err());
    }

    /// `parse_content` rejects wrong arg count (empty, 1, 3+).
    #[test]
    fn parse_content_rejects_wrong_arg_count() {
        let empty: Vec<Par> = vec![];
        let one = vec![RhoNumber::create_par(1)];
        let three = vec![
            RhoNumber::create_par(1),
            RhoNumber::create_par(2),
            RhoNumber::create_par(3),
        ];
        assert!(FsTruncateHandler::parse_content(&empty).is_err());
        assert!(FsTruncateHandler::parse_content(&one).is_err());
        assert!(FsTruncateHandler::parse_content(&three).is_err());
    }

    /// `parse_content` rejects a non-GInt arg (e.g., a String in
    /// either slot).
    #[test]
    fn parse_content_rejects_non_int_args() {
        let non_int_fd = vec![
            RhoString::create_par("not a fd".to_string()),
            RhoNumber::create_par(0),
        ];
        let non_int_n = vec![
            RhoNumber::create_par(0),
            RhoString::create_par("not a length".to_string()),
        ];
        assert!(FsTruncateHandler::parse_content(&non_int_fd).is_err());
        assert!(FsTruncateHandler::parse_content(&non_int_n).is_err());
    }

    /// `pre_charge_cost` delegates to `costs::fs_truncate_cost`.
    /// Golden value pinned at the costs module.
    #[test]
    fn pre_charge_cost_delegates_to_costs_helper() {
        let via_handler = FsTruncateHandler::pre_charge_cost();
        let via_helper = fs_truncate_cost();
        assert_eq!(via_handler.value, via_helper.value);
        assert_eq!(via_handler.operation, via_helper.operation);
    }

    /// MAX_TRUNCATE_BYTES is reachable from this module's import
    /// path (crate::rust::interpreter::io::MAX_TRUNCATE_BYTES).
    /// Pin that the constant is a plain u64 > 0 — a future
    /// consensus-coordinated bump moves this number; a regression
    /// that typo'd a lower-case `max_truncate_bytes` into local
    /// scope would trip here before Wave 6.
    #[test]
    fn max_truncate_bytes_is_positive() {
        let _: u64 = MAX_TRUNCATE_BYTES;
        assert!(MAX_TRUNCATE_BYTES > 0);
    }
}
