// See block-storage/src/main/scala/coop/rchain/blockstorage/deploy/KeyValueDeployStorage.scala

use std::collections::HashSet;
use std::marker::PhantomData;
use std::sync::Arc;

use crypto::rust::signatures::signed::Signed;
use models::rust::casper::protocol::casper_message::DeployData;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use shared::rust::store::key_value_store::{KeyValueStore, KvStoreError};
use shared::rust::store::key_value_typed_store::KeyValueTypedStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;
use shared::rust::{BitVector, ByteString};

use super::deploy_record_quarantine::DeployRecordQuarantine;

pub const DEPLOY_STORAGE_DB: &str = "deploy_storage";
pub const DEPLOY_STORAGE_QUARANTINE_DB: &str = "deploy_storage_quarantine";
pub const REJECTED_DEPLOY_BUFFER_DB: &str = "rejected_deploy_buffer";
pub const REJECTED_DEPLOY_BUFFER_QUARANTINE_DB: &str = "rejected_deploy_buffer_quarantine";
pub const DEPLOY_STORAGE_VERIFIED_DB: &str = "deploy_storage_verified";
pub const REJECTED_DEPLOY_BUFFER_VERIFIED_DB: &str = "rejected_deploy_buffer_verified";

const DEPLOY_STORE_SIZE_METRIC: &str = "deploy.store.size";
const DEPLOY_STORE_READ_ALL_DECODED_METRIC: &str = "deploy-store.read-all.decoded";

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
    _name: PhantomData<N>,
}

pub struct PreparedDeploy {
    key: BitVector,
    value: BitVector,
    fingerprint: Blake2b256Hash,
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
        let verified_kv_store = match kvm.store(format!("{}_verified", N::NAME)).await {
            Ok(store) => Some(store),
            Err(error) => {
                tracing::warn!(store = N::NAME, %error, "Could not open deploy verification evidence; signature checks remain enabled");
                None
            }
        };
        Self::from_stores_with_verification(kv_store, quarantine_kv_store, verified_kv_store)
    }

    pub fn from_stores(
        store: Arc<dyn KeyValueStore>,
        quarantine: Arc<dyn KeyValueStore>,
    ) -> Result<Self, KvStoreError> {
        Self::from_stores_with_verification(store, quarantine, None)
    }

    pub(super) fn from_stores_with_verification(
        store: Arc<dyn KeyValueStore>,
        quarantine: Arc<dyn KeyValueStore>,
        verified: Option<Arc<dyn KeyValueStore>>,
    ) -> Result<Self, KvStoreError> {
        let mut quarantine = DeployRecordQuarantine::new(N::NAME, quarantine);
        if let Some(verified) = verified {
            quarantine = quarantine.with_verified_store(verified);
        }
        let storage = Self {
            store: KeyValueTypedStoreImpl::with_value_codec(
                store,
                super::deploy_codec::encode,
                super::deploy_codec::decode,
            ),
            quarantine,
            _name: PhantomData,
        };
        storage.quarantine.restore_readable(&storage.store)?;
        Ok(storage)
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
        let fingerprint = Blake2b256Hash::new(&value);
        Ok(PreparedDeploy {
            key: self.store.encode_key(&deploy.sig.to_vec())?,
            value,
            fingerprint,
        })
    }

    pub fn add_prepared(&self, deploys: Vec<PreparedDeploy>) -> Result<(), KvStoreError> {
        let (records, verified): (Vec<_>, Vec<_>) = deploys
            .into_iter()
            .map(|deploy| {
                (
                    (deploy.key.clone(), deploy.value),
                    (deploy.key, deploy.fingerprint),
                )
            })
            .unzip();
        self.store.raw_store().put(records)?;
        for (key, fingerprint) in &verified {
            self.quarantine
                .remember_verified(key.clone(), fingerprint.clone());
        }
        self.quarantine.persist_verified(verified);
        Ok(())
    }

    pub fn add_prepared_if_absent(&self, deploy: PreparedDeploy) -> Result<bool, KvStoreError> {
        if self.quarantine.contains(&self.store, &deploy.key)? {
            return Ok(false);
        }
        let inserted = self
            .store
            .raw_store()
            .put_one_if_absent(deploy.key.clone(), deploy.value)?;
        if inserted {
            self.quarantine
                .remember_verified(deploy.key.clone(), deploy.fingerprint.clone());
            self.quarantine
                .persist_verified(vec![(deploy.key, deploy.fingerprint)]);
        } else if !self.quarantine.contains(&self.store, &deploy.key)? {
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
        deploy: PreparedDeploy,
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
        let inserted = self
            .store
            .raw_store()
            .put_one_if_absent(deploy.key.clone(), deploy.value)?;
        if inserted {
            self.quarantine
                .remember_verified(deploy.key.clone(), deploy.fingerprint.clone());
            self.quarantine
                .persist_verified(vec![(deploy.key, deploy.fingerprint)]);
        }
        Ok(if inserted {
            PreparedInsertResult::Inserted
        } else {
            PreparedInsertResult::Changed
        })
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
        self.store.raw_store().delete(keys.clone())?;
        self.quarantine.forget_all(&keys);
        Ok(())
    }

    pub fn remove_by_sig(&self, sig: &[u8]) -> Result<bool, KvStoreError> {
        let key: ByteString = sig.to_vec();
        let key = self.store.encode_key(&key)?;
        let removed = self.store.raw_store().delete(vec![key.clone()])? > 0;
        self.quarantine.forget_all(&[key]);
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
                phlo_price: 1,
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
                .add_prepared_if_unchanged(storage.prepare(&candidate).unwrap(), &absent)
                .unwrap(),
            PreparedInsertResult::Changed
        );
        let present = storage.inspect_sig(&candidate.sig).unwrap();
        assert!(present.is_present());
        assert_eq!(
            storage
                .add_prepared_if_unchanged(storage.prepare(&candidate).unwrap(), &present)
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
                .add_prepared_if_unchanged(storage.prepare(&candidate).unwrap(), &damaged)
                .unwrap(),
            PreparedInsertResult::Changed
        );
        let absent = storage.inspect_sig(&candidate.sig).unwrap();
        assert_eq!(
            storage
                .add_prepared_if_unchanged(storage.prepare(&candidate).unwrap(), &absent)
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
                .add_prepared_if_unchanged(prepared, &present)
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
                phlo_price: 1,
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
}
