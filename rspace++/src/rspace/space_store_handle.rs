// Reified RSpaces storage substrate: `SpaceTxnHost`, `SpaceStoreHandle` and
// `SpaceTxn`.
//
// A space agent opens a `SpaceTxn`, reads a staged view of the hot store, and
// stages whole host-equivalent operations (a produce or a consume with an
// explicit outcome). `commit` applies them with the observer calls, events
// and store writes of the equivalent host `ISpace` program. `ISpace` is
// unchanged: spaces are not `ISpace` implementors.
//
// Identity and order. Every view map, set and comparison uses the store
// identity of a channel, `stable_hash_provider::hash(channel)`, and of a
// group, its channel keys in group order (FR-2). A group with two or more
// channels must be non-decreasing by channel key, so it may repeat a
// channel (FR-1'). The sorted list is the only canonical order of a channel
// multiset, so one multiset has one hot-store key. The check runs before any
// hot-store access, so an aborted transaction leaves no entry under a
// permuted key.
//
// Commit checks (FR-7'). Each operation checks only the entries it changes,
// taken from its staged effects, in every build profile. Before any effect,
// every record and continuation that it removes must sit where the staged
// view held it. After its effects, every staged change must have its event.
//
// Module layout:
//   space_store_handle.rs          — public API, staged view, staging rules,
//                                    commit loop, event correlation, sweep
//   rspace/space_play.rs           — `SpaceTxnHost` for `RSpace` (play)
//   replay_rspace/space_replay.rs  — `SpaceTxnHost` for `ReplayRSpace` and
//                                    `ReportingRspace` (replay, reporting)

use std::any::Any;
use std::borrow::{Borrow, Cow};
use std::cmp::{Ordering, Reverse};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::{self, Debug};
use std::hash::Hash;
use std::mem::size_of;
use std::sync::{Arc, Mutex, Weak};
use std::thread::ThreadId;

use async_trait::async_trait;
use serde::Serialize;
use shared::rust::clone_backing::CloneBacking;

use crate::rspace::candidate_order::{self, CandidateSource, OrderWork};
use crate::rspace::errors::RSpaceError;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::hashing::native_source::{self, SourceMeter};
use crate::rspace::hashing::stable_hash_provider::hash_from_hashes;
use crate::rspace::history::history_reader::HistoryReader;
use crate::rspace::hot_store::HotStore;
use crate::rspace::internal::{ConsumeCandidate, Datum, Install, WaitingContinuation};
use crate::rspace::r#match::Match;
use crate::rspace::operation_context::{self, CausalPath, OperationOrder};
use crate::rspace::rspace_interface::{
    ContResult, MaybeConsumeResult, RSpaceOperationCompletion, RSpaceOperationSource, RSpaceResult,
};
use crate::rspace::striped_locks::{self, ChannelLockGuard};
use crate::rspace::trace::event::{COMM, Consume, Event, IOEvent, Produce};

/// Execution mode of a [`SpaceTxnHost`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpaceMode {
    Play,
    Replay,
    Reporting,
}

/// Typed failure of a space transaction. Host and observer errors pass
/// through unchanged in [`SpaceTxnError::Host`].
#[derive(Clone, Debug, PartialEq)]
pub enum SpaceTxnError {
    /// Observer or host error, unchanged (`OutOfPhlogistons`,
    /// `HostWorkRejected`, ...).
    Host(RSpaceError),
    /// Empty channels, arity mismatch, empty lock keys, or a bad incoming
    /// candidate.
    InvalidOperation(&'static str),
    /// Peeks that are neither empty nor `{0..binds-1}`.
    MixedArrowJoin { binds: usize, peeks: BTreeSet<i32> },
    /// A candidate or continuation is absent from the staged view.
    NotInView(&'static str),
    /// The channel carries an installed (system-process) continuation or join.
    InstalledChannel,
    /// The hot store was replaced between `begin` and `commit`.
    StoreSwapped,
    /// An unpaired replay fire: the recorded bindings of its trigger event
    /// hold no free occurrence of its COMM. Commit raises it after the
    /// observer accepts every call of the fire, and the operations before
    /// the fire stay applied (decision 36).
    ReplayDivergence(Box<COMM>),
    /// The observer returned a native replay directive.
    NativeReplayDirectiveUnsupported,
    /// A commit-time recomputation differs from staging (a substrate defect).
    Inconsistent(&'static str),
    /// A store change of operation `op` lacks its event.
    EventCoverage { op: usize, rule: &'static str },
    /// A join group mixes swept and kept channels.
    SweepCrossesDurable,
    /// A swept channel or group is present at the history root.
    SweepTouchesPersisted,
    /// The transaction was used after `commit`.
    Finished,
    /// A channel group is not sorted (non-decreasing) by channel hash.
    NonCanonicalChannelOrder,
    /// The transaction does not hold the lock stripe of a channel that the
    /// staged operation touches, in the phase the host uses for it.
    UnlockedChannel,
    /// A transaction already committed under the current parent operation
    /// order.
    ParentOrderReused,
    /// A produce counter would exceed `i32::MAX`.
    CounterOverflow,
    /// The current task already holds an open transaction of this handle.
    NestedTransaction,
}

impl fmt::Display for SpaceTxnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpaceTxnError::Host(error) => write!(f, "{}", error),
            SpaceTxnError::InvalidOperation(reason) => {
                write!(f, "Invalid space operation: {}", reason)
            }
            SpaceTxnError::MixedArrowJoin { binds, peeks } => {
                write!(f, "Mixed arrow join: {} binds with peeks {:?}", binds, peeks)
            }
            SpaceTxnError::NotInView(what) => write!(f, "Not in the staged view: {}", what),
            SpaceTxnError::InstalledChannel => {
                write!(f, "The channel carries an installed continuation or join")
            }
            SpaceTxnError::StoreSwapped => {
                write!(f, "The hot store was replaced while the space transaction was open")
            }
            SpaceTxnError::ReplayDivergence(comm) => {
                write!(f, "Replay divergence: COMM {:?} is not among the recorded bindings", comm)
            }
            SpaceTxnError::NativeReplayDirectiveUnsupported => {
                write!(f, "A native replay directive is not supported for a space operation")
            }
            SpaceTxnError::Inconsistent(what) => {
                write!(f, "Space transaction inconsistency: {}", what)
            }
            SpaceTxnError::EventCoverage { op, rule } => {
                write!(f, "Event correlation failed for space operation {}: {}", op, rule)
            }
            SpaceTxnError::SweepCrossesDurable => {
                write!(f, "The volatile sweep crosses a durable join group")
            }
            SpaceTxnError::SweepTouchesPersisted => {
                write!(f, "The volatile sweep touches state persisted at the history root")
            }
            SpaceTxnError::Finished => write!(f, "The space transaction is finished"),
            SpaceTxnError::NonCanonicalChannelOrder => {
                write!(f, "A channel group is not sorted by channel hash")
            }
            SpaceTxnError::UnlockedChannel => {
                write!(f, "The space transaction does not hold the lock of a touched channel")
            }
            SpaceTxnError::ParentOrderReused => {
                write!(f, "A space transaction already committed under this operation order")
            }
            SpaceTxnError::CounterOverflow => write!(f, "A produce counter overflows"),
            SpaceTxnError::NestedTransaction => {
                write!(f, "The current task already holds an open space transaction")
            }
        }
    }
}

impl From<RSpaceError> for SpaceTxnError {
    fn from(e: RSpaceError) -> Self { Self::Host(e) }
}

impl From<SpaceTxnError> for RSpaceError {
    fn from(e: SpaceTxnError) -> Self {
        match e {
            SpaceTxnError::Host(e) => e,
            o => RSpaceError::InterpreterError(o.to_string()),
        }
    }
}

/// Runs at commit, immediately before this op's `observe_operation_start`:
/// the host position of `register_introduction_authority` in the reducer's
/// `produce_inner` / `consume_inner`.
pub type BeforeObserve =
    Box<dyn for<'s> FnOnce(RSpaceOperationSource<'s>) -> Result<(), RSpaceError> + Send>;

/// Outcome of a space produce, decided by the space agent.
#[derive(Clone, Debug)]
pub enum ProduceOutcome<C, P: Clone, A: Clone, K: Clone> {
    /// Store the datum (host produce without a match).
    Store,
    /// Fire `continuation` on `channels`. `data[i]` binds `channels[i]`, and
    /// exactly one candidate is the incoming datum (`datum_index == -1`).
    /// The fire never stores the incoming datum. When `continuation` peeks
    /// and is not persistent, the agent offers a non-persistent incoming
    /// datum again as the next produce of the transaction, with the same
    /// identity: HEAD's re-produce. A persistent `continuation` stays in
    /// place, so the agent re-issues the datum in a new transaction instead.
    Fire {
        channels: Vec<C>,
        continuation: WaitingContinuation<P, K>,
        data: Vec<ConsumeCandidate<C, A>>,
    },
}

/// Outcome of a space consume, decided by the space agent.
#[derive(Clone, Debug)]
pub enum ConsumeOutcome<C, A: Clone> {
    /// Park the continuation (host consume without a match).
    Park,
    /// Fire with stored records; `data[i]` binds `channels[i]`.
    Fire { data: Vec<ConsumeCandidate<C, A>> },
}

/// A host-equivalent produce with its outcome.
pub struct SpaceProduce<C, P: Clone, A: Clone, K: Clone> {
    pub channel: C,
    pub data: A,
    pub persist: bool,
    pub outcome: ProduceOutcome<C, P, A, K>,
    pub before_observe: Option<BeforeObserve>,
}

/// A host-equivalent consume with its outcome. `channels` is a canonical
/// group: non-decreasing by channel hash. A repeated channel binds one record
/// per occurrence.
pub struct SpaceConsume<C, P: Clone, A: Clone, K: Clone> {
    pub channels: Vec<C>,
    pub patterns: Vec<P>,
    pub continuation: K,
    pub persist: bool,
    pub peeks: BTreeSet<i32>,
    pub outcome: ConsumeOutcome<C, A>,
    pub before_observe: Option<BeforeObserve>,
}

/// Identity of a staged operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpSource {
    Produce(Produce),
    Consume(Consume),
}

/// Result of staging: the operation's position, identity, and staged COMM.
#[derive(Clone, Debug)]
pub struct StagedOp {
    pub index: usize,
    pub source: OpSource,
    pub comm: Option<COMM>,
}

/// Result of one committed operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpaceOpReceipt<C, P, A, K> {
    Stored {
        produce: Produce,
    },
    Parked {
        consume: Consume,
    },
    /// `produce` is the produce side of a produce-triggered fire: its own
    /// source in play, and the copy recorded in the paired COMM in replay.
    Fired {
        comm: COMM,
        continuation: ContResult<C, P, K>,
        results: Vec<RSpaceResult<C, A>>,
        produce: Option<Produce>,
    },
}

/// Results of a committed transaction, in staging order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpaceTxnReceipt<C, P, A, K> {
    pub ops: Vec<SpaceOpReceipt<C, P, A, K>>,
}

/// Counts removed by [`SpaceTxnHost::sweep_volatile`]: distinct channels,
/// data records, waiting continuations, and join entries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SweepReport {
    pub channels: usize,
    pub data: usize,
    pub continuations: usize,
    pub joins: usize,
}

/// The bincode bytes of `value`, encoded by the metered writer of the host
/// (`native_source::encode`): each write and each growth of the buffer is
/// reserved on `meter` before it happens. Space agents meter their encodes
/// with the rule of the host (decision 42).
pub fn metered_encode<T: Serialize + ?Sized>(
    value: &T,
    meter: &impl SourceMeter,
) -> Result<Vec<u8>, RSpaceError> {
    native_source::encode(value, meter)
}

/// The blake2b-256 hash of the bincode bytes of `value`, hashed by the
/// metered writer of the host (`native_source::hash`): each write is reserved
/// on `meter` before it happens. A set agent hashes its class keys with it
/// (P5a, decision 41).
pub fn metered_hash<T: Serialize + ?Sized>(
    value: &T,
    meter: &impl SourceMeter,
) -> Result<Blake2b256Hash, RSpaceError> {
    native_source::hash(value, meter)
}

/// The canonical order of the stored records `data` of one collection
/// (DR-75, decision 41): by the hash of the source produce of each record,
/// then by its full digest inside a run of equal source hashes, then by its
/// index. It returns the indices of `data` in that order, which is the order
/// of `candidate_order::canonical_candidates`. The work is reserved on
/// `meter` as metered native replay reserves it for the same order
/// (`MeteredOrder` of `native_candidate/metered.rs`, restated in
/// [`SpaceOrder`]), so a bag or a set agent charges for its order what the
/// branch charges. The records are ordered as borrowed views, and the agent
/// copies only the records that it binds.
pub fn canonical_data_order<A>(
    data: &[Datum<A>],
    meter: &dyn SourceMeter,
) -> Result<Vec<usize>, RSpaceError>
where
    A: Clone + Serialize + CloneBacking,
{
    canonical_view_order(data, meter)
}

/// [`canonical_data_order`] for the waiting continuations of one collection:
/// by the hash of the source consume of each continuation, then by its
/// digest, then by its index.
pub fn canonical_continuation_order<P, K>(
    waiters: &[WaitingContinuation<P, K>],
    meter: &dyn SourceMeter,
) -> Result<Vec<usize>, RSpaceError>
where
    P: Clone + Serialize + CloneBacking,
    K: Clone + Serialize + CloneBacking,
{
    canonical_view_order(waiters, meter)
}

fn canonical_view_order<T>(
    entries: &[T],
    meter: &dyn SourceMeter,
) -> Result<Vec<usize>, RSpaceError>
where
    T: CandidateSource + Clone + Serialize + CloneBacking,
{
    let mut views = space_order_buffer(entries.len(), meter)?;
    views.extend(entries.iter().map(Cow::Borrowed));
    let ordered = candidate_order::canonical_order(views, &SpaceOrder(meter))?;
    let mut indices = space_order_buffer(ordered.len(), meter)?;
    for (_, index) in ordered {
        indices.push(usize::try_from(index).map_err(|_| RSpaceError::HostWorkRejected)?);
    }
    Ok(indices)
}

/// The order work of a space agent over borrowed views. It restates the
/// reservations of `MeteredOrder` and of its helpers `buffer` and `sort` in
/// `native_candidate/metered.rs`, a file of `feature/cost-accounted-rho` that
/// the campaign does not edit (M-3).
struct SpaceOrder<'m>(&'m dyn SourceMeter);

impl<'v, T> OrderWork<Cow<'v, T>> for SpaceOrder<'_>
where T: Clone + Serialize + CloneBacking
{
    type Error = RSpaceError;

    fn index_bound(&self, length: usize) -> Result<i32, RSpaceError> {
        i32::try_from(length).map_err(|_| RSpaceError::HostWorkRejected)
    }

    fn buffer<U>(&self, length: usize) -> Result<Vec<U>, RSpaceError> {
        space_order_buffer(length, self.0)
    }

    fn sort<U>(
        &self,
        values: &mut [U],
        compare: impl Fn(&U, &U) -> Ordering,
        scanned: impl Fn(&U) -> usize,
    ) -> Result<(), RSpaceError> {
        let moves = size_of::<U>()
            .checked_mul(2)
            .ok_or(RSpaceError::HostWorkRejected)?;
        self.0.reserve(
            values.len(),
            values
                .len()
                .checked_mul(moves)
                .ok_or(RSpaceError::HostWorkRejected)?,
            0,
        )?;
        shared::rust::fallible_sort::sort(values, |a, b| {
            self.0.reserve(
                1,
                moves
                    .checked_add(scanned(a))
                    .and_then(|bytes| bytes.checked_add(scanned(b)))
                    .ok_or(RSpaceError::HostWorkRejected)?,
                0,
            )?;
            Ok(compare(a, b))
        })
    }

    fn scan(&self, left: &Blake2b256Hash, right: &Blake2b256Hash) -> Result<(), RSpaceError> {
        self.0.reserve(
            1,
            left.0
                .len()
                .checked_add(right.0.len())
                .ok_or(RSpaceError::HostWorkRejected)?,
            0,
        )
    }

    fn digest(&self, value: &Cow<'v, T>) -> Result<Blake2b256Hash, RSpaceError> {
        crate::rspace::native_backing::inspect(value, self.0)?;
        native_source::hash(value, &|operations, scanned, backing| {
            self.0.reserve(operations, scanned, backing)
        })
    }
}

/// An empty buffer for `length` values, reserved as the `buffer` helper of
/// `native_candidate/metered.rs` reserves it.
fn space_order_buffer<U>(length: usize, meter: &dyn SourceMeter) -> Result<Vec<U>, RSpaceError> {
    let bytes = length
        .checked_mul(size_of::<U>())
        .ok_or(RSpaceError::HostWorkRejected)?;
    meter.reserve(length.checked_add(1).ok_or(RSpaceError::HostWorkRejected)?, bytes, bytes)?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(length)
        .map_err(|_| RSpaceError::HostWorkRejected)?;
    Ok(values)
}

/// Candidate that binds the stored record `stored` (at `view_index` in the
/// staged view) with the matcher result `matched`.
pub fn bind_candidate<C, A: Clone>(
    channel: C,
    stored: &Datum<A>,
    matched: A,
    view_index: i32,
) -> ConsumeCandidate<C, A> {
    ConsumeCandidate {
        channel,
        datum: Datum {
            a: matched,
            persist: stored.persist,
            source: stored.source.clone(),
        },
        removed_datum: stored.a.clone(),
        datum_index: view_index,
    }
}

