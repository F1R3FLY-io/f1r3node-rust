// See shared/src/main/scala/coop/rchain/store/InMemoryKeyValueStore.scala

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use dashmap::DashMap;
use dashmap::mapref::entry::Entry;
use shared::rust::store::key_value_store::{
    AtomicStoreMutation, AtomicStoreOperation, KeyValueStore, KvStoreError,
};
use shared::rust::{ByteBuffer, ByteVector};

use super::sparse_transaction::{self, Mutation, Operation};

#[derive(Clone)]
pub struct InMemoryKeyValueStore {
    state: Arc<DashMap<ByteBuffer, ByteVector>>,
    coordinator: Arc<RwLock<()>>,
}

impl KeyValueStore for InMemoryKeyValueStore {
    fn as_any(&self) -> &dyn std::any::Any { self }

    fn visit_entries(
        &self,
        reader: &mut shared::rust::store::key_value_store::EntryReader<'_>,
    ) -> Result<(), KvStoreError> {
        sparse_transaction::with_snapshot(self, || {
            for entry in self.state.iter() {
                reader(entry.key(), entry.value())?;
            }
            Ok(())
        })
    }

    fn with_value(
        &self,
        key: &ByteBuffer,
        reader: &mut shared::rust::store::key_value_store::ValueReader<'_>,
    ) -> Result<(), KvStoreError> {
        sparse_transaction::with_snapshot(self, || {
            let value = self.state.get(key);
            reader(value.as_ref().map(|entry| entry.value().as_slice()))
        })
    }

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
        let borrowed = mutations
            .iter()
            .map(|mutation| {
                let store = mutation
                    .store
                    .as_any()
                    .downcast_ref::<InMemoryKeyValueStore>()
                    .ok_or_else(|| {
                        KvStoreError::AtomicityUnavailable(
                            "strict in-memory transaction includes another backend".to_string(),
                        )
                    })?;
                Ok(Mutation {
                    store,
                    key: &mutation.key,
                    operation: match &mutation.operation {
                        AtomicStoreOperation::Put(value) => Operation::Put(value),
                        AtomicStoreOperation::PutIfAbsentOrEqual(value) => {
                            Operation::PutIfAbsentOrEqual(value)
                        }
                        AtomicStoreOperation::Delete => Operation::Delete,
                        AtomicStoreOperation::CompareAndSwap {
                            expected,
                            replacement,
                        } => Operation::CompareAndSwap {
                            expected,
                            replacement,
                        },
                    },
                })
            })
            .collect::<Result<Vec<_>, KvStoreError>>()?;
        sparse_transaction::apply(self, &borrowed)
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

impl sparse_transaction::Store for InMemoryKeyValueStore {
    type Key = ByteBuffer;
    type Value = ByteVector;
    type Error = KvStoreError;
    type ReadGuard<'a> = std::sync::RwLockReadGuard<'a, ()>;
    type WriteGuard<'a> = std::sync::RwLockWriteGuard<'a, ()>;

    fn same_manager(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.coordinator, &other.coordinator)
    }

    fn same_store(&self, other: &Self) -> bool { Arc::ptr_eq(&self.state, &other.state) }
    fn read_guard(&self) -> Self::ReadGuard<'_> { self.read_guard() }
    fn write_guard(&self) -> Self::WriteGuard<'_> { self.write_guard() }

    fn current(&self, key: &ByteBuffer) -> Option<ByteVector> {
        self.state.get(key).map(|entry| entry.value().clone())
    }

    fn put(&self, key: ByteBuffer, value: ByteVector) { self.state.insert(key, value); }

    fn delete(&self, key: &ByteBuffer) { self.state.remove(key); }

    fn manager_error() -> KvStoreError {
        KvStoreError::AtomicityUnavailable(
            "strict in-memory transaction spans multiple managers".to_string(),
        )
    }

