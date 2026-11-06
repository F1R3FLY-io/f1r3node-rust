use std::sync::Arc;
use std::time::Instant;

use crypto::rust::hash::blake2b256::Blake2b256;
use models::rust::block_hash::{BlockHash, LENGTH};
use models::rust::block_metadata::BlockMetadata;
use serde::{Deserialize, Serialize};
use shared::rust::store::atomic_store::{AtomicStoreGroup, AtomicTransaction, StorePage};
use shared::rust::store::key_value_store::KvStoreError;

pub const MEMBERSHIP_TABLE: &str = "dag-membership-v1";
pub const HEIGHT_TABLE: &str = "dag-height-v1";
pub const CHILDREN_TABLE: &str = "dag-children-v1";
pub const STATE_TABLE: &str = "dag-history-state-v1";

#[derive(Clone, Copy, Debug)]
pub struct HistoryTableSlots {
    pub membership: usize,
    pub heights: usize,
    pub children: usize,
    pub state: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MembershipRecord {
    pub sequence: u64,
    pub height: i64,
    pub invalid: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryPage<T> {
    pub rows: Vec<T>,
    pub next: Option<Vec<u8>>,
}

#[derive(Clone)]
pub struct DagHistoryIndex {
    group: Arc<AtomicStoreGroup>,
    slots: HistoryTableSlots,
}

struct HistoryQuery {
    operation: &'static str,
    started: Instant,
}

impl HistoryQuery {
    fn new(operation: &'static str) -> Self {
        metrics::counter!("dag.history.query.count", "operation" => operation).increment(1);
        Self {
            operation,
            started: Instant::now(),
        }
    }

    fn visited(&self, rows: usize) {
        metrics::counter!("dag.history.rows.visited", "operation" => self.operation)
            .increment(rows as u64);
    }
}

impl Drop for HistoryQuery {
    fn drop(&mut self) {
        metrics::histogram!("dag.history.query.time", "operation" => self.operation)
            .record(self.started.elapsed().as_secs_f64());
    }
}

fn invalid(message: impl Into<String>) -> KvStoreError {
    KvStoreError::InvalidArgument(message.into())
}

fn validate_hash(hash: &[u8]) -> Result<(), KvStoreError> {
    if hash.len() != LENGTH {
        return Err(invalid(format!(
            "DAG history hash must contain {LENGTH} bytes"
        )));
    }
    Ok(())
}

pub fn encode_height_key(height: i64, hash: &[u8]) -> Result<Vec<u8>, KvStoreError> {
    validate_hash(hash)?;
    let mut key = Vec::with_capacity(8 + LENGTH);
    key.extend_from_slice(&((height as u64) ^ (1 << 63)).to_be_bytes());
    key.extend_from_slice(hash);
    Ok(key)
}

pub fn decode_height_key(key: &[u8]) -> Result<(i64, BlockHash), KvStoreError> {
    if key.len() != 8 + LENGTH {
        return Err(KvStoreError::SerializationError(
            "Invalid DAG height key length".into(),
        ));
    }
    let mut bytes = [0; 8];
    bytes.copy_from_slice(&key[..8]);
    let height = (u64::from_be_bytes(bytes) ^ (1 << 63)) as i64;
    Ok((height, BlockHash::copy_from_slice(&key[8..])))
}

pub fn hash_prefix_bounds(prefix: &[u8]) -> Result<(Vec<u8>, Vec<u8>), KvStoreError> {
    if prefix.len() > LENGTH {
        return Err(invalid("DAG hash prefix is longer than a hash"));
    }
    let mut lower = vec![0; LENGTH];
    let mut upper = vec![u8::MAX; LENGTH];
    lower[..prefix.len()].copy_from_slice(prefix);
    upper[..prefix.len()].copy_from_slice(prefix);
    Ok((lower, upper))
}

fn child_key(parent: &[u8], child: &[u8]) -> Result<Vec<u8>, KvStoreError> {
    validate_hash(child)?;
    let mut key = Vec::with_capacity(2 * LENGTH + 1);
    if parent.len() == LENGTH {
        key.extend_from_slice(parent);
    } else {
        key.push(1);
        key.extend_from_slice(&Blake2b256::hash(parent.to_vec()));
    }
    key.extend_from_slice(child);
    Ok(key)
}

impl DagHistoryIndex {
    pub fn new(group: Arc<AtomicStoreGroup>, slots: HistoryTableSlots) -> Self {
        Self { group, slots }
    }

    pub fn group(&self) -> &Arc<AtomicStoreGroup> { &self.group }

    pub fn slots(&self) -> HistoryTableSlots { self.slots }

    pub fn membership(
        &self,
        hash: &BlockHash,
        view_sequence: u64,
    ) -> Result<Option<MembershipRecord>, KvStoreError> {
        if hash.len() != LENGTH {
            return Ok(None);
        }
        let query = HistoryQuery::new("membership");
        let row = self.group.get(self.slots.membership, hash)?;
        query.visited(usize::from(row.is_some()));
        row.map(|bytes| {
            bincode::deserialize::<MembershipRecord>(&bytes).map_err(KvStoreError::from)
        })
        .transpose()
        .map(|record| record.filter(|record| record.sequence <= view_sequence))
    }

    pub fn membership_in(
        &self,
        tx: &mut AtomicTransaction<'_>,
        hash: &BlockHash,
    ) -> Result<Option<MembershipRecord>, KvStoreError> {
        if hash.len() != LENGTH {
            return Ok(None);
        }
        let query = HistoryQuery::new("membership_in");
        let row = tx.get(self.slots.membership, hash)?;
        query.visited(usize::from(row.is_some()));
        row.map(|bytes| bincode::deserialize(&bytes).map_err(KvStoreError::from))
            .transpose()
    }

    pub fn index_block(
        &self,
        tx: &mut AtomicTransaction<'_>,
        metadata: &BlockMetadata,
        sequence: u64,
    ) -> Result<(), KvStoreError> {
        validate_hash(&metadata.block_hash)?;
        if let Some(existing) = self.membership_in(tx, &metadata.block_hash)? {
            if existing.height != metadata.block_number || existing.invalid != metadata.invalid {
                return Err(invalid(
                    "DAG membership cannot change block height or invalid status",
                ));
            }
            return Ok(());
        }
        let record = MembershipRecord {
            sequence,
            height: metadata.block_number,
            invalid: metadata.invalid,
        };
        tx.put(
            self.slots.membership,
            metadata.block_hash.to_vec(),
            bincode::serialize(&record)?,
        )?;
        tx.put(
            self.slots.heights,
            encode_height_key(record.height, &metadata.block_hash)?,
            bincode::serialize(&sequence)?,
        )?;
        for parent in &metadata.parents {
            let key = child_key(parent, &metadata.block_hash)?;
            let value = if parent.len() == LENGTH {
                bincode::serialize(&sequence)?
            } else {
                let mut originals = match tx.get(self.slots.children, &key)? {
                    Some(bytes) => {
                        let (existing_sequence, originals): (u64, Vec<Vec<u8>>) =
                            bincode::deserialize(&bytes)?;
                        if existing_sequence != sequence {
                            return Err(KvStoreError::SerializationError(
                                "DAG exceptional child index sequence disagrees".into(),
                            ));
                        }
                        originals
                    }
                    None => Vec::new(),
                };
                if !originals
                    .iter()
                    .any(|original| original.as_slice() == parent.as_ref())
                {
                    originals.push(parent.to_vec());
                }
                bincode::serialize(&(sequence, originals))?
            };
            tx.put(self.slots.children, key, value)?;
        }
        Ok(())
    }

    pub fn scan_height_range(
        &self,
        start: i64,
        end: i64,
        view_sequence: u64,
        valid_only: bool,
        after: Option<&[u8]>,
        limit: usize,
    ) -> Result<HistoryPage<BlockHash>, KvStoreError> {
        let query = HistoryQuery::new("height_range");
        let lower = encode_height_key(start, &[0; LENGTH])?;
        let upper = encode_height_key(end, &[u8::MAX; LENGTH])?;
        let page =
            self.group
                .scan_page(self.slots.heights, Some(&lower), Some(&upper), after, limit)?;
        query.visited(page.rows.len());
        let mut rows = Vec::new();
        for (key, value) in page.rows {
            let sequence: u64 = bincode::deserialize(&value)?;
            if sequence > view_sequence {
                continue;
            }
            let (height, hash) = decode_height_key(&key)?;
            let membership = self.membership(&hash, view_sequence)?.ok_or_else(|| {
                KvStoreError::KeyNotFound("DAG height index has no visible membership".into())
            })?;
            if membership.height != height || membership.sequence != sequence {
                return Err(KvStoreError::SerializationError(
                    "DAG height and membership indexes disagree".into(),
                ));
            }
            if !valid_only || !membership.invalid {
                rows.push(hash);
            }
        }
        Ok(HistoryPage {
            rows,
            next: page.next,
        })
    }

    pub fn scan_children(
        &self,
        parent: &BlockHash,
        view_sequence: u64,
        after: Option<&[u8]>,
        limit: usize,
    ) -> Result<HistoryPage<BlockHash>, KvStoreError> {
        let query = HistoryQuery::new("children");
        let lower = child_key(parent, &[0; LENGTH])?;
        let upper = child_key(parent, &[u8::MAX; LENGTH])?;
        let page = self.group.scan_page(
            self.slots.children,
            Some(&lower),
            Some(&upper),
            after,
            limit,
        )?;
        query.visited(page.rows.len());
        let mut rows = Vec::new();
        for (key, value) in page.rows {
            let sequence: u64 = if parent.len() == LENGTH {
                bincode::deserialize(&value)?
            } else {
                let (sequence, originals): (u64, Vec<Vec<u8>>) = bincode::deserialize(&value)?;
                if !originals
                    .iter()
                    .any(|original| original.as_slice() == parent.as_ref())
                {
                    continue;
                }
                sequence
            };
            if sequence > view_sequence {
                continue;
            }
            let prefix_length = if parent.len() == LENGTH {
                LENGTH
            } else {
                LENGTH + 1
            };
            if key.len() != prefix_length + LENGTH {
                return Err(KvStoreError::SerializationError(
                    "Invalid DAG child key length".into(),
                ));
            }
            let hash = BlockHash::copy_from_slice(&key[prefix_length..]);
            let membership = self.membership(&hash, view_sequence)?.ok_or_else(|| {
                KvStoreError::KeyNotFound("DAG child index has no visible membership".into())
            })?;
            if membership.sequence != sequence {
                return Err(KvStoreError::SerializationError(
                    "DAG child and membership indexes disagree".into(),
                ));
            }
            rows.push(hash);
        }
        Ok(HistoryPage {
            rows,
            next: page.next,
        })
    }

    fn membership_page(
        &self,
        page: StorePage,
        view_sequence: u64,
    ) -> Result<HistoryPage<BlockHash>, KvStoreError> {
        let mut rows = Vec::new();
        for (key, value) in page.rows {
            if key.len() != LENGTH {
                return Err(KvStoreError::SerializationError(
                    "Invalid DAG membership key length".into(),
                ));
            }
            let membership: MembershipRecord = bincode::deserialize(&value)?;
            if membership.sequence <= view_sequence {
                rows.push(BlockHash::from(key));
            }
        }
        Ok(HistoryPage {
            rows,
            next: page.next,
        })
    }

    pub fn scan_membership(
        &self,
        view_sequence: u64,
        after: Option<&[u8]>,
        limit: usize,
    ) -> Result<HistoryPage<BlockHash>, KvStoreError> {
        let query = HistoryQuery::new("membership_page");
        let page = self
            .group
            .scan_page(self.slots.membership, None, None, after, limit)?;
        query.visited(page.rows.len());
        self.membership_page(page, view_sequence)
    }

    pub fn scan_prefix(
        &self,
        prefix: &[u8],
        view_sequence: u64,
        after: Option<&[u8]>,
        limit: usize,
    ) -> Result<HistoryPage<BlockHash>, KvStoreError> {
        let query = HistoryQuery::new("prefix");
        let (lower, upper) = hash_prefix_bounds(prefix)?;
        let page = self.group.scan_page(
            self.slots.membership,
            Some(&lower),
            Some(&upper),
            after,
            limit,
        )?;
        query.visited(page.rows.len());
        self.membership_page(page, view_sequence)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    use rspace_plus_plus::rspace::shared::lmdb_dir_store_manager::{
        Db, LmdbDirStoreManager, LmdbEnvConfig,
    };
    use shared::rust::store::key_value_store::KeyValueStore;
    use shared::rust::store::memory_key_value_store::MemoryEnvironment;

    use super::*;

    fn slots() -> HistoryTableSlots {
        HistoryTableSlots {
            membership: 0,
            heights: 1,
            children: 2,
            state: 3,
        }
    }

    async fn fixtures() -> Vec<(DagHistoryIndex, Option<tempfile::TempDir>)> {
        let env = Arc::new(MemoryEnvironment::new());
        let stores = (0..4)
            .map(|_| Arc::new(env.create_store()) as Arc<dyn KeyValueStore>)
            .collect();
        let memory =
            DagHistoryIndex::new(Arc::new(AtomicStoreGroup::new(stores).unwrap()), slots());
        let directory = tempfile::tempdir().unwrap();
        let names = [MEMBERSHIP_TABLE, HEIGHT_TABLE, CHILDREN_TABLE, STATE_TABLE];
        let mapping = names
            .iter()
            .map(|name| {
                (
                    Db::new((*name).into(), None),
                    LmdbEnvConfig::new("dag".into(), 10 * 1024 * 1024),
                )
            })
            .collect();
        let mut manager = LmdbDirStoreManager::new(directory.path().to_path_buf(), mapping);
        let mut stores = Vec::new();
        for table in [MEMBERSHIP_TABLE, HEIGHT_TABLE, CHILDREN_TABLE, STATE_TABLE] {
            stores.push(manager.store(table.into()).await.unwrap());
        }
        let lmdb = DagHistoryIndex::new(Arc::new(AtomicStoreGroup::new(stores).unwrap()), slots());
        vec![(memory, None), (lmdb, Some(directory))]
    }

    fn hash(value: u8) -> BlockHash { BlockHash::from(vec![value; LENGTH]) }

    fn metadata(value: u8, height: i64, parents: Vec<BlockHash>, invalid: bool) -> BlockMetadata {
        BlockMetadata {
            block_hash: hash(value),
            parents,
            sender: hash(99),
            justifications: Vec::new(),
            weight_map: BTreeMap::new(),
            block_number: height,
            sequence_number: 0,
            invalid,
            directly_finalized: false,
            finalized: false,
            fault_tolerance_value: 0.0,
            merge_base: BlockHash::new(),
        }
    }

    #[tokio::test]
    async fn historical_reads_preserve_sequence_and_invalid_filters() {
        for (history, _directory) in fixtures().await {
            history
                .group
                .transaction(|tx| {
                    history.index_block(tx, &metadata(1, 255, Vec::new(), false), 1)?;
                    history.index_block(tx, &metadata(2, 256, vec![hash(1)], false), 2)?;
                    history.index_block(tx, &metadata(3, 256, vec![hash(1)], true), 3)
                })
                .unwrap();
            assert!(history.membership(&hash(2), 1).unwrap().is_none());
            assert_eq!(
                history
                    .scan_height_range(255, 256, 3, true, None, 1024)
                    .unwrap()
                    .rows,
                vec![hash(1), hash(2)]
            );
            assert_eq!(
                history
                    .scan_height_range(256, 256, 3, false, None, 1024)
                    .unwrap()
                    .rows,
                vec![hash(2), hash(3)]
            );
            assert_eq!(
                history.scan_children(&hash(1), 2, None, 1024).unwrap().rows,
                vec![hash(2)]
            );
            assert_eq!(
                history.scan_children(&hash(1), 3, None, 1024).unwrap().rows,
                vec![hash(2), hash(3)]
            );
            history
                .group
                .transaction(|tx| {
                    history.index_block(tx, &metadata(2, 256, vec![hash(1)], false), 99)
                })
                .unwrap();
            assert_eq!(
                history.membership(&hash(2), 2).unwrap().unwrap().sequence,
                2
            );
        }
    }

    #[tokio::test]
    async fn filtered_empty_page_keeps_continuation() {
        use metrics_util::debugging::{DebugValue, DebuggingRecorder};

        for (history, _directory) in fixtures().await {
            history
                .group
                .transaction(|tx| {
                    history.index_block(tx, &metadata(1, 0, Vec::new(), false), 5)?;
                    history.index_block(tx, &metadata(2, 1, Vec::new(), false), 1)
                })
                .unwrap();
            let recorder = DebuggingRecorder::new();
            let snapshotter = recorder.snapshotter();
            let first = metrics::with_local_recorder(&recorder, || {
                history.scan_membership(1, None, 1).unwrap()
            });
            assert!(first.rows.is_empty());
            assert!(first.next.is_some());
            let visited =
                snapshotter
                    .snapshot()
                    .into_vec()
                    .into_iter()
                    .find_map(|(key, _, _, value)| {
                        if key.key().name() == "dag.history.rows.visited"
                            && key.key().labels().any(|label| {
                                label.key() == "operation" && label.value() == "membership_page"
                            })
                        {
                            if let DebugValue::Counter(value) = value {
                                return Some(value);
                            }
                        }
                        None
                    });
            assert_eq!(visited, Some(1));
            let second = history
                .scan_membership(1, first.next.as_deref(), 1)
                .unwrap();
            assert_eq!(second.rows, vec![hash(2)]);
        }
    }

    #[tokio::test]
    async fn prefix_scan_seeks_within_fixed_hash_bounds() {
        for (history, _directory) in fixtures().await {
            history
                .group
                .transaction(|tx| {
                    history.index_block(tx, &metadata(0xab, 0, Vec::new(), false), 1)?;
                    history.index_block(tx, &metadata(0xac, 0, Vec::new(), false), 2)
                })
                .unwrap();
            assert_eq!(
                history.scan_prefix(&[0xab], 2, None, 1024).unwrap().rows,
                vec![hash(0xab)]
            );
            assert!(history
                .scan_prefix(&[0xaa], 2, None, 1024)
                .unwrap()
                .rows
                .is_empty());
            let (lower, upper) = hash_prefix_bounds(&[0xa0]).unwrap();
            assert!(lower <= hash(0xa0).to_vec() && upper >= hash(0xa0).to_vec());
        }
    }

    #[test]
    fn signed_height_key_order_and_round_trip() {
        let heights = [i64::MIN, -1, 0, 255, 256, i64::MAX];
        let keys: Vec<_> = heights
            .into_iter()
            .map(|height| encode_height_key(height, &hash(1)).unwrap())
            .collect();
        assert!(keys.windows(2).all(|pair| pair[0] < pair[1]));
        for (height, key) in heights.into_iter().zip(keys) {
            assert_eq!(decode_height_key(&key).unwrap(), (height, hash(1)));
        }
    }

    #[tokio::test]
    async fn malformed_parent_lengths_keep_separate_reverse_edges() {
        for (history, _directory) in fixtures().await {
            let parents: Vec<BlockHash> = [0, 1, 32, 223, 224, 512, 1024]
                .into_iter()
                .map(|length| BlockHash::from(vec![7; length]))
                .collect();
            history
                .group
                .transaction(|tx| {
                    for (index, parent) in parents.iter().enumerate() {
                        history.index_block(
                            tx,
                            &metadata(index as u8 + 1, 1, vec![parent.clone()], true),
                            index as u64 + 1,
                        )?;
                    }
                    Ok(())
                })
                .unwrap();
            for (index, parent) in parents.iter().enumerate() {
                assert_eq!(
                    history.scan_children(parent, 7, None, 1024).unwrap().rows,
                    vec![hash(index as u8 + 1)]
                );
            }
        }
    }

    #[tokio::test]
    async fn exceptional_parent_bucket_merges_and_checks_exact_original_bytes() {
        for (history, _directory) in fixtures().await {
            let parent = BlockHash::from(vec![1; 1024]);
            let other_parent = vec![2; 1024];
            let key = child_key(&parent, &hash(1)).unwrap();
            assert_eq!(key.len(), 65);
            assert_eq!(child_key(&hash(0), &hash(1)).unwrap().len(), 64);
            history
                .group
                .transaction(|tx| {
                    tx.put(
                        history.slots.children,
                        key.clone(),
                        bincode::serialize(&(1u64, vec![other_parent.clone()]))?,
                    )?;
                    history.index_block(
                        tx,
                        &metadata(1, 10, vec![parent.clone(), parent.clone()], true),
                        1,
                    )
                })
                .unwrap();
            let bytes = history
                .group
                .get(history.slots.children, &key)
                .unwrap()
                .unwrap();
            let (sequence, originals): (u64, Vec<Vec<u8>>) = bincode::deserialize(&bytes).unwrap();
            assert_eq!(sequence, 1);
            assert_eq!(originals, vec![other_parent.clone(), parent.to_vec()]);
            assert_eq!(
                history.scan_children(&parent, 1, None, 1).unwrap().rows,
                vec![hash(1)]
            );
            history
                .group
                .transaction(|tx| {
                    tx.put(
                        history.slots.children,
                        key.clone(),
                        bincode::serialize(&(1u64, vec![other_parent]))?,
                    )
                })
                .unwrap();
            let filtered = history.scan_children(&parent, 1, None, 1).unwrap();
            assert!(filtered.rows.is_empty());
            assert_eq!(filtered.next, Some(key));
        }
    }

    #[tokio::test]
    async fn failed_indexing_rolls_back_all_relations() {
        for (history, _directory) in fixtures().await {
            let result: Result<(), KvStoreError> = history.group.transaction(|tx| {
                history.index_block(tx, &metadata(1, 10, vec![hash(0)], false), 1)?;
                Err(KvStoreError::IoError("injected publication failure".into()))
            });
            assert!(result.is_err());
            assert!(history.membership(&hash(1), 1).unwrap().is_none());
            assert!(history
                .scan_children(&hash(0), 1, None, 1024)
                .unwrap()
                .rows
                .is_empty());
            assert!(history
                .scan_height_range(0, 10, 1, false, None, 1024)
                .unwrap()
                .rows
                .is_empty());
        }
    }

    #[tokio::test]
    async fn corrupt_membership_propagates_through_every_history_reader() {
        for (history, _directory) in fixtures().await {
            history
                .group
                .transaction(|tx| {
                    history.index_block(tx, &metadata(1, 10, vec![hash(0)], false), 1)?;
                    tx.put(history.slots.membership, hash(1).to_vec(), vec![0xff])
                })
                .unwrap();
            assert!(history.membership(&hash(1), 1).is_err());
            assert!(history.scan_membership(1, None, 1024).is_err());
            assert!(history.scan_prefix(&[1], 1, None, 1024).is_err());
            assert!(history.scan_children(&hash(0), 1, None, 1024).is_err());
            assert!(history
                .scan_height_range(10, 10, 1, false, None, 1024)
                .is_err());
        }
    }
}
