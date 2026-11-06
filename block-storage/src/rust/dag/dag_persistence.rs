use std::collections::HashMap;
use std::sync::Arc;

use models::rust::block_hash::{BlockHash, BlockHashSerde};
use models::rust::block_metadata::BlockMetadata;
use models::rust::validator::{Validator, ValidatorSerde};
use serde::{Deserialize, Serialize};
use shared::rust::store::atomic_store::{AtomicStoreGroup, AtomicTransaction};
use shared::rust::store::key_value_store::KvStoreError;

use super::dag_gc_store::{DagGcSlots, DagGcStore};
use super::dag_history_index::{DagHistoryIndex, HistoryTableSlots};

pub(crate) const METADATA: usize = 0;
const LATEST: usize = 1;
const INVALID: usize = 2;
const GENESIS: usize = 3;
const STATE: usize = 7;
const PROGRESS: &[u8] = b"dag.progress";
const GC_SLOTS: DagGcSlots = DagGcSlots {
    state: STATE,
    retry: 8,
    work: 9,
    visited: 10,
};

#[derive(Clone, Default)]
pub(crate) struct DagProgress {
    pub ready: bool,
    pub cursor: Option<Vec<u8>>,
    pub sequence: u64,
    pub max_height: Option<i64>,
    pub lfb: Option<(BlockHash, i64)>,
    pub genesis_candidate: Option<BlockHash>,
    pub total: u64,
    pub edge_rows: u64,
}

impl Serialize for DagProgress {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        (
            self.ready,
            &self.cursor,
            self.sequence,
            self.max_height,
            self.lfb
                .as_ref()
                .map(|(hash, height)| (BlockHashSerde(hash.clone()), *height)),
            self.genesis_candidate
                .as_ref()
                .map(|hash| BlockHashSerde(hash.clone())),
            self.total,
            self.edge_rows,
        )
            .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for DagProgress {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        type Wire = (
            bool,
            Option<Vec<u8>>,
            u64,
            Option<i64>,
            Option<(BlockHashSerde, i64)>,
            Option<BlockHashSerde>,
            u64,
            u64,
        );
        let (ready, cursor, sequence, max_height, lfb, genesis_candidate, total, edge_rows) =
            Wire::deserialize(deserializer)?;
        Ok(Self {
            ready,
            cursor,
            sequence,
            max_height,
            lfb: lfb.map(|(hash, height)| (hash.0, height)),
            genesis_candidate: genesis_candidate.map(|hash| hash.0),
            total,
            edge_rows,
        })
    }
}

#[derive(Clone)]
pub(crate) struct DagPersistence {
    pub membership_store: Arc<dyn shared::rust::store::key_value_store::KeyValueStore>,
    pub group: Arc<AtomicStoreGroup>,
    pub history: Arc<DagHistoryIndex>,
    pub gc: Arc<DagGcStore>,
}

pub(crate) fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, KvStoreError> {
    Ok(bincode::serialize(value)?)
}
pub(crate) fn decode<T: for<'de> Deserialize<'de>>(value: &[u8]) -> Result<T, KvStoreError> {
    Ok(bincode::deserialize(value)?)
}

impl DagPersistence {
    pub fn new(
        group: Arc<AtomicStoreGroup>,
        membership_store: Arc<dyn shared::rust::store::key_value_store::KeyValueStore>,
    ) -> Self {
        Self {
            membership_store,
            history: Arc::new(DagHistoryIndex::new(group.clone(), HistoryTableSlots {
                membership: 4,
                heights: 5,
                children: 6,
                state: STATE,
            })),
            gc: Arc::new(DagGcStore::new(group.clone(), GC_SLOTS)),
            group,
        }
    }

    fn progress_in(tx: &mut AtomicTransaction<'_>) -> Result<DagProgress, KvStoreError> {
        Ok(tx
            .get(STATE, PROGRESS)?
            .map(|bytes| decode(&bytes))
            .transpose()?
            .unwrap_or_default())
    }

    fn genesis_in(tx: &mut AtomicTransaction<'_>, hash: &BlockHash) -> Result<(), KvStoreError> {
        let key = encode(&"genesis".to_string())?;
        if let Some(bytes) = tx.get(GENESIS, &key)? {
            let existing: BlockHashSerde = decode(&bytes)?;
            if existing.0 != *hash {
                return Err(KvStoreError::InvalidArgument(format!(
                    "genesis identity conflicts with {}",
                    hex::encode(hash)
                )));
            }
        } else {
            tx.put(GENESIS, key, encode(&BlockHashSerde(hash.clone()))?)?;
        }
        Ok(())
    }

