use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::{BlockDagKeyValueStorage, InsertMode};
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use casper::rust::finality::floor::{finalized_floor, floor_of_block};
use casper::rust::metrics_constants::CLIQUE_ORACLE_COMPUTE_TIME_METRIC;
use casper::rust::safety::clique_oracle::FtThreshold;
use crypto::rust::public_key::PublicKey;
use metrics_util::debugging::{DebugValue, DebuggingRecorder};
use models::rust::block_hash::BlockHash;
use models::rust::block_implicits;
use models::rust::casper::protocol::casper_message::{BlockMessage, Bond, Justification};
use models::rust::validator::Validator;
use rspace_plus_plus::rspace::history::Either;
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

use crate::helper::block_util::generate_validator;
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

const ROUNDS: i64 = 4;

fn block(
    number: i64,
    sender: &Validator,
    parents: Vec<BlockHash>,
    justifications: Vec<Justification>,
    bonds: &[Bond],
) -> BlockMessage {
    block_implicits::get_random_block(
        Some(number),
        Some(number as i32),
        None,
        None,
        Some(sender.clone()),
        None,
        Some(number),
        Some(parents),
        Some(justifications),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(bonds.to_vec()),
        Some("root".to_string()),
        None,
    )
}

fn empty_block_store() -> KeyValueBlockStore {
    KeyValueBlockStore::new(
        Arc::new(InMemoryKeyValueStore::new()),
        Arc::new(InMemoryKeyValueStore::new()),
    )
}

/// Clique computations spent by ONE floor derivation over the last round of
/// a unanimous committee. Every validator builds on validator 0's spine and
/// justifies the whole previous round, so the last round's blocks are
/// `validator_count` parents whose main-parent spines coincide below the top.
async fn clique_computations_for_one_derivation(validator_count: usize) -> usize {
    let mut kvm = InMemoryStoreManager::new();
    let dag_storage = BlockDagKeyValueStorage::new(&mut kvm)
        .await
        .expect("dag storage");
    let block_store = empty_block_store();

    let validators: Vec<Validator> = (0..validator_count)
        .map(|i| generate_validator(Some(&format!("floor-derivation-reuse {}", i))))
        .collect();
    let bonds: Vec<Bond> = validators
        .iter()
        .map(|validator| Bond {
            validator: validator.clone(),
            stake: 10.into(),
        })
        .collect();

    let genesis = block(0, &validators[0], Vec::new(), Vec::new(), &bonds);
    dag_storage
        .insert(&genesis, InsertMode::Approved)
        .expect("insert genesis");

    let mut latest: Vec<BlockHash> = vec![genesis.block_hash.clone(); validator_count];
    for height in 1..=ROUNDS {
        let justifications: Vec<Justification> = validators
            .iter()
            .zip(latest.iter())
            .map(|(validator, hash)| Justification {
                validator: validator.clone(),
                latest_block_hash: hash.clone(),
            })
            .collect();
        let round: Vec<BlockMessage> = validators
            .iter()
            .map(|validator| {
                block(
                    height,
                    validator,
                    vec![latest[0].clone()],
                    justifications.clone(),
                    &bonds,
                )
            })
            .collect();
        for b in &round {
            dag_storage
                .insert(b, InsertMode::Normal)
                .expect("insert block");
        }
        latest = round.iter().map(|b| b.block_hash.clone()).collect();
    }

    let dag = dag_storage
        .get_representation()
        .expect("dag representation");
    let ftt = FtThreshold::from_f32_lossy(0.1);
    for parent in &latest {
        floor_of_block(&dag, &block_store, parent, ftt)
            .await
            .expect("parent floor");
    }
    let snapshot: BTreeMap<Validator, BlockHash> = validators
        .iter()
        .cloned()
        .zip(latest.iter().cloned())
        .collect();

    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let floor = {
        let _guard = metrics::set_default_local_recorder(&recorder);
        finalized_floor(&dag, &block_store, &latest, &snapshot, ftt)
            .await
            .expect("floor derivation")
    };
    assert!(
        floor.block_number > 0,
        "a unanimous committee must finalize above genesis"
    );

    snapshotter
        .snapshot()
        .into_vec()
        .into_iter()
        .filter(|(key, _, _, _)| key.key().name() == CLIQUE_ORACLE_COMPUTE_TIME_METRIC)
        .map(|(_, _, _, value)| match value {
            DebugValue::Histogram(samples) => samples.len(),
            _ => 0,
        })
        .sum()
}

#[tokio::test]
async fn one_floor_derivation_evaluates_each_shared_spine_block_once() {
    let small = clique_computations_for_one_derivation(4).await;
    let large = clique_computations_for_one_derivation(16).await;
    assert!(small > 0, "the derivation must consult the clique oracle");
    assert!(
        large <= small,
        "parents sharing one spine must share its oracle verdicts: \
         4 parents -> {small} clique computations, 16 parents -> {large}"
    );
}

fn equal_bonds(pks: Vec<PublicKey>) -> HashMap<PublicKey, num_bigint::BigInt> {
    pks.into_iter().map(|pk| (pk, 100.into())).collect()
}

#[tokio::test]
async fn a_validated_block_keeps_the_floor_its_validation_derived() {
    let genesis_parameters =
        GenesisBuilder::build_genesis_parameters_with_defaults(Some(equal_bonds), Some(3));
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(genesis_parameters))
        .await
        .expect("genesis");
    let mut nodes = TestNode::create_network(genesis, 2, None, None, None, None)
        .await
        .expect("create_network");
    for node in nodes.iter_mut() {
        node.allow_empty_blocks = true;
    }

    let block = nodes[0].create_block_unsafe(&[]).await.expect("block");
    let status = nodes[1]
        .process_block(block.clone())
        .await
        .expect("process");
    assert!(
        matches!(status, Either::Right(_)),
        "the block must validate: {status:?}"
    );

    let dag = nodes[1]
        .block_dag_storage
        .get_representation()
        .expect("dag representation");
    let cached_floor = dag
        .get_cached_floor(&block.block_hash)
        .expect("floor cache read");
    let cached_frontier = dag
        .get_cached_frontier(&block.block_hash)
        .expect("frontier cache read");
    assert!(
        cached_floor.is_some() && cached_frontier.is_some(),
        "validation derived this block's floor; it must not be derived again"
    );

    let latest_messages: BTreeMap<Validator, BlockHash> = block
        .justifications
        .iter()
        .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
        .collect();
    let derived = finalized_floor(
        &dag,
        &nodes[1].block_store,
        &block.header.parents_hash_list,
        &latest_messages,
        FtThreshold::from_f32_lossy(0.1),
    )
    .await
    .expect("independent derivation");
    assert_eq!(
        cached_floor,
        Some(derived.hash),
        "the cached floor must equal an independent derivation"
    );
}
