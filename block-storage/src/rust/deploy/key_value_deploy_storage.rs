// See block-storage/src/main/scala/coop/rchain/blockstorage/deploy/KeyValueDeployStorage.scala

use std::collections::HashSet;

use crypto::rust::signatures::signed::{Cosigned, Signed};
use models::rust::casper::protocol::casper_message::DeployData;
use models::rust::deploy_id::DeployIdV6;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use shared::rust::store::key_value_store::KvStoreError;
use shared::rust::store::key_value_typed_store::KeyValueTypedStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;
use shared::rust::ByteString;

use super::pending_deploy::PendingDeploy;

#[derive(Clone)]
pub struct KeyValueDeployStorage {
    pub store: KeyValueTypedStoreImpl<ByteString, Signed<DeployData>>,
    pub envelope_store: KeyValueTypedStoreImpl<DeployIdV6, Cosigned<DeployData>>,
}

impl KeyValueDeployStorage {
    pub async fn new(kvm: &mut impl KeyValueStoreManager) -> Result<Self, KvStoreError> {
        let deploy_storage_kv_store = kvm.store("deploy_storage".to_string()).await?;
        let deploy_storage_db: KeyValueTypedStoreImpl<ByteString, Signed<DeployData>> =
            KeyValueTypedStoreImpl::new(deploy_storage_kv_store);
        let envelope_storage_kv_store = kvm.store("deploy_envelope_storage_v6".to_string()).await?;
        let envelope_storage_db: KeyValueTypedStoreImpl<DeployIdV6, Cosigned<DeployData>> =
            KeyValueTypedStoreImpl::new(envelope_storage_kv_store);
        let storage = Self {
            store: deploy_storage_db,
            envelope_store: envelope_storage_db,
        };
        storage.validate_consistency()?;
        Ok(storage)
    }

    fn validate_consistency(&self) -> Result<(), KvStoreError> {
        for (key, envelope) in self.envelope_store.to_map()? {
            let commitment = envelope.envelope_commitment().map_err(|error| {
                KvStoreError::InvalidArgument(format!("invalid deploy envelope row: {error}"))
            })?;
            if commitment.as_ref() != key.as_ref() {
                return Err(KvStoreError::InvalidArgument(
                    "deploy envelope row key does not match its commitment".to_string(),
                ));
            }
        }
        Ok(())
    }

    pub fn add_envelope_if_absent(
        &mut self,
        envelope: Cosigned<DeployData>,
    ) -> Result<bool, KvStoreError> {
        let commitment = envelope.envelope_commitment().map_err(|error| {
            KvStoreError::InvalidArgument(format!("invalid deploy envelope: {error}"))
        })?;
        let deploy_id = DeployIdV6::try_from(commitment.as_ref())
            .map_err(|error| KvStoreError::InvalidArgument(error.to_string()))?;
        self.envelope_store.put_one_if_absent(deploy_id, envelope)
    }

    pub fn get_envelope(
        &self,
        deploy_id: &[u8],
    ) -> Result<Option<Cosigned<DeployData>>, KvStoreError> {
        let deploy_id = DeployIdV6::try_from(deploy_id)
            .map_err(|error| KvStoreError::InvalidArgument(error.to_string()))?;
        self.envelope_store.get_one(&deploy_id)
    }

    pub fn contains_envelope(&self, deploy_id: &[u8]) -> Result<bool, KvStoreError> {
        let deploy_id = DeployIdV6::try_from(deploy_id)
            .map_err(|error| KvStoreError::InvalidArgument(error.to_string()))?;
        self.envelope_store.contains_key(deploy_id)
    }

    pub fn read_all_envelopes(&self) -> Result<HashSet<Cosigned<DeployData>>, KvStoreError> {
        self.envelope_store
            .to_map()
            .map(|map| map.into_values().collect())
    }

    pub fn read_all_for_protocol(
        &self,
        protocol_version: i64,
    ) -> Result<HashSet<PendingDeploy>, KvStoreError> {
        if protocol_version >= 6 {
            if self.store.non_empty()? {
                return Err(KvStoreError::InvalidArgument(
                    "protocol-v6 deploy pool contains legacy payload signatures".to_string(),
                ));
            }
            self.envelope_store
                .to_map()?
                .into_values()
                .map(|envelope| {
                    PendingDeploy::from_envelope_v6(envelope).map_err(KvStoreError::InvalidArgument)
                })
                .collect()
        } else {
            if self.envelope_store.non_empty()? {
                return Err(KvStoreError::InvalidArgument(
                    "pre-v6 deploy pool contains protocol-v6 envelopes".to_string(),
                ));
            }
            self.store
                .to_map()?
                .into_values()
                .map(|deploy| {
                    PendingDeploy::from_legacy(deploy).map_err(KvStoreError::InvalidArgument)
                })
                .collect()
        }
    }

