use std::sync::Arc;

use models::rust::block_hash::BlockHash;
use serde::{Deserialize, Serialize};
use shared::rust::store::atomic_store::{AtomicStoreGroup, AtomicTransaction};
use shared::rust::store::key_value_store::KvStoreError;

use super::{decode, encode};

pub const GC_RETRY_TABLE: &str = "dag-gc-retry-v1";
pub const GC_PAGE_SIZE: usize = 1_024;

const DISCOVERY_KEY: &[u8] = b"gc.discovery";
const RETRY_CURSOR_KEY: &[u8] = b"gc.retry-cursor";

#[derive(Clone, Copy)]
pub struct DagGcSlots {
    pub state: usize,
    pub retry: usize,
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

impl DagGcStore {
    pub fn new(group: Arc<AtomicStoreGroup>, slots: DagGcSlots) -> Self { Self { group, slots } }

    fn discovery_in(
        tx: &mut AtomicTransaction<'_>,
        slot: usize,
    ) -> Result<DiscoveryState, KvStoreError> {
        Ok(tx
            .get(slot, DISCOVERY_KEY)?
            .map(|bytes| decode(&bytes))
            .transpose()?
            .unwrap_or_default())
    }

    pub fn enqueue_if_claimed(
        tx: &mut AtomicTransaction<'_>,
        slots: DagGcSlots,
        hash: &BlockHash,
        height: i64,
    ) -> Result<(), KvStoreError> {
        let state = Self::discovery_in(tx, slots.state)?;
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
            let mut state = Self::discovery_in(tx, self.slots.state)?;
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
            let mut state = Self::discovery_in(tx, self.slots.state)?;
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

    pub fn retry_page(
        &self,
        after: Option<&[u8]>,
        limit: usize,
    ) -> Result<RetryPage, KvStoreError> {
        let page = self
            .group
            .scan_page(self.slots.retry, None, None, after, limit)?;
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
}
