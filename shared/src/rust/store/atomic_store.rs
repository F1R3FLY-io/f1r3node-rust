use std::collections::BTreeMap;
use std::ops::Bound;
use std::sync::Arc;

use heed::types::SerdeBincode;
use heed::{Database, Env, RoTxn, RwTxn};

use super::key_value_store::{KeyValueStore, KvStoreError};
use super::lmdb_key_value_store::LmdbKeyValueStore;
use super::memory_key_value_store::{MemoryEnvironment, MemoryKeyValueStore, MemoryState};
use super::soak_snapshot::encode_length_prefixed;

type LmdbDatabase = Database<SerdeBincode<Vec<u8>>, SerdeBincode<Vec<u8>>>;
type Changes = BTreeMap<u64, BTreeMap<Vec<u8>, Option<Vec<u8>>>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorePage {
    pub rows: Vec<(Vec<u8>, Vec<u8>)>,
    pub next: Option<Vec<u8>>,
}

#[derive(Clone)]
pub struct AtomicStoreGroup {
    backend: Backend,
}

#[derive(Clone)]
enum Backend {
    Lmdb {
        env: Arc<Env>,
        databases: Vec<LmdbDatabase>,
    },
    Memory {
        env: Arc<MemoryEnvironment>,
        tables: Vec<u64>,
    },
}

pub struct AtomicTransaction<'a> {
    inner: TransactionInner<'a>,
}

enum TransactionInner<'a> {
    Lmdb {
        txn: RwTxn<'a>,
        databases: &'a [LmdbDatabase],
    },
    Memory {
        state: &'a MemoryState,
        tables: &'a [u64],
        changes: Changes,
    },
}

fn invalid(message: impl Into<String>) -> KvStoreError {
    KvStoreError::InvalidArgument(message.into())
}

fn slot<T>(items: &[T], index: usize) -> Result<&T, KvStoreError> {
    items
        .get(index)
        .ok_or_else(|| invalid(format!("Atomic store slot {index} is not registered")))
}

fn check_limit(limit: usize) -> Result<(), KvStoreError> {
    if limit == 0 || limit > 1024 {
        return Err(invalid("Atomic scan limit must be between 1 and 1024"));
    }
    Ok(())
}

fn bounds(
    lower: Option<&[u8]>,
    upper: Option<&[u8]>,
    after: Option<&[u8]>,
) -> (Bound<Vec<u8>>, Bound<Vec<u8>>) {
    let low = lower.map(encode_length_prefixed);
    let cursor = after.map(encode_length_prefixed);
    let start = match (low, cursor) {
        (Some(low), Some(cursor)) if low > cursor => Bound::Included(low),
        (_, Some(cursor)) => Bound::Excluded(cursor),
        (Some(low), None) => Bound::Included(low),
        (None, None) => Bound::Unbounded,
    };
    (
        start,
        upper
            .map(encode_length_prefixed)
            .map(Bound::Included)
            .unwrap_or(Bound::Unbounded),
    )
}

fn empty_bounds(start: &Bound<Vec<u8>>, end: &Bound<Vec<u8>>) -> bool {
    match (start, end) {
        (Bound::Included(a), Bound::Included(b)) => a > b,
        (Bound::Excluded(a), Bound::Included(b)) => a >= b,
        _ => false,
    }
}

fn page(rows: Vec<(Vec<u8>, Vec<u8>)>, limit: usize) -> StorePage {
    let next = (rows.len() == limit).then(|| rows.last().expect("positive page limit").0.clone());
    StorePage { rows, next }
}

fn scan_lmdb(
    db: LmdbDatabase,
    txn: &RoTxn<'_>,
    lower: Option<&[u8]>,
    upper: Option<&[u8]>,
    after: Option<&[u8]>,
    limit: usize,
) -> Result<StorePage, KvStoreError> {
    check_limit(limit)?;
    let (start, end) = bounds(lower, upper, after);
    if empty_bounds(&start, &end) {
        return Ok(page(Vec::new(), limit));
    }
    let raw = db.remap_types::<heed::types::Bytes, heed::types::Bytes>();
    let range = (
        start.as_ref().map(Vec::as_slice),
        end.as_ref().map(Vec::as_slice),
    );
    let mut rows = Vec::new();
    for row in raw.range(txn, &range)?.take(limit) {
        let (key, value) = row?;
        rows.push((bincode::deserialize(key)?, bincode::deserialize(value)?));
    }
    Ok(page(rows, limit))
}

