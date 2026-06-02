//! Equivocation detection — single-step `check_equivocations` plus the
//! neglected-equivocation RMW path.
//!
//! ## Responsibilities
//!
//! * [`EquivocationDetector::check_equivocations`] — checks whether a
//!   freshly-arrived block equivocates against its sender's prior
//!   creator-justification.
//! * [`EquivocationDetector::check_neglected_equivocations_with_update`]
//!   — atomic RMW over the equivocation tracker that mints
//!   [`EquivocationRecord`]s for blocks observing equivocations they
//!   failed to acknowledge (Bug #2 / T-9.2).
//! * [`NeglectedEquivocationOutcome`] — typed outcome of the neglected-
//!   equivocation pass (P2-15).
//!
//! ## Slashing-protocol position
//!
//! This module is the dispatcher between block validation and the
//! `EquivocationTrackerStore`. It does not mutate validator bonds —
//! that's the PoS contract's job. It records `EquivocationRecord`s
//! that the proposer layer later turns into `SlashDeploy`s.
//!
//! See `docs/theory/slashing/slashing-verification.md` §6 for the
//! detector's role in the full slashing protocol.

use std::collections::{BTreeMap, HashMap, HashSet};

use block_storage::rust::dag::block_dag_key_value_storage::{
    BlockDagKeyValueStorage, KeyValueDagRepresentation,
};
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::{BlockMessage, Bond};
use models::rust::equivocation_record::{EquivocationDiscoveryStatus, EquivocationRecord};
use models::rust::validator::Validator;
use rspace_plus_plus::rspace::history::Either;
use shared::rust::store::key_value_store::KvStoreError;

use crate::rust::block_status::{BlockError, InvalidBlock, ValidBlock};
use crate::rust::util::proto_util;
use crate::rust::ValidBlockProcessing;

/// Equivocation detection logic for blockchain consensus
pub struct EquivocationDetector;

/// P2-15: outcome of one pass of `check_neglected_equivocation`.
///
/// Replaces the prior `Result<bool, _>` shape — `bool` was overloaded with
/// the mutating side-effect (the tracker was updated even on `Ok(false)`),
/// and callers had to infer which branch fired from a single bit.
///
/// * `Neglected` — the block could observe an equivocation it failed to
///   acknowledge. The proposer is therefore complicit; the block is rejected
///   with `InvalidBlock::NeglectedEquivocation`.
/// * `DetectedAndRecorded(records)` — the block correctly observes one or
///   more equivocations; the tracker has been updated to record the witness.
///   `records` carries the post-update records for logging / telemetry.
/// * `Oblivious` — the block had no view of the equivocation (e.g. its
///   justifications precede the equivocation base). No tracker mutation
///   occurred. Validation accepts.
#[derive(Debug, Clone, PartialEq)]
pub enum NeglectedEquivocationOutcome {
    Neglected,
    DetectedAndRecorded(Vec<EquivocationRecord>),
    Oblivious,
}

/// Memoizes per-justification canonical-child resolution within a single
/// detection pass. The key is (justification block hash, equivocating
/// validator, equivocation-base seq); the value is the canonical child hash
/// (or `None` if no child exists above the base). Without this cache,
/// `is_equivocation_detectable` would re-walk the self-justification chain
/// O(N×J) times for every iteration of the outer record loop.
type CanonicalChildCache = HashMap<(BlockHash, Validator, i64), Option<BlockHash>>;

