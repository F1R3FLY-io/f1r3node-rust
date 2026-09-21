use std::collections::BTreeMap;
use std::sync::Arc;

use heed::types::{Bytes, SerdeBincode};
use heed::{Database, Env, EnvOpenOptions};
use shared::rust::store::key_value_store::{KeyValueStore, KvStoreError};
use shared::rust::store::lmdb_key_value_store::LmdbKeyValueStore;
use shared::rust::store::soak_snapshot::{
    decode_length_prefixed, encode_length_prefixed, BoundedLmdbReader, ReadLimits, SnapshotError,
    LENGTH_PREFIX_BYTES,
};
use shared::rust::ByteBuffer;

struct Fixture {
    _dir: tempfile::TempDir,
    env: Arc<Env>,
}

fn open_env() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let mut builder = EnvOpenOptions::new();
    builder.map_size(16 * 1024 * 1024);
    builder.max_dbs(8);
    let env = unsafe { builder.open(dir.path()).unwrap() };
    Fixture {
        _dir: dir,
        env: Arc::new(env),
    }
}

fn open_store(env: &Arc<Env>, name: &str) -> (Arc<dyn KeyValueStore>, Arc<LmdbKeyValueStore>) {
    let mut wtxn = env.write_txn().unwrap();
    let db: Database<SerdeBincode<ByteBuffer>, SerdeBincode<ByteBuffer>> =
        env.create_database(&mut wtxn, Some(name)).unwrap();
    wtxn.commit().unwrap();
    let store = Arc::new(LmdbKeyValueStore::new(env.clone(), db));
    let dynamic: Arc<dyn KeyValueStore> = store.clone();
    (dynamic, store)
}

fn limits() -> ReadLimits {
    ReadLimits {
        max_value_bytes: 4096,
        max_total_bytes: 1 << 20,
        max_records: 64,
        max_operations: 256,
    }
}

fn kv(pairs: &[(&str, &str)]) -> Vec<(ByteBuffer, ByteBuffer)> {
    pairs
        .iter()
        .map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec()))
        .collect()
}

fn key(name: &str) -> ByteBuffer { name.as_bytes().to_vec() }

fn write_from_other_thread(
    env: Arc<Env>,
    store: Arc<LmdbKeyValueStore>,
    pairs: Vec<(ByteBuffer, ByteBuffer)>,
) {
    std::thread::spawn(move || {
        let _keep = env;
        store.put(pairs).unwrap();
    })
    .join()
    .unwrap();
}

#[derive(Clone)]
struct MemoryStore {
    map: Arc<std::sync::Mutex<BTreeMap<ByteBuffer, ByteBuffer>>>,
}

impl KeyValueStore for MemoryStore {
    fn as_any(&self) -> &dyn std::any::Any { self }

    fn get(&self, keys: &Vec<ByteBuffer>) -> Result<Vec<Option<ByteBuffer>>, KvStoreError> {
        let map = self.map.lock().unwrap();
        Ok(keys.iter().map(|k| map.get(k).cloned()).collect())
    }

    fn put(&self, kv_pairs: Vec<(ByteBuffer, ByteBuffer)>) -> Result<(), KvStoreError> {
        let mut map = self.map.lock().unwrap();
        for (k, v) in kv_pairs {
            map.insert(k, v);
        }
        Ok(())
    }

    fn put_one_if_absent(&self, key: ByteBuffer, value: ByteBuffer) -> Result<bool, KvStoreError> {
        let mut map = self.map.lock().unwrap();
        if map.contains_key(&key) {
            return Ok(false);
        }
        map.insert(key, value);
        Ok(true)
    }

    fn delete(&self, keys: Vec<ByteBuffer>) -> Result<usize, KvStoreError> {
        let mut map = self.map.lock().unwrap();
        Ok(keys.iter().filter(|k| map.remove(*k).is_some()).count())
    }

    fn iterate(&self, _f: fn(ByteBuffer, ByteBuffer)) -> Result<(), KvStoreError> { Ok(()) }

    fn iterate_while(
        &self,
        _f: &mut dyn FnMut(ByteBuffer, ByteBuffer) -> Result<bool, KvStoreError>,
    ) -> Result<(), KvStoreError> {
        Ok(())
    }

    fn clone_box(&self) -> Box<dyn KeyValueStore> { Box::new(self.clone()) }

    fn to_map(&self) -> Result<BTreeMap<ByteBuffer, ByteBuffer>, KvStoreError> {
        Ok(self.map.lock().unwrap().clone())
    }

