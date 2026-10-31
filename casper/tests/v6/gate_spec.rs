use casper::rust::genesis::contracts::vault::Vault;
use casper::rust::genesis::genesis::Genesis;
use casper::rust::util::construct_deploy;
use casper::rust::v6::gate::{
    check_recorded, gate_block, GateInput, PhloLimitDemand, Refusal, StoreReader, Verdict,
};
use casper::rust::v6::resource_policy::{ShardResourcePolicy, PROTOCOL_V6};
use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use rholang::rust::interpreter::accounting::rev_stack::{wallet_channel_id, WalletOwner};
use rholang::rust::interpreter::util::vault_address::VaultAddress;

use crate::util::genesis_builder::GenesisBuilder;
use crate::util::rholang::resources::mk_runtime_manager;

fn input(sec: &PrivateKey, ts: i64, limit: i64, price: i64) -> GateInput {
    let deploy = construct_deploy::source_deploy(
        "Nil".to_string(),
        ts,
        Some(limit),
        Some(price),
        Some(sec.clone()),
        None,
        Some("root".to_string()),
    )
    .unwrap();
    GateInput::from_deploy(&deploy, Vec::new()).unwrap()
}

#[tokio::test]
async fn the_gate_reads_wallets_natively_at_the_pre_state() {
    let rm = mk_runtime_manager("v6-gate", Some(Genesis::default_mergeable_tags_arc())).await;
    let (alice_sk, alice_pk): (PrivateKey, PublicKey) = Secp256k1.new_key_pair();
    let (bob_sk, _) = Secp256k1.new_key_pair();
    let (_, _, mut genesis) = GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(1));
    genesis.version = PROTOCOL_V6;
    genesis.vaults = vec![Vault {
        vault_address: VaultAddress::from_public_key(&alice_pk).unwrap(),
        initial_balance: 10_000,
    }];
    let block = Genesis::create_genesis_block_with_policy(
        &rm,
        &genesis,
        Some(&ShardResourcePolicy::default()),
    )
    .await
    .unwrap();
    let pre = &block.body.state.post_state_hash;
    let reader = StoreReader::at(&rm, pre).await.unwrap();

    let inputs = vec![
        input(&alice_sk, 1, 1_000, 4),
        input(&alice_sk, 2, 1_000, 4),
        input(&alice_sk, 3, 1_000, 4),
        input(&bob_sk, 4, 10, 1),
        input(&bob_sk, 5, 10, 0),
    ];
    let out = gate_block(&reader, &PhloLimitDemand, &inputs).unwrap();
    assert_eq!(out.admitted(), vec![true, true, false, false, true]);
    let alice = wallet_channel_id(&WalletOwner::from_public_key(&alice_pk).unwrap());
    assert_eq!(out.table.committed(&alice), 8_000);
    assert_eq!(
        out.entries[2].verdict,
        Verdict::Refused(Refusal::InsufficientFunds {
            wallet: alice.clone(),
            need: 4_000,
            have: 2_000,
        })
    );
    assert_eq!(out.visible_balance(10_000, 1, &alice), 2_000);

    let replay_reader = StoreReader::at(&rm, pre).await.unwrap();
    assert!(check_recorded(&replay_reader, &PhloLimitDemand, &inputs, &out.entries).is_ok());
}