fn scan_memory(
    state: &MemoryState,
    table: u64,
    changes: Option<&BTreeMap<Vec<u8>, Option<Vec<u8>>>>,
    lower: Option<&[u8]>,
    upper: Option<&[u8]>,
    after: Option<&[u8]>,
    limit: usize,
) -> Result<StorePage, KvStoreError> {
    check_limit(limit)?;
    let (mut start, end) = bounds(lower, upper, after);
    if empty_bounds(&start, &end) {
        return Ok(page(Vec::new(), limit));
    }
    let base = state
        .tables
        .get(&table)
        .ok_or_else(|| invalid("Memory transaction table missing"))?;
    let mut rows = Vec::new();
    let mut inspected = 0;
    let mut last = None;
    while inspected < limit && !empty_bounds(&start, &end) {
        let base_row = base.range((start.clone(), end.clone())).next();
        let changed_row = changes.and_then(|c| c.range((start.clone(), end.clone())).next());
        let selected = match (base_row, changed_row) {
            (Some((key, value)), Some((changed_key, _))) if key < changed_key => {
                Some((key, Some(value)))
            }
            (_, Some((key, value))) => Some((key, value.as_ref())),
            (Some((key, value)), None) => Some((key, Some(value))),
            (None, None) => None,
        };
        let Some((key, value)) = selected else {
            break;
        };
        inspected += 1;
        last = Some(key[8..].to_vec());
        start = Bound::Excluded(key.clone());
        if let Some(value) = value {
            rows.push((key[8..].to_vec(), value.clone()));
        }
    }
    Ok(StorePage {
        rows,
        next: if inspected == limit { last } else { None },
    })
}

impl AtomicStoreGroup {
    pub fn len(&self, index: usize) -> Result<u64, KvStoreError> {
        match &self.backend {
            Backend::Lmdb { env, databases } => {
                let txn = env.read_txn()?;
                Ok(slot(databases, index)?.len(&txn)?)
            }
            Backend::Memory { env, tables } => {
                let table = *slot(tables, index)?;
                let state = env.state.read().unwrap_or_else(|e| e.into_inner());
                Ok(state
                    .tables
                    .get(&table)
                    .ok_or_else(|| invalid("Memory table missing"))?
                    .len() as u64)
            }
        }
    }

    pub fn new(stores: Vec<Arc<dyn KeyValueStore>>) -> Result<Self, KvStoreError> {
        let first = stores
            .first()
            .ok_or_else(|| invalid("Atomic store group cannot be empty"))?;
        if let Some(first) = first.as_any().downcast_ref::<LmdbKeyValueStore>() {
            let mut databases = Vec::with_capacity(stores.len());
            for store in &stores {
                let store = store
                    .as_any()
                    .downcast_ref::<LmdbKeyValueStore>()
                    .ok_or_else(|| invalid("Atomic group contains different backend types"))?;
                if !Arc::ptr_eq(&first.env, &store.env) {
                    return Err(invalid("Atomic group contains different LMDB environments"));
                }
                databases.push(store.db);
            }
            return Ok(Self {
                backend: Backend::Lmdb {
                    env: first.env.clone(),
                    databases,
                },
            });
        }
        if let Some(first) = first.as_any().downcast_ref::<MemoryKeyValueStore>() {
            let mut tables = Vec::with_capacity(stores.len());
            for store in &stores {
                let store = store
                    .as_any()
                    .downcast_ref::<MemoryKeyValueStore>()
                    .ok_or_else(|| invalid("Atomic group contains different backend types"))?;
                if !Arc::ptr_eq(&first.environment, &store.environment) {
                    return Err(invalid(
                        "Atomic group contains different memory environments",
                    ));
                }
                tables.push(store.table);
            }
            return Ok(Self {
                backend: Backend::Memory {
                    env: first.environment.clone(),
                    tables,
                },
            });
        }
        Err(invalid("Store backend does not support atomic groups"))
    }

