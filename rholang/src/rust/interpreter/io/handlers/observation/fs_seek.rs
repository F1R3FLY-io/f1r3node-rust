// fs_seek — (fd, off, whence) -> [true, new_pos] | [false, FSERR_*, msg]
//
// **FIRST VERIFYING HANDLER** in the Wave 4 migration.  Combines
// the observation-handler shape from fs_tell with the verifying-
// handler protocol:
//
//   - `const VERIFYING: bool = true;` — Consensus-cmode replay
//     re-executes `libc::lseek` and verifies the fresh reply hash
//     against the leader's cached reply hash via
//     `verify_reply_hash_matches_cached` (dispatcher step 7).
//   - `resolve_replay_cmode` — reads the fd's shadow `cmode`
//     field so the dispatcher's step-4 is_replay short-circuit
//     can decide between the Oracular tautological echo path
//     (`None` or `Oracular`) and the Consensus re-execute path
//     (`Some(Consensus)`).
//   - `journal` — doesn't journal to WAL (fs_seek is a position
//     mutator, not a path mutator); the hook is used purely to
//     advance the shadow `position` field on Leader / VerifySuccess
//     / OracularEcho paths.  Skipped on VerifyDivergence — the
//     fresh reply didn't agree with the leader, so the follower's
//     local position-advance is suppressed to stay converged with
//     the leader's cached state.
//
// # verify::FdPositionMutator link
//
// `fs_seek` IS one of the three variants in
// `FdPositionMutator::ALL` (alongside `FsRead` + `FsWrite`).
// `fs_tell`'s F-3 click-through aid points code reviewers here —
// a future `fs_pread` would add `FsPreadAdvance` to the enum.
//
// # park_external_during deferral
//
// Same deferral as `fs_flush` + `fs_tell` — raw
// `tokio::task::spawn_blocking` without reduction-driver parking.
// Wave 5 scope.
//
// # Cross-handler fd-formatting audit (consensus-observable)
//
// fs_seek formats the unknown-fd FSERR message as **i64**
// (`args.fd as i64`); fs_close / fs_flush / fs_tell format as
// **u64** (`args.fd` directly).  This is intentional — per-trait
// byte identity is preserved against each pre-trait handler's
// original format string:
//
//   | Handler        | Pre-trait format | Rationale                           |
//   |----------------|------------------|-------------------------------------|
//   | fs_seek        | `{fd as i64}`    | Pre-trait fs_seek used i64 directly |
//   | fs_close       | `{fd as u64}`    | Pre-trait fs_close used u64         |
//   | fs_flush       | `{fd as u64}`    | Pre-trait fs_flush used u64         |
//   | fs_tell        | `{fd as u64}`    | Pre-trait fs_tell used u64          |
//
// Under Wave 6, these messages are consensus-observable (part of
// the FSERR reply Par the Rholang caller pattern-matches on).
// A future "harmonize all handlers to u64" refactor would
// silently drift fs_seek's unknown-fd message bytes — a
// consensus-observable regression masquerading as a cleanup.
//
// If / when a cross-handler message-format audit lands as a
// consensus-coordinated hard-fork change, the whole table above
// moves together.  Until then, each handler preserves its own
// pre-trait format.

use std::future::Future;
use std::os::fd::AsRawFd;
use std::pin::Pin;

use linkme::distributed_slice;
use models::rhoapi::Par;
use tokio::task::spawn_blocking;

use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::io::costs::fs_seek_cost;
use crate::rust::interpreter::io::errors::{
    io_err_code, join_err_abort, FSERR_BAD_ARG, FSERR_CLOSED,
};
use crate::rust::interpreter::io::handler_trait::{
    dispatch_via_trait_owned, FsHandler, FsHandlerEntry, HandlerFamily, HandlerReply, JournalPath,
    SyscallCtx, FS_HANDLERS,
};
use crate::rust::interpreter::io::path::io_msg_scrub;
use crate::rust::interpreter::io::response::{extract_ok_u64, ok_u64};
use crate::rust::interpreter::io::ConsensusMode;
use crate::rust::interpreter::rho_type::{RhoNumber, RhoString};
use crate::rust::interpreter::system_processes::{BodyRefs, FixedChannels};

/// Zero-sized handler type.
pub struct FsSeekHandler;

/// Parsed args for [`FsSeekHandler`].
///
/// `whence` is a `libc::c_int` discriminant for `SEEK_SET` /
/// `SEEK_CUR` / `SEEK_END` — translated from the caller's
/// `"set"` / `"cur"` / `"end"` string in `parse_content`.  Keeping
/// the libc type here lets `dispatch`'s `libc::lseek` call take
/// the discriminant directly without re-translation.
pub struct FsSeekArgs {
    fd: u64,
    off: i64,
    whence: libc::c_int,
}

impl FsHandler for FsSeekHandler {
    const NAME: &'static str = "fs_seek";
    const ARITY: usize = 4; // (fd, off, whence, ack)
    const VERIFYING: bool = true;

