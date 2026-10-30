// Quarantine of undecodable records in the node-local deploy stores. The
// behavior and the operator procedure are in docs/block-storage/README.md.
//
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
        let mut predicate_error = None;
        let mut damaged = Vec::new();
        store.raw_store().iterate_while(&mut |key, value| {
            match decode_record(store, &key, &value) {
                Ok(deploy) => match predicate(&deploy) {
                    Ok(true) => {
                        matched = true;
                        return Ok(false);
                    }
                    Ok(false) => {}
                    Err(error) => {
                        predicate_error = Some(error);
                        return Ok(false);
                    }
                },
                Err(error) => damaged.push(DamagedRecord { key, value, error }),
            }
            Ok(true)
        })?;
        self.isolate(store, damaged)?;
        match predicate_error {
            Some(error) => Err(error),
            None => Ok(matched),
        }
    }

    pub(super) fn restore_readable(&self, store: &DeployTypedStore) -> Result<(), KvStoreError> {
        let quarantined = self.quarantine.to_map()?;
        if quarantined.is_empty() {
            return Ok(());
        }
        let total = quarantined.len();
        let live = store.raw_store();
        let (mut restored, mut duplicates, mut conflicts, mut removed) =
            (0usize, 0usize, 0usize, 0usize);
        for (key, value) in quarantined {
            if decode_record(store, &key, &value).is_err() {
                continue;
            }
            let inserted = live.put_one_if_absent(key.clone(), value.clone())?;
            if inserted {
                restored += 1;
            } else if live.get_one(&key)?.as_ref() != Some(&value) {
                conflicts += 1;
                continue;
            }
            if self.quarantine.delete_if_value(&key, &value)? {
                removed += 1;
                if !inserted {
                    duplicates += 1;
                }
            }
        }
        if restored > 0 {
            metrics::counter!(RESTORED_RECORDS_METRIC, "store" => self.store_name)
                .increment(restored as u64);
        }
        tracing::warn!(
            store = self.store_name,
            restored,
            duplicates_removed = duplicates,
            kept_on_conflict = conflicts,
            remaining = total - removed,
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
        let (mut moved, mut conflicts, mut logged) = (0usize, 0usize, 0usize);
        for record in damaged {
            let preserved = self
                .quarantine
                .put_one_if_absent(record.key.clone(), record.value.clone())?
                || self.quarantine.get_one(&record.key)?.as_ref() == Some(&record.value);
            if !preserved {
                conflicts += 1;
                if logged < MAX_LOGGED_RECORDS_PER_SCAN {
                    logged += 1;
                    tracing::warn!(
                        store = self.store_name,
                        key_prefix = %key_prefix(&record.key),
                        key_len = record.key.len(),
                        value_len = record.value.len(),
                        error = %bounded_error(&record.error),
                        "Kept an undecodable deploy record in place: its quarantine key holds different bytes"
                    );
                }
                continue;
            }
            if !live.delete_if_value(&record.key, &record.value)? {
                continue;
            }
            moved += 1;
            metrics::counter!(QUARANTINED_RECORDS_METRIC, "store" => self.store_name).increment(1);
            if logged < MAX_LOGGED_RECORDS_PER_SCAN {
                logged += 1;
                tracing::warn!(
                    store = self.store_name,
                    key_prefix = %key_prefix(&record.key),
                    key_len = record.key.len(),
                    value_len = record.value.len(),
                    error = %bounded_error(&record.error),
                    "Moved an undecodable deploy record to quarantine"
                );
            }
        }
        if total > logged {
            tracing::warn!(
                store = self.store_name,
                moved,
                kept_on_conflict = conflicts,
                logged,
                "Found more undecodable deploy records than were logged"
            );
        }
        Ok(())
    }
}