    fn conflict(key: &ByteBuffer, compare_and_swap: bool) -> KvStoreError {
        let message = if compare_and_swap {
            "compare-and-swap expectation failed for key"
        } else {
            "existing value differs for key"
        };
        KvStoreError::TransactionConflict(format!("{message} {}", hex::encode(key)))
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use shared::rust::store::key_value_store::{
        AtomicStoreMutation, AtomicStoreOperation, KeyValueStore, KvStoreError,
        strict_atomic_mutate,
    };

    use super::*;

    fn operation_strategy() -> impl Strategy<Value = AtomicStoreOperation> {
        prop_oneof![
            any::<u8>().prop_map(|value| AtomicStoreOperation::Put(vec![value])),
            any::<u8>().prop_map(|value| AtomicStoreOperation::PutIfAbsentOrEqual(vec![value])),
            Just(AtomicStoreOperation::Delete),
            (proptest::option::of(any::<u8>()), proptest::option::of(any::<u8>())).prop_map(
                |(expected, replacement)| AtomicStoreOperation::CompareAndSwap {
                    expected: expected.map(|value| vec![value]),
                    replacement: replacement.map(|value| vec![value]),
                }
            ),
        ]
    }

    fn dense_reference(
        initial: &[BTreeMap<ByteBuffer, ByteVector>],
        operations: &[(usize, u8, AtomicStoreOperation)],
    ) -> Option<Vec<BTreeMap<ByteBuffer, ByteVector>>> {
        let mut result = initial.to_vec();
        for (selector, key, operation) in operations {
            let store = &mut result[selector % initial.len()];
            let key = vec![*key];
            match operation {
                AtomicStoreOperation::Put(value) => {
                    store.insert(key, value.clone());
                }
                AtomicStoreOperation::PutIfAbsentOrEqual(value) => {
                    if store.get(&key).is_some_and(|current| current != value) {
                        return None;
                    }
                    store.insert(key, value.clone());
                }
                AtomicStoreOperation::Delete => {
                    store.remove(&key);
                }
                AtomicStoreOperation::CompareAndSwap {
                    expected,
                    replacement,
                } => {
                    if store.get(&key) != expected.as_ref() {
                        return None;
                    }
                    match replacement {
                        Some(value) => {
                            store.insert(key, value.clone());
                        }
                        None => {
                            store.remove(&key);
                        }
                    }
                }
            }
        }
        Some(result)
    }

    proptest! {
        #[test]
        fn sparse_transactions_match_dense_reference_with_repeated_keys_and_store_aliases(
            store_count in 1usize..9,
            initial in proptest::collection::vec((0usize..16, 0u8..6, any::<u8>()), 0..32),
            operations in proptest::collection::vec((0usize..16, 0u8..6, operation_strategy()), 0..65),
        ) {
            let coordinator = Arc::new(RwLock::new(()));
            let stores = (0..store_count).map(|_| InMemoryKeyValueStore::new_with_coordinator(coordinator.clone())).collect::<Vec<_>>();
            for (selector, key, value) in initial {
                stores[selector % store_count].put_one(vec![key], vec![value]).unwrap();
            }
            let before = stores.iter().map(|store| store.to_map().unwrap()).collect::<Vec<_>>();
            let handles = stores.iter().chain(&stores).cloned().collect::<Vec<_>>();
            let mutations = operations.iter().map(|(selector, key, operation)| AtomicStoreMutation {
                store: &handles[selector % handles.len()],
                key: vec![*key],
                operation: operation.clone(),
            }).collect::<Vec<_>>();
            let expected = dense_reference(&before, &operations);
            let actual = strict_atomic_mutate(&mutations);
            prop_assert_eq!(actual.is_ok(), expected.is_some());
            if actual.is_err() { prop_assert!(matches!(actual, Err(KvStoreError::TransactionConflict(_)))); }
            let observed = stores.iter().map(|store| store.to_map().unwrap()).collect::<Vec<_>>();
            prop_assert_eq!(observed, expected.unwrap_or(before));
        }
    }

    #[test]
    fn sparse_staging_preserves_deletions_across_later_operations() {
        let store = InMemoryKeyValueStore::new();
        let alias = store.clone();
        let key = vec![1];
        store.put_one(key.clone(), vec![1]).unwrap();
        let operations = [
            AtomicStoreOperation::Delete,
            AtomicStoreOperation::CompareAndSwap {
                expected: None,
                replacement: Some(vec![2]),
            },
            AtomicStoreOperation::PutIfAbsentOrEqual(vec![2]),
            AtomicStoreOperation::Put(vec![3]),
            AtomicStoreOperation::CompareAndSwap {
                expected: Some(vec![3]),
                replacement: None,
            },
            AtomicStoreOperation::PutIfAbsentOrEqual(vec![4]),
        ];
        let mutations = operations
            .into_iter()
            .enumerate()
            .map(|(index, operation)| AtomicStoreMutation {
                store: if index % 2 == 0 { &store } else { &alias },
                key: key.clone(),
                operation,
            })
            .collect::<Vec<_>>();
        strict_atomic_mutate(&mutations).unwrap();
        assert_eq!(store.get_one(&key).unwrap(), Some(vec![4]));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        #[test]
        fn state_import_retry_budget_uses_disjoint_production_alias_keys(
            prefix in any::<[u8; 31]>(),
            suffixes in proptest::collection::vec(any::<u8>(), 1..65),
        ) {
            use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;

            let mut physical = BTreeMap::new();
            let mut logical = std::collections::BTreeSet::new();
            for suffix in suffixes.iter().chain(&suffixes) {
                let mut bytes = prefix.to_vec();
                bytes.push(*suffix);
                let hash = Blake2b256Hash::from_bytes(bytes);
                let raw = hash.bytes();
                let legacy = bincode::serialize(&hash).unwrap();
                let mut expected_legacy = 32u64.to_le_bytes().to_vec();
                expected_legacy.extend(&raw);
                prop_assert_eq!(raw.len(), 32);
                prop_assert_eq!(legacy.len(), 40);
                prop_assert_ne!(&raw, &legacy);
                prop_assert_eq!(&legacy, &expected_legacy);
                prop_assert_eq!(bincode::deserialize::<Blake2b256Hash>(&legacy).unwrap(), hash.clone());
                logical.insert(raw.clone());
                for (alias, key) in [raw.clone(), legacy].into_iter().enumerate() {
                    let value = (raw.clone(), alias);
                    if let Some(previous) = physical.insert(key, value.clone()) {
                        prop_assert_eq!(previous, value);
                    }
                }
            }
            prop_assert_eq!(physical.len(), 2 * logical.len());
        }

        #[test]
        fn state_import_retry_budget_tracks_distinct_absent_aliases(
            requested in proptest::collection::vec(0u8..32, 1..65),
            initially_present in proptest::collection::vec(any::<bool>(), 64),
            arrivals in proptest::collection::vec(0usize..256, 0..129),
        ) {
            let store = InMemoryKeyValueStore::new();
            let locations = requested.iter().flat_map(|key| [vec![*key, 0], vec![*key, 1]])
                .collect::<std::collections::BTreeSet<_>>().into_iter().collect::<Vec<_>>();
            for key in &locations {
                if initially_present[usize::from(key[0]) * 2 + usize::from(key[1])] {
                    store.put_one(key.clone(), vec![key[0]]).unwrap();
                }
            }
            let mut arrivals = arrivals.into_iter();
            let mut budget = None;
            let mut previous = None::<BTreeMap<Vec<u8>, Option<Vec<u8>>>>;
            let mut conflicts = 0usize;
            let mut attempts = 0usize;
            let mut charged = std::collections::BTreeSet::new();
            loop {
                let mut observed = BTreeMap::new();
                for key in &locations {
                    if let Some(arrival) = arrivals.next() {
                        let inserted = &locations[arrival % locations.len()];
                        strict_atomic_mutate(&[AtomicStoreMutation {
                            store: &store, key: inserted.clone(),
                            operation: AtomicStoreOperation::PutIfAbsentOrEqual(vec![inserted[0]]),
                        }]).unwrap();
                    }
                    observed.insert(key.clone(), store.get_one(key).unwrap());
                }
                let missing = observed.values().filter(|value| value.is_none()).count();
                let initial_budget = *budget.get_or_insert(missing);
                if let Some(old) = previous.as_ref() {
                    prop_assert!(missing < old.values().filter(|value| value.is_none()).count());
                    for (key, value) in old {
                        if value.is_some() { prop_assert_eq!(observed.get(key), Some(value)); }
                    }
                }
                prop_assert!(conflicts + missing <= initial_budget);
                if let Some(key) = observed.iter().find_map(|(key, value)| value.is_none().then_some(key)) {
                    strict_atomic_mutate(&[AtomicStoreMutation {
                        store: &store, key: key.clone(),
                        operation: AtomicStoreOperation::PutIfAbsentOrEqual(vec![key[0]]),
                    }]).unwrap();
                }
                let before = store.to_map().unwrap();
                let mutations = observed.iter().map(|(key, expected)| AtomicStoreMutation {
                    store: &store, key: key.clone(),
                    operation: AtomicStoreOperation::CompareAndSwap {
                        expected: expected.clone(), replacement: Some(vec![key[0]]),
                    },
                }).collect::<Vec<_>>();
                attempts += 1;
                prop_assert!(attempts <= initial_budget + 1);
                match strict_atomic_mutate(&mutations) {
                    Ok(()) => {
                        prop_assert_eq!(store.to_map().unwrap(), locations.iter()
                            .map(|key| (key.clone(), vec![key[0]])).collect::<BTreeMap<_, _>>());
                        break;
                    }
                    Err(KvStoreError::TransactionConflict(_)) => {
                        conflicts += 1;
                        prop_assert!(conflicts <= initial_budget);
                        prop_assert_eq!(store.to_map().unwrap(), before.clone());
                        let changed = observed.iter().find(|(key, expected)| before.get(*key) != expected.as_ref())
                            .expect("a normalized CAS conflict must have an external mismatch");
                        prop_assert!(changed.1.is_none());
                        prop_assert!(charged.insert(changed.0.clone()));
                        previous = Some(observed);
                    }
                    Err(error) => return Err(TestCaseError::fail(format!("unexpected storage error: {error}"))),
                }
            }
        }
    }

    #[test]
    fn sparse_transactions_preserve_unrelated_allocations_on_commit_and_rollback() {
        let coordinator = Arc::new(RwLock::new(()));
        let stores = [
            InMemoryKeyValueStore::new_with_coordinator(coordinator.clone()),
            InMemoryKeyValueStore::new_with_coordinator(coordinator),
        ];
        let unrelated_key = vec![0];
        for store in &stores {
            store
                .put_one(unrelated_key.clone(), vec![7; 1024 * 1024])
                .unwrap();
        }
        let addresses = stores
            .iter()
            .map(|store| store.state.get(&unrelated_key).unwrap().value().as_ptr() as usize)
            .collect::<Vec<_>>();
        for succeeds in [true, false] {
            let mutations = [
                AtomicStoreMutation {
                    store: &stores[0],
                    key: vec![1],
                    operation: AtomicStoreOperation::Put(vec![9]),
                },
                AtomicStoreMutation {
                    store: &stores[1],
                    key: vec![1],
                    operation: AtomicStoreOperation::CompareAndSwap {
                        expected: if succeeds { None } else { Some(vec![0]) },
                        replacement: Some(vec![9]),
                    },
                },
            ];
            assert_eq!(strict_atomic_mutate(&mutations).is_ok(), succeeds);
            for (store, address) in stores.iter().zip(&addresses) {
                assert_eq!(
                    store.state.get(&unrelated_key).unwrap().value().as_ptr() as usize,
                    *address
                );
            }
        }
    }

    #[test]
    fn concurrent_sparse_transactions_publish_only_the_winning_cross_store_write() {
        let coordinator = Arc::new(RwLock::new(()));
        let guard = InMemoryKeyValueStore::new_with_coordinator(coordinator.clone());
        let payloads = InMemoryKeyValueStore::new_with_coordinator(coordinator);
        guard.put_one(vec![0], vec![0]).unwrap();
        let start = Arc::new(std::sync::Barrier::new(3));
        let threads = (1..=2u8)
            .map(|id| {
                let guard = guard.clone();
                let payloads = payloads.clone();
                let start = start.clone();
                std::thread::spawn(move || {
                    start.wait();
                    let result = strict_atomic_mutate(&[
                        AtomicStoreMutation {
                            store: &payloads,
                            key: vec![id],
                            operation: AtomicStoreOperation::Put(vec![id]),
                        },
                        AtomicStoreMutation {
                            store: &guard,
                            key: vec![0],
                            operation: AtomicStoreOperation::CompareAndSwap {
                                expected: Some(vec![0]),
                                replacement: Some(vec![id]),
                            },
                        },
                    ]);
                    (id, result)
                })
            })
            .collect::<Vec<_>>();
        start.wait();
        let results = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(results.iter().filter(|(_, result)| result.is_ok()).count(), 1);
        let winner = results.iter().find(|(_, result)| result.is_ok()).unwrap().0;
        assert_eq!(guard.get_one(&vec![0]).unwrap(), Some(vec![winner]));
        assert_eq!(payloads.to_map().unwrap(), BTreeMap::from([(vec![winner], vec![winner])]));
    }

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

    #[test]
    fn borrowed_read_holds_the_original_value_and_releases_its_guard_on_error() {
        let store = InMemoryKeyValueStore::new();
        let key = vec![1];
        store.put(vec![(key.clone(), vec![7; 4096])]).unwrap();
        let address = store.state.get(&key).unwrap().value().as_ptr();
        let mut calls = 0;
        let result = store.with_value(&key, &mut |bytes| {
            calls += 1;
            assert_eq!(bytes.unwrap().as_ptr(), address);
            assert!(matches!(
                store.coordinator.try_write(),
                Err(std::sync::TryLockError::WouldBlock)
            ));
            Err(KvStoreError::InvalidArgument("injected reader error".to_string()))
        });
        assert_eq!(calls, 1);
        assert!(result.is_err());
        assert!(store.coordinator.try_write().is_ok());
        store.put(vec![(key.clone(), vec![8])]).unwrap();
        store
            .with_value(&key, &mut |bytes| {
                assert_eq!(bytes, Some([8].as_slice()));
                Ok(())
            })
            .unwrap();
        store
            .with_value(&vec![2], &mut |bytes| {
                assert!(bytes.is_none());
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn borrowed_scan_visits_original_rows_once_and_releases_after_error() {
        let store = InMemoryKeyValueStore::new();
        store
            .put((0..8u8).map(|key| (vec![key], vec![key; 4096])).collect())
            .unwrap();
        let addresses = store
            .state
            .iter()
            .map(|entry| {
                (
                    entry.key().clone(),
                    (entry.key().as_ptr() as usize, entry.value().as_ptr() as usize),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let mut visited = BTreeMap::new();
        store
            .visit_entries(&mut |key, value| {
                assert_eq!(addresses[key], (key.as_ptr() as usize, value.as_ptr() as usize));
                assert!(store.coordinator.try_write().is_err());
                assert!(visited.insert(key.to_vec(), value.to_vec()).is_none());
                Ok(())
            })
            .unwrap();
        assert_eq!(visited, store.to_map().unwrap());
        let mut calls = 0;
        let error = KvStoreError::InvalidArgument("injected visitor error".to_string());
        assert_eq!(
            store.visit_entries(&mut |_, _| {
                calls += 1;
                Err(error.clone())
            }),
            Err(error)
        );
        assert_eq!(calls, 1);
        assert!(store.coordinator.try_write().is_ok());
        store.put(vec![(vec![9], vec![9])]).unwrap();
        assert_eq!(store.to_map().unwrap().len(), 9);
    }

    #[test]
    fn borrowed_scan_excludes_concurrent_replacement_until_snapshot_release() {
        let store = InMemoryKeyValueStore::new();
        let expected = (0..8u8)
            .map(|key| (vec![key], vec![key]))
            .collect::<BTreeMap<_, _>>();
        store.put(expected.clone().into_iter().collect()).unwrap();
        let writer = store.clone();
        let (begin_tx, begin_rx) = std::sync::mpsc::channel();
        let (blocked_tx, blocked_rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            begin_rx.recv().unwrap();
            assert!(matches!(
                writer.coordinator.try_write(),
                Err(std::sync::TryLockError::WouldBlock)
            ));
            blocked_tx.send(()).unwrap();
            writer
                .put((0..9u8).map(|key| (vec![key], vec![9])).collect())
                .unwrap();
        });
        let mut observed = BTreeMap::new();
        store
            .visit_entries(&mut |key, value| {
                if observed.is_empty() {
                    begin_tx.send(()).unwrap();
                    blocked_rx
                        .recv_timeout(std::time::Duration::from_secs(10))
                        .unwrap();
                }
                observed.insert(key.to_vec(), value.to_vec());
                Ok(())
            })
            .unwrap();
        thread.join().unwrap();
        assert_eq!(observed, expected);
        assert_eq!(store.to_map().unwrap().len(), 9);
        assert_eq!(store.get_one(&vec![0]).unwrap(), Some(vec![9]));
    }
}
