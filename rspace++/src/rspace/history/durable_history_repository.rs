use std::hash::Hash;
use std::sync::Arc;

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
    DurablePublishedCoreV1, DurableStateReadHandleErrorV1, DurableStateRootV1, DurableViewReaderV1,
    SemanticProfile,
};
use crate::rspace::shared::trie_exporter::{
    AuthenticatedTraversalPage, KeyHash, NodePath, StateExportError, TrieExporter, Value,
};
use crate::rspace::state::import_session::{StateImportError, StateImportSession, StateImportSpec};
use crate::rspace::state::rspace_exporter::RSpaceExporter;
use crate::rspace::state::rspace_importer::RSpaceImporter;

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
    published: DurablePublishedCoreV1<P>,
    root: DurableStateRootV1,
    genesis: DurableStateRootV1,
    reader: DurableViewReaderV1<P>,
    exporter: Arc<dyn RSpaceExporter>,
    importer: Arc<dyn RSpaceImporter>,
}

impl<P: SemanticProfile> DurableHistoryRepositoryV1<P>
where P::Channel: Clone + Hash + Eq
{
    pub(crate) fn new(published: DurablePublishedCoreV1<P>) -> Result<Self, HistoryError> {
        let (root, reader) = pinned_state(&published, None)?;
        Ok(Self {
            exporter: Arc::new(DurableRSpaceExporterV1 {
                published: published.clone(),
            }),
            importer: Arc::new(DurableRSpaceImporterV1),
            genesis: DurableStateRootV1::store_genesis::<P>(),
            published,
            root,
            reader,
        })
    }

    pub(crate) fn reset_view(&self, root: &Blake2b256Hash) -> Result<Self, HistoryError> {
        let target = DurableStateRootV1::try_from(root)?;
        let (root, reader) = pinned_state(&self.published, Some(target))?;
        Ok(Self {
            published: self.published.clone(),
            root,
            genesis: self.genesis,
            reader,
            exporter: Arc::clone(&self.exporter),
            importer: Arc::clone(&self.importer),
        })
    }

    pub(crate) fn reader(&self) -> &DurableViewReaderV1<P> { &self.reader }
}

fn history_error(refusal: DurableStateReadHandleErrorV1) -> HistoryError {
    match refusal {
        DurableStateReadHandleErrorV1::Root(error) => HistoryError::RootError(error),
        lease => HistoryError::RootError(RootError::KvStoreError(lease.to_string())),
    }
}

fn pinned_state<P: SemanticProfile>(
    published: &DurablePublishedCoreV1<P>,
    target: Option<DurableStateRootV1>,
) -> Result<(DurableStateRootV1, DurableViewReaderV1<P>), HistoryError> {
    let (resolved, handle) = published.read_handle(target).map_err(history_error)?;
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
        match self.published.read_handle(Some(target)) {
            Ok(_) => Ok(true),
            Err(DurableStateReadHandleErrorV1::Root(RootError::UnknownRootError(_))) => Ok(false),
            Err(refusal) => Err(history_error(refusal)),
        }
    }
}

struct DurableRSpaceExporterV1<P: SemanticProfile>
where P::Channel: Clone + Hash + Eq
{
    published: DurablePublishedCoreV1<P>,
}

impl<P: SemanticProfile> RSpaceExporter for DurableRSpaceExporterV1<P>
where P::Channel: Clone + Hash + Eq
{
    fn get_root(&self) -> Result<KeyHash, RootError> { Ok(self.published.current_root().into()) }
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
