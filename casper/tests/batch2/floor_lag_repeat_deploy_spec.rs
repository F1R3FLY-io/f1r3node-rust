// A block that re-executes a deploy whose effect is already on its own
// ancestry, judged by a peer through the full received-block path
// (`block_summary`, checkpoint recompute + replay, bonds, equivocation).
//
// Expiry is judged on the block's FINALIZED FLOOR, not its tip. While the
// floor trails the tip by two or more heights, a deploy applied at height h
// with
//   floor - lifespan < valid_after < h <= tip - lifespan
// is not yet expired and its carrier lies below the tip's `deploy_lifespan`
// window, so the repeat-deploy scan must reach below that window.
//
// Topology: three validators with the default stakes {1, 3, 5}. nodes[0]
// stays silent; nodes[1] and nodes[2] (8/9 of the stake) progress. The floor
// advances only while both of them propose and see each other.

use std::collections::{BTreeMap, HashMap};

use casper::rust::block_status::{BlockError, InvalidBlock};
use casper::rust::casper::Casper;
use casper::rust::finality::floor::floor_committee;
use casper::rust::finality::floor_context::FloorContext;
use casper::rust::safety::clique_oracle::FtThreshold;
use casper::rust::util::rholang::costacc::close_block_deploy::CloseBlockDeploy;
use casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum;
use casper::rust::util::rholang::{interpreter_util, system_deploy_util};
use casper::rust::util::{construct_deploy, proto_util};
use casper::rust::ValidBlockProcessing;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::signed::Signed;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Body, DeployData, F1r3flyState, Header, ProcessedDeploy,
};
use models::rust::validator::Validator;
use prost::bytes::Bytes;
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::history::Either;
use serial_test::serial;

use crate::helper::test_node::{ShardOverrides, TestNode};
use crate::util::genesis_builder::GenesisBuilder;

const DEPLOY_LIFESPAN: i64 = 3;
const REPLAYED_CHANNEL: &str = "applied_once";
const PROPOSER: usize = 1;
const RECEIVER: usize = 2;

fn proposer_key(node: usize) -> PrivateKey {
    match node {
        1 => construct_deploy::DEFAULT_SEC2.clone(),
        2 => crate::util::genesis_builder::EXTRA_GENESIS_VAULT_KEY_PAIRS[0]
            .0
            .clone(),
        other => panic!("node {other} does not propose in this spec"),
    }
}

async fn three_validator_network() -> (Vec<TestNode>, String) {
    let n_validators = 3usize;
    let genesis_parameters =
        GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(n_validators));
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(genesis_parameters))
        .await
        .expect("genesis");
    let shard_id = genesis.genesis_block.shard_id.clone();
    let nodes = TestNode::create_network_with_overrides(genesis, n_validators, ShardOverrides {
        deploy_lifespan: Some(DEPLOY_LIFESPAN),
        ..Default::default()
    })
    .await
    .expect("create_network");
    (nodes, shard_id)
}

async fn tip_number(node: &TestNode) -> i64 {
    node.casper
        .get_snapshot()
        .await
        .expect("snapshot")
        .max_block_num
}

async fn sync(nodes: &mut [TestNode], receiver: usize, sender: usize) {
    let [receiver_node, sender_node] = nodes
        .get_disjoint_mut([receiver, sender])
        .expect("receiver and sender are different nodes");
    receiver_node
        .sync_with_one(sender_node)
        .await
        .expect("sync");
}

/// An honest proposal by `proposer` carrying a fresh marker that stays inside
/// its validity window for the whole spec.
async fn propose_marker(nodes: &mut [TestNode], proposer: usize, shard_id: &str, nonce: i32) {
    let valid_after = tip_number(&nodes[proposer]).await;
    tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
    let marker = construct_deploy::source_deploy_now_full(
        format!("@{nonce}!({nonce})"),
        None,
        None,
        Some(proposer_key(proposer)),
        Some(valid_after),
        Some(shard_id.to_string()),
    )
    .expect("marker");
    let block = nodes[proposer]
        .add_block_from_deploys(std::slice::from_ref(&marker))
        .await
        .expect("honest proposal");
    assert!(
        block
            .body
            .deploys
            .iter()
            .any(|pd| pd.deploy.sig == marker.sig),
        "the honest proposal at height {} must carry its marker",
        block.body.state.block_number
    );
}

