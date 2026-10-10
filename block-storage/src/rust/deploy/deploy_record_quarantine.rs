// Quarantine of undecodable records in the node-local deploy stores. The
// behavior and the operator procedure are in docs/block-storage/README.md.

use std::collections::HashSet;
use std::sync::Arc;

use crypto::rust::signatures::signed::Signed;
use models::rust::casper::protocol::casper_message::DeployData;
use shared::rust::store::key_value_store::{KeyValueStore, KvStoreError};
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;
use shared::rust::{ByteBuffer, ByteString};

pub(super) type DeployTypedStore = KeyValueTypedStoreImpl<ByteString, Signed<DeployData>>;

pub const QUARANTINED_RECORDS_METRIC: &str = "deploy_store.quarantined_records";
pub const RESTORED_RECORDS_METRIC: &str = "deploy_store.restored_records";

const MAX_LOGGED_RECORDS_PER_SCAN: usize = 10;
const MAX_LOGGED_KEY_BYTES: usize = 32;
const MAX_LOGGED_ERROR_CHARS: usize = 160;

struct DamagedRecord {
    key: ByteBuffer,
    value: ByteBuffer,
    error: KvStoreError,
}

#[derive(Clone)]
pub(super) struct DeployRecordQuarantine {
    store_name: &'static str,
    quarantine: Arc<dyn KeyValueStore>,
}

impl DeployRecordQuarantine {
    pub(super) fn new(store_name: &'static str, quarantine: Arc<dyn KeyValueStore>) -> Self {
        Self {
            store_name,
            quarantine,
        }
    }

    pub(super) fn read_all(
        &self,
        store: &DeployTypedStore,
    ) -> Result<HashSet<Signed<DeployData>>, KvStoreError> {
        let mut deploys = HashSet::new();
        let mut damaged = Vec::new();
        for (key, value) in store.raw_store().to_map()? {
            match decode_record(store, &key, &value) {
                Ok(deploy) => {
                    deploys.insert(deploy);
                }
                Err(error) => damaged.push(DamagedRecord { key, value, error }),
            }
        }
        self.isolate(store, damaged)?;
        Ok(deploys)
    }

    pub(super) fn any<F>(
        &self,
        store: &DeployTypedStore,
        mut predicate: F,
    ) -> Result<bool, KvStoreError>
    where
        F: FnMut(&Signed<DeployData>) -> Result<bool, KvStoreError>,
    {
        let mut matched = false;
        let mut damaged = Vec::new();
        store.raw_store().iterate_while(&mut |key, value| {
            match decode_record(store, &key, &value) {
                Ok(deploy) => {
                    if predicate(&deploy)? {
                        matched = true;
                        return Ok(false);
                    }
                }
                Err(error) => damaged.push(DamagedRecord { key, value, error }),
            }
            Ok(true)
        })?;
        self.isolate(store, damaged)?;
        Ok(matched)
    }

    pub(super) fn restore_readable(&self, store: &DeployTypedStore) -> Result<(), KvStoreError> {
        let quarantined = self.quarantine.to_map()?;
        if quarantined.is_empty() {
            return Ok(());
        }
        let total = quarantined.len();
        let mut restored = 0usize;
        for (key, value) in quarantined {
            if decode_record(store, &key, &value).is_err() {
                continue;
            }
            store.raw_store().put_one_if_absent(key.clone(), value)?;
            self.quarantine.delete(vec![key])?;
            restored += 1;
        }
        if restored > 0 {
            metrics::counter!(RESTORED_RECORDS_METRIC, "store" => self.store_name)
                .increment(restored as u64);
        }
        tracing::warn!(
            store = self.store_name,
            restored,
            remaining = total - restored,
            "Deploy store opened with quarantined records"
        );
        Ok(())
    }

