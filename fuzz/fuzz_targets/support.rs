//! Shared fixture builders for the slashing fuzz harnesses.
//!
//! Provides:
//!   * Deterministic synthetic identities (`validator`, `block_hash`) at the
//!     correct widths for production code to accept.
//!   * `ProcessedSystemDeploy` builders for the three relevant variants.
//!   * `BlockMessage` builders pre-wired with a synthetic header / body.
//!   * `empty_dag` + `snapshot` for building an in-memory `CasperSnapshot`
//!     against `InMemoryKeyValueStore` — no LMDB I/O, deterministic per
//!     iteration.
//!
//! `#[allow(dead_code)]` is at the module level because each `fuzz_target`
//! is a separate binary and uses a different subset of these helpers; the
//! unused ones in any given binary must not produce warnings.

#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::dag::block_metadata_store::BlockMetadataStore;
use block_storage::rust::dag::deploy_lifecycle_types::DeployLifecycleTables;
use block_storage::rust::dag::deploy_occurrence_store::DeployOccurrenceStore;
use casper::rust::casper::{CasperShardConf, CasperSnapshot, OnChainCasperState};
use casper::rust::causal_equivocation::CertifiedConsensusContext;
use casper::rust::slashing_authorization::CanonicalSlashAuthority;
use crypto::rust::public_key::PublicKey;
use dashmap::DashSet;
use models::rust::block_hash::BlockHash;
use models::rust::block_metadata::{
    AdmissionRejectionReason, BlockMetadata, CERTIFIED_ADMISSION_PROTOCOL_VERSION,
    CertifiedAdmissionOutcome, CertifiedSenderAuthority,
};
use models::rust::bond_generation::BondGeneration;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Body, Bond, F1r3flyState, FinalizedFloorCommitment, Header,
    ProcessedSystemDeploy, SystemDeployData, ValidatorBondGeneration,
};
use models::rust::validator::Validator;
use parking_lot::RwLock;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

#[derive(Clone, Debug)]
pub struct Evidence {
    pub hash: BlockHash,
    pub sender: Validator,
    pub block_number: i64,
    pub sequence_number: i32,
    pub invalid: bool,
}

/// Build a `Bytes` value of `len` copies of `seed`. Distinct seeds produce
/// distinct values (the byte repetition is the identity function on seed
/// space), so this is a deterministic, collision-resistant key generator
/// for synthetic DAGs without dragging in a hashing pass.
pub fn repeated(seed: u8, len: usize) -> Bytes { Bytes::from(vec![seed; len]) }

/// Synthetic validator identity. Width = 65 because that is the
/// uncompressed Secp256k1 public-key length (1-byte prefix + 32-byte X +
/// 32-byte Y). Production validation rejects other widths, so generating
/// validators at 65 bytes is mandatory for the snapshot to be accepted.
pub fn validator(seed: u8) -> Validator { repeated(seed, 65) }

/// Synthetic block hash. Width = 32 because production block hashes are
/// Blake2b-256 digests. Other widths fail equality comparison against the
/// hashes the DAG layer computes for real blocks.
pub fn block_hash(seed: u8) -> BlockHash { repeated(seed, 32) }

/// Builder for a successful Slash system deploy. Together with
/// [`close_deploy`] and [`failed_deploy`], this is a tagged-union
/// constructor kit for `ProcessedSystemDeploy` — the three are
/// mutually exclusive and a block body's `system_deploys` vector
/// typically holds a small mixed set of these.
pub fn slash_deploy(
    invalid_block_hash: BlockHash,
    issuer: Validator,
    target_activation_epoch: i64,
) -> ProcessedSystemDeploy {
    ProcessedSystemDeploy::Succeeded {
        event_list: vec![],
        system_deploy: SystemDeployData::Slash {
            invalid_block_hash,
            equivocation_block_hash: None,
            issuer_public_key: PublicKey::from_bytes(&issuer),
            target_activation_epoch,
            target_bond_generation: BondGeneration::GENESIS,
        },
        pre_state_hash: Vec::<u8>::new().into(),
        post_state_hash: Vec::<u8>::new().into(),
    }
}

