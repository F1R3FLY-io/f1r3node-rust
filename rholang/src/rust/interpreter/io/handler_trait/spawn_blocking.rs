// Syscall-dispatch wrapper the handler framework (slice 4.9) uses
// to run long-blocking fs syscalls off the tokio reactor.
// Centralizes two invariants:
//
//   1. **Fail-closed on JoinError** — a panic inside the blocking
//      task propagates as `super::super::errors::join_err_abort`
//      (which panics with the `JOIN_ERR_ABORT_PREFIX` banner).  Per
//      DD-FailClosedOnInvariantBreak, a task panic is a programmer
//      error / unrecoverable fs invariant break and MUST surface as
//      a deploy abort, NOT as an `FSERR_IO` reply that would mask
//      the bug.
//
//   2. **Return shape** — `-> Par` lets the handler body just
//      return the reply Par it built from the syscall result; no
//      ok-vs-err wrapping at this layer (handler bodies do that
//      via `response::err` / `response::ok_*` or
//      `consensus_divergence_reply`).
//
// # Wave 4 shim vs. Wave 6 full-fat
//
// On fileio, `spawn_blocking_par` wraps `tokio::task::spawn_blocking`
// through a `deterministic_reduction::park_external_during(...)`
// helper that removes the current reduction participant from the
// session's participant set before awaiting (so the reduction
// driver's `frontier_ready` check doesn't stall).  That
// `deterministic_reduction.rs` module is 1,437 LOC on fileio and
// has not yet been ported to dev — its scope is a Wave 5-level
// infrastructure addition.
//
// Under Wave 4 + 5 (this slice), `spawn_blocking_par` calls
// `tokio::task::spawn_blocking` directly.  Consensus-observable
// behavior is unaffected (cost accounting is NoopMetering; the
// reduction driver's parking isn't a wire-format concern).  When
// `deterministic_reduction::park_external_during` lands, this
// function will wrap its `spawn_blocking` call through that
// helper — a one-line change at a single call site, which is why
// the wrapper exists in the first place.
//
// The fail-closed JoinError discipline + return shape are already
// in place now and WILL stay stable across the deterministic-
// reduction integration — the pins in this module hold at Wave 4,
// Wave 5, and Wave 6.

use models::rhoapi::Par;
use tokio::task::spawn_blocking;

use crate::rust::interpreter::io::errors::join_err_abort;

/// Run a closure that produces a `Par` on tokio's blocking-task
/// pool.  Panics via [`join_err_abort`] on JoinError (fail-closed
/// per DD-FailClosedOnInvariantBreak).
///
/// # Panic semantics
///
/// Any panic inside `f` surfaces as a `JoinError::Panic`, which this
/// wrapper converts to a panic via [`join_err_abort`].  The panic
/// message starts with
/// [`crate::rust::interpreter::io::errors::JOIN_ERR_ABORT_PREFIX`]
/// so operational log scanning can identify the hazard class.  Do
/// NOT silently swallow the panic as an `FSERR_IO` reply — that
/// would mask a real fs invariant break.
///
/// # Why not just inline `tokio::task::spawn_blocking(f).await`?
///
/// Centralizing the JoinError handling in one wrapper means every
/// handler's `dispatch` body is one line: `spawn_blocking_par(move
/// || { ... }).await`.  No chance of a handler accidentally
/// matching JoinError into an `FSERR_IO` fallback (which would make
/// one specific panic class invisible on alerting).
///
/// # Capture discipline
///
/// The `Send + 'static` bound on `F` forbids borrowing from the
/// enclosing scope.  Handler closures MUST own all captured state:
/// `Arc::clone` from `SyscallCtx` fields before the spawn, and
/// move owned values in.  Standard tokio practice, but worth
/// flagging because the compile error is far from the handler
/// author's intent.
///
/// # Wave 6 (deterministic-reduction integration)
///
/// A Wave 5 slice that ports
/// `deterministic_reduction::park_external_during` will modify this
/// function to wrap the `spawn_blocking(f)` call through that
/// helper — a one-line change at the construction site.  The
/// panics-on-JoinError + Par-return contract stays stable; only
/// the reduction-participant parking semantics change.
pub async fn spawn_blocking_par<F>(f: F) -> Par
where F: FnOnce() -> Par + Send + 'static {
    match spawn_blocking(f).await {
        Ok(par) => par,
        Err(je) => join_err_abort(je),
    }
}

