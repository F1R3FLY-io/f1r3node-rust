use casper::rust::casper::{Casper, MultiParentCasper};
use casper::rust::util::construct_deploy;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

/// Bonded validators that have never proposed carry the genesis hash in their
/// latest-message slot. Once this node has a block of its own, the snapshot's
/// parent must be that block — a placeholder slot must not put genesis beside
/// it. Genesis is height 0, and a height-0 candidate bounds every ancestry
/// walk at zero, which on a restored node reaches below the restore horizon.
#[tokio::test]
async fn a_snapshot_never_cites_a_genesis_placeholder_as_a_parent() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("build genesis");
    let genesis_hash = genesis.genesis_block.block_hash.clone();
    let shard_id = genesis.genesis_block.shard_id.clone();

    let mut node = TestNode::standalone(genesis)
        .await
        .expect("standalone node");

    let dag = node.casper.block_dag().await.expect("dag");
    let placeholder_slots = dag
        .latest_message_hashes()
        .iter()
        .filter(|(_, hash)| **hash == genesis_hash)
        .count();
    assert!(
        placeholder_slots > 0,
        "fixture must seed a placeholder for a bonded validator with no blocks"
    );

    let deploy = construct_deploy::basic_deploy_data(0, None, Some(shard_id)).expect("deploy");
    let own = node
        .add_block_from_deploys(&[deploy])
        .await
        .expect("propose one block");

    let snapshot = node.casper.get_snapshot().await.expect("snapshot");
    let parents: Vec<_> = snapshot
        .parents
        .iter()
        .map(|parent| parent.block_hash.clone())
        .collect();

    assert!(
        parents.contains(&own.block_hash),
        "this node's own block is a parent; got {parents:?}"
    );
    assert!(
        !parents.contains(&genesis_hash),
        "the placeholder slots must be abstained: genesis is not a parent once \
         a real one exists; got {parents:?}"
    );

    // The estimator feeds the read API, which takes the first tip off a set:
    // a placeholder there roots `show_main_chain` at height 0.
    let mut dag = node.casper.block_dag().await.expect("dag");
    let tips = node.casper.estimator(&mut dag).await.expect("estimator");
    assert_eq!(
        tips,
        vec![own.block_hash],
        "the estimator's tips are the blocks validators signed; got {tips:?}"
    );
}

