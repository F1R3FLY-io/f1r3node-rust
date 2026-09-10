use std::collections::BTreeMap;
use std::sync::{mpsc, Arc};
use std::time::Duration;

use parking_lot::Mutex;
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};
use shared::rust::store::key_value_store::{EntryReader, KeyValueStore, KvStoreError, ValueReader};
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;
use shared::rust::ByteBuffer;

use super::fixture;
use crate::rust::dag::block_metadata_store::BlockMetadataStore;

struct ReadGate {
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
}

#[derive(Clone)]
struct GatedStore {
    inner: Arc<dyn KeyValueStore>,
    gate: Arc<Mutex<Option<ReadGate>>>,
}

impl KeyValueStore for GatedStore {
    fn as_any(&self) -> &dyn std::any::Any { self }

    fn get(&self, keys: &Vec<ByteBuffer>) -> Result<Vec<Option<ByteBuffer>>, KvStoreError> {
        let gate = self.gate.lock().take();
        if let Some(gate) = gate {
            gate.entered.send(()).unwrap();
            gate.release.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        self.inner.get(keys)
    }

    fn with_value(
        &self,
        key: &ByteBuffer,
        reader: &mut ValueReader<'_>,
    ) -> Result<(), KvStoreError> {
        self.inner.with_value(key, reader)
    }

    fn visit_entries(&self, reader: &mut EntryReader<'_>) -> Result<(), KvStoreError> {
        self.inner.visit_entries(reader)
    }

    fn put(&self, pairs: Vec<(ByteBuffer, ByteBuffer)>) -> Result<(), KvStoreError> {
        self.inner.put(pairs)
    }

    fn put_one_if_absent(&self, key: ByteBuffer, value: ByteBuffer) -> Result<bool, KvStoreError> {
        self.inner.put_one_if_absent(key, value)
    }

    fn delete(&self, keys: Vec<ByteBuffer>) -> Result<usize, KvStoreError> {
        self.inner.delete(keys)
    }

    fn iterate(&self, f: fn(ByteBuffer, ByteBuffer)) -> Result<(), KvStoreError> {
        self.inner.iterate(f)
    }

    fn iterate_while(
        &self,
        f: &mut dyn FnMut(ByteBuffer, ByteBuffer) -> Result<bool, KvStoreError>,
    ) -> Result<(), KvStoreError> {
        self.inner.iterate_while(f)
    }

    fn clone_box(&self) -> Box<dyn KeyValueStore> { Box::new(self.clone()) }

    fn to_map(&self) -> Result<BTreeMap<ByteBuffer, ByteBuffer>, KvStoreError> {
        self.inner.to_map()
    }

    fn print_store(&self) -> Result<(), KvStoreError> { self.inner.print_store() }

    fn non_empty(&self) -> Result<bool, KvStoreError> { self.inner.non_empty() }

    fn size_bytes(&self) -> usize { self.inner.size_bytes() }
}

#[tokio::test]
async fn narrow_read_requires_published_valid_metadata() {
    let (_manager, storage, genesis, child) = fixture().await;
    assert!(storage.has_admitted_metadata(&genesis.block_hash).unwrap());
    let index = storage.block_metadata_index.read();
    let metadata = index.get(&child.block_hash).unwrap().unwrap();
    let (key, value) = index.encode_add(&metadata).unwrap();
    index.dag_state().write().dag_set.remove(&child.block_hash);
    drop(index);
    assert!(!storage.has_admitted_metadata(&child.block_hash).unwrap());
    {
        let index = storage.block_metadata_index.read();
        index.raw_store().put_one(key.clone(), vec![0xff]).unwrap();
    }
    assert!(!storage.has_admitted_metadata(&child.block_hash).unwrap());
    {
        let index = storage.block_metadata_index.read();
        index
            .dag_state()
            .write()
            .dag_set
            .insert(child.block_hash.clone());
    }
    assert!(storage.has_admitted_metadata(&child.block_hash).is_err());
    {
        let index = storage.block_metadata_index.read();
        index.raw_store().put_one(key, value).unwrap();
    }
    assert!(storage.has_admitted_metadata(&child.block_hash).unwrap());
    storage
        .block_metadata_index
        .read()
        .delete_kv_row_for_tests(&child.block_hash)
        .unwrap();
    assert!(!storage.has_admitted_metadata(&child.block_hash).unwrap());
}

#[tokio::test]
async fn publication_guard_rejects_missing_admitted_metadata() {
    let (_manager, storage, _genesis, child) = fixture().await;
    storage
        .block_metadata_index
        .read()
        .delete_kv_row_for_tests(&child.block_hash)
        .unwrap();
    assert!(storage
        .get_representation()
        .unwrap()
        .contains(&child.block_hash));
    let invoked = std::cell::Cell::new(false);
    let result = storage.publish_if_unadmitted(&child.block_hash, || {
        invoked.set(true);
        Ok::<_, KvStoreError>(())
    });
    assert!(
        result.is_err(),
        "missing admitted metadata must not authorize publication"
    );
    assert!(!invoked.get());
}

#[tokio::test]
async fn narrow_read_propagates_metadata_key_mismatch() {
    let (_manager, storage, genesis, child) = fixture().await;
    let index = storage.block_metadata_index.read();
    let child_metadata = index.get(&child.block_hash).unwrap().unwrap();
    let genesis_metadata = index.get(&genesis.block_hash).unwrap().unwrap();
    let (child_key, _) = index.encode_add(&child_metadata).unwrap();
    let (_, genesis_value) = index.encode_add(&genesis_metadata).unwrap();
    index.raw_store().put_one(child_key, genesis_value).unwrap();
    drop(index);
    let error = storage
        .has_admitted_metadata(&child.block_hash)
        .unwrap_err();
    assert!(error.to_string().contains("key does not match"), "{error}");
}

#[tokio::test]
async fn narrow_reads_match_generated_row_and_publication_histories() {
    let (_manager, storage, genesis, child) = fixture().await;
    let (key, value, mismatched, raw) = {
        let index = storage.block_metadata_index.read();
        let metadata = index.get(&child.block_hash).unwrap().unwrap();
        let (key, value) = index.encode_add(&metadata).unwrap();
        let (_, mismatched) = index
            .encode_add(&index.get_unsafe(&genesis.block_hash).unwrap())
            .unwrap();
        (key, value, mismatched, index.raw_store().clone())
    };
    let mut runner = TestRunner::new(Config {
        cases: 128,
        ..Config::default()
    });
    runner
        .run(
            &prop::collection::vec((any::<bool>(), 0_u8..4, any::<bool>()), 0..128),
            |operations| {
                let exhaustive = [false, true].into_iter().flat_map(|visible| {
                    (0_u8..4).flat_map(move |row| {
                        [false, true].into_iter().map(move |fail| (visible, row, fail))
                    })
                });
                for (visible, row, fail) in exhaustive.chain(operations) {
                    {
                        let _guard = storage.global_lock.write();
                        let index = storage.block_metadata_index.read();
                        if visible {
                            index
                                .dag_state()
                                .write()
                                .dag_set
                                .insert(child.block_hash.clone());
                        } else {
                            index.dag_state().write().dag_set.remove(&child.block_hash);
                        }
                        match row {
                            0 => {
                                raw.delete(vec![key.clone()]).unwrap();
                            }
                            1 => raw.put_one(key.clone(), value.clone()).unwrap(),
                            2 => raw.put_one(key.clone(), vec![0xff]).unwrap(),
                            _ => raw.put_one(key.clone(), mismatched.clone()).unwrap(),
                        }
                    }
                    let result = storage.has_admitted_metadata(&child.block_hash);
                    match (visible, row) {
                        (false, _) | (true, 0) => prop_assert_eq!(result.unwrap(), false),
                        (true, 1) => prop_assert_eq!(result.unwrap(), true),
                        _ => prop_assert!(result.is_err()),
                    }
                    let invoked = std::cell::Cell::new(false);
                    let publication = storage.publish_if_unadmitted(&child.block_hash, || {
                        invoked.set(true);
                        if fail {
                            Err(KvStoreError::InvalidArgument("publication failure".to_owned()))
                        } else {
                            Ok(17)
                        }
                    });
                    match (visible, row, fail) {
                        (false, _, false) => prop_assert_eq!(publication.unwrap(), Some(17)),
                        (false, _, true) => prop_assert!(matches!(publication,
                            Err(KvStoreError::InvalidArgument(ref message)) if message == "publication failure")),
                        (true, 1, _) => prop_assert_eq!(publication.unwrap(), None),
                        (true, 0, _) => prop_assert!(matches!(publication, Err(KvStoreError::KeyNotFound(_)))),
                        _ => prop_assert!(publication.is_err()),
                    }
                    prop_assert_eq!(invoked.get(), !visible);
                    prop_assert!(storage.global_lock.try_write().is_some());
                }
                Ok(())
            },
        )
        .unwrap();
}

#[tokio::test]
async fn narrow_read_holds_publication_and_index_locks_through_row_read() {
    let (_manager, storage, _genesis, child) = fixture().await;
    let gate = Arc::new(Mutex::new(None));
    let raw = storage.block_metadata_index.read().raw_store().clone();
    let index = BlockMetadataStore::new(KeyValueTypedStoreImpl::new(Arc::new(GatedStore {
        inner: raw,
        gate: gate.clone(),
    })))
    .unwrap();
    *storage.block_metadata_index.write() = index;
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    *gate.lock() = Some(ReadGate {
        entered: entered_tx,
        release: release_rx,
    });
    let reader_storage = storage.clone();
    let hash = child.block_hash.clone();
    let reader = std::thread::spawn(move || reader_storage.has_admitted_metadata(&hash));
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let publication_locked = storage.global_lock.try_write().is_none();
    let index_locked = storage.block_metadata_index.try_write().is_none();
    release_tx.send(()).unwrap();
    let result = reader.join().unwrap().unwrap();
    assert!(
        publication_locked,
        "publication lock released before the row read"
    );
    assert!(
        index_locked,
        "metadata index lock released before the row read"
    );
    assert!(result);
}
