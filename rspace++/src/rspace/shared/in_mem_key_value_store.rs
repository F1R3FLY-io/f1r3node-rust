// See shared/src/main/scala/coop/rchain/store/InMemoryKeyValueStore.scala

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use dashmap::DashMap;
use shared::rust::store::key_value_store::{
    KeyValueStore, KeyValueStoreMutation, KvStoreError, validate_atomic_batch,
};
use shared::rust::{ByteBuffer, ByteVector};

#[derive(Clone)]
pub struct InMemoryKeyValueStore {
    state: Arc<DashMap<ByteBuffer, ByteVector>>,
    transaction_lock: Arc<Mutex<()>>,
}

impl KeyValueStore for InMemoryKeyValueStore {
    fn get(&self, keys: &Vec<ByteBuffer>) -> Result<Vec<Option<ByteBuffer>>, KvStoreError> {
        let _transaction = self.transaction_lock.lock().map_err(|_| {
            KvStoreError::LockError("in-memory key-value transaction lock poisoned".to_string())
        })?;
        let result = keys
            .iter()
            .map(|key| self.state.get(key).map(|entry| entry.value().clone()))
            .collect::<Vec<Option<ByteBuffer>>>();

        Ok(result)
    }

    fn put(&self, kv_pairs: Vec<(ByteBuffer, ByteBuffer)>) -> Result<(), KvStoreError> {
        let _transaction = self.transaction_lock.lock().map_err(|_| {
            KvStoreError::LockError("in-memory key-value transaction lock poisoned".to_string())
        })?;
        for (key, value) in kv_pairs {
            self.state.insert(key, value);
        }

        Ok(())
    }

    fn compare_and_swap_batch(
        &self,
        conditions: &[(ByteBuffer, Option<ByteBuffer>)],
        mutations: &[KeyValueStoreMutation],
    ) -> Result<bool, KvStoreError> {
        validate_atomic_batch(conditions, mutations)?;
        let _transaction = self.transaction_lock.lock().map_err(|_| {
            KvStoreError::LockError("in-memory key-value transaction lock poisoned".to_string())
        })?;

        for (key, expected) in conditions {
            let actual = self.state.get(key).map(|entry| entry.value().clone());
            if actual.as_ref() != expected.as_ref() {
                return Ok(false);
            }
        }

        for mutation in mutations {
            match mutation {
                KeyValueStoreMutation::Put(key, value) => {
                    self.state.insert(key.clone(), value.clone());
                }
                KeyValueStoreMutation::Delete(key) => {
                    let _ = self.state.remove(key);
                }
            }
        }
        Ok(true)
    }

    fn delete(&self, keys: Vec<ByteBuffer>) -> Result<usize, KvStoreError> {
        let _transaction = self.transaction_lock.lock().map_err(|_| {
            KvStoreError::LockError("in-memory key-value transaction lock poisoned".to_string())
        })?;
        Ok(keys
            .into_iter()
            .filter_map(|key| self.state.remove(&key).map(|(_, v)| v))
            .count())
    }

    fn iterate(&self, f: fn(ByteBuffer, ByteBuffer)) -> Result<(), KvStoreError> {
        let snapshot = self.to_map()?;
        for (key, value) in snapshot {
            f(key, value);
        }
        Ok(())
    }

    fn iterate_while(
        &self,
        f: &mut dyn FnMut(ByteBuffer, ByteBuffer) -> Result<bool, KvStoreError>,
    ) -> Result<(), KvStoreError> {
        let snapshot = self.to_map()?;
        for (key, value) in snapshot {
            if !f(key, value)? {
                break;
            }
        }
        Ok(())
    }

    fn clone_box(&self) -> Box<dyn KeyValueStore> { Box::new(self.clone()) }

    fn to_map(&self) -> Result<BTreeMap<ByteBuffer, ByteBuffer>, KvStoreError> {
        let _transaction = self.transaction_lock.lock().map_err(|_| {
            KvStoreError::LockError("in-memory key-value transaction lock poisoned".to_string())
        })?;
        let mut map = BTreeMap::new();

        for entry in self.state.iter() {
            map.insert(entry.key().to_vec(), entry.value().to_vec());
        }

        Ok(map)
    }

    fn size_bytes(&self) -> Result<usize, KvStoreError> {
        let _transaction = self.transaction_lock.lock().map_err(|_| {
            KvStoreError::LockError("in-memory key-value transaction lock poisoned".to_string())
        })?;
        Ok(self
            .state
            .iter()
            .map(|entry| entry.key().len() + entry.value().len())
            .sum())
    }

    fn print_store(&self) -> Result<(), KvStoreError> {
        println!("\nIn Mem Key Value Store: {:?}", self.to_map()?);
        Ok(())
    }

    fn non_empty(&self) -> Result<bool, KvStoreError> {
        let _transaction = self.transaction_lock.lock().map_err(|_| {
            KvStoreError::LockError("in-memory key-value transaction lock poisoned".to_string())
        })?;
        Ok(!self.state.is_empty())
    }
}

impl InMemoryKeyValueStore {
    pub fn new() -> Self {
        InMemoryKeyValueStore {
            state: Arc::new(DashMap::new()),
            transaction_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn clear(&self) -> Result<(), KvStoreError> {
        let _transaction = self.transaction_lock.lock().map_err(|_| {
            KvStoreError::LockError("in-memory key-value transaction lock poisoned".to_string())
        })?;
        self.state.clear();
        Ok(())
    }

    pub fn num_records(&self) -> Result<usize, KvStoreError> {
        let _transaction = self.transaction_lock.lock().map_err(|_| {
            KvStoreError::LockError("in-memory key-value transaction lock poisoned".to_string())
        })?;
        Ok(self.state.len())
    }
}
