use std::sync::Arc;

use arc_swap::ArcSwap;
use block_storage::rust::dag::block_dag_key_value_storage::{
    BlockDagKeyValueStorage, KeyValueDagRepresentation,
};
use models::rust::block_hash::BlockHash;
use prost::bytes::Bytes;

use crate::rust::errors::CasperError;
use crate::rust::validator_identity::ValidatorIdentity;

struct CachedBond {
    lfb_hash: BlockHash,
    bonded: bool,
}

#[derive(Clone)]
pub struct FinalizedBondCache {
    validator: Option<Bytes>,
    entry: Arc<ArcSwap<CachedBond>>,
}

impl FinalizedBondCache {
    fn read(
        dag: &KeyValueDagRepresentation,
        validator: Option<&Bytes>,
    ) -> Result<CachedBond, CasperError> {
        let lfb_hash = dag.last_finalized_block();
        let metadata = dag.lookup_unsafe(&lfb_hash)?;
        let bonded = validator
            .map(|key| metadata.weight_map.contains_key(key))
            .unwrap_or(false);
        Ok(CachedBond { lfb_hash, bonded })
    }

    pub fn from_storage(
        storage: &BlockDagKeyValueStorage,
        validator: Option<&ValidatorIdentity>,
    ) -> Result<Self, CasperError> {
        let dag = storage.get_representation()?;
        let validator = validator.map(|id| id.public_key.bytes.clone());
        let entry = Self::read(&dag, validator.as_ref())?;
        Ok(Self {
            validator,
            entry: Arc::new(ArcSwap::from_pointee(entry)),
        })
    }

    pub fn is_bonded(&self, dag: &KeyValueDagRepresentation) -> Result<bool, CasperError> {
        let entry = self.entry.load();
        if entry.lfb_hash == dag.last_finalized_block() {
            Ok(entry.bonded)
        } else {
            Ok(Self::read(dag, self.validator.as_ref())?.bonded)
        }
    }

    pub fn refresh(&self, storage: &BlockDagKeyValueStorage) -> Result<(), CasperError> {
        let dag = storage.get_representation()?;
        self.entry
            .store(Arc::new(Self::read(&dag, self.validator.as_ref())?));
        Ok(())
    }
}
