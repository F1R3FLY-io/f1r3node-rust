use std::collections::HashSet;
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::{
    BlockDagKeyValueStorage, KeyValueDagRepresentation,
};
use block_storage::rust::dag::dag_gc_store::{DagGcStore, GC_PAGE_SIZE};
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::BlockMessage;
use shared::rust::store::key_value_store::KvStoreError;

use crate::rust::finality::floor::Floor;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GcProgress {
    Yielded { deleted: usize },
    Complete { deleted: usize },
}

/// A garbage-collection pass, resumed one page per turn. Discovery progress
/// and the queue of candidates not yet deletable are durable; the walk that
/// proves every validator has moved past a candidate is rebuilt in memory for
/// each pass.
#[derive(Default)]
pub struct GcSweep {
    phase: Phase,
    pending_len: usize,
}

impl GcSweep {
    pub fn new() -> Self { Self::default() }
}

struct Pass {
    dag: KeyValueDagRepresentation,
    floor: Floor,
    max_allowed_depth: i64,
    target: i64,
    discovery_start: i64,
    discovery_cursor: Option<Vec<u8>>,
    retry_cursor: Option<Vec<u8>>,
}

#[derive(Default)]
enum Phase {
    #[default]
    Idle,
    Discover(Pass),
    FindMinimum {
        pass: Pass,
        cursor: Option<Vec<u8>>,
        minimum: Option<i64>,
    },
    Delete {
        pass: Pass,
        moved_past: Option<HashSet<BlockHash>>,
        cursor: Option<Vec<u8>>,
    },
}

fn cached_floor(dag: &KeyValueDagRepresentation) -> Result<Option<Floor>, KvStoreError> {
    let Some(hash) = dag.get_cached_floor(&dag.last_finalized_block())? else {
        return Ok(None);
    };
    let block_number = dag.block_number_unsafe(&hash)?;
    Ok(Some(Floor { hash, block_number }))
}

fn start_pass(
    storage: &BlockDagKeyValueStorage,
    store: &DagGcStore,
    max_allowed_depth: i64,
) -> Result<Option<Pass>, KvStoreError> {
    let preliminary = storage.get_representation()?;
    let Some(floor) = cached_floor(&preliminary)? else {
        return Ok(None);
    };
    let state = store.reserve_discovery(floor.block_number.saturating_sub(max_allowed_depth))?;
    drop(preliminary);
    let dag = storage.get_representation()?;
    let Some(floor) = cached_floor(&dag)? else {
        return Ok(None);
    };
    Ok(Some(Pass {
        dag,
        floor,
        max_allowed_depth,
        target: state.claimed_ceiling.unwrap_or(-1),
        discovery_start: state
            .completed_ceiling
            .map_or(0, |height| height.saturating_add(1)),
        discovery_cursor: state.cursor,
        retry_cursor: store.retry_cursor()?,
    }))
}

