// Leaky-bucket votes by claim and voter, recorded from the bodies of stored
// blocks before they become DAG-visible, so a vote is findable as soon as the
// block that carries it is.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use models::rust::casper::protocol::casper_message::{ClaimFamily, ClaimId};
use models::rust::validator::Validator;
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use serde::{Deserialize, Serialize};
use shared::rust::store::key_value_store::{KeyValueStore, KvStoreError};
use shared::rust::store::key_value_typed_store::KeyValueTypedStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;
use shared::rust::ByteString;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoteEntry {
    pub block_hash: ByteString,
    pub sequence_number: i32,
    pub yea: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct VoteRow {
    opened_at: i64,
    entries: Vec<VoteEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Stored {
    Votes(VoteRow),
    /// Claims opened at or before this block number may have lost their rows.
    PrunedThrough(i64),
}

#[derive(Clone)]
pub struct VoteIndex {
    rows: KeyValueTypedStoreImpl<ByteString, Stored>,
    /// The persisted mark, or `i64::MIN` before the first pruning.
    pruned_through: Arc<AtomicI64>,
}

impl VoteIndex {
    /// Pruning walks the whole table, so it runs only when the cutoff has
    /// advanced by at least this many blocks since the last walk.
    const PRUNE_STRIDE: i64 = 64;

    /// Every row key begins with a claim family byte, which is never zero.
    const MARK_KEY: [u8; 1] = [0];

    pub fn new(rows_kv: Arc<dyn KeyValueStore>) -> Result<Self, KvStoreError> {
        let rows = KeyValueTypedStoreImpl::new(rows_kv);
        let pruned_through = match rows.get_one(&Self::MARK_KEY.to_vec())? {
            Some(Stored::PrunedThrough(mark)) => mark,
            None => i64::MIN,
            Some(Stored::Votes(_)) => {
                return Err(KvStoreError::SerializationError(
                    "the vote index mark key holds a vote row".to_string(),
                ))
            }
        };
        Ok(Self {
            rows,
            pruned_through: Arc::new(AtomicI64::new(pruned_through)),
        })
    }

    pub fn in_memory() -> Self {
        Self::new(Arc::new(InMemoryKeyValueStore::new()))
            .expect("an empty store holds no mark to misread")
    }

    fn key(claim: &ClaimId, voter: &Validator) -> ByteString {
        let family: u8 = match claim.family {
            ClaimFamily::Eject => 1,
            ClaimFamily::Rejoin => 2,
            ClaimFamily::Overflow => 3,
        };
        let mut key = Vec::with_capacity(
            1 + 8 + 3 * 4 + claim.target.len() + claim.proposer.len() + voter.len(),
        );
        key.push(family);
        key.extend_from_slice(&claim.opened_at.to_be_bytes());
        for part in [&claim.target, &claim.proposer, voter] {
            key.extend_from_slice(&(part.len() as u32).to_be_bytes());
            key.extend_from_slice(part);
        }
        key
    }

    fn row(&self, key: &ByteString) -> Result<Option<VoteRow>, KvStoreError> {
        match self.rows.get_one(key)? {
            Some(Stored::Votes(row)) => Ok(Some(row)),
            None => Ok(None),
            Some(Stored::PrunedThrough(_)) => Err(KvStoreError::SerializationError(
                "a vote row key holds the pruning mark".to_string(),
            )),
        }
    }

    /// Idempotent per block, so a redelivered insert writes nothing twice.
    pub fn record_once(
        &self,
        claim: &ClaimId,
        voter: &Validator,
        entry: VoteEntry,
    ) -> Result<(), KvStoreError> {
        let key = Self::key(claim, voter);
        // Absent means no vote has been recorded yet.
        let mut row = self.row(&key)?.unwrap_or(VoteRow {
            opened_at: claim.opened_at,
            entries: Vec::new(),
        });
        if row.entries.iter().any(|e| e.block_hash == entry.block_hash) {
            return Ok(());
        }
        row.entries.push(entry);
        self.rows.put_one(key, Stored::Votes(row))
    }

    /// The votes recorded for `voter` on `claim`, or `None` when pruning may
    /// have removed them and the index cannot answer.
    pub fn entries(
        &self,
        claim: &ClaimId,
        voter: &Validator,
    ) -> Result<Option<Vec<VoteEntry>>, KvStoreError> {
        let row = self.row(&Self::key(claim, voter))?;
        // The row is read before the mark. Pruning raises the mark before it
        // deletes, so a row found missing because it was just deleted is
        // always followed by a mark that covers it.
        if claim.opened_at <= self.pruned_through.load(Ordering::SeqCst) {
            return Ok(None);
        }
        Ok(Some(row.map(|row| row.entries).unwrap_or_default()))
    }

    /// Drop the votes on claims opened at or before `opened_at_cutoff`, at
    /// most `PRUNE_STRIDE` blocks after the cutoff first passes them.
    pub fn prune_opened_at_or_before(&self, opened_at_cutoff: i64) -> Result<u64, KvStoreError> {
        let mark = self.pruned_through.load(Ordering::SeqCst);
        if mark != i64::MIN && opened_at_cutoff < mark.saturating_add(Self::PRUNE_STRIDE) {
            return Ok(0);
        }
        self.rows.put_one(
            Self::MARK_KEY.to_vec(),
            Stored::PrunedThrough(opened_at_cutoff),
        )?;
        self.pruned_through
            .store(opened_at_cutoff, Ordering::SeqCst);

        let expired: Vec<ByteString> = self
            .rows
            .to_map()?
            .into_iter()
            .filter(|(_, stored)| {
                matches!(stored, Stored::Votes(row) if row.opened_at <= opened_at_cutoff)
            })
            .map(|(key, _)| key)
            .collect();
        let removed = expired.len() as u64;
        if !expired.is_empty() {
            self.rows.delete(expired)?;
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use prost::bytes::Bytes;

    use super::*;

    fn claim(opened_at: i64, target: u8) -> ClaimId {
        ClaimId {
            family: ClaimFamily::Eject,
            target: Bytes::from(vec![target; 65]),
            opened_at,
            proposer: Bytes::from(vec![9; 65]),
        }
    }

    fn entry(block: u8, sequence_number: i32) -> VoteEntry {
        VoteEntry {
            block_hash: vec![block; 32],
            sequence_number,
            yea: true,
        }
    }

    #[test]
    fn a_redelivered_vote_is_recorded_once() {
        let index = VoteIndex::in_memory();
        let voter = Bytes::from(vec![1; 65]);
        index
            .record_once(&claim(5, 2), &voter, entry(7, 3))
            .unwrap();
        index
            .record_once(&claim(5, 2), &voter, entry(7, 3))
            .unwrap();
        assert_eq!(
            index.entries(&claim(5, 2), &voter).unwrap(),
            Some(vec![entry(7, 3)])
        );
    }

    #[test]
    fn votes_are_kept_apart_by_claim_and_voter() {
        let index = VoteIndex::in_memory();
        let voter = Bytes::from(vec![1; 65]);
        let other_voter = Bytes::from(vec![3; 65]);
        index
            .record_once(&claim(5, 2), &voter, entry(7, 3))
            .unwrap();
        assert_eq!(index.entries(&claim(6, 2), &voter).unwrap(), Some(vec![]));
        assert_eq!(index.entries(&claim(5, 4), &voter).unwrap(), Some(vec![]));
        assert_eq!(
            index.entries(&claim(5, 2), &other_voter).unwrap(),
            Some(vec![])
        );
    }

    /// A pruned claim is reported as unknown, not as unvoted.
    #[test]
    fn pruning_drops_only_claims_opened_at_or_before_the_cutoff() {
        let index = VoteIndex::in_memory();
        let voter = Bytes::from(vec![1; 65]);
        index
            .record_once(&claim(5, 2), &voter, entry(7, 3))
            .unwrap();
        index
            .record_once(&claim(6, 2), &voter, entry(8, 4))
            .unwrap();
        assert_eq!(index.prune_opened_at_or_before(5).unwrap(), 1);
        assert_eq!(index.entries(&claim(5, 2), &voter).unwrap(), None);
        assert_eq!(
            index.entries(&claim(6, 2), &voter).unwrap(),
            Some(vec![entry(8, 4)])
        );
    }

    #[test]
    fn a_passed_claim_is_pruned_within_one_stride() {
        let index = VoteIndex::in_memory();
        let voter = Bytes::from(vec![1; 65]);
        index.prune_opened_at_or_before(0).unwrap();
        index
            .record_once(&claim(6, 2), &voter, entry(8, 4))
            .unwrap();
        index.prune_opened_at_or_before(6).unwrap();
        assert_eq!(
            index.entries(&claim(6, 2), &voter).unwrap(),
            Some(vec![entry(8, 4)]),
            "inside the stride nothing is pruned"
        );
        index
            .prune_opened_at_or_before(VoteIndex::PRUNE_STRIDE)
            .unwrap();
        assert_eq!(index.entries(&claim(6, 2), &voter).unwrap(), None);
    }

    /// A restarted node still knows which claims it can no longer answer for.
    #[test]
    fn the_pruning_mark_survives_a_restart() {
        let store: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let voter = Bytes::from(vec![1; 65]);
        {
            let index = VoteIndex::new(store.clone()).unwrap();
            index
                .record_once(&claim(5, 2), &voter, entry(7, 3))
                .unwrap();
            index
                .record_once(&claim(9, 2), &voter, entry(8, 4))
                .unwrap();
            index.prune_opened_at_or_before(5).unwrap();
        }
        let restarted = VoteIndex::new(store).unwrap();
        assert_eq!(restarted.entries(&claim(5, 2), &voter).unwrap(), None);
        assert_eq!(
            restarted.entries(&claim(9, 2), &voter).unwrap(),
            Some(vec![entry(8, 4)])
        );
    }
}