/// Candidate that binds the incoming datum of a produce-triggered fire.
pub fn incoming_candidate<C, A: Clone>(
    channel: C,
    source: Produce,
    data: A,
    persist: bool,
    matched: A,
) -> ConsumeCandidate<C, A> {
    ConsumeCandidate {
        channel,
        datum: Datum {
            a: matched,
            persist,
            source,
        },
        removed_datum: data,
        datum_index: -1,
    }
}

/// Path tag of a sub-order. `ReductionContext` uses 0 (operation) and 1
/// (split).
pub const SUB_OPERATION_TAG: u64 = 2;

/// Order of operation `index` of a transaction committed under `parent`:
/// `parent.path ++ [(index, SUB_OPERATION_TAG)]`.
pub fn sub_order(parent: &OperationOrder, index: usize) -> OperationOrder {
    let mut path = parent.path.clone();
    path.push_back((index as u64, SUB_OPERATION_TAG));
    OperationOrder {
        session: parent.session,
        path,
    }
}

/// An open transaction as returned by [`SpaceTxnHost::begin`].
pub type BoxedSpaceTxn<C, P, A, K> = Box<dyn SpaceTxn<C, P, A, K> + Send>;

/// Implemented for RSpace (space_play.rs), ReplayRSpace and ReportingRspace
/// (space_replay.rs). The runtime constructors are bounded by this trait (P4).
/// ISpace is unchanged.
#[async_trait]
pub trait SpaceTxnHost<C, P, A, K>: Send + Sync
where
    C: Clone + Send + Sync + 'static,
    P: Clone + Send + Sync + 'static,
    A: Clone + Send + Sync + 'static,
    K: Clone + Send + Sync + 'static,
{
    /// Play, replay or reporting.
    fn mode(&self) -> SpaceMode;

    /// The host's own injected matcher.
    fn matcher(&self) -> Arc<Box<dyn Match<P, A, K>>>;

    /// Takes the striped consume locks of `lock_keys` (both phases, the host
    /// order) and opens a transaction over the current hot store. Only
    /// [`SpaceStoreHandle::begin`] can supply `registration`.
    async fn begin(
        &self,
        lock_keys: &[C],
        registration: TxnRegistration,
    ) -> Result<BoxedSpaceTxn<C, P, A, K>, SpaceTxnError>;

    /// Number of data records in `channel`'s leaf at the history root (outer
    /// frame).
    fn root_leaf_record_count(&self, channel: &C) -> Result<u64, SpaceTxnError>;

    /// Removes the data, continuations and join entries of `channels`. It
    /// emits no events and makes no observer calls. Only
    /// [`SpaceStoreHandle::sweep_volatile`] can supply `registration`.
    async fn sweep_volatile(
        &self,
        channels: &[C],
        registration: &TxnRegistration,
    ) -> Result<SweepReport, SpaceTxnError>;
}

// The execution context that holds an open transaction: the tokio task, or
// the thread for code that `block_on` drives (it has no task id and cannot
// move to another thread).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum TxnContext {
    Task(tokio::task::Id),
    Thread(ThreadId),
}

impl TxnContext {
    fn current() -> Self {
        match tokio::task::try_id() {
            Some(id) => TxnContext::Task(id),
            None => TxnContext::Thread(std::thread::current().id()),
        }
    }
}

// The hot store that a commit ran against, without its type and without
// keeping it alive. The weak reference keeps the allocation, so a later store
// cannot reuse its address while the tracker records it.
type StoreIdentity = Weak<dyn Any + Send + Sync>;

// Parent orders committed in the current root evaluation (FR-5'). The set
// resets when `SpaceStoreHandle::begin_evaluation_scope` starts a root
// evaluation, when a commit runs against another hot store than the recorded
// one (a reset or a revert replaced it), and when a commit arrives with
// another session. So it holds at most the parents of one evaluation.
#[derive(Default)]
struct CommittedParents {
    session: Option<[u8; 32]>,
    store: Option<StoreIdentity>,
    paths: BTreeSet<CausalPath>,
}

impl CommittedParents {
    fn reset(&mut self) {
        self.session = None;
        self.store = None;
        self.paths.clear();
    }

    fn admit(
        &mut self,
        parent: &OperationOrder,
        store: StoreIdentity,
    ) -> Result<(), SpaceTxnError> {
        let same_store = self
            .store
            .as_ref()
            .is_some_and(|recorded| Weak::ptr_eq(recorded, &store));
        if !same_store || self.session != Some(parent.session) {
            self.session = Some(parent.session);
            self.store = Some(store);
            self.paths.clear();
        }
        match self.paths.insert(parent.path.clone()) {
            true => Ok(()),
            false => Err(SpaceTxnError::ParentOrderReused),
        }
    }
}

// State that a handle shares with its clones: the contexts with an open
// transaction or a running sweep (FR-9, FR-9') and the committed parent
// orders (FR-5'). A handle owns it, so no host struct changes.
#[derive(Default)]
struct HandleState {
    open: Mutex<HashSet<TxnContext>>,
    parents: Mutex<CommittedParents>,
}

/// Registration of an open transaction or a running sweep with its
/// [`SpaceStoreHandle`]. Only the handle creates one, so every transaction
/// and every sweep goes through a handle: the handle refuses a nested `begin`
/// or sweep in one task and admits one commit per parent operation order.
/// The registration ends when the transaction commits, fails closed, or is
/// dropped, and when the sweep returns.
pub struct TxnRegistration {
    state: Arc<HandleState>,
    context: TxnContext,
}

impl TxnRegistration {
    fn register(state: &Arc<HandleState>) -> Result<Self, SpaceTxnError> {
        let context = TxnContext::current();
        let inserted = state
            .open
            .lock()
            .expect("open transactions lock")
            .insert(context);
        match inserted {
            true => Ok(Self {
                state: Arc::clone(state),
                context,
            }),
            false => Err(SpaceTxnError::NestedTransaction),
        }
    }

    fn admit_parent<C, P, A, K>(
        &self,
        parent: &OperationOrder,
        store: &SharedStore<C, P, A, K>,
    ) -> Result<(), SpaceTxnError>
    where
        C: Clone + Hash + Eq + 'static,
        P: Clone + 'static,
        A: Clone + 'static,
        K: Clone + 'static,
    {
        let weak = Arc::downgrade(store);
        let store: StoreIdentity = weak;
        self.state
            .parents
            .lock()
            .expect("committed parents lock")
            .admit(parent, store)
    }
}

impl Drop for TxnRegistration {
    fn drop(&mut self) {
        self.state
            .open
            .lock()
            .expect("open transactions lock")
            .remove(&self.context);
    }
}

/// Type-erased, cheaply clonable handle the runtime stores. Captured from the
/// concrete host before the DeterministicRSpace wrap; host clones share all
/// state (every host field is an Arc). Clones of one handle share the
/// nested-transaction set and the committed parent orders, so a host has one
/// handle.
pub struct SpaceStoreHandle<C, P, A, K>
where
    C: Clone + Send + Sync + 'static,
    P: Clone + Send + Sync + 'static,
    A: Clone + Send + Sync + 'static,
    K: Clone + Send + Sync + 'static,
{
    host: Arc<dyn SpaceTxnHost<C, P, A, K>>,
    state: Arc<HandleState>,
}

impl<C, P, A, K> Clone for SpaceStoreHandle<C, P, A, K>
where
    C: Clone + Send + Sync + 'static,
    P: Clone + Send + Sync + 'static,
    A: Clone + Send + Sync + 'static,
    K: Clone + Send + Sync + 'static,
{
    fn clone(&self) -> Self {
        Self {
            host: Arc::clone(&self.host),
            state: Arc::clone(&self.state),
        }
    }
}

impl<C, P, A, K> SpaceStoreHandle<C, P, A, K>
where
    C: Clone + Send + Sync + 'static,
    P: Clone + Send + Sync + 'static,
    A: Clone + Send + Sync + 'static,
    K: Clone + Send + Sync + 'static,
{
    /// Wraps a concrete host (`RSpace`, `ReplayRSpace` or `ReportingRspace`).
    pub fn new<H: SpaceTxnHost<C, P, A, K> + 'static>(host: H) -> Self {
        Self {
            host: Arc::new(host),
            state: Arc::new(HandleState::default()),
        }
    }

    /// See [`SpaceTxnHost::mode`].
    pub fn mode(&self) -> SpaceMode { self.host.mode() }

    /// See [`SpaceTxnHost::matcher`].
    pub fn matcher(&self) -> Arc<Box<dyn Match<P, A, K>>> { self.host.matcher() }

    /// See [`SpaceTxnHost::begin`]. Fails with `NestedTransaction`, before
    /// it takes any lock, when the current task already holds an open
    /// transaction of this handle.
    pub async fn begin(&self, lock_keys: &[C]) -> Result<BoxedSpaceTxn<C, P, A, K>, SpaceTxnError> {
        let registration = TxnRegistration::register(&self.state)?;
        self.host.begin(lock_keys, registration).await
    }

    /// Starts a root evaluation: the parent orders committed before it no
    /// longer count, so a deploy that is evaluated again (after a revert, or
    /// in a later block) commits under the same parent orders. Call it at the
    /// entry of every root evaluation, in play and in replay.
    pub fn begin_evaluation_scope(&self) {
        self.state
            .parents
            .lock()
            .expect("committed parents lock")
            .reset();
    }

    /// See [`SpaceTxnHost::root_leaf_record_count`].
    pub fn root_leaf_record_count(&self, channel: &C) -> Result<u64, SpaceTxnError> {
        self.host.root_leaf_record_count(channel)
    }

    /// See [`SpaceTxnHost::sweep_volatile`]. Fails with `NestedTransaction`,
    /// before it takes any lock, when the current task holds an open
    /// transaction of this handle.
    pub async fn sweep_volatile(&self, channels: &[C]) -> Result<SweepReport, SpaceTxnError> {
        let registration = TxnRegistration::register(&self.state)?;
        self.host.sweep_volatile(channels, &registration).await
    }
}

/// An open transaction. Reads see the hot store plus the effects staged so
/// far; the first touch of a key reads through history like a host read.
/// Every group (the channels of a continuation key or of a join) must be
/// sorted (non-decreasing) by channel hash.
#[async_trait]
pub trait SpaceTxn<C, P, A, K>: Send
where
    C: Clone + Send + Sync,
    P: Clone + Send + Sync,
    A: Clone + Send + Sync,
    K: Clone + Send + Sync,
{
    /// The mode of the host that opened the transaction.
    fn mode(&self) -> SpaceMode;

    /// Data at `channel` in the staged view.
    fn data(&mut self, channel: &C) -> Result<&[Datum<A>], SpaceTxnError>;

    /// Waiting continuations keyed by the canonical group `channels` in the
    /// staged view.
    fn continuations(
        &mut self,
        channels: &[C],
    ) -> Result<&[WaitingContinuation<P, K>], SpaceTxnError>;

    /// Join groups of `channel` in the staged view; each must be canonical.
    fn joins(&mut self, channel: &C) -> Result<&[Vec<C>], SpaceTxnError>;

    /// Host produce counters plus the increments staged so far.
    fn produce_counters(
        &mut self,
        produces: &[Produce],
    ) -> Result<BTreeMap<Produce, i32>, SpaceTxnError>;

    /// Recorded COMMs bound to `source` (its own trigger key) and not yet
    /// paired in this transaction, one entry per recorded occurrence, sorted
    /// by bincode bytes. `None` in play.
    fn recorded_comms(&mut self, source: RSpaceOperationSource<'_>) -> Option<Vec<COMM>>;

    /// Replay admission of `datum` for `comm` over the staged counters;
    /// always `true` in play. `incoming` is `true` when `datum.source` is the
    /// source of the produce being matched.
    fn admits(
        &mut self,
        comm: &COMM,
        datum: &Datum<A>,
        incoming: bool,
    ) -> Result<bool, SpaceTxnError>;

    /// Validates and stages a produce. Nothing touches the host before commit.
    fn produce(&mut self, op: SpaceProduce<C, P, A, K>) -> Result<StagedOp, SpaceTxnError>;

    /// Validates and stages a consume. Nothing touches the host before commit.
    fn consume(&mut self, op: SpaceConsume<C, P, A, K>) -> Result<StagedOp, SpaceTxnError>;

    /// The single mutation point. Releases the locks before returning.
    async fn commit(&mut self) -> Result<SpaceTxnReceipt<C, P, A, K>, SpaceTxnError>;
}

pub(crate) type SharedStore<C, P, A, K> = Arc<Box<dyn HotStore<C, P, A, K>>>;

pub(crate) fn record_bytes<T: Serialize + ?Sized>(record: &T) -> Result<Vec<u8>, SpaceTxnError> {
    bincode::serialize(record)
        .map_err(|_| SpaceTxnError::InvalidOperation("a record is not serializable"))
}

/// The store identity of a channel: `stable_hash_provider::hash(channel)`,
/// the Blake2b-256 hash of its bincode bytes, with a typed error where that
/// function panics.
pub(crate) fn channel_key<C: Serialize>(channel: &C) -> Result<Blake2b256Hash, SpaceTxnError> {
    record_bytes(channel).map(|bytes| Blake2b256Hash::new(&bytes))
}

/// The store identity of a group: its channel keys in group order.
pub(crate) fn group_identity<C: Serialize>(
    group: &[C],
) -> Result<Vec<Blake2b256Hash>, SpaceTxnError> {
    let mut keys = Vec::with_capacity(group.len());
    for channel in group {
        keys.push(channel_key(channel)?);
    }
    Ok(keys)
}

/// A group is canonical when its keys are sorted (non-decreasing), so a group
/// may repeat a channel. The sorted list is the only canonical member of the
/// permutations of a key multiset. Any other order fails with
/// `NonCanonicalChannelOrder`. One channel is always canonical.
pub(crate) fn require_canonical(keys: &[Blake2b256Hash]) -> Result<(), SpaceTxnError> {
    match keys.windows(2).all(|pair| pair[0] <= pair[1]) {
        true => Ok(()),
        false => Err(SpaceTxnError::NonCanonicalChannelOrder),
    }
}

/// The key of a non-empty canonical group.
pub(crate) fn canonical_group_key<C: Serialize>(
    group: &[C],
) -> Result<Vec<Blake2b256Hash>, SpaceTxnError> {
    if group.is_empty() {
        return Err(SpaceTxnError::InvalidOperation("channels must not be empty"));
    }
    let keys = group_identity(group)?;
    require_canonical(&keys)?;
    Ok(keys)
}

// The stripes that one lock phase holds, indexed as
// `striped_locks::acquire_locks` indexes them: `channel hash % stripes`.
#[derive(Clone, Debug)]
struct HeldStripes {
    stripes: usize,
    held: BTreeSet<usize>,
}

impl HeldStripes {
    fn new(stripes: usize, lock_hashes: &[u64]) -> Self {
        let held = match stripes {
            0 => BTreeSet::new(),
            stripes => lock_hashes
                .iter()
                .map(|hash| (*hash as usize) % stripes)
                .collect(),
        };
        Self { stripes, held }
    }

    fn holds(&self, channel_hash: u64) -> bool {
        match self.stripes {
            0 => false,
            stripes => self.held.contains(&((channel_hash as usize) % stripes)),
        }
    }
}

// How the host locks a channel that an operation touches: a produce locks
// its own channel in phase A and every channel of its join groups in phase
// B; a consume locks its channels in both phases.
#[derive(Clone, Copy, Debug)]
enum LockUse {
    Produced,
    Joined,
    Consumed,
}

/// The exact stripes that `begin` holds. `begin` takes both phases for every
/// lock key (`striped_locks::consume_lock`), so a channel that an operation
/// touches is covered when its stripe is held in the phase that host
/// produce and consume use for it. No host operation can then interleave
/// with the commit on a touched channel.
#[derive(Clone, Debug)]
pub(crate) struct LockCoverage {
    phase_a: HeldStripes,
    phase_b: HeldStripes,
}

impl LockCoverage {
    pub(crate) fn new(lock_hashes: &[u64], phase_a_stripes: usize, phase_b_stripes: usize) -> Self {
        Self {
            phase_a: HeldStripes::new(phase_a_stripes, lock_hashes),
            phase_b: HeldStripes::new(phase_b_stripes, lock_hashes),
        }
    }

    fn require<C: Hash>(&self, channel: &C, lock_use: LockUse) -> Result<(), SpaceTxnError> {
        let hash = striped_locks::channel_hash(channel);
        let held = match lock_use {
            LockUse::Produced => self.phase_a.holds(hash),
            LockUse::Joined => self.phase_b.holds(hash),
            LockUse::Consumed => self.phase_a.holds(hash) && self.phase_b.holds(hash),
        };
        match held {
            true => Ok(()),
            false => Err(SpaceTxnError::UnlockedChannel),
        }
    }
}

// The hot store's private continuation dedup identity (hot_store.rs), copied
// word for word so the staged view deduplicates exactly as `put_continuation`.
pub(crate) fn continuation_identity<P: Clone + Debug, K: Clone + Debug>(
    wc: &WaitingContinuation<P, K>,
) -> String {
    format!("{:?}|{:?}|{}|{:?}", wc.patterns, wc.continuation, wc.persist, wc.peeks)
}

// First record byte-equal to `record`, skipping claimed positions. Records
// with different sources cannot be byte-equal, so the source filters first.
pub(crate) fn first_equal_datum<A: Clone + Serialize>(
    records: &[Datum<A>],
    record: &Datum<A>,
    claimed: &BTreeSet<usize>,
) -> Result<Option<usize>, SpaceTxnError> {
    let target = record_bytes(record)?;
    for (position, candidate) in records.iter().enumerate() {
        if claimed.contains(&position) || candidate.source != record.source {
            continue;
        }
        if record_bytes(candidate)? == target {
            return Ok(Some(position));
        }
    }
    Ok(None)
}

pub(crate) fn first_equal_continuation<P, K, W>(
    waiting: &[W],
    continuation: &WaitingContinuation<P, K>,
) -> Result<Option<usize>, SpaceTxnError>
where
    P: Clone + Serialize,
    K: Clone + Serialize,
    W: Borrow<WaitingContinuation<P, K>>,
{
    let target = record_bytes(continuation)?;
    for (position, candidate) in waiting.iter().enumerate() {
        let candidate = candidate.borrow();
        if candidate.source != continuation.source {
            continue;
        }
        if record_bytes(candidate)? == target {
            return Ok(Some(position));
        }
    }
    Ok(None)
}

// Disabled by decision 45 (Rule E, "Apply everywhere"): the substrate keeps no
// record for a peek. A fire removes every stored non-persistent candidate, as
// the host does, and the space agent stores a peeked record again.
// `check_peeks` keeps the rule on the peek set.
//
// ```
// /// Peeks rule of an operation with `binds` channels: `Ok(false)` for no
// /// peeks, `Ok(true)` for `{0..binds-1}` (retention), `MixedArrowJoin`
// /// otherwise.
// pub(crate) fn peek_retention(
//     binds: usize,
//     peeks: &BTreeSet<i32>,
// ) -> Result<bool, SpaceTxnError> {
//     let all_binds =
//         || i32::try_from(binds).is_ok_and(|binds| peeks.iter().copied().eq(0..binds));
//     match peeks.is_empty() {
//         true => Ok(false),
//         false if all_binds() => Ok(true),
//         false => Err(SpaceTxnError::MixedArrowJoin {
//             binds,
//             peeks: peeks.clone(),
//         }),
//     }
// }
// ```

/// Peeks rule of an operation with `binds` channels: no peeks, or
/// `{0..binds-1}` (every bind peeks). Any other set is `MixedArrowJoin`. A peek
/// fire removes the same records as any other fire, because the host COMM
/// removes every stored non-persistent candidate whatever its peeks (decision
/// 45, Rule E).
pub(crate) fn check_peeks(binds: usize, peeks: &BTreeSet<i32>) -> Result<(), SpaceTxnError> {
    let all_binds = || i32::try_from(binds).is_ok_and(|binds| peeks.iter().copied().eq(0..binds));
    match peeks.is_empty() {
        true => Ok(()),
        false if all_binds() => Ok(()),
        false => Err(SpaceTxnError::MixedArrowJoin {
            binds,
            peeks: peeks.clone(),
        }),
    }
}

/// Bindings that one recorded occurrence of `comm` adds under `key` in
/// `rig` (replay_rspace.rs), and that `remove_bindings_for` removes: one for
/// the consume, and one per occurrence of a produce in the produce list.
pub(crate) fn binding_multiplicity(key: &IOEvent, comm: &COMM) -> usize {
    match key {
        IOEvent::Consume(consume) => usize::from(*consume == comm.consume),
        IOEvent::Produce(produce) => comm
            .produces
            .iter()
            .filter(|listed| *listed == produce)
            .count(),
    }
}

pub(crate) fn stored_record<C, A: Clone>(candidate: &ConsumeCandidate<C, A>) -> Datum<A> {
    Datum {
        a: candidate.removed_datum.clone(),
        persist: candidate.datum.persist,
        source: candidate.datum.source.clone(),
    }
}

/// Position of the record that each stored candidate binds: the first record
/// of its channel's list that is byte-equal to the bound record and that no
/// earlier non-persistent candidate claimed. `keys[i]` is the key of
/// candidate `i`, and `records` gives a key's list. The staged view and the
/// commit locate records with this one rule. The incoming candidate has no
/// position.
pub(crate) fn claim_positions<'r, C, A>(
    candidates: &[ConsumeCandidate<C, A>],
    keys: &[Blake2b256Hash],
    records: impl Fn(&Blake2b256Hash) -> Option<&'r [Datum<A>]>,
    absent: SpaceTxnError,
) -> Result<Vec<Option<usize>>, SpaceTxnError>
where
    A: Clone + Serialize + 'r,
{
    let mut claimed: BTreeMap<&Blake2b256Hash, BTreeSet<usize>> = BTreeMap::new();
    let mut positions = Vec::with_capacity(candidates.len());
    for (candidate, key) in candidates.iter().zip(keys) {
        if candidate.datum_index < 0 {
            positions.push(None);
            continue;
        }
        let list = records(key).ok_or_else(|| absent.clone())?;
        let claims = claimed.entry(key).or_default();
        let position = first_equal_datum(list, &stored_record(candidate), claims)?
            .ok_or_else(|| absent.clone())?;
        if !candidate.datum.persist {
            claims.insert(position);
        }
        positions.push(Some(position));
    }
    Ok(positions)
}

