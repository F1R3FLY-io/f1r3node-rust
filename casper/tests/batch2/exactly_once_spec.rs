use casper::rust::util::construct_deploy;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use models::rust::casper::protocol::casper_message::BlockMessage;
use prost::bytes::Bytes;
use serial_test::serial;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

fn rejected_sigs(block: &BlockMessage) -> Vec<Bytes> {
    block
        .body
        .rejected_deploys
        .iter()
        .map(|record| record.sig.clone())
        .collect()
}

fn short(sig: &Bytes) -> String { hex::encode(&sig[..8.min(sig.len())]) }

async fn three_node_network() -> (Vec<TestNode>, String) {
    let genesis_parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(3));
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(genesis_parameters))
        .await
        .expect("build genesis");
    let shard_id = genesis.genesis_block.shard_id.clone();
    let mut nodes = TestNode::create_network(genesis, 3, None, None, None, None)
        .await
        .expect("create three-node network");
    for node in &mut nodes {
        node.allow_empty_blocks = true;
    }
    (nodes, shard_id)
}

async fn string_datums(node: &TestNode, state_hash: &Bytes, name: &str) -> Vec<String> {
    let channel = Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GString(name.to_string())),
        }],
        ..Default::default()
    };
    node.runtime_manager
        .get_data(state_hash.clone(), &channel)
        .await
        .unwrap_or_else(|error| panic!("get_data @\"{name}\": {error:?}"))
        .iter()
        .map(|par| {
            match par
                .exprs
                .first()
                .and_then(|expr| expr.expr_instance.clone())
            {
                Some(ExprInstance::GString(value)) => value,
                other => format!("<non-string datum: {other:?}>"),
            }
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn duplicate_sibling_inclusions_must_reconcile_to_one_effect() {
    let (mut nodes, shard_id) = three_node_network().await;
    let duplicate = construct_deploy::source_deploy_now_full(
        r#"@"X3"!("once")"#.to_string(),
        None,
        None,
        Some(construct_deploy::DEFAULT_SEC.clone()),
        Some(0),
        Some(shard_id.clone()),
    )
    .expect("build duplicate deploy");

    let first_carrier = nodes[2]
        .add_block_from_deploys(std::slice::from_ref(&duplicate))
        .await
        .expect("propose first sibling carrier");
    let spacer = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        let deploy = construct_deploy::basic_deploy_data(
            0,
            Some(construct_deploy::DEFAULT_SEC2.clone()),
            Some(shard_id.clone()),
        )
        .expect("build spacer deploy");
        nodes[0]
            .add_block_from_deploys(std::slice::from_ref(&deploy))
            .await
            .expect("propose spacer")
    };
    let second_carrier = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&duplicate))
        .await
        .expect("propose second sibling carrier");

    for block in [&first_carrier, &spacer, &second_carrier] {
        nodes[1]
            .process_block(block.clone())
            .await
            .expect("process sibling carrier");
    }
    let marker = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::basic_deploy_data(
            1,
            Some(construct_deploy::DEFAULT_SEC2.clone()),
            Some(shard_id),
        )
        .expect("build merge marker")
    };
    let merge_block = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&marker))
        .await
        .expect("propose reconciling merge");

    assert!(
        merge_block
            .header
            .parents_hash_list
            .contains(&first_carrier.block_hash)
            && merge_block
                .header
                .parents_hash_list
                .contains(&second_carrier.block_hash),
        "merge must include both sibling carriers"
    );

    let datums = string_datums(&nodes[1], &merge_block.body.state.post_state_hash, "X3").await;
    assert_eq!(
        datums.len(),
        1,
        "duplicate sibling inclusion produced {} canonical effects: {datums:?}; rejected: {:?}",
        datums.len(),
        rejected_sigs(&merge_block)
            .iter()
            .map(short)
            .collect::<Vec<_>>()
    );
}
