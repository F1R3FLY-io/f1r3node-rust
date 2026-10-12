// The generic framework loop that every migrated fs_* handler
// dispatches through.  Runs the 7-step dispatch order documented
// in [`FsHandler`](super::fs_handler::FsHandler)'s module header:
//
//   Step 1: `is_contract_call.unapply` — framework shape check.
//     Rejects with `illegal_argument_error(H::NAME)`.
//   Step 2: Arity check (`args.len() == H::ARITY`).
//   Step 3: Pre-charge via `metering.reserve_primitive` /
//     `reserve_incremental_primitive`.
//   Step 4: `is_replay` short-circuit — verifying + Consensus
//     cmode falls through to re-execute + verify; everything else
//     echoes `previous` after running `on_replay_side_effect` +
//     optional `journal(OracularEcho)` + `post_reply_supplement`.
//   Step 5: `parse_content` — content-level mismatch replies via
//     `HandlerReply::err`.
//   Step 5b: `pre_syscall` — pre-append WAL entry.  Early-exits
//     on WAL-cap exhaustion.
//   Step 6: `dispatch` — the handler's syscall body.
//   Step 7: Verify (Consensus follower only) → `journal` →
//     `post_reply_supplement` → produce to ack.
//
// # X-3 / SEC-3 ordering discipline (consensus-observable)
//
// The cost pre-charge (Step 3) runs BEFORE the WAL-cap check
// (`pre_syscall`, Step 5b) AND before content parse (Step 5).
// When the WAL is full OR content parse fails, the pre-charged
// cost is CONSUMED even though the handler returns an error
// and appends no WAL entry.  This is a UX / audit-trail wart,
// NOT a consensus concern: the pre-charge is a cost reservation
// that both leader and follower emit identically, so no
// divergence, no double-charge, no missing entry.
//
// The ordering is INTENTIONAL (per the fileio won't-fix
// discipline).  Never introduce a "charge-then-maybe-refund"
// pattern; refunds under Wave 6 would emit new billable events
// that shift the cost-witness fold bytes — a hard-fork surface.
//
// # Owned adapter (`dispatch_via_trait_owned`)
//
// The `FsHandlerEntry.dispatch` fn-pointer in
// [`FS_HANDLERS`](super::fs_handlers::FS_HANDLERS) has signature
// `fn(FsProcesses, ...)`, not `fn(&FsProcesses, ...)` — fn
// pointers can't carry lifetimes.  The owned adapter wraps
// `dispatch_via_trait<H>` so each handler's registration site
// just coerces a non-capturing closure.
//
// # Budget-exhaustion ack-orphan semantics (Wave 6)
//
// If `post_reply_supplement`'s `reserve_incremental_primitive(supp)?`
// fails mid-dispatch (Wave 6 budget exhaustion after Step 6), the
// function returns `Err` BEFORE calling `produce` — the ack channel
// is left orphaned.  This is the correct behavior for a failed
// deploy: the reduction driver tears down the deploy's scope, and
// the orphan ack goes away with it.  Under NoopMetering (Wave 4 + 5)
// this never triggers because reserves are no-ops.  Under Wave 6
// the orphan-ack path becomes reachable and is the intended
// failure mode — a caller waiting on the ack sees deploy failure,
// not a stuck process.

use std::future::Future;
use std::pin::Pin;

use models::rhoapi::{ListParWithRandom, Par};

use crate::rust::interpreter::errors::{illegal_argument_error, InterpreterError};
use crate::rust::interpreter::io::handler_trait::consensus_divergence::consensus_divergence_reply;
use crate::rust::interpreter::io::handler_trait::fs_handler::FsHandler;
use crate::rust::interpreter::io::handler_trait::fs_processes::FsProcesses;
use crate::rust::interpreter::io::handler_trait::journal_path::JournalPath;
use crate::rust::interpreter::io::handler_trait::syscall_ctx::SyscallCtx;
use crate::rust::interpreter::io::{verify, ConsensusMode};

