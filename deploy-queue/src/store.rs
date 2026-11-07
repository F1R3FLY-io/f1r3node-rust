use std::path::Path;
use std::sync::Arc;

use heed::EnvOpenOptions;
use shared::rust::store::key_value_store::{KeyValueStore, KvStoreError};
use shared::rust::store::lmdb_key_value_store::LmdbKeyValueStore;

use crate::record::{QueueRecord, Sig};

/// LMDB reserves the map as address space; disk use follows the records.
const MAP_SIZE: usize = 16 << 30;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("queue store: {0}")]
    Kv(#[from] KvStoreError),
    #[error("queue store: {0}")]
    Heed(#[from] heed::Error),
    #[error("queue store: {0}")]
    Io(#[from] std::io::Error),
    #[error("queue store: encoding a record: {0}")]
    Encode(bincode::Error),
    #[error("queue store: record {sig} is corrupt: {reason}")]
    Corrupt { sig: String, reason: String },
}

/// The durable record of every deploy the queue holds, keyed by signature.
#[derive(Clone)]
pub struct QueueStore {
    kv: LmdbKeyValueStore,
}

impl QueueStore {
    pub fn open(dir: &Path) -> Result<Self, StoreError> {
        std::fs::create_dir_all(dir)?;
        let mut options = EnvOpenOptions::new();
        options.map_size(MAP_SIZE);
        options.max_dbs(1);
        // SAFETY: the queue is the only process that opens this directory,
        // and it opens it once.
        let env = unsafe { options.open(dir)? };
        let mut wtxn = env.write_txn()?;
        let db = env.create_database(&mut wtxn, Some("deploys"))?;
        wtxn.commit()?;
        Ok(Self {
            kv: LmdbKeyValueStore::new(Arc::new(env), db),
        })
    }

    /// Stores a new record; false when the signature is already held.
    pub fn insert_new(&self, sig: &Sig, record: &QueueRecord) -> Result<bool, StoreError> {
        Ok(self.kv.put_one_if_absent(sig.clone(), encode(record)?)?)
    }

    pub fn put(&self, sig: &Sig, record: &QueueRecord) -> Result<(), StoreError> {
        Ok(self.kv.put_one(sig.clone(), encode(record)?)?)
    }

    pub fn remove(&self, sigs: Vec<Sig>) -> Result<(), StoreError> {
        self.kv.delete(sigs)?;
        Ok(())
    }

    pub fn load(&self) -> Result<Vec<(Sig, QueueRecord)>, StoreError> {
        self.kv
            .to_map()?
            .into_iter()
            .map(|(sig, bytes)| {
                let record = bincode::deserialize(&bytes).map_err(|e| StoreError::Corrupt {
                    sig: hex::encode(&sig),
                    reason: e.to_string(),
                })?;
                Ok((sig, record))
            })
            .collect()
    }
}

fn encode(record: &QueueRecord) -> Result<Vec<u8>, StoreError> {
    bincode::serialize(record).map_err(StoreError::Encode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::QueueState;

    fn record(sequence: u64) -> QueueRecord {
        QueueRecord {
            deploy: vec![1, 2, 3],
            deployer: vec![4],
            valid_after: 10,
            sequence,
            accepted_at_ms: 0,
            state: QueueState::Queued,
            submitted_at_height: None,
            submissions: 0,
            rejection_count: 0,
            latest_block_hash: None,
            decided_at_height: None,
            detail: None,
        }
    }

    #[test]
    fn records_survive_a_reopen_and_insert_refuses_a_held_sig() {
        let dir = tempfile::tempdir().unwrap();
        {
            let store = QueueStore::open(dir.path()).unwrap();
            assert!(store.insert_new(&vec![1], &record(1)).unwrap());
            assert!(!store.insert_new(&vec![1], &record(2)).unwrap());
            let mut submitted = record(1);
            submitted.state = QueueState::Submitted;
            store.put(&vec![1], &submitted).unwrap();
            assert!(store.insert_new(&vec![2], &record(3)).unwrap());
            store.remove(vec![vec![2]]).unwrap();
        }
        let store = QueueStore::open(dir.path()).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].0, vec![1]);
        assert_eq!(loaded[0].1.state, QueueState::Submitted);
        assert_eq!(loaded[0].1.sequence, 1);
    }
}
