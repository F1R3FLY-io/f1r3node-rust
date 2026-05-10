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
use casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum;
use casper::rust::util::rholang::{interpreter_util, system_deploy_util};
use casper::rust::validator_identity::ValidatorIdentity;
use crypto::rust::signatures::signed::Signed;
use models::rust::casper::protocol::casper_message::{BlockMessage, DeployData};
use prost::bytes::Bytes;
use rholang::rust::interpreter::system_processes::BlockData;

use super::production_adapter::SlashingProductionAdapter;
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisContext;

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

    let bonds = node.runtime_manager.compute_bonds(&post_state_hash).await?;
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
pub fn bonds_by_label(bonds: &[(Bytes, i64)], validators: &[Bytes]) -> HashMap<String, i64> {
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
            CasperError::RuntimeError("producing_node has no validator identity".to_string())
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

/// Like `propose_neglecting_block` but with caller-supplied
/// `Justification` entries — used when the producing node's
/// natural snapshot does NOT contain a justification we need (for
/// example, because the latest-messages-index does not promote
/// invalid blocks). The caller-supplied justifications MERGE with
/// snapshot's natural justifications, with caller's overriding
/// for any (validator) key collision.
///
/// Use case: NeglectedEquivocation production-tier integration
/// test — the seeder must produce a block whose justifications
/// explicitly cite b1p (an invalid block), but the natural
/// snapshot only carries valid latest messages.
pub async fn propose_with_explicit_justifications(
    producing_node: &mut TestNode,
    alt_deploys: Vec<Signed<DeployData>>,
    extra_justifications: Vec<models::rust::casper::protocol::casper_message::Justification>,
) -> Result<BlockMessage, CasperError> {
    let validator_identity = producing_node
        .validator_id_opt
        .as_ref()
        .ok_or_else(|| {
            CasperError::RuntimeError("producing_node has no validator identity".to_string())
        })?
        .clone();

    let snapshot = producing_node.casper.get_snapshot().await?;

    let next_seq_num = snapshot
        .max_seq_nums
        .get(&validator_identity.public_key.bytes)
        .map(|seq| (*seq + 1) as i32)
        .unwrap_or(1);
    let next_block_num = snapshot.max_block_num + 1;
    let shard_id = snapshot.on_chain_state.shard_conf.shard_name.clone();

    let parents = snapshot.parents.clone();

    use std::collections::HashMap;
    let mut merged: HashMap<
        prost::bytes::Bytes,
        models::rust::casper::protocol::casper_message::Justification,
    > = HashMap::new();
    for j in snapshot.justifications.iter() {
        merged.insert(j.validator.clone(), j.clone());
    }
    for j in extra_justifications {
        merged.insert(j.validator.clone(), j);
    }
    let justifications: Vec<_> = merged.into_values().collect();

    let parent_max_ts = parents
        .iter()
        .map(|p| p.header.timestamp)
        .max()
        .unwrap_or(0);
    let block_data = BlockData {
        time_stamp: parent_max_ts + 1,
        block_number: next_block_num,
        sender: validator_identity.public_key.clone(),
        seq_num: next_seq_num,
    };

    let system_deploys = vec![SystemDeployEnum::Close(CloseBlockDeploy {
        initial_rand: system_deploy_util::generate_close_deploy_random_seed_from_pk(
            validator_identity.public_key.clone(),
            next_seq_num,
        ),
    })];

    let invalid_blocks = snapshot.invalid_blocks.clone();

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

    let signed = validator_identity.sign_block(&unsigned);
    Ok(signed)
}

/// Process a block bypassing `check_if_of_interest` (the upstream
/// shard / version / age filter on
/// `BlockProcessor::check_if_of_interest`). Used for UC-32
/// InvalidShardId where the upstream filter would otherwise reject
/// the block as `NotOfInterest` BEFORE reaching the
/// `Validate::shard_identifier` validator inside `block_summary`.
///
/// The deeper-layer `InvalidShardId` is defence-in-depth — the same
/// check at a different layer of the pipeline. The dispatcher's
/// catch-all routes it through `is_slashable()` (block_status.rs:181)
/// so we still want to verify the catch-all minted a record when
/// it fires; we just have to bypass the upstream early-rejection.
///
/// Reference: `casper/tests/helper/test_node.rs::process_block_through_pipe`
/// for the full pipeline.
pub async fn process_block_bypassing_of_interest_filter(
    node: &mut TestNode,
    block: models::rust::casper::protocol::casper_message::BlockMessage,
) -> Result<
    rspace_plus_plus::rspace::history::Either<
        casper::rust::block_status::BlockError,
        casper::rust::block_status::ValidBlock,
    >,
    CasperError,
> {
    use casper::rust::block_status::BlockStatus;
    let is_well_formed = node
        .block_processor
        .check_if_well_formed_and_store(&block)
        .await?;
    if !is_well_formed {
        return Ok(rspace_plus_plus::rspace::history::Either::Left(
            BlockStatus::invalid_format(),
        ));
    }
    let dependencies_ready = node
        .block_processor
        .check_dependencies_with_effects(node.casper.clone(), &block)
        .await?;
    if !dependencies_ready {
        return Ok(rspace_plus_plus::rspace::history::Either::Left(
            BlockStatus::missing_blocks(),
        ));
    }
    node.block_processor
        .validate_with_effects(node.casper.clone(), &block, None)
        .await
}

/// Build a normally-proposed block, then apply `mutator` to the
/// unsigned block before signing. The signing step recomputes the
/// block_hash for the mutated body, so the resulting block is
/// correctly-signed but semantically invalid in whatever way the
/// mutator dictates.
///
/// Use case: drive each non-equivocation slashable `InvalidBlock`
/// variant from production validators (Item 6 of the principled-
/// resolution session). Each Tier-1 production-driven variant test
/// passes a different `mutator` that sabotages the field its
/// targeted validator inspects, while keeping all earlier validators
/// happy. For the validator order see
/// `casper/src/rust/validate.rs::block_summary` and
/// `multi_parent_casper_impl.rs::validate_block_checkpoint` flow.
pub async fn propose_with_block_mutation(
    producing_node: &mut TestNode,
    alt_deploys: Vec<Signed<DeployData>>,
    mutator: impl FnOnce(&mut models::rust::casper::protocol::casper_message::BlockMessage),
) -> Result<BlockMessage, CasperError> {
    let validator_identity = producing_node
        .validator_id_opt
        .as_ref()
        .ok_or_else(|| {
            CasperError::RuntimeError("producing_node has no validator identity".to_string())
        })?
        .clone();

    let snapshot = producing_node.casper.get_snapshot().await?;

    let next_seq_num = snapshot
        .max_seq_nums
        .get(&validator_identity.public_key.bytes)
        .map(|seq| (*seq + 1) as i32)
        .unwrap_or(1);
    let next_block_num = snapshot.max_block_num + 1;
    let shard_id = snapshot.on_chain_state.shard_conf.shard_name.clone();

    let parents = snapshot.parents.clone();
    let justifications: Vec<_> = snapshot.justifications.iter().map(|j| j.clone()).collect();

    let parent_max_ts = parents
        .iter()
        .map(|p| p.header.timestamp)
        .max()
        .unwrap_or(0);
    let block_data = BlockData {
        time_stamp: parent_max_ts + 1,
        block_number: next_block_num,
        sender: validator_identity.public_key.clone(),
        seq_num: next_seq_num,
    };

    let system_deploys = vec![SystemDeployEnum::Close(CloseBlockDeploy {
        initial_rand: system_deploy_util::generate_close_deploy_random_seed_from_pk(
            validator_identity.public_key.clone(),
            next_seq_num,
        ),
    })];

    let invalid_blocks = snapshot.invalid_blocks.clone();

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
    let mut unsigned = proto_util::unsigned_block_proto(
        body,
        header,
        justifications,
        shard_id,
        Some(block_data.seq_num),
    );

    // Apply the caller's mutation BEFORE signing. The signing step
    // recomputes the block_hash, so the mutated block is correctly
    // signed but its body deliberately violates the validator the
    // test targets.
    mutator(&mut unsigned);

    let signed = validator_identity.sign_block(&unsigned);
    Ok(signed)
}

/// Construct an honest "neglecting" block by `producing_node`'s
/// validator: the block cites the receiver's view (which contains
/// the equivocator's bad block) in justifications but EXPLICITLY
/// omits any SlashDeploy. When processed on a node whose tracker
/// already holds the equivocator's record, the receiver's
/// `is_neglected_equivocation_detected_with_update` fires and
/// classifies as `InvalidBlock::NeglectedEquivocation`.
///
/// Reference: docs/theory/slashing/design/14-test-plan.md §14.3.5
/// (production-path integration). Plan-agent designed Item 5 of
/// the principled-resolution session.
///
/// Why this helper exists: the production proposer's
/// `prepare_slashing_deploys` would auto-emit a SlashDeploy for
/// any validator with an outstanding `EquivocationRecord` in the
/// proposer's tracker view. To produce a NEGLECTING block — one
/// that ignores the offence — we bypass `prepare_slashing_deploys`
/// entirely by going through `compute_deploys_checkpoint` with
/// `system_deploys = [CloseBlockDeploy]` only.
pub async fn propose_neglecting_block(
    producing_node: &mut TestNode,
    alt_deploys: Vec<Signed<DeployData>>,
) -> Result<BlockMessage, CasperError> {
    let validator_identity = producing_node
        .validator_id_opt
        .as_ref()
        .ok_or_else(|| {
            CasperError::RuntimeError("producing_node has no validator identity".to_string())
        })?
        .clone();

    let snapshot = producing_node.casper.get_snapshot().await?;

    // Compute the proposer's natural seq + block_num from snapshot.
    let next_seq_num = snapshot
        .max_seq_nums
        .get(&validator_identity.public_key.bytes)
        .map(|seq| (*seq + 1) as i32)
        .unwrap_or(1);
    let next_block_num = snapshot.max_block_num + 1;
    let shard_id = snapshot.on_chain_state.shard_conf.shard_name.clone();

    // Use the snapshot's natural parents and justifications. The
    // CRITICAL property: producing_node's snapshot must already
    // contain the equivocator's invalid block in its DAG (so the
    // receiver's check_neglected_equivocations_with_update
    // recognises that this block "saw" the equivocation).
    let parents = snapshot.parents.clone();
    let justifications: Vec<_> = snapshot.justifications.iter().map(|j| j.clone()).collect();

    // Honest block timestamp: pick "now" matching production
    // semantics. Use the max parent timestamp + 1 so we satisfy
    // the parent-timestamp ordering.
    let parent_max_ts = parents
        .iter()
        .map(|p| p.header.timestamp)
        .max()
        .unwrap_or(0);
    let block_data = BlockData {
        time_stamp: parent_max_ts + 1,
        block_number: next_block_num,
        sender: validator_identity.public_key.clone(),
        seq_num: next_seq_num,
    };

    // System deploys: ONLY CloseBlock. NO SlashDeploys — this is
    // the structural difference from production
    // `prepare_slashing_deploys`. The receiver's
    // `is_neglected_equivocation_detected_with_update` will see
    // the missing slash and classify NeglectedEquivocation.
    let system_deploys = vec![SystemDeployEnum::Close(CloseBlockDeploy {
        initial_rand: system_deploy_util::generate_close_deploy_random_seed_from_pk(
            validator_identity.public_key.clone(),
            next_seq_num,
        ),
    })];

    let invalid_blocks = snapshot.invalid_blocks.clone();

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

    let signed = validator_identity.sign_block(&unsigned);
    Ok(signed)
}
