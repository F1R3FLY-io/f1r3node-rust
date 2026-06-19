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
            issuer_public_key: PublicKey::from_bytes(&issuer),
            target_activation_epoch,
        },
    }
}

/// Builder for a CloseBlock system deploy (the per-block terminator).
pub fn close_deploy() -> ProcessedSystemDeploy {
    ProcessedSystemDeploy::Succeeded {
        event_list: vec![],
        system_deploy: SystemDeployData::CloseBlockSystemDeployData,
    }
}

/// Builder for the Failed variant. Sibling to `slash_deploy` and
/// `close_deploy` — these three together exhaust the variant space the
/// production validator inspects.
pub fn failed_deploy() -> ProcessedSystemDeploy {
    ProcessedSystemDeploy::Failed {
        event_list: vec![],
        error_msg: "fuzz".to_string(),
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

/// Build a fully-zeroed `KeyValueDagRepresentation` against
/// `InMemoryKeyValueStore`. The `InMemory` choice is load-bearing here —
/// fuzz iterations must not hit disk, must not share state across
/// iterations, and must complete in microseconds. No LMDB, no global
/// lock, no per-iteration cleanup.
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

/// Build a `CasperSnapshot` whose DAG and on-chain state are populated
/// from `evidences` + `bonds`. The four DAG collections —
/// `dag_set`, `height_map`, `block_metadata_index`, and (conditionally)
/// `invalid_blocks_set` — are populated in lockstep for each evidence;
/// production code assumes them consistent and panics or returns
/// `KeyNotFound` if they aren't. Any future change to those collections
/// must update this builder in the same atomic step.
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
