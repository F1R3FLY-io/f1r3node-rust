use std::collections::BTreeMap;
use std::sync::Arc;

use heed::types::{Bytes, SerdeBincode};
use heed::{Database, Env, Error as HeedError, MdbError, PutFlags};

use super::key_value_store::{
    AtomicStoreMutation, AtomicStoreOperation, KeyValueStore, KvStoreError,
};
use crate::rust::ByteBuffer;

// `heed::Database` is a `Copy` handle (a `u32` dbi) and is `Send + Sync`; it
// carries no mutable state. It was previously wrapped in `Arc<Mutex<Database>>`,
// which forced every read to take a blocking `std::sync::Mutex` and serialised
// all history-store reads across concurrent par-branches — the dominant
// serialisation point on the LMDB-backed node (CPU stuck at ~2 cores during
// intra-deploy parallel execution). LMDB is MVCC: independent read
// transactions run concurrently, and writers are already serialised by LMDB's
// own single-writer lock inside `env.write_txn()`. The Mutex was therefore
// unnecessary and is removed so reads proceed in parallel.
pub struct LmdbKeyValueStore {
    pub env: Arc<Env>,
    pub db: Database<SerdeBincode<ByteBuffer>, SerdeBincode<ByteBuffer>>,
}

fn in_blocking<T>(f: impl FnOnce() -> T) -> T {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(f)
        }
        _ => f(),
    }
}

impl KeyValueStore for LmdbKeyValueStore {
    fn as_any(&self) -> &dyn std::any::Any { self }

    fn get(&self, keys: &Vec<ByteBuffer>) -> Result<Vec<Option<ByteBuffer>>, KvStoreError> {
        in_blocking(|| {
            let reader = self.env.read_txn()?;
            let results = keys
                .iter()
                .map(|key| self.db.get(&reader, key).map_err(|e| e.into()))
                .collect();
            drop(reader);
            results
        })
    }

    fn put(&self, kv_pairs: Vec<(ByteBuffer, ByteBuffer)>) -> Result<(), KvStoreError> {
        in_blocking(|| {
            let mut writer = self.env.write_txn()?;
            for (key, value) in kv_pairs {
                self.db.put(&mut writer, &key, &value)?;
            }
            writer.commit()?;

            Ok(())
        })
    }

    fn put_one_if_absent(&self, key: ByteBuffer, value: ByteBuffer) -> Result<bool, KvStoreError> {
        in_blocking(|| {
            let mut writer = self.env.write_txn()?;
            match self
                .db
                .put_with_flags(&mut writer, PutFlags::NO_OVERWRITE, &key, &value)
            {
                Ok(()) => {
                    writer.commit()?;
                    Ok(true)
                }
                Err(HeedError::Mdb(MdbError::KeyExist)) => Ok(false),
                Err(error) => Err(error.into()),
            }
        })
    }

    fn delete(&self, keys: Vec<ByteBuffer>) -> Result<usize, KvStoreError> {
        in_blocking(|| {
            let mut writer = self.env.write_txn()?;
            let mut delete_count = 0;
            for key in &keys {
                if self.db.delete(&mut writer, key)? {
                    delete_count += 1;
                }
            }
            writer.commit()?;
            Ok(delete_count)
        })
    }

    fn iterate(&self, f: fn(ByteBuffer, ByteBuffer)) -> Result<(), KvStoreError> {
        in_blocking(|| {
            let reader = self.env.read_txn()?;
            let iter = self.db.iter(&reader)?;
            for result in iter {
                let (key, value) = result?;
                f(key.to_vec(), value);
            }
            drop(reader);
            Ok(())
        })
    }

    fn iterate_while(
        &self,
        f: &mut dyn FnMut(ByteBuffer, ByteBuffer) -> Result<bool, KvStoreError>,
    ) -> Result<(), KvStoreError> {
        in_blocking(|| {
            let reader = self.env.read_txn()?;
            let iter = self.db.iter(&reader)?;
            for result in iter {
                let (key, value) = result?;
                if !f(key.to_vec(), value)? {
                    break;
                }
            }
            drop(reader);
            Ok(())
        })
    }

    fn clone_box(&self) -> Box<dyn KeyValueStore> { Box::new(self.clone()) }

