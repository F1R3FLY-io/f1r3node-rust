pub mod block_dag_key_value_storage;
pub mod carrier_index;
pub mod block_metadata_store;
pub mod dag_history_index;
pub mod dag_gc_store;
pub(crate) mod dag_persistence;
pub mod deploy_lifecycle_types;
pub mod equivocation_tracker_store;
pub mod equivocations_access;
pub mod finalization_commit;
pub mod soak_snapshot;
pub mod soak_equivocations;

use serde::de::DeserializeOwned;
use serde::Serialize;
use shared::rust::store::key_value_store::KvStoreError;

fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, KvStoreError> {
    Ok(bincode::serialize(value)?)
}

fn decode<T: DeserializeOwned>(value: &[u8]) -> Result<T, KvStoreError> {
    Ok(bincode::deserialize(value)?)
}
