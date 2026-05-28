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
    pub fn from_proto(proto: HasBlockRequestProto) -> Self {
        Self { hash: proto.hash }
    }

    pub fn to_proto(self) -> HasBlockRequestProto {
        HasBlockRequestProto { hash: self.hash }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct HasBlock {
    pub hash: ByteString,
}

impl HasBlock {
    pub fn from_proto(proto: HasBlockProto) -> Self {
        Self { hash: proto.hash }
    }

    pub fn to_proto(self) -> HasBlockProto {
        HasBlockProto { hash: self.hash }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BlockRequest {
    pub hash: ByteString,
}

impl BlockRequest {
    pub fn from_proto(proto: BlockRequestProto) -> Self {
        Self { hash: proto.hash }
    }

    pub fn to_proto(self) -> BlockRequestProto {
        BlockRequestProto { hash: self.hash }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ForkChoiceTipRequest;

impl ForkChoiceTipRequest {
    pub fn to_proto(self) -> ForkChoiceTipRequestProto {
        ForkChoiceTipRequestProto {}
    }
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

#[derive(Debug, Clone, PartialEq)]
pub struct RejectedDeploy {
    pub sig: ByteString,
}

impl RejectedDeploy {
    pub fn from_proto(proto: RejectedDeployProto) -> Self {
        Self { sig: proto.sig }
    }

    pub fn to_proto(self) -> RejectedDeployProto {
        RejectedDeployProto { sig: self.sig }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    pub state: F1r3flyState,
    pub deploys: Vec<ProcessedDeploy>,
    pub rejected_deploys: Vec<RejectedDeploy>,
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
    /// Primary signer's phlo share. Zero for legacy single-signature deploys
    /// (in which case the primary covers the entire `phlo_limit`); explicit
    /// value when `cosigners` is non-empty. Round-trips through
    /// `DeployDataProto.primary_phlo_share` (proto field 15).
    pub primary_phlo_share: i64,
    /// M-of-N quorum threshold (Phase 2). 0 = N-of-N semantics (every
    /// signer's signature must verify); k > 0 = at least k signatures
    /// must verify. Round-trips through `DeployDataProto.cosigner_threshold`
    /// (proto field 16).
    pub cosigner_threshold: i32,
}

impl ProcessedDeploy {
    pub fn try_refund_amount(&self) -> Result<i64, String> {
        let token_cost = i64::try_from(self.cost.cost).map_err(|_| {
            format!(
                "Token cost {} exceeds the supported i64 settlement range.",
                self.cost.cost
            )
        })?;
        self.deploy.data.refund_amount_for_token_cost(token_cost)
    }

    pub fn refund_amount(&self) -> i64 {
        self.try_refund_amount()
            .expect("deploy phlo terms must be validated before refund settlement")
    }

    pub fn empty(deploy: Signed<DeployData>) -> Self {
        Self {
            deploy,
            cost: PCost { cost: 0 },
            deploy_log: Vec::new(),
            is_failed: false,
            system_deploy_error: None,
            cosigners: Vec::new(),
            primary_phlo_share: 0,
            cosigner_threshold: 0,
        }
    }

    /// Construct an empty processed-deploy stub from a `Cosigned<DeployData>`
    /// envelope, preserving the full cosigner list and primary phlo share.
    /// Used by error-envelope construction paths in the multi-sig runtime
    /// fan-out where a deploy fails BEFORE evaluation begins.
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
        let (cosigners, primary_phlo_share) = if is_compound {
            (
                cosigned
                    .signers()
                    .iter()
                    .skip(1)
                    .map(|c| crate::casper::CompoundSigner {
                        pk: c.pk.bytes.clone().into(),
                        sig: c.sig.clone(),
                        sig_algorithm: c.sig_algorithm.name(),
                        phlo_share: c.phlo_share,
                    })
                    .collect(),
                primary.phlo_share,
            )
        } else {
            (Vec::new(), 0_i64)
        };
        Self {
            deploy,
            cost: PCost { cost: 0 },
            deploy_log: Vec::new(),
            is_failed: false,
            system_deploy_error: None,
            cosigners,
            primary_phlo_share,
            // empty_from_cosigned has no view of the runtime threshold —
            // callers needing M-of-N must set the field after construction.
            cosigner_threshold: 0,
        }
    }

    /// Reconstitute the [`Cosigned<DeployData>`] envelope from on-disk
    /// `ProcessedDeploy` shape. For legacy deploys (`cosigners.is_empty()`
    /// AND `primary_phlo_share == 0`), uplifts via
    /// `Cosigned::from_single_signer` for byte-identical replay behavior.
    /// For multi-sig deploys, rebuilds the full canonical envelope with
    /// per-signer re-verification.
    pub fn to_cosigned(
        &self,
    ) -> Result<crypto::rust::signatures::signed::Cosigned<DeployData>, String> {
        use crypto::rust::signatures::signed::{Cosigned, Cosigner};

        if self.cosigners.is_empty() && self.primary_phlo_share == 0 {
            // Legacy single-sig path: byte-identical to single-sig replay.
            Cosigned::from_single_signer(self.deploy.clone(), self.deploy.data.phlo_limit)
                .map_err(|e| format!("legacy uplift to Cosigned failed: {}", e))
        } else {
            // Multi-sig: rebuild signer list with full re-verification.
            let primary = Cosigner {
                pk: self.deploy.pk.clone(),
                sig: self.deploy.sig.clone(),
                sig_algorithm: self.deploy.sig_algorithm.clone(),
                phlo_share: self.primary_phlo_share,
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
                    phlo_share: cs.phlo_share,
                });
            }
            // Phase 2 dispatch on threshold; preserves replay determinism
            // because the threshold is a wire-level constant captured at
            // proposal time.
            if self.cosigner_threshold > 0 {
                Cosigned::from_signed_data_threshold(
                    self.deploy.data.clone(),
                    signers,
                    self.deploy.data.phlo_limit,
                    self.cosigner_threshold as u32,
                )
                .map_err(|e| {
                    format!(
                        "ProcessedDeploy to_cosigned threshold reconstruction failed (threshold={}): {}",
                        self.cosigner_threshold, e
                    )
                })
            } else {
                Cosigned::from_signed_data(
                    self.deploy.data.clone(),
                    signers,
                    self.deploy.data.phlo_limit,
                )
                .map_err(|e| format!("ProcessedDeploy to_cosigned reconstruction failed: {}", e))
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
            phlo_price: self.deploy.data.phlo_price,
            phlo_limit: self.deploy.data.phlo_limit,
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
        // only the primary signer; the cosigners[] + primary_phlo_share
        // populate the ProcessedDeploy fields directly so the multi-sig
        // shape survives serialization.
        let cosigners = deploy_proto.cosigners.clone();
        let primary_phlo_share = deploy_proto.primary_phlo_share;
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
            primary_phlo_share,
            cosigner_threshold,
        })
    }

    pub fn to_proto(self) -> ProcessedDeployProto {
        let mut deploy_proto = DeployData::to_proto(self.deploy);
        // Re-attach the cosigner metadata that lives at the
        // ProcessedDeploy level into the inner DeployDataProto so the
        // wire shape carries it through block-storage round-trip.
        deploy_proto.cosigners = self.cosigners;
        deploy_proto.primary_phlo_share = self.primary_phlo_share;
        deploy_proto.cosigner_threshold = self.cosigner_threshold;
        ProcessedDeployProto {
            deploy: Some(deploy_proto),
            cost: Some(self.cost),
            deploy_log: self.deploy_log.into_iter().map(|e| e.to_proto()).collect(),
            errored: self.is_failed,
            system_deploy_error: self.system_deploy_error.unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SystemDeployData {
    Slash {
        invalid_block_hash: ByteString,
        issuer_public_key: PublicKey,
        target_activation_epoch: i64,
    },
    CloseBlockSystemDeployData,
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

    pub fn create_close() -> Self {
        Self::CloseBlockSystemDeployData
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
    },
    Failed {
        event_list: Vec<Event>,
        error_msg: String,
    },
}

impl ProcessedSystemDeploy {
    pub fn failed(self) -> bool {
        matches!(self, ProcessedSystemDeploy::Failed { .. })
    }

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
            } => if_failed(event_list, error_msg),
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
                    })
                } else {
                    Ok(ProcessedSystemDeploy::Failed {
                        event_list: deploy_log,
                        error_msg: psd.error_msg,
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
            } => ProcessedSystemDeployProto {
                system_deploy: Some(SystemDeployData::to_proto(system_deploy)),
                deploy_log: event_list
                    .into_iter()
                    .map(|arg0: Event| Event::to_proto(&arg0))
                    .collect(),
                error_msg: "".to_string(),
            },
            ProcessedSystemDeploy::Failed {
                event_list,
                error_msg,
            } => ProcessedSystemDeployProto {
                system_deploy: None,
                deploy_log: event_list
                    .into_iter()
                    .map(|arg0: Event| Event::to_proto(&arg0))
                    .collect(),
                error_msg,
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
    #[serde(rename = "phloPrice")]
    pub phlo_price: i64,
    #[serde(rename = "phloLimit")]
    pub phlo_limit: i64,
    #[serde(rename = "validAfterBlockNumber")]
    pub valid_after_block_number: i64,
    #[serde(rename = "shardId")]
    pub shard_id: String,
    /// Optional millisecond timestamp after which deploy is invalid (None = no expiration)
    pub expiration_timestamp: Option<i64>,
}

impl ToMessage for DeployData {
    type Type = DeployDataProto;
    fn to_message(&self) -> Self::Type {
        DeployData::_to_proto(self.clone())
    }
}

/// Internal helper for walking a `SigCompound` expression and collecting
/// atomic signer leaves. Used exclusively by
/// [`DeployData::from_proto_cosigned_with_sig_algebra`] and its callees.
#[derive(Clone, Debug)]
struct AlgebraAtom {
    pk: Vec<u8>,
    sig: prost::bytes::Bytes,
    sig_algorithm: String,
    phlo_share: i64,
}

impl AlgebraAtom {
    fn from_proto(atom: &crate::casper::SigAtom) -> Self {
        Self {
            pk: atom.pk.to_vec(),
            sig: atom.sig.clone(),
            sig_algorithm: atom.sig_algorithm.clone(),
            phlo_share: atom.phlo_share,
        }
    }
}

impl DeployData {
    fn checked_total_phlo_charge_value(phlo_limit: i64, phlo_price: i64) -> Option<i64> {
        if phlo_limit < 0 || phlo_price < 0 {
            return None;
        }
        phlo_limit.checked_mul(phlo_price)
    }

    fn refund_amount_for_token_cost_value(
        phlo_limit: i64,
        phlo_price: i64,
        token_cost: i64,
    ) -> Option<i64> {
        if phlo_limit < 0 || phlo_price < 0 || token_cost < 0 {
            return None;
        }
        Self::checked_total_phlo_charge_value(phlo_limit, phlo_price)?;
        let refundable_tokens = phlo_limit.saturating_sub(token_cost).max(0);
        refundable_tokens.checked_mul(phlo_price)
    }

    pub fn validate_phlo(&self, min_phlo_price: i64) -> Result<(), String> {
        if self.phlo_limit < 0 {
            return Err(format!(
                "Phlo limit {} must be non-negative.",
                self.phlo_limit
            ));
        }
        if self.phlo_price < 0 {
            return Err(format!(
                "Phlo price {} must be non-negative.",
                self.phlo_price
            ));
        }
        if self.phlo_price < min_phlo_price {
            return Err(format!(
                "Phlo price {} is less than minimum price {}.",
                self.phlo_price, min_phlo_price
            ));
        }
        self.checked_total_phlo_charge().map(|_| ())
    }

    pub fn checked_total_phlo_charge(&self) -> Result<i64, String> {
        if self.phlo_limit < 0 {
            return Err(format!(
                "Phlo limit {} must be non-negative.",
                self.phlo_limit
            ));
        }
        if self.phlo_price < 0 {
            return Err(format!(
                "Phlo price {} must be non-negative.",
                self.phlo_price
            ));
        }
        Self::checked_total_phlo_charge_value(self.phlo_limit, self.phlo_price).ok_or_else(|| {
            format!(
                "Phlo charge overflows i64: limit={}, price={}",
                self.phlo_limit, self.phlo_price
            )
        })
    }

    pub fn total_phlo_charge(&self) -> i64 {
        self.checked_total_phlo_charge()
            .expect("deploy phlo terms must be validated before settlement")
    }

    pub fn refund_amount_for_token_cost(&self, token_cost: i64) -> Result<i64, String> {
        if token_cost < 0 {
            return Err(format!("Token cost {} must be non-negative.", token_cost));
        }
        self.checked_total_phlo_charge()?;
        let refundable_tokens = self.phlo_limit.saturating_sub(token_cost).max(0);
        Self::refund_amount_for_token_cost_value(self.phlo_limit, self.phlo_price, token_cost)
            .ok_or_else(|| {
                format!(
                    "Deploy refund overflows i64: refundable_tokens={}, price={}",
                    refundable_tokens, self.phlo_price
                )
            })
    }

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

    pub fn encode(a: DeployData) -> ByteVector {
        DeployData::_to_proto(a).encode_to_vec()
    }

    pub fn decode(a: ByteVector) -> Result<DeployData, String> {
        let proto = DeployDataProto::decode(&a[..])
            .map_err(|e| format!("Failed to decode DeployData: {}", e))?;
        Ok(DeployData::_from_proto(proto))
    }

    fn _from_proto(proto: DeployDataProto) -> Self {
        Self {
            term: proto.term,
            time_stamp: proto.timestamp,
            phlo_price: proto.phlo_price,
            phlo_limit: proto.phlo_limit,
            valid_after_block_number: proto.valid_after_block_number,
            shard_id: proto.shard_id,
            // 0 in protobuf means not set, convert to None
            expiration_timestamp: if proto.expiration_timestamp == 0 {
                None
            } else {
                Some(proto.expiration_timestamp)
            },
        }
    }

    /// Primary-signer-only decode. Returns `Signed<DeployData>` constructed
    /// from the primary signer's fields (`deployer`, `sig`, `sig_algorithm`)
    /// regardless of whether the wire deploy carries cosigners. Callers that
    /// need the full multi-signature envelope MUST use
    /// [`Self::from_proto_cosigned`].
    ///
    /// `ProcessedDeploy::from_proto` calls this routine and SEPARATELY
    /// captures `proto.cosigners` and `proto.primary_phlo_share` into the
    /// `ProcessedDeploy.cosigners` and `ProcessedDeploy.primary_phlo_share`
    /// fields, so the cosigner data is preserved across deserialization
    /// even though the inner `Signed<DeployData>` only carries the primary.
    pub fn from_proto(proto: DeployDataProto) -> Result<Signed<DeployData>, String> {
        let algorithm = SignaturesAlgFactory::apply(&proto.sig_algorithm)
            .ok_or_else(|| format!("Unknown signature algorithm: {}", proto.sig_algorithm))?;

        let sig = proto.sig.clone();
        let pk = PublicKey::from_bytes(&proto.deployer);
        let signed = Signed::from_signed_data(DeployData::_from_proto(proto), pk, sig, algorithm)?;

        match signed {
            Some(signed) => Ok(signed),
            None => Err("Invalid signature".to_string()),
        }
    }

    /// Multi-signature aware decode. Returns a [`Cosigned<DeployData>`]
    /// envelope covering both legacy single-sig wire deploys (`cosigners`
    /// empty → one-element envelope with the primary signer's phlo_share
    /// equal to `phlo_limit`) and multi-sig wire deploys (`cosigners`
    /// non-empty → N-element envelope with explicit per-signer shares).
    ///
    /// Invariants enforced by `Cosigned::from_signed_data` at construction:
    /// 1. All signers' signatures verify against the canonical message hash.
    /// 2. Canonical pk-ascending sort; no duplicates.
    /// 3. Σ phlo_share == phlo_limit.
    ///
    /// For multi-sig deploys: the primary signer (fields 1/4/5 of
    /// `DeployDataProto`) contributes `primary_phlo_share` (field 15);
    /// each cosigner in `cosigners[]` (field 14) contributes their own
    /// `phlo_share`. The sum must equal `phlo_limit` (field 8) — enforced
    /// by `Cosigned::from_signed_data`.
    pub fn from_proto_cosigned(
        proto: DeployDataProto,
    ) -> Result<crypto::rust::signatures::signed::Cosigned<DeployData>, String> {
        use crypto::rust::signatures::signed::{Cosigned, Cosigner};

        // Resolve the primary signer's algorithm.
        let primary_alg = SignaturesAlgFactory::apply(&proto.sig_algorithm).ok_or_else(|| {
            format!(
                "Unknown primary signature algorithm: {}",
                proto.sig_algorithm
            )
        })?;

        // Phase 3 dispatch: sig_algebra (LL-rich algebra) OVERRIDES
        // the flat cosigners[] + cosigner_threshold path. Take it out
        // of proto upfront so the later _from_proto consumes the rest.
        let sig_algebra = proto.sig_algebra.clone();

        // Capture phlo_limit before consuming `proto` for _from_proto.
        let phlo_limit = proto.phlo_limit;
        let is_multi_sig = !proto.cosigners.is_empty();
        let cosigner_threshold = proto.cosigner_threshold;
        let total_signers = 1 + proto.cosigners.len() as i32;
        // Validate threshold range at the wire boundary so we can return
        // a clear protocol-level error before signer construction.
        if cosigner_threshold < 0 || cosigner_threshold > total_signers {
            return Err(format!(
                "Invalid cosigner_threshold {}: must satisfy 0 ≤ threshold ≤ {} (total signers)",
                cosigner_threshold, total_signers
            ));
        }
        let primary_phlo_share = if is_multi_sig {
            // Multi-sig: explicit share from wire field 15.
            proto.primary_phlo_share
        } else {
            // Legacy single-sig: primary covers entire phlo_limit.
            phlo_limit
        };

        // Build the canonical signer list. Primary first (will be sorted
        // canonically by Cosigned::from_signed_data).
        let mut signers = Vec::with_capacity(1 + proto.cosigners.len());
        signers.push(Cosigner {
            pk: PublicKey::from_bytes(&proto.deployer),
            sig: proto.sig.clone(),
            sig_algorithm: primary_alg,
            phlo_share: primary_phlo_share,
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
                phlo_share: cs.phlo_share,
            });
        }

        let data = DeployData::_from_proto(proto);
        // Phase 3 dispatch: when sig_algebra is set, the flat cosigners
        // path is ignored; verification walks the algebra.
        if let Some(algebra) = sig_algebra {
            return Self::from_proto_cosigned_with_sig_algebra(data, &algebra, phlo_limit);
        }
        // Phase 2 dispatch: cosigner_threshold == 0 → N-of-N (Phase 1
        // semantics; every signer must verify); cosigner_threshold > 0 →
        // M-of-N (at least `threshold` valid signatures required;
        // placeholder signers with empty sig are admitted).
        if cosigner_threshold == 0 {
            Cosigned::from_signed_data(data, signers, phlo_limit).map_err(|e| {
                format!(
                    "Cosigned envelope validation failed (is_multi_sig={}): {}",
                    is_multi_sig, e
                )
            })
        } else {
            Cosigned::from_signed_data_threshold(
                data,
                signers,
                phlo_limit,
                cosigner_threshold as u32,
            )
            .map_err(|e| {
                format!(
                    "Cosigned threshold envelope validation failed (threshold={}, total_signers={}): {}",
                    cosigner_threshold, total_signers, e
                )
            })
        }
    }

    /// Phase 3 LL-rich algebra dispatch. Validates a deploy against
    /// the given `SigCompound` algebraic expression:
    ///
    /// - Tensor(left, right): both branches must verify (recursive).
    /// - Plus(left, right, chosen_branch): only the chosen branch verifies.
    /// - With(left, right): both branches' atoms must be presented and
    ///   verify (the verifier picks which to consume at evaluation).
    /// - Bang(inner): inner atom must verify (replicable at evaluation).
    /// - WhyNot(inner): if inner has a non-empty sig, it must verify;
    ///   empty sig is permitted (optional / zero-or-more uses).
    /// - Lolly(from, to, handle): both branches must verify (the
    ///   transformer runs at evaluation via the capability registry).
    /// - Threshold{threshold, members}: at least `threshold` members
    ///   must verify (Phase 2 quorum semantics, lifted into the algebra).
    /// - Atom: the leaf signer; signature must verify.
    ///
    /// The collected atoms are folded into a canonical Cosigner list
    /// (sorted ascending by pk; placeholders for non-required atoms get
    /// empty sigs and zero shares) and `Cosigned::from_signed_data_threshold`
    /// (with `threshold = required_signers.len()`) finalizes the envelope.
    pub fn from_proto_cosigned_with_sig_algebra(
        data: DeployData,
        sig_algebra: &crate::casper::SigCompound,
        phlo_limit: i64,
    ) -> Result<crypto::rust::signatures::signed::Cosigned<DeployData>, String> {
        use crypto::rust::signatures::signed::{Cosigned, Cosigner};

        // Walk the algebra and collect EVERY atom into the signer list
        // (with its actual sig / phlo_share), and compute the minimum
        // number of valid signatures the algebra requires (`min_required`).
        // Per-connective semantics live in `min_required_for`:
        //
        //   - Atom: 1 (must verify)
        //   - Tensor(a, b): min(a) + min(b)
        //   - Plus(a, b, chosen=0): min(a)
        //   - Plus(a, b, chosen=1): min(b)
        //   - With(a, b): min(a) + min(b) (both committed at envelope time)
        //   - Bang(inner): min(inner)
        //   - WhyNot(inner): 0 (entirely optional)
        //   - Lolly(from, to): min(from) + min(to)
        //   - Threshold(k, members): k
        let mut atoms: Vec<AlgebraAtom> = Vec::new();
        Self::collect_atoms(sig_algebra, &mut atoms)?;
        let min_required = Self::min_required_for(sig_algebra)?;

        if atoms.is_empty() {
            return Err(
                "Sig algebra contains no atomic signers — at least one Atom is required"
                    .to_string(),
            );
        }

        let mut signers: Vec<Cosigner> = Vec::with_capacity(atoms.len());
        for atom in atoms.into_iter() {
            let alg = SignaturesAlgFactory::apply(&atom.sig_algorithm)
                .ok_or_else(|| format!("Unknown signature algorithm: {}", atom.sig_algorithm))?;
            signers.push(Cosigner {
                pk: PublicKey::from_bytes(&atom.pk),
                sig: atom.sig,
                sig_algorithm: alg,
                phlo_share: atom.phlo_share,
            });
        }

        // Dispatch:
        //   - All atoms required (min_required == signers.len(), no Plus
        //     branch dropped, no WhyNot absent, no Threshold) →
        //     from_signed_data (canonical N-of-N).
        //   - Otherwise → from_signed_data_threshold with min_required.
        let total = signers.len() as u32;
        if min_required == total && Self::algebra_is_all_required(sig_algebra)? {
            Cosigned::from_signed_data(data, signers, phlo_limit)
                .map_err(|e| format!("Cosigned sig_algebra validation failed: {}", e))
        } else if min_required == 0 {
            let presented: Vec<Cosigner> = signers
                .into_iter()
                .filter(|signer| !signer.sig.is_empty())
                .collect();
            if presented.is_empty() {
                return Err(
                    "Sig algebra requires zero valid signatures and presents no signer; a Cosigned envelope requires at least one"
                        .to_string(),
                );
            }
            Cosigned::from_signed_data(data, presented, phlo_limit).map_err(|e| {
                format!(
                    "Cosigned sig_algebra optional-present validation failed: {}",
                    e
                )
            })
        } else {
            Cosigned::from_signed_data_threshold(data, signers, phlo_limit, min_required).map_err(
                |e| {
                    format!(
                        "Cosigned sig_algebra threshold validation failed (min_required={}): {}",
                        min_required, e
                    )
                },
            )
        }
    }

    fn collect_atoms(
        sig: &crate::casper::SigCompound,
        atoms: &mut Vec<AlgebraAtom>,
    ) -> Result<(), String> {
        use crate::casper::sig_compound::Connective;
        let connective = sig
            .connective
            .as_ref()
            .ok_or_else(|| "SigCompound.connective missing".to_string())?;
        match connective {
            Connective::Atom(atom) => {
                atoms.push(AlgebraAtom::from_proto(atom));
                Ok(())
            }
            Connective::Tensor(pair) | Connective::With(pair) => {
                Self::collect_atoms_pair(pair, atoms)
            }
            Connective::Plus(plus) => {
                if plus.chosen_branch != 0 && plus.chosen_branch != 1 {
                    return Err(format!(
                        "SigPlus.chosen_branch must be 0 or 1, got {}",
                        plus.chosen_branch
                    ));
                }
                let left = plus
                    .left
                    .as_deref()
                    .ok_or_else(|| "SigPlus.left missing".to_string())?;
                let right = plus
                    .right
                    .as_deref()
                    .ok_or_else(|| "SigPlus.right missing".to_string())?;
                Self::collect_atoms(left, atoms)?;
                Self::collect_atoms(right, atoms)
            }
            Connective::Bang(bang) => {
                let inner = bang
                    .inner
                    .as_deref()
                    .ok_or_else(|| "SigBang.inner missing".to_string())?;
                Self::collect_atoms(inner, atoms)
            }
            Connective::Whynot(inner) => Self::collect_atoms(inner, atoms),
            Connective::Lolly(lolly) => {
                let from = lolly
                    .from
                    .as_deref()
                    .ok_or_else(|| "SigLolly.from missing".to_string())?;
                let to = lolly
                    .to
                    .as_deref()
                    .ok_or_else(|| "SigLolly.to missing".to_string())?;
                Self::collect_atoms(from, atoms)?;
                Self::collect_atoms(to, atoms)
            }
            Connective::Threshold(thresh) => {
                if thresh.threshold < 1 || (thresh.threshold as usize) > thresh.members.len() {
                    return Err(format!(
                        "SigThreshold.threshold must satisfy 1 ≤ threshold ≤ members.len() ({}), got {}",
                        thresh.members.len(),
                        thresh.threshold
                    ));
                }
                for member in &thresh.members {
                    Self::collect_atoms(member, atoms)?;
                }
                Ok(())
            }
        }
    }

    fn collect_atoms_pair(
        pair: &crate::casper::SigPair,
        atoms: &mut Vec<AlgebraAtom>,
    ) -> Result<(), String> {
        let left = pair
            .left
            .as_deref()
            .ok_or_else(|| "SigPair.left missing".to_string())?;
        let right = pair
            .right
            .as_deref()
            .ok_or_else(|| "SigPair.right missing".to_string())?;
        Self::collect_atoms(left, atoms)?;
        Self::collect_atoms(right, atoms)
    }

    fn min_required_for(sig: &crate::casper::SigCompound) -> Result<u32, String> {
        use crate::casper::sig_compound::Connective;
        let connective = sig
            .connective
            .as_ref()
            .ok_or_else(|| "SigCompound.connective missing".to_string())?;
        match connective {
            Connective::Atom(_) => Ok(1),
            Connective::Tensor(pair) | Connective::With(pair) => {
                let l = pair
                    .left
                    .as_deref()
                    .ok_or_else(|| "SigPair.left missing".to_string())?;
                let r = pair
                    .right
                    .as_deref()
                    .ok_or_else(|| "SigPair.right missing".to_string())?;
                Ok(Self::min_required_for(l)? + Self::min_required_for(r)?)
            }
            Connective::Plus(plus) => {
                let l = plus
                    .left
                    .as_deref()
                    .ok_or_else(|| "SigPlus.left missing".to_string())?;
                let r = plus
                    .right
                    .as_deref()
                    .ok_or_else(|| "SigPlus.right missing".to_string())?;
                if plus.chosen_branch == 0 {
                    Self::min_required_for(l)
                } else {
                    Self::min_required_for(r)
                }
            }
            Connective::Bang(bang) => {
                let inner = bang
                    .inner
                    .as_deref()
                    .ok_or_else(|| "SigBang.inner missing".to_string())?;
                Self::min_required_for(inner)
            }
            Connective::Whynot(_) => Ok(0),
            Connective::Lolly(lolly) => {
                let from = lolly
                    .from
                    .as_deref()
                    .ok_or_else(|| "SigLolly.from missing".to_string())?;
                let to = lolly
                    .to
                    .as_deref()
                    .ok_or_else(|| "SigLolly.to missing".to_string())?;
                Ok(Self::min_required_for(from)? + Self::min_required_for(to)?)
            }
            Connective::Threshold(thresh) => Ok(thresh.threshold as u32),
        }
    }

    /// Returns true iff the algebra has no optional branch (no Plus,
    /// no WhyNot, no Threshold). Used to choose between the N-of-N
    /// constructor and the threshold constructor.
    fn algebra_is_all_required(sig: &crate::casper::SigCompound) -> Result<bool, String> {
        use crate::casper::sig_compound::Connective;
        let connective = sig
            .connective
            .as_ref()
            .ok_or_else(|| "SigCompound.connective missing".to_string())?;
        match connective {
            Connective::Atom(_) => Ok(true),
            Connective::Tensor(pair) | Connective::With(pair) => {
                let l = pair
                    .left
                    .as_deref()
                    .ok_or_else(|| "SigPair.left missing".to_string())?;
                let r = pair
                    .right
                    .as_deref()
                    .ok_or_else(|| "SigPair.right missing".to_string())?;
                Ok(Self::algebra_is_all_required(l)? && Self::algebra_is_all_required(r)?)
            }
            Connective::Bang(bang) => {
                let inner = bang
                    .inner
                    .as_deref()
                    .ok_or_else(|| "SigBang.inner missing".to_string())?;
                Self::algebra_is_all_required(inner)
            }
            Connective::Lolly(lolly) => {
                let from = lolly
                    .from
                    .as_deref()
                    .ok_or_else(|| "SigLolly.from missing".to_string())?;
                let to = lolly
                    .to
                    .as_deref()
                    .ok_or_else(|| "SigLolly.to missing".to_string())?;
                Ok(Self::algebra_is_all_required(from)? && Self::algebra_is_all_required(to)?)
            }
            Connective::Plus(_) | Connective::Whynot(_) | Connective::Threshold(_) => Ok(false),
        }
    }

    fn _to_proto(dd: DeployData) -> DeployDataProto {
        DeployDataProto {
            term: dd.term,
            timestamp: dd.time_stamp,
            phlo_price: dd.phlo_price,
            phlo_limit: dd.phlo_limit,
            valid_after_block_number: dd.valid_after_block_number,
            shard_id: dd.shard_id,
            // Only include expirationTimestamp if set to maintain backward compatibility
            expiration_timestamp: dd.expiration_timestamp.unwrap_or(0),
            ..Default::default()
        }
    }

    pub fn to_proto(dd: Signed<DeployData>) -> DeployDataProto {
        DeployDataProto {
            term: dd.data.term.clone(),
            timestamp: dd.data.time_stamp,
            phlo_price: dd.data.phlo_price,
            phlo_limit: dd.data.phlo_limit,
            valid_after_block_number: dd.data.valid_after_block_number,
            shard_id: dd.data.shard_id.clone(),
            deployer: dd.pk.bytes.clone().into(),
            sig: dd.sig.clone().into(),
            sig_algorithm: dd.sig_algorithm.name(),
            // Only include expirationTimestamp if set to maintain backward compatibility
            expiration_timestamp: dd.data.expiration_timestamp.unwrap_or(0),
            ..Default::default()
        }
    }

    /// Serialize a [`Cosigned<DeployData>`] back to [`DeployDataProto`] wire
    /// format. For single-signer cosigned envelopes the output is
    /// byte-identical to `to_proto(signed)` (cosigners empty,
    /// primary_phlo_share = 0). For multi-signer envelopes the additional
    /// cosigners populate the `cosigners[]` field and `primary_phlo_share`
    /// carries the primary signer's contribution.
    pub fn to_proto_cosigned(
        cosigned: &crypto::rust::signatures::signed::Cosigned<DeployData>,
    ) -> DeployDataProto {
        let primary = cosigned.primary();
        let is_compound = cosigned.is_compound();
        let cosigners_proto: Vec<crate::casper::CompoundSigner> = if is_compound {
            cosigned
                .signers()
                .iter()
                .skip(1) // primary occupies fields 1/4/5/15; cosigners[] is the rest
                .map(|c| crate::casper::CompoundSigner {
                    pk: c.pk.bytes.clone().into(),
                    sig: c.sig.clone(),
                    sig_algorithm: c.sig_algorithm.name(),
                    phlo_share: c.phlo_share,
                })
                .collect()
        } else {
            Vec::new()
        };
        // For multi-sig deploys, primary_phlo_share is the explicit share
        // from the wire. For single-sig (legacy uplift), set to 0 so
        // round-trip with `from_proto_cosigned` recovers the single-sig
        // legacy semantics (where the primary's share = phlo_limit and
        // `cosigners` is empty).
        let primary_phlo_share = if is_compound { primary.phlo_share } else { 0 };
        DeployDataProto {
            term: cosigned.data.term.clone(),
            timestamp: cosigned.data.time_stamp,
            phlo_price: cosigned.data.phlo_price,
            phlo_limit: cosigned.data.phlo_limit,
            valid_after_block_number: cosigned.data.valid_after_block_number,
            shard_id: cosigned.data.shard_id.clone(),
            deployer: primary.pk.bytes.clone().into(),
            sig: primary.sig.clone(),
            sig_algorithm: primary.sig_algorithm.name(),
            expiration_timestamp: cosigned.data.expiration_timestamp.unwrap_or(0),
            cosigners: cosigners_proto,
            primary_phlo_share,
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

#[cfg(kani)]
mod kani_cost_accounting {
    use super::*;

    #[kani::proof]
    fn checked_total_phlo_charge_rejects_negative_inputs() {
        let phlo_limit: i64 = kani::any();
        let phlo_price: i64 = kani::any();
        kani::assume(phlo_limit < 0 || phlo_price < 0);

        let result = DeployData::checked_total_phlo_charge_value(phlo_limit, phlo_price);

        assert!(result.is_none());
    }

    #[kani::proof]
    fn checked_total_phlo_charge_matches_product_on_small_valid_domain() {
        let phlo_limit_raw: u8 = kani::any();
        let phlo_price_raw: u8 = kani::any();
        let phlo_limit = i64::from(phlo_limit_raw);
        let phlo_price = i64::from(phlo_price_raw);

        let result = DeployData::checked_total_phlo_charge_value(phlo_limit, phlo_price);

        assert_eq!(result.unwrap(), phlo_limit * phlo_price);
    }

    #[kani::proof]
    fn refund_amount_is_bounded_on_small_valid_domain() {
        let phlo_limit_raw: u8 = kani::any();
        let phlo_price_raw: u8 = kani::any();
        let token_cost_raw: u8 = kani::any();
        kani::assume(phlo_limit_raw <= 15);
        kani::assume(phlo_price_raw <= 15);
        kani::assume(token_cost_raw <= 20);
        let phlo_limit = i64::from(phlo_limit_raw);
        let phlo_price = i64::from(phlo_price_raw);
        let token_cost = i64::from(token_cost_raw);

        let refund =
            DeployData::refund_amount_for_token_cost_value(phlo_limit, phlo_price, token_cost)
                .expect("bounded valid domain");
        let escrow = phlo_limit * phlo_price;

        assert!(refund >= 0);
        assert!(refund <= escrow);
        if token_cost <= phlo_limit {
            assert_eq!(refund + token_cost * phlo_price, escrow);
        } else {
            assert_eq!(refund, 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;
    use crypto::rust::signatures::signed::Signed;
    use proptest::prelude::*;

    use super::*;

    fn deploy_data(phlo_limit: i64, phlo_price: i64) -> DeployData {
        DeployData {
            term: "Nil".to_string(),
            time_stamp: 0,
            phlo_price,
            phlo_limit,
            valid_after_block_number: 0,
            shard_id: "root".to_string(),
            expiration_timestamp: None,
        }
    }

    fn signed_deploy(data: DeployData) -> Signed<DeployData> {
        let alg: Box<dyn SignaturesAlg> = Box::new(Secp256k1);
        let (sk, _) = alg.new_key_pair();
        Signed::create(data, alg, sk).expect("signed deploy")
    }

    #[test]
    fn checked_total_phlo_charge_rejects_invalid_or_overflowing_inputs() {
        assert_eq!(deploy_data(5, 2).checked_total_phlo_charge(), Ok(10));
        assert!(deploy_data(i64::MAX, 2)
            .checked_total_phlo_charge()
            .is_err());
        assert!(deploy_data(-1, 2).checked_total_phlo_charge().is_err());
        assert!(deploy_data(10, -1).checked_total_phlo_charge().is_err());
        assert!(deploy_data(10, 1).validate_phlo(2).is_err());
    }

    #[test]
    fn refund_amount_is_bounded_by_valid_escrow() {
        let partial = ProcessedDeploy {
            deploy: signed_deploy(deploy_data(5, 2)),
            cost: PCost { cost: 1 },
            deploy_log: Vec::new(),
            is_failed: false,
            system_deploy_error: None,
            cosigners: Vec::new(),
            primary_phlo_share: 0,
            cosigner_threshold: 0,
        };
        assert_eq!(partial.try_refund_amount(), Ok(8));

        let exhausted = ProcessedDeploy {
            deploy: signed_deploy(deploy_data(5, 2)),
            cost: PCost { cost: 10 },
            deploy_log: Vec::new(),
            is_failed: true,
            system_deploy_error: None,
            cosigners: Vec::new(),
            primary_phlo_share: 0,
            cosigner_threshold: 0,
        };
        assert_eq!(exhausted.try_refund_amount(), Ok(0));

        let negative_price = ProcessedDeploy {
            deploy: signed_deploy(deploy_data(10, -1)),
            cost: PCost { cost: 1 },
            deploy_log: Vec::new(),
            is_failed: false,
            system_deploy_error: None,
            cosigners: Vec::new(),
            primary_phlo_share: 0,
            cosigner_threshold: 0,
        };
        assert!(negative_price.try_refund_amount().is_err());

        let oversized_cost = ProcessedDeploy {
            deploy: signed_deploy(deploy_data(10, 1)),
            cost: PCost { cost: u64::MAX },
            deploy_log: Vec::new(),
            is_failed: false,
            system_deploy_error: None,
            cosigners: Vec::new(),
            primary_phlo_share: 0,
            cosigner_threshold: 0,
        };
        assert!(oversized_cost.try_refund_amount().is_err());
    }

    #[test]
    fn settlement_edge_cases_are_total_and_deterministic() {
        assert_eq!(deploy_data(10, 0).checked_total_phlo_charge(), Ok(0));
        assert_eq!(deploy_data(10, 0).refund_amount_for_token_cost(5), Ok(0));
        assert_eq!(deploy_data(0, 10).checked_total_phlo_charge(), Ok(0));
        assert_eq!(deploy_data(0, 10).refund_amount_for_token_cost(0), Ok(0));
        assert_eq!(deploy_data(5, 2).refund_amount_for_token_cost(5), Ok(0));
        assert_eq!(deploy_data(5, 2).refund_amount_for_token_cost(8), Ok(0));
        assert!(deploy_data(5, 2).refund_amount_for_token_cost(-1).is_err());
        assert_eq!(
            deploy_data(i64::MAX, 1).checked_total_phlo_charge(),
            Ok(i64::MAX)
        );
        assert_eq!(
            deploy_data(i64::MAX, 1).refund_amount_for_token_cost(i64::MAX),
            Ok(0)
        );
        assert!(deploy_data(i64::MAX, 2)
            .checked_total_phlo_charge()
            .is_err());
    }

    fn fresh_atom_signing(
        payload: &DeployData,
        phlo_share: i64,
    ) -> (crate::casper::SigAtom, Vec<u8>) {
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
                phlo_share,
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
            phlo_share: 0,
        }
    }

    #[test]
    fn from_proto_cosigned_sig_algebra_tensor_validates_both_branches() {
        let payload = deploy_data(200, 1);
        let (atom_a, _) = fresh_atom_signing(&payload, 100);
        let (atom_b, _) = fresh_atom_signing(&payload, 100);
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
        let cosigned = DeployData::from_proto_cosigned_with_sig_algebra(payload, &algebra, 200)
            .expect("Tensor with two valid signers must verify");
        assert_eq!(cosigned.signers().len(), 2);
    }

    #[test]
    fn from_proto_cosigned_sig_algebra_plus_chosen_left_only() {
        let payload = deploy_data(100, 1);
        let (atom_a, _) = fresh_atom_signing(&payload, 100);
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
        let cosigned = DeployData::from_proto_cosigned_with_sig_algebra(payload, &algebra, 100)
            .expect("Plus with chosen=0 + valid left sig must verify");
        assert_eq!(cosigned.signers().len(), 2);
    }

    #[test]
    fn from_proto_cosigned_sig_algebra_threshold_2_of_3_satisfied() {
        let payload = deploy_data(200, 1);
        let (atom_a, _) = fresh_atom_signing(&payload, 100);
        let (atom_b, _) = fresh_atom_signing(&payload, 100);
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
        let cosigned = DeployData::from_proto_cosigned_with_sig_algebra(payload, &algebra, 200)
            .expect("Threshold 2-of-3 with 2 valid sigs must verify");
        assert_eq!(cosigned.signers().len(), 3);
    }

    #[test]
    fn from_proto_cosigned_sig_algebra_whynot_admits_absent_signer() {
        let payload = deploy_data(0, 1);
        let absent_atom = empty_atom();
        let algebra = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Whynot(Box::new(
                crate::casper::SigCompound {
                    connective: Some(crate::casper::sig_compound::Connective::Atom(absent_atom)),
                },
            ))),
        };
        // WhyNot with empty atom → no required signers; phlo_limit=0
        // because no fuel is paid by an absent atom. We need at least
        // one signer in the envelope, so the algebra walks to "optional"
        // and the Cosigned constructor sees a single placeholder. This
        // exercises the "WhyNot absent" code path.
        let result = DeployData::from_proto_cosigned_with_sig_algebra(payload, &algebra, 0);
        // The envelope requires a non-empty signer list; absent WhyNot
        // alone is invalid (the deploy must have at least one signer).
        // We expect this to fail at the empty-signer-list invariant
        // rather than at quorum.
        assert!(
            result.is_err(),
            "WhyNot with only absent atom must fail (empty signer list)"
        );
    }

    #[test]
    fn from_proto_cosigned_sig_algebra_whynot_present_signer_verifies() {
        let payload = deploy_data(100, 1);
        let (atom, _) = fresh_atom_signing(&payload, 100);
        let algebra = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Whynot(Box::new(
                crate::casper::SigCompound {
                    connective: Some(crate::casper::sig_compound::Connective::Atom(atom)),
                },
            ))),
        };
        let cosigned = DeployData::from_proto_cosigned_with_sig_algebra(payload, &algebra, 100)
            .expect("present WhyNot signer must verify when it funds phlo");
        assert_eq!(cosigned.signers().len(), 1);
        assert_eq!(cosigned.total_phlo_share(), 100);
    }

    #[test]
    fn from_proto_cosigned_sig_algebra_whynot_present_invalid_rejected() {
        let payload = deploy_data(100, 1);
        let other_payload = deploy_data(100, 99);
        let (mut atom, _) = fresh_atom_signing(&other_payload, 100);
        atom.phlo_share = 100;
        let algebra = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Whynot(Box::new(
                crate::casper::SigCompound {
                    connective: Some(crate::casper::sig_compound::Connective::Atom(atom)),
                },
            ))),
        };
        let err = DeployData::from_proto_cosigned_with_sig_algebra(payload, &algebra, 100)
            .expect_err("present invalid WhyNot signer must reject");
        assert!(
            err.contains("failed signature verification"),
            "error must identify signature verification failure: {}",
            err
        );
    }

    #[test]
    fn from_proto_cosigned_sig_algebra_plus_invalid_chosen_branch_rejected() {
        let payload = deploy_data(100, 1);
        let (atom_a, _) = fresh_atom_signing(&payload, 100);
        let (atom_b, _) = fresh_atom_signing(&payload, 100);
        let algebra = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Plus(Box::new(
                crate::casper::SigPlus {
                    left: Some(Box::new(crate::casper::SigCompound {
                        connective: Some(crate::casper::sig_compound::Connective::Atom(atom_a)),
                    })),
                    right: Some(Box::new(crate::casper::SigCompound {
                        connective: Some(crate::casper::sig_compound::Connective::Atom(atom_b)),
                    })),
                    chosen_branch: 2, // invalid
                },
            ))),
        };
        let err = DeployData::from_proto_cosigned_with_sig_algebra(payload, &algebra, 100)
            .expect_err("invalid chosen_branch must reject");
        assert!(err.contains("chosen_branch"));
    }

    #[test]
    fn processed_deploy_cosigner_threshold_roundtrips_through_proto() {
        let processed = ProcessedDeploy {
            deploy: signed_deploy(deploy_data(100, 1)),
            cost: PCost { cost: 0 },
            deploy_log: Vec::new(),
            is_failed: false,
            system_deploy_error: None,
            cosigners: Vec::new(),
            primary_phlo_share: 100,
            cosigner_threshold: 2,
        };

        let decoded = ProcessedDeploy::from_proto(processed.clone().to_proto()).unwrap();
        assert_eq!(decoded.cosigner_threshold, 2);
        assert_eq!(decoded.primary_phlo_share, 100);
    }

    proptest! {
        #![proptest_config(proptest::test_runner::Config::with_cases(256))]

        #[test]
        fn refund_amount_property_is_bounded_by_valid_escrow(
            phlo_limit in 0i64..1_000_000,
            phlo_price in 0i64..1_000_000,
            token_cost in 0u64..2_000_000,
        ) {
            let processed = ProcessedDeploy {
                deploy: signed_deploy(deploy_data(phlo_limit, phlo_price)),
                cost: PCost { cost: token_cost },
                deploy_log: Vec::new(),
                is_failed: false,
                system_deploy_error: None,
            cosigners: Vec::new(),
            primary_phlo_share: 0,
            cosigner_threshold: 0,
            };

            let refund = processed.try_refund_amount().unwrap();
            let escrow = phlo_limit.checked_mul(phlo_price).unwrap();
            prop_assert!(refund >= 0);
            prop_assert!(refund <= escrow);

            let token_cost_i64 = i64::try_from(token_cost).unwrap();
            if token_cost_i64 <= phlo_limit {
                prop_assert_eq!(refund + token_cost_i64 * phlo_price, escrow);
            } else {
                prop_assert_eq!(refund, 0);
            }
        }
    }
}