/// The records that a fire removes (its stored non-persistent candidates),
/// as `(candidate, position)` pairs. A peek fire removes them too, as the
/// host does (decision 45, Rule E).
pub(crate) fn removed_records<C, A: Clone>(
    candidates: &[ConsumeCandidate<C, A>],
    positions: &[Option<usize>],
) -> Vec<(usize, usize)> {
    // Disabled by decision 45 (Rule E, "Apply everywhere"): a retaining
    // (peek) fire removed nothing. The `false` arm is the removal below.
    //
    // ```
    // match retain {
    //     true => Vec::new(),
    //     false => candidates
    //         .iter()
    //         .zip(positions)
    //         .enumerate()
    //         .filter_map(|(index, (candidate, position))| match candidate.datum.persist {
    //             true => None,
    //             false => position.map(|position| (index, position)),
    //         })
    //         .collect(),
    // }
    // ```
    candidates
        .iter()
        .zip(positions)
        .enumerate()
        .filter_map(|(index, (candidate, position))| match candidate.datum.persist {
            true => None,
            false => position.map(|position| (index, position)),
        })
        .collect()
}

// Disabled by decision 45 (Rule E, "Apply everywhere"): only the retaining
// fire unjoined its candidates without removing them, so this helper has no
// caller.
//
// ```
// // The host removes and unjoins candidates in descending `datum_index` order
// // (ops_produce.rs, ops_consume.rs, replay_rspace.rs).
// pub(crate) fn by_descending_index<C, A: Clone>(
//     candidates: &[ConsumeCandidate<C, A>],
// ) -> Vec<&ConsumeCandidate<C, A>> {
//     let mut sorted: Vec<_> = candidates.iter().collect();
//     sorted.sort_by_key(|candidate| Reverse(candidate.datum_index));
//     sorted
// }
// ```

/// Keys of every channel of every install key; touching one fails with
/// `InstalledChannel`.
pub(crate) fn installed_channels<C: Serialize, P, K>(
    installs: &Mutex<HashMap<Vec<C>, Install<P, K>>>,
) -> Result<BTreeSet<Blake2b256Hash>, SpaceTxnError> {
    let installs = installs.lock().expect("installs lock");
    let mut installed = BTreeSet::new();
    for channel in installs.keys().flatten() {
        installed.insert(channel_key(channel)?);
    }
    Ok(installed)
}

// One channel's data list in the view.
pub(crate) struct DataEntry<A: Clone> {
    pub(crate) records: Vec<Datum<A>>,
}

// One group's waiting continuations in the view.
pub(crate) struct ContinuationEntry<P: Clone, K: Clone> {
    pub(crate) waiting: Vec<WaitingContinuation<P, K>>,
}

// One channel's join groups in the view, with each group's key.
pub(crate) struct JoinEntry<C> {
    pub(crate) groups: Vec<Vec<C>>,
    pub(crate) keys: Vec<Vec<Blake2b256Hash>>,
}

/// Staged view: the hot store plus the effects staged so far, keyed by store
/// identity. Each list follows the hot store's list rules (hot_store.rs):
/// `put_datum` inserts at 0, `put_continuation` inserts at 0 unless the Debug
/// identity is present, `put_join` inserts at 0 only if absent, `remove_join`
/// removes only when the join's continuation list is empty. A fire removes
/// records at their claimed positions (`claim_positions`, bincode equality,
/// which matches leaf identity), as the commit does.
pub(crate) struct TxnView<C, P: Clone, A: Clone, K: Clone> {
    pub(crate) data: BTreeMap<Blake2b256Hash, DataEntry<A>>,
    pub(crate) conts: BTreeMap<Vec<Blake2b256Hash>, ContinuationEntry<P, K>>,
    pub(crate) joins: BTreeMap<Blake2b256Hash, JoinEntry<C>>,
    pub(crate) count_delta: BTreeMap<Produce, i32>,
}

impl<C, P: Clone, A: Clone, K: Clone> Default for TxnView<C, P, A, K> {
    fn default() -> Self {
        Self {
            data: BTreeMap::new(),
            conts: BTreeMap::new(),
            joins: BTreeMap::new(),
            count_delta: BTreeMap::new(),
        }
    }
}

