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
use super::deploy_lifecycle_types::{
    DeployLifecycleTables, LifecycleEvent, LifecycleEventKind, LifecycleEvents, TerminalRecord,
};
use super::equivocation_tracker_store::EquivocationTrackerStore;
use crate::rust::key_value_block_store::KeyValueBlockStore;

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
    /// Settled-history insertion: a hash-checked, unjudged block below the
    /// node's sync anchor, admitted the way LFS restore admitted its
    /// neighbours. Identical to `Normal` except that latest messages are
    /// untouched — settled history predates the anchor's justification
    /// frontier, so it is never anyone's latest message, and letting it
    /// advance one hands fork choice a frontier this node does not hold.
    SettledHistory,
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

    /// Lifecycle: every open sig — the register's schedule rebuild input.
    pub fn open_lifecycle_sigs(&self) -> Result<Vec<DeployId>, KvStoreError> {
        self.lifecycle.read().open_sigs()
    }

    /// Repeat-deploy signature index: true when the startup backfill has
    /// certified the carrier-index completeness invariant for this
    /// database. Absence proofs are sound only behind this gate.
    pub fn carrier_index_complete(&self) -> Result<bool, KvStoreError> {
        self.lifecycle.read().carrier_index_complete()
    }

    /// Repeat-deploy signature index: true when the index PROVES the sig
    /// has no carrier anywhere in the DAG (no terminal record, no event
    /// row). A `false` is not a verdict — it routes the sig to the exact
    /// ancestor scan.
    pub fn carrier_index_proves_absence(&self, sig: &[u8]) -> Result<bool, KvStoreError> {
        self.lifecycle.read().proves_absence(sig)
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
            .canonical_appearance(sig, &|h| {
                self.dag_set.contains(&BlockHash::copy_from_slice(h))
            })?
            .map(BlockHash::from))
    }

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
        self.block_number(block_hash)
            .ok_or_else(|| missing_block(block_hash, "block_number_unsafe"))
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

    // See block-storage/src/main/scala/coop/rchain/blockstorage/dag/BlockDagRepresentationSyntax.scala

    // Get block metadata, "unsafe" because method expects block already in the DAG.
    // (see `missing_hash_context` below for why these errors carry a backtrace)
    pub fn lookup_unsafe(&self, block_hash: &BlockHash) -> Result<BlockMetadata, KvStoreError> {
        match self.lookup(block_hash) {
            Ok(Some(metadata)) => Ok(metadata),
            _ => Err(missing_block(block_hash, "lookup_unsafe")),
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
            return Err(missing_block(block_hash, "self_justification"));
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

    /// Is `ancestor` reachable from `descendant` via ANY parent path (general
    /// DAG ancestry), as opposed to `is_in_main_chain` which follows only the
    /// main-parent spine. Used by multi-parent finality: a validator whose
    /// latest message DAG-descends from a target counts as agreeing on it, even
    /// when the target sits on a secondary (merged-in) branch. Height-pruned
    /// BFS up the parents — a block at or below the ancestor's height cannot
    /// have it among its strictly-lower parents, so that branch is pruned.
    pub fn is_dag_ancestor(
        &self,
        ancestor: &BlockHash,
        descendant: &BlockHash,
    ) -> Result<bool, KvStoreError> {
        if ancestor == descendant {
            return Ok(true);
        }

        let stop_height = self.block_number_unsafe(ancestor)?;
        let mut visited: HashSet<BlockHash> = HashSet::new();
        let mut queue: VecDeque<BlockHash> = VecDeque::new();
        visited.insert(descendant.clone());
        queue.push_back(descendant.clone());

        while let Some(current) = queue.pop_front() {
            if current == *ancestor {
                return Ok(true);
            }
            if self.block_number_unsafe(&current)? <= stop_height {
                continue;
            }
            for parent in self.parents_unsafe(&current)? {
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
        filter_f: impl Fn(&BlockHash) -> bool,
    ) -> Result<HashSet<BlockHash>, KvStoreError> {
        let mut result = HashSet::new();
        let mut current_level = vec![self.lookup_unsafe(&block_hash)?];

        while !current_level.is_empty() {
            let mut next_level = Vec::new();

            for metadata in &current_level {
                for parent in &metadata.parents {
                    if filter_f(parent) && !result.contains(parent) {
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
}

impl BlockDagKeyValueStorage {
    /// Storage-level twin of `KeyValueDagRepresentation::deploy_canonical_appearance`
    /// (same shared lifecycle tables), for callers holding the storage rather
    /// than a representation. Lock order: `block_metadata_index` before
    /// `lifecycle` (the DAG-visibility filter reads the metadata state).
    pub fn deploy_canonical_appearance(
        &self,
        sig: &[u8],
    ) -> Result<Option<BlockHash>, KvStoreError> {
        let metadata_guard = self.block_metadata_index.read();
        Ok(self
            .lifecycle
            .read()
            .canonical_appearance(sig, &|h| {
                metadata_guard.contains(&BlockHash::copy_from_slice(h))
            })?
            .map(BlockHash::from))
    }

    /// One-time carrier-index backfill (startup, next to the LFB-migration
    /// precedent): restores lifecycle events for every DAG-visible block so
    /// the completeness invariant holds for databases written before the
    /// invariant existed — valid blocks from before the lifecycle tables,
    /// and invalid blocks from before `CarriedInvalid`. Idempotent via
    /// `append_event_once`, no-op behind terminal records (pruned rows must
    /// not reopen), and short-circuited by the marker on later starts.
    ///
    /// The marker is written ONLY when the walk examined every DAG-visible
    /// block: a missing body OR a missing metadata record leaves the marker
    /// unwritten and the fast path off — an absence proof over state this
    /// node could not read would claim more than this node can see.
    ///
    /// Lock order (matches `insert` and every other mutator):
    /// `global_lock` -> `block_metadata_index` -> `lifecycle`.
    pub fn ensure_carrier_index_complete(
        &self,
        block_store: &KeyValueBlockStore,
    ) -> Result<bool, KvStoreError> {
        let _lock_guard = self.global_lock.write();
        {
            let lifecycle_guard = self.lifecycle.read();
            if lifecycle_guard.carrier_index_complete()? {
                return Ok(true);
            }
        }
        let hashes = {
            let metadata_guard = self.block_metadata_index.read();
            metadata_guard.dag_set()
        };
        let mut walk_complete = true;
        let mut restored: u64 = 0;
        let lifecycle_guard = self.lifecycle.write();
        for hash in hashes.iter() {
            let invalid = {
                let metadata_guard = self.block_metadata_index.read();
                match metadata_guard.get(hash)? {
                    Some(meta) => meta.invalid,
                    None => {
                        tracing::warn!(
                            "carrier-index backfill: no metadata record for DAG-visible \
                             block {}; the completeness marker stays unwritten",
                            PrettyPrinter::build_string_bytes(hash),
                        );
                        walk_complete = false;
                        continue;
                    }
                }
            };
            let Some(block) = block_store.get(hash)? else {
                tracing::warn!(
                    "carrier-index backfill: no body for DAG-visible block {}; \
                     the completeness marker stays unwritten",
                    PrettyPrinter::build_string_bytes(hash),
                );
                walk_complete = false;
                continue;
            };
            let block_number = block.body.state.block_number;
            for pd in &block.body.deploys {
                let (kind, valid_after) = if invalid {
                    (LifecycleEventKind::CarriedInvalid, None)
                } else {
                    (
                        LifecycleEventKind::Included {
                            is_failed: pd.is_failed,
                        },
                        Some(pd.deploy.data.valid_after_block_number),
                    )
                };
                lifecycle_guard.append_event_once(&pd.deploy.sig, valid_after, LifecycleEvent {
                    height: block_number,
                    block_hash: block.block_hash.to_vec(),
                    kind,
                })?;
                restored += 1;
            }
            if !invalid {
                for rd in &block.body.rejected_deploys {
                    lifecycle_guard.append_event_once(&rd.sig, None, LifecycleEvent {
                        height: block_number,
                        block_hash: block.block_hash.to_vec(),
                        kind: LifecycleEventKind::Rejected {
                            duplicate: rd.duplicate,
                            carrier: rd.carrier.to_vec(),
                        },
                    })?;
                    restored += 1;
                }
            }
        }
        if walk_complete {
            lifecycle_guard.mark_carrier_index_complete()?;
        }
        tracing::info!(
            blocks = hashes.len(),
            events_examined = restored,
            marker_written = walk_complete,
            "carrier-index backfill completed"
        );
        Ok(walk_complete)
    }

    /// Test-only corruption helper (P2-14-style escape hatch): deletes the
    /// PERSISTED metadata row for a block while the in-memory DAG state
    /// still lists it, simulating an inconsistent or partially corrupted
    /// database for the backfill's fail-closed paths.
    #[doc(hidden)]
    pub fn remove_block_metadata_row_for_tests(
        &self,
        hash: &BlockHash,
    ) -> Result<(), KvStoreError> {
        let metadata_guard = self.block_metadata_index.read();
        metadata_guard.delete_kv_row_for_tests(hash)
    }

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

        let floor_index_kv_store = kvm.store("floor-index".to_string()).await?;
        let floor_index_db: KeyValueTypedStoreImpl<BlockHashSerde, BlockHashSerde> =
            KeyValueTypedStoreImpl::new(floor_index_kv_store);
        let frontier_index_kv_store = kvm.store("frontier-index".to_string()).await?;
        let frontier_index_db: KeyValueTypedStoreImpl<BlockHashSerde, BlockHashSerde> =
            KeyValueTypedStoreImpl::new(frontier_index_kv_store);
        let lifecycle_events_kv_store = kvm.store("deploy-lifecycle-events".to_string()).await?;
        let lifecycle_terminal_kv_store =
            kvm.store("deploy-lifecycle-terminal".to_string()).await?;
        let lifecycle_tables =
            DeployLifecycleTables::new(lifecycle_events_kv_store, lifecycle_terminal_kv_store);

        let genesis_hash_kv_store = kvm.store("genesis-hash".to_string()).await?;
        let genesis_hash_db: KeyValueTypedStoreImpl<String, BlockHashSerde> =
            KeyValueTypedStoreImpl::new(genesis_hash_kv_store);

        Ok(Self {
            global_lock: Arc::new(PlRwLock::new(())),
            block_metadata_index: Arc::new(PlRwLock::new(block_metadata_store)),
            invalid_blocks_index: invalid_blocks_db,
            floor_index: floor_index_db,
            frontier_index: frontier_index_db,
            equivocation_tracker_index: equivocation_tracker_store,
            lifecycle: Arc::new(PlRwLock::new(lifecycle_tables)),
            latest_messages_index: latest_messages_db,
            dag_generation: Arc::new(AtomicU64::new(0)),
            ft_lower_bound: Arc::new(AtomicU32::new(0.0f32.to_bits())),
            genesis_hash_index: genesis_hash_db,
        })
    }

    const GENESIS_HASH_KEY: &'static str = "genesis";

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
            global_lock,
            latest_messages_index,
            block_metadata_index,
            invalid_blocks_index,
            floor_index,
            frontier_index,
            equivocation_tracker_index,
            lifecycle: Arc::new(PlRwLock::new(DeployLifecycleTables::in_memory())),
            dag_generation,
            ft_lower_bound: Arc::new(AtomicU32::new(0.0f32.to_bits())),
            genesis_hash_index: KeyValueTypedStoreImpl::new(Arc::new(
                rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore::new(),
            )),
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
            floor_index: self.floor_index.clone(),
            frontier_index: self.frontier_index.clone(),
            lifecycle: self.lifecycle.clone(),
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
        let settled_history = matches!(mode, InsertMode::SettledHistory);
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
        // register newly-bonded validators just like valid blocks. This matches
        // the Scala source-of-truth (`BlockDagKeyValueStorage.scala`, where
        // `newLatestMessages` and `shouldAddAsLatest` never reference `invalid`).
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
        //     `prepare_slashing_deploys`. The pre-fix `if invalid { return empty }`
        //     guard had no Scala counterpart and silently disabled the slashing
        //     pipeline (no slashes ever issued, equivocators never punished).
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

            let newly_bonded_unseen: Vec<Validator> = newly_bonded_set
                .difference(&justification_validators)
                .filter_map(|validator| {
                    match self
                        .latest_messages_index
                        .contains_key(ValidatorSerde((*validator).clone()))
                    {
                        Ok(false) => Some((*validator).clone()),
                        _ => None,
                    }
                })
                .collect();

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
                let placeholder = self.genesis_hash()?.ok_or_else(|| {
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

        let block_exists = {
            let block_metadata_index_guard = self.block_metadata_index.read();

            block_metadata_index_guard.contains(&block.block_hash)
        };

        if block_exists {
            tracing::warn!("{}", log_already_stored);
            self.get_representation_internal()
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

            // Lifecycle event ingest: one body pass projects inclusion and
            // rejection events into the per-sig lifecycle rows. An invalid
            // block's body is not canonical history, but the repeat-deploy
            // ancestor scan still reads it, so its sigs are recorded as
            // `CarriedInvalid` — carrier-index testimony, never a lifecycle
            // outcome. Ingest runs BEFORE the metadata-index add so the
            // carrier-index completeness invariant holds at every crash
            // point: a crash here leaves orphan row events (harmless — a
            // row hit just routes to the exact scan), never a DAG-visible
            // block with unindexed sigs. Every insert path (validated,
            // proposed, genesis, LFS, fixtures) flows through here, so
            // ingest coverage is total by construction.
            // Appends go through `append_event_once`: after a crash inside
            // this ingest-first window the block is not yet DAG-visible, so
            // its redelivery re-runs this pass — the per-(block, kind)
            // dedup makes that retry write nothing twice.
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
                                carrier: rd.carrier.to_vec(),
                            },
                        })?;
                    }
                } else {
                    // `valid_after` stays unset: `CarriedInvalid` is
                    // carrier-index testimony, not an inclusion, so the row
                    // remains record-only until a valid inclusion arrives.
                    for pd in &block.body.deploys {
                        lifecycle_guard.append_event_once(
                            &pd.deploy.sig,
                            None,
                            LifecycleEvent {
                                height: block_number,
                                block_hash: block.block_hash.to_vec(),
                                kind: LifecycleEventKind::CarriedInvalid,
                            },
                        )?;
                    }
                }
                drop(lifecycle_guard);
            }

            let block_metadata = BlockMetadata::from_block(block, invalid, None, None);
            let mut block_metadata_guard = self.block_metadata_index.write();
            block_metadata_guard.add(block_metadata.clone())?;
            drop(block_metadata_guard);
            self.dag_generation.fetch_add(1, Ordering::Relaxed);

            if invalid {
                self.invalid_blocks_index
                    .put_one(block_hash.clone().into(), block_metadata)?;
            }

            // Settled-history blocks never touch latest messages: neither the
            // sender advance below, nor the newly-bonded seeding above —
            // a sub-anchor block's bond set is stale testimony.
            if !settled_history {
                let new_latest_from_sender = if !sender_is_empty {
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
            }

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
                if !dag.contains(&directly_finalized_hash) {
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
fn missing_block(block_hash: &BlockHash, method: &str) -> KvStoreError {
    KvStoreError::MissingBlock {
        hash: block_hash.clone(),
        context: format!(
            " [{}]\n  caller backtrace:\n{}",
            method,
            std::backtrace::Backtrace::force_capture()
        ),
    }
}
