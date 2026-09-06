// See casper/src/main/scala/coop/rchain/casper/util/ProtoUtil.scala

use std::collections::HashSet;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use crypto::rust::hash::blake2b256::Blake2b256;
use crypto::rust::signatures::signed::Signed;
use models::casper::{BondInfo, JustificationInfo};
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use models::rust::block_hash::BlockHash;
use models::rust::block_metadata::BlockMetadata;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Body, Bond, DeployData, Header, Justification, ProcessedDeploy,
    ProcessedSystemDeploy, RejectedDeploy,
};
use models::rust::validator::Validator;
use rholang::rust::interpreter::deploy_parameters::DeployParameters;
use shared::rust::store::key_value_store::KvStoreError;
use shared::rust::ByteString;

use crate::rust::errors::CasperError;

pub fn get_main_chain_until_depth(
    block_store: &KeyValueBlockStore,
    estimate: BlockMessage,
    mut acc: Vec<BlockMessage>,
    depth: i32,
) -> Result<Vec<BlockMessage>, KvStoreError> {
    let parents_hashes = parent_hashes(&estimate);
    let maybe_main_parent_hash = parents_hashes.first();
    match maybe_main_parent_hash {
        Some(main_parent_hash) => {
            let updated_estimate = block_store.get_unsafe(main_parent_hash);
            let depth_delta = block_number(&updated_estimate) - block_number(&estimate);
            let new_depth = depth + depth_delta as i32;
            if new_depth <= 0 {
                acc.push(estimate);
                Ok(acc)
            } else {
                acc.push(estimate);
                get_main_chain_until_depth(block_store, updated_estimate, acc, new_depth)
            }
        }
        None => {
            acc.push(estimate);
            Ok(acc)
        }
    }
}

pub fn creator_justification_block_message(block: &BlockMessage) -> Option<Justification> {
    block
        .justifications
        .iter()
        .find(|j| j.validator == block.sender)
        .cloned()
}

pub fn creator_justification_block_metadata(block: &BlockMetadata) -> Option<Justification> {
    block
        .justifications
        .iter()
        .find(|j| j.validator == block.sender)
        .cloned()
}

/// Get creator justification as list until goal in memory
/// Since the creator justification is unique, we don't need to return a list.
/// However, the bfTraverseF requires a list to be returned.
/// When we reach the goalFunc, we return an empty list.
pub fn get_creator_justification_as_list_until_goal_in_memory(
    dag: &KeyValueDagRepresentation,
    block_hash: &BlockHash,
    goal_func: impl Fn(&BlockHash) -> bool,
) -> Result<Vec<BlockHash>, KvStoreError> {
    match dag.lookup(block_hash)? {
        Some(block) => {
            // Find creator justification hash
            let creator_justification = block
                .justifications
                .iter()
                .find(|j| j.validator == block.sender)
                .map(|j| &j.latest_block_hash);

            match creator_justification {
                Some(creator_justification_hash) => {
                    // Look up creator justification metadata
                    match dag.lookup(creator_justification_hash)? {
                        Some(creator_justification) => {
                            // Check if goal is reached
                            if goal_func(&creator_justification.block_hash) {
                                Ok(Vec::new())
                            } else {
                                Ok(vec![creator_justification.block_hash.clone()])
                            }
                        }
                        None => Ok(Vec::new()),
                    }
                }
                None => Ok(Vec::new()),
            }
        }
        None => Ok(Vec::new()),
    }
}

/// Get weight map from a block message
pub fn weight_map(
    block_message: &BlockMessage,
) -> std::collections::HashMap<prost::bytes::Bytes, i64> {
    weight_map_from_state(&block_message.body.state)
}

/// Get weight map from a state
fn weight_map_from_state(
    state: &models::rust::casper::protocol::casper_message::F1r3flyState,
) -> std::collections::HashMap<prost::bytes::Bytes, i64> {
    state
        .bonds
        .iter()
        .map(|bond| (bond.validator.clone(), bond.stake))
        .collect()
}

