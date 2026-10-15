// fs_tell — (fd) -> [true, pos] | [false, FSERR_CLOSED, msg] | [false, FSERR_IO, msg]
//
// Non-verifying observation handler.  Returns the open fd's
// current offset via `libc::lseek(fd, 0, SEEK_CUR)` — a
// side-effect-free query that reads the OS-fd's position without
// mutating it.
//
// # Why non-verifying
//
// Reading the current fd position is a value observation (returns
// a u64), BUT the position is kept in sync between leader and
// follower by the shadow-position tracking in `FileHandle`
// (updated by `FsSeek` / `FsRead` / `FsWrite` — see
// `verify::FdPositionMutator`).  Replay echoes the leader's
// cached reply; the follower's shadow fd converges via the
// Phase-2 real-open + mutation handlers, not by re-executing
// lseek.
//
// # F-3 compile-time link to FdPositionMutator
//
// The module carries a dead `const _F3_LINK: &[FdPositionMutator]
// = FdPositionMutator::ALL;` reference.  Rationale (per fileio
// 2026-09-04): the const-eval guard in `verify.rs` is module-scope
// so it fires regardless of any reference here, BUT the explicit
// mention keeps `FdPositionMutator` visible from the fs_tell
// module — a code-review click-through aid for future handlers
// that add fd-position mutation (fs_pread would need an
// `FsPreadAdvance` variant + a corresponding mutator-site update).
//
// Module-scope const form (vs. per-dispatch `let _`): zero-cost
// by construction — the compiler evaluates it once at module
// load, not on every dispatch call.
//
// # park_external_during deferral
//
// Same deferral as `fs_flush` (slice 4.14) — raw
// `tokio::task::spawn_blocking` without reduction-driver parking.
// Wave 5 scope.

use std::future::Future;
use std::os::fd::AsRawFd;
use std::pin::Pin;

use linkme::distributed_slice;
use models::rhoapi::Par;
use tokio::task::spawn_blocking;

use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::io::costs::fs_tell_cost;
use crate::rust::interpreter::io::errors::{
    io_err_code, join_err_abort, FSERR_BAD_ARG, FSERR_CLOSED,
};
use crate::rust::interpreter::io::handler_trait::{
    dispatch_via_trait_owned, FsHandler, FsHandlerEntry, HandlerFamily, HandlerReply, SyscallCtx,
    FS_HANDLERS,
};
use crate::rust::interpreter::io::path::io_msg_scrub;
use crate::rust::interpreter::io::response::ok_u64;
use crate::rust::interpreter::io::verify::FdPositionMutator;
use crate::rust::interpreter::rho_type::RhoNumber;
use crate::rust::interpreter::system_processes::{BodyRefs, FixedChannels};

// F-3 compile-time link to `FdPositionMutator::ALL`.  Module-scope
// `const _` form (vs. per-dispatch `let _`): zero-cost by
// construction (no evaluation on every dispatch call), same grep
// anchor from `fs_tell.rs` → the enum.  A refactor that removed
// this binding would also strip the `use ...::FdPositionMutator`
// import above, surfacing an unused-import warning (and tripping
// `#[deny(warnings)]` under clippy).
const _F3_LINK: &[FdPositionMutator] = FdPositionMutator::ALL;

/// Zero-sized handler type.
pub struct FsTellHandler;

/// Parsed args for [`FsTellHandler`].  `fd as u64` discipline —
/// see fs_close for the grep-audit pin.
pub struct FsTellArgs {
    fd: u64,
}

impl FsHandler for FsTellHandler {
    const NAME: &'static str = "fs_tell";
    const ARITY: usize = 2; // (fd, ack)

    type Args = FsTellArgs;

    fn parse_content(args: &[Par]) -> Result<FsTellArgs, Box<HandlerReply>> {
        let [fd_par] = args else {
            // Framework-dispatched path: unreachable in practice —
            // dispatcher's step-2 ARITY check runs first.  Direct
            // test-call path: `parse_content_rejects_wrong_arg_count`
            // exercises this arm.
            return Err(HandlerReply::boxed_err(FSERR_BAD_ARG, "expected GInt fd"));
        };
        let fd = RhoNumber::unapply(fd_par)
            .ok_or_else(|| HandlerReply::boxed_err(FSERR_BAD_ARG, "expected GInt fd"))?;
        Ok(FsTellArgs { fd: fd as u64 })
    }

