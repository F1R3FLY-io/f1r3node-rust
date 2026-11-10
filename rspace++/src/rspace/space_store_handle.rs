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
// Reads. A transaction reads through the store of its host (`TxnStore`):
// the hot store in play, replay and reporting, and the digest-keyed store of
// the native replay session in native replay (DR-96). Each read gives the
// value that the host reads, and a value that the store does not hold is
// read at the history root. A space operation meters only what its host
// operation meters: its COMMs and the bytes that it stores, through the
// observer calls of the host (DR-131).
//
// Module layout:
//   space_store_handle.rs          — public API, staged view, staging rules,
//                                    commit loop, event correlation, sweep
//   rspace/space_play.rs           — `SpaceTxnHost` for `RSpace` (play)
//   replay_rspace/space_replay.rs  — `SpaceTxnHost` for `ReplayRSpace` and
//                                    `ReportingRspace` (replay, reporting)
//   replay_rspace/native_session/space.rs
//                                  — `SpaceTxnHost` for the native replay
//                                    session (native replay)

use std::any::Any;
use std::borrow::{Borrow, Cow};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::{self, Debug};
use std::hash::Hash;
use std::sync::{Arc, Mutex, Weak};
use std::thread::ThreadId;

use async_trait::async_trait;
use serde::Serialize;

use crate::rspace::candidate_order::{self, CandidateSource};
use crate::rspace::errors::RSpaceError;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::hashing::stable_hash_provider::hash_from_hashes;
use crate::rspace::history::history_reader::HistoryReader;
use crate::rspace::hot_store::HotStore;
use crate::rspace::internal::{ConsumeCandidate, Datum, Install, WaitingContinuation};
use crate::rspace::r#match::Match;
// Changed by decision 123 (plan v4, V3-5): the play item test of a row
// search.
// use crate::rspace::operation_context::{self, CausalPath, OperationOrder};
use crate::rspace::operation_context::{self, CausalPath, MatchAdmission, OperationOrder};
// Added by decision 102 (decision 119 S2): the outcome of a recorded row.
// Added by decision 123 (plan v4, V3-5): the recorded identity and the
// replay item test of a row search.
use crate::rspace::replay_rspace::native_epoch::{
    NativeCandidateIdentity, NativeOwnerFilter, NativeReplayOutcome,
};
// Added by decision 123 (plan v4, V3-8, R7): the COMM build of a produce fire.
use crate::rspace::rspace::produce_comm_with;
use crate::rspace::rspace_interface::{
    ContResult, MaybeConsumeResult, RSpaceOperationCompletion, RSpaceOperationSource, RSpaceResult,
};
// Added by decision 123 (plan v4, V3-5): the shared search loops of a row.
use crate::rspace::space_matcher::{MatchCandidates, MatchGroup, TupleRequest};
use crate::rspace::striped_locks::{self, ChannelLockGuard};
use crate::rspace::trace::event::{COMM, Consume, Event, IOEvent, Produce};

/// Added by decision 102 (decision 119 S2): a recorded row of native replay,
/// as `NativeOperationEpoch::space_recorded_row` gives it: the journal index
/// of the row, its outcome and its COMM.
pub(crate) type RecordedRow = (usize, NativeReplayOutcome, Option<COMM>);

/// Execution mode of a [`SpaceTxnHost`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpaceMode {
    Play,
    // Retired by decision 90 (C6, step D): no production path reaches the
    // rigged replay modes. Outside native execution every space entry point
    // fails closed (`ReifiedRuntime::host`), and native replay runs through
    // its session. The text before:
    // Replay,
    // Reporting,
    /// Native replay through the native replay session (decision 47). The
    /// session has no rigged log: each staged operation meets its recorded
    /// journal row at commit. Staging therefore pairs nothing and admits every
    /// candidate, as in play.
    NativeReplay,
}

/// Typed failure of a space transaction. Host and observer errors pass
/// through unchanged in [`SpaceTxnError::Host`].
#[derive(Clone, Debug, PartialEq)]
pub enum SpaceTxnError {
    /// Observer or host error, unchanged (`OutOfPhlogistons`,
    /// `SignedLimitExhausted`, ...).
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
    ///
    /// Changed by decision 94: the channel key is not one of the keys of
    /// phase B ([`SpaceTxn::lock_phase_b`]). Before phase B, every channel is
    /// unlocked.
    UnlockedChannel,
    /// A transaction already committed under the current parent operation
    /// order.
    ParentOrderReused,
    /// A produce counter would exceed `i32::MAX`.
    CounterOverflow,
    /// The current task already holds an open transaction of this handle.
    NestedTransaction,
    /// Added by decision 90 (DR-132): in native replay, a recorded row of a
    /// staged operation is not ready, at the replay state version `seen`.
    /// The commit applies nothing. The job releases its stripes, waits for a
    /// change after `seen` ([`SpaceStoreHandle::wait_changed`]) and runs
    /// again from the start (`NativeReplayStall.tla`, the transaction
    /// actions).
    NotReady { seen: u64 },
    /// Added by decision 102 (decision 119 S1, rule CR-6) and decision 124
    /// (L11): in native replay, the source of a stage call has no successor
    /// row on the chain of the footprint key. The call stages nothing, and the
    /// staging ends. The commit of the followed prefix needs CanStop of its
    /// last row (CR-18). Otherwise it fails with the session mismatch.
    RecordEnded,
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
            SpaceTxnError::NotReady { seen } => {
                write!(f, "A recorded row of the space transaction is not ready at version {seen}")
            }
            // Added by decision 102 (decision 119 S1, rule CR-6).
            SpaceTxnError::RecordEnded => {
                write!(f, "The recorded rows of the space transaction end before this operation")
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
    /// Changed by decision 103 (decision 118, U3 (i) of decision 122): a
    /// persistent incoming datum can be two or more candidates, each on the
    /// channel of the produce (CR-11).
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
    /// Added by decision 92: true when the operation is its job's own, so its
    /// source is unique to the job. The random state of its record is the
    /// random state of the job or a purpose derivation of it. A transaction
    /// with a footprint key commits only when its first operation is its own
    /// ([`SpaceTxn::set_footprint_key`]).
    pub own: bool,
    /// Added by decision 123 (plan v4, V3-5, V3-9): the search of the row at
    /// its commit ([`RowSearch`]). The commit runs it only for a staged fire.
    /// None commits the staged outcome.
    pub search: Option<Box<dyn RowSearch<C, P, A, K>>>,
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
    /// Added by decision 92: true when the operation is its job's own, so its
    /// source is unique to the job. The random state of its continuation is
    /// the random state of the job or a purpose derivation of it
    /// ([`SpaceProduce::own`]).
    pub own: bool,
    /// Added by decision 123 (plan v4, V3-5, V3-9): the search of the row at
    /// its commit ([`SpaceProduce::search`]).
    pub search: Option<Box<dyn RowSearch<C, P, A, K>>>,
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

/// Added by decision 123 (plan v4, V3-5): one run of a row order, a group and
/// the candidates of that group that follow each other (N5).
#[derive(Clone, Debug)]
pub struct RowRun<C> {
    /// The channels of the group.
    pub channels: Vec<C>,
    /// The positions of the candidates of the run in the row order.
    pub candidates: std::ops::Range<usize>,
}

/// Added by decision 123 (plan v4, V3-5, V3-11): the reads of a row search at
/// commit. `RowCandidates` builds the closures over the store of the commit.
/// Each read checks the lock coverage and the installed guard first (CR-36).
pub struct RowLists<'r, C, P: Clone, A: Clone, K: Clone> {
    data: &'r dyn Fn(&C) -> Result<Vec<Datum<A>>, SpaceTxnError>,
    continuations: &'r dyn Fn(&[C]) -> Result<Vec<Arc<WaitingContinuation<P, K>>>, SpaceTxnError>,
    joins: &'r dyn Fn(&C) -> Result<Vec<Vec<C>>, SpaceTxnError>,
    named: Option<&'r dyn NativeCandidateIdentity>,
    counter: &'r dyn Fn(&Produce) -> i32,
}

impl<'r, C, P: Clone, A: Clone, K: Clone> RowLists<'r, C, P, A, K> {
    /// The lists over the reads `data`, `continuations` and `joins` of the
    /// commit ([`RowReads`]), the recorded identity `named` of the row (None in
    /// play) and the produce counter `counter` of the commit. It is public
    /// because the row searches are in another crate, and their unit tests
    /// build lists with a recorded identity.
    pub fn new(
        data: &'r dyn Fn(&C) -> Result<Vec<Datum<A>>, SpaceTxnError>,
        continuations: &'r dyn Fn(
            &[C],
        )
            -> Result<Vec<Arc<WaitingContinuation<P, K>>>, SpaceTxnError>,
        joins: &'r dyn Fn(&C) -> Result<Vec<Vec<C>>, SpaceTxnError>,
        named: Option<&'r dyn NativeCandidateIdentity>,
        counter: &'r dyn Fn(&Produce) -> i32,
    ) -> Self {
        Self {
            data,
            continuations,
            joins,
            named,
            counter,
        }
    }

    /// The data of `channel` in the store of the commit.
    pub fn data(&self, channel: &C) -> Result<Vec<Datum<A>>, SpaceTxnError> { (self.data)(channel) }

    /// The waiting continuations of the group `channels`.
    pub fn continuations(
        &self,
        channels: &[C],
    ) -> Result<Vec<Arc<WaitingContinuation<P, K>>>, SpaceTxnError> {
        (self.continuations)(channels)
    }

    /// Added by decision 124 (plan v4 C7.9, ruling OP-J): the join groups that
    /// name `channel` in the store of the commit.
    pub fn joins(&self, channel: &C) -> Result<Vec<Vec<C>>, SpaceTxnError> { (self.joins)(channel) }

    /// Added by decision 124 (plan v4 C7.9, ruling OP-J): the pick order of a
    /// search of the commit over `data` and `extra`, whose index is
    /// `data.len()`, from their canonical order `order`. Play shuffles it, as a
    /// pick site plays (DR-129, [`play_order`]). Replay puts first the records
    /// that the recorded row names (CR-4), each part in `order`.
    pub fn pick_order(
        &self,
        data: &[Datum<A>],
        extra: Option<&Datum<A>>,
        order: Vec<usize>,
    ) -> Result<Vec<usize>, SpaceTxnError> {
        let Some(_) = self.named else {
            let mut shuffled = Vec::with_capacity(order.len());
            shuffled.extend(play_order(order).into_iter().map(|(index, _)| index));
            return Ok(shuffled);
        };
        let mut named = Vec::with_capacity(order.len());
        let mut others = Vec::with_capacity(order.len());
        for index in order {
            let (datum, incoming) = match (data.get(index), extra) {
                (Some(datum), _) => (datum, false),
                (None, Some(datum)) => (datum, !datum.persist),
                (None, None) => {
                    return Err(SpaceTxnError::Inconsistent(
                        "a pick index is outside the data of the commit",
                    ));
                }
            };
            match self.named(datum, incoming)? {
                true => named.push(index),
                false => others.push(index),
            }
        }
        named.append(&mut others);
        Ok(named)
    }

    /// Whether the recorded row names `datum` (CR-4, R13). It holds in play.
    /// `incoming` holds when the source of `datum` is the source of the
    /// non-persistent incoming datum of a produce, so its count is one more,
    /// as in the base selection.
    pub fn named(&self, datum: &Datum<A>, incoming: bool) -> Result<bool, SpaceTxnError> {
        let Some(identity) = self.named else {
            return Ok(true);
        };
        candidate_order::candidate_matches_with(
            datum,
            incoming,
            |produce| identity.matches_produce(produce),
            |produce| identity.repetition(produce),
            |produce| Ok::<_, SpaceTxnError>((self.counter)(produce)),
        )?
        .ok_or(SpaceTxnError::CounterOverflow)
    }

    /// Added by decision 124 (rules CR-4 and R13): whether the recorded row
    /// names every datum of `tuple`. It holds in play. The incoming datum of
    /// a produce counts one more, as `named` states.
    pub fn names_tuple(&self, tuple: &[ConsumeCandidate<C, A>]) -> Result<bool, SpaceTxnError> {
        let incoming = tuple
            .iter()
            .find(|candidate| candidate.datum_index == -1 && !candidate.datum.persist)
            .map(|candidate| &candidate.datum.source);
        for candidate in tuple {
            let incoming = incoming.is_some_and(|produce| produce == &candidate.datum.source);
            match self.named(&candidate.datum, incoming)? {
                true => {}
                false => return Ok(false),
            }
        }
        Ok(true)
    }
}

/// Added by decision 123 (plan v4, V3-1, V3-5): the item rule of one role.
/// Play adapts `MatchAdmission`. Replay adapts `NativeOwnerFilter`.
pub trait RowItemRule<K, A> {
    /// Whether `continuation` may take `datum` (the value that its pattern
    /// binds) as one item of a match (R5). False makes the datum a miss of
    /// the pattern.
    fn admit_item(
        &self,
        continuation: &K,
        continuation_persistent: bool,
        channel_count: usize,
        datum: &A,
        datum_persistent: bool,
    ) -> Result<bool, SpaceTxnError>;
}

/// Added by decision 123 (plan v4, V3-5): the item test of one continuation.
pub struct RowItemTest<'r, K, A> {
    pub rule: &'r dyn RowItemRule<K, A>,
    pub continuation: &'r K,
    pub persistent: bool,
    pub channel_count: usize,
}

impl<K, A> RowItemTest<'_, K, A> {
    /// The item test of `datum`, the value that its pattern binds.
    pub fn admits(&self, datum: &A, datum_persistent: bool) -> Result<bool, SpaceTxnError> {
        self.rule.admit_item(
            self.continuation,
            self.persistent,
            self.channel_count,
            datum,
            datum_persistent,
        )
    }
}

/// Added by decision 123 (plan v4, V3-5): the terms of one tuple search of a
/// row.
pub struct RowTupleRequest<'r, C, P: Clone, A: Clone, K: Clone> {
    pub matcher: &'r dyn Match<P, A, K>,
    pub group: &'r [C],
    pub patterns: &'r [P],
    pub continuation: &'r K,
    pub persistent: bool,
    /// The consume of the continuation. A refusal books its hash (CR-20).
    pub source: &'r Consume,
    /// The channel of the incoming datum of a produce (R6).
    pub own: Option<&'r C>,
    /// The item test of an owner-funded operation (R5).
    pub item: Option<RowItemTest<'r, K, A>>,
}

// Added by decision 123 (plan v4, V3-7): `Clone` and `Copy` by hand for
// `RowItemTest` and `RowTupleRequest`, as for `TupleRequest`, so C, P, A and
// K need no `Copy` bound.
impl<K, A> Clone for RowItemTest<'_, K, A> {
    fn clone(&self) -> Self { *self }
}

impl<K, A> Copy for RowItemTest<'_, K, A> {}

impl<C, P: Clone, A: Clone, K: Clone> Clone for RowTupleRequest<'_, C, P, A, K> {
    fn clone(&self) -> Self { *self }
}

impl<C, P: Clone, A: Clone, K: Clone> Copy for RowTupleRequest<'_, C, P, A, K> {}

/// Added by decision 123 (plan v4, V3-5, V3-10): the actual fire of a row.
pub struct ActualFire<C, P: Clone, A: Clone, K: Clone> {
    /// The `K_retire` requests of the actual class, in DR-75 or list order
    /// (CR-33). Empty for a fire without a class.
    pub retires: Vec<SpaceConsume<C, P, A, K>>,
    /// The results that a `FirePrefix` row dispatches.
    pub dispatched: Option<usize>,
}

/// Added by decision 124 (plan v4 C7.11, rules CR-31 and CR-33, ruling
/// RADD2v2 6.6): one finish row of a cut row (P1). The search of the row
/// builds it at the commit, and the unit loop stages and commits it after the
/// group of the row.
pub enum FinishRow<C, P: Clone, A: Clone, K: Clone> {
    /// A produce that stores its record.
    Store(SpaceProduce<C, P, A, K>),
    /// A consume that fires on stored records.
    Take(SpaceConsume<C, P, A, K>),
}

impl<C, P: Clone, A: Clone, K: Clone> FinishRow<C, P, A, K> {
    // The identity of the row, as its staging gives it.
    fn source(&self) -> OpSource
    where
        C: Clone + Serialize,
        P: Serialize,
        A: Serialize,
        K: Serialize,
    {
        match self {
            FinishRow::Store(row) => {
                OpSource::Produce(Produce::create(&row.channel, &row.data, row.persist))
            }
            FinishRow::Take(row) => OpSource::Consume(Consume::create(
                &row.channels,
                &row.patterns,
                &row.continuation,
                row.persist,
            )),
        }
    }
}

/// Added by decision 123 (plan v4, V3-5): the search of one Reified row at its
/// commit. The discipline builds it at staging. It holds the row order of the
/// staging, c0 first with its staged tuple, and it builds the tuple of an
/// alternative only when the search reaches it (F11).
pub trait RowSearch<C, P: Clone, A: Clone, K: Clone>: Send {
    /// The runs of the row order, copied (N5).
    fn runs(&self) -> Vec<RowRun<C>>;

    /// The candidate continuations of the run `run`, with their store
    /// indices, in the row order.
    fn continuations(
        &mut self,
        run: usize,
        lists: &RowLists<'_, C, P, A, K>,
    ) -> Result<Vec<(Arc<WaitingContinuation<P, K>>, i32)>, SpaceTxnError>;

    /// The first tuple of `request` in the run `run`, or None. A datum that
    /// fails `request.item` is a miss of its pattern (R5). So is a datum that
    /// `lists` does not name (CR-4, R13): the search of a recorded refusal
    /// binds only the records of that refusal, in whatever order the staging
    /// left them. A search that keeps a staged tuple gives it only when
    /// `lists` names it (`RowLists::names_tuple`).
    fn tuple(
        &mut self,
        run: usize,
        lists: &RowLists<'_, C, P, A, K>,
        request: RowTupleRequest<'_, C, P, A, K>,
    ) -> Result<Option<Vec<ConsumeCandidate<C, A>>>, SpaceTxnError>;

    /// Undoes what the last `tuple` call took, in reverse.
    fn restore(&mut self);

    /// c0's staged tuple admits with no new run (N3). Every other tuple runs
    /// the guard once, then the theory.
    fn commit_check(
        &mut self,
        request: &RowTupleRequest<'_, C, P, A, K>,
        tuple: &[ConsumeCandidate<C, A>],
    ) -> Result<bool, SpaceTxnError>;

    /// E3-a for one continuation: whether its commit can veto.
    fn may_veto(&self, matcher: &dyn Match<P, A, K>, continuation: &K) -> bool;

    /// The count of staged retire rows after the fire row (CR-33).
    fn staged_retires(&self) -> usize;

    /// The actual fire of the tuple that the search found.
    fn actual_fire(
        &mut self,
        request: &RowTupleRequest<'_, C, P, A, K>,
        tuple: &[ConsumeCandidate<C, A>],
    ) -> ActualFire<C, P, A, K>;

    /// The allowed pairs of the row that fail `rule`, over every candidate in
    /// DR-75 order (CR-20). Each pair is the source hash of the record and
    /// the source hash of the continuation. The first bind whose pattern
    /// binds a record gives its value. No guard or theory runs.
    fn item_failures(
        &self,
        lists: &RowLists<'_, C, P, A, K>,
        matcher: &dyn Match<P, A, K>,
        rule: &dyn RowItemRule<K, A>,
    ) -> Result<Vec<(Blake2b256Hash, Blake2b256Hash)>, SpaceTxnError>;

    /// Added by decision 124 (plan v4 C7.11, CR-31, ruling RDOJ 5.2): the
    /// first step of the continuation of the kind after the actual outcome of
    /// the row, over the store `lists` after the unit of the row (CR-33). It
    /// stages nothing. The default answers that no step reads the store.
    fn tail_read(
        &mut self,
        _lists: &RowLists<'_, C, P, A, K>,
        _matcher: &dyn Match<P, A, K>,
    ) -> Result<TailRead, SpaceTxnError> {
        Ok(TailRead::Ends)
    }

    /// Added by decision 124 (plan v4 C7.19, CR-24, ruling RADD 1.8): the claim
    /// channels of the actual unit of the row, when the row deviates: the
    /// actual fire and its retire group, or the store or the park of the row.
    /// The default answers that the staged claims of the row stand.
    fn actual_claims(&self) -> Option<RowClaims<C>> { None }

    /// Added by decision 124 (plan v4 C7.11, CR-31, ruling RADD2 1.1): the
    /// re-issue that the search built for the last tail read of the row, taken
    /// out of the search. The default answers that the agent builds the
    /// re-issue.
    fn tail_reissue(&mut self) -> Option<RowReissue> { None }

    /// Added by decision 124 (plan v4 C7.11, rules CR-31 and CR-33, ruling
    /// RADD2v2 6.6): the finish rows that the search built for the last tail
    /// read of the row, taken out of the search, in their commit order (P1).
    /// The default answers that the row owes no finish.
    /// A finish row names only channels that need no claim, unless the cut of
    /// its row reports claims. The unit loop adds the channels of a finish row
    /// only to the claims that replace the staged claims of the row (ruling
    /// RADD2v4 2).
    fn finish_rows(&mut self) -> Vec<FinishRow<C, P, A, K>> { Vec::new() }

    /// Added by decision 124 (ruling RADD2 1.2, J-STORE): true when a row of
    /// this search that deviates to a store writes the record that it staged
    /// on its channel. False when no store keeps that record there, so the
    /// row writes nothing and `store_unit` holds the stores of the row. The
    /// default answers true.
    fn keeps_staged_record(&self) -> bool { true }

    /// Added by decision 124 (ruling RADD2 1.2, rules CR-31 and CR-33,
    /// J-STORE): the rows of the store of a row that deviates to a store, in
    /// their commit order, taken out of the search. The unit loop commits them
    /// after the row and before its tail read. `actual_claims` names their
    /// channels. The default answers that the row is its whole unit.
    fn store_unit(&mut self) -> Vec<FinishRow<C, P, A, K>> { Vec::new() }
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
    /// Added by decision 123 (plan v4 C3d, V3-9, R22): an operation whose
    /// introduction its owner refused (R8, CR-18). It stored nothing, as the
    /// base maps the refusal to `Ok(None)` (`refused_stores_nothing` of
    /// execution_space.rs).
    Skipped {
        source: OpSource,
    },
}

/// Results of a committed transaction, in staging order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpaceTxnReceipt<C, P, A, K> {
    pub ops: Vec<SpaceOpReceipt<C, P, A, K>>,
    /// Added by decision 124 (L12, plan v4 C3d): the deviation of the
    /// transaction, or None.
    pub deviation: Option<RowDeviation<C>>,
    /// Added by decision 123 (plan v4 C3d, errata V4-17): the refused pairs
    /// of every committed row, the group rows included (CR-20). Each pair is
    /// the source hash of a record and the source hash of a continuation. The
    /// list allocates on its first pair.
    pub refused: Vec<(Blake2b256Hash, Blake2b256Hash)>,
}

/// Added by decision 124 (L12, plan v4 C3d): the deviation of a committed
/// transaction (F4). Row `index` is row d: the row whose actual fire differs
/// from its staged fire, or the last row of a cut in replay (CR-18). The
/// retire group of row d, if any, follows it in `ops`, and then the finish
/// rows of its search (P1, ruling RADD2v2 6.6), so the group and the finish
/// rows have `ops.len() - index - 1` rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowDeviation<C> {
    /// The index of row d in `SpaceTxnReceipt::ops`.
    pub index: usize,
    /// The allowed pairs of row d that fail their items (CR-20).
    pub item_failed: Vec<(Blake2b256Hash, Blake2b256Hash)>,
    /// The results that a `FirePrefix` row d dispatches.
    pub dispatched: Option<usize>,
    /// True when a candidate of row d may veto its commit (CR-20, item 2).
    pub vetoing: bool,
    /// Added by decision 124 (plan v4 C7.11, ruling RDOJ 5.2): the part of
    /// Tail of row d that reads the store (CR-31): the answer of the search of
    /// the unit of row d, or `Stages` after a record end or a CR-32 end.
    pub tail_read: TailRead,
    /// Added by decision 124 (plan v4 C7.19, CR-24, ruling RADD 1.8): the claim
    /// channels of the actual unit of row d, or `None` when the staged claims
    /// of row d stand.
    pub claims: Option<RowClaims<C>>,
    /// Added by decision 124 (plan v4 C7.11, CR-31, ruling RADD2 1.1): the
    /// re-issue that the search of row d built with its tail read, or `None`
    /// when the agent builds it from the receipt.
    pub reissue: Option<RowReissue>,
}

