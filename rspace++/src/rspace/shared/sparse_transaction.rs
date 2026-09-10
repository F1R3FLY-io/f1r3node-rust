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
        let current = staged
            .entry(mutation.key.clone())
            .or_insert_with(|| mutation.store.current(mutation.key));
        match &mutation.operation {
            Operation::Put(value) => *current = Some((*value).clone()),
            Operation::PutIfAbsentOrEqual(value) => match current {
                Some(existing) if existing != *value => {
                    return Err(S::conflict(mutation.key, false));
                }
                Some(_) => {}
                None => *current = Some((*value).clone()),
            },
            Operation::Delete => *current = None,
            Operation::CompareAndSwap {
                expected,
                replacement,
            } => {
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
