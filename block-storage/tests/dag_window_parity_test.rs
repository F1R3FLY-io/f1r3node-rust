use std::collections::HashMap;

use block_storage::rust::dag::block_dag_key_value_storage::{
    BlockDagKeyValueStorage, InsertMode, KeyValueDagRepresentation,
};
use models::rust::block_hash::BlockHash;
use models::rust::block_implicits::get_random_block;
use models::rust::casper::protocol::casper_message::{BlockMessage, Bond, Justification};
use models::rust::validator::Validator;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use tokio::runtime::Runtime;

const VALIDATORS: usize = 4;
const WINDOW: i64 = 3;

fn validator(i: usize) -> Validator { Bytes::from(vec![i as u8 + 1; 65]) }

fn shard_block(
    height: i64,
    sender: usize,
    seq: i32,
    parents: &[BlockHash],
    latest: &HashMap<Validator, BlockHash>,
) -> BlockMessage {
    let justifications = (0..VALIDATORS)
        .filter_map(|v| {
            latest.get(&validator(v)).map(|hash| Justification {
                validator: validator(v),
                latest_block_hash: hash.clone(),
            })
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

/// The same DAG in a windowed and an unbounded storage. The last validator
/// stops proposing after height 2, so its latest message falls below the
/// window.
struct Fixture {
    runtime: Runtime,
    windowed: BlockDagKeyValueStorage,
    reference: BlockDagKeyValueStorage,
    heights: Vec<Vec<BlockHash>>,
    silent_validator_latest: BlockHash,
}

impl Fixture {
    fn build(heights: usize) -> Self {
        let runtime = Runtime::new().unwrap();
        let (windowed, reference) = runtime.block_on(async {
            let mut windowed_kvm = InMemoryStoreManager::new();
            let mut reference_kvm = InMemoryStoreManager::new();
            (
                BlockDagKeyValueStorage::new_with_hot_window(&mut windowed_kvm, WINDOW)
                    .await
                    .unwrap(),
                BlockDagKeyValueStorage::new_with_hot_window(&mut reference_kvm, i64::MAX / 2)
                    .await
                    .unwrap(),
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
            let active = if height <= 2 {
                VALIDATORS
            } else {
                VALIDATORS - 1
            };
            for sender in 0..active {
                let block = shard_block(
                    height,
                    sender,
                    height as i32,
                    &layers[height as usize - 1],
                    &latest,
                );
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

    fn views(&self) -> (KeyValueDagRepresentation, KeyValueDagRepresentation) {
        (
            self.windowed.get_representation().unwrap(),
            self.reference.get_representation().unwrap(),
        )
    }
}

fn sorted<T: Ord>(mut items: Vec<T>) -> Vec<T> {
    items.sort();
    items
}

#[test]
fn every_accessor_agrees_with_an_unbounded_dag_below_the_window() {
    let fx = Fixture::build(12);
    fx.finalize(9);
    let (w, r) = fx.views();
    assert!(
        w.resident_counts().blocks < r.resident_counts().blocks,
        "the window evicted history"
    );

    let all: Vec<BlockHash> = fx.heights.iter().flatten().cloned().collect();
    for hash in &all {
        assert_eq!(w.lookup(hash).unwrap(), r.lookup(hash).unwrap());
        assert_eq!(w.contains(hash).unwrap(), r.contains(hash).unwrap());
        assert_eq!(w.block_number(hash).unwrap(), r.block_number(hash).unwrap());
        assert_eq!(w.main_parent(hash).unwrap(), r.main_parent(hash).unwrap());
        assert_eq!(
            w.self_justification(hash).unwrap(),
            r.self_justification(hash).unwrap()
        );
        assert_eq!(w.is_finalized(hash).unwrap(), r.is_finalized(hash).unwrap());
        assert_eq!(
            w.main_parent_chain(hash.clone(), 0).unwrap(),
            r.main_parent_chain(hash.clone(), 0).unwrap()
        );
        assert_eq!(
            w.children(hash)
                .unwrap()
                .map(|children| sorted(children.into_iter().collect())),
            r.children(hash)
                .unwrap()
                .map(|children| sorted(children.into_iter().collect()))
        );
        assert_eq!(
            sorted(w.descendants(hash).unwrap().into_iter().collect()),
            sorted(r.descendants(hash).unwrap().into_iter().collect())
        );
    }

    let genesis = &fx.heights[0][0];
    let tip = fx.heights[12][0].clone();
    for hash in &all {
        assert_eq!(
            w.is_dag_ancestor(hash, &tip).unwrap(),
            r.is_dag_ancestor(hash, &tip).unwrap()
        );
        assert_eq!(
            w.is_in_main_chain(genesis, hash).unwrap(),
            r.is_in_main_chain(genesis, hash).unwrap()
        );
    }

    let layers = |view: &KeyValueDagRepresentation| -> Vec<Vec<BlockHash>> {
        view.topo_sort(0, None)
            .unwrap()
            .into_iter()
            .map(sorted)
            .collect()
    };
    assert_eq!(layers(&w), layers(&r));
    assert_eq!(w.get_max_height(), r.get_max_height());
    assert_eq!(w.latest_block_number(), r.latest_block_number());
    assert_eq!(
        fx.windowed.genesis_hash().unwrap(),
        fx.reference.genesis_hash().unwrap()
    );

    let prefix = hex::encode(&genesis[..3]);
    assert_eq!(w.find(&prefix).unwrap(), r.find(&prefix).unwrap());

    let silent = validator(VALIDATORS - 1);
    assert_eq!(
        w.latest_message_hash(&silent),
        Some(fx.silent_validator_latest.clone())
    );
    assert_eq!(
        w.latest_message(&silent).unwrap(),
        r.latest_message(&silent).unwrap(),
        "a silent validator's latest message below the window still resolves"
    );
    assert_eq!(
        w.latest_messages().unwrap().len(),
        r.latest_messages().unwrap().len()
    );
    assert_eq!(
        w.non_finalized_blocks().unwrap(),
        r.non_finalized_blocks().unwrap()
    );
}
