#![allow(clippy::mutable_key_type)]
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::dag::block_metadata_store::BlockMetadataStore;
use block_storage::rust::dag::deploy_lifecycle_types::DeployLifecycleTables;
use casper::rust::errors::CasperError;
use casper::rust::merging::deploy_chain_index::DeployChainIndex;
use casper::rust::merging::{block_index, dag_merger};
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::signed::Signed;
use models::rhoapi::expr::ExprInstance;
use models::rust::block_hash::BlockHash;
use models::rust::block_metadata::BlockMetadata;
use models::rust::casper::protocol::casper_message::DeployData;
use models::rust::utils::new_gstring_par;
use parking_lot::RwLock as PlRwLock;
use prost::bytes::Bytes;
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

pub fn block_hash(seed: u8) -> BlockHash { Bytes::from(vec![seed; 32]) }

pub struct IndexedBlock {
    pub hash: BlockHash,
    pub number: i64,
    pub post_state: Bytes,
    pub chains: Vec<DeployChainIndex>,
    pub seeds: HashMap<Bytes, Vec<u8>>,
}

/// Executes `deploys` on `pre_state` as block `hash` and indexes its chains.
pub async fn index_block(
    runtime_manager: &RuntimeManager,
    pre_state: &Bytes,
    deploys: Vec<Signed<DeployData>>,
    creator: &PublicKey,
    hash: BlockHash,
    number: i64,
) -> IndexedBlock {
    let seq_num = 1;
    let deploy_count = deploys.len();
    let seeds: HashMap<Bytes, Vec<u8>> = deploys
        .iter()
        .map(|d| {
            (
                d.sig.clone(),
                models::rust::deploy_seed::deploy_name_seed(d),
            )
        })
        .collect();
    let block_data = BlockData {
        time_stamp: deploys[0].data.time_stamp,
        seq_num,
        block_number: number,
        sender: creator.clone(),
    };
    let (post_state, processed_deploys, processed_system_deploys) = runtime_manager
        .compute_state(pre_state, deploys, vec![], block_data, Some(HashMap::new()))
        .await
        .unwrap();
    assert_eq!(processed_deploys.len(), deploy_count);
    assert!(processed_deploys.iter().all(|d| !d.is_failed));

    let mergeable_channels = runtime_manager
        .load_mergeable_channels(&post_state, creator.bytes.clone(), seq_num)
        .unwrap();
    let chains = block_index::new(
        &hash,
        number,
        &processed_deploys,
        &processed_system_deploys,
        &Blake2b256Hash::from_bytes_prost(pre_state),
        &Blake2b256Hash::from_bytes_prost(&post_state),
        &runtime_manager.get_history_repo(),
        &mergeable_channels,
    )
    .unwrap()
    .deploy_chains;
    IndexedBlock {
        hash,
        number,
        post_state,
        chains,
        seeds,
    }
}

pub fn dag_with_children(
    genesis: &BlockHash,
    children: &[&IndexedBlock],
) -> KeyValueDagRepresentation {
    let with_parents: Vec<(&IndexedBlock, BlockHash)> = children
        .iter()
        .map(|block| (*block, genesis.clone()))
        .collect();
    dag_with_parents(genesis, &with_parents)
}

/// A DAG of `genesis` and each block with its single main parent.
pub fn dag_with_parents(
    genesis: &BlockHash,
    blocks: &[(&IndexedBlock, BlockHash)],
) -> KeyValueDagRepresentation {
    let mut dag_set = imbl::HashSet::new();
    let mut block_number_map = imbl::HashMap::new();
    let mut main_parent_map = imbl::HashMap::new();
    let mut height_map = imbl::OrdMap::new();
    let mut child_map: imbl::HashMap<BlockHash, imbl::HashSet<BlockHash>> = imbl::HashMap::new();
    let mut metadata_store = BlockMetadataStore::new(KeyValueTypedStoreImpl::new(Arc::new(
        InMemoryKeyValueStore::new(),
    )));
    let entries = std::iter::once((genesis, 0i64, None)).chain(
        blocks
            .iter()
            .map(|(block, parent)| (&block.hash, block.number, Some(parent))),
    );
    for (hash, number, parent) in entries {
        dag_set.insert(hash.clone());
        block_number_map.insert(hash.clone(), number);
        height_map
            .entry(number)
            .or_insert_with(imbl::HashSet::new)
            .insert(hash.clone());
        if let Some(parent) = parent {
            main_parent_map.insert(hash.clone(), parent.clone());
            child_map
                .entry(parent.clone())
                .or_default()
                .insert(hash.clone());
            metadata_store
                .add(BlockMetadata {
                    block_hash: hash.clone(),
                    parents: vec![parent.clone()],
                    sender: Bytes::new(),
                    justifications: vec![],
                    weight_map: BTreeMap::new(),
                    block_number: number,
                    sequence_number: 1,
                    equivocates: None,
                    directly_finalized: false,
                    finalized: false,
                    fault_tolerance_value: 0.0,
                    merge_base: Bytes::new(),
                })
                .unwrap();
        }
    }
    let mut finalized_blocks_set = imbl::HashSet::new();
    finalized_blocks_set.insert(genesis.clone());

    block_storage::rust::dag::block_dag_key_value_storage::DagFixture {
        history: None,
        view_sequence: u64::MAX,
        view_max_height: None,
        pruning_cutoff: 0,
        dag_set,
        latest_messages_map: imbl::HashMap::new(),
        child_map,
        height_map,
        block_number_map,
        main_parent_map,
        self_justification_map: imbl::HashMap::new(),
        last_finalized_block_hash: genesis.clone(),
        finalized_blocks_set,
        block_metadata_index: Arc::new(PlRwLock::new(metadata_store)),
        floor_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
        frontier_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
        lifecycle: Arc::new(PlRwLock::new(DeployLifecycleTables::in_memory())),
        carrier_index: Arc::new(PlRwLock::new(
            block_storage::rust::dag::carrier_index::CarrierIndex::in_memory(),
        )),
    }
    .build()
}

