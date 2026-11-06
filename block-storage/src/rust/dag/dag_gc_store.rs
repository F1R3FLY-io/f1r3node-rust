use std::sync::Arc;

use models::rust::block_hash::BlockHash;
use serde::{Deserialize, Serialize};
use shared::rust::store::atomic_store::{AtomicStoreGroup, AtomicTransaction};
use shared::rust::store::key_value_store::KvStoreError;

pub const GC_RETRY_TABLE: &str = "dag-gc-retry-v1";
pub const GC_WORK_TABLE: &str = "dag-gc-work-v1";
pub const GC_VISITED_TABLE: &str = "dag-gc-visited-v1";
pub const GC_PAGE_SIZE: usize = 1_024;

const DISCOVERY_KEY: &[u8] = b"gc.discovery";
const RETRY_CURSOR_KEY: &[u8] = b"gc.retry-cursor";
const NEXT_JOB_KEY: &[u8] = b"gc.next-job";

#[derive(Clone, Copy)]
pub struct DagGcSlots {
    pub state: usize,
    pub retry: usize,
    pub work: usize,
    pub visited: usize,
}

#[derive(Clone)]
pub struct DagGcStore {
    group: Arc<AtomicStoreGroup>,
    slots: DagGcSlots,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct DiscoveryState {
    pub claimed_ceiling: Option<i64>,
    pub completed_ceiling: Option<i64>,
    pub cursor: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetryPage {
    pub rows: Vec<(BlockHash, i64)>,
    pub next: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkEntry {
    pub hash: BlockHash,
    pub next_parent: usize,
}

fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, KvStoreError> {
    Ok(bincode::serialize(value)?)
}

fn decode<T: for<'de> Deserialize<'de>>(value: &[u8]) -> Result<T, KvStoreError> {
    Ok(bincode::deserialize(value)?)
}

fn scratch_key(job: u64, hash: &BlockHash) -> Result<Vec<u8>, KvStoreError> {
    if hash.len() != 32 {
        return Err(KvStoreError::InvalidArgument(
            "GC scratch hash must contain 32 bytes".into(),
        ));
    }
    let mut key = job.to_be_bytes().to_vec();
    key.extend_from_slice(hash);
    Ok(key)
}

fn scratch_bounds(job: u64) -> (Vec<u8>, Vec<u8>) {
    let prefix = job.to_be_bytes().to_vec();
    let mut lower = prefix.clone();
    lower.extend_from_slice(&[0; 32]);
    let mut upper = prefix;
    upper.extend_from_slice(&[u8::MAX; 32]);
    (lower, upper)
}

impl DagGcStore {
    pub fn new(group: Arc<AtomicStoreGroup>, slots: DagGcSlots) -> Self { Self { group, slots } }

    #[cfg(any(test, feature = "test-internals"))]
    pub fn scratch_bytes(&self) -> Result<u64, KvStoreError> {
        let mut bytes = 0u64;
        for slot in [self.slots.work, self.slots.visited] {
            let mut after = None;
            loop {
                let page =
                    self.group
                        .scan_page(slot, None, None, after.as_deref(), GC_PAGE_SIZE)?;
                for (key, value) in page.rows {
                    bytes += (key.len() + value.len()) as u64;
                }
                after = page.next;
                if after.is_none() {
                    break;
                }
            }
        }
        Ok(bytes)
    }

    pub fn enqueue_if_claimed(
        tx: &mut AtomicTransaction<'_>,
        slots: DagGcSlots,
        hash: &BlockHash,
        height: i64,
    ) -> Result<(), KvStoreError> {
        let state: DiscoveryState = tx
            .get(slots.state, DISCOVERY_KEY)?
            .map(|bytes| decode(&bytes))
            .transpose()?
            .unwrap_or_default();
        if state
            .claimed_ceiling
            .is_some_and(|ceiling| height <= ceiling)
        {
            tx.put(slots.retry, hash.to_vec(), encode(&height)?)?;
        }
        Ok(())
    }

    pub fn discovery_state(&self) -> Result<DiscoveryState, KvStoreError> {
        self.group
            .get(self.slots.state, DISCOVERY_KEY)?
            .map(|bytes| decode(&bytes))
            .transpose()
            .map(|state| state.unwrap_or_default())
    }

    pub fn reserve_discovery(&self, target: i64) -> Result<DiscoveryState, KvStoreError> {
        self.group.transaction(|tx| {
            let mut state: DiscoveryState = tx
                .get(self.slots.state, DISCOVERY_KEY)?
                .map(|bytes| decode(&bytes))
                .transpose()?
                .unwrap_or_default();
            state.claimed_ceiling = Some(state.claimed_ceiling.map_or(target, |h| h.max(target)));
            tx.put(self.slots.state, DISCOVERY_KEY.to_vec(), encode(&state)?)?;
            Ok(state)
        })
    }

    pub fn commit_discovery_page(
        &self,
        rows: &[(BlockHash, i64)],
        cursor: Option<Vec<u8>>,
        completed_ceiling: Option<i64>,
    ) -> Result<(), KvStoreError> {
        if rows.len() > GC_PAGE_SIZE {
            return Err(KvStoreError::InvalidArgument(
                "GC discovery page exceeds its bound".into(),
            ));
        }
        self.group.transaction(|tx| {
            let mut state: DiscoveryState = tx
                .get(self.slots.state, DISCOVERY_KEY)?
                .map(|bytes| decode(&bytes))
                .transpose()?
                .unwrap_or_default();
            if completed_ceiling
                .is_some_and(|h| state.claimed_ceiling.is_none_or(|claim| h > claim))
            {
                return Err(KvStoreError::InvalidArgument(
                    "GC completion exceeds its claim".into(),
                ));
            }
            for (hash, height) in rows {
                tx.put(self.slots.retry, hash.to_vec(), encode(height)?)?;
            }
            state.cursor = cursor.clone();
            if let Some(ceiling) = completed_ceiling {
                state.completed_ceiling =
                    Some(state.completed_ceiling.map_or(ceiling, |h| h.max(ceiling)));
                state.cursor = None;
            }
            tx.put(self.slots.state, DISCOVERY_KEY.to_vec(), encode(&state)?)
        })
    }

    pub fn retry_cursor(&self) -> Result<Option<Vec<u8>>, KvStoreError> {
        self.group.get(self.slots.state, RETRY_CURSOR_KEY)
    }

    pub fn retry_page(&self, after: Option<&[u8]>) -> Result<RetryPage, KvStoreError> {
        let page = self
            .group
            .scan_page(self.slots.retry, None, None, after, GC_PAGE_SIZE)?;
        Ok(RetryPage {
            rows: page
                .rows
                .into_iter()
                .map(|(hash, height)| Ok((BlockHash::from(hash), decode(&height)?)))
                .collect::<Result<_, KvStoreError>>()?,
            next: page.next,
        })
    }

    pub fn finish_retry_page(&self, next: Option<&[u8]>) -> Result<(), KvStoreError> {
        self.group.transaction(|tx| {
            match next {
                Some(cursor) => {
                    tx.put(self.slots.state, RETRY_CURSOR_KEY.to_vec(), cursor.to_vec())?
                }
                None => {
                    tx.delete(self.slots.state, RETRY_CURSOR_KEY)?;
                }
            }
            Ok(())
        })
    }

    pub fn remove_retry(&self, hash: &BlockHash) -> Result<(), KvStoreError> {
        self.group.transaction(|tx| {
            tx.delete(self.slots.retry, hash)?;
            Ok(())
        })
    }

    pub fn next_job(&self) -> Result<u64, KvStoreError> {
        self.group.transaction(|tx| {
            let previous: u64 = tx
                .get(self.slots.state, NEXT_JOB_KEY)?
                .map(|bytes| decode(&bytes))
                .transpose()?
                .unwrap_or(0);
            let next = previous
                .checked_add(1)
                .ok_or_else(|| KvStoreError::InvalidArgument("GC job sequence exhausted".into()))?;
            tx.put(self.slots.state, NEXT_JOB_KEY.to_vec(), encode(&next)?)?;
            Ok(next)
        })
    }

    pub fn seed_work(&self, job: u64, hash: &BlockHash, expand: bool) -> Result<(), KvStoreError> {
        self.group.transaction(|tx| {
            let key = scratch_key(job, hash)?;
            tx.put(self.slots.visited, key.clone(), Vec::new())?;
            if expand {
                tx.put(self.slots.work, key, encode(&0u64)?)?;
            }
            Ok(())
        })
    }

    pub fn next_work(&self, job: u64) -> Result<Option<WorkEntry>, KvStoreError> {
        let (lower, upper) = scratch_bounds(job);
        let page = self
            .group
            .scan_page(self.slots.work, Some(&lower), Some(&upper), None, 1)?;
        page.rows
            .into_iter()
            .next()
            .map(|(key, value)| {
                if key.len() != 40 {
                    return Err(KvStoreError::SerializationError(
                        "Invalid GC work key length".into(),
                    ));
                }
                let offset: u64 = decode(&value)?;
                let next_parent = usize::try_from(offset).map_err(|_| {
                    KvStoreError::SerializationError("GC parent offset exceeds usize".into())
                })?;
                Ok(WorkEntry {
                    hash: BlockHash::copy_from_slice(&key[8..]),
                    next_parent,
                })
            })
            .transpose()
    }

    pub fn advance_work(
        &self,
        job: u64,
        current: &BlockHash,
        next_parent: Option<usize>,
        children: &[(BlockHash, bool)],
    ) -> Result<(), KvStoreError> {
        if children.len() > GC_PAGE_SIZE {
            return Err(KvStoreError::InvalidArgument(
                "GC work page exceeds its bound".into(),
            ));
        }
        self.group.transaction(|tx| {
            for (hash, expand) in children {
                let key = scratch_key(job, hash)?;
                if tx.get(self.slots.visited, &key)?.is_none() {
                    tx.put(self.slots.visited, key.clone(), Vec::new())?;
                    if *expand {
                        tx.put(self.slots.work, key, encode(&0u64)?)?;
                    }
                }
            }
            let current_key = scratch_key(job, current)?;
            match next_parent {
                Some(offset) => tx.put(self.slots.work, current_key, encode(&(offset as u64))?)?,
                None => {
                    tx.delete(self.slots.work, &current_key)?;
                }
            }
            Ok(())
        })
    }

    pub fn visited(&self, job: u64, hash: &BlockHash) -> Result<bool, KvStoreError> {
        Ok(self
            .group
            .get(self.slots.visited, &scratch_key(job, hash)?)?
            .is_some())
    }

    pub fn cleanup_page(&self, job: Option<u64>) -> Result<bool, KvStoreError> {
        let bounds = job.map(scratch_bounds);
        self.group.transaction(|tx| {
            let mut remaining = GC_PAGE_SIZE;
            for slot in [self.slots.work, self.slots.visited] {
                if remaining == 0 {
                    return Ok(false);
                }
                let page = tx.scan_page(
                    slot,
                    bounds.as_ref().map(|(lower, _)| lower.as_slice()),
                    bounds.as_ref().map(|(_, upper)| upper.as_slice()),
                    None,
                    remaining,
                )?;
                remaining -= page.rows.len();
                for (key, _) in page.rows {
                    tx.delete(slot, &key)?;
                }
                if page.next.is_some() {
                    return Ok(false);
                }
            }
            Ok(true)
        })
    }
}

#[cfg(test)]
mod tests {
    use shared::rust::store::key_value_store::KeyValueStore;
    use shared::rust::store::memory_key_value_store::MemoryEnvironment;

    use super::*;

    fn setup() -> (Arc<AtomicStoreGroup>, DagGcSlots, DagGcStore) {
        let env = Arc::new(MemoryEnvironment::new());
        let tables: Vec<Arc<dyn KeyValueStore>> = (0..4)
            .map(|_| Arc::new(env.create_store()) as Arc<dyn KeyValueStore>)
            .collect();
        let group = Arc::new(AtomicStoreGroup::new(tables).unwrap());
        let slots = DagGcSlots {
            state: 0,
            retry: 1,
            work: 2,
            visited: 3,
        };
        let store = DagGcStore::new(group.clone(), slots);
        (group, slots, store)
    }

    fn hash(value: u64) -> BlockHash {
        let mut bytes = vec![0; 32];
        bytes[24..].copy_from_slice(&value.to_be_bytes());
        BlockHash::from(bytes)
    }

    #[test]
    fn publication_uses_reserved_ceiling_before_discovery_completes() {
        let (group, slots, store) = setup();
        store.reserve_discovery(100).unwrap();
        store.commit_discovery_page(&[], None, Some(100)).unwrap();
        store.reserve_discovery(200).unwrap();
        group
            .transaction(|tx| {
                for height in [99, 150, 200, 201] {
                    DagGcStore::enqueue_if_claimed(tx, slots, &hash(height as u64), height)?;
                }
                Ok(())
            })
            .unwrap();
        assert_eq!(
            store.discovery_state().unwrap().completed_ceiling,
            Some(100)
        );
        assert_eq!(store.retry_page(None).unwrap().rows, vec![
            (hash(99), 99),
            (hash(150), 150),
            (hash(200), 200)
        ]);
        store
            .commit_discovery_page(&[(hash(150), 150)], None, Some(200))
            .unwrap();
        assert_eq!(store.retry_page(None).unwrap().rows.len(), 3);
    }

    #[test]
    fn reservation_and_cursor_survive_reopen_and_never_regress_claim() {
        let (group, slots, store) = setup();
        store.reserve_discovery(200).unwrap();
        store
            .commit_discovery_page(&[(hash(100), 100)], Some(vec![7; 40]), None)
            .unwrap();
        let reopened = DagGcStore::new(group, slots);
        let state = reopened.reserve_discovery(150).unwrap();
        assert_eq!(state.claimed_ceiling, Some(200));
        assert_eq!(state.cursor, Some(vec![7; 40]));
        assert_eq!(state.completed_ceiling, None);
        assert_eq!(reopened.retry_page(None).unwrap().rows, vec![(
            hash(100),
            100
        )]);
        assert!(reopened
            .commit_discovery_page(&[], None, Some(201))
            .is_err());
    }

    #[test]
    fn failed_publication_does_not_enqueue_an_unpublished_candidate() {
        let (group, slots, store) = setup();
        store.reserve_discovery(200).unwrap();
        let result: Result<(), KvStoreError> = group.transaction(|tx| {
            DagGcStore::enqueue_if_claimed(tx, slots, &hash(150), 150)?;
            Err(KvStoreError::IoError("injected publication failure".into()))
        });
        assert!(result.is_err());
        assert!(store.retry_page(None).unwrap().rows.is_empty());
    }

    #[test]
    fn retry_pages_bound_heap_and_do_not_skip_after_candidate_deletion() {
        let (_, _, store) = setup();
        store.reserve_discovery(2_000).unwrap();
        let first: Vec<_> = (0..GC_PAGE_SIZE as u64)
            .map(|n| (hash(n), n as i64))
            .collect();
        store
            .commit_discovery_page(&first, Some(vec![1; 40]), None)
            .unwrap();
        store
            .commit_discovery_page(&[(hash(1_024), 1_024)], None, Some(2_000))
            .unwrap();
        let page = store.retry_page(None).unwrap();
        assert_eq!(page.rows.len(), GC_PAGE_SIZE);
        assert!(page.next.is_some());
        for (hash, _) in page.rows {
            store.remove_retry(&hash).unwrap();
        }
        store.finish_retry_page(page.next.as_deref()).unwrap();
        let next = store
            .retry_page(store.retry_cursor().unwrap().as_deref())
            .unwrap();
        assert_eq!(next.rows, vec![(hash(1_024), 1_024)]);
        assert!(next.next.is_none());
    }

    #[test]
    fn malformed_scratch_hashes_are_rejected_atomically() {
        let (group, slots, store) = setup();
        let job = store.next_job().unwrap();
        for malformed in [Vec::new(), vec![0], vec![0; 1_024]] {
            let malformed = BlockHash::from(malformed);
            assert!(matches!(
                store.seed_work(job, &malformed, true),
                Err(KvStoreError::InvalidArgument(_))
            ));
            assert!(store
                .advance_work(job, &hash(1), None, &[
                    (hash(2), true),
                    (malformed.clone(), true)
                ])
                .is_err());
            assert!(store
                .advance_work(job, &malformed, None, &[(hash(2), true)])
                .is_err());
            for slot in [slots.work, slots.visited] {
                assert!(group
                    .scan_page(slot, None, None, None, 1)
                    .unwrap()
                    .rows
                    .is_empty());
            }
        }
    }

    #[test]
    fn ancestry_work_preserves_offsets_deduplicates_and_isolates_jobs() {
        let (_, _, store) = setup();
        let job = store.next_job().unwrap();
        let other = store.next_job().unwrap();
        store.seed_work(job, &hash(10), true).unwrap();
        store.seed_work(other, &hash(20), true).unwrap();
        store
            .advance_work(job, &hash(10), Some(2), &[
                (hash(8), false),
                (hash(9), true),
            ])
            .unwrap();
        assert!(store.visited(job, &hash(8)).unwrap());
        assert_eq!(store.next_work(job).unwrap().unwrap().hash, hash(9));
        store
            .advance_work(job, &hash(9), None, &[(hash(10), true)])
            .unwrap();
        assert_eq!(
            store.next_work(job).unwrap(),
            Some(WorkEntry {
                hash: hash(10),
                next_parent: 2
            })
        );
        store.advance_work(job, &hash(10), None, &[]).unwrap();
        assert!(store.next_work(job).unwrap().is_none());
        assert!(store.cleanup_page(Some(job)).unwrap());
        assert!(!store.visited(job, &hash(10)).unwrap());
        assert_eq!(store.next_work(other).unwrap().unwrap().hash, hash(20));
        assert!(store.cleanup_page(None).unwrap());
        assert!(store.next_work(other).unwrap().is_none());
    }

    #[test]
    fn cleanup_is_bounded_and_can_resume_after_reopen() {
        let (group, slots, store) = setup();
        let job = store.next_job().unwrap();
        store.seed_work(job, &hash(2_000), true).unwrap();
        let reached: Vec<_> = (0..GC_PAGE_SIZE as u64).map(|n| (hash(n), true)).collect();
        store
            .advance_work(job, &hash(2_000), None, &reached)
            .unwrap();
        assert!(!store.cleanup_page(None).unwrap());
        let reopened = DagGcStore::new(group, slots);
        let mut turns = 0;
        while !reopened.cleanup_page(None).unwrap() {
            turns += 1;
            assert!(turns < 4);
        }
        assert!(reopened.next_work(job).unwrap().is_none());
        assert!(!reopened.visited(job, &hash(0)).unwrap());
        assert!(reopened.next_job().unwrap() > job);
    }

    fn lmdb_store(path: &std::path::Path) -> DagGcStore {
        use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
        use rspace_plus_plus::rspace::shared::lmdb_dir_store_manager::{
            Db, LmdbDirStoreManager, LmdbEnvConfig,
        };
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let names = ["state", "retry", "work", "visited"];
            let mapping = names
                .iter()
                .map(|name| {
                    (
                        Db::new(name.to_string(), None),
                        LmdbEnvConfig::new("gc".into(), 64 * 1024 * 1024),
                    )
                })
                .collect();
            let mut manager = LmdbDirStoreManager::new(path.to_path_buf(), mapping);
            let mut stores = Vec::new();
            for name in names {
                stores.push(manager.store(name.to_string()).await.unwrap());
            }
            DagGcStore::new(
                Arc::new(AtomicStoreGroup::new(stores).unwrap()),
                DagGcSlots {
                    state: 0,
                    retry: 1,
                    work: 2,
                    visited: 3,
                },
            )
        })
    }

    #[test]
    fn lmdb_restart_preserves_retries_but_scratch_can_be_discarded_in_isolation() {
        let directory = tempfile::tempdir().unwrap();
        let store = lmdb_store(directory.path());
        store.reserve_discovery(10).unwrap();
        store
            .commit_discovery_page(&[(hash(1), 1)], Some(vec![8; 40]), None)
            .unwrap();
        let old_job = store.next_job().unwrap();
        let other_job = store.next_job().unwrap();
        store.seed_work(old_job, &hash(1), true).unwrap();
        store.seed_work(other_job, &hash(2), true).unwrap();
        assert_eq!(store.next_work(old_job).unwrap().unwrap().hash, hash(1));
        assert_eq!(store.next_work(other_job).unwrap().unwrap().hash, hash(2));
        drop(store);
        let reopened = lmdb_store(directory.path());
        assert_eq!(
            reopened.discovery_state().unwrap().cursor,
            Some(vec![8; 40])
        );
        assert_eq!(reopened.retry_page(None).unwrap().rows, vec![(hash(1), 1)]);
        assert!(reopened.cleanup_page(Some(old_job)).unwrap());
        assert!(reopened.next_work(old_job).unwrap().is_none());
        assert_eq!(
            reopened.next_work(other_job).unwrap().unwrap().hash,
            hash(2)
        );
        assert!(reopened.cleanup_page(None).unwrap());
        assert!(!reopened.visited(other_job, &hash(2)).unwrap());
        assert_eq!(reopened.retry_page(None).unwrap().rows, vec![(hash(1), 1)]);
        assert!(reopened.next_job().unwrap() > other_job);
    }
}
