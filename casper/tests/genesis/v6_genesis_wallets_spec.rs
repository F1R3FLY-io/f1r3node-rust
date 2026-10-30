use casper::rust::genesis::contracts::vault::Vault;
use casper::rust::genesis::genesis::Genesis;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use casper::rust::v6::genesis_wallets::{
    bonds_total, genesis_supply_of, read_genesis_supply, read_legacy_bonds, read_wallet_balance,
    wallets_supply,
};
use casper::rust::v6::resource_policy::{ShardResourcePolicy, PROTOCOL_V6};
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use rholang::rust::interpreter::accounting::rev_stack::{WalletOwner, MAX_AMOUNT};
use rholang::rust::interpreter::rho_type::{RhoBigInt, RhoNumber};
use rholang::rust::interpreter::system_processes::BlockData;
use rholang::rust::interpreter::util::vault_address::VaultAddress;

use crate::util::genesis_builder::GenesisBuilder;
use crate::util::rholang::resources::mk_runtime_manager;

fn policy() -> ShardResourcePolicy {
    ShardResourcePolicy {
        block_records: 16,
        block_record_bytes: 1 << 20,
        max_envelopes: 32,
        demand_cap: 1_000_000,
    }
}

fn fresh_vault(initial_balance: u128) -> Vault {
    let (_, pk) = Secp256k1.new_key_pair();
    Vault {
        vault_address: VaultAddress::from_public_key(&pk).unwrap(),
        initial_balance,
    }
}

fn parameters(version: i64, vaults: Vec<Vault>) -> Genesis {
    let (_, _, mut genesis) = GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(1));
    genesis.version = version;
    genesis.vaults = vaults;
    genesis
}

async fn runtime_manager() -> RuntimeManager {
    mk_runtime_manager(
        "v6-genesis-wallets",
        Some(Genesis::default_mergeable_tags_arc()),
    )
    .await
}

const LEGACY_BALANCE_QUERY: &str = r#"
new ret, rl(`rho:registry:lookup`), svCh, vaultCh, balanceCh in {
  rl!(`rho:vault:system`, *svCh) |
  for (@(_, SystemVault) <- svCh) {
    @SystemVault!("findOrCreate", "__ADDRESS__", *vaultCh) |
    for (@(true, vault) <- vaultCh) {
      @vault!("balance", *balanceCh) |
      for (@balance <- balanceCh) { ret!(balance) }
    }
  }
}
"#;

async fn legacy_vault_balance(
    rm: &RuntimeManager,
    state: &models::rust::block::state_hash::StateHash,
    address: &str,
) -> String {
    let (result, _) = rm
        .play_exploratory_deploy(
            LEGACY_BALANCE_QUERY.replace("__ADDRESS__", address),
            state,
            None,
        )
        .await
        .expect("the legacy balance query runs");
    let par = result
        .first()
        .unwrap_or_else(|| panic!("no result for {address}"));
    RhoBigInt::unapply(par)
        .map(|n| n.to_string())
        .or_else(|| RhoNumber::unapply(par).map(|n| n.to_string()))
        .unwrap_or_else(|| panic!("not a number: {result:?}"))
}

#[tokio::test]
async fn a_v6_genesis_holds_wallets_txt_on_native_stacks() {
    let rm = runtime_manager().await;
    let (alice, bob, carol) = (fresh_vault(1_000), fresh_vault(250), fresh_vault(0));
    let duplicate = Vault {
        initial_balance: 5,
        ..alice.clone()
    };
    let vaults: Vec<Vault> = (0..230)
        .map(|i| fresh_vault(i + 1))
        .chain([alice.clone(), bob.clone(), carol.clone(), duplicate])
        .collect();
    let genesis = parameters(PROTOCOL_V6, vaults.clone());
    let block = Genesis::create_genesis_block_with_policy(&rm, &genesis, Some(&policy()))
        .await
        .expect("a protocol 6 genesis builds");
    assert!(block.body.deploys.iter().all(|d| !d.is_failed));
    let post = &block.body.state.post_state_hash;

    let expected: u64 = vaults.iter().map(|v| v.initial_balance as u64).sum();
    assert_eq!(wallets_supply(&vaults).unwrap(), expected);
    let bonds = bonds_total(&genesis.proof_of_stake).unwrap();
    assert!(bonds > 0);
    assert_eq!(
        genesis_supply_of(&vaults, &genesis.proof_of_stake).unwrap(),
        expected + bonds
    );
    assert_eq!(
        read_genesis_supply(&rm, post).await.unwrap(),
        expected + bonds
    );
    assert_eq!(read_legacy_bonds(&rm, post).await.unwrap(), bonds);

    let owner = |v: &Vault| WalletOwner::from_vault_address(v.vault_address.clone());
    assert_eq!(
        read_wallet_balance(&rm, post, &owner(&alice))
            .await
            .unwrap(),
        1_005
    );
    assert_eq!(
        read_wallet_balance(&rm, post, &owner(&bob)).await.unwrap(),
        250
    );
    assert_eq!(
        read_wallet_balance(&rm, post, &owner(&carol))
            .await
            .unwrap(),
        0
    );
    let mut held = 0;
    for v in vaults.iter().take(230) {
        held += read_wallet_balance(&rm, post, &owner(v)).await.unwrap();
    }
    assert_eq!(held + 1_005 + 250, expected);

    for v in [&alice, &bob] {
        assert_eq!(
            legacy_vault_balance(&rm, post, &v.vault_address.to_base58()).await,
            "0"
        );
    }

    let replayed = rm
        .replay_compute_state(
            &block.body.state.pre_state_hash,
            block.body.deploys.clone(),
            Vec::new(),
            &BlockData::from_block(&block),
            None,
            true,
        )
        .await
        .expect("genesis replays");
    assert_eq!(&replayed, post);
}

#[tokio::test]
async fn a_legacy_genesis_mints_no_native_stacks() {
    let rm = runtime_manager().await;
    let alice = fresh_vault(1_000);
    let genesis = parameters(1, vec![alice.clone()]);
    let block = Genesis::create_genesis_block(&rm, &genesis)
        .await
        .expect("a legacy genesis builds");
    let post = &block.body.state.post_state_hash;
    assert_eq!(read_genesis_supply(&rm, post).await.unwrap(), 0);
    let owner = WalletOwner::from_vault_address(alice.vault_address.clone());
    assert_eq!(read_wallet_balance(&rm, post, &owner).await.unwrap(), 0);
    assert_eq!(
        legacy_vault_balance(&rm, post, &alice.vault_address.to_base58()).await,
        "1000"
    );
}

#[tokio::test]
async fn a_v6_genesis_refuses_a_supply_beyond_the_largest_amount() {
    let rm = runtime_manager().await;
    let too_big = parameters(PROTOCOL_V6, vec![fresh_vault(MAX_AMOUNT as u128 + 1)]);
    assert!(
        Genesis::create_genesis_block_with_policy(&rm, &too_big, Some(&policy()))
            .await
            .is_err()
    );
    let overflow = parameters(
        PROTOCOL_V6,
        vec![fresh_vault(MAX_AMOUNT as u128), fresh_vault(1)],
    );
    assert!(
        Genesis::create_genesis_block_with_policy(&rm, &overflow, Some(&policy()))
            .await
            .is_err()
    );
}