    pub fn transaction<T>(
        &self,
        f: impl FnOnce(&mut AtomicTransaction<'_>) -> Result<T, KvStoreError>,
    ) -> Result<T, KvStoreError> {
        match &self.backend {
            Backend::Lmdb { env, databases } => {
                let mut transaction = AtomicTransaction {
                    inner: TransactionInner::Lmdb {
                        txn: env.write_txn()?,
                        databases,
                    },
                };
                let result = f(&mut transaction)?;
                let TransactionInner::Lmdb { txn, .. } = transaction.inner else {
                    unreachable!()
                };
                txn.commit()?;
                Ok(result)
            }
            Backend::Memory { env, tables } => {
                let mut state = env.state.write().unwrap_or_else(|e| e.into_inner());
                let (result, changes) = {
                    let mut transaction = AtomicTransaction {
                        inner: TransactionInner::Memory {
                            state: &state,
                            tables,
                            changes: BTreeMap::new(),
                        },
                    };
                    let result = f(&mut transaction)?;
                    let TransactionInner::Memory { changes, .. } = &mut transaction.inner else {
                        unreachable!()
                    };
                    (result, std::mem::take(changes))
                };
                for (table, records) in changes {
                    let rows = state
                        .tables
                        .get_mut(&table)
                        .expect("validated memory table");
                    for (key, value) in records {
                        if let Some(value) = value {
                            rows.insert(key, value);
                        } else {
                            rows.remove(&key);
                        }
                    }
                }
                Ok(result)
            }
        }
    }

    pub fn get(&self, index: usize, key: &[u8]) -> Result<Option<Vec<u8>>, KvStoreError> {
        match &self.backend {
            Backend::Lmdb { env, databases } => {
                let txn = env.read_txn()?;
                Ok(slot(databases, index)?.get(&txn, &key.to_vec())?)
            }
            Backend::Memory { env, tables } => {
                let table = *slot(tables, index)?;
                let state = env.state.read().unwrap_or_else(|e| e.into_inner());
                Ok(state
                    .tables
                    .get(&table)
                    .ok_or_else(|| invalid("Memory table missing"))?
                    .get(&encode_length_prefixed(key))
                    .cloned())
            }
        }
    }

    pub fn scan_page(
        &self,
        index: usize,
        lower: Option<&[u8]>,
        upper: Option<&[u8]>,
        after: Option<&[u8]>,
        limit: usize,
    ) -> Result<StorePage, KvStoreError> {
        match &self.backend {
            Backend::Lmdb { env, databases } => {
                let txn = env.read_txn()?;
                scan_lmdb(*slot(databases, index)?, &txn, lower, upper, after, limit)
            }
            Backend::Memory { env, tables } => {
                let state = env.state.read().unwrap_or_else(|e| e.into_inner());
                scan_memory(
                    &state,
                    *slot(tables, index)?,
                    None,
                    lower,
                    upper,
                    after,
                    limit,
                )
            }
        }
    }
}

impl AtomicTransaction<'_> {
    pub fn len(&mut self, index: usize) -> Result<u64, KvStoreError> {
        match &mut self.inner {
            TransactionInner::Lmdb { txn, databases } => Ok(slot(databases, index)?.len(txn)?),
            TransactionInner::Memory {
                state,
                tables,
                changes,
            } => {
                let table = *slot(tables, index)?;
                let base = state
                    .tables
                    .get(&table)
                    .ok_or_else(|| invalid("Memory table missing"))?;
                let mut count = base.len() as u64;
                if let Some(changes) = changes.get(&table) {
                    for (key, value) in changes {
                        match (base.contains_key(key), value.is_some()) {
                            (false, true) => count += 1,
                            (true, false) => count -= 1,
                            _ => (),
                        }
                    }
                }
                Ok(count)
            }
        }
    }