/// Added by decision 124 (plan v4 C7.19, CR-24, ruling RADD 1.8): the claim
/// channels of the actual unit of a deviating row (row d and its retire
/// group), by role. The finish stores them in place of the staged claims of
/// row d (CR-24, bullet 2).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RowClaims<C> {
    /// The channels whose records the unit stores, delivers or removes.
    pub data: Vec<C>,
    /// The channels of the group of a waiting receive of the unit.
    pub waiters: Vec<C>,
}

/// Added by decision 124 (plan v4 C7.11, CR-31, ruling RADD2 1.1): the
/// re-issue that the search of a deviating row builds with its tail read,
/// when the rest of the operation needs the state of that search. The agent
/// gives the value its type, so the receipt names no kind. Two values are
/// equal when they share one allocation.
#[derive(Clone)]
pub struct RowReissue(Arc<dyn Any + Send + Sync>);

impl RowReissue {
    /// Added by decision 124 (ruling RADD2 1.1): the re-issue `value`.
    pub fn new<T: Any + Send + Sync>(value: T) -> Self { RowReissue(Arc::new(value)) }

    /// Added by decision 124 (ruling RADD2 1.1): the value, when it has the
    /// type `T`.
    pub fn get<T: Any>(&self) -> Option<&T> { self.0.downcast_ref::<T>() }
}

impl PartialEq for RowReissue {
    fn eq(&self, other: &Self) -> bool { Arc::ptr_eq(&self.0, &other.0) }
}

impl Eq for RowReissue {}

impl Debug for RowReissue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str("RowReissue") }
}

/// Added by decision 124 (plan v4 C7.11, CR-31, ruling RDOJ 5.2): the part of
/// Tail of a deviating row that reads the store. The search of the row runs
/// the first step of the continuation of its kind after the actual outcome,
/// read-only, under the commit hold. The finish reads the answer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TailRead {
    /// No step that reads the store follows: the continuation ends.
    #[default]
    Ends,
    /// One more staging step follows, whether it stages or fails (CR-31).
    Stages,
    /// The continuation re-issues the operation and stages nothing more: the
    /// cap of the fires stops it before its next fire.
    Reissues,
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

/// The canonical order of the stored records `data` of one collection
/// (DR-75, decision 41): by the hash of the source produce of each record,
/// then by its full digest inside a run of equal source hashes, then by its
/// index. It returns the indices of `data` in that order, the order of
/// `candidate_order::canonical_candidates`. The records are ordered as
/// borrowed views, and the agent copies only the records that it binds.
pub fn canonical_data_order<A: Clone + Serialize>(data: &[Datum<A>]) -> Vec<usize> {
    canonical_view_order(data)
}

/// [`canonical_data_order`] of the stored records `data` and one record `extra`
/// that is not stored: the indices of their union in the canonical order of
/// DR-75, where `extra` has the index `data.len()`. A VectorDB offer ranks its
/// datum among the stored records with it, so the list of a list-mode waiter
/// that the datum completes has the order of the records once the datum is
/// stored (P6, design v3 4.5).
pub fn canonical_data_order_with<A: Clone + Serialize>(
    data: &[Datum<A>],
    extra: &Datum<A>,
) -> Vec<usize> {
    let mut views = Vec::with_capacity(data.len() + 1);
    views.extend(data.iter().map(Cow::Borrowed));
    views.push(Cow::Borrowed(extra));
    view_indices(candidate_order::canonical_candidates(views))
}

/// [`canonical_data_order`] for the waiting continuations of one collection:
/// by the hash of the source consume of each continuation, then by its
/// digest, then by its index.
pub fn canonical_continuation_order<P, K>(waiters: &[WaitingContinuation<P, K>]) -> Vec<usize>
where
    P: Clone + Serialize,
    K: Clone + Serialize,
{
    canonical_view_order(waiters)
}

/// Added by decision 123 (plan v4, V3-8): the random play order of DR-129,
/// base `candidate_order::shuffle_with_index`. It pairs each entry of `t`
/// with its index in `t` and shuffles the pairs only when there are two or
/// more. A pick site of a Reified discipline plays in this order, as native
/// play does (`ops_produce.rs`, `ops_consume.rs`).
pub fn play_order<D>(t: Vec<D>) -> Vec<(D, i32)> { candidate_order::shuffle_with_index(t) }

fn canonical_view_order<T: CandidateSource + Clone + Serialize>(entries: &[T]) -> Vec<usize> {
    let mut views = Vec::with_capacity(entries.len());
    views.extend(entries.iter().map(Cow::Borrowed));
    view_indices(candidate_order::canonical_candidates(views))
}

