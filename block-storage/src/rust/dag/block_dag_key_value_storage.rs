//! Persistent block-DAG storage.
//!
//! [`BlockDagKeyValueStorage`] owns the on-disk LMDB-backed indices and
//! the in-memory `BlockMetadataStore` that together represent the DAG
//! of casper blocks. [`KeyValueDagRepresentation`] is the read-only
//! snapshot type that the validation, fork-choice, and finalization
//! paths consume.
//!
//! ## Slashing-protocol position
//!
//! The store is the **canonical home** of the equivocation tracker
//! (`equivocation_tracker_index`). All RMW on the tracker MUST route
//! through [`BlockDagKeyValueStorage::access_equivocations_tracker`] to
//! preserve Bug #2 / T-9.2 atomicity — see
//! [`crate::rust::dag::equivocations_access::EquivocationsAccess`] for
//! the trait contract and
//! `formal/rocq/slashing/theories/BugFixAtomicTracker.v` for the
//! mechanized proof.
//!
//! ## Lock discipline (P1-3 + P2-12)
//!
//! * `global_lock: Arc<parking_lot::RwLock<()>>` coordinates pure-read
//!   snapshot acquisition (via `.read()`) against mutators (`.write()`).
//! * `block_metadata_index`, `deploy_index` are themselves
//!   `parking_lot::RwLock`-wrapped for fine-grained concurrency.
//!
//! See `docs/theory/slashing/slashing-verification.md` for the
//! protocol-level theorems whose witnesses are recorded here.

// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// See block-storage/src/main/scala/coop/rchain/blockstorage/dag/BlockDagKeyValueStorage.scala

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use models::rust::block_hash::{self, BlockHash, BlockHashSerde};
use models::rust::block_metadata::BlockMetadata;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::BlockMessage;
#[cfg(any(test, feature = "test-internals"))]
use models::rust::equivocation_record::EquivocationRecord;
use models::rust::equivocation_record::SequenceNumber;
use models::rust::validator::{self, Validator, ValidatorSerde};
// Slashing-critical RMW locks are routed through `parking_lot` (P1-3): no
// poison propagation, faster acquire, and `.lock()` / `.read()` / `.write()`
// return guards directly without a `Result`. Bug #2 / T-9.2's
// `access_equivocations_tracker` RMW contract is preserved by holding
// `global_lock` for the duration of the critical section.
use parking_lot::RwLock as PlRwLock;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use shared::rust::store::key_value_store::KvStoreError;
use shared::rust::store::key_value_typed_store::KeyValueTypedStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

use super::block_metadata_store::BlockMetadataStore;
use super::equivocation_tracker_store::EquivocationTrackerStore;

pub type DeployId = shared::rust::ByteString;

/// P4-2: replaces the prior `(invalid: bool, approved: bool)` pair on
/// [`BlockDagKeyValueStorage::insert`]. The two booleans are not
/// independent — an approved block is by definition not invalid — and
/// the enum encodes that invariant at the type level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertMode {
    /// Standard insertion: block is valid and not the approved (genesis)
    /// block. Used for all non-genesis valid blocks accepted into the
    /// DAG.
    Normal,
    /// Block has failed validation and is being recorded for evidence
    /// but is not eligible for fork-choice. Used by
    /// `dispatch_handle_invalid_block`.
    Invalid,
    /// Genesis / approved-block insertion. Marks the block as the
    /// initial finalization root.
    Approved,
}

// Phase 8 (A-6): `InsertMode::flags()` projection deleted; `insert_internal`
// now dispatches on `mode` via `matches!` directly.

#[derive(Clone)]
pub struct KeyValueDagRepresentation {
    pub dag_set: imbl::HashSet<BlockHash>,
    pub latest_messages_map: imbl::HashMap<Validator, BlockHash>,
    pub child_map: imbl::HashMap<BlockHash, imbl::HashSet<BlockHash>>,
    pub height_map: imbl::OrdMap<i64, imbl::HashSet<BlockHash>>,
    pub block_number_map: imbl::HashMap<BlockHash, i64>,
    pub main_parent_map: imbl::HashMap<BlockHash, BlockHash>,
    pub self_justification_map: imbl::HashMap<BlockHash, BlockHash>,
    pub invalid_blocks_set: imbl::HashSet<BlockMetadata>,
    pub last_finalized_block_hash: BlockHash,
    pub finalized_blocks_set: imbl::HashSet<BlockHash>,
    // P2-14: the metadata + deploy indices are kept `pub` for cross-crate
    // test fixtures that build a `KeyValueDagRepresentation` from raw
    // components. Production code on the same crate (block-storage)
    // accesses them through the inherent methods on this type; treat
    // direct manipulation as a test-only escape hatch.
    #[doc(hidden)]
    pub block_metadata_index: Arc<PlRwLock<BlockMetadataStore>>,
    #[doc(hidden)]
    pub deploy_index: Arc<PlRwLock<KeyValueTypedStoreImpl<DeployId, BlockHashSerde>>>,
}

impl KeyValueDagRepresentation {
    pub fn lookup(&self, block_hash: &BlockHash) -> Result<Option<BlockMetadata>, KvStoreError> {
        if self.dag_set.contains(block_hash) {
            let block_metadata_index_guard = self.block_metadata_index.read();
            block_metadata_index_guard.get(block_hash)
        } else {
            Ok(None)
        }
    }

