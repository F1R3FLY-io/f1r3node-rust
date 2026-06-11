// See casper/src/test/scala/coop/rchain/casper/batch1/MultiParentCasperMergeSpec.scala

use casper::rust::block_status::ValidBlock;
use casper::rust::util::{construct_deploy, rspace_util};
use rspace_plus_plus::rspace::history::Either;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

#[tokio::test]
async fn hash_set_casper_should_handle_multi_parent_blocks_correctly() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(
            GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(3)),
        ))
        .await
        .expect("Failed to build genesis");

    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .unwrap();

    let shard_id = genesis.genesis_block.shard_id.clone();

    let deploy_data0 = construct_deploy::basic_deploy_data(
        0,
        Some(construct_deploy::DEFAULT_SEC2.clone()),
        Some(shard_id.clone()),
    )
    .unwrap();

    let deploy_data1 = construct_deploy::source_deploy_now(
        "@1!(1) | for(@x <- @1){ @1!(x) }".to_string(),
        None,
        None,
        Some(shard_id.clone()),
    )
    .unwrap();

    let deploy_data2 =
        construct_deploy::basic_deploy_data(2, None, Some(shard_id.clone())).unwrap();

    let deploys = vec![deploy_data0, deploy_data1, deploy_data2];

    let block0 = nodes[0]
        .add_block_from_deploys(&[deploys[0].clone()])
        .await
        .unwrap();

    let block1 = nodes[1]
        .add_block_from_deploys(&[deploys[1].clone()])
        .await
        .unwrap();

    let nodes_refs: Vec<&mut TestNode> = nodes.iter_mut().collect();
    TestNode::propagate(&mut nodes_refs.into_iter().collect::<Vec<_>>())
        .await
        .unwrap();

    assert!(nodes[0]
        .block_dag_storage
        .get_representation()
        .is_finalized(&genesis.genesis_block.block_hash));
    assert!(!nodes[0]
        .block_dag_storage
        .get_representation()
        .is_finalized(&block0.block_hash));
    assert!(!nodes[0]
        .block_dag_storage
        .get_representation()
        .is_finalized(&block1.block_hash));

    //multiparent block joining block0 and block1 since they do not conflict
    let multiparent_block = {
        let (node0_slice, rest) = nodes.split_at_mut(1);
        let mut nodes_for_propagate: Vec<&mut TestNode> = rest.iter_mut().collect();
        node0_slice[0]
            .propagate_block(&[deploys[2].clone()], &mut nodes_for_propagate)
            .await
            .unwrap()
    };

    assert_eq!(block0.header.parents_hash_list, vec![genesis
        .genesis_block
        .block_hash
        .clone()]);
    assert_eq!(block1.header.parents_hash_list, vec![genesis
        .genesis_block
        .block_hash
        .clone()]);
    // With multi-parent merging, all validators' latest blocks are included as parents
    // (block0 from node0, block1 from node1, genesis from node2 who hasn't created a block yet)
    assert_eq!(multiparent_block.header.parents_hash_list.len(), 3);
    assert!(nodes[0].contains(&multiparent_block.block_hash));
    assert!(nodes[1].contains(&multiparent_block.block_hash));
    assert_eq!(multiparent_block.body.rejected_deploys.len(), 0);

    let data0 = rspace_util::get_data_at_public_channel_block(
        &multiparent_block,
        0,
        &nodes[0].runtime_manager,
    )
    .await;
    assert_eq!(data0, vec!["0"]);

    let data1 = rspace_util::get_data_at_public_channel_block(
        &multiparent_block,
        1,
        &nodes[1].runtime_manager,
    )
    .await;
    assert_eq!(data1, vec!["1"]);

    let data2 = rspace_util::get_data_at_public_channel_block(
        &multiparent_block,
        2,
        &nodes[0].runtime_manager,
    )
    .await;
    assert_eq!(data2, vec!["2"]);
}