/// Owned-`FsProcesses` adapter around [`dispatch_via_trait`].
/// Used as the fn-pointer body in every
/// `#[distributed_slice(FS_HANDLERS)] static FS_X_ENTRY`
/// registration.
/// [`FsHandlerEntry.dispatch`](super::fs_handlers::FsHandlerEntry)
/// is a plain `fn(FsProcesses, ...) -> Pin<Box<...>>` — no
/// lifetimes on the pointer type — so the closure body owns `fs`
/// for the future's lifetime and borrows into it for the trait
/// dispatch call.
///
/// Non-capturing generic fn (rather than a closure captured in
/// each registration site) so the pointer coercion at the static's
/// init expression is uniform across all handler entries.
pub async fn dispatch_via_trait_owned<H: FsHandler>(
    fs: FsProcesses,
    contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
) -> Result<Vec<Par>, InterpreterError> {
    dispatch_via_trait::<H>(&fs, contract_args).await
}

/// The generic framework loop.  Called by every migrated handler's
/// [`FsHandlerEntry.dispatch`](super::fs_handlers::FsHandlerEntry)
/// fn-pointer entry (via [`dispatch_via_trait_owned`]).  See the
/// module docstring for the step-by-step dispatch order.
pub async fn dispatch_via_trait<H: FsHandler>(
    fs: &FsProcesses,
    contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
) -> Result<Vec<Par>, InterpreterError> {
    // Step 1: unapply.
    let Some((produce, is_replay, previous, args)) = fs.is_contract_call().unapply(contract_args)
    else {
        return Err(illegal_argument_error(H::NAME));
    };

    // Step 2: arity check.  Framework rejects with
    // `illegal_argument_error`.  Ack is the last Par.
    //
    // `H::ARITY >= 1` is a construction-time invariant — every
    // fs_* contract has at least an ack channel.  Debug-only
    // guard (release skips) so a hypothetical `const ARITY: usize
    // = 0` handler trips loudly in test / debug before the
    // `args[H::ARITY - 1]` indexing below panics on usize
    // underflow.
    debug_assert!(H::ARITY >= 1, "FsHandler::ARITY must be >= 1 (ack channel)");
    if args.len() != H::ARITY {
        return Err(illegal_argument_error(H::NAME));
    }

    // Ack is the last Par.  Cloned because `produce` takes `&Par`;
    // the clone cost is a Par-header + Arc bump, not a deep copy.
    let ack = args[H::ARITY - 1].clone();

    let raw_pre_ack: &[Par] = &args[..H::ARITY - 1];

    // Step 3: cost pre-charge — post-unapply, post-arity.  See
    // module docstring (§ X-3 / SEC-3) for the ordering discipline.
    match H::pre_charge_incremental(raw_pre_ack) {
        Some(cost) => fs.metering.reserve_incremental_primitive(cost)?,
        None => fs.metering.reserve_primitive(H::pre_charge_cost())?,
    }

    // Step 4: is_replay short-circuit.
    if is_replay {
        // Verifying-handler cmode dispatch.  Framework resolves
        // the handler's effective cmode from raw args + ctx.  Under
        // Oracular / unresolved cmode we take the tautological echo
        // path; under Consensus we fall through to re-execute + verify.
        let verifying_consensus_replay = if H::VERIFYING {
            let replay_cmode =
                H::resolve_replay_cmode(SyscallCtx::from_fs_processes(fs, &ack), raw_pre_ack).await;
            replay_cmode == Some(ConsensusMode::Consensus)
        } else {
            false
        };

        if !verifying_consensus_replay {
            // Non-verifying replay OR verifying Oracular/unresolved
            // replay.  Run the optional side-effect hook, then
            // tautologically echo `previous`.
            H::on_replay_side_effect(
                SyscallCtx::from_fs_processes(fs, &ack),
                raw_pre_ack,
                &previous,
            )
            .await;
            // For verifying handlers on the Oracular echo path,
            // journal `previous.first()` — matches pre-refactor
            // structural parity.  `journal` self-guards on cmode
            // (handler-specific).
            if H::VERIFYING {
                if let Some(previous_reply) = previous.first() {
                    H::journal(
                        SyscallCtx::from_fs_processes(fs, &ack),
                        raw_pre_ack,
                        JournalPath::OracularEcho { previous_reply },
                    )
                    .await;
                }
            }
            // Post-reply supplement based on `previous`'s shape
            // (length-parameterized non-verifying replays like
            // `fs_entries_stream_next` use this).
            if let Some(supp) = H::post_reply_supplement(&previous) {
                fs.metering.reserve_incremental_primitive(supp)?;
            }
            produce(&previous, &ack).await?;
            return Ok(previous);
        }
        // Verifying + Consensus follower — fall through to
        // re-execute + verify path.
    }

    // Step 5: content parse.  Content-level mismatches produce a
    // normal reply, not an `illegal_argument_error`.
    let parsed = match H::parse_content(raw_pre_ack) {
        Ok(a) => a,
        Err(boxed_reply) => {
            let out = vec![(*boxed_reply).into_par()];
            produce(&out, &ack).await?;
            return Ok(out);
        }
    };

    // Step 5b: pre-syscall hook.  Path-mutation handlers pre-append
    // a WAL entry with a Success placeholder before the syscall
    // runs; on WAL-cap exhaustion the handler returns
    // Err(boxed_reply) and the framework produces the reply +
    // early-returns.  Default: no-op for non-journaling handlers.
    if let Err(early_reply) = H::pre_syscall(SyscallCtx::from_fs_processes(fs, &ack), &parsed).await
    {
        let out = vec![(*early_reply).into_par()];
        produce(&out, &ack).await?;
        return Ok(out);
    }

    // Step 6: dispatch — runs on the leader path AND on the
    // Consensus-follower verifying-replay path (the point of
    // "re-execute + verify").
    let fresh_reply = H::dispatch(SyscallCtx::from_fs_processes(fs, &ack), parsed).await;
    let fresh_par = fresh_reply.into_par();

    // Step 7: verify (Consensus follower only) + journal + produce.
    if is_replay {
        // Consensus follower verify — compare fresh reply's stable
        // hash against the leader's cached reply hash in `previous`.
        match verify::verify_reply_hash_matches_cached(&fresh_par, &previous) {
            Ok(()) => {
                H::journal(
                    SyscallCtx::from_fs_processes(fs, &ack),
                    raw_pre_ack,
                    JournalPath::VerifySuccess {
                        fresh_reply: &fresh_par,
                    },
                )
                .await;
                let out = vec![fresh_par];
                if let Some(supp) = H::post_reply_supplement(&out) {
                    fs.metering.reserve_incremental_primitive(supp)?;
                }
                produce(&out, &ack).await?;
                Ok(out)
            }
            Err(reason) => {
                let divergence = consensus_divergence_reply(H::NAME, reason);
                H::journal(
                    SyscallCtx::from_fs_processes(fs, &ack),
                    raw_pre_ack,
                    JournalPath::VerifyDivergence {
                        fresh_reply: &fresh_par,
                        divergence_reply: &divergence,
                    },
                )
                .await;
                // Cost supplement billed against the FRESH syscall
                // reply (state_source), not the divergence reply
                // produced to ack.  Pre-refactor fs_entries
                // computed n_entries from the fresh reply BEFORE
                // the verify branch and charged that same n
                // regardless of verify Ok/Err.  Divergence-path
                // charging against `divergence` (list_len=0 for
                // err payload) would silently drop cost witness
                // bytes vs. the pre-refactor shape.
                if let Some(supp) = H::post_reply_supplement(std::slice::from_ref(&fresh_par)) {
                    fs.metering.reserve_incremental_primitive(supp)?;
                }
                let out = vec![divergence];
                produce(&out, &ack).await?;
                Ok(out)
            }
        }
    } else {
        // Leader path — journal fresh_par, produce it.
        H::journal(
            SyscallCtx::from_fs_processes(fs, &ack),
            raw_pre_ack,
            JournalPath::Leader {
                fresh_reply: &fresh_par,
            },
        )
        .await;
        let out = vec![fresh_par];
        if let Some(supp) = H::post_reply_supplement(&out) {
            fs.metering.reserve_incremental_primitive(supp)?;
        }
        produce(&out, &ack).await?;
        Ok(out)
    }
}