    pub fn recover(&self) -> Result<DagProgress, KvStoreError> {
        loop {
            let progress = self.group.transaction(|tx| {
                let mut progress = Self::progress_in(tx)?;
                if progress.ready {
                    return Ok(progress);
                }
                let page = tx.scan_page(METADATA, None, None, progress.cursor.as_deref(), 1024)?;
                for (key, bytes) in page.rows {
                    let hash: BlockHashSerde = decode(&key)?;
                    let metadata: BlockMetadata = decode(&bytes)?;
                    if hash.0 != metadata.block_hash || hash.0.len() != 32 {
                        return Err(KvStoreError::InvalidArgument(format!(
                            "invalid legacy metadata key {}",
                            hex::encode(key)
                        )));
                    }
                    self.history.index_block(tx, &metadata, 0)?;
                    if !metadata.invalid {
                        progress.max_height =
                            Some(progress.max_height.map_or(metadata.block_number, |height| {
                                height.max(metadata.block_number)
                            }));
                        if metadata.block_number == 0
                            && progress
                                .genesis_candidate
                                .as_ref()
                                .is_none_or(|old| metadata.block_hash < *old)
                        {
                            progress.genesis_candidate = Some(metadata.block_hash.clone());
                        }
                    }
                    if metadata.directly_finalized
                        && progress.lfb.as_ref().is_none_or(|(hash, height)| {
                            metadata.block_number > *height
                                || (metadata.block_number == *height && metadata.block_hash < *hash)
                        })
                    {
                        progress.lfb = Some((metadata.block_hash.clone(), metadata.block_number));
                    }
                    progress.total = progress.total.checked_add(1).ok_or_else(|| {
                        KvStoreError::InvalidArgument("DAG block count overflow".into())
                    })?;
                }
                progress.cursor = page.next;
                progress.edge_rows = tx.len(6)?;
                if progress.cursor.is_none() {
                    if let Some(hash) = &progress.genesis_candidate {
                        Self::genesis_in(tx, hash)?;
                    }
                    progress.ready = true;
                }
                tx.put(STATE, PROGRESS.to_vec(), encode(&progress)?)?;
                Ok(progress)
            })?;
            if progress.ready {
                if self.group.len(6)? != progress.edge_rows {
                    return Err(KvStoreError::InvalidArgument(
                        "DAG children index cardinality disagrees with committed edge count".into(),
                    ));
                }
                for (slot, name) in [
                    (METADATA, "block-metadata"),
                    (4, "dag-membership-v1"),
                    (5, "dag-height-v1"),
                ] {
                    if self.group.len(slot)? != progress.total {
                        return Err(KvStoreError::InvalidArgument(format!("DAG table {name} cardinality disagrees with committed publication count {}", progress.total)));
                    }
                }
                if let Some((hash, height)) = &progress.lfb {
                    let member = self
                        .history
                        .membership(hash, progress.sequence)?
                        .ok_or_else(|| {
                            KvStoreError::KeyNotFound("persisted DAG LFB membership missing".into())
                        })?;
                    let metadata: BlockMetadata = self
                        .group
                        .get(METADATA, &encode(&BlockHashSerde(hash.clone()))?)?
                        .map(|bytes| decode(&bytes))
                        .transpose()?
                        .ok_or_else(|| {
                            KvStoreError::KeyNotFound("persisted DAG LFB metadata missing".into())
                        })?;
                    if member.height != *height
                        || metadata.block_hash != *hash
                        || metadata.invalid
                        || member.invalid != metadata.invalid
                        || metadata.block_number != *height
                        || !metadata.directly_finalized
                    {
                        return Err(KvStoreError::InvalidArgument(
                            "persisted DAG LFB disagrees with metadata".into(),
                        ));
                    }
                }
                return Ok(progress);
            }
        }
    }

