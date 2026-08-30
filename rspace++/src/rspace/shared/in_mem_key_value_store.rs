// See shared/src/main/scala/coop/rchain/store/InMemoryKeyValueStore.scala

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use dashmap::DashMap;
use dashmap::mapref::entry::Entry;
use shared::rust::store::key_value_store::{
    AtomicStoreMutation, AtomicStoreOperation, KeyValueStore, KvStoreError,
};
use shared::rust::{ByteBuffer, ByteVector};

#[derive(Clone)]
pub struct InMemoryKeyValueStore {
    state: Arc<DashMap<ByteBuffer, ByteVector>>,
    coordinator: Arc<RwLock<()>>,
}

impl KeyValueStore for InMemoryKeyValueStore {
    fn as_any(&self) -> &dyn std::any::Any { self }

    fn get(&self, keys: &Vec<ByteBuffer>) -> Result<Vec<Option<ByteBuffer>>, KvStoreError> {
        let _guard = self.read_guard();
        let result = keys
            .iter()
            .map(|key| self.state.get(key).map(|entry| entry.value().clone()))
            .collect::<Vec<Option<ByteBuffer>>>();

        Ok(result)
    }

    fn put(&self, kv_pairs: Vec<(ByteBuffer, ByteBuffer)>) -> Result<(), KvStoreError> {
        let _guard = self.write_guard();
        for (key, value) in kv_pairs {
            self.state.insert(key, value);
        }

        Ok(())
    }

    fn put_one_if_absent(&self, key: ByteBuffer, value: ByteBuffer) -> Result<bool, KvStoreError> {
        let _guard = self.write_guard();
        match self.state.entry(key) {
            Entry::Occupied(_) => Ok(false),
            Entry::Vacant(entry) => {
                entry.insert(value);
                Ok(true)
            }
        }
    }

    fn delete(&self, keys: Vec<ByteBuffer>) -> Result<usize, KvStoreError> {
        let _guard = self.write_guard();
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
        let _guard = self.read_guard();
        for entry in self.state.iter() {
            if !f(entry.key().to_vec(), entry.value().to_vec())? {
                break;
            }
        }
        Ok(())
    }

    fn clone_box(&self) -> Box<dyn KeyValueStore> { Box::new(self.clone()) }

    fn to_map(&self) -> Result<BTreeMap<ByteBuffer, ByteBuffer>, KvStoreError> {
        let _guard = self.read_guard();
        let mut map = BTreeMap::new();

        for entry in self.state.iter() {
            map.insert(entry.key().to_vec(), entry.value().to_vec());
        }

        Ok(map)
    }