impl EquivocationDetector {
    pub async fn check_equivocations(
        requested_as_dependency: bool,
        block: &BlockMessage,
        dag: &KeyValueDagRepresentation,
    ) -> Result<ValidBlockProcessing, KvStoreError> {
        // P4-5: per-block hot path; demote info!→debug! per slashing audit.
        tracing::debug!("Calculate checkEquivocations.");

        let maybe_latest_message_of_creator_hash = dag.latest_message_hash(&block.sender);
        let maybe_creator_justification = Self::creator_justification_hash(block);
        let is_not_equivocation =
            maybe_creator_justification == maybe_latest_message_of_creator_hash;

        if is_not_equivocation {
            Ok(Either::Right(ValidBlock::Valid))
        } else if requested_as_dependency {
            Ok(Either::Left(BlockError::Invalid(
                InvalidBlock::AdmissibleEquivocation,
            )))
        } else {
            // C15 / Smell-5: render `None` as the literal `<none>` rather
            // than `unwrap_or_default()` (which prints `BlockHash`'s
            // default value — an empty `Bytes`, visually indistinguishable
            // from a zero-hash). Operators reading this log line need to
            // be able to tell "absent justification" from "all-zero hash".
            let sender = PrettyPrinter::build_string_no_limit(&block.sender);
            let creator_justification_hash = maybe_creator_justification
                .as_ref()
                .map(|hash| PrettyPrinter::build_string_no_limit(hash))
                .unwrap_or_else(|| "<none>".to_string());
            let latest_message_of_creator = maybe_latest_message_of_creator_hash
                .as_ref()
                .map(|hash| PrettyPrinter::build_string_no_limit(hash))
                .unwrap_or_else(|| "<none>".to_string());

            tracing::warn!(
                "Ignorable equivocation: sender is {}, creator justification is {}, latest message of creator is {}",
                sender,
                creator_justification_hash,
                latest_message_of_creator
            );

            Ok(Either::Left(BlockError::Invalid(
                InvalidBlock::IgnorableEquivocation,
            )))
        }
    }

    pub fn creator_justification_hash(block: &BlockMessage) -> Option<BlockHash> {
        proto_util::creator_justification_block_message(block)
            .map(|justification| justification.latest_block_hash)
    }

    pub async fn check_neglected_equivocations_with_update(
        block: &BlockMessage,
        dag: &KeyValueDagRepresentation,
        block_store: &KeyValueBlockStore,
        genesis: &BlockMessage,
        block_dag_storage: &BlockDagKeyValueStorage,
    ) -> Result<ValidBlockProcessing, KvStoreError> {
        // P4-5: per-block hot path; demote info!→debug! per slashing audit.
        tracing::debug!("Calculate checkNeglectedEquivocationsWithUpdate");

        let outcome =
            Self::check_neglected_equivocation(block, dag, block_store, genesis, block_dag_storage)
                .await?;

        // P2-15: the outcome enum makes the detect/record/oblivious decision
        // a first-class value. Callers convert it to a validation verdict;
        // the storage write happened atomically inside
        // `check_neglected_equivocation`'s closure.
        let status = match outcome {
            NeglectedEquivocationOutcome::Neglected => {
                Either::Left(BlockError::Invalid(InvalidBlock::NeglectedEquivocation))
            }
            NeglectedEquivocationOutcome::DetectedAndRecorded(_)
            | NeglectedEquivocationOutcome::Oblivious => Either::Right(ValidBlock::Valid),
        };

        Ok(status)
    }

