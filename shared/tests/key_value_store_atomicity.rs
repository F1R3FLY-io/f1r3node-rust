use std::sync::Arc;

use heed::types::SerdeBincode;
use heed::EnvOpenOptions;
use shared::rust::store::key_value_store::{KeyValueStore, KeyValueStoreMutation, KvStoreError};
use shared::rust::store::lmdb_key_value_store::LmdbKeyValueStore;

fn lmdb_store() -> (tempfile::TempDir, LmdbKeyValueStore) {
    let directory = tempfile::tempdir().unwrap();
    let mut options = EnvOpenOptions::new();
    options.map_size(16 * 1024 * 1024).max_dbs(2);
    let env = Arc::new(unsafe { options.open(directory.path()).unwrap() });
    let mut transaction = env.write_txn().unwrap();
    let database = env
        .create_database::<SerdeBincode<Vec<u8>>, SerdeBincode<Vec<u8>>>(
            &mut transaction,
            Some("atomicity"),
        )
        .unwrap();
    transaction.commit().unwrap();
    (directory, LmdbKeyValueStore::new(env, database))
}

fn assert_atomic_compare_and_swap(store: &dyn KeyValueStore) {
    let key_a = b"a".to_vec();
    let key_b = b"b".to_vec();
    store.put_one(key_a.clone(), b"old".to_vec()).unwrap();

    let mismatch = store
        .compare_and_swap_batch(
            &[
                (key_a.clone(), Some(b"wrong".to_vec())),
                (key_b.clone(), None),
            ],
            &[
                KeyValueStoreMutation::Put(key_a.clone(), b"new".to_vec()),
                KeyValueStoreMutation::Put(key_b.clone(), b"created".to_vec()),
            ],
        )
        .unwrap();
    assert!(!mismatch);
    assert_eq!(store.get_one(&key_a).unwrap(), Some(b"old".to_vec()));
    assert_eq!(store.get_one(&key_b).unwrap(), None);

    let swapped = store
        .compare_and_swap_batch(
            &[
                (key_a.clone(), Some(b"old".to_vec())),
                (key_b.clone(), None),
            ],
            &[
                KeyValueStoreMutation::Put(key_a.clone(), b"new".to_vec()),
                KeyValueStoreMutation::Put(key_b.clone(), b"created".to_vec()),
            ],
        )
        .unwrap();
    assert!(swapped);
    assert_eq!(store.get_one(&key_a).unwrap(), Some(b"new".to_vec()));
    assert_eq!(store.get_one(&key_b).unwrap(), Some(b"created".to_vec()));
}

#[test]
fn lmdb_compare_and_swap_commits_all_mutations_in_one_transaction() {
    let (_directory, store) = lmdb_store();
    assert_atomic_compare_and_swap(&store);
}

#[test]
fn lmdb_put_if_absent_rejects_a_different_existing_value_without_mutation() {
    let (_directory, store) = lmdb_store();
    let present = b"present".to_vec();
    let absent = b"absent".to_vec();
    store
        .put_one(present.clone(), b"authoritative".to_vec())
        .unwrap();

    let result = store.put_if_absent_or_verify_equal(vec![
        (present.clone(), b"collision".to_vec()),
        (absent.clone(), b"must-not-appear".to_vec()),
    ]);
    assert!(matches!(result, Err(KvStoreError::InvalidArgument(_))));
    assert_eq!(
        store.get_one(&present).unwrap(),
        Some(b"authoritative".to_vec())
    );
    assert_eq!(store.get_one(&absent).unwrap(), None);
}

#[test]
fn lmdb_size_bytes_is_exact_and_fallible() {
    let (_directory, store) = lmdb_store();
    store
        .put(vec![
            (b"first".to_vec(), b"value".to_vec()),
            (b"second".to_vec(), b"payload".to_vec()),
        ])
        .unwrap();
    assert_eq!(store.size_bytes().unwrap(), 5 + 5 + 6 + 7);
}