    pub fn contains(&self, block_hash: &BlockHash) -> bool {
        block_hash.len() == block_hash::LENGTH && self.dag_set.contains(block_hash)
    }

    pub fn children(&self, block_hash: &BlockHash) -> Option<imbl::HashSet<BlockHash>> {
        self.child_map.get(block_hash).cloned()
    }

    pub fn latest_message_hash(&self, validator: &Validator) -> Option<BlockHash> {
        self.latest_messages_map.get(validator).cloned()
    }

    pub fn latest_message_hashes(&self) -> imbl::HashMap<Validator, BlockHash> {
        self.latest_messages_map.clone()
    }

    pub fn invalid_blocks(&self) -> imbl::HashSet<BlockMetadata> { self.invalid_blocks_set.clone() }

    pub fn last_finalized_block(&self) -> BlockHash { self.last_finalized_block_hash.clone() }

    // latestBlockNumber, topoSort and lookupByDeployId are only used in BlockAPI.
    // Do they need to be part of the DAG current state or they can be moved to DAG storage directly?

    pub fn get_max_height(&self) -> i64 {
        if self.height_map.is_empty() {
            0
        } else {
            self.height_map.get_max().expect("height_map is empty").0 + 1
        }
    }

    pub fn latest_block_number(&self) -> i64 { self.get_max_height() }

    pub fn block_number(&self, block_hash: &BlockHash) -> Option<i64> {
        self.block_number_map.get(block_hash).copied()
    }

    pub fn block_number_unsafe(&self, block_hash: &BlockHash) -> Result<i64, KvStoreError> {
        self.block_number(block_hash).ok_or_else(|| {
            KvStoreError::InvalidArgument(format!(
                "DAG storage is missing hash {}",
                PrettyPrinter::build_string_bytes(block_hash)
            ))
        })
    }

    pub fn main_parent(&self, block_hash: &BlockHash) -> Option<BlockHash> {
        self.main_parent_map.get(block_hash).cloned()
    }

    pub fn is_finalized(&self, block_hash: &BlockHash) -> bool {
        if self.finalized_blocks_set.contains(block_hash) {
            return true;
        }

        // Finalized status is persisted in block metadata; in-memory set is a bounded cache.
        self.block_metadata_index
            .read()
            .get(block_hash)
            .ok()
            .flatten()
            .map(|m| m.finalized)
            .unwrap_or(false)
    }

    pub fn find(&self, truncated_hash: &str) -> Result<Option<BlockHash>, KvStoreError> {
        let (decode_target, do_full_string_filter) = if truncated_hash.len().is_multiple_of(2) {
            (truncated_hash, false)
        } else {
            // if truncatedHash is odd length string we cannot convert it to ByteString with 8 bit resolution
            // because each symbol has 4 bit resolution. Need to make a string of even length by removing the last symbol,
            // then find all the matching hashes and choose one that matches the full truncatedHash string
            (&truncated_hash[..truncated_hash.len() - 1], true)
        };
        let truncated_bytes = hex::decode(decode_target).map_err(|e| {
            KvStoreError::InvalidArgument(format!(
                "invalid truncated hash {:?}: {}",
                truncated_hash, e
            ))
        })?;
        if do_full_string_filter {
            Ok(self
                .dag_set
                .iter()
                .filter(|hash| hash.starts_with(&truncated_bytes))
                .find(|hash| hex::encode(&**hash).starts_with(truncated_hash))
                .cloned())
        } else {
            Ok(self
                .dag_set
                .iter()
                .find(|hash| hash.starts_with(&truncated_bytes))
                .cloned())
        }
    }

    pub fn topo_sort(
        &self,
        start_block_number: i64,
        maybe_end_block_number: Option<i64>,
    ) -> Result<Vec<Vec<BlockHash>>, KvStoreError> {
        let max_number = self.get_max_height();
        let start_number = std::cmp::max(0, start_block_number);
        let end_number = maybe_end_block_number
            .map(|n| std::cmp::min(max_number, n))
            .unwrap_or(max_number);

        if start_number >= 0 && start_number <= end_number {
            Ok(self
                .height_map
                .range(start_number..=end_number)
                .map(|(_, hashes)| hashes.iter().cloned().collect())
                .collect())
        } else {
            Err(KvStoreError::InvalidArgument(format!(
                "Invalid start block number: {}, end block number: {}",
                start_number, end_number
            )))
        }
    }

    pub fn lookup_by_deploy_id(
        &self,
        deploy_id: &DeployId,
    ) -> Result<Option<BlockHash>, KvStoreError> {
        let deploy_index_guard = self.deploy_index.read();
        deploy_index_guard
            .get_one(deploy_id)
            .map(|result| result.map(|block_hash_serde| block_hash_serde.into()))
    }

    // See block-storage/src/main/scala/coop/rchain/blockstorage/dag/BlockDagRepresentationSyntax.scala

    // Get block metadata, "unsafe" because method expects block already in the DAG.
    pub fn lookup_unsafe(&self, block_hash: &BlockHash) -> Result<BlockMetadata, KvStoreError> {
        match self.lookup(block_hash) {
            Ok(Some(metadata)) => Ok(metadata),
            _ => Err(KvStoreError::InvalidArgument(format!(
                "DAG storage is missing hash {}",
                PrettyPrinter::build_string_bytes(block_hash)
            ))),
        }
    }