/// Get total weight from a weight map
pub fn weight_map_total(weights: &std::collections::HashMap<ByteString, i64>) -> i64 {
    weights.values().sum()
}

/// Get minimum total validator weight
pub fn min_total_validator_weight(
    dag: &mut KeyValueDagRepresentation,
    block_hash: &BlockHash,
    max_clique_min_size: i32,
) -> Result<i64, KvStoreError> {
    dag.lookup(block_hash).map(|block_metadata_opt| {
        let block_metadata = block_metadata_opt.expect("Block metadata should exist");
        let mut sorted_weights: Vec<i64> = block_metadata.weight_map.values().cloned().collect();
        sorted_weights.sort();
        sorted_weights
            .iter()
            .take(max_clique_min_size as usize)
            .sum()
    })
}

/// Get main parent of a block
pub fn main_parent(
    block_store: &mut KeyValueBlockStore,
    block_message: &BlockMessage,
) -> Result<Option<BlockMessage>, KvStoreError> {
    match block_message.header.parents_hash_list.first() {
        Some(parent_hash) => block_store.get(parent_hash),
        None => Ok(None),
    }
}

/// Get weight from validator by dag
pub fn weight_from_validator_by_dag(
    dag: &mut KeyValueDagRepresentation,
    block_hash: &BlockHash,
    validator: &Validator,
) -> Result<i64, KvStoreError> {
    // On the fork-choice BFS a traversed block — or its main parent, read for
    // the weight map — can be absent from the metadata index: a sync/prune
    // window, or, on an LFS-restored node, a parent below the restore horizon
    // (held as a hash only, never indexed). Absence is a statement about THIS
    // node's history, never about the block being judged: `MissingBlock`
    // collapses to `BlockNotHeld` and the pipeline defers the block for
    // fetch-and-retry, where a `KeyNotFound` hard-failed admission (the #306
    // storm on restored joiners and observers). No backtrace in the context —
    // this is a hot path with exactly one caller (`estimator::build_scores_map`).
    let block_metadata = dag
        .lookup(block_hash)?
        .ok_or_else(|| KvStoreError::MissingBlock {
            hash: block_hash.clone(),
            context: " [weight_from_validator_by_dag: traversed block]".to_string(),
        })?;

    // Try to get parent's weight for this validator
    match block_metadata.parents.first() {
        Some(parent_hash) => {
            // Look up parent
            let parent_metadata =
                dag.lookup(parent_hash)?
                    .ok_or_else(|| KvStoreError::MissingBlock {
                        hash: parent_hash.clone(),
                        context: " [weight_from_validator_by_dag: main parent]".to_string(),
                    })?;
            // Return validator's weight from parent or 0 if not found
            Ok(parent_metadata
                .weight_map
                .get(validator)
                .cloned()
                .unwrap_or(0))
        }
        None => {
            // No parents (genesis) - use current block's weight map
            Ok(block_metadata
                .weight_map
                .get(validator)
                .cloned()
                .unwrap_or(0))
        }
    }
}

/// Get weight from validator
pub fn weight_from_validator(
    block_store: &mut KeyValueBlockStore,
    b: &BlockMessage,
    validator: &prost::bytes::Bytes,
) -> Result<i64, KvStoreError> {
    // Get main parent
    let maybe_main_parent = main_parent(block_store, b)?;

    // Get weight from validator (from parent or current block)
    let weight = match maybe_main_parent {
        Some(parent) => weight_map(&parent).get(validator).cloned().unwrap_or(0),
        None => weight_map(b).get(validator).cloned().unwrap_or(0), // No parents means genesis - use itself
    };

    Ok(weight)
}

/// Get weight from sender
pub fn weight_from_sender(
    block_store: &mut KeyValueBlockStore,
    b: &BlockMessage,
) -> Result<i64, KvStoreError> {
    weight_from_validator(block_store, b, &b.sender)
}

pub fn parent_hashes(block: &BlockMessage) -> Vec<prost::bytes::Bytes> {
    block.header.parents_hash_list.to_vec()
}

