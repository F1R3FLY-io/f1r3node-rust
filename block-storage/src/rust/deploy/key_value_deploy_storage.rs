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
use shared::rust::store::lmdb_key_value_store::LmdbKeyValueStore;
use shared::rust::{BitVector, ByteString};

use super::deploy_record_quarantine::{DeployRecordQuarantine, VerifiedMetadata};

pub const DEPLOY_STORAGE_DB: &str = "deploy_storage";
pub const DEPLOY_STORAGE_QUARANTINE_DB: &str = "deploy_storage_quarantine";
pub const REJECTED_DEPLOY_BUFFER_DB: &str = "rejected_deploy_buffer";
pub const REJECTED_DEPLOY_BUFFER_QUARANTINE_DB: &str = "rejected_deploy_buffer_quarantine";

const DEPLOY_STORE_SIZE_METRIC: &str = "deploy.store.size";
const DEPLOY_STORE_READ_ALL_DECODED_METRIC: &str = "deploy-store.read-all.decoded";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeployMetadata {
    pub signature: Vec<u8>,
    pub valid_after_block_number: i64,
    pub timestamp: i64,
    pub expiration_timestamp: Option<i64>,
}

impl From<&Signed<DeployData>> for DeployMetadata {
    fn from(deploy: &Signed<DeployData>) -> Self {
        Self {
            signature: deploy.sig.to_vec(),
            valid_after_block_number: deploy.data.valid_after_block_number,
            timestamp: deploy.data.time_stamp,
            expiration_timestamp: deploy.data.expiration_timestamp,
        }
    }
}

pub fn store_revision(store: &Arc<dyn KeyValueStore>) -> Option<usize> {
    store
        .as_any()
        .downcast_ref::<LmdbKeyValueStore>()
        .and_then(LmdbKeyValueStore::revision)
}

#[derive(Clone, Default)]
pub struct MetadataSnapshot {
    pub entries: imbl::HashMap<Vec<u8>, DeployMetadata>,
    pub generation: u64,
    pub parent_generation: Option<u64>,
    pub additions: Vec<DeployMetadata>,
    pub removals: Vec<Vec<u8>>,
}

#[derive(Default)]
pub(super) struct MetadataCache {
    pub revision: Option<usize>,
    pub initialized: bool,
    pub snapshot: MetadataSnapshot,
    verified: std::collections::HashMap<ByteString, VerifiedMetadata>,
}

impl MetadataCache {
    fn compact_verified(&mut self) {
        let len = self.verified.len();
        if self.verified.is_empty() {
            self.verified.shrink_to_fit();
        } else if self.verified.capacity() > len.saturating_mul(4).max(128) {
            self.verified.shrink_to(len.saturating_mul(2));
        }
    }

    pub fn update(
        &mut self,
        before: Option<usize>,
        after: Option<usize>,
        writes: usize,
        additions: Vec<DeployMetadata>,
        removals: Vec<Vec<u8>>,
    ) {
        if writes > 0 {
            for deploy in &additions {
                self.verified.remove(&deploy.signature);
            }
            for signature in &removals {
                self.verified.remove(signature);
            }
            self.compact_verified();
        }
        let trusted = self.initialized
            && before.is_some()
            && before == self.revision
            && before.and_then(|id| id.checked_add(writes)) == after;
        self.revision = None;
        self.snapshot.parent_generation = None;
        self.snapshot.additions.clear();
        self.snapshot.removals.clear();
        if trusted {
            self.revision = after;
            if writes == 0 {
                return;
            }
            self.snapshot.parent_generation = Some(self.snapshot.generation);
            for deploy in &additions {
                self.snapshot
                    .entries
                    .insert(deploy.signature.clone(), deploy.clone());
            }
            for signature in &removals {
                self.snapshot.entries.remove(signature);
            }
            self.snapshot.additions = additions;
            self.snapshot.removals = removals;
        }
        self.snapshot.generation = self.snapshot.generation.wrapping_add(1);
    }
}

