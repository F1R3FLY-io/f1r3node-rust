use std::path::Path;
use std::sync::Arc;

use heed::EnvOpenOptions;
use shared::rust::blocking::in_blocking;
use shared::rust::store::key_value_store::{KeyValueStore, KvStoreError};
use shared::rust::store::lmdb_key_value_store::LmdbKeyValueStore;

use crate::record::{QueueRecord, Sig};

/// LMDB reserves the map as address space; disk use follows the records.
const MAP_SIZE: usize = 16 << 30;

/// The most signed-deploy bytes the queue holds: half the map, leaving the rest
/// for records and LMDB's own pages.
pub const PAYLOAD_BUDGET: u64 = (MAP_SIZE / 2) as u64;

const FORMAT_KEY: &[u8] = b"format";
const FORMAT_VERSION: u32 = 1;

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
    #[error(
        "queue store: {dir} holds format {found}, this build reads format {expected}; \
         remove the directory to start an empty queue"
    )]
    Format {
        dir: String,
        found: String,
        expected: u32,
    },
}

/// The durable record of every deploy the queue holds, keyed by signature.
/// Records stay in memory; each signed deploy is read from disk only to send
/// it, and is deleted once the deploy is decided.
#[derive(Clone)]
pub struct QueueStore {
    kv: LmdbKeyValueStore,
    payloads: LmdbKeyValueStore,
}

/// One batch of changes, written in one transaction.
#[derive(Default)]
pub struct Batch<'a> {
    pub records: Vec<(&'a Sig, &'a QueueRecord)>,
    pub payloads: Vec<(&'a Sig, &'a [u8])>,
    pub removed_records: &'a [Sig],
    pub removed_payloads: &'a [Sig],
}

impl QueueStore {
    pub fn open(dir: &Path) -> Result<Self, StoreError> {
        std::fs::create_dir_all(dir)?;
        let mut options = EnvOpenOptions::new();
        options.map_size(MAP_SIZE);
        options.max_dbs(3);
        // SAFETY: the queue is the only process that opens this directory,
        // and it opens it once.
        let env = Arc::new(unsafe { options.open(dir)? });
        let mut wtxn = env.write_txn()?;
        let records = env.create_database(&mut wtxn, Some("records"))?;
        let payloads = env.create_database(&mut wtxn, Some("payloads"))?;
        let meta: heed::Database<heed::types::Bytes, heed::types::Bytes> =
            env.create_database(&mut wtxn, Some("meta"))?;
        let expected = FORMAT_VERSION.to_be_bytes();
        match meta.get(&wtxn, FORMAT_KEY)? {
            Some(found) if found == expected.as_slice() => {}
            Some(found) => {
                return Err(StoreError::Format {
                    dir: dir.display().to_string(),
                    found: hex::encode(found),
                    expected: FORMAT_VERSION,
                })
            }
            None if records.is_empty(&wtxn)? => meta.put(&mut wtxn, FORMAT_KEY, &expected)?,
            None => {
                return Err(StoreError::Format {
                    dir: dir.display().to_string(),
                    found: "none".to_string(),
                    expected: FORMAT_VERSION,
                })
            }
        }
        wtxn.commit()?;
        Ok(Self {
            kv: LmdbKeyValueStore::new(env.clone(), records),
            payloads: LmdbKeyValueStore::new(env, payloads),
        })
    }

    /// Applies the deletions, then the writes, in one transaction: all of
    /// them are durable, or none.
    pub fn commit(&self, batch: &Batch) -> Result<(), StoreError> {
        let encoded = batch
            .records
            .iter()
            .map(|(sig, record)| Ok(((*sig).clone(), encode(record)?)))
            .collect::<Result<Vec<_>, StoreError>>()?;
        in_blocking(|| {
            let mut writer = self.kv.env.write_txn()?;
            for sig in batch.removed_records {
                self.kv.db.delete(&mut writer, sig)?;
                self.payloads.db.delete(&mut writer, sig)?;
            }
            for sig in batch.removed_payloads {
                self.payloads.db.delete(&mut writer, sig)?;
            }
            for (sig, bytes) in &encoded {
                self.kv.db.put(&mut writer, sig, bytes)?;
            }
            for (sig, bytes) in &batch.payloads {
                self.payloads.db.put(&mut writer, sig, &bytes.to_vec())?;
            }
            writer.commit()?;
            Ok(())
        })
    }

