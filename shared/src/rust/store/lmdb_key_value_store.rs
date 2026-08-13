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

impl KeyValueStore for LmdbKeyValueStore {
    fn as_any(&self) -> &dyn std::any::Any { self }

    fn get(&self, keys: &Vec<ByteBuffer>) -> Result<Vec<Option<ByteBuffer>>, KvStoreError> {
        let reader = self.env.read_txn()?;
        let results = keys
            .iter()
            .map(|key| self.db.get(&reader, key).map_err(|e| e.into()))
            .collect();
        drop(reader);
        results
    }

    fn put(&self, kv_pairs: Vec<(ByteBuffer, ByteBuffer)>) -> Result<(), KvStoreError> {
        let mut writer = self.env.write_txn()?;
        for (key, value) in kv_pairs {
            self.db.put(&mut writer, &key, &value)?;
        }
        writer.commit()?;

        Ok(())
    }

    fn put_one_if_absent(&self, key: ByteBuffer, value: ByteBuffer) -> Result<bool, KvStoreError> {
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
    }

    fn delete(&self, keys: Vec<ByteBuffer>) -> Result<usize, KvStoreError> {
        let mut writer = self.env.write_txn()?;
        let mut delete_count = 0;
        for key in &keys {
            if self.db.delete(&mut writer, key)? {
                delete_count += 1;
            }
        }
        writer.commit()?;
        Ok(delete_count)
    }

    fn iterate(&self, f: fn(ByteBuffer, ByteBuffer)) -> Result<(), KvStoreError> {
        let reader = self.env.read_txn()?;
        let iter = self.db.iter(&reader)?;
        for result in iter {
            let (key, value) = result?;
            f(key.to_vec(), value);
        }
        drop(reader);
        Ok(())
    }

    fn iterate_while(
        &self,
        f: &mut dyn FnMut(ByteBuffer, ByteBuffer) -> Result<bool, KvStoreError>,
    ) -> Result<(), KvStoreError> {
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
    }

    fn clone_box(&self) -> Box<dyn KeyValueStore> { Box::new(self.clone()) }

    fn to_map(&self) -> Result<BTreeMap<ByteBuffer, ByteBuffer>, KvStoreError> {
        let reader = self.env.read_txn()?;
        let iter = self.db.iter(&reader)?;
        let mut map = BTreeMap::new();
        for result in iter {
            let (key, value) = result?;
            map.insert(key.to_vec(), value);
        }
        drop(reader);
        Ok(map)
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

/// Writes to multiple stores in a single LMDB write transaction when they
/// are all `LmdbKeyValueStore`s backed by the same `Env` — LMDB serialises
/// all writers on one environment-wide lock (see the module comment above),
/// so combining N separate `put()` calls (N lock acquisitions) into one
/// transaction (one lock acquisition) directly reduces contention on that
/// lock under concurrent block processing. Falls back to independent
/// `put()` calls — preserving prior behavior exactly — when any store isn't
/// an `LmdbKeyValueStore` (e.g. an in-memory test double) or the stores
/// don't all share one `Env`.
/// One store's worth of key/value pairs to write, paired with the store to write them to.
pub type StoreWrite<'a> = (&'a dyn KeyValueStore, Vec<(ByteBuffer, ByteBuffer)>);

type LmdbStoreWrite<'a> = (&'a LmdbKeyValueStore, &'a Vec<(ByteBuffer, ByteBuffer)>);

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

    let batchable = match &lmdb_stores {
        Some(stores) => stores
            .windows(2)
            .all(|w| Arc::ptr_eq(&w[0].0.env, &w[1].0.env)),
        None => false,
    };

    if batchable {
        let stores = lmdb_stores.expect("batchable implies Some, checked above");
        if let Some((first, _)) = stores.first() {
            let mut wtxn = first.env.write_txn()?;
            for (store, kv_pairs) in &stores {
                for (key, value) in kv_pairs.iter() {
                    store.db.put(&mut wtxn, key, value)?;
                }
            }
            wtxn.commit()?;
        }
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
