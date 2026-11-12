use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::convert::Infallible;
use std::fmt::Debug;
use std::hash::Hash;
use std::ops::Bound;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use pathmap::PathMap;
#[cfg(test)]
use proptest::prelude::*;
#[cfg(test)]
use rand::{Rng, thread_rng};
use serde::Serialize;
use tracing::warn;

use super::bag::{BagDelta, BagError};
use super::data_index::{CandidatePool, DataIndexError, RSpaceDataIndex, RSpaceDataStore};
use super::errors::RSpaceError;
use crate::rspace::hashing::stable_hash_provider::StableHashSerialize;
use crate::rspace::history::history_reader::HistoryReaderBase;
use crate::rspace::hot_store_action::{
    DeleteAction, DeleteCanonicalCollection, DeleteCanonicalData, DeleteContinuations, DeleteData,
    DeleteJoins, HotStoreAction, InsertAction, InsertCanonicalCollection, InsertCanonicalData,
    InsertContinuations, InsertData, InsertJoins,
};
use crate::rspace::internal::{AcceptedDatumChanges, Datum, Row, WaitingContinuation};
use crate::rspace::metrics_constants::{
    HOT_STORE_GET_CONT_CALLS_METRIC, HOT_STORE_GET_CONT_HISTORY_FILL_METRIC,
    HOT_STORE_GET_DATA_CALLS_METRIC, HOT_STORE_GET_DATA_HISTORY_FILL_METRIC,
    HOT_STORE_GET_JOINS_CALLS_METRIC, HOT_STORE_GET_JOINS_HISTORY_FILL_METRIC,
    HOT_STORE_HISTORY_CACHE_BULK_CLEAR_CONT_METRIC,
    HOT_STORE_HISTORY_CACHE_BULK_CLEAR_DATUMS_METRIC,
    HOT_STORE_HISTORY_CACHE_BULK_CLEAR_JOINS_METRIC, HOT_STORE_HISTORY_CONT_CACHE_ITEMS_METRIC,
    HOT_STORE_HISTORY_CONT_CACHE_SIZE_METRIC, HOT_STORE_HISTORY_DATA_CACHE_ITEMS_METRIC,
    HOT_STORE_HISTORY_DATA_CACHE_SIZE_METRIC, HOT_STORE_HISTORY_JOINS_CACHE_ITEMS_METRIC,
    HOT_STORE_HISTORY_JOINS_CACHE_SIZE_METRIC, HOT_STORE_PUT_CONT_CALLS_METRIC,
    HOT_STORE_PUT_CONT_DUPLICATES_METRIC, HOT_STORE_PUT_CONT_EXISTING_COUNT_METRIC,
    HOT_STORE_PUT_CONT_HISTORY_FILL_METRIC, HOT_STORE_PUT_CONT_IDENTITY_BUILD_NS_METRIC,
    HOT_STORE_PUT_CONT_IDENTITY_COMPARE_NS_METRIC, HOT_STORE_PUT_CONT_TIME_NS_METRIC,
    HOT_STORE_PUT_DATUM_CALLS_METRIC, HOT_STORE_PUT_DATUM_HISTORY_FILL_METRIC,
    HOT_STORE_PUT_DATUM_TIME_NS_METRIC, HOT_STORE_PUT_JOIN_CALLS_METRIC,
    HOT_STORE_PUT_JOIN_HISTORY_FILL_METRIC, HOT_STORE_PUT_JOIN_TIME_NS_METRIC,
    HOT_STORE_STATE_CONT_ITEMS_METRIC, HOT_STORE_STATE_CONT_SIZE_METRIC,
    HOT_STORE_STATE_DATA_ITEMS_METRIC, HOT_STORE_STATE_DATA_SIZE_METRIC,
    HOT_STORE_STATE_INSTALLED_CONT_ITEMS_METRIC, HOT_STORE_STATE_INSTALLED_CONT_SIZE_METRIC,
    HOT_STORE_STATE_INSTALLED_JOINS_ITEMS_METRIC, HOT_STORE_STATE_INSTALLED_JOINS_SIZE_METRIC,
    HOT_STORE_STATE_JOINS_ITEMS_METRIC, HOT_STORE_STATE_JOINS_SIZE_METRIC, RSPACE_METRICS_SOURCE,
};
use crate::rspace::ordered_receive::{
    OrderedReceiveError, OrderedReceiveView, PreparedContinuationRecord, RestingReceiveVisit,
};
use crate::rspace::persistence::{
    CanonicalFormatError, CatalogSlotV1, ContentTransition, DagTransactionError,
    GenerationLeaseRegistryV1, ImmutableObjectStore, MutationError, ObjectRefV1,
    OpenDagTransaction, RSpaceMutationBatchV1, SemanticNamespace, SemanticProfile, StateVersionV1,
};
#[cfg(test)]
use crate::rspace::persistence::{
    DurableAuthorityViewV1, DurableObjectReadErrorV1, RSpaceSemanticProfile,
};
use crate::rspace::receive_accelerator::{
    ReceiveAcceleratorClass, ReceiveAcceleratorHandle, ReceiveAnswer, ReceiveId,
    ReceiveOccurrenceIndex, ReceiveTarget, channel_frame, join_scope_class,
};
use crate::rspace::record_index::{RSpaceContinuationStore, RSpaceJoinStore, RecordIndexError};
use crate::rspace::rho_path::{RhoPathEncode, RhoPathError, RhoPathV1};
use crate::rspace::serializers::cold_store_decode::ColdStoreDecode;
use crate::rspace::trace::event::Consume;

const MAX_HISTORY_STORE_CACHE_ENTRIES: usize = 512;
const MAX_HISTORY_STORE_CACHE_CONT_ITEMS: usize = 8192;
const MAX_HISTORY_STORE_CACHE_DATA_ITEMS: usize = 8192;
const MAX_HISTORY_STORE_CACHE_JOIN_ITEMS: usize = 8192;
const HOT_STORE_STATE_METRICS_UPDATE_INTERVAL_MS: u64 = 250;
const HOT_STORE_HISTORY_CACHE_METRICS_UPDATE_INTERVAL_MS: u64 = 250;

/// Revision-local journals captured as one coherent hot-store observation.
///
/// The optional fields preserve the capability distinction: an implementation
/// that cannot expose a namespace journal returns `None`, never an invented
/// empty journal. Implementations with one shared state lock should override
/// `HotStore::pending_deltas` to read all three roots under that lock.
#[derive(Clone, Debug, Default)]
pub struct PendingDeltas {
    pub(crate) data: Option<BagDelta>,
    pub(crate) continuations: Option<BagDelta>,
    pub(crate) continuation_content: Option<PathMap<ContentTransition>>,
    pub(crate) continuation_source_bytes: Option<PathMap<Arc<[u8]>>>,
    pub(crate) joins: Option<BagDelta>,
}

/// One coherent read-boundary observation of the journal and the post-state
/// endpoint counts needed to invert its deltas. The count maps contain only
/// endpoints named by the journal, never a materialized copy of a namespace.
#[derive(Clone, Debug, Default)]
pub struct PendingMutationObservation {
    pub(crate) pending: PendingDeltas,
    pub(crate) data_counts: PathMap<u64>,
    pub(crate) join_counts: PathMap<u64>,
}

struct JournalIdentityV1;

#[derive(Clone)]
struct JournalVersionV1 {
    identity: Arc<JournalIdentityV1>,
    revision: u64,
}

impl JournalVersionV1 {
    fn new() -> Self {
        Self {
            identity: Arc::new(JournalIdentityV1),
            revision: 0,
        }
    }

    fn advance(&mut self) {
        if let Some(next) = self.revision.checked_add(1) {
            self.revision = next;
        } else {
            self.identity = Arc::new(JournalIdentityV1);
            self.revision = 0;
        }
    }

    fn next(&self) -> Self {
        let mut next = self.clone();
        next.advance();
        next
    }

    fn matches(&self, other: &Self) -> bool {
        self.revision == other.revision && Arc::ptr_eq(&self.identity, &other.identity)
    }
}

struct JournalCaptureIdentityV1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JournalCaptureKindV1 {
    SameBase,
    CrossBase,
}

pub struct CapturedHotJournalV1 {
    identity: Arc<JournalCaptureIdentityV1>,
    version: JournalVersionV1,
    base: CatalogSlotV1,
    origin: ObjectRefV1,
    kind: JournalCaptureKindV1,
    observation: PendingMutationObservation,
}

impl CapturedHotJournalV1 {
    pub(crate) fn observation(&self) -> &PendingMutationObservation { &self.observation }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotJournalAcknowledgementV1 {
    Committed,
    AlreadyCommitted,
}

struct HotJournalAckMarkerV1 {
    identity: Arc<JournalCaptureIdentityV1>,
    version: JournalVersionV1,
    terminal: CatalogSlotV1,
}

pub(crate) struct DurableJournalBaseV1 {
    pub manifest: ObjectRefV1,
    pub registry: GenerationLeaseRegistryV1,
}

enum JournalBaseV1 {
    Detached(ObjectRefV1),
    Bound(CatalogSlotV1),
    Invalidated,
}

struct DurableJournalStateV1 {
    base: JournalBaseV1,
    registry: GenerationLeaseRegistryV1,
    version: JournalVersionV1,
    acknowledged: Option<HotJournalAckMarkerV1>,
}

impl DurableJournalStateV1 {
    #[cfg(test)]
    fn new(base: CatalogSlotV1, registry: GenerationLeaseRegistryV1) -> Self {
        Self {
            base: JournalBaseV1::Bound(base),
            registry,
            version: JournalVersionV1::new(),
            acknowledged: None,
        }
    }

    fn detached(base: DurableJournalBaseV1) -> Self {
        Self {
            base: JournalBaseV1::Detached(base.manifest),
            registry: base.registry,
            version: JournalVersionV1::new(),
            acknowledged: None,
        }
    }

    fn is_bound_to(&self, slot: CatalogSlotV1) -> bool {
        matches!(self.base, JournalBaseV1::Bound(bound) if bound == slot)
    }

    fn recorded_manifest(&self) -> Option<ObjectRefV1> {
        match self.base {
            JournalBaseV1::Detached(manifest) => Some(manifest),
            JournalBaseV1::Bound(slot) => Some(slot.state().manifest()),
            JournalBaseV1::Invalidated => None,
        }
    }

