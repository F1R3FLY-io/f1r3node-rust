use casper::rust::genesis::genesis::Genesis;
use casper::rust::v6::genesis_wallets::cells_at;
use casper::rust::v6::hooks::{after_settlement, genesis_authority, read_hook_authority, HookCall};
use casper::rust::v6::resource_policy::{ShardResourcePolicy, PROTOCOL_V6};
use rholang::rust::interpreter::accounting::rev_stack::compute::compute_balance;
use rholang::rust::interpreter::accounting::rev_stack::CellStore;

use crate::util::genesis_builder::GenesisBuilder;
use crate::util::rholang::resources::mk_runtime_manager;

#[tokio::test]
async fn a_v6_genesis_installs_the_hook_authority_and_a_noop_rev_hook() {
    let rm = mk_runtime_manager("v6-hooks", Some(Genesis::default_mergeable_tags_arc())).await;
    let (_, _, mut genesis) = GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(1));
    genesis.version = PROTOCOL_V6;
    let block = Genesis::create_genesis_block_with_policy(
        &rm,
        &genesis,
        Some(&ShardResourcePolicy::default()),
    )
    .await
    .unwrap();
    let post = &block.body.state.post_state_hash;
    let expected = genesis_authority(&genesis.proof_of_stake).unwrap();
    assert_eq!(
        read_hook_authority(&rm, post).await.unwrap(),
        Some(expected)
    );
    let cells = cells_at(&rm, post).await.unwrap();
    assert_eq!(after_settlement(&cells).unwrap(), HookCall::Noop);
    let cells: std::sync::Arc<dyn CellStore> = std::sync::Arc::new(cells);
    assert_eq!(compute_balance(cells).unwrap(), 0);

    let (_, _, legacy) = GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(1));
    let block = Genesis::create_genesis_block(&rm, &legacy).await.unwrap();
    assert_eq!(
        read_hook_authority(&rm, &block.body.state.post_state_hash)
            .await
            .unwrap(),
        None
    );
}
