// Releasing the proposer's deploy-pool copy keys on the FLOOR, not on the
// finality marker.
//
// Finalization marks the new LFB and its whole indirectly-finalized ancestor
// closure, and that marker is not a statement about the floor: a marked block
// can still be excluded from every future cone if fork choice moves off its
// branch (ucc gate 38237bb7 — carrier #598 marked finalized as an ancestor of
// #599, #599 orphaned three heights later, the deploy destroyed with it).
//
// The pool copy is the ONLY recovery net for that case. An orphaned carrier
// was never merged, so no disposition record exists, nothing enters the
// rejected-deploy buffer, and the record-driven recovery machinery cannot see
// the deploy at all — orphan_reinclusion_spec is built on exactly that premise
// ("The pool still holds the deploy, and re-inclusion under a foreign parent
// is the ONLY path back into a live branch"). Releasing on the marker removes
// the premise.
//
// So the release belongs to the deploy-lifecycle register, the one component
// that re-evaluates as the floor advances: it happens when the register writes
// its write-once terminal verdict. Gating the finalization edge on the floor
// instead does not defer the release, it DROPS it — the block is already
// marked by the next round and never reappears in a `finalized_set`. Both
// halves are pinned below.

use casper::rust::finality::floor::floor_of_block;
use casper::rust::safety::clique_oracle::FtThreshold;
use casper::rust::util::construct_deploy;
use prost::bytes::Bytes;
use serial_test::serial;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

/// The threshold `TestNode` builds its shard conf with; the floor is a
/// function of it, so the test must derive the floor with the same value the
/// nodes used.
fn test_ftt() -> FtThreshold { FtThreshold::from_f32_lossy(0.0) }

fn pool_holds(node: &TestNode, sig: &Bytes) -> bool {
    node.deploy_storage
        .lock()
        .read_all()
        .expect("read deploy pool")
        .iter()
        .any(|d| d.sig == *sig)
}

/// Drive one round of empty proposals by every validator, delivering each to
/// the rest. Returns after every validator has proposed once.
async fn drive_round(nodes: &mut [TestNode]) {
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
    }
}

/// THE regression. Finality lands on a block before the floor reaches it, and
/// it sweeps in the whole indirectly-finalized ancestor closure — so a carrier
/// is marked finalized while its contents are NOT yet represented in every
/// future merge base and its branch can still be abandoned. Removing the pool
/// copy there is what turns an orphaned carrier into destroyed work.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn a_finalized_carrier_above_the_floor_keeps_its_deploy_in_the_pool() {
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
        r#"@"fe_cell"!(1)"#.to_string(),
        None,
        None,
        Some(construct_deploy::DEFAULT_SEC.clone()),
        Some(0),
        Some(shard_id.clone()),
    )
    .expect("build carrier deploy");
    let sig: Bytes = deploy.sig.clone();

    let carrier = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&deploy))
        .await
        .expect("validator 1 proposes the carrier");
    let carrier_hash = carrier.block_hash.clone();
    for (i, node) in nodes.iter_mut().enumerate() {
        if i != 0 {
            node.process_block(carrier.clone())
                .await
                .expect("deliver the carrier");
        }
    }

    assert!(
        pool_holds(&nodes[0], &sig),
        "fixture precondition: inclusion alone must not evict the pool copy — \
         the owner keeps it until the work is irreversibly settled",
    );

    // Advance one proposal at a time and stop at the FIRST observation where
    // the carrier is finalized. The carrier need not ever be the LFB itself —
    // in the production incident it was finalized INDIRECTLY, as an ancestor
    // of the new LFB, which is precisely how its deploys were swept up.
    let mut observation = None;
    'search: for _ in 0..16 {
        for proposer in 0..n_validators {
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
            let dag = nodes[0]
                .block_dag_storage
                .get_representation()
                .expect("dag representation");
            if dag.is_finalized(&carrier_hash) {
                let lfb = dag.last_finalized_block();
                let floor = floor_of_block(&dag, &nodes[0].block_store, &lfb, test_ftt())
                    .await
                    .expect("floor of the LFB");
                let floor_covers_carrier = carrier_hash == floor.hash
                    || dag
                        .is_dag_ancestor(&carrier_hash, &floor.hash)
                        .expect("ancestry");
                observation = Some(floor_covers_carrier);
                break 'search;
            }
        }
    }
    let floor_covers_carrier = observation.expect(
        "fixture precondition: the carrier must reach finality within the \
         driven rounds, or this test is not observing the window it is about",
    );
    assert!(
        !floor_covers_carrier,
        "fixture precondition: the carrier must still be ABOVE the floor at \
         the first observation of its finality — that is the window this test \
         is about, and if the floor already covers it the eviction under test \
         is the legitimate one",
    );

    assert!(
        pool_holds(&nodes[0], &sig),
        "the carrier is finalized but the floor has NOT reached it, so its \
         contents are not yet in every future merge base and its branch can \
         still be abandoned. Evicting the pool copy here destroys the deploy \
         outright: an orphaned carrier is never merged, so no rejection record \
         exists, nothing reaches the rejected-deploy buffer, and the pool copy \
         is the only path back into a live branch",
    );
}

/// The discriminator against a never-evict over-correction: once the floor
/// has climbed over the carrier, its contents ARE in every future merge base,
/// re-proposal can never be needed, and the pool copy must go.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn a_deploy_is_evicted_once_the_floor_covers_its_carrier() {
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
        r#"@"fe_cell2"!(1)"#.to_string(),
        None,
        None,
        Some(construct_deploy::DEFAULT_SEC.clone()),
        Some(0),
        Some(shard_id.clone()),
    )
    .expect("build carrier deploy");
    let sig: Bytes = deploy.sig.clone();

    let carrier = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&deploy))
        .await
        .expect("validator 1 proposes the carrier");
    for (i, node) in nodes.iter_mut().enumerate() {
        if i != 0 {
            node.process_block(carrier.clone())
                .await
                .expect("deliver the carrier");
        }
    }

    let mut evicted = false;
    for _ in 0..16 {
        drive_round(&mut nodes).await;
        if !pool_holds(&nodes[0], &sig) {
            evicted = true;
            break;
        }
    }

    assert!(
        evicted,
        "once the floor climbs past the carrier its contents are represented \
         in every future merge base, so the deploy can never need re-proposal \
         and the pool copy must be released — holding it forever would leak \
         the pool and re-admit settled work",
    );
}
