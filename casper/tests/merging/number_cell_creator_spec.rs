#![allow(clippy::mutable_key_type)]
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::dag::block_metadata_store::BlockMetadataStore;
use block_storage::rust::dag::deploy_lifecycle_types::DeployLifecycleTables;
use casper::rust::merging::deploy_chain_index::DeployChainIndex;
use casper::rust::merging::{block_index, dag_merger};
use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use crypto::rust::hash::keccak256::Keccak256;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::signed::Signed;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use models::rust::block_hash::BlockHash;
use models::rust::block_metadata::BlockMetadata;
use models::rust::casper::protocol::casper_message::{BlockMessage, DeployData};
use parking_lot::RwLock as PlRwLock;
use prost::bytes::Bytes;
use prost::Message;
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

use crate::util::genesis_builder::GenesisContext;
use crate::util::rholang::resources::with_runtime_manager;

const MAP: &str = "number-cell-creator-map";
const CONFLICT: &str = "number-cell-creator-conflict";

fn block_hash(seed: u8) -> BlockHash { Bytes::from(vec![seed; 32]) }

fn deploy(source: String, sec: &PrivateKey) -> Signed<DeployData> {
    construct_deploy::source_deploy_now_full(
        source,
        Some(5_000_000),
        None,
        Some(sec.clone()),
        None,
        None,
    )
    .unwrap()
}

/// The two TreeHashMap path nybbles of a string key at depth 1: the root's
/// child bit, then the bit in that child node.
fn nybbles(key: &str) -> (u8, u8) {
    let par = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GString(key.to_string())),
    }]);
    let hash = Keccak256::hash(par.encode_to_vec());
    (hash[0] % 16, hash[0] / 16)
}

/// Keys whose paths share one interior node: the first creates it, the others
/// only set further bits in it.
fn keys_through_one_node() -> (String, String, String) {
    let k1 = "creator-key-0".to_string();
    let (n0, n1) = nybbles(&k1);
    let sibling = |skip: u8| {
        (1..10_000)
            .map(|i| format!("creator-key-{i}"))
            .find(|k| {
                let (a, b) = nybbles(k);
                a == n0 && b != n1 && b != skip
            })
            .expect("a key through the same node")
    };
    let k2 = sibling(n1);
    let k3 = sibling(nybbles(&k2).1);
    (k1, k2, k3)
}

fn init_map() -> String {
    format!(
        r#"new rl(`rho:registry:lookup`), thmCh, mapCh in {{
  rl!(`rho:lang:treeHashMap`, *thmCh) |
  for (TreeHashMap <- thmCh) {{
    TreeHashMap!("init", 1, *mapCh) |
    for (@map <- mapCh) {{ @"{MAP}"!(map) }}
  }}
}}"#
    )
}

fn insert(key: &str, conflict: bool) -> String {
    let race = if conflict {
        format!(r#" | @"{CONFLICT}"!(0)"#)
    } else {
        String::new()
    };
    format!(
        r#"new rl(`rho:registry:lookup`), thmCh, ret in {{
  rl!(`rho:lang:treeHashMap`, *thmCh) |
  for (TreeHashMap <- thmCh; @map <<- @"{MAP}") {{
    TreeHashMap!("set", map, "{key}", "v", *ret)
  }}{race}
}}"#
    )
}

struct Block {
    hash: BlockHash,
    number: i64,
    parents: Vec<BlockHash>,
    post_state: Bytes,
    chains: Vec<DeployChainIndex>,
}

