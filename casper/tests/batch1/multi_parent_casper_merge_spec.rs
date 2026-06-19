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

/// Multi-parent merge must SERIALIZE concurrent writes to one single-value cell,
/// never bag them into a multi-value cell.
///
/// Two sibling blocks concurrently read-modify-write ONE shared cell off a
/// common base (`@7!(0)`): node0 adds 10, node1 adds 1. Both consume the SAME
/// base `0` datum (they are not propagated between creation). `@7` is a plain
/// user channel (NOT in the mergeable-tag registry), so it is a single-value
/// cell — it must hold exactly ONE datum.
///
/// The merge keeps ONE write (the cell becomes 10 or 1) and rejects the other to
/// recovery, where it re-executes against the merged value in a later block.
/// (That cross-round convergence to BOTH writes is verified by
/// `fs_seal_must_preserve_both_concurrent_single_value_cell_writes`.) The bug
/// this guards against is the cell going MULTI-value — `[0, 1, 10]` (bagged, the
/// content-twin shape) — which finalizes and regresses downstream. So the gate
/// is: exactly one datum, a real write, and the loser recorded as rejected.
#[tokio::test]
async fn multi_parent_merge_serializes_concurrent_single_value_cell_writes() {
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

    // Seed the shared single-value cell on a block every node sees — the common
    // base both sibling writes read-modify-write.
    let seed = construct_deploy::source_deploy_now(
        "@7!(0)".to_string(),
        None,
        None,
        Some(shard_id.clone()),
    )
    .unwrap();
    let _block_seed = nodes[0].add_block_from_deploys(&[seed]).await.unwrap();
    {
        let nodes_refs: Vec<&mut TestNode> = nodes.iter_mut().collect();
        TestNode::propagate(&mut nodes_refs.into_iter().collect::<Vec<_>>())
            .await
            .unwrap();
    }

    // Two concurrent read-modify-write siblings off the seeded base. They are
    // NOT propagated between creation, so each consumes the SAME `0` datum.
    let add_10 = construct_deploy::source_deploy_now(
        "for (@v <- @7) { @7!(v + 10) }".to_string(),
        None,
        None,
        Some(shard_id.clone()),
    )
    .unwrap();
    let add_1 = construct_deploy::source_deploy_now(
        "for (@v <- @7) { @7!(v + 1) }".to_string(),
        Some(construct_deploy::DEFAULT_SEC2.clone()),
        None,
        Some(shard_id.clone()),
    )
    .unwrap();
    let block_a = nodes[0].add_block_from_deploys(&[add_10]).await.unwrap();
    let block_b = nodes[1].add_block_from_deploys(&[add_1]).await.unwrap();
    {
        let nodes_refs: Vec<&mut TestNode> = nodes.iter_mut().collect();
        TestNode::propagate(&mut nodes_refs.into_iter().collect::<Vec<_>>())
            .await
            .unwrap();
    }

    // node2 proposes a merge block over both siblings; the noop write is on a
    // disjoint channel so the merge is non-empty but does not itself touch @7.
    let noop = construct_deploy::source_deploy_now(
        "@9!(9)".to_string(),
        None,
        None,
        Some(shard_id.clone()),
    )
    .unwrap();
    let merge_block = TestNode::propagate_block_at_index(&mut nodes, 2, &[noop])
        .await
        .unwrap();

    // The merge must include both sibling writes as parents (the multi-parent
    // conflict case this test exists to exercise).
    assert!(
        merge_block
            .header
            .parents_hash_list
            .contains(&block_a.block_hash)
            && merge_block
                .header
                .parents_hash_list
                .contains(&block_b.block_hash),
        "merge block must have both sibling writes as parents; got {:?}",
        merge_block.header.parents_hash_list
    );

    // Read the shared cell at the merged post-state.
    let cell =
        rspace_util::get_data_at_public_channel_block(&merge_block, 7, &nodes[0].runtime_manager)
            .await;

    // Single-value cell: exactly ONE datum (serialized, NOT bagged to multi-value).
    assert_eq!(
        cell.len(),
        1,
        "single-value cell @7 must hold exactly one datum after the merge (serialized), not a bag \
         of concurrent writes; got {:?}",
        cell
    );
    // The surviving value is one of the two writes — the merge applied one; it is
    // not garbage, and the un-consumed seed `0` did not survive.
    assert!(
        cell == vec!["10".to_string()] || cell == vec!["1".to_string()],
        "the serialized value must be one of the two concurrent writes (+10 -> 10 or +1 -> 1), \
         not the un-consumed seed or a bag; got {:?}",
        cell
    );
    // The losing write is NOT lost: the merge records it as rejected, which feeds
    // recovery to re-execute it on the merged value in a later block.
    assert_eq!(
        merge_block.body.rejected_deploys.len(),
        1,
        "the concurrent write that lost serialization must be recorded as rejected (-> recovery), \
         not silently dropped or bagged; rejected_deploys count = {}",
        merge_block.body.rejected_deploys.len()
    );
}