    pub fn get(&mut self, index: usize, key: &[u8]) -> Result<Option<Vec<u8>>, KvStoreError> {
        match &mut self.inner {
            TransactionInner::Lmdb { txn, databases } => {
                Ok(slot(databases, index)?.get(txn, &key.to_vec())?)
            }
            TransactionInner::Memory {
                state,
                tables,
                changes,
            } => {
                let table = *slot(tables, index)?;
                let key = encode_length_prefixed(key);
                if let Some(value) = changes.get(&table).and_then(|rows| rows.get(&key)) {
                    return Ok(value.clone());
                }
                Ok(state
                    .tables
                    .get(&table)
                    .ok_or_else(|| invalid("Memory table missing"))?
                    .get(&key)
                    .cloned())
            }
        }
    }

    pub fn put(&mut self, index: usize, key: Vec<u8>, value: Vec<u8>) -> Result<(), KvStoreError> {
        match &mut self.inner {
            TransactionInner::Lmdb { txn, databases } => {
                slot(databases, index)?.put(txn, &key, &value)?;
            }
            TransactionInner::Memory {
                tables, changes, ..
            } => {
                changes
                    .entry(*slot(tables, index)?)
                    .or_default()
                    .insert(encode_length_prefixed(&key), Some(value));
            }
        }
        Ok(())
    }

    pub fn delete(&mut self, index: usize, key: &[u8]) -> Result<bool, KvStoreError> {
        match &mut self.inner {
            TransactionInner::Lmdb { txn, databases } => {
                Ok(slot(databases, index)?.delete(txn, &key.to_vec())?)
            }
            TransactionInner::Memory {
                state,
                tables,
                changes,
            } => {
                let table = *slot(tables, index)?;
                let key = encode_length_prefixed(key);
                let exists = match changes.get(&table).and_then(|rows| rows.get(&key)) {
                    Some(value) => value.is_some(),
                    None => state
                        .tables
                        .get(&table)
                        .ok_or_else(|| invalid("Memory table missing"))?
                        .contains_key(&key),
                };
                changes.entry(table).or_default().insert(key, None);
                Ok(exists)
            }
        }
    }

