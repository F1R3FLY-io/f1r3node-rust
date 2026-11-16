use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use block_storage::rust::dag::block_dag_key_value_storage::{BlockDagKeyValueStorage, InsertMode};
use models::rust::block_hash::BlockHash;
use models::rust::block_implicits::get_random_block;
use models::rust::casper::protocol::casper_message::{BlockMessage, Bond, Justification};
use models::rust::validator::Validator;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::shared::lmdb_dir_store_manager::{
    Db, LmdbDirStoreManager, LmdbEnvConfig,
};
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

/// The allocator counter is process-wide, so measurements must not overlap.
static MEASURING: Mutex<()> = Mutex::new(());

const DAG_STORES: [&str; 17] = [
    "block-metadata",
    "equivocation-tracker",
    "latest-messages",
    "dag-equivocations-v1",
    "floor-index",
    "frontier-index",
    "deploy-lifecycle-events",
    "deploy-lifecycle-terminal",
    "carrier-index",
    "carrier-index-meta",
    "genesis-hash",
    "dag-membership-v1",
    "dag-height-v1",
    "dag-children-v1",
    "dag-history-state-v1",
    "dag-gc-retry-v1",
    "dag-sequence-v1",
];

const VALIDATORS: usize = 10;
const BLOCKS_PER_HEIGHT: usize = 7;
const WARMUP_HEIGHTS: usize = 400;
const MEASURED_HEIGHTS: usize = 1600;
const WINDOW: i64 = 100;
const FINALIZE_EVERY_HEIGHTS: usize = 20;
const FINALITY_LAG: i64 = 5;
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

/// A shard-shaped DAG: every height has `BLOCKS_PER_HEIGHT` blocks citing the
/// whole previous height, finalized `FINALITY_LAG` heights behind the tip.
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
    fn open(dir: PathBuf, window: i64) -> Self {
        let runtime = Runtime::new().unwrap();
        let dag = runtime.block_on(async {
            let mut manager = LmdbDirStoreManager::new(dir, mapping());
            BlockDagKeyValueStorage::new_with_hot_window(&mut manager, window)
                .await
                .unwrap()
        });
        let genesis = shard_block(0, 0, 0, &[], &HashMap::new());
        dag.insert(&genesis, InsertMode::Approved).unwrap();
        let latest = (0..VALIDATORS)
            .map(|v| (validator(v), genesis.block_hash.clone()))
            .collect();
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

    fn advance(&mut self) {
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
            self.latest
                .insert(validator(sender), block.block_hash.clone());
            produced.push(block.block_hash);
        }
        self.seq += 1;
        self.previous_height = produced;
        self.height += 1;
        if (self.height as usize).is_multiple_of(FINALIZE_EVERY_HEIGHTS) {
            self.finalize_behind();
        }
    }

    fn finalize_behind(&mut self) {
        let target_height = self.height - 1 - FINALITY_LAG;
        if target_height <= self.last_finalized_height {
            return;
        }
        let target = self
            .dag
            .get_representation()
            .unwrap()
            .topo_sort(target_height, Some(target_height))
            .unwrap()
            .into_iter()
            .flatten()
            .next()
            .unwrap();
        self.runtime
            .block_on(
                self.dag
                    .record_directly_finalized(target, 0.9, |_| async { Ok(()) }),
            )
            .unwrap();
        self.last_finalized_height = target_height;
    }
}

/// Live-heap growth per inserted block over the measured heights, and the
/// number of blocks resident at the end.
fn measure(window: i64) -> (f64, usize) {
    let _measuring = MEASURING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let temp = tempfile::tempdir().unwrap();
    let mut shard = Shard::open(temp.path().to_path_buf(), window);
    for _ in 0..WARMUP_HEIGHTS {
        shard.advance();
    }
    let start = LIVE_BYTES.load(Ordering::Relaxed) as f64;
    for _ in 0..MEASURED_HEIGHTS {
        shard.advance();
    }
    let resident = shard
        .dag
        .get_representation()
        .unwrap()
        .resident_counts()
        .blocks;
    let end = LIVE_BYTES.load(Ordering::Relaxed) as f64;
    (
        (end - start) / (MEASURED_HEIGHTS * BLOCKS_PER_HEIGHT) as f64,
        resident,
    )
}

#[test]
fn a_windowed_dag_retains_a_bounded_number_of_blocks() {
    let (bytes_per_block, resident) = measure(WINDOW);
    let window_blocks =
        (WINDOW as usize + FINALITY_LAG as usize + FINALIZE_EVERY_HEIGHTS + 2) * BLOCKS_PER_HEIGHT;
    assert!(
        resident <= window_blocks,
        "{resident} blocks resident; the window allows at most {window_blocks}"
    );
    assert!(
        bytes_per_block < MAX_RETAINED_BYTES_PER_BLOCK,
        "live heap grew {bytes_per_block:.1} B per inserted block with the window on"
    );
}

#[test]
#[ignore = "baseline: an unbounded window keeps every block resident"]
fn an_unbounded_window_grows_with_every_inserted_block() {
    let (bytes_per_block, resident) = measure(i64::MAX / 2);
    assert_eq!(
        resident,
        (WARMUP_HEIGHTS + MEASURED_HEIGHTS) * BLOCKS_PER_HEIGHT + 1
    );
    assert!(bytes_per_block > MAX_RETAINED_BYTES_PER_BLOCK);
}
