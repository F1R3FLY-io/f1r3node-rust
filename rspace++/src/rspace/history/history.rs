use std::sync::Arc;

use shared::rust::store::key_value_store::KeyValueStore;

use crate::rspace::errors::{HistoryError, RSpaceError};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::hashing::native_source::SourceMeter;
use crate::rspace::history::history_action::HistoryAction;
use crate::rspace::history::instances::radix_history::RadixHistory;
pub struct NativePreparedHistory {
    pub(crate) next: Box<dyn History>,
    pub(crate) store: Arc<dyn KeyValueStore>,
    pub(crate) writes: Vec<(Vec<u8>, Vec<u8>)>,
}

// See rspace/src/main/scala/coop/rchain/rspace/history/History.scala
pub trait History: Send + Sync {
    fn read(&self, key: Vec<u8>) -> Result<Option<Vec<u8>>, HistoryError>;

    fn process(&self, actions: Vec<HistoryAction>) -> Result<Box<dyn History>, HistoryError>;

    fn prepare_native(
        &self,
        actions: Vec<HistoryAction>,
        meter: &dyn SourceMeter,
    ) -> Result<NativePreparedHistory, RSpaceError>;

    fn root_ref(&self) -> &Blake2b256Hash;

    fn root(&self) -> Blake2b256Hash { self.root_ref().clone() }

    fn reset(&self, root: &Blake2b256Hash) -> Result<Box<dyn History>, HistoryError>;

    fn snapshot_native(&self, meter: &dyn SourceMeter) -> Result<Box<dyn History>, RSpaceError>;
}

pub struct HistoryInstances;

impl HistoryInstances {
    pub fn create(
        root: Blake2b256Hash,
        store: Arc<dyn KeyValueStore>,
    ) -> Result<RadixHistory, HistoryError> {
        let typed_store = RadixHistory::create_store(store.to_owned());
        RadixHistory::create(root, typed_store)
    }
}
