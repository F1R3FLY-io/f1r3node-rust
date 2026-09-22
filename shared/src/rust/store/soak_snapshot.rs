use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use heed::types::Bytes;
use heed::{Database, Env, RoTxn, WithTls};

use super::key_value_store::KeyValueStore;
use super::lmdb_key_value_store::LmdbKeyValueStore;
use crate::rust::ByteBuffer;

pub const LENGTH_PREFIX_BYTES: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadLimits {
    pub max_value_bytes: usize,
    pub max_total_bytes: usize,
    pub max_records: usize,
    pub max_operations: usize,
}

impl ReadLimits {
    pub fn validate(&self) -> Result<(), SnapshotError> {
        if self.max_value_bytes < LENGTH_PREFIX_BYTES {
            return Err(SnapshotError::InvalidLimits(format!(
                "max_value_bytes must be at least {LENGTH_PREFIX_BYTES}"
            )));
        }
        if self.max_total_bytes < self.max_value_bytes {
            return Err(SnapshotError::InvalidLimits(
                "max_total_bytes must be at least max_value_bytes".to_string(),
            ));
        }
        if self.max_records == 0 {
            return Err(SnapshotError::InvalidLimits(
                "max_records must be positive".to_string(),
            ));
        }
        if self.max_operations == 0 {
            return Err(SnapshotError::InvalidLimits(
                "max_operations must be positive".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SnapshotError {
    #[error("invalid limits: {0}")]
    InvalidLimits(String),
    #[error("unsupported storage backend: {0}")]
    Unsupported(String),
    #[error("{kind} limit {limit} exceeded by observed {observed}")]
    LimitExceeded {
        kind: &'static str,
        limit: usize,
        observed: usize,
    },
    #[error("environment {environment} changed: transaction {opened} observed as {observed}")]
    EnvironmentChanged {
        environment: String,
        opened: usize,
        observed: usize,
    },
    #[error("read failed: {0}")]
    ReadFailed(String),
    #[error("malformed encoding: {0}")]
    Malformed(String),
    #[error("lock acquisition exceeded {0:?}")]
    LockTimeout(Duration),
    #[error("incomplete evidence: {0}")]
    Incomplete(String),
    #[error("{0} counter overflowed")]
    CounterOverflow(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionIdentity {
    pub environment: String,
    pub last_txn_id_before_open: usize,
    pub txn_id: usize,
    pub last_txn_id_after_validation: Option<usize>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReadUsage {
    pub operations: usize,
    pub records: usize,
    pub bytes: usize,
}

fn check_limit(kind: &'static str, observed: usize, limit: usize) -> Result<(), SnapshotError> {
    if observed > limit {
        return Err(SnapshotError::LimitExceeded {
            kind,
            limit,
            observed,
        });
    }
    Ok(())
}

fn checked_total(
    kind: &'static str,
    current: usize,
    amount: usize,
) -> Result<usize, SnapshotError> {
    current
        .checked_add(amount)
        .ok_or(SnapshotError::CounterOverflow(kind))
}

impl ReadUsage {
    fn charge_operation(&mut self, limits: &ReadLimits) -> Result<(), SnapshotError> {
        let observed = checked_total("operations", self.operations, 1)?;
        check_limit("operations", observed, limits.max_operations)?;
        self.operations = observed;
        Ok(())
    }

    fn charge_record(&mut self, limits: &ReadLimits, raw_len: usize) -> Result<(), SnapshotError> {
        let records = checked_total("records", self.records, 1)?;
        check_limit("records", records, limits.max_records)?;
        let bytes = checked_total("total bytes", self.bytes, raw_len)?;
        check_limit("total bytes", bytes, limits.max_total_bytes)?;
        self.records = records;
        self.bytes = bytes;
        Ok(())
    }
}

struct EnvSession {
    env: Arc<Env>,
    path: PathBuf,
    txn: RoTxn<'static, WithTls>,
    last_txn_id_before_open: usize,
    txn_id: usize,
}

pub struct BoundedLmdbReader {
    limits: ReadLimits,
    sessions: Vec<EnvSession>,
    usage: ReadUsage,
}

pub fn encode_length_prefixed(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(LENGTH_PREFIX_BYTES + payload.len());
    out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    out.extend_from_slice(payload);
    out
}

pub fn decode_length_prefixed(raw: &[u8]) -> Result<Vec<u8>, SnapshotError> {
    length_prefixed_payload(raw).map(<[u8]>::to_vec)
}

fn length_prefixed_payload(raw: &[u8]) -> Result<&[u8], SnapshotError> {
    if raw.len() < LENGTH_PREFIX_BYTES {
        return Err(SnapshotError::Malformed(format!(
            "raw value of {} bytes is shorter than its length prefix",
            raw.len()
        )));
    }
    let mut prefix = [0u8; LENGTH_PREFIX_BYTES];
    prefix.copy_from_slice(&raw[..LENGTH_PREFIX_BYTES]);
    let declared = u64::from_le_bytes(prefix);
    let actual = (raw.len() - LENGTH_PREFIX_BYTES) as u64;
    if declared != actual {
        return Err(SnapshotError::Malformed(format!(
            "length prefix declares {declared} bytes but {actual} bytes follow"
        )));
    }
    Ok(&raw[LENGTH_PREFIX_BYTES..])
}

fn lmdb_store(store: &Arc<dyn KeyValueStore>) -> Result<&LmdbKeyValueStore, SnapshotError> {
    store
        .as_any()
        .downcast_ref::<LmdbKeyValueStore>()
        .ok_or_else(|| {
            SnapshotError::Unsupported(
                "bounded capture requires an LMDB-backed key-value store".to_string(),
            )
        })
}

impl BoundedLmdbReader {
    pub fn open(
        stores: &[&Arc<dyn KeyValueStore>],
        limits: ReadLimits,
    ) -> Result<Self, SnapshotError> {
        limits.validate()?;
        check_limit("stores", stores.len(), limits.max_operations)?;
        let mut lmdb_stores = Vec::with_capacity(stores.len());
        for store in stores {
            lmdb_stores.push(lmdb_store(store)?);
        }
        let mut sessions: Vec<EnvSession> = Vec::new();
        for store in lmdb_stores {
            let path = store.env.path().to_path_buf();
            if sessions.iter().any(|session| session.path == path) {
                continue;
            }
            let last_txn_id_before_open = store.env.info().last_txn_id;
            let txn = Env::clone(&store.env)
                .static_read_txn()
                .map_err(|error| SnapshotError::ReadFailed(error.to_string()))?;
            let txn_id = txn.id();
            if txn_id != last_txn_id_before_open {
                return Err(SnapshotError::EnvironmentChanged {
                    environment: path.display().to_string(),
                    opened: last_txn_id_before_open,
                    observed: txn_id,
                });
            }
            sessions.push(EnvSession {
                env: store.env.clone(),
                path,
                txn,
                last_txn_id_before_open,
                txn_id,
            });
        }
        Ok(Self {
            limits,
            sessions,
            usage: ReadUsage::default(),
        })
    }

    pub fn limits(&self) -> &ReadLimits { &self.limits }

    pub fn usage(&self) -> ReadUsage { self.usage }

    pub fn identities(&self) -> Vec<TransactionIdentity> {
        self.sessions
            .iter()
            .map(|session| TransactionIdentity {
                environment: session.path.display().to_string(),
                last_txn_id_before_open: session.last_txn_id_before_open,
                txn_id: session.txn_id,
                last_txn_id_after_validation: None,
            })
            .collect()
    }

    fn session_for(
        &self,
        store: &Arc<dyn KeyValueStore>,
    ) -> Result<(&EnvSession, Database<Bytes, Bytes>), SnapshotError> {
        let lmdb = lmdb_store(store)?;
        let path = lmdb.env.path();
        let session = self
            .sessions
            .iter()
            .find(|session| session.path == path)
            .ok_or_else(|| {
                SnapshotError::ReadFailed(format!(
                    "store environment {} was not opened by this reader",
                    path.display()
                ))
            })?;
        Ok((session, lmdb.db.remap_types::<Bytes, Bytes>()))
    }

    pub fn read_raw(
        &mut self,
        store: &Arc<dyn KeyValueStore>,
        key: &ByteBuffer,
    ) -> Result<Option<Vec<u8>>, SnapshotError> {
        self.usage.charge_operation(&self.limits)?;
        let key_len = checked_total("key bytes", key.len(), LENGTH_PREFIX_BYTES)?;
        check_limit("key bytes", key_len, self.limits.max_value_bytes)?;
        let encoded_key = encode_length_prefixed(key);
        let mut usage = self.usage;
        let copied = {
            let (session, db) = self.session_for(store)?;
            let found = db
                .get(&session.txn, encoded_key.as_slice())
                .map_err(|error| SnapshotError::ReadFailed(error.to_string()))?;
            match found {
                None => return Ok(None),
                Some(raw) => {
                    check_limit("value bytes", raw.len(), self.limits.max_value_bytes)?;
                    usage.charge_record(&self.limits, raw.len())?;
                    raw.to_vec()
                }
            }
        };
        self.usage = usage;
        Ok(Some(copied))
    }

    pub fn read_value(
        &mut self,
        store: &Arc<dyn KeyValueStore>,
        key: &ByteBuffer,
    ) -> Result<Option<Vec<u8>>, SnapshotError> {
        match self.read_raw(store, key)? {
            None => Ok(None),
            Some(raw) => decode_length_prefixed(&raw).map(Some),
        }
    }

    pub fn restrict_remaining(
        &mut self,
        bytes: usize,
        operations: usize,
    ) -> Result<(), SnapshotError> {
        self.limits.max_total_bytes =
            self.limits
                .max_total_bytes
                .min(checked_total("total bytes", self.usage.bytes, bytes)?);
        self.limits.max_operations = self.limits.max_operations.min(checked_total(
            "operations",
            self.usage.operations,
            operations,
        )?);
        Ok(())
    }

    pub fn scan(
        &mut self,
        store: &Arc<dyn KeyValueStore>,
    ) -> Result<BTreeMap<ByteBuffer, ByteBuffer>, SnapshotError> {
        self.scan_limited(store, usize::MAX)
    }

    pub fn scan_limited(
        &mut self,
        store: &Arc<dyn KeyValueStore>,
        max_records: usize,
    ) -> Result<BTreeMap<ByteBuffer, ByteBuffer>, SnapshotError> {
        let mut usage = self.usage;
        let initial_records = usage.records;
        let result = (|| {
            usage.charge_operation(&self.limits)?;
            let (session, db) = self.session_for(store)?;
            let mut iter = db
                .iter(&session.txn)
                .map_err(|error| SnapshotError::ReadFailed(error.to_string()))?;
            let mut out = BTreeMap::new();
            loop {
                let Some(item) = iter.next() else {
                    return Ok(out);
                };
                let (raw_key, raw_value) =
                    item.map_err(|error| SnapshotError::ReadFailed(error.to_string()))?;
                check_limit("key bytes", raw_key.len(), self.limits.max_value_bytes)?;
                check_limit("value bytes", raw_value.len(), self.limits.max_value_bytes)?;
                let raw_len = checked_total("record bytes", raw_key.len(), raw_value.len())?;
                check_limit(
                    "scan records",
                    checked_total("scan records", usage.records - initial_records, 1)?,
                    max_records,
                )?;
                usage.charge_record(&self.limits, raw_len)?;
                let key = length_prefixed_payload(raw_key)?;
                let value = length_prefixed_payload(raw_value)?;
                out.insert(key.to_vec(), value.to_vec());
                usage.charge_operation(&self.limits)?;
            }
        })();
        self.usage = usage;
        result
    }

    pub fn validate(self) -> Result<Vec<TransactionIdentity>, SnapshotError> {
        let mut identities = Vec::with_capacity(self.sessions.len());
        for session in &self.sessions {
            let after = session.env.info().last_txn_id;
            if after != session.txn_id {
                return Err(SnapshotError::EnvironmentChanged {
                    environment: session.path.display().to_string(),
                    opened: session.txn_id,
                    observed: after,
                });
            }
            identities.push(TransactionIdentity {
                environment: session.path.display().to_string(),
                last_txn_id_before_open: session.last_txn_id_before_open,
                txn_id: session.txn_id,
                last_txn_id_after_validation: Some(after),
            });
        }
        drop(self);
        Ok(identities)
    }
}