/// Companion FS-layer guard: the SEALED finalized state `FS(floor)` must durably
/// retain a concurrent single-value-cell write, never silently drop it.
///
/// Six rounds of two concurrent `set()` writes to ONE shared map cell (`@7`),
/// each merged and fully propagated, so finalization advances well past the
/// early rounds. Round 0's two writes (`Ka0`, `Kb0`) are therefore deeply
/// finalized by the end. The finalized state the node serves (`FS(floor)`, which
/// `/validators` and the default `/explore-deploy` read) must contain BOTH: a
/// concurrent write the merge dropped and recovery never re-landed is lost
/// finalized work — the multi-parent finalized-state regression this branch
/// exists to eliminate.
///
/// Unlike `single_value_cell_concurrent_writes_must_both_survive_merge` (which
/// asserts on a single merge BLOCK's post-state), this asserts on the
/// independently re-folded `FS(floor)` — the layer where `FS(floor) != the
/// finalized block's post-state` shows up.
#[tokio::test]
async fn fs_seal_must_preserve_both_concurrent_single_value_cell_writes() {
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

    // Seed the shared single-value cell with the empty map.
    let seed = construct_deploy::source_deploy_now(
        "@7!({})".to_string(),
        None,
        None,
        Some(shard_id.clone()),
    )
    .unwrap();
    nodes[0].add_block_from_deploys(&[seed]).await.unwrap();
    {
        let nodes_refs: Vec<&mut TestNode> = nodes.iter_mut().collect();
        TestNode::propagate(&mut nodes_refs.into_iter().collect::<Vec<_>>())
            .await
            .unwrap();
    }

    // Rounds of concurrent set() writes to the one cell, each merged by node2.
    let rounds = 6u32;
    let mut last_tip = None;
    for r in 0..rounds {
        let set_a = construct_deploy::source_deploy_now(
            format!("for (@m <- @7) {{ @7!(m.set(\"Ka{}\", 1)) }}", r),
            None,
            None,
            Some(shard_id.clone()),
        )
        .unwrap();
        let set_b = construct_deploy::source_deploy_now(
            format!("for (@m <- @7) {{ @7!(m.set(\"Kb{}\", 2)) }}", r),
            Some(construct_deploy::DEFAULT_SEC2.clone()),
            None,
            Some(shard_id.clone()),
        )
        .unwrap();
        // Capture sigs before the deploys are moved into the proposal calls.
        let ka_sig = hex::encode(&set_a.sig[..8]);
        let kb_sig = hex::encode(&set_b.sig[..8]);
        // Siblings: not propagated between creation, so both consume the same
        // current cell value.
        nodes[0].add_block_from_deploys(&[set_a]).await.unwrap();
        nodes[1].add_block_from_deploys(&[set_b]).await.unwrap();
        {
            let nodes_refs: Vec<&mut TestNode> = nodes.iter_mut().collect();
            TestNode::propagate(&mut nodes_refs.into_iter().collect::<Vec<_>>())
                .await
                .unwrap();
        }
        let noop = construct_deploy::source_deploy_now(
            format!("@9!({})", r),
            None,
            None,
            Some(shard_id.clone()),
        )
        .unwrap();
        let merge = TestNode::propagate_block_at_index(&mut nodes, 2, &[noop])
            .await
            .unwrap();
        // DIAG: per-round labels + decoded @7 so the recovery walkthrough can
        // track writes by name (Ka{r}/Kb{r}) and read the cell value at each
        // height instead of by raw signature.
        let cell_now =
            rspace_util::get_data_at_public_channel_block(&merge, 7, &nodes[0].runtime_manager)
                .await;
        tracing::info!(
            target: "f1r3.trace.cell",
            round = r,
            ka_sig = %ka_sig,
            kb_sig = %kb_sig,
            merge_block = %hex::encode(&merge.block_hash[..6]),
            merge_rejected = merge.body.rejected_deploys.len(),
            cell = ?cell_now,
            "round cell state: Ka{r}=ka_sig Kb{r}=kb_sig",
        );
        last_tip = Some(merge);
    }
    let tip = last_tip.expect("rounds produced a merge block");

    // The sealed finalized state at the tip's justification-derived floor.
    let dag = nodes[0].block_dag_storage.get_representation();
    let floor = floor_of_block(&dag, &tip.block_hash, FT_THRESHOLD)
        .await
        .expect("floor must resolve");
    assert_ne!(
        floor.hash, genesis.genesis_block.block_hash,
        "test needs floors past genesis; raise the round count if this fires"
    );
    let fs = floor_state_get_or_compute(
        &dag,
        &nodes[0].block_store,
        &nodes[0].runtime_manager,
        &floor.hash,
        FT_THRESHOLD,
    )
    .await
    .expect("FS(floor) must fold");

    // Read the shared cell out of the SEALED finalized state (across all datums
    // if the cell went multi-value).
    let fs_cell =
        rspace_util::get_data_at_public_channel(&fs.state_hash.0, 7, &nodes[0].runtime_manager)
            .await;
    let fs_joined = fs_cell.join(" | ");

    // The cell is single-value: the finalized state must hold exactly ONE map
    // datum, not a bag of divergent maps (multi-datum = the cell silently went
    // multi-value, a different shape of the same corruption).
    assert_eq!(
        fs_cell.len(),
        1,
        "finalized state FS(floor #{}) holds {} datums on the single-value cell @7 — it was bagged \
         to multi-value, not serialized: {:?}",
        floor.block_number,
        fs_cell.len(),
        fs_cell,
    );

    // Round 0's two concurrent writes are deeply finalized (the floor is many
    // blocks past them). The single finalized map must retain BOTH.
    assert!(
        fs_joined.contains("Ka0") && fs_joined.contains("Kb0"),
        "finalized state FS(floor #{}) dropped a concurrent single-value-cell write from round 0: \
         expected BOTH Ka0 and Kb0 present (deeply finalized after {} rounds), got {:?}. A \
         concurrent write the merge dropped and recovery never re-landed is lost finalized \
         work — the regression seal-the-base must prevent.",
        floor.block_number,
        rounds,
        fs_cell,
    );
}

