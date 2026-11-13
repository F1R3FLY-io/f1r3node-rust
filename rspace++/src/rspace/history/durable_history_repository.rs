use std::cmp::Reverse;
use std::hash::Hash;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use shared::rust::store::key_value_store::KvStoreError;
use shared::rust::{Byte, ByteVector};

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
    DurableExportSessionV1, DurablePublishedCoreV1, DurableStateReadHandleErrorV1,
    DurableStateRootV1, DurableViewReaderV1, SemanticProfile, TransferExportErrorV1,
    TransferPageLimitsV1, TransferPageViewV1, TransferTokenV1,
};
use crate::rspace::shared::trie_exporter::{
    AuthenticatedTraversalPage, KeyHash, NodePath, StateExportError, TrieExporter, Value,
    dag_transfer_cursor, parse_dag_transfer_cursor,
};
use crate::rspace::state::import_session::{StateImportError, StateImportSession, StateImportSpec};
use crate::rspace::state::rspace_exporter::{
    RSpaceExporter, StateTransferFormat, StateTransferPage,
};
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
            exporter: Arc::new(DurableRSpaceExporterV1::new(published.clone())),
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

pub(crate) const DAG_TRANSFER_PAGE_MAX_BYTES: u64 = 64 * 1024 * 1024;
pub(crate) const DAG_TRANSFER_PAGE_MAX_OBJECTS: u64 = 1_024;
pub(crate) const DAG_TRANSFER_SESSION_LIMIT: usize = 4;

struct DagTransferSessionV1<P: SemanticProfile> {
    root: DurableStateRootV1,
    session: DurableExportSessionV1<P>,
    served_end: u64,
    complete: bool,
    cached_position: u64,
    next_position: u64,
    last_used: u64,
}

impl<P: SemanticProfile> DagTransferSessionV1<P> {
    fn serves(&self, root: DurableStateRootV1, position: u64) -> bool {
        self.root == root && (self.complete || self.served_end >= position)
    }

    fn produce(
        &mut self,
        token: TransferTokenV1,
        limits: TransferPageLimitsV1,
    ) -> Result<(TransferTokenV1, &[u8]), StateExportError> {
        let page = self.session.page(token).map_err(transfer_refusal)?;
        let view = TransferPageViewV1::parse(page, limits).map_err(|malformed| {
            StateExportError::Store(KvStoreError::SerializationError(malformed.to_string()))
        })?;
        let next = view.next_token();
        self.cached_position = token.position();
        self.next_position = next.position();
        self.served_end = self.served_end.max(next.position());
        self.complete |= view.object_count() == 0;
        Ok((next, page))
    }

    fn advance_from_start(
        &mut self,
        position: u64,
        limits: TransferPageLimitsV1,
    ) -> Result<(), StateExportError> {
        let mut token = self.session.first_token();
        while !self.complete && self.served_end < position {
            (token, _) = self.produce(token, limits)?;
        }
        Ok(())
    }

    fn page_at(
        &mut self,
        position: u64,
        limits: TransferPageLimitsV1,
    ) -> Result<(Vec<u8>, u64, bool), StateExportError> {
        match position <= self.served_end {
            true => {
                let token = TransferTokenV1::new(self.session.binding(), position);
                let (next, page) = self.produce(token, limits)?;
                Ok((page.to_vec(), next.position(), next.position() == position))
            }
            false => Err(StateExportError::InvalidPath(format!(
                "DAG transfer position {position} lies beyond the {} objects of the state",
                self.served_end
            ))),
        }
    }
}

fn transfer_refusal(refusal: TransferExportErrorV1) -> StateExportError {
    match refusal {
        oversized @ TransferExportErrorV1::ObjectExceedsPage { .. } => {
            StateExportError::Store(KvStoreError::SerializationError(oversized.to_string()))
        }
        other => StateExportError::Store(KvStoreError::IoError(other.to_string())),
    }
}

fn open_refusal(refusal: DurableStateReadHandleErrorV1, root: &KeyHash) -> StateExportError {
    match refusal {
        DurableStateReadHandleErrorV1::Root(RootError::UnknownRootError(_)) => {
            StateExportError::MissingHistoryObject(root.clone())
        }
        other => StateExportError::Store(KvStoreError::IoError(other.to_string())),
    }
}

