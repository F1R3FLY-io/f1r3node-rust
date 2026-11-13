use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::{
    BlockDagKeyValueStorage, KeyValueDagRepresentation,
};
use prost::bytes::Bytes;

use crate::rust::errors::CasperError;
use crate::rust::validator_identity::ValidatorIdentity;

/// Whether this node's validator is bonded at the persisted LFB. The finalizer
/// refreshes it after every cycle, so it trails the LFB only while the cycle
/// that moved the LFB finishes.
#[derive(Clone)]
pub struct FinalizedBondCache {
    validator: Option<Bytes>,
    bonded: Arc<AtomicBool>,
}

impl FinalizedBondCache {
    fn read(
        dag: &KeyValueDagRepresentation,
        validator: Option<&Bytes>,
    ) -> Result<bool, CasperError> {
        let metadata = dag.lookup_unsafe(&dag.last_finalized_block())?;
        Ok(validator.is_some_and(|key| metadata.weight_map.contains_key(key)))
    }

    pub fn from_storage(
        storage: &BlockDagKeyValueStorage,
        validator: Option<&ValidatorIdentity>,
    ) -> Result<Self, CasperError> {
        let dag = storage.get_representation()?;
        let validator = validator.map(|id| id.public_key.bytes.clone());
        let bonded = Self::read(&dag, validator.as_ref())?;
        Ok(Self {
            validator,
            bonded: Arc::new(AtomicBool::new(bonded)),
        })
    }

    pub fn is_bonded(&self) -> bool { self.bonded.load(Ordering::Acquire) }

    pub fn refresh(&self, storage: &BlockDagKeyValueStorage) -> Result<(), CasperError> {
        let dag = storage.get_representation()?;
        self.bonded.store(
            Self::read(&dag, self.validator.as_ref())?,
            Ordering::Release,
        );
        Ok(())
    }
}
