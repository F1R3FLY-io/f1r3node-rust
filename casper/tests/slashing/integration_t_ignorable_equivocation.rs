// Protocol-v5 production-path regression for an unsolicited certified sibling.
// Arrival context changes only the observation. Both siblings remain
// intrinsically valid DAG members and their certified identity creates the same
// canonical generation-aware objective evidence as a requested sibling.

use std::collections::BTreeSet;

use casper::rust::block_status::EquivocationObservation;
use casper::rust::casper::{Casper, MultiParentCasper};
use casper::rust::util::construct_deploy;
use rspace_plus_plus::rspace::history::Either;

use super::integration_helpers::{
    canonical_validator_order, equivocate_block, production_snapshot_at,
};
use super::observer::SlashingObserver;
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

#[serial_test::serial]
#[tokio::test]
async fn integration_t_ignorable_equivocation() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");
    let shard_id = genesis.genesis_block.shard_id.clone();

    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .expect("Failed to create network");

    let validators = canonical_validator_order(&genesis);

    // Round 1: v0 (nodes[0]) creates b1 with deploy d1.
    // `create_block_unsafe` returns the block but does NOT add it
    // to nodes[0]'s DAG, so the snapshot taken inside
    // `equivocate_block` is still at genesis.
    let d1 = construct_deploy::basic_deploy_data(0, None, Some(shard_id.clone())).expect("d1");
    let b1 = nodes[0]
        .create_block_unsafe(&[d1])
        .await
        .expect("create b1");

    // Construct b1p: same v0, same seq_num, same parents/justifications
    // as b1, but a different deploy. Must use a distinct nonce so
    // Validate::repeat_deploy doesn't reject the second block.
    let d2 = construct_deploy::basic_deploy_data(1, None, Some(shard_id.clone()))
        .expect("d2 (distinct nonce from d1)");
    let b1p = equivocate_block(&mut nodes[0], &b1, vec![d2])
        .await
        .expect("equivocate_block");

    assert_ne!(
        b1.block_hash, b1p.block_hash,
        "equivocation requires distinct hashes"
    );
    assert_eq!(
        b1.seq_num, b1p.seq_num,
        "equivocation requires same seq_num"
    );
    assert_eq!(b1.sender, b1p.sender, "equivocation requires same sender");

    // Process b1 (well-formed) on node 1 first. Node 1 sees this
    // as a normal block; no equivocation yet.
    let s1 = nodes[1]
        .process_block(b1.clone())
        .await
        .expect("process b1");
    assert!(
        matches!(s1, Either::Right(_)),
        "first block accepts: {:?}",
        s1
    );

    let mut validation_snapshot = nodes[1]
        .casper
        .get_snapshot()
        .await
        .expect("validation snapshot");
    let validation = nodes[1]
        .casper
        .validate(&b1p, &mut validation_snapshot)
        .await
        .expect("validate unsolicited sibling");
    assert!(matches!(validation.status(), Either::Right(_)));
    assert!(validation.sender_authority().is_some());
    assert_eq!(
        validation.equivocation_observation(),
        Some(EquivocationObservation::Unsolicited)
    );

    let s2 = nodes[1]
        .process_block(b1p.clone())
        .await
        .expect("process b1p");
    assert!(
        matches!(s2, Either::Right(_)),
        "an unsolicited certified sibling remains intrinsically valid: {:?}",
        s2
    );

    let dag = nodes[1]
        .casper
        .block_dag()
        .await
        .expect("post-admission DAG");
    for sibling in [&b1, &b1p] {
        assert!(dag
            .lookup_unsafe(&sibling.block_hash)
            .expect("certified sibling metadata")
            .is_accepted());
    }
    let generation = b1
        .header
        .sender_bond_generation
        .expect("certified sender generation");
    let identity = (b1.sender.clone(), generation, b1.seq_num);
    assert_eq!(
        dag.equivocation_observations().get(&identity).cloned(),
        Some(BTreeSet::from([
            b1.block_hash.clone(),
            b1p.block_hash.clone(),
        ]))
    );

    // Snapshot and assert post-fix #1: a record exists at
    // (v0, some base_seq) for v0. The exact base seq depends on
    // genesis-block sequence numbering; we assert presence rather
    // than a specific base value.
    let snapshot = production_snapshot_at(&nodes[1], &b1, &genesis.genesis_block, validators)
        .await
        .expect("snapshot");

    let v0_label = "v0";
    let has_any_record =
        (0..=10).any(|base| <_ as SlashingObserver>::has_record(&snapshot, v0_label, base));
    assert!(
        has_any_record,
        "certified unsolicited siblings must create generation-aware objective evidence"
    );
}
