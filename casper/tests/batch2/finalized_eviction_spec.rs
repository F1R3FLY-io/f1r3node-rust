// The proposer's deploy-pool copy is released when the finalizer writes the
// deploy's terminal verdict, which happens on the advance that puts the
// carrier's effect on the adopted LFB's state lineage. Inclusion alone never
// releases it, and the derived floor, which trails the LFB, is not waited on.

use casper::rust::finality::floor::floor_of_block;
use casper::rust::safety::clique_oracle::FtThreshold;
use casper::rust::util::construct_deploy;
use crypto::rust::signatures::signed::Signed;
use models::rust::casper::protocol::casper_message::DeployData;
use prost::bytes::Bytes;
use serial_test::serial;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

/// The threshold `TestNode` builds its shard conf with; the floor is a
/// function of it, so the test must derive the floor with the same value the
/// nodes used.
fn test_ftt() -> FtThreshold { FtThreshold::from_ppm(100_000) }

fn pool_holds(node: &TestNode, sig: &Bytes) -> bool {
    node.deploy_storage
        .lock()
        .read_all()
        .expect("read deploy pool")
        .iter()
        .any(|d| d.sig == *sig)
}

async fn network_with_carrier(cell: &str) -> (Vec<TestNode>, Signed<DeployData>) {
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

    let deploy = construct_deploy::source_deploy_now_full(
        format!(r#"@"{cell}"!(1)"#),
        None,
        None,
        Some(construct_deploy::DEFAULT_SEC.clone()),
        Some(0),
        Some(shard_id),
    )
    .expect("build carrier deploy");

    let carrier = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&deploy))
        .await
        .expect("validator 1 proposes the carrier");
    for node in nodes.iter_mut().skip(1) {
        node.process_block(carrier.clone())
            .await
            .expect("deliver the carrier");
    }
    (nodes, deploy)
}

/// Advance one proposal at a time and stop at the first finalizer cycle that
/// leaves the carrier finalized. Returns whether the derived floor of the LFB
/// covered the carrier at that moment.
async fn finalize_until_carrier_is_finalized(nodes: &mut [TestNode], deploy_sig: &Bytes) -> bool {
    for _ in 0..16 {
        for proposer in 0..nodes.len() {
            let block = nodes[proposer]
                .add_block_from_deploys(&[])
                .await
                .expect("empty proposal");
            for (i, node) in nodes.iter_mut().enumerate() {
                if i != proposer {
                    node.process_block(block.clone())
                        .await
                        .expect("deliver empty proposal");
                }
            }
            let lfb = nodes[0].casper.finalize().await.expect("finalizer cycle");
            let dag = nodes[0]
                .block_dag_storage
                .get_representation()
                .expect("dag representation");
            let carrier_hash = dag
                .deploy_canonical_appearance(deploy_sig)
                .expect("appearance")
                .expect("the carrier is indexed");
            if dag.is_finalized(&carrier_hash) {
                let floor =
                    floor_of_block(&dag, &nodes[0].block_store, &lfb.block_hash, test_ftt())
                        .await
                        .expect("floor of the LFB");
                return carrier_hash == floor.hash
                    || dag
                        .is_dag_ancestor(&carrier_hash, &floor.hash)
                        .expect("ancestry");
            }
        }
    }
    panic!(
        "fixture precondition: the carrier must reach finality within the driven \
         rounds, or this test is not observing the window it is about"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn a_finalized_carrier_releases_its_pool_copy_before_floor_coverage() {
    let (mut nodes, deploy) = network_with_carrier("fe_cell").await;
    let sig: Bytes = deploy.sig.clone();

    assert!(
        pool_holds(&nodes[0], &sig),
        "inclusion alone must not release the pool copy: the owner keeps it \
         until the deploy has a terminal verdict",
    );

    let floor_covers_carrier = finalize_until_carrier_is_finalized(&mut nodes, &sig).await;
    assert!(
        !floor_covers_carrier,
        "fixture precondition: the carrier must still be ABOVE the floor when \
         it first finalizes — that is the window this test is about",
    );
    assert!(
        !pool_holds(&nodes[0], &sig),
        "the carrier's effect is on the adopted LFB's lineage, so the cycle \
         that finalized it wrote the verdict and released the pool copy — it \
         must not wait for the floor to climb over the carrier",
    );
}

/// The release keeps no state: a pool copy that outlived its verdict (a
/// failed removal, or a crash between the verdict write and the release) is
/// removed by the next cycle, even one that does not advance the LFB.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn a_pool_copy_left_behind_its_verdict_is_released_by_the_next_cycle() {
    let (mut nodes, deploy) = network_with_carrier("fe_cell2").await;
    let sig: Bytes = deploy.sig.clone();

    finalize_until_carrier_is_finalized(&mut nodes, &sig).await;
    assert!(
        !pool_holds(&nodes[0], &sig),
        "fixture precondition: released"
    );

    nodes[0]
        .deploy_storage
        .lock()
        .add(vec![deploy])
        .expect("leave a copy behind");
    assert!(pool_holds(&nodes[0], &sig));

    nodes[0].casper.finalize().await.expect("finalizer cycle");
    assert!(
        !pool_holds(&nodes[0], &sig),
        "the next cycle must release a copy whose sig already has a terminal verdict"
    );
}
