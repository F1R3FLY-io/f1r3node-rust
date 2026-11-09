use std::collections::HashMap;

use block_storage::rust::dag::block_dag_key_value_storage::{
    BlockDagKeyValueStorage, InsertMode, KeyValueDagRepresentation,
};
use block_storage::rust::dag::block_metadata_store::UNBOUNDED_WINDOW_FLOOR;
use models::rust::block_hash::BlockHash;
use models::rust::block_implicits::get_random_block;
use models::rust::casper::protocol::casper_message::{BlockMessage, Bond, Justification};
use models::rust::validator::Validator;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use tokio::runtime::Runtime;

const VALIDATORS: usize = 4;
const RETENTION_DEPTH: i64 = 3;

fn validator(i: usize) -> Validator { Bytes::from(vec![i as u8 + 1; 65]) }

fn shard_block(
    height: i64,
    sender: usize,
    seq: i32,
    parents: &[BlockHash],
    latest: &HashMap<Validator, BlockHash>,
) -> BlockMessage {
    let justifications = (0..VALIDATORS)
        .map(|v| Justification {
            validator: validator(v),
            latest_block_hash: latest.get(&validator(v)).cloned().unwrap_or_default(),
        })
        .collect();
    let bonds = (0..VALIDATORS)
        .map(|v| Bond {
            validator: validator(v),
            stake: 100.into(),
        })
        .collect();
    get_random_block(
        Some(height),
        Some(seq),
        None,
        None,
        Some(validator(sender)),
        None,
        None,
        Some(parents.to_vec()),
        Some(justifications),
        Some(vec![]),
        None,
        Some(bonds),
        None,
        None,
    )
}

struct Fixture {
    runtime: Runtime,
    windowed: BlockDagKeyValueStorage,
    reference: BlockDagKeyValueStorage,
    heights: Vec<Vec<BlockHash>>,
    latest: HashMap<Validator, BlockHash>,
    silent_validator_latest: BlockHash,
}

impl Fixture {
    fn build(heights: usize) -> Self {
        let runtime = Runtime::new().unwrap();
        let (windowed, reference) = runtime.block_on(async {
            let mut kvm_a = InMemoryStoreManager::new();
            let mut kvm_b = InMemoryStoreManager::new();
            (
                BlockDagKeyValueStorage::new_with_memory_window(&mut kvm_a, Some(RETENTION_DEPTH))
                    .await
                    .unwrap(),
                BlockDagKeyValueStorage::new(&mut kvm_b).await.unwrap(),
            )
        });

        let genesis = shard_block(0, 0, 0, &[], &HashMap::new());
        for dag in [&windowed, &reference] {
            dag.insert(&genesis, InsertMode::Approved).unwrap();
            dag.record_genesis_hash(genesis.block_hash.clone()).unwrap();
        }
        let mut latest: HashMap<Validator, BlockHash> = (0..VALIDATORS)
            .map(|v| (validator(v), genesis.block_hash.clone()))
            .collect();
        let mut layers = vec![vec![genesis.block_hash.clone()]];
        let mut silent_validator_latest = genesis.block_hash.clone();

        for height in 1..=heights as i64 {
            let mut layer = Vec::new();
            let active: Vec<usize> = if height <= 2 {
                (0..VALIDATORS).collect()
            } else {
                (0..VALIDATORS - 1).collect()
            };
            for sender in active {
                let block = shard_block(height, sender, height as i32, &layers[height as usize - 1], &latest);
                for dag in [&windowed, &reference] {
                    dag.insert(&block, InsertMode::Normal).unwrap();
                }
                latest.insert(validator(sender), block.block_hash.clone());
                if sender == VALIDATORS - 1 {
                    silent_validator_latest = block.block_hash.clone();
                }
                layer.push(block.block_hash);
            }
            layers.push(layer);
        }

        Self {
            runtime,
            windowed,
            reference,
            heights: layers,
            latest,
            silent_validator_latest,
        }
    }

    fn finalize(&self, height: usize) {
        let target = self.heights[height][0].clone();
        for dag in [&self.windowed, &self.reference] {
            self.runtime
                .block_on(dag.record_directly_finalized(target.clone(), 0.9, |_| async { Ok(()) }))
                .unwrap();
        }
    }

    fn reps(&self) -> (KeyValueDagRepresentation, KeyValueDagRepresentation) {
        (
            self.windowed.get_representation().unwrap(),
            self.reference.get_representation().unwrap(),
        )
    }
}

#[test]
fn eviction_happens_on_finalization_and_the_floor_follows_the_lfb() {
    let fx = Fixture::build(12);
    assert_eq!(fx.windowed.memory_window_floor(), UNBOUNDED_WINDOW_FLOOR);

    fx.finalize(8);
    assert_eq!(fx.windowed.memory_window_floor(), 8 - RETENTION_DEPTH);
    assert_eq!(fx.reference.memory_window_floor(), UNBOUNDED_WINDOW_FLOOR);

    let (w, r) = fx.reps();
    assert!(w.dag_set.len() < r.dag_set.len());
    for hash in fx.heights[..5].iter().flatten() {
        assert!(!w.dag_set.contains(hash));
        assert!(w.contains(hash), "held history stays visible below the window");
    }
    for hash in fx.heights[5..].iter().flatten() {
        assert!(w.dag_set.contains(hash));
    }

    fx.finalize(10);
    assert_eq!(fx.windowed.memory_window_floor(), 10 - RETENTION_DEPTH);
}