    fn print_store(&self) -> Result<(), KvStoreError> { Ok(()) }

    fn non_empty(&self) -> Result<bool, KvStoreError> { Ok(!self.map.lock().unwrap().is_empty()) }

    fn size_bytes(&self) -> usize { 0 }
}

#[test]
fn invalid_limits_are_rejected_before_any_store_access() {
    let fixture = open_env();
    let (store, _) = open_store(&fixture.env, "s");
    let cases = [
        ReadLimits {
            max_value_bytes: LENGTH_PREFIX_BYTES - 1,
            ..limits()
        },
        ReadLimits {
            max_total_bytes: 8,
            max_value_bytes: 16,
            ..limits()
        },
        ReadLimits {
            max_records: 0,
            ..limits()
        },
        ReadLimits {
            max_operations: 0,
            ..limits()
        },
    ];
    for case in cases {
        let result = BoundedLmdbReader::open(&[&store], case);
        assert!(matches!(
            result.err(),
            Some(SnapshotError::InvalidLimits(_))
        ));
    }
}

#[test]
fn unsupported_backend_is_rejected_instead_of_substituted() {
    let memory: Arc<dyn KeyValueStore> = Arc::new(MemoryStore {
        map: Arc::new(std::sync::Mutex::new(BTreeMap::new())),
    });
    memory.put(kv(&[("k", "v")])).unwrap();
    let result = BoundedLmdbReader::open(&[&memory], limits());
    assert!(matches!(result.err(), Some(SnapshotError::Unsupported(_))));

    let fixture = open_env();
    let (lmdb, _) = open_store(&fixture.env, "s");
    let mixed = BoundedLmdbReader::open(&[&lmdb, &memory], limits());
    assert!(matches!(mixed.err(), Some(SnapshotError::Unsupported(_))));
}

#[test]
fn raw_length_is_checked_before_the_value_is_copied() {
    let fixture = open_env();
    let (store, _) = open_store(&fixture.env, "s");
    store
        .put(vec![
            (key("big"), vec![7u8; 1000]),
            (key("small"), vec![1u8; 10]),
        ])
        .unwrap();
    let mut reader = BoundedLmdbReader::open(&[&store], ReadLimits {
        max_value_bytes: 100,
        ..limits()
    })
    .unwrap();
    let result = reader.read_raw(&store, &key("big"));
    assert_eq!(
        result,
        Err(SnapshotError::LimitExceeded {
            kind: "value bytes",
            limit: 100,
            observed: 1000 + LENGTH_PREFIX_BYTES,
        })
    );
    assert_eq!(reader.usage().bytes, 0);
    assert_eq!(reader.usage().records, 0);
    assert_eq!(reader.usage().operations, 1);

    let small = reader.read_value(&store, &key("small")).unwrap();
    assert_eq!(small, Some(vec![1u8; 10]));
    assert_eq!(reader.usage().records, 1);
    assert_eq!(reader.usage().bytes, 10 + LENGTH_PREFIX_BYTES);
}

#[test]
fn total_bytes_records_and_operations_budgets_are_enforced() {
    let fixture = open_env();
    let (store, _) = open_store(&fixture.env, "s");
    store
        .put(kv(&[("a", "1"), ("b", "2"), ("c", "3"), ("d", "4")]))
        .unwrap();

    let mut by_records = BoundedLmdbReader::open(&[&store], ReadLimits {
        max_records: 2,
        ..limits()
    })
    .unwrap();
    assert!(matches!(
        by_records.scan(&store),
        Err(SnapshotError::LimitExceeded {
            kind: "records",
            ..
        })
    ));
    let while_open = BoundedLmdbReader::open(&[&store], limits());
    assert!(matches!(
        while_open.err(),
        Some(SnapshotError::ReadFailed(_))
    ));
    drop(by_records);

    let mut by_operations = BoundedLmdbReader::open(&[&store], ReadLimits {
        max_operations: 1,
        ..limits()
    })
    .unwrap();
    by_operations.read_raw(&store, &key("a")).unwrap();
    assert!(matches!(
        by_operations.read_raw(&store, &key("b")),
        Err(SnapshotError::LimitExceeded {
            kind: "operations",
            ..
        })
    ));
    drop(by_operations);

    let mut by_total = BoundedLmdbReader::open(&[&store], ReadLimits {
        max_value_bytes: 16,
        max_total_bytes: 20,
        ..limits()
    })
    .unwrap();
    by_total.read_raw(&store, &key("a")).unwrap();
    by_total.read_raw(&store, &key("b")).unwrap();
    assert!(matches!(
        by_total.read_raw(&store, &key("c")),
        Err(SnapshotError::LimitExceeded {
            kind: "total bytes",
            ..
        })
    ));
}

