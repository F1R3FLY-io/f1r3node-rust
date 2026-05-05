// Shared helpers for the Track 2 production-path integration
// tests and Track 3 triple-bisimilarity proptests.
//
// Reference: docs/theory/slashing/design/14-test-plan.md §14.3.5
// (production-path integration), §14.5 (cross-tier bisim).
// Plan-agent design from session ending at commit 030336a.

#![allow(dead_code)]

use std::collections::HashMap;

use casper::rust::casper::Casper;
use casper::rust::errors::CasperError;
use casper::rust::util::proto_util;
use casper::rust::util::rholang::costacc::close_block_deploy::CloseBlockDeploy;
use casper::rust::util::rholang::interpreter_util;
use casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum;
use casper::rust::util::rholang::system_deploy_util;
use casper::rust::validator_identity::ValidatorIdentity;
use crypto::rust::signatures::signed::Signed;
use models::rust::casper::protocol::casper_message::{BlockMessage, DeployData};
use prost::bytes::Bytes;
use rholang::rust::interpreter::system_processes::BlockData;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisContext;

use super::production_adapter::SlashingProductionAdapter;

/// Capture a production-tier snapshot at the post-state hash of
/// `block`. Reads bonds and active set from the node's
/// `RuntimeManager`, projects equivocation records from the node's
/// `BlockDagKeyValueStorage`, and computes `coop_vault` via the
/// stake-conservation formula.
///
/// `validators` is the canonical "v0", "v1", ... mapping established
/// by `canonical_validator_order(genesis)`.
pub async fn production_snapshot_at(
    node: &TestNode,
    block: &BlockMessage,
    genesis_block: &BlockMessage,
    validators: Vec<Bytes>,
) -> Result<SlashingProductionAdapter, CasperError> {
    let post_state_hash = proto_util::post_state_hash(block);

    let bonds = node
        .runtime_manager
        .compute_bonds(&post_state_hash)
        .await?;
    let active_set = node
        .runtime_manager
        .get_active_validators(&post_state_hash)
        .await?;

    // Convert Bond { validator: ByteString, stake: i64 } to the
    // adapter's expected `&[(Bytes, i64)]`.
    let bonds_table: Vec<(Bytes, i64)> = bonds
        .iter()
        .map(|b| (b.validator.clone(), b.stake))
        .collect();
    let active_table: Vec<Bytes> = active_set.iter().map(|v| v.clone()).collect();

    // Coop-vault formula per design §14.6 stake-conservation
    // invariant: genesis_total_stake = sum(current_bonds) + coop_vault
    // (modulo bond-floor residue from already-slashed validators).
    let genesis_total: i64 = genesis_block.body.state.bonds.iter().map(|b| b.stake).sum();
    let current_total: i64 = bonds.iter().map(|b| b.stake).sum();
    // Bond-floor residue: bonds at the test-mode floor (typically 1)
    // are validators that have been slashed but still hold the floor
    // amount. `coop_vault = genesis_total - current_total - residue`.
    let bond_floor: i64 = 1;
    let residue: i64 = bonds.iter().filter(|b| b.stake == bond_floor).count() as i64 * bond_floor;
    let coop_vault = genesis_total - current_total - residue;

    SlashingProductionAdapter::snapshot(
        validators,
        &bonds_table,
        &active_table,
        &node.block_dag_storage,
        coop_vault.max(0),
    )
    .map_err(CasperError::RuntimeError)
}

/// Build the canonical "v0", "v1", ... validator-label mapping
/// from a genesis block's bond list. The order is determined by
/// the genesis bond entries' iteration order, which mirrors the
/// `validator_key_pairs` order in `GenesisContext`.
pub fn canonical_validator_order(genesis: &GenesisContext) -> Vec<Bytes> {
    genesis
        .validator_pks()
        .into_iter()
        .map(|pk| pk.bytes)
        .collect()
}

/// Map a validator's public-key bytes back to its harness label.
/// `None` if the validator is not in the canonical order.
pub fn label_for(validator_bytes: &Bytes, validators: &[Bytes]) -> Option<String> {
    validators
        .iter()
        .position(|v| v.as_ref() == validator_bytes.as_ref())
        .map(|i| format!("v{}", i))
}

/// Build a label → bond mapping from the production tier's bonds
/// list, indexed by the canonical order.
pub fn bonds_by_label(
    bonds: &[(Bytes, i64)],
    validators: &[Bytes],
) -> HashMap<String, i64> {
    let mut out = HashMap::new();
    for (b, stake) in bonds {
        if let Some(label) = label_for(b, validators) {
            out.insert(label, *stake);
        }
    }
    out
}

