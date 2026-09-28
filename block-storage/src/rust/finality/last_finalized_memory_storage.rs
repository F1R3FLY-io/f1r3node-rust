// See block-storage/src/main/scala/coop/rchain/blockstorage/finality/LastFinalizedMemoryStorage.scala

use std::sync::{Arc, Mutex};

use models::rust::block_hash::BlockHash;
use shared::rust::store::key_value_store::KvStoreError;

use super::LastFinalizedStorage;

/// In-memory implementation of LastFinalizedStorage
/// Uses Arc<Mutex<>> for thread-safe mutable state
pub struct LastFinalizedMemoryStorage {
    state: Arc<Mutex<Option<BlockHash>>>,
}

impl LastFinalizedMemoryStorage {
    /// Create a new LastFinalizedMemoryStorage with empty initial state
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(None)),
        }
    }
}

impl Default for LastFinalizedMemoryStorage {
    fn default() -> Self { Self::new() }
}

impl LastFinalizedStorage for LastFinalizedMemoryStorage {
    fn put(&self, block_hash: BlockHash) -> Result<(), KvStoreError> {
        let mut state = self.state.lock().map_err(|e| {
            KvStoreError::LockError(format!(
                "LastFinalizedMemoryStorage: Failed to acquire lock: {}",
                e
            ))
        })?;
        *state = Some(block_hash);
        Ok(())
    }

    fn get(&self) -> Result<Option<BlockHash>, KvStoreError> {
        let state = self.state.lock().map_err(|e| {
            KvStoreError::LockError(format!(
                "LastFinalizedMemoryStorage: Failed to acquire lock: {}",
                e
            ))
        })?;
        Ok(state.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(byte: u8) -> BlockHash { BlockHash::from(vec![byte; 32]) }

    #[test]
    fn get_returns_none_before_any_put() {
        let storage = LastFinalizedMemoryStorage::default();
        assert_eq!(storage.get().unwrap(), None);
    }

    #[test]
    fn put_then_get_returns_the_latest_value() {
        let storage = LastFinalizedMemoryStorage::new();
        storage.put(hash(1)).unwrap();
        assert_eq!(storage.get().unwrap(), Some(hash(1)));
        storage.put(hash(2)).unwrap();
        assert_eq!(storage.get().unwrap(), Some(hash(2)));
    }

    #[test]
    fn get_or_else_falls_back_only_when_empty() {
        let storage = LastFinalizedMemoryStorage::new();
        assert_eq!(storage.get_or_else(hash(9)).unwrap(), hash(9));
        storage.put(hash(3)).unwrap();
        assert_eq!(storage.get_or_else(hash(9)).unwrap(), hash(3));
    }

    #[test]
    fn get_unsafe_errors_when_empty_and_returns_value_when_present() {
        let storage = LastFinalizedMemoryStorage::new();
        assert!(matches!(
            storage.get_unsafe(),
            Err(KvStoreError::KeyNotFound(_))
        ));
        storage.put(hash(4)).unwrap();
        assert_eq!(storage.get_unsafe().unwrap(), hash(4));
    }
}