    type Args = FsSeekArgs;

    fn parse_content(args: &[Par]) -> Result<FsSeekArgs, Box<HandlerReply>> {
        let [fd_par, off_par, whence_par] = args else {
            return Err(HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "expected (u64, i64, String)",
            ));
        };
        // First tier: all three args must parse as the expected
        // Rholang types.  Shared error message per pre-trait
        // fs_seek.
        let combined_err = || HandlerReply::boxed_err(FSERR_BAD_ARG, "expected (u64, i64, String)");
        let fd = RhoNumber::unapply(fd_par).ok_or_else(combined_err)?;
        let off = RhoNumber::unapply(off_par).ok_or_else(combined_err)?;
        let w = RhoString::unapply(whence_par).ok_or_else(combined_err)?;
        // Second tier: whence must be in {set, cur, end}, and
        // "set" additionally requires `off >= 0` (SEEK_SET with
        // negative off is EINVAL on all platforms, but libc still
        // sets errno; we reject earlier for a clearer FSERR).
        // Distinct pre-trait error message.
        let whence = match w.as_str() {
            "set" if off >= 0 => libc::SEEK_SET,
            "cur" => libc::SEEK_CUR,
            "end" => libc::SEEK_END,
            _ => {
                return Err(HandlerReply::boxed_err(
                    FSERR_BAD_ARG,
                    "expected whence in {set,cur,end}",
                ));
            }
        };
        Ok(FsSeekArgs {
            fd: fd as u64,
            off,
            whence,
        })
    }

    fn pre_charge_cost() -> Cost { fs_seek_cost() }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsSeekArgs,
    ) -> Pin<Box<dyn Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            let file_arc = match ctx.handles.raw_fd(args.fd).await {
                Some(f) => f,
                None => {
                    // Format `fd` as signed i64 to match pre-trait
                    // fs_seek byte-identity (RhoNumber::unapply
                    // returns i64; the handler stores it as u64
                    // internally for the shadow lookup).  Cast
                    // back for the error message so wire bytes
                    // match.
                    return HandlerReply::err(
                        FSERR_CLOSED,
                        format!("unknown fd {}", args.fd as i64),
                    );
                }
            };
            let r = spawn_blocking(move || {
                let raw_fd = file_arc.as_raw_fd();
                // SAFETY: raw_fd derives from file_arc: Arc<File>
                // whose lifetime spans this closure; fd is open
                // for the call.  `lseek` accepts any integer fd
                // and returns -1 with errno on invalid fd or
                // offset.
                unsafe {
                    let pos = libc::lseek(raw_fd, args.off, args.whence);
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

    /// Verifying-handler cmode resolution.  The dispatcher's
    /// step-4 is_replay short-circuit calls this to decide
    /// between the Oracular tautological echo path and the
    /// Consensus re-execute + verify path.
    ///
    /// Reads the fd's shadow `cmode` field via `with_mut` (which
    /// takes a write lock internally).  `None` return (unknown
    /// fd) → dispatcher takes the Oracular echo path — matches
    /// pre-trait behavior where an unresolved cmode on replay
    /// tautologically echoed `previous`.
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

    /// Position-advance hook.  fs_seek does NOT journal to WAL
    /// (seek is a position mutation, not a persistent data
    /// mutation); the "journal" hook here is purely for shadow
    /// position advance so leader + follower stay converged
    /// across the lseek.
    ///
    /// # Path handling
    ///
    ///   - Leader / VerifySuccess / OracularEcho: extract the
    ///     reply's `ok_u64(pos)` and set `shadow.position =
    ///     new_pos`.  The leader's reply hash matches the
    ///     follower's by construction on these paths.
    ///   - VerifyDivergence: skip (pre-trait fs_seek's
    ///     `Err(reason) =>` branch does not touch shadow —
    ///     keeping the follower's shadow consistent with the
    ///     leader's cached state matters more than reflecting
    ///     the diverged syscall outcome).
    fn journal<'a>(
        ctx: SyscallCtx<'a>,
        raw_args: &'a [Par],
        path: JournalPath<'a>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            if path.is_divergence() {
                return;
            }
            let [fd_par, ..] = raw_args else {
                return;
            };
            let Some(fd_i64) = RhoNumber::unapply(fd_par) else {
                return;
            };
            // `state_source_reply` on Leader / VerifySuccess
            // returns the fresh reply; on OracularEcho returns
            // `previous.first()`.  Both paths carry the
            // ok_u64(pos) payload — extract and advance.
            let state_source = path.state_source_reply();
            if let Some(new_pos) = extract_ok_u64(std::slice::from_ref(state_source)) {
                let _ = ctx
                    .handles
                    .with_mut(fd_i64 as u64, |h| h.position = new_pos)
                    .await;
            }
        })
    }
}

