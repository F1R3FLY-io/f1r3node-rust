use std::collections::BTreeSet;

use loom::sync::{Arc, Mutex};
use loom::thread;

#[derive(Clone, Debug, PartialEq, Eq)]
struct PublishedFloor {
    block: usize,
    root: usize,
    effects: BTreeSet<usize>,
}

struct ValidatorState {
    durable: Mutex<ValidatorDurable>,
}

struct ValidatorDurable {
    published: PublishedFloor,
    recorded_roots: BTreeSet<usize>,
}

impl ValidatorState {
    fn new() -> Self {
        Self {
            durable: Mutex::new(ValidatorDurable {
                published: PublishedFloor {
                    block: 0,
                    root: 0,
                    effects: BTreeSet::from([0]),
                },
                recorded_roots: BTreeSet::from([0]),
            }),
        }
    }

    fn capture(&self) -> PublishedFloor { self.durable.lock().unwrap().published.clone() }

    fn record_replay_root(&self, root: usize) {
        self.durable.lock().unwrap().recorded_roots.insert(root);
    }

    fn has_recorded_root(&self, root: usize) -> bool {
        self.durable.lock().unwrap().recorded_roots.contains(&root)
    }

    fn crash_and_restart(&self) { thread::yield_now() }

    fn promote(&self, captured: &PublishedFloor, candidate: PublishedFloor) -> bool {
        let mut durable = self.durable.lock().unwrap();
        if durable.published != *captured
            || !captured.effects.is_subset(&candidate.effects)
            || candidate.block != candidate.root
            || !durable.recorded_roots.contains(&candidate.root)
        {
            return false;
        }
        durable.published = candidate;
        true
    }
}

fn candidate(block: usize) -> PublishedFloor {
    PublishedFloor {
        block,
        root: block,
        effects: (0..=block).collect(),
    }
}

#[test]
fn shared_current_pointer_cannot_authorize_capture_or_publication() {
    loom::model(|| {
        let current_pointer = Arc::new(Mutex::new(0usize));
        let left = Arc::new(ValidatorState::new());
        let right = Arc::new(ValidatorState::new());

        let promote_left = {
            let left = left.clone();
            thread::spawn(move || {
                left.record_replay_root(1);
                let captured = left.capture();
                thread::yield_now();
                assert!(left.promote(&captured, candidate(1)));
            })
        };
        let promote_right = {
            let right = right.clone();
            thread::spawn(move || {
                right.record_replay_root(1);
                let captured = right.capture();
                thread::yield_now();
                assert!(right.promote(&captured, candidate(1)));
            })
        };
        let churn_pointer = {
            let current_pointer = current_pointer.clone();
            thread::spawn(move || {
                *current_pointer.lock().unwrap() = 2;
                thread::yield_now();
                *current_pointer.lock().unwrap() = 0;
            })
        };

        promote_left.join().unwrap();
        promote_right.join().unwrap();
        churn_pointer.join().unwrap();
        assert_eq!(left.capture(), candidate(1));
        assert_eq!(right.capture(), candidate(1));
    });
}

#[test]
fn floor_publication_is_observed_as_one_block_root_effect_tuple() {
    loom::model(|| {
        let state = Arc::new(ValidatorState::new());
        state.record_replay_root(2);
        let writer = {
            let state = state.clone();
            thread::spawn(move || {
                let captured = state.capture();
                assert!(state.promote(&captured, candidate(2)));
            })
        };
        let reader = {
            let state = state.clone();
            thread::spawn(move || {
                let observed = state.capture();
                assert!(observed == candidate(0) || observed == candidate(2));
            })
        };

        writer.join().unwrap();
        reader.join().unwrap();
        assert_eq!(state.capture(), candidate(2));
    });
}

#[test]
fn distinct_validator_promotions_commute_in_the_shared_world() {
    loom::model(|| {
        let world = Arc::new(Mutex::new([
            PublishedFloor {
                block: 0,
                root: 0,
                effects: BTreeSet::from([0]),
            },
            PublishedFloor {
                block: 0,
                root: 0,
                effects: BTreeSet::from([0]),
            },
        ]));
        let left = {
            let world = world.clone();
            thread::spawn(move || world.lock().unwrap()[0] = candidate(1))
        };
        let right = {
            let world = world.clone();
            thread::spawn(move || world.lock().unwrap()[1] = candidate(2))
        };

        left.join().unwrap();
        right.join().unwrap();
        let final_world = world.lock().unwrap();
        assert_eq!(final_world[0], candidate(1));
        assert_eq!(final_world[1], candidate(2));
    });
}

#[test]
fn racing_same_validator_promotions_cannot_publish_from_a_stale_capture() {
    loom::model(|| {
        let state = Arc::new(ValidatorState::new());
        state.record_replay_root(1);
        state.record_replay_root(2);
        let left_capture = state.capture();
        let right_capture = state.capture();
        let left = {
            let state = state.clone();
            thread::spawn(move || state.promote(&left_capture, candidate(1)))
        };
        let right = {
            let state = state.clone();
            thread::spawn(move || state.promote(&right_capture, candidate(2)))
        };

        let left_promoted = left.join().unwrap();
        let right_promoted = right.join().unwrap();
        assert_ne!(left_promoted, right_promoted);
        let published = state.capture();
        assert!(published == candidate(1) || published == candidate(2));
    });
}

#[test]
fn crash_and_restart_preserve_validator_local_replay_roots() {
    loom::model(|| {
        let state = Arc::new(ValidatorState::new());
        let replay = {
            let state = state.clone();
            thread::spawn(move || state.record_replay_root(2))
        };
        replay.join().unwrap();

        let restart = {
            let state = state.clone();
            thread::spawn(move || state.crash_and_restart())
        };
        let observe = {
            let state = state.clone();
            thread::spawn(move || assert!(state.has_recorded_root(2)))
        };

        restart.join().unwrap();
        observe.join().unwrap();
        assert!(state.has_recorded_root(2));
        let captured = state.capture();
        assert!(state.promote(&captured, candidate(2)));
        assert_eq!(state.capture(), candidate(2));
    });
}