#[cfg(test)]
mod tests {
    use prost::Message;

    use super::*;
    use crate::rust::interpreter::io::errors::JOIN_ERR_ABORT_PREFIX;
    use crate::rust::interpreter::io::response;

    /// LOAD-BEARING (DD-FailClosedOnInvariantBreak):
    /// `spawn_blocking_par` MUST panic (deploy abort) when the
    /// blocking task panics.  A regression that caught the panic
    /// and produced an `FSERR_IO` reply would silently mask real
    /// syscall bugs.
    ///
    /// The panic message carries the canonical
    /// [`JOIN_ERR_ABORT_PREFIX`] so operational log scanning can
    /// grep for this hazard class.  A regression that suppressed
    /// the panic or dropped the prefix would silently mask real
    /// syscall bugs — same failure mode this test guards.
    #[tokio::test]
    async fn spawn_blocking_par_panics_on_join_err() {
        let result = std::panic::AssertUnwindSafe(async {
            spawn_blocking_par(|| -> Par { panic!("simulated task panic") }).await
        });
        let outcome = futures::FutureExt::catch_unwind(result).await;
        let payload = outcome.expect_err(
            "spawn_blocking_par MUST panic on JoinError (deploy abort), not \
             produce an FSERR_IO reply",
        );
        let msg = payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| {
                payload
                    .downcast_ref::<&'static str>()
                    .map(|s| s.to_string())
            })
            .unwrap_or_default();
        assert!(
            msg.starts_with(JOIN_ERR_ABORT_PREFIX),
            "JoinError abort must panic with `JOIN_ERR_ABORT_PREFIX` for \
             log-scan alerting; got: {msg:?}"
        );
        assert!(
            msg.contains("simulated task panic"),
            "the underlying panic payload must be preserved in the abort \
             banner for operator triage; got: {msg:?}"
        );
    }

    /// Happy path: the closure's returned `Par` is forwarded through
    /// the wrapper unchanged.  Pin against a future "sanitize the
    /// reply" refactor that would silently reshape valid returns.
    #[tokio::test]
    async fn spawn_blocking_par_happy_path_forwards_closure_par() {
        let payload = response::ok_u64(42);
        let expected = payload.clone();
        let via_wrapper = spawn_blocking_par(move || payload).await;
        assert_eq!(
            via_wrapper.encode_to_vec(),
            expected.encode_to_vec(),
            "spawn_blocking_par MUST forward the closure's Par unchanged.  \
             A regression that reshaped the reply would break every call site."
        );
    }

    /// `response::err` reply forwarded unchanged — the wrapper
    /// treats error Par values identically to success Par values
    /// at the wrapper layer (ok/err discrimination lives in the
    /// Par bytes, not in the Rust type).
    #[tokio::test]
    async fn spawn_blocking_par_forwards_err_par_unchanged() {
        use crate::rust::interpreter::io::errors::FSERR_IO;
        let payload = response::err(FSERR_IO, "test");
        let expected = payload.clone();
        let via_wrapper = spawn_blocking_par(move || payload).await;
        assert_eq!(via_wrapper.encode_to_vec(), expected.encode_to_vec());
    }

    /// Blocking closures DO run — the test is tautological at the
    /// Rust-type level (otherwise the test would hang on the
    /// non-returning closure).  Keep the pin anyway: a future
    /// refactor that short-circuited the spawn (returned a default
    /// Par without running `f`) would silently break every handler
    /// that uses this wrapper to perform the actual syscall work.
    #[tokio::test]
    async fn spawn_blocking_par_actually_runs_closure() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let ran = Arc::new(AtomicBool::new(false));
        let ran_clone = ran.clone();
        let _ = spawn_blocking_par(move || {
            ran_clone.store(true, Ordering::SeqCst);
            response::ok_bare()
        })
        .await;
        assert!(ran.load(Ordering::SeqCst), "closure must actually run");
    }
}
