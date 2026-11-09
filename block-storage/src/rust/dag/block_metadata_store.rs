// See block-storage/src/main/scala/coop/rchain/blockstorage/dag/BlockMetadataStore.scala

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use models::rust::block_hash::{BlockHash, BlockHashSerde};
use models::rust::block_metadata::BlockMetadata;
use models::rust::casper::pretty_printer::PrettyPrinter;
// Slashing-critical DagState lock is held inside the
// `BlockDagKeyValueStorage::get_representation_internal` RMW chain;
// migrating to `parking_lot::RwLock` aligns it with the parent
// crate's parking_lot migration (P1-3, slashing audit).
use parking_lot::RwLock;
use shared::rust::store::key_value_store::KvStoreError;
use shared::rust::store::key_value_typed_store::KeyValueTypedStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;
use shared::rust::store::soak_snapshot::SnapshotError;

use super::height_index::HeightIndex;

pub(crate) struct DagStateCopy {
    pub(crate) dag_set: imbl::HashSet<BlockHash>,
    pub(crate) child_map: imbl::HashMap<BlockHash, imbl::HashSet<BlockHash>>,
    pub(crate) height_map: imbl::OrdMap<i64, imbl::HashSet<BlockHash>>,
    pub(crate) block_number_map: imbl::HashMap<BlockHash, i64>,
    pub(crate) main_parent_map: imbl::HashMap<BlockHash, BlockHash>,
    pub(crate) self_justification_map: imbl::HashMap<BlockHash, BlockHash>,
    pub(crate) last_finalized_block: Option<(BlockHash, i64)>,
    pub(crate) finalized_block_set: imbl::HashSet<BlockHash>,
}

pub struct BlockMetadataStore {
    store: KeyValueTypedStoreImpl<BlockHashSerde, BlockMetadata>,
    dag_state: Arc<RwLock<DagState>>,
    height_index: Option<HeightIndex>,
    retention_depth: Option<i64>,
}

/// In-memory DAG state using persistent immutable collections (imbl).
/// Clone is O(1) via structural sharing, enabling race-free snapshots.
pub(crate) struct DagState {
    pub(crate) dag_set: imbl::HashSet<BlockHash>,
    pub(crate) child_map: imbl::HashMap<BlockHash, imbl::HashSet<BlockHash>>,
    pub(crate) height_map: imbl::OrdMap<i64, imbl::HashSet<BlockHash>>,
    pub(crate) invalid_by_height: imbl::OrdMap<i64, imbl::HashSet<BlockHash>>,
    // Lightweight per-block indices used by propose/finality hot paths to avoid
    // repeated metadata deserialization from LMDB.
    pub(crate) block_number_map: imbl::HashMap<BlockHash, i64>,
    pub(crate) main_parent_map: imbl::HashMap<BlockHash, BlockHash>,
    pub(crate) self_justification_map: imbl::HashMap<BlockHash, BlockHash>,
    // In general - at least genesis should be LFB.
    // But dagstate can be empty, as it is initialized before genesis is inserted.
    // Also lots of tests do not have genesis properly initialised, so fixing all this is pain.
    // So this is Option.
    pub(crate) last_finalized_block: Option<(BlockHash, i64)>,
    pub(crate) finalized_block_set: imbl::HashSet<BlockHash>,
    pub(crate) window_floor: i64,
}

// Keep the in-memory finalized set bounded; finalized truth is persisted in block metadata.
const FINALIZED_BLOCK_CACHE_MAX: usize = 50_000;
const FINALIZED_BLOCK_CACHE_RETAIN: usize = 25_000;

pub const UNBOUNDED_WINDOW_FLOOR: i64 = i64::MIN;

impl DagState {
    fn new() -> Self {
        Self {
            dag_set: imbl::HashSet::new(),
            child_map: imbl::HashMap::new(),
            height_map: imbl::OrdMap::new(),
            invalid_by_height: imbl::OrdMap::new(),
            block_number_map: imbl::HashMap::new(),
            main_parent_map: imbl::HashMap::new(),
            self_justification_map: imbl::HashMap::new(),
            last_finalized_block: Some((BlockHash::new(), 0)),
            finalized_block_set: imbl::HashSet::new(),
            window_floor: UNBOUNDED_WINDOW_FLOOR,
        }
    }
}

struct BlockInfo {
    hash: BlockHash,
    parents: Vec<BlockHash>,
    main_parent: Option<BlockHash>,
    self_justification: Option<BlockHash>,
    block_num: i64,
    is_invalid: bool,
    is_directly_finalized: bool,
    is_finalized: bool,
}

