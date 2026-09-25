use loom::sync::atomic::{AtomicBool, Ordering};
use loom::sync::Arc;
use loom::thread;

#[path = "../../../../rspace++/src/rspace/replay_rspace/native_session/publication.rs"]
mod production;

use production::PublicationGuard;

fn model(test: impl Fn() + Send + Sync + 'static) {
    let mut builder = loom::model::Builder::new();
    builder.preemption_bound = None;
    builder.max_permutations = None;
    builder.max_duration = None;
    builder.checkpoint_file = None;
    builder.max_branches = 1000;
    builder.check(test);
}

#[test]
fn independent_success_cannot_reopen_a_failed_publication() {
    for left_aborts in [false, true] {
        for right_aborts in [false, true] {
            model(move || {
                let closed = Arc::new(AtomicBool::new(false));
                let workers: Vec<_> = [left_aborts, right_aborts]
                    .into_iter()
                    .map(|aborts| {
                        let closed = Arc::clone(&closed);
                        thread::spawn(move || {
                            let guard = PublicationGuard::new(&closed);
                            thread::yield_now();
                            if aborts {
                                drop(guard);
                            } else {
                                guard.complete();
                            }
                        })
                    })
                    .collect();
                for worker in workers {
                    worker.join().unwrap();
                }
                assert_eq!(closed.load(Ordering::Acquire), left_aborts || right_aborts);
            });
        }
    }
}

#[test]
fn success_does_not_clear_concurrent_explicit_closure() {
    model(|| {
        let closed = Arc::new(AtomicBool::new(false));
        let publishing = Arc::clone(&closed);
        let worker = thread::spawn(move || {
            let guard = PublicationGuard::new(&publishing);
            thread::yield_now();
            guard.complete();
        });
        closed.store(true, Ordering::Release);
        worker.join().unwrap();
        assert!(closed.load(Ordering::Acquire));
    });
}

#[test]
fn poison_precedes_release_of_an_older_owned_ticket() {
    model(|| {
        struct Ticket(Arc<AtomicBool>);
        impl Drop for Ticket {
            fn drop(&mut self) {
                assert!(self.0.load(Ordering::Acquire));
            }
        }
        let closed = Arc::new(AtomicBool::new(false));
        let owned = Arc::clone(&closed);
        thread::spawn(move || {
            let _ticket = Ticket(Arc::clone(&owned));
            let _publication = PublicationGuard::new(&owned);
        })
        .join()
        .unwrap();
        assert!(closed.load(Ordering::Acquire));
    });
}

#[test]
fn failure_closes_before_notifying_waiters_and_success_does_not_notify() {
    for aborted in [false, true] {
        model(move || {
            let closed = Arc::new(AtomicBool::new(false));
            let notified = Arc::new(AtomicBool::new(false));
            let guard = PublicationGuard::with_invalidation(&closed, || {
                assert!(closed.load(Ordering::Acquire));
                notified.store(true, Ordering::Release);
            });
            if aborted {
                drop(guard);
            } else {
                guard.complete();
            }
            assert_eq!(notified.load(Ordering::Acquire), aborted);
        });
    }
}

#[test]
#[should_panic(expected = "completion reopened failed session")]
fn successful_flag_reset_is_an_unsafe_control() {
    model(|| {
        let closed = Arc::new(AtomicBool::new(false));
        let publishing = Arc::clone(&closed);
        let worker = thread::spawn(move || {
            thread::yield_now();
            publishing.store(false, Ordering::Release);
        });
        closed.store(true, Ordering::Release);
        worker.join().unwrap();
        assert!(
            closed.load(Ordering::Acquire),
            "completion reopened failed session"
        );
    });
}
