// See block-storage/src/main/scala/coop/rchain/blockstorage/deploy/KeyValueDeployStorage.scala

use std::collections::HashSet;

use crypto::rust::signatures::signed::Signed;
use models::rust::casper::protocol::casper_message::DeployData;
use models::rust::deploy_envelope::{DeployEnvelope, DeployEnvelopeLimits, StoredDeployEnvelope};
use models::rust::deploy_id::{DeployIdV6, DeployLookupId};
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use shared::rust::store::key_value_store::KvStoreError;
use shared::rust::store::key_value_typed_store::KeyValueTypedStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;
use shared::rust::ByteString;

#[derive(Clone)]
pub struct KeyValueDeployStorage {
    pub store: KeyValueTypedStoreImpl<ByteString, Signed<DeployData>>,
    pub envelope_store: KeyValueTypedStoreImpl<ByteString, StoredDeployEnvelope>,
}

#[derive(Clone, Debug)]
pub enum PendingDeployCandidate {
    Legacy(Signed<DeployData>),
    Envelope(DeployEnvelope),
}

impl KeyValueDeployStorage {
    pub async fn new(kvm: &mut impl KeyValueStoreManager) -> Result<Self, KvStoreError> {
        let deploy_storage_kv_store = kvm.store("deploy_storage".to_string()).await?;
        let deploy_storage_db: KeyValueTypedStoreImpl<ByteString, Signed<DeployData>> =
            KeyValueTypedStoreImpl::new(deploy_storage_kv_store);
        let envelope_storage_kv_store = kvm.store("deploy_envelope_storage".to_string()).await?;
        Ok(Self {
            store: deploy_storage_db,
            envelope_store: KeyValueTypedStoreImpl::new(envelope_storage_kv_store),
        })
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

    pub fn contains_envelope_id(&self, deploy_id: &DeployIdV6) -> Result<bool, KvStoreError> {
        Ok(self
            .envelope_store
            .contains(vec![deploy_id.as_ref().to_vec()])?
            .into_iter()
            .next()
            .unwrap_or(false))
    }

    pub fn add_envelope_if_absent(
        &mut self,
        envelope: &DeployEnvelope,
    ) -> Result<bool, KvStoreError> {
        let DeployLookupId::V6(deploy_id) = envelope.identity() else {
            return Err(KvStoreError::InvalidArgument(
                "legacy envelopes belong in legacy deploy storage".to_string(),
            ));
        };
        let stored =
            StoredDeployEnvelope::new(envelope).map_err(KvStoreError::SerializationError)?;
        self.envelope_store
            .put_one_if_absent(deploy_id.as_ref().to_vec(), stored)
    }

    pub fn read_all_envelopes(
        &self,
        limits: DeployEnvelopeLimits,
    ) -> Result<Vec<PendingDeployCandidate>, KvStoreError> {
        let mut envelopes = self
            .read_all()?
            .into_iter()
            .map(PendingDeployCandidate::Legacy)
            .collect::<Vec<_>>();
        for (key, stored) in self.envelope_store.to_map()? {
            let deploy_id = DeployIdV6::try_from(key.as_slice())
                .map_err(|error| KvStoreError::SerializationError(error.to_string()))?;
            let envelope = stored
                .decode(&DeployLookupId::V6(deploy_id), limits)
                .map_err(KvStoreError::SerializationError)?;
            envelopes.push(PendingDeployCandidate::Envelope(envelope));
        }
        envelopes.sort_by(|left, right| {
            let left_id = match left {
                PendingDeployCandidate::Legacy(deploy) => deploy.sig.as_ref(),
                PendingDeployCandidate::Envelope(deploy) => deploy.identity().as_bytes(),
            };
            let right_id = match right {
                PendingDeployCandidate::Legacy(deploy) => deploy.sig.as_ref(),
                PendingDeployCandidate::Envelope(deploy) => deploy.identity().as_bytes(),
            };
            left_id.cmp(right_id)
        });
        Ok(envelopes)
    }

    pub fn remove_envelope_by_id(&mut self, deploy_id: &DeployIdV6) -> Result<bool, KvStoreError> {
        let key: ByteString = deploy_id.as_ref().to_vec();
        if !self.contains_envelope_id(deploy_id)? {
            return Ok(false);
        }
        self.envelope_store.delete(vec![key])?;
        Ok(true)
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
        self.store.to_map().map(|map| map.into_values().collect())
    }

    /// Check if the storage contains any pending deploys. O(1) time and space.
    pub fn non_empty(&self) -> Result<bool, KvStoreError> {
        Ok(self.store.non_empty()? || self.envelope_store.non_empty()?)
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;
    use std::sync::{Arc, Barrier};

    use crypto::rust::private_key::PrivateKey;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signed::Cosigned;
    use models::rust::cost_deploy_data::DeployData as CostDeployData;
    use models::rust::phlo_controls::PhloControlsLimits;
    use models::rust::phlo_intent::PhloFundingIntentLimits;
    use models::rust::phlo_wire::PhloWireLimits;
    use models::rust::signed_phlo_deploy::FundedDeployLimits;
    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use shared::rust::store::key_value_store::KeyValueStore;

    use super::*;

    fn deploy(time_stamp: i64) -> Signed<DeployData> {
        Signed::create(
            DeployData {
                term: "Nil".to_string(),
                time_stamp,
                phlo_price: 1,
                phlo_limit: 100_000,
                valid_after_block_number: 0,
                shard_id: "root".to_string(),
                expiration_timestamp: None,
            },
            Box::new(Secp256k1),
            PrivateKey::from_bytes(&[1; 32]),
        )
        .unwrap()
    }

    fn envelope_limits() -> DeployEnvelopeLimits {
        let wire = PhloWireLimits {
            total_bytes: 1_048_576,
            field_bytes: 524_288,
        };
        DeployEnvelopeLimits {
            payload: FundedDeployLimits {
                deploy_bytes: 2_097_152,
                signing: wire,
                funding: PhloFundingIntentLimits {
                    wire,
                    controls: PhloControlsLimits {
                        wire,
                        owners: 64,
                        schedules: 4,
                        total_classes: 16,
                    },
                    sources: 64,
                    resource_permissions: 1024,
                    authority_nodes: 4096,
                },
            },
            members: NonZeroUsize::new(64).unwrap(),
        }
    }

    fn body_envelope() -> DeployEnvelope {
        let body = CostDeployData {
            term: "Nil".to_string(),
            language: "rholang".to_string(),
            time_stamp: 1,
            valid_after_block_number: 0,
            shard_id: "root".to_string(),
            expiration_timestamp: None,
            authority_presentations: Vec::new(),
        };
        let signed = Cosigned::create_single_envelope(
            body,
            Box::new(Secp256k1),
            PrivateKey::from_bytes(&[2; 32]),
        )
        .unwrap();
        DeployEnvelope::from_body_envelope(signed).unwrap()
    }

    #[tokio::test]
    async fn envelope_storage_preserves_legacy_rows_and_v6_identity() {
        let mut kvm = InMemoryStoreManager::new();
        let mut storage = KeyValueDeployStorage::new(&mut kvm).await.unwrap();
        let legacy = deploy(5);
        let envelope = body_envelope();
        let DeployLookupId::V6(id) = envelope.identity() else {
            panic!("expected v6 identity")
        };
        storage.add_if_absent(legacy.clone()).unwrap();
        assert!(storage.add_envelope_if_absent(&envelope).unwrap());
        assert!(!storage.add_envelope_if_absent(&envelope).unwrap());
        assert!(storage.contains_envelope_id(id).unwrap());
        assert!(storage.contains_sig(&legacy.sig).unwrap());
        assert!(storage.non_empty().unwrap());
        let candidates = storage.read_all_envelopes(envelope_limits()).unwrap();
        assert!(candidates.iter().any(|candidate| matches!(candidate, PendingDeployCandidate::Legacy(deploy) if deploy == &legacy)));
        assert!(candidates.iter().any(|candidate| matches!(candidate, PendingDeployCandidate::Envelope(deploy) if deploy == &envelope)));
        assert!(storage.remove_envelope_by_id(id).unwrap());
        assert!(!storage.contains_envelope_id(id).unwrap());
        assert!(storage.contains_sig(&legacy.sig).unwrap());
        let reopened = KeyValueDeployStorage::new(&mut kvm).await.unwrap();
        assert_eq!(
            reopened
                .read_all_envelopes(envelope_limits())
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn storage_round_trips_add_contains_read_and_remove() {
        let mut kvm = InMemoryStoreManager::new();
        let mut storage = KeyValueDeployStorage::new(&mut kvm).await.unwrap();
        assert!(!storage.non_empty().unwrap());

        let (d1, d2) = (deploy(1), deploy(2));
        storage.add(vec![d1.clone(), d2.clone()]).unwrap();

        assert!(storage.non_empty().unwrap());
        assert!(storage.contains_sig(&d1.sig).unwrap());
        assert!(!storage.contains_sig(&[0u8; 64]).unwrap());
        assert_eq!(
            storage.read_all().unwrap(),
            HashSet::from([d1.clone(), d2.clone()])
        );

        assert!(storage.any(|d| Ok(d.data.time_stamp == 2)).unwrap());
        assert!(!storage.any(|d| Ok(d.data.time_stamp == 99)).unwrap());

        storage.remove(vec![d2]).unwrap();
        assert_eq!(storage.read_all().unwrap(), HashSet::from([d1.clone()]));

        assert!(storage.remove_by_sig(&d1.sig).unwrap());
        assert!(!storage.remove_by_sig(&d1.sig).unwrap());
        assert!(!storage.non_empty().unwrap());
    }

    #[tokio::test]
    async fn add_if_absent_reports_the_duplicate() {
        let mut kvm = InMemoryStoreManager::new();
        let mut storage = KeyValueDeployStorage::new(&mut kvm).await.unwrap();
        let d1 = deploy(1);
        assert!(storage.add_if_absent(d1.clone()).unwrap());
        assert!(!storage.add_if_absent(d1).unwrap());
        assert_eq!(storage.read_all().unwrap().len(), 1);
    }

    #[test]
    fn add_if_absent_is_atomic_across_storage_handles() {
        let store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let storage = KeyValueDeployStorage {
            store: KeyValueTypedStoreImpl::new(store),
            envelope_store: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
        };
        let deploy = Signed::create(
            DeployData {
                term: "Nil".to_string(),
                time_stamp: 1,
                phlo_price: 1,
                phlo_limit: 100_000,
                valid_after_block_number: 0,
                shard_id: "root".to_string(),
                expiration_timestamp: None,
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
