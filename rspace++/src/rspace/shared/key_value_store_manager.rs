use std::sync::Arc;

use async_trait::async_trait;
use shared::rust::store::key_value_store::KeyValueStore;

use crate::rspace::rspace::RSpaceStore;
use crate::rspace::state::rspace_state_authority::RSpaceStateAuthority;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RSpaceBackendSelection {
    Legacy,
    Durable,
}

// See shared/src/main/scala/coop/rchain/store/KeyValueStoreManager.scala
#[async_trait]
pub trait KeyValueStoreManager: Send + Sync {
    async fn store(&mut self, name: String) -> Result<Arc<dyn KeyValueStore>, heed::Error>;

    async fn shutdown(&mut self) -> Result<(), heed::Error>;

    async fn state_authority(
        &mut self,
        db_prefix: &str,
    ) -> Result<Arc<RSpaceStateAuthority>, heed::Error>;

    async fn select_rspace_backend(
        &mut self,
        db_prefix: &str,
        selection: RSpaceBackendSelection,
    ) -> Result<(), heed::Error> {
        match selection {
            RSpaceBackendSelection::Legacy => Ok(()),
            RSpaceBackendSelection::Durable => Err(heed::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "this store manager has no durable RSpace backend to select for prefix \
                     {db_prefix}"
                ),
            ))),
        }
    }

    async fn r_space_stores(&mut self) -> Result<RSpaceStore, heed::Error> {
        self.get_stores("rspace").await
    }

    async fn eval_stores(&mut self) -> Result<RSpaceStore, heed::Error> {
        self.get_stores("eval").await
    }

    // TODO: This function should be private
    async fn get_stores(&mut self, db_prefix: &str) -> Result<RSpaceStore, heed::Error> {
        Ok(RSpaceStore::new(self.state_authority(db_prefix).await?))
    }
}
