use std::collections::BTreeMap;
use std::fmt::Debug;

use crate::rust::ByteBuffer;

// See shared/src/main/scala/coop/rchain/store/KeyValueStore.scala
pub trait KeyValueStore: Send + Sync + 'static {
    /// Enables downcasting to a concrete store type (e.g. `LmdbKeyValueStore`)
    /// so callers holding a store behind `Arc<dyn KeyValueStore>` can detect
    /// and batch same-environment LMDB writes — see
    /// `lmdb_key_value_store::batched_put`. Has no default body (trait
    /// objects can't provide one); every implementor must define it as
    /// `fn as_any(&self) -> &dyn std::any::Any { self }`.
    fn as_any(&self) -> &dyn std::any::Any;

    fn get(&self, keys: &Vec<ByteBuffer>) -> Result<Vec<Option<ByteBuffer>>, KvStoreError>;

    fn put(&self, kv_pairs: Vec<(ByteBuffer, ByteBuffer)>) -> Result<(), KvStoreError>;

    /// Atomically insert one key/value pair, returning false when the key already exists.
    fn put_one_if_absent(&self, key: ByteBuffer, value: ByteBuffer) -> Result<bool, KvStoreError>;

    fn delete(&self, keys: Vec<ByteBuffer>) -> Result<usize, KvStoreError>;

    fn iterate(&self, f: fn(ByteBuffer, ByteBuffer)) -> Result<(), KvStoreError>;
    fn iterate_while(
        &self,
        f: &mut dyn FnMut(ByteBuffer, ByteBuffer) -> Result<bool, KvStoreError>,
    ) -> Result<(), KvStoreError>;

    fn clone_box(&self) -> Box<dyn KeyValueStore>;

    fn to_map(&self) -> Result<BTreeMap<ByteBuffer, ByteBuffer>, KvStoreError>;

    fn print_store(&self) -> Result<(), KvStoreError>;

    /// Check if the store contains any entries. O(1) time and space.
    fn non_empty(&self) -> Result<bool, KvStoreError>;

    fn contains(&self, keys: &Vec<ByteBuffer>) -> Result<Vec<bool>, KvStoreError> {
        let results = self.get(keys)?;

        Ok(results.into_iter().map(|result| result.is_some()).collect())
    }

    // See shared/src/main/scala/coop/rchain/store/KeyValueStoreSyntax.scala

    fn get_one(&self, key: &ByteBuffer) -> Result<Option<ByteBuffer>, KvStoreError> {
        let values = self.get(&vec![key.to_vec()])?;

        match values.split_first() {
            Some((first_value, _)) => Ok(first_value.clone()),
            None => Ok(None),
        }
    }

    fn put_one(&self, key: ByteBuffer, value: ByteBuffer) -> Result<(), KvStoreError> {
        self.put(vec![(key, value)])
    }

    fn put_if_absent(&self, kv_pairs: Vec<(ByteBuffer, ByteBuffer)>) -> Result<(), KvStoreError> {
        let keys: Vec<ByteBuffer> = kv_pairs.iter().map(|(k, _)| k.clone()).collect();
        let if_absent = self.contains(&keys)?;
        let kv_if_absent: Vec<_> = kv_pairs.into_iter().zip(if_absent).collect();
        let kv_absent: Vec<_> = kv_if_absent
            .clone()
            .into_iter()
            .filter(|(_, is_present)| !is_present)
            .map(|(kv, _)| kv)
            .collect();

        self.put(kv_absent)
    }

    fn size_bytes(&self) -> usize;
}

impl Clone for Box<dyn KeyValueStore> {
    fn clone(&self) -> Box<dyn KeyValueStore> { self.clone_box() }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum KvStoreError {
    KeyNotFound(String),
    IoError(String),
    SerializationError(String),
    InvalidArgument(String),
    LockError(String),
    /// Returned when a DAG representation is requested before the
    /// approved-block / last-finalized-block bootstrap has completed.
    LastFinalizedBlockUninitialized,
    /// A block the DAG index does not hold, carried as bytes so the caller can
    /// request it. Distinct from [`KvStoreError::KeyNotFound`], which means a
    /// store lost a value its index still points at: this one means the index
    /// never had the block, the normal condition of a node restored from a sync
    /// anchor. Callers that judge blocks must be able to tell the two apart.
    MissingBlock {
        hash: prost::bytes::Bytes,
        context: String,
    },
}

impl std::fmt::Display for KvStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            KvStoreError::KeyNotFound(key) => write!(f, "Key not found: {}", key),
            KvStoreError::IoError(e) => write!(f, "I/O error: {}", e),
            KvStoreError::SerializationError(e) => write!(f, "SerializationError error: {}", e),
            KvStoreError::InvalidArgument(e) => write!(f, "Invalid argument: {}", e),
            KvStoreError::LockError(e) => write!(f, "Lock error: {}", e),
            KvStoreError::LastFinalizedBlockUninitialized => write!(
                f,
                "DagState does not contain lastFinalizedBlock (bootstrap incomplete)"
            ),
            KvStoreError::MissingBlock { hash, context } => {
                write!(
                    f,
                    "DAG storage is missing hash {}{}",
                    hex::encode(hash),
                    context
                )
            }
        }
    }
}

impl From<heed::Error> for KvStoreError {
    fn from(error: heed::Error) -> Self { KvStoreError::IoError(error.to_string()) }
}

impl From<Box<bincode::ErrorKind>> for KvStoreError {
    fn from(error: Box<bincode::ErrorKind>) -> Self {
        KvStoreError::SerializationError(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_formats_each_variant() {
        assert_eq!(
            KvStoreError::KeyNotFound("k1".to_string()).to_string(),
            "Key not found: k1"
        );
        assert_eq!(
            KvStoreError::IoError("disk gone".to_string()).to_string(),
            "I/O error: disk gone"
        );
        assert_eq!(
            KvStoreError::SerializationError("bad bytes".to_string()).to_string(),
            "SerializationError error: bad bytes"
        );
        assert_eq!(
            KvStoreError::InvalidArgument("nope".to_string()).to_string(),
            "Invalid argument: nope"
        );
        assert_eq!(
            KvStoreError::LockError("poisoned".to_string()).to_string(),
            "Lock error: poisoned"
        );
        assert_eq!(
            KvStoreError::LastFinalizedBlockUninitialized.to_string(),
            "DagState does not contain lastFinalizedBlock (bootstrap incomplete)"
        );
        assert_eq!(
            KvStoreError::MissingBlock {
                hash: prost::bytes::Bytes::from_static(&[0xab, 0xcd]),
                context: " while merging".to_string(),
            }
            .to_string(),
            "DAG storage is missing hash abcd while merging"
        );
    }

    #[test]
    fn bincode_errors_convert_to_serialization_errors() {
        let bincode_err = bincode::deserialize::<String>(&[0xff]).unwrap_err();
        let converted: KvStoreError = bincode_err.into();
        assert!(matches!(converted, KvStoreError::SerializationError(_)));
    }

    #[test]
    fn heed_errors_convert_to_io_errors() {
        let heed_err = heed::Error::Io(std::io::Error::other("mmap failure"));
        let converted: KvStoreError = heed_err.into();
        assert_eq!(converted, KvStoreError::IoError("mmap failure".to_string()));
    }
}
