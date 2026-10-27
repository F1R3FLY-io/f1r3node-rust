// See shared/src/main/scala/coop/rchain/store/InMemoryKeyValueStore.scala

use std::collections::BTreeMap;
use std::sync::Arc;

use dashmap::DashMap;
use dashmap::mapref::entry::Entry;
use shared::rust::store::key_value_store::{KeyValueStore, KvStoreError};
use shared::rust::{ByteBuffer, ByteVector};

#[derive(Clone)]
pub struct InMemoryKeyValueStore {
    state: Arc<DashMap<ByteBuffer, ByteVector>>,
}

impl KeyValueStore for InMemoryKeyValueStore {
    fn as_any(&self) -> &dyn std::any::Any { self }

    fn get(&self, keys: &Vec<ByteBuffer>) -> Result<Vec<Option<ByteBuffer>>, KvStoreError> {
        let result = keys
            .iter()
            .map(|key| self.state.get(key).map(|entry| entry.value().clone()))
            .collect::<Vec<Option<ByteBuffer>>>();

        Ok(result)
    }

    fn put(&self, kv_pairs: Vec<(ByteBuffer, ByteBuffer)>) -> Result<(), KvStoreError> {
        for (key, value) in kv_pairs {
            self.state.insert(key, value);
        }

        Ok(())
    }

    fn put_one_if_absent(&self, key: ByteBuffer, value: ByteBuffer) -> Result<bool, KvStoreError> {
        match self.state.entry(key) {
            Entry::Occupied(_) => Ok(false),
            Entry::Vacant(entry) => {
                entry.insert(value);
                Ok(true)
            }
        }
    }

    fn delete_if_value(
        &self,
        key: &ByteBuffer,
        expected: &ByteBuffer,
    ) -> Result<bool, KvStoreError> {
        Ok(self
            .state
            .remove_if(key, |_, value| value == expected)
            .is_some())
    }

    fn delete(&self, keys: Vec<ByteBuffer>) -> Result<usize, KvStoreError> {
        Ok(keys
            .into_iter()
            .filter_map(|key| self.state.remove(&key).map(|(_, v)| v))
            .count())
    }

    fn iterate(&self, _f: fn(ByteBuffer, ByteBuffer)) -> Result<(), KvStoreError> { todo!() }

    fn iterate_while(
        &self,
        f: &mut dyn FnMut(ByteBuffer, ByteBuffer) -> Result<bool, KvStoreError>,
    ) -> Result<(), KvStoreError> {
        for entry in self.state.iter() {
            if !f(entry.key().to_vec(), entry.value().to_vec())? {
                break;
            }
        }
        Ok(())
    }

    fn clone_box(&self) -> Box<dyn KeyValueStore> { Box::new(self.clone()) }

    fn to_map(&self) -> Result<BTreeMap<ByteBuffer, ByteBuffer>, KvStoreError> {
        let mut map = BTreeMap::new();

        for entry in self.state.iter() {
            map.insert(entry.key().to_vec(), entry.value().to_vec());
        }

        Ok(map)
    }

    fn size_bytes(&self) -> usize {
        self.state
            .iter()
            .map(|entry| entry.key().len() + entry.value().len())
            .sum()
    }

    fn print_store(&self) -> Result<(), KvStoreError> {
        println!("\nIn Mem Key Value Store: {:?}", self.to_map()?);
        Ok(())
    }

    fn non_empty(&self) -> Result<bool, KvStoreError> { Ok(!self.state.is_empty()) }
}

impl InMemoryKeyValueStore {
    pub fn new() -> Self {
        InMemoryKeyValueStore {
            state: Arc::new(DashMap::new()),
        }
    }

    pub fn clear(&self) { self.state.clear(); }

    pub fn num_records(&self) -> usize { self.state.len() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conditional_delete_preserves_different_and_absent_values() {
        let store = InMemoryKeyValueStore::new();
        let key = b"key".to_vec();
        let value = b"value".to_vec();
        store.put_one(key.clone(), value.clone()).unwrap();
        assert!(!store.delete_if_value(&key, &b"other".to_vec()).unwrap());
        assert_eq!(store.get_one(&key).unwrap(), Some(value.clone()));
        assert!(store.delete_if_value(&key, &value).unwrap());
        assert!(!store.delete_if_value(&key, &value).unwrap());
    }

    #[test]
    fn concurrent_conditional_deletes_remove_one_memory_record() {
        let store = InMemoryKeyValueStore::new();
        store.put_one(b"key".to_vec(), b"value".to_vec()).unwrap();
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let store = store.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    store
                        .delete_if_value(&b"key".to_vec(), &b"value".to_vec())
                        .unwrap()
                })
            })
            .collect();
        let removed = handles
            .into_iter()
            .map(|handle| usize::from(handle.join().unwrap()))
            .sum::<usize>();
        assert_eq!(removed, 1);
        assert_eq!(store.get_one(&b"key".to_vec()).unwrap(), None);
    }
}