pub struct EvictionReport {
    pub window_floor: i64,
    pub evicted_blocks: usize,
}

impl BlockMetadataStore {
    fn prune_finalized_cache_if_needed(state: &mut DagState) {
        let len = state.finalized_block_set.len();
        if len <= FINALIZED_BLOCK_CACHE_MAX {
            return;
        }

        let to_remove = len.saturating_sub(FINALIZED_BLOCK_CACHE_RETAIN);
        let evict: Vec<BlockHash> = state
            .finalized_block_set
            .iter()
            .take(to_remove)
            .cloned()
            .collect();
        for hash in evict {
            state.finalized_block_set.remove(&hash);
        }
    }

    pub fn try_new(
        block_metadata_store: KeyValueTypedStoreImpl<BlockHashSerde, BlockMetadata>,
    ) -> Result<Self, KvStoreError> {
        Self::build(block_metadata_store, None, None)
    }

    pub fn try_new_windowed(
        block_metadata_store: KeyValueTypedStoreImpl<BlockHashSerde, BlockMetadata>,
        height_index: HeightIndex,
        retention_depth: Option<i64>,
    ) -> Result<Self, KvStoreError> {
        Self::build(block_metadata_store, Some(height_index), retention_depth)
    }

    fn build(
        block_metadata_store: KeyValueTypedStoreImpl<BlockHashSerde, BlockMetadata>,
        height_index: Option<HeightIndex>,
        retention_depth: Option<i64>,
    ) -> Result<Self, KvStoreError> {
        let retention_depth = retention_depth.map(|depth| depth.max(1));
        let windowed_floor = match (&height_index, retention_depth) {
            (Some(index), Some(depth)) if index.is_backfilled()? => index
                .lfb_height()?
                .map(|lfb| (lfb - depth).max(0)),
            _ => None,
        };

        let dag_state = match windowed_floor {
            Some(floor) => {
                let index = height_index.as_ref().expect("windowed floor requires an index");
                Self::load_window(&block_metadata_store, index, floor)?
            }
            None => {
                let blocks_info = Self::collect_all(&block_metadata_store)?;
                if let Some(index) = &height_index {
                    if !index.is_backfilled()? {
                        let lfb_height = blocks_info
                            .values()
                            .filter(|info| info.is_directly_finalized)
                            .map(|info| info.block_num)
                            .max();
                        index.backfill(
                            blocks_info
                                .values()
                                .map(|info| (info.block_num, info.hash.clone(), info.is_invalid)),
                        )?;
                        if let Some(lfb) = lfb_height {
                            index.set_lfb_height(lfb)?;
                        }
                    }
                }
                Self::recreate_in_memory_state(blocks_info)
            }
        };

        Ok(Self {
            store: block_metadata_store,
            dag_state,
            height_index,
            retention_depth,
        })
    }

    fn collect_all(
        store: &KeyValueTypedStoreImpl<BlockHashSerde, BlockMetadata>,
    ) -> Result<HashMap<BlockHash, BlockInfo>, KvStoreError> {
        let blocks_info = store.collect(|(hash, metadata)| {
            Some((
                hash.0.clone(),
                Self::block_metadata_to_info(&hash.0, metadata),
            ))
        })?;
        Ok(blocks_info.into_iter().collect())
    }

    fn load_window(
        store: &KeyValueTypedStoreImpl<BlockHashSerde, BlockMetadata>,
        index: &HeightIndex,
        floor: i64,
    ) -> Result<Arc<RwLock<DagState>>, KvStoreError> {
        let max_height = index.max_height()?.unwrap_or(-1);
        let mut blocks_info: HashMap<BlockHash, BlockInfo> = HashMap::new();
        for height in floor..=max_height {
            let keys: Vec<BlockHashSerde> = index
                .entries_at(height)?
                .into_iter()
                .map(|entry| entry.hash)
                .collect();
            if keys.is_empty() {
                continue;
            }
            for (key, metadata) in keys.iter().zip(store.get(&keys)?) {
                if let Some(metadata) = metadata {
                    blocks_info.insert(
                        key.0.clone(),
                        Self::block_metadata_to_info(&key.0, &metadata),
                    );
                }
            }
        }
        let state = Self::recreate_in_memory_state(blocks_info);
        state.write().window_floor = floor;
        Ok(state)
    }

    pub(crate) fn capture_typed_store(
        &self,
    ) -> &KeyValueTypedStoreImpl<BlockHashSerde, BlockMetadata> {
        &self.store
    }