    fn advance(&mut self) {
        self.version.advance();
        self.acknowledged = None;
    }
}

/// Build one exact three-namespace mutation batch from a coherent hot-store
/// journal snapshot. The caller supplies authoritative base Bags; no typed
/// record projection or continuation re-encoding occurs here.
#[cfg(test)]
pub(crate) fn mutation_batch_from_pending(
    base: StateVersionV1,
    data_base: &super::bag::Bag,
    join_base: &super::bag::Bag,
    pending: &PendingDeltas,
) -> Result<RSpaceMutationBatchV1, MutationError> {
    let data = pending.data.as_ref().ok_or(MutationError::EmptyBatch)?;
    let continuations = pending
        .continuation_content
        .as_ref()
        .ok_or(MutationError::EmptyBatch)?;
    let joins = pending.joins.as_ref().ok_or(MutationError::EmptyBatch)?;
    let mut batch = RSpaceMutationBatchV1::new(base)?;
    batch.record_data_delta(data_base, data)?;
    batch.record_continuation_transitions(continuations)?;
    batch.record_join_delta(join_base, joins)?;
    if batch.is_empty() {
        return Err(MutationError::EmptyBatch);
    }
    Ok(batch)
}

/// Build a mutation batch from exact resident endpoint-count lookups. This is
/// the production adapter for the PathMap-backed hot store: it reads only the
/// endpoints named by the journal and never reconstructs a global Bag.
pub(crate) fn mutation_batch_from_pending_with_counts(
    base: StateVersionV1,
    pending: &PendingDeltas,
    mut data_count: impl FnMut(&[u8]) -> Option<u64>,
    mut join_count: impl FnMut(&[u8]) -> Option<u64>,
) -> Result<RSpaceMutationBatchV1, MutationError> {
    let data = pending.data.as_ref().ok_or(MutationError::EmptyBatch)?;
    let continuations = pending
        .continuation_content
        .as_ref()
        .ok_or(MutationError::EmptyBatch)?;
    let joins = pending.joins.as_ref().ok_or(MutationError::EmptyBatch)?;
    let mut batch = RSpaceMutationBatchV1::new(base)?;
    for (path, amount) in data.additions().iter() {
        let after = data_count(&path).ok_or(MutationError::MissingCountObservation {
            namespace: SemanticNamespace::DataBag,
        })?;
        let before = after
            .checked_sub(amount.get())
            .ok_or(MutationError::DeltaCountUnderflow)?;
        batch.record_data(&path, before, after)?;
    }
    for (path, amount) in data.removals().iter() {
        let after = data_count(&path).ok_or(MutationError::MissingCountObservation {
            namespace: SemanticNamespace::DataBag,
        })?;
        let before = after
            .checked_add(amount.get())
            .ok_or(MutationError::DeltaCountOverflow)?;
        batch.record_data(&path, before, after)?;
    }
    batch.record_continuation_transitions(continuations)?;
    for (path, amount) in joins.additions().iter() {
        let after = join_count(&path).ok_or(MutationError::MissingCountObservation {
            namespace: SemanticNamespace::JoinBag,
        })?;
        let before = after
            .checked_sub(amount.get())
            .ok_or(MutationError::DeltaCountUnderflow)?;
        batch.record_join(&path, before, after)?;
    }
    for (path, amount) in joins.removals().iter() {
        let after = join_count(&path).ok_or(MutationError::MissingCountObservation {
            namespace: SemanticNamespace::JoinBag,
        })?;
        let before = after
            .checked_add(amount.get())
            .ok_or(MutationError::DeltaCountOverflow)?;
        batch.record_join(&path, before, after)?;
    }
    if batch.is_empty() {
        return Err(MutationError::EmptyBatch);
    }
    Ok(batch)
}

pub(crate) fn mutation_batch_from_observation(
    base: StateVersionV1,
    observation: &PendingMutationObservation,
) -> Result<RSpaceMutationBatchV1, MutationError> {
    mutation_batch_from_pending_with_counts(
        base,
        &observation.pending,
        |path| observation.data_counts.get(path).copied(),
        |path| observation.join_counts.get(path).copied(),
    )
}

/// Stage the exact continuation sources retained by one coherent journal
/// snapshot. Missing journal capabilities are rejected rather than treated as
/// empty, and source bytes flow directly to transaction admission.
pub(crate) fn stage_pending_continuation_sources<'store, 'mutation, S, P>(
    transaction: &mut OpenDagTransaction<'store, 'mutation, S, P>,
    pending: &PendingDeltas,
) -> Result<(), DagTransactionError<S::Error>>
where
    S: ImmutableObjectStore,
    P: SemanticProfile,
{
    let transitions = pending.continuation_content.as_ref().ok_or_else(|| {
        CanonicalFormatError::SemanticCoherence {
            detail: "continuation transition journal capability is unavailable".to_owned(),
        }
    })?;
    let source_bytes = pending.continuation_source_bytes.as_ref().ok_or_else(|| {
        CanonicalFormatError::SemanticCoherence {
            detail: "continuation source-byte journal capability is unavailable".to_owned(),
        }
    })?;
    transaction.stage_continuation_source_journal(transitions, source_bytes)
}

// See rspace/src/main/scala/coop/rchain/rspace/HotStore.scala
pub trait HotStore<C: Clone + Hash + Eq, P: Clone, A: Clone, K: Clone>: Sync + Send {
    fn get_continuations(&self, channels: &[C]) -> Vec<WaitingContinuation<P, K>>;
    fn get_continuations_versioned(
        &self,
        channels: &[C],
    ) -> (Option<ContinuationVersion>, Vec<WaitingContinuation<P, K>>) {
        (None, self.get_continuations(channels))
    }
    fn put_continuation(&self, channels: &[C], wc: WaitingContinuation<P, K>) -> Option<bool>;
    fn install_continuation(&self, channels: &[C], wc: WaitingContinuation<P, K>) -> Option<()>;
    fn remove_continuation(&self, channels: &[C], index: i32) -> Option<()>;
    fn try_visit_resting_receive_views(
        &self,
        channels: &[C],
        visitor: &mut dyn FnMut(&[u64], Result<OrderedReceiveView<'_>, OrderedReceiveError>),
    ) -> RestingReceiveVisit;
    fn receive_revision(&self) -> Option<ReceiveRevision> { None }
    fn rebuild_receive_accelerator(&self) -> ReceiveAcceleratorClass {
        ReceiveAcceleratorClass::Absent
    }
    fn receive_accelerator(&self) -> Option<ReceiveAcceleratorHandle<C, P, K>>
    where
        C: Send + Sync,
        P: Send + Sync,
        K: Send + Sync,
    {
        None
    }
    fn detached_receive_accelerator(&self) -> Option<ReceiveAcceleratorHandle<C, P, K>>
    where
        C: Send + Sync,
        P: Send + Sync,
        K: Send + Sync,
    {
        None
    }
    fn receive_occurrences(
        &self,
        _held: Option<&ReceiveAcceleratorHandle<C, P, K>>,
        _channel: &C,
    ) -> ReceiveAnswer<C, P, K>
    where
        C: Send + Sync,
        P: Send + Sync,
        K: Send + Sync,
    {
        ReceiveAnswer::Fallback(ReceiveAcceleratorClass::Absent)
    }
    fn current_receive_occurrences(&self, _channel: &C) -> ReceiveAnswer<C, P, K> {
        ReceiveAnswer::Fallback(ReceiveAcceleratorClass::Absent)
    }
    fn resolve_receive(&self, _id: &ReceiveId<C, P, K>) -> Option<ReceiveTarget<C, P, K>> { None }
    fn hydrate_receive_scope(&self, channel: &C) {
        for group in self.get_joins(channel) {
            self.get_continuations_versioned(&group);
        }
    }

    fn get_data(&self, channel: &C) -> Vec<Datum<A>>;
    fn get_candidate_pool(&self, channel: &C) -> CandidatePool<A>;
    fn put_datum(&self, channel: &C, d: Datum<A>) -> ();
    fn remove_datum(&self, channel: &C, index: i32) -> Result<(), RSpaceError>;
    fn remove_datum_path(&self, path: &[u8]) -> Result<(), RSpaceError>;
    fn apply_datum_delta(&self, delta: &BagDelta) -> Result<(), RSpaceError>;
    /// Return the normalized datum-path changes retained since the current
    /// hot-store revision was created. `None` means this implementation does
    /// not expose a journal; it must never be interpreted as an empty journal.
    fn pending_data_delta(&self) -> Result<Option<BagDelta>, RSpaceError>;
    /// Return the exact resident DATA multiplicity for one canonical path.
    /// The lookup is against the authoritative PathMap and does not decode or
    /// materialize a channel projection.
    fn data_count_at_path(&self, path: &[u8]) -> Result<u64, RSpaceError>;
    /// Return normalized resting-join path changes, or `None` when the
    /// implementation has no join-journal capability.
    fn pending_join_delta(&self) -> Result<Option<BagDelta>, RSpaceError>;
    /// Return the exact resident JOIN multiplicity for one canonical path.
    fn join_count_at_path(&self, path: &[u8]) -> Result<u64, RSpaceError>;
    /// Return normalized continuation-record path changes, or `None` when the
    /// implementation has no continuation journal capability.
    fn pending_continuation_delta(&self) -> Result<Option<BagDelta>, RSpaceError>;
    fn pending_continuation_content(
        &self,
    ) -> Result<Option<PathMap<ContentTransition>>, RSpaceError>;
    fn pending_continuation_source_bytes(&self) -> Result<Option<PathMap<Arc<[u8]>>>, RSpaceError>;
    /// Capture all supported revision-local journals as one store snapshot.
    fn pending_deltas(&self) -> Result<PendingDeltas, RSpaceError> {
        Ok(PendingDeltas {
            data: self.pending_data_delta()?,
            continuations: self.pending_continuation_delta()?,
            continuation_content: self.pending_continuation_content()?,
            continuation_source_bytes: self.pending_continuation_source_bytes()?,
            joins: self.pending_join_delta()?,
        })
    }

    /// Capture journals and all referenced post-state counts under one
    /// implementation-defined state boundary. This is the required input to
    /// durable publication; separate calls to `pending_deltas` and count
    /// lookups are not a coherent checkpoint observation.
    fn pending_mutation_observation(&self) -> Result<PendingMutationObservation, RSpaceError>;

    fn capture_pending_journal(
        &self,
        base: CatalogSlotV1,
    ) -> Result<CapturedHotJournalV1, RSpaceError>;

    fn capture_cross_base_journal(
        &self,
        origin: ObjectRefV1,
        publication_base: CatalogSlotV1,
        registry: &GenerationLeaseRegistryV1,
    ) -> Result<CapturedHotJournalV1, RSpaceError>;

    fn validate_pending_journal(&self, captured: &CapturedHotJournalV1) -> Result<(), RSpaceError>;

    fn validate_journal_authority(
        &self,
        base: CatalogSlotV1,
        registry: &GenerationLeaseRegistryV1,
    ) -> Result<(), RSpaceError>;

    fn bind_journal_authority(&self, slot: CatalogSlotV1) -> Result<(), RSpaceError>;

    fn validate_journal_acknowledgement(
        &self,
        captured: &CapturedHotJournalV1,
        terminal: CatalogSlotV1,
        registry: &GenerationLeaseRegistryV1,
    ) -> Result<(), RSpaceError>;

    fn acknowledge_pending_journal(
        &self,
        captured: &CapturedHotJournalV1,
        expected: CatalogSlotV1,
        admitted: CatalogSlotV1,
    ) -> Result<HotJournalAcknowledgementV1, RSpaceError>;

    /// Commit one exact revision-local journal snapshot after its durable
    /// publication succeeds. Implementations must compare all namespaces
    /// while holding their state boundary and must not clear a newer journal.
    fn commit_pending_deltas(&self, expected: &PendingDeltas) -> Result<(), RSpaceError>;
    /// Restore revision-local journals captured by a soft checkpoint.
    fn restore_pending_deltas(
        &self,
        data: BagDelta,
        continuations: BagDelta,
        continuation_content: PathMap<ContentTransition>,
        continuation_source_bytes: PathMap<Arc<[u8]>>,
        joins: BagDelta,
    ) -> Result<(), RSpaceError>;
    fn rebase_pending_deltas(
        &self,
        data: &BagDelta,
        continuations: &BagDelta,
        continuation_content: &PathMap<ContentTransition>,
        continuation_source_bytes: &PathMap<Arc<[u8]>>,
        joins: &BagDelta,
    ) -> Result<(), RSpaceError>;
    fn apply_accepted_match(
        &self,
        transition: AcceptedMatch<C>,
    ) -> Result<AcceptedMatchOutcome<P, K>, RSpaceError>;

    fn get_joins(&self, channel: &C) -> Vec<Vec<C>>;
    fn put_join(&self, channel: &C, join: &[C]) -> Option<()>;
    fn install_join(&self, channel: &C, join: &[C]) -> Option<()>;
    fn remove_join(&self, channel: &C, join: &[C]) -> Option<()>;

    fn changes(&self) -> Vec<HotStoreAction<C, P, A, K>>;
    /// Checkpoint-specific actions. Implementations may override this boundary
    /// to preserve already-canonical resident bytes while `changes()` remains
    /// the typed diagnostic and compatibility surface.
    fn checkpoint_changes(&self) -> Result<Vec<HotStoreAction<C, P, A, K>>, RSpaceError> {
        Ok(self.changes())
    }
    fn to_map(&self) -> HashMap<Vec<C>, Row<P, A, K>>;
    fn snapshot(&self) -> HotStoreState<C, P, A, K>;

    fn print(&self) -> ();
    fn clear(&self) -> ();

    // See rspace/src/test/scala/coop/rchain/rspace/test/package.scala
    fn is_empty(&self) -> bool;

    fn set_state(&self, state: HotStoreState<C, P, A, K>);
}

/// Opaque, limit-free freshness capability for one continuation group.
///
/// Equality is allocation identity, not payload equality. Replacing the
/// group's epoch invalidates every held address while those addresses keep the
/// old allocation alive, so allocator-address reuse cannot make a stale
/// candidate current again.
#[derive(Debug)]
struct ContinuationEpoch;

#[derive(Clone, Debug)]
pub struct ContinuationVersion {
    epoch: Arc<ContinuationEpoch>,
    group_prefix: Arc<[u8]>,
    ordinary_paths: Arc<OnceLock<Arc<[Arc<[u8]>]>>>,
}

impl PartialEq for ContinuationVersion {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.epoch, &other.epoch) && self.group_prefix == other.group_prefix
    }
}

impl Eq for ContinuationVersion {}

impl ContinuationVersion {
    fn new(epoch: Arc<ContinuationEpoch>, group_prefix: Arc<[u8]>) -> Self {
        Self {
            epoch,
            group_prefix,
            ordinary_paths: Arc::new(OnceLock::new()),
        }
    }

    fn bind_paths(&self, paths: Vec<Arc<[u8]>>) { let _ = self.ordinary_paths.set(paths.into()); }

    fn path(&self, index: usize) -> Option<Arc<[u8]>> {
        self.ordinary_paths
            .get()
            .and_then(|paths| paths.get(index))
            .cloned()
    }
}

#[derive(Clone, Debug)]
pub struct ReceiveRevision {
    generation: Arc<ContinuationEpoch>,
    store: Arc<ContinuationEpoch>,
}

impl PartialEq for ReceiveRevision {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.generation, &other.generation) && Arc::ptr_eq(&self.store, &other.store)
    }
}

impl Eq for ReceiveRevision {}

impl ReceiveRevision {
    pub fn shares_generation(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.generation, &other.generation)
    }
}

#[derive(Clone, Debug)]
pub struct ContinuationAddress {
    pub(crate) version: Option<ContinuationVersion>,
    pub(crate) index: i32,
}

impl ContinuationAddress {
    pub(crate) fn new(version: Option<ContinuationVersion>, index: i32) -> Self {
        Self { version, index }
    }

    pub fn index(&self) -> i32 { self.index }
}

#[derive(Clone, Debug)]
pub struct StoredContinuationCommit {
    pub(crate) address: ContinuationAddress,
    pub(crate) source: Consume,
    pub(crate) persist: bool,
    pub(crate) peeks: BTreeSet<i32>,
}

#[derive(Clone, Debug)]
pub struct AcceptedMatch<C> {
    pub(crate) channels: Vec<C>,
    pub(crate) continuation: Option<StoredContinuationCommit>,
    pub(crate) datum_delta: AcceptedDatumChanges,
    pub(crate) required_persistent_paths: Vec<Arc<[u8]>>,
    pub(crate) join_channels: Vec<C>,
}

#[derive(Clone, Debug)]
pub enum AcceptedMatchOutcome<P: Clone, K: Clone> {
    Committed {
        /// The authoritative stored continuation, when this transaction named
        /// one. Callers use it instead of trusting public-facing candidate
        /// payloads after the commit boundary.
        continuation: Option<WaitingContinuation<P, K>>,
    },
    Stale,
}

fn with_installed<P: Clone, K: Clone>(
    installed: Option<WaitingContinuation<P, K>>,
    continuations: Vec<WaitingContinuation<P, K>>,
) -> Vec<WaitingContinuation<P, K>> {
    let Some(installed) = installed else {
        return continuations;
    };
    let mut result = Vec::with_capacity(continuations.len() + 1);
    result.push(installed);
    result.extend(continuations);
    result
}

