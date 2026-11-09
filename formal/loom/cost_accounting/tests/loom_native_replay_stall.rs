//! Added by DR-132 (decision D2-B): the stall rule of native replay under
//! Loom. The model includes the production `state.rs` and `progress.rs`, so
//! the readiness checks, the version rule, the live and parked counts and the
//! stall declaration are the production code. The notification is a list of
//! registered threads: a waiter registers before its check, as
//! `tokio::sync::Notify` registers a waiter before `wait_ready` takes the state
//! lock, and then parks. A notification unparks each registered thread, and
//! an unpark before the park leaves a token, so no registered waiter misses a
//! notification. A thread that notifies without a change is a spurious wake.
//! Each negative control removes one safeguard, and Loom finds the
//! interleaving that breaks it. The replay is shared through `std::sync::Arc`,
//! which Loom does not instrument, so a control that deadlocks can unwind
//! cleanly.

use std::sync::Arc;

use loom::sync::Mutex;
use loom::thread::{self, Thread};

mod atomic {
    pub(crate) use loom::sync::atomic::{AtomicU64, Ordering};
}

// The model uses the readiness, publication and stall part of the ledger.
// The checkpoint part has its own Loom tests (`loom_native_replay_ledger.rs`).
#[allow(dead_code)]
#[path = "../../../../rholang/src/rust/interpreter/accounting/native_runtime/checked_operations/trace/replay/state.rs"]
mod state;

#[path = "../../../../rholang/src/rust/interpreter/accounting/native_runtime/checked_operations/trace/replay/progress.rs"]
mod progress;

use progress::{after_change, check_and_park, stall, Registration, ReplayProgress, WaitDecision};
use state::{LedgerError, ReplayState, SlotState};

fn explore(test: impl Fn() + Sync + Send + 'static) {
    let mut builder = loom::model::Builder::new();
    builder.max_threads = 4;
    builder.max_branches = 10_000;
    builder.max_permutations = None;
    builder.max_duration = None;
    builder.preemption_bound = Some(builder.preemption_bound.unwrap_or(3));
    builder.checkpoint_file = None;
    builder.check(test);
}

/// The safeguards of the protocol. `Protocol::default()` keeps all of them.
#[derive(Clone, Copy, Default)]
struct Protocol {
    /// Counts the park in a second critical section, after the check.
    park_after_unlock: bool,
    /// A change does not clear the parked count.
    skip_reset: bool,
    /// The waiter registers for the notification after its check.
    notified_after_check: bool,
    /// Each wake starts a new registration, so the version rule is lost.
    skip_version: bool,
}

/// How a wait ended.
#[derive(Debug, PartialEq, Eq)]
enum Waited {
    Ready,
    Stalled,
    Cancelled,
}

struct Replay {
    state: Mutex<ReplayState>,
    progress: ReplayProgress,
    /// The threads registered for the next notification.
    registered: Mutex<Vec<Thread>>,
    cancelled: Mutex<bool>,
    protocol: Protocol,
}

