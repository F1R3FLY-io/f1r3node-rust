use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;

use crate::rust::ByteBuffer;

// See shared/src/main/scala/coop/rchain/store/KeyValueStore.scala
pub trait KeyValueStore: Send + Sync {
    fn get(&self, keys: &Vec<ByteBuffer>) -> Result<Vec<Option<ByteBuffer>>, KvStoreError>;

    fn put(&self, kv_pairs: Vec<(ByteBuffer, ByteBuffer)>) -> Result<(), KvStoreError>;

    fn compare_and_swap_batch(
        &self,
        conditions: &[(ByteBuffer, Option<ByteBuffer>)],
        mutations: &[KeyValueStoreMutation],
    ) -> Result<bool, KvStoreError> {
        validate_atomic_batch(conditions, mutations)?;
        Err(KvStoreError::InvalidArgument(
            "atomic compare-and-swap is not supported by this key-value store".to_string(),
        ))
    }

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

    fn put_if_absent_or_verify_equal(
        &self,
        kv_pairs: Vec<(ByteBuffer, ByteBuffer)>,
    ) -> Result<(), KvStoreError> {
        let mut requested = BTreeMap::new();
        for (key, value) in kv_pairs {
            match requested.get(&key) {
                Some(existing) if existing != &value => {
                    return Err(KvStoreError::InvalidArgument(
                        "put-if-absent batch assigns different values to the same key".to_string(),
                    ));
                }
                Some(_) => {}
                None => {
                    requested.insert(key, value);
                }
            }
        }

        let keys: Vec<_> = requested.keys().cloned().collect();
        loop {
            let observed = self.get(&keys)?;
            if observed.len() != keys.len() {
                return Err(KvStoreError::IoError(format!(
                    "key-value store returned {} values for {} requested keys",
                    observed.len(),
                    keys.len()
                )));
            }
            let mut conditions = Vec::with_capacity(keys.len());
            let mut mutations = Vec::new();

            for ((key, value), actual) in requested.iter().zip(observed) {
                if let Some(actual_value) = actual.as_ref() {
                    if actual_value != value {
                        return Err(KvStoreError::InvalidArgument(format!(
                            "put-if-absent found a different value for key {}",
                            hex::encode(key)
                        )));
                    }
                } else {
                    mutations.push(KeyValueStoreMutation::Put(key.clone(), value.clone()));
                }
                conditions.push((key.clone(), actual));
            }

            if mutations.is_empty() || self.compare_and_swap_batch(&conditions, &mutations)? {
                return Ok(());
            }
        }
    }

    fn size_bytes(&self) -> Result<usize, KvStoreError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyValueStoreMutation {
    Put(ByteBuffer, ByteBuffer),
    Delete(ByteBuffer),
}

impl KeyValueStoreMutation {
    pub(crate) fn key(&self) -> &ByteBuffer {
        match self {
            Self::Put(key, _) | Self::Delete(key) => key,
        }
    }
}

pub fn validate_atomic_batch(
    conditions: &[(ByteBuffer, Option<ByteBuffer>)],
    mutations: &[KeyValueStoreMutation],
) -> Result<(), KvStoreError> {
    let mut condition_keys = BTreeSet::new();
    for (key, _) in conditions {
        if !condition_keys.insert(key.as_slice()) {
            return Err(KvStoreError::InvalidArgument(
                "atomic compare-and-swap contains a duplicate condition key".to_string(),
            ));
        }
    }

    let mut mutation_keys = BTreeSet::new();
    for mutation in mutations {
        if !mutation_keys.insert(mutation.key().as_slice()) {
            return Err(KvStoreError::InvalidArgument(
                "atomic compare-and-swap contains a duplicate mutation key".to_string(),
            ));
        }
    }

    Ok(())
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
