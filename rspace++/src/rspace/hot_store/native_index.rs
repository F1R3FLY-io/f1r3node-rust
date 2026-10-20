//! D-S1 (D-C2b, DR-96): the digest-keyed ordered index of the native session
//! store.
//!
//! The index has 256 shards. Each shard is an imbl 7.0.2 `OrdMap` from a
//! [`StoreKey`] to a shared entry, and the shard of a key is its first byte.
//! The store never removes a key.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use super::NUM_SHARDS;
use crate::rspace::errors::RSpaceError;
pub use crate::rspace::hashing::native_source::StoreKey;

const _: () = assert!(NUM_SHARDS == 256);

/// One stored entry: the key value, the value, and whether a publishing
/// write changed the entry since the session began. The key value serves the
/// collision check of D-C2e and the export; D-C3 reads the dirty flag.
#[derive(Debug)]
pub struct NativeEntry<K, V> {
    pub key: Arc<K>,
    pub value: V,
    pub dirty: bool,
}

pub type DigestShard<K, V> = imbl::OrdMap<StoreKey, Arc<NativeEntry<K, V>>>;

/// The shards and the key count of an index at a checkpoint.
pub struct DigestSnapshot<K, V> {
    shards: Box<[DigestShard<K, V>; NUM_SHARDS]>,
    keys: usize,
}

pub struct DigestShards<K, V> {
    shards: Box<[RwLock<DigestShard<K, V>>; NUM_SHARDS]>,
    keys: AtomicUsize,
}

impl<K, V> Default for DigestShards<K, V> {
    fn default() -> Self { Self::new() }
}

impl<K, V> DigestShards<K, V> {
    pub fn new() -> Self {
        Self {
            shards: Box::new(std::array::from_fn(|_| RwLock::new(imbl::OrdMap::new()))),
            keys: AtomicUsize::new(0),
        }
    }

    /// The number of distinct keys stored.
    pub fn key_count(&self) -> usize { self.keys.load(Ordering::Acquire) }

    pub fn read(&self, key: &StoreKey) -> RwLockReadGuard<'_, DigestShard<K, V>> {
        self.shards[key.shard()]
            .read()
            .expect("digest shard read lock")
    }

    pub fn write(&self, key: &StoreKey) -> RwLockWriteGuard<'_, DigestShard<K, V>> {
        self.shards[key.shard()]
            .write()
            .expect("digest shard write lock")
    }

    pub fn write_shard(&self, shard: usize) -> RwLockWriteGuard<'_, DigestShard<K, V>> {
        self.shards[shard].write().expect("digest shard write lock")
    }

    /// D-C2c (DR-96): one shard, for the export's iteration in digest order.
    pub fn read_shard(&self, shard: usize) -> RwLockReadGuard<'_, DigestShard<K, V>> {
        self.shards[shard].read().expect("digest shard read lock")
    }

    /// The entry of `key`.
    pub fn get<'s>(
        shard: &'s DigestShard<K, V>,
        key: &StoreKey,
    ) -> Option<&'s Arc<NativeEntry<K, V>>> {
        shard.get(key)
    }

    /// Inserts a key that the shard does not hold. A present key is a broken
    /// invariant of the store, because every caller searched for the key
    /// under the same write guard. Returns the stored entry.
    pub fn insert_new(
        &self,
        shard: &mut DigestShard<K, V>,
        key: StoreKey,
        value_key: Arc<K>,
        value: V,
        dirty: bool,
    ) -> Result<Arc<NativeEntry<K, V>>, RSpaceError> {
        if shard.contains_key(&key) {
            return Err(RSpaceError::BugFoundError(
                "a new native store key is already present".to_owned(),
            ));
        }
        let entry = Arc::new(NativeEntry {
            key: value_key,
            value,
            dirty,
        });
        shard.insert(key, Arc::clone(&entry));
        self.keys.fetch_add(1, Ordering::AcqRel);
        Ok(entry)
    }

    /// Replaces the value of a present key: imbl copies the path to the
    /// entry, and a new entry takes the old entry's key value. The caller
    /// found the key under the same write guard.
    pub fn replace(shard: &mut DigestShard<K, V>, key: &StoreKey, value: V) {
        let slot = shard
            .get_mut(key)
            .expect("a replaced key was found under the same write guard");
        let key_value = Arc::clone(&slot.key);
        *slot = Arc::new(NativeEntry {
            key: key_value,
            value,
            dirty: true,
        });
    }

    /// The shards at this point: 256 constant-time clones. The caller holds
    /// the session's exclusive gate.
    pub fn snapshot(&self) -> DigestSnapshot<K, V> {
        DigestSnapshot {
            shards: Box::new(std::array::from_fn(|index| {
                self.shards[index]
                    .read()
                    .expect("digest shard read lock")
                    .clone()
            })),
            keys: self.keys.load(Ordering::Acquire),
        }
    }

    /// Returns the shards to a snapshot.
    pub fn restore(&self, snapshot: DigestSnapshot<K, V>) {
        for (lock, shard) in self.shards.iter().zip(*snapshot.shards) {
            *lock.write().expect("digest shard write lock") = shard;
        }
        self.keys.store(snapshot.keys, Ordering::Release);
    }
}

#[cfg(test)]
mod tests;