#[test]
fn every_accessor_agrees_with_an_unbounded_dag_below_the_window() {
    let fx = Fixture::build(12);
    fx.finalize(9);
    let (w, r) = fx.reps();
    let floor = fx.windowed.memory_window_floor();
    assert!(floor > 1);

    let all: Vec<BlockHash> = fx.heights.iter().flatten().cloned().collect();
    for hash in &all {
        assert_eq!(w.lookup(hash).unwrap(), r.lookup(hash).unwrap());
        assert_eq!(w.contains(hash), r.contains(hash));
        assert_eq!(w.block_number(hash), r.block_number(hash));
        assert_eq!(w.main_parent(hash), r.main_parent(hash));
        assert_eq!(w.self_justification(hash).unwrap(), r.self_justification(hash).unwrap());
        assert_eq!(w.is_finalized(hash), r.is_finalized(hash));
        assert_eq!(
            w.main_parent_chain(hash.clone(), 0).unwrap(),
            r.main_parent_chain(hash.clone(), 0).unwrap()
        );
    }

    let genesis = &fx.heights[0][0];
    let tip = fx.heights[12][0].clone();
    for hash in &all {
        assert_eq!(w.is_dag_ancestor(hash, &tip).unwrap(), r.is_dag_ancestor(hash, &tip).unwrap());
        assert_eq!(w.is_in_main_chain(genesis, hash).unwrap(), r.is_in_main_chain(genesis, hash).unwrap());
    }

    assert_eq!(w.topo_sort(0, None).unwrap().len(), r.topo_sort(0, None).unwrap().len());
    for (a, b) in w.topo_sort(0, None).unwrap().iter().zip(r.topo_sort(0, None).unwrap()) {
        let mut a = a.clone();
        let mut b = b.clone();
        a.sort();
        b.sort();
        assert_eq!(a, b);
    }
    assert_eq!(w.get_max_height(), r.get_max_height());
    assert_eq!(w.latest_block_number(), r.latest_block_number());
    assert_eq!(
        w.blocks_in_height_range(None, 4).unwrap().len(),
        r.blocks_in_height_range(None, 4).unwrap().len()
    );
    assert_eq!(fx.windowed.genesis_hash().unwrap(), fx.reference.genesis_hash().unwrap());

    let prefix = hex::encode(&genesis[..3]);
    assert_eq!(w.find(&prefix).unwrap(), r.find(&prefix).unwrap());

    let silent = validator(VALIDATORS - 1);
    assert_eq!(w.latest_message_hash(&silent), Some(fx.silent_validator_latest.clone()));
    assert_eq!(
        w.latest_message(&silent).unwrap(),
        r.latest_message(&silent).unwrap(),
        "a silent validator's latest message below the window still resolves"
    );
    assert_eq!(w.latest_messages().unwrap().len(), r.latest_messages().unwrap().len());
    assert_eq!(
        w.non_finalized_blocks().unwrap(),
        r.non_finalized_blocks().unwrap()
    );
}

#[test]
fn a_late_block_citing_parents_below_the_window_is_admitted_and_held() {
    let fx = Fixture::build(12);
    fx.finalize(10);
    let floor = fx.windowed.memory_window_floor();
    let old_parent_height = (floor - 2) as usize;
    let late = shard_block(
        old_parent_height as i64 + 1,
        VALIDATORS - 1,
        50,
        &fx.heights[old_parent_height],
        &fx.latest,
    );

    let (w_before, _) = fx.reps();
    for parent in &late.header.parents_hash_list {
        assert!(!w_before.dag_set.contains(parent));
        assert!(w_before.lookup(parent).unwrap().is_some(), "an evicted parent is still held");
    }

    let w = fx.windowed.insert(&late, InsertMode::Normal).unwrap();
    let r = fx.reference.insert(&late, InsertMode::Normal).unwrap();
    assert!(w.contains(&late.block_hash));
    assert_eq!(w.lookup(&late.block_hash).unwrap(), r.lookup(&late.block_hash).unwrap());
    assert_eq!(w.block_number(&late.block_hash), Some(old_parent_height as i64 + 1));
    assert!(w.is_dag_ancestor(&fx.heights[0][0], &late.block_hash).unwrap());

    fx.finalize(11);
    let w = fx.windowed.get_representation().unwrap();
    assert!(!w.dag_set.contains(&late.block_hash), "the late block is evicted with its height");
    assert!(w.contains(&late.block_hash));
    assert_eq!(w.block_number(&late.block_hash), Some(old_parent_height as i64 + 1));
}

#[test]
fn reinserting_an_evicted_block_is_a_no_op() {
    let fx = Fixture::build(12);
    fx.finalize(10);
    let old = fx.heights[1][0].clone();
    let w = fx.windowed.get_representation().unwrap();
    assert!(!w.dag_set.contains(&old));
    let generation = fx.windowed.current_generation();

    let replay = shard_block(1, 0, 1, &fx.heights[0], &HashMap::new());
    let mut replay = replay;
    replay.block_hash = old.clone();
    let after = fx.windowed.insert(&replay, InsertMode::Normal).unwrap();
    assert_eq!(fx.windowed.current_generation(), generation);
    assert!(!after.dag_set.contains(&old));
    assert!(after.contains(&old));
}