#[tokio::test]
async fn snapshots_handle_withdrawal_gc_and_rebonding_with_the_same_key() {
    use casper::rust::finality::floor::{active_latest_messages, floor_committee, floor_of_block};
    use rholang::rust::interpreter::util::vault_address::VaultAddress;
    use rspace_plus_plus::rspace::history::Either;

    let mut parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    parameters.2.proof_of_stake.epoch_length = 2;
    parameters.2.proof_of_stake.deploy_lifespan = 4;
    parameters.2.proof_of_stake.max_parent_depth = 3;
    let addresses: Vec<_> = parameters
        .2
        .proof_of_stake
        .validators
        .iter()
        .map(|validator| VaultAddress::from_public_key(&validator.pk).unwrap())
        .collect();
    for vault in &mut parameters.2.vaults {
        if addresses.contains(&vault.vault_address) {
            vault.initial_balance = 1_000_000u64.into();
        }
    }
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .unwrap();
    let shard_id = genesis.genesis_block.shard_id.clone();
    let mut nodes = TestNode::create_network_with_overrides(
        genesis,
        4,
        crate::helper::test_node::ShardOverrides {
            synchrony_constraint_threshold: Some(0.0),
            finalization_rate: Some(0),
            max_parent_depth: Some(3),
            deploy_lifespan: Some(4),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    for node in &mut nodes {
        node.allow_empty_blocks = true;
    }
    let leaving = nodes[0].validator_id_opt.as_ref().unwrap().clone();
    let leaving_key = prost::bytes::Bytes::copy_from_slice(&leaving.public_key.bytes);
    let withdraw = construct_deploy::source_deploy(
        r#"new rl(`rho:registry:lookup`), posCh, deployerId(`rho:system:deployerId`), ret in {
            rl!(`rho:system:pos`, *posCh) |
            for (@(_, pos) <- posCh) { @pos!("withdraw", *deployerId, *ret) }
        }"#
        .to_string(),
        1,
        Some(500_000),
        None,
        Some(leaving.private_key.clone()),
        None,
        Some(shard_id.clone()),
    )
    .unwrap();
    let last_leaving = nodes[0].create_block_unsafe(&[withdraw]).await.unwrap();
    assert_eq!(last_leaving.body.deploys.len(), 1);
    assert!(!last_leaving.body.deploys[0].is_failed);
    for node in &mut nodes {
        assert!(matches!(
            node.add_block(last_leaving.clone()).await.unwrap(),
            Either::Right(_)
        ));
    }

    let mut retired = false;
    for round in 0..60 {
        let block = nodes[1 + round % 3].create_block_unsafe(&[]).await.unwrap();
        for node in &mut nodes {
            assert!(matches!(
                node.add_block(block.clone()).await.unwrap(),
                Either::Right(_)
            ));
            node.casper.finalize().await.unwrap();
        }
        let dag = nodes[1].casper.block_dag().await.unwrap();
        let floor = floor_of_block(
            &dag,
            &nodes[1].block_store,
            &dag.last_finalized_block(),
            casper::rust::safety::clique_oracle::FtThreshold::from_ppm(
                nodes[1]
                    .casper
                    .casper_shard_conf()
                    .fault_tolerance_threshold_ppm,
            ),
        )
        .await
        .unwrap();
        let floor_block = nodes[1].block_store.get(&floor.hash).unwrap().unwrap();
        let committee = floor_committee(
            &nodes[1].runtime_manager,
            &floor_block.body.state.post_state_hash,
        )
        .await
        .unwrap();
        let conf = nodes[1].casper.casper_shard_conf();
        let retention_depth = i64::from(conf.max_parent_depth)
            + i64::from(conf.mergeable_channels_gc_depth_buffer)
            + conf.deploy_lifespan;
        let active_latest = active_latest_messages(
            &dag,
            &nodes[1].block_store,
            &nodes[1].casper.runtime_manager,
            &floor,
        )
        .await
        .unwrap();
        let mut oldest_floor = floor.block_number;
        for hash in active_latest.values() {
            if let Some(cached) = dag.get_cached_floor(hash).unwrap() {
                oldest_floor = oldest_floor.min(dag.lookup_unsafe(&cached).unwrap().block_number);
            }
        }
        if oldest_floor > last_leaving.body.state.block_number + retention_depth
            && dag.is_finalized(&last_leaving.block_hash).unwrap()
            && !committee.iter().any(|bond| bond.validator == leaving_key)
        {
            retired = true;
            break;
        }
    }
    assert!(
        retired,
        "the finalized floor must record the validator's exit"
    );
    assert!(nodes[1]
        .block_store
        .get(&last_leaving.block_hash)
        .unwrap()
        .is_some());
    let before = nodes[1].casper.get_snapshot().await.unwrap();
    let dag = nodes[1].casper.block_dag().await.unwrap();
    assert_eq!(
        dag.latest_message_hashes().get(&leaving_key),
        Some(&last_leaving.block_hash)
    );
    let mut sweep = casper::rust::util::mergeable_channels_gc::GcSweep::default();
    let mut gc_conf = nodes[1].casper.casper_shard_conf().clone();
    gc_conf.enable_block_store_gc = true;
    for _ in 0..100 {
        casper::rust::util::mergeable_channels_gc::collect_garbage(
            &mut sweep,
            &nodes[1].block_dag_storage,
            &nodes[1].block_store,
            &nodes[1].casper.runtime_manager,
            &gc_conf,
            &nodes[1].casper.effective_anchor,
        )
        .await
        .unwrap();
        if nodes[1]
            .block_store
            .get(&last_leaving.block_hash)
            .unwrap()
            .is_none()
        {
            break;
        }
    }
    assert!(
        nodes[1]
            .block_store
            .get(&last_leaving.block_hash)
            .unwrap()
            .is_none(),
        "ordinary GC must remove the retired validator's old body"
    );
    assert!(dag.lookup(&last_leaving.block_hash).unwrap().is_some());
    let after = nodes[1]
        .casper
        .get_snapshot()
        .await
        .expect("a retired validator's pruned body must not prevent a snapshot");
    assert_eq!(
        before
            .parents
            .iter()
            .map(|block| &block.block_hash)
            .collect::<Vec<_>>(),
        after
            .parents
            .iter()
            .map(|block| &block.block_hash)
            .collect::<Vec<_>>()
    );
    assert_eq!(before.max_seq_nums, after.max_seq_nums);
    assert_eq!(before.justifications, after.justifications);
    assert_eq!(
        before.on_chain_state.bonds_map,
        after.on_chain_state.bonds_map
    );

    let rebond = construct_deploy::source_deploy(
        r#"new rl(`rho:registry:lookup`), posCh, deployerId(`rho:system:deployerId`), ret in {
            rl!(`rho:system:pos`, *posCh) |
            for (@(_, pos) <- posCh) { @pos!("bond", *deployerId, 1n, *ret) }
        }"#
        .to_string(),
        2,
        Some(500_000),
        None,
        Some(leaving.private_key.clone()),
        Some(after.max_block_num),
        Some(shard_id),
    )
    .unwrap();
    assert!(nodes[2]
        .block_store
        .get(&last_leaving.block_hash)
        .unwrap()
        .is_some());
    let rebond_block = nodes[2].create_block_unsafe(&[rebond]).await.unwrap();
    assert_eq!(rebond_block.body.deploys.len(), 1);
    assert!(!rebond_block.body.deploys[0].is_failed);
    for node in &mut nodes {
        assert!(matches!(
            node.add_block(rebond_block.clone()).await.unwrap(),
            Either::Right(_)
        ));
        node.casper.finalize().await.unwrap();
    }

    for round in 0..12 {
        let proposer = [3, 1, 2][round % 3];
        let block = nodes[proposer].create_block_unsafe(&[]).await.unwrap();
        for node in &mut nodes {
            assert!(matches!(
                node.add_block(block.clone()).await.unwrap(),
                Either::Right(_)
            ));
            node.casper.finalize().await.unwrap();
        }

        let dag = nodes[1].casper.block_dag().await.unwrap();
        let Some(floor_hash) = dag.get_cached_floor(&dag.last_finalized_block()).unwrap() else {
            continue;
        };
        let floor_block = nodes[1].block_store.get(&floor_hash).unwrap().unwrap();
        let committee = floor_committee(
            &nodes[1].runtime_manager,
            &floor_block.body.state.post_state_hash,
        )
        .await
        .unwrap();
        if committee.iter().any(|bond| bond.validator == leaving_key) {
            assert_eq!(
                dag.latest_message_hashes().get(&leaving_key),
                Some(&last_leaving.block_hash)
            );
            nodes[1]
                .casper
                .get_snapshot()
                .await
                .expect("the old pruned body must not block snapshot after a same-key rebond");
            let first_after_rebond = nodes[0].create_block_unsafe(&[]).await.unwrap();
            assert_eq!(first_after_rebond.seq_num, last_leaving.seq_num + 1);
            assert!(matches!(
                nodes[1].add_block(first_after_rebond).await.unwrap(),
                Either::Right(_)
            ));
            return;
        }
    }
    panic!("the same key must re-enter the finalized committee after bonding");
}

