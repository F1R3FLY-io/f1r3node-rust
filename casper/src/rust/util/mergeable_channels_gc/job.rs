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
pub struct GcWorkStats {
    pub parent_edges: u64,
    pub work_records: u64,
    pub turns: u64,
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
    pub fn pending_len(&self) -> usize { self.pending_len }
    pub fn work_stats(&self) -> GcWorkStats { self.work }
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
    sweep.work.turns += 1;
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
    use block_storage::rust::dag::dag_gc_store::DagGcSlots;
    use shared::rust::store::atomic_store::AtomicStoreGroup;
    use shared::rust::store::key_value_store::KeyValueStore;
    use shared::rust::store::memory_key_value_store::MemoryEnvironment;

    use super::*;

    fn hash(n: u8) -> BlockHash { BlockHash::from(vec![n; 32]) }

    fn store() -> DagGcStore {
        let env = Arc::new(MemoryEnvironment::new());
        let tables: Vec<Arc<dyn KeyValueStore>> = (0..4)
            .map(|_| Arc::new(env.create_store()) as Arc<dyn KeyValueStore>)
            .collect();
        DagGcStore::new(
            Arc::new(AtomicStoreGroup::new(tables).unwrap()),
            DagGcSlots {
                state: 0,
                retry: 1,
                work: 2,
                visited: 3,
            },
        )
    }

    fn job(store: &DagGcStore) -> Job {
        Job {
            pass: Pass {
                dag: super::super::tests::linear_chain_dag(),
                floor: super::super::tests::floor_at(20),
                conf: super::super::tests::conf(),
                target: 16,
                discovery_start: 0,
                discovery_cursor: None,
                retry_cursor: None,
            },
            id: store.next_job().unwrap(),
            candidates: vec![hash(0), hash(1)],
            next: None,
            minimum: 0,
            frontier: vec![hash(19)],
            frontier_seen: vec![None],
            advance: Vec::new(),
            deletion_position: 0,
            cleanup_position: 0,
        }
    }

    #[test]
    fn ancestry_resumes_small_turns_and_matches_the_unbounded_reference() {
        let store = store();
        let mut job = job(&store);
        assert!(advance_anchor(&mut job, &store, &mut 2).unwrap());
        assert!(!advance_ancestry(&job, &store, &mut 3, &mut GcWorkStats::default()).unwrap());
        assert!(!store.visited(job.id, &hash(0)).unwrap());
        let mut turns = 1;
        while !advance_ancestry(&job, &store, &mut 3, &mut GcWorkStats::default()).unwrap() {
            turns += 1;
            assert!(turns < 40);
        }
        assert!(turns > 1);
        let expected = super::super::moved_past_by_every_validator(&job.pass.dag, 0)
            .unwrap()
            .unwrap();
        for n in 0..=20 {
            assert_eq!(
                store.visited(job.id, &hash(n)).unwrap(),
                expected.contains(&hash(n))
            );
        }
        assert!(store.next_work(job.id).unwrap().is_none());
    }

    #[test]
    fn malformed_unknown_parents_do_not_escape_scratch_cleanup() {
        let store = store();
        let mut job = job(&store);
        let mut metadata = job.pass.dag.lookup_unsafe(&hash(19)).unwrap();
        metadata.parents.extend([
            BlockHash::from(Vec::new()),
            BlockHash::from(vec![0]),
            BlockHash::from(vec![0; 1_024]),
            hash(250),
        ]);
        job.pass
            .dag
            .block_metadata_index
            .write()
            .add(metadata)
            .unwrap();
        assert!(advance_anchor(&mut job, &store, &mut 2).unwrap());
        let mut stats = GcWorkStats::default();
        let mut turns = 0;
        while !advance_ancestry(&job, &store, &mut 3, &mut stats).unwrap() {
            turns += 1;
            assert!(turns < 50);
        }
        assert!(store.visited(job.id, &hash(250)).unwrap());
        assert!(store.visited(job.id, &hash(0)).unwrap());
        assert_eq!(stats.parent_edges, 23);
        while !store.cleanup_page(Some(job.id)).unwrap() {}
        assert_eq!(store.scratch_bytes().unwrap(), 0);
    }

