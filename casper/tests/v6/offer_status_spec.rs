use block_storage::rust::deploy::offer_rejection::OfferRejection;
use casper::rust::api::block_api::BlockAPI;
use casper::rust::casper::Casper;
use casper::rust::util::construct_deploy;
use casper::rust::v6::receipt::OfferedStatus;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

async fn status(node: &TestNode, id: &[u8]) -> OfferedStatus {
    BlockAPI::offered_status(&node.engine_cell, id)
        .await
        .expect("an offered status")
}

async fn standalone() -> TestNode {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(
            GenesisBuilder::build_genesis_parameters_with_defaults(None, None),
        ))
        .await
        .expect("genesis");
    TestNode::standalone(genesis)
        .await
        .expect("standalone node")
}

#[tokio::test]
async fn an_offer_reports_unknown_pending_rejected_pending_then_ordered() {
    let mut node = standalone().await;
    let offer = construct_deploy::basic_deploy_data(1, None, Some(node.genesis.shard_id.clone()))
        .expect("an offer");
    let id = offer.sig.to_vec();

    assert_eq!(status(&node, &id).await, OfferedStatus::Unknown);
    assert!(matches!(
        node.casper.deploy(offer.clone()).unwrap(),
        Either::Right(_)
    ));
    assert_eq!(status(&node, &id).await, OfferedStatus::Pending);

    let rejection = OfferRejection {
        deploy_id: id.clone(),
        pre_state_root: Some(vec![3; 32]),
        block_number: 1,
        reason: "refused by the gate".to_string(),
    };
    assert!(node
        .deploy_storage
        .record_rejection(rejection.clone())
        .unwrap());
    assert!(!node.deploy_storage.contains_sig(&id).unwrap());
    assert_eq!(status(&node, &id).await, OfferedStatus::Rejected(rejection));

    assert!(matches!(
        node.casper.deploy(offer.clone()).unwrap(),
        Either::Right(_)
    ));
    assert_eq!(status(&node, &id).await, OfferedStatus::Pending);

    let block = node
        .add_block_from_deploys(&[])
        .await
        .expect("a block with the offer");
    assert!(block
        .body
        .deploys
        .iter()
        .any(|deploy| deploy.deploy.sig == offer.sig));
    assert_eq!(
        status(&node, &id).await,
        OfferedStatus::Ordered {
            block_hash: block.block_hash.clone(),
        }
    );
}

#[tokio::test]
async fn an_empty_offer_id_is_refused() {
    let node = standalone().await;
    assert!(BlockAPI::offered_status(&node.engine_cell, &[])
        .await
        .is_err());
}
