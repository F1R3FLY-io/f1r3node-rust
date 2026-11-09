use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use block_storage::rust::dag::block_dag_key_value_storage::{BlockDagKeyValueStorage, InsertMode};
use models::rust::block_hash::BlockHash;
use models::rust::block_implicits::get_random_block;
use models::rust::casper::protocol::casper_message::{BlockMessage, Bond, Justification};
use models::rust::validator::Validator;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::shared::lmdb_dir_store_manager::{Db, LmdbDirStoreManager, LmdbEnvConfig};
use tokio::runtime::Runtime;

struct CountingAllocator;

static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            LIVE_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

const DAG_STORES: [&str; 13] = [
    "block-metadata",
    "equivocation-tracker",
    "latest-messages",
    "invalid-blocks",
    "floor-index",
    "frontier-index",
    "deploy-lifecycle-events",
    "deploy-lifecycle-terminal",
    "carrier-index",
    "carrier-index-meta",
    "genesis-hash",
    "height-index",
    "height-index-meta",
];

const VALIDATORS: usize = 10;
const BLOCKS_PER_HEIGHT: usize = 7;
const WARMUP_HEIGHTS: usize = 400;
const MEASURED_HEIGHTS: usize = 1600;
const RETENTION_DEPTH: i64 = 100;
const FINALIZE_EVERY_HEIGHTS: usize = 20;
const MAX_RETAINED_BYTES_PER_BLOCK: f64 = 256.0;

fn mapping() -> HashMap<Db, LmdbEnvConfig> {
    DAG_STORES
        .into_iter()
        .map(|name| {
            (Db::new(name.to_string(), None), LmdbEnvConfig {
                name: "dagstorage".to_string(),
                max_env_size: 2 * 1024 * 1024 * 1024,
                max_dbs: 32,
            })
        })
        .collect()
}

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

struct Shard {
    dag: BlockDagKeyValueStorage,
    runtime: Runtime,
    previous_height: Vec<BlockHash>,
    latest: HashMap<Validator, BlockHash>,
    seq: i32,
    height: i64,
    last_finalized_height: i64,
}

impl Shard {
    fn open(dir: PathBuf, retention_depth: Option<i64>) -> Self {
        let runtime = Runtime::new().unwrap();
        let dag = runtime.block_on(async {
            let mut manager = LmdbDirStoreManager::new(dir, mapping());
            BlockDagKeyValueStorage::new_with_memory_window(&mut manager, retention_depth)
                .await
                .unwrap()
        });
        let genesis = shard_block(0, 0, 0, &[], &HashMap::new());
        dag.insert(&genesis, InsertMode::Approved).unwrap();
        dag.record_genesis_hash(genesis.block_hash.clone()).unwrap();
        let mut latest = HashMap::new();
        for v in 0..VALIDATORS {
            latest.insert(validator(v), genesis.block_hash.clone());
        }
        Self {
            dag,
            runtime,
            previous_height: vec![genesis.block_hash],
            latest,
            seq: 1,
            height: 1,
            last_finalized_height: 0,
        }
    }

    fn advance_height(&mut self) {
        let mut produced = Vec::with_capacity(BLOCKS_PER_HEIGHT);
        for i in 0..BLOCKS_PER_HEIGHT {
            let sender = (self.height as usize + i) % VALIDATORS;
            let block = shard_block(
                self.height,
                sender,
                self.seq,
                &self.previous_height,
                &self.latest,
            );
            self.dag.insert(&block, InsertMode::Normal).unwrap();
            self.latest.insert(validator(sender), block.block_hash.clone());
            produced.push(block.block_hash);
        }
        self.seq += 1;
        self.previous_height = produced;
        self.height += 1;
    }

    fn finalize_behind(&mut self, lag: i64) {
        let target_height = self.height - 1 - lag;
        if target_height <= self.last_finalized_height {
            return;
        }
        let dag = self.dag.get_representation().unwrap();
        let target = dag
            .topo_sort(target_height, Some(target_height))
            .unwrap()
            .into_iter()
            .flatten()
            .next()
            .unwrap();
        self.runtime
            .block_on(self.dag.record_directly_finalized(target, 0.9, |_| async { Ok(()) }))
            .unwrap();
        self.last_finalized_height = target_height;
    }
}

fn live_bytes() -> f64 { LIVE_BYTES.load(Ordering::Relaxed) as f64 }

fn run(retention_depth: Option<i64>) -> (f64, usize, i64) {
    let temp = tempfile::Builder::new()
        .prefix("f1r3-dag-memory-bound-")
        .tempdir()
        .unwrap();
    let mut shard = Shard::open(temp.path().to_path_buf(), retention_depth);
    for _ in 0..WARMUP_HEIGHTS {
        shard.advance_height();
        if shard.height as usize % FINALIZE_EVERY_HEIGHTS == 0 {
            shard.finalize_behind(5);
        }
    }
    let start = live_bytes();
    for _ in 0..MEASURED_HEIGHTS {
        shard.advance_height();
        if shard.height as usize % FINALIZE_EVERY_HEIGHTS == 0 {
            shard.finalize_behind(5);
        }
    }
    let end = live_bytes();
    let measured_blocks = MEASURED_HEIGHTS * BLOCKS_PER_HEIGHT;
    let dag = shard.dag.get_representation().unwrap();
    let retained = dag.dag_set.len();
    let floor = shard.dag.memory_window_floor();
    ((end - start) / measured_blocks as f64, retained, floor)
}

#[test]
fn a_windowed_dag_retains_a_bounded_number_of_blocks_per_inserted_block() {
    let (bytes_per_block, retained, floor) = run(Some(RETENTION_DEPTH));
    let expected_max_retained =
        ((RETENTION_DEPTH as usize) + 5 + FINALIZE_EVERY_HEIGHTS + 2) * BLOCKS_PER_HEIGHT;
    eprintln!(
        "windowed: {bytes_per_block:.1} B/block live heap growth, {retained} blocks retained, floor {floor}"
    );
    assert!(floor > 0, "eviction never ran");
    assert!(
        retained <= expected_max_retained,
        "retained {retained} blocks, window allows at most {expected_max_retained}"
    );
    assert!(
        bytes_per_block < MAX_RETAINED_BYTES_PER_BLOCK,
        "live heap grew {bytes_per_block:.1} B per inserted block with the window enabled"
    );
}

#[test]
#[ignore = "baseline measurement for issue #933; run by hand to record the unbounded growth"]
fn an_unbounded_dag_grows_with_every_inserted_block() {
    let (bytes_per_block, retained, floor) = run(None);
    eprintln!(
        "unbounded: {bytes_per_block:.1} B/block live heap growth, {retained} blocks retained, floor {floor}"
    );
    assert!(bytes_per_block > MAX_RETAINED_BYTES_PER_BLOCK);
}