    /// P2-15: replaces `is_neglected_equivocation_detected_with_update` (which
    /// returned `bool` while writing to durable storage — a name that hid the
    /// mutating side-effect). The returned `NeglectedEquivocationOutcome`
    /// names the three outcomes explicitly:
    ///
    /// * `Neglected` — block ignored an equivocation it was responsible to
    ///   detect. Validation must reject (`InvalidBlock::NeglectedEquivocation`).
    /// * `DetectedAndRecorded(records)` — block correctly observed (one or
    ///   more) equivocations and the tracker was updated to record this
    ///   witness. Caller receives the list of records that were updated for
    ///   logging / telemetry. Validation accepts the block.
    /// * `Oblivious` — block had no view of the equivocation; no tracker
    ///   mutation occurred. Validation accepts the block.
    ///
    /// The entire `tracker.data()` → decide → `tracker.add(...)` flow runs
    /// inside the `access_equivocations_tracker` closure to preserve Bug #2
    /// / T-9.2 atomicity.
    async fn check_neglected_equivocation(
        block: &BlockMessage,
        dag: &KeyValueDagRepresentation,
        block_store: &KeyValueBlockStore,
        genesis: &BlockMessage,
        block_dag_storage: &BlockDagKeyValueStorage,
    ) -> Result<NeglectedEquivocationOutcome, KvStoreError> {
        // C14 / Perf-6: hoist `latest_messages` and `bonds` out of the
        // per-equivocation-record loop. Both are computed solely from
        // `block`; recomputing them inside the loop is O(B+J) work per
        // record, for total O((B+J)·|records|). Hoisting collapses
        // this to O(B+J+|records|·setup) and removes a per-iteration
        // BTreeMap construction.
        let latest_messages = Self::to_latest_message_hashes(&block.justifications);
        let bonds = proto_util::bonds(block);

        block_dag_storage.access_equivocations_tracker(|tracker| {
            let equivocations = tracker.data()?;
            let mut canonical_child_cache = CanonicalChildCache::new();
            let mut recorded: Vec<EquivocationRecord> = Vec::new();
            for equivocation_record in equivocations {
                let status = Self::get_equivocation_discovery_status(
                    dag,
                    block_store,
                    &equivocation_record,
                    genesis,
                    &mut canonical_child_cache,
                    &latest_messages,
                    &bonds,
                )?;
                match status {
                    EquivocationDiscoveryStatus::EquivocationNeglected => {
                        return Ok(NeglectedEquivocationOutcome::Neglected);
                    }
                    EquivocationDiscoveryStatus::EquivocationDetected => {
                        let mut updated = equivocation_record.clone();
                        updated
                            .equivocation_detected_block_hashes
                            .insert(block.block_hash.clone());
                        tracker.add(updated.clone())?;
                        // P4-5: detection is a (rare) consensus event; keep info!
                        // here so operators see the per-block record but use the
                        // structured `target: "f1r3fly.slashing"` namespace so
                        // ops can filter without grepping for the message text.
                        tracing::info!(
                            target: "f1r3fly.slashing",
                            block = %PrettyPrinter::build_string_no_limit(&block.block_hash),
                            "Equivocation detected and tracker updated"
                        );
                        recorded.push(updated);
                    }
                    EquivocationDiscoveryStatus::EquivocationOblivious => {}
                }
            }
            if recorded.is_empty() {
                Ok(NeglectedEquivocationOutcome::Oblivious)
            } else {
                Ok(NeglectedEquivocationOutcome::DetectedAndRecorded(recorded))
            }
        })
    }

    // C14 / Perf-6: `block` is no longer needed here — `latest_messages`
    // and `bonds` (both derived from `block`) are precomputed once
    // by the caller (`check_neglected_equivocation`) and passed in.
    fn get_equivocation_discovery_status(
        dag: &KeyValueDagRepresentation,
        block_store: &KeyValueBlockStore,
        equivocation_record: &EquivocationRecord,
        genesis: &BlockMessage,
        canonical_child_cache: &mut CanonicalChildCache,
        latest_messages: &BTreeMap<Validator, BlockHash>,
        bonds: &[Bond],
    ) -> Result<EquivocationDiscoveryStatus, KvStoreError> {
        let equivocating_validator = &equivocation_record.equivocator;

        let maybe_equivocating_validator_bond = bonds
            .iter()
            .find(|bond| bond.validator == *equivocating_validator);

        match maybe_equivocating_validator_bond {
            Some(bond) => Self::get_equivocation_discovery_status_for_bonded_validator(
                dag,
                block_store,
                equivocation_record,
                latest_messages,
                bond.stake,
                genesis,
                canonical_child_cache,
            ),
            None => {
                // P5 (slashing audit): a validator absent from the bond map
                // who appears as an equivocator is a degenerate case — the
                // detector still classifies as EquivocationDetected, but
                // operators should be alerted because this can indicate a
                // bond-map / equivocation-tracker desync (rare; Bug #5 was
                // the original site of this branch).
                tracing::warn!(
                    target: "f1r3fly.slashing",
                    validator = %hex::encode(&equivocation_record.equivocator),
                    base_seq = equivocation_record.equivocation_base_block_seq_num,
                    "unbonded equivocation detected (validator absent from bond map)"
                );
                Ok(EquivocationDiscoveryStatus::EquivocationDetected)
            }
        }
    }