    pub(crate) fn capture_state(
        &self,
        deadline: Instant,
        wait: Duration,
    ) -> Result<DagStateCopy, SnapshotError> {
        let guard = self
            .dag_state
            .try_read_until(deadline)
            .ok_or(SnapshotError::LockTimeout(wait))?;
        Ok(DagStateCopy {
            dag_set: guard.dag_set.clone(),
            child_map: guard.child_map.clone(),
            height_map: guard.height_map.clone(),
            block_number_map: guard.block_number_map.clone(),
            main_parent_map: guard.main_parent_map.clone(),
            self_justification_map: guard.self_justification_map.clone(),
            last_finalized_block: guard.last_finalized_block.clone(),
            finalized_block_set: guard.finalized_block_set.clone(),
        })
    }

    pub fn new(
        block_metadata_store: KeyValueTypedStoreImpl<BlockHashSerde, BlockMetadata>,
    ) -> Self {
        match Self::try_new(block_metadata_store.clone()) {
            Ok(store) => store,
            Err(e) => {
                tracing::warn!(
                    "Warning: Failed to collect block metadata: {}. Continuing with empty store.",
                    e
                );
                Self {
                    store: block_metadata_store,
                    dag_state: Self::recreate_in_memory_state(HashMap::new()),
                    height_index: None,
                    retention_depth: None,
                }
            }
        }
    }

    fn block_metadata_to_info(hash: &BlockHash, block_metadata: &BlockMetadata) -> BlockInfo {
        let main_parent = block_metadata.parents.first().cloned();
        let self_justification = block_metadata
            .justifications
            .iter()
            .find(|justification| justification.validator == block_metadata.sender)
            .map(|justification| justification.latest_block_hash.clone());

        BlockInfo {
            hash: hash.clone(),
            parents: block_metadata.parents.clone(),
            main_parent,
            self_justification,
            block_num: block_metadata.block_number,
            is_invalid: block_metadata.invalid,
            is_directly_finalized: block_metadata.directly_finalized,
            is_finalized: block_metadata.finalized,
        }
    }

    pub fn add(&mut self, block_metadata: BlockMetadata) -> Result<(), KvStoreError> {
        let block_hash = block_metadata.block_hash.clone();
        let block_info = Self::block_metadata_to_info(&block_hash, &block_metadata);

        if let Some(index) = &self.height_index {
            index.record(block_info.block_num, block_hash.clone(), block_info.is_invalid)?;
        }

        self.dag_state = Self::validate_dag_state(Self::add_block_to_dag_state(
            self.dag_state.clone(),
            block_info,
        ));

        // Update persistent block metadata store
        self.store
            .put_one(BlockHashSerde(block_hash), block_metadata)?;

        Ok(())
    }

    /** Record new last finalized lock. Directly finalized is the output of finalizer,
     * indirectly finalized are new LFB ancestors. */
    pub fn record_finalized(
        &mut self,
        directly: BlockHash,
        indirectly: HashSet<BlockHash>,
        ft_value: f32,
    ) -> Result<(), KvStoreError> {
        let indirectly_serde: Vec<BlockHashSerde> = indirectly
            .iter()
            .map(|hash| BlockHashSerde(hash.clone()))
            .collect();

        let cur_metas_for_if = self.store.get_batch(&indirectly_serde)?;

        // new values to persist
        let mut new_meta_for_df = self.store.get_unsafe(&BlockHashSerde(directly.clone()))?;
        new_meta_for_df.finalized = true;
        new_meta_for_df.directly_finalized = true;
        if ft_value > new_meta_for_df.fault_tolerance_value {
            new_meta_for_df.fault_tolerance_value = ft_value;
        }

        let new_metas_for_if: Vec<(BlockHashSerde, BlockMetadata)> = cur_metas_for_if
            .into_iter()
            .map(|mut v| {
                v.finalized = true;
                if v.fault_tolerance_value < ft_value {
                    v.fault_tolerance_value = ft_value;
                }
                (BlockHashSerde(v.block_hash.clone()), v)
            })
            .collect();

        // Add all blocks to finalized set
        let mut dag_state_guard = self.dag_state.write();
        for hash in indirectly {
            dag_state_guard.finalized_block_set.insert(hash);
        }
        dag_state_guard.finalized_block_set.insert(directly.clone());

        // update lastFinalizedBlock only when current one is lower
        if dag_state_guard.last_finalized_block.is_none()
            || dag_state_guard.last_finalized_block.as_ref().unwrap().1
                <= new_meta_for_df.block_number
        {
            dag_state_guard.last_finalized_block =
                Some((directly.clone(), new_meta_for_df.block_number));
        }
        Self::prune_finalized_cache_if_needed(&mut dag_state_guard);
        let lfb_height = dag_state_guard
            .last_finalized_block
            .as_ref()
            .map(|(_, height)| *height);
        drop(dag_state_guard);

        if let (Some(index), Some(height)) = (&self.height_index, lfb_height) {
            index.set_lfb_height(height)?;
        }

        // persist new values all at once
        let mut new_values = Vec::with_capacity(1 + new_metas_for_if.len());
        new_values.push((BlockHashSerde(directly), new_meta_for_df));
        new_values.extend(new_metas_for_if);
        self.store.put(new_values)?;

        Ok(())
    }

