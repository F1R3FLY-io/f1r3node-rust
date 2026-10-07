// The per-syscall trait every fs_* handler impl satisfies.  Combined
// with the generic dispatcher
// [`dispatch_via_trait<H>`](super::dispatch::dispatch_via_trait)
// (slice 4.7), the trait defines the exact handler contract:
//
//   1. [`FsHandler::NAME`] / [`FsHandler::ARITY`] — identity +
//      wire-format pin.
//   2. [`FsHandler::VERIFYING`] — whether this handler re-executes
//      + verifies the reply hash on `is_replay = true` under
//      Consensus cmode.  Defaults to `false` (tautological echo).
//   3. [`FsHandler::parse_content`] — extract typed `Args` from the
//      caller's pre-ack `Par` slice.  Shape-level errors (wrong
//      number of args) are rejected by the framework via
//      `illegal_argument_error(NAME)`; content-level errors
//      (wrong Par type in a slot) return a `HandlerReply::err`
//      that the framework produces to ack.
//   4. [`FsHandler::pre_charge_cost`] /
//      [`FsHandler::pre_charge_incremental`] — the cost reservation
//      the framework charges before dispatch.  Incremental overrides
//      allow zero-weight (empty read, zero-length write, empty dir).
//   5. [`FsHandler::dispatch`] — the syscall body itself.  Returns
//      a `HandlerReply`; the framework produces
//      `vec![reply.into_par()]` to ack.
//   6. [`FsHandler::pre_syscall`] (optional) — path-mutation handlers
//      pre-append a WAL entry with a Success placeholder before the
//      syscall runs; the placeholder is patched to Failure by
//      `journal` after the outcome is known.
//   7. [`FsHandler::on_replay_side_effect`] (optional) — side effect
//      on the non-verifying `is_replay` branch BEFORE the
//      tautological echo (shadow fd install, deploy-scope sweep,
//      etc.).
//   8. [`FsHandler::post_reply_supplement`] (optional) — length-
//      parameterized handlers charge the per-entry supplement AFTER
//      the reply is known.  Single SUPPLEMENT, matching the base
//      charge in `pre_charge_cost`.
//   9. [`FsHandler::resolve_replay_cmode`] (verifying only) —
//      determine the caller's `ConsensusMode` on `is_replay = true`
//      so the framework can pick between the Oracular tautological
//      echo path and the Consensus re-execute + verify path.
//   10. [`FsHandler::journal`] (optional) — journal the reply to
//      the WAL.  Framework calls at four semantic sites discriminated
//      by [`JournalPath`].
//
// # Dispatch order (slice 4.7 framework)
//
// For reference — the `dispatch_via_trait<H>` loop runs these
// steps in order:
//
//   Step 1: `is_contract_call.unapply(contract_args)` — framework
//     shape check.  Rejects with `illegal_argument_error`.
//   Step 2: Arity check (`args.len() == H::ARITY`).
//   Step 3: Pre-charge via `metering.reserve_primitive`
//     (`pre_charge_cost`) or `reserve_incremental_primitive`
//     (`pre_charge_incremental`).
//   Step 4: `is_replay` short-circuit — verifying handler under
//     Consensus cmode falls through; everything else runs
//     `on_replay_side_effect` + optional `journal(OracularEcho)` +
//     echoes `previous`.
//   Step 5: `parse_content` — content-level errors reply via
//     `HandlerReply::err`.
//   Step 5b: `pre_syscall` — pre-append WAL entry.  Returns early
//     via `HandlerReply::err` on WAL-cap exhaustion.
//   Step 6: `dispatch` — the syscall body.
//   Step 7: Verify (Consensus follower only) → `journal` →
//     `post_reply_supplement` → produce to ack.
//
// # Trait-exempt handler (fs_remove_dir)
//
// Fileio's `fs_remove_dir` is NOT a `FsHandler` impl — its four
// divergence reply shapes + per-entry WAL journaling inside the
// recursive Consensus walk don't fit the per-step hook contract
// without adding a one-off `divergence_reply(args, reason)` trait
// method used by only this handler.
//
// Current state (dev): a stub replies with `[false,
// "FSERR_UNSUPPORTED", ...]` on the ack channel (see
// `SystemProcesses::fs_remove_dir_stub` + the explicit
// Definition registration in
// `rho_runtime::dispatch_table_creator`, both added in slice 5.44).
// The real DD-RemoveDirReplyShape handler with its four divergence
// shapes lands at a future Wave 4 handler slice.

