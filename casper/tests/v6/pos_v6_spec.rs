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
        .play_exploratory_deploy(BONDS_QUERY.to_string(), post, None, Vec::new())
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
    let (answer, _) = rm.play_exploratory_deploy(query, post, None, Vec::new()).await.unwrap();
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

const ACTIVE_SET_PICKS: &str = r#"
new ret, rl(`rho:registry:lookup`), listOpsCh, fst, mapped in {
  contract fst(@(first, _), resultCh) = { resultCh!(first) } |
  rl!(`rho:lang:listOps`, *listOpsCh) |
  for (@(_, ListOps) <- listOpsCh) {
    @ListOps!("map", __BONDS__.toList(), *fst, *mapped) |
    for (@pks <- mapped) {
      ret!((pks.take(__N__).toSet(), __BONDS__.keys().toList().take(__N__).toSet()))
    }
  }
}
"#;

fn bonds_map(size: usize, seed: u64) -> String {
    let entries: Vec<String> = (0..size)
        .map(|i| {
            let mut key = Vec::with_capacity(65);
            let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(i as u64);
            for _ in 0..65 {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                key.push((state & 0xff) as u8);
            }
            format!("\"{}\".hexToBytes(): {}", hex::encode(key), (i as u64 + 1) * 1_000)
        })
        .collect();
    format!("{{{}}}", entries.join(", "))
}

#[tokio::test]
async fn the_one_expression_active_set_pick_agrees_with_the_list_ops_pick() {
    let rm = mk_runtime_manager("v6-active-set", Some(Genesis::default_mergeable_tags_arc())).await;
    let (_, _, mut genesis) = GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(1));
    genesis.version = PROTOCOL_V6;
    let block = Genesis::create_genesis_block_with_policy(
        &rm,
        &genesis,
        Some(&ShardResourcePolicy::default()),
    )
    .await
    .expect("a v6 genesis builds");
    let post = &block.body.state.post_state_hash;
    let mut checked = 0;
    for (size, seed) in [(0, 1), (1, 2), (3, 3), (7, 4), (12, 5), (30, 6), (30, 7)] {
        for n in [1, 3, 5, 12, 100] {
            let query = ACTIVE_SET_PICKS
                .replace("__BONDS__", &bonds_map(size, seed))
                .replace("__N__", &n.to_string());
            let (answer, _) = rm
                .play_exploratory_deploy(query, post, None, Vec::new())
                .await
                .unwrap();
            let (old, new) =
                rholang::rust::interpreter::rho_type::RhoTuple2::unapply(&answer[0]).unwrap();
            assert_eq!(old, new, "{size} bonds, {n} active");
            checked += 1;
        }
    }
    assert_eq!(checked, 35);
}