/// Construct a Byzantine-equivocation sibling of `b1`: a properly-
/// signed block by the SAME validator at the SAME `seq_num` and
/// SAME parents/justifications as `b1`, but with a different deploy
/// set. Replay validation passes for this block in isolation; only
/// `check_equivocations` (run after the block's own validation)
/// detects the equivocation by comparing creator-justifications
/// against the receiving node's already-recorded latest message.
///
/// Per the Plan agent's design (§Track 2 of design/14a-tier-
/// architecture.md): the only correct construction is to pick a
/// distinct deploy set and run replay forward — no header-mutation
/// shortcut works because system deploys (`rho:block:data`) read
/// header fields and recompute via `compute_state_with_bonds`.
///
/// `producing_node` must be a TestNode whose DAG state is the
/// SAME parent state `b1` was created from (typically genesis).
/// The simplest way to get that: spin up a fresh TestNode using
/// v0's key, do not feed it `b1`, then call `equivocate_block`.
///
/// `alt_deploys` must be DISJOINT from `b1.body.deploys` (otherwise
/// `Validate::repeat_deploy` rejects). Use distinct deploy nonces.
pub async fn equivocate_block(
    producing_node: &mut TestNode,
    b1: &BlockMessage,
    alt_deploys: Vec<Signed<DeployData>>,
) -> Result<BlockMessage, CasperError> {
    let validator_identity = producing_node
        .validator_id_opt
        .as_ref()
        .ok_or_else(|| {
            CasperError::RuntimeError(
                "producing_node has no validator identity".to_string(),
            )
        })?
        .clone();

    // Take a snapshot from the producing node — its DAG must NOT
    // contain b1 (otherwise the snapshot's max_seq_num for v0 would
    // be b1.seq_num, and the new block's seq_num would be b1.seq_num + 1).
    let snapshot = producing_node.casper.get_snapshot().await?;

    let next_block_num = b1.body.state.block_number;
    let next_seq_num = b1.seq_num;
    let shard_id = snapshot.on_chain_state.shard_conf.shard_name.clone();

    // Build the same parent set b1 used.
    let parents = snapshot.parents.clone();
    let justifications: Vec<_> = snapshot.justifications.iter().map(|j| j.clone()).collect();

    // Build BlockData from b1's header (matching the timestamp in
    // particular — `rho:block:data` reads it during replay; an
    // honest sibling at the same sequence COULD have been produced
    // at a different millisecond, but using b1's timestamp keeps
    // the replay invariant simple). The `block_number` and
    // `seq_num` match b1's; the sender is v0.
    let block_data = BlockData {
        time_stamp: b1.header.timestamp,
        block_number: next_block_num,
        sender: validator_identity.public_key.clone(),
        seq_num: next_seq_num,
    };

    // System deploys: just CloseBlock. No SlashDeploys (this is the
    // Byzantine validator's first-equivocation block — it would not
    // self-slash).
    let system_deploys = vec![SystemDeployEnum::Close(CloseBlockDeploy {
        initial_rand: system_deploy_util::generate_close_deploy_random_seed_from_pk(
            validator_identity.public_key.clone(),
            next_seq_num,
        ),
    })];

    let invalid_blocks = snapshot.invalid_blocks.clone();

    // Compute checkpoint via the real interpreter — this is what
    // gives us a post-state hash that matches replay.
    let checkpoint = interpreter_util::compute_deploys_checkpoint(
        &mut producing_node.block_store,
        parents.clone(),
        alt_deploys,
        system_deploys,
        &snapshot,
        &mut producing_node.runtime_manager,
        block_data.clone(),
        invalid_blocks,
    )
    .await?;

    let (
        pre_state_hash,
        post_state_hash,
        processed_deploys,
        rejected_deploys,
        processed_system_deploys,
        new_bonds,
    ) = checkpoint;

    let casper_version = snapshot.on_chain_state.shard_conf.casper_version;

    // Inline the equivalent of `block_creator::package_block` —
    // that function is private to block_creator.rs (`fn`, not
    // `pub fn`), so we replicate its 25-line body here. The
    // proto_util helpers are public.
    use models::rust::casper::protocol::casper_message::{
        Body, F1r3flyState, Header, RejectedDeploy,
    };

    let state = F1r3flyState {
        pre_state_hash,
        post_state_hash,
        bonds: new_bonds,
        block_number: block_data.block_number,
    };
    let rejected_deploys_wrapped: Vec<RejectedDeploy> = rejected_deploys
        .into_iter()
        .map(|sig| RejectedDeploy { sig })
        .collect();
    let body = Body {
        state,
        deploys: processed_deploys,
        rejected_deploys: rejected_deploys_wrapped,
        system_deploys: processed_system_deploys,
        extra_bytes: Bytes::new(),
    };
    let header = Header {
        parents_hash_list: parents.iter().map(|p| p.block_hash.clone()).collect(),
        timestamp: block_data.time_stamp,
        version: casper_version,
        extra_bytes: Bytes::new(),
    };
    let unsigned = proto_util::unsigned_block_proto(
        body,
        header,
        justifications,
        shard_id,
        Some(block_data.seq_num),
    );

    // Sign with v0's identity — this is the Byzantine signing step.
    let signed = validator_identity.sign_block(&unsigned);
    Ok(signed)
}
