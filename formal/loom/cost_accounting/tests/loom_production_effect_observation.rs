use std::cell::RefCell;

use loom::sync::{Arc, Mutex};
use loom::thread;

#[path = "../../../../block-storage/src/rust/finality/finalization_ledger/effect_observation.rs"]
mod effect_observation;

use effect_observation::effect_is_complete;

struct Store {
    cursor: u64,
    receipts: [[bool; 4]; 2],
}

fn explore(test: impl Fn() + Sync + Send + 'static) {
    let mut builder = loom::model::Builder::new();
    builder.max_threads = 4;
    builder.max_branches = 1_000;
    builder.max_permutations = None;
    builder.max_duration = None;
    builder.preemption_bound = None;
    builder.checkpoint_file = None;
    builder.check(test);
}

#[test]
fn concurrent_completion_queries_preserve_all_effect_kinds_during_compaction() {
    for kind in 0..4 {
        explore(move || {
            let store = Arc::new(Mutex::new(Store {
                cursor: 0,
                receipts: [[true; 4]; 2],
            }));
            let readers = (1..=2)
                .map(|revision| {
                    let store = store.clone();
                    thread::spawn(move || {
                        assert!(
                            effect_is_complete(
                                revision,
                                || Ok::<_, ()>(store.lock().unwrap().cursor),
                                || Ok(store.lock().unwrap().receipts[revision as usize - 1][kind]),
                            )
                            .unwrap(),
                            "compaction must preserve logical effect completion"
                        );
                    })
                })
                .collect::<Vec<_>>();
            let writer = {
                let store = store.clone();
                thread::spawn(move || {
                    for revision in 1..=2 {
                        store.lock().unwrap().cursor = revision;
                        store.lock().unwrap().receipts[revision as usize - 1] = [false; 4];
                    }
                })
            };
            for reader in readers {
                reader.join().unwrap();
            }
            writer.join().unwrap();
        });
    }
}

#[test]
fn concurrent_receipt_creation_and_removal_have_a_query_linearization_point() {
    explore(|| {
        let store = Arc::new(Mutex::new(Store {
            cursor: 0,
            receipts: [[false; 4]; 2],
        }));
        let reader = {
            let store = store.clone();
            thread::spawn(move || {
                let snapshots = RefCell::new(Vec::new());
                let result = effect_is_complete(
                    1,
                    || {
                        let store = store.lock().unwrap();
                        snapshots
                            .borrow_mut()
                            .push(store.cursor >= 1 || store.receipts[0][0]);
                        Ok::<_, ()>(store.cursor)
                    },
                    || {
                        let store = store.lock().unwrap();
                        snapshots
                            .borrow_mut()
                            .push(store.cursor >= 1 || store.receipts[0][0]);
                        Ok(store.receipts[0][0])
                    },
                )
                .unwrap();
                let snapshots = snapshots.borrow();
                assert!(snapshots.contains(&result));
                if !result {
                    assert!(!snapshots[1]);
                }
            })
        };
        let writer = thread::spawn(move || {
            store.lock().unwrap().receipts[0] = [true; 4];
            store.lock().unwrap().cursor = 1;
            store.lock().unwrap().receipts[0] = [false; 4];
        });
        reader.join().unwrap();
        writer.join().unwrap();
    });
}

#[test]
#[should_panic(expected = "negative control: completed effect appeared incomplete")]
fn missing_cursor_recheck_has_a_named_false_negative() {
    explore(|| {
        let store = Arc::new(Mutex::new(Store {
            cursor: 0,
            receipts: [[true; 4]; 2],
        }));
        let reader = {
            let store = store.clone();
            thread::spawn(move || {
                let covered = store.lock().unwrap().cursor >= 1;
                let result = covered || store.lock().unwrap().receipts[0][0];
                assert!(
                    result,
                    "negative control: completed effect appeared incomplete"
                );
            })
        };
        let writer = thread::spawn(move || {
            store.lock().unwrap().cursor = 1;
            store.lock().unwrap().receipts[0] = [false; 4];
        });
        reader.join().unwrap();
        writer.join().unwrap();
    });
}