#[tokio::test]
async fn hash_set_casper_should_not_produce_unused_comm_event_while_merging_non_conflicting_blocks_in_the_presence_of_conflicting_ones(
) {
    let registry_rho = r#"
// Expected output
//
// "REGISTRY_SIMPLE_INSERT_TEST: create arbitrary process X to store in the registry"
// Unforgeable(0xd3f4cbdcc634e7d6f8edb05689395fef7e190f68fe3a2712e2a9bbe21eb6dd10)
// "REGISTRY_SIMPLE_INSERT_TEST: adding X to the registry and getting back a new identifier"
// `rho:id:pnrunpy1yntnsi63hm9pmbg8m1h1h9spyn7zrbh1mcf6pcsdunxcci`
// "REGISTRY_SIMPLE_INSERT_TEST: got an identifier for X from the registry"
// "REGISTRY_SIMPLE_LOOKUP_TEST: looking up X in the registry using identifier"
// "REGISTRY_SIMPLE_LOOKUP_TEST: got X from the registry using identifier"
// Unforgeable(0xd3f4cbdcc634e7d6f8edb05689395fef7e190f68fe3a2712e2a9bbe21eb6dd10)

new simpleInsertTest, simpleInsertTestReturnID, simpleLookupTest,
    signedInsertTest, signedInsertTestReturnID, signedLookupTest,
    ri(`rho:registry:insertArbitrary`),
    rl(`rho:registry:lookup`),
    stdout(`rho:io:stdout`),
    stdoutAck(`rho:io:stdoutAck`), ack in {
        simpleInsertTest!(*simpleInsertTestReturnID) |
        for(@idFromTest1 <- simpleInsertTestReturnID) {
            simpleLookupTest!(idFromTest1, *ack)
        } |

        contract simpleInsertTest(registryIdentifier) = {
            stdout!("REGISTRY_SIMPLE_INSERT_TEST: create arbitrary process X to store in the registry") |
            new X, Y, innerAck in {
                stdoutAck!(*X, *innerAck) |
                for(_ <- innerAck){
                    stdout!("REGISTRY_SIMPLE_INSERT_TEST: adding X to the registry and getting back a new identifier") |
                    ri!(*X, *Y) |
                    for(@uri <- Y) {
                        stdout!("REGISTRY_SIMPLE_INSERT_TEST: got an identifier for X from the registry") |
                        stdout!(uri) |
                        registryIdentifier!(uri)
                    }
                }
            }
        } |

        contract simpleLookupTest(@uri, result) = {
            stdout!("REGISTRY_SIMPLE_LOOKUP_TEST: looking up X in the registry using identifier") |
            new lookupResponse in {
                rl!(uri, *lookupResponse) |
                for(@val <- lookupResponse) {
                    stdout!("REGISTRY_SIMPLE_LOOKUP_TEST: got X from the registry using identifier") |
                    stdoutAck!(val, *result)
                }
            }
        }
    }
"#;

    let tuples_rho = r#"
// tuples only support random access
new stdout(`rho:io:stdout`) in {

  // prints 2 because tuples are 0-indexed
  stdout!((1,2,3).nth(1))
}
"#;

    let time_rho = r#"
new getBlockData(`rho:block:data`), stdout(`rho:io:stdout`), tCh in {
  getBlockData!(*tCh) |
  for(@_, @t, @_ <- tCh) {
    match t {
      Nil => { stdout!("no block time; no blocks yet? Not connected to Casper network?") }
      _ => { stdout!({"block time": t}) }
    }
  }
}
"#;

    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(
            GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(3)),
        ))
        .await
        .expect("Failed to build genesis");

    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .unwrap();

    let shard_id = genesis.genesis_block.shard_id.clone();

    let short = construct_deploy::source_deploy(
        "new x in { x!(0) }".to_string(),
        1,
        None,
        None,
        None,
        None,
        Some(shard_id.clone()),
    )
    .unwrap();

    let time = construct_deploy::source_deploy(
        time_rho.to_string(),
        3,
        None,
        None,
        None,
        None,
        Some(shard_id.clone()),
    )
    .unwrap();

    let tuples = construct_deploy::source_deploy(
        tuples_rho.to_string(),
        2,
        None,
        None,
        None,
        None,
        Some(shard_id.clone()),
    )
    .unwrap();

    let reg = construct_deploy::source_deploy(
        registry_rho.to_string(),
        4,
        None,
        None,
        None,
        None,
        Some(shard_id.clone()),
    )
    .unwrap();

    let _b1n3 = nodes[2].add_block_from_deploys(&[short]).await.unwrap();

    let _b1n2 = nodes[1].add_block_from_deploys(&[time]).await.unwrap();

    let _b1n1 = nodes[0].add_block_from_deploys(&[tuples]).await.unwrap();

    nodes[1].handle_receive().await.unwrap();

    let _b2n2 = nodes[1].create_block(&[reg]).await.unwrap();
}

