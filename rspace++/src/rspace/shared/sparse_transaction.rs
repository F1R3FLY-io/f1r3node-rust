use std::collections::BTreeMap;

pub(super) enum Operation<'a, V> {
    Put(&'a V),
    PutIfAbsentOrEqual(&'a V),
    Delete,
    CompareAndSwap {
        expected: &'a Option<V>,
        replacement: &'a Option<V>,
    },
}

pub(super) struct Mutation<'a, S: Store> {
    pub store: &'a S,
    pub key: &'a S::Key,
    pub operation: Operation<'a, S::Value>,
}

pub(super) trait Store {
    type Key: Clone + Ord;
    type Value: Clone + Eq;
    type Error;
    type ReadGuard<'a>
    where Self: 'a;
    type WriteGuard<'a>
    where Self: 'a;

    fn same_manager(&self, other: &Self) -> bool;
    fn same_store(&self, other: &Self) -> bool;
    fn read_guard(&self) -> Self::ReadGuard<'_>;
    fn write_guard(&self) -> Self::WriteGuard<'_>;
    fn current(&self, key: &Self::Key) -> Option<Self::Value>;
    fn put(&self, key: Self::Key, value: Self::Value);
    fn delete(&self, key: &Self::Key);
    fn manager_error() -> Self::Error;
    fn conflict(key: &Self::Key, compare_and_swap: bool) -> Self::Error;
}

pub(super) fn with_snapshot<S: Store, R>(store: &S, read: impl FnOnce() -> R) -> R {
    let _guard = store.read_guard();
    read()
}

type StagedValues<S> = BTreeMap<<S as Store>::Key, Option<<S as Store>::Value>>;
type StagedStore<'a, S> = (&'a S, StagedValues<S>);

pub(super) fn apply<S: Store>(
    coordinator: &S,
    mutations: &[Mutation<'_, S>],
) -> Result<(), S::Error> {
    if mutations
        .iter()
        .any(|mutation| !coordinator.same_manager(mutation.store))
    {
        return Err(S::manager_error());
    }
    let _guard = coordinator.write_guard();
    let mut staged_stores: Vec<StagedStore<'_, S>> = Vec::new();
    for mutation in mutations {
        if staged_stores
            .iter()
            .any(|(store, _)| store.same_store(mutation.store))
        {
            continue;
        }
        staged_stores.push((mutation.store, BTreeMap::new()));
    }
    for mutation in mutations {
        let (_, staged) = staged_stores
            .iter_mut()
            .find(|(store, _)| store.same_store(mutation.store))
            .expect("transaction staging map exists");
        match &mutation.operation {
            Operation::Put(value) => {
                staged.insert(mutation.key.clone(), Some((*value).clone()));
            }
            Operation::PutIfAbsentOrEqual(value) => {
                let current = staged
                    .entry(mutation.key.clone())
                    .or_insert_with(|| mutation.store.current(mutation.key));
                match current {
                    Some(existing) if existing != *value => {
                        return Err(S::conflict(mutation.key, false));
                    }
                    Some(_) => {}
                    None => *current = Some((*value).clone()),
                }
            }
            Operation::Delete => {
                staged.insert(mutation.key.clone(), None);
            }
            Operation::CompareAndSwap {
                expected,
                replacement,
            } => {
                let current = staged
                    .entry(mutation.key.clone())
                    .or_insert_with(|| mutation.store.current(mutation.key));
                if current.as_ref() != expected.as_ref() {
                    return Err(S::conflict(mutation.key, true));
                }
                current.clone_from(replacement);
            }
        }
    }
    for (store, staged) in staged_stores {
        for (key, value) in staged {
            match value {
                Some(value) => store.put(key, value),
                None => store.delete(&key),
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use super::*;

    #[derive(Default)]
    struct ReadCountingStore {
        values: RefCell<BTreeMap<Vec<u8>, Vec<u8>>>,
        reads: Cell<usize>,
    }

    impl Store for ReadCountingStore {
        type Key = Vec<u8>;
        type Value = Vec<u8>;
        type Error = ();
        type ReadGuard<'a> = ();
        type WriteGuard<'a> = ();

        fn same_manager(&self, other: &Self) -> bool { std::ptr::eq(self, other) }
        fn same_store(&self, other: &Self) -> bool { std::ptr::eq(self, other) }
        fn read_guard(&self) -> Self::ReadGuard<'_> {}
        fn write_guard(&self) -> Self::WriteGuard<'_> {}
        fn current(&self, key: &Self::Key) -> Option<Self::Value> {
            self.reads.set(self.reads.get() + 1);
            self.values.borrow().get(key).cloned()
        }
        fn put(&self, key: Self::Key, value: Self::Value) {
            self.values.borrow_mut().insert(key, value);
        }
        fn delete(&self, key: &Self::Key) { self.values.borrow_mut().remove(key); }
        fn manager_error() -> Self::Error {}
        fn conflict(_key: &Self::Key, _compare_and_swap: bool) -> Self::Error {}
    }

    #[test]
    fn unconditional_mutations_skip_existing_values_and_preserve_staged_dependencies() {
        let store = ReadCountingStore::default();
        store
            .values
            .borrow_mut()
            .insert(b"root".to_vec(), vec![7; 1_000_000]);
        store
            .values
            .borrow_mut()
            .insert(b"old".to_vec(), vec![8; 1_000_000]);
        let root = b"root".to_vec();
        let old = b"old".to_vec();
        let new_root = b"new-root".to_vec();
        let mutations = [
            Mutation {
                store: &store,
                key: &root,
                operation: Operation::Put(&new_root),
            },
            Mutation {
                store: &store,
                key: &root,
                operation: Operation::CompareAndSwap {
                    expected: &Some(new_root.clone()),
                    replacement: &Some(b"final-root".to_vec()),
                },
            },
            Mutation {
                store: &store,
                key: &old,
                operation: Operation::Delete,
            },
            Mutation {
                store: &store,
                key: &old,
                operation: Operation::CompareAndSwap {
                    expected: &None,
                    replacement: &Some(b"replaced".to_vec()),
                },
            },
        ];
        apply(&store, &mutations).unwrap();
        assert_eq!(store.reads.get(), 0);
        let final_root = b"final-root".to_vec();
        assert_eq!(store.values.borrow().get(&root), Some(&final_root));
        assert_eq!(store.values.borrow().get(&old), Some(&b"replaced".to_vec()));
    }
}
