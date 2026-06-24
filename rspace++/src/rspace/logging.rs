use std::collections::BTreeSet;
use std::sync::Arc;

use super::internal::{ConsumeCandidate, WaitingContinuation};
use super::trace::event::{COMM, Consume, Produce};

/// Back-pressure gate for the MeTTaIL reactive single-stepper (OSLF cost-accounting integration).
/// The reducer `pause()`s after each *committed* COMM — at a point that holds no tuplespace lock
/// (after `space.{produce,consume}().await` returns, before the continuation dispatches) — and the
/// stepper `release_one()`s to advance exactly one COMM. It lives in `rspace++` so both the
/// `ISpace`/[`StepCommObserver`] seams here and the reducer (in the `rholang` crate, which depends
/// on `rspace++`) can name it. Built on a `tokio::sync::Semaphore`: 0 permits ⇒ paused; each
/// `release_one` grants one. `abort()` closes the semaphore so a paused reducer's `pause()` returns
/// `Err(StepAborted)` and `inj` unwinds (used when the stepper session is dropped). The pause is a
/// cooperative async yield — it parks the *task*, not the OS thread, and holds no lock.
pub struct StepGate {
    sem: tokio::sync::Semaphore,
}

/// Returned by [`StepGate::pause`] when the gate was aborted (stepper session dropped). The reducer
/// maps this to an interpreter error so `inj` unwinds and the soft-checkpoint reverts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepAborted;

impl StepGate {
    pub fn new() -> Self {
        StepGate { sem: tokio::sync::Semaphore::new(0) }
    }

    /// Reducer side: pause until the stepper releases one step. Each successful return consumes
    /// exactly one permit (so one `release_one` ⇒ one COMM advance). `Err(StepAborted)` once the
    /// gate has been [`abort`](StepGate::abort)ed.
    pub async fn pause(&self) -> Result<(), StepAborted> {
        match self.sem.acquire().await {
            Ok(permit) => {
                permit.forget();
                Ok(())
            }
            Err(_) => Err(StepAborted),
        }
    }

    /// Stepper side: advance the reducer by exactly one COMM.
    pub fn release_one(&self) {
        self.sem.add_permits(1);
    }

    /// Stepper side: abort the session — a currently-paused (or next) `pause()` returns
    /// `Err(StepAborted)`.
    pub fn abort(&self) {
        self.sem.close();
    }
}

impl Default for StepGate {
    fn default() -> Self {
        Self::new()
    }
}

/// Core logging operations that can be overridden by different RSpace loggers
pub trait RSpaceLogger<C, P: Clone, A: Clone, K: Clone>: Send + Sync {
    fn log_comm(
        &self,
        _data_candidates: &Vec<ConsumeCandidate<C, A>>,
        _channels: &Vec<C>,
        _wk: WaitingContinuation<P, K>,
        comm: COMM,
        _label: &str,
    ) -> COMM {
        comm
    }

    fn log_consume(
        &self,
        consume_ref: Consume,
        _channels: &Vec<C>,
        _patterns: &Vec<P>,
        _continuation: &K,
        _persist: bool,
        _peeks: &BTreeSet<i32>,
    ) -> Consume {
        consume_ref
    }

    fn log_produce(
        &self,
        produce_ref: Produce,
        _channel: &C,
        _data: &A,
        _persist: bool,
    ) -> Produce {
        produce_ref
    }
}

/// The kind of a single *non-COMM* Rho-machine reduction, for the live per-reduction stepper. The
/// COMM rendezvous keeps its rich [`StepCommObserver::observe_comm`] event; every *other* structural
/// reduction the reducer performs — dereference `*N`, method call, and a `match`/`if`/`new`/`bundle`
/// body — is reported through [`StepCommObserver::observe_reduction`] with one of these tags. (A
/// resting output is the *residual* of a reduction, not a reduction itself — it is the result of the
/// last reduction, not a separate step — so it is intentionally not a kind here; nor is the post-COMM
/// continuation body, whose own constituent reductions are observed one level down.) Off by default
/// with no `#[cfg]`, exactly like the COMM seam (see
/// `docs/theory/cost-accounting-impl/w3-live-single-step-comm-observation-oslf-fold-seam.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReductionKind {
    /// Dereference `*N` — the `EVarBody` reducer arm resolves a bound name to its quoted process.
    Deref,
    /// A `match` case body firing.
    Match,
    /// An `if` branch firing.
    If,
    /// A `new` scope body, after fresh-name allocation.
    New,
    /// A `bundle` body, after unwrapping.
    Bundle,
    /// A method call (`EMethodBody`) re-eval.
    Method,
}

