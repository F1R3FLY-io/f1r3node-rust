use std::sync::Arc;

use async_trait::async_trait;
use dashmap::DashMap;
use shared::rust::store::key_value_store::KeyValueStore;

use super::in_mem_key_value_store::InMemoryKeyValueStore;
use super::key_value_store_manager::KeyValueStoreManager;
use super::lmdb_dir_store_manager::{ReadOnlyStore, authority_store_parts};
use crate::rspace::state::rspace_state_authority::RSpaceStateAuthority;

// See shared/src/main/scala/coop/rchain/store/InMemoryStoreManager.scala
// Simple in-memory key value store manager
pub struct InMemoryStoreManager {
    state: DashMap<String, InMemoryKeyValueStore>,
    authorities: DashMap<String, Arc<RSpaceStateAuthority>>,
}

#[async_trait]
impl KeyValueStoreManager for InMemoryStoreManager {
    // This method will return a new instance of the store because of cloning the
    // store
    async fn store(&mut self, name: String) -> Result<Arc<dyn KeyValueStore>, heed::Error> {
        if let Some((prefix, domain)) = authority_store_parts(&name) {
            let prefix = prefix.to_owned();
            let domain = domain.to_owned();
            let authority = self.state_authority(&prefix).await?;
            let inner = match domain.as_str() {
                "history" => authority.history_store()?,
                "cold" => authority.cold_store()?,
                "roots" => authority.roots_store_for_inspection()?,
                _ => unreachable!("authority_store_parts admitted an unavailable domain"),
            };
            return Ok(Arc::new(ReadOnlyStore::new(inner)));
        }
        let kv_store = self
            .state
            .entry(name)
            .or_insert_with(|| InMemoryKeyValueStore::new());

        Ok(Arc::new(kv_store.value().clone()))
    }

    async fn shutdown(&mut self) -> Result<(), heed::Error> {
        self.state.clear();
        self.authorities.clear();
        Ok(())
    }

    async fn state_authority(
        &mut self,
        db_prefix: &str,
    ) -> Result<Arc<RSpaceStateAuthority>, heed::Error> {
        Ok(self
            .authorities
            .entry(db_prefix.to_string())
            .or_insert_with(RSpaceStateAuthority::in_memory)
            .clone())
    }
}

impl InMemoryStoreManager {
    pub fn new() -> Self {
        InMemoryStoreManager {
            state: DashMap::new(),
            authorities: DashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn authority_names_are_read_only_without_claiming_unrelated_suffixes() {
        let mut manager = InMemoryStoreManager::new();
        let authority_history = manager
            .store("rspace-history".to_string())
            .await
            .expect("authority history view");
        assert!(authority_history.put_one(vec![1], vec![2]).is_err());

        let application_history = manager
            .store("application-history".to_string())
            .await
            .expect("ordinary application store");
        application_history
            .put_one(vec![1], vec![2])
            .expect("unrelated suffix remains writable");
        assert_eq!(application_history.get_one(&vec![1]).unwrap(), Some(vec![2]));
    }
}
