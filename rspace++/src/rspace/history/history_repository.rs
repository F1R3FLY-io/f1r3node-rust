use std::marker::PhantomData;
use std::sync::Arc;

use super::instances::rspace_history_reader_impl::RSpaceHistoryReaderImpl;
use crate::rspace::errors::{HistoryError, HistoryRepositoryError};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::hashing::stable_hash_provider::StableHashSerialize;
use crate::rspace::history::history::HistoryInstances;
use crate::rspace::history::history_reader::HistoryReader;
use crate::rspace::history::history_repository_impl::HistoryRepositoryImpl;
use crate::rspace::history::instances::radix_history::RadixHistory;
use crate::rspace::history::roots_store::CurrentRoot;
use crate::rspace::hot_store_action::HotStoreAction;
use crate::rspace::hot_store_trie_action::HotStoreTrieAction;
use crate::rspace::serializers::cold_store_decode::ColdStoreDecode;
use crate::rspace::state::instances::rspace_exporter_store::RSpaceExporterStore;
use crate::rspace::state::instances::rspace_importer_store::{
    RSpaceImporterStore, record_validator,
};
use crate::rspace::state::rspace_exporter::RSpaceExporter;
use crate::rspace::state::rspace_importer::RSpaceImporter;
use crate::rspace::state::rspace_state_authority::RSpaceStateAuthority;

// See rspace/src/main/scala/coop/rchain/rspace/history/HistoryRepository.scala
pub trait HistoryRepository<C: Clone, P: Clone, A: Clone, K: Clone>: Send + Sync {
    fn checkpoint(
        &self,
        actions: Vec<HotStoreAction<C, P, A, K>>,
    ) -> Result<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>, HistoryError>;

    /// Checkpoint only if the legacy authority still names `expected`.
    /// Implementations must perform this check before transforming actions;
    /// callers use it to close the stale-root window at the legacy boundary.
    fn checkpoint_at(
        &self,
        expected: &CurrentRoot,
        actions: Vec<HotStoreAction<C, P, A, K>>,
    ) -> Result<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>, HistoryError>;

    /// Return the authenticated legacy root and generation used by the next
    /// checkpoint. Callers pass this exact value to `checkpoint_at`.
    fn current_authority_root(&self) -> Result<CurrentRoot, HistoryError>;

    fn do_checkpoint(
        &self,
        actions: Vec<HotStoreTrieAction<C, P, A, K>>,
    ) -> Result<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>, HistoryError>;

    fn reset(
        &self,
        root: &Blake2b256Hash,
    ) -> Result<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>, HistoryError>;

    fn exporter(&self) -> Arc<dyn RSpaceExporter>;

    fn importer(&self) -> Arc<dyn RSpaceImporter>;

    fn get_history_reader(
        &self,
        state_hash: &Blake2b256Hash,
    ) -> Result<Box<dyn HistoryReader<Blake2b256Hash, C, P, A, K>>, HistoryError>;

    fn get_history_reader_struct(
        &self,
        state_hash: &Blake2b256Hash,
    ) -> Result<RSpaceHistoryReaderImpl<C, P, A, K>, HistoryError>;

    fn root(&self) -> Blake2b256Hash;

    fn empty_root(&self) -> Blake2b256Hash { RadixHistory::empty_root_node_hash() }

    fn contains_verified_root(&self, root: &Blake2b256Hash) -> Result<bool, HistoryError>;

    fn contains_root(&self, root: &Blake2b256Hash) -> Result<bool, HistoryError> {
        self.contains_verified_root(root)
    }
}

pub struct HistoryRepositoryInstances<C, P, A, K> {
    _marker: PhantomData<(C, P, A, K)>,
}

pub const PREFIX_DATUM: u8 = 0x00;
pub const PREFIX_KONT: u8 = 0x01;
pub const PREFIX_JOINS: u8 = 0x02;

impl<C, P, A, K> HistoryRepositoryInstances<C, P, A, K>
where
    C: Clone + Send + Sync + StableHashSerialize + ColdStoreDecode + 'static,
    P: Clone + Send + Sync + StableHashSerialize + ColdStoreDecode + 'static,
    A: Clone + Send + Sync + StableHashSerialize + ColdStoreDecode + 'static,
    K: Clone + Send + Sync + StableHashSerialize + ColdStoreDecode + 'static,
{
    pub fn repository(
        authority: Arc<RSpaceStateAuthority>,
    ) -> Result<
        Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>,
        HistoryRepositoryError,
    > {
        let validator = record_validator::<C, P, A, K>();
        authority.initialize_for_records(&validator)?;
        let current_root = authority.current_root()?;

        tracing::debug!(
            "[HistoryRepository] lmdbRepository initialized with root={}",
            current_root
        );

        // History store
        let history = HistoryInstances::create_authorized(current_root, authority.clone())?;

        // Cold store
        // let cold_store =
        // ColdStoreInstances::cold_store(cold_key_value_store.clone());

        // RSpace importer/exporter / directly operates on Store (lmdb)
        let exporter = RSpaceExporterStore::create(authority.clone());
        let importer = RSpaceImporterStore::create_with_validator(authority.clone(), validator);
        let cold_key_value_store = authority.cold_store()?;

        Ok(Box::new(HistoryRepositoryImpl::new(
            Box::new(history),
            authority,
            cold_key_value_store,
            Arc::new(exporter),
            Arc::new(importer),
        )))
    }
}
