// Integration test — Tier 1 production-path verification of the
// `NeglectedInvalidBlock` arm of
// `MultiParentCasperImpl::handle_invalid_block`.
//
// Reference: docs/theory/slashing/design/14-test-plan.md §14.3.5,
// design/08-two-level-and-collusion.md.
//
// Why NeglectedInvalidBlock and not NeglectedEquivocation:
// the production validation pipeline runs `neglected_invalid_block`
// (validate.rs:~1018) BEFORE `neglected_equivocation`. A block
// whose justifications cite an invalid block without including a
// SlashDeploy fires NeglectedInvalidBlock first. The deeper
// NeglectedEquivocation arm fires only when the invalid block is
// NOT directly in justifications but is transitively reachable
// through another validator's justification chain — and producing
// such a chain in a single integration test is structurally
// impossible (the dependency-check / tracker-state cycle: a block
// whose justifications cite an invalid block becomes itself
// invalid the moment the receiver has the equivocator's record).
// The harness UC-15 + Rocq T-9.7 (TwoLevelSlashing.v) carry the
// multi-block-closure NeglectedEquivocation coverage at the
// formal-model abstraction level. This integration test pins the
// production-tier behaviour for the SINGLE-BLOCK direct-cite
// scenario, which is the achievable production-pipeline closure
// of the two-level-slashing requirement.
//
// Recipe:
//   1. v0 (nodes[0]) creates b1 (valid).
//   2. v0 creates b1p via equivocate_block (Byzantine sibling).
//   3. nodes[1] processes b1p first (unsolicited → Ignorable;
//      record minted with b1p as witness; b1p stored as invalid).
//   4. nodes[1] proposes b3 via propose_with_explicit_justifications,
//      explicitly citing v0→b1p. The natural snapshot would only
//      cite v0→b1; the helper merges in the explicit (v0, b1p)
//      entry so b3 directly references the invalid block.
//   5. nodes[2] processes b1p first (so it has v0's record + b1p
//      stored as invalid), then b3.
//   6. b3 cites an invalid block (b1p) AND has no SlashDeploy →
//      `Validate::neglected_invalid_block` fires →
//      Left(Invalid(NeglectedInvalidBlock)).
//   7. Post-fix #3: dispatcher mints a record for v1 (the
//      neglecter — b3's sender).

use casper::rust::block_status::{BlockError, InvalidBlock};
use casper::rust::casper::Casper;
use casper::rust::util::construct_deploy;
use models::rust::casper::protocol::casper_message::Justification;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

use super::integration_helpers::{
    canonical_validator_order, equivocate_block, production_snapshot_at,
    propose_with_explicit_justifications,
};
use super::observer::SlashingObserver;

#[serial_test::serial]
#[tokio::test]
async fn integration_t_neglected_invalid_block() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");
    let shard_id = genesis.genesis_block.shard_id.clone();

    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .expect("Failed to create network");

    let validators = canonical_validator_order(&genesis);

    // Step 1: v0 creates b1.
    let d1 = construct_deploy::basic_deploy_data(0, None, Some(shard_id.clone()))
        .expect("d1");
    let b1 = nodes[0]
        .create_block_unsafe(&[d1])
        .await
        .expect("create b1");

    // Step 2: v0 creates b1p (Byzantine sibling).
    let d2 = construct_deploy::basic_deploy_data(1, None, Some(shard_id.clone()))
        .expect("d2");
    let b1p = equivocate_block(&mut nodes[0], &b1, vec![d2])
        .await
        .expect("equivocate_block");

    // Step 3: nodes[1] processes b1 first (Valid), then b1p
    // (equivocation detected, record minted with b1p as witness,
    // b1p stored as invalid in nodes[1]'s DAG).
    let _ = nodes[1]
        .process_block(b1.clone())
        .await
        .expect("nodes[1] process b1");
    let s_b1p = nodes[1]
        .process_block(b1p.clone())
        .await
        .expect("nodes[1] process b1p");
    assert!(
        matches!(
            s_b1p,
            Either::Left(BlockError::Invalid(InvalidBlock::IgnorableEquivocation))
                | Either::Left(BlockError::Invalid(InvalidBlock::AdmissibleEquivocation))
        ),
        "b1p classified as equivocation, got: {:?}",
        s_b1p
    );

    // Step 4: nodes[1] proposes b3 with explicit (v0, b1p)
    // justification — directly citing the invalid block.
    let d3 = construct_deploy::basic_deploy_data(20, None, Some(shard_id.clone()))
        .expect("d3");
    let mut b3_justifs: Vec<Justification> = Vec::new();
    b3_justifs.push(Justification {
        validator: nodes[0]
            .validator_id_opt
            .as_ref()
            .unwrap()
            .public_key
            .bytes
            .clone(),
        latest_block_hash: b1p.block_hash.clone(),
    });
    let b3 = propose_with_explicit_justifications(&mut nodes[1], vec![d3], b3_justifs)
        .await
        .expect("propose b3");

    // Confirm b3 has no SlashDeploy.
    use models::rust::casper::protocol::casper_message::{
        ProcessedSystemDeploy, SystemDeployData,
    };
    let has_slash_deploy = b3.body.system_deploys.iter().any(|sd| {
        matches!(
            sd,
            ProcessedSystemDeploy::Succeeded {
                system_deploy: SystemDeployData::Slash { .. },
                ..
            }
        )
    });
    assert!(!has_slash_deploy, "neglecting b3 must not contain SlashDeploy");

    // Step 5: nodes[2] processes b1 + b1p (so v0's record exists
    // and b1p is stored as invalid), then b3. Without the
    // SlashDeploy in b3, neglected_invalid_block fires.
    let _ = nodes[2]
        .process_block(b1.clone())
        .await
        .expect("nodes[2] process b1");
    let _ = nodes[2]
        .process_block(b1p.clone())
        .await
        .expect("nodes[2] process b1p");

    let s_b3 = nodes[2]
        .process_block(b3.clone())
        .await
        .expect("nodes[2] process b3");
    assert!(
        matches!(
            s_b3,
            Either::Left(BlockError::Invalid(InvalidBlock::NeglectedInvalidBlock))
        ),
        "b3 must classify NeglectedInvalidBlock (cites invalid b1p without slash), got: {:?}",
        s_b3
    );

    // Step 6: Snapshot — post-fix #3 catch-all minted records for
    // both v0 (equivocator) and v1 (neglecter who cited the
    // invalid block without slashing).
    let snapshot =
        production_snapshot_at(&nodes[2], &b1p, &genesis.genesis_block, validators)
            .await
            .expect("snapshot");

    let has_v0 = (0..=10)
        .any(|b| <_ as SlashingObserver>::has_record(&snapshot, "v0", b));
    let has_v1 = (0..=10)
        .any(|b| <_ as SlashingObserver>::has_record(&snapshot, "v1", b));
    assert!(has_v0, "v0's equivocation record persists in nodes[2]'s tracker");
    assert!(
        has_v1,
        "post-fix #3 catch-all: dispatcher mints record for v1 \
         (the NeglectedInvalidBlock neglecter) at the production tier"
    );
}
