// See models/src/main/scala/coop/rchain/casper/protocol/CasperMessage.scala

use crypto::rust::hash::blake2b256::Blake2b256;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::signatures_alg::SignaturesAlgFactory;
use crypto::rust::signatures::signed::{Signed, ToMessage};
use prost::Message;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::state::rspace_exporter::RSpaceExporterInstance;
use shared::rust::{Byte, ByteVector};

use crate::casper::system_deploy_data_proto::SystemDeploy;
use crate::casper::*;
use crate::rhoapi::PCost;
use crate::rust::block_hash::{BlockHash, BlockHashSerde};
use crate::rust::bond_generation::BondGeneration;
use crate::rust::casper::pretty_printer::PrettyPrinter;
use crate::rust::deploy_id::{DeployIdV6, DeployLookupId, LegacyDeploySignature};
use crate::rust::validator::ValidatorSerde;
use crate::rust::{block_hash, validator};

// TODO: Use type ByteString from models crate
type ByteString = prost::bytes::Bytes;

#[derive(Clone, Debug, PartialEq)]
pub enum CasperMessage {
    BlockHashMessage(BlockHashMessage),
    BlockMessage(BlockMessage),
    ApprovedBlockCandidate(ApprovedBlockCandidate),
    ApprovedBlock(ApprovedBlock),
    ApprovedBlockRequest(ApprovedBlockRequest),
    BlockApproval(BlockApproval),
    BlockRequest(BlockRequest),
    FinalizationCertificateRequest(FinalizationCertificateRequest),
    FinalizationCertificateResponse(FinalizationCertificateResponse),
    ForkChoiceTipRequest(ForkChoiceTipRequest),
    HasBlock(HasBlock),
    HasBlockRequest(HasBlockRequest),
    NoApprovedBlockAvailable(NoApprovedBlockAvailable),
    UnapprovedBlock(UnapprovedBlock),
    // Last finalized state messages
    StoreItemsMessageRequest(StoreItemsMessageRequest),
    StoreItemsMessage(StoreItemsMessage),
    MergeableEntryRequest(MergeableEntryRequest),
    MergeableEntryResponse(MergeableEntryResponse),
    FloorCacheRequest(FloorCacheRequest),
    FloorCacheResponse(FloorCacheResponse),
}

impl CasperMessage {
    /// Convert from individual proto message types to CasperMessage
    /// This matches the Scala CasperMessage.from method behavior
    pub fn from_block_hash_message(proto: BlockHashMessageProto) -> Self {
        CasperMessage::BlockHashMessage(BlockHashMessage::from_proto(proto))
    }

    pub fn from_block_message(proto: BlockMessageProto) -> Result<Self, String> {
        Ok(CasperMessage::BlockMessage(BlockMessage::from_proto(
            proto,
        )?))
    }

    pub fn from_approved_block_candidate(
        proto: ApprovedBlockCandidateProto,
    ) -> Result<Self, String> {
        Ok(CasperMessage::ApprovedBlockCandidate(
            ApprovedBlockCandidate::from_proto(proto)?,
        ))
    }

    pub fn from_approved_block(proto: ApprovedBlockProto) -> Result<Self, String> {
        Ok(CasperMessage::ApprovedBlock(ApprovedBlock::from_proto(
            proto,
        )?))
    }

    pub fn from_approved_block_request(proto: ApprovedBlockRequestProto) -> Self {
        CasperMessage::ApprovedBlockRequest(ApprovedBlockRequest::from_proto(proto))
    }

    pub fn from_block_approval(proto: BlockApprovalProto) -> Result<Self, String> {
        Ok(CasperMessage::BlockApproval(BlockApproval::from_proto(
            proto,
        )?))
    }

    pub fn from_block_request(proto: BlockRequestProto) -> Self {
        CasperMessage::BlockRequest(BlockRequest::from_proto(proto))
    }

    pub fn from_finalization_certificate_request(
        proto: FinalizationCertificateRequestProto,
    ) -> Result<Self, String> {
        Ok(CasperMessage::FinalizationCertificateRequest(
            FinalizationCertificateRequest::from_proto(proto)?,
        ))
    }

    pub fn from_finalization_certificate_response(
        proto: FinalizationCertificateResponseProto,
    ) -> Result<Self, String> {
        Ok(CasperMessage::FinalizationCertificateResponse(
            FinalizationCertificateResponse::from_proto(proto)?,
        ))
    }

    pub fn from_fork_choice_tip_request(_proto: ForkChoiceTipRequestProto) -> Self {
        CasperMessage::ForkChoiceTipRequest(ForkChoiceTipRequest)
    }

    pub fn from_has_block(proto: HasBlockProto) -> Self {
        CasperMessage::HasBlock(HasBlock::from_proto(proto))
    }

    pub fn from_has_block_request(proto: HasBlockRequestProto) -> Self {
        CasperMessage::HasBlockRequest(HasBlockRequest::from_proto(proto))
    }

    pub fn from_no_approved_block_available(proto: NoApprovedBlockAvailableProto) -> Self {
        CasperMessage::NoApprovedBlockAvailable(NoApprovedBlockAvailable::from_proto(proto))
    }

    pub fn from_unapproved_block(proto: UnapprovedBlockProto) -> Result<Self, String> {
        Ok(CasperMessage::UnapprovedBlock(UnapprovedBlock::from_proto(
            proto,
        )?))
    }

    pub fn from_store_items_message_request(proto: StoreItemsMessageRequestProto) -> Self {
        CasperMessage::StoreItemsMessageRequest(StoreItemsMessageRequest::from_proto(proto))
    }

    pub fn from_store_items_message(proto: StoreItemsMessageProto) -> Self {
        CasperMessage::StoreItemsMessage(StoreItemsMessage::from_proto(proto))
    }

    pub fn from_mergeable_entry_request(proto: MergeableEntryRequestProto) -> Self {
        CasperMessage::MergeableEntryRequest(MergeableEntryRequest::from_proto(proto))
    }

    pub fn from_mergeable_entry_response(proto: MergeableEntryResponseProto) -> Self {
        CasperMessage::MergeableEntryResponse(MergeableEntryResponse::from_proto(proto))
    }

    pub fn from_floor_cache_request(proto: FloorCacheRequestProto) -> Self {
        CasperMessage::FloorCacheRequest(FloorCacheRequest::from_proto(proto))
    }

    pub fn from_floor_cache_response(proto: FloorCacheResponseProto) -> Self {
        CasperMessage::FloorCacheResponse(FloorCacheResponse::from_proto(proto))
    }
}

// TODO: Remove all into() and to_vec() once we have correct ByteString type in the models crate
#[derive(Clone, Debug, PartialEq)]
pub struct HasBlockRequest {
    pub hash: ByteString,
}

impl HasBlockRequest {
    pub fn from_proto(proto: HasBlockRequestProto) -> Self { Self { hash: proto.hash } }

    pub fn to_proto(self) -> HasBlockRequestProto { HasBlockRequestProto { hash: self.hash } }
}

#[derive(Clone, Debug, PartialEq)]
pub struct HasBlock {
    pub hash: ByteString,
}

impl HasBlock {
    pub fn from_proto(proto: HasBlockProto) -> Self { Self { hash: proto.hash } }

    pub fn to_proto(self) -> HasBlockProto { HasBlockProto { hash: self.hash } }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BlockRequest {
    pub hash: ByteString,
}

impl BlockRequest {
    pub fn from_proto(proto: BlockRequestProto) -> Self { Self { hash: proto.hash } }

    pub fn to_proto(self) -> BlockRequestProto { BlockRequestProto { hash: self.hash } }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FinalizationCertificateRequest {
    pub digest: ByteString,
}

impl FinalizationCertificateRequest {
    pub const MAX_ENCODED_BYTES: usize = 64;

    pub fn from_proto(proto: FinalizationCertificateRequestProto) -> Result<Self, String> {
        if proto.digest.len() != block_hash::LENGTH {
            return Err(format!(
                "finalization certificate request digest must be {} bytes",
                block_hash::LENGTH
            ));
        }
        Ok(Self {
            digest: proto.digest,
        })
    }

