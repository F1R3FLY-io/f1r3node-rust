// Per-node local buffer of deploys rejected during multi-parent merge.
//
// When the merge algorithm drops a deploy from the canonical merged state,
// its data is placed here so the block creator can re-propose it in a
// subsequent block. Each validator maintains its own buffer; there is no
// cross-validator coordination.
//
// Mirrors KeyValueDeployStorage in shape and storage backing.

use std::collections::HashSet;

use models::rust::deploy_id::DeployLookupId;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use shared::rust::store::key_value_store::KvStoreError;
use shared::rust::store::key_value_typed_store::KeyValueTypedStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

use super::pending_deploy::PendingDeploy;

#[derive(Clone)]
pub struct KeyValueRejectedDeployBuffer {
    pub store: KeyValueTypedStoreImpl<DeployLookupId, PendingDeploy>,
}

impl KeyValueRejectedDeployBuffer {
    pub async fn new(kvm: &mut impl KeyValueStoreManager) -> Result<Self, KvStoreError> {
        let buffer_kv_store = kvm.store("rejected_deploy_buffer".to_string()).await?;
        let buffer_db: KeyValueTypedStoreImpl<DeployLookupId, PendingDeploy> =
            KeyValueTypedStoreImpl::new(buffer_kv_store);
        let buffer = Self { store: buffer_db };
        for (deploy_id, deploy) in buffer.store.to_map()? {
            let protocol_version = match deploy_id {
                DeployLookupId::Legacy(_) => 5,
                DeployLookupId::V6(_) => 6,
            };
            deploy
                .validate_for_protocol(protocol_version)
                .map_err(KvStoreError::InvalidArgument)?;
            if deploy.typed_deploy_id() != &deploy_id {
                return Err(KvStoreError::InvalidArgument(
                    "rejected deploy buffer key does not match its deploy identity".to_string(),
                ));
            }
        }
        Ok(buffer)
    }

    pub fn add(&mut self, deploys: Vec<PendingDeploy>) -> Result<(), KvStoreError> {
        self.store.put(
            deploys
                .into_iter()
                .map(|deploy| (deploy.typed_deploy_id().clone(), deploy))
                .collect(),
        )
    }

    pub fn remove(&mut self, deploys: Vec<PendingDeploy>) -> Result<(), KvStoreError> {
        self.store.delete(
            deploys
                .into_iter()
                .map(|deploy| deploy.typed_deploy_id().clone())
                .collect(),
        )
    }

    pub fn remove_by_id(&mut self, key: &DeployLookupId) -> Result<bool, KvStoreError> {
        let exists = self
            .store
            .contains(vec![key.clone()])?
            .into_iter()
            .next()
            .unwrap_or(false);
        if !exists {
            return Ok(false);
        }
        self.store.delete(vec![key.clone()])?;
        Ok(true)
    }

    pub fn contains_id(&self, key: &DeployLookupId) -> Result<bool, KvStoreError> {
        let exists = self
            .store
            .contains(vec![key.clone()])?
            .into_iter()
            .next()
            .unwrap_or(false);
        Ok(exists)
    }

    pub fn get_by_id(&self, key: &DeployLookupId) -> Result<Option<PendingDeploy>, KvStoreError> {
        let results = self.store.get(&vec![key.clone()])?;
        Ok(results.into_iter().next().flatten())
    }

    pub fn read_all(&self) -> Result<HashSet<PendingDeploy>, KvStoreError> {
        self.store.to_map().map(|map| map.into_values().collect())
    }

    pub fn non_empty(&self) -> Result<bool, KvStoreError> { self.store.non_empty() }
}

#[cfg(test)]
mod tests {
    use crypto::rust::private_key::PrivateKey;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signed::Signed;
    use models::rust::casper::protocol::casper_message::DeployData;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

    use super::*;

    fn deploy(time_stamp: i64) -> PendingDeploy {
        let signed = Signed::create(
            DeployData {
                term: "Nil".to_string(),
                language: "rholang".to_string(),
                time_stamp,
                valid_after_block_number: 0,
                shard_id: "root".to_string(),
                expiration_timestamp: None,
                authority_presentations: Vec::new(),
            },
            Box::new(Secp256k1),
            PrivateKey::from_bytes(&[1; 32]),
        )
        .unwrap();
        PendingDeploy::from_legacy(signed).unwrap()
    }

    async fn buffer() -> KeyValueRejectedDeployBuffer {
        let mut kvm = InMemoryStoreManager::new();
        KeyValueRejectedDeployBuffer::new(&mut kvm).await.unwrap()
    }

    #[tokio::test]
    async fn add_read_all_and_non_empty() {
        let mut buffer = buffer().await;
        assert!(!buffer.non_empty().unwrap());

        let (d1, d2) = (deploy(1), deploy(2));
        buffer.add(vec![d1.clone(), d2.clone()]).unwrap();

        assert!(buffer.non_empty().unwrap());
        assert_eq!(buffer.read_all().unwrap(), HashSet::from([d1, d2]));
    }

    #[tokio::test]
    async fn contains_and_get_by_typed_id() {
        let mut buffer = buffer().await;
        let (d1, d2) = (deploy(1), deploy(2));
        buffer.add(vec![d1.clone()]).unwrap();
        let d1_id = d1.typed_deploy_id().clone();
        let d2_id = d2.typed_deploy_id().clone();

        assert!(buffer.contains_id(&d1_id).unwrap());
        assert!(!buffer.contains_id(&d2_id).unwrap());
        assert_eq!(buffer.get_by_id(&d1_id).unwrap(), Some(d1));
        assert_eq!(buffer.get_by_id(&d2_id).unwrap(), None);
    }

    #[tokio::test]
    async fn remove_deletes_listed_deploys() {
        let mut buffer = buffer().await;
        let (d1, d2) = (deploy(1), deploy(2));
        buffer.add(vec![d1.clone(), d2.clone()]).unwrap();

        buffer.remove(vec![d2]).unwrap();
        assert_eq!(buffer.read_all().unwrap(), HashSet::from([d1]));
    }

    #[tokio::test]
    async fn remove_by_typed_id_reports_presence() {
        let mut buffer = buffer().await;
        let d1 = deploy(1);
        let deploy_id = d1.typed_deploy_id().clone();
        buffer.add(vec![d1.clone()]).unwrap();

        assert!(buffer.remove_by_id(&deploy_id).unwrap());
        assert!(!buffer.remove_by_id(&deploy_id).unwrap());
        assert!(!buffer.contains_id(&deploy_id).unwrap());
        assert!(!buffer.non_empty().unwrap());
    }
}
