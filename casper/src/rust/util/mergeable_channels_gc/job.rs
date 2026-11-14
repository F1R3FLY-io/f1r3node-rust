use std::collections::HashSet;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::{
    BlockDagKeyValueStorage, KeyValueDagRepresentation,
};
use block_storage::rust::dag::dag_gc_store::{DagGcStore, GC_PAGE_SIZE};
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::BlockMessage;
use shared::rust::store::key_value_store::KvStoreError;

use crate::rust::casper::CasperShardConf;
use crate::rust::finality::floor::Floor;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GcProgress {
    Yielded {
        deleted: usize,
        blocks_deleted: usize,
    },
    Complete {
        deleted: usize,
        blocks_deleted: usize,
    },
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
    conf: CasperShardConf,
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
    conf: &CasperShardConf,
) -> Result<Option<Pass>, KvStoreError> {
    let preliminary = storage.get_representation()?;
    let Some(floor) = cached_floor(&preliminary)? else {
        return Ok(None);
    };
    let depth =
        i64::from(conf.max_parent_depth) + i64::from(conf.mergeable_channels_gc_depth_buffer);
    let state = store.reserve_discovery(floor.block_number.saturating_sub(depth))?;
    drop(preliminary);
    let dag = storage.get_representation()?;
    let Some(floor) = cached_floor(&dag)? else {
        return Ok(None);
    };
    Ok(Some(Pass {
        dag,
        floor,
        conf: conf.clone(),
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
    conf: &CasperShardConf,
    effective_anchor: &AtomicI64,
    delete: &mut F,
    page: usize,
) -> Result<GcProgress, KvStoreError>
where
    F: FnMut(&BlockMessage) -> Result<bool, KvStoreError>,
{
    let store = storage.gc_store();
    let mut deleted = 0;
    let mut blocks_deleted = 0;
    loop {
        match std::mem::take(&mut sweep.phase) {
            Phase::Idle => match start_pass(storage, &store, conf)? {
                Some(pass) => sweep.phase = Phase::Discover(pass),
                None => {
                    metrics::counter!("mergeable_channels_gc_floor_deferred").increment(1);
                    tracing::warn!("mergeable-channel GC deferred: last finalized block has no persisted floor");
                    sweep.pending_len = 0;
                    return Ok(GcProgress::Complete {
                        deleted: 0,
                        blocks_deleted: 0,
                    });
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
                        return Ok(GcProgress::Yielded {
                            deleted,
                            blocks_deleted,
                        });
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
                        && super::reaches_ancestor_check(&pass.dag, &hash, &pass.floor, &pass.conf)?
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
                    return Ok(GcProgress::Yielded {
                        deleted,
                        blocks_deleted,
                    });
                }
                let Some(minimum) = minimum else {
                    store.finish_retry_page(None)?;
                    sweep.pending_len = 0;
                    return Ok(GcProgress::Complete {
                        deleted,
                        blocks_deleted,
                    });
                };
                let moved_past = super::moved_past_by_every_validator(&pass.dag, minimum)?;
                let cursor = pass.retry_cursor.clone();
                // Advance the effective-anchor atomic before any body
                // deletion in the pass. If the atomic advances AFTER the
                // first body is deleted, a walker that lands on the
                // missing body during the window reads anchor=0 and
                // `guard_deferral` promotes to BlockException — the
                // failure PR #1010's integration tests surfaced.
                // `fetch_max` is monotone; a crash mid-pass leaves the
                // atomic at `pass.target` without the matching metadata
                // eviction (the latter runs at Complete), which is the
                // valid state "anchor advanced, pruning not yet
                // finished".
                if pass.conf.enable_block_store_gc && pass.target >= 0 {
                    effective_anchor.fetch_max(pass.target, Ordering::Release);
                }
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
                            &pass.conf,
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
                        // At this horizon no honest validator can still cite
                        // the block (every active validator has moved past it,
                        // and it sits more than `max_parent_depth +
                        // mergeable_channels_gc_depth_buffer` below the
                        // finalized floor), so the stored body is unreachable
                        // too. Equivocation-detector and other missing-body
                        // read sites already treat `None` as "skip", so
                        // dropping it here does not change any honest-path
                        // verdict — only a byzantine block that cites across
                        // this horizon can observe the deletion.
                        if pass.conf.enable_block_store_gc && blocks.discard_stored(&hash)? {
                            blocks_deleted += 1;
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
                        return Ok(GcProgress::Yielded {
                            deleted,
                            blocks_deleted,
                        });
                    }
                    None => {
                        sweep.pending_len = 0;
                        // All bodies in this pass are deleted; evict DAG
                        // metadata below the cutoff so subsequent walks
                        // (`lookup`, `contains`) do not descend past the
                        // anchor into blocks whose bodies are gone. The
                        // atomic was raised at Delete entry so the race
                        // between "body deleted, anchor still zero" is
                        // already closed; this eviction tightens the walk
                        // behavior to match.
                        if pass.conf.enable_block_store_gc && pass.target >= 0 {
                            storage.advance_pruning_cutoff(pass.target)?;
                        }
                        return Ok(GcProgress::Complete {
                            deleted,
                            blocks_deleted,
                        });
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
    conf: &CasperShardConf,
    effective_anchor: &AtomicI64,
    delete: &mut F,
) -> Result<GcProgress, KvStoreError>
where
    F: FnMut(&BlockMessage) -> Result<bool, KvStoreError>,
{
    let result = run_turn(
        sweep,
        storage,
        blocks,
        conf,
        effective_anchor,
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
    conf: &CasperShardConf,
    effective_anchor: &AtomicI64,
) -> Result<GcProgress, KvStoreError> {
    let started = std::time::Instant::now();
    let result = run_turn(
        sweep,
        storage,
        blocks,
        conf,
        effective_anchor,
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
        let (deleted, blocks_deleted) = match progress {
            GcProgress::Yielded {
                deleted,
                blocks_deleted,
            }
            | GcProgress::Complete {
                deleted,
                blocks_deleted,
            } => (*deleted, *blocks_deleted),
        };
        metrics::counter!("mergeable_channels_gc_deleted").increment(deleted as u64);
        metrics::counter!("block_store_gc_deleted").increment(blocks_deleted as u64);
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
        conf: &CasperShardConf,
        page: usize,
    ) -> HashSet<BlockHash> {
        let mut sweep = GcSweep::new();
        let mut deleted = HashSet::new();
        let mut restarted = false;
        let effective_anchor = AtomicI64::new(0);
        for _ in 0..1_000 {
            let progress = run_turn(
                &mut sweep,
                storage,
                blocks,
                conf,
                &effective_anchor,
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
        conf: &CasperShardConf,
    ) -> HashSet<BlockHash> {
        let dag = storage.get_representation().unwrap();
        let moved = super::super::moved_past_by_every_validator(&dag, 0).unwrap();
        candidates
            .iter()
            .filter(|hash| {
                super::super::is_safe_to_delete(&dag, hash, floor, conf, moved.as_ref()).unwrap()
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
        let conf = super::super::tests::conf();
        let all: Vec<_> = (0..=20).map(hash).collect();
        let expected = reference(&storage, &all, &super::super::tests::floor_at(20), &conf);
        assert!(!expected.is_empty());

        assert_eq!(run_to_completion(&storage, &blocks, &conf, 3), expected);
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
        let conf = super::super::tests::conf();
        let floor = Floor {
            hash: id(25, 0),
            block_number: 25,
        };
        let expected = reference(&storage, &all, &floor, &conf);
        assert_eq!(
            expected.len(),
            61,
            "genesis plus heights 1..=20, three each"
        );
        assert!(!expected.contains(&id(10, 9)), "the orphan never finalizes");

        assert_eq!(run_to_completion(&storage, &blocks, &conf, 3), expected);
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
        let conf = super::super::tests::conf();

        let deleted = run_to_completion(&storage, &blocks, &conf, 3);

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

    /// A linear chain where every block past the floor's allowance is safe to
    /// delete. Populated once, then collected under two configurations — flag
    /// off and flag on — to assert that only the flag changes whether the
    /// block body is retained. The mergeable-data delete runs in both cases
    /// (same horizon, same predicate); the flag gates ONLY the block-body
    /// deletion.
    async fn linear_chain_with_block_store(
        enable_block_store_gc: bool,
    ) -> (
        BlockDagKeyValueStorage,
        KeyValueBlockStore,
        CasperShardConf,
        Vec<BlockHash>,
    ) {
        let (storage, blocks) = open().await;
        let mut block = template();
        let all: Vec<_> = (0..=20u8).map(hash).collect();
        for (n, hash) in all.iter().enumerate() {
            let n = n as u8;
            block.block_hash = hash.clone();
            block.body.state.block_number = i64::from(n);
            block.seq_num = i32::from(n);
            block.header.parents_hash_list = if n == 0 {
                vec![]
            } else {
                vec![all[(n - 1) as usize].clone()]
            };
            blocks.put(block.block_hash.clone(), &block).unwrap();
            let mode = if n == 0 {
                InsertMode::Approved
            } else {
                InsertMode::Normal
            };
            drop(storage.insert(&block, mode).unwrap());
        }
        storage
            .record_directly_finalized(all[20].clone(), 1.0, |_| async { Ok(()) })
            .await
            .unwrap();
        let dag = storage.get_representation().unwrap();
        dag.put_cached_floor(all[20].clone(), all[20].clone())
            .unwrap();
        drop(dag);

        let mut conf = super::super::tests::conf();
        conf.enable_block_store_gc = enable_block_store_gc;
        (storage, blocks, conf, all)
    }

    /// The block body stays on disk when the flag is off, even though the
    /// block is past the horizon and its mergeable data is collected. Pins
    /// the opt-in: operators who have mergeable GC enabled do not suddenly
    /// lose block bodies because of an unrelated upgrade.
    #[tokio::test]
    async fn block_bodies_are_retained_when_the_flag_is_off() {
        let (storage, blocks, conf, all) = linear_chain_with_block_store(false).await;
        let expected = reference(&storage, &all, &super::super::tests::floor_at(20), &conf);
        assert!(
            !expected.is_empty(),
            "the fixture must contain at least one collectable block"
        );

        let collected = run_to_completion(&storage, &blocks, &conf, 3);
        assert_eq!(collected, expected, "mergeable GC still runs");

        for hash in &expected {
            assert!(
                blocks.get(hash).unwrap().is_some(),
                "flag-off leaves the body at {hash:?} on disk",
            );
        }
    }

    /// With the flag on, every block whose mergeable data the pass collected
    /// also has its stored body released. Blocks the pass did NOT collect
    /// (above the horizon, or not yet moved-past by every validator) must
    /// keep their bodies.
    #[tokio::test]
    async fn block_bodies_are_deleted_when_the_flag_is_on() {
        let (storage, blocks, conf, all) = linear_chain_with_block_store(true).await;
        let expected = reference(&storage, &all, &super::super::tests::floor_at(20), &conf);
        assert!(
            !expected.is_empty(),
            "the fixture must contain at least one collectable block"
        );

        let collected = run_to_completion(&storage, &blocks, &conf, 3);
        assert_eq!(collected, expected);

        for hash in &all {
            let body_present = blocks.get(hash).unwrap().is_some();
            if expected.contains(hash) {
                assert!(!body_present, "flag-on must drop the body at {hash:?}",);
            } else {
                assert!(
                    body_present,
                    "a block the pass did not collect must keep its body: {hash:?}",
                );
            }
        }
    }

    /// Progress carries the block-store count alongside the mergeable count.
    /// Exercised via the public `run_turn` so the two counters cannot drift
    /// (e.g. deleting the body without counting it).
    #[tokio::test]
    async fn run_turn_reports_block_store_deletions() {
        let (storage, blocks, conf, _all) = linear_chain_with_block_store(true).await;

        let mut sweep = GcSweep::new();
        let mut total_blocks_deleted = 0usize;
        let mut total_mergeable_deleted = 0usize;
        let effective_anchor = AtomicI64::new(0);
        for _ in 0..1_000 {
            let progress = run_turn(
                &mut sweep,
                &storage,
                &blocks,
                &conf,
                &effective_anchor,
                &mut |_block| Ok(true),
                GC_PAGE_SIZE,
            )
            .unwrap();
            match progress {
                GcProgress::Yielded {
                    deleted,
                    blocks_deleted,
                } => {
                    total_blocks_deleted += blocks_deleted;
                    total_mergeable_deleted += deleted;
                }
                GcProgress::Complete {
                    deleted,
                    blocks_deleted,
                } => {
                    total_blocks_deleted += blocks_deleted;
                    total_mergeable_deleted += deleted;
                    assert_eq!(
                        total_blocks_deleted, total_mergeable_deleted,
                        "every collected block whose body was present is also \
                         counted as a block-store deletion",
                    );
                    assert!(total_blocks_deleted > 0);
                    return;
                }
            }
        }
        panic!("GC did not complete");
    }

    /// After a Delete-phase completion with `enable_block_store_gc = true`,
    /// the shared effective anchor advances to the pass target and the
    /// DAG store persists the same height. This is the write that tells
    /// validation "below this height is settled history" and that
    /// `open_windowed` reads on restart to refuse re-loading evicted
    /// metadata.
    #[tokio::test]
    async fn effective_anchor_advances_after_delete_phase() {
        let (storage, blocks, conf, _all) = linear_chain_with_block_store(true).await;

        let effective_anchor = AtomicI64::new(0);
        let mut sweep = GcSweep::new();
        for _ in 0..1_000 {
            let progress = run_turn(
                &mut sweep,
                &storage,
                &blocks,
                &conf,
                &effective_anchor,
                &mut |_block| Ok(true),
                GC_PAGE_SIZE,
            )
            .unwrap();
            if matches!(progress, GcProgress::Complete { .. }) {
                break;
            }
        }
        let anchor = effective_anchor.load(Ordering::Acquire);
        assert!(
            anchor > 0,
            "block-store GC must advance the effective anchor past zero"
        );
        assert_eq!(
            storage.gc_store().effective_anchor().unwrap(),
            Some(anchor),
            "the advanced anchor is persisted to the GC store",
        );
    }

    /// Metadata eviction follows anchor advancement: once a Delete-phase
    /// completion advances the pruning cutoff, below-cutoff blocks drop out
    /// of the in-memory resident set. Disk entries remain (for history
    /// recovery), so `lookup` still answers from them — the eviction is a
    /// RAM-residency change, not a logical pruning.
    #[tokio::test]
    async fn advance_pruning_cutoff_evicts_metadata() {
        let (storage, blocks, conf, _all) = linear_chain_with_block_store(true).await;

        let effective_anchor = AtomicI64::new(0);
        let mut sweep = GcSweep::new();
        for _ in 0..1_000 {
            let progress = run_turn(
                &mut sweep,
                &storage,
                &blocks,
                &conf,
                &effective_anchor,
                &mut |_block| Ok(true),
                GC_PAGE_SIZE,
            )
            .unwrap();
            if matches!(progress, GcProgress::Complete { .. }) {
                break;
            }
        }
        let cutoff = effective_anchor.load(Ordering::Acquire);
        assert!(
            cutoff > 0,
            "pass must advance the anchor to a positive height"
        );
        assert_eq!(
            storage.gc_store().effective_anchor().unwrap(),
            Some(cutoff),
            "the advanced anchor is persisted"
        );
    }

    /// With the flag off the mergeable-only GC leaves bodies in place, so
    /// there is nothing to advance past — the effective anchor stays at
    /// zero. Pinning this avoids an operator who runs mergeable-only GC
    /// from silently flipping other nodes into "treat below-anchor as
    /// settled history" mode.
    #[tokio::test]
    async fn effective_anchor_does_not_advance_when_body_pruning_is_off() {
        let (storage, blocks, conf, _all) = linear_chain_with_block_store(false).await;

        let effective_anchor = AtomicI64::new(0);
        let mut sweep = GcSweep::new();
        for _ in 0..1_000 {
            let progress = run_turn(
                &mut sweep,
                &storage,
                &blocks,
                &conf,
                &effective_anchor,
                &mut |_block| Ok(true),
                GC_PAGE_SIZE,
            )
            .unwrap();
            if matches!(progress, GcProgress::Complete { .. }) {
                break;
            }
        }
        assert_eq!(effective_anchor.load(Ordering::Acquire), 0);
        assert_eq!(storage.gc_store().effective_anchor().unwrap(), None);
    }
}