    pub fn to_proto(self) -> FinalizationCertificateRequestProto {
        FinalizationCertificateRequestProto {
            digest: self.digest,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FinalizationCertificateResponse {
    pub digest: ByteString,
    pub certificate: FinalizationCertificate,
}

impl FinalizationCertificateResponse {
    pub const MAX_ENCODED_BYTES: usize = FinalizationCertificate::MAX_ENCODED_BYTES + 128;

    pub fn from_proto(proto: FinalizationCertificateResponseProto) -> Result<Self, String> {
        if proto.digest.len() != block_hash::LENGTH {
            return Err(format!(
                "finalization certificate response digest must be {} bytes",
                block_hash::LENGTH
            ));
        }
        let certificate =
            FinalizationCertificate::from_proto(proto.certificate.ok_or_else(|| {
                "finalization certificate response is missing proof".to_string()
            })?)?;
        if certificate.digest() != proto.digest {
            return Err("finalization certificate response digest mismatch".to_string());
        }
        Ok(Self {
            digest: proto.digest,
            certificate,
        })
    }

    pub fn to_proto(self) -> FinalizationCertificateResponseProto {
        FinalizationCertificateResponseProto {
            digest: self.digest,
            certificate: Some(self.certificate.to_proto()),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ForkChoiceTipRequest;

impl ForkChoiceTipRequest {
    pub fn to_proto(self) -> ForkChoiceTipRequestProto { ForkChoiceTipRequestProto {} }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ApprovedBlockCandidate {
    pub block: BlockMessage,
    pub required_sigs: i32,
}

impl ApprovedBlockCandidate {
    pub fn from_proto(proto: ApprovedBlockCandidateProto) -> Result<Self, String> {
        Ok(Self {
            block: BlockMessage::from_proto(
                proto
                    .block
                    .ok_or_else(|| "Missing block field".to_string())?,
            )?,
            required_sigs: proto.required_sigs,
        })
    }

    pub fn to_proto(self) -> ApprovedBlockCandidateProto {
        ApprovedBlockCandidateProto {
            block: Some(self.block.to_proto()),
            required_sigs: self.required_sigs,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct UnapprovedBlock {
    pub candidate: ApprovedBlockCandidate,
    pub timestamp: i64,
    pub duration: i64,
}

impl UnapprovedBlock {
    pub fn from_proto(proto: UnapprovedBlockProto) -> Result<Self, String> {
        Ok(Self {
            candidate: ApprovedBlockCandidate::from_proto(
                proto
                    .candidate
                    .ok_or_else(|| "Missing candidate field".to_string())?,
            )?,
            timestamp: proto.timestamp,
            duration: proto.duration,
        })
    }

    pub fn to_proto(self) -> UnapprovedBlockProto {
        UnapprovedBlockProto {
            candidate: Some(self.candidate.to_proto()),
            timestamp: self.timestamp,
            duration: self.duration,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BlockApproval {
    pub candidate: ApprovedBlockCandidate,
    pub sig: Signature,
}

impl BlockApproval {
    pub fn from_proto(proto: BlockApprovalProto) -> Result<Self, String> {
        Ok(Self {
            candidate: ApprovedBlockCandidate::from_proto(
                proto
                    .candidate
                    .ok_or_else(|| "Missing candidate field".to_string())?,
            )?,
            sig: proto.sig.ok_or_else(|| "Missing sig field".to_string())?,
        })
    }

    pub fn to_proto(self) -> BlockApprovalProto {
        BlockApprovalProto {
            candidate: Some(self.candidate.to_proto()),
            sig: Some(self.sig),
        }
    }
}

/// Ask a peer for its cached finalized-floor values for the named blocks.
#[derive(Debug, Clone, PartialEq)]
pub struct FloorCacheRequest {
    pub hashes: Vec<ByteString>,
}

impl FloorCacheRequest {
    pub fn from_proto(proto: FloorCacheRequestProto) -> Self {
        Self {
            hashes: proto.hashes,
        }
    }

    pub fn to_proto(self) -> FloorCacheRequestProto {
        FloorCacheRequestProto {
            hashes: self.hashes,
        }
    }
}

/// One block's cached floor and frontier, as the responder derived them when
/// it validated the block.
#[derive(Debug, Clone, PartialEq)]
pub struct FloorCacheEntry {
    pub block_hash: ByteString,
    pub floor_hash: ByteString,
    pub frontier_hash: ByteString,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FloorCacheResponse {
    pub entries: Vec<FloorCacheEntry>,
    /// The shard's genesis block hash; empty when the responder does not
    /// hold it.
    pub genesis_hash: ByteString,
    /// The genesis block itself; absent when the responder does not hold
    /// it. Verified against `genesis_hash` by the receiver before storing.
    pub genesis_block: Option<BlockMessage>,
}

impl FloorCacheResponse {
    pub fn from_proto(proto: FloorCacheResponseProto) -> Self {
        Self {
            entries: proto
                .entries
                .into_iter()
                .map(|entry| FloorCacheEntry {
                    block_hash: entry.block_hash,
                    floor_hash: entry.floor_hash,
                    frontier_hash: entry.frontier_hash,
                })
                .collect(),
            genesis_hash: proto.genesis_hash,
            genesis_block: proto
                .genesis_block
                .and_then(|block| BlockMessage::from_proto(block).ok()),
        }
    }

    pub fn to_proto(self) -> FloorCacheResponseProto {
        FloorCacheResponseProto {
            entries: self
                .entries
                .into_iter()
                .map(|entry| FloorCacheEntryProto {
                    block_hash: entry.block_hash,
                    floor_hash: entry.floor_hash,
                    frontier_hash: entry.frontier_hash,
                })
                .collect(),
            genesis_hash: self.genesis_hash,
            genesis_block: self.genesis_block.map(|block| block.to_proto()),
        }
    }
}

/// The anchor's finalized floor and frontier, carried with the approved block
/// so a restored node can start deriving forward from them.
///
/// Each block is named by hash AND number: the number sizes the receiver's
/// download window, which it must fix before it holds any block to look up.
#[derive(Debug, Clone, PartialEq)]
pub struct FinalizedFloorSeed {
    pub floor_hash: ByteString,
    pub floor_number: i64,
    pub frontier_hash: ByteString,
    pub frontier_number: i64,
}

impl FinalizedFloorSeed {
    pub fn from_proto(proto: FinalizedFloorSeedProto) -> Self {
        Self {
            floor_hash: proto.floor_hash,
            floor_number: proto.floor_number,
            frontier_hash: proto.frontier_hash,
            frontier_number: proto.frontier_number,
        }
    }

    pub fn to_proto(self) -> FinalizedFloorSeedProto {
        FinalizedFloorSeedProto {
            floor_hash: self.floor_hash,
            floor_number: self.floor_number,
            frontier_hash: self.frontier_hash,
            frontier_number: self.frontier_number,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ApprovedBlock {
    pub candidate: ApprovedBlockCandidate,
    pub sigs: Vec<Signature>,
    /// Absent from peers that predate the seed, and from the non-trim response
    /// (a node syncing from genesis derives its own floors).
    pub floor_seed: Option<FinalizedFloorSeed>,
}

impl ApprovedBlock {
    pub fn from_proto(proto: ApprovedBlockProto) -> Result<Self, String> {
        Ok(Self {
            candidate: ApprovedBlockCandidate::from_proto(
                proto
                    .candidate
                    .ok_or_else(|| "Missing candidate field".to_string())?,
            )?,
            sigs: proto.sigs,
            floor_seed: proto.floor_seed.map(FinalizedFloorSeed::from_proto),
        })
    }

    pub fn to_proto(self) -> ApprovedBlockProto {
        ApprovedBlockProto {
            candidate: Some(self.candidate.to_proto()),
            sigs: self.sigs,
            floor_seed: self.floor_seed.map(FinalizedFloorSeed::to_proto),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NoApprovedBlockAvailable {
    pub identifier: String,
    pub node_identifier: String,
}

impl NoApprovedBlockAvailable {
    pub fn from_proto(proto: NoApprovedBlockAvailableProto) -> Self {
        Self {
            identifier: proto.identifier,
            node_identifier: proto.node_identifier,
        }
    }

    pub fn to_proto(self) -> NoApprovedBlockAvailableProto {
        NoApprovedBlockAvailableProto {
            identifier: self.identifier,
            node_identifier: self.node_identifier,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ApprovedBlockRequest {
    pub identifier: String,
    pub trim_state: bool,
}

impl ApprovedBlockRequest {
    pub fn from_proto(proto: ApprovedBlockRequestProto) -> Self {
        Self {
            identifier: proto.identifier,
            trim_state: proto.trim_state,
        }
    }

    pub fn to_proto(self) -> ApprovedBlockRequestProto {
        ApprovedBlockRequestProto {
            identifier: self.identifier,
            trim_state: self.trim_state,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BlockHashMessage {
    pub block_hash: ByteString,
    pub block_creator: ByteString,
}

impl BlockHashMessage {
    pub fn from_proto(proto: BlockHashMessageProto) -> Self {
        Self {
            block_hash: proto.hash,
            block_creator: proto.block_creator,
        }
    }

    pub fn to_proto(self) -> BlockHashMessageProto {
        BlockHashMessageProto {
            hash: self.block_hash,
            block_creator: self.block_creator,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BlockMessage {
    pub block_hash: ByteString,
    pub header: Header,
    pub body: Body,
    pub justifications: Vec<Justification>,
    pub sender: ByteString,
    pub seq_num: i32,
    pub sig: ByteString,
    pub sig_algorithm: String,
    pub shard_id: String,
    pub extra_bytes: ByteString,
    pub finalized_floor_certificate: Option<FinalizationCertificate>,
}

impl BlockMessage {
    pub fn from_proto(proto: BlockMessageProto) -> Result<Self, String> {
        Ok(Self {
            block_hash: proto.block_hash,
            header: Header::from_proto(
                proto
                    .header
                    .ok_or_else(|| "Missing header field".to_string())?,
            )?,
            body: Body::from_proto(proto.body.ok_or_else(|| "Missing body field".to_string())?)?,
            justifications: proto
                .justifications
                .into_iter()
                .map(|j| Justification::from_proto(j))
                .collect(),
            sender: proto.sender,
            seq_num: proto.seq_num,
            sig: proto.sig,
            sig_algorithm: proto.sig_algorithm,
            shard_id: proto.shard_id,
            extra_bytes: proto.extra_bytes,
            finalized_floor_certificate: proto
                .finalized_floor_certificate
                .map(FinalizationCertificate::from_proto)
                .transpose()?,
        })
    }

    pub fn to_proto(&self) -> BlockMessageProto {
        BlockMessageProto {
            block_hash: self.block_hash.clone(),
            header: Some(self.header.to_proto()),
            body: Some(self.body.to_proto()),
            justifications: self
                .justifications
                .clone()
                .into_iter()
                .map(|j| j.to_proto())
                .collect(),
            sender: self.sender.clone(),
            seq_num: self.seq_num,
            sig: self.sig.clone(),
            sig_algorithm: self.sig_algorithm.clone(),
            shard_id: self.shard_id.clone(),
            extra_bytes: self.extra_bytes.clone(),
            finalized_floor_certificate: self
                .finalized_floor_certificate
                .as_ref()
                .map(FinalizationCertificate::to_proto),
        }
    }

    pub fn to_string(self) -> String {
        PrettyPrinter::build_string(CasperMessage::BlockMessage(self), false)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    pub parents_hash_list: Vec<ByteString>,
    pub timestamp: i64,
    pub version: i64,
    pub extra_bytes: ByteString,
    pub sender_bond_generation: Option<BondGeneration>,
    pub objective_equivocation_evidence_delta: Vec<ObjectiveEquivocationEvidence>,
    pub finalized_floor: Option<FinalizedFloorCommitment>,
}

impl Header {
    pub fn from_proto(proto: HeaderProto) -> Result<Self, String> {
        let sender_bond_generation = proto
            .sender_bond_generation
            .map(BondGeneration::try_from)
            .transpose()
            .map_err(|error| error.to_string())?;
        let objective_equivocation_evidence_delta = proto
            .objective_equivocation_evidence_delta
            .into_iter()
            .map(ObjectiveEquivocationEvidence::from_proto)
            .collect::<Result<Vec<_>, _>>()?;
        if objective_equivocation_evidence_delta
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        {
            return Err(
                "objective equivocation evidence delta must be strictly sorted and unique"
                    .to_string(),
            );
        }
        Ok(Self {
            parents_hash_list: proto.parents_hash_list,
            timestamp: proto.timestamp,
            version: proto.version,
            extra_bytes: proto.extra_bytes,
            sender_bond_generation,
            objective_equivocation_evidence_delta,
            finalized_floor: proto
                .finalized_floor
                .map(FinalizedFloorCommitment::from_proto)
                .transpose()?,
        })
    }

    pub fn to_proto(&self) -> HeaderProto {
        HeaderProto {
            parents_hash_list: self.parents_hash_list.clone(),
            timestamp: self.timestamp,
            version: self.version,
            extra_bytes: self.extra_bytes.clone(),
            sender_bond_generation: self.sender_bond_generation.map(BondGeneration::get),
            objective_equivocation_evidence_delta: self
                .objective_equivocation_evidence_delta
                .iter()
                .map(ObjectiveEquivocationEvidence::to_proto)
                .collect(),
            finalized_floor: self
                .finalized_floor
                .as_ref()
                .map(FinalizedFloorCommitment::to_proto),
        }
    }
}

#[derive(
    Clone,
    Debug,
    Eq,
    PartialEq,
    Hash,
    serde::Serialize,
    serde::Deserialize
)]
pub struct FinalizedFloorCommitment {
    #[serde(with = "shared::rust::serde_bytes")]
    pub floor_hash: ByteString,
    #[serde(with = "shared::rust::serde_bytes")]
    pub floor_post_state_hash: ByteString,
    #[serde(with = "shared::rust::serde_bytes")]
    pub certificate_digest: ByteString,
    #[serde(with = "shared::rust::serde_bytes")]
    pub authority_context_digest: ByteString,
}

impl FinalizedFloorCommitment {
    pub fn from_proto(proto: FinalizedFloorCommitmentProto) -> Result<Self, String> {
        let commitment = Self {
            floor_hash: proto.floor_hash,
            floor_post_state_hash: proto.floor_post_state_hash,
            certificate_digest: proto.certificate_digest,
            authority_context_digest: proto.authority_context_digest,
        };
        commitment.validate_shape()?;
        Ok(commitment)
    }

    pub fn to_proto(&self) -> FinalizedFloorCommitmentProto {
        FinalizedFloorCommitmentProto {
            floor_hash: self.floor_hash.clone(),
            floor_post_state_hash: self.floor_post_state_hash.clone(),
            certificate_digest: self.certificate_digest.clone(),
            authority_context_digest: self.authority_context_digest.clone(),
        }
    }

    pub fn validate_shape(&self) -> Result<(), String> {
        for (name, value) in [
            ("floor hash", &self.floor_hash),
            ("floor post-state hash", &self.floor_post_state_hash),
            ("certificate digest", &self.certificate_digest),
            ("authority-context digest", &self.authority_context_digest),
        ] {
            if value.len() != block_hash::LENGTH {
                return Err(format!(
                    "finalized-floor commitment {name} must be {} bytes, got {}",
                    block_hash::LENGTH,
                    value.len()
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FinalizationCertificate {
    pub schema_version: u32,
    pub protocol_version: i64,
    pub shard_id: String,
    pub genesis_hash: BlockHashSerde,
    pub predecessor_floor_hash: BlockHashSerde,
    pub predecessor_certificate_digest: BlockHashSerde,
    pub predecessor_certificate_block_hash: BlockHashSerde,
    pub target_floor_hash: BlockHashSerde,
    pub target_post_state_hash: BlockHashSerde,
    pub target_block_number: i64,
    pub fault_tolerance_numerator: i64,
    pub fault_tolerance_denominator: i64,
    pub exact_latest_messages: std::collections::BTreeMap<ValidatorSerde, BlockHashSerde>,
    pub authority_context_digest: BlockHashSerde,
    pub supporting_manifest_digest: BlockHashSerde,
    pub finalized_manifest_digest: BlockHashSerde,
    pub supporting_block_count: u32,
    pub finalized_block_count: u32,
}

impl FinalizationCertificate {
    pub const SCHEMA_VERSION: u32 = 4;
    pub const MAX_ENCODED_BYTES: usize = 2 * 1024 * 1024;
    pub const MAX_EXACT_LATEST_MESSAGES: usize = 10_000;
    pub const MAX_SUPPORTING_BLOCKS: usize = 262_144;
    pub const MAX_FINALIZED_BLOCKS: usize = 262_144;
    pub const MAX_DAG_VISITS_PER_VERIFICATION: usize = 1_048_576;
    pub const MAX_SHARD_ID_BYTES: usize = 256;

    pub fn from_proto(proto: FinalizationCertificateProto) -> Result<Self, String> {
        if proto.encoded_len() > Self::MAX_ENCODED_BYTES {
            return Err(format!(
                "finalization certificate exceeds {} encoded bytes",
                Self::MAX_ENCODED_BYTES
            ));
        }
        if proto.exact_latest_messages.len() > Self::MAX_EXACT_LATEST_MESSAGES {
            return Err(format!(
                "finalization certificate has more than {} latest messages",
                Self::MAX_EXACT_LATEST_MESSAGES
            ));
        }
        let mut exact_latest_messages = std::collections::BTreeMap::new();
        for justification in proto.exact_latest_messages {
            let justification = Justification::from_proto(justification);
            if exact_latest_messages
                .insert(
                    ValidatorSerde(justification.validator),
                    BlockHashSerde(justification.latest_block_hash),
                )
                .is_some()
            {
                return Err(
                    "finalization certificate exact latest messages contain duplicate validators"
                        .to_string(),
                );
            }
        }
        let certificate = Self {
            schema_version: proto.schema_version,
            protocol_version: proto.protocol_version,
            shard_id: proto.shard_id,
            genesis_hash: BlockHashSerde(proto.genesis_hash),
            predecessor_floor_hash: BlockHashSerde(proto.predecessor_floor_hash),
            predecessor_certificate_digest: BlockHashSerde(proto.predecessor_certificate_digest),
            predecessor_certificate_block_hash: BlockHashSerde(
                proto.predecessor_certificate_block_hash,
            ),
            target_floor_hash: BlockHashSerde(proto.target_floor_hash),
            target_post_state_hash: BlockHashSerde(proto.target_post_state_hash),
            target_block_number: proto.target_block_number,
            fault_tolerance_numerator: proto.fault_tolerance_numerator,
            fault_tolerance_denominator: proto.fault_tolerance_denominator,
            exact_latest_messages,
            authority_context_digest: BlockHashSerde(proto.authority_context_digest),
            supporting_manifest_digest: BlockHashSerde(proto.supporting_manifest_digest),
            finalized_manifest_digest: BlockHashSerde(proto.finalized_manifest_digest),
            supporting_block_count: proto.supporting_block_count,
            finalized_block_count: proto.finalized_block_count,
        };
        certificate.validate_shape()?;
        Ok(certificate)
    }

    pub fn to_proto(&self) -> FinalizationCertificateProto {
        FinalizationCertificateProto {
            schema_version: self.schema_version,
            protocol_version: self.protocol_version,
            shard_id: self.shard_id.clone(),
            genesis_hash: self.genesis_hash.0.clone(),
            predecessor_floor_hash: self.predecessor_floor_hash.0.clone(),
            predecessor_certificate_digest: self.predecessor_certificate_digest.0.clone(),
            predecessor_certificate_block_hash: self.predecessor_certificate_block_hash.0.clone(),
            target_floor_hash: self.target_floor_hash.0.clone(),
            target_post_state_hash: self.target_post_state_hash.0.clone(),
            target_block_number: self.target_block_number,
            fault_tolerance_numerator: self.fault_tolerance_numerator,
            fault_tolerance_denominator: self.fault_tolerance_denominator,
            exact_latest_messages: self
                .exact_latest_messages
                .iter()
                .map(|(validator, block_hash)| JustificationProto {
                    validator: validator.0.clone(),
                    latest_block_hash: block_hash.0.clone(),
                })
                .collect(),
            authority_context_digest: self.authority_context_digest.0.clone(),
            supporting_manifest_digest: self.supporting_manifest_digest.0.clone(),
            finalized_manifest_digest: self.finalized_manifest_digest.0.clone(),
            supporting_block_count: self.supporting_block_count,
            finalized_block_count: self.finalized_block_count,
        }
    }

    pub fn digest(&self) -> BlockHash {
        let mut bytes = Vec::new();
        append_certificate_bytes(&mut bytes, b"f1r3fly-finalization-certificate-v4");
        bytes.extend_from_slice(&self.schema_version.to_be_bytes());
        bytes.extend_from_slice(&self.protocol_version.to_be_bytes());
        append_certificate_bytes(&mut bytes, self.shard_id.as_bytes());
        append_certificate_bytes(&mut bytes, &self.genesis_hash.0);
        append_certificate_bytes(&mut bytes, &self.predecessor_floor_hash.0);
        append_certificate_bytes(&mut bytes, &self.predecessor_certificate_digest.0);
        append_certificate_bytes(&mut bytes, &self.predecessor_certificate_block_hash.0);
        append_certificate_bytes(&mut bytes, &self.target_floor_hash.0);
        append_certificate_bytes(&mut bytes, &self.target_post_state_hash.0);
        bytes.extend_from_slice(&self.target_block_number.to_be_bytes());
        bytes.extend_from_slice(&self.fault_tolerance_numerator.to_be_bytes());
        bytes.extend_from_slice(&self.fault_tolerance_denominator.to_be_bytes());
        bytes.extend_from_slice(&(self.exact_latest_messages.len() as u64).to_be_bytes());
        for (validator, block_hash) in &self.exact_latest_messages {
            append_certificate_bytes(&mut bytes, &validator.0);
            append_certificate_bytes(&mut bytes, &block_hash.0);
        }
        append_certificate_bytes(&mut bytes, &self.authority_context_digest.0);
        append_certificate_bytes(&mut bytes, &self.supporting_manifest_digest.0);
        append_certificate_bytes(&mut bytes, &self.finalized_manifest_digest.0);
        bytes.extend_from_slice(&self.supporting_block_count.to_be_bytes());
        bytes.extend_from_slice(&self.finalized_block_count.to_be_bytes());
        Blake2b256::hash(bytes).into()
    }

    pub fn manifest_digest(
        domain: &[u8],
        hashes: &std::collections::BTreeSet<BlockHashSerde>,
    ) -> BlockHashSerde {
        let mut bytes = Vec::with_capacity(
            domain.len() + 16 + hashes.len().saturating_mul(block_hash::LENGTH + 8),
        );
        append_certificate_bytes(&mut bytes, b"f1r3fly-finalization-manifest-v1");
        append_certificate_bytes(&mut bytes, domain);
        bytes.extend_from_slice(&(hashes.len() as u64).to_be_bytes());
        for hash in hashes {
            append_certificate_bytes(&mut bytes, &hash.0);
        }
        BlockHashSerde(Blake2b256::hash(bytes).into())
    }

    pub fn supporting_digest(
        hashes: &std::collections::BTreeSet<BlockHashSerde>,
    ) -> BlockHashSerde {
        Self::manifest_digest(b"supporting", hashes)
    }

    pub fn finalized_digest(hashes: &std::collections::BTreeSet<BlockHashSerde>) -> BlockHashSerde {
        Self::manifest_digest(b"finalized", hashes)
    }

    pub fn commitment(
        &self,
        candidate_authority_context_digest: ByteString,
    ) -> FinalizedFloorCommitment {
        FinalizedFloorCommitment {
            floor_hash: self.target_floor_hash.0.clone(),
            floor_post_state_hash: self.target_post_state_hash.0.clone(),
            certificate_digest: self.digest(),
            authority_context_digest: candidate_authority_context_digest,
        }
    }

    pub fn validate_shape(&self) -> Result<(), String> {
        if self.schema_version != Self::SCHEMA_VERSION {
            return Err(format!(
                "unsupported finalization certificate schema {}; expected {}",
                self.schema_version,
                Self::SCHEMA_VERSION
            ));
        }
        if self.protocol_version <= 0
            || self.shard_id.is_empty()
            || self.shard_id.len() > Self::MAX_SHARD_ID_BYTES
        {
            return Err("finalization certificate protocol and shard must be present".to_string());
        }
        if self.exact_latest_messages.len() > Self::MAX_EXACT_LATEST_MESSAGES {
            return Err(format!(
                "finalization certificate has more than {} latest messages",
                Self::MAX_EXACT_LATEST_MESSAGES
            ));
        }
        if self.supporting_block_count == 0
            || !usize::try_from(self.supporting_block_count)
                .is_ok_and(|count| count <= Self::MAX_SUPPORTING_BLOCKS)
            || self.finalized_block_count == 0
            || !usize::try_from(self.finalized_block_count)
                .is_ok_and(|count| count <= Self::MAX_FINALIZED_BLOCKS)
            || self.finalized_block_count > self.supporting_block_count
        {
            return Err("finalization certificate manifest counts are invalid".to_string());
        }
        if self.target_block_number < 0
            || self.fault_tolerance_denominator <= 0
            || self.fault_tolerance_numerator < -self.fault_tolerance_denominator
            || self.fault_tolerance_numerator > self.fault_tolerance_denominator
        {
            return Err("finalization certificate numeric domain is invalid".to_string());
        }
        for (name, value) in [
            ("genesis hash", &self.genesis_hash.0),
            ("predecessor floor hash", &self.predecessor_floor_hash.0),
            (
                "predecessor certificate digest",
                &self.predecessor_certificate_digest.0,
            ),
            (
                "predecessor certificate block hash",
                &self.predecessor_certificate_block_hash.0,
            ),
            ("target floor hash", &self.target_floor_hash.0),
            ("target post-state hash", &self.target_post_state_hash.0),
            ("authority-context digest", &self.authority_context_digest.0),
            (
                "supporting-manifest digest",
                &self.supporting_manifest_digest.0,
            ),
            (
                "finalized-manifest digest",
                &self.finalized_manifest_digest.0,
            ),
        ] {
            if value.len() != block_hash::LENGTH {
                return Err(format!(
                    "finalization certificate {name} must be {} bytes, got {}",
                    block_hash::LENGTH,
                    value.len()
                ));
            }
        }
        if self.exact_latest_messages.is_empty()
            || self.exact_latest_messages.iter().any(|(validator, hash)| {
                validator.0.len() != validator::LENGTH || hash.0.len() != block_hash::LENGTH
            })
            || (self
                .predecessor_certificate_digest
                .0
                .iter()
                .all(|byte| *byte == 0)
                != self
                    .predecessor_certificate_block_hash
                    .0
                    .iter()
                    .all(|byte| *byte == 0))
            || (!self
                .predecessor_certificate_digest
                .0
                .iter()
                .all(|byte| *byte == 0)
                && self.predecessor_certificate_block_hash.0 == self.target_floor_hash.0)
            || self.to_proto().encoded_len() > Self::MAX_ENCODED_BYTES
        {
            return Err("finalization certificate structure is invalid".to_string());
        }
        Ok(())
    }

    pub fn validate_commitment(&self, commitment: &FinalizedFloorCommitment) -> Result<(), String> {
        commitment.validate_shape()?;
        self.validate_shape()?;
        if commitment.floor_hash != self.target_floor_hash.0
            || commitment.floor_post_state_hash != self.target_post_state_hash.0
            || commitment.certificate_digest != self.digest()
        {
            return Err(
                "finalization certificate does not match the signed floor commitment".to_string(),
            );
        }
        Ok(())
    }
}

fn append_certificate_bytes(output: &mut Vec<u8>, value: &[u8]) {
    output.extend_from_slice(&(value.len() as u64).to_be_bytes());
    output.extend_from_slice(value);
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize
)]
pub struct ObjectiveEquivocationEvidence {
    #[serde(with = "shared::rust::serde_bytes")]
    pub validator: ByteString,
    pub bond_generation: BondGeneration,
    pub sequence_number: i32,
    #[serde(with = "shared::rust::serde_bytes")]
    pub first_block_hash: ByteString,
    #[serde(with = "shared::rust::serde_bytes")]
    pub second_block_hash: ByteString,
}

impl ObjectiveEquivocationEvidence {
    pub fn new(
        validator: ByteString,
        bond_generation: BondGeneration,
        sequence_number: i32,
        first_block_hash: ByteString,
        second_block_hash: ByteString,
    ) -> Result<Self, String> {
        if validator.len() != validator::LENGTH {
            return Err(format!(
                "objective evidence validator must be {} bytes, got {}",
                validator::LENGTH,
                validator.len()
            ));
        }
        if sequence_number < 0 {
            return Err(format!(
                "objective evidence sequence number must be nonnegative, got {sequence_number}"
            ));
        }
        if first_block_hash.len() != block_hash::LENGTH
            || second_block_hash.len() != block_hash::LENGTH
        {
            return Err(format!(
                "objective evidence block hashes must be {} bytes",
                block_hash::LENGTH
            ));
        }
        if first_block_hash == second_block_hash {
            return Err("objective evidence must identify two distinct blocks".to_string());
        }
        let (first_block_hash, second_block_hash) = if first_block_hash < second_block_hash {
            (first_block_hash, second_block_hash)
        } else {
            (second_block_hash, first_block_hash)
        };
        Ok(Self {
            validator,
            bond_generation,
            sequence_number,
            first_block_hash,
            second_block_hash,
        })
    }

    pub fn from_proto(proto: ObjectiveEquivocationEvidenceProto) -> Result<Self, String> {
        Self::new(
            proto.validator,
            BondGeneration::try_from(proto.bond_generation).map_err(|error| error.to_string())?,
            proto.sequence_number,
            proto.first_block_hash,
            proto.second_block_hash,
        )
    }

    pub fn to_proto(&self) -> ObjectiveEquivocationEvidenceProto {
        ObjectiveEquivocationEvidenceProto {
            validator: self.validator.clone(),
            bond_generation: self.bond_generation.get(),
            sequence_number: self.sequence_number,
            first_block_hash: self.first_block_hash.clone(),
            second_block_hash: self.second_block_hash.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RejectedDeployReason {
    #[default]
    Unspecified,
    MergeConflict,
    DuplicateOccurrence,
    CollateralChainDrop,
    ValidityWindowClosed,
}

impl RejectedDeployReason {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unspecified => "unspecified",
            Self::MergeConflict => "merge_conflict",
            Self::DuplicateOccurrence => "duplicate_occurrence",
            Self::CollateralChainDrop => "collateral_chain_drop",
            Self::ValidityWindowClosed => "validity_window_closed",
        }
    }

    pub fn canonical_join(self, other: Self) -> Self {
        use RejectedDeployReason::{
            CollateralChainDrop, DuplicateOccurrence, MergeConflict, Unspecified,
            ValidityWindowClosed,
        };

        match (self, other) {
            (DuplicateOccurrence, _) | (_, DuplicateOccurrence) => DuplicateOccurrence,
            (ValidityWindowClosed, _) | (_, ValidityWindowClosed) => ValidityWindowClosed,
            (MergeConflict, _) | (_, MergeConflict) => MergeConflict,
            (CollateralChainDrop, _) | (_, CollateralChainDrop) => CollateralChainDrop,
            (Unspecified, Unspecified) => Unspecified,
        }
    }

    fn from_proto(value: i32) -> Self {
        match RejectedDeployReasonProto::try_from(value)
            .unwrap_or(RejectedDeployReasonProto::RejectedDeployReasonUnspecified)
        {
            RejectedDeployReasonProto::RejectedDeployReasonUnspecified => Self::Unspecified,
            RejectedDeployReasonProto::RejectedDeployReasonMergeConflict => Self::MergeConflict,
            RejectedDeployReasonProto::RejectedDeployReasonDuplicateOccurrence => {
                Self::DuplicateOccurrence
            }
            RejectedDeployReasonProto::RejectedDeployReasonCollateralChainDrop => {
                Self::CollateralChainDrop
            }
            RejectedDeployReasonProto::RejectedDeployReasonValidityWindowClosed => {
                Self::ValidityWindowClosed
            }
        }
    }

    fn to_proto(self) -> i32 {
        match self {
            Self::Unspecified => RejectedDeployReasonProto::RejectedDeployReasonUnspecified as i32,
            Self::MergeConflict => {
                RejectedDeployReasonProto::RejectedDeployReasonMergeConflict as i32
            }
            Self::DuplicateOccurrence => {
                RejectedDeployReasonProto::RejectedDeployReasonDuplicateOccurrence as i32
            }
            Self::CollateralChainDrop => {
                RejectedDeployReasonProto::RejectedDeployReasonCollateralChainDrop as i32
            }
            Self::ValidityWindowClosed => {
                RejectedDeployReasonProto::RejectedDeployReasonValidityWindowClosed as i32
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RejectedDeploy {
    deploy_id: DeployLookupId,
    pub source_block_hash: BlockHash,
    pub reason: RejectedDeployReason,
}

impl RejectedDeploy {
    pub fn legacy(sig: ByteString) -> Self {
        Self {
            deploy_id: DeployLookupId::Legacy(LegacyDeploySignature::new(sig.to_vec())),
            source_block_hash: ByteString::new(),
            reason: RejectedDeployReason::Unspecified,
        }
    }

    pub fn occurrence_legacy(
        sig: LegacyDeploySignature,
        source_block_hash: BlockHash,
        reason: RejectedDeployReason,
    ) -> Self {
        Self {
            deploy_id: DeployLookupId::Legacy(sig),
            source_block_hash,
            reason,
        }
    }

    pub fn occurrence_v6(
        deploy_id: DeployIdV6,
        source_block_hash: BlockHash,
        reason: RejectedDeployReason,
    ) -> Self {
        Self {
            deploy_id: DeployLookupId::V6(deploy_id),
            source_block_hash,
            reason,
        }
    }

    pub fn deploy_id(&self) -> &[u8] { self.deploy_id.as_bytes() }

    pub fn typed_deploy_id(&self) -> &DeployLookupId { &self.deploy_id }

    pub fn has_provenance(&self) -> bool { !self.source_block_hash.is_empty() }

    pub fn is_duplicate(&self) -> bool { self.reason == RejectedDeployReason::DuplicateOccurrence }

    pub fn from_proto(proto: RejectedDeployProto) -> Result<Self, String> {
        let deploy_id = match (proto.sig.is_empty(), proto.deploy_id_v6.is_empty()) {
            (false, true) => DeployLookupId::Legacy(LegacyDeploySignature::new(proto.sig.to_vec())),
            (true, false) => DeployLookupId::V6(
                DeployIdV6::try_from(proto.deploy_id_v6.as_ref())
                    .map_err(|error| error.to_string())?,
            ),
            (false, false) => {
                return Err(
                    "rejected deploy cannot contain both legacy and v6 identities".to_string(),
                );
            }
            (true, true) => return Err("rejected deploy identity is missing".to_string()),
        };
        if !proto.source_block_hash.is_empty()
            && !proto.carrier.is_empty()
            && proto.source_block_hash != proto.carrier
        {
            return Err("rejected deploy source and compatibility carrier disagree".to_string());
        }
        let source_block_hash = if proto.source_block_hash.is_empty() {
            proto.carrier
        } else {
            proto.source_block_hash
        };
        let reason = RejectedDeployReason::from_proto(proto.reason);
        let reason = if reason == RejectedDeployReason::Unspecified && proto.duplicate {
            RejectedDeployReason::DuplicateOccurrence
        } else if reason == RejectedDeployReason::Unspecified && !source_block_hash.is_empty() {
            RejectedDeployReason::MergeConflict
        } else {
            reason
        };
        if proto.duplicate != (reason == RejectedDeployReason::DuplicateOccurrence)
            && proto.reason != RejectedDeployReasonProto::RejectedDeployReasonUnspecified as i32
        {
            return Err(
                "rejected deploy reason and compatibility duplicate flag disagree".to_string(),
            );
        }
        Ok(Self {
            deploy_id,
            source_block_hash,
            reason,
        })
    }

    pub fn to_proto(self) -> RejectedDeployProto {
        let duplicate = self.is_duplicate();
        let (sig, deploy_id_v6) = match self.deploy_id {
            DeployLookupId::Legacy(sig) => (ByteString::from(sig.into_bytes()), ByteString::new()),
            DeployLookupId::V6(deploy_id) => (
                ByteString::new(),
                ByteString::copy_from_slice(deploy_id.as_ref()),
            ),
        };
        RejectedDeployProto {
            sig,
            duplicate,
            carrier: self.source_block_hash.clone(),
            source_block_hash: self.source_block_hash,
            reason: self.reason.to_proto(),
            deploy_id_v6,
        }
    }
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize
)]
pub struct StateEffectId {
    #[serde(with = "shared::rust::serde_bytes")]
    pub source_block_hash: ByteString,
    pub execution_index: u32,
}

impl StateEffectId {
    pub fn from_proto(proto: StateEffectIdProto) -> Self {
        Self {
            source_block_hash: proto.source_block_hash,
            execution_index: proto.execution_index,
        }
    }

    pub fn to_proto(&self) -> StateEffectIdProto {
        StateEffectIdProto {
            source_block_hash: self.source_block_hash.clone(),
            execution_index: self.execution_index,
        }
    }

    pub fn validate_canonical_sequence(effects: &[Self], field: &str) -> Result<(), String> {
        if let Some(effect) = effects
            .iter()
            .find(|effect| effect.source_block_hash.len() != block_hash::LENGTH)
        {
            return Err(format!(
                "{field} contains a {}-byte source block hash at execution index {}, expected {} bytes",
                effect.source_block_hash.len(),
                effect.execution_index,
                block_hash::LENGTH
            ));
        }
        if effects.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(format!(
                "{field} must be strictly ordered without duplicate effect identities"
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    pub state: F1r3flyState,
    pub deploys: Vec<ProcessedDeploy>,
    pub rejected_deploys: Vec<RejectedDeploy>,
    pub rejected_state_effects: Vec<StateEffectId>,
    pub applied_state_effects: Vec<StateEffectId>,
    pub system_deploys: Vec<ProcessedSystemDeploy>,
    pub extra_bytes: ByteString,
    pub applied_from_scope: Vec<ByteString>,
    pub merge_base: ByteString,
}

impl Body {
    pub fn from_proto(proto: BodyProto) -> Result<Self, String> {
        let rejected_state_effects = proto
            .rejected_state_effects
            .into_iter()
            .map(StateEffectId::from_proto)
            .collect::<Vec<_>>();
        StateEffectId::validate_canonical_sequence(
            &rejected_state_effects,
            "rejectedStateEffects",
        )?;
        let applied_state_effects = proto
            .applied_state_effects
            .into_iter()
            .map(StateEffectId::from_proto)
            .collect::<Vec<_>>();
        StateEffectId::validate_canonical_sequence(&applied_state_effects, "appliedStateEffects")?;
        Ok(Self {
            state: F1r3flyState::from_proto(
                proto
                    .state
                    .ok_or_else(|| "Missing state field".to_string())?,
            )?,
            deploys: proto
                .deploys
                .into_iter()
                .map(|d| ProcessedDeploy::from_proto(d))
                .collect::<Result<Vec<ProcessedDeploy>, String>>()?,
            rejected_deploys: proto
                .rejected_deploys
                .into_iter()
                .map(|r| RejectedDeploy::from_proto(r))
                .collect::<Result<Vec<_>, _>>()?,
            rejected_state_effects,
            applied_state_effects,
            system_deploys: proto
                .system_deploys
                .into_iter()
                .map(|s| ProcessedSystemDeploy::from_proto(s))
                .collect::<Result<Vec<ProcessedSystemDeploy>, String>>()?,
            extra_bytes: proto.extra_bytes,
            applied_from_scope: proto.applied_from_scope,
            merge_base: proto.merge_base,
        })
    }

    pub fn to_proto(&self) -> BodyProto {
        BodyProto {
            state: Some(self.state.to_proto()),
            deploys: self
                .deploys
                .clone()
                .into_iter()
                .map(|d| d.to_proto())
                .collect(),
            rejected_deploys: self
                .rejected_deploys
                .clone()
                .into_iter()
                .map(|r| r.to_proto())
                .collect(),
            rejected_state_effects: self
                .rejected_state_effects
                .iter()
                .map(StateEffectId::to_proto)
                .collect(),
            applied_state_effects: self
                .applied_state_effects
                .iter()
                .map(StateEffectId::to_proto)
                .collect(),
            system_deploys: self
                .system_deploys
                .clone()
                .into_iter()
                .map(|s| s.to_proto())
                .collect(),
            extra_bytes: self.extra_bytes.clone(),
            applied_from_scope: self.applied_from_scope.clone(),
            merge_base: self.merge_base.clone(),
        }
    }
}

#[derive(
    Debug,
    Clone,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    serde::Serialize,
    serde::Deserialize,
    Hash
)]
pub struct Justification {
    #[serde(with = "shared::rust::serde_bytes")]
    pub validator: ByteString,
    #[serde(with = "shared::rust::serde_bytes")]
    pub latest_block_hash: ByteString,
}

impl Justification {
    pub fn from_proto(proto: JustificationProto) -> Self {
        Self {
            validator: proto.validator,
            latest_block_hash: proto.latest_block_hash,
        }
    }

    pub fn to_proto(&self) -> JustificationProto {
        JustificationProto {
            validator: self.validator.clone(),
            latest_block_hash: self.latest_block_hash.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct F1r3flyState {
    pub pre_state_hash: ByteString,
    pub post_state_hash: ByteString,
    pub bonds: Vec<Bond>,
    pub bond_generations: Vec<ValidatorBondGeneration>,
    pub active_validators: Vec<ByteString>,
    pub block_number: i64,
}

impl F1r3flyState {
    pub fn from_proto(proto: RChainStateProto) -> Result<Self, String> {
        let bond_generations = proto
            .bond_generations
            .into_iter()
            .map(ValidatorBondGeneration::from_proto)
            .collect::<Result<Vec<_>, _>>()?;
        if bond_generations.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err("bond generation cache must be strictly sorted and unique".to_string());
        }
        if proto
            .active_validators
            .iter()
            .any(|validator| validator.len() != validator::LENGTH)
        {
            return Err(format!(
                "active validator keys must be {} bytes",
                validator::LENGTH
            ));
        }
        if proto
            .active_validators
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        {
            return Err("active validator cache must be strictly sorted and unique".to_string());
        }
        Ok(Self {
            pre_state_hash: proto.pre_state_hash,
            post_state_hash: proto.post_state_hash,
            bonds: proto
                .bonds
                .into_iter()
                .map(|b| Bond::from_proto(b))
                .collect(),
            bond_generations,
            active_validators: proto.active_validators,
            block_number: proto.block_number,
        })
    }

    pub fn to_proto(&self) -> RChainStateProto {
        RChainStateProto {
            pre_state_hash: self.pre_state_hash.clone(),
            post_state_hash: self.post_state_hash.clone(),
            bonds: self
                .bonds
                .clone()
                .into_iter()
                .map(|b| Bond::to_proto(b))
                .collect(),
            bond_generations: self
                .bond_generations
                .iter()
                .map(ValidatorBondGeneration::to_proto)
                .collect(),
            active_validators: self.active_validators.clone(),
            block_number: self.block_number,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ValidatorBondGeneration {
    pub validator: ByteString,
    pub generation: BondGeneration,
}

impl ValidatorBondGeneration {
    pub fn from_proto(proto: ValidatorBondGenerationProto) -> Result<Self, String> {
        if proto.validator.len() != validator::LENGTH {
            return Err(format!(
                "bond-generation validator must be {} bytes, got {}",
                validator::LENGTH,
                proto.validator.len()
            ));
        }
        Ok(Self {
            validator: proto.validator,
            generation: BondGeneration::try_from(proto.generation)
                .map_err(|error| error.to_string())?,
        })
    }

    pub fn to_proto(&self) -> ValidatorBondGenerationProto {
        ValidatorBondGenerationProto {
            validator: self.validator.clone(),
            generation: self.generation.get(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProcessedDeploy {
    pub deploy: Signed<DeployData>,
    pub envelope_commitment: ByteString,
    pub cost: PCost,
    pub deploy_log: Vec<Event>,
    pub is_failed: bool,
    pub system_deploy_error: Option<String>,
    /// Additional cosigners beyond the primary (`deploy.pk` / `deploy.sig`).
    /// Empty for legacy single-signature deploys. Round-trips through
    /// `DeployDataProto.cosigners` (proto field 14 on `deploy`).
    pub cosigners: Vec<crate::casper::CompoundSigner>,
    /// M-of-N quorum threshold. Protocol v6 uses an explicit value in
    /// `1..=N`. Zero is reserved for the pre-v6 N-of-N encoding.
    pub cosigner_threshold: i32,
    pub pre_state_hash: ByteString,
    pub post_state_hash: ByteString,
    pub authority_funding_certificate: Option<CostAuthorityFundingCertificateProto>,
    pub authority_cost_witness: Option<CostAuthorityWitnessProto>,
    pub admission_status: DeployAdmissionStatus,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeployAdmissionStatus {
    #[default]
    Executed,
    Rejected,
}

impl DeployAdmissionStatus {
    fn from_proto(value: i32) -> Self {
        match DeployAdmissionStatusProto::try_from(value)
            .unwrap_or(DeployAdmissionStatusProto::DeployAdmissionStatusExecuted)
        {
            DeployAdmissionStatusProto::DeployAdmissionStatusExecuted => Self::Executed,
            DeployAdmissionStatusProto::DeployAdmissionStatusRejected => Self::Rejected,
        }
    }

    fn to_proto(self) -> i32 {
        match self {
            Self::Executed => DeployAdmissionStatusProto::DeployAdmissionStatusExecuted as i32,
            Self::Rejected => DeployAdmissionStatusProto::DeployAdmissionStatusRejected as i32,
        }
    }
}

impl ProcessedDeploy {
    pub const FUNDING_ADMISSION_REJECTION: &'static str =
        "Cost-accounting funding admission rejected";

    // D3 (DR-9): `try_refund_amount`/`refund_amount` are REMOVED — there is no
    // escrow to refund. The deploy's `cost` is the per-COMM token count. Ordinary
    // block execution reserves the maximum certified authority before retaining
    // the transition, then settles the realized cost through canonical
    // SystemVault custody and prepaid located stacks.

    pub fn empty(deploy: Signed<DeployData>) -> Self {
        Self {
            deploy,
            envelope_commitment: ByteString::new(),
            cost: PCost { cost: 0 },
            deploy_log: Vec::new(),
            is_failed: false,
            system_deploy_error: None,
            cosigners: Vec::new(),
            cosigner_threshold: 0,
            pre_state_hash: ByteString::new(),
            post_state_hash: ByteString::new(),
            authority_funding_certificate: None,
            authority_cost_witness: None,
            admission_status: DeployAdmissionStatus::Executed,
        }
    }

    /// Construct an empty processed-deploy record from a `Cosigned<DeployData>`
    /// envelope, preserving the full cosigner list. Used by error-envelope
    /// construction paths in the multi-sig runtime fan-out where a deploy
    /// fails BEFORE evaluation begins.
    pub fn empty_from_cosigned(
        cosigned: &crypto::rust::signatures::signed::Cosigned<DeployData>,
    ) -> Self {
        let primary_index = if cosigned.is_envelope_bound() {
            cosigned
                .signers()
                .iter()
                .position(|signer| !signer.sig.is_empty())
                .expect("validated protocol-v6 envelope has a selected signer")
        } else {
            0
        };
        let primary = &cosigned.signers()[primary_index];
        let deploy = Signed {
            data: cosigned.data.clone(),
            pk: primary.pk.clone(),
            sig: primary.sig.clone(),
            sig_algorithm: primary.sig_algorithm.clone(),
        };
        let is_compound = cosigned.is_compound();
        let cosigners = if is_compound {
            cosigned
                .signers()
                .iter()
                .enumerate()
                .filter(|(index, _)| *index != primary_index)
                .map(|(_, c)| crate::casper::CompoundSigner {
                    pk: c.pk.bytes.clone().into(),
                    sig: c.sig.clone(),
                    sig_algorithm: c.sig_algorithm.name(),
                })
                .collect()
        } else {
            Vec::new()
        };
        Self {
            deploy,
            envelope_commitment: if cosigned.is_envelope_bound() {
                cosigned
                    .envelope_commitment()
                    .expect("envelope-bound Cosigned invariant")
            } else {
                ByteString::new()
            },
            cost: PCost { cost: 0 },
            deploy_log: Vec::new(),
            is_failed: false,
            system_deploy_error: None,
            cosigners,
            cosigner_threshold: i32::try_from(cosigned.cosigner_threshold()).unwrap_or(i32::MAX),
            pre_state_hash: ByteString::new(),
            post_state_hash: ByteString::new(),
            authority_funding_certificate: None,
            authority_cost_witness: None,
            admission_status: DeployAdmissionStatus::Executed,
        }
    }

    pub fn admission_rejected(
        cosigned: &crypto::rust::signatures::signed::Cosigned<DeployData>,
        pre_state_hash: ByteString,
    ) -> Self {
        let mut rejected = Self::empty_from_cosigned(cosigned);
        rejected.cosigner_threshold =
            i32::try_from(cosigned.cosigner_threshold()).unwrap_or(i32::MAX);
        rejected.is_failed = true;
        rejected.system_deploy_error = Some(Self::FUNDING_ADMISSION_REJECTION.to_string());
        rejected.pre_state_hash = pre_state_hash.clone();
        rejected.post_state_hash = pre_state_hash;
        rejected.admission_status = DeployAdmissionStatus::Rejected;
        rejected
    }

    pub fn is_admission_rejected(&self) -> bool {
        self.admission_status == DeployAdmissionStatus::Rejected
    }

    pub fn has_committed_state_effect(&self) -> bool {
        !self.is_admission_rejected()
            && (!self.is_failed
                || (self.authority_funding_certificate.is_some()
                    && self.authority_cost_witness.is_some()))
    }

    pub fn deploy_id(&self) -> &ByteString {
        if self.envelope_commitment.is_empty() {
            &self.deploy.sig
        } else {
            &self.envelope_commitment
        }
    }

    pub fn deploy_id_v6(&self) -> Result<crate::rust::deploy_id::DeployIdV6, String> {
        crate::rust::deploy_id::DeployIdV6::try_from(self.envelope_commitment.as_ref())
            .map_err(|error| error.to_string())
    }

    pub fn deploy_id_for_protocol(
        &self,
        protocol_version: i64,
    ) -> Result<crate::rust::deploy_id::DeployLookupId, String> {
        if protocol_version >= 6 {
            self.deploy_id_v6()
                .map(crate::rust::deploy_id::DeployLookupId::V6)
        } else {
            Ok(crate::rust::deploy_id::DeployLookupId::Legacy(
                crate::rust::deploy_id::LegacyDeploySignature::new(self.deploy.sig.to_vec()),
            ))
        }
    }

    /// Reconstitute the [`Cosigned<DeployData>`] envelope from on-disk
    /// `ProcessedDeploy` shape. For legacy deploys (`cosigners.is_empty()`),
    /// uplifts via `Cosigned::from_single_signer` for byte-identical replay
    /// behavior. For multi-sig deploys, rebuilds the full canonical envelope
    /// with per-signer re-verification.
    pub fn to_cosigned(
        &self,
    ) -> Result<crypto::rust::signatures::signed::Cosigned<DeployData>, String> {
        use crypto::rust::signatures::signed::{Cosigned, Cosigner};

        let mut signers = Vec::with_capacity(1 + self.cosigners.len());
        signers.push(Cosigner {
            pk: self.deploy.pk.clone(),
            sig: self.deploy.sig.clone(),
            sig_algorithm: self.deploy.sig_algorithm.clone(),
        });
        for cs in &self.cosigners {
            let alg = SignaturesAlgFactory::apply(&cs.sig_algorithm).ok_or_else(|| {
                format!(
                    "Unknown cosigner signature algorithm: {} for cosigner pk={}",
                    cs.sig_algorithm,
                    hex::encode(&cs.pk)
                )
            })?;
            signers.push(Cosigner {
                pk: PublicKey::from_bytes(&cs.pk),
                sig: cs.sig.clone(),
                sig_algorithm: alg,
            });
        }
        if !self.envelope_commitment.is_empty() {
            if self.cosigner_threshold < 1 {
                return Err(
                    "ProcessedDeploy v6 envelope requires an explicit positive threshold"
                        .to_string(),
                );
            }
            let envelope = Cosigned::from_envelope_signed_data_threshold(
                self.deploy.data.clone(),
                signers,
                self.cosigner_threshold as u32,
            )
            .map_err(|error| format!("ProcessedDeploy v6 envelope invalid: {error}"))?;
            if envelope
                .envelope_commitment()
                .map_err(|error| format!("ProcessedDeploy v6 envelope invalid: {error}"))?
                != self.envelope_commitment
            {
                return Err("ProcessedDeploy envelope commitment mismatch".to_string());
            }
            Ok(envelope)
        } else if self.cosigners.is_empty() {
            Cosigned::from_single_signer(self.deploy.clone())
                .map_err(|error| format!("legacy uplift to Cosigned failed: {error}"))
        } else if self.cosigner_threshold > 0 {
            Cosigned::from_signed_data_threshold(
                self.deploy.data.clone(),
                signers,
                self.cosigner_threshold as u32,
            )
            .map_err(|error| format!("legacy threshold envelope invalid: {error}"))
        } else {
            Cosigned::from_signed_data(self.deploy.data.clone(), signers)
                .map_err(|error| format!("legacy envelope invalid: {error}"))
        }
    }

    pub fn to_deploy_info(self) -> DeployInfo {
        let deploy_id = self.deploy_id().clone();
        DeployInfo {
            deployer: PrettyPrinter::build_string_no_limit(&self.deploy.pk.bytes),
            term: self.deploy.data.term.clone(),
            timestamp: self.deploy.data.time_stamp,
            sig: PrettyPrinter::build_string_no_limit(&self.deploy.sig),
            sig_algorithm: self.deploy.sig_algorithm.name(),
            valid_after_block_number: self.deploy.data.valid_after_block_number,
            cost: self.cost.cost,
            errored: self.is_failed,
            system_deploy_error: self.system_deploy_error.unwrap_or_default(),
            transfers: Vec::new(),
            transfers_available: false,
            authority_funding_certificate: self.authority_funding_certificate,
            authority_cost_witness: self.authority_cost_witness,
            pre_state_hash: self.pre_state_hash,
            post_state_hash: self.post_state_hash,
            admission_status: self.admission_status.to_proto(),
            deploy_id,
        }
    }

    pub fn from_proto(proto: ProcessedDeployProto) -> Result<Self, String> {
        let deploy_proto = proto
            .deploy
            .ok_or_else(|| "Missing deploy field".to_string())?;
        // Capture cosigner metadata BEFORE moving `deploy_proto` into
        // `DeployData::from_proto`. The inner Signed<DeployData> carries
        // only the primary signer; the cosigners[] populate the
        // ProcessedDeploy fields directly so the multi-sig shape survives
        // serialization.
        let mut cosigners = deploy_proto.cosigners.clone();
        let mut cosigner_threshold = deploy_proto.cosigner_threshold;
        let envelope_commitment = deploy_proto.deploy_id.clone();
        let deploy = if envelope_commitment.is_empty() {
            DeployData::from_proto(deploy_proto)?
        } else {
            let envelope = DeployData::from_proto_cosigned(deploy_proto)?;
            let selected_index = envelope
                .signers()
                .iter()
                .position(|signer| !signer.sig.is_empty())
                .ok_or_else(|| "protocol-v6 envelope has no selected signer".to_string())?;
            let selected = &envelope.signers()[selected_index];
            cosigner_threshold = i32::try_from(envelope.cosigner_threshold())
                .map_err(|_| "protocol-v6 threshold exceeds i32".to_string())?;
            cosigners = envelope
                .signers()
                .iter()
                .enumerate()
                .filter(|(index, _)| *index != selected_index)
                .map(|(_, signer)| crate::casper::CompoundSigner {
                    pk: signer.pk.bytes.clone().into(),
                    sig: signer.sig.clone(),
                    sig_algorithm: signer.sig_algorithm.name(),
                })
                .collect();
            Signed {
                data: envelope.data.clone(),
                pk: selected.pk.clone(),
                sig: selected.sig.clone(),
                sig_algorithm: selected.sig_algorithm.clone(),
            }
        };
        let processed = Self {
            deploy,
            envelope_commitment,
            cost: proto.cost.ok_or_else(|| "Missing cost field".to_string())?,
            deploy_log: proto
                .deploy_log
                .into_iter()
                .map(|e| Event::from_proto(e))
                .collect::<Result<Vec<Event>, String>>()?,
            is_failed: proto.errored,
            system_deploy_error: {
                if proto.system_deploy_error.is_empty() {
                    None
                } else {
                    Some(proto.system_deploy_error)
                }
            },
            cosigners,
            cosigner_threshold,
            pre_state_hash: proto.pre_state_hash,
            post_state_hash: proto.post_state_hash,
            authority_funding_certificate: proto.authority_funding_certificate,
            authority_cost_witness: proto.authority_cost_witness,
            admission_status: DeployAdmissionStatus::from_proto(proto.admission_status),
        };
        processed.to_cosigned()?;
        Ok(processed)
    }

    pub fn to_proto(self) -> ProcessedDeployProto {
        let mut deploy_proto = if self.envelope_commitment.is_empty() {
            DeployData::to_proto(self.deploy.clone())
        } else {
            DeployData::to_proto_cosigned(
                &self
                    .to_cosigned()
                    .expect("validated ProcessedDeploy v6.1 envelope"),
            )
        };
        if self.envelope_commitment.is_empty() {
            deploy_proto.cosigners = self.cosigners.clone();
            deploy_proto.cosigner_threshold = self.cosigner_threshold;
        }
        ProcessedDeployProto {
            deploy: Some(deploy_proto),
            cost: Some(self.cost),
            deploy_log: self.deploy_log.into_iter().map(|e| e.to_proto()).collect(),
            errored: self.is_failed,
            system_deploy_error: self.system_deploy_error.unwrap_or_default(),
            pre_state_hash: self.pre_state_hash,
            post_state_hash: self.post_state_hash,
            authority_funding_certificate: self.authority_funding_certificate,
            authority_cost_witness: self.authority_cost_witness,
            admission_status: self.admission_status.to_proto(),
        }
    }
}

/// A single cosigner authorization over the Cost-Accounted Rho Stage-C
/// redemption datum (DR-7/DR-12): a `(public_key, signature)` pair carried in
/// the block body so replay can re-run the multisig-quorum verification.
#[derive(Debug, Clone, PartialEq)]
pub struct RedemptionAuthorizationData {
    pub public_key: ByteString,
    pub signature: ByteString,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SystemDeployData {
    Slash {
        invalid_block_hash: ByteString,
        equivocation_block_hash: Option<ByteString>,
        issuer_public_key: PublicKey,
        target_activation_epoch: i64,
        target_bond_generation: BondGeneration,
    },
    CloseBlockSystemDeployData,
    /// Cost-Accounted Rho Stage-C validator redemption (DR-7/DR-12). Carries the
    /// FULL redemption-authorization material so replay can re-run the DR-12
    /// PoS-multisig-quorum platform obligation byte-identically to play.
    Redeem {
        validator_pk: ByteString,
        target_bond_generation: BondGeneration,
        /// Outcome tag: "Vindicated" | "Guilty" | "Burned".
        outcome_tag: String,
        /// Penalty for Guilty (0 otherwise).
        penalty: i64,
        pos_multi_sig_public_keys: Vec<String>,
        pos_multi_sig_quorum: u32,
        authorizations: Vec<RedemptionAuthorizationData>,
    },
    Empty,
}

impl SystemDeployData {
    pub fn create_slash(
        invalid_block_hash: ByteString,
        issuer_public_key: PublicKey,
        target_activation_epoch: i64,
        target_bond_generation: BondGeneration,
    ) -> Self {
        Self::Slash {
            invalid_block_hash,
            equivocation_block_hash: None,
            issuer_public_key,
            target_activation_epoch,
            target_bond_generation,
        }
    }

    pub fn create_close() -> Self { Self::CloseBlockSystemDeployData }

    pub fn create_equivocation_slash(
        first_block_hash: ByteString,
        second_block_hash: ByteString,
        issuer_public_key: PublicKey,
        target_activation_epoch: i64,
        target_bond_generation: BondGeneration,
    ) -> Self {
        let (invalid_block_hash, equivocation_block_hash) = if first_block_hash < second_block_hash
        {
            (first_block_hash, second_block_hash)
        } else {
            (second_block_hash, first_block_hash)
        };
        Self::Slash {
            invalid_block_hash,
            equivocation_block_hash: Some(equivocation_block_hash),
            issuer_public_key,
            target_activation_epoch,
            target_bond_generation,
        }
    }

    pub fn create_redeem(
        validator_pk: ByteString,
        target_bond_generation: BondGeneration,
        outcome_tag: String,
        penalty: i64,
        pos_multi_sig_public_keys: Vec<String>,
        pos_multi_sig_quorum: u32,
        authorizations: Vec<RedemptionAuthorizationData>,
    ) -> Self {
        Self::Redeem {
            validator_pk,
            target_bond_generation,
            outcome_tag,
            penalty,
            pos_multi_sig_public_keys,
            pos_multi_sig_quorum,
            authorizations,
        }
    }

    pub fn from_proto(proto: SystemDeployDataProto) -> Result<Self, String> {
        match proto
            .system_deploy
            .ok_or_else(|| "Missing system deploy field".to_string())?
        {
            system_deploy_data_proto::SystemDeploy::SlashSystemDeploy(
                slash_system_deploy_data_proto,
            ) => {
                let target_bond_generation = slash_system_deploy_data_proto
                    .target_bond_generation
                    .ok_or_else(|| "slash deploy is missing target bond generation".to_string())
                    .and_then(|generation| {
                        BondGeneration::try_from(generation).map_err(|error| error.to_string())
                    })?;
                Ok(Self::Slash {
                    invalid_block_hash: slash_system_deploy_data_proto.invalid_block_hash,
                    equivocation_block_hash: (!slash_system_deploy_data_proto
                        .equivocation_block_hash
                        .is_empty())
                    .then_some(slash_system_deploy_data_proto.equivocation_block_hash),
                    issuer_public_key: PublicKey::from_bytes(
                        &slash_system_deploy_data_proto.issuer_public_key,
                    ),
                    target_activation_epoch: slash_system_deploy_data_proto.target_activation_epoch,
                    target_bond_generation,
                })
            }
            system_deploy_data_proto::SystemDeploy::CloseBlockSystemDeploy(_) => {
                Ok(Self::CloseBlockSystemDeployData)
            }
            system_deploy_data_proto::SystemDeploy::RedeemSystemDeploy(redeem) => {
                Ok(Self::Redeem {
                    validator_pk: redeem.validator_pk,
                    target_bond_generation: redeem
                        .target_bond_generation
                        .ok_or_else(|| {
                            "redeem deploy is missing target bond generation".to_string()
                        })
                        .and_then(|generation| {
                            BondGeneration::try_from(generation).map_err(|error| error.to_string())
                        })?,
                    outcome_tag: redeem.outcome_tag,
                    penalty: redeem.penalty,
                    pos_multi_sig_public_keys: redeem.pos_multi_sig_public_keys,
                    pos_multi_sig_quorum: redeem.pos_multi_sig_quorum,
                    authorizations: redeem
                        .authorizations
                        .into_iter()
                        .map(|a| RedemptionAuthorizationData {
                            public_key: a.public_key,
                            signature: a.signature,
                        })
                        .collect(),
                })
            }
        }
    }

    pub fn to_proto(sdd: SystemDeployData) -> SystemDeployDataProto {
        match sdd {
            Self::Slash {
                invalid_block_hash,
                equivocation_block_hash,
                issuer_public_key,
                target_activation_epoch,
                target_bond_generation,
            } => SystemDeployDataProto {
                system_deploy: Some(SystemDeploy::SlashSystemDeploy(
                    SlashSystemDeployDataProto {
                        invalid_block_hash,
                        issuer_public_key: issuer_public_key.bytes.into(),
                        target_activation_epoch,
                        equivocation_block_hash: equivocation_block_hash.unwrap_or_default(),
                        target_bond_generation: Some(target_bond_generation.get()),
                    },
                )),
            },
            Self::CloseBlockSystemDeployData => SystemDeployDataProto {
                system_deploy: Some(SystemDeploy::CloseBlockSystemDeploy(
                    CloseBlockSystemDeployDataProto {},
                )),
            },
            Self::Redeem {
                validator_pk,
                target_bond_generation,
                outcome_tag,
                penalty,
                pos_multi_sig_public_keys,
                pos_multi_sig_quorum,
                authorizations,
            } => SystemDeployDataProto {
                system_deploy: Some(SystemDeploy::RedeemSystemDeploy(
                    RedeemSystemDeployDataProto {
                        validator_pk,
                        outcome_tag,
                        penalty,
                        pos_multi_sig_public_keys,
                        pos_multi_sig_quorum,
                        authorizations: authorizations
                            .into_iter()
                            .map(|a| RedemptionAuthorizationProto {
                                public_key: a.public_key,
                                signature: a.signature,
                            })
                            .collect(),
                        target_bond_generation: Some(target_bond_generation.get()),
                    },
                )),
            },
            Self::Empty => SystemDeployDataProto {
                system_deploy: None,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProcessedSystemDeploy {
    Succeeded {
        event_list: Vec<Event>,
        system_deploy: SystemDeployData,
        pre_state_hash: ByteString,
        post_state_hash: ByteString,
    },
    Failed {
        event_list: Vec<Event>,
        error_msg: String,
        pre_state_hash: ByteString,
        post_state_hash: ByteString,
    },
}

impl ProcessedSystemDeploy {
    pub fn failed(self) -> bool { matches!(self, ProcessedSystemDeploy::Failed { .. }) }

    pub fn fold<A, F, G>(self, if_succeeded: F, if_failed: G) -> A
    where
        F: Fn(Vec<Event>) -> A,
        G: Fn(Vec<Event>, String) -> A,
    {
        match self {
            ProcessedSystemDeploy::Succeeded { event_list, .. } => if_succeeded(event_list),
            ProcessedSystemDeploy::Failed {
                event_list,
                error_msg,
                ..
            } => if_failed(event_list, error_msg),
        }
    }

    pub fn state_hashes(&self) -> (&ByteString, &ByteString) {
        match self {
            ProcessedSystemDeploy::Succeeded {
                pre_state_hash,
                post_state_hash,
                ..
            }
            | ProcessedSystemDeploy::Failed {
                pre_state_hash,
                post_state_hash,
                ..
            } => (pre_state_hash, post_state_hash),
        }
    }

    pub fn from_proto(psd: ProcessedSystemDeployProto) -> Result<Self, String> {
        let deploy_log: Result<Vec<Event>, String> =
            psd.deploy_log.into_iter().map(Event::from_proto).collect();

        match deploy_log {
            Ok(deploy_log) => {
                if psd.error_msg.is_empty() {
                    Ok(ProcessedSystemDeploy::Succeeded {
                        event_list: deploy_log,
                        system_deploy: SystemDeployData::from_proto(
                            psd.system_deploy
                                .ok_or_else(|| "Missing system deploy field".to_string())?,
                        )?,
                        pre_state_hash: psd.pre_state_hash,
                        post_state_hash: psd.post_state_hash,
                    })
                } else {
                    Ok(ProcessedSystemDeploy::Failed {
                        event_list: deploy_log,
                        error_msg: psd.error_msg,
                        pre_state_hash: psd.pre_state_hash,
                        post_state_hash: psd.post_state_hash,
                    })
                }
            }
            Err(err) => Err(err),
        }
    }

    pub fn to_proto(self) -> ProcessedSystemDeployProto {
        match self {
            ProcessedSystemDeploy::Succeeded {
                event_list,
                system_deploy,
                pre_state_hash,
                post_state_hash,
            } => ProcessedSystemDeployProto {
                system_deploy: Some(SystemDeployData::to_proto(system_deploy)),
                deploy_log: event_list
                    .into_iter()
                    .map(|arg0: Event| Event::to_proto(&arg0))
                    .collect(),
                error_msg: "".to_string(),
                pre_state_hash,
                post_state_hash,
            },
            ProcessedSystemDeploy::Failed {
                event_list,
                error_msg,
                pre_state_hash,
                post_state_hash,
            } => ProcessedSystemDeployProto {
                system_deploy: None,
                deploy_log: event_list
                    .into_iter()
                    .map(|arg0: Event| Event::to_proto(&arg0))
                    .collect(),
                error_msg,
                pre_state_hash,
                post_state_hash,
            },
        }
    }
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    Eq,
    Hash,
    utoipa::ToSchema
)]
pub struct DeployData {
    pub term: String,
    pub language: String,
    #[serde(rename = "timestamp")]
    pub time_stamp: i64,
    #[serde(rename = "validAfterBlockNumber")]
    pub valid_after_block_number: i64,
    #[serde(rename = "shardId")]
    pub shard_id: String,
    /// Optional millisecond timestamp after which deploy is invalid (None = no expiration)
    pub expiration_timestamp: Option<i64>,
    #[serde(default, rename = "authorityPresentations")]
    pub authority_presentations: Vec<crate::rhoapi::CostSignature>,
}

impl ToMessage for DeployData {
    type Type = DeployDataProto;
    fn to_message(&self) -> Self::Type { DeployData::_to_proto(self.clone()) }
    fn envelope_intent_v61(&self) -> Result<Vec<u8>, String> {
        self.validate_authority_presentations()?;
        if self.language != "rholang" {
            return Err("protocol-v6 deploy language must be rholang".to_string());
        }
        let timestamp = u64::try_from(self.time_stamp)
            .map_err(|_| "protocol-v6 deploy timestamp must be nonnegative".to_string())?;
        let valid_after = u64::try_from(self.valid_after_block_number)
            .map_err(|_| "protocol-v6 valid-after block must be nonnegative".to_string())?;
        if self.shard_id.is_empty() {
            return Err("protocol-v6 shard ID must be nonempty".to_string());
        }
        let mut intent = Vec::new();
        intent.extend_from_slice(&1u16.to_be_bytes());
        intent.push(1);
        append_deploy_intent_field(&mut intent, self.term.as_bytes());
        intent.extend_from_slice(&timestamp.to_be_bytes());
        intent.extend_from_slice(&valid_after.to_be_bytes());
        append_deploy_intent_field(&mut intent, self.shard_id.as_bytes());
        match self.expiration_timestamp {
            None => intent.push(0),
            Some(expiration) if expiration > 0 => {
                intent.push(1);
                intent.extend_from_slice(&(expiration as u64).to_be_bytes());
            }
            Some(_) => {
                return Err("protocol-v6 expiration timestamp must be positive".to_string());
            }
        }
        intent.extend_from_slice(&(self.authority_presentations.len() as u32).to_be_bytes());
        for presentation in &self.authority_presentations {
            append_deploy_intent_field(&mut intent, &canonical_cost_signature_bytes(presentation)?);
        }
        Ok(intent)
    }
}

fn append_deploy_intent_field(output: &mut Vec<u8>, bytes: &[u8]) {
    output.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
    output.extend_from_slice(bytes);
}

fn canonical_cost_signature_bytes(
    signature: &crate::rhoapi::CostSignature,
) -> Result<Vec<u8>, String> {
    use crate::rhoapi::cost_signature::Value;
    use crate::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
    use crate::rust::rholang::sorter::sortable::Sortable;

    let mut encoded = Vec::new();
    match signature.value.as_ref() {
        Some(Value::Unit(true)) => encoded.push(0),
        Some(Value::Ground(bytes)) => {
            encoded.push(1);
            append_deploy_intent_field(&mut encoded, bytes);
        }
        Some(Value::Quote(par)) => {
            let canonical = ParSortMatcher::sort_match(par).term;
            if canonical != *par {
                return Err("authority quote must contain a canonical process".to_string());
            }
            encoded.push(2);
            append_deploy_intent_field(&mut encoded, &canonical.encode_to_vec());
        }
        Some(Value::Name(par)) => {
            let canonical = ParSortMatcher::sort_match(par).term;
            if canonical != *par {
                return Err("authority name must contain a canonical process".to_string());
            }
            encoded.push(3);
            append_deploy_intent_field(&mut encoded, &canonical.encode_to_vec());
        }
        Some(Value::Compound(compound)) => {
            let mut children = Vec::new();
            for child in &compound.elements {
                match child.value.as_ref() {
                    Some(Value::Compound(_)) | Some(Value::Unit(_)) => {
                        return Err(
                            "authority compound must be flat and contain no unit".to_string()
                        );
                    }
                    _ => children.push(canonical_cost_signature_bytes(child)?),
                }
            }
            if children.len() < 2 {
                return Err("authority compound must contain at least two elements".to_string());
            }
            let supplied = children.clone();
            children.sort();
            if children != supplied {
                return Err("authority compound elements must be canonically ordered".to_string());
            }
            encoded.push(4);
            encoded.extend_from_slice(&(children.len() as u32).to_be_bytes());
            for child in children {
                append_deploy_intent_field(&mut encoded, &child);
            }
        }
        Some(Value::BoundLevel(_)) => {
            return Err(
                "authority presentation contains an unresolved bound signature".to_string(),
            );
        }
        Some(Value::Unit(false)) => {
            return Err("authority presentation contains a false unit".to_string());
        }
        None => return Err("authority presentation is missing its signature".to_string()),
    }
    Ok(encoded)
}

/// Internal helper for walking a `SigCompound` expression and collecting
/// atomic signer leaves. Used exclusively by
/// [`DeployData::from_proto_cosigned_with_sig_algebra`] and its callees.
#[derive(Clone, Debug)]
struct AlgebraAtom {
    pk: Vec<u8>,
    sig: prost::bytes::Bytes,
    sig_algorithm: String,
}

struct FundingAlgebraAnalysis {
    min_required: u32,
    all_required: bool,
}

impl AlgebraAtom {
    fn from_proto(atom: &crate::casper::SigAtom) -> Self {
        Self {
            pk: atom.pk.to_vec(),
            sig: atom.sig.clone(),
            sig_algorithm: atom.sig_algorithm.clone(),
        }
    }
}

impl DeployData {
    // D3 (DR-9): the singular-phlo escrow/price arithmetic
    // (`checked_total_phlo_charge[_value]`, `total_phlo_charge`,
    // `refund_amount_for_token_cost[_value]`, `validate_phlo`) is REMOVED. A
    // deploy's cost is the per-COMM token count (computed by the runtime); it is
    // funded by canonical SystemVault custody and prepaid located stacks and is
    // gated at block assembly (`casper/.../util/rholang/acceptance.rs`). There is
    // no client-supplied phlo limit or price and no legacy escrow refund.

    /// Returns true if this deploy has a time-based expiration set
    pub fn has_expiration(&self) -> bool {
        self.expiration_timestamp
            .map(|exp| exp > 0)
            .unwrap_or(false)
    }

    /// Returns true if this deploy has expired at the given time
    pub fn is_expired_at(&self, current_time_millis: i64) -> bool {
        self.expiration_timestamp
            .map(|exp| current_time_millis > exp)
            .unwrap_or(false)
    }

    pub fn encode(a: DeployData) -> ByteVector { DeployData::_to_proto(a).encode_to_vec() }

    pub fn decode(a: ByteVector) -> Result<DeployData, String> {
        let proto = DeployDataProto::decode(&a[..])
            .map_err(|e| format!("Failed to decode DeployData: {}", e))?;
        let data = DeployData::_from_proto(proto);
        data.validate_authority_presentations()?;
        Ok(data)
    }

    fn _from_proto(proto: DeployDataProto) -> Self {
        Self {
            term: proto.term,
            language: proto.language,
            time_stamp: proto.timestamp,
            valid_after_block_number: proto.valid_after_block_number,
            shard_id: proto.shard_id,
            // 0 in protobuf means not set, convert to None
            expiration_timestamp: if proto.expiration_timestamp == 0 {
                None
            } else {
                Some(proto.expiration_timestamp)
            },
            authority_presentations: proto.authority_presentations,
        }
    }

    fn validate_authority_presentations(&self) -> Result<(), String> {
        use crate::rhoapi::cost_signature::Value;
        use crate::rhoapi::CostSignature;
        use crate::rust::rholang::sorter::cost_accounting_sorter::sort_signature;
        use crate::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
        use crate::rust::rholang::sorter::sortable::Sortable;

        fn validate(signature: &CostSignature) -> Result<(), String> {
            match signature.value.as_ref() {
                Some(Value::Ground(_)) | Some(Value::Unit(true)) => Ok(()),
                Some(Value::Quote(par)) | Some(Value::Name(par))
                    if ParSortMatcher::sort_match(par).term == *par =>
                {
                    Ok(())
                }
                Some(Value::Compound(compound)) if compound.elements.len() >= 2 => {
                    compound.elements.iter().try_for_each(validate)
                }
                Some(Value::BoundLevel(_)) => {
                    Err("authority presentation contains an unresolved bound signature".to_string())
                }
                Some(Value::Compound(_)) => Err(
                    "authority presentation contains a malformed compound signature".to_string(),
                ),
                Some(Value::Unit(false)) | Some(Value::Quote(_)) | Some(Value::Name(_)) => {
                    Err("authority presentation contains a non-canonical signature".to_string())
                }
                None => Err("authority presentation is missing its signature".to_string()),
            }
        }

        let mut previous: Option<Vec<u8>> = None;
        for signature in &self.authority_presentations {
            let canonical = sort_signature(signature).term;
            validate(&canonical)?;
            if &canonical != signature {
                return Err("authority presentations must contain canonical signatures".to_string());
            }
            let encoded = canonical_cost_signature_bytes(&canonical)?;
            if previous.as_ref().is_some_and(|prior| prior >= &encoded) {
                return Err(
                    "authority presentations must be strictly ordered and unique".to_string(),
                );
            }
            previous = Some(encoded);
        }
        Ok(())
    }

    /// Primary-signer-only decode. Returns `Signed<DeployData>` constructed
    /// from the primary signer's fields (`deployer`, `sig`, `sig_algorithm`)
    /// regardless of whether the wire deploy carries cosigners. Callers that
    /// need the full multi-signature envelope MUST use
    /// [`Self::from_proto_cosigned`].
    ///
    /// `ProcessedDeploy::from_proto` calls this routine and SEPARATELY
    /// captures `proto.cosigners` into the `ProcessedDeploy.cosigners` field,
    /// so the cosigner data is preserved across deserialization even though the
    /// inner `Signed<DeployData>` only carries the primary.
    pub fn from_proto(proto: DeployDataProto) -> Result<Signed<DeployData>, String> {
        let algorithm = SignaturesAlgFactory::apply(&proto.sig_algorithm)
            .ok_or_else(|| format!("Unknown signature algorithm: {}", proto.sig_algorithm))?;

        let sig = proto.sig.clone();
        let pk = PublicKey::from_bytes(&proto.deployer);
        let data = DeployData::_from_proto(proto);
        data.validate_authority_presentations()?;
        let signed = Signed::from_signed_data(data, pk, sig, algorithm)?;

        match signed {
            Some(signed) => Ok(signed),
            None => Err("Invalid signature".to_string()),
        }
    }

    /// Multi-signature aware decode. Returns a [`Cosigned<DeployData>`]
    /// envelope covering both legacy single-sig wire deploys (`cosigners`
    /// empty → one-element envelope) and multi-sig wire deploys (`cosigners`
    /// non-empty → N-element envelope).
    ///
    /// Invariants enforced by `Cosigned::from_signed_data` at construction:
    /// 1. All signers' signatures verify against the canonical message hash.
    /// 2. Canonical pk-ascending sort; no duplicates.
    ///
    /// D3 (DR-9): there is no per-signer `phlo_share` and no share-sum
    /// invariant — authority is resolved from canonical vault custody and
    /// prepaid located stacks.
    pub fn from_proto_cosigned_legacy(
        proto: DeployDataProto,
    ) -> Result<crypto::rust::signatures::signed::Cosigned<DeployData>, String> {
        use crypto::rust::signatures::signed::{Cosigned, Cosigner};

        if !proto.deploy_id.is_empty() || proto.authorization_v61.is_some() {
            return Err(
                "legacy deploy cannot contain protocol-v6 authorization fields".to_string(),
            );
        }

        if let Some(sig_algebra) = proto.sig_algebra.clone() {
            let data = DeployData::_from_proto(proto);
            data.validate_authority_presentations()?;
            return Self::from_proto_cosigned_with_sig_algebra(data, &sig_algebra);
        }

        let is_multi_sig = !proto.cosigners.is_empty();
        let cosigner_threshold = proto.cosigner_threshold;
        let total_signers = 1 + proto.cosigners.len();
        if cosigner_threshold < 0 || cosigner_threshold as usize > total_signers {
            return Err(format!(
                "Invalid cosigner_threshold {}: must satisfy 0 ≤ threshold ≤ {} (total signers)",
                cosigner_threshold, total_signers
            ));
        }

        let primary_alg = SignaturesAlgFactory::apply(&proto.sig_algorithm).ok_or_else(|| {
            format!(
                "Unknown primary signature algorithm: {}",
                proto.sig_algorithm
            )
        })?;

        // Build the canonical signer list. Primary first (will be sorted
        // canonically by Cosigned::from_signed_data). D3 (DR-9): no per-signer
        // phlo_share — funding follows the verified authority algebra.
        let mut signers = Vec::with_capacity(1 + proto.cosigners.len());
        signers.push(Cosigner {
            pk: PublicKey::from_bytes(&proto.deployer),
            sig: proto.sig.clone(),
            sig_algorithm: primary_alg,
        });
        for cs in &proto.cosigners {
            let alg = SignaturesAlgFactory::apply(&cs.sig_algorithm).ok_or_else(|| {
                format!(
                    "Unknown cosigner signature algorithm: {} for cosigner pk={}",
                    cs.sig_algorithm,
                    hex::encode(&cs.pk)
                )
            })?;
            signers.push(Cosigner {
                pk: PublicKey::from_bytes(&cs.pk),
                sig: cs.sig.clone(),
                sig_algorithm: alg,
            });
        }

        let data = DeployData::_from_proto(proto);
        data.validate_authority_presentations()?;
        if cosigner_threshold == 0 {
            Cosigned::from_signed_data(data, signers).map_err(|e| {
                format!(
                    "Cosigned envelope validation failed (is_multi_sig={}): {}",
                    is_multi_sig, e
                )
            })
        } else {
            Cosigned::from_signed_data_threshold(data, signers, cosigner_threshold as u32)
                .map_err(|e| {
                    format!(
                        "Cosigned threshold envelope validation failed (threshold={}, total_signers={}): {}",
                        cosigner_threshold, total_signers, e
                    )
                })
        }
    }

    pub fn from_proto_cosigned(
        proto: DeployDataProto,
    ) -> Result<crypto::rust::signatures::signed::Cosigned<DeployData>, String> {
        use crypto::rust::signatures::signed::{Cosigned, Cosigner};

        use crate::casper::authorization_policy_v61::Policy;

        if proto.deploy_id.len() != DeployIdV6::LENGTH {
            return Err("protocol-v6 deploy requires a 32-byte DeployId".to_string());
        }
        if !proto.deployer.is_empty()
            || !proto.sig.is_empty()
            || !proto.sig_algorithm.is_empty()
            || !proto.cosigners.is_empty()
            || proto.cosigner_threshold != 0
            || proto.sig_algebra.is_some()
        {
            return Err(
                "protocol-v6 deploy cannot contain legacy authorization fields".to_string(),
            );
        }
        let authorization = proto
            .authorization_v61
            .as_ref()
            .ok_or_else(|| "protocol-v6 deploy authorization is missing".to_string())?;
        if authorization.format_version != 0x0006_0001 {
            return Err("protocol-v6 deploy authorization format is not v6.1".to_string());
        }
        let policy = authorization
            .policy
            .as_ref()
            .and_then(|policy| policy.policy.as_ref())
            .ok_or_else(|| "protocol-v6 deploy authorization policy is missing".to_string())?;
        let (members, threshold) = match policy {
            Policy::AllOf(policy) => {
                let count = u32::try_from(policy.members.len())
                    .map_err(|_| "protocol-v6 signer count exceeds u32".to_string())?;
                if count == 0 {
                    return Err("protocol-v6 AllOf policy must contain members".to_string());
                }
                (&policy.members, count)
            }
            Policy::Threshold(policy) => {
                let count = u32::try_from(policy.members.len())
                    .map_err(|_| "protocol-v6 signer count exceeds u32".to_string())?;
                if policy.minimum == 0 || policy.minimum >= count {
                    return Err("protocol-v6 threshold must satisfy 1 <= k < N".to_string());
                }
                (&policy.members, policy.minimum)
            }
        };
        let expected_bitmap_len = members.len().div_ceil(8);
        if authorization.presence_bitmap.len() != expected_bitmap_len
            || authorization.presence_bitmap.last().is_some_and(|last| {
                let used = members.len() % 8;
                used != 0 && *last & !((1u8 << used) - 1) != 0
            })
        {
            return Err("protocol-v6 presence bitmap is not canonical".to_string());
        }
        let selected_indices = authorization
            .presence_bitmap
            .iter()
            .enumerate()
            .flat_map(|(byte_index, byte)| {
                (0..8).filter_map(move |bit| {
                    ((*byte & (1 << bit)) != 0).then_some(byte_index * 8 + bit)
                })
            })
            .filter(|index| *index < members.len())
            .collect::<Vec<_>>();
        if authorization.witnesses.len() != selected_indices.len()
            || authorization
                .witnesses
                .iter()
                .zip(&selected_indices)
                .any(|(witness, expected)| {
                    witness.signature.is_empty() || witness.member_index as usize != *expected
                })
        {
            return Err(
                "protocol-v6 witnesses do not exactly match the presence bitmap".to_string(),
            );
        }
        let mut witness_iter = authorization.witnesses.iter().peekable();
        let signers = members
            .iter()
            .enumerate()
            .map(|(index, member)| {
                let algorithm_name = match SignatureSchemeV61::try_from(member.scheme)
                    .unwrap_or(SignatureSchemeV61::Unspecified)
                {
                    SignatureSchemeV61::Secp256k1 => "secp256k1",
                    SignatureSchemeV61::Secp256k1Eth => "secp256k1:eth",
                    _ => return Err("protocol-v6 signature scheme is not active".to_string()),
                };
                let signature = if witness_iter
                    .peek()
                    .is_some_and(|witness| witness.member_index as usize == index)
                {
                    witness_iter
                        .next()
                        .expect("peeked witness")
                        .signature
                        .clone()
                } else {
                    ByteString::new()
                };
                Ok(Cosigner {
                    pk: PublicKey::from_bytes(&member.public_key),
                    sig: signature,
                    sig_algorithm: SignaturesAlgFactory::apply(algorithm_name)
                        .expect("active protocol-v6 signature scheme"),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Cosigned::<DeployData>::validate_envelope_signer_order(&signers)
            .map_err(|error| format!("protocol-v6 envelope validation failed: {error}"))?;
        let expected_commitment = proto.deploy_id.clone();
        let data = DeployData::_from_proto(proto);
        data.validate_authority_presentations()?;
        let envelope = Cosigned::from_envelope_signed_data_threshold(data, signers, threshold)
            .map_err(|error| format!("protocol-v6 envelope validation failed: {error}"))?;
        if envelope
            .envelope_commitment()
            .map_err(|error| format!("protocol-v6 envelope validation failed: {error}"))?
            != expected_commitment
        {
            return Err("protocol-v6 DeployId mismatch".to_string());
        }
        Ok(envelope)
    }

    /// Validates the admission algebra accepted at the deploy boundary. `Atom`
    /// and `Tensor` realize the funding-signature grammar. A top-level
    /// `Threshold` realizes a k-of-N admission quorum whose members must be
    /// atomic candidate signers. Thresholds nested under `Tensor` are rejected
    /// because the flat `Cosigned` threshold cannot preserve that formula.
    /// Capability connectives are rejected before the algebra is lowered to a
    /// canonical `Cosigned` envelope.
    pub fn from_proto_cosigned_with_sig_algebra(
        data: DeployData,
        sig_algebra: &crate::casper::SigCompound,
    ) -> Result<crypto::rust::signatures::signed::Cosigned<DeployData>, String> {
        use crypto::rust::signatures::signed::{Cosigned, Cosigner};

        data.validate_authority_presentations()?;
        let mut atoms: Vec<AlgebraAtom> = Vec::new();
        let analysis = Self::analyze_funding_algebra(sig_algebra, &mut atoms)?;

        let mut signers: Vec<Cosigner> = Vec::with_capacity(atoms.len());
        for atom in atoms.into_iter() {
            let alg = SignaturesAlgFactory::apply(&atom.sig_algorithm)
                .ok_or_else(|| format!("Unknown signature algorithm: {}", atom.sig_algorithm))?;
            signers.push(Cosigner {
                pk: PublicKey::from_bytes(&atom.pk),
                sig: atom.sig,
                sig_algorithm: alg,
            });
        }

        let total = signers.len() as u32;
        if analysis.all_required {
            debug_assert_eq!(analysis.min_required, total);
            Cosigned::from_signed_data(data, signers)
                .map_err(|e| format!("Cosigned sig_algebra validation failed: {}", e))
        } else {
            Cosigned::from_signed_data_threshold(data, signers, analysis.min_required).map_err(
                |e| {
                    format!(
                        "Cosigned sig_algebra threshold validation failed (min_required={}): {}",
                        analysis.min_required, e
                    )
                },
            )
        }
    }

    fn analyze_funding_algebra(
        sig: &crate::casper::SigCompound,
        atoms: &mut Vec<AlgebraAtom>,
    ) -> Result<FundingAlgebraAnalysis, String> {
        use crate::casper::sig_compound::Connective;
        let connective = sig
            .connective
            .as_ref()
            .ok_or_else(|| "SigCompound.connective missing".to_string())?;
        match connective {
            Connective::Atom(atom) => {
                atoms.push(AlgebraAtom::from_proto(atom));
                Ok(FundingAlgebraAnalysis {
                    min_required: 1,
                    all_required: true,
                })
            }
            Connective::Tensor(pair) => {
                let left = pair
                    .left
                    .as_deref()
                    .ok_or_else(|| "SigPair.left missing".to_string())?;
                let right = pair
                    .right
                    .as_deref()
                    .ok_or_else(|| "SigPair.right missing".to_string())?;
                let left = Self::analyze_funding_algebra(left, atoms)?;
                let right = Self::analyze_funding_algebra(right, atoms)?;
                if !left.all_required || !right.all_required {
                    return Err(
                        "SigThreshold must be the top-level admission connective; a scalar signer threshold cannot preserve Tensor composition"
                            .to_string(),
                    );
                }
                Ok(FundingAlgebraAnalysis {
                    min_required: left
                        .min_required
                        .checked_add(right.min_required)
                        .ok_or_else(|| "Funding algebra signer count overflow".to_string())?,
                    all_required: left.all_required && right.all_required,
                })
            }
            Connective::Threshold(threshold) => {
                if threshold.threshold < 1
                    || (threshold.threshold as usize) > threshold.members.len()
                {
                    return Err(format!(
                        "SigThreshold.threshold must satisfy 1 ≤ threshold ≤ members.len() ({}), got {}",
                        threshold.members.len(), threshold.threshold
                    ));
                }
                for member in &threshold.members {
                    match member.connective.as_ref() {
                        Some(Connective::Atom(atom)) => atoms.push(AlgebraAtom::from_proto(atom)),
                        Some(Connective::Plus(_)) => {
                            return Err(Self::capability_connective_error("⊕", "Plus"));
                        }
                        Some(Connective::With(_)) => {
                            return Err(Self::capability_connective_error("&", "With"));
                        }
                        Some(Connective::Bang(_)) => {
                            return Err(Self::capability_connective_error("!", "Bang"));
                        }
                        Some(Connective::Whynot(_)) => {
                            return Err(Self::capability_connective_error("?", "WhyNot"));
                        }
                        Some(Connective::Lolly(_)) => {
                            return Err(Self::capability_connective_error("⊸", "Lolly"));
                        }
                        Some(Connective::Tensor(_)) | Some(Connective::Threshold(_)) => {
                            return Err(
                                "SigThreshold members must be atomic candidate signers".to_string()
                            );
                        }
                        None => return Err("SigThreshold member connective missing".to_string()),
                    }
                }
                Ok(FundingAlgebraAnalysis {
                    min_required: threshold.threshold as u32,
                    all_required: false,
                })
            }
            Connective::Plus(_) => Err(Self::capability_connective_error("⊕", "Plus")),
            Connective::With(_) => Err(Self::capability_connective_error("&", "With")),
            Connective::Bang(_) => Err(Self::capability_connective_error("!", "Bang")),
            Connective::Whynot(_) => Err(Self::capability_connective_error("?", "WhyNot")),
            Connective::Lolly(_) => Err(Self::capability_connective_error("⊸", "Lolly")),
        }
    }

    fn capability_connective_error(symbol: &str, name: &str) -> String {
        format!(
            "value/capability connective {symbol} ({name}) is not a funding-signature former (cost-accounted-rho §App-A: funding signatures are g | #P | s∘s — ground/quote atoms folded by the tensor ∘; value/capability connectives ⊕/&/!/?/⊸ are capability-layer only)"
        )
    }

    fn _to_proto(dd: DeployData) -> DeployDataProto {
        DeployDataProto {
            term: dd.term,
            language: String::new(),
            timestamp: dd.time_stamp,
            valid_after_block_number: dd.valid_after_block_number,
            shard_id: dd.shard_id,
            // Only include expirationTimestamp if set to maintain backward compatibility
            expiration_timestamp: dd.expiration_timestamp.unwrap_or(0),
            authority_presentations: dd.authority_presentations,
            ..Default::default()
        }
    }

    pub fn to_proto(dd: Signed<DeployData>) -> DeployDataProto { Self::to_proto_ref(&dd) }

    pub fn to_proto_ref(dd: &Signed<DeployData>) -> DeployDataProto {
        DeployDataProto {
            term: dd.data.term.clone(),
            language: dd.data.language.clone(),
            timestamp: dd.data.time_stamp,
            valid_after_block_number: dd.data.valid_after_block_number,
            shard_id: dd.data.shard_id.clone(),
            deployer: dd.pk.bytes.clone().into(),
            sig: dd.sig.clone().into(),
            sig_algorithm: dd.sig_algorithm.name(),
            // Only include expirationTimestamp if set to maintain backward compatibility
            expiration_timestamp: dd.data.expiration_timestamp.unwrap_or(0),
            authority_presentations: dd.data.authority_presentations.clone(),
            ..Default::default()
        }
    }

    /// Serialize a [`Cosigned<DeployData>`] back to [`DeployDataProto`] wire
    /// format. For single-signer cosigned envelopes the output is
    /// byte-identical to `to_proto(signed)` (cosigners empty). For
    /// multi-signer envelopes the additional cosigners populate the
    /// `cosigners[]` field. D3 (DR-9): no per-signer phlo_share.
    pub fn to_proto_cosigned(
        cosigned: &crypto::rust::signatures::signed::Cosigned<DeployData>,
    ) -> DeployDataProto {
        if !cosigned.is_envelope_bound() {
            let primary = cosigned.primary();
            return DeployDataProto {
                term: cosigned.data.term.clone(),
                language: cosigned.data.language.clone(),
                timestamp: cosigned.data.time_stamp,
                valid_after_block_number: cosigned.data.valid_after_block_number,
                shard_id: cosigned.data.shard_id.clone(),
                deployer: primary.pk.bytes.clone().into(),
                sig: primary.sig.clone(),
                sig_algorithm: primary.sig_algorithm.name(),
                expiration_timestamp: cosigned.data.expiration_timestamp.unwrap_or(0),
                authority_presentations: cosigned.data.authority_presentations.clone(),
                cosigners: cosigned
                    .signers()
                    .iter()
                    .skip(1)
                    .map(|signer| crate::casper::CompoundSigner {
                        pk: signer.pk.bytes.clone().into(),
                        sig: signer.sig.clone(),
                        sig_algorithm: signer.sig_algorithm.name(),
                    })
                    .collect(),
                cosigner_threshold: i32::try_from(cosigned.cosigner_threshold())
                    .unwrap_or(i32::MAX),
                ..Default::default()
            };
        }

        use crate::casper::authorization_policy_v61::Policy;
        let members = cosigned
            .signers()
            .iter()
            .map(|signer| crate::casper::PrincipalV61 {
                scheme: i32::from(signer.scheme_id_v61().expect("validated v6.1 scheme")),
                public_key: signer.pk.bytes.clone().into(),
            })
            .collect::<Vec<_>>();
        let threshold = cosigned.cosigner_threshold();
        let policy = if threshold == members.len() as u32 {
            Policy::AllOf(crate::casper::AllOfPolicyV61 { members })
        } else {
            Policy::Threshold(crate::casper::ThresholdPolicyV61 {
                minimum: threshold,
                members,
            })
        };
        let witnesses = cosigned
            .signers()
            .iter()
            .enumerate()
            .filter(|(_, signer)| !signer.sig.is_empty())
            .map(|(index, signer)| crate::casper::SignatureWitnessV61 {
                member_index: index as u32,
                signature: signer.sig.clone(),
            })
            .collect();
        DeployDataProto {
            term: cosigned.data.term.clone(),
            language: cosigned.data.language.clone(),
            timestamp: cosigned.data.time_stamp,
            valid_after_block_number: cosigned.data.valid_after_block_number,
            shard_id: cosigned.data.shard_id.clone(),
            expiration_timestamp: cosigned.data.expiration_timestamp.unwrap_or(0),
            authority_presentations: cosigned.data.authority_presentations.clone(),
            deploy_id: cosigned
                .envelope_commitment()
                .expect("envelope-bound Cosigned invariant"),
            authorization_v61: Some(crate::casper::DeployAuthorizationV61 {
                format_version: 0x0006_0001,
                policy: Some(crate::casper::AuthorizationPolicyV61 {
                    policy: Some(policy),
                }),
                presence_bitmap: cosigned
                    .presence_bitmap_v61()
                    .expect("envelope-bound Cosigned invariant")
                    .into(),
                witnesses,
            }),
            ..Default::default()
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Peek {
    pub channel_index: i32,
}

impl Peek {
    pub fn from_proto(proto: PeekProto) -> Self {
        Self {
            channel_index: proto.channel_index,
        }
    }

    pub fn to_proto(self) -> PeekProto {
        PeekProto {
            channel_index: self.channel_index,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Produce(ProduceEvent),
    Consume(ConsumeEvent),
    Comm(CommEvent),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProduceEvent {
    pub channels_hash: ByteString,
    pub hash: ByteString,
    pub persistent: bool,
    pub times_repeated: i32,
    pub is_deterministic: bool,
    pub output_value: Vec<ByteString>,
    /// Indicates whether this produce event represents a failed non-deterministic process.
    pub failed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConsumeEvent {
    pub channels_hashes: Vec<ByteString>,
    pub hash: ByteString,
    pub persistent: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CommEvent {
    pub consume: ConsumeEvent,
    pub produces: Vec<ProduceEvent>,
    pub peeks: Vec<Peek>,
}

impl Event {
    pub fn from_proto(proto: EventProto) -> Result<Event, String> {
        match proto.event_instance {
            Some(event_proto::EventInstance::Produce(pe)) => {
                Ok(Event::Produce(ProduceEvent::from_proto(pe)))
            }
            Some(event_proto::EventInstance::Consume(ce)) => {
                Ok(Event::Consume(ConsumeEvent::from_proto(ce)))
            }
            Some(event_proto::EventInstance::Comm(CommEventProto {
                consume,
                produces,
                peeks,
            })) => Ok(Event::Comm(CommEvent {
                consume: ConsumeEvent::from_proto(
                    consume.ok_or_else(|| "Missing consume field".to_string())?,
                ),
                produces: produces.into_iter().map(ProduceEvent::from_proto).collect(),
                peeks: peeks.into_iter().map(Peek::from_proto).collect(),
            })),

            _ => Err("Received malformed Event: None".to_string()),
        }
    }

    pub fn to_proto(&self) -> EventProto {
        match self {
            Event::Produce(pe) => EventProto {
                event_instance: Some(event_proto::EventInstance::Produce(pe.clone().to_proto())),
            },
            Event::Consume(ce) => EventProto {
                event_instance: Some(event_proto::EventInstance::Consume(ce.clone().to_proto())),
            },
            Event::Comm(cme) => EventProto {
                event_instance: Some(event_proto::EventInstance::Comm(cme.clone().to_proto())),
            },
        }
    }
}

impl ProduceEvent {
    pub fn to_proto(self) -> ProduceEventProto {
        ProduceEventProto {
            channels_hash: self.channels_hash,
            hash: self.hash,
            persistent: self.persistent,
            times_repeated: self.times_repeated,
            is_deterministic: self.is_deterministic,
            output_value: self.output_value,
            failed: self.failed,
        }
    }

    pub fn from_proto(proto: ProduceEventProto) -> Self {
        ProduceEvent {
            channels_hash: proto.channels_hash,
            hash: proto.hash,
            persistent: proto.persistent,
            times_repeated: proto.times_repeated,
            is_deterministic: proto.is_deterministic,
            output_value: proto.output_value,
            failed: proto.failed,
        }
    }
}

impl ConsumeEvent {
    pub fn to_proto(self) -> ConsumeEventProto {
        ConsumeEventProto {
            channels_hashes: self.channels_hashes,
            hash: self.hash,
            persistent: self.persistent,
        }
    }

    pub fn from_proto(proto: ConsumeEventProto) -> Self {
        ConsumeEvent {
            channels_hashes: proto.channels_hashes,
            hash: proto.hash,
            persistent: proto.persistent,
        }
    }
}

impl CommEvent {
    pub fn to_proto(self) -> CommEventProto {
        CommEventProto {
            consume: Some(self.consume.to_proto()),
            produces: self.produces.into_iter().map(|pe| pe.to_proto()).collect(),
            peeks: self.peeks.into_iter().map(|pk| pk.to_proto()).collect(),
        }
    }

    pub fn from_proto(
        consume: ConsumeEventProto,
        produces: Vec<ProduceEventProto>,
        peeks: Vec<PeekProto>,
    ) -> Self {
        CommEvent {
            consume: ConsumeEvent::from_proto(consume),
            produces: produces.into_iter().map(ProduceEvent::from_proto).collect(),
            peeks: peeks.into_iter().map(Peek::from_proto).collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bond {
    pub validator: ByteString,
    pub stake: i64,
}

impl Bond {
    pub fn from_proto(proto: BondProto) -> Self {
        Self {
            validator: proto.validator,
            stake: proto.stake,
        }
    }

    pub fn to_proto(self) -> BondProto {
        BondProto {
            validator: self.validator,
            stake: self.stake,
        }
    }
}

// Last finalized state

pub struct StoreNodeKey {
    pub hash: Blake2b256Hash,
    pub index: Option<Byte>,
}

impl StoreNodeKey {
    // Encoding of non-existent index for store node (Skip or Leaf node)
    const NONE_INDEX: i32 = 0x100;

    pub fn from_proto(proto: StoreNodeKeyProto) -> (Blake2b256Hash, Option<Byte>) {
        // Key hash
        let hash_bytes = Blake2b256Hash::from_bytes(proto.hash.to_vec());

        // Relative branch index / max 8-bit
        let idx = if proto.index == Self::NONE_INDEX {
            None
        } else {
            Some(proto.index as u8)
        };

        (hash_bytes, idx)
    }

    pub fn to_proto(s: &(Blake2b256Hash, Option<Byte>)) -> StoreNodeKeyProto {
        StoreNodeKeyProto {
            hash: s.0.bytes().into(),
            index: s.1.map(|b| b as i32).unwrap_or(Self::NONE_INDEX),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StoreItemsMessageRequest {
    pub start_path: Vec<(Blake2b256Hash, Option<Byte>)>,
    pub skip: i32,
    pub take: i32,
}

impl StoreItemsMessageRequest {
    pub fn from_proto(proto: StoreItemsMessageRequestProto) -> Self {
        Self {
            start_path: proto
                .start_path
                .into_iter()
                .map(StoreNodeKey::from_proto)
                .collect(),
            skip: proto.skip,
            take: proto.take,
        }
    }

    pub fn to_proto(self) -> StoreItemsMessageRequestProto {
        StoreItemsMessageRequestProto {
            start_path: self.start_path.iter().map(StoreNodeKey::to_proto).collect(),
            skip: self.skip,
            take: self.take,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StoreItemsMessage {
    pub start_path: Vec<(Blake2b256Hash, Option<Byte>)>,
    pub last_path: Vec<(Blake2b256Hash, Option<Byte>)>,
    pub history_items: Vec<(Blake2b256Hash, ByteString)>,
    pub data_items: Vec<(Blake2b256Hash, ByteString)>,
}

impl StoreItemsMessage {
    pub fn pretty(self) -> String {
        let start: String = self
            .start_path
            .iter()
            .map(RSpaceExporterInstance::path_pretty)
            .collect();

        let last: String = self
            .last_path
            .iter()
            .map(RSpaceExporterInstance::path_pretty)
            .collect();

        let history_size = self.history_items.len();
        let data_size = self.data_items.len();

        format!(
            "StoreItemsMessage(history: {:?}, data: {:?}, start: {:?}, last: {:?})",
            history_size, data_size, start, last
        )
    }

    pub fn from_proto(proto: StoreItemsMessageProto) -> Self {
        Self {
            start_path: proto
                .start_path
                .into_iter()
                .map(StoreNodeKey::from_proto)
                .collect(),
            last_path: proto
                .last_path
                .into_iter()
                .map(StoreNodeKey::from_proto)
                .collect(),
            history_items: proto
                .history_items
                .into_iter()
                .map(|store_item_proto| {
                    (
                        Blake2b256Hash::from_bytes(store_item_proto.key.to_vec()),
                        store_item_proto.value,
                    )
                })
                .collect(),
            data_items: proto
                .data_items
                .into_iter()
                .map(|store_item_proto| {
                    (
                        Blake2b256Hash::from_bytes(store_item_proto.key.to_vec()),
                        store_item_proto.value,
                    )
                })
                .collect(),
        }
    }

    pub fn to_proto(self) -> StoreItemsMessageProto {
        StoreItemsMessageProto {
            start_path: self.start_path.iter().map(StoreNodeKey::to_proto).collect(),
            last_path: self.last_path.iter().map(StoreNodeKey::to_proto).collect(),
            history_items: self
                .history_items
                .into_iter()
                .map(|(key, value)| StoreItemProto {
                    key: key.bytes().into(),
                    value,
                })
                .collect(),
            data_items: self
                .data_items
                .into_iter()
                .map(|(key, value)| StoreItemProto {
                    key: key.bytes().into(),
                    value,
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MergeableEntryRequest {
    pub block_hash: ByteString,
}

impl MergeableEntryRequest {
    pub fn from_proto(proto: MergeableEntryRequestProto) -> Self {
        Self {
            block_hash: proto.block_hash,
        }
    }

    pub fn to_proto(self) -> MergeableEntryRequestProto {
        MergeableEntryRequestProto {
            block_hash: self.block_hash,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MergeableEntryResponse {
    pub block_hash: ByteString,
    /// Deprecated compatibility field. Receivers ignore nonempty bytes because
    /// mergeable evidence is not block-authenticated.
    pub serialized_entry: ByteString,
}

impl MergeableEntryResponse {
    pub fn from_proto(proto: MergeableEntryResponseProto) -> Self {
        Self {
            block_hash: proto.block_hash,
            serialized_entry: proto.serialized_entry,
        }
    }

    pub fn to_proto(self) -> MergeableEntryResponseProto {
        MergeableEntryResponseProto {
            block_hash: self.block_hash,
            serialized_entry: self.serialized_entry,
        }
    }
}

// D3 (DR-9): the escrow-charge/refund kani proofs (over the now-deleted
// `checked_total_phlo_charge_value` / `refund_amount_for_token_cost_value`)
// are removed with the escrow model. The replacement supply-side
// no-underflow kani proof lives with the settlement writer (Commit 2 fuzz/
// kani retarget — see `docs/casper/theory/cost-accounting-impl/d3-replace-phlo-with-tokens.md`
// §Sequencing, Commit 2).

#[cfg(test)]
mod tests {
    use crypto::rust::private_key::PrivateKey;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::secp256k1_eth::Secp256k1Eth;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;
    use crypto::rust::signatures::signed::{Cosigned, Cosigner, Signed};
    use proptest::prelude::*;
    use prost::bytes::Bytes;

    use super::*;

    fn finalization_certificate() -> FinalizationCertificate {
        let target = BlockHashSerde(Bytes::from(vec![3; block_hash::LENGTH]));
        let latest = BlockHashSerde(Bytes::from(vec![4; block_hash::LENGTH]));
        let carrier = BlockHashSerde(Bytes::from(vec![9; block_hash::LENGTH]));
        FinalizationCertificate {
            schema_version: FinalizationCertificate::SCHEMA_VERSION,
            protocol_version: 6,
            shard_id: "root".to_string(),
            genesis_hash: BlockHashSerde(Bytes::from(vec![1; block_hash::LENGTH])),
            predecessor_floor_hash: BlockHashSerde(Bytes::from(vec![2; block_hash::LENGTH])),
            predecessor_certificate_digest: BlockHashSerde(Bytes::from(vec![
                5;
                block_hash::LENGTH
            ])),
            predecessor_certificate_block_hash: carrier.clone(),
            target_floor_hash: target.clone(),
            target_post_state_hash: BlockHashSerde(Bytes::from(vec![6; block_hash::LENGTH])),
            target_block_number: 9,
            fault_tolerance_numerator: 100_000,
            fault_tolerance_denominator: 1_000_000,
            exact_latest_messages: std::collections::BTreeMap::from([(
                ValidatorSerde(Bytes::from(vec![7; validator::LENGTH])),
                latest.clone(),
            )]),
            authority_context_digest: BlockHashSerde(Bytes::from(vec![8; block_hash::LENGTH])),
            supporting_manifest_digest: FinalizationCertificate::supporting_digest(
                &std::collections::BTreeSet::from([target.clone(), latest, carrier]),
            ),
            finalized_manifest_digest: FinalizationCertificate::finalized_digest(
                &std::collections::BTreeSet::from([target]),
            ),
            supporting_block_count: 3,
            finalized_block_count: 1,
        }
    }

    #[test]
    fn finalization_certificate_round_trip_preserves_canonical_digest() {
        let certificate = finalization_certificate();
        let decoded = FinalizationCertificate::from_proto(certificate.to_proto())
            .expect("canonical finalization certificate");
        assert_eq!(decoded, certificate);
        assert_eq!(decoded.digest(), certificate.digest());
        assert!(certificate.to_proto().encoded_len() <= FinalizationCertificate::MAX_ENCODED_BYTES);
    }

    #[test]
    fn finalization_certificate_request_and_response_are_content_addressed() {
        let certificate = finalization_certificate();
        let digest = certificate.digest();
        let request = FinalizationCertificateRequest::from_proto(
            FinalizationCertificateRequest {
                digest: digest.clone(),
            }
            .to_proto(),
        )
        .expect("valid certificate request");
        assert_eq!(request.digest, digest);

        let response = FinalizationCertificateResponse {
            digest: digest.clone(),
            certificate: certificate.clone(),
        };
        let proto = response.clone().to_proto();
        assert!(proto.encoded_len() <= FinalizationCertificateResponse::MAX_ENCODED_BYTES);
        assert_eq!(
            FinalizationCertificateResponse::from_proto(proto).expect("valid certificate response"),
            response
        );

        let mut mismatched = response.to_proto();
        mismatched.digest = Bytes::from(vec![11; block_hash::LENGTH]);
        assert!(FinalizationCertificateResponse::from_proto(mismatched).is_err());
    }

    #[test]
    fn finalization_certificate_requests_reject_non_digest_identifiers() {
        for digest in [Bytes::new(), Bytes::from(vec![0; block_hash::LENGTH + 1])] {
            assert!(FinalizationCertificateRequest::from_proto(
                FinalizationCertificateRequestProto { digest }
            )
            .is_err());
        }
    }

    #[test]
    fn finalization_certificate_manifest_commitments_are_domain_separated_and_binding() {
        let certificate = finalization_certificate();
        let singleton = std::collections::BTreeSet::from([certificate.target_floor_hash.clone()]);
        assert_ne!(
            FinalizationCertificate::supporting_digest(&singleton),
            FinalizationCertificate::finalized_digest(&singleton)
        );

        let commitment = certificate.commitment(certificate.authority_context_digest.0.clone());
        let mut tampered = certificate;
        tampered.supporting_manifest_digest =
            BlockHashSerde(Bytes::from(vec![10; block_hash::LENGTH]));
        assert_ne!(tampered.digest(), commitment.certificate_digest);
        assert!(tampered.validate_commitment(&commitment).is_err());
    }

    #[test]
    fn finalization_certificate_manifest_counts_are_digest_bound_and_bounded() {
        let certificate = finalization_certificate();
        let digest = certificate.digest();
        let mut tampered = certificate.clone();
        tampered.supporting_block_count += 1;
        assert_ne!(tampered.digest(), digest);

        for (supporting, finalized) in [
            (0, 1),
            (1, 0),
            (1, 2),
            (
                u32::try_from(FinalizationCertificate::MAX_SUPPORTING_BLOCKS).unwrap() + 1,
                1,
            ),
            (
                2,
                u32::try_from(FinalizationCertificate::MAX_FINALIZED_BLOCKS).unwrap() + 1,
            ),
        ] {
            let mut invalid = certificate.clone();
            invalid.supporting_block_count = supporting;
            invalid.finalized_block_count = finalized;
            assert!(invalid.validate_shape().is_err());
        }
    }

    #[test]
    fn finalization_certificate_rejects_latest_message_count_before_canonicalization() {
        let mut proto = finalization_certificate().to_proto();
        proto.exact_latest_messages = vec![
            JustificationProto {
                validator: Bytes::from(vec![7; validator::LENGTH]),
                latest_block_hash: Bytes::from(vec![4; block_hash::LENGTH]),
            };
            FinalizationCertificate::MAX_EXACT_LATEST_MESSAGES + 1
        ];
        let error = FinalizationCertificate::from_proto(proto).unwrap_err();
        assert!(error.contains("latest messages"));
    }

    #[test]
    fn maximum_supported_finalization_committee_fits_the_wire_budget() {
        let mut certificate = finalization_certificate();
        certificate.exact_latest_messages = (0..FinalizationCertificate::MAX_EXACT_LATEST_MESSAGES)
            .map(|index| {
                let index = u32::try_from(index).unwrap().to_be_bytes();
                let mut validator_bytes = vec![0; validator::LENGTH];
                validator_bytes[validator::LENGTH - index.len()..].copy_from_slice(&index);
                let mut hash_bytes = vec![0; block_hash::LENGTH];
                hash_bytes[block_hash::LENGTH - index.len()..].copy_from_slice(&index);
                (
                    ValidatorSerde(Bytes::from(validator_bytes)),
                    BlockHashSerde(Bytes::from(hash_bytes)),
                )
            })
            .collect();
        certificate.validate_shape().unwrap();
        assert!(certificate.to_proto().encoded_len() <= FinalizationCertificate::MAX_ENCODED_BYTES);
    }

    #[test]
    fn finalization_certificate_rejects_oversized_shard_identity() {
        let mut certificate = finalization_certificate();
        certificate.shard_id = "s".repeat(FinalizationCertificate::MAX_SHARD_ID_BYTES + 1);
        assert!(certificate.validate_shape().is_err());
    }

    #[test]
    fn finalization_certificate_binds_floor_state_and_certificate_digest() {
        let certificate = finalization_certificate();
        let commitment = FinalizedFloorCommitment {
            floor_hash: certificate.target_floor_hash.0.clone(),
            floor_post_state_hash: certificate.target_post_state_hash.0.clone(),
            certificate_digest: certificate.digest(),
            authority_context_digest: certificate.authority_context_digest.0.clone(),
        };
        certificate
            .validate_commitment(&commitment)
            .expect("matching commitment");

        for tampered in [
            FinalizedFloorCommitment {
                floor_hash: Bytes::from(vec![9; block_hash::LENGTH]),
                ..commitment.clone()
            },
            FinalizedFloorCommitment {
                floor_post_state_hash: Bytes::from(vec![9; block_hash::LENGTH]),
                ..commitment.clone()
            },
            FinalizedFloorCommitment {
                certificate_digest: Bytes::from(vec![9; block_hash::LENGTH]),
                ..commitment.clone()
            },
        ] {
            assert!(certificate.validate_commitment(&tampered).is_err());
        }

        let candidate_specific_context = FinalizedFloorCommitment {
            authority_context_digest: Bytes::from(vec![9; block_hash::LENGTH]),
            ..commitment
        };
        certificate
            .validate_commitment(&candidate_specific_context)
            .expect(
                "candidate authority context is bound by the signed block, not the certificate",
            );
    }

    #[test]
    fn equivocation_slash_canonicalizes_and_round_trips_both_hashes() {
        let first = Bytes::from_static(b"block-a");
        let second = Bytes::from_static(b"block-b");
        let issuer = PublicKey::from_bytes(b"issuer");
        let forward = SystemDeployData::create_equivocation_slash(
            first.clone(),
            second.clone(),
            issuer.clone(),
            7,
            BondGeneration::GENESIS,
        );
        let reverse = SystemDeployData::create_equivocation_slash(
            second.clone(),
            first.clone(),
            issuer,
            7,
            BondGeneration::GENESIS,
        );
        assert_eq!(forward, reverse);
        assert_eq!(
            SystemDeployData::from_proto(SystemDeployData::to_proto(forward.clone()))
                .expect("equivocation slash"),
            forward
        );
    }

    #[test]
    fn unary_slash_round_trip_preserves_absent_equivocation_hash() {
        let slash = SystemDeployData::create_slash(
            Bytes::from_static(b"invalid"),
            PublicKey::from_bytes(b"issuer"),
            9,
            BondGeneration::GENESIS,
        );
        let encoded = SystemDeployData::to_proto(slash.clone()).encode_to_vec();
        let decoded_proto = SystemDeployDataProto::decode(encoded.as_slice()).expect("slash proto");
        let decoded = SystemDeployData::from_proto(decoded_proto).expect("unary slash");
        assert_eq!(decoded, slash);
        assert!(matches!(decoded, SystemDeployData::Slash {
            equivocation_block_hash: None,
            ..
        }));
    }

    #[test]
    fn slash_proto_canonicalizes_empty_equivocation_hash_to_absence() {
        let slash = SystemDeployData::Slash {
            invalid_block_hash: Bytes::from_static(b"invalid"),
            equivocation_block_hash: Some(Bytes::new()),
            issuer_public_key: PublicKey::from_bytes(b"issuer"),
            target_activation_epoch: 9,
            target_bond_generation: BondGeneration::GENESIS,
        };
        let decoded =
            SystemDeployData::from_proto(SystemDeployData::to_proto(slash)).expect("slash proto");
        assert!(matches!(decoded, SystemDeployData::Slash {
            equivocation_block_hash: None,
            ..
        }));
    }

    fn deploy_data() -> DeployData {
        DeployData {
            term: "Nil".to_string(),
            language: "rholang".to_string(),
            time_stamp: 0,
            valid_after_block_number: 0,
            shard_id: "root".to_string(),
            expiration_timestamp: None,
            authority_presentations: Vec::new(),
        }
    }

    fn v61_envelope(selected: &[usize], threshold: u32) -> Cosigned<DeployData> {
        let secp = Secp256k1;
        let mut members = (0..3)
            .map(|_| {
                let (private_key, public_key) = secp.new_key_pair();
                (
                    Cosigner {
                        pk: public_key,
                        sig: Bytes::new(),
                        sig_algorithm: Box::new(secp.clone()),
                    },
                    private_key,
                )
            })
            .collect::<Vec<(Cosigner, PrivateKey)>>();
        members.sort_by_key(|(signer, _)| signer.principal_bytes_v61().unwrap());
        let mut bitmap = vec![0u8; members.len().div_ceil(8)];
        for index in selected {
            bitmap[index / 8] |= 1 << (index % 8);
        }
        let unsigned = members
            .iter()
            .map(|(signer, _)| signer.clone())
            .collect::<Vec<_>>();
        let data = deploy_data();
        for index in selected {
            let hash = Cosigned::<DeployData>::envelope_signing_hash_for_presence(
                &data,
                &unsigned,
                threshold,
                &bitmap,
                &members[*index].0.sig_algorithm.name(),
            )
            .unwrap();
            members[*index].0.sig = members[*index]
                .0
                .sig_algorithm
                .sign(&hash, &members[*index].1.bytes)
                .into();
        }
        Cosigned::from_envelope_signed_data_threshold(
            data,
            members.into_iter().map(|(signer, _)| signer).collect(),
            threshold,
        )
        .unwrap()
    }

    #[test]
    fn v61_wire_round_trip_preserves_authorization_and_identity() {
        let envelope = v61_envelope(&[0, 2], 2);
        let proto = DeployData::to_proto_cosigned(&envelope);
        let decoded = DeployData::from_proto_cosigned(proto).unwrap();
        assert_eq!(decoded, envelope);
        assert_eq!(
            decoded.envelope_commitment().unwrap(),
            envelope.envelope_commitment().unwrap()
        );
    }

    #[test]
    fn v61_wire_rejects_signer_order_presence_and_legacy_mutations() {
        let envelope = v61_envelope(&[0, 2], 2);
        let proto = DeployData::to_proto_cosigned(&envelope);

        let mut reordered = proto.clone();
        let policy = reordered
            .authorization_v61
            .as_mut()
            .unwrap()
            .policy
            .as_mut()
            .unwrap()
            .policy
            .as_mut()
            .unwrap();
        let crate::casper::authorization_policy_v61::Policy::Threshold(policy) = policy else {
            panic!("expected threshold policy");
        };
        policy.members.swap(0, 1);
        assert!(DeployData::from_proto_cosigned(reordered).is_err());

        let mut presence = proto.clone();
        let mut bitmap = presence
            .authorization_v61
            .as_ref()
            .unwrap()
            .presence_bitmap
            .to_vec();
        bitmap[0] ^= 0b0000_0010;
        presence.authorization_v61.as_mut().unwrap().presence_bitmap = bitmap.into();
        assert!(DeployData::from_proto_cosigned(presence).is_err());

        let mut coexistence = proto;
        coexistence.sig = Bytes::from_static(b"legacy");
        assert!(DeployData::from_proto_cosigned(coexistence).is_err());
    }

    #[test]
    fn v61_wire_rejects_noncanonical_threshold_n_of_n() {
        let envelope = v61_envelope(&[0, 1, 2], 3);
        let mut proto = DeployData::to_proto_cosigned(&envelope);
        let authorization = proto.authorization_v61.as_mut().unwrap();
        let policy = authorization.policy.as_mut().unwrap();
        let crate::casper::authorization_policy_v61::Policy::AllOf(all_of) =
            policy.policy.take().unwrap()
        else {
            panic!("expected all-of policy");
        };
        policy.policy = Some(crate::casper::authorization_policy_v61::Policy::Threshold(
            crate::casper::ThresholdPolicyV61 {
                minimum: all_of.members.len() as u32,
                members: all_of.members,
            },
        ));
        assert!(DeployData::from_proto_cosigned(proto).is_err());
    }

    #[test]
    fn v61_processed_deploy_primary_is_an_authenticated_witness() {
        let envelope = v61_envelope(&[1, 2], 2);
        let unsigned = envelope.signers()[0].pk.clone();
        let processed = ProcessedDeploy::empty_from_cosigned(&envelope);

        assert_ne!(processed.deploy.pk, unsigned);
        assert!(!processed.deploy.sig.is_empty());
        assert_eq!(processed.to_cosigned().unwrap(), envelope);
    }

    fn authority_signature(tag: u8) -> crate::rhoapi::CostSignature {
        crate::rhoapi::CostSignature {
            value: Some(crate::rhoapi::cost_signature::Value::Ground(vec![tag])),
        }
    }

    #[test]
    fn authority_presentations_are_signed_and_round_trip() {
        let mut data = deploy_data();
        data.authority_presentations = vec![authority_signature(1), authority_signature(2)];
        let signed = signed_deploy(data.clone());
        let proto = DeployData::to_proto(signed);
        let decoded = DeployData::from_proto(proto).unwrap();
        assert_eq!(decoded.data, data);

        let mut changed = data;
        changed.authority_presentations.push(authority_signature(3));
        assert_ne!(
            DeployData::_to_proto(decoded.data).encode_to_vec(),
            DeployData::_to_proto(changed).encode_to_vec()
        );
    }

    #[test]
    fn authority_presentation_signature_preimage_matches_python_client() {
        let mut data = deploy_data();
        data.authority_presentations = vec![
            crate::rhoapi::CostSignature {
                value: Some(crate::rhoapi::cost_signature::Value::Ground(vec![b'a'])),
            },
            crate::rhoapi::CostSignature {
                value: Some(crate::rhoapi::cost_signature::Value::Ground(vec![b'b'])),
            },
        ];
        assert_eq!(
            hex::encode(DeployData::_to_proto(data).encode_to_vec()),
            "12034e696c5a04726f6f749201030a01619201030a0162"
        );
    }

    #[test]
    fn authority_presentations_round_trip_through_bincode_storage() {
        for presentations in [Vec::new(), vec![
            authority_signature(1),
            authority_signature(2),
        ]] {
            let mut data = deploy_data();
            data.authority_presentations = presentations;
            let signed = signed_deploy(data);
            let encoded = bincode::serialize(&signed).expect("serialize stored deploy");
            let decoded: Signed<DeployData> =
                bincode::deserialize(&encoded).expect("deserialize stored deploy");
            assert_eq!(decoded, signed);
        }
    }

    #[test]
    fn authority_presentations_reject_noncanonical_order_and_duplicates() {
        for presentations in [vec![authority_signature(2), authority_signature(1)], vec![
            authority_signature(1),
            authority_signature(1),
        ]] {
            let mut data = deploy_data();
            data.authority_presentations = presentations;
            let proto = DeployData::to_proto(signed_deploy(data));
            assert!(DeployData::from_proto(proto)
                .unwrap_err()
                .contains("strictly ordered and unique"));
        }
    }

    fn signed_deploy(data: DeployData) -> Signed<DeployData> {
        let alg: Box<dyn SignaturesAlg> = Box::new(Secp256k1);
        let (sk, _) = alg.new_key_pair();
        Signed::create(data, alg, sk).expect("signed deploy")
    }

    #[test]
    fn rejected_deploy_occurrence_round_trips_through_proto() {
        let rejected = RejectedDeploy::occurrence_legacy(
            LegacyDeploySignature::new(b"deploy".to_vec()),
            Bytes::from_static(b"source"),
            RejectedDeployReason::DuplicateOccurrence,
        );

        assert_eq!(
            RejectedDeploy::from_proto(rejected.clone().to_proto()).unwrap(),
            rejected
        );
    }

    #[test]
    fn state_effects_round_trip_through_body_proto_without_reordering() {
        let effects = vec![
            StateEffectId {
                source_block_hash: Bytes::from(vec![1; block_hash::LENGTH]),
                execution_index: 2,
            },
            StateEffectId {
                source_block_hash: Bytes::from(vec![2; block_hash::LENGTH]),
                execution_index: 1,
            },
        ];
        let applied = vec![StateEffectId {
            source_block_hash: Bytes::from(vec![3; block_hash::LENGTH]),
            execution_index: 4,
        }];
        let body = Body {
            state: F1r3flyState {
                pre_state_hash: Bytes::from_static(b"pre"),
                post_state_hash: Bytes::from_static(b"post"),
                bonds: Vec::new(),
                bond_generations: Vec::new(),
                active_validators: Vec::new(),
                block_number: 7,
            },
            deploys: Vec::new(),
            rejected_deploys: Vec::new(),
            rejected_state_effects: effects.clone(),
            applied_state_effects: applied.clone(),
            system_deploys: Vec::new(),
            extra_bytes: Bytes::new(),
            applied_from_scope: Vec::new(),
            merge_base: Bytes::new(),
        };

        let decoded = Body::from_proto(body.to_proto()).unwrap();
        assert_eq!(decoded, body);
        assert_eq!(decoded.rejected_state_effects, effects);
        assert_eq!(decoded.applied_state_effects, applied);
    }

    #[test]
    fn body_proto_rejects_noncanonical_state_effect_sequences() {
        let first = StateEffectId {
            source_block_hash: Bytes::from(vec![1; block_hash::LENGTH]),
            execution_index: 0,
        };
        let second = StateEffectId {
            source_block_hash: Bytes::from(vec![2; block_hash::LENGTH]),
            execution_index: 0,
        };
        let mut body = Body {
            state: F1r3flyState {
                pre_state_hash: Bytes::from(vec![3; block_hash::LENGTH]),
                post_state_hash: Bytes::from(vec![4; block_hash::LENGTH]),
                bonds: Vec::new(),
                bond_generations: Vec::new(),
                active_validators: Vec::new(),
                block_number: 1,
            },
            deploys: Vec::new(),
            rejected_deploys: Vec::new(),
            rejected_state_effects: Vec::new(),
            applied_state_effects: vec![first.clone(), second.clone()],
            system_deploys: Vec::new(),
            extra_bytes: Bytes::new(),
            applied_from_scope: Vec::new(),
            merge_base: Bytes::new(),
        };

        body.applied_state_effects.reverse();
        assert!(Body::from_proto(body.to_proto())
            .expect_err("unordered effects must fail")
            .contains("strictly ordered"));

        body.applied_state_effects = vec![first.clone(), first];
        assert!(Body::from_proto(body.to_proto())
            .expect_err("duplicate effects must fail")
            .contains("duplicate"));

        body.applied_state_effects = vec![StateEffectId {
            source_block_hash: Bytes::from_static(b"short"),
            execution_index: 0,
        }];
        assert!(Body::from_proto(body.to_proto())
            .expect_err("malformed source hash must fail")
            .contains("expected 32 bytes"));
    }

    #[test]
    fn rejection_reason_join_uses_direct_cause_precedence() {
        assert_eq!(
            RejectedDeployReason::CollateralChainDrop
                .canonical_join(RejectedDeployReason::MergeConflict),
            RejectedDeployReason::MergeConflict
        );
        assert_eq!(
            RejectedDeployReason::MergeConflict
                .canonical_join(RejectedDeployReason::DuplicateOccurrence),
            RejectedDeployReason::DuplicateOccurrence
        );
        assert_eq!(
            RejectedDeployReason::Unspecified
                .canonical_join(RejectedDeployReason::CollateralChainDrop),
            RejectedDeployReason::CollateralChainDrop
        );
    }

    fn rejection_reason_from_byte(value: u8) -> RejectedDeployReason {
        match value % 4 {
            0 => RejectedDeployReason::Unspecified,
            1 => RejectedDeployReason::CollateralChainDrop,
            2 => RejectedDeployReason::MergeConflict,
            _ => RejectedDeployReason::DuplicateOccurrence,
        }
    }

    proptest! {
        #[test]
        fn rejection_reason_join_is_commutative(left: u8, right: u8) {
            let left = rejection_reason_from_byte(left);
            let right = rejection_reason_from_byte(right);
            prop_assert_eq!(left.canonical_join(right), right.canonical_join(left));
        }

        #[test]
        fn rejection_reason_join_is_associative(left: u8, middle: u8, right: u8) {
            let left = rejection_reason_from_byte(left);
            let middle = rejection_reason_from_byte(middle);
            let right = rejection_reason_from_byte(right);
            prop_assert_eq!(
                left.canonical_join(middle).canonical_join(right),
                left.canonical_join(middle.canonical_join(right))
            );
        }

        #[test]
        fn rejection_reason_join_is_idempotent(reason: u8) {
            let reason = rejection_reason_from_byte(reason);
            prop_assert_eq!(reason.canonical_join(reason), reason);
        }
    }

    #[test]
    fn legacy_rejected_deploy_proto_remains_readable() {
        let proto = RejectedDeployProto {
            sig: Bytes::from_static(b"deploy"),
            duplicate: false,
            carrier: Bytes::new(),
            source_block_hash: Bytes::new(),
            reason: 0,
            deploy_id_v6: Bytes::new(),
        };

        assert_eq!(
            RejectedDeploy::from_proto(proto).unwrap(),
            RejectedDeploy::legacy(Bytes::from_static(b"deploy"))
        );
    }

    /// Consensus-fork guard for the Workstream-B ground-`g` / quote-`#P`
    /// signature-atom split. The split adds `SigAtom.atom_kind` and the
    /// `Sig::Ground`/`Sig::Quote` runtime variants, but NONE of that may
    /// enter the deploy-signature preimage — otherwise every legacy
    /// single-signature deploy on chain would re-hash to a different
    /// `deploy_id` and the network would hard-fork.
    ///
    /// The preimage is `DeployData::_to_proto(..).encode_to_vec()`, and the
    /// signing digest is `Signed::signature_hash(alg, preimage)`. Both the
    /// preimage and the digest below were captured from the PRE-split code
    /// (the `_to_proto` body emits only term/timestamp/phlo_price/phlo_limit/
    /// valid_after_block_number/shard_id/expiration_timestamp and never a
    /// `SigAtom`/`atom_kind`/`sig_algebra`). If this assertion ever fails,
    /// the preimage changed — STOP: that is a consensus fork, and the fix is
    /// to exclude the offending field from `_to_proto`, never to update the
    /// pinned digest.
    #[test]
    fn deploy_signature_hash_excludes_retired_phlo_fields() {
        use prost::Message;

        // D3 (DR-9, fresh-genesis): the deploy-signature preimage NO LONGER
        // carries phloPrice (tag 7) / phloLimit (tag 8) — those tags are
        // reserved and `_to_proto` never emits them. This re-pins the preimage
        // and digest for the post-D3 single-sig wire shape. The retired tag
        // bytes (`3802...40...` for tags 7/8) MUST be absent.
        //
        // Fixed legacy single-sig deploy: term="Nil", timestamp=0,
        // valid_after_block_number=0, shard_id="root", no expiration.
        let data = deploy_data();

        let preimage = DeployData::_to_proto(data.clone()).encode_to_vec();
        // Post-D3 preimage. Field tags: 2=term("Nil"), 11=shardId("root").
        // timestamp/valid_after_block_number default to 0 (omitted). No tag 7
        // (phloPrice) or tag 8 (phloLimit), no SigAtom/atom_kind/sig_algebra.
        const PINNED_PREIMAGE_HEX: &str = "12034e696c5a04726f6f74";
        assert_eq!(
            hex::encode(&preimage),
            PINNED_PREIMAGE_HEX,
            "deploy-signature preimage changed — consensus fork risk; \
             do NOT update the pin, exclude the offending field from _to_proto"
        );
        // The retired phloPrice/phloLimit tag-7/8 bytes must NOT appear.
        assert!(
            !hex::encode(&preimage).contains("3802")
                && !preimage.windows(2).any(|w| w == [0x40, 0x05]),
            "retired phloPrice/phloLimit bytes must be absent from the D3 preimage"
        );

        // Blake2b256 digest of the post-D3 preimage (secp256k1 path).
        const PINNED_GOLDEN_DIGEST_HEX: &str =
            "c2ac266875edd634b52a2c7272ea7e1e06d5a33a1864ad90a471d56aa89b45df";
        let digest = Signed::<DeployData>::signature_hash(&Secp256k1::name(), preimage);
        assert_eq!(
            hex::encode(&digest),
            PINNED_GOLDEN_DIGEST_HEX,
            "post-D3 deploy signature_hash changed — re-pin only if the wire \
             shape intentionally changed (fresh-genesis)"
        );
    }

    /// A single-signature deploy serialized via the legacy `to_proto` path
    /// must NOT carry the multi-sig/algebra fields: `sig_algebra` is `None`
    /// and `cosigners` is empty. This is the structural complement to the
    /// golden-vector pin — it asserts the split did not start emitting a
    /// `SigCompound`/`SigAtom` onto the legacy single-sig wire shape.
    #[test]
    fn single_sig_to_proto_omits_sig_algebra_and_cosigners() {
        let signed = signed_deploy(deploy_data());
        let proto = DeployData::to_proto(signed);
        assert!(
            proto.sig_algebra.is_none(),
            "single-sig deploy must not emit sig_algebra"
        );
        assert!(
            proto.cosigners.is_empty(),
            "single-sig deploy must not emit cosigners"
        );
    }

    // D3 (DR-9): the escrow unit tests (`checked_total_phlo_charge_*`,
    // `refund_amount_is_bounded_by_valid_escrow`,
    // `settlement_edge_cases_are_total_and_deterministic`) and the
    // `refund_amount_property_is_bounded_by_valid_escrow` proptest are removed
    // with the escrow arithmetic they exercised. A deploy's cost is the
    // per-COMM token count (validated by the runtime/replay equivalence in the
    // `casper`/`rholang` crates), debited once from Σ⟦s⟧ — there is no per-deploy
    // charge/refund to bound.

    fn fresh_atom_signing(payload: &DeployData) -> (crate::casper::SigAtom, Vec<u8>) {
        let secp = Secp256k1;
        let (sk, pk) = secp.new_key_pair();
        let serialized = DeployData::_to_proto(payload.clone()).encode_to_vec();
        let hash = Signed::<DeployData>::signature_hash(&Secp256k1::name(), serialized);
        let sig = secp.sign(&hash, &sk.bytes);
        let pk_bytes_vec: Vec<u8> = pk.bytes.to_vec();
        (
            crate::casper::SigAtom {
                pk: pk.bytes.clone().into(),
                sig: prost::bytes::Bytes::from(sig),
                sig_algorithm: Secp256k1::name(),
                ..Default::default()
            },
            pk_bytes_vec,
        )
    }

    fn empty_atom() -> crate::casper::SigAtom {
        let secp = Secp256k1;
        let (_, pk) = secp.new_key_pair();
        crate::casper::SigAtom {
            pk: pk.bytes.into(),
            sig: prost::bytes::Bytes::new(),
            sig_algorithm: Secp256k1::name(),
            ..Default::default()
        }
    }

    #[test]
    fn from_proto_cosigned_sig_algebra_tensor_validates_both_branches() {
        let payload = deploy_data();
        let (atom_a, _) = fresh_atom_signing(&payload);
        let (atom_b, _) = fresh_atom_signing(&payload);
        let algebra = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Tensor(Box::new(
                crate::casper::SigPair {
                    left: Some(Box::new(crate::casper::SigCompound {
                        connective: Some(crate::casper::sig_compound::Connective::Atom(atom_a)),
                    })),
                    right: Some(Box::new(crate::casper::SigCompound {
                        connective: Some(crate::casper::sig_compound::Connective::Atom(atom_b)),
                    })),
                },
            ))),
        };
        let cosigned = DeployData::from_proto_cosigned_with_sig_algebra(payload, &algebra)
            .expect("Tensor with two valid signers must verify");
        assert_eq!(cosigned.signers().len(), 2);
    }

    /// F-A ingress reject (c): a `Plus` (⊕) `sig_algebra` is a VALUE/CAPABILITY
    /// connective, NOT a funding-signature former — it is now REJECTED at the
    /// deploy-decode boundary BEFORE any branch-witness validation (previously
    /// this test asserted the Plus was processed; F-A reclassifies ⊕ to the
    /// capability layer).
    #[test]
    fn from_proto_cosigned_sig_algebra_plus_rejected_at_ingress() {
        let payload = deploy_data();
        let (atom_a, _) = fresh_atom_signing(&payload);
        let atom_b_unsigned = empty_atom(); // not chosen, sig is empty
        let algebra = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Plus(Box::new(
                crate::casper::SigPlus {
                    left: Some(Box::new(crate::casper::SigCompound {
                        connective: Some(crate::casper::sig_compound::Connective::Atom(atom_a)),
                    })),
                    right: Some(Box::new(crate::casper::SigCompound {
                        connective: Some(crate::casper::sig_compound::Connective::Atom(
                            atom_b_unsigned,
                        )),
                    })),
                    chosen_branch: 0, // left
                },
            ))),
        };
        let err = DeployData::from_proto_cosigned_with_sig_algebra(payload, &algebra)
            .expect_err("Plus (⊕) is a capability connective, rejected at ingress");
        assert!(
            err.contains("Plus") && err.contains("not a funding-signature former"),
            "error must name the rejected ⊕/Plus connective: {}",
            err
        );
    }

    #[test]
    fn from_proto_cosigned_sig_algebra_threshold_2_of_3_satisfied() {
        let payload = deploy_data();
        let (atom_a, _) = fresh_atom_signing(&payload);
        let (atom_b, _) = fresh_atom_signing(&payload);
        let atom_c_unsigned = empty_atom();
        let algebra = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Threshold(
                crate::casper::SigThreshold {
                    threshold: 2,
                    members: vec![
                        crate::casper::SigCompound {
                            connective: Some(crate::casper::sig_compound::Connective::Atom(atom_a)),
                        },
                        crate::casper::SigCompound {
                            connective: Some(crate::casper::sig_compound::Connective::Atom(atom_b)),
                        },
                        crate::casper::SigCompound {
                            connective: Some(crate::casper::sig_compound::Connective::Atom(
                                atom_c_unsigned,
                            )),
                        },
                    ],
                },
            )),
        };
        let cosigned = DeployData::from_proto_cosigned_with_sig_algebra(payload, &algebra)
            .expect("Threshold 2-of-3 with 2 valid sigs must verify");
        assert_eq!(cosigned.signers().len(), 3);
    }

    /// F-A ingress reject (c): a `WhyNot` (?) `sig_algebra` is a
    /// VALUE/CAPABILITY connective and is REJECTED at the deploy-decode boundary
    /// — regardless of whether its wrapped atom is present, absent, valid, or
    /// invalid. Previously the absent / present-valid / present-invalid WhyNot
    /// cases each took a different decode branch; F-A reclassifies `?` to the
    /// capability layer, so all of them now fail at ingress with the SAME
    /// connective-rejection error (verified here for the absent and the
    /// present-invalid cases).
    #[test]
    fn from_proto_cosigned_sig_algebra_whynot_rejected_at_ingress() {
        // (i) absent (empty) wrapped atom.
        let payload = deploy_data();
        let absent_atom = empty_atom();
        let algebra_absent = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Whynot(Box::new(
                crate::casper::SigCompound {
                    connective: Some(crate::casper::sig_compound::Connective::Atom(absent_atom)),
                },
            ))),
        };
        let err_absent = DeployData::from_proto_cosigned_with_sig_algebra(payload, &algebra_absent)
            .expect_err("WhyNot (?) is a capability connective, rejected at ingress");
        assert!(
            err_absent.contains("WhyNot") && err_absent.contains("not a funding-signature former"),
            "error must name the rejected ?/WhyNot connective: {}",
            err_absent
        );

        // (ii) present-but-wrong-payload wrapped atom — still rejected at the
        // connective boundary BEFORE any signature verification runs.
        let payload2 = deploy_data();
        let other_payload = DeployData {
            term: "other-payload".to_string(),
            ..deploy_data()
        };
        let (atom_invalid, _) = fresh_atom_signing(&other_payload);
        let algebra_invalid = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Whynot(Box::new(
                crate::casper::SigCompound {
                    connective: Some(crate::casper::sig_compound::Connective::Atom(atom_invalid)),
                },
            ))),
        };
        let err_invalid =
            DeployData::from_proto_cosigned_with_sig_algebra(payload2, &algebra_invalid)
                .expect_err("present-invalid WhyNot is still rejected at the connective boundary");
        assert!(
            err_invalid.contains("WhyNot")
                && !err_invalid.contains("failed signature verification"),
            "ingress reject must fire BEFORE signature verification: {}",
            err_invalid
        );
    }

    /// F-A ingress reject (c): a `Plus` (⊕) `sig_algebra` is rejected at the
    /// connective boundary BEFORE the `chosen_branch` range check — so even a
    /// structurally invalid `chosen_branch` surfaces the connective-rejection,
    /// not the branch error (previously this asserted the `chosen_branch`
    /// message). The funding grammar has no ⊕ former at all.
    #[test]
    fn from_proto_cosigned_sig_algebra_plus_rejected_before_chosen_branch_check() {
        let payload = deploy_data();
        let (atom_a, _) = fresh_atom_signing(&payload);
        let (atom_b, _) = fresh_atom_signing(&payload);
        let algebra = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Plus(Box::new(
                crate::casper::SigPlus {
                    left: Some(Box::new(crate::casper::SigCompound {
                        connective: Some(crate::casper::sig_compound::Connective::Atom(atom_a)),
                    })),
                    right: Some(Box::new(crate::casper::SigCompound {
                        connective: Some(crate::casper::sig_compound::Connective::Atom(atom_b)),
                    })),
                    chosen_branch: 2, // invalid — but the connective is rejected first
                },
            ))),
        };
        let err = DeployData::from_proto_cosigned_with_sig_algebra(payload, &algebra)
            .expect_err("Plus (⊕) rejected at ingress before chosen_branch validation");
        assert!(
            err.contains("Plus") && err.contains("not a funding-signature former"),
            "ingress reject must fire before the chosen_branch check: {}",
            err
        );
    }

    #[test]
    fn processed_deploy_cosigner_threshold_roundtrips_through_proto() {
        let processed = ProcessedDeploy {
            deploy: signed_deploy(deploy_data()),
            envelope_commitment: ByteString::new(),
            cost: PCost { cost: 0 },
            deploy_log: Vec::new(),
            is_failed: false,
            system_deploy_error: None,
            cosigners: Vec::new(),
            cosigner_threshold: 2,
            pre_state_hash: ByteString::new(),
            post_state_hash: ByteString::new(),
            authority_funding_certificate: None,
            authority_cost_witness: None,
            admission_status: Default::default(),
        };

        let decoded = ProcessedDeploy::from_proto(processed.clone().to_proto()).unwrap();
        assert_eq!(decoded.cosigner_threshold, 2);
    }

    #[test]
    fn processed_deploy_secp256k1_eth_roundtrips_through_proto() {
        let algorithm: Box<dyn SignaturesAlg> = Box::new(Secp256k1Eth);
        let (private_key, _) = algorithm.new_key_pair();
        let deploy = Signed::create(deploy_data(), algorithm, private_key).unwrap();
        let processed = ProcessedDeploy {
            deploy,
            envelope_commitment: ByteString::new(),
            cost: PCost { cost: 1 },
            deploy_log: Vec::new(),
            is_failed: false,
            system_deploy_error: None,
            cosigners: Vec::new(),
            cosigner_threshold: 0,
            pre_state_hash: ByteString::new(),
            post_state_hash: ByteString::new(),
            authority_funding_certificate: None,
            authority_cost_witness: None,
            admission_status: Default::default(),
        };

        let proto = processed.clone().to_proto();
        assert_eq!(
            proto.deploy.as_ref().unwrap().sig_algorithm,
            Secp256k1Eth::NAME
        );
        assert_eq!(ProcessedDeploy::from_proto(proto).unwrap(), processed);
    }

    #[test]
    fn deploy_info_preserves_cost_authority_evidence() {
        let certificate = CostAuthorityFundingCertificateProto {
            protocol_version: 8,
            program_hash: ByteString::from_static(&[1; 32]),
            pre_state_root: ByteString::from_static(&[2; 32]),
            reservation_id: ByteString::from_static(&[3; 32]),
            byte_cost_schedule_version: 1,
            byte_cost_schedule_digest: ByteString::from_static(&[4; 32]),
            byte_cost_bound: 17,
            ..Default::default()
        };
        let witness = CostAuthorityWitnessProto {
            protocol_version: 8,
            certificate_id: ByteString::from_static(&[5; 32]),
            pre_state_root: ByteString::from_static(&[2; 32]),
            post_state_root: ByteString::from_static(&[6; 32]),
            byte_cost_schedule_version: 1,
            byte_cost_schedule_digest: ByteString::from_static(&[4; 32]),
            byte_cost: 11,
            ..Default::default()
        };
        let pre_state = ByteString::from_static(&[2; 32]);
        let post_state = ByteString::from_static(&[6; 32]);
        let processed = ProcessedDeploy {
            deploy: signed_deploy(deploy_data()),
            envelope_commitment: ByteString::new(),
            cost: PCost { cost: 12 },
            deploy_log: Vec::new(),
            is_failed: false,
            system_deploy_error: None,
            cosigners: Vec::new(),
            cosigner_threshold: 0,
            pre_state_hash: pre_state.clone(),
            post_state_hash: post_state.clone(),
            authority_funding_certificate: Some(certificate.clone()),
            authority_cost_witness: Some(witness.clone()),
            admission_status: DeployAdmissionStatus::Executed,
        };

        let info = processed.to_deploy_info();

        assert_eq!(info.authority_funding_certificate, Some(certificate));
        assert_eq!(info.authority_cost_witness, Some(witness));
        assert_eq!(info.pre_state_hash, pre_state);
        assert_eq!(info.post_state_hash, post_state);
        assert_eq!(
            info.admission_status,
            DeployAdmissionStatusProto::DeployAdmissionStatusExecuted as i32
        );
    }

    #[test]
    fn funding_admission_rejection_roundtrips_as_terminal_non_execution() {
        let signed = signed_deploy(deploy_data());
        let cosigned =
            crypto::rust::signatures::signed::Cosigned::from_single_signer(signed).unwrap();
        let pre_state = ByteString::from_static(&[7; 32]);
        let rejected = ProcessedDeploy::admission_rejected(&cosigned, pre_state.clone());

        assert_eq!(rejected.admission_status, DeployAdmissionStatus::Rejected);
        assert!(rejected.is_failed);
        assert_eq!(rejected.cost.cost, 0);
        assert!(rejected.deploy_log.is_empty());
        assert_eq!(rejected.pre_state_hash, pre_state);
        assert_eq!(rejected.post_state_hash, pre_state);
        assert_eq!(
            ProcessedDeploy::from_proto(rejected.clone().to_proto()).unwrap(),
            rejected
        );
        assert!(!rejected.has_committed_state_effect());
    }

    #[test]
    fn failed_state_bound_execution_keeps_its_committed_settlement_effect() {
        let signed = signed_deploy(deploy_data());
        let mut processed = ProcessedDeploy::empty(signed);
        processed.is_failed = true;
        assert!(!processed.has_committed_state_effect());

        processed.authority_funding_certificate =
            Some(CostAuthorityFundingCertificateProto::default());
        assert!(!processed.has_committed_state_effect());

        processed.authority_cost_witness = Some(CostAuthorityWitnessProto::default());
        assert!(processed.has_committed_state_effect());

        processed.is_failed = false;
        processed.authority_funding_certificate = None;
        processed.authority_cost_witness = None;
        assert!(processed.has_committed_state_effect());
    }

    // =================================================================
    // F-A funding/capability separation — INGRESS REJECT (c) tests.
    //
    // `docs/casper/theory/cost-accounting-impl/f-a-funding-vs-capability-separation.md`
    // §3/§6: the deploy-decode path
    // (`from_proto_cosigned_with_sig_algebra`) REJECTS the five value/capability
    // type-logic connectives (`Plus` ⊕ / `With` & / `Bang` ! / `WhyNot` ? /
    // `Lolly` ⊸) — they are NOT funding-signature formers (§App-A `g|#P|s∘s`).
    // `Atom`, `Tensor` (the funding `And` ∘) and `Threshold` (a k-of-N
    // admission-boundary quorum, F-A Threshold=(A)) are KEPT. Plus + WhyNot
    // rejection are covered by the two repaired tests above; the remaining three
    // connectives (`Bang`/`Lolly`/`With`) + nesting + the Threshold-kept and
    // flat-N-of-N no-regression cases are pinned here.
    // =================================================================

    fn atom_compound(payload: &DeployData) -> crate::casper::SigCompound {
        let (atom, _) = fresh_atom_signing(payload);
        crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Atom(atom)),
        }
    }

    /// `Bang` (!) at the top level is rejected at ingress.
    #[test]
    fn from_proto_cosigned_sig_algebra_bang_rejected_at_ingress() {
        let payload = deploy_data();
        let algebra = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Bang(Box::new(
                crate::casper::SigBang {
                    inner: Some(Box::new(atom_compound(&payload))),
                    uses_bound: 0,
                    capability_handle: prost::bytes::Bytes::new(),
                },
            ))),
        };
        let err = DeployData::from_proto_cosigned_with_sig_algebra(payload, &algebra)
            .expect_err("Bang (!) is a capability connective, rejected at ingress");
        assert!(
            err.contains("Bang") && err.contains("not a funding-signature former"),
            "error must name the rejected !/Bang connective: {}",
            err
        );
    }

    /// `Lolly` (⊸) at the top level is rejected at ingress.
    #[test]
    fn from_proto_cosigned_sig_algebra_lolly_rejected_at_ingress() {
        let payload = deploy_data();
        let algebra = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Lolly(Box::new(
                crate::casper::SigLolly {
                    from: Some(Box::new(atom_compound(&payload))),
                    to: Some(Box::new(atom_compound(&payload))),
                    capability_handle: prost::bytes::Bytes::new(),
                },
            ))),
        };
        let err = DeployData::from_proto_cosigned_with_sig_algebra(payload, &algebra)
            .expect_err("Lolly (⊸) is a capability connective, rejected at ingress");
        assert!(
            err.contains("Lolly") && err.contains("not a funding-signature former"),
            "error must name the rejected ⊸/Lolly connective: {}",
            err
        );
    }

    /// `With` (&) at the top level is rejected at ingress.
    #[test]
    fn from_proto_cosigned_sig_algebra_with_rejected_at_ingress() {
        let payload = deploy_data();
        let algebra = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::With(Box::new(
                crate::casper::SigPair {
                    left: Some(Box::new(atom_compound(&payload))),
                    right: Some(Box::new(atom_compound(&payload))),
                },
            ))),
        };
        let err = DeployData::from_proto_cosigned_with_sig_algebra(payload, &algebra)
            .expect_err("With (&) is a capability connective, rejected at ingress");
        assert!(
            err.contains("With") && err.contains("not a funding-signature former"),
            "error must name the rejected &/With connective: {}",
            err
        );
    }

    /// A capability connective NESTED inside an otherwise-funding `Tensor`
    /// (`Atom ∘ Bang`) is still caught — the reject walk recurses through the
    /// tensor's sides.
    #[test]
    fn from_proto_cosigned_sig_algebra_connective_nested_in_tensor_rejected() {
        let payload = deploy_data();
        let bang = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Bang(Box::new(
                crate::casper::SigBang {
                    inner: Some(Box::new(atom_compound(&payload))),
                    uses_bound: 0,
                    capability_handle: prost::bytes::Bytes::new(),
                },
            ))),
        };
        let algebra = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Tensor(Box::new(
                crate::casper::SigPair {
                    left: Some(Box::new(atom_compound(&payload))),
                    right: Some(Box::new(bang)),
                },
            ))),
        };
        let err = DeployData::from_proto_cosigned_with_sig_algebra(payload, &algebra)
            .expect_err("a Bang nested in a Tensor must still be rejected");
        assert!(
            err.contains("Bang") && err.contains("not a funding-signature former"),
            "nested-connective reject must name the offending connective: {}",
            err
        );
    }

    /// A capability connective NESTED inside a `Threshold` member is still
    /// caught — the reject walk recurses through threshold members (Threshold
    /// itself is kept, but its members must be funding-grammar).
    #[test]
    fn from_proto_cosigned_sig_algebra_connective_nested_in_threshold_member_rejected() {
        let payload = deploy_data();
        let lolly = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Lolly(Box::new(
                crate::casper::SigLolly {
                    from: Some(Box::new(atom_compound(&payload))),
                    to: Some(Box::new(atom_compound(&payload))),
                    capability_handle: prost::bytes::Bytes::new(),
                },
            ))),
        };
        let algebra = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Threshold(
                crate::casper::SigThreshold {
                    threshold: 1,
                    members: vec![atom_compound(&payload), lolly],
                },
            )),
        };
        let err = DeployData::from_proto_cosigned_with_sig_algebra(payload, &algebra)
            .expect_err("a Lolly nested in a Threshold member must still be rejected");
        assert!(
            err.contains("Lolly") && err.contains("not a funding-signature former"),
            "nested-in-threshold reject must name the offending connective: {}",
            err
        );
    }

    /// F-A Threshold=(A): a `Threshold` k-of-N (with funding-grammar atom
    /// members) is STILL ACCEPTED — it is an admission-boundary quorum, lowered
    /// to a flat `Cosigned` + scalar `cosigner_threshold`, NOT rejected by the
    /// ingress guard. (Mirrors the kept Tensor case which the existing
    /// `..._tensor_validates_both_branches` test already covers.)
    #[test]
    fn from_proto_cosigned_sig_algebra_threshold_still_accepted_post_f_a() {
        let payload = deploy_data();
        let (atom_a, _) = fresh_atom_signing(&payload);
        let (atom_b, _) = fresh_atom_signing(&payload);
        let atom_c_unsigned = empty_atom();
        let algebra = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Threshold(
                crate::casper::SigThreshold {
                    threshold: 2,
                    members: vec![
                        crate::casper::SigCompound {
                            connective: Some(crate::casper::sig_compound::Connective::Atom(atom_a)),
                        },
                        crate::casper::SigCompound {
                            connective: Some(crate::casper::sig_compound::Connective::Atom(atom_b)),
                        },
                        crate::casper::SigCompound {
                            connective: Some(crate::casper::sig_compound::Connective::Atom(
                                atom_c_unsigned,
                            )),
                        },
                    ],
                },
            )),
        };
        let cosigned = DeployData::from_proto_cosigned_with_sig_algebra(payload, &algebra)
            .expect("Threshold 2-of-3 (funding atoms) must still verify post-F-A");
        assert_eq!(cosigned.signers().len(), 3);
        assert_eq!(cosigned.cosigner_threshold(), 2);
    }

    /// No-regression: a plain flat N-of-N deploy (`cosigners[]`, NO
    /// `sig_algebra`) decodes through the legacy `from_proto_cosigned` path
    /// exactly as before F-A (the ingress reject only touches the `sig_algebra`
    /// branch). Pairs with `single_sig_to_proto_omits_sig_algebra_and_cosigners`
    /// (single-sig wire shape) and the `deploy_signature_hash_excludes_*` golden
    /// pin (single-sig byte identity).
    #[test]
    fn flat_n_of_n_without_sig_algebra_still_decodes_post_f_a() {
        use prost::Message;
        let payload = deploy_data();
        let serialized = DeployData::_to_proto(payload.clone()).encode_to_vec();
        let hash = Signed::<DeployData>::signature_hash(&Secp256k1::name(), serialized);
        let secp = Secp256k1;

        let (sk_primary, pk_primary) = secp.new_key_pair();
        let (sk_co, pk_co) = secp.new_key_pair();
        let proto = DeployDataProto {
            deployer: pk_primary.bytes.clone().into(),
            term: payload.term.clone(),
            timestamp: payload.time_stamp,
            sig: prost::bytes::Bytes::from(secp.sign(&hash, &sk_primary.bytes)),
            sig_algorithm: Secp256k1::name(),
            valid_after_block_number: payload.valid_after_block_number,
            shard_id: payload.shard_id.clone(),
            language: String::new(),
            expiration_timestamp: 0,
            cosigners: vec![CompoundSigner {
                pk: pk_co.bytes.clone().into(),
                sig: prost::bytes::Bytes::from(secp.sign(&hash, &sk_co.bytes)),
                sig_algorithm: Secp256k1::name(),
            }],
            cosigner_threshold: 0, // N-of-N
            sig_algebra: None,
            authority_presentations: Vec::new(),
            deploy_id: ByteString::new(),
            authorization_v61: None,
        };
        let cosigned = DeployData::from_proto_cosigned_legacy(proto)
            .expect("flat N-of-N (no sig_algebra) must decode unchanged post-F-A");
        assert_eq!(cosigned.signers().len(), 2);
        assert!(cosigned.is_compound());
    }

    #[test]
    fn sig_algebra_wire_dispatch_ignores_all_flat_envelope_fields() {
        let payload = deploy_data();
        let algebra = atom_compound(&payload);
        let proto = DeployDataProto {
            term: payload.term.clone(),
            language: payload.language.clone(),
            timestamp: payload.time_stamp,
            valid_after_block_number: payload.valid_after_block_number,
            shard_id: payload.shard_id.clone(),
            sig_algorithm: "unused-invalid-algorithm".to_string(),
            cosigner_threshold: -1,
            cosigners: vec![CompoundSigner {
                pk: Bytes::new(),
                sig: Bytes::new(),
                sig_algorithm: "unused-invalid-cosigner-algorithm".to_string(),
            }],
            sig_algebra: Some(algebra),
            ..Default::default()
        };

        let cosigned = DeployData::from_proto_cosigned_legacy(proto)
            .expect("sig algebra must completely override the flat envelope fields");
        assert_eq!(cosigned.signers().len(), 1);
    }

    #[test]
    fn flat_threshold_wire_boundary_accepts_valid_quorum_and_rejects_both_invalid_bounds() {
        use prost::Message;

        let payload = deploy_data();
        let serialized = DeployData::_to_proto(payload.clone()).encode_to_vec();
        let hash = Signed::<DeployData>::signature_hash(&Secp256k1::name(), serialized);
        let secp = Secp256k1;
        let (primary_sk, primary_pk) = secp.new_key_pair();
        let (_, placeholder_pk) = secp.new_key_pair();
        let base = DeployDataProto {
            deployer: primary_pk.bytes.clone().into(),
            term: payload.term,
            language: payload.language,
            timestamp: payload.time_stamp,
            sig: Bytes::from(secp.sign(&hash, &primary_sk.bytes)),
            sig_algorithm: Secp256k1::name(),
            valid_after_block_number: payload.valid_after_block_number,
            shard_id: payload.shard_id,
            cosigners: vec![CompoundSigner {
                pk: placeholder_pk.bytes.into(),
                sig: Bytes::new(),
                sig_algorithm: Secp256k1::name(),
            }],
            cosigner_threshold: 1,
            ..Default::default()
        };

        let cosigned = DeployData::from_proto_cosigned_legacy(base.clone())
            .expect("one valid signer must satisfy a one-of-two threshold");
        assert_eq!(cosigned.cosigner_threshold(), 1);

        let mut negative = base.clone();
        negative.cosigner_threshold = -1;
        assert!(DeployData::from_proto_cosigned_legacy(negative)
            .unwrap_err()
            .contains("Invalid cosigner_threshold"));

        let mut excessive = base;
        excessive.cosigner_threshold = 3;
        assert!(DeployData::from_proto_cosigned_legacy(excessive)
            .unwrap_err()
            .contains("Invalid cosigner_threshold"));
    }

    #[test]
    fn funding_algebra_rejects_invalid_threshold_shapes_and_missing_structure() {
        let payload = deploy_data();
        let member = atom_compound(&payload);
        for (threshold, members) in [(0, vec![member.clone()]), (2, vec![member.clone()])] {
            let algebra = crate::casper::SigCompound {
                connective: Some(crate::casper::sig_compound::Connective::Threshold(
                    crate::casper::SigThreshold { threshold, members },
                )),
            };
            assert!(
                DeployData::from_proto_cosigned_with_sig_algebra(payload.clone(), &algebra)
                    .unwrap_err()
                    .contains("SigThreshold.threshold")
            );
        }

        let missing_member = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Threshold(
                crate::casper::SigThreshold {
                    threshold: 1,
                    members: vec![crate::casper::SigCompound { connective: None }],
                },
            )),
        };
        assert!(
            DeployData::from_proto_cosigned_with_sig_algebra(payload.clone(), &missing_member)
                .unwrap_err()
                .contains("member connective missing")
        );

        let nested_tensor = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Threshold(
                crate::casper::SigThreshold {
                    threshold: 1,
                    members: vec![crate::casper::SigCompound {
                        connective: Some(crate::casper::sig_compound::Connective::Tensor(
                            Box::new(crate::casper::SigPair {
                                left: Some(Box::new(member.clone())),
                                right: Some(Box::new(member)),
                            }),
                        )),
                    }],
                },
            )),
        };
        assert!(
            DeployData::from_proto_cosigned_with_sig_algebra(payload.clone(), &nested_tensor)
                .unwrap_err()
                .contains("atomic candidate signers")
        );

        let missing_root = crate::casper::SigCompound { connective: None };
        assert!(
            DeployData::from_proto_cosigned_with_sig_algebra(payload.clone(), &missing_root)
                .unwrap_err()
                .contains("SigCompound.connective missing")
        );

        for pair in [
            crate::casper::SigPair {
                left: None,
                right: Some(Box::new(atom_compound(&payload))),
            },
            crate::casper::SigPair {
                left: Some(Box::new(atom_compound(&payload))),
                right: None,
            },
        ] {
            let algebra = crate::casper::SigCompound {
                connective: Some(crate::casper::sig_compound::Connective::Tensor(Box::new(
                    pair,
                ))),
            };
            assert!(
                DeployData::from_proto_cosigned_with_sig_algebra(payload.clone(), &algebra)
                    .is_err()
            );
        }
    }

    #[test]
    fn tensor_containing_threshold_is_rejected_instead_of_flattening_policy() {
        let payload = deploy_data();
        let threshold = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Threshold(
                crate::casper::SigThreshold {
                    threshold: 1,
                    members: vec![atom_compound(&payload), atom_compound(&payload)],
                },
            )),
        };
        let algebra = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Tensor(Box::new(
                crate::casper::SigPair {
                    left: Some(Box::new(threshold)),
                    right: Some(Box::new(atom_compound(&payload))),
                },
            ))),
        };

        let err = DeployData::from_proto_cosigned_with_sig_algebra(payload, &algebra).expect_err(
            "flattening Tensor(Threshold(1-of-2), Atom) would erase the mandatory atom",
        );
        assert!(
            err.contains("top-level admission connective")
                && err.contains("cannot preserve Tensor composition"),
            "nested threshold rejection must explain the policy-preservation boundary: {err}"
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            cases: 64,
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn every_threshold_nested_under_tensor_is_rejected(
            member_count in 1usize..8,
            threshold_seed in any::<usize>(),
            threshold_on_left in any::<bool>(),
        ) {
            let payload = deploy_data();
            let threshold = crate::casper::SigCompound {
                connective: Some(crate::casper::sig_compound::Connective::Threshold(
                    crate::casper::SigThreshold {
                        threshold: (1 + threshold_seed % member_count) as i32,
                        members: (0..member_count)
                            .map(|_| crate::casper::SigCompound {
                                connective: Some(crate::casper::sig_compound::Connective::Atom(
                                    empty_atom(),
                                )),
                            })
                            .collect(),
                    },
                )),
            };
            let atom = Box::new(atom_compound(&payload));
            let threshold = Box::new(threshold);
            let (left, right) = if threshold_on_left {
                (threshold, atom)
            } else {
                (atom, threshold)
            };
            let algebra = crate::casper::SigCompound {
                connective: Some(crate::casper::sig_compound::Connective::Tensor(Box::new(
                    crate::casper::SigPair {
                        left: Some(left),
                        right: Some(right),
                    },
                ))),
            };

            let err = DeployData::from_proto_cosigned_with_sig_algebra(payload, &algebra)
                .expect_err("no threshold policy can be nested under a tensor");
            prop_assert!(err.contains("top-level admission connective"));
        }
    }

    fn candidate() -> ApprovedBlockCandidate {
        ApprovedBlockCandidate {
            block: BlockMessage {
                block_hash: Bytes::from_static(b"anchor"),
                header: Header {
                    parents_hash_list: vec![],
                    timestamp: 0,
                    version: 0,
                    extra_bytes: Bytes::new(),
                    sender_bond_generation: None,
                    objective_equivocation_evidence_delta: vec![],
                    finalized_floor: None,
                },
                body: Body {
                    state: F1r3flyState {
                        pre_state_hash: Bytes::new(),
                        post_state_hash: Bytes::new(),
                        bonds: vec![],
                        bond_generations: vec![],
                        active_validators: vec![],
                        block_number: 87,
                    },
                    deploys: vec![],
                    rejected_deploys: vec![],
                    rejected_state_effects: vec![],
                    applied_state_effects: vec![],
                    system_deploys: vec![],
                    extra_bytes: Bytes::new(),
                    applied_from_scope: vec![],
                    merge_base: Bytes::new(),
                },
                justifications: vec![],
                sender: Bytes::new(),
                seq_num: 0,
                sig: Bytes::new(),
                sig_algorithm: String::new(),
                shard_id: "root".to_string(),
                extra_bytes: Bytes::new(),
                finalized_floor_certificate: None,
            },
            required_sigs: 0,
        }
    }

    /// The finalized-floor cache travels with the LFS window. A restored node
    /// cannot derive floors for blocks below its anchor — the derivation
    /// recurses through history it deliberately does not keep — and without
    /// them every sibling-branch validation crawls gap-by-gap toward genesis.
    /// The responder computed these values when it validated the blocks; the
    /// numbers are hashes only, a few KB for a window whose size is constant
    /// in chain height.
    #[test]
    fn the_floor_cache_survives_the_wire() {
        let entry = FloorCacheEntry {
            block_hash: Bytes::from_static(b"window-block"),
            floor_hash: Bytes::from_static(b"its-floor"),
            frontier_hash: Bytes::from_static(b"its-frontier"),
        };
        let request = FloorCacheRequest {
            hashes: vec![Bytes::from_static(b"window-block")],
        };
        let response = FloorCacheResponse {
            entries: vec![entry],
            genesis_hash: Bytes::from_static(b"the-genesis"),
            genesis_block: Some(crate::rust::block_implicits::get_random_block_default()),
        };

        assert_eq!(
            FloorCacheRequest::from_proto(request.clone().to_proto()),
            request,
            "the requested hash set must survive the wire"
        );
        assert_eq!(
            FloorCacheResponse::from_proto(response.clone().to_proto()),
            response,
            "every entry must survive intact: the receiver writes these into the \
             same caches its own validation would have filled"
        );
    }

    /// The seed rides on the ApprovedBlock, never inside its candidate: the
    /// candidate's serialized bytes are what the genesis ceremony signs and
    /// what `Validate::approved_block` re-derives to verify, so a field added
    /// there would put unsigned peer-supplied data inside the signed envelope
    /// and make two ceremony participants disagree on the digest.
    #[test]
    fn the_floor_seed_survives_the_wire_and_the_candidate_digest_does_not_move() {
        let seed = FinalizedFloorSeed {
            floor_hash: Bytes::from_static(b"floor"),
            floor_number: 37,
            frontier_hash: Bytes::from_static(b"frontier"),
            frontier_number: 41,
        };
        let seeded = ApprovedBlock {
            candidate: candidate(),
            sigs: vec![],
            floor_seed: Some(seed.clone()),
        };
        let bare = ApprovedBlock {
            candidate: candidate(),
            sigs: vec![],
            floor_seed: None,
        };

        assert_eq!(
            ApprovedBlock::from_proto(seeded.clone().to_proto()).expect("round trip"),
            seeded,
            "the seed must survive the wire intact: the receiver sizes its download \
             window from these numbers before it requests a single block"
        );
        assert_eq!(
            ApprovedBlock::from_proto(bare.clone().to_proto()).expect("round trip"),
            bare,
            "a peer that sends no seed must decode as no seed, not as a zero floor"
        );

        assert_eq!(
            seeded
                .to_proto()
                .candidate
                .expect("candidate")
                .encode_to_vec(),
            bare.to_proto()
                .candidate
                .expect("candidate")
                .encode_to_vec(),
            "seeding must not shift one byte of the candidate: those bytes are the \
             ceremony's signed payload"
        );
    }
}

#[cfg(test)]
mod coverage_tests {
    use proptest::strategy::{Strategy, ValueTree};
    use proptest::test_runner::TestRunner;

    use super::*;
    use crate::rust::block_implicits::{get_random_block_default, signed_deploy_data_gen};

    type Bytes = prost::bytes::Bytes;

    fn signed_deploy() -> Signed<DeployData> {
        signed_deploy_data_gen()
            .new_tree(&mut TestRunner::default())
            .unwrap()
            .current()
    }

    fn produce_event() -> ProduceEvent {
        ProduceEvent {
            channels_hash: Bytes::from_static(b"chan"),
            hash: Bytes::from_static(b"produce"),
            persistent: true,
            times_repeated: 2,
            is_deterministic: false,
            output_value: vec![Bytes::from_static(b"out")],
            failed: true,
        }
    }

    fn consume_event() -> ConsumeEvent {
        ConsumeEvent {
            channels_hashes: vec![Bytes::from_static(b"c1"), Bytes::from_static(b"c2")],
            hash: Bytes::from_static(b"consume"),
            persistent: false,
        }
    }

    fn hash32(fill: u8) -> Blake2b256Hash { Blake2b256Hash::from_bytes(vec![fill; 32]) }

    #[test]
    fn rejected_deploy_decodes_legacy_wire_format() {
        let legacy = [0x0a, 0x03, b's', b'i', b'g'];
        let decoded = RejectedDeployProto::decode(legacy.as_slice()).unwrap();
        let record = RejectedDeploy::from_proto(decoded).unwrap();

        assert_eq!(record.deploy_id(), b"sig");
        assert!(!record.is_duplicate());
        assert!(record.source_block_hash.is_empty());
    }

    #[test]
    fn hash_addressed_messages_round_trip() {
        let has_block = HasBlock {
            hash: Bytes::from_static(b"h1"),
        };
        assert_eq!(
            HasBlock::from_proto(has_block.clone().to_proto()),
            has_block
        );

        let has_block_request = HasBlockRequest {
            hash: Bytes::from_static(b"h2"),
        };
        assert_eq!(
            HasBlockRequest::from_proto(has_block_request.clone().to_proto()),
            has_block_request
        );

        let block_request = BlockRequest {
            hash: Bytes::from_static(b"h3"),
        };
        assert_eq!(
            BlockRequest::from_proto(block_request.clone().to_proto()),
            block_request
        );

        let mergeable_request = MergeableEntryRequest {
            block_hash: Bytes::from_static(b"h4"),
        };
        assert_eq!(
            MergeableEntryRequest::from_proto(mergeable_request.clone().to_proto()),
            mergeable_request
        );

        let mergeable_response = MergeableEntryResponse {
            block_hash: Bytes::from_static(b"h5"),
            serialized_entry: Bytes::from_static(b"entry"),
        };
        assert_eq!(
            MergeableEntryResponse::from_proto(mergeable_response.clone().to_proto()),
            mergeable_response
        );
    }

    #[test]
    fn identifier_messages_round_trip() {
        let block_hash_message = BlockHashMessage {
            block_hash: Bytes::from_static(b"hash"),
            block_creator: Bytes::from_static(b"creator"),
        };
        assert_eq!(
            BlockHashMessage::from_proto(block_hash_message.clone().to_proto()),
            block_hash_message
        );

        let no_approved = NoApprovedBlockAvailable {
            identifier: "id".to_string(),
            node_identifier: "node".to_string(),
        };
        assert_eq!(
            NoApprovedBlockAvailable::from_proto(no_approved.clone().to_proto()),
            no_approved
        );

        let approved_request = ApprovedBlockRequest {
            identifier: "id".to_string(),
            trim_state: true,
        };
        assert_eq!(
            ApprovedBlockRequest::from_proto(approved_request.clone().to_proto()),
            approved_request
        );

        assert_eq!(
            ForkChoiceTipRequest.to_proto(),
            ForkChoiceTipRequestProto {}
        );
    }

    #[test]
    fn block_message_round_trips_through_proto() {
        let block = get_random_block_default();
        let round_tripped = BlockMessage::from_proto(block.to_proto()).unwrap();
        assert_eq!(round_tripped, block);
    }

    #[test]
    fn block_message_from_proto_requires_header_and_body() {
        let block = get_random_block_default();

        let mut missing_header = block.to_proto();
        missing_header.header = None;
        assert_eq!(
            BlockMessage::from_proto(missing_header),
            Err("Missing header field".to_string())
        );

        let mut missing_body = block.to_proto();
        missing_body.body = None;
        assert_eq!(
            BlockMessage::from_proto(missing_body),
            Err("Missing body field".to_string())
        );
    }

    #[test]
    fn block_message_to_string_pretty_prints() {
        let block = get_random_block_default();
        let rendered = block.clone().to_string();
        assert!(rendered.contains(&format!("#{}", block.body.state.block_number)));
    }

    #[test]
    fn unapproved_block_and_block_approval_round_trip() {
        let block = get_random_block_default();
        let candidate = ApprovedBlockCandidate {
            block,
            required_sigs: 3,
        };

        let unapproved = UnapprovedBlock {
            candidate: candidate.clone(),
            timestamp: 11,
            duration: 22,
        };
        assert_eq!(
            UnapprovedBlock::from_proto(unapproved.clone().to_proto()).unwrap(),
            unapproved
        );

        let approval = BlockApproval {
            candidate: candidate.clone(),
            sig: Signature {
                public_key: Bytes::from_static(b"pk"),
                algorithm: "secp256k1".to_string(),
                sig: Bytes::from_static(b"sig"),
            },
        };
        assert_eq!(
            BlockApproval::from_proto(approval.clone().to_proto()).unwrap(),
            approval
        );

        assert_eq!(
            BlockApproval::from_proto(BlockApprovalProto {
                candidate: None,
                sig: None,
            }),
            Err("Missing candidate field".to_string())
        );
        assert_eq!(
            UnapprovedBlock::from_proto(UnapprovedBlockProto {
                candidate: None,
                timestamp: 0,
                duration: 0,
            }),
            Err("Missing candidate field".to_string())
        );
    }

    #[test]
    fn events_round_trip_through_proto() {
        let produce = Event::Produce(produce_event());
        assert_eq!(Event::from_proto(produce.to_proto()).unwrap(), produce);

        let consume = Event::Consume(consume_event());
        assert_eq!(Event::from_proto(consume.to_proto()).unwrap(), consume);

        let comm = Event::Comm(CommEvent {
            consume: consume_event(),
            produces: vec![produce_event()],
            peeks: vec![Peek { channel_index: 4 }],
        });
        assert_eq!(Event::from_proto(comm.to_proto()).unwrap(), comm);
    }

    #[test]
    fn empty_event_proto_is_rejected() {
        assert_eq!(
            Event::from_proto(EventProto {
                event_instance: None,
            }),
            Err("Received malformed Event: None".to_string())
        );
    }

    #[test]
    fn processed_deploy_round_trips_and_derives_values() {
        let deploy = signed_deploy();
        let mut processed = ProcessedDeploy::empty(deploy.clone());
        processed.cost = PCost { cost: 100 };
        processed.deploy_log = vec![Event::Produce(produce_event())];
        processed.is_failed = true;
        processed.system_deploy_error = Some("boom".to_string());
        assert_eq!(
            ProcessedDeploy::from_proto(processed.clone().to_proto()).unwrap(),
            processed
        );

        let empty = ProcessedDeploy::empty(deploy.clone());
        assert_eq!(empty.cost, PCost { cost: 0 });
        assert!(!empty.is_failed);
        assert_eq!(empty.system_deploy_error, None);
        assert_eq!(
            ProcessedDeploy::from_proto(empty.clone().to_proto()).unwrap(),
            empty
        );

        let info = processed.clone().to_deploy_info();
        assert_eq!(info.term, deploy.data.term);
        assert_eq!(info.cost, 100);
        assert!(info.errored);
        assert_eq!(info.system_deploy_error, "boom".to_string());
        assert_eq!(info.sig, hex::encode(&deploy.sig));
    }

    #[test]
    fn system_deploy_data_round_trips_all_variants() {
        let slash = SystemDeployData::create_slash(
            Bytes::from_static(b"invalid-block"),
            PublicKey::from_bytes(b"issuer"),
            5,
            BondGeneration::new(2).unwrap(),
        );
        assert_eq!(
            SystemDeployData::from_proto(SystemDeployData::to_proto(slash.clone())).unwrap(),
            slash
        );

        let close = SystemDeployData::create_close();
        assert_eq!(close, SystemDeployData::CloseBlockSystemDeployData);
        assert_eq!(
            SystemDeployData::from_proto(SystemDeployData::to_proto(close.clone())).unwrap(),
            close
        );

        assert_eq!(
            SystemDeployData::from_proto(SystemDeployData::to_proto(SystemDeployData::Empty)),
            Err("Missing system deploy field".to_string())
        );
    }

    #[test]
    fn processed_system_deploy_round_trips_and_folds() {
        let succeeded = ProcessedSystemDeploy::Succeeded {
            event_list: vec![Event::Consume(consume_event())],
            system_deploy: SystemDeployData::CloseBlockSystemDeployData,
            pre_state_hash: Bytes::from_static(b"before"),
            post_state_hash: Bytes::from_static(b"after"),
        };
        assert_eq!(
            ProcessedSystemDeploy::from_proto(succeeded.clone().to_proto()).unwrap(),
            succeeded
        );
        assert!(!succeeded.clone().failed());
        assert_eq!(succeeded.fold(|events| events.len(), |_, _| 999), 1);

        let failed = ProcessedSystemDeploy::Failed {
            event_list: vec![],
            error_msg: "went wrong".to_string(),
            pre_state_hash: Bytes::from_static(b"before"),
            post_state_hash: Bytes::from_static(b"after"),
        };
        assert_eq!(
            ProcessedSystemDeploy::from_proto(failed.clone().to_proto()).unwrap(),
            failed
        );
        assert!(failed.clone().failed());
        assert_eq!(
            failed.fold(|_| "ok".to_string(), |_, msg| msg),
            "went wrong".to_string()
        );
    }

    #[test]
    fn deploy_data_legacy_encoding_round_trips_compatibility_projection() {
        let with_expiration = DeployData {
            term: "Nil".to_string(),
            language: "rholang".to_string(),
            time_stamp: 1,
            valid_after_block_number: 4,
            shard_id: "root".to_string(),
            expiration_timestamp: Some(500),
            authority_presentations: Vec::new(),
        };
        let mut expected_with_expiration = with_expiration.clone();
        expected_with_expiration.language.clear();
        assert_eq!(
            DeployData::decode(DeployData::encode(with_expiration.clone())).unwrap(),
            expected_with_expiration
        );

        let without_expiration = DeployData {
            expiration_timestamp: None,
            ..with_expiration.clone()
        };
        let mut expected_without_expiration = without_expiration.clone();
        expected_without_expiration.language.clear();
        assert_eq!(
            DeployData::decode(DeployData::encode(without_expiration.clone())).unwrap(),
            expected_without_expiration
        );

        assert!(DeployData::decode(vec![0xff, 0xff, 0xff]).is_err());
    }

    #[test]
    fn deploy_data_expiration_helpers() {
        let mut deploy = DeployData {
            term: "Nil".to_string(),
            language: "rholang".to_string(),
            time_stamp: 1,
            valid_after_block_number: 0,
            shard_id: "root".to_string(),
            expiration_timestamp: None,
            authority_presentations: Vec::new(),
        };
        assert!(!deploy.has_expiration());
        assert!(!deploy.is_expired_at(i64::MAX));

        deploy.expiration_timestamp = Some(100);
        assert!(deploy.has_expiration());
        assert!(!deploy.is_expired_at(100));
        assert!(deploy.is_expired_at(101));
    }

    #[test]
    fn signed_deploy_data_survives_proto_round_trip() {
        let signed = signed_deploy();
        let round_tripped = DeployData::from_proto(DeployData::to_proto_ref(&signed)).unwrap();
        assert_eq!(round_tripped.data, signed.data);
        assert_eq!(round_tripped.sig, signed.sig);
        assert_eq!(round_tripped.pk, signed.pk);
    }

    #[test]
    fn deploy_data_from_proto_rejects_unknown_algorithm_and_bad_signature() {
        let signed = signed_deploy();

        let mut unknown_alg = DeployData::to_proto_ref(&signed);
        unknown_alg.sig_algorithm = "no-such-alg".to_string();
        assert!(DeployData::from_proto(unknown_alg)
            .unwrap_err()
            .contains("Unknown signature algorithm"));

        let mut tampered = DeployData::to_proto_ref(&signed);
        tampered.term = format!("{} ", tampered.term);
        assert!(DeployData::from_proto(tampered).is_err());
    }

    #[test]
    fn store_node_key_round_trips_with_and_without_index() {
        let with_index = (hash32(1), Some(7u8));
        assert_eq!(
            StoreNodeKey::from_proto(StoreNodeKey::to_proto(&with_index)),
            with_index
        );

        let without_index = (hash32(2), None);
        assert_eq!(
            StoreNodeKey::from_proto(StoreNodeKey::to_proto(&without_index)),
            without_index
        );
    }

    #[test]
    fn store_items_messages_round_trip() {
        let request = StoreItemsMessageRequest {
            start_path: vec![(hash32(1), Some(0)), (hash32(2), None)],
            skip: 5,
            take: 10,
        };
        assert_eq!(
            StoreItemsMessageRequest::from_proto(request.clone().to_proto()),
            request
        );

        let message = StoreItemsMessage {
            start_path: vec![(hash32(1), None)],
            last_path: vec![(hash32(2), Some(3))],
            history_items: vec![(hash32(3), Bytes::from_static(b"history"))],
            data_items: vec![(hash32(4), Bytes::from_static(b"data"))],
        };
        assert_eq!(
            StoreItemsMessage::from_proto(message.clone().to_proto()),
            message
        );

        let pretty = message.pretty();
        assert!(pretty.starts_with("StoreItemsMessage(history: 1, data: 1"));
    }

    #[test]
    fn casper_message_wrappers_tag_the_right_variant() {
        let hash = Bytes::from_static(b"h");
        assert_eq!(
            CasperMessage::from_has_block(HasBlockProto { hash: hash.clone() }),
            CasperMessage::HasBlock(HasBlock { hash: hash.clone() })
        );
        assert_eq!(
            CasperMessage::from_has_block_request(HasBlockRequestProto { hash: hash.clone() }),
            CasperMessage::HasBlockRequest(HasBlockRequest { hash: hash.clone() })
        );
        assert_eq!(
            CasperMessage::from_block_request(BlockRequestProto { hash: hash.clone() }),
            CasperMessage::BlockRequest(BlockRequest { hash: hash.clone() })
        );
        assert_eq!(
            CasperMessage::from_fork_choice_tip_request(ForkChoiceTipRequestProto {}),
            CasperMessage::ForkChoiceTipRequest(ForkChoiceTipRequest)
        );

        let block = get_random_block_default();
        assert_eq!(
            CasperMessage::from_block_message(block.to_proto()).unwrap(),
            CasperMessage::BlockMessage(block)
        );

        let request = FloorCacheRequest {
            hashes: vec![hash.clone()],
        };
        assert_eq!(
            CasperMessage::from_floor_cache_request(request.clone().to_proto()),
            CasperMessage::FloorCacheRequest(request)
        );
    }
}