/// Focused guard for the recovery base-check (`recovered_deploy_effect_in_base`).
///
/// A merge-rejected deploy may be re-proposed by recovery, but it must be
/// re-executed ONLY when its effect is not already in the execution base. Every
/// deploy allocates a sig-derived per-deploy number cell (via pre-charge); a
/// recovered deploy whose cell is already present in the base is the "flip" (kept
/// on a branch the base descends from while a sibling merge rejected it), and
/// re-executing it would re-create that cell — the content-twin. The base-check
/// must therefore return:
///   - `true`  (skip)    when the deploy's per-deploy cell IS in the base, and
///   - `false` (execute) when it is NOT (a genuine loser that must re-land).
///
/// This exercises both directions against a real executed deploy without
/// depending on the separate finalized-state convergence (`#71`) the bundled
/// `fs_seal_*` test also asserts: base = the deploy's own post-state (effect
/// present) and base = the pre-state it built on (effect absent).
#[tokio::test]
async fn recovery_base_check_skips_only_when_effect_is_in_base() {
    use casper::rust::util::rholang::interpreter_util::recovered_deploy_effect_in_base;
    use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

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

    // Any user deploy is pre-charged, so it allocates its own sig-derived number
    // cell — the per-deploy cell the base-check keys on.
    let deploy = construct_deploy::source_deploy_now(
        "@7!(42)".to_string(),
        None,
        None,
        Some(shard_id.clone()),
    )
    .unwrap();
    let sig = deploy.sig.clone();

    let block = nodes[0].add_block_from_deploys(&[deploy]).await.unwrap();

    let dag = nodes[0].block_dag_storage.get_representation();
    let block_post = Blake2b256Hash::from_bytes_prost(&block.body.state.post_state_hash);
    let genesis_post =
        Blake2b256Hash::from_bytes_prost(&genesis.genesis_block.body.state.post_state_hash);

    // base = the deploy's own post-state: its cell is present -> skip (true).
    let in_own_post = recovered_deploy_effect_in_base(
        &dag,
        &nodes[0].block_store,
        &nodes[0].runtime_manager,
        &block_post,
        &sig,
    )
    .expect("base-check must not error on a healthy block");
    assert!(
        in_own_post,
        "base-check must return true when the deploy's per-deploy cell is present in \
         the base (re-executing would content-twin it)"
    );

    // base = the pre-state the deploy built on (genesis): its cell is absent ->
    // execute (false). This is the genuine-loser case recovery must re-land.
    let in_genesis = recovered_deploy_effect_in_base(
        &dag,
        &nodes[0].block_store,
        &nodes[0].runtime_manager,
        &genesis_post,
        &sig,
    )
    .expect("base-check must not error against genesis state");
    assert!(
        !in_genesis,
        "base-check must return false when the deploy's effect is absent from the base \
         (a genuine merge loser that must re-land)"
    );
}
