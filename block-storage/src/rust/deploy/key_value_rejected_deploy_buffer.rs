// Per-node local buffer of deploys rejected during multi-parent merge.
//
// When the merge algorithm drops a deploy from the canonical merged state,
// its data is placed here so the block creator can re-propose it in a
// subsequent block. Each validator maintains its own buffer; there is no
// cross-validator coordination.
//
// Mirrors KeyValueDeployStorage in shape and storage backing.

use std::collections::HashSet;

use models::rust::deploy_envelope::{DeployEnvelopeFormat, DeployEnvelopeLimits};
use models::rust::deploy_id::DeployLookupId;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use shared::rust::store::key_value_store::{
    strict_atomic_mutate, AtomicStoreMutation, AtomicStoreOperation, KvStoreError,
};
use shared::rust::store::key_value_typed_store::KeyValueTypedStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

use super::pending_deploy::PendingDeploy;
use super::versioned_deploy_storage::{DeployEnvelopeStoreKind, FundedDeployStorage};

#[derive(Clone)]
pub struct KeyValueRejectedDeployBuffer {
    pub store: KeyValueTypedStoreImpl<DeployLookupId, PendingDeploy>,
    funded_store: Option<FundedDeployStorage>,
}

impl KeyValueRejectedDeployBuffer {
    pub async fn new(kvm: &mut impl KeyValueStoreManager) -> Result<Self, KvStoreError> {
        let buffer_kv_store = kvm.store("rejected_deploy_buffer".to_string()).await?;
        let buffer_db: KeyValueTypedStoreImpl<DeployLookupId, PendingDeploy> =
            KeyValueTypedStoreImpl::new(buffer_kv_store);
        let buffer = Self::from_legacy_store(buffer_db);
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

    pub fn from_legacy_store(store: KeyValueTypedStoreImpl<DeployLookupId, PendingDeploy>) -> Self {
        Self {
            store,
            funded_store: None,
        }
    }

    pub async fn new_with_limits(
        kvm: &mut impl KeyValueStoreManager,
        limits: DeployEnvelopeLimits,
    ) -> Result<Self, KvStoreError> {
        let mut buffer = Self::new(kvm).await?;
        buffer.funded_store =
            Some(FundedDeployStorage::new(kvm, DeployEnvelopeStoreKind::Rejected, limits).await?);
        Ok(buffer)
    }

    fn is_funded(deploy: &PendingDeploy) -> bool {
        matches!(
            deploy.envelope().format(),
            DeployEnvelopeFormat::Funded | DeployEnvelopeFormat::OfferedFunded
        )
    }

    fn require_funded_store(&self) -> Result<&FundedDeployStorage, KvStoreError> {
        self.funded_store.as_ref().ok_or_else(|| {
            KvStoreError::InvalidArgument(
                "funded rejected storage requires explicit envelope limits".to_string(),
            )
        })
    }

    pub fn add(&mut self, deploys: Vec<PendingDeploy>) -> Result<(), KvStoreError> {
        if self.funded_store.is_some() {
            let mutations = deploys
                .iter()
                .map(|deploy| {
                    if Self::is_funded(deploy) {
                        self.require_funded_store()?.prepare_put(deploy.envelope())
                    } else {
                        Ok(AtomicStoreMutation {
                            store: self.store.raw_store().as_ref(),
                            key: self.store.encode_key(deploy.typed_deploy_id())?,
                            operation: AtomicStoreOperation::Put(self.store.encode_value(deploy)?),
                        })
                    }
                })
                .collect::<Result<Vec<_>, KvStoreError>>()?;
            return strict_atomic_mutate(&mutations);
        }
        self.store.put(
            deploys
                .into_iter()
                .map(|deploy| (deploy.typed_deploy_id().clone(), deploy))
                .collect(),
        )
    }

    pub fn remove(&mut self, deploys: Vec<PendingDeploy>) -> Result<(), KvStoreError> {
        if self.funded_store.is_some() {
            let mutations = deploys
                .iter()
                .map(|deploy| {
                    if Self::is_funded(deploy) {
                        self.require_funded_store()?
                            .prepare_delete(deploy.typed_deploy_id())
                    } else {
                        Ok(AtomicStoreMutation {
                            store: self.store.raw_store().as_ref(),
                            key: self.store.encode_key(deploy.typed_deploy_id())?,
                            operation: AtomicStoreOperation::Delete,
                        })
                    }
                })
                .collect::<Result<Vec<_>, KvStoreError>>()?;
            return strict_atomic_mutate(&mutations);
        }
        if deploys.iter().any(Self::is_funded) {
            self.require_funded_store()?;
        }
        self.store.delete(
            deploys
                .into_iter()
                .map(|deploy| deploy.typed_deploy_id().clone())
                .collect(),
        )
    }

    pub fn remove_by_id(&mut self, key: &DeployLookupId) -> Result<bool, KvStoreError> {
        match self.get_by_id(key)? {
            None => Ok(false),
            Some(deploy) if Self::is_funded(&deploy) => self.require_funded_store()?.remove(key),
            Some(_) => Ok(self
                .store
                .raw_store()
                .delete(vec![self.store.encode_key(key)?])?
                != 0),
        }
    }

    pub fn contains_id(&self, key: &DeployLookupId) -> Result<bool, KvStoreError> {
        Ok(self.get_by_id(key)?.is_some())
    }

    pub fn get_by_id(&self, key: &DeployLookupId) -> Result<Option<PendingDeploy>, KvStoreError> {
        let historical = self.store.get_one(key)?;
        if historical
            .as_ref()
            .is_some_and(|deploy| deploy.typed_deploy_id() != key)
        {
            return Err(KvStoreError::InvalidArgument(
                "rejected deploy buffer key does not match its deploy identity".to_string(),
            ));
        }
        let funded = self
            .funded_store
            .as_ref()
            .map(|store| store.get(key))
            .transpose()?
            .flatten()
            .map(PendingDeploy::from_envelope)
            .transpose()
            .map_err(KvStoreError::InvalidArgument)?;
        match (historical, funded) {
            (Some(_), Some(_)) => Err(KvStoreError::InvalidArgument(
                "rejected deploy identity occurs in multiple stores".to_string(),
            )),
            (historical, funded) => Ok(historical.or(funded)),
        }
    }

    pub fn read_all(&self) -> Result<HashSet<PendingDeploy>, KvStoreError> {
        let mut records = self.store.to_map()?;
        for (key, deploy) in &records {
            if key != deploy.typed_deploy_id() {
                return Err(KvStoreError::InvalidArgument(
                    "rejected deploy buffer key does not match its deploy identity".to_string(),
                ));
            }
        }
        if let Some(store) = &self.funded_store {
            store.visit(&mut |envelope| {
                let deploy = PendingDeploy::from_envelope(envelope)
                    .map_err(KvStoreError::InvalidArgument)?;
                if records
                    .insert(deploy.typed_deploy_id().clone(), deploy)
                    .is_some()
                {
                    return Err(KvStoreError::InvalidArgument(
                        "rejected deploy identity occurs in multiple stores".to_string(),
                    ));
                }
                Ok(())
            })?;
        }
        Ok(records.into_values().collect())
    }

    pub fn non_empty(&self) -> Result<bool, KvStoreError> {
        Ok(self.store.non_empty()?
            || self
                .funded_store
                .as_ref()
                .map(FundedDeployStorage::non_empty)
                .transpose()?
                .unwrap_or(false))
    }
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
