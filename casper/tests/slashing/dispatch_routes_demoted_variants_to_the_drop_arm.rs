// Parameterized coverage for the dispatcher's routing of verdicts that are
// NOT slash-worthy. `is_slashable()` narrowed to the equivocation class
// after CI run 32588262605: view-relative and locally-judged verdicts
// minted slash evidence that diverged across honest nodes, and the
// recursive carrier verdicts burned honest stake to FT −18.55. Every
// demoted variant now routes past the evidence-minting arms to the drop
// arm: buffer entry removed, no DAG insert, no EquivocationRecord — the
// block is refused without becoming anyone's economic evidence.
//
// AdmissibleEquivocation and IgnorableEquivocation keep their own
// dispatcher arms and their minting coverage in
// `integration_t_admissible_equivocation` /
// `integration_t_ignorable_equivocation`.

use casper::rust::block_status::InvalidBlock;
use casper::rust::casper::{Casper, MultiParentCasper};
use models::rust::block_hash::BlockHashSerde;
use models::rust::block_metadata::{CertifiedAdmissionOutcome, CertifiedSenderAuthority};
use models::rust::bond_generation::BondGeneration;

use super::detector_totality_helpers::{block as synth_block, validator as synth_validator};
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

/// Every `InvalidBlock` variant outside the equivocation class. If a new
/// variant lands, `block_status.rs::is_slashable()`'s exhaustive match
/// forces a deliberate classification there first; extending this list is
/// the one-line follow-up.
const DEMOTED_VARIANTS: &[InvalidBlock] = &[
    InvalidBlock::InvalidFormat,
    InvalidBlock::InvalidSignature,
    InvalidBlock::InvalidSender,
    InvalidBlock::InvalidVersion,
    InvalidBlock::InvalidTimestamp,
    InvalidBlock::DeployNotSigned,
    InvalidBlock::InvalidBlockNumber,
    InvalidBlock::InvalidRepeatDeploy,
    InvalidBlock::InvalidParents,
    InvalidBlock::InvalidFollows,
    InvalidBlock::InvalidSequenceNumber,
    InvalidBlock::InvalidShardId,
    InvalidBlock::JustificationRegression,
    InvalidBlock::NeglectedInvalidBlock,
    InvalidBlock::NeglectedEquivocation,
    InvalidBlock::InvalidTransaction,
    InvalidBlock::InvalidBondsCache,
    InvalidBlock::InvalidEquivocationEvidence,
    InvalidBlock::InvalidBlockHash,
    InvalidBlock::UnauthorizedSlashDeploy,
    InvalidBlock::InvalidRejectedDeploy,
    InvalidBlock::ContainsExpiredDeploy,
    InvalidBlock::ContainsTimeExpiredDeploy,
    InvalidBlock::ContainsFutureDeploy,
    InvalidBlock::NotOfInterest,
    InvalidBlock::LowDeployCost,
    InvalidBlock::PrematureDeployRetry,
];

#[tokio::test]
async fn dispatch_persists_every_certified_rejection_without_false_slash_evidence() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");
    let node = TestNode::standalone(genesis.clone())
        .await
        .expect("Failed to build standalone node");

    let seq_num: i32 = 2;
    let expected_base_seq: i32 = 1;

    for (i, variant) in DEMOTED_VARIANTS.iter().enumerate() {
        assert!(
            !variant.is_slashable(),
            "[{:?}] variant must be demoted for this drop-arm test to be meaningful",
            variant
        );

        let hash_byte = (i as u8).saturating_add(1);
        let sender = synth_validator(hash_byte.saturating_add(21));
        let synth = synth_block(hash_byte, sender.clone(), seq_num, vec![], vec![]);

        let hash_serde = BlockHashSerde(synth.block_hash.clone());
        node.casper
            .casper_buffer_storage
            .put_pendant(hash_serde.clone())
            .expect("pre-populate buffer pendant");

        let dag_repr = node.casper.block_dag().await.expect("dag representation");
        let commitment = synth
            .header
            .finalized_floor
            .as_ref()
            .expect("finalized-floor commitment");
        let certificate = CertifiedSenderAuthority::new(
            &synth,
            commitment.floor_hash.clone(),
            commitment.floor_post_state_hash.clone(),
            commitment.authority_context_digest.clone(),
            BondGeneration::GENESIS,
            100,
        )
        .expect("certified sender authority");
        let outcome = CertifiedAdmissionOutcome::rejected(&synth, &certificate, variant.into())
            .expect("certified rejection");
        node.casper
            .handle_invalid_block(&synth, variant, &dag_repr, &certificate, &outcome)
            .expect("dispatcher must persist the certified rejection");

        let dag_after = node
            .casper
            .block_dag()
            .await
            .expect("post-dispatch dag representation");
        assert!(
            dag_after.contains(&synth.block_hash),
            "[{:?}] certified rejection metadata must enter the DAG",
            variant
        );
        let metadata = dag_after
            .lookup(&synth.block_hash)
            .expect("metadata lookup")
            .expect("certified rejection metadata");
        assert_eq!(metadata.rejection_reason(), Some(variant.into()));
        assert!(!metadata.is_slash_evidence_eligible());

        assert!(
            !node
                .casper
                .casper_buffer_storage
                .get_pendants()
                .contains(&hash_serde),
            "[{:?}] buffer must be purged after durable metadata insertion",
            variant
        );

        let records = node
            .casper
            .block_dag_storage
            .access_equivocations_tracker(|tracker| tracker.data())
            .expect("equivocations tracker access");
        let has_record = records.iter().any(|record| {
            record.equivocator == synth.sender
                && record.equivocation_base_block_seq_num == expected_base_seq
        });
        assert!(
            !has_record,
            "[{:?}] a demoted verdict must not mint an EquivocationRecord",
            variant
        );
    }
}
