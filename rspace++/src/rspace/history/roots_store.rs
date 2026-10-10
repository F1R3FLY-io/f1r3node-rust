// See rspace/src/main/scala/coop/rchain/rspace/history/RootsStore.scala

use std::sync::Arc;
use std::time::Instant;

use shared::rust::ByteBuffer;
use shared::rust::store::key_value_store::{KeyValueStore, KvStoreError};

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

    /// Pure lookup: returns true if the root has been recorded in the store.
    /// It never updates the current-root pointer. Used by joiner-side LFS sync
    /// to check whether a root has already been imported before requesting
    /// it from peers.
    fn contains_root(&self, key: &Blake2b256Hash) -> Result<bool, RootError>;
}

pub fn root_record_kvs(root: &Blake2b256Hash) -> Vec<(ByteBuffer, ByteBuffer)> {
    let root_bytes = root.bytes().to_vec();
    vec![
        (root_bytes.clone(), "tag".as_bytes().to_vec()),
        ("current-root".as_bytes().to_vec(), root_bytes),
    ]
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
                self.put(root_record_kvs(key))?;

                Ok(())
            }

            fn contains_root(&self, key: &Blake2b256Hash) -> Result<bool, RootError> {
                Ok(self.get_one(&key.bytes())?.is_some())
            }
        }

        RootsStoreInstance { store }
    }
}
