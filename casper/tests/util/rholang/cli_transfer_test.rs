use std::time::{SystemTime, UNIX_EPOCH};

use casper::rust::rholang::runtime::RuntimeOps;
use casper::rust::util::comm::deploy_runtime::{DeployParams, DeployRuntime};
use casper::rust::util::construct_deploy::DEFAULT_SEC;
use casper::rust::util::proto_util;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{GDeployId, GUnforgeable, Par};
use rholang::rust::interpreter::accounting::rev_stack::{native, RevStacks, WalletOwner};
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::rho_type::{RhoBoolean, RhoString, RhoTuple2};
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn either(par: &Par) -> (bool, Par) {
    let (flag, value) = RhoTuple2::unapply(par).unwrap();
    (RhoBoolean::unapply(&flag).unwrap(), value)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_cli_transfer_deploy_runs_on_the_node_runtime() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .unwrap();
    let node = TestNode::standalone(genesis).await.unwrap();

    let payer_pk = Secp256k1.to_public(&DEFAULT_SEC);
    let payer = WalletOwner::from_public_key(&payer_pk).unwrap();
    let (_, payee_pk) = Secp256k1.new_key_pair();
    let payee = WalletOwner::from_public_key(&payee_pk).unwrap();
    let payee_address = VaultAddress::from_public_key(&payee_pk)
        .unwrap()
        .to_base58();

    let params = DeployParams {
        phlo_limit: 5_000_000,
        phlo_price: 1,
        valid_after_block: 0,
        shard_id: node.genesis.shard_id.clone(),
    };
    let signed =
        DeployRuntime::transfer_deploy(&payee_address, 40, &params, &DEFAULT_SEC, now_ms())
            .unwrap();
    assert_eq!(
        signed.data.term,
        native::transfer_term(&payee_address, 40).unwrap()
    );
    let deploy_id = Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GDeployIdBody(GDeployId {
            sig: signed.sig.to_vec(),
        })),
    }]);

    let mut runtime = node.runtime_manager.spawn_runtime().await.unwrap();
    let start = proto_util::post_state_hash(&node.genesis);
    runtime
        .reset(&Blake2b256Hash::from_bytes_prost(&start))
        .await
        .unwrap();
    let stacks = RevStacks::over_hot_store(runtime.reducer.space.hot_store().unwrap());
    stacks.deposit(&payer, 100).unwrap();

    let mut ops = RuntimeOps::new(runtime);
    let result = ops.evaluate(&signed).await.unwrap();
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let reported = ops.get_data_par(&deploy_id).await;
    assert_eq!(reported.len(), 1);
    assert_eq!(either(&reported[0]), (true, Par::default()));
    assert_eq!(stacks.balance(&payer).unwrap(), 60);
    assert_eq!(stacks.balance(&payee).unwrap(), 40);

    let again =
        DeployRuntime::transfer_deploy(&payee_address, 61, &params, &DEFAULT_SEC, now_ms() + 1)
            .unwrap();
    let again_id = Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GDeployIdBody(GDeployId {
            sig: again.sig.to_vec(),
        })),
    }]);
    let result = ops.evaluate(&again).await.unwrap();
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(
        either(&ops.get_data_par(&again_id).await[0]),
        (
            false,
            RhoString::create_par("Insufficient funds".to_string())
        )
    );
    assert_eq!(stacks.balance(&payer).unwrap(), 60);
}

#[test]
fn the_cli_refuses_a_bad_transfer_before_signing() {
    let params = DeployParams {
        phlo_limit: 1,
        phlo_price: 1,
        valid_after_block: 0,
        shard_id: String::new(),
    };
    assert!(DeployRuntime::transfer_deploy("lala", 1, &params, &DEFAULT_SEC, 0).is_err());
    let (_, pk) = Secp256k1.new_key_pair();
    let address = VaultAddress::from_public_key(&pk).unwrap().to_base58();
    assert!(DeployRuntime::transfer_deploy(&address, 0, &params, &DEFAULT_SEC, 0).is_err());
}