#[test]
fn malformed_length_prefix_is_rejected() {
    assert!(matches!(
        decode_length_prefixed(&[1, 2, 3]),
        Err(SnapshotError::Malformed(_))
    ));
    let mut oversized = encode_length_prefixed(b"abc");
    oversized[0] = 200;
    assert!(matches!(
        decode_length_prefixed(&oversized),
        Err(SnapshotError::Malformed(_))
    ));
    assert_eq!(
        decode_length_prefixed(&encode_length_prefixed(b"abc")).unwrap(),
        b"abc".to_vec()
    );

    let fixture = open_env();
    let (store, lmdb) = open_store(&fixture.env, "s");
    let raw_db: Database<Bytes, Bytes> = lmdb.db.remap_types::<Bytes, Bytes>();
    let mut wtxn = fixture.env.write_txn().unwrap();
    let mut bad = encode_length_prefixed(b"payload");
    bad[0] = 99;
    raw_db
        .put(
            &mut wtxn,
            encode_length_prefixed(b"bad").as_slice(),
            bad.as_slice(),
        )
        .unwrap();
    wtxn.commit().unwrap();

    let mut reader = BoundedLmdbReader::open(&[&store], limits()).unwrap();
    assert!(matches!(
        reader.read_value(&store, &key("bad")),
        Err(SnapshotError::Malformed(_))
    ));
    assert!(matches!(
        reader.scan(&store),
        Err(SnapshotError::Malformed(_))
    ));
}

#[test]
fn write_between_open_and_validation_is_rejected() {
    let fixture = open_env();
    let (store, lmdb) = open_store(&fixture.env, "s");
    store.put(kv(&[("k1", "v1")])).unwrap();

    let mut reader = BoundedLmdbReader::open(&[&store], limits()).unwrap();
    let before = reader.identities();
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].txn_id, before[0].last_txn_id_before_open);
    assert_eq!(
        reader.read_value(&store, &key("k1")).unwrap(),
        Some(b"v1".to_vec())
    );

    write_from_other_thread(fixture.env.clone(), lmdb, kv(&[("k2", "v2")]));

    let result = reader.validate();
    assert_eq!(
        result,
        Err(SnapshotError::EnvironmentChanged {
            environment: fixture.env.path().display().to_string(),
            opened: before[0].txn_id,
            observed: before[0].txn_id + 1,
        })
    );
}

#[test]
fn value_changed_back_to_original_bytes_is_still_rejected() {
    let fixture = open_env();
    let (store, lmdb) = open_store(&fixture.env, "s");
    store.put(kv(&[("k1", "v1")])).unwrap();

    let mut reader = BoundedLmdbReader::open(&[&store], limits()).unwrap();
    let captured = reader.read_value(&store, &key("k1")).unwrap();

    write_from_other_thread(fixture.env.clone(), lmdb.clone(), kv(&[("k1", "other")]));
    write_from_other_thread(fixture.env.clone(), lmdb, kv(&[("k1", "v1")]));

    assert_eq!(captured, Some(b"v1".to_vec()));
    assert!(matches!(
        reader.validate(),
        Err(SnapshotError::EnvironmentChanged { .. })
    ));
    assert_eq!(store.get_one(&key("k1")).unwrap(), Some(b"v1".to_vec()));
}

#[test]
fn unchanged_environment_validates_and_stored_bytes_are_untouched() {
    let fixture = open_env();
    let (store, _) = open_store(&fixture.env, "s");
    store.put(kv(&[("k1", "v1"), ("k2", "v2")])).unwrap();
    let before = store.to_map().unwrap();

    let mut reader = BoundedLmdbReader::open(&[&store], limits()).unwrap();
    let scanned = reader.scan(&store).unwrap();
    assert_eq!(scanned.len(), 2);
    assert_eq!(scanned.get(&key("k1")), Some(&b"v1".to_vec()));
    assert_eq!(reader.read_value(&store, &key("missing")).unwrap(), None);
    let identities = reader.validate().unwrap();
    assert_eq!(identities.len(), 1);
    assert_eq!(
        identities[0].last_txn_id_after_validation,
        Some(identities[0].txn_id)
    );
    assert_eq!(identities[0].txn_id, identities[0].last_txn_id_before_open);

    assert_eq!(store.to_map().unwrap(), before);
}

