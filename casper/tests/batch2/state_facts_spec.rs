// The recorded state-construction facts are consensus content. A block's
// body carries `applied_from_scope` (the user sigs whose chains its merge
// re-applied onto the base) and `merge_base` (the block whose committed
// state its pre-state derives from; empty where the header derives it) —
// state(B) = state(mergeBase) + appliedFromScope + deploys. The verdict
// plane walks these recorded pointers instead of re-deriving lineage, so
// a block recording facts its validator-recomputed merge did not produce
// must be invalid, the same contract as the rejected records.

use std::collections::HashMap;

use casper::rust::block_status::{BlockError, InvalidBlock};
use casper::rust::casper::{Casper, MultiParentCasper};
use casper::rust::finality::floor::floor_of_block;
use casper::rust::safety::clique_oracle::FtThreshold;
use casper::rust::util::rholang::interpreter_util;
use casper::rust::util::{construct_deploy, proto_util};
use models::rust::casper::protocol::casper_message::{Body, F1r3flyState, Header, Justification};
use prost::bytes::Bytes;
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::history::Either;
use serial_test::serial;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

async fn two_validator_network() -> (Vec<TestNode>, String, Bytes) {
    let n_validators = 2usize;
    let genesis_parameters =
        GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(n_validators));
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(genesis_parameters))
        .await
        .unwrap();
    let shard_id = genesis.genesis_block.shard_id.clone();
    let genesis_hash = genesis.genesis_block.block_hash.clone();

    let mut nodes = TestNode::create_network(genesis, n_validators, None, None, None, None)
        .await
        .expect("create_network");
    for node in nodes.iter_mut() {
        node.allow_empty_blocks = true;
    }
    (nodes, shard_id, genesis_hash)
}

/// Assemble the block the production checkpoint computes for the given
/// deploys on the node's current snapshot, with a caller-supplied tamper
/// applied to the body before signing.
async fn checkpoint_block_with(
    node: &mut TestNode,
    shard_id: &str,
    deploys: Vec<
        crypto::rust::signatures::signed::Signed<
            models::rust::casper::protocol::casper_message::DeployData,
        >,
    >,
    tamper: impl FnOnce(&mut Body),
) -> models::rust::casper::protocol::casper_message::BlockMessage {
    let snapshot = node.casper.get_snapshot().await.expect("snapshot");
    let validator_identity = node.validator_id_opt.clone().expect("validator identity");
    let runtime_manager = node.runtime_manager.clone();
    let next_block_num = snapshot.max_block_num + 1;
    let next_seq_num = snapshot
        .max_seq_nums
        .get(&validator_identity.public_key.bytes)
        .map(|seq| *seq as i32 + 1)
        .unwrap_or(1);
    let now_millis = snapshot
        .parents
        .iter()
        .map(|p| p.header.timestamp)
        .max()
        .expect("parents have timestamps")
        + 1;
    let block_data = BlockData {
        time_stamp: now_millis,
        block_number: next_block_num,
        sender: validator_identity.public_key.clone(),
        seq_num: next_seq_num,
    };
    let checkpoint = interpreter_util::compute_deploys_checkpoint(
        &mut node.block_store,
        snapshot.parents.clone(),
        deploys,
        Vec::new(),
        &snapshot,
        &runtime_manager,
        block_data,
        HashMap::new(),
        None,
        None,
        None,
    )
    .await
    .expect("production checkpoint");
    let mut bond_generations = snapshot
        .on_chain_state
        .bond_generations
        .iter()
        .map(|(validator, generation)| {
            models::rust::casper::protocol::casper_message::ValidatorBondGeneration {
                validator: validator.clone(),
                generation: *generation,
            }
        })
        .collect::<Vec<_>>();
    bond_generations.sort_unstable();
    let mut active_validators = snapshot.on_chain_state.active_validators.clone();
    active_validators.sort_unstable();
    let finalized_floor_certificate = snapshot.finalized_floor_certificate.clone();
    let finalized_floor = finalized_floor_certificate
        .as_ref()
        .map(|certificate| certificate.commitment(snapshot.consensus_context.digest().clone()));
    let mut body = Body {
        state: F1r3flyState {
            pre_state_hash: checkpoint.pre_state_hash,
            post_state_hash: checkpoint.post_state_hash,
            bonds: checkpoint.bonds,
            bond_generations,
            active_validators,
            block_number: next_block_num,
        },
        deploys: checkpoint.deploys,
        rejected_deploys: checkpoint.rejected_deploys,
        rejected_state_effects: checkpoint.rejected_state_effects,
        system_deploys: checkpoint.system_deploys,
        extra_bytes: Bytes::new(),
        applied_from_scope: checkpoint.applied_from_scope,
        merge_base: checkpoint.merge_base.unwrap_or_default(),
    };
    tamper(&mut body);
    let header = Header {
        parents_hash_list: snapshot
            .parents
            .iter()
            .map(|p| p.block_hash.clone())
            .collect(),
        timestamp: now_millis,
        version: snapshot.on_chain_state.shard_conf.casper_version,
        extra_bytes: Bytes::new(),
        sender_bond_generation: snapshot
            .on_chain_state
            .bond_generations
            .get(&validator_identity.public_key.bytes)
            .copied(),
        objective_equivocation_evidence_delta: Vec::new(),
        finalized_floor,
    };
    let justifications: Vec<Justification> = snapshot.justifications.to_vec();
    let mut unsigned = proto_util::unsigned_block_proto(
        body,
        header,
        justifications,
        shard_id.to_string(),
        Some(next_seq_num),
    );
    unsigned.finalized_floor_certificate = finalized_floor_certificate;
    validator_identity.sign_block(&unsigned)
}