// The positions of an ordered list of views. A position is an index of the
// list before the order, so it is not negative.
fn view_indices<V>(ordered: Vec<(V, i32)>) -> Vec<usize> {
    let mut indices = Vec::with_capacity(ordered.len());
    indices.extend(ordered.into_iter().map(|(_, index)| index as usize));
    indices
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

// Changed by decision 90 (C6, step D): the rigged replay modes retire, so the
// rigged spaces no longer implement this trait. The text before:
// ```
// /// Implemented for RSpace (space_play.rs), ReplayRSpace and ReportingRspace
// /// (space_replay.rs). The runtime constructors are bounded by this trait (P4).
// /// ISpace is unchanged.
// ```
/// Implemented for RSpace (space_play.rs) and the host of native replay
/// (native_session/space.rs). `create_rho_runtime` is bounded by this trait
/// (P4). ISpace is unchanged.
#[async_trait]
pub trait SpaceTxnHost<C, P, A, K>: Send + Sync
where
    C: Clone + Send + Sync + 'static,
    P: Clone + Send + Sync + 'static,
    A: Clone + Send + Sync + 'static,
    K: Clone + Send + Sync + 'static,
{
    // Changed by decision 90 (C6, step D): the rigged replay modes retire. The
    // text before:
    // /// Play, replay or reporting.
    /// Play or native replay.
    fn mode(&self) -> SpaceMode;

    /// The host's own injected matcher.
    fn matcher(&self) -> Arc<Box<dyn Match<P, A, K>>>;

    /// Takes the striped consume locks of `lock_keys` (both phases, the host
    /// order) and opens a transaction over the current hot store. Only
    /// [`SpaceStoreHandle::begin`] can supply `registration`.
    ///
    /// Changed by decision 94: the default body takes phase A
    /// ([`SpaceTxnHost::begin_phase_a`]) and then phase B over the same keys
    /// ([`SpaceTxn::lock_phase_b`]). It is the lock sequence of the host
    /// consume.
    // async fn begin(
    //     &self,
    //     lock_keys: &[C],
    //     registration: TxnRegistration,
    // ) -> Result<BoxedSpaceTxn<C, P, A, K>, SpaceTxnError>;
    async fn begin(
        &self,
        lock_keys: &[C],
        registration: TxnRegistration,
    ) -> Result<BoxedSpaceTxn<C, P, A, K>, SpaceTxnError> {
        let mut txn = self.begin_phase_a(lock_keys, registration).await?;
        txn.lock_phase_b(&[]).await?;
        Ok(txn)
    }

    /// Added by decision 94: takes the phase A stripes of `lock_keys` in one
    /// sorted call and opens a transaction over the current hot store. The
    /// transaction covers no channel until [`SpaceTxn::lock_phase_b`] takes
    /// phase B (`JoinScopeLock.tla`). Only [`SpaceStoreHandle`] can supply
    /// `registration`.
    async fn begin_phase_a(
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

    /// Added by decision 90 (DR-132): the wait of a job whose commit failed
    /// with [`SpaceTxnError::NotReady`] at the replay state version `seen`,
    /// after the job released its stripes. It returns at once when the state
    /// changed after `seen`, and otherwise it parks as a counted waiter until
    /// a change notifies it. Only native replay fails with `NotReady`, so
    /// every other host fails here as a substrate defect.
    async fn wait_changed(&self, seen: u64) -> Result<(), SpaceTxnError> {
        let _ = seen;
        Err(SpaceTxnError::Inconsistent("only native replay waits for its recorded rows"))
    }
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

    fn admit_parent<S: Any + Send + Sync>(
        &self,
        parent: &OperationOrder,
        store: &Arc<S>,
    ) -> Result<(), SpaceTxnError> {
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

    /// Added by decision 94: see [`SpaceTxnHost::begin_phase_a`]. Fails with
    /// `NestedTransaction`, before it takes any lock, when the current task
    /// already holds an open transaction of this handle.
    pub async fn begin_phase_a(
        &self,
        lock_keys: &[C],
    ) -> Result<BoxedSpaceTxn<C, P, A, K>, SpaceTxnError> {
        let registration = TxnRegistration::register(&self.state)?;
        self.host.begin_phase_a(lock_keys, registration).await
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

    /// Added by decision 90 (DR-132): see [`SpaceTxnHost::wait_changed`].
    /// Fails with `NestedTransaction`, before it waits, when the current task
    /// holds an open transaction of this handle, so no task waits while it
    /// holds stripes.
    pub async fn wait_changed(&self, seen: u64) -> Result<(), SpaceTxnError> {
        drop(TxnRegistration::register(&self.state)?);
        self.host.wait_changed(seen).await
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
    ///
    /// Changed by decision 102 (decision 119 S1, rules CR-4 and CR-5): in
    /// native replay, the recorded row of `source` at the cursor gives the
    /// answer. A Stored row gives no COMM, and a Matched or DeniedComm row
    /// gives its COMM. Another row, or no row, gives `None`. The call never
    /// answers `RecordEnded` (CR-6). The text before: "`None` in play and in
    /// native replay."
    fn recorded_comms(&mut self, source: RSpaceOperationSource<'_>) -> Option<Vec<COMM>>;

    /// Added by decision 124 (F2, base one row source for each produce,
    /// replay_rspace.rs:1058-1082): the journal index of the recorded row of
    /// `source` at the cursor, the row that `recorded_comms` reads (rule
    /// CR-5). `None` outside native replay, when the lookup finds no row, and
    /// when the lookup fails (`pending_row` records nothing then).
    /// The call never answers `RecordEnded` (CR-6).
    ///
    /// Unlike the look-ahead that decision 124 retired (L1 and L2), it reads
    /// the row that the stage call already reads at the cursor, so it probes no
    /// successor ahead and pins no pick.
    fn recorded_row_index(&mut self, source: RSpaceOperationSource<'_>) -> Option<usize>;

    /// Replay admission of `datum` for `comm` over the staged counters;
    /// always `true` in play. `incoming` is `true` when `datum.source` is
    /// the source of the produce being matched.
    ///
    /// Changed by decision 102 (rules CR-4 and R13): native replay admits a
    /// record as replay does, by the named-record test of the base. The text
    /// before: "always `true` in play and in native replay."
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

    /// Added by decision 94: takes phase B over the own lock keys and the
    /// keys `scope_keys` of a join scope, in one sorted call
    /// (`JoinScopeLock.tla`). The transaction then covers exactly these keys.
    /// It stages nothing before this call, and a second call fails with
    /// `InvalidOperation`.
    async fn lock_phase_b(&mut self, scope_keys: &[C]) -> Result<(), SpaceTxnError>;

    /// Added by decision 92: sets the footprint key `key` of a space
    /// transaction, `S(sid)`, which must be one of the own lock keys. Every
    /// operation of the transaction then names the key in its footprint, and
    /// the commit requires that the first operation is its job's own
    /// (`SpaceReplayOrder.tla`). A second call fails with `InvalidOperation`.
    fn set_footprint_key(&mut self, key: C) -> Result<(), SpaceTxnError>;

    /// Added by decision 92: removes every staged operation and its staged
    /// effects. The transaction keeps its locks, its coverage and its
    /// footprint key, so it can stage an anchor and commit it.
    ///
    /// Changed by decision 102 (plan v4 C3b): the discard also resets the
    /// cursor of native replay, so the next stage call takes the first open
    /// row of its source again.
    fn discard_staged(&mut self) -> Result<(), SpaceTxnError>;

    /// Added by plan v4 errata (V4-12, rule CR-5): marks the operation that
    /// the caller just staged as the leading anchor of the transaction. The
    /// row of a leading anchor is not a followed row for CR-6 and CR-32. The
    /// default does nothing.
    fn mark_leading_anchor(&mut self) {}

    /// Added by plan v4 errata (V4-12, rule CR-32) and decision 124 (L11): the
    /// query after a staging error. True ends the staging. That holds in native
    /// replay after the transaction followed a row after its leading anchor.
    /// The commit of the followed prefix then needs CanStop of its last row
    /// (CR-18). Otherwise it fails with the session mismatch. False keeps the
    /// failure path of the caller (the error rule of CR-5). The default returns
    /// false.
    fn end_record(&mut self) -> bool { false }

    /// The single mutation point. Releases the locks before returning.
    async fn commit(&mut self) -> Result<SpaceTxnReceipt<C, P, A, K>, SpaceTxnError>;

    /// Added by decision 90 (item 3.10, and decision 91 SQ-4): `commit`, which
    /// keeps the stripes and the registration of a committed transaction in
    /// the returned [`TxnHold`] until the caller drops it. The caller
    /// finishes work that must follow the commit under its stripes, such as
    /// the store of the seq claims and the mirror publication of play (decision
    /// 98). A failed commit releases them at once, as `commit` does. A
    /// transaction that holds no stripe returns a released hold.
    async fn commit_holding(
        &mut self,
    ) -> Result<(SpaceTxnReceipt<C, P, A, K>, TxnHold), SpaceTxnError> {
        self.commit()
            .await
            .map(|receipt| (receipt, TxnHold::released()))
    }
}

pub(crate) type SharedStore<C, P, A, K> = Arc<Box<dyn HotStore<C, P, A, K>>>;

/// The store that a transaction reads: the hot store in play, replay and
/// reporting, or the digest-keyed store of the native replay session in
/// native replay (DR-96). Each read gives the list that the host reads, with
/// the installed entries first, and a value that the store does not hold is
/// read at the history root. `key` is the store identity of `channel`
/// ([`channel_key`]), and `keys` are the channel keys of a group in group
/// order, so a store that is keyed by digests computes no hash of its own.
pub(crate) trait TxnStore<C, P, A, K>: Send + Sync
where
    C: Clone + Hash + Eq,
    P: Clone,
    A: Clone,
    K: Clone,
{
    /// The data of `channel`, whose key is `key`.
    fn data(&self, channel: &C, key: &Blake2b256Hash) -> Result<Vec<Datum<A>>, SpaceTxnError>;

    /// The waiting continuations of the group `channels`, whose channel keys
    /// are `keys`.
    fn continuations(
        &self,
        channels: &[C],
        keys: &[Blake2b256Hash],
    ) -> Result<Vec<Arc<WaitingContinuation<P, K>>>, SpaceTxnError>;

    /// The join groups of `channel`, whose key is `key`.
    fn joins(&self, channel: &C, key: &Blake2b256Hash) -> Result<Vec<Vec<C>>, SpaceTxnError>;
}

// The hot store reads by the channels, so it does not use the keys.
impl<C, P, A, K> TxnStore<C, P, A, K> for dyn HotStore<C, P, A, K> + '_
where
    C: Clone + Hash + Eq,
    P: Clone,
    A: Clone,
    K: Clone,
{
    fn data(&self, channel: &C, _key: &Blake2b256Hash) -> Result<Vec<Datum<A>>, SpaceTxnError> {
        Ok(self.get_data(channel))
    }

    fn continuations(
        &self,
        channels: &[C],
        _keys: &[Blake2b256Hash],
    ) -> Result<Vec<Arc<WaitingContinuation<P, K>>>, SpaceTxnError> {
        Ok(self.get_continuations_arc(channels))
    }

    fn joins(&self, channel: &C, _key: &Blake2b256Hash) -> Result<Vec<Vec<C>>, SpaceTxnError> {
        Ok(self.get_joins(channel))
    }
}

// A boxed store reads as the store in the box. A host keeps its hot store
// boxed in an `Arc` (`SharedStore`).
impl<S, C, P, A, K> TxnStore<C, P, A, K> for Box<S>
where
    S: TxnStore<C, P, A, K> + ?Sized,
    C: Clone + Hash + Eq,
    P: Clone,
    A: Clone,
    K: Clone,
{
    fn data(&self, channel: &C, key: &Blake2b256Hash) -> Result<Vec<Datum<A>>, SpaceTxnError> {
        (**self).data(channel, key)
    }

    fn continuations(
        &self,
        channels: &[C],
        keys: &[Blake2b256Hash],
    ) -> Result<Vec<Arc<WaitingContinuation<P, K>>>, SpaceTxnError> {
        (**self).continuations(channels, keys)
    }

    fn joins(&self, channel: &C, key: &Blake2b256Hash) -> Result<Vec<Vec<C>>, SpaceTxnError> {
        (**self).joins(channel, key)
    }
}

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

// Disabled by decision 94: the coverage compares the exact keys of phase B,
// so no outcome depends on the lock stripes (`JoinScopeLock.tla`, the control
// `CoverageByHostUseUnsafe`). A stripe of another channel no longer covers a
// channel, and a key of a join scope holds phase B only.
//
// // The stripes that one lock phase holds, indexed as
// // `striped_locks::acquire_locks` indexes them: `channel hash % stripes`.
// #[derive(Clone, Debug)]
// struct HeldStripes {
//     stripes: usize,
//     held: BTreeSet<usize>,
// }
//
// impl HeldStripes {
//     fn new(stripes: usize, lock_hashes: &[u64]) -> Self {
//         let held = match stripes {
//             0 => BTreeSet::new(),
//             stripes => lock_hashes
//                 .iter()
//                 .map(|hash| (*hash as usize) % stripes)
//                 .collect(),
//         };
//         Self { stripes, held }
//     }
//
//     fn holds(&self, channel_hash: u64) -> bool {
//         match self.stripes {
//             0 => false,
//             stripes => self.held.contains(&((channel_hash as usize) %
// stripes)),         }
//     }
// }
//
// // How the host locks a channel that an operation touches: a produce locks
// // its own channel in phase A and every channel of its join groups in phase
// // B; a consume locks its channels in both phases.
// #[derive(Clone, Copy, Debug)]
// enum LockUse {
//     Produced,
//     Joined,
//     Consumed,
// }
//
// /// The exact stripes that `begin` holds. `begin` takes both phases for every
// /// lock key (`striped_locks::consume_lock`), so a channel that an operation
// /// touches is covered when its stripe is held in the phase that host
// /// produce and consume use for it. No host operation can then interleave
// /// with the commit on a touched channel.
// #[derive(Clone, Debug)]
// pub(crate) struct LockCoverage {
//     phase_a: HeldStripes,
//     phase_b: HeldStripes,
// }
//
// impl LockCoverage {
//     pub(crate) fn new(lock_hashes: &[u64], phase_a_stripes: usize,
// phase_b_stripes: usize) -> Self {         Self {
//             phase_a: HeldStripes::new(phase_a_stripes, lock_hashes),
//             phase_b: HeldStripes::new(phase_b_stripes, lock_hashes),
//         }
//     }
//
//     fn require<C: Hash>(&self, channel: &C, lock_use: LockUse) -> Result<(),
// SpaceTxnError> {         let hash = striped_locks::channel_hash(channel);
//         let held = match lock_use {
//             LockUse::Produced => self.phase_a.holds(hash),
//             LockUse::Joined => self.phase_b.holds(hash),
//             LockUse::Consumed => self.phase_a.holds(hash) &&
// self.phase_b.holds(hash),         };
//         match held {
//             true => Ok(()),
//             false => Err(SpaceTxnError::UnlockedChannel),
//         }
//     }
// }

/// Changed by decision 94: the exact channel keys ([`channel_key`]) of the
/// lock keys that the transaction holds in phase B (`JoinScopeLock.tla`,
/// `Covered`). [`SpaceTxn::lock_phase_b`] takes phase B over the own lock
/// keys and the keys of a join scope, and [`SpaceStoreHandle::begin`] takes
/// it over the own lock keys. A channel that a staged operation touches is
/// covered when its key is one of them. Before phase B the coverage is empty,
/// so no operation stages.
#[derive(Clone, Debug, Default)]
pub(crate) struct LockCoverage {
    keys: BTreeSet<Blake2b256Hash>,
}

impl LockCoverage {
    // The coverage of the phase B keys `keys`.
    fn of<C: Serialize>(keys: &[C]) -> Result<Self, SpaceTxnError> {
        let mut covered = BTreeSet::new();
        for key in keys {
            covered.insert(channel_key(key)?);
        }
        Ok(Self { keys: covered })
    }

    // Changed by decision 94: the check takes the channel key, which the
    // staging computes once.
    fn require(&self, key: &Blake2b256Hash) -> Result<(), SpaceTxnError> {
        match self.keys.contains(key) {
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

// Retired by decision 90 (C6, step D): it counted the recorded bindings of the
// rigged replay modes, and its last callers retire with them. The text before:
// ```
// /// Bindings that one recorded occurrence of `comm` adds under `key` in
// /// `rig` (replay_rspace.rs), and that `remove_bindings_for` removes: one for
// /// the consume, and one per occurrence of a produce in the produce list.
// pub(crate) fn binding_multiplicity(key: &IOEvent, comm: &COMM) -> usize {
//     match key {
//         IOEvent::Consume(consume) => usize::from(*consume == comm.consume),
//         IOEvent::Produce(produce) => comm
//             .produces
//             .iter()
//             .filter(|listed| *listed == produce)
//             .count(),
//     }
// }
// ```

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
    installed_channel_keys(installs.keys())
}

/// Keys of every channel of the installed groups `groups` (R2). Play takes
/// the groups from its installs ([`installed_channels`]), and native replay
/// takes them from the installations of its session.
pub fn installed_channel_keys<'g, C: Serialize + 'g>(
    groups: impl IntoIterator<Item = &'g Vec<C>>,
) -> Result<BTreeSet<Blake2b256Hash>, SpaceTxnError> {
    let mut installed = BTreeSet::new();
    for channel in groups.into_iter().flatten() {
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
    /// Loads `channel`'s data on the first touch of `key`. A failed read
    /// caches nothing in the view.
    pub(crate) fn load_data<S: TxnStore<C, P, A, K> + ?Sized>(
        &mut self,
        store: &S,
        key: &Blake2b256Hash,
        channel: &C,
    ) -> Result<&[Datum<A>], SpaceTxnError> {
        if !self.data.contains_key(key) {
            let records = store.data(channel, key)?;
            self.data.insert(key.clone(), DataEntry { records });
        }
        self.data
            .get(key)
            .map(|entry| entry.records.as_slice())
            .ok_or(SpaceTxnError::Inconsistent("staged data is not loaded"))
    }

    /// Loads the continuations of the group `channels` on the first touch of
    /// `key`, as copies of the shared continuations of the store. A failed
    /// read caches nothing in the view.
    pub(crate) fn load_continuations<S: TxnStore<C, P, A, K> + ?Sized>(
        &mut self,
        store: &S,
        key: &[Blake2b256Hash],
        channels: &[C],
    ) -> Result<&[WaitingContinuation<P, K>], SpaceTxnError> {
        if !self.conts.contains_key(key) {
            let shared = store.continuations(channels, key)?;
            let mut waiting = Vec::with_capacity(shared.len());
            waiting.extend(shared.iter().map(|continuation| (**continuation).clone()));
            self.conts
                .insert(key.to_vec(), ContinuationEntry { waiting });
        }
        self.conts
            .get(key)
            .map(|entry| entry.waiting.as_slice())
            .ok_or(SpaceTxnError::Inconsistent("staged continuations are not loaded"))
    }

    /// Loads `channel`'s join groups. A stored group that is not canonical
    /// fails the read and is not cached in the view.
    pub(crate) fn load_joins<S: TxnStore<C, P, A, K> + ?Sized>(
        &mut self,
        store: &S,
        key: &Blake2b256Hash,
        channel: &C,
    ) -> Result<&JoinEntry<C>, SpaceTxnError> {
        if !self.joins.contains_key(key) {
            let groups = store.joins(channel, key)?;
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

    /// Added by decision 124 (plan v4 C3g bullet 4, ruling D-2): readies a
    /// consume fire of `candidates` over the group `group` of `channels`, for
    /// the staging and for a retire at commit. It gives the positions of the
    /// bound records and the COMM of the fire.
    pub(crate) fn ready_fire<B, S>(
        &mut self,
        backend: &B,
        store: &S,
        group: &[Blake2b256Hash],
        channels: &[C],
        candidates: &[ConsumeCandidate<C, A>],
        source: (&Consume, &BTreeSet<i32>),
    ) -> Result<(Vec<Option<usize>>, COMM), SpaceTxnError>
    where
        B: TxnBackend<C, P, A, K>,
        S: TxnStore<C, P, A, K> + ?Sized,
    {
        check_lined_up(group, candidates)?;
        if candidates.iter().any(|candidate| candidate.datum_index < 0) {
            return Err(SpaceTxnError::InvalidOperation(
                "a consume fire binds only stored records",
            ));
        }
        for (channel_key, channel) in group.iter().zip(channels) {
            self.load_data(store, channel_key, channel)?;
        }
        self.check_stored_candidates(group, candidates)?;
        let positions = self.claim_positions(group, candidates)?;
        let counters = self.comm_counters(backend, candidates, None)?;
        let (consume, peeks) = source;
        let comm = COMM::new(candidates, consume.clone(), peeks.clone(), |produces| {
            lookup_counters(&counters, produces)
        });
        Ok((positions, comm))
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

    /// Added by decision 124 (plan v4 C3g bullet 4, ruling D-2): the staged
    /// counter of `produce`, from the counter `backend` gives. The body of
    /// `TxnCore::view_counter` moved here, so that a commit can apply a row
    /// over a fresh view.
    fn view_counter<B: TxnBackend<C, P, A, K>>(
        &self,
        backend: &B,
        produce: &Produce,
    ) -> Result<i32, SpaceTxnError> {
        self.staged_count(backend.host_counter(produce), produce)
    }

    /// Added by decision 124 (plan v4 C3g bullet 4, ruling D-2): the counters
    /// of the COMM's produces, the staged counter of each produce plus one for
    /// the produce being fired when it is not persistent (`incoming`). The body
    /// of `TxnCore::comm_counters` moved here.
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

    /// Added by decision 124 (plan v4 C3g bullet 4, ruling D-2): every stored
    /// candidate binds the record at its index in the view, and a
    /// non-persistent record binds once per COMM (FR-1'). `keys[i]` is the key
    /// of candidate `i`. The body of `TxnCore::check_stored_candidates` moved
    /// here.
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

    /// Added by decision 124 (plan v4 C3g bullet 4, ruling D-2): the view
    /// effects of a produce-triggered fire, in the host order, and the
    /// position of the removed continuation. The body of
    /// `TxnCore::apply_fire_to_view` moved here.
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
                let position = self.remove_continuation(group, continuation)?;
                effects.push(StoreEffect::RemoveContinuation {
                    group: group.to_vec(),
                    source: continuation.source.clone(),
                });
                Some(position)
            }
        };
        effects.extend(self.remove_records(group, candidates, removed)?);
        for channel_key in group {
            if self.remove_join(channel_key, group)? {
                effects.push(StoreEffect::RemoveJoin {
                    channel: channel_key.clone(),
                    group: group.to_vec(),
                });
            }
        }
        Ok((effects, continuation_position))
    }

    /// Added by decision 124 (plan v4 C3g bullet 4, ruling D-2): the view
    /// effects of a park. The body of `TxnCore::park_in_view` moved here.
    fn park_in_view(
        &mut self,
        group: &[Blake2b256Hash],
        channels: &[C],
        waiting: WaitingContinuation<P, K>,
    ) -> Result<Vec<StoreEffect>, SpaceTxnError> {
        let source = waiting.source.clone();
        let mut effects = Vec::with_capacity(group.len() + 1);
        if self.put_continuation(group, waiting)? {
            effects.push(StoreEffect::PutContinuation {
                group: group.to_vec(),
                source,
            });
        }
        for channel_key in group {
            if self.put_join(channel_key, group, channels)? {
                effects.push(StoreEffect::PutJoin {
                    channel: channel_key.clone(),
                    group: group.to_vec(),
                });
            }
        }
        Ok(effects)
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
    // Added by decision 124 (K18, rule CR-7, F2): true for a row that fires
    // another candidate than its staged one.
    deviated: bool,
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
                // Changed by decision 124 (K18, rule CR-7, F2): a deviating row keeps
                // the Produce event of its staged source for replay (R-dev), and its
                // COMM lists the source of the actual fire, which can be another
                // source. The Produce event of the row is the trigger of that COMM.
                // The text before:
                // ```
                // Event::IoEvent(IOEvent::Produce(produce)) => comm.produces.contains(produce),
                // ```
                Event::IoEvent(IOEvent::Produce(produce)) => {
                    deviated || comm.produces.contains(produce)
                }
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

// Changed by decision 90 (C6, step D): a fire holds no recorded COMM, because
// only the rigged replay modes paired one. The text before:
// ```
// /// A staged fire of a produce: the fired join and its key, its continuation,
// /// the candidates, the staged COMM, retention, and the recorded COMM in
// /// replay. `removed` holds the `(candidate, position)` pairs of the records
// /// that the fire removes, and `continuation_position` the position of a
// /// removed (non-persistent) continuation, both in the staged lists before
// /// the fire.
// ```
/// A staged fire of a produce: the fired join and its key, its continuation,
/// the candidates, and the staged COMM. `removed` holds the
/// `(candidate, position)` pairs of the records that the fire removes, and
/// `continuation_position` the position of a removed (non-persistent)
/// continuation, both in the staged lists before the fire.
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
    // Retired by decision 90 (C6, step D): only the rigged replay modes paired
    // a fire with a recorded COMM. The text before:
    // ```
    // /// The paired COMM of the log. `None` in play, and for an unpaired fire
    // /// in replay or reporting.
    // pub(crate) recorded: Option<COMM>,
    // ```
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

// Changed by decision 90 (C6, step D): a fire holds no recorded COMM, because
// only the rigged replay modes paired one. The text before:
// ```
// /// A staged fire of a consume: the key of its group, the candidates, the
// /// staged COMM, and the recorded COMM in replay. `removed` holds the
// /// `(candidate, position)` pairs of the records that the fire removes, in the
// /// staged lists before the fire.
// ```
/// A staged fire of a consume: the key of its group, the candidates, and the
/// staged COMM. `removed` holds the `(candidate, position)` pairs of the
/// records that the fire removes, in the staged lists before the fire.
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
    // Retired by decision 90 (C6, step D): only the rigged replay modes paired
    // a fire with a recorded COMM. The text before:
    // ```
    // /// The paired COMM of the log. `None` in play, and for an unpaired fire
    // /// in replay or reporting.
    // pub(crate) recorded: Option<COMM>,
    // ```
}

pub(crate) struct StagedConsume<C, P: Clone, A: Clone, K: Clone> {
    pub(crate) source: Consume,
    pub(crate) channels: Vec<C>,
    /// The key of the group `channels`, which a native park publishes under.
    pub(crate) group: Vec<Blake2b256Hash>,
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
    // Added by decision 92: the operation is its job's own
    // ([`SpaceProduce::own`]).
    own: bool,
    // Added by decision 123 (plan v4, V3-5): the search of the row at its
    // commit ([`SpaceProduce::search`]).
    search: Option<Box<dyn RowSearch<C, P, A, K>>>,
}

/// Added by decision 123 (plan v4, V3-10, V3-11): what each read of a row
/// search checks at commit before it reads: the phase B keys of the
/// transaction, then its installed channels, in the order of the staging
/// (CR-36). A miss fails closed with `UnlockedChannel` or `InstalledChannel`.
/// It is a bug, not a deviation.
#[derive(Clone, Copy)]
pub(crate) struct RowReads<'r> {
    coverage: &'r LockCoverage,
    installed: &'r BTreeSet<Blake2b256Hash>,
}

impl<'r> RowReads<'r> {
    // The checks of the channel key `key`.
    fn require(self, key: &Blake2b256Hash) -> Result<(), SpaceTxnError> {
        self.coverage.require(key)?;
        match self.installed.contains(key) {
            true => Err(SpaceTxnError::InstalledChannel),
            false => Ok(()),
        }
    }

    /// The data read of a row search over the store `store` of the commit
    /// ([`RowLists::data`]).
    pub(crate) fn data<S, C, P, A, K>(
        self,
        store: &'r S,
    ) -> impl Fn(&C) -> Result<Vec<Datum<A>>, SpaceTxnError> + 'r
    where
        S: TxnStore<C, P, A, K> + ?Sized,
        C: Clone + Hash + Eq + Serialize,
        P: Clone,
        A: Clone,
        K: Clone,
    {
        move |channel| {
            let key = channel_key(channel)?;
            self.require(&key)?;
            store.data(channel, &key)
        }
    }

    /// The continuation read of a row search over the store `store` of the
    /// commit ([`RowLists::continuations`]). The group must be canonical, as
    /// in the staging.
    pub(crate) fn continuations<S, C, P, A, K>(
        self,
        store: &'r S,
    ) -> impl Fn(&[C]) -> Result<Vec<Arc<WaitingContinuation<P, K>>>, SpaceTxnError> + 'r
    where
        S: TxnStore<C, P, A, K> + ?Sized,
        C: Clone + Hash + Eq + Serialize,
        P: Clone,
        A: Clone,
        K: Clone,
    {
        move |channels| {
            let keys = canonical_group_key(channels)?;
            for key in &keys {
                self.require(key)?;
            }
            store.continuations(channels, &keys)
        }
    }

    /// Added by decision 124 (plan v4 C7.9, ruling OP-J): the join read of a
    /// row search over the store `store` of the commit ([`RowLists::joins`]).
    pub(crate) fn joins<S, C, P, A, K>(
        self,
        store: &'r S,
    ) -> impl Fn(&C) -> Result<Vec<Vec<C>>, SpaceTxnError> + 'r
    where
        S: TxnStore<C, P, A, K> + ?Sized,
        C: Clone + Hash + Eq + Serialize,
        P: Clone,
        A: Clone,
        K: Clone,
    {
        move |channel| {
            let key = channel_key(channel)?;
            self.require(&key)?;
            store.joins(channel, &key)
        }
    }
}

/// Added by decision 124 (L11, L16): the search of a row with the reads of
/// that search at commit (CR-36). The name keeps `TxnBackend::apply_row`
/// within seven inputs.
pub(crate) type RowSearchWithReads<'r, C, P, A, K> = (Box<dyn RowSearch<C, P, A, K>>, RowReads<'r>);

/// Added by decision 123 (plan v4, V3-10): one row that the commit applied
/// with its search. Decision 124 (plan v4 C7.15, C7.16, ruling OP-PARTS) keeps
/// the parts of the row in `RowParts`, which the search finds before the row
/// commits.
pub(crate) struct AppliedRow<C, P: Clone, A: Clone, K: Clone> {
    /// The receipt of the row.
    pub(crate) receipt: SpaceOpReceipt<C, P, A, K>,
    /// The parts of the row that its receipt does not give.
    pub(crate) parts: RowParts<C, P, A, K>,
}

impl<C, P: Clone, A: Clone, K: Clone> AppliedRow<C, P, A, K> {
    /// The row with the receipt `receipt` of its staged operation: no
    /// effects of its own, no deviation, no pairs and no actual fire. It
    /// cannot stop a replay, and it can continue one. The empty lists
    /// allocate nothing.
    pub(crate) fn staged(receipt: SpaceOpReceipt<C, P, A, K>) -> Self {
        Self {
            receipt,
            parts: RowParts::staged(),
        }
    }
}

/// Added by decision 124 (L10, plan v4 C7.15, C7.16): the row of an
/// introduction that its owner cannot pay (R8). It stores nothing, applies no
/// effect and deviates. In replay it can stop a unit by `can_stop`, and it
/// never continues one (CR-18).
pub(crate) fn skipped_row<C, P: Clone, A: Clone, K: Clone>(
    source: OpSource,
    can_stop: bool,
) -> AppliedRow<C, P, A, K> {
    let parts = RowParts {
        effects: Some(Vec::new()),
        deviated: true,
        refused: Vec::new(),
        item_failed: Vec::new(),
        actual: None,
        can_stop,
        can_continue: false,
        vetoing: false,
        claims: None,
        search: None,
    };
    AppliedRow {
        receipt: SpaceOpReceipt::Skipped { source },
        parts,
    }
}

/// Added by decision 124 (L15, L16, plan v4 C7.15, C7.16, CR-18, CR-20,
/// ruling OP-PARTS): the parts of an applied row that its receipt does not
/// give. The search of a row finds them before the row commits, and
/// `AppliedRow` joins them with the receipt of the commit.
pub(crate) struct RowParts<C, P: Clone, A: Clone, K: Clone> {
    /// The effects that a deviating row applied. None keeps the staged
    /// effects.
    pub(crate) effects: Option<Vec<StoreEffect>>,
    /// True when the row fired another candidate than the staged one, or
    /// none.
    pub(crate) deviated: bool,
    /// The refused pairs of the row (R7): the source hash of each record and
    /// the source hash of the continuation.
    pub(crate) refused: Vec<(Blake2b256Hash, Blake2b256Hash)>,
    /// The allowed pairs of a deviating row that fail their items (CR-20).
    pub(crate) item_failed: Vec<(Blake2b256Hash, Blake2b256Hash)>,
    /// The actual fire of the row.
    pub(crate) actual: Option<ActualFire<C, P, A, K>>,
    /// CanStop of the row, in replay only (CR-18).
    pub(crate) can_stop: bool,
    /// CanContinue of the row, in replay only (CR-18).
    pub(crate) can_continue: bool,
    /// Added by decision 124 (L12, plan v4 errata V4-18): true on a deviating
    /// Stored or Parked row when `RowSearch::may_veto` holds for a candidate
    /// of the row search (CR-20, item 2). Both roles set it.
    pub(crate) vetoing: bool,
    /// Added by decision 124 (plan v4 C7.19, CR-24, ruling RADD 1.8): the claim
    /// channels of the actual unit of a deviating row, or `None` when the
    /// staged claims stand (`RowSearch::actual_claims`).
    pub(crate) claims: Option<RowClaims<C>>,
    /// Added by decision 124 (plan v4 C7.11, CR-31, ruling RADD 1.2): the
    /// search of the row, which reads the tail of a deviating unit after its
    /// retire group (`TxnBackend::tail_read`). None for a row without a search.
    pub(crate) search: Option<Box<dyn RowSearch<C, P, A, K>>>,
}

impl<C, P: Clone, A: Clone, K: Clone> RowParts<C, P, A, K> {
    /// The parts of a row that applies as staged (`AppliedRow::staged`).
    pub(crate) fn staged() -> Self {
        Self {
            effects: None,
            deviated: false,
            refused: Vec::new(),
            item_failed: Vec::new(),
            actual: None,
            can_stop: false,
            can_continue: true,
            vetoing: false,
            claims: None,
            search: None,
        }
    }
}

/// Added by decision 123 (plan v4, V3-5): the item rule of play, the item
/// test of the admission of an owner-funded operation (R5). It never fails.
pub(crate) struct PlayItemRule<'a, C, P: Clone, A: Clone, K: Clone>(
    pub(crate) &'a dyn MatchAdmission<C, P, A, K>,
);

impl<C, P: Clone, A: Clone, K: Clone> RowItemRule<K, A> for PlayItemRule<'_, C, P, A, K> {
    fn admit_item(
        &self,
        continuation: &K,
        continuation_persistent: bool,
        channel_count: usize,
        datum: &A,
        datum_persistent: bool,
    ) -> Result<bool, SpaceTxnError> {
        Ok(self.0.admit_item(
            continuation,
            continuation_persistent,
            channel_count,
            datum,
            datum_persistent,
        ))
    }
}

/// Added by decision 123 (plan v4, V3-5): the item rule of replay, the item
/// test of the owner filter of the row (R5). A read error of the filter is a
/// host error.
pub(crate) struct ReplayItemRule<'a, C, P, A, K>(pub(crate) &'a dyn NativeOwnerFilter<C, P, A, K>);

impl<C, P, A, K> RowItemRule<K, A> for ReplayItemRule<'_, C, P, A, K> {
    fn admit_item(
        &self,
        continuation: &K,
        continuation_persistent: bool,
        channel_count: usize,
        datum: &A,
        datum_persistent: bool,
    ) -> Result<bool, SpaceTxnError> {
        self.0
            .admit_item(
                continuation,
                continuation_persistent,
                channel_count,
                datum,
                datum_persistent,
            )
            .map_err(SpaceTxnError::Host)
    }
}

/// Added by decision 123 (plan v4, V3-5, V3-11, V4-2): the `MatchCandidates`
/// source of one row with a search, at its commit. Its groups are the runs of
/// the row order (N5). In play a tuple runs the item test of its request
/// through [`PlayItemRule`]. In replay it runs the replay item rule of the
/// source. The source takes no lock. The commit holds every key of the row
/// and its scope (decision 94), and every read checks the coverage (CR-36).
pub(crate) struct RowCandidates<'r, C, P: Clone, A: Clone, K: Clone> {
    search: &'r mut dyn RowSearch<C, P, A, K>,
    runs: Vec<RowRun<C>>,
    lists: RowLists<'r, C, P, A, K>,
    rule: Option<&'r dyn RowItemRule<K, A>>,
    refused: Vec<(Blake2b256Hash, Blake2b256Hash)>,
}

impl<'r, C, P: Clone, A: Clone, K: Clone> RowCandidates<'r, C, P, A, K> {
    /// The source over the search `search` with the reads `lists` and the
    /// replay item rule `rule` (None in play). The refused list allocates on
    /// the first refusal.
    pub(crate) fn new(
        search: &'r mut dyn RowSearch<C, P, A, K>,
        lists: RowLists<'r, C, P, A, K>,
        rule: Option<&'r dyn RowItemRule<K, A>>,
    ) -> Self {
        let runs = search.runs();
        Self {
            search,
            runs,
            lists,
            rule,
            refused: Vec::new(),
        }
    }

    /// Added by decision 124 (L16, plan v4 C7.15, C7.16): the channels of
    /// each run, the groups of the base produce search.
    pub(crate) fn groups(&self) -> Vec<Vec<C>>
    where C: Clone {
        let mut groups = Vec::with_capacity(self.runs.len());
        groups.extend(self.runs.iter().map(|run| run.channels.clone()));
        groups
    }

    /// Added by decision 124 (L16, plan v4 C7.15, C7.16, errata V4-17): the
    /// refused pairs that the search booked.
    pub(crate) fn into_refused(self) -> Vec<(Blake2b256Hash, Blake2b256Hash)> { self.refused }
}

/// Added by decision 123 (plan v4, V4-2): the lists of one run of a row
/// search. The group step reads the continuations of the run, and
/// `continuations` hands them out once.
pub(crate) struct RowGroup<'g, 'r, C, P: Clone, A: Clone, K: Clone> {
    source: &'g mut RowCandidates<'r, C, P, A, K>,
    run: usize,
    continuations: Vec<(Arc<WaitingContinuation<P, K>>, i32)>,
}

impl<'r, C, P, A, K> MatchCandidates<C, P, A, K> for RowCandidates<'r, C, P, A, K>
where
    C: Clone,
    P: Clone,
    A: Clone,
    K: Clone,
{
    type Error = SpaceTxnError;
    type Group<'g>
        = RowGroup<'g, 'r, C, P, A, K>
    where
        Self: 'g,
        C: 'g;

    /// The lists of the run `index`, whose channels are the group. A read
    /// error of the continuations of the run (CR-36) returns here, so the
    /// search fails closed.
    fn group<'g>(
        &'g mut self,
        index: usize,
        _channels: &'g [C],
    ) -> Result<RowGroup<'g, 'r, C, P, A, K>, SpaceTxnError> {
        if index >= self.runs.len() {
            return Err(SpaceTxnError::Inconsistent("a row search has no run at the group index"));
        }
        let continuations = self.search.continuations(index, &self.lists)?;
        Ok(RowGroup {
            source: self,
            run: index,
            continuations,
        })
    }

    fn may_veto(&self, matcher: &dyn Match<P, A, K>, continuation: &K) -> bool {
        self.search.may_veto(matcher, continuation)
    }
}

impl<C, P, A, K> MatchGroup<C, P, A, K> for RowGroup<'_, '_, C, P, A, K>
where
    C: Clone,
    P: Clone,
    A: Clone,
    K: Clone,
{
    type Error = SpaceTxnError;

    fn continuations(&mut self) -> Vec<(Arc<WaitingContinuation<P, K>>, i32)> {
        std::mem::take(&mut self.continuations)
    }

    /// The first tuple of the run for `request`. The recorded row must name
    /// every bound datum (CR-4, R13). An unnamed datum fails closed.
    fn tuple(
        &mut self,
        request: TupleRequest<'_, C, P, A, K>,
    ) -> Result<Option<Vec<ConsumeCandidate<C, A>>>, SpaceTxnError> {
        let mut play = None;
        let source = &mut *self.source;
        let request = row_request(request, source.rule, &mut play);
        let Some(tuple) = source.search.tuple(self.run, &source.lists, request)? else {
            return Ok(None);
        };
        match source.lists.names_tuple(&tuple)? {
            true => Ok(Some(tuple)),
            false => Err(SpaceTxnError::InvalidOperation("a row search bound an unnamed record")),
        }
    }

    fn restore(&mut self) { self.source.search.restore() }

    fn commit_check(
        &mut self,
        request: &TupleRequest<'_, C, P, A, K>,
        tuple: &[ConsumeCandidate<C, A>],
    ) -> Result<bool, SpaceTxnError> {
        let mut play = None;
        let source = &mut *self.source;
        let request = row_request(*request, source.rule, &mut play);
        source.search.commit_check(&request, tuple)
    }

    fn may_veto(&self, matcher: &dyn Match<P, A, K>, continuation: &K) -> bool {
        self.source.search.may_veto(matcher, continuation)
    }

    /// Books each record of the refused tuple with the continuation of the
    /// request (CR-20).
    fn refused(
        &mut self,
        request: &TupleRequest<'_, C, P, A, K>,
        tuple: &[ConsumeCandidate<C, A>],
    ) {
        self.source.refused.extend(
            tuple.iter().map(|candidate| {
                (candidate.datum.source.hash.clone(), request.source.hash.clone())
            }),
        );
    }
}

// Added by decision 123 (plan v4, V3-5): the row request of the tuple request
// `request`, with the item test of the role. In play the item test of the
// request runs through the play rule, which goes into `play`. In replay the
// item rule `rule` of the source runs.
fn row_request<'x, C, P: Clone, A: Clone, K: Clone>(
    request: TupleRequest<'x, C, P, A, K>,
    rule: Option<&'x dyn RowItemRule<K, A>>,
    play: &'x mut Option<PlayItemRule<'x, C, P, A, K>>,
) -> RowTupleRequest<'x, C, P, A, K> {
    let item = match request.item {
        Some(item) => Some(RowItemTest {
            rule: play.insert(PlayItemRule(item.admission)),
            continuation: item.continuation,
            persistent: item.persistent,
            channel_count: item.channel_count,
        }),
        None => rule.map(|rule| RowItemTest {
            rule,
            continuation: request.continuation,
            persistent: request.persistent,
            channel_count: request.group.len(),
        }),
    };
    RowTupleRequest {
        matcher: request.matcher,
        group: request.group,
        patterns: request.patterns,
        continuation: request.continuation,
        persistent: request.persistent,
        source: request.source,
        own: request.own,
        item,
    }
}

/// Added by decision 124 (L15, plan v4 C7.15, C7.16, errata V4-18): the
/// enumeration of a deviating row (CR-20). It gives the allowed pairs of the
/// row that fail `rule`, and whether the commit of the own continuation
/// `own` or of a candidate continuation of the row may veto.
pub(crate) fn row_enumeration<C, P: Clone, A: Clone, K: Clone>(
    search: &mut dyn RowSearch<C, P, A, K>,
    lists: &RowLists<'_, C, P, A, K>,
    matcher: &dyn Match<P, A, K>,
    rule: &dyn RowItemRule<K, A>,
    own: Option<&K>,
) -> Result<(Vec<(Blake2b256Hash, Blake2b256Hash)>, bool), SpaceTxnError> {
    let item_failed = search.item_failures(lists, matcher, rule)?;
    let mut vetoing = own.is_some_and(|continuation| search.may_veto(matcher, continuation));
    let runs = search.runs().len();
    let mut run = 0;
    while !vetoing && run < runs {
        let waiters = search.continuations(run, lists)?;
        vetoing = waiters
            .iter()
            .any(|(waiter, _)| search.may_veto(matcher, &waiter.continuation));
        run += 1;
    }
    Ok((item_failed, vetoing))
}

// Changed by decision 90 (C6, step D): `space_replay` retires. The text before:
// ```
// /// One host mode. Each child module (`space_play`, `space_replay`,
// /// `native_session/space`) implements `apply_op` in the host's exact call
// /// order.
// ```
/// One host mode. Each child module (`space_play`, `native_session/space`)
/// implements `apply_op` in the host's exact call order.
pub(crate) trait TxnBackend<C, P, A, K>: Send + Sync + 'static
where
    C: Clone + Hash + Eq,
    P: Clone,
    A: Clone,
    K: Clone,
{
    /// The store that the host reads and changes. A commit compares the
    /// store with the store of its parent order, so it lives as long as the
    /// program (`admit_parent`).
    type Store: TxnStore<C, P, A, K> + 'static;

    /// The host's current store.
    fn store(&self) -> Arc<Self::Store>;

    /// The host produce counter (play: trace_log.rs; replay:
    /// `get_produce_count`).
    fn host_counter(&self, produce: &Produce) -> i32;

    // Retired by decision 90 (C6, step D): these read the rigged log of the replay
    // modes, which retire. The text before:
    // ```
    // /// Recorded bindings under `event` with their multiplicities (empty in
    // /// play).
    // fn recorded(&self, event: &IOEvent) -> Vec<(COMM, usize)>;
    //
    // /// The recorded copy of `comm` under `event` and its multiplicity.
    // fn recorded_binding(&self, event: &IOEvent, comm: &COMM) -> Option<(COMM, usize)>;
    // ```

    /// Applies one staged operation to `store` and reports the events it
    /// emitted.
    ///
    /// Changed by decision 92: `footprint_key` is the footprint key of the
    /// transaction ([`SpaceTxn::set_footprint_key`]). Play records it as one
    /// more join group of the footprint of the row, and native replay
    /// authenticates it in the recorded footprint.
    // fn apply_op(
    //     &self,
    //     store: &Self::Store,
    //     operation: StagedOperation<C, P, A, K>,
    //     before_observe: Option<BeforeObserve>,
    //     events: &mut Vec<Event>,
    // ) -> Result<SpaceOpReceipt<C, P, A, K>, SpaceTxnError>;
    fn apply_op(
        &self,
        store: &Self::Store,
        operation: StagedOperation<C, P, A, K>,
        before_observe: Option<BeforeObserve>,
        footprint_key: Option<&C>,
        events: &mut Vec<Event>,
    ) -> Result<SpaceOpReceipt<C, P, A, K>, SpaceTxnError>;

    /// Added by decision 124 (L11, L16, plan v4 section 2.10 item 1): applies
    /// one staged row with its row search and the reads of that search
    /// (CR-36). The search can fire another candidate than the staged one, or
    /// none. The default applies the staged operation, as `apply_op` does.
    fn apply_row(
        &self,
        store: &Self::Store,
        operation: StagedOperation<C, P, A, K>,
        before_observe: Option<BeforeObserve>,
        footprint_key: Option<&C>,
        search: Option<RowSearchWithReads<'_, C, P, A, K>>,
        events: &mut Vec<Event>,
    ) -> Result<AppliedRow<C, P, A, K>, SpaceTxnError> {
        let _ = search;
        self.apply_op(store, operation, before_observe, footprint_key, events)
            .map(AppliedRow::staged)
    }

    /// Added by decision 124 (plan v4 C7.11, CR-31, ruling RADD 1.2): the part
    /// of Tail of a deviating unit that reads the store. The search `search` of
    /// row d runs the first step of the continuation of its kind over the store
    /// after the retire group of the unit (CR-33), with the lists and the
    /// matcher of the backend (`RowSearch::tail_read`). It stages nothing. The
    /// default answers that no step reads the store.
    fn tail_read(
        &self,
        store: &Self::Store,
        search: &mut dyn RowSearch<C, P, A, K>,
        reads: RowReads<'_>,
    ) -> Result<TailRead, SpaceTxnError> {
        let _ = (store, search, reads);
        Ok(TailRead::Ends)
    }

    /// Added by decision 90 (DR-132): the readiness check of the whole
    /// transaction, under its stripes and before its first operation, so a
    /// transaction that is not ready applies nothing, runs no
    /// `before_observe` hook and resolves no authority (review amendment (e)).
    /// Native replay checks the recorded rows of `operations` and fails with
    /// [`SpaceTxnError::NotReady`] (`native_session/space.rs`). Play records
    /// its own log, so it has nothing to wait for.
    ///
    /// Changed by decision 92: the check reads the first operation `first`
    /// only. Every row of a space transaction names the footprint key
    /// `S(sid)`, so the rows of one space form one chain in play order, and
    /// the first row of a committed transaction is its job's own. When that
    /// row is open and its predecessors are complete, the space is in the
    /// state that play staged against (`SpaceReplayOrder.tla`). A later row
    /// that is not ready or does not fit fails closed at the commit.
    // fn check_ready(
    //     &self,
    //     operations: &[&StagedOperation<C, P, A, K>],
    // ) -> Result<(), SpaceTxnError> {
    //     let _ = operations;
    //     Ok(())
    // }
    fn check_ready(&self, first: &StagedOperation<C, P, A, K>) -> Result<(), SpaceTxnError> {
        let _ = first;
        Ok(())
    }

    /// Added by decision 102 (decision 119 S2): the recorded row of the
    /// staged operation `source` (rule CR-5 of the plan). With `previous`
    /// None it is the first open row of the source. Otherwise it is the row
    /// of the source whose S(sid) predecessor is the journal row `previous`.
    /// `footprint_key` is S(sid). Native replay asks its epoch
    /// (`native_session/space.rs`). Play never asks, so the default fails
    /// closed.
    fn recorded_row(
        &self,
        source: RSpaceOperationSource<'_>,
        previous: Option<usize>,
        footprint_key: &C,
    ) -> Result<Option<RecordedRow>, SpaceTxnError> {
        let _ = (source, previous, footprint_key);
        Err(SpaceTxnError::InvalidOperation("this host mode has no recorded rows"))
    }

    /// Added by decision 94: phase B over the channel hashes `hashes`, in one
    /// sorted call, as the host takes phase B (`striped_locks::acquire_locks`,
    /// and the fallible preparation of `striped_locks::native` in native
    /// replay).
    fn lock_phase_b(
        &self,
        hashes: &[u64],
    ) -> impl std::future::Future<Output = Result<ChannelLockGuard, SpaceTxnError>> + Send;
}

/// Added by decision 102 (decision 119 S1, plan v4 C3b): the cursor of a
/// native replay transaction over the recorded rows of its footprint key
/// `S(sid)` (rules CR-5, CR-6 and CR-32). Each index is a journal index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReplayCursor {
    /// The transaction staged no operation. The next stage call takes the
    /// first open row of its source.
    Unread,
    /// Added by plan v4 errata (V4-12): the transaction followed only the row
    /// of its leading anchor (CR-5). That row is not a followed row for CR-6
    /// and CR-32.
    Anchored(usize),
    /// The last staged operation follows the row with this index.
    Following(usize),
    /// The transaction follows no row. It stages in DR-75 mode.
    Free,
    /// The record ended. A stage call found no successor row (CR-6), or a
    /// staging error ended the staging (CR-32).
    Ended,
}

