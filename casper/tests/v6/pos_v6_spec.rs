use casper::rust::genesis::genesis::Genesis;
use casper::rust::v6::genesis_wallets::{read_bond_balance, read_legacy_bonds};
use casper::rust::v6::resource_policy::{ShardResourcePolicy, PROTOCOL_V6};

use crate::util::genesis_builder::GenesisBuilder;
use crate::util::rholang::resources::mk_runtime_manager;

const BONDS_QUERY: &str = r#"
new ret, rl(`rho:registry:lookup`), posCh in {
  rl!(`rho:system:pos`, *posCh) |
  for (@(_, PoS) <- posCh) { @PoS!("getBonds", *ret) }
}
"#;

const VAULT_FACADE_QUERY: &str = r#"
new ret, rl(`rho:registry:lookup`), svCh in {
  rl!(`rho:vault:system`, *svCh) |
  for (@(_, SystemVault) <- svCh) { @SystemVault!("balance", "__ADDRESS__", *ret) }
}
"#;

#[tokio::test]
async fn a_v6_genesis_runs_pos_on_native_bonds_and_the_vault_facade() {
    let rm = mk_runtime_manager("v6-pos", Some(Genesis::default_mergeable_tags_arc())).await;
    let (_, vault_keys, mut genesis) =
        GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(1));
    genesis.version = PROTOCOL_V6;
    let block = Genesis::create_genesis_block_with_policy(
        &rm,
        &genesis,
        Some(&ShardResourcePolicy::default()),
    )
    .await
    .expect("a v6 genesis with PoSV6 builds");
    assert!(block.body.deploys.iter().all(|d| !d.is_failed));
    let post = &block.body.state.post_state_hash;

    let (bonds, _) = rm
        .play_exploratory_deploy(BONDS_QUERY.to_string(), post, None)
        .await
        .unwrap();
    assert_eq!(bonds.len(), 1, "PoS answers getBonds");
    for v in &genesis.proof_of_stake.validators {
        assert_eq!(
            read_bond_balance(&rm, post, &v.pk.bytes).await.unwrap(),
            v.stake as u64
        );
    }
    assert_eq!(read_legacy_bonds(&rm, post).await.unwrap(), 0);

    let funded = &genesis.vaults[0];
    let query = VAULT_FACADE_QUERY.replace("__ADDRESS__", &funded.vault_address.to_base58());
    let (answer, _) = rm.play_exploratory_deploy(query, post, None).await.unwrap();
    let (ok, balance) =
        rholang::rust::interpreter::rho_type::RhoTuple2::unapply(&answer[0]).unwrap();
    assert_eq!(
        rholang::rust::interpreter::rho_type::RhoBoolean::unapply(&ok),
        Some(true)
    );
    assert_eq!(
        rholang::rust::interpreter::rho_type::RhoNumber::unapply(&balance),
        Some(funded.initial_balance as i64)
    );
    let _ = vault_keys;
}
