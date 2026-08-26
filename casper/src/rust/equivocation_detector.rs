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
use models::rust::bond_generation::BondGeneration;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::BlockMessage;
use models::rust::equivocation_record::{EquivocationDiscoveryStatus, EquivocationRecord};
use models::rust::validator::Validator;
use rspace_plus_plus::rspace::history::Either;
use shared::rust::store::key_value_store::KvStoreError;

use crate::rust::block_status::{BlockError, EquivocationObservation, InvalidBlock, ValidBlock};
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
/// validator, bond generation, equivocation-base seq); the value is the canonical child hash
/// (or `None` if no child exists above the base). Without this cache,
/// `is_equivocation_detectable` would re-walk the self-justification chain
/// O(N×J) times for every iteration of the outer record loop.
type CanonicalChildCache = HashMap<(BlockHash, Validator, BondGeneration, i64), Option<BlockHash>>;

impl EquivocationDetector {
    pub async fn check_equivocations(
        requested_as_dependency: bool,
        block: &BlockMessage,
        dag: &KeyValueDagRepresentation,
    ) -> Result<Option<EquivocationObservation>, KvStoreError> {
        // P4-5: per-block hot path; demote info!→debug! per slashing audit.
        tracing::debug!("Calculate checkEquivocations.");

        let maybe_latest_message_of_creator_hash = dag.latest_message_hash(&block.sender);
        let maybe_creator_justification = Self::creator_justification_hash(block);
        let is_not_equivocation =
            maybe_creator_justification == maybe_latest_message_of_creator_hash;

        if is_not_equivocation {
            Ok(None)
        } else if requested_as_dependency {
            Ok(Some(EquivocationObservation::RequestedDependency))
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

            Ok(Some(EquivocationObservation::Unsolicited))
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
        pre_state_bonds: &HashMap<Validator, i64>,
        pre_state_generations: &HashMap<Validator, BondGeneration>,
    ) -> Result<ValidBlockProcessing, KvStoreError> {
        // P4-5: per-block hot path; demote info!→debug! per slashing audit.
        tracing::debug!("Calculate checkNeglectedEquivocationsWithUpdate");

        let outcome = Self::check_neglected_equivocation(
            block,
            dag,
            block_store,
            genesis,
            block_dag_storage,
            pre_state_bonds,
            pre_state_generations,
        )
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
        pre_state_bonds: &HashMap<Validator, i64>,
        pre_state_generations: &HashMap<Validator, BondGeneration>,
    ) -> Result<NeglectedEquivocationOutcome, KvStoreError> {
        // C14 / Perf-6: hoist `latest_messages` out of the
        // per-equivocation-record loop and receive the canonical pre-state
        // bonds once from the validation dispatcher.
        let latest_messages = Self::to_latest_message_hashes(&block.justifications);

        block_dag_storage.access_equivocations_tracker(|tracker| {
            let equivocations = tracker.data()?;
            let mut canonical_child_cache = CanonicalChildCache::new();
            // FV audit #6: post-fix the EquivocationDetected arm is unreachable and
            // never pushes, so `recorded` is always empty and the pass returns
            // Oblivious. The Vec + DetectedAndRecorded return path is retained (the
            // enum variant and its caller arm stay live) but is now dead at runtime.
            let recorded: Vec<EquivocationRecord> = Vec::new();
            for equivocation_record in equivocations {
                let status = Self::get_equivocation_discovery_status(
                    dag,
                    block_store,
                    &equivocation_record,
                    genesis,
                    &mut canonical_child_cache,
                    &latest_messages,
                    pre_state_bonds,
                    pre_state_generations,
                )?;
                match status {
                    EquivocationDiscoveryStatus::EquivocationNeglected => {
                        return Ok(NeglectedEquivocationOutcome::Neglected);
                    }
                    // FV audit #6 remediation (unbonded-window record pollution
                    // fork). Post-fix, `get_equivocation_discovery_status` NEVER
                    // returns `EquivocationDetected`: the unbonded/stake-0 branches
                    // (:280 / :311) now return `EquivocationOblivious`, and the
                    // bonded stake>0 branch returns only Neglected/Oblivious. This
                    // arm is therefore UNREACHABLE. We keep it — the enum variant
                    // still exists (models::…::EquivocationDiscoveryStatus) — but it
                    // must NOT mutate the record: stamping observer block hashes into
                    // an unbonded offender's witness set was the root cause of the
                    // observation-order-dependent NeglectedEquivocation consensus
                    // fork (docs/theory/slashing/design/12-failure-modes.md
                    // §12.2.1a). The body is a strict no-op over `tracker` and
                    // `recorded`, so `recorded` stays empty ⇒ the pass resolves to
                    // Oblivious. Reaching it at all is a regression; alert loudly.
                    EquivocationDiscoveryStatus::EquivocationDetected => {
                        tracing::warn!(
                            target: "f1r3fly.slashing",
                            block = %PrettyPrinter::build_string_no_limit(&block.block_hash),
                            validator = %hex::encode(&equivocation_record.equivocator),
                            base_seq = equivocation_record.equivocation_base_block_seq_num,
                            "unexpected EquivocationDetected post-fix (regression): witness \
                             stamping suppressed, record left untouched (FV audit #6)"
                        );
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

    // C14 / Perf-6: `block` is no longer needed here. `latest_messages` is
    // projected once from the candidate while bond authority comes from the
    // candidate's immutable merged pre-state.
    fn get_equivocation_discovery_status(
        dag: &KeyValueDagRepresentation,
        block_store: &KeyValueBlockStore,
        equivocation_record: &EquivocationRecord,
        genesis: &BlockMessage,
        canonical_child_cache: &mut CanonicalChildCache,
        latest_messages: &BTreeMap<Validator, BlockHash>,
        pre_state_bonds: &HashMap<Validator, i64>,
        pre_state_generations: &HashMap<Validator, BondGeneration>,
    ) -> Result<EquivocationDiscoveryStatus, KvStoreError> {
        let equivocating_validator = &equivocation_record.equivocator;

        if pre_state_generations.get(equivocating_validator)
            != Some(&equivocation_record.equivocator_bond_generation)
        {
            return Ok(EquivocationDiscoveryStatus::EquivocationOblivious);
        }

        match pre_state_bonds.get(equivocating_validator) {
            Some(stake) => Self::get_equivocation_discovery_status_for_bonded_validator(
                dag,
                block_store,
                equivocation_record,
                latest_messages,
                *stake,
                genesis,
                canonical_child_cache,
            ),
            None => {
                // P5 (slashing audit): a validator absent from the bond map
                // who appears as an equivocator is a degenerate case. Operators
                // should still be alerted because this can indicate a bond-map /
                // equivocation-tracker desync (rare; Bug #5 was the original site
                // of this branch), so the warn! below is KEPT.
                //
                // FV audit #6 remediation: the return is now
                // EquivocationOblivious (was EquivocationDetected). An unbonded
                // offender has no stake to slash and, per
                // slashing-specification.md §11.6, must never be recorded. Since
                // the caller only stamps the record on EquivocationDetected,
                // returning Oblivious leaves the (empty) witness set untouched —
                // no witness is recorded — which closes the observation-order-
                // dependent NeglectedEquivocation fork (§12.2.1a). Detectability
                // then rests solely on the deterministic
                // `updated_equivocation_children.len() > 1` mechanism.
                tracing::warn!(
                    target: "f1r3fly.slashing",
                    validator = %hex::encode(&equivocation_record.equivocator),
                    base_seq = equivocation_record.equivocation_base_block_seq_num,
                    "unbonded equivocation observed (validator absent from bond map); \
                     classified Oblivious, no witness recorded (FV audit #6)"
                );
                Ok(EquivocationDiscoveryStatus::EquivocationOblivious)
            }
        }
    }

    /// Resolve the discovery status for an offender that IS present in the
    /// block's bond map.
    ///
    /// * `stake > 0` (bonded): `EquivocationNeglected` iff the equivocation is
    ///   detectable in the observing block's latest-message view, else
    ///   `EquivocationOblivious`.
    /// * `stake == 0` (unbonded/unbonding): `EquivocationOblivious` — **no
    ///   witness recorded** (FV audit #6; `slashing-specification.md §11.6`).
    ///   Returning Oblivious (rather than the pre-fix `EquivocationDetected`)
    ///   makes the caller's stamping arm unreachable, so the witness set stays
    ///   empty and the observation-order-dependent NeglectedEquivocation fork
    ///   (`§12.2.1a`) cannot arise.
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
            // FV audit #6 remediation: a stake-0 (bonded-but-unbonding) offender
            // ⇒ EquivocationOblivious (was EquivocationDetected). No stake to
            // slash and, per slashing-specification.md §11.6, nothing to record;
            // the caller stamps only on EquivocationDetected, so returning
            // Oblivious keeps the witness set empty and prevents the
            // observation-order-dependent NeglectedEquivocation fork (§12.2.1a).
            Ok(EquivocationDiscoveryStatus::EquivocationOblivious)
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
        let equivocator_bond_generation = equivocation_record.equivocator_bond_generation;
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
                equivocator_bond_generation,
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
        equivocator_bond_generation: BondGeneration,
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
            if justification_block.header.sender_bond_generation
                != Some(equivocator_bond_generation)
            {
                return Ok(false);
            }
            let justification_seq_num = i64::from(justification_block.seq_num);
            if justification_seq_num > equivocation_base_block_seq_num {
                Self::add_equivocation_child(
                    dag,
                    block_store,
                    justification_block,
                    equivocating_validator,
                    equivocator_bond_generation,
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
                                    equivocator_bond_generation,
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
        equivocator_bond_generation: BondGeneration,
        equivocation_base_block_seq_num: i64,
        equivocation_children: &mut Vec<BlockMessage>,
        canonical_child_cache: &mut CanonicalChildCache,
    ) -> Result<bool, KvStoreError> {
        let key = (
            justification_block.block_hash.clone(),
            equivocating_validator.clone(),
            equivocator_bond_generation,
            equivocation_base_block_seq_num,
        );
        let maybe_equivocation_child_hash = match canonical_child_cache.get(&key) {
            Some(cached) => cached.clone(),
            None => {
                let computed = Self::find_canonical_creator_justification_child_above_seq(
                    dag,
                    justification_block,
                    equivocating_validator,
                    equivocator_bond_generation,
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
        target_bond_generation: BondGeneration,
        base_seq_num: i64,
    ) -> Result<Option<BlockHash>, KvStoreError> {
        if block.sender != *target_validator
            || block.header.sender_bond_generation != Some(target_bond_generation)
            || i64::from(block.seq_num) <= base_seq_num
        {
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
                        && parent_metadata.sender_bond_generation()
                            == Some(target_bond_generation)
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

    fn validator(id: u8) -> Validator { Bytes::from(vec![id; models::rust::validator::LENGTH]) }

    fn hash(id: u8) -> BlockHash { Bytes::from(vec![id; block_hash::LENGTH]) }

    fn block(
        sender: &Validator,
        seq_num: i32,
        block_hash: BlockHash,
        self_parent: Option<BlockHash>,
    ) -> BlockMessage {
        block_in_generation(
            sender,
            BondGeneration::GENESIS,
            seq_num,
            block_hash,
            self_parent,
        )
    }

    fn block_in_generation(
        sender: &Validator,
        bond_generation: BondGeneration,
        seq_num: i32,
        block_hash: BlockHash,
        self_parent: Option<BlockHash>,
    ) -> BlockMessage {
        let pre_state_hash = self_parent.clone().unwrap_or_else(|| block_hash.clone());
        let post_state_hash = block_hash.clone();
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
                version: crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
                extra_bytes: Bytes::new(),
                sender_bond_generation: Some(bond_generation),
                objective_equivocation_evidence_delta: Vec::new(),
            },
            body: Body {
                state: F1r3flyState {
                    pre_state_hash,
                    post_state_hash,
                    bonds: Vec::new(),
                    bond_generations: Vec::new(),
                    active_validators: Vec::new(),
                    block_number: i64::from(seq_num),
                },
                deploys: Vec::new(),
                rejected_deploys: Vec::new(),
                rejected_state_effects: Vec::new(),
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
        crate::rust::test_metadata::certify(
            BlockMetadata {
                block_hash: block.block_hash.clone(),
                post_state_hash: block.body.state.post_state_hash.clone(),
                parents: Vec::new(),
                sender: block.sender.clone(),
                justifications: block.justifications.clone(),
                weight_map: BTreeMap::new(),
                bond_generation_map: BTreeMap::from([(
                    block.sender.clone(),
                    block.header.sender_bond_generation.unwrap(),
                )]),
                active_validator_set: BTreeSet::from([block.sender.clone()]),
                block_number,
                sequence_number: block.seq_num,
                admission_outcome: None,
                directly_finalized: false,
                finalized: false,
                fault_tolerance_value: 0.0,
                successful_state_effect_indices: Default::default(),
                rejected_state_effects: Default::default(),
                protocol_version: crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
                objective_equivocation_evidence_delta: Vec::new(),
                sender_authority: None,
                admission_schema_version: models::rust::block_metadata::ADMISSION_SCHEMA_VERSION,
                approved_genesis: false,
            },
            block.header.sender_bond_generation.unwrap(),
        )
    }

    fn dag_with(blocks: &[BlockMessage]) -> KeyValueDagRepresentation {
        let metadata_store = KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new()));
        let block_metadata_index = Arc::new(RwLock::new(
            BlockMetadataStore::new(metadata_store).unwrap(),
        ));
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
            equivocation_observations: imbl::HashMap::new(),
            last_finalized_block_hash: BlockHash::new(),
            finalized_blocks_set: imbl::HashSet::new(),
            block_metadata_index,
            deploy_index,
            deploy_occurrence_index: Arc::new(RwLock::new(KeyValueTypedStoreImpl::new(Arc::new(
                InMemoryKeyValueStore::new(),
            )))),
            floor_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
            frontier_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
        };

        for (index, block) in blocks.iter().enumerate() {
            let block_number = index as i64;
            dag.dag_set.insert(block.block_hash.clone());
            dag.block_number_map
                .insert(block.block_hash.clone(), block_number);
            dag.height_map
                .entry(block_number)
                .or_default()
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
        let record =
            EquivocationRecord::new(sender.clone(), BondGeneration::GENESIS, 0, BTreeSet::new());
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
            &dag,
            &b100,
            &sender,
            BondGeneration::GENESIS,
            0,
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
            &dag,
            &b10,
            &sender,
            BondGeneration::GENESIS,
            0,
        )
        .unwrap();
        let from_11 = EquivocationDetector::find_canonical_creator_justification_child_above_seq(
            &dag,
            &b11,
            &sender,
            BondGeneration::GENESIS,
            0,
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
                &dag,
                &left,
                &sender,
                BondGeneration::GENESIS,
                0,
            )
            .unwrap();
        let right_found =
            EquivocationDetector::find_canonical_creator_justification_child_above_seq(
                &dag,
                &right,
                &sender,
                BondGeneration::GENESIS,
                0,
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
            &dag,
            &b3,
            &sender,
            BondGeneration::GENESIS,
            0,
        )
        .unwrap();

        assert!(found.is_some());
    }

    #[test]
    fn canonical_child_does_not_cross_bond_generation_boundary() {
        let sender = validator(1);
        let generation_one = BondGeneration::new(1).unwrap();
        let generation_zero_parent = block(&sender, 1, hash(20), None);
        let generation_one_child = block_in_generation(
            &sender,
            generation_one,
            2,
            hash(30),
            Some(generation_zero_parent.block_hash.clone()),
        );
        let dag = dag_with(&[generation_zero_parent, generation_one_child.clone()]);

        let found = EquivocationDetector::find_canonical_creator_justification_child_above_seq(
            &dag,
            &generation_one_child,
            &sender,
            generation_one,
            0,
        )
        .unwrap();

        assert_eq!(found, Some(generation_one_child.block_hash));
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
            BondGeneration::GENESIS,
            0,
            &mut children,
            &mut cache,
        )
        .unwrap();

        assert!(added);
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].block_hash, b10.block_hash);
        assert_eq!(
            cache
                .get(&(b11.block_hash, sender, BondGeneration::GENESIS, 0))
                .cloned(),
            Some(Some(children[0].block_hash.clone()))
        );
    }

    #[test]
    fn unbonded_equivocation_discovery_is_oblivious() {
        let sender = validator(1);
        let genesis = block(&sender, 0, hash(10), None);
        let dag = dag_with(std::slice::from_ref(&genesis));
        let block_store = block_store_with(std::slice::from_ref(&genesis));
        let record =
            EquivocationRecord::new(sender.clone(), BondGeneration::GENESIS, 0, BTreeSet::new());
        let mut cache = CanonicalChildCache::new();

        let status = EquivocationDetector::get_equivocation_discovery_status(
            &dag,
            &block_store,
            &record,
            &genesis,
            &mut cache,
            &BTreeMap::new(),
            &HashMap::new(),
            &HashMap::from([(sender, BondGeneration::GENESIS)]),
        )
        .expect("discovery status");

        assert_eq!(status, EquivocationDiscoveryStatus::EquivocationOblivious);
    }

    #[test]
    fn zero_stake_equivocation_discovery_is_oblivious() {
        let sender = validator(1);
        let genesis = block(&sender, 0, hash(10), None);
        let dag = dag_with(std::slice::from_ref(&genesis));
        let block_store = block_store_with(std::slice::from_ref(&genesis));
        let record =
            EquivocationRecord::new(sender.clone(), BondGeneration::GENESIS, 0, BTreeSet::new());
        let mut cache = CanonicalChildCache::new();
        let bonds = HashMap::from([(sender.clone(), 0)]);

        let status = EquivocationDetector::get_equivocation_discovery_status(
            &dag,
            &block_store,
            &record,
            &genesis,
            &mut cache,
            &BTreeMap::new(),
            &bonds,
            &HashMap::from([(sender, BondGeneration::GENESIS)]),
        )
        .expect("discovery status");

        assert_eq!(status, EquivocationDiscoveryStatus::EquivocationOblivious);
    }

    #[test]
    fn stale_generation_record_is_noninterfering_with_current_authority() {
        let sender = validator(1);
        let generation_one = BondGeneration::new(1).unwrap();
        let base = block(&sender, 0, hash(10), None);
        let left = block(&sender, 1, hash(20), Some(base.block_hash.clone()));
        let right = block(&sender, 1, hash(30), Some(base.block_hash.clone()));
        let dag = dag_with(&[base.clone(), left.clone(), right.clone()]);
        let block_store = block_store_with(&[base.clone(), left.clone(), right.clone()]);
        let record =
            EquivocationRecord::new(sender.clone(), BondGeneration::GENESIS, 0, BTreeSet::new());
        let latest_messages = BTreeMap::from([
            (validator(2), left.block_hash),
            (validator(3), right.block_hash),
        ]);
        let bonds = HashMap::from([(sender.clone(), 100)]);

        let stale_status = EquivocationDetector::get_equivocation_discovery_status(
            &dag,
            &block_store,
            &record,
            &base,
            &mut CanonicalChildCache::new(),
            &latest_messages,
            &bonds,
            &HashMap::from([(sender.clone(), generation_one)]),
        )
        .unwrap();
        let matching_status = EquivocationDetector::get_equivocation_discovery_status(
            &dag,
            &block_store,
            &record,
            &base,
            &mut CanonicalChildCache::new(),
            &latest_messages,
            &bonds,
            &HashMap::from([(sender, BondGeneration::GENESIS)]),
        )
        .unwrap();

        assert_eq!(
            stale_status,
            EquivocationDiscoveryStatus::EquivocationOblivious
        );
        assert_eq!(
            matching_status,
            EquivocationDiscoveryStatus::EquivocationNeglected
        );
    }

    #[test]
    fn traversal_ignores_equivocation_children_from_other_generations() {
        let sender = validator(1);
        let generation_one = BondGeneration::new(1).unwrap();
        let base = block_in_generation(&sender, generation_one, 0, hash(10), None);
        let generation_zero_child = block(&sender, 1, hash(20), None);
        let generation_one_child = block_in_generation(
            &sender,
            generation_one,
            1,
            hash(30),
            Some(base.block_hash.clone()),
        );
        let dag = dag_with(&[
            base.clone(),
            generation_zero_child.clone(),
            generation_one_child.clone(),
        ]);
        let block_store =
            block_store_with(&[generation_zero_child.clone(), generation_one_child.clone()]);
        let record = EquivocationRecord::new(sender, generation_one, 0, BTreeSet::new());
        let latest_messages = BTreeMap::from([
            (validator(2), generation_zero_child.block_hash),
            (validator(3), generation_one_child.block_hash),
        ]);

        let detectable = EquivocationDetector::is_equivocation_detectable(
            &dag,
            &block_store,
            &latest_messages,
            &record,
            &[],
            &base,
            &mut CanonicalChildCache::new(),
        )
        .unwrap();

        assert!(!detectable);
    }

    // ══════════════════════════════════════════════════════════════════════
    // Tier-0 dynamic verification (slashing FV audit finding #6):
    //   unbonded-window record pollution fork — REMEDIATION SHIPPED.
    //
    // Historical mechanism (pre-fix): while an equivocator V was stake-0/
    // unbonded, its EquivocationRecord (minted with an EMPTY witness set — e.g.
    // the UnauthorizedSlashDeploy record `EquivocationRecord::new(V, seq-1, {})`)
    // resolved to `EquivocationDetected` in `get_equivocation_discovery_status`
    // (:280 unbonded / :311 stake-0), and the caller
    // `check_neglected_equivocation` then STAMPED the currently-validated
    // block's hash into that record. Every block validated during the unbonded
    // window polluted V's record; once V re-bonded, `is_equivocation_detectable`
    // returned TRUE for ANY later block whose justifications cited a stamped hash
    // — even a perfectly honest block — classifying it `EquivocationNeglected`.
    // Because different nodes stamped different hashes (observation-order
    // dependent), two honest nodes could DISAGREE — a consensus fork.
    //
    // Fix (candidate a): the unbonded/stake-0 branches now return
    // `EquivocationOblivious`, so the caller's stamping arm is UNREACHABLE, the
    // witness set stays empty, and detectability reduces to the deterministic
    // `updated_equivocation_children.len() > 1` mechanism ⇒ all nodes agree.
    //
    // The three tests below now pin the FIX: (1) an unbonded validator resolves
    // to Oblivious (no stamp), (2) an honest block over the (never-polluted)
    // record resolves Oblivious, and (3) two nodes observing in different orders
    // CONVERGE to the same empty record and the same verdict.
    // ══════════════════════════════════════════════════════════════════════

    // (1) SOURCE (now neutralized): an unbonded validator's record resolves to
    // `EquivocationOblivious`, so the caller's stamping arm (which only fires on
    // `EquivocationDetected`) is unreachable — nothing is stamped.
    #[test]
    fn tier0_unbonded_validator_discovery_is_oblivious_no_stamp() {
        let v = validator(1);
        let observer_block_hash = hash(77);
        let record =
            EquivocationRecord::new(v.clone(), BondGeneration::GENESIS, 0, BTreeSet::new());
        let genesis = block(&v, 0, hash(10), None);
        let dag = dag_with(&[genesis.clone()]);
        let block_store = block_store_with(&[genesis.clone()]);
        let latest_messages = BTreeMap::from([(v.clone(), observer_block_hash)]);
        let mut cache = CanonicalChildCache::new();
        let bonds = HashMap::new();

        let status = EquivocationDetector::get_equivocation_discovery_status(
            &dag,
            &block_store,
            &record,
            &genesis,
            &mut cache,
            &latest_messages,
            &bonds,
            &HashMap::from([(v.clone(), BondGeneration::GENESIS)]),
        )
        .unwrap();

        assert_eq!(
            status,
            EquivocationDiscoveryStatus::EquivocationOblivious,
            "FV audit #6 fix: unbonded validator ⇒ Oblivious ⇒ the caller's stamping arm is unreachable (no witness recorded)"
        );
        // A discovery-status query never mutates the record (only the now-
        // unreachable Detected caller-arm stamps): the witness set stays empty.
        assert!(
            record.equivocation_detected_block_hashes.is_empty(),
            "unbonded discovery must leave the witness set empty"
        );
    }

    // (2) NO FALSE POSITIVE (post-fix): drive the ACTUAL detector over an
    // unbonded offender V and an observer that cites V's honest chain tip. The
    // discovery status is Oblivious, so the record's witness set stays EMPTY,
    // and the honest block therefore resolves Oblivious — never a spurious
    // NeglectedEquivocation. (Pre-fix, the unbonded observation stamped the
    // record and a later citing block was falsely flagged.)
    #[test]
    fn tier0_polluted_record_falsely_neglects_honest_block() {
        let v = validator(1);
        // Honest single chain: b0 (seq 0) → b1 (seq 1). No equivocation.
        let b0 = block(&v, 0, hash(10), None);
        let b1 = block(&v, 1, hash(11), Some(b0.block_hash.clone()));
        let dag = dag_with(&[b0.clone(), b1.clone()]);
        let block_store = block_store_with(&[b0.clone(), b1.clone()]);
        // A later observer cites V's real latest message (b1) while V is UNBONDED.
        let latest_messages = BTreeMap::from([(v.clone(), b1.block_hash.clone())]);
        let record =
            EquivocationRecord::new(v.clone(), BondGeneration::GENESIS, 0, BTreeSet::new());
        let mut cache = CanonicalChildCache::new();
        let bonds = HashMap::new();

        // Post-fix: the unbonded offender resolves to Oblivious, so the caller
        // never stamps — the witness set stays EMPTY.
        let status = EquivocationDetector::get_equivocation_discovery_status(
            &dag,
            &block_store,
            &record,
            &b0,
            &mut cache,
            &latest_messages,
            &bonds,
            &HashMap::from([(v.clone(), BondGeneration::GENESIS)]),
        )
        .unwrap();
        assert_eq!(
            status,
            EquivocationDiscoveryStatus::EquivocationOblivious,
            "unbonded offender ⇒ Oblivious"
        );
        assert!(
            record.equivocation_detected_block_hashes.is_empty(),
            "FV audit #6 fix: an unbonded observation records NO witness"
        );

        // With the witness set empty (as it now always is for an unbonded
        // offender), the honest block is NOT detectable — it resolves Oblivious,
        // never a spurious NeglectedEquivocation.
        let mut cache2 = CanonicalChildCache::new();
        let detectable = EquivocationDetector::is_equivocation_detectable(
            &dag,
            &block_store,
            &latest_messages,
            &record,
            &[],
            &b0,
            &mut cache2,
        )
        .unwrap();
        assert!(
            !detectable,
            "empty (unpolluted) record must NOT flag the honest block"
        );
    }

    // (3) CROSS-NODE CONVERGENCE (post-fix): two nodes observe the unbonded
    // offender's record and the honest block in OPPOSITE orders (node A: record-
    // then-block; node B: block-then-record). Post-fix neither node stamps
    // (unbonded ⇒ Oblivious), so both records stay empty and both reach the SAME
    // verdict on the honest block — the observation-order-dependent fork is gone.
    #[test]
    fn tier0_cross_node_observation_order_converges() {
        let v = validator(1);
        let b0 = block(&v, 0, hash(10), None);
        let b1 = block(&v, 1, hash(11), Some(b0.block_hash.clone()));
        let dag = dag_with(&[b0.clone(), b1.clone()]);
        let block_store = block_store_with(&[b0.clone(), b1.clone()]);
        let latest_messages = BTreeMap::from([(v.clone(), b1.block_hash.clone())]);
        let bonds = HashMap::new();

        // Node A: observe the record (V unbonded) BEFORE evaluating the block.
        let record_a =
            EquivocationRecord::new(v.clone(), BondGeneration::GENESIS, 0, BTreeSet::new());
        let mut cache_a = CanonicalChildCache::new();
        let status_a = EquivocationDetector::get_equivocation_discovery_status(
            &dag,
            &block_store,
            &record_a,
            &b0,
            &mut cache_a,
            &latest_messages,
            &bonds,
            &HashMap::from([(v.clone(), BondGeneration::GENESIS)]),
        )
        .unwrap();
        let mut cache_a2 = CanonicalChildCache::new();
        let verdict_a = EquivocationDetector::is_equivocation_detectable(
            &dag,
            &block_store,
            &latest_messages,
            &record_a,
            &[],
            &b0,
            &mut cache_a2,
        )
        .unwrap();

        // Node B: evaluate the block BEFORE observing the record — opposite order.
        let record_b =
            EquivocationRecord::new(v.clone(), BondGeneration::GENESIS, 0, BTreeSet::new());
        let mut cache_b2 = CanonicalChildCache::new();
        let verdict_b = EquivocationDetector::is_equivocation_detectable(
            &dag,
            &block_store,
            &latest_messages,
            &record_b,
            &[],
            &b0,
            &mut cache_b2,
        )
        .unwrap();
        let mut cache_b = CanonicalChildCache::new();
        let status_b = EquivocationDetector::get_equivocation_discovery_status(
            &dag,
            &block_store,
            &record_b,
            &b0,
            &mut cache_b,
            &latest_messages,
            &bonds,
            &HashMap::from([(v.clone(), BondGeneration::GENESIS)]),
        )
        .unwrap();

        // FV audit #6 fix: neither node stamps (unbonded ⇒ Oblivious), so both
        // records stay empty and the honest-block verdicts AGREE — no fork.
        assert_eq!(status_a, EquivocationDiscoveryStatus::EquivocationOblivious);
        assert_eq!(status_b, EquivocationDiscoveryStatus::EquivocationOblivious);
        assert_eq!(
            record_a.equivocation_detected_block_hashes,
            record_b.equivocation_detected_block_hashes,
            "both nodes reach the SAME (empty) witness set"
        );
        assert!(record_a.equivocation_detected_block_hashes.is_empty());
        assert_eq!(
            verdict_a, verdict_b,
            "FV audit #6 fix: observation order no longer changes the verdict — the nodes CONVERGE (no consensus fork)"
        );
        assert!(!verdict_a, "honest block resolves Oblivious on both nodes");
    }
}