/// The packaging pin: a merged block records the floor as its base and the
/// sibling chains it re-applied as its applied set; single-parent blocks
/// record neither (the header derives their state parent). The recorded
/// facts validate everywhere.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn merged_block_records_the_floor_base_and_the_applied_set() {
    let (mut nodes, shard_id, _genesis_hash) = two_validator_network().await;

    let deploy_a = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::source_deploy_now_full(
            r#"@"sf_a"!(1)"#.to_string(),
            None,
            None,
            Some(construct_deploy::DEFAULT_SEC.clone()),
            None,
            Some(shard_id.clone()),
        )
        .expect("build deploy_a")
    };
    let deploy_b = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::source_deploy_now_full(
            r#"@"sf_b"!(2)"#.to_string(),
            None,
            None,
            Some(construct_deploy::DEFAULT_SEC2.clone()),
            None,
            Some(shard_id.clone()),
        )
        .expect("build deploy_b")
    };

    let block_a = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&deploy_a))
        .await
        .expect("validator 1 proposes A");
    let block_b = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&deploy_b))
        .await
        .expect("validator 2 proposes B");
    assert!(
        block_a.body.merge_base.is_empty() && block_a.body.applied_from_scope.is_empty(),
        "a single-parent block records no base and no applied set — the \
         header derives its state parent"
    );

    nodes[0]
        .process_block(block_b.clone())
        .await
        .expect("validator 1 processes B");
    nodes[1]
        .process_block(block_a.clone())
        .await
        .expect("validator 2 processes A");

    let marker = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::basic_deploy_data(
            0,
            Some(construct_deploy::DEFAULT_SEC.clone()),
            Some(shard_id.clone()),
        )
        .expect("build marker")
    };
    let merge_block = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&marker))
        .await
        .expect("validator 1 proposes the merge");
    assert_eq!(
        merge_block.header.parents_hash_list.len(),
        2,
        "staging precondition: the proposal must merge both siblings"
    );
    // Cross-check against the finality module's independent derivation:
    // the recorded base is the merge operation's floor, derived from the
    // block's own frozen justifications.
    let expected_floor = {
        let dag = nodes[0].casper.block_dag().await.expect("dag");
        floor_of_block(
            &dag,
            &nodes[0].block_store,
            &merge_block.block_hash,
            FtThreshold::from_f32_lossy(0.0),
        )
        .await
        .expect("floor_of_block")
    };
    assert_eq!(
        merge_block.body.merge_base, expected_floor.hash,
        "a merged block records its floor as the base its pre-state \
         derives from"
    );
    assert!(
        !merge_block.body.merge_base.is_empty(),
        "the merged path always records its base"
    );
    // The applied set is exactly the ABOVE-floor chains: a sibling the
    // floor covers is base content (its effect is already in the base
    // state), a sibling above the floor is re-applied and recorded.
    let dag = nodes[0].casper.block_dag().await.expect("dag");
    for (block, sig, name) in [
        (&block_a, &deploy_a.sig, "A"),
        (&block_b, &deploy_b.sig, "B"),
    ] {
        let in_base = block.block_hash == expected_floor.hash
            || dag
                .is_dag_ancestor(&block.block_hash, &expected_floor.hash)
                .expect("ancestor query");
        assert_eq!(
            merge_block.body.applied_from_scope.contains(sig),
            !in_base,
            "sibling {}: applied_from_scope records exactly the chains the \
             merge re-applied from ABOVE-floor scope (in_base={}, applied \
             set: {:?})",
            name,
            in_base,
            merge_block
                .body
                .applied_from_scope
                .iter()
                .map(|s| hex::encode(&s[..8.min(s.len())]))
                .collect::<Vec<_>>()
        );
    }
    assert!(
        !merge_block.body.applied_from_scope.is_empty(),
        "the floor is a single chain point: it cannot cover both \
         incomparable siblings, so at least one chain is re-applied"
    );
    assert!(
        !merge_block.body.applied_from_scope.contains(&marker.sig),
        "the marker executed FRESH in this block (a deploys entry), not \
         re-applied from scope"
    );

    let outcome = nodes[1]
        .process_block(merge_block.clone())
        .await
        .expect("validator 2 processes the merge");
    assert!(
        matches!(outcome, Either::Right(_)),
        "the recorded facts are validator-recomputable: the merge must \
         validate, got {:?}",
        outcome
    );
}

