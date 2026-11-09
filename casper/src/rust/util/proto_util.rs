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
    ProcessedSystemDeploy, ProcessedUserDeploy, RejectedDeploy,
};
use models::rust::validator::Validator;
use rholang::rust::interpreter::deploy_parameters::DeployParameters;
use shared::rust::dag::observation_work::{NoopWork, WorkMeter};
use shared::rust::store::key_value_store::{KvStoreError, MissingBlockContext};
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
    weight_from_validator_by_dag_metered(&NoopWork, dag, block_hash, validator)
}

pub fn weight_from_validator_by_dag_metered<W: WorkMeter>(
    meter: &W,
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
    meter.lookup()?;
    let block_metadata = dag
        .lookup(block_hash)?
        .ok_or_else(|| KvStoreError::MissingBlock {
            hash: block_hash.clone(),
            context: MissingBlockContext::new("weight_from_validator_by_dag: traversed block"),
        })?;

    // Try to get parent's weight for this validator
    match block_metadata.parents.first() {
        Some(parent_hash) => {
            // Look up parent
            meter.lookup()?;
            let parent_metadata =
                dag.lookup(parent_hash)?
                    .ok_or_else(|| KvStoreError::MissingBlock {
                        hash: parent_hash.clone(),
                        context: MissingBlockContext::new(
                            "weight_from_validator_by_dag: main parent",
                        ),
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
                .ok_or_else(|| {
                    CasperError::BlockNotHeld(
                        parent.clone(),
                        MissingBlockContext::new("get_parents_metadata"),
                    )
                })
        })
        .collect()
}

/// How a walk treats a parent this node does not hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnheldParent {
    /// Verdict walks: surfaced as `BlockNotHeld` — a swallowed gap admits a
    /// repeated deploy.
    Surface,
    /// Live-decision walks: settled ancestry below the restore horizon,
    /// skipped with a warn. Under an undersized block floor the skip
    /// under-fills the proposer's dedup window — self-harm, not a safety
    /// hole.
    SkipSettled,
}

pub fn parent_metadatas_above_block_number(
    block: &BlockMetadata,
    block_number: i64,
    dag: &KeyValueDagRepresentation,
    on_unheld: UnheldParent,
) -> Result<Vec<BlockMetadata>, CasperError> {
    let mut result = Vec::with_capacity(block.parents.len());
    for parent in &block.parents {
        match dag.lookup(parent).map_err(CasperError::from)? {
            Some(meta) => {
                if meta.block_number >= block_number {
                    result.push(meta);
                }
            }
            None => match on_unheld {
                UnheldParent::Surface => {
                    return Err(CasperError::BlockNotHeld(
                        parent.clone(),
                        MissingBlockContext::new("parent_metadatas_above_block_number"),
                    ))
                }
                UnheldParent::SkipSettled => {
                    tracing::warn!(
                        parent = %PrettyPrinter::build_string_bytes(parent),
                        child = %PrettyPrinter::build_string_bytes(&block.block_hash),
                        earliest_wanted = block_number,
                        "in-scope walk skipped an unheld parent: settled ancestry below \
                         a restore horizon, or an undersized block floor"
                    );
                }
            },
        }
    }
    Ok(result)
}

pub fn deploys(block: &BlockMessage) -> Vec<ProcessedUserDeploy> { block.body.deploys.clone() }

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

/// Fast-path variant of `unsigned_block_proto` that reuses per-deploy
/// Merkle leaves computed earlier on the propose path.  See
/// `hash_block_with_leaves` for the contract on `leaves` ordering.
pub fn unsigned_block_proto_with_leaves(
    body: Body,
    header: Header,
    justifications: Vec<Justification>,
    shard_id: String,
    seq_num: Option<i32>,
    leaves: &[[u8; 32]],
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

    let hash = hash_block_with_leaves(&block, leaves);
    block.block_hash = hash.into();
    block
}

// =========================================================================
// Block hash — Merkle scheme v1
// =========================================================================
//
// Shape:
//   block_hash = hash_top(hash_leaf(LEAF_HEADER, header_preimage),
//                         deploys_root)
//
// where `deploys_root` is a Merkle root over per-deploy leaves (user
// deploys first, then system deploys, in commit order) and
// `header_preimage` concatenates header + body-state + metadata fields
// together with Merkle roots of the rejected-deploy and
// applied-from-scope lists.  See `docs/designs/merkle-block-hash.md` for
// the full recipe.  Hard-fork surface: the domain-byte constants below
// pin the shape — any change flips every block hash on-network.
//
// This is a per-release hard fork for each shard.  Shards running an
// older release keep the previous flat Blake2b hash; shards cut from
// this release use the Merkle scheme.
//
// Benefits:
//   * per-deploy inclusion proofs (log_2(n) sibling hashes + one top
//     sibling against the signed `block_hash`);
//   * leaves can be computed on the hot path during replay, so the
//     end-of-block hash check collapses the already-computed leaves
//     into a root instead of serial-encoding header+body;
//   * fail-fast: a bad deploy is detected the moment its leaf is
//     computed, not at end of block.

/// Scheme-version byte prepended to the header preimage.  Bumping this
/// constant is a hard fork — see the file-level docstring.
pub const BLOCK_HASH_SCHEME_VERSION: u8 = 0x01;

/// Interior-node domain byte used by `MERKLE_PARAMS`.
const INTERIOR_DOMAIN: u8 = 0x01;

/// Top-node domain byte: `hash_top(h_header, deploys_root)` =
/// `Blake2b256(TOP_DOMAIN || h_header || deploys_root)`.  Disjoint from
/// `INTERIOR_DOMAIN` so a top node cannot be confused with an interior
/// node of a sub-tree.
const TOP_DOMAIN: u8 = 0x02;

const LEAF_PROCESSED_DEPLOY: u8 = 0x10;
const LEAF_PROCESSED_SYSTEM_DEPLOY: u8 = 0x11;
const LEAF_HEADER: u8 = 0x12;
const LEAF_REJECTED_DEPLOY: u8 = 0x13;
const LEAF_APPLIED_FROM_SCOPE_SIG: u8 = 0x14;
const LEAF_BOND: u8 = 0x15;

const MERKLE_PARAMS: crypto::rust::hash::merkle::MerkleParams =
    crypto::rust::hash::merkle::MerkleParams::new(INTERIOR_DOMAIN);

/// Compute a `ProcessedDeploy` leaf hash:
/// `Blake2b256(LEAF_PROCESSED_DEPLOY || prost(pd))`.
pub fn hash_processed_deploy_leaf(pd: &ProcessedDeploy) -> [u8; 32] {
    use prost::Message;
    let proto_bytes = pd.clone().to_proto().encode_to_vec();
    crypto::rust::hash::merkle::hash_leaf_tagged(LEAF_PROCESSED_DEPLOY, &proto_bytes)
}

pub fn hash_processed_user_deploy_leaf(pd: &ProcessedUserDeploy) -> [u8; 32] {
    use prost::Message;
    crypto::rust::hash::merkle::hash_leaf_tagged(
        LEAF_PROCESSED_DEPLOY,
        &pd.to_proto().encode_to_vec(),
    )
}

/// Compute a `ProcessedSystemDeploy` leaf hash:
/// `Blake2b256(LEAF_PROCESSED_SYSTEM_DEPLOY || prost(psd))`.
pub fn hash_processed_system_deploy_leaf(psd: &ProcessedSystemDeploy) -> [u8; 32] {
    use prost::Message;
    let proto_bytes = psd.clone().to_proto().encode_to_vec();
    crypto::rust::hash::merkle::hash_leaf_tagged(LEAF_PROCESSED_SYSTEM_DEPLOY, &proto_bytes)
}

/// Build the deploys-root leaf vector from a `BlockMessage` body in
/// commit order: all user deploys first, then all system deploys.  This
/// is the recomputation path used by validators that are not replaying
/// the block (e.g., validating a stored block).  The hot path accepts
/// pre-computed leaves via `hash_block_with_leaves`.
fn deploys_root_leaves(block: &BlockMessage) -> Vec<[u8; 32]> {
    let mut leaves = Vec::with_capacity(block.body.deploys.len() + block.body.system_deploys.len());
    for pd in &block.body.deploys {
        leaves.push(hash_processed_user_deploy_leaf(pd));
    }
    for psd in &block.body.system_deploys {
        leaves.push(hash_processed_system_deploy_leaf(psd));
    }
    leaves
}

/// Merkle root over the rejected-deploy sub-tree.  Rejected deploys are
/// sorted by signature bytes before leaf-hashing so the root is
/// independent of insertion order.
fn rejected_root(block: &BlockMessage) -> [u8; 32] {
    use prost::Message;
    let mut sorted: Vec<&RejectedDeploy> = block.body.rejected_deploys.iter().collect();
    sorted.sort_by(|a, b| a.sig.as_ref().cmp(b.sig.as_ref()));
    let leaves: Vec<[u8; 32]> = sorted
        .iter()
        .map(|r| {
            let proto_bytes = (*r).clone().to_proto().encode_to_vec();
            crypto::rust::hash::merkle::hash_leaf_tagged(LEAF_REJECTED_DEPLOY, &proto_bytes)
        })
        .collect();
    crypto::rust::hash::merkle::merkle_root(&MERKLE_PARAMS, &leaves)
}

/// Merkle root over the applied-from-scope sub-tree.  Signatures are
/// sorted lexicographically before leaf-hashing.
fn applied_from_scope_root(block: &BlockMessage) -> [u8; 32] {
    let mut sorted: Vec<&prost::bytes::Bytes> = block.body.applied_from_scope.iter().collect();
    sorted.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
    let leaves: Vec<[u8; 32]> = sorted
        .iter()
        .map(|sig| {
            crypto::rust::hash::merkle::hash_leaf_tagged(LEAF_APPLIED_FROM_SCOPE_SIG, sig.as_ref())
        })
        .collect();
    crypto::rust::hash::merkle::merkle_root(&MERKLE_PARAMS, &leaves)
}

/// Merkle root over the bonds sub-tree.  Bonds are sorted by validator
/// public key before leaf-hashing so the root is independent of
/// in-memory order.  Each leaf is
/// `Blake2b256(LEAF_BOND || validator_pk || stake_le)`.
fn bonds_root(bonds: &[Bond]) -> [u8; 32] {
    let mut sorted: Vec<&Bond> = bonds.iter().collect();
    sorted.sort_by(|a, b| a.validator.as_ref().cmp(b.validator.as_ref()));
    let leaves: Vec<[u8; 32]> = sorted
        .iter()
        .map(|b| {
            let mut buf = Vec::with_capacity(b.validator.len() + 8);
            buf.extend_from_slice(b.validator.as_ref());
            buf.extend_from_slice(&b.stake.to_le_bytes());
            crypto::rust::hash::merkle::hash_leaf_tagged(LEAF_BOND, &buf)
        })
        .collect();
    crypto::rust::hash::merkle::merkle_root(&MERKLE_PARAMS, &leaves)
}

/// Build the header-side preimage.  See file-level docstring and
/// `docs/designs/merkle-block-hash.md` for the field order.  Any change
/// here is a hard fork — pinned by the `hash_block` golden-hex tests.
fn header_preimage(block: &BlockMessage) -> Vec<u8> {
    use prost::Message;
    let state = &block.body.state;
    let header_bytes = block.header.to_proto().encode_to_vec();
    let r_root = rejected_root(block);
    let afs_root = applied_from_scope_root(block);
    let b_root = bonds_root(&state.bonds);

    let mut buf = Vec::with_capacity(
        1 + header_bytes.len()
            + state.pre_state_hash.len()
            + state.post_state_hash.len()
            + 32  // bonds_root
            + 8   // block_number
            + 32  // rejected_root
            + 32  // applied_from_scope_root
            + block.body.merge_base.len()
            + block.body.extra_bytes.len()
            + block.sender.len()
            + block.sig_algorithm.len()
            + 4   // seq_num
            + block.shard_id.len()
            + block.extra_bytes.len(),
    );
    buf.push(BLOCK_HASH_SCHEME_VERSION);
    buf.extend_from_slice(&header_bytes);
    buf.extend_from_slice(state.pre_state_hash.as_ref());
    buf.extend_from_slice(state.post_state_hash.as_ref());
    buf.extend_from_slice(&b_root);
    buf.extend_from_slice(&state.block_number.to_le_bytes());
    buf.extend_from_slice(&r_root);
    buf.extend_from_slice(&afs_root);
    buf.extend_from_slice(block.body.merge_base.as_ref());
    buf.extend_from_slice(block.body.extra_bytes.as_ref());
    buf.extend_from_slice(block.sender.as_ref());
    buf.extend_from_slice(block.sig_algorithm.as_bytes());
    buf.extend_from_slice(&block.seq_num.to_le_bytes());
    buf.extend_from_slice(block.shard_id.as_bytes());
    buf.extend_from_slice(block.extra_bytes.as_ref());
    buf
}

/// Combine the header-leaf hash and the deploys root under the TOP
/// domain byte to produce the final 32-byte block hash.
fn hash_top(h_header: &[u8; 32], deploys_root: &[u8; 32]) -> [u8; 32] {
    let mut input = [0u8; 1 + 32 + 32];
    input[0] = TOP_DOMAIN;
    input[1..33].copy_from_slice(h_header);
    input[33..].copy_from_slice(deploys_root);
    let h = Blake2b256::hash(input.to_vec());
    let mut out = [0u8; 32];
    out.copy_from_slice(&h);
    out
}

/// Compute the block hash from a `BlockMessage` alone — recomputes
/// per-deploy leaves from the body.  Used by validators that are not
/// replaying (e.g., stored-block hash check) and anywhere a hash is
/// needed without pre-computed leaves on hand.
pub fn hash_block(block: &BlockMessage) -> BlockHash {
    let leaves = deploys_root_leaves(block);
    hash_block_with_leaves(block, &leaves)
}

/// Fast-path block hash: skips leaf recomputation by trusting the
/// caller-supplied leaves.  Used on the propose / replay hot path
/// where each deploy's leaf was hashed as that deploy finished.
///
/// `deploys_leaves` must be in commit order: all user-deploy leaves
/// first, then all system-deploy leaves.  A caller that supplies the
/// wrong leaves produces a wrong hash (mismatch surfaces at
/// `Validate::block_hash` on another node).
pub fn hash_block_with_leaves(block: &BlockMessage, deploys_leaves: &[[u8; 32]]) -> BlockHash {
    let d_root = crypto::rust::hash::merkle::merkle_root(&MERKLE_PARAMS, deploys_leaves);
    let h_header =
        crypto::rust::hash::merkle::hash_leaf_tagged(LEAF_HEADER, &header_preimage(block));
    let top = hash_top(&h_header, &d_root);
    top.to_vec().into()
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
        // absence becomes BlockNotHeld with the accessor tag riding along.
        match crate::rust::errors::CasperError::from(err) {
            crate::rust::errors::CasperError::BlockNotHeld(hash, site) => {
                assert_eq!(hash, missing);
                assert!(
                    !site.accessor().is_empty(),
                    "the accessor tag must survive the collapse, got {site:?}"
                );
            }
            other => panic!("MissingBlock must collapse to BlockNotHeld, got {other:?}"),
        }
    }

    /// The live-decision policy skips a sub-horizon parent instead of
    /// erroring; the held sibling and the number filter are unaffected.
    #[test]
    fn in_scope_walk_skips_an_unheld_parent_and_keeps_the_held_window() {
        let v = h(9);
        let held_parent = h(4);
        let missing_parent = h(2); // below the horizon: referenced, never indexed
        let child = h(1);
        let held_meta = md(held_parent.clone(), vec![], 5, &v);
        let child_meta = md(
            child.clone(),
            vec![held_parent.clone(), missing_parent.clone()],
            6,
            &v,
        );
        let dag = dag_with(vec![held_meta, child_meta.clone()]);

        let in_window =
            parent_metadatas_above_block_number(&child_meta, 3, &dag, UnheldParent::SkipSettled)
                .expect("unheld parent must be skipped, not an error");
        assert_eq!(
            in_window.iter().map(|m| &m.block_hash).collect::<Vec<_>>(),
            vec![&held_parent],
            "the held parent survives; the unheld one is silently settled"
        );

        let above_window =
            parent_metadatas_above_block_number(&child_meta, 6, &dag, UnheldParent::SkipSettled)
                .expect("unheld parent must be skipped, not an error");
        assert!(
            above_window.is_empty(),
            "the number filter still bounds the held parent"
        );

        // The verdict policy refuses the same DAG.
        assert!(
            matches!(
                parent_metadatas_above_block_number(&child_meta, 3, &dag, UnheldParent::Surface),
                Err(crate::rust::errors::CasperError::BlockNotHeld(ref h, _)) if *h == missing_parent
            ),
            "the verdict walk must surface the unheld parent"
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

#[cfg(test)]
mod merkle_block_hash_tests {
    //! Tests for the Merkle-tree block hash scheme introduced per the
    //! `tidy-honey` plan.  See the file-level docstring above `hash_block`
    //! for the scheme.
    use crypto::rust::hash::merkle;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;
    use models::rhoapi::PCost;
    use models::rust::casper::protocol::casper_message::{
        BlockMessage, Body, Bond, DeployData, F1r3flyState, Header, ProcessedDeploy,
        ProcessedSystemDeploy, RejectedDeploy, SystemDeployData,
    };
    use prost::bytes::Bytes;

    use super::*;

    /// Build a `ProcessedDeploy` with a deterministic term string.  Uses
    /// a shared secp256k1 key so repeated calls produce identical
    /// `ProcessedDeploy` structures (identical signatures) when `nonce`
    /// is identical — which is exactly what we want for the reordering
    /// test (change position, not content).
    fn processed_deploy(nonce: i64) -> ProcessedUserDeploy {
        let (sk, _pk) = Secp256k1.new_key_pair();
        let data = DeployData {
            term: format!("@{nonce}!({nonce})"),
            time_stamp: nonce,
            phlo_price: 1,
            phlo_limit: 100_000,
            valid_after_block_number: 0,
            shard_id: "test-shard".to_string(),
            expiration_timestamp: None,
            parameters: vec![],
        };
        let signed = Signed::create(data, Secp256k1.box_clone(), sk).expect("sign deploy");
        ProcessedUserDeploy::Legacy(ProcessedDeploy {
            deploy: signed,
            cost: PCost { cost: 0 },
            deploy_log: vec![],
            is_failed: false,
            system_deploy_error: None,
        })
    }

    fn processed_system_deploy_close() -> ProcessedSystemDeploy {
        ProcessedSystemDeploy::Succeeded {
            event_list: vec![],
            system_deploy: SystemDeployData::CloseBlockSystemDeployData,
        }
    }

    fn processed_system_deploy_slash(seed: u8) -> ProcessedSystemDeploy {
        ProcessedSystemDeploy::Succeeded {
            event_list: vec![],
            system_deploy: SystemDeployData::Slash {
                invalid_block_hash: Bytes::from(vec![seed; 32]),
                issuer_public_key: crypto::rust::public_key::PublicKey::from_bytes(&[seed; 33]),
                target_activation_epoch: seed as i64 + 100,
            },
        }
    }

    fn processed_system_deploy_failed(msg: &str) -> ProcessedSystemDeploy {
        ProcessedSystemDeploy::Failed {
            event_list: vec![],
            error_msg: msg.to_string(),
        }
    }

    fn empty_block() -> BlockMessage {
        BlockMessage {
            block_hash: Bytes::new(),
            header: Header {
                parents_hash_list: vec![],
                timestamp: 1_700_000_000_000,
                version: 1,
                extra_bytes: Bytes::new(),
            },
            body: Body {
                state: F1r3flyState {
                    pre_state_hash: Bytes::from(vec![0u8; 32]),
                    post_state_hash: Bytes::from(vec![0u8; 32]),
                    bonds: vec![],
                    block_number: 0,
                },
                deploys: vec![],
                rejected_deploys: vec![],
                system_deploys: vec![],
                extra_bytes: Bytes::new(),
                applied_from_scope: vec![],
                merge_base: Bytes::new(),
            },
            justifications: vec![],
            sender: Bytes::from(vec![0u8; 33]),
            seq_num: 0,
            sig: Bytes::new(),
            sig_algorithm: "secp256k1".to_string(),
            shard_id: "test-shard".to_string(),
            extra_bytes: Bytes::new(),
        }
    }

    #[test]
    fn hash_block_is_deterministic() {
        let b = empty_block();
        let h1 = hash_block(&b);
        let h2 = hash_block(&b);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 32, "block hash must be 32 bytes");
    }

    #[test]
    fn empty_block_deploys_root_equals_empty_sentinel() {
        let b = empty_block();
        let leaves = deploys_root_leaves(&b);
        assert!(leaves.is_empty());
        assert_eq!(
            crypto::rust::hash::merkle::merkle_root(&MERKLE_PARAMS, &leaves),
            merkle::EMPTY_ROOT,
        );
    }

    #[test]
    fn hash_block_with_leaves_matches_hash_block() {
        // The fast path (leaves provided from replay) and the recompute
        // path (hash_block alone) must agree byte-for-byte.
        let mut b = empty_block();
        b.body.deploys = vec![processed_deploy(1), processed_deploy(2)];
        b.body.system_deploys = vec![processed_system_deploy_close()];
        let leaves = deploys_root_leaves(&b);
        let h_fast = hash_block_with_leaves(&b, &leaves);
        let h_slow = hash_block(&b);
        assert_eq!(h_fast, h_slow);
    }

    #[test]
    fn reordering_deploys_changes_block_hash() {
        let d1 = processed_deploy(1);
        let d2 = processed_deploy(2);
        let mut a = empty_block();
        a.body.deploys = vec![d1.clone(), d2.clone()];
        let mut b = empty_block();
        b.body.deploys = vec![d2, d1];
        assert_ne!(hash_block(&a), hash_block(&b));
    }

    #[test]
    fn flipping_any_metadata_field_changes_hash() {
        let base = {
            let mut b = empty_block();
            b.body.deploys = vec![processed_deploy(1)];
            b
        };
        let h0 = hash_block(&base);

        // sender
        let mut b = base.clone();
        b.sender = Bytes::from(vec![1u8; 33]);
        assert_ne!(hash_block(&b), h0, "sender must affect hash");

        // seq_num
        let mut b = base.clone();
        b.seq_num = 1;
        assert_ne!(hash_block(&b), h0, "seq_num must affect hash");

        // shard_id
        let mut b = base.clone();
        b.shard_id = "other-shard".to_string();
        assert_ne!(hash_block(&b), h0, "shard_id must affect hash");

        // sig_algorithm
        let mut b = base.clone();
        b.sig_algorithm = "ed25519".to_string();
        assert_ne!(hash_block(&b), h0, "sig_algorithm must affect hash");

        // extra_bytes (block level)
        let mut b = base.clone();
        b.extra_bytes = Bytes::from(vec![0xFF]);
        assert_ne!(hash_block(&b), h0, "block.extra_bytes must affect hash");
    }

    #[test]
    fn flipping_any_header_field_changes_hash() {
        let base = empty_block();
        let h0 = hash_block(&base);

        let mut b = base.clone();
        b.header.timestamp += 1;
        assert_ne!(hash_block(&b), h0, "header.timestamp must affect hash");

        let mut b = base.clone();
        b.header.version += 1;
        assert_ne!(hash_block(&b), h0, "header.version must affect hash");

        let mut b = base.clone();
        b.header.parents_hash_list = vec![Bytes::from(vec![0xAA; 32])];
        assert_ne!(
            hash_block(&b),
            h0,
            "header.parents_hash_list must affect hash"
        );

        let mut b = base.clone();
        b.header.extra_bytes = Bytes::from(vec![0xCC]);
        assert_ne!(hash_block(&b), h0, "header.extra_bytes must affect hash");
    }

    #[test]
    fn flipping_any_body_field_changes_hash() {
        let base = empty_block();
        let h0 = hash_block(&base);

        let mut b = base.clone();
        b.body.state.pre_state_hash = Bytes::from(vec![0xA1; 32]);
        assert_ne!(hash_block(&b), h0, "pre_state_hash must affect hash");

        let mut b = base.clone();
        b.body.state.post_state_hash = Bytes::from(vec![0xA2; 32]);
        assert_ne!(hash_block(&b), h0, "post_state_hash must affect hash");

        let mut b = base.clone();
        b.body.state.block_number = 42;
        assert_ne!(hash_block(&b), h0, "block_number must affect hash");

        let mut b = base.clone();
        b.body.state.bonds = vec![Bond {
            validator: Bytes::from(vec![0xBB; 33]),
            stake: 1,
        }];
        assert_ne!(hash_block(&b), h0, "bonds must affect hash");

        let mut b = base.clone();
        b.body.extra_bytes = Bytes::from(vec![0xDD]);
        assert_ne!(hash_block(&b), h0, "body.extra_bytes must affect hash");

        let mut b = base.clone();
        b.body.merge_base = Bytes::from(vec![0xEE; 32]);
        assert_ne!(hash_block(&b), h0, "merge_base must affect hash");

        let mut b = base.clone();
        b.body.deploys = vec![processed_deploy(1)];
        assert_ne!(hash_block(&b), h0, "deploys must affect hash");

        let mut b = base.clone();
        b.body.system_deploys = vec![processed_system_deploy_close()];
        assert_ne!(hash_block(&b), h0, "system_deploys must affect hash");

        let mut b = base.clone();
        b.body.rejected_deploys = vec![RejectedDeploy {
            sig: Bytes::from(vec![0xF1; 64]),
            duplicate: false,
            carrier: Bytes::from(vec![0xF2; 32]),
        }];
        assert_ne!(hash_block(&b), h0, "rejected_deploys must affect hash");

        let mut b = base.clone();
        b.body.applied_from_scope = vec![Bytes::from(vec![0xF3; 64])];
        assert_ne!(hash_block(&b), h0, "applied_from_scope must affect hash");
    }

    #[test]
    fn rejected_deploys_root_is_order_independent() {
        let r_a = RejectedDeploy {
            sig: Bytes::from(vec![0x01; 64]),
            duplicate: false,
            carrier: Bytes::new(),
        };
        let r_b = RejectedDeploy {
            sig: Bytes::from(vec![0x02; 64]),
            duplicate: false,
            carrier: Bytes::new(),
        };
        let mut a = empty_block();
        a.body.rejected_deploys = vec![r_a.clone(), r_b.clone()];
        let mut b = empty_block();
        b.body.rejected_deploys = vec![r_b, r_a];
        assert_eq!(
            hash_block(&a),
            hash_block(&b),
            "rejected_deploys root must be invariant under in-memory ordering"
        );
    }

    #[test]
    fn applied_from_scope_root_is_order_independent() {
        let s1 = Bytes::from(vec![0x11; 64]);
        let s2 = Bytes::from(vec![0x22; 64]);
        let mut a = empty_block();
        a.body.applied_from_scope = vec![s1.clone(), s2.clone()];
        let mut b = empty_block();
        b.body.applied_from_scope = vec![s2, s1];
        assert_eq!(
            hash_block(&a),
            hash_block(&b),
            "applied_from_scope root must be invariant under in-memory ordering"
        );
    }

    #[test]
    fn bonds_root_is_order_independent() {
        let v1 = Bytes::from(vec![0x11; 33]);
        let v2 = Bytes::from(vec![0x22; 33]);
        let mut a = empty_block();
        a.body.state.bonds = vec![
            Bond {
                validator: v1.clone(),
                stake: 100,
            },
            Bond {
                validator: v2.clone(),
                stake: 200,
            },
        ];
        let mut b = empty_block();
        b.body.state.bonds = vec![
            Bond {
                validator: v2,
                stake: 200,
            },
            Bond {
                validator: v1,
                stake: 100,
            },
        ];
        assert_eq!(
            hash_block(&a),
            hash_block(&b),
            "bonds root must be invariant under in-memory ordering"
        );
    }

    #[test]
    fn deploy_inclusion_proof_verifies_against_block_hash() {
        // Given a block, a validator with the signed block_hash and
        // h_header + the deploy's Merkle path can verify that a
        // specific deploy appears in the block without the whole body.
        let mut b = empty_block();
        b.body.deploys = (0..5).map(processed_deploy).collect();
        b.body.system_deploys = vec![processed_system_deploy_close()];
        let block_hash = hash_block(&b);

        let leaves = deploys_root_leaves(&b);
        // Pick a middle leaf — tests the general proof path.
        let target_index = 3u32;
        let target_leaf = leaves[target_index as usize];

        // Deploys-root sibling path.
        let deploys_proof = merkle::build_proof(&MERKLE_PARAMS, &leaves, target_index)
            .expect("proof for valid index");
        let d_root = merkle::merkle_root(&MERKLE_PARAMS, &leaves);
        assert!(
            merkle::verify_proof(&MERKLE_PARAMS, &d_root, &target_leaf, &deploys_proof),
            "deploys-tree inclusion proof must verify",
        );

        // Top-level: combine recomputed d_root with recomputed h_header.
        let header_preimage_bytes = header_preimage(&b);
        let h_header = merkle::hash_leaf_tagged(LEAF_HEADER, &header_preimage_bytes);
        let recomputed_top = hash_top(&h_header, &d_root);
        assert_eq!(
            recomputed_top.to_vec(),
            block_hash.to_vec(),
            "recomputed top node must equal block_hash",
        );
    }

    /// Determinism guard: a `ProcessedDeploy` built twice with the same
    /// data must prost-encode to byte-identical output.  The Merkle
    /// leaf hash is `Blake2b256(LEAF_DOMAIN || prost(pd))`, so any
    /// nondeterminism in prost's encoding of these messages (e.g. a
    /// future proto edit that adds a `map<_,_>` field) would silently
    /// desync validators' block hashes.  This test guards against that
    /// drift at the lowest cost: byte stability of two independent
    /// encodings.
    #[test]
    fn processed_deploy_encodes_deterministically() {
        let a = processed_deploy(42);
        let b = processed_deploy(42);
        // `processed_deploy` uses a fresh keypair each call, so the
        // signatures differ — strip the signature-related fields and
        // compare the remainder for byte-stability of the proto
        // encoding ITSELF (not the random key).
        let bytes_of = |pd: &ProcessedUserDeploy| -> Vec<u8> {
            use prost::Message as _;
            // Zero out non-content fields that vary with the random
            // keypair.
            let mut normalized = pd.clone();
            let ProcessedUserDeploy::Legacy(deploy) = &mut normalized else {
                panic!("expected a legacy fixture");
            };
            deploy.deploy.sig = Bytes::new();
            deploy.deploy.pk = crypto::rust::public_key::PublicKey::from_bytes(&[]);
            normalized.to_proto().encode_to_vec()
        };
        assert_eq!(
            bytes_of(&a),
            bytes_of(&b),
            "ProcessedDeploy prost encoding must be byte-deterministic for identical content"
        );
        // Encode twice: same bytes.
        assert_eq!(bytes_of(&a), bytes_of(&a));
    }

    #[test]
    fn processed_system_deploy_encodes_deterministically() {
        use prost::Message as _;
        let a = processed_system_deploy_close();
        let b = processed_system_deploy_close();
        let ea = a.clone().to_proto().encode_to_vec();
        let eb = b.to_proto().encode_to_vec();
        assert_eq!(ea, eb);
        // Encode twice: same bytes.
        assert_eq!(
            a.clone().to_proto().encode_to_vec(),
            a.to_proto().encode_to_vec()
        );
    }

    // ---------------------------------------------------------------
    // Gap-closing tests from the coverage review (tests 1–5).
    // ---------------------------------------------------------------

    /// Test 1 — end-to-end sign → verify round-trip through
    /// `sign_block_with_leaves`.  The paper's hot path is: compute
    /// leaves during replay, pass them to `sign_block_with_leaves`,
    /// gossip the signed block, every other node recomputes the hash
    /// via `hash_block(stored_block)`.  If the leaves are ever threaded
    /// in the wrong order, the signed `block_hash` and the recomputed
    /// hash diverge and the block is rejected by every validator.
    /// This test pins the contract: for any valid (block, leaves)
    /// pair where leaves == `deploys_root_leaves(block)` in commit
    /// order, `sign_block_with_leaves(block, leaves)` produces a
    /// signed block whose `block_hash` equals `hash_block(signed)`.
    #[test]
    fn sign_block_with_leaves_roundtrips_to_hash_block() {
        use crypto::rust::private_key::PrivateKey;

        use crate::rust::validator_identity::ValidatorIdentity;

        let mut unsigned = empty_block();
        unsigned.body.deploys = vec![
            processed_deploy(1),
            processed_deploy(2),
            processed_deploy(3),
        ];
        unsigned.body.system_deploys = vec![
            processed_system_deploy_close(),
            processed_system_deploy_slash(0x42),
        ];
        // Also give the block some rejected deploys and applied-from-scope
        // so the header preimage is non-trivial.
        unsigned.body.rejected_deploys = vec![RejectedDeploy {
            sig: Bytes::from(vec![0x77; 64]),
            duplicate: false,
            carrier: Bytes::from(vec![0x88; 32]),
        }];
        unsigned.body.applied_from_scope = vec![Bytes::from(vec![0x99; 64])];

        // Compute leaves the way the propose path would: from the body
        // in commit order (user deploys first, then system).  This is
        // the same vector `replay_deploys` streams out.
        let leaves = deploys_root_leaves(&unsigned);

        // Sign.  `sign_block_with_leaves` sets sender + sig_algorithm +
        // computes `block_hash` from the leaves + signs that hash.
        let sk = PrivateKey::from_bytes(&[0x11; 32]);
        let vi = ValidatorIdentity::new(&sk);
        let signed = vi.sign_block_with_leaves(&unsigned, &leaves);

        // The signed block_hash must equal what a receiver computes
        // via Validate::block_hash → hash_block.
        let recomputed = hash_block(&signed);
        assert_eq!(
            signed.block_hash.to_vec(),
            recomputed.to_vec(),
            "sign_block_with_leaves produced a hash that hash_block disagrees with — \
             every validator would reject this block"
        );

        // And the slow-path sign (which recomputes leaves itself) must
        // agree with the fast path.
        let signed_slow = vi.sign_block(&unsigned);
        assert_eq!(
            signed.block_hash.to_vec(),
            signed_slow.block_hash.to_vec(),
            "sign_block and sign_block_with_leaves disagree on block_hash"
        );
    }

    /// Test 2 — the per-deploy leaf stream from `replay_deploys` is
    /// identical to what `deploys_root_leaves` recomputes from the
    /// block body.  This is the propose/validate agreement contract:
    /// propose hashes leaves on the fly, validate recomputes them from
    /// the stored body, and the two sequences MUST match element-wise.
    /// A silent reordering in the replay loop would ship bad blocks.
    #[test]
    fn replay_order_leaves_equal_body_order_leaves() {
        // Simulate the replay loop: user deploys first (in commit
        // order), then system deploys.  Each leaf is hashed with the
        // same per-item hash the stored-body recomputation uses.
        let user: Vec<ProcessedUserDeploy> = (0..5).map(processed_deploy).collect();
        let system: Vec<ProcessedSystemDeploy> = vec![
            processed_system_deploy_close(),
            processed_system_deploy_slash(0x11),
            processed_system_deploy_failed("replay-error"),
        ];
        let streamed: Vec<[u8; 32]> = user
            .iter()
            .map(hash_processed_user_deploy_leaf)
            .chain(system.iter().map(hash_processed_system_deploy_leaf))
            .collect();

        let mut b = empty_block();
        b.body.deploys = user;
        b.body.system_deploys = system;
        let recomputed = deploys_root_leaves(&b);

        assert_eq!(
            streamed, recomputed,
            "replay-time leaf stream must equal body-recompute leaf stream — \
             a mismatch means propose and validate would disagree on block_hash"
        );
    }

    /// Test 3a — all three `ProcessedSystemDeploy` variants
    /// (Close/Slash/Failed) encode deterministically and change the
    /// block hash when inserted.  CloseBlockSystemDeploy was already
    /// covered; this fills in Slash and Failed.
    #[test]
    fn all_system_deploy_variants_affect_hash_and_encode_deterministically() {
        use prost::Message as _;

        let variants: Vec<(&str, ProcessedSystemDeploy)> = vec![
            ("close", processed_system_deploy_close()),
            ("slash", processed_system_deploy_slash(0x33)),
            ("failed", processed_system_deploy_failed("insufficient gas")),
        ];

        // Each variant must prost-encode byte-stably.
        for (name, v) in &variants {
            let a = v.clone().to_proto().encode_to_vec();
            let b = v.clone().to_proto().encode_to_vec();
            assert_eq!(a, b, "{name} must prost-encode deterministically");
        }

        // Each variant must produce a distinct block hash when it's the
        // sole system deploy — guards against the proto encoding
        // accidentally emitting identical bytes across variants.
        let mut hashes = std::collections::HashSet::new();
        let empty_hash = hash_block(&empty_block());
        hashes.insert(empty_hash.to_vec());
        for (name, v) in variants {
            let mut b = empty_block();
            b.body.system_deploys = vec![v];
            let h = hash_block(&b);
            assert!(
                hashes.insert(h.to_vec()),
                "{name} variant did not change block_hash — hash collision across variants",
            );
        }
    }

    /// Test 3b — Slash with different field values produces different
    /// hashes.  Guards against the oneof encoding dropping a field.
    #[test]
    fn slash_system_deploy_field_mutations_change_hash() {
        let mut b = empty_block();
        b.body.system_deploys = vec![processed_system_deploy_slash(0x01)];
        let h0 = hash_block(&b);

        let mut b2 = empty_block();
        b2.body.system_deploys = vec![processed_system_deploy_slash(0x02)];
        assert_ne!(
            hash_block(&b2),
            h0,
            "Slash with different seed must produce different hash"
        );

        // Build a Slash with modified target_activation_epoch only.
        let mut b3 = empty_block();
        b3.body.system_deploys = vec![ProcessedSystemDeploy::Succeeded {
            event_list: vec![],
            system_deploy: SystemDeployData::Slash {
                invalid_block_hash: Bytes::from(vec![0x01; 32]),
                issuer_public_key: crypto::rust::public_key::PublicKey::from_bytes(&[0x01; 33]),
                target_activation_epoch: 999, // vs. seed=0x01 → 0x01+100 = 101
            },
        }];
        assert_ne!(
            hash_block(&b3),
            h0,
            "Slash.target_activation_epoch must affect hash",
        );
    }

    /// Test 4 — strong determinism guard: no `map<_,_>` field exists
    /// on any protobuf message transitively reachable from
    /// `ProcessedDeployProto` or `ProcessedSystemDeployProto`.  A
    /// `map<>` field under proto3 encodes map entries in
    /// implementation-dependent order, which would silently desync
    /// block hashes across validators.  This static check runs at
    /// test time against the committed .proto files.
    ///
    /// If this test fails: either (a) a new reachable message was
    /// added — list it in `REACHABLE` below after confirming it has
    /// no `map<>` field, or (b) a `map<>` field was added to an
    /// existing message — redesign the leaf-hash construction to
    /// canonicalize the map before hashing, or remove the map field.
    #[test]
    fn processed_deploy_subtree_has_no_proto_map_fields() {
        const CASPER_PROTO: &str =
            include_str!("../../../../models/src/main/protobuf/CasperMessage.proto");
        const RHOTYPES_PROTO: &str =
            include_str!("../../../../models/src/main/protobuf/RhoTypes.proto");

        // Transitive closure of messages reachable from
        // ProcessedDeployProto and ProcessedSystemDeployProto.  Review
        // when adding any new field to any of these messages.
        const REACHABLE_IN_CASPER: &[&str] = &[
            "ProcessedDeployProto",
            "ProcessedSystemDeployProto",
            "DeployDataProto",
            "DeployParameter",
            "RholangValue",
            "RholangSequence",
            "RholangMap",
            "RholangMapEntry",
            "RholangNil",
            "SystemDeployDataProto",
            "SlashSystemDeployDataProto",
            "CloseBlockSystemDeployDataProto",
            "EventProto",
            "ProduceEventProto",
            "ConsumeEventProto",
            "CommEventProto",
            "PeekProto",
        ];
        const REACHABLE_IN_RHOTYPES: &[&str] = &["PCost"];

        for name in REACHABLE_IN_CASPER {
            let body = extract_message_body(CASPER_PROTO, name)
                .unwrap_or_else(|| panic!("message `{name}` not found in CasperMessage.proto"));
            let stripped = strip_proto_line_comments(&body);
            assert!(
                !stripped.contains("map<"),
                "message `{name}` contains a proto `map<>` field.  Maps are nondeterministically \
                 encoded by prost under proto3, which would desync block hashes across \
                 validators.  Either (a) replace the map with a repeated-of-entries message \
                 and sort before hashing, or (b) canonicalize the map in the leaf-hash input."
            );
        }
        for name in REACHABLE_IN_RHOTYPES {
            let body = extract_message_body(RHOTYPES_PROTO, name)
                .unwrap_or_else(|| panic!("message `{name}` not found in RhoTypes.proto"));
            let stripped = strip_proto_line_comments(&body);
            assert!(
                !stripped.contains("map<"),
                "message `{name}` (RhoTypes.proto) contains a proto `map<>` field — see \
                 processed_deploy_subtree_has_no_proto_map_fields docs for the fix path."
            );
        }
    }

    /// Extract the body (text between the message's opening `{` and its
    /// matching `}`) for the named message in a .proto file.  Handles
    /// nested braces (none expected in our schemas but defensive).
    fn extract_message_body(proto: &str, msg_name: &str) -> Option<String> {
        // Match either `message Foo {` or `message Foo{` (no space).
        let needle_a = format!("message {msg_name} {{");
        let needle_b = format!("message {msg_name}{{");
        let start = proto.find(&needle_a).or_else(|| proto.find(&needle_b))?;
        let body_start = proto[start..].find('{')? + start + 1;
        let bytes = proto.as_bytes();
        let mut depth: i32 = 1;
        let mut i = body_start;
        while i < bytes.len() {
            match bytes[i] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(proto[body_start..i].to_string());
                    }
                }
                _ => {}
            }
            i += 1;
        }
        None
    }

    /// Strip `//` line comments.  The .proto files use `//` for comments
    /// that may contain the text `map<` incidentally; the brace parser
    /// doesn't care about these but the `map<` substring check would
    /// false-positive without stripping.
    fn strip_proto_line_comments(s: &str) -> String {
        s.lines()
            .map(|l| l.split("//").next().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Test 5a — `hash_block_with_leaves` pins its own contract:
    /// given leaves that DO match the body (commit-order deploys),
    /// output equals the slow-path `hash_block`.  Given leaves that
    /// don't match, output is different.  This catches a bug where
    /// the fast path silently normalizes / sorts / dedupes leaves
    /// internally — which would mask propose/validate desync.
    #[test]
    fn hash_block_with_leaves_respects_the_leaves_argument() {
        let mut b = empty_block();
        b.body.deploys = vec![processed_deploy(1), processed_deploy(2)];
        b.body.system_deploys = vec![processed_system_deploy_close()];

        let correct_leaves = deploys_root_leaves(&b);
        let h_correct = hash_block_with_leaves(&b, &correct_leaves);
        assert_eq!(
            h_correct.to_vec(),
            hash_block(&b).to_vec(),
            "correct leaves must agree with recompute path",
        );

        // All-zero leaves of correct length: must differ.
        let zero_leaves = vec![[0u8; 32]; correct_leaves.len()];
        assert_ne!(
            hash_block_with_leaves(&b, &zero_leaves).to_vec(),
            h_correct.to_vec(),
            "zeroed leaves must produce a different hash — fast path must not ignore its input",
        );

        // Reordered leaves: must differ.
        let mut reversed = correct_leaves.clone();
        reversed.reverse();
        assert_ne!(
            hash_block_with_leaves(&b, &reversed).to_vec(),
            h_correct.to_vec(),
            "reversed leaves must produce a different hash — fast path must not sort internally",
        );
    }

    /// Test 5b — wrong leaf count (too few / too many) produces a
    /// different hash.  Catches a bug where the fast path silently
    /// pads or truncates to match the body length.
    #[test]
    fn hash_block_with_leaves_rejects_wrong_cardinality() {
        let mut b = empty_block();
        b.body.deploys = vec![processed_deploy(1), processed_deploy(2)];
        let correct_leaves = deploys_root_leaves(&b);
        let h_correct = hash_block_with_leaves(&b, &correct_leaves);

        // Truncated: 1 leaf instead of 2.
        let truncated = &correct_leaves[..1];
        assert_ne!(
            hash_block_with_leaves(&b, truncated).to_vec(),
            h_correct.to_vec(),
            "truncated leaves must produce a different hash",
        );

        // Padded: 3 leaves instead of 2.
        let mut padded = correct_leaves.clone();
        padded.push([0xAA; 32]);
        assert_ne!(
            hash_block_with_leaves(&b, &padded).to_vec(),
            h_correct.to_vec(),
            "padded leaves must produce a different hash",
        );

        // Empty leaves on non-empty body: must differ.
        assert_ne!(
            hash_block_with_leaves(&b, &[]).to_vec(),
            h_correct.to_vec(),
            "empty leaves on non-empty body must produce a different hash",
        );
    }

    /// Golden hex for the empty block fixture.  Pins the whole hash
    /// recipe (header-preimage field order, domain bytes, scheme
    /// version, top-combiner).  Any accidental change to the Merkle
    /// scheme trips this test visibly in CI.  Regenerate ONLY when
    /// intentionally hard-forking the shard's block-hash scheme.
    #[test]
    fn hash_block_empty_golden_hex() {
        let b = empty_block();
        let h = hash_block(&b);
        let hex = h.iter().fold(String::with_capacity(64), |mut acc, byte| {
            use std::fmt::Write;
            let _ = write!(acc, "{byte:02x}");
            acc
        });
        // Regenerate via:
        //   cargo test -p casper merkle_block_hash_tests::hash_block_empty_golden_hex -- --nocapture
        // ONLY when intentionally hard-forking the Merkle scheme.  The
        // empty-block fixture is defined by `empty_block()` above:
        // zero deploys, zero system deploys, zero bonds, 32-byte zero
        // pre/post state hashes, sender = 33 zero bytes, timestamp =
        // 1_700_000_000_000, version = 1, shard_id = "test-shard",
        // sig_algorithm = "secp256k1".
        const EXPECTED: &str = "e1959261b81b13412671713f51402b701fee1e1e261970e0f9215df87b051d23";
        assert_eq!(hex, EXPECTED, "block-hash recipe changed");
    }
}