    pub fn scan_page(
        &mut self,
        index: usize,
        lower: Option<&[u8]>,
        upper: Option<&[u8]>,
        after: Option<&[u8]>,
        limit: usize,
    ) -> Result<StorePage, KvStoreError> {
        match &mut self.inner {
            TransactionInner::Lmdb { txn, databases } => {
                scan_lmdb(*slot(databases, index)?, txn, lower, upper, after, limit)
            }
            TransactionInner::Memory {
                state,
                tables,
                changes,
            } => {
                let table = *slot(tables, index)?;
                scan_memory(
                    state,
                    table,
                    changes.get(&table),
                    lower,
                    upper,
                    after,
                    limit,
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        group: AtomicStoreGroup,
        stores: Vec<Arc<dyn KeyValueStore>>,
        _directory: Option<tempfile::TempDir>,
    }

    fn memory() -> Fixture {
        let env = Arc::new(MemoryEnvironment::new());
        let stores: Vec<Arc<dyn KeyValueStore>> = (0..2)
            .map(|_| Arc::new(env.create_store()) as Arc<dyn KeyValueStore>)
            .collect();
        Fixture {
            group: AtomicStoreGroup::new(stores.clone()).unwrap(),
            stores,
            _directory: None,
        }
    }

    fn lmdb() -> Fixture {
        let directory = tempfile::tempdir().unwrap();
        let env = Arc::new(unsafe {
            heed::EnvOpenOptions::new()
                .map_size(10 * 1024 * 1024)
                .max_dbs(4)
                .open(directory.path())
                .unwrap()
        });
        let mut txn = env.write_txn().unwrap();
        let a = env.create_database(&mut txn, Some("a")).unwrap();
        let b = env.create_database(&mut txn, Some("b")).unwrap();
        txn.commit().unwrap();
        let stores: Vec<Arc<dyn KeyValueStore>> = vec![
            Arc::new(LmdbKeyValueStore::new(env.clone(), a)),
            Arc::new(LmdbKeyValueStore::new(env, b)),
        ];
        Fixture {
            group: AtomicStoreGroup::new(stores.clone()).unwrap(),
            stores,
            _directory: Some(directory),
        }
    }

    #[test]
    fn transaction_commits_all_tables_and_reads_its_writes() {
        for fixture in [memory(), lmdb()] {
            fixture
                .group
                .transaction(|tx| {
                    tx.put(0, b"a".to_vec(), b"one".to_vec())?;
                    tx.put(1, b"b".to_vec(), b"two".to_vec())?;
                    assert_eq!(tx.get(0, b"a")?, Some(b"one".to_vec()));
                    assert_eq!(tx.get(1, b"b")?, Some(b"two".to_vec()));
                    Ok(())
                })
                .unwrap();
            assert_eq!(
                fixture.stores[0].get_one(&b"a".to_vec()).unwrap(),
                Some(b"one".to_vec())
            );
            assert_eq!(
                fixture.stores[1].get_one(&b"b".to_vec()).unwrap(),
                Some(b"two".to_vec())
            );
        }
    }

    #[test]
    fn callback_error_rolls_back_puts_and_deletes_in_every_table() {
        for fixture in [memory(), lmdb()] {
            fixture.stores[0]
                .put_one(b"old".to_vec(), b"value".to_vec())
                .unwrap();
            let result: Result<(), KvStoreError> = fixture.group.transaction(|tx| {
                assert!(tx.delete(0, b"old")?);
                tx.put(1, b"new".to_vec(), b"value".to_vec())?;
                assert_eq!(tx.get(0, b"old")?, None);
                Err(KvStoreError::IoError("injected failure".into()))
            });
            assert_eq!(
                result,
                Err(KvStoreError::IoError("injected failure".into()))
            );
            assert_eq!(
                fixture.group.get(0, b"old").unwrap(),
                Some(b"value".to_vec())
            );
            assert_eq!(fixture.group.get(1, b"new").unwrap(), None);
        }
    }

    #[test]
    fn unsupported_slot_aborts_prior_writes() {
        for fixture in [memory(), lmdb()] {
            let result = fixture.group.transaction(|tx| {
                tx.put(0, vec![1], vec![1])?;
                tx.put(2, vec![2], vec![2])
            });
            assert!(matches!(result, Err(KvStoreError::InvalidArgument(_))));
            assert_eq!(fixture.group.get(0, &[1]).unwrap(), None);
        }
    }

    #[test]
    fn pages_have_exclusive_continuation_and_transaction_overlay() {
        for fixture in [memory(), lmdb()] {
            fixture.stores[0]
                .put((0..5u8).map(|i| (vec![i], vec![i])).collect())
                .unwrap();
            let first = fixture
                .group
                .scan_page(0, Some(&[1]), Some(&[4]), None, 2)
                .unwrap();
            assert_eq!(first.rows, vec![(vec![1], vec![1]), (vec![2], vec![2])]);
            let second = fixture
                .group
                .scan_page(0, Some(&[1]), Some(&[4]), first.next.as_deref(), 2)
                .unwrap();
            assert_eq!(second.rows, vec![(vec![3], vec![3]), (vec![4], vec![4])]);
            let final_page = fixture
                .group
                .scan_page(0, Some(&[1]), Some(&[4]), second.next.as_deref(), 2)
                .unwrap();
            assert!(final_page.rows.is_empty());
            assert_eq!(final_page.next, None);
            fixture
                .group
                .transaction(|tx| {
                    tx.delete(0, &[2])?;
                    tx.put(0, vec![3], vec![99])?;
                    tx.put(0, vec![5], vec![5])?;
                    assert_eq!(
                        tx.scan_page(0, Some(&[2]), Some(&[5]), None, 4)?.rows,
                        vec![(vec![3], vec![99]), (vec![4], vec![4]), (vec![5], vec![5])]
                    );
                    Ok(())
                })
                .unwrap();
        }
    }

    #[test]
    fn memory_and_lmdb_match_actual_encoded_key_order() {
        let memory = memory();
        let lmdb = lmdb();
        let keys = [vec![255], vec![0, 0], vec![2], vec![0; 256], vec![1; 255]];
        for fixture in [&memory, &lmdb] {
            fixture.stores[0]
                .put(keys.iter().map(|key| (key.clone(), Vec::new())).collect())
                .unwrap();
        }
        let expected = lmdb.group.scan_page(0, None, None, None, 1024).unwrap();
        assert_eq!(
            memory.group.scan_page(0, None, None, None, 1024).unwrap(),
            expected
        );
        assert_eq!(expected.rows[0].0.len(), 256);
    }

    #[test]
    fn mixed_environments_and_invalid_limits_are_rejected() {
        let a = memory();
        let b = memory();
        assert!(AtomicStoreGroup::new(vec![a.stores[0].clone(), b.stores[0].clone()]).is_err());
        let a = lmdb();
        let b = lmdb();
        assert!(AtomicStoreGroup::new(vec![a.stores[0].clone(), b.stores[0].clone()]).is_err());
        assert!(AtomicStoreGroup::new(Vec::new()).is_err());
        for fixture in [memory(), lmdb()] {
            assert!(fixture.group.scan_page(0, None, None, None, 0).is_err());
            assert!(fixture.group.scan_page(0, None, None, None, 1025).is_err());
            assert!(fixture
                .group
                .scan_page(0, Some(&[4]), Some(&[2]), None, 1)
                .unwrap()
                .rows
                .is_empty());
        }
    }

    #[test]
    fn memory_scan_bounds_inspected_tombstones_and_keeps_cursor() {
        let fixture = memory();
        fixture.stores[0]
            .put((0..10u8).map(|key| (vec![key], vec![key])).collect())
            .unwrap();
        fixture
            .group
            .transaction(|tx| {
                for key in 0..9u8 {
                    tx.delete(0, &[key])?;
                }
                let first = tx.scan_page(0, None, None, None, 2)?;
                assert!(first.rows.is_empty());
                assert_eq!(first.next, Some(vec![1]));
                let second = tx.scan_page(0, None, None, first.next.as_deref(), 2)?;
                assert!(second.rows.is_empty());
                assert_eq!(second.next, Some(vec![3]));
                let last = tx.scan_page(0, None, None, Some(&[7]), 2)?;
                assert_eq!(last.rows, vec![(vec![9], vec![9])]);
                assert_eq!(last.next, Some(vec![9]));
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn table_lengths_count_only_committed_rows() {
        for fixture in [memory(), lmdb()] {
            assert_eq!(fixture.group.len(0).unwrap(), 0);
            fixture
                .group
                .transaction(|tx| {
                    tx.put(0, vec![1], vec![1])?;
                    tx.put(0, vec![2], vec![2])?;
                    tx.put(1, vec![1], vec![1])?;
                    assert_eq!(tx.len(0)?, 2);
                    assert_eq!(tx.len(1)?, 1);
                    tx.put(0, vec![1], vec![2])?;
                    assert_eq!(tx.len(0)?, 2);
                    tx.delete(0, &[2])?;
                    assert_eq!(tx.len(0)?, 1);
                    tx.put(0, vec![2], vec![3])?;
                    assert_eq!(tx.len(0)?, 2);
                    Ok(())
                })
                .unwrap();
            assert_eq!(fixture.group.len(0).unwrap(), 2);
            assert_eq!(fixture.group.len(1).unwrap(), 1);
            let aborted: Result<(), KvStoreError> = fixture.group.transaction(|tx| {
                tx.delete(0, &[1])?;
                assert_eq!(tx.len(0)?, 1);
                tx.delete(0, &[99])?;
                assert_eq!(tx.len(0)?, 1);
                Err(KvStoreError::IoError("injected abort".into()))
            });
            assert!(aborted.is_err());
            assert_eq!(fixture.group.len(0).unwrap(), 2);
            fixture.group.transaction(|tx| tx.delete(0, &[1])).unwrap();
            assert_eq!(fixture.group.len(0).unwrap(), 1);
            assert!(fixture.group.len(2).is_err());
        }
    }
}