    fn to_map(&self) -> Result<BTreeMap<ByteBuffer, ByteBuffer>, KvStoreError> {
        in_blocking(|| {
            let reader = self.env.read_txn()?;
            let iter = self.db.iter(&reader)?;
            let mut map = BTreeMap::new();
            for result in iter {
                let (key, value) = result?;
                map.insert(key.to_vec(), value);
            }
            drop(reader);
            Ok(map)
        })
    }

    fn scan_prefix(&self, prefix: &[u8]) -> Result<Vec<(ByteBuffer, ByteBuffer)>, KvStoreError> {
        in_blocking(|| {
            let reader = self.env.read_txn()?;
            let iter = self.db.iter(&reader)?;
            let mut rows = Vec::new();
            for result in iter {
                let (key, value) = result?;
                if key.starts_with(prefix) {
                    rows.push((key.to_vec(), value));
                }
            }
            drop(reader);
            rows.sort_by(|left, right| left.0.cmp(&right.0));
            Ok(rows)
        })
    }

    fn scan_prefix_exact_len(
        &self,
        prefix: &[u8],
        key_length: usize,
    ) -> Result<Vec<(ByteBuffer, ByteBuffer)>, KvStoreError> {
        if prefix.len() > key_length {
            return Ok(Vec::new());
        }
        in_blocking(|| {
            let reader = self.env.read_txn()?;
            let raw_db = self.db.remap_key_type::<Bytes>();
            let encoded_key = bincode::serialize(&vec![0u8; key_length])?;
            let header_length = encoded_key.len().checked_sub(key_length).ok_or_else(|| {
                KvStoreError::SerializationError(
                    "LMDB composite key encoding is shorter than its payload".to_string(),
                )
            })?;
            let mut encoded_prefix = encoded_key[..header_length].to_vec();
            encoded_prefix.extend_from_slice(prefix);
            let iter = raw_db.prefix_iter(&reader, encoded_prefix.as_slice())?;
            let mut rows = Vec::new();
            for result in iter {
                let (encoded, value) = result?;
                let key: Vec<u8> = bincode::deserialize(encoded)?;
                if key.len() == key_length && key.starts_with(prefix) {
                    rows.push((key, value));
                }
            }
            drop(reader);
            Ok(rows)
        })
    }