pub fn equivocation_slash_deploy(
    first_block_hash: BlockHash,
    second_block_hash: BlockHash,
    issuer: Validator,
    target_activation_epoch: i64,
) -> ProcessedSystemDeploy {
    ProcessedSystemDeploy::Succeeded {
        event_list: vec![],
        system_deploy: SystemDeployData::create_equivocation_slash(
            first_block_hash,
            second_block_hash,
            PublicKey::from_bytes(&issuer),
            target_activation_epoch,
            BondGeneration::GENESIS,
        ),
        pre_state_hash: Vec::<u8>::new().into(),
        post_state_hash: Vec::<u8>::new().into(),
    }
}

/// Builder for a CloseBlock system deploy (the per-block terminator).
pub fn close_deploy() -> ProcessedSystemDeploy {
    ProcessedSystemDeploy::Succeeded {
        event_list: vec![],
        system_deploy: SystemDeployData::CloseBlockSystemDeployData,
        pre_state_hash: Vec::<u8>::new().into(),
        post_state_hash: Vec::<u8>::new().into(),
    }
}

/// Builder for the Failed variant. Sibling to `slash_deploy` and
/// `close_deploy` — these three together exhaust the variant space the
/// production validator inspects.
pub fn failed_deploy() -> ProcessedSystemDeploy {
    ProcessedSystemDeploy::Failed {
        event_list: vec![],
        error_msg: "fuzz".to_string(),
        pre_state_hash: Vec::<u8>::new().into(),
        post_state_hash: Vec::<u8>::new().into(),
    }
}

/// Build a `BlockMessage` whose header timestamp, state.block_number,
/// and seq_num all equal `block_number`. The triple-coupling is
/// deliberate: it lets the harnesses parametrize a synthetic block by a
/// single integer and have all three slots stay consistent (the
/// production block-number / timestamp / seq drift relations are
/// covered by integration tests, not by these fuzzers). Pre-state and
/// post-state hashes are derived from `hash_seed` so they are distinct
/// from the block hash itself.
pub fn block_with_system_deploys(
    hash_seed: u8,
    sender: Validator,
    block_number: i64,
    system_deploys: Vec<ProcessedSystemDeploy>,
) -> BlockMessage {
    BlockMessage {
        block_hash: block_hash(hash_seed),
        header: Header {
            parents_hash_list: vec![],
            timestamp: block_number,
            version: CERTIFIED_ADMISSION_PROTOCOL_VERSION,
            extra_bytes: Bytes::new(),
            sender_bond_generation: Some(BondGeneration::GENESIS),
            objective_equivocation_evidence_delta: Vec::new(),
            finalized_floor: Some(FinalizedFloorCommitment {
                floor_hash: block_hash(251),
                floor_post_state_hash: block_hash(250),
                certificate_digest: block_hash(248),
                authority_context_digest: block_hash(249),
            }),
        },
        body: Body {
            state: F1r3flyState {
                pre_state_hash: repeated(hash_seed.wrapping_add(1), 32),
                post_state_hash: repeated(hash_seed.wrapping_add(2), 32),
                bonds: vec![],
                bond_generations: vec![],
                active_validators: vec![],
                block_number,
            },
            deploys: vec![],
            rejected_deploys: vec![],
            rejected_state_effects: vec![],
            system_deploys,
            extra_bytes: Bytes::new(),
            applied_from_scope: Vec::new(),
            merge_base: Bytes::new(),
        },
        justifications: vec![],
        sender,
        seq_num: i32::try_from(block_number).unwrap_or_default(),
        sig: Bytes::new(),
        sig_algorithm: String::new(),
        shard_id: "root".to_string(),
        extra_bytes: Bytes::new(),
        finalized_floor_certificate: None,
    }
}