    #[test]
    fn malformed_anchor_abstains_without_writing_scratch() {
        for parent in [Vec::new(), vec![0], vec![0; 1_024]] {
            let store = store();
            let mut job = job(&store);
            job.frontier = vec![BlockHash::from(parent.clone())];
            assert!(advance_anchor(&mut job, &store, &mut 2).unwrap());
            assert!(job.frontier.is_empty());
            assert_eq!(store.scratch_bytes().unwrap(), 0);
            job.frontier = vec![hash(19), hash(8)];
            job.pass
                .dag
                .fixture_mut()
                .main_parent_map
                .insert(hash(19), BlockHash::from(parent));
            assert!(advance_anchor(&mut job, &store, &mut 2).unwrap());
            assert!(job.frontier.is_empty());
            assert_eq!(store.scratch_bytes().unwrap(), 0);
        }
    }

    #[test]
    fn anchor_frontier_resumes_without_restarting_the_walk() {
        let store = store();
        let mut job = job(&store);
        job.frontier = vec![hash(19), hash(8)];
        assert!(!advance_anchor(&mut job, &store, &mut 2).unwrap());
        assert_eq!(job.frontier[0], hash(17));
        let mut turns = 1;
        while !advance_anchor(&mut job, &store, &mut 2).unwrap() {
            turns += 1;
            assert!(turns < 20);
        }
        assert_eq!(job.frontier, vec![hash(8), hash(8)]);
        assert!(store.visited(job.id, &hash(8)).unwrap());
        assert!(!store.visited(job.id, &hash(19)).unwrap());
    }

    #[test]
    fn anchor_allows_non_monotonic_acyclic_parent_edges() {
        let store = store();
        let mut job = job(&store);
        job.frontier = vec![hash(19), hash(8)];
        job.pass
            .dag
            .fixture_mut()
            .main_parent_map
            .insert(hash(18), hash(20));
        job.pass
            .dag
            .fixture_mut()
            .main_parent_map
            .insert(hash(20), hash(17));
        let mut turns = 0;
        while !advance_anchor(&mut job, &store, &mut 2).unwrap() {
            turns += 1;
            assert!(turns < 20);
        }
        assert_eq!(job.frontier, vec![hash(8), hash(8)]);
        assert!(store.visited(job.id, &hash(8)).unwrap());
    }

    #[tokio::test]
    async fn anchor_cycle_abandons_the_view_and_reclaims_seen_rows_without_losing_retry() {
        use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
        let mut manager = InMemoryStoreManager::new();
        let storage = BlockDagKeyValueStorage::new(&mut manager).await.unwrap();
        let blocks = KeyValueBlockStore::create_from_kvm(&mut manager)
            .await
            .unwrap();
        let store = storage.gc_store();
        store.reserve_discovery(16).unwrap();
        store
            .commit_discovery_page(&[(hash(1), 1)], None, Some(16))
            .unwrap();
        let mut job = job(&store);
        job.frontier = vec![hash(19), hash(8)];
        job.pass
            .dag
            .fixture_mut()
            .main_parent_map
            .insert(hash(18), hash(19));
        let captured_view = Arc::downgrade(&job.pass.dag.block_metadata_index);
        let mut sweep = GcSweep {
            phase: Phase::Anchor(job),
            pending_len: 1,
            work: GcWorkStats::default(),
        };
        let conf = super::super::tests::conf();
        let error = collect_garbage_with(&mut sweep, &storage, &blocks, &conf, &mut |_| {
            panic!("cyclic proof must not delete")
        })
        .unwrap_err();
        assert!(
            matches!(error, KvStoreError::InvalidArgument(message) if message.contains("main-parent cycle"))
        );
        assert!(
            matches!(sweep.phase, Phase::Recover),
            "no phase retaining a captured view may survive the cycle error"
        );
        assert!(
            captured_view.upgrade().is_none(),
            "the failed job must release its captured view"
        );
        assert_eq!(sweep.pending_len(), 0);
        assert!(store.scratch_bytes().unwrap() > 0);
        assert_eq!(
            collect_garbage_with(&mut sweep, &storage, &blocks, &conf, &mut |_| panic!(
                "recovery must not delete"
            ))
            .unwrap(),
            GcProgress::Yielded { deleted: 0 }
        );
        assert!(matches!(sweep.phase, Phase::Idle));
        assert_eq!(store.scratch_bytes().unwrap(), 0);
        assert_eq!(store.retry_page(None).unwrap().rows, vec![(hash(1), 1)]);
    }

