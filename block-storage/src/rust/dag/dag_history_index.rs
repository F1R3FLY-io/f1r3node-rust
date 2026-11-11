use std::sync::Arc;
use std::time::Instant;

use crypto::rust::hash::blake2b256::Blake2b256;
use models::rust::block_hash::{BlockHash, LENGTH};
use models::rust::block_metadata::BlockMetadata;
use serde::{Deserialize, Serialize};
use shared::rust::store::atomic_store::{AtomicStoreGroup, AtomicTransaction, StorePage};
use shared::rust::store::key_value_store::KvStoreError;

use super::{decode, encode};

pub const MEMBERSHIP_TABLE: &str = "dag-membership-v1";
pub const HEIGHT_TABLE: &str = "dag-height-v1";
pub const CHILDREN_TABLE: &str = "dag-children-v1";
pub const STATE_TABLE: &str = "dag-history-state-v1";
pub const SEQUENCE_TABLE: &str = "dag-sequence-v1";

#[derive(Clone, Copy, Debug)]
pub struct HistoryTableSlots {
    pub membership: usize,
    pub heights: usize,
    pub children: usize,
    pub sequences: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MembershipRecord {
    pub sequence: u64,
    pub height: i64,
    pub invalid: bool,
}

impl MembershipRecord {
    pub(crate) fn matches(&self, hash: &BlockHash, metadata: &BlockMetadata) -> bool {
        metadata.block_hash == *hash
            && metadata.block_number == self.height
            && metadata.is_invalid() == self.invalid
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryPage<T> {
    pub rows: Vec<T>,
    pub next: Option<Vec<u8>>,
}

#[derive(Clone)]
pub struct DagHistoryIndex {
    group: Arc<AtomicStoreGroup>,
    slots: HistoryTableSlots,
}

struct HistoryQuery {
    operation: &'static str,
    started: Instant,
}

impl HistoryQuery {
    fn new(operation: &'static str) -> Self {
        metrics::counter!("dag.history.query.count", "operation" => operation).increment(1);
        Self {
            operation,
            started: Instant::now(),
        }
    }

    fn visited(&self, rows: usize) {
        metrics::counter!("dag.history.rows.visited", "operation" => self.operation)
            .increment(rows as u64);
    }
}

impl Drop for HistoryQuery {
    fn drop(&mut self) {
        metrics::histogram!("dag.history.query.time", "operation" => self.operation)
            .record(self.started.elapsed().as_secs_f64());
    }
}

fn invalid(message: impl Into<String>) -> KvStoreError {
    KvStoreError::InvalidArgument(message.into())
}

fn validate_hash(hash: &[u8]) -> Result<(), KvStoreError> {
    if hash.len() != LENGTH {
        return Err(invalid(format!(
            "DAG history hash must contain {LENGTH} bytes"
        )));
    }
    Ok(())
}

pub fn encode_height_key(height: i64, hash: &[u8]) -> Result<Vec<u8>, KvStoreError> {
    validate_hash(hash)?;
    let mut key = Vec::with_capacity(8 + LENGTH);
    key.extend_from_slice(&((height as u64) ^ (1 << 63)).to_be_bytes());
    key.extend_from_slice(hash);
    Ok(key)
}

pub fn decode_height_key(key: &[u8]) -> Result<(i64, BlockHash), KvStoreError> {
    if key.len() != 8 + LENGTH {
        return Err(KvStoreError::SerializationError(
            "Invalid DAG height key length".into(),
        ));
    }
    let mut bytes = [0; 8];
    bytes.copy_from_slice(&key[..8]);
    let height = (u64::from_be_bytes(bytes) ^ (1 << 63)) as i64;
    Ok((height, BlockHash::copy_from_slice(&key[8..])))
}

pub fn hash_prefix_bounds(prefix: &[u8]) -> Result<(Vec<u8>, Vec<u8>), KvStoreError> {
    if prefix.len() > LENGTH {
        return Err(invalid("DAG hash prefix is longer than a hash"));
    }
    let mut lower = vec![0; LENGTH];
    let mut upper = vec![u8::MAX; LENGTH];
    lower[..prefix.len()].copy_from_slice(prefix);
    upper[..prefix.len()].copy_from_slice(prefix);
    Ok((lower, upper))
}

/// `sender`'s length, `sender`, then `seq_num` in big-endian order with the
/// sign bit flipped, so one sender's blocks at one sequence number share a
/// prefix and the hash follows.
fn sequence_prefix(sender: &[u8], seq_num: i32) -> Result<Vec<u8>, KvStoreError> {
    let length = u8::try_from(sender.len()).map_err(|_| invalid("DAG sender is too long"))?;
    let mut key = Vec::with_capacity(1 + sender.len() + 4 + LENGTH);
    key.push(length);
    key.extend_from_slice(sender);
    key.extend_from_slice(&((seq_num as u32) ^ (1 << 31)).to_be_bytes());
    Ok(key)
}

fn sequence_key(sender: &[u8], seq_num: i32, hash: &[u8]) -> Result<Vec<u8>, KvStoreError> {
    validate_hash(hash)?;
    let mut key = sequence_prefix(sender, seq_num)?;
    key.extend_from_slice(hash);
    Ok(key)
}

fn child_key(parent: &[u8], child: &[u8]) -> Result<Vec<u8>, KvStoreError> {
    validate_hash(child)?;
    let mut key = Vec::with_capacity(2 * LENGTH + 1);
    if parent.len() == LENGTH {
        key.extend_from_slice(parent);
    } else {
        key.push(1);
        key.extend_from_slice(&Blake2b256::hash(parent.to_vec()));
    }
    key.extend_from_slice(child);
    Ok(key)
}

impl DagHistoryIndex {
    pub fn new(group: Arc<AtomicStoreGroup>, slots: HistoryTableSlots) -> Self {
        Self { group, slots }
    }

    pub fn membership(
        &self,
        hash: &BlockHash,
        view_sequence: u64,
    ) -> Result<Option<MembershipRecord>, KvStoreError> {
        if hash.len() != LENGTH {
            return Ok(None);
        }
        let query = HistoryQuery::new("membership");
        let row = self.group.get(self.slots.membership, hash)?;
        query.visited(usize::from(row.is_some()));
        row.map(|bytes| decode::<MembershipRecord>(&bytes))
            .transpose()
            .map(|record| record.filter(|record| record.sequence <= view_sequence))
    }

    pub fn membership_in(
        &self,
        tx: &mut AtomicTransaction<'_>,
        hash: &BlockHash,
    ) -> Result<Option<MembershipRecord>, KvStoreError> {
        if hash.len() != LENGTH {
            return Ok(None);
        }
        let query = HistoryQuery::new("membership_in");
        let row = tx.get(self.slots.membership, hash)?;
        query.visited(usize::from(row.is_some()));
        row.map(|bytes| decode(&bytes)).transpose()
    }

    pub fn index_block(
        &self,
        tx: &mut AtomicTransaction<'_>,
        metadata: &BlockMetadata,
        sequence: u64,
    ) -> Result<(), KvStoreError> {
        validate_hash(&metadata.block_hash)?;
        if let Some(existing) = self.membership_in(tx, &metadata.block_hash)? {
            if existing.height != metadata.block_number || existing.invalid != metadata.is_invalid()
            {
                return Err(invalid(
                    "DAG membership cannot change block height or invalid status",
                ));
            }
            return Ok(());
        }
        let record = MembershipRecord {
            sequence,
            height: metadata.block_number,
            invalid: metadata.is_invalid(),
        };
        tx.put(
            self.slots.membership,
            metadata.block_hash.to_vec(),
            encode(&record)?,
        )?;
        tx.put(
            self.slots.heights,
            encode_height_key(record.height, &metadata.block_hash)?,
            encode(&sequence)?,
        )?;
        tx.put(
            self.slots.sequences,
            sequence_key(
                &metadata.sender,
                metadata.sequence_number,
                &metadata.block_hash,
            )?,
            encode(&sequence)?,
        )?;
        for parent in &metadata.parents {
            let key = child_key(parent, &metadata.block_hash)?;
            let value = if parent.len() == LENGTH {
                encode(&sequence)?
            } else {
                let mut originals = match tx.get(self.slots.children, &key)? {
                    Some(bytes) => {
                        let (existing_sequence, originals): (u64, Vec<Vec<u8>>) = decode(&bytes)?;
                        if existing_sequence != sequence {
                            return Err(KvStoreError::SerializationError(
                                "DAG exceptional child index sequence disagrees".into(),
                            ));
                        }
                        originals
                    }
                    None => Vec::new(),
                };
                if !originals
                    .iter()
                    .any(|original| original.as_slice() == parent.as_ref())
                {
                    originals.push(parent.to_vec());
                }
                encode(&(sequence, originals))?
            };
            tx.put(self.slots.children, key, value)?;
        }
        Ok(())
    }

    pub fn scan_height_range(
        &self,
        start: i64,
        end: i64,
        view_sequence: u64,
        valid_only: bool,
        after: Option<&[u8]>,
        limit: usize,
    ) -> Result<HistoryPage<BlockHash>, KvStoreError> {
        let query = HistoryQuery::new("height_range");
        let lower = encode_height_key(start, &[0; LENGTH])?;
        let upper = encode_height_key(end, &[u8::MAX; LENGTH])?;
        let page =
            self.group
                .scan_page(self.slots.heights, Some(&lower), Some(&upper), after, limit)?;
        query.visited(page.rows.len());
        let mut rows = Vec::new();
        for (key, value) in page.rows {
            let sequence: u64 = decode(&value)?;
            if sequence > view_sequence {
                continue;
            }
            let (height, hash) = decode_height_key(&key)?;
            let membership = self.membership(&hash, view_sequence)?.ok_or_else(|| {
                KvStoreError::KeyNotFound("DAG height index has no visible membership".into())
            })?;
            if membership.height != height || membership.sequence != sequence {
                return Err(KvStoreError::SerializationError(
                    "DAG height and membership indexes disagree".into(),
                ));
            }
            if !valid_only || !membership.invalid {
                rows.push(hash);
            }
        }
        Ok(HistoryPage {
            rows,
            next: page.next,
        })
    }

    /// The blocks visible at `view_sequence` that `sender` signed at
    /// `seq_num`.
    pub fn blocks_at_sequence(
        &self,
        sender: &[u8],
        seq_num: i32,
        view_sequence: u64,
    ) -> Result<Vec<BlockHash>, KvStoreError> {
        let query = HistoryQuery::new("sequence");
        let prefix = sequence_prefix(sender, seq_num)?;
        let mut lower = prefix.clone();
        lower.extend_from_slice(&[0; LENGTH]);
        let mut upper = prefix.clone();
        upper.extend_from_slice(&[u8::MAX; LENGTH]);
        let mut rows = Vec::new();
        let mut after: Option<Vec<u8>> = None;
        loop {
            let page = self.group.scan_page(
                self.slots.sequences,
                Some(&lower),
                Some(&upper),
                after.as_deref(),
                1024,
            )?;
            query.visited(page.rows.len());
            for (key, value) in page.rows {
                let sequence: u64 = decode(&value)?;
                if sequence > view_sequence {
                    continue;
                }
                if key.len() != prefix.len() + LENGTH {
                    return Err(KvStoreError::SerializationError(
                        "Invalid DAG sequence key length".into(),
                    ));
                }
                let hash = BlockHash::copy_from_slice(&key[prefix.len()..]);
                let membership = self.membership(&hash, view_sequence)?.ok_or_else(|| {
                    KvStoreError::KeyNotFound("DAG sequence index has no visible membership".into())
                })?;
                if membership.sequence != sequence {
                    return Err(KvStoreError::SerializationError(
                        "DAG sequence and membership indexes disagree".into(),
                    ));
                }
                rows.push(hash);
            }
            match page.next {
                Some(next) => after = Some(next),
                None => return Ok(rows),
            }
        }
    }

    pub fn scan_children(
        &self,
        parent: &BlockHash,
        view_sequence: u64,
        after: Option<&[u8]>,
        limit: usize,
    ) -> Result<HistoryPage<BlockHash>, KvStoreError> {
        let query = HistoryQuery::new("children");
        let lower = child_key(parent, &[0; LENGTH])?;
        let upper = child_key(parent, &[u8::MAX; LENGTH])?;
        let page = self.group.scan_page(
            self.slots.children,
            Some(&lower),
            Some(&upper),
            after,
            limit,
        )?;
        query.visited(page.rows.len());
        let mut rows = Vec::new();
        for (key, value) in page.rows {
            let sequence: u64 = if parent.len() == LENGTH {
                decode(&value)?
            } else {
                let (sequence, originals): (u64, Vec<Vec<u8>>) = decode(&value)?;
                if !originals
                    .iter()
                    .any(|original| original.as_slice() == parent.as_ref())
                {
                    continue;
                }
                sequence
            };
            if sequence > view_sequence {
                continue;
            }
            let prefix_length = if parent.len() == LENGTH {
                LENGTH
            } else {
                LENGTH + 1
            };
            if key.len() != prefix_length + LENGTH {
                return Err(KvStoreError::SerializationError(
                    "Invalid DAG child key length".into(),
                ));
            }
            let hash = BlockHash::copy_from_slice(&key[prefix_length..]);
            let membership = self.membership(&hash, view_sequence)?.ok_or_else(|| {
                KvStoreError::KeyNotFound("DAG child index has no visible membership".into())
            })?;
            if membership.sequence != sequence {
                return Err(KvStoreError::SerializationError(
                    "DAG child and membership indexes disagree".into(),
                ));
            }
            rows.push(hash);
        }
        Ok(HistoryPage {
            rows,
            next: page.next,
        })
    }

    fn membership_page(
        &self,
        page: StorePage,
        view_sequence: u64,
    ) -> Result<HistoryPage<BlockHash>, KvStoreError> {
        let mut rows = Vec::new();
        for (key, value) in page.rows {
            if key.len() != LENGTH {
                return Err(KvStoreError::SerializationError(
                    "Invalid DAG membership key length".into(),
                ));
            }
            let membership: MembershipRecord = decode(&value)?;
            if membership.sequence <= view_sequence {
                rows.push(BlockHash::from(key));
            }
        }
        Ok(HistoryPage {
            rows,
            next: page.next,
        })
    }

    pub fn scan_membership(
        &self,
        view_sequence: u64,
        after: Option<&[u8]>,
        limit: usize,
    ) -> Result<HistoryPage<BlockHash>, KvStoreError> {
        let query = HistoryQuery::new("membership_page");
        let page = self
            .group
            .scan_page(self.slots.membership, None, None, after, limit)?;
        query.visited(page.rows.len());
        self.membership_page(page, view_sequence)
    }

    pub fn scan_prefix(
        &self,
        prefix: &[u8],
        view_sequence: u64,
        after: Option<&[u8]>,
        limit: usize,
    ) -> Result<HistoryPage<BlockHash>, KvStoreError> {
        let query = HistoryQuery::new("prefix");
        let (lower, upper) = hash_prefix_bounds(prefix)?;
        let page = self.group.scan_page(
            self.slots.membership,
            Some(&lower),
            Some(&upper),
            after,
            limit,
        )?;
        query.visited(page.rows.len());
        self.membership_page(page, view_sequence)
    }
}