// Compile-time witnesses pinning both entry-point signatures.
// A regression that drifted the fn signatures (wrong lifetimes,
// wrong return type, wrong contract-args tuple shape) trips a
// build error here before per-family handler slices (4.12+) start
// failing one at a time.  Both witnesses sit in production code
// (`const fn()`), not test-only, so `cargo build` catches drift
// regardless of test selection.
const _DISPATCH_VIA_TRAIT_SIG: fn() = || {
    #[allow(dead_code)]
    fn assert_sig<H: FsHandler + 'static>(
        fs: &FsProcesses,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Pin<Box<dyn Future<Output = Result<Vec<Par>, InterpreterError>> + Send + '_>> {
        Box::pin(dispatch_via_trait::<H>(fs, contract_args))
    }
};

const _DISPATCH_VIA_TRAIT_OWNED_SIG: fn() = || {
    #[allow(dead_code)]
    fn assert_sig<H: FsHandler + 'static>(
        fs: FsProcesses,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Pin<Box<dyn Future<Output = Result<Vec<Par>, InterpreterError>> + Send>> {
        Box::pin(dispatch_via_trait_owned::<H>(fs, contract_args))
    }
};

#[cfg(test)]
mod tests {
    // Runtime testing of `dispatch_via_trait<H>` requires a full
    // `FsProcesses` fixture (RhoDispatch + RhoISpace), which pulls
    // in the whole async rspace init path.  Too much setup for a
    // unit test on the dispatcher's 7-step pipeline.
    //
    // The compile-time witnesses above pin the entry-point
    // signatures; end-to-end dispatch behavior is tested when per-
    // family handlers land (slices 4.12+) and the dispatcher is
    // exercised through real handler flows.
    //
    // The individual steps of the 7-step pipeline are already
    // pinned at their underlying sites:
    //
    //   Step 1 (unapply):             contract_call::ContractCall
    //                                 (existing dev tests).
    //   Step 2 (arity check):         `illegal_argument_error`
    //                                 (errors.rs).
    //   Step 3 (pre-charge):          Metering trait tests in
    //                                 accounting/noop.rs.
    //   Step 4 (is_replay):           JournalPath + FsHandler
    //                                 default-hook tests
    //                                 (slices 4.4 + 4.6).
    //   Step 5 (parse_content):       FsHandler trait tests
    //                                 (slice 4.6).
    //   Step 5b (pre_syscall):        FsHandler compile-time
    //                                 witness (slice 4.6).
    //   Step 6 (dispatch):            FsHandler compile-time
    //                                 witness (slice 4.6).
    //   Step 7 (verify + journal):    verify.rs tests + the
    //                                 `JournalPath` enum
    //                                 behavior pins (slice 4.4) +
    //                                 `consensus_divergence_reply`
    //                                 tests (slice 4.7).
    //
    // Each step's isolated invariant is pinned at its own test
    // module; the dispatcher glue is pinned at the compile-time
    // witness.  Full-pipeline integration tests land with the
    // handler-family slices.

