// A Finalized verdict written against the adopted LFB survives a later merge
// that rejects the deploy over a lower base.
//
// v0 and v2 finalize the carrier A while v1 is partitioned with a conflicting
// sibling B. Once v0 has written Finalized for A's deploy, v1 receives A alone
// and proposes a merge M over its own branch. M's justifications have not
// witnessed A's finality, so its base is v1's branch and the merge rejects A's
// deploy. After the partition heals, every node's LFB must keep advancing and
// keep A's effect on its state lineage.

use block_storage::rust::dag::deploy_lifecycle_types::TerminalState;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use casper::rust::finality::floor::floor_of_block;
use casper::rust::safety::clique_oracle::FtThreshold;
use casper::rust::util::construct_deploy;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::BlockMessage;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::history::Either;
use serial_test::serial;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

fn ftt() -> FtThreshold { FtThreshold::from_ppm(100_000) }

/// The register's membership rule, over public block-store reads.
fn effect_in_state(store: &KeyValueBlockStore, from: &BlockHash, sig: &Bytes) -> bool {
    let mut cur = from.clone();
    loop {
        let block = store
            .get(&cur)
            .expect("block store read")
            .expect("lineage block held");
        if block
            .body
            .deploys
            .iter()
            .any(|pd| pd.deploy.sig == *sig && !pd.is_failed)
            || block.body.applied_from_scope.iter().any(|s| s == sig)
        {
            return true;
        }
        cur = if !block.body.merge_base.is_empty() {
            block.body.merge_base.clone()
        } else {
            match block.header.parents_hash_list.as_slice() {
                [] => return false,
                [parent] => parent.clone(),
                _ => panic!("multi-parent block without a recorded merge_base"),
            }
        };
    }
}

async fn deliver(node: &mut TestNode, log: &[BlockMessage]) {
    for block in log {
        if !node.contains(&block.block_hash) {
            let status = node
                .process_block(block.clone())
                .await
                .expect("process block");
            assert!(
                matches!(status, Either::Right(_)),
                "block not admitted: {:?}",
                status
            );
        }
    }
}

struct View {
    lfb_height: i64,
    lfb_has: bool,
    floor_has: bool,
    terminal: Option<TerminalState>,
}

async fn view(node: &TestNode, sig: &Bytes) -> View {
    let lfb = node
        .casper
        .finalize()
        .await
        .expect("finalizer cycle")
        .block_hash;
    let dag = node.block_dag_storage.get_representation().expect("dag");
    let floor = floor_of_block(&dag, &node.block_store, &lfb, ftt())
        .await
        .expect("floor of lfb");
    View {
        lfb_height: dag.block_number_unsafe(&lfb).expect("lfb height"),
        lfb_has: effect_in_state(&node.block_store, &lfb, sig),
        floor_has: effect_in_state(&node.block_store, &floor.hash, sig),
        terminal: dag
            .deploy_terminal(sig)
            .expect("terminal lookup")
            .map(|t| t.state),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn a_finalized_verdict_on_the_lfb_survives_a_low_base_merge() {
    let parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(3));
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("genesis");
    let shard_id = genesis.genesis_block.shard_id.clone();
    let mut nodes = TestNode::create_network(genesis, 3, None, None, None, None)
        .await
        .expect("create_network");
    for node in nodes.iter_mut() {
        node.allow_empty_blocks = true;
    }

    let make = |rho: &str| {
        construct_deploy::source_deploy_now_full(
            rho.to_string(),
            None,
            None,
            Some(construct_deploy::DEFAULT_SEC.clone()),
            None,
            Some(shard_id.clone()),
        )
        .expect("deploy")
    };

    // Both contenders consume the one seeded datum: a hard conflict.
    let seed_deploy = make(r#"@"low_base_cell"!(0)"#);
    let mut log: Vec<BlockMessage> = Vec::new();
    let seed = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&seed_deploy))
        .await
        .expect("v0 proposes the seed");
    log.push(seed);
    for node in nodes.iter_mut() {
        deliver(node, &log).await;
    }

    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    let deploy_a = make(r#"for (_ <- @"low_base_cell") { @"low_base_cell"!(1) }"#);
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    let deploy_b = make(r#"for (_ <- @"low_base_cell") { @"low_base_cell"!(2) }"#);
    let sig_a: Bytes = deploy_a.sig.clone();

    let a = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&deploy_a))
        .await
        .expect("v0 proposes A");
    log.push(a.clone());
    // v1 builds two blocks on B, so its own branch is the main parent of the
    // merge it later forms.
    let b = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&deploy_b))
        .await
        .expect("v1 proposes B");
    let b2 = nodes[1]
        .add_block_from_deploys(&[])
        .await
        .expect("v1 extends B");

    deliver(&mut nodes[2], &log).await;
    let mut window = None;
    'finalize: for _ in 0..16 {
        for proposer in [0usize, 2] {
            let block = nodes[proposer]
                .add_block_from_deploys(&[])
                .await
                .expect("partitioned proposal");
            log.push(block);
            deliver(&mut nodes[2 - proposer], &log).await;
            let v = view(&nodes[0], &sig_a).await;
            if v.terminal.is_some() {
                window = Some(v);
                break 'finalize;
            }
        }
    }
    let window = window.expect("v0 must write a verdict for A's deploy while v1 is partitioned");
    assert_eq!(window.terminal, Some(TerminalState::Finalized));
    assert!(
        window.lfb_has && !window.floor_has,
        "fixture precondition: the verdict must be written while A's effect is \
         on the LFB's lineage but not yet on the floor's"
    );

    deliver(&mut nodes[1], std::slice::from_ref(&a)).await;
    let m = nodes[1]
        .add_block_from_deploys(&[])
        .await
        .expect("v1 proposes M");
    assert!(
        m.body
            .rejected_deploys
            .iter()
            .any(|r| r.sig == sig_a && !r.duplicate),
        "fixture precondition: M must reject A's deploy"
    );
    log.push(b);
    log.push(b2);
    log.push(m);

    for node in nodes.iter_mut() {
        deliver(node, &log).await;
    }
    let mut healed = Vec::new();
    for node in nodes.iter() {
        healed.push(view(node, &sig_a).await.lfb_height);
    }
    for _ in 0..16 {
        for proposer in 0..3 {
            let block = nodes[proposer]
                .add_block_from_deploys(&[])
                .await
                .expect("healed proposal");
            log.push(block);
            for node in nodes.iter_mut() {
                deliver(node, &log).await;
            }
        }
        for (i, node) in nodes.iter().enumerate() {
            let v = view(node, &sig_a).await;
            assert!(
                v.lfb_has,
                "v{i}'s LFB #{} lacks the effect v0 declared Finalized",
                v.lfb_height
            );
        }
    }
    for (i, node) in nodes.iter().enumerate() {
        let v = view(node, &sig_a).await;
        assert!(
            v.lfb_height > healed[i],
            "v{i}'s LFB stalled at #{} after healing",
            v.lfb_height
        );
        assert_eq!(v.terminal, Some(TerminalState::Finalized), "v{i}");
    }
}