/// Added by decision 102 (plan v4 C3b): the cursor and the slot that a stage
/// call takes when it stages its operation.
#[derive(Clone, Copy, Debug)]
struct CursorStep {
    cursor: ReplayCursor,
    slot: Option<usize>,
}

pub(crate) struct TxnCore<S, C, P, A, K>
where
    C: Clone + Hash + Eq,
    P: Clone,
    A: Clone,
    K: Clone,
{
    mode: SpaceMode,
    store: Arc<S>,
    installed: BTreeSet<Blake2b256Hash>,
    // Added by decision 94: the own lock keys, which phase A holds from the
    // begin on. Phase B takes them with the keys of a join scope.
    lock_keys: Vec<C>,
    coverage: LockCoverage,
    // Added by decision 92: the footprint key of a space transaction, `S(sid)`
    // ([`SpaceTxn::set_footprint_key`]). Every row of the transaction names it
    // in its footprint.
    footprint_key: Option<C>,
    view: TxnView<C, P, A, K>,
    // Retired by decision 90 (C6, step D): only the rigged replay modes paired
    // recorded COMMs. The text before:
    // paired: Vec<COMM>,
    staged: Vec<Staged<C, P, A, K>>,
    // Added by decision 102 (decision 119 S1, plan v4 C3b): the cursor of
    // native replay (rule CR-5).
    cursor: ReplayCursor,
    // Added by decision 102 (plan v4 C3b): the journal index of the recorded
    // row of each staged operation, parallel to `staged`. `None` marks an
    // operation that follows no row.
    slots: Vec<Option<usize>>,
    // Added by decision 102 (plan v4 C3b): the last lookup of
    // `recorded_comms` and its source. The next stage call of that source
    // takes its row, so the lookup runs once for each operation.
    pending: Option<(OpSource, Option<RecordedRow>)>,
    finished: bool,
}

impl<S, C, P, A, K> TxnCore<S, C, P, A, K>
where
    S: TxnStore<C, P, A, K>,
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
    // Changed by decision 94: the coverage checks the exact phase B keys, so
    // the group takes no lock use.
    // fn admit_group(
    //     &self,
    //     channels: &[C],
    //     patterns: usize,
    //     lock_use: LockUse,
    // ) -> Result<Vec<Blake2b256Hash>, SpaceTxnError> {
    //     check_arity(channels, patterns)?;
    //     let key = canonical_group_key(channels)?;
    //     for channel in channels {
    //         self.coverage.require(channel, lock_use)?;
    //     }
    fn admit_group(
        &self,
        channels: &[C],
        patterns: usize,
    ) -> Result<Vec<Blake2b256Hash>, SpaceTxnError> {
        check_arity(channels, patterns)?;
        let key = canonical_group_key(channels)?;
        for channel_key in &key {
            self.coverage.require(channel_key)?;
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
        self.view.load_data(&*self.store, &key, channel)
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
        self.view.load_continuations(&*self.store, &key, channels)
    }

    fn read_joins(&mut self, channel: &C) -> Result<&[Vec<C>], SpaceTxnError> {
        self.ensure_open()?;
        let key = channel_key(channel)?;
        self.admit(&key)?;
        self.view
            .load_joins(&*self.store, &key, channel)
            .map(|entry| entry.groups.as_slice())
    }

    // Retired by decision 124 (plan v4 C3g bullet 4, ruling D-2, OP-FWD): the body
    // moved to `TxnView::view_counter`, and each caller calls it through the view.
    // The retired text:
    // ```
    // fn view_counter<B: TxnBackend<C, P, A, K>>(
    //     &self,
    //     backend: &B,
    //     produce: &Produce,
    // ) -> Result<i32, SpaceTxnError> {
    //     self.view
    //         .staged_count(backend.host_counter(produce), produce)
    // }
    // ```

    // Retired by decision 124 (plan v4 C3g bullet 4, ruling D-2, OP-FWD): the body
    // moved to `TxnView::comm_counters`, and each caller calls it through the view.
    // The retired text:
    // ```
    // // Counters of the COMM's produces: the staged counter of each produce,
    // // plus one for the produce being fired when it is not persistent (the
    // // play closure of ops_produce.rs). `incoming` is that produce, if any.
    // fn comm_counters<B: TxnBackend<C, P, A, K>>(
    //     &self,
    //     backend: &B,
    //     candidates: &[ConsumeCandidate<C, A>],
    //     incoming: Option<&Produce>,
    // ) -> Result<BTreeMap<Produce, i32>, SpaceTxnError> {
    //     let mut counters = BTreeMap::new();
    //     for candidate in candidates {
    //         let produce = &candidate.datum.source;
    //         if counters.contains_key(produce) {
    //             continue;
    //         }
    //         let staged = self.view_counter(backend, produce)?;
    //         let count = match incoming {
    //             Some(source) if source == produce => staged
    //                 .checked_add(1)
    //                 .ok_or(SpaceTxnError::CounterOverflow)?,
    //             _ => staged,
    //         };
    //         counters.insert(produce.clone(), count);
    //     }
    //     Ok(counters)
    // }
    // ```

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
                // Changed by decision 124 (plan v4 C3g bullet 4, ruling D-2, OP-FWD):
                // the caller calls the method of the view. The text before:
                // ```
                // self.view_counter(backend, source)?
                //     .checked_add(1)
                //     .ok_or(SpaceTxnError::CounterOverflow)?;
                // ```
                self.view
                    .view_counter(backend, source)?
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

    // Retired by decision 90 (C6, step D): the rigged replay modes retire, and
    // native replay checks each fire against its journal row at commit. The text
    // before:
    // ```
    // // Replay pairing under the trigger key only, as host replay looks it up:
    // // `Produce(source)` for a produce-side fire and `Consume(source)` for a
    // // consume-side fire. The key holds one binding per occurrence of the key
    // // in the COMM (`binding_multiplicity`), so a recorded occurrence is
    // // available while the paired copies use fewer bindings than it holds.
    // // Returns the recorded copy, or `None` in play, in native replay and for
    // // an unpaired fire. An unpaired fire stages as in play, never pairs, and
    // // fails at commit after the observer calls of play (decision 36), so
    // // replay staging equals play staging for every log. Native replay
    // // checks each fire against its journal row at commit (decision 47).
    // fn verify<B: TxnBackend<C, P, A, K>>(
    //     &self,
    //     backend: &B,
    //     comm: &COMM,
    //     trigger: &IOEvent,
    // ) -> Option<COMM> {
    //     match self.mode {
    //         SpaceMode::Play | SpaceMode::NativeReplay => None,
    //         SpaceMode::Replay | SpaceMode::Reporting => {
    //             let (recorded, count) = backend.recorded_binding(trigger, comm)?;
    //             let available = match binding_multiplicity(trigger, comm) {
    //                 0 => 0,
    //                 multiplicity => count / multiplicity,
    //             };
    //             let paired = self.paired.iter().filter(|paired| *paired == comm).count();
    //             (paired < available).then_some(recorded)
    //         }
    //     }
    // }
    // ```

    // Added by decision 102 (decision 119 S1, plan v4 C3b): the recorded row of
    // the operation `source` at the cursor (rule CR-5). The cursor `Unread`
    // takes the first open row of the source. The cursors `Anchored(j)` and
    // `Following(j)` take the successor of row j on the chain of the
    // footprint key. The cursors `Free` and `Ended` follow no row. A
    // transaction without a footprint key has no chain, so it follows no row
    // either. The lookup reserves nothing and never waits.
    fn lookup<B: TxnBackend<C, P, A, K>>(
        &self,
        backend: &B,
        source: RSpaceOperationSource<'_>,
    ) -> Result<Option<RecordedRow>, SpaceTxnError> {
        let previous = match self.cursor {
            ReplayCursor::Unread => None,
            ReplayCursor::Anchored(row) | ReplayCursor::Following(row) => Some(row),
            ReplayCursor::Free | ReplayCursor::Ended => return Ok(None),
        };
        match &self.footprint_key {
            Some(footprint_key) => backend.recorded_row(source, previous, footprint_key),
            None => Ok(None),
        }
    }

    // Added by decision 102 (plan v4 C3b): the recorded row of `source` for
    // `recorded_comms`. The call reuses the pending lookup of the same
    // source, and a new lookup becomes the pending lookup. A failed lookup
    // gives `None` and records nothing, so the next stage call looks up
    // again and fails with the error.
    fn pending_row<B: TxnBackend<C, P, A, K>>(
        &mut self,
        backend: &B,
        source: RSpaceOperationSource<'_>,
    ) -> Option<RecordedRow> {
        match &self.pending {
            Some((pending, row)) if same_source(pending, source) => row.clone(),
            Some(_) | None => {
                let row = self.lookup(backend, source).ok()?;
                self.pending = Some((owned_source(source), row.clone()));
                row
            }
        }
    }

    // Added by decision 102 (plan v4 C3b): the pending lookup of
    // `recorded_comms` when it has the source `source`, and otherwise a new
    // lookup. Either way the pending lookup is spent.
    fn take_row<B: TxnBackend<C, P, A, K>>(
        &mut self,
        backend: &B,
        source: RSpaceOperationSource<'_>,
    ) -> Result<Option<RecordedRow>, SpaceTxnError> {
        match self.pending.take() {
            Some((pending, row)) if same_source(&pending, source) => Ok(row),
            Some(_) | None => self.lookup(backend, source),
        }
    }

    // Added by decision 102 (decision 119 S1, plan v4 C3b): the cursor step of
    // a stage call of `source` (rules CR-5 and CR-6). Only native replay
    // follows rows. A row of the source makes the cursor follow it. A
    // successor miss after a followed row ends the record: the cursor becomes
    // `Ended`, and the call stages nothing and fails with `RecordEnded`.
    // Without a row, the cursors `Unread` and `Free` stage in DR-75 mode.
    // Added by plan v4 errata (V4-12): the miss rule of CR-5. With the cursor
    // `Anchored`, a successor miss makes the cursor `Free`, and the call
    // stages its operation in DR-75 mode. It does not answer `RecordEnded`.
    fn cursor_step<B: TxnBackend<C, P, A, K>>(
        &mut self,
        backend: &B,
        source: RSpaceOperationSource<'_>,
    ) -> Result<CursorStep, SpaceTxnError> {
        match self.mode {
            SpaceMode::NativeReplay => {}
            SpaceMode::Play => {
                return Ok(CursorStep {
                    cursor: self.cursor,
                    slot: None,
                });
            }
        }
        let row = self.take_row(backend, source)?.map(|(row, _, _)| row);
        match (self.cursor, row) {
            (ReplayCursor::Ended, _) => Err(SpaceTxnError::RecordEnded),
            (ReplayCursor::Following(_), None) => {
                self.cursor = ReplayCursor::Ended;
                Err(SpaceTxnError::RecordEnded)
            }
            (
                ReplayCursor::Unread | ReplayCursor::Anchored(_) | ReplayCursor::Following(_),
                Some(row),
            ) => Ok(CursorStep {
                cursor: ReplayCursor::Following(row),
                slot: Some(row),
            }),
            (ReplayCursor::Unread | ReplayCursor::Anchored(_), None) | (ReplayCursor::Free, _) => {
                Ok(CursorStep {
                    cursor: ReplayCursor::Free,
                    slot: None,
                })
            }
        }
    }

    // Added by decision 102 (plan v4 C3b): a staged operation takes its cursor
    // step, so `slots` stays parallel to `staged`.
    fn take_step(&mut self, step: CursorStep) {
        self.cursor = step.cursor;
        self.slots.push(step.slot);
    }

    // Loads a fired group: the data and the join groups of every channel and
    // the group's continuations.
    fn load_group(&mut self, key: &[Blake2b256Hash], channels: &[C]) -> Result<(), SpaceTxnError> {
        let store = &*self.store;
        for (channel_key, channel) in key.iter().zip(channels) {
            self.view.load_data(store, channel_key, channel)?;
            self.view.load_joins(store, channel_key, channel)?;
        }
        self.view.load_continuations(store, key, channels)?;
        Ok(())
    }

    // Retired by decision 124 (plan v4 C3g bullet 4, ruling D-2, OP-FWD): the body
    // moved to `TxnView::check_stored_candidates`, and each caller calls it through
    // the view. The retired text:
    // ```
    // // Every stored candidate binds the record at its index in the staged
    // // view, and a non-persistent record binds once per COMM, also when the
    // // group repeats its channel (FR-1'). `keys[i]` is the key of candidate
    // // `i`.
    // fn check_stored_candidates(
    //     &self,
    //     keys: &[Blake2b256Hash],
    //     candidates: &[ConsumeCandidate<C, A>],
    // ) -> Result<(), SpaceTxnError> {
    //     let mut bound: BTreeSet<(&Blake2b256Hash, usize)> = BTreeSet::new();
    //     for (candidate, key) in candidates.iter().zip(keys) {
    //         let Ok(position) = usize::try_from(candidate.datum_index) else {
    //             continue;
    //         };
    //         let entry = self
    //             .view
    //             .data
    //             .get(key)
    //             .ok_or(SpaceTxnError::Inconsistent("staged data is not loaded"))?;
    //         let record = entry
    //             .records
    //             .get(position)
    //             .ok_or(SpaceTxnError::NotInView("a stored candidate index"))?;
    //         if record_bytes(record)? != record_bytes(&stored_record(candidate))? {
    //             return Err(SpaceTxnError::NotInView("a stored candidate record"));
    //         }
    //         if !candidate.datum.persist && !bound.insert((key, position)) {
    //             return Err(SpaceTxnError::NotInView("a non-persistent record bound twice"));
    //         }
    //     }
    //     Ok(())
    // }
    // ```

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
    // Changed by decision 94: every lock check is a check of the exact phase B
    // keys.
    fn stage_produce<B: TxnBackend<C, P, A, K>>(
        &mut self,
        backend: &B,
        op: SpaceProduce<C, P, A, K>,
    ) -> Result<StagedOp, SpaceTxnError> {
        self.ensure_open()?;
        // Changed by decision 92: the produce carries whether it is its job's
        // own operation.
        // let SpaceProduce {
        //     channel,
        //     data,
        //     persist,
        //     outcome,
        //     before_observe,
        // } = op;
        let SpaceProduce {
            channel,
            data,
            persist,
            outcome,
            before_observe,
            own,
            // Added by decision 123 (plan v4, V3-5): the row search moves
            // into the staged operation.
            search,
        } = op;
        let key = channel_key(&channel)?;
        // self.coverage.require(&channel, LockUse::Produced)?;
        self.coverage.require(&key)?;
        self.admit(&key)?;
        let source = Produce::create(&channel, &data, persist);
        let fired = match outcome {
            ProduceOutcome::Store => None,
            ProduceOutcome::Fire {
                channels,
                continuation,
                data: candidates,
            } => {
                // let group =
                //     self.admit_group(&channels, continuation.patterns.len(),
                // LockUse::Joined)?;
                let group = self.admit_group(&channels, continuation.patterns.len())?;
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
            let entry = self.view.load_joins(&*self.store, &key, &channel)?;
            // for group in &entry.groups {
            //     for joined in group {
            //         self.coverage.require(joined, LockUse::Joined)?;
            //     }
            // }
            for group_key in &entry.keys {
                for joined_key in group_key {
                    self.coverage.require(joined_key)?;
                }
            }
            entry.groups.clone()
        };
        self.view.load_data(&*self.store, &key, &channel)?;
        // Added by decision 102 (plan v4 C3b): the cursor step of the produce
        // (rules CR-5 and CR-6). A record end stages nothing.
        let step = self.cursor_step(backend, RSpaceOperationSource::Produce(&source))?;
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
                    own,
                    // Added by decision 123 (plan v4, V3-5).
                    search,
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
                // Added by decision 102 (plan v4 C3b).
                self.take_step(step);
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
                // Changed by decision 124 (plan v4 C3g bullet 4, ruling D-2, OP-FWD):
                // the caller calls the method of the view. The text before:
                // self.check_stored_candidates(&group, &candidates)?;
                self.view.check_stored_candidates(&group, &candidates)?;
                let positions = self.view.claim_positions(&group, &candidates)?;
                let incoming = match persist {
                    true => None,
                    false => Some(&source),
                };
                // Changed by decision 124 (plan v4 C3g bullet 4, ruling D-2, OP-FWD):
                // the caller calls the method of the view. The text before:
                // let counters = self.comm_counters(backend, &candidates, incoming)?;
                let counters = self.view.comm_counters(backend, &candidates, incoming)?;
                let comm = COMM::new(
                    &candidates,
                    continuation.source.clone(),
                    continuation.peeks.clone(),
                    |produces| lookup_counters(&counters, produces),
                );
                // Retired by decision 90 (C6, step D): no fire pairs a recorded
                // COMM. The text before:
                // ```text
                // let recorded = self.verify(backend, &comm, &IOEvent::Produce(source.clone()));
                // ```
                let delta = self.next_delta(backend, &source, persist)?;
                let removed = removed_records(&candidates, &positions);
                // Changed by decision 124 (plan v4 C3g bullet 4, ruling D-2, OP-FWD):
                // the caller calls the method of the view. The text before:
                // let applied = self.apply_fire_to_view(&group, &continuation, &candidates,
                // &removed);
                let applied =
                    self.view
                        .apply_fire_to_view(&group, &continuation, &candidates, &removed);
                let (effects, continuation_position) = match applied {
                    Ok(applied) => applied,
                    Err(error) => return Err(self.poison(error)),
                };
                if let Some(delta) = delta {
                    self.view.count_delta.insert(source.clone(), delta);
                }
                // Retired by decision 90 (C6, step D): the text before:
                // ```
                // if recorded.is_some() {
                //     self.paired.push(comm.clone());
                // }
                // ```
                self.staged.push(Staged {
                    own,
                    // Added by decision 123 (plan v4, V3-5).
                    search,
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
                            // Retired by decision 90 (C6, step D): the field is gone.
                            // recorded,
                            removed,
                            continuation_position,
                        }),
                    }),
                    before_observe,
                    effects,
                });
                // Added by decision 102 (plan v4 C3b).
                self.take_step(step);
                Ok(StagedOp {
                    index,
                    source: OpSource::Produce(source),
                    comm: Some(comm),
                })
            }
        }
    }

    // Retired by decision 124 (plan v4 C3g bullet 4, ruling D-2, OP-FWD): the body
    // moved to `TxnView::apply_fire_to_view`, and each caller calls it through the
    // view. The retired text:
    // ```
    // // View effects of a produce-triggered fire, in the host order: remove a
    // // non-persistent continuation, then the records that the fire removes
    // // (`removed`) and the fired join of every candidate channel. The incoming
    // // datum is never stored: a peek fire leaves it to the agent, which offers
    // // it again (FR-10, decision 30). Returns the staged effects and the
    // // position of the removed continuation.
    // fn apply_fire_to_view(
    //     &mut self,
    //     group: &[Blake2b256Hash],
    //     continuation: &WaitingContinuation<P, K>,
    //     candidates: &[ConsumeCandidate<C, A>],
    //     removed: &[(usize, usize)],
    // ) -> Result<(Vec<StoreEffect>, Option<usize>), SpaceTxnError> {
    //     let mut effects = Vec::with_capacity(removed.len() + group.len() + 1);
    //     let continuation_position = match continuation.persist {
    //         true => None,
    //         false => {
    //             let position = self.view.remove_continuation(group, continuation)?;
    //             effects.push(StoreEffect::RemoveContinuation {
    //                 group: group.to_vec(),
    //                 source: continuation.source.clone(),
    //             });
    //             Some(position)
    //         }
    //     };
    //     effects.extend(self.view.remove_records(group, candidates, removed)?);
    //     for channel_key in group {
    //         if self.view.remove_join(channel_key, group)? {
    //             effects.push(StoreEffect::RemoveJoin {
    //                 channel: channel_key.clone(),
    //                 group: group.to_vec(),
    //             });
    //         }
    //     }
    //     Ok((effects, continuation_position))
    // }
    // ```

    // Retired by decision 124 (plan v4 C3g bullet 4, ruling D-2, OP-FWD): the body
    // moved to `TxnView::park_in_view`, and each caller calls it through the view.
    // The retired text:
    // ```
    // // View effects of a park: the continuation, unless the view holds one
    // // with its Debug identity, and the join of every channel that lacks it.
    // // Returns the staged effects.
    // fn park_in_view(
    //     &mut self,
    //     group: &[Blake2b256Hash],
    //     channels: &[C],
    //     waiting: WaitingContinuation<P, K>,
    // ) -> Result<Vec<StoreEffect>, SpaceTxnError> {
    //     let source = waiting.source.clone();
    //     let mut effects = Vec::with_capacity(group.len() + 1);
    //     if self.view.put_continuation(group, waiting)? {
    //         effects.push(StoreEffect::PutContinuation {
    //             group: group.to_vec(),
    //             source,
    //         });
    //     }
    //     for channel_key in group {
    //         if self.view.put_join(channel_key, group, channels)? {
    //             effects.push(StoreEffect::PutJoin {
    //                 channel: channel_key.clone(),
    //                 group: group.to_vec(),
    //             });
    //         }
    //     }
    //     Ok(effects)
    // }
    // ```

    // Validation order: arity, canonical order, both lock phases of every
    // channel, the installed guard, and the peeks rule; then the outcome.
    // Changed by decision 94: the lock check is a check of the exact phase B
    // keys.
    fn stage_consume<B: TxnBackend<C, P, A, K>>(
        &mut self,
        backend: &B,
        op: SpaceConsume<C, P, A, K>,
    ) -> Result<StagedOp, SpaceTxnError> {
        self.ensure_open()?;
        // Changed by decision 92: the consume carries whether it is its job's
        // own operation.
        // let SpaceConsume {
        //     channels,
        //     patterns,
        //     continuation,
        //     persist,
        //     peeks,
        //     outcome,
        //     before_observe,
        // } = op;
        let SpaceConsume {
            channels,
            patterns,
            continuation,
            persist,
            peeks,
            outcome,
            before_observe,
            own,
            // Added by decision 123 (plan v4, V3-5): the row search moves
            // into the staged operation.
            search,
        } = op;
        // let group = self.admit_group(&channels, patterns.len(), LockUse::Consumed)?;
        let group = self.admit_group(&channels, patterns.len())?;
        check_peeks(channels.len(), &peeks)?;
        let source = Consume::create(&channels, &patterns, &continuation, persist);
        // Added by decision 102 (plan v4 C3b): the cursor step of the consume
        // (rules CR-5 and CR-6). A record end stages nothing.
        let step = self.cursor_step(backend, RSpaceOperationSource::Consume(&source))?;
        let index = self.staged.len();
        match outcome {
            ConsumeOutcome::Park => {
                {
                    let store = &*self.store;
                    self.view.load_continuations(store, &group, &channels)?;
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
                // Changed by decision 124 (plan v4 C3g bullet 4, ruling D-2, OP-FWD):
                // the caller calls the method of the view. The text before:
                // let effects = match self.park_in_view(&group, &channels, waiting) {
                let effects = match self.view.park_in_view(&group, &channels, waiting) {
                    Ok(effects) => effects,
                    Err(error) => return Err(self.poison(error)),
                };
                self.staged.push(Staged {
                    own,
                    // Added by decision 123 (plan v4, V3-5).
                    search,
                    operation: StagedOperation::Consume(StagedConsume {
                        source: source.clone(),
                        channels,
                        group,
                        patterns,
                        continuation,
                        persist,
                        peeks,
                        fire: None,
                    }),
                    before_observe,
                    effects,
                });
                // Added by decision 102 (plan v4 C3b).
                self.take_step(step);
                Ok(StagedOp {
                    index,
                    source: OpSource::Consume(source),
                    comm: None,
                })
            }
            ConsumeOutcome::Fire { data: candidates } => {
                // Changed by decision 124 (plan v4 C3g bullet 4, ruling D-2):
                // the steps moved to `TxnView::ready_fire`, which the retire of
                // an actual fire at commit shares (6.8). The text before:
                // ```
                // check_lined_up(&group, &candidates)?;
                // if candidates.iter().any(|candidate| candidate.datum_index < 0) {
                //     return Err(SpaceTxnError::InvalidOperation(
                //         "a consume fire binds only stored records",
                //     ));
                // }
                // {
                //     let store = &*self.store;
                //     for (channel_key, channel) in group.iter().zip(&channels) {
                //         self.view.load_data(store, channel_key, channel)?;
                //     }
                // }
                // self.check_stored_candidates(&group, &candidates)?;
                // let positions = self.view.claim_positions(&group, &candidates)?;
                // let counters = self.comm_counters(backend, &candidates, None)?;
                // let comm = COMM::new(&candidates, source.clone(), peeks.clone(), |produces| {
                //     lookup_counters(&counters, produces)
                // });
                // ```
                let (positions, comm) = self.view.ready_fire(
                    backend,
                    &*self.store,
                    &group,
                    &channels,
                    &candidates,
                    (&source, &peeks),
                )?;
                // Retired by decision 90 (C6, step D): no fire pairs a recorded
                // COMM. The text before:
                // ```
                // let recorded = self.verify(backend, &comm, &IOEvent::Consume(source.clone()));
                // ```
                let removed = removed_records(&candidates, &positions);
                let effects = match self.view.remove_records(&group, &candidates, &removed) {
                    Ok(effects) => effects,
                    Err(error) => return Err(self.poison(error)),
                };
                // Retired by decision 90 (C6, step D): the text before:
                // ```
                // if recorded.is_some() {
                //     self.paired.push(comm.clone());
                // }
                // ```
                self.staged.push(Staged {
                    own,
                    // Added by decision 123 (plan v4, V3-5).
                    search,
                    operation: StagedOperation::Consume(StagedConsume {
                        source: source.clone(),
                        channels,
                        group: group.clone(),
                        patterns,
                        continuation,
                        persist,
                        peeks,
                        fire: Some(StagedConsumeFire {
                            group,
                            candidates,
                            comm: comm.clone(),
                            removed,
                            // Retired by decision 90 (C6, step D): the field is gone.
                            // recorded,
                        }),
                    }),
                    before_observe,
                    effects,
                });
                // Added by decision 102 (plan v4 C3b).
                self.take_step(step);
                Ok(StagedOp {
                    index,
                    source: OpSource::Consume(source),
                    comm: Some(comm),
                })
            }
        }
    }
}

