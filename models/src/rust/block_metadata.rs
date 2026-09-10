// See models/src/main/scala/coop/rchain/models/BlockMetadata.scala

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use crypto::rust::hash::blake2b256::Blake2b256;
use prost::bytes::Bytes;
use prost::Message;

use super::casper::protocol::casper_message::{
    BlockMessage, F1r3flyState, FinalizedFloorCommitment, Justification,
    ObjectiveEquivocationEvidence, ProcessedSystemDeploy, StateEffectId, SystemDeployData,
};
use crate::casper::{
    BlockMetadataInternal, BondProto, CertifiedAdmissionOutcomeProto,
    CertifiedSenderAuthorityProto, CertifiedSettledHistoryAdmissionProto,
};
use crate::rust::bond_generation::BondGeneration;
use crate::rust::{block_hash, validator};

pub const ADMISSION_SCHEMA_VERSION: u32 = 15;
pub const CERTIFIED_ADMISSION_PROTOCOL_VERSION: i64 = 6;
pub const STATE_EFFECT_PROVENANCE_PROTOCOL_VERSION: i64 = 3;
pub const APPLIED_STATE_EFFECTS_PROTOCOL_VERSION: i64 = 6;
const ADMISSION_RULESET_DOMAIN: &str = "f1r3fly-certified-admission-v15";
const ADMISSION_RULESET_SUBSYSTEMS: &str = "finalization-ledger:atomic-rooted-hash-chain-v2|finalized-floor:durable-exact-state-occurrence-v2|certificate-cache:exact-state-candidate-v2|candidate-authority-context:signed-exact-v1|certificate-sidecar:manifest-digests-and-counts-v2|settled-history:durable-citation-ticket-v1";
static ADMISSION_RULESET_MANIFEST: LazyLock<String> = LazyLock::new(|| {
    let mut manifest = format!("{ADMISSION_RULESET_DOMAIN}|0:accepted");
    for reason in AdmissionRejectionReason::ALL {
        manifest.push_str(&format!("|{}:{}", reason as u32, reason.manifest_label()));
    }
    manifest.push('|');
    manifest.push_str(ADMISSION_RULESET_SUBSYSTEMS);
    manifest
});
static ADMISSION_RULESET_DIGEST: LazyLock<Bytes> =
    LazyLock::new(|| Blake2b256::hash(ADMISSION_RULESET_MANIFEST.as_bytes().to_vec()).into());

pub fn admission_ruleset_manifest() -> &'static str { ADMISSION_RULESET_MANIFEST.as_str() }

pub fn admission_ruleset_digest() -> Bytes { ADMISSION_RULESET_DIGEST.clone() }

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
    Debug,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize
)]
pub struct CertifiedSettledHistoryAdmission {
    #[serde(with = "shared::rust::serde_bytes")]
    target_block_hash: Bytes,
    target_protocol_version: i64,
    target_block_height: i64,
    #[serde(with = "shared::rust::serde_bytes")]
    target_sender: Bytes,
    target_generation: BondGeneration,
    #[serde(with = "shared::rust::serde_bytes")]
    anchor_block_hash: Bytes,
    anchor_block_height: i64,
    #[serde(with = "shared::rust::serde_bytes")]
    anchor_post_state_hash: Bytes,
    #[serde(with = "shared::rust::serde_bytes")]
    citer_block_hash: Bytes,
    citer_protocol_version: i64,
    #[serde(with = "shared::rust::serde_bytes")]
    citer_sender: Bytes,
    citer_generation: BondGeneration,
    citer_stake: i64,
    admission_schema_version: u32,
    #[serde(with = "shared::rust::serde_bytes")]
    ruleset_digest: Bytes,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CertifiedSettledHistoryAdmissionError {
    #[error("settled-history target hash must be {expected} bytes, got {actual}")]
    InvalidTargetHash { expected: usize, actual: usize },
    #[error("settled-history target sender must be {expected} bytes, got {actual}")]
    InvalidTargetSender { expected: usize, actual: usize },
    #[error("settled-history anchor hash must be {expected} bytes, got {actual}")]
    InvalidAnchorHash { expected: usize, actual: usize },
    #[error("settled-history anchor post-state hash must be {expected} bytes, got {actual}")]
    InvalidAnchorPostStateHash { expected: usize, actual: usize },
    #[error("settled-history citer hash must be {expected} bytes, got {actual}")]
    InvalidCiterHash { expected: usize, actual: usize },
    #[error("settled-history citer sender must be {expected} bytes, got {actual}")]
    InvalidCiterSender { expected: usize, actual: usize },
    #[error("settled-history ruleset digest must be {expected} bytes, got {actual}")]
    InvalidRulesetDigest { expected: usize, actual: usize },
    #[error("settled-history proof target does not match the block")]
    TargetMismatch,
    #[error("settled-history proof uses unsupported target protocol {0}")]
    UnsupportedTargetProtocol(i64),
    #[error("settled-history proof uses unsupported citer protocol {0}")]
    UnsupportedCiterProtocol(i64),
    #[error("settled-history proof uses unsupported admission schema {0}")]
    UnsupportedSchema(u32),
    #[error("settled-history proof ruleset digest does not match the compiled ruleset")]
    RulesetDigestMismatch,
    #[error("settled-history proof anchor height must be positive")]
    InvalidAnchorHeight,
    #[error("settled-history proof target is above its anchor")]
    TargetAboveAnchor,
    #[error("settled-history proof target height must be nonnegative")]
    InvalidTargetHeight,
    #[error("settled-history proof citer stake must be positive, got {0}")]
    InvalidCiterStake(i64),
    #[error("settled-history target is missing its bond generation")]
    MissingTargetGeneration,
    #[error("settled-history proof target generation does not match the block")]
    TargetGenerationMismatch,
    #[error("settled-history proof contains an invalid target generation: {0}")]
    InvalidTargetGeneration(String),
    #[error("settled-history proof contains an invalid citer generation: {0}")]
    InvalidCiterGeneration(String),
    #[error("settled-history citer does not reference the target")]
    MissingCitation,
    #[error("settled-history citer is not bonded at the anchor")]
    CiterNotBonded,
    #[error("settled-history citer generation does not match the anchor")]
    CiterGenerationMismatch,
    #[error("settled-history target content hash is invalid")]
    InvalidTargetContentHash,
    #[error("settled-history target signature is invalid")]
    InvalidTargetSignature,
    #[error("settled-history anchor content hash is invalid")]
    InvalidAnchorContentHash,
    #[error("settled-history citer content hash is invalid")]
    InvalidCiterContentHash,
    #[error("settled-history citer signature is invalid")]
    InvalidCiterSignature,
    #[error("settled-history record does not match its validated evidence")]
    EvidenceMismatch,
}

impl CertifiedSettledHistoryAdmission {
    pub fn new(
        target: &BlockMessage,
        anchor: &BlockMessage,
        citer: &BlockMessage,
        citer_generation: BondGeneration,
        citer_stake: i64,
    ) -> Result<Self, CertifiedSettledHistoryAdmissionError> {
        if !Self::cites(citer, &target.block_hash) {
            return Err(CertifiedSettledHistoryAdmissionError::MissingCitation);
        }
        let anchor_stake = anchor
            .body
            .state
            .bonds
            .iter()
            .find(|bond| bond.validator == citer.sender && bond.stake > 0)
            .map(|bond| bond.stake)
            .ok_or(CertifiedSettledHistoryAdmissionError::CiterNotBonded)?;
        let anchor_generation = anchor
            .body
            .state
            .bond_generations
            .iter()
            .find(|entry| entry.validator == citer.sender)
            .map(|entry| entry.generation)
            .ok_or(CertifiedSettledHistoryAdmissionError::CiterNotBonded)?;
        if anchor_stake != citer_stake
            || anchor_generation != citer_generation
            || citer.header.sender_bond_generation != Some(citer_generation)
        {
            return Err(CertifiedSettledHistoryAdmissionError::CiterGenerationMismatch);
        }
        let target_generation = target
            .header
            .sender_bond_generation
            .ok_or(CertifiedSettledHistoryAdmissionError::MissingTargetGeneration)?;
        let proof = Self {
            target_block_hash: target.block_hash.clone(),
            target_protocol_version: target.header.version,
            target_block_height: target.body.state.block_number,
            target_sender: target.sender.clone(),
            target_generation,
            anchor_block_hash: anchor.block_hash.clone(),
            anchor_block_height: anchor.body.state.block_number,
            anchor_post_state_hash: anchor.body.state.post_state_hash.clone(),
            citer_block_hash: citer.block_hash.clone(),
            citer_protocol_version: citer.header.version,
            citer_sender: citer.sender.clone(),
            citer_generation,
            citer_stake,
            admission_schema_version: ADMISSION_SCHEMA_VERSION,
            ruleset_digest: admission_ruleset_digest(),
        };
        proof.validate_for(target)?;
        Ok(proof)
    }