impl Replay {
    fn new(slots: usize, protocol: Protocol) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(ReplayState::new(
                vec![SlotState::Available; slots],
                Vec::with_capacity(slots),
                slots as u64,
            )),
            progress: ReplayProgress::new(),
            registered: Mutex::new(Vec::new()),
            cancelled: Mutex::new(false),
            protocol,
        })
    }

    /// `notify_waiters`, after the unlock of the change: unparks each
    /// registered thread.
    fn notify(&self) {
        let waiters = std::mem::take(&mut *self.registered.lock().expect("the registration lock"));
        for waiter in waiters {
            waiter.unpark();
        }
    }

    /// `notified()` and `enable()`: the current thread receives the next
    /// notification.
    fn register(&self) {
        self.registered
            .lock()
            .expect("the registration lock")
            .push(thread::current());
    }

    /// `wait_ready` for `slot` after `predecessor`, by a counted task.
    fn wait_ready(&self, slot: usize, predecessor: usize) -> Waited {
        let mut registration = Registration::default();
        loop {
            if !self.protocol.notified_after_check {
                self.register();
            }
            let decision = if self.protocol.park_after_unlock {
                let ready = {
                    let state = self.state.lock().expect("the state lock");
                    state.open().and_then(|()| state.ready(slot, [predecessor]))
                };
                let mut state = self.state.lock().expect("the state lock");
                check_and_park(&mut state, &self.progress, &mut registration, ready, true)
            } else {
                let mut state = self.state.lock().expect("the state lock");
                let ready = state.open().and_then(|()| state.ready(slot, [predecessor]));
                check_and_park(&mut state, &self.progress, &mut registration, ready, true)
            };
            match decision {
                Ok(WaitDecision::Ready) => {
                    assert!(!registration.held(), "a ready waiter holds no registration");
                    return Waited::Ready;
                }
                Ok(WaitDecision::Park) => {}
                Ok(WaitDecision::Stall) => {
                    assert!(!registration.held(), "the declarer holds no registration");
                    self.notify();
                    return Waited::Stalled;
                }
                Err(LedgerError::Stalled) => {
                    assert!(
                        !registration.held(),
                        "a failed waiter holds no registration"
                    );
                    return Waited::Stalled;
                }
                Err(error) => panic!("unexpected ledger error: {error:?}"),
            }
            // A cancel drops the parked future, and the drop of `Wait` leaves
            // the registration under the state lock.
            if *self.cancelled.lock().expect("the cancel flag lock") {
                let state = self.state.lock().expect("the state lock");
                registration.leave(&self.progress, state.version());
                return Waited::Cancelled;
            }
            if self.protocol.notified_after_check {
                self.register();
            }
            thread::park();
            if self.protocol.skip_version {
                registration = Registration::default();
            }
        }
    }

    /// Publishes `slot` as one change. A stalled replay refuses it.
    fn publish(&self, slot: usize) -> bool {
        let mut state = self.state.lock().expect("the state lock");
        let Ok(serial) = state.reserve(slot) else {
            return false;
        };
        state.publish(slot, serial, 1);
        if !self.protocol.skip_reset {
            after_change(&mut state, &self.progress);
        }
        drop(state);
        self.notify();
        true
    }

    /// The live token of a task drops.
    fn finish(&self) {
        if self.progress.finish() {
            let mut state = self.state.lock().expect("the state lock");
            if stall(&mut state, &self.progress) {
                drop(state);
                self.notify();
            }
        }
    }

    /// Spawns a counted task. The parent counts the task before the spawn.
    fn spawn(
        parent: &Arc<Replay>,
        count_in_child: bool,
        body: impl FnOnce(&Replay) -> Waited + Send + 'static,
    ) -> thread::JoinHandle<Waited> {
        if !count_in_child {
            parent.progress.enter();
        }
        let replay = Arc::clone(parent);
        thread::spawn(move || {
            if count_in_child {
                replay.progress.enter();
            }
            let waited = body(&replay);
            replay.finish();
            waited
        })
    }
}

/// The root publishes slot 0, and a counted task waits on slot 0 and then
/// publishes slot 1. The publication can come before or after the park.
fn publish_around_a_park(protocol: Protocol) -> (Arc<Replay>, Waited) {
    let replay = Replay::new(2, protocol);
    assert!(replay.progress.begin());
    let waiter = Replay::spawn(&replay, false, |replay| {
        let waited = replay.wait_ready(1, 0);
        if waited == Waited::Ready {
            assert!(replay.publish(1));
        }
        waited
    });
    let published = replay.publish(0);
    replay.finish();
    let waited = waiter.join().expect("the waiter thread ends");
    assert!(published || replay.progress.stalled());
    (replay, waited)
}

#[test]
fn publication_before_or_after_a_park_never_reads_as_a_stall() {
    explore(|| {
        let (replay, waited) = publish_around_a_park(Protocol::default());
        assert_eq!(waited, Waited::Ready);
        assert!(!replay.progress.stalled());
        replay
            .state
            .lock()
            .expect("the state lock")
            .check_complete()
            .expect("every row completes");
    });
}

