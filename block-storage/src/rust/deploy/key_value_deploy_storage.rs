// See block-storage/src/main/scala/coop/rchain/blockstorage/deploy/KeyValueDeployStorage.scala

use std::collections::{HashMap, HashSet, VecDeque};
use std::marker::PhantomData;
use std::sync::Arc;

use crypto::rust::signatures::signed::Signed;
use models::rust::casper::protocol::casper_message::DeployData;
use models::rust::deploy_envelope::{DeployEnvelope, DeployEnvelopeLimits, StoredDeployEnvelope};
use models::rust::deploy_id::{DeployIdV6, DeployLookupId};
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use shared::rust::store::key_value_store::{KeyValueStore, KvStoreError};
use shared::rust::store::key_value_typed_store::KeyValueTypedStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;
use shared::rust::ByteString;

use super::deploy_record_quarantine::DeployRecordQuarantine;

pub const DEPLOY_STORAGE_DB: &str = "deploy_storage";
pub const DEPLOY_STORAGE_QUARANTINE_DB: &str = "deploy_storage_quarantine";
pub const REJECTED_DEPLOY_BUFFER_DB: &str = "rejected_deploy_buffer";
pub const REJECTED_DEPLOY_BUFFER_QUARANTINE_DB: &str = "rejected_deploy_buffer_quarantine";
pub const DEPLOY_ENVELOPE_STORAGE_DB: &str = "deploy_envelope_storage";

pub const MAX_ENVELOPE_REJECTIONS: usize = 4096;
pub const MAX_ENVELOPE_REJECTION_REASON_BYTES: usize = 1024;

const DEPLOY_STORE_SIZE_METRIC: &str = "deploy.store.size";
const DEPLOY_STORE_READ_ALL_DECODED_METRIC: &str = "deploy-store.read-all.decoded";

pub trait DeployStoreName {
    const NAME: &'static str;
    const QUARANTINE_NAME: &'static str;
    const ENVELOPE_NAME: Option<&'static str> = None;
    const SIZE_WARNING_THRESHOLD: Option<usize> = None;
}

#[derive(Clone)]
pub struct DeployStorageName;

impl DeployStoreName for DeployStorageName {
    const NAME: &'static str = DEPLOY_STORAGE_DB;
    const QUARANTINE_NAME: &'static str = DEPLOY_STORAGE_QUARANTINE_DB;
    const ENVELOPE_NAME: Option<&'static str> = Some(DEPLOY_ENVELOPE_STORAGE_DB);
}

#[derive(Clone)]
pub struct RejectedDeployBufferName;

impl DeployStoreName for RejectedDeployBufferName {
    const NAME: &'static str = REJECTED_DEPLOY_BUFFER_DB;
    const QUARANTINE_NAME: &'static str = REJECTED_DEPLOY_BUFFER_QUARANTINE_DB;
    const SIZE_WARNING_THRESHOLD: Option<usize> = Some(1_000);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvelopeRejection {
    pub deploy_id: DeployIdV6,
    pub pre_state_root: Option<ByteString>,
    pub block_number: i64,
    pub reason: String,
}

struct EnvelopeRejectionEntries {
    capacity: usize,
    order: VecDeque<DeployIdV6>,
    entries: HashMap<DeployIdV6, EnvelopeRejection>,
}

impl EnvelopeRejectionEntries {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity,
            order: VecDeque::new(),
            entries: HashMap::new(),
        }
    }
}

impl Default for EnvelopeRejectionEntries {
    fn default() -> Self { Self::with_capacity(MAX_ENVELOPE_REJECTIONS) }
}

#[derive(Clone, Default)]
pub struct EnvelopeRejectionLog(Arc<parking_lot::Mutex<EnvelopeRejectionEntries>>);

impl EnvelopeRejectionLog {
    fn record(&self, mut rejection: EnvelopeRejection) {
        if rejection.reason.len() > MAX_ENVELOPE_REJECTION_REASON_BYTES {
            let mut end = MAX_ENVELOPE_REJECTION_REASON_BYTES;
            while !rejection.reason.is_char_boundary(end) {
                end -= 1;
            }
            rejection.reason.truncate(end);
        }
        let deploy_id = rejection.deploy_id;
        let mut log = self.0.lock();
        if log.entries.insert(deploy_id, rejection).is_none() {
            log.order.push_back(deploy_id);
        }
        while log.order.len() > log.capacity {
            if let Some(oldest) = log.order.pop_front() {
                log.entries.remove(&oldest);
            }
        }
    }

