use std::time::Duration;

use casper::rust::casper::MultiParentCasper;
use casper::rust::util::construct_deploy;
use casper::rust::v6::evidence::{Evidence, Offence};
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::rust::casper::protocol::casper_message::ProcessedDeploy;

use crate::helper::test_node::TestNode;
use crate::helper::v6_network::{v6_genesis, broadcast, engine, propose, propose_and_broadcast};

async fn heartbeats(nodes: &mut [TestNode], turn: &mut usize, until: impl Fn(&[TestNode]) -> bool) {
    for _ in 0..240 {
        if until(nodes) {
            return;
        }
        for node in nodes.iter() {
            engine(node).wake();
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
        let proposer = *turn % nodes.len();
        propose_and_broadcast(nodes, proposer, &[]).await;
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
    let genesis = v6_genesis().await;
    let shard_id = genesis.genesis_block.shard_id.clone();
    let mut nodes = TestNode::create_v6_network(genesis, 3).await.unwrap();

    let mut turn = 0usize;
    heartbeats(&mut nodes, &mut turn, |nodes| {
        nodes.iter().all(|n| engine(n).canonical_len() >= 2)
    })
    .await;
    while !turn.is_multiple_of(nodes.len()) {
        let proposer = turn % nodes.len();
        propose_and_broadcast(&mut nodes, proposer, &[]).await;
        turn += 1;
    }

    let (unfunded, _) = Secp256k1.new_key_pair();
    let envelope = construct_deploy::source_deploy(
        r#"@"unfunded"!(1)"#.to_string(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64,
        Some(500_000),
        Some(1),
        Some(unfunded),
        None,
        Some(shard_id),
    )
    .unwrap();
    let honest = propose(&nodes[0], &[]).await;
    assert!(honest.body.deploys.is_empty());
    let mut dishonest = honest.clone();
    dishonest.body.deploys = vec![ProcessedDeploy::empty(envelope.clone())];
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
            envelope_id: envelope.sig.clone(),
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