pub fn get_parents(block_store: &KeyValueBlockStore, block: &BlockMessage) -> Vec<BlockMessage> {
    parent_hashes(block)
        .iter()
        .map(|bytes| block_store.get_unsafe(bytes))
        .collect()
}

/// The metadata of every parent, or [`CasperError::BlockNotHeld`] naming the
/// first parent this node does not have.
///
/// A node restored from a sync anchor holds no history below it, so the walks
/// that call this can legitimately reach a parent it will never have. That is a
/// fact about the node, not about the block being validated, and it must stay
/// distinguishable from a storage fault: the caller turns the latter into a
/// slashable verdict against the block's proposer.
pub fn get_parents_metadata(
    dag: &KeyValueDagRepresentation,
    block: &BlockMetadata,
) -> Result<Vec<BlockMetadata>, CasperError> {
    block
        .parents
        .iter()
        .map(|parent| {
            dag.lookup(parent)
                .map_err(CasperError::from)?
                .ok_or_else(|| CasperError::BlockNotHeld(parent.clone()))
        })
        .collect()
}

pub fn get_parent_metadatas_above_block_number(
    block: &BlockMetadata,
    block_number: i64,
    dag: &KeyValueDagRepresentation,
) -> Result<Vec<BlockMetadata>, CasperError> {
    get_parents_metadata(dag, block).map(|parents| {
        parents
            .into_iter()
            .filter(|p| p.block_number >= block_number)
            .collect()
    })
}

pub fn deploys(block: &BlockMessage) -> Vec<ProcessedDeploy> { block.body.deploys.clone() }

/// The block's KEPT rejection records. A duplicate-flagged record states
/// that the copy it discarded was redundant — its effect already stood in
/// the forming merge's own post-state — so it does not dispute the sig's
/// standing win. Every disposition reader consumes records through this
/// one filter so the discard policy cannot drift between readers.
pub fn kept_rejected_records(block: &BlockMessage) -> impl Iterator<Item = &RejectedDeploy> {
    block
        .body
        .rejected_deploys
        .iter()
        .filter(|record| !record.duplicate)
}

pub fn system_deploys(block: &BlockMessage) -> Vec<ProcessedSystemDeploy> {
    block.body.system_deploys.clone()
}

pub fn post_state_hash(block: &BlockMessage) -> prost::bytes::Bytes {
    block.body.state.post_state_hash.clone()
}

pub fn pre_state_hash(block: &BlockMessage) -> prost::bytes::Bytes {
    block.body.state.pre_state_hash.clone()
}

pub fn bonds(block: &BlockMessage) -> Vec<Bond> { block.body.state.bonds.clone() }

pub fn block_number(block: &BlockMessage) -> i64 { block.body.state.block_number }

pub fn bond_to_bond_info(bond: &Bond) -> BondInfo {
    BondInfo {
        validator: PrettyPrinter::build_string_no_limit(&bond.validator),
        stake: bond.stake,
    }
}

pub fn max_block_number_metadata(blocks: &Vec<BlockMetadata>) -> i64 {
    blocks
        .iter()
        .fold(-1, |acc, block| std::cmp::max(acc, block.block_number))
}

pub fn justifications_to_justification_infos(justification: &Justification) -> JustificationInfo {
    JustificationInfo {
        validator: PrettyPrinter::build_string_no_limit(&justification.validator),
        latest_block_hash: PrettyPrinter::build_string_no_limit(&justification.latest_block_hash),
    }
}

pub fn to_justification(
    latest_messages: std::collections::HashMap<Validator, BlockMetadata>,
) -> Vec<Justification> {
    latest_messages
        .into_iter()
        .map(|(validator, block_metadata)| Justification {
            validator,
            latest_block_hash: block_metadata.block_hash,
        })
        .collect()
}