    fn get(&self, deploy_id: &DeployIdV6) -> Option<EnvelopeRejection> {
        self.0.lock().entries.get(deploy_id).cloned()
    }

    fn clear(&self, deploy_id: &DeployIdV6) {
        let mut log = self.0.lock();
        if log.entries.remove(deploy_id).is_some() {
            log.order.retain(|recorded| recorded != deploy_id);
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize { self.0.lock().entries.len() }
}

#[derive(Clone)]
pub struct KeyValueDeployStore<N: DeployStoreName> {
    pub store: KeyValueTypedStoreImpl<ByteString, Signed<DeployData>>,
    pub envelope_store: KeyValueTypedStoreImpl<ByteString, StoredDeployEnvelope>,
    pub envelope_rejections: EnvelopeRejectionLog,
    quarantine: DeployRecordQuarantine,
    _name: PhantomData<N>,
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
        let storage = Self::from_stores(kv_store, quarantine_kv_store)?;
        match N::ENVELOPE_NAME {
            Some(name) => Ok(storage.with_envelope_store(kvm.store(name.to_string()).await?)),
            None => Ok(storage),
        }
    }

    pub fn from_stores(
        store: Arc<dyn KeyValueStore>,
        quarantine: Arc<dyn KeyValueStore>,
    ) -> Result<Self, KvStoreError> {
        let storage = Self {
            store: KeyValueTypedStoreImpl::with_value_codec(
                store,
                super::deploy_codec::encode,
                super::deploy_codec::decode,
            ),
            envelope_store: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
            envelope_rejections: EnvelopeRejectionLog::default(),
            quarantine: DeployRecordQuarantine::new(N::NAME, quarantine),
            _name: PhantomData,
        };
        storage.quarantine.restore_readable(&storage.store)?;
        Ok(storage)
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
        if self
            .store
            .contains(vec![key])?
            .into_iter()
            .next()
            .unwrap_or(false)
        {
            return Ok(true);
        }
        match DeployIdV6::try_from(sig) {
            Ok(deploy_id) => self.contains_envelope_id(&deploy_id),
            Err(_) => Ok(false),
        }
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
            return match DeployIdV6::try_from(sig) {
                Ok(deploy_id) => self.remove_envelope_by_id(&deploy_id),
                Err(_) => Ok(false),
            };
        }
        self.store.delete(vec![key])?;
        Ok(true)
    }

    pub fn with_envelope_store(mut self, store: Arc<dyn KeyValueStore>) -> Self {
        self.envelope_store = KeyValueTypedStoreImpl::new(store);
        self
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
        let inserted = self
            .envelope_store
            .put_one_if_absent(deploy_id.as_ref().to_vec(), stored)?;
        if inserted {
            self.envelope_rejections.clear(deploy_id);
        }
        Ok(inserted)
    }

    pub fn quarantine_envelope(
        &mut self,
        rejection: EnvelopeRejection,
    ) -> Result<bool, KvStoreError> {
        let removed = self.remove_envelope_by_id(&rejection.deploy_id)?;
        self.envelope_rejections.record(rejection);
        Ok(removed)
    }

    pub fn envelope_rejection(&self, deploy_id: &DeployIdV6) -> Option<EnvelopeRejection> {
        self.envelope_rejections.get(deploy_id)
    }

    pub fn read_all_envelopes(
        &self,
        limits: DeployEnvelopeLimits,
    ) -> Result<Vec<DeployEnvelope>, KvStoreError> {
        let mut envelopes = Vec::new();
        for (key, stored) in self.envelope_store.to_map()? {
            let deploy_id = DeployIdV6::try_from(key.as_slice())
                .map_err(|error| KvStoreError::SerializationError(error.to_string()))?;
            envelopes.push(
                stored
                    .decode(&DeployLookupId::V6(deploy_id), limits)
                    .map_err(KvStoreError::SerializationError)?,
            );
        }
        envelopes
            .sort_by(|left, right| left.identity().as_bytes().cmp(right.identity().as_bytes()));
        Ok(envelopes)
    }