#[tokio::test]
#[ignore = "Scala ignore"]
async fn hash_set_casper_should_not_merge_blocks_that_touch_the_same_channel_involving_joins() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(
            GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(3)),
        ))
        .await
        .expect("Failed to build genesis");

    let mut nodes = TestNode::create_network(genesis.clone(), 2, None, None, None, None)
        .await
        .unwrap();

    let shard_id = genesis.genesis_block.shard_id.clone();

    let deploy0 = construct_deploy::source_deploy(
        "@1!(47)".to_string(),
        1,
        None,
        None,
        Some(construct_deploy::DEFAULT_SEC2.clone()),
        None,
        Some(shard_id.clone()),
    )
    .unwrap();

    let deploy1 = construct_deploy::source_deploy(
        "for(@x <- @1 & @y <- @2){ @1!(x) }".to_string(),
        2,
        None,
        None,
        None,
        None,
        Some(shard_id.clone()),
    )
    .unwrap();

    let deploy2 = construct_deploy::basic_deploy_data(2, None, Some(shard_id.clone())).unwrap();

    let deploys = vec![deploy0, deploy1, deploy2];

    let _block0 = nodes[0]
        .add_block_from_deploys(&[deploys[0].clone()])
        .await
        .unwrap();

    let _block1 = nodes[1]
        .add_block_from_deploys(&[deploys[1].clone()])
        .await
        .unwrap();

    let nodes_refs: Vec<&mut TestNode> = nodes.iter_mut().collect();
    TestNode::propagate(&mut nodes_refs.into_iter().collect::<Vec<_>>())
        .await
        .unwrap();

    let single_parent_block = nodes[0]
        .add_block_from_deploys(&[deploys[2].clone()])
        .await
        .unwrap();

    nodes[1].handle_receive().await.unwrap();

    assert_eq!(single_parent_block.header.parents_hash_list.len(), 1);
    assert!(nodes[0].contains(&single_parent_block.block_hash));
    assert!(nodes[1].knows_about(&single_parent_block.block_hash));
}

/// This test verifies the determinism fix for LCA computation and merge ordering.
/// Before the fix, validators could compute different post-states for the same
/// merge block due to non-deterministic ancestor traversal (isFinalized boundary)
/// and ordering (hashCode, Set.head). This test creates a multi-round scenario
/// where each round forces a multi-parent merge and verifies all nodes agree.
#[tokio::test]
async fn hash_set_casper_should_compute_identical_post_states_across_validators_for_merge_blocks() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(
            GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(3)),
        ))
        .await
        .expect("Failed to build genesis");

    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .unwrap();

    let shard_id = genesis.genesis_block.shard_id.clone();

    // Round 1: Create divergent blocks on two validators
    let d0 = construct_deploy::source_deploy_now(
        "@10!(1)".to_string(),
        None,
        None,
        Some(shard_id.clone()),
    )
    .unwrap();

    let d1 = construct_deploy::source_deploy_now(
        "@20!(2)".to_string(),
        Some(construct_deploy::DEFAULT_SEC2.clone()),
        None,
        Some(shard_id.clone()),
    )
    .unwrap();

    let _b0 = nodes[0].add_block_from_deploys(&[d0]).await.unwrap();
    let _b1 = nodes[1].add_block_from_deploys(&[d1]).await.unwrap();

    let nodes_refs: Vec<&mut TestNode> = nodes.iter_mut().collect();
    TestNode::propagate(&mut nodes_refs.into_iter().collect::<Vec<_>>())
        .await
        .unwrap();

    // Merge block from node2 -- must have both b0 and b1 as parents
    let d2 = construct_deploy::source_deploy_now(
        "@30!(3)".to_string(),
        None,
        None,
        Some(shard_id.clone()),
    )
    .unwrap();

    let merge_block = TestNode::propagate_block_at_index(&mut nodes, 2, &[d2])
        .await
        .unwrap();

    // All validators must have the same post-state for the merge block
    assert!(nodes[0].contains(&merge_block.block_hash));
    assert!(nodes[1].contains(&merge_block.block_hash));
    assert!(nodes[2].contains(&merge_block.block_hash));

    // Round 2: Another round of divergent blocks + merge
    let d3 = construct_deploy::source_deploy_now(
        "@40!(4)".to_string(),
        None,
        None,
        Some(shard_id.clone()),
    )
    .unwrap();

    let d4 = construct_deploy::source_deploy_now(
        "@50!(5)".to_string(),
        Some(construct_deploy::DEFAULT_SEC2.clone()),
        None,
        Some(shard_id.clone()),
    )
    .unwrap();

    let _b3 = nodes[0].add_block_from_deploys(&[d3]).await.unwrap();
    let _b4 = nodes[1].add_block_from_deploys(&[d4]).await.unwrap();

    let nodes_refs: Vec<&mut TestNode> = nodes.iter_mut().collect();
    TestNode::propagate(&mut nodes_refs.into_iter().collect::<Vec<_>>())
        .await
        .unwrap();

    let d5 = construct_deploy::source_deploy_now(
        "@60!(6)".to_string(),
        None,
        None,
        Some(shard_id.clone()),
    )
    .unwrap();

    let merge_block2 = TestNode::propagate_block_at_index(&mut nodes, 2, &[d5])
        .await
        .unwrap();

    assert!(nodes[0].contains(&merge_block2.block_hash));
    assert!(nodes[1].contains(&merge_block2.block_hash));
    assert!(nodes[2].contains(&merge_block2.block_hash));

    // Verify no deploys were rejected (non-conflicting channels)
    assert_eq!(merge_block.body.rejected_deploys.len(), 0);
    assert_eq!(merge_block2.body.rejected_deploys.len(), 0);
}

