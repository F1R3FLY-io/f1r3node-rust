// See models/src/main/scala/coop/rchain/casper/protocol/CasperMessage.scala

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
use crate::rust::block_hash::BlockHash;
use crate::rust::casper::pretty_printer::PrettyPrinter;

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

#[derive(Debug, Clone, PartialEq)]
pub struct ApprovedBlock {
    pub candidate: ApprovedBlockCandidate,
    pub sigs: Vec<Signature>,
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
        })
    }

    pub fn to_proto(self) -> ApprovedBlockProto {
        ApprovedBlockProto {
            candidate: Some(self.candidate.to_proto()),
            sigs: self.sigs,
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
}

impl BlockMessage {
    pub fn from_proto(proto: BlockMessageProto) -> Result<Self, String> {
        Ok(Self {
            block_hash: proto.block_hash,
            header: Header::from_proto(
                proto
                    .header
                    .ok_or_else(|| "Missing header field".to_string())?,
            ),
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
}

impl Header {
    pub fn from_proto(proto: HeaderProto) -> Self {
        Self {
            parents_hash_list: proto.parents_hash_list,
            timestamp: proto.timestamp,
            version: proto.version,
            extra_bytes: proto.extra_bytes,
        }
    }

    pub fn to_proto(&self) -> HeaderProto {
        HeaderProto {
            parents_hash_list: self.parents_hash_list.clone(),
            timestamp: self.timestamp,
            version: self.version,
            extra_bytes: self.extra_bytes.clone(),
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
}

impl RejectedDeployReason {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unspecified => "unspecified",
            Self::MergeConflict => "merge_conflict",
            Self::DuplicateOccurrence => "duplicate_occurrence",
            Self::CollateralChainDrop => "collateral_chain_drop",
        }
    }

    pub fn canonical_join(self, other: Self) -> Self {
        use RejectedDeployReason::{
            CollateralChainDrop, DuplicateOccurrence, MergeConflict, Unspecified,
        };

        match (self, other) {
            (DuplicateOccurrence, _) | (_, DuplicateOccurrence) => DuplicateOccurrence,
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
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RejectedDeploy {
    pub sig: ByteString,
    pub source_block_hash: BlockHash,
    pub reason: RejectedDeployReason,
}

impl RejectedDeploy {
    pub fn legacy(sig: ByteString) -> Self {
        Self {
            sig,
            source_block_hash: ByteString::new(),
            reason: RejectedDeployReason::Unspecified,
        }
    }

    pub fn occurrence(
        sig: ByteString,
        source_block_hash: BlockHash,
        reason: RejectedDeployReason,
    ) -> Self {
        Self {
            sig,
            source_block_hash,
            reason,
        }
    }

    pub fn has_provenance(&self) -> bool { !self.source_block_hash.is_empty() }

    pub fn from_proto(proto: RejectedDeployProto) -> Self {
        Self {
            sig: proto.sig,
            source_block_hash: proto.source_block_hash,
            reason: RejectedDeployReason::from_proto(proto.reason),
        }
    }

    pub fn to_proto(self) -> RejectedDeployProto {
        RejectedDeployProto {
            sig: self.sig,
            source_block_hash: self.source_block_hash,
            reason: self.reason.to_proto(),
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
}

#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    pub state: F1r3flyState,
    pub deploys: Vec<ProcessedDeploy>,
    pub rejected_deploys: Vec<RejectedDeploy>,
    pub rejected_state_effects: Vec<StateEffectId>,
    pub system_deploys: Vec<ProcessedSystemDeploy>,
    pub extra_bytes: ByteString,
}

impl Body {
    pub fn from_proto(proto: BodyProto) -> Result<Self, String> {
        Ok(Self {
            state: F1r3flyState::from_proto(
                proto
                    .state
                    .ok_or_else(|| "Missing state field".to_string())?,
            ),
            deploys: proto
                .deploys
                .into_iter()
                .map(|d| ProcessedDeploy::from_proto(d))
                .collect::<Result<Vec<ProcessedDeploy>, String>>()?,
            rejected_deploys: proto
                .rejected_deploys
                .into_iter()
                .map(|r| RejectedDeploy::from_proto(r))
                .collect(),
            rejected_state_effects: proto
                .rejected_state_effects
                .into_iter()
                .map(StateEffectId::from_proto)
                .collect(),
            system_deploys: proto
                .system_deploys
                .into_iter()
                .map(|s| ProcessedSystemDeploy::from_proto(s))
                .collect::<Result<Vec<ProcessedSystemDeploy>, String>>()?,
            extra_bytes: proto.extra_bytes,
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
            system_deploys: self
                .system_deploys
                .clone()
                .into_iter()
                .map(|s| s.to_proto())
                .collect(),
            extra_bytes: self.extra_bytes.clone(),
        }
    }
}

#[derive(
    Debug,
    Clone,
    Eq,
    PartialEq,
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
    pub block_number: i64,
}

impl F1r3flyState {
    pub fn from_proto(proto: RChainStateProto) -> Self {
        Self {
            pre_state_hash: proto.pre_state_hash,
            post_state_hash: proto.post_state_hash,
            bonds: proto
                .bonds
                .into_iter()
                .map(|b| Bond::from_proto(b))
                .collect(),
            block_number: proto.block_number,
        }
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
            block_number: self.block_number,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProcessedDeploy {
    pub deploy: Signed<DeployData>,
    pub cost: PCost,
    pub deploy_log: Vec<Event>,
    pub is_failed: bool,
    pub system_deploy_error: Option<String>,
    /// Additional cosigners beyond the primary (`deploy.pk` / `deploy.sig`).
    /// Empty for legacy single-signature deploys. Round-trips through
    /// `DeployDataProto.cosigners` (proto field 14 on `deploy`).
    pub cosigners: Vec<crate::casper::CompoundSigner>,
    /// M-of-N quorum threshold (Phase 2). 0 = N-of-N semantics (every
    /// signer's signature must verify); k > 0 = at least k signatures
    /// must verify. Round-trips through `DeployDataProto.cosigner_threshold`
    /// (proto field 16).
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

    /// Construct an empty processed-deploy stub from a `Cosigned<DeployData>`
    /// envelope, preserving the full cosigner list. Used by error-envelope
    /// construction paths in the multi-sig runtime fan-out where a deploy
    /// fails BEFORE evaluation begins.
    pub fn empty_from_cosigned(
        cosigned: &crypto::rust::signatures::signed::Cosigned<DeployData>,
    ) -> Self {
        let primary = cosigned.primary();
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
                .skip(1)
                .map(|c| crate::casper::CompoundSigner {
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
            cost: PCost { cost: 0 },
            deploy_log: Vec::new(),
            is_failed: false,
            system_deploy_error: None,
            cosigners,
            // empty_from_cosigned has no view of the runtime threshold —
            // callers needing M-of-N must set the field after construction.
            cosigner_threshold: 0,
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

    /// Reconstitute the [`Cosigned<DeployData>`] envelope from on-disk
    /// `ProcessedDeploy` shape. For legacy deploys (`cosigners.is_empty()`),
    /// uplifts via `Cosigned::from_single_signer` for byte-identical replay
    /// behavior. For multi-sig deploys, rebuilds the full canonical envelope
    /// with per-signer re-verification.
    pub fn to_cosigned(
        &self,
    ) -> Result<crypto::rust::signatures::signed::Cosigned<DeployData>, String> {
        use crypto::rust::signatures::signed::{Cosigned, Cosigner};

        if self.cosigners.is_empty() {
            // Legacy single-sig path: byte-identical to single-sig replay.
            Cosigned::from_single_signer(self.deploy.clone())
                .map_err(|e| format!("legacy uplift to Cosigned failed: {}", e))
        } else {
            // Multi-sig: rebuild signer list with full re-verification.
            let primary = Cosigner {
                pk: self.deploy.pk.clone(),
                sig: self.deploy.sig.clone(),
                sig_algorithm: self.deploy.sig_algorithm.clone(),
            };
            let mut signers = Vec::with_capacity(1 + self.cosigners.len());
            signers.push(primary);
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
            // Phase 2 dispatch on threshold; preserves replay determinism
            // because the threshold is a wire-level constant captured at
            // proposal time.
            if self.cosigner_threshold > 0 {
                Cosigned::from_signed_data_threshold(
                    self.deploy.data.clone(),
                    signers,
                    self.cosigner_threshold as u32,
                )
                .map_err(|e| {
                    format!(
                        "ProcessedDeploy to_cosigned threshold reconstruction failed (threshold={}): {}",
                        self.cosigner_threshold, e
                    )
                })
            } else {
                Cosigned::from_signed_data(self.deploy.data.clone(), signers).map_err(|e| {
                    format!("ProcessedDeploy to_cosigned reconstruction failed: {}", e)
                })
            }
        }
    }

    pub fn to_deploy_info(self) -> DeployInfo {
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
        let cosigners = deploy_proto.cosigners.clone();
        let cosigner_threshold = deploy_proto.cosigner_threshold;
        Ok(Self {
            deploy: DeployData::from_proto(deploy_proto)?,
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
        })
    }

    pub fn to_proto(self) -> ProcessedDeployProto {
        let mut deploy_proto = DeployData::to_proto(self.deploy);
        // Re-attach the cosigner metadata that lives at the
        // ProcessedDeploy level into the inner DeployDataProto so the
        // wire shape carries it through block-storage round-trip.
        deploy_proto.cosigners = self.cosigners;
        deploy_proto.cosigner_threshold = self.cosigner_threshold;
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
        issuer_public_key: PublicKey,
        target_activation_epoch: i64,
    },
    CloseBlockSystemDeployData,
    /// Cost-Accounted Rho Stage-C validator redemption (DR-7/DR-12). Carries the
    /// FULL redemption-authorization material so replay can re-run the DR-12
    /// PoS-multisig-quorum platform obligation byte-identically to play.
    Redeem {
        validator_pk: ByteString,
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
    ) -> Self {
        Self::Slash {
            invalid_block_hash,
            issuer_public_key,
            target_activation_epoch,
        }
    }

    pub fn create_close() -> Self { Self::CloseBlockSystemDeployData }

    pub fn create_redeem(
        validator_pk: ByteString,
        outcome_tag: String,
        penalty: i64,
        pos_multi_sig_public_keys: Vec<String>,
        pos_multi_sig_quorum: u32,
        authorizations: Vec<RedemptionAuthorizationData>,
    ) -> Self {
        Self::Redeem {
            validator_pk,
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
            ) => Ok(Self::Slash {
                invalid_block_hash: slash_system_deploy_data_proto.invalid_block_hash,
                issuer_public_key: PublicKey::from_bytes(
                    &slash_system_deploy_data_proto.issuer_public_key,
                ),
                target_activation_epoch: slash_system_deploy_data_proto.target_activation_epoch,
            }),
            system_deploy_data_proto::SystemDeploy::CloseBlockSystemDeploy(_) => {
                Ok(Self::CloseBlockSystemDeployData)
            }
            system_deploy_data_proto::SystemDeploy::RedeemSystemDeploy(redeem) => {
                Ok(Self::Redeem {
                    validator_pk: redeem.validator_pk,
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
                issuer_public_key,
                target_activation_epoch,
            } => SystemDeployDataProto {
                system_deploy: Some(SystemDeploy::SlashSystemDeploy(
                    SlashSystemDeployDataProto {
                        invalid_block_hash,
                        issuer_public_key: issuer_public_key.bytes.into(),
                        target_activation_epoch,
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
            let encoded = canonical.encode_to_vec();
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
    pub fn from_proto_cosigned(
        proto: DeployDataProto,
    ) -> Result<crypto::rust::signatures::signed::Cosigned<DeployData>, String> {
        use crypto::rust::signatures::signed::{Cosigned, Cosigner};

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
        let primary = cosigned.primary();
        let is_compound = cosigned.is_compound();
        let cosigners_proto: Vec<crate::casper::CompoundSigner> = if is_compound {
            cosigned
                .signers()
                .iter()
                .skip(1) // primary occupies fields 1/4/5; cosigners[] is the rest
                .map(|c| crate::casper::CompoundSigner {
                    pk: c.pk.bytes.clone().into(),
                    sig: c.sig.clone(),
                    sig_algorithm: c.sig_algorithm.name(),
                })
                .collect()
        } else {
            Vec::new()
        };
        DeployDataProto {
            term: cosigned.data.term.clone(),
            timestamp: cosigned.data.time_stamp,
            valid_after_block_number: cosigned.data.valid_after_block_number,
            shard_id: cosigned.data.shard_id.clone(),
            deployer: primary.pk.bytes.clone().into(),
            sig: primary.sig.clone(),
            sig_algorithm: primary.sig_algorithm.name(),
            expiration_timestamp: cosigned.data.expiration_timestamp.unwrap_or(0),
            authority_presentations: cosigned.data.authority_presentations.clone(),
            cosigners: cosigners_proto,
            // Single-signer / N-of-N round-trip emits 0 (legacy semantics).
            // M-of-N round-trip requires the caller to set this directly on
            // the proto AFTER calling this routine; the Cosigned envelope
            // does not carry the threshold value through the data path.
            cosigner_threshold: 0,
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
    /// Bincode of `Vec<DeployMergeableData>`. Empty bytes = peer has the block
    /// but no entry for it.
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
// kani retarget — see `docs/theory/cost-accounting-impl/d3-replace-phlo-with-tokens.md`
// §Sequencing, Commit 2).

#[cfg(test)]
mod tests {
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::secp256k1_eth::Secp256k1Eth;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;
    use crypto::rust::signatures::signed::Signed;
    use proptest::prelude::*;
    use prost::bytes::Bytes;

    use super::*;

    fn deploy_data() -> DeployData {
        DeployData {
            term: "Nil".to_string(),
            time_stamp: 0,
            valid_after_block_number: 0,
            shard_id: "root".to_string(),
            expiration_timestamp: None,
            authority_presentations: Vec::new(),
        }
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
        let rejected = RejectedDeploy::occurrence(
            Bytes::from_static(b"deploy"),
            Bytes::from_static(b"source"),
            RejectedDeployReason::DuplicateOccurrence,
        );

        assert_eq!(
            RejectedDeploy::from_proto(rejected.clone().to_proto()),
            rejected
        );
    }

    #[test]
    fn rejected_state_effects_round_trip_through_body_proto_without_reordering() {
        let effects = vec![
            StateEffectId {
                source_block_hash: Bytes::from_static(b"source-a"),
                execution_index: 2,
            },
            StateEffectId {
                source_block_hash: Bytes::from_static(b"source-b"),
                execution_index: 1,
            },
        ];
        let body = Body {
            state: F1r3flyState {
                pre_state_hash: Bytes::from_static(b"pre"),
                post_state_hash: Bytes::from_static(b"post"),
                bonds: Vec::new(),
                block_number: 7,
            },
            deploys: Vec::new(),
            rejected_deploys: Vec::new(),
            rejected_state_effects: effects.clone(),
            system_deploys: Vec::new(),
            extra_bytes: Bytes::new(),
        };

        let decoded = Body::from_proto(body.to_proto()).unwrap();
        assert_eq!(decoded, body);
        assert_eq!(decoded.rejected_state_effects, effects);
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
            source_block_hash: Bytes::new(),
            reason: 0,
        };

        assert_eq!(
            RejectedDeploy::from_proto(proto),
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
    }

    // =================================================================
    // F-A funding/capability separation — INGRESS REJECT (c) tests.
    //
    // `docs/theory/cost-accounting-impl/f-a-funding-vs-capability-separation.md`
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
        };
        let cosigned = DeployData::from_proto_cosigned(proto)
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

        let cosigned = DeployData::from_proto_cosigned(proto)
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

        let cosigned = DeployData::from_proto_cosigned(base.clone())
            .expect("one valid signer must satisfy a one-of-two threshold");
        assert_eq!(cosigned.cosigner_threshold(), 1);

        let mut negative = base.clone();
        negative.cosigner_threshold = -1;
        assert!(DeployData::from_proto_cosigned(negative)
            .unwrap_err()
            .contains("Invalid cosigner_threshold"));

        let mut excessive = base;
        excessive.cosigner_threshold = 3;
        assert!(DeployData::from_proto_cosigned(excessive)
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
}
