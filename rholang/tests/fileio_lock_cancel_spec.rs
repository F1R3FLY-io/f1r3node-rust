//! Slice X-6g / D-06 (2026-09-13, branch-review-2026-09-11.md
//! Track D): `#[ignore]`-guarded placeholder for the wait:true
//! lock-acquisition cancel path integration test.
//!
//! # What this pins
//!
//! `FSERR_CANCELLED` fires when a `wait:true` lock acquire that
//! parked in `LockRegistry::state.waiters` is cancelled — either
//! via `LockRegistry::cancel_wait(lock_id)`,
//! `cancel_all_waiters_for_deploy(&scope)`, or
//! `cancel_all_waiters_for_holder(&holder)`.  The Rust-side
//! lock-registry unit tests in `lock.rs::tests` cover the direct
//! cancel path (`try_acquire_range_wait` returning
//! `AcquireOutcome::Waiting(rx)` + explicit cancel producing
//! `Err(Cancelled)`).
//!
//! What this file will eventually pin: **the end-to-end cancel
//! path through the Rholang runtime.**  The Rholang caller
//! invokes `fs_lock_range!(..., wait: true, ...)` against a
//! locked-out range, the acquire parks, an out-of-band call to
//! one of the cancel APIs above triggers `Err(Cancelled)` on the
//! parked oneshot, and the reply Par on the ack channel carries
//! `[false, "FSERR_CANCELLED", ...]`.
//!
//! # Why this is a placeholder (integration-test complexity, not
//!   a missing runtime API)
//!
//! An earlier iteration of this file claimed the runtime lacked a
//! cancel-token mechanism.  That was wrong — `LockRegistry` has
//! `pub fn cancel_wait(lock_id)` +
//! `cancel_all_waiters_for_deploy(&scope)` +
//! `cancel_all_waiters_for_holder(&holder)` all reachable via
//! `runtime.fs_handles.lock_registry`.  What's actually missing
//! is a concurrent-test harness that orchestrates:
//!
//! - Spawn `runtime.evaluate(...)` for a term that takes a WRITE
//!   range on fd A / [0..100] — this holds the blocking acquire.
//! - Spawn a SECOND `runtime.evaluate(...)` for a term that
//!   invokes `fs_lock_range!(fd A, [0..100], WRITE, wait: true,
//!   ...)`.  This second evaluate parks inside the wait:true
//!   admit await.
//! - Poll until the park has actually landed (currently no clean
//!   API; would need `LockRegistry::waiters_count(&scope)` or
//!   similar, OR a poll on some internal state — a modest new
//!   introspection helper).
//! - Call `runtime.fs_handles.lock_registry.cancel_all_waiters_for_deploy(&scope)`
//!   from the test task while the second evaluate is still
//!   parked.
//! - Await the second evaluate's JoinHandle.  Inspect the ack
//!   channel via `runtime.get_hot_changes()` and assert
//!   `[false, "FSERR_CANCELLED", ...]` on the ack.
//!
//! The tricky bits are the "wait until parked" poll (may need a
//! `LockRegistry::waiters_count(&scope)` helper if the current
//! surface doesn't expose it) and the ack-channel extraction
//! (existing helper pattern from `fileio_g01_park_external_spec.rs`
//! is a good reference).
//!
//! # Plan reference
//!
//! Not blocked on Phase 9 (Cost Accounting, delivered 2026-08-23)
//! or any deferred runtime slice.  This is an integration-test
//! that can land whenever a session has bandwidth to write the
//! orchestration.  The `#[ignore]` gate is discipline: if a
//! reviewer picks it up, they land the harness in one shot rather
//! than a half-broken pin.

#[cfg(test)]
mod tests {

    /// D-06 placeholder — see file-level docstring.  Trips on
    /// `cargo test --ignored` so a reviewer picking it up sees the
    /// harness sketch they need to write.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "D-06: integration-test harness not yet written — \
                see file-level docstring for the orchestration \
                sketch.  Cancel APIs exist on LockRegistry; \
                what's needed is the two-evaluate concurrent \
                harness + ack-channel inspection."]
    async fn d06_wait_true_cancel_returns_fserr_cancelled_on_ack() {
        panic!(
            "D-06 placeholder: the two-evaluate concurrent harness \
             this test needs has not been written yet.  See the \
             file-level docstring in \
             `rholang/tests/fileio_lock_cancel_spec.rs` for the \
             harness sketch (spawn WRITE-holder + wait:true parker + \
             out-of-band cancel + ack-channel FSERR_CANCELLED \
             assertion)."
        );
    }
}