/// Observer for live single-step COMM tracing — the emit seam the MeTTaIL reactive stepper installs
/// on the base [`super::rspace::RSpace`] at runtime (distinct from [`RSpaceLogger`], which
/// `ReplayRSpace` uses for replay determinism). Off by default with no `#[cfg]`: a production
/// `RSpace` simply has no observer installed. It is invoked once per committed COMM from
/// `RSpace::log_comm` with the full rendezvous payload, already extracted to slices so this trait
/// carries no `Clone` bounds on its type parameters — that keeps the generic `RSpace` struct
/// definition bound-free (the struct stores an `Option<Arc<dyn StepCommObserver<..>>>`). The base
/// `RSpace` installs no observer by default (`None`), so production reduction pays only one
/// branch-predicted `is_none` check per COMM — zero allocation, no vtable, no channel. See
/// `docs/theory/cost-accounting-impl/w3-live-single-step-comm-observation-oslf-fold-seam.md`.
pub trait StepCommObserver<C, P, A, K>: Send + Sync {
    /// Invoked once per committed COMM. `channels` are the rendezvous channels, `consumed` the
    /// data matched by the rendezvous, `patterns`/`continuation` the firing waiting-continuation,
    /// `comm` the trace event, and `label` is either `"comm.consume"` or `"comm.produce"`.
    /// Implementations MUST be lock-free / non-blocking (e.g. a `crossbeam_queue::ArrayQueue`
    /// push) — the call site may hold a per-channel tuplespace lock, so blocking here would stall
    /// the reducer. The async back-pressure pause lives elsewhere (the reducer step-gate), not in
    /// this seam.
    fn observe_comm(
        &self,
        channels: &[C],
        consumed: &[A],
        patterns: &[P],
        continuation: &K,
        comm: &COMM,
        label: &str,
    );

    /// The back-pressure gate the reducer should pause on after each committed COMM, if this
    /// observer drives a live single-step session. Default `None` (a pure capture observer that
    /// never pauses the reducer). The reactive stepper returns `Some(gate)`; the same gate the
    /// stepper `release_one`s. Fetched by `RSpace`'s `ISpace::step_gate` forwarder so the reducer
    /// can reach it without any construction-path threading.
    fn step_gate(&self) -> Option<Arc<StepGate>> {
        None
    }

    /// Invoked once per *non-COMM* structural reduction the reducer is about to perform — the
    /// dereference `*N`, a method call, a `match`/`if`/`new`/`bundle` body, or a `produce` that
    /// reaches quiescence (`OutputAtQuiescence`). `redex` is the process (`C` = `Par` at the reducer's
    /// instantiation) about to be evaluated (or the resting send). The observer self-numbers the
    /// step ordinal (a single monotone counter shared with `observe_comm`), so no ordinal is passed.
    /// Default no-op: a production `RSpace` installs no observer, so the reducer's forwarder is one
    /// branch-predicted `is_none` check — zero allocation, no vtable. Implementations MUST be
    /// lock-free / non-blocking, same contract as `observe_comm` (the call sites run inside the
    /// reducer task; the COMM emit site may hold a tuplespace lock). The async back-pressure pause
    /// lives in the reducer (the same [`StepGate`] returned by [`Self::step_gate`]), not here.
    fn observe_reduction(&self, _redex: &C, _kind: ReductionKind) {}
}

/// Default logger that mirrors current ReplayRSpace behavior (no-op
/// passthrough)
pub struct BasicLogger;

impl BasicLogger {
    pub fn new() -> Self { BasicLogger }
}

impl<C, P: Clone, A: Clone, K: Clone> RSpaceLogger<C, P, A, K> for BasicLogger {}
