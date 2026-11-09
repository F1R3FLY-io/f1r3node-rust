//! Added by DR-132 (decision D2-B): unit tests of the progress word of the
//! stall rule and of its decisions. They live outside `progress.rs`, because
//! the Loom crate includes that file and Loom atomics panic outside a model.

use super::progress::{
    after_change, check_and_park, stall, Registration, ReplayProgress, WaitDecision,
};
use super::state::{LedgerError, ReplayState, SlotState};

fn state(slots: usize) -> ReplayState {
    ReplayState::new(
        vec![SlotState::Available; slots],
        Vec::with_capacity(slots),
        slots as u64,
    )
}

/// Publishes `slot` as one change of the replay.
fn publish(state: &mut ReplayState, progress: &ReplayProgress, slot: usize) {
    let serial = state.reserve(slot).expect("the slot reserves");
    state.publish(slot, serial, 1);
    after_change(state, progress);
}

/// Checks the readiness of `slot` after `predecessor` and decides, as
/// `wait_ready` does in one critical section.
fn check(
    state: &mut ReplayState,
    progress: &ReplayProgress,
    waiter: &mut Registration,
    slot: usize,
    predecessor: usize,
    counted: bool,
) -> Result<WaitDecision, LedgerError> {
    let ready = state.open().and_then(|()| state.ready(slot, [predecessor]));
    check_and_park(state, progress, waiter, ready, counted)
}

#[test]
fn begin_requires_an_idle_word_and_a_normal_end_returns_to_it() {
    let progress = ReplayProgress::new();
    assert!(progress.idle());
    assert!(!progress.armed());
    assert!(progress.begin());
    assert!(!progress.begin(), "a second evaluation cannot begin");
    assert!(progress.armed());
    progress.enter();
    assert_eq!(progress.counts(), (2, 0));
    assert!(!progress.finish());
    assert!(!progress.finish());
    assert!(progress.idle());
    assert!(progress.begin(), "an idle word begins again");
    assert!(!progress.finish());
    assert!(progress.idle());
}

#[test]
fn a_waiter_counts_once_for_each_version() {
    let progress = ReplayProgress::new();
    let mut state = state(2);
    assert!(progress.begin());
    progress.enter();
    let mut waiter = Registration::default();
    assert_eq!(
        check(&mut state, &progress, &mut waiter, 1, 0, true),
        Ok(WaitDecision::Park)
    );
    assert_eq!(progress.counts(), (2, 1));
    assert!(waiter.held());
    assert_eq!(
        check(&mut state, &progress, &mut waiter, 1, 0, true),
        Ok(WaitDecision::Park),
        "a wake without a change checks the same version"
    );
    assert_eq!(progress.counts(), (2, 1), "the second check does not count");
    publish(&mut state, &progress, 0);
    assert_eq!(progress.counts(), (2, 0), "the change clears the count");
    assert_eq!(
        check(&mut state, &progress, &mut waiter, 1, 0, true),
        Ok(WaitDecision::Ready)
    );
    assert!(!waiter.held(), "a ready waiter holds no registration");
    assert_eq!(progress.counts(), (2, 0));
}

#[test]
fn the_last_park_declares_the_stall() {
    let progress = ReplayProgress::new();
    let mut state = state(3);
    assert!(progress.begin());
    progress.enter();
    let (mut first, mut second) = (Registration::default(), Registration::default());
    assert_eq!(
        check(&mut state, &progress, &mut first, 1, 0, true),
        Ok(WaitDecision::Park)
    );
    assert_eq!(
        check(&mut state, &progress, &mut second, 2, 0, true),
        Ok(WaitDecision::Stall),
        "both live tasks wait on slot 0, which no task publishes"
    );
    assert!(progress.stalled());
    assert!(!second.held());
    assert_eq!(state.open(), Err(LedgerError::Stalled));
    assert_eq!(
        check(&mut state, &progress, &mut first, 1, 0, true),
        Err(LedgerError::Stalled),
        "the other waiter wakes and meets the stall"
    );
    assert!(!first.held());
    assert_eq!(progress.counts(), (2, 0));
    assert!(!progress.finish(), "a stall is declared once");
    assert!(!progress.finish());
    assert!(!progress.begin(), "a stalled replay never begins again");
}

#[test]
fn the_finish_that_leaves_every_task_parked_declares_the_stall() {
    let progress = ReplayProgress::new();
    let mut state = state(2);
    assert!(progress.begin());
    progress.enter();
    let mut waiter = Registration::default();
    assert_eq!(
        check(&mut state, &progress, &mut waiter, 1, 0, true),
        Ok(WaitDecision::Park)
    );
    assert!(
        progress.finish(),
        "the root ends, and its waiter is the last live task"
    );
    assert!(stall(&mut state, &progress));
    assert!(!stall(&mut state, &progress), "a stall is declared once");
    assert_eq!(state.open(), Err(LedgerError::Stalled));
    assert_eq!(progress.counts(), (1, 0));
}

#[test]
fn an_uncounted_waiter_never_counts() {
    let progress = ReplayProgress::new();
    let mut state = state(2);
    let mut waiter = Registration::default();
    assert_eq!(
        check(&mut state, &progress, &mut waiter, 1, 0, false),
        Ok(WaitDecision::Park)
    );
    assert!(!waiter.held());
    assert!(progress.idle());
}

#[test]
fn a_cancelled_evaluation_declares_no_stall_and_never_begins_again() {
    let progress = ReplayProgress::new();
    let mut state = state(2);
    assert!(progress.begin());
    progress.enter();
    let mut waiter = Registration::default();
    assert_eq!(
        check(&mut state, &progress, &mut waiter, 1, 0, true),
        Ok(WaitDecision::Park)
    );
    progress.close();
    assert!(!progress.armed());
    assert!(
        !progress.finish(),
        "the aborted root leaves every live task parked, but the word is closed"
    );
    assert!(
        !stall(&mut state, &progress),
        "a closed word declares no stall"
    );
    waiter.leave(&progress, state.version());
    assert!(!progress.finish());
    assert_eq!(progress.counts(), (0, 0));
    assert!(!progress.idle(), "the closed flag stays");
    assert!(!progress.begin());
    assert_eq!(state.open(), Ok(()), "a cancel does not stall the replay");
}

#[test]
fn closing_an_ended_evaluation_keeps_the_word_idle() {
    let progress = ReplayProgress::new();
    assert!(progress.begin());
    assert!(!progress.finish());
    progress.close();
    assert!(progress.idle());
    assert!(progress.begin());
    assert!(!progress.finish());
}

#[test]
fn a_stale_leave_changes_no_count() {
    let progress = ReplayProgress::new();
    let mut state = state(3);
    assert!(progress.begin());
    progress.enter();
    progress.enter();
    let mut stale = Registration::default();
    assert_eq!(
        check(&mut state, &progress, &mut stale, 2, 0, true),
        Ok(WaitDecision::Park)
    );
    assert_eq!(progress.counts(), (3, 1));
    publish(&mut state, &progress, 1);
    assert_eq!(progress.counts(), (3, 0));
    stale.leave(&progress, state.version());
    assert_eq!(progress.counts(), (3, 0), "the stale leave does not unpark");
    assert!(!stale.held());
}