    pub fn lookups_unsafe(
        &self,
        hashes: Vec<BlockHash>,
    ) -> Result<Vec<BlockMetadata>, KvStoreError> {
        // Small batches are common on propose/snapshot paths; avoid Rayon scheduling overhead there.
        //
        // P5 (slashing audit): threshold of 64 chosen because:
        //  * the propose path's parent/justification lookup typically holds
        //    `n_validators` ≤ ~50 hashes (so the cheap iterator wins);
        //  * the finalization path's ancestor-walk lookup can exceed ~100
        //    hashes (so Rayon's work-stealing wins).
        // Both paths produce identical outputs; the threshold is purely a
        // scheduling-overhead tradeoff. Future tuning would benchmark
        // against representative DAG sizes — until that data exists,
        // 64 is the stable midpoint.
        const PARALLEL_LOOKUP_THRESHOLD: usize = 64;

        if hashes.len() < PARALLEL_LOOKUP_THRESHOLD {
            hashes.iter().map(|h| self.lookup_unsafe(h)).collect()
        } else {
            hashes.par_iter().map(|h| self.lookup_unsafe(h)).collect()
        }
    }

    pub fn latest_message_hash_unsafe(
        &self,
        validator: &Validator,
    ) -> Result<BlockHash, KvStoreError> {
        match self.latest_message_hash(validator) {
            Some(hash) => Ok(hash),
            None => Err(KvStoreError::InvalidArgument(format!(
                "No latest message for validator {}",
                PrettyPrinter::build_string_bytes(validator)
            ))),
        }
    }

    pub fn latest_message(
        &self,
        validator: &Validator,
    ) -> Result<Option<BlockMetadata>, KvStoreError> {
        match self.latest_message_hash(validator) {
            Some(hash) => self.lookup_unsafe(&hash).map(Some),
            None => Ok(None),
        }
    }

    pub fn latest_messages(&self) -> Result<HashMap<Validator, BlockMetadata>, KvStoreError> {
        let latest_messages = self.latest_message_hashes();

        let mut result = HashMap::new();
        for (validator, hash) in latest_messages.iter() {
            let metadata = self.lookup_unsafe(hash)?;
            result.insert(validator.clone(), metadata);
        }

        Ok(result)
    }

    pub fn invalid_latest_messages(&self) -> Result<HashMap<Validator, BlockHash>, KvStoreError> {
        let latest_messages = self.latest_messages()?;
        let latest_message_hashes = latest_messages
            .into_iter()
            .map(|(validator, metadata)| (validator, metadata.block_hash))
            .collect();

        self.invalid_latest_messages_from_hashes(&latest_message_hashes)
    }

    // C13 / Perf-1: take `latest_message_hashes` by shared reference.
    // Callers no longer need to clone a fully-materialized HashMap
    // (the snapshot path used to do this per snapshot) — the hash
    // values are only cloned for the (small) set of entries that
    // actually appear in `invalid_blocks`, so the steady-state work
    // is proportional to |invalid_latest_messages| rather than
    // |latest_message_hashes|.
    pub fn invalid_latest_messages_from_hashes(
        &self,
        latest_message_hashes: &HashMap<Validator, BlockHash>,
    ) -> Result<HashMap<Validator, BlockHash>, KvStoreError> {
        let invalid_blocks = self.invalid_blocks();
        let invalid_block_hashes: HashSet<BlockHash> = invalid_blocks
            .iter()
            .map(|block| block.block_hash.clone())
            .collect();

        Ok(latest_message_hashes
            .iter()
            .filter(|(_, block_hash)| invalid_block_hashes.contains(*block_hash))
            .map(|(validator, block_hash)| (validator.clone(), block_hash.clone()))
            .collect())
    }

    pub fn invalid_blocks_map(&self) -> Result<HashMap<BlockHash, Validator>, KvStoreError> {
        let invalid_blocks = self.invalid_blocks();
        let invalid_block_hashes: HashMap<BlockHash, Validator> = invalid_blocks
            .iter()
            .map(|block| (block.block_hash.clone(), block.sender.clone()))
            .collect();

        Ok(invalid_block_hashes)
    }

    pub fn self_justification_chain(
        &self,
        block_hash: BlockHash,
    ) -> Result<Vec<BlockHash>, KvStoreError> {
        let mut result = Vec::new();
        let mut current_hash = block_hash;

        loop {
            match self.self_justification(&current_hash)? {
                Some(next_hash) => {
                    result.push(next_hash.clone());
                    current_hash = next_hash;
                }
                None => break,
            }
        }

        Ok(result)
    }

    pub fn self_justification(
        &self,
        block_hash: &BlockHash,
    ) -> Result<Option<BlockHash>, KvStoreError> {
        if let Some(hash) = self.self_justification_map.get(block_hash).cloned() {
            return Ok(Some(hash));
        }

        // Keep behavior for blocks that intentionally have no self-justification.
        if !self.contains(block_hash) {
            return Err(KvStoreError::InvalidArgument(format!(
                "DAG storage is missing hash {}",
                PrettyPrinter::build_string_bytes(block_hash)
            )));
        }
        Ok(None)
    }