/// The string and integer values on a string-named channel, sorted.
pub async fn channel_values(
    runtime_manager: &RuntimeManager,
    state: &Bytes,
    channel: &str,
) -> Vec<String> {
    let data = runtime_manager
        .get_data(
            state.clone(),
            &new_gstring_par(channel.to_string(), vec![], false),
        )
        .await
        .unwrap();
    let mut values: Vec<String> = data
        .iter()
        .flat_map(|par| par.exprs.iter())
        .filter_map(|expr| match &expr.expr_instance {
            Some(ExprInstance::GString(s)) => Some(s.clone()),
            Some(ExprInstance::GInt(n)) => Some(n.to_string()),
            _ => None,
        })
        .collect();
    values.sort();
    values
}

pub struct Merged {
    pub state: Bytes,
    pub rejected: HashSet<Bytes>,
    pub applied: HashSet<Bytes>,
}

/// Merges `blocks` onto `base`. A chain carrying `expensive_sig` is the
/// costliest to reject; every other chain costs its deploys' cost.
pub fn merge_blocks(
    runtime_manager: &RuntimeManager,
    dag: &KeyValueDagRepresentation,
    base: &BlockHash,
    base_state: &Bytes,
    blocks: &[&IndexedBlock],
    expensive_sig: Option<Bytes>,
) -> Merged {
    let scope: HashSet<BlockHash> = blocks.iter().map(|block| block.hash.clone()).collect();
    let chains: Vec<(BlockHash, Vec<DeployChainIndex>)> = blocks
        .iter()
        .map(|block| (block.hash.clone(), block.chains.clone()))
        .collect();
    let name_seeds = dag_merger::NameSeeds {
        of_scope_deploys: blocks
            .iter()
            .flat_map(|block| block.seeds.iter())
            .map(|(sig, seed)| (sig.clone(), seed.clone()))
            .collect(),
        held_by_base: HashMap::new(),
    };
    let (state, rejected, applied) = dag_merger::merge(
        dag,
        base,
        &Blake2b256Hash::from_bytes_prost(base_state),
        move |hash: &BlockHash| -> Result<Vec<DeployChainIndex>, CasperError> {
            Ok(chains
                .iter()
                .find(|(h, _)| h == hash)
                .map(|(_, chains)| chains.clone())
                .unwrap_or_default())
        },
        &runtime_manager.get_history_repo(),
        move |chain: &DeployChainIndex| {
            if chain
                .deploys_with_cost
                .0
                .iter()
                .any(|d| Some(&d.deploy_id) == expensive_sig.as_ref())
            {
                1_000_000
            } else {
                chain.deploys_with_cost.0.iter().map(|d| d.cost).sum()
            }
        },
        Some(scope),
        0,
        50,
        &|_| Ok(false),
        &|_| Ok(false),
        &HashSet::new(),
        &HashMap::new(),
        &name_seeds,
        casper::rust::merging::conflict_set_merger::RejectionSearch::EXHAUSTIVE,
    )
    .expect("merge");
    let rejected: HashSet<Bytes> = rejected.iter().map(|r| r.sig.clone()).collect();
    assert!(
        rejected.is_disjoint(&applied),
        "a deploy is both rejected and applied: rejected={rejected:?} applied={applied:?}"
    );
    Merged {
        state: state.to_bytes_prost(),
        rejected,
        applied,
    }
}
