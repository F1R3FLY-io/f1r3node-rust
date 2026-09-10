use std::collections::hash_map::{Entry, RandomState};
use std::collections::HashMap;
use std::hash::{BuildHasher, Hash};

use super::{Arc, IdentityMutex};

#[derive(Debug)]
pub struct AdmissionIdentities<K, const SHARDS: usize = 16> {
    shards: [IdentityMutex<HashMap<K, Arc<()>>>; SHARDS],
    hash_builder: RandomState,
}

impl<K, const SHARDS: usize> Default for AdmissionIdentities<K, SHARDS> {
    fn default() -> Self {
        assert!(SHARDS > 0);
        Self {
            shards: std::array::from_fn(|_| IdentityMutex::new(HashMap::new())),
            hash_builder: RandomState::new(),
        }
    }
}

impl<K: Clone + Eq + Hash, const SHARDS: usize> AdmissionIdentities<K, SHARDS> {
    pub fn new() -> Self { Self::default() }

    fn shard(&self, key: &K) -> &IdentityMutex<HashMap<K, Arc<()>>> {
        &self.shards[(self.hash_builder.hash_one(key) % SHARDS as u64) as usize]
    }

    pub fn try_claim(self: &Arc<Self>, key: K) -> Option<AdmissionIdentity<K, SHARDS>> {
        let mut entries = self
            .shard(&key)
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        match entries.entry(key.clone()) {
            Entry::Occupied(_) => None,
            Entry::Vacant(entry) => {
                let token = Arc::new(());
                entry.insert(token.clone());
                Some(AdmissionIdentity {
                    identities: self.clone(),
                    key,
                    token,
                })
            }
        }
    }

    pub fn contains(&self, key: &K) -> bool {
        self.shard(key)
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .contains_key(key)
    }

    pub fn len(&self) -> usize {
        self.shards
            .iter()
            .map(|shard| {
                shard
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .len()
            })
            .sum()
    }

    pub fn is_empty(&self) -> bool { self.len() == 0 }

    fn release(&self, key: &K, token: &Arc<()>) {
        let mut entries = self
            .shard(key)
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if entries
            .get(key)
            .is_some_and(|current| Arc::ptr_eq(current, token))
        {
            entries.remove(key);
        }
    }
}

#[derive(Debug)]
pub struct AdmissionIdentity<K: Clone + Eq + Hash, const SHARDS: usize = 16> {
    identities: Arc<AdmissionIdentities<K, SHARDS>>,
    key: K,
    token: Arc<()>,
}

impl<K: Clone + Eq + Hash, const SHARDS: usize> Drop for AdmissionIdentity<K, SHARDS> {
    fn drop(&mut self) { self.identities.release(&self.key, &self.token); }
}

#[cfg(test)]
pub(super) fn previous_owner_cannot_release_replacement_identity() {
    let identities = Arc::new(AdmissionIdentities::<u8, 2>::new());
    let original = identities.try_claim(1).unwrap();
    identities.release(&original.key, &original.token);
    let replacement = identities.try_claim(1).unwrap();
    drop(original);
    assert!(identities.contains(&1));
    assert!(identities.try_claim(1).is_none());
    drop(replacement);
    assert!(identities.is_empty());
}