    fn isolate(
        &self,
        store: &DeployTypedStore,
        damaged: Vec<DamagedRecord>,
    ) -> Result<(), KvStoreError> {
        if damaged.is_empty() {
            return Ok(());
        }
        let total = damaged.len();
        let live = store.raw_store();
        for (index, record) in damaged.into_iter().enumerate() {
            self.quarantine
                .put_one(record.key.clone(), record.value.clone())?;
            if live.get_one(&record.key)?.as_ref() == Some(&record.value) {
                live.delete(vec![record.key.clone()])?;
            }
            metrics::counter!(QUARANTINED_RECORDS_METRIC, "store" => self.store_name).increment(1);
            if index < MAX_LOGGED_RECORDS_PER_SCAN {
                tracing::warn!(
                    store = self.store_name,
                    key_prefix = %hex::encode(&record.key[..record.key.len().min(MAX_LOGGED_KEY_BYTES)]),
                    key_len = record.key.len(),
                    value_len = record.value.len(),
                    error = %bounded_error(&record.error),
                    "Moved an undecodable deploy record to quarantine"
                );
            }
        }
        if total > MAX_LOGGED_RECORDS_PER_SCAN {
            tracing::warn!(
                store = self.store_name,
                quarantined = total,
                logged = MAX_LOGGED_RECORDS_PER_SCAN,
                "Moved more undecodable deploy records to quarantine than were logged"
            );
        }
        Ok(())
    }
}

fn decode_record(
    store: &DeployTypedStore,
    key: &ByteBuffer,
    value: &ByteBuffer,
) -> Result<Signed<DeployData>, KvStoreError> {
    store.decode_key(key)?;
    store.decode_value(value)
}