enum DagTransferSlotV1<P: SemanticProfile> {
    Vacant,
    Idle(Box<DagTransferSessionV1<P>>),
    Busy,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum DagTransferSlotFitV1 {
    Replaceable,
    Vacant,
    Served,
    Positioned,
}

impl<P: SemanticProfile> DagTransferSlotV1<P> {
    fn fit(
        &self,
        root: DurableStateRootV1,
        position: u64,
    ) -> Option<(DagTransferSlotFitV1, Reverse<u64>)> {
        match self {
            Self::Busy => None,
            Self::Vacant => Some((DagTransferSlotFitV1::Vacant, Reverse(0))),
            Self::Idle(held) if held.serves(root, position) => {
                match (position == held.cached_position, position == held.next_position) {
                    (false, false) => Some((DagTransferSlotFitV1::Served, Reverse(held.last_used))),
                    _ => Some((DagTransferSlotFitV1::Positioned, Reverse(held.last_used))),
                }
            }
            Self::Idle(held) => Some((DagTransferSlotFitV1::Replaceable, Reverse(held.last_used))),
        }
    }
}

struct DagTransferSlotsV1<P: SemanticProfile> {
    entries: [DagTransferSlotV1<P>; DAG_TRANSFER_SESSION_LIMIT],
    uses: u64,
}

struct DagTransferSessionSetV1<P: SemanticProfile> {
    slots: Mutex<DagTransferSlotsV1<P>>,
    released: Condvar,
}

impl<P: SemanticProfile> DagTransferSessionSetV1<P> {
    fn new() -> Self {
        Self {
            slots: Mutex::new(DagTransferSlotsV1 {
                entries: std::array::from_fn(|_| DagTransferSlotV1::Vacant),
                uses: 0,
            }),
            released: Condvar::new(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, DagTransferSlotsV1<P>> {
        match self.slots.lock() {
            Ok(slots) => slots,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn check_out(&self, root: DurableStateRootV1, position: u64) -> DagTransferClaimV1<'_, P> {
        let mut slots = self.lock();
        loop {
            let fittest = slots
                .entries
                .iter()
                .enumerate()
                .filter_map(|(index, slot)| {
                    slot.fit(root, position).map(|fit| (fit, Reverse(index)))
                })
                .max();
            match fittest {
                Some((_, Reverse(index))) => {
                    slots.uses += 1;
                    let held =
                        match std::mem::replace(&mut slots.entries[index], DagTransferSlotV1::Busy)
                        {
                            DagTransferSlotV1::Idle(held) => Some(held),
                            DagTransferSlotV1::Vacant | DagTransferSlotV1::Busy => None,
                        };
                    return DagTransferClaimV1 {
                        set: self,
                        index,
                        stamp: slots.uses,
                        held,
                    };
                }
                None => {
                    slots = match self.released.wait(slots) {
                        Ok(slots) => slots,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                }
            }
        }
    }
}

struct DagTransferClaimV1<'a, P: SemanticProfile> {
    set: &'a DagTransferSessionSetV1<P>,
    index: usize,
    stamp: u64,
    held: Option<Box<DagTransferSessionV1<P>>>,
}

impl<P: SemanticProfile> Drop for DagTransferClaimV1<'_, P> {
    fn drop(&mut self) {
        let released = match (std::thread::panicking(), self.held.take()) {
            (false, Some(held)) => DagTransferSlotV1::Idle(held),
            _ => DagTransferSlotV1::Vacant,
        };
        self.set.lock().entries[self.index] = released;
        self.set.released.notify_one();
    }
}

pub(crate) struct DurableRSpaceExporterV1<P: SemanticProfile>
where P::Channel: Clone + Hash + Eq
{
    published: DurablePublishedCoreV1<P>,
    limits: TransferPageLimitsV1,
    sessions: DagTransferSessionSetV1<P>,
}

impl<P: SemanticProfile> DurableRSpaceExporterV1<P>
where P::Channel: Clone + Hash + Eq
{
    fn new(published: DurablePublishedCoreV1<P>) -> Self {
        let limits =
            TransferPageLimitsV1::new(DAG_TRANSFER_PAGE_MAX_BYTES, DAG_TRANSFER_PAGE_MAX_OBJECTS)
                .expect("the DAG transfer page limits admit at least one object");
        Self::with_limits(published, limits)
    }

    pub(crate) fn with_limits(
        published: DurablePublishedCoreV1<P>,
        limits: TransferPageLimitsV1,
    ) -> Self {
        Self {
            published,
            limits,
            sessions: DagTransferSessionSetV1::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn live_sessions_for_test(&self) -> Vec<Option<(DurableStateRootV1, u64)>> {
        let slots = self.sessions.lock();
        let mut live = Vec::with_capacity(DAG_TRANSFER_SESSION_LIMIT);
        for slot in &slots.entries {
            match slot {
                DagTransferSlotV1::Vacant => {}
                DagTransferSlotV1::Idle(held) => live.push(Some((held.root, held.served_end))),
                DagTransferSlotV1::Busy => live.push(None),
            }
        }
        live
    }
}

impl<P: SemanticProfile> RSpaceExporter for DurableRSpaceExporterV1<P>
where P::Channel: Clone + Hash + Eq
{
    fn get_root(&self) -> Result<KeyHash, RootError> { Ok(self.published.current_root().into()) }

    fn transfer_format(&self) -> StateTransferFormat { StateTransferFormat::DagStreamV1 }

    fn get_transfer_page(
        &self,
        cursor: &[(KeyHash, Option<Byte>)],
    ) -> Result<StateTransferPage, StateExportError> {
        let (root_hash, position) = parse_dag_transfer_cursor(cursor)?;
        let root = DurableStateRootV1::try_from(root_hash)
            .map_err(|refusal| StateExportError::InvalidPath(refusal.to_string()))?;
        let mut claim = self.sessions.check_out(root, position);
        let stamp = claim.stamp;
        let held = match claim.held.take() {
            Some(mut held) if held.serves(root, position) => {
                held.last_used = stamp;
                claim.held.insert(held)
            }
            replaced => {
                let handle = match self.published.read_handle(Some(root)) {
                    Ok((_, handle)) => handle,
                    Err(refusal) => {
                        claim.held = replaced;
                        return Err(open_refusal(refusal, root_hash));
                    }
                };
                drop(replaced);
                let opened = claim.held.insert(Box::new(DagTransferSessionV1 {
                    root,
                    session: DurableExportSessionV1::from_read_handle(handle, self.limits),
                    served_end: 0,
                    complete: false,
                    cached_position: 0,
                    next_position: 0,
                    last_used: stamp,
                }));
                opened.advance_from_start(position, self.limits)?;
                opened
            }
        };
        let (bytes, next_position, terminal) = held.page_at(position, self.limits)?;
        Ok(StateTransferPage {
            bytes,
            next_cursor: dag_transfer_cursor(root_hash, next_position),
            terminal,
        })
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