/// Project a list of justifications into a per-validator latest-message
/// map. Returns `BTreeMap` (not `HashMap`) so iteration is deterministic
/// across nodes — consensus-critical for the operator-visible audit trail
/// emitted by `validate.rs::justification_regressions`, whose "first
/// regression detected" log line is logged at the moment of early return;
/// HashMap entropy would make different nodes report different regressors
/// for the same offending block, fragmenting the audit trail.
pub fn to_latest_message_hashes(
    justifications: &[Justification],
) -> std::collections::BTreeMap<Validator, BlockHash> {
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

pub fn to_latest_message(
    justifications: &Vec<Justification>,
    dag: &KeyValueDagRepresentation,
) -> Result<std::collections::HashMap<Validator, BlockMetadata>, KvStoreError> {
    let mut latest_messages = std::collections::HashMap::new();
    for justification in justifications {
        let block_metadata = dag.lookup(&justification.latest_block_hash)?;
        match block_metadata {
            Some(block_metadata) => {
                latest_messages.insert(justification.validator.clone(), block_metadata);
            }
            None => {
                return Err(KvStoreError::KeyNotFound(format!(
                    "Could not find a block for {} in the DAG storage",
                    PrettyPrinter::build_string_bytes(&justification.latest_block_hash)
                )));
            }
        }
    }
    Ok(latest_messages)
}

pub fn block_header(parent_hashes: Vec<ByteString>, version: i64, timestamp: i64) -> Header {
    Header {
        parents_hash_list: parent_hashes.into_iter().map(Into::into).collect(),
        timestamp,
        version,
        extra_bytes: prost::bytes::Bytes::new(),
    }
}

pub fn unsigned_block_proto(
    body: Body,
    header: Header,
    justifications: Vec<Justification>,
    shard_id: String,
    seq_num: Option<i32>,
) -> BlockMessage {
    let seq_num = seq_num.unwrap_or(0);
    let mut block = BlockMessage {
        block_hash: prost::bytes::Bytes::new(),
        header,
        body,
        justifications,
        sender: prost::bytes::Bytes::new(),
        seq_num,
        sig: prost::bytes::Bytes::new(),
        sig_algorithm: "".to_string(),
        shard_id,
        extra_bytes: prost::bytes::Bytes::new(),
    };

    let hash = hash_block(&block);
    block.block_hash = hash.into();
    block
}

pub fn hash_block(block: &BlockMessage) -> BlockHash {
    use prost::Message;

    let bytes: Vec<u8> = block
        .header
        .to_proto()
        .encode_to_vec()
        .into_iter()
        .chain(block.body.to_proto().encode_to_vec().into_iter())
        .chain(block.sender.clone().into_iter())
        .chain(block.sig_algorithm.as_bytes().to_vec().into_iter())
        .chain(block.seq_num.to_le_bytes().into_iter())
        .chain(block.shard_id.as_bytes().to_vec().into_iter())
        .chain(block.extra_bytes.clone().into_iter())
        .collect();

    Blake2b256::hash(bytes).into()
}

pub fn hash_string(b: &BlockMessage) -> BlockHash {
    use prost::Message;
    hex::encode(b.block_hash.encode_to_vec()).into()
}

pub fn compute_code_hash(dd: &DeployData) -> Par {
    let term = dd.term.as_bytes();
    let hash = Blake2b256::hash(term.to_vec());
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GByteArray(hash)),
        }],
        ..Default::default()
    }
}

pub fn get_rholang_deploy_params(dd: &Signed<DeployData>) -> DeployParameters {
    let user_id: Par = Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GByteArray(dd.pk.bytes.to_vec())),
        }],
        ..Default::default()
    };

    DeployParameters { user_id }
}

pub fn dependencies_hashes_of(b: &BlockMessage) -> Vec<BlockHash> {
    let missing_parents: HashSet<BlockHash> = parent_hashes(b).into_iter().collect();
    let missing_justifications: HashSet<BlockHash> = b
        .justifications
        .iter()
        .map(|j| j.latest_block_hash.clone())
        .collect();

    (missing_parents.union(&missing_justifications))
        .cloned()
        .collect()
}