/// The consensus check, applied-set side: recording a sig the
/// validator-recomputed merge did not apply invalidates the block.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn a_block_lying_about_its_applied_set_is_invalid() {
    let (mut nodes, shard_id, _genesis_hash) = two_validator_network().await;

    let deploy = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::source_deploy_now_full(
            r#"@"sf_lie_a"!(1)"#.to_string(),
            None,
            None,
            Some(construct_deploy::DEFAULT_SEC.clone()),
            None,
            Some(shard_id.clone()),
        )
        .expect("build deploy")
    };
    let lying_block = checkpoint_block_with(&mut nodes[0], &shard_id, vec![deploy], |body| {
        body.applied_from_scope = vec![Bytes::from(vec![0xAB; 70])];
    })
    .await;

    let verdict = nodes[1]
        .process_block(lying_block)
        .await
        .expect("validator 2 processes the lying block");
    assert_eq!(
        verdict,
        Either::Left(BlockError::Invalid(InvalidBlock::InvalidRejectedDeploy)),
        "an applied set the validator's own merge did not produce must \
         invalidate the block",
    );
}

/// The consensus check, base side: recording a base the
/// validator-recomputed merge did not derive invalidates the block.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn a_block_lying_about_its_merge_base_is_invalid() {
    let (mut nodes, shard_id, _genesis_hash) = two_validator_network().await;

    let deploy = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::source_deploy_now_full(
            r#"@"sf_lie_b"!(1)"#.to_string(),
            None,
            None,
            Some(construct_deploy::DEFAULT_SEC.clone()),
            None,
            Some(shard_id.clone()),
        )
        .expect("build deploy")
    };
    let lying_block = checkpoint_block_with(&mut nodes[0], &shard_id, vec![deploy], |body| {
        body.merge_base = Bytes::from(vec![0xCD; 32]);
    })
    .await;

    let verdict = nodes[1]
        .process_block(lying_block)
        .await
        .expect("validator 2 processes the lying block");
    assert_eq!(
        verdict,
        Either::Left(BlockError::Invalid(InvalidBlock::InvalidRejectedDeploy)),
        "a merge base the validator's own merge did not derive must \
         invalidate the block",
    );
}