    pub fn main_parent_chain(
        &self,
        block_hash: BlockHash,
        stop_at_height: i64,
    ) -> Result<Vec<BlockHash>, KvStoreError> {
        let mut result = Vec::new();
        let mut current_hash = block_hash;

        loop {
            let current_block_number = self.block_number_unsafe(&current_hash)?;
            if current_block_number <= stop_at_height {
                break;
            }

            match self.main_parent(&current_hash) {
                Some(parent_hash) => {
                    result.push(parent_hash.clone());
                    current_hash = parent_hash;
                }
                None => break,
            }
        }

        Ok(result)
    }

    pub fn is_in_main_chain(
        &self,
        ancestor: &BlockHash,
        descendant: &BlockHash,
    ) -> Result<bool, KvStoreError> {
        if ancestor == descendant {
            return Ok(true);
        }

        let stop_height = self.block_number_unsafe(ancestor)?;
        let mut current_hash = descendant.clone();

        loop {
            let current_height = self.block_number_unsafe(&current_hash)?;
            if current_height <= stop_height {
                return Ok(current_hash == ancestor);
            }

            let Some(main_parent) = self.main_parent(&current_hash) else {
                return Ok(false);
            };
            current_hash = main_parent;
        }
    }

    pub fn parents_unsafe(&self, block_hash: &BlockHash) -> Result<Vec<BlockHash>, KvStoreError> {
        let metadata = self.lookup_unsafe(block_hash)?;
        Ok(metadata.parents)
    }

    pub fn non_finalized_blocks(&self) -> Result<HashSet<BlockHash>, KvStoreError> {
        let mut result = HashSet::new();
        let mut visited = HashSet::new();
        let mut tips: VecDeque<BlockHash> = self
            .latest_messages()?
            .values()
            .map(|metadata| metadata.block_hash.clone())
            .collect::<VecDeque<_>>();

        while let Some(hash) = tips.pop_front() {
            if !visited.insert(hash.clone()) {
                continue;
            }

            if self.is_finalized(&hash) {
                continue;
            }

            result.insert(hash.clone());

            let metadata = self.lookup_unsafe(&hash)?;
            for parent in metadata.parents {
                if !visited.contains(&parent) {
                    tips.push_back(parent);
                }
            }
        }

        Ok(result)
    }

    pub fn descendants(&self, block_hash: &BlockHash) -> Result<HashSet<BlockHash>, KvStoreError> {
        let mut result = HashSet::new();
        let mut current_level = vec![block_hash.clone()];

        while !current_level.is_empty() {
            let mut next_level = Vec::new();

            for hash in &current_level {
                if let Some(children) = self.children(hash) {
                    for child in children.iter() {
                        if result.insert(child.clone()) {
                            next_level.push(child.clone());
                        }
                    }
                }
            }

            current_level = next_level;
        }

        Ok(result)
    }

    pub fn ancestors(
        &self,
        block_hash: BlockHash,
        filter_f: impl Fn(&BlockHash) -> bool,
    ) -> Result<HashSet<BlockHash>, KvStoreError> {
        let mut result = HashSet::new();
        let mut current_level = vec![block_hash];

        while !current_level.is_empty() {
            let mut next_level = Vec::new();

            for hash in &current_level {
                let metadata = self.lookup_unsafe(hash)?;

                for parent in &metadata.parents {
                    if filter_f(parent) && !result.contains(parent) {
                        result.insert(parent.clone());
                        next_level.push(parent.clone());
                    }
                }
            }

            current_level = next_level;
        }

        Ok(result)
    }

    pub fn with_ancestors(
        &self,
        block_hash: BlockHash,
        filter_f: impl Fn(&BlockHash) -> bool,
    ) -> Result<HashSet<BlockHash>, KvStoreError> {
        let mut result = self.ancestors(block_hash.clone(), filter_f)?;
        result.insert(block_hash);
        Ok(result)
    }
}

/// P2-14 / Phase 11: every internal index is `pub(crate)`. Cross-crate
/// test fixtures that previously poked at these fields must now go
/// through the `#[cfg(any(test, feature = "test-internals"))]`-gated
/// constructor (`from_parts`) and the matching `metadata_index_for_tests`
/// / `deploy_index_for_tests` accessors — see further down this file.
/// **Production code MUST NOT touch these fields directly.** All RMW on
/// the equivocation tracker must route through
/// `access_equivocations_tracker` (Bug #2 / T-9.2 contract). All
/// read/write paths on the metadata / deploy / invalid blocks /
/// latest-messages indices must take `global_lock`.
///
/// Future-self: if you find yourself accessing one of these from
/// outside this file in non-test code, you are introducing a bug.
#[derive(Clone)]
pub struct BlockDagKeyValueStorage {
    /// Global lock to ensure atomic snapshots, similar to Scala's lock.withPermit.
    /// This prevents race conditions during concurrent DAG modifications.
    ///
    /// P2-12: an `RwLock<()>` rather than a `Mutex<()>` so pure-read paths
    /// (`get_representation`) can proceed concurrently with one another while
    /// mutation paths (`insert`, `record_directly_finalized`,
    /// `access_equivocations_tracker`) still take exclusive access. The Bug
    /// #2 / T-9.2 RMW atomicity contract is preserved because every mutator
    /// takes `.write()` — exclusive — and the `access_equivocations_tracker`
    /// closure receives the equivocation index under an exclusive guard.
    pub(crate) global_lock: Arc<PlRwLock<()>>,
    pub(crate) latest_messages_index: KeyValueTypedStoreImpl<ValidatorSerde, BlockHashSerde>,
    pub(crate) block_metadata_index: Arc<PlRwLock<BlockMetadataStore>>,
    pub(crate) deploy_index: Arc<PlRwLock<KeyValueTypedStoreImpl<DeployId, BlockHashSerde>>>,
    pub(crate) invalid_blocks_index: KeyValueTypedStoreImpl<BlockHashSerde, BlockMetadata>,
    /// Equivocation tracker — RMW MUST route through
    /// `access_equivocations_tracker` (Bug #2 / T-9.2).
    pub(crate) equivocation_tracker_index: EquivocationTrackerStore,
    /// Monotonically increasing counter incremented on every successful block insert.
    /// Used by caches to detect when the DAG has changed.
    pub(crate) dag_generation: Arc<AtomicU64>,
}