    fn get_equivocation_discovery_status_for_bonded_validator(
        dag: &KeyValueDagRepresentation,
        block_store: &KeyValueBlockStore,
        equivocation_record: &EquivocationRecord,
        latest_messages: &BTreeMap<Validator, BlockHash>,
        stake: i64,
        genesis: &BlockMessage,
        canonical_child_cache: &mut CanonicalChildCache,
    ) -> Result<EquivocationDiscoveryStatus, KvStoreError> {
        if stake > 0 {
            let equivocation_detectable = Self::is_equivocation_detectable(
                dag,
                block_store,
                latest_messages,
                equivocation_record,
                &[],
                genesis,
                canonical_child_cache,
            )?;

            if equivocation_detectable {
                Ok(EquivocationDiscoveryStatus::EquivocationNeglected)
            } else {
                Ok(EquivocationDiscoveryStatus::EquivocationOblivious)
            }
        } else {
            Ok(EquivocationDiscoveryStatus::EquivocationDetected)
        }
    }

    /// Project a block's justification list into a `validator -> latest-hash`
    /// map. **`BTreeMap` is consensus-critical here** — every node iterates
    /// the map below in `is_equivocation_detectable`, and `HashMap` iteration
    /// order leaks `RandomState` entropy into consensus, leading to divergent
    /// classifications across nodes. Do not switch to `HashMap`.
    fn to_latest_message_hashes(
        justifications: &[models::rust::casper::protocol::casper_message::Justification],
    ) -> BTreeMap<Validator, BlockHash> {
        justifications
            .iter()
            .map(|justification| {
                (
                    justification.validator.clone(),
                    justification.latest_block_hash.clone(),
                )
            })
            .collect()
    }