/// Both progressing validators propose in turn and see each other's blocks.
async fn alternate(nodes: &mut [TestNode], shard_id: &str, rounds: i32, nonce_base: i32) {
    for round in 0..rounds {
        let proposer = 1 + (round % 2) as usize;
        let other = 3 - proposer;
        propose_marker(nodes, proposer, shard_id, nonce_base + round).await;
        sync(nodes, other, proposer).await;
    }
}

/// Applies `deploy` in an honest block by `PROPOSER` and delivers it to
/// `RECEIVER`. Returns the carrier.
async fn carry(nodes: &mut [TestNode], deploy: &Signed<DeployData>) -> BlockMessage {
    let carrier = nodes[PROPOSER]
        .add_block_from_deploys(std::slice::from_ref(deploy))
        .await
        .expect("carrier proposal");
    assert!(
        carrier
            .body
            .deploys
            .iter()
            .any(|pd| pd.deploy.sig == deploy.sig && !pd.is_failed),
        "the carrier must execute the deploy cleanly"
    );
    sync(nodes, RECEIVER, PROPOSER).await;
    carrier
}

struct DishonestBlock {
    block: BlockMessage,
    play_error: String,
}

/// A block by `node`'s validator on its current snapshot that carries
/// `carried` (the processed form of `deploy` taken from its first carrier)
/// on top of the real merged pre-state, rejected records, state facts, close
/// block and floor committee. The node's own executor refuses to play
/// `deploy` again on that pre-state, so no honest post-state exists; the
/// block records the post-state of the same block without the deploy.
async fn block_carrying_again(
    node: &mut TestNode,
    deploy: &Signed<DeployData>,
    carried: &ProcessedDeploy,
) -> DishonestBlock {
    let snapshot = node.casper.get_snapshot().await.expect("snapshot");
    let validator_identity = node.validator_id_opt.clone().expect("validator identity");
    let runtime_manager = node.runtime_manager.clone();
    let parents = snapshot.parents.clone();
    let parent_hashes: Vec<BlockHash> = parents.iter().map(|p| p.block_hash.clone()).collect();
    let latest_messages: BTreeMap<Validator, BlockHash> = snapshot
        .justifications
        .iter()
        .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
        .collect();
    let floor_ctx = FloorContext::derive(
        &snapshot.dag,
        &node.block_store,
        &parent_hashes,
        &latest_messages,
        FtThreshold::from_ppm(
            snapshot
                .on_chain_state
                .shard_conf
                .fault_tolerance_threshold_ppm,
        ),
    )
    .await
    .expect("floor context");

    let block_number = snapshot.max_block_num + 1;
    let seq_num = snapshot
        .max_seq_nums
        .get(&validator_identity.public_key.bytes)
        .map(|seq| *seq as i32 + 1)
        .unwrap_or(1);
    let now_millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_millis() as i64;
    let timestamp = parents
        .iter()
        .map(|p| p.header.timestamp)
        .max()
        .map_or(now_millis, |parent_ts| parent_ts.max(now_millis));
    let block_data = BlockData {
        time_stamp: timestamp,
        block_number,
        sender: validator_identity.public_key.clone(),
        seq_num,
    };
    let system_deploys = vec![SystemDeployEnum::Close(CloseBlockDeploy {
        initial_rand: system_deploy_util::generate_close_deploy_random_seed_from_pk(
            validator_identity.public_key.clone(),
            seq_num,
        ),
    })];
    let play_error = match interpreter_util::compute_deploys_checkpoint(
        &mut node.block_store,
        parents.clone(),
        vec![deploy.clone()],
        system_deploys.clone(),
        &snapshot,
        &runtime_manager,
        block_data.clone(),
        HashMap::new(),
        None,
        Some(&floor_ctx),
        None,
    )
    .await
    {
        Ok(_) => panic!("the executor played a deploy whose effect is already in the pre-state"),
        Err(error) => error.to_string(),
    };
    let checkpoint = interpreter_util::compute_deploys_checkpoint(
        &mut node.block_store,
        parents,
        Vec::new(),
        system_deploys,
        &snapshot,
        &runtime_manager,
        block_data,
        HashMap::new(),
        None,
        Some(&floor_ctx),
        None,
    )
    .await
    .expect("deploy-less checkpoint");
    let bonds = floor_committee(&runtime_manager, &floor_ctx.floor_state)
        .await
        .expect("floor committee");

    let body = Body {
        state: F1r3flyState {
            pre_state_hash: checkpoint.pre_state_hash,
            post_state_hash: checkpoint.post_state_hash,
            bonds,
            block_number,
        },
        deploys: vec![carried.clone()],
        rejected_deploys: checkpoint.rejected_deploys,
        system_deploys: checkpoint.system_deploys,
        extra_bytes: Bytes::new(),
        applied_from_scope: checkpoint.applied_from_scope,
        merge_base: checkpoint.merge_base.unwrap_or_default(),
        v6: None,
    };
    let header = Header {
        parents_hash_list: parent_hashes,
        timestamp,
        version: snapshot.on_chain_state.shard_conf.casper_version,
        extra_bytes: Bytes::new(),
    };
    let unsigned = proto_util::unsigned_block_proto(
        body,
        header,
        snapshot.justifications.iter().cloned().collect(),
        snapshot.on_chain_state.shard_conf.shard_name.clone(),
        Some(seq_num),
    );
    DishonestBlock {
        block: validator_identity.sign_block(&unsigned),
        play_error,
    }
}

