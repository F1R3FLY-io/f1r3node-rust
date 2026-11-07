use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use super::key_value_store::{KeyValueStore, KvStoreError};
use super::soak_snapshot::encode_length_prefixed;
use crate::rust::ByteBuffer;

type Records = BTreeMap<Vec<u8>, Vec<u8>>;

#[derive(Default)]
pub(crate) struct MemoryState {
    pub(crate) tables: BTreeMap<u64, Records>,
    next_table: u64,
}

#[derive(Default)]
pub struct MemoryEnvironment {
    pub(crate) state: RwLock<MemoryState>,
}

impl MemoryEnvironment {
    pub fn new() -> Self { Self::default() }

    pub fn create_store(self: &Arc<Self>) -> MemoryKeyValueStore {
        let mut state = self.state.write().unwrap_or_else(|e| e.into_inner());
        let table = state.next_table;
        state.next_table = table
            .checked_add(1)
            .expect("memory table identifiers exhausted");
        state.tables.insert(table, BTreeMap::new());
        MemoryKeyValueStore {
            environment: self.clone(),
            table,
        }
    }
}

#[derive(Clone)]
pub struct MemoryKeyValueStore {
    pub(crate) environment: Arc<MemoryEnvironment>,
    pub(crate) table: u64,
}

impl Default for MemoryKeyValueStore {
    fn default() -> Self { Self::new() }
}

impl MemoryKeyValueStore {
    pub fn new() -> Self { Arc::new(MemoryEnvironment::new()).create_store() }

    fn with_rows_mut<T>(&self, f: impl FnOnce(&mut Records) -> T) -> T {
        let mut state = self
            .environment
            .state
            .write()
            .unwrap_or_else(|e| e.into_inner());
        f(state
            .tables
            .get_mut(&self.table)
            .expect("memory table missing"))
    }

    fn with_rows<T>(&self, f: impl FnOnce(&Records) -> T) -> T {
        let state = self
            .environment
            .state
            .read()
            .unwrap_or_else(|e| e.into_inner());
        f(state.tables.get(&self.table).expect("memory table missing"))
    }

    pub fn clear(&self) { self.with_rows_mut(Records::clear); }

    pub fn num_records(&self) -> usize { self.with_rows(Records::len) }
}

impl KeyValueStore for MemoryKeyValueStore {
    fn as_any(&self) -> &dyn std::any::Any { self }

    fn get(&self, keys: &Vec<ByteBuffer>) -> Result<Vec<Option<ByteBuffer>>, KvStoreError> {
        Ok(self.with_rows(|rows| {
            keys.iter()
                .map(|key| rows.get(&encode_length_prefixed(key)).cloned())
                .collect()
        }))
    }

    fn put(&self, pairs: Vec<(ByteBuffer, ByteBuffer)>) -> Result<(), KvStoreError> {
        self.with_rows_mut(|rows| {
            for (key, value) in pairs {
                rows.insert(encode_length_prefixed(&key), value);
            }
        });
        Ok(())
    }

    fn put_one_if_absent(&self, key: ByteBuffer, value: ByteBuffer) -> Result<bool, KvStoreError> {
        self.with_rows_mut(|rows| match rows.entry(encode_length_prefixed(&key)) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(value);
                Ok(true)
            }
            std::collections::btree_map::Entry::Occupied(_) => Ok(false),
        })
    }

    fn delete(&self, keys: Vec<ByteBuffer>) -> Result<usize, KvStoreError> {
        Ok(self.with_rows_mut(|rows| {
            keys.into_iter()
                .filter(|key| rows.remove(&encode_length_prefixed(key)).is_some())
                .count()
        }))
    }

    fn delete_if_value(
        &self,
        key: &ByteBuffer,
        expected: &ByteBuffer,
    ) -> Result<bool, KvStoreError> {
        self.with_rows_mut(|rows| {
            let key = encode_length_prefixed(key);
            if rows.get(&key) != Some(expected) {
                return Ok(false);
            }
            rows.remove(&key);
            Ok(true)
        })
    }

    fn replace_if_values(
        &self,
        replacements: Vec<(ByteBuffer, ByteBuffer, ByteBuffer)>,
    ) -> Result<usize, KvStoreError> {
        self.with_rows_mut(|rows| {
            let mut count = 0;
            for (key, expected, replacement) in replacements {
                let key = encode_length_prefixed(&key);
                if rows.get(&key) == Some(&expected) {
                    rows.insert(key, replacement);
                    count += 1;
                }
            }
            Ok(count)
        })
    }

    fn iterate(&self, f: fn(ByteBuffer, ByteBuffer)) -> Result<(), KvStoreError> {
        self.iterate_while(&mut |key, value| {
            f(key, value);
            Ok(true)
        })
    }

    fn iterate_while(
        &self,
        f: &mut dyn FnMut(ByteBuffer, ByteBuffer) -> Result<bool, KvStoreError>,
    ) -> Result<(), KvStoreError> {
        let mut after = std::ops::Bound::Unbounded;
        loop {
            let next = self.with_rows(|rows| {
                rows.range((after.clone(), std::ops::Bound::Unbounded))
                    .next()
                    .map(|(key, value)| (key.clone(), value.clone()))
            });
            let Some((key, value)) = next else {
                break;
            };
            after = std::ops::Bound::Excluded(key.clone());
            if !f(key[8..].to_vec(), value)? {
                break;
            }
        }
        Ok(())
    }

    fn clone_box(&self) -> Box<dyn KeyValueStore> { Box::new(self.clone()) }

    fn to_map(&self) -> Result<BTreeMap<ByteBuffer, ByteBuffer>, KvStoreError> {
        let mut result = BTreeMap::new();
        self.iterate_while(&mut |key, value| {
            result.insert(key, value);
            Ok(true)
        })?;
        Ok(result)
    }

    fn print_store(&self) -> Result<(), KvStoreError> {
        for (key, value) in self.to_map()? {
            println!(
                "Key: {:?}, Value: {:?}",
                hex::encode(key),
                hex::encode(value)
            );
        }
        Ok(())
    }

    fn non_empty(&self) -> Result<bool, KvStoreError> { Ok(self.num_records() != 0) }

    fn size_bytes(&self) -> usize {
        self.with_rows(|rows| rows.iter().map(|(k, v)| k.len() - 8 + v.len()).sum())
    }
}
