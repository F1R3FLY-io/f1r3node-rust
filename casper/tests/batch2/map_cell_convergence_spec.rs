use casper::rust::blocks::proposer::block_creator;
use casper::rust::blocks::proposer::propose_result::BlockCreatorResult;
use casper::rust::casper::{Casper, MultiParentCasper};
use casper::rust::util::construct_deploy;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::signed::Signed;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::Par;
use models::rust::casper::protocol::casper_message::DeployData;
use serial_test::serial;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::{GenesisBuilder, GenesisContext, EXTRA_GENESIS_VAULT_KEY_PAIRS};

struct TestContext {
    genesis: GenesisContext,
}

impl TestContext {
    async fn new(validators: usize) -> Self {
        let parameters =
            GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(validators));
        let genesis = GenesisBuilder::new()
            .build_genesis_with_parameters(Some(parameters))
            .await
            .expect("genesis");
        Self { genesis }
    }
}

fn signer(index: usize) -> PrivateKey {
    match index {
        0 => construct_deploy::DEFAULT_SEC.clone(),
        1 => construct_deploy::DEFAULT_SEC2.clone(),
        2 => EXTRA_GENESIS_VAULT_KEY_PAIRS[0].0.clone(),
        _ => panic!("unsupported validator count"),
    }
}

