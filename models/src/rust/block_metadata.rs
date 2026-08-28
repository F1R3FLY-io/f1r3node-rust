// See models/src/main/scala/coop/rchain/models/BlockMetadata.scala

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use crypto::rust::hash::blake2b256::Blake2b256;
use prost::bytes::Bytes;
use prost::Message;

use super::casper::protocol::casper_message::{
    BlockMessage, F1r3flyState, FinalizedFloorCommitment, Justification,
    ObjectiveEquivocationEvidence, ProcessedSystemDeploy, StateEffectId,
};
use crate::casper::{
    BlockMetadataInternal, BondProto, CertifiedAdmissionOutcomeProto, CertifiedSenderAuthorityProto,
};
use crate::rust::bond_generation::BondGeneration;
use crate::rust::{block_hash, validator};

pub const ADMISSION_SCHEMA_VERSION: u32 = 12;
pub const CERTIFIED_ADMISSION_PROTOCOL_VERSION: i64 = 6;
pub const STATE_EFFECT_PROVENANCE_PROTOCOL_VERSION: i64 = 3;
pub const ADMISSION_RULESET_MANIFEST: &str = "f1r3fly-certified-admission-v12|0:accepted|1:invalid-format|2:invalid-signature|3:invalid-sender|4:invalid-version|5:invalid-timestamp|6:deploy-not-signed|7:invalid-block-number|8:invalid-repeat-deploy|9:invalid-parents|10:invalid-follows|11:invalid-sequence-number|12:invalid-shard-id|13:justification-regression|14:neglected-invalid-block|15:neglected-equivocation|16:invalid-transaction|17:invalid-bonds-cache|18:invalid-equivocation-evidence|19:invalid-block-hash|20:unauthorized-slash-deploy|21:invalid-rejected-deploy|22:contains-expired-deploy|23:contains-time-expired-deploy|24:contains-future-deploy|25:not-of-interest|26:low-deploy-cost|finalization-ledger:atomic-rooted-hash-chain-v2|finalized-floor:durable-parent-effective-floor-v1|certificate-cache:candidate-transparent-v1|candidate-authority-context:signed-exact-v1|certificate-sidecar:manifest-digests-and-counts-v2";

pub fn admission_ruleset_digest() -> Bytes {
    Blake2b256::hash(ADMISSION_RULESET_MANIFEST.as_bytes().to_vec()).into()
}

fn append_digest_bytes(output: &mut Vec<u8>, value: &[u8]) {
    output.extend_from_slice(&(value.len() as u64).to_be_bytes());
    output.extend_from_slice(value);
}

fn append_digest_i64(output: &mut Vec<u8>, value: i64) {
    output.extend_from_slice(&value.to_be_bytes());
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize
)]
pub struct CertifiedSenderAuthority {
    #[serde(with = "shared::rust::serde_bytes")]
    block_hash: Bytes,
    protocol_version: i64,
    #[serde(with = "shared::rust::serde_bytes")]
    authority_floor_hash: Bytes,
    #[serde(with = "shared::rust::serde_bytes")]
    authority_floor_post_state_hash: Bytes,
    #[serde(with = "shared::rust::serde_bytes")]
    context_digest: Bytes,
    #[serde(with = "shared::rust::serde_bytes")]
    sender: Bytes,
    generation: BondGeneration,
    stake: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CertifiedSenderAuthorityError {
    #[error("sender authority certificate block hash must be {expected} bytes, got {actual}")]
    InvalidBlockHash { expected: usize, actual: usize },
    #[error("sender authority certificate floor hash must be {expected} bytes, got {actual}")]
    InvalidAuthorityFloorHash { expected: usize, actual: usize },
    #[error(
        "sender authority certificate floor post-state hash must be {expected} bytes, got {actual}"
    )]
    InvalidAuthorityFloorPostStateHash { expected: usize, actual: usize },
    #[error("sender authority certificate context digest must be {expected} bytes, got {actual}")]
    InvalidContextDigest { expected: usize, actual: usize },
    #[error("sender authority certificate sender must be {expected} bytes, got {actual}")]
    InvalidSender { expected: usize, actual: usize },
    #[error("sender authority certificate generation does not match the block")]
    GenerationMismatch,
    #[error("sender authority certificate protocol version does not match the block")]
    ProtocolVersionMismatch,
    #[error("sender authority certificate block hash does not match the block")]
    BlockHashMismatch,
    #[error("sender authority certificate sender does not match the block")]
    SenderMismatch,
    #[error("sender authority certificate floor hash does not match the certified context")]
    AuthorityFloorMismatch,
    #[error("sender authority certificate floor post-state does not match the certified context")]
    AuthorityFloorPostStateMismatch,
    #[error("sender authority certificate digest does not match the certified context")]
    ContextDigestMismatch,
    #[error("sender authority certificate stake does not match the certified context")]
    StakeMismatch,
    #[error("sender authority certificate stake must be positive, got {0}")]
    InvalidStake(i64),
    #[error("sender authority certificate protobuf contains an invalid generation: {0}")]
    InvalidGeneration(String),
}

impl CertifiedSenderAuthority {
    pub fn new(
        block: &BlockMessage,
        authority_floor_hash: Bytes,
        authority_floor_post_state_hash: Bytes,
        context_digest: Bytes,
        generation: BondGeneration,
        stake: i64,
    ) -> Result<Self, CertifiedSenderAuthorityError> {
        let certificate = Self {
            block_hash: block.block_hash.clone(),
            protocol_version: block.header.version,
            authority_floor_hash,
            authority_floor_post_state_hash,
            context_digest,
            sender: block.sender.clone(),
            generation,
            stake,
        };
        certificate.validate_for(block)?;
        Ok(certificate)
    }

    pub fn block_hash(&self) -> &Bytes { &self.block_hash }

    pub const fn protocol_version(&self) -> i64 { self.protocol_version }

    pub fn authority_floor_hash(&self) -> &Bytes { &self.authority_floor_hash }

    pub fn authority_floor_post_state_hash(&self) -> &Bytes {
        &self.authority_floor_post_state_hash
    }

    pub fn context_digest(&self) -> &Bytes { &self.context_digest }

    pub fn sender(&self) -> &Bytes { &self.sender }

    pub const fn generation(&self) -> BondGeneration { self.generation }

    pub const fn stake(&self) -> i64 { self.stake }

    pub fn digest(&self) -> Bytes {
        let mut bytes = Vec::new();
        append_digest_bytes(&mut bytes, b"f1r3fly-certified-sender-authority-v1");
        append_digest_bytes(&mut bytes, &self.block_hash);
        append_digest_i64(&mut bytes, self.protocol_version);
        append_digest_bytes(&mut bytes, &self.authority_floor_hash);
        append_digest_bytes(&mut bytes, &self.authority_floor_post_state_hash);
        append_digest_bytes(&mut bytes, &self.context_digest);
        append_digest_bytes(&mut bytes, &self.sender);
        append_digest_i64(&mut bytes, self.generation.get());
        append_digest_i64(&mut bytes, self.stake);
        Blake2b256::hash(bytes).into()
    }

    pub fn validate_for(&self, block: &BlockMessage) -> Result<(), CertifiedSenderAuthorityError> {
        self.validate_shape()?;
        if self.block_hash != block.block_hash {
            return Err(CertifiedSenderAuthorityError::BlockHashMismatch);
        }
        if self.protocol_version != block.header.version {
            return Err(CertifiedSenderAuthorityError::ProtocolVersionMismatch);
        }
        if self.sender != block.sender {
            return Err(CertifiedSenderAuthorityError::SenderMismatch);
        }
        if block.header.sender_bond_generation != Some(self.generation) {
            return Err(CertifiedSenderAuthorityError::GenerationMismatch);
        }
        Ok(())
    }

