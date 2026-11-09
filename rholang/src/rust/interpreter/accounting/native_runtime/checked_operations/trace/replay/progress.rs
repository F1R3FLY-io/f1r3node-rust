//! Added by DR-132 (decision D2-B): the stall rule of native replay.
//!
//! Native replay waits until the recorded predecessors of a live operation
//! complete. Forged evidence can make that wait permanent. The progress word
//! counts the live tasks of one evaluation and the waiters among them. When
//! every live task waits on a recorded operation, replay declares a stall and
//! rejects the block, so every replay ends in a verdict, as dev's replay does.
//!
//! The protocol, checked by TLA+ `NativeReplayStall.tla` and
//! `NativeReplayStallLifecycle.tla` and by Loom `loom_native_replay_stall.rs`:
//! - A task counts as live from before its spawn until its future ends. The
//!   root counts from `begin` until its evaluation returns.
//! - A waiter counts its park under the state lock, in the critical section
//!   of its failed readiness check, once for each state version.
//! - Every change that can make a waiter ready bumps the version and clears
//!   the parked count in its own critical section. It notifies after it
//!   unlocks.
//! - A park or a finish that leaves every live task parked declares the
//!   stall.
//!
//! The Loom crate includes this file, so the atomics come from the parent
//! module: std in production, Loom in the model.

use super::atomic::{AtomicU64, Ordering};
use super::state::ReplayState;

/// Bits 0 to 30 count the parked waiters, and bits 31 to 61 the live tasks.
const FIELD: u64 = (1 << 31) - 1;
const PARKED: u64 = 1;
const LIVE: u64 = 1 << 31;
const STALLED: u64 = 1 << 62;
const CLOSED: u64 = 1 << 63;

fn parked(word: u64) -> u64 { word & FIELD }

fn live(word: u64) -> u64 { (word >> 31) & FIELD }

/// No flag is set, some task is live, and every live task is parked.
fn stalls(word: u64) -> bool {
    word & (STALLED | CLOSED) == 0 && live(word) != 0 && parked(word) == live(word)
}

/// The progress word of one replay.
pub(super) struct ReplayProgress {
    word: AtomicU64,
}

impl ReplayProgress {
    pub(super) fn new() -> Self {
        Self {
            word: AtomicU64::new(0),
        }
    }

    /// Starts an evaluation with the root as its one live task. Only an idle
    /// word can begin, so a stalled or cancelled replay never runs again.
    pub(super) fn begin(&self) -> bool {
        self.word
            .compare_exchange(0, LIVE, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// Counts a task before its spawn, so its parent cannot park first.
    pub(super) fn enter(&self) {
        let old = self.word.fetch_add(LIVE, Ordering::AcqRel);
        assert!(
            live(old) < FIELD,
            "native replay live tasks exceed the progress word"
        );
    }

    /// A live task ended. True when every remaining live task is parked.
    pub(super) fn finish(&self) -> bool {
        let old = self.word.fetch_sub(LIVE, Ordering::AcqRel);
        debug_assert!(live(old) != 0, "native replay finished an uncounted task");
        stalls(old.wrapping_sub(LIVE))
    }

    /// Ends the evaluation by cancellation. While some task is live, the word
    /// closes, so no later finish declares a stall and the replay cannot
    /// begin again. An evaluation that ended normally leaves the word idle.
    pub(super) fn close(&self) {
        let _ = self
            .word
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |word| {
                (live(word) != 0).then_some(word | CLOSED)
            });
    }

    /// Some task is live, and no stall or cancellation ended the evaluation.
    pub(super) fn armed(&self) -> bool {
        let word = self.word.load(Ordering::Acquire);
        word & (STALLED | CLOSED) == 0 && live(word) != 0
    }

    /// No task is live, no waiter is parked and no flag is set.
    #[cfg(test)]
    pub(super) fn idle(&self) -> bool { self.word.load(Ordering::Acquire) == 0 }

    /// The stall flag is set.
    #[cfg(test)]
    pub(super) fn stalled(&self) -> bool { self.word.load(Ordering::Acquire) & STALLED != 0 }

    /// The live tasks and the parked waiters.
    #[cfg(test)]
    pub(super) fn counts(&self) -> (u64, u64) {
        let word = self.word.load(Ordering::Acquire);
        (live(word), parked(word))
    }

    /// Counts one more parked waiter. True when every live task is parked.
    /// The state lock is held.
    fn park(&self) -> bool {
        let old = self.word.fetch_add(PARKED, Ordering::AcqRel);
        debug_assert!(
            parked(old) < live(old),
            "native replay parked more waiters than live tasks"
        );
        stalls(old + PARKED)
    }

    /// One counted waiter leaves at the current version. The state lock is
    /// held.
    fn unpark(&self) {
        let old = self.word.fetch_sub(PARKED, Ordering::AcqRel);
        debug_assert!(
            parked(old) != 0,
            "native replay unparked an uncounted waiter"
        );
    }

    /// A change can make a waiter ready, so every waiter must check and count
    /// again. The state lock is held.
    fn wake_all(&self) { self.word.fetch_and(!FIELD, Ordering::AcqRel); }

    /// Sets the stall flag, unless a stall is already declared or a
    /// cancellation closed the word.
    fn declare_stall(&self) -> bool {
        self.word
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |word| {
                (word & (STALLED | CLOSED) == 0).then_some(word | STALLED)
            })
            .is_ok()
    }
}