impl BlockDagKeyValueStorage {
    pub async fn new(kvm: &mut impl KeyValueStoreManager) -> Result<Self, KvStoreError> {
        let block_metadata_kv_store = kvm.store("block-metadata".to_string()).await?;
        let block_metadata_db: KeyValueTypedStoreImpl<BlockHashSerde, BlockMetadata> =
            KeyValueTypedStoreImpl::new(block_metadata_kv_store);
        let block_metadata_store = BlockMetadataStore::new(block_metadata_db);

        let equivocation_tracker_kv_store = kvm.store("equivocation-tracker".to_string()).await?;
        let equivocation_tracker_db: KeyValueTypedStoreImpl<
            (ValidatorSerde, SequenceNumber),
            BTreeSet<BlockHashSerde>,
        > = KeyValueTypedStoreImpl::new(equivocation_tracker_kv_store);
        let equivocation_tracker_store = EquivocationTrackerStore::new(equivocation_tracker_db);

        let latest_messages_kv_store = kvm.store("latest-messages".to_string()).await?;
        let latest_messages_db: KeyValueTypedStoreImpl<ValidatorSerde, BlockHashSerde> =
            KeyValueTypedStoreImpl::new(latest_messages_kv_store);

        let invalid_blocks_kv_store = kvm.store("invalid-blocks".to_string()).await?;
        let invalid_blocks_db: KeyValueTypedStoreImpl<BlockHashSerde, BlockMetadata> =
            KeyValueTypedStoreImpl::new(invalid_blocks_kv_store);

        let deploy_index_kv_store = kvm.store("deploy-index".to_string()).await?;
        let deploy_index_db: KeyValueTypedStoreImpl<DeployId, BlockHashSerde> =
            KeyValueTypedStoreImpl::new(deploy_index_kv_store);

        Ok(Self {
            global_lock: Arc::new(PlRwLock::new(())),
            block_metadata_index: Arc::new(PlRwLock::new(block_metadata_store)),
            deploy_index: Arc::new(PlRwLock::new(deploy_index_db)),
            invalid_blocks_index: invalid_blocks_db,
            equivocation_tracker_index: equivocation_tracker_store,
            latest_messages_index: latest_messages_db,
            dag_generation: Arc::new(AtomicU64::new(0)),
        })
    }

    // P2-16: the following three methods bypass `global_lock` — production
    // code MUST route through `access_equivocations_tracker` to honor the
    // Bug #2 / T-9.2 atomicity contract (see
    // `docs/theory/slashing/slashing-verification.md` §9.2 and
    // `formal/rocq/slashing/theories/BugFixAtomicTracker.v`). They are
    // gated behind `#[cfg(any(test, feature = "test-internals"))]` so the
    // compiler hard-fails on any production caller — the prior
    // `#[deprecated]` annotation was warning-only and could be silenced.
    #[cfg(any(test, feature = "test-internals"))]
    #[doc(hidden)]
    pub fn equivocation_records(&self) -> Result<HashSet<EquivocationRecord>, KvStoreError> {
        self.equivocation_tracker_index.data()
    }

    #[cfg(any(test, feature = "test-internals"))]
    #[doc(hidden)]
    pub fn insert_equivocation_record(
        &self,
        record: EquivocationRecord,
    ) -> Result<(), KvStoreError> {
        self.equivocation_tracker_index.add(record)
    }

    #[cfg(any(test, feature = "test-internals"))]
    #[doc(hidden)]
    pub fn update_equivocation_record(
        &self,
        mut record: EquivocationRecord,
        block_hash: BlockHash,
    ) -> Result<(), KvStoreError> {
        self.equivocation_tracker_index.add({
            record.equivocation_detected_block_hashes.insert(block_hash);
            record
        })
    }