#[test]
fn stores_in_one_environment_share_one_transaction_and_a_second_same_thread_reader_fails() {
    let fixture = open_env();
    let (a, _) = open_store(&fixture.env, "a");
    let (b, _) = open_store(&fixture.env, "b");
    a.put(kv(&[("k", "a")])).unwrap();
    b.put(kv(&[("k", "b")])).unwrap();

    let mut reader = BoundedLmdbReader::open(&[&a, &b], limits()).unwrap();
    assert_eq!(reader.identities().len(), 1);
    assert_eq!(
        reader.read_value(&a, &key("k")).unwrap(),
        Some(b"a".to_vec())
    );
    assert_eq!(
        reader.read_value(&b, &key("k")).unwrap(),
        Some(b"b".to_vec())
    );

    let nested = fixture.env.read_txn();
    assert!(nested.is_err());
    drop(nested);

    let same_thread_trait_read = a.get_one(&key("k"));
    assert!(same_thread_trait_read.is_err());

    reader.validate().unwrap();
    assert_eq!(a.get_one(&key("k")).unwrap(), Some(b"a".to_vec()));
}

#[test]
fn separate_environments_are_opened_and_validated_together() {
    let first = open_env();
    let second = open_env();
    let (a, _) = open_store(&first.env, "a");
    let (b, lmdb_b) = open_store(&second.env, "b");
    a.put(kv(&[("k", "a")])).unwrap();
    b.put(kv(&[("k", "b")])).unwrap();

    let mut reader = BoundedLmdbReader::open(&[&a, &b], limits()).unwrap();
    assert_eq!(reader.identities().len(), 2);
    assert_eq!(
        reader.read_value(&a, &key("k")).unwrap(),
        Some(b"a".to_vec())
    );
    assert_eq!(
        reader.read_value(&b, &key("k")).unwrap(),
        Some(b"b".to_vec())
    );
    let identities = reader.validate().unwrap();
    assert_eq!(identities.len(), 2);
    assert_ne!(identities[0].environment, identities[1].environment);

    let mut changed = BoundedLmdbReader::open(&[&a, &b], limits()).unwrap();
    changed.read_value(&a, &key("k")).unwrap();
    write_from_other_thread(second.env.clone(), lmdb_b, kv(&[("k", "changed")]));
    match changed.validate() {
        Err(SnapshotError::EnvironmentChanged { environment, .. }) => {
            assert_eq!(environment, second.env.path().display().to_string());
        }
        other => panic!("expected environment change, got {other:?}"),
    }
}

#[test]
fn failed_scan_retains_consumed_work_and_record_budget() {
    let fixture = open_env();
    let (store, _) = open_store(&fixture.env, "s");
    store.put(kv(&[("a", "1"), ("b", "2")])).unwrap();
    let before = store.to_map().unwrap();
    let mut reader = BoundedLmdbReader::open(&[&store], ReadLimits {
        max_records: 1,
        max_operations: 2,
        ..limits()
    })
    .unwrap();

    assert!(matches!(
        reader.scan(&store),
        Err(SnapshotError::LimitExceeded {
            kind: "records",
            ..
        })
    ));
    assert_eq!(reader.usage().records, 1);
    assert_eq!(reader.usage().operations, 2);
    assert_eq!(reader.usage().bytes, 2 * (LENGTH_PREFIX_BYTES + 1));
    assert!(matches!(
        reader.scan(&store),
        Err(SnapshotError::LimitExceeded {
            kind: "operations",
            ..
        })
    ));
    reader.validate().unwrap();
    assert_eq!(store.to_map().unwrap(), before);
}

#[test]
fn empty_scans_consume_operation_budget() {
    let fixture = open_env();
    let (store, _) = open_store(&fixture.env, "s");
    let mut reader = BoundedLmdbReader::open(&[&store], ReadLimits {
        max_operations: 1,
        ..limits()
    })
    .unwrap();
    assert!(reader.scan(&store).unwrap().is_empty());
    assert_eq!(reader.usage().operations, 1);
    assert!(matches!(
        reader.scan(&store),
        Err(SnapshotError::LimitExceeded {
            kind: "operations",
            ..
        })
    ));
}