    pub fn retention_depth(&self) -> Option<i64> { self.retention_depth }

    pub fn set_retention_depth(&mut self, depth: Option<i64>) {
        self.retention_depth = depth.map(|d| d.max(1));
    }

    pub fn height_index(&self) -> Option<&HeightIndex> { self.height_index.as_ref() }

    pub fn window_floor(&self) -> i64 { self.dag_state.read().window_floor }

    pub fn evict_below_window(&mut self) -> Option<EvictionReport> {
        let depth = self.retention_depth?;
        let lfb_height = self.dag_state.read().last_finalized_block.as_ref()?.1;
        let floor = (lfb_height - depth).max(0);
        Some(self.evict_below(floor))
    }

    pub fn evict_below(&mut self, floor: i64) -> EvictionReport {
        let mut state = self.dag_state.write();
        if floor <= state.window_floor {
            return EvictionReport {
                window_floor: state.window_floor,
                evicted_blocks: 0,
            };
        }

        let mut evicted: Vec<BlockHash> = Vec::new();
        let mut heights: Vec<i64> = Vec::new();
        for (height, hashes) in state.height_map.range(..floor) {
            heights.push(*height);
            evicted.extend(hashes.iter().cloned());
        }
        let mut invalid_heights: Vec<i64> = Vec::new();
        for (height, hashes) in state.invalid_by_height.range(..floor) {
            invalid_heights.push(*height);
            evicted.extend(hashes.iter().cloned());
        }

        for hash in &evicted {
            state.dag_set.remove(hash);
            state.child_map.remove(hash);
            state.block_number_map.remove(hash);
            state.main_parent_map.remove(hash);
            state.self_justification_map.remove(hash);
            state.finalized_block_set.remove(hash);
        }
        for height in heights {
            state.height_map.remove(&height);
        }
        for height in invalid_heights {
            state.invalid_by_height.remove(&height);
        }
        state.window_floor = floor;

        EvictionReport {
            window_floor: floor,
            evicted_blocks: evicted.len(),
        }
    }

    pub fn update_ft_if_higher(
        &mut self,
        block_hashes: HashSet<BlockHash>,
        ft_value: f32,
    ) -> Result<(), KvStoreError> {
        let serde_keys: Vec<BlockHashSerde> = block_hashes
            .iter()
            .map(|h| BlockHashSerde(h.clone()))
            .collect();
        let metas = self.store.get_batch(&serde_keys)?;

        let updates: Vec<(BlockHashSerde, BlockMetadata)> = metas
            .into_iter()
            .filter(|m| m.fault_tolerance_value < ft_value)
            .map(|mut m| {
                m.fault_tolerance_value = ft_value;
                (BlockHashSerde(m.block_hash.clone()), m)
            })
            .collect();

        if !updates.is_empty() {
            self.store.put(updates)?;
        }
        Ok(())
    }

    pub fn finalized_block_hashes(&self) -> HashSet<BlockHash> {
        self.dag_state
            .read()
            .finalized_block_set
            .iter()
            .cloned()
            .collect()
    }

    pub fn get(&self, hash: &BlockHash) -> Result<Option<BlockMetadata>, KvStoreError> {
        self.store.get_one(&BlockHashSerde(hash.clone()))
    }

    pub fn get_below_window(&self, hash: &BlockHash) -> Result<Option<BlockMetadata>, KvStoreError> {
        let floor = self.dag_state.read().window_floor;
        if floor == UNBOUNDED_WINDOW_FLOOR {
            return Ok(None);
        }
        Ok(self
            .get(hash)?
            .filter(|metadata| metadata.block_number < floor))
    }

    /// Test-only corruption helper: deletes the persisted row while the
    /// in-memory `dag_state` still lists the hash (simulates a DAG set /
    /// metadata inconsistency for fail-closed tests).
    #[doc(hidden)]
    pub fn delete_kv_row_for_tests(&self, hash: &BlockHash) -> Result<(), KvStoreError> {
        self.store.delete(vec![BlockHashSerde(hash.clone())])
    }