    fn is_equivocation_detectable(
        dag: &KeyValueDagRepresentation,
        block_store: &KeyValueBlockStore,
        latest_messages: &BTreeMap<Validator, BlockHash>,
        equivocation_record: &EquivocationRecord,
        equivocation_children: &[BlockMessage],
        genesis: &BlockMessage,
        canonical_child_cache: &mut CanonicalChildCache,
    ) -> Result<bool, KvStoreError> {
        // P2-11: mutate a single owned Vec in place instead of returning a
        // fresh Vec from each helper. Eliminates O(n) clones per
        // justification (9 sites collapsed into a single allocation).
        let mut updated_equivocation_children: Vec<BlockMessage> = equivocation_children.to_vec();
        let equivocating_validator = &equivocation_record.equivocator;
        let equivocation_base_block_seq_num = equivocation_record.equivocation_base_block_seq_num;

        for justification_block_hash in latest_messages.values() {
            if equivocation_record
                .equivocation_detected_block_hashes
                .contains(justification_block_hash)
            {
                return Ok(true);
            }

            let Some(justification_block) = block_store.get(justification_block_hash)? else {
                continue;
            };

            Self::maybe_add_equivocation_child(
                dag,
                block_store,
                &justification_block,
                equivocating_validator,
                equivocation_base_block_seq_num.into(),
                &mut updated_equivocation_children,
                genesis,
                canonical_child_cache,
            )?;

            if updated_equivocation_children.len() > 1 {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// P2-11: mutates `equivocation_children` in place — returns `Ok(true)`
    /// iff a new equivocation child was appended.
    fn maybe_add_equivocation_child(
        dag: &KeyValueDagRepresentation,
        block_store: &KeyValueBlockStore,
        justification_block: &BlockMessage,
        equivocating_validator: &Validator,
        equivocation_base_block_seq_num: i64,
        equivocation_children: &mut Vec<BlockMessage>,
        genesis: &BlockMessage,
        canonical_child_cache: &mut CanonicalChildCache,
    ) -> Result<bool, KvStoreError> {
        // Genesis is unconditionally the equivocation root, never a child of
        // it. Returning early keeps the (genesis, validator, seq) cache key
        // out of the cache.
        if justification_block.block_hash == genesis.block_hash {
            return Ok(false);
        }

        if justification_block.sender == *equivocating_validator {
            let justification_seq_num = i64::from(justification_block.seq_num);
            if justification_seq_num > equivocation_base_block_seq_num {
                Self::add_equivocation_child(
                    dag,
                    block_store,
                    justification_block,
                    equivocating_validator,
                    equivocation_base_block_seq_num,
                    equivocation_children,
                    canonical_child_cache,
                )
            } else {
                Ok(false)
            }
        } else {
            let latest_messages =
                Self::to_latest_message_hashes(&justification_block.justifications);

            // A missing latest-message for the equivocating validator (no
            // entry in `latest_messages`, or the referenced hash not in the
            // store) is treated as *obliviousness*, not as a store
            // inconsistency — the prior code returned `Err(KeyNotFound)` here
            // and rejected the block. Per §9.x of the design we now let the
            // detection pass continue: the block simply contributes no
            // equivocation child via this justification.
            match latest_messages.get(equivocating_validator) {
                Some(latest_equivocating_validator_block_hash) => {
                    match block_store.get(latest_equivocating_validator_block_hash)? {
                        Some(latest_equivocating_validator_block) => {
                            let latest_seq_num =
                                i64::from(latest_equivocating_validator_block.seq_num);
                            if latest_seq_num > equivocation_base_block_seq_num {
                                Self::add_equivocation_child(
                                    dag,
                                    block_store,
                                    &latest_equivocating_validator_block,
                                    equivocating_validator,
                                    equivocation_base_block_seq_num,
                                    equivocation_children,
                                    canonical_child_cache,
                                )
                            } else {
                                Ok(false)
                            }
                        }
                        None => Ok(false),
                    }
                }
                None => Ok(false),
            }
        }
    }

    /// P2-11: mutates `equivocation_children` in place — returns `Ok(true)`
    /// iff a new (deduplicated) equivocation child was appended.
    fn add_equivocation_child(
        dag: &KeyValueDagRepresentation,
        block_store: &KeyValueBlockStore,
        justification_block: &BlockMessage,
        equivocating_validator: &Validator,
        equivocation_base_block_seq_num: i64,
        equivocation_children: &mut Vec<BlockMessage>,
        canonical_child_cache: &mut CanonicalChildCache,
    ) -> Result<bool, KvStoreError> {
        let key = (
            justification_block.block_hash.clone(),
            equivocating_validator.clone(),
            equivocation_base_block_seq_num,
        );
        let maybe_equivocation_child_hash = match canonical_child_cache.get(&key) {
            Some(cached) => cached.clone(),
            None => {
                let computed = Self::find_canonical_creator_justification_child_above_seq(
                    dag,
                    justification_block,
                    equivocating_validator,
                    equivocation_base_block_seq_num,
                )?;
                canonical_child_cache.insert(key, computed.clone());
                computed
            }
        };

        match maybe_equivocation_child_hash {
            Some(equivocation_child_hash) => match block_store.get(&equivocation_child_hash)? {
                Some(equivocation_child) => {
                    let already_present = equivocation_children
                        .iter()
                        .any(|child| child.block_hash == equivocation_child.block_hash);
                    if !already_present {
                        equivocation_children.push(equivocation_child);
                        Ok(true)
                    } else {
                        Ok(false)
                    }
                }
                None => Ok(false),
            },
            None => Ok(false),
        }
    }

    /// Walk the self-justification chain upward from `block`, returning the
    /// **oldest** ancestor authored by `target_validator` whose sequence
    /// number still exceeds `base_seq_num`. This is the canonical child of
    /// the equivocation base — the block we hold against the validator when
    /// deciding whether an equivocation has been observed by `block`'s
    /// causal cone.
    ///
    /// The `visited` set is a defensive cycle guard against
    /// byzantine-crafted self-justifications, **not** genuine DAG cycles —
    /// honest blocks form a strict DAG. A cycle here would loop forever
    /// without it; treat the first repeat as the end of the walk.
    fn find_canonical_creator_justification_child_above_seq(
        dag: &KeyValueDagRepresentation,
        block: &BlockMessage,
        target_validator: &Validator,
        base_seq_num: i64,
    ) -> Result<Option<BlockHash>, KvStoreError> {
        if block.sender != *target_validator || i64::from(block.seq_num) <= base_seq_num {
            return Ok(None);
        }

        let mut candidate_hash = block.block_hash.clone();
        let mut current_hash = block.block_hash.clone();
        let mut visited = HashSet::new();

        loop {
            if !visited.insert(current_hash.clone()) {
                break;
            }

            let Some(parent_hash) = dag.self_justification(&current_hash)? else {
                break;
            };

            match dag.lookup_unsafe(&parent_hash) {
                Ok(parent_metadata)
                    if parent_metadata.sender == *target_validator
                        && i64::from(parent_metadata.sequence_number) > base_seq_num =>
                {
                    candidate_hash = parent_hash.clone();
                    current_hash = parent_hash;
                }
                Ok(_) => break,
                // Storage failure during canonical-child walk: propagate
                // rather than absorbing as a normal walk termination — the
                // previous `_ => break` would silently truncate the search
                // and produce an incorrect canonical child, defeating the
                // detector's totality claim (T-9.11).
                Err(e) => return Err(e),
            }
        }

        Ok(Some(candidate_hash))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Arc;

    use block_storage::rust::dag::block_metadata_store::BlockMetadataStore;
    use models::rust::block_hash;
    use models::rust::block_metadata::BlockMetadata;
    use models::rust::casper::protocol::casper_message::{
        Body, F1r3flyState, Header, Justification,
    };
    use parking_lot::RwLock;
    use prost::bytes::Bytes;
    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
    use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

    use super::*;

    fn validator(id: u8) -> Validator {
        Bytes::from(vec![id])
    }

    fn hash(id: u8) -> BlockHash {
        Bytes::from(vec![id; block_hash::LENGTH])
    }

    fn block(
        sender: &Validator,
        seq_num: i32,
        block_hash: BlockHash,
        self_parent: Option<BlockHash>,
    ) -> BlockMessage {
        let justifications = self_parent
            .map(|latest_block_hash| {
                vec![Justification {
                    validator: sender.clone(),
                    latest_block_hash,
                }]
            })
            .unwrap_or_default();

        BlockMessage {
            block_hash,
            header: Header {
                parents_hash_list: Vec::new(),
                timestamp: 0,
                version: 0,
                extra_bytes: Bytes::new(),
            },
            body: Body {
                state: F1r3flyState {
                    pre_state_hash: Bytes::new(),
                    post_state_hash: Bytes::new(),
                    bonds: Vec::new(),
                    block_number: i64::from(seq_num),
                },
                deploys: Vec::new(),
                rejected_deploys: Vec::new(),
                system_deploys: Vec::new(),
                extra_bytes: Bytes::new(),
            },
            justifications,
            sender: sender.clone(),
            seq_num,
            sig: Bytes::new(),
            sig_algorithm: String::new(),
            shard_id: String::new(),
            extra_bytes: Bytes::new(),
        }
    }

    fn metadata(block: &BlockMessage, block_number: i64) -> BlockMetadata {
        BlockMetadata {
            block_hash: block.block_hash.clone(),
            parents: Vec::new(),
            sender: block.sender.clone(),
            justifications: block.justifications.clone(),
            weight_map: BTreeMap::new(),
            block_number,
            sequence_number: block.seq_num,
            invalid: false,
            directly_finalized: false,
            finalized: false,
            fault_tolerance_value: 0.0,
        }
    }

    fn dag_with(blocks: &[BlockMessage]) -> KeyValueDagRepresentation {
        let metadata_store = KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new()));
        let block_metadata_index = Arc::new(RwLock::new(BlockMetadataStore::new(metadata_store)));
        let deploy_index = Arc::new(RwLock::new(KeyValueTypedStoreImpl::new(Arc::new(
            InMemoryKeyValueStore::new(),
        ))));

        let mut dag = KeyValueDagRepresentation {
            dag_set: imbl::HashSet::new(),
            latest_messages_map: imbl::HashMap::new(),
            child_map: imbl::HashMap::new(),
            height_map: imbl::OrdMap::new(),
            block_number_map: imbl::HashMap::new(),
            main_parent_map: imbl::HashMap::new(),
            self_justification_map: imbl::HashMap::new(),
            invalid_blocks_set: imbl::HashSet::new(),
            last_finalized_block_hash: BlockHash::new(),
            finalized_blocks_set: imbl::HashSet::new(),
            block_metadata_index,
            deploy_index,
        };

        for (index, block) in blocks.iter().enumerate() {
            let block_number = index as i64;
            dag.dag_set.insert(block.block_hash.clone());
            dag.block_number_map
                .insert(block.block_hash.clone(), block_number);
            dag.height_map
                .entry(block_number)
                .or_insert_with(imbl::HashSet::new)
                .insert(block.block_hash.clone());
            if let Some(self_parent) = EquivocationDetector::creator_justification_hash(block) {
                dag.self_justification_map
                    .insert(block.block_hash.clone(), self_parent);
            }
            dag.block_metadata_index
                .write()
                .add(metadata(block, block_number))
                .unwrap();
        }

        dag
    }

    fn block_store_with(blocks: &[BlockMessage]) -> KeyValueBlockStore {
        let store = KeyValueBlockStore::new(
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
        );
        for block in blocks {
            store.put_block_message(block).unwrap();
        }
        store
    }

    #[test]
    fn latest_messages_are_projected_in_validator_order() {
        let justifications = vec![
            Justification {
                validator: validator(3),
                latest_block_hash: hash(30),
            },
            Justification {
                validator: validator(1),
                latest_block_hash: hash(10),
            },
            Justification {
                validator: validator(2),
                latest_block_hash: hash(20),
            },
        ];

        let latest_messages = EquivocationDetector::to_latest_message_hashes(&justifications);
        let validators: Vec<_> = latest_messages.keys().cloned().collect();

        assert_eq!(validators, vec![validator(1), validator(2), validator(3)]);
    }

    #[test]
    fn iterative_detection_skips_missing_latest_pointer_and_continues() {
        let sender = validator(1);
        let observer = validator(2);
        let missing = validator(3);
        let b0 = block(&sender, 0, hash(10), None);
        let left = block(&sender, 10, hash(20), Some(b0.block_hash.clone()));
        let right = block(&sender, 10, hash(30), Some(b0.block_hash.clone()));
        let mut observer_block = block(&observer, 1, hash(40), None);
        observer_block.justifications = vec![Justification {
            validator: sender.clone(),
            latest_block_hash: right.block_hash.clone(),
        }];

        let dag = dag_with(&[
            b0.clone(),
            left.clone(),
            right.clone(),
            observer_block.clone(),
        ]);
        let block_store = block_store_with(&[left.clone(), right.clone(), observer_block.clone()]);
        let latest_messages = BTreeMap::from([
            (missing, hash(99)),
            (observer, observer_block.block_hash.clone()),
        ]);
        let record = EquivocationRecord::new(sender.clone(), 0, BTreeSet::new());
        let mut cache = CanonicalChildCache::new();

        let detected = EquivocationDetector::is_equivocation_detectable(
            &dag,
            &block_store,
            &latest_messages,
            &record,
            &[left],
            &b0,
            &mut cache,
        )
        .unwrap();

        assert!(detected);
    }

    #[test]
    fn canonical_child_returns_oldest_visible_block_above_base() {
        let sender = validator(1);
        let b0 = block(&sender, 0, hash(10), None);
        let b2 = block(&sender, 2, hash(20), Some(b0.block_hash.clone()));
        let b100 = block(&sender, 100, hash(30), Some(b2.block_hash.clone()));
        let dag = dag_with(&[b0, b2.clone(), b100.clone()]);

        let found = EquivocationDetector::find_canonical_creator_justification_child_above_seq(
            &dag, &b100, &sender, 0,
        )
        .unwrap();

        assert_eq!(found, Some(b2.block_hash));
    }

    #[test]
    fn canonical_child_collapses_same_branch_latest_messages() {
        let sender = validator(1);
        let b0 = block(&sender, 0, hash(10), None);
        let b10 = block(&sender, 10, hash(20), Some(b0.block_hash.clone()));
        let b11 = block(&sender, 11, hash(30), Some(b10.block_hash.clone()));
        let dag = dag_with(&[b0, b10.clone(), b11.clone()]);

        let from_10 = EquivocationDetector::find_canonical_creator_justification_child_above_seq(
            &dag, &b10, &sender, 0,
        )
        .unwrap();
        let from_11 = EquivocationDetector::find_canonical_creator_justification_child_above_seq(
            &dag, &b11, &sender, 0,
        )
        .unwrap();

        assert_eq!(from_10, Some(b10.block_hash.clone()));
        assert_eq!(from_11, Some(b10.block_hash));
    }

    #[test]
    fn canonical_child_distinguishes_two_visible_branches() {
        let sender = validator(1);
        let b0 = block(&sender, 0, hash(10), None);
        let left = block(&sender, 10, hash(20), Some(b0.block_hash.clone()));
        let right = block(&sender, 10, hash(30), Some(b0.block_hash.clone()));
        let dag = dag_with(&[b0, left.clone(), right.clone()]);

        let left_found =
            EquivocationDetector::find_canonical_creator_justification_child_above_seq(
                &dag, &left, &sender, 0,
            )
            .unwrap();
        let right_found =
            EquivocationDetector::find_canonical_creator_justification_child_above_seq(
                &dag, &right, &sender, 0,
            )
            .unwrap();

        assert_eq!(left_found, Some(left.block_hash));
        assert_eq!(right_found, Some(right.block_hash));
        assert_ne!(left_found, right_found);
    }

    #[test]
    fn canonical_child_cycle_guard_terminates() {
        let sender = validator(1);
        let b2 = block(&sender, 2, hash(20), Some(hash(30)));
        let b3 = block(&sender, 3, hash(30), Some(hash(20)));
        let dag = dag_with(&[b2.clone(), b3.clone()]);

        let found = EquivocationDetector::find_canonical_creator_justification_child_above_seq(
            &dag, &b3, &sender, 0,
        )
        .unwrap();

        assert!(found.is_some());
    }

    #[test]
    fn canonical_child_cache_is_transparent_for_add_child() {
        let sender = validator(1);
        let b0 = block(&sender, 0, hash(10), None);
        let b10 = block(&sender, 10, hash(20), Some(b0.block_hash.clone()));
        let b11 = block(&sender, 11, hash(30), Some(b10.block_hash.clone()));
        let dag = dag_with(&[b0, b10.clone(), b11.clone()]);
        let block_store = block_store_with(&[b10.clone(), b11.clone()]);
        let mut cache = CanonicalChildCache::new();

        // P2-11: helper mutates `children` in place; the boolean return
        // reflects whether anything was appended.
        let mut children: Vec<BlockMessage> = Vec::new();
        let added = EquivocationDetector::add_equivocation_child(
            &dag,
            &block_store,
            &b11,
            &sender,
            0,
            &mut children,
            &mut cache,
        )
        .unwrap();

        assert!(added);
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].block_hash, b10.block_hash);
        assert_eq!(
            cache.get(&(b11.block_hash, sender, 0)).cloned(),
            Some(Some(children[0].block_hash.clone()))
        );
    }
}
