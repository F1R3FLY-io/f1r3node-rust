use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use models::rust::block_hash::{BlockHash, BlockHashSerde};
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use serde::{Deserialize, Serialize};
use shared::rust::store::key_value_store::{KeyValueStore, KvStoreError};
use shared::rust::store::key_value_typed_store::KeyValueTypedStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct HeightEntry {
    pub hash: BlockHashSerde,
    pub invalid: bool,
}

#[derive(Clone)]
pub struct HeightIndex {
    rows: KeyValueTypedStoreImpl<i64, BTreeSet<HeightEntry>>,
    meta: KeyValueTypedStoreImpl<String, i64>,
}

const META_MAX_HEIGHT: &str = "max-height";
const META_LFB_HEIGHT: &str = "lfb-height";
const META_BACKFILLED: &str = "backfilled";
const BACKFILL_WRITE_CHUNK: usize = 1024;

impl HeightIndex {
    pub fn new(rows: Arc<dyn KeyValueStore>, meta: Arc<dyn KeyValueStore>) -> Self {
        Self {
            rows: KeyValueTypedStoreImpl::new(rows),
            meta: KeyValueTypedStoreImpl::new(meta),
        }
    }

    pub fn in_memory() -> Self {
        Self::new(
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
        )
    }

    pub fn is_backfilled(&self) -> Result<bool, KvStoreError> {
        Ok(self.meta.get_one(&META_BACKFILLED.to_string())?.is_some())
    }

    pub fn mark_backfilled(&self) -> Result<(), KvStoreError> {
        self.meta.put_one(META_BACKFILLED.to_string(), 1)
    }

    pub fn max_height(&self) -> Result<Option<i64>, KvStoreError> {
        self.meta.get_one(&META_MAX_HEIGHT.to_string())
    }

    pub fn lfb_height(&self) -> Result<Option<i64>, KvStoreError> {
        self.meta.get_one(&META_LFB_HEIGHT.to_string())
    }

    pub fn set_lfb_height(&self, height: i64) -> Result<(), KvStoreError> {
        match self.lfb_height()? {
            Some(current) if current >= height => Ok(()),
            _ => self.meta.put_one(META_LFB_HEIGHT.to_string(), height),
        }
    }

    fn raise_max_height(&self, height: i64) -> Result<(), KvStoreError> {
        match self.max_height()? {
            Some(current) if current >= height => Ok(()),
            _ => self.meta.put_one(META_MAX_HEIGHT.to_string(), height),
        }
    }

    pub fn record(&self, height: i64, hash: BlockHash, invalid: bool) -> Result<(), KvStoreError> {
        let mut entries = self.rows.get_one(&height)?.unwrap_or_default();
        entries.insert(HeightEntry {
            hash: BlockHashSerde(hash),
            invalid,
        });
        self.rows.put_one(height, entries)?;
        self.raise_max_height(height)
    }

    pub fn backfill(
        &self,
        entries: impl IntoIterator<Item = (i64, BlockHash, bool)>,
    ) -> Result<(), KvStoreError> {
        let mut by_height: BTreeMap<i64, BTreeSet<HeightEntry>> = BTreeMap::new();
        for (height, hash, invalid) in entries {
            by_height.entry(height).or_default().insert(HeightEntry {
                hash: BlockHashSerde(hash),
                invalid,
            });
        }
        let max = by_height.keys().next_back().copied();
        let mut rows: Vec<(i64, BTreeSet<HeightEntry>)> = Vec::with_capacity(BACKFILL_WRITE_CHUNK);
        for (height, mut set) in by_height {
            if let Some(existing) = self.rows.get_one(&height)? {
                set.extend(existing);
            }
            rows.push((height, set));
            if rows.len() >= BACKFILL_WRITE_CHUNK {
                self.rows.put(std::mem::take(&mut rows))?;
            }
        }
        if !rows.is_empty() {
            self.rows.put(rows)?;
        }
        if let Some(max) = max {
            self.raise_max_height(max)?;
        }
        self.mark_backfilled()
    }

    pub fn entries_at(&self, height: i64) -> Result<BTreeSet<HeightEntry>, KvStoreError> {
        Ok(self.rows.get_one(&height)?.unwrap_or_default())
    }

    pub fn valid_hashes_at(&self, height: i64) -> Result<Vec<BlockHash>, KvStoreError> {
        Ok(self
            .entries_at(height)?
            .into_iter()
            .filter(|entry| !entry.invalid)
            .map(|entry| entry.hash.0)
            .collect())
    }

    pub fn all_hashes_in(&self, start: i64, end_inclusive: i64) -> Result<Vec<BlockHash>, KvStoreError> {
        let mut result = Vec::new();
        for height in start..=end_inclusive {
            result.extend(self.entries_at(height)?.into_iter().map(|entry| entry.hash.0));
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use prost::bytes::Bytes;

    use super::*;

    fn hash(i: u8) -> BlockHash { Bytes::from(vec![i; 32]) }

    #[test]
    fn record_groups_hashes_by_height_and_tracks_the_maximum() {
        let index = HeightIndex::in_memory();
        index.record(3, hash(1), false).unwrap();
        index.record(3, hash(2), true).unwrap();
        index.record(5, hash(3), false).unwrap();

        assert_eq!(index.max_height().unwrap(), Some(5));
        assert_eq!(index.valid_hashes_at(3).unwrap(), vec![hash(1)]);
        assert_eq!(index.entries_at(3).unwrap().len(), 2);
        assert!(index.entries_at(4).unwrap().is_empty());
        assert_eq!(index.all_hashes_in(3, 5).unwrap().len(), 3);
    }

    #[test]
    fn backfill_merges_with_existing_rows_and_marks_completion() {
        let index = HeightIndex::in_memory();
        assert!(!index.is_backfilled().unwrap());
        index.record(1, hash(9), false).unwrap();
        index
            .backfill(vec![(0, hash(1), false), (1, hash(2), false), (7, hash(3), true)])
            .unwrap();

        assert!(index.is_backfilled().unwrap());
        assert_eq!(index.max_height().unwrap(), Some(7));
        assert_eq!(index.entries_at(1).unwrap().len(), 2);
        assert!(index.valid_hashes_at(7).unwrap().is_empty());
    }

    #[test]
    fn lfb_height_never_regresses() {
        let index = HeightIndex::in_memory();
        index.set_lfb_height(10).unwrap();
        index.set_lfb_height(4).unwrap();
        assert_eq!(index.lfb_height().unwrap(), Some(10));
    }
}