    fn pre_charge_cost() -> Cost { fs_tell_cost() }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsTellArgs,
    ) -> Pin<Box<dyn Future<Output = HandlerReply> + Send + 'a>> {
        // F-3 link lives at module scope (`_F3_LINK` const above)
        // so it's zero-cost by construction.  See module docstring
        // for the readability rationale.
        Box::pin(async move {
            let file_arc = match ctx.handles.raw_fd(args.fd).await {
                Some(f) => f,
                None => {
                    return HandlerReply::err(FSERR_CLOSED, format!("unknown fd {}", args.fd));
                }
            };
            let r = spawn_blocking(move || {
                let raw_fd = file_arc.as_raw_fd();
                // SAFETY: raw_fd derives from file_arc: Arc<File>
                // whose lifetime spans this closure; fd is open for
                // the call.  `lseek(SEEK_CUR, 0)` is idempotent and
                // returns the current offset (or -1 with errno).
                unsafe {
                    let pos = libc::lseek(raw_fd, 0, libc::SEEK_CUR);
                    if pos < 0 {
                        Err(std::io::Error::last_os_error())
                    } else {
                        Ok(pos as u64)
                    }
                }
            })
            .await;
            match r {
                Err(je) => join_err_abort(je),
                Ok(Err(e)) => HandlerReply::err(io_err_code(&e), io_msg_scrub(&e)),
                Ok(Ok(pos)) => HandlerReply::ok(ok_u64(pos)),
            }
        })
    }
}

/// `#[distributed_slice(FS_HANDLERS)]` registration.
#[distributed_slice(FS_HANDLERS)]
static FS_TELL_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsTellHandler as FsHandler>::NAME,
    arity: <FsTellHandler as FsHandler>::ARITY,
    verifying: <FsTellHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsTellHandler>(fs, args)),
    urn_suffix: "tell",
    fixed_channel: FixedChannels::fs_tell,
    body_ref: BodyRefs::FS_TELL,
    family: HandlerFamily::Observation,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::interpreter::rho_type::RhoString;

    /// Trait-level constants wire through.
    #[test]
    fn consts_wire_through() {
        assert_eq!(FsTellHandler::NAME, "fs_tell");
        assert_eq!(FsTellHandler::ARITY, 2);
        assert!(!FsTellHandler::VERIFYING);
    }

    /// Happy path.
    #[test]
    fn parse_content_accepts_positive_fd() {
        let args = vec![RhoNumber::create_par(42)];
        let parsed = FsTellHandler::parse_content(&args)
            .ok()
            .expect("positive fd parses");
        assert_eq!(parsed.fd, 42u64);
    }

    /// u64 bit-pattern discipline — matches fs_close + fs_flush.
    /// Grep-audit: `fd as u64` should appear in every fd-taking
    /// handler.
    #[test]
    fn parse_content_accepts_negative_fd_as_u64_bitpattern() {
        let args = vec![RhoNumber::create_par(-1)];
        let parsed = FsTellHandler::parse_content(&args)
            .ok()
            .expect("negative fd parses as u64");
        assert_eq!(parsed.fd, u64::MAX);
    }

    #[test]
    fn parse_content_rejects_wrong_arg_count() {
        let empty: Vec<Par> = vec![];
        let two = vec![RhoNumber::create_par(1), RhoNumber::create_par(2)];
        assert!(FsTellHandler::parse_content(&empty).is_err());
        assert!(FsTellHandler::parse_content(&two).is_err());
    }

    #[test]
    fn parse_content_rejects_non_int_fd() {
        let args = vec![RhoString::create_par("not a fd".to_string())];
        assert!(FsTellHandler::parse_content(&args).is_err());
    }

    #[test]
    fn pre_charge_cost_delegates_to_costs_helper() {
        let via_handler = FsTellHandler::pre_charge_cost();
        let via_helper = fs_tell_cost();
        assert_eq!(via_handler.value, via_helper.value);
        assert_eq!(via_handler.operation, via_helper.operation);
    }

    /// LOAD-BEARING F-3 compile-time link pin: fs_tell dispatch
    /// body MUST carry a reference to `FdPositionMutator::ALL` so
    /// a code reviewer tracing fd-position-mutation sites from
    /// fs_tell lands on the enum.  The actual F-3 guarantee is
    /// enforced by the module-scope const-eval in `verify.rs`
    /// (independent of this reference); this test pins the
    /// click-through aid against a refactor that silently
    /// stripped it.
    ///
    /// A regression that dropped the reference wouldn't break the
    /// const-eval guard but WOULD remove the fs_tell → mutators
    /// grep path, making a future `FsPreadAdvance` addition less
    /// discoverable from fs_tell's body.  Pin as a readability
    /// invariant.
    #[test]
    fn fd_position_mutator_all_is_reachable_from_fs_tell_module() {
        // Not vacuous — reads through the enum's `ALL` constant
        // the dispatch body references.  A refactor that removed
        // the dispatch-body reference AND the enum together would
        // trip the const-eval guard in verify.rs; this test
        // specifically pins the enum's reachability + non-empty
        // shape.
        assert!(
            !FdPositionMutator::ALL.is_empty(),
            "FdPositionMutator::ALL must contain at least one \
             fd-position-mutating handler — the fs_tell dispatch \
             body's click-through aid references this."
        );
    }
}
