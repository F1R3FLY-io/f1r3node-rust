// See casper/src/test/scala/coop/rchain/casper/util/rholang/DeployIdTest.scala

use std::time::SystemTime;

// Changed by DR-134 (DR-66 item 1, DR-71): test 1 reads an offered block, so no
// test of this file uses a bare runtime manager.
// use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use casper::rust::util::{construct_deploy, proto_util};
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::signed::Signed;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{Expr, GDeployId, GUnforgeable, Par};
use models::rust::casper::protocol::casper_message::DeployData;

// Added by DR-134 (DR-66 item 1): the deploy of a block is an offered deploy.
use crate::helper::offered_deploy;
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;
// Changed by DR-134 (DR-66 item 1, DR-71): test 1 reads an offered block.
// use crate::util::rholang::resources::with_runtime_manager;

fn default_sec() -> PrivateKey { construct_deploy::DEFAULT_SEC.clone() }

fn deploy(
    deployer: PrivateKey,
    rho: String,
    _timestamp: Option<i64>,
    shard_id: String,
) -> Signed<DeployData> {
    construct_deploy::source_deploy(
        rho,
        SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64,
        None,
        None,
        Some(deployer),
        None,
        Some(shard_id),
    )
    .unwrap()
}

// Changed by DR-134 (DR-66 item 1, DR-71): every user deploy is an offered
// deploy, and its deploy id is the 32-byte deploy identity that the offer
// signs. A block carries the offer, and the test reads the deploy id that the
// term saw from the deploy's return channel, the first name of its identity
// rng, at the post-state. An exploratory dry run has its own dry-run identity,
// so it cannot show the id of a deploy.
// #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
// async fn deploy_id_should_be_equal_to_deploy_signature() {
//     with_runtime_manager(|runtime_manager, genesis_context, _| async move {
//         let sk = default_sec();
//
//         let d = deploy(
//             sk,
//             r#"new return, deployId(`rho:system:deployId`) in { return!(*deployId) }"#.to_string(),
//             None,
//             genesis_context.genesis_block.shard_id.clone(),
//         );
//
//         let result = runtime_manager
//             .capture_results(&RuntimeManager::empty_state_hash_fixed(), &d)
//             .await
//             .unwrap();
//
//         assert_eq!(result.len(), 1);
//
//         let expected = Par {
//             unforgeables: vec![GUnforgeable {
//                 unf_instance: Some(UnfInstance::GDeployIdBody(GDeployId {
//                     sig: d.sig.to_vec(),
//                 })),
//             }],
//             ..Default::default()
//         };
//         assert_eq!(result[0], expected);
//     })
//     .await
//     .unwrap();
// }
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deploy_id_should_be_equal_to_deploy_signature() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis context");
    let mut node = TestNode::standalone(genesis)
        .await
        .expect("Failed to create standalone node");

    let offer = offered_deploy::source_deploy_now(
        r#"new return, deployId(`rho:system:deployId`) in { return!(*deployId) }"#.to_string(),
        Some(default_sec()),
        None,
        Some(node.genesis.shard_id.clone()),
    )
    .unwrap();
    let identity = offer.sig.clone();
    let block = node.add_block_from_deploys(&[offer]).await.unwrap();

    let mut return_rand =
        casper::rust::util::rholang::tools::Tools::user_identity_rng(identity.as_ref());
    let return_name = Par {
        unforgeables: vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(models::rhoapi::GPrivate {
                id: return_rand.next().into_iter().map(|b| b as u8).collect(),
            })),
        }],
        ..Default::default()
    };
    let result = node
        .runtime_manager
        .get_data(proto_util::post_state_hash(&block), &return_name)
        .await
        .unwrap();

    assert_eq!(result.len(), 1);

    let expected = Par {
        unforgeables: vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GDeployIdBody(GDeployId {
                sig: identity.to_vec(),
            })),
        }],
        ..Default::default()
    };
    assert_eq!(result[0], expected);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deploy_id_should_be_resolved_during_normalization() {
    let mut genesis_builder = GenesisBuilder::new();
    let genesis_parameters_tuple =
        GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    let genesis_context = genesis_builder
        .build_genesis_with_parameters(Some(genesis_parameters_tuple))
        .await
        .expect("Failed to build genesis context");

    let mut node = TestNode::standalone(genesis_context)
        .await
        .expect("Failed to create standalone node");

    let sk = default_sec();

    // Changed by DR-134 (DR-66 item 1): the deploy of the block is an offered
    // deploy. The exploratory read below keeps a legacy deploy.
    // let contract = deploy(
    //     sk.clone(),
    //     r#"contract @"check"(input, ret) = { new deployId(`rho:system:deployId`) in { ret!(*input == *deployId) }}"#
    //         .to_string(),
    //     None,
    //     node.genesis.shard_id.clone(),
    // );
    let contract = offered_deploy::source_deploy_now(
        r#"contract @"check"(input, ret) = { new deployId(`rho:system:deployId`) in { ret!(*input == *deployId) }}"#
            .to_string(),
        Some(sk.clone()),
        None,
        Some(node.genesis.shard_id.clone()),
    )
    .unwrap();

    let contract_call = deploy(
        sk,
        r#"new return, deployId(`rho:system:deployId`), ret in { @"check"!(*deployId, *return) }"#
            .to_string(),
        None,
        node.genesis.shard_id.clone(),
    );

    let block = node.add_block_from_deploys(&[contract]).await.unwrap();

    let result = node
        .runtime_manager
        .capture_results(
            &proto_util::post_state_hash(&block),
            &contract_call,
            node.adopted_policy(),
        )
        .await
        .unwrap();

    assert_eq!(result.len(), 1);
    let expected = Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GBool(false)),
        }],
        ..Default::default()
    };
    assert_eq!(result[0], expected);
}