fn run_turn<F>(
    sweep: &mut GcSweep,
    storage: &BlockDagKeyValueStorage,
    blocks: &KeyValueBlockStore,
    max_allowed_depth: i64,
    delete: &mut F,
    page: usize,
) -> Result<GcProgress, KvStoreError>
where
    F: FnMut(&BlockMessage) -> Result<bool, KvStoreError>,
{
    let store = storage.gc_store();
    let mut deleted = 0;
    loop {
        match std::mem::take(&mut sweep.phase) {
            Phase::Idle => match start_pass(storage, &store, max_allowed_depth)? {
                Some(pass) => sweep.phase = Phase::Discover(pass),
                None => {
                    metrics::counter!("mergeable_channels_gc_floor_deferred").increment(1);
                    tracing::warn!("mergeable-channel GC deferred: last finalized block has no persisted floor");
                    sweep.pending_len = 0;
                    return Ok(GcProgress::Complete { deleted: 0 });
                }
            },
            Phase::Discover(mut pass) => {
                if pass.discovery_start <= pass.target {
                    let rows = pass.dag.scan_height_range(
                        pass.discovery_start,
                        pass.target,
                        true,
                        pass.discovery_cursor.as_deref(),
                        page,
                    )?;
                    let discovered = rows
                        .rows
                        .iter()
                        .map(|hash| Ok((hash.clone(), pass.dag.block_number_unsafe(hash)?)))
                        .collect::<Result<Vec<_>, KvStoreError>>()?;
                    let completed = rows.next.is_none().then_some(pass.target);
                    store.commit_discovery_page(&discovered, rows.next.clone(), completed)?;
                    if let Some(next) = rows.next {
                        pass.discovery_cursor = Some(next);
                        sweep.phase = Phase::Discover(pass);
                        return Ok(GcProgress::Yielded { deleted });
                    }
                }
                let cursor = pass.retry_cursor.clone();
                sweep.phase = Phase::FindMinimum {
                    pass,
                    cursor,
                    minimum: None,
                };
            }
            Phase::FindMinimum {
                pass,
                cursor,
                mut minimum,
            } => {
                let rows = store.retry_page(cursor.as_deref(), page)?;
                for (hash, _) in rows.rows {
                    if pass.dag.contains(&hash)?
                        && super::reaches_ancestor_check(
                            &pass.dag,
                            &hash,
                            &pass.floor,
                            pass.max_allowed_depth,
                        )?
                    {
                        let height = pass.dag.block_number_unsafe(&hash)?;
                        minimum = Some(minimum.map_or(height, |lowest| lowest.min(height)));
                    }
                }
                if let Some(next) = rows.next {
                    sweep.phase = Phase::FindMinimum {
                        pass,
                        cursor: Some(next),
                        minimum,
                    };
                    return Ok(GcProgress::Yielded { deleted });
                }
                let Some(minimum) = minimum else {
                    store.finish_retry_page(None)?;
                    sweep.pending_len = 0;
                    return Ok(GcProgress::Complete { deleted });
                };
                let moved_past = super::moved_past_by_every_validator(&pass.dag, minimum)?;
                let cursor = pass.retry_cursor.clone();
                sweep.phase = Phase::Delete {
                    pass,
                    moved_past,
                    cursor,
                };
            }
            Phase::Delete {
                pass,
                moved_past,
                cursor,
            } => {
                let rows = store.retry_page(cursor.as_deref(), page)?;
                let mut collected = 0;
                for (hash, _) in rows.rows {
                    if !pass.dag.contains(&hash)?
                        || !super::is_safe_to_delete(
                            &pass.dag,
                            &hash,
                            &pass.floor,
                            pass.max_allowed_depth,
                            moved_past.as_ref(),
                        )?
                    {
                        continue;
                    }
                    collected += 1;
                    if let Some(block) = blocks.get(&hash)? {
                        if delete(&block)? {
                            deleted += 1;
                        }
                    }
                    store.remove_retry(&hash)?;
                }
                sweep.pending_len = collected;
                store.finish_retry_page(rows.next.as_deref())?;
                match rows.next {
                    Some(next) => {
                        sweep.phase = Phase::Delete {
                            pass,
                            moved_past,
                            cursor: Some(next),
                        };
                        return Ok(GcProgress::Yielded { deleted });
                    }
                    None => {
                        sweep.pending_len = 0;
                        return Ok(GcProgress::Complete { deleted });
                    }
                }
            }
        }
    }
}

#[cfg(any(test, feature = "test-utils"))]
pub fn collect_garbage_with<F>(
    sweep: &mut GcSweep,
    storage: &BlockDagKeyValueStorage,
    blocks: &KeyValueBlockStore,
    max_allowed_depth: i64,
    delete: &mut F,
) -> Result<GcProgress, KvStoreError>
where
    F: FnMut(&BlockMessage) -> Result<bool, KvStoreError>,
{
    let result = run_turn(
        sweep,
        storage,
        blocks,
        max_allowed_depth,
        delete,
        GC_PAGE_SIZE,
    );
    if result.is_err() {
        sweep.phase = Phase::Idle;
        sweep.pending_len = 0;
    }
    result
}

pub async fn collect_garbage(
    sweep: &mut GcSweep,
    storage: &BlockDagKeyValueStorage,
    blocks: &KeyValueBlockStore,
    runtime: &Arc<RuntimeManager>,
    max_allowed_depth: i64,
) -> Result<GcProgress, KvStoreError> {
    let started = std::time::Instant::now();
    let result = run_turn(
        sweep,
        storage,
        blocks,
        max_allowed_depth,
        &mut |block| {
            runtime
                .delete_mergeable_channels(
                    &block.body.state.post_state_hash,
                    block.sender.clone(),
                    block.seq_num,
                )
                .map_err(|error| KvStoreError::IoError(error.to_string()))
        },
        GC_PAGE_SIZE,
    );
    if result.is_err() {
        sweep.phase = Phase::Idle;
        sweep.pending_len = 0;
    }
    if let Ok(progress) = result.as_ref() {
        let deleted = match progress {
            GcProgress::Yielded { deleted } | GcProgress::Complete { deleted } => *deleted,
        };
        metrics::counter!("mergeable_channels_gc_deleted").increment(deleted as u64);
        if matches!(progress, GcProgress::Yielded { .. }) {
            metrics::counter!("mergeable_channels_gc_yields").increment(1);
        }
    }
    metrics::gauge!("mergeable_channels_gc_active_candidates").set(sweep.pending_len as f64);
    metrics::histogram!("mergeable_channels_gc.turn.time").record(started.elapsed().as_secs_f64());
    result
}

