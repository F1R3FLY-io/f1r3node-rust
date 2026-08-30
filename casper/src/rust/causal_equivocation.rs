use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use crypto::rust::hash::blake2b256::Blake2b256;
use models::rust::block_hash::BlockHash;
use models::rust::block_metadata::{CertifiedSenderAuthority, CertifiedSenderAuthorityError};
use models::rust::bond_generation::{BondGeneration, ValidatorIncarnation};
use models::rust::casper::protocol::casper_message::{BlockMessage, ObjectiveEquivocationEvidence};
use models::rust::validator::Validator;
use prost::bytes::Bytes;

use crate::rust::errors::CasperError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VoteExclusion {
    AbsentAuthority,
    NonPositiveStake,
    AbsentAuthorityGeneration,
    MissingCertifiedGeneration,
    WrongGeneration,
    IntrinsicallyInvalid,
    SenderMismatch,
    ObjectiveEquivocation,
    DoesNotDescendFromFloor,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CausalParentExclusion {
    AbsentAuthority,
    NonPositiveStake,
    AbsentAuthorityGeneration,
    MissingCertifiedGeneration,
    WrongGeneration,
    IntrinsicallyInvalid,
    SenderMismatch,
    ObjectiveEquivocation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CausalParentProjection {
    eligible_latest_messages: BTreeMap<Validator, BlockHash>,
    exclusions: BTreeMap<Validator, CausalParentExclusion>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FinalityVoteProjection {
    exact_latest_messages: BTreeMap<Validator, BlockHash>,
    eligible_latest_messages: BTreeMap<Validator, BlockHash>,
    incoming_evidence: BTreeSet<ObjectiveEquivocationEvidence>,
    exclusions: BTreeMap<Validator, VoteExclusion>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CertifiedConsensusContext {
    incoming_finalized_floor: BlockHash,
    incoming_finalized_floor_post_state_hash: BlockHash,
    active_validators: BTreeSet<Validator>,
    authority_stakes: BTreeMap<Validator, i64>,
    authority_generations: BTreeMap<Validator, BondGeneration>,
    causal_parent_projection: CausalParentProjection,
    vote_projection: FinalityVoteProjection,
    digest: BlockHash,
}

impl CertifiedConsensusContext {
    pub fn pre_genesis() -> Self {
        let mut context = Self {
            incoming_finalized_floor: Bytes::from(vec![0; models::rust::block_hash::LENGTH]),
            incoming_finalized_floor_post_state_hash: Bytes::from(vec![
                0;
                models::rust::block_hash::LENGTH
            ]),
            active_validators: BTreeSet::new(),
            authority_stakes: BTreeMap::new(),
            authority_generations: BTreeMap::new(),
            causal_parent_projection: CausalParentProjection {
                eligible_latest_messages: BTreeMap::new(),
                exclusions: BTreeMap::new(),
            },
            vote_projection: FinalityVoteProjection {
                exact_latest_messages: BTreeMap::new(),
                eligible_latest_messages: BTreeMap::new(),
                incoming_evidence: BTreeSet::new(),
                exclusions: BTreeMap::new(),
            },
            digest: Bytes::new(),
        };
        context.digest = context.compute_digest();
        context
    }

    pub fn from_frozen_authority(
        dag: &KeyValueDagRepresentation,
        incoming_finalized_floor: BlockHash,
        incoming_finalized_floor_post_state_hash: BlockHash,
        exact_latest_messages: &BTreeMap<Validator, BlockHash>,
        incoming_evidence: BTreeSet<ObjectiveEquivocationEvidence>,
        active_validators: BTreeSet<Validator>,
        authority_stakes: BTreeMap<Validator, i64>,
        authority_generations: BTreeMap<Validator, BondGeneration>,
    ) -> Result<Self, CasperError> {
        if active_validators.len()
            > models::rust::casper::protocol::casper_message::FinalizationCertificate::MAX_EXACT_LATEST_MESSAGES
        {
            return Err(CasperError::RuntimeError(format!(
                "certified consensus context has {} active validators, exceeding finalization-certificate capacity {}",
                active_validators.len(),
                models::rust::casper::protocol::casper_message::FinalizationCertificate::MAX_EXACT_LATEST_MESSAGES
            )));
        }
        if incoming_finalized_floor.len() != models::rust::block_hash::LENGTH
            || incoming_finalized_floor_post_state_hash.len() != models::rust::block_hash::LENGTH
        {
            return Err(CasperError::RuntimeError(
                "certified consensus context has a malformed floor identity".to_string(),
            ));
        }
        if active_validators.iter().any(|validator| {
            validator.len() != models::rust::validator::LENGTH
                || authority_stakes
                    .get(validator)
                    .is_none_or(|stake| *stake <= 0)
                || !authority_generations.contains_key(validator)
        }) || authority_stakes.keys().collect::<BTreeSet<_>>()
            != active_validators.iter().collect::<BTreeSet<_>>()
            || authority_generations.keys().collect::<BTreeSet<_>>()
                != active_validators.iter().collect::<BTreeSet<_>>()
        {
            return Err(CasperError::RuntimeError(
                "certified consensus context authority maps do not exactly match its active-validator set"
                    .to_string(),
            ));
        }
        let (causal_parent_projection, vote_projection) = derive_consensus_projections(
            dag,
            &incoming_finalized_floor,
            exact_latest_messages,
            incoming_evidence,
            &authority_stakes,
            &authority_generations,
        )?;
        let mut context = Self {
            incoming_finalized_floor,
            incoming_finalized_floor_post_state_hash,
            active_validators,
            authority_stakes,
            authority_generations,
            causal_parent_projection,
            vote_projection,
            digest: Bytes::new(),
        };
        context.digest = context.compute_digest();
        Ok(context)
    }

    pub fn for_parents(
        dag: &KeyValueDagRepresentation,
        parents: &[BlockHash],
        exact_latest_messages: &BTreeMap<Validator, BlockHash>,
    ) -> Result<Self, CasperError> {
        let incoming_finalized_floor = incoming_finalized_floor(dag, parents)?;
        let closure_roots = std::iter::once(&incoming_finalized_floor)
            .chain(parents.iter())
            .chain(exact_latest_messages.values())
            .cloned()
            .collect::<Vec<_>>();
        Self::from_authority_floor(
            dag,
            incoming_finalized_floor,
            exact_latest_messages,
            effective_evidence_context(&closure_roots, dag)?,
        )
    }

    pub fn for_frozen_floor(
        dag: &KeyValueDagRepresentation,
        incoming_finalized_floor: BlockHash,
        exact_latest_messages: &BTreeMap<Validator, BlockHash>,
    ) -> Result<Self, CasperError> {
        let closure_roots = std::iter::once(incoming_finalized_floor.clone())
            .chain(exact_latest_messages.values().cloned())
            .collect::<Vec<_>>();
        Self::from_authority_floor(
            dag,
            incoming_finalized_floor,
            exact_latest_messages,
            effective_evidence_context(&closure_roots, dag)?,
        )
    }

    pub fn for_authority_floor_baseline(
        dag: &KeyValueDagRepresentation,
        incoming_finalized_floor: BlockHash,
    ) -> Result<Self, CasperError> {
        let authority = dag.lookup_unsafe(&incoming_finalized_floor)?;
        let exact_latest_messages = authority
            .active_validator_set
            .iter()
            .map(|validator| (validator.clone(), incoming_finalized_floor.clone()))
            .collect();
        Self::for_frozen_floor(dag, incoming_finalized_floor, &exact_latest_messages)
    }

    pub fn for_finalized_floor(
        dag: &KeyValueDagRepresentation,
        incoming_finalized_floor: BlockHash,
    ) -> Result<Self, CasperError> {
        let authority = dag.lookup_unsafe(&incoming_finalized_floor)?;
        let exact_latest_messages =
            exact_latest_messages_for_active_validators(dag, &authority.active_validator_set)?;
        Self::for_frozen_floor(dag, incoming_finalized_floor, &exact_latest_messages)
    }

    pub async fn for_candidate(
        dag: &KeyValueDagRepresentation,
        block_store: &block_storage::rust::key_value_block_store::KeyValueBlockStore,
        parents: &[BlockHash],
        exact_latest_messages: &BTreeMap<Validator, BlockHash>,
        ftt: crate::rust::safety::clique_oracle::FtThreshold,
    ) -> Result<Self, CasperError> {
        let floor = crate::rust::finality::floor::finalized_floor(
            dag,
            block_store,
            parents,
            exact_latest_messages,
            ftt,
        )
        .await?;
        let closure_roots = std::iter::once(&floor.hash)
            .chain(parents.iter())
            .chain(exact_latest_messages.values())
            .cloned()
            .collect::<Vec<_>>();
        Self::from_authority_floor(
            dag,
            floor.hash,
            exact_latest_messages,
            effective_evidence_context(&closure_roots, dag)?,
        )
    }

    fn from_authority_floor(
        dag: &KeyValueDagRepresentation,
        incoming_finalized_floor: BlockHash,
        exact_latest_messages: &BTreeMap<Validator, BlockHash>,
        incoming_evidence: BTreeSet<ObjectiveEquivocationEvidence>,
    ) -> Result<Self, CasperError> {
        let authority = dag.lookup_unsafe(&incoming_finalized_floor)?;
        let active_validators = authority.active_validator_set.clone();
        let authority_stakes = active_validators
            .iter()
            .map(|validator| {
                authority
                    .weight_map
                    .get(validator)
                    .copied()
                    .filter(|stake| *stake > 0)
                    .map(|stake| (validator.clone(), stake))
                    .ok_or_else(|| {
                        CasperError::RuntimeError(format!(
                            "active floor validator {} has no positive certified stake",
                            hex::encode(validator)
                        ))
                    })
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        let authority_generations = active_validators
            .iter()
            .map(|validator| {
                authority
                    .bond_generation_map
                    .get(validator)
                    .copied()
                    .map(|generation| (validator.clone(), generation))
                    .ok_or_else(|| {
                        CasperError::RuntimeError(format!(
                            "active floor validator {} has no certified generation",
                            hex::encode(validator)
                        ))
                    })
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        Self::from_frozen_authority(
            dag,
            incoming_finalized_floor,
            authority.post_state_hash.clone(),
            exact_latest_messages,
            incoming_evidence,
            active_validators,
            authority_stakes,
            authority_generations,
        )
    }

    pub fn for_target(
        dag: &KeyValueDagRepresentation,
        target: &BlockHash,
    ) -> Result<Self, CasperError> {
        let metadata = dag.lookup_unsafe(target)?;
        let exact_latest_messages = metadata
            .justifications
            .iter()
            .map(|justification| {
                (
                    justification.validator.clone(),
                    justification.latest_block_hash.clone(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        if metadata.parents.is_empty() {
            return Self::from_authority_floor(
                dag,
                target.clone(),
                &exact_latest_messages,
                BTreeSet::new(),
            );
        }
        Self::for_parents(dag, &metadata.parents, &exact_latest_messages)
    }

    pub fn incoming_finalized_floor(&self) -> &BlockHash { &self.incoming_finalized_floor }

    pub fn incoming_finalized_floor_post_state_hash(&self) -> &BlockHash {
        &self.incoming_finalized_floor_post_state_hash
    }

    pub fn active_validators(&self) -> &BTreeSet<Validator> { &self.active_validators }

    pub fn authority_stakes(&self) -> &BTreeMap<Validator, i64> { &self.authority_stakes }

    pub fn authority_generations(&self) -> &BTreeMap<Validator, BondGeneration> {
        &self.authority_generations
    }

    pub fn vote_projection(&self) -> &FinalityVoteProjection { &self.vote_projection }

    pub fn causal_parent_projection(&self) -> &CausalParentProjection {
        &self.causal_parent_projection
    }

    pub fn digest(&self) -> &BlockHash { &self.digest }

    pub fn has_complete_latest_message_slots(&self) -> bool {
        self.vote_projection
            .exact_latest_messages
            .keys()
            .eq(self.active_validators.iter())
    }

    pub fn certify_sender(
        &self,
        block: &BlockMessage,
    ) -> Result<CertifiedSenderAuthority, CertifiedSenderAuthorityError> {
        let generation = self
            .authority_generations
            .get(&block.sender)
            .copied()
            .ok_or(CertifiedSenderAuthorityError::SenderMismatch)?;
        let stake = self
            .authority_stakes
            .get(&block.sender)
            .copied()
            .ok_or(CertifiedSenderAuthorityError::SenderMismatch)?;
        let certificate = CertifiedSenderAuthority::new(
            block,
            self.incoming_finalized_floor.clone(),
            self.incoming_finalized_floor_post_state_hash.clone(),
            self.digest.clone(),
            generation,
            stake,
        )?;
        certificate.validate_context(
            &self.incoming_finalized_floor,
            &self.incoming_finalized_floor_post_state_hash,
            &self.digest,
            generation,
            stake,
        )?;
        Ok(certificate)
    }

    pub fn validate_certificate(
        &self,
        block: &BlockMessage,
        certificate: &CertifiedSenderAuthority,
    ) -> Result<(), CertifiedSenderAuthorityError> {
        certificate.validate_for(block)?;
        let generation = self
            .authority_generations
            .get(&block.sender)
            .copied()
            .ok_or(CertifiedSenderAuthorityError::SenderMismatch)?;
        let stake = self
            .authority_stakes
            .get(&block.sender)
            .copied()
            .ok_or(CertifiedSenderAuthorityError::SenderMismatch)?;
        certificate.validate_context(
            &self.incoming_finalized_floor,
            &self.incoming_finalized_floor_post_state_hash,
            &self.digest,
            generation,
            stake,
        )
    }

    pub fn incoming_evidence(&self) -> &BTreeSet<ObjectiveEquivocationEvidence> {
        self.vote_projection.incoming_evidence()
    }

    pub fn normalized_initial_fault(&self) -> f32 {
        let equivocating_incarnations = self
            .incoming_evidence()
            .iter()
            .map(|evidence| (evidence.validator.clone(), evidence.bond_generation))
            .collect::<BTreeSet<_>>();
        let equivocating_weight: i64 = self
            .authority_stakes
            .iter()
            .filter(|(validator, stake)| {
                **stake > 0
                    && self
                        .authority_generations
                        .get(*validator)
                        .is_some_and(|generation| {
                            equivocating_incarnations.contains(&((*validator).clone(), *generation))
                        })
            })
            .map(|(_, stake)| *stake)
            .sum();
        let total_weight: i64 = self
            .authority_stakes
            .values()
            .filter(|stake| **stake > 0)
            .sum();
        if total_weight == 0 {
            0.0
        } else {
            equivocating_weight as f32 / total_weight as f32
        }
    }

    fn compute_digest(&self) -> BlockHash {
        let mut bytes = Vec::new();
        append_bytes(&mut bytes, b"f1r3fly-certified-consensus-context-v2");
        append_bytes(&mut bytes, &self.incoming_finalized_floor);
        append_bytes(&mut bytes, &self.incoming_finalized_floor_post_state_hash);
        append_len(&mut bytes, self.active_validators.len());
        for validator in &self.active_validators {
            append_bytes(&mut bytes, validator);
            append_i64(&mut bytes, self.authority_stakes[validator]);
            append_i64(&mut bytes, self.authority_generations[validator].get());
        }
        append_len(&mut bytes, self.vote_projection.exact_latest_messages.len());
        for (validator, block_hash) in &self.vote_projection.exact_latest_messages {
            append_bytes(&mut bytes, validator);
            append_bytes(&mut bytes, block_hash);
        }
        append_len(&mut bytes, self.vote_projection.incoming_evidence.len());
        for evidence in &self.vote_projection.incoming_evidence {
            append_bytes(&mut bytes, &evidence.validator);
            append_i64(&mut bytes, evidence.bond_generation.get());
            append_i32(&mut bytes, evidence.sequence_number);
            append_bytes(&mut bytes, &evidence.first_block_hash);
            append_bytes(&mut bytes, &evidence.second_block_hash);
        }
        append_len(
            &mut bytes,
            self.causal_parent_projection.eligible_latest_messages.len(),
        );
        for (validator, block_hash) in &self.causal_parent_projection.eligible_latest_messages {
            append_bytes(&mut bytes, validator);
            append_bytes(&mut bytes, block_hash);
        }
        append_len(&mut bytes, self.causal_parent_projection.exclusions.len());
        for (validator, exclusion) in &self.causal_parent_projection.exclusions {
            append_bytes(&mut bytes, validator);
            bytes.push(exclusion.digest_tag());
        }
        append_len(
            &mut bytes,
            self.vote_projection.eligible_latest_messages.len(),
        );
        for (validator, block_hash) in &self.vote_projection.eligible_latest_messages {
            append_bytes(&mut bytes, validator);
            append_bytes(&mut bytes, block_hash);
        }
        append_len(&mut bytes, self.vote_projection.exclusions.len());
        for (validator, exclusion) in &self.vote_projection.exclusions {
            append_bytes(&mut bytes, validator);
            bytes.push(exclusion.digest_tag());
        }
        Blake2b256::hash(bytes).into()
    }
}

fn exact_latest_messages_for_active_validators(
    dag: &KeyValueDagRepresentation,
    active_validators: &BTreeSet<Validator>,
) -> Result<BTreeMap<Validator, BlockHash>, CasperError> {
    let latest_messages = dag.latest_message_hashes();
    active_validators
        .iter()
        .map(|validator| {
            latest_messages
                .get(validator)
                .cloned()
                .map(|hash| (validator.clone(), hash))
                .ok_or_else(|| {
                    CasperError::RuntimeError(format!(
                        "active finalized-floor validator {} has no exact latest-message slot",
                        hex::encode(validator)
                    ))
                })
        })
        .collect()
}

fn append_len(output: &mut Vec<u8>, length: usize) {
    output.extend_from_slice(&(length as u64).to_be_bytes());
}

fn append_bytes(output: &mut Vec<u8>, value: &[u8]) {
    append_len(output, value.len());
    output.extend_from_slice(value);
}

fn append_i64(output: &mut Vec<u8>, value: i64) { output.extend_from_slice(&value.to_be_bytes()); }

fn append_i32(output: &mut Vec<u8>, value: i32) { output.extend_from_slice(&value.to_be_bytes()); }

impl VoteExclusion {
    const fn digest_tag(&self) -> u8 {
        match self {
            Self::AbsentAuthority => 0,
            Self::NonPositiveStake => 1,
            Self::AbsentAuthorityGeneration => 2,
            Self::MissingCertifiedGeneration => 3,
            Self::WrongGeneration => 4,
            Self::IntrinsicallyInvalid => 5,
            Self::SenderMismatch => 6,
            Self::ObjectiveEquivocation => 7,
            Self::DoesNotDescendFromFloor => 8,
        }
    }
}

impl CausalParentExclusion {
    const fn digest_tag(&self) -> u8 {
        match self {
            Self::AbsentAuthority => 0,
            Self::NonPositiveStake => 1,
            Self::AbsentAuthorityGeneration => 2,
            Self::MissingCertifiedGeneration => 3,
            Self::WrongGeneration => 4,
            Self::IntrinsicallyInvalid => 5,
            Self::SenderMismatch => 6,
            Self::ObjectiveEquivocation => 7,
        }
    }
}

pub fn incoming_finalized_floor(
    dag: &KeyValueDagRepresentation,
    parents: &[BlockHash],
) -> Result<BlockHash, CasperError> {
    let mut floors = parents
        .iter()
        .map(|parent| {
            let floor = dag.get_cached_floor(parent)?.ok_or_else(|| {
                CasperError::RuntimeError(format!(
                    "parent {} has no certified finalized-floor context",
                    hex::encode(parent)
                ))
            })?;
            let block_number = dag.block_number_unsafe(&floor)?;
            Ok((block_number, floor))
        })
        .collect::<Result<Vec<_>, CasperError>>()?;
    floors.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    let (_, selected) = floors.last().cloned().ok_or_else(|| {
        CasperError::RuntimeError(
            "a non-genesis consensus context requires at least one parent".to_string(),
        )
    })?;
    for (_, floor) in floors {
        if floor != selected && !dag.is_dag_ancestor(&floor, &selected)? {
            return Err(CasperError::RuntimeError(format!(
                "parents carry incompatible finalized floors {} and {}",
                hex::encode(floor),
                hex::encode(&selected)
            )));
        }
    }
    Ok(selected)
}

fn derive_consensus_projections(
    dag: &KeyValueDagRepresentation,
    authority_floor: &BlockHash,
    exact_latest_messages: &BTreeMap<Validator, BlockHash>,
    incoming_evidence: BTreeSet<ObjectiveEquivocationEvidence>,
    authority_stakes: &BTreeMap<Validator, i64>,
    authority_generations: &BTreeMap<Validator, BondGeneration>,
) -> Result<(CausalParentProjection, FinalityVoteProjection), CasperError> {
    let equivocating_incarnations = incoming_evidence
        .iter()
        .map(|evidence| {
            ValidatorIncarnation::new(evidence.validator.clone(), evidence.bond_generation)
        })
        .collect::<HashSet<_>>();
    let mut causal_parent_latest_messages = BTreeMap::new();
    let mut causal_parent_exclusions = BTreeMap::new();
    let mut vote_latest_messages = BTreeMap::new();
    let mut vote_exclusions = BTreeMap::new();
    for (validator, block_hash) in exact_latest_messages {
        let base_exclusion = match authority_stakes.get(validator) {
            None => Some(CausalParentExclusion::AbsentAuthority),
            Some(stake) if *stake <= 0 => Some(CausalParentExclusion::NonPositiveStake),
            Some(_) => match authority_generations.get(validator).copied() {
                None => Some(CausalParentExclusion::AbsentAuthorityGeneration),
                Some(authority_generation) => {
                    let metadata = dag.lookup(block_hash)?.ok_or_else(|| {
                        CasperError::RuntimeError(format!(
                            "consensus projection cites absent block {} for validator {}",
                            hex::encode(block_hash),
                            hex::encode(validator)
                        ))
                    })?;
                    if !metadata.is_accepted() {
                        Some(CausalParentExclusion::IntrinsicallyInvalid)
                    } else if !metadata.approved_genesis && metadata.sender != *validator {
                        Some(CausalParentExclusion::SenderMismatch)
                    } else if !metadata.approved_genesis {
                        match metadata.sender_bond_generation() {
                            None => Some(CausalParentExclusion::MissingCertifiedGeneration),
                            Some(certified_generation)
                                if certified_generation != authority_generation =>
                            {
                                Some(CausalParentExclusion::WrongGeneration)
                            }
                            Some(_) => {
                                if equivocating_incarnations.contains(&ValidatorIncarnation::new(
                                    validator.clone(),
                                    authority_generation,
                                )) {
                                    Some(CausalParentExclusion::ObjectiveEquivocation)
                                } else {
                                    None
                                }
                            }
                        }
                    } else if equivocating_incarnations.contains(&ValidatorIncarnation::new(
                        validator.clone(),
                        authority_generation,
                    )) {
                        Some(CausalParentExclusion::ObjectiveEquivocation)
                    } else {
                        None
                    }
                }
            },
        };
        if let Some(exclusion) = base_exclusion {
            vote_exclusions.insert(validator.clone(), VoteExclusion::from(exclusion.clone()));
            causal_parent_exclusions.insert(validator.clone(), exclusion);
            continue;
        }
        causal_parent_latest_messages.insert(validator.clone(), block_hash.clone());
        if block_hash == authority_floor || dag.is_dag_ancestor(authority_floor, block_hash)? {
            vote_latest_messages.insert(validator.clone(), block_hash.clone());
        } else {
            vote_exclusions.insert(validator.clone(), VoteExclusion::DoesNotDescendFromFloor);
        }
    }
    Ok((
        CausalParentProjection {
            eligible_latest_messages: causal_parent_latest_messages,
            exclusions: causal_parent_exclusions,
        },
        FinalityVoteProjection {
            exact_latest_messages: exact_latest_messages.clone(),
            eligible_latest_messages: vote_latest_messages,
            incoming_evidence,
            exclusions: vote_exclusions,
        },
    ))
}

impl From<CausalParentExclusion> for VoteExclusion {
    fn from(value: CausalParentExclusion) -> Self {
        match value {
            CausalParentExclusion::AbsentAuthority => Self::AbsentAuthority,
            CausalParentExclusion::NonPositiveStake => Self::NonPositiveStake,
            CausalParentExclusion::AbsentAuthorityGeneration => Self::AbsentAuthorityGeneration,
            CausalParentExclusion::MissingCertifiedGeneration => Self::MissingCertifiedGeneration,
            CausalParentExclusion::WrongGeneration => Self::WrongGeneration,
            CausalParentExclusion::IntrinsicallyInvalid => Self::IntrinsicallyInvalid,
            CausalParentExclusion::SenderMismatch => Self::SenderMismatch,
            CausalParentExclusion::ObjectiveEquivocation => Self::ObjectiveEquivocation,
        }
    }
}

impl CausalParentProjection {
    pub fn eligible_latest_messages(&self) -> &BTreeMap<Validator, BlockHash> {
        &self.eligible_latest_messages
    }

    pub fn exclusions(&self) -> &BTreeMap<Validator, CausalParentExclusion> { &self.exclusions }
}

impl FinalityVoteProjection {
    pub fn derive(
        dag: &KeyValueDagRepresentation,
        authority_floor: &BlockHash,
        exact_latest_messages: &BTreeMap<Validator, BlockHash>,
        incoming_evidence: BTreeSet<ObjectiveEquivocationEvidence>,
        authority_stakes: &BTreeMap<Validator, i64>,
        authority_generations: &BTreeMap<Validator, BondGeneration>,
    ) -> Result<Self, CasperError> {
        Ok(derive_consensus_projections(
            dag,
            authority_floor,
            exact_latest_messages,
            incoming_evidence,
            authority_stakes,
            authority_generations,
        )?
        .1)
    }

    pub fn for_target(
        dag: &KeyValueDagRepresentation,
        target: &BlockHash,
        exact_latest_messages: &BTreeMap<Validator, BlockHash>,
        incoming_evidence: BTreeSet<ObjectiveEquivocationEvidence>,
    ) -> Result<Self, CasperError> {
        let target_metadata = dag.lookup_unsafe(target)?;
        if target_metadata.parents.is_empty() {
            return Self::derive(
                dag,
                target,
                exact_latest_messages,
                incoming_evidence,
                &target_metadata.weight_map,
                &target_metadata.bond_generation_map,
            );
        }
        let incoming_floor = incoming_finalized_floor(dag, &target_metadata.parents)?;
        let authority = dag.lookup_unsafe(&incoming_floor)?;
        Self::derive(
            dag,
            &incoming_floor,
            exact_latest_messages,
            incoming_evidence,
            &authority.weight_map,
            &authority.bond_generation_map,
        )
    }

    pub fn exact_latest_messages(&self) -> &BTreeMap<Validator, BlockHash> {
        &self.exact_latest_messages
    }

    pub fn eligible_latest_messages(&self) -> &BTreeMap<Validator, BlockHash> {
        &self.eligible_latest_messages
    }

    pub fn incoming_evidence(&self) -> &BTreeSet<ObjectiveEquivocationEvidence> {
        &self.incoming_evidence
    }

    pub fn exclusions(&self) -> &BTreeMap<Validator, VoteExclusion> { &self.exclusions }
}

pub fn project_parent_votes(
    dag: &KeyValueDagRepresentation,
    parents: &[BlockHash],
    exact_latest_messages: &BTreeMap<Validator, BlockHash>,
) -> Result<FinalityVoteProjection, CasperError> {
    Ok(
        CertifiedConsensusContext::for_parents(dag, parents, exact_latest_messages)?
            .vote_projection,
    )
}

pub fn inherited_evidence_context(
    roots: &[BlockHash],
    dag: &KeyValueDagRepresentation,
) -> Result<BTreeSet<ObjectiveEquivocationEvidence>, CasperError> {
    Ok(causal_evidence_closure(roots, dag)?
        .inherited
        .into_values()
        .collect())
}

fn effective_evidence_context(
    roots: &[BlockHash],
    dag: &KeyValueDagRepresentation,
) -> Result<BTreeSet<ObjectiveEquivocationEvidence>, CasperError> {
    Ok(causal_evidence_closure(roots, dag)?
        .effective
        .into_values()
        .collect())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CausalEvidenceClosure {
    inherited: BTreeMap<ValidatorIncarnation, ObjectiveEquivocationEvidence>,
    effective: BTreeMap<ValidatorIncarnation, ObjectiveEquivocationEvidence>,
}

impl CausalEvidenceClosure {
    fn required_delta(&self) -> Vec<ObjectiveEquivocationEvidence> {
        self.effective
            .iter()
            .filter(|(incarnation, evidence)| self.inherited.get(*incarnation) != Some(*evidence))
            .map(|(_, evidence)| evidence.clone())
            .collect()
    }
}

fn evidence_incarnation(evidence: &ObjectiveEquivocationEvidence) -> ValidatorIncarnation {
    ValidatorIncarnation::new(evidence.validator.clone(), evidence.bond_generation)
}

fn insert_canonical_evidence(
    context: &mut BTreeMap<ValidatorIncarnation, ObjectiveEquivocationEvidence>,
    evidence: ObjectiveEquivocationEvidence,
) {
    let incarnation = evidence_incarnation(&evidence);
    match context.entry(incarnation) {
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(evidence);
        }
        std::collections::btree_map::Entry::Occupied(mut entry) => {
            if evidence < *entry.get() {
                entry.insert(evidence);
            }
        }
    }
}

fn evidence_fact_is_sound(
    evidence: &ObjectiveEquivocationEvidence,
    dag: &KeyValueDagRepresentation,
) -> Result<bool, CasperError> {
    let first = dag.lookup(&evidence.first_block_hash)?;
    let second = dag.lookup(&evidence.second_block_hash)?;
    let (Some(first), Some(second)) = (first, second) else {
        return Ok(false);
    };
    Ok(first.block_hash < second.block_hash
        && first.sender == evidence.validator
        && second.sender == evidence.validator
        && first.sender_bond_generation() == Some(evidence.bond_generation)
        && second.sender_bond_generation() == Some(evidence.bond_generation)
        && first.sequence_number == evidence.sequence_number
        && second.sequence_number == evidence.sequence_number
        && first.sequence_number >= 0)
}

fn causal_evidence_closure(
    roots: &[BlockHash],
    dag: &KeyValueDagRepresentation,
) -> Result<CausalEvidenceClosure, CasperError> {
    let mut pending = VecDeque::from(roots.to_vec());
    let mut visited = HashSet::new();
    let mut inherited = BTreeMap::new();
    let mut messages = BTreeMap::<(ValidatorIncarnation, i32), BTreeSet<BlockHash>>::new();
    while let Some(hash) = pending.pop_front() {
        if !visited.insert(hash.clone()) {
            continue;
        }
        let metadata = dag.lookup(&hash)?.ok_or_else(|| {
            CasperError::RuntimeError(format!(
                "causal evidence parent is absent from the certified DAG: {}",
                hex::encode(&hash)
            ))
        })?;
        if metadata.is_accepted() {
            for evidence in &metadata.objective_equivocation_evidence_delta {
                if !evidence_fact_is_sound(evidence, dag)? {
                    return Err(CasperError::RuntimeError(format!(
                        "accepted block {} contains unsound objective equivocation evidence",
                        hex::encode(&metadata.block_hash)
                    )));
                }
                insert_canonical_evidence(&mut inherited, evidence.clone());
            }
        }
        if let Some(generation) = metadata.sender_bond_generation() {
            if metadata.sequence_number >= 0 {
                messages
                    .entry((
                        ValidatorIncarnation::new(metadata.sender.clone(), generation),
                        metadata.sequence_number,
                    ))
                    .or_default()
                    .insert(metadata.block_hash.clone());
            }
        }
        pending.extend(metadata.parents.iter().cloned());
        pending.extend(
            metadata
                .justifications
                .iter()
                .map(|justification| justification.latest_block_hash.clone()),
        );
    }

    let mut effective = inherited.clone();
    for ((incarnation, sequence_number), hashes) in messages {
        if hashes.len() < 2 {
            continue;
        }
        let mut hashes = hashes.into_iter();
        let first_hash = hashes.next().ok_or_else(|| {
            CasperError::RuntimeError("equivocation group lost its first hash".to_string())
        })?;
        let second_hash = hashes.next().ok_or_else(|| {
            CasperError::RuntimeError("equivocation group lost its second hash".to_string())
        })?;
        let evidence = ObjectiveEquivocationEvidence::new(
            incarnation.validator,
            incarnation.generation,
            sequence_number,
            first_hash,
            second_hash,
        )
        .map_err(CasperError::RuntimeError)?;
        insert_canonical_evidence(&mut effective, evidence);
    }

    Ok(CausalEvidenceClosure {
        inherited,
        effective,
    })
}

pub fn validate_evidence_delta(
    block: &BlockMessage,
    dag: &KeyValueDagRepresentation,
) -> Result<EvidenceDeltaVerdict, CasperError> {
    let roots = block
        .header
        .parents_hash_list
        .iter()
        .chain(
            block
                .justifications
                .iter()
                .map(|justification| &justification.latest_block_hash),
        )
        .cloned()
        .collect::<Vec<_>>();
    let required = causal_evidence_closure(&roots, dag)?.required_delta();
    let actual = &block.header.objective_equivocation_evidence_delta;
    if actual == &required {
        return Ok(EvidenceDeltaVerdict::Valid);
    }
    let actual_set = actual.iter().cloned().collect::<BTreeSet<_>>();
    let required_set = required.into_iter().collect::<BTreeSet<_>>();
    if actual.len() == actual_set.len()
        && actual.windows(2).all(|window| window[0] < window[1])
        && actual_set.is_subset(&required_set)
    {
        Ok(EvidenceDeltaVerdict::Neglected)
    } else {
        Ok(EvidenceDeltaVerdict::Invalid)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvidenceDeltaVerdict {
    Valid,
    Neglected,
    Invalid,
}

pub fn proposer_evidence_delta(
    roots: &[BlockHash],
    dag: &KeyValueDagRepresentation,
) -> Result<Vec<ObjectiveEquivocationEvidence>, CasperError> {
    Ok(causal_evidence_closure(roots, dag)?.required_delta())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use block_storage::rust::dag::block_metadata_store::BlockMetadataStore;
    use block_storage::rust::dag::deploy_occurrence_store::DeployOccurrenceStore;
    use models::rust::block_metadata::{AdmissionRejectionReason, BlockMetadata};
    use models::rust::bond_generation::BondGeneration;
    use models::rust::casper::protocol::casper_message::{
        Body, F1r3flyState, Header, Justification,
    };
    use parking_lot::RwLock;
    use proptest::prelude::*;
    use prost::bytes::Bytes;
    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
    use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

    use super::*;

    #[test]
    fn evidence_identity_orders_hash_pair_canonically() {
        let evidence = ObjectiveEquivocationEvidence::new(
            Bytes::from(vec![7; models::rust::validator::LENGTH]),
            BondGeneration::GENESIS,
            3,
            Bytes::from(vec![2; models::rust::block_hash::LENGTH]),
            Bytes::from(vec![1; models::rust::block_hash::LENGTH]),
        )
        .unwrap();
        assert!(evidence.first_block_hash < evidence.second_block_hash);
    }

    fn hash(byte: u8) -> Bytes { Bytes::from(vec![byte; models::rust::block_hash::LENGTH]) }

    fn validator(byte: u8) -> Bytes { Bytes::from(vec![byte; models::rust::validator::LENGTH]) }

    fn metadata(
        block_hash: Bytes,
        parents: Vec<Bytes>,
        block_number: i64,
        sender: Bytes,
        weight_map: BTreeMap<Bytes, i64>,
    ) -> BlockMetadata {
        metadata_with_admission(
            block_hash,
            parents,
            Vec::new(),
            block_number,
            block_number as i32,
            sender,
            weight_map,
            Vec::new(),
            None,
        )
    }

    fn metadata_with_admission(
        block_hash: Bytes,
        parents: Vec<Bytes>,
        justifications: Vec<Justification>,
        block_number: i64,
        sequence_number: i32,
        sender: Bytes,
        weight_map: BTreeMap<Bytes, i64>,
        evidence_delta: Vec<ObjectiveEquivocationEvidence>,
        rejection: Option<AdmissionRejectionReason>,
    ) -> BlockMetadata {
        let active_validator_set = weight_map.keys().cloned().collect();
        let metadata = BlockMetadata {
            block_hash,
            post_state_hash: hash(250),
            parents,
            sender,
            justifications,
            bond_generation_map: weight_map
                .keys()
                .cloned()
                .map(|validator| (validator, BondGeneration::GENESIS))
                .collect(),
            weight_map,
            active_validator_set,
            block_number,
            sequence_number,
            admission_outcome: None,
            directly_finalized: false,
            finalized: false,
            fault_tolerance_value: 0.0,
            successful_state_effect_indices: BTreeSet::new(),
            rejected_state_effects: BTreeSet::new(),
            protocol_version: crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
            objective_equivocation_evidence_delta: evidence_delta,
            sender_authority: None,
            finalized_floor_commitment: None,
            admission_schema_version: models::rust::block_metadata::ADMISSION_SCHEMA_VERSION,
            approved_genesis: false,
            merge_base: Bytes::new(),
        };
        match rejection {
            Some(reason) => crate::rust::test_metadata::certify_rejected(
                metadata,
                BondGeneration::GENESIS,
                reason,
            ),
            None => crate::rust::test_metadata::certify(metadata, BondGeneration::GENESIS),
        }
    }

    fn dag(blocks: Vec<BlockMetadata>) -> KeyValueDagRepresentation {
        let metadata_store = KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new()));
        let mut metadata_index = BlockMetadataStore::new(metadata_store).unwrap();
        let mut dag_set = imbl::HashSet::new();
        let mut block_number_map = imbl::HashMap::new();
        let mut main_parent_map = imbl::HashMap::new();
        for block in blocks {
            dag_set.insert(block.block_hash.clone());
            block_number_map.insert(block.block_hash.clone(), block.block_number);
            if let Some(parent) = block.parents.first() {
                main_parent_map.insert(block.block_hash.clone(), parent.clone());
            }
            metadata_index.add(block).unwrap();
        }
        KeyValueDagRepresentation {
            dag_set,
            latest_messages_map: imbl::HashMap::new(),
            child_map: imbl::HashMap::new(),
            height_map: imbl::OrdMap::new(),
            block_number_map,
            main_parent_map,
            self_justification_map: imbl::HashMap::new(),
            invalid_blocks_set: imbl::HashSet::new(),
            equivocation_observations: imbl::HashMap::new(),
            last_finalized_block_hash: Bytes::new(),
            finalized_blocks_set: imbl::HashSet::new(),
            block_metadata_index: Arc::new(RwLock::new(metadata_index)),
            deploy_index: Arc::new(RwLock::new(KeyValueTypedStoreImpl::new(Arc::new(
                InMemoryKeyValueStore::new(),
            )))),
            deploy_occurrence_store: DeployOccurrenceStore::activate_fresh(Arc::new(
                InMemoryKeyValueStore::new(),
            ))
            .unwrap(),
            floor_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
            frontier_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
            lifecycle: Arc::new(RwLock::new(
                block_storage::rust::dag::deploy_lifecycle_types::DeployLifecycleTables::in_memory(
                ),
            )),
        }
    }

    fn candidate_block(
        block_hash: Bytes,
        sender: Bytes,
        generation: BondGeneration,
        pre_state_hash: Bytes,
        post_state_hash: Bytes,
    ) -> BlockMessage {
        BlockMessage {
            block_hash,
            header: Header {
                parents_hash_list: vec![hash(90)],
                timestamp: 0,
                version: crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
                extra_bytes: Bytes::new(),
                sender_bond_generation: Some(generation),
                objective_equivocation_evidence_delta: Vec::new(),
                finalized_floor: None,
            },
            body: Body {
                state: F1r3flyState {
                    pre_state_hash,
                    post_state_hash,
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
                applied_from_scope: Vec::new(),
                merge_base: Bytes::new(),
            },
            justifications: Vec::new(),
            sender,
            seq_num: 1,
            sig: Bytes::new(),
            sig_algorithm: String::new(),
            shard_id: "root".to_string(),
            extra_bytes: Bytes::new(),
            finalized_floor_certificate: None,
        }
    }

    #[test]
    fn certified_context_digest_is_canonical_and_commits_every_input() {
        let first_validator = validator(1);
        let second_validator = validator(2);
        let first_vote = hash(31);
        let second_vote = hash(32);
        let alternate_vote = hash(33);
        let dag = dag(vec![
            metadata(
                hash(42),
                Vec::new(),
                0,
                first_validator.clone(),
                BTreeMap::new(),
            ),
            metadata(
                hash(40),
                vec![hash(42)],
                1,
                first_validator.clone(),
                BTreeMap::new(),
            ),
            metadata(
                first_vote.clone(),
                vec![hash(40)],
                2,
                first_validator.clone(),
                BTreeMap::new(),
            ),
            metadata(
                second_vote.clone(),
                vec![hash(40)],
                2,
                second_validator.clone(),
                BTreeMap::new(),
            ),
            metadata(
                alternate_vote.clone(),
                vec![hash(40)],
                3,
                first_validator.clone(),
                BTreeMap::new(),
            ),
        ]);
        let active = BTreeSet::from([first_validator.clone(), second_validator.clone()]);
        let stakes = BTreeMap::from([
            (first_validator.clone(), 10),
            (second_validator.clone(), 20),
        ]);
        let generations = BTreeMap::from([
            (first_validator.clone(), BondGeneration::GENESIS),
            (second_validator.clone(), BondGeneration::GENESIS),
        ]);
        let mut forward_exact = BTreeMap::new();
        forward_exact.insert(first_validator.clone(), first_vote.clone());
        forward_exact.insert(second_validator.clone(), second_vote.clone());
        let mut reverse_exact = BTreeMap::new();
        reverse_exact.insert(second_validator.clone(), second_vote.clone());
        reverse_exact.insert(first_validator.clone(), first_vote.clone());

        let base = CertifiedConsensusContext::from_frozen_authority(
            &dag,
            hash(40),
            hash(41),
            &forward_exact,
            BTreeSet::new(),
            active.clone(),
            stakes.clone(),
            generations.clone(),
        )
        .unwrap();
        let reordered = CertifiedConsensusContext::from_frozen_authority(
            &dag,
            hash(40),
            hash(41),
            &reverse_exact,
            BTreeSet::new(),
            active.clone(),
            stakes.clone(),
            generations.clone(),
        )
        .unwrap();
        assert_eq!(base, reordered);

        let changed_floor = CertifiedConsensusContext::from_frozen_authority(
            &dag,
            hash(42),
            hash(41),
            &forward_exact,
            BTreeSet::new(),
            active.clone(),
            stakes.clone(),
            generations.clone(),
        )
        .unwrap();
        let changed_post_state = CertifiedConsensusContext::from_frozen_authority(
            &dag,
            hash(40),
            hash(43),
            &forward_exact,
            BTreeSet::new(),
            active.clone(),
            stakes.clone(),
            generations.clone(),
        )
        .unwrap();
        let changed_stake = CertifiedConsensusContext::from_frozen_authority(
            &dag,
            hash(40),
            hash(41),
            &forward_exact,
            BTreeSet::new(),
            active.clone(),
            BTreeMap::from([
                (first_validator.clone(), 11),
                (second_validator.clone(), 20),
            ]),
            generations.clone(),
        )
        .unwrap();
        let changed_generation = CertifiedConsensusContext::from_frozen_authority(
            &dag,
            hash(40),
            hash(41),
            &forward_exact,
            BTreeSet::new(),
            active.clone(),
            stakes.clone(),
            BTreeMap::from([
                (
                    first_validator.clone(),
                    BondGeneration::try_from(1).unwrap(),
                ),
                (second_validator.clone(), BondGeneration::GENESIS),
            ]),
        )
        .unwrap();
        let changed_latest = CertifiedConsensusContext::from_frozen_authority(
            &dag,
            hash(40),
            hash(41),
            &BTreeMap::from([
                (first_validator.clone(), alternate_vote),
                (second_validator.clone(), second_vote),
            ]),
            BTreeSet::new(),
            active.clone(),
            stakes.clone(),
            generations.clone(),
        )
        .unwrap();
        let changed_evidence = CertifiedConsensusContext::from_frozen_authority(
            &dag,
            hash(40),
            hash(41),
            &forward_exact,
            BTreeSet::from([evidence(
                &first_validator,
                BondGeneration::GENESIS,
                1,
                50,
                51,
            )]),
            active,
            stakes,
            generations,
        )
        .unwrap();

        for changed in [
            changed_floor,
            changed_post_state,
            changed_stake,
            changed_generation,
            changed_latest,
            changed_evidence,
        ] {
            assert_ne!(base.digest(), changed.digest());
        }
    }

    #[test]
    fn sender_certificate_is_bound_to_context_but_not_candidate_prestate() {
        let sender = validator(7);
        let vote = hash(60);
        let dag = dag(vec![
            metadata(hash(61), Vec::new(), 0, sender.clone(), BTreeMap::new()),
            metadata(
                vote.clone(),
                vec![hash(61)],
                1,
                sender.clone(),
                BTreeMap::new(),
            ),
        ]);
        let context = CertifiedConsensusContext::from_frozen_authority(
            &dag,
            hash(61),
            hash(62),
            &BTreeMap::from([(sender.clone(), vote)]),
            BTreeSet::new(),
            BTreeSet::from([sender.clone()]),
            BTreeMap::from([(sender.clone(), 10)]),
            BTreeMap::from([(sender.clone(), BondGeneration::GENESIS)]),
        )
        .unwrap();
        let first = candidate_block(
            hash(63),
            sender.clone(),
            BondGeneration::GENESIS,
            hash(64),
            hash(65),
        );
        let second = candidate_block(
            hash(66),
            sender.clone(),
            BondGeneration::GENESIS,
            hash(67),
            hash(68),
        );
        let first_certificate = context.certify_sender(&first).unwrap();
        let second_certificate = context.certify_sender(&second).unwrap();

        context
            .validate_certificate(&first, &first_certificate)
            .unwrap();
        context
            .validate_certificate(&second, &second_certificate)
            .unwrap();
        assert_eq!(first_certificate.context_digest(), context.digest());
        assert_eq!(second_certificate.context_digest(), context.digest());
        assert_ne!(
            first_certificate.block_hash(),
            second_certificate.block_hash()
        );

        let wrong_generation = candidate_block(
            hash(69),
            sender,
            BondGeneration::try_from(1).unwrap(),
            hash(70),
            hash(71),
        );
        assert!(context.certify_sender(&wrong_generation).is_err());
    }

    #[test]
    fn missing_active_validator_latest_message_is_explicitly_incomplete() {
        let first_validator = validator(10);
        let second_validator = validator(11);
        let first_vote = hash(72);
        let dag = dag(vec![
            metadata(
                hash(73),
                Vec::new(),
                0,
                first_validator.clone(),
                BTreeMap::new(),
            ),
            metadata(
                first_vote.clone(),
                vec![hash(73)],
                1,
                first_validator.clone(),
                BTreeMap::new(),
            ),
        ]);
        let context = CertifiedConsensusContext::from_frozen_authority(
            &dag,
            hash(73),
            hash(74),
            &BTreeMap::from([(first_validator.clone(), first_vote)]),
            BTreeSet::new(),
            BTreeSet::from([first_validator.clone(), second_validator.clone()]),
            BTreeMap::from([
                (first_validator.clone(), 10),
                (second_validator.clone(), 20),
            ]),
            BTreeMap::from([
                (first_validator, BondGeneration::GENESIS),
                (second_validator, BondGeneration::GENESIS),
            ]),
        )
        .unwrap();

        assert!(!context.has_complete_latest_message_slots());
    }

    #[test]
    fn frozen_finalization_projection_preserves_slots_but_excludes_ineligible_votes() {
        let honest = validator(12);
        let rejected = validator(13);
        let floor = hash(75);
        let honest_vote = hash(76);
        let rejected_vote = hash(77);
        let stakes = BTreeMap::from([(honest.clone(), 30), (rejected.clone(), 10)]);
        let dag = dag(vec![
            metadata(floor.clone(), Vec::new(), 0, honest.clone(), stakes),
            metadata(
                honest_vote.clone(),
                vec![floor.clone()],
                1,
                honest.clone(),
                BTreeMap::new(),
            ),
            metadata_with_admission(
                rejected_vote.clone(),
                vec![floor.clone()],
                Vec::new(),
                1,
                1,
                rejected.clone(),
                BTreeMap::new(),
                Vec::new(),
                Some(AdmissionRejectionReason::InvalidTransaction),
            ),
        ]);
        let exact = BTreeMap::from([
            (honest.clone(), honest_vote.clone()),
            (rejected.clone(), rejected_vote),
        ]);
        let context = CertifiedConsensusContext::for_frozen_floor(&dag, floor, &exact).unwrap();

        assert!(context.has_complete_latest_message_slots());
        assert_eq!(context.vote_projection().exact_latest_messages(), &exact);
        assert_eq!(
            context.vote_projection().eligible_latest_messages(),
            &BTreeMap::from([(honest.clone(), honest_vote.clone())])
        );
        assert_eq!(
            context.vote_projection().exclusions().get(&rejected),
            Some(&VoteExclusion::IntrinsicallyInvalid)
        );
        assert_eq!(
            context
                .causal_parent_projection()
                .eligible_latest_messages(),
            &BTreeMap::from([(honest.clone(), honest_vote.clone())])
        );
        assert_eq!(
            context
                .causal_parent_projection()
                .exclusions()
                .get(&rejected),
            Some(&CausalParentExclusion::IntrinsicallyInvalid)
        );
    }

    #[test]
    fn stale_intrinsically_invalid_latest_is_excluded_from_both_projections() {
        let first = validator(14);
        let second = validator(15);
        let root = hash(78);
        let floor = hash(79);
        let rejected_stale = hash(80);
        let stakes = BTreeMap::from([(first.clone(), 30), (second.clone(), 10)]);
        let dag = dag(vec![
            metadata(root.clone(), Vec::new(), 0, first.clone(), stakes.clone()),
            metadata(floor.clone(), vec![root.clone()], 1, first.clone(), stakes),
            metadata_with_admission(
                rejected_stale.clone(),
                vec![root],
                Vec::new(),
                1,
                1,
                second.clone(),
                BTreeMap::new(),
                Vec::new(),
                Some(AdmissionRejectionReason::InvalidTransaction),
            ),
        ]);
        let exact = BTreeMap::from([
            (first.clone(), floor.clone()),
            (second.clone(), rejected_stale),
        ]);
        let context = CertifiedConsensusContext::for_frozen_floor(&dag, floor, &exact).unwrap();

        assert_eq!(
            context.vote_projection().exclusions().get(&second),
            Some(&VoteExclusion::IntrinsicallyInvalid)
        );
        assert_eq!(
            context.causal_parent_projection().exclusions().get(&second),
            Some(&CausalParentExclusion::IntrinsicallyInvalid)
        );
        assert!(context
            .vote_projection()
            .eligible_latest_messages()
            .keys()
            .all(|validator| context
                .causal_parent_projection()
                .eligible_latest_messages()
                .contains_key(validator)));
    }

    #[test]
    fn finalized_floor_evidence_excludes_an_offender_when_every_latest_message_is_stale() {
        let offender = validator(32);
        let honest = validator(33);
        let root = hash(220);
        let first = hash(221);
        let second = hash(222);
        let honest_tip = hash(223);
        let floor = hash(224);
        let evidence = evidence(&offender, BondGeneration::GENESIS, 7, 221, 222);
        let stakes = BTreeMap::from([(offender.clone(), 10), (honest.clone(), 30)]);
        let dag = dag(vec![
            metadata(root.clone(), Vec::new(), 0, honest.clone(), BTreeMap::new()),
            metadata_with_admission(
                first.clone(),
                vec![root.clone()],
                Vec::new(),
                1,
                7,
                offender.clone(),
                BTreeMap::new(),
                Vec::new(),
                None,
            ),
            metadata_with_admission(
                second,
                vec![root.clone()],
                Vec::new(),
                1,
                7,
                offender.clone(),
                BTreeMap::new(),
                Vec::new(),
                None,
            ),
            metadata_with_admission(
                honest_tip.clone(),
                vec![root.clone()],
                Vec::new(),
                1,
                1,
                honest.clone(),
                BTreeMap::new(),
                Vec::new(),
                None,
            ),
            metadata_with_admission(
                floor.clone(),
                vec![root],
                Vec::new(),
                2,
                2,
                honest.clone(),
                stakes,
                vec![evidence.clone()],
                None,
            ),
        ]);
        let exact = BTreeMap::from([(offender.clone(), first), (honest.clone(), honest_tip)]);

        let context = CertifiedConsensusContext::for_frozen_floor(&dag, floor, &exact)
            .expect("floor-rooted certified context");

        assert_eq!(context.incoming_evidence(), &BTreeSet::from([evidence]));
        assert_eq!(
            context
                .causal_parent_projection()
                .exclusions()
                .get(&offender),
            Some(&CausalParentExclusion::ObjectiveEquivocation)
        );
        assert_eq!(
            context.vote_projection().exclusions().get(&offender),
            Some(&VoteExclusion::ObjectiveEquivocation)
        );
        assert_eq!(
            context
                .causal_parent_projection()
                .eligible_latest_messages()
                .get(&honest),
            exact.get(&honest)
        );
        assert_eq!(
            context.vote_projection().exclusions().get(&honest),
            Some(&VoteExclusion::DoesNotDescendFromFloor)
        );
    }

    #[test]
    fn parent_order_cannot_change_frozen_floor_committee() {
        let old_validator = validator(1);
        let active_validator = validator(2);
        let floor_zero = hash(10);
        let floor_one = hash(11);
        let old_parent = hash(12);
        let active_parent = hash(13);
        let active_vote = hash(14);
        let dag = dag(vec![
            metadata(
                floor_zero.clone(),
                Vec::new(),
                0,
                old_validator.clone(),
                BTreeMap::from([(old_validator.clone(), 10)]),
            ),
            metadata(
                floor_one.clone(),
                vec![floor_zero.clone()],
                1,
                active_validator.clone(),
                BTreeMap::from([(active_validator.clone(), 20)]),
            ),
            metadata(
                old_parent.clone(),
                vec![floor_zero.clone()],
                2,
                old_validator,
                BTreeMap::new(),
            ),
            metadata(
                active_parent.clone(),
                vec![floor_one.clone()],
                2,
                active_validator.clone(),
                BTreeMap::new(),
            ),
            metadata(
                active_vote.clone(),
                vec![floor_one.clone()],
                3,
                active_validator.clone(),
                BTreeMap::new(),
            ),
        ]);
        dag.put_cached_floor(old_parent.clone(), floor_zero)
            .unwrap();
        dag.put_cached_floor(active_parent.clone(), floor_one.clone())
            .unwrap();
        let exact = BTreeMap::from([(active_validator.clone(), active_vote)]);

        let forward = CertifiedConsensusContext::for_parents(
            &dag,
            &[old_parent.clone(), active_parent.clone()],
            &exact,
        )
        .unwrap();
        let reverse =
            CertifiedConsensusContext::for_parents(&dag, &[active_parent, old_parent], &exact)
                .unwrap();

        assert_eq!(forward, reverse);
        assert_eq!(forward.incoming_finalized_floor(), &floor_one);
        assert_eq!(forward.vote_projection().eligible_latest_messages(), &exact);
    }

    #[test]
    fn incompatible_parent_floors_fail_closed() {
        let first_validator = validator(1);
        let second_validator = validator(2);
        let genesis = hash(20);
        let first_floor = hash(21);
        let second_floor = hash(22);
        let first_parent = hash(23);
        let second_parent = hash(24);
        let dag = dag(vec![
            metadata(
                genesis.clone(),
                Vec::new(),
                0,
                first_validator.clone(),
                BTreeMap::new(),
            ),
            metadata(
                first_floor.clone(),
                vec![genesis.clone()],
                1,
                first_validator.clone(),
                BTreeMap::new(),
            ),
            metadata(
                second_floor.clone(),
                vec![genesis],
                1,
                second_validator.clone(),
                BTreeMap::new(),
            ),
            metadata(
                first_parent.clone(),
                vec![first_floor.clone()],
                2,
                first_validator,
                BTreeMap::new(),
            ),
            metadata(
                second_parent.clone(),
                vec![second_floor.clone()],
                2,
                second_validator,
                BTreeMap::new(),
            ),
        ]);
        dag.put_cached_floor(first_parent.clone(), first_floor)
            .unwrap();
        dag.put_cached_floor(second_parent.clone(), second_floor)
            .unwrap();

        assert!(CertifiedConsensusContext::for_parents(
            &dag,
            &[first_parent, second_parent],
            &BTreeMap::new(),
        )
        .is_err());
    }

    fn evidence(
        validator: &Bytes,
        generation: BondGeneration,
        sequence: i32,
        first: u8,
        second: u8,
    ) -> ObjectiveEquivocationEvidence {
        ObjectiveEquivocationEvidence::new(
            validator.clone(),
            generation,
            sequence,
            Bytes::from(vec![first; models::rust::block_hash::LENGTH]),
            Bytes::from(vec![second; models::rust::block_hash::LENGTH]),
        )
        .unwrap()
    }

    fn context(
        active_generation: BondGeneration,
        incoming_evidence: BTreeSet<ObjectiveEquivocationEvidence>,
    ) -> CertifiedConsensusContext {
        let equivocator = Bytes::from(vec![7; models::rust::validator::LENGTH]);
        let honest = Bytes::from(vec![8; models::rust::validator::LENGTH]);
        let authority_stakes = BTreeMap::from([(equivocator.clone(), 10), (honest.clone(), 30)]);
        let authority_generations = BTreeMap::from([
            (equivocator.clone(), active_generation),
            (honest.clone(), BondGeneration::GENESIS),
        ]);
        CertifiedConsensusContext::from_frozen_authority(
            &dag(Vec::new()),
            hash(0),
            hash(1),
            &BTreeMap::new(),
            incoming_evidence,
            BTreeSet::from([equivocator, honest]),
            authority_stakes,
            authority_generations,
        )
        .unwrap()
    }

    #[test]
    fn causal_parent_and_justification_closure_requires_exact_canonical_evidence() {
        let offender = validator(20);
        let observer = validator(21);
        let first_hash = hash(30);
        let second_hash = hash(31);
        let root_hash = hash(32);
        let first = metadata_with_admission(
            first_hash.clone(),
            Vec::new(),
            Vec::new(),
            1,
            7,
            offender.clone(),
            BTreeMap::new(),
            Vec::new(),
            None,
        );
        let second = metadata_with_admission(
            second_hash.clone(),
            Vec::new(),
            Vec::new(),
            1,
            7,
            offender.clone(),
            BTreeMap::new(),
            Vec::new(),
            None,
        );
        let root = metadata_with_admission(
            root_hash.clone(),
            vec![first_hash.clone()],
            vec![Justification {
                validator: offender.clone(),
                latest_block_hash: second_hash.clone(),
            }],
            2,
            2,
            observer.clone(),
            BTreeMap::new(),
            Vec::new(),
            None,
        );
        let dag = dag(vec![first, second, root]);
        let expected = evidence(&offender, BondGeneration::GENESIS, 7, 30, 31);

        assert_eq!(
            proposer_evidence_delta(std::slice::from_ref(&root_hash), &dag).unwrap(),
            vec![expected.clone()]
        );

        let mut candidate = candidate_block(
            hash(33),
            observer,
            BondGeneration::GENESIS,
            hash(34),
            hash(35),
        );
        candidate.header.parents_hash_list = vec![root_hash];
        candidate.header.objective_equivocation_evidence_delta = vec![expected.clone()];
        assert_eq!(
            validate_evidence_delta(&candidate, &dag).unwrap(),
            EvidenceDeltaVerdict::Valid
        );

        candidate
            .header
            .objective_equivocation_evidence_delta
            .clear();
        assert_eq!(
            validate_evidence_delta(&candidate, &dag).unwrap(),
            EvidenceDeltaVerdict::Neglected
        );

        candidate.header.objective_equivocation_evidence_delta =
            vec![evidence(&offender, BondGeneration::GENESIS, 8, 36, 37)];
        assert_eq!(
            validate_evidence_delta(&candidate, &dag).unwrap(),
            EvidenceDeltaVerdict::Invalid
        );
    }

    #[test]
    fn rejected_nodes_are_structural_but_never_propagate_their_evidence_delta() {
        let offender = validator(22);
        let wrapper_sender = validator(23);
        let first_hash = hash(40);
        let second_hash = hash(41);
        let rejected_wrapper_hash = hash(42);
        let accepted_root_hash = hash(43);
        let first = metadata_with_admission(
            first_hash.clone(),
            Vec::new(),
            Vec::new(),
            1,
            3,
            offender.clone(),
            BTreeMap::new(),
            Vec::new(),
            None,
        );
        let second = metadata_with_admission(
            second_hash.clone(),
            Vec::new(),
            Vec::new(),
            1,
            3,
            offender.clone(),
            BTreeMap::new(),
            Vec::new(),
            Some(AdmissionRejectionReason::InvalidTransaction),
        );
        let pair = evidence(&offender, BondGeneration::GENESIS, 3, 40, 41);
        let rejected_wrapper = metadata_with_admission(
            rejected_wrapper_hash.clone(),
            vec![first_hash, second_hash],
            Vec::new(),
            2,
            2,
            wrapper_sender.clone(),
            BTreeMap::new(),
            vec![pair.clone()],
            Some(AdmissionRejectionReason::InvalidTransaction),
        );
        let accepted_root = metadata_with_admission(
            accepted_root_hash.clone(),
            vec![rejected_wrapper_hash],
            Vec::new(),
            3,
            3,
            wrapper_sender,
            BTreeMap::new(),
            Vec::new(),
            None,
        );
        let dag = dag(vec![first, second, rejected_wrapper, accepted_root]);
        let closure =
            causal_evidence_closure(std::slice::from_ref(&accepted_root_hash), &dag).unwrap();

        assert!(closure.inherited.is_empty());
        assert_eq!(closure.required_delta(), vec![pair]);
    }

    #[test]
    fn proof_roots_are_leaf_facts_and_do_not_import_their_own_context() {
        let first_offender = validator(24);
        let second_offender = validator(25);
        let first_pair = evidence(&first_offender, BondGeneration::GENESIS, 4, 50, 51);
        let second_pair = evidence(&second_offender, BondGeneration::GENESIS, 5, 52, 53);
        let second_left = metadata_with_admission(
            hash(52),
            Vec::new(),
            Vec::new(),
            1,
            5,
            second_offender.clone(),
            BTreeMap::new(),
            Vec::new(),
            None,
        );
        let second_right = metadata_with_admission(
            hash(53),
            Vec::new(),
            Vec::new(),
            1,
            5,
            second_offender,
            BTreeMap::new(),
            Vec::new(),
            None,
        );
        let first_left = metadata_with_admission(
            hash(50),
            Vec::new(),
            Vec::new(),
            1,
            4,
            first_offender.clone(),
            BTreeMap::new(),
            vec![second_pair.clone()],
            None,
        );
        let first_right = metadata_with_admission(
            hash(51),
            Vec::new(),
            Vec::new(),
            1,
            4,
            first_offender,
            BTreeMap::new(),
            Vec::new(),
            None,
        );
        let root_hash = hash(54);
        let root = metadata_with_admission(
            root_hash.clone(),
            Vec::new(),
            Vec::new(),
            2,
            2,
            validator(26),
            BTreeMap::new(),
            vec![first_pair.clone()],
            None,
        );
        let dag = dag(vec![
            second_left,
            second_right,
            first_left,
            first_right,
            root,
        ]);

        assert_eq!(
            effective_evidence_context(std::slice::from_ref(&root_hash), &dag).unwrap(),
            BTreeSet::from([first_pair])
        );
        assert!(!effective_evidence_context(&[root_hash], &dag)
            .unwrap()
            .contains(&second_pair));
    }

    #[test]
    fn evidence_join_is_arrival_order_independent_and_bounded_per_incarnation() {
        let offender = validator(27);
        let roots = vec![hash(60), hash(61), hash(62), hash(63)];
        let blocks = vec![
            metadata_with_admission(
                roots[0].clone(),
                Vec::new(),
                Vec::new(),
                1,
                9,
                offender.clone(),
                BTreeMap::new(),
                Vec::new(),
                None,
            ),
            metadata_with_admission(
                roots[1].clone(),
                Vec::new(),
                Vec::new(),
                1,
                9,
                offender.clone(),
                BTreeMap::new(),
                Vec::new(),
                None,
            ),
            metadata_with_admission(
                roots[2].clone(),
                Vec::new(),
                Vec::new(),
                1,
                2,
                offender.clone(),
                BTreeMap::new(),
                Vec::new(),
                None,
            ),
            metadata_with_admission(
                roots[3].clone(),
                Vec::new(),
                Vec::new(),
                1,
                2,
                offender.clone(),
                BTreeMap::new(),
                Vec::new(),
                None,
            ),
        ];
        let dag = dag(blocks);
        let mut reverse_roots = roots.clone();
        reverse_roots.reverse();
        let expected = vec![evidence(&offender, BondGeneration::GENESIS, 2, 62, 63)];

        assert_eq!(proposer_evidence_delta(&roots, &dag).unwrap(), expected);
        assert_eq!(
            proposer_evidence_delta(&reverse_roots, &dag).unwrap(),
            expected
        );
    }

    #[test]
    fn ambient_equivocation_tracker_state_cannot_change_consensus_evidence() {
        let offender = validator(28);
        let first_hash = hash(70);
        let second_hash = hash(71);
        let base = dag(vec![
            metadata_with_admission(
                first_hash.clone(),
                Vec::new(),
                Vec::new(),
                1,
                6,
                offender.clone(),
                BTreeMap::new(),
                Vec::new(),
                None,
            ),
            metadata_with_admission(
                second_hash.clone(),
                Vec::new(),
                Vec::new(),
                1,
                6,
                offender.clone(),
                BTreeMap::new(),
                Vec::new(),
                None,
            ),
        ]);
        let mut with_ambient_hint = base.clone();
        with_ambient_hint.equivocation_observations.insert(
            (offender, BondGeneration::GENESIS, 6),
            BTreeSet::from([first_hash, second_hash]),
        );

        assert_eq!(
            proposer_evidence_delta(&[hash(70), hash(71)], &base).unwrap(),
            proposer_evidence_delta(&[hash(70), hash(71)], &with_ambient_hint).unwrap()
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        #[test]
        fn causal_parent_and_vote_projections_match_the_reference_predicates(
            stale in any::<bool>(),
            rejected in any::<bool>(),
            wrong_sender in any::<bool>(),
            wrong_generation in any::<bool>(),
            equivocating in any::<bool>(),
            stake_present in any::<bool>(),
            stake_positive in any::<bool>(),
            generation_present in any::<bool>(),
        ) {
            let validator_key = validator(30);
            let sender = if wrong_sender { validator(31) } else { validator_key.clone() };
            let root = hash(200);
            let floor = hash(201);
            let tip = hash(202);
            let authority_generation = if wrong_generation {
                BondGeneration::try_from(1).unwrap()
            } else {
                BondGeneration::GENESIS
            };
            let tip_metadata = metadata_with_admission(
                tip.clone(),
                vec![if stale { root.clone() } else { floor.clone() }],
                Vec::new(),
                2,
                2,
                sender,
                BTreeMap::new(),
                Vec::new(),
                rejected.then_some(AdmissionRejectionReason::InvalidTransaction),
            );
            let dag = dag(vec![
                metadata(root.clone(), Vec::new(), 0, validator_key.clone(), BTreeMap::new()),
                metadata(
                    floor.clone(),
                    vec![root],
                    1,
                    validator_key.clone(),
                    BTreeMap::new(),
                ),
                tip_metadata,
            ]);
            let exact = BTreeMap::from([(validator_key.clone(), tip.clone())]);
            let stakes = if stake_present {
                BTreeMap::from([(validator_key.clone(), if stake_positive { 10 } else { 0 })])
            } else {
                BTreeMap::new()
            };
            let generations = if generation_present {
                BTreeMap::from([(validator_key.clone(), authority_generation)])
            } else {
                BTreeMap::new()
            };
            let evidence = if equivocating && generation_present {
                BTreeSet::from([evidence(&validator_key, authority_generation, 2, 210, 211)])
            } else {
                BTreeSet::new()
            };
            let (causal, votes) = derive_consensus_projections(
                &dag,
                &floor,
                &exact,
                evidence,
                &stakes,
                &generations,
            )
            .unwrap();
            let base_admissible = stake_present
                && stake_positive
                && generation_present
                && !rejected
                && !wrong_sender
                && !wrong_generation
                && !equivocating;

            prop_assert_eq!(
                causal.eligible_latest_messages().contains_key(&validator_key),
                base_admissible
            );
            prop_assert_eq!(
                votes.eligible_latest_messages().contains_key(&validator_key),
                base_admissible && !stale
            );
            prop_assert!(votes
                .eligible_latest_messages()
                .keys()
                .all(|candidate| causal.eligible_latest_messages().contains_key(candidate)));
        }

        #[test]
        fn canonical_evidence_join_is_associative_commutative_idempotent_and_bounded(
            entries in proptest::collection::vec(
                (0i32..256, any::<u8>(), any::<u8>())
                    .prop_filter("evidence hashes must differ", |(_, first, second)| first != second),
                1..64,
            )
        ) {
            let offender = validator(29);
            let evidence = entries
                .into_iter()
                .map(|(sequence, first, second)| {
                    ObjectiveEquivocationEvidence::new(
                        offender.clone(),
                        BondGeneration::GENESIS,
                        sequence,
                        hash(first),
                        hash(second),
                    )
                    .unwrap()
                })
                .collect::<Vec<_>>();
            let fold = |items: &[ObjectiveEquivocationEvidence]| {
                let mut result = BTreeMap::new();
                for item in items {
                    insert_canonical_evidence(&mut result, item.clone());
                }
                result
            };

            let forward = fold(&evidence);
            let mut reversed = evidence.clone();
            reversed.reverse();
            let reverse = fold(&reversed);
            prop_assert_eq!(&forward, &reverse);
            prop_assert_eq!(forward.len(), 1);

            let split = evidence.len() / 2;
            let mut combined = fold(&evidence[..split]);
            for item in fold(&evidence[split..]).into_values() {
                insert_canonical_evidence(&mut combined, item);
            }
            prop_assert_eq!(&forward, &combined);

            let mut duplicated = evidence.clone();
            duplicated.extend(evidence);
            prop_assert_eq!(&forward, &fold(&duplicated));
        }
    }

    #[test]
    fn normalized_fault_counts_active_validator_generation_once() {
        let validator = Bytes::from(vec![7; models::rust::validator::LENGTH]);
        let generation = BondGeneration::try_from(3).unwrap();
        let evidence = BTreeSet::from([
            evidence(&validator, generation, 4, 1, 2),
            evidence(&validator, generation, 9, 3, 4),
        ]);

        assert_eq!(
            context(generation, evidence).normalized_initial_fault(),
            0.25
        );
    }

    #[test]
    fn normalized_fault_ignores_stale_validator_generation() {
        let validator = Bytes::from(vec![7; models::rust::validator::LENGTH]);
        let active_generation = BondGeneration::try_from(3).unwrap();
        let stale_generation = BondGeneration::try_from(2).unwrap();
        let evidence = BTreeSet::from([evidence(&validator, stale_generation, 4, 1, 2)]);

        assert_eq!(
            context(active_generation, evidence).normalized_initial_fault(),
            0.0
        );
    }
}