    pub fn validate_context(
        &self,
        authority_floor_hash: &Bytes,
        authority_floor_post_state_hash: &Bytes,
        context_digest: &Bytes,
        generation: BondGeneration,
        stake: i64,
    ) -> Result<(), CertifiedSenderAuthorityError> {
        self.validate_shape()?;
        if self.authority_floor_hash != authority_floor_hash {
            return Err(CertifiedSenderAuthorityError::AuthorityFloorMismatch);
        }
        if self.authority_floor_post_state_hash != authority_floor_post_state_hash {
            return Err(CertifiedSenderAuthorityError::AuthorityFloorPostStateMismatch);
        }
        if self.context_digest != context_digest {
            return Err(CertifiedSenderAuthorityError::ContextDigestMismatch);
        }
        if self.generation != generation {
            return Err(CertifiedSenderAuthorityError::GenerationMismatch);
        }
        if self.stake != stake {
            return Err(CertifiedSenderAuthorityError::StakeMismatch);
        }
        Ok(())
    }

    pub fn validate_shape(&self) -> Result<(), CertifiedSenderAuthorityError> {
        if self.block_hash.len() != block_hash::LENGTH {
            return Err(CertifiedSenderAuthorityError::InvalidBlockHash {
                expected: block_hash::LENGTH,
                actual: self.block_hash.len(),
            });
        }
        if self.authority_floor_hash.len() != block_hash::LENGTH {
            return Err(CertifiedSenderAuthorityError::InvalidAuthorityFloorHash {
                expected: block_hash::LENGTH,
                actual: self.authority_floor_hash.len(),
            });
        }
        if self.authority_floor_post_state_hash.len() != block_hash::LENGTH {
            return Err(
                CertifiedSenderAuthorityError::InvalidAuthorityFloorPostStateHash {
                    expected: block_hash::LENGTH,
                    actual: self.authority_floor_post_state_hash.len(),
                },
            );
        }
        if self.context_digest.len() != block_hash::LENGTH {
            return Err(CertifiedSenderAuthorityError::InvalidContextDigest {
                expected: block_hash::LENGTH,
                actual: self.context_digest.len(),
            });
        }
        if self.sender.len() != validator::LENGTH {
            return Err(CertifiedSenderAuthorityError::InvalidSender {
                expected: validator::LENGTH,
                actual: self.sender.len(),
            });
        }
        if self.stake <= 0 {
            return Err(CertifiedSenderAuthorityError::InvalidStake(self.stake));
        }
        Ok(())
    }

    pub fn to_proto(&self) -> CertifiedSenderAuthorityProto {
        CertifiedSenderAuthorityProto {
            block_hash: self.block_hash.clone(),
            protocol_version: self.protocol_version,
            sender: self.sender.clone(),
            bond_generation: self.generation.get(),
            stake: self.stake,
            authority_floor_hash: self.authority_floor_hash.clone(),
            authority_floor_post_state_hash: self.authority_floor_post_state_hash.clone(),
            context_digest: self.context_digest.clone(),
        }
    }

    pub fn from_proto(
        proto: CertifiedSenderAuthorityProto,
    ) -> Result<Self, CertifiedSenderAuthorityError> {
        let certificate = Self {
            block_hash: proto.block_hash,
            protocol_version: proto.protocol_version,
            authority_floor_hash: proto.authority_floor_hash,
            authority_floor_post_state_hash: proto.authority_floor_post_state_hash,
            context_digest: proto.context_digest,
            sender: proto.sender,
            generation: BondGeneration::try_from(proto.bond_generation).map_err(|error| {
                CertifiedSenderAuthorityError::InvalidGeneration(error.to_string())
            })?,
            stake: proto.stake,
        };
        certificate.validate_shape()?;
        Ok(certificate)
    }
}

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize
)]
#[repr(u32)]
pub enum AdmissionRejectionReason {
    InvalidFormat = 1,
    InvalidSignature = 2,
    InvalidSender = 3,
    InvalidVersion = 4,
    InvalidTimestamp = 5,
    DeployNotSigned = 6,
    InvalidBlockNumber = 7,
    InvalidRepeatDeploy = 8,
    InvalidParents = 9,
    InvalidFollows = 10,
    InvalidSequenceNumber = 11,
    InvalidShardId = 12,
    JustificationRegression = 13,
    NeglectedInvalidBlock = 14,
    NeglectedEquivocation = 15,
    InvalidTransaction = 16,
    InvalidBondsCache = 17,
    InvalidEquivocationEvidence = 18,
    InvalidBlockHash = 19,
    UnauthorizedSlashDeploy = 20,
    InvalidRejectedDeploy = 21,
    ContainsExpiredDeploy = 22,
    ContainsTimeExpiredDeploy = 23,
    ContainsFutureDeploy = 24,
    NotOfInterest = 25,
    LowDeployCost = 26,
}

impl TryFrom<u32> for AdmissionRejectionReason {
    type Error = CertifiedAdmissionOutcomeError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::InvalidFormat),
            2 => Ok(Self::InvalidSignature),
            3 => Ok(Self::InvalidSender),
            4 => Ok(Self::InvalidVersion),
            5 => Ok(Self::InvalidTimestamp),
            6 => Ok(Self::DeployNotSigned),
            7 => Ok(Self::InvalidBlockNumber),
            8 => Ok(Self::InvalidRepeatDeploy),
            9 => Ok(Self::InvalidParents),
            10 => Ok(Self::InvalidFollows),
            11 => Ok(Self::InvalidSequenceNumber),
            12 => Ok(Self::InvalidShardId),
            13 => Ok(Self::JustificationRegression),
            14 => Ok(Self::NeglectedInvalidBlock),
            15 => Ok(Self::NeglectedEquivocation),
            16 => Ok(Self::InvalidTransaction),
            17 => Ok(Self::InvalidBondsCache),
            18 => Ok(Self::InvalidEquivocationEvidence),
            19 => Ok(Self::InvalidBlockHash),
            20 => Ok(Self::UnauthorizedSlashDeploy),
            21 => Ok(Self::InvalidRejectedDeploy),
            22 => Ok(Self::ContainsExpiredDeploy),
            23 => Ok(Self::ContainsTimeExpiredDeploy),
            24 => Ok(Self::ContainsFutureDeploy),
            25 => Ok(Self::NotOfInterest),
            26 => Ok(Self::LowDeployCost),
            _ => Err(CertifiedAdmissionOutcomeError::UnknownRejectionReason(
                value,
            )),
        }
    }
}

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize
)]
pub enum CertifiedAdmissionDecision {
    Accepted,
    Rejected(AdmissionRejectionReason),
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize
)]
pub struct CertifiedAdmissionOutcome {
    #[serde(with = "shared::rust::serde_bytes")]
    block_hash: Bytes,
    protocol_version: i64,
    admission_schema_version: u32,
    #[serde(with = "shared::rust::serde_bytes")]
    ruleset_digest: Bytes,
    #[serde(with = "shared::rust::serde_bytes")]
    incoming_context_digest: Bytes,
    #[serde(with = "shared::rust::serde_bytes")]
    sender_authority_digest: Bytes,
    decision: CertifiedAdmissionDecision,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CertifiedAdmissionOutcomeError {
    #[error("admission outcome block hash must be {expected} bytes, got {actual}")]
    InvalidBlockHash { expected: usize, actual: usize },
    #[error("admission outcome ruleset digest must be {expected} bytes, got {actual}")]
    InvalidRulesetDigest { expected: usize, actual: usize },
    #[error("admission outcome context digest must be {expected} bytes, got {actual}")]
    InvalidContextDigest { expected: usize, actual: usize },
    #[error("admission outcome authority digest must be {expected} bytes, got {actual}")]
    InvalidAuthorityDigest { expected: usize, actual: usize },
    #[error("admission outcome uses unsupported schema {0}")]
    UnsupportedSchema(u32),
    #[error("admission outcome uses unsupported protocol version {0}")]
    UnsupportedProtocolVersion(i64),
    #[error("admission outcome ruleset digest does not match the compiled ruleset")]
    RulesetDigestMismatch,
    #[error("admission outcome block hash does not match the block")]
    BlockHashMismatch,
    #[error("admission outcome protocol version does not match the block")]
    ProtocolVersionMismatch,
    #[error("admission outcome context does not match sender authority")]
    ContextDigestMismatch,
    #[error("admission outcome does not match sender authority")]
    AuthorityDigestMismatch,
    #[error("admission outcome protobuf contains an unknown disposition {0}")]
    UnknownDisposition(u32),
    #[error("admission outcome protobuf contains an unknown rejection reason {0}")]
    UnknownRejectionReason(u32),
    #[error("accepted admission outcome must use rejection reason zero")]
    AcceptedWithRejectionReason,
    #[error("rejected admission outcome must use a nonzero rejection reason")]
    RejectedWithoutReason,
}

impl CertifiedAdmissionOutcome {
    pub fn accepted(
        block: &BlockMessage,
        sender_authority: &CertifiedSenderAuthority,
    ) -> Result<Self, CertifiedAdmissionOutcomeError> {
        Self::new(
            block,
            sender_authority,
            CertifiedAdmissionDecision::Accepted,
        )
    }