#[cfg(test)]
mod tests {
    use block_storage::rust::dag::block_dag_key_value_storage::InsertMode;
    use models::rust::block_implicits::get_random_block;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

    use super::*;

    fn hash(n: u8) -> BlockHash { vec![n; 32].into() }

    fn template() -> BlockMessage {
        get_random_block(
            Some(0),
            Some(0),
            None,
            None,
            Some(vec![1; 65].into()),
            None,
            Some(0),
            Some(vec![]),
            Some(vec![]),
            Some(vec![]),
            Some(vec![]),
            Some(vec![]),
            None,
            None,
        )
    }

    async fn open() -> (BlockDagKeyValueStorage, KeyValueBlockStore) {
        let mut manager = InMemoryStoreManager::new();
        let storage = BlockDagKeyValueStorage::new_with_hot_window(&mut manager, 2)
            .await
            .unwrap();
        let blocks = KeyValueBlockStore::create_from_kvm(&mut manager)
            .await
            .unwrap();
        (storage, blocks)
    }

    /// Runs turns until the pass completes, replacing the sweep once mid-way
    /// through deletion, and returns the hashes `delete` saw.
    fn run_to_completion(
        storage: &BlockDagKeyValueStorage,
        blocks: &KeyValueBlockStore,
        max_allowed_depth: i64,
        page: usize,
    ) -> HashSet<BlockHash> {
        let mut sweep = GcSweep::new();
        let mut deleted = HashSet::new();
        let mut restarted = false;
        for _ in 0..1_000 {
            let progress = run_turn(
                &mut sweep,
                storage,
                blocks,
                max_allowed_depth,
                &mut |block| {
                    assert!(
                        deleted.insert(block.block_hash.clone()),
                        "candidate must not be deleted twice"
                    );
                    Ok(true)
                },
                page,
            )
            .unwrap();
            if !restarted && matches!(sweep.phase, Phase::Delete { .. }) {
                sweep = GcSweep::new();
                restarted = true;
            }
            if matches!(progress, GcProgress::Complete { .. }) {
                assert!(
                    restarted,
                    "the pass must have been interrupted mid-deletion"
                );
                assert!(matches!(sweep.phase, Phase::Idle));
                return deleted;
            }
        }
        panic!("GC did not complete");
    }

    fn reference(
        storage: &BlockDagKeyValueStorage,
        candidates: &[BlockHash],
        floor: &Floor,
        max_allowed_depth: i64,
    ) -> HashSet<BlockHash> {
        let dag = storage.get_representation().unwrap();
        let moved = super::super::moved_past_by_every_validator(&dag, 0).unwrap();
        candidates
            .iter()
            .filter(|hash| {
                super::super::is_safe_to_delete(
                    &dag,
                    hash,
                    floor,
                    max_allowed_depth,
                    moved.as_ref(),
                )
                .unwrap()
            })
            .cloned()
            .collect()
    }

    #[tokio::test]
    async fn gc_resumes_after_restart_and_matches_reference_on_cold_history() {
        let (storage, blocks) = open().await;
        let mut block = template();
        for n in 0..=20 {
            block.block_hash = hash(n);
            block.body.state.block_number = i64::from(n);
            block.seq_num = i32::from(n);
            block.header.parents_hash_list = if n == 0 { vec![] } else { vec![hash(n - 1)] };
            blocks.put(block.block_hash.clone(), &block).unwrap();
            let mode = if n == 0 {
                InsertMode::Approved
            } else {
                InsertMode::Normal
            };
            drop(storage.insert(&block, mode).unwrap());
        }
        storage
            .record_directly_finalized(hash(20), 1.0, |_| async { Ok(()) })
            .await
            .unwrap();
        let dag = storage.get_representation().unwrap();
        dag.put_cached_floor(hash(20), hash(20)).unwrap();
        assert!(dag.resident_counts().blocks < 21);
        drop(dag);
        let max_allowed_depth = super::super::tests::depth();
        let all: Vec<_> = (0..=20).map(hash).collect();
        let expected = reference(
            &storage,
            &all,
            &super::super::tests::floor_at(20),
            max_allowed_depth,
        );
        assert!(!expected.is_empty());

        assert_eq!(
            run_to_completion(&storage, &blocks, max_allowed_depth, 3),
            expected
        );
    }

