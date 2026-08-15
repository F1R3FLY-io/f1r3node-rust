use casper::rust::util::construct_deploy;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use rspace_plus_plus::rspace::history::Either;
use serial_test::serial;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

fn gstring_channel(name: &str) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GString(name.to_string())),
        }],
        ..Default::default()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn netted_install_fire_pair_merges_as_a_noop() {
    let n_validators = 3usize;
    let genesis_parameters =
        GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(n_validators));
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(genesis_parameters))
        .await
        .unwrap();
    let shard_id = genesis.genesis_block.shard_id.clone();
    let mut nodes = TestNode::create_network(genesis, n_validators, None, None, None, None)
        .await
        .expect("create network");
    for node in &mut nodes {
        node.allow_empty_blocks = true;
    }

    tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
    let install = construct_deploy::source_deploy_now_full(
        r#"for (@x <- @"nn_cell") { @"nn_out"!(x) }"#.to_string(),
        None,
        None,
        Some(construct_deploy::DEFAULT_SEC.clone()),
        Some(0),
        Some(shard_id.clone()),
    )
    .expect("build install deploy");
    let install_block = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&install))
        .await
        .expect("install block");
    nodes[1]
        .process_block(install_block)
        .await
        .expect("process install block");

    tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
    let fire = construct_deploy::source_deploy_now_full(
        r#"@"nn_cell"!(42)"#.to_string(),
        None,
        None,
        Some(construct_deploy::DEFAULT_SEC2.clone()),
        Some(0),
        Some(shard_id.clone()),
    )
    .expect("build fire deploy");
    let fire_block = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&fire))
        .await
        .expect("fire block");

    tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
    let marker = construct_deploy::basic_deploy_data(
        7,
        Some(construct_deploy::DEFAULT_SEC.clone()),
        Some(shard_id),
    )
    .expect("build marker");
    let marker_block = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&marker))
        .await
        .expect("marker block");
    nodes[1]
        .process_block(marker_block.clone())
        .await
        .expect("process marker block");
    nodes[0]
        .process_block(fire_block.clone())
        .await
        .expect("process fire block");

    let merge_result = nodes[0].create_block_unsafe(&[]).await;
    assert!(
        merge_result.is_ok(),
        "a fully netted channel change must merge as a no-op: {:?}",
        merge_result.err()
    );
    let merge_block = merge_result.expect("merge block");
    assert!(
        merge_block
            .header
            .parents_hash_list
            .contains(&fire_block.block_hash)
            && merge_block
                .header
                .parents_hash_list
                .contains(&marker_block.block_hash),
        "the block must merge both sibling parents"
    );
    assert!(merge_block.body.rejected_deploys.is_empty());

    let own_outcome = nodes[0]
        .process_block(merge_block.clone())
        .await
        .expect("process own merge block");
    assert!(matches!(own_outcome, Either::Right(_)));
    let peer_outcome = nodes[1]
        .process_block(merge_block.clone())
        .await
        .expect("peer processes merge block");
    assert!(matches!(peer_outcome, Either::Right(_)));

    let output = nodes[0]
        .runtime_manager
        .get_data(
            merge_block.body.state.post_state_hash.clone(),
            &gstring_channel("nn_out"),
        )
        .await
        .expect("read output channel");
    assert_eq!(output.len(), 1);
    let cell = nodes[0]
        .runtime_manager
        .get_data(
            merge_block.body.state.post_state_hash,
            &gstring_channel("nn_cell"),
        )
        .await
        .expect("read cell channel");
    assert!(cell.is_empty());
}