/// Build a fully-zeroed `KeyValueDagRepresentation` against
/// `InMemoryKeyValueStore`. The `InMemory` choice is load-bearing here —
/// fuzz iterations must not hit disk, must not share state across
/// iterations, and must complete in microseconds. No LMDB, no global
/// lock, no per-iteration cleanup.
fn empty_dag() -> KeyValueDagRepresentation {
    let metadata_store = KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new()));
    let deploy_store = KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new()));
    let occurrence_store = Arc::new(InMemoryKeyValueStore::new());
    let floor_store = KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new()));
    let frontier_store = KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new()));
    KeyValueDagRepresentation {
        dag_set: imbl::HashSet::new(),
        canonical_genesis_hash: None,
        latest_messages_map: imbl::HashMap::new(),
        child_map: imbl::HashMap::new(),
        height_map: imbl::OrdMap::new(),
        block_number_map: imbl::HashMap::new(),
        main_parent_map: imbl::HashMap::new(),
        self_justification_map: imbl::HashMap::new(),
        invalid_blocks_set: imbl::HashSet::new(),
        equivocation_observations: imbl::HashMap::new(),
        last_finalized_block_hash: Bytes::new(),
        finalized_blocks_set: imbl::HashSet::new(),
        block_metadata_index: Arc::new(RwLock::new(
            BlockMetadataStore::new(metadata_store).unwrap(),
        )),
        deploy_index: Arc::new(RwLock::new(deploy_store)),
        deploy_occurrence_store: DeployOccurrenceStore::activate_fresh(occurrence_store).unwrap(),
        floor_index: floor_store,
        frontier_index: frontier_store,
        lifecycle: Arc::new(RwLock::new(DeployLifecycleTables::in_memory())),
    }
}