// Added by decision 102 (plan v4 C3b): the owned identity of the source
// `source`.
fn owned_source(source: RSpaceOperationSource<'_>) -> OpSource {
    match source {
        RSpaceOperationSource::Produce(produce) => OpSource::Produce(produce.clone()),
        RSpaceOperationSource::Consume(consume) => OpSource::Consume(consume.clone()),
    }
}

// Added by decision 102 (plan v4 C3b): whether `owned` is the identity of the
// source `source`.
fn same_source(owned: &OpSource, source: RSpaceOperationSource<'_>) -> bool {
    match (owned, source) {
        (OpSource::Produce(owned), RSpaceOperationSource::Produce(produce)) => owned == produce,
        (OpSource::Consume(owned), RSpaceOperationSource::Consume(consume)) => owned == consume,
        (OpSource::Produce(_), RSpaceOperationSource::Consume(_)) |
        (OpSource::Consume(_), RSpaceOperationSource::Produce(_)) => false,
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
// original data and persistence. Changed by decision 103 (decision 118, U3
// (i) of decision 122): a produce fire has exactly one, or two or more when
// the datum is persistent (CR-11). Each one carries the channel of the
// produce, so each lies in the fired collection. The text before: "A produce
// fire has exactly one."
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
    // Changed by decision 103 (decision 118, U3 (i) of decision 122): a
    // persistent datum can bind two or more positions of its collection.
    // match incoming {
    //     1 => Ok(()),
    //     _ => Err(SpaceTxnError::InvalidOperation(
    //         "a produce fire needs exactly one incoming candidate",
    //     )),
    // }
    match (incoming, persist) {
        (1, _) | (2.., true) => Ok(()),
        _ => Err(SpaceTxnError::InvalidOperation(
            "a produce fire needs one incoming candidate, or more of a persistent datum in its \
             collection",
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
    // Changed by decision 123 (plan v4, V3-8, R7): the base
    // `produce_comm_with` builds the COMM. Its increment of the incoming count
    // has no check. The only caller with an incoming produce is
    // `ready_produce_fire`, and `ready_produce` runs `counter_room` on the
    // same host count before it. So the increment cannot overflow. The old
    // build:
    // ```
    // let produces: Vec<Produce> = candidates
    //     .iter()
    //     .map(|candidate| candidate.datum.source.clone())
    //     .collect();
    // let mut live = counters(&produces);
    // if let Some(source) = incoming {
    //     if produces.contains(source) {
    //         let count = live.entry(source.clone()).or_insert(0);
    //         *count = count.checked_add(1).ok_or(SpaceTxnError::CounterOverflow)?;
    //     }
    // }
    // let comm = COMM::new(candidates, consume.clone(), peeks.clone(), |listed: &[Produce]| {
    //     lookup_counters(&live, listed)
    // });
    // ```
    let comm = produce_comm_with(
        candidates,
        consume,
        peeks,
        incoming.map(|produce| (produce, false)),
        counters,
    );
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
pub(crate) fn committed_candidates<S, C, P, A, K>(
    store: &S,
    candidates: &[ConsumeCandidate<C, A>],
    keys: &[Blake2b256Hash],
) -> Result<Vec<ConsumeCandidate<C, A>>, SpaceTxnError>
where
    S: TxnStore<C, P, A, K> + ?Sized,
    C: Clone + Hash + Eq + Serialize,
    P: Clone,
    A: Clone + Serialize,
    K: Clone,
{
    let mut live: BTreeMap<&Blake2b256Hash, Vec<Datum<A>>> = BTreeMap::new();
    for (candidate, key) in candidates.iter().zip(keys) {
        if candidate.datum_index >= 0 && !live.contains_key(key) {
            live.insert(key, store.data(&candidate.channel, key)?);
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
/// The comparison reads the shared continuations of the store. `keys` are
/// the channel keys of the group `channels`.
pub(crate) fn committed_continuation_index<S, C, P, A, K>(
    store: &S,
    channels: &[C],
    keys: &[Blake2b256Hash],
    continuation: &WaitingContinuation<P, K>,
) -> Result<i32, SpaceTxnError>
where
    S: TxnStore<C, P, A, K> + ?Sized,
    C: Clone + Hash + Eq + Serialize,
    P: Clone + Serialize,
    A: Clone,
    K: Clone + Serialize,
{
    let position = first_equal_continuation(&store.continuations(channels, keys)?, continuation)?
        .ok_or(SpaceTxnError::Inconsistent(
        "the fired continuation is absent from the hot store",
    ))?;
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

pub(crate) fn ready_produce_fire<S, C, P, A, K>(
    store: &S,
    fire: StagedFire<C, P, A, K>,
    incoming: Option<&Produce>,
    counters: impl Fn(&[Produce]) -> BTreeMap<Produce, i32>,
) -> Result<ReadyFire<C, P, A, K>, SpaceTxnError>
where
    S: TxnStore<C, P, A, K> + ?Sized,
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
            let live = committed_continuation_index(
                store,
                &fire.channels,
                &fire.group,
                &fire.continuation,
            )?;
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

pub(crate) fn ready_consume_fire<S, C, P, A, K>(
    store: &S,
    fire: StagedConsumeFire<C, A>,
    consume: &Consume,
    peeks: &BTreeSet<i32>,
    counters: impl Fn(&[Produce]) -> BTreeMap<Produce, i32>,
) -> Result<ReadyConsumeFire<C, A>, SpaceTxnError>
where
    S: TxnStore<C, P, A, K> + ?Sized,
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

/// Added by decision 124 (L15, L16, plan v4 C7.15, C7.16, section 2.10 item
/// 3): readies the actual fire `fire` of a produce row, which the row search
/// found in place of the staged fire, against the store after the rows before
/// it. A fresh view takes the steps of the fire arm of `stage_produce`, and
/// `ready_produce_fire` readies the result, so the commit path of a staged
/// fire commits it unchanged. `incoming` is the produce of the row when it is
/// not persistent. It gives the ready fire and its view effects.
pub(crate) fn ready_actual_produce_fire<B, S, C, P, A, K>(
    backend: &B,
    store: &S,
    reads: RowReads<'_>,
    incoming: Option<&Produce>,
    fire: (Vec<C>, WaitingContinuation<P, K>, Vec<ConsumeCandidate<C, A>>),
) -> Result<(ReadyFire<C, P, A, K>, Vec<StoreEffect>), SpaceTxnError>
where
    B: TxnBackend<C, P, A, K>,
    S: TxnStore<C, P, A, K> + ?Sized,
    C: Clone + Debug + Serialize + Hash + Eq,
    P: Clone + Debug + Serialize,
    A: Clone + Serialize,
    K: Clone + Debug + Serialize,
{
    let (channels, continuation, candidates) = fire;
    let group = canonical_group_key(&channels)?;
    for key in &group {
        reads.require(key)?;
    }
    let mut view = TxnView::default();
    for (key, channel) in group.iter().zip(&channels) {
        view.load_data(store, key, channel)?;
        view.load_joins(store, key, channel)?;
    }
    let waiting = view.load_continuations(store, &group, &channels)?;
    let Some(_) = first_equal_continuation(waiting, &continuation)? else {
        return Err(SpaceTxnError::NotInView("the waiting continuation"));
    };
    view.check_stored_candidates(&group, &candidates)?;
    let positions = view.claim_positions(&group, &candidates)?;
    let counters = view.comm_counters(backend, &candidates, incoming)?;
    let comm = COMM::new(
        &candidates,
        continuation.source.clone(),
        continuation.peeks.clone(),
        |produces| lookup_counters(&counters, produces),
    );
    let removed = removed_records(&candidates, &positions);
    let (effects, continuation_position) =
        view.apply_fire_to_view(&group, &continuation, &candidates, &removed)?;
    let staged = StagedFire {
        channels,
        group,
        continuation,
        candidates,
        comm,
        // Retired by decision 90 (C6, step D): the field is gone.
        // recorded: None,
        removed,
        continuation_position,
    };
    let ready =
        ready_produce_fire(store, staged, incoming, |produces| host_counters(backend, produces))?;
    Ok((ready, effects))
}

/// Added by decision 124 (L15, L16, plan v4 C7.15, C7.16, section 2.10 item
/// 3): readies the actual fire `candidates` of a consume row on `channels`,
/// which the row search found in place of the staged fire, against the store
/// after the rows before it. A fresh view takes the steps of the fire arm of
/// `stage_consume` (`TxnView::ready_fire`), and `ready_consume_fire` readies
/// the result. `source` is the consume of the row and its peeks. It gives the
/// ready fire and its view effects.
pub(crate) fn ready_actual_consume_fire<B, S, C, P, A, K>(
    backend: &B,
    store: &S,
    reads: RowReads<'_>,
    source: (&Consume, &BTreeSet<i32>),
    channels: &[C],
    candidates: Vec<ConsumeCandidate<C, A>>,
) -> Result<(ReadyConsumeFire<C, A>, Vec<StoreEffect>), SpaceTxnError>
where
    B: TxnBackend<C, P, A, K>,
    S: TxnStore<C, P, A, K> + ?Sized,
    C: Clone + Debug + Serialize + Hash + Eq,
    P: Clone + Debug + Serialize,
    A: Clone + Serialize,
    K: Clone + Debug + Serialize,
{
    let group = canonical_group_key(channels)?;
    for key in &group {
        reads.require(key)?;
    }
    let mut view = TxnView::default();
    let (positions, comm) =
        view.ready_fire(backend, store, &group, channels, &candidates, source)?;
    let removed = removed_records(&candidates, &positions);
    let effects = view.remove_records(&group, &candidates, &removed)?;
    let (consume, peeks) = source;
    let staged = StagedConsumeFire {
        group,
        candidates,
        comm,
        removed,
        // Retired by decision 90 (C6, step D): the field is gone.
        // recorded: None,
    };
    let ready = ready_consume_fire(store, staged, consume, peeks, |produces| {
        host_counters(backend, produces)
    })?;
    Ok((ready, effects))
}

/// Added by decision 124 (L15, L16, plan v4 C7.15, C7.16, section 2.10 item
/// 3): the view effects of the park of `waiting` on `channels`, which a
/// consume row commits in place of its staged fire, against the store after
/// the rows before it. A fresh view takes the steps of the park arm of
/// `stage_consume`.
pub(crate) fn actual_park_effects<S, C, P, A, K>(
    store: &S,
    reads: RowReads<'_>,
    channels: &[C],
    waiting: WaitingContinuation<P, K>,
) -> Result<Vec<StoreEffect>, SpaceTxnError>
where
    S: TxnStore<C, P, A, K> + ?Sized,
    C: Clone + Debug + Serialize + Hash + Eq,
    P: Clone + Debug + Serialize,
    A: Clone + Serialize,
    K: Clone + Debug + Serialize,
{
    let group = canonical_group_key(channels)?;
    for key in &group {
        reads.require(key)?;
    }
    let mut view = TxnView::default();
    view.load_continuations(store, &group, channels)?;
    for (key, channel) in group.iter().zip(channels) {
        view.load_joins(store, key, channel)?;
    }
    view.park_in_view(&group, channels, waiting)
}

// Added by decision 124 (L15, L16, plan v4 C7.15, C7.16): the host counters of
// `produces`, as the readying of a staged fire takes them (`ready_produce`).
fn host_counters<B, C, P, A, K>(backend: &B, produces: &[Produce]) -> BTreeMap<Produce, i32>
where
    B: TxnBackend<C, P, A, K>,
    C: Clone + Hash + Eq,
    P: Clone,
    A: Clone,
    K: Clone,
{
    let mut counters = BTreeMap::new();
    for produce in produces {
        counters.insert(produce.clone(), backend.host_counter(produce));
    }
    counters
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
/// `observe_operation_start`. The channel hash of the source is the store
/// identity of the channel, so the join read computes no key.
pub(crate) fn ready_produce<S, C, P, A, K>(
    store: &S,
    produce: StagedProduce<C, P, A, K>,
    host_counter: i32,
    counters: impl Fn(&[Produce]) -> BTreeMap<Produce, i32>,
) -> Result<(ReadyProduce<C, P, A, K>, Vec<Vec<C>>), SpaceTxnError>
where
    S: TxnStore<C, P, A, K> + ?Sized,
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
    let joins = store.joins(&channel, &source.channel_hash)?;
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
    /// The key of the group `channels`.
    pub(crate) group: Vec<Blake2b256Hash>,
    pub(crate) patterns: Vec<P>,
    pub(crate) continuation: K,
    pub(crate) persist: bool,
    pub(crate) peeks: BTreeSet<i32>,
    pub(crate) fire: Option<ReadyConsumeFire<C, A>>,
}

/// Read-only commit checks of a staged consume, before any effect of the
/// operation: a fire is located in the live store with its COMM recomputed
/// from `counters`.
pub(crate) fn ready_consume<S, C, P, A, K>(
    store: &S,
    consume: StagedConsume<C, P, A, K>,
    counters: impl Fn(&[Produce]) -> BTreeMap<Produce, i32>,
) -> Result<ReadyConsume<C, P, A, K>, SpaceTxnError>
where
    S: TxnStore<C, P, A, K> + ?Sized,
    C: Clone + Hash + Eq + Serialize,
    P: Clone,
    A: Clone + Serialize,
    K: Clone,
{
    let StagedConsume {
        source,
        channels,
        group,
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
        group,
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

/// Added by decision 92: the join groups `joins` of a row footprint and the
/// footprint key `footprint_key` as one more group. The recorder sorts and
/// de-duplicates the footprint, and the predecessor of a row is the last
/// row on each of its footprint channels. So the rows of one space form one
/// chain in play order (`SpaceReplayOrder.tla`, `Footprint`).
pub(crate) fn footprint_joins<C: Clone>(
    joins: &[Vec<C>],
    footprint_key: Option<&C>,
) -> Vec<Vec<C>> {
    let mut groups = Vec::with_capacity(joins.len() + 1);
    groups.extend(joins.iter().cloned());
    if let Some(key) = footprint_key {
        groups.push(vec![key.clone()]);
    }
    groups
}

// Retired by decision 90 (C6, step D): the rigged replay path and the play
// paths that called it retire, and `receipt_completion` serves the frame of
// `apply_row`. The text before:
// ```text
// pub(crate) fn completion<C, P, A, K>(
//     result: &Result<SpaceOpReceipt<C, P, A, K>, SpaceTxnError>,
// ) -> RSpaceOperationCompletion {
//     match result {
//         Ok(SpaceOpReceipt::Stored { .. }) | Ok(SpaceOpReceipt::Parked { .. }) => {
//             RSpaceOperationCompletion::Stored
//         }
//         Ok(SpaceOpReceipt::Fired { .. }) => RSpaceOperationCompletion::Matched,
//         Err(_) => RSpaceOperationCompletion::Rejected,
//     }
// }
// ```

/// Added by decision 124 (L11, L16, plan v4 C7.15): the completion of a
/// committed row, which the frame of `apply_row` reads.
pub(crate) fn receipt_completion<C, P, A, K>(
    receipt: &SpaceOpReceipt<C, P, A, K>,
) -> RSpaceOperationCompletion {
    match receipt {
        SpaceOpReceipt::Stored { .. } | SpaceOpReceipt::Parked { .. } => {
            RSpaceOperationCompletion::Stored
        }
        SpaceOpReceipt::Fired { .. } => RSpaceOperationCompletion::Matched,
        // Added by decision 123 (plan v4 C3d): a skipped row stored nothing.
        // The base records its refused introduction as Rejected (R8).
        SpaceOpReceipt::Skipped { .. } => RSpaceOperationCompletion::Rejected,
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

// Retired by decision 123 (plan v4, V3-8): space_play.rs calls the base
// functions of ops_produce.rs and ops_consume.rs, so no code calls the four
// copies below. The retired text:
// ```
// // The store steps of the host, `store_data`, `remove_matched_datum_and_join`,
// // `store_waiting_continuation` and `store_persistent_data`, are private to
// // ops_produce.rs and ops_consume.rs, so they are re-stated here over the
// // public `HotStore` API, statement for statement. Play and native replay
// // apply every effect through them, so their effects are equal by
// // construction (decision 47, design v2, S2).
//
// // ops_produce.rs `store_data`.
// pub(crate) fn store_data<C, P, A, K>(
//     store: &dyn HotStore<C, P, A, K>,
//     channel: C,
//     data: A,
//     persist: bool,
//     produce_ref: Produce,
// ) where
//     C: Clone + Hash + Eq,
//     P: Clone,
//     A: Clone,
//     K: Clone,
// {
//     store.put_datum(&channel, Datum {
//         a: data,
//         persist,
//         source: produce_ref,
//     });
// }
//
// // ops_produce.rs `remove_matched_datum_and_join`.
// pub(crate) fn remove_matched_datum_and_join<C, P, A, K>(
//     store: &dyn HotStore<C, P, A, K>,
//     channels: &[C],
//     data_candidates: &[ConsumeCandidate<C, A>],
// ) where
//     C: Clone + Hash + Eq,
//     P: Clone,
//     A: Clone,
//     K: Clone,
// {
//     let mut sorted_candidates: Vec<_> = data_candidates.iter().collect();
//     sorted_candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.datum_index));
//     for consume_candidate in sorted_candidates {
//         let channel = &consume_candidate.channel;
//         let datum_index = consume_candidate.datum_index;
//
//         if datum_index >= 0 &&
//             !consume_candidate.datum.persist &&
//             store.remove_datum(channel, datum_index).is_err()
//         {
//             continue;
//         }
//         store.remove_join(channel, channels);
//     }
// }
//
// // Disabled by decision 45 (Rule E, "Apply everywhere"): no fire keeps its
// // records, so the retained form of `remove_matched_datum_and_join` has no
// // caller.
// //
// // ```
// // // The retained (peek) form of `remove_matched_datum_and_join`: the records
// // // stay and only the joins of the fired group follow the continuation.
// // fn remove_joins<C, P, A, K>(
// //     store: &dyn HotStore<C, P, A, K>,
// //     channels: &[C],
// //     data_candidates: &[ConsumeCandidate<C, A>],
// // ) where
// //     C: Clone + Hash + Eq,
// //     P: Clone,
// //     A: Clone,
// //     K: Clone,
// // {
// //     for consume_candidate in by_descending_index(data_candidates) {
// //         store.remove_join(&consume_candidate.channel, channels);
// //     }
// // }
// // ```
//
// // ops_consume.rs `store_waiting_continuation`. Returns whether the store
// // took the continuation, which the waiting-continuation estimate of native
// // replay counts, as the replay form of this step does.
// pub(crate) fn store_waiting_continuation<C, P, A, K>(
//     store: &dyn HotStore<C, P, A, K>,
//     channels: Vec<C>,
//     wc: WaitingContinuation<P, K>,
// ) -> bool
// where
//     C: Clone + Hash + Eq,
//     P: Clone,
//     A: Clone,
//     K: Clone,
// {
//     let inserted = store.put_continuation(&channels, wc).unwrap_or(false);
//     for channel in channels.iter() {
//         store.put_join(channel, &channels);
//     }
//     inserted
// }
//
// // ops_consume.rs `store_persistent_data`.
// pub(crate) fn store_persistent_data<C, P, A, K>(
//     store: &dyn HotStore<C, P, A, K>,
//     data_candidates: &[ConsumeCandidate<C, A>],
// ) where
//     C: Clone + Hash + Eq,
//     P: Clone,
//     A: Clone,
//     K: Clone,
// {
//     let mut sorted_candidates: Vec<_> = data_candidates.iter().collect();
//     sorted_candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.datum_index));
//     for consume_candidate in sorted_candidates {
//         if !consume_candidate.datum.persist {
//             let _ =
//                 store.remove_datum(&consume_candidate.channel, consume_candidate.datum_index);
//         }
//     }
// }
// ```

// Commits one staged operation and correlates its staged effects with the
// events it emitted. The state check of the entries that it removes runs in
// `apply_op` (`ready_produce`, `ready_consume`) before any of its effects.
// Both checks cost O(changed entries) and run in every build profile, so
// debug and release commit identical results (FR-7').
// Changed by decision 92: the operation runs with the footprint key of its
// transaction.
// fn commit_operation<B, C, P, A, K>(
//     backend: &B,
//     store: &B::Store,
//     index: usize,
//     staged: Staged<C, P, A, K>,
// ) -> Result<SpaceOpReceipt<C, P, A, K>, SpaceTxnError>
// Changed by decision 124 (L11, L16, plan v4 section 2.10 item 4): the row
// runs with its search and the reads of that search, and its events
// correlate with the effects that it applied.
fn commit_operation<B, C, P, A, K>(
    backend: &B,
    store: &B::Store,
    reads: RowReads<'_>,
    index: usize,
    staged: Staged<C, P, A, K>,
    footprint_key: Option<&C>,
) -> Result<AppliedRow<C, P, A, K>, SpaceTxnError>
where
    B: TxnBackend<C, P, A, K>,
    C: Clone + Hash + Eq,
    P: Clone,
    A: Clone,
    K: Clone,
{
    // let Staged {
    //     operation,
    //     before_observe,
    //     effects,
    // } = staged;
    // let mut events = Vec::with_capacity(2);
    // let receipt = backend.apply_op(store, operation, before_observe, &mut
    // events)?;
    let Staged {
        operation,
        before_observe,
        effects,
        own: _,
        // Added by decision 123 (plan v4, V3-5) and decision 124 (L11, L16):
        // the row passes its search to `apply_row`.
        search,
    } = staged;
    let mut events = Vec::with_capacity(2);
    // Changed by decision 124 (L11, L16): the text before:
    // check_event_correlation(index, &effects, &events)?;
    // Ok(receipt)
    let applied = backend.apply_row(
        store,
        operation,
        before_observe,
        footprint_key,
        search.map(|search| (search, reads)),
        &mut events,
    )?;
    check_event_correlation(
        index,
        applied.parts.effects.as_deref().unwrap_or(&effects),
        &events,
        applied.parts.deviated,
    )?;
    Ok(applied)
}

/// Added by decision 124 (plan v4 C7.21, section 2.10 item 5, errata V4-5,
/// V4-7, ruling RADD2v2 6.6): readies the consume request `request` that fires
/// on stored records, a retire of an actual fire or the take of a finish row
/// (P1), against the store after the rows before it, just before its commit.
/// The reads check the phase B keys first (CR-36). The row keeps the machinery
/// search of the request.
fn stage_retire<B, C, P, A, K>(
    backend: &B,
    store: &B::Store,
    reads: RowReads<'_>,
    request: SpaceConsume<C, P, A, K>,
) -> Result<Staged<C, P, A, K>, SpaceTxnError>
where
    B: TxnBackend<C, P, A, K>,
    C: Clone + Debug + Serialize + Hash + Eq,
    P: Clone + Debug + Serialize,
    A: Clone + Serialize,
    K: Clone + Debug + Serialize,
{
    let SpaceConsume {
        channels,
        patterns,
        continuation,
        persist,
        peeks,
        outcome,
        before_observe,
        own,
        search,
    } = request;
    let ConsumeOutcome::Fire {
        data: mut candidates,
    } = outcome
    else {
        return Err(SpaceTxnError::Inconsistent("a retire request is not a fire"));
    };
    let group = canonical_group_key(&channels)?;
    for key in &group {
        reads.require(key)?;
    }
    let mut view = TxnView::default();
    for (key, channel) in group.iter().zip(&channels) {
        view.load_data(store, key, channel)?;
    }
    let positions = view.claim_positions(&group, &candidates)?;
    for (candidate, position) in candidates.iter_mut().zip(positions) {
        let position = position.ok_or(SpaceTxnError::NotInView("a retired record"))?;
        candidate.datum_index = i32::try_from(position)
            .map_err(|_| SpaceTxnError::Inconsistent("a record index overflows i32"))?;
    }
    let source = Consume::create(&channels, &patterns, &continuation, persist);
    let (positions, comm) =
        view.ready_fire(backend, store, &group, &channels, &candidates, (&source, &peeks))?;
    let removed = removed_records(&candidates, &positions);
    let effects = view.remove_records(&group, &candidates, &removed)?;
    Ok(Staged {
        operation: StagedOperation::Consume(StagedConsume {
            source,
            channels,
            group: group.clone(),
            patterns,
            continuation,
            persist,
            peeks,
            fire: Some(StagedConsumeFire {
                group,
                candidates,
                comm,
                removed,
                // Retired by decision 90 (C6, step D): the field is gone.
                // recorded: None,
            }),
        }),
        before_observe,
        effects,
        own,
        search,
    })
}

/// Added by decision 124 (plan v4 C7.11, rule CR-33, ruling RADD2v2 6.6):
/// readies the store request `request` of a finish row (P1) against the store
/// after the rows before it, just before its commit, as `stage_retire` readies
/// a retire. The reads check the phase B keys first (CR-36).
fn stage_store<B, C, P, A, K>(
    backend: &B,
    store: &B::Store,
    reads: RowReads<'_>,
    request: SpaceProduce<C, P, A, K>,
) -> Result<Staged<C, P, A, K>, SpaceTxnError>
where
    B: TxnBackend<C, P, A, K>,
    C: Clone + Debug + Serialize + Hash + Eq,
    P: Clone + Debug + Serialize,
    A: Clone + Serialize,
    K: Clone + Debug + Serialize,
{
    let SpaceProduce {
        channel,
        data,
        persist,
        outcome,
        before_observe,
        own,
        search,
    } = request;
    let ProduceOutcome::Store = outcome else {
        return Err(SpaceTxnError::Inconsistent("a finish store is not a store"));
    };
    let key = channel_key(&channel)?;
    reads.require(&key)?;
    let mut view = TxnView::default();
    let entry = view.load_joins(store, &key, &channel)?;
    for group_key in &entry.keys {
        for joined_key in group_key {
            reads.require(joined_key)?;
        }
    }
    let joins_at_start = entry.groups.clone();
    let source = Produce::create(&channel, &data, persist);
    match persist {
        true => {}
        false => {
            backend
                .host_counter(&source)
                .checked_add(1)
                .ok_or(SpaceTxnError::CounterOverflow)?;
        }
    }
    Ok(Staged {
        operation: StagedOperation::Produce(StagedProduce {
            source: source.clone(),
            channel,
            data,
            persist,
            joins_at_start,
            fire: None,
        }),
        before_observe,
        effects: vec![StoreEffect::PutDatum(source)],
        own,
        search,
    })
}

/// Added by decision 124 (L11, L16, CR-33): commits the row `entry` at
/// `index`, under the sub-order `index` of the parent order when there is
/// one.
async fn commit_row<B, C, P, A, K>(
    backend: &B,
    store: &B::Store,
    reads: RowReads<'_>,
    parent: Option<&OperationOrder>,
    index: usize,
    entry: Staged<C, P, A, K>,
    footprint_key: Option<&C>,
) -> Result<AppliedRow<C, P, A, K>, SpaceTxnError>
where
    B: TxnBackend<C, P, A, K>,
    C: Clone + Hash + Eq,
    P: Clone,
    A: Clone,
    K: Clone,
{
    match parent {
        Some(parent) => {
            operation_context::scope(sub_order(parent, index), async move {
                commit_operation(backend, store, reads, index, entry, footprint_key)
            })
            .await
        }
        None => commit_operation(backend, store, reads, index, entry, footprint_key),
    }
}

/// Added by decision 124 (CR-33, plan v4 section 2.10 item 5, ruling D-11):
/// books one row of the retire group of the unit of row `unit`. It returns
/// the cut of the row, which names row `unit` (L12), and whether the row has
/// a refusal.
fn take_group_row<C, P: Clone, A: Clone, K: Clone>(
    unit: usize,
    row: AppliedRow<C, P, A, K>,
    ops: &mut Vec<SpaceOpReceipt<C, P, A, K>>,
    refused: &mut Vec<(Blake2b256Hash, Blake2b256Hash)>,
) -> (Option<RowDeviation<C>>, bool) {
    let AppliedRow { receipt, parts } = row;
    let RowParts {
        effects: _,
        deviated,
        refused: row_refused,
        item_failed,
        actual,
        can_stop: _,
        can_continue: _,
        vetoing,
        claims: _,
        search: _,
    } = parts;
    let held = !row_refused.is_empty();
    refused.extend(row_refused);
    ops.push(receipt);
    let dispatched = actual.and_then(|fire| fire.dispatched);
    let deviation = deviated.then_some(RowDeviation {
        index: unit,
        item_failed,
        dispatched,
        vetoing,
        // Added by decision 124 (plan v4 C7.11, ruling RADD 1.2): the unit loop
        // sets the tail read of the cut that stops the commit (B.6 row 8b).
        tail_read: TailRead::Ends,
        // Added by decision 124 (plan v4 C7.19, ruling RADD 1.8): row d of a cut
        // in the staged group is c0, so its staged claims stand.
        claims: None,
        // Added by decision 124 (plan v4 C7.11, ruling RADD2 1.1): the unit loop
        // sets the re-issue of the cut that stops the commit.
        reissue: None,
    });
    (deviation, held)
}

// Added by decision 124 (plan v4 C7.11, rules CR-5, CR-6 and CR-18, ruling
// RADD2v2 6.6): true when the row with the search `search` is the last row
// with a search, because no later row `rest` has one. After a record end only
// such a row can be the cut row of play whose finish rows the later rows
// follow (P2).
fn tail_candidate<C, P: Clone, A: Clone, K: Clone>(
    search: Option<&dyn RowSearch<C, P, A, K>>,
    rest: &[Staged<C, P, A, K>],
) -> bool {
    match search {
        None => false,
        Some(_) => rest.iter().all(|entry| entry.search.is_none()),
    }
}

// Added by decision 124 (plan v4 C7.11, rules CR-5, CR-6 and CR-18, rulings
// RADD2v2 6.6 and RADD2 1.2): true when the sources of the staged rows `rest`
// follow the store unit `unit` and the finish rows `finish` in order, as the
// cursor follows the record (P2).
// Changed by CD-2 (clippy -D warnings, the final verification of merge 2):
// each bound of a parameter is in the `where` clause
// (`clippy::multiple_bound_locations`). The text before:
// fn follows_finish<C, P: Clone, A: Clone, K: Clone>(
//     rest: &[Staged<C, P, A, K>],
//     unit: &[OpSource],
//     finish: &[FinishRow<C, P, A, K>],
// ) -> bool
// where
//     C: Clone + Serialize,
//     P: Serialize,
//     A: Serialize,
//     K: Serialize,
fn follows_finish<C, P, A, K>(
    rest: &[Staged<C, P, A, K>],
    unit: &[OpSource],
    finish: &[FinishRow<C, P, A, K>],
) -> bool
where
    C: Clone + Serialize,
    P: Clone + Serialize,
    A: Clone + Serialize,
    K: Clone + Serialize,
{
    let mut sources = unit
        .iter()
        .cloned()
        .chain(finish.iter().map(FinishRow::source));
    rest.iter().all(|entry| {
        sources.any(|source| match (&entry.operation, source) {
            (StagedOperation::Produce(row), OpSource::Produce(other)) => row.source == other,
            (StagedOperation::Consume(row), OpSource::Consume(other)) => row.source == other,
            (StagedOperation::Produce(_), OpSource::Consume(_)) |
            (StagedOperation::Consume(_), OpSource::Produce(_)) => false,
        })
    })
}

// Applies the staged operations in order. A commit under a parent order
// that already committed in this root evaluation fails before any effect;
// then operation `i` runs under `sub_order(parent, i)`. Without an operation
// context the commit is not tracked and emits no sub-orders. A failure of
// operation `i` leaves operations `0..i` applied, as the equivalent host
// program would. An unpaired replay fire fails here too: with the error of
// a call that the observer rejects, or else with `ReplayDivergence` after
// its calls, and with the same prefix (decision 36).
//
// Changed by decision 92: a transaction with a footprint key commits only
// when its first operation is its job's own, so the first row of every
// committed space transaction is unique to its job (`SpaceReplayOrder.tla`,
// the control `AnchorAppendedUnsafe`). The Reified layer stages an anchor
// first when the first operation is not its own. Each operation runs with
// the footprint key.
// async fn commit_staged<B, C, P, A, K>(
//     backend: &B,
//     store: Arc<B::Store>,
//     registration: &TxnRegistration,
//     staged: Vec<Staged<C, P, A, K>>,
// ) -> Result<SpaceTxnReceipt<C, P, A, K>, SpaceTxnError>
// Changed by decision 124 (L11, L12, L16, plan v4 section 2.10 items 5 and
// 6): the commit takes the reads of the row searches, and in native replay
// the cursor. It stops after a cut and books the receipt fields. A denial
// fails earlier, in the apply of its row (V4-19).
async fn commit_staged<B, C, P, A, K>(
    backend: &B,
    store: Arc<B::Store>,
    registration: &TxnRegistration,
    footprint_key: Option<&C>,
    staged: Vec<Staged<C, P, A, K>>,
    reads: RowReads<'_>,
    replay: Option<ReplayCursor>,
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
    // Added by decision 92.
    if footprint_key.is_some() {
        match staged.first() {
            Some(first) if first.own => {}
            Some(_) | None => {
                return Err(SpaceTxnError::InvalidOperation(
                    "the first operation of a space transaction is not its job's own",
                ));
            }
        }
    }
    // Added by decision 90 (DR-132): the readiness check of the whole
    // transaction runs before the parent order is admitted, so a transaction
    // that is not ready leaves no trace and can run again.
    // Changed by decision 92: the check reads the first operation only.
    if let Some(first) = staged.first() {
        backend.check_ready(&first.operation)?;
    }
    let parent = operation_context::current();
    if let Some(parent) = &parent {
        registration.admit_parent(parent, &store)?;
    }
    // Changed by decision 124 (L11, L12, L16, plan v4 section 2.10 items 5
    // and 6): the commit applies each row with the reads of its search,
    // checks the cut, and books the receipt fields. `ended` holds after a
    // record end or a CR-32 end of native replay. The text before:
    // let mut ops = Vec::with_capacity(staged.len());
    let count = staged.len();
    let ended = matches!(replay, Some(ReplayCursor::Ended));
    let mut ops = Vec::with_capacity(count);
    let mut refused = Vec::new();
    let mut deviation = None;
    // Changed by decision 124 (CR-33, plan v4 section 2.10 items 5 and 6): a
    // unit is a row and the retire group of its actual fire. With the staged
    // fire c0 the staged group commits as staged. Another actual fire drops
    // the staged group, and each of its retires is readied against the store
    // just before its commit (V4-7). The text before:
    // for (index, entry) in staged.into_iter().enumerate() {
    let parent = parent.as_ref();
    let mut rows = staged.into_iter();
    while let Some(entry) = rows.next() {
        let index = count - 1 - rows.len();
        let group = entry
            .search
            .as_ref()
            .map_or(0, |search| search.staged_retires());
        let end = index + 1 + group;
        // Changed by decision 92: each operation takes the footprint key.
        // let receipt = match &parent {
        //     Some(parent) => {
        //         let store = &*store;
        //         operation_context::scope(sub_order(parent, index), async move {
        //             commit_operation(backend, store, index, entry)
        //         })
        //         .await?
        //     }
        //     None => commit_operation(backend, &*store, index, entry)?,
        // };
        // Changed by decision 124 (L11, L16): each row takes the reads of its
        // search.
        let applied =
            commit_row(backend, &*store, reads, parent, index, entry, footprint_key).await?;
        let AppliedRow { receipt, parts } = applied;
        let RowParts {
            effects: _,
            deviated,
            refused: row_refused,
            item_failed,
            actual,
            can_stop,
            can_continue,
            vetoing,
            claims,
            mut search,
        } = parts;
        let (retires, dispatched) = match actual {
            Some(ActualFire {
                retires,
                dispatched,
            }) => (retires, dispatched),
            None => (Vec::new(), None),
        };
        refused.extend(row_refused);
        ops.push(receipt);
        // Added by decision 124 (L12, ruling D-11): every cut of the unit
        // names row d of the unit. Row d keeps its pairs for a stop at a
        // record end, as the loop of part 1 does (CR-20).
        let row_cut = RowDeviation {
            index,
            item_failed,
            dispatched,
            vetoing,
            // Added by decision 124 (plan v4 C7.11, ruling RADD 1.2): the unit
            // loop sets the tail read of the cut that stops the commit.
            tail_read: TailRead::Ends,
            // Added by decision 124 (plan v4 C7.19, ruling RADD 1.8): the claims
            // of the actual unit of a deviating row d.
            claims,
            // Added by decision 124 (plan v4 C7.11, ruling RADD2 1.1): the unit
            // loop sets the re-issue of the cut that stops the commit.
            reissue: None,
        };
        let mut group_cut = None;
        let mut group_refused = false;
        match (deviated, retires.is_empty()) {
            (true, false) => {
                for (offset, request) in retires.into_iter().enumerate() {
                    let at = index + 1 + offset;
                    let row = stage_retire(backend, &*store, reads, request)?;
                    let applied =
                        commit_row(backend, &*store, reads, parent, at, row, footprint_key).await?;
                    let (_, held) = take_group_row(index, applied, &mut ops, &mut refused);
                    group_refused |= held;
                }
            }
            (true, true) => {}
            (false, _) => {
                let mut parked_before = false;
                for (offset, mut entry) in rows.by_ref().take(group).enumerate() {
                    let at = index + 1 + offset;
                    // Added by decision 124 (CR-33, FR-7'): a retire that
                    // parked leaves its record, so the staged positions of
                    // the later retires no longer name the store. Each record
                    // is still located by its bytes.
                    match (parked_before, &mut entry.operation) {
                        // Changed by CD-2 (clippy -D warnings, the final
                        // verification of merge 2): the empty arm comes
                        // first (`clippy::single_match`). The text before:
                        // (true, StagedOperation::Consume(row)) => match row.fire.as_mut() {
                        //     Some(fire) => fire.removed.clear(),
                        //     None => {}
                        // },
                        // (true, StagedOperation::Produce(_)) | (false, _) => {}
                        (true, StagedOperation::Produce(_)) | (false, _) => {}
                        (true, StagedOperation::Consume(row)) => match row.fire.as_mut() {
                            None => {}
                            Some(fire) => fire.removed.clear(),
                        },
                    }
                    let applied =
                        commit_row(backend, &*store, reads, parent, at, entry, footprint_key)
                            .await?;
                    parked_before |= matches!(applied.receipt, SpaceOpReceipt::Parked { .. });
                    let (cut, held) = take_group_row(index, applied, &mut ops, &mut refused);
                    group_refused |= held;
                    group_cut = group_cut.or(cut);
                }
            }
        }
        // Added by decision 124 (ruling RADD2 1.2, rules CR-31 and CR-33,
        // J-STORE): the store unit of a row that deviates to a store commits
        // after the row and before its tail read, which reads the store after
        // the unit. Replay commits the same rows from its own search.
        // `actual_claims` names their channels, so the claims of the cut stay.
        let unit = match (deviated, search.as_mut()) {
            (true, Some(search)) => search.store_unit(),
            (_, _) => Vec::new(),
        };
        let mut unit_sources = Vec::<OpSource>::with_capacity(unit.len());
        for request in unit {
            unit_sources.push(request.source());
            let at = ops.len();
            let row = match request {
                FinishRow::Store(request) => stage_store(backend, &*store, reads, request)?,
                FinishRow::Take(request) => stage_retire(backend, &*store, reads, request)?,
            };
            let applied =
                commit_row(backend, &*store, reads, parent, at, row, footprint_key).await?;
            take_group_row(index, applied, &mut ops, &mut refused);
        }
        // Added by decision 124 (L11, CR-18): the unit rule of native replay.
        // CanStop(U) holds when CanStop(d) holds or a row of the group has a
        // verified refusal. CanContinue(U) holds when CanContinue(d) holds and
        // no row of the unit has a refusal or a cut. A unit before the last
        // needs CanContinue(U). The last unit needs CanStop(U) after a record
        // end or a CR-32 end. Every other cut fails with the session mismatch.
        // The unit of a cut row also holds the later rows that follow its finish
        // rows (P2, ruling RADD2v2 6.6 (c)).
        let last = end >= count;
        let can_stop = can_stop || group_refused;
        let can_continue = can_continue && !group_refused && !deviated && group_cut.is_none();
        // Changed by decision 124 (plan v4 C7.11, rules CR-18 and CR-33, ruling
        // RADD2v2 6.6): the cut of the unit comes before the check of the unit,
        // because the finish rows of its search can lengthen the unit (P2). After
        // a record end, the last row with a search can cut before the later rows
        // that follow its finish rows (REPLAY-END).
        let rest = match deviated {
            true => rows.as_slice().get(group..).unwrap_or(&[]),
            false => rows.as_slice(),
        };
        // Added by decision 124 (L11, L12, plan v4 C7.11, rulings RADD 1.2 and
        // RADD2 1.1): the cut of row d, else the first cut of its group, else
        // row d after a record end, is the deviation (F4). The commit stops
        // after it. The search of row d reads the tail of the unit after its
        // group, once (B.6 row 8b), and builds the re-issue of its kind. A cut
        // after a record end or a CR-32 end follows one more staging step, so
        // without a search it reads `Stages` (ADD A2.4).
        let (cut, ended_cut, tried) = match (deviated, group_cut, ended) {
            (true, _, _) => (Some(row_cut), false, false),
            (false, Some(cut), _) => (Some(cut), false, false),
            (false, None, true) => match (last, tail_candidate(search.as_deref(), rest)) {
                (true, _) => (Some(row_cut), true, false),
                (false, true) => (Some(row_cut), true, true),
                (false, false) => (None, false, false),
            },
            (false, None, false) => (None, false, false),
        };
        let Some(mut cut) = cut else {
            if let (Some(_), false, false) = (replay, last, can_continue) {
                return Err(SpaceTxnError::Host(
                    crate::rspace::replay_rspace::native_session::mismatch(),
                ));
            }
            continue;
        };
        let mut finish = Vec::new();
        match search {
            None => {
                cut.tail_read = match ended_cut {
                    false => TailRead::Ends,
                    true => TailRead::Stages,
                };
            }
            Some(mut search) => {
                cut.tail_read = backend.tail_read(&*store, &mut *search, reads)?;
                cut.reissue = search.tail_reissue();
                finish = search.finish_rows();
            }
        }
        let followed = match replay {
            Some(_) => follows_finish(rest, &unit_sources, &finish),
            None => false,
        };
        if let (true, false) = (tried, followed) {
            if let (Some(_), false, false) = (replay, last, can_continue) {
                return Err(SpaceTxnError::Host(
                    crate::rspace::replay_rspace::native_session::mismatch(),
                ));
            }
            continue;
        }
        for request in finish {
            let at = ops.len();
            let row = match request {
                FinishRow::Store(request) => {
                    match cut.claims.as_mut() {
                        None => {}
                        Some(claims) => claims.data.push(request.channel.clone()),
                    }
                    stage_store(backend, &*store, reads, request)?
                }
                FinishRow::Take(request) => {
                    match cut.claims.as_mut() {
                        None => {}
                        Some(claims) => claims.data.extend(request.channels.iter().cloned()),
                    }
                    stage_retire(backend, &*store, reads, request)?
                }
            };
            let applied =
                commit_row(backend, &*store, reads, parent, at, row, footprint_key).await?;
            take_group_row(index, applied, &mut ops, &mut refused);
        }
        let last = last || followed;
        if let (Some(_), false, false, _) | (Some(ReplayCursor::Ended), true, _, false) =
            (replay, last, can_continue, can_stop)
        {
            return Err(SpaceTxnError::Host(
                crate::rspace::replay_rspace::native_session::mismatch(),
            ));
        }
        deviation = Some(cut);
        break;
    }
    // Changed by decision 123 (plan v4 C3d, V3-9, errata V4-17) and decision
    // 124 (L11, L12): the receipt books the deviation and the refused pairs.
    // The text before:
    // Ok(SpaceTxnReceipt { ops })
    Ok(SpaceTxnReceipt {
        ops,
        deviation,
        refused,
    })
}

// What an open transaction holds: the striped locks of `begin` and its
// registration with the handle. The fields drop in order: the locks, then
// the registration.
// Changed by decision 94: the transaction takes phase A at its begin and
// phase B at `lock_phase_b`, as the host produce takes its phases.
// struct HeldTxn {
//     _locks: (ChannelLockGuard, ChannelLockGuard),
//     registration: TxnRegistration,
// }
struct HeldTxn {
    _phase_a: ChannelLockGuard,
    phase_b: Option<ChannelLockGuard>,
    registration: TxnRegistration,
}

/// Added by decision 90 (item 3.10, and decision 91 SQ-4): the striped locks
/// and the registration of a committed transaction
/// ([`SpaceTxn::commit_holding`]). The commit has applied its operations, and
/// no other transaction can take a stripe of the transaction until
/// the hold drops. While the hold lives, the task still holds an open
/// transaction of its handle, so it begins no other transaction of that handle.
pub struct TxnHold {
    _held: Option<HeldTxn>,
}

impl TxnHold {
    /// The hold of a transaction that holds no lock.
    pub fn released() -> Self { Self { _held: None } }
}

/// The transaction of every host mode: a staging core, the mode's backend,
/// and what `begin` holds until commit, a fail-closed error, or drop.
pub(crate) struct HostTxn<B, C, P, A, K>
where
    B: TxnBackend<C, P, A, K>,
    C: Clone + Hash + Eq,
    P: Clone,
    A: Clone,
    K: Clone,
{
    core: TxnCore<B::Store, C, P, A, K>,
    backend: B,
    held: Option<HeldTxn>,
}

impl<B, C, P, A, K> HostTxn<B, C, P, A, K>
where
    B: TxnBackend<C, P, A, K>,
    C: Clone + Hash + Eq,
    P: Clone,
    A: Clone,
    K: Clone,
{
    /// A transaction of `backend` over `store`, the current store of its
    /// host.
    ///
    /// Changed by decision 94: the transaction holds phase A (`phase_a`) over
    /// its own lock keys `lock_keys`. It covers no channel until
    /// [`SpaceTxn::lock_phase_b`] takes phase B.
    // pub(crate) fn new(
    //     mode: SpaceMode,
    //     backend: B,
    //     store: Arc<B::Store>,
    //     installed: BTreeSet<Blake2b256Hash>,
    //     coverage: LockCoverage,
    //     locks: (ChannelLockGuard, ChannelLockGuard),
    //     registration: TxnRegistration,
    // ) -> Self {
    //     Self {
    //         core: TxnCore {
    //             mode,
    //             store,
    //             installed,
    //             coverage,
    //             view: TxnView::default(),
    //             paired: Vec::new(),
    //             staged: Vec::new(),
    //             finished: false,
    //         },
    //         backend,
    //         held: Some(HeldTxn {
    //             _locks: locks,
    //             registration,
    //         }),
    //     }
    // }
    pub(crate) fn new(
        mode: SpaceMode,
        backend: B,
        store: Arc<B::Store>,
        installed: BTreeSet<Blake2b256Hash>,
        lock_keys: Vec<C>,
        phase_a: ChannelLockGuard,
        registration: TxnRegistration,
    ) -> Self {
        Self {
            core: TxnCore {
                mode,
                store,
                installed,
                lock_keys,
                coverage: LockCoverage::default(),
                footprint_key: None,
                view: TxnView::default(),
                // Retired by decision 90 (C6, step D): the text before:
                // paired: Vec::new(),
                staged: Vec::new(),
                // Added by decision 102 (plan v4 C3b).
                cursor: ReplayCursor::Unread,
                slots: Vec::new(),
                pending: None,
                finished: false,
            },
            backend,
            held: Some(HeldTxn {
                _phase_a: phase_a,
                phase_b: None,
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
            // Changed by decision 124 (plan v4 C3g bullet 4, ruling D-2, OP-FWD):
            // the caller calls the method of the view. The text before:
            // let count = self.core.view_counter(&self.backend, produce)?;
            let count = self.core.view.view_counter(&self.backend, produce)?;
            counters.insert(produce.clone(), count);
        }
        Ok(counters)
    }

    fn recorded_comms(&mut self, source: RSpaceOperationSource<'_>) -> Option<Vec<COMM>> {
        match self.core.mode {
            // Changed by decision 102 (decision 119 S1, plan v4 C3b): native
            // replay answers from the recorded row at the cursor.
            // SpaceMode::Play | SpaceMode::NativeReplay => None,
            SpaceMode::Play => None,
            // Added by decision 102 (decision 119 S1, plan v4 C3b): the recorded
            // row of `source` at the cursor (rules CR-4 and CR-5). A Stored row
            // names no COMM. A Matched or DeniedComm row names its COMM. Another
            // row, or no row, gives `None`, so the pick takes DR-75 order.
            SpaceMode::NativeReplay => match self.core.pending_row(&self.backend, source)? {
                (_, NativeReplayOutcome::Stored, _) => Some(Vec::new()),
                (_, NativeReplayOutcome::Matched | NativeReplayOutcome::DeniedComm, Some(comm)) => {
                    Some(vec![comm])
                }
                (_, NativeReplayOutcome::Matched | NativeReplayOutcome::DeniedComm, None) |
                (
                    _,
                    NativeReplayOutcome::DeniedIntroduction |
                    NativeReplayOutcome::SkippedIntroduction,
                    _,
                ) => None,
            },
            // Retired by decision 90 (C6, step D): only the rigged replay modes
            // paired the COMMs of a rigged log, and the modes retire. Native
            // replay answers from its recorded row (the arm above). The text
            // before:
            // ```
            // SpaceMode::Replay | SpaceMode::Reporting => {
            //     let key = match source {
            //         RSpaceOperationSource::Produce(produce) => IOEvent::Produce(produce.clone()),
            //         RSpaceOperationSource::Consume(consume) => IOEvent::Consume(consume.clone()),
            //     };
            //     let mut available = Vec::new();
            //     for (comm, count) in self.backend.recorded(&key) {
            //         let recorded = match binding_multiplicity(&key, &comm) {
            //             0 => 0,
            //             multiplicity => count / multiplicity,
            //         };
            //         let paired = self
            //             .core
            //             .paired
            //             .iter()
            //             .filter(|paired| **paired == comm)
            //             .count();
            //         for _ in paired..recorded {
            //             available.push(comm.clone());
            //         }
            //     }
            //     available.sort_by_cached_key(|comm| bincode::serialize(comm).unwrap_or_default());
            //     Some(available)
            // }
            // ```
        }
    }

    // Added by decision 124 (F2, base one row source for each produce,
    // replay_rspace.rs:1058-1082): native replay gives the journal index of
    // the pending row of `source` (`RecordedRow`), the row that
    // `recorded_comms` reads. The other modes read no journal index.
    fn recorded_row_index(&mut self, source: RSpaceOperationSource<'_>) -> Option<usize> {
        match self.core.mode {
            SpaceMode::NativeReplay => self
                .core
                .pending_row(&self.backend, source)
                .map(|(index, _, _)| index),
            SpaceMode::Play => None,
        }
    }

    fn admits(
        &mut self,
        comm: &COMM,
        datum: &Datum<A>,
        incoming: bool,
    ) -> Result<bool, SpaceTxnError> {
        match self.core.mode {
            // Changed by decision 102 (rules CR-4 and R13): native replay names a
            // record by the COMM of its recorded row, as replay does.
            // SpaceMode::Play | SpaceMode::NativeReplay => Ok(true),
            SpaceMode::Play => Ok(true),
            // Changed by decision 123 (plan v4, V3-8, R13): the test is the base
            // `candidate_matches_with` over the staged counters. A counter
            // overflow fails with `CounterOverflow`, as before.
            // SpaceMode::Replay | SpaceMode::Reporting => {
            //     if !comm.produces.contains(&datum.source) {
            //         return Ok(false);
            //     }
            //     if datum.persist {
            //         return Ok(true);
            //     }
            //     let recorded = comm.times_repeated.get(&datum.source).copied().unwrap_or(0);
            //     let staged = self.core.view_counter(&self.backend, &datum.source)?;
            //     let expected = match incoming {
            //         true => staged
            //             .checked_add(1)
            //             .ok_or(SpaceTxnError::CounterOverflow)?,
            //         false => staged,
            //     };
            //     Ok(expected == recorded)
            // }
            SpaceMode::NativeReplay => {
                let core = &self.core;
                let backend = &self.backend;
                candidate_order::candidate_matches_with(
                    datum,
                    incoming,
                    |source| comm.produces.contains(source),
                    |source| comm.times_repeated.get(source).copied(),
                    |source| core.view.view_counter(backend, source),
                )?
                .ok_or(SpaceTxnError::CounterOverflow)
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

    // Added by decision 94: phase B over the own lock keys and the scope keys,
    // in one sorted call of the backend. The coverage holds the exact keys.
    async fn lock_phase_b(&mut self, scope_keys: &[C]) -> Result<(), SpaceTxnError> {
        self.core.ensure_open()?;
        let held = self.held.as_mut().ok_or(SpaceTxnError::Finished)?;
        if held.phase_b.is_some() {
            return Err(SpaceTxnError::InvalidOperation("the transaction already holds phase B"));
        }
        let mut keys = Vec::with_capacity(self.core.lock_keys.len() + scope_keys.len());
        keys.extend(self.core.lock_keys.iter().cloned());
        keys.extend(scope_keys.iter().cloned());
        let coverage = LockCoverage::of(&keys)?;
        let hashes: Vec<u64> = keys.iter().map(striped_locks::channel_hash).collect();
        let guard = self.backend.lock_phase_b(&hashes).await?;
        held.phase_b = Some(guard);
        self.core.coverage = coverage;
        Ok(())
    }

    // Added by decision 92: the footprint key is one of the own lock keys, so
    // both phases hold it.
    fn set_footprint_key(&mut self, key: C) -> Result<(), SpaceTxnError> {
        self.core.ensure_open()?;
        if self.core.footprint_key.is_some() {
            return Err(SpaceTxnError::InvalidOperation("the footprint key is already set"));
        }
        let wanted = channel_key(&key)?;
        for lock_key in &self.core.lock_keys {
            if channel_key(lock_key)? == wanted {
                self.core.footprint_key = Some(key);
                return Ok(());
            }
        }
        Err(SpaceTxnError::InvalidOperation("the footprint key is not an own lock key"))
    }

    // Added by decision 92: the staged view starts again from the store, which
    // the locks keep unchanged.
    fn discard_staged(&mut self) -> Result<(), SpaceTxnError> {
        self.core.ensure_open()?;
        self.core.staged.clear();
        // Retired by decision 90 (C6, step D): the text before:
        // self.core.paired.clear();
        self.core.view = TxnView::default();
        // Added by decision 102 (plan v4 C3b, F6): the cursor starts again, so
        // the next stage call takes the first open row of its source.
        self.core.cursor = ReplayCursor::Unread;
        self.core.slots.clear();
        self.core.pending = None;
        Ok(())
    }

    // Added by plan v4 errata (V4-12, C9 of the anchor-only arbiter): in native
    // replay, the row of a leading anchor is not a followed row (rule CR-5).
    // When the staged list holds only the anchor and the cursor follows its
    // row, the cursor becomes `Anchored`. Otherwise the call changes nothing.
    fn mark_leading_anchor(&mut self) {
        if let (SpaceMode::NativeReplay, 1, ReplayCursor::Following(row)) =
            (self.core.mode, self.core.staged.len(), self.core.cursor)
        {
            self.core.cursor = ReplayCursor::Anchored(row);
        }
    }

    // Added by plan v4 errata (V4-12): the query of rule CR-32. With the cursor
    // `Following`, a staging error ends the staging: the cursor becomes
    // `Ended`, and the call returns true. With every other cursor the call
    // returns false and changes nothing, so the staging error takes the
    // failure path (the error rule of CR-5).
    fn end_record(&mut self) -> bool {
        match self.core.cursor {
            ReplayCursor::Following(_) => {
                self.core.cursor = ReplayCursor::Ended;
                true
            }
            ReplayCursor::Unread |
            ReplayCursor::Anchored(_) |
            ReplayCursor::Free |
            ReplayCursor::Ended => false,
        }
    }

    // Changed by decision 90 (item 3.10, and decision 91 SQ-4): the commit is
    // `commit_holding` with a hold that drops at once.
    // async fn commit(&mut self) -> Result<SpaceTxnReceipt<C, P, A, K>,
    // SpaceTxnError> {     self.core.ensure_open()?;
    //     let held = self.held.take().ok_or(SpaceTxnError::Finished)?;
    //     self.core.finished = true;
    //     let staged = std::mem::take(&mut self.core.staged);
    //     // After commit the host counters and recorded bindings are
    // authoritative.     self.core.paired.clear();
    //     self.core.view.count_delta.clear();
    //     let store = Arc::clone(&self.core.store);
    //     let result = commit_staged(&self.backend, store, &held.registration,
    // staged).await;     drop(held);
    //     result
    // }
    async fn commit(&mut self) -> Result<SpaceTxnReceipt<C, P, A, K>, SpaceTxnError> {
        self.commit_holding().await.map(|(receipt, hold)| {
            drop(hold);
            receipt
        })
    }

    // Added by decision 90 (item 3.10, and decision 91 SQ-4): a failed commit drops
    // `held` on the error path, so it releases its stripes at once. A
    // successful commit returns them.
    async fn commit_holding(
        &mut self,
    ) -> Result<(SpaceTxnReceipt<C, P, A, K>, TxnHold), SpaceTxnError> {
        self.core.ensure_open()?;
        let held = self.held.take().ok_or(SpaceTxnError::Finished)?;
        self.core.finished = true;
        let staged = std::mem::take(&mut self.core.staged);
        // After commit the host counters and recorded bindings are authoritative.
        // Retired by decision 90 (C6, step D): the text before:
        // self.core.paired.clear();
        self.core.view.count_delta.clear();
        let store = Arc::clone(&self.core.store);
        // Changed by decision 92: the commit runs with the footprint key.
        let footprint_key = self.core.footprint_key.clone();
        // Changed by decision 124 (L11, L16, plan v4 section 2.10 item 5): the
        // commit takes the reads of the row searches, and the cursor in native
        // replay.
        let reads = RowReads {
            coverage: &self.core.coverage,
            installed: &self.core.installed,
        };
        let replay = match self.core.mode {
            SpaceMode::NativeReplay => Some(self.core.cursor),
            SpaceMode::Play => None,
        };
        let receipt = commit_staged(
            &self.backend,
            store,
            &held.registration,
            footprint_key.as_ref(),
            staged,
            reads,
            replay,
        )
        .await?;
        Ok((receipt, TxnHold { _held: Some(held) }))
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

/// The swept groups of a volatile sweep, by group key: the group of each
/// swept channel, and every join group of a swept channel.
pub(crate) type SweptGroups<C> = BTreeMap<Vec<Blake2b256Hash>, Vec<C>>;

/// The read-only validation of a volatile sweep: no channel is installed,
/// every stored join group of a swept channel is canonical and is swept as a
/// whole, and no swept key has a leaf at the history root. It returns the
/// swept groups. Every mode validates with it before any effect.
pub(crate) fn plan_sweep<S, C, P, A, K>(
    store: &S,
    reader: &dyn HistoryReader<Blake2b256Hash, C, P, A, K>,
    installed: &BTreeSet<Blake2b256Hash>,
    channels: &BTreeMap<Blake2b256Hash, C>,
) -> Result<SweptGroups<C>, SpaceTxnError>
where
    S: TxnStore<C, P, A, K> + ?Sized,
    C: Clone + Serialize + Hash + Eq,
    P: Clone,
    A: Clone,
    K: Clone,
{
    if channels.keys().any(|key| installed.contains(key)) {
        return Err(SpaceTxnError::InstalledChannel);
    }
    let mut groups: SweptGroups<C> = BTreeMap::new();
    for (key, channel) in channels {
        groups.insert(vec![key.clone()], vec![channel.clone()]);
        for group in store.joins(channel, key)? {
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
    Ok(groups)
}

/// The number of join groups of the swept channels.
pub(crate) fn swept_join_count<S, C, P, A, K>(
    store: &S,
    channels: &BTreeMap<Blake2b256Hash, C>,
) -> Result<usize, SpaceTxnError>
where
    S: TxnStore<C, P, A, K> + ?Sized,
    C: Clone + Hash + Eq,
    P: Clone,
    A: Clone,
    K: Clone,
{
    let mut count = 0usize;
    for (key, channel) in channels {
        count += store.joins(channel, key)?.len();
    }
    Ok(count)
}

/// The residue check of a volatile sweep: after its effects, every swept
/// channel holds no data and no join, and every swept group holds no
/// waiting continuation.
pub(crate) fn check_swept<S, C, P, A, K>(
    store: &S,
    channels: &BTreeMap<Blake2b256Hash, C>,
    groups: &SweptGroups<C>,
) -> Result<(), SpaceTxnError>
where
    S: TxnStore<C, P, A, K> + ?Sized,
    C: Clone + Hash + Eq,
    P: Clone,
    A: Clone,
    K: Clone,
{
    let residue = SpaceTxnError::Inconsistent("the volatile sweep left residue");
    for (key, channel) in channels {
        if !store.data(channel, key)?.is_empty() || !store.joins(channel, key)?.is_empty() {
            return Err(residue);
        }
    }
    for (group_key, group) in groups {
        if !store.continuations(group, group_key)?.is_empty() {
            return Err(residue);
        }
    }
    Ok(())
}

/// The volatile sweep of a hot store, in play, replay and reporting. The
/// validation of [`plan_sweep`] runs first. Then continuations, joins and
/// data are removed through the hot store, so the keys end empty and
/// `changes()` emits deletes of keys absent from the trie (root neutral).
/// `on_continuation_removed` runs once per removed continuation, and the
/// report counts what was actually removed.
pub(crate) fn sweep_volatile_channels<C, P, A, K>(
    hot: &dyn HotStore<C, P, A, K>,
    reader: &dyn HistoryReader<Blake2b256Hash, C, P, A, K>,
    installed: &BTreeSet<Blake2b256Hash>,
    channels: &BTreeMap<Blake2b256Hash, C>,
    mut on_continuation_removed: impl FnMut(),
) -> Result<SweepReport, SpaceTxnError>
where
    C: Clone + Serialize + Hash + Eq + 'static,
    P: Clone + 'static,
    A: Clone + 'static,
    K: Clone + 'static,
{
    let groups = plan_sweep(hot, reader, installed, channels)?;
    let mut report = SweepReport {
        channels: channels.len(),
        ..SweepReport::default()
    };
    for group in groups.values() {
        let waiting = hot.get_continuations_arc(group).len();
        for index in (0..waiting).rev() {
            let index = i32::try_from(index)
                .map_err(|_| SpaceTxnError::Inconsistent("a continuation index exceeds i32"))?;
            hot.remove_continuation(group, index)
                .ok_or(SpaceTxnError::Inconsistent("a swept continuation could not be removed"))?;
            report.continuations += 1;
            on_continuation_removed();
        }
    }
    for channel in channels.values() {
        let joined = hot.get_joins(channel);
        for group in &joined {
            hot.remove_join(channel, group);
        }
        let removed = joined
            .len()
            .checked_sub(hot.get_joins(channel).len())
            .ok_or(SpaceTxnError::Inconsistent("the volatile sweep added a join"))?;
        report.joins += removed;
    }
    for channel in channels.values() {
        let records = hot.get_data(channel).len();
        for index in (0..records).rev() {
            let index = i32::try_from(index)
                .map_err(|_| SpaceTxnError::Inconsistent("a record index exceeds i32"))?;
            hot.remove_datum(channel, index)?;
            report.data += 1;
        }
    }
    check_swept(hot, channels, &groups)?;
    Ok(report)
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

    // The indices of `values` in the order of
    // `candidate_order::canonical_candidates`.
    fn canonical_indices<D: CandidateSource + Serialize>(values: Vec<D>) -> Vec<usize> {
        candidate_order::canonical_candidates(values)
            .into_iter()
            .map(|(_, index)| index as usize)
            .collect()
    }

    proptest::proptest! {
        // DR-75 (decision 41): the order of a space agent is the canonical
        // candidate order, also with tie runs of equal source hashes. The
        // order of the stored records and one more record is the order of the
        // list that holds the record last.
        #[test]
        fn canonical_data_order_is_the_canonical_candidate_order(
            data in candidate_order::candidate_strategy()
        ) {
            proptest::prop_assert_eq!(canonical_data_order(&data), canonical_indices(data.clone()));
            if let Some((extra, stored)) = data.split_last() {
                proptest::prop_assert_eq!(
                    canonical_data_order_with(stored, extra),
                    canonical_indices(data.clone())
                );
            }
        }
    }

    #[test]
    fn canonical_continuation_order_is_the_canonical_candidate_order() {
        let waiters = vec![
            wk(&["a"], "k3", BTreeSet::new()),
            wk(&["a"], "k1", BTreeSet::new()),
            wk(&["a"], "k2", BTreeSet::from([0])),
        ];
        assert_eq!(canonical_continuation_order(&waiters), canonical_indices(waiters.clone()));
        assert_eq!(
            canonical_continuation_order(&[] as &[WaitingContinuation<String, String>]),
            Vec::<usize>::new()
        );
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
            // Changed by decision 124 (K18): the check takes the deviated flag.
            // The text before:
            // check_event_correlation(3, &effects, &[]),
            check_event_correlation(3, &effects, &[], false),
            Err(SpaceTxnError::EventCoverage {
                op: 3,
                rule: "stored datum without its produce event",
            })
        );
        let events = vec![Event::IoEvent(IOEvent::Produce(source))];
        // Changed by decision 124 (K18): the check takes the deviated flag. The
        // text before:
        // assert_eq!(check_event_correlation(3, &effects, &events), Ok(()));
        assert_eq!(check_event_correlation(3, &effects, &events, false), Ok(()));
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
            // Changed by decision 124 (K18): the check takes the deviated flag.
            // The text before:
            // check_event_correlation(0, &effects, &early),
            check_event_correlation(0, &effects, &early, false),
            Err(SpaceTxnError::EventCoverage {
                op: 0,
                rule: "COMM precedes its trigger event",
            })
        );
        let ordered = vec![Event::IoEvent(IOEvent::Consume(consume)), Event::Comm(comm)];
        // Changed by decision 124 (K18): the check takes the deviated flag. The
        // text before:
        // assert_eq!(check_event_correlation(0, &effects, &ordered), Ok(()));
        assert_eq!(check_event_correlation(0, &effects, &ordered, false), Ok(()));
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
    // Disabled by decision 94: the coverage holds the exact keys of phase B,
    // not their stripes, so a channel that shares the stripe of a key is not
    // covered. `space_store_handle_lock_coverage_holds_the_exact_phase_b_keys`
    // checks the exact coverage.
    // #[tokio::test]
    // async fn space_store_handle_lock_coverage_matches_acquired_stripes() {
    //     let stripes = striped_locks::new_striped_locks();
    //     let keys = names(&["a"]);
    //     let hashes: Vec<u64> =
    // keys.iter().map(striped_locks::channel_hash).collect();     let coverage
    // = LockCoverage::new(&hashes, stripes.len(), stripes.len());     let _held
    // = striped_locks::acquire_locks(&stripes, &hashes).await;
    //     let shares_stripe = |channel: &String| {
    //         (striped_locks::channel_hash(channel) as usize) % stripes.len() ==
    //             (hashes[0] as usize) % stripes.len()
    //     };
    //     let sibling = (0..1_000_000)
    //         .map(|index| format!("probe-{}", index))
    //         .find(|channel| shares_stripe(channel))
    //         .expect("a channel that shares the stripe of a");
    //     let mut probes: Vec<String> = (0..64).map(|index| format!("channel-{}",
    // index)).collect();     probes.push(keys[0].clone());
    //     probes.push(sibling.clone());
    //     let mut covered = Vec::with_capacity(probes.len());
    //     for channel in &probes {
    //         let probe_hashes = [striped_locks::channel_hash(channel)];
    //         let mut acquire = Box::pin(striped_locks::acquire_locks(&stripes,
    // &probe_hashes));         let blocked = matches!(futures::poll!(&mut
    // acquire), Poll::Pending);         let held = coverage.require(channel,
    // LockUse::Consumed).is_ok();         assert_eq!(blocked, held, "channel
    // {}", channel);         if held {
    //             covered.push(channel.clone());
    //         }
    //     }
    //     assert!(covered.contains(&keys[0]) && covered.contains(&sibling));
    //     assert!(covered.len() < probes.len());
    //     for lock_use in [LockUse::Produced, LockUse::Joined, LockUse::Consumed] {
    //         assert_eq!(coverage.require(&sibling, lock_use), Ok(()));
    //     }
    //     let empty = LockCoverage::new(&[], stripes.len(), stripes.len());
    //     assert_eq!(empty.require(&keys[0], LockUse::Produced),
    // Err(SpaceTxnError::UnlockedChannel));     let unstriped =
    // LockCoverage::new(&hashes, 0, 0);     assert_eq!(
    //         unstriped.require(&keys[0], LockUse::Joined),
    //         Err(SpaceTxnError::UnlockedChannel)
    //     );
    // }

    // Added by decision 94: the coverage holds exactly the channel keys of
    // phase B (`JoinScopeLock.tla`, `Covered`, and the control
    // `CoverageByHostUseUnsafe`). While the lock of "a" is held, a channel
    // that shares its stripe blocks on the lock, and the coverage still does
    // not hold the channel. The empty coverage holds no channel.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_store_handle_lock_coverage_holds_the_exact_phase_b_keys() {
        let stripes = striped_locks::new_striped_locks();
        let keys = names(&["a", "b"]);
        let hashes: Vec<u64> = keys.iter().map(striped_locks::channel_hash).collect();
        let coverage = LockCoverage::of(&keys).expect("the coverage of the phase B keys");
        let _held = striped_locks::acquire_locks(&stripes, &hashes).await;
        let key_of = |channel: &String| channel_key(channel).expect("the channel key");
        for key in &keys {
            assert_eq!(coverage.require(&key_of(key)), Ok(()), "key {}", key);
        }
        let shares_stripe = |channel: &String| {
            (striped_locks::channel_hash(channel) as usize) % stripes.len() ==
                (hashes[0] as usize) % stripes.len()
        };
        let sibling = (0..1_000_000)
            .map(|index| format!("probe-{}", index))
            .find(|channel| shares_stripe(channel))
            .expect("a channel that shares the stripe of a");
        let sibling_hashes = [striped_locks::channel_hash(&sibling)];
        let mut acquire = Box::pin(striped_locks::acquire_locks(&stripes, &sibling_hashes));
        assert!(
            matches!(futures::poll!(&mut acquire), Poll::Pending),
            "the sibling blocks on the stripe of a"
        );
        assert_eq!(coverage.require(&key_of(&sibling)), Err(SpaceTxnError::UnlockedChannel));
        let empty = LockCoverage::default();
        assert_eq!(empty.require(&key_of(&keys[0])), Err(SpaceTxnError::UnlockedChannel));
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

    // Added by decision 123 (plan v4 C3d): a skipped row stored nothing, so
    // its completion is Rejected, as the base records a refused introduction
    // (R8).
    #[test]
    fn completion_maps_a_skipped_row_to_rejected() {
        let skipped: SpaceOpReceipt<String, String, String, String> = SpaceOpReceipt::Skipped {
            source: OpSource::Produce(Produce::create(&"a", &"x", false)),
        };
        assert_eq!(receipt_completion(&skipped), RSpaceOperationCompletion::Rejected);
    }

    /// Added by decision 124 (plan v4 C7.11, CR-31, ruling RADD2 1.1): the
    /// carrier gives its value to its own type only, and two values are equal
    /// when they share one allocation.
    #[test]
    fn row_reissue_hands_out_a_value_of_its_type() {
        let reissue = RowReissue::new(7_u32);
        assert_eq!(reissue.get::<u32>(), Some(&7));
        assert_eq!(reissue.get::<u64>(), None);
        assert_eq!(reissue.clone(), reissue);
        assert_ne!(RowReissue::new(7_u32), reissue);
        assert_eq!(format!("{reissue:?}"), "RowReissue");
    }

    // Added by decision 124 (ADD A1.16, OP-LISTS): the reads of a row search
    // over an empty store.
    fn no_data<C>(_channel: &C) -> Result<Vec<Datum<String>>, SpaceTxnError> { Ok(Vec::new()) }

    fn no_continuations(
        _channels: &[String],
    ) -> Result<Vec<Arc<WaitingContinuation<String, String>>>, SpaceTxnError> {
        Ok(Vec::new())
    }

    fn no_joins<C>(_channel: &C) -> Result<Vec<Vec<String>>, SpaceTxnError> { Ok(Vec::new()) }

    fn no_counter(_produce: &Produce) -> i32 { 0 }

    fn empty_lists(
        named: Option<&dyn NativeCandidateIdentity>,
    ) -> RowLists<'_, String, String, String, String> {
        RowLists::new(
            &no_data::<String>,
            &no_continuations,
            &no_joins::<String>,
            named,
            &no_counter,
        )
    }

    // Added by decision 124 (ADD A1.16, OP-LISTS): a recorded identity that
    // names the records of `named`, each with repetition 0.
    struct NamedProduces {
        named: Vec<Produce>,
    }

    impl NativeCandidateIdentity for NamedProduces {
        fn matches_consume(&self, _source: &Consume) -> bool { false }

        fn matches_produce(&self, source: &Produce) -> bool { self.named.contains(source) }

        fn repetition(&self, _source: &Produce) -> Option<i32> { Some(0) }

        fn matches_comm(&self, _source: &COMM) -> bool { false }
    }

    // Added by decision 124 (ADD A1.16, OP-LISTS): play keeps every index of
    // the order, and a recorded identity puts the records that it names
    // first. Each part keeps the order that the search gave.
    #[test]
    fn row_lists_pick_order_shuffles_in_play_and_names_first_in_replay() {
        let data =
            vec![record("c", "r0", false), record("c", "r1", false), record("c", "r2", false)];
        let play = empty_lists(None);
        let mut picked = play
            .pick_order(&data, None, vec![2, 0, 1])
            .expect("the pick order of play");
        picked.sort_unstable();
        assert_eq!(picked, vec![0, 1, 2]);
        let identity = NamedProduces {
            named: vec![data[1].source.clone()],
        };
        let replay = empty_lists(Some(&identity));
        assert_eq!(
            replay
                .pick_order(&data, None, vec![2, 1, 0])
                .expect("the pick order of replay"),
            vec![1, 2, 0]
        );
    }

    // Added by decision 124 (ADD A2.9): a search that overrides no provided
    // method of `RowSearch`, so its tail read, its re-issue and its finish
    // rows are the defaults.
    struct BareSearch;

    impl RowSearch<String, String, String, String> for BareSearch {
        fn runs(&self) -> Vec<RowRun<String>> { Vec::new() }

        fn continuations(
            &mut self,
            _run: usize,
            _lists: &RowLists<'_, String, String, String, String>,
        ) -> Result<Vec<(Arc<WaitingContinuation<String, String>>, i32)>, SpaceTxnError> {
            Ok(Vec::new())
        }

        fn tuple(
            &mut self,
            _run: usize,
            _lists: &RowLists<'_, String, String, String, String>,
            _request: RowTupleRequest<'_, String, String, String, String>,
        ) -> Result<Option<Vec<ConsumeCandidate<String, String>>>, SpaceTxnError> {
            Ok(None)
        }

        fn restore(&mut self) {}

        fn commit_check(
            &mut self,
            _request: &RowTupleRequest<'_, String, String, String, String>,
            _tuple: &[ConsumeCandidate<String, String>],
        ) -> Result<bool, SpaceTxnError> {
            Ok(false)
        }

        fn may_veto(
            &self,
            _matcher: &dyn Match<String, String, String>,
            _continuation: &String,
        ) -> bool {
            false
        }

        fn staged_retires(&self) -> usize { 0 }

        fn actual_fire(
            &mut self,
            _request: &RowTupleRequest<'_, String, String, String, String>,
            _tuple: &[ConsumeCandidate<String, String>],
        ) -> ActualFire<String, String, String, String> {
            ActualFire {
                retires: Vec::new(),
                dispatched: None,
            }
        }

        fn item_failures(
            &self,
            _lists: &RowLists<'_, String, String, String, String>,
            _matcher: &dyn Match<String, String, String>,
            _rule: &dyn RowItemRule<String, String>,
        ) -> Result<Vec<(Blake2b256Hash, Blake2b256Hash)>, SpaceTxnError> {
            Ok(Vec::new())
        }
    }

    struct BindAll;

    impl Match<String, String, String> for BindAll {
        fn get(&self, _pattern: &String, datum: &String) -> Option<String> { Some(datum.clone()) }
    }

    // Added by decision 124 (ADD A2.9): a `RowSearch` that does not override
    // the method answers `TailRead::Ends`.
    #[test]
    fn tail_read_of_a_search_without_an_override_ends() {
        assert_eq!(BareSearch.tail_read(&empty_lists(None), &BindAll), Ok(TailRead::Ends));
    }

    // Added by decision 124 (ruling RADD2 1.1): a `RowSearch` that does not
    // override the method leaves the re-issue to its agent.
    #[test]
    fn tail_reissue_of_a_search_without_an_override_is_none() {
        assert_eq!(BareSearch.tail_reissue(), None);
    }

    // Added by decision 124 (ruling RADD2 1.2): a `RowSearch` that does not
    // override the methods keeps the record of its row and runs no unit.
    #[test]
    fn store_unit_of_a_search_without_an_override_is_empty() {
        assert!(BareSearch.keeps_staged_record());
        assert!(BareSearch.store_unit().is_empty());
    }

    // Added by decision 124 (V4-7, L10): the store of a commit test: the data
    // of each channel, no continuations and no joins.
    #[derive(Default)]
    struct RowStore {
        records: Mutex<HashMap<String, Vec<Datum<String>>>>,
    }

    impl RowStore {
        fn hold(&self, channel: &str, records: Vec<Datum<String>>) {
            self.records
                .lock()
                .expect("the records of the test store")
                .insert(channel.to_string(), records);
        }
    }

    impl TxnStore<String, String, String, String> for RowStore {
        fn data(
            &self,
            channel: &String,
            _key: &Blake2b256Hash,
        ) -> Result<Vec<Datum<String>>, SpaceTxnError> {
            let records = self.records.lock().expect("the records of the test store");
            match records.get(channel) {
                Some(held) => Ok(held.clone()),
                None => Ok(Vec::new()),
            }
        }

        fn continuations(
            &self,
            _channels: &[String],
            _keys: &[Blake2b256Hash],
        ) -> Result<Vec<Arc<WaitingContinuation<String, String>>>, SpaceTxnError> {
            Ok(Vec::new())
        }

        fn joins(
            &self,
            _channel: &String,
            _key: &Blake2b256Hash,
        ) -> Result<Vec<Vec<String>>, SpaceTxnError> {
            Ok(Vec::new())
        }
    }

    // Added by decision 124 (L10, CR-18): a backend whose every row is a
    // skipped row that can stop a replay. It counts the rows that it applies.
    #[derive(Default)]
    struct RowBackend {
        store: Arc<RowStore>,
        applied: std::sync::atomic::AtomicUsize,
    }

    impl TxnBackend<String, String, String, String> for RowBackend {
        type Store = RowStore;

        fn store(&self) -> Arc<RowStore> { Arc::clone(&self.store) }

        fn host_counter(&self, _produce: &Produce) -> i32 { 0 }

        // Retired by decision 90 (C6, step D): the trait no longer asks for the
        // rigged log.

        fn apply_op(
            &self,
            _store: &RowStore,
            _operation: StagedOperation<String, String, String, String>,
            _before_observe: Option<BeforeObserve>,
            _footprint_key: Option<&String>,
            _events: &mut Vec<Event>,
        ) -> Result<SpaceOpReceipt<String, String, String, String>, SpaceTxnError> {
            Err(SpaceTxnError::Inconsistent("this backend applies whole rows only"))
        }

        fn apply_row(
            &self,
            _store: &RowStore,
            operation: StagedOperation<String, String, String, String>,
            _before_observe: Option<BeforeObserve>,
            _footprint_key: Option<&String>,
            _search: Option<RowSearchWithReads<'_, String, String, String, String>>,
            _events: &mut Vec<Event>,
        ) -> Result<AppliedRow<String, String, String, String>, SpaceTxnError> {
            self.applied
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let source = match operation {
                StagedOperation::Produce(row) => OpSource::Produce(row.source),
                StagedOperation::Consume(row) => OpSource::Consume(row.source),
            };
            Ok(skipped_row(source, true))
        }

        async fn lock_phase_b(&self, _hashes: &[u64]) -> Result<ChannelLockGuard, SpaceTxnError> {
            Err(SpaceTxnError::Inconsistent("a test of the unit loop takes no locks"))
        }
    }

    // Added by decision 124: a `TxnBackend` without the override answers
    // `TailRead::Ends`.
    #[test]
    fn tail_read_of_a_backend_without_an_override_ends() {
        let backend = RowBackend::default();
        let coverage = LockCoverage::default();
        let installed = BTreeSet::new();
        let reads = RowReads {
            coverage: &coverage,
            installed: &installed,
        };
        let store = backend.store();
        assert_eq!(backend.tail_read(&*store, &mut BareSearch, reads), Ok(TailRead::Ends));
    }

    // The fire of a row that `stage_retire` readied.
    fn retire_fire(
        row: Staged<String, String, String, String>,
    ) -> StagedConsumeFire<String, String> {
        let StagedOperation::Consume(StagedConsume {
            fire: Some(fire), ..
        }) = row.operation
        else {
            panic!("a retire row is a consume fire");
        };
        fire
    }

    // Added by decision 124 (V4-7): a retire reads the position of its record
    // from the store after the rows before it, and it does not keep the
    // position that its search gave. A record that an earlier row took
    // fails the retire.
    #[test]
    fn stage_retire_readies_against_the_store_after_the_rows_before_it() {
        let r0 = record("c", "r0", false);
        let r1 = record("c", "r1", false);
        let retire = || SpaceConsume {
            channels: names(&["c"]),
            patterns: names(&["_"]),
            continuation: "retire".to_string(),
            persist: false,
            peeks: BTreeSet::new(),
            outcome: ConsumeOutcome::Fire {
                data: vec![bind_candidate("c".to_string(), &r1, "r1".to_string(), 1)],
            },
            before_observe: None,
            own: false,
            search: None,
        };
        let backend = RowBackend::default();
        let coverage = LockCoverage::of(&names(&["c"])).expect("the coverage of the channel");
        let installed = BTreeSet::new();
        let reads = RowReads {
            coverage: &coverage,
            installed: &installed,
        };
        let store = backend.store();

        store.hold("c", vec![r0, r1.clone()]);
        let row = stage_retire(&backend, &*store, reads, retire()).expect("a retire over both");
        let fire = retire_fire(row);
        assert_eq!(fire.candidates[0].datum_index, 1);
        assert_eq!(fire.removed, vec![(0, 1)]);

        store.hold("c", vec![r1.clone()]);
        let row = stage_retire(&backend, &*store, reads, retire()).expect("a retire over r1");
        assert_eq!(row.effects, vec![StoreEffect::RemoveDatum(r1.source.clone())]);
        let fire = retire_fire(row);
        assert_eq!(fire.candidates[0].datum_index, 0);
        assert_eq!(fire.removed, vec![(0, 0)]);

        store.hold("c", Vec::new());
        assert_eq!(
            stage_retire(&backend, &*store, reads, retire()).err(),
            Some(SpaceTxnError::Inconsistent("a bound record is absent from the view"))
        );
    }

    // A staged store of `data` on `channel`, with no search and no effects.
    fn staged_store_row(channel: &str, data: &str) -> Staged<String, String, String, String> {
        let channel = channel.to_string();
        let data = data.to_string();
        let source = Produce::create(&channel, &data, false);
        Staged {
            operation: StagedOperation::Produce(StagedProduce {
                source,
                channel,
                data,
                persist: false,
                joins_at_start: Vec::new(),
                fire: None,
            }),
            before_observe: None,
            effects: Vec::new(),
            own: false,
            search: None,
        }
    }

    // Added by decision 124 (L10, CR-18): a skipped row cuts the commit. Play
    // stops after it. In replay it can stop a unit only at a record end, and
    // it never continues one, so a later row fails with the session mismatch.
    #[tokio::test]
    async fn skipped_row_cuts_the_commit() {
        let coverage = LockCoverage::default();
        let installed = BTreeSet::new();
        let reads = RowReads {
            coverage: &coverage,
            installed: &installed,
        };
        let registration = TxnRegistration::register(&Arc::new(HandleState::default()))
            .expect("register the transaction");
        let skipped = Produce::create(&"c".to_string(), &"x".to_string(), false);

        let backend = RowBackend::default();
        let rows = vec![staged_store_row("c", "x"), staged_store_row("c", "y")];
        let receipt =
            commit_staged(&backend, backend.store(), &registration, None, rows, reads, None)
                .await
                .expect("a play commit stops at its skipped row");
        assert_eq!(receipt.ops, vec![SpaceOpReceipt::Skipped {
            source: OpSource::Produce(skipped.clone()),
        }]);
        assert_eq!(receipt.refused, Vec::new());
        let deviation = receipt.deviation.expect("the skipped row is the cut");
        assert_eq!(deviation.index, 0);
        assert_eq!(deviation.tail_read, TailRead::Ends);
        assert_eq!(backend.applied.load(std::sync::atomic::Ordering::SeqCst), 1);

        let backend = RowBackend::default();
        let rows = vec![staged_store_row("c", "x")];
        let ended = Some(ReplayCursor::Ended);
        let receipt =
            commit_staged(&backend, backend.store(), &registration, None, rows, reads, ended)
                .await
                .expect("a skipped row can stop at a record end");
        assert_eq!(receipt.deviation.map(|cut| cut.index), Some(0));

        let backend = RowBackend::default();
        let rows = vec![staged_store_row("c", "x"), staged_store_row("c", "y")];
        let free = Some(ReplayCursor::Free);
        let failed =
            commit_staged(&backend, backend.store(), &registration, None, rows, reads, free).await;
        assert_eq!(
            failed.err(),
            Some(SpaceTxnError::Host(crate::rspace::replay_rspace::native_session::mismatch()))
        );
    }
}
