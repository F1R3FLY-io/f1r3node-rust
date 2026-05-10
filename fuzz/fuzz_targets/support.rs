#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, RwLock};

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::dag::block_metadata_store::BlockMetadataStore;
use casper::rust::casper::{CasperShardConf, CasperSnapshot, OnChainCasperState};
use crypto::rust::public_key::PublicKey;
use dashmap::{DashMap, DashSet};
use models::rust::block_hash::BlockHash;
use models::rust::block_metadata::BlockMetadata;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Body, F1r3flyState, Header, ProcessedSystemDeploy, SystemDeployData,
};
use models::rust::validator::Validator;
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

pub fn repeated(seed: u8, len: usize) -> Bytes { Bytes::from(vec![seed; len]) }

pub fn validator(seed: u8) -> Validator { repeated(seed, 65) }

pub fn block_hash(seed: u8) -> BlockHash { repeated(seed, 32) }

pub fn slash_deploy(
    invalid_block_hash: BlockHash,
    issuer: Validator,
    target_activation_epoch: i64,
) -> ProcessedSystemDeploy {
    ProcessedSystemDeploy::Succeeded {
        event_list: vec![],
        system_deploy: SystemDeployData::Slash {
            invalid_block_hash,
            issuer_public_key: PublicKey::from_bytes(&issuer),
            target_activation_epoch,
        },
    }
}

pub fn close_deploy() -> ProcessedSystemDeploy {
    ProcessedSystemDeploy::Succeeded {
        event_list: vec![],
        system_deploy: SystemDeployData::CloseBlockSystemDeployData,
    }
}

pub fn failed_deploy() -> ProcessedSystemDeploy {
    ProcessedSystemDeploy::Failed {
        event_list: vec![],
        error_msg: "fuzz".to_string(),
    }
}

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
            version: 1,
            extra_bytes: Bytes::new(),
        },
        body: Body {
            state: F1r3flyState {
                pre_state_hash: repeated(hash_seed.wrapping_add(1), 32),
                post_state_hash: repeated(hash_seed.wrapping_add(2), 32),
                bonds: vec![],
                block_number,
            },
            deploys: vec![],
            rejected_deploys: vec![],
            system_deploys,
            extra_bytes: Bytes::new(),
        },
        justifications: vec![],
        sender,
        seq_num: i32::try_from(block_number).unwrap_or_default(),
        sig: Bytes::new(),
        sig_algorithm: String::new(),
        shard_id: "root".to_string(),
        extra_bytes: Bytes::new(),
    }
}

fn empty_dag() -> KeyValueDagRepresentation {
    let metadata_store = KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new()));
    let deploy_store = KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new()));
    KeyValueDagRepresentation {
        dag_set: imbl::HashSet::new(),
        latest_messages_map: imbl::HashMap::new(),
        child_map: imbl::HashMap::new(),
        height_map: imbl::OrdMap::new(),
        block_number_map: imbl::HashMap::new(),
        main_parent_map: imbl::HashMap::new(),
        self_justification_map: imbl::HashMap::new(),
        invalid_blocks_set: imbl::HashSet::new(),
        last_finalized_block_hash: Bytes::new(),
        finalized_blocks_set: imbl::HashSet::new(),
        block_metadata_index: Arc::new(RwLock::new(BlockMetadataStore::new(metadata_store))),
        deploy_index: Arc::new(RwLock::new(deploy_store)),
    }
}

fn metadata(evidence: &Evidence) -> BlockMetadata {
    BlockMetadata {
        block_hash: evidence.hash.clone(),
        parents: vec![],
        sender: evidence.sender.clone(),
        justifications: vec![],
        weight_map: BTreeMap::new(),
        block_number: evidence.block_number,
        sequence_number: evidence.sequence_number,
        invalid: evidence.invalid,
        directly_finalized: false,
        finalized: false,
        fault_tolerance_value: 0.0,
    }
}

pub fn snapshot(
    evidences: &[Evidence],
    max_block_num: i64,
    epoch_length: i32,
    bonds: Vec<(Validator, i64)>,
) -> CasperSnapshot {
    let mut dag = empty_dag();
    for evidence in evidences {
        let metadata = metadata(evidence);
        dag.dag_set.insert(metadata.block_hash.clone());
        dag.block_number_map
            .insert(metadata.block_hash.clone(), metadata.block_number);
        dag.height_map
            .entry(metadata.block_number)
            .or_insert_with(imbl::HashSet::new)
            .insert(metadata.block_hash.clone());
        if metadata.invalid {
            dag.invalid_blocks_set.insert(metadata.clone());
        }
        dag.block_metadata_index
            .write()
            .expect("metadata lock")
            .add(metadata)
            .expect("metadata insert");
    }
    let bonds_map = bonds.iter().cloned().collect::<HashMap<_, _>>();
    let active_validators = bonds.into_iter().map(|(validator, _)| validator).collect();
    CasperSnapshot {
        dag,
        last_finalized_block: Bytes::new(),
        lca: Bytes::new(),
        tips: vec![],
        parents: vec![],
        justifications: DashSet::new(),
        invalid_blocks: HashMap::new(),
        deploys_in_scope: Arc::new(DashSet::new()),
        max_block_num,
        max_seq_nums: DashMap::new(),
        on_chain_state: OnChainCasperState {
            shard_conf: CasperShardConf {
                epoch_length,
                ..CasperShardConf::new()
            },
            bonds_map,
            active_validators,
        },
    }
}
