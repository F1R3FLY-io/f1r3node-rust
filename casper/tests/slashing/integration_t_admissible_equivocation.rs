// Integration test — Tier 1 production-path verification of the
// `AdmissibleEquivocation` arm of
// `MultiParentCasperImpl::handle_invalid_block`.
//
// Reference: docs/casper/theory/slashing/design/14-test-plan.md §14.3.5,
// design/09-bug-fixes-and-rationale.md §9.1.
// Plan-agent designed Track 2 / equivocation-construction recipe.
//
// AdmissibleEquivocation differs from IgnorableEquivocation by whether a
// block the node holds cites the equivocation (block ingest's
// `is_cited(b1p)`):
//   * Ignorable: no held block cites b1p
//   * Admissible: a held block cites b1p, so it MUST be added to the DAG
//     even though it's an equivocation — see equivocation_detector.rs:38-41
//
// Recipe:
//   1. Same setup as Ignorable: build b1, build b1p via
//      `equivocate_block(nodes[0], &b1, vec![d2])`.
//   2. nodes[2] sees only b1p (valid there) and builds c2 on it.
//   3. nodes[1] receives c2 first: c2 waits on b1p.
//   4. nodes[1].process_block(b1) → Right(Valid).
//   5. nodes[1].process_block(b1p) → Left(Invalid(AdmissibleEquivocation)).
//   6. SlashingProductionAdapter snapshot confirms post-fix #1+#3
//      record-mint at the production tier.

use casper::rust::block_status::{BlockError, InvalidBlock};
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
async fn integration_t_admissible_equivocation() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");
    let shard_id = genesis.genesis_block.shard_id.clone();

    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None)
        .await
        .expect("Failed to create network");

    let validators = canonical_validator_order(&genesis);

    // v0 produces b1 with deploy d1.
    let d1 = construct_deploy::basic_deploy_data(0, None, Some(shard_id.clone())).expect("d1");
    let b1 = nodes[0]
        .create_block_unsafe(&[d1])
        .await
        .expect("create b1");

    // Build the equivocating sibling b1p with a distinct deploy.
    let d2 = construct_deploy::basic_deploy_data(1, None, Some(shard_id.clone()))
        .expect("d2 (distinct nonce)");
    let b1p = equivocate_block(&mut nodes[0], &b1, vec![d2])
        .await
        .expect("equivocate_block");

    // nodes[2] sees only b1p and builds on it; nodes[1] receives that
    // child first, so a held block waits on b1p. This is the structural
    // difference between the Admissible and Ignorable arms — the
    // dispatcher reads it in equivocation_detector.rs:38.
    let s0 = nodes[2]
        .process_block(b1p.clone())
        .await
        .expect("nodes[2] process b1p");
    assert!(
        matches!(s0, Either::Right(_)),
        "b1p valid on nodes[2], got: {:?}",
        s0
    );
    let d3 = construct_deploy::basic_deploy_data(2, None, Some(shard_id.clone())).expect("d3");
    let c2 = nodes[2]
        .add_block_from_deploys(&[d3])
        .await
        .expect("nodes[2] builds on b1p");
    assert!(
        c2.header.parents_hash_list.contains(&b1p.block_hash),
        "c2 must cite b1p"
    );
    nodes[1].process_block(c2).await.expect("nodes[1] holds c2");
    assert!(
        nodes[1].ingest_view.is_cited(&b1p.block_hash),
        "a held block must cite b1p"
    );

    // Process b1 on nodes[1] — must classify Valid.
    let s1 = nodes[1]
        .process_block(b1.clone())
        .await
        .expect("process b1");
    assert!(matches!(s1, Either::Right(_)), "b1 valid, got: {:?}", s1);

    // Process b1p on nodes[1] — must classify AdmissibleEquivocation
    // because a held block waits on it.
    let s2 = nodes[1]
        .process_block(b1p.clone())
        .await
        .expect("process b1p");
    assert!(
        matches!(
            s2,
            Either::Left(BlockError::Invalid(InvalidBlock::AdmissibleEquivocation))
        ),
        "b1p must classify AdmissibleEquivocation given dependency seeding, got: {:?}",
        s2
    );

    // Snapshot and assert post-fix #1+#3: dispatcher minted a record
    // at the production tier.
    let snapshot = production_snapshot_at(&nodes[1], &b1, &genesis.genesis_block, validators)
        .await
        .expect("snapshot");

    let v0_label = "v0";
    let has_any_record =
        (0..=10).any(|base| <_ as SlashingObserver>::has_record(&snapshot, v0_label, base));
    assert!(
        has_any_record,
        "post-fix #1+#3 invariant: dispatcher mints EquivocationRecord \
         in the AdmissibleEquivocation arm"
    );
}
