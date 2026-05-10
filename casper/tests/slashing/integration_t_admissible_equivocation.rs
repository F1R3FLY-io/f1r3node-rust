// Integration test — Tier 1 production-path verification of the
// `AdmissibleEquivocation` arm of
// `MultiParentCasperImpl::handle_invalid_block`.
//
// Reference: docs/theory/slashing/design/14-test-plan.md §14.3.5,
// design/09-bug-fixes-and-rationale.md §9.1.
// Plan-agent designed Track 2 / equivocation-construction recipe.
//
// AdmissibleEquivocation differs from IgnorableEquivocation by
// the value of `casper_buffer_storage.requested_as_dependency(b1p)`:
//   * Ignorable: false (no other block has cited b1p as a dependency)
//   * Admissible: true (b1p has been requested as a dependency
//     of some other block, so it MUST be added to the DAG even
//     though it's an equivocation — see equivocation_detector.rs:38-41)
//
// Recipe:
//   1. Same setup as Ignorable: build b1, build b1p via
//      `equivocate_block(nodes[0], &b1, vec![d2])`.
//   2. Before processing b1p on nodes[1], seed the casper buffer:
//      `nodes[1].casper.casper_buffer_storage.add_relation(b1p, dummy_child)`.
//      This puts b1p into `parent_to_child_adjacency_list`,
//      causing `requested_as_dependency(b1p) == true`
//      (casper_buffer_key_value_storage.rs:195-202).
//   3. nodes[1].process_block(b1) → Right(Valid).
//   4. nodes[1].process_block(b1p) → Left(Invalid(AdmissibleEquivocation)).
//   5. SlashingProductionAdapter snapshot confirms post-fix #1+#3
//      record-mint at the production tier.

use block_storage::rust::casperbuffer::casper_buffer_key_value_storage::CasperBufferKeyValueStorage;
use casper::rust::block_status::{BlockError, InvalidBlock};
use casper::rust::casper::Casper;
use casper::rust::util::construct_deploy;
use models::rust::block_hash::BlockHashSerde;
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

    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
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

    // Seed nodes[1]'s casper buffer to mark b1p as a requested
    // dependency. This is the structural difference between
    // Admissible and Ignorable arms — the dispatcher reads this
    // flag in equivocation_detector.rs:38.
    let dummy_child = BlockHashSerde(prost::bytes::Bytes::from_static(
        b"dummy-child-of-equivocation-test",
    ));
    nodes[1]
        .casper
        .casper_buffer_storage
        .add_relation(BlockHashSerde(b1p.block_hash.clone()), dummy_child)
        .expect("seed casper buffer with b1p as dependency");

    // Sanity: confirm the buffer reports b1p as requested.
    assert!(
        nodes[1]
            .casper
            .casper_buffer_storage
            .requested_as_dependency(&BlockHashSerde(b1p.block_hash.clone())),
        "casper buffer should report b1p as a requested dependency"
    );

    // Process b1 on nodes[1] — must classify Valid.
    let s1 = nodes[1]
        .process_block(b1.clone())
        .await
        .expect("process b1");
    assert!(matches!(s1, Either::Right(_)), "b1 valid, got: {:?}", s1);

    // Process b1p on nodes[1] — must classify AdmissibleEquivocation
    // because b1p is in the buffer's dependency adjacency list.
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
