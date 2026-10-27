use std::sync::Arc;
use std::time::Duration;

use casper::rust::blocks::proposer::propose_result::BlockCreatorResult;
use casper::rust::blocks::proposer::proposer::DeploySelection;
use casper::rust::casper::{Casper, MultiParentCasper};
use casper::rust::util::rholang::costacc::genesis_resource_policy::shard_resource_schedule;
use casper::rust::util::rholang::costacc::owner_offer::owner_direct_offer;
use casper::rust::v6::evidence::{Evidence, Offence};
use casper::rust::v6::V6Engine;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::rust::casper::protocol::casper_message::{BlockMessage, ProcessedUserDeploy};
use models::rust::cost_deploy_data::DeployData;
use models::rust::cost_protocol_limits::offered_funded_v6_limits;
use models::rust::deploy_envelope::DeployEnvelope;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

fn engine(node: &TestNode) -> Arc<V6Engine> { node.casper.v6.clone().expect("v6 engine") }

fn unfunded_offer(shard_id: &str) -> DeployEnvelope {
    let (unfunded, _) = crypto::rust::signatures::secp256k1::Secp256k1.new_key_pair();
    let time_stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let body = DeployData {
        term: r#"@"unfunded"!(1)"#.to_string(),
        language: "rholang".to_string(),
        time_stamp,
        valid_after_block_number: 0,
        shard_id: shard_id.to_string(),
        expiration_timestamp: None,
        authority_presentations: Vec::new(),
    };
    let proto = owner_direct_offer(
        &unfunded,
        body,
        1_000_000,
        1,
        &shard_resource_schedule(shard_id, 8),
    )
    .unwrap();
    DeployEnvelope::from_proto(proto, offered_funded_v6_limits().envelope).unwrap()
}

async fn propose(node: &TestNode) -> BlockMessage {
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

async fn heartbeats(nodes: &mut [TestNode], turn: &mut usize, until: impl Fn(&[TestNode]) -> bool) {
    for _ in 0..240 {
        if until(nodes) {
            return;
        }
        for node in nodes.iter() {
            engine(node).wake();
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
        let block = propose(&nodes[*turn % nodes.len()]).await;
        broadcast(nodes, &block).await;
        *turn += 1;
    }
    panic!("the condition was not reached");
}

fn inclusion_evidence(node: &TestNode) -> Vec<Evidence> {
    engine(node)
        .evidence()
        .into_iter()
        .filter(|e| matches!(e.offence, Offence::InclusionViolation { .. }))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_block_that_orders_an_unfunded_envelope_yields_inclusion_evidence_on_every_validator() {
    crate::init_logger();
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    let mut parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    parameters.2.version = casper::rust::casper::PROTOCOL_V6_VERSION;
    parameters.2.proof_of_stake.min_phlo_price = 1;
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .unwrap();
    let shard_id = genesis.genesis_block.shard_id.clone();
    let mut nodes = TestNode::create_v6_network(genesis, 3).await.unwrap();

    let mut turn = 0usize;
    heartbeats(&mut nodes, &mut turn, |nodes| {
        nodes.iter().all(|n| engine(n).canonical_len() >= 2)
    })
    .await;
    while !turn.is_multiple_of(nodes.len()) {
        let block = propose(&nodes[turn % nodes.len()]).await;
        broadcast(&mut nodes, &block).await;
        turn += 1;
    }

    let envelope = unfunded_offer(&shard_id);
    let honest = propose(&nodes[0]).await;
    assert!(honest.body.deploys.is_empty());
    let mut dishonest = honest.clone();
    dishonest.body.deploys = vec![ProcessedUserDeploy::Ordered(envelope.clone())];
    let dishonest = nodes[0]
        .casper
        .get_validator()
        .unwrap()
        .sign_block(&dishonest);
    let attested = dishonest
        .body
        .v6
        .as_ref()
        .unwrap()
        .attestation
        .clone()
        .unwrap();
    broadcast(&mut nodes, &dishonest).await;
    turn += 1;

    heartbeats(&mut nodes, &mut turn, |nodes| {
        nodes.iter().all(|n| !inclusion_evidence(n).is_empty())
    })
    .await;
    heartbeats(&mut nodes, &mut turn, |nodes| {
        nodes
            .iter()
            .all(|n| engine(n).executed(&dishonest.block_hash).is_some())
    })
    .await;

    let expected = Evidence {
        offender: dishonest.sender.clone(),
        citer: dishonest.block_hash.clone(),
        target: attested.block_hash.clone(),
        offence: Offence::InclusionViolation {
            envelope_id: Bytes::copy_from_slice(envelope.identity().as_bytes()),
            cause: String::new(),
        },
    };
    for node in &nodes {
        let evidence = engine(node).evidence();
        assert_eq!(evidence.len(), 1, "unexpected evidence: {:?}", evidence);
        let mut found = evidence[0].clone();
        let Offence::InclusionViolation { cause, .. } = &mut found.offence else {
            panic!("expected inclusion evidence, got {:?}", found);
        };
        assert!(!cause.is_empty());
        cause.clear();
        assert_eq!(found, expected);
    }
}
