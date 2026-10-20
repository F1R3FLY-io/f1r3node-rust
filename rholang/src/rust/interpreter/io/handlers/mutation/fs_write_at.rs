// fs_write_at — (fd, off, bytes) -> [true, nWritten] | [false, FSERR, msg]
//
// Positional sibling of fs_write.  Verifying, length-parameterized
// cost (single-event via `pre_charge_incremental`).  Cmode fd-based
// (shadow handle lookup).  Three differences from fs_write:
//
//   1. ARITY = 4 (fd, off, bytes, ack) — one extra slot for the
//      offset.  `off` must be `>= 0` at parse (negative offset is
//      FSERR_BAD_ARG; `pwrite` with a negative off is EINVAL at the
//      syscall layer, but surfacing it at the Rholang boundary is
//      consensus-observable).
//   2. `write_impl_via_table` called with `offset = Some(off)` →
//      libc::pwrite instead of libc::write.  WAL op is `WriteAt`
//      (set inside `journal_write_via_table` on the `Some(offset)`
//      branch).
//   3. **No shadow position advance.**  POSIX pwrite doesn't move
//      the OS-fd position, so the shadow must not either —
//      otherwise fs_tell / fs_seek on the same fd would see a
//      position the kernel disagrees with.  The `journal` hook
//      otherwise mirrors fs_write: partial-write finalize +
//      failure finalize, gated on `Some(n)` from `extract_ok_u64`.
//
// Reuses `MAX_WRITE_BYTES` (same cap applies — the write op is
// semantically equivalent at the byte-volume level).  Reuses
// `journal_write_via_table` + `finalize_write_journal_via_table` +
// `write_impl_via_table` — all the fs_write helpers are generic
// over the offset shape.

use std::future::Future;
use std::pin::Pin;

use linkme::distributed_slice;
use models::rhoapi::Par;

use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::io::costs::fs_write_at_cost;
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
pub struct FsWriteAtHandler;

/// Parsed args.  `fd` is a u64 bit-pattern (sign bit accepted).
/// `off` is a non-negative u64 (negative GInt rejected at parse —
/// pwrite with negative off is EINVAL at the syscall but surfacing
/// at the Rholang boundary is consensus-observable).
pub struct FsWriteAtArgs {
    fd: u64,
    off: u64,
    bytes: Vec<u8>,
}

impl FsHandler for FsWriteAtHandler {
    const NAME: &'static str = "fs_write_at";
    const ARITY: usize = 4; // (fd, off, bytes, ack)
    const VERIFYING: bool = true;

    type Args = FsWriteAtArgs;

    fn parse_content(args: &[Par]) -> Result<FsWriteAtArgs, Box<HandlerReply>> {
        let [fd_par, off_par, bytes_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (u64, u64, ByteArray)",
            ));
        };
        match (
            RhoNumber::unapply(fd_par),
            RhoNumber::unapply(off_par),
            RhoByteArray::unapply(bytes_par),
        ) {
            (Some(fd), Some(off), Some(bytes)) if off >= 0 => Ok(FsWriteAtArgs {
                fd: fd as u64,
                off: off as u64,
                bytes,
            }),
            _ => Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (u64, u64, ByteArray)",
            )),
        }
    }

    fn pre_charge_cost() -> Cost { fs_write_at_cost(0) }

    fn pre_charge_incremental(raw_args: &[Par]) -> Option<Cost> {
        // bytes_par is at slot 2 for fs_write_at (fd, off, bytes).
        let requested_bytes: u64 = raw_args
            .get(2)
            .and_then(RhoByteArray::unapply)
            .map(|b| b.len() as u64)
            .unwrap_or(0);
        Some(fs_write_at_cost(requested_bytes))
    }

    fn pre_syscall<'a>(
        ctx: SyscallCtx<'a>,
        args: &'a FsWriteAtArgs,
    ) -> Pin<Box<dyn Future<Output = Result<(), Box<HandlerReply>>> + Send + 'a>> {
        Box::pin(async move {
            // MAX_WRITE_BYTES gate first — same as fs_write.
            if args.bytes.len() as u64 > MAX_WRITE_BYTES {
                return Err(HandlerReply::boxed_err(
                    FSERR_QUOTA_EXCEEDED,
                    format!("write {} exceeds MAX_WRITE_BYTES", args.bytes.len()),
                ));
            }
            // `Some(args.off)` → journal_write_via_table records
            // WalOp::WriteAt + offset = args.off (not the shadow
            // position).
            if journal_write_via_table(ctx.handles, args.fd, &args.bytes, Some(args.off), ctx.ack)
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
        args: FsWriteAtArgs,
    ) -> Pin<Box<dyn Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            // `Some(args.off)` → write_impl_via_table dispatches to
            // libc::pwrite.
            let par = write_impl_via_table(ctx.handles, args.fd, args.bytes, Some(args.off)).await;
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
        // Same journal shape as fs_write but WITHOUT the shadow
        // position advance — POSIX pwrite doesn't move the OS-fd
        // position, so the shadow must not either.
        Box::pin(async move {
            let [_, _, bytes_par, ..] = raw_args else {
                return;
            };
            let bytes_opt = RhoByteArray::unapply(bytes_par);
            let state_source = path.state_source_reply();
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
        })
    }
}

