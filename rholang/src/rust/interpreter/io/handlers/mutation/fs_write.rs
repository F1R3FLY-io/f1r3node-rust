// fs_write — (fd, bytes) -> [true, nWritten] | [false, FSERR, msg]
//
// Verifying mutation, length-parameterized cost (two-event via
// `pre_charge_incremental` — single cost event, charged after
// `parse_content` succeeds and bytes.len() is known).  Cmode fd-
// based (lookup via `with_mut` on the shadow handle).
//
// # Reserve + finalize (H-6 pattern) with partial-write patch
//
//   - `pre_syscall`: MAX_WRITE_BYTES gate runs FIRST so oversize
//     writes don't consume a WAL slot (M-R3 review round 2).
//     Then reserve a `WalOp::Write` entry via
//     `journal_write_via_table` with `WalOutcome::Success`
//     placeholder, offset = shadow position, length = bytes.len,
//     payload_ref = PayloadRef::hash(bytes).  Fires on BOTH leader
//     and follower (both re-execute the write in Consensus
//     verifying-replay).  Payload persistence to the payload store
//     + `payload_hash -> deploy_sig` recorder runs inside the
//     reserve helper (Phase 7b-2 / DD-7b-2 Option 2) — fail-open.
//   - `dispatch`: `libc::write(raw_fd, bytes.as_ptr(), len)` on the
//     shadow fd under `tokio::task::spawn_blocking`.  Success →
//     `ok_u64(n_actual)`; syscall error → `err(io_err_code,
//     io_msg_scrub)`.  Oversize bail runs symmetrically here so the
//     reply agrees with the pre_syscall-skipped-reserve path.
//   - `journal`: on syscall error or verify-divergence, patch the
//     reserved entry's outcome to `Failure { code }` via
//     `finalize_failure_journal_via_table`.  On a success-path
//     PARTIAL write (`n < requested_bytes.len()`), additionally
//     patch the entry's length + payload_ref via
//     `finalize_write_journal_via_table` so the WAL reflects what
//     actually hit disk.  Shadow position advances by `n` on all 4
//     journal paths (POSIX libc::write moves the OS-fd position by
//     the actually-written count — the shadow must sync so later
//     fs_tell / fs_seek see the same offset the kernel sees).  The
//     advance is gated on `Some(n)` from `extract_ok_u64` — on
//     error replies there's nothing to sync and the kernel didn't
//     move the fd position.
//
// # Length-parameterized cost
//
//   - `pre_charge_cost()` returns `fs_write_cost(0)` as the setup
//     cost (constant base).  The framework charges this at the
//     dispatcher's step-3 pre-charge barrier.
//   - `pre_charge_incremental(raw_args)` reads bytes.len() from
//     `raw_args[1]` and returns `Some(fs_write_cost(bytes))` —
//     SINGLE-EVENT (not base + supplement).  Framework reserves
//     via `metering.reserve_incremental_primitive(cost)`.
//
// # park_external_during deferral
//
// Same deferral as sibling mutation handlers: raw
// `tokio::task::spawn_blocking` without reduction-driver parking.
// Wave 5 scope.
//
// # fd-formatting discipline (consensus-observable)
//
// Unknown-fd FSERR message formats `args.fd` as **u64** (direct)
// via `write_impl_via_table`.  Matches fileio's pre-trait fs_write
// shape.  See `fs_truncate` module header for the cross-handler
// audit discipline.

use std::future::Future;
use std::pin::Pin;

use linkme::distributed_slice;
use models::rhoapi::Par;

use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::io::costs::fs_write_cost;
use crate::rust::interpreter::io::errors::{
    fserr_to_code, FSERR_BAD_ARG, FSERR_CODE_CONSENSUS_DIVERGENCE, FSERR_QUOTA_EXCEEDED,
};
use crate::rust::interpreter::io::handler_trait::{
    dispatch_via_trait_owned, FsHandler, FsHandlerEntry, HandlerFamily, HandlerReply, JournalPath,
    SyscallCtx, FS_HANDLERS,
};
use crate::rust::interpreter::io::handlers::helpers::{
    finalize_failure_journal_via_table, finalize_write_journal_via_table, journal_write_via_table,
    write_impl_via_table,
};
use crate::rust::interpreter::io::response::{extract_err_code, extract_ok_u64};
use crate::rust::interpreter::io::{ConsensusMode, MAX_WRITE_BYTES};
use crate::rust::interpreter::rho_type::{RhoByteArray, RhoNumber};
use crate::rust::interpreter::system_processes::{BodyRefs, FixedChannels};

/// Zero-sized handler type.
pub struct FsWriteHandler;

/// Parsed args.  `fd` is a u64 bit-pattern (sign bit accepted —
/// matches fileio's convention; see fs_truncate for discipline).
/// `bytes` is the payload.
pub struct FsWriteArgs {
    fd: u64,
    bytes: Vec<u8>,
}

