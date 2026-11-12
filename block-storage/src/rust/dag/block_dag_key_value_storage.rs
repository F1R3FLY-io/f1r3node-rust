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
//! * `block_metadata_index` is itself `parking_lot::RwLock`-wrapped for
//!   fine-grained concurrency.
//!
//! See `docs/casper/theory/slashing/slashing-verification.md` for the
//! protocol-level theorems whose witnesses are recorded here.

// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/casper/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// See block-storage/src/main/scala/coop/rchain/blockstorage/dag/BlockDagKeyValueStorage.scala

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

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
use shared::rust::blocking::in_blocking;
use shared::rust::dag::observation_work::{NoopWork, WorkKind, WorkMeter};
use shared::rust::store::atomic_store::AtomicStoreGroup;
use shared::rust::store::key_value_store::{KvStoreError, MissingBlockContext};
use shared::rust::store::key_value_typed_store::KeyValueTypedStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;
use shared::rust::store::soak_snapshot::SnapshotError;
use tokio::sync::mpsc;

use super::block_metadata_store::BlockMetadataStore;
use super::carrier_index::CarrierIndex;
use super::dag_gc_store::DagGcStore;
use super::dag_history_index::{DagHistoryIndex, HistoryPage};
use super::dag_persistence::DagPersistence;
use super::deploy_lifecycle_types::{
    DeployLifecycleTables, LifecycleEvent, LifecycleEventKind, LifecycleEvents, TerminalRecord,
};
use super::equivocation_tracker_store::EquivocationTrackerStore;

pub type DeployId = shared::rust::ByteString;

pub struct DagResidentCounts {
    pub blocks: usize,
    pub child_keys: usize,
    pub heights: usize,
    pub finalized: usize,
}

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
    /// Settled-history insertion: a hash-checked, unjudged block below the
    /// node's sync anchor, admitted the way LFS restore admitted its
    /// neighbours. Identical to `Normal` except that latest messages are
    /// untouched — settled history predates the anchor's justification
    /// frontier, so it is never anyone's latest message, and letting it
    /// advance one hands fork choice a frontier this node does not hold.
    SettledHistory,
    SlashEvidence,
}

// Phase 8 (A-6): `InsertMode::flags()` projection deleted; `insert_internal`
// now dispatches on `mode` via `matches!` directly.

#[derive(Clone)]
pub struct KeyValueDagRepresentation {
    pub(crate) history: Option<Arc<DagHistoryIndex>>,
    pub(crate) view_sequence: u64,
    pub(crate) view_max_height: Option<i64>,
    pub(crate) dag_set: imbl::HashSet<BlockHash>,
    pub(crate) latest_messages_map: imbl::HashMap<Validator, BlockHash>,
    pub(crate) child_map: imbl::HashMap<BlockHash, imbl::HashSet<BlockHash>>,
    pub(crate) height_map: imbl::OrdMap<i64, imbl::HashSet<BlockHash>>,
    pub(crate) block_number_map: imbl::HashMap<BlockHash, i64>,
    pub(crate) main_parent_map: imbl::HashMap<BlockHash, BlockHash>,
    pub(crate) self_justification_map: imbl::HashMap<BlockHash, BlockHash>,
    pub(crate) invalid_blocks_set: imbl::HashSet<BlockMetadata>,
    pub last_finalized_block_hash: BlockHash,
    pub(crate) finalized_blocks_set: imbl::HashSet<BlockHash>,
    // P2-14: the metadata index is kept `pub` for cross-crate test
    // fixtures that build a `KeyValueDagRepresentation` from raw
    // components. Production code on the same crate (block-storage)
    // accesses it through the inherent methods on this type; treat
    // direct manipulation as a test-only escape hatch.
    #[doc(hidden)]
    pub block_metadata_index: Arc<PlRwLock<BlockMetadataStore>>,
    /// Memoized justification-derived floor per block (block hash -> floor hash).
    /// Pure function of the block, so node-identical; persistent to avoid
    /// re-walking to genesis on every floor query.
    pub floor_index: KeyValueTypedStoreImpl<BlockHashSerde, BlockHashSerde>,
    /// Memoized per-block finalized FRONTIER (block hash -> frontier hash):
    /// F(X) = parent_frontier(X, just(X)), the highest witnessed-finalized block
    /// on X's own main-parent spine over X's frozen justification snapshot. A
    /// pure function of the block (like `floor_index`); caching it turns the
    /// per-merge floor walk from O(Delta^2) into an amortized-O(1) incremental
    /// up-walk (finalized-floor fix; see finality/floor.rs).
    pub frontier_index: KeyValueTypedStoreImpl<BlockHashSerde, BlockHashSerde>,
    /// The deploy-lifecycle tables (see `deploy_lifecycle_types`): per-sig
    /// event rows fed by `insert`'s body pass, and the WRITE-ONCE terminal
    /// verdicts the finality layer's register writes. The status API reads
    /// these — Pending/Finalized/Expired/Failed are lookups, never
    /// computations.
    pub lifecycle: Arc<PlRwLock<DeployLifecycleTables>>,
    /// The repeat-deploy carrier index (see `carrier_index`): per-sig
    /// carrier records over valid, invalid, and settled blocks, plus the
    /// engagement watermark. Fed by `insert` before DAG visibility.
    pub carrier_index: Arc<PlRwLock<CarrierIndex>>,
}

#[cfg(any(test, feature = "test-internals"))]
pub struct DagFixture {
    pub history: Option<Arc<DagHistoryIndex>>,
    pub view_sequence: u64,
    pub view_max_height: Option<i64>,
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
    pub block_metadata_index: Arc<PlRwLock<BlockMetadataStore>>,
    pub floor_index: KeyValueTypedStoreImpl<BlockHashSerde, BlockHashSerde>,
    pub frontier_index: KeyValueTypedStoreImpl<BlockHashSerde, BlockHashSerde>,
    pub lifecycle: Arc<PlRwLock<DeployLifecycleTables>>,
    pub carrier_index: Arc<PlRwLock<CarrierIndex>>,
}

#[cfg(any(test, feature = "test-internals"))]
impl DagFixture {
    pub fn build(self) -> KeyValueDagRepresentation {
        assert!(
            self.history.is_none(),
            "fixtures must be detached from durable publication"
        );
        KeyValueDagRepresentation {
            history: None,
            view_sequence: self.view_sequence,
            view_max_height: self.view_max_height,
            dag_set: self.dag_set,
            latest_messages_map: self.latest_messages_map,
            child_map: self.child_map,
            height_map: self.height_map,
            block_number_map: self.block_number_map,
            main_parent_map: self.main_parent_map,
            self_justification_map: self.self_justification_map,
            invalid_blocks_set: self.invalid_blocks_set,
            last_finalized_block_hash: self.last_finalized_block_hash,
            finalized_blocks_set: self.finalized_blocks_set,
            block_metadata_index: self.block_metadata_index,
            floor_index: self.floor_index,
            frontier_index: self.frontier_index,
            lifecycle: self.lifecycle,
            carrier_index: self.carrier_index,
        }
    }
}

#[cfg(any(test, feature = "test-internals"))]
pub struct DagFixtureMut<'a> {
    pub dag_set: &'a mut imbl::HashSet<BlockHash>,
    pub latest_messages_map: &'a mut imbl::HashMap<Validator, BlockHash>,
    pub child_map: &'a mut imbl::HashMap<BlockHash, imbl::HashSet<BlockHash>>,
    pub height_map: &'a mut imbl::OrdMap<i64, imbl::HashSet<BlockHash>>,
    pub block_number_map: &'a mut imbl::HashMap<BlockHash, i64>,
    pub main_parent_map: &'a mut imbl::HashMap<BlockHash, BlockHash>,
    pub self_justification_map: &'a mut imbl::HashMap<BlockHash, BlockHash>,
    pub invalid_blocks_set: &'a mut imbl::HashSet<BlockMetadata>,
    pub finalized_blocks_set: &'a mut imbl::HashSet<BlockHash>,
}

#[cfg(any(test, feature = "test-internals"))]
impl KeyValueDagRepresentation {
    pub fn resident_bounds(&self) -> (Option<i64>, usize) {
        (
            self.block_number_map.values().copied().min(),
            self.child_map.values().map(|children| children.len()).sum(),
        )
    }
    pub fn detached_fixture(&self) -> Result<DagFixture, KvStoreError> {
        let raw = Arc::new(
            rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore::new(),
        );
        let mut metadata = BlockMetadataStore::new(KeyValueTypedStoreImpl::new(raw));
        let mut invalid_blocks = imbl::HashSet::new();
        let mut after = None;
        loop {
            let page = self.scan_membership_page(after.as_deref(), 1024)?;
            for hash in page.rows {
                let block = self.lookup_unsafe(&hash)?;
                if block.invalid {
                    invalid_blocks.insert(block.clone());
                }
                metadata.add(block)?;
            }
            after = page.next;
            if after.is_none() {
                break;
            }
        }
        let state = metadata.dag_state().read().clone();
        Ok(DagFixture {
            history: None,
            view_sequence: self.view_sequence,
            view_max_height: self.view_max_height,
            dag_set: state.dag_set,
            latest_messages_map: self.latest_messages_map.clone(),
            child_map: state.child_map,
            height_map: state.height_map,
            block_number_map: state.block_number_map,
            main_parent_map: state.main_parent_map,
            self_justification_map: state.self_justification_map,
            invalid_blocks_set: invalid_blocks,
            last_finalized_block_hash: self.last_finalized_block_hash.clone(),
            finalized_blocks_set: state.finalized_block_set,
            block_metadata_index: Arc::new(PlRwLock::new(metadata)),
            floor_index: self.floor_index.clone(),
            frontier_index: self.frontier_index.clone(),
            lifecycle: self.lifecycle.clone(),
            carrier_index: self.carrier_index.clone(),
        })
    }

    pub fn fixture_mut(&mut self) -> DagFixtureMut<'_> {
        assert!(
            self.history.is_none(),
            "durable views cannot be edited as fixtures"
        );
        DagFixtureMut {
            dag_set: &mut self.dag_set,
            latest_messages_map: &mut self.latest_messages_map,
            child_map: &mut self.child_map,
            height_map: &mut self.height_map,
            block_number_map: &mut self.block_number_map,
            main_parent_map: &mut self.main_parent_map,
            self_justification_map: &mut self.self_justification_map,
            invalid_blocks_set: &mut self.invalid_blocks_set,
            finalized_blocks_set: &mut self.finalized_blocks_set,
        }
    }
}