pub fn new_hashmap<K: std::cmp::Eq + std::hash::Hash, V>() -> HashMap<K, V> { HashMap::new() }

#[derive(Default, Debug, Clone)]
pub struct HotStoreState<C, P, A, K>
where
    C: Eq + Hash,
    A: Clone,
    P: Clone,
    K: Clone,
{
    pub continuations: HashMap<Vec<C>, Vec<WaitingContinuation<P, K>>>,
    pub installed_continuations: HashMap<Vec<C>, WaitingContinuation<P, K>>,
    pub data: HashMap<C, Vec<Datum<A>>>,
    pub joins: HashMap<C, Vec<Vec<C>>>,
    pub installed_joins: HashMap<C, Vec<Vec<C>>>,
}

#[derive(Clone, PartialEq)]
struct ReceiveBuildBase {
    revision: ReceiveRevision,
    loads: u64,
}

struct ReceiveBuild<C, P, K>
where
    C: Clone + Send + Sync,
    P: Clone + Send + Sync,
    K: Clone + Send + Sync,
{
    base: ReceiveBuildBase,
    snapshot: RSpaceContinuationStore<C, P, K>,
}

struct InMemHotStoreState<C, P, A, K>
where
    C: Clone + Eq + Hash + Send + Sync,
    A: Clone + Send + Sync,
    P: Clone + Send + Sync,
    K: Clone + Send + Sync,
{
    continuations: RSpaceContinuationStore<C, P, K>,
    data: RSpaceDataStore<C, A>,
    joins: RSpaceJoinStore<C>,
    continuation_base_epoch: Arc<ContinuationEpoch>,
    continuation_store_epoch: Arc<ContinuationEpoch>,
    continuation_epochs: PathMap<Arc<ContinuationEpoch>>,
    durable_journal: Option<Box<DurableJournalStateV1>>,
    receive_build: Option<ReceiveBuildBase>,
}

impl<C, P, A, K> InMemHotStoreState<C, P, A, K>
where
    C: Clone + Eq + Hash + RhoPathEncode + ColdStoreDecode + Send + Sync,
    P: Clone + StableHashSerialize + Send + Sync,
    A: Clone + Serialize + RhoPathEncode + ColdStoreDecode + Send + Sync,
    K: Clone + StableHashSerialize + Send + Sync,
{
    fn from_snapshot(snapshot: HotStoreState<C, P, A, K>) -> Result<Self, DataIndexError> {
        let mut continuations = RSpaceContinuationStore::new();
        for (channels, values) in snapshot.continuations {
            continuations.set_group(channels, values);
        }
        for (channels, value) in snapshot.installed_continuations {
            continuations.install(&channels, value);
        }
        let mut data = RSpaceDataStore::new();
        for (channel, datums) in snapshot.data {
            data.set_channel(channel, datums)?;
        }
        let mut joins = RSpaceJoinStore::new();
        for (channel, values) in snapshot.joins {
            joins.set_channel(channel, values);
        }
        for (channel, values) in snapshot.installed_joins {
            if !values.is_empty() {
                joins.set_installed_channel(channel, values);
            }
        }
        let continuation_base_epoch = Arc::new(ContinuationEpoch);
        Ok(Self {
            continuations,
            data,
            joins,
            continuation_store_epoch: Arc::clone(&continuation_base_epoch),
            continuation_base_epoch,
            continuation_epochs: PathMap::new(),
            durable_journal: None,
            receive_build: None,
        })
    }

    fn receive_handle(&self) -> Option<ReceiveAcceleratorHandle<C, P, K>> {
        ReceiveAcceleratorHandle::new(self.receive_revision(), self.continuations.clone())
    }

    fn read_receive_occurrences(
        &self,
        handle: &ReceiveAcceleratorHandle<C, P, K>,
        channel: &C,
    ) -> ReceiveAnswer<C, P, K> {
        let class = handle
            .token_class(&self.receive_revision())
            .or_else(|| join_scope_class(handle.scope(), &self.joins, channel));
        match class {
            Some(class) => ReceiveAnswer::Fallback(class),
            None => handle.materialize(channel),
        }
    }

    fn vacant_receive_class(&self) -> ReceiveAcceleratorClass {
        match self.receive_build {
            Some(_) => ReceiveAcceleratorClass::Building,
            None => ReceiveAcceleratorClass::Absent,
        }
    }

    fn begin_receive_build(&mut self) -> Option<ReceiveBuild<C, P, K>> {
        match (self.continuations.receive_index(), &self.receive_build) {
            (None, None) => {
                let base = ReceiveBuildBase {
                    revision: self.receive_revision(),
                    loads: self.continuations.loads(),
                };
                self.receive_build = Some(base.clone());
                Some(ReceiveBuild {
                    base,
                    snapshot: self.continuations.clone(),
                })
            }
            (Some(_), _) | (None, Some(_)) => None,
        }
    }

    fn finish_receive_build(
        &mut self,
        base: &ReceiveBuildBase,
        built: Result<ReceiveOccurrenceIndex, RhoPathError>,
    ) -> bool {
        let open = self
            .receive_build
            .take_if(|marker| *marker == *base)
            .is_some();
        let same_revision = base.revision == self.receive_revision();
        let same_loads = base.loads == self.continuations.loads();
        let vacant = self.continuations.receive_index().is_none();
        match (open && same_revision && same_loads && vacant, built) {
            (true, Ok(index)) => {
                self.continuations.publish_receive_index(index);
                true
            }
            (true, Err(_)) | (false, _) => false,
        }
    }

    fn rebuild_receive_index_locked(&mut self) -> ReceiveAcceleratorClass {
        self.receive_build = None;
        match self.continuations.rebuild_receive_index() {
            true => ReceiveAcceleratorClass::Coherent,
            false => ReceiveAcceleratorClass::Absent,
        }
    }

    fn unloaded_installed_groups(&self, channel: &C) -> Vec<Arc<[C]>> {
        let mut groups = Vec::new();
        let Some(index) = self.continuations.receive_index() else {
            return groups;
        };
        let Ok(()) = index.try_visit_groups_of::<Infallible>(&channel_frame(channel), |prefix| {
            if let (false, Some(entry)) = (
                self.continuations.is_loaded_prefix(prefix),
                self.continuations.installed_entry(prefix),
            ) {
                groups.push(Arc::clone(&entry.0));
            }
            Ok(())
        });
        groups
    }

    fn pending_journal(&self) -> PendingDeltas {
        PendingDeltas {
            data: Some(self.data.pending_delta().clone()),
            continuations: Some(self.continuations.pending_delta().clone()),
            continuation_content: Some(self.continuations.pending_content().clone()),
            continuation_source_bytes: Some(self.continuations.pending_source_bytes().clone()),
            joins: Some(self.joins.pending_delta().clone()),
        }
    }

    fn pending_observation(&self) -> PendingMutationObservation {
        let pending = self.pending_journal();
        let mut data_counts = PathMap::new();
        for (path, _) in self
            .data
            .pending_delta()
            .additions()
            .iter()
            .chain(self.data.pending_delta().removals().iter())
        {
            data_counts.insert(&path, self.data.count_path(&path));
        }
        let mut join_counts = PathMap::new();
        for (path, _) in self
            .joins
            .pending_delta()
            .additions()
            .iter()
            .chain(self.joins.pending_delta().removals().iter())
        {
            join_counts.insert(&path, self.joins.count_path(&path));
        }
        PendingMutationObservation {
            pending,
            data_counts,
            join_counts,
        }
    }

    fn matches_pending_journal(&self, expected: &PendingDeltas) -> bool {
        expected
            .data
            .as_ref()
            .is_some_and(|data| self.data.pending_delta() == data) &&
            expected
                .continuations
                .as_ref()
                .is_some_and(|delta| self.continuations.pending_delta() == delta) &&
            expected
                .continuation_content
                .as_ref()
                .is_some_and(|content| {
                    self.continuations
                        .pending_content()
                        .iter()
                        .eq(content.iter())
                }) &&
            expected
                .continuation_source_bytes
                .as_ref()
                .is_some_and(|sources| {
                    self.continuations
                        .pending_source_bytes()
                        .iter()
                        .eq(sources.iter())
                }) &&
            expected
                .joins
                .as_ref()
                .is_some_and(|joins| self.joins.pending_delta() == joins)
    }

    fn matches_captured_journal(&self, captured: &CapturedHotJournalV1) -> bool {
        self.durable_journal.as_ref().is_some_and(|binding| {
            let anchored = match captured.kind {
                JournalCaptureKindV1::SameBase => binding.is_bound_to(captured.base),
                JournalCaptureKindV1::CrossBase => true,
            };
            anchored &&
                binding.recorded_manifest() == Some(captured.origin) &&
                binding.version.matches(&captured.version)
        }) && self.matches_pending_journal(&captured.observation.pending)
    }

    fn matches_acknowledged_journal(
        &self,
        captured: &CapturedHotJournalV1,
        terminal: CatalogSlotV1,
    ) -> bool {
        self.durable_journal.as_ref().is_some_and(|binding| {
            binding.is_bound_to(terminal) &&
                binding.acknowledged.as_ref().is_some_and(|marker| {
                    Arc::ptr_eq(&marker.identity, &captured.identity) &&
                        marker.terminal == terminal &&
                        marker.version.matches(&binding.version)
                })
        }) && self.pending_journal_is_empty()
    }

    fn pending_journal_is_empty(&self) -> bool {
        self.data.pending_delta().is_empty() &&
            self.continuations.pending_delta().is_empty() &&
            self.continuations.pending_content().is_empty() &&
            self.continuations.pending_source_bytes().is_empty() &&
            self.joins.pending_delta().is_empty()
    }

    fn clear_pending_journal(&mut self) {
        self.data.restore_pending_delta(BagDelta::new());
        self.continuations.restore_pending_delta(BagDelta::new());
        self.continuations.restore_pending_content(PathMap::new());
        self.continuations
            .restore_pending_source_bytes(PathMap::new());
        self.joins.restore_pending_delta(BagDelta::new());
    }

    fn snapshot(&self) -> Result<HotStoreState<C, P, A, K>, DataIndexError> {
        Ok(HotStoreState {
            continuations: self
                .continuations
                .loaded_records()
                .into_iter()
                .map(|(channels, values)| (channels.to_vec(), values))
                .collect(),
            installed_continuations: self
                .continuations
                .installed_records()
                .into_iter()
                .map(|(channels, value)| (channels.to_vec(), value))
                .collect(),
            data: self.data.loaded_records()?.into_iter().collect(),
            joins: self.joins.loaded_records().into_iter().collect(),
            installed_joins: self.joins.installed_records().into_iter().collect(),
        })
    }
}

impl<C, P, A, K> Default for InMemHotStoreState<C, P, A, K>
where
    C: Clone + Eq + Hash + Send + Sync,
    A: Clone + Send + Sync,
    P: Clone + Send + Sync,
    K: Clone + Send + Sync,
{
    fn default() -> Self {
        let continuation_base_epoch = Arc::new(ContinuationEpoch);
        Self {
            continuations: RSpaceContinuationStore::default(),
            data: RSpaceDataStore::new(),
            joins: RSpaceJoinStore::default(),
            continuation_store_epoch: Arc::clone(&continuation_base_epoch),
            continuation_base_epoch,
            continuation_epochs: PathMap::new(),
            durable_journal: None,
            receive_build: None,
        }
    }
}

impl<C, P, A, K> InMemHotStoreState<C, P, A, K>
where
    C: Clone + Eq + Hash + Send + Sync,
    A: Clone + Send + Sync,
    P: Clone + Send + Sync,
    K: Clone + Send + Sync,
{
    fn continuation_version(&self, channels: &[C]) -> Option<ContinuationVersion>
    where C: RhoPathEncode {
        let group_prefix = RhoPathV1::continuation_prefix(channels);
        let epoch = self
            .continuation_epochs
            .get(&group_prefix)
            .unwrap_or(&self.continuation_base_epoch);
        Some(ContinuationVersion::new(Arc::clone(epoch), group_prefix.into()))
    }

    fn bump_continuation_revision(&mut self, channels: &[C])
    where C: RhoPathEncode {
        self.advance_journal_version();
        let epoch = Arc::new(ContinuationEpoch);
        self.continuation_store_epoch = Arc::clone(&epoch);
        self.continuation_epochs
            .insert(RhoPathV1::continuation_prefix(channels), epoch);
    }

    fn bump_continuation_generation(&mut self) {
        self.continuation_base_epoch = Arc::new(ContinuationEpoch);
        self.continuation_store_epoch = Arc::clone(&self.continuation_base_epoch);
        self.continuation_epochs = PathMap::new();
    }

    fn receive_revision(&self) -> ReceiveRevision {
        ReceiveRevision {
            generation: Arc::clone(&self.continuation_base_epoch),
            store: Arc::clone(&self.continuation_store_epoch),
        }
    }

    fn advance_journal_version(&mut self) {
        if let Some(journal) = &mut self.durable_journal {
            journal.advance();
        }
    }

    fn invalidate_journal_base(&mut self) {
        if let Some(journal) = &mut self.durable_journal {
            journal.advance();
            journal.base = JournalBaseV1::Invalidated;
        }
    }
}