    /// The signed deploy stored for `sig`, if it is still held.
    pub fn payload(&self, sig: &Sig) -> Result<Option<Vec<u8>>, StoreError> {
        Ok(self.payloads.get_one(sig)?)
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
            deployer: vec![4],
            payload_bytes: 3,
            valid_after: 10,
            sequence,
            accepted_at_ms: 0,
            state: QueueState::Queued,
            submitted_at_height: None,
            last_sent_height: None,
            submissions: 0,
            rejection_count: 0,
            latest_block_hash: None,
            decided_at_height: None,
            detail: None,
        }
    }

    #[test]
    fn a_commit_writes_and_deletes_together_and_survives_a_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let (one, two) = (vec![1], vec![2]);
        {
            let store = QueueStore::open(dir.path()).unwrap();
            store
                .commit(&Batch {
                    records: vec![(&one, &record(1)), (&two, &record(3))],
                    payloads: vec![(&one, &[7, 7]), (&two, &[8])],
                    ..Batch::default()
                })
                .unwrap();
            let mut submitted = record(1);
            submitted.state = QueueState::Submitted;
            store
                .commit(&Batch {
                    records: vec![(&one, &submitted)],
                    removed_records: std::slice::from_ref(&two),
                    ..Batch::default()
                })
                .unwrap();
        }
        let store = QueueStore::open(dir.path()).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].0, one);
        assert_eq!(loaded[0].1.state, QueueState::Submitted);
        assert_eq!(loaded[0].1.sequence, 1);
        assert_eq!(store.payload(&one).unwrap(), Some(vec![7, 7]));
        assert_eq!(
            store.payload(&two).unwrap(),
            None,
            "a removed record takes its payload"
        );
    }

    #[test]
    fn a_decided_deploy_drops_its_payload_and_keeps_its_record() {
        let dir = tempfile::tempdir().unwrap();
        let store = QueueStore::open(dir.path()).unwrap();
        let sig = vec![5];
        store
            .commit(&Batch {
                records: vec![(&sig, &record(1))],
                payloads: vec![(&sig, &[9])],
                ..Batch::default()
            })
            .unwrap();
        store
            .commit(&Batch {
                removed_payloads: std::slice::from_ref(&sig),
                ..Batch::default()
            })
            .unwrap();
        assert_eq!(store.payload(&sig).unwrap(), None);
        assert_eq!(store.load().unwrap().len(), 1);
    }

    #[test]
    fn a_store_without_this_builds_format_is_refused_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let sig = vec![6];
        {
            let store = QueueStore::open(dir.path()).unwrap();
            store
                .commit(&Batch {
                    records: vec![(&sig, &record(1))],
                    ..Batch::default()
                })
                .unwrap();
            let mut wtxn = store.kv.env.write_txn().unwrap();
            let meta: heed::Database<heed::types::Bytes, heed::types::Bytes> = store
                .kv
                .env
                .open_database(&wtxn, Some("meta"))
                .unwrap()
                .unwrap();
            meta.delete(&mut wtxn, FORMAT_KEY).unwrap();
            wtxn.commit().unwrap();
        }
        assert!(matches!(
            QueueStore::open(dir.path()),
            Err(StoreError::Format { .. })
        ));
        let fresh = tempfile::tempdir().unwrap();
        QueueStore::open(fresh.path()).unwrap();
        QueueStore::open(fresh.path()).unwrap();
    }
}
