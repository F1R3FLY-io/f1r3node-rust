pub use shared::rust::store::memory_key_value_store::MemoryKeyValueStore as InMemoryKeyValueStore;

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use shared::rust::store::key_value_store::KeyValueStore;

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
    fn conditional_replacement_preserves_changed_and_missing_memory_records() {
        let store = InMemoryKeyValueStore::new();
        store
            .put(vec![(b"a".to_vec(), b"old".to_vec()), (b"b".to_vec(), b"changed".to_vec())])
            .unwrap();
        assert_eq!(
            store
                .replace_if_values(vec![
                    (b"a".to_vec(), b"old".to_vec(), b"new".to_vec()),
                    (b"b".to_vec(), b"old".to_vec(), b"overwrite".to_vec()),
                    (b"missing".to_vec(), b"old".to_vec(), b"resurrect".to_vec()),
                ])
                .unwrap(),
            1
        );
        assert_eq!(store.get_one(&b"a".to_vec()).unwrap(), Some(b"new".to_vec()));
        assert_eq!(store.get_one(&b"b".to_vec()).unwrap(), Some(b"changed".to_vec()));
        assert_eq!(store.get_one(&b"missing".to_vec()).unwrap(), None);
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
