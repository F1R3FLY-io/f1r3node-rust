use std::collections::BTreeMap;
use std::sync::Arc;

use heed::types::SerdeBincode;
use heed::{Database, Env, Error as HeedError, MdbError, PutFlags};

use super::key_value_store::{KeyValueStore, KvStoreError};
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

    use heed::EnvOpenOptions;

    use super::*;

    pub(super) fn open_env() -> Arc<Env> {
        let dir = tempfile::tempdir().unwrap();
        // Leak the tempdir path so the Env (and its mmap) stay valid for the
        // lifetime of the test; each test opens its own directory.
        let path = Box::leak(Box::new(dir)).path();
        let mut builder = EnvOpenOptions::new();
        builder.map_size(10 * 1024 * 1024);
        builder.max_dbs(4);
        let env = unsafe { builder.open(path).unwrap() };
        Arc::new(env)
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

    use super::batched_put_tests::{kv, open_env, open_store};
    use super::*;

    fn seeded_store() -> LmdbKeyValueStore {
        let store = open_store(&open_env(), "s");
        store
            .put(kv(&[("k1", "v1"), ("k2", "v2"), ("k3", "v3")]))
            .unwrap();
        store
    }

    #[test]
    fn get_returns_values_in_key_order_with_none_for_missing() {
        let store = seeded_store();
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
        let store = seeded_store();
        store.put(kv(&[("k1", "updated")])).unwrap();
        assert_eq!(
            store.get_one(&b"k1".to_vec()).unwrap(),
            Some(b"updated".to_vec())
        );
    }

    #[test]
    fn put_one_if_absent_inserts_once_and_keeps_first_value() {
        let store = open_store(&open_env(), "s");
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
        let store = seeded_store();
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

        let store = seeded_store();
        store.iterate(visit).unwrap();
        assert_eq!(VISITED.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn iterate_while_stops_when_callback_returns_false() {
        let store = seeded_store();
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
        let store = seeded_store();
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
        let store = seeded_store();
        let map = store.to_map().unwrap();
        assert_eq!(map.len(), 3);
        assert_eq!(map.get(b"k2".as_slice()), Some(&b"v2".to_vec()));
        store.print_store().unwrap();
    }

    #[test]
    fn non_empty_flips_when_first_entry_lands() {
        let store = open_store(&open_env(), "s");
        assert!(!store.non_empty().unwrap());
        store.put_one(b"k".to_vec(), b"v".to_vec()).unwrap();
        assert!(store.non_empty().unwrap());
    }

    #[test]
    fn boxed_clone_shares_the_same_database() {
        let store = seeded_store();
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
        let store = seeded_store();
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