#[test]
fn scan_keys_obey_the_per_buffer_limit() {
    let fixture = open_env();
    let (store, _) = open_store(&fixture.env, "s");
    store.put(vec![(vec![7; 64], vec![1])]).unwrap();
    let mut reader = BoundedLmdbReader::open(&[&store], ReadLimits {
        max_value_bytes: 32,
        ..limits()
    })
    .unwrap();
    assert_eq!(
        reader.scan(&store),
        Err(SnapshotError::LimitExceeded {
            kind: "key bytes",
            limit: 32,
            observed: LENGTH_PREFIX_BYTES + 64,
        })
    );
    assert_eq!(reader.usage().records, 0);
    assert_eq!(reader.usage().bytes, 0);
    assert_eq!(reader.usage().operations, 1);
}

#[test]
fn lookup_keys_are_bounded_before_encoding() {
    let fixture = open_env();
    let (store, _) = open_store(&fixture.env, "s");
    let mut reader = BoundedLmdbReader::open(&[&store], ReadLimits {
        max_value_bytes: 32,
        ..limits()
    })
    .unwrap();
    assert_eq!(
        reader.read_raw(&store, &vec![7; 64]),
        Err(SnapshotError::LimitExceeded {
            kind: "key bytes",
            limit: 32,
            observed: LENGTH_PREFIX_BYTES + 64,
        })
    );
    assert_eq!(reader.usage().operations, 1);
    assert_eq!(reader.usage().records, 0);
}

#[test]
fn store_list_is_bounded_before_backend_inspection() {
    let memory: Arc<dyn KeyValueStore> = Arc::new(MemoryStore {
        map: Arc::new(std::sync::Mutex::new(BTreeMap::new())),
    });
    assert_eq!(
        BoundedLmdbReader::open(&[&memory, &memory], ReadLimits {
            max_operations: 1,
            ..limits()
        })
        .err(),
        Some(SnapshotError::LimitExceeded {
            kind: "stores",
            limit: 1,
            observed: 2,
        })
    );
}

#[test]
fn malformed_scan_charges_only_the_attempted_prefix() {
    let fixture = open_env();
    let (store, lmdb) = open_store(&fixture.env, "s");
    store
        .put(kv(&[("a", "1"), ("b", "2"), ("c", "3")]))
        .unwrap();
    let mut writer = fixture.env.write_txn().unwrap();
    lmdb.db
        .remap_types::<Bytes, Bytes>()
        .put(&mut writer, &encode_length_prefixed(b"a"), &[0])
        .unwrap();
    writer.commit().unwrap();
    let mut reader = BoundedLmdbReader::open(&[&store], limits()).unwrap();
    assert!(matches!(
        reader.scan(&store),
        Err(SnapshotError::Malformed(_))
    ));
    assert_eq!(reader.usage().operations, 1);
    assert_eq!(reader.usage().records, 1);
    assert_eq!(reader.usage().bytes, LENGTH_PREFIX_BYTES + 2);
}

#[test]
fn per_scan_limit_checks_before_copy_and_retains_consumption() {
    let fixture = open_env();
    let (store, _) = open_store(&fixture.env, "s");
    store.put(kv(&[("a", "1"), ("b", "2")])).unwrap();
    let mut reader = BoundedLmdbReader::open(&[&store], limits()).unwrap();
    assert!(matches!(
        reader.scan_limited(&store, 1),
        Err(SnapshotError::LimitExceeded {
            kind: "scan records",
            ..
        })
    ));
    assert_eq!(reader.usage().records, 1);
    assert_eq!(reader.usage().operations, 2);
    assert_eq!(reader.usage().bytes, 18);
}

#[test]
fn remaining_budget_cannot_be_increased() {
    let fixture = open_env();
    let (store, _) = open_store(&fixture.env, "s");
    store.put(kv(&[("a", "1")])).unwrap();
    let mut reader = BoundedLmdbReader::open(&[&store], limits()).unwrap();
    reader.restrict_remaining(0, 1).unwrap();
    reader.restrict_remaining(100, 100).unwrap();
    assert!(matches!(
        reader.read_value(&store, &key("a")),
        Err(SnapshotError::LimitExceeded {
            kind: "total bytes",
            ..
        })
    ));
    assert!(matches!(
        reader.read_value(&store, &key("missing")),
        Err(SnapshotError::LimitExceeded {
            kind: "operations",
            ..
        })
    ));
}

#[test]
fn store_outside_the_opened_set_is_refused() {
    let first = open_env();
    let second = open_env();
    let (a, _) = open_store(&first.env, "a");
    let (b, _) = open_store(&second.env, "b");
    let mut reader = BoundedLmdbReader::open(&[&a], limits()).unwrap();
    assert!(matches!(
        reader.read_raw(&b, &key("k")),
        Err(SnapshotError::ReadFailed(_))
    ));
}
