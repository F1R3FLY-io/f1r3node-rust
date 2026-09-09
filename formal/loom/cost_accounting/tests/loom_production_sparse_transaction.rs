use std::collections::BTreeMap;

use loom::sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};
use loom::thread;

#[path = "../../../../rspace++/src/rspace/shared/sparse_transaction.rs"]
mod sparse_transaction;

use sparse_transaction::{Mutation, Operation, Store as TransactionStore};

#[derive(Clone, Debug, Eq, PartialEq)]
enum Error {
    Manager,
    Conflict(u8, bool),
}

#[derive(Clone)]
struct Store {
    coordinator: Arc<RwLock<()>>,
    data: Arc<Mutex<BTreeMap<u8, u8>>>,
    read_gate: bool,
    write_gate: bool,
}

impl Store {
    fn new(coordinator: Arc<RwLock<()>>, read_gate: bool, write_gate: bool) -> Self {
        Self {
            coordinator,
            data: Arc::new(Mutex::new(BTreeMap::new())),
            read_gate,
            write_gate,
        }
    }
}

impl TransactionStore for Store {
    type Key = u8;
    type Value = u8;
    type Error = Error;
    type ReadGuard<'a> = Option<RwLockReadGuard<'a, ()>>;
    type WriteGuard<'a> = Option<RwLockWriteGuard<'a, ()>>;

    fn same_manager(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.coordinator, &other.coordinator)
    }

    fn same_store(&self, other: &Self) -> bool { Arc::ptr_eq(&self.data, &other.data) }
    fn read_guard(&self) -> Self::ReadGuard<'_> {
        self.read_gate.then(|| self.coordinator.read().unwrap())
    }

    fn write_guard(&self) -> Self::WriteGuard<'_> {
        self.write_gate.then(|| self.coordinator.write().unwrap())
    }

    fn current(&self, key: &u8) -> Option<u8> { self.data.lock().unwrap().get(key).copied() }
    fn put(&self, key: u8, value: u8) { self.data.lock().unwrap().insert(key, value); }
    fn delete(&self, key: &u8) { self.data.lock().unwrap().remove(key); }
    fn manager_error() -> Error { Error::Manager }
    fn conflict(key: &u8, compare_and_swap: bool) -> Error {
        Error::Conflict(*key, compare_and_swap)
    }
}

fn explore(test: impl Fn() + Sync + Send + 'static) {
    let mut builder = loom::model::Builder::new();
    builder.max_threads = 3;
    builder.max_branches = 5_000;
    builder.max_permutations = None;
    builder.max_duration = None;
    builder.preemption_bound = None;
    builder.checkpoint_file = None;
    builder.check(test);
}

fn competing_transactions(write_gate: bool) {
    let coordinator = Arc::new(RwLock::new(()));
    let guard = Store::new(coordinator.clone(), true, write_gate);
    let data = Store::new(coordinator, true, write_gate);
    let workers = [1, 2].map(|owner| {
        let guard = guard.clone();
        let data = data.clone();
        thread::spawn(move || {
            sparse_transaction::apply(&guard, &[
                Mutation {
                    store: &guard,
                    key: &0,
                    operation: Operation::CompareAndSwap {
                        expected: &None,
                        replacement: &Some(owner),
                    },
                },
                Mutation {
                    store: &data,
                    key: &0,
                    operation: Operation::Put(&owner),
                },
            ])
        })
    });
    let results = workers.map(|worker| worker.join().unwrap());
    assert_eq!(
        results.iter().filter(|result| result.is_ok()).count(),
        1,
        "negative control: competing transactions both claimed the same authority"
    );
    let winner = results.iter().position(Result::is_ok).unwrap() as u8 + 1;
    assert_eq!(guard.current(&0), Some(winner));
    assert_eq!(data.current(&0), Some(winner));
    assert_eq!(
        results[usize::from(winner == 1)],
        Err(Error::Conflict(0, true))
    );
    assert!(guard.coordinator.try_write().is_ok());
}

#[test]
fn concurrent_transactions_publish_only_the_cross_store_winner() {
    explore(|| competing_transactions(true));
}

#[test]
#[should_panic(
    expected = "negative control: competing transactions both claimed the same authority"
)]
fn missing_transaction_gate_exposes_a_duplicate_compare_and_swap_winner() {
    explore(|| competing_transactions(false));
}

