use std::collections::BTreeMap;

use loom::sync::{Arc, Mutex};
use loom::thread;

#[path = "../../../../casper/src/rust/util/rholang/replay_cache_state.rs"]
mod replay_cache_state;

use replay_cache_state::{persist_before_publish, ReplayCacheState};

#[derive(Debug)]
struct Entry {
    context: u8,
    result: u8,
    bytes: usize,
}

type Cache = ReplayCacheState<u8, Arc<Entry>>;

fn charge(_: &u8, entry: &Arc<Entry>) -> usize { 1 + entry.bytes }

fn entry(context: u8, result: u8, bytes: usize) -> Arc<Entry> {
    Arc::new(Entry {
        context,
        result,
        bytes,
    })
}

fn check(cache: &Cache, max_entries: usize, max_bytes: usize) {
    let (count, retained) = cache.stats();
    assert!(count <= max_entries);
    assert!(retained <= max_bytes);
    assert_eq!(
        retained,
        cache.entries().map(|(k, v)| charge(k, v)).sum::<usize>()
    );
    for (key, value) in cache.entries() {
        assert_eq!(*key, value.context);
    }
}

#[test]
fn shared_cache_replacement_eviction_lookup_and_clear_preserve_accounting() {
    loom::model(|| {
        let cache = Arc::new(Mutex::new(Cache::new(2)));
        assert!(cache.lock().unwrap().put(0, entry(0, 10, 1), 2, 5, charge));
        let writer = {
            let cache = cache.clone();
            thread::spawn(move || {
                {
                    let mut state = cache.lock().unwrap();
                    assert!(state.put(0, entry(0, 20, 2), 2, 5, charge));
                    check(&state, 2, 5);
                }
                let mut state = cache.lock().unwrap();
                assert!(state.put(1, entry(1, 30, 2), 2, 5, charge));
                check(&state, 2, 5);
            })
        };
        let reader = {
            let cache = cache.clone();
            thread::spawn(move || {
                let held = {
                    let mut state = cache.lock().unwrap();
                    let held = state.get(&0);
                    check(&state, 2, 5);
                    held
                };
                {
                    let mut state = cache.lock().unwrap();
                    assert!(state.put(2, entry(2, 40, 1), 2, 5, charge));
                    check(&state, 2, 5);
                }
                {
                    let mut state = cache.lock().unwrap();
                    state.clear();
                    assert_eq!(state.stats(), (0, 0));
                }
                if let Some(held) = held {
                    assert_eq!(held.context, 0);
                    assert!(held.result == 10 || held.result == 20);
                }
            })
        };
        writer.join().unwrap();
        reader.join().unwrap();
        check(&cache.lock().unwrap(), 2, 5);
    });
}

#[test]
fn rejected_replacement_does_not_change_value_or_recency() {
    loom::model(|| {
        let cache = Arc::new(Mutex::new(Cache::new(2)));
        {
            let mut state = cache.lock().unwrap();
            assert!(state.put(0, entry(0, 10, 1), 2, 4, charge));
            assert!(state.put(1, entry(1, 20, 1), 2, 4, charge));
        }
        let reject = {
            let cache = cache.clone();
            thread::spawn(move || {
                let mut state = cache.lock().unwrap();
                assert!(!state.put(0, entry(0, 99, 4), 2, 4, charge));
                check(&state, 2, 4);
            })
        };
        let evict = {
            let cache = cache.clone();
            thread::spawn(move || {
                let mut state = cache.lock().unwrap();
                assert!(state.put(2, entry(2, 30, 1), 2, 4, charge));
                check(&state, 2, 4);
            })
        };
        reject.join().unwrap();
        evict.join().unwrap();
        let mut state = cache.lock().unwrap();
        assert!(state.get(&0).is_none());
        assert_eq!(state.get(&1).unwrap().result, 20);
        assert_eq!(state.get(&2).unwrap().result, 30);
    });
}

#[test]
fn shared_publication_persists_before_a_concurrent_cache_hit() {
    loom::model(|| {
        let cache = Arc::new(Mutex::new(Cache::new(2)));
        let durable = Arc::new(Mutex::new(BTreeMap::new()));
        let publish = {
            let cache = cache.clone();
            let durable = durable.clone();
            thread::spawn(move || {
                persist_before_publish(
                    || {
                        durable.lock().unwrap().insert(0, 10);
                        Ok::<(), ()>(())
                    },
                    || cache.lock().unwrap().put(0, entry(0, 10, 1), 2, 4, charge),
                )
                .unwrap()
            })
        };
        let lookup = {
            let cache = cache.clone();
            let durable = durable.clone();
            thread::spawn(move || {
                let hit = cache.lock().unwrap().get(&0);
                if let Some(hit) = hit {
                    assert_eq!(hit.context, 0);
                    assert_eq!(durable.lock().unwrap().get(&0), Some(&hit.result));
                }
            })
        };
        assert!(publish.join().unwrap());
        lookup.join().unwrap();
        let mut state = cache.lock().unwrap();
        assert_eq!(state.get(&0).unwrap().result, 10);
        assert!(state.get(&1).is_none());
        check(&state, 2, 4);
    });
}

#[test]
fn persistence_failure_cannot_publish_or_destroy_an_independent_result() {
    loom::model(|| {
        let cache = Arc::new(Mutex::new(Cache::new(2)));
        let failed = {
            let cache = cache.clone();
            thread::spawn(move || {
                let outcome = persist_before_publish(
                    || Err::<(), _>("injected persistence failure"),
                    || {
                        cache.lock().unwrap().put(0, entry(0, 99, 1), 2, 4, charge);
                        panic!("failed persistence invoked publication");
                    },
                );
                assert_eq!(outcome, Err("injected persistence failure"));
            })
        };
        let successful = {
            let cache = cache.clone();
            thread::spawn(move || {
                assert!(persist_before_publish(
                    || Ok::<(), ()>(()),
                    || cache.lock().unwrap().put(0, entry(0, 10, 1), 2, 4, charge),
                )
                .unwrap());
            })
        };
        failed.join().unwrap();
        successful.join().unwrap();
        let mut state = cache.lock().unwrap();
        assert_eq!(state.get(&0).unwrap().result, 10);
        check(&state, 2, 4);
    });
}

#[test]
fn successful_persistence_does_not_require_optional_cache_admission() {
    for (entries, bytes) in [(0, 4), (2, 1)] {
        loom::model(move || {
            let durable = Mutex::new(false);
            let mut cache = Cache::new(entries);
            let cached = persist_before_publish(
                || {
                    *durable.lock().unwrap() = true;
                    Ok::<(), ()>(())
                },
                || cache.put(0, entry(0, 10, 1), entries, bytes, charge),
            )
            .unwrap();
            assert!(!cached);
            assert!(*durable.lock().unwrap());
            assert_eq!(cache.stats(), (0, 0));
        });
    }
}
