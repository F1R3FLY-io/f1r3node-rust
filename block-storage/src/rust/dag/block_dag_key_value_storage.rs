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

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use models::rust::block_hash::{self, BlockHash, BlockHashSerde};
#[cfg(any(test, feature = "test-internals"))]
use models::rust::block_metadata::AdmissionRejectionReason;
use models::rust::block_metadata::{BlockMetadata, ADMISSION_SCHEMA_VERSION};
pub use models::rust::block_metadata::{CertifiedAdmissionOutcome, CertifiedSenderAuthority};
use models::rust::bond_generation::BondGeneration;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::{BlockMessage, FinalizationCertificate};
use models::rust::deploy_id::{DeployLookupId, LegacyDeploySignature};
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
use prost::bytes::Bytes;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use shared::rust::store::key_value_store::{
    strict_atomic_mutate, AtomicStoreMutation, AtomicStoreOperation, KeyValueStore, KvStoreError,
};
use shared::rust::store::key_value_typed_store::KeyValueTypedStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

use super::block_metadata_store::BlockMetadataStore;
use super::deploy_lifecycle_types::{
    DeployLifecycleTables, LifecycleEvent, LifecycleEventKind, LifecycleEvents, TerminalRecord,
};
use super::deploy_occurrence_store::DeployOccurrenceStore;
use super::deploy_occurrence_types::{
    DeployOccurrence, OccurrenceAdmissionMode, DEPLOY_OCCURRENCE_PROTOCOL_VERSION,
    DEPLOY_OCCURRENCE_SCHEMA_VERSION,
};
use super::equivocation_tracker_store::EquivocationTrackerStore;
use crate::rust::finality::{
    state_preservation, FinalizationAppendOutcome, FinalizationEffectId, FinalizationHead,
    FinalizationLedger, FinalizationRecord,
};
use crate::rust::key_value_block_store::KeyValueBlockStore;

pub type DeployId = shared::rust::ByteString;