// This impl is needed for hot_store_spec.rs
#[cfg(test)]
impl<C, P, A, K> HotStoreState<C, P, A, K>
where
    C: Eq + Hash + Debug + Arbitrary + Default + Clone,
    A: Clone + Debug + Arbitrary + Default,
    P: Clone + Debug + Arbitrary + Default,
    K: Clone + Debug + Arbitrary + Default,
{
    fn random_vec<T>(size: usize) -> Vec<T>
    where T: Default + Clone {
        let mut rng = thread_rng();
        (0..size)
            .map(|_| T::default())
            .collect::<Vec<T>>()
            .iter()
            .take(rng.gen_range(0..size + 1))
            .cloned()
            .collect()
    }

    pub fn random_state() -> Self {
        let channels: Vec<C> = HotStoreState::<C, P, A, K>::random_vec(10);
        let continuations: Vec<WaitingContinuation<P, K>> =
            HotStoreState::<C, P, A, K>::random_vec(10);
        let installed_continuations = WaitingContinuation::default();
        let data: Vec<Datum<A>> = HotStoreState::<C, P, A, K>::random_vec(10);
        let channel = C::default();
        let joins: Vec<Vec<C>> = HotStoreState::<C, P, A, K>::random_vec(10);
        let installed_joins: Vec<Vec<C>> = HotStoreState::<C, P, A, K>::random_vec(10);

        HotStoreState {
            continuations: HashMap::from_iter(vec![(channels.clone(), continuations.clone())]),
            installed_continuations: HashMap::from_iter(vec![(
                channels.clone(),
                installed_continuations.clone(),
            )]),
            data: HashMap::from_iter(vec![(channel.clone(), data.clone())]),
            joins: HashMap::from_iter(vec![(channel.clone(), joins)]),
            installed_joins: HashMap::from_iter(vec![(channel, installed_joins)]),
        }
    }
}

#[derive(Default)]
struct HistoryStoreCache<C, P, A, K>
where
    C: Eq + Hash,
    A: Clone,
    P: Clone,
    K: Clone,
{
    continuations: HashMap<Vec<C>, Vec<WaitingContinuation<P, K>>>,
    datums: HashMap<C, Vec<Datum<A>>>,
    joins: HashMap<C, Vec<Vec<C>>>,
}

struct InMemHotStore<C, P, A, K>
where
    C: Clone + Eq + Hash + Sync + Send,
    A: Clone + Sync + Send,
    P: Clone + Sync + Send,
    K: Clone + Sync + Send,
{
    state: std::sync::RwLock<InMemHotStoreState<C, P, A, K>>,
    history_cache: std::sync::RwLock<HistoryStoreCache<C, P, A, K>>,
    history_reader_base: Box<dyn HistoryReaderBase<C, P, A, K>>,
}

#[cfg(not(feature = "accepted-match-transition-instrumentation"))]
type HotStateWriteGuard<'a, T> = std::sync::RwLockWriteGuard<'a, T>;

#[cfg(feature = "accepted-match-transition-instrumentation")]
struct HotStateWriteGuard<'a, T> {
    guard: std::sync::RwLockWriteGuard<'a, T>,
    hold_started: std::time::Instant,
    measured: bool,
}

#[cfg(feature = "accepted-match-transition-instrumentation")]
impl<T> std::ops::Deref for HotStateWriteGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target { &self.guard }
}

#[cfg(feature = "accepted-match-transition-instrumentation")]
impl<T> std::ops::DerefMut for HotStateWriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.guard }
}

#[cfg(feature = "accepted-match-transition-instrumentation")]
impl<T> Drop for HotStateWriteGuard<'_, T> {
    fn drop(&mut self) {
        if self.measured {
            accepted_match_transition_experiment::record_write_hold(self.hold_started.elapsed());
        }
    }
}

#[cfg(not(feature = "accepted-match-transition-instrumentation"))]
#[inline(always)]
fn hot_state_write<T>(lock: &std::sync::RwLock<T>) -> HotStateWriteGuard<'_, T> {
    lock.write().expect("hot store state write lock")
}

#[cfg(feature = "accepted-match-transition-instrumentation")]
#[inline(always)]
fn hot_state_write<T>(lock: &std::sync::RwLock<T>) -> HotStateWriteGuard<'_, T> {
    let wait_started = std::time::Instant::now();
    let guard = lock.write().expect("hot store state write lock");
    let measured =
        accepted_match_transition_experiment::record_write_acquisition(wait_started.elapsed());
    HotStateWriteGuard {
        guard,
        hold_started: std::time::Instant::now(),
        measured,
    }
}