fn metadata(evidence: &Evidence) -> BlockMetadata {
    let block = BlockMessage {
        block_hash: evidence.hash.clone(),
        header: Header {
            parents_hash_list: Vec::new(),
            timestamp: evidence.block_number,
            version: CERTIFIED_ADMISSION_PROTOCOL_VERSION,
            extra_bytes: Bytes::new(),
            sender_bond_generation: Some(BondGeneration::GENESIS),
            objective_equivocation_evidence_delta: Vec::new(),
            finalized_floor: Some(FinalizedFloorCommitment {
                floor_hash: block_hash(251),
                floor_post_state_hash: block_hash(250),
                certificate_digest: block_hash(248),
                authority_context_digest: block_hash(249),
            }),
        },
        body: Body {
            state: F1r3flyState {
                pre_state_hash: block_hash(252),
                post_state_hash: block_hash(evidence.hash[0].wrapping_add(1)),
                bonds: vec![Bond {
                    validator: evidence.sender.clone(),
                    stake: 1,
                }],
                bond_generations: vec![ValidatorBondGeneration {
                    validator: evidence.sender.clone(),
                    generation: BondGeneration::GENESIS,
                }],
                active_validators: vec![evidence.sender.clone()],
                block_number: evidence.block_number,
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
        sender: evidence.sender.clone(),
        seq_num: evidence.sequence_number,
        sig: Bytes::new(),
        sig_algorithm: "fuzz".to_string(),
        shard_id: "root".to_string(),
        extra_bytes: Bytes::new(),
        finalized_floor_certificate: None,
    };
    let authority = CertifiedSenderAuthority::new(
        &block,
        block_hash(251),
        block_hash(250),
        block_hash(249),
        BondGeneration::GENESIS,
        1,
    )
    .expect("sender authority");
    let outcome = if evidence.invalid {
        CertifiedAdmissionOutcome::rejected(
            &block,
            &authority,
            AdmissionRejectionReason::AdmissibleEquivocation,
        )
    } else {
        CertifiedAdmissionOutcome::accepted(&block, &authority)
    }
    .expect("admission outcome");
    BlockMetadata::from_certified_block(&block, None, None, &authority, &outcome)
        .expect("certified metadata")
}

/// Build a `CasperSnapshot` whose DAG and on-chain state are populated
/// from `evidences` + `bonds`. The four DAG collections —
/// `dag_set`, `height_map`, `block_metadata_index`, and (conditionally)
/// `invalid_blocks_set` — are populated in lockstep for each evidence;
/// production code assumes them consistent and panics or returns
/// `KeyNotFound` if they aren't. Any future change to those collections
/// must update this builder in the same atomic step.
///
/// Duplicate evidence hashes are skipped (first occurrence wins): a real
/// DAG has exactly one metadata per block hash, and arbitrary fuzz inputs
/// that reuse a hash seed would otherwise split the collections — the
/// first metadata lands in `invalid_blocks_set` while a later one
/// overwrites `block_metadata_index`, an unreachable state that makes
/// candidate derivation and validation diverge by construction (found by
/// the slash_lifecycle_trace fuzzer, crash b60fee62).
pub fn snapshot(
    evidences: &[Evidence],
    max_block_num: i64,
    epoch_length: i32,
    bonds: Vec<(Validator, i64)>,
) -> CasperSnapshot {
    let mut dag = empty_dag();
    let mut seen_hashes: HashSet<BlockHash> = HashSet::new();
    for evidence in evidences {
        if !seen_hashes.insert(evidence.hash.clone()) {
            continue;
        }
        let metadata = metadata(evidence);
        dag.dag_set.insert(metadata.block_hash.clone());
        dag.block_number_map
            .insert(metadata.block_hash.clone(), metadata.block_number);
        dag.height_map
            .entry(metadata.block_number)
            .or_insert_with(imbl::HashSet::new)
            .insert(metadata.block_hash.clone());
        let key = (
            metadata.sender.clone(),
            BondGeneration::GENESIS,
            metadata.sequence_number,
        );
        let mut observations = dag
            .equivocation_observations
            .get(&key)
            .cloned()
            .unwrap_or_default();
        observations.insert(metadata.block_hash.clone());
        dag.equivocation_observations.insert(key, observations);
        if metadata.is_rejected() {
            dag.invalid_blocks_set.insert(metadata.clone());
        }
        dag.block_metadata_index
            .write()
            .add(metadata)
            .expect("metadata insert");
    }
    let bonds_map = bonds.iter().cloned().collect::<HashMap<_, _>>();
    let bond_generations = bonds_map
        .keys()
        .cloned()
        .map(|validator| (validator, BondGeneration::GENESIS))
        .collect();
    let finalized_floor_bonds = bonds
        .iter()
        .map(|(validator, stake)| Bond {
            validator: validator.clone(),
            stake: *stake,
        })
        .collect();
    let active_validators = bonds.into_iter().map(|(validator, _)| validator).collect();
    CasperSnapshot {
        dag,
        last_finalized_block: Bytes::new(),
        lca: Bytes::new(),
        tips: vec![],
        parents: vec![],
        justifications: Vec::new(),
        invalid_blocks: HashMap::new(),
        deploys_in_scope: Arc::new(DashSet::new()),
        rejected_in_scope: Arc::new(DashSet::new()),
        max_block_num,
        max_seq_nums: HashMap::new(),
        finalized_floor_bonds,
        on_chain_state: OnChainCasperState {
            shard_conf: CasperShardConf {
                epoch_length,
                ..CasperShardConf::new()
            },
            bonds_map,
            bond_generations,
            active_validators,
        },
        consensus_context: CertifiedConsensusContext::pre_genesis(),
        finalized_floor_certificate: None,
    }
}

pub fn slash_authority(snapshot: &CasperSnapshot) -> CanonicalSlashAuthority {
    CanonicalSlashAuthority::from_parts(
        block_hash(248),
        snapshot.on_chain_state.bonds_map.clone(),
        snapshot.on_chain_state.bond_generations.clone(),
    )
    .expect("slash authority")
}
