use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Weak};

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use models::rust::block_hash::{BlockHash, BlockHashSerde};
use models::rust::casper::protocol::casper_message::{
    BlockMessage, FinalizationCertificate, FinalizedFloorCommitment,
};
use models::rust::validator::{Validator, ValidatorSerde};
use parking_lot::Mutex;
use prost::bytes::Bytes;
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard, OwnedSemaphorePermit, Semaphore};

use crate::rust::casper_conf::FinalizerConf;
use crate::rust::errors::CasperError;
use crate::rust::safety::clique_oracle::FtThreshold;

#[derive(Debug, thiserror::Error)]
pub enum FinalizationCertificateError {
    #[error("invalid finalization certificate: {0}")]
    Invalid(String),
    #[error("missing finalization certificate dependency {0}")]
    MissingDependency(String),
    #[error("finalization certificate verification failed locally: {0}")]
    Local(#[from] CasperError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PredecessorCertificateCarrier {
    pub block_hash: BlockHash,
    pub certificate_digest: BlockHash,
}

pub struct CertificateVerificationSchedule {
    digests: Mutex<HashMap<BlockHash, Weak<AsyncMutex<()>>>>,
    workers: Arc<Semaphore>,
    worker_limit: usize,
}

#[derive(Debug)]
struct CertificateVerificationPermit {
    _digest: OwnedMutexGuard<()>,
    _worker: OwnedSemaphorePermit,
}

impl CertificateVerificationSchedule {
    pub fn new(worker_limit: usize) -> Self {
        assert!(
            worker_limit > 0,
            "certificate verification worker limit must be at least one"
        );
        Self {
            digests: Mutex::new(HashMap::new()),
            workers: Arc::new(Semaphore::new(worker_limit)),
            worker_limit,
        }
    }

    async fn acquire(&self, digest: &BlockHash) -> Result<CertificateVerificationPermit, String> {
        let digest_lock = {
            let mut digests = self.digests.lock();
            digests.retain(|_, lock| lock.strong_count() > 0);
            if let Some(lock) = digests.get(digest).and_then(Weak::upgrade) {
                lock
            } else {
                let lock = Arc::new(AsyncMutex::new(()));
                digests.insert(digest.clone(), Arc::downgrade(&lock));
                lock
            }
        };
        let digest_guard = digest_lock.lock_owned().await;
        let worker = self
            .workers
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| "certificate verification worker pool is closed".to_string())?;
        Ok(CertificateVerificationPermit {
            _digest: digest_guard,
            _worker: worker,
        })
    }

    pub const fn worker_limit(&self) -> usize { self.worker_limit }
}

fn invalid(message: impl Into<String>) -> FinalizationCertificateError {
    FinalizationCertificateError::Invalid(message.into())
}

pub fn genesis_finalization_certificate(
    dag: &KeyValueDagRepresentation,
    genesis: &BlockMessage,
    protocol_version: i64,
    shard_id: String,
    fault_tolerance_numerator: i64,
    fault_tolerance_denominator: i64,
) -> Result<FinalizationCertificate, CasperError> {
    let active_validators = genesis
        .body
        .state
        .active_validators
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let authority_stakes = genesis
        .body
        .state
        .bonds
        .iter()
        .filter(|bond| active_validators.contains(&bond.validator) && bond.stake > 0)
        .map(|bond| (bond.validator.clone(), bond.stake))
        .collect::<BTreeMap<_, _>>();
    let authority_generations = genesis
        .body
        .state
        .bond_generations
        .iter()
        .filter(|entry| active_validators.contains(&entry.validator))
        .map(|entry| (entry.validator.clone(), entry.generation))
        .collect::<BTreeMap<_, _>>();
    let exact_latest_messages = active_validators
        .iter()
        .map(|validator| (validator.clone(), genesis.block_hash.clone()))
        .collect::<BTreeMap<_, _>>();
    let context =
        crate::rust::causal_equivocation::CertifiedConsensusContext::from_frozen_authority(
            dag,
            genesis.block_hash.clone(),
            genesis.body.state.post_state_hash.clone(),
            &exact_latest_messages,
            BTreeSet::new(),
            active_validators,
            authority_stakes,
            authority_generations,
        )?;
    let zero = BlockHashSerde(Bytes::from(vec![0; models::rust::block_hash::LENGTH]));
    let target = BlockHashSerde(genesis.block_hash.clone());
    let certificate = FinalizationCertificate {
        schema_version: FinalizationCertificate::SCHEMA_VERSION,
        protocol_version,
        shard_id,
        genesis_hash: target.clone(),
        predecessor_floor_hash: target.clone(),
        predecessor_certificate_digest: zero.clone(),
        predecessor_certificate_block_hash: zero,
        target_floor_hash: target.clone(),
        target_post_state_hash: BlockHashSerde(genesis.body.state.post_state_hash.clone()),
        target_block_number: genesis.body.state.block_number,
        fault_tolerance_numerator,
        fault_tolerance_denominator,
        exact_latest_messages: exact_latest_messages
            .into_iter()
            .map(|(validator, hash)| (ValidatorSerde(validator), BlockHashSerde(hash)))
            .collect(),
        authority_context_digest: BlockHashSerde(context.digest().clone()),
        supporting_manifest_digest: FinalizationCertificate::supporting_digest(&BTreeSet::from([
            target.clone(),
        ])),
        finalized_manifest_digest: FinalizationCertificate::finalized_digest(&BTreeSet::from([
            target,
        ])),
        supporting_block_count: 1,
        finalized_block_count: 1,
    };
    certificate
        .validate_shape()
        .map_err(CasperError::RuntimeError)?;
    Ok(certificate)
}