/// The floor `node` derives for `block` from the block's own justifications,
/// the same derivation `block_summary` performs.
async fn derived_floor(node: &TestNode, block: &BlockMessage) -> i64 {
    let dag = node
        .block_dag_storage
        .get_representation()
        .expect("dag representation");
    let ftt_ppm = node
        .casper
        .get_snapshot()
        .await
        .expect("snapshot")
        .on_chain_state
        .shard_conf
        .fault_tolerance_threshold_ppm;
    let latest_messages: BTreeMap<Validator, BlockHash> = block
        .justifications
        .iter()
        .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
        .collect();
    FloorContext::derive(
        &dag,
        &node.block_store,
        &block.header.parents_hash_list,
        &latest_messages,
        FtThreshold::from_ppm(ftt_ppm),
    )
    .await
    .expect("floor derivation")
    .floor
    .block_number
}

fn on_main_chain(node: &TestNode, block: &BlockMessage, ancestor: &BlockMessage) -> bool {
    let mut current = block.clone();
    while current.body.state.block_number > ancestor.body.state.block_number {
        let Some(main_parent) = current.header.parents_hash_list.first() else {
            return false;
        };
        current = node
            .block_store
            .get(main_parent)
            .expect("block store read")
            .expect("main parent is held");
    }
    current.block_hash == ancestor.block_hash
}

struct ReExecution {
    nodes: Vec<TestNode>,
    carrier: BlockMessage,
    valid_after: i64,
    re_execution: BlockMessage,
    play_error: String,
    floor: i64,
    tip: i64,
}