    pub fn publish(
        &self,
        metadata: &BlockMetadata,
        latest: &HashMap<Validator, BlockHash>,
        approved: bool,
    ) -> Result<(BlockMetadata, DagProgress, bool), KvStoreError> {
        self.group.transaction(|tx| {
            let mut progress = Self::progress_in(tx)?;
            if !progress.ready {
                return Err(KvStoreError::InvalidArgument(
                    "DAG migration is incomplete".into(),
                ));
            }
            let existing = self.history.membership_in(tx, &metadata.block_hash)?;
            let expected_hash = metadata.block_hash.clone();
            let key = encode(&BlockHashSerde(metadata.block_hash.clone()))?;
            let mut metadata = if existing.is_some() {
                tx.get(METADATA, &key)?
                    .map(|bytes| decode(&bytes))
                    .transpose()?
                    .ok_or_else(|| KvStoreError::KeyNotFound("published metadata missing".into()))?
            } else {
                metadata.clone()
            };
            if let Some(member) = &existing {
                if metadata.block_hash != expected_hash
                    || metadata.block_number != member.height
                    || metadata.invalid != member.invalid
                {
                    return Err(KvStoreError::InvalidArgument(
                        "published metadata disagrees with membership during duplicate insertion"
                            .into(),
                    ));
                }
            }
            if approved && metadata.invalid {
                return Err(KvStoreError::InvalidArgument(
                    "cannot approve previously published invalid metadata".into(),
                ));
            }
            if approved && metadata.block_number == 0 {
                Self::genesis_in(tx, &metadata.block_hash)?;
            }
            if approved {
                metadata.finalized = true;
                metadata.directly_finalized = true;
                metadata.fault_tolerance_value = metadata.fault_tolerance_value.max(1.0);
                if progress
                    .lfb
                    .as_ref()
                    .is_none_or(|(_, height)| *height <= metadata.block_number)
                {
                    progress.lfb = Some((metadata.block_hash.clone(), metadata.block_number));
                }
            }
            if existing.is_none() {
                progress.sequence = progress.sequence.checked_add(1).ok_or_else(|| {
                    KvStoreError::InvalidArgument("DAG publication sequence overflow".into())
                })?;
                progress.total = progress.total.checked_add(1).ok_or_else(|| {
                    KvStoreError::InvalidArgument("DAG block count overflow".into())
                })?;
                self.history.index_block(tx, &metadata, progress.sequence)?;
                if !metadata.invalid {
                    progress.max_height =
                        Some(progress.max_height.map_or(metadata.block_number, |height| {
                            height.max(metadata.block_number)
                        }));
                }
                if metadata.invalid {
                    tx.put(INVALID, key.clone(), encode(&metadata)?)?;
                }
                for (validator, hash) in latest {
                    tx.put(
                        LATEST,
                        encode(&ValidatorSerde(validator.clone()))?,
                        encode(&BlockHashSerde(hash.clone()))?,
                    )?;
                }
                DagGcStore::enqueue_if_claimed(
                    tx,
                    GC_SLOTS,
                    &metadata.block_hash,
                    metadata.block_number,
                )?;
            }
            tx.put(METADATA, key, encode(&metadata)?)?;
            progress.edge_rows = tx.len(6)?;
            tx.put(STATE, PROGRESS.to_vec(), encode(&progress)?)?;
            Ok((metadata, progress, existing.is_none()))
        })
    }

    pub fn finalize(
        &self,
        directly: &BlockHash,
        indirectly: &std::collections::HashSet<BlockHash>,
        ft: f32,
    ) -> Result<DagProgress, KvStoreError> {
        self.group.transaction(|tx| {
            let mut progress = Self::progress_in(tx)?;
            for hash in indirectly.iter().chain(std::iter::once(directly)) {
                let member = self.history.membership_in(tx, hash)?.ok_or_else(|| {
                    KvStoreError::KeyNotFound("cannot finalize unpublished block".into())
                })?;
                let key = encode(&BlockHashSerde(hash.clone()))?;
                let mut metadata: BlockMetadata = tx
                    .get(METADATA, &key)?
                    .map(|bytes| decode(&bytes))
                    .transpose()?
                    .ok_or_else(|| KvStoreError::KeyNotFound("finality metadata missing".into()))?;
                if metadata.block_hash != *hash
                    || metadata.block_number != member.height
                    || metadata.invalid != member.invalid
                {
                    return Err(KvStoreError::InvalidArgument(
                        "finality metadata disagrees with membership".into(),
                    ));
                }
                metadata.finalized = true;
                metadata.fault_tolerance_value = metadata.fault_tolerance_value.max(ft);
                if hash == directly {
                    metadata.directly_finalized = true;
                    if progress
                        .lfb
                        .as_ref()
                        .is_none_or(|(_, height)| *height <= metadata.block_number)
                    {
                        progress.lfb = Some((hash.clone(), metadata.block_number));
                    }
                }
                tx.put(METADATA, key, encode(&metadata)?)?;
            }
            tx.put(STATE, PROGRESS.to_vec(), encode(&progress)?)?;
            Ok(progress)
        })
    }
}
