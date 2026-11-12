use std::hash::Hash;
use std::sync::{Arc, Mutex, MutexGuard};

use shared::rust::ByteVector;

use super::cold_store::ColdLeafAddress;
use super::history_reader::HistoryReader;
use super::history_repository::HistoryRepository;
use super::instances::rspace_history_reader_impl::RSpaceHistoryReaderImpl;
use super::roots_store::CurrentRoot;
use crate::rspace::errors::{AuthorityBackendMismatch, HistoryError, RootError};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::hot_store_action::HotStoreAction;
use crate::rspace::hot_store_trie_action::HotStoreTrieAction;
use crate::rspace::persistence::{
    DurableRSpaceAuthorityV1, DurableStateReadHandleErrorV1, DurableStateRootV1,
    DurableViewReaderV1, SemanticProfile,
};
use crate::rspace::shared::trie_exporter::{
    AuthenticatedTraversalPage, KeyHash, NodePath, StateExportError, TrieExporter, Value,
};
use crate::rspace::state::import_session::{StateImportError, StateImportSession, StateImportSpec};
use crate::rspace::state::rspace_exporter::RSpaceExporter;
use crate::rspace::state::rspace_importer::RSpaceImporter;

type SharedDurableOwnerV1<P> = Arc<Mutex<DurableRSpaceAuthorityV1<P>>>;

type DurableHistoryRepositoryObjectV1<P> = Box<
    dyn HistoryRepository<
            <P as SemanticProfile>::Channel,
            <P as SemanticProfile>::Pattern,
            <P as SemanticProfile>::Datum,
            <P as SemanticProfile>::Continuation,
        > + Send
        + Sync
        + 'static,
>;

pub(crate) struct DurableHistoryRepositoryV1<P: SemanticProfile>
where P::Channel: Clone + Hash + Eq
{
    owner: SharedDurableOwnerV1<P>,
    root: DurableStateRootV1,
    genesis: DurableStateRootV1,
    reader: DurableViewReaderV1<P>,
    exporter: Arc<dyn RSpaceExporter>,
    importer: Arc<dyn RSpaceImporter>,
}

impl<P: SemanticProfile> DurableHistoryRepositoryV1<P>
where P::Channel: Clone + Hash + Eq
{
    pub(crate) fn new(owner: SharedDurableOwnerV1<P>) -> Result<Self, HistoryError> {
        let (root, reader) = {
            let current = lock_owner(&owner)?;
            pinned_state(&current, current.current_root())?
        };
        Ok(Self {
            exporter: Arc::new(DurableRSpaceExporterV1 {
                owner: Arc::clone(&owner),
            }),
            importer: Arc::new(DurableRSpaceImporterV1),
            genesis: DurableStateRootV1::store_genesis::<P>(),
            owner,
            root,
            reader,
        })
    }

    pub(crate) fn reset_view(&self, root: &Blake2b256Hash) -> Result<Self, HistoryError> {
        let target = DurableStateRootV1::try_from(root)?;
        let (root, reader) = {
            let current = lock_owner(&self.owner)?;
            pinned_state(&current, target)?
        };
        Ok(Self {
            owner: Arc::clone(&self.owner),
            root,
            genesis: self.genesis,
            reader,
            exporter: Arc::clone(&self.exporter),
            importer: Arc::clone(&self.importer),
        })
    }

    pub(crate) fn reader(&self) -> &DurableViewReaderV1<P> { &self.reader }
}

fn lock_owner<P: SemanticProfile>(
    owner: &Mutex<DurableRSpaceAuthorityV1<P>>,
) -> Result<MutexGuard<'_, DurableRSpaceAuthorityV1<P>>, RootError>
where P::Channel: Clone + Hash + Eq {
    owner
        .lock()
        .map_err(|_| RootError::KvStoreError("durable RSpace owner lock poisoned".to_owned()))
}

fn pinned_state<P: SemanticProfile>(
    owner: &DurableRSpaceAuthorityV1<P>,
    root: DurableStateRootV1,
) -> Result<(DurableStateRootV1, DurableViewReaderV1<P>), HistoryError>
where
    P::Channel: Clone + Hash + Eq,
{
    let (resolved, handle) = owner
        .resolve_read_handle(root)
        .map_err(|refusal| match refusal {
            DurableStateReadHandleErrorV1::Root(error) => HistoryError::RootError(error),
            lease => HistoryError::RootError(RootError::KvStoreError(lease.to_string())),
        })?;
    Ok((resolved.root(), DurableViewReaderV1::new(handle, resolved.roots())))
}

impl<P: SemanticProfile> HistoryRepository<P::Channel, P::Pattern, P::Datum, P::Continuation>
    for DurableHistoryRepositoryV1<P>