    pub fn rejected(
        block: &BlockMessage,
        sender_authority: &CertifiedSenderAuthority,
        reason: AdmissionRejectionReason,
    ) -> Result<Self, CertifiedAdmissionOutcomeError> {
        Self::new(
            block,
            sender_authority,
            CertifiedAdmissionDecision::Rejected(reason),
        )
    }

    fn new(
        block: &BlockMessage,
        sender_authority: &CertifiedSenderAuthority,
        decision: CertifiedAdmissionDecision,
    ) -> Result<Self, CertifiedAdmissionOutcomeError> {
        let outcome = Self {
            block_hash: block.block_hash.clone(),
            protocol_version: block.header.version,
            admission_schema_version: ADMISSION_SCHEMA_VERSION,
            ruleset_digest: admission_ruleset_digest(),
            incoming_context_digest: sender_authority.context_digest().clone(),
            sender_authority_digest: sender_authority.digest(),
            decision,
        };
        outcome.validate_for(block, sender_authority)?;
        Ok(outcome)
    }

    pub fn block_hash(&self) -> &Bytes { &self.block_hash }

    pub const fn protocol_version(&self) -> i64 { self.protocol_version }

    pub const fn admission_schema_version(&self) -> u32 { self.admission_schema_version }

    pub fn ruleset_digest(&self) -> &Bytes { &self.ruleset_digest }

    pub fn incoming_context_digest(&self) -> &Bytes { &self.incoming_context_digest }

    pub fn sender_authority_digest(&self) -> &Bytes { &self.sender_authority_digest }

    pub const fn decision(&self) -> CertifiedAdmissionDecision { self.decision }

    pub const fn is_accepted(&self) -> bool {
        matches!(self.decision, CertifiedAdmissionDecision::Accepted)
    }

    pub const fn is_rejected(&self) -> bool {
        matches!(self.decision, CertifiedAdmissionDecision::Rejected(_))
    }

    pub fn validate_for(
        &self,
        block: &BlockMessage,
        sender_authority: &CertifiedSenderAuthority,
    ) -> Result<(), CertifiedAdmissionOutcomeError> {
        self.validate_shape()?;
        if self.block_hash != block.block_hash {
            return Err(CertifiedAdmissionOutcomeError::BlockHashMismatch);
        }
        if self.protocol_version != block.header.version {
            return Err(CertifiedAdmissionOutcomeError::ProtocolVersionMismatch);
        }
        self.validate_authority(sender_authority)
    }

    pub fn validate_metadata(
        &self,
        block_hash: &Bytes,
        protocol_version: i64,
        sender_authority: &CertifiedSenderAuthority,
    ) -> Result<(), CertifiedAdmissionOutcomeError> {
        self.validate_shape()?;
        if self.block_hash != *block_hash {
            return Err(CertifiedAdmissionOutcomeError::BlockHashMismatch);
        }
        if self.protocol_version != protocol_version {
            return Err(CertifiedAdmissionOutcomeError::ProtocolVersionMismatch);
        }
        self.validate_authority(sender_authority)
    }

    fn validate_authority(
        &self,
        sender_authority: &CertifiedSenderAuthority,
    ) -> Result<(), CertifiedAdmissionOutcomeError> {
        if self.incoming_context_digest != *sender_authority.context_digest() {
            return Err(CertifiedAdmissionOutcomeError::ContextDigestMismatch);
        }
        if self.sender_authority_digest != sender_authority.digest() {
            return Err(CertifiedAdmissionOutcomeError::AuthorityDigestMismatch);
        }
        Ok(())
    }

    pub fn validate_shape(&self) -> Result<(), CertifiedAdmissionOutcomeError> {
        let digest_length = block_hash::LENGTH;
        if self.block_hash.len() != block_hash::LENGTH {
            return Err(CertifiedAdmissionOutcomeError::InvalidBlockHash {
                expected: block_hash::LENGTH,
                actual: self.block_hash.len(),
            });
        }
        if self.ruleset_digest.len() != digest_length {
            return Err(CertifiedAdmissionOutcomeError::InvalidRulesetDigest {
                expected: digest_length,
                actual: self.ruleset_digest.len(),
            });
        }
        if self.incoming_context_digest.len() != digest_length {
            return Err(CertifiedAdmissionOutcomeError::InvalidContextDigest {
                expected: digest_length,
                actual: self.incoming_context_digest.len(),
            });
        }
        if self.sender_authority_digest.len() != digest_length {
            return Err(CertifiedAdmissionOutcomeError::InvalidAuthorityDigest {
                expected: digest_length,
                actual: self.sender_authority_digest.len(),
            });
        }
        if self.admission_schema_version != ADMISSION_SCHEMA_VERSION {
            return Err(CertifiedAdmissionOutcomeError::UnsupportedSchema(
                self.admission_schema_version,
            ));
        }
        if self.protocol_version != CERTIFIED_ADMISSION_PROTOCOL_VERSION {
            return Err(CertifiedAdmissionOutcomeError::UnsupportedProtocolVersion(
                self.protocol_version,
            ));
        }
        if self.ruleset_digest != admission_ruleset_digest() {
            return Err(CertifiedAdmissionOutcomeError::RulesetDigestMismatch);
        }
        Ok(())
    }

    pub fn to_proto(&self) -> CertifiedAdmissionOutcomeProto {
        let (disposition, rejection_reason) = match self.decision {
            CertifiedAdmissionDecision::Accepted => (1, 0),
            CertifiedAdmissionDecision::Rejected(reason) => (2, reason as u32),
        };
        CertifiedAdmissionOutcomeProto {
            block_hash: self.block_hash.clone(),
            protocol_version: self.protocol_version,
            admission_schema_version: self.admission_schema_version,
            ruleset_digest: self.ruleset_digest.clone(),
            incoming_context_digest: self.incoming_context_digest.clone(),
            sender_authority_digest: self.sender_authority_digest.clone(),
            disposition,
            rejection_reason,
        }
    }

