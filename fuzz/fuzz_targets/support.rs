//! Shared fixture builders for the slashing fuzz harnesses.
//!
//! Provides:
//!   * Signing keys and blocks signed with them, so evidence carries real
//!     signatures.
//!   * `ProcessedSystemDeploy` builders for the relevant variants.
//!   * `snapshot`, an in-memory `CasperSnapshot` holding equivocations the
//!     way validation stores them, and `block_store` holding their bodies.
//!
//! `#[allow(dead_code)]` is at the module level because each `fuzz_target`
//! is a separate binary and uses a different subset of these helpers.

#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::dag::block_metadata_store::BlockMetadataStore;
use block_storage::rust::dag::deploy_lifecycle_types::DeployLifecycleTables;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use casper::rust::casper::{CasperShardConf, CasperSnapshot, OnChainCasperState};
use casper::rust::util::proto_util;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use dashmap::DashSet;
use models::rust::block_hash::BlockHash;
use models::rust::block_metadata::BlockMetadata;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Body, EquivocationEvidence, F1r3flyState, Header, ProcessedSystemDeploy,
    SystemDeployData,
};
use models::rust::validator::Validator;
use parking_lot::RwLock;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

pub const SHARD: &str = "root";
pub const CHAIN: [u8; 32] = [0xC1; 32];

pub fn repeated(seed: u8, len: usize) -> Bytes { Bytes::from(vec![seed; len]) }

pub fn block_hash(seed: u8) -> BlockHash { repeated(seed, 32) }

/// A secp256k1 key; `index` 0..=254 gives distinct valid keys.
pub fn key(index: u8) -> PrivateKey { PrivateKey::from_bytes(&[index.wrapping_add(1); 32]) }

pub fn public(key: &PrivateKey) -> Validator { Secp256k1.to_public(key).bytes.into() }

/// A block `key` signs at `seq_num` for `shard` and `chain`, told apart from
/// its siblings by `nonce`.
pub fn signed_block(
    key: &PrivateKey,
    seq_num: i32,
    nonce: u8,
    shard: &str,
    chain: &[u8],
) -> BlockMessage {
    let mut block = block_with_system_deploys(nonce, public(key), i64::from(seq_num), vec![]);
    block.seq_num = seq_num;
    block.shard_id = shard.to_string();
    block.header.chain_id = Bytes::copy_from_slice(chain);
    block.extra_bytes = Bytes::from(vec![nonce]);
    block.sig_algorithm = Secp256k1::name();
    block.block_hash = proto_util::hash_block(&block);
    block.sig = Secp256k1.sign(&block.block_hash, &key.bytes).into();
    block
}

/// The evidence two blocks form, ordered by block hash.
pub fn evidence(a: &BlockMessage, b: &BlockMessage) -> EquivocationEvidence {
    let (first, second) = if a.block_hash <= b.block_hash {
        (a, b)
    } else {
        (b, a)
    };
    EquivocationEvidence {
        first: proto_util::block_commitment(first),
        second: proto_util::block_commitment(second),
    }
}

pub fn slash_deploy(evidence: EquivocationEvidence, issuer: &Validator) -> ProcessedSystemDeploy {
    ProcessedSystemDeploy::Succeeded {
        event_list: vec![],
        system_deploy: SystemDeployData::create_slash(evidence, PublicKey::from_bytes(issuer)),
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

/// An unsigned block whose header timestamp, block number and seq number all
/// equal `block_number`.
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
            chain_id: Bytes::copy_from_slice(&CHAIN),
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
            applied_from_scope: vec![],
            merge_base: Bytes::new(),
        },
        justifications: vec![],
        sender,
        seq_num: i32::try_from(block_number).expect("fuzz block numbers fit a sequence number"),
        sig: Bytes::new(),
        sig_algorithm: String::new(),
        shard_id: SHARD.to_string(),
        extra_bytes: Bytes::new(),
    }
}

/// A block the snapshot holds: valid when `equivocates` is `None`, otherwise
/// held as an equivocation with that sibling.
#[derive(Clone, Debug)]
pub struct Held {
    pub block: BlockMessage,
    pub equivocates: Option<BlockHash>,
}

