// See casper/src/test/scala/coop/rchain/casper/util/rholang/DeployerIdTest.scala

use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use casper::rust::util::{construct_deploy, proto_util};
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
// Changed by DR-134 (DR-66 item 1): the expected deployer id is a principal id.
// use models::rhoapi::{Expr, GDeployerId, GUnforgeable, Par};
use models::rhoapi::{Expr, GUnforgeable, Par};
use prost::bytes::Bytes;

// Added by DR-134 (DR-66 item 1): the deploy of a block is an offered deploy.
use crate::helper::offered_deploy;
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;
// Changed by DR-134 (DR-71): the capture takes the adopted policy.
// use crate::util::rholang::resources::with_runtime_manager;
use crate::util::rholang::resources::{adopted_policy, with_runtime_manager};

fn default_sec() -> PrivateKey { construct_deploy::DEFAULT_SEC.clone() }

fn default_sec2() -> PrivateKey { construct_deploy::DEFAULT_SEC2.clone() }

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deployer_id_should_be_equal_to_the_deployers_public_key() {
    // Changed by DR-134 (DR-71): the capture is the native dry run of the
    // adopted policy.
    // with_runtime_manager(|runtime_manager, _, _| async move {
    with_runtime_manager(|runtime_manager, _, genesis_block| async move {
        let adopted = adopted_policy(&runtime_manager, &genesis_block).await;
        let sk = PrivateKey::from_bytes(
            &hex::decode("b18e1d0045995ec3d010c387ccfeb984d783af8fbb0f40fa7db126d889f6dadd")
                .unwrap(),
        );
        let pk = Bytes::from(Secp256k1.to_public(&sk).bytes.to_vec());

        let deploy = construct_deploy::source_deploy_now_full(
            r#"new return, auth(`rho:system:deployerId`) in { return!(*auth) }"#.to_string(),
            None,
            None,
            Some(sk),
            None,
            None,
        )
        .unwrap();

        let empty_state_hash = RuntimeManager::empty_state_hash_fixed();
        let result = runtime_manager
            .capture_results(&empty_state_hash, &deploy, &adopted)
            .await
            .unwrap();

        assert_eq!(result.len(), 1);

        // Changed by DR-134 (DR-66 item 1, DR-71): a protocol-6 deploy binds
        // `rho:system:deployerId` to the signer's principal id, key family 1
        // (models/src/rust/normalizer_env.rs), which holds the public key.
        // let expected = Par {
        //     unforgeables: vec![GUnforgeable {
        //         unf_instance: Some(UnfInstance::GDeployerIdBody(GDeployerId {
        //             public_key: pk.to_vec(),
        //         })),
        //     }],
        //     ..Default::default()
        // };
        let expected = Par {
            unforgeables: vec![GUnforgeable {
                unf_instance: Some(UnfInstance::GPrincipalIdBody(
                    models::rhoapi::GPrincipalId {
                        key_family: 1,
                        public_key: pk.to_vec(),
                    },
                )),
            }],
            ..Default::default()
        };
        assert_eq!(result[0], expected);
    })
    .await
    .unwrap();
}

async fn check_access_granted(
    deployer: PrivateKey,
    contract_user: PrivateKey,
    is_access_granted: bool,
) {
    let check_deployer_definition = r#"
contract @"checkAuth"(input, ret) = {
  new auth(`rho:system:deployerId`) in {
    ret!(*input == *auth)
  }
}"#;

    let check_deployer_call = r#"
new return, auth(`rho:system:deployerId`), ret in {
  @"checkAuth"!(*auth, *ret) |
  for(isAuthenticated <- ret) {
    return!(*isAuthenticated)
  }
}"#;

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

    // Changed by DR-134 (DR-66 item 1): the deploy of the block is an offered
    // deploy. The exploratory read below keeps a legacy deploy.
    // let contract = construct_deploy::source_deploy_now_full(
    let contract = offered_deploy::source_deploy_now_full(
        check_deployer_definition.to_string(),
        None,
        None,
        Some(deployer),
        None,
        Some(node.genesis.shard_id.clone()),
    )
    .unwrap();

    let block = node.add_block_from_deploys(&[contract]).await.unwrap();

    let check_auth_deploy = construct_deploy::source_deploy_now_full(
        check_deployer_call.to_string(),
        None,
        None,
        Some(contract_user),
        None,
        None,
    )
    .unwrap();

    let result = node
        .runtime_manager
        .capture_results(
            &proto_util::post_state_hash(&block),
            &check_auth_deploy,
            node.adopted_policy(),
        )
        .await
        .unwrap();

    assert_eq!(result.len(), 1);
    let expected = Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GBool(is_access_granted)),
        }],
        ..Default::default()
    };
    assert_eq!(result[0], expected);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deployer_id_should_make_drain_vault_attacks_impossible() {
    let deployer = default_sec();
    let attacker = default_sec2();

    check_access_granted(deployer.clone(), deployer.clone(), true).await;
    check_access_granted(deployer, attacker, false).await;
}
