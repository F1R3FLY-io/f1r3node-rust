// See block-storage/src/main/scala/coop/rchain/blockstorage/deploy/KeyValueDeployStorage.scala

use std::collections::HashSet;
use std::marker::PhantomData;
use std::sync::Arc;

use crypto::rust::signatures::signed::Signed;
use models::rust::casper::protocol::casper_message::DeployData;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use shared::rust::store::key_value_store::{KeyValueStore, KvStoreError};
use shared::rust::store::key_value_typed_store::KeyValueTypedStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;
use shared::rust::{BitVector, ByteString};

use super::deploy_record_quarantine::{DeployRecordQuarantine, StoredSizes};

pub const DEPLOY_STORAGE_DB: &str = "deploy_storage";
pub const DEPLOY_STORAGE_QUARANTINE_DB: &str = "deploy_storage_quarantine";
pub const REJECTED_DEPLOY_BUFFER_DB: &str = "rejected_deploy_buffer";
pub const REJECTED_DEPLOY_BUFFER_QUARANTINE_DB: &str = "rejected_deploy_buffer_quarantine";
pub const DEPLOY_STORAGE_SEEDS_DB: &str = "deploy_storage_seeds";
pub const REJECTED_DEPLOY_BUFFER_SEEDS_DB: &str = "rejected_deploy_buffer_seeds";

const DEPLOY_STORE_SIZE_METRIC: &str = "deploy.store.size";
const DEPLOY_STORE_BYTES_METRIC: &str = "deploy.store.bytes";
const DEPLOY_STORE_READ_ALL_DECODED_METRIC: &str = "deploy-store.read-all.decoded";

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    serde::Deserialize,
    serde::Serialize
)]
#[serde(rename_all = "kebab-case", deny_unknown_fields, default)]
pub struct DeployPoolLimits {
    pub max_records: usize,
    pub max_bytes: u64,
}

