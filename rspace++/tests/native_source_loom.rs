use loom::sync::Arc;
use loom::sync::atomic::{AtomicUsize, Ordering};
use loom::thread;
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::native_source::{self, SourceMeter};
use rspace_plus_plus::rspace::trace::event::Produce;

struct Meter {
    remaining: AtomicUsize,
    atomic: bool,
}

impl SourceMeter for Meter {
    fn reserve(&self, _: usize, _: usize, backing: usize) -> Result<(), RSpaceError> {
        if backing == 0 {
            return Ok(());
        }
        if self.atomic {
            self.remaining
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |remaining| {
                    remaining.checked_sub(backing)
                })
                .map(|_| ())
                .map_err(|_| RSpaceError::HostWorkRejected)
        } else {
            let remaining = self.remaining.load(Ordering::Acquire);
            let next = remaining
                .checked_sub(backing)
                .ok_or(RSpaceError::HostWorkRejected)?;
            thread::yield_now();
            self.remaining.store(next, Ordering::Release);
            Ok(())
        }
    }
}

fn check(limit: usize, atomic: bool) {
    loom::model(move || {
        let meter = Arc::new(Meter {
            remaining: AtomicUsize::new(limit),
            atomic,
        });
        let workers: Vec<_> = (0..2)
            .map(|_| {
                let meter = Arc::clone(&meter);
                thread::Builder::new()
                    .stack_size(1024 * 1024)
                    .spawn(move || match native_source::produce(&(), &(), false, meter.as_ref()) {
                        Ok(source) => {
                            let expected = Produce::create(&(), &(), false);
                            assert_eq!(source.channel_hash, expected.channel_hash);
                            assert_eq!(source.hash, expected.hash);
                            1
                        }
                        Err(RSpaceError::HostWorkRejected) => 0,
                        other => panic!("{other:?}"),
                    })
                    .unwrap()
            })
            .collect();
        let completed: usize = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .sum();
        assert!(completed * 64 <= limit, "source results exceed shared backing budget");
        assert!(meter.remaining.load(Ordering::Acquire) <= limit);
        if limit == 128 {
            assert_eq!(completed, 2);
        }
    });
}

#[test]
fn concurrent_source_construction_respects_shared_backing_budget() {
    for limit in [0, 32, 64, 128] {
        check(limit, true);
    }
}

#[test]
#[should_panic(expected = "source results exceed shared backing budget")]
fn separate_budget_check_and_update_can_authorize_unpaid_sources() { check(64, false); }
