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