fn snapshot_during_publication(read_gate: bool, callback_fails: bool) {
    let store = Store::new(Arc::new(RwLock::new(())), read_gate, true);
    store.data.lock().unwrap().extend([(0, 0), (1, 0), (2, 0)]);
    let writer = {
        let store = store.clone();
        thread::spawn(move || {
            sparse_transaction::apply(&store, &[
                Mutation {
                    store: &store,
                    key: &0,
                    operation: Operation::Put(&1),
                },
                Mutation {
                    store: &store,
                    key: &1,
                    operation: Operation::Put(&1),
                },
                Mutation {
                    store: &store,
                    key: &2,
                    operation: Operation::Delete,
                },
            ])
            .unwrap();
        })
    };
    let reader = {
        let store = store.clone();
        thread::spawn(move || {
            let result = sparse_transaction::with_snapshot(&store, || {
                let first = store.current(&0);
                let second = store.current(&1);
                let deleted = store.current(&2);
                assert!(
                    (first, second, deleted) == (Some(0), Some(0), Some(0))
                        || (first, second, deleted) == (Some(1), Some(1), None),
                    "negative control: snapshot observed partial publication"
                );
                if callback_fails {
                    Err("callback")
                } else {
                    Ok(())
                }
            });
            assert_eq!(
                result,
                if callback_fails {
                    Err("callback")
                } else {
                    Ok(())
                }
            );
        })
    };
    writer.join().unwrap();
    reader.join().unwrap();
    assert_eq!(
        *store.data.lock().unwrap(),
        BTreeMap::from([(0, 1), (1, 1)])
    );
    assert!(store.coordinator.try_write().is_ok());
}

#[test]
fn snapshots_exclude_partial_publication_and_release_after_callback_errors() {
    for callback_fails in [false, true] {
        explore(move || snapshot_during_publication(true, callback_fails));
    }
}

#[test]
#[should_panic(expected = "negative control: snapshot observed partial publication")]
fn missing_snapshot_gate_exposes_partial_publication() {
    explore(|| snapshot_during_publication(false, false));
}

#[test]
fn repeated_alias_operations_preserve_rollback_and_explicit_deletion() {
    for conflict in [false, true] {
        explore(move || {
            let coordinator = Arc::new(RwLock::new(()));
            let first = Store::new(coordinator.clone(), true, true);
            let alias = first.clone();
            let second = Store::new(coordinator, true, true);
            first.put(7, 9);
            let result = sparse_transaction::apply(&first, &[
                Mutation {
                    store: &first,
                    key: &0,
                    operation: Operation::Put(&1),
                },
                Mutation {
                    store: &alias,
                    key: &0,
                    operation: Operation::Delete,
                },
                Mutation {
                    store: &first,
                    key: &0,
                    operation: Operation::PutIfAbsentOrEqual(&2),
                },
                Mutation {
                    store: &second,
                    key: &0,
                    operation: Operation::Put(&3),
                },
                Mutation {
                    store: &alias,
                    key: &0,
                    operation: Operation::CompareAndSwap {
                        expected: &Some(if conflict { 1 } else { 2 }),
                        replacement: &None,
                    },
                },
            ]);
            assert_eq!(
                result,
                if conflict {
                    Err(Error::Conflict(0, true))
                } else {
                    Ok(())
                }
            );
            assert_eq!(*first.data.lock().unwrap(), BTreeMap::from([(7, 9)]));
            assert_eq!(second.current(&0), if conflict { None } else { Some(3) });
            assert!(first.coordinator.try_write().is_ok());
        });
    }
}

#[test]
fn distinct_managers_fail_before_the_first_storage_mutation() {
    explore(|| {
        let first = Store::new(Arc::new(RwLock::new(())), true, true);
        let second = Store::new(Arc::new(RwLock::new(())), true, true);
        let _held = first.coordinator.write().unwrap();
        assert_eq!(
            sparse_transaction::apply(&first, &[
                Mutation {
                    store: &first,
                    key: &0,
                    operation: Operation::Put(&1)
                },
                Mutation {
                    store: &second,
                    key: &0,
                    operation: Operation::Put(&2)
                },
            ],),
            Err(Error::Manager)
        );
        assert!(first.data.lock().unwrap().is_empty());
        assert!(second.data.lock().unwrap().is_empty());
    });
}

fn competing_cold_alias_commits(check_other_alias: bool) {
    let store = Store::new(Arc::new(RwLock::new(())), true, true);
    let observed = [store.current(&0), store.current(&1)];
    assert_eq!(observed, [None, None]);
    let workers = [0, 1].map(|alias| {
        let store = store.clone();
        thread::spawn(move || {
            let other = 1 - alias;
            let replacement = Some(alias + 1);
            let mut mutations = vec![Mutation {
                store: &store,
                key: &alias,
                operation: Operation::CompareAndSwap {
                    expected: &observed[usize::from(alias)],
                    replacement: &replacement,
                },
            }];
            if check_other_alias {
                mutations.push(Mutation {
                    store: &store,
                    key: &other,
                    operation: Operation::CompareAndSwap {
                        expected: &observed[usize::from(other)],
                        replacement: &observed[usize::from(other)],
                    },
                });
            }
            sparse_transaction::apply(&store, &mutations)
        })
    });
    let results = workers.map(|worker| worker.join().unwrap());
    assert_eq!(
        results.iter().filter(|result| result.is_ok()).count(),
        1,
        "negative control: stale preflight admitted conflicting cold aliases"
    );
    let winner = results.iter().position(Result::is_ok).unwrap() as u8;
    assert_eq!(store.current(&winner), Some(winner + 1));
    assert_eq!(store.current(&(1 - winner)), None);
}