/// Progress both validators, apply the deploy in a carrier, extend the chain
/// so the carrier falls below the repeat window, then have `PROPOSER` build a
/// block that carries the same deploy again. `extend_with_both` decides
/// whether the floor keeps up during the extension.
async fn stage_re_execution(extend_with_both: bool) -> ReExecution {
    let (mut nodes, shard_id) = three_validator_network().await;
    alternate(&mut nodes, &shard_id, 4, 0).await;

    let valid_after = tip_number(&nodes[PROPOSER]).await;
    let deploy = construct_deploy::source_deploy_now_full(
        format!(r#"@"{REPLAYED_CHANNEL}"!(1)"#),
        None,
        None,
        Some(construct_deploy::DEFAULT_SEC.clone()),
        Some(valid_after),
        Some(shard_id.clone()),
    )
    .expect("deploy");
    let carrier = carry(&mut nodes, &deploy).await;
    let carrier_height = carrier.body.state.block_number;

    if extend_with_both {
        alternate(&mut nodes, &shard_id, 8, 100).await;
    } else {
        for nonce in 0..DEPLOY_LIFESPAN as i32 {
            propose_marker(&mut nodes, PROPOSER, &shard_id, 200 + nonce).await;
        }
    }
    sync(&mut nodes, RECEIVER, PROPOSER).await;

    let carried = carrier
        .body
        .deploys
        .iter()
        .find(|pd| pd.deploy.sig == deploy.sig)
        .expect("the carrier holds the deploy")
        .clone();
    let DishonestBlock {
        block: re_execution,
        play_error,
    } = block_carrying_again(&mut nodes[PROPOSER], &deploy, &carried).await;
    let tip = re_execution.body.state.block_number - 1;
    let floor = derived_floor(&nodes[RECEIVER], &re_execution).await;
    tracing::info!(
        target: "f1r3fly.test.floor_lag_repeat_deploy",
        extend_with_both,
        valid_after,
        carrier_height,
        tip,
        floor,
        lifespan = DEPLOY_LIFESPAN,
        play_error = %play_error,
        "re-execution staged"
    );

    assert!(
        on_main_chain(&nodes[RECEIVER], &re_execution, &carrier),
        "the carrier (height {carrier_height}) must be on the re-execution block's main-parent chain"
    );
    assert!(
        carrier_height <= tip - DEPLOY_LIFESPAN,
        "the carrier (height {carrier_height}) must lie below the repeat-deploy window of a \
         block on tip {tip} (lifespan {DEPLOY_LIFESPAN})"
    );

    ReExecution {
        nodes,
        carrier,
        valid_after,
        re_execution,
        play_error,
        floor,
        tip,
    }
}

/// With the floor two or more heights behind the tip, the deploy is not
/// expired and its carrier is below the tip window. The block is rejected as
/// a repeat before replay.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn a_block_re_executing_a_deploy_already_applied_on_its_ancestry_is_rejected_as_a_repeat_while_the_floor_lags(
) {
    let mut staged = stage_re_execution(false).await;
    assert!(
        staged.floor - DEPLOY_LIFESPAN < staged.valid_after,
        "geometry: the deploy (valid after {}) must not be expired at floor {} (lifespan {})",
        staged.valid_after,
        staged.floor,
        DEPLOY_LIFESPAN
    );
    assert!(
        staged.tip - staged.floor >= 2,
        "geometry: the floor ({}) must lag the tip ({}) by at least two heights",
        staged.floor,
        staged.tip
    );

    let status: ValidBlockProcessing = staged.nodes[RECEIVER]
        .process_block(staged.re_execution.clone())
        .await
        .expect("receiver processes the re-execution");
    assert_eq!(
        status,
        Either::Left(BlockError::Invalid(InvalidBlock::InvalidRepeatDeploy)),
        "a block re-executing a deploy already applied at height {} (valid after {}, floor {}, \
         tip {}, lifespan {}; the proposer's executor refused to play it: {})",
        staged.carrier.body.state.block_number,
        staged.valid_after,
        staged.floor,
        staged.tip,
        DEPLOY_LIFESPAN,
        staged.play_error
    );
}

/// Control for the test above: the same re-execution once the floor has
/// moved past the deploy's validity window is rejected as expired.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn a_block_re_executing_a_deploy_already_applied_on_its_ancestry_is_rejected_as_expired_when_the_floor_keeps_up(
) {
    let mut staged = stage_re_execution(true).await;
    assert!(
        staged.valid_after <= staged.floor - DEPLOY_LIFESPAN,
        "geometry: the deploy (valid after {}) must be expired at floor {} (lifespan {})",
        staged.valid_after,
        staged.floor,
        DEPLOY_LIFESPAN
    );

    let status = staged.nodes[RECEIVER]
        .process_block(staged.re_execution.clone())
        .await
        .expect("receiver processes the re-execution");
    assert_eq!(
        status,
        Either::Left(BlockError::Invalid(InvalidBlock::ContainsExpiredDeploy)),
        "floor {}, tip {}, valid after {}, lifespan {}",
        staged.floor,
        staged.tip,
        staged.valid_after,
        DEPLOY_LIFESPAN
    );
}