    #[test]
    fn missing_held_metadata_fails_without_completing_ancestry() {
        let store = store();
        let mut job = job(&store);
        assert!(advance_anchor(&mut job, &store, &mut 2).unwrap());
        job.pass
            .dag
            .block_metadata_index
            .read()
            .delete_kv_row_for_tests(&hash(19))
            .unwrap();
        assert!(advance_ancestry(&job, &store, &mut 10, &mut GcWorkStats::default()).is_err());
        assert_eq!(store.next_work(job.id).unwrap().unwrap().hash, hash(19));
        assert!(!store.visited(job.id, &hash(0)).unwrap());
    }

    #[test]
    fn abandoned_scratch_never_supplies_the_next_jobs_proof() {
        let store = store();
        let mut old = job(&store);
        assert!(advance_anchor(&mut old, &store, &mut 2).unwrap());
        assert!(!advance_ancestry(&old, &store, &mut 3, &mut GcWorkStats::default()).unwrap());
        while !store.cleanup_page(None).unwrap() {}
        let next = job(&store);
        assert_ne!(old.id, next.id);
        assert!(store.next_work(next.id).unwrap().is_none());
        assert!(!store.visited(next.id, &hash(19)).unwrap());
    }

    #[test]
    fn missing_floor_defers_without_derivation_and_missing_cached_target_errors() {
        let dag = super::super::tests::linear_chain_dag();
        assert!(cached_floor(&dag).unwrap().is_none());
        assert!(dag.get_cached_floor(&hash(20)).unwrap().is_none());
        dag.put_cached_floor(hash(20), hash(18)).unwrap();
        assert_eq!(
            cached_floor(&dag).unwrap(),
            Some(Floor {
                hash: hash(18),
                block_number: 18
            })
        );
        let another = super::super::tests::linear_chain_dag();
        another.put_cached_floor(hash(20), hash(99)).unwrap();
        assert!(cached_floor(&another).is_err());
    }

    fn shuffled_hash(index: u32) -> BlockHash {
        let mut bytes = vec![0; 32];
        bytes[28..].copy_from_slice(&index.wrapping_mul(2_654_435_761).to_be_bytes());
        bytes.into()
    }