fn required_metadata(
    dag: &KeyValueDagRepresentation,
    hash: &BlockHash,
) -> Result<models::rust::block_metadata::BlockMetadata, FinalizationCertificateError> {
    dag.lookup(hash)
        .map_err(CasperError::from)?
        .ok_or_else(|| FinalizationCertificateError::MissingDependency(hex::encode(hash)))
}

fn effective_parent_floor(
    parent: &BlockHash,
    dag: &KeyValueDagRepresentation,
    approved_genesis: &BlockMessage,
) -> Result<(BlockHash, BlockHash), FinalizationCertificateError> {
    let metadata = required_metadata(dag, parent)?;
    if !metadata.is_accepted() {
        return Err(invalid("candidate parent was not admitted"));
    }
    if metadata.approved_genesis {
        if metadata.block_hash != approved_genesis.block_hash {
            return Err(invalid(
                "candidate parent claims a different approved genesis",
            ));
        }
        return Ok((metadata.block_hash, metadata.post_state_hash));
    }
    let commitment = metadata
        .finalized_floor_commitment
        .as_ref()
        .ok_or_else(|| invalid("candidate parent has no durable finalized-floor commitment"))?;
    commitment.validate_shape().map_err(invalid)?;
    let floor_metadata = required_metadata(dag, &commitment.floor_hash)?;
    if !floor_metadata.is_accepted()
        || floor_metadata.post_state_hash != commitment.floor_post_state_hash
    {
        return Err(invalid(
            "candidate parent finalized-floor commitment does not bind admitted state",
        ));
    }
    Ok((
        commitment.floor_hash.clone(),
        commitment.floor_post_state_hash.clone(),
    ))
}