#[test]
#[should_panic(
    expected = "negative control: a park counted after the unlock stalled a ready waiter"
)]
fn a_park_after_the_unlock_reads_a_publication_as_a_stall() {
    explore(|| {
        let (_, waited) = publish_around_a_park(Protocol {
            park_after_unlock: true,
            ..Protocol::default()
        });
        assert_eq!(
            waited,
            Waited::Ready,
            "negative control: a park counted after the unlock stalled a ready waiter"
        );
    });
}

#[test]
#[should_panic(expected = "negative control: a stale parked count stalled a ready waiter")]
fn a_change_that_keeps_the_count_reads_as_a_stall() {
    explore(|| {
        let (_, waited) = publish_around_a_park(Protocol {
            skip_reset: true,
            ..Protocol::default()
        });
        assert_eq!(
            waited,
            Waited::Ready,
            "negative control: a stale parked count stalled a ready waiter"
        );
    });
}

#[test]
#[should_panic(expected = "deadlock")]
fn a_notification_registered_after_the_check_is_lost() {
    explore(|| {
        publish_around_a_park(Protocol {
            notified_after_check: true,
            ..Protocol::default()
        });
    });
}

#[test]
fn a_lost_predecessor_stalls_every_waiter() {
    explore(|| {
        let replay = Replay::new(3, Protocol::default());
        assert!(replay.progress.begin());
        let first = Replay::spawn(&replay, false, |replay| replay.wait_ready(1, 0));
        let second = Replay::spawn(&replay, false, |replay| replay.wait_ready(2, 0));
        replay.finish();
        assert_eq!(
            first.join().expect("the first waiter ends"),
            Waited::Stalled
        );
        assert_eq!(
            second.join().expect("the second waiter ends"),
            Waited::Stalled
        );
        assert!(replay.progress.stalled());
        assert_eq!(replay.progress.counts(), (0, 0));
        assert!(
            !replay.progress.begin(),
            "a stalled replay never begins again"
        );
    });
}

#[test]
fn an_inversion_stalls_both_waiters() {
    explore(|| {
        let replay = Replay::new(2, Protocol::default());
        assert!(replay.progress.begin());
        let first = Replay::spawn(&replay, false, |replay| {
            let waited = replay.wait_ready(0, 1);
            if waited == Waited::Ready {
                assert!(replay.publish(0));
            }
            waited
        });
        let second = Replay::spawn(&replay, false, |replay| {
            let waited = replay.wait_ready(1, 0);
            if waited == Waited::Ready {
                assert!(replay.publish(1));
            }
            waited
        });
        replay.finish();
        assert_eq!(
            first.join().expect("the first waiter ends"),
            Waited::Stalled
        );
        assert_eq!(
            second.join().expect("the second waiter ends"),
            Waited::Stalled
        );
        assert!(replay.progress.stalled());
    });
}

/// The root spawns a child that publishes slot 0, then waits on slot 0 and
/// publishes slot 1.
fn parent_waits_on_its_child(count_in_child: bool) -> (Arc<Replay>, Waited) {
    let replay = Replay::new(2, Protocol::default());
    assert!(replay.progress.begin());
    let child = Replay::spawn(&replay, count_in_child, |replay| {
        if replay.publish(0) {
            Waited::Ready
        } else {
            Waited::Stalled
        }
    });
    let waited = replay.wait_ready(1, 0);
    if waited == Waited::Ready {
        assert!(replay.publish(1));
    }
    replay.finish();
    child.join().expect("the child task ends");
    (replay, waited)
}

#[test]
fn a_spawned_task_counts_before_its_parent_waits() {
    explore(|| {
        let (replay, waited) = parent_waits_on_its_child(false);
        assert_eq!(waited, Waited::Ready);
        assert!(!replay.progress.stalled());
        assert!(replay.progress.idle());
    });
}