    fn strict_atomic_mutate(
        &self,
        mutations: &[AtomicStoreMutation<'_>],
    ) -> Result<(), KvStoreError> {
        let stores = mutations
            .iter()
            .map(|mutation| {
                mutation
                    .store
                    .as_any()
                    .downcast_ref::<InMemoryKeyValueStore>()
                    .ok_or_else(|| {
                        KvStoreError::AtomicityUnavailable(
                            "strict in-memory transaction includes another backend".to_string(),
                        )
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if stores
            .iter()
            .any(|store| !Arc::ptr_eq(&self.coordinator, &store.coordinator))
        {
            return Err(KvStoreError::AtomicityUnavailable(
                "strict in-memory transaction spans multiple managers".to_string(),
            ));
        }
        let _guard = self.write_guard();
        let mut snapshots: Vec<(
            Arc<DashMap<ByteBuffer, ByteVector>>,
            BTreeMap<ByteBuffer, ByteVector>,
        )> = Vec::new();
        for store in &stores {
            if snapshots
                .iter()
                .any(|(state, _)| Arc::ptr_eq(state, &store.state))
            {
                continue;
            }
            let snapshot = store
                .state
                .iter()
                .map(|entry| (entry.key().clone(), entry.value().clone()))
                .collect();
            snapshots.push((store.state.clone(), snapshot));
        }
        for (mutation, store) in mutations.iter().zip(stores) {
            let (_, snapshot) = snapshots
                .iter_mut()
                .find(|(state, _)| Arc::ptr_eq(state, &store.state))
                .expect("transaction snapshot exists");
            let current = snapshot.get(&mutation.key).cloned();
            match &mutation.operation {
                AtomicStoreOperation::Put(value) => {
                    snapshot.insert(mutation.key.clone(), value.clone());
                }
                AtomicStoreOperation::PutIfAbsentOrEqual(value) => match current {
                    Some(existing) if existing != *value => {
                        return Err(KvStoreError::TransactionConflict(format!(
                            "existing value differs for key {}",
                            hex::encode(&mutation.key)
                        )));
                    }
                    Some(_) => {}
                    None => {
                        snapshot.insert(mutation.key.clone(), value.clone());
                    }
                },
                AtomicStoreOperation::Delete => {
                    snapshot.remove(&mutation.key);
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
                        Some(value) => {
                            snapshot.insert(mutation.key.clone(), value.clone());
                        }
                        None => {
                            snapshot.remove(&mutation.key);
                        }
                    }
                }
            }
        }
        for (state, snapshot) in snapshots {
            state.clear();
            for (key, value) in snapshot {
                state.insert(key, value);
            }
        }
        Ok(())
    }

    fn size_bytes(&self) -> usize {
        let _guard = self.read_guard();
        self.state
            .iter()
            .map(|entry| entry.key().len() + entry.value().len())
            .sum()
    }

    fn print_store(&self) -> Result<(), KvStoreError> {
        println!("\nIn Mem Key Value Store: {:?}", self.to_map()?);
        Ok(())
    }

    fn non_empty(&self) -> Result<bool, KvStoreError> {
        let _guard = self.read_guard();
        Ok(!self.state.is_empty())
    }
}

impl InMemoryKeyValueStore {
    fn read_guard(&self) -> std::sync::RwLockReadGuard<'_, ()> {
        self.coordinator
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn write_guard(&self) -> std::sync::RwLockWriteGuard<'_, ()> {
        self.coordinator
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn new() -> Self {
        InMemoryKeyValueStore {
            state: Arc::new(DashMap::new()),
            coordinator: Arc::new(RwLock::new(())),
        }
    }

    pub fn new_with_coordinator(coordinator: Arc<RwLock<()>>) -> Self {
        InMemoryKeyValueStore {
            state: Arc::new(DashMap::new()),
            coordinator,
        }
    }

    pub fn clear(&self) {
        let _guard = self.write_guard();
        self.state.clear();
    }

    pub fn num_records(&self) -> usize {
        let _guard = self.read_guard();
        self.state.len()
    }
}

#[cfg(test)]
mod tests {
    use shared::rust::store::key_value_store::{
        AtomicStoreMutation, AtomicStoreOperation, KeyValueStore, KvStoreError,
        strict_atomic_mutate,
    };

    use super::*;

    #[test]
    fn manager_coordinator_commits_and_rolls_back_across_stores() {
        let coordinator = Arc::new(RwLock::new(()));
        let a = InMemoryKeyValueStore::new_with_coordinator(coordinator.clone());
        let b = InMemoryKeyValueStore::new_with_coordinator(coordinator);
        a.put_one(b"guard".to_vec(), b"current".to_vec()).unwrap();
        let commit = [
            AtomicStoreMutation {
                store: &a,
                key: b"guard".to_vec(),
                operation: AtomicStoreOperation::CompareAndSwap {
                    expected: Some(b"current".to_vec()),
                    replacement: Some(b"next".to_vec()),
                },
            },
            AtomicStoreMutation {
                store: &b,
                key: b"row".to_vec(),
                operation: AtomicStoreOperation::PutIfAbsentOrEqual(b"value".to_vec()),
            },
        ];
        strict_atomic_mutate(&commit).unwrap();
        let rollback = [
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
                    replacement: None,
                },
            },
        ];

        assert!(matches!(
            strict_atomic_mutate(&rollback),
            Err(KvStoreError::TransactionConflict(_))
        ));
        assert_eq!(a.get_one(&b"guard".to_vec()).unwrap(), Some(b"next".to_vec()));
        assert_eq!(b.get_one(&b"row".to_vec()).unwrap(), Some(b"value".to_vec()));
        assert_eq!(b.get_one(&b"uncommitted".to_vec()).unwrap(), None);
    }

    #[test]
    fn distinct_manager_coordinators_fail_closed() {
        let a = InMemoryKeyValueStore::new();
        let b = InMemoryKeyValueStore::new();
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
}
