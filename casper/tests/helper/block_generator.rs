// See casper/src/test/scala/coop/rchain/casper/helper/BlockGenerator.scala
#![allow(clippy::too_many_arguments)]

use std::collections::{BTreeMap, HashMap};
use std::time::{SystemTime, UNIX_EPOCH};

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use block_storage::rust::test::indexed_block_dag_storage::IndexedBlockDagStorage;
use casper::rust::casper::CasperSnapshot;
use casper::rust::causal_equivocation::CertifiedConsensusContext;
use casper::rust::errors::CasperError;
use casper::rust::estimator::{Estimator, ForkChoice};
use casper::rust::util::rholang::interpreter_util::compute_deploys_checkpoint_cosigned;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use casper::rust::util::{construct_deploy, proto_util};
use models::rust::block::state_hash::StateHash;
use models::rust::block_hash::BlockHash;
use models::rust::block_implicits;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Bond, FinalizationCertificate, Justification, ProcessedDeploy,
    ProcessedSystemDeploy, RejectedDeploy,
};
use models::rust::validator::Validator;
use rholang::rust::interpreter::system_processes::BlockData;
use shared::rust::store::key_value_store::KvStoreError;

fn default_state_hash() -> StateHash { vec![0; models::rust::block_hash::LENGTH].into() }

fn default_validator() -> Validator { vec![2; models::rust::validator::LENGTH].into() }

fn generated_genesis_certificate(
    genesis: &BlockMessage,
    dag: &KeyValueDagRepresentation,
) -> FinalizationCertificate {
    if dag.get_cached_floor(&genesis.block_hash).unwrap().is_none() {
        dag.put_cached_floor(genesis.block_hash.clone(), genesis.block_hash.clone())
            .unwrap();
    }
    if dag
        .get_cached_frontier(&genesis.block_hash)
        .unwrap()
        .is_none()
    {
        dag.put_cached_frontier(genesis.block_hash.clone(), genesis.block_hash.clone())
            .unwrap();
    }
    casper::rust::finality::certificate::genesis_finalization_certificate(
        dag,
        genesis,
        genesis.header.version,
        genesis.shard_id.clone(),
        0,
        1_000_000,
    )
    .unwrap()
}