    pub fn get_unsafe(&self, hash: &BlockHash) -> Result<BlockMetadata, KvStoreError> {
        self.get(hash)?.ok_or_else(|| {
            KvStoreError::KeyNotFound(format!(
                "BlockMetadataStore is missing key {}",
                PrettyPrinter::build_string_bytes(&hash.to_vec())
            ))
        })
    }

    // DAG state operations — all return O(1) clones via imbl structural sharing

    pub(crate) fn dag_state(&self) -> &Arc<RwLock<DagState>> { &self.dag_state }

    pub fn dag_set(&self) -> imbl::HashSet<BlockHash> { self.dag_state.read().dag_set.clone() }

    pub fn contains(&self, hash: &BlockHash) -> bool {
        if self.dag_state.read().dag_set.contains(hash) {
            return true;
        }
        self.get_below_window(hash).ok().flatten().is_some()
    }

    pub fn child_map(&self) -> imbl::HashMap<BlockHash, imbl::HashSet<BlockHash>> {
        self.dag_state.read().child_map.clone()
    }

    pub fn height_map(&self) -> imbl::OrdMap<i64, imbl::HashSet<BlockHash>> {
        self.dag_state.read().height_map.clone()
    }

    pub fn block_number_map(&self) -> imbl::HashMap<BlockHash, i64> {
        self.dag_state.read().block_number_map.clone()
    }

    pub fn main_parent_map(&self) -> imbl::HashMap<BlockHash, BlockHash> {
        self.dag_state.read().main_parent_map.clone()
    }

    pub fn self_justification_map(&self) -> imbl::HashMap<BlockHash, BlockHash> {
        self.dag_state.read().self_justification_map.clone()
    }

    pub fn last_finalized_block(&self) -> BlockHash {
        self.dag_state
            .read()
            .last_finalized_block
            .as_ref()
            .expect("DagState does not contain lastFinalizedBlock. Are you calling this on empty BlockDagStorage? Otherwise there is a bug.")
            .0
            .clone()
    }

    pub fn finalized_block_set(&self) -> imbl::HashSet<BlockHash> {
        self.dag_state.read().finalized_block_set.clone()
    }

    fn add_block_to_dag_state(
        state: Arc<RwLock<DagState>>,
        block_info: BlockInfo,
    ) -> Arc<RwLock<DagState>> {
        let hash = &block_info.hash;
        let mut state_guard = state.write();

        // Update dag set / all blocks in the DAG
        state_guard.dag_set.insert(hash.clone());

        // Update children relation map
        // Create entry for current block (with empty children set initially)
        if !state_guard.child_map.contains_key(hash) {
            state_guard
                .child_map
                .insert(hash.clone(), imbl::HashSet::new());
        }

        // Add current block as child to all its parents
        for parent in block_info.parents.iter() {
            let mut children = state_guard
                .child_map
                .get(parent)
                .cloned()
                .unwrap_or_else(imbl::HashSet::new);
            children.insert(hash.clone());
            state_guard.child_map.insert(parent.clone(), children);
        }

        // Update height map
        if !block_info.is_invalid {
            let mut hashes = state_guard
                .height_map
                .get(&block_info.block_num)
                .cloned()
                .unwrap_or_else(imbl::HashSet::new);
            hashes.insert(hash.clone());
            state_guard.height_map.insert(block_info.block_num, hashes);
        } else {
            let mut hashes = state_guard
                .invalid_by_height
                .get(&block_info.block_num)
                .cloned()
                .unwrap_or_else(imbl::HashSet::new);
            hashes.insert(hash.clone());
            state_guard
                .invalid_by_height
                .insert(block_info.block_num, hashes);
        }

        state_guard
            .block_number_map
            .insert(hash.clone(), block_info.block_num);

        if let Some(main_parent) = block_info.main_parent {
            state_guard
                .main_parent_map
                .insert(hash.clone(), main_parent);
        }

        if let Some(self_justification) = block_info.self_justification {
            state_guard
                .self_justification_map
                .insert(hash.clone(), self_justification);
        }

        if block_info.is_directly_finalized
            && state_guard
                .last_finalized_block
                .as_ref()
                .is_none_or(|&(_, height)| height <= block_info.block_num)
        {
            state_guard.last_finalized_block = Some((hash.clone(), block_info.block_num));
        }

        if block_info.is_finalized {
            state_guard.finalized_block_set.insert(block_info.hash);
        }

        state.clone()
    }