// Return hashes of all blocks that are yet to be seen by the passed in block
pub fn unseen_block_hashes(
    dag: &KeyValueDagRepresentation,
    justifications: &Vec<Justification>,
    current_block_hash: Option<&BlockHash>,
) -> Result<HashSet<BlockHash>, KvStoreError> {
    let dags_latest_messages = dag.latest_messages()?;
    let blocks_latest_messages = to_latest_message(justifications, dag)?;

    // From input block perspective we want to find what latest messages are not seen
    //  that are in the DAG latest messages.
    // - if validator is not in the justification of the block
    // - if justification contains validator's newer latest message
    let unseen_latest_messages =
        dags_latest_messages
            .into_iter()
            .filter(|(validator, dag_latest_message)| {
                let validator_in_justification = blocks_latest_messages.contains_key(validator);
                let block_has_newer_latest_message =
                    blocks_latest_messages
                        .get(validator)
                        .map(|block_latest_message| {
                            dag_latest_message.sequence_number
                                > block_latest_message.sequence_number
                        });

                !validator_in_justification
                    || (validator_in_justification
                        && block_has_newer_latest_message.unwrap_or(false))
            });

    // Collect all unseen block hashes
    let mut all_unseen_blocks = HashSet::new();
    for (validator, unseen_latest_message) in unseen_latest_messages {
        let validator_latest_message = blocks_latest_messages.get(&validator);
        let creator_blocks =
            get_creator_blocks_between(dag, &unseen_latest_message, validator_latest_message)?;
        all_unseen_blocks.extend(creator_blocks);
    }

    // Remove blocks that are already in the block's justifications
    for block_metadata in blocks_latest_messages.values() {
        all_unseen_blocks.remove(&block_metadata.block_hash);
    }

    // Remove the current block hash (at block creation the block does not exist yet)
    if let Some(h) = current_block_hash {
        all_unseen_blocks.remove(h);
    }

    Ok(all_unseen_blocks)
}

/// Deterministic invalid-blocks map (block_hash -> sender) for the PoS slash
/// system deploys in a block: exactly the blocks this block slashes, each keyed
/// to its immutable sender. Derived from the block's own recorded slash targets
/// (NOT the node's DAG invalid-set view, which is node-view-dependent and so
/// differs between the proposer and a validator), so block creation and replay
/// produce a byte-identical map. That makes the slash deploy's
/// `rho:casper:invalidBlocks` produce reproduce at replay (no slash
/// ConsumeFailed). The slash contract only looks up its own target via
/// `invalidBlocks.getOrElse(blockHash, ..)`, so this is exactly what it needs —
/// and it makes the slash hit the block's true sender instead of falling back.
pub fn slashed_block_senders(
    dag: &KeyValueDagRepresentation,
    slashed_hashes: &[BlockHash],
) -> Result<std::collections::HashMap<BlockHash, Validator>, KvStoreError> {
    let mut map = std::collections::HashMap::with_capacity(slashed_hashes.len());
    for h in slashed_hashes {
        let meta = dag.lookup_unsafe(h)?;
        map.insert(h.clone(), meta.sender.clone());
    }
    Ok(map)
}

fn get_creator_blocks_between(
    dag: &KeyValueDagRepresentation,
    top_block: &BlockMetadata,
    bottom_block: Option<&BlockMetadata>,
) -> Result<HashSet<BlockHash>, KvStoreError> {
    match bottom_block {
        Some(bottom_block) => {
            // Use the bf_traverse function from dag_ops for breadth-first traversal
            let neighbor_fn = |block: &BlockMetadata| -> Vec<BlockMetadata> {
                get_creator_justification_unless_goal(dag, block, bottom_block).unwrap_or_default()
            };

            // Start traversal from top_block
            let traversal_result =
                shared::rust::dag::dag_ops::bf_traverse(vec![top_block.clone()], neighbor_fn);

            // Collect all block hashes into a HashSet
            let blocks_set: HashSet<BlockHash> = traversal_result
                .into_iter()
                .map(|block| block.block_hash.clone())
                .collect();

            Ok(blocks_set)
        }

        None => Ok(HashSet::from([top_block.block_hash.clone()])),
    }
}

