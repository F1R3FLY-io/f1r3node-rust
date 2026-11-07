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

use crate::rust::casper::CasperShardConf;
use crate::rust::finality::floor::Floor;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

const TURN_BUDGET: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GcProgress {
    Yielded { deleted: usize },
    Complete { deleted: usize },
}

pub struct GcSweep {
    phase: Phase,
    pending_len: usize,
    work: GcWorkStats,
}

#[derive(Default, Debug, Clone, Copy)]
struct GcWorkStats {
    parent_edges: u64,
    work_records: u64,
}

impl Default for GcSweep {
    fn default() -> Self {
        Self {
            phase: Phase::Recover,
            pending_len: 0,
            work: GcWorkStats::default(),
        }
    }
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

struct Job {
    pass: Pass,
    id: u64,
    candidates: Vec<BlockHash>,
    next: Option<Vec<u8>>,
    minimum: i64,
    frontier: Vec<BlockHash>,
    frontier_seen: Vec<Option<u64>>,
    advance: Vec<usize>,
    deletion_position: usize,
    cleanup_position: usize,
}

enum Phase {
    Recover,
    Idle,
    Discover(Pass),
    FindMinimum {
        pass: Pass,
        cursor: Option<Vec<u8>>,
        minimum: Option<i64>,
    },
    Select(Job),
    Anchor(Job),
    Walk(Job),
    Delete(Job),
    Cleanup(Job),
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

fn create_job(pass: Pass, store: &DagGcStore, minimum: i64) -> Result<Job, KvStoreError> {
    let mut frontier = Vec::new();
    let mut unique = HashSet::new();
    for (validator, hash) in pass.dag.latest_message_hashes() {
        if pass.dag.own_testimony(&validator, &hash)?.is_some() && unique.insert(hash.clone()) {
            let Some(parent) = pass.dag.main_parent(&hash)? else {
                frontier.clear();
                break;
            };
            frontier.push(parent);
        }
    }
    Ok(Job {
        pass,
        id: store.next_job()?,
        candidates: Vec::new(),
        next: None,
        minimum,
        frontier_seen: vec![None; frontier.len()],
        frontier,
        advance: Vec::new(),
        deletion_position: 0,
        cleanup_position: 0,
    })
}

fn advance_anchor(
    job: &mut Job,
    store: &DagGcStore,
    budget: &mut usize,
) -> Result<bool, KvStoreError> {
    while *budget > 0 {
        if job.frontier.is_empty() {
            return Ok(true);
        }
        if job.frontier.iter().any(|hash| hash.len() != 32) {
            job.frontier.clear();
            job.advance.clear();
            return Ok(true);
        }
        job.frontier_seen.resize(job.frontier.len(), None);
        if job.advance.is_empty() {
            if job.frontier.iter().all(|hash| hash == &job.frontier[0]) {
                let anchor = &job.frontier[0];
                let expand = job
                    .pass
                    .dag
                    .block_number(anchor)?
                    .is_some_and(|h| h > job.minimum);
                store.seed_work(job.id, anchor, expand)?;
                return Ok(true);
            }
            let mut highest = i64::MIN;
            for hash in &job.frontier {
                let Some(height) = job.pass.dag.block_number(hash)? else {
                    job.frontier.clear();
                    return Ok(true);
                };
                highest = highest.max(height);
            }
            for (index, hash) in job.frontier.iter().enumerate() {
                if job.pass.dag.block_number(hash)? == Some(highest) {
                    job.advance.push(index);
                }
            }
        }
        while *budget > 0 {
            let Some(index) = job.advance.pop() else {
                break;
            };
            *budget -= 1;
            let Some(parent) = job.pass.dag.main_parent(&job.frontier[index])? else {
                job.frontier.clear();
                job.advance.clear();
                return Ok(true);
            };
            if parent.len() != 32 {
                job.frontier.clear();
                job.advance.clear();
                return Ok(true);
            }
            let seen = if let Some(id) = job.frontier_seen[index] {
                id
            } else {
                let id = store.next_job()?;
                store.seed_work(id, &job.frontier[index], false)?;
                job.frontier_seen[index] = Some(id);
                id
            };
            if store.visited(seen, &parent)? {
                return Err(KvStoreError::InvalidArgument(format!(
                    "GC common-anchor main-parent cycle at {}",
                    hex::encode(&parent)
                )));
            }
            store.seed_work(seen, &parent, false)?;
            job.frontier[index] = parent;
        }
    }
    Ok(false)
}

fn advance_ancestry(
    job: &Job,
    store: &DagGcStore,
    budget: &mut usize,
    stats: &mut GcWorkStats,
) -> Result<bool, KvStoreError> {
    while *budget > 0 {
        let Some(current) = store.next_work(job.id)? else {
            return Ok(true);
        };
        *budget -= 1;
        stats.work_records += 1;
        let metadata = job.pass.dag.lookup_unsafe(&current.hash)?;
        if current.next_parent > metadata.parents.len() {
            return Err(KvStoreError::SerializationError(
                "GC parent cursor exceeds metadata".into(),
            ));
        }
        let end = metadata.parents.len().min(
            current
                .next_parent
                .saturating_add((*budget).min(GC_PAGE_SIZE)),
        );
        let mut discovered = Vec::with_capacity(end - current.next_parent);
        for parent in &metadata.parents[current.next_parent..end] {
            *budget -= 1;
            stats.parent_edges += 1;
            if parent.len() != 32 {
                continue;
            }
            let expand = job
                .pass
                .dag
                .block_number(parent)?
                .is_some_and(|height| height > job.minimum);
            discovered.push((parent.clone(), expand));
        }
        let next = (end < metadata.parents.len()).then_some(end);
        store.advance_work(job.id, &current.hash, next, &discovered)?;
    }
    Ok(false)
}

fn run_turn<F>(
    sweep: &mut GcSweep,
    storage: &BlockDagKeyValueStorage,
    blocks: &KeyValueBlockStore,
    conf: &CasperShardConf,
    delete: &mut F,
    budget_limit: usize,
) -> Result<GcProgress, KvStoreError>
where
    F: FnMut(&BlockMessage) -> Result<bool, KvStoreError>,
{
    let store = storage.gc_store();
    let mut budget = budget_limit;
    let mut deleted = 0;
    loop {
        match std::mem::replace(&mut sweep.phase, Phase::Recover) {
            Phase::Recover => {
                if store.cleanup_page(None)? {
                    sweep.phase = Phase::Idle;
                }
                return Ok(GcProgress::Yielded { deleted });
            }
            Phase::Idle => match start_pass(storage, &store, conf)? {
                Some(pass) => sweep.phase = Phase::Discover(pass),
                None => {
                    metrics::counter!("mergeable_channels_gc_floor_deferred").increment(1);
                    tracing::warn!("mergeable-channel GC deferred: last finalized block has no persisted floor");
                    sweep.phase = Phase::Idle;
                    sweep.pending_len = 0;
                    return Ok(GcProgress::Complete { deleted: 0 });
                }
            },
            Phase::Discover(mut pass) => {
                if pass.discovery_start <= pass.target {
                    let page = pass.dag.scan_height_range(
                        pass.discovery_start,
                        pass.target,
                        true,
                        pass.discovery_cursor.as_deref(),
                        GC_PAGE_SIZE,
                    )?;
                    let rows = page
                        .rows
                        .iter()
                        .map(|hash| Ok((hash.clone(), pass.dag.block_number_unsafe(hash)?)))
                        .collect::<Result<Vec<_>, KvStoreError>>()?;
                    let completed = page.next.is_none().then_some(pass.target);
                    store.commit_discovery_page(&rows, page.next.clone(), completed)?;
                    if let Some(next) = page.next {
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
                let page = store.retry_page(cursor.as_deref())?;
                for (hash, _) in page.rows {
                    if pass.dag.contains(&hash)?
                        && super::reaches_ancestor_check(&pass.dag, &hash, &pass.floor, &pass.conf)?
                    {
                        let height = pass.dag.block_number_unsafe(&hash)?;
                        minimum = Some(minimum.map_or(height, |old| old.min(height)));
                    }
                }
                if let Some(cursor) = page.next {
                    sweep.phase = Phase::FindMinimum {
                        pass,
                        cursor: Some(cursor),
                        minimum,
                    };
                    return Ok(GcProgress::Yielded { deleted });
                }
                if let Some(minimum) = minimum {
                    let job = create_job(pass, &store, minimum)?;
                    sweep.phase = if job.frontier.is_empty() {
                        Phase::Cleanup(job)
                    } else {
                        Phase::Anchor(job)
                    };
                } else {
                    store.finish_retry_page(None)?;
                    sweep.phase = Phase::Idle;
                    sweep.pending_len = 0;
                    return Ok(GcProgress::Complete { deleted });
                }
            }
            Phase::Select(mut job) => {
                let page = store.retry_page(job.pass.retry_cursor.as_deref())?;
                job.candidates.clear();
                for (hash, _) in page.rows {
                    if job.pass.dag.contains(&hash)?
                        && job.pass.dag.block_number_unsafe(&hash)? >= job.minimum
                        && super::reaches_ancestor_check(
                            &job.pass.dag,
                            &hash,
                            &job.pass.floor,
                            &job.pass.conf,
                        )?
                    {
                        job.candidates.push(hash);
                    }
                }
                job.deletion_position = 0;
                job.next = page.next;
                sweep.pending_len = job.candidates.len();
                sweep.phase = Phase::Delete(job);
            }
            Phase::Anchor(mut job) => {
                let before = budget;
                let complete = advance_anchor(&mut job, &store, &mut budget)?;
                sweep.work.parent_edges += (before - budget) as u64;
                if complete {
                    sweep.phase = if job.frontier.is_empty() {
                        Phase::Cleanup(job)
                    } else {
                        Phase::Walk(job)
                    };
                } else {
                    sweep.phase = Phase::Anchor(job);
                    return Ok(GcProgress::Yielded { deleted });
                }
            }
            Phase::Walk(job) => {
                if advance_ancestry(&job, &store, &mut budget, &mut sweep.work)? {
                    sweep.phase = Phase::Select(job);
                } else {
                    sweep.phase = Phase::Walk(job);
                    return Ok(GcProgress::Yielded { deleted });
                }
            }
            Phase::Delete(mut job) => {
                while let Some(hash) = job.candidates.get(job.deletion_position) {
                    if store.visited(job.id, hash)?
                        && super::reaches_ancestor_check(
                            &job.pass.dag,
                            hash,
                            &job.pass.floor,
                            &job.pass.conf,
                        )?
                    {
                        if let Some(block) = blocks.get(hash)? {
                            if delete(&block)? {
                                deleted += 1;
                            }
                            store.remove_retry(hash)?;
                        }
                    }
                    job.deletion_position += 1;
                }
                store.finish_retry_page(job.next.as_deref())?;
                match job.next.take() {
                    Some(next) => {
                        job.pass.retry_cursor = Some(next);
                        job.candidates.clear();
                        sweep.pending_len = 0;
                        sweep.phase = Phase::Select(job);
                        return Ok(GcProgress::Yielded { deleted });
                    }
                    None => sweep.phase = Phase::Cleanup(job),
                }
            }
            Phase::Cleanup(mut job) => {
                while job.cleanup_position <= job.frontier_seen.len() {
                    let id = if job.cleanup_position == 0 {
                        Some(job.id)
                    } else {
                        job.frontier_seen[job.cleanup_position - 1]
                    };
                    let Some(id) = id else {
                        job.cleanup_position += 1;
                        continue;
                    };
                    if store.cleanup_page(Some(id))? {
                        job.cleanup_position += 1;
                    }
                    sweep.phase = Phase::Cleanup(job);
                    return Ok(GcProgress::Yielded { deleted });
                }
                store.finish_retry_page(None)?;
                sweep.phase = Phase::Idle;
                sweep.pending_len = 0;
                return Ok(GcProgress::Complete { deleted });
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
    delete: &mut F,
) -> Result<GcProgress, KvStoreError>
where
    F: FnMut(&BlockMessage) -> Result<bool, KvStoreError>,
{
    let result = run_turn(sweep, storage, blocks, conf, delete, TURN_BUDGET);
    if result.is_err() {
        sweep.phase = Phase::Recover;
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
) -> Result<GcProgress, KvStoreError> {
    let started = std::time::Instant::now();
    let before = sweep.work;
    let result = run_turn(
        sweep,
        storage,
        blocks,
        conf,
        &mut |block| {
            runtime
                .delete_mergeable_channels(
                    &block.body.state.post_state_hash,
                    block.sender.clone(),
                    block.seq_num,
                )
                .map_err(|error| KvStoreError::IoError(error.to_string()))
        },
        TURN_BUDGET,
    );
    if result.is_err() {
        sweep.phase = Phase::Recover;
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
    metrics::counter!("mergeable_channels_gc_parent_edges")
        .increment(sweep.work.parent_edges - before.parent_edges);
    metrics::counter!("mergeable_channels_gc_work_records")
        .increment(sweep.work.work_records - before.work_records);
    metrics::histogram!("mergeable_channels_gc.turn.time").record(started.elapsed().as_secs_f64());
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(n: u8) -> BlockHash { vec![n; 32].into() }

    #[tokio::test]
    async fn gc_resumes_after_restart_and_matches_reference_on_cold_history() {
        use block_storage::rust::dag::block_dag_key_value_storage::InsertMode;
        use models::rust::block_implicits::get_random_block;
        use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
        let mut manager = InMemoryStoreManager::new();
        let storage = BlockDagKeyValueStorage::new_with_hot_window(&mut manager, 2)
            .await
            .unwrap();
        let blocks = KeyValueBlockStore::create_from_kvm(&mut manager)
            .await
            .unwrap();
        let mut block = get_random_block(
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
        );
        for n in 0..=20 {
            block.block_hash = hash(n);
            block.body.state.block_number = i64::from(n);
            block.seq_num = i32::from(n);
            block.header.parents_hash_list = if n == 0 { vec![] } else { vec![hash(n - 1)] };
            blocks.put(block.block_hash.clone(), &block).unwrap();
            drop(
                storage
                    .insert(
                        &block,
                        if n == 0 {
                            InsertMode::Approved
                        } else {
                            InsertMode::Normal
                        },
                    )
                    .unwrap(),
            );
        }
        storage
            .record_directly_finalized(hash(20), 1.0, |_| async { Ok(()) })
            .await
            .unwrap();
        let dag = storage.get_representation().unwrap();
        dag.put_cached_floor(hash(20), hash(20)).unwrap();
        assert!(dag.resident_counts().blocks < 21);
        let conf = super::super::tests::conf();
        let floor = super::super::tests::floor_at(20);
        let moved = super::super::moved_past_by_every_validator(&dag, 0).unwrap();
        let expected: HashSet<_> = (0..=20)
            .map(hash)
            .filter(|hash| {
                super::super::is_safe_to_delete(&dag, hash, &floor, &conf, moved.as_ref()).unwrap()
            })
            .collect();
        drop(dag);
        let mut sweep = GcSweep::new();
        let mut deleted = HashSet::new();
        let mut complete = false;
        let mut walking_yields = 0;
        let mut restarted = false;
        for _ in 0..100 {
            let progress = run_turn(
                &mut sweep,
                &storage,
                &blocks,
                &conf,
                &mut |block| {
                    assert!(
                        deleted.insert(block.block_hash.clone()),
                        "candidate must not be deleted twice"
                    );
                    Ok(true)
                },
                3,
            )
            .unwrap();
            if matches!(sweep.phase, Phase::Anchor(_) | Phase::Walk(_)) {
                walking_yields += 1;
                assert!(
                    deleted.is_empty(),
                    "incomplete proof must never authorize deletion"
                );
            }
            if !restarted && matches!(sweep.phase, Phase::Walk(_)) {
                assert!(storage.gc_store().scratch_bytes().unwrap() > 0);
                sweep = GcSweep::new();
                restarted = true;
            }
            if matches!(progress, GcProgress::Complete { .. }) {
                complete = true;
                break;
            }
        }
        assert!(complete && restarted);
        assert!(walking_yields > 1);
        assert_eq!(deleted, expected);
        assert_eq!(storage.gc_store().scratch_bytes().unwrap(), 0);
        assert!(matches!(sweep.phase, Phase::Idle));
    }
}