#[cfg(any(test, feature = "test-internals"))]
fn test_sender_authority_certificate(
    block: &BlockMessage,
    generation: BondGeneration,
    stake: i64,
) -> Result<CertifiedSenderAuthority, KvStoreError> {
    let commitment = block.header.finalized_floor.as_ref().ok_or_else(|| {
        KvStoreError::InvalidArgument(
            "a non-genesis test block requires a finalized-floor commitment".to_string(),
        )
    })?;
    commitment
        .validate_shape()
        .map_err(KvStoreError::InvalidArgument)?;
    CertifiedSenderAuthority::new(
        block,
        commitment.floor_hash.clone(),
        commitment.floor_post_state_hash.clone(),
        commitment.authority_context_digest.clone(),
        generation,
        stake,
    )
    .map_err(|error| KvStoreError::InvalidArgument(error.to_string()))
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
    ApprovedGenesis,
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
    pub canonical_genesis_hash: Option<BlockHash>,
    pub latest_messages_map: imbl::HashMap<Validator, BlockHash>,
    pub child_map: imbl::HashMap<BlockHash, imbl::HashSet<BlockHash>>,
    pub height_map: imbl::OrdMap<i64, imbl::HashSet<BlockHash>>,
    pub block_number_map: imbl::HashMap<BlockHash, i64>,
    pub main_parent_map: imbl::HashMap<BlockHash, BlockHash>,
    pub self_justification_map: imbl::HashMap<BlockHash, BlockHash>,
    pub invalid_blocks_set: imbl::HashSet<BlockMetadata>,
    pub equivocation_observations:
        imbl::HashMap<(Validator, BondGeneration, SequenceNumber), BTreeSet<BlockHash>>,
    pub last_finalized_block_hash: BlockHash,
    pub finalized_blocks_set: imbl::HashSet<BlockHash>,
    // P2-14: the metadata index is kept `pub` for cross-crate test
    // fixtures that build a `KeyValueDagRepresentation` from raw
    // components. Production code on the same crate (block-storage)
    // accesses it through the inherent methods on this type; treat
    // direct manipulation as a test-only escape hatch.
    #[doc(hidden)]
    pub block_metadata_index: Arc<PlRwLock<BlockMetadataStore>>,
    #[doc(hidden)]
    pub deploy_index: Arc<PlRwLock<KeyValueTypedStoreImpl<DeployId, BlockHashSerde>>>,
    #[doc(hidden)]
    pub deploy_occurrence_store: DeployOccurrenceStore,
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
    pub fn canonical_genesis_hash(&self) -> Option<&BlockHash> {
        self.canonical_genesis_hash.as_ref()
    }

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

    pub fn validate_latest_message_materialization(&self) -> Result<(), KvStoreError> {
        for (validator, hash) in &self.latest_messages_map {
            if self.canonical_genesis_hash() == Some(hash) {
                continue;
            }
            self.lookup_unsafe(hash)
                .map_err(|_| KvStoreError::MissingBlock {
                    hash: hash.clone(),
                    context: format!(
                        "latest message for validator {}",
                        PrettyPrinter::build_string_bytes(validator)
                    ),
                })?;
        }
        Ok(())
    }

    pub fn invalid_blocks(&self) -> imbl::HashSet<BlockMetadata> { self.invalid_blocks_set.clone() }

    pub fn equivocation_observations(
        &self,
    ) -> imbl::HashMap<(Validator, BondGeneration, SequenceNumber), BTreeSet<BlockHash>> {
        self.equivocation_observations.clone()
    }

    pub fn structural_equivocation_keys(
        &self,
    ) -> HashSet<(Validator, BondGeneration, SequenceNumber)> {
        self.equivocation_observations
            .iter()
            .filter(|(_, hashes)| hashes.len() > 1)
            .map(|((validator, generation, sequence), _)| {
                (validator.clone(), *generation, *sequence)
            })
            .collect()
    }

    pub fn objective_equivocations_for_generations(
        &self,
        active_generations: &HashMap<Validator, BondGeneration>,
    ) -> imbl::HashMap<(Validator, BondGeneration, SequenceNumber), (BlockHash, BlockHash)> {
        let mut result = imbl::HashMap::new();
        for ((validator, generation, sequence), hashes) in &self.equivocation_observations {
            if active_generations.get(validator) == Some(generation) && hashes.len() >= 2 {
                let mut hashes = hashes.iter();
                result.insert(
                    (validator.clone(), *generation, *sequence),
                    (
                        hashes.next().expect("pair has first hash").clone(),
                        hashes.next().expect("pair has second hash").clone(),
                    ),
                );
            }
        }
        result
    }

    pub fn objective_equivocations(
        &self,
    ) -> imbl::HashMap<(Validator, BondGeneration, SequenceNumber), (BlockHash, BlockHash)> {
        self.equivocation_observations
            .iter()
            .filter_map(|((validator, generation, sequence), hashes)| {
                if hashes.len() < 2 {
                    return None;
                }
                let mut hashes = hashes.iter();
                Some((
                    (validator.clone(), *generation, *sequence),
                    (
                        hashes.next().expect("pair has first hash").clone(),
                        hashes.next().expect("pair has second hash").clone(),
                    ),
                ))
            })
            .collect()
    }

    pub fn objective_equivocators_for_generations(
        &self,
        active_generations: &HashMap<Validator, BondGeneration>,
    ) -> HashSet<Validator> {
        self.objective_equivocations_for_generations(active_generations)
            .keys()
            .map(|(validator, _, _)| validator.clone())
            .collect()
    }

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
    pub fn deploy_terminal(
        &self,
        deploy_id: &DeployLookupId,
    ) -> Result<Option<TerminalRecord>, KvStoreError> {
        self.lifecycle.read().get_terminal(deploy_id)
    }

    /// Lifecycle: the open event row for a sig (pruned once terminal).
    pub fn deploy_lifecycle_events(
        &self,
        deploy_id: &DeployLookupId,
    ) -> Result<Option<LifecycleEvents>, KvStoreError> {
        self.lifecycle.read().get_events(deploy_id)
    }

    /// Lifecycle: WRITE-ONCE terminal write (prunes the event row on a
    /// fresh write; returns the survivor on a duplicate attempt).
    pub fn put_deploy_terminal_if_absent(
        &self,
        deploy_id: &DeployLookupId,
        record: TerminalRecord,
    ) -> Result<TerminalRecord, KvStoreError> {
        if matches!(deploy_id, DeployLookupId::V6(_)) {
            return Err(KvStoreError::InvalidArgument(
                "protocol-v6 terminalization requires atomic occurrence compaction".to_string(),
            ));
        }
        self.lifecycle
            .write()
            .put_terminal_if_absent(deploy_id, record)
    }

    pub fn put_deploy_terminal_and_compact_occurrences(
        &self,
        deploy_id: models::rust::deploy_id::DeployIdV6,
        record: TerminalRecord,
        finalization_revision: u64,
        finalized_floor_hash: [u8; 32],
        finalized_floor_height: i64,
        compaction_horizon: i64,
    ) -> Result<TerminalRecord, KvStoreError> {
        let typed_id = DeployLookupId::V6(deploy_id);
        let lifecycle = self.lifecycle.write();
        let survivor = lifecycle
            .get_terminal(&typed_id)?
            .unwrap_or_else(|| record.clone());
        let occurrence_plan = self.deploy_occurrence_store.prepare_compaction(
            deploy_id,
            survivor.state,
            survivor.rejection_count,
            finalization_revision,
            finalized_floor_hash,
            finalized_floor_height,
            compaction_horizon,
        )?;
        let terminal_store = lifecycle.terminal_store();
        let events_store = lifecycle.events_store();
        let expected_terminal = terminal_store
            .get_one(&typed_id)?
            .map(|existing| terminal_store.encode_value(&existing))
            .transpose()?;
        let mut owned_mutations = vec![
            (
                terminal_store.raw_store().clone(),
                terminal_store.encode_key(&typed_id)?,
                AtomicStoreOperation::CompareAndSwap {
                    expected: expected_terminal,
                    replacement: Some(terminal_store.encode_value(&survivor)?),
                },
            ),
            (
                events_store.raw_store().clone(),
                events_store.encode_key(&typed_id)?,
                AtomicStoreOperation::Delete,
            ),
        ];
        owned_mutations.extend(
            occurrence_plan
                .mutations
                .into_iter()
                .map(|(key, operation)| {
                    (
                        self.deploy_occurrence_store.raw_store().clone(),
                        key,
                        operation,
                    )
                }),
        );
        let mutations = owned_mutations
            .iter()
            .map(|(store, key, operation)| AtomicStoreMutation {
                store: store.as_ref(),
                key: key.clone(),
                operation: operation.clone(),
            })
            .collect::<Vec<_>>();
        strict_atomic_mutate(&mutations)?;
        Ok(survivor)
    }

    /// Lifecycle: every open sig — the register's schedule rebuild input.
    pub fn open_lifecycle_sigs(&self) -> Result<Vec<DeployLookupId>, KvStoreError> {
        self.lifecycle.read().open_sigs()
    }

    /// The sig's most recent canonical appearance — the latest lifecycle
    /// event by (height, hash), or the terminal record's frozen display
    /// block once the row is pruned. A pure function of the DAG's bodies,
    /// so the answer never depends on node-local insertion order.
    pub fn deploy_canonical_appearance(
        &self,
        deploy_id: &DeployLookupId,
    ) -> Result<Option<BlockHash>, KvStoreError> {
        Ok(self
            .lifecycle
            .read()
            .canonical_appearance(deploy_id)?
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

    pub fn lookup_by_legacy_signature(
        &self,
        deploy_id: &LegacyDeploySignature,
    ) -> Result<Option<BlockHash>, KvStoreError> {
        let deploy_index_guard = self.deploy_index.read();
        deploy_index_guard
            .get_one(&deploy_id.as_bytes().to_vec())
            .map(|result| result.map(|block_hash_serde| block_hash_serde.into()))
    }

    pub fn lookup_by_deploy_id(
        &self,
        deploy_id: &DeployLookupId,
    ) -> Result<Option<BlockHash>, KvStoreError> {
        match deploy_id {
            DeployLookupId::Legacy(signature) => self.lookup_by_legacy_signature(signature),
            DeployLookupId::V6(deploy_id) => self.deploy_occurrence_store.canonical(*deploy_id),
        }
    }

    pub fn lookup_deploy_occurrences(
        &self,
        deploy_id: &DeployLookupId,
    ) -> Result<BTreeSet<BlockHash>, KvStoreError> {
        match deploy_id {
            DeployLookupId::Legacy(signature) => Ok(self
                .lookup_by_legacy_signature(signature)?
                .into_iter()
                .collect()),
            DeployLookupId::V6(deploy_id) => Ok(self
                .deploy_occurrence_store
                .exact_occurrences(*deploy_id)?
                .into_keys()
                .collect()),
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

    pub fn latest_message(
        &self,
        validator: &Validator,
    ) -> Result<Option<BlockMetadata>, KvStoreError> {
        match self.latest_message_hash(validator) {
            Some(hash) if self.canonical_genesis_hash() == Some(&hash) => Ok(None),
            Some(hash) => self.lookup_unsafe(&hash).map(Some),
            None => Ok(None),
        }
    }

    pub fn latest_messages(&self) -> Result<HashMap<Validator, BlockMetadata>, KvStoreError> {
        let latest_messages = self.latest_message_hashes();

        let mut result = HashMap::new();
        for (validator, hash) in latest_messages.iter() {
            if self.canonical_genesis_hash() == Some(hash) {
                continue;
            }
            match self.lookup(hash)? {
                Some(metadata) => {
                    result.insert(validator.clone(), metadata);
                }
                None => {
                    return Err(KvStoreError::MissingBlock {
                        hash: hash.clone(),
                        context: "latest message".to_string(),
                    });
                }
            }
        }

        Ok(result)
    }

    pub fn valid_latest_messages(
        &self,
        active_generations: &HashMap<Validator, BondGeneration>,
    ) -> Result<HashMap<Validator, BlockMetadata>, KvStoreError> {
        let latest = self.latest_messages()?;
        let objective_equivocators =
            self.objective_equivocators_for_generations(active_generations);
        Ok(latest
            .into_iter()
            .filter(|(validator, metadata)| {
                metadata.is_accepted()
                    && !objective_equivocators.contains(validator)
                    && (metadata.parents.is_empty()
                        || metadata.sender_bond_generation().is_some_and(|generation| {
                            active_generations.get(validator) == Some(&generation)
                        }))
            })
            .collect())
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
                .map(|metadata| metadata.is_rejected())
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
                if metadata.is_rejected() {
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

    fn consume_certified_support_work(remaining: &mut usize) -> Result<(), KvStoreError> {
        *remaining = remaining.checked_sub(1).ok_or_else(|| {
            KvStoreError::InvalidArgument(format!(
                "finalization certificate exceeds the deterministic DAG-visit limit {}",
                FinalizationCertificate::MAX_DAG_VISITS_PER_VERIFICATION
            ))
        })?;
        Ok(())
    }

    fn is_dag_ancestor_certified_support(
        &self,
        ancestor: &BlockHash,
        descendant: &BlockHash,
        remaining: &mut usize,
    ) -> Result<bool, KvStoreError> {
        if ancestor == descendant {
            return Ok(true);
        }
        let stop_height = self.block_number_unsafe(ancestor)?;
        let mut visited = HashSet::from([descendant.clone()]);
        let mut pending = BTreeSet::from([descendant.clone()]);
        while let Some(hash) = pending.pop_first() {
            Self::consume_certified_support_work(remaining)?;
            if hash == *ancestor {
                return Ok(true);
            }
            let metadata = self.lookup_unsafe(&hash)?;
            if metadata.block_number <= stop_height {
                continue;
            }
            for parent in metadata.parents {
                if visited.insert(parent.clone()) {
                    pending.insert(parent);
                }
            }
        }
        Ok(false)
    }

    fn is_predecessor_history_certified_support(
        &self,
        hash: &BlockHash,
        block_number: i64,
        predecessor: &BlockHash,
        predecessor_block_number: i64,
        remaining_work: &mut usize,
        predecessor_cache: &mut HashMap<BlockHash, bool>,
    ) -> Result<bool, KvStoreError> {
        if hash == predecessor {
            return Ok(true);
        }
        if block_number > predecessor_block_number {
            return Ok(false);
        }
        if let Some(result) = predecessor_cache.get(hash) {
            return Ok(*result);
        }
        let result = self.is_dag_ancestor_certified_support(hash, predecessor, remaining_work)?;
        predecessor_cache.insert(hash.clone(), result);
        Ok(result)
    }

    pub fn certified_finalized_delta(
        &self,
        predecessor: &BlockHash,
        target: &BlockHash,
        maximum_finalized_blocks: usize,
        remaining_work: &mut usize,
    ) -> Result<BTreeSet<BlockHashSerde>, KvStoreError> {
        if maximum_finalized_blocks == 0
            || maximum_finalized_blocks > FinalizationCertificate::MAX_FINALIZED_BLOCKS
        {
            return Err(KvStoreError::InvalidArgument(format!(
                "finalization delta limit must be between 1 and {}",
                FinalizationCertificate::MAX_FINALIZED_BLOCKS
            )));
        }
        let predecessor_block_number = self.block_number_unsafe(predecessor)?;
        let mut finalized = BTreeSet::new();
        let mut queued = HashSet::from([target.clone()]);
        let mut pending = BTreeSet::from([target.clone()]);
        let mut predecessor_cache = HashMap::new();
        while let Some(hash) = pending.pop_first() {
            Self::consume_certified_support_work(remaining_work)?;
            let metadata = self.lookup_unsafe(&hash)?;
            if self.is_predecessor_history_certified_support(
                &hash,
                metadata.block_number,
                predecessor,
                predecessor_block_number,
                remaining_work,
                &mut predecessor_cache,
            )? {
                continue;
            }
            if finalized.insert(BlockHashSerde(hash.clone()))
                && finalized.len() > maximum_finalized_blocks
            {
                return Err(KvStoreError::InvalidArgument(format!(
                    "finalization ancestry delta exceeds the protocol limit {}",
                    maximum_finalized_blocks
                )));
            }
            for parent in metadata.parents {
                if queued.insert(parent.clone()) {
                    pending.insert(parent);
                }
            }
        }
        Ok(finalized)
    }

    pub fn certified_support_closure(
        &self,
        predecessor: &BlockHash,
        roots: impl IntoIterator<Item = BlockHash>,
        maximum_supporting_blocks: usize,
        remaining_work: &mut usize,
    ) -> Result<BTreeSet<BlockHashSerde>, KvStoreError> {
        if maximum_supporting_blocks == 0
            || maximum_supporting_blocks > FinalizationCertificate::MAX_SUPPORTING_BLOCKS
        {
            return Err(KvStoreError::InvalidArgument(format!(
                "finalization support limit must be between 1 and {}",
                FinalizationCertificate::MAX_SUPPORTING_BLOCKS
            )));
        }
        let predecessor_block_number = self.block_number_unsafe(predecessor)?;
        let mut support = BTreeSet::new();
        let mut queued = HashSet::new();
        let mut pending = BTreeSet::new();
        let mut predecessor_cache = HashMap::new();
        for hash in roots {
            if queued.insert(hash.clone()) {
                pending.insert(hash);
            }
        }
        while let Some(hash) = pending.pop_first() {
            Self::consume_certified_support_work(remaining_work)?;
            if !support.insert(BlockHashSerde(hash.clone())) {
                continue;
            }
            if support.len() > maximum_supporting_blocks {
                return Err(KvStoreError::InvalidArgument(format!(
                    "finalization support closure exceeds the protocol limit {}",
                    maximum_supporting_blocks
                )));
            }
            if self.canonical_genesis_hash() == Some(&hash) {
                continue;
            }
            let metadata = self.lookup_unsafe(&hash)?;
            let predecessor_history = self.is_predecessor_history_certified_support(
                &hash,
                metadata.block_number,
                predecessor,
                predecessor_block_number,
                remaining_work,
                &mut predecessor_cache,
            )?;
            if predecessor_history {
                continue;
            }
            let dependencies = metadata
                .parents
                .into_iter()
                .chain(
                    metadata
                        .justifications
                        .into_iter()
                        .map(|justification| justification.latest_block_hash),
                )
                .collect::<BTreeSet<_>>();
            for dependency in dependencies {
                if queued.insert(dependency.clone()) {
                    pending.insert(dependency);
                }
            }
        }
        Ok(support)
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
    pub(crate) deploy_index: Arc<PlRwLock<KeyValueTypedStoreImpl<DeployId, BlockHashSerde>>>,
    pub(crate) deploy_occurrence_store: DeployOccurrenceStore,
    pub(crate) invalid_blocks_index: KeyValueTypedStoreImpl<BlockHashSerde, BlockMetadata>,
    /// Memoized justification-derived floor per block (block hash -> floor hash).
    pub(crate) floor_index: KeyValueTypedStoreImpl<BlockHashSerde, BlockHashSerde>,
    /// Memoized per-block finalized frontier (see `KeyValueDagRepresentation::frontier_index`).
    pub(crate) frontier_index: KeyValueTypedStoreImpl<BlockHashSerde, BlockHashSerde>,
    /// Equivocation tracker — RMW MUST route through
    /// `access_equivocations_tracker` (Bug #2 / T-9.2).
    pub(crate) equivocation_tracker_index: EquivocationTrackerStore,
    pub(crate) equivocation_evidence_index: KeyValueTypedStoreImpl<
        (ValidatorSerde, BondGeneration, SequenceNumber),
        BTreeSet<BlockHashSerde>,
    >,
    pub(crate) finalization_ledger: FinalizationLedger,
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

#[derive(Clone)]
pub struct FinalizationBase {
    pub head: FinalizationHead,
    pub dag: KeyValueDagRepresentation,
    pub dag_generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FinalizationWitnessInputs {
    pub protocol_version: i64,
    pub shard_id: String,
    pub predecessor_certificate_digest: BlockHashSerde,
    pub predecessor_certificate_block_hash: BlockHashSerde,
    pub fault_tolerance_numerator: i64,
    pub fault_tolerance_denominator: i64,
    pub latest_messages: BTreeMap<ValidatorSerde, BlockHashSerde>,
    pub authority_context_digest: BlockHashSerde,
}

fn predecessor_certificate_carrier_digest(
    dag: &KeyValueDagRepresentation,
    carrier: &BlockHash,
    predecessor_floor: &BlockHash,
    predecessor_post_state: &BlockHash,
    protocol_version: i64,
) -> Result<Option<BlockHash>, KvStoreError> {
    if dag.canonical_genesis_hash() == Some(carrier) {
        return Ok(None);
    }
    let metadata = dag.lookup_unsafe(carrier)?;
    metadata
        .validate()
        .map_err(|error| KvStoreError::InvalidArgument(error.to_string()))?;
    if metadata.approved_genesis
        || !metadata.is_accepted()
        || metadata.protocol_version != protocol_version
    {
        return Ok(None);
    }
    let Some(commitment) = metadata.finalized_floor_commitment.as_ref() else {
        return Ok(None);
    };
    commitment
        .validate_shape()
        .map_err(KvStoreError::InvalidArgument)?;
    Ok((commitment.floor_hash == *predecessor_floor
        && commitment.floor_post_state_hash == *predecessor_post_state)
        .then(|| commitment.certificate_digest.clone()))
}

impl BlockDagKeyValueStorage {
    /// Storage-level twin of `KeyValueDagRepresentation::deploy_canonical_appearance`
    /// (same shared lifecycle tables), for callers holding the storage rather
    /// than a representation.
    pub fn deploy_canonical_appearance(
        &self,
        deploy_id: &DeployLookupId,
    ) -> Result<Option<BlockHash>, KvStoreError> {
        Ok(self
            .lifecycle
            .read()
            .canonical_appearance(deploy_id)?
            .map(BlockHash::from))
    }

    pub async fn new(kvm: &mut (impl KeyValueStoreManager + ?Sized)) -> Result<Self, KvStoreError> {
        let admission_schema_kv_store = kvm.store("dag-admission-schema".to_string()).await?;
        let admission_schema_db: KeyValueTypedStoreImpl<String, u32> =
            KeyValueTypedStoreImpl::new(admission_schema_kv_store);
        let block_metadata_kv_store = kvm.store("block-metadata".to_string()).await?;
        let equivocation_tracker_kv_store =
            kvm.store("equivocation-tracker-v5".to_string()).await?;
        let equivocation_evidence_kv_store =
            kvm.store("equivocation-evidence-v5".to_string()).await?;
        let latest_messages_kv_store = kvm.store("latest-messages".to_string()).await?;
        let invalid_blocks_kv_store = kvm.store("invalid-blocks".to_string()).await?;
        let deploy_index_kv_store = kvm.store("deploy-index".to_string()).await?;
        let deploy_occurrence_index_kv_store =
            kvm.store("deploy-occurrence-index".to_string()).await?;
        let floor_index_kv_store = kvm.store("floor-index".to_string()).await?;
        let frontier_index_kv_store = kvm.store("frontier-index".to_string()).await?;
        let genesis_hash_kv_store = kvm.store("genesis-hash".to_string()).await?;
        let finalization_ledger_kv_store = kvm
            .store(FinalizationLedger::STORE_NAME.to_string())
            .await?;
        let lifecycle_events_kv_store = kvm.store("deploy-lifecycle-events".to_string()).await?;
        let lifecycle_terminal_kv_store =
            kvm.store("deploy-lifecycle-terminal".to_string()).await?;

        let schema_key = "casper-v6".to_string();
        match admission_schema_db.get_one(&schema_key)? {
            Some(version) if version == ADMISSION_SCHEMA_VERSION => {}
            Some(version) => {
                return Err(KvStoreError::InvalidArgument(format!(
                    "unsupported DAG admission schema {version}; expected {ADMISSION_SCHEMA_VERSION}"
                )));
            }
            None => {
                let existing_indices = [
                    ("block-metadata", &block_metadata_kv_store),
                    ("equivocation-tracker-v5", &equivocation_tracker_kv_store),
                    ("equivocation-evidence-v5", &equivocation_evidence_kv_store),
                    ("latest-messages", &latest_messages_kv_store),
                    ("invalid-blocks", &invalid_blocks_kv_store),
                    ("deploy-index", &deploy_index_kv_store),
                    ("deploy-occurrence-index", &deploy_occurrence_index_kv_store),
                    ("floor-index", &floor_index_kv_store),
                    ("frontier-index", &frontier_index_kv_store),
                    ("genesis-hash", &genesis_hash_kv_store),
                    (
                        FinalizationLedger::STORE_NAME,
                        &finalization_ledger_kv_store,
                    ),
                    ("deploy-lifecycle-events", &lifecycle_events_kv_store),
                    ("deploy-lifecycle-terminal", &lifecycle_terminal_kv_store),
                ];
                for (name, store) in existing_indices {
                    if store.non_empty()? {
                        return Err(KvStoreError::InvalidArgument(format!(
                            "DAG index {name} contains data without a protocol-v6 admission schema; start from a fresh protocol-v6 genesis or run an explicit verified migration"
                        )));
                    }
                }
                admission_schema_db.put_one(schema_key, ADMISSION_SCHEMA_VERSION)?;
            }
        }
        let schema_rows = admission_schema_db.to_map()?;
        if schema_rows.len() != 1 || schema_rows.get("casper-v6") != Some(&ADMISSION_SCHEMA_VERSION)
        {
            return Err(KvStoreError::InvalidArgument(
                "DAG admission schema contains partial or unknown activation state".to_string(),
            ));
        }
        if deploy_index_kv_store.non_empty()? {
            return Err(KvStoreError::InvalidArgument(
                "protocol-v6 DAG contains a legacy deploy index; start from a fresh protocol-v6 genesis"
                    .to_string(),
            ));
        }

        let block_metadata_db: KeyValueTypedStoreImpl<BlockHashSerde, BlockMetadata> =
            KeyValueTypedStoreImpl::new(block_metadata_kv_store);
        let mut block_metadata_store = BlockMetadataStore::new(block_metadata_db)?;
        let finalization_ledger = FinalizationLedger::from_store(finalization_ledger_kv_store);
        finalization_ledger.validate_integrity()?;
        if let Some(head) = finalization_ledger.head()? {
            if head.revision == 0 && block_metadata_store.contains(&head.block_hash.0) {
                block_metadata_store.record_finalized(
                    head.block_hash.0.clone(),
                    HashSet::new(),
                    1.0,
                )?;
            }
            for record in finalization_ledger.pending_projection_records()? {
                let indirectly = record
                    .finalized
                    .iter()
                    .filter(|hash| *hash != &record.directly_finalized)
                    .map(|hash| hash.0.clone())
                    .collect();
                block_metadata_store.record_finalized(
                    record.directly_finalized.0,
                    indirectly,
                    f32::from_bits(record.fault_tolerance_bits),
                )?;
                let finalized = block_metadata_store.finalized_block_hashes();
                block_metadata_store
                    .update_ft_if_higher(finalized, f32::from_bits(record.fault_tolerance_bits))?;
                finalization_ledger.record_projection_completed(record.revision)?;
            }
        } else if !block_metadata_store.dag_set().is_empty() {
            return Err(KvStoreError::InvalidArgument(
                "protocol-v6 DAG metadata exists without a finalization ledger; start from a fresh genesis or run an explicit verified migration"
                    .to_string(),
            ));
        }
        let equivocation_tracker_db: KeyValueTypedStoreImpl<
            (ValidatorSerde, BondGeneration, SequenceNumber),
            BTreeSet<BlockHashSerde>,
        > = KeyValueTypedStoreImpl::new(equivocation_tracker_kv_store);
        let equivocation_tracker_store = EquivocationTrackerStore::new(equivocation_tracker_db);
        let equivocation_evidence_db: KeyValueTypedStoreImpl<
            (ValidatorSerde, BondGeneration, SequenceNumber),
            BTreeSet<BlockHashSerde>,
        > = KeyValueTypedStoreImpl::new(equivocation_evidence_kv_store);
        let latest_messages_db: KeyValueTypedStoreImpl<ValidatorSerde, BlockHashSerde> =
            KeyValueTypedStoreImpl::new(latest_messages_kv_store);
        let invalid_blocks_db: KeyValueTypedStoreImpl<BlockHashSerde, BlockMetadata> =
            KeyValueTypedStoreImpl::new(invalid_blocks_kv_store);
        let floor_index_db: KeyValueTypedStoreImpl<BlockHashSerde, BlockHashSerde> =
            KeyValueTypedStoreImpl::new(floor_index_kv_store);
        let frontier_index_db: KeyValueTypedStoreImpl<BlockHashSerde, BlockHashSerde> =
            KeyValueTypedStoreImpl::new(frontier_index_kv_store);
        let genesis_hash_db: KeyValueTypedStoreImpl<String, BlockHashSerde> =
            KeyValueTypedStoreImpl::new(genesis_hash_kv_store);
        let deploy_index_db: KeyValueTypedStoreImpl<DeployId, BlockHashSerde> =
            KeyValueTypedStoreImpl::new(deploy_index_kv_store);
        let deploy_occurrence_store =
            DeployOccurrenceStore::activate_fresh(deploy_occurrence_index_kv_store)?;
        let lifecycle_tables =
            DeployLifecycleTables::new(lifecycle_events_kv_store, lifecycle_terminal_kv_store);

        Ok(Self {
            global_lock: Arc::new(PlRwLock::new(())),
            block_metadata_index: Arc::new(PlRwLock::new(block_metadata_store)),
            deploy_index: Arc::new(PlRwLock::new(deploy_index_db)),
            deploy_occurrence_store,
            invalid_blocks_index: invalid_blocks_db,
            floor_index: floor_index_db,
            frontier_index: frontier_index_db,
            equivocation_tracker_index: equivocation_tracker_store,
            equivocation_evidence_index: equivocation_evidence_db,
            finalization_ledger,
            lifecycle: Arc::new(PlRwLock::new(lifecycle_tables)),
            latest_messages_index: latest_messages_db,
            dag_generation: Arc::new(AtomicU64::new(0)),
            ft_lower_bound: Arc::new(AtomicU32::new(0.0f32.to_bits())),
            genesis_hash_index: genesis_hash_db,
        })
    }

    const GENESIS_HASH_KEY: &'static str = "genesis";

    fn record_genesis_hash_internal(&self, hash: BlockHash) -> Result<(), KvStoreError> {
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

    pub fn record_genesis_hash(&self, hash: BlockHash) -> Result<(), KvStoreError> {
        let _lock_guard = self.global_lock.write();
        self.record_genesis_hash_internal(hash)
    }

    fn genesis_hash_internal(&self) -> Result<Option<BlockHash>, KvStoreError> {
        if let Some(BlockHashSerde(hash)) = self
            .genesis_hash_index
            .get_one(&Self::GENESIS_HASH_KEY.to_string())?
        {
            return Ok(Some(hash));
        }
        let metadata = self.block_metadata_index.read();
        let state = metadata.dag_state().read();
        Ok(state
            .height_map
            .get(&0)
            .and_then(|blocks| blocks.iter().min().cloned()))
    }

    pub fn genesis_hash(&self) -> Result<Option<BlockHash>, KvStoreError> {
        let _lock_guard = self.global_lock.read();
        self.genesis_hash_internal()
    }

    #[cfg(any(test, feature = "test-internals"))]
    #[doc(hidden)]
    pub fn delete_latest_message_for_test(&self, validator: Validator) -> Result<(), KvStoreError> {
        let _lock_guard = self.global_lock.write();
        self.latest_messages_index
            .delete(vec![ValidatorSerde(validator)])
    }

    #[cfg(any(test, feature = "test-internals"))]
    #[doc(hidden)]
    pub fn put_latest_message_for_test(
        &self,
        validator: Validator,
        hash: BlockHash,
    ) -> Result<(), KvStoreError> {
        let _lock_guard = self.global_lock.write();
        self.latest_messages_index
            .put_one(ValidatorSerde(validator), BlockHashSerde(hash))
    }

    #[cfg(any(test, feature = "test-internals"))]
    #[doc(hidden)]
    pub fn delete_objective_evidence_for_test(
        &self,
        validator: Validator,
        generation: BondGeneration,
        sequence_number: SequenceNumber,
    ) -> Result<(), KvStoreError> {
        let _lock_guard = self.global_lock.write();
        self.equivocation_evidence_index.delete(vec![(
            ValidatorSerde(validator),
            generation,
            sequence_number,
        )])
    }

    #[cfg(any(test, feature = "test-internals"))]
    #[doc(hidden)]
    pub fn delete_invalid_evidence_for_test(
        &self,
        block_hash: BlockHash,
    ) -> Result<(), KvStoreError> {
        let _lock_guard = self.global_lock.write();
        self.invalid_blocks_index
            .delete(vec![BlockHashSerde(block_hash)])
    }

    #[cfg(any(test, feature = "test-internals"))]
    #[doc(hidden)]
    pub fn put_invalid_evidence_for_test(
        &self,
        block_hash: BlockHash,
        metadata: BlockMetadata,
    ) -> Result<(), KvStoreError> {
        let _lock_guard = self.global_lock.write();
        self.invalid_blocks_index
            .put_one(BlockHashSerde(block_hash), metadata)
    }

    pub fn reconcile_latest_messages(
        &self,
        block_store: &KeyValueBlockStore,
    ) -> Result<(), KvStoreError> {
        let _lock_guard = self.global_lock.write();
        let genesis_hash = self.genesis_hash_internal()?.ok_or_else(|| {
            KvStoreError::InvalidArgument(
                "cannot reconcile latest messages without canonical approved genesis".to_string(),
            )
        })?;
        let metadata = {
            let metadata_guard = self.block_metadata_index.read();
            metadata_guard
                .dag_set()
                .into_iter()
                .map(|hash| metadata_guard.get_unsafe(&hash))
                .collect::<Result<Vec<_>, _>>()?
        };

        let mut registered = HashSet::new();
        for entry in metadata.iter().filter(|entry| entry.is_accepted()) {
            let block = block_store.get(&entry.block_hash)?.ok_or_else(|| {
                KvStoreError::KeyNotFound(format!(
                    "accepted DAG block {} is missing from block store during latest-message reconciliation",
                    PrettyPrinter::build_string_bytes(&entry.block_hash)
                ))
            })?;
            registered.extend(
                block
                    .body
                    .state
                    .bonds
                    .iter()
                    .filter(|bond| bond.stake > 0)
                    .map(|bond| bond.validator.clone()),
            );
        }

        let mut evidence_groups: HashMap<
            (ValidatorSerde, BondGeneration, SequenceNumber),
            BTreeSet<BlockHashSerde>,
        > = HashMap::new();
        for entry in &metadata {
            let Some(generation) = entry.sender_bond_generation() else {
                continue;
            };
            if entry.sender.is_empty() || entry.sequence_number < 0 {
                continue;
            }
            evidence_groups
                .entry((
                    ValidatorSerde(entry.sender.clone()),
                    generation,
                    entry.sequence_number,
                ))
                .or_default()
                .insert(BlockHashSerde(entry.block_hash.clone()));
        }
        let expected_evidence = evidence_groups;
        self.equivocation_evidence_index.put(
            expected_evidence
                .iter()
                .map(|(key, hashes)| (key.clone(), hashes.clone()))
                .collect(),
        )?;
        let expected_evidence_keys: HashSet<_> = expected_evidence.into_keys().collect();
        let stale_evidence = self
            .equivocation_evidence_index
            .to_map()?
            .into_keys()
            .filter(|key| !expected_evidence_keys.contains(key))
            .collect::<Vec<_>>();
        if !stale_evidence.is_empty() {
            self.equivocation_evidence_index.delete(stale_evidence)?;
        }

        let expected_invalid = metadata
            .iter()
            .filter(|entry| entry.is_rejected())
            .map(|entry| (BlockHashSerde(entry.block_hash.clone()), entry.clone()))
            .collect::<HashMap<_, _>>();
        self.invalid_blocks_index.put(
            expected_invalid
                .iter()
                .map(|(hash, entry)| (hash.clone(), entry.clone()))
                .collect(),
        )?;
        let expected_invalid_hashes = expected_invalid.into_keys().collect::<HashSet<_>>();
        let stale_invalid = self
            .invalid_blocks_index
            .to_map()?
            .into_keys()
            .filter(|hash| !expected_invalid_hashes.contains(hash))
            .collect::<Vec<_>>();
        if !stale_invalid.is_empty() {
            self.invalid_blocks_index.delete(stale_invalid)?;
        }

        let mut expected: HashMap<Validator, (i32, BlockHash)> = registered
            .iter()
            .map(|validator| (validator.clone(), (-1, genesis_hash.clone())))
            .collect();
        for entry in metadata {
            if entry.sender.is_empty() || !registered.contains(&entry.sender) {
                continue;
            }
            if entry.sequence_number < 0 {
                return Err(KvStoreError::InvalidArgument(format!(
                    "cannot reconcile negative latest-message sequence {} for block {}",
                    entry.sequence_number,
                    PrettyPrinter::build_string_bytes(&entry.block_hash)
                )));
            }
            let candidate = (entry.sequence_number, entry.block_hash);
            expected
                .entry(entry.sender)
                .and_modify(|current| {
                    if candidate.0 > current.0
                        || (candidate.0 == current.0 && candidate.1 < current.1)
                    {
                        *current = candidate.clone();
                    }
                })
                .or_insert(candidate);
        }

        self.latest_messages_index.put(
            expected
                .iter()
                .map(|(validator, (_, hash))| {
                    (
                        ValidatorSerde(validator.clone()),
                        BlockHashSerde(hash.clone()),
                    )
                })
                .collect(),
        )?;
        let expected_validators: HashSet<ValidatorSerde> =
            expected.into_keys().map(ValidatorSerde).collect();
        let stale = self
            .latest_messages_index
            .to_map()?
            .into_keys()
            .filter(|validator| !expected_validators.contains(validator))
            .collect::<Vec<_>>();
        if !stale.is_empty() {
            self.latest_messages_index.delete(stale)?;
        }
        self.get_representation_internal()?
            .validate_latest_message_materialization()
    }

    // P2-16: the following three methods bypass `global_lock` — production
    // P2-16: the following methods bypass `global_lock` — production
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
        deploy_index: Arc<PlRwLock<KeyValueTypedStoreImpl<DeployId, BlockHashSerde>>>,
        deploy_occurrence_index: Arc<
            PlRwLock<KeyValueTypedStoreImpl<DeployId, BTreeSet<BlockHashSerde>>>,
        >,
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
            deploy_index,
            deploy_occurrence_store: DeployOccurrenceStore::activate_fresh(
                deploy_occurrence_index.read().raw_store().clone(),
            )
            .expect("fresh test deploy occurrence storage"),
            invalid_blocks_index,
            floor_index,
            frontier_index,
            equivocation_tracker_index,
            equivocation_evidence_index: KeyValueTypedStoreImpl::new(Arc::new(
                rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore::new(),
            )),
            finalization_ledger: FinalizationLedger::from_store(Arc::new(
                rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore::new(),
            )),
            lifecycle: Arc::new(PlRwLock::new(DeployLifecycleTables::in_memory())),
            dag_generation,
            ft_lower_bound: Arc::new(AtomicU32::new(0.0f32.to_bits())),
            genesis_hash_index: KeyValueTypedStoreImpl::new(Arc::new(
                rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore::new(),
            )),
        }
    }

    /// Test-only accessor for the deploy-index handle. The field itself
    /// is `pub(crate)` so production callers route through
    /// `lookup_by_deploy_id` / `insert`. Tests in other crates that need to
    /// inject corrupt entries (e.g. resolver / deploy-finalization-status
    /// regression tests) enable the `test-internals` feature to reach this
    /// accessor.
    #[cfg(any(test, feature = "test-internals"))]
    #[doc(hidden)]
    pub fn deploy_index_for_tests(
        &self,
    ) -> Arc<PlRwLock<KeyValueTypedStoreImpl<DeployId, BlockHashSerde>>> {
        self.deploy_index.clone()
    }

    #[cfg(any(test, feature = "test-internals"))]
    #[doc(hidden)]
    pub fn deploy_occurrence_store_for_tests(&self) -> DeployOccurrenceStore {
        self.deploy_occurrence_store.clone()
    }

    /// Test-only accessor for the block-metadata-index handle. Same
    /// rationale as `deploy_index_for_tests`.
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
        let equivocation_observations = self
            .equivocation_evidence_index
            .to_map()?
            .into_iter()
            .map(|((validator, generation, sequence), hashes)| {
                (
                    (validator.0, generation, sequence),
                    hashes.into_iter().map(|hash| hash.0).collect(),
                )
            })
            .collect();

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
            canonical_genesis_hash: self.genesis_hash_internal()?,
            latest_messages_map: latest_messages,
            child_map,
            height_map,
            block_number_map,
            main_parent_map,
            self_justification_map,
            invalid_blocks_set: invalid_blocks,
            equivocation_observations,
            last_finalized_block_hash: last_finalized_block,
            finalized_blocks_set: finalized_blocks,
            block_metadata_index: self.block_metadata_index.clone(),
            deploy_index: self.deploy_index.clone(),
            deploy_occurrence_store: self.deploy_occurrence_store.clone(),
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
        let result = if matches!(mode, InsertMode::ApprovedGenesis) {
            self.insert_internal_impl(block, mode, None, None)
        } else {
            #[cfg(any(test, feature = "test-internals"))]
            {
                let generation = block.header.sender_bond_generation.ok_or_else(|| {
                    KvStoreError::InvalidArgument(
                        "test block is missing sender bond generation".to_string(),
                    )
                })?;
                let stake = block
                    .body
                    .state
                    .bonds
                    .iter()
                    .find(|bond| bond.validator == block.sender && bond.stake > 0)
                    .map(|bond| bond.stake)
                    .unwrap_or(1);
                let certificate = test_sender_authority_certificate(block, generation, stake)?;
                let outcome = match mode {
                    InsertMode::Normal | InsertMode::SettledHistory => {
                        CertifiedAdmissionOutcome::accepted(block, &certificate)
                    }
                    InsertMode::Invalid => CertifiedAdmissionOutcome::rejected(
                        block,
                        &certificate,
                        AdmissionRejectionReason::InvalidTransaction,
                    ),
                    InsertMode::ApprovedGenesis => unreachable!(),
                }
                .map_err(|error| KvStoreError::InvalidArgument(error.to_string()))?;
                self.insert_internal_impl(block, mode, Some(&certificate), Some(&outcome))
            }
            #[cfg(not(any(test, feature = "test-internals")))]
            {
                Err(KvStoreError::InvalidArgument(
                    "normal and invalid block admission requires certified sender authority"
                        .to_string(),
                ))
            }
        };
        metrics::histogram!("dag.insert.time", "source" => "f1r3fly.casper.block-dag")
            .record(__insert_start.elapsed().as_secs_f64());
        result
    }

    fn repair_certified_objective_evidence(
        &self,
        block: &BlockMessage,
        certificate: &CertifiedSenderAuthority,
    ) -> Result<(), KvStoreError> {
        certificate
            .validate_for(block)
            .map_err(|error| KvStoreError::InvalidArgument(error.to_string()))?;
        if block.seq_num < 0 {
            return Ok(());
        }
        let key = (
            ValidatorSerde(block.sender.clone()),
            certificate.generation(),
            block.seq_num,
        );
        let mut observed = self
            .equivocation_evidence_index
            .get_one(&key)?
            .unwrap_or_default();
        observed.insert(BlockHashSerde(block.block_hash.clone()));
        self.equivocation_evidence_index
            .put_one(key, observed.clone())?;
        if observed.len() >= 2 {
            if let Some(base_sequence_number) = block.seq_num.checked_sub(1) {
                self.equivocation_tracker_index.ensure_identity(
                    block.sender.clone(),
                    certificate.generation(),
                    base_sequence_number,
                )?;
            }
        }
        Ok(())
    }

    /// Internal method to insert without acquiring lock.
    /// Used when lock is already held by the caller.
    /// Public to allow IndexedBlockDagStorage to use it.
    pub fn insert_internal(
        &self,
        block: &BlockMessage,
        mode: InsertMode,
    ) -> Result<KeyValueDagRepresentation, KvStoreError> {
        if matches!(mode, InsertMode::ApprovedGenesis) {
            return self.insert_internal_impl(block, mode, None, None);
        }
        #[cfg(any(test, feature = "test-internals"))]
        {
            let generation = block.header.sender_bond_generation.ok_or_else(|| {
                KvStoreError::InvalidArgument(
                    "test block is missing sender bond generation".to_string(),
                )
            })?;
            let stake = block
                .body
                .state
                .bonds
                .iter()
                .find(|bond| bond.validator == block.sender && bond.stake > 0)
                .map(|bond| bond.stake)
                .unwrap_or(1);
            let certificate = test_sender_authority_certificate(block, generation, stake)?;
            let outcome = match mode {
                InsertMode::Normal | InsertMode::SettledHistory => {
                    CertifiedAdmissionOutcome::accepted(block, &certificate)
                }
                InsertMode::Invalid => CertifiedAdmissionOutcome::rejected(
                    block,
                    &certificate,
                    AdmissionRejectionReason::InvalidTransaction,
                ),
                InsertMode::ApprovedGenesis => unreachable!(),
            }
            .map_err(|error| KvStoreError::InvalidArgument(error.to_string()))?;
            self.insert_internal_impl(block, mode, Some(&certificate), Some(&outcome))
        }
        #[cfg(not(any(test, feature = "test-internals")))]
        {
            Err(KvStoreError::InvalidArgument(
                "normal and invalid block admission requires certified sender authority"
                    .to_string(),
            ))
        }
    }

    pub fn insert_certified(
        &self,
        block: &BlockMessage,
        mode: InsertMode,
        certificate: &CertifiedSenderAuthority,
        outcome: &CertifiedAdmissionOutcome,
    ) -> Result<KeyValueDagRepresentation, KvStoreError> {
        if matches!(mode, InsertMode::ApprovedGenesis) {
            return Err(KvStoreError::InvalidArgument(
                "approved genesis must not carry a sender authority certificate".to_string(),
            ));
        }
        let __insert_start = std::time::Instant::now();
        let _lock_guard = self.global_lock.write();
        let result = self.insert_internal_certified(block, mode, certificate, outcome);
        metrics::histogram!("dag.insert.time", "source" => "f1r3fly.casper.block-dag")
            .record(__insert_start.elapsed().as_secs_f64());
        result
    }

    pub fn insert_internal_certified(
        &self,
        block: &BlockMessage,
        mode: InsertMode,
        certificate: &CertifiedSenderAuthority,
        outcome: &CertifiedAdmissionOutcome,
    ) -> Result<KeyValueDagRepresentation, KvStoreError> {
        if matches!(mode, InsertMode::ApprovedGenesis) {
            return Err(KvStoreError::InvalidArgument(
                "approved genesis must not carry a sender authority certificate".to_string(),
            ));
        }
        self.insert_internal_impl(block, mode, Some(certificate), Some(outcome))
    }

    fn insert_internal_impl(
        &self,
        block: &BlockMessage,
        mode: InsertMode,
        certificate: Option<&CertifiedSenderAuthority>,
        outcome: Option<&CertifiedAdmissionOutcome>,
    ) -> Result<KeyValueDagRepresentation, KvStoreError> {
        match (certificate, outcome, mode) {
            (None, None, InsertMode::ApprovedGenesis) => {}
            (
                Some(certificate),
                Some(outcome),
                InsertMode::Normal | InsertMode::Invalid | InsertMode::SettledHistory,
            ) => {
                certificate
                    .validate_for(block)
                    .map_err(|error| KvStoreError::InvalidArgument(error.to_string()))?;
                outcome
                    .validate_for(block, certificate)
                    .map_err(|error| KvStoreError::InvalidArgument(error.to_string()))?;
                let mode_matches = match mode {
                    InsertMode::Normal | InsertMode::SettledHistory => outcome.is_accepted(),
                    InsertMode::Invalid => outcome.is_rejected(),
                    InsertMode::ApprovedGenesis => false,
                };
                if !mode_matches {
                    return Err(KvStoreError::InvalidArgument(
                        "DAG insert mode disagrees with certified admission outcome".to_string(),
                    ));
                }
            }
            _ => {
                return Err(KvStoreError::InvalidArgument(
                    "DAG insertion requires exactly the certificates prescribed by its mode"
                        .to_string(),
                ))
            }
        }
        // Phase 8 (A-6): derive the per-branch booleans directly from the
        // enum via `matches!`. The previous `mode.flags()` projection
        // shim survived a Phase-4 transition; it is no longer needed.
        let invalid = matches!(mode, InsertMode::Invalid);
        let approved = matches!(mode, InsertMode::ApprovedGenesis);
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

        // Invalid blocks advance the sender's latest message for equivocation
        // evidence, but only accepted post-state bond caches may register new
        // validator slots.
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
        //   - Finalized-floor authority requires every committee validator to
        //     appear in a new block's justifications. Invalid senders therefore
        //     still advance their own slot, while arbitrary bonds declared by an
        //     invalid block cannot create new slots.
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
                .filter(|bond| bond.stake > 0)
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
                let placeholder = self.genesis_hash_internal()?.ok_or_else(|| {
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

        if approved {
            BlockMetadata::from_approved_genesis(block)
                .map_err(|error| KvStoreError::InvalidArgument(error.to_string()))?;
            if block.block_hash.len() != block_hash::LENGTH {
                return Err(KvStoreError::InvalidArgument(format!(
                    "Block hash {} is not correct length.",
                    PrettyPrinter::build_string_bytes(&block.block_hash)
                )));
            }
            self.record_genesis_hash_internal(block.block_hash.clone())?;
            self.finalization_ledger
                .ensure_genesis(block.block_hash.clone(), block.body.state.block_number)?;
        }

        let block_hash = block.block_hash.clone();
        if sender_has_invalid_format {
            return Err(KvStoreError::InvalidArgument(format!(
                "Block sender is malformed., Block: {:?}",
                block
            )));
        }
        if block_hash.len() != block_hash::LENGTH {
            return Err(KvStoreError::InvalidArgument(format!(
                "Block hash {} is not correct length.",
                PrettyPrinter::build_string_bytes(&block_hash)
            )));
        }
        if sender_is_empty {
            tracing::warn!("{}", log_empty_sender);
        }

        let block_metadata = if block_exists {
            let existing = self
                .block_metadata_index
                .read()
                .get(&block.block_hash)?
                .ok_or_else(|| {
                    KvStoreError::KeyNotFound(
                        "DAG membership exists without block metadata".to_string(),
                    )
                })?;
            if approved {
                let mut supplied = BlockMetadata::from_approved_genesis(block)
                    .map_err(|error| KvStoreError::InvalidArgument(error.to_string()))?;
                supplied.directly_finalized = existing.directly_finalized;
                supplied.finalized = existing.finalized;
                supplied.fault_tolerance_value = existing.fault_tolerance_value;
                if supplied != existing {
                    return Err(KvStoreError::InvalidArgument(
                        "duplicate approved genesis disagrees with immutable stored metadata"
                            .to_string(),
                    ));
                }
            }
            if let (Some(certificate), Some(outcome)) = (certificate, outcome) {
                if existing.sender_authority.as_ref() != Some(certificate) {
                    return Err(KvStoreError::InvalidArgument(
                        "stored block metadata disagrees with certified sender authority"
                            .to_string(),
                    ));
                }
                if existing.admission_outcome.as_ref() != Some(outcome) {
                    return Err(KvStoreError::InvalidArgument(
                        "stored block metadata disagrees with certified admission outcome"
                            .to_string(),
                    ));
                }
            }
            existing
        } else {
            match (certificate, outcome) {
                (Some(certificate), Some(outcome)) => {
                    BlockMetadata::from_certified_block(block, None, None, certificate, outcome)
                        .map_err(|error| KvStoreError::InvalidArgument(error.to_string()))?
                }
                (None, None) => BlockMetadata::from_approved_genesis(block)
                    .map_err(|error| KvStoreError::InvalidArgument(error.to_string()))?,
                _ => unreachable!(),
            }
        };
        let (metadata_key, metadata_value, metadata_store) = {
            let metadata_guard = self.block_metadata_index.read();
            let (key, value) = metadata_guard.encode_add(&block_metadata)?;
            (key, value, metadata_guard.raw_store().clone())
        };
        let mut owned_mutations: Vec<(Arc<dyn KeyValueStore>, Vec<u8>, AtomicStoreOperation)> =
            vec![(
                metadata_store,
                metadata_key,
                AtomicStoreOperation::PutIfAbsentOrEqual(metadata_value),
            )];

        if !invalid {
            let source_block_hash: [u8; 32] =
                block.block_hash.as_ref().try_into().map_err(|_| {
                    KvStoreError::InvalidArgument(
                        "deploy occurrence source block hash must be 32 bytes".to_string(),
                    )
                })?;
            for (ordinal, deploy) in block.body.deploys.iter().enumerate() {
                match deploy
                    .deploy_id_for_protocol(block.header.version)
                    .map_err(KvStoreError::InvalidArgument)?
                {
                    DeployLookupId::V6(deploy_id) => {
                        let (
                            admission_mode,
                            admission_ruleset_digest,
                            admission_context_digest,
                            sender_authority_digest,
                        ) = if approved {
                            (
                                OccurrenceAdmissionMode::ApprovedGenesis,
                                Vec::new(),
                                Vec::new(),
                                Vec::new(),
                            )
                        } else {
                            let outcome = outcome.ok_or_else(|| {
                                KvStoreError::InvalidArgument(
                                    "protocol-v6 occurrence requires certified admission"
                                        .to_string(),
                                )
                            })?;
                            (
                                if settled_history {
                                    OccurrenceAdmissionMode::SettledHistory
                                } else {
                                    OccurrenceAdmissionMode::Normal
                                },
                                outcome.ruleset_digest().to_vec(),
                                outcome.incoming_context_digest().to_vec(),
                                outcome.sender_authority_digest().to_vec(),
                            )
                        };
                        let plan =
                            self.deploy_occurrence_store
                                .prepare_insert(DeployOccurrence {
                                    schema_version: DEPLOY_OCCURRENCE_SCHEMA_VERSION,
                                    deploy_id,
                                    protocol_version: DEPLOY_OCCURRENCE_PROTOCOL_VERSION,
                                    source_block_hash,
                                    source_block_height: block.body.state.block_number,
                                    source_validator: if approved {
                                        Vec::new()
                                    } else {
                                        block.sender.to_vec()
                                    },
                                    deploy_ordinal: u32::try_from(ordinal).map_err(|_| {
                                        KvStoreError::InvalidArgument(
                                            "deploy occurrence ordinal exceeds u32".to_string(),
                                        )
                                    })?,
                                    admission_mode,
                                    admission_ruleset_digest,
                                    admission_context_digest,
                                    sender_authority_digest,
                                    is_failed: deploy.is_failed,
                                })?;
                        owned_mutations.extend(plan.mutations.into_iter().map(
                            |(key, operation)| {
                                (
                                    self.deploy_occurrence_store.raw_store().clone(),
                                    key,
                                    operation,
                                )
                            },
                        ));
                    }
                    DeployLookupId::Legacy(signature) => {
                        let deploy_id = signature.into_bytes();
                        let candidate_hash = block.block_hash.clone();
                        let selected_hash = match self.deploy_index.read().get_one(&deploy_id)? {
                            Some(existing) => {
                                let existing_hash: BlockHash = existing.into();
                                let existing_height = self
                                    .block_metadata_index
                                    .read()
                                    .get(&existing_hash)?
                                    .map(|metadata| metadata.block_number)
                                    .unwrap_or(i64::MIN);
                                if existing_height > block.body.state.block_number
                                    || (existing_height == block.body.state.block_number
                                        && existing_hash < candidate_hash)
                                {
                                    existing_hash
                                } else {
                                    candidate_hash
                                }
                            }
                            None => candidate_hash,
                        };
                        let deploy_index = self.deploy_index.read();
                        owned_mutations.push((
                            deploy_index.raw_store().clone(),
                            deploy_index.encode_key(&deploy_id)?,
                            AtomicStoreOperation::Put(
                                deploy_index.encode_value(&BlockHashSerde(selected_hash))?,
                            ),
                        ));
                    }
                }
            }

            let block_number = block.body.state.block_number;
            let lifecycle_guard = self.lifecycle.write();
            let mut lifecycle_rows: BTreeMap<DeployLookupId, (Option<Vec<u8>>, LifecycleEvents)> =
                BTreeMap::new();
            for deploy in &block.body.deploys {
                let deploy_id = deploy
                    .deploy_id_for_protocol(block.header.version)
                    .map_err(KvStoreError::InvalidArgument)?;
                if lifecycle_guard.get_terminal(&deploy_id)?.is_some() {
                    continue;
                }
                let entry = match lifecycle_rows.entry(deploy_id.clone()) {
                    std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        let current = lifecycle_guard.get_events(&deploy_id)?;
                        let encoded = current
                            .as_ref()
                            .map(|row| lifecycle_guard.events_store().encode_value(row))
                            .transpose()?;
                        entry.insert((
                            encoded,
                            current.unwrap_or(LifecycleEvents {
                                valid_after: None,
                                events: Vec::new(),
                            }),
                        ))
                    }
                };
                if entry.1.valid_after.is_none() {
                    entry.1.valid_after = Some(deploy.deploy.data.valid_after_block_number);
                }
                let event = LifecycleEvent {
                    height: block_number,
                    block_hash: block.block_hash.to_vec(),
                    kind: LifecycleEventKind::Included {
                        is_failed: deploy.is_failed,
                    },
                };
                if !entry.1.events.contains(&event) {
                    entry.1.events.push(event);
                }
            }
            for rejected in &block.body.rejected_deploys {
                let rejected_id = match (
                    block.header.version >= DEPLOY_OCCURRENCE_PROTOCOL_VERSION,
                    rejected.typed_deploy_id(),
                ) {
                    (true, DeployLookupId::V6(deploy_id)) => DeployLookupId::V6(*deploy_id),
                    (false, DeployLookupId::Legacy(signature)) => {
                        DeployLookupId::Legacy(signature.clone())
                    }
                    (true, DeployLookupId::Legacy(_)) => {
                        return Err(KvStoreError::InvalidArgument(
                            "protocol-v6 rejected deploy requires a v6 deploy identity".to_string(),
                        ));
                    }
                    (false, DeployLookupId::V6(_)) => {
                        return Err(KvStoreError::InvalidArgument(
                            "pre-v6 rejected deploy requires a legacy deploy identity".to_string(),
                        ));
                    }
                };
                if lifecycle_guard.get_terminal(&rejected_id)?.is_some() {
                    continue;
                }
                let entry = match lifecycle_rows.entry(rejected_id.clone()) {
                    std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        let current = lifecycle_guard.get_events(&rejected_id)?;
                        let encoded = current
                            .as_ref()
                            .map(|row| lifecycle_guard.events_store().encode_value(row))
                            .transpose()?;
                        entry.insert((
                            encoded,
                            current.unwrap_or(LifecycleEvents {
                                valid_after: None,
                                events: Vec::new(),
                            }),
                        ))
                    }
                };
                let event = LifecycleEvent {
                    height: block_number,
                    block_hash: block.block_hash.to_vec(),
                    kind: LifecycleEventKind::Rejected {
                        duplicate: rejected.is_duplicate(),
                        carrier: rejected.source_block_hash.to_vec(),
                    },
                };
                if !entry.1.events.contains(&event) {
                    entry.1.events.push(event);
                }
            }
            for (deploy_id, (expected, replacement)) in lifecycle_rows {
                let events_store = lifecycle_guard.events_store();
                owned_mutations.push((
                    events_store.raw_store().clone(),
                    events_store.encode_key(&deploy_id)?,
                    AtomicStoreOperation::CompareAndSwap {
                        expected,
                        replacement: Some(events_store.encode_value(&replacement)?),
                    },
                ));
            }
            let mutations = owned_mutations
                .iter()
                .map(|(store, key, operation)| AtomicStoreMutation {
                    store: store.as_ref(),
                    key: key.clone(),
                    operation: operation.clone(),
                })
                .collect::<Vec<_>>();
            commit_admission_mutations(block.header.version, &mutations)?;
            drop(lifecycle_guard);
        } else {
            let mutations = owned_mutations
                .iter()
                .map(|(store, key, operation)| AtomicStoreMutation {
                    store: store.as_ref(),
                    key: key.clone(),
                    operation: operation.clone(),
                })
                .collect::<Vec<_>>();
            commit_admission_mutations(block.header.version, &mutations)?;
        }

        if !block_exists {
            self.block_metadata_index
                .write()
                .apply_committed_add(block_metadata.clone());
            self.dag_generation.fetch_add(1, Ordering::Relaxed);
        }

        if let Some(certificate) = certificate {
            self.repair_certified_objective_evidence(block, certificate)?;
        }

        if block_exists {
            tracing::warn!("{}", log_already_stored);
            return self.get_representation_internal();
        }

        if invalid {
            self.invalid_blocks_index
                .put_one(block_hash.clone().into(), block_metadata)?;
        }

        if !settled_history {
            let sender_is_registered = !sender_is_empty
                && self
                    .latest_messages_index
                    .contains_key(ValidatorSerde(block.sender.clone()))?;
            let new_latest_from_sender = if !sender_is_empty && (!invalid || sender_is_registered) {
                // Add LM either if there is no existing message for the sender, or if sequence number advances
                // - assumes block sender is not valid hash
                if match self
                    .latest_messages_index
                    .get_one(&block.sender.clone().into())
                {
                    Ok(Some(latest_message_hash)) => {
                        let block_metadata_index_guard = self.block_metadata_index.read();
                        match block_metadata_index_guard.get(&latest_message_hash.into()) {
                            Ok(Some(metadata)) => {
                                block.seq_num > metadata.sequence_number
                                    || (block.seq_num == metadata.sequence_number
                                        && block.block_hash < metadata.block_hash)
                            }
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

            let mut new_latest_to_add = if invalid {
                HashMap::new()
            } else {
                new_latest_messages()?
            };
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
        // `record_directly_finalized`, `reconcile_finalization_projection`,
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

    pub fn finalization_head(&self) -> Result<Option<FinalizationHead>, KvStoreError> {
        self.finalization_ledger.head()
    }

    pub fn finalized_floor_certificate(
        &self,
    ) -> Result<Option<FinalizationCertificate>, KvStoreError> {
        let Some(head) = self.finalization_ledger.head()? else {
            return Ok(None);
        };
        self.finalized_floor_certificate_for_head(&head)
    }

    pub fn finalized_floor_certificate_for_head(
        &self,
        head: &FinalizationHead,
    ) -> Result<Option<FinalizationCertificate>, KvStoreError> {
        if head.revision == 0 {
            return Ok(None);
        }
        let witness = self
            .finalization_ledger
            .witness(&head.certificate_digest)?
            .ok_or_else(|| {
                KvStoreError::SerializationError(
                    "durable finalized head has no matching certificate witness".to_string(),
                )
            })?;
        let certificate = witness.to_certificate();
        if certificate.digest() != head.certificate_digest.0 {
            return Err(KvStoreError::SerializationError(
                "durable finalized head certificate digest does not match its witness".to_string(),
            ));
        }
        if certificate.target_floor_hash != head.block_hash
            || certificate.target_block_number != head.block_number
        {
            return Err(KvStoreError::SerializationError(
                "durable finalized head does not match its certificate target".to_string(),
            ));
        }
        Ok(Some(certificate))
    }

    pub fn capture_finalization_base(&self) -> Result<FinalizationBase, KvStoreError> {
        self.reconcile_finalization_projection()?;
        let _lock_guard = self.global_lock.read();
        let before = self
            .finalization_ledger
            .head()?
            .ok_or(KvStoreError::LastFinalizedBlockUninitialized)?;
        let dag = self.get_representation_internal()?;
        let dag_generation = self.dag_generation.load(Ordering::Acquire);
        let after = self
            .finalization_ledger
            .head()?
            .ok_or(KvStoreError::LastFinalizedBlockUninitialized)?;
        if before != after {
            return Err(KvStoreError::StaleFinalization {
                expected_revision: before.revision,
                actual_revision: after.revision,
            });
        }
        if dag.last_finalized_block() != before.block_hash.0 {
            return Err(KvStoreError::SerializationError(
                "projected finalized head does not match the durable finalization ledger"
                    .to_string(),
            ));
        }
        Ok(FinalizationBase {
            head: before,
            dag,
            dag_generation,
        })
    }

    pub fn committed_finalization_records(&self) -> Result<Vec<FinalizationRecord>, KvStoreError> {
        self.finalization_ledger.records_through_head()
    }

    pub fn pending_finalization_effect_records(
        &self,
    ) -> Result<Vec<FinalizationRecord>, KvStoreError> {
        self.finalization_ledger.pending_effect_records()
    }

    pub fn reconcile_finalization_effect_compaction(&self) -> Result<(), KvStoreError> {
        self.finalization_ledger.reconcile_effect_compaction()
    }

    pub fn finalization_effect_completed(
        &self,
        id: &FinalizationEffectId,
    ) -> Result<bool, KvStoreError> {
        self.finalization_ledger.effect_completed(id)
    }

    pub fn record_finalization_effect(&self, id: FinalizationEffectId) -> Result<(), KvStoreError> {
        self.finalization_ledger.record_effect(id)
    }

    pub fn record_finalization_round_effects_completed(
        &self,
        revision: u64,
    ) -> Result<u64, KvStoreError> {
        self.finalization_ledger
            .record_round_effects_completed(revision)
    }

    pub fn reconcile_finalization_projection(&self) -> Result<(), KvStoreError> {
        for record in self.finalization_ledger.pending_projection_records()? {
            let fault_tolerance = f32::from_bits(record.fault_tolerance_bits);
            let indirectly_finalized = record
                .finalized
                .iter()
                .filter(|hash| *hash != &record.directly_finalized)
                .map(|hash| hash.0.clone())
                .collect();
            {
                let _lock_guard = self.global_lock.write();
                let mut metadata = self.block_metadata_index.write();
                metadata.record_finalized(
                    record.directly_finalized.0.clone(),
                    indirectly_finalized,
                    fault_tolerance,
                )?;
                let current_lower_bound =
                    f32::from_bits(self.ft_lower_bound.load(Ordering::Relaxed));
                if fault_tolerance < current_lower_bound {
                    self.ft_lower_bound
                        .store(fault_tolerance.to_bits(), Ordering::Relaxed);
                }
            }
            self.propagate_ft_to_finalized_blocks(fault_tolerance)?;
            self.finalization_ledger
                .record_projection_completed(record.revision)?;
        }
        Ok(())
    }

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
        let base = self.capture_finalization_base()?;
        self.record_directly_finalized_atomic(
            &base.head,
            directly_finalized_hash,
            "root".to_string(),
            ft_value,
            move |_revision, finalized| finalization_effect(finalized),
        )
        .await
        .map(|_| ())
    }

    pub async fn record_directly_finalized_atomic<F, Fut>(
        &self,
        expected: &FinalizationHead,
        directly_finalized_hash: BlockHash,
        shard_id: String,
        ft_value: f32,
        finalization_effect: F,
    ) -> Result<FinalizationAppendOutcome, KvStoreError>
    where
        F: FnMut(u64, &HashSet<BlockHash>) -> Fut,
        Fut: std::future::Future<Output = Result<(), KvStoreError>>,
    {
        let dag = self.get_representation()?;
        let protocol_version = dag
            .lookup_unsafe(&directly_finalized_hash)?
            .protocol_version;
        let latest_messages: BTreeMap<ValidatorSerde, BlockHashSerde> = dag
            .latest_messages_map
            .iter()
            .map(|(validator, block_hash)| {
                (
                    ValidatorSerde(validator.clone()),
                    BlockHashSerde(block_hash.clone()),
                )
            })
            .collect();
        let zero = Bytes::from(vec![0; block_hash::LENGTH]);
        let (predecessor_certificate_digest, predecessor_certificate_block_hash) =
            if expected.revision == 0 {
                (BlockHashSerde(zero.clone()), BlockHashSerde(zero))
            } else {
                let predecessor_post_state =
                    dag.lookup_unsafe(&expected.block_hash.0)?.post_state_hash;
                let roots = latest_messages
                    .values()
                    .map(|hash| hash.0.clone())
                    .chain(std::iter::once(directly_finalized_hash.clone()))
                    .collect::<Vec<_>>();
                let mut remaining_work = FinalizationCertificate::MAX_DAG_VISITS_PER_VERIFICATION;
                let support = dag.certified_support_closure(
                    &expected.block_hash.0,
                    roots,
                    FinalizationCertificate::MAX_SUPPORTING_BLOCKS,
                    &mut remaining_work,
                )?;
                let mut selected = None;
                for carrier in support {
                    if carrier.0 == directly_finalized_hash {
                        continue;
                    }
                    if let Some(digest) = predecessor_certificate_carrier_digest(
                        &dag,
                        &carrier.0,
                        &expected.block_hash.0,
                        &predecessor_post_state,
                        protocol_version,
                    )? {
                        selected = Some((BlockHashSerde(digest), carrier));
                        break;
                    }
                }
                selected.ok_or_else(|| KvStoreError::FinalizationCertificateCarrierPending {
                    expected_revision: expected.revision,
                    floor_hash: expected.block_hash.0.to_vec(),
                    certificate_digest: expected.certificate_digest.0.to_vec(),
                })?
            };
        self.record_directly_finalized_certified_atomic(
            expected,
            directly_finalized_hash,
            ft_value,
            FinalizationWitnessInputs {
                protocol_version,
                shard_id,
                predecessor_certificate_digest,
                predecessor_certificate_block_hash,
                fault_tolerance_numerator: 0,
                fault_tolerance_denominator: 1,
                latest_messages,
                authority_context_digest: expected.record_digest.clone(),
            },
            finalization_effect,
        )
        .await
    }

    pub async fn record_directly_finalized_certified_atomic<F, Fut>(
        &self,
        expected: &FinalizationHead,
        directly_finalized_hash: BlockHash,
        ft_value: f32,
        witness_inputs: FinalizationWitnessInputs,
        mut finalization_effect: F,
    ) -> Result<FinalizationAppendOutcome, KvStoreError>
    where
        F: FnMut(u64, &HashSet<BlockHash>) -> Fut,
        Fut: std::future::Future<Output = Result<(), KvStoreError>>,
    {
        if witness_inputs.fault_tolerance_denominator <= 0 {
            return Err(KvStoreError::InvalidArgument(
                "finality certificate threshold denominator must be positive".to_string(),
            ));
        }
        self.reconcile_finalization_projection()?;
        let current = self
            .finalization_ledger
            .head()?
            .ok_or(KvStoreError::LastFinalizedBlockUninitialized)?;
        if current != *expected {
            return Err(KvStoreError::StaleFinalization {
                expected_revision: expected.revision,
                actual_revision: current.revision,
            });
        }
        if expected.block_hash.0 == directly_finalized_hash {
            if expected.revision == 0 {
                return Err(KvStoreError::InvalidArgument(
                    "approved genesis is already the durable finalized head".to_string(),
                ));
            }
            let record = self
                .finalization_ledger
                .record(expected.revision)?
                .ok_or_else(|| {
                    KvStoreError::SerializationError(format!(
                        "durable finalization head revision {} has no record",
                        expected.revision
                    ))
                })?;
            let finalized = record.finalized.into_iter().map(|hash| hash.0).collect();
            finalization_effect(expected.revision, &finalized).await?;
            return Ok(FinalizationAppendOutcome::AlreadyCommitted(
                expected.clone(),
            ));
        }
        let (block_number, target_post_state_hash, all_finalized, supporting_block_hashes) = {
            let _lock_guard = self.global_lock.read();
            let dag = self.get_representation_internal()?;
            if dag.last_finalized_block() != expected.block_hash.0 {
                return Err(KvStoreError::SerializationError(
                    "projected finalized head does not match the bound finalization base"
                        .to_string(),
                ));
            }
            if !dag.contains(&directly_finalized_hash) {
                return Err(KvStoreError::InvalidArgument(format!(
                    "Attempting to finalize nonexistent hash {}",
                    PrettyPrinter::build_string_bytes(&directly_finalized_hash)
                )));
            }
            let target_metadata = dag.lookup_unsafe(&directly_finalized_hash)?;
            let block_number = target_metadata.block_number;
            if !dag.is_dag_ancestor(&expected.block_hash.0, &directly_finalized_hash)? {
                return Err(KvStoreError::InvalidArgument(
                    "finalization candidate does not preserve the durable finalized floor"
                        .to_string(),
                ));
            }
            if !state_preservation::is_state_preserved(
                &dag,
                &expected.block_hash.0,
                &directly_finalized_hash,
            )? {
                return Err(KvStoreError::InvalidArgument(
                    "finalization candidate does not preserve the durable finalized state"
                        .to_string(),
                ));
            }
            let mut remaining_work = FinalizationCertificate::MAX_DAG_VISITS_PER_VERIFICATION;
            let finalized = dag
                .certified_finalized_delta(
                    &expected.block_hash.0,
                    &directly_finalized_hash,
                    FinalizationCertificate::MAX_FINALIZED_BLOCKS,
                    &mut remaining_work,
                )?
                .into_iter()
                .map(|hash| hash.0)
                .collect::<HashSet<_>>();
            let supporting_roots = witness_inputs
                .latest_messages
                .values()
                .map(|hash| hash.0.clone())
                .chain(std::iter::once(directly_finalized_hash.clone()))
                .collect::<Vec<_>>();
            let supporting = dag.certified_support_closure(
                &expected.block_hash.0,
                supporting_roots,
                FinalizationCertificate::MAX_SUPPORTING_BLOCKS,
                &mut remaining_work,
            )?;
            let predecessor_digest_is_zero = witness_inputs
                .predecessor_certificate_digest
                .0
                .iter()
                .all(|byte| *byte == 0);
            let predecessor_carrier_is_zero = witness_inputs
                .predecessor_certificate_block_hash
                .0
                .iter()
                .all(|byte| *byte == 0);
            if expected.revision == 0 {
                if !predecessor_digest_is_zero || !predecessor_carrier_is_zero {
                    return Err(KvStoreError::InvalidArgument(
                        "genesis predecessor must use the zero certificate carrier".to_string(),
                    ));
                }
            } else {
                if predecessor_digest_is_zero
                    || predecessor_carrier_is_zero
                    || !supporting.contains(&witness_inputs.predecessor_certificate_block_hash)
                {
                    return Err(KvStoreError::InvalidArgument(
                        "non-genesis predecessor requires a supported certificate carrier"
                            .to_string(),
                    ));
                }
                let predecessor_post_state =
                    dag.lookup_unsafe(&expected.block_hash.0)?.post_state_hash;
                let committed_digest = predecessor_certificate_carrier_digest(
                    &dag,
                    &witness_inputs.predecessor_certificate_block_hash.0,
                    &expected.block_hash.0,
                    &predecessor_post_state,
                    witness_inputs.protocol_version,
                )?
                .ok_or_else(|| {
                    KvStoreError::InvalidArgument(
                        "predecessor certificate carrier does not commit to the expected floor state"
                            .to_string(),
                    )
                })?;
                if committed_digest != witness_inputs.predecessor_certificate_digest.0 {
                    return Err(KvStoreError::InvalidArgument(
                        "predecessor certificate carrier and digest are not the same proof"
                            .to_string(),
                    ));
                }
            }
            (
                block_number,
                target_metadata.post_state_hash,
                finalized,
                supporting,
            )
        };
        let manifest: BTreeSet<BlockHashSerde> =
            all_finalized.iter().cloned().map(BlockHashSerde).collect();
        let genesis = self
            .finalization_ledger
            .genesis_anchor()?
            .ok_or(KvStoreError::LastFinalizedBlockUninitialized)?;
        let witness = FinalizationLedger::prepare_witness(
            witness_inputs.protocol_version,
            witness_inputs.shard_id,
            genesis.block_hash.0,
            expected,
            directly_finalized_hash.clone(),
            witness_inputs.predecessor_certificate_digest.0,
            witness_inputs.predecessor_certificate_block_hash.0,
            block_number,
            target_post_state_hash,
            witness_inputs.fault_tolerance_numerator,
            witness_inputs.fault_tolerance_denominator,
            witness_inputs.latest_messages,
            supporting_block_hashes.clone(),
            witness_inputs.authority_context_digest,
            manifest.clone(),
        )?;
        self.finalization_ledger
            .persist_witness(expected, &witness)?;
        let record = FinalizationLedger::prepare_record(
            expected,
            directly_finalized_hash.clone(),
            block_number,
            ft_value,
            manifest,
            &witness,
        )?;
        let outcome = self.finalization_ledger.try_append(expected, &record)?;
        let revision = match &outcome {
            FinalizationAppendOutcome::Committed(head)
            | FinalizationAppendOutcome::AlreadyCommitted(head) => head.revision,
            FinalizationAppendOutcome::Stale(head) => {
                return Err(KvStoreError::StaleFinalization {
                    expected_revision: expected.revision,
                    actual_revision: head.revision,
                });
            }
        };
        self.reconcile_finalization_projection()?;
        finalization_effect(revision, &all_finalized).await?;
        Ok(outcome)
    }

    fn propagate_ft_to_finalized_blocks(&self, ft_value: f32) -> Result<(), KvStoreError> {
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

fn commit_admission_mutations(
    protocol_version: i64,
    mutations: &[AtomicStoreMutation<'_>],
) -> Result<(), KvStoreError> {
    if protocol_version >= DEPLOY_OCCURRENCE_PROTOCOL_VERSION {
        if let [mutation] = mutations {
            if let AtomicStoreOperation::PutIfAbsentOrEqual(value) = &mutation.operation {
                if mutation
                    .store
                    .put_one_if_absent(mutation.key.clone(), value.clone())?
                {
                    return Ok(());
                }
                return match mutation.store.get_one(&mutation.key)? {
                    Some(existing) if existing == *value => Ok(()),
                    Some(_) => Err(KvStoreError::TransactionConflict(format!(
                        "existing value differs for key {}",
                        hex::encode(&mutation.key)
                    ))),
                    None => Err(KvStoreError::TransactionConflict(format!(
                        "atomic insert lost key {}",
                        hex::encode(&mutation.key)
                    ))),
                };
            }
        }
        return strict_atomic_mutate(mutations);
    }
    for mutation in mutations {
        let current = mutation.store.get_one(&mutation.key)?;
        match &mutation.operation {
            AtomicStoreOperation::Put(value) => {
                mutation
                    .store
                    .put_one(mutation.key.clone(), value.clone())?;
            }
            AtomicStoreOperation::PutIfAbsentOrEqual(value) => match current {
                Some(existing) if existing != *value => {
                    return Err(KvStoreError::TransactionConflict(format!(
                        "existing legacy value differs for key {}",
                        hex::encode(&mutation.key)
                    )));
                }
                Some(_) => {}
                None => mutation
                    .store
                    .put_one(mutation.key.clone(), value.clone())?,
            },
            AtomicStoreOperation::Delete => {
                mutation.store.delete(vec![mutation.key.clone()])?;
            }
            AtomicStoreOperation::CompareAndSwap {
                expected,
                replacement,
            } => {
                if current.as_ref() != expected.as_ref() {
                    return Err(KvStoreError::TransactionConflict(format!(
                        "legacy compare-and-swap expectation failed for key {}",
                        hex::encode(&mutation.key)
                    )));
                }
                match replacement {
                    Some(value) => mutation
                        .store
                        .put_one(mutation.key.clone(), value.clone())?,
                    None => {
                        mutation.store.delete(vec![mutation.key.clone()])?;
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod certified_closure_tests {
    use models::rust::block_hash::{self, BlockHash, BlockHashSerde};
    use models::rust::block_implicits;
    use models::rust::casper::protocol::casper_message::{
        BlockMessage, Bond, FinalizationCertificate, FinalizedFloorCommitment,
    };
    use models::rust::validator::{self, Validator};
    use proptest::prelude::*;
    use proptest::test_runner::{Config, TestRunner};
    use prost::bytes::Bytes;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

    use super::{BlockDagKeyValueStorage, InsertMode};
    use crate::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;

    fn rank(value: u8) -> BlockHash { Bytes::from(vec![value; block_hash::LENGTH]) }

    fn validator() -> Validator { Bytes::from(vec![7; validator::LENGTH]) }

    fn make_block(
        hash: u8,
        height: i64,
        parents: Vec<BlockHash>,
        floor: Option<(BlockHash, BlockHash, BlockHash)>,
    ) -> BlockMessage {
        let validator = validator();
        let mut block = block_implicits::get_random_block(
            Some(height),
            Some(i32::try_from(height).unwrap()),
            Some(rank(hash.saturating_add(20))),
            Some(rank(hash.saturating_add(40))),
            Some(validator.clone()),
            Some(models::rust::block_metadata::CERTIFIED_ADMISSION_PROTOCOL_VERSION),
            Some(height),
            Some(parents),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(vec![Bond {
                validator,
                stake: 1,
            }]),
            Some("root".to_string()),
            None,
        );
        block.block_hash = rank(hash);
        block.header.finalized_floor =
            floor.map(|(floor_hash, floor_post_state_hash, certificate_digest)| {
                FinalizedFloorCommitment {
                    floor_hash,
                    floor_post_state_hash,
                    certificate_digest,
                    authority_context_digest: rank(99),
                }
            });
        block
    }

    async fn fixture(include_ambient: bool) -> (KeyValueDagRepresentation, Vec<BlockHash>) {
        let mut kvm = InMemoryStoreManager::new();
        let storage = BlockDagKeyValueStorage::new(&mut kvm).await.unwrap();
        let genesis = make_block(0, 0, Vec::new(), None);
        storage
            .insert(&genesis, InsertMode::ApprovedGenesis)
            .unwrap();
        let predecessor = make_block(1, 1, vec![rank(0)], Some((rank(0), rank(40), rank(90))));
        let carrier = make_block(2, 2, vec![rank(1)], Some((rank(1), rank(41), rank(91))));
        let descendant = make_block(3, 3, vec![rank(2)], Some((rank(1), rank(41), rank(91))));
        let target = make_block(4, 4, vec![rank(3)], Some((rank(1), rank(41), rank(91))));
        for block in [&predecessor, &carrier, &descendant, &target] {
            storage.insert(block, InsertMode::Normal).unwrap();
        }
        if include_ambient {
            let ambient = make_block(5, 2, vec![rank(1)], Some((rank(1), rank(41), rank(91))));
            storage.insert(&ambient, InsertMode::Normal).unwrap();
        }
        (storage.get_representation().unwrap(), vec![
            rank(1),
            rank(2),
            rank(3),
            rank(4),
        ])
    }

    #[tokio::test]
    async fn certified_closures_use_objective_predecessor_history_and_ignore_ambient_blocks() {
        let (base, expected) = fixture(false).await;
        let (extended, _) = fixture(true).await;
        let mut base_work = FinalizationCertificate::MAX_DAG_VISITS_PER_VERIFICATION;
        let mut extended_work = FinalizationCertificate::MAX_DAG_VISITS_PER_VERIFICATION;
        let base_support = base
            .certified_support_closure(
                &rank(1),
                [rank(4)],
                FinalizationCertificate::MAX_SUPPORTING_BLOCKS,
                &mut base_work,
            )
            .unwrap();
        let extended_support = extended
            .certified_support_closure(
                &rank(1),
                [rank(4)],
                FinalizationCertificate::MAX_SUPPORTING_BLOCKS,
                &mut extended_work,
            )
            .unwrap();
        assert_eq!(base_support, extended_support);
        assert_eq!(
            base_support,
            expected.into_iter().map(BlockHashSerde).collect()
        );
        assert!(!base_support.contains(&BlockHashSerde(rank(0))));
        assert!(!extended_support.contains(&BlockHashSerde(rank(5))));

        let mut finalized_work = FinalizationCertificate::MAX_DAG_VISITS_PER_VERIFICATION;
        let finalized = extended
            .certified_finalized_delta(
                &rank(1),
                &rank(4),
                FinalizationCertificate::MAX_FINALIZED_BLOCKS,
                &mut finalized_work,
            )
            .unwrap();
        assert_eq!(
            finalized,
            [rank(2), rank(3), rank(4)]
                .into_iter()
                .map(BlockHashSerde)
                .collect()
        );
    }

    #[tokio::test]
    async fn certified_support_is_invariant_under_root_order_and_duplication() {
        let (dag, _) = fixture(true).await;
        let mut oracle_work = FinalizationCertificate::MAX_DAG_VISITS_PER_VERIFICATION;
        let oracle = dag
            .certified_support_closure(
                &rank(1),
                [rank(4), rank(0)],
                FinalizationCertificate::MAX_SUPPORTING_BLOCKS,
                &mut oracle_work,
            )
            .unwrap();
        let strategy = proptest::collection::vec(0usize..4, 0..32);
        let mut runner = TestRunner::new(Config::with_cases(256));
        runner
            .run(&strategy, |indices| {
                let mut roots = vec![rank(4), rank(0)];
                roots.extend(indices.into_iter().map(|index| rank(index as u8 + 1)));
                let mut remaining = FinalizationCertificate::MAX_DAG_VISITS_PER_VERIFICATION;
                let actual = dag
                    .certified_support_closure(
                        &rank(1),
                        roots,
                        FinalizationCertificate::MAX_SUPPORTING_BLOCKS,
                        &mut remaining,
                    )
                    .unwrap();
                prop_assert_eq!(&actual, &oracle);
                Ok(())
            })
            .unwrap();
    }

    #[tokio::test]
    async fn canonical_genesis_support_identity_is_stable_when_its_body_is_omitted() {
        let (full, _) = fixture(false).await;
        let mut restored = full.clone();
        restored.dag_set.remove(&rank(0));

        let mut full_work = FinalizationCertificate::MAX_DAG_VISITS_PER_VERIFICATION;
        let full_support = full
            .certified_support_closure(
                &rank(1),
                [rank(4), rank(0)],
                FinalizationCertificate::MAX_SUPPORTING_BLOCKS,
                &mut full_work,
            )
            .unwrap();
        let mut restored_work = FinalizationCertificate::MAX_DAG_VISITS_PER_VERIFICATION;
        let restored_support = restored
            .certified_support_closure(
                &rank(1),
                [rank(0), rank(4)],
                FinalizationCertificate::MAX_SUPPORTING_BLOCKS,
                &mut restored_work,
            )
            .unwrap();

        assert_eq!(restored_support, full_support);
        assert!(restored_support.contains(&BlockHashSerde(rank(0))));

        let mut missing_work = FinalizationCertificate::MAX_DAG_VISITS_PER_VERIFICATION;
        assert!(restored
            .certified_support_closure(
                &rank(1),
                [rank(6)],
                FinalizationCertificate::MAX_SUPPORTING_BLOCKS,
                &mut missing_work,
            )
            .is_err());
    }

    #[tokio::test]
    async fn canonical_genesis_latest_reader_is_invariant_when_its_body_is_omitted() {
        let (mut full, _) = fixture(false).await;
        let validator = validator();
        full.latest_messages_map.insert(validator.clone(), rank(0));
        let mut restored = full.clone();
        restored.dag_set.remove(&rank(0));

        assert!(full.latest_message(&validator).unwrap().is_none());
        assert!(restored.latest_message(&validator).unwrap().is_none());
        assert_eq!(
            full.latest_messages().unwrap(),
            restored.latest_messages().unwrap()
        );
        assert!(full.latest_messages().unwrap().is_empty());
    }

    #[tokio::test]
    async fn certified_closure_limits_fail_closed() {
        let (dag, _) = fixture(false).await;
        let mut no_work = 0;
        assert!(dag
            .certified_support_closure(&rank(1), [rank(4)], 4, &mut no_work)
            .is_err());
        let mut bounded_work = FinalizationCertificate::MAX_DAG_VISITS_PER_VERIFICATION;
        assert!(dag
            .certified_support_closure(&rank(1), [rank(4)], 3, &mut bounded_work)
            .is_err());
    }
}