#[tokio::test]
async fn a_snapshot_reports_a_missing_body_for_a_bonded_validator() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .unwrap();
    let deploy =
        construct_deploy::basic_deploy_data(0, None, Some(genesis.genesis_block.shard_id.clone()))
            .unwrap();
    let mut node = TestNode::standalone(genesis).await.unwrap();
    let own = node.add_block_from_deploys(&[deploy]).await.unwrap();
    assert!(node.block_store.discard_stored(&own.block_hash).unwrap());
    let error = match node.casper.get_snapshot().await {
        Ok(_) => panic!("an active validator's missing body must be reported"),
        Err(error) => error,
    };
    assert!(
        error
            .to_string()
            .contains("parent block referenced by latest_messages missing"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn a_snapshot_keeps_a_still_bonded_validator_with_an_old_latest_message() {
    use casper::rust::finality::floor::floor_committee;
    use rspace_plus_plus::rspace::history::Either;

    let mut parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    parameters.2.proof_of_stake.deploy_lifespan = 4;
    parameters.2.proof_of_stake.max_parent_depth = 3;
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .unwrap();
    let mut nodes = TestNode::create_network_with_overrides(
        genesis,
        4,
        crate::helper::test_node::ShardOverrides {
            synchrony_constraint_threshold: Some(0.0),
            finalization_rate: Some(0),
            max_parent_depth: Some(3),
            deploy_lifespan: Some(4),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    for node in &mut nodes {
        node.allow_empty_blocks = true;
    }

    let quiet = nodes[0].validator_id_opt.as_ref().unwrap().clone();
    let quiet_key = prost::bytes::Bytes::copy_from_slice(&quiet.public_key.bytes);
    let last = nodes[0].create_block_unsafe(&[]).await.unwrap();
    for node in &mut nodes {
        assert!(matches!(
            node.add_block(last.clone()).await.unwrap(),
            Either::Right(_)
        ));
    }

    let retention_depth = {
        let conf = nodes[1].casper.casper_shard_conf();
        i64::from(conf.max_parent_depth)
            + i64::from(conf.mergeable_channels_gc_depth_buffer)
            + conf.deploy_lifespan
    };

    let mut aged = false;
    for round in 0..80 {
        let block = nodes[1 + round % 3].create_block_unsafe(&[]).await.unwrap();
        for node in &mut nodes {
            assert!(matches!(
                node.add_block(block.clone()).await.unwrap(),
                Either::Right(_)
            ));
            node.casper.finalize().await.unwrap();
        }
        let dag = nodes[1].casper.block_dag().await.unwrap();
        let Some(floor_hash) = dag.get_cached_floor(&dag.last_finalized_block()).unwrap() else {
            continue;
        };
        let floor_number = dag.block_number_unsafe(&floor_hash).unwrap();
        if dag.is_finalized(&last.block_hash).unwrap()
            && floor_number.saturating_sub(last.body.state.block_number) > retention_depth
        {
            aged = true;
            break;
        }
    }
    assert!(
        aged,
        "the quiet validator's block must age past the body-retention window"
    );

    let dag = nodes[1].casper.block_dag().await.unwrap();
    assert_eq!(
        dag.latest_message_hashes().get(&quiet_key),
        Some(&last.block_hash),
        "the quiet validator still owns its old latest message"
    );
    let floor_hash = dag
        .get_cached_floor(&dag.last_finalized_block())
        .unwrap()
        .unwrap();
    let floor_block = nodes[1].block_store.get(&floor_hash).unwrap().unwrap();
    let committee = floor_committee(
        &nodes[1].runtime_manager,
        &floor_block.body.state.post_state_hash,
    )
    .await
    .unwrap();
    assert!(
        committee.iter().any(|bond| bond.validator == quiet_key),
        "the quiet validator must still be bonded for this regression"
    );

    assert!(nodes[1]
        .block_store
        .get(&last.block_hash)
        .unwrap()
        .is_some());
    let snapshot = nodes[1].casper.get_snapshot().await.unwrap();
    assert!(snapshot
        .justifications
        .iter()
        .any(|j| j.validator == quiet_key && j.latest_block_hash == last.block_hash));
    assert!(nodes[1]
        .block_store
        .discard_stored(&last.block_hash)
        .unwrap());
    let error = nodes[1]
        .casper
        .get_snapshot()
        .await
        .err()
        .expect("the quiet validator never left the committee");
    assert!(
        error.to_string().contains(&format!(
            "parent block referenced by latest_messages missing from block_store: {}",
            hex::encode(&last.block_hash)
        )),
        "unexpected error: {error}"
    );
}
