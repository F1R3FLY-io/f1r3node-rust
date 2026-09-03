use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::dag::block_metadata_store::BlockMetadataStore;
use block_storage::rust::dag::deploy_lifecycle_types::DeployLifecycleTables;
use block_storage::rust::dag::deploy_occurrence_store::DeployOccurrenceStore;
use models::rust::block_hash::BlockHash;
use models::rust::block_metadata::{
    BlockMetadata, CertifiedAdmissionOutcome, CertifiedSenderAuthority,
};
use models::rust::bond_generation::BondGeneration;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Body, F1r3flyState, FinalizedFloorCommitment, Header,
};
use models::rust::validator::Validator;
use parking_lot::RwLock;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use shared::rust::store::key_value_store::KvStoreError;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

fn hash(byte: u8) -> BlockHash { Bytes::from(vec![byte; models::rust::block_hash::LENGTH]) }

fn validator(byte: u8) -> Validator { Bytes::from(vec![byte; models::rust::validator::LENGTH]) }

fn metadata(block_hash: BlockHash, block_number: i64) -> BlockMetadata {
    let sender = validator(9);
    let post_state_hash = hash(250);
    let authority_floor_hash = hash(1);
    let authority_floor_post_state_hash = hash(249);
    let authority_context_digest = hash(248);
    let commitment = FinalizedFloorCommitment {
        floor_hash: authority_floor_hash.clone(),
        floor_post_state_hash: authority_floor_post_state_hash.clone(),
        certificate_digest: hash(247),
        authority_context_digest: authority_context_digest.clone(),
    };
    let block = BlockMessage {
        block_hash,
        header: Header {
            parents_hash_list: Vec::new(),
            timestamp: 0,
            version: casper::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
            extra_bytes: Bytes::new(),
            sender_bond_generation: Some(BondGeneration::GENESIS),
            objective_equivocation_evidence_delta: Vec::new(),
            finalized_floor: Some(commitment),
        },
        body: Body {
            state: F1r3flyState {
                pre_state_hash: authority_floor_post_state_hash.clone(),
                post_state_hash,
                bonds: Vec::new(),
                bond_generations: Vec::new(),
                active_validators: Vec::new(),
                block_number,
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
        sender,
        seq_num: block_number as i32,
        sig: Bytes::new(),
        sig_algorithm: String::new(),
        shard_id: "root".to_string(),
        extra_bytes: Bytes::new(),
        finalized_floor_certificate: None,
    };
    let sender_authority = CertifiedSenderAuthority::new(
        &block,
        authority_floor_hash,
        authority_floor_post_state_hash,
        authority_context_digest,
        BondGeneration::GENESIS,
        1,
    )
    .unwrap();
    let admission_outcome = CertifiedAdmissionOutcome::accepted(&block, &sender_authority).unwrap();
    BlockMetadata::from_certified_block(
        &block,
        Some(false),
        Some(false),
        &sender_authority,
        &admission_outcome,
    )
    .unwrap()
}

fn restored_dag(
    canonical_genesis: BlockHash,
    held: Vec<BlockMetadata>,
    latest_messages: imbl::HashMap<Validator, BlockHash>,
) -> KeyValueDagRepresentation {
    let metadata_store = KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new()));
    let mut metadata_index = BlockMetadataStore::new(metadata_store).unwrap();
    let mut dag_set = imbl::HashSet::new();
    let mut block_number_map = imbl::HashMap::new();
    for entry in held {
        dag_set.insert(entry.block_hash.clone());
        block_number_map.insert(entry.block_hash.clone(), entry.block_number);
        metadata_index.add(entry).unwrap();
    }
    let deploy_store = Arc::new(InMemoryKeyValueStore::new());
    KeyValueDagRepresentation {
        dag_set,
        canonical_genesis_hash: Some(canonical_genesis),
        latest_messages_map: latest_messages,
        child_map: imbl::HashMap::new(),
        height_map: imbl::OrdMap::new(),
        block_number_map,
        main_parent_map: imbl::HashMap::new(),
        self_justification_map: imbl::HashMap::new(),
        invalid_blocks_set: imbl::HashSet::new(),
        equivocation_observations: imbl::HashMap::new(),
        last_finalized_block_hash: hash(2),
        finalized_blocks_set: imbl::HashSet::new(),
        block_metadata_index: Arc::new(RwLock::new(metadata_index)),
        deploy_index: Arc::new(RwLock::new(KeyValueTypedStoreImpl::new(
            deploy_store.clone(),
        ))),
        deploy_occurrence_store: DeployOccurrenceStore::activate_fresh(deploy_store).unwrap(),
        floor_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
        frontier_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
        lifecycle: Arc::new(RwLock::new(DeployLifecycleTables::in_memory())),
        carrier_index: Arc::new(RwLock::new(
            block_storage::rust::dag::carrier_index::CarrierIndex::in_memory(),
        )),
    }
}

#[test]
fn canonical_genesis_body_may_be_absent_without_deleting_its_exact_slot() {
    let live = validator(1);
    let silent = validator(2);
    let genesis = hash(1);
    let live_hash = hash(3);
    let latest = imbl::HashMap::from(vec![
        (live.clone(), live_hash.clone()),
        (silent.clone(), genesis.clone()),
    ]);
    let dag = restored_dag(genesis.clone(), vec![metadata(live_hash, 2)], latest);

    assert_eq!(dag.latest_message_hash(&silent), Some(genesis));
    assert_eq!(dag.latest_message_hashes().len(), 2);
    let materialized = dag.latest_messages().unwrap();
    assert_eq!(materialized.len(), 1);
    assert!(materialized.contains_key(&live));
}

#[test]
fn noncanonical_missing_latest_body_is_a_storage_error() {
    let signer = validator(3);
    let genesis = hash(1);
    let missing = hash(4);
    let dag = restored_dag(
        genesis,
        Vec::new(),
        imbl::HashMap::from(vec![(signer.clone(), missing.clone())]),
    );

    assert!(matches!(
        dag.latest_message(&signer),
        Err(KvStoreError::MissingBlock { hash, .. }) if hash == missing
    ));
    assert!(matches!(
        dag.latest_messages(),
        Err(KvStoreError::MissingBlock { hash, .. }) if hash == missing
    ));
}