fn parent_floor_frontier_is_valid<E>(
    committed_floor: &BlockHash,
    parent_floors: &[BlockHash],
    mut is_ancestor: impl FnMut(&BlockHash, &BlockHash) -> Result<bool, E>,
    mut state_preserves: impl FnMut(&BlockHash, &BlockHash) -> Result<bool, E>,
) -> Result<bool, E> {
    for parent_floor in parent_floors {
        if !is_ancestor(parent_floor, committed_floor)?
            || !state_preserves(parent_floor, committed_floor)?
        {
            return Ok(false);
        }
    }
    for left in 0..parent_floors.len() {
        for right in (left + 1)..parent_floors.len() {
            if !is_ancestor(&parent_floors[left], &parent_floors[right])?
                && !is_ancestor(&parent_floors[right], &parent_floors[left])?
            {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn state_is_preserved(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    predecessor: &BlockHash,
    target: &BlockHash,
) -> Result<bool, CasperError> {
    let predecessor_metadata = dag
        .lookup(predecessor)?
        .ok_or_else(|| CasperError::BlockNotHeld(predecessor.clone()))?;
    let target_metadata = dag
        .lookup(target)?
        .ok_or_else(|| CasperError::BlockNotHeld(target.clone()))?;
    let predecessor_floor = crate::rust::finality::floor::Floor {
        hash: predecessor.clone(),
        block_number: predecessor_metadata.block_number,
    };
    let target_floor = crate::rust::finality::floor::Floor {
        hash: target.clone(),
        block_number: target_metadata.block_number,
    };
    let mut memo = std::collections::HashMap::new();
    crate::rust::finality::floor::state_contains(
        dag,
        block_store,
        &target_floor,
        &predecessor_floor,
        &mut memo,
    )
}

pub(crate) fn validate_candidate_parent_frontier(
    parents: &[BlockHash],
    commitment: &FinalizedFloorCommitment,
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    approved_genesis: &BlockMessage,
) -> Result<(), FinalizationCertificateError> {
    if parents.is_empty() {
        return Err(invalid("certified non-genesis block has no causal parent"));
    }
    let committed_floor_metadata = required_metadata(dag, &commitment.floor_hash)?;
    if !committed_floor_metadata.is_accepted()
        || committed_floor_metadata.post_state_hash != commitment.floor_post_state_hash
    {
        return Err(invalid(
            "candidate finalized-floor commitment does not bind admitted state",
        ));
    }

    let mut parent_floors = Vec::with_capacity(parents.len());
    let mut floor_is_causal_input = false;
    for parent in parents {
        let (parent_floor, _) = effective_parent_floor(parent, dag, approved_genesis)?;
        if dag
            .is_dag_ancestor(&commitment.floor_hash, parent)
            .map_err(CasperError::from)?
        {
            floor_is_causal_input = true;
        }
        parent_floors.push(parent_floor);
    }
    if !parent_floor_frontier_is_valid(
        &commitment.floor_hash,
        &parent_floors,
        |left, right| dag.is_dag_ancestor(left, right).map_err(CasperError::from),
        |left, right| state_is_preserved(dag, block_store, left, right),
    )? {
        return Err(invalid(
            "candidate finalized floor does not preserve one comparable parent-floor chain",
        ));
    }
    if !floor_is_causal_input {
        return Err(invalid(
            "candidate finalized floor is absent from the causal parent frontier",
        ));
    }
    Ok(())
}

pub(crate) fn select_predecessor_certificate_carrier(
    support: &BTreeSet<BlockHashSerde>,
    target: &BlockHash,
    predecessor_floor: &BlockHash,
    predecessor_post_state: &BlockHash,
    protocol_version: i64,
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    approved_genesis: &BlockMessage,
) -> Result<Option<PredecessorCertificateCarrier>, FinalizationCertificateError> {
    for block_hash in support {
        if block_hash.0 == *target {
            continue;
        }
        let metadata = required_metadata(dag, &block_hash.0)?;
        metadata
            .validate()
            .map_err(|error| invalid(error.to_string()))?;
        if metadata.approved_genesis
            || !metadata.is_accepted()
            || metadata.protocol_version != protocol_version
        {
            continue;
        }
        let Some(commitment) = metadata.finalized_floor_commitment.as_ref() else {
            continue;
        };
        commitment.validate_shape().map_err(invalid)?;
        if commitment.floor_hash != *predecessor_floor
            || commitment.floor_post_state_hash != *predecessor_post_state
        {
            continue;
        }
        validate_candidate_parent_frontier(
            &metadata.parents,
            commitment,
            dag,
            block_store,
            approved_genesis,
        )?;
        return Ok(Some(PredecessorCertificateCarrier {
            block_hash: block_hash.0.clone(),
            certificate_digest: commitment.certificate_digest.clone(),
        }));
    }
    Ok(None)
}

fn validate_candidate_use(
    block: &BlockMessage,
    commitment: &FinalizedFloorCommitment,
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    approved_genesis: &BlockMessage,
) -> Result<(), FinalizationCertificateError> {
    validate_candidate_parent_frontier(
        &block.header.parents_hash_list,
        commitment,
        dag,
        block_store,
        approved_genesis,
    )
}

fn validate_accepted_predecessor_anchor(
    certificate: &FinalizationCertificate,
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    approved_genesis: &BlockMessage,
    expected_protocol_version: i64,
) -> Result<(), FinalizationCertificateError> {
    let has_genesis_anchor = certificate
        .predecessor_certificate_digest
        .0
        .iter()
        .all(|byte| *byte == 0);
    if has_genesis_anchor {
        if certificate.predecessor_floor_hash.0 != approved_genesis.block_hash {
            return Err(invalid(
                "certificate predecessor anchor is not approved genesis",
            ));
        }
        let genesis_metadata = required_metadata(dag, &approved_genesis.block_hash)?;
        genesis_metadata
            .validate()
            .map_err(|error| invalid(error.to_string()))?;
        if !genesis_metadata.approved_genesis
            || !genesis_metadata.is_accepted()
            || genesis_metadata.post_state_hash != approved_genesis.body.state.post_state_hash
        {
            return Err(invalid(
                "certificate genesis anchor does not bind approved genesis state",
            ));
        }
        return Ok(());
    }

    let carrier_hash = &certificate.predecessor_certificate_block_hash.0;
    let carrier_metadata = required_metadata(dag, carrier_hash)?;
    carrier_metadata
        .validate()
        .map_err(|error| invalid(error.to_string()))?;
    if carrier_metadata.block_hash != *carrier_hash
        || carrier_metadata.approved_genesis
        || !carrier_metadata.is_accepted()
        || carrier_metadata.protocol_version != expected_protocol_version
    {
        return Err(invalid(
            "predecessor certificate carrier is not an accepted block in this protocol",
        ));
    }
    let carrier_commitment = carrier_metadata
        .finalized_floor_commitment
        .as_ref()
        .ok_or_else(|| invalid("accepted predecessor carrier has no floor commitment"))?;
    carrier_commitment.validate_shape().map_err(invalid)?;
    if carrier_commitment.floor_hash != certificate.predecessor_floor_hash.0
        || carrier_commitment.certificate_digest != certificate.predecessor_certificate_digest.0
    {
        return Err(invalid(
            "accepted predecessor carrier does not bind the required floor certificate",
        ));
    }
    let predecessor_metadata = required_metadata(dag, &certificate.predecessor_floor_hash.0)?;
    predecessor_metadata
        .validate()
        .map_err(|error| invalid(error.to_string()))?;
    if !predecessor_metadata.is_accepted()
        || predecessor_metadata.post_state_hash != carrier_commitment.floor_post_state_hash
    {
        return Err(invalid(
            "accepted predecessor carrier does not bind admitted floor state",
        ));
    }
    validate_candidate_parent_frontier(
        &carrier_metadata.parents,
        carrier_commitment,
        dag,
        block_store,
        approved_genesis,
    )
}

fn candidate_use_before_chain_cache<E>(
    chain_is_verified: bool,
    validate_candidate: impl FnOnce() -> Result<(), E>,
) -> Result<bool, E> {
    validate_candidate()?;
    Ok(chain_is_verified)
}

fn exact_latest(certificate: &FinalizationCertificate) -> BTreeMap<Validator, BlockHash> {
    certificate
        .exact_latest_messages
        .iter()
        .map(|(validator, block_hash)| (validator.0.clone(), block_hash.0.clone()))
        .collect()
}

struct VerificationWork {
    remaining: usize,
}

impl VerificationWork {
    fn new() -> Self {
        Self {
            remaining: FinalizationCertificate::MAX_DAG_VISITS_PER_VERIFICATION,
        }
    }
}

async fn validate_decision(
    certificate: &FinalizationCertificate,
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    ftt: FtThreshold,
    _finalizer_conf: &FinalizerConf,
) -> Result<(), FinalizationCertificateError> {
    let predecessor = &certificate.predecessor_floor_hash.0;
    let target = &certificate.target_floor_hash.0;
    let predecessor_metadata = required_metadata(dag, predecessor)?;
    let target_metadata = required_metadata(dag, target)?;
    if target_metadata.block_number != certificate.target_block_number
        || target_metadata.post_state_hash != certificate.target_post_state_hash.0
        || target_metadata.protocol_version != certificate.protocol_version
        || predecessor_metadata.protocol_version != certificate.protocol_version
    {
        return Err(invalid("target or predecessor metadata binding differs"));
    }
    if target == predecessor {
        if target != &certificate.genesis_hash.0
            || target_metadata.block_number != 0
            || certificate
                .exact_latest_messages
                .values()
                .any(|hash| hash.0 != *target)
        {
            return Err(invalid("self-target certificate is not the genesis anchor"));
        }
    } else if target_metadata.block_number <= predecessor_metadata.block_number
        || !dag
            .is_dag_ancestor(predecessor, target)
            .map_err(CasperError::from)?
        || !state_is_preserved(dag, block_store, predecessor, target)?
    {
        return Err(invalid(
            "target does not preserve and extend its certified predecessor",
        ));
    }

    let latest = exact_latest(certificate);
    let context = crate::rust::causal_equivocation::CertifiedConsensusContext::for_frozen_floor(
        dag,
        predecessor.clone(),
        &latest,
    )?;
    if !context.has_complete_latest_message_slots()
        || context.digest() != &certificate.authority_context_digest.0
    {
        return Err(invalid(
            "authority context is incomplete or has a different digest",
        ));
    }
    if target == predecessor {
        return Ok(());
    }
    let current = crate::rust::finality::floor::Floor {
        hash: predecessor.clone(),
        block_number: predecessor_metadata.block_number,
    };
    let selected = crate::rust::finality::floor::floor_of_frozen_vote_projection(
        dag,
        block_store,
        &current,
        context.vote_projection().eligible_latest_messages(),
        ftt,
    )
    .await
    .map_err(FinalizationCertificateError::Local)?;
    if !matches!(
        selected,
        crate::rust::finality::floor::FloorOfView::Advance(ref floor)
            if floor.hash == *target
    ) {
        return Err(invalid(
            "exact finalizer did not select the committed target",
        ));
    }
    Ok(())
}

fn expected_support(
    certificate: &FinalizationCertificate,
    dag: &KeyValueDagRepresentation,
    work: &mut VerificationWork,
) -> Result<BTreeSet<BlockHashSerde>, FinalizationCertificateError> {
    let declared_count = usize::try_from(certificate.supporting_block_count)
        .map_err(|_| invalid("supporting block count does not fit this platform"))?;
    let roots = certificate
        .exact_latest_messages
        .values()
        .map(|hash| hash.0.clone())
        .chain(std::iter::once(certificate.target_floor_hash.0.clone()))
        .collect::<Vec<_>>();
    let support = dag
        .certified_support_closure(
            &certificate.predecessor_floor_hash.0,
            roots,
            declared_count,
            &mut work.remaining,
        )
        .map_err(|error| invalid(error.to_string()))?;
    if support.len() != declared_count {
        return Err(invalid(
            "support closure differs from its certificate-declared block count",
        ));
    }
    Ok(support)
}

fn expected_newly_finalized(
    certificate: &FinalizationCertificate,
    dag: &KeyValueDagRepresentation,
    work: &mut VerificationWork,
) -> Result<BTreeSet<BlockHashSerde>, FinalizationCertificateError> {
    if certificate.target_floor_hash == certificate.predecessor_floor_hash {
        if certificate.finalized_block_count != 1 {
            return Err(invalid(
                "genesis certificate finalized count must be exactly one",
            ));
        }
        return Ok(BTreeSet::from([certificate.target_floor_hash.clone()]));
    }
    let declared_count = usize::try_from(certificate.finalized_block_count)
        .map_err(|_| invalid("finalized block count does not fit this platform"))?;
    let finalized = dag
        .certified_finalized_delta(
            &certificate.predecessor_floor_hash.0,
            &certificate.target_floor_hash.0,
            declared_count,
            &mut work.remaining,
        )
        .map_err(|error| invalid(error.to_string()))?;
    if finalized.len() != declared_count {
        return Err(invalid(
            "finalized ancestry delta differs from its certificate-declared block count",
        ));
    }
    Ok(finalized)
}

pub async fn verify(
    block: &BlockMessage,
    commitment: &FinalizedFloorCommitment,
    certificate: &FinalizationCertificate,
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    approved_genesis: &BlockMessage,
    expected_protocol_version: i64,
    expected_shard_id: &str,
    ftt: FtThreshold,
    finalizer_conf: &FinalizerConf,
    verification_schedule: &CertificateVerificationSchedule,
) -> Result<(), FinalizationCertificateError> {
    certificate
        .validate_commitment(commitment)
        .map_err(invalid)?;
    if block.header.version != expected_protocol_version
        || certificate.protocol_version != expected_protocol_version
        || block.shard_id != expected_shard_id
        || certificate.shard_id != expected_shard_id
        || certificate.genesis_hash.0 != approved_genesis.block_hash
        || certificate.fault_tolerance_numerator != ftt.num
        || certificate.fault_tolerance_denominator != ftt.den
    {
        return Err(invalid(
            "protocol, shard, genesis, or threshold binding differs",
        ));
    }
    let certificate_digest = certificate.digest();
    if candidate_use_before_chain_cache(
        block_store.is_finalization_certificate_verified(&certificate_digest),
        || validate_candidate_use(block, commitment, dag, block_store, approved_genesis),
    )? {
        return Ok(());
    }

    let _permit = verification_schedule
        .acquire(&certificate_digest)
        .await
        .map_err(|error| FinalizationCertificateError::Local(CasperError::RuntimeError(error)))?;
    if block_store.is_finalization_certificate_verified(&certificate_digest) {
        return Ok(());
    }

    certificate.validate_shape().map_err(invalid)?;
    validate_accepted_predecessor_anchor(
        certificate,
        dag,
        block_store,
        approved_genesis,
        expected_protocol_version,
    )?;
    validate_decision(certificate, dag, block_store, ftt, finalizer_conf).await?;
    let mut work = VerificationWork::new();
    let expected_finalized = expected_newly_finalized(certificate, dag, &mut work)?;
    if certificate.finalized_manifest_digest
        != FinalizationCertificate::finalized_digest(&expected_finalized)
    {
        return Err(invalid(
            "finalized manifest differs from the exact DAG ancestry delta",
        ));
    }
    let expected_support = expected_support(certificate, dag, &mut work)?;
    if !certificate
        .predecessor_certificate_digest
        .0
        .iter()
        .all(|byte| *byte == 0)
        && !expected_support.contains(&certificate.predecessor_certificate_block_hash)
    {
        return Err(invalid(
            "predecessor certificate carrier is outside the exact proof closure",
        ));
    }
    if certificate.supporting_manifest_digest
        != FinalizationCertificate::supporting_digest(&expected_support)
    {
        return Err(invalid(
            "support manifest differs from the exact proof closure",
        ));
    }
    block_store
        .mark_finalization_certificate_verified(certificate_digest)
        .map_err(CasperError::from)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Arc;
    use std::time::Duration;

    use block_storage::rust::dag::block_metadata_store::BlockMetadataStore;
    use block_storage::rust::dag::deploy_occurrence_store::DeployOccurrenceStore;
    use block_storage::rust::key_value_block_store::KeyValueBlockStore;
    use crypto::rust::hash::blake2b256::Blake2b256;
    use models::rust::block_hash::{BlockHash, BlockHashSerde};
    use models::rust::block_metadata::{AdmissionRejectionReason, BlockMetadata};
    use models::rust::bond_generation::BondGeneration;
    use models::rust::casper::protocol::casper_message::{
        Body, F1r3flyState, FinalizationCertificate, FinalizedFloorCommitment, Header,
    };
    use models::rust::validator::ValidatorSerde;
    use parking_lot::RwLock;
    use proptest::prelude::*;
    use prost::bytes::Bytes;
    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
    use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

    use super::{
        candidate_use_before_chain_cache, parent_floor_frontier_is_valid,
        select_predecessor_certificate_carrier, validate_accepted_predecessor_anchor,
        validate_candidate_parent_frontier, CertificateVerificationSchedule,
        FinalizationCertificateError, PredecessorCertificateCarrier,
    };

    fn rank(value: u8) -> BlockHash { Bytes::from(vec![value; 32]) }

    fn ordered(left: &BlockHash, right: &BlockHash) -> Result<bool, ()> { Ok(left[0] <= right[0]) }

    fn validator(value: u8) -> Bytes { Bytes::from(vec![value; models::rust::validator::LENGTH]) }

    fn approved_genesis() -> models::rust::casper::protocol::casper_message::BlockMessage {
        models::rust::casper::protocol::casper_message::BlockMessage {
            block_hash: rank(0),
            header: Header {
                parents_hash_list: Vec::new(),
                timestamp: 0,
                version: crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
                extra_bytes: Bytes::new(),
                sender_bond_generation: None,
                objective_equivocation_evidence_delta: Vec::new(),
                finalized_floor: None,
            },
            body: Body {
                state: F1r3flyState {
                    pre_state_hash: rank(0),
                    post_state_hash: rank(0),
                    bonds: Vec::new(),
                    bond_generations: Vec::new(),
                    active_validators: Vec::new(),
                    block_number: 0,
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
            sender: Bytes::new(),
            seq_num: 0,
            sig: Bytes::new(),
            sig_algorithm: String::new(),
            shard_id: "root".to_string(),
            extra_bytes: Bytes::new(),
            finalized_floor_certificate: None,
        }
    }

    fn carrier_metadata(
        genesis: &models::rust::casper::protocol::casper_message::BlockMessage,
        certificate_digest: BlockHash,
        accepted: bool,
    ) -> BlockMetadata {
        carrier_metadata_at(genesis, rank(1), certificate_digest, accepted)
    }

    fn carrier_metadata_at(
        genesis: &models::rust::casper::protocol::casper_message::BlockMessage,
        block_hash: BlockHash,
        certificate_digest: BlockHash,
        accepted: bool,
    ) -> BlockMetadata {
        let sender = validator(1);
        let mut context_preimage = b"f1r3fly-certified-test-metadata-context-v1".to_vec();
        context_preimage.extend_from_slice(&genesis.block_hash);
        context_preimage.extend_from_slice(&genesis.body.state.post_state_hash);
        let commitment = FinalizedFloorCommitment {
            floor_hash: genesis.block_hash.clone(),
            floor_post_state_hash: genesis.body.state.post_state_hash.clone(),
            certificate_digest,
            authority_context_digest: Blake2b256::hash(context_preimage).into(),
        };
        let metadata = BlockMetadata {
            block_hash,
            post_state_hash: rank(2),
            parents: vec![genesis.block_hash.clone()],
            sender: sender.clone(),
            justifications: Vec::new(),
            weight_map: BTreeMap::from([(sender.clone(), 1)]),
            bond_generation_map: BTreeMap::from([(sender.clone(), BondGeneration::GENESIS)]),
            active_validator_set: BTreeSet::from([sender]),
            block_number: 1,
            sequence_number: 1,
            admission_outcome: None,
            directly_finalized: false,
            finalized: false,
            fault_tolerance_value: 0.0,
            successful_state_effect_indices: BTreeSet::new(),
            rejected_state_effects: BTreeSet::new(),
            protocol_version: crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
            objective_equivocation_evidence_delta: Vec::new(),
            sender_authority: None,
            finalized_floor_commitment: Some(commitment),
            admission_schema_version: models::rust::block_metadata::ADMISSION_SCHEMA_VERSION,
            approved_genesis: false,
            merge_base: Bytes::new(),
        };
        if accepted {
            crate::rust::test_metadata::certify(metadata, BondGeneration::GENESIS)
        } else {
            crate::rust::test_metadata::certify_rejected(
                metadata,
                BondGeneration::GENESIS,
                AdmissionRejectionReason::InvalidFollows,
            )
        }
    }

    fn anchor_certificate(
        genesis: &models::rust::casper::protocol::casper_message::BlockMessage,
        digest: BlockHash,
    ) -> FinalizationCertificate {
        FinalizationCertificate {
            schema_version: FinalizationCertificate::SCHEMA_VERSION,
            protocol_version: crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
            shard_id: "root".to_string(),
            genesis_hash: genesis.block_hash.clone().into(),
            predecessor_floor_hash: genesis.block_hash.clone().into(),
            predecessor_certificate_digest: digest.into(),
            predecessor_certificate_block_hash: rank(1).into(),
            target_floor_hash: rank(2).into(),
            target_post_state_hash: rank(3).into(),
            target_block_number: 2,
            fault_tolerance_numerator: 100_000,
            fault_tolerance_denominator: 1_000_000,
            exact_latest_messages: BTreeMap::from([(ValidatorSerde(validator(1)), rank(1).into())]),
            authority_context_digest: rank(4).into(),
            supporting_manifest_digest: rank(5).into(),
            finalized_manifest_digest: rank(6).into(),
            supporting_block_count: 1,
            finalized_block_count: 1,
        }
    }

    fn anchor_dag(
        genesis: &models::rust::casper::protocol::casper_message::BlockMessage,
        carrier: BlockMetadata,
    ) -> block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation {
        anchor_dag_with_carriers(genesis, vec![carrier])
    }

    fn anchor_dag_with_carriers(
        genesis: &models::rust::casper::protocol::casper_message::BlockMessage,
        carriers: Vec<BlockMetadata>,
    ) -> block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation {
        let metadata_store = KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new()));
        let mut metadata_index = BlockMetadataStore::new(metadata_store).unwrap();
        let genesis_metadata = BlockMetadata::from_approved_genesis(genesis).unwrap();
        metadata_index.add(genesis_metadata).unwrap();
        let carrier_hashes = carriers
            .iter()
            .map(|carrier| carrier.block_hash.clone())
            .collect::<Vec<_>>();
        for carrier in carriers {
            metadata_index.add(carrier).unwrap();
        }
        block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation {
            dag_set: imbl::HashSet::from_iter(
                std::iter::once(genesis.block_hash.clone()).chain(carrier_hashes.iter().cloned()),
            ),
            latest_messages_map: imbl::HashMap::new(),
            child_map: imbl::HashMap::new(),
            height_map: imbl::OrdMap::new(),
            block_number_map: imbl::HashMap::from_iter(
                std::iter::once((genesis.block_hash.clone(), 0))
                    .chain(carrier_hashes.iter().cloned().map(|hash| (hash, 1))),
            ),
            main_parent_map: imbl::HashMap::from_iter(
                carrier_hashes
                    .iter()
                    .cloned()
                    .map(|hash| (hash, genesis.block_hash.clone())),
            ),
            self_justification_map: imbl::HashMap::new(),
            invalid_blocks_set: imbl::HashSet::new(),
            equivocation_observations: imbl::HashMap::new(),
            last_finalized_block_hash: genesis.block_hash.clone(),
            finalized_blocks_set: imbl::HashSet::from_iter([genesis.block_hash.clone()]),
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

    fn block_store() -> KeyValueBlockStore {
        KeyValueBlockStore::new(
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
        )
    }

    #[test]
    fn historical_floor_cannot_be_reused_over_a_newer_parent_floor() {
        assert!(!parent_floor_frontier_is_valid(&rank(0), &[rank(1)], ordered, ordered,).unwrap());
    }

    #[test]
    fn candidate_parent_frontier_must_carry_committed_floor_ancestry() {
        let genesis = approved_genesis();
        let floor = carrier_metadata_at(&genesis, rank(1), rank(9), true);
        let incompatible = carrier_metadata_at(&genesis, rank(2), rank(8), true);
        let commitment = FinalizedFloorCommitment {
            floor_hash: floor.block_hash.clone(),
            floor_post_state_hash: floor.post_state_hash.clone(),
            certificate_digest: rank(7),
            authority_context_digest: rank(6),
        };
        let dag = anchor_dag_with_carriers(&genesis, vec![floor, incompatible]);
        let error = validate_candidate_parent_frontier(
            &[rank(2)],
            &commitment,
            &dag,
            &block_store(),
            &genesis,
        )
        .unwrap_err();
        assert!(matches!(
            error,
            FinalizationCertificateError::Invalid(detail)
                if detail == "candidate finalized floor is absent from the causal parent frontier"
        ));
    }

    #[test]
    fn incomparable_parent_floors_fail_closed_even_if_both_reach_candidate() {
        let is_ancestor = |left: &BlockHash, right: &BlockHash| {
            Ok::<bool, ()>(left == right || right[0] == 3 && matches!(left[0], 1 | 2))
        };
        assert!(!parent_floor_frontier_is_valid(
            &rank(3),
            &[rank(1), rank(2)],
            is_ancestor,
            |left, right| Ok::<bool, ()>(left == right || right[0] == 3),
        )
        .unwrap());
    }

    #[test]
    fn verified_chain_cache_never_bypasses_candidate_specific_admission() {
        let incompatible = candidate_use_before_chain_cache(true, || Err::<(), _>("incompatible"));
        assert_eq!(incompatible, Err("incompatible"));
        assert_eq!(
            candidate_use_before_chain_cache(true, || Ok::<(), &str>(())),
            Ok(true)
        );
        assert_eq!(
            candidate_use_before_chain_cache(false, || Ok::<(), &str>(())),
            Ok(false)
        );
    }

    #[test]
    #[should_panic(expected = "certificate verification worker limit must be at least one")]
    fn certificate_verification_requires_a_positive_worker_limit() {
        CertificateVerificationSchedule::new(0);
    }

    #[tokio::test]
    async fn equal_certificate_digests_share_one_verification_lane() {
        let schedule = Arc::new(CertificateVerificationSchedule::new(2));
        let first = schedule.acquire(&rank(1)).await.unwrap();
        let blocked =
            tokio::time::timeout(Duration::from_millis(20), schedule.acquire(&rank(1))).await;
        assert!(blocked.is_err());
        drop(first);
        assert!(schedule.acquire(&rank(1)).await.is_ok());
    }

    #[tokio::test]
    async fn distinct_certificate_digests_verify_in_parallel_up_to_the_worker_limit() {
        let schedule = CertificateVerificationSchedule::new(2);
        let first = schedule.acquire(&rank(1)).await.unwrap();
        let second = schedule.acquire(&rank(2)).await.unwrap();
        let blocked =
            tokio::time::timeout(Duration::from_millis(20), schedule.acquire(&rank(3))).await;
        assert!(blocked.is_err());
        drop(first);
        assert!(schedule.acquire(&rank(3)).await.is_ok());
        drop(second);
    }

    #[tokio::test]
    async fn cancelled_waiter_releases_certificate_verification_state() {
        let schedule = Arc::new(CertificateVerificationSchedule::new(1));
        let first = schedule.acquire(&rank(1)).await.unwrap();
        let waiting_schedule = schedule.clone();
        let waiter = tokio::spawn(async move { waiting_schedule.acquire(&rank(1)).await });
        tokio::task::yield_now().await;
        waiter.abort();
        assert!(waiter.await.unwrap_err().is_cancelled());
        drop(first);
        assert!(schedule.acquire(&rank(1)).await.is_ok());
    }

    #[test]
    fn certificate_verification_reports_its_parallel_capacity() {
        assert_eq!(CertificateVerificationSchedule::new(3).worker_limit(), 3);
    }

    #[test]
    fn accepted_carrier_is_a_restart_stable_predecessor_induction_anchor() {
        let genesis = approved_genesis();
        let digest = rank(9);
        let certificate = anchor_certificate(&genesis, digest.clone());
        let dag = anchor_dag(&genesis, carrier_metadata(&genesis, digest, true));
        validate_accepted_predecessor_anchor(
            &certificate,
            &dag,
            &block_store(),
            &genesis,
            crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
        )
        .unwrap();
    }

    #[test]
    fn carrier_selection_requires_causal_support_and_excludes_the_target() {
        let genesis = approved_genesis();
        let digest = rank(9);
        let dag = anchor_dag(&genesis, carrier_metadata(&genesis, digest.clone(), true));
        let supported = select_predecessor_certificate_carrier(
            &BTreeSet::from([BlockHashSerde(rank(1))]),
            &rank(2),
            &genesis.block_hash,
            &genesis.body.state.post_state_hash,
            crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
            &dag,
            &block_store(),
            &genesis,
        )
        .unwrap();
        assert_eq!(
            supported,
            Some(PredecessorCertificateCarrier {
                block_hash: rank(1),
                certificate_digest: digest.clone(),
            })
        );

        let ambient = select_predecessor_certificate_carrier(
            &BTreeSet::from([BlockHashSerde(genesis.block_hash.clone())]),
            &rank(2),
            &genesis.block_hash,
            &genesis.body.state.post_state_hash,
            crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
            &dag,
            &block_store(),
            &genesis,
        )
        .unwrap();
        assert_eq!(ambient, None);

        let target = select_predecessor_certificate_carrier(
            &BTreeSet::from([BlockHashSerde(rank(1))]),
            &rank(1),
            &genesis.block_hash,
            &genesis.body.state.post_state_hash,
            crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
            &dag,
            &block_store(),
            &genesis,
        )
        .unwrap();
        assert_eq!(target, None);
    }

    #[test]
    fn semantically_equivalent_witness_carriers_preserve_the_selected_proof_pair() {
        let genesis = approved_genesis();
        let first_digest = rank(9);
        let second_digest = rank(8);
        let dag = anchor_dag_with_carriers(&genesis, vec![
            carrier_metadata_at(&genesis, rank(1), first_digest.clone(), true),
            carrier_metadata_at(&genesis, rank(2), second_digest.clone(), true),
        ]);
        let selected = select_predecessor_certificate_carrier(
            &BTreeSet::from([BlockHashSerde(rank(2)), BlockHashSerde(rank(1))]),
            &rank(3),
            &genesis.block_hash,
            &genesis.body.state.post_state_hash,
            crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
            &dag,
            &block_store(),
            &genesis,
        )
        .unwrap();
        assert_eq!(
            selected,
            Some(PredecessorCertificateCarrier {
                block_hash: rank(1),
                certificate_digest: first_digest.clone(),
            })
        );

        let first = anchor_certificate(&genesis, first_digest.clone());
        validate_accepted_predecessor_anchor(
            &first,
            &dag,
            &block_store(),
            &genesis,
            crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
        )
        .unwrap();
        let mut second = anchor_certificate(&genesis, second_digest);
        second.predecessor_certificate_block_hash = rank(2).into();
        validate_accepted_predecessor_anchor(
            &second,
            &dag,
            &block_store(),
            &genesis,
            crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
        )
        .unwrap();

        let mut spliced = second;
        spliced.predecessor_certificate_digest = first_digest.into();
        assert!(matches!(
            validate_accepted_predecessor_anchor(
                &spliced,
                &dag,
                &block_store(),
                &genesis,
                crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
            ),
            Err(FinalizationCertificateError::Invalid(_))
        ));
    }

    #[test]
    fn rejected_or_mismatched_carrier_never_anchors_a_predecessor_certificate() {
        let genesis = approved_genesis();
        let digest = rank(9);
        let certificate = anchor_certificate(&genesis, digest.clone());
        let rejected = anchor_dag(&genesis, carrier_metadata(&genesis, digest.clone(), false));
        assert!(matches!(
            validate_accepted_predecessor_anchor(
                &certificate,
                &rejected,
                &block_store(),
                &genesis,
                crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
            ),
            Err(FinalizationCertificateError::Invalid(_))
        ));

        let mismatched = anchor_dag(&genesis, carrier_metadata(&genesis, rank(8), true));
        assert!(matches!(
            validate_accepted_predecessor_anchor(
                &certificate,
                &mismatched,
                &block_store(),
                &genesis,
                crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
            ),
            Err(FinalizationCertificateError::Invalid(_))
        ));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn parent_permutations_have_identical_compatibility(
            values in proptest::collection::vec(0u8..32, 1..32),
            candidate in 0u8..32,
        ) {
            let parents = values.iter().copied().map(rank).collect::<Vec<_>>();
            let mut reversed = parents.clone();
            reversed.reverse();
            let committed = rank(candidate);
            let forward = parent_floor_frontier_is_valid(
                &committed,
                &parents,
                ordered,
                ordered,
            )
            .unwrap();
            let backward = parent_floor_frontier_is_valid(
                &committed,
                &reversed,
                ordered,
                ordered,
            )
            .unwrap();
            prop_assert_eq!(forward, backward);
        }

        #[test]
        fn comparable_parent_chain_accepts_exactly_when_candidate_is_maximal(
            values in proptest::collection::vec(0u8..32, 1..32),
            candidate in 0u8..32,
        ) {
            let parents = values.iter().copied().map(rank).collect::<Vec<_>>();
            let committed = rank(candidate);
            let valid = parent_floor_frontier_is_valid(
                &committed,
                &parents,
                ordered,
                ordered,
            )
            .unwrap();
            prop_assert_eq!(valid, values.iter().all(|parent| *parent <= candidate));
        }

        #[test]
        fn carrier_selection_is_permutation_invariant_and_preserves_digest_pairing(
            first_hash in 1u8..120,
            second_hash in 121u8..240,
            first_digest in 1u8..128,
            second_digest in 128u8..=255,
            reverse in any::<bool>(),
        ) {
            let genesis = approved_genesis();
            let first = carrier_metadata_at(
                &genesis,
                rank(first_hash),
                rank(first_digest),
                true,
            );
            let second = carrier_metadata_at(
                &genesis,
                rank(second_hash),
                rank(second_digest),
                true,
            );
            let carriers = if reverse {
                vec![second, first]
            } else {
                vec![first, second]
            };
            let dag = anchor_dag_with_carriers(&genesis, carriers);
            let support = if reverse {
                BTreeSet::from([
                    BlockHashSerde(rank(second_hash)),
                    BlockHashSerde(rank(first_hash)),
                ])
            } else {
                BTreeSet::from([
                    BlockHashSerde(rank(first_hash)),
                    BlockHashSerde(rank(second_hash)),
                ])
            };
            let selected = select_predecessor_certificate_carrier(
                &support,
                &rank(250),
                &genesis.block_hash,
                &genesis.body.state.post_state_hash,
                crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
                &dag,
                &block_store(),
                &genesis,
            )
            .unwrap();
            prop_assert_eq!(
                selected,
                Some(PredecessorCertificateCarrier {
                    block_hash: rank(first_hash),
                    certificate_digest: rank(first_digest),
                })
            );
        }
    }
}