/// Regression test for the InvalidBondsCache bug.
/// Scenario: Two validators have the same DAG structure but different finalization
/// states. With the old code (isFinalized-bounded ancestor traversal), they would
/// compute different ancestor sets, different LCAs, and different post-state hashes
/// for the same block -- causing the receiving validator to reject the block with
/// InvalidBondsCache. With the Phase 1 fix (allAncestors), finalization state is
/// irrelevant to the merge computation, so both validators accept the block.
#[tokio::test]
async fn hash_set_casper_should_produce_identical_merge_results_regardless_of_finalization_state_divergence(
) {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(
            GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(3)),
        ))
        .await
        .expect("Failed to build genesis");

    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .unwrap();

    let shard_id = genesis.genesis_block.shard_id.clone();

    // Create divergent blocks on two validators
    let d0 = construct_deploy::source_deploy_now(
        "@100!(1)".to_string(),
        None,
        None,
        Some(shard_id.clone()),
    )
    .unwrap();

    let d1 = construct_deploy::source_deploy_now(
        "@200!(2)".to_string(),
        Some(construct_deploy::DEFAULT_SEC2.clone()),
        None,
        Some(shard_id.clone()),
    )
    .unwrap();

    let block0 = nodes[0].add_block_from_deploys(&[d0]).await.unwrap();
    let block1 = nodes[1].add_block_from_deploys(&[d1]).await.unwrap();

    let nodes_refs: Vec<&mut TestNode> = nodes.iter_mut().collect();
    TestNode::propagate(&mut nodes_refs.into_iter().collect::<Vec<_>>())
        .await
        .unwrap();

    // All nodes have the same DAG: genesis -> {block0, block1}
    assert!(nodes[0].contains(&block0.block_hash));
    assert!(nodes[0].contains(&block1.block_hash));
    assert!(nodes[1].contains(&block0.block_hash));
    assert!(nodes[1].contains(&block1.block_hash));

    // Advance finalization on node0 to block0 (node1 does NOT finalize block0)
    nodes[0]
        .block_dag_storage
        .record_directly_finalized(block0.block_hash.clone(), 1.0, |_| async { Ok(()) })
        .await
        .unwrap();

    // Verify divergent finalization state
    assert!(nodes[0]
        .block_dag_storage
        .get_representation()
        .is_finalized(&block0.block_hash));
    assert!(!nodes[1]
        .block_dag_storage
        .get_representation()
        .is_finalized(&block0.block_hash));

    // Node2 creates a merge block (node2 has NOT finalized block0 either)
    let d2 = construct_deploy::source_deploy_now(
        "@300!(3)".to_string(),
        None,
        None,
        Some(shard_id.clone()),
    )
    .unwrap();

    let merge_block = nodes[2].create_block_unsafe(&[d2]).await.unwrap();

    // Process merge block on node2 (self-validate, no finalization advance)
    let status2 = nodes[2].process_block(merge_block.clone()).await.unwrap();
    assert_eq!(status2, Either::Right(ValidBlock::Valid));

    // Process the same merge block on node0 (HAS finalized block0)
    // With the old code, this would fail with InvalidBondsCache because node0's
    // finalization-bounded ancestor traversal would produce a different LCA.
    let status0 = nodes[0].process_block(merge_block.clone()).await.unwrap();
    assert_eq!(status0, Either::Right(ValidBlock::Valid));

    // Process the same merge block on node1 (has NOT finalized block0)
    let status1 = nodes[1].process_block(merge_block.clone()).await.unwrap();
    assert_eq!(status1, Either::Right(ValidBlock::Valid));
}