/// The version at which a waiter counted itself parked.
#[derive(Debug, Default)]
pub(super) struct Registration {
    parked_at: Option<u64>,
}

impl Registration {
    /// Counts the waiter as parked at `version`, once for each version. True
    /// when the park leaves every live task parked. The state lock is held.
    fn park(&mut self, progress: &ReplayProgress, version: u64) -> bool {
        if self.parked_at == Some(version) {
            return false;
        }
        self.parked_at = Some(version);
        progress.park()
    }

    /// Leaves the registration. Only a registration at the current version
    /// is counted, because each change clears the count. The state lock is
    /// held.
    pub(super) fn leave(&mut self, progress: &ReplayProgress, version: u64) {
        if self.parked_at.take() == Some(version) {
            progress.unpark();
        }
    }

    /// The waiter holds a registration, so its drop must leave it.
    pub(super) fn held(&self) -> bool { self.parked_at.is_some() }
}

/// The outcome of one readiness check in `wait_ready`.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum WaitDecision {
    /// The selected row is ready.
    Ready,
    /// The selected row is not ready, so the waiter waits for a change.
    Park,
    /// This park declared the stall. The caller notifies after it unlocks.
    Stall,
}

/// The decision of `wait_ready`, in the critical section of its readiness
/// check. A counted waiter whose row is not ready counts its park, and the
/// park that leaves every live task parked declares the stall. A ready row or
/// an error leaves the registration. An uncounted waiter never counts. It can
/// wait only while no evaluation is armed (`NativeReplayError::Uncounted`).
pub(super) fn check_and_park<E>(
    state: &mut ReplayState,
    progress: &ReplayProgress,
    registration: &mut Registration,
    ready: Result<bool, E>,
    counted: bool,
) -> Result<WaitDecision, E> {
    match ready {
        Ok(false) if counted => {
            if registration.park(progress, state.version()) && stall(state, progress) {
                registration.parked_at = None;
                Ok(WaitDecision::Stall)
            } else {
                Ok(WaitDecision::Park)
            }
        }
        Ok(false) => Ok(WaitDecision::Park),
        Ok(true) => {
            registration.leave(progress, state.version());
            Ok(WaitDecision::Ready)
        }
        Err(error) => {
            registration.leave(progress, state.version());
            Err(error)
        }
    }
}

/// The end of a change that can make a waiter ready, in the critical section
/// of the change and after the state bumped its version. Every waiter must
/// check and count again. The caller notifies after it unlocks.
pub(super) fn after_change(_state: &mut ReplayState, progress: &ReplayProgress) {
    progress.wake_all();
}

/// Declares the stall: sets the flag, closes the replay state, bumps its
/// version and clears the parked count. False when a stall is already
/// declared or a cancellation closed the word. The state lock is held, and
/// the caller notifies after it unlocks.
pub(super) fn stall(state: &mut ReplayState, progress: &ReplayProgress) -> bool {
    if !progress.declare_stall() {
        return false;
    }
    state.stall();
    progress.wake_all();
    true
}
