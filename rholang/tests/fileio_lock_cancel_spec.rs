//! Slice X-6g / D-06 (2026-09-13, branch-review-2026-09-11.md
//! Track D): `#[ignore]`-guarded placeholder for the wait:true
//! lock-acquisition cancel path integration test.
//!
//! # What this pins
//!
//! `FSERR_CANCELLED` fires when a `wait:true` lock acquire that
//! parked in `LockRegistry::state.waiters` is cancelled — either
//! via `LockRegistry::cancel_all_waiters_for_deploy` at deploy end
//! or via a future explicit cancel-token API.  The Rust-side
//! lock-registry unit tests in `lock.rs::tests` cover the direct
//! cancel path (`try_acquire_range_wait` returning
//! `AcquireOutcome::Waiting(rx)` + explicit
//! `cancel_all_waiters_for_deploy` producing `Err(Cancelled)`).
//!
//! What this file will eventually pin (once Phase 9 lands):
//! **the end-to-end cancel path through the Rholang runtime**.
//! The Rholang caller invokes `fs_lock_range!(..., wait: true, ...)`
//! against a locked-out range, the acquire parks, an out-of-band
//! signal cancels it (via a future runtime cancel-token
//! mechanism), and the reply Par on the ack channel carries
//! `[false, "FSERR_CANCELLED", ...]`.
//!
//! # Why this is a placeholder
//!
//! The Rholang runtime today has no cancel-token mechanism
//! exposed to test harnesses:
//!
//! - The `LockRegistry` internals expose
//!   `cancel_all_waiters_for_deploy(&scope)` but the only caller
//!   is `WalDeployScope::drop`, which fires at deploy END — no way
//!   to trigger mid-flight.
//! - `RhoRuntimeImpl::evaluate(&term, ...)` returns a
//!   `Result<EvaluateResult, InterpreterError>`; there's no
//!   `evaluate_cancellable` variant that returns a handle a test
//!   could cancel.
//! - Phase 9's plan calls for a cancel-token surface tied to the
//!   deploy scope, so a test harness could call
//!   `runtime.cancel_deploy(scope)` mid-evaluation.
//!
//! Until Phase 9 lands, this file is `#[ignore]`-guarded so `cargo
//! test` skips it by default.  Reviewers running `cargo test
//! --ignored` see the placeholder + this docstring.
//!
//! # Plan reference
//!
//! `plan-consensus-reexecute-verify.md` § Phase 9 (deferred).  When
//! that slice lands, this file should: (1) remove the `#[ignore]`
//! attribute, (2) import the new cancel-token API, (3) fill the
//! test body with a concrete harness that acquires a WRITE range
//! on fd A / range [0..100], spawns a task that invokes
//! `fs_lock_range!(fd A, [0..100], WRITE, wait: true, ...)` (which
//! will park), awaits until the wait is parked (poll
//! `LockRegistry::held_locks` + waiters_count), cancels via the
//! new API, and asserts the parked acquire's ack channel receives
//! `[false, "FSERR_CANCELLED", ...]`.

#[cfg(test)]
mod tests {

    /// D-06 placeholder — see file-level docstring.  Trips on
    /// `cargo test --ignored` so a reviewer picking up Phase 9 sees
    /// exactly what needs writing.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "D-06: Phase 9 cancel-token mechanism not yet plumbed; \
                see file-level docstring for the Rholang-runtime API \
                surface this test needs before it can be written."]
    async fn d06_wait_true_cancel_returns_fserr_cancelled_on_ack() {
        // Placeholder body.  Deliberately fails if run — the
        // `#[ignore]` attribute skips it by default; a reviewer
        // that lifts the ignore without writing the harness sees
        // this panic.
        panic!(
            "D-06 placeholder: the Rholang-runtime cancel-token \
             mechanism required for this end-to-end test has not \
             yet landed (Phase 9 deferral per Track D).  See the \
             file-level docstring in \
             `rholang/tests/fileio_lock_cancel_spec.rs` for the \
             harness this test needs before it can be lifted from \
             the `#[ignore]` gate."
        );
    }
}
