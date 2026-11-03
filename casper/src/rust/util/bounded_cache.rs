use std::hash::Hash;
use std::sync::Arc;

use hashlink::LruCache;
use parking_lot::Mutex;

/// A least-recently-used cache holding at most `capacity` entries.
///
/// Entries and their recency order live under one lock, and every operation
/// takes it once and calls nothing else while holding it. Values are shared
/// through `Arc`, so a hit clones a pointer under the lock and the caller
/// clones the value, if it needs to, after the lock is released.
pub struct BoundedCache<K, V> {
    entries: Mutex<LruCache<K, Arc<V>>>,
}

impl<K: Eq + Hash, V> BoundedCache<K, V> {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "a bounded cache needs room for one entry");
        Self {
            entries: Mutex::new(LruCache::new(capacity)),
        }
    }

    /// Returns the entry and marks it most recently used.
    pub fn get(&self, key: &K) -> Option<Arc<V>> { self.entries.lock().get(key).cloned() }

    /// Inserts the entry as most recently used, evicting the least recently
    /// used one when the cache is full.
    pub fn insert(&self, key: K, value: Arc<V>) { self.entries.lock().insert(key, value); }

    pub fn remove(&self, key: &K) { self.entries.lock().remove(key); }

    /// Tests presence without changing the recency order.
    pub fn contains_key(&self, key: &K) -> bool { self.entries.lock().contains_key(key) }

    pub fn len(&self) -> usize { self.entries.lock().len() }

    pub fn is_empty(&self) -> bool { self.entries.lock().is_empty() }

    pub fn clear(&self) { self.entries.lock().clear(); }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    #[test]
    fn a_full_cache_evicts_the_least_recently_used_entry() {
        let cache = BoundedCache::new(2);
        cache.insert(1, Arc::new(11));
        cache.insert(2, Arc::new(22));
        assert_eq!(cache.get(&1).as_deref(), Some(&11));

        cache.insert(3, Arc::new(33));

        assert!(cache.contains_key(&1));
        assert!(!cache.contains_key(&2));
        assert!(cache.contains_key(&3));
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn presence_checks_do_not_refresh_an_entry() {
        let cache = BoundedCache::new(2);
        cache.insert(1, Arc::new(11));
        cache.insert(2, Arc::new(22));
        assert!(cache.contains_key(&1));

        cache.insert(3, Arc::new(33));

        assert!(!cache.contains_key(&1));
    }

    #[test]
    fn a_removed_entry_frees_its_slot() {
        let cache = BoundedCache::new(2);
        cache.insert(1, Arc::new(11));
        cache.insert(2, Arc::new(22));
        cache.remove(&1);

        cache.insert(3, Arc::new(33));

        assert!(cache.contains_key(&2));
        assert!(cache.contains_key(&3));
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn a_hit_does_not_clone_the_value() {
        struct CloneSignal(mpsc::Sender<()>);

        impl Clone for CloneSignal {
            fn clone(&self) -> Self {
                self.0.send(()).expect("clone receiver dropped");
                Self(self.0.clone())
            }
        }

        let (cloned_tx, cloned_rx) = mpsc::channel();
        let cache = BoundedCache::new(1);
        cache.insert(1, Arc::new(CloneSignal(cloned_tx)));

        assert!(cache.get(&1).is_some());
        assert!(cloned_rx.try_recv().is_err());
    }
}