fn generated_candidate_context_digest(
    block: &BlockMessage,
    genesis: &BlockMessage,
    dag: &KeyValueDagRepresentation,
) -> BlockHash {
    let authority = dag.lookup_unsafe(&genesis.block_hash).unwrap();
    let justifications = block
        .justifications
        .iter()
        .map(|justification| {
            (
                justification.validator.clone(),
                justification.latest_block_hash.clone(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let exact_latest_messages = authority
        .active_validator_set
        .iter()
        .map(|validator| {
            let latest = justifications
                .get(validator)
                .cloned()
                .or_else(|| {
                    dag.latest_message(validator)
                        .unwrap()
                        .map(|metadata| metadata.block_hash)
                })
                .unwrap_or_else(|| genesis.block_hash.clone());
            (validator.clone(), latest)
        })
        .collect::<BTreeMap<_, _>>();
    CertifiedConsensusContext::for_parents(
        dag,
        &block.header.parents_hash_list,
        &exact_latest_messages,
    )
    .unwrap()
    .digest()
    .clone()
}

#[derive(Clone, Default)]
pub struct MergeFacts {
    pub merge_base: Option<BlockHash>,
    pub applied_from_scope: Vec<prost::bytes::Bytes>,
    pub rejected_deploys: Vec<RejectedDeploy>,
}

pub async fn certified_fork_choice(
    estimator: &Estimator,
    dag: &KeyValueDagRepresentation,
    authority_floor: &BlockMessage,
    latest_messages: HashMap<Validator, BlockHash>,
) -> Result<ForkChoice, KvStoreError> {
    let context = certified_consensus_context(dag, authority_floor, latest_messages)?;
    estimator
        .tips_with_context(dag, authority_floor, &context)
        .await
}

pub fn certified_consensus_context(
    dag: &KeyValueDagRepresentation,
    authority_floor: &BlockMessage,
    latest_messages: HashMap<Validator, BlockHash>,
) -> Result<CertifiedConsensusContext, KvStoreError> {
    let authority = dag.lookup_unsafe(&authority_floor.block_hash)?;
    let exact_latest_messages = authority
        .active_validator_set
        .iter()
        .map(|validator| {
            (
                validator.clone(),
                latest_messages
                    .get(validator)
                    .cloned()
                    .unwrap_or_else(|| authority_floor.block_hash.clone()),
            )
        })
        .collect::<BTreeMap<_, _>>();
    CertifiedConsensusContext::for_frozen_floor(
        dag,
        authority_floor.block_hash.clone(),
        &exact_latest_messages,
    )
    .map_err(|error| KvStoreError::InvalidArgument(error.to_string()))
}

pub fn mk_casper_snapshot(dag: KeyValueDagRepresentation) -> CasperSnapshot {
    CasperSnapshot::new(dag)
}

pub async fn step(
    block_dag_storage: &mut IndexedBlockDagStorage,
    block_store: &mut KeyValueBlockStore,
    runtime_manager: &mut RuntimeManager,
    block: &BlockMessage,
) -> Result<(), CasperError> {
    let dag = block_dag_storage
        .get_representation()
        .expect("dag representation");
    let (pre_state_hash, post_state_hash, processed_deploys, processed_system_deploys) =
        compute_block_checkpoint(
            block_store,
            block,
            &mk_casper_snapshot(dag),
            runtime_manager,
        )
        .await?;

    inject_state_hashes(
        block_store,
        block_dag_storage,
        block,
        pre_state_hash,
        post_state_hash,
        processed_deploys,
        processed_system_deploys,
    )
}

async fn compute_block_checkpoint(
    block_store: &mut KeyValueBlockStore,
    block: &BlockMessage,
    casper_snapshot: &CasperSnapshot,
    runtime_manager: &mut RuntimeManager,
) -> Result<
    (
        StateHash,
        StateHash,
        Vec<ProcessedDeploy>,
        Vec<ProcessedSystemDeploy>,
    ),
    CasperError,
> {
    let parents = proto_util::get_parents(block_store, block);
    let deploys = proto_util::deploys(block)
        .into_iter()
        .map(|deploy| deploy.to_cosigned().map_err(CasperError::RuntimeError))
        .collect::<Result<Vec<_>, _>>()?;

    let (pre_state_hash, post_state_hash, processed_deploys, _, processed_system_deploys, _) =
        compute_deploys_checkpoint_cosigned(
            block_store,
            parents,
            deploys,
            Vec::new(), // No system deploys
            casper_snapshot,
            runtime_manager,
            BlockData::from_block(block),
            HashMap::new(),
            None,
        )
        .await?;

    Ok((
        pre_state_hash,
        post_state_hash,
        processed_deploys,
        processed_system_deploys,
    ))
}

fn inject_state_hashes(
    block_store: &mut KeyValueBlockStore,
    block_dag_storage: &mut IndexedBlockDagStorage,
    block: &BlockMessage,
    pre_state_hash: StateHash,
    post_state_hash: StateHash,
    processed_deploys: Vec<ProcessedDeploy>,
    processed_system_deploys: Vec<ProcessedSystemDeploy>,
) -> Result<(), CasperError> {
    let mut updated_block = block.clone();
    updated_block.body.state.pre_state_hash = pre_state_hash;
    updated_block.body.state.post_state_hash = post_state_hash;
    updated_block.body.deploys = processed_deploys;
    updated_block.body.system_deploys = processed_system_deploys;
    block_store.put(block.block_hash.clone(), &updated_block)?;
    block_dag_storage.insert(
        &updated_block,
        block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
    )?;
    Ok(())
}

pub fn build_block(
    parents_hash_list: Vec<BlockHash>,
    creator: Option<Validator>,
    now: i64,
    bonds: Option<Vec<Bond>>,
    justifications: Option<Vec<Justification>>,
    deploys: Option<Vec<ProcessedDeploy>>,
    post_state_hash: Option<StateHash>,
    shard_id: Option<String>,
    pre_state_hash: Option<StateHash>,
    seq_num: Option<i32>,
) -> BlockMessage {
    build_block_with_system_deploys(
        parents_hash_list,
        creator,
        now,
        bonds,
        justifications,
        deploys,
        post_state_hash,
        shard_id,
        pre_state_hash,
        seq_num,
        None,
    )
}

pub fn build_block_at_height(
    block_number: i64,
    parents_hash_list: Vec<BlockHash>,
    creator: Option<Validator>,
    now: i64,
    bonds: Option<Vec<Bond>>,
    justifications: Option<Vec<Justification>>,
    deploys: Option<Vec<ProcessedDeploy>>,
    post_state_hash: Option<StateHash>,
    shard_id: Option<String>,
    pre_state_hash: Option<StateHash>,
    seq_num: Option<i32>,
) -> BlockMessage {
    build_block_with_system_deploys_at_height(
        Some(block_number),
        parents_hash_list,
        creator,
        now,
        bonds,
        justifications,
        deploys,
        post_state_hash,
        shard_id,
        pre_state_hash,
        seq_num,
        None,
    )
}

pub fn build_block_with_system_deploys(
    parents_hash_list: Vec<BlockHash>,
    creator: Option<Validator>,
    now: i64,
    bonds: Option<Vec<Bond>>,
    justifications: Option<Vec<Justification>>,
    deploys: Option<Vec<ProcessedDeploy>>,
    post_state_hash: Option<StateHash>,
    shard_id: Option<String>,
    pre_state_hash: Option<StateHash>,
    seq_num: Option<i32>,
    system_deploys: Option<Vec<ProcessedSystemDeploy>>,
) -> BlockMessage {
    build_block_with_system_deploys_at_height(
        None,
        parents_hash_list,
        creator,
        now,
        bonds,
        justifications,
        deploys,
        post_state_hash,
        shard_id,
        pre_state_hash,
        seq_num,
        system_deploys,
    )
}

fn build_block_with_system_deploys_at_height(
    block_number: Option<i64>,
    parents_hash_list: Vec<BlockHash>,
    creator: Option<Validator>,
    now: i64,
    bonds: Option<Vec<Bond>>,
    justifications: Option<Vec<Justification>>,
    deploys: Option<Vec<ProcessedDeploy>>,
    post_state_hash: Option<StateHash>,
    shard_id: Option<String>,
    pre_state_hash: Option<StateHash>,
    seq_num: Option<i32>,
    system_deploys: Option<Vec<ProcessedSystemDeploy>>,
) -> BlockMessage {
    let creator = creator.unwrap_or_else(default_validator);
    let bonds = bonds.unwrap_or_default();
    let justifications = justifications.unwrap_or_default();
    let deploys = deploys.unwrap_or_default();
    let post_state_hash = post_state_hash.unwrap_or_else(default_state_hash);
    let shard_id = shard_id.unwrap_or("root".to_string());
    let pre_state_hash = pre_state_hash.unwrap_or_else(default_state_hash);
    let seq_num = seq_num.unwrap_or(0);
    let system_deploys = system_deploys.unwrap_or_default();

    block_implicits::get_random_block(
        block_number,
        Some(seq_num),
        Some(pre_state_hash),
        Some(post_state_hash),
        Some(creator),
        Some(casper::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION),
        Some(now),
        Some(parents_hash_list),
        Some(justifications),
        Some(deploys),
        Some(system_deploys),
        Some(bonds),
        Some(shard_id),
        None,
    )
}

pub fn create_genesis_block(
    block_store: &mut KeyValueBlockStore,
    indexed_block_dag_storage: &mut IndexedBlockDagStorage,
    creator: Option<Validator>,
    bonds: Option<Vec<Bond>>,
    justifications: Option<Vec<Justification>>,
    deploys: Option<Vec<ProcessedDeploy>>,
    ts_hash: Option<StateHash>,
    shard_id: Option<String>,
    pre_state_hash: Option<StateHash>,
    seq_num: Option<i32>,
) -> BlockMessage {
    let creator = creator.unwrap_or_else(default_validator);
    let bonds = bonds.filter(|bonds| !bonds.is_empty()).unwrap_or_else(|| {
        vec![Bond {
            validator: creator.clone(),
            stake: 1,
        }]
    });
    let justifications = justifications.unwrap_or_default();
    let deploys = deploys.unwrap_or_default();
    let ts_hash = ts_hash.unwrap_or_else(default_state_hash);
    let shard_id = shard_id.unwrap_or("root".to_string());
    let pre_state_hash = pre_state_hash.unwrap_or_else(default_state_hash);
    let seq_num = seq_num.unwrap_or(0);

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    let genesis = build_block(
        vec![],
        Some(creator),
        now,
        Some(bonds),
        Some(justifications),
        Some(deploys),
        Some(ts_hash),
        Some(shard_id),
        Some(pre_state_hash),
        Some(seq_num),
    );

    let modified_block = indexed_block_dag_storage
        .insert_indexed(&genesis, &genesis, false)
        .unwrap();

    let dag = indexed_block_dag_storage.get_representation().unwrap();
    dag.put_cached_floor(genesis.block_hash.clone(), genesis.block_hash.clone())
        .unwrap();
    dag.put_cached_frontier(genesis.block_hash.clone(), genesis.block_hash.clone())
        .unwrap();

    block_store
        .put(genesis.block_hash.clone(), &modified_block)
        .unwrap();

    genesis
}

pub fn create_block(
    block_store: &mut KeyValueBlockStore,
    indexed_block_dag_storage: &mut IndexedBlockDagStorage,
    parents_hash_list: Vec<BlockHash>,
    genesis: &BlockMessage,
    creator: Option<Validator>,
    bonds: Option<Vec<Bond>>,
    justifications: Option<std::collections::HashMap<Validator, BlockHash>>,
    deploys: Option<Vec<ProcessedDeploy>>,
    post_state_hash: Option<StateHash>,
    shard_id: Option<String>,
    pre_state_hash: Option<StateHash>,
    seq_num: Option<i32>,
    invalid: Option<bool>,
) -> BlockMessage {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    create_block_with_merge_facts_and_system_deploys_at(
        block_store,
        indexed_block_dag_storage,
        parents_hash_list,
        genesis,
        creator,
        bonds,
        justifications,
        deploys,
        post_state_hash,
        shard_id,
        pre_state_hash,
        seq_num,
        invalid,
        None,
        None,
        None,
        now,
    )
}

pub fn create_block_with_merge_facts(
    block_store: &mut KeyValueBlockStore,
    indexed_block_dag_storage: &mut IndexedBlockDagStorage,
    parents_hash_list: Vec<BlockHash>,
    genesis: &BlockMessage,
    creator: Option<Validator>,
    bonds: Option<Vec<Bond>>,
    justifications: Option<std::collections::HashMap<Validator, BlockHash>>,
    deploys: Option<Vec<ProcessedDeploy>>,
    post_state_hash: Option<StateHash>,
    shard_id: Option<String>,
    pre_state_hash: Option<StateHash>,
    seq_num: Option<i32>,
    invalid: Option<bool>,
    merge_facts: MergeFacts,
) -> BlockMessage {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    create_block_with_merge_facts_and_system_deploys_at(
        block_store,
        indexed_block_dag_storage,
        parents_hash_list,
        genesis,
        creator,
        bonds,
        justifications,
        deploys,
        post_state_hash,
        shard_id,
        pre_state_hash,
        seq_num,
        invalid,
        Some(merge_facts),
        None,
        None,
        now,
    )
}

pub fn create_block_with_finalized_floor_certificate(
    block_store: &mut KeyValueBlockStore,
    indexed_block_dag_storage: &mut IndexedBlockDagStorage,
    parents_hash_list: Vec<BlockHash>,
    genesis: &BlockMessage,
    creator: Validator,
    bonds: Vec<Bond>,
    justifications: std::collections::HashMap<Validator, BlockHash>,
    certificate: FinalizationCertificate,
) -> BlockMessage {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    create_block_with_merge_facts_and_system_deploys_at(
        block_store,
        indexed_block_dag_storage,
        parents_hash_list,
        genesis,
        Some(creator),
        Some(bonds),
        Some(justifications),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(certificate),
        now,
    )
}

pub fn create_block_with_system_deploys_at(
    block_store: &mut KeyValueBlockStore,
    indexed_block_dag_storage: &mut IndexedBlockDagStorage,
    parents_hash_list: Vec<BlockHash>,
    genesis: &BlockMessage,
    creator: Option<Validator>,
    bonds: Option<Vec<Bond>>,
    justifications: Option<std::collections::HashMap<Validator, BlockHash>>,
    deploys: Option<Vec<ProcessedDeploy>>,
    post_state_hash: Option<StateHash>,
    shard_id: Option<String>,
    pre_state_hash: Option<StateHash>,
    seq_num: Option<i32>,
    invalid: Option<bool>,
    system_deploys: Option<Vec<ProcessedSystemDeploy>>,
    time_stamp: i64,
) -> BlockMessage {
    create_block_with_merge_facts_and_system_deploys_at(
        block_store,
        indexed_block_dag_storage,
        parents_hash_list,
        genesis,
        creator,
        bonds,
        justifications,
        deploys,
        post_state_hash,
        shard_id,
        pre_state_hash,
        seq_num,
        invalid,
        None,
        system_deploys,
        None,
        time_stamp,
    )
}

fn create_block_with_merge_facts_and_system_deploys_at(
    block_store: &mut KeyValueBlockStore,
    indexed_block_dag_storage: &mut IndexedBlockDagStorage,
    parents_hash_list: Vec<BlockHash>,
    genesis: &BlockMessage,
    creator: Option<Validator>,
    bonds: Option<Vec<Bond>>,
    justifications: Option<std::collections::HashMap<Validator, BlockHash>>,
    deploys: Option<Vec<ProcessedDeploy>>,
    post_state_hash: Option<StateHash>,
    shard_id: Option<String>,
    pre_state_hash: Option<StateHash>,
    seq_num: Option<i32>,
    invalid: Option<bool>,
    merge_facts: Option<MergeFacts>,
    system_deploys: Option<Vec<ProcessedSystemDeploy>>,
    finalized_floor_certificate: Option<FinalizationCertificate>,
    time_stamp: i64,
) -> BlockMessage {
    let creator = creator.unwrap_or_else(default_validator);
    let bonds = bonds
        .filter(|bonds| !bonds.is_empty())
        .unwrap_or_else(|| genesis.body.state.bonds.clone());
    let justifications = justifications
        .unwrap_or_default()
        .into_iter()
        .map(|(validator, block_hash)| Justification {
            validator,
            latest_block_hash: block_hash,
        })
        .collect();
    let deploys = deploys.unwrap_or_default();
    let post_state_hash = post_state_hash.unwrap_or_else(default_state_hash);
    let shard_id = shard_id.unwrap_or("root".to_string());
    let pre_state_hash = pre_state_hash.unwrap_or_else(default_state_hash);
    let seq_num = seq_num.unwrap_or(0);
    let invalid = invalid.unwrap_or(false);

    let mut block = build_block_with_system_deploys(
        parents_hash_list,
        Some(creator),
        time_stamp,
        Some(bonds),
        Some(justifications),
        Some(deploys),
        Some(post_state_hash),
        Some(shard_id),
        Some(pre_state_hash),
        Some(seq_num),
        system_deploys,
    );

    let merge_facts = merge_facts.unwrap_or_default();
    block.body.merge_base = merge_facts.merge_base.unwrap_or_default();
    block.body.applied_from_scope = merge_facts.applied_from_scope;
    block.body.rejected_deploys = merge_facts.rejected_deploys;

    if block.header.version >= casper::rust::casper::CERTIFIED_FINALIZED_FLOOR_PROTOCOL_VERSION {
        let dag = indexed_block_dag_storage.get_representation().unwrap();
        let certificate = finalized_floor_certificate
            .unwrap_or_else(|| generated_genesis_certificate(genesis, &dag));
        let context_digest = generated_candidate_context_digest(&block, genesis, &dag);
        block.header.finalized_floor = Some(certificate.commitment(context_digest));
        block.finalized_floor_certificate = Some(certificate);
    }

    let modified_block = indexed_block_dag_storage
        .insert_indexed(&block, genesis, invalid)
        .unwrap();

    let dag = indexed_block_dag_storage.get_representation().unwrap();
    dag.put_cached_floor(
        modified_block.block_hash.clone(),
        genesis.block_hash.clone(),
    )
    .unwrap();
    dag.put_cached_frontier(
        modified_block.block_hash.clone(),
        genesis.block_hash.clone(),
    )
    .unwrap();

    block_store
        .put(block.block_hash.clone(), &modified_block)
        .unwrap();

    modified_block
}

pub fn create_block_fast(
    block_store: &mut KeyValueBlockStore,
    block_dag_storage: &mut IndexedBlockDagStorage,
    parents: Vec<BlockHash>,
    genesis: &BlockMessage,
) -> BlockMessage {
    create_block(
        block_store,
        block_dag_storage,
        parents,
        genesis,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
}

pub fn create_block_fast_with_creator(
    block_store: &mut KeyValueBlockStore,
    block_dag_storage: &mut IndexedBlockDagStorage,
    parents: Vec<BlockHash>,
    genesis: &BlockMessage,
    creator: Validator,
) -> BlockMessage {
    create_block(
        block_store,
        block_dag_storage,
        parents,
        genesis,
        Some(creator),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
}

pub fn create_validator_block(
    block_store: &mut KeyValueBlockStore,
    indexed_block_dag_storage: &mut IndexedBlockDagStorage,
    parents: Vec<BlockMessage>,
    genesis: &BlockMessage,
    justifications: Vec<BlockMessage>,
    validator: Validator,
    bonds: Vec<Bond>,
    seq_num: Option<i32>,
    invalid: Option<bool>,
    shard_id: String,
) -> BlockMessage {
    let deploy = construct_deploy::basic_processed_deploy(0, Some(shard_id.clone())).unwrap();

    let justifications_map: std::collections::HashMap<Validator, BlockHash> = justifications
        .into_iter()
        .map(|b| (b.sender, b.block_hash))
        .collect();

    create_block(
        block_store,
        indexed_block_dag_storage,
        parents.into_iter().map(|p| p.block_hash).collect(),
        genesis,
        Some(validator),
        Some(bonds),
        Some(justifications_map),
        Some(vec![deploy]),
        None,
        Some(shard_id),
        None,
        seq_num,
        invalid,
    )
}