pub trait DeployStoreName {
    const NAME: &'static str;
    const QUARANTINE_NAME: &'static str;
    const SIZE_WARNING_THRESHOLD: Option<usize> = None;
}

#[derive(Clone)]
pub struct DeployStorageName;

impl DeployStoreName for DeployStorageName {
    const NAME: &'static str = DEPLOY_STORAGE_DB;
    const QUARANTINE_NAME: &'static str = DEPLOY_STORAGE_QUARANTINE_DB;
}

#[derive(Clone)]
pub struct RejectedDeployBufferName;

impl DeployStoreName for RejectedDeployBufferName {
    const NAME: &'static str = REJECTED_DEPLOY_BUFFER_DB;
    const QUARANTINE_NAME: &'static str = REJECTED_DEPLOY_BUFFER_QUARANTINE_DB;
    const SIZE_WARNING_THRESHOLD: Option<usize> = Some(1_000);
}

#[derive(Clone)]
pub struct KeyValueDeployStore<N: DeployStoreName> {
    pub store: KeyValueTypedStoreImpl<ByteString, Signed<DeployData>>,
    quarantine: DeployRecordQuarantine,
    metadata: Arc<parking_lot::Mutex<MetadataCache>>,
    _name: PhantomData<N>,
}

pub struct PreparedDeploy {
    key: BitVector,
    value: BitVector,
    metadata: DeployMetadata,
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
        Self::from_stores(kv_store, quarantine_kv_store)
    }

    pub fn from_stores(
        store: Arc<dyn KeyValueStore>,
        quarantine: Arc<dyn KeyValueStore>,
    ) -> Result<Self, KvStoreError> {
        let quarantine = DeployRecordQuarantine::new(N::NAME, quarantine);
        let storage = Self {
            store: KeyValueTypedStoreImpl::with_value_codec(
                store,
                super::deploy_codec::encode,
                super::deploy_codec::decode,
            ),
            quarantine,
            metadata: Arc::default(),
            _name: PhantomData,
        };
        storage.quarantine.restore_readable(&storage.store)?;
        Ok(storage)
    }

    pub fn metadata_snapshot(&self) -> Result<MetadataSnapshot, KvStoreError> {
        let mut cache = self.metadata.lock();
        let before = store_revision(self.store.raw_store());
        if cache.initialized && before.is_some() && cache.revision == before {
            return Ok(cache.snapshot.clone());
        }
        let (entries, decoded) = self
            .quarantine
            .read_metadata(&self.store, &mut cache.verified)?;
        cache.compact_verified();
        metrics::gauge!(DEPLOY_STORE_SIZE_METRIC, "store" => N::NAME).set(entries.len() as f64);
        if decoded > 0 {
            metrics::counter!(DEPLOY_STORE_READ_ALL_DECODED_METRIC, "store" => N::NAME)
                .increment(decoded as u64);
        }
        if let Some(limit) = N::SIZE_WARNING_THRESHOLD {
            if entries.len() > limit {
                tracing::warn!(
                    store = N::NAME,
                    size = entries.len(),
                    limit,
                    "deploy store above size warning threshold"
                );
            }
        }
        let after = store_revision(self.store.raw_store());
        if !cache.initialized || entries != cache.snapshot.entries {
            cache.snapshot.generation = cache.snapshot.generation.wrapping_add(1);
            cache.snapshot.entries = entries;
        }
        cache.snapshot.parent_generation = None;
        cache.snapshot.additions.clear();
        cache.snapshot.removals.clear();
        cache.initialized = true;
        cache.revision = before.filter(|revision| Some(*revision) == after);
        Ok(cache.snapshot.clone())
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
            metadata: DeployMetadata::from(deploy),
        })
    }

    pub fn add_prepared(&self, deploys: Vec<PreparedDeploy>) -> Result<(), KvStoreError> {
        let additions = deploys
            .iter()
            .map(|deploy| deploy.metadata.clone())
            .collect();
        let records = deploys
            .into_iter()
            .map(|deploy| (deploy.key, deploy.value))
            .collect();
        let mut cache = self.metadata.lock();
        let before = store_revision(self.store.raw_store());
        let result = self.store.raw_store().put(records);
        cache.update(
            before,
            store_revision(self.store.raw_store()),
            1,
            additions,
            Vec::new(),
        );
        if result.is_err() {
            cache.revision = None;
        }
        result
    }

    pub fn add_prepared_if_absent(&self, deploy: PreparedDeploy) -> Result<bool, KvStoreError> {
        if self.quarantine.contains(&self.store, &deploy.key)? {
            return Ok(false);
        }
        let mut cache = self.metadata.lock();
        let before = store_revision(self.store.raw_store());
        let result = self
            .store
            .raw_store()
            .put_one_if_absent(deploy.key.clone(), deploy.value);
        let writes = usize::from(matches!(result, Ok(true)));
        cache.update(
            before,
            store_revision(self.store.raw_store()),
            writes,
            vec![deploy.metadata],
            Vec::new(),
        );
        if result.is_err() {
            cache.revision = None;
        }
        let inserted = result?;
        if !inserted && !self.quarantine.contains(&self.store, &deploy.key)? {
            return Err(KvStoreError::InvalidArgument(
                "Deploy insertion was blocked by an invalid or concurrently changed record"
                    .to_string(),
            ));
        }
        Ok(inserted)
    }

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
    ) -> Result<PreparedInsertResult, KvStoreError> {
        if deploy.key != snapshot.key {
            return Err(KvStoreError::InvalidArgument(
                "Deploy snapshot belongs to a different signature".to_string(),
            ));
        }
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
        let mut cache = self.metadata.lock();
        let before = store_revision(self.store.raw_store());
        let result = self
            .store
            .raw_store()
            .put_one_if_absent(deploy.key.clone(), deploy.value.clone());
        let writes = usize::from(matches!(result, Ok(true)));
        cache.update(
            before,
            store_revision(self.store.raw_store()),
            writes,
            vec![deploy.metadata.clone()],
            Vec::new(),
        );
        if result.is_err() {
            cache.revision = None;
        }
        Ok(if result? {
            PreparedInsertResult::Inserted
        } else {
            PreparedInsertResult::Changed
        })
    }

    pub fn sigs(&self) -> Result<Vec<ByteString>, KvStoreError> {
        self.quarantine.sigs(&self.store)
    }

    pub fn remove(&self, deploys: Vec<Signed<DeployData>>) -> Result<(), KvStoreError> {
        self.remove_signatures(deploys.into_iter().map(|d| d.sig.to_vec()).collect())
    }

    fn remove_signatures(&self, signatures: Vec<Vec<u8>>) -> Result<(), KvStoreError> {
        let mut cache = self.metadata.lock();
        let before = store_revision(self.store.raw_store());
        let result = self.store.delete(signatures.clone());
        cache.update(
            before,
            store_revision(self.store.raw_store()),
            1,
            Vec::new(),
            signatures,
        );
        if result.is_err() {
            cache.revision = None;
        }
        result
    }

    pub fn remove_by_sig(&self, sig: &[u8]) -> Result<bool, KvStoreError> {
        let mut cache = self.metadata.lock();
        let before = store_revision(self.store.raw_store());
        let key = self.store.encode_key(&sig.to_vec())?;
        let result = self
            .store
            .raw_store()
            .delete(vec![key])
            .map(|count| count > 0);
        cache.update(
            before,
            store_revision(self.store.raw_store()),
            1,
            Vec::new(),
            vec![sig.to_vec()],
        );
        if result.is_err() {
            cache.revision = None;
        }
        result
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

        assert!(storage.any(|d| Ok(d.data.time_stamp == 2)).unwrap());
        assert!(!storage.any(|d| Ok(d.data.time_stamp == 99)).unwrap());

        storage.remove(vec![d2]).unwrap();
        assert_eq!(storage.read_all().unwrap(), HashSet::from([d1.clone()]));

        assert!(storage.remove_by_sig(&d1.sig).unwrap());
        assert!(!storage.remove_by_sig(&d1.sig).unwrap());
        assert!(!storage.non_empty().unwrap());
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
    fn checked_insertion_rejects_an_outdated_absence_snapshot() {
        let storage = KeyValueDeployStorage::from_stores(
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
        )
        .unwrap();
        let candidate = deploy(1);
        let absent = storage.inspect_sig(&candidate.sig).unwrap();
        assert!(!absent.is_present());
        storage.add(vec![candidate.clone()]).unwrap();
        assert_eq!(
            storage
                .add_prepared_if_unchanged(&storage.prepare(&candidate).unwrap(), &absent)
                .unwrap(),
            PreparedInsertResult::Changed
        );
        let present = storage.inspect_sig(&candidate.sig).unwrap();
        assert!(present.is_present());
        assert_eq!(
            storage
                .add_prepared_if_unchanged(&storage.prepare(&candidate).unwrap(), &present)
                .unwrap(),
            PreparedInsertResult::AlreadyPresent
        );
    }

    #[test]
    fn checked_insertion_requires_a_fresh_snapshot_after_quarantining() {
        let live: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let quarantine: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let storage = KeyValueDeployStorage::from_stores(live.clone(), quarantine.clone()).unwrap();
        let candidate = deploy(1);
        let key = storage.store.encode_key(&candidate.sig.to_vec()).unwrap();
        live.put_one(key.clone(), vec![0xff]).unwrap();
        let damaged = storage.inspect_sig(&candidate.sig).unwrap();
        assert!(!damaged.is_present());
        assert_eq!(
            storage
                .add_prepared_if_unchanged(&storage.prepare(&candidate).unwrap(), &damaged)
                .unwrap(),
            PreparedInsertResult::Changed
        );
        let absent = storage.inspect_sig(&candidate.sig).unwrap();
        assert_eq!(
            storage
                .add_prepared_if_unchanged(&storage.prepare(&candidate).unwrap(), &absent)
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
                .add_prepared_if_unchanged(&prepared, &present)
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
        let storage =
            KeyValueDeployStorage::from_stores(store, Arc::new(InMemoryKeyValueStore::new()))
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

    mod metadata_cache {
        use rspace_plus_plus::rspace::shared::lmdb_dir_store_manager::{
            Db, LmdbDirStoreManager, LmdbEnvConfig,
        };

        use super::{
            deploy, KeyValueDeployStorage, KeyValueStoreManager, DEPLOY_STORAGE_DB,
            DEPLOY_STORAGE_QUARANTINE_DB,
        };

        async fn storage(path: &std::path::Path) -> KeyValueDeployStorage {
            let mapping = [DEPLOY_STORAGE_DB, DEPLOY_STORAGE_QUARANTINE_DB]
                .into_iter()
                .map(|name| {
                    (
                        Db::new(name.into(), None),
                        LmdbEnvConfig::new("deploys".into(), 10 * 1024 * 1024),
                    )
                })
                .collect();
            let mut manager = LmdbDirStoreManager::new(path.to_path_buf(), mapping);
            let store = manager.store(DEPLOY_STORAGE_DB.into()).await.unwrap();
            let quarantine = manager
                .store(DEPLOY_STORAGE_QUARANTINE_DB.into())
                .await
                .unwrap();
            KeyValueDeployStorage::from_stores(store, quarantine).unwrap()
        }

        thread_local! {
            static ON_DECODE: std::cell::RefCell<Option<Box<dyn FnMut()>>> =
                const { std::cell::RefCell::new(None) };
        }

        struct DecodeHook;

        impl DecodeHook {
            fn install(pool: &mut KeyValueDeployStorage, hook: impl FnMut() + 'static) -> Self {
                use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

                use crate::rust::deploy::deploy_codec;

                ON_DECODE.with(|slot| {
                    let mut slot = slot.borrow_mut();
                    assert!(slot.is_none());
                    *slot = Some(Box::new(hook));
                });
                pool.store = KeyValueTypedStoreImpl::with_value_codec(
                    pool.store.raw_store().clone(),
                    deploy_codec::encode,
                    |bytes| {
                        let deploy = deploy_codec::decode(bytes)?;
                        ON_DECODE.with(|slot| {
                            if let Some(hook) = slot.borrow_mut().as_mut() {
                                hook();
                            }
                        });
                        Ok(deploy)
                    },
                );
                Self
            }
        }

        impl Drop for DecodeHook {
            fn drop(&mut self) { ON_DECODE.with(|slot| *slot.borrow_mut() = None); }
        }

        #[tokio::test]
        async fn unrelated_writes_reuse_verified_records_without_decoding_again() {
            use std::cell::Cell;
            use std::rc::Rc;
            use std::sync::Arc;

            use shared::rust::store::key_value_store::KeyValueStore;
            use shared::rust::store::lmdb_key_value_store::LmdbKeyValueStore;

            let dir = tempfile::tempdir().unwrap();
            let mut pool = storage(dir.path()).await;
            let first = deploy(1);
            pool.add(vec![first.clone()]).unwrap();

            let store = pool
                .store
                .raw_store()
                .as_any()
                .downcast_ref::<LmdbKeyValueStore>()
                .unwrap();
            let mut txn = store.env.write_txn().unwrap();
            let db = store
                .env
                .create_database(&mut txn, Some("unrelated"))
                .unwrap();
            txn.commit().unwrap();
            let unrelated = Arc::new(LmdbKeyValueStore::new(store.env.clone(), db));
            let writes = Rc::new(Cell::new(0_u64));
            let observed = writes.clone();
            let mutation = unrelated.clone();
            let _hook = DecodeHook::install(&mut pool, move || {
                let next = observed.get() + 1;
                mutation
                    .put(vec![(b"key".to_vec(), next.to_be_bytes().to_vec())])
                    .unwrap();
                observed.set(next);
            });

            let first_snapshot = pool.metadata_snapshot().unwrap();
            for index in 100_u64..103 {
                unrelated
                    .put(vec![(b"key".to_vec(), index.to_be_bytes().to_vec())])
                    .unwrap();
                let snapshot = pool.metadata_snapshot().unwrap();
                assert_eq!(snapshot.entries, first_snapshot.entries);
                assert_eq!(snapshot.generation, first_snapshot.generation);
                assert!(snapshot.entries.contains_key(&first.sig.to_vec()));
            }
            assert_eq!(writes.get(), 1);
        }

        #[tokio::test]
        async fn a_changed_raw_record_is_decoded_before_reusing_metadata() {
            use std::cell::Cell;
            use std::rc::Rc;

            let dir = tempfile::tempdir().unwrap();
            let mut pool = storage(dir.path()).await;
            let independent = storage(dir.path()).await;
            let first = deploy(1);
            let second = deploy(2);
            pool.add(vec![first.clone()]).unwrap();
            let decodes = Rc::new(Cell::new(0));
            let observed = decodes.clone();
            let _hook = DecodeHook::install(&mut pool, move || observed.set(observed.get() + 1));
            let initial = pool.metadata_snapshot().unwrap();
            assert_eq!(decodes.get(), 1);

            independent
                .store
                .put_one(first.sig.to_vec(), second.clone())
                .unwrap();
            let changed = pool.metadata_snapshot().unwrap();
            assert_eq!(decodes.get(), 2);
            assert_ne!(changed.generation, initial.generation);
            assert!(!changed.entries.contains_key(&first.sig.to_vec()));
            assert!(!changed.entries.contains_key(&second.sig.to_vec()));
            assert!(changed.entries.is_empty());

            let temporary = deploy(3);
            independent.add(vec![temporary.clone()]).unwrap();
            independent.remove_by_sig(&temporary.sig).unwrap();
            assert_eq!(pool.metadata_snapshot().unwrap().entries, changed.entries);
            assert_eq!(decodes.get(), 2);
        }

        #[tokio::test]
        async fn tampering_with_a_verified_record_still_quarantines_it() {
            use std::sync::Arc;

            use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
            use shared::rust::store::key_value_store::KeyValueStore;

            let dir = tempfile::tempdir().unwrap();
            let persistent = storage(dir.path()).await;
            let quarantine = Arc::new(InMemoryKeyValueStore::new());
            let pool = KeyValueDeployStorage::from_stores(
                persistent.store.raw_store().clone(),
                quarantine.clone(),
            )
            .unwrap();
            let first = deploy(1);
            pool.add(vec![first.clone()]).unwrap();
            assert_eq!(pool.metadata_snapshot().unwrap().entries.len(), 1);
            let key = pool.store.encode_key(&first.sig.to_vec()).unwrap();
            let mut bytes = crate::rust::deploy::deploy_codec::encode(&first).unwrap();
            bytes[b"F1R3DEP\x02".len()] ^= 1;
            persistent
                .store
                .raw_store()
                .put(vec![(key.clone(), bytes.clone())])
                .unwrap();

            assert!(pool.metadata_snapshot().unwrap().entries.is_empty());
            assert!(!pool.contains_sig(&first.sig).unwrap());
            assert_eq!(quarantine.get_one(&key).unwrap(), Some(bytes));
            assert!(pool.metadata.lock().verified.is_empty());
        }

        #[tokio::test]
        async fn known_deletions_evict_verified_records_without_accumulating_history() {
            let dir = tempfile::tempdir().unwrap();
            let pool = storage(dir.path()).await;
            let independent = storage(dir.path()).await;
            for round in 0..3 {
                let deploys: Vec<_> = (0..32).map(|i| deploy(round * 32 + i)).collect();
                pool.add(deploys.clone()).unwrap();
                independent
                    .store
                    .put_one(deploys[0].sig.to_vec(), deploys[0].clone())
                    .unwrap();
                pool.metadata_snapshot().unwrap();
                assert_eq!(pool.metadata.lock().verified.len(), 32);
                pool.remove(deploys).unwrap();
                assert!(pool.metadata.lock().verified.is_empty());
                assert_eq!(pool.metadata.lock().verified.capacity(), 0);
                assert!(pool.metadata_snapshot().unwrap().entries.is_empty());
            }
        }

        #[tokio::test]
        async fn a_sparse_verified_cache_shrinks_and_preserves_remaining_records() {
            use std::cell::Cell;
            use std::rc::Rc;

            let dir = tempfile::tempdir().unwrap();
            let mut pool = storage(dir.path()).await;
            let independent = storage(dir.path()).await;
            let deploys: Vec<_> = (0..256).map(deploy).collect();
            pool.add(deploys.clone()).unwrap();
            pool.metadata_snapshot().unwrap();
            let before = pool.metadata.lock().verified.capacity();
            pool.remove(deploys[..240].to_vec()).unwrap();
            {
                let cache = pool.metadata.lock();
                assert_eq!(cache.verified.len(), 16);
                assert!(cache.verified.capacity() < before);
                assert!(cache.verified.capacity() <= 4 * cache.verified.len());
            }
            let decodes = Rc::new(Cell::new(0));
            let observed = decodes.clone();
            let _hook = DecodeHook::install(&mut pool, move || observed.set(observed.get() + 1));
            independent
                .store
                .put_one(deploys[240].sig.to_vec(), deploys[240].clone())
                .unwrap();
            let snapshot = pool.metadata_snapshot().unwrap();
            assert_eq!(snapshot.entries.len(), 16);
            for deploy in &deploys[240..] {
                assert!(snapshot.entries.contains_key(&deploy.sig.to_vec()));
            }
            assert_eq!(decodes.get(), 0);
        }

        #[tokio::test]
        async fn metadata_refresh_releases_capacity_reserved_for_damaged_records() {
            for keep_valid in [false, true] {
                let dir = tempfile::tempdir().unwrap();
                let pool = storage(dir.path()).await;
                if keep_valid {
                    pool.add(vec![deploy(1)]).unwrap();
                }
                let damaged = (0_u8..128)
                    .map(|index| (bincode::serialize(&vec![index; 64]).unwrap(), vec![255]))
                    .collect();
                pool.store.raw_store().put(damaged).unwrap();
                let snapshot = pool.metadata_snapshot().unwrap();
                let expected = usize::from(keep_valid);
                assert_eq!(snapshot.entries.len(), expected);
                let cache = pool.metadata.lock();
                assert_eq!(cache.verified.len(), expected);
                if keep_valid {
                    assert!(cache.verified.capacity() <= 4);
                } else {
                    assert_eq!(cache.verified.capacity(), 0);
                }
            }
        }

        #[tokio::test]
        async fn metadata_refresh_detects_pool_changes_during_decode() {
            let dir = tempfile::tempdir().unwrap();
            for insert in [true, false] {
                let case = dir.path().join(if insert { "insert" } else { "remove" });
                std::fs::create_dir(&case).unwrap();
                let mut pool = storage(&case).await;
                let independent = storage(&case).await;
                let first = deploy(1);
                let second = deploy(2);
                pool.add(vec![first.clone()]).unwrap();
                let signature = first.sig.clone();
                let mut change = Some(second.clone());
                let hook = DecodeHook::install(&mut pool, move || {
                    if let Some(deploy) = change.take() {
                        if insert {
                            independent.add(vec![deploy]).unwrap();
                        } else {
                            independent.remove_by_sig(&signature).unwrap();
                        }
                    }
                });

                let initial = pool.metadata_snapshot().unwrap();
                let refreshed = pool.metadata_snapshot().unwrap();
                assert_eq!(refreshed.entries.len(), if insert { 2 } else { 0 });
                assert_eq!(refreshed.entries.contains_key(&second.sig.to_vec()), insert);
                if initial.entries != refreshed.entries {
                    assert_ne!(initial.generation, refreshed.generation);
                }
                assert_eq!(
                    pool.metadata_snapshot().unwrap().generation,
                    refreshed.generation
                );
                drop(hook);
            }
        }

        #[tokio::test]
        async fn checked_conditional_insert_updates_warm_metadata() {
            use crate::rust::deploy::key_value_deploy_storage::{
                DeployMetadata, PreparedInsertResult,
            };

            let dir = tempfile::tempdir().unwrap();
            let pool = storage(dir.path()).await;
            let initial = pool.metadata_snapshot().unwrap();
            let candidate = deploy(1);
            let prepared = pool.prepare_checked(&candidate).unwrap();
            let absent = pool.inspect_sig(&candidate.sig).unwrap();
            assert_eq!(
                pool.add_prepared_if_unchanged(&prepared, &absent).unwrap(),
                PreparedInsertResult::Inserted
            );
            let added = pool.metadata_snapshot().unwrap();
            assert_eq!(added.parent_generation, Some(initial.generation));
            assert_eq!(
                added.entries.get(&candidate.sig.to_vec()),
                Some(&DeployMetadata::from(&candidate))
            );
            let present = pool.inspect_sig(&candidate.sig).unwrap();
            assert_eq!(
                pool.add_prepared_if_unchanged(&prepared, &present).unwrap(),
                PreparedInsertResult::AlreadyPresent
            );
            assert_eq!(
                pool.metadata_snapshot().unwrap().generation,
                added.generation
            );
        }

        #[tokio::test]
        async fn metadata_tracks_writes_and_removals_from_cloned_handles() {
            let dir = tempfile::tempdir().unwrap();
            let pool = storage(dir.path()).await;
            let proposer = pool.clone();
            let initial = pool.metadata_snapshot().unwrap();
            let first = deploy(1);
            pool.add(vec![first.clone()]).unwrap();
            let added = pool.metadata_snapshot().unwrap();
            assert_eq!(added.entries.len(), 1);
            assert_eq!(added.parent_generation, Some(initial.generation));
            assert_eq!(
                proposer.metadata_snapshot().unwrap().generation,
                added.generation
            );
            assert!(!proposer.add_if_absent(first.clone()).unwrap());
            assert_eq!(
                pool.metadata_snapshot().unwrap().generation,
                added.generation
            );
            proposer.remove_by_sig(&first.sig).unwrap();
            let removed = pool.metadata_snapshot().unwrap();
            assert!(removed.entries.is_empty());
            assert_ne!(removed.generation, added.generation);
            assert_eq!(removed.parent_generation, Some(added.generation));
        }

        #[tokio::test]
        async fn raw_and_independently_opened_writes_force_a_metadata_refresh() {
            let dir = tempfile::tempdir().unwrap();
            let pool = storage(dir.path()).await;
            let initial = pool.metadata_snapshot().unwrap();
            let first = deploy(1);
            pool.store
                .put_one(first.sig.to_vec(), first.clone())
                .unwrap();
            let added = pool.metadata_snapshot().unwrap();
            assert_eq!(added.entries.len(), 1);
            assert_ne!(added.generation, initial.generation);
            assert_eq!(added.parent_generation, None);
            let independent = storage(dir.path()).await;
            independent.add(vec![deploy(2)]).unwrap();
            assert_eq!(pool.metadata_snapshot().unwrap().entries.len(), 2);
            independent.remove_by_sig(&first.sig).unwrap();
            assert!(!pool
                .metadata_snapshot()
                .unwrap()
                .entries
                .contains_key(&first.sig.to_vec()));
        }

        #[tokio::test]
        async fn unchanged_metadata_keeps_its_generation_after_an_unrelated_write() {
            let dir = tempfile::tempdir().unwrap();
            let pool = storage(dir.path()).await;
            let initial = pool.metadata_snapshot().unwrap();
            let independent = storage(dir.path()).await;
            let first = deploy(1);
            independent.add(vec![first.clone()]).unwrap();
            independent.remove_by_sig(&first.sig).unwrap();
            let refreshed = pool.metadata_snapshot().unwrap();
            assert!(refreshed.entries.is_empty());
            assert_eq!(refreshed.generation, initial.generation);
            assert_eq!(refreshed.parent_generation, None);
        }

        #[tokio::test]
        async fn restart_rebuilds_metadata_from_the_persistent_pool() {
            let dir = tempfile::tempdir().unwrap();
            let signature = {
                let pool = storage(dir.path()).await;
                let first = deploy(1);
                pool.add(vec![first.clone()]).unwrap();
                pool.metadata_snapshot().unwrap();
                first.sig.to_vec()
            };
            let pool = storage(dir.path()).await;
            assert!(pool
                .metadata_snapshot()
                .unwrap()
                .entries
                .contains_key(&signature));
        }

        #[tokio::test]
        async fn warm_metadata_does_not_decode_signed_pool_records() {
            use metrics_util::debugging::DebuggingRecorder;
            let dir = tempfile::tempdir().unwrap();
            let pool = storage(dir.path()).await;
            pool.add((0..24).map(deploy).collect()).unwrap();
            pool.metadata_snapshot().unwrap();
            let recorder = DebuggingRecorder::new();
            let snapshots = recorder.snapshotter();
            metrics::with_local_recorder(&recorder, || {
                for timestamp in 24..32 {
                    assert_eq!(
                        pool.metadata_snapshot().unwrap().entries.len(),
                        timestamp as usize
                    );
                    pool.add_if_absent(deploy(timestamp)).unwrap();
                    pool.metadata_snapshot().unwrap();
                }
            });
            assert!(snapshots
                .snapshot()
                .into_vec()
                .iter()
                .all(|(key, ..)| { key.key().name() != "deploy-store.read-all.decoded" }));
            assert_eq!(pool.metadata_snapshot().unwrap().entries.len(), 32);
        }
    }
}