    fn cites(citer: &BlockMessage, target: &Bytes) -> bool {
        citer
            .header
            .parents_hash_list
            .iter()
            .any(|hash| hash == target)
            || citer
                .justifications
                .iter()
                .any(|justification| justification.latest_block_hash == target)
            || citer
                .body
                .system_deploys
                .iter()
                .filter_map(|deploy| match deploy {
                    ProcessedSystemDeploy::Succeeded {
                        system_deploy:
                            SystemDeployData::Slash {
                                invalid_block_hash,
                                equivocation_block_hash,
                                ..
                            },
                        ..
                    } => Some((invalid_block_hash, equivocation_block_hash.as_ref())),
                    _ => None,
                })
                .any(|(first, second)| first == target || second == Some(target))
            || citer
                .header
                .objective_equivocation_evidence_delta
                .iter()
                .any(|evidence| {
                    evidence.first_block_hash == target || evidence.second_block_hash == target
                })
            || citer
                .finalized_floor_certificate
                .as_ref()
                .is_some_and(|certificate| {
                    certificate
                        .exact_latest_messages
                        .values()
                        .any(|hash| hash.0 == target)
                        || certificate.predecessor_floor_hash.0 == target
                        || certificate.predecessor_certificate_block_hash.0 == target
                        || certificate.target_floor_hash.0 == target
                })
    }

    pub fn validate_for(
        &self,
        target: &BlockMessage,
    ) -> Result<(), CertifiedSettledHistoryAdmissionError> {
        self.validate_shape()?;
        if self.target_block_hash != target.block_hash
            || self.target_protocol_version != target.header.version
            || self.target_sender != target.sender
        {
            return Err(CertifiedSettledHistoryAdmissionError::TargetMismatch);
        }
        if target.header.sender_bond_generation != Some(self.target_generation) {
            return Err(CertifiedSettledHistoryAdmissionError::TargetGenerationMismatch);
        }
        if target.body.state.block_number != self.target_block_height {
            return Err(CertifiedSettledHistoryAdmissionError::TargetMismatch);
        }
        if self.target_block_height < 0 {
            return Err(CertifiedSettledHistoryAdmissionError::InvalidTargetHeight);
        }
        if self.target_block_height > self.anchor_block_height {
            return Err(CertifiedSettledHistoryAdmissionError::TargetAboveAnchor);
        }
        Ok(())
    }

    pub fn validate_metadata(
        &self,
        block_hash: &Bytes,
        protocol_version: i64,
        sender: &Bytes,
    ) -> Result<(), CertifiedSettledHistoryAdmissionError> {
        self.validate_shape()?;
        if self.target_block_hash != *block_hash
            || self.target_protocol_version != protocol_version
            || self.target_sender != *sender
        {
            return Err(CertifiedSettledHistoryAdmissionError::TargetMismatch);
        }
        Ok(())
    }

    pub fn validate_shape(&self) -> Result<(), CertifiedSettledHistoryAdmissionError> {
        let hash_length = block_hash::LENGTH;
        if self.target_block_hash.len() != hash_length {
            return Err(CertifiedSettledHistoryAdmissionError::InvalidTargetHash {
                expected: hash_length,
                actual: self.target_block_hash.len(),
            });
        }
        if self.target_sender.len() != validator::LENGTH {
            return Err(CertifiedSettledHistoryAdmissionError::InvalidTargetSender {
                expected: validator::LENGTH,
                actual: self.target_sender.len(),
            });
        }
        if self.anchor_block_hash.len() != hash_length {
            return Err(CertifiedSettledHistoryAdmissionError::InvalidAnchorHash {
                expected: hash_length,
                actual: self.anchor_block_hash.len(),
            });
        }
        if self.anchor_post_state_hash.len() != hash_length {
            return Err(
                CertifiedSettledHistoryAdmissionError::InvalidAnchorPostStateHash {
                    expected: hash_length,
                    actual: self.anchor_post_state_hash.len(),
                },
            );
        }
        if self.citer_block_hash.len() != hash_length {
            return Err(CertifiedSettledHistoryAdmissionError::InvalidCiterHash {
                expected: hash_length,
                actual: self.citer_block_hash.len(),
            });
        }
        if self.citer_sender.len() != validator::LENGTH {
            return Err(CertifiedSettledHistoryAdmissionError::InvalidCiterSender {
                expected: validator::LENGTH,
                actual: self.citer_sender.len(),
            });
        }
        if self.ruleset_digest.len() != hash_length {
            return Err(
                CertifiedSettledHistoryAdmissionError::InvalidRulesetDigest {
                    expected: hash_length,
                    actual: self.ruleset_digest.len(),
                },
            );
        }
        if self.target_protocol_version != CERTIFIED_ADMISSION_PROTOCOL_VERSION {
            return Err(
                CertifiedSettledHistoryAdmissionError::UnsupportedTargetProtocol(
                    self.target_protocol_version,
                ),
            );
        }
        if self.citer_protocol_version != CERTIFIED_ADMISSION_PROTOCOL_VERSION {
            return Err(
                CertifiedSettledHistoryAdmissionError::UnsupportedCiterProtocol(
                    self.citer_protocol_version,
                ),
            );
        }
        if self.admission_schema_version != ADMISSION_SCHEMA_VERSION {
            return Err(CertifiedSettledHistoryAdmissionError::UnsupportedSchema(
                self.admission_schema_version,
            ));
        }
        if self.ruleset_digest != admission_ruleset_digest() {
            return Err(CertifiedSettledHistoryAdmissionError::RulesetDigestMismatch);
        }
        if self.anchor_block_height <= 0 {
            return Err(CertifiedSettledHistoryAdmissionError::InvalidAnchorHeight);
        }
        if self.target_block_height < 0 {
            return Err(CertifiedSettledHistoryAdmissionError::InvalidTargetHeight);
        }
        if self.target_block_height > self.anchor_block_height {
            return Err(CertifiedSettledHistoryAdmissionError::TargetAboveAnchor);
        }
        if self.citer_stake <= 0 {
            return Err(CertifiedSettledHistoryAdmissionError::InvalidCiterStake(
                self.citer_stake,
            ));
        }
        Ok(())
    }