    /// Phase 11 (visibility hardening): test fixtures used to build a
    /// `BlockDagKeyValueStorage` via struct-literal syntax against the
    /// `#[doc(hidden)] pub` indices. Now the indices are `pub(crate)`;
    /// cross-crate test code that needs to wire in custom in-memory
    /// stores must call this constructor instead. Gated behind
    /// `test-internals` so production builds cannot reach it.
    #[cfg(any(test, feature = "test-internals"))]
    #[doc(hidden)]
    #[allow(clippy::too_many_arguments)]
    pub fn from_parts(
        global_lock: Arc<PlRwLock<()>>,
        latest_messages_index: KeyValueTypedStoreImpl<ValidatorSerde, BlockHashSerde>,
        block_metadata_index: Arc<PlRwLock<BlockMetadataStore>>,
        deploy_index: Arc<PlRwLock<KeyValueTypedStoreImpl<DeployId, BlockHashSerde>>>,
        invalid_blocks_index: KeyValueTypedStoreImpl<BlockHashSerde, BlockMetadata>,
        equivocation_tracker_index: EquivocationTrackerStore,
        dag_generation: Arc<AtomicU64>,
    ) -> Self {
        Self {
            global_lock,
            latest_messages_index,
            block_metadata_index,
            deploy_index,
            invalid_blocks_index,
            equivocation_tracker_index,
            dag_generation,
        }
    }

    /// Current DAG generation — incremented on every block insert.
    /// Can be used by caches to detect whether the DAG has changed since the last snapshot.
    pub fn current_generation(&self) -> u64 { self.dag_generation.load(Ordering::Relaxed) }

    /// Public method to get DAG representation with global lock protection.
    /// Matches Scala's lock.withPermit(representation).
    ///
    /// Returns `Err(KvStoreError::LastFinalizedBlockUninitialized)` when called
    /// before the approved-block bootstrap has populated `last_finalized_block`.
    pub fn get_representation(&self) -> Result<KeyValueDagRepresentation, KvStoreError> {
        // P2-12: pure-read path; acquire shared lock so concurrent snapshot
        // readers do not serialize on each other. Mutators take `.write()`.
        let _lock_guard = self.global_lock.read();
        self.get_representation_internal()
    }

    /// Internal method to get representation without acquiring lock.
    /// Used when lock is already held by the caller.
    /// Public to allow IndexedBlockDagStorage to use it.
    pub fn get_representation_internal(&self) -> Result<KeyValueDagRepresentation, KvStoreError> {
        let latest_messages: imbl::HashMap<Validator, BlockHash> = self
            .latest_messages_index
            .to_map()?
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();

        let invalid_blocks: imbl::HashSet<BlockMetadata> =
            self.invalid_blocks_index.to_map()?.into_values().collect();

        let block_metadata_index_guard = self.block_metadata_index.read();
        let dag_state_guard = block_metadata_index_guard.dag_state().read();
        let dag_set = dag_state_guard.dag_set.clone();
        let child_map = dag_state_guard.child_map.clone();
        let height_map = dag_state_guard.height_map.clone();
        let block_number_map = dag_state_guard.block_number_map.clone();
        let main_parent_map = dag_state_guard.main_parent_map.clone();
        let self_justification_map = dag_state_guard.self_justification_map.clone();
        let last_finalized_block = dag_state_guard
            .last_finalized_block
            .as_ref()
            .ok_or(KvStoreError::LastFinalizedBlockUninitialized)?
            .0
            .clone();
        let finalized_blocks = dag_state_guard.finalized_block_set.clone();

        drop(dag_state_guard);
        drop(block_metadata_index_guard);

        Ok(KeyValueDagRepresentation {
            dag_set,
            latest_messages_map: latest_messages,
            child_map,
            height_map,
            block_number_map,
            main_parent_map,
            self_justification_map,
            invalid_blocks_set: invalid_blocks,
            last_finalized_block_hash: last_finalized_block,
            finalized_blocks_set: finalized_blocks,
            block_metadata_index: self.block_metadata_index.clone(),
            deploy_index: self.deploy_index.clone(),
        })
    }

    pub fn insert(
        &self,
        block: &BlockMessage,
        mode: InsertMode,
    ) -> Result<KeyValueDagRepresentation, KvStoreError> {
        // P2-12: insert mutates state; acquire exclusive write lock.
        let _lock_guard = self.global_lock.write();
        self.insert_internal(block, mode)
    }

