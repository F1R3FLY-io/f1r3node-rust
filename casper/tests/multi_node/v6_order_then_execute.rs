use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use casper::rust::blocks::proposer::propose_result::BlockCreatorResult;
use casper::rust::blocks::proposer::proposer::DeploySelection;
use casper::rust::casper::{Casper, MultiParentCasper};
use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::costacc::genesis_resource_policy::shard_resource_schedule;
use casper::rust::util::rholang::costacc::owner_offer::owner_direct_offer;
use casper::rust::v6::block_record::BlockRecord;
use casper::rust::v6::body::V6Records;
use casper::rust::v6::record::ExecutionRecord;
use casper::rust::v6::settlement::EnvelopeOutcome;
use casper::rust::v6::V6Engine;
use crypto::rust::private_key::PrivateKey;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::BlockMessage;
use models::rust::cost_deploy_data::DeployData;
use models::rust::cost_protocol_limits::offered_funded_v6_limits;
use models::rust::deploy_envelope::DeployEnvelope;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

fn engine(node: &TestNode) -> Arc<V6Engine> { node.casper.v6.clone().expect("v6 engine") }

fn offer(key: &PrivateKey, term: &str, time_stamp: i64, shard_id: &str) -> DeployEnvelope {
    let body = DeployData {
        term: term.to_string(),
        language: "rholang".to_string(),
        time_stamp,
        valid_after_block_number: 0,
        shard_id: shard_id.to_string(),
        expiration_timestamp: None,
        authority_presentations: Vec::new(),
    };
    let proto = owner_direct_offer(
        key,
        body,
        1_000_000,
        1,
        &shard_resource_schedule(shard_id, 8),
    )
    .unwrap();
    DeployEnvelope::from_proto(proto, offered_funded_v6_limits().envelope).unwrap()
}

async fn propose(node: &mut TestNode, deploys: &[DeployEnvelope]) -> BlockMessage {
    for deploy in deploys {
        assert!(matches!(
            node.casper.deploy_envelope(deploy.clone()).unwrap(),
            Either::Right(_)
        ));
    }
    let snapshot = node.casper.get_snapshot().await.unwrap();
    let validator = node.casper.get_validator().unwrap();
    match engine(node)
        .create_block(&snapshot, &validator, DeploySelection::StandardAllowEmpty)
        .await
        .unwrap()
    {
        BlockCreatorResult::Created(block, ..) => block,
        other => panic!("expected a block, got {:?}", other),
    }
}

async fn broadcast(nodes: &mut [TestNode], block: &BlockMessage) {
    for node in nodes.iter_mut() {
        let status = node.process_block(block.clone()).await.unwrap();
        assert!(
            matches!(status, Either::Right(_)),
            "block rejected: {:?}",
            status
        );
    }
}

async fn drive(nodes: &mut [TestNode], blocks: &mut Vec<BlockHash>, target: usize) -> bool {
    for round in 0..240 {
        if nodes.iter().all(|n| engine(n).canonical_len() >= target) {
            return true;
        }
        for node in nodes.iter() {
            engine(node).wake();
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
        let proposer = round % nodes.len();
        let block = propose(&mut nodes[proposer], &[]).await;
        broadcast(nodes, &block).await;
        blocks.push(block.block_hash.clone());
    }
    false
}

fn executed_roots(
    nodes: &[TestNode],
    blocks: &[BlockHash],
) -> Vec<HashMap<BlockHash, prost::bytes::Bytes>> {
    nodes
        .iter()
        .map(|n| {
            blocks
                .iter()
                .filter_map(|b| engine(n).executed(b).map(|r| (b.clone(), r)))
                .collect()
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn validators_agree_on_racing_blocks_without_merging_or_replaying_on_the_consensus_path() {
    crate::init_logger();
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    let mut parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    parameters.2.version = casper::rust::casper::PROTOCOL_V6_VERSION;
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .unwrap();
    let shard_id = genesis.genesis_block.shard_id.clone();
    let mut nodes = TestNode::create_v6_network(genesis.clone(), 3)
        .await
        .unwrap();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let keys = [
        construct_deploy::DEFAULT_SEC.clone(),
        construct_deploy::DEFAULT_SEC2.clone(),
    ];
    let mut offset = 0i64;
    let mut deploy = |source: &str, key: usize| {
        offset += 1;
        offer(&keys[key], source, now + offset, &shard_id)
    };

    let racing = vec![
        deploy(r#"@"race"!(1)"#, 0),
        deploy(r#"@"race"!(2)"#, 1),
        deploy(r#"for (@v <- @"race") { @"won"!(v) }"#, 0),
        deploy(r#"for (@v <- @"race") { @"second"!(v) }"#, 1),
    ];
    let mut blocks = Vec::new();
    let first = propose(&mut nodes[0], &racing).await;
    assert_eq!(first.body.deploys.len(), 4);
    assert!(first.body.deploys.iter().all(|d| d.as_ordered().is_some()));
    assert!(first.body.rejected_deploys.is_empty());
    broadcast(&mut nodes, &first).await;
    blocks.push(first.block_hash.clone());

    for round in 1..24 {
        let proposer = round % nodes.len();
        let extra = if round % 4 == 0 {
            vec![deploy(&format!(r#"@"tick"!({})"#, round), round % 2)]
        } else {
            vec![]
        };
        let block = propose(&mut nodes[proposer], &extra).await;
        broadcast(&mut nodes, &block).await;
        blocks.push(block.block_hash.clone());
    }

    assert!(
        drive(&mut nodes, &mut blocks, 4).await,
        "the executors did not fix canonical records"
    );

    let roots = executed_roots(&nodes, &blocks);
    assert!(
        roots[0].contains_key(&first.block_hash),
        "the racing block has no canonical root"
    );
    for other in &roots[1..] {
        for (block, root) in other {
            if let Some(mine) = roots[0].get(block) {
                assert_eq!(mine, root, "validators disagree on an executed root");
            }
        }
    }
    for node in &nodes {
        assert!(
            engine(node).evidence().is_empty(),
            "honest validators produced evidence: {:?}",
            engine(node).evidence()
        );
    }

    let cited = blocks.iter().any(|hash| {
        let block = nodes[0].block_store.get(hash).unwrap().unwrap();
        block
            .body
            .v6
            .as_ref()
            .map(|b| {
                b.decode_records::<BlockRecord>()
                    .unwrap()
                    .iter()
                    .any(|r| r.target() == &first.block_hash)
            })
            .unwrap_or(false)
    });
    assert!(cited, "no block carried a record of the racing block");

    let record = blocks
        .iter()
        .find_map(|hash| {
            let block = nodes[0].block_store.get(hash).unwrap().unwrap();
            block
                .body
                .v6
                .as_ref()
                .and_then(|b| b.decode_records::<BlockRecord>().ok())
                .and_then(|records| {
                    records
                        .into_iter()
                        .find(|r| r.target() == &first.block_hash)
                })
        })
        .expect("the racing block's record");
    let outcomes: Vec<EnvelopeOutcome> = record
        .entries()
        .iter()
        .map(|entry| entry.executed.outcome)
        .collect();
    assert_eq!(outcomes, vec![
        EnvelopeOutcome::Completed,
        EnvelopeOutcome::Conflicted,
        EnvelopeOutcome::Conflicted,
        EnvelopeOutcome::Conflicted,
    ]);
    assert!(record
        .entries()
        .iter()
        .all(|entry| entry.settled.rev_spent > 0));
}