impl<C, P, A, K> TxnView<C, P, A, K>
where
    C: Clone + Debug + Serialize + Hash + Eq,
    P: Clone + Debug + Serialize,
    A: Clone + Serialize,
    K: Clone + Debug + Serialize,
{
    pub(crate) fn load_data(
        &mut self,
        store: &dyn HotStore<C, P, A, K>,
        key: &Blake2b256Hash,
        channel: &C,
    ) -> &[Datum<A>] {
        &self
            .data
            .entry(key.clone())
            .or_insert_with(|| DataEntry {
                records: store.get_data(channel),
            })
            .records
    }

    pub(crate) fn load_continuations(
        &mut self,
        store: &dyn HotStore<C, P, A, K>,
        key: &[Blake2b256Hash],
        channels: &[C],
    ) -> &[WaitingContinuation<P, K>] {
        &self
            .conts
            .entry(key.to_vec())
            .or_insert_with(|| ContinuationEntry {
                waiting: store.get_continuations(channels),
            })
            .waiting
    }

    /// Loads `channel`'s join groups. A stored group that is not canonical
    /// fails the read and is not cached in the view.
    pub(crate) fn load_joins(
        &mut self,
        store: &dyn HotStore<C, P, A, K>,
        key: &Blake2b256Hash,
        channel: &C,
    ) -> Result<&JoinEntry<C>, SpaceTxnError> {
        if !self.joins.contains_key(key) {
            let groups = store.get_joins(channel);
            let mut keys = Vec::with_capacity(groups.len());
            for group in &groups {
                let group_key = group_identity(group)?;
                require_canonical(&group_key)?;
                keys.push(group_key);
            }
            self.joins.insert(key.clone(), JoinEntry { groups, keys });
        }
        self.joins
            .get(key)
            .ok_or(SpaceTxnError::Inconsistent("staged joins are not loaded"))
    }

    pub(crate) fn put_datum(
        &mut self,
        key: &Blake2b256Hash,
        datum: Datum<A>,
    ) -> Result<(), SpaceTxnError> {
        match self.data.get_mut(key) {
            Some(entry) => {
                entry.records.insert(0, datum);
                Ok(())
            }
            None => Err(SpaceTxnError::Inconsistent("staged data is not loaded")),
        }
    }

    /// Positions of the records that `candidates` bind, in the lists before
    /// the fire (`claim_positions`). `keys[i]` is the key of candidate `i`.
    pub(crate) fn claim_positions(
        &self,
        keys: &[Blake2b256Hash],
        candidates: &[ConsumeCandidate<C, A>],
    ) -> Result<Vec<Option<usize>>, SpaceTxnError> {
        claim_positions(
            candidates,
            keys,
            |key| self.data.get(key).map(|entry| entry.records.as_slice()),
            SpaceTxnError::Inconsistent("a bound record is absent from the view"),
        )
    }

    /// Removes the records of `removed` (`(candidate, position)` pairs, the
    /// positions in the lists before the fire), highest position first so
    /// that each position still holds its record, and returns their effects.
    /// `keys[i]` is the key of candidate `i`.
    pub(crate) fn remove_records(
        &mut self,
        keys: &[Blake2b256Hash],
        candidates: &[ConsumeCandidate<C, A>],
        removed: &[(usize, usize)],
    ) -> Result<Vec<StoreEffect>, SpaceTxnError> {
        let absent = SpaceTxnError::Inconsistent("a bound record is absent from the view");
        let mut ordered: Vec<&(usize, usize)> = removed.iter().collect();
        ordered.sort_by_key(|(_, position)| Reverse(*position));
        let mut effects = Vec::with_capacity(ordered.len());
        for (candidate, position) in ordered {
            let (Some(key), Some(candidate)) = (keys.get(*candidate), candidates.get(*candidate))
            else {
                return Err(absent);
            };
            let entry = self
                .data
                .get_mut(key)
                .ok_or(SpaceTxnError::Inconsistent("staged data is not loaded"))?;
            if *position >= entry.records.len() {
                return Err(absent);
            }
            entry.records.remove(*position);
            effects.push(StoreEffect::RemoveDatum(candidate.datum.source.clone()));
        }
        Ok(effects)
    }

    /// Returns `false` when a continuation with the same Debug identity is
    /// present.
    pub(crate) fn put_continuation(
        &mut self,
        key: &[Blake2b256Hash],
        continuation: WaitingContinuation<P, K>,
    ) -> Result<bool, SpaceTxnError> {
        let identity = continuation_identity(&continuation);
        let entry = self
            .conts
            .get_mut(key)
            .ok_or(SpaceTxnError::Inconsistent("staged continuations are not loaded"))?;
        match entry
            .waiting
            .iter()
            .any(|existing| continuation_identity(existing) == identity)
        {
            true => Ok(false),
            false => {
                entry.waiting.insert(0, continuation);
                Ok(true)
            }
        }
    }

    /// Removes the first continuation byte-equal to `continuation` and returns
    /// its position.
    pub(crate) fn remove_continuation(
        &mut self,
        key: &[Blake2b256Hash],
        continuation: &WaitingContinuation<P, K>,
    ) -> Result<usize, SpaceTxnError> {
        let entry = self
            .conts
            .get_mut(key)
            .ok_or(SpaceTxnError::Inconsistent("staged continuations are not loaded"))?;
        match first_equal_continuation(&entry.waiting, continuation)? {
            Some(position) => {
                entry.waiting.remove(position);
                Ok(position)
            }
            None => {
                Err(SpaceTxnError::Inconsistent("the fired continuation is absent from the view"))
            }
        }
    }

    /// Returns `false` when the joins of `key` already hold the group.
    pub(crate) fn put_join(
        &mut self,
        key: &Blake2b256Hash,
        group_key: &[Blake2b256Hash],
        group: &[C],
    ) -> Result<bool, SpaceTxnError> {
        let entry = self
            .joins
            .get_mut(key)
            .ok_or(SpaceTxnError::Inconsistent("staged joins are not loaded"))?;
        match entry
            .keys
            .iter()
            .any(|joined| joined.as_slice() == group_key)
        {
            true => Ok(false),
            false => {
                entry.groups.insert(0, group.to_vec());
                entry.keys.insert(0, group_key.to_vec());
                Ok(true)
            }
        }
    }

    /// Removes the group `group_key` from the joins of `key` only if the
    /// group has no waiting continuation. Returns whether it removed it.
    pub(crate) fn remove_join(
        &mut self,
        key: &Blake2b256Hash,
        group_key: &[Blake2b256Hash],
    ) -> Result<bool, SpaceTxnError> {
        let waiting = self
            .conts
            .get(group_key)
            .ok_or(SpaceTxnError::Inconsistent("staged continuations are not loaded"))?;
        if !waiting.waiting.is_empty() {
            return Ok(false);
        }
        let entry = self
            .joins
            .get_mut(key)
            .ok_or(SpaceTxnError::Inconsistent("staged joins are not loaded"))?;
        match entry
            .keys
            .iter()
            .position(|joined| joined.as_slice() == group_key)
        {
            Some(position) => {
                entry.groups.remove(position);
                entry.keys.remove(position);
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Staged counter: `host + delta(produce)`.
    pub(crate) fn staged_count(&self, host: i32, produce: &Produce) -> Result<i32, SpaceTxnError> {
        host.checked_add(self.count_delta.get(produce).copied().unwrap_or(0))
            .ok_or(SpaceTxnError::CounterOverflow)
    }
}

/// One entry that a staged operation changes, keyed by store identity. The
/// view records these staged effects as it stages the operation, and commit
/// correlates them with the operation's events (FR-7').
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum StoreEffect {
    PutDatum(Produce),
    RemoveDatum(Produce),
    PutContinuation {
        group: Vec<Blake2b256Hash>,
        source: Consume,
    },
    RemoveContinuation {
        group: Vec<Blake2b256Hash>,
        source: Consume,
    },
    PutJoin {
        channel: Blake2b256Hash,
        group: Vec<Blake2b256Hash>,
    },
    RemoveJoin {
        channel: Blake2b256Hash,
        group: Vec<Blake2b256Hash>,
    },
}

/// Correlation of one operation's staged effects with its events, in every
/// build profile: a stored datum has its Produce event, a removed datum has
/// a COMM that lists its source, a parked continuation has its Consume
/// event, a removed continuation has a COMM of its consume, every join
/// change has the matching continuation change, and every COMM follows its
/// trigger event. The cost is O(effects × events) per operation.
pub(crate) fn check_event_correlation(
    op: usize,
    effects: &[StoreEffect],
    events: &[Event],
) -> Result<(), SpaceTxnError> {
    let produce_event = |source: &Produce| {
        events.iter().any(
            |event| matches!(event, Event::IoEvent(IOEvent::Produce(produce)) if produce == source),
        )
    };
    let consume_event = |source: &Consume| {
        events.iter().any(
            |event| matches!(event, Event::IoEvent(IOEvent::Consume(consume)) if consume == source),
        )
    };
    let comm_with_produce = |source: &Produce| {
        events
            .iter()
            .any(|event| matches!(event, Event::Comm(comm) if comm.produces.contains(source)))
    };
    let comm_with_consume = |source: &Consume| {
        events
            .iter()
            .any(|event| matches!(event, Event::Comm(comm) if comm.consume == *source))
    };
    let continuation_put = |group: &Vec<Blake2b256Hash>| {
        effects.iter().any(
            |effect| matches!(effect, StoreEffect::PutContinuation { group: put, .. } if put == group),
        )
    };
    let continuation_removed = |group: &Vec<Blake2b256Hash>| {
        effects.iter().any(
            |effect| matches!(effect, StoreEffect::RemoveContinuation { group: removed, .. } if removed == group),
        )
    };
    for effect in effects {
        let (covered, rule) = match effect {
            StoreEffect::PutDatum(source) => {
                (produce_event(source), "stored datum without its produce event")
            }
            StoreEffect::RemoveDatum(source) => {
                (comm_with_produce(source), "removed datum without a COMM that lists it")
            }
            StoreEffect::PutContinuation { source, .. } => {
                (consume_event(source), "parked continuation without its consume event")
            }
            StoreEffect::RemoveContinuation { source, .. } => {
                (comm_with_consume(source), "removed continuation without its COMM")
            }
            StoreEffect::PutJoin { group, .. } => {
                (continuation_put(group), "join added without a parked continuation")
            }
            StoreEffect::RemoveJoin { group, .. } => {
                (continuation_removed(group), "join removed without a removed continuation")
            }
        };
        if !covered {
            return Err(SpaceTxnError::EventCoverage { op, rule });
        }
    }
    for (position, event) in events.iter().enumerate() {
        if let Event::Comm(comm) = event {
            let triggered = events.iter().take(position).any(|earlier| match earlier {
                Event::IoEvent(IOEvent::Consume(consume)) => *consume == comm.consume,
                Event::IoEvent(IOEvent::Produce(produce)) => comm.produces.contains(produce),
                Event::Comm(_) => false,
            });
            if !triggered {
                return Err(SpaceTxnError::EventCoverage {
                    op,
                    rule: "COMM precedes its trigger event",
                });
            }
        }
    }
    Ok(())
}

/// A staged fire of a produce: the fired join and its key, its continuation,
/// the candidates, the staged COMM, retention, and the recorded COMM in
/// replay. `removed` holds the `(candidate, position)` pairs of the records
/// that the fire removes, and `continuation_position` the position of a
/// removed (non-persistent) continuation, both in the staged lists before
/// the fire.
pub(crate) struct StagedFire<C, P: Clone, A: Clone, K: Clone> {
    pub(crate) channels: Vec<C>,
    pub(crate) group: Vec<Blake2b256Hash>,
    pub(crate) continuation: WaitingContinuation<P, K>,
    pub(crate) candidates: Vec<ConsumeCandidate<C, A>>,
    pub(crate) comm: COMM,
    // Disabled by decision 45 (Rule E, "Apply everywhere"): a fire no
    // longer retains its records.
    //
    // ```
    // pub(crate) retain: bool,
    // ```
    /// The paired COMM of the log. `None` in play, and for an unpaired fire
    /// in replay or reporting.
    pub(crate) recorded: Option<COMM>,
    pub(crate) removed: Vec<(usize, usize)>,
    pub(crate) continuation_position: Option<usize>,
}

pub(crate) struct StagedProduce<C, P: Clone, A: Clone, K: Clone> {
    pub(crate) source: Produce,
    pub(crate) channel: C,
    pub(crate) data: A,
    pub(crate) persist: bool,
    pub(crate) joins_at_start: Vec<Vec<C>>,
    pub(crate) fire: Option<StagedFire<C, P, A, K>>,
}

/// A staged fire of a consume: the key of its group, the candidates, the
/// staged COMM, and the recorded COMM in replay. `removed` holds the
/// `(candidate, position)` pairs of the records that the fire removes, in the
/// staged lists before the fire.
pub(crate) struct StagedConsumeFire<C, A: Clone> {
    pub(crate) group: Vec<Blake2b256Hash>,
    pub(crate) candidates: Vec<ConsumeCandidate<C, A>>,
    pub(crate) comm: COMM,
    // Disabled by decision 45 (Rule E, "Apply everywhere"): a fire no
    // longer retains its records.
    //
    // ```
    // pub(crate) retain: bool,
    // ```
    pub(crate) removed: Vec<(usize, usize)>,
    /// The paired COMM of the log. `None` in play, and for an unpaired fire
    /// in replay or reporting.
    pub(crate) recorded: Option<COMM>,
}

pub(crate) struct StagedConsume<C, P: Clone, A: Clone, K: Clone> {
    pub(crate) source: Consume,
    pub(crate) channels: Vec<C>,
    pub(crate) patterns: Vec<P>,
    pub(crate) continuation: K,
    pub(crate) persist: bool,
    pub(crate) peeks: BTreeSet<i32>,
    pub(crate) fire: Option<StagedConsumeFire<C, A>>,
}

pub(crate) enum StagedOperation<C, P: Clone, A: Clone, K: Clone> {
    Produce(StagedProduce<C, P, A, K>),
    Consume(StagedConsume<C, P, A, K>),
}

// A staged operation with its staged effects (FR-7').
pub(crate) struct Staged<C, P: Clone, A: Clone, K: Clone> {
    operation: StagedOperation<C, P, A, K>,
    before_observe: Option<BeforeObserve>,
    effects: Vec<StoreEffect>,
}

/// One host mode. Each child module (`space_play`, `space_replay`)
/// implements `apply_op` in the host's exact call order.
pub(crate) trait TxnBackend<C, P, A, K>: Send + Sync + 'static
where
    C: Clone + Hash + Eq,
    P: Clone,
    A: Clone,
    K: Clone,
{
    /// The host's current hot store.
    fn store(&self) -> SharedStore<C, P, A, K>;

    /// The host produce counter (play: trace_log.rs; replay:
    /// `get_produce_count`).
    fn host_counter(&self, produce: &Produce) -> i32;

    /// Recorded bindings under `event` with their multiplicities (empty in
    /// play).
    fn recorded(&self, event: &IOEvent) -> Vec<(COMM, usize)>;

    /// The recorded copy of `comm` under `event` and its multiplicity.
    fn recorded_binding(&self, event: &IOEvent, comm: &COMM) -> Option<(COMM, usize)>;

    /// Applies one staged operation to `store` and reports the events it
    /// emitted.
    fn apply_op(
        &self,
        store: &dyn HotStore<C, P, A, K>,
        operation: StagedOperation<C, P, A, K>,
        before_observe: Option<BeforeObserve>,
        events: &mut Vec<Event>,
    ) -> Result<SpaceOpReceipt<C, P, A, K>, SpaceTxnError>;
}

pub(crate) struct TxnCore<C, P, A, K>
where
    C: Clone + Hash + Eq,
    P: Clone,
    A: Clone,
    K: Clone,
{
    mode: SpaceMode,
    store: SharedStore<C, P, A, K>,
    installed: BTreeSet<Blake2b256Hash>,
    coverage: LockCoverage,
    view: TxnView<C, P, A, K>,
    paired: Vec<COMM>,
    staged: Vec<Staged<C, P, A, K>>,
    finished: bool,
}

impl<C, P, A, K> TxnCore<C, P, A, K>
where
    C: Clone + Debug + Serialize + Hash + Eq + Send + Sync + 'static,
    P: Clone + Debug + Serialize + Send + Sync + 'static,
    A: Clone + Debug + Serialize + Send + Sync + 'static,
    K: Clone + Debug + Serialize + Send + Sync + 'static,
{
    fn ensure_open(&self) -> Result<(), SpaceTxnError> {
        match self.finished {
            true => Err(SpaceTxnError::Finished),
            false => Ok(()),
        }
    }

    fn admit(&self, key: &Blake2b256Hash) -> Result<(), SpaceTxnError> {
        match self.installed.contains(key) {
            true => Err(SpaceTxnError::InstalledChannel),
            false => Ok(()),
        }
    }

    // Arity, canonical order, lock coverage and the installed guard of a
    // staged group, in that order and before any hot-store access.
    fn admit_group(
        &self,
        channels: &[C],
        patterns: usize,
        lock_use: LockUse,
    ) -> Result<Vec<Blake2b256Hash>, SpaceTxnError> {
        check_arity(channels, patterns)?;
        let key = canonical_group_key(channels)?;
        for channel in channels {
            self.coverage.require(channel, lock_use)?;
        }
        for channel_key in &key {
            self.admit(channel_key)?;
        }
        Ok(key)
    }

    fn read_data(&mut self, channel: &C) -> Result<&[Datum<A>], SpaceTxnError> {
        self.ensure_open()?;
        let key = channel_key(channel)?;
        self.admit(&key)?;
        Ok(self.view.load_data(&**self.store, &key, channel))
    }

    fn read_continuations(
        &mut self,
        channels: &[C],
    ) -> Result<&[WaitingContinuation<P, K>], SpaceTxnError> {
        self.ensure_open()?;
        let key = canonical_group_key(channels)?;
        for channel_key in &key {
            self.admit(channel_key)?;
        }
        Ok(self.view.load_continuations(&**self.store, &key, channels))
    }

    fn read_joins(&mut self, channel: &C) -> Result<&[Vec<C>], SpaceTxnError> {
        self.ensure_open()?;
        let key = channel_key(channel)?;
        self.admit(&key)?;
        self.view
            .load_joins(&**self.store, &key, channel)
            .map(|entry| entry.groups.as_slice())
    }

    fn view_counter<B: TxnBackend<C, P, A, K>>(
        &self,
        backend: &B,
        produce: &Produce,
    ) -> Result<i32, SpaceTxnError> {
        self.view
            .staged_count(backend.host_counter(produce), produce)
    }

    // Counters of the COMM's produces: the staged counter of each produce,
    // plus one for the produce being fired when it is not persistent (the
    // play closure of ops_produce.rs). `incoming` is that produce, if any.
    fn comm_counters<B: TxnBackend<C, P, A, K>>(
        &self,
        backend: &B,
        candidates: &[ConsumeCandidate<C, A>],
        incoming: Option<&Produce>,
    ) -> Result<BTreeMap<Produce, i32>, SpaceTxnError> {
        let mut counters = BTreeMap::new();
        for candidate in candidates {
            let produce = &candidate.datum.source;
            if counters.contains_key(produce) {
                continue;
            }
            let staged = self.view_counter(backend, produce)?;
            let count = match incoming {
                Some(source) if source == produce => staged
                    .checked_add(1)
                    .ok_or(SpaceTxnError::CounterOverflow)?,
                _ => staged,
            };
            counters.insert(produce.clone(), count);
        }
        Ok(counters)
    }

    // The staged increment of a non-persistent produce: the new delta
    // `δ(src) + 1`. The host counter after the operation, the staged counter
    // plus one, must also fit, because the host increments it unchecked.
    fn next_delta<B: TxnBackend<C, P, A, K>>(
        &self,
        backend: &B,
        source: &Produce,
        persist: bool,
    ) -> Result<Option<i32>, SpaceTxnError> {
        match persist {
            true => Ok(None),
            false => {
                self.view_counter(backend, source)?
                    .checked_add(1)
                    .ok_or(SpaceTxnError::CounterOverflow)?;
                self.view
                    .count_delta
                    .get(source)
                    .copied()
                    .unwrap_or(0)
                    .checked_add(1)
                    .map(Some)
                    .ok_or(SpaceTxnError::CounterOverflow)
            }
        }
    }

    // Replay pairing under the trigger key only, as host replay looks it up:
    // `Produce(source)` for a produce-side fire and `Consume(source)` for a
    // consume-side fire. The key holds one binding per occurrence of the key
    // in the COMM (`binding_multiplicity`), so a recorded occurrence is
    // available while the paired copies use fewer bindings than it holds.
    // Returns the recorded copy, or `None` in play and for an unpaired fire.
    // An unpaired fire stages as in play, never pairs, and fails at commit
    // after the observer calls of play (decision 36), so replay staging
    // equals play staging for every log.
    fn verify<B: TxnBackend<C, P, A, K>>(
        &self,
        backend: &B,
        comm: &COMM,
        trigger: &IOEvent,
    ) -> Option<COMM> {
        match self.mode {
            SpaceMode::Play => None,
            SpaceMode::Replay | SpaceMode::Reporting => {
                let (recorded, count) = backend.recorded_binding(trigger, comm)?;
                let available = match binding_multiplicity(trigger, comm) {
                    0 => 0,
                    multiplicity => count / multiplicity,
                };
                let paired = self.paired.iter().filter(|paired| *paired == comm).count();
                (paired < available).then_some(recorded)
            }
        }
    }

    // Loads a fired group: the data and the join groups of every channel and
    // the group's continuations.
    fn load_group(&mut self, key: &[Blake2b256Hash], channels: &[C]) -> Result<(), SpaceTxnError> {
        let store = &**self.store;
        for (channel_key, channel) in key.iter().zip(channels) {
            self.view.load_data(store, channel_key, channel);
            self.view.load_joins(store, channel_key, channel)?;
        }
        self.view.load_continuations(store, key, channels);
        Ok(())
    }

    // Every stored candidate binds the record at its index in the staged
    // view, and a non-persistent record binds once per COMM, also when the
    // group repeats its channel (FR-1'). `keys[i]` is the key of candidate
    // `i`.
    fn check_stored_candidates(
        &self,
        keys: &[Blake2b256Hash],
        candidates: &[ConsumeCandidate<C, A>],
    ) -> Result<(), SpaceTxnError> {
        let mut bound: BTreeSet<(&Blake2b256Hash, usize)> = BTreeSet::new();
        for (candidate, key) in candidates.iter().zip(keys) {
            let Ok(position) = usize::try_from(candidate.datum_index) else {
                continue;
            };
            let entry = self
                .view
                .data
                .get(key)
                .ok_or(SpaceTxnError::Inconsistent("staged data is not loaded"))?;
            let record = entry
                .records
                .get(position)
                .ok_or(SpaceTxnError::NotInView("a stored candidate index"))?;
            if record_bytes(record)? != record_bytes(&stored_record(candidate))? {
                return Err(SpaceTxnError::NotInView("a stored candidate record"));
            }
            if !candidate.datum.persist && !bound.insert((key, position)) {
                return Err(SpaceTxnError::NotInView("a non-persistent record bound twice"));
            }
        }
        Ok(())
    }

    // A view mutation that fails after validation leaves the view unusable,
    // so the transaction is closed before the error is returned.
    fn poison(&mut self, error: SpaceTxnError) -> SpaceTxnError {
        self.finished = true;
        error
    }

    // Validation order: the produce channel's lock (phase A) and installed
    // guard; for a fire, the fired group (arity, canonical order, phase-B
    // locks, installed guard) and its candidates. Then the hot-store reads:
    // the channel's join groups (canonical, every channel locked in phase
    // B), its data, and for a fire the fired group.
    fn stage_produce<B: TxnBackend<C, P, A, K>>(
        &mut self,
        backend: &B,
        op: SpaceProduce<C, P, A, K>,
    ) -> Result<StagedOp, SpaceTxnError> {
        self.ensure_open()?;
        let SpaceProduce {
            channel,
            data,
            persist,
            outcome,
            before_observe,
        } = op;
        let key = channel_key(&channel)?;
        self.coverage.require(&channel, LockUse::Produced)?;
        self.admit(&key)?;
        let source = Produce::create(&channel, &data, persist);
        let fired = match outcome {
            ProduceOutcome::Store => None,
            ProduceOutcome::Fire {
                channels,
                continuation,
                data: candidates,
            } => {
                let group =
                    self.admit_group(&channels, continuation.patterns.len(), LockUse::Joined)?;
                if !group.contains(&key) {
                    return Err(SpaceTxnError::InvalidOperation(
                        "the produce channel is not a channel of the fired join",
                    ));
                }
                check_peeks(channels.len(), &continuation.peeks)?;
                check_lined_up(&group, &candidates)?;
                check_incoming(&candidates, &key, &source, &data, persist)?;
                Some((group, channels, continuation, candidates))
            }
        };
        let joins_at_start = {
            let entry = self.view.load_joins(&**self.store, &key, &channel)?;
            for group in &entry.groups {
                for joined in group {
                    self.coverage.require(joined, LockUse::Joined)?;
                }
            }
            entry.groups.clone()
        };
        self.view.load_data(&**self.store, &key, &channel);
        let index = self.staged.len();
        match fired {
            None => {
                let delta = self.next_delta(backend, &source, persist)?;
                let record = Datum {
                    a: data.clone(),
                    persist,
                    source: source.clone(),
                };
                if let Err(error) = self.view.put_datum(&key, record) {
                    return Err(self.poison(error));
                }
                if let Some(delta) = delta {
                    self.view.count_delta.insert(source.clone(), delta);
                }
                self.staged.push(Staged {
                    operation: StagedOperation::Produce(StagedProduce {
                        source: source.clone(),
                        channel,
                        data,
                        persist,
                        joins_at_start,
                        fire: None,
                    }),
                    before_observe,
                    effects: vec![StoreEffect::PutDatum(source.clone())],
                });
                Ok(StagedOp {
                    index,
                    source: OpSource::Produce(source),
                    comm: None,
                })
            }
            Some((group, channels, continuation, candidates)) => {
                self.load_group(&group, &channels)?;
                let entry = self
                    .view
                    .conts
                    .get(&group)
                    .ok_or(SpaceTxnError::Inconsistent("staged continuations are not loaded"))?;
                if first_equal_continuation(&entry.waiting, &continuation)?.is_none() {
                    return Err(SpaceTxnError::NotInView("the waiting continuation"));
                }
                self.check_stored_candidates(&group, &candidates)?;
                let positions = self.view.claim_positions(&group, &candidates)?;
                let incoming = match persist {
                    true => None,
                    false => Some(&source),
                };
                let counters = self.comm_counters(backend, &candidates, incoming)?;
                let comm = COMM::new(
                    &candidates,
                    continuation.source.clone(),
                    continuation.peeks.clone(),
                    |produces| lookup_counters(&counters, produces),
                );
                let recorded = self.verify(backend, &comm, &IOEvent::Produce(source.clone()));
                let delta = self.next_delta(backend, &source, persist)?;
                let removed = removed_records(&candidates, &positions);
                let applied = self.apply_fire_to_view(&group, &continuation, &candidates, &removed);
                let (effects, continuation_position) = match applied {
                    Ok(applied) => applied,
                    Err(error) => return Err(self.poison(error)),
                };
                if let Some(delta) = delta {
                    self.view.count_delta.insert(source.clone(), delta);
                }
                if recorded.is_some() {
                    self.paired.push(comm.clone());
                }
                self.staged.push(Staged {
                    operation: StagedOperation::Produce(StagedProduce {
                        source: source.clone(),
                        channel,
                        data,
                        persist,
                        joins_at_start,
                        fire: Some(StagedFire {
                            channels,
                            group,
                            continuation,
                            candidates,
                            comm: comm.clone(),
                            recorded,
                            removed,
                            continuation_position,
                        }),
                    }),
                    before_observe,
                    effects,
                });
                Ok(StagedOp {
                    index,
                    source: OpSource::Produce(source),
                    comm: Some(comm),
                })
            }
        }
    }

    // View effects of a produce-triggered fire, in the host order: remove a
    // non-persistent continuation, then the records that the fire removes
    // (`removed`) and the fired join of every candidate channel. The incoming
    // datum is never stored: a peek fire leaves it to the agent, which offers
    // it again (FR-10, decision 30). Returns the staged effects and the
    // position of the removed continuation.
    fn apply_fire_to_view(
        &mut self,
        group: &[Blake2b256Hash],
        continuation: &WaitingContinuation<P, K>,
        candidates: &[ConsumeCandidate<C, A>],
        removed: &[(usize, usize)],
    ) -> Result<(Vec<StoreEffect>, Option<usize>), SpaceTxnError> {
        let mut effects = Vec::with_capacity(removed.len() + group.len() + 1);
        let continuation_position = match continuation.persist {
            true => None,
            false => {
                let position = self.view.remove_continuation(group, continuation)?;
                effects.push(StoreEffect::RemoveContinuation {
                    group: group.to_vec(),
                    source: continuation.source.clone(),
                });
                Some(position)
            }
        };
        effects.extend(self.view.remove_records(group, candidates, removed)?);
        for channel_key in group {
            if self.view.remove_join(channel_key, group)? {
                effects.push(StoreEffect::RemoveJoin {
                    channel: channel_key.clone(),
                    group: group.to_vec(),
                });
            }
        }
        Ok((effects, continuation_position))
    }

    // View effects of a park: the continuation, unless the view holds one
    // with its Debug identity, and the join of every channel that lacks it.
    // Returns the staged effects.
    fn park_in_view(
        &mut self,
        group: &[Blake2b256Hash],
        channels: &[C],
        waiting: WaitingContinuation<P, K>,
    ) -> Result<Vec<StoreEffect>, SpaceTxnError> {
        let source = waiting.source.clone();
        let mut effects = Vec::with_capacity(group.len() + 1);
        if self.view.put_continuation(group, waiting)? {
            effects.push(StoreEffect::PutContinuation {
                group: group.to_vec(),
                source,
            });
        }
        for channel_key in group {
            if self.view.put_join(channel_key, group, channels)? {
                effects.push(StoreEffect::PutJoin {
                    channel: channel_key.clone(),
                    group: group.to_vec(),
                });
            }
        }
        Ok(effects)
    }

    // Validation order: arity, canonical order, both lock phases of every
    // channel, the installed guard, and the peeks rule; then the outcome.
    fn stage_consume<B: TxnBackend<C, P, A, K>>(
        &mut self,
        backend: &B,
        op: SpaceConsume<C, P, A, K>,
    ) -> Result<StagedOp, SpaceTxnError> {
        self.ensure_open()?;
        let SpaceConsume {
            channels,
            patterns,
            continuation,
            persist,
            peeks,
            outcome,
            before_observe,
        } = op;
        let group = self.admit_group(&channels, patterns.len(), LockUse::Consumed)?;
        check_peeks(channels.len(), &peeks)?;
        let source = Consume::create(&channels, &patterns, &continuation, persist);
        let index = self.staged.len();
        match outcome {
            ConsumeOutcome::Park => {
                {
                    let store = &**self.store;
                    self.view.load_continuations(store, &group, &channels);
                    for (channel_key, channel) in group.iter().zip(&channels) {
                        self.view.load_joins(store, channel_key, channel)?;
                    }
                }
                let waiting = WaitingContinuation {
                    patterns: patterns.clone(),
                    continuation: continuation.clone(),
                    persist,
                    peeks: peeks.clone(),
                    source: source.clone(),
                };
                let effects = match self.park_in_view(&group, &channels, waiting) {
                    Ok(effects) => effects,
                    Err(error) => return Err(self.poison(error)),
                };
                self.staged.push(Staged {
                    operation: StagedOperation::Consume(StagedConsume {
                        source: source.clone(),
                        channels,
                        patterns,
                        continuation,
                        persist,
                        peeks,
                        fire: None,
                    }),
                    before_observe,
                    effects,
                });
                Ok(StagedOp {
                    index,
                    source: OpSource::Consume(source),
                    comm: None,
                })
            }
            ConsumeOutcome::Fire { data: candidates } => {
                check_lined_up(&group, &candidates)?;
                if candidates.iter().any(|candidate| candidate.datum_index < 0) {
                    return Err(SpaceTxnError::InvalidOperation(
                        "a consume fire binds only stored records",
                    ));
                }
                {
                    let store = &**self.store;
                    for (channel_key, channel) in group.iter().zip(&channels) {
                        self.view.load_data(store, channel_key, channel);
                    }
                }
                self.check_stored_candidates(&group, &candidates)?;
                let positions = self.view.claim_positions(&group, &candidates)?;
                let counters = self.comm_counters(backend, &candidates, None)?;
                let comm = COMM::new(&candidates, source.clone(), peeks.clone(), |produces| {
                    lookup_counters(&counters, produces)
                });
                let recorded = self.verify(backend, &comm, &IOEvent::Consume(source.clone()));
                let removed = removed_records(&candidates, &positions);
                let effects = match self.view.remove_records(&group, &candidates, &removed) {
                    Ok(effects) => effects,
                    Err(error) => return Err(self.poison(error)),
                };
                if recorded.is_some() {
                    self.paired.push(comm.clone());
                }
                self.staged.push(Staged {
                    operation: StagedOperation::Consume(StagedConsume {
                        source: source.clone(),
                        channels,
                        patterns,
                        continuation,
                        persist,
                        peeks,
                        fire: Some(StagedConsumeFire {
                            group,
                            candidates,
                            comm: comm.clone(),
                            removed,
                            recorded,
                        }),
                    }),
                    before_observe,
                    effects,
                });
                Ok(StagedOp {
                    index,
                    source: OpSource::Consume(source),
                    comm: Some(comm),
                })
            }
        }
    }
}

fn check_arity<C>(channels: &[C], patterns: usize) -> Result<(), SpaceTxnError> {
    match (channels.is_empty(), channels.len() == patterns) {
        (true, _) => Err(SpaceTxnError::InvalidOperation("channels must not be empty")),
        (false, false) => {
            Err(SpaceTxnError::InvalidOperation("channels and patterns differ in length"))
        }
        (false, true) => Ok(()),
    }
}

// `candidates[i]` binds the channel with key `group[i]`.
fn check_lined_up<C: Serialize, A: Clone>(
    group: &[Blake2b256Hash],
    candidates: &[ConsumeCandidate<C, A>],
) -> Result<(), SpaceTxnError> {
    let misaligned =
        SpaceTxnError::InvalidOperation("the candidates do not line up with the channels");
    if candidates.len() != group.len() {
        return Err(misaligned);
    }
    for (candidate, key) in candidates.iter().zip(group) {
        if channel_key(&candidate.channel)? != *key {
            return Err(misaligned);
        }
    }
    Ok(())
}

// The incoming candidate carries the produced record: its channel, source,
// original data and persistence. A produce fire has exactly one.
fn check_incoming<C: Serialize, A: Clone + Serialize>(
    candidates: &[ConsumeCandidate<C, A>],
    key: &Blake2b256Hash,
    source: &Produce,
    data: &A,
    persist: bool,
) -> Result<(), SpaceTxnError> {
    let source_bytes = record_bytes(source)?;
    let data_bytes = record_bytes(data)?;
    let mut incoming = 0usize;
    for candidate in candidates {
        match candidate.datum_index {
            -1 => {
                let carries = channel_key(&candidate.channel)? == *key &&
                    candidate.datum.persist == persist &&
                    record_bytes(&candidate.datum.source)? == source_bytes &&
                    record_bytes(&candidate.removed_datum)? == data_bytes;
                if !carries {
                    return Err(SpaceTxnError::InvalidOperation(
                        "the incoming candidate does not carry the produced record",
                    ));
                }
                incoming += 1;
            }
            index if index < -1 => {
                return Err(SpaceTxnError::InvalidOperation("a candidate index is below -1"));
            }
            _ => {}
        }
    }
    match incoming {
        1 => Ok(()),
        _ => Err(SpaceTxnError::InvalidOperation(
            "a produce fire needs exactly one incoming candidate",
        )),
    }
}

fn lookup_counters(
    counters: &BTreeMap<Produce, i32>,
    produces: &[Produce],
) -> BTreeMap<Produce, i32> {
    produces
        .iter()
        .map(|produce| (produce.clone(), counters.get(produce).copied().unwrap_or(0)))
        .collect()
}

/// A non-persistent produce increments the host counter without a check, so
/// commit refuses it before any effect when the counter is full.
pub(crate) fn counter_room(host: i32) -> Result<(), SpaceTxnError> {
    host.checked_add(1)
        .map(|_| ())
        .ok_or(SpaceTxnError::CounterOverflow)
}

/// The COMM of a staged fire from the live host counters, before any effect
/// of its operation. `counters` reads the host counters of the listed
/// produces. A non-persistent incoming produce counts once more: the play
/// closure of ops_produce.rs, which equals replay's increment-first count.
/// It must equal the staged COMM.
pub(crate) fn live_comm<C, A: Clone>(
    candidates: &[ConsumeCandidate<C, A>],
    consume: &Consume,
    peeks: &BTreeSet<i32>,
    incoming: Option<&Produce>,
    counters: impl Fn(&[Produce]) -> BTreeMap<Produce, i32>,
    staged: &COMM,
) -> Result<COMM, SpaceTxnError> {
    let produces: Vec<Produce> = candidates
        .iter()
        .map(|candidate| candidate.datum.source.clone())
        .collect();
    let mut live = counters(&produces);
    if let Some(source) = incoming {
        if produces.contains(source) {
            let count = live.entry(source.clone()).or_insert(0);
            *count = count.checked_add(1).ok_or(SpaceTxnError::CounterOverflow)?;
        }
    }
    let comm = COMM::new(candidates, consume.clone(), peeks.clone(), |listed: &[Produce]| {
        lookup_counters(&live, listed)
    });
    match comm == *staged {
        true => Ok(comm),
        false => {
            Err(SpaceTxnError::Inconsistent("the COMM recomputed at commit differs from staging"))
        }
    }
}

/// Positions of the bound records in the live store (`claim_positions`):
/// the first byte-equal record not claimed by an earlier candidate. `keys[i]`
/// is the key of candidate `i`. The incoming candidate keeps `-1`.
pub(crate) fn committed_candidates<C, P, A, K>(
    store: &dyn HotStore<C, P, A, K>,
    candidates: &[ConsumeCandidate<C, A>],
    keys: &[Blake2b256Hash],
) -> Result<Vec<ConsumeCandidate<C, A>>, SpaceTxnError>
where
    C: Clone + Hash + Eq + Serialize,
    P: Clone,
    A: Clone + Serialize,
    K: Clone,
{
    let mut live: BTreeMap<&Blake2b256Hash, Vec<Datum<A>>> = BTreeMap::new();
    for (candidate, key) in candidates.iter().zip(keys) {
        if candidate.datum_index >= 0 && !live.contains_key(key) {
            live.insert(key, store.get_data(&candidate.channel));
        }
    }
    let positions = claim_positions(
        candidates,
        keys,
        |key| live.get(key).map(Vec::as_slice),
        SpaceTxnError::Inconsistent("a bound record is absent from the hot store"),
    )?;
    let mut committed = Vec::with_capacity(candidates.len());
    for (candidate, position) in candidates.iter().zip(positions) {
        let mut candidate = candidate.clone();
        if let Some(position) = position {
            candidate.datum_index = i32::try_from(position)
                .map_err(|_| SpaceTxnError::Inconsistent("a record index exceeds i32"))?;
        }
        committed.push(candidate);
    }
    Ok(committed)
}

// FR-7': a live position located by the commit must equal the position of
// the entry in the staged view before the operation.
fn check_staged_position(live: Option<usize>, staged: usize) -> Result<(), SpaceTxnError> {
    match live == Some(staged) {
        true => Ok(()),
        false => {
            Err(SpaceTxnError::Inconsistent("the committed state differs from the staged view"))
        }
    }
}

/// The state check of FR-7' for the records that an operation removes:
/// before any effect, each one sits at the position where the staged view
/// held it. `committed` holds the live positions, `removed` the staged
/// `(candidate, position)` pairs.
pub(crate) fn check_removed_records<C, A: Clone>(
    committed: &[ConsumeCandidate<C, A>],
    removed: &[(usize, usize)],
) -> Result<(), SpaceTxnError> {
    for (candidate, position) in removed {
        let live = committed
            .get(*candidate)
            .and_then(|candidate| usize::try_from(candidate.datum_index).ok());
        check_staged_position(live, *position)?;
    }
    Ok(())
}

/// Index of the fired continuation in the live store, by bincode equality.
pub(crate) fn committed_continuation_index<C, P, A, K>(
    store: &dyn HotStore<C, P, A, K>,
    channels: &[C],
    continuation: &WaitingContinuation<P, K>,
) -> Result<i32, SpaceTxnError>
where
    C: Clone + Hash + Eq,
    P: Clone + Serialize,
    A: Clone,
    K: Clone + Serialize,
{
    let waiting = store.get_continuations_arc(channels);
    let position = first_equal_continuation(&waiting, continuation)?.ok_or(
        SpaceTxnError::Inconsistent("the fired continuation is absent from the hot store"),
    )?;
    i32::try_from(position)
        .map_err(|_| SpaceTxnError::Inconsistent("a continuation index exceeds i32"))
}

/// A staged produce fire located in the live store before any effect of its
/// operation: the COMM recomputed from the live counters, the index of a
/// non-persistent continuation, and the positions of the bound records. The
/// removed continuation and records sit where the staged view held them.
pub(crate) struct ReadyFire<C, P: Clone, A: Clone, K: Clone> {
    pub(crate) fire: StagedFire<C, P, A, K>,
    pub(crate) comm: COMM,
    pub(crate) continuation_index: Option<i32>,
    pub(crate) committed: Vec<ConsumeCandidate<C, A>>,
}

pub(crate) fn ready_produce_fire<C, P, A, K>(
    store: &dyn HotStore<C, P, A, K>,
    fire: StagedFire<C, P, A, K>,
    incoming: Option<&Produce>,
    counters: impl Fn(&[Produce]) -> BTreeMap<Produce, i32>,
) -> Result<ReadyFire<C, P, A, K>, SpaceTxnError>
where
    C: Clone + Hash + Eq + Serialize,
    P: Clone + Serialize,
    A: Clone + Serialize,
    K: Clone + Serialize,
{
    let comm = live_comm(
        &fire.candidates,
        &fire.continuation.source,
        &fire.continuation.peeks,
        incoming,
        counters,
        &fire.comm,
    )?;
    let continuation_index = match fire.continuation_position {
        None => None,
        Some(position) => {
            let live = committed_continuation_index(store, &fire.channels, &fire.continuation)?;
            check_staged_position(usize::try_from(live).ok(), position)?;
            Some(live)
        }
    };
    let committed = committed_candidates(store, &fire.candidates, &fire.group)?;
    check_removed_records(&committed, &fire.removed)?;
    Ok(ReadyFire {
        fire,
        comm,
        continuation_index,
        committed,
    })
}

/// A staged consume fire located in the live store before any effect of
/// its operation. The removed records sit where the staged view held them.
pub(crate) struct ReadyConsumeFire<C, A: Clone> {
    pub(crate) fire: StagedConsumeFire<C, A>,
    pub(crate) comm: COMM,
    pub(crate) committed: Vec<ConsumeCandidate<C, A>>,
}

pub(crate) fn ready_consume_fire<C, P, A, K>(
    store: &dyn HotStore<C, P, A, K>,
    fire: StagedConsumeFire<C, A>,
    consume: &Consume,
    peeks: &BTreeSet<i32>,
    counters: impl Fn(&[Produce]) -> BTreeMap<Produce, i32>,
) -> Result<ReadyConsumeFire<C, A>, SpaceTxnError>
where
    C: Clone + Hash + Eq + Serialize,
    P: Clone,
    A: Clone + Serialize,
    K: Clone,
{
    let comm = live_comm(&fire.candidates, consume, peeks, None, counters, &fire.comm)?;
    let committed = committed_candidates(store, &fire.candidates, &fire.group)?;
    check_removed_records(&committed, &fire.removed)?;
    Ok(ReadyConsumeFire {
        fire,
        comm,
        committed,
    })
}

/// A staged produce whose fire, if any, is located in the live store.
pub(crate) struct ReadyProduce<C, P: Clone, A: Clone, K: Clone> {
    pub(crate) source: Produce,
    pub(crate) channel: C,
    pub(crate) data: A,
    pub(crate) persist: bool,
    pub(crate) fire: Option<ReadyFire<C, P, A, K>>,
}

/// Read-only commit checks of a staged produce, before any effect of the
/// operation: the channel's join groups are the ones staged against, a
/// non-persistent produce has counter room (`host_counter` is its live host
/// counter), and a fire is located in the live store with its COMM
/// recomputed from `counters`. Returns the produce and the join groups for
/// `observe_operation_start`.
pub(crate) fn ready_produce<C, P, A, K>(
    store: &dyn HotStore<C, P, A, K>,
    produce: StagedProduce<C, P, A, K>,
    host_counter: i32,
    counters: impl Fn(&[Produce]) -> BTreeMap<Produce, i32>,
) -> Result<(ReadyProduce<C, P, A, K>, Vec<Vec<C>>), SpaceTxnError>
where
    C: Clone + Hash + Eq + Serialize,
    P: Clone + Serialize,
    A: Clone + Serialize,
    K: Clone + Serialize,
{
    let StagedProduce {
        source,
        channel,
        data,
        persist,
        joins_at_start,
        fire,
    } = produce;
    let joins = store.get_joins(&channel);
    if record_bytes(&joins)? != record_bytes(&joins_at_start)? {
        return Err(SpaceTxnError::Inconsistent("the join groups changed since staging"));
    }
    if !persist {
        counter_room(host_counter)?;
    }
    let fire = match fire {
        None => None,
        Some(fire) => {
            let incoming = match persist {
                true => None,
                false => Some(&source),
            };
            Some(ready_produce_fire(store, fire, incoming, counters)?)
        }
    };
    Ok((
        ReadyProduce {
            source,
            channel,
            data,
            persist,
            fire,
        },
        joins,
    ))
}

/// A staged consume whose fire, if any, is located in the live store.
pub(crate) struct ReadyConsume<C, P: Clone, A: Clone, K: Clone> {
    pub(crate) source: Consume,
    pub(crate) channels: Vec<C>,
    pub(crate) patterns: Vec<P>,
    pub(crate) continuation: K,
    pub(crate) persist: bool,
    pub(crate) peeks: BTreeSet<i32>,
    pub(crate) fire: Option<ReadyConsumeFire<C, A>>,
}

/// Read-only commit checks of a staged consume, before any effect of the
/// operation: a fire is located in the live store with its COMM recomputed
/// from `counters`.
pub(crate) fn ready_consume<C, P, A, K>(
    store: &dyn HotStore<C, P, A, K>,
    consume: StagedConsume<C, P, A, K>,
    counters: impl Fn(&[Produce]) -> BTreeMap<Produce, i32>,
) -> Result<ReadyConsume<C, P, A, K>, SpaceTxnError>
where
    C: Clone + Hash + Eq + Serialize,
    P: Clone,
    A: Clone + Serialize,
    K: Clone,
{
    let StagedConsume {
        source,
        channels,
        patterns,
        continuation,
        persist,
        peeks,
        fire,
    } = consume;
    let fire = match fire {
        None => None,
        Some(fire) => Some(ready_consume_fire(store, fire, &source, &peeks, counters)?),
    };
    Ok(ReadyConsume {
        source,
        channels,
        patterns,
        continuation,
        persist,
        peeks,
        fire,
    })
}

pub(crate) fn run_before_observe(
    before_observe: Option<BeforeObserve>,
    source: RSpaceOperationSource<'_>,
) -> Result<(), SpaceTxnError> {
    match before_observe {
        Some(callback) => callback(source).map_err(SpaceTxnError::Host),
        None => Ok(()),
    }
}

pub(crate) fn completion<C, P, A, K>(
    result: &Result<SpaceOpReceipt<C, P, A, K>, SpaceTxnError>,
) -> RSpaceOperationCompletion {
    match result {
        Ok(SpaceOpReceipt::Stored { .. }) | Ok(SpaceOpReceipt::Parked { .. }) => {
            RSpaceOperationCompletion::Stored
        }
        Ok(SpaceOpReceipt::Fired { .. }) => RSpaceOperationCompletion::Matched,
        Err(_) => RSpaceOperationCompletion::Rejected,
    }
}

// Disabled by decision 45 (Rule E, "Apply everywhere"): no fire retains a
// record, so every result keeps the flag of the host's `wrap_result`.
//
// ```
// /// The results of a fire that its receipt reports as retained: when the fire
// /// retains (peeks), the results of its stored candidates. The incoming
// /// candidate of a produce fire is never retained, because the fire does not
// /// store it (FR-10).
// pub(crate) fn retained_results<C, A: Clone>(
//     candidates: &[ConsumeCandidate<C, A>],
//     retain: bool,
// ) -> Vec<bool> {
//     candidates
//         .iter()
//         .map(|candidate| retain && candidate.datum_index >= 0)
//         .collect()
// }
// ```

/// Receipt of a fire from the host's `wrap_result`, whose result `i` belongs
/// to candidate `i` of the `candidates` candidates of the fire. Each result
/// keeps the flag of the host, so a non-persistent result tells the agent that
/// the fire removed or spent its datum (decision 45, Rule E).
pub(crate) fn fired_receipt<C, P, A, K>(
    comm: COMM,
    wrapped: MaybeConsumeResult<C, P, A, K>,
    candidates: usize,
    produce: Option<Produce>,
) -> Result<SpaceOpReceipt<C, P, A, K>, SpaceTxnError> {
    match wrapped {
        Some((continuation, results)) => {
            if results.len() != candidates {
                return Err(SpaceTxnError::Inconsistent(
                    "the host fire results do not line up with the candidates",
                ));
            }
            // Disabled by decision 45 (Rule E, "Apply everywhere"): a
            // retained result was reported persistent.
            //
            // ```
            // for (result, retained) in results.iter_mut().zip(retained) {
            //     if *retained {
            //         result.persistent = true;
            //     }
            // }
            // ```
            Ok(SpaceOpReceipt::Fired {
                comm,
                continuation,
                results,
                produce,
            })
        }
        None => Err(SpaceTxnError::Inconsistent("the host returned no fire result")),
    }
}

// Commits one staged operation and correlates its staged effects with the
// events it emitted. The state check of the entries that it removes runs in
// `apply_op` (`ready_produce`, `ready_consume`) before any of its effects.
// Both checks cost O(changed entries) and run in every build profile, so
// debug and release commit identical results (FR-7').
fn commit_operation<B, C, P, A, K>(
    backend: &B,
    store: &dyn HotStore<C, P, A, K>,
    index: usize,
    staged: Staged<C, P, A, K>,
) -> Result<SpaceOpReceipt<C, P, A, K>, SpaceTxnError>
where
    B: TxnBackend<C, P, A, K>,
    C: Clone + Hash + Eq,
    P: Clone,
    A: Clone,
    K: Clone,
{
    let Staged {
        operation,
        before_observe,
        effects,
    } = staged;
    let mut events = Vec::with_capacity(2);
    let receipt = backend.apply_op(store, operation, before_observe, &mut events)?;
    check_event_correlation(index, &effects, &events)?;
    Ok(receipt)
}

// Applies the staged operations in order. A commit under a parent order
// that already committed in this root evaluation fails before any effect;
// then operation `i` runs under `sub_order(parent, i)`. Without an operation
// context the commit is not tracked and emits no sub-orders. A failure of
// operation `i` leaves operations `0..i` applied, as the equivalent host
// program would. An unpaired replay fire fails here too: with the error of
// a call that the observer rejects, or else with `ReplayDivergence` after
// its calls, and with the same prefix (decision 36).
async fn commit_staged<B, C, P, A, K>(
    backend: &B,
    store: SharedStore<C, P, A, K>,
    registration: &TxnRegistration,
    staged: Vec<Staged<C, P, A, K>>,
) -> Result<SpaceTxnReceipt<C, P, A, K>, SpaceTxnError>
where
    B: TxnBackend<C, P, A, K>,
    C: Clone + Debug + Serialize + Hash + Eq + Send + Sync + 'static,
    P: Clone + Debug + Serialize + Send + Sync + 'static,
    A: Clone + Debug + Serialize + Send + Sync + 'static,
    K: Clone + Debug + Serialize + Send + Sync + 'static,
{
    if !Arc::ptr_eq(&store, &backend.store()) {
        return Err(SpaceTxnError::StoreSwapped);
    }
    let parent = operation_context::current();
    if let Some(parent) = &parent {
        registration.admit_parent(parent, &store)?;
    }
    let mut ops = Vec::with_capacity(staged.len());
    for (index, entry) in staged.into_iter().enumerate() {
        let receipt = match &parent {
            Some(parent) => {
                let store = &**store;
                operation_context::scope(sub_order(parent, index), async move {
                    commit_operation(backend, store, index, entry)
                })
                .await?
            }
            None => commit_operation(backend, &**store, index, entry)?,
        };
        ops.push(receipt);
    }
    Ok(SpaceTxnReceipt { ops })
}

// What an open transaction holds: the striped locks of `begin` and its
// registration with the handle. The fields drop in order: the locks, then
// the registration.
struct HeldTxn {
    _locks: (ChannelLockGuard, ChannelLockGuard),
    registration: TxnRegistration,
}

/// The transaction of every host mode: a staging core, the mode's backend,
/// and what `begin` holds until commit, a fail-closed error, or drop.
pub(crate) struct HostTxn<B, C, P, A, K>
where
    C: Clone + Hash + Eq,
    P: Clone,
    A: Clone,
    K: Clone,
{
    core: TxnCore<C, P, A, K>,
    backend: B,
    held: Option<HeldTxn>,
}

impl<B, C, P, A, K> HostTxn<B, C, P, A, K>
where
    C: Clone + Hash + Eq,
    P: Clone,
    A: Clone,
    K: Clone,
{
    pub(crate) fn new(
        mode: SpaceMode,
        backend: B,
        store: SharedStore<C, P, A, K>,
        installed: BTreeSet<Blake2b256Hash>,
        coverage: LockCoverage,
        locks: (ChannelLockGuard, ChannelLockGuard),
        registration: TxnRegistration,
    ) -> Self {
        Self {
            core: TxnCore {
                mode,
                store,
                installed,
                coverage,
                view: TxnView::default(),
                paired: Vec::new(),
                staged: Vec::new(),
                finished: false,
            },
            backend,
            held: Some(HeldTxn {
                _locks: locks,
                registration,
            }),
        }
    }

    // A transaction that failed closed releases its locks and its
    // registration at once.
    fn release_if_finished(&mut self) {
        if self.core.finished {
            self.held = None;
        }
    }
}

#[async_trait]
impl<B, C, P, A, K> SpaceTxn<C, P, A, K> for HostTxn<B, C, P, A, K>
where
    B: TxnBackend<C, P, A, K>,
    C: Clone + Debug + Serialize + Hash + Eq + Send + Sync + 'static,
    P: Clone + Debug + Serialize + Send + Sync + 'static,
    A: Clone + Debug + Serialize + Send + Sync + 'static,
    K: Clone + Debug + Serialize + Send + Sync + 'static,
{
    fn mode(&self) -> SpaceMode { self.core.mode }

    fn data(&mut self, channel: &C) -> Result<&[Datum<A>], SpaceTxnError> {
        self.core.read_data(channel)
    }

    fn continuations(
        &mut self,
        channels: &[C],
    ) -> Result<&[WaitingContinuation<P, K>], SpaceTxnError> {
        self.core.read_continuations(channels)
    }

    fn joins(&mut self, channel: &C) -> Result<&[Vec<C>], SpaceTxnError> {
        self.core.read_joins(channel)
    }

    fn produce_counters(
        &mut self,
        produces: &[Produce],
    ) -> Result<BTreeMap<Produce, i32>, SpaceTxnError> {
        let mut counters = BTreeMap::new();
        for produce in produces {
            let count = self.core.view_counter(&self.backend, produce)?;
            counters.insert(produce.clone(), count);
        }
        Ok(counters)
    }

    fn recorded_comms(&mut self, source: RSpaceOperationSource<'_>) -> Option<Vec<COMM>> {
        match self.core.mode {
            SpaceMode::Play => None,
            SpaceMode::Replay | SpaceMode::Reporting => {
                let key = match source {
                    RSpaceOperationSource::Produce(produce) => IOEvent::Produce(produce.clone()),
                    RSpaceOperationSource::Consume(consume) => IOEvent::Consume(consume.clone()),
                };
                let mut available = Vec::new();
                for (comm, count) in self.backend.recorded(&key) {
                    let recorded = match binding_multiplicity(&key, &comm) {
                        0 => 0,
                        multiplicity => count / multiplicity,
                    };
                    let paired = self
                        .core
                        .paired
                        .iter()
                        .filter(|paired| **paired == comm)
                        .count();
                    for _ in paired..recorded {
                        available.push(comm.clone());
                    }
                }
                available.sort_by_cached_key(|comm| bincode::serialize(comm).unwrap_or_default());
                Some(available)
            }
        }
    }

    fn admits(
        &mut self,
        comm: &COMM,
        datum: &Datum<A>,
        incoming: bool,
    ) -> Result<bool, SpaceTxnError> {
        match self.core.mode {
            SpaceMode::Play => Ok(true),
            SpaceMode::Replay | SpaceMode::Reporting => {
                if !comm.produces.contains(&datum.source) {
                    return Ok(false);
                }
                if datum.persist {
                    return Ok(true);
                }
                let recorded = comm.times_repeated.get(&datum.source).copied().unwrap_or(0);
                let staged = self.core.view_counter(&self.backend, &datum.source)?;
                let expected = match incoming {
                    true => staged
                        .checked_add(1)
                        .ok_or(SpaceTxnError::CounterOverflow)?,
                    false => staged,
                };
                Ok(expected == recorded)
            }
        }
    }

    fn produce(&mut self, op: SpaceProduce<C, P, A, K>) -> Result<StagedOp, SpaceTxnError> {
        let staged = self.core.stage_produce(&self.backend, op);
        self.release_if_finished();
        staged
    }

    fn consume(&mut self, op: SpaceConsume<C, P, A, K>) -> Result<StagedOp, SpaceTxnError> {
        let staged = self.core.stage_consume(&self.backend, op);
        self.release_if_finished();
        staged
    }

    async fn commit(&mut self) -> Result<SpaceTxnReceipt<C, P, A, K>, SpaceTxnError> {
        self.core.ensure_open()?;
        let held = self.held.take().ok_or(SpaceTxnError::Finished)?;
        self.core.finished = true;
        let staged = std::mem::take(&mut self.core.staged);
        // After commit the host counters and recorded bindings are authoritative.
        self.core.paired.clear();
        self.core.view.count_delta.clear();
        let store = Arc::clone(&self.core.store);
        let result = commit_staged(&self.backend, store, &held.registration, staged).await;
        drop(held);
        result
    }
}

/// Records in `channel`'s data leaf at the history root (the outer frame).
pub(crate) fn root_leaf_records<C, P, A, K>(
    reader: &dyn HistoryReader<Blake2b256Hash, C, P, A, K>,
    channel: &C,
) -> Result<u64, SpaceTxnError>
where
    C: Clone + Serialize,
    P: Clone,
    A: Clone,
    K: Clone,
{
    let records = reader
        .get_data_proj_binary(&channel_key(channel)?)
        .map_err(RSpaceError::from)?;
    Ok(records.len() as u64)
}

/// Distinct swept channels by key; the first value given for a key wins.
pub(crate) fn sweep_channels<C: Clone + Serialize>(
    channels: &[C],
) -> Result<BTreeMap<Blake2b256Hash, C>, SpaceTxnError> {
    let mut swept = BTreeMap::new();
    for channel in channels {
        swept
            .entry(channel_key(channel)?)
            .or_insert_with(|| channel.clone());
    }
    Ok(swept)
}

/// The volatile sweep. Validation is read-only: no channel is installed,
/// every stored join group of a swept channel is canonical and is swept as a
/// whole, and no swept key has a leaf at the history root. Then
/// continuations, joins and data are removed through the hot store, so the
/// keys end empty and `changes()` emits deletes of keys absent from the trie
/// (root neutral). `on_continuation_removed` runs once per removed
/// continuation, and the report counts what was actually removed.
pub(crate) fn sweep_volatile_channels<C, P, A, K>(
    store: &dyn HotStore<C, P, A, K>,
    reader: &dyn HistoryReader<Blake2b256Hash, C, P, A, K>,
    installed: &BTreeSet<Blake2b256Hash>,
    channels: &BTreeMap<Blake2b256Hash, C>,
    mut on_continuation_removed: impl FnMut(),
) -> Result<SweepReport, SpaceTxnError>
where
    C: Clone + Serialize + Hash + Eq,
    P: Clone,
    A: Clone,
    K: Clone,
{
    if channels.keys().any(|key| installed.contains(key)) {
        return Err(SpaceTxnError::InstalledChannel);
    }
    let mut groups: BTreeMap<Vec<Blake2b256Hash>, Vec<C>> = BTreeMap::new();
    for (key, channel) in channels {
        groups.insert(vec![key.clone()], vec![channel.clone()]);
        for group in store.get_joins(channel) {
            let group_key = group_identity(&group)?;
            require_canonical(&group_key)?;
            groups.insert(group_key, group);
        }
    }
    if groups
        .keys()
        .any(|group| group.iter().any(|key| !channels.contains_key(key)))
    {
        return Err(SpaceTxnError::SweepCrossesDurable);
    }
    for key in channels.keys() {
        let persisted = !reader
            .get_data_proj_binary(key)
            .map_err(RSpaceError::from)?
            .is_empty() ||
            !reader
                .get_joins_proj_binary(key)
                .map_err(RSpaceError::from)?
                .is_empty();
        if persisted {
            return Err(SpaceTxnError::SweepTouchesPersisted);
        }
    }
    for group_key in groups.keys() {
        let persisted = !reader
            .get_continuations_proj_binary(&hash_from_hashes(group_key))
            .map_err(RSpaceError::from)?
            .is_empty();
        if persisted {
            return Err(SpaceTxnError::SweepTouchesPersisted);
        }
    }

    let mut report = SweepReport {
        channels: channels.len(),
        ..SweepReport::default()
    };
    for group in groups.values() {
        let waiting = store.get_continuations_arc(group).len();
        for index in (0..waiting).rev() {
            let index = i32::try_from(index)
                .map_err(|_| SpaceTxnError::Inconsistent("a continuation index exceeds i32"))?;
            store
                .remove_continuation(group, index)
                .ok_or(SpaceTxnError::Inconsistent("a swept continuation could not be removed"))?;
            report.continuations += 1;
            on_continuation_removed();
        }
    }
    for channel in channels.values() {
        let joined = store.get_joins(channel);
        for group in &joined {
            store.remove_join(channel, group);
        }
        let removed = joined
            .len()
            .checked_sub(store.get_joins(channel).len())
            .ok_or(SpaceTxnError::Inconsistent("the volatile sweep added a join"))?;
        report.joins += removed;
    }
    for channel in channels.values() {
        let records = store.get_data(channel).len();
        for index in (0..records).rev() {
            let index = i32::try_from(index)
                .map_err(|_| SpaceTxnError::Inconsistent("a record index exceeds i32"))?;
            store.remove_datum(channel, index)?;
            report.data += 1;
        }
    }

    let residue = channels
        .values()
        .any(|channel| !store.get_data(channel).is_empty() || !store.get_joins(channel).is_empty()) ||
        groups
            .values()
            .any(|group| !store.get_continuations_arc(group).is_empty());
    match residue {
        true => Err(SpaceTxnError::Inconsistent("the volatile sweep left residue")),
        false => Ok(report),
    }
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;
    use std::task::Poll;

    use super::*;
    use crate::rspace::hashing::stable_hash_provider::{hash, hash_from_vec};

    fn wk(
        channels: &[&str],
        continuation: &str,
        peeks: BTreeSet<i32>,
    ) -> WaitingContinuation<String, String> {
        let channels: Vec<String> = channels.iter().map(|c| c.to_string()).collect();
        let patterns = vec!["_".to_string(); channels.len()];
        WaitingContinuation::create(&channels, &patterns, &continuation.to_string(), false, peeks)
    }

    fn parent(prefix: Vec<(u64, u64)>) -> OperationOrder {
        OperationOrder {
            session: [7; 32],
            path: CausalPath::from(prefix),
        }
    }

    fn names(values: &[&str]) -> Vec<String> { values.iter().map(|v| v.to_string()).collect() }

    // The channels of `values` sorted into canonical order.
    fn canonical(values: &[&str]) -> Vec<String> {
        let mut channels = names(values);
        channels.sort_by_key(hash);
        channels
    }

    // A meter that counts the reserved operations, scanned bytes and backing
    // bytes, and that rejects once the operations pass `limit`.
    fn counting_meter(
        counts: &std::cell::Cell<(usize, usize, usize)>,
        limit: usize,
    ) -> impl Fn(usize, usize, usize) -> Result<(), RSpaceError> + '_ {
        move |operations, scanned, backing| {
            let (o, s, b) = counts.get();
            let next = (o + operations, s + scanned, b + backing);
            if next.0 > limit {
                return Err(RSpaceError::HostWorkRejected);
            }
            counts.set(next);
            Ok(())
        }
    }

    fn play_indices<D: CandidateSource + Serialize>(values: Vec<D>) -> Vec<usize> {
        candidate_order::canonical_candidates(values)
            .into_iter()
            .map(|(_, index)| index as usize)
            .collect()
    }

    proptest::proptest! {
        // DR-75 (decision 41): the metered order of a space agent is the
        // order of play, also with tie runs of equal source hashes.
        #[test]
        fn canonical_data_order_is_the_order_of_play(
            data in candidate_order::candidate_strategy()
        ) {
            let counts = std::cell::Cell::new((0, 0, 0));
            let order = canonical_data_order(&data, &counting_meter(&counts, usize::MAX))
                .expect("an unlimited meter accepts");
            proptest::prop_assert_eq!(order, play_indices(data.clone()));
            // A cut one operation below the usage rejects, and the exact usage
            // accepts, so every reservation precedes the result.
            let used = counts.get().0;
            let exact = std::cell::Cell::new((0, 0, 0));
            proptest::prop_assert!(
                canonical_data_order(&data, &counting_meter(&exact, used)).is_ok()
            );
            proptest::prop_assert_eq!(exact.get(), counts.get());
            if used > 0 {
                let cut = std::cell::Cell::new((0, 0, 0));
                proptest::prop_assert_eq!(
                    canonical_data_order(&data, &counting_meter(&cut, used - 1)),
                    Err(RSpaceError::HostWorkRejected)
                );
            }
        }
    }

    #[test]
    fn canonical_continuation_order_is_the_order_of_play() {
        let waiters = vec![
            wk(&["a"], "k3", BTreeSet::new()),
            wk(&["a"], "k1", BTreeSet::new()),
            wk(&["a"], "k2", BTreeSet::from([0])),
        ];
        let counts = std::cell::Cell::new((0, 0, 0));
        let order = canonical_continuation_order(&waiters, &counting_meter(&counts, usize::MAX))
            .expect("an unlimited meter accepts");
        assert_eq!(order, play_indices(waiters.clone()));
        assert!(counts.get().0 > 0, "the order reserves its buffers and its sort");
        assert_eq!(
            canonical_continuation_order(
                &[] as &[WaitingContinuation<String, String>],
                &|_, _, _| { Ok(()) }
            ),
            Ok(Vec::new())
        );
    }

    #[test]
    fn metered_hash_is_the_hash_of_the_bincode_bytes() {
        let value = (String::from("rrs-set-key"), vec![1u8, 2, 3], 7i64);
        let counts = std::cell::Cell::new((0, 0, 0));
        let hashed = metered_hash(&value, &counting_meter(&counts, usize::MAX))
            .expect("an unlimited meter accepts");
        let bytes = bincode::serialize(&value).expect("the value encodes");
        assert_eq!(hashed, Blake2b256Hash::new(&bytes));
        assert!(counts.get().1 >= bytes.len(), "the hash reserves the bytes that it reads");
    }

    // DR-75 (decision 41): each primitive of the space order reserves what the
    // branch's `MeteredOrder` and its helpers `buffer` and `sort` reserve
    // (`native_candidate/metered.rs`). Both implement the one `OrderWork` of
    // `candidate_order::canonical_order`, so the order of a space agent costs
    // what the order of metered native replay costs.
    #[test]
    fn space_order_primitives_reserve_the_restated_formula() {
        type View<'v> = Cow<'v, Datum<String>>;
        let recorded = std::cell::RefCell::new(Vec::new());
        let recorder = |operations: usize, scanned: usize, backing: usize| {
            recorded.borrow_mut().push((operations, scanned, backing));
            Ok(())
        };
        let order = SpaceOrder(&recorder);
        // A buffer of `length` values: `length + 1` operations and
        // `length * size` bytes, scanned and backed.
        let buffered: Vec<(u64, u32)> =
            <SpaceOrder<'_> as OrderWork<View<'_>>>::buffer(&order, 5).expect("a buffer");
        assert!(buffered.capacity() >= 5);
        let bytes = 5 * size_of::<(u64, u32)>();
        assert_eq!(recorded.take(), vec![(6, bytes, bytes)]);
        // A sort: one reservation of the moves of every value, then one for
        // each comparison, which reads the moves of two values and the bytes
        // that `scanned` gives for each.
        let mut values = vec![5_u64, 3, 9, 1, 7];
        let comparisons = std::cell::Cell::new(0_usize);
        <SpaceOrder<'_> as OrderWork<View<'_>>>::sort(
            &order,
            &mut values,
            |left, right| {
                comparisons.set(comparisons.get() + 1);
                left.cmp(right)
            },
            |_| 32,
        )
        .expect("a sort");
        assert_eq!(values, vec![1, 3, 5, 7, 9]);
        let moves = 2 * size_of::<u64>();
        let mut expected = vec![(5, 5 * moves, 0)];
        expected.extend(std::iter::repeat_n((1, moves + 64, 0), comparisons.get()));
        assert!(comparisons.get() > 0);
        assert_eq!(recorded.take(), expected);
        // A scan: one comparison of two hashes.
        let (left, right) = (Blake2b256Hash::new(b"left"), Blake2b256Hash::new(b"right"));
        <SpaceOrder<'_> as OrderWork<View<'_>>>::scan(&order, &left, &right).expect("a scan");
        assert_eq!(recorded.take(), vec![(1, left.0.len() + right.0.len(), 0)]);
        // A digest: the inspect of the value, then the metered hash of its
        // bincode bytes.
        let datum: View<'_> =
            Cow::Owned(Datum::create(&"channel".to_string(), "payload".to_string(), false));
        let digest = order.digest(&datum).expect("a digest");
        let digested = recorded.take();
        let reference = std::cell::RefCell::new(Vec::new());
        let reference_meter = |operations: usize, scanned: usize, backing: usize| {
            reference.borrow_mut().push((operations, scanned, backing));
            Ok(())
        };
        crate::rspace::native_backing::inspect(&datum, &reference_meter).expect("an inspect");
        let expected_digest =
            native_source::hash(&datum, &reference_meter).expect("a metered hash");
        assert_eq!(digest, expected_digest);
        assert_eq!(digested, reference.take());
        assert!(!digested.is_empty());
    }

    // A sub-order sorts after its parent operation and before the parent's
    // next step, and consecutive sub-orders are increasing.
    #[test]
    fn sub_order_sorts_between_parent_and_next_step() {
        let participant = vec![(3, 1), (0, 0)];
        let mut operation = participant.clone();
        operation.push((5, 0));
        let mut next_step = participant.clone();
        next_step.push((6, 0));
        let mut next_split = participant;
        next_split.push((6, 1));
        let operation = parent(operation);
        let mut previous = operation.clone();
        for index in 0..8 {
            let sub = sub_order(&operation, index);
            assert_eq!(previous.cmp(&sub), Ordering::Less, "sub-order {}", index);
            assert_eq!(sub.cmp(&parent(next_step.clone())), Ordering::Less);
            assert_eq!(sub.cmp(&parent(next_split.clone())), Ordering::Less);
            assert_eq!(sub.session, operation.session);
            assert_eq!(sub.path.len(), operation.path.len() + 1);
            previous = sub;
        }
    }

    // No ReductionContext order (tags 0 and 1 only) equals a sub-order: the
    // last segment of a sub-order carries tag 2.
    #[test]
    fn sub_order_never_equals_a_reduction_path() {
        let operation = parent(vec![(1, 0), (2, 1), (0, 0), (4, 0)]);
        for index in 0..4u64 {
            let sub = sub_order(&operation, index as usize);
            let segments = sub.path.to_vec();
            assert_eq!(segments.last(), Some(&(index, SUB_OPERATION_TAG)));
            for tag in [0u64, 1u64] {
                let mut reduction = operation.path.to_vec();
                reduction.push((index, tag));
                assert_ne!(sub, parent(reduction));
            }
            assert_ne!(sub, operation);
        }
    }

    // The view deduplicates continuations by the hot store's Debug identity,
    // which ignores the consume source.
    #[test]
    fn view_put_continuation_deduplicates_by_debug_identity() {
        let mut view: TxnView<String, String, String, String> = TxnView::default();
        let group = names(&["a"]);
        let key = group_identity(&group).expect("group key");
        view.conts.insert(key.clone(), ContinuationEntry {
            waiting: Vec::new(),
        });
        let first = wk(&["a"], "k", BTreeSet::new());
        assert_eq!(view.put_continuation(&key, first.clone()), Ok(true));
        let mut same_identity = first.clone();
        same_identity.source = Consume::default();
        assert_eq!(view.put_continuation(&key, same_identity), Ok(false));
        let peeking = wk(&["a"], "k", BTreeSet::from([0]));
        assert_eq!(view.put_continuation(&key, peeking.clone()), Ok(true));
        assert_eq!(
            view.conts.get(&key).map(|entry| entry.waiting.clone()),
            Some(vec![peeking, first])
        );
    }

    // A join entry is removed only when its continuation list is empty.
    #[test]
    fn view_remove_join_requires_empty_continuations() {
        let mut view: TxnView<String, String, String, String> = TxnView::default();
        let group = canonical(&["a", "b"]);
        let group_key = group_identity(&group).expect("group key");
        let a = channel_key(&"a".to_string()).expect("key a");
        let b = channel_key(&"b".to_string()).expect("key b");
        let waiting = wk(&["a", "b"], "k", BTreeSet::new());
        view.conts.insert(group_key.clone(), ContinuationEntry {
            waiting: vec![waiting.clone()],
        });
        view.joins.insert(a.clone(), JoinEntry {
            groups: vec![group.clone()],
            keys: vec![group_key.clone()],
        });
        assert_eq!(view.remove_join(&a, &group_key), Ok(false));
        assert_eq!(view.joins.get(&a).map(|entry| entry.groups.clone()), Some(vec![group.clone()]));
        assert_eq!(view.remove_continuation(&group_key, &waiting), Ok(0));
        assert_eq!(view.remove_join(&a, &group_key), Ok(true));
        assert_eq!(view.joins.get(&a).map(|entry| entry.groups.len()), Some(0));
        assert_eq!(view.remove_join(&a, &group_key), Ok(false));
        assert_eq!(
            view.remove_join(&b, &group_key),
            Err(SpaceTxnError::Inconsistent("staged joins are not loaded"))
        );
    }

    // Negative control: a stored datum without its Produce event is rejected.
    #[test]
    fn correlation_rejects_datum_without_produce_event() {
        let source = Produce::create(&"a", &"x", false);
        let effects = vec![StoreEffect::PutDatum(source.clone())];
        assert_eq!(
            check_event_correlation(3, &effects, &[]),
            Err(SpaceTxnError::EventCoverage {
                op: 3,
                rule: "stored datum without its produce event",
            })
        );
        let events = vec![Event::IoEvent(IOEvent::Produce(source))];
        assert_eq!(check_event_correlation(3, &effects, &events), Ok(()));
    }

    // Negative control: a COMM logged before its trigger event is rejected.
    #[test]
    fn correlation_rejects_comm_before_trigger() {
        let channels = vec!["a".to_string()];
        let consume = Consume::create(&channels, &vec!["_".to_string()], &"k".to_string(), false);
        let produce = Produce::create(&"a", &"x", false);
        let comm = COMM {
            consume: consume.clone(),
            produces: vec![produce.clone()],
            peeks: BTreeSet::new(),
            times_repeated: BTreeMap::from([(produce.clone(), 1)]),
        };
        let effects = vec![StoreEffect::RemoveDatum(produce)];
        let early =
            vec![Event::Comm(comm.clone()), Event::IoEvent(IOEvent::Consume(consume.clone()))];
        assert_eq!(
            check_event_correlation(0, &effects, &early),
            Err(SpaceTxnError::EventCoverage {
                op: 0,
                rule: "COMM precedes its trigger event",
            })
        );
        let ordered = vec![Event::IoEvent(IOEvent::Consume(consume)), Event::Comm(comm)];
        assert_eq!(check_event_correlation(0, &effects, &ordered), Ok(()));
    }

    // Peeks are either empty or cover every bind; anything else is a mixed
    // arrow join.
    #[test]
    fn peeks_rule_accepts_only_empty_or_all_binds() {
        assert_eq!(check_peeks(2, &BTreeSet::new()), Ok(()));
        assert_eq!(check_peeks(2, &BTreeSet::from([0, 1])), Ok(()));
        assert_eq!(check_peeks(1, &BTreeSet::from([0])), Ok(()));
        for peeks in [BTreeSet::from([0]), BTreeSet::from([1]), BTreeSet::from([0, 2])] {
            assert_eq!(
                check_peeks(2, &peeks),
                Err(SpaceTxnError::MixedArrowJoin {
                    binds: 2,
                    peeks: peeks.clone()
                })
            );
        }
    }

    // The store identity is `stable_hash_provider::hash`, and the key of a
    // canonical group addresses the history leaf that `hash_from_vec`
    // addresses.
    #[test]
    fn space_store_handle_channel_key_is_stable_hash() {
        for channel in ["a", "b", "v1", "", "a longer channel name"] {
            let channel = channel.to_string();
            assert_eq!(channel_key(&channel), Ok(hash(&channel)));
        }
        let group = canonical(&["a", "b", "c", "d"]);
        let key = canonical_group_key(&group).expect("a canonical group");
        assert_eq!(hash_from_hashes(&key), hash_from_vec(&group));
        assert!(key.windows(2).all(|pair| pair[0] < pair[1]));
    }

    // FR-1': a sorted group is canonical, also when it repeats a channel;
    // every other order, a repeat out of order included, is rejected. One
    // channel is always canonical.
    #[test]
    fn space_store_handle_canonical_group_rules() {
        let sorted = canonical(&["a", "b", "c"]);
        assert!(canonical_group_key(&sorted).is_ok());
        assert!(canonical_group_key(&names(&["a"])).is_ok());
        let mut reversed = sorted.clone();
        reversed.reverse();
        assert_eq!(canonical_group_key(&reversed), Err(SpaceTxnError::NonCanonicalChannelOrder));
        let mut rotated = sorted.clone();
        rotated.rotate_left(1);
        assert_eq!(canonical_group_key(&rotated), Err(SpaceTxnError::NonCanonicalChannelOrder));
        for repeats in [
            vec![sorted[0].clone(), sorted[0].clone()],
            vec![sorted[0].clone(), sorted[0].clone(), sorted[1].clone()],
            vec![sorted[0].clone(), sorted[1].clone(), sorted[1].clone(), sorted[2].clone()],
            vec![sorted[2].clone(), sorted[2].clone(), sorted[2].clone()],
        ] {
            let key = canonical_group_key(&repeats).expect("a sorted group with a repeat");
            assert_eq!(key.len(), repeats.len());
            assert!(key.windows(2).all(|pair| pair[0] <= pair[1]));
        }
        let unsorted_repeat = vec![sorted[0].clone(), sorted[1].clone(), sorted[0].clone()];
        assert_eq!(
            canonical_group_key(&unsorted_repeat),
            Err(SpaceTxnError::NonCanonicalChannelOrder)
        );
        let repeat_after_larger = vec![sorted[1].clone(), sorted[0].clone(), sorted[0].clone()];
        assert_eq!(
            canonical_group_key(&repeat_after_larger),
            Err(SpaceTxnError::NonCanonicalChannelOrder)
        );
        assert_eq!(
            canonical_group_key::<String>(&[]),
            Err(SpaceTxnError::InvalidOperation("channels must not be empty"))
        );
    }

    // All permutations of `values`, with repeats when `values` repeats.
    fn permutations(values: &[String]) -> Vec<Vec<String>> {
        match values.len() {
            0 => vec![Vec::new()],
            length => {
                let mut all = Vec::new();
                for index in 0..length {
                    let mut rest = values.to_vec();
                    let first = rest.remove(index);
                    for mut tail in permutations(&rest) {
                        tail.insert(0, first.clone());
                        all.push(tail);
                    }
                }
                all
            }
        }
    }

    // FR-1': of all orders of a channel multiset, exactly one is canonical,
    // and it is the multiset sorted by channel hash. So the hot store never
    // holds one multiset under two keys.
    #[test]
    fn space_store_handle_sorted_order_is_the_unique_canonical_order() {
        let multisets = [
            names(&["a"]),
            names(&["a", "a"]),
            names(&["a", "b"]),
            names(&["a", "a", "b"]),
            names(&["a", "b", "b"]),
            names(&["a", "b", "c"]),
            names(&["a", "a", "a"]),
            names(&["a", "a", "b", "b"]),
            names(&["a", "b", "c", "c"]),
            names(&["d", "c", "b", "a"]),
        ];
        for multiset in multisets {
            let orders: BTreeSet<Vec<String>> = permutations(&multiset).into_iter().collect();
            let accepted: Vec<&Vec<String>> = orders
                .iter()
                .filter(|order| canonical_group_key(order).is_ok())
                .collect();
            let mut sorted = multiset.clone();
            sorted.sort_by_key(hash);
            assert_eq!(accepted, vec![&sorted], "multiset {:?}", multiset);
            for order in orders.iter().filter(|order| **order != sorted) {
                assert_eq!(
                    canonical_group_key(order),
                    Err(SpaceTxnError::NonCanonicalChannelOrder),
                    "order {:?}",
                    order
                );
            }
        }
    }

    // The coverage stripes are the stripes `acquire_locks` takes. While
    // the lock of "a" is held, a channel blocks on it exactly when the
    // coverage holds the channel's stripe, including another channel that
    // shares the stripe of "a".
    #[tokio::test]
    async fn space_store_handle_lock_coverage_matches_acquired_stripes() {
        let stripes = striped_locks::new_striped_locks();
        let keys = names(&["a"]);
        let hashes: Vec<u64> = keys.iter().map(striped_locks::channel_hash).collect();
        let coverage = LockCoverage::new(&hashes, stripes.len(), stripes.len());
        let _held = striped_locks::acquire_locks(&stripes, &hashes).await;
        let shares_stripe = |channel: &String| {
            (striped_locks::channel_hash(channel) as usize) % stripes.len() ==
                (hashes[0] as usize) % stripes.len()
        };
        let sibling = (0..1_000_000)
            .map(|index| format!("probe-{}", index))
            .find(|channel| shares_stripe(channel))
            .expect("a channel that shares the stripe of a");
        let mut probes: Vec<String> = (0..64).map(|index| format!("channel-{}", index)).collect();
        probes.push(keys[0].clone());
        probes.push(sibling.clone());
        let mut covered = Vec::with_capacity(probes.len());
        for channel in &probes {
            let probe_hashes = [striped_locks::channel_hash(channel)];
            let mut acquire = Box::pin(striped_locks::acquire_locks(&stripes, &probe_hashes));
            let blocked = matches!(futures::poll!(&mut acquire), Poll::Pending);
            let held = coverage.require(channel, LockUse::Consumed).is_ok();
            assert_eq!(blocked, held, "channel {}", channel);
            if held {
                covered.push(channel.clone());
            }
        }
        assert!(covered.contains(&keys[0]) && covered.contains(&sibling));
        assert!(covered.len() < probes.len());
        for lock_use in [LockUse::Produced, LockUse::Joined, LockUse::Consumed] {
            assert_eq!(coverage.require(&sibling, lock_use), Ok(()));
        }
        let empty = LockCoverage::new(&[], stripes.len(), stripes.len());
        assert_eq!(empty.require(&keys[0], LockUse::Produced), Err(SpaceTxnError::UnlockedChannel));
        let unstriped = LockCoverage::new(&hashes, 0, 0);
        assert_eq!(
            unstriped.require(&keys[0], LockUse::Joined),
            Err(SpaceTxnError::UnlockedChannel)
        );
    }

    fn store_identity(store: &Arc<String>) -> StoreIdentity {
        let weak = Arc::downgrade(store);
        let store: StoreIdentity = weak;
        store
    }

    // The parent set admits one commit per path in a session and resets on
    // a new session.
    #[test]
    fn space_store_handle_committed_parents_track_one_session() {
        let store = Arc::new(String::from("store"));
        let mut parents = CommittedParents::default();
        let first = parent(vec![(0, 0)]);
        let second = parent(vec![(1, 0)]);
        assert_eq!(parents.admit(&first, store_identity(&store)), Ok(()));
        assert_eq!(parents.admit(&second, store_identity(&store)), Ok(()));
        assert_eq!(
            parents.admit(&first, store_identity(&store)),
            Err(SpaceTxnError::ParentOrderReused)
        );
        let other_session = OperationOrder {
            session: [8; 32],
            path: first.path.clone(),
        };
        assert_eq!(parents.admit(&other_session, store_identity(&store)), Ok(()));
        assert_eq!(parents.paths.len(), 1);
        assert_eq!(parents.admit(&first, store_identity(&store)), Ok(()));
        assert_eq!(parents.admit(&second, store_identity(&store)), Ok(()));
    }

    // FR-5': the parent set also resets at the start of a root evaluation
    // and when a commit runs against another hot store; without either, a
    // second commit under one parent fails. A dropped store whose identity
    // the set still records cannot be mistaken for a new store, because the
    // weak reference keeps its allocation.
    #[test]
    fn space_store_handle_committed_parents_reset_by_scope_and_store() {
        let first_store = Arc::new(String::from("first store"));
        let mut parents = CommittedParents::default();
        let order = parent(vec![(2, 0)]);
        assert_eq!(parents.admit(&order, store_identity(&first_store)), Ok(()));
        assert_eq!(
            parents.admit(&order, store_identity(&first_store)),
            Err(SpaceTxnError::ParentOrderReused)
        );
        parents.reset();
        assert_eq!(parents.paths.len(), 0);
        assert_eq!(parents.admit(&order, store_identity(&first_store)), Ok(()));

        let second_store = Arc::new(String::from("second store"));
        assert_eq!(parents.admit(&order, store_identity(&second_store)), Ok(()));
        assert_eq!(
            parents.admit(&order, store_identity(&second_store)),
            Err(SpaceTxnError::ParentOrderReused)
        );
        assert_eq!(parents.admit(&order, store_identity(&first_store)), Ok(()));

        let recorded = store_identity(&first_store);
        drop(first_store);
        let third_store = Arc::new(String::from("third store"));
        assert!(!Weak::ptr_eq(&recorded, &store_identity(&third_store)));
        assert_eq!(parents.admit(&order, recorded), Err(SpaceTxnError::ParentOrderReused));
        assert_eq!(parents.admit(&order, store_identity(&third_store)), Ok(()));
    }

    // One context holds at most one registration; dropping it frees the
    // context, and another task registers on its own.
    #[tokio::test]
    async fn space_store_handle_registration_refuses_nested_context() {
        let state = Arc::new(HandleState::default());
        let first = TxnRegistration::register(&state).expect("register");
        assert!(matches!(TxnRegistration::register(&state), Err(SpaceTxnError::NestedTransaction)));
        let spawned_state = Arc::clone(&state);
        let other_task =
            tokio::spawn(async move { TxnRegistration::register(&spawned_state).map(drop) })
                .await
                .expect("join the other task");
        assert_eq!(other_task, Ok(()));
        drop(first);
        assert_eq!(state.open.lock().expect("open transactions lock").len(), 0);
        let again = TxnRegistration::register(&state).expect("register after the drop");
        drop(again);
    }

    fn record(channel: &str, value: &str, persist: bool) -> Datum<String> {
        Datum {
            a: value.to_string(),
            persist,
            source: Produce::create(&channel.to_string(), &value.to_string(), persist),
        }
    }

    // The staged view and the commit locate bound records with one rule: the
    // first byte-equal record that no earlier non-persistent candidate
    // claimed, per channel. A persistent record can bind twice; the incoming
    // candidate has no position (FR-1', FR-7').
    #[test]
    fn space_store_handle_claim_positions_skip_claimed_records() {
        let a = channel_key(&"a".to_string()).expect("key a");
        let b = channel_key(&"b".to_string()).expect("key b");
        let x = record("a", "x", false);
        let y = record("a", "y", false);
        let p = record("a", "p", true);
        let z = record("b", "z", false);
        let lists: BTreeMap<Blake2b256Hash, Vec<Datum<String>>> = BTreeMap::from([
            (a.clone(), vec![y.clone(), x.clone(), p.clone(), x.clone()]),
            (b.clone(), vec![z.clone()]),
        ]);
        let bind = |channel: &str, stored: &Datum<String>, index: i32| {
            bind_candidate(channel.to_string(), stored, stored.a.clone(), index)
        };
        let incoming = incoming_candidate(
            "b".to_string(),
            Produce::create(&"b".to_string(), &"q".to_string(), false),
            "q".to_string(),
            false,
            "q".to_string(),
        );
        let candidates = vec![
            bind("a", &x, 3),
            bind("a", &x, 1),
            bind("a", &p, 2),
            bind("a", &p, 2),
            incoming,
            bind("b", &z, 0),
        ];
        let keys = vec![a.clone(), a.clone(), a.clone(), a.clone(), b.clone(), b.clone()];
        let absent = SpaceTxnError::Inconsistent("absent");
        let positions = claim_positions(
            &candidates,
            &keys,
            |key| lists.get(key).map(Vec::as_slice),
            absent.clone(),
        )
        .expect("claim the positions");
        assert_eq!(positions, vec![Some(1), Some(3), Some(2), Some(2), None, Some(0)]);
        assert_eq!(removed_records(&candidates, &positions), vec![(0, 1), (1, 3), (5, 0)]);

        let third_x = vec![bind("a", &x, 1), bind("a", &x, 3), bind("a", &x, 1)];
        assert_eq!(
            claim_positions(
                &third_x,
                &keys[..3],
                |key| lists.get(key).map(Vec::as_slice),
                absent.clone()
            ),
            Err(absent)
        );
    }

    // Decision 45 (Rule E): the receipt of a fire keeps the flag of the host's
    // `wrap_result` for every result, also for a peek fire, because the fire
    // removes its stored non-persistent records as the host does. Results
    // that do not line up with the candidates fail closed.
    #[test]
    fn space_store_handle_receipt_keeps_the_host_flags() {
        let candidates = [
            bind_candidate("a".to_string(), &record("a", "x", false), "x".to_string(), 0),
            incoming_candidate(
                "b".to_string(),
                Produce::create(&"b".to_string(), &"y".to_string(), false),
                "y".to_string(),
                false,
                "y".to_string(),
            ),
            bind_candidate("c".to_string(), &record("c", "p", true), "p".to_string(), 1),
        ];
        let continuation = ContResult {
            continuation: "k".to_string(),
            persistent: false,
            channels: names(&["a", "b", "c"]),
            patterns: names(&["_", "_", "_"]),
            peek: true,
        };
        let results: Vec<RSpaceResult<String, String>> = candidates
            .iter()
            .map(|candidate| RSpaceResult {
                channel: candidate.channel.clone(),
                matched_datum: candidate.datum.a.clone(),
                removed_datum: candidate.removed_datum.clone(),
                persistent: candidate.datum.persist,
            })
            .collect();
        let comm = COMM {
            consume: Consume::create(
                &names(&["a", "b", "c"]),
                &names(&["_", "_", "_"]),
                &"k".to_string(),
                false,
            ),
            produces: candidates
                .iter()
                .map(|candidate| candidate.datum.source.clone())
                .collect(),
            peeks: BTreeSet::from([0, 1, 2]),
            times_repeated: BTreeMap::new(),
        };
        let flags = |fired: usize| -> Result<Vec<bool>, SpaceTxnError> {
            let wrapped = Some((continuation.clone(), results.clone()));
            match fired_receipt::<String, String, String, String>(
                comm.clone(),
                wrapped,
                fired,
                None,
            )? {
                SpaceOpReceipt::Fired { results, .. } => {
                    Ok(results.iter().map(|result| result.persistent).collect())
                }
                _ => Err(SpaceTxnError::Inconsistent("not a fire receipt")),
            }
        };
        assert_eq!(flags(candidates.len()), Ok(vec![false, false, true]));
        assert_eq!(
            flags(2),
            Err(SpaceTxnError::Inconsistent(
                "the host fire results do not line up with the candidates"
            ))
        );
        assert_eq!(
            fired_receipt::<String, String, String, String>(comm, None, 0, None),
            Err(SpaceTxnError::Inconsistent("the host returned no fire result"))
        );
    }
}