#[test]
#[should_panic(expected = "negative control: the parent stalled before its child counted")]
fn a_child_that_counts_itself_lets_its_parent_stall() {
    explore(|| {
        let (_, waited) = parent_waits_on_its_child(true);
        assert_eq!(
            waited,
            Waited::Ready,
            "negative control: the parent stalled before its child counted"
        );
    });
}

/// A counted task waits on slot 1 after slot 0 while the root runs, and a
/// third thread wakes the waiter twice without a change. The root then
/// publishes slot 0.
fn spurious_wakes(protocol: Protocol) -> (Arc<Replay>, Waited) {
    let replay = Replay::new(2, protocol);
    assert!(replay.progress.begin());
    let waiter = Replay::spawn(&replay, false, |replay| replay.wait_ready(1, 0));
    let waker = {
        let replay = Arc::clone(&replay);
        thread::spawn(move || {
            replay.notify();
            replay.notify();
        })
    };
    waker.join().expect("the waker thread ends");
    let published = replay.publish(0);
    replay.finish();
    let waited = waiter.join().expect("the waiter thread ends");
    assert!(published || replay.progress.stalled());
    (replay, waited)
}

#[test]
fn a_wake_without_a_change_does_not_count_twice() {
    explore(|| {
        let (replay, waited) = spurious_wakes(Protocol::default());
        assert_eq!(waited, Waited::Ready);
        assert!(!replay.progress.stalled());
        assert!(replay.progress.idle());
    });
}

#[test]
#[should_panic(expected = "negative control: a second count of one waiter stalled the root")]
fn a_wake_that_counts_again_stalls_a_running_root() {
    explore(|| {
        let (_, waited) = spurious_wakes(Protocol {
            skip_version: true,
            ..Protocol::default()
        });
        assert_eq!(
            waited,
            Waited::Ready,
            "negative control: a second count of one waiter stalled the root"
        );
    });
}

/// A counted task waits on slot 1 after slot 0, which no task publishes, and
/// the root cancels the evaluation. The cancel closes the word before the
/// aborts, unless `close_after_aborts`.
fn cancel_a_parked_task(close_after_aborts: bool) -> Arc<Replay> {
    let replay = Replay::new(2, Protocol::default());
    assert!(replay.progress.begin());
    let waiter = Replay::spawn(&replay, false, |replay| replay.wait_ready(1, 0));
    if !close_after_aborts {
        replay.progress.close();
    }
    *replay.cancelled.lock().expect("the cancel flag lock") = true;
    replay.notify();
    replay.finish();
    let waited = waiter.join().expect("the waiter thread ends");
    if close_after_aborts {
        replay.progress.close();
    }
    assert!(waited == Waited::Cancelled || waited == Waited::Stalled);
    replay
}

#[test]
fn a_cancelled_evaluation_declares_no_stall_and_cannot_begin_again() {
    explore(|| {
        let replay = cancel_a_parked_task(false);
        assert!(!replay.progress.stalled());
        assert_eq!(replay.progress.counts(), (0, 0));
        assert!(!replay.progress.idle(), "the closed flag stays");
        assert!(!replay.progress.begin());
        assert_eq!(replay.state.lock().expect("the state lock").open(), Ok(()));
    });
}

#[test]
#[should_panic(expected = "negative control: a cancelled evaluation declared a stall")]
fn aborts_before_the_close_declare_a_late_stall() {
    explore(|| {
        let replay = cancel_a_parked_task(true);
        assert!(
            !replay.progress.stalled(),
            "negative control: a cancelled evaluation declared a stall"
        );
    });
}

#[test]
fn a_finished_evaluation_leaves_the_word_idle() {
    explore(|| {
        let (replay, waited) = publish_around_a_park(Protocol::default());
        assert_eq!(waited, Waited::Ready);
        assert!(replay.progress.idle());
        assert!(replay.progress.begin(), "the next evaluation begins");
        assert!(replay.progress.armed());
        replay.finish();
        assert!(replay.progress.idle());
    });
}
