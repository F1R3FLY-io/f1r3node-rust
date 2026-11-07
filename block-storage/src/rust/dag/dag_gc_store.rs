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
