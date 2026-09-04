// See block-storage/src/main/scala/coop/rchain/blockstorage/dag/BlockMetadataStore.scala

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

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

pub struct BlockMetadataStore {
    store: KeyValueTypedStoreImpl<BlockHashSerde, BlockMetadata>,
    dag_state: Arc<RwLock<DagState>>,
}

/// In-memory DAG state using persistent immutable collections (imbl).
/// Clone is O(1) via structural sharing, enabling race-free snapshots.
pub(crate) struct DagState {
    pub(crate) dag_set: imbl::HashSet<BlockHash>,
    pub(crate) child_map: imbl::HashMap<BlockHash, imbl::HashSet<BlockHash>>,
    pub(crate) height_map: imbl::OrdMap<i64, imbl::HashSet<BlockHash>>,
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
}

// Keep the in-memory finalized set bounded; finalized truth is persisted in block metadata.
const FINALIZED_BLOCK_CACHE_MAX: usize = 50_000;
const FINALIZED_BLOCK_CACHE_RETAIN: usize = 25_000;

impl DagState {
    fn new() -> Self {
        Self {
            dag_set: imbl::HashSet::new(),
            child_map: imbl::HashMap::new(),
            height_map: imbl::OrdMap::new(),
            block_number_map: imbl::HashMap::new(),
            main_parent_map: imbl::HashMap::new(),
            self_justification_map: imbl::HashMap::new(),
            last_finalized_block: Some((BlockHash::new(), 0)),
            finalized_block_set: imbl::HashSet::new(),
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

impl BlockMetadataStore {
    pub(crate) fn raw_store(
        &self,
    ) -> &Arc<dyn shared::rust::store::key_value_store::KeyValueStore> {
        self.store.raw_store()
    }

    pub(crate) fn encode_add(
        &self,
        block_metadata: &BlockMetadata,
    ) -> Result<(Vec<u8>, Vec<u8>), KvStoreError> {
        block_metadata
            .validate()
            .map_err(|error| KvStoreError::InvalidArgument(error.to_string()))?;
        Ok((
            self.store
                .encode_key(&BlockHashSerde(block_metadata.block_hash.clone()))?,
            self.store.encode_value(block_metadata)?,
        ))
    }

    pub(crate) fn apply_committed_add(&mut self, block_metadata: BlockMetadata) {
        let block_hash = block_metadata.block_hash.clone();
        let block_info = Self::block_metadata_to_info(&block_hash, &block_metadata);
        self.dag_state = Self::validate_dag_state(Self::add_block_to_dag_state(
            self.dag_state.clone(),
            block_info,
        ));
    }

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

    pub fn new(
        block_metadata_store: KeyValueTypedStoreImpl<BlockHashSerde, BlockMetadata>,
    ) -> Result<Self, KvStoreError> {
        let stored_metadata = block_metadata_store
            .collect(|(hash, metadata)| Some((hash.0.clone(), metadata.clone())))?;
        let mut blocks_info_map = HashMap::with_capacity(stored_metadata.len());
        for (hash, metadata) in stored_metadata {
            metadata
                .validate()
                .map_err(|error| KvStoreError::SerializationError(error.to_string()))?;
            if hash != metadata.block_hash {
                return Err(KvStoreError::SerializationError(
                    "block metadata key does not match its certified block hash".to_string(),
                ));
            }
            blocks_info_map.insert(hash.clone(), Self::block_metadata_to_info(&hash, &metadata));
        }
        let dag_state = Self::recreate_in_memory_state(blocks_info_map);

        Ok(Self {
            store: block_metadata_store,
            dag_state,
        })
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
            is_invalid: block_metadata.is_rejected(),
            is_directly_finalized: block_metadata.directly_finalized,
            is_finalized: block_metadata.finalized,
        }
    }

    pub fn add(&mut self, block_metadata: BlockMetadata) -> Result<(), KvStoreError> {
        let (key, value) = self.encode_add(&block_metadata)?;
        self.store.raw_store().put_one(key, value)?;
        self.apply_committed_add(block_metadata);

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
        new_meta_for_df
            .validate()
            .map_err(|error| KvStoreError::SerializationError(error.to_string()))?;
        if new_meta_for_df.block_hash != directly {
            return Err(KvStoreError::SerializationError(
                "direct-finalization metadata key does not match its certified block hash"
                    .to_string(),
            ));
        }
        if new_meta_for_df.is_rejected() {
            return Err(KvStoreError::InvalidArgument(
                "cannot directly finalize a rejected certified block".to_string(),
            ));
        }
        new_meta_for_df.finalized = true;
        new_meta_for_df.directly_finalized = true;
        if ft_value > new_meta_for_df.fault_tolerance_value {
            new_meta_for_df.fault_tolerance_value = ft_value;
        }

        let new_metas_for_if: Vec<(BlockHashSerde, BlockMetadata)> = indirectly_serde
            .into_iter()
            .zip(cur_metas_for_if)
            .map(|(key, mut v)| {
                v.validate()
                    .map_err(|error| KvStoreError::SerializationError(error.to_string()))?;
                if v.block_hash != key.0 {
                    return Err(KvStoreError::SerializationError(
                        "indirect-finalization metadata key does not match its certified block hash"
                            .to_string(),
                    ));
                }
                if v.is_rejected() {
                    return Err(KvStoreError::InvalidArgument(
                        "cannot indirectly finalize a rejected certified block".to_string(),
                    ));
                }
                v.finalized = true;
                if v.fault_tolerance_value < ft_value {
                    v.fault_tolerance_value = ft_value;
                }
                v.validate()
                    .map_err(|error| KvStoreError::SerializationError(error.to_string()))?;
                Ok((key, v))
            })
            .collect::<Result<Vec<_>, _>>()?;

        new_meta_for_df
            .validate()
            .map_err(|error| KvStoreError::SerializationError(error.to_string()))?;

        let directly_finalized_number = new_meta_for_df.block_number;
        let mut new_values = Vec::with_capacity(1 + new_metas_for_if.len());
        new_values.push((BlockHashSerde(directly.clone()), new_meta_for_df));
        new_values.extend(new_metas_for_if);
        self.store.put(new_values)?;

        let mut dag_state_guard = self.dag_state.write();
        for hash in indirectly {
            dag_state_guard.finalized_block_set.insert(hash);
        }
        dag_state_guard.finalized_block_set.insert(directly.clone());
        if dag_state_guard.last_finalized_block.is_none()
            || dag_state_guard.last_finalized_block.as_ref().unwrap().1 <= directly_finalized_number
        {
            dag_state_guard.last_finalized_block = Some((directly, directly_finalized_number));
        }
        Self::prune_finalized_cache_if_needed(&mut dag_state_guard);

        Ok(())
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
        let metadata = self.store.get_one(&BlockHashSerde(hash.clone()))?;
        metadata
            .map(|metadata| {
                metadata
                    .validate()
                    .map_err(|error| KvStoreError::SerializationError(error.to_string()))?;
                if metadata.block_hash != *hash {
                    return Err(KvStoreError::SerializationError(
                        "block metadata key does not match its certified block hash".to_string(),
                    ));
                }
                Ok(metadata)
            })
            .transpose()
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
        self.dag_state.read().dag_set.contains(hash)
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
        let height_map = &dag_state_guard.height_map;
        // Validate height map index (block numbers) are in sequence without holes
        let (min, max) = if !height_map.is_empty() {
            (
                height_map.get_min().unwrap().0,
                height_map.get_max().unwrap().0 + 1,
            )
        } else {
            (0, 0)
        };
        // A non-contiguous height map is a sanity-check failure, not a reason to
        // crash the node mid-validation. Log it for investigation and continue;
        // this is a diagnostic invariant, not a correctness gate.
        if max - min != height_map.len() as i64 {
            tracing::warn!(
                target: "f1r3fly.block_storage",
                min,
                max,
                len = height_map.len(),
                keys = ?height_map.keys().cloned().collect::<Vec<i64>>(),
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
    use models::rust::block_metadata::{
        AdmissionRejectionReason, CertifiedAdmissionOutcome, CertifiedSenderAuthority,
        CERTIFIED_ADMISSION_PROTOCOL_VERSION,
    };
    use models::rust::bond_generation::BondGeneration;
    use models::rust::casper::protocol::casper_message::{
        BlockMessage, FinalizedFloorCommitment, Justification,
    };
    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;

    use super::*;

    fn block(number: i64, parents: Vec<BlockHash>) -> BlockMessage {
        let mut block = get_random_block(
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
        );
        block.header.version = CERTIFIED_ADMISSION_PROTOCOL_VERSION;
        block.header.sender_bond_generation = Some(BondGeneration::GENESIS);
        block.header.finalized_floor = Some(FinalizedFloorCommitment {
            floor_hash: block
                .header
                .parents_hash_list
                .first()
                .cloned()
                .unwrap_or_else(|| block.block_hash.clone()),
            floor_post_state_hash: block.body.state.pre_state_hash.clone(),
            certificate_digest: BlockHash::from(vec![1; 32]),
            authority_context_digest: BlockHash::from(vec![2; 32]),
        });
        block
    }

    fn meta(block: &BlockMessage) -> BlockMetadata { certified_meta(block, false) }

    fn certified_meta(block: &BlockMessage, rejected: bool) -> BlockMetadata {
        let commitment = block.header.finalized_floor.as_ref().unwrap();
        let authority = CertifiedSenderAuthority::new(
            block,
            commitment.floor_hash.clone(),
            commitment.floor_post_state_hash.clone(),
            commitment.authority_context_digest.clone(),
            BondGeneration::GENESIS,
            1,
        )
        .unwrap();
        let outcome = if rejected {
            CertifiedAdmissionOutcome::rejected(
                block,
                &authority,
                AdmissionRejectionReason::InvalidTransaction,
            )
        } else {
            CertifiedAdmissionOutcome::accepted(block, &authority)
        }
        .unwrap();
        BlockMetadata::from_certified_block(block, None, None, &authority, &outcome).unwrap()
    }

    fn store_over(kv: Arc<InMemoryKeyValueStore>) -> BlockMetadataStore {
        BlockMetadataStore::new(KeyValueTypedStoreImpl::new(kv)).unwrap()
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
        store.add(certified_meta(&invalid, true)).unwrap();

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
}
