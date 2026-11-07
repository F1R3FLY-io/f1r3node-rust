use std::collections::BTreeMap;
use std::ops::Bound;
use std::sync::Arc;

use heed::types::SerdeBincode;
use heed::{Database, Env, RoTxn, RwTxn};

use super::key_value_store::{KeyValueStore, KvStoreError};
use super::lmdb_key_value_store::LmdbKeyValueStore;
use super::memory_key_value_store::{MemoryEnvironment, MemoryKeyValueStore, MemoryState};
use super::soak_snapshot::encode_length_prefixed;
use crate::rust::blocking::in_blocking;

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
            Backend::Lmdb { env, databases } => in_blocking(|| {
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
            }),
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
            Backend::Lmdb { env, databases } => in_blocking(|| {
                let txn = env.read_txn()?;
                scan_lmdb(*slot(databases, index)?, &txn, lower, upper, after, limit)
            }),
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

    #[test]
    fn atomic_failure_rolls_back_tables_and_scan_continuation() {
        for lmdb in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let stores: Vec<Arc<dyn KeyValueStore>> = if lmdb {
                let env = Arc::new(unsafe {
                    heed::EnvOpenOptions::new()
                        .map_size(10 * 1024 * 1024)
                        .max_dbs(2)
                        .open(directory.path())
                        .unwrap()
                });
                let mut txn = env.write_txn().unwrap();
                let a = env.create_database(&mut txn, Some("a")).unwrap();
                let b = env.create_database(&mut txn, Some("b")).unwrap();
                txn.commit().unwrap();
                vec![
                    Arc::new(LmdbKeyValueStore::new(env.clone(), a)),
                    Arc::new(LmdbKeyValueStore::new(env, b)),
                ]
            } else {
                let env = Arc::new(MemoryEnvironment::new());
                vec![Arc::new(env.create_store()), Arc::new(env.create_store())]
            };
            let group = AtomicStoreGroup::new(stores).unwrap();
            group
                .transaction(|tx| {
                    for key in 0..4 {
                        tx.put(0, vec![key], vec![key])?;
                    }
                    Ok(())
                })
                .unwrap();
            let failed: Result<(), KvStoreError> = group.transaction(|tx| {
                assert!(tx.delete(0, &[0])?);
                tx.put(1, vec![9], vec![9])?;
                assert_eq!(tx.get(0, &[0])?, None);
                assert_eq!(tx.get(1, &[9])?, Some(vec![9]));
                assert_eq!(tx.len(0)?, 3);
                assert_eq!(
                    tx.scan_page(0, Some(&[1]), Some(&[3]), None, 2)?.rows,
                    vec![(vec![1], vec![1]), (vec![2], vec![2])]
                );
                Err(invalid("injected transaction failure"))
            });
            assert!(failed.is_err());
            assert_eq!(group.len(0).unwrap(), 4);
            assert_eq!(group.len(1).unwrap(), 0);
            assert_eq!(group.get(0, &[0]).unwrap(), Some(vec![0]));
            assert_eq!(group.get(1, &[9]).unwrap(), None);
            let first = group.scan_page(0, Some(&[0]), Some(&[3]), None, 2).unwrap();
            assert_eq!(first.rows, vec![(vec![0], vec![0]), (vec![1], vec![1])]);
            let second = group
                .scan_page(0, Some(&[0]), Some(&[3]), first.next.as_deref(), 2)
                .unwrap();
            assert_eq!(second.rows, vec![(vec![2], vec![2]), (vec![3], vec![3])]);
            assert!(group
                .scan_page(0, Some(&[0]), Some(&[3]), second.next.as_deref(), 2)
                .unwrap()
                .rows
                .is_empty());
            group
                .transaction(|tx| {
                    tx.delete(0, &[0])?;
                    tx.put(1, vec![9], vec![9])
                })
                .unwrap();
            assert_eq!(group.get(0, &[0]).unwrap(), None);
            assert_eq!(group.get(1, &[9]).unwrap(), Some(vec![9]));
        }
    }
}