/// `#[distributed_slice(FS_HANDLERS)]` registration.
#[distributed_slice(FS_HANDLERS)]
static FS_WRITE_AT_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsWriteAtHandler as FsHandler>::NAME,
    arity: <FsWriteAtHandler as FsHandler>::ARITY,
    verifying: <FsWriteAtHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsWriteAtHandler>(fs, args)),
    urn_suffix: "writeAt",
    fixed_channel: FixedChannels::fs_write_at,
    body_ref: BodyRefs::FS_WRITE_AT,
    family: HandlerFamily::Mutation,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::interpreter::rho_type::RhoString;

    /// Trait-level constants wire through.  ARITY = 4 (fd + off +
    /// bytes + ack); verifying.
    #[test]
    fn consts_wire_through() {
        assert_eq!(FsWriteAtHandler::NAME, "fs_write_at");
        assert_eq!(FsWriteAtHandler::ARITY, 4);
        assert!(FsWriteAtHandler::VERIFYING);
    }

    /// `parse_content` success path returns `(fd, off, bytes)`.
    #[test]
    fn parse_content_accepts_fd_off_bytes() {
        let args = vec![
            RhoNumber::create_par(7),
            RhoNumber::create_par(4096),
            RhoByteArray::create_par(vec![1, 2, 3]),
        ];
        let parsed = FsWriteAtHandler::parse_content(&args)
            .ok()
            .expect("fd + off + ByteArray parses");
        assert_eq!(parsed.fd, 7u64);
        assert_eq!(parsed.off, 4096u64);
        assert_eq!(parsed.bytes, vec![1, 2, 3]);
    }

    /// LOAD-BEARING: fd parses as u64 bit-pattern even when the
    /// signed GInt has the sign bit set.  Same discipline as
    /// fs_write / fs_truncate / fs_close.
    #[test]
    fn parse_content_accepts_negative_fd_as_u64_bitpattern() {
        let args = vec![
            RhoNumber::create_par(-1),
            RhoNumber::create_par(0),
            RhoByteArray::create_par(vec![]),
        ];
        let parsed = FsWriteAtHandler::parse_content(&args)
            .ok()
            .expect("negative fd parses as u64");
        assert_eq!(parsed.fd, u64::MAX);
    }

    /// LOAD-BEARING: a negative offset is rejected at parse.  A
    /// future refactor that treated `off as u64` as the parse path
    /// would accept the sign-bit set as a huge positive and
    /// silently diverge from the pre-trait reject-at-boundary
    /// semantics.
    #[test]
    fn parse_content_rejects_negative_offset() {
        let args = vec![
            RhoNumber::create_par(0),
            RhoNumber::create_par(-1),
            RhoByteArray::create_par(vec![1]),
        ];
        assert!(FsWriteAtHandler::parse_content(&args).is_err());
    }

    /// Empty byte array accepted — a 0-byte pwrite is a no-op the
    /// kernel returns 0 for; the WAL records a 0-length WriteAt.
    #[test]
    fn parse_content_accepts_empty_bytes() {
        let args = vec![
            RhoNumber::create_par(0),
            RhoNumber::create_par(0),
            RhoByteArray::create_par(vec![]),
        ];
        assert!(FsWriteAtHandler::parse_content(&args).is_ok());
    }

    /// `parse_content` rejects wrong arg count (empty, 1, 2, 4+).
    #[test]
    fn parse_content_rejects_wrong_arg_count() {
        let empty: Vec<Par> = vec![];
        let one = vec![RhoNumber::create_par(1)];
        let two = vec![RhoNumber::create_par(1), RhoNumber::create_par(0)];
        let four = vec![
            RhoNumber::create_par(1),
            RhoNumber::create_par(0),
            RhoByteArray::create_par(vec![1]),
            RhoNumber::create_par(2),
        ];
        assert!(FsWriteAtHandler::parse_content(&empty).is_err());
        assert!(FsWriteAtHandler::parse_content(&one).is_err());
        assert!(FsWriteAtHandler::parse_content(&two).is_err());
        assert!(FsWriteAtHandler::parse_content(&four).is_err());
    }

    /// `parse_content` rejects wrong types in any slot.
    #[test]
    fn parse_content_rejects_wrong_arg_types() {
        let wrong_fd = vec![
            RhoString::create_par("not fd".into()),
            RhoNumber::create_par(0),
            RhoByteArray::create_par(vec![1]),
        ];
        let wrong_off = vec![
            RhoNumber::create_par(0),
            RhoString::create_par("not off".into()),
            RhoByteArray::create_par(vec![1]),
        ];
        let wrong_bytes = vec![
            RhoNumber::create_par(0),
            RhoNumber::create_par(0),
            RhoString::create_par("not bytes".into()),
        ];
        assert!(FsWriteAtHandler::parse_content(&wrong_fd).is_err());
        assert!(FsWriteAtHandler::parse_content(&wrong_off).is_err());
        assert!(FsWriteAtHandler::parse_content(&wrong_bytes).is_err());
    }

    /// `pre_charge_cost` returns the setup cost `fs_write_at_cost(0)`.
    #[test]
    fn pre_charge_cost_is_setup_only() {
        let via_handler = FsWriteAtHandler::pre_charge_cost();
        let via_helper = fs_write_at_cost(0);
        assert_eq!(via_handler.value, via_helper.value);
        assert_eq!(via_handler.operation, via_helper.operation);
    }

    /// `pre_charge_incremental` reads bytes.len() from raw_args[2]
    /// (slot 2 — slot 1 is off, not bytes, unlike fs_write).
    #[test]
    fn pre_charge_incremental_reads_payload_length_from_slot_two() {
        let raw = vec![
            RhoNumber::create_par(1),
            RhoNumber::create_par(0),
            RhoByteArray::create_par(vec![0u8; 1000]),
        ];
        let cost = FsWriteAtHandler::pre_charge_incremental(&raw)
            .expect("incremental returns Some for length-parameterized handler");
        let expected = fs_write_at_cost(1000);
        assert_eq!(cost.value, expected.value);
    }

    /// LOAD-BEARING: a ByteArray at slot 1 (where off lives) is
    /// NOT mistaken for the payload.  A regression that read
    /// bytes.len() from raw_args[1] (copying fs_write's slot
    /// indexing) would charge a 1000-byte write on a 0-byte
    /// payload if the caller passed `off=ByteArray(1000)` — a
    /// cross-handler drift bug the slot-index invariant guards
    /// against.
    #[test]
    fn pre_charge_incremental_slot_two_discipline() {
        let raw = vec![
            RhoNumber::create_par(1),
            RhoByteArray::create_par(vec![0u8; 1000]), // ByteArray in off slot
            RhoByteArray::create_par(vec![0u8; 42]),   // real payload
        ];
        let cost =
            FsWriteAtHandler::pre_charge_incremental(&raw).expect("incremental still returns Some");
        let expected = fs_write_at_cost(42);
        assert_eq!(cost.value, expected.value);
    }

    /// `pre_charge_incremental` falls through to 0 bytes when the
    /// raw args don't have a ByteArray at slot 2.
    #[test]
    fn pre_charge_incremental_defaults_to_zero_bytes_on_missing_payload() {
        let raw: Vec<Par> = vec![RhoNumber::create_par(1), RhoNumber::create_par(0)];
        let cost = FsWriteAtHandler::pre_charge_incremental(&raw)
            .expect("still Some on missing bytes slot");
        let expected = fs_write_at_cost(0);
        assert_eq!(cost.value, expected.value);
    }
}