/// The canonical floor-state recursion is a pure function of the cut: the
/// sealed state for a floor must be bit-identical whether it is folded cold
/// from genesis in one call, folded via a pre-warmed intermediate floor, or
/// computed on a different node's storage from the same propagated DAG. This
/// is the property whose absence (seal vs read-path folding different chains)
/// was the verified FS path-dependence root cause in the prior experiment.
#[tokio::test]
async fn fs_floor_state_is_path_independent_and_cross_node_identical() {
    use casper::rust::finality::floor::floor_of_block;
    use casper::rust::finality::floor_seal::floor_state_get_or_compute;

    const FT_THRESHOLD: f32 = 0.1;

    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(
            GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(3)),
        ))
        .await
        .expect("Failed to build genesis");

    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .unwrap();
    let shard_id = genesis.genesis_block.shard_id.clone();

    // Three rounds of divergent blocks + a merge block, with full propagation,
    // so justification-derived floors advance well past genesis.
    let mut last_merge_block = None;
    for round in 0..3u32 {
        let da = construct_deploy::source_deploy_now(
            format!("@{}!({})", 100 + round * 10, round),
            None,
            None,
            Some(shard_id.clone()),
        )
        .unwrap();
        let db = construct_deploy::source_deploy_now(
            format!("@{}!({})", 200 + round * 10, round),
            Some(construct_deploy::DEFAULT_SEC2.clone()),
            None,
            Some(shard_id.clone()),
        )
        .unwrap();
        let _ba = nodes[0].add_block_from_deploys(&[da]).await.unwrap();
        let _bb = nodes[1].add_block_from_deploys(&[db]).await.unwrap();
        let nodes_refs: Vec<&mut TestNode> = nodes.iter_mut().collect();
        TestNode::propagate(&mut nodes_refs.into_iter().collect::<Vec<_>>())
            .await
            .unwrap();

        let dm = construct_deploy::source_deploy_now(
            format!("@{}!({})", 300 + round * 10, round),
            None,
            None,
            Some(shard_id.clone()),
        )
        .unwrap();
        let merge_block = TestNode::propagate_block_at_index(&mut nodes, 2, &[dm])
            .await
            .unwrap();
        last_merge_block = Some(merge_block);
    }
    let tip = last_merge_block.expect("three rounds produced a merge block");

    // The floor of the tip must be identical across nodes (it is derived from
    // the tip's own justifications, not from node-local state).
    let dag2 = nodes[2].block_dag_storage.get_representation();
    let dag0 = nodes[0].block_dag_storage.get_representation();
    let floor2 = floor_of_block(&dag2, &tip.block_hash, FT_THRESHOLD)
        .await
        .expect("floor must resolve on node2");
    let floor0 = floor_of_block(&dag0, &tip.block_hash, FT_THRESHOLD)
        .await
        .expect("floor must resolve on node0");
    assert_eq!(
        floor2, floor0,
        "justification-derived floor must be node-identical"
    );
    assert_ne!(
        floor2.hash, genesis.genesis_block.block_hash,
        "test needs floors past genesis; raise the round count if this fires"
    );

    // Path A (node2): cold fold, genesis up, in one call.
    let fs_cold = floor_state_get_or_compute(
        &dag2,
        &nodes[2].block_store,
        &nodes[2].runtime_manager,
        &floor2.hash,
        FT_THRESHOLD,
    )
    .await
    .expect("cold floor-state fold must succeed");

    // Path B (node0): pre-warm the intermediate floor, then resolve the target —
    // the fold starts from a different stored base.
    let mid = floor_of_block(&dag0, &floor0.hash, FT_THRESHOLD)
        .await
        .expect("intermediate floor must resolve");
    let _fs_mid = floor_state_get_or_compute(
        &dag0,
        &nodes[0].block_store,
        &nodes[0].runtime_manager,
        &mid.hash,
        FT_THRESHOLD,
    )
    .await
    .expect("intermediate floor-state must compute");
    let fs_warm = floor_state_get_or_compute(
        &dag0,
        &nodes[0].block_store,
        &nodes[0].runtime_manager,
        &floor0.hash,
        FT_THRESHOLD,
    )
    .await
    .expect("warm floor-state fold must succeed");

    assert_eq!(
        fs_cold.state_hash, fs_warm.state_hash,
        "FS(floor) must be bit-identical regardless of fold path and node"
    );
    assert_eq!(
        fs_cold.rejected_deploys, fs_warm.rejected_deploys,
        "sealed rejection decisions must be identical regardless of fold path and node"
    );

    // Store hit must return the same value the fold produced.
    let fs_again = floor_state_get_or_compute(
        &dag2,
        &nodes[2].block_store,
        &nodes[2].runtime_manager,
        &floor2.hash,
        FT_THRESHOLD,
    )
    .await
    .expect("store hit must succeed");
    assert_eq!(fs_cold, fs_again, "store hit must equal the folded value");
}
