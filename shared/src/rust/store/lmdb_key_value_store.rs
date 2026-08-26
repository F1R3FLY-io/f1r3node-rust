use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use heed::types::SerdeBincode;
use heed::{Database, Env};

use super::key_value_store::{
    validate_atomic_batch, KeyValueStore, KeyValueStoreMutation, KvStoreError,
};
use crate::rust::ByteBuffer;

pub struct LmdbKeyValueStore {
    pub env: Arc<Env>,
    pub db: Arc<Mutex<Database<SerdeBincode<ByteBuffer>, SerdeBincode<ByteBuffer>>>>,
}

impl KeyValueStore for LmdbKeyValueStore {
    fn get(&self, keys: &Vec<ByteBuffer>) -> Result<Vec<Option<ByteBuffer>>, KvStoreError> {
        let db = self.db.lock().map_err(|_| {
            KvStoreError::LockError(
                "LMDB Key Value Store: Failed to acquire lock on db".to_string(),
            )
        })?;

        let reader = self.env.read_txn()?;
        let results = keys
            .iter()
            .map(|key| db.get(&reader, key).map_err(|e| e.into()))
            .collect();
        drop(reader);
        results
    }

    fn put(&self, kv_pairs: Vec<(ByteBuffer, ByteBuffer)>) -> Result<(), KvStoreError> {
        let db = self.db.lock().map_err(|_| {
            KvStoreError::LockError(
                "LMDB Key Value Store: Failed to acquire lock on db".to_string(),
            )
        })?;

        let mut writer = self.env.write_txn()?;
        for (key, value) in kv_pairs {
            db.put(&mut writer, &key, &value)?;
        }
        writer.commit()?;

        Ok(())
    }

    fn compare_and_swap_batch(
        &self,
        conditions: &[(ByteBuffer, Option<ByteBuffer>)],
        mutations: &[KeyValueStoreMutation],
    ) -> Result<bool, KvStoreError> {
        validate_atomic_batch(conditions, mutations)?;
        let db = self.db.lock().map_err(|_| {
            KvStoreError::LockError(
                "LMDB Key Value Store: Failed to acquire lock on db".to_string(),
            )
        })?;

        let mut writer = self.env.write_txn()?;
        for (key, expected) in conditions {
            let actual = db.get(&writer, key)?;
            if actual.as_ref() != expected.as_ref() {
                return Ok(false);
            }
        }

        for mutation in mutations {
            match mutation {
                KeyValueStoreMutation::Put(key, value) => db.put(&mut writer, key, value)?,
                KeyValueStoreMutation::Delete(key) => {
                    let _ = db.delete(&mut writer, key)?;
                }
            }
        }
        writer.commit()?;
        Ok(true)
    }

    fn delete(&self, keys: Vec<ByteBuffer>) -> Result<usize, KvStoreError> {
        let db = self.db.lock().map_err(|_| {
            KvStoreError::LockError(
                "LMDB Key Value Store: Failed to acquire lock on db".to_string(),
            )
        })?;

        let mut writer = self.env.write_txn()?;
        let mut delete_count = 0;
        for key in &keys {
            if db.delete(&mut writer, key)? {
                delete_count += 1;
            }
        }
        writer.commit()?;
        Ok(delete_count)
    }

    fn iterate(&self, f: fn(ByteBuffer, ByteBuffer)) -> Result<(), KvStoreError> {
        let db = self.db.lock().map_err(|_| {
            KvStoreError::LockError(
                "LMDB Key Value Store: Failed to acquire lock on db".to_string(),
            )
        })?;

        let reader = self.env.read_txn()?;
        let iter = db.iter(&reader)?;
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
        let db = self.db.lock().map_err(|_| {
            KvStoreError::LockError(
                "LMDB Key Value Store: Failed to acquire lock on db".to_string(),
            )
        })?;

        let reader = self.env.read_txn()?;
        let iter = db.iter(&reader)?;
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
        let db = self.db.lock().map_err(|_| {
            KvStoreError::LockError(
                "LMDB Key Value Store: Failed to acquire lock on db".to_string(),
            )
        })?;

        let reader = self.env.read_txn()?;
        let iter = db.iter(&reader)?;
        let mut map = BTreeMap::new();
        for result in iter {
            let (key, value) = result?;
            map.insert(key.to_vec(), value);
        }
        drop(reader);
        Ok(map)
    }

    fn size_bytes(&self) -> Result<usize, KvStoreError> {
        let db = self.db.lock().map_err(|_| {
            KvStoreError::LockError(
                "LMDB Key Value Store: Failed to acquire lock on db".to_string(),
            )
        })?;
        let reader = self.env.read_txn()?;
        let mut total = 0usize;
        for result in db.iter(&reader)? {
            let (key, value) = result?;
            total = total
                .checked_add(key.len())
                .and_then(|size| size.checked_add(value.len()))
                .ok_or_else(|| {
                    KvStoreError::InvalidArgument(
                        "key-value store byte size exceeds usize".to_string(),
                    )
                })?;
        }
        Ok(total)
    }

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
        let db = self.db.lock().map_err(|_| {
            KvStoreError::LockError(
                "LMDB Key Value Store: Failed to acquire lock on db".to_string(),
            )
        })?;

        let reader = self.env.read_txn()?;
        let has_first = {
            let mut iter = db.iter(&reader)?;
            iter.next().is_some()
        };
        drop(reader);
        Ok(has_first)
    }
}

impl LmdbKeyValueStore {
    pub fn new(
        env: Arc<Env>,
        db: Database<SerdeBincode<ByteBuffer>, SerdeBincode<ByteBuffer>>,
    ) -> Self {
        LmdbKeyValueStore {
            env,
            db: Arc::new(Mutex::new(db)),
        }
    }
}

impl Clone for LmdbKeyValueStore {
    fn clone(&self) -> Self {
        Self {
            db: self.db.clone(),
            env: self.env.clone(),
        }
    }
}