use std::future::Future;
use std::pin::Pin;

use models::rhoapi::Par;

use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::io::handler_trait::journal_path::JournalPath;
use crate::rust::interpreter::io::handler_trait::reply::HandlerReply;
use crate::rust::interpreter::io::handler_trait::syscall_ctx::SyscallCtx;
use crate::rust::interpreter::io::ConsensusMode;

/// One impl per fs_* syscall.  The dispatcher
/// [`dispatch_via_trait`](super::dispatch::dispatch_via_trait)
/// (slice 4.7) calls these methods in a fixed order; see the
/// module docstring for the step-by-step dispatch.
pub trait FsHandler {
    /// Human-readable handler name.  Framework passes this to
    /// `illegal_argument_error(NAME)` on shape mismatch, stores it
    /// as `FsHandlerEntry.name` (slice 4.7), and surfaces it in
    /// cost / WAL / consensus-divergence replies.  Must match the
    /// spec-canonical literal (e.g., `"fs_flush"`, `"fs_read"`).
    const NAME: &'static str;

    /// Total argument count INCLUDING the trailing ack channel.
    /// The pre-ack args are `args[..ARITY-1]`; ack is `args[ARITY-1]`.
    /// Framework arity check (`args.len() == ARITY`) rejects
    /// mismatches with `illegal_argument_error(NAME)`.
    const ARITY: usize;

    /// `false` = non-verifying — tautological echo of `previous` on
    /// `is_replay = true`.  `true` = re-execute + verify the fresh
    /// reply's hash against the leader's cached hash on Consensus
    /// cmode (Oracular cmode still echoes).
    ///
    /// # Implicit-default hazard
    ///
    /// `VERIFYING` defaults to `false`.  If a new handler is added
    /// for a re-execute-verify op and the author forgets
    /// `const VERIFYING: bool = true;`, the framework silently
    /// treats it as non-verifying → tautological echo on replay →
    /// follower never re-executes → divergence undetectable at the
    /// fs layer.  Discipline is enforced at the `FS_HANDLERS`
    /// registry layer by two runtime pins (slices 5.50 + 5.53): a
    /// whole-slice verifying-count assertion
    /// (`EXPECTED_VERIFYING_HANDLER_COUNT`) and a per-family
    /// verifying-count assertion
    /// (`EXPECTED_PER_FAMILY_VERIFYING_COUNTS`).  A drop in either
    /// trips the regression at test time rather than at consensus
    /// time.
    const VERIFYING: bool = false;

    /// Parsed content-arg type produced by [`parse_content`](Self::parse_content)
    /// and consumed by [`dispatch`](Self::dispatch).  `Send +
    /// 'static` so it can move into the async block of the handler
    /// body.
    type Args: Send + 'static;

    /// Extract typed args from `args[..ARITY-1]` (the framework has
    /// already sliced off ack).  Returns
    /// `Err(HandlerReply::boxed_err(code, msg))` on type-level
    /// mismatch (e.g., an integer slot got a string Par); the
    /// framework produces the reply to ack and returns.
    ///
    /// The Err arm is boxed because `HandlerReply` wraps a Par
    /// (~296 bytes); `Result<T, HandlerReply>` fires
    /// `clippy::result_large_err` at the pedantic threshold.
    /// Handlers use [`HandlerReply::boxed_err`] for the one-line
    /// construction.
    fn parse_content(args: &[Par]) -> Result<Self::Args, Box<HandlerReply>>;

