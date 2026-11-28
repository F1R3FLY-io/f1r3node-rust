use std::collections::BTreeMap;
use std::ops::Bound;
use std::sync::{Arc, RwLock};

use super::key_value_store::{check_scan_limit, KeyValueStore, KvStoreError, StorePage};
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
    fn entry_count(&self) -> Result<Option<usize>, KvStoreError> {
        Ok(Some(self.with_rows(|rows| rows.len())))
    }
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

    fn contains(&self, keys: &Vec<ByteBuffer>) -> Result<Vec<bool>, KvStoreError> {
        Ok(self.with_rows(|rows| {
            keys.iter()
                .map(|key| rows.contains_key(&encode_length_prefixed(key)))
                .collect()
        }))
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

    /// Cursor-paginated scan matching [`KeyValueStore::scan_page`] semantics.
    /// Rows are stored length-prefixed internally (8-byte big-endian length
    /// via `encode_length_prefixed`), but the cursor and returned keys are
    /// raw bytes. The order BTreeMap visits them in is the same as the order
    /// `LmdbKeyValueStore::scan_page` returns, by construction: both encode
    /// the length prefix before the raw bytes, so same-length keys sort by
    /// raw bytes and different-length keys sort by length first.
    fn scan_page(&self, after: Option<&[u8]>, limit: usize) -> Result<StorePage, KvStoreError> {
        check_scan_limit(limit)?;
        let encoded_after = after.map(encode_length_prefixed);
        let start: Bound<Vec<u8>> = match encoded_after {
            Some(bytes) => Bound::Excluded(bytes),
            None => Bound::Unbounded,
        };
        let rows: Vec<(ByteBuffer, ByteBuffer)> = self.with_rows(|rows| {
            rows.range((start, Bound::Unbounded))
                .take(limit)
                .map(|(key, value)| (key[8..].to_vec(), value.clone()))
                .collect()
        });
        let next =
            (rows.len() == limit).then(|| rows.last().expect("positive page limit").0.clone());
        Ok(StorePage { rows, next })
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

#[cfg(test)]
mod scan_tests {
    use super::*;

    fn seeded(keys: &[&[u8]]) -> MemoryKeyValueStore {
        let store = MemoryKeyValueStore::new();
        let pairs: Vec<(ByteBuffer, ByteBuffer)> =
            keys.iter().map(|k| (k.to_vec(), k.to_vec())).collect();
        store.put(pairs).unwrap();
        store
    }

    #[test]
    fn empty_store_returns_empty_page_without_next() {
        let page = MemoryKeyValueStore::new().scan_page(None, 16).unwrap();
        assert!(page.rows.is_empty());
        assert!(page.next.is_none());
    }

    #[test]
    fn walks_same_length_keys_in_byte_order() {
        let keys: Vec<Vec<u8>> = (0u8..10).map(|n| vec![n; 32]).collect();
        let refs: Vec<&[u8]> = keys.iter().map(Vec::as_slice).collect();
        let store = seeded(&refs);

        let mut seen: Vec<ByteBuffer> = Vec::new();
        let mut cursor: Option<ByteBuffer> = None;
        loop {
            let page = store.scan_page(cursor.as_deref(), 3).unwrap();
            seen.extend(page.rows.iter().map(|(k, _)| k.clone()));
            match page.next {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        assert_eq!(seen, keys);
    }

    #[test]
    fn cursor_is_exclusive() {
        let keys: Vec<Vec<u8>> = (0u8..5).map(|n| vec![n; 32]).collect();
        let refs: Vec<&[u8]> = keys.iter().map(Vec::as_slice).collect();
        let store = seeded(&refs);

        let first = store.scan_page(None, 2).unwrap();
        assert_eq!(first.rows.iter().map(|(k, _)| k).collect::<Vec<_>>(), vec![
            &keys[0], &keys[1]
        ]);
        let second = store.scan_page(first.next.as_deref(), 2).unwrap();
        assert_eq!(
            second.rows.iter().map(|(k, _)| k).collect::<Vec<_>>(),
            vec![&keys[2], &keys[3]]
        );
        let third = store.scan_page(second.next.as_deref(), 2).unwrap();
        assert_eq!(third.rows.iter().map(|(k, _)| k).collect::<Vec<_>>(), vec![
            &keys[4]
        ]);
        assert!(third.next.is_none());
    }

    #[test]
    fn limit_out_of_range_is_rejected() {
        let store = MemoryKeyValueStore::new();
        assert!(matches!(
            store.scan_page(None, 0),
            Err(KvStoreError::InvalidArgument(_))
        ));
        assert!(matches!(
            store.scan_page(None, 1025),
            Err(KvStoreError::InvalidArgument(_))
        ));
    }

    #[test]
    fn cursor_past_the_end_returns_empty() {
        let keys: Vec<Vec<u8>> = (0u8..3).map(|n| vec![n; 32]).collect();
        let refs: Vec<&[u8]> = keys.iter().map(Vec::as_slice).collect();
        let store = seeded(&refs);

        let past_end = vec![0xff; 32];
        let page = store.scan_page(Some(&past_end), 16).unwrap();
        assert!(page.rows.is_empty());
        assert!(page.next.is_none());
    }

    /// Matches the LMDB impl: for same-length keys, visitation order is raw
    /// byte-order. Different-length keys sort by length first (both impls
    /// encode a length prefix; both are therefore length-then-bytes).
    #[test]
    fn lmdb_memory_visitation_order_matches_for_same_length_keys() {
        let keys: Vec<Vec<u8>> = vec![
            vec![0x10; 32],
            vec![0x01; 32],
            vec![0xff; 32],
            vec![0x80; 32],
            vec![0x00; 32],
        ];
        let refs: Vec<&[u8]> = keys.iter().map(Vec::as_slice).collect();
        let store = seeded(&refs);

        let mut seen: Vec<ByteBuffer> = Vec::new();
        let mut cursor: Option<ByteBuffer> = None;
        loop {
            let page = store.scan_page(cursor.as_deref(), 2).unwrap();
            seen.extend(page.rows.iter().map(|(k, _)| k.clone()));
            match page.next {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        let mut expected = keys.clone();
        expected.sort();
        assert_eq!(seen, expected, "memory impl must yield keys in byte order");
    }
}