    pub fn target_generation(&self) -> BondGeneration { self.target_generation }

    pub fn anchor_block_hash(&self) -> &Bytes { &self.anchor_block_hash }

    pub const fn anchor_block_height(&self) -> i64 { self.anchor_block_height }

    pub fn anchor_post_state_hash(&self) -> &Bytes { &self.anchor_post_state_hash }

    pub fn citer_block_hash(&self) -> &Bytes { &self.citer_block_hash }

    pub fn citer_sender(&self) -> &Bytes { &self.citer_sender }

    pub fn citer_generation(&self) -> BondGeneration { self.citer_generation }

    pub const fn citer_stake(&self) -> i64 { self.citer_stake }

    pub fn ruleset_digest(&self) -> &Bytes { &self.ruleset_digest }

    pub fn context_digest(&self) -> Bytes {
        let mut bytes = Vec::new();
        append_digest_bytes(&mut bytes, b"f1r3fly-settled-history-context-v1");
        append_digest_bytes(&mut bytes, &self.anchor_block_hash);
        append_digest_i64(&mut bytes, self.anchor_block_height);
        append_digest_bytes(&mut bytes, &self.anchor_post_state_hash);
        append_digest_bytes(&mut bytes, &self.citer_sender);
        append_digest_i64(&mut bytes, self.citer_generation.get());
        append_digest_i64(&mut bytes, self.citer_stake);
        Blake2b256::hash(bytes).into()
    }

    pub fn digest(&self) -> Bytes {
        let mut bytes = Vec::new();
        append_digest_bytes(&mut bytes, b"f1r3fly-certified-settled-history-v1");
        append_digest_bytes(&mut bytes, &self.target_block_hash);
        append_digest_i64(&mut bytes, self.target_protocol_version);
        append_digest_i64(&mut bytes, self.target_block_height);
        append_digest_bytes(&mut bytes, &self.target_sender);
        append_digest_i64(&mut bytes, self.target_generation.get());
        append_digest_bytes(&mut bytes, &self.anchor_block_hash);
        append_digest_i64(&mut bytes, self.anchor_block_height);
        append_digest_bytes(&mut bytes, &self.anchor_post_state_hash);
        append_digest_bytes(&mut bytes, &self.citer_block_hash);
        append_digest_i64(&mut bytes, self.citer_protocol_version);
        append_digest_bytes(&mut bytes, &self.citer_sender);
        append_digest_i64(&mut bytes, self.citer_generation.get());
        append_digest_i64(&mut bytes, self.citer_stake);
        append_digest_i64(&mut bytes, i64::from(self.admission_schema_version));
        append_digest_bytes(&mut bytes, &self.ruleset_digest);
        Blake2b256::hash(bytes).into()
    }

    pub fn to_proto(&self) -> CertifiedSettledHistoryAdmissionProto {
        CertifiedSettledHistoryAdmissionProto {
            target_block_hash: self.target_block_hash.clone(),
            target_protocol_version: self.target_protocol_version,
            target_block_height: self.target_block_height,
            target_sender: self.target_sender.clone(),
            target_bond_generation: self.target_generation.get(),
            anchor_block_hash: self.anchor_block_hash.clone(),
            anchor_block_height: self.anchor_block_height,
            anchor_post_state_hash: self.anchor_post_state_hash.clone(),
            citer_block_hash: self.citer_block_hash.clone(),
            citer_protocol_version: self.citer_protocol_version,
            citer_sender: self.citer_sender.clone(),
            citer_bond_generation: self.citer_generation.get(),
            citer_stake: self.citer_stake,
            admission_schema_version: self.admission_schema_version,
            ruleset_digest: self.ruleset_digest.clone(),
        }
    }

    pub fn from_proto(
        proto: CertifiedSettledHistoryAdmissionProto,
    ) -> Result<Self, CertifiedSettledHistoryAdmissionError> {
        let proof = Self {
            target_block_hash: proto.target_block_hash,
            target_protocol_version: proto.target_protocol_version,
            target_block_height: proto.target_block_height,
            target_sender: proto.target_sender,
            target_generation: BondGeneration::try_from(proto.target_bond_generation).map_err(
                |error| {
                    CertifiedSettledHistoryAdmissionError::InvalidTargetGeneration(
                        error.to_string(),
                    )
                },
            )?,
            anchor_block_hash: proto.anchor_block_hash,
            anchor_block_height: proto.anchor_block_height,
            anchor_post_state_hash: proto.anchor_post_state_hash,
            citer_block_hash: proto.citer_block_hash,
            citer_protocol_version: proto.citer_protocol_version,
            citer_sender: proto.citer_sender,
            citer_generation: BondGeneration::try_from(proto.citer_bond_generation).map_err(
                |error| {
                    CertifiedSettledHistoryAdmissionError::InvalidCiterGeneration(error.to_string())
                },
            )?,
            citer_stake: proto.citer_stake,
            admission_schema_version: proto.admission_schema_version,
            ruleset_digest: proto.ruleset_digest,
        };
        proof.validate_shape()?;
        Ok(proof)
    }
}

#[derive(Clone, Debug)]
pub struct ValidatedSettledHistoryAdmission(CertifiedSettledHistoryAdmission);

impl ValidatedSettledHistoryAdmission {
    pub fn new(
        target: &BlockMessage,
        anchor: &BlockMessage,
        citer: &BlockMessage,
        citer_generation: BondGeneration,
        citer_stake: i64,
    ) -> Result<Self, CertifiedSettledHistoryAdmissionError> {
        if !target.has_valid_content_hash() {
            return Err(CertifiedSettledHistoryAdmissionError::InvalidTargetContentHash);
        }
        if !target.has_valid_block_signature() {
            return Err(CertifiedSettledHistoryAdmissionError::InvalidTargetSignature);
        }
        if !anchor.has_valid_content_hash() {
            return Err(CertifiedSettledHistoryAdmissionError::InvalidAnchorContentHash);
        }
        if !citer.has_valid_content_hash() {
            return Err(CertifiedSettledHistoryAdmissionError::InvalidCiterContentHash);
        }
        if !citer.has_valid_block_signature() {
            return Err(CertifiedSettledHistoryAdmissionError::InvalidCiterSignature);
        }
        CertifiedSettledHistoryAdmission::new(target, anchor, citer, citer_generation, citer_stake)
            .map(Self)
    }

    pub fn from_record(
        record: &CertifiedSettledHistoryAdmission,
        target: &BlockMessage,
        anchor: &BlockMessage,
        citer: &BlockMessage,
    ) -> Result<Self, CertifiedSettledHistoryAdmissionError> {
        let validated = Self::new(
            target,
            anchor,
            citer,
            record.citer_generation(),
            record.citer_stake(),
        )?;
        if validated.record() != record {
            return Err(CertifiedSettledHistoryAdmissionError::EvidenceMismatch);
        }
        Ok(validated)
    }