    /// Internal method to insert without acquiring lock.
    /// Used when lock is already held by the caller.
    /// Public to allow IndexedBlockDagStorage to use it.
    pub fn insert_internal(
        &self,
        block: &BlockMessage,
        mode: InsertMode,
    ) -> Result<KeyValueDagRepresentation, KvStoreError> {
        // Phase 8 (A-6): derive the per-branch booleans directly from the
        // enum via `matches!`. The previous `mode.flags()` projection
        // shim survived a Phase-4 transition; it is no longer needed.
        let invalid = matches!(mode, InsertMode::Invalid);
        let approved = matches!(mode, InsertMode::Approved);
        let sender_is_empty = block.sender.is_empty();
        let sender_has_invalid_format =
            !sender_is_empty && (block.sender.len() != validator::LENGTH);
        let senders_new_lm = (block.sender.clone(), block.block_hash.clone());

        let log_already_stored = format!(
            "Block {} is already stored.",
            PrettyPrinter::build_string_block_message(block, true)
        );
        let log_empty_sender = format!(
            "Block {} sender is empty.",
            PrettyPrinter::build_string_block_message(block, true)
        );

        let new_latest_messages = || -> Result<HashMap<Validator, BlockHash>, KvStoreError> {
            if invalid {
                return Ok(HashMap::new());
            }

            let block_hash: BlockHash = block.block_hash.clone();

            let newly_bonded_set: HashSet<_> = block
                .body
                .state
                .bonds
                .iter()
                .map(|bond| &bond.validator)
                .collect();

            let justification_validators: HashSet<_> = block
                .justifications
                .iter()
                .map(|justification| &justification.validator)
                .collect();

            let mut result = HashMap::new();
            for validator in newly_bonded_set.difference(&justification_validators) {
                // This filter is required to enable adding blocks backward from higher height to lower
                if let Ok(false) = self
                    .latest_messages_index
                    .contains_key(ValidatorSerde((*validator).clone()))
                {
                    result.insert((*validator).clone(), block_hash.clone());
                }
            }

            Ok(result)
        };

        let block_exists = {
            let block_metadata_index_guard = self.block_metadata_index.read();

            block_metadata_index_guard.contains(&block.block_hash)
        };

        if block_exists {
            tracing::warn!("{}", log_already_stored);
            self.get_representation_internal()
        } else {
            let block_hash = block.block_hash.clone();
            let block_hash_is_invalid = block_hash.len() != block_hash::LENGTH;

            if sender_has_invalid_format {
                return Err(KvStoreError::InvalidArgument(format!(
                    "Block sender is malformed., Block: {:?}",
                    block
                )));
            }
            // TODO: should we have special error type for block hash error also?
            //  Should this be checked before calling insert? Is DAG storage responsible for that? - OLD
            if block_hash_is_invalid {
                return Err(KvStoreError::InvalidArgument(format!(
                    "Block hash {} is not correct length.",
                    PrettyPrinter::build_string_bytes(&block_hash)
                )));
            }

            if sender_is_empty {
                tracing::warn!("{}", log_empty_sender);
            }

            let block_metadata = BlockMetadata::from_block(block, invalid, None, None);
            let mut block_metadata_guard = self.block_metadata_index.write();
            block_metadata_guard.add(block_metadata.clone())?;
            drop(block_metadata_guard);
            self.dag_generation.fetch_add(1, Ordering::Relaxed);

            let deploy_hashes: Vec<DeployId> = block
                .body
                .deploys
                .iter()
                .map(|deploy| deploy.deploy.sig.clone().into())
                .collect();
            let deploy_entries: Vec<(DeployId, BlockHashSerde)> = deploy_hashes
                .into_iter()
                .map(|deploy_id| (deploy_id, BlockHashSerde(block.block_hash.clone())))
                .collect();
            let deploy_index_guard = self.deploy_index.write();
            deploy_index_guard.put(deploy_entries)?;
            drop(deploy_index_guard);

            if invalid {
                self.invalid_blocks_index
                    .put_one(block_hash.clone().into(), block_metadata)?;
            }

            let new_latest_from_sender = if !sender_is_empty && !invalid {
                // Add LM either if there is no existing message for the sender, or if sequence number advances
                // - assumes block sender is not valid hash
                if match self
                    .latest_messages_index
                    .get_one(&block.sender.clone().into())
                {
                    Ok(Some(latest_message_hash)) => {
                        let block_metadata_index_guard = self.block_metadata_index.read();
                        match block_metadata_index_guard.get(&latest_message_hash.into()) {
                            Ok(Some(metadata)) => block.seq_num >= metadata.sequence_number,
                            _ => true,
                        }
                    }
                    _ => true,
                } {
                    HashMap::from([senders_new_lm])
                } else {
                    HashMap::new()
                }
            } else {
                HashMap::new()
            };

            let mut new_latest_to_add = new_latest_messages()?;
            new_latest_to_add.extend(new_latest_from_sender);

            self.latest_messages_index.put(
                new_latest_to_add
                    .into_iter()
                    .map(|(k, v)| (k.into(), v.into()))
                    .collect(),
            )?;

            if approved {
                let mut block_metadata_guard = self.block_metadata_index.write();
                // Genesis/approved block has FT=1.0 by construction: it is the DAG root,
                // all validators start from it, so all stake agrees.
                block_metadata_guard.record_finalized(block_hash, HashSet::new(), 1.0)?;
            }

            self.get_representation_internal()
        }
    }

    pub fn access_equivocations_tracker<A>(
        &self,
        f: impl FnOnce(&EquivocationTrackerStore) -> Result<A, KvStoreError>,
    ) -> Result<A, KvStoreError> {
        // P2-12: RMW path — acquire exclusive write lock. Bug #2 / T-9.2
        // atomicity contract: the closure observes the equivocation index
        // under exclusive access; no concurrent reader or writer may
        // observe a partial mutation.
        //
        // SAFETY/CONTRACT (P2-13): non-reentrant. The closure `f` MUST NOT
        // recursively call `access_equivocations_tracker`, nor any
        // operation that acquires `global_lock` (e.g. `insert`,
        // `record_directly_finalized`, `propagate_ft_to_finalized_blocks`,
        // `get_representation`). Doing so deadlocks the
        // `parking_lot::RwLock<()>` based implementation.
        //
        // The bound is `FnOnce` (more permissive than `Fn`, accepts
        // strictly more closures); this aligns with the
        // `EquivocationsAccess` trait at
        // `crate::rust::dag::equivocations_access`. The trait impl for
        // this type delegates to this method, so both surfaces
        // (inherent + trait) share one implementation.
        let _lock_guard = self.global_lock.write();
        f(&self.equivocation_tracker_index)
    }

