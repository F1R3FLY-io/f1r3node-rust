// See block-storage/src/main/scala/coop/rchain/blockstorage/finality/LastFinalizedKeyValueStorage.scala

use models::rust::block_hash::{BlockHash, BlockHashSerde};
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use shared::rust::store::key_value_store::KvStoreError;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

use super::LastFinalizedStorage;
use crate::rust::key_value_block_store::KeyValueBlockStore;

/// LMDB-backed implementation of LastFinalizedStorage
pub struct LastFinalizedKeyValueStorage {
    last_finalized_block_db: KeyValueTypedStoreImpl<i32, BlockHashSerde>,
    fixed_key: i32,
}

impl LastFinalizedKeyValueStorage {
    /// Sentinel value to mark migration as complete (32 bytes of 0xFF)
    const DONE: BlockHash = Bytes::from_static(&[
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0xFF, 0xFF,
    ]);

    /// Create a new LastFinalizedKeyValueStorage from a typed store
    pub fn new(last_finalized_block_db: KeyValueTypedStoreImpl<i32, BlockHashSerde>) -> Self {
        Self {
            last_finalized_block_db,
            fixed_key: 1,
        }
    }

    /// Create a new LastFinalizedKeyValueStorage from a KeyValueStoreManager
    pub async fn create_from_kvm(kvm: &mut dyn KeyValueStoreManager) -> Result<Self, KvStoreError> {
        let last_finalized_kv_store = kvm.store("last-finalized-block".to_string()).await?;
        let last_finalized_block_db: KeyValueTypedStoreImpl<i32, BlockHashSerde> =
            KeyValueTypedStoreImpl::new(last_finalized_kv_store);
        Ok(Self::new(last_finalized_block_db))
    }

    /// Check if migration from old LastFinalizedStorage format is required
    pub fn require_migration(&self) -> Result<bool, KvStoreError> {
        let value = self.get()?;
        Ok(value.is_some_and(|hash| hash != Self::DONE))
    }

    /// Refuse the legacy LFB migration because it cannot create protocol-v5
    /// certified admission metadata or prove generation-scoped evidence.
    pub async fn migrate_lfb(
        &self,
        _kvm: &mut dyn KeyValueStoreManager,
        _block_store: &KeyValueBlockStore,
    ) -> Result<(), KvStoreError> {
        Err(KvStoreError::InvalidArgument(
            "legacy last-finalized migration cannot produce protocol-v5 certified admission metadata; start from a fresh protocol-v5 genesis or run an explicit verified migration"
                .to_string(),
        ))
    }
}

impl LastFinalizedStorage for LastFinalizedKeyValueStorage {
    fn put(&self, block_hash: BlockHash) -> Result<(), KvStoreError> {
        self.last_finalized_block_db
            .put_one(self.fixed_key, BlockHashSerde(block_hash))
    }

    fn get(&self) -> Result<Option<BlockHash>, KvStoreError> {
        self.last_finalized_block_db
            .get_one(&self.fixed_key)
            .map(|opt| opt.map(|hash_serde| hash_serde.0))
    }
}

#[cfg(test)]
mod tests {
    use models::rust::block_hash;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

    use super::*;

    #[tokio::test]
    async fn legacy_migration_fails_without_mutating_its_marker() {
        let mut manager = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut manager)
            .await
            .unwrap();
        let storage = LastFinalizedKeyValueStorage::create_from_kvm(&mut manager)
            .await
            .unwrap();
        let legacy = Bytes::from(vec![7; block_hash::LENGTH]);
        storage.put(legacy.clone()).unwrap();

        assert!(storage
            .migrate_lfb(&mut manager, &block_store)
            .await
            .is_err());
        assert_eq!(storage.get().unwrap(), Some(legacy));
        assert!(storage.require_migration().unwrap());
    }
}
