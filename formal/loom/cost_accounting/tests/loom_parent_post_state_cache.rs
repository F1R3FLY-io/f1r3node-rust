use std::collections::BTreeMap;

use loom::sync::{Arc, Mutex};
use loom::thread;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct CacheKey {
    main: usize,
    secondary: Vec<usize>,
}

impl CacheKey {
    fn new(main: usize, mut secondary: Vec<usize>) -> Self {
        secondary.sort_unstable();
        Self { main, secondary }
    }
}

#[test]
fn concurrent_different_main_parents_never_share_a_cached_state() {
    loom::model(|| {
        let cache = Arc::new(Mutex::new(BTreeMap::new()));
        let left = {
            let cache = cache.clone();
            thread::spawn(move || {
                cache
                    .lock()
                    .unwrap()
                    .insert(CacheKey::new(1, vec![2, 3]), 101);
            })
        };
        let right = {
            let cache = cache.clone();
            thread::spawn(move || {
                cache
                    .lock()
                    .unwrap()
                    .insert(CacheKey::new(2, vec![1, 3]), 202);
            })
        };

        left.join().unwrap();
        right.join().unwrap();

        let cache = cache.lock().unwrap();
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.get(&CacheKey::new(1, vec![3, 2])), Some(&101));
        assert_eq!(cache.get(&CacheKey::new(2, vec![3, 1])), Some(&202));
    });
}

#[test]
fn concurrent_secondary_permutations_share_the_same_cached_state() {
    loom::model(|| {
        let cache = Arc::new(Mutex::new(BTreeMap::new()));
        let left = {
            let cache = cache.clone();
            thread::spawn(move || {
                cache
                    .lock()
                    .unwrap()
                    .insert(CacheKey::new(1, vec![2, 3]), 101);
            })
        };
        let right = {
            let cache = cache.clone();
            thread::spawn(move || {
                cache
                    .lock()
                    .unwrap()
                    .insert(CacheKey::new(1, vec![3, 2]), 101);
            })
        };

        left.join().unwrap();
        right.join().unwrap();

        let cache = cache.lock().unwrap();
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.get(&CacheKey::new(1, vec![2, 3])), Some(&101));
    });
}
