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
    /// M-of-N quorum threshold (Phase 2). 0 = N-of-N semantics (every
    /// signer's signature must verify); k > 0 = at least k signatures
    /// must verify. Round-trips through `DeployDataProto.cosigner_threshold`
    /// (proto field 16).
    pub cosigner_threshold: i32,
}

impl ProcessedDeploy {
    // D3 (DR-9): `try_refund_amount`/`refund_amount` are REMOVED — there is no
    // escrow to refund. The deploy's `cost` is the per-COMM token count, debited
    // once from the per-signature supply pool Σ⟦s⟧ at block close (no per-deploy
    // pre-charge/refund settlement).

    pub fn empty(deploy: Signed<DeployData>) -> Self {
        Self {
            deploy,
            cost: PCost { cost: 0 },
            deploy_log: Vec::new(),
            is_failed: false,
            system_deploy_error: None,
            cosigners: Vec::new(),
            cosigner_threshold: 0,
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
                Cosigned::from_signed_data(self.deploy.data.clone(), signers)
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

    pub fn create_close() -> Self {
        Self::CloseBlockSystemDeployData
    }

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
            system_deploy_data_proto::SystemDeploy::RedeemSystemDeploy(redeem) => Ok(Self::Redeem {
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
            }),
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
    // deploy's cost is the per-COMM token count (computed by the runtime); it
    // is funded by the per-signature supply pool Σ⟦s⟧ and gated at block
    // assembly (`casper/.../util/rholang/acceptance.rs`). There is no per-deploy
    // budget cap and no refund settlement.

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
    /// captures `proto.cosigners` into the `ProcessedDeploy.cosigners` field,
    /// so the cosigner data is preserved across deserialization even though the
    /// inner `Signed<DeployData>` only carries the primary.
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
    /// empty → one-element envelope) and multi-sig wire deploys (`cosigners`
    /// non-empty → N-element envelope).
    ///
    /// Invariants enforced by `Cosigned::from_signed_data` at construction:
    /// 1. All signers' signatures verify against the canonical message hash.
    /// 2. Canonical pk-ascending sort; no duplicates.
    ///
    /// D3 (DR-9): there is no per-signer `phlo_share` and no share-sum
    /// invariant — fuel is the per-signature supply pool Σ⟦s⟧.
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

        // Build the canonical signer list. Primary first (will be sorted
        // canonically by Cosigned::from_signed_data). D3 (DR-9): no per-signer
        // phlo_share — funding is the per-signature supply pool Σ⟦s⟧.
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
        // Phase 3 dispatch: when sig_algebra is set, the flat cosigners
        // path is ignored; verification walks the algebra.
        if let Some(algebra) = sig_algebra {
            return Self::from_proto_cosigned_with_sig_algebra(data, &algebra);
        }
        // Phase 2 dispatch: cosigner_threshold == 0 → N-of-N (Phase 1
        // semantics; every signer must verify); cosigner_threshold > 0 →
        // M-of-N (at least `threshold` valid signatures required;
        // placeholder signers with empty sig are admitted).
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
    ) -> Result<crypto::rust::signatures::signed::Cosigned<DeployData>, String> {
        use crypto::rust::signatures::signed::{Cosigned, Cosigner};

        // Walk the algebra and collect EVERY atom into the signer list
        // (with its actual sig), and compute the minimum number of valid
        // signatures the algebra requires (`min_required`).
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
            });
        }

        // Dispatch:
        //   - All atoms required (min_required == signers.len(), no Plus
        //     branch dropped, no WhyNot absent, no Threshold) →
        //     from_signed_data (canonical N-of-N).
        //   - Otherwise → from_signed_data_threshold with min_required.
        let total = signers.len() as u32;
        if min_required == total && Self::algebra_is_all_required(sig_algebra)? {
            Cosigned::from_signed_data(data, signers)
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
            Cosigned::from_signed_data(data, presented).map_err(|e| {
                format!(
                    "Cosigned sig_algebra optional-present validation failed: {}",
                    e
                )
            })
        } else {
            Cosigned::from_signed_data_threshold(data, signers, min_required).map_err(|e| {
                format!(
                    "Cosigned sig_algebra threshold validation failed (min_required={}): {}",
                    min_required, e
                )
            })
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
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;
    use crypto::rust::signatures::signed::Signed;
    use proptest::prelude::*;

    use super::*;

    fn deploy_data() -> DeployData {
        DeployData {
            term: "Nil".to_string(),
            time_stamp: 0,
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
            !hex::encode(&preimage).contains("3802") && !preimage.windows(2).any(|w| w == [0x40, 0x05]),
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

    #[test]
    fn from_proto_cosigned_sig_algebra_plus_chosen_left_only() {
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
        let cosigned = DeployData::from_proto_cosigned_with_sig_algebra(payload, &algebra)
            .expect("Plus with chosen=0 + valid left sig must verify");
        assert_eq!(cosigned.signers().len(), 2);
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

    #[test]
    fn from_proto_cosigned_sig_algebra_whynot_admits_absent_signer() {
        let payload = deploy_data();
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
        let result = DeployData::from_proto_cosigned_with_sig_algebra(payload, &algebra);
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
        let payload = deploy_data();
        let (atom, _) = fresh_atom_signing(&payload);
        let algebra = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Whynot(Box::new(
                crate::casper::SigCompound {
                    connective: Some(crate::casper::sig_compound::Connective::Atom(atom)),
                },
            ))),
        };
        let cosigned = DeployData::from_proto_cosigned_with_sig_algebra(payload, &algebra)
            .expect("present WhyNot signer must verify");
        assert_eq!(cosigned.signers().len(), 1);
    }

    #[test]
    fn from_proto_cosigned_sig_algebra_whynot_present_invalid_rejected() {
        let payload = deploy_data();
        let other_payload = DeployData {
            term: "other-payload".to_string(),
            ..deploy_data()
        };
        let (atom, _) = fresh_atom_signing(&other_payload);
        let algebra = crate::casper::SigCompound {
            connective: Some(crate::casper::sig_compound::Connective::Whynot(Box::new(
                crate::casper::SigCompound {
                    connective: Some(crate::casper::sig_compound::Connective::Atom(atom)),
                },
            ))),
        };
        let err = DeployData::from_proto_cosigned_with_sig_algebra(payload, &algebra)
            .expect_err("present invalid WhyNot signer must reject");
        assert!(
            err.contains("failed signature verification"),
            "error must identify signature verification failure: {}",
            err
        );
    }

    #[test]
    fn from_proto_cosigned_sig_algebra_plus_invalid_chosen_branch_rejected() {
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
                    chosen_branch: 2, // invalid
                },
            ))),
        };
        let err = DeployData::from_proto_cosigned_with_sig_algebra(payload, &algebra)
            .expect_err("invalid chosen_branch must reject");
        assert!(err.contains("chosen_branch"));
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
        };

        let decoded = ProcessedDeploy::from_proto(processed.clone().to_proto()).unwrap();
        assert_eq!(decoded.cosigner_threshold, 2);
    }
}
