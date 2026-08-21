// Exhaustive interleaving checks for the metering ownership invariant.
//
// Why this is a shadow model:
// `MeteredMachine` and `RuntimeBudget` use `std::sync` and `std::sync::atomic`.
// Loom only controls interleavings through `loom::sync` and `loom::sync::atomic`,
// so wrapping the production type directly in `loom::model` would not explore
// the queue/reservation race. This model mirrors only the ownership-critical
// protocol: live frame creation, optional shared pending drain, and atomic
// source-token reservation.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering as StdOrdering};
use std::sync::Arc as StdArc;

use loom::sync::atomic::{AtomicUsize, Ordering};
use loom::sync::{Arc, Mutex};
use loom::thread;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owner {
    A,
    B,
}

#[derive(Clone, Copy, Debug)]
struct Frame {
    owner: Owner,
    index: usize,
    weight: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Failure {
    caller: Owner,
    failed_owner: Owner,
}

struct Budget {
    limit: usize,
    consumed: AtomicUsize,
}

impl Budget {
    fn reserve(&self, frame: Frame) -> Result<(), Owner> {
        loop {
            let consumed = self.consumed.load(Ordering::Acquire);
            let next = consumed.saturating_add(frame.weight);

            if next > self.limit {
                self.consumed.store(self.limit, Ordering::Release);
                return Err(frame.owner);
            }

            if self
                .consumed
                .compare_exchange(consumed, next, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return Ok(());
            }
        }
    }
}

fn old_shared_live_reserve(
    caller: Owner,
    frame: Frame,
    pending: &Arc<Mutex<VecDeque<Frame>>>,
    budget: &Arc<Budget>,
) -> Result<(), Failure> {
    pending.lock().unwrap().push_back(frame);

    // This is the old race window: another live branch can drain this
    // branch's just-created frame and inherit its exhaustion error.
    thread::yield_now();

    let mut frames = {
        let mut pending = pending.lock().unwrap();
        pending.drain(..).collect::<Vec<_>>()
    };
    frames.sort_by_key(|frame| frame.index);

    for frame in frames {
        if let Err(failed_owner) = budget.reserve(frame) {
            return Err(Failure {
                caller,
                failed_owner,
            });
        }
    }

    Ok(())
}

fn current_local_live_reserve(
    caller: Owner,
    frame: Frame,
    budget: &Arc<Budget>,
) -> Result<(), Failure> {
    thread::yield_now();

    budget.reserve(frame).map_err(|failed_owner| Failure {
        caller,
        failed_owner,
    })
}

#[test]
fn pre_fix_shared_live_queue_can_misattribute_out_of_phlogistons() {
    let bug_observed = StdArc::new(AtomicBool::new(false));
    let bug_observed_outer = bug_observed.clone();

    loom::model(move || {
        let pending = Arc::new(Mutex::new(VecDeque::new()));
        let budget = Arc::new(Budget {
            limit: 2,
            consumed: AtomicUsize::new(0),
        });

        let h_a = {
            let pending = pending.clone();
            let budget = budget.clone();
            thread::spawn(move || {
                let _ = old_shared_live_reserve(
                    Owner::A,
                    Frame {
                        owner: Owner::A,
                        index: 0,
                        weight: 3,
                    },
                    &pending,
                    &budget,
                );
            })
        };

        let h_b = {
            let pending = pending.clone();
            let budget = budget.clone();
            let observed = bug_observed.clone();
            thread::spawn(move || {
                let result = old_shared_live_reserve(
                    Owner::B,
                    Frame {
                        owner: Owner::B,
                        index: 1,
                        weight: 0,
                    },
                    &pending,
                    &budget,
                );

                if matches!(
                    result,
                    Err(Failure {
                        caller: Owner::B,
                        failed_owner: Owner::A
                    })
                ) {
                    observed.store(true, StdOrdering::SeqCst);
                }
            })
        };

        h_a.join().unwrap();
        h_b.join().unwrap();
    });

    assert!(
        bug_observed_outer.load(StdOrdering::SeqCst),
        "old shared live queue must have an interleaving where B receives A's OOP"
    );
}

#[test]
fn post_fix_local_live_reserve_preserves_error_ownership() {
    loom::model(|| {
        let budget = Arc::new(Budget {
            limit: 2,
            consumed: AtomicUsize::new(0),
        });
        let results = Arc::new(Mutex::new(Vec::new()));

        let h_a = {
            let budget = budget.clone();
            let results = results.clone();
            thread::spawn(move || {
                let result = current_local_live_reserve(
                    Owner::A,
                    Frame {
                        owner: Owner::A,
                        index: 0,
                        weight: 3,
                    },
                    &budget,
                );
                results.lock().unwrap().push(result);
            })
        };

        let h_b = {
            let budget = budget.clone();
            let results = results.clone();
            thread::spawn(move || {
                let result = current_local_live_reserve(
                    Owner::B,
                    Frame {
                        owner: Owner::B,
                        index: 1,
                        weight: 0,
                    },
                    &budget,
                );
                results.lock().unwrap().push(result);
            })
        };

        h_a.join().unwrap();
        h_b.join().unwrap();

        for result in results.lock().unwrap().iter() {
            if let Err(failure) = result {
                assert_eq!(
                    failure.caller, failure.failed_owner,
                    "live reserve returned another branch's exhaustion error"
                );
            }
        }
    });
}