    fn validate_dag_state(dag_state: Arc<RwLock<DagState>>) -> Arc<RwLock<DagState>> {
        let dag_state_guard = dag_state.read();
        let floor = dag_state_guard.window_floor;
        let in_window: Vec<i64> = dag_state_guard
            .height_map
            .range(floor..)
            .map(|(height, _)| *height)
            .collect();
        // Validate height map index (block numbers) are in sequence without holes
        let (min, max) = match (in_window.first(), in_window.last()) {
            (Some(min), Some(max)) => (*min, *max + 1),
            _ => (0, 0),
        };
        // A non-contiguous height map is a sanity-check failure, not a reason to
        // crash the node mid-validation. Log it for investigation and continue;
        // this is a diagnostic invariant, not a correctness gate.
        if max - min != in_window.len() as i64 {
            tracing::warn!(
                target: "f1r3fly.block_storage",
                min,
                max,
                len = in_window.len(),
                keys = ?in_window,
                "DAG store height map has block numbers not in sequence",
            );
        }
        drop(dag_state_guard);
        dag_state.clone()
    }

    fn recreate_in_memory_state(
        blocks_info_map: HashMap<BlockHash, BlockInfo>,
    ) -> Arc<RwLock<DagState>> {
        let empty_state = Arc::new(RwLock::new(DagState::new()));

        // Add blocks to DAG state
        let dag_state = blocks_info_map
            .into_iter()
            .fold(empty_state, |state, (_, block_info)| {
                Self::add_block_to_dag_state(state, block_info)
            });

        Self::validate_dag_state(dag_state)
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests for `BlockMetadataStore` DAG state maintenance: index and
    //! relation accessors, height-map exclusion of invalid blocks, finalized
    //! ancestry marking with LFB advancement, monotone finality-target
    //! updates, and restart recovery of in-memory state from persisted
    //! metadata. Every test builds its store over an `InMemoryKeyValueStore`,
    //! so no filesystem setup or teardown is required.

    use models::rust::block_implicits::get_random_block;
    use models::rust::casper::protocol::casper_message::{BlockMessage, Justification};
    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;

    use super::*;
    use super::super::height_index::HeightIndex;

    fn block(number: i64, parents: Vec<BlockHash>) -> BlockMessage {
        get_random_block(
            Some(number),
            None,
            None,
            None,
            None,
            None,
            None,
            Some(parents),
            Some(vec![]),
            None,
            None,
            Some(vec![]),
            None,
            None,
        )
    }

    fn meta(block: &BlockMessage) -> BlockMetadata {
        BlockMetadata::from_block(block, false, None, None)
    }

    fn store_over(kv: Arc<InMemoryKeyValueStore>) -> BlockMetadataStore {
        BlockMetadataStore::new(KeyValueTypedStoreImpl::new(kv))
    }

    #[test]
    fn add_indexes_relations_and_accessors_expose_them() {
        let mut store = store_over(Arc::new(InMemoryKeyValueStore::new()));
        let genesis = block(0, vec![]);
        let mut child = block(1, vec![genesis.block_hash.clone()]);
        child.justifications = vec![Justification {
            validator: child.sender.clone(),
            latest_block_hash: genesis.block_hash.clone(),
        }];

        store.add(meta(&genesis)).unwrap();
        store.add(meta(&child)).unwrap();

        assert!(store.contains(&genesis.block_hash));
        assert!(store.contains(&child.block_hash));
        assert!(!store.contains(&BlockHash::from(vec![0u8; 32])));
        assert_eq!(store.dag_set().len(), 2);

        let children = store.child_map().get(&genesis.block_hash).cloned().unwrap();
        assert!(children.contains(&child.block_hash));
        assert!(store.child_map().get(&child.block_hash).unwrap().is_empty());

        assert!(store
            .height_map()
            .get(&0)
            .unwrap()
            .contains(&genesis.block_hash));
        assert!(store
            .height_map()
            .get(&1)
            .unwrap()
            .contains(&child.block_hash));
        assert_eq!(store.block_number_map().get(&child.block_hash), Some(&1));
        assert_eq!(
            store.main_parent_map().get(&child.block_hash),
            Some(&genesis.block_hash)
        );
        assert_eq!(
            store.self_justification_map().get(&child.block_hash),
            Some(&genesis.block_hash)
        );

        assert_eq!(store.get(&child.block_hash).unwrap(), Some(meta(&child)));
        assert_eq!(store.get_unsafe(&child.block_hash).unwrap(), meta(&child));
        assert!(matches!(
            store.get_unsafe(&BlockHash::from(vec![0u8; 32])),
            Err(KvStoreError::KeyNotFound(_))
        ));
    }

    #[test]
    fn invalid_blocks_stay_out_of_the_height_map() {
        let mut store = store_over(Arc::new(InMemoryKeyValueStore::new()));
        let invalid = block(5, vec![]);
        store
            .add(BlockMetadata::from_block(&invalid, true, None, None))
            .unwrap();

        assert!(store.contains(&invalid.block_hash));
        assert!(store.height_map().get(&5).is_none());
    }

    #[test]
    fn record_finalized_marks_blocks_and_advances_the_lfb() {
        let mut store = store_over(Arc::new(InMemoryKeyValueStore::new()));
        let genesis = block(0, vec![]);
        let b1 = block(1, vec![genesis.block_hash.clone()]);
        let b2 = block(2, vec![b1.block_hash.clone()]);
        for b in [&genesis, &b1, &b2] {
            store.add(meta(b)).unwrap();
        }

        store
            .record_finalized(
                b2.block_hash.clone(),
                HashSet::from([genesis.block_hash.clone(), b1.block_hash.clone()]),
                0.5,
            )
            .unwrap();

        assert_eq!(store.last_finalized_block(), b2.block_hash);
        let finalized = store.finalized_block_hashes();
        for b in [&genesis, &b1, &b2] {
            assert!(finalized.contains(&b.block_hash));
            assert!(store.finalized_block_set().contains(&b.block_hash));
        }

        let b2_meta = store.get_unsafe(&b2.block_hash).unwrap();
        assert!(b2_meta.finalized);
        assert!(b2_meta.directly_finalized);
        assert_eq!(b2_meta.fault_tolerance_value, 0.5);

        let b1_meta = store.get_unsafe(&b1.block_hash).unwrap();
        assert!(b1_meta.finalized);
        assert!(!b1_meta.directly_finalized);
        assert_eq!(b1_meta.fault_tolerance_value, 0.5);

        store
            .record_finalized(genesis.block_hash.clone(), HashSet::new(), 0.7)
            .unwrap();
        assert_eq!(
            store.last_finalized_block(),
            b2.block_hash,
            "a lower-height finalization must not regress the LFB"
        );
        assert_eq!(
            store
                .get_unsafe(&genesis.block_hash)
                .unwrap()
                .fault_tolerance_value,
            0.7
        );
    }

    #[test]
    fn update_ft_if_higher_only_raises() {
        let mut store = store_over(Arc::new(InMemoryKeyValueStore::new()));
        let genesis = block(0, vec![]);
        store.add(meta(&genesis)).unwrap();
        store
            .record_finalized(genesis.block_hash.clone(), HashSet::new(), 0.5)
            .unwrap();

        store
            .update_ft_if_higher(HashSet::from([genesis.block_hash.clone()]), 0.9)
            .unwrap();
        assert_eq!(
            store
                .get_unsafe(&genesis.block_hash)
                .unwrap()
                .fault_tolerance_value,
            0.9
        );

        store
            .update_ft_if_higher(HashSet::from([genesis.block_hash.clone()]), 0.1)
            .unwrap();
        assert_eq!(
            store
                .get_unsafe(&genesis.block_hash)
                .unwrap()
                .fault_tolerance_value,
            0.9
        );
    }

    #[test]
    fn restart_recreates_dag_state_from_persisted_metadata() {
        let kv = Arc::new(InMemoryKeyValueStore::new());
        let genesis = block(0, vec![]);
        let b1 = block(1, vec![genesis.block_hash.clone()]);
        {
            let mut store = store_over(kv.clone());
            store.add(meta(&genesis)).unwrap();
            store.add(meta(&b1)).unwrap();
            store
                .record_finalized(
                    b1.block_hash.clone(),
                    HashSet::from([genesis.block_hash.clone()]),
                    0.8,
                )
                .unwrap();
        }

        let restored = store_over(kv);
        assert!(restored.contains(&genesis.block_hash));
        assert!(restored.contains(&b1.block_hash));
        assert_eq!(restored.last_finalized_block(), b1.block_hash);
        assert!(restored.finalized_block_set().contains(&genesis.block_hash));
        assert!(restored.finalized_block_set().contains(&b1.block_hash));
        assert!(restored
            .child_map()
            .get(&genesis.block_hash)
            .unwrap()
            .contains(&b1.block_hash));
        assert!(restored
            .height_map()
            .get(&1)
            .unwrap()
            .contains(&b1.block_hash));
    }

    fn windowed_store_over(kv: Arc<InMemoryKeyValueStore>, index: HeightIndex, depth: i64) -> BlockMetadataStore {
        BlockMetadataStore::try_new_windowed(KeyValueTypedStoreImpl::new(kv), index, Some(depth)).unwrap()
    }

    fn chain(len: usize) -> Vec<BlockMessage> {
        let mut blocks = vec![block(0, vec![])];
        for i in 1..len {
            let parent = blocks[i - 1].block_hash.clone();
            blocks.push(block(i as i64, vec![parent]));
        }
        blocks
    }

    #[test]
    fn eviction_drops_every_index_below_the_floor_and_keeps_the_rest() {
        let mut store = windowed_store_over(Arc::new(InMemoryKeyValueStore::new()), HeightIndex::in_memory(), 2);
        let blocks = chain(10);
        for b in &blocks {
            store.add(meta(b)).unwrap();
        }
        let invalid = BlockMetadata::from_block(&block(3, vec![blocks[2].block_hash.clone()]), true, None, None);
        let invalid_hash = invalid.block_hash.clone();
        store.add(invalid).unwrap();
        store
            .record_finalized(blocks[8].block_hash.clone(), blocks[..8].iter().map(|b| b.block_hash.clone()).collect(), 0.9)
            .unwrap();

        let report = store.evict_below_window().unwrap();
        assert_eq!(report.window_floor, 6);
        assert_eq!(report.evicted_blocks, 7);
        assert_eq!(store.window_floor(), 6);

        for b in &blocks[..6] {
            assert!(!store.dag_set().contains(&b.block_hash));
            assert!(store.child_map().get(&b.block_hash).is_none());
            assert!(store.block_number_map().get(&b.block_hash).is_none());
            assert!(store.main_parent_map().get(&b.block_hash).is_none());
            assert!(!store.finalized_block_set().contains(&b.block_hash));
            assert!(store.contains(&b.block_hash), "evicted blocks stay held through the metadata store");
            assert!(store.get_below_window(&b.block_hash).unwrap().is_some());
        }
        assert!(!store.dag_set().contains(&invalid_hash));
        assert!(store.contains(&invalid_hash));
        for b in &blocks[6..] {
            assert!(store.dag_set().contains(&b.block_hash));
            assert!(store.get_below_window(&b.block_hash).unwrap().is_none());
        }
        assert_eq!(store.height_map().keys().copied().collect::<Vec<_>>(), vec![6, 7, 8, 9]);
        assert_eq!(store.last_finalized_block(), blocks[8].block_hash);

        let again = store.evict_below_window().unwrap();
        assert_eq!(again.evicted_blocks, 0);
    }

    #[test]
    fn eviction_is_disabled_without_a_retention_depth() {
        let mut store = store_over(Arc::new(InMemoryKeyValueStore::new()));
        for b in &chain(5) {
            store.add(meta(b)).unwrap();
        }
        assert!(store.evict_below_window().is_none());
        assert_eq!(store.window_floor(), UNBOUNDED_WINDOW_FLOOR);
        assert!(store.get_below_window(&block(0, vec![]).block_hash).unwrap().is_none());
    }

    #[test]
    fn windowed_restart_loads_only_the_window_and_backfills_the_height_index() {
        let kv = Arc::new(InMemoryKeyValueStore::new());
        let blocks = chain(12);
        {
            let mut store = store_over(kv.clone());
            for b in &blocks {
                store.add(meta(b)).unwrap();
            }
            store
                .record_finalized(blocks[10].block_hash.clone(), blocks[..10].iter().map(|b| b.block_hash.clone()).collect(), 0.9)
                .unwrap();
        }

        let index = HeightIndex::in_memory();
        {
            let store = windowed_store_over(kv.clone(), index.clone(), 3);
            assert_eq!(store.window_floor(), UNBOUNDED_WINDOW_FLOOR, "first windowed start backfills and loads everything");
            assert_eq!(store.dag_set().len(), 12);
            assert!(index.is_backfilled().unwrap());
            assert_eq!(index.max_height().unwrap(), Some(11));
            assert_eq!(index.lfb_height().unwrap(), Some(10));
        }

        let restored = windowed_store_over(kv, index, 3);
        assert_eq!(restored.window_floor(), 7);
        assert_eq!(restored.dag_set().len(), 5);
        assert_eq!(restored.last_finalized_block(), blocks[10].block_hash);
        assert!(!restored.dag_set().contains(&blocks[0].block_hash));
        assert!(restored.contains(&blocks[0].block_hash));
        assert_eq!(restored.height_map().keys().copied().collect::<Vec<_>>(), vec![7, 8, 9, 10, 11]);
    }
}
