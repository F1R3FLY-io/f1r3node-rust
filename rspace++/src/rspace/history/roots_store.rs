// See rspace/src/main/scala/coop/rchain/rspace/history/RootsStore.scala

use std::sync::Arc;
use std::time::Instant;

use shared::rust::ByteBuffer;
use shared::rust::store::key_value_store::{
    AtomicStoreMutation, AtomicStoreOperation, KeyValueStore, KvStoreError, strict_atomic_mutate,
};

use crate::rspace::errors::RootError;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::metrics_constants::{
    HISTORY_ROOTS_STORE_READ_NS_METRIC, HISTORY_ROOTS_STORE_READS_METRIC,
    HISTORY_ROOTS_STORE_WRITE_NS_METRIC, HISTORY_ROOTS_STORE_WRITES_METRIC,
    HISTORY_RSPACE_METRICS_SOURCE,
};

pub trait RootsStore: Send + Sync {
    fn current_root(&self) -> Result<Option<Blake2b256Hash>, RootError>;

    fn record_root(&self, key: &Blake2b256Hash) -> Result<(), RootError>;

    fn record_root_atomic(&self, _key: &Blake2b256Hash) -> Result<(), RootError> {
        Err(KvStoreError::AtomicityUnavailable(
            "roots store does not provide atomic root recording".to_owned(),
        )
        .into())
    }

    fn supports_atomic_record(&self) -> bool { false }

    /// Pure lookup: returns true if the root has been recorded in the store.
    /// It never updates the current-root pointer. Used by joiner-side LFS sync
    /// to check whether a root has already been imported before requesting
    /// it from peers.
    fn contains_root(&self, key: &Blake2b256Hash) -> Result<bool, RootError>;
}

pub struct RootsStoreInstances;

impl RootsStoreInstances {
    pub fn roots_store(store: Arc<dyn KeyValueStore>) -> impl RootsStore {
        struct RootsStoreInstance {
            store: Arc<dyn KeyValueStore>,
        }

        impl RootsStoreInstance {
            fn get_one(&self, key: &ByteBuffer) -> Result<Option<ByteBuffer>, KvStoreError> {
                let start = Instant::now();
                let result = self.store.get_one(key);
                metrics::counter!(HISTORY_ROOTS_STORE_READ_NS_METRIC, "source" => HISTORY_RSPACE_METRICS_SOURCE)
                    .increment(start.elapsed().as_nanos() as u64);
                metrics::counter!(HISTORY_ROOTS_STORE_READS_METRIC, "source" => HISTORY_RSPACE_METRICS_SOURCE)
                    .increment(1);
                result
            }

            fn put(&self, kv_pairs: Vec<(ByteBuffer, ByteBuffer)>) -> Result<(), KvStoreError> {
                let start = Instant::now();
                let result = self.store.put(kv_pairs);
                metrics::counter!(HISTORY_ROOTS_STORE_WRITE_NS_METRIC, "source" => HISTORY_RSPACE_METRICS_SOURCE)
                    .increment(start.elapsed().as_nanos() as u64);
                metrics::counter!(HISTORY_ROOTS_STORE_WRITES_METRIC, "source" => HISTORY_RSPACE_METRICS_SOURCE)
                    .increment(1);
                result
            }
        }

        impl RootsStore for RootsStoreInstance {
            fn current_root(&self) -> Result<Option<Blake2b256Hash>, RootError> {
                let current_root_name: ByteBuffer = "current-root".as_bytes().to_vec();

                let bytes = self.get_one(&current_root_name)?;

                let maybe_decoded = bytes.map(Blake2b256Hash::from_bytes);

                Ok(maybe_decoded)
            }

            fn record_root(&self, key: &Blake2b256Hash) -> Result<(), RootError> {
                let tag: ByteBuffer = "tag".as_bytes().to_vec();
                let current_root_name: ByteBuffer = "current-root".as_bytes().to_vec();
                let key_bytes = key.bytes();

                self.put(vec![(key_bytes.to_vec(), tag), (current_root_name, key_bytes.to_vec())])?;

                Ok(())
            }

            fn record_root_atomic(&self, key: &Blake2b256Hash) -> Result<(), RootError> {
                let bytes = key.bytes();
                strict_atomic_mutate(&[
                    AtomicStoreMutation {
                        store: self.store.as_ref(),
                        key: bytes.clone(),
                        operation: AtomicStoreOperation::Put(b"tag".to_vec()),
                    },
                    AtomicStoreMutation {
                        store: self.store.as_ref(),
                        key: b"current-root".to_vec(),
                        operation: AtomicStoreOperation::Put(bytes),
                    },
                ])?;
                Ok(())
            }

            fn supports_atomic_record(&self) -> bool { self.store.supports_strict_atomic_mutate() }

            fn contains_root(&self, key: &Blake2b256Hash) -> Result<bool, RootError> {
                Ok(self.get_one(&key.bytes())?.is_some())
            }
        }

        RootsStoreInstance { store }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;

    #[test]
    fn native_root_record_publishes_tag_and_current_pointer_together() {
        let store = Arc::new(InMemoryKeyValueStore::new());
        let roots = RootsStoreInstances::roots_store(store.clone());
        let root = Blake2b256Hash::new(b"native-root");
        assert!(roots.supports_atomic_record());
        roots.record_root_atomic(&root).unwrap();
        assert_eq!(store.get_one(&root.bytes()).unwrap(), Some(b"tag".to_vec()));
        assert_eq!(store.get_one(&b"current-root".to_vec()).unwrap(), Some(root.bytes()));
        assert_eq!(roots.current_root().unwrap(), Some(root));
    }
}