    /// Handler's constant-work cost weight.  Framework reserves
    /// via `metering.reserve_primitive` when
    /// [`pre_charge_incremental`](Self::pre_charge_incremental)
    /// returns None (default).  Must return a positive-value
    /// `Cost` by construction — [`crate::rust::interpreter::io::costs`]
    /// provides the canonical helpers (e.g., `fs_open_cost()`).
    fn pre_charge_cost() -> Cost;

    /// Length-parameterized cost override.  Returns `Some(cost)`
    /// for handlers whose cost depends on caller-supplied args
    /// (fs_read / fs_read_at / fs_write / fs_write_at / fs_entries).
    /// Framework then reserves via
    /// `metering.reserve_incremental_primitive(cost)` (which
    /// allows zero-weight, unlike `reserve_primitive`).  Called
    /// with the raw pre-ack args.
    ///
    /// # Single-event discipline
    ///
    /// This is a SINGLE-EVENT charge, not a base + supplement.
    /// Splitting into two events would change the Wave 6 cost
    /// witness fold bytes and split peering.  See
    /// [`post_reply_supplement`](Self::post_reply_supplement) for
    /// the two-event discipline when a per-entry supplement fires
    /// AFTER the reply is known.
    fn pre_charge_incremental(_raw_args: &[Par]) -> Option<Cost> { None }