    fn strict_atomic_mutate(
        &self,
        mutations: &[AtomicStoreMutation<'_>],
    ) -> Result<(), KvStoreError> {
        in_blocking(|| {
            let stores = mutations
                .iter()
                .map(|mutation| {
                    mutation
                        .store
                        .as_any()
                        .downcast_ref::<LmdbKeyValueStore>()
                        .ok_or_else(|| {
                            KvStoreError::AtomicityUnavailable(
                                "strict LMDB transaction includes a non-LMDB store".to_string(),
                            )
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            if stores
                .iter()
                .any(|store| !Arc::ptr_eq(&self.env, &store.env))
            {
                return Err(KvStoreError::AtomicityUnavailable(
                    "strict LMDB transaction spans multiple environments".to_string(),
                ));
            }
            let mut writer = self.env.write_txn()?;
            for (mutation, store) in mutations.iter().zip(stores) {
                let current = store.db.get(&writer, &mutation.key)?;
                match &mutation.operation {
                    AtomicStoreOperation::Put(value) => {
                        store.db.put(&mut writer, &mutation.key, value)?;
                    }
                    AtomicStoreOperation::PutIfAbsentOrEqual(value) => match current {
                        Some(existing) if existing != *value => {
                            return Err(KvStoreError::TransactionConflict(format!(
                                "existing value differs for key {}",
                                hex::encode(&mutation.key)
                            )));
                        }
                        Some(_) => {}
                        None => store.db.put(&mut writer, &mutation.key, value)?,
                    },
                    AtomicStoreOperation::Delete => {
                        store.db.delete(&mut writer, &mutation.key)?;
                    }
                    AtomicStoreOperation::CompareAndSwap {
                        expected,
                        replacement,
                    } => {
                        if current.as_ref() != expected.as_ref() {
                            return Err(KvStoreError::TransactionConflict(format!(
                                "compare-and-swap expectation failed for key {}",
                                hex::encode(&mutation.key)
                            )));
                        }
                        match replacement {
                            Some(value) => store.db.put(&mut writer, &mutation.key, value)?,
                            None => {
                                store.db.delete(&mut writer, &mutation.key)?;
                            }
                        }
                    }
                }
            }
            writer.commit()?;
            Ok(())
        })
    }

    // This is only needed for testing purposes
    fn size_bytes(&self) -> usize { todo!() }

    fn print_store(&self) -> Result<(), KvStoreError> {
        let kv_store_map = self.to_map()?;

        for (key, value) in &kv_store_map {
            println!(
                "Key: {:?}, Value: {:?}",
                hex::encode(key),
                hex::encode(value)
            );
        }

        Ok(())
    }

    fn non_empty(&self) -> Result<bool, KvStoreError> {
        let reader = self.env.read_txn()?;
        let has_first = {
            let mut iter = self.db.iter(&reader)?;
            iter.next().is_some()
        };
        drop(reader);
        Ok(has_first)
    }
}

/// One store's worth of key/value pairs to write, paired with the store to write them to.
pub type StoreWrite<'a> = (&'a dyn KeyValueStore, Vec<(ByteBuffer, ByteBuffer)>);

type LmdbStoreWrite<'a> = (&'a LmdbKeyValueStore, &'a Vec<(ByteBuffer, ByteBuffer)>);

/// Writes to multiple stores in a single LMDB write transaction when they
/// are all `LmdbKeyValueStore`s backed by the same `Env` — LMDB serialises
/// all writers on one environment-wide lock (see the module comment above),
/// so combining N separate `put()` calls (N lock acquisitions) into one
/// transaction (one lock acquisition) directly reduces contention on that
/// lock under concurrent block processing. Falls back to independent
/// `put()` calls — preserving prior behavior exactly — when any store isn't
/// an `LmdbKeyValueStore` (e.g. an in-memory test double) or the stores
/// don't all share one `Env`. The batched path writes via `db.put` directly
/// rather than through `LmdbKeyValueStore::put`, but the two are equivalent:
/// `put` is itself a plain `write_txn` + `db.put` loop + `commit`, with no
/// additional map-full/resize handling to lose.
pub fn batched_put(writes: Vec<StoreWrite<'_>>) -> Result<(), KvStoreError> {
    let lmdb_stores: Option<Vec<LmdbStoreWrite<'_>>> = writes
        .iter()
        .map(|(store, kv_pairs)| {
            store
                .as_any()
                .downcast_ref::<LmdbKeyValueStore>()
                .map(|s| (s, kv_pairs))
        })
        .collect();

    let batchable_stores = match &lmdb_stores {
        Some(stores)
            if stores
                .windows(2)
                .all(|w| Arc::ptr_eq(&w[0].0.env, &w[1].0.env)) =>
        {
            Some(stores)
        }
        _ => None,
    };

    if let Some(stores) = batchable_stores {
        let Some((first, _)) = stores.first() else {
            return Ok(());
        };
        if stores.iter().all(|(_, kv_pairs)| kv_pairs.is_empty()) {
            return Ok(());
        }
        let mut wtxn = first.env.write_txn()?;
        for (store, kv_pairs) in stores {
            for (key, value) in kv_pairs.iter() {
                store.db.put(&mut wtxn, key, value)?;
            }
        }
        wtxn.commit()?;
        Ok(())
    } else {
        for (store, kv_pairs) in writes {
            store.put(kv_pairs)?;
        }
        Ok(())
    }
}

impl LmdbKeyValueStore {
    pub fn new(
        env: Arc<Env>,
        db: Database<SerdeBincode<ByteBuffer>, SerdeBincode<ByteBuffer>>,
    ) -> Self {
        LmdbKeyValueStore { env, db }
    }
}

impl Clone for LmdbKeyValueStore {
    fn clone(&self) -> Self {
        Self {
            db: self.db,
            env: self.env.clone(),
        }
    }
}

#[cfg(test)]
mod batched_put_tests {
    use std::collections::BTreeMap;
    use std::ops::Deref;

    use heed::EnvOpenOptions;
    use tempfile::TempDir;

    use super::*;
    use crate::rust::store::key_value_store::strict_atomic_mutate;

    pub(super) struct TestEnv {
        env: Arc<Env>,
        _dir: TempDir,
    }

    impl Deref for TestEnv {
        type Target = Arc<Env>;

        fn deref(&self) -> &Self::Target { &self.env }
    }

    pub(super) fn open_env() -> TestEnv {
        let scratch = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("target/shared-test-scratch");
        std::fs::create_dir_all(&scratch).unwrap();
        let dir = tempfile::Builder::new()
            .prefix("lmdb-")
            .tempdir_in(scratch)
            .unwrap();
        let mut builder = EnvOpenOptions::new();
        builder.map_size(10 * 1024 * 1024);
        builder.max_dbs(4);
        let env = unsafe { builder.open(dir.path()).unwrap() };
        TestEnv {
            env: Arc::new(env),
            _dir: dir,
        }
    }

    pub(super) fn open_store(env: &Arc<Env>, name: &str) -> LmdbKeyValueStore {
        let mut wtxn = env.write_txn().unwrap();
        let db = env.create_database(&mut wtxn, Some(name)).unwrap();
        wtxn.commit().unwrap();
        LmdbKeyValueStore::new(env.clone(), db)
    }

    pub(super) fn kv(pairs: &[(&str, &str)]) -> Vec<(ByteBuffer, ByteBuffer)> {
        pairs
            .iter()
            .map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec()))
            .collect()
    }

    #[derive(Clone)]
    struct InMemStore {
        map: Arc<std::sync::Mutex<BTreeMap<ByteBuffer, ByteBuffer>>>,
    }

    impl InMemStore {
        fn new() -> Self {
            Self {
                map: Arc::new(std::sync::Mutex::new(BTreeMap::new())),
            }
        }
    }

    impl KeyValueStore for InMemStore {
        fn as_any(&self) -> &dyn std::any::Any { self }

        fn get(&self, keys: &Vec<ByteBuffer>) -> Result<Vec<Option<ByteBuffer>>, KvStoreError> {
            let map = self.map.lock().unwrap();
            Ok(keys.iter().map(|k| map.get(k).cloned()).collect())
        }

        fn put(&self, kv_pairs: Vec<(ByteBuffer, ByteBuffer)>) -> Result<(), KvStoreError> {
            let mut map = self.map.lock().unwrap();
            for (k, v) in kv_pairs {
                map.insert(k, v);
            }
            Ok(())
        }

        fn put_one_if_absent(
            &self,
            key: ByteBuffer,
            value: ByteBuffer,
        ) -> Result<bool, KvStoreError> {
            let mut map = self.map.lock().unwrap();
            match map.entry(key) {
                std::collections::btree_map::Entry::Occupied(_) => Ok(false),
                std::collections::btree_map::Entry::Vacant(e) => {
                    e.insert(value);
                    Ok(true)
                }
            }
        }

        fn delete(&self, keys: Vec<ByteBuffer>) -> Result<usize, KvStoreError> {
            let mut map = self.map.lock().unwrap();
            Ok(keys.iter().filter(|k| map.remove(*k).is_some()).count())
        }

        fn iterate(&self, _f: fn(ByteBuffer, ByteBuffer)) -> Result<(), KvStoreError> { Ok(()) }

        fn iterate_while(
            &self,
            _f: &mut dyn FnMut(ByteBuffer, ByteBuffer) -> Result<bool, KvStoreError>,
        ) -> Result<(), KvStoreError> {
            Ok(())
        }

        fn clone_box(&self) -> Box<dyn KeyValueStore> { Box::new(self.clone()) }

        fn to_map(&self) -> Result<BTreeMap<ByteBuffer, ByteBuffer>, KvStoreError> {
            Ok(self.map.lock().unwrap().clone())
        }

        fn print_store(&self) -> Result<(), KvStoreError> { Ok(()) }

        fn non_empty(&self) -> Result<bool, KvStoreError> {
            Ok(!self.map.lock().unwrap().is_empty())
        }

        fn size_bytes(&self) -> usize { 0 }
    }

    #[test]
    fn same_env_writes_land_in_one_transaction() {
        let env = open_env();
        let a = open_store(&env, "a");
        let b = open_store(&env, "b");

        batched_put(vec![(&a, kv(&[("k1", "v1")])), (&b, kv(&[("k2", "v2")]))]).unwrap();

        assert_eq!(a.get_one(&b"k1".to_vec()).unwrap(), Some(b"v1".to_vec()));
        assert_eq!(b.get_one(&b"k2".to_vec()).unwrap(), Some(b"v2".to_vec()));
    }

    #[test]
    fn cross_env_falls_back_to_independent_puts() {
        let env_a = open_env();
        let env_b = open_env();
        let a = open_store(&env_a, "a");
        let b = open_store(&env_b, "b");

        batched_put(vec![(&a, kv(&[("k1", "v1")])), (&b, kv(&[("k2", "v2")]))]).unwrap();

        assert_eq!(a.get_one(&b"k1".to_vec()).unwrap(), Some(b"v1".to_vec()));
        assert_eq!(b.get_one(&b"k2".to_vec()).unwrap(), Some(b"v2".to_vec()));
    }

    #[test]
    fn non_lmdb_store_falls_back_to_independent_puts() {
        let env = open_env();
        let a = open_store(&env, "a");
        let mem = InMemStore::new();

        batched_put(vec![(&a, kv(&[("k1", "v1")])), (&mem, kv(&[("k2", "v2")]))]).unwrap();

        assert_eq!(a.get_one(&b"k1".to_vec()).unwrap(), Some(b"v1".to_vec()));
        assert_eq!(mem.get_one(&b"k2".to_vec()).unwrap(), Some(b"v2".to_vec()));
    }

    #[test]
    fn empty_input_is_a_noop() { batched_put(vec![]).unwrap(); }

    #[test]
    fn strict_transaction_commits_same_environment_mutations() {
        let env = open_env();
        let a = open_store(&env, "strict-a");
        let b = open_store(&env, "strict-b");
        let mutations = [
            AtomicStoreMutation {
                store: &a,
                key: b"a".to_vec(),
                operation: AtomicStoreOperation::PutIfAbsentOrEqual(b"one".to_vec()),
            },
            AtomicStoreMutation {
                store: &b,
                key: b"b".to_vec(),
                operation: AtomicStoreOperation::CompareAndSwap {
                    expected: None,
                    replacement: Some(b"two".to_vec()),
                },
            },
        ];

        strict_atomic_mutate(&mutations).unwrap();

        assert_eq!(a.get_one(&b"a".to_vec()).unwrap(), Some(b"one".to_vec()));
        assert_eq!(b.get_one(&b"b".to_vec()).unwrap(), Some(b"two".to_vec()));
    }

    #[test]
    fn strict_transaction_rolls_back_every_mutation_on_conflict() {
        let env = open_env();
        let a = open_store(&env, "rollback-a");
        let b = open_store(&env, "rollback-b");
        a.put_one(b"guard".to_vec(), b"current".to_vec()).unwrap();
        let mutations = [
            AtomicStoreMutation {
                store: &b,
                key: b"uncommitted".to_vec(),
                operation: AtomicStoreOperation::Put(b"value".to_vec()),
            },
            AtomicStoreMutation {
                store: &a,
                key: b"guard".to_vec(),
                operation: AtomicStoreOperation::CompareAndSwap {
                    expected: Some(b"stale".to_vec()),
                    replacement: Some(b"next".to_vec()),
                },
            },
        ];

        assert!(matches!(
            strict_atomic_mutate(&mutations),
            Err(KvStoreError::TransactionConflict(_))
        ));
        assert_eq!(
            a.get_one(&b"guard".to_vec()).unwrap(),
            Some(b"current".to_vec())
        );
        assert_eq!(b.get_one(&b"uncommitted".to_vec()).unwrap(), None);
    }

    #[test]
    fn strict_transaction_rejects_cross_environment_mutations() {
        let env_a = open_env();
        let env_b = open_env();
        let a = open_store(&env_a, "cross-a");
        let b = open_store(&env_b, "cross-b");
        let mutations = [
            AtomicStoreMutation {
                store: &a,
                key: b"a".to_vec(),
                operation: AtomicStoreOperation::Put(b"one".to_vec()),
            },
            AtomicStoreMutation {
                store: &b,
                key: b"b".to_vec(),
                operation: AtomicStoreOperation::Put(b"two".to_vec()),
            },
        ];

        assert!(matches!(
            strict_atomic_mutate(&mutations),
            Err(KvStoreError::AtomicityUnavailable(_))
        ));
        assert_eq!(a.get_one(&b"a".to_vec()).unwrap(), None);
        assert_eq!(b.get_one(&b"b".to_vec()).unwrap(), None);
    }

    #[test]
    fn prefix_scan_is_isolated_and_lexicographically_ordered() {
        let env = open_env();
        let store = open_store(&env, "prefix");
        store
            .put(kv(&[("p/2", "two"), ("other", "x"), ("p/1", "one")]))
            .unwrap();

        let rows = store.scan_prefix(b"p/").unwrap();

        assert_eq!(rows, kv(&[("p/1", "one"), ("p/2", "two")]));
    }

    #[test]
    fn exact_length_prefix_scan_uses_the_encoded_composite_key_prefix() {
        let env = open_env();
        let store = open_store(&env, "composite-prefix");
        store
            .put(vec![
                (vec![1, 7, 9, 1], vec![1]),
                (vec![1, 7, 9, 0], vec![2]),
                (vec![1, 7, 9], vec![3]),
                (vec![1, 8, 9, 0], vec![4]),
            ])
            .unwrap();

        let rows = store.scan_prefix_exact_len(&[1, 7], 4).unwrap();

        assert_eq!(rows, vec![
            (vec![1, 7, 9, 0], vec![2]),
            (vec![1, 7, 9, 1], vec![1]),
        ]);
    }

    #[test]
    fn compare_and_swap_has_one_winner_under_concurrency() {
        let env = open_env();
        let store = Arc::new(open_store(&env, "cas-race"));
        let barrier = Arc::new(std::sync::Barrier::new(16));
        let handles = (0u8..16)
            .map(|value| {
                let store = store.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    strict_atomic_mutate(&[AtomicStoreMutation {
                        store: store.as_ref(),
                        key: b"winner".to_vec(),
                        operation: AtomicStoreOperation::CompareAndSwap {
                            expected: None,
                            replacement: Some(vec![value]),
                        },
                    }])
                })
            })
            .collect::<Vec<_>>();

        let winners = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .filter(Result::is_ok)
            .count();

        assert_eq!(winners, 1);
        assert!(store.get_one(&b"winner".to_vec()).unwrap().is_some());
    }