fn map_write(key: &str, value: i64, key_pair: &PrivateKey, shard: &str) -> Signed<DeployData> {
    construct_deploy::source_deploy_now_full(
        format!(r#"for (@m <- @"m") {{ @"m"!(m.set("{key}", {value})) }}"#),
        None,
        None,
        Some(key_pair.clone()),
        None,
        Some(shard.to_string()),
    )
    .expect("map deploy")
}

fn par_to_i64(par: &Par) -> Option<i64> {
    par.exprs
        .first()
        .and_then(|expr| match &expr.expr_instance {
            Some(ExprInstance::GInt(value)) => Some(*value),
            _ => None,
        })
}

async fn present_keys(
    node: &TestNode,
    state_hash: &prost::bytes::Bytes,
    writes: &[(String, i64)],
) -> Vec<String> {
    let mut keys = Vec::new();
    for (key, _) in writes {
        let term = format!(
            r#"new return in {{ for (@m <<- @"m") {{ return!(m.getOrElse("{key}", -999)) }} }}"#
        );
        if let Ok((result, _)) = node
            .runtime_manager
            .play_exploratory_deploy(term, state_hash)
            .await
        {
            if result.first().and_then(par_to_i64) != Some(-999) {
                keys.push(key.clone());
            }
        }
    }
    keys.sort();
    keys
}

async fn finalized_keys(nodes: &[TestNode], writes: &[(String, i64)]) -> (i64, Vec<String>) {
    let reference = nodes[0]
        .casper
        .last_finalized_block()
        .await
        .expect("reference lfb");
    let expected = present_keys(&nodes[0], &reference.body.state.post_state_hash, writes).await;
    for (index, node) in nodes.iter().enumerate().skip(1) {
        let lfb = node.casper.last_finalized_block().await.expect("peer lfb");
        let actual = present_keys(node, &lfb.body.state.post_state_hash, writes).await;
        assert_eq!(
            lfb.block_hash, reference.block_hash,
            "node {index} finalized a different block"
        );
        assert_eq!(actual, expected, "node {index} finalized different state");
    }
    (reference.body.state.block_number, expected)
}

async fn run_convergence(validators: usize, write_rounds: usize, drain_rounds: usize) {
    let context = TestContext::new(validators).await;
    let shard = context.genesis.genesis_block.shard_id.clone();
    let mut nodes =
        TestNode::create_network(context.genesis.clone(), validators, None, None, None, None)
            .await
            .expect("network");
    let signers: Vec<_> = (0..validators).map(signer).collect();
    let init = construct_deploy::source_deploy_now_full(
        r#"@"m"!({})"#.to_string(),
        None,
        None,
        Some(construct_deploy::DEFAULT_SEC.clone()),
        None,
        Some(shard.clone()),
    )
    .expect("init deploy");
    nodes[0].casper.deploy(init).expect("queue init");
    let init_block = nodes[0].create_block_unsafe(&[]).await.expect("init block");
    for node in &mut nodes {
        node.process_block(init_block.clone())
            .await
            .expect("process init");
    }

    let mut writes = Vec::new();
    let mut previous_finalized = Vec::<String>::new();
    for round in 0..write_rounds {
        let mut siblings = Vec::new();
        for validator in 0..validators {
            let key = format!("v{}_{}", validator + 1, round);
            let value = (round * validators + validator + 1) as i64;
            tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
            nodes[validator]
                .casper
                .deploy(map_write(&key, value, &signers[validator], &shard))
                .expect("queue write");
            writes.push((key, value));
            siblings.push(
                nodes[validator]
                    .create_block_unsafe(&[])
                    .await
                    .expect("sibling"),
            );
        }
        for block in siblings {
            for node in &mut nodes {
                node.process_block(block.clone()).await.ok();
            }
        }
        let marker = construct_deploy::basic_deploy_data(round as i32, None, Some(shard.clone()))
            .expect("merge marker");
        nodes[0].casper.deploy(marker).expect("queue marker");
        let merged = nodes[0]
            .create_block_unsafe(&[])
            .await
            .expect("merge block");
        for node in &mut nodes {
            node.process_block(merged.clone()).await.ok();
        }
        let (_, current) = finalized_keys(&nodes, &writes).await;
        assert!(
            previous_finalized.iter().all(|key| current.contains(key)),
            "finalized state regressed from {previous_finalized:?} to {current:?}"
        );
        previous_finalized = current;
    }

    for round in 0..drain_rounds {
        let proposer = round % validators;
        let marker =
            construct_deploy::basic_deploy_data((1000 + round) as i32, None, Some(shard.clone()))
                .expect("drain marker");
        nodes[proposer].casper.deploy(marker).expect("queue drain");
        if let Ok(block) = nodes[proposer].create_block_unsafe(&[]).await {
            for node in &mut nodes {
                node.process_block(block.clone()).await.ok();
            }
            let (_, current) = finalized_keys(&nodes, &writes).await;
            assert!(
                previous_finalized.iter().all(|key| current.contains(key)),
                "finalized state regressed from {previous_finalized:?} to {current:?}"
            );
            previous_finalized = current;
        }
    }

    let marker =
        construct_deploy::basic_deploy_data(9999, None, Some(shard)).expect("final marker");
    nodes[0].casper.deploy(marker).expect("queue final");
    let final_block = nodes[0]
        .create_block_unsafe(&[])
        .await
        .expect("final block");
    let final_keys =
        present_keys(&nodes[0], &final_block.body.state.post_state_hash, &writes).await;
    let missing: Vec<_> = writes
        .iter()
        .filter(|(key, _)| !final_keys.contains(key))
        .collect();
    assert!(missing.is_empty(), "unrecovered writes: {missing:?}");
}

async fn create_allow_empty(node: &mut TestNode) -> BlockCreatorResult {
    let snapshot = node.casper.get_snapshot().await.expect("snapshot");
    let validator = node.casper.get_validator().expect("validator");
    block_creator::create(
        &snapshot,
        &validator,
        None,
        node.deploy_storage.clone(),
        node.rejected_deploy_buffer.clone(),
        &node.runtime_manager,
        &mut node.block_store,
        true,
    )
    .await
    .expect("block creation")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn unresolved_user_frontier_has_one_deploy_inclusion_leader() {
    let context = TestContext::new(2).await;
    let shard = context.genesis.genesis_block.shard_id.clone();
    let mut nodes = TestNode::create_network(context.genesis, 2, None, None, None, None)
        .await
        .expect("network");
    let first = map_write("leader-a", 1, &signer(0), &shard);
    let second = map_write("leader-b", 2, &signer(1), &shard);
    let block_a = nodes[0]
        .add_block_from_deploys(&[first])
        .await
        .expect("first sibling");
    let block_b = nodes[1]
        .add_block_from_deploys(&[second])
        .await
        .expect("second sibling");
    for node in &mut nodes {
        node.process_block(block_a.clone()).await.ok();
        node.process_block(block_b.clone()).await.ok();
    }
    nodes[0]
        .casper
        .deploy(map_write("fresh-a", 3, &signer(0), &shard))
        .expect("fresh a");
    nodes[1]
        .casper
        .deploy(map_write("fresh-b", 4, &signer(1), &shard))
        .expect("fresh b");
    let proposal_a = create_allow_empty(&mut nodes[0]).await;
    let proposal_b = create_allow_empty(&mut nodes[1]).await;
    let packaged = [proposal_a, proposal_b]
        .iter()
        .filter(|result| match result {
            BlockCreatorResult::Created(block, ..) => !block.body.deploys.is_empty(),
            _ => false,
        })
        .count();

    assert_eq!(
        packaged, 1,
        "exactly one validator may package user deploys"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn two_writers_converge() { run_convergence(2, 1, 7).await; }

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn three_writers_converge() { run_convergence(3, 1, 21).await; }

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn three_writers_converge_under_load() { run_convergence(3, 3, 21).await; }