impl FsHandler for FsWriteHandler {
    const NAME: &'static str = "fs_write";
    const ARITY: usize = 3; // (fd, bytes, ack)
    const VERIFYING: bool = true;

    type Args = FsWriteArgs;

    fn parse_content(args: &[Par]) -> Result<FsWriteArgs, Box<HandlerReply>> {
        let [fd_par, bytes_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (u64, ByteArray)",
            ));
        };
        match (RhoNumber::unapply(fd_par), RhoByteArray::unapply(bytes_par)) {
            (Some(fd), Some(bytes)) => Ok(FsWriteArgs {
                fd: fd as u64,
                bytes,
            }),
            _ => Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (u64, ByteArray)",
            )),
        }
    }

    fn pre_charge_cost() -> Cost { fs_write_cost(0) }

    fn pre_charge_incremental(raw_args: &[Par]) -> Option<Cost> {
        // Single-event, length-parameterized.  Reads bytes.len()
        // from the raw pre-ack arg at slot 1.  Framework reserves
        // via `metering.reserve_incremental_primitive`.
        let requested_bytes: u64 = raw_args
            .get(1)
            .and_then(RhoByteArray::unapply)
            .map(|b| b.len() as u64)
            .unwrap_or(0);
        Some(fs_write_cost(requested_bytes))
    }

    fn pre_syscall<'a>(
        ctx: SyscallCtx<'a>,
        args: &'a FsWriteArgs,
    ) -> Pin<Box<dyn Future<Output = Result<(), Box<HandlerReply>>> + Send + 'a>> {
        Box::pin(async move {
            // MAX_WRITE_BYTES gate first — oversize writes must NOT
            // consume a WAL slot (M-R3 review round 2).
            if args.bytes.len() as u64 > MAX_WRITE_BYTES {
                return Err(HandlerReply::boxed_err(
                    FSERR_QUOTA_EXCEEDED,
                    format!("write {} exceeds MAX_WRITE_BYTES", args.bytes.len()),
                ));
            }
            // Pre-append WAL entry (fd-based cmode; self-guards for
            // Oracular).  Err → WAL cap → early exit with
            // FSERR_QUOTA_EXCEEDED.
            if journal_write_via_table(ctx.handles, args.fd, &args.bytes, None, ctx.ack)
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
        args: FsWriteArgs,
    ) -> Pin<Box<dyn Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            let par = write_impl_via_table(ctx.handles, args.fd, args.bytes, None).await;
            HandlerReply::Ok(par)
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
        raw_args: &'a [Par],
        path: JournalPath<'a>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            let [fd_par, bytes_par, ..] = raw_args else {
                return;
            };
            let Some(fd_i64) = RhoNumber::unapply(fd_par) else {
                return;
            };
            let fd_u = fd_i64 as u64;
            let bytes_opt = RhoByteArray::unapply(bytes_par);
            let state_source = path.state_source_reply();
            // Gated `n_opt` — matches pre-refactor's `if let Some(n)
            // = extract_ok_u64(...)` gate.  On error reply,
            // extract_ok_u64 returns None → NO finalize_write_journal
            // (would incorrectly patch WAL length to 0) and NO shadow
            // advance (nothing to sync — the syscall didn't move OS
            // fd position).
            let n_opt = extract_ok_u64(std::slice::from_ref(state_source));
            if path.is_divergence() {
                finalize_failure_journal_via_table(
                    ctx.handles,
                    FSERR_CODE_CONSENSUS_DIVERGENCE,
                    ctx.ack,
                );
            } else {
                if let Some(n) = n_opt {
                    if let Some(bytes) = bytes_opt.as_ref() {
                        if n < bytes.len() as u64 {
                            finalize_write_journal_via_table(ctx.handles, bytes, n, ctx.ack);
                        }
                    }
                }
                let reply = path.produce_reply();
                if let Some(code_str) = extract_err_code(std::slice::from_ref(reply)) {
                    finalize_failure_journal_via_table(
                        ctx.handles,
                        fserr_to_code(&code_str),
                        ctx.ack,
                    );
                }
            }
            if let Some(n) = n_opt {
                if n > 0 {
                    let _ = ctx
                        .handles
                        .with_mut(fd_u, |h| h.position = h.position.saturating_add(n))
                        .await;
                }
            }
        })
    }
}

