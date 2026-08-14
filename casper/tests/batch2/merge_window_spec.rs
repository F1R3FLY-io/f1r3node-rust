use casper::rust::util::construct_deploy;
use crypto::rust::private_key::PrivateKey;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use prost::bytes::Bytes;
use serial_test::serial;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

const DEPLOY_LIFESPAN: i64 = 5;

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
async fn late_carrier_past_window_is_rejected_with_record_and_without_effect() {
    let n_validators = 3usize;
    let genesis_parameters =
        GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(n_validators));
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(genesis_parameters))
        .await
        .unwrap();
    let shard_id = genesis.genesis_block.shard_id.clone();

    let mut nodes = TestNode::create_network_with_deploy_lifespan(
        genesis,
        n_validators,
        None,
        None,
        None,
        None,
        Some(DEPLOY_LIFESPAN),
    )
    .await
    .expect("create network");
    for node in &mut nodes {
        node.allow_empty_blocks = true;
    }

    let late_deploy = construct_deploy::source_deploy_now_full(
        r#"@"late"!(1)"#.to_string(),
        None,
        None,
        Some(construct_deploy::DEFAULT_SEC.clone()),
        Some(0),
        Some(shard_id.clone()),
    )
    .expect("build late deploy");
    let late_sig = late_deploy.sig.clone();
    let carrier = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&late_deploy))
        .await
        .expect("carrier block");
    assert!(
        carrier
            .body
            .deploys
            .iter()
            .any(|deploy| deploy.deploy.sig == late_sig && !deploy.is_failed),
        "the carrier must execute the deploy"
    );

    let keep_deploy = construct_deploy::source_deploy_now_full(
        r#"@"keep"!(1)"#.to_string(),
        None,
        None,
        Some(construct_deploy::DEFAULT_SEC2.clone()),
        Some(0),
        Some(shard_id.clone()),
    )
    .expect("build retained deploy");
    let keep_sig = keep_deploy.sig.clone();
    nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&keep_deploy))
        .await
        .expect("retained-deploy block");
    {
        let (first_two, third) = nodes.split_at_mut(2);
        third[0]
            .sync_with_one(&mut first_two[1])
            .await
            .expect("sync retained-deploy block");
    }

    let progression_keys: [&PrivateKey; 2] = [
        &construct_deploy::DEFAULT_SEC2,
        &crate::util::genesis_builder::EXTRA_GENESIS_VAULT_KEY_PAIRS[0].0,
    ];
    for round in 0..8i32 {
        let proposer = 1 + (round % 2) as usize;
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        let marker = construct_deploy::basic_deploy_data(
            round,
            Some(progression_keys[proposer - 1].clone()),
            Some(shard_id.clone()),
        )
        .expect("progression marker");
        nodes[proposer]
            .add_block_from_deploys(std::slice::from_ref(&marker))
            .await
            .expect("progression block");
        let (first_two, third) = nodes.split_at_mut(2);
        if proposer == 1 {
            third[0]
                .sync_with_one(&mut first_two[1])
                .await
                .expect("sync progression to third validator");
        } else {
            first_two[1]
                .sync_with_one(&mut third[0])
                .await
                .expect("sync progression to second validator");
        }
    }

    {
        let (carrier_node, other_nodes) = nodes.split_at_mut(1);
        other_nodes[0]
            .sync_with_one(&mut carrier_node[0])
            .await
            .expect("late carrier sync");
    }
    assert!(nodes[1].contains(&carrier.block_hash));

    tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
    let merge_marker = construct_deploy::basic_deploy_data(
        100,
        Some(construct_deploy::DEFAULT_SEC2.clone()),
        Some(shard_id),
    )
    .expect("merge marker");
    let merge_block = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&merge_marker))
        .await
        .expect("late merge block");
    assert!(
        merge_block
            .header
            .parents_hash_list
            .contains(&carrier.block_hash),
        "the merge must include the late carrier"
    );

    let rejected_sigs: Vec<Bytes> = merge_block
        .body
        .rejected_deploys
        .iter()
        .map(|record| record.sig.clone())
        .collect();
    assert!(
        rejected_sigs.contains(&late_sig),
        "a carrier outside the floor window must have a rejection record"
    );
    assert!(
        !rejected_sigs.contains(&keep_sig),
        "the merge must preserve in-window history"
    );

    let keep_datums = nodes[1]
        .runtime_manager
        .get_data(
            merge_block.body.state.post_state_hash.clone(),
            &gstring_channel("keep"),
        )
        .await
        .expect("read retained channel");
    assert!(!keep_datums.is_empty());
    let late_datums = nodes[1]
        .runtime_manager
        .get_data(
            merge_block.body.state.post_state_hash,
            &gstring_channel("late"),
        )
        .await
        .expect("read late channel");
    assert!(
        late_datums.is_empty(),
        "a carrier outside the floor window must not apply its effect"
    );
}