    #[tokio::test]
    async fn gc_matches_reference_on_a_three_validator_dag_with_an_orphan() {
        let (storage, blocks) = open().await;
        let id = |height: u8, v: u8| -> BlockHash {
            let mut bytes = vec![7u8; 32];
            bytes[0] = height;
            bytes[1] = v;
            bytes.into()
        };
        let mut block = template();
        let mut put = |hash: BlockHash,
                       height: u8,
                       seq: i32,
                       sender: u8,
                       parents: Vec<BlockHash>,
                       mode: InsertMode| {
            block.block_hash = hash;
            block.body.state.block_number = i64::from(height);
            block.seq_num = seq;
            block.sender = vec![sender; 65].into();
            block.header.parents_hash_list = parents;
            blocks.put(block.block_hash.clone(), &block).unwrap();
            drop(storage.insert(&block, mode).unwrap());
        };
        put(hash(0), 0, 0, 10, vec![], InsertMode::Approved);
        let mut all = vec![hash(0)];
        for height in 1..=30u8 {
            for v in 0..3u8 {
                let parents = if height == 1 {
                    vec![hash(0)]
                } else {
                    (0..3).map(|o| id(height - 1, o)).collect()
                };
                put(
                    id(height, v),
                    height,
                    i32::from(height),
                    10 + v,
                    parents,
                    InsertMode::Normal,
                );
                all.push(id(height, v));
            }
            if height == 10 {
                put(id(10, 9), 10, 10, 10, vec![id(9, 0)], InsertMode::Normal);
                all.push(id(10, 9));
            }
        }
        storage
            .record_directly_finalized(id(28, 0), 1.0, |_| async { Ok(()) })
            .await
            .unwrap();
        let dag = storage.get_representation().unwrap();
        dag.put_cached_floor(id(28, 0), id(25, 0)).unwrap();
        assert!(dag.resident_counts().blocks < 20);
        drop(dag);
        let max_allowed_depth = super::super::tests::depth();
        let floor = Floor {
            hash: id(25, 0),
            block_number: 25,
        };
        let expected = reference(&storage, &all, &floor, max_allowed_depth);
        assert_eq!(
            expected.len(),
            61,
            "genesis plus heights 1..=20, three each"
        );
        assert!(!expected.contains(&id(10, 9)), "the orphan never finalizes");

        assert_eq!(
            run_to_completion(&storage, &blocks, max_allowed_depth, 3),
            expected
        );
    }

    #[tokio::test]
    async fn a_candidate_whose_body_is_missing_leaves_the_retry_queue() {
        let (storage, blocks) = open().await;
        let mut block = template();
        for n in 0..=20 {
            block.block_hash = hash(n);
            block.body.state.block_number = i64::from(n);
            block.seq_num = i32::from(n);
            block.header.parents_hash_list = if n == 0 { vec![] } else { vec![hash(n - 1)] };
            if n != 5 {
                blocks.put(block.block_hash.clone(), &block).unwrap();
            }
            let mode = if n == 0 {
                InsertMode::Approved
            } else {
                InsertMode::Normal
            };
            drop(storage.insert(&block, mode).unwrap());
        }
        storage
            .record_directly_finalized(hash(20), 1.0, |_| async { Ok(()) })
            .await
            .unwrap();
        storage
            .get_representation()
            .unwrap()
            .put_cached_floor(hash(20), hash(20))
            .unwrap();
        let max_allowed_depth = super::super::tests::depth();

        let deleted = run_to_completion(&storage, &blocks, max_allowed_depth, 3);

        assert!(!deleted.contains(&hash(5)), "there is no body to delete");
        assert!(deleted.contains(&hash(4)) && deleted.contains(&hash(6)));
        let queued: HashSet<_> = storage
            .gc_store()
            .retry_page(None, GC_PAGE_SIZE)
            .unwrap()
            .rows
            .into_iter()
            .map(|(hash, _)| hash)
            .collect();
        assert!(
            !queued.contains(&hash(5)),
            "a collectable candidate leaves the queue even without a body"
        );
    }
}