    pub fn from_proto(
        proto: CertifiedAdmissionOutcomeProto,
    ) -> Result<Self, CertifiedAdmissionOutcomeError> {
        let decision = match (proto.disposition, proto.rejection_reason) {
            (1, 0) => CertifiedAdmissionDecision::Accepted,
            (1, _) => return Err(CertifiedAdmissionOutcomeError::AcceptedWithRejectionReason),
            (2, 0) => return Err(CertifiedAdmissionOutcomeError::RejectedWithoutReason),
            (2, reason) => {
                CertifiedAdmissionDecision::Rejected(AdmissionRejectionReason::try_from(reason)?)
            }
            (disposition, _) => {
                return Err(CertifiedAdmissionOutcomeError::UnknownDisposition(
                    disposition,
                ))
            }
        };
        let outcome = Self {
            block_hash: proto.block_hash,
            protocol_version: proto.protocol_version,
            admission_schema_version: proto.admission_schema_version,
            ruleset_digest: proto.ruleset_digest,
            incoming_context_digest: proto.incoming_context_digest,
            sender_authority_digest: proto.sender_authority_digest,
            decision,
        };
        outcome.validate_shape()?;
        Ok(outcome)
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct BlockMetadata {
    #[serde(with = "shared::rust::serde_bytes")]
    pub block_hash: Bytes,
    #[serde(with = "shared::rust::serde_bytes")]
    pub post_state_hash: Bytes,
    #[serde(with = "shared::rust::serde_vec_bytes")]
    pub parents: Vec<Bytes>,
    #[serde(with = "shared::rust::serde_bytes")]
    pub sender: Bytes,
    pub justifications: Vec<Justification>,
    #[serde(with = "shared::rust::serde_btreemap_bytes_i64")]
    pub weight_map: BTreeMap<Bytes, i64>,
    #[serde(with = "serde_btreemap_bytes_bond_generation")]
    pub bond_generation_map: BTreeMap<Bytes, BondGeneration>,
    #[serde(with = "serde_btreeset_bytes")]
    pub active_validator_set: BTreeSet<Bytes>,
    pub block_number: i64,
    pub sequence_number: i32,
    pub admission_outcome: Option<CertifiedAdmissionOutcome>,
    pub directly_finalized: bool,
    pub finalized: bool,
    pub fault_tolerance_value: f32,
    pub successful_state_effect_indices: BTreeSet<u32>,
    pub rejected_state_effects: BTreeSet<StateEffectId>,
    pub protocol_version: i64,
    pub objective_equivocation_evidence_delta: Vec<ObjectiveEquivocationEvidence>,
    pub sender_authority: Option<CertifiedSenderAuthority>,
    pub finalized_floor_commitment: Option<FinalizedFloorCommitment>,
    pub admission_schema_version: u32,
    pub approved_genesis: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum BlockMetadataError {
    #[error("failed to decode block metadata protobuf: {0}")]
    Decode(String),
    #[error("block metadata uses unsupported admission schema {0}")]
    UnsupportedAdmissionSchema(u32),
    #[error("block metadata uses unsupported Casper protocol version {0}")]
    UnsupportedProtocolVersion(i64),
    #[error("block metadata contains an invalid bond generation: {0}")]
    InvalidBondGeneration(String),
    #[error("block metadata contains a malformed active-validator set")]
    InvalidActiveValidatorSet,
    #[error("block metadata post-state hash must be {expected} bytes, got {actual}")]
    InvalidPostStateHash { expected: usize, actual: usize },
    #[error("block metadata contains malformed objective evidence: {0}")]
    InvalidObjectiveEvidence(String),
    #[error("block metadata contains an invalid authority certificate: {0}")]
    InvalidAuthorityCertificate(#[from] CertifiedSenderAuthorityError),
    #[error("block metadata contains an invalid admission outcome: {0}")]
    InvalidAdmissionOutcome(#[from] CertifiedAdmissionOutcomeError),
    #[error("block metadata contains an invalid finalized-floor commitment: {0}")]
    InvalidFinalizedFloorCommitment(String),
    #[error("non-genesis block metadata is missing its authority certificate")]
    MissingAuthorityCertificate,
    #[error("non-genesis block metadata is missing its admission outcome")]
    MissingAdmissionOutcome,
    #[error("non-genesis block metadata is missing its finalized-floor commitment")]
    MissingFinalizedFloorCommitment,
    #[error("genesis block metadata must not contain a sender authority certificate")]
    UnexpectedGenesisAuthorityCertificate,
    #[error("genesis block metadata must not contain an admission outcome")]
    UnexpectedGenesisAdmissionOutcome,
    #[error("genesis block metadata must not contain a finalized-floor commitment")]
    UnexpectedGenesisFinalizedFloorCommitment,
    #[error("authority certificate does not match block metadata")]
    AuthorityCertificateMismatch,
    #[error("finalized-floor commitment does not match accepted sender authority")]
    FinalizedFloorAuthorityMismatch,
    #[error("rejected certified block metadata cannot be finalized")]
    InvalidBlockFinalized,
    #[error("approved genesis metadata has an invalid shape")]
    InvalidApprovedGenesis,
}

mod serde_btreemap_bytes_bond_generation {
    use std::collections::BTreeMap;

    use prost::bytes::Bytes;
    use serde::de::Error;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use crate::rust::bond_generation::BondGeneration;

    pub fn serialize<S>(
        map: &BTreeMap<Bytes, BondGeneration>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        map.iter()
            .map(|(validator, generation)| (validator.to_vec(), generation.get()))
            .collect::<Vec<_>>()
            .serialize(serializer)
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<BTreeMap<Bytes, BondGeneration>, D::Error>
    where D: Deserializer<'de> {
        Vec::<(Vec<u8>, i64)>::deserialize(deserializer)?
            .into_iter()
            .map(|(validator, generation)| {
                BondGeneration::try_from(generation)
                    .map(|generation| (Bytes::from(validator), generation))
                    .map_err(D::Error::custom)
            })
            .collect()
    }
}

mod serde_btreeset_bytes {
    use std::collections::BTreeSet;

    use prost::bytes::Bytes;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(set: &BTreeSet<Bytes>, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer {
        set.iter()
            .map(|value| value.to_vec())
            .collect::<Vec<_>>()
            .serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<BTreeSet<Bytes>, D::Error>
    where D: Deserializer<'de> {
        Ok(Vec::<Vec<u8>>::deserialize(deserializer)?
            .into_iter()
            .map(Bytes::from)
            .collect())
    }
}

impl PartialEq for BlockMetadata {
    fn eq(&self, other: &Self) -> bool {
        self.block_hash == other.block_hash
            && self.post_state_hash == other.post_state_hash
            && self.parents == other.parents
            && self.sender == other.sender
            && self.justifications == other.justifications
            && self.weight_map == other.weight_map
            && self.bond_generation_map == other.bond_generation_map
            && self.active_validator_set == other.active_validator_set
            && self.block_number == other.block_number
            && self.sequence_number == other.sequence_number
            && self.admission_outcome == other.admission_outcome
            && self.directly_finalized == other.directly_finalized
            && self.finalized == other.finalized
            && self.successful_state_effect_indices == other.successful_state_effect_indices
            && self.rejected_state_effects == other.rejected_state_effects
            && self.protocol_version == other.protocol_version
            && self.objective_equivocation_evidence_delta
                == other.objective_equivocation_evidence_delta
            && self.sender_authority == other.sender_authority
            && self.finalized_floor_commitment == other.finalized_floor_commitment
            && self.admission_schema_version == other.admission_schema_version
            && self.approved_genesis == other.approved_genesis
    }
}

impl Eq for BlockMetadata {}

impl std::hash::Hash for BlockMetadata {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.block_hash.hash(state);
        self.post_state_hash.hash(state);
        self.parents.hash(state);
        self.sender.hash(state);
        self.justifications.hash(state);
        self.weight_map.iter().for_each(|(k, v)| {
            k.hash(state);
            v.hash(state);
        });
        self.bond_generation_map.iter().for_each(|(k, v)| {
            k.hash(state);
            v.hash(state);
        });
        self.active_validator_set.hash(state);
        self.block_number.hash(state);
        self.sequence_number.hash(state);
        self.admission_outcome.hash(state);
        self.directly_finalized.hash(state);
        self.finalized.hash(state);
        self.successful_state_effect_indices.hash(state);
        self.rejected_state_effects.hash(state);
        self.protocol_version.hash(state);
        self.objective_equivocation_evidence_delta.hash(state);
        self.sender_authority.hash(state);
        self.finalized_floor_commitment.hash(state);
        self.admission_schema_version.hash(state);
        self.approved_genesis.hash(state);
    }
}

impl BlockMetadata {
    pub fn from_proto(proto: BlockMetadataInternal) -> Result<Self, BlockMetadataError> {
        let bond_generation_map = proto
            .bond_generations
            .into_iter()
            .map(|entry| {
                BondGeneration::try_from(entry.generation)
                    .map(|generation| (entry.validator, generation))
                    .map_err(|error| BlockMetadataError::InvalidBondGeneration(error.to_string()))
            })
            .collect::<Result<_, _>>()?;
        let sender_generation_claim = proto
            .sender_bond_generation
            .map(BondGeneration::try_from)
            .transpose()
            .map_err(|error| BlockMetadataError::InvalidBondGeneration(error.to_string()))?;
        if proto
            .active_validators
            .iter()
            .any(|validator| validator.len() != validator::LENGTH)
            || proto
                .active_validators
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err(BlockMetadataError::InvalidActiveValidatorSet);
        }
        let active_validator_set = proto.active_validators.into_iter().collect();
        let objective_equivocation_evidence_delta = proto
            .objective_equivocation_evidence_delta
            .into_iter()
            .map(ObjectiveEquivocationEvidence::from_proto)
            .collect::<Result<Vec<_>, _>>()
            .map_err(BlockMetadataError::InvalidObjectiveEvidence)?;
        let sender_authority = proto
            .sender_authority
            .map(CertifiedSenderAuthority::from_proto)
            .transpose()?;
        let admission_outcome = proto
            .admission_outcome
            .map(CertifiedAdmissionOutcome::from_proto)
            .transpose()?;
        let finalized_floor_commitment = proto
            .finalized_floor_commitment
            .map(FinalizedFloorCommitment::from_proto)
            .transpose()
            .map_err(BlockMetadataError::InvalidFinalizedFloorCommitment)?;
        if sender_generation_claim != sender_authority.as_ref().map(|cert| cert.generation()) {
            return Err(BlockMetadataError::AuthorityCertificateMismatch);
        }
        let metadata = BlockMetadata {
            block_hash: proto.block_hash,
            post_state_hash: proto.post_state_hash,
            parents: proto.parents,
            sender: proto.sender,
            justifications: proto
                .justifications
                .into_iter()
                .map(|j| Justification::from_proto(j))
                .collect(),
            weight_map: proto
                .bonds
                .into_iter()
                .map(|b| (b.validator.into(), b.stake))
                .collect(),
            bond_generation_map,
            active_validator_set,
            block_number: proto.block_num,
            sequence_number: proto.seq_num,
            admission_outcome,
            directly_finalized: proto.directly_finalized,
            finalized: proto.finalized,
            fault_tolerance_value: proto.fault_tolerance_value,
            successful_state_effect_indices: proto
                .successful_state_effect_indices
                .into_iter()
                .collect(),
            rejected_state_effects: proto
                .rejected_state_effects
                .into_iter()
                .map(StateEffectId::from_proto)
                .collect(),
            protocol_version: proto.protocol_version,
            objective_equivocation_evidence_delta,
            sender_authority,
            finalized_floor_commitment,
            admission_schema_version: proto.admission_schema_version,
            approved_genesis: proto.approved_genesis,
        };
        metadata.validate()?;
        Ok(metadata)
    }

    pub fn to_proto(&self) -> BlockMetadataInternal {
        BlockMetadataInternal {
            block_hash: self.block_hash.clone(),
            post_state_hash: self.post_state_hash.clone(),
            parents: self.parents.clone(),
            sender: self.sender.clone(),
            justifications: self.justifications.iter().map(|j| j.to_proto()).collect(),
            bonds: self
                .weight_map
                .iter()
                .map(|(v, s)| BondProto {
                    validator: v.clone(),
                    stake: *s,
                })
                .collect(),
            bond_generations: self
                .bond_generation_map
                .iter()
                .map(
                    |(validator, generation)| crate::casper::ValidatorBondGenerationProto {
                        validator: validator.clone(),
                        generation: generation.get(),
                    },
                )
                .collect(),
            active_validators: self.active_validator_set.iter().cloned().collect(),
            block_num: self.block_number,
            seq_num: self.sequence_number,
            directly_finalized: self.directly_finalized,
            finalized: self.finalized,
            fault_tolerance_value: self.fault_tolerance_value,
            successful_state_effect_indices: self
                .successful_state_effect_indices
                .iter()
                .copied()
                .collect(),
            rejected_state_effects: self
                .rejected_state_effects
                .iter()
                .map(StateEffectId::to_proto)
                .collect(),
            protocol_version: self.protocol_version,
            sender_bond_generation: self.sender_bond_generation().map(BondGeneration::get),
            objective_equivocation_evidence_delta: self
                .objective_equivocation_evidence_delta
                .iter()
                .map(ObjectiveEquivocationEvidence::to_proto)
                .collect(),
            sender_authority: self
                .sender_authority
                .as_ref()
                .map(CertifiedSenderAuthority::to_proto),
            admission_schema_version: self.admission_schema_version,
            approved_genesis: self.approved_genesis,
            admission_outcome: self
                .admission_outcome
                .as_ref()
                .map(CertifiedAdmissionOutcome::to_proto),
            finalized_floor_commitment: self
                .finalized_floor_commitment
                .as_ref()
                .map(FinalizedFloorCommitment::to_proto),
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> { self.to_proto().encode_to_vec() }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, BlockMetadataError> {
        let proto = BlockMetadataInternal::decode(bytes)
            .map_err(|error| BlockMetadataError::Decode(error.to_string()))?;
        Self::from_proto(proto)
    }

    fn bytes_ordering(left: &Bytes, right: &Bytes) -> Ordering { left.iter().cmp(right.iter()) }

    pub fn ordering_by_num(left: &BlockMetadata, right: &BlockMetadata) -> Ordering {
        match left.block_number.cmp(&right.block_number) {
            Ordering::Equal => Self::bytes_ordering(&left.block_hash, &right.block_hash),
            other => other,
        }
    }

    fn weight_map(state: &F1r3flyState) -> BTreeMap<Bytes, i64> {
        state
            .bonds
            .iter()
            .map(|b| (b.validator.clone(), b.stake))
            .collect()
    }

    pub fn from_block(
        b: &BlockMessage,
        directly_finalized: Option<bool>,
        finalized: Option<bool>,
    ) -> Self {
        let directly_finalized = directly_finalized.unwrap_or(false);
        let finalized = finalized.unwrap_or(false);
        Self {
            block_hash: b.block_hash.clone(),
            post_state_hash: b.body.state.post_state_hash.clone(),
            parents: b.header.parents_hash_list.clone(),
            sender: b.sender.clone(),
            justifications: b.justifications.clone(),
            weight_map: Self::weight_map(&b.body.state),
            bond_generation_map: b
                .body
                .state
                .bond_generations
                .iter()
                .map(|entry| (entry.validator.clone(), entry.generation))
                .collect(),
            active_validator_set: b.body.state.active_validators.iter().cloned().collect(),
            block_number: b.body.state.block_number,
            sequence_number: b.seq_num,
            admission_outcome: None,
            directly_finalized,
            finalized,
            fault_tolerance_value: 0.0,
            successful_state_effect_indices: b
                .body
                .deploys
                .iter()
                .enumerate()
                .filter(|(_, deploy)| !deploy.is_failed)
                .map(|(index, _)| u32::try_from(index).expect("block deploy index must fit in u32"))
                .chain(
                    b.body
                        .system_deploys
                        .iter()
                        .enumerate()
                        .filter(|(_, deploy)| {
                            matches!(deploy, ProcessedSystemDeploy::Succeeded { .. })
                        })
                        .map(|(index, _)| {
                            u32::try_from(b.body.deploys.len() + index)
                                .expect("block system deploy index must fit in u32")
                        }),
                )
                .collect(),
            rejected_state_effects: b.body.rejected_state_effects.iter().cloned().collect(),
            protocol_version: b.header.version,
            objective_equivocation_evidence_delta: b
                .header
                .objective_equivocation_evidence_delta
                .clone(),
            sender_authority: None,
            finalized_floor_commitment: b.header.finalized_floor.clone(),
            admission_schema_version: ADMISSION_SCHEMA_VERSION,
            approved_genesis: false,
        }
    }

    pub fn from_certified_block(
        b: &BlockMessage,
        directly_finalized: Option<bool>,
        finalized: Option<bool>,
        sender_authority: &CertifiedSenderAuthority,
        admission_outcome: &CertifiedAdmissionOutcome,
    ) -> Result<Self, BlockMetadataError> {
        sender_authority.validate_for(b)?;
        admission_outcome.validate_for(b, sender_authority)?;
        let mut metadata = Self::from_block(b, directly_finalized, finalized);
        metadata.sender_authority = Some(sender_authority.clone());
        metadata.admission_outcome = Some(admission_outcome.clone());
        metadata.validate()?;
        Ok(metadata)
    }

    pub fn from_approved_genesis(b: &BlockMessage) -> Result<Self, BlockMetadataError> {
        let mut metadata = Self::from_block(b, Some(true), Some(true));
        metadata.approved_genesis = true;
        metadata.validate()?;
        Ok(metadata)
    }

    pub fn sender_bond_generation(&self) -> Option<BondGeneration> {
        self.sender_authority
            .as_ref()
            .map(CertifiedSenderAuthority::generation)
    }

    pub fn is_accepted(&self) -> bool {
        self.approved_genesis
            || self
                .admission_outcome
                .as_ref()
                .is_some_and(CertifiedAdmissionOutcome::is_accepted)
    }

    pub fn is_rejected(&self) -> bool {
        self.admission_outcome
            .as_ref()
            .is_some_and(CertifiedAdmissionOutcome::is_rejected)
    }

    pub fn validate(&self) -> Result<(), BlockMetadataError> {
        if self.post_state_hash.len() != block_hash::LENGTH {
            return Err(BlockMetadataError::InvalidPostStateHash {
                expected: block_hash::LENGTH,
                actual: self.post_state_hash.len(),
            });
        }
        if self.admission_schema_version != ADMISSION_SCHEMA_VERSION {
            return Err(BlockMetadataError::UnsupportedAdmissionSchema(
                self.admission_schema_version,
            ));
        }
        if self.protocol_version != CERTIFIED_ADMISSION_PROTOCOL_VERSION {
            return Err(BlockMetadataError::UnsupportedProtocolVersion(
                self.protocol_version,
            ));
        }
        if self.is_rejected() && (self.directly_finalized || self.finalized) {
            return Err(BlockMetadataError::InvalidBlockFinalized);
        }
        if self.approved_genesis && self.finalized_floor_commitment.is_some() {
            return Err(BlockMetadataError::UnexpectedGenesisFinalizedFloorCommitment);
        }
        if self.approved_genesis
            && (!self.parents.is_empty()
                || self.block_number != 0
                || self.sequence_number != 0
                || self.admission_outcome.is_some())
        {
            return Err(BlockMetadataError::InvalidApprovedGenesis);
        }
        match (
            &self.sender_authority,
            &self.admission_outcome,
            self.approved_genesis,
        ) {
            (None, None, true) => Ok(()),
            (Some(_), _, true) => Err(BlockMetadataError::UnexpectedGenesisAuthorityCertificate),
            (_, Some(_), true) => Err(BlockMetadataError::UnexpectedGenesisAdmissionOutcome),
            (None, _, false) => Err(BlockMetadataError::MissingAuthorityCertificate),
            (_, None, false) => Err(BlockMetadataError::MissingAdmissionOutcome),
            (Some(certificate), Some(outcome), false) => {
                let commitment = self
                    .finalized_floor_commitment
                    .as_ref()
                    .ok_or(BlockMetadataError::MissingFinalizedFloorCommitment)?;
                commitment
                    .validate_shape()
                    .map_err(BlockMetadataError::InvalidFinalizedFloorCommitment)?;
                certificate.validate_shape()?;
                if certificate.block_hash() != &self.block_hash
                    || certificate.protocol_version() != self.protocol_version
                    || certificate.sender() != &self.sender
                {
                    return Err(BlockMetadataError::AuthorityCertificateMismatch);
                }
                outcome.validate_metadata(&self.block_hash, self.protocol_version, certificate)?;
                if outcome.is_accepted()
                    && (certificate.authority_floor_hash() != &commitment.floor_hash
                        || certificate.authority_floor_post_state_hash()
                            != &commitment.floor_post_state_hash
                        || certificate.context_digest() != &commitment.authority_context_digest)
                {
                    return Err(BlockMetadataError::FinalizedFloorAuthorityMismatch);
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;
    use crypto::rust::signatures::signed::Signed;

    use super::*;
    use crate::rhoapi::PCost;
    use crate::rust::casper::protocol::casper_message::{
        Body, DeployAdmissionStatus, DeployData, Header, ProcessedDeploy, SystemDeployData,
    };

    fn processed_deploy(is_failed: bool) -> ProcessedDeploy {
        let algorithm: Box<dyn SignaturesAlg> = Box::new(Secp256k1);
        let (private_key, _) = algorithm.new_key_pair();
        let deploy = Signed::create(
            DeployData {
                term: "Nil".to_string(),
                time_stamp: 0,
                valid_after_block_number: 0,
                shard_id: "root".to_string(),
                expiration_timestamp: None,
                authority_presentations: Vec::new(),
            },
            algorithm,
            private_key,
        )
        .unwrap();
        ProcessedDeploy {
            deploy,
            cost: PCost { cost: 0 },
            deploy_log: Vec::new(),
            is_failed,
            system_deploy_error: None,
            cosigners: Vec::new(),
            cosigner_threshold: 0,
            pre_state_hash: Bytes::new(),
            post_state_hash: Bytes::new(),
            authority_funding_certificate: None,
            authority_cost_witness: None,
            admission_status: DeployAdmissionStatus::Executed,
        }
    }

    fn authority_block() -> BlockMessage {
        BlockMessage {
            block_hash: Bytes::from(vec![1; block_hash::LENGTH]),
            header: Header {
                parents_hash_list: vec![Bytes::from(vec![3; block_hash::LENGTH])],
                timestamp: 0,
                version: CERTIFIED_ADMISSION_PROTOCOL_VERSION,
                extra_bytes: Bytes::new(),
                sender_bond_generation: Some(BondGeneration::GENESIS),
                objective_equivocation_evidence_delta: Vec::new(),
                finalized_floor: Some(FinalizedFloorCommitment {
                    floor_hash: Bytes::from(vec![10; block_hash::LENGTH]),
                    floor_post_state_hash: Bytes::from(vec![11; block_hash::LENGTH]),
                    certificate_digest: Bytes::from(vec![13; block_hash::LENGTH]),
                    authority_context_digest: Bytes::from(vec![12; block_hash::LENGTH]),
                }),
            },
            body: Body {
                state: F1r3flyState {
                    pre_state_hash: Bytes::from(vec![4; block_hash::LENGTH]),
                    post_state_hash: Bytes::from(vec![5; block_hash::LENGTH]),
                    bonds: Vec::new(),
                    bond_generations: Vec::new(),
                    active_validators: Vec::new(),
                    block_number: 1,
                },
                deploys: Vec::new(),
                rejected_deploys: Vec::new(),
                rejected_state_effects: Vec::new(),
                system_deploys: Vec::new(),
                extra_bytes: Bytes::new(),
            },
            justifications: Vec::new(),
            sender: Bytes::from(vec![2; validator::LENGTH]),
            seq_num: 1,
            sig: Bytes::new(),
            sig_algorithm: String::new(),
            shard_id: "root".to_string(),
            extra_bytes: Bytes::new(),
            finalized_floor_certificate: None,
        }
    }

    fn authority_certificate(
        block: &BlockMessage,
        stake: i64,
    ) -> Result<CertifiedSenderAuthority, CertifiedSenderAuthorityError> {
        CertifiedSenderAuthority::new(
            block,
            Bytes::from(vec![10; block_hash::LENGTH]),
            Bytes::from(vec![11; block_hash::LENGTH]),
            Bytes::from(vec![12; block_hash::LENGTH]),
            BondGeneration::GENESIS,
            stake,
        )
    }

    #[test]
    fn block_metadata_records_only_successful_execution_effects_and_round_trips() {
        let rejected = StateEffectId {
            source_block_hash: Bytes::from_static(b"source"),
            execution_index: 4,
        };
        let block = BlockMessage {
            block_hash: Bytes::from(vec![1; block_hash::LENGTH]),
            header: Header {
                parents_hash_list: vec![Bytes::from_static(b"parent")],
                timestamp: 0,
                version: CERTIFIED_ADMISSION_PROTOCOL_VERSION,
                extra_bytes: Bytes::new(),
                sender_bond_generation: Some(BondGeneration::GENESIS),
                objective_equivocation_evidence_delta: Vec::new(),
                finalized_floor: Some(FinalizedFloorCommitment {
                    floor_hash: Bytes::from(vec![10; block_hash::LENGTH]),
                    floor_post_state_hash: Bytes::from(vec![11; block_hash::LENGTH]),
                    certificate_digest: Bytes::from(vec![13; block_hash::LENGTH]),
                    authority_context_digest: Bytes::from(vec![12; block_hash::LENGTH]),
                }),
            },
            body: Body {
                state: F1r3flyState {
                    pre_state_hash: Bytes::from(vec![3; block_hash::LENGTH]),
                    post_state_hash: Bytes::from(vec![4; block_hash::LENGTH]),
                    bonds: Vec::new(),
                    bond_generations: Vec::new(),
                    active_validators: Vec::new(),
                    block_number: 9,
                },
                deploys: vec![processed_deploy(false), processed_deploy(true)],
                rejected_deploys: Vec::new(),
                rejected_state_effects: vec![rejected.clone()],
                system_deploys: vec![
                    ProcessedSystemDeploy::Succeeded {
                        event_list: Vec::new(),
                        system_deploy: SystemDeployData::Empty,
                        pre_state_hash: Bytes::new(),
                        post_state_hash: Bytes::new(),
                    },
                    ProcessedSystemDeploy::Failed {
                        event_list: Vec::new(),
                        error_msg: "failed".to_string(),
                        pre_state_hash: Bytes::new(),
                        post_state_hash: Bytes::new(),
                    },
                ],
                extra_bytes: Bytes::new(),
            },
            justifications: Vec::new(),
            sender: Bytes::from(vec![2; validator::LENGTH]),
            seq_num: 3,
            sig: Bytes::new(),
            sig_algorithm: String::new(),
            shard_id: "root".to_string(),
            extra_bytes: Bytes::new(),
            finalized_floor_certificate: None,
        };

        let certificate = authority_certificate(&block, 1).unwrap();
        let outcome = CertifiedAdmissionOutcome::accepted(&block, &certificate).unwrap();
        let metadata = BlockMetadata::from_certified_block(
            &block,
            Some(true),
            Some(true),
            &certificate,
            &outcome,
        )
        .unwrap();
        assert_eq!(
            metadata.successful_state_effect_indices,
            BTreeSet::from([0, 2])
        );
        assert_eq!(metadata.rejected_state_effects, BTreeSet::from([rejected]));
        assert_eq!(
            metadata.protocol_version,
            CERTIFIED_ADMISSION_PROTOCOL_VERSION
        );
        assert_eq!(
            BlockMetadata::from_bytes(&metadata.to_bytes()).unwrap(),
            metadata
        );
    }

    #[test]
    fn persisted_non_genesis_metadata_requires_a_block_bound_authority_certificate() {
        let block = authority_block();

        let metadata = BlockMetadata::from_block(&block, None, None);
        assert_eq!(
            metadata.validate(),
            Err(BlockMetadataError::MissingAuthorityCertificate)
        );

        let certificate = authority_certificate(&block, 10).unwrap();
        let outcome = CertifiedAdmissionOutcome::accepted(&block, &certificate).unwrap();
        let metadata =
            BlockMetadata::from_certified_block(&block, None, None, &certificate, &outcome)
                .unwrap();
        assert_eq!(metadata.sender_authority, Some(certificate));
        assert_eq!(metadata.admission_outcome, Some(outcome));
        assert_eq!(metadata.validate(), Ok(()));
    }

    #[test]
    fn accepted_metadata_binds_the_exact_signed_floor_authority_context() {
        let block = authority_block();
        let certificate = authority_certificate(&block, 10).unwrap();
        let outcome = CertifiedAdmissionOutcome::accepted(&block, &certificate).unwrap();
        let metadata =
            BlockMetadata::from_certified_block(&block, None, None, &certificate, &outcome)
                .unwrap();

        for (index, mut tampered) in [metadata.clone(), metadata.clone(), metadata.clone()]
            .into_iter()
            .enumerate()
        {
            let commitment = tampered.finalized_floor_commitment.as_mut().unwrap();
            match index {
                0 => commitment.floor_hash = Bytes::from(vec![20; block_hash::LENGTH]),
                1 => commitment.floor_post_state_hash = Bytes::from(vec![21; block_hash::LENGTH]),
                2 => {
                    commitment.authority_context_digest = Bytes::from(vec![22; block_hash::LENGTH])
                }
                _ => unreachable!(),
            }
            assert_eq!(
                tampered.validate(),
                Err(BlockMetadataError::FinalizedFloorAuthorityMismatch)
            );
        }

        let mut missing = metadata;
        missing.finalized_floor_commitment = None;
        assert_eq!(
            missing.validate(),
            Err(BlockMetadataError::MissingFinalizedFloorCommitment)
        );
    }

    #[test]
    fn approved_genesis_rejects_a_finalized_floor_commitment_explicitly() {
        let mut block = authority_block();
        block.header.parents_hash_list.clear();
        block.header.finalized_floor = None;
        block.body.state.block_number = 0;
        block.seq_num = 0;
        let mut metadata = BlockMetadata::from_approved_genesis(&block).unwrap();
        metadata.finalized_floor_commitment = Some(FinalizedFloorCommitment {
            floor_hash: Bytes::from(vec![1; block_hash::LENGTH]),
            floor_post_state_hash: Bytes::from(vec![2; block_hash::LENGTH]),
            certificate_digest: Bytes::from(vec![3; block_hash::LENGTH]),
            authority_context_digest: Bytes::from(vec![4; block_hash::LENGTH]),
        });
        assert_eq!(
            metadata.validate(),
            Err(BlockMetadataError::UnexpectedGenesisFinalizedFloorCommitment)
        );
    }

    #[test]
    fn persisted_authority_certificate_rejects_wrong_context_and_stake() {
        let block = authority_block();
        let certificate = authority_certificate(&block, 10).unwrap();

        assert_eq!(
            certificate.validate_context(
                &Bytes::from(vec![9; block_hash::LENGTH]),
                &Bytes::from(vec![11; block_hash::LENGTH]),
                &Bytes::from(vec![12; block_hash::LENGTH]),
                BondGeneration::GENESIS,
                10,
            ),
            Err(CertifiedSenderAuthorityError::AuthorityFloorMismatch)
        );
        assert_eq!(
            certificate.validate_context(
                &Bytes::from(vec![10; block_hash::LENGTH]),
                &Bytes::from(vec![11; block_hash::LENGTH]),
                &Bytes::from(vec![12; block_hash::LENGTH]),
                BondGeneration::GENESIS,
                11,
            ),
            Err(CertifiedSenderAuthorityError::StakeMismatch)
        );
        assert_eq!(
            authority_certificate(&block, 0),
            Err(CertifiedSenderAuthorityError::InvalidStake(0))
        );
    }

    #[test]
    fn certified_admission_outcome_binds_block_context_authority_ruleset_and_decision() {
        let block = authority_block();
        let certificate = authority_certificate(&block, 10).unwrap();
        let accepted = CertifiedAdmissionOutcome::accepted(&block, &certificate).unwrap();
        let rejected = CertifiedAdmissionOutcome::rejected(
            &block,
            &certificate,
            AdmissionRejectionReason::InvalidTransaction,
        )
        .unwrap();

        assert_eq!(
            CertifiedAdmissionOutcome::from_proto(accepted.to_proto()).unwrap(),
            accepted
        );
        assert_eq!(
            CertifiedAdmissionOutcome::from_proto(rejected.to_proto()).unwrap(),
            rejected
        );

        let mut wrong_block = accepted.to_proto();
        wrong_block.block_hash = Bytes::from(vec![99; block_hash::LENGTH]);
        assert_eq!(
            CertifiedAdmissionOutcome::from_proto(wrong_block)
                .unwrap()
                .validate_for(&block, &certificate),
            Err(CertifiedAdmissionOutcomeError::BlockHashMismatch)
        );

        let mut wrong_context = accepted.to_proto();
        wrong_context.incoming_context_digest = Bytes::from(vec![98; block_hash::LENGTH]);
        assert_eq!(
            CertifiedAdmissionOutcome::from_proto(wrong_context)
                .unwrap()
                .validate_for(&block, &certificate),
            Err(CertifiedAdmissionOutcomeError::ContextDigestMismatch)
        );

        let mut wrong_authority = accepted.to_proto();
        wrong_authority.sender_authority_digest = Bytes::from(vec![97; block_hash::LENGTH]);
        assert_eq!(
            CertifiedAdmissionOutcome::from_proto(wrong_authority)
                .unwrap()
                .validate_for(&block, &certificate),
            Err(CertifiedAdmissionOutcomeError::AuthorityDigestMismatch)
        );

        let mut wrong_ruleset = accepted.to_proto();
        wrong_ruleset.ruleset_digest = Bytes::from(vec![96; block_hash::LENGTH]);
        assert_eq!(
            CertifiedAdmissionOutcome::from_proto(wrong_ruleset),
            Err(CertifiedAdmissionOutcomeError::RulesetDigestMismatch)
        );

        let mut wrong_schema = accepted.to_proto();
        wrong_schema.admission_schema_version = ADMISSION_SCHEMA_VERSION + 1;
        assert_eq!(
            CertifiedAdmissionOutcome::from_proto(wrong_schema),
            Err(CertifiedAdmissionOutcomeError::UnsupportedSchema(
                ADMISSION_SCHEMA_VERSION + 1
            ))
        );

        let mut unsupported_protocol = accepted.to_proto();
        unsupported_protocol.protocol_version = CERTIFIED_ADMISSION_PROTOCOL_VERSION - 1;
        assert_eq!(
            CertifiedAdmissionOutcome::from_proto(unsupported_protocol),
            Err(CertifiedAdmissionOutcomeError::UnsupportedProtocolVersion(
                CERTIFIED_ADMISSION_PROTOCOL_VERSION - 1
            ))
        );

        let mut wrong_protocol_block = block.clone();
        wrong_protocol_block.header.version = CERTIFIED_ADMISSION_PROTOCOL_VERSION - 1;
        assert_eq!(
            accepted.validate_for(&wrong_protocol_block, &certificate),
            Err(CertifiedAdmissionOutcomeError::ProtocolVersionMismatch)
        );

        let mut accepted_with_reason = accepted.to_proto();
        accepted_with_reason.rejection_reason = AdmissionRejectionReason::InvalidParents as u32;
        assert_eq!(
            CertifiedAdmissionOutcome::from_proto(accepted_with_reason),
            Err(CertifiedAdmissionOutcomeError::AcceptedWithRejectionReason)
        );

        let mut rejected_without_reason = rejected.to_proto();
        rejected_without_reason.rejection_reason = 0;
        assert_eq!(
            CertifiedAdmissionOutcome::from_proto(rejected_without_reason),
            Err(CertifiedAdmissionOutcomeError::RejectedWithoutReason)
        );
    }

    #[test]
    fn every_stable_admission_rejection_code_round_trips() {
        for code in 1..=26 {
            let reason = AdmissionRejectionReason::try_from(code).unwrap();
            assert_eq!(reason as u32, code);
        }
        assert_eq!(
            AdmissionRejectionReason::try_from(0),
            Err(CertifiedAdmissionOutcomeError::UnknownRejectionReason(0))
        );
        assert_eq!(
            AdmissionRejectionReason::try_from(27),
            Err(CertifiedAdmissionOutcomeError::UnknownRejectionReason(27))
        );
    }

    #[test]
    fn rejected_certified_metadata_cannot_be_finalized() {
        let block = authority_block();
        let certificate = authority_certificate(&block, 10).unwrap();
        let outcome = CertifiedAdmissionOutcome::rejected(
            &block,
            &certificate,
            AdmissionRejectionReason::InvalidTransaction,
        )
        .unwrap();
        assert_eq!(
            BlockMetadata::from_certified_block(
                &block,
                Some(true),
                Some(true),
                &certificate,
                &outcome,
            ),
            Err(BlockMetadataError::InvalidBlockFinalized)
        );
    }
}