where P::Channel: Clone + Hash + Eq
{
    fn checkpoint(
        &self,
        _actions: Vec<HotStoreAction<P::Channel, P::Pattern, P::Datum, P::Continuation>>,
    ) -> Result<DurableHistoryRepositoryObjectV1<P>, HistoryError> {
        Err(HistoryError::BackendMismatch(AuthorityBackendMismatch::History))
    }

    fn checkpoint_at(
        &self,
        _expected: &CurrentRoot,
        _actions: Vec<HotStoreAction<P::Channel, P::Pattern, P::Datum, P::Continuation>>,
    ) -> Result<DurableHistoryRepositoryObjectV1<P>, HistoryError> {
        Err(HistoryError::BackendMismatch(AuthorityBackendMismatch::Roots))
    }

    fn current_authority_root(&self) -> Result<CurrentRoot, HistoryError> {
        Err(HistoryError::BackendMismatch(AuthorityBackendMismatch::Roots))
    }

    fn do_checkpoint(
        &self,
        _actions: Vec<HotStoreTrieAction<P::Channel, P::Pattern, P::Datum, P::Continuation>>,
    ) -> Result<DurableHistoryRepositoryObjectV1<P>, HistoryError> {
        Err(HistoryError::BackendMismatch(AuthorityBackendMismatch::History))
    }

    fn reset(
        &self,
        root: &Blake2b256Hash,
    ) -> Result<DurableHistoryRepositoryObjectV1<P>, HistoryError> {
        Ok(Box::new(self.reset_view(root)?))
    }

    fn exporter(&self) -> Arc<dyn RSpaceExporter> { Arc::clone(&self.exporter) }

    fn importer(&self) -> Arc<dyn RSpaceImporter> { Arc::clone(&self.importer) }

    fn get_history_reader(
        &self,
        _state_hash: &Blake2b256Hash,
    ) -> Result<
        Box<dyn HistoryReader<Blake2b256Hash, P::Channel, P::Pattern, P::Datum, P::Continuation>>,
        HistoryError,
    > {
        Err(HistoryError::BackendMismatch(AuthorityBackendMismatch::History))
    }

    fn get_history_reader_struct(
        &self,
        _state_hash: &Blake2b256Hash,
    ) -> Result<
        RSpaceHistoryReaderImpl<P::Channel, P::Pattern, P::Datum, P::Continuation>,
        HistoryError,
    > {
        Err(HistoryError::BackendMismatch(AuthorityBackendMismatch::History))
    }

    fn root(&self) -> Blake2b256Hash { self.root.into() }

    fn empty_root(&self) -> Blake2b256Hash { self.genesis.into() }

    fn contains_verified_root(&self, root: &Blake2b256Hash) -> Result<bool, HistoryError> {
        let target = DurableStateRootV1::try_from(root)?;
        match lock_owner(&self.owner)?.resolve(target) {
            Ok(_) => Ok(true),
            Err(RootError::UnknownRootError(_)) => Ok(false),
            Err(error) => Err(HistoryError::RootError(error)),
        }
    }
}

struct DurableRSpaceExporterV1<P: SemanticProfile>
where P::Channel: Clone + Hash + Eq
{
    owner: SharedDurableOwnerV1<P>,
}

impl<P: SemanticProfile> RSpaceExporter for DurableRSpaceExporterV1<P>
where P::Channel: Clone + Hash + Eq
{
    fn get_root(&self) -> Result<KeyHash, RootError> {
        Ok(lock_owner(&self.owner)?.current_root().into())
    }
}

impl<P: SemanticProfile> TrieExporter for DurableRSpaceExporterV1<P>
where P::Channel: Clone + Hash + Eq
{
    fn get_page(
        &self,
        _start_path: NodePath,
        _skip: i32,
        _take: i32,
    ) -> Result<AuthenticatedTraversalPage<KeyHash>, StateExportError> {
        Err(StateExportError::BackendMismatch(AuthorityBackendMismatch::History))
    }

    fn get_history_items(
        &self,
        _keys: Vec<KeyHash>,
    ) -> Result<Vec<(KeyHash, Value)>, StateExportError> {
        Err(StateExportError::BackendMismatch(AuthorityBackendMismatch::History))
    }

    fn get_data_items(
        &self,
        _addresses: Vec<ColdLeafAddress>,
    ) -> Result<Vec<(KeyHash, Value)>, StateExportError> {
        Err(StateExportError::BackendMismatch(AuthorityBackendMismatch::Cold))
    }
}

struct DurableRSpaceImporterV1;

impl RSpaceImporter for DurableRSpaceImporterV1 {
    fn begin_import(
        &self,
        _spec: StateImportSpec,
    ) -> Result<Box<dyn StateImportSession>, StateImportError> {
        Err(StateImportError::BackendMismatch(AuthorityBackendMismatch::History))
    }

    fn get_history_item(
        &self,
        _hash: Blake2b256Hash,
    ) -> Result<Option<ByteVector>, StateImportError> {
        Err(StateImportError::BackendMismatch(AuthorityBackendMismatch::History))
    }
}
