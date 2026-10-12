use std::sync::Arc;

use shared::rust::ByteBuffer;
use shared::rust::store::key_value_store::{KeyValueStore, KvStoreError};
use shared::rust::store::lmdb_key_value_store::batched_put;

use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::roots_store::root_record_kvs;

pub trait CheckpointWriter: Send + Sync {
    fn write(
        &self,
        nodes: Vec<(ByteBuffer, ByteBuffer)>,
        root: &Blake2b256Hash,
    ) -> Result<(), KvStoreError>;
}

pub struct KvCheckpointWriter {
    pub history: Arc<dyn KeyValueStore>,
    pub roots: Arc<dyn KeyValueStore>,
}

impl CheckpointWriter for KvCheckpointWriter {
    fn write(
        &self,
        nodes: Vec<(ByteBuffer, ByteBuffer)>,
        root: &Blake2b256Hash,
    ) -> Result<(), KvStoreError> {
        batched_put(vec![
            (self.history.as_ref(), nodes),
            (self.roots.as_ref(), root_record_kvs(root)),
        ])
    }
}