#[test]
fn state_import_two_alias_guards_reject_stale_opposite_alias_commits() {
    explore(|| competing_cold_alias_commits(true));
}

#[test]
#[should_panic(expected = "negative control: stale preflight admitted conflicting cold aliases")]
fn state_import_missing_opposite_alias_guard_exposes_conflicting_bindings() {
    explore(|| competing_cold_alias_commits(false));
}

#[test]
fn state_import_alias_commit_preserves_an_unrelated_concurrent_write() {
    explore(|| {
        let store = Store::new(Arc::new(RwLock::new(())), true, true);
        let observed = [store.current(&0), store.current(&1)];
        let writer = {
            let store = store.clone();
            thread::spawn(move || {
                sparse_transaction::apply(&store, &[
                    Mutation {
                        store: &store,
                        key: &0,
                        operation: Operation::CompareAndSwap {
                            expected: &observed[0],
                            replacement: &Some(7),
                        },
                    },
                    Mutation {
                        store: &store,
                        key: &1,
                        operation: Operation::CompareAndSwap {
                            expected: &observed[1],
                            replacement: &observed[1],
                        },
                    },
                ])
                .unwrap();
            })
        };
        let unrelated = {
            let store = store.clone();
            thread::spawn(move || {
                sparse_transaction::apply(&store, &[Mutation {
                    store: &store,
                    key: &2,
                    operation: Operation::Put(&9),
                }])
                .unwrap();
            })
        };
        writer.join().unwrap();
        unrelated.join().unwrap();
        assert_eq!(
            *store.data.lock().unwrap(),
            BTreeMap::from([(0, 7), (2, 9)])
        );
    });
}

#[test]
fn state_import_split_reader_preserves_legacy_value_across_raw_insertion() {
    explore(|| {
        let store = Store::new(Arc::new(RwLock::new(())), true, true);
        store.put(1, 7);
        let writer = {
            let store = store.clone();
            thread::spawn(move || {
                sparse_transaction::apply(&store, &[
                    Mutation {
                        store: &store,
                        key: &0,
                        operation: Operation::CompareAndSwap {
                            expected: &None,
                            replacement: &Some(7),
                        },
                    },
                    Mutation {
                        store: &store,
                        key: &1,
                        operation: Operation::CompareAndSwap {
                            expected: &Some(7),
                            replacement: &Some(7),
                        },
                    },
                ])
                .unwrap();
            })
        };
        let reader = {
            let store = store.clone();
            thread::spawn(move || {
                let raw = sparse_transaction::with_snapshot(&store, || store.current(&0));
                let value =
                    raw.or_else(|| sparse_transaction::with_snapshot(&store, || store.current(&1)));
                assert_eq!(value, Some(7));
            })
        };
        writer.join().unwrap();
        reader.join().unwrap();
        assert_eq!(
            *store.data.lock().unwrap(),
            BTreeMap::from([(0, 7), (1, 7)])
        );
    });
}

fn competing_history_batches(identical: bool, guarded: bool) {
    let store = Store::new(Arc::new(RwLock::new(())), true, true);
    store.put(7, 9);
    assert_eq!(store.current(&0), None);
    let workers = [1, 2].map(|owner| {
        let store = store.clone();
        thread::spawn(move || {
            let value = if identical { 8 } else { owner };
            sparse_transaction::apply(&store, &[
                Mutation {
                    store: &store,
                    key: &owner,
                    operation: Operation::PutIfAbsentOrEqual(&owner),
                },
                Mutation {
                    store: &store,
                    key: &0,
                    operation: if guarded {
                        Operation::PutIfAbsentOrEqual(&value)
                    } else {
                        Operation::Put(&value)
                    },
                },
            ])
        })
    });
    let results = workers.map(|worker| worker.join().unwrap());
    assert_eq!(
        results.iter().filter(|result| result.is_ok()).count(),
        if identical { 2 } else { 1 },
        "negative control: stale history absence allowed a conflicting overwrite"
    );
    let mut expected = BTreeMap::from([(7, 9)]);
    for (owner, result) in [1, 2].into_iter().zip(results) {
        if result.is_ok() {
            expected.insert(owner, owner);
            expected.insert(0, if identical { 8 } else { owner });
        } else {
            assert_eq!(result, Err(Error::Conflict(0, false)));
        }
    }
    assert_eq!(*store.data.lock().unwrap(), expected);
}

