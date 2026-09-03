// Release the proposer's deploy-pool copy only when the adopted LFB state
// contains the exact execution effect.
//
// A finality marker proves causal support. It does not prove state-effect
// membership. A candidate can contain a source block causally while its merge
// rejects that source effect. Marker-only cleanup would destroy the pool's
// recovery copy.
//
// The block that becomes the LFB also carries an older frozen floor. That
// frozen floor authenticated proposal context. It is not the current state
// anchor. Once exact provenance proves that the adopted LFB contains the
// effect, cleanup must not wait for the older frozen floor to catch up.

use casper::rust::finality::floor::floor_of_block;
use casper::rust::safety::clique_oracle::FtThreshold;
use casper::rust::util::construct_deploy;
use models::rust::casper::protocol::casper_message::StateEffectId;
use prost::bytes::Bytes;
use serial_test::serial;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

/// The threshold `TestNode` builds its shard conf with; the floor is a
/// function of it, so the test must derive the floor with the same value the
/// nodes used.
fn test_ftt() -> FtThreshold { FtThreshold::from_f32_lossy(0.0) }

fn pool_holds(node: &TestNode, deploy_id: &Bytes) -> bool {
    node.deploy_storage
        .lock()
        .contains_envelope(deploy_id)
        .expect("read deploy pool")
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

/// The adopted LFB contains the effect before the LFB block's frozen floor
/// covers its source. Exact state provenance must release the pool copy.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn an_adopted_lfb_effect_evicts_when_its_frozen_floor_lags() {
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
    let carrier = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&deploy))
        .await
        .expect("validator 1 proposes the carrier");
    let deploy_id = Bytes::copy_from_slice(carrier.body.deploys[0].deploy_id());
    let carrier_hash = carrier.block_hash.clone();
    for (i, node) in nodes.iter_mut().enumerate() {
        if i != 0 {
            node.process_block(carrier.clone())
                .await
                .expect("deliver the carrier");
        }
    }

    assert!(
        pool_holds(&nodes[0], &deploy_id),
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

    let dag = nodes[0]
        .block_dag_storage
        .get_representation()
        .expect("dag representation");
    let adopted_lfb = dag.last_finalized_block();
    let effect = StateEffectId {
        source_block_hash: carrier_hash,
        execution_index: 0,
    };
    assert!(
        block_storage::rust::finality::state_preservation::is_state_effect_active(
            &dag,
            &adopted_lfb,
            &effect,
        )
        .expect("committed effect provenance"),
        "the adopted LFB state must contain the carrier effect even when the \
         LFB block's older frozen floor does not cover the carrier",
    );
    assert!(
        !pool_holds(&nodes[0], &deploy_id),
        "the adopted LFB state contains the effect, so the lifecycle register \
         must release the pool copy without waiting for the LFB block's older \
         frozen floor",
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
    let carrier = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&deploy))
        .await
        .expect("validator 1 proposes the carrier");
    let deploy_id = Bytes::copy_from_slice(carrier.body.deploys[0].deploy_id());
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
        if !pool_holds(&nodes[0], &deploy_id) {
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