    async fn published_chain(
        tip: u32,
        side: bool,
    ) -> (BlockDagKeyValueStorage, KeyValueBlockStore) {
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
        for n in 0..=tip {
            if side && n == 25 {
                let mut sibling = block.clone();
                sibling.block_hash = shuffled_hash(u32::MAX);
                sibling.body.state.block_number = 1;
                sibling.seq_num = 1;
                sibling.header.parents_hash_list = vec![shuffled_hash(0)];
                blocks.put(sibling.block_hash.clone(), &sibling).unwrap();
                drop(storage.insert(&sibling, InsertMode::Normal).unwrap());
            }
            block.block_hash = shuffled_hash(n);
            block.body.state.block_number = i64::from(n);
            block.seq_num = n as i32;
            block.header.parents_hash_list = if n == 0 {
                vec![]
            } else {
                vec![shuffled_hash(n - 1)]
            };
            if side && n == 25 {
                block.header.parents_hash_list.push(shuffled_hash(u32::MAX));
            }
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
        let finalized = if side { 20 } else { tip };
        storage
            .record_directly_finalized(shuffled_hash(finalized), 1.0, |_| async { Ok(()) })
            .await
            .unwrap();
        storage
            .get_representation()
            .unwrap()
            .put_cached_floor(shuffled_hash(finalized), shuffled_hash(finalized))
            .unwrap();
        (storage, blocks)
    }

    #[tokio::test]
    async fn random_order_backlog_reuses_one_proof_across_candidate_pages() {
        let tip = 2_300;
        let (storage, blocks) = published_chain(tip, false).await;
        let dag = storage.get_representation().unwrap();
        let conf = super::super::tests::conf();
        let floor = Floor {
            hash: shuffled_hash(tip),
            block_number: i64::from(tip),
        };
        let moved = super::super::moved_past_by_every_validator(&dag, 0).unwrap();
        let expected: HashSet<_> = (0..=tip)
            .map(shuffled_hash)
            .filter(|hash| {
                super::super::is_safe_to_delete(&dag, hash, &floor, &conf, moved.as_ref()).unwrap()
            })
            .collect();
        assert!(expected.len() > 2 * GC_PAGE_SIZE);
        drop(dag);
        let mut sweep = GcSweep::new();
        let mut deleted = HashSet::new();
        let mut complete = false;
        for _ in 0..100 {
            let progress = run_turn(
                &mut sweep,
                &storage,
                &blocks,
                &conf,
                &mut |block| {
                    assert!(deleted.insert(block.block_hash.clone()));
                    Ok(true)
                },
                TURN_BUDGET,
            )
            .unwrap();
            if matches!(progress, GcProgress::Complete { .. }) {
                complete = true;
                break;
            }
        }
        assert!(complete);
        assert_eq!(deleted, expected);
        assert!(
            sweep.work.work_records <= u64::from(tip) + 1,
            "candidate pages must reuse one ancestry proof"
        );
        assert!(sweep.work.parent_edges <= u64::from(tip) + 1);
        assert_eq!(storage.gc_store().scratch_bytes().unwrap(), 0);
    }

    #[tokio::test]
    async fn newly_finalized_candidate_below_prepass_minimum_waits_for_next_cycle() {
        let (storage, blocks) = published_chain(30, true).await;
        let store = storage.gc_store();
        let side = shuffled_hash(u32::MAX);
        assert!(!storage
            .get_representation()
            .unwrap()
            .is_finalized(&side)
            .unwrap());
        store.reserve_discovery(16).unwrap();
        store
            .commit_discovery_page(&[(side.clone(), 1), (shuffled_hash(5), 5)], None, Some(16))
            .unwrap();
        let conf = super::super::tests::conf();
        let mut sweep = GcSweep::new();
        let mut deleted = HashSet::new();
        let mut finalized = false;
        let mut complete = false;
        for _ in 0..100 {
            let progress = run_turn(
                &mut sweep,
                &storage,
                &blocks,
                &conf,
                &mut |block| {
                    deleted.insert(block.block_hash.clone());
                    Ok(true)
                },
                3,
            )
            .unwrap();
            if !finalized && matches!(&sweep.phase, Phase::Walk(job) if job.minimum == 5) {
                storage
                    .record_directly_finalized(side.clone(), 1.0, |_| async { Ok(()) })
                    .await
                    .unwrap();
                finalized = true;
            }
            if matches!(progress, GcProgress::Complete { .. }) {
                complete = true;
                break;
            }
        }
        assert!(complete && finalized);
        assert_eq!(deleted, HashSet::from([shuffled_hash(5)]));
        assert_eq!(store.retry_page(None).unwrap().rows, vec![(
            side.clone(),
            1
        )]);
        complete = false;
        for _ in 0..100 {
            let progress = run_turn(
                &mut sweep,
                &storage,
                &blocks,
                &conf,
                &mut |block| {
                    deleted.insert(block.block_hash.clone());
                    Ok(true)
                },
                3,
            )
            .unwrap();
            if matches!(progress, GcProgress::Complete { .. }) {
                complete = true;
                break;
            }
        }
        assert!(complete);
        assert!(deleted.contains(&side));
        assert!(store.retry_page(None).unwrap().rows.is_empty());
    }

    #[tokio::test]
    async fn full_machine_yields_before_deletion_and_matches_reference_on_cold_history() {
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
            if matches!(progress, GcProgress::Complete { .. }) {
                complete = true;
                break;
            }
        }
        assert!(complete);
        assert!(walking_yields > 1);
        assert_eq!(deleted, expected);
        assert!(storage.gc_store().cleanup_page(None).unwrap());
        assert!(matches!(sweep.phase, Phase::Idle));
    }
}
