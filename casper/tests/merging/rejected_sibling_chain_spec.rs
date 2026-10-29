#![allow(clippy::mutable_key_type)]
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::dag::block_metadata_store::BlockMetadataStore;
use block_storage::rust::dag::deploy_lifecycle_types::DeployLifecycleTables;
use casper::rust::errors::CasperError;
use casper::rust::merging::deploy_chain_index::DeployChainIndex;
use casper::rust::merging::{block_index, dag_merger};
use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use crypto::rust::private_key::PrivateKey;
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

use crate::util::rholang::resources::with_runtime_manager;

const SHARED_CHANNEL: &str = "issue-873-shared";
const CONFLICT_CHANNEL: &str = "issue-873-conflict";

fn block_hash(seed: u8) -> BlockHash { Bytes::from(vec![seed; 32]) }

fn deploy(source: &str, sec: &PrivateKey) -> Signed<DeployData> {
    construct_deploy::source_deploy_now_full(
        source.to_string(),
        None,
        None,
        Some(sec.clone()),
        None,
        None,
    )
    .unwrap()
}

struct IndexedBlock {
    hash: BlockHash,
    number: i64,
    chains: Vec<DeployChainIndex>,
}

async fn index_block(
    runtime_manager: &RuntimeManager,
    pre_state: &Bytes,
    deploys: Vec<Signed<DeployData>>,
    creator: &PublicKey,
    hash: BlockHash,
    number: i64,
) -> IndexedBlock {
    let seq_num = 1;
    let deploy_count = deploys.len();
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
        chains,
    }
}