async fn build(
    runtime_manager: &RuntimeManager,
    pre_state: &Bytes,
    deploys: Vec<Signed<DeployData>>,
    creator: &PublicKey,
    hash: BlockHash,
    number: i64,
    parents: Vec<BlockHash>,
) -> Block {
    let block_data = BlockData {
        time_stamp: deploys[0].data.time_stamp,
        seq_num: 1,
        block_number: number,
        sender: creator.clone(),
    };
    let (post_state, processed, system) = runtime_manager
        .compute_state(pre_state, deploys, vec![], block_data, Some(HashMap::new()))
        .await
        .unwrap();
    assert!(
        processed.iter().all(|d| !d.is_failed),
        "every deploy of block {number} must execute"
    );
    let mergeable = runtime_manager
        .load_mergeable_channels(&post_state, creator.bytes.clone(), 1)
        .unwrap();
    let chains = block_index::new(
        &hash,
        number,
        &processed,
        &system,
        &Blake2b256Hash::from_bytes_prost(pre_state),
        &Blake2b256Hash::from_bytes_prost(&post_state),
        &runtime_manager.get_history_repo(),
        &mergeable,
    )
    .unwrap()
    .deploy_chains;
    Block {
        hash,
        number,
        parents,
        post_state,
        chains,
    }
}