    #[test]
    fn single_store_batches_trivially() {
        let env = open_env();
        let a = open_store(&env, "a");

        batched_put(vec![(&a, kv(&[("k1", "v1"), ("k2", "v2")]))]).unwrap();

        assert_eq!(a.get_one(&b"k1".to_vec()).unwrap(), Some(b"v1".to_vec()));
        assert_eq!(a.get_one(&b"k2".to_vec()).unwrap(), Some(b"v2".to_vec()));
    }
}

#[cfg(test)]
mod store_tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::batched_put_tests::{kv, open_env, open_store, TestEnv};
    use super::*;

    fn new_store() -> (TestEnv, LmdbKeyValueStore) {
        let env = open_env();
        let store = open_store(&env, "s");
        (env, store)
    }

    fn seeded_store() -> (TestEnv, LmdbKeyValueStore) {
        let (env, store) = new_store();
        store
            .put(kv(&[("k1", "v1"), ("k2", "v2"), ("k3", "v3")]))
            .unwrap();
        (env, store)
    }

    #[test]
    fn get_returns_values_in_key_order_with_none_for_missing() {
        let (_env, store) = seeded_store();
        let results = store
            .get(&vec![b"k2".to_vec(), b"missing".to_vec(), b"k1".to_vec()])
            .unwrap();
        assert_eq!(results, vec![
            Some(b"v2".to_vec()),
            None,
            Some(b"v1".to_vec())
        ]);
    }

    #[test]
    fn put_overwrites_existing_keys() {
        let (_env, store) = seeded_store();
        store.put(kv(&[("k1", "updated")])).unwrap();
        assert_eq!(
            store.get_one(&b"k1".to_vec()).unwrap(),
            Some(b"updated".to_vec())
        );
    }

    #[test]
    fn put_one_if_absent_inserts_once_and_keeps_first_value() {
        let (_env, store) = new_store();
        assert!(store
            .put_one_if_absent(b"k".to_vec(), b"first".to_vec())
            .unwrap());
        assert!(!store
            .put_one_if_absent(b"k".to_vec(), b"second".to_vec())
            .unwrap());
        assert_eq!(
            store.get_one(&b"k".to_vec()).unwrap(),
            Some(b"first".to_vec())
        );
    }

    #[test]
    fn delete_returns_count_of_keys_actually_removed() {
        let (_env, store) = seeded_store();
        let deleted = store
            .delete(vec![b"k1".to_vec(), b"missing".to_vec(), b"k3".to_vec()])
            .unwrap();
        assert_eq!(deleted, 2);
        assert_eq!(store.get_one(&b"k1".to_vec()).unwrap(), None);
        assert_eq!(
            store.get_one(&b"k2".to_vec()).unwrap(),
            Some(b"v2".to_vec())
        );
    }

    #[test]
    fn iterate_visits_every_entry() {
        static VISITED: AtomicUsize = AtomicUsize::new(0);
        fn visit(_key: ByteBuffer, _value: ByteBuffer) { VISITED.fetch_add(1, Ordering::SeqCst); }

        let (_env, store) = seeded_store();
        store.iterate(visit).unwrap();
        assert_eq!(VISITED.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn iterate_while_stops_when_callback_returns_false() {
        let (_env, store) = seeded_store();
        let mut seen = Vec::new();
        store
            .iterate_while(&mut |key, _value| {
                seen.push(key);
                Ok(seen.len() < 2)
            })
            .unwrap();
        assert_eq!(seen.len(), 2);
    }

    #[test]
    fn iterate_while_propagates_callback_errors() {
        let (_env, store) = seeded_store();
        let result = store.iterate_while(&mut |_key, _value| {
            Err(KvStoreError::InvalidArgument("boom".to_string()))
        });
        assert_eq!(
            result,
            Err(KvStoreError::InvalidArgument("boom".to_string()))
        );
    }

    #[test]
    fn to_map_and_print_store_reflect_all_entries() {
        let (_env, store) = seeded_store();
        let map = store.to_map().unwrap();
        assert_eq!(map.len(), 3);
        assert_eq!(map.get(b"k2".as_slice()), Some(&b"v2".to_vec()));
        store.print_store().unwrap();
    }

    #[test]
    fn non_empty_flips_when_first_entry_lands() {
        let (_env, store) = new_store();
        assert!(!store.non_empty().unwrap());
        store.put_one(b"k".to_vec(), b"v".to_vec()).unwrap();
        assert!(store.non_empty().unwrap());
    }

    #[test]
    fn boxed_clone_shares_the_same_database() {
        let (_env, store) = seeded_store();
        let boxed: Box<dyn KeyValueStore> = store.clone_box();
        let cloned = boxed.clone();
        assert_eq!(
            cloned.get_one(&b"k1".to_vec()).unwrap(),
            Some(b"v1".to_vec())
        );
        cloned.put_one(b"k4".to_vec(), b"v4".to_vec()).unwrap();
        assert_eq!(
            store.get_one(&b"k4".to_vec()).unwrap(),
            Some(b"v4".to_vec())
        );
    }

    #[test]
    fn trait_contains_and_put_if_absent_defaults_work_through_lmdb() {
        let (_env, store) = seeded_store();
        assert_eq!(
            store
                .contains(&vec![b"k1".to_vec(), b"missing".to_vec()])
                .unwrap(),
            vec![true, false]
        );

        store
            .put_if_absent(kv(&[("k1", "clobbered"), ("k9", "fresh")]))
            .unwrap();
        assert_eq!(
            store.get_one(&b"k1".to_vec()).unwrap(),
            Some(b"v1".to_vec())
        );
        assert_eq!(
            store.get_one(&b"k9".to_vec()).unwrap(),
            Some(b"fresh".to_vec())
        );
    }
}