    pub fn record(&self) -> &CertifiedSettledHistoryAdmission { &self.0 }
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
    PrematureDeployRetry = 27,
    AdmissibleEquivocation = 28,
    IgnorableEquivocation = 29,
}

impl AdmissionRejectionReason {
    pub const ALL: [Self; 29] = [
        Self::InvalidFormat,
        Self::InvalidSignature,
        Self::InvalidSender,
        Self::InvalidVersion,
        Self::InvalidTimestamp,
        Self::DeployNotSigned,
        Self::InvalidBlockNumber,
        Self::InvalidRepeatDeploy,
        Self::InvalidParents,
        Self::InvalidFollows,
        Self::InvalidSequenceNumber,
        Self::InvalidShardId,
        Self::JustificationRegression,
        Self::NeglectedInvalidBlock,
        Self::NeglectedEquivocation,
        Self::InvalidTransaction,
        Self::InvalidBondsCache,
        Self::InvalidEquivocationEvidence,
        Self::InvalidBlockHash,
        Self::UnauthorizedSlashDeploy,
        Self::InvalidRejectedDeploy,
        Self::ContainsExpiredDeploy,
        Self::ContainsTimeExpiredDeploy,
        Self::ContainsFutureDeploy,
        Self::NotOfInterest,
        Self::LowDeployCost,
        Self::PrematureDeployRetry,
        Self::AdmissibleEquivocation,
        Self::IgnorableEquivocation,
    ];

    pub const fn manifest_label(self) -> &'static str {
        match self {
            Self::InvalidFormat => "invalid-format",
            Self::InvalidSignature => "invalid-signature",
            Self::InvalidSender => "invalid-sender",
            Self::InvalidVersion => "invalid-version",
            Self::InvalidTimestamp => "invalid-timestamp",
            Self::DeployNotSigned => "deploy-not-signed",
            Self::InvalidBlockNumber => "invalid-block-number",
            Self::InvalidRepeatDeploy => "invalid-repeat-deploy",
            Self::InvalidParents => "invalid-parents",
            Self::InvalidFollows => "invalid-follows",
            Self::InvalidSequenceNumber => "invalid-sequence-number",
            Self::InvalidShardId => "invalid-shard-id",
            Self::JustificationRegression => "justification-regression",
            Self::NeglectedInvalidBlock => "neglected-invalid-block",
            Self::NeglectedEquivocation => "neglected-equivocation",
            Self::InvalidTransaction => "invalid-transaction",
            Self::InvalidBondsCache => "invalid-bonds-cache",
            Self::InvalidEquivocationEvidence => "invalid-equivocation-evidence",
            Self::InvalidBlockHash => "invalid-block-hash",
            Self::UnauthorizedSlashDeploy => "unauthorized-slash-deploy",
            Self::InvalidRejectedDeploy => "invalid-rejected-deploy",
            Self::ContainsExpiredDeploy => "contains-expired-deploy",
            Self::ContainsTimeExpiredDeploy => "contains-time-expired-deploy",
            Self::ContainsFutureDeploy => "contains-future-deploy",
            Self::NotOfInterest => "not-of-interest",
            Self::LowDeployCost => "low-deploy-cost",
            Self::PrematureDeployRetry => "premature-deploy-retry",
            Self::AdmissibleEquivocation => "admissible-equivocation",
            Self::IgnorableEquivocation => "ignorable-equivocation",
        }
    }

    pub const fn is_slash_evidence_eligible(self) -> bool {
        match self {
            Self::AdmissibleEquivocation | Self::IgnorableEquivocation => true,
            Self::InvalidFormat
            | Self::InvalidSignature
            | Self::InvalidSender
            | Self::InvalidVersion
            | Self::InvalidTimestamp
            | Self::DeployNotSigned
            | Self::InvalidBlockNumber
            | Self::InvalidRepeatDeploy
            | Self::InvalidParents
            | Self::InvalidFollows
            | Self::InvalidSequenceNumber
            | Self::InvalidShardId
            | Self::JustificationRegression
            | Self::NeglectedInvalidBlock
            | Self::NeglectedEquivocation
            | Self::InvalidTransaction
            | Self::InvalidBondsCache
            | Self::InvalidEquivocationEvidence
            | Self::InvalidBlockHash
            | Self::UnauthorizedSlashDeploy
            | Self::InvalidRejectedDeploy
            | Self::ContainsExpiredDeploy
            | Self::ContainsTimeExpiredDeploy
            | Self::ContainsFutureDeploy
            | Self::NotOfInterest
            | Self::LowDeployCost
            | Self::PrematureDeployRetry => false,
        }
    }
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
            27 => Ok(Self::PrematureDeployRetry),
            28 => Ok(Self::AdmissibleEquivocation),
            29 => Ok(Self::IgnorableEquivocation),
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

    pub const fn rejection_reason(&self) -> Option<AdmissionRejectionReason> {
        match self.decision {
            CertifiedAdmissionDecision::Accepted => None,
            CertifiedAdmissionDecision::Rejected(reason) => Some(reason),
        }
    }