    /// Syscall + reply generation.  Runs on the leader path AND on
    /// the Consensus-follower verifying-replay path (that's the
    /// point of "re-execute + verify").  Returns a
    /// [`HandlerReply`]; the framework handles produce / ack.
    ///
    /// The future's lifetime is bounded by the passed
    /// [`SyscallCtx<'a>`] — the context's borrowed refs are held
    /// alive for the future's duration by the dispatcher's call
    /// site (slice 4.7).
    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: Self::Args,
    ) -> Pin<Box<dyn Future<Output = HandlerReply> + Send + 'a>>;

    /// Optional pre-syscall hook — runs AFTER [`parse_content`](Self::parse_content)
    /// but BEFORE [`dispatch`](Self::dispatch) on the leader path +
    /// the Consensus-follower verifying-replay fall-through path.
    /// Framework SKIPS the hook on the Oracular tautological echo
    /// path (`parse_content` isn't called on that path either).
    ///
    /// Returns `Err(boxed_reply)` to produce the reply and
    /// return early (used for `FSERR_QUOTA_EXCEEDED` when the WAL
    /// is at cap).  Default: no-op `Ok(())`.
    fn pre_syscall<'a>(
        _ctx: SyscallCtx<'a>,
        _args: &'a Self::Args,
    ) -> Pin<Box<dyn Future<Output = Result<(), Box<HandlerReply>>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }

    /// Optional side effect on the non-verifying `is_replay`
    /// branch BEFORE the tautological echo of `previous` to ack.
    /// Default: no-op.
    ///
    /// Takes the raw pre-ack `Par` slice (`args[..ARITY-1]`)
    /// rather than a parsed [`Self::Args`] — matches the pre-
    /// refactor `is_replay` branch of fs_* handlers that parsed
    /// args OPPORTUNISTICALLY on replay.  Running `parse_content`
    /// before the is_replay short-circuit would surface an
    /// `FSERR_BAD_ARG` reply that would diverge from what the
    /// leader produced (which cached the real reply in `previous`).
    ///
    /// Ack is available on `ctx.ack` if the hook needs to journal.
    fn on_replay_side_effect<'a>(
        _ctx: SyscallCtx<'a>,
        _raw_args: &'a [Par],
        _previous: &'a [Par],
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async {})
    }

    /// Optional post-reply cost supplement — for length-
    /// parameterized handlers that charge in two events
    /// (fs_entries, fs_entries_stream_next, fs_remove_dir).
    /// Default: no supplement.
    ///
    /// Called by the framework AFTER [`dispatch`](Self::dispatch)
    /// (or after the `is_replay` echo, using `previous` as the
    /// reply slice).  Returns `Some(cost)` to charge via
    /// `metering.reserve_incremental_primitive`.
    ///
    /// `reply` is the caller-facing reply slice — a 1-element
    /// `&[Par]` containing the reply Par.  Handlers inspect its
    /// shape (via `reply_is_ok` or similar) to compute the per-
    /// entry supplement weight from the entry count.
    fn post_reply_supplement(_reply: &[Par]) -> Option<Cost> { None }

    /// For verifying handlers (`VERIFYING = true`): determine the
    /// caller-supplied [`ConsensusMode`] from the raw pre-ack args
    /// or a context lookup (fs_size reads the fd's shadow).
    /// Framework calls this at the `is_replay` short-circuit to
    /// pick between the Oracular tautological echo and the
    /// Consensus re-execute + verify path.
    ///
    /// Returns `None` if cmode can't be determined (bad cmode
    /// string on fs_stat, unknown fd on fs_size).  In that case
    /// the framework takes the Oracular path — matches pre-trait
    /// behavior where an unresolved cmode on replay tautologically
    /// echoed `previous`.
    ///
    /// Non-verifying handlers do NOT override this; the framework
    /// skips the call.  Default returns `None`.
    fn resolve_replay_cmode<'a>(
        _ctx: SyscallCtx<'a>,
        _raw_args: &'a [Par],
    ) -> Pin<Box<dyn Future<Output = Option<ConsensusMode>> + Send + 'a>> {
        Box::pin(async { None })
    }

    /// For verifying handlers that journal to the WAL: journal the
    /// reply + perform any handler-specific state advance (shadow
    /// position for fs_read / fs_write).  Framework calls at four
    /// semantic sites discriminated by [`JournalPath`] (see that
    /// enum's variants).  Default: no-op (non-journaling handlers
    /// skip).
    fn journal<'a>(
        _ctx: SyscallCtx<'a>,
        _raw_args: &'a [Par],
        _path: JournalPath<'a>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async {})
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::*;
    use crate::rust::interpreter::io::errors::FSERR_BAD_ARG;
    use crate::rust::interpreter::io::response;

    // A minimal impl exercising every default + required method so
    // the trait's shape is pinned at test time.  The dispatch body
    // doesn't touch ctx — that lets us exercise `parse_content`,
    // `pre_charge_cost`, and the per-default methods without
    // constructing a real `SyscallCtx` (which would need async
    // rspace init).  Real-handler dispatch is tested indirectly
    // when per-family handlers land (slices 4.7+).

    struct TestHandler;

    struct TestArgs {
        value: u64,
    }

    impl FsHandler for TestHandler {
        const NAME: &'static str = "fs_test";
        const ARITY: usize = 2; // one data arg + ack
        type Args = TestArgs;

        fn parse_content(args: &[Par]) -> Result<Self::Args, Box<HandlerReply>> {
            // Shape: one Par slot encoded as a u64.  Framework
            // sliced off ack; `args` is the one pre-ack arg.
            if args.len() != 1 {
                return Err(HandlerReply::boxed_err(
                    FSERR_BAD_ARG,
                    "need exactly one arg",
                ));
            }
            // Minimal parse — accept any Par and return a fixed
            // value.  Real handlers would unapply RhoNumber here.
            let _ = &args[0];
            Ok(TestArgs { value: 42 })
        }

        fn pre_charge_cost() -> Cost {
            Cost {
                value: 100,
                operation: Cow::Borrowed("fs_test"),
            }
        }

        fn dispatch<'a>(
            _ctx: SyscallCtx<'a>,
            args: Self::Args,
        ) -> Pin<Box<dyn Future<Output = HandlerReply> + Send + 'a>> {
            Box::pin(async move { HandlerReply::ok(response::ok_u64(args.value)) })
        }
    }

    // A second impl that overrides the optional methods to prove
    // the override path reaches the trait reads (not just the
    // defaults).  `VERIFYING = true` + `pre_charge_incremental` +
    // `post_reply_supplement` exercises the surface that
    // `TestHandler` leaves defaulted.

    struct TestIncrementalHandler;

    impl FsHandler for TestIncrementalHandler {
        const NAME: &'static str = "fs_test_incremental";
        const ARITY: usize = 2;
        const VERIFYING: bool = true;
        type Args = ();

        fn parse_content(_args: &[Par]) -> Result<Self::Args, Box<HandlerReply>> { Ok(()) }

        fn pre_charge_cost() -> Cost {
            Cost {
                value: 100,
                operation: Cow::Borrowed("fs_test_incremental"),
            }
        }

        fn pre_charge_incremental(raw_args: &[Par]) -> Option<Cost> {
            // Return a length-parameterized cost based on raw arg
            // count — contrived but exercises the override path.
            Some(Cost {
                value: 10 + raw_args.len() as i64,
                operation: Cow::Borrowed("fs_test_incremental_incr"),
            })
        }

        fn post_reply_supplement(reply: &[Par]) -> Option<Cost> {
            Some(Cost {
                value: reply.len() as i64,
                operation: Cow::Borrowed("fs_test_incremental_supplement"),
            })
        }

        fn dispatch<'a>(
            _ctx: SyscallCtx<'a>,
            _args: Self::Args,
        ) -> Pin<Box<dyn Future<Output = HandlerReply> + Send + 'a>> {
            Box::pin(async { HandlerReply::ok(response::ok_bare()) })
        }
    }

    /// Required associated items are reachable and carry the
    /// declared values.  A regression that renamed one would trip
    /// here before any dispatcher regressions appear.
    #[test]
    fn required_consts_and_type_wire_through() {
        assert_eq!(TestHandler::NAME, "fs_test");
        assert_eq!(TestHandler::ARITY, 2);
    }

    /// `VERIFYING` defaults to `false`.  A regression that flipped
    /// the default (so new handlers silently became verifying)
    /// would trip here.
    #[test]
    fn verifying_defaults_to_false() {
        assert!(!TestHandler::VERIFYING);
    }

    /// `parse_content` success path returns a parsed `Args`.
    #[test]
    fn parse_content_accepts_one_arg() {
        let args = vec![Par::default()];
        let parsed = TestHandler::parse_content(&args)
            .ok()
            .expect("one arg should parse");
        assert_eq!(parsed.value, 42);
    }

    /// `parse_content` failure path returns a boxed err reply with
    /// the handler's reasoning.  Framework will produce this reply
    /// to ack; the Par bytes match `response::err` by construction
    /// (pinned by `handler_reply_err_matches_response_err_bytes`
    /// in reply.rs tests).
    #[test]
    fn parse_content_rejects_wrong_arg_count() {
        let empty: Vec<Par> = vec![];
        let too_many = vec![Par::default(), Par::default()];

        assert!(TestHandler::parse_content(&empty).is_err());
        assert!(TestHandler::parse_content(&too_many).is_err());
    }

    /// `pre_charge_cost` wires through to the declared value.
    #[test]
    fn pre_charge_cost_wires_through() {
        let cost = TestHandler::pre_charge_cost();
        assert_eq!(cost.value, 100);
        assert_eq!(cost.operation, "fs_test");
    }

    /// Default `pre_charge_incremental` returns `None` — constant-
    /// work handlers use `pre_charge_cost` via `reserve_primitive`.
    #[test]
    fn pre_charge_incremental_defaults_to_none() {
        assert!(TestHandler::pre_charge_incremental(&[]).is_none());
    }

    /// Default `post_reply_supplement` returns `None` — single-event
    /// handlers skip the two-event supplement discipline.
    #[test]
    fn post_reply_supplement_defaults_to_none() {
        assert!(TestHandler::post_reply_supplement(&[]).is_none());
    }

    /// An impl overriding `VERIFYING = true` surfaces through the
    /// trait read.  Pin the override path so a regression that
    /// made `VERIFYING` ignore its explicit value (e.g., by
    /// hiding it behind a feature flag) would trip here.
    #[test]
    fn verifying_override_wires_through() {
        assert!(TestIncrementalHandler::VERIFYING);
    }

    /// Override path for `pre_charge_incremental` — a handler that
    /// returns `Some(cost)` surfaces through the trait read.
    /// Length-parameterized handlers (fs_read / fs_write /
    /// fs_entries) rely on this path; pin it at the trait level so
    /// a regression that short-circuited the override (e.g., always
    /// returning `None`) would trip before handler-family slices
    /// catch it.
    #[test]
    fn pre_charge_incremental_override_wires_through() {
        let args = vec![Par::default(), Par::default(), Par::default()];
        let cost =
            TestIncrementalHandler::pre_charge_incremental(&args).expect("override returns Some");
        assert_eq!(cost.value, 13); // 10 + 3 raw args
        assert_eq!(cost.operation, "fs_test_incremental_incr");
    }

    /// Override path for `post_reply_supplement` — a handler that
    /// returns `Some(cost)` surfaces through the trait read.
    /// Two-event handlers (fs_entries, fs_remove_dir) rely on
    /// this path.
    #[test]
    fn post_reply_supplement_override_wires_through() {
        let reply = vec![Par::default(), Par::default()];
        let supp =
            TestIncrementalHandler::post_reply_supplement(&reply).expect("override returns Some");
        assert_eq!(supp.value, 2);
        assert_eq!(supp.operation, "fs_test_incremental_supplement");
    }

    // Compile-time witness that the async method shapes compile for
    // the TestHandler impl.  Not called at runtime (would need a
    // real SyscallCtx) — this is a build-time pin on the trait's
    // method signatures.  Clippy's `let_underscore_future` is
    // allowed because we're only type-checking the return type —
    // the future is dropped intentionally.
    const _ASYNC_METHOD_SHAPES: fn() = || {
        #[allow(dead_code, clippy::let_underscore_future)]
        fn require_dispatch_shape<'a>(ctx: SyscallCtx<'a>, args: TestArgs) {
            // Each line forces the compiler to verify the method's
            // signature matches the trait.  A drift (wrong lifetime,
            // wrong return type) would trip a compile error here.
            let _: Pin<Box<dyn Future<Output = HandlerReply> + Send + 'a>> =
                <TestHandler as FsHandler>::dispatch(ctx, args);
        }

        #[allow(dead_code, clippy::let_underscore_future)]
        fn require_default_hook_shapes<'a>(
            ctx1: SyscallCtx<'a>,
            ctx2: SyscallCtx<'a>,
            ctx3: SyscallCtx<'a>,
            ctx4: SyscallCtx<'a>,
            raw: &'a [Par],
            prev: &'a [Par],
            args: &'a TestArgs,
            fresh: &'a Par,
        ) {
            let _: Pin<Box<dyn Future<Output = Result<(), Box<HandlerReply>>> + Send + 'a>> =
                <TestHandler as FsHandler>::pre_syscall(ctx1, args);
            let _: Pin<Box<dyn Future<Output = ()> + Send + 'a>> =
                <TestHandler as FsHandler>::on_replay_side_effect(ctx2, raw, prev);
            let _: Pin<Box<dyn Future<Output = Option<ConsensusMode>> + Send + 'a>> =
                <TestHandler as FsHandler>::resolve_replay_cmode(ctx3, raw);
            let _: Pin<Box<dyn Future<Output = ()> + Send + 'a>> =
                <TestHandler as FsHandler>::journal(ctx4, raw, JournalPath::Leader {
                    fresh_reply: fresh,
                });
        }
    };
}