/// `#[distributed_slice(FS_HANDLERS)]` registration.
#[distributed_slice(FS_HANDLERS)]
static FS_WRITE_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsWriteHandler as FsHandler>::NAME,
    arity: <FsWriteHandler as FsHandler>::ARITY,
    verifying: <FsWriteHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsWriteHandler>(fs, args)),
    urn_suffix: "write",
    fixed_channel: FixedChannels::fs_write,
    body_ref: BodyRefs::FS_WRITE,
    family: HandlerFamily::Mutation,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::interpreter::rho_type::RhoString;

    /// Trait-level constants wire through.  ARITY = 3 (fd + bytes +
    /// ack); verifying.
    #[test]
    fn consts_wire_through() {
        assert_eq!(FsWriteHandler::NAME, "fs_write");
        assert_eq!(FsWriteHandler::ARITY, 3);
        assert!(FsWriteHandler::VERIFYING);
    }

    /// `parse_content` success path returns `(fd, bytes)`.
    #[test]
    fn parse_content_accepts_fd_and_bytes() {
        let args = vec![
            RhoNumber::create_par(7),
            RhoByteArray::create_par(vec![1, 2, 3]),
        ];
        let parsed = FsWriteHandler::parse_content(&args)
            .ok()
            .expect("fd + ByteArray parses");
        assert_eq!(parsed.fd, 7u64);
        assert_eq!(parsed.bytes, vec![1, 2, 3]);
    }

    /// LOAD-BEARING: fd parses as u64 bit-pattern even when the
    /// signed GInt has the sign bit set.  Same discipline as
    /// `fs_close` / `fs_seek` / `fs_truncate`.
    #[test]
    fn parse_content_accepts_negative_fd_as_u64_bitpattern() {
        let args = vec![RhoNumber::create_par(-1), RhoByteArray::create_par(vec![])];
        let parsed = FsWriteHandler::parse_content(&args)
            .ok()
            .expect("negative fd parses as u64");
        assert_eq!(parsed.fd, u64::MAX);
        assert!(parsed.bytes.is_empty());
    }

    /// `parse_content` accepts a zero-length byte array (write of 0
    /// bytes is a no-op syscall; the kernel returns 0, which the
    /// WAL records as a 0-length Write entry).
    #[test]
    fn parse_content_accepts_empty_bytes() {
        let args = vec![RhoNumber::create_par(0), RhoByteArray::create_par(vec![])];
        assert!(FsWriteHandler::parse_content(&args).is_ok());
    }

    /// `parse_content` rejects wrong arg count (empty, 1, 3+).
    #[test]
    fn parse_content_rejects_wrong_arg_count() {
        let empty: Vec<Par> = vec![];
        let one = vec![RhoNumber::create_par(1)];
        let three = vec![
            RhoNumber::create_par(1),
            RhoByteArray::create_par(vec![1]),
            RhoNumber::create_par(2),
        ];
        assert!(FsWriteHandler::parse_content(&empty).is_err());
        assert!(FsWriteHandler::parse_content(&one).is_err());
        assert!(FsWriteHandler::parse_content(&three).is_err());
    }

    /// `parse_content` rejects a non-GInt fd or non-ByteArray
    /// payload.
    #[test]
    fn parse_content_rejects_wrong_arg_types() {
        let wrong_fd = vec![
            RhoString::create_par("not a fd".to_string()),
            RhoByteArray::create_par(vec![1]),
        ];
        let wrong_bytes = vec![
            RhoNumber::create_par(0),
            RhoString::create_par("not bytes".to_string()),
        ];
        assert!(FsWriteHandler::parse_content(&wrong_fd).is_err());
        assert!(FsWriteHandler::parse_content(&wrong_bytes).is_err());
    }

    /// `pre_charge_cost` returns the setup cost `fs_write_cost(0)`.
    /// Pins the two-step cost pattern (setup + incremental).
    #[test]
    fn pre_charge_cost_is_setup_only() {
        let via_handler = FsWriteHandler::pre_charge_cost();
        let via_helper = fs_write_cost(0);
        assert_eq!(via_handler.value, via_helper.value);
        assert_eq!(via_handler.operation, via_helper.operation);
    }

    /// `pre_charge_incremental` reads bytes.len() from raw_args[1]
    /// and returns the length-parameterized cost.
    #[test]
    fn pre_charge_incremental_reads_payload_length() {
        let raw = vec![
            RhoNumber::create_par(1),
            RhoByteArray::create_par(vec![0u8; 1000]),
        ];
        let cost = FsWriteHandler::pre_charge_incremental(&raw)
            .expect("incremental returns Some for length-parameterized handler");
        let expected = fs_write_cost(1000);
        assert_eq!(cost.value, expected.value);
    }

    /// `pre_charge_incremental` falls through to 0 bytes when the
    /// raw args don't have a ByteArray at slot 1 (defensive —
    /// `parse_content` would already reject this shape, but the
    /// incremental hook runs before parse_content on some paths).
    #[test]
    fn pre_charge_incremental_defaults_to_zero_bytes_on_missing_payload() {
        let raw: Vec<Par> = vec![RhoNumber::create_par(1)];
        let cost =
            FsWriteHandler::pre_charge_incremental(&raw).expect("still Some on missing bytes slot");
        let expected = fs_write_cost(0);
        assert_eq!(cost.value, expected.value);
    }

    /// MAX_WRITE_BYTES is reachable from this module's import path.
    #[test]
    fn max_write_bytes_is_positive() {
        let _: u64 = MAX_WRITE_BYTES;
        assert!(MAX_WRITE_BYTES > 0);
    }
}