fn get_creator_justification_unless_goal(
    dag: &KeyValueDagRepresentation,
    block: &BlockMetadata,
    goal: &BlockMetadata,
) -> Result<Vec<BlockMetadata>, KvStoreError> {
    match creator_justification_block_metadata(block) {
        Some(Justification {
            validator: _,
            latest_block_hash,
        }) => match dag.lookup(&latest_block_hash) {
            Ok(Some(creator_justification)) => {
                if creator_justification == *goal {
                    Ok(vec![])
                } else {
                    Ok(vec![creator_justification])
                }
            }

            _ => Err(KvStoreError::KeyNotFound(format!(
                "BlockDAG is missing justification {} for {}",
                PrettyPrinter::build_string_bytes(&latest_block_hash),
                PrettyPrinter::build_string_bytes(&block.block_hash)
            ))),
        },

        None => Ok(vec![]),
    }
}

pub fn justification_to_justification_info(justification: &Justification) -> JustificationInfo {
    JustificationInfo {
        validator: PrettyPrinter::build_string_no_limit(&justification.validator),
        latest_block_hash: PrettyPrinter::build_string_no_limit(&justification.latest_block_hash),
    }
}

// ---------------------------------------------------------------------------
// Fork-choice FV — B1, refined by the restore-horizon walk (#306).
//
// `weight_from_validator_by_dag` (above) reads the traversed block's MAIN
// PARENT weight map on the fork-choice BFS hot path (`estimator::build_scores_map`).
// B1 (FIXED): a traversed block whose main parent is momentarily absent from the
// metadata index — a sync / prune window — previously panicked via
// `.expect("Parent metadata should exist")`. The typed error is now
// `KvStoreError::MissingBlock`, which collapses to `CasperError::BlockNotHeld`
// so admission DEFERS the block (fetch-and-retry) instead of hard-failing —
// on an LFS-restored node a main parent below the restore horizon is the
// normal condition, not a fault.
// See docs/casper/theory/fork-choice/fork-choice-verification.md (B1).
// ---------------------------------------------------------------------------
#[cfg(test)]
mod fork_choice_b1_repro_tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use block_storage::rust::dag::block_metadata_store::BlockMetadataStore;
    use parking_lot::RwLock as PlRwLock;
    use proptest::prelude::*;
    use prost::bytes::Bytes;
    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
    use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

    use super::*;

    fn h(n: u8) -> Bytes { Bytes::from(vec![n; 32]) }

    fn md(hash: Bytes, parents: Vec<Bytes>, num: i64, v: &Bytes) -> BlockMetadata {
        let mut wm = BTreeMap::new();
        wm.insert(v.clone(), 7i64);
        BlockMetadata {
            block_hash: hash,
            parents,
            sender: v.clone(),
            justifications: vec![],
            weight_map: wm,
            block_number: num,
            sequence_number: num as i32,
            invalid: false,
            directly_finalized: false,
            finalized: false,
            fault_tolerance_value: 0.0,
            merge_base: Bytes::new(),
        }
    }

    fn dag_with(blocks: Vec<BlockMetadata>) -> KeyValueDagRepresentation {
        let store = KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new()));
        let mut bms = BlockMetadataStore::new(store);
        let mut dag_set = imbl::HashSet::new();
        let mut bnum = imbl::HashMap::new();
        let mut mp = imbl::HashMap::new();
        for b in &blocks {
            dag_set.insert(b.block_hash.clone());
            bnum.insert(b.block_hash.clone(), b.block_number);
            if let Some(p) = b.parents.first() {
                mp.insert(b.block_hash.clone(), p.clone());
            }
        }
        for b in blocks {
            assert!(bms.add(b).is_ok(), "test DAG metadata insert failed");
        }
        KeyValueDagRepresentation {
            dag_set,
            latest_messages_map: imbl::HashMap::new(),
            child_map: imbl::HashMap::new(),
            height_map: imbl::OrdMap::new(),
            block_number_map: bnum,
            main_parent_map: mp,
            self_justification_map: imbl::HashMap::new(),
            invalid_blocks_set: imbl::HashSet::new(),
            last_finalized_block_hash: Bytes::new(),
            finalized_blocks_set: imbl::HashSet::new(),
            block_metadata_index: Arc::new(PlRwLock::new(bms)),
            floor_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
            frontier_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
            lifecycle: Arc::new(parking_lot::RwLock::new(
                block_storage::rust::dag::deploy_lifecycle_types::DeployLifecycleTables::in_memory(
                ),
            )),
            carrier_index: Arc::new(parking_lot::RwLock::new(
                block_storage::rust::dag::carrier_index::CarrierIndex::in_memory(),
            )),
        }
    }

    /// The sixth restore-horizon walk (#306). On an LFS-restored node a held
    /// block's main parent can sit below the horizon — hash-only, never
    /// indexed. That absence is a statement about THIS node's sync, so it
    /// must surface as `MissingBlock` (which collapses to `BlockNotHeld` and
    /// defers the block for fetch-and-retry), never as a `KeyNotFound`
    /// processing failure that hard-fails admission.
    #[test]
    fn a_main_parent_below_the_restore_horizon_is_a_missing_block() {
        let v = h(9);
        let child = h(1);
        let missing = h(2); // below the horizon: referenced, never indexed
        let mut dag = dag_with(vec![md(child.clone(), vec![missing.clone()], 1, &v)]);
        let result = weight_from_validator_by_dag(&mut dag, &child, &v);
        let Err(err) = result else {
            panic!("unheld main parent must error, got {result:?}");
        };
        assert!(
            matches!(&err, KvStoreError::MissingBlock { hash, .. } if *hash == missing),
            "unheld main parent must be MissingBlock naming the parent, got {err:?}"
        );
        // The deferral collapse the block pipeline routes on: the typed
        // absence becomes BlockNotHeld, never a judged exception.
        assert!(
            matches!(
                crate::rust::errors::CasperError::from(err),
                crate::rust::errors::CasperError::BlockNotHeld(hash) if hash == missing
            ),
            "MissingBlock must collapse to BlockNotHeld for the deferral path"
        );
    }

    /// The BFS twin of the case above: the traversed block itself is unheld
    /// (its hash was queued from a held child's parent list). Same contract.
    #[test]
    fn an_unheld_traversed_block_is_a_missing_block() {
        let v = h(9);
        let missing = h(3); // never added to the index
        let mut dag = dag_with(vec![]);
        let result = weight_from_validator_by_dag(&mut dag, &missing, &v);
        assert!(
            matches!(&result, Err(KvStoreError::MissingBlock { hash, .. }) if *hash == missing),
            "unheld traversed block must be MissingBlock naming it, got {result:?}"
        );
    }

    // G1 (CRITICAL) — slash-replay determinism. `slashed_block_senders` builds the slash
    // system-deploy's invalid_blocks map from the block's OWN recorded slash targets and
    // each target's IMMUTABLE sender — NOT the node's `dag.invalid_blocks` view (which
    // diverges proposer-vs-validator and previously caused a `ConsumeFailed` at replay →
    // block rejection + finalization stall). This test pins that VIEW-INDEPENDENCE: two
    // DAGs with identical block metadata but DIFFERENT invalid-block sets produce a
    // byte-identical map, so the PLAY map (block creation) ≡ the REPLAY map (validation).
    fn dag_with_invalid(
        blocks: Vec<BlockMetadata>,
        invalid: Vec<BlockMetadata>,
    ) -> KeyValueDagRepresentation {
        let mut dag = dag_with(blocks);
        let mut inv = imbl::HashSet::new();
        for b in invalid {
            inv.insert(b);
        }
        dag.invalid_blocks_set = inv;
        dag
    }

    #[test]
    fn slashed_block_senders_is_view_independent_g1() {
        let (va, vb, vc) = (h(50), h(51), h(52));
        let (b1, b2, b3) = (h(1), h(2), h(3));
        let blocks = vec![
            md(b1.clone(), vec![], 1, &va),
            md(b2.clone(), vec![b1.clone()], 2, &vb),
            md(b3.clone(), vec![b2.clone()], 3, &vc),
        ];
        // The block slashes b1 and b3 (its own recorded targets).
        let slashed = vec![b1.clone(), b3.clone()];
        // Two nodes with DIVERGENT invalid-block views over identical block metadata
        // (the proposer sees none invalid; the validator's view flags b2 and b3).
        let dag_proposer = dag_with_invalid(blocks.clone(), vec![]);
        let dag_validator = dag_with_invalid(blocks.clone(), vec![
            md(b2.clone(), vec![b1.clone()], 2, &vb),
            md(b3.clone(), vec![b2.clone()], 3, &vc),
        ]);

        let map_play = slashed_block_senders(&dag_proposer, &slashed).expect("play map");
        let map_replay = slashed_block_senders(&dag_validator, &slashed).expect("replay map");

        // Byte-identical map regardless of the invalid-block view (PLAY ≡ REPLAY) ...
        assert_eq!(
            map_play, map_replay,
            "slash map must be node-view-independent (PLAY ≡ REPLAY)"
        );
        // ... and it keys each slashed block to its immutable sender.
        let mut expected = std::collections::HashMap::new();
        expected.insert(b1.clone(), va.clone());
        expected.insert(b3.clone(), vc.clone());
        assert_eq!(
            map_play, expected,
            "slash map must key each slashed block to its sender"
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(200))]
        // G1 (∀-views property) — the standalone view-independence "lemma" for
        // `slashed_block_senders`, mechanized as the modality that can actually catch
        // the regression (a Rocq abstraction of the function would model it as
        // content-only BY CONSTRUCTION and could never witness the defect). Over
        // arbitrary block sets, arbitrary on-chain slash records, and TWO independently
        // random — hence divergent — `invalid_blocks_set` node views, the map is
        // (a) identical across the two views (PLAY ≡ REPLAY) and (b) equal to the
        // content-derived ground truth (each slashed hash ↦ its immutable sender). Were
        // the function to read `dag.invalid_blocks_set`, (a) would fail on the first
        // view pair that disagrees on a slashed block.
        #[test]
        fn slashed_block_senders_is_view_independent_prop_g1(
            senders in prop::collection::vec(0u8..5, 1..10),
            slashed_flags in prop::collection::vec(any::<bool>(), 1..10),
            view_a in prop::collection::vec(any::<bool>(), 1..10),
            view_b in prop::collection::vec(any::<bool>(), 1..10),
        ) {
            let n = senders.len();
            // n distinct blocks h(1..=n) in a chain; block i's immutable sender = h(100+vid).
            let blocks: Vec<BlockMetadata> = (0..n)
                .map(|i| {
                    let hash = h((i + 1) as u8);
                    let parents = if i == 0 { vec![] } else { vec![h(i as u8)] };
                    let sender = h(100 + senders[i]);
                    md(hash, parents, (i + 1) as i64, &sender)
                })
                .collect();
            // On-chain slash record = the flagged subset (by index, within range).
            let slashed: Vec<Bytes> = (0..n)
                .filter(|&i| *slashed_flags.get(i).unwrap_or(&false))
                .map(|i| h((i + 1) as u8))
                .collect();
            // Two nodes: identical block metadata, INDEPENDENTLY random invalid-block sets.
            let inv_a: Vec<BlockMetadata> = (0..n)
                .filter(|&i| *view_a.get(i).unwrap_or(&false))
                .map(|i| blocks[i].clone())
                .collect();
            let inv_b: Vec<BlockMetadata> = (0..n)
                .filter(|&i| *view_b.get(i).unwrap_or(&false))
                .map(|i| blocks[i].clone())
                .collect();
            let dag_a = dag_with_invalid(blocks.clone(), inv_a);
            let dag_b = dag_with_invalid(blocks.clone(), inv_b);

            let map_a = slashed_block_senders(&dag_a, &slashed).expect("map a");
            let map_b = slashed_block_senders(&dag_b, &slashed).expect("map b");
            prop_assert_eq!(&map_a, &map_b);

            let mut expected = std::collections::HashMap::new();
            for i in 0..n {
                if *slashed_flags.get(i).unwrap_or(&false) {
                    expected.insert(h((i + 1) as u8), h(100 + senders[i]));
                }
            }
            prop_assert_eq!(&map_a, &expected);
        }
    }
}