#[test]
fn state_import_history_identical_competing_batches_both_commit() {
    explore(|| competing_history_batches(true, true));
}

#[test]
fn state_import_history_conflicting_batch_discards_its_private_prefix() {
    explore(|| competing_history_batches(false, true));
}

#[test]
#[should_panic(
    expected = "negative control: stale history absence allowed a conflicting overwrite"
)]
fn state_import_history_unconditional_write_exposes_stale_absence_corruption() {
    explore(|| competing_history_batches(false, false));
}

#[test]
fn state_import_history_conflicting_duplicate_rolls_back_without_losing_cold_writes() {
    explore(|| {
        let coordinator = Arc::new(RwLock::new(()));
        let history = Store::new(coordinator.clone(), true, true);
        let cold = Store::new(coordinator, true, true);
        let batch = {
            let history = history.clone();
            thread::spawn(move || {
                sparse_transaction::apply(&history, &[
                    Mutation {
                        store: &history,
                        key: &1,
                        operation: Operation::PutIfAbsentOrEqual(&9),
                    },
                    Mutation {
                        store: &history,
                        key: &0,
                        operation: Operation::PutIfAbsentOrEqual(&1),
                    },
                    Mutation {
                        store: &history,
                        key: &0,
                        operation: Operation::PutIfAbsentOrEqual(&2),
                    },
                ])
            })
        };
        let unrelated = {
            let cold = cold.clone();
            thread::spawn(move || {
                sparse_transaction::apply(&cold, &[Mutation {
                    store: &cold,
                    key: &4,
                    operation: Operation::PutIfAbsentOrEqual(&5),
                }])
                .unwrap();
            })
        };
        assert_eq!(batch.join().unwrap(), Err(Error::Conflict(0, false)));
        unrelated.join().unwrap();
        assert!(history.data.lock().unwrap().is_empty());
        assert_eq!(*cold.data.lock().unwrap(), BTreeMap::from([(4, 5)]));
    });
}

#[test]
fn state_import_retry_fresh_split_reads_bound_competing_alias_commits() {
    explore(|| {
        let store = Store::new(Arc::new(RwLock::new(())), true, true);
        let worker = {
            let store = store.clone();
            thread::spawn(move || {
                for key in [0, 1] {
                    sparse_transaction::apply(&store, &[Mutation {
                        store: &store,
                        key: &key,
                        operation: Operation::PutIfAbsentOrEqual(&7),
                    }])
                    .unwrap();
                }
            })
        };
        let mut initial_budget = None;
        let mut previous_missing = None;
        let mut attempts = 0;
        let mut conflicts = 0;
        loop {
            let observed =
                [0, 1].map(|key| sparse_transaction::with_snapshot(&store, || store.current(&key)));
            let missing = observed.iter().filter(|value| value.is_none()).count();
            let budget = *initial_budget.get_or_insert(missing);
            if let Some(previous) = previous_missing {
                assert!(missing < previous);
            }
            assert!(conflicts + missing <= budget);
            attempts += 1;
            assert!(attempts <= budget + 1);
            let mutations = [0, 1].map(|key| Mutation {
                store: &store,
                key: if key == 0 { &0 } else { &1 },
                operation: Operation::CompareAndSwap {
                    expected: &observed[key],
                    replacement: &Some(7),
                },
            });
            match sparse_transaction::apply(&store, &mutations) {
                Ok(()) => break,
                Err(Error::Conflict(key, true)) => {
                    assert_eq!(observed[usize::from(key)], None);
                    conflicts += 1;
                    assert!(conflicts <= budget);
                    previous_missing = Some(missing);
                }
                other => panic!("unexpected transaction outcome: {other:?}"),
            }
        }
        worker.join().unwrap();
        assert_eq!(
            *store.data.lock().unwrap(),
            BTreeMap::from([(0, 7), (1, 7)])
        );
    });
}

#[test]
fn state_import_retry_duplicate_guards_have_no_external_progress_witness() {
    explore(|| {
        let store = Store::new(Arc::new(RwLock::new(())), true, true);
        for _ in 0..3 {
            let mutations = [0, 0].map(|_| Mutation {
                store: &store,
                key: &0,
                operation: Operation::CompareAndSwap {
                    expected: &None,
                    replacement: &Some(7),
                },
            });
            assert_eq!(
                sparse_transaction::apply(&store, &mutations),
                Err(Error::Conflict(0, true))
            );
            assert_eq!(store.current(&0), None);
        }
    });
}