fn dag_with_children(genesis: &BlockHash, children: &[&IndexedBlock]) -> KeyValueDagRepresentation {
    let mut dag_set = imbl::HashSet::new();
    let mut block_number_map = imbl::HashMap::new();
    let mut main_parent_map = imbl::HashMap::new();
    let mut height_map = imbl::OrdMap::new();
    let mut children_of_genesis = imbl::HashSet::new();
    let mut metadata_store = BlockMetadataStore::new(KeyValueTypedStoreImpl::new(Arc::new(
        InMemoryKeyValueStore::new(),
    )));
    let blocks = std::iter::once((genesis, 0i64))
        .chain(children.iter().map(|block| (&block.hash, block.number)));
    for (hash, number) in blocks {
        dag_set.insert(hash.clone());
        block_number_map.insert(hash.clone(), number);
        height_map
            .entry(number)
            .or_insert_with(imbl::HashSet::new)
            .insert(hash.clone());
        if number > 0 {
            main_parent_map.insert(hash.clone(), genesis.clone());
            children_of_genesis.insert(hash.clone());
            metadata_store
                .add(BlockMetadata {
                    block_hash: hash.clone(),
                    parents: vec![genesis.clone()],
                    sender: Bytes::new(),
                    justifications: vec![],
                    weight_map: BTreeMap::new(),
                    block_number: number,
                    sequence_number: 1,
                    invalid: false,
                    directly_finalized: false,
                    finalized: false,
                    fault_tolerance_value: 0.0,
                    merge_base: Bytes::new(),
                })
                .unwrap();
        }
    }
    let mut child_map = imbl::HashMap::new();
    child_map.insert(genesis.clone(), children_of_genesis);
    let mut finalized_blocks_set = imbl::HashSet::new();
    finalized_blocks_set.insert(genesis.clone());

    KeyValueDagRepresentation {
        dag_set,
        latest_messages_map: imbl::HashMap::new(),
        child_map,
        height_map,
        block_number_map,
        main_parent_map,
        self_justification_map: imbl::HashMap::new(),
        invalid_blocks_set: imbl::HashSet::new(),
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
}

struct MergeOutcome {
    shared_values: Vec<String>,
    rejected: HashSet<Bytes>,
    applied: HashSet<Bytes>,
}

async fn merge_on_genesis(
    runtime_manager: &RuntimeManager,
    genesis: &BlockHash,
    base_state: &Bytes,
    blocks: Vec<IndexedBlock>,
    expensive_sig: Option<Bytes>,
) -> MergeOutcome {
    let dag = dag_with_children(genesis, &blocks.iter().collect::<Vec<_>>());
    let scope: HashSet<BlockHash> = blocks.iter().map(|block| block.hash.clone()).collect();
    let (merged_state, rejected, _mergeable, applied) = dag_merger::merge(
        &dag,
        genesis,
        &Blake2b256Hash::from_bytes_prost(base_state),
        move |hash: &BlockHash| -> Result<Vec<DeployChainIndex>, CasperError> {
            Ok(blocks
                .iter()
                .find(|block| block.hash == *hash)
                .map(|block| block.chains.clone())
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
    )
    .expect("merge");

    let shared = runtime_manager
        .get_data(
            merged_state.to_bytes_prost(),
            &new_gstring_par(SHARED_CHANNEL.to_string(), vec![], false),
        )
        .await
        .unwrap();
    let mut shared_values: Vec<String> = shared
        .iter()
        .flat_map(|par| par.exprs.iter())
        .filter_map(|expr| match &expr.expr_instance {
            Some(ExprInstance::GString(s)) => Some(s.clone()),
            _ => None,
        })
        .collect();
    shared_values.sort();
    let rejected: HashSet<Bytes> = rejected.iter().map(|r| r.sig.clone()).collect();
    assert!(
        rejected.is_disjoint(&applied),
        "a deploy is both rejected and applied: rejected={rejected:?} applied={applied:?}"
    );
    MergeOutcome {
        shared_values,
        rejected,
        applied,
    }
}

fn assert_independent_chains(block: &IndexedBlock, first: &Bytes, second: &Bytes) {
    let chain_of = |sig: &Bytes| {
        block
            .chains
            .iter()
            .position(|chain| {
                chain
                    .deploys_with_cost
                    .0
                    .iter()
                    .any(|d| d.deploy_id == *sig)
            })
            .unwrap()
    };
    assert_ne!(
        chain_of(first),
        chain_of(second),
        "the two deploys must be independent deploy chains of one block"
    );
}

/// Issue #873: block B holds two independent deploys that both send to one
/// non-mergeable channel. D2 also sends to a channel on which a deploy of a
/// sibling block waits, so the merge rejects the chain of D2. The chain of D1
/// carries the block diff of the shared channel, which holds the datum of D2,
/// so the merge must reject it too: the merged state never holds the datum of
/// a rejected deploy.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_kept_sibling_chain_does_not_apply_the_datum_of_a_rejected_sibling() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let base_state = genesis_block.body.state.post_state_hash.clone();
            let genesis = BlockHash::from(genesis_block.block_hash.to_vec());
            let vaults = &genesis_context.genesis_vaults;
            let validators = &genesis_context.validator_key_pairs;

            let d1 = deploy(&format!(r#"@"{SHARED_CHANNEL}"!("d1")"#), &vaults[0].0);
            let d2 = deploy(
                &format!(r#"@"{SHARED_CHANNEL}"!("d2") | @"{CONFLICT_CHANNEL}"!(0)"#),
                &vaults[1].0,
            );
            let waiter = deploy(
                &format!(r#"for (_ <- @"{CONFLICT_CHANNEL}") {{ Nil }}"#),
                &vaults[2].0,
            );
            let (d1_sig, d2_sig, waiter_sig) = (d1.sig.clone(), d2.sig.clone(), waiter.sig.clone());

            let block_b = index_block(
                &runtime_manager,
                &base_state,
                vec![d1, d2],
                &validators[0].1,
                block_hash(0xB1),
                1,
            )
            .await;
            assert_independent_chains(&block_b, &d1_sig, &d2_sig);
            let block_c = index_block(
                &runtime_manager,
                &base_state,
                vec![waiter],
                &validators[1].1,
                block_hash(0xC1),
                1,
            )
            .await;

            let outcome = merge_on_genesis(
                &runtime_manager,
                &genesis,
                &base_state,
                vec![block_b, block_c],
                Some(waiter_sig.clone()),
            )
            .await;

            assert!(
                outcome.rejected.contains(&d2_sig),
                "the merge must reject D2, which races the waiter of block C"
            );
            assert!(
                outcome.applied.contains(&waiter_sig),
                "the merge must keep the waiter of block C"
            );
            assert_eq!(
                outcome.shared_values,
                Vec::<String>::new(),
                "the shared channel must hold no datum of a rejected deploy"
            );
            assert!(
            outcome.rejected.contains(&d1_sig),
            "D1 shares a non-mergeable channel with the rejected D2, so it must be rejected too"
        );
        },
    )
    .await
    .unwrap()
}

/// Issue #873, recovery path: D2 of block B is re-proposed in block B2, so the
/// merge drops the stale chain of D2 in B as a duplicate. The sibling chain of
/// D1 in B still carries the block diff of the shared channel with the stale
/// datum of D2. The merge must reject that sibling, so the shared channel holds
/// only the datum of the fresh copy of D2.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_kept_sibling_chain_does_not_apply_the_stale_datum_of_a_deduplicated_sibling() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let base_state = genesis_block.body.state.post_state_hash.clone();
            let genesis = BlockHash::from(genesis_block.block_hash.to_vec());
            let vaults = &genesis_context.genesis_vaults;
            let validators = &genesis_context.validator_key_pairs;

            let d1 = deploy(&format!(r#"@"{SHARED_CHANNEL}"!("d1")"#), &vaults[0].0);
            let d2 = deploy(
                &format!(
                    r#"new bd(`rho:block:data`), bdCh in {{
                     bd!(*bdCh) |
                     for (@number, @_, @_ <- bdCh) {{
                       @"{SHARED_CHANNEL}"!("d2@${{n}}" %% {{"n": number}})
                     }}
                   }}"#
                ),
                &vaults[1].0,
            );
            let (d1_sig, d2_sig) = (d1.sig.clone(), d2.sig.clone());

            let block_b = index_block(
                &runtime_manager,
                &base_state,
                vec![d1, d2.clone()],
                &validators[0].1,
                block_hash(0xB1),
                1,
            )
            .await;
            assert_independent_chains(&block_b, &d1_sig, &d2_sig);
            let block_b2 = index_block(
                &runtime_manager,
                &base_state,
                vec![d2],
                &validators[1].1,
                block_hash(0xB2),
                2,
            )
            .await;

            let outcome = merge_on_genesis(
                &runtime_manager,
                &genesis,
                &base_state,
                vec![block_b, block_b2],
                None,
            )
            .await;

            assert!(
                outcome.applied.contains(&d2_sig),
                "the merge must apply the fresh copy of D2 from block B2"
            );
            assert_eq!(
                outcome.shared_values,
                vec!["d2@2".to_string()],
                "the shared channel must hold only the datum of the fresh copy of D2"
            );
            assert!(
            outcome.rejected.contains(&d1_sig),
            "D1 shares a non-mergeable channel with the dropped stale D2, so it must be rejected"
        );
        },
    )
    .await
    .unwrap()
}