    pub fn remove_envelope_by_id(&mut self, deploy_id: &[u8]) -> Result<bool, KvStoreError> {
        let key = DeployIdV6::try_from(deploy_id)
            .map_err(|error| KvStoreError::InvalidArgument(error.to_string()))?;
        if !self.envelope_store.contains_key(key.clone())? {
            return Ok(false);
        }
        self.envelope_store.delete(vec![key])?;
        Ok(true)
    }

    pub fn add(&mut self, deploys: Vec<Signed<DeployData>>) -> Result<(), KvStoreError> {
        self.store.put(
            deploys
                .into_iter()
                .map(|d| (d.sig.clone().into(), d))
                .collect(),
        )
    }

    /// Atomically insert a deploy by signature, returning false when it already exists.
    pub fn add_if_absent(&mut self, deploy: Signed<DeployData>) -> Result<bool, KvStoreError> {
        let key: ByteString = deploy.sig.to_vec();
        self.store.put_one_if_absent(key, deploy)
    }

    pub fn contains_sig(&self, sig: &[u8]) -> Result<bool, KvStoreError> {
        let key: ByteString = sig.to_vec();
        Ok(self
            .store
            .contains(vec![key])?
            .into_iter()
            .next()
            .unwrap_or(false))
    }

    pub fn remove(&mut self, deploys: Vec<Signed<DeployData>>) -> Result<(), KvStoreError> {
        self.store
            .delete(deploys.into_iter().map(|d| d.sig.clone().into()).collect())
    }

    pub fn remove_by_sig(&mut self, sig: &[u8]) -> Result<bool, KvStoreError> {
        let key: ByteString = sig.to_vec();
        let exists = self
            .store
            .contains(vec![key.clone()])?
            .into_iter()
            .next()
            .unwrap_or(false);
        if !exists {
            return Ok(false);
        }
        self.store.delete(vec![key])?;
        Ok(true)
    }

    pub fn any<F>(&self, predicate: F) -> Result<bool, KvStoreError>
    where F: FnMut(&Signed<DeployData>) -> Result<bool, KvStoreError> {
        self.store.any_value(predicate)
    }

    pub fn read_all(&self) -> Result<HashSet<Signed<DeployData>>, KvStoreError> {
        Ok(self.store.to_map()?.into_values().collect())
    }

    /// Check if the storage contains any pending deploys. O(1) time and space.
    pub fn non_empty(&self) -> Result<bool, KvStoreError> {
        Ok(self.store.non_empty()? || self.envelope_store.non_empty()?)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};

    use crypto::rust::private_key::PrivateKey;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
    use shared::rust::store::key_value_store::KeyValueStore;

    use super::*;

    #[test]
    fn add_if_absent_is_atomic_across_storage_handles() {
        let store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let envelope_store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let storage = KeyValueDeployStorage {
            store: KeyValueTypedStoreImpl::new(store),
            envelope_store: KeyValueTypedStoreImpl::new(envelope_store),
        };
        let deploy = Signed::create(
            DeployData {
                term: "Nil".to_string(),
                language: "rholang".to_string(),
                time_stamp: 1,
                valid_after_block_number: 0,
                shard_id: "root".to_string(),
                expiration_timestamp: None,
                authority_presentations: Vec::new(),
            },
            Box::new(Secp256k1),
            PrivateKey::from_bytes(&[1; 32]),
        )
        .unwrap();
        let barrier = Arc::new(Barrier::new(32));

        let handles = (0..32)
            .map(|_| {
                let mut storage = storage.clone();
                let deploy = deploy.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    storage.add_if_absent(deploy).unwrap()
                })
            })
            .collect::<Vec<_>>();

        let inserted = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .filter(|inserted| *inserted)
            .count();

        assert_eq!(inserted, 1);
        assert_eq!(storage.read_all().unwrap().len(), 1);
    }
}