    use super::*;

    /// `dispatch_via_trait` compile-time sig witness lives in
    /// production code; re-assert at test time via a trivial
    /// instantiation so a regression lights up `cargo test` as well
    /// as `cargo build`.  Signature: generic `<H: FsHandler>`,
    /// takes `&FsProcesses` + the raw contract_args tuple, returns
    /// `Result<Vec<Par>, InterpreterError>`.
    #[test]
    fn dispatch_via_trait_has_expected_sig_at_test_time() {
        #[allow(dead_code, clippy::let_underscore_future)]
        fn require_sig<H: FsHandler + 'static>(
            fs: &FsProcesses,
            contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
        ) {
            // Not awaited — the future's existence is the witness.
            let _future: Pin<Box<dyn Future<Output = Result<Vec<Par>, InterpreterError>> + Send>> =
                Box::pin(dispatch_via_trait::<H>(fs, contract_args));
        }
    }

    /// `dispatch_via_trait_owned` compile-time sig witness.
    /// Signature: owned `FsProcesses` (not reference) — matches
    /// the `fn(FsProcesses, ...)` shape of
    /// `FsHandlerEntry.dispatch`.
    #[test]
    fn dispatch_via_trait_owned_has_expected_sig_at_test_time() {
        #[allow(dead_code, clippy::let_underscore_future)]
        fn require_sig<H: FsHandler + 'static>(
            fs: FsProcesses,
            contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
        ) {
            let _future: Pin<Box<dyn Future<Output = Result<Vec<Par>, InterpreterError>> + Send>> =
                Box::pin(dispatch_via_trait_owned::<H>(fs, contract_args));
        }
    }
}