fn empty_dag() -> KeyValueDagRepresentation {
    let metadata_store = KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new()));
    KeyValueDagRepresentation {
        dag_set: imbl::HashSet::new(),
        latest_messages_map: imbl::HashMap::new(),
        child_map: imbl::HashMap::new(),
        height_map: imbl::OrdMap::new(),
        block_number_map: imbl::HashMap::new(),
        main_parent_map: imbl::HashMap::new(),
        self_justification_map: imbl::HashMap::new(),
        sequence_map: imbl::HashMap::new(),
        invalid_blocks_set: imbl::HashSet::new(),
        last_finalized_block_hash: Bytes::new(),
        finalized_blocks_set: imbl::HashSet::new(),
        block_metadata_index: Arc::new(RwLock::new(BlockMetadataStore::new(metadata_store))),
        floor_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
        frontier_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
        lifecycle: Arc::new(RwLock::new(DeployLifecycleTables::in_memory())),
        carrier_index: Arc::new(RwLock::new(
            block_storage::rust::dag::carrier_index::CarrierIndex::in_memory(),
        )),
    }
}

fn metadata(held: &Held) -> BlockMetadata {
    BlockMetadata {
        block_hash: held.block.block_hash.clone(),
        parents: vec![],
        sender: held.block.sender.clone(),
        justifications: vec![],
        weight_map: BTreeMap::new(),
        block_number: held.block.body.state.block_number,
        sequence_number: held.block.seq_num,
        equivocates: held.equivocates.clone(),
        directly_finalized: false,
        finalized: false,
        fault_tolerance_value: 0.0,
        merge_base: Bytes::new(),
    }
}

/// A snapshot holding `held` and bonding `bonds`. A repeated block hash is
/// held once, first occurrence first, as a DAG holds one metadata per hash.
pub fn snapshot(held: &[Held], max_block_num: i64, bonds: Vec<(Validator, i64)>) -> CasperSnapshot {
    let mut dag = empty_dag();
    let mut seen: HashSet<BlockHash> = HashSet::new();
    for item in held {
        if !seen.insert(item.block.block_hash.clone()) {
            continue;
        }
        let metadata = metadata(item);
        dag.dag_set.insert(metadata.block_hash.clone());
        dag.block_number_map
            .insert(metadata.block_hash.clone(), metadata.block_number);
        dag.sequence_map
            .entry((metadata.sender.clone(), metadata.sequence_number))
            .or_default()
            .insert(metadata.block_hash.clone());
        if metadata.is_invalid() {
            dag.invalid_blocks_set.insert(metadata.clone());
        } else {
            dag.height_map
                .entry(metadata.block_number)
                .or_default()
                .insert(metadata.block_hash.clone());
        }
        dag.block_metadata_index
            .write()
            .add(metadata)
            .expect("metadata insert");
    }
    let bonds_map = bonds.iter().cloned().collect::<HashMap<_, _>>();
    let active_validators = bonds.into_iter().map(|(validator, _)| validator).collect();
    CasperSnapshot {
        dag,
        last_finalized_block: Bytes::new(),
        parents: vec![],
        justifications: HashSet::new(),
        invalid_blocks: HashMap::new(),
        deploys_in_scope: Arc::new(DashSet::new()),
        rejected_in_scope: Arc::new(DashSet::new()),
        max_block_num,
        max_seq_nums: HashMap::new(),
        on_chain_state: OnChainCasperState {
            shard_conf: CasperShardConf {
                shard_name: SHARD.to_string(),
                ..CasperShardConf::new()
            },
            bonds_map,
            active_validators,
        },
        chain_id: Bytes::copy_from_slice(&CHAIN),
    }
}

/// A block store holding every block of `held`.
pub fn block_store(held: &[Held]) -> KeyValueBlockStore {
    let store = KeyValueBlockStore::new(
        Arc::new(InMemoryKeyValueStore::new()),
        Arc::new(InMemoryKeyValueStore::new()),
    );
    for item in held {
        store.put_block_message(&item.block).expect("block insert");
    }
    store
}