fn bounded_error(error: &KvStoreError) -> String {
    error
        .to_string()
        .chars()
        .take(MAX_LOGGED_ERROR_CHARS)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicBool, Ordering};

    use crypto::rust::private_key::PrivateKey;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use metrics_util::debugging::{DebugValue, DebuggingRecorder, Snapshotter};
    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

    use super::*;
    use crate::rust::deploy::key_value_deploy_storage::{
        KeyValueDeployStorage, DEPLOY_STORAGE_DB, DEPLOY_STORAGE_QUARANTINE_DB,
    };
    use crate::rust::deploy::key_value_rejected_deploy_buffer::{
        KeyValueRejectedDeployBuffer, REJECTED_DEPLOY_BUFFER_DB,
        REJECTED_DEPLOY_BUFFER_QUARANTINE_DB,
    };

    const LEGACY_DEPLOY_HEX: &str = concat!(
        "03000000000000004e696c7b000000000000000700000000000000a086010000",
        "00000011000000000000000400000000000000726f6f7401e803000000000000",
        "4100000000000000041b84c5567b126440995d3ed5aaba0565d71e1834604819",
        "ff9c17f5e9d5dd078f70beaf8f588b541507fed6a642c5ab42dfdf8120a7f639",
        "de5122d47a69a8e8d14700000000000000304502210092526253b63faa274731",
        "a1907908ded5bb5e46fea2d28464985ca4bf3ec3c4b802206f05828da0310cfd",
        "c5e7da0cd34385559b5f6d4cf3b25ca78176416ecf6fadb50900000000000000",
        "736563703235366b31",
    );
    const MALFORMED_VALUE: &[u8] = &[0xff, 0x01];
    const UNSUPPORTED_FORMAT_VALUE: &[u8] = b"F1R3DEP\x02\x0a\x03Nil\x10\x7b";

    fn deploy(time_stamp: i64) -> Signed<DeployData> {
        signed(DeployData {
            term: "Nil".to_string(),
            time_stamp,
            phlo_price: 1,
            phlo_limit: 100_000,
            valid_after_block_number: 0,
            shard_id: "root".to_string(),
            expiration_timestamp: None,
        })
    }

    fn legacy_deploy() -> Signed<DeployData> {
        signed(DeployData {
            term: "Nil".to_string(),
            time_stamp: 123,
            phlo_price: 7,
            phlo_limit: 100_000,
            valid_after_block_number: 17,
            shard_id: "root".to_string(),
            expiration_timestamp: Some(1000),
        })
    }

    fn signed(data: DeployData) -> Signed<DeployData> {
        Signed::create(data, Box::new(Secp256k1), PrivateKey::from_bytes(&[1; 32])).unwrap()
    }

    fn legacy_bytes() -> ByteBuffer { hex::decode(LEGACY_DEPLOY_HEX).unwrap() }

    fn key_for(sig: &[u8]) -> ByteBuffer {
        KeyValueTypedStoreImpl::<ByteString, Signed<DeployData>>::new(Arc::new(
            InMemoryKeyValueStore::new(),
        ))
        .encode_key(&sig.to_vec())
        .unwrap()
    }

    enum Pool {
        Storage(KeyValueDeployStorage),
        Buffer(KeyValueRejectedDeployBuffer),
    }

    impl Pool {
        fn open(
            name: &str,
            live: &Arc<dyn KeyValueStore>,
            quarantine: &Arc<dyn KeyValueStore>,
        ) -> Self {
            match name {
                DEPLOY_STORAGE_DB => Pool::Storage(
                    KeyValueDeployStorage::from_stores(live.clone(), quarantine.clone()).unwrap(),
                ),
                _ => Pool::Buffer(
                    KeyValueRejectedDeployBuffer::from_stores(live.clone(), quarantine.clone())
                        .unwrap(),
                ),
            }
        }

        fn read_all(&self) -> Result<HashSet<Signed<DeployData>>, KvStoreError> {
            match self {
                Pool::Storage(storage) => storage.read_all(),
                Pool::Buffer(buffer) => buffer.read_all(),
            }
        }

        fn add(&mut self, deploys: Vec<Signed<DeployData>>) {
            match self {
                Pool::Storage(storage) => storage.add(deploys).unwrap(),
                Pool::Buffer(buffer) => buffer.add(deploys).unwrap(),
            }
        }
    }

    const POOLS: [&str; 2] = [DEPLOY_STORAGE_DB, REJECTED_DEPLOY_BUFFER_DB];

    fn in_memory() -> Arc<dyn KeyValueStore> { Arc::new(InMemoryKeyValueStore::new()) }

    fn counter(snapshotter: &Snapshotter, name: &str, store: &str) -> u64 {
        snapshotter
            .snapshot()
            .into_vec()
            .into_iter()
            .filter(|(key, _, _, _)| {
                key.key().name() == name
                    && key
                        .key()
                        .labels()
                        .any(|label| label.key() == "store" && label.value() == store)
            })
            .map(|(_, _, _, value)| match value {
                DebugValue::Counter(count) => count,
                _ => 0,
            })
            .sum()
    }

    fn seed_damaged(live: &Arc<dyn KeyValueStore>) -> (ByteBuffer, ByteBuffer) {
        let malformed_key = key_for(&[7; 64]);
        let unsupported_key = key_for(&[8; 64]);
        live.put(vec![
            (malformed_key.clone(), MALFORMED_VALUE.to_vec()),
            (unsupported_key.clone(), UNSUPPORTED_FORMAT_VALUE.to_vec()),
        ])
        .unwrap();
        (malformed_key, unsupported_key)
    }

    #[test]
    fn read_all_keeps_valid_and_legacy_records_and_quarantines_damaged_ones() {
        for name in POOLS {
            let (live, quarantine) = (in_memory(), in_memory());
            let mut pool = Pool::open(name, &live, &quarantine);
            let (d1, d2, legacy) = (deploy(1), deploy(2), legacy_deploy());
            pool.add(vec![d1.clone(), d2.clone()]);
            live.put_one(key_for(&legacy.sig), legacy_bytes()).unwrap();
            let (malformed_key, unsupported_key) = seed_damaged(&live);

            let expected = HashSet::from([d1, d2, legacy]);
            assert_eq!(pool.read_all().unwrap(), expected, "{name}");
            assert_eq!(
                quarantine.to_map().unwrap(),
                BTreeMap::from([
                    (malformed_key.clone(), MALFORMED_VALUE.to_vec()),
                    (unsupported_key.clone(), UNSUPPORTED_FORMAT_VALUE.to_vec()),
                ]),
                "{name}"
            );
            assert_eq!(
                live.get(&vec![malformed_key, unsupported_key]).unwrap(),
                vec![None, None],
                "{name}"
            );
            assert_eq!(pool.read_all().unwrap(), expected, "{name}");
        }
    }

    #[test]
    fn any_skips_damaged_records_and_still_finds_matches() {
        let (live, quarantine) = (in_memory(), in_memory());
        let mut storage =
            KeyValueDeployStorage::from_stores(live.clone(), quarantine.clone()).unwrap();
        storage.add(vec![deploy(1), deploy(2)]).unwrap();
        seed_damaged(&live);

        assert!(!storage.any(|_| Ok(false)).unwrap());
        assert_eq!(quarantine.to_map().unwrap().len(), 2);
        assert!(storage.any(|d| Ok(d.data.time_stamp == 2)).unwrap());
        assert!(matches!(
            storage.any(|_| Err(KvStoreError::InvalidArgument("predicate".to_string()))),
            Err(KvStoreError::InvalidArgument(_))
        ));
    }

    #[test]
    fn reopened_store_scans_cleanly_and_counts_each_damaged_record_once() {
        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        metrics::with_local_recorder(&recorder, || {
            for name in POOLS {
                let (live, quarantine) = (in_memory(), in_memory());
                let d1 = deploy(1);
                Pool::open(name, &live, &quarantine).add(vec![d1.clone()]);
                seed_damaged(&live);

                let reopened = Pool::open(name, &live, &quarantine);
                assert_eq!(reopened.read_all().unwrap(), HashSet::from([d1.clone()]));
                drop(reopened);

                let reopened = Pool::open(name, &live, &quarantine);
                assert_eq!(reopened.read_all().unwrap(), HashSet::from([d1]));
                assert_eq!(quarantine.to_map().unwrap().len(), 2, "{name}");
                assert_eq!(counter(&snapshotter, QUARANTINED_RECORDS_METRIC, name), 2);
                assert_eq!(counter(&snapshotter, RESTORED_RECORDS_METRIC, name), 0);
            }
        });
    }

    #[test]
    fn open_restores_quarantined_records_that_now_decode() {
        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        metrics::with_local_recorder(&recorder, || {
            for name in POOLS {
                let (live, quarantine) = (in_memory(), in_memory());
                let legacy = legacy_deploy();
                let legacy_key = key_for(&legacy.sig);
                quarantine
                    .put_one(legacy_key.clone(), legacy_bytes())
                    .unwrap();
                let (malformed_key, _) = seed_damaged(&quarantine);

                let pool = Pool::open(name, &live, &quarantine);

                assert_eq!(pool.read_all().unwrap(), HashSet::from([legacy]), "{name}");
                assert_eq!(quarantine.get_one(&legacy_key).unwrap(), None, "{name}");
                assert!(
                    quarantine.get_one(&malformed_key).unwrap().is_some(),
                    "{name}"
                );
                assert_eq!(counter(&snapshotter, RESTORED_RECORDS_METRIC, name), 1);
            }
        });
    }

    #[test]
    fn open_does_not_overwrite_a_live_record_with_its_quarantined_copy() {
        let (live, quarantine) = (in_memory(), in_memory());
        let legacy = legacy_deploy();
        let legacy_key = key_for(&legacy.sig);
        let mut storage =
            KeyValueDeployStorage::from_stores(live.clone(), quarantine.clone()).unwrap();
        storage.add(vec![legacy.clone()]).unwrap();
        let live_bytes = live.get_one(&legacy_key).unwrap();
        quarantine
            .put_one(legacy_key.clone(), legacy_bytes())
            .unwrap();

        KeyValueDeployStorage::from_stores(live.clone(), quarantine.clone()).unwrap();

        assert_eq!(live.get_one(&legacy_key).unwrap(), live_bytes);
        assert!(!quarantine.non_empty().unwrap());
    }

    #[tokio::test]
    async fn store_manager_opens_quarantine_next_to_each_store() {
        for (name, quarantine_name) in [
            (DEPLOY_STORAGE_DB, DEPLOY_STORAGE_QUARANTINE_DB),
            (
                REJECTED_DEPLOY_BUFFER_DB,
                REJECTED_DEPLOY_BUFFER_QUARANTINE_DB,
            ),
        ] {
            let mut kvm = InMemoryStoreManager::new();
            let live = kvm.store(name.to_string()).await.unwrap();
            let d1 = deploy(1);
            let (malformed_key, _) = seed_damaged(&live);
            let read = match name {
                DEPLOY_STORAGE_DB => {
                    let mut storage = KeyValueDeployStorage::new(&mut kvm).await.unwrap();
                    storage.add(vec![d1.clone()]).unwrap();
                    storage.read_all().unwrap()
                }
                _ => {
                    let mut buffer = KeyValueRejectedDeployBuffer::new(&mut kvm).await.unwrap();
                    buffer.add(vec![d1.clone()]).unwrap();
                    buffer.read_all().unwrap()
                }
            };
            assert_eq!(read, HashSet::from([d1]));
            let quarantine = kvm.store(quarantine_name.to_string()).await.unwrap();
            assert_eq!(
                quarantine.get_one(&malformed_key).unwrap(),
                Some(MALFORMED_VALUE.to_vec())
            );
        }
    }

    struct FailingStore {
        inner: InMemoryKeyValueStore,
        fail_scans: AtomicBool,
        fail_writes: AtomicBool,
    }

    impl FailingStore {
        fn new(fail_scans: bool, fail_writes: bool) -> Arc<Self> {
            Arc::new(Self {
                inner: InMemoryKeyValueStore::new(),
                fail_scans: AtomicBool::new(fail_scans),
                fail_writes: AtomicBool::new(fail_writes),
            })
        }

        fn check(flag: &AtomicBool) -> Result<(), KvStoreError> {
            if flag.load(Ordering::SeqCst) {
                Err(KvStoreError::IoError("backend unavailable".to_string()))
            } else {
                Ok(())
            }
        }
    }

    impl KeyValueStore for FailingStore {
        fn as_any(&self) -> &dyn std::any::Any { self }

        fn get(&self, keys: &Vec<ByteBuffer>) -> Result<Vec<Option<ByteBuffer>>, KvStoreError> {
            self.inner.get(keys)
        }

        fn put(&self, kv_pairs: Vec<(ByteBuffer, ByteBuffer)>) -> Result<(), KvStoreError> {
            Self::check(&self.fail_writes)?;
            self.inner.put(kv_pairs)
        }

        fn put_one_if_absent(
            &self,
            key: ByteBuffer,
            value: ByteBuffer,
        ) -> Result<bool, KvStoreError> {
            Self::check(&self.fail_writes)?;
            self.inner.put_one_if_absent(key, value)
        }

        fn delete(&self, keys: Vec<ByteBuffer>) -> Result<usize, KvStoreError> {
            Self::check(&self.fail_writes)?;
            self.inner.delete(keys)
        }

        fn iterate(&self, f: fn(ByteBuffer, ByteBuffer)) -> Result<(), KvStoreError> {
            self.inner.iterate(f)
        }

        fn iterate_while(
            &self,
            f: &mut dyn FnMut(ByteBuffer, ByteBuffer) -> Result<bool, KvStoreError>,
        ) -> Result<(), KvStoreError> {
            Self::check(&self.fail_scans)?;
            self.inner.iterate_while(f)
        }

        fn clone_box(&self) -> Box<dyn KeyValueStore> { self.inner.clone_box() }

        fn to_map(&self) -> Result<BTreeMap<ByteBuffer, ByteBuffer>, KvStoreError> {
            Self::check(&self.fail_scans)?;
            self.inner.to_map()
        }

        fn print_store(&self) -> Result<(), KvStoreError> { self.inner.print_store() }

        fn non_empty(&self) -> Result<bool, KvStoreError> { self.inner.non_empty() }

        fn size_bytes(&self) -> usize { self.inner.size_bytes() }
    }

    #[test]
    fn backend_scan_failures_fail_the_operation_without_quarantining() {
        let live = FailingStore::new(false, false);
        let quarantine = in_memory();
        let live_dyn: Arc<dyn KeyValueStore> = live.clone();
        let storage =
            KeyValueDeployStorage::from_stores(live_dyn.clone(), quarantine.clone()).unwrap();
        seed_damaged(&live_dyn);
        live.fail_scans.store(true, Ordering::SeqCst);

        assert!(matches!(storage.read_all(), Err(KvStoreError::IoError(_))));
        assert!(matches!(
            storage.any(|_| Ok(false)),
            Err(KvStoreError::IoError(_))
        ));
        assert!(!quarantine.non_empty().unwrap());
        assert_eq!(live.inner.num_records(), 2);
    }

    #[test]
    fn quarantine_write_failures_fail_the_scan_and_keep_the_record() {
        let live = in_memory();
        let quarantine = FailingStore::new(false, true);
        let storage = KeyValueDeployStorage::from_stores(live.clone(), quarantine.clone()).unwrap();
        storage
            .store
            .put_one(deploy(1).sig.to_vec(), deploy(1))
            .unwrap();
        let (malformed_key, _) = seed_damaged(&live);

        assert!(matches!(storage.read_all(), Err(KvStoreError::IoError(_))));
        assert_eq!(
            live.get_one(&malformed_key).unwrap(),
            Some(MALFORMED_VALUE.to_vec())
        );
    }

    #[test]
    fn logged_error_text_is_bounded() {
        let error = KvStoreError::SerializationError("x".repeat(10_000));
        assert_eq!(
            bounded_error(&error).chars().count(),
            MAX_LOGGED_ERROR_CHARS
        );
    }
}
