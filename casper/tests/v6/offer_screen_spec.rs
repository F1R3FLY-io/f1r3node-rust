use std::sync::Arc;

use block_storage::rust::deploy::key_value_deploy_storage::KeyValueDeployStorage;
use casper::rust::genesis::contracts::vault::Vault;
use casper::rust::genesis::genesis::Genesis;
use casper::rust::util::construct_deploy;
use casper::rust::v6::inclusion::{screen_offers, GateScreen};
use casper::rust::v6::resource_policy::{ShardResourcePolicy, PROTOCOL_V6};
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use crypto::rust::signatures::signed::Signed;
use models::rust::casper::protocol::casper_message::{DeployData, ProcessedDeploy};
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

use crate::util::genesis_builder::GenesisBuilder;
use crate::util::rholang::resources::mk_runtime_manager;

fn offer(sk: &PrivateKey, ts: i64) -> Signed<DeployData> {
    construct_deploy::source_deploy(
        "Nil".to_string(),
        ts,
        Some(1_000),
        Some(3),
        Some(sk.clone()),
        None,
        Some("root".to_string()),
    )
    .unwrap()
}

#[tokio::test]
async fn a_refused_offer_reports_rejected_and_leaves_the_pool() {
    let (rich_sk, rich_pk) = Secp256k1.new_key_pair();
    let (poor_sk, _) = Secp256k1.new_key_pair();
    let rm = mk_runtime_manager(
        "v6-offer-screen",
        Some(Genesis::default_mergeable_tags_arc()),
    )
    .await;
    let (_, _, mut genesis) = GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(1));
    genesis.version = PROTOCOL_V6;
    genesis.vaults = vec![Vault {
        vault_address: VaultAddress::from_public_key(&rich_pk).unwrap(),
        initial_balance: 100_000,
    }];
    let block = Genesis::create_genesis_block_with_policy(
        &rm,
        &genesis,
        Some(&ShardResourcePolicy::default()),
    )
    .await
    .unwrap();
    let root = block.body.state.post_state_hash.clone();

    let mut kvm = InMemoryStoreManager::new();
    let pool = KeyValueDeployStorage::new(&mut kvm).await.unwrap();
    let (funded, unfunded) = (offer(&rich_sk, 1), offer(&poor_sk, 2));
    pool.add(vec![funded.clone(), unfunded.clone()]).unwrap();

    let screen = GateScreen::new(Arc::new(rm));
    let kept = screen_offers(
        &screen,
        &pool,
        vec![
            ProcessedDeploy::empty(funded.clone()),
            ProcessedDeploy::empty(unfunded.clone()),
        ],
        &root,
        7,
        1,
    )
    .await
    .unwrap();

    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].deploy.sig, funded.sig);
    assert!(pool.contains_sig(&funded.sig).unwrap());
    assert!(pool.rejection(&funded.sig).is_none());

    assert!(!pool.contains_sig(&unfunded.sig).unwrap());
    let rejection = pool
        .rejection(&unfunded.sig)
        .expect("the refusal is recorded");
    assert_eq!(rejection.block_number, 7);
    assert_eq!(rejection.pre_state_root, Some(root.to_vec()));
    assert!(rejection.reason.contains("needs"), "{}", rejection.reason);
}
