use std::hash::Hash;

use indexmap::IndexMap;

pub struct ReplayCacheState<K, V> {
    map: IndexMap<K, V>,
    retained_bytes: usize,
}

impl<K: Eq + Hash, V: Clone> ReplayCacheState<K, V> {
    pub fn new(max_entries: usize) -> Self {
        Self {
            map: IndexMap::with_capacity(max_entries),
            retained_bytes: 0,
        }
    }

    pub fn stats(&self) -> (usize, usize) { (self.map.len(), self.retained_bytes) }

    pub fn get(&mut self, key: &K) -> Option<V> {
        let index = self.map.get_index_of(key)?;
        let last = self.map.len() - 1;
        self.map.move_index(index, last);
        self.map.get_index(last).map(|(_, entry)| entry.clone())
    }

    pub fn put(
        &mut self,
        key: K,
        entry: V,
        max_entries: usize,
        max_bytes: usize,
        charged_bytes: impl Fn(&K, &V) -> usize,
    ) -> bool {
        let charged = charged_bytes(&key, &entry);
        if max_entries == 0 || charged > max_bytes {
            return false;
        }

        if let Some((replaced_key, replaced)) = self.map.shift_remove_entry(&key) {
            self.retained_bytes = self
                .retained_bytes
                .saturating_sub(charged_bytes(&replaced_key, &replaced));
        }

        self.retained_bytes = self.retained_bytes.saturating_add(charged);
        self.map.insert(key, entry);

        while self.map.len() > max_entries || self.retained_bytes > max_bytes {
            let Some((removed_key, removed)) = self.map.shift_remove_index(0) else {
                self.retained_bytes = 0;
                break;
            };
            self.retained_bytes = self
                .retained_bytes
                .saturating_sub(charged_bytes(&removed_key, &removed));
        }

        true
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.retained_bytes = 0;
    }

    #[cfg(test)]
    pub fn entries(&self) -> impl DoubleEndedIterator<Item = (&K, &V)> { self.map.iter() }
}

pub fn persist_before_publish<E, R>(
    persist: impl FnOnce() -> Result<(), E>,
    publish: impl FnOnce() -> R,
) -> Result<R, E> {
    persist()?;
    Ok(publish())
}
