use crypto::rust::hash::blake2b256::Blake2b256;
use models::rust::block_hash;
use models::rust::block_metadata::{
    AdmissionRejectionReason, BlockMetadata, CertifiedAdmissionDecision, CertifiedAdmissionOutcome,
    CertifiedSenderAuthority,
};
use models::rust::bond_generation::BondGeneration;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Body, F1r3flyState, FinalizedFloorCommitment, Header,
};
use prost::bytes::Bytes;

pub(crate) fn certify(metadata: BlockMetadata, generation: BondGeneration) -> BlockMetadata {
    certify_with_decision(metadata, generation, CertifiedAdmissionDecision::Accepted)
}

pub(crate) fn certify_rejected(
    metadata: BlockMetadata,
    generation: BondGeneration,
    reason: AdmissionRejectionReason,
) -> BlockMetadata {
    certify_with_decision(
        metadata,
        generation,
        CertifiedAdmissionDecision::Rejected(reason),
    )
}

fn certify_with_decision(
    mut metadata: BlockMetadata,
    generation: BondGeneration,
    decision: CertifiedAdmissionDecision,
) -> BlockMetadata {
    let pre_state_hash = Bytes::from(vec![0; block_hash::LENGTH]);
    let mut block = BlockMessage {
        block_hash: metadata.block_hash.clone(),
        header: Header {
            parents_hash_list: metadata.parents.clone(),
            timestamp: 0,
            version: metadata.protocol_version,
            extra_bytes: Bytes::new(),
            sender_bond_generation: Some(generation),
            objective_equivocation_evidence_delta: metadata
                .objective_equivocation_evidence_delta
                .clone(),
            finalized_floor: None,
        },
        body: Body {
            state: F1r3flyState {
                pre_state_hash: pre_state_hash.clone(),
                post_state_hash: metadata.post_state_hash.clone(),
                bonds: Vec::new(),
                bond_generations: Vec::new(),
                active_validators: Vec::new(),
                block_number: metadata.block_number,
            },
            deploys: Vec::new(),
            rejected_deploys: Vec::new(),
            rejected_state_effects: Vec::new(),
            system_deploys: Vec::new(),
            extra_bytes: Bytes::new(),
            applied_from_scope: Vec::new(),
            merge_base: Bytes::new(),
        },
        justifications: metadata.justifications.clone(),
        sender: metadata.sender.clone(),
        seq_num: metadata.sequence_number,
        sig: Bytes::new(),
        sig_algorithm: String::new(),
        shard_id: "root".to_string(),
        extra_bytes: Bytes::new(),
        finalized_floor_certificate: None,
    };
    let stake = metadata
        .weight_map
        .get(&metadata.sender)
        .copied()
        .filter(|stake| *stake > 0)
        .unwrap_or(1);
    let authority_floor_hash = metadata
        .parents
        .first()
        .cloned()
        .unwrap_or_else(|| metadata.block_hash.clone());
    let authority_floor_post_state_hash = pre_state_hash;
    let mut preimage = b"f1r3fly-certified-test-metadata-context-v1".to_vec();
    preimage.extend_from_slice(&authority_floor_hash);
    preimage.extend_from_slice(&authority_floor_post_state_hash);
    let context_digest: Bytes = Blake2b256::hash(preimage).into();
    let commitment = metadata
        .finalized_floor_commitment
        .clone()
        .unwrap_or_else(|| FinalizedFloorCommitment {
            floor_hash: authority_floor_hash.clone(),
            floor_post_state_hash: authority_floor_post_state_hash.clone(),
            certificate_digest: Blake2b256::hash(
                b"f1r3fly-certified-test-finalization-certificate-v1".to_vec(),
            )
            .into(),
            authority_context_digest: context_digest.clone(),
        });
    block.header.finalized_floor = Some(commitment.clone());
    metadata.finalized_floor_commitment = Some(commitment);
    let sender_authority = CertifiedSenderAuthority::new(
        &block,
        authority_floor_hash,
        authority_floor_post_state_hash,
        context_digest,
        generation,
        stake,
    )
    .expect("valid test sender authority");
    metadata.admission_outcome = Some(
        match decision {
            CertifiedAdmissionDecision::Accepted => {
                CertifiedAdmissionOutcome::accepted(&block, &sender_authority)
            }
            CertifiedAdmissionDecision::Rejected(reason) => {
                CertifiedAdmissionOutcome::rejected(&block, &sender_authority, reason)
            }
        }
        .expect("valid test admission outcome"),
    );
    metadata.sender_authority = Some(sender_authority);
    metadata.validate().expect("valid certified test metadata");
    metadata
}
