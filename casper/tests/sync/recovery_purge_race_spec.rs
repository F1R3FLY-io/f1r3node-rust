//! A block mid-dependency-recovery consumed again as a duplicate copy must
//! not be purged: the purge removes it from every retry structure at once,
//! leaving recovery to survive only on peer resends — and when those stop,
//! the block is gone for good and finality can freeze behind the affected
//! validator's stake.

use casper::rust::blocks::block_processor::OfInterestVerdict;
use casper::rust::casper::Casper;
use casper::rust::util::construct_deploy;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

#[tokio::test]
async fn a_racing_duplicate_must_not_destroy_dependency_recovery() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");

    let mut nodes = TestNode::create_network(genesis.clone(), 2, None, None, None, None)
        .await
        .unwrap();
    let shard = genesis.genesis_block.shard_id.clone();

    // node0 creates A then B; node1 misses A, so B arrives with a missing
    // dependency and enters recovery (buffered against A).
    let deploy_a = construct_deploy::basic_deploy_data(0, None, Some(shard.clone())).unwrap();
    let block_a = nodes[0].add_block_from_deploys(&[deploy_a]).await.unwrap();
    nodes[1].shutoff().unwrap();

    let deploy_b = construct_deploy::basic_deploy_data(1, None, Some(shard.clone())).unwrap();
    let block_b = nodes[0].add_block_from_deploys(&[deploy_b]).await.unwrap();

    let first = nodes[1].add_block(block_b.clone()).await.unwrap();
    assert!(
        matches!(first, Either::Left(_)),
        "B without A must not validate, got {first:?}"
    );
    assert!(
        nodes[1].casper.buffer_contains(&block_b.block_hash),
        "B must be buffered against its missing dependency"
    );

    // The race: a duplicate copy of B (a peer resend, or a pendant re-offer
    // crossing a fresh receipt) is consumed while B sits in recovery.
    let verdict = nodes[1]
        .block_processor
        .check_if_of_interest(nodes[1].casper.clone(), &block_b)
        .unwrap();
    assert_eq!(verdict, OfInterestVerdict::AlreadyProcessed);
    nodes[1]
        .block_processor
        .dispose_not_of_interest(verdict, &block_b)
        .await
        .unwrap();

    // The recovery state must survive the duplicate. Before the fix the
    // disposition purged B here — not in the DAG, not buffered, not
    // in-flight — and nothing ever re-offered it.
    assert!(
        nodes[1].casper.buffer_contains(&block_b.block_hash),
        "the duplicate copy must not purge B's recovery state"
    );

    // A lands; B must be offered back and complete.
    let a_result = nodes[1].add_block(block_a.clone()).await.unwrap();
    assert!(
        matches!(a_result, Either::Right(_)),
        "A must validate, got {a_result:?}"
    );
    let pendants = nodes[1].casper.get_dependency_free_from_buffer().unwrap();
    assert!(
        pendants.iter().any(|b| b.block_hash == block_b.block_hash),
        "B must be re-offered once its dependency lands"
    );

    let recovered = nodes[1].add_block(block_b.clone()).await.unwrap();
    assert!(
        matches!(recovered, Either::Right(_)),
        "B must validate after recovery, got {recovered:?}"
    );
    assert!(nodes[1].casper.dag_contains(&block_b.block_hash));
}