// See rspace/src/main/scala/coop/rchain/rspace/HotStore.scala
impl<C, P, A, K> HotStore<C, P, A, K> for InMemHotStore<C, P, A, K>
where
    C: Clone
        + Debug
        + Hash
        + Eq
        + RhoPathEncode
        + StableHashSerialize
        + ColdStoreDecode
        + Send
        + Sync,
    P: Clone + Debug + StableHashSerialize + Send + Sync,
    A: Clone + Debug + Serialize + RhoPathEncode + ColdStoreDecode + Send + Sync,
    K: Clone + Debug + StableHashSerialize + Send + Sync,
{
    fn snapshot(&self) -> HotStoreState<C, P, A, K> {
        let state = self.state.read().expect("hot store state read lock");
        state
            .snapshot()
            .expect("PathMap-backed hot store must decode values it encoded")
    }

    // Continuations

    fn get_continuations(&self, channels: &[C]) -> Vec<WaitingContinuation<P, K>> {
        self.get_continuations_versioned(channels).1
    }

    fn get_continuations_versioned(
        &self,
        channels: &[C],
    ) -> (Option<ContinuationVersion>, Vec<WaitingContinuation<P, K>>) {
        metrics::counter!(HOT_STORE_GET_CONT_CALLS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(1);
        let state = self.state.read().expect("hot store state read lock");
        if let Some(records) = state.continuations.resting_records(channels) {
            let version = state.continuation_version(channels);
            if let Some(version) = &version {
                version.bind_paths(
                    records
                        .iter()
                        .map(|record| Arc::clone(record.path()))
                        .collect(),
                );
            }
            let result = with_installed(
                state.continuations.installed(channels),
                records
                    .into_iter()
                    .map(|record| record.value().clone())
                    .collect(),
            );
            return (version, result);
        }
        drop(state);
        metrics::counter!(HOT_STORE_GET_CONT_HISTORY_FILL_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(1);
        let from_history_store = self.get_cont_from_history_store(channels);
        let mut state = self.state.write().expect("hot store state write lock");
        if !state.continuations.is_loaded(channels) {
            state
                .continuations
                .set_group(channels.to_vec(), from_history_store);
        }
        let records = state
            .continuations
            .resting_records(channels)
            .expect("history-filled continuation group must be loaded");
        let version = state.continuation_version(channels);
        if let Some(version) = &version {
            version.bind_paths(
                records
                    .iter()
                    .map(|record| Arc::clone(record.path()))
                    .collect(),
            );
        }
        let result = records
            .into_iter()
            .map(|record| record.value().clone())
            .collect();
        (version, with_installed(state.continuations.installed(channels), result))
    }

    fn put_continuation(&self, channels: &[C], wc: WaitingContinuation<P, K>) -> Option<bool> {
        let __put_start = std::time::Instant::now();
        metrics::counter!(HOT_STORE_PUT_CONT_CALLS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(1);

        let has_existing = self
            .state
            .read()
            .expect("hot store state read lock")
            .continuations
            .is_loaded(channels);
        let from_history_store = if has_existing {
            None
        } else {
            metrics::counter!(HOT_STORE_PUT_CONT_HISTORY_FILL_METRIC, "source" => RSPACE_METRICS_SOURCE)
                .increment(1);
            Some(self.get_cont_from_history_store(channels))
        };

        let mut state = self.state.write().expect("hot store state write lock");
        if !state.continuations.is_loaded(channels) {
            state
                .continuations
                .set_group(channels.to_vec(), from_history_store.unwrap_or_default());
        }
        let existing_count = state.continuations.resting_len(channels).unwrap_or(0) as u64;
        metrics::counter!(HOT_STORE_PUT_CONT_EXISTING_COUNT_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(existing_count);

        let __ident_build_start = std::time::Instant::now();
        let prepared = PreparedContinuationRecord::encode_waiting(&wc);
        metrics::counter!(HOT_STORE_PUT_CONT_IDENTITY_BUILD_NS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(__ident_build_start.elapsed().as_nanos() as u64);
        let __cmp_start = std::time::Instant::now();
        let inserted = state.continuations.put_prepared(channels, wc, prepared);
        metrics::counter!(HOT_STORE_PUT_CONT_IDENTITY_COMPARE_NS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(__cmp_start.elapsed().as_nanos() as u64);
        if inserted {
            state.bump_continuation_revision(channels);
        } else {
            metrics::counter!(HOT_STORE_PUT_CONT_DUPLICATES_METRIC, "source" => RSPACE_METRICS_SOURCE)
                .increment(1);
        }
        metrics::counter!(HOT_STORE_PUT_CONT_TIME_NS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(__put_start.elapsed().as_nanos() as u64);
        Some(inserted)
    }

    fn install_continuation(&self, channels: &[C], wc: WaitingContinuation<P, K>) -> Option<()> {
        let mut state = self.state.write().expect("hot store state write lock");
        state.continuations.install(channels, wc);
        state.bump_continuation_revision(channels);
        Some(())
    }

    fn remove_continuation(&self, channels: &[C], index: i32) -> Option<()> {
        let mut state = hot_state_write(&self.state);
        if !state.continuations.is_loaded(channels) {
            let from_history_store = self.get_cont_from_history_store(channels);
            state
                .continuations
                .set_group(channels.to_vec(), from_history_store);
        }
        let is_installed = state.continuations.contains_installed(channels);
        let removing_installed = is_installed && index == 0;
        let removed_index = if is_installed { index - 1 } else { index };

        let removed = if removing_installed {
            warn!("Attempted to remove an installed continuation");
            None
        } else {
            let records = state
                .continuations
                .resting_records(channels)
                .expect("continuation group was loaded before removal");
            let path = usize::try_from(removed_index)
                .ok()
                .and_then(|index| records.get(index))
                .map(|record| Arc::clone(record.path()));
            match path {
                Some(path) => state.continuations.remove_path(channels, &path).map(|_| ()),
                None => {
                    warn!(index, "Index out of bounds when removing continuation");
                    None
                }
            }
        };
        if removed.is_some() {
            state.bump_continuation_revision(channels);
        }
        removed
    }

    fn try_visit_resting_receive_views(
        &self,
        channels: &[C],
        visitor: &mut dyn FnMut(&[u64], Result<OrderedReceiveView<'_>, OrderedReceiveError>),
    ) -> RestingReceiveVisit {
        let state = self.state.read().expect("hot store state read lock");
        state
            .continuations
            .visit_resting_receive_records(channels, visitor)
    }

    fn receive_revision(&self) -> Option<ReceiveRevision> {
        let state = self.state.read().expect("hot store state read lock");
        Some(state.receive_revision())
    }

    fn rebuild_receive_accelerator(&self) -> ReceiveAcceleratorClass {
        let opened = hot_state_write(&self.state).begin_receive_build();
        let published = opened.is_some_and(|build| {
            let built = build.snapshot.build_receive_index();
            hot_state_write(&self.state).finish_receive_build(&build.base, built)
        });
        match published {
            true => ReceiveAcceleratorClass::Coherent,
            false => hot_state_write(&self.state).rebuild_receive_index_locked(),
        }
    }

    fn receive_accelerator(&self) -> Option<ReceiveAcceleratorHandle<C, P, K>> {
        let state = self.state.read().expect("hot store state read lock");
        state.receive_handle()
    }

    fn detached_receive_accelerator(&self) -> Option<ReceiveAcceleratorHandle<C, P, K>> {
        let (revision, mut snapshot) = {
            let state = self.state.read().expect("hot store state read lock");
            (state.receive_revision(), state.continuations.clone())
        };
        match snapshot.rebuild_receive_index() {
            true => ReceiveAcceleratorHandle::new(revision, snapshot),
            false => None,
        }
    }

    fn receive_occurrences(
        &self,
        held: Option<&ReceiveAcceleratorHandle<C, P, K>>,
        channel: &C,
    ) -> ReceiveAnswer<C, P, K> {
        let state = self.state.read().expect("hot store state read lock");
        match held {
            Some(handle) => state.read_receive_occurrences(handle, channel),
            None => ReceiveAnswer::Fallback(state.vacant_receive_class()),
        }
    }

    fn current_receive_occurrences(&self, channel: &C) -> ReceiveAnswer<C, P, K> {
        let state = self.state.read().expect("hot store state read lock");
        match state.receive_handle() {
            Some(handle) => state.read_receive_occurrences(&handle, channel),
            None => ReceiveAnswer::Fallback(state.vacant_receive_class()),
        }
    }

    fn resolve_receive(&self, id: &ReceiveId<C, P, K>) -> Option<ReceiveTarget<C, P, K>> {
        let state = self.state.read().expect("hot store state read lock");
        id.resolve(&state.continuations, &state.receive_revision())
    }

    fn hydrate_receive_scope(&self, channel: &C) {
        for group in self.get_joins(channel) {
            self.get_continuations_versioned(&group);
        }
        let installed = self
            .state
            .read()
            .expect("hot store state read lock")
            .unloaded_installed_groups(channel);
        for group in installed {
            self.get_continuations_versioned(&group);
        }
    }

    // Data

    fn get_data(&self, channel: &C) -> Vec<Datum<A>> {
        metrics::counter!(HOT_STORE_GET_DATA_CALLS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(1);
        let state = self.state.read().expect("hot store state read lock");
        if let Some(data) = state
            .data
            .channel_data(channel)
            .expect("PathMap-backed hot store must decode values it encoded")
        {
            return data;
        }
        drop(state);
        self.ensure_data_loaded(channel);
        self.state
            .read()
            .expect("hot store state read lock")
            .data
            .channel_data(channel)
            .expect("PathMap-backed hot store must decode values it encoded")
            .expect("loaded channel must have a data view")
    }

    fn get_candidate_pool(&self, channel: &C) -> CandidatePool<A> {
        metrics::counter!(HOT_STORE_GET_DATA_CALLS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(1);
        let state = self.state.read().expect("hot store state read lock");
        if let Some(pool) = state
            .data
            .channel_pool(channel)
            .expect("PathMap-backed hot store must decode values it encoded")
        {
            return pool;
        }
        drop(state);
        self.ensure_data_loaded(channel);
        self.state
            .read()
            .expect("hot store state read lock")
            .data
            .channel_pool(channel)
            .expect("PathMap-backed hot store must decode values it encoded")
            .expect("loaded channel must have a candidate pool")
    }

    fn put_datum(&self, channel: &C, d: Datum<A>) {
        let __start = std::time::Instant::now();
        metrics::counter!(HOT_STORE_PUT_DATUM_CALLS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(1);
        let has_existing = self
            .state
            .read()
            .expect("hot store state read lock")
            .data
            .is_loaded(channel);
        let from_history_store = if has_existing {
            None
        } else {
            metrics::counter!(HOT_STORE_PUT_DATUM_HISTORY_FILL_METRIC, "source" => RSPACE_METRICS_SOURCE)
                .increment(1);
            Some(self.get_data_from_history_store(channel))
        };

        let mut state = hot_state_write(&self.state);
        if state.data.is_loaded(channel) {
            state
                .data
                .insert(channel, &d)
                .expect("datum multiplicity must fit in u64");
        } else {
            let inserted_path = RSpaceDataIndex::datum_path(channel, &d);
            let mut new_data = from_history_store.unwrap_or_default();
            new_data.push(d);
            state
                .data
                .set_channel(channel.clone(), new_data)
                .expect("history data must fit the PathMap multiplicity model");
            state
                .data
                .record_pending_datum_addition(&inserted_path)
                .expect("a cold-channel datum insertion is representable");
        }
        state.advance_journal_version();
        metrics::counter!(HOT_STORE_PUT_DATUM_TIME_NS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(__start.elapsed().as_nanos() as u64);
    }

    fn remove_datum(&self, channel: &C, index: i32) -> Result<(), RSpaceError> {
        let has_existing = self
            .state
            .read()
            .expect("hot store state read lock")
            .data
            .is_loaded(channel);
        let from_history_store = if has_existing {
            None
        } else {
            Some(self.get_data_from_history_store(channel))
        };

        let mut state = self.state.write().expect("hot store state write lock");
        if !state.data.is_loaded(channel) {
            state
                .data
                .set_channel(channel.clone(), from_history_store.unwrap_or_default())?;
        }
        match state.data.remove_index(channel, index) {
            Ok(()) => {
                state.advance_journal_version();
                Ok(())
            }
            Err(DataIndexError::IndexOutOfBounds { cardinality, .. }) => {
                Err(RSpaceError::BugFoundError(format!(
                    "Index {index} out of bounds when removing datum (len={cardinality})"
                )))
            }
            Err(error) => Err(error.into()),
        }
    }

    fn remove_datum_path(&self, path: &[u8]) -> Result<(), RSpaceError> {
        let mut state = hot_state_write(&self.state);
        if state.data.remove_path(path) {
            state.advance_journal_version();
            Ok(())
        } else {
            Err(RSpaceError::BugFoundError(
                "Stored datum path was absent during removal".to_string(),
            ))
        }
    }

    fn apply_datum_delta(&self, delta: &BagDelta) -> Result<(), RSpaceError> {
        let mut state = hot_state_write(&self.state);
        state.data.apply_delta(delta)?;
        if !delta.is_empty() {
            state.advance_journal_version();
        }
        Ok(())
    }

    fn pending_data_delta(&self) -> Result<Option<BagDelta>, RSpaceError> {
        Ok(Some(
            self.state
                .read()
                .expect("hot store state read lock")
                .data
                .pending_delta()
                .clone(),
        ))
    }

    fn data_count_at_path(&self, path: &[u8]) -> Result<u64, RSpaceError> {
        Ok(self
            .state
            .read()
            .expect("hot store state read lock")
            .data
            .count_path(path))
    }

    fn pending_join_delta(&self) -> Result<Option<BagDelta>, RSpaceError> {
        Ok(Some(
            self.state
                .read()
                .expect("hot store state read lock")
                .joins
                .pending_delta()
                .clone(),
        ))
    }

    fn join_count_at_path(&self, path: &[u8]) -> Result<u64, RSpaceError> {
        Ok(self
            .state
            .read()
            .expect("hot store state read lock")
            .joins
            .count_path(path))
    }

    fn pending_continuation_delta(&self) -> Result<Option<BagDelta>, RSpaceError> {
        Ok(Some(
            self.state
                .read()
                .expect("hot store state read lock")
                .continuations
                .pending_delta()
                .clone(),
        ))
    }

    fn pending_continuation_content(
        &self,
    ) -> Result<Option<PathMap<ContentTransition>>, RSpaceError> {
        Ok(Some(
            self.state
                .read()
                .expect("hot store state read lock")
                .continuations
                .pending_content()
                .clone(),
        ))
    }

    fn pending_continuation_source_bytes(&self) -> Result<Option<PathMap<Arc<[u8]>>>, RSpaceError> {
        Ok(Some(
            self.state
                .read()
                .expect("hot store state read lock")
                .continuations
                .pending_source_bytes()
                .clone(),
        ))
    }

    fn pending_deltas(&self) -> Result<PendingDeltas, RSpaceError> {
        Ok(self
            .state
            .read()
            .expect("hot store state read lock")
            .pending_journal())
    }

    fn pending_mutation_observation(&self) -> Result<PendingMutationObservation, RSpaceError> {
        Ok(self
            .state
            .read()
            .expect("hot store state read lock")
            .pending_observation())
    }

    fn capture_pending_journal(
        &self,
        base: CatalogSlotV1,
    ) -> Result<CapturedHotJournalV1, RSpaceError> {
        let state = self.state.read().expect("hot store state read lock");
        let binding = state.durable_journal.as_ref().ok_or_else(|| {
            RSpaceError::JournalConflict("hot store has no durable journal authority".to_owned())
        })?;
        match binding.base {
            JournalBaseV1::Bound(bound) if bound == base => {}
            JournalBaseV1::Detached(_) => {
                return Err(RSpaceError::JournalConflict(
                    "durable journal is detached from every catalog slot".to_owned(),
                ));
            }
            JournalBaseV1::Bound(_) | JournalBaseV1::Invalidated => {
                return Err(RSpaceError::JournalConflict(
                    "durable journal base changed before capture".to_owned(),
                ));
            }
        }
        Ok(CapturedHotJournalV1 {
            identity: Arc::new(JournalCaptureIdentityV1),
            version: binding.version.clone(),
            base,
            origin: base.state().manifest(),
            kind: JournalCaptureKindV1::SameBase,
            observation: state.pending_observation(),
        })
    }

    fn capture_cross_base_journal(
        &self,
        origin: ObjectRefV1,
        publication_base: CatalogSlotV1,
        registry: &GenerationLeaseRegistryV1,
    ) -> Result<CapturedHotJournalV1, RSpaceError> {
        let state = self.state.read().expect("hot store state read lock");
        let binding = state.durable_journal.as_ref().ok_or_else(|| {
            RSpaceError::JournalConflict("hot store has no durable journal authority".to_owned())
        })?;
        match (binding.registry.shares_registry(registry), binding.recorded_manifest()) {
            (true, Some(recorded)) if recorded == origin => Ok(CapturedHotJournalV1 {
                identity: Arc::new(JournalCaptureIdentityV1),
                version: binding.version.clone(),
                base: publication_base,
                origin,
                kind: JournalCaptureKindV1::CrossBase,
                observation: state.pending_observation(),
            }),
            (false, _) => Err(RSpaceError::JournalConflict(
                "durable journal belongs to another directory authority".to_owned(),
            )),
            (true, Some(_)) => Err(RSpaceError::JournalConflict(
                "durable journal was recorded against another state than the capture's origin"
                    .to_owned(),
            )),
            (true, None) => Err(RSpaceError::JournalConflict(
                "durable journal base was invalidated and cannot be captured".to_owned(),
            )),
        }
    }

    fn validate_pending_journal(&self, captured: &CapturedHotJournalV1) -> Result<(), RSpaceError> {
        let state = self.state.read().expect("hot store state read lock");
        if !state.matches_captured_journal(captured) {
            return Err(RSpaceError::JournalConflict(
                "captured durable journal is no longer current".to_owned(),
            ));
        }
        Ok(())
    }

    fn validate_journal_authority(
        &self,
        base: CatalogSlotV1,
        registry: &GenerationLeaseRegistryV1,
    ) -> Result<(), RSpaceError> {
        let state = self.state.read().expect("hot store state read lock");
        if !state.durable_journal.as_ref().is_some_and(|binding| {
            binding.is_bound_to(base) && binding.registry.shares_registry(registry)
        }) {
            return Err(RSpaceError::JournalConflict(
                "hot store is not bound to this durable base and directory authority".to_owned(),
            ));
        }
        Ok(())
    }

    fn bind_journal_authority(&self, slot: CatalogSlotV1) -> Result<(), RSpaceError> {
        let mut state = hot_state_write(&self.state);
        let binding = state.durable_journal.as_mut().ok_or_else(|| {
            RSpaceError::JournalConflict("hot store has no durable journal authority".to_owned())
        })?;
        match binding.recorded_manifest() {
            Some(manifest) if manifest == slot.state().manifest() => {
                binding.base = JournalBaseV1::Bound(slot);
                Ok(())
            }
            Some(_) => Err(RSpaceError::JournalConflict(
                "catalog slot names another state than the durable journal's base".to_owned(),
            )),
            None => Err(RSpaceError::JournalConflict(
                "durable journal base was invalidated and cannot be bound".to_owned(),
            )),
        }
    }

    fn acknowledge_pending_journal(
        &self,
        captured: &CapturedHotJournalV1,
        expected: CatalogSlotV1,
        admitted: CatalogSlotV1,
    ) -> Result<HotJournalAcknowledgementV1, RSpaceError> {
        let mut state = hot_state_write(&self.state);
        if expected != admitted {
            return Err(RSpaceError::JournalConflict(
                "recovered terminal does not match the captured publication".to_owned(),
            ));
        }
        let binding = state.durable_journal.as_ref().ok_or_else(|| {
            RSpaceError::JournalConflict("hot store has no durable journal authority".to_owned())
        })?;
        if state.matches_acknowledged_journal(captured, expected) {
            return Ok(HotJournalAcknowledgementV1::AlreadyCommitted);
        }
        if !state.matches_captured_journal(captured) ||
            expected.store_id() != captured.base.store_id() ||
            expected.profile_id() != captured.base.profile_id() ||
            Some(expected.epoch()) != captured.base.epoch().checked_add(1) ||
            Some(expected.state().generation()) !=
                captured.base.state().generation().checked_add(1) ||
            Some(expected.commit_sequence()) != captured.base.commit_sequence().successor().ok() ||
            state.pending_journal_is_empty()
        {
            return Err(RSpaceError::JournalConflict(
                "durable acknowledgement is not bound to this exact journal and successor"
                    .to_owned(),
            ));
        }
        let next = binding.version.next();
        let marker = HotJournalAckMarkerV1 {
            identity: Arc::clone(&captured.identity),
            version: next.clone(),
            terminal: expected,
        };
        state.clear_pending_journal();
        let binding = state
            .durable_journal
            .as_mut()
            .expect("journal clearing preserves its authority");
        binding.base = JournalBaseV1::Bound(expected);
        binding.version = next;
        binding.acknowledged = Some(marker);
        Ok(HotJournalAcknowledgementV1::Committed)
    }

    fn validate_journal_acknowledgement(
        &self,
        captured: &CapturedHotJournalV1,
        terminal: CatalogSlotV1,
        registry: &GenerationLeaseRegistryV1,
    ) -> Result<(), RSpaceError> {
        let state = self.state.read().expect("hot store state read lock");
        if !state.matches_acknowledged_journal(captured, terminal) ||
            !state
                .durable_journal
                .as_ref()
                .is_some_and(|binding| binding.registry.shares_registry(registry))
        {
            return Err(RSpaceError::JournalConflict(
                "exact durable acknowledgement marker is no longer current".to_owned(),
            ));
        }
        Ok(())
    }

    fn commit_pending_deltas(&self, expected: &PendingDeltas) -> Result<(), RSpaceError> {
        let mut state = hot_state_write(&self.state);
        if state.durable_journal.is_some() {
            return Err(RSpaceError::JournalConflict(
                "durable journals require identity-bound acknowledgement".to_owned(),
            ));
        }
        if !state.matches_pending_journal(expected) {
            return Err(RSpaceError::JournalConflict(
                "hot-store journal changed before durable commit acknowledgement".to_owned(),
            ));
        }
        state.clear_pending_journal();
        Ok(())
    }

    fn restore_pending_deltas(
        &self,
        data: BagDelta,
        continuations: BagDelta,
        continuation_content: PathMap<ContentTransition>,
        continuation_source_bytes: PathMap<Arc<[u8]>>,
        joins: BagDelta,
    ) -> Result<(), RSpaceError> {
        let mut state = hot_state_write(&self.state);
        state.data.restore_pending_delta(data);
        state.continuations.restore_pending_delta(continuations);
        state
            .continuations
            .restore_pending_content(continuation_content);
        state
            .continuations
            .restore_pending_source_bytes(continuation_source_bytes);
        state.joins.restore_pending_delta(joins);
        state.advance_journal_version();
        Ok(())
    }

    fn rebase_pending_deltas(
        &self,
        data: &BagDelta,
        continuations: &BagDelta,
        continuation_content: &PathMap<ContentTransition>,
        continuation_source_bytes: &PathMap<Arc<[u8]>>,
        joins: &BagDelta,
    ) -> Result<(), RSpaceError> {
        let mut state = hot_state_write(&self.state);
        let mut domain = JournalRebaseDomain {
            data: state.data.loaded_channels()?.into_iter().collect(),
            continuations: state.continuations.loaded_groups().into_iter().collect(),
            joins: state.joins.loaded_channels().into_iter().collect(),
        };
        domain.add_data_keys(data)?;
        domain.add_join_keys(joins)?;
        domain.add_continuation_identity_keys("continuation content", continuation_content)?;
        domain.add_continuation_identity_keys("continuation source", continuation_source_bytes)?;
        domain.check_continuation_keys(continuations)?;

        let mut rebased_data = BagDelta::new();
        for (marker, channel) in &domain.data {
            let base = self.history_reader_base.get_data_canonical(channel);
            state
                .data
                .record_rebased_channel(marker, base, &mut rebased_data)?;
        }
        let mut rebased_continuations = BagDelta::new();
        let mut rebased_content = PathMap::new();
        let mut rebased_source_bytes = PathMap::new();
        for channels in domain.continuations.values() {
            let base = self
                .history_reader_base
                .get_continuations_canonical(channels);
            state
                .continuations
                .record_rebased_group(
                    channels,
                    base,
                    &mut rebased_continuations,
                    &mut rebased_content,
                    &mut rebased_source_bytes,
                )
                .map_err(DataIndexError::Bag)?;
        }
        let mut rebased_joins = BagDelta::new();
        for channel in domain.joins.values() {
            let base = self.history_reader_base.get_joins_canonical(channel);
            state
                .joins
                .record_rebased_channel(channel, base, &mut rebased_joins)
                .map_err(DataIndexError::Bag)?;
        }

        state.data.restore_pending_delta(rebased_data);
        state
            .continuations
            .restore_pending_delta(rebased_continuations);
        state.joins.restore_pending_delta(rebased_joins);
        state.continuations.restore_pending_content(rebased_content);
        state
            .continuations
            .restore_pending_source_bytes(rebased_source_bytes);
        state.advance_journal_version();
        Ok(())
    }

    fn apply_accepted_match(
        &self,
        transition: AcceptedMatch<C>,
    ) -> Result<AcceptedMatchOutcome<P, K>, RSpaceError> {
        // History reads are completed before the linearization lock. They are
        // proposals only: a stale transition returns without installing them
        // into the hot cache, so rejection is observably non-mutating.
        let missing_join_channels = {
            let state = self.state.read().expect("hot store state read lock");
            transition
                .join_channels
                .iter()
                .filter(|channel| !state.joins.is_loaded(*channel))
                .cloned()
                .collect::<Vec<_>>()
        };
        let mut prepared_joins = missing_join_channels
            .into_iter()
            .map(|channel| {
                let joins = self.get_joins_from_history_store(&channel);
                (channel, joins)
            })
            .collect::<Vec<_>>();

        let mut state = hot_state_write(&self.state);

        let mut authoritative_continuation = None;
        let mut ordinary_removal_path = None;
        if let Some(expected) = &transition.continuation {
            let current_version = state.continuation_version(&transition.channels);
            if expected.address.version.is_none() || expected.address.version != current_version {
                return Ok(AcceptedMatchOutcome::Stale);
            }

            let installed = state.continuations.installed(&transition.channels);
            let (current, ordinary_path, is_installed) =
                if installed.is_some() && expected.address.index == 0 {
                    (installed, None, true)
                } else {
                    let adjusted = if installed.is_some() {
                        expected.address.index.checked_sub(1)
                    } else {
                        Some(expected.address.index)
                    };
                    let Some(index) = adjusted.and_then(|index| usize::try_from(index).ok()) else {
                        return Ok(AcceptedMatchOutcome::Stale);
                    };
                    let path = expected
                        .address
                        .version
                        .as_ref()
                        .and_then(|version| version.path(index));
                    let current = path
                        .as_deref()
                        .and_then(|path| state.continuations.get_path(&transition.channels, path))
                        .map(|record| record.value().clone());
                    (current, path, false)
                };
            let Some(current) = current else {
                return Ok(AcceptedMatchOutcome::Stale);
            };
            if current.source != expected.source ||
                current.persist != expected.persist ||
                current.peeks != expected.peeks ||
                (is_installed && !current.persist)
            {
                return Ok(AcceptedMatchOutcome::Stale);
            }
            authoritative_continuation = Some(current.clone());
            if !current.persist && !is_installed {
                ordinary_removal_path = ordinary_path;
            }
        }

        if transition
            .required_persistent_paths
            .iter()
            .any(|path| state.data.index().count_path(path) == 0)
        {
            return Ok(AcceptedMatchOutcome::Stale);
        }

        if !transition
            .datum_delta
            .guards_satisfied_by(|path| state.data.index().count_path(path))
        {
            return Ok(AcceptedMatchOutcome::Stale);
        }

        match state.data.apply_delta(transition.datum_delta.delta()) {
            Ok(()) => {}
            Err(DataIndexError::Bag(BagError::InsufficientMultiplicity { .. })) => {
                return Ok(AcceptedMatchOutcome::Stale);
            }
            Err(error) => return Err(error.into()),
        }
        if !transition.datum_delta.delta().is_empty() {
            state.advance_journal_version();
        }

        if let Some(path) = ordinary_removal_path {
            state
                .continuations
                .remove_path(&transition.channels, &path)
                .expect("a validated continuation path must remain present");
            state.bump_continuation_revision(&transition.channels);

            let group_remains = state.continuations.contains_installed(&transition.channels) ||
                state
                    .continuations
                    .resting_len(&transition.channels)
                    .is_some_and(|len| len != 0);
            if !group_remains {
                for channel in transition.join_channels {
                    if !state.joins.is_loaded(&channel) {
                        let prepared = prepared_joins
                            .iter_mut()
                            .find(|(prepared_channel, _)| prepared_channel == &channel)
                            .map(|(_, joins)| std::mem::take(joins))
                            .unwrap_or_default();
                        state.joins.set_channel(channel.clone(), prepared);
                    }
                    let _ = state.joins.remove(&channel, &transition.channels);
                }
            }
        }

        Ok(AcceptedMatchOutcome::Committed {
            continuation: authoritative_continuation,
        })
    }

    // Joins

    fn get_joins(&self, channel: &C) -> Vec<Vec<C>> {
        metrics::counter!(HOT_STORE_GET_JOINS_CALLS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(1);
        let state = self.state.read().expect("hot store state read lock");
        let joins = state.joins.resting_values(channel);
        let installed_joins = state.joins.installed_values(channel);
        drop(state);

        match joins {
            Some(joins_data) => {
                let mut result = installed_joins;
                result.extend(joins_data);
                result
            }
            None => {
                metrics::counter!(HOT_STORE_GET_JOINS_HISTORY_FILL_METRIC, "source" => RSPACE_METRICS_SOURCE)
                    .increment(1);
                let from_history_store = self.get_joins_from_history_store(channel);
                let mut state = self.state.write().expect("hot store state write lock");
                if !state.joins.is_loaded(channel) {
                    state.joins.set_channel(channel.clone(), from_history_store);
                }
                let mut result = state.joins.installed_values(channel);
                result.extend(
                    state
                        .joins
                        .resting_values(channel)
                        .expect("history-filled join channel must be loaded"),
                );
                result
            }
        }
    }

    fn put_join(&self, channel: &C, join: &[C]) -> Option<()> {
        let __start = std::time::Instant::now();
        metrics::counter!(HOT_STORE_PUT_JOIN_CALLS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(1);
        let has_existing = self
            .state
            .read()
            .expect("hot store state read lock")
            .joins
            .is_loaded(channel);
        let from_history_store = if has_existing {
            None
        } else {
            metrics::counter!(HOT_STORE_PUT_JOIN_HISTORY_FILL_METRIC, "source" => RSPACE_METRICS_SOURCE)
                .increment(1);
            Some(self.get_joins_from_history_store(channel))
        };

        let mut state = self.state.write().expect("hot store state write lock");
        if !state.joins.is_loaded(channel) {
            state
                .joins
                .set_channel(channel.clone(), from_history_store.unwrap_or_default());
        }
        if state.joins.put(channel, join) {
            state.advance_journal_version();
        }
        metrics::counter!(HOT_STORE_PUT_JOIN_TIME_NS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(__start.elapsed().as_nanos() as u64);
        Some(())
    }

    fn install_join(&self, channel: &C, join: &[C]) -> Option<()> {
        let mut state = self.state.write().expect("hot store state write lock");
        if state.joins.install(channel, join) {
            state.advance_journal_version();
        }
        Some(())
    }

    fn remove_join(&self, channel: &C, join: &[C]) -> Option<()> {
        let mut state = hot_state_write(&self.state);
        let has_join_in_state = state.joins.is_loaded(channel);
        let current_continuations = {
            let mut conts = state
                .continuations
                .installed(join)
                .map(|c| vec![c.clone()])
                .unwrap_or_default();
            conts.extend(
                state
                    .continuations
                    .resting_values(join)
                    .unwrap_or_else(|| self.get_cont_from_history_store(join)),
            );
            conts
        };

        let do_remove = current_continuations.is_empty();

        if !do_remove {
            if !has_join_in_state {
                let joins_in_history_store = self.get_joins_from_history_store(channel);
                state
                    .joins
                    .set_channel(channel.clone(), joins_in_history_store);
            }
            Some(())
        } else {
            if !has_join_in_state {
                let joins_in_history_store = self.get_joins_from_history_store(channel);
                state
                    .joins
                    .set_channel(channel.clone(), joins_in_history_store);
            }
            if state.joins.remove(channel, join).is_none() {
                warn!("Join not found when removing join");
            } else {
                state.advance_journal_version();
            }
            Some(())
        }
    }

    fn changes(&self) -> Vec<HotStoreAction<C, P, A, K>> {
        let cache = self.state.read().expect("hot store state read lock");
        let capacity = cache
            .continuations
            .loaded_group_count()
            .checked_add(cache.data.loaded_channel_count())
            .and_then(|count| count.checked_add(cache.joins.loaded_channel_count()))
            .expect("resident group counts must fit the address space that stores them");
        let mut actions = Vec::with_capacity(capacity);

        cache
            .continuations
            .visit_loaded_records(|channels, continuations| {
                actions.push(if continuations.is_empty() {
                    HotStoreAction::Delete(DeleteAction::DeleteContinuations(DeleteContinuations {
                        channels: channels.to_vec(),
                    }))
                } else {
                    HotStoreAction::Insert(InsertAction::InsertContinuations(InsertContinuations {
                        channels: channels.to_vec(),
                        continuations,
                    }))
                });
            });

        cache
            .data
            .try_visit_loaded_records(|channel, data| {
                actions.push(if data.is_empty() {
                    HotStoreAction::Delete(DeleteAction::DeleteData(DeleteData { channel }))
                } else {
                    HotStoreAction::Insert(InsertAction::InsertData(InsertData { channel, data }))
                });
                Ok(())
            })
            .expect("PathMap-backed hot store must decode values it encoded");

        cache.joins.visit_loaded_records(|channel, joins| {
            actions.push(if joins.is_empty() {
                HotStoreAction::Delete(DeleteAction::DeleteJoins(DeleteJoins { channel }))
            } else {
                HotStoreAction::Insert(InsertAction::InsertJoins(InsertJoins { channel, joins }))
            });
        });

        actions
    }

    fn checkpoint_changes(&self) -> Result<Vec<HotStoreAction<C, P, A, K>>, RSpaceError> {
        let cache = self.state.read().expect("hot store state read lock");
        let group_count = cache
            .continuations
            .loaded_group_count()
            .checked_add(cache.data.loaded_channel_count())
            .and_then(|count| count.checked_add(cache.joins.loaded_channel_count()))
            .ok_or(RecordIndexError::EncodedCardinalityOverflow(u128::MAX))?;
        let mut actions = Vec::new();
        actions
            .try_reserve_exact(group_count)
            .map_err(|_| RecordIndexError::AllocationFailure(group_count))?;

        cache
            .continuations
            .try_visit_loaded_canonical_plans(|plan| {
                actions.push(if plan.record_count() == 0 {
                    HotStoreAction::Delete(DeleteAction::DeleteCanonicalCollection(
                        DeleteCanonicalCollection::new(
                            plan.kind(),
                            plan.history_key_hash().clone(),
                        ),
                    ))
                } else {
                    let record = plan.into_record_group()?;
                    HotStoreAction::Insert(InsertAction::InsertCanonicalCollection(
                        InsertCanonicalCollection::from_trusted_parts(
                            record.kind,
                            record.history_key_hash,
                            record.encoded_persisted_data,
                            record.record_count,
                        ),
                    ))
                });
                Ok(())
            })?;

        cache.data.try_visit_loaded_canonical_records(|record| {
            actions.push(if record.datum_count == 0 {
                HotStoreAction::Delete(DeleteAction::DeleteCanonicalData(DeleteCanonicalData::new(
                    record.channel_hash,
                )))
            } else {
                HotStoreAction::Insert(InsertAction::InsertCanonicalData(
                    InsertCanonicalData::from_trusted_parts(
                        record.channel_hash,
                        record.encoded_persisted_data,
                        record.datum_count,
                    ),
                ))
            });
            Ok(())
        })?;

        cache.joins.try_visit_loaded_canonical_plans(|plan| {
            actions.push(if plan.record_count() == 0 {
                HotStoreAction::Delete(DeleteAction::DeleteCanonicalCollection(
                    DeleteCanonicalCollection::new(plan.kind(), plan.history_key_hash().clone()),
                ))
            } else {
                let record = plan.into_record_group()?;
                HotStoreAction::Insert(InsertAction::InsertCanonicalCollection(
                    InsertCanonicalCollection::from_trusted_parts(
                        record.kind,
                        record.history_key_hash,
                        record.encoded_persisted_data,
                        record.record_count,
                    ),
                ))
            });
            Ok(())
        })?;

        debug_assert_eq!(actions.len(), group_count);
        Ok(actions)
    }

    fn to_map(&self) -> HashMap<Vec<C>, Row<P, A, K>> {
        let state = self.state.read().expect("hot store state read lock");
        let mut map = HashMap::with_capacity(
            state.data.loaded_channel_count() +
                state.continuations.loaded_group_count() +
                state.continuations.installed_count(),
        );

        // Data and continuations use different key shapes: a datum is indexed
        // by one channel, while a joined continuation is indexed by its full
        // channel vector. Build the union of those domains. Iterating only the
        // data keys silently dropped continuation-only rows and every join.
        for (channel, data) in state
            .data
            .loaded_records()
            .expect("PathMap-backed hot store must decode values it encoded")
        {
            if !data.is_empty() {
                map.insert(vec![channel], Row {
                    data,
                    wks: Vec::new(),
                });
            }
        }

        for (channels, continuations) in state.continuations.loaded_records() {
            if !continuations.is_empty() {
                map.entry(channels.to_vec())
                    .or_insert_with(|| Row {
                        data: Vec::new(),
                        wks: Vec::new(),
                    })
                    .wks
                    .extend(continuations);
            }
        }

        for (channels, continuation) in state.continuations.installed_records() {
            map.entry(channels.to_vec())
                .or_insert_with(|| Row {
                    data: Vec::new(),
                    wks: Vec::new(),
                })
                .wks
                .push(continuation);
        }

        map
    }

    fn print(&self) {
        let hot_store_state = self.state.read().expect("hot store state read lock");
        println!("\nHot Store");

        println!("Continuations:");
        for (key, value) in hot_store_state.continuations.loaded_records() {
            println!("Key: {:?}, Value: {:?}", key, value);
        }

        println!("\nInstalled Continuations:");
        for (key, value) in hot_store_state.continuations.installed_records() {
            println!("Key: {:?}, Value: {:?}", key, value);
        }

        println!("\nData:");
        for (key, value) in hot_store_state
            .data
            .loaded_records()
            .expect("PathMap-backed hot store must decode values it encoded")
        {
            println!("Key: {:?}, Value: {:?}", key, value);
        }

        println!("\nJoins:");
        for (key, value) in hot_store_state.joins.loaded_records() {
            println!("Key: {:?}, Value: {:?}", key, value);
        }

        println!("\nInstalled Joins:");
        for (key, value) in hot_store_state.joins.installed_records() {
            println!("Key: {:?}, Value: {:?}", key, value);
        }

        let history_cache_state = self.history_cache.read().expect("history cache read lock");
        println!("\nHistory Cache");

        println!("Continuations:");
        for entry in history_cache_state.continuations.iter() {
            let (key, value) = (entry.0, entry.1);
            println!("Key: {:?}, Value: {:?}", key, value);
        }

        println!("\nData:");
        for entry in history_cache_state.datums.iter() {
            let (key, value) = (entry.0, entry.1);
            println!("Key: {:?}, Value: {:?}", key, value);
        }

        println!("\nJoins:");
        for entry in history_cache_state.joins.iter() {
            let (key, value) = (entry.0, entry.1);
            println!("Key: {:?}, Value: {:?}", key, value);
        }
    }

    fn clear(&self) {
        let mut state = self.state.write().expect("hot store state write lock");
        state.invalidate_journal_base();
        state.bump_continuation_generation();
        state.continuations.clear();
        state.data.clear();
        state.joins.clear();
        state.receive_build = None;
        drop(state);

        let mut history_cache = self
            .history_cache
            .write()
            .expect("history cache write lock");
        history_cache.continuations.clear();
        history_cache.datums.clear();
        history_cache.joins.clear();
        metrics::gauge!(HOT_STORE_HISTORY_CONT_CACHE_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(0.0);
        metrics::gauge!(HOT_STORE_HISTORY_DATA_CACHE_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(0.0);
        metrics::gauge!(HOT_STORE_HISTORY_JOINS_CACHE_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(0.0);
        metrics::gauge!(HOT_STORE_HISTORY_CONT_CACHE_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(0.0);
        metrics::gauge!(HOT_STORE_HISTORY_DATA_CACHE_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(0.0);
        metrics::gauge!(HOT_STORE_HISTORY_JOINS_CACHE_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(0.0);
        metrics::gauge!(HOT_STORE_STATE_INSTALLED_CONT_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(0.0);
        metrics::gauge!(HOT_STORE_STATE_INSTALLED_JOINS_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(0.0);
        metrics::gauge!(HOT_STORE_STATE_INSTALLED_CONT_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(0.0);
        metrics::gauge!(HOT_STORE_STATE_INSTALLED_JOINS_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(0.0);

        Self::update_hot_store_state_metrics(
            &self.state.read().expect("hot store state read lock"),
        );
    }

    // See rspace/src/test/scala/coop/rchain/rspace/test/package.scala
    fn is_empty(&self) -> bool {
        let state = self.state.read().expect("hot store state read lock");
        !state.continuations.has_nonempty_loaded_group() &&
            !state.data.has_nonempty_loaded_channel() &&
            !state.joins.has_nonempty_loaded_group()
    }

    fn set_state(&self, new_state: HotStoreState<C, P, A, K>) {
        let mut state = self
            .state
            .write()
            .expect("hot store state write lock for set_state");
        let mut replacement = InMemHotStoreState::from_snapshot(new_state)
            .expect("snapshot data must fit the PathMap multiplicity model");
        replacement.durable_journal = state.durable_journal.take();
        replacement.invalidate_journal_base();
        *state = replacement;
    }
}

struct JournalRebaseDomain<C> {
    data: BTreeMap<Vec<u8>, C>,
    continuations: BTreeMap<Vec<u8>, Arc<[C]>>,
    joins: BTreeMap<Vec<u8>, C>,
}

fn malformed_journal_key(journal: &str, error: impl std::fmt::Display) -> RSpaceError {
    RSpaceError::JournalConflict(format!(
        "captured {journal} journal holds a malformed key: {error}"
    ))
}

fn noncanonical_journal_key(journal: &str) -> RSpaceError {
    RSpaceError::JournalConflict(format!(
        "captured {journal} journal holds a key that is not canonical for its channels"
    ))
}

fn rebase_domain_member<'domain, V>(
    domain: &'domain BTreeMap<Vec<u8>, V>,
    key: &[u8],
) -> Option<&'domain V> {
    domain
        .range::<[u8], _>((Bound::Unbounded, Bound::Included(key)))
        .next_back()
        .and_then(|(prefix, member)| key.starts_with(prefix).then_some(member))
}

impl<C> JournalRebaseDomain<C>
where C: RhoPathEncode + ColdStoreDecode
{
    fn add_data_keys(&mut self, journal: &BagDelta) -> Result<(), RSpaceError> {
        for (path, _) in journal.additions().iter().chain(journal.removals().iter()) {
            let view = RhoPathV1::parse_datum(&path)
                .map_err(|error| malformed_journal_key("data", error))?;
            let (marker, _) = path.split_at(view.channel_prefix_len());
            if self.data.contains_key(marker) {
                continue;
            }
            let channel = C::cold_decode(&view.channel_identity())
                .map_err(|error| malformed_journal_key("data", error))?;
            if RhoPathV1::datum_prefix(&channel) != marker {
                return Err(noncanonical_journal_key("data"));
            }
            self.data.insert(marker.to_vec(), channel);
        }
        Ok(())
    }

    fn add_join_keys(&mut self, journal: &BagDelta) -> Result<(), RSpaceError> {
        for (path, _) in journal.additions().iter().chain(journal.removals().iter()) {
            let view = RhoPathV1::parse_join(&path)
                .map_err(|error| malformed_journal_key("join", error))?;
            if rebase_domain_member(&self.joins, &path).is_some() {
                continue;
            }
            let channel = C::cold_decode(&view.indexed_channel_identity())
                .map_err(|error| malformed_journal_key("join", error))?;
            let prefix = RhoPathV1::join_prefix(&channel);
            if !path.starts_with(&prefix) {
                return Err(noncanonical_journal_key("join"));
            }
            self.joins.insert(prefix, channel);
        }
        Ok(())
    }

    fn add_continuation_identity_keys<V: Clone + Send + Sync + Unpin>(
        &mut self,
        journal: &str,
        keys: &PathMap<V>,
    ) -> Result<(), RSpaceError> {
        for (path, _) in keys.iter() {
            let view = RhoPathV1::parse_continuation(&path)
                .map_err(|error| malformed_journal_key(journal, error))?;
            if rebase_domain_member(&self.continuations, &path).is_some() {
                continue;
            }
            let channels = view
                .channel_identities()
                .map(|identity| C::cold_decode(&identity))
                .collect::<Result<Vec<C>, _>>()
                .map_err(|error| malformed_journal_key(journal, error))?;
            let prefix = RhoPathV1::continuation_prefix(&channels);
            if !path.starts_with(&prefix) {
                return Err(noncanonical_journal_key(journal));
            }
            self.continuations.insert(prefix, Arc::from(channels));
        }
        Ok(())
    }

    fn check_continuation_keys(&self, journal: &BagDelta) -> Result<(), RSpaceError> {
        for (path, _) in journal.additions().iter().chain(journal.removals().iter()) {
            if rebase_domain_member(&self.continuations, &path).is_none() {
                return Err(RSpaceError::JournalConflict(
                    "captured continuation journal holds a key outside every captured \
                     continuation group"
                        .to_owned(),
                ));
            }
        }
        Ok(())
    }
}

impl<C, P, A, K> InMemHotStore<C, P, A, K>
where
    C: Clone + Debug + Hash + Eq + RhoPathEncode + ColdStoreDecode + Sync + Send,
    P: Clone + Debug + StableHashSerialize + Sync + Send,
    A: Clone + Debug + Serialize + RhoPathEncode + ColdStoreDecode + Sync + Send,
    K: Clone + Debug + StableHashSerialize + Sync + Send,
{
    fn now_millis() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    fn state_metrics_update_interval_ms() -> u64 { HOT_STORE_STATE_METRICS_UPDATE_INTERVAL_MS }

    fn history_cache_metrics_update_interval_ms() -> u64 {
        HOT_STORE_HISTORY_CACHE_METRICS_UPDATE_INTERVAL_MS
    }

    fn should_emit_metrics(last_emit_at_ms: &AtomicU64, update_interval_ms: u64) -> bool {
        if update_interval_ms == 0 {
            return true;
        }

        let now = Self::now_millis();
        loop {
            let last = last_emit_at_ms.load(Ordering::Relaxed);
            if now.saturating_sub(last) < update_interval_ms {
                return false;
            }
            if last_emit_at_ms
                .compare_exchange_weak(last, now, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return true;
            }
        }
    }

    fn update_hot_store_state_metrics(state: &InMemHotStoreState<C, P, A, K>) {
        static LAST_EMIT_AT_MS: AtomicU64 = AtomicU64::new(0);
        if !Self::should_emit_metrics(&LAST_EMIT_AT_MS, Self::state_metrics_update_interval_ms()) {
            return;
        }

        let cont_items = state.continuations.resting_record_count();
        let data_items = state.data.cardinality();
        let joins_items = state.joins.resting_record_count();
        let installed_cont_items = state.continuations.installed_count();
        let installed_joins_items = state.joins.installed_record_count();

        metrics::gauge!(HOT_STORE_STATE_CONT_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(state.continuations.loaded_group_count() as f64);
        metrics::gauge!(HOT_STORE_STATE_DATA_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(state.data.loaded_channel_count() as f64);
        metrics::gauge!(HOT_STORE_STATE_JOINS_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(state.joins.loaded_channel_count() as f64);
        metrics::gauge!(HOT_STORE_STATE_INSTALLED_CONT_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(state.continuations.installed_count() as f64);
        metrics::gauge!(HOT_STORE_STATE_INSTALLED_JOINS_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(state.joins.installed_channel_count() as f64);
        metrics::gauge!(HOT_STORE_STATE_CONT_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(cont_items as f64);
        metrics::gauge!(HOT_STORE_STATE_DATA_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(data_items as f64);
        metrics::gauge!(HOT_STORE_STATE_JOINS_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(joins_items as f64);
        metrics::gauge!(HOT_STORE_STATE_INSTALLED_CONT_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(installed_cont_items as f64);
        metrics::gauge!(HOT_STORE_STATE_INSTALLED_JOINS_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(installed_joins_items as f64);
    }

    fn update_history_cache_metrics(cache: &HistoryStoreCache<C, P, A, K>) {
        static LAST_EMIT_AT_MS: AtomicU64 = AtomicU64::new(0);
        if !Self::should_emit_metrics(
            &LAST_EMIT_AT_MS,
            Self::history_cache_metrics_update_interval_ms(),
        ) {
            return;
        }

        let cont_items: usize = cache.continuations.values().map(|v| v.len()).sum();
        let data_items: usize = cache.datums.values().map(|v| v.len()).sum();
        let joins_items: usize = cache.joins.values().map(|v| v.len()).sum();

        metrics::gauge!(HOT_STORE_HISTORY_CONT_CACHE_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(cache.continuations.len() as f64);
        metrics::gauge!(HOT_STORE_HISTORY_DATA_CACHE_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(cache.datums.len() as f64);
        metrics::gauge!(HOT_STORE_HISTORY_JOINS_CACHE_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(cache.joins.len() as f64);
        metrics::gauge!(HOT_STORE_HISTORY_CONT_CACHE_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(cont_items as f64);
        metrics::gauge!(HOT_STORE_HISTORY_DATA_CACHE_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(data_items as f64);
        metrics::gauge!(HOT_STORE_HISTORY_JOINS_CACHE_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(joins_items as f64);
    }

    fn enforce_history_cache_bounds(cache: &mut HistoryStoreCache<C, P, A, K>) {
        let cont_items: usize = cache.continuations.values().map(|v| v.len()).sum();
        let data_items: usize = cache.datums.values().map(|v| v.len()).sum();
        let joins_items: usize = cache.joins.values().map(|v| v.len()).sum();

        if cache.continuations.len() >= MAX_HISTORY_STORE_CACHE_ENTRIES ||
            cont_items >= MAX_HISTORY_STORE_CACHE_CONT_ITEMS
        {
            metrics::counter!(HOT_STORE_HISTORY_CACHE_BULK_CLEAR_CONT_METRIC, "source" => RSPACE_METRICS_SOURCE)
                .increment(1);
            cache.continuations.clear();
        }
        if cache.datums.len() >= MAX_HISTORY_STORE_CACHE_ENTRIES ||
            data_items >= MAX_HISTORY_STORE_CACHE_DATA_ITEMS
        {
            metrics::counter!(HOT_STORE_HISTORY_CACHE_BULK_CLEAR_DATUMS_METRIC, "source" => RSPACE_METRICS_SOURCE)
                .increment(1);
            cache.datums.clear();
        }
        if cache.joins.len() >= MAX_HISTORY_STORE_CACHE_ENTRIES ||
            joins_items >= MAX_HISTORY_STORE_CACHE_JOIN_ITEMS
        {
            metrics::counter!(HOT_STORE_HISTORY_CACHE_BULK_CLEAR_JOINS_METRIC, "source" => RSPACE_METRICS_SOURCE)
                .increment(1);
            cache.joins.clear();
        }
    }

    fn get_cont_from_history_store(&self, channels: &[C]) -> Vec<WaitingContinuation<P, K>> {
        let mut cache = self
            .history_cache
            .write()
            .expect("history cache write lock");
        Self::enforce_history_cache_bounds(&mut cache);
        let channels_vec = channels.to_vec();
        let entry = cache.continuations.entry(channels_vec.clone());
        let result = match entry {
            Entry::Occupied(o) => o.get().clone(),
            Entry::Vacant(v) => {
                let ks = self
                    .history_reader_base
                    .get_continuations_canonical(&channels_vec);
                v.insert(ks.clone());
                ks
            }
        };
        Self::update_history_cache_metrics(&cache);
        result
    }

    fn get_data_from_history_store(&self, channel: &C) -> Vec<Datum<A>> {
        let mut cache = self
            .history_cache
            .write()
            .expect("history cache write lock");
        Self::enforce_history_cache_bounds(&mut cache);
        let entry = cache.datums.entry(channel.clone());
        let result = match entry {
            Entry::Occupied(o) => o.get().clone(),
            Entry::Vacant(v) => {
                let datums = self.history_reader_base.get_data_canonical(channel);
                v.insert(datums.clone());
                datums
            }
        };
        Self::update_history_cache_metrics(&cache);
        result
    }

    fn ensure_data_loaded(&self, channel: &C) {
        if self
            .state
            .read()
            .expect("hot store state read lock")
            .data
            .is_loaded(channel)
        {
            return;
        }
        metrics::counter!(HOT_STORE_GET_DATA_HISTORY_FILL_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(1);
        let data = self.get_data_from_history_store(channel);
        let mut state = self.state.write().expect("hot store state write lock");
        if !state.data.is_loaded(channel) {
            state
                .data
                .set_channel(channel.clone(), data)
                .expect("history data must fit the PathMap multiplicity model");
        }
    }

    fn get_joins_from_history_store(&self, channel: &C) -> Vec<Vec<C>> {
        let mut cache = self
            .history_cache
            .write()
            .expect("history cache write lock");
        Self::enforce_history_cache_bounds(&mut cache);
        let entry = cache.joins.entry(channel.clone());
        let result = match entry {
            Entry::Occupied(o) => o.get().clone(),
            Entry::Vacant(v) => {
                let joins = self.history_reader_base.get_joins_canonical(channel);
                v.insert(joins.clone());
                joins
            }
        };
        Self::update_history_cache_metrics(&cache);
        result
    }
}

#[cfg(test)]
#[path = "../../tests/hot_store/group_scoped_version.rs"]
mod group_scoped_version_tests;

#[cfg(test)]
#[path = "../../tests/hot_store/accepted_match_formal_bindings.rs"]
mod accepted_match_formal_bindings_tests;

#[cfg(test)]
#[path = "../../tests/hot_store/journal_commit.rs"]
mod journal_commit_tests;

#[cfg(test)]
#[path = "../../tests/hot_store/durable_reader_bootstrap.rs"]
mod durable_reader_bootstrap_tests;

#[cfg(test)]
#[path = "../../tests/hot_store/cross_base_capture.rs"]
mod cross_base_capture_tests;

#[cfg(test)]
#[path = "../../tests/hot_store/receive_revision_tokens.rs"]
mod receive_revision_tokens_tests;

#[cfg(test)]
#[path = "../../tests/hot_store/receive_accelerator_bindings.rs"]
mod receive_accelerator_bindings_tests;

#[cfg(test)]
#[path = "../../tests/hot_store/journal_rebase.rs"]
mod journal_rebase_tests;

#[cfg(feature = "accepted-match-transition-experiment")]
#[doc(hidden)]
#[path = "../../tests/hot_store/accepted_match_transition_experiment.rs"]
pub mod accepted_match_transition_experiment;

pub struct HotStoreInstances;

/// Resource ceilings used while reconstructing the live PathMap state from a
/// fully authenticated durable generation. These are startup budgets, not
/// semantic traversal-depth limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DurableHotStoreBootstrapLimits {
    pub max_steps: u64,
    pub max_endpoints: u64,
    pub max_payload_bytes: u64,
    pub max_join_occurrences: u64,
}

impl Default for DurableHotStoreBootstrapLimits {
    fn default() -> Self {
        Self {
            max_steps: u64::MAX,
            max_endpoints: u64::MAX,
            max_payload_bytes: u64::MAX,
            max_join_occurrences: u64::MAX,
        }
    }
}

#[cfg(test)]
#[derive(Debug)]
#[expect(
    dead_code,
    reason = "the variant payloads are diagnostic context read only by the derived Debug impl"
)]
pub(crate) enum DurableHotStoreBootstrapError {
    AuthenticatedRead(crate::rspace::persistence::ClosureError<DurableObjectReadErrorV1>),
    Data(DataIndexError),
    JoinMultiplicityOverflow {
        channel: String,
        attempted: u64,
        limit: u64,
    },
}

#[cfg(test)]
pub(crate) struct ClosedDurableHistoryReader;

#[cfg(test)]
impl<C: Clone, P: Clone, A: Clone, K: Clone> HistoryReaderBase<C, P, A, K>
    for ClosedDurableHistoryReader
{
    fn get_data_canonical(&self, _key: &C) -> Vec<Datum<A>> { Vec::new() }

    fn get_continuations_canonical(&self, _key: &[C]) -> Vec<WaitingContinuation<P, K>> {
        Vec::new()
    }

    fn get_joins_canonical(&self, _key: &C) -> Vec<Vec<C>> { Vec::new() }
}

impl HotStoreInstances {
    /// Reconstruct the live PathMap-backed HotStore from one authenticated,
    /// generation-pinned durable view. The complete namespace enumeration is
    /// performed before publication; any authenticated read or checked Bag
    /// failure aborts construction and cannot become an empty fallback.
    #[cfg(test)]
    pub(crate) fn create_from_durable<C, P, A, K>(
        authority: &DurableAuthorityViewV1<RSpaceSemanticProfile<C, P, A, K>>,
        limits: DurableHotStoreBootstrapLimits,
    ) -> Result<Box<dyn HotStore<C, P, A, K>>, DurableHotStoreBootstrapError>
    where
        C: Default
            + Clone
            + Debug
            + Eq
            + Hash
            + RhoPathEncode
            + ColdStoreDecode
            + Send
            + Sync
            + 'static,
        P: Default + Clone + Debug + StableHashSerialize + ColdStoreDecode + Send + Sync + 'static,
        A: Default
            + Clone
            + Debug
            + Serialize
            + RhoPathEncode
            + ColdStoreDecode
            + Send
            + Sync
            + 'static,
        K: Default + Clone + Debug + StableHashSerialize + ColdStoreDecode + Send + Sync + 'static,
    {
        let data_entries = authority
            .data_entries(limits.max_steps, limits.max_endpoints)
            .map_err(DurableHotStoreBootstrapError::AuthenticatedRead)?;
        let continuation_entries = authority
            .continuation_entries(limits.max_steps, limits.max_endpoints, limits.max_payload_bytes)
            .map_err(DurableHotStoreBootstrapError::AuthenticatedRead)?;
        let join_entries = authority
            .join_entries(limits.max_steps, limits.max_endpoints)
            .map_err(DurableHotStoreBootstrapError::AuthenticatedRead)?;

        let mut state = InMemHotStoreState {
            durable_journal: Some(Box::new(DurableJournalStateV1::new(
                authority.catalog(),
                authority.generation_leases(),
            ))),
            ..InMemHotStoreState::default()
        };
        let mut data_groups: HashMap<C, Vec<(Datum<A>, super::bag::Multiplicity)>> = HashMap::new();
        for (channel, datum, multiplicity) in data_entries {
            let multiplicity = super::bag::Multiplicity::new(multiplicity.get())
                .expect("authenticated durable multiplicity is nonzero");
            data_groups
                .entry(channel)
                .or_default()
                .push((datum, multiplicity));
        }
        for (channel, entries) in data_groups {
            state
                .data
                .set_channel_counts(channel, entries)
                .map_err(DurableHotStoreBootstrapError::Data)?;
        }

        let mut continuation_groups: HashMap<Vec<C>, Vec<WaitingContinuation<P, K>>> =
            HashMap::new();
        for (channels, continuation) in continuation_entries {
            continuation_groups
                .entry(channels)
                .or_default()
                .push(continuation);
        }
        for (channels, continuations) in continuation_groups {
            state.continuations.set_group(channels, continuations);
        }

        let mut join_groups: HashMap<C, Vec<Vec<C>>> = HashMap::new();
        for (channel, join, multiplicity) in join_entries {
            let group = join_groups.entry(channel.clone()).or_default();
            let attempted = group.len() as u64 + multiplicity.get();
            if attempted > limits.max_join_occurrences {
                return Err(DurableHotStoreBootstrapError::JoinMultiplicityOverflow {
                    channel: format!("{channel:?}"),
                    attempted,
                    limit: limits.max_join_occurrences,
                });
            }
            let count = usize::try_from(multiplicity.get()).map_err(|_| {
                DurableHotStoreBootstrapError::JoinMultiplicityOverflow {
                    channel: format!("{channel:?}"),
                    attempted,
                    limit: limits.max_join_occurrences,
                }
            })?;
            group.extend(std::iter::repeat_with(|| join.clone()).take(count));
        }
        for (channel, joins) in join_groups {
            state.joins.set_channel(channel, joins);
        }

        Ok(Box::new(InMemHotStore {
            state: std::sync::RwLock::new(state),
            history_cache: std::sync::RwLock::new(HistoryStoreCache::default()),
            history_reader_base: Box::new(ClosedDurableHistoryReader),
        }))
    }

    pub(crate) fn create_from_durable_reader<C, P, A, K>(
        reader: Box<dyn HistoryReaderBase<C, P, A, K>>,
        cache: HotStoreState<C, P, A, K>,
        base: DurableJournalBaseV1,
    ) -> Result<Box<dyn HotStore<C, P, A, K>>, DataIndexError>
    where
        C: Default
            + Clone
            + Debug
            + Eq
            + Hash
            + RhoPathEncode
            + ColdStoreDecode
            + Send
            + Sync
            + 'static,
        P: Default + Clone + Debug + StableHashSerialize + Send + Sync + 'static,
        A: Default
            + Clone
            + Debug
            + Serialize
            + RhoPathEncode
            + ColdStoreDecode
            + Send
            + Sync
            + 'static,
        K: Default + Clone + Debug + StableHashSerialize + Send + Sync + 'static,
    {
        let state = InMemHotStoreState {
            durable_journal: Some(Box::new(DurableJournalStateV1::detached(base))),
            ..InMemHotStoreState::from_snapshot(cache)?
        };
        Ok(Box::new(InMemHotStore {
            state: std::sync::RwLock::new(state),
            history_cache: std::sync::RwLock::new(HistoryStoreCache::default()),
            history_reader_base: reader,
        }))
    }

    pub fn create_from_hs_and_hr<C, P, A, K>(
        cache: HotStoreState<C, P, A, K>,
        history_reader: Box<dyn HistoryReaderBase<C, P, A, K>>,
    ) -> Box<dyn HotStore<C, P, A, K>>
    where
        C: Default
            + Clone
            + Debug
            + Eq
            + Hash
            + RhoPathEncode
            + ColdStoreDecode
            + Send
            + Sync
            + 'static,
        P: Default + Clone + Debug + StableHashSerialize + Send + Sync + 'static,
        A: Default
            + Clone
            + Debug
            + Serialize
            + RhoPathEncode
            + ColdStoreDecode
            + Send
            + Sync
            + 'static,
        K: Default + Clone + Debug + StableHashSerialize + Send + Sync + 'static,
    {
        Box::new(InMemHotStore {
            state: std::sync::RwLock::new(
                InMemHotStoreState::from_snapshot(cache)
                    .expect("snapshot data must fit the PathMap multiplicity model"),
            ),
            history_cache: std::sync::RwLock::new(HistoryStoreCache::default()),
            history_reader_base: history_reader,
        })
    }

    pub fn create_from_hr<C, P, A, K>(
        history_reader: Box<dyn HistoryReaderBase<C, P, A, K>>,
    ) -> Box<dyn HotStore<C, P, A, K>>
    where
        C: Default
            + Clone
            + Debug
            + Eq
            + Hash
            + RhoPathEncode
            + ColdStoreDecode
            + 'static
            + Send
            + Sync,
        P: Default + Clone + Debug + StableHashSerialize + 'static + Send + Sync,
        A: Default
            + Clone
            + Debug
            + Serialize
            + RhoPathEncode
            + ColdStoreDecode
            + 'static
            + Send
            + Sync,
        K: Default + Clone + Debug + StableHashSerialize + 'static + Send + Sync,
    {
        HotStoreInstances::create_from_hs_and_hr(HotStoreState::default(), history_reader)
    }
}
