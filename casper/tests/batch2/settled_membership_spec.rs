// State membership is decided by recorded construction facts, never by a
// state-shape probe. The old probe read a deploy's created NUMBER cells
// out of the history trie, so a deploy that creates none — a plain send
// is enough — was invisible: its floor-settled effect read as
// not-settled, and every floor-keyed consumer (the deploy-storage sweep,
// the merge's settled-sig dedup, the floor-context memo) mis-answered
// for it. The pointer walk (state(B) = state(mergeBase) +
// appliedFromScope + deploys) sees every deploy a block executed,
// whatever its effect's shape.

use casper::rust::casper::MultiParentCasper;
use casper::rust::finality::floor::floor_of_block;
use casper::rust::safety::clique_oracle::FtThreshold;
use casper::rust::util::construct_deploy;
use prost::bytes::Bytes;
use serial_test::serial;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

/// A plain send settles under the floor and the deploy-storage sweep
/// releases it as floor-settled — membership must not depend on the
/// effect's shape.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn floor_settled_plain_send_is_released_from_deploy_storage() {
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
        .expect("create_network");
    for node in nodes.iter_mut() {
        node.allow_empty_blocks = true;
    }

    // The deploy whose effect has no number-cell shape: one datum on a
    // public name.
    let plain_send = construct_deploy::source_deploy_now_full(
        r#"@"settled_plain"!("datum")"#.to_string(),
        None,
        None,
        Some(construct_deploy::DEFAULT_SEC.clone()),
        Some(0),
        Some(shard_id.clone()),
    )
    .expect("build plain send");
    let plain_sig: Bytes = plain_send.sig.clone();

    let carrier = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&plain_send))
        .await
        .expect("validator 1 proposes the carrier");
    for idx in [1usize, 2] {
        nodes[idx]
            .process_block(carrier.clone())
            .await
            .expect("carrier delivery");
    }
    assert!(
        nodes[0]
            .deploy_storage
            .lock()
            .read_all()
            .expect("storage read")
            .iter()
            .any(|d| d.sig == plain_sig),
        "staging precondition: the owner's pool holds the deploy after \
         proposing its carrier"
    );

    // Settle: nodes[2] (majority stake) ladders the floor over the
    // carrier, delivering every round to all — each accepted block runs
    // the finalization sweep on every node.
    let mut covered = false;
    for round in 0..30i32 {
        let marker = construct_deploy::basic_deploy_data(
            100 + round,
            Some(construct_deploy::DEFAULT_SEC2.clone()),
            Some(shard_id.clone()),
        )
        .expect("build settle marker");
        let b = nodes[2]
            .add_block_from_deploys(std::slice::from_ref(&marker))
            .await
            .expect("settle round");
        for (other, node) in nodes.iter_mut().enumerate() {
            if other != 2 {
                node.process_block(b.clone())
                    .await
                    .expect("settle delivery");
            }
        }
        let dag = nodes[0].casper.block_dag().await.expect("dag");
        let floor = floor_of_block(&dag, &b.block_hash, FtThreshold::from_f32_lossy(0.0))
            .await
            .expect("floor_of_block");
        if floor.hash == carrier.block_hash
            || (floor.block_number >= carrier.body.state.block_number
                && dag
                    .is_dag_ancestor(&carrier.block_hash, &floor.hash)
                    .expect("ancestor query"))
        {
            covered = true;
            break;
        }
    }
    assert!(
        covered,
        "staging precondition: the floor must come to cover the carrier \
         within the settle rounds"
    );

    // Two more delivered rounds so the eviction sweep observes the
    // covering floor on validator 1.
    for round in 0..2i32 {
        let marker = construct_deploy::basic_deploy_data(
            200 + round,
            Some(construct_deploy::DEFAULT_SEC2.clone()),
            Some(shard_id.clone()),
        )
        .expect("build post-cover marker");
        let b = nodes[2]
            .add_block_from_deploys(std::slice::from_ref(&marker))
            .await
            .expect("post-cover round");
        for (other, node) in nodes.iter_mut().enumerate() {
            if other != 2 {
                node.process_block(b.clone())
                    .await
                    .expect("post-cover delivery");
            }
        }
    }

    assert!(
        !nodes[0]
            .deploy_storage
            .lock()
            .read_all()
            .expect("storage read")
            .iter()
            .any(|d| d.sig == plain_sig),
        "the deploy's effect is settled in the floor state: the sweep must \
         release the pool copy as floor-settled, whatever the effect's \
         shape — an invisible-to-the-probe deploy held here reads as \
         unsettled forever and is only ever released by window close"
    );
}
