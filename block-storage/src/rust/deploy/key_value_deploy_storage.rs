// See block-storage/src/main/scala/coop/rchain/blockstorage/deploy/KeyValueDeployStorage.scala

use std::collections::HashSet;

use crypto::rust::signatures::signed::{Cosigned, Signed};
use models::rust::casper::protocol::casper_message::DeployData;
use models::rust::deploy_envelope::{DeployEnvelopeFormat, DeployEnvelopeLimits};
use models::rust::deploy_id::{DeployIdV6, DeployLookupId};
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use shared::rust::store::key_value_store::KvStoreError;
use shared::rust::store::key_value_typed_store::KeyValueTypedStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;
use shared::rust::ByteString;

use super::pending_deploy::PendingDeploy;
use super::versioned_deploy_storage::{DeployEnvelopeStoreKind, FundedDeployStorage};

#[derive(Clone)]
pub struct KeyValueDeployStorage {
    pub store: KeyValueTypedStoreImpl<ByteString, Signed<DeployData>>,
    pub envelope_store: KeyValueTypedStoreImpl<DeployIdV6, Cosigned<DeployData>>,
    funded_store: Option<FundedDeployStorage>,
}

impl KeyValueDeployStorage {
    pub fn from_legacy_stores(
        store: KeyValueTypedStoreImpl<ByteString, Signed<DeployData>>,
        envelope_store: KeyValueTypedStoreImpl<DeployIdV6, Cosigned<DeployData>>,
    ) -> Self {
        Self {
            store,
            envelope_store,
            funded_store: None,
        }
    }

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
            funded_store: None,
        };
        storage.validate_consistency()?;
        Ok(storage)
    }

    pub async fn new_with_limits(
        kvm: &mut impl KeyValueStoreManager,
        limits: DeployEnvelopeLimits,
    ) -> Result<Self, KvStoreError> {
        let mut storage = Self::new(kvm).await?;
        storage.funded_store =
            Some(FundedDeployStorage::new(kvm, DeployEnvelopeStoreKind::Pending, limits).await?);
        Ok(storage)
    }

    pub fn add_pending_if_absent(&mut self, pending: &PendingDeploy) -> Result<bool, KvStoreError> {
        match pending.envelope().format() {
            DeployEnvelopeFormat::Legacy => {
                let envelope = pending
                    .envelope()
                    .body_envelope()
                    .map_err(KvStoreError::InvalidArgument)?;
                if envelope.signers().len() != 1 {
                    return Err(KvStoreError::InvalidArgument(
                        "legacy pending storage requires one signer".to_string(),
                    ));
                }
                let signer = envelope.primary();
                self.add_if_absent(Signed {
                    data: envelope.data.clone(),
                    pk: signer.pk.clone(),
                    sig: signer.sig.clone(),
                    sig_algorithm: signer.sig_algorithm.clone(),
                })
            }
            DeployEnvelopeFormat::BodyV61 => self.add_envelope_if_absent(
                pending
                    .envelope()
                    .body_envelope()
                    .map_err(KvStoreError::InvalidArgument)?
                    .clone(),
            ),
            DeployEnvelopeFormat::Funded | DeployEnvelopeFormat::OfferedFunded => self
                .funded_store
                .as_ref()
                .ok_or_else(|| {
                    KvStoreError::InvalidArgument(
                        "funded pending storage requires explicit envelope limits".to_string(),
                    )
                })?
                .insert_if_absent(pending.envelope()),
        }
    }

    pub fn get_pending(
        &self,
        identity: &DeployLookupId,
    ) -> Result<Option<PendingDeploy>, KvStoreError> {
        let historical = match identity {
            DeployLookupId::Legacy(signature) => self
                .store
                .get_one(&signature.as_bytes().to_vec())?
                .map(PendingDeploy::from_legacy)
                .transpose(),
            DeployLookupId::V6(id) => self
                .envelope_store
                .get_one(id)?
                .map(PendingDeploy::from_envelope_v6)
                .transpose(),
        }
        .map_err(KvStoreError::InvalidArgument)?;
        if historical
            .as_ref()
            .is_some_and(|pending| pending.typed_deploy_id() != identity)
        {
            return Err(KvStoreError::InvalidArgument(
                "pending store key does not match its envelope".to_string(),
            ));
        }
        let funded = self
            .funded_store
            .as_ref()
            .map(|store| store.get(identity))
            .transpose()?
            .flatten()
            .map(PendingDeploy::from_envelope)
            .transpose()
            .map_err(KvStoreError::InvalidArgument)?;
        match (historical, funded) {
            (Some(_), Some(_)) => Err(KvStoreError::InvalidArgument(
                "pending identity occurs in multiple stores".to_string(),
            )),
            (historical, funded) => Ok(historical.or(funded)),
        }
    }

    pub fn read_all_pending(&self) -> Result<HashSet<PendingDeploy>, KvStoreError> {
        let mut pending = HashSet::new();
        for (key, deploy) in self.store.to_map()? {
            let deploy =
                PendingDeploy::from_legacy(deploy).map_err(KvStoreError::InvalidArgument)?;
            if deploy.deploy_id().as_ref() != key.as_slice() {
                return Err(KvStoreError::InvalidArgument(
                    "legacy pending key does not match its signature".to_string(),
                ));
            }
            pending.insert(deploy);
        }
        for (key, envelope) in self.envelope_store.to_map()? {
            let deploy =
                PendingDeploy::from_envelope_v6(envelope).map_err(KvStoreError::InvalidArgument)?;
            if deploy.typed_deploy_id() != &DeployLookupId::V6(key) {
                return Err(KvStoreError::InvalidArgument(
                    "pending envelope key does not match its commitment".to_string(),
                ));
            }
            pending.insert(deploy);
        }
        if let Some(store) = &self.funded_store {
            store.visit(&mut |envelope| {
                let deploy = PendingDeploy::from_envelope(envelope)
                    .map_err(KvStoreError::InvalidArgument)?;
                if !pending.insert(deploy) {
                    return Err(KvStoreError::InvalidArgument(
                        "pending identity occurs in multiple stores".to_string(),
                    ));
                }
                Ok(())
            })?;
        }
        Ok(pending)
    }

    pub fn remove_pending(&mut self, pending: &PendingDeploy) -> Result<bool, KvStoreError> {
        match pending.envelope().format() {
            DeployEnvelopeFormat::Legacy => self.remove_by_sig(pending.deploy_id()),
            DeployEnvelopeFormat::BodyV61 => self.remove_envelope_by_id(pending.deploy_id()),
            DeployEnvelopeFormat::Funded | DeployEnvelopeFormat::OfferedFunded => self
                .funded_store
                .as_ref()
                .ok_or_else(|| {
                    KvStoreError::InvalidArgument(
                        "funded pending storage requires explicit envelope limits".to_string(),
                    )
                })?
                .remove(pending.typed_deploy_id()),
        }
    }

    fn validate_consistency(&self) -> Result<(), KvStoreError> {
        for (key, envelope) in self.envelope_store.to_map()? {
            envelope.validate_envelope().map_err(|error| {
                KvStoreError::InvalidArgument(format!(
                    "invalid authenticated deploy envelope row: {error}"
                ))
            })?;
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
        envelope.validate_envelope().map_err(|error| {
            KvStoreError::InvalidArgument(format!("invalid authenticated deploy envelope: {error}"))
        })?;
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
        if self
            .funded_store
            .as_ref()
            .map(FundedDeployStorage::non_empty)
            .transpose()?
            .unwrap_or(false)
        {
            return Err(KvStoreError::InvalidArgument(
                "funded pending records require an explicit admission policy".to_string(),
            ));
        }
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
        Ok(self.store.non_empty()?
            || self.envelope_store.non_empty()?
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
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::{Arc, Barrier};

    use crypto::rust::private_key::PrivateKey;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;
    use crypto::rust::signatures::signed::Cosigner;
    use models::rust::casper::protocol::casper_message::ProcessedDeploy;
    use prost::bytes::Bytes;
    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    use rspace_plus_plus::rspace::shared::lmdb_dir_store_manager::{
        Db, LmdbDirStoreManager, LmdbEnvConfig,
    };
    use shared::rust::store::key_value_store::KeyValueStore;

    use super::*;

    fn deploy_data() -> DeployData {
        DeployData {
            term: "Nil".to_string(),
            language: "rholang".to_string(),
            time_stamp: 1,
            valid_after_block_number: 0,
            shard_id: "root".to_string(),
            expiration_timestamp: None,
            authority_presentations: Vec::new(),
        }
    }

    fn threshold_envelope() -> Cosigned<DeployData> {
        let secp = Secp256k1;
        let mut members = (0..3)
            .map(|_| {
                let (private_key, public_key) = secp.new_key_pair();
                (
                    Cosigner {
                        pk: public_key,
                        sig: Bytes::new(),
                        sig_algorithm: Box::new(secp.clone()),
                    },
                    private_key,
                )
            })
            .collect::<Vec<_>>();
        members.sort_by_key(|(signer, _)| signer.principal_bytes_v61().unwrap());
        let selected = [0usize, 2];
        let threshold = 2;
        let mut bitmap = vec![0u8; members.len().div_ceil(8)];
        for index in selected {
            bitmap[index / 8] |= 1 << (index % 8);
        }
        let unsigned = members
            .iter()
            .map(|(signer, _)| signer.clone())
            .collect::<Vec<_>>();
        let data = deploy_data();
        for index in selected {
            let hash = Cosigned::<DeployData>::envelope_signing_hash_for_presence(
                &data,
                &unsigned,
                threshold,
                &bitmap,
                &members[index].0.sig_algorithm.name(),
            )
            .unwrap();
            members[index].0.sig = members[index]
                .0
                .sig_algorithm
                .sign(&hash, &members[index].1.bytes)
                .into();
        }
        Cosigned::from_envelope_signed_data_threshold(
            data,
            members.into_iter().map(|(signer, _)| signer).collect(),
            threshold,
        )
        .unwrap()
    }

    fn deploy(time_stamp: i64) -> Signed<DeployData> {
        let mut data = deploy_data();
        data.time_stamp = time_stamp;
        Signed::create(data, Box::new(Secp256k1), PrivateKey::from_bytes(&[1; 32])).unwrap()
    }

    fn lmdb_manager(path: PathBuf) -> impl KeyValueStoreManager {
        let config = LmdbEnvConfig::new("deploy-env".to_string(), 16 << 20).with_max_dbs(4);
        LmdbDirStoreManager::new(
            path,
            HashMap::from([
                (Db::new("deploy_storage".to_string(), None), config.clone()),
                (
                    Db::new("deploy_envelope_storage_v6".to_string(), None),
                    config,
                ),
            ]),
        )
    }

    fn scratch_directory(prefix: &str) -> tempfile::TempDir {
        let scratch = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("target/block-storage-test-scratch");
        std::fs::create_dir_all(&scratch).unwrap();
        tempfile::Builder::new()
            .prefix(prefix)
            .tempdir_in(scratch)
            .unwrap()
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
        let envelope_store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let storage = KeyValueDeployStorage {
            store: KeyValueTypedStoreImpl::new(store),
            envelope_store: KeyValueTypedStoreImpl::new(envelope_store),
            funded_store: None,
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

    #[test]
    fn threshold_envelope_survives_lmdb_reopen_and_processed_round_trip() {
        let directory = scratch_directory("deploy-envelope-reopen-");
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let envelope = threshold_envelope();
        let commitment = envelope.envelope_commitment().unwrap();

        {
            let mut manager = lmdb_manager(directory.path().to_path_buf());
            let mut storage = runtime
                .block_on(KeyValueDeployStorage::new(&mut manager))
                .unwrap();
            assert!(storage.add_envelope_if_absent(envelope.clone()).unwrap());
            drop(storage);
            runtime.block_on(manager.shutdown()).unwrap();
        }

        {
            let mut manager = lmdb_manager(directory.path().to_path_buf());
            let storage = runtime
                .block_on(KeyValueDeployStorage::new(&mut manager))
                .unwrap();
            let pending = storage
                .read_all_for_protocol(6)
                .unwrap()
                .into_iter()
                .next()
                .unwrap();
            assert_eq!(pending.deploy_id().as_ref(), commitment.as_ref());
            assert_eq!(pending.envelope().body_envelope().unwrap(), &envelope);
            let processed =
                ProcessedDeploy::empty_from_cosigned(pending.envelope().body_envelope().unwrap())
                    .unwrap();
            assert_eq!(processed.to_cosigned().unwrap(), envelope);
            drop(storage);
            runtime.block_on(manager.shutdown()).unwrap();
        }
    }

    #[test]
    fn lmdb_reopen_rejects_tampered_persisted_envelope_signature() {
        let directory = scratch_directory("deploy-envelope-tamper-");
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let envelope = threshold_envelope();

        {
            let mut manager = lmdb_manager(directory.path().to_path_buf());
            let mut storage = runtime
                .block_on(KeyValueDeployStorage::new(&mut manager))
                .unwrap();
            storage.add_envelope_if_absent(envelope.clone()).unwrap();
            let deploy_id =
                DeployIdV6::try_from(envelope.envelope_commitment().unwrap().as_ref()).unwrap();
            let key = storage.envelope_store.encode_key(&deploy_id).unwrap();
            let mut value = storage
                .envelope_store
                .raw_store()
                .get_one(&key)
                .unwrap()
                .unwrap();
            let signature = envelope.selected_signers_v61().unwrap()[0].sig.as_ref();
            let offset = value
                .windows(signature.len())
                .position(|window| window == signature)
                .unwrap();
            value[offset + signature.len() - 1] ^= 1;
            let decoded = storage.envelope_store.decode_value(&value).unwrap();
            assert!(decoded.validate_envelope().is_err());
            storage
                .envelope_store
                .raw_store()
                .put_one(key, value)
                .unwrap();
            drop(storage);
            runtime.block_on(manager.shutdown()).unwrap();
        }

        {
            let mut manager = lmdb_manager(directory.path().to_path_buf());
            let reopened = runtime.block_on(KeyValueDeployStorage::new(&mut manager));
            assert!(matches!(reopened, Err(KvStoreError::InvalidArgument(_))));
            runtime.block_on(manager.shutdown()).unwrap();
        }
    }
}