/// `#[distributed_slice(FS_HANDLERS)]` registration.
#[distributed_slice(FS_HANDLERS)]
static FS_SEEK_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsSeekHandler as FsHandler>::NAME,
    arity: <FsSeekHandler as FsHandler>::ARITY,
    verifying: <FsSeekHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsSeekHandler>(fs, args)),
    urn_suffix: "seek",
    fixed_channel: FixedChannels::fs_seek,
    body_ref: BodyRefs::FS_SEEK,
    family: HandlerFamily::Observation,
};

#[cfg(test)]
mod tests {
    use super::*;

    /// Trait-level constants wire through — including VERIFYING=true
    /// which marks this as the first verifying handler.
    #[test]
    fn consts_wire_through() {
        assert_eq!(FsSeekHandler::NAME, "fs_seek");
        assert_eq!(FsSeekHandler::ARITY, 4);
        assert!(FsSeekHandler::VERIFYING, "fs_seek is a verifying handler");
    }

    /// Happy path: all three args parse + whence is accepted.
    #[test]
    fn parse_content_accepts_cur_whence() {
        let args = vec![
            RhoNumber::create_par(42),
            RhoNumber::create_par(10),
            RhoString::create_par("cur".to_string()),
        ];
        let parsed = FsSeekHandler::parse_content(&args)
            .ok()
            .expect("valid args parse");
        assert_eq!(parsed.fd, 42u64);
        assert_eq!(parsed.off, 10i64);
        assert_eq!(parsed.whence, libc::SEEK_CUR);
    }

    #[test]
    fn parse_content_accepts_set_with_nonneg_off() {
        let args = vec![
            RhoNumber::create_par(1),
            RhoNumber::create_par(0),
            RhoString::create_par("set".to_string()),
        ];
        let parsed = FsSeekHandler::parse_content(&args)
            .ok()
            .expect("set+0 parses");
        assert_eq!(parsed.whence, libc::SEEK_SET);
    }

    #[test]
    fn parse_content_accepts_end_whence() {
        let args = vec![
            RhoNumber::create_par(1),
            RhoNumber::create_par(-5),
            RhoString::create_par("end".to_string()),
        ];
        let parsed = FsSeekHandler::parse_content(&args)
            .ok()
            .expect("end parses");
        assert_eq!(parsed.whence, libc::SEEK_END);
        assert_eq!(parsed.off, -5);
    }

    /// LOAD-BEARING: `"set"` whence with negative `off` MUST be
    /// rejected at parse time.  SEEK_SET with negative off is
    /// EINVAL on every platform; rejecting earlier gives the
    /// caller a clearer FSERR than the raw errno.  Pin against a
    /// future refactor that moved the check into dispatch (which
    /// would surface the error as FSERR_IO, not FSERR_BAD_ARG).
    #[test]
    fn parse_content_rejects_set_with_negative_off() {
        let args = vec![
            RhoNumber::create_par(1),
            RhoNumber::create_par(-1),
            RhoString::create_par("set".to_string()),
        ];
        assert!(FsSeekHandler::parse_content(&args).is_err());
    }

    /// Whence outside {set, cur, end} rejected.
    #[test]
    fn parse_content_rejects_unknown_whence() {
        let args = vec![
            RhoNumber::create_par(1),
            RhoNumber::create_par(0),
            RhoString::create_par("start".to_string()),
        ];
        assert!(FsSeekHandler::parse_content(&args).is_err());
    }

    /// u64 bit-pattern discipline — same grep-audit pin.
    #[test]
    fn parse_content_accepts_negative_fd_as_u64_bitpattern() {
        let args = vec![
            RhoNumber::create_par(-1),
            RhoNumber::create_par(0),
            RhoString::create_par("cur".to_string()),
        ];
        let parsed = FsSeekHandler::parse_content(&args)
            .ok()
            .expect("negative fd parses as u64");
        assert_eq!(parsed.fd, u64::MAX);
    }

    #[test]
    fn parse_content_rejects_wrong_arg_count() {
        let empty: Vec<Par> = vec![];
        let two = vec![RhoNumber::create_par(1), RhoNumber::create_par(2)];
        assert!(FsSeekHandler::parse_content(&empty).is_err());
        assert!(FsSeekHandler::parse_content(&two).is_err());
    }

    #[test]
    fn parse_content_rejects_non_int_fd() {
        let args = vec![
            RhoString::create_par("not a fd".to_string()),
            RhoNumber::create_par(0),
            RhoString::create_par("cur".to_string()),
        ];
        assert!(FsSeekHandler::parse_content(&args).is_err());
    }

    #[test]
    fn pre_charge_cost_delegates_to_costs_helper() {
        let via_handler = FsSeekHandler::pre_charge_cost();
        let via_helper = fs_seek_cost();
        assert_eq!(via_handler.value, via_helper.value);
        assert_eq!(via_handler.operation, via_helper.operation);
    }
}