fn dag(genesis: &BlockHash, blocks: &[&Block]) -> KeyValueDagRepresentation {
    let mut dag_set = imbl::HashSet::new();
    let mut block_number_map = imbl::HashMap::new();
    let mut main_parent_map = imbl::HashMap::new();
    let mut height_map = imbl::OrdMap::new();
    let mut child_map: imbl::HashMap<BlockHash, imbl::HashSet<BlockHash>> = imbl::HashMap::new();
    let mut metadata_store = BlockMetadataStore::new(KeyValueTypedStoreImpl::new(Arc::new(
        InMemoryKeyValueStore::new(),
    )));
    dag_set.insert(genesis.clone());
    block_number_map.insert(genesis.clone(), 0i64);
    height_map
        .entry(0)
        .or_insert_with(imbl::HashSet::new)
        .insert(genesis.clone());
    for block in blocks {
        dag_set.insert(block.hash.clone());
        block_number_map.insert(block.hash.clone(), block.number);
        height_map
            .entry(block.number)
            .or_insert_with(imbl::HashSet::new)
            .insert(block.hash.clone());
        main_parent_map.insert(block.hash.clone(), block.parents[0].clone());
        for parent in &block.parents {
            child_map
                .entry(parent.clone())
                .or_default()
                .insert(block.hash.clone());
        }
        metadata_store
            .add(BlockMetadata {
                block_hash: block.hash.clone(),
                parents: block.parents.clone(),
                sender: Bytes::new(),
                justifications: vec![],
                weight_map: BTreeMap::new(),
                block_number: block.number,
                sequence_number: 1,
                equivocates: None,
                directly_finalized: false,
                finalized: false,
                fault_tolerance_value: 0.0,
                merge_base: Bytes::new(),
            })
            .unwrap();
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

/// Merges `scope` onto `base`; `expensive` is the sig the cost function makes
/// costliest to reject. Returns (merged state, rejected sigs, applied sigs).
fn merge(
    runtime_manager: &RuntimeManager,
    dag: &KeyValueDagRepresentation,
    base: &BlockHash,
    base_state: &Bytes,
    scope: Vec<(BlockHash, Vec<DeployChainIndex>)>,
    expensive: Option<Bytes>,
    seeds: &HashMap<Bytes, Vec<u8>>,
) -> (Bytes, HashSet<Bytes>, HashSet<Bytes>) {
    let hashes: HashSet<BlockHash> = scope.iter().map(|(h, _)| h.clone()).collect();
    let name_seeds = dag_merger::NameSeeds {
        of_scope_deploys: scope
            .iter()
            .flat_map(|(_, chains)| chains.iter())
            .flat_map(|chain| chain.deploys_with_cost.0.iter())
            .filter_map(|d| {
                seeds
                    .get(&d.deploy_id)
                    .map(|s| (d.deploy_id.clone(), s.clone()))
            })
            .collect(),
        held_by_base: HashMap::new(),
    };
    let (state, rejected, applied) = dag_merger::merge(
        dag,
        base,
        &Blake2b256Hash::from_bytes_prost(base_state),
        move |hash: &BlockHash| -> Result<Vec<DeployChainIndex>, casper::rust::errors::CasperError> {
            Ok(scope
                .iter()
                .find(|(h, _)| h == hash)
                .map(|(_, c)| c.clone())
                .unwrap_or_default())
        },
        &runtime_manager.get_history_repo(),
        move |chain: &DeployChainIndex| {
            if chain
                .deploys_with_cost
                .0
                .iter()
                .any(|d| Some(&d.deploy_id) == expensive.as_ref())
            {
                1_000_000
            } else {
                chain.deploys_with_cost.0.iter().map(|d| d.cost).sum()
            }
        },
        Some(hashes),
        0,
        50,
        &|_| Ok(false),
        &|_| Ok(false),
        &HashSet::new(),
        &HashMap::new(),
        &name_seeds,
    )
    .expect("merge");
    (
        state.to_bytes_prost(),
        rejected.iter().map(|r| r.sig.clone()).collect(),
        applied,
    )
}

fn seeds_of(deploys: &[&Signed<DeployData>]) -> HashMap<Bytes, Vec<u8>> {
    deploys
        .iter()
        .map(|d| {
            (
                d.sig.clone(),
                models::rust::deploy_seed::deploy_name_seed(d),
            )
        })
        .collect()
}

async fn execute(
    runtime_manager: &RuntimeManager,
    state: &Bytes,
    deploy: Signed<DeployData>,
    number: i64,
    sender: &PublicKey,
) -> Bytes {
    let block_data = BlockData {
        time_stamp: deploy.data.time_stamp,
        seq_num: 1,
        block_number: number,
        sender: sender.clone(),
    };
    let (post_state, processed, _) = runtime_manager
        .compute_state(
            state,
            vec![deploy],
            vec![],
            block_data,
            Some(HashMap::new()),
        )
        .await
        .expect("execution");
    assert!(
        processed.iter().all(|d| !d.is_failed),
        "the deploy must execute"
    );
    post_state
}

/// D's insert creates the interior node N of a map; an earlier merge X
/// rejects D. E, built on D's state, inserts a sibling key through N, so its
/// delta on N was computed on a value the base never held. The merge of E's
/// block onto X must reject E; recovery then runs D again, which creates N
/// once, and both E and a later insert through N run on the result.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_write_to_a_number_cell_whose_creator_an_earlier_merge_rejected_is_rejected() {
    with_runtime_manager(
        |rm, ctx: GenesisContext, genesis_block: BlockMessage| async move {
            let genesis_state = genesis_block.body.state.post_state_hash.clone();
            let vault = |i: usize| ctx.genesis_vaults[i % ctx.genesis_vaults.len()].0.clone();
            let validator = |i: usize| {
                ctx.validator_key_pairs[i % ctx.validator_key_pairs.len()]
                    .1
                    .clone()
            };
            let (k1, k2, k3) = keys_through_one_node();
            let root = BlockHash::from(genesis_block.block_hash.to_vec());

            let p = build(
                &rm,
                &genesis_state,
                vec![deploy(init_map(), &vault(0))],
                &validator(0),
                block_hash(0xA0),
                1,
                vec![root.clone()],
            )
            .await;
            let d = deploy(insert(&k1, true), &vault(1));
            let c = deploy(format!(r#"for (_ <- @"{CONFLICT}") {{ Nil }}"#), &vault(2));
            let e = deploy(insert(&k2, false), &vault(3));
            let (d_sig, c_sig, e_sig) = (d.sig.clone(), c.sig.clone(), e.sig.clone());
            let seeds = seeds_of(&[&d, &c, &e]);

            let a = build(
                &rm,
                &p.post_state,
                vec![d.clone()],
                &validator(1),
                block_hash(0xA1),
                2,
                vec![p.hash.clone()],
            )
            .await;
            let c_block = build(
                &rm,
                &p.post_state,
                vec![c],
                &validator(2),
                block_hash(0xC1),
                2,
                vec![p.hash.clone()],
            )
            .await;
            let b = build(
                &rm,
                &a.post_state,
                vec![e.clone()],
                &validator(3),
                block_hash(0xB1),
                3,
                vec![a.hash.clone()],
            )
            .await;

            let dag_x = dag(&root, &[&p, &a, &c_block]);
            let (x_state, rejected_x, _) = merge(
                &rm,
                &dag_x,
                &p.hash,
                &p.post_state,
                vec![
                    (a.hash.clone(), a.chains.clone()),
                    (c_block.hash.clone(), c_block.chains.clone()),
                ],
                Some(c_sig),
                &seeds,
            );
            assert!(
                rejected_x.contains(&d_sig),
                "the merge of A and C must reject D"
            );

            let x = Block {
                hash: block_hash(0xD0),
                number: 3,
                parents: vec![c_block.hash.clone(), a.hash.clone()],
                post_state: x_state.clone(),
                chains: vec![],
            };
            let dag_y = dag(&root, &[&p, &a, &c_block, &x, &b]);
            let (y_state, rejected_y, applied_y) = merge(
                &rm,
                &dag_y,
                &x.hash,
                &x.post_state,
                vec![(b.hash.clone(), b.chains.clone())],
                None,
                &seeds,
            );
            assert!(
                !applied_y.contains(&e_sig),
                "the merge onto X must not apply E"
            );
            assert!(
                rejected_y.contains(&e_sig),
                "E must be recorded for recovery"
            );

            let recovered = execute(&rm, &y_state, d, 5, &validator(1)).await;
            let e_again = execute(&rm, &recovered, e, 6, &validator(3)).await;
            execute(
                &rm,
                &e_again,
                deploy(insert(&k3, false), &vault(4)),
                7,
                &validator(2),
            )
            .await;
            Ok::<(), casper::rust::errors::CasperError>(())
        },
    )
    .await
    .unwrap()
    .unwrap();
}

/// D creates N and E, built on D, writes to it; both land in one merge with
/// nothing rejected. E's delta is backed by D's creation, which the merge
/// keeps, even when the two sit in separate branches, so E must be kept.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_write_to_a_number_cell_whose_creator_the_merge_keeps_is_kept() {
    with_runtime_manager(
        |rm, ctx: GenesisContext, genesis_block: BlockMessage| async move {
            let genesis_state = genesis_block.body.state.post_state_hash.clone();
            let vault = |i: usize| ctx.genesis_vaults[i % ctx.genesis_vaults.len()].0.clone();
            let validator = |i: usize| {
                ctx.validator_key_pairs[i % ctx.validator_key_pairs.len()]
                    .1
                    .clone()
            };
            let (k1, k2, k3) = keys_through_one_node();
            let root = BlockHash::from(genesis_block.block_hash.to_vec());

            let p = build(
                &rm,
                &genesis_state,
                vec![deploy(init_map(), &vault(0))],
                &validator(0),
                block_hash(0xA0),
                1,
                vec![root.clone()],
            )
            .await;
            let d = deploy(insert(&k1, false), &vault(1));
            let e = deploy(insert(&k2, false), &vault(3));
            let (d_sig, e_sig) = (d.sig.clone(), e.sig.clone());
            let seeds = seeds_of(&[&d, &e]);

            let a = build(
                &rm,
                &p.post_state,
                vec![d],
                &validator(1),
                block_hash(0xA1),
                2,
                vec![p.hash.clone()],
            )
            .await;
            let b = build(
                &rm,
                &a.post_state,
                vec![e],
                &validator(3),
                block_hash(0xB1),
                3,
                vec![a.hash.clone()],
            )
            .await;

            let dag_y = dag(&root, &[&p, &a, &b]);
            let (y_state, rejected_y, applied_y) = merge(
                &rm,
                &dag_y,
                &p.hash,
                &p.post_state,
                vec![
                    (a.hash.clone(), a.chains.clone()),
                    (b.hash.clone(), b.chains.clone()),
                ],
                None,
                &seeds,
            );
            assert!(rejected_y.is_empty(), "nothing in the merge conflicts");
            assert!(applied_y.contains(&d_sig) && applied_y.contains(&e_sig));

            execute(
                &rm,
                &y_state,
                deploy(insert(&k3, false), &vault(4)),
                5,
                &validator(2),
            )
            .await;
            Ok::<(), casper::rust::errors::CasperError>(())
        },
    )
    .await
    .unwrap()
    .unwrap();
}