impl Default for DeployPoolLimits {
    fn default() -> Self {
        Self {
            max_records: 512,
            max_bytes: 16 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoolRefusal {
    TooLarge {
        record_bytes: u64,
        limit: u64,
    },
    Full {
        resource: &'static str,
        required: u64,
        limit: u64,
    },
}

impl DeployPoolLimits {
    fn refusal(&self, sizes: &StoredSizes, record_bytes: u64) -> Option<PoolRefusal> {
        if record_bytes > self.max_bytes {
            return Some(PoolRefusal::TooLarge {
                record_bytes,
                limit: self.max_bytes,
            });
        }
        if sizes.records() >= self.max_records {
            return Some(PoolRefusal::Full {
                resource: "records",
                required: sizes.records() as u64 + 1,
                limit: self.max_records as u64,
            });
        }
        let required = sizes.bytes().saturating_add(record_bytes);
        (required > self.max_bytes).then_some(PoolRefusal::Full {
            resource: "bytes",
            required,
            limit: self.max_bytes,
        })
    }
}

pub trait DeployStoreName {
    const NAME: &'static str;
    const QUARANTINE_NAME: &'static str;
    const SEEDS_NAME: &'static str;
    const SIZE_WARNING_THRESHOLD: Option<usize> = None;
}

#[derive(Clone)]
pub struct DeployStorageName;

impl DeployStoreName for DeployStorageName {
    const NAME: &'static str = DEPLOY_STORAGE_DB;
    const QUARANTINE_NAME: &'static str = DEPLOY_STORAGE_QUARANTINE_DB;
    const SEEDS_NAME: &'static str = DEPLOY_STORAGE_SEEDS_DB;
}

#[derive(Clone)]
pub struct RejectedDeployBufferName;

impl DeployStoreName for RejectedDeployBufferName {
    const NAME: &'static str = REJECTED_DEPLOY_BUFFER_DB;
    const QUARANTINE_NAME: &'static str = REJECTED_DEPLOY_BUFFER_QUARANTINE_DB;
    const SEEDS_NAME: &'static str = REJECTED_DEPLOY_BUFFER_SEEDS_DB;
    const SIZE_WARNING_THRESHOLD: Option<usize> = Some(1_000);
}

/// Deploys sharing a name seed create the same unforgeable names, so a store
/// admits at most one of them through its checked insert. `seeds` maps each
/// claimed seed to the sig holding it, and that sig back to the seed, so a
/// removal by sig releases the claim without reading the record. A claim is
/// written before its record and released after its record is deleted, so a
/// claim may briefly name a sig with no record yet; claims whose record never
/// arrived are dropped when the store opens.
#[derive(Clone)]
pub struct KeyValueDeployStore<N: DeployStoreName> {
    pub store: KeyValueTypedStoreImpl<ByteString, Signed<DeployData>>,
    quarantine: DeployRecordQuarantine,
    seeds: Arc<dyn KeyValueStore>,
    _name: PhantomData<N>,
}

pub struct PreparedDeploy {
    key: BitVector,
    value: BitVector,
    sig: ByteString,
    seed: ByteString,
}

pub struct DeployRecordSnapshot {
    key: BitVector,
    value: Option<BitVector>,
    verified: bool,
}

impl DeployRecordSnapshot {
    pub fn is_present(&self) -> bool { self.verified }
}

#[derive(Debug, PartialEq, Eq)]
pub enum PreparedInsertResult {
    Inserted,
    AlreadyPresent,
    Changed,
    /// Another deploy holds this deploy's name seed.
    SeedHeld(ByteString),
    Refused(PoolRefusal),
}

#[derive(Debug)]
pub enum DeployPreparationError {
    InvalidSignature,
    UnsupportedAlgorithm(String),
    Serialization(KvStoreError),
}

impl From<DeployPreparationError> for KvStoreError {
    fn from(error: DeployPreparationError) -> Self {
        match error {
            DeployPreparationError::InvalidSignature => {
                Self::SerializationError("Invalid deploy signature".to_string())
            }
            DeployPreparationError::UnsupportedAlgorithm(algorithm) => {
                Self::SerializationError(format!("Unknown signature algorithm: {algorithm}"))
            }
            DeployPreparationError::Serialization(error) => error,
        }
    }
}

pub type KeyValueDeployStorage = KeyValueDeployStore<DeployStorageName>;
/// Per-node local buffer of deploys rejected during multi-parent merge.
///
/// When the merge algorithm drops a deploy from the canonical merged state,
/// its data is placed here so the block creator can re-propose it in a
/// subsequent block. Each validator maintains its own buffer; there is no
/// cross-validator coordination.
pub type KeyValueRejectedDeployBuffer = KeyValueDeployStore<RejectedDeployBufferName>;

impl<N: DeployStoreName> KeyValueDeployStore<N> {
    pub async fn new(kvm: &mut impl KeyValueStoreManager) -> Result<Self, KvStoreError> {
        let kv_store = kvm.store(N::NAME.to_string()).await?;
        let quarantine_kv_store = kvm.store(N::QUARANTINE_NAME.to_string()).await?;
        let seeds_kv_store = kvm.store(N::SEEDS_NAME.to_string()).await?;
        Self::from_stores(kv_store, quarantine_kv_store, seeds_kv_store)
    }

    pub fn from_stores(
        store: Arc<dyn KeyValueStore>,
        quarantine: Arc<dyn KeyValueStore>,
        seeds: Arc<dyn KeyValueStore>,
    ) -> Result<Self, KvStoreError> {
        let quarantine = DeployRecordQuarantine::new(N::NAME, quarantine);
        let storage = Self {
            store: KeyValueTypedStoreImpl::with_value_codec(
                store,
                super::deploy_codec::encode,
                super::deploy_codec::decode,
            ),
            quarantine,
            seeds,
            _name: PhantomData,
        };
        storage.quarantine.restore_readable(&storage.store)?;
        storage.drop_claims_without_records()?;
        Ok(storage)
    }

    const SEED_HOLDER: u8 = 0;
    const SIG_SEED: u8 = 1;

    fn tagged(tag: u8, bytes: &[u8]) -> ByteString {
        let mut key = Vec::with_capacity(bytes.len() + 1);
        key.push(tag);
        key.extend_from_slice(bytes);
        key
    }

    fn drop_claims_without_records(&self) -> Result<(), KvStoreError> {
        for (key, value) in self.seeds.to_map()? {
            let sig = match key.split_first() {
                Some((&Self::SEED_HOLDER, _)) => value.clone(),
                Some((&Self::SIG_SEED, sig)) => sig.to_vec(),
                _ => {
                    return Err(KvStoreError::InvalidArgument(format!(
                        "{} holds a seed claim key with no known tag",
                        N::SEEDS_NAME
                    )))
                }
            };
            if !self.contains_sig(&sig)? {
                self.seeds.delete_if_value(&key, &value)?;
            }
        }
        Ok(())
    }

    /// The sig holding `seed` here, if any. A holder may be mid-insert, so
    /// its record is not yet guaranteed to exist.
    pub fn seed_holder(&self, seed: &[u8]) -> Result<Option<ByteString>, KvStoreError> {
        self.seeds.get_one(&Self::tagged(Self::SEED_HOLDER, seed))
    }

    /// Claims `seed` for `sig`, returning the other sig holding it, if any.
    fn claim_seed(&self, seed: &[u8], sig: &[u8]) -> Result<Option<ByteString>, KvStoreError> {
        let holder_key = Self::tagged(Self::SEED_HOLDER, seed);
        loop {
            let held = self
                .seeds
                .put_one_if_absent(holder_key.clone(), sig.to_vec())?;
            if !held {
                match self.seeds.get_one(&holder_key)? {
                    Some(holder) if holder == sig => {}
                    Some(holder) => return Ok(Some(holder)),
                    None => continue,
                }
            }
            self.seeds
                .put_one(Self::tagged(Self::SIG_SEED, sig), seed.to_vec())?;
            return Ok(None);
        }
    }

    fn release_seed_of(&self, sig: &[u8]) -> Result<(), KvStoreError> {
        let sig_key = Self::tagged(Self::SIG_SEED, sig);
        if let Some(seed) = self.seeds.get_one(&sig_key)? {
            self.seeds
                .delete_if_value(&Self::tagged(Self::SEED_HOLDER, &seed), &sig.to_vec())?;
            self.seeds.delete_if_value(&sig_key, &seed)?;
        }
        Ok(())
    }

    fn sizes(&self) -> Result<parking_lot::MutexGuard<'_, StoredSizes>, KvStoreError> {
        let mut sizes = self.quarantine.sizes().lock();
        sizes.load(&self.store)?;
        Ok(sizes)
    }

    fn publish(sizes: &StoredSizes) {
        metrics::gauge!(DEPLOY_STORE_SIZE_METRIC, "store" => N::NAME).set(sizes.records() as f64);
        metrics::gauge!(DEPLOY_STORE_BYTES_METRIC, "store" => N::NAME).set(sizes.bytes() as f64);
    }

    pub fn refusal(
        &self,
        deploy: &PreparedDeploy,
        limits: DeployPoolLimits,
    ) -> Result<Option<PoolRefusal>, KvStoreError> {
        Ok(limits.refusal(&*self.sizes()?, deploy.value.len() as u64))
    }

    pub fn add(&self, deploys: Vec<Signed<DeployData>>) -> Result<(), KvStoreError> {
        let prepared = deploys
            .iter()
            .map(|deploy| self.prepare(deploy))
            .collect::<Result<Vec<_>, _>>()?;
        self.add_prepared(prepared)
    }

    pub fn prepare(&self, deploy: &Signed<DeployData>) -> Result<PreparedDeploy, KvStoreError> {
        let value = self.store.encode_value(deploy)?;
        self.prepare_value(deploy, value)
    }

    pub fn prepare_checked(
        &self,
        deploy: &Signed<DeployData>,
    ) -> Result<PreparedDeploy, DeployPreparationError> {
        let value = super::deploy_codec::encode_checked(deploy)?;
        self.prepare_value(deploy, value)
            .map_err(DeployPreparationError::Serialization)
    }

    fn prepare_value(
        &self,
        deploy: &Signed<DeployData>,
        value: BitVector,
    ) -> Result<PreparedDeploy, KvStoreError> {
        Ok(PreparedDeploy {
            key: self.store.encode_key(&deploy.sig.to_vec())?,
            value,
            sig: deploy.sig.to_vec(),
            seed: models::rust::deploy_seed::deploy_name_seed(deploy),
        })
    }

    /// Stores every deploy, including one whose seed another deploy holds:
    /// these are deploys the node already carries, not new submissions.
    pub fn add_prepared(&self, deploys: Vec<PreparedDeploy>) -> Result<(), KvStoreError> {
        for deploy in &deploys {
            self.claim_seed(&deploy.seed, &deploy.sig)?;
        }
        let records: Vec<_> = deploys
            .into_iter()
            .map(|deploy| (deploy.key, deploy.value))
            .collect();
        let sized: Vec<_> = records
            .iter()
            .map(|(key, value)| (key.clone(), value.len() as u64))
            .collect();
        let mut sizes = self.sizes()?;
        self.store.raw_store().put(records)?;
        for (key, len) in sized {
            sizes.insert(key, len);
        }
        Self::publish(&sizes);
        Ok(())
    }

    pub fn add_prepared_if_absent(&self, deploy: PreparedDeploy) -> Result<bool, KvStoreError> {
        if self.quarantine.contains(&self.store, &deploy.key)? {
            return Ok(false);
        }
        self.claim_seed(&deploy.seed, &deploy.sig)?;
        let inserted = {
            let mut sizes = self.sizes()?;
            let len = deploy.value.len() as u64;
            let inserted = self
                .store
                .raw_store()
                .put_one_if_absent(deploy.key.clone(), deploy.value)?;
            if inserted {
                sizes.insert(deploy.key.clone(), len);
                Self::publish(&sizes);
            }
            inserted
        };
        if !inserted && !self.quarantine.contains(&self.store, &deploy.key)? {
            return Err(KvStoreError::InvalidArgument(
                "Deploy insertion was blocked by an invalid or concurrently changed record"
                    .to_string(),
            ));
        }
        Ok(inserted)
    }

    /// Atomically insert a deploy by signature, returning false when it already exists.
    pub fn add_if_absent(&self, deploy: Signed<DeployData>) -> Result<bool, KvStoreError> {
        self.add_prepared_if_absent(self.prepare(&deploy)?)
    }

    pub fn contains_sig(&self, sig: &[u8]) -> Result<bool, KvStoreError> {
        self.quarantine
            .contains(&self.store, &self.store.encode_key(&sig.to_vec())?)
    }

    pub fn inspect_sig(&self, sig: &[u8]) -> Result<DeployRecordSnapshot, KvStoreError> {
        let key = self.store.encode_key(&sig.to_vec())?;
        let (value, verified) = self.quarantine.inspect(&self.store, &key)?;
        Ok(DeployRecordSnapshot {
            key,
            value,
            verified,
        })
    }

    pub fn matches_snapshot(&self, snapshot: &DeployRecordSnapshot) -> Result<bool, KvStoreError> {
        Ok(self.store.raw_store().get_one(&snapshot.key)? == snapshot.value)
    }

    pub fn add_prepared_if_unchanged(
        &self,
        deploy: &PreparedDeploy,
        snapshot: &DeployRecordSnapshot,
        limits: DeployPoolLimits,
    ) -> Result<PreparedInsertResult, KvStoreError> {
        if deploy.key != snapshot.key {
            return Err(KvStoreError::InvalidArgument(
                "Deploy snapshot belongs to a different signature".to_string(),
            ));
        }
        let mut sizes = self.sizes()?;
        if !self.matches_snapshot(snapshot)? {
            return Ok(PreparedInsertResult::Changed);
        }
        if snapshot.verified {
            return Ok(PreparedInsertResult::AlreadyPresent);
        }
        if snapshot.value.is_some() {
            return Err(KvStoreError::InvalidArgument(
                "Deploy insertion was blocked by an invalid record".to_string(),
            ));
        }
        let len = deploy.value.len() as u64;
        if let Some(refusal) = limits.refusal(&sizes, len) {
            return Ok(PreparedInsertResult::Refused(refusal));
        }
        if let Some(holder) = self.claim_seed(&deploy.seed, &deploy.sig)? {
            return Ok(PreparedInsertResult::SeedHeld(holder));
        }
        let inserted = self
            .store
            .raw_store()
            .put_one_if_absent(deploy.key.clone(), deploy.value.clone())?;
        if !inserted {
            return Ok(PreparedInsertResult::Changed);
        }
        sizes.insert(deploy.key.clone(), len);
        Self::publish(&sizes);
        Ok(PreparedInsertResult::Inserted)
    }

    /// The signature of every stored deploy, read from the keys without
    /// decoding a deploy.
    pub fn sigs(&self) -> Result<Vec<ByteString>, KvStoreError> {
        self.quarantine.sigs(&self.store)
    }

    pub fn remove(&self, deploys: Vec<Signed<DeployData>>) -> Result<(), KvStoreError> {
        let keys = deploys
            .iter()
            .map(|deploy| self.store.encode_key(&deploy.sig.to_vec()))
            .collect::<Result<Vec<_>, _>>()?;
        self.delete_keys(keys)?;
        for deploy in &deploys {
            self.release_seed_of(&deploy.sig)?;
        }
        Ok(())
    }

    pub fn remove_by_sig(&self, sig: &[u8]) -> Result<bool, KvStoreError> {
        let key: ByteString = sig.to_vec();
        let key = self.store.encode_key(&key)?;
        let removed = self.delete_keys(vec![key])? > 0;
        self.release_seed_of(sig)?;
        Ok(removed)
    }

    fn delete_keys(&self, keys: Vec<BitVector>) -> Result<usize, KvStoreError> {
        let mut sizes = self.sizes()?;
        let removed = self.store.raw_store().delete(keys.clone())?;
        for key in &keys {
            sizes.remove(key);
        }
        Self::publish(&sizes);
        Ok(removed)
    }

    pub fn any<F>(&self, predicate: F) -> Result<bool, KvStoreError>
    where F: FnMut(&Signed<DeployData>) -> Result<bool, KvStoreError> {
        self.quarantine.any(&self.store, predicate)
    }

    pub fn read_all(&self) -> Result<HashSet<Signed<DeployData>>, KvStoreError> {
        let all = self.quarantine.read_all(&self.store)?;
        metrics::gauge!(DEPLOY_STORE_SIZE_METRIC, "store" => N::NAME).set(all.len() as f64);
        metrics::counter!(DEPLOY_STORE_READ_ALL_DECODED_METRIC, "store" => N::NAME)
            .increment(all.len() as u64);
        if let Some(limit) = N::SIZE_WARNING_THRESHOLD {
            if all.len() > limit {
                tracing::warn!(
                    store = N::NAME,
                    size = all.len(),
                    limit,
                    "deploy store above size warning threshold"
                );
            }
        }
        Ok(all)
    }

    /// Check if the storage contains any pending deploys. O(1) time and space.
    pub fn non_empty(&self) -> Result<bool, KvStoreError> { self.store.non_empty() }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Barrier};

    use crypto::rust::private_key::PrivateKey;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use metrics_util::debugging::{DebugValue, DebuggingRecorder};
    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use shared::rust::store::key_value_store::KeyValueStore;

    use super::*;

    fn deploy(time_stamp: i64) -> Signed<DeployData> {
        Signed::create(
            DeployData {
                term: "Nil".to_string(),
                time_stamp,
                phlo_price: (1).into(),
                phlo_limit: 100_000,
                valid_after_block_number: 0,
                shard_id: "root".to_string(),
                expiration_timestamp: None,
                parameters: Vec::new(),
            },
            Box::new(Secp256k1),
            PrivateKey::from_bytes(&[1; 32]),
        )
        .unwrap()
    }

    /// A distinct deploy with `deploy(time_stamp)`'s name seed.
    fn seed_twin(time_stamp: i64) -> Signed<DeployData> {
        let mut twin = deploy(time_stamp);
        twin.data.phlo_limit += 1;
        Signed::create(
            twin.data,
            Box::new(Secp256k1),
            PrivateKey::from_bytes(&[1; 32]),
        )
        .unwrap()
    }

    fn seed(deploy: &Signed<DeployData>) -> ByteString {
        models::rust::deploy_seed::deploy_name_seed(deploy)
    }

    #[tokio::test]
    async fn storage_round_trips_add_contains_read_and_remove() {
        let mut kvm = InMemoryStoreManager::new();
        let storage = KeyValueDeployStorage::new(&mut kvm).await.unwrap();
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
        assert_eq!(
            storage.seed_holder(&seed(&d1)).unwrap(),
            Some(d1.sig.to_vec())
        );
        assert_eq!(
            storage.seed_holder(&seed(&d2)).unwrap(),
            Some(d2.sig.to_vec())
        );

        assert!(storage.any(|d| Ok(d.data.time_stamp == 2)).unwrap());
        assert!(!storage.any(|d| Ok(d.data.time_stamp == 99)).unwrap());

        storage.remove(vec![d2.clone()]).unwrap();
        assert_eq!(storage.read_all().unwrap(), HashSet::from([d1.clone()]));
        assert_eq!(storage.seed_holder(&seed(&d2)).unwrap(), None);

        assert!(storage.remove_by_sig(&d1.sig).unwrap());
        assert!(!storage.remove_by_sig(&d1.sig).unwrap());
        assert!(!storage.non_empty().unwrap());
        assert_eq!(storage.seed_holder(&seed(&d1)).unwrap(), None);
    }

    async fn buffer() -> KeyValueRejectedDeployBuffer {
        let mut kvm = InMemoryStoreManager::new();
        KeyValueRejectedDeployBuffer::new(&mut kvm).await.unwrap()
    }

    #[tokio::test]
    async fn add_read_all_and_non_empty() {
        let buffer = buffer().await;
        assert!(!buffer.non_empty().unwrap());

        let (d1, d2) = (deploy(1), deploy(2));
        buffer.add(vec![d1.clone(), d2.clone()]).unwrap();

        assert!(buffer.non_empty().unwrap());
        assert_eq!(buffer.read_all().unwrap(), HashSet::from([d1, d2]));
    }

    #[tokio::test]
    async fn remove_deletes_listed_deploys() {
        let buffer = buffer().await;
        let (d1, d2) = (deploy(1), deploy(2));
        buffer.add(vec![d1.clone(), d2.clone()]).unwrap();

        buffer.remove(vec![d2]).unwrap();
        assert_eq!(buffer.read_all().unwrap(), HashSet::from([d1]));
    }

    #[tokio::test]
    async fn remove_by_sig_reports_presence() {
        let buffer = buffer().await;
        let d1 = deploy(1);
        buffer.add(vec![d1.clone()]).unwrap();

        assert!(buffer.remove_by_sig(&d1.sig).unwrap());
        assert!(!buffer.remove_by_sig(&d1.sig).unwrap());
        assert!(!buffer.contains_sig(&d1.sig).unwrap());
        assert!(!buffer.non_empty().unwrap());
    }

    #[test]
    fn concurrent_signature_removals_report_one_actual_delete() {
        let storage = KeyValueDeployStorage::from_stores(
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
        )
        .unwrap();
        let valid = deploy(1);
        storage.add(vec![valid.clone()]).unwrap();
        let barrier = Arc::new(Barrier::new(16));
        let handles: Vec<_> = (0..16)
            .map(|_| {
                let storage = storage.clone();
                let sig = valid.sig.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    storage.remove_by_sig(&sig).unwrap()
                })
            })
            .collect();
        assert_eq!(
            handles
                .into_iter()
                .map(|handle| usize::from(handle.join().unwrap()))
                .sum::<usize>(),
            1
        );
        assert!(!storage.contains_sig(&valid.sig).unwrap());
    }

    #[tokio::test]
    async fn read_all_reports_the_size_of_each_store() {
        let mut kvm = InMemoryStoreManager::new();
        let storage = KeyValueDeployStorage::new(&mut kvm).await.unwrap();
        let buffer = KeyValueRejectedDeployBuffer::new(&mut kvm).await.unwrap();
        storage.add(vec![deploy(1), deploy(2)]).unwrap();
        buffer.add(vec![deploy(3)]).unwrap();

        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        metrics::with_local_recorder(&recorder, || {
            storage.read_all().unwrap();
            buffer.read_all().unwrap();
        });

        let sizes: HashMap<String, f64> = snapshotter
            .snapshot()
            .into_vec()
            .into_iter()
            .filter(|(key, ..)| key.key().name() == DEPLOY_STORE_SIZE_METRIC)
            .filter_map(|(key, _, _, value)| match value {
                DebugValue::Gauge(size) => key
                    .key()
                    .labels()
                    .find(|label| label.key() == "store")
                    .map(|label| (label.value().to_string(), size.into_inner())),
                _ => None,
            })
            .collect();
        assert_eq!(
            sizes,
            HashMap::from([
                ("deploy_storage".to_string(), 2.0),
                ("rejected_deploy_buffer".to_string(), 1.0),
            ])
        );
    }

    #[tokio::test]
    async fn writes_publish_the_totals_of_every_stored_record_after_a_reopen() {
        let mut kvm = InMemoryStoreManager::new();
        let storage = KeyValueDeployStorage::new(&mut kvm).await.unwrap();
        let buffer = KeyValueRejectedDeployBuffer::new(&mut kvm).await.unwrap();
        storage.add(vec![deploy(1), deploy(2)]).unwrap();
        buffer.add(vec![deploy(3)]).unwrap();
        let storage = KeyValueDeployStorage::new(&mut kvm).await.unwrap();
        let buffer = KeyValueRejectedDeployBuffer::new(&mut kvm).await.unwrap();
        let bytes =
            |store: &KeyValueDeployStorage, ts| store.prepare(&deploy(ts)).unwrap().value.len();
        let buffer_bytes = buffer.prepare(&deploy(3)).unwrap().value.len()
            + buffer.prepare(&deploy(4)).unwrap().value.len();

        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        metrics::with_local_recorder(&recorder, || {
            storage.add(vec![deploy(5)]).unwrap();
            buffer.add(vec![deploy(4)]).unwrap();
        });

        let gauges: HashMap<(String, String), f64> = snapshotter
            .snapshot()
            .into_vec()
            .into_iter()
            .filter_map(|(key, _, _, value)| match value {
                DebugValue::Gauge(value) => key
                    .key()
                    .labels()
                    .find(|label| label.key() == "store")
                    .map(|label| {
                        (
                            (key.key().name().to_string(), label.value().to_string()),
                            value.into_inner(),
                        )
                    }),
                _ => None,
            })
            .collect();
        let gauge = |name: &str, store: &str| gauges[&(name.to_string(), store.to_string())];
        assert_eq!(gauge(DEPLOY_STORE_SIZE_METRIC, "deploy_storage"), 3.0);
        assert_eq!(
            gauge(DEPLOY_STORE_BYTES_METRIC, "deploy_storage"),
            (bytes(&storage, 1) + bytes(&storage, 2) + bytes(&storage, 5)) as f64
        );
        assert_eq!(
            gauge(DEPLOY_STORE_SIZE_METRIC, "rejected_deploy_buffer"),
            2.0
        );
        assert_eq!(
            gauge(DEPLOY_STORE_BYTES_METRIC, "rejected_deploy_buffer"),
            buffer_bytes as f64
        );
    }

    #[tokio::test]
    async fn read_all_counts_each_decoded_deploy_of_each_store() {
        let mut kvm = InMemoryStoreManager::new();
        let storage = KeyValueDeployStorage::new(&mut kvm).await.unwrap();
        let buffer = KeyValueRejectedDeployBuffer::new(&mut kvm).await.unwrap();
        storage.add(vec![deploy(1), deploy(2)]).unwrap();
        buffer.add(vec![deploy(3)]).unwrap();

        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        metrics::with_local_recorder(&recorder, || {
            storage.read_all().unwrap();
            storage.read_all().unwrap();
            buffer.read_all().unwrap();
        });

        let decoded: HashMap<String, u64> = snapshotter
            .snapshot()
            .into_vec()
            .into_iter()
            .filter(|(key, ..)| key.key().name() == DEPLOY_STORE_READ_ALL_DECODED_METRIC)
            .filter_map(|(key, _, _, value)| match value {
                DebugValue::Counter(count) => key
                    .key()
                    .labels()
                    .find(|label| label.key() == "store")
                    .map(|label| (label.value().to_string(), count)),
                _ => None,
            })
            .collect();
        assert_eq!(
            decoded,
            HashMap::from([
                ("deploy_storage".to_string(), 4),
                ("rejected_deploy_buffer".to_string(), 1),
            ])
        );
    }

    #[tokio::test]
    async fn add_if_absent_reports_the_duplicate() {
        let mut kvm = InMemoryStoreManager::new();
        let storage = KeyValueDeployStorage::new(&mut kvm).await.unwrap();
        let d1 = deploy(1);
        assert!(storage.add_if_absent(d1.clone()).unwrap());
        assert!(!storage.add_if_absent(d1).unwrap());
        assert_eq!(storage.read_all().unwrap().len(), 1);
    }

    #[test]
    fn prepared_deploy_keeps_the_verified_bytes_when_the_source_changes() {
        let storage = KeyValueDeployStorage::from_stores(
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
        )
        .unwrap();
        let mut source = deploy(1);
        let expected = source.clone();
        let prepared = storage.prepare(&source).unwrap();
        assert!(!storage.non_empty().unwrap());

        source.data.phlo_limit += 1;
        assert!(storage.prepare(&source).is_err());
        assert!(storage.add_prepared_if_absent(prepared).unwrap());
        assert_eq!(storage.read_all().unwrap(), HashSet::from([expected]));
    }

    #[test]
    fn checked_insertion_rejects_an_outdated_absence_snapshot_and_a_seed_twin() {
        let live: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let quarantine: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let seeds: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let storage =
            KeyValueDeployStorage::from_stores(live.clone(), quarantine.clone(), seeds.clone())
                .unwrap();
        let candidate = deploy(1);
        let absent = storage.inspect_sig(&candidate.sig).unwrap();
        assert!(!absent.is_present());
        storage.add(vec![candidate.clone()]).unwrap();
        assert_eq!(
            storage
                .add_prepared_if_unchanged(
                    &storage.prepare(&candidate).unwrap(),
                    &absent,
                    DeployPoolLimits::default()
                )
                .unwrap(),
            PreparedInsertResult::Changed
        );
        let present = storage.inspect_sig(&candidate.sig).unwrap();
        assert!(present.is_present());
        assert_eq!(
            storage
                .add_prepared_if_unchanged(
                    &storage.prepare(&candidate).unwrap(),
                    &present,
                    DeployPoolLimits::default()
                )
                .unwrap(),
            PreparedInsertResult::AlreadyPresent
        );

        let twin = seed_twin(1);
        assert_ne!(twin.sig, candidate.sig);
        let twin_absent = storage.inspect_sig(&twin.sig).unwrap();
        assert_eq!(
            storage
                .add_prepared_if_unchanged(
                    &storage.prepare(&twin).unwrap(),
                    &twin_absent,
                    DeployPoolLimits::default()
                )
                .unwrap(),
            PreparedInsertResult::SeedHeld(candidate.sig.to_vec())
        );
        assert!(!storage.contains_sig(&twin.sig).unwrap());

        storage.remove(vec![candidate.clone()]).unwrap();
        assert_eq!(
            storage
                .add_prepared_if_unchanged(
                    &storage.prepare(&twin).unwrap(),
                    &twin_absent,
                    DeployPoolLimits::default()
                )
                .unwrap(),
            PreparedInsertResult::Inserted
        );

        let unrecorded = deploy(2);
        seeds
            .put_one(
                KeyValueDeployStorage::tagged(
                    KeyValueDeployStorage::SEED_HOLDER,
                    &seed(&unrecorded),
                ),
                unrecorded.sig.to_vec(),
            )
            .unwrap();
        seeds
            .put_one(
                KeyValueDeployStorage::tagged(KeyValueDeployStorage::SIG_SEED, &unrecorded.sig),
                seed(&unrecorded),
            )
            .unwrap();
        let reopened = KeyValueDeployStorage::from_stores(live, quarantine, seeds.clone()).unwrap();
        assert_eq!(reopened.seed_holder(&seed(&unrecorded)).unwrap(), None);
        assert_eq!(
            seeds
                .get_one(&KeyValueDeployStorage::tagged(
                    KeyValueDeployStorage::SIG_SEED,
                    &unrecorded.sig
                ))
                .unwrap(),
            None
        );
        assert_eq!(
            reopened.seed_holder(&seed(&twin)).unwrap(),
            Some(twin.sig.to_vec())
        );
    }

    #[test]
    fn checked_insertion_requires_a_fresh_snapshot_after_quarantining() {
        let live: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let quarantine: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let storage = KeyValueDeployStorage::from_stores(
            live.clone(),
            quarantine.clone(),
            Arc::new(InMemoryKeyValueStore::new()),
        )
        .unwrap();
        let candidate = deploy(1);
        let key = storage.store.encode_key(&candidate.sig.to_vec()).unwrap();
        live.put_one(key.clone(), vec![0xff]).unwrap();
        let damaged = storage.inspect_sig(&candidate.sig).unwrap();
        assert!(!damaged.is_present());
        assert_eq!(
            storage
                .add_prepared_if_unchanged(
                    &storage.prepare(&candidate).unwrap(),
                    &damaged,
                    DeployPoolLimits::default()
                )
                .unwrap(),
            PreparedInsertResult::Changed
        );
        let absent = storage.inspect_sig(&candidate.sig).unwrap();
        assert_eq!(
            storage
                .add_prepared_if_unchanged(
                    &storage.prepare(&candidate).unwrap(),
                    &absent,
                    DeployPoolLimits::default()
                )
                .unwrap(),
            PreparedInsertResult::Inserted
        );
        assert_eq!(quarantine.get_one(&key).unwrap(), Some(vec![0xff]));
        assert_eq!(storage.read_all().unwrap(), HashSet::from([candidate]));
    }

    #[test]
    fn checked_insertion_does_not_decode_an_already_inspected_duplicate() {
        let mut storage = KeyValueDeployStorage::from_stores(
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
        )
        .unwrap();
        let candidate = deploy(1);
        storage.add(vec![candidate.clone()]).unwrap();
        let present = storage.inspect_sig(&candidate.sig).unwrap();
        let prepared = storage.prepare(&candidate).unwrap();
        storage.store = KeyValueTypedStoreImpl::with_value_codec(
            storage.store.raw_store().clone(),
            super::super::deploy_codec::encode,
            |_| panic!("A checked insertion must not decode or verify a record"),
        );
        assert_eq!(
            storage
                .add_prepared_if_unchanged(&prepared, &present, DeployPoolLimits::default())
                .unwrap(),
            PreparedInsertResult::AlreadyPresent
        );
    }

    #[test]
    fn prepared_deploy_is_encoded_once_and_inserted_once_across_handles() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        static ENCODED: AtomicUsize = AtomicUsize::new(0);
        fn encode(deploy: &Signed<DeployData>) -> Result<BitVector, KvStoreError> {
            ENCODED.fetch_add(1, Ordering::SeqCst);
            super::super::deploy_codec::encode(deploy)
        }

        let mut storage = KeyValueDeployStorage::from_stores(
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
        )
        .unwrap();
        storage.store = KeyValueTypedStoreImpl::with_value_codec(
            storage.store.raw_store().clone(),
            encode,
            super::super::deploy_codec::decode,
        );
        let source = deploy(1);
        let prepared = (0..8)
            .map(|_| storage.prepare(&source).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(ENCODED.load(Ordering::SeqCst), 8);
        assert!(!storage.non_empty().unwrap());
        let barrier = Arc::new(Barrier::new(8));
        let handles = prepared
            .into_iter()
            .map(|prepared| {
                let storage = storage.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    storage.add_prepared_if_absent(prepared).unwrap()
                })
            })
            .collect::<Vec<_>>();
        let inserted = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .filter(|inserted| *inserted)
            .count();
        assert_eq!(inserted, 1);
        assert_eq!(ENCODED.load(Ordering::SeqCst), 8);
        assert_eq!(storage.read_all().unwrap(), HashSet::from([source]));
    }

    #[tokio::test]
    async fn the_two_stores_open_separate_lmdb_stores() {
        let mut kvm = InMemoryStoreManager::new();
        let storage = KeyValueDeployStorage::new(&mut kvm).await.unwrap();
        let buffer = KeyValueRejectedDeployBuffer::new(&mut kvm).await.unwrap();
        storage.add(vec![deploy(1)]).unwrap();
        assert!(storage.non_empty().unwrap());
        assert!(!buffer.non_empty().unwrap());
    }

    #[test]
    fn add_if_absent_is_atomic_across_storage_handles() {
        let store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let storage = KeyValueDeployStorage::from_stores(
            store,
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
        )
        .unwrap();
        let deploy = Signed::create(
            DeployData {
                term: "Nil".to_string(),
                time_stamp: 1,
                phlo_price: (1).into(),
                phlo_limit: 100_000,
                valid_after_block_number: 0,
                shard_id: "root".to_string(),
                expiration_timestamp: None,
                parameters: Vec::new(),
            },
            Box::new(Secp256k1),
            PrivateKey::from_bytes(&[1; 32]),
        )
        .unwrap();
        let barrier = Arc::new(Barrier::new(32));

        let handles = (0..32)
            .map(|_| {
                let storage = storage.clone();
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

    fn in_memory_storage() -> KeyValueDeployStorage {
        KeyValueDeployStorage::from_stores(
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
        )
        .unwrap()
    }

    fn admit(
        storage: &KeyValueDeployStorage,
        candidate: &Signed<DeployData>,
        limits: DeployPoolLimits,
    ) -> PreparedInsertResult {
        let snapshot = storage.inspect_sig(&candidate.sig).unwrap();
        storage
            .add_prepared_if_unchanged(&storage.prepare(candidate).unwrap(), &snapshot, limits)
            .unwrap()
    }

    fn totals(storage: &KeyValueDeployStorage) -> (usize, u64) {
        let sizes = storage.sizes().unwrap();
        (sizes.records(), sizes.bytes())
    }

    fn record_bytes(storage: &KeyValueDeployStorage, candidate: &Signed<DeployData>) -> u64 {
        storage.prepare(candidate).unwrap().value.len() as u64
    }

    #[test]
    fn totals_follow_inserts_overwrites_and_deletions() {
        let storage = in_memory_storage();
        let (d1, d2) = (deploy(1), deploy(2));
        let (b1, b2) = (record_bytes(&storage, &d1), record_bytes(&storage, &d2));
        storage.add(vec![d1.clone(), d2.clone()]).unwrap();
        assert_eq!(totals(&storage), (2, b1 + b2));
        storage.add(vec![d1.clone()]).unwrap();
        assert_eq!(totals(&storage), (2, b1 + b2));
        assert!(storage.remove_by_sig(&d1.sig).unwrap());
        assert!(!storage.remove_by_sig(&d1.sig).unwrap());
        assert_eq!(totals(&storage), (1, b2));
        storage.remove(vec![d2]).unwrap();
        assert_eq!(totals(&storage), (0, 0));
    }

    #[test]
    fn the_record_limit_refuses_the_next_deploy_and_a_duplicate_comes_first() {
        let storage = in_memory_storage();
        let limits = DeployPoolLimits {
            max_records: 2,
            max_bytes: u64::MAX,
        };
        assert_eq!(
            admit(&storage, &deploy(1), limits),
            PreparedInsertResult::Inserted
        );
        assert_eq!(
            admit(&storage, &deploy(2), limits),
            PreparedInsertResult::Inserted
        );
        assert_eq!(
            admit(&storage, &deploy(3), limits),
            PreparedInsertResult::Refused(PoolRefusal::Full {
                resource: "records",
                required: 3,
                limit: 2,
            })
        );
        assert_eq!(
            admit(&storage, &deploy(1), limits),
            PreparedInsertResult::AlreadyPresent
        );
        assert!(!storage.contains_sig(&deploy(3).sig).unwrap());
        assert!(storage.remove_by_sig(&deploy(1).sig).unwrap());
        assert_eq!(
            admit(&storage, &deploy(3), limits),
            PreparedInsertResult::Inserted
        );
    }

    #[test]
    fn the_byte_limit_refuses_the_total_and_an_oversized_record_even_when_empty() {
        let storage = in_memory_storage();
        let one = record_bytes(&storage, &deploy(1));
        let limits = DeployPoolLimits {
            max_records: 128,
            max_bytes: one,
        };
        assert_eq!(
            admit(&storage, &deploy(1), DeployPoolLimits {
                max_bytes: one - 1,
                ..limits
            }),
            PreparedInsertResult::Refused(PoolRefusal::TooLarge {
                record_bytes: one,
                limit: one - 1,
            })
        );
        assert_eq!(
            admit(&storage, &deploy(1), limits),
            PreparedInsertResult::Inserted
        );
        assert!(matches!(
            admit(&storage, &deploy(2), limits),
            PreparedInsertResult::Refused(PoolRefusal::Full {
                resource: "bytes",
                ..
            })
        ));
    }

    #[test]
    fn concurrent_submissions_admit_exactly_one_into_the_last_slot() {
        let storage = in_memory_storage();
        let limits = DeployPoolLimits {
            max_records: 1,
            max_bytes: u64::MAX,
        };
        let barrier = Arc::new(Barrier::new(16));
        let handles = (0..16)
            .map(|index| {
                let storage = storage.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let candidate = deploy(100 + index);
                    barrier.wait();
                    admit(&storage, &candidate, limits)
                })
            })
            .collect::<Vec<_>>();
        let results: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        assert_eq!(
            results
                .iter()
                .filter(|result| **result == PreparedInsertResult::Inserted)
                .count(),
            1
        );
        assert!(results.iter().all(|result| matches!(
            result,
            PreparedInsertResult::Inserted | PreparedInsertResult::Refused(_)
        )));
        assert_eq!(totals(&storage).0, 1);
    }

    #[tokio::test]
    async fn a_reopened_store_counts_every_stored_record_and_refuses_above_a_lowered_limit() {
        let mut kvm = InMemoryStoreManager::new();
        let storage = KeyValueDeployStorage::new(&mut kvm).await.unwrap();
        storage.add(vec![deploy(1), deploy(2), deploy(3)]).unwrap();
        let expected = totals(&storage);
        let reopened = KeyValueDeployStorage::new(&mut kvm).await.unwrap();
        assert_eq!(totals(&reopened), expected);
        let lowered = DeployPoolLimits {
            max_records: 2,
            max_bytes: u64::MAX,
        };
        assert!(matches!(
            admit(&reopened, &deploy(4), lowered),
            PreparedInsertResult::Refused(PoolRefusal::Full {
                resource: "records",
                ..
            })
        ));
        assert_eq!(reopened.read_all().unwrap().len(), 3);
    }

    #[test]
    fn a_quarantined_record_releases_its_charge() {
        let live: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let seeds: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let storage = KeyValueDeployStorage::from_stores(
            live.clone(),
            Arc::new(InMemoryKeyValueStore::new()),
            seeds.clone(),
        )
        .unwrap();
        storage.add(vec![deploy(1)]).unwrap();
        let damaged = deploy(2);
        let key = storage.store.encode_key(&damaged.sig.to_vec()).unwrap();
        let reopened = KeyValueDeployStorage::from_stores(
            live.clone(),
            Arc::new(InMemoryKeyValueStore::new()),
            seeds.clone(),
        )
        .unwrap();
        live.put_one(key, vec![0xff]).unwrap();
        let counted =
            KeyValueDeployStorage::from_stores(live, Arc::new(InMemoryKeyValueStore::new()), seeds)
                .unwrap();
        assert_eq!(totals(&counted).0, 2);
        assert_eq!(counted.read_all().unwrap(), HashSet::from([deploy(1)]));
        assert_eq!(totals(&counted), totals(&reopened));
    }
}