    pub fn remove_envelope_by_id(&mut self, deploy_id: &DeployIdV6) -> Result<bool, KvStoreError> {
        if !self.contains_envelope_id(deploy_id)? {
            return Ok(false);
        }
        self.envelope_store
            .delete(vec![deploy_id.as_ref().to_vec()])?;
        Ok(true)
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
    pub fn non_empty(&self) -> Result<bool, KvStoreError> {
        Ok(self.store.non_empty()? || self.envelope_store.non_empty()?)
    }

    pub fn pending_count(&self) -> Result<Option<usize>, KvStoreError> {
        let legacy = self.store.raw_store().entry_count()?;
        let offered = self.envelope_store.raw_store().entry_count()?;
        Ok(legacy
            .zip(offered)
            .map(|(legacy, offered)| legacy + offered))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Barrier};

    use crypto::rust::private_key::PrivateKey;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signed::Cosigned;
    use metrics_util::debugging::{DebugValue, DebuggingRecorder};
    use models::rust::cost_deploy_data::DeployData as CostDeployData;
    use models::rust::cost_protocol_limits::offered_funded_v6_limits;
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

    fn v6_id(envelope: &DeployEnvelope) -> DeployIdV6 {
        match envelope.identity() {
            DeployLookupId::V6(id) => *id,
            DeployLookupId::Legacy(_) => panic!("expected a v6 identity"),
        }
    }

    #[tokio::test]
    async fn envelope_pool_keeps_legacy_rows_and_survives_reopen() {
        let mut kvm = InMemoryStoreManager::new();
        let mut storage = KeyValueDeployStorage::new(&mut kvm).await.unwrap();
        let limits = offered_funded_v6_limits().envelope;
        let legacy = deploy(5);
        let envelope = body_envelope();
        let id = v6_id(&envelope);
        storage.add_if_absent(legacy.clone()).unwrap();
        assert!(storage.add_envelope_if_absent(&envelope).unwrap());
        assert!(!storage.add_envelope_if_absent(&envelope).unwrap());
        assert!(storage.contains_envelope_id(&id).unwrap());
        assert_eq!(storage.read_all_envelopes(limits).unwrap(), vec![
            envelope.clone()
        ]);
        assert_eq!(storage.read_all().unwrap(), HashSet::from([legacy.clone()]));
        let reopened = KeyValueDeployStorage::new(&mut kvm).await.unwrap();
        assert_eq!(reopened.read_all_envelopes(limits).unwrap(), vec![envelope]);
        assert!(storage.remove_envelope_by_id(&id).unwrap());
        assert!(!storage.remove_envelope_by_id(&id).unwrap());
        assert!(!storage.contains_envelope_id(&id).unwrap());
        assert!(storage.contains_sig(&legacy.sig).unwrap());
        assert!(storage.non_empty().unwrap());
        storage.remove_by_sig(&legacy.sig).unwrap();
        assert!(!storage.non_empty().unwrap());
    }

    #[tokio::test]
    async fn identity_lookups_find_an_envelope_by_its_deploy_id() {
        let mut kvm = InMemoryStoreManager::new();
        let mut storage = KeyValueDeployStorage::new(&mut kvm).await.unwrap();
        let legacy = deploy(5);
        let envelope = body_envelope();
        let id = v6_id(&envelope);
        storage.add_if_absent(legacy.clone()).unwrap();
        assert!(storage.add_envelope_if_absent(&envelope).unwrap());
        assert!(storage.non_empty().unwrap());
        assert!(storage.contains_sig(id.as_ref()).unwrap());
        assert!(!storage.contains_sig(&[7; 32]).unwrap());
        assert!(storage.remove_by_sig(id.as_ref()).unwrap());
        assert!(!storage.contains_sig(id.as_ref()).unwrap());
        assert!(!storage.remove_by_sig(id.as_ref()).unwrap());
        assert!(storage.contains_sig(&legacy.sig).unwrap());
    }

    #[tokio::test]
    async fn stores_without_an_envelope_name_keep_envelopes_in_memory() {
        let mut storage = KeyValueDeployStorage::from_stores(
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
        )
        .unwrap();
        let envelope = body_envelope();
        assert!(storage.add_envelope_if_absent(&envelope).unwrap());
        assert!(storage.contains_envelope_id(&v6_id(&envelope)).unwrap());
    }

    fn rejection(deploy_id: DeployIdV6, reason: &str) -> EnvelopeRejection {
        EnvelopeRejection {
            deploy_id,
            pre_state_root: Some(vec![9; 32]),
            block_number: 7,
            reason: reason.to_string(),
        }
    }

    fn deploy_id_at(index: u64) -> DeployIdV6 {
        let mut bytes = [0u8; DeployIdV6::LENGTH];
        bytes[..8].copy_from_slice(&index.to_be_bytes());
        DeployIdV6::try_from(bytes.as_slice()).unwrap()
    }

    #[tokio::test]
    async fn quarantine_removes_an_envelope_records_why_and_resubmission_clears_it() {
        let mut kvm = InMemoryStoreManager::new();
        let mut storage = KeyValueDeployStorage::new(&mut kvm).await.unwrap();
        let envelope = body_envelope();
        let id = v6_id(&envelope);
        assert!(storage.add_envelope_if_absent(&envelope).unwrap());
        assert!(storage
            .quarantine_envelope(rejection(id, "no feasible signed assignment"))
            .unwrap());
        assert!(!storage.contains_envelope_id(&id).unwrap());
        assert_eq!(
            storage.envelope_rejection(&id),
            Some(rejection(id, "no feasible signed assignment"))
        );
        assert!(!storage
            .quarantine_envelope(rejection(id, "second decision"))
            .unwrap());
        assert_eq!(
            storage
                .envelope_rejection(&id)
                .map(|recorded| recorded.reason),
            Some("second decision".to_string())
        );
        assert_eq!(storage.envelope_rejections.len(), 1);
        assert!(storage.add_envelope_if_absent(&envelope).unwrap());
        assert_eq!(storage.envelope_rejection(&id), None);
        assert_eq!(storage.envelope_rejections.len(), 0);
    }

    #[test]
    fn the_rejection_log_is_bounded_and_truncates_reasons() {
        let log = EnvelopeRejectionLog::default();
        let total = MAX_ENVELOPE_REJECTIONS as u64 + 10;
        for index in 0..total {
            log.record(rejection(deploy_id_at(index), "x"));
        }
        assert_eq!(log.len(), MAX_ENVELOPE_REJECTIONS);
        assert_eq!(log.get(&deploy_id_at(9)), None);
        assert!(log.get(&deploy_id_at(10)).is_some());
        assert!(log.get(&deploy_id_at(total - 1)).is_some());
        let long = "\u{e9}".repeat(MAX_ENVELOPE_REJECTION_REASON_BYTES);
        log.record(rejection(deploy_id_at(u64::MAX), &long));
        let recorded = log.get(&deploy_id_at(u64::MAX)).unwrap().reason;
        assert!(recorded.len() <= MAX_ENVELOPE_REJECTION_REASON_BYTES);
        assert!(long.starts_with(&recorded));
        assert_eq!(log.len(), MAX_ENVELOPE_REJECTIONS);
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
    async fn remove_deletes_listed_deploys() {
        let mut buffer = buffer().await;
        let (d1, d2) = (deploy(1), deploy(2));
        buffer.add(vec![d1.clone(), d2.clone()]).unwrap();

        buffer.remove(vec![d2]).unwrap();
        assert_eq!(buffer.read_all().unwrap(), HashSet::from([d1]));
    }

    #[tokio::test]
    async fn remove_by_sig_reports_presence() {
        let mut buffer = buffer().await;
        let d1 = deploy(1);
        buffer.add(vec![d1.clone()]).unwrap();

        assert!(buffer.remove_by_sig(&d1.sig).unwrap());
        assert!(!buffer.remove_by_sig(&d1.sig).unwrap());
        assert!(!buffer.contains_sig(&d1.sig).unwrap());
        assert!(!buffer.non_empty().unwrap());
    }

    #[tokio::test]
    async fn read_all_reports_the_size_of_each_store() {
        let mut kvm = InMemoryStoreManager::new();
        let mut storage = KeyValueDeployStorage::new(&mut kvm).await.unwrap();
        let mut buffer = KeyValueRejectedDeployBuffer::new(&mut kvm).await.unwrap();
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
        let mut storage = KeyValueDeployStorage::new(&mut kvm).await.unwrap();
        let mut buffer = KeyValueRejectedDeployBuffer::new(&mut kvm).await.unwrap();
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
        let mut storage = KeyValueDeployStorage::new(&mut kvm).await.unwrap();
        let d1 = deploy(1);
        assert!(storage.add_if_absent(d1.clone()).unwrap());
        assert!(!storage.add_if_absent(d1).unwrap());
        assert_eq!(storage.read_all().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn the_two_stores_open_separate_lmdb_stores() {
        let mut kvm = InMemoryStoreManager::new();
        let mut storage = KeyValueDeployStorage::new(&mut kvm).await.unwrap();
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