    pub const fn is_slash_evidence_eligible(&self) -> bool {
        match self.rejection_reason() {
            Some(reason) => reason.is_slash_evidence_eligible(),
            None => false,
        }
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
    /// The wire name is historical. This set contains every committed state
    /// effect, including settlement after a failed user-body execution.
    pub successful_state_effect_indices: BTreeSet<u32>,
    pub rejected_state_effects: BTreeSet<StateEffectId>,
    pub applied_state_effects: BTreeSet<StateEffectId>,
    pub protocol_version: i64,
    pub objective_equivocation_evidence_delta: Vec<ObjectiveEquivocationEvidence>,
    pub sender_authority: Option<CertifiedSenderAuthority>,
    pub settled_history_admission: Option<CertifiedSettledHistoryAdmission>,
    pub finalized_floor_commitment: Option<FinalizedFloorCommitment>,
    pub admission_schema_version: u32,
    pub approved_genesis: bool,
    #[serde(with = "shared::rust::serde_bytes", default)]
    pub merge_base: Bytes,
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
    #[error("block metadata contains malformed state-effect provenance: {0}")]
    InvalidStateEffectProvenance(String),
    #[error("block metadata applies and rejects the same state effect")]
    ConflictingStateEffectDisposition,
    #[error("block metadata merge base must be empty or 32 bytes, got {0}")]
    InvalidMergeBase(usize),
    #[error("block metadata contains malformed objective evidence: {0}")]
    InvalidObjectiveEvidence(String),
    #[error("block metadata contains an invalid authority certificate: {0}")]
    InvalidAuthorityCertificate(#[from] CertifiedSenderAuthorityError),
    #[error("block metadata contains an invalid admission outcome: {0}")]
    InvalidAdmissionOutcome(#[from] CertifiedAdmissionOutcomeError),
    #[error("block metadata contains an invalid settled-history proof: {0}")]
    InvalidSettledHistoryAdmission(#[from] CertifiedSettledHistoryAdmissionError),
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
    #[error("genesis block metadata must not contain a settled-history proof")]
    UnexpectedGenesisSettledHistoryAdmission,
    #[error("certified block metadata must not contain a settled-history proof")]
    UnexpectedSettledHistoryAdmission,
    #[error("settled-history block metadata must not contain ordinary admission certificates")]
    UnexpectedSettledHistoryCertificates,
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
            && self.applied_state_effects == other.applied_state_effects
            && self.protocol_version == other.protocol_version
            && self.objective_equivocation_evidence_delta
                == other.objective_equivocation_evidence_delta
            && self.sender_authority == other.sender_authority
            && self.settled_history_admission == other.settled_history_admission
            && self.finalized_floor_commitment == other.finalized_floor_commitment
            && self.admission_schema_version == other.admission_schema_version
            && self.approved_genesis == other.approved_genesis
            && self.merge_base == other.merge_base
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
        self.applied_state_effects.hash(state);
        self.protocol_version.hash(state);
        self.objective_equivocation_evidence_delta.hash(state);
        self.sender_authority.hash(state);
        self.settled_history_admission.hash(state);
        self.finalized_floor_commitment.hash(state);
        self.admission_schema_version.hash(state);
        self.approved_genesis.hash(state);
        self.merge_base.hash(state);
    }
}

impl BlockMetadata {
    pub fn from_proto(proto: BlockMetadataInternal) -> Result<Self, BlockMetadataError> {
        let rejected_state_effects = proto
            .rejected_state_effects
            .into_iter()
            .map(StateEffectId::from_proto)
            .collect::<Vec<_>>();
        StateEffectId::validate_canonical_sequence(&rejected_state_effects, "rejectedStateEffects")
            .map_err(BlockMetadataError::InvalidStateEffectProvenance)?;
        let applied_state_effects = proto
            .applied_state_effects
            .into_iter()
            .map(StateEffectId::from_proto)
            .collect::<Vec<_>>();
        StateEffectId::validate_canonical_sequence(&applied_state_effects, "appliedStateEffects")
            .map_err(BlockMetadataError::InvalidStateEffectProvenance)?;
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
        let settled_history_admission = proto
            .settled_history_admission
            .map(CertifiedSettledHistoryAdmission::from_proto)
            .transpose()?;
        let finalized_floor_commitment = proto
            .finalized_floor_commitment
            .map(FinalizedFloorCommitment::from_proto)
            .transpose()
            .map_err(BlockMetadataError::InvalidFinalizedFloorCommitment)?;
        let certified_generation = sender_authority
            .as_ref()
            .map(CertifiedSenderAuthority::generation)
            .or_else(|| {
                settled_history_admission
                    .as_ref()
                    .map(CertifiedSettledHistoryAdmission::target_generation)
            });
        if sender_generation_claim != certified_generation {
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
            rejected_state_effects: rejected_state_effects.into_iter().collect(),
            applied_state_effects: applied_state_effects.into_iter().collect(),
            protocol_version: proto.protocol_version,
            objective_equivocation_evidence_delta,
            sender_authority,
            settled_history_admission,
            finalized_floor_commitment,
            admission_schema_version: proto.admission_schema_version,
            approved_genesis: proto.approved_genesis,
            merge_base: proto.merge_base,
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
            applied_state_effects: self
                .applied_state_effects
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
            settled_history_admission: self
                .settled_history_admission
                .as_ref()
                .map(CertifiedSettledHistoryAdmission::to_proto),
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
            merge_base: self.merge_base.clone(),
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

    fn committed_state_effect_indices(block: &BlockMessage) -> BTreeSet<u32> {
        let mut execution_index = 0usize;
        let mut indices = BTreeSet::new();
        for deploy in &block.body.deploys {
            if deploy.is_admission_rejected() {
                continue;
            }
            if deploy.has_committed_state_effect() {
                indices.insert(
                    u32::try_from(execution_index).expect("block deploy index must fit in u32"),
                );
            }
            execution_index += 1;
        }
        for deploy in &block.body.system_deploys {
            if matches!(deploy, ProcessedSystemDeploy::Succeeded { .. }) {
                indices.insert(
                    u32::try_from(execution_index)
                        .expect("block system deploy index must fit in u32"),
                );
            }
            execution_index += 1;
        }
        indices
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
            successful_state_effect_indices: Self::committed_state_effect_indices(b),
            rejected_state_effects: b.body.rejected_state_effects.iter().cloned().collect(),
            applied_state_effects: b.body.applied_state_effects.iter().cloned().collect(),
            protocol_version: b.header.version,
            objective_equivocation_evidence_delta: b
                .header
                .objective_equivocation_evidence_delta
                .clone(),
            sender_authority: None,
            settled_history_admission: None,
            finalized_floor_commitment: b.header.finalized_floor.clone(),
            admission_schema_version: ADMISSION_SCHEMA_VERSION,
            approved_genesis: false,
            merge_base: b.body.merge_base.clone(),
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

    pub fn from_settled_history_block(
        b: &BlockMessage,
        proof: &CertifiedSettledHistoryAdmission,
    ) -> Result<Self, BlockMetadataError> {
        proof.validate_for(b)?;
        let mut metadata = Self::from_block(b, None, None);
        metadata.settled_history_admission = Some(proof.clone());
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
            .or_else(|| {
                self.settled_history_admission
                    .as_ref()
                    .map(CertifiedSettledHistoryAdmission::target_generation)
            })
    }

    pub fn is_accepted(&self) -> bool {
        self.approved_genesis
            || self.settled_history_admission.is_some()
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

    pub fn rejection_reason(&self) -> Option<AdmissionRejectionReason> {
        self.admission_outcome
            .as_ref()
            .and_then(CertifiedAdmissionOutcome::rejection_reason)
    }

    pub fn is_slash_evidence_eligible(&self) -> bool {
        self.admission_outcome
            .as_ref()
            .is_some_and(CertifiedAdmissionOutcome::is_slash_evidence_eligible)
    }

    pub fn validate(&self) -> Result<(), BlockMetadataError> {
        if self.post_state_hash.len() != block_hash::LENGTH {
            return Err(BlockMetadataError::InvalidPostStateHash {
                expected: block_hash::LENGTH,
                actual: self.post_state_hash.len(),
            });
        }
        if !self.merge_base.is_empty() && self.merge_base.len() != block_hash::LENGTH {
            return Err(BlockMetadataError::InvalidMergeBase(self.merge_base.len()));
        }
        if !self
            .rejected_state_effects
            .is_disjoint(&self.applied_state_effects)
        {
            return Err(BlockMetadataError::ConflictingStateEffectDisposition);
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
            &self.settled_history_admission,
            self.approved_genesis,
        ) {
            (None, None, None, true) => Ok(()),
            (Some(_), _, _, true) => Err(BlockMetadataError::UnexpectedGenesisAuthorityCertificate),
            (_, Some(_), _, true) => Err(BlockMetadataError::UnexpectedGenesisAdmissionOutcome),
            (_, _, Some(_), true) => {
                Err(BlockMetadataError::UnexpectedGenesisSettledHistoryAdmission)
            }
            (None, None, Some(proof), false) => {
                if self.directly_finalized || self.finalized {
                    return Err(BlockMetadataError::InvalidBlockFinalized);
                }
                proof.validate_metadata(&self.block_hash, self.protocol_version, &self.sender)?;
                Ok(())
            }
            (Some(_), _, Some(_), false) | (_, Some(_), Some(_), false) => {
                Err(BlockMetadataError::UnexpectedSettledHistoryCertificates)
            }
            (None, _, None, false) => Err(BlockMetadataError::MissingAuthorityCertificate),
            (_, None, None, false) => Err(BlockMetadataError::MissingAdmissionOutcome),
            (Some(certificate), Some(outcome), None, false) => {
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
    use crypto::rust::private_key::PrivateKey;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;
    use crypto::rust::signatures::signed::Signed;
    use proptest::prelude::*;

    use super::*;
    use crate::rhoapi::PCost;
    use crate::rust::casper::protocol::casper_message::{
        Body, Bond, DeployAdmissionStatus, DeployData, Header, ProcessedDeploy, SystemDeployData,
        ValidatorBondGeneration,
    };

    #[test]
    fn admission_ruleset_manifest_covers_each_rejection_reason_once() {
        let manifest = admission_ruleset_manifest();
        let mut labels = BTreeSet::new();
        let mut discriminants = BTreeSet::new();
        let expected_rules = AdmissionRejectionReason::ALL
            .iter()
            .map(|reason| format!("{}:{}", *reason as u32, reason.manifest_label()))
            .collect::<Vec<_>>();
        let rules = manifest
            .strip_prefix(&format!("{ADMISSION_RULESET_DOMAIN}|0:accepted|"))
            .unwrap()
            .strip_suffix(&format!("|{ADMISSION_RULESET_SUBSYSTEMS}"))
            .unwrap()
            .split('|')
            .collect::<Vec<_>>();

        for (index, reason) in AdmissionRejectionReason::ALL.iter().copied().enumerate() {
            let discriminant = reason as u32;
            assert_eq!(discriminant, u32::try_from(index + 1).unwrap());
            assert_eq!(AdmissionRejectionReason::try_from(discriminant), Ok(reason));
            assert!(labels.insert(reason.manifest_label()));
            assert!(discriminants.insert(discriminant));
        }

        assert_eq!(rules, expected_rules);
        assert_eq!(labels.len(), AdmissionRejectionReason::ALL.len());
        assert_eq!(discriminants.len(), AdmissionRejectionReason::ALL.len());
        assert!(manifest.starts_with("f1r3fly-certified-admission-v15|0:accepted"));
        assert!(manifest.ends_with(ADMISSION_RULESET_SUBSYSTEMS));
    }

    #[test]
    fn admission_ruleset_digest_binds_schema_and_every_rule() {
        let manifest = admission_ruleset_manifest();
        let digest = admission_ruleset_digest();
        let digest_of =
            |value: &str| -> Bytes { Blake2b256::hash(value.as_bytes().to_vec()).into() };

        assert_eq!(digest, digest_of(manifest));
        assert_ne!(
            digest,
            digest_of(&manifest.replacen(ADMISSION_RULESET_DOMAIN, "changed-domain", 1))
        );

        for reason in AdmissionRejectionReason::ALL {
            let entry = format!("|{}:{}", reason as u32, reason.manifest_label());
            assert_ne!(digest, digest_of(&manifest.replacen(&entry, "", 1)));
            assert_ne!(
                digest,
                digest_of(&manifest.replacen(
                    &entry,
                    &format!("|{}:{}", reason as u32 + 100, reason.manifest_label()),
                    1,
                ))
            );
            assert_ne!(
                digest,
                digest_of(&manifest.replacen(&entry, &format!("{entry}-changed"), 1))
            );
        }
    }

    fn processed_deploy(is_failed: bool) -> ProcessedDeploy {
        let algorithm: Box<dyn SignaturesAlg> = Box::new(Secp256k1);
        let (private_key, _) = algorithm.new_key_pair();
        let deploy = Signed::create(
            DeployData {
                term: "Nil".to_string(),
                language: "rholang".to_string(),
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
            envelope_commitment: Bytes::new(),
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
                applied_state_effects: Vec::new(),
                system_deploys: Vec::new(),
                extra_bytes: Bytes::new(),
                applied_from_scope: Vec::new(),
                merge_base: Bytes::new(),
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

    fn settled_history_fixture() -> (
        BlockMessage,
        BlockMessage,
        BlockMessage,
        CertifiedSettledHistoryAdmission,
    ) {
        let mut target = authority_block();
        target.block_hash = Bytes::from(vec![20; block_hash::LENGTH]);
        target.sender = Bytes::from(vec![21; validator::LENGTH]);
        target.body.state.block_number = 3;
        target.header.sender_bond_generation = Some(BondGeneration::GENESIS);

        let citer_sender = Bytes::from(vec![22; validator::LENGTH]);
        let mut citer = authority_block();
        citer.block_hash = Bytes::from(vec![23; block_hash::LENGTH]);
        citer.sender = citer_sender.clone();
        citer.header.parents_hash_list = vec![target.block_hash.clone()];
        citer.header.sender_bond_generation = Some(BondGeneration::GENESIS);
        citer.body.state.block_number = 11;

        let mut anchor = authority_block();
        anchor.block_hash = Bytes::from(vec![24; block_hash::LENGTH]);
        anchor.body.state.block_number = 10;
        anchor.body.state.post_state_hash = Bytes::from(vec![25; block_hash::LENGTH]);
        anchor.body.state.bonds = vec![Bond {
            validator: citer_sender.clone(),
            stake: 100,
        }];
        anchor.body.state.bond_generations = vec![ValidatorBondGeneration {
            validator: citer_sender,
            generation: BondGeneration::GENESIS,
        }];

        let proof = CertifiedSettledHistoryAdmission::new(
            &target,
            &anchor,
            &citer,
            BondGeneration::GENESIS,
            100,
        )
        .unwrap();
        (target, anchor, citer, proof)
    }

    fn sign_block(mut block: BlockMessage, key_byte: u8) -> BlockMessage {
        let algorithm = Secp256k1;
        let private_key = PrivateKey::from_bytes(&[key_byte; 32]);
        block.sender = algorithm.to_public(&private_key).bytes.into();
        block.sig_algorithm = algorithm.name();
        block.block_hash = block.computed_block_hash();
        block.sig = algorithm.sign(&block.block_hash, &private_key.bytes).into();
        block
    }

    fn validated_settled_history_fixture() -> (
        BlockMessage,
        BlockMessage,
        BlockMessage,
        ValidatedSettledHistoryAdmission,
    ) {
        let mut target = authority_block();
        target.body.state.block_number = 3;
        let target = sign_block(target, 31);

        let mut citer = authority_block();
        citer.header.parents_hash_list = vec![target.block_hash.clone()];
        citer.body.state.block_number = 11;
        let citer = sign_block(citer, 32);

        let mut anchor = authority_block();
        anchor.body.state.block_number = 10;
        anchor.body.state.bonds = vec![Bond {
            validator: citer.sender.clone(),
            stake: 100,
        }];
        anchor.body.state.bond_generations = vec![ValidatorBondGeneration {
            validator: citer.sender.clone(),
            generation: BondGeneration::GENESIS,
        }];
        anchor.block_hash = anchor.computed_block_hash();

        let proof = ValidatedSettledHistoryAdmission::new(
            &target,
            &anchor,
            &citer,
            BondGeneration::GENESIS,
            100,
        )
        .unwrap();
        (target, anchor, citer, proof)
    }

    #[test]
    fn settled_history_proof_roundtrips_and_certifies_metadata() {
        let (target, _, _, proof) = settled_history_fixture();
        let decoded = CertifiedSettledHistoryAdmission::from_proto(proof.to_proto()).unwrap();
        assert_eq!(decoded, proof);

        let metadata = BlockMetadata::from_settled_history_block(&target, &proof).unwrap();
        assert!(metadata.is_accepted());
        assert!(!metadata.is_rejected());
        assert!(metadata.sender_authority.is_none());
        assert!(metadata.admission_outcome.is_none());
        assert_eq!(metadata.settled_history_admission, Some(proof));
        assert_eq!(
            BlockMetadata::from_bytes(&metadata.to_bytes()).unwrap(),
            metadata
        );
    }

    #[test]
    fn settled_history_proof_rejects_missing_citation_and_unbonded_citer() {
        let (target, mut anchor, mut citer, _) = settled_history_fixture();
        citer.header.parents_hash_list.clear();
        assert_eq!(
            CertifiedSettledHistoryAdmission::new(
                &target,
                &anchor,
                &citer,
                BondGeneration::GENESIS,
                100,
            ),
            Err(CertifiedSettledHistoryAdmissionError::MissingCitation)
        );

        citer.header.parents_hash_list = vec![target.block_hash.clone()];
        anchor.body.state.bonds.clear();
        assert_eq!(
            CertifiedSettledHistoryAdmission::new(
                &target,
                &anchor,
                &citer,
                BondGeneration::GENESIS,
                100,
            ),
            Err(CertifiedSettledHistoryAdmissionError::CiterNotBonded)
        );
    }

    #[test]
    fn settled_history_validation_rejects_forged_target_and_citer() {
        let (target, anchor, citer, proof) = validated_settled_history_fixture();
        assert_eq!(
            ValidatedSettledHistoryAdmission::from_record(
                proof.record(),
                &target,
                &anchor,
                &citer,
            )
            .unwrap()
            .record(),
            proof.record()
        );

        let mut forged_target = target.clone();
        forged_target.body.state.block_number += 1;
        assert!(matches!(
            ValidatedSettledHistoryAdmission::from_record(
                proof.record(),
                &forged_target,
                &anchor,
                &citer,
            ),
            Err(CertifiedSettledHistoryAdmissionError::InvalidTargetContentHash)
        ));

        let mut forged_citer = citer;
        let mut forged_signature = forged_citer.sig.to_vec();
        forged_signature[0] ^= 1;
        forged_citer.sig = forged_signature.into();
        assert!(matches!(
            ValidatedSettledHistoryAdmission::from_record(
                proof.record(),
                &target,
                &anchor,
                &forged_citer,
            ),
            Err(CertifiedSettledHistoryAdmissionError::InvalidCiterSignature)
        ));
    }

    proptest! {
        #[test]
        fn settled_history_digest_binds_each_target_hash_byte(index in 0usize..block_hash::LENGTH, replacement in any::<u8>()) {
            let (_, _, _, proof) = settled_history_fixture();
            let mut changed = proof.clone();
            let mut hash = changed.target_block_hash.to_vec();
            prop_assume!(hash[index] != replacement);
            hash[index] = replacement;
            changed.target_block_hash = Bytes::from(hash);
            prop_assert_ne!(changed.digest(), proof.digest());
        }
    }

    #[test]
    fn block_metadata_records_all_committed_effects_in_execution_order() {
        let rejected = StateEffectId {
            source_block_hash: Bytes::from(vec![5; block_hash::LENGTH]),
            execution_index: 4,
        };
        let successful = processed_deploy(false);
        let mut admission_rejected = processed_deploy(true);
        admission_rejected.admission_status = DeployAdmissionStatus::Rejected;
        let legacy_failure = processed_deploy(true);
        let mut settled_failure = processed_deploy(true);
        settled_failure.authority_funding_certificate = Some(Default::default());
        settled_failure.authority_cost_witness = Some(Default::default());
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
                deploys: vec![
                    successful,
                    admission_rejected,
                    legacy_failure,
                    settled_failure,
                ],
                rejected_deploys: Vec::new(),
                rejected_state_effects: vec![rejected.clone()],
                applied_state_effects: Vec::new(),
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
                applied_from_scope: Vec::new(),
                merge_base: Bytes::new(),
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
            BTreeSet::from([0, 2, 3])
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

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn committed_effect_projection_compacts_rejections_and_retains_settled_failures(
            dispositions in proptest::collection::vec(
                (any::<bool>(), any::<bool>(), any::<bool>()),
                0..48,
            ),
            system_successes in proptest::collection::vec(any::<bool>(), 0..16),
        ) {
            let template = processed_deploy(false);
            let mut block = authority_block();
            block.body.deploys = dispositions
                .iter()
                .map(|(admission_rejected, failed, settled)| {
                    let mut deploy = template.clone();
                    deploy.is_failed = *failed || *admission_rejected;
                    deploy.admission_status = if *admission_rejected {
                        DeployAdmissionStatus::Rejected
                    } else {
                        DeployAdmissionStatus::Executed
                    };
                    if *settled {
                        deploy.authority_funding_certificate = Some(Default::default());
                        deploy.authority_cost_witness = Some(Default::default());
                    }
                    deploy
                })
                .collect();
            block.body.system_deploys = system_successes
                .iter()
                .map(|success| {
                    if *success {
                        ProcessedSystemDeploy::Succeeded {
                            event_list: Vec::new(),
                            system_deploy: SystemDeployData::Empty,
                            pre_state_hash: Bytes::new(),
                            post_state_hash: Bytes::new(),
                        }
                    } else {
                        ProcessedSystemDeploy::Failed {
                            event_list: Vec::new(),
                            error_msg: "failed".to_string(),
                            pre_state_hash: Bytes::new(),
                            post_state_hash: Bytes::new(),
                        }
                    }
                })
                .collect();

            let mut expected = BTreeSet::new();
            let mut execution_index = 0u32;
            for (admission_rejected, failed, settled) in &dispositions {
                if *admission_rejected {
                    continue;
                }
                if !*failed || *settled {
                    expected.insert(execution_index);
                }
                execution_index += 1;
            }
            for success in &system_successes {
                if *success {
                    expected.insert(execution_index);
                }
                execution_index += 1;
            }

            prop_assert_eq!(
                BlockMetadata::from_block(&block, None, None)
                    .successful_state_effect_indices,
                expected,
            );
        }
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
        for code in 1..=29 {
            let reason = AdmissionRejectionReason::try_from(code).unwrap();
            assert_eq!(reason as u32, code);
            assert_eq!(
                reason.is_slash_evidence_eligible(),
                matches!(
                    reason,
                    AdmissionRejectionReason::AdmissibleEquivocation
                        | AdmissionRejectionReason::IgnorableEquivocation
                )
            );
        }
        assert_eq!(
            AdmissionRejectionReason::try_from(0),
            Err(CertifiedAdmissionOutcomeError::UnknownRejectionReason(0))
        );
        assert_eq!(
            AdmissionRejectionReason::try_from(30),
            Err(CertifiedAdmissionOutcomeError::UnknownRejectionReason(30))
        );
    }

    #[test]
    fn certified_outcome_and_metadata_expose_the_same_evidence_classification() {
        let block = authority_block();
        let certificate = authority_certificate(&block, 10).unwrap();
        let accepted = CertifiedAdmissionOutcome::accepted(&block, &certificate).unwrap();
        assert_eq!(accepted.rejection_reason(), None);
        assert!(!accepted.is_slash_evidence_eligible());

        for code in 1..=29 {
            let reason = AdmissionRejectionReason::try_from(code).unwrap();
            let outcome =
                CertifiedAdmissionOutcome::rejected(&block, &certificate, reason).unwrap();
            let metadata = BlockMetadata::from_certified_block(
                &block,
                Some(false),
                Some(false),
                &certificate,
                &outcome,
            )
            .unwrap();

            assert_eq!(outcome.rejection_reason(), Some(reason));
            assert_eq!(metadata.rejection_reason(), Some(reason));
            assert_eq!(
                metadata.is_slash_evidence_eligible(),
                reason.is_slash_evidence_eligible()
            );
        }
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

#[cfg(test)]
mod round_trip_tests {
    use std::hash::{DefaultHasher, Hash, Hasher};

    use super::*;
    use crate::rust::block_implicits::get_random_block_default;

    fn sample() -> BlockMetadata {
        BlockMetadata {
            block_hash: Bytes::from(vec![1; block_hash::LENGTH]),
            post_state_hash: Bytes::from(vec![2; block_hash::LENGTH]),
            parents: Vec::new(),
            sender: Bytes::from_static(b"sender"),
            justifications: vec![Justification {
                validator: Bytes::from_static(b"validator"),
                latest_block_hash: Bytes::from_static(b"latest"),
            }],
            weight_map: BTreeMap::from([
                (Bytes::from_static(b"v1"), 10),
                (Bytes::from_static(b"v2"), 20),
            ]),
            bond_generation_map: BTreeMap::new(),
            active_validator_set: BTreeSet::new(),
            block_number: 0,
            sequence_number: 0,
            admission_outcome: None,
            directly_finalized: true,
            finalized: true,
            fault_tolerance_value: 0.5,
            successful_state_effect_indices: BTreeSet::from([0, 2]),
            rejected_state_effects: BTreeSet::from([StateEffectId {
                source_block_hash: Bytes::from(vec![3; block_hash::LENGTH]),
                execution_index: 1,
            }]),
            applied_state_effects: BTreeSet::from([StateEffectId {
                source_block_hash: Bytes::from(vec![4; block_hash::LENGTH]),
                execution_index: 3,
            }]),
            protocol_version: CERTIFIED_ADMISSION_PROTOCOL_VERSION,
            objective_equivocation_evidence_delta: Vec::new(),
            sender_authority: None,
            settled_history_admission: None,
            finalized_floor_commitment: None,
            admission_schema_version: ADMISSION_SCHEMA_VERSION,
            approved_genesis: true,
            merge_base: Bytes::from(vec![5; block_hash::LENGTH]),
        }
    }

    fn hash_of(m: &BlockMetadata) -> u64 {
        let mut hasher = DefaultHasher::new();
        m.hash(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn proto_round_trip_preserves_every_field() {
        let metadata = sample();
        let round_tripped = BlockMetadata::from_proto(metadata.to_proto()).unwrap();
        assert_eq!(round_tripped, metadata);
        assert_eq!(
            round_tripped.fault_tolerance_value,
            metadata.fault_tolerance_value
        );
    }

    #[test]
    fn bytes_round_trip_preserves_every_field() {
        let metadata = sample();
        let round_tripped = BlockMetadata::from_bytes(&metadata.to_bytes()).unwrap();
        assert_eq!(round_tripped, metadata);
        assert_eq!(
            round_tripped.fault_tolerance_value,
            metadata.fault_tolerance_value
        );
    }

    #[test]
    fn proto_rejects_noncanonical_applied_state_effects() {
        let mut unordered = sample().to_proto();
        let first = unordered.applied_state_effects[0].clone();
        let second = StateEffectId {
            source_block_hash: Bytes::from(vec![6; block_hash::LENGTH]),
            execution_index: 0,
        }
        .to_proto();
        unordered.applied_state_effects = vec![second, first.clone()];
        assert!(matches!(
            BlockMetadata::from_proto(unordered),
            Err(BlockMetadataError::InvalidStateEffectProvenance(message))
                if message.contains("strictly ordered")
        ));

        let mut duplicate = sample().to_proto();
        duplicate.applied_state_effects = vec![first.clone(), first];
        assert!(matches!(
            BlockMetadata::from_proto(duplicate),
            Err(BlockMetadataError::InvalidStateEffectProvenance(message))
                if message.contains("duplicate")
        ));

        let mut malformed = sample().to_proto();
        malformed.applied_state_effects = vec![StateEffectId {
            source_block_hash: Bytes::from_static(b"short"),
            execution_index: 0,
        }
        .to_proto()];
        assert!(matches!(
            BlockMetadata::from_proto(malformed),
            Err(BlockMetadataError::InvalidStateEffectProvenance(message))
                if message.contains("expected 32 bytes")
        ));
    }

    #[test]
    fn proto_rejects_conflicting_effect_dispositions_and_malformed_state_parent() {
        let mut conflicting = sample().to_proto();
        conflicting.applied_state_effects = conflicting.rejected_state_effects.clone();
        assert_eq!(
            BlockMetadata::from_proto(conflicting),
            Err(BlockMetadataError::ConflictingStateEffectDisposition)
        );

        let mut malformed_parent = sample().to_proto();
        malformed_parent.merge_base = Bytes::from_static(b"short");
        assert_eq!(
            BlockMetadata::from_proto(malformed_parent),
            Err(BlockMetadataError::InvalidMergeBase(5))
        );
    }

    #[test]
    fn to_proto_maps_weight_map_to_bonds() {
        let proto = sample().to_proto();
        let validators: Vec<&[u8]> = proto.bonds.iter().map(|b| b.validator.as_ref()).collect();
        let stakes: Vec<i64> = proto.bonds.iter().map(|b| b.stake).collect();
        assert_eq!(validators, vec![b"v1".as_ref(), b"v2".as_ref()]);
        assert_eq!(stakes, vec![10, 20]);
    }

    #[test]
    fn ordering_by_num_orders_by_block_number_first() {
        let mut low = sample();
        low.block_number = 1;
        let mut high = sample();
        high.block_number = 2;
        assert_eq!(BlockMetadata::ordering_by_num(&low, &high), Ordering::Less);
        assert_eq!(
            BlockMetadata::ordering_by_num(&high, &low),
            Ordering::Greater
        );
    }

    #[test]
    fn ordering_by_num_breaks_ties_by_block_hash() {
        let mut a = sample();
        a.block_hash = Bytes::from_static(b"aaa");
        let mut b = sample();
        b.block_hash = Bytes::from_static(b"bbb");
        assert_eq!(BlockMetadata::ordering_by_num(&a, &b), Ordering::Less);
        assert_eq!(BlockMetadata::ordering_by_num(&a, &a), Ordering::Equal);
    }

    #[test]
    fn from_block_copies_block_fields_and_defaults_flags() {
        let block = get_random_block_default();
        let metadata = BlockMetadata::from_block(&block, None, None);

        assert_eq!(metadata.block_hash, block.block_hash);
        assert_eq!(metadata.post_state_hash, block.body.state.post_state_hash);
        assert_eq!(metadata.parents, block.header.parents_hash_list);
        assert_eq!(metadata.sender, block.sender);
        assert_eq!(metadata.justifications, block.justifications);
        assert_eq!(metadata.block_number, block.body.state.block_number);
        assert_eq!(metadata.sequence_number, block.seq_num);
        assert_eq!(metadata.merge_base, block.body.merge_base);
        assert_eq!(
            metadata.applied_state_effects,
            block.body.applied_state_effects.iter().cloned().collect()
        );
        assert_eq!(
            metadata.rejected_state_effects,
            block.body.rejected_state_effects.iter().cloned().collect()
        );
        assert!(!metadata.directly_finalized);
        assert!(!metadata.finalized);
        assert_eq!(metadata.fault_tolerance_value, 0.0);

        for bond in &block.body.state.bonds {
            assert_eq!(metadata.weight_map.get(&bond.validator), Some(&bond.stake));
        }
        assert_eq!(metadata.weight_map.len(), block.body.state.bonds.len());
    }

    #[test]
    fn from_block_honors_explicit_finalization_flags() {
        let block = get_random_block_default();
        let metadata = BlockMetadata::from_block(&block, Some(true), Some(true));
        assert!(metadata.directly_finalized);
        assert!(metadata.finalized);
    }

    #[test]
    fn equality_and_hash_ignore_fault_tolerance_value() {
        let a = sample();
        let mut b = sample();
        b.fault_tolerance_value = -1.0;
        assert_eq!(a, b);
        assert_eq!(hash_of(&a), hash_of(&b));
    }

    #[test]
    fn equality_distinguishes_block_hash() {
        let a = sample();
        let mut b = sample();
        b.block_hash = Bytes::from_static(b"other");
        assert_ne!(a, b);
        assert_ne!(hash_of(&a), hash_of(&b));
    }
}