fn key_prefix(key: &ByteBuffer) -> String {
    hex::encode(&key[..key.len().min(MAX_LOGGED_KEY_BYTES)])
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
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Barrier, Mutex};

    use crypto::rust::private_key::PrivateKey;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use metrics_util::debugging::{DebugValue, DebuggingRecorder, Snapshotter};
    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

    use super::*;
    use crate::rust::deploy::key_value_deploy_storage::{
        KeyValueDeployStorage, KeyValueRejectedDeployBuffer, DEPLOY_STORAGE_DB,
        DEPLOY_STORAGE_QUARANTINE_DB, REJECTED_DEPLOY_BUFFER_DB,
        REJECTED_DEPLOY_BUFFER_QUARANTINE_DB,
    };

    const MALFORMED_VALUE: &[u8] = &[0xff, 0x01];
    const UNSUPPORTED_FORMAT_VALUE: &[u8] = b"F1R3DEP\x03\x0a\x03Nil\x10\x7b";

    fn deploy(time_stamp: i64) -> Signed<DeployData> {
        signed(DeployData {
            term: "Nil".to_string(),
            time_stamp,
            phlo_price: (1).into(),
            phlo_limit: 100_000,
            valid_after_block_number: 0,
            shard_id: "root".to_string(),
            expiration_timestamp: None,
            parameters: Vec::new(),
        })
    }

    fn stored_deploy() -> Signed<DeployData> {
        signed(DeployData {
            term: "Nil".to_string(),
            time_stamp: 123,
            phlo_price: (7).into(),
            phlo_limit: 100_000,
            valid_after_block_number: 17,
            shard_id: "root".to_string(),
            expiration_timestamp: Some(1000),
            parameters: Vec::new(),
        })
    }

    fn signed(data: DeployData) -> Signed<DeployData> {
        Signed::create(data, Box::new(Secp256k1), PrivateKey::from_bytes(&[1; 32])).unwrap()
    }

    fn stored_bytes() -> ByteBuffer {
        super::super::deploy_codec::encode(&stored_deploy()).unwrap()
    }

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

    struct InterleavingStore {
        inner: Arc<dyn KeyValueStore>,
        before_delete: Mutex<Option<Box<dyn FnOnce() + Send>>>,
        after_scan: Option<Arc<Barrier>>,
        reads: AtomicUsize,
        deletes: AtomicUsize,
    }

    impl InterleavingStore {
        fn new(inner: Arc<dyn KeyValueStore>, after_scan: Option<Arc<Barrier>>) -> Arc<Self> {
            Arc::new(Self {
                inner,
                before_delete: Mutex::new(None),
                after_scan,
                reads: AtomicUsize::new(0),
                deletes: AtomicUsize::new(0),
            })
        }

        fn run_hook(&self) {
            let hook = self.before_delete.lock().unwrap().take();
            if let Some(hook) = hook {
                hook();
            }
        }
    }

    impl KeyValueStore for InterleavingStore {
        fn as_any(&self) -> &dyn std::any::Any { self }

        fn get(&self, keys: &Vec<ByteBuffer>) -> Result<Vec<Option<ByteBuffer>>, KvStoreError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            let values = self.inner.get(keys)?;
            self.run_hook();
            Ok(values)
        }

        fn put(&self, pairs: Vec<(ByteBuffer, ByteBuffer)>) -> Result<(), KvStoreError> {
            self.inner.put(pairs)
        }

        fn put_one_if_absent(
            &self,
            key: ByteBuffer,
            value: ByteBuffer,
        ) -> Result<bool, KvStoreError> {
            self.inner.put_one_if_absent(key, value)
        }

        fn delete(&self, keys: Vec<ByteBuffer>) -> Result<usize, KvStoreError> {
            self.deletes.fetch_add(1, Ordering::SeqCst);
            self.run_hook();
            self.inner.delete(keys)
        }

        fn delete_if_value(
            &self,
            key: &ByteBuffer,
            expected: &ByteBuffer,
        ) -> Result<bool, KvStoreError> {
            self.run_hook();
            self.inner.delete_if_value(key, expected)
        }

        fn iterate(&self, f: fn(ByteBuffer, ByteBuffer)) -> Result<(), KvStoreError> {
            for (key, value) in self.to_map()? {
                f(key, value);
            }
            Ok(())
        }

        fn iterate_while(
            &self,
            f: &mut dyn FnMut(ByteBuffer, ByteBuffer) -> Result<bool, KvStoreError>,
        ) -> Result<(), KvStoreError> {
            for (key, value) in self.to_map()? {
                if !f(key, value)? {
                    break;
                }
            }
            Ok(())
        }

        fn clone_box(&self) -> Box<dyn KeyValueStore> { self.inner.clone_box() }

        fn to_map(&self) -> Result<BTreeMap<ByteBuffer, ByteBuffer>, KvStoreError> {
            let records = self.inner.to_map()?;
            if let Some(barrier) = &self.after_scan {
                barrier.wait();
            }
            Ok(records)
        }

        fn print_store(&self) -> Result<(), KvStoreError> { self.inner.print_store() }

        fn non_empty(&self) -> Result<bool, KvStoreError> { self.inner.non_empty() }

        fn size_bytes(&self) -> usize { self.inner.size_bytes() }
    }

    #[test]
    fn scans_preserve_a_live_record_replaced_before_deletion() {
        for name in POOLS {
            for use_any in [false, true] {
                let inner = in_memory();
                let live = InterleavingStore::new(inner.clone(), None);
                let quarantine = in_memory();
                let pool = Pool::open(name, &(live.clone() as Arc<dyn KeyValueStore>), &quarantine);
                let valid = deploy(1);
                let key = key_for(&valid.sig);
                inner
                    .put_one(key.clone(), MALFORMED_VALUE.to_vec())
                    .unwrap();
                let replacement = super::super::deploy_codec::encode(&valid).unwrap();
                let replacement_key = key.clone();
                let replacement_bytes = replacement.clone();
                let writer = inner.clone();
                *live.before_delete.lock().unwrap() = Some(Box::new(move || {
                    writer.put_one(replacement_key, replacement_bytes).unwrap();
                }));
                let recorder = DebuggingRecorder::new();
                let snapshotter = recorder.snapshotter();
                metrics::with_local_recorder(&recorder, || {
                    if use_any {
                        match &pool {
                            Pool::Storage(storage) => assert!(!storage.any(|_| Ok(false)).unwrap()),
                            Pool::Buffer(buffer) => assert!(!buffer.any(|_| Ok(false)).unwrap()),
                        }
                    } else {
                        assert!(pool.read_all().unwrap().is_empty());
                    }
                });
                assert_eq!(inner.get_one(&key).unwrap(), Some(replacement));
                assert_eq!(
                    quarantine.get_one(&key).unwrap(),
                    Some(MALFORMED_VALUE.to_vec())
                );
                assert_eq!(counter(&snapshotter, QUARANTINED_RECORDS_METRIC, name), 0);
                assert_eq!(pool.read_all().unwrap(), HashSet::from([valid]));
            }
        }
    }

    #[test]
    fn concurrent_scans_count_one_actual_quarantine_move() {
        let inner = in_memory();
        let live = InterleavingStore::new(inner.clone(), Some(Arc::new(Barrier::new(2))));
        let quarantine = in_memory();
        let storage = KeyValueDeployStorage::from_stores(live, quarantine.clone()).unwrap();
        let key = key_for(&deploy(1).sig);
        inner
            .put_one(key.clone(), MALFORMED_VALUE.to_vec())
            .unwrap();
        let handles: Vec<_> = [false, true]
            .into_iter()
            .map(|use_any| {
                let storage = storage.clone();
                std::thread::spawn(move || {
                    let recorder = DebuggingRecorder::new();
                    let snapshotter = recorder.snapshotter();
                    metrics::with_local_recorder(&recorder, || {
                        if use_any {
                            assert!(!storage.any(|_| Ok(false)).unwrap());
                        } else {
                            assert!(storage.read_all().unwrap().is_empty());
                        }
                    });
                    counter(&snapshotter, QUARANTINED_RECORDS_METRIC, DEPLOY_STORAGE_DB)
                })
            })
            .collect();
        assert_eq!(
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .sum::<u64>(),
            1
        );
        assert_eq!(inner.get_one(&key).unwrap(), None);
        assert_eq!(
            quarantine.get_one(&key).unwrap(),
            Some(MALFORMED_VALUE.to_vec())
        );
    }

    #[test]
    fn restore_preserves_quarantined_bytes_replaced_before_cleanup() {
        let live = in_memory();
        let inner = in_memory();
        let quarantine = InterleavingStore::new(inner.clone(), None);
        let valid = deploy(1);
        let key = key_for(&valid.sig);
        let encoded = super::super::deploy_codec::encode(&valid).unwrap();
        inner.put_one(key.clone(), encoded.clone()).unwrap();
        let writer = inner.clone();
        let replacement_key = key.clone();
        *quarantine.before_delete.lock().unwrap() = Some(Box::new(move || {
            writer
                .put_one(replacement_key, MALFORMED_VALUE.to_vec())
                .unwrap();
        }));
        KeyValueDeployStorage::from_stores(live.clone(), quarantine).unwrap();
        assert_eq!(live.get_one(&key).unwrap(), Some(encoded));
        assert_eq!(inner.get_one(&key).unwrap(), Some(MALFORMED_VALUE.to_vec()));
    }

    #[test]
    fn signature_removal_uses_one_delete_without_a_presence_read() {
        let live = InterleavingStore::new(in_memory(), None);
        let storage = KeyValueDeployStorage::from_stores(live.clone(), in_memory()).unwrap();
        let valid = deploy(1);
        storage.add(vec![valid.clone()]).unwrap();
        assert!(storage.remove_by_sig(&valid.sig).unwrap());
        assert!(!storage.remove_by_sig(&valid.sig).unwrap());
        assert_eq!(live.reads.load(Ordering::SeqCst), 0);
        assert_eq!(live.deletes.load(Ordering::SeqCst), 2);
    }

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
    fn read_all_keeps_valid_and_stored_records_and_quarantines_damaged_ones() {
        for name in POOLS {
            let (live, quarantine) = (in_memory(), in_memory());
            let mut pool = Pool::open(name, &live, &quarantine);
            let (d1, d2, stored) = (deploy(1), deploy(2), stored_deploy());
            pool.add(vec![d1.clone(), d2.clone()]);
            live.put_one(key_for(&stored.sig), stored_bytes()).unwrap();
            let (malformed_key, unsupported_key) = seed_damaged(&live);

            let expected = HashSet::from([d1, d2, stored]);
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
    fn read_all_quarantines_a_versioned_record_with_an_invalid_signature() {
        for name in POOLS {
            let (live, quarantine) = (in_memory(), in_memory());
            let mut pool = Pool::open(name, &live, &quarantine);
            let valid = deploy(1);
            pool.add(vec![valid.clone()]);
            let mut tampered = deploy(2);
            let tampered_key = key_for(&tampered.sig);
            tampered.data.phlo_limit += 1;
            let tampered_bytes = crate::rust::deploy::deploy_codec::encode(&tampered).unwrap();
            live.put_one(tampered_key.clone(), tampered_bytes.clone())
                .unwrap();

            assert_eq!(pool.read_all().unwrap(), HashSet::from([valid]), "{name}");
            assert_eq!(
                quarantine.get_one(&tampered_key).unwrap(),
                Some(tampered_bytes),
                "{name}"
            );
        }
    }
    #[test]
    fn any_skips_damaged_records_and_still_finds_matches() {
        let (live, quarantine) = (in_memory(), in_memory());
        let storage = KeyValueDeployStorage::from_stores(live.clone(), quarantine.clone()).unwrap();
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
                let stored = stored_deploy();
                let stored_key = key_for(&stored.sig);
                quarantine
                    .put_one(stored_key.clone(), stored_bytes())
                    .unwrap();
                let (malformed_key, _) = seed_damaged(&quarantine);

                let pool = Pool::open(name, &live, &quarantine);

                assert_eq!(pool.read_all().unwrap(), HashSet::from([stored]), "{name}");
                assert_eq!(quarantine.get_one(&stored_key).unwrap(), None, "{name}");
                assert!(
                    quarantine.get_one(&malformed_key).unwrap().is_some(),
                    "{name}"
                );
                assert_eq!(counter(&snapshotter, RESTORED_RECORDS_METRIC, name), 1);
            }
        });
    }

    #[test]
    fn open_keeps_a_readable_quarantined_copy_when_the_live_key_holds_other_bytes() {
        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        metrics::with_local_recorder(&recorder, || {
            let stored = stored_deploy();
            let stored_key = key_for(&stored.sig);
            for live_bytes in [None, Some(MALFORMED_VALUE.to_vec())] {
                let (live, quarantine) = (in_memory(), in_memory());
                match &live_bytes {
                    Some(bytes) => live.put_one(stored_key.clone(), bytes.clone()).unwrap(),
                    None => {
                        let mut bytes = stored_bytes();
                        bytes.extend_from_slice(&[0xf8, 0x07, 0x01]);
                        live.put_one(stored_key.clone(), bytes).unwrap();
                    }
                }
                let live_before = live.get_one(&stored_key).unwrap();
                quarantine
                    .put_one(stored_key.clone(), stored_bytes())
                    .unwrap();

                let reopened =
                    KeyValueDeployStorage::from_stores(live.clone(), quarantine.clone()).unwrap();
                reopened.read_all().unwrap();

                assert_eq!(live.get_one(&stored_key).unwrap(), live_before);
                assert_eq!(
                    quarantine.get_one(&stored_key).unwrap(),
                    Some(stored_bytes())
                );
            }
            assert_eq!(
                counter(&snapshotter, RESTORED_RECORDS_METRIC, DEPLOY_STORAGE_DB),
                0
            );
        });
    }

    #[test]
    fn open_removes_a_quarantined_copy_identical_to_the_live_record() {
        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        metrics::with_local_recorder(&recorder, || {
            let (live, quarantine) = (in_memory(), in_memory());
            let stored = stored_deploy();
            let stored_key = key_for(&stored.sig);
            live.put_one(stored_key.clone(), stored_bytes()).unwrap();
            quarantine
                .put_one(stored_key.clone(), stored_bytes())
                .unwrap();

            KeyValueDeployStorage::from_stores(live.clone(), quarantine.clone()).unwrap();

            assert_eq!(live.get_one(&stored_key).unwrap(), Some(stored_bytes()));
            assert!(!quarantine.non_empty().unwrap());
            assert_eq!(
                counter(&snapshotter, RESTORED_RECORDS_METRIC, DEPLOY_STORAGE_DB),
                0
            );
        });
    }

    #[test]
    fn scan_keeps_a_damaged_record_when_its_quarantine_key_holds_different_bytes() {
        for name in POOLS {
            let (live, quarantine) = (in_memory(), in_memory());
            let pool = Pool::open(name, &live, &quarantine);
            let (malformed_key, _) = seed_damaged(&live);
            quarantine
                .put_one(malformed_key.clone(), vec![0xee])
                .unwrap();

            assert!(pool.read_all().unwrap().is_empty(), "{name}");
            assert_eq!(
                quarantine.get_one(&malformed_key).unwrap(),
                Some(vec![0xee]),
                "{name}"
            );
            assert_eq!(
                live.get_one(&malformed_key).unwrap(),
                Some(MALFORMED_VALUE.to_vec()),
                "{name}"
            );
        }
    }

    #[test]
    fn scan_completes_a_move_interrupted_after_the_quarantine_write() {
        for name in POOLS {
            let (live, quarantine) = (in_memory(), in_memory());
            let pool = Pool::open(name, &live, &quarantine);
            let (malformed_key, _) = seed_damaged(&live);
            quarantine
                .put_one(malformed_key.clone(), MALFORMED_VALUE.to_vec())
                .unwrap();

            assert!(pool.read_all().unwrap().is_empty(), "{name}");
            assert_eq!(live.get_one(&malformed_key).unwrap(), None, "{name}");
            assert_eq!(
                quarantine.get_one(&malformed_key).unwrap(),
                Some(MALFORMED_VALUE.to_vec()),
                "{name}"
            );
        }
    }

    #[test]
    fn any_quarantines_damaged_records_seen_before_a_predicate_error() {
        let live = FailingStore::new(false, false);
        let live_dyn: Arc<dyn KeyValueStore> = live.clone();
        let quarantine = in_memory();
        let storage =
            KeyValueDeployStorage::from_stores(live_dyn.clone(), quarantine.clone()).unwrap();
        let valid = deploy(1);
        storage.add(vec![valid.clone()]).unwrap();
        let malformed_key = key_for(&[0; 64]);
        live.put_one(malformed_key.clone(), MALFORMED_VALUE.to_vec())
            .unwrap();
        assert!(malformed_key < key_for(&valid.sig));

        assert!(matches!(
            storage.any(|_| Err(KvStoreError::InvalidArgument("predicate".to_string()))),
            Err(KvStoreError::InvalidArgument(_))
        ));
        assert_eq!(
            quarantine.get_one(&malformed_key).unwrap(),
            Some(MALFORMED_VALUE.to_vec())
        );
        assert_eq!(live.get_one(&malformed_key).unwrap(), None);
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
                    let storage = KeyValueDeployStorage::new(&mut kvm).await.unwrap();
                    storage.add(vec![d1.clone()]).unwrap();
                    storage.read_all().unwrap()
                }
                _ => {
                    let buffer = KeyValueRejectedDeployBuffer::new(&mut kvm).await.unwrap();
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

        fn delete_if_value(&self, key: &Vec<u8>, expected: &Vec<u8>) -> Result<bool, KvStoreError> {
            Self::check(&self.fail_writes)?;
            self.inner.delete_if_value(key, expected)
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
            for (key, value) in self.inner.to_map()? {
                if !f(key, value)? {
                    break;
                }
            }
            Ok(())
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
    fn conditional_delete_failures_preserve_both_copies_until_retry() {
        let live = FailingStore::new(false, false);
        let quarantine = in_memory();
        let storage = KeyValueDeployStorage::from_stores(live.clone(), quarantine.clone()).unwrap();
        let key = key_for(&deploy(1).sig);
        live.inner
            .put_one(key.clone(), MALFORMED_VALUE.to_vec())
            .unwrap();
        live.fail_writes.store(true, Ordering::SeqCst);
        assert!(matches!(storage.read_all(), Err(KvStoreError::IoError(_))));
        assert_eq!(
            live.inner.get_one(&key).unwrap(),
            Some(MALFORMED_VALUE.to_vec())
        );
        assert_eq!(
            quarantine.get_one(&key).unwrap(),
            Some(MALFORMED_VALUE.to_vec())
        );
        live.fail_writes.store(false, Ordering::SeqCst);
        assert!(storage.read_all().unwrap().is_empty());
        assert_eq!(live.inner.get_one(&key).unwrap(), None);
        assert_eq!(
            quarantine.get_one(&key).unwrap(),
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