    /** Record that some hash is directly finalized (detected by finalizer and becomes LFB). */
    pub async fn record_directly_finalized<F, Fut>(
        &self,
        directly_finalized_hash: BlockHash,
        ft_value: f32,
        mut finalization_effect: F,
    ) -> Result<(), KvStoreError>
    where
        F: FnMut(&HashSet<BlockHash>) -> Fut,
        Fut: std::future::Future<Output = Result<(), KvStoreError>>,
    {
        // P5 (slashing audit): bound chosen because typical reconciliation
        // converges in 1–4 loops under realistic block-insert load. A
        // 128-loop ceiling prevents the (TOCTOU-driven) pathological case
        // from spinning indefinitely while leaving generous headroom for
        // catastrophic concurrency. The cap is observable: hitting it
        // emits an `IoError(...)` so operators can detect the condition.
        const MAX_FINALIZATION_RECONCILE_LOOPS: usize = 128;

        // Close TOCTOU race by repeatedly applying effects for newly observed finalized
        // hashes until the lock-protected snapshot is stable. Keep metadata persistence
        // aligned with already-applied effects when exiting due to errors or retry cap.
        let persist_effect_applied =
            |force_direct: bool, effect_applied: &HashSet<BlockHash>| -> Result<(), KvStoreError> {
                if !force_direct && effect_applied.is_empty() {
                    return Ok(());
                }

                let indirectly_finalized: HashSet<BlockHash> = effect_applied
                    .iter()
                    .filter(|hash| *hash != &directly_finalized_hash)
                    .cloned()
                    .collect();

                // P2-12: record_finalized mutates block metadata; exclusive lock.
                let _lock_guard = self.global_lock.write();
                let mut block_metadata_index_guard = self.block_metadata_index.write();
                block_metadata_index_guard.record_finalized(
                    directly_finalized_hash.clone(),
                    indirectly_finalized,
                    ft_value,
                )
            };

        let mut effect_applied: HashSet<BlockHash> = HashSet::new();
        for _attempt in 0..MAX_FINALIZATION_RECONCILE_LOOPS {
            let pending_effect: HashSet<BlockHash> = {
                // P2-12: snapshot read; shared lock allows concurrent readers.
                let _lock_guard = self.global_lock.read();

                let dag = self.get_representation_internal()?;
                if !dag.contains(&directly_finalized_hash) {
                    return Err(KvStoreError::InvalidArgument(format!(
                        "Attempting to finalize nonexistent hash {}",
                        PrettyPrinter::build_string_bytes(&directly_finalized_hash)
                    )));
                }

                let indirectly_finalized = dag
                    .ancestors(directly_finalized_hash.clone(), |hash| {
                        !dag.is_finalized(hash)
                    })?;

                let mut all_finalized = indirectly_finalized.clone();
                all_finalized.insert(directly_finalized_hash.clone());

                let pending: HashSet<BlockHash> =
                    all_finalized.difference(&effect_applied).cloned().collect();

                pending
            };

            if pending_effect.is_empty() {
                persist_effect_applied(true, &effect_applied)?;

                // Propagate FT to all finalized blocks whose cached value is lower.
                // This ensures FT converges toward 1.0 as later finalization
                // rounds produce higher agreement. Covers orphaned branches
                // not reachable via the new LFB's ancestor chain.
                self.propagate_ft_to_finalized_blocks(ft_value)?;

                return Ok(());
            }

            // Execute async effect without holding lock.
            if let Err(err) = finalization_effect(&pending_effect).await {
                persist_effect_applied(false, &effect_applied)?;
                return Err(err);
            }
            effect_applied.extend(pending_effect);
        }

        persist_effect_applied(false, &effect_applied)?;
        Err(KvStoreError::IoError(format!(
            "record_directly_finalized exceeded {} reconcile loops for {}",
            MAX_FINALIZATION_RECONCILE_LOOPS,
            PrettyPrinter::build_string_bytes(&directly_finalized_hash)
        )))
    }

    fn propagate_ft_to_finalized_blocks(&self, ft_value: f32) -> Result<(), KvStoreError> {
        // P2-12: mutates `block_metadata_index`; exclusive lock.
        let _lock_guard = self.global_lock.write();

        // Update ALL finalized blocks with lower FT, not just ancestors of the
        // current LFB. In a multi-parent DAG, finalized blocks on orphaned
        // branches are not reachable via the ancestor chain of the new LFB.
        let mut block_metadata_index_guard = self.block_metadata_index.write();
        let finalized_hashes = block_metadata_index_guard.finalized_block_hashes();
        block_metadata_index_guard.update_ft_if_higher(finalized_hashes, ft_value)
    }
}

// EquivocationsAccess trait impl — delegates to the inherent method.
// The inherent method remains the canonical implementation; the trait
// gives callers a type-level dispatch contract for atomic-RMW access
// to the equivocation tracker. See `equivocations_access.rs` for the
// full design rationale (T-9.2 anchor, atomic-RMW contract).
impl super::equivocations_access::EquivocationsAccess for BlockDagKeyValueStorage {
    fn access_equivocations_tracker<A>(
        &self,
        f: impl FnOnce(&EquivocationTrackerStore) -> Result<A, KvStoreError>,
    ) -> Result<A, KvStoreError> {
        BlockDagKeyValueStorage::access_equivocations_tracker(self, f)
    }
}
