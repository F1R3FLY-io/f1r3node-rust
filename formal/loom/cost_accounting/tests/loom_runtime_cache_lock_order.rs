use std::collections::VecDeque;

use loom::sync::{Arc, Mutex, RwLock};
use loom::thread;

struct Cache {
    shard: RwLock<Option<usize>>,
    order: Mutex<VecDeque<u8>>,
    writer: Mutex<()>,
}

impl Cache {
    fn new() -> Self {
        Self {
            shard: RwLock::new(Some(17)),
            order: Mutex::new(VecDeque::from([1])),
            writer: Mutex::new(()),
        }
    }

    fn read(&self, indexed: bool) -> Option<usize> {
        let _writer = indexed.then(|| self.writer.lock().unwrap());
        let guard = self.shard.read().unwrap();
        let cached = *guard;
        drop(guard);
        if cached.is_some() {
            let mut order = self.order.lock().unwrap();
            if let Some(position) = order.iter().position(|key| *key == 1) {
                order.remove(position);
            }
            order.push_back(1);
        }
        cached
    }

    fn evict(&self, indexed: bool) {
        let _writer = indexed.then(|| self.writer.lock().unwrap());
        let mut order = self.order.lock().unwrap();
        if order.pop_front().is_some() {
            *self.shard.write().unwrap() = None;
        }
    }
}

fn check_readers_and_eviction(indexed: bool) {
    let mut model = loom::model::Builder::new();
    model.preemption_bound = None;
    model.max_permutations = None;
    model.max_duration = None;
    model.check(move || {
        let cache = Arc::new(Cache::new());
        let second = {
            let cache = cache.clone();
            thread::spawn(move || cache.read(indexed))
        };
        let evictor = {
            let cache = cache.clone();
            thread::spawn(move || cache.evict(indexed))
        };
        let fast_copy = cache.read(false);
        let second_copy = second.join().unwrap();
        evictor.join().unwrap();
        for copied in [fast_copy, second_copy] {
            assert!(copied.is_none() || copied == Some(17));
        }
        assert_eq!(*cache.shard.read().unwrap(), None);
        assert!(cache.order.lock().unwrap().len() <= 1);
        assert!(cache.writer.try_lock().is_ok());
    });
}

#[test]
fn independent_readers_and_eviction_complete_without_a_lock_cycle() {
    check_readers_and_eviction(false);
}

#[test]
fn fast_reader_remains_outside_the_block_index_writer_lock() { check_readers_and_eviction(true); }