impl KeyValueDagRepresentation {
    pub fn publication_sequence(&self) -> u64 { self.view_sequence }

    pub fn resident_counts(&self) -> DagResidentCounts {
        DagResidentCounts {
            blocks: self.dag_set.len(),
            child_keys: self.child_map.len(),
            heights: self.height_map.len(),
            finalized: self.finalized_blocks_set.len(),
        }
    }

    pub fn scan_membership_page(
        &self,
        after: Option<&[u8]>,
        limit: usize,
    ) -> Result<HistoryPage<BlockHash>, KvStoreError> {
        if let Some(history) = &self.history {
            return history.scan_membership(self.view_sequence, after, limit);
        }
        if limit == 0 || limit > 1024 {
            return Err(KvStoreError::InvalidArgument(
                "DAG page limit must be 1..=1024".into(),
            ));
        }
        let mut rows: Vec<_> = self
            .dag_set
            .iter()
            .filter(|hash| after.is_none_or(|key| hash.as_ref() > key))
            .cloned()
            .collect();
        rows.sort();
        rows.truncate(limit);
        let next = (rows.len() == limit).then(|| rows.last().unwrap().to_vec());
        Ok(HistoryPage { rows, next })
    }

    pub fn scan_height_range(
        &self,
        start: i64,
        end: i64,
        valid_only: bool,
        after: Option<&[u8]>,
        limit: usize,
    ) -> Result<HistoryPage<BlockHash>, KvStoreError> {
        if let Some(history) = &self.history {
            return history.scan_height_range(
                start,
                end,
                self.view_sequence,
                valid_only,
                after,
                limit,
            );
        }
        if limit == 0 || limit > 1024 {
            return Err(KvStoreError::InvalidArgument(
                "DAG page limit must be 1..=1024".into(),
            ));
        }
        let mut entries = Vec::new();
        for hash in &self.dag_set {
            let metadata = self.lookup_unsafe(hash)?;
            if metadata.block_number < start
                || metadata.block_number > end
                || (valid_only && metadata.invalid)
            {
                continue;
            }
            let mut key = ((metadata.block_number as u64) ^ (1 << 63))
                .to_be_bytes()
                .to_vec();
            key.extend_from_slice(hash);
            if after.is_none_or(|after| key.as_slice() > after) {
                entries.push((key, hash.clone()));
            }
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        entries.truncate(limit);
        let next = (entries.len() == limit).then(|| entries.last().unwrap().0.clone());
        Ok(HistoryPage {
            rows: entries.into_iter().map(|(_, hash)| hash).collect(),
            next,
        })
    }
    pub fn lookup(&self, block_hash: &BlockHash) -> Result<Option<BlockMetadata>, KvStoreError> {
        let expected = if let Some(history) = &self.history {
            if block_hash.len() != block_hash::LENGTH {
                return Ok(None);
            }
            if let Some(height) = self.block_number_map.get(block_hash) {
                Some((
                    *height,
                    !self
                        .height_map
                        .get(height)
                        .is_some_and(|hashes| hashes.contains(block_hash)),
                ))
            } else if let Some(member) = history.membership(block_hash, self.view_sequence)? {
                Some((member.height, member.invalid))
            } else {
                return Ok(None);
            }
        } else {
            if !self.dag_set.contains(block_hash) {
                return Ok(None);
            }
            None
        };
        let metadata = self
            .block_metadata_index
            .read()
            .get(block_hash)?
            .ok_or_else(|| {
                KvStoreError::KeyNotFound(format!(
                    "published block metadata missing: {}",
                    hex::encode(block_hash)
                ))
            })?;
        if let Some((height, invalid)) = expected {
            if metadata.block_hash != *block_hash
                || metadata.block_number != height
                || metadata.invalid != invalid
            {
                return Err(KvStoreError::InvalidArgument(format!(
                    "published metadata disagrees with DAG membership: {}",
                    hex::encode(block_hash)
                )));
            }
        }
        Ok(Some(metadata))
    }

    pub fn contains(&self, block_hash: &BlockHash) -> Result<bool, KvStoreError> {
        if self.history.is_none() {
            return Ok(self.dag_set.contains(block_hash));
        }
        if block_hash.len() != block_hash::LENGTH {
            return Ok(false);
        }
        if self.dag_set.contains(block_hash) {
            return Ok(true);
        }
        match &self.history {
            Some(history) => Ok(history
                .membership(block_hash, self.view_sequence)?
                .is_some()),
            None => Ok(false),
        }
    }

    pub fn children(
        &self,
        block_hash: &BlockHash,
    ) -> Result<Option<imbl::HashSet<BlockHash>>, KvStoreError> {
        let Some(history) = &self.history else {
            return Ok(self.child_map.get(block_hash).cloned());
        };
        let mut children = imbl::HashSet::new();
        let mut after = None;
        loop {
            let page =
                history.scan_children(block_hash, self.view_sequence, after.as_deref(), 1024)?;
            children.extend(page.rows);
            after = page.next;
            if after.is_none() {
                break;
            }
        }
        if children.is_empty() && !self.contains(block_hash)? {
            Ok(None)
        } else {
            Ok(Some(children))
        }
    }

    pub fn has_children(&self, block_hash: &BlockHash) -> Result<bool, KvStoreError> {
        let Some(history) = &self.history else {
            return Ok(self
                .child_map
                .get(block_hash)
                .is_some_and(|children| !children.is_empty()));
        };
        let mut after = None;
        loop {
            let page =
                history.scan_children(block_hash, self.view_sequence, after.as_deref(), 1)?;
            if !page.rows.is_empty() {
                return Ok(true);
            }
            after = page.next;
            if after.is_none() {
                return Ok(false);
            }
        }
    }

    pub fn latest_message_hash(&self, validator: &Validator) -> Option<BlockHash> {
        self.latest_messages_map.get(validator).cloned()
    }

    pub fn latest_message_hashes(&self) -> imbl::HashMap<Validator, BlockHash> {
        self.latest_messages_map.clone()
    }

    /// The latest-message slot's block if it is the validator's own testimony:
    /// held, and signed by that validator. A held block signed by someone else
    /// is the genesis placeholder seeded for a bonded validator that never
    /// proposed; citing it makes a height-0 block a parent candidate, which
    /// bounds every walk at zero.
    pub fn own_testimony(
        &self,
        validator: &Validator,
        hash: &BlockHash,
    ) -> Result<Option<BlockMetadata>, KvStoreError> {
        Ok(self
            .lookup(hash)?
            .filter(|metadata| metadata.sender == *validator))
    }

    pub fn invalid_blocks(&self) -> imbl::HashSet<BlockMetadata> { self.invalid_blocks_set.clone() }

    /// Cached justification-derived floor of a block, if already computed.
    pub fn get_cached_floor(
        &self,
        block_hash: &BlockHash,
    ) -> Result<Option<BlockHash>, KvStoreError> {
        Ok(self
            .floor_index
            .get_one(&BlockHashSerde(block_hash.clone()))?
            .map(|serde| serde.0))
    }

    /// Cache a block's justification-derived floor (pure function of the block).
    pub fn put_cached_floor(
        &self,
        block_hash: BlockHash,
        floor_hash: BlockHash,
    ) -> Result<(), KvStoreError> {
        self.floor_index
            .put_one(BlockHashSerde(block_hash), BlockHashSerde(floor_hash))
    }

    /// Cached per-block finalized frontier F(block), if already computed.
    pub fn get_cached_frontier(
        &self,
        block_hash: &BlockHash,
    ) -> Result<Option<BlockHash>, KvStoreError> {
        Ok(self
            .frontier_index
            .get_one(&BlockHashSerde(block_hash.clone()))?
            .map(|serde| serde.0))
    }

    /// Cache a block's finalized frontier F(block) (pure function of the block).
    pub fn put_cached_frontier(
        &self,
        block_hash: BlockHash,
        frontier_hash: BlockHash,
    ) -> Result<(), KvStoreError> {
        self.frontier_index
            .put_one(BlockHashSerde(block_hash), BlockHashSerde(frontier_hash))
    }

    /// Lifecycle: the write-once terminal record for a sig, if determined.
    pub fn deploy_terminal(&self, sig: &[u8]) -> Result<Option<TerminalRecord>, KvStoreError> {
        self.lifecycle.read().get_terminal(sig)
    }

    /// Lifecycle: the open event row for a sig (pruned once terminal).
    pub fn deploy_lifecycle_events(
        &self,
        sig: &[u8],
    ) -> Result<Option<LifecycleEvents>, KvStoreError> {
        self.lifecycle.read().get_events(sig)
    }

    /// Lifecycle: WRITE-ONCE terminal write (prunes the event row on a
    /// fresh write; returns the survivor on a duplicate attempt).
    pub fn put_deploy_terminal_if_absent(
        &self,
        sig: &[u8],
        record: TerminalRecord,
    ) -> Result<TerminalRecord, KvStoreError> {
        self.lifecycle.write().put_terminal_if_absent(sig, record)
    }

    /// Lifecycle: every open event row — the work list of a settle pass.
    pub fn open_lifecycle_rows(&self) -> Result<Vec<(DeployId, LifecycleEvents)>, KvStoreError> {
        self.lifecycle.read().open_rows()
    }

    /// Lifecycle: drop an open row if `discardable` still holds for its
    /// contents under the write lock, so an event appended since the caller
    /// read the row is never deleted with it.
    pub fn discard_lifecycle_events_if(
        &self,
        sig: &[u8],
        discardable: impl Fn(&LifecycleEvents) -> bool,
    ) -> Result<bool, KvStoreError> {
        self.lifecycle.write().discard_events_if(sig, discardable)
    }

    /// Repeat-deploy carrier index: the height since which every insert
    /// has recorded carriers on this database. The fast path engages only
    /// for scan windows starting at or above this watermark; `None` means
    /// the index was never initialized and the fast path stays off.
    pub fn carrier_index_watermark(&self) -> Result<Option<i64>, KvStoreError> {
        self.carrier_index.read().watermark()
    }

    /// Repeat-deploy carrier index: the highest cutoff a prune walk has
    /// started with. The fast path engages only for scan windows starting
    /// at or above it; a prune runs concurrently with validation, so the
    /// caller re-reads it after probing.
    pub fn carrier_index_prune_cutoff(&self) -> Result<Option<i64>, KvStoreError> {
        self.carrier_index.read().prune_cutoff()
    }

    /// Repeat-deploy carrier index: true when the index holds NO carrier
    /// for the sig. Sound as an absence proof only when the scan window
    /// starts at or above the watermark and the prune cutoff. A `false` is
    /// not a verdict — it routes the sig to the exact ancestor scan.
    pub fn carrier_index_proves_absence(&self, sig: &[u8]) -> Result<bool, KvStoreError> {
        self.carrier_index.read().proves_absence(sig)
    }

    /// Repeat-deploy carrier index: drop entries below the cutoff. The
    /// finalizer drives this on LFB advances.
    pub fn prune_carriers_below(&self, cutoff: i64) -> Result<u64, KvStoreError> {
        self.carrier_index.read().prune_below(cutoff)
    }

    /// The sig's most recent canonical appearance — the latest lifecycle
    /// event by (height, hash), or the terminal record's frozen display
    /// block once the row is pruned. A pure function of the DAG's bodies,
    /// so the answer never depends on node-local insertion order. Events
    /// whose block is not in this representation's `dag_set` are orphans
    /// from a crash inside the ingest-first window and never resolve.
    pub fn deploy_canonical_appearance(
        &self,
        sig: &[u8],
    ) -> Result<Option<BlockHash>, KvStoreError> {
        Ok(self
            .lifecycle
            .read()
            .canonical_appearance(sig, &|h| self.contains(&BlockHash::copy_from_slice(h)))?
            .map(BlockHash::from))
    }

    pub fn last_finalized_block(&self) -> BlockHash { self.last_finalized_block_hash.clone() }

    // latestBlockNumber, topoSort and lookupByDeployId are only used in BlockAPI.
    // Do they need to be part of the DAG current state or they can be moved to DAG storage directly?

    pub fn get_max_height(&self) -> i64 {
        if self.history.is_some() {
            return self
                .view_max_height
                .map_or(0, |height| height.saturating_add(1));
        }
        if self.height_map.is_empty() {
            0
        } else {
            self.height_map.get_max().expect("height_map is empty").0 + 1
        }
    }

    pub fn latest_block_number(&self) -> i64 { self.get_max_height() }

    pub fn block_number(&self, block_hash: &BlockHash) -> Result<Option<i64>, KvStoreError> {
        if let Some(height) = self.block_number_map.get(block_hash) {
            return Ok(Some(*height));
        }
        Ok(self
            .lookup(block_hash)?
            .map(|metadata| metadata.block_number))
    }

    pub fn block_number_unsafe(&self, block_hash: &BlockHash) -> Result<i64, KvStoreError> {
        self.block_number(block_hash)?
            .ok_or_else(|| missing_block(block_hash, "block_number_unsafe"))
    }

    pub fn main_parent(&self, block_hash: &BlockHash) -> Result<Option<BlockHash>, KvStoreError> {
        if let Some(parent) = self.main_parent_map.get(block_hash) {
            return Ok(Some(parent.clone()));
        }
        Ok(self
            .lookup(block_hash)?
            .and_then(|metadata| metadata.parents.first().cloned()))
    }

    pub fn is_finalized(&self, block_hash: &BlockHash) -> Result<bool, KvStoreError> {
        if self.finalized_blocks_set.contains(block_hash) {
            return Ok(true);
        }

        // Finalized status is persisted in block metadata; in-memory set is a bounded cache.
        Ok(self.lookup(block_hash)?.is_some_and(|m| m.finalized))
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
        if self.history.is_some() {
            let mut after = None;
            loop {
                let page = self.history.as_ref().unwrap().scan_prefix(
                    &truncated_bytes,
                    self.view_sequence,
                    after.as_deref(),
                    1024,
                )?;
                if let Some(hash) = page.rows.into_iter().find(|hash| {
                    hash.starts_with(&truncated_bytes)
                        && (!do_full_string_filter || hex::encode(hash).starts_with(truncated_hash))
                }) {
                    return Ok(Some(hash));
                }
                after = page.next;
                if after.is_none() {
                    return Ok(None);
                }
            }
        }
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
            if self.history.is_some() {
                let mut heights = std::collections::BTreeMap::<i64, Vec<BlockHash>>::new();
                let mut after = None;
                loop {
                    let page = self.scan_height_range(
                        start_number,
                        end_number,
                        true,
                        after.as_deref(),
                        1024,
                    )?;
                    for hash in page.rows {
                        heights
                            .entry(self.block_number_unsafe(&hash)?)
                            .or_default()
                            .push(hash);
                    }
                    after = page.next;
                    if after.is_none() {
                        break;
                    }
                }
                return Ok(heights.into_values().collect());
            }
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

    // See block-storage/src/main/scala/coop/rchain/blockstorage/dag/BlockDagRepresentationSyntax.scala

    // Get block metadata, "unsafe" because method expects block already in the DAG.
    // (see `missing_hash_context` below for why these errors carry a backtrace)
    pub fn lookup_unsafe(&self, block_hash: &BlockHash) -> Result<BlockMetadata, KvStoreError> {
        self.lookup(block_hash)?
            .ok_or_else(|| missing_block(block_hash, "lookup_unsafe"))
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

    // A latest-message slot can name a block this node does not hold (a
    // stale slot below an LFS restore horizon); both readers treat it as
    // absent rather than erroring.
    pub fn latest_message(
        &self,
        validator: &Validator,
    ) -> Result<Option<BlockMetadata>, KvStoreError> {
        match self.latest_message_hash(validator) {
            Some(hash) => self.lookup(&hash),
            None => Ok(None),
        }
    }

    pub fn latest_messages(&self) -> Result<HashMap<Validator, BlockMetadata>, KvStoreError> {
        let latest_messages = self.latest_message_hashes();

        let mut result = HashMap::new();
        for (validator, hash) in latest_messages.iter() {
            if let Some(metadata) = self.lookup(hash)? {
                result.insert(validator.clone(), metadata);
            }
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
        let mut result = HashMap::new();
        for (validator, block_hash) in latest_message_hashes {
            if self
                .lookup(block_hash)?
                .map(|metadata| metadata.invalid)
                .unwrap_or(false)
            {
                result.insert(validator.clone(), block_hash.clone());
            }
        }
        Ok(result)
    }

    #[cfg(any(test, feature = "test-internals"))]
    pub fn invalid_blocks_map(&self) -> Result<HashMap<BlockHash, Validator>, KvStoreError> {
        let invalid_blocks = self.invalid_blocks();
        let mut invalid_block_hashes = HashMap::new();
        for block in invalid_blocks {
            if let Some(metadata) = self.lookup(&block.block_hash)? {
                if metadata.invalid {
                    invalid_block_hashes.insert(metadata.block_hash, metadata.sender);
                }
            }
        }
        Ok(invalid_block_hashes)
    }

    pub fn self_justification(
        &self,
        block_hash: &BlockHash,
    ) -> Result<Option<BlockHash>, KvStoreError> {
        if let Some(hash) = self.self_justification_map.get(block_hash).cloned() {
            return Ok(Some(hash));
        }

        // Keep behavior for blocks that intentionally have no self-justification.
        let metadata = self.lookup_unsafe(block_hash)?;
        Ok(metadata
            .justifications
            .iter()
            .find(|j| j.validator == metadata.sender)
            .map(|j| j.latest_block_hash.clone()))
    }

    pub fn lookup_metered<W: WorkMeter>(
        &self,
        meter: &W,
        hash: &BlockHash,
    ) -> Result<Option<BlockMetadata>, KvStoreError> {
        meter.lookup()?;
        self.lookup(hash)
    }

    pub fn lookup_unsafe_metered<W: WorkMeter>(
        &self,
        meter: &W,
        hash: &BlockHash,
    ) -> Result<BlockMetadata, KvStoreError> {
        meter.lookup()?;
        self.lookup_unsafe(hash)
    }

    pub fn own_testimony_metered<W: WorkMeter>(
        &self,
        meter: &W,
        validator: &Validator,
        hash: &BlockHash,
    ) -> Result<Option<BlockMetadata>, KvStoreError> {
        Ok(self
            .lookup_metered(meter, hash)?
            .filter(|meta| meta.sender == *validator))
    }

    pub fn main_parent_chain(
        &self,
        block_hash: BlockHash,
        stop_at_height: i64,
    ) -> Result<Vec<BlockHash>, KvStoreError> {
        self.main_parent_chain_metered(&NoopWork, block_hash, stop_at_height)
    }

    pub fn is_in_main_chain(
        &self,
        ancestor: &BlockHash,
        descendant: &BlockHash,
    ) -> Result<bool, KvStoreError> {
        self.is_in_main_chain_metered(&NoopWork, ancestor, descendant)
    }

    pub fn is_dag_ancestor(
        &self,
        ancestor: &BlockHash,
        descendant: &BlockHash,
    ) -> Result<bool, KvStoreError> {
        self.is_dag_ancestor_metered(&NoopWork, ancestor, descendant)
    }

    pub fn main_parent_chain_metered<W: WorkMeter>(
        &self,
        meter: &W,
        block_hash: BlockHash,
        stop_at_height: i64,
    ) -> Result<Vec<BlockHash>, KvStoreError> {
        let mut result = Vec::new();
        let mut current_hash = block_hash;

        loop {
            meter.step(WorkKind::Traversal)?;
            let current_block_number = self.block_number_unsafe(&current_hash)?;
            if current_block_number <= stop_at_height {
                break;
            }

            match self.main_parent(&current_hash)? {
                Some(parent_hash) => {
                    meter.allocate(2, 64)?;
                    result.push(parent_hash.clone());
                    current_hash = parent_hash;
                }
                None => break,
            }
        }

        Ok(result)
    }

    pub fn is_in_main_chain_metered<W: WorkMeter>(
        &self,
        meter: &W,
        ancestor: &BlockHash,
        descendant: &BlockHash,
    ) -> Result<bool, KvStoreError> {
        if ancestor == descendant {
            return Ok(true);
        }

        let stop_height = self.block_number_unsafe(ancestor)?;
        let mut current_hash = descendant.clone();

        loop {
            meter.step(WorkKind::Traversal)?;
            let current_height = self.block_number_unsafe(&current_hash)?;
            if current_height <= stop_height {
                return Ok(current_hash == ancestor);
            }

            let Some(main_parent) = self.main_parent(&current_hash)? else {
                return Ok(false);
            };
            current_hash = main_parent;
        }
    }

    /// Is `ancestor` reachable from `descendant` via ANY parent path (general
    /// DAG ancestry), as opposed to `is_in_main_chain` which follows only the
    /// main-parent spine. Used by multi-parent finality: a validator whose
    /// latest message DAG-descends from a target counts as agreeing on it, even
    /// when the target sits on a secondary (merged-in) branch. Height-pruned
    /// BFS up the parents — a block at or below the ancestor's height cannot
    /// have it among its strictly-lower parents, so that branch is pruned.
    pub fn is_dag_ancestor_metered<W: WorkMeter>(
        &self,
        meter: &W,
        ancestor: &BlockHash,
        descendant: &BlockHash,
    ) -> Result<bool, KvStoreError> {
        if ancestor == descendant {
            return Ok(true);
        }

        let stop_height = self.block_number_unsafe(ancestor)?;
        let mut visited: HashSet<BlockHash> = HashSet::new();
        let mut queue: VecDeque<BlockHash> = VecDeque::new();
        meter.allocate(2, 128)?;
        visited.insert(descendant.clone());
        queue.push_back(descendant.clone());

        while let Some(current) = queue.pop_front() {
            meter.step(WorkKind::Traversal)?;
            if current == *ancestor {
                return Ok(true);
            }
            if self.block_number_unsafe(&current)? <= stop_height {
                continue;
            }
            for parent in self.lookup_unsafe_metered(meter, &current)?.parents {
                meter.step(WorkKind::Traversal)?;
                meter.allocate(2, 128)?;
                if visited.insert(parent.clone()) {
                    queue.push_back(parent);
                }
            }
        }

        Ok(false)
    }

    pub fn parents_unsafe(&self, block_hash: &BlockHash) -> Result<Vec<BlockHash>, KvStoreError> {
        let metadata = self.lookup_unsafe(block_hash)?;
        Ok(metadata.parents)
    }

    pub fn non_finalized_blocks(&self) -> Result<HashSet<BlockHash>, KvStoreError> {
        in_blocking(|| {
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

                if self.is_finalized(&hash)? {
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
        })
    }

    pub fn descendants(&self, block_hash: &BlockHash) -> Result<HashSet<BlockHash>, KvStoreError> {
        let mut result = HashSet::new();
        let mut current_level = vec![block_hash.clone()];

        while !current_level.is_empty() {
            let mut next_level = Vec::new();

            for hash in &current_level {
                if let Some(children) = self.children(hash)? {
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
        self.try_ancestors(block_hash, |hash| Ok(filter_f(hash)))
    }

    pub fn try_ancestors(
        &self,
        block_hash: BlockHash,
        filter_f: impl Fn(&BlockHash) -> Result<bool, KvStoreError>,
    ) -> Result<HashSet<BlockHash>, KvStoreError> {
        let mut result = HashSet::new();
        let mut current_level = vec![block_hash];

        while !current_level.is_empty() {
            let mut next_level = Vec::new();

            for hash in &current_level {
                let metadata = self.lookup_unsafe(hash)?;

                for parent in &metadata.parents {
                    if filter_f(parent)? && !result.contains(parent) {
                        result.insert(parent.clone());
                        next_level.push(parent.clone());
                    }
                }
            }

            current_level = next_level;
        }

        Ok(result)
    }

    /// `ancestors` for the finalized-ancestry MARKING walk over a possibly
    /// truncated DAG. A parent referenced by a held block whose own metadata
    /// is not held is the restore horizon: everything below it is below the
    /// anchor's floor, hence already settled, so the walk terminates there —
    /// the parent is neither marked nor expanded. Erroring instead (as
    /// `ancestors` does) aborts the LFB adoption and wedges the finalizer
    /// forever while the chain grows past it. Callers for whom a missing
    /// block is an availability failure to surface (merge scope) stay on
    /// `ancestors`.
    pub fn held_ancestors(
        &self,
        block_hash: BlockHash,
        filter_f: impl Fn(&BlockHash) -> Result<bool, KvStoreError>,
    ) -> Result<HashSet<BlockHash>, KvStoreError> {
        let mut result = HashSet::new();
        let mut current_level = vec![self.lookup_unsafe(&block_hash)?];

        while !current_level.is_empty() {
            let mut next_level = Vec::new();

            for metadata in &current_level {
                for parent in &metadata.parents {
                    if filter_f(parent)? && !result.contains(parent) {
                        if let Some(parent_metadata) = self.lookup(parent)? {
                            result.insert(parent.clone());
                            next_level.push(parent_metadata);
                        } else {
                            // Routine on a truncated node while the sweep
                            // first crosses its restore horizon; on a node
                            // holding full history the same skip means the
                            // index lost a block — keep it visible either
                            // way rather than terminating silently.
                            tracing::warn!(
                                parent = %PrettyPrinter::build_string_bytes(parent),
                                child = %PrettyPrinter::build_string_bytes(&metadata.block_hash),
                                "finalization sweep skipped an unheld parent: settled \
                                 ancestry below a restore horizon, or lost data on a \
                                 fully-synced node"
                            );
                        }
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

    pub fn try_with_ancestors(
        &self,
        block_hash: BlockHash,
        filter_f: impl Fn(&BlockHash) -> Result<bool, KvStoreError>,
    ) -> Result<HashSet<BlockHash>, KvStoreError> {
        let mut result = self.try_ancestors(block_hash.clone(), filter_f)?;
        result.insert(block_hash);
        Ok(result)
    }
}

/// P2-14 / Phase 11: every internal index is `pub(crate)`. Cross-crate
/// test fixtures that previously poked at these fields must now go
/// through the `#[cfg(any(test, feature = "test-internals"))]`-gated
/// constructor (`from_parts`) and the matching `metadata_index_for_tests`
/// accessor — see further down this file.
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
    pub(crate) persistence: Option<Arc<DagPersistence>>,
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
    pub(crate) invalid_blocks_index: KeyValueTypedStoreImpl<BlockHashSerde, BlockMetadata>,
    /// Memoized justification-derived floor per block (block hash -> floor hash).
    pub(crate) floor_index: KeyValueTypedStoreImpl<BlockHashSerde, BlockHashSerde>,
    /// Memoized per-block finalized frontier (see `KeyValueDagRepresentation::frontier_index`).
    pub(crate) frontier_index: KeyValueTypedStoreImpl<BlockHashSerde, BlockHashSerde>,
    /// Equivocation tracker — RMW MUST route through
    /// `access_equivocations_tracker` (Bug #2 / T-9.2).
    pub(crate) equivocation_tracker_index: EquivocationTrackerStore,
    /// Deploy-lifecycle tables (see `KeyValueDagRepresentation::lifecycle`).
    pub(crate) lifecycle: Arc<PlRwLock<DeployLifecycleTables>>,
    /// Repeat-deploy carrier index (see `KeyValueDagRepresentation::carrier_index`).
    pub(crate) carrier_index: Arc<PlRwLock<CarrierIndex>>,
    /// Monotonically increasing counter incremented on every successful block insert.
    /// Used by caches to detect when the DAG has changed.
    pub(crate) dag_generation: Arc<AtomicU64>,
    /// Lower bound on `fault_tolerance_value` across `finalized_block_set`,
    /// held as `f32` bits (`f32::to_bits` / `from_bits`) so it fits an atomic.
    /// `propagate_ft_to_finalized_blocks` reads it to skip a scan that could
    /// only raise blocks already at or above `ft_value`.
    ///
    /// The invariant is one-sided: too LOW costs one needless scan, too HIGH
    /// skips a scan that was needed and leaves blocks under-propagated. So
    /// lowering is free and may happen at any time, while raising is only
    /// sound once a scan has actually rewritten every finalized block.
    ///
    /// Both writers hold `global_lock` exclusively — the lowering in
    /// `record_directly_finalized`'s persist closure and the raise at the end
    /// of `propagate_ft_to_finalized_blocks`. That is what makes `Relaxed`
    /// sufficient here; a reader outside the lock would need stronger
    /// ordering, and could observe a stale-high bound.
    pub(crate) ft_lower_bound: Arc<AtomicU32>,
    /// The shard's genesis block hash, persisted as a single-slot register.
    /// On ceremony nodes it is derivable from the DAG (the height-0 block);
    /// a truncated (LFS-restored) node holds no height-0 block and must
    /// LEARN it — hash only, never the block — during restore. Consumers
    /// that need a network-uniform genesis sentinel read this register.
    pub(crate) genesis_hash_index: KeyValueTypedStoreImpl<String, BlockHashSerde>,
    /// Receives the hash of every block this storage inserts, in every mode,
    /// once the block is visible in the DAG.
    insert_observer: Arc<OnceLock<mpsc::UnboundedSender<BlockHash>>>,
}

impl BlockDagKeyValueStorage {
    /// Report every block inserted from now on to `observer`. A storage has
    /// one observer for its lifetime.
    pub fn set_insert_observer(
        &self,
        observer: mpsc::UnboundedSender<BlockHash>,
    ) -> Result<(), KvStoreError> {
        self.insert_observer.set(observer).map_err(|_| {
            KvStoreError::InvalidArgument(
                "the DAG storage already has an insert observer".to_string(),
            )
        })
    }

    /// The block is in the DAG, whether or not it is still resident.
    pub fn contains(&self, hash: &BlockHash) -> Result<bool, KvStoreError> {
        match &self.persistence {
            Some(persistence) => Ok(persistence.history.membership(hash, u64::MAX)?.is_some()),
            None => Ok(self.block_metadata_index.read().contains(hash)),
        }
    }

    /// A block that depends on `hash` need not wait for it: the block is in
    /// the DAG, or it was detected as an equivocation.
    pub fn is_dependency_satisfied(&self, hash: &BlockHash) -> Result<bool, KvStoreError> {
        Ok(self
            .unsatisfied_dependencies(std::slice::from_ref(hash))?
            .is_empty())
    }

    /// The hashes in `hashes` that a dependent block must still wait for:
    /// neither in the DAG nor detected as an equivocation. The equivocation
    /// records are read once for the whole list.
    pub fn unsatisfied_dependencies(
        &self,
        hashes: &[BlockHash],
    ) -> Result<Vec<BlockHash>, KvStoreError> {
        let mut absent = Vec::new();
        for hash in hashes {
            if !self.contains(hash)? {
                absent.push(hash.clone());
            }
        }
        if absent.is_empty() {
            return Ok(absent);
        }
        self.access_equivocations_tracker(|tracker| {
            let records = tracker.data()?;
            Ok(absent
                .into_iter()
                .filter(|hash| {
                    !records
                        .iter()
                        .any(|record| record.equivocation_detected_block_hashes.contains(hash))
                })
                .collect())
        })
    }

    pub fn lookup(&self, hash: &BlockHash) -> Result<Option<BlockMetadata>, KvStoreError> {
        self.block_metadata_index.read().get(hash)
    }

    pub fn latest_message_hash(
        &self,
        validator: &Validator,
    ) -> Result<Option<BlockHash>, KvStoreError> {
        Ok(self
            .latest_messages_index
            .get_one(&ValidatorSerde(validator.clone()))?
            .map(BlockHash::from))
    }

    /// `None` until the approved block is inserted.
    pub fn last_finalized_block(&self) -> Option<BlockHash> {
        let guard = self.block_metadata_index.read();
        let dag_state = guard.dag_state().read();
        dag_state
            .last_finalized_block
            .as_ref()
            .map(|(hash, _)| hash.clone())
    }

    /// Storage-level twin of `KeyValueDagRepresentation::deploy_canonical_appearance`
    /// (same shared lifecycle tables), for callers holding the storage rather
    /// than a representation. Lock order: `block_metadata_index` before
    /// `lifecycle` (the DAG-visibility filter reads the metadata state).
    pub fn deploy_canonical_appearance(
        &self,
        sig: &[u8],
    ) -> Result<Option<BlockHash>, KvStoreError> {
        let dag = self.get_representation()?;
        Ok(self
            .lifecycle
            .read()
            .canonical_appearance(sig, &|h| dag.contains(&BlockHash::copy_from_slice(h)))?
            .map(BlockHash::from))
    }

    /// Startup carrier-index initialization (next to the LFB-migration
    /// precedent). On a database that already holds blocks, coverage starts at
    /// the next insert: max height + 1. Blocks below W are never claimed — the
    /// fast path engages only for scan windows starting at or above W — so no
    /// backfill walk exists and there is no walk-completeness state to certify
    /// or to forge.
    ///
    /// An EMPTY database is left unwatermarked: at startup the node cannot yet
    /// know whether it will root its history at genesis or restore at an
    /// anchor, and W = 0 would claim coverage over history a restore never
    /// downloads. [`Self::record_carrier_coverage_from`] writes it once that is
    /// known. Until then the fast path stays disabled, which is merely slower.
    pub fn ensure_carrier_watermark(&self) -> Result<Option<i64>, KvStoreError> {
        let _lock_guard = self.global_lock.write();
        let next_height = {
            let metadata_guard = self.block_metadata_index.read();
            let dag_state_guard = metadata_guard.dag_state().read();
            if self.persistence.is_some() {
                dag_state_guard.max_height.map(|h| h.saturating_add(1))
            } else {
                dag_state_guard.height_map.get_max().map(|(h, _)| h + 1)
            }
        };
        match next_height {
            Some(height) => self
                .carrier_index
                .read()
                .set_watermark_if_absent(height)
                .map(Some),
            None => self.carrier_index.read().watermark(),
        }
    }

    /// Record the height from which this node's carrier index is complete,
    /// called once the history root is established: genesis for a ceremony
    /// node, the lowest height the restore actually indexed for an LFS joiner.
    /// Write-once, so a restart cannot lower it.
    pub fn record_carrier_coverage_from(&self, lowest_held: i64) -> Result<i64, KvStoreError> {
        let _lock_guard = self.global_lock.write();
        self.carrier_index
            .read()
            .set_watermark_if_absent(lowest_held)
    }

    /// Test-only corruption helper (P2-16-style escape hatch): deletes the
    /// PERSISTED metadata row for a block while the in-memory DAG state
    /// still lists it, simulating a crash between the lifecycle ingest and
    /// the metadata add.
    #[cfg(any(test, feature = "test-internals"))]
    pub fn remove_block_metadata_row_for_tests(
        &self,
        hash: &BlockHash,
    ) -> Result<(), KvStoreError> {
        let metadata_guard = self.block_metadata_index.read();
        metadata_guard.delete_kv_row_for_tests(hash)
    }

    pub async fn new(kvm: &mut (impl KeyValueStoreManager + ?Sized)) -> Result<Self, KvStoreError> {
        Self::new_with_hot_window(kvm, 1024).await
    }

    pub async fn new_with_hot_window(
        kvm: &mut (impl KeyValueStoreManager + ?Sized),
        hot_window: i64,
    ) -> Result<Self, KvStoreError> {
        let block_metadata_kv_store = kvm.store("block-metadata".to_string()).await?;
        let invalid_blocks_kv_store = kvm.store("invalid-blocks".to_string()).await?;
        let block_metadata_db: KeyValueTypedStoreImpl<BlockHashSerde, BlockMetadata> =
            KeyValueTypedStoreImpl::new(block_metadata_kv_store.clone());

        let equivocation_tracker_kv_store = kvm.store("equivocation-tracker".to_string()).await?;
        let equivocation_tracker_db: KeyValueTypedStoreImpl<
            (ValidatorSerde, SequenceNumber),
            BTreeSet<BlockHashSerde>,
        > = KeyValueTypedStoreImpl::new(equivocation_tracker_kv_store);
        let equivocation_tracker_store = EquivocationTrackerStore::new(equivocation_tracker_db);

        let latest_messages_kv_store = kvm.store("latest-messages".to_string()).await?;
        let latest_messages_db: KeyValueTypedStoreImpl<ValidatorSerde, BlockHashSerde> =
            KeyValueTypedStoreImpl::new(latest_messages_kv_store.clone());

        let invalid_blocks_db: KeyValueTypedStoreImpl<BlockHashSerde, BlockMetadata> =
            KeyValueTypedStoreImpl::new(invalid_blocks_kv_store.clone());

        let floor_index_kv_store = kvm.store("floor-index".to_string()).await?;
        let floor_index_db: KeyValueTypedStoreImpl<BlockHashSerde, BlockHashSerde> =
            KeyValueTypedStoreImpl::new(floor_index_kv_store);
        let frontier_index_kv_store = kvm.store("frontier-index".to_string()).await?;
        let frontier_index_db: KeyValueTypedStoreImpl<BlockHashSerde, BlockHashSerde> =
            KeyValueTypedStoreImpl::new(frontier_index_kv_store);
        let lifecycle_events_kv_store = kvm.store("deploy-lifecycle-events".to_string()).await?;
        let lifecycle_terminal_kv_store =
            kvm.store("deploy-lifecycle-terminal".to_string()).await?;
        let carrier_index_kv_store = kvm.store("carrier-index".to_string()).await?;
        let carrier_index_meta_kv_store = kvm.store("carrier-index-meta".to_string()).await?;
        let carrier_index_tables =
            CarrierIndex::new(carrier_index_kv_store, carrier_index_meta_kv_store);
        let lifecycle_tables =
            DeployLifecycleTables::new(lifecycle_events_kv_store, lifecycle_terminal_kv_store);

        let genesis_hash_kv_store = kvm.store("genesis-hash".to_string()).await?;
        let genesis_hash_db: KeyValueTypedStoreImpl<String, BlockHashSerde> =
            KeyValueTypedStoreImpl::new(genesis_hash_kv_store.clone());

        let mut atomic_stores = vec![
            block_metadata_kv_store,
            latest_messages_kv_store,
            invalid_blocks_kv_store,
            genesis_hash_kv_store,
        ];
        for name in [
            "dag-membership-v1",
            "dag-height-v1",
            "dag-children-v1",
            "dag-history-state-v1",
            "dag-gc-retry-v1",
        ] {
            atomic_stores.push(kvm.store(name.to_string()).await?);
        }
        let membership_store = atomic_stores[4].clone();
        let persistence = Arc::new(DagPersistence::new(
            Arc::new(AtomicStoreGroup::new(atomic_stores)?),
            membership_store,
        ));
        let block_metadata_store =
            BlockMetadataStore::open_windowed(block_metadata_db, persistence.clone(), hot_window)?;

        Ok(Self {
            persistence: Some(persistence),
            global_lock: Arc::new(PlRwLock::new(())),
            block_metadata_index: Arc::new(PlRwLock::new(block_metadata_store)),
            invalid_blocks_index: invalid_blocks_db,
            floor_index: floor_index_db,
            frontier_index: frontier_index_db,
            equivocation_tracker_index: equivocation_tracker_store,
            lifecycle: Arc::new(PlRwLock::new(lifecycle_tables)),
            carrier_index: Arc::new(PlRwLock::new(carrier_index_tables)),
            latest_messages_index: latest_messages_db,
            dag_generation: Arc::new(AtomicU64::new(0)),
            ft_lower_bound: Arc::new(AtomicU32::new(0.0f32.to_bits())),
            genesis_hash_index: genesis_hash_db,
            insert_observer: Arc::new(OnceLock::new()),
        })
    }

    const GENESIS_HASH_KEY: &'static str = "genesis";

    pub fn gc_store(&self) -> Arc<DagGcStore> {
        self.persistence
            .as_ref()
            .expect("GC requires transactional DAG storage")
            .gc
            .clone()
    }

    /// Record the shard's genesis block hash. Write-once: recording the same
    /// value again is a no-op; recording a DIFFERENT value is an error — two
    /// genesis identities on one node is a bootstrap-integrity violation,
    /// never something to silently overwrite.
    pub fn record_genesis_hash(&self, hash: BlockHash) -> Result<(), KvStoreError> {
        let _lock_guard = self.global_lock.write();
        let key = Self::GENESIS_HASH_KEY.to_string();
        if let Some(BlockHashSerde(existing)) = self.genesis_hash_index.get_one(&key)? {
            if existing == hash {
                return Ok(());
            }
            return Err(KvStoreError::InvalidArgument(format!(
                "genesis hash already recorded as {}; refusing to overwrite with {}",
                PrettyPrinter::build_string_bytes(&existing),
                PrettyPrinter::build_string_bytes(&hash),
            )));
        }
        self.genesis_hash_index.put_one(key, BlockHashSerde(hash))
    }

    /// The shard's genesis hash: the learned register when present, else
    /// derived from the held height-0 block (ceremony nodes). `None` only on
    /// a truncated node that has not learned it.
    pub fn genesis_hash(&self) -> Result<Option<BlockHash>, KvStoreError> {
        if let Some(BlockHashSerde(hash)) = self
            .genesis_hash_index
            .get_one(&Self::GENESIS_HASH_KEY.to_string())?
        {
            return Ok(Some(hash));
        }
        if let Some(persistence) = &self.persistence {
            let mut after = None;
            let mut minimum = None;
            loop {
                let page = persistence.history.scan_height_range(
                    0,
                    0,
                    u64::MAX,
                    true,
                    after.as_deref(),
                    1024,
                )?;
                for hash in page.rows {
                    if minimum.as_ref().is_none_or(|old| hash < old) {
                        minimum = Some(hash);
                    }
                }
                after = page.next;
                if after.is_none() {
                    return Ok(minimum);
                }
            }
        }
        let guard = self.block_metadata_index.read();
        let dag_state = guard.dag_state().read();
        Ok(dag_state
            .height_map
            .get(&0)
            .and_then(|blocks| blocks.iter().min().cloned()))
    }

    // P2-16: the following two methods bypass `global_lock` — production
    // code MUST route through `access_equivocations_tracker` to honor the
    // Bug #2 / T-9.2 atomicity contract (see
    // `docs/casper/theory/slashing/slashing-verification.md` §9.2 and
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
        invalid_blocks_index: KeyValueTypedStoreImpl<BlockHashSerde, BlockMetadata>,
        floor_index: KeyValueTypedStoreImpl<BlockHashSerde, BlockHashSerde>,
        frontier_index: KeyValueTypedStoreImpl<BlockHashSerde, BlockHashSerde>,
        equivocation_tracker_index: EquivocationTrackerStore,
        dag_generation: Arc<AtomicU64>,
    ) -> Self {
        Self {
            persistence: None,
            global_lock,
            latest_messages_index,
            block_metadata_index,
            invalid_blocks_index,
            floor_index,
            frontier_index,
            equivocation_tracker_index,
            lifecycle: Arc::new(PlRwLock::new(DeployLifecycleTables::in_memory())),
            carrier_index: Arc::new(PlRwLock::new(CarrierIndex::in_memory())),
            dag_generation,
            ft_lower_bound: Arc::new(AtomicU32::new(0.0f32.to_bits())),
            genesis_hash_index: KeyValueTypedStoreImpl::new(Arc::new(
                rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore::new(),
            )),
            insert_observer: Arc::new(OnceLock::new()),
        }
    }

    /// Test-only accessor for the block-metadata-index handle: tests in
    /// other crates that need to inject entries enable the
    /// `test-internals` feature to reach this accessor.
    #[cfg(any(test, feature = "test-internals"))]
    #[doc(hidden)]
    pub fn metadata_index_for_tests(&self) -> Arc<PlRwLock<BlockMetadataStore>> {
        self.block_metadata_index.clone()
    }

    /// Test-only accessor for the floor-index handle. Same rationale as
    /// `metadata_index_for_tests`. `floor_index` is a plain memoization store
    /// (block hash -> floor hash) with interior mutability, so a shared
    /// reference suffices for the round-trip test.
    #[cfg(any(test, feature = "test-internals"))]
    #[doc(hidden)]
    pub fn floor_index_for_tests(&self) -> &KeyValueTypedStoreImpl<BlockHashSerde, BlockHashSerde> {
        &self.floor_index
    }

    /// Test-only accessor for the frontier-index handle. Same rationale as
    /// `floor_index_for_tests`.
    #[cfg(any(test, feature = "test-internals"))]
    #[doc(hidden)]
    pub fn frontier_index_for_tests(
        &self,
    ) -> &KeyValueTypedStoreImpl<BlockHashSerde, BlockHashSerde> {
        &self.frontier_index
    }

    /// Current DAG generation — incremented on every block insert.
    /// Can be used by caches to detect whether the DAG has changed since the last snapshot.
    pub fn current_generation(&self) -> u64 { self.dag_generation.load(Ordering::Relaxed) }

    pub(crate) fn soak_capture_access(
        &self,
        deadline: std::time::Instant,
        wait: std::time::Duration,
    ) -> Result<SoakCaptureAccess<'_>, SnapshotError> {
        let global = self
            .global_lock
            .try_read_until(deadline)
            .ok_or(SnapshotError::LockTimeout(wait))?;
        let metadata = self
            .block_metadata_index
            .try_read_until(deadline)
            .ok_or(SnapshotError::LockTimeout(wait))?;
        Ok(SoakCaptureAccess {
            _global: global,
            metadata,
            storage: self,
        })
    }

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
        self.capture_representation(false)
    }

    fn capture_representation(
        &self,
        allow_uninitialized: bool,
    ) -> Result<KeyValueDagRepresentation, KvStoreError> {
        let latest_messages: imbl::HashMap<Validator, BlockHash> = self
            .latest_messages_index
            .to_map()?
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();

        let invalid_blocks: imbl::HashSet<BlockMetadata> =
            self.invalid_blocks_index.to_map()?.into_values().collect();

        self.block_metadata_index.write().refresh_window()?;

        let block_metadata_index_guard = self.block_metadata_index.read();
        let dag_state_guard = block_metadata_index_guard.dag_state().read();
        let dag_set = dag_state_guard.dag_set.clone();
        let child_map = dag_state_guard.child_map.clone();
        let height_map = dag_state_guard.height_map.clone();
        let block_number_map = dag_state_guard.block_number_map.clone();
        let main_parent_map = dag_state_guard.main_parent_map.clone();
        let self_justification_map = dag_state_guard.self_justification_map.clone();
        let last_finalized_block = match &dag_state_guard.last_finalized_block {
            Some((hash, _)) => hash.clone(),
            None if allow_uninitialized => BlockHash::new(),
            None => return Err(KvStoreError::LastFinalizedBlockUninitialized),
        };
        let finalized_blocks = dag_state_guard.finalized_block_set.clone();
        let view_sequence = dag_state_guard.sequence;
        let view_max_height = dag_state_guard.max_height;
        metrics::gauge!("dag.held.blocks").set(dag_state_guard.total as f64);
        metrics::gauge!("dag.resident.blocks").set(dag_state_guard.dag_set.len() as f64);
        metrics::gauge!("dag.resident.cutoff").set(dag_state_guard.cutoff as f64);
        metrics::gauge!("dag.resident.child-keys").set(dag_state_guard.child_map.len() as f64);
        metrics::gauge!("dag.resident.edges").set(dag_state_guard.edges as f64);
        metrics::gauge!("dag.lfb.height").set(
            dag_state_guard
                .last_finalized_block
                .as_ref()
                .map_or(0, |(_, height)| *height) as f64,
        );
        metrics::gauge!("dag.max.height").set(dag_state_guard.max_height.unwrap_or(0) as f64);

        drop(dag_state_guard);
        drop(block_metadata_index_guard);

        Ok(KeyValueDagRepresentation {
            history: self.persistence.as_ref().map(|p| p.history.clone()),
            view_sequence,
            view_max_height,
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
            floor_index: self.floor_index.clone(),
            frontier_index: self.frontier_index.clone(),
            lifecycle: self.lifecycle.clone(),
            carrier_index: self.carrier_index.clone(),
        })
    }

    pub fn insert(
        &self,
        block: &BlockMessage,
        mode: InsertMode,
    ) -> Result<KeyValueDagRepresentation, KvStoreError> {
        // P2-12: insert mutates state; acquire exclusive write lock.
        // The `dag.insert.time` histogram instruments the post-lock critical
        // section so percentile graphs reflect actual contention + work, not
        // wait time queuing for the lock (which is captured by the LMDB
        // dashboard's lock-contention metric).
        let __insert_start = std::time::Instant::now();
        let _lock_guard = self.global_lock.write();
        let result = self.insert_internal(block, mode);
        metrics::histogram!("dag.insert.time", "source" => "f1r3fly.casper.block-dag")
            .record(__insert_start.elapsed().as_secs_f64());
        result
    }

    /// Internal method to insert without acquiring lock.
    /// Used when lock is already held by the caller.
    /// Public to allow IndexedBlockDagStorage to use it.
    pub(crate) fn insert_internal(
        &self,
        block: &BlockMessage,
        mode: InsertMode,
    ) -> Result<KeyValueDagRepresentation, KvStoreError> {
        // Phase 8 (A-6): derive the per-branch booleans directly from the
        // enum via `matches!`. The previous `mode.flags()` projection
        // shim survived a Phase-4 transition; it is no longer needed.
        let invalid = matches!(mode, InsertMode::Invalid | InsertMode::SlashEvidence);
        let approved = matches!(mode, InsertMode::Approved);
        let settled_history =
            matches!(mode, InsertMode::SettledHistory | InsertMode::SlashEvidence);
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

        // Latest-message updates are NOT gated on `invalid`. Equivocation blocks
        // (and other invalid blocks) advance the sender's latest message and
        // register newly-bonded validators just like valid blocks.
        //
        // Safety argument:
        //   - Fork choice and finalization are unaffected. Parent selection filters
        //     `latest_messages` through `invalid_latest_messages_from_hashes` to
        //     produce `valid_latest_msgs` (see
        //     `engine::multi_parent_casper::create_block_data`, ~line 160). Only
        //     valid-latest validators contribute candidate parents; invalid blocks
        //     therefore cannot become parents, cannot enter the ancestor chain of
        //     any parent, and cannot influence the Estimator's fork-choice scoring
        //     or finalization depth.
        //   - Slashing requires invalid blocks to BE in the LMM. The equivocation
        //     detector reads `invalid_latest_messages` and feeds it to
        //     `prepare_slashing_deploys`. Gating the LMM on `invalid` would leave
        //     the slashing pipeline with nothing to act on — no slashes issued,
        //     equivocators never punished.
        //   - `justification_follows` validation requires every bonded validator
        //     to appear in a new block's justifications. Without the LMM advancing
        //     on invalid blocks, validators whose latest is invalid would be
        //     missing from the creator's view and `justification_follows` would
        //     reject otherwise-valid blocks.
        //
        // Companion sites that depend on this invariant:
        //   - `engine::multi_parent_casper::create_block_data` (justifications
        //     and max_seq_nums both read the unfiltered `latest_msgs_hashes`).
        //   - The
        //     `dag_storage_should_advance_latest_message_to_invalid_block_from_same_sender`
        //     test in `block-storage/tests/block_dag_storage_test.rs` exercises
        //     this directly.
        let new_latest_messages = || -> Result<HashMap<Validator, BlockHash>, KvStoreError> {
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

            let mut newly_bonded_unseen = Vec::new();
            for validator in newly_bonded_set.difference(&justification_validators) {
                if !self
                    .latest_messages_index
                    .contains_key(ValidatorSerde((*validator).clone()))?
                {
                    newly_bonded_unseen.push((*validator).clone());
                }
            }

            let mut result = HashMap::new();
            if !newly_bonded_unseen.is_empty() {
                // The placeholder must be NETWORK-UNIFORM: every node seeds
                // the same slot with the same value, or the joiner's first
                // self-justifying proposal reads as an equivocation on
                // whichever side seeded differently. Ceremony nodes derive
                // genesis from their height-0 block; a truncated node holds
                // none and uses the genesis hash it learned during restore.
                // There is no third source — the block being inserted is
                // right on no node.
                let placeholder = self
                    .genesis_hash()?
                    .or_else(|| {
                        (!invalid && block.body.state.block_number == 0).then(|| block_hash.clone())
                    })
                    .ok_or_else(|| {
                        KvStoreError::InvalidArgument(format!(
                            "cannot seed newly-bonded latest-message slot(s) while inserting {}: \
                         no height-0 block is held and no genesis hash was learned",
                            PrettyPrinter::build_string_bytes(&block_hash),
                        ))
                    })?;

                for validator in newly_bonded_unseen {
                    tracing::debug!(
                        target: "f1r3.trace.lm_register",
                        via = "newly_bonded",
                        validator = %PrettyPrinter::build_string_bytes(&validator),
                        registered_block = %PrettyPrinter::build_string_bytes(&placeholder),
                        inserting_sender = %PrettyPrinter::build_string_bytes(&block.sender),
                        inserting_seq = block.seq_num,
                        "newly bonded validator latest-message slot registered"
                    );
                    result.insert(validator, placeholder.clone());
                }
            }

            Ok(result)
        };

        let block_exists = if let Some(persistence) = &self.persistence {
            persistence
                .history
                .membership(&block.block_hash, u64::MAX)?
                .is_some()
        } else {
            let block_metadata_index_guard = self.block_metadata_index.read();

            block_metadata_index_guard.contains(&block.block_hash)
        };

        if block_exists {
            tracing::warn!("{}", log_already_stored);
            if approved {
                if let Some(persistence) = &self.persistence {
                    let metadata = self
                        .block_metadata_index
                        .read()
                        .get_unsafe(&block.block_hash)?;
                    let (metadata, progress, _) =
                        persistence.publish(&metadata, &HashMap::new(), true)?;
                    self.block_metadata_index
                        .write()
                        .publish_committed(&metadata, &progress)?;
                }
            }
        } else {
            let block_hash = block.block_hash.clone();
            let block_hash_is_invalid = !(block_hash.len() == block_hash::LENGTH);

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

            // Carrier-index ingest: every body sig — valid, invalid, and
            // settled alike — is recorded BEFORE the metadata-index add, so
            // the completeness invariant behind the repeat-deploy fast path
            // holds at every crash point: a crash here leaves orphan
            // carrier entries (harmless — a hit just routes to the exact
            // scan), never a DAG-visible block with unindexed sigs. Every
            // insert path (validated, proposed, genesis, LFS, fixtures)
            // flows through here, so coverage is total by construction
            // from the watermark onward. `body.deploys` sigs are
            // decode-verified (`Signed::from_signed_data`); the unverified
            // `rejected_deploys` sigs are deliberately NOT recorded — the
            // ancestor scan never reads them, and the carrier index must
            // not ingest attacker-chosen keys.
            {
                let block_number = block.body.state.block_number;
                let carrier_guard = self.carrier_index.write();
                for pd in &block.body.deploys {
                    carrier_guard.record_once(
                        &pd.deploy.sig,
                        block_number,
                        block.block_hash.to_vec(),
                    )?;
                }
            }

            // Lifecycle event ingest: one body pass projects inclusion and
            // rejection events into the per-sig lifecycle rows. Invalid
            // blocks contribute nothing — their bodies are not canonical
            // history (the carrier index above covers the repeat-deploy
            // scan's view of them). Appends go through `append_event_once`:
            // after a crash inside this ingest-first window the block is
            // not yet DAG-visible, so its redelivery re-runs this pass —
            // the per-(block, kind) dedup makes that retry write nothing
            // twice.
            {
                let block_number = block.body.state.block_number;
                let lifecycle_guard = self.lifecycle.write();
                if !invalid {
                    for pd in &block.body.deploys {
                        lifecycle_guard.append_event_once(
                            &pd.deploy.sig,
                            Some(pd.deploy.data.valid_after_block_number),
                            LifecycleEvent {
                                height: block_number,
                                block_hash: block.block_hash.to_vec(),
                                kind: LifecycleEventKind::Included {
                                    is_failed: pd.is_failed,
                                },
                            },
                        )?;
                    }
                    for rd in &block.body.rejected_deploys {
                        lifecycle_guard.append_event_once(&rd.sig, None, LifecycleEvent {
                            height: block_number,
                            block_hash: block.block_hash.to_vec(),
                            kind: LifecycleEventKind::Rejected {
                                duplicate: rd.duplicate,
                            },
                        })?;
                    }
                }
                drop(lifecycle_guard);
            }

            let block_metadata = BlockMetadata::from_block(block, invalid, None, None);
            let mut latest_to_publish = HashMap::new();

            // Settled-history blocks never touch latest messages: neither the
            // sender advance below, nor the newly-bonded seeding above —
            // a sub-anchor block's bond set is stale testimony.
            if !settled_history {
                let new_latest_from_sender = if !sender_is_empty {
                    // Add LM either if there is no existing message for the sender, or if sequence number advances
                    // - assumes block sender is not valid hash
                    if match self
                        .latest_messages_index
                        .get_one(&block.sender.clone().into())?
                    {
                        Some(latest_message_hash) => {
                            let latest_hash = latest_message_hash.0;
                            let metadata = self.block_metadata_index.read().get(&latest_hash)?;
                            if let Some(persistence) = &self.persistence {
                                match persistence.history.membership(&latest_hash, u64::MAX)? {
                                    Some(member) => {
                                        let row = metadata.as_ref().ok_or_else(|| {
                                            KvStoreError::KeyNotFound(
                                                "latest message metadata missing".into(),
                                            )
                                        })?;
                                        if row.block_hash != latest_hash
                                            || row.block_number != member.height
                                            || row.invalid != member.invalid
                                        {
                                            return Err(KvStoreError::InvalidArgument(
                                                "latest message metadata disagrees with membership"
                                                    .into(),
                                            ));
                                        }
                                    }
                                    None if self.genesis_hash()?.as_ref() == Some(&latest_hash)
                                        && metadata.is_none() => {}
                                    None => {
                                        return Err(KvStoreError::KeyNotFound(
                                            "latest message publication missing".into(),
                                        ))
                                    }
                                }
                            }
                            match metadata {
                                Some(metadata) => block.seq_num >= metadata.sequence_number,
                                None => true,
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
                latest_to_publish = new_latest_to_add;
            }
            if let Some(persistence) = &self.persistence {
                let (metadata, progress, _) =
                    persistence.publish(&block_metadata, &latest_to_publish, approved)?;
                self.dag_generation.fetch_add(1, Ordering::Relaxed);
                self.block_metadata_index
                    .write()
                    .publish_committed(&metadata, &progress)?;
            } else {
                self.block_metadata_index
                    .write()
                    .add(block_metadata.clone())?;
                if invalid {
                    self.invalid_blocks_index
                        .put_one(block_hash.clone().into(), block_metadata)?;
                }
                self.latest_messages_index.put(
                    latest_to_publish
                        .into_iter()
                        .map(|(k, v)| (k.into(), v.into()))
                        .collect(),
                )?;
            }
            if self.persistence.is_none() {
                self.dag_generation.fetch_add(1, Ordering::Relaxed);
            }
            if let Some(observer) = self.insert_observer.get() {
                // A closed receiver means the observer has stopped; inserts go on.
                let _ = observer.send(block_hash);
            }
        }

        // Shared by both paths on purpose: the mark is the only thing an approved
        // insert adds over an ordinary one, and it went missing precisely because
        // it lived on one branch while the other returned early. Genesis or a
        // restore anchor has FT=1.0 by construction — it is the root every
        // validator starts from, so all stake agrees.
        if approved && self.persistence.is_none() {
            let mut block_metadata_guard = self.block_metadata_index.write();
            block_metadata_guard.record_finalized(block.block_hash.clone(), HashSet::new(), 1.0)?;
        }

        self.capture_representation(true)
    }

    pub fn access_equivocations_tracker<A>(
        &self,
        f: impl FnOnce(&EquivocationTrackerStore) -> Result<A, KvStoreError>,
    ) -> Result<A, KvStoreError> {
        // Exclusive access only, not a transaction: no concurrent reader or
        // writer interleaves with the closure, but writes it makes before
        // returning `Err` persist.
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

                // These blocks enter at `ft_value`, possibly below what earlier
                // rounds propagated — the bound has to follow them down. Lowered
                // BEFORE the fallible write, not after: `record_finalized` adds to
                // `finalized_block_set` before it persists, so an error on the way
                // out would leave those blocks in the set with the bound still
                // high, and every later propagate at or under it would skip the
                // scan that raises them. Lowering early can only cost one
                // unnecessary scan.
                let _ = self.ft_lower_bound.fetch_update(
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                    |bits| (ft_value < f32::from_bits(bits)).then(|| ft_value.to_bits()),
                );

                let mut block_metadata_index_guard = self.block_metadata_index.write();
                block_metadata_index_guard.record_finalized(
                    directly_finalized_hash.clone(),
                    indirectly_finalized,
                    ft_value,
                )?;
                drop(block_metadata_index_guard);
                Ok(())
            };

        let mut effect_applied: HashSet<BlockHash> = HashSet::new();
        for _attempt in 0..MAX_FINALIZATION_RECONCILE_LOOPS {
            let pending_effect: HashSet<BlockHash> = {
                // P2-12: snapshot read; shared lock allows concurrent readers.
                let _lock_guard = self.global_lock.read();

                let dag = self.get_representation_internal()?;
                if !dag.contains(&directly_finalized_hash)? {
                    return Err(KvStoreError::InvalidArgument(format!(
                        "Attempting to finalize nonexistent hash {}",
                        PrettyPrinter::build_string_bytes(&directly_finalized_hash)
                    )));
                }

                // Held, unfinalized ancestry only. A parent edge can reach
                // BELOW a restored node's truncation horizon — referenced but
                // not held — and such an ancestor is settled by the restore
                // contract (everything under the shipped window is finalized
                // ancestry). Descending into it errored the whole finalizer
                // run on the first sub-horizon parent, permanently: the same
                // ancestry re-walks every run, so a restored node's LFB froze
                // at its restore-era floor while the shard finalized on.
                let indirectly_finalized = dag
                    .held_ancestors(directly_finalized_hash.clone(), |hash| {
                        Ok(!dag.is_finalized(hash)?)
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
        in_blocking(|| {
            // P2-12: mutates `block_metadata_index`; exclusive lock.
            let _lock_guard = self.global_lock.write();

            // Nothing is below the bound and this scan only raises, so it would
            // rewrite nothing. Skipping keeps `global_lock` off an O(finalized) walk.
            if ft_value <= f32::from_bits(self.ft_lower_bound.load(Ordering::Relaxed)) {
                metrics::counter!("propagate_ft.skipped", "source" => "f1r3fly.casper.block-dag")
                    .increment(1);
                return Ok(());
            }

            // Update ALL finalized blocks with lower FT, not just ancestors of the
            // current LFB. In a multi-parent DAG, finalized blocks on orphaned
            // branches are not reachable via the ancestor chain of the new LFB.
            let scan_started = std::time::Instant::now();
            let mut block_metadata_index_guard = self.block_metadata_index.write();
            let finalized_hashes = block_metadata_index_guard.finalized_block_hashes();
            let scanned = finalized_hashes.len();
            block_metadata_index_guard.update_ft_if_higher(finalized_hashes, ft_value)?;
            drop(block_metadata_index_guard);
            metrics::histogram!("propagate_ft.scan.time", "source" => "f1r3fly.casper.block-dag")
                .record(scan_started.elapsed().as_secs_f64());
            metrics::histogram!("propagate_ft.scan.blocks", "source" => "f1r3fly.casper.block-dag")
                .record(scanned as f64);

            // Every finalized block is now at or above `ft_value`.
            self.ft_lower_bound
                .store(ft_value.to_bits(), Ordering::Relaxed);
            Ok(())
        })
    }
}

pub(crate) struct SoakCaptureAccess<'a> {
    _global: parking_lot::RwLockReadGuard<'a, ()>,
    metadata: parking_lot::RwLockReadGuard<'a, BlockMetadataStore>,
    storage: &'a BlockDagKeyValueStorage,
}

impl SoakCaptureAccess<'_> {
    pub(crate) fn membership_index(
        &self,
    ) -> Option<&Arc<dyn shared::rust::store::key_value_store::KeyValueStore>> {
        self.storage
            .persistence
            .as_ref()
            .map(|p| &p.membership_store)
    }
    pub(crate) fn metadata_store(&self) -> &BlockMetadataStore { &self.metadata }

    pub(crate) fn latest_messages_index(
        &self,
    ) -> &KeyValueTypedStoreImpl<ValidatorSerde, BlockHashSerde> {
        &self.storage.latest_messages_index
    }

    pub(crate) fn invalid_blocks_index(
        &self,
    ) -> &KeyValueTypedStoreImpl<BlockHashSerde, BlockMetadata> {
        &self.storage.invalid_blocks_index
    }

    pub(crate) fn floor_index(&self) -> &KeyValueTypedStoreImpl<BlockHashSerde, BlockHashSerde> {
        &self.storage.floor_index
    }

    pub(crate) fn frontier_index(&self) -> &KeyValueTypedStoreImpl<BlockHashSerde, BlockHashSerde> {
        &self.storage.frontier_index
    }

    pub(crate) fn equivocations_index(&self) -> &EquivocationTrackerStore {
        &self.storage.equivocation_tracker_index
    }

    pub(crate) fn generation(&self) -> u64 { self.storage.current_generation() }
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

/// A block the DAG does not hold, named so a caller can request it.
///
/// These lookups assume the block is already in the DAG, and for a node whose
/// history reaches genesis a miss is a caller bug. For a node restored from a
/// sync anchor it is the normal condition — its history stops at the anchor —
/// so the hash travels as data rather than inside a message: a walk that cannot
/// read this node's own history must never become a verdict against whoever
/// proposed the block it was judging.
///
/// `method` and the backtrace stay in `context` because the same absence is
/// reachable from three methods and many call sites, and in a live shard the
/// error otherwise surfaces as "block processing failed" with no indication of
/// WHICH lookup asked — not enough to tell a gated dependency from an ancestor
/// walk that was never gated at all (ucc runs: 7-12 occurrences per run, every
/// run, escalating into propose failures). Captured only on the error path, and
/// with `force_capture` so it does not depend on RUST_BACKTRACE being set in the
/// shard's environment.
fn missing_block(block_hash: &BlockHash, method: &'static str) -> KvStoreError {
    KvStoreError::MissingBlock {
        hash: block_hash.clone(),
        context: MissingBlockContext::with_backtrace(method),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use rspace_plus_plus::rspace::shared::lmdb_dir_store_manager::{
        Db, LmdbDirStoreManager, LmdbEnvConfig,
    };

    use super::*;

    const DAG_STORES: [&str; 16] = [
        "block-metadata",
        "equivocation-tracker",
        "latest-messages",
        "invalid-blocks",
        "floor-index",
        "frontier-index",
        "deploy-lifecycle-events",
        "deploy-lifecycle-terminal",
        "carrier-index",
        "carrier-index-meta",
        "genesis-hash",
        "dag-membership-v1",
        "dag-height-v1",
        "dag-children-v1",
        "dag-history-state-v1",
        "dag-gc-retry-v1",
    ];

    #[test]
    fn ft_propagation_scan_hands_off_its_worker() {
        let dir = tempfile::tempdir().unwrap();
        let mapping: HashMap<Db, LmdbEnvConfig> = DAG_STORES
            .iter()
            .map(|name| {
                (Db::new(name.to_string(), None), LmdbEnvConfig {
                    name: "dagstorage".to_string(),
                    max_env_size: 1 << 30,
                    max_dbs: 32,
                })
            })
            .collect();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let storage = runtime.block_on(async {
            let mut kvm = LmdbDirStoreManager::new(dir.path().to_path_buf(), mapping);
            let storage = BlockDagKeyValueStorage::new_with_hot_window(&mut kvm, i64::MAX / 2)
                .await
                .unwrap();
            let mut parent: Option<BlockHash> = None;
            for height in 0..20_000i64 {
                let block = models::rust::block_implicits::get_random_block(
                    Some(height),
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    Some(parent.iter().cloned().collect()),
                    Some(vec![]),
                    None,
                    None,
                    Some(vec![]),
                    None,
                    None,
                );
                storage.insert(&block, InsertMode::Approved).unwrap();
                parent = Some(block.block_hash);
            }
            Arc::new(storage)
        });

        let (scan, largest_gap) = runtime.block_on(async move {
            let ticks = Arc::new(parking_lot::Mutex::new(Vec::<Instant>::new()));
            let ticker_ticks = ticks.clone();
            let ticker = tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                    ticker_ticks.lock().push(Instant::now());
                }
            });
            tokio::time::sleep(Duration::from_millis(20)).await;
            let (started, finished) = tokio::spawn(async move {
                let started = Instant::now();
                storage.propagate_ft_to_finalized_blocks(0.5).unwrap();
                (started, Instant::now())
            })
            .await
            .unwrap();
            ticker.abort();
            let mut marks = vec![started];
            marks.extend(
                ticks
                    .lock()
                    .iter()
                    .copied()
                    .filter(|t| *t > started && *t < finished),
            );
            marks.push(finished);
            let largest_gap = marks
                .windows(2)
                .map(|pair| pair[1] - pair[0])
                .max()
                .unwrap();
            (finished - started, largest_gap)
        });

        assert!(
            largest_gap < scan / 4,
            "the FT propagation scan held the only worker for {largest_gap:?} of its {scan:?}"
        );
    }

    #[test]
    fn a_finalization_that_fails_part_way_changes_nothing_before_or_after_restart() {
        let dir = tempfile::tempdir().unwrap();
        let mapping = || -> HashMap<Db, LmdbEnvConfig> {
            DAG_STORES
                .iter()
                .map(|name| {
                    (Db::new(name.to_string(), None), LmdbEnvConfig {
                        name: "dagstorage".to_string(),
                        max_env_size: 1 << 30,
                        max_dbs: 32,
                    })
                })
                .collect()
        };
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let open = || {
            runtime.block_on(async {
                let mut kvm = LmdbDirStoreManager::new(dir.path().to_path_buf(), mapping());
                BlockDagKeyValueStorage::new_with_hot_window(&mut kvm, 3)
                    .await
                    .unwrap()
            })
        };
        let storage = open();
        let mut chain: Vec<BlockMessage> = Vec::new();
        for height in 0..10i64 {
            let parents = chain
                .last()
                .map(|parent| vec![parent.block_hash.clone()])
                .unwrap_or_default();
            let block = models::rust::block_implicits::get_random_block(
                Some(height),
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
            let mode = if height == 0 {
                InsertMode::Approved
            } else {
                InsertMode::Normal
            };
            storage.insert(&block, mode).unwrap();
            chain.push(block);
        }
        runtime
            .block_on(storage.record_directly_finalized(
                chain[5].block_hash.clone(),
                0.5,
                |_| async { Ok(()) },
            ))
            .unwrap();

        let never_published = BlockHash::from(vec![9u8; 32]);
        assert!(storage
            .block_metadata_index
            .write()
            .record_finalized(
                chain[9].block_hash.clone(),
                HashSet::from([chain[7].block_hash.clone(), never_published]),
                0.9,
            )
            .is_err());

        let unchanged = |storage: &BlockDagKeyValueStorage| {
            let dag = storage.get_representation().unwrap();
            assert_eq!(dag.last_finalized_block(), chain[5].block_hash);
            for block in [&chain[7], &chain[9]] {
                let metadata = dag.lookup_unsafe(&block.block_hash).unwrap();
                assert!(!metadata.finalized && !metadata.directly_finalized);
            }
        };
        unchanged(&storage);
        drop(storage);
        unchanged(&open());
    }

    #[test]
    fn ft_propagation_raises_only_finalized_blocks_still_in_memory() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let storage = runtime.block_on(async {
            let mut kvm =
                rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager::new();
            BlockDagKeyValueStorage::new_with_hot_window(&mut kvm, 5)
                .await
                .unwrap()
        });
        let mut chain: Vec<BlockMessage> = Vec::new();
        for height in 0..50i64 {
            let parents = chain
                .last()
                .map(|parent| vec![parent.block_hash.clone()])
                .unwrap_or_default();
            let block = models::rust::block_implicits::get_random_block(
                Some(height),
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
            let mode = if height == 0 {
                InsertMode::Approved
            } else {
                InsertMode::Normal
            };
            storage.insert(&block, mode).unwrap();
            chain.push(block);
        }
        let tip = chain.last().unwrap().block_hash.clone();
        runtime
            .block_on(storage.record_directly_finalized(tip.clone(), 0.5, |_| async { Ok(()) }))
            .unwrap();

        storage.propagate_ft_to_finalized_blocks(0.8).unwrap();

        let dag = storage.get_representation().unwrap();
        let ft = |hash: &BlockHash| dag.lookup_unsafe(hash).unwrap().fault_tolerance_value;
        assert_eq!(ft(&tip), 0.8, "a finalized block in memory is raised");
        assert_eq!(
            ft(&chain[10].block_hash),
            0.5,
            "a finalized block that left memory keeps its value"
        );
    }
}
