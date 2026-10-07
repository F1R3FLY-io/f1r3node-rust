//! The generic native space (P4 design B.4, P5 design v3 A.3): an outer store and a discipline
//! over the substrate.
//!
//! [`GenericSpace<S, D>`](GenericSpace) runs the collection operations of the family whose outer
//! store is `S` and whose inner collection is `D`. The
//! [`FamilyAgent`](super::families::FamilyAgent) reads the config of a space once and sends each
//! operation to the `GenericSpace` of the family that the config names (P5 design v3, A.3.4).
//!
//! A collection operation runs in one transaction:
//!
//! 1. `begin` with the lock keys `[C(sid), S(sid), S(sid,k), D(sid,k)]`: every channel that the
//!    operation can touch (readiness review S2).
//! 2. Read the config record on `C(sid)`. Without it the operation fails with `DanglingName` for
//!    a space name and with `DanglingHandle` for every other channel.
//! 3. Check the family of the config (`FamilyUnavailable`). Then classify the operation
//!    ([`FamilySpace::classify`]): a collection whose data channel holds no join group other
//!    than `[D(sid,k)]` runs the steps below. Every other operation, a join and a settle with a
//!    cascade run in the join engine of P5c (`spaces::join`), which escalates the locks to the
//!    join closure first ([`prepare_collection_op`]).
//! 4. Before any staging, check the routed key with the outer store (`OuterStore::check_key`).
//!    Then check the named params and the query with the schema of `D`: the keys
//!    (`UnknownParam`), a query that `D` does not take (`QuerySpecUnsupported`), and the values
//!    (`Discipline::check_values`).
//! 5. Bump the collection counter when `D` uses one (`counter::bump`). The value before the bump
//!    is the `ord` of the operation. A re-issued persistent operation keeps the `ord` of its
//!    first round instead (`operation_ord`, decision 43). Without a counter, every `ord` is 0.
//! 6. Plan and stage the operation: `D::offer` for a produce, `D::receive` for a consume, and
//!    `D::settle_job` for a settle. The default plans are the head driver of this module
//!    ([`offer_head`], [`receive_head`]), which the queue uses (decision 11).
//! 7. The job commits the transaction exactly once (`Prepared::commit`). A produce and a consume
//!    stage at least a store, a park or a fire, so their commits are never empty (readiness
//!    review S3).
//! 8. The receipts become the `SpaceOutcome`: the fired continuations without their envelopes,
//!    in staging order, and the re-issue. A counter fire, a retirement and a lift are never
//!    dispatched.
//!
//! The produce plan of the head driver is the `OFFER` of the substrate design (decision 30):
//!
//! ```text
//! if the datum does not reach the head of the records: stage Store, stop    ▷ its FIFO place
//! loop:
//!     W ← the head waiter, or stage Store and stop
//!     if W does not match the payload, or its guard refuses: stage Store, stop   ▷ W blocks
//!     stage Fire(datum, W)
//!     after_fire(datum spent, W stays):
//!         Done: stop
//!         Continue: offer the datum to the next head waiter (the re-offer)
//!         Reissue(cause): re-issue the produce as a new Intent::Space, stop
//! ```
//!
//! The datum is spent when it is not persistent and `W` does not peek, and `W` stays when it is
//! persistent. The consume plan is symmetric: the continuation waits behind earlier waiters, then
//! meets the head records one by one. It is spent when it is not persistent, and the head stays
//! when it is persistent or the consume peeks. A peek fire removes a non-persistent head record,
//! as the host COMM does. The discipline then stores the same record again right after the fire
//! (`Discipline::restore`), as the host reducer's `produce_peeks` does (decision 45, Rule E). The
//! re-store registers the record's own authority and bumps no counter. A divergent pair
//! re-issues after one round (A10), and a chain re-issues after `FIRE_ROUND_CAP` rounds (I-21).
//! Each round makes at most two metered matcher calls, and the bump makes one more. A store or a
//! park on a side that holds `D::CAPACITY` entries fails with `CellOccupied` before it stages
//! anything (F15).
//!
//! A re-issued persistent operation keeps the `ord` of its first round (decision 43), so its
//! record, its identity and its place stay the same in every round. Each round still bumps the
//! counter. Other operations can run between two rounds, so the element re-enters at its place
//! ([`Cx::at_head`]): at the head when its side is empty or its `ord` precedes the head of its
//! side (`Discipline::precedes`), and otherwise stored or parked behind the head, with no fire.
//! A fresh operation and a re-issued datum that is not persistent take the `ord` of their bump,
//! so they reach the head only of an empty side. While a persistent element is in flight, the
//! heads can match only when the element precedes the head of its side, and every completed
//! deploy ends with no element in flight (`HeadOnlyDisciplines.v`, `HeadOnlyDiscipline.tla`).
//!
//! The plans read the staged view, so play and replay stage the same operations: the heads come
//! from the `ord` of the envelopes, never from the list order of the store. A plan reaches the
//! transaction only through [`Cx`]. Each staging primitive of `Cx` sends its operation through
//! the legacy chain of decision 45 (Option P) and registers an introduction (P4 design B.7). A
//! re-offer and a re-fire repeat an identity with its first authority (readiness review S6).
//!
//! An operation reserves each of its reads and copies on the meter of the job before it makes it
//! (decision 42): the ids of its channels, the config decode, the envelope decode of each entry
//! of a head scan, each comparison, the copy of the head, and the copies of each staged
//! operation. It copies only the head that it meets, and a fire that ends the loop moves its
//! values instead of copying them. The outcome after the commit only moves values out of its
//! receipt, and the operation prepays that work at staging, so a host-work rejection never
//! follows the commit (`AgentReadCoverage.v`).

use std::collections::{BTreeMap, BTreeSet};
use std::mem::size_of;
use std::sync::{Arc, LazyLock};

use async_trait::async_trait;
use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::{BindPattern, CostAuthority, ListParWithRandom, Par, TaggedContinuation};
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::internal::{ConsumeCandidate, Datum, WaitingContinuation};
use rspace_plus_plus::rspace::space_store_handle::{
    canonical_continuation_order, canonical_data_order, incoming_candidate, ConsumeOutcome,
    ProduceOutcome, SpaceConsume, SpaceOpReceipt, SpaceProduce,
};
use rspace_plus_plus::rspace::trace::event::Produce;
use serde::Serialize;
use shared::rust::collection_backing::tree_backing;

use super::agent::{
    AllocOp, Allocation, ConsumeOp, JoinOp, ParSpaceReceipt, ParSpaceTxn, Prepared, ProduceOp,
    QueryParams, SettleOp, SpaceAgent, SpaceCreation, SpaceEnv, SpaceFire, SpaceOp, SpaceOutcome,
    SpaceReissue,
};
use super::counter::{self, BumpContext, CounterChannels, CounterOp};
use super::errors::SpaceError;
use super::factory::defaults::suggestion;
use super::join::{self, JoinRoute, JoinScope};
use super::matching::SpaceMatcher;
use super::namespace::{self, CollectionKey, NativeSpace, RandomPurpose, ReifiedId};
use super::record::{self, Introduction, OperationAuthority};
use super::settle::reissued_operation;
use super::types::config::SpaceConfig;
use crate::rust::interpreter::reified::costs::{
    FireRounds, FireStep, ReissueCause, ScanMeter, FIRE_ROUND_CAP,
};
use crate::rust::interpreter::reified::legacy_charges::LegacyChain;

/// A record of a collection in the staged view.
pub type ParDatum = Datum<ListParWithRandom>;

/// A waiting continuation of a collection in the staged view.
pub type ParWaiter = WaitingContinuation<BindPattern, TaggedContinuation>;

/// A candidate of a staged fire.
pub type ParCandidate = ConsumeCandidate<Par, ListParWithRandom>;

/// The outer store of a family: how a channel names a collection, and where the records and the
/// counter of a collection live.
pub trait OuterStore: Default + Send + Sync + 'static {
    /// The `outer` name of the family in the config record.
    const NAME: &'static str;

    /// True when a `Join` operation of the store always runs in a new transaction over the lock
    /// keys of its scope. The first transaction of an operation locks the channels of its routed
    /// key. A store whose scope never holds these channels (the PathMap store) escalates every
    /// join operation (joins plan section 9).
    const ALWAYS_ESCALATES: bool = false;

    /// CLASSIFY of a collection operation of the store (joins plan section 4.1). The keyed stores
    /// use the keyed classification. A store with a join source of its own routes its operations
    /// to the join engine over its own scope (joins plan section 9).
    fn classify<D: Discipline>(
        agent: &GenericSpace<Self, D>,
        env: &SpaceEnv,
        opened: &mut CollectionTxn,
        op: &SpaceOp,
    ) -> Result<JoinRoute, SpaceError> {
        join::classify(agent, env, opened, op)
    }

    /// RUN-JOIN of a collection operation of the store (joins plan section 4.2). The keyed stores
    /// run the engine with the keyed source. A store with a join source of its own runs the
    /// engine with that source (joins plan section 9).
    fn run_join<D: Discipline>(
        agent: &GenericSpace<Self, D>,
        env: &SpaceEnv,
        opened: CollectionTxn,
        scope: JoinScope,
        op: SpaceOp,
    ) -> Result<Prepared<SpaceOutcome>, SpaceError> {
        join::run_join(agent, env, opened, scope, op)
    }

    /// True when the store allocates index names (P5d, F11). A store without an allocator
    /// answers every allocation with [`Allocation::Baseline`] and does no work for it.
    const ALLOCATOR: bool = false;

    /// The key of the collection that `channel` names. Routing computes the same key.
    fn key(&self, channel: &Par) -> Result<CollectionKey, SpaceError>;

    /// Checks the routed key `key` of the collection that `channel` names, under the config
    /// `config` of the space, before any staging. An Array or a Vector space accepts only its own
    /// index names below its size (`NotAnIndexName`, F8). The default accepts every key and does
    /// no work.
    fn check_key(
        &self,
        _config: &SpaceConfig,
        _channel: &Par,
        _key: &CollectionKey,
        _meter: &ScanMeter,
    ) -> Result<(), SpaceError> {
        Ok(())
    }

    /// The channel of the records and waiting continuations of the collection `key`.
    fn data_channel(&self, space: NativeSpace, key: &CollectionKey) -> Par;

    /// The counter channel of the collection `key`.
    fn counter_channel(&self, space: NativeSpace, key: &CollectionKey) -> Par;

    /// The non-persistent records that the creation of `space` stages after the space counter,
    /// with their channels: for example the allocator `A(sid) = 0` of an Array or a Vector space
    /// (P5d). `random` is the random state of the factory call, and `authority` is the cost
    /// authority of the caller. The channels join the lock keys of the creation. The default
    /// stages none.
    fn creation_records(
        &self,
        _space: NativeSpace,
        _random: &Blake2b512Random,
        _authority: &CostAuthority,
        _meter: &ScanMeter,
    ) -> Result<Vec<(Par, ListParWithRandom)>, SpaceError> {
        Ok(Vec::new())
    }

    /// Stages the allocation of `op.count` index names in `cx` and returns them in order (P5d,
    /// F11). Only a store with [`OuterStore::ALLOCATOR`] is asked, so the default fails with
    /// `Inconsistent`.
    fn allocate(
        &self,
        _cx: &mut AllocCx<'_>,
        _config: &SpaceConfig,
        _op: &AllocOp,
    ) -> Result<Vec<u64>, SpaceError> {
        Err(SpaceError::Inconsistent("the outer store has no allocator"))
    }

    /// A fresh name of `space` for the handle method `gensym`. `random` is the random state of
    /// the call.
    fn gensym(&self, space: NativeSpace, random: &Blake2b512Random) -> Result<Par, SpaceError>;
}

/// One side of a collection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    /// The records.
    Data,
    /// The waiting continuations.
    Waiters,
}

/// An entry of one side of a collection with the `ord` of its envelope.
#[derive(Clone, Copy, Debug)]
pub struct Ordered<'a, T> {
    /// The order of the entry.
    pub ord: i64,
    /// The entry.
    pub entry: &'a T,
}

/// The entries of a collection that a bind of a join can bind (P5c, joins plan section 3.2). A
/// join with `m` binds on one collection binds `m` distinct entries of its window (decision 28).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum JoinWindow {
    /// The first `m` entries of the head order of the records: the queue, the stack and the cell
    /// (decision 11).
    Head,
    /// For each level, the first `m` records of the level that the lane of the bind sees: the
    /// priority queue (decision 38).
    Lanes,
    /// Every stored record, in the canonical order of DR-75: the bag (decision 41).
    Scan,
    /// One representative for each class, the first member of the class in the canonical order:
    /// the set (F5). A bind claims its whole class.
    Classes,
}

/// The inner collection of a family: the order in which the entries of a collection interact,
/// and the plans of its operations (P5 design v3, A.3.3).
///
/// The plans have defaults: the head driver of this module for `offer` and `receive`, a plain
/// re-store for `restore`, no settle, no capacity and no query params. A discipline overrides
/// the defaults that its rules change. A plan stages only through [`Cx`].
pub trait Discipline: Default + Send + Sync + 'static {
    /// The `inner` name of the family in the config record.
    const NAME: &'static str;

    /// True when every operation bumps the collection counter (M-1, decision 31).
    const USES_COUNTER: bool;

    // Disabled by decision 29: same-space joins into every collection run in the join engine of
    // P5c (`spaces::join`), so no family is gated on this flag. `Discipline::JOIN_WINDOW` gives
    // the entries that a join bind sees instead.
    // /// True when a receive may join this collection with other channels (P5).
    // const JOINS: bool;

    /// The named send params of the discipline.
    const SEND_PARAMS: &'static [&'static str];

    /// The named query params of a receive, for example `priority` on a priority queue (P5b).
    /// A discipline without query params rejects every query with `QuerySpecUnsupported`.
    const QUERY_PARAMS: &'static [&'static str] = &[];

    /// The most entries of each side of a collection, or `None` for no bound. A cell holds one
    /// record and one waiter (F15).
    const CAPACITY: Option<usize> = None;

    /// The window of a join bind on a collection of the discipline (P5c). The default is the
    /// head window of the head-only disciplines.
    const JOIN_WINDOW: JoinWindow = JoinWindow::Head;

    /// The head of one side: the entry that interacts next, or `None` for an empty side. The
    /// result depends on the entry set only, not on the order of `entries`. Each comparison and
    /// each tie-break encode is reserved on `meter` before it is made (decision 42).
    fn head_metered<T: Serialize>(
        &self,
        side: Side,
        entries: &[Ordered<'_, T>],
        meter: &ScanMeter,
    ) -> Result<Option<usize>, SpaceError>;

    /// [`Discipline::head_metered`] without a budget.
    fn head<T: Serialize>(
        &self,
        side: Side,
        entries: &[Ordered<'_, T>],
    ) -> Result<Option<usize>, SpaceError> {
        self.head_metered(side, entries, &ScanMeter::new(None))
    }

    /// True when a fresh element of `side` becomes the head of its side, which holds `queued`
    /// entries. A re-issued persistent element uses [`Discipline::precedes`] instead.
    fn reaches_head(&self, side: Side, queued: usize) -> bool;

    /// True when an element of `side` with the order `ord` interacts before the head of the side,
    /// whose order is `head` (decision 43). The two orders differ. A re-issued persistent element
    /// re-enters at the head exactly when its side is empty or this holds. The comparison is
    /// reserved on `meter` before it is made.
    fn precedes(
        &self,
        side: Side,
        ord: i64,
        head: i64,
        meter: &ScanMeter,
    ) -> Result<bool, SpaceError>;

    /// Checks the values of the named params `params` of a send, and of the query `query` of a
    /// receive, under the config `config`, before the bump. The keys are already checked against
    /// the schema of the discipline. The default accepts every value.
    fn check_values(
        &self,
        _config: &SpaceConfig,
        _params: &BTreeMap<String, Par>,
        _query: Option<&QueryParams>,
        _meter: &ScanMeter,
    ) -> Result<(), SpaceError> {
        Ok(())
    }

    /// The produce plan of the datum of `op` with the order `ord`. The default is the head
    /// driver [`offer_head`].
    fn offer(
        &self,
        cx: &mut Cx<'_, '_>,
        op: ProduceOp,
        ord: i64,
    ) -> Result<Option<SpaceReissue>, SpaceError> {
        offer_head(self, cx, op, ord)
    }

    /// The consume plan of the continuation of `op` with the order `ord`. The default is the
    /// head driver [`receive_head`].
    fn receive(
        &self,
        cx: &mut Cx<'_, '_>,
        op: ConsumeOp,
        ord: i64,
    ) -> Result<Option<SpaceReissue>, SpaceError> {
        receive_head(self, cx, op, ord)
    }

    /// Stores again the non-persistent record `record` that a consume-side peek fire removed
    /// (decision 45, Rule E). The record bytes, the channel and `persist = false` fix the
    /// produce identity, so the record alone determines it, as for the host's second send. The
    /// default is a plain store with the record's own authority and no counter bump. It is
    /// exact for a head-only discipline, because a peek receive fires only when no waiter is
    /// queued.
    fn restore(
        &self,
        cx: &mut Cx<'_, '_>,
        record: ListParWithRandom,
    ) -> Result<Option<SpaceReissue>, SpaceError> {
        cx.restore(record)?;
        Ok(None)
    }

    /// The fires that the discipline owes before the plan of the operation `op`. The default
    /// owes none.
    fn settle_before(
        &self,
        _cx: &mut Cx<'_, '_>,
        _op: OpView<'_>,
    ) -> Result<Option<SpaceReissue>, SpaceError> {
        Ok(None)
    }

    /// The fires that the discipline owes after the plan of the operation `op` (P5b, F2). The
    /// default owes none.
    fn settle_after(
        &self,
        _cx: &mut Cx<'_, '_>,
        _op: OpView<'_>,
    ) -> Result<Option<SpaceReissue>, SpaceError> {
        Ok(None)
    }

    /// The plan of a re-issued settle (P5b). A discipline without a settle rule never issues
    /// one, so the default fails with `Inconsistent`.
    fn settle_job(
        &self,
        _cx: &mut Cx<'_, '_>,
        _op: SettleOp,
    ) -> Result<Option<SpaceReissue>, SpaceError> {
        Err(SpaceError::Inconsistent("the discipline has no settle"))
    }

    /// The class of the payload `payload` in a join window (P5c). `None` makes each record a
    /// class of its own, as every discipline but the set does. The set gives its F5 class key,
    /// with its work reserved on `meter` first. The default does no work.
    fn join_class_key(
        &self,
        _payload: &[Par],
        _meter: &ScanMeter,
    ) -> Result<Option<Vec<u8>>, SpaceError> {
        Ok(None)
    }
}

/// The operation around which a settle hook runs (P5b): a borrowed view. A hook copies only
/// the fields that it uses, for example to build the `SettleOp` of a re-issue, and it reserves
/// each copy first (decision 42).
#[derive(Clone, Copy, Debug)]
pub enum OpView<'a> {
    /// A produce.
    Produce(&'a ProduceOp),
    /// A consume.
    Consume(&'a ConsumeOp),
}

/// The steps of a family after the dispatch (P5 design v3, A.3.4). The family agent opens the
/// transaction of an operation and reads the config once. The agent of the family that the
/// config names then runs the operation in that transaction.
pub trait FamilySpace: SpaceAgent {
    /// Classifies the operation `op` in the open transaction `opened` (P5c, joins plan section
    /// 4.1): `Single` when the data channel of its collection holds no join group other than
    /// its own, and otherwise `Join` with the closure of its collections and the lock keys of
    /// that closure. A join and a settle with a cascade are always `Join`. The reads are not
    /// lock-checked, and the classification stages nothing. It reserves what the join check of
    /// P4 reserved for a collection without joins, so the host work of such an operation does not
    /// change.
    fn classify(
        &self,
        env: &SpaceEnv,
        opened: &mut CollectionTxn,
        op: &SpaceOp,
    ) -> Result<JoinRoute, SpaceError>;

    /// Steps 3 to 8 of the collection operation `op` in the open transaction `opened`, whose
    /// config names the family of this agent (`RUN-WITH`).
    fn run_with(
        &self,
        env: &SpaceEnv,
        opened: CollectionTxn,
        op: SpaceOp,
    ) -> Result<Prepared<SpaceOutcome>, SpaceError>;

    /// The join engine (P5c) for the operation `op` in the open transaction `opened`, whose
    /// lock keys cover the closure `scope`.
    fn run_join(
        &self,
        env: &SpaceEnv,
        opened: CollectionTxn,
        scope: JoinScope,
        op: SpaceOp,
    ) -> Result<Prepared<SpaceOutcome>, SpaceError>;

    /// The allocation of `op` in the open transaction `opened`, whose config names the family of
    /// this agent (P5d, F11).
    fn run_allocate(
        &self,
        env: &SpaceEnv,
        opened: AllocationTxn,
        op: &AllocOp,
    ) -> Result<Allocation, SpaceError>;

    /// True when a `Join` operation of the family always runs in a new transaction over the
    /// lock keys of its scope (`OuterStore::ALWAYS_ESCALATES`).
    fn always_escalates(&self) -> bool;
}

/// The native space agent of the family `S:D`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct GenericSpace<S, D> {
    outer: S,
    discipline: D,
}

impl<S, D> GenericSpace<S, D> {
    /// The agent of the outer store `outer` and the discipline `discipline`.
    pub const fn new(outer: S, discipline: D) -> Self { Self { outer, discipline } }

    /// The outer store of the family.
    pub(crate) fn outer(&self) -> &S { &self.outer }

    /// The discipline of the family.
    pub(crate) fn discipline(&self) -> &D { &self.discipline }
}

/// The channels of one collection operation, in the order of its lock keys: the config channel
/// `C(sid)`, the space counter `S(sid)`, the collection counter `S(sid,k)` and the data channel
/// `D(sid,k)`.
#[derive(Clone, Debug, PartialEq)]
pub struct CollectionChannels {
    keys: [Par; 4],
}

/// The template of a `GPrivate` name with an empty id. A channel of a collection is this name
/// with its id, so a reserved copy of the template and a reserved id buffer cover its
/// allocations.
pub(crate) static GPRIVATE: LazyLock<Par> = LazyLock::new(|| namespace::gprivate(Vec::new()));

impl CollectionChannels {
    /// The channels of the collection `key` of `space` in the outer store `outer`.
    pub fn new<S: OuterStore>(outer: &S, space: NativeSpace, key: &CollectionKey) -> Self {
        Self {
            keys: [
                namespace::gprivate(namespace::config_channel(space)),
                namespace::gprivate(namespace::space_counter(space)),
                outer.counter_channel(space, key),
                outer.data_channel(space, key),
            ],
        }
    }

    /// [`CollectionChannels::new`] with its allocations reserved (decision 42): four names, the
    /// ids of the config channel and of the space counter, and the ids of the collection counter
    /// and of the data channel, which also hold the bytes of the key. The outer store derives
    /// each id from the space and the key.
    pub fn new_metered<S: OuterStore>(
        outer: &S,
        space: NativeSpace,
        key: &CollectionKey,
        meter: &ScanMeter,
    ) -> Result<Self, SpaceError> {
        let scoped = namespace::SPACE_ID_PREFIX_LEN;
        let keyed = scoped
            .checked_add(key.as_bytes().len())
            .ok_or(RSpaceError::HostWorkRejected)?;
        for bytes in [scoped, scoped, keyed, keyed] {
            meter.reserve_copy(&*GPRIVATE)?;
            meter.reserve_buffer::<u8>(bytes)?;
        }
        Ok(Self::new(outer, space, key))
    }

    /// The config channel `C(sid)`.
    pub fn config(&self) -> &Par {
        let [config, _, _, _] = &self.keys;
        config
    }

    /// The space counter `S(sid)`.
    pub fn space_counter(&self) -> &Par {
        let [_, space_counter, _, _] = &self.keys;
        space_counter
    }

    /// The collection counter `S(sid,k)`.
    pub fn counter(&self) -> &Par {
        let [_, _, counter, _] = &self.keys;
        counter
    }

    /// The data channel `D(sid,k)`.
    pub fn data(&self) -> &Par {
        let [_, _, _, data] = &self.keys;
        data
    }

    /// The lock keys of a collection operation: every channel that it can touch (readiness
    /// review S2). The space counter is a lock key, because the first operation on a collection
    /// consumes it.
    pub fn lock_keys(&self) -> &[Par; 4] { &self.keys }

    fn counters(&self) -> CounterChannels<'_> {
        CounterChannels {
            space: self.space_counter(),
            collection: self.counter(),
        }
    }
}

/// The kind of a staged operation, to check its receipt and to build the outcome. The staged
/// list holds one byte for each operation, so the reservation of a head-only plan does not
/// change with the kinds that other plans use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Staged {
    /// A take or a put of a counter.
    Counter(CounterOp),
    /// A produce that stores its record.
    Store,
    /// A consume that parks its continuation.
    Park,
    /// A fire of a continuation of the program. The outcome dispatches every result of its
    /// receipt.
    Fire,
    /// A fire of a continuation of the program whose outcome dispatches only the first results
    /// of its receipt. Their count is the next entry of the prefix list of the plan: a consume
    /// over `[D; n]` that retires the other members of a set class binds only the first (F3).
    FirePrefix,
    /// A consume fire that retires a stored member of a set class (P5a, F3). It is never
    /// dispatched.
    Retire,
    /// A consume fire that lifts a stored record out of its collection during a settle (P5b,
    /// F2). It is never dispatched.
    Lift,
    /// A fire of a waiter with two or more binds (P5c). Its continuation record has the slots
    /// of version 2, and the outcome dispatches every result of its receipt in the bind order of
    /// the receive.
    JoinFire,
}

/// What the plans of one operation share.
struct Plan<'a> {
    data: &'a Par,
    matcher: &'a SpaceMatcher<'a>,
    /// The introduction of the operation, which every staged operation registers by default.
    introduction: &'a Arc<Introduction>,
    staged: Vec<Staged>,
    /// The dispatched counts of the `FirePrefix` fires, in staging order. A head-only plan
    /// stages none, so the list never allocates for it.
    prefixes: Vec<usize>,
    /// The value of the collection counter before the bump of this round, or `None` for a
    /// discipline without a counter.
    bumped: Option<i64>,
    /// The legacy charges of the staged operations (decision 45, Option P). Every staged
    /// operation of the plan goes through it.
    legacy: LegacyChain,
    /// The fires of this transaction, its settles included (I-21). Every fire loop of the
    /// transaction counts on it, so one transaction makes at most `FIRE_ROUND_CAP` fires.
    rounds: FireRounds,
}

/// A produce-side fire: the head waiter that the datum fires, and the values that it binds.
pub struct OfferFire {
    /// True for a persistent send.
    pub persistent: bool,
    /// The waiter that the datum fires.
    pub waiter: ParWaiter,
    /// The values that the waiter binds.
    pub matched: ListParWithRandom,
}

/// A consume-side fire of one record: the record (the plan's own copy), the values that it
/// binds, and its index in the staged view of its channel.
pub struct ReceiveFire {
    /// The fired record.
    pub head: ParDatum,
    /// The values that the record binds.
    pub matched: ListParWithRandom,
    /// The index of the record in the staged view of its channel.
    pub view_index: i32,
}

/// The view and the staging primitives of one collection operation (P5 design v3, A.3.3). A
/// plan reads the staged view and stages only through these methods, so every staged operation
/// goes through the legacy chain of decision 45 (Option P), registers an introduction (P4
/// design B.7) and reserves its reads and copies (decision 42). `Cx` exposes no raw transaction.
pub struct Cx<'c, 'p> {
    txn: &'c mut ParSpaceTxn,
    plan: &'c mut Plan<'p>,
}

impl<'p> Cx<'_, 'p> {
    /// The meter of the job. Its lifetime is the lifetime of the job, not of a borrow of the
    /// view, so a plan can stage while it holds the meter.
    pub fn meter(&self) -> &'p ScanMeter { self.plan.matcher.meter() }

    /// The data channel `D(sid,k)` of the collection of the operation.
    pub fn data_channel(&self) -> &'p Par { self.plan.data }

    /// The introduction of the operation.
    pub fn introduction(&self) -> &'p Arc<Introduction> { self.plan.introduction }

    /// The value of the collection counter before the bump of this round, or `None` for a
    /// discipline without a counter. Each round bumps the counter, so the value differs in every
    /// round, also for a re-issued persistent operation that keeps its first-round order
    /// (decision 43). The body of a `K_lift` continuation needs it (P5b).
    pub fn bumped(&self) -> Option<i64> { self.plan.bumped }

    /// The fires of the transaction so far (I-21). The settle before the plan, the plan and the
    /// settle after it count on this one `FireRounds`, so the transaction makes at most
    /// `FIRE_ROUND_CAP` fires.
    pub fn rounds(&mut self) -> &mut FireRounds { &mut self.plan.rounds }

    /// The records of the collection in the staged view.
    pub fn data(&mut self) -> Result<&[ParDatum], SpaceError> {
        let channel = self.plan.data;
        self.data_on(channel)
    }

    /// The waiting continuations of the collection in the staged view.
    pub fn waiters(&mut self) -> Result<&[ParWaiter], SpaceError> {
        let channel = self.plan.data;
        self.waiters_on(channel)
    }

    /// The records of `channel` in the staged view. The lock keys of the operation must hold
    /// `channel`.
    pub fn data_on(&mut self, channel: &Par) -> Result<&[ParDatum], SpaceError> {
        Ok(self.txn.data(channel)?)
    }

    /// The waiting continuations of `channel` in the staged view. The lock keys of the operation
    /// must hold `channel`.
    pub fn waiters_on(&mut self, channel: &Par) -> Result<&[ParWaiter], SpaceError> {
        Ok(self.txn.continuations(std::slice::from_ref(channel))?)
    }

    /// The indices of the records of `channel` in the canonical order of DR-75 (decision 41),
    /// with the work reserved as the branch reserves it.
    pub fn canonical_data_order(&mut self, channel: &Par) -> Result<Vec<usize>, SpaceError> {
        let meter = self.meter();
        Ok(canonical_data_order(self.txn.data(channel)?, meter)?)
    }

    /// The indices of the waiting continuations of `channel` in the canonical order of DR-75,
    /// with the work reserved as the branch reserves it.
    pub fn canonical_waiter_order(&mut self, channel: &Par) -> Result<Vec<usize>, SpaceError> {
        let meter = self.meter();
        Ok(canonical_continuation_order(
            self.txn.continuations(std::slice::from_ref(channel))?,
            meter,
        )?)
    }

    /// The spatial match of `pattern` against `data`, metered.
    pub fn bind(
        &self,
        pattern: &BindPattern,
        data: &ListParWithRandom,
    ) -> Result<Option<ListParWithRandom>, SpaceError> {
        self.plan.matcher.bind(pattern, data)
    }

    /// The `where` guard of `continuation` over `matched`, metered (decision 37).
    pub fn admits(
        &self,
        continuation: &TaggedContinuation,
        matched: &ListParWithRandom,
    ) -> Result<bool, SpaceError> {
        self.plan.matcher.admits(continuation, matched)
    }

    /// The `where` guard of `continuation` over the values `matched` of every bind, in bind
    /// order, metered (decision 37: once, after every bind matched).
    pub fn admits_all(
        &self,
        continuation: &TaggedContinuation,
        matched: &[ListParWithRandom],
    ) -> Result<bool, SpaceError> {
        self.plan.matcher.admits_all(continuation, matched)
    }

    /// The spatial match and then the guard: `None` when either refuses.
    pub fn matches(
        &self,
        pattern: &BindPattern,
        data: &ListParWithRandom,
        continuation: &TaggedContinuation,
    ) -> Result<Option<ListParWithRandom>, SpaceError> {
        self.plan.matcher.matches(pattern, data, continuation)
    }

    /// The head of the entries of one side: its index in `entries` and its order. The scan
    /// reserves its buffer and the decode of each envelope (`ord_of`), and it copies nothing.
    pub(crate) fn scan_head<D: Discipline, T: Serialize>(
        discipline: &D,
        side: Side,
        entries: &[T],
        ord_of: fn(&T, &ScanMeter) -> Result<i64, SpaceError>,
        meter: &ScanMeter,
    ) -> Result<Option<(usize, i64)>, SpaceError> {
        let mut ordered = meter.buffer::<Ordered<'_, T>>(entries.len())?;
        for entry in entries {
            ordered.push(Ordered {
                ord: ord_of(entry, meter)?,
                entry,
            });
        }
        let head = discipline.head_metered(side, &ordered, meter)?;
        Ok(head.and_then(|index| ordered.get(index).map(|entry| (index, entry.ord))))
    }

    /// The head waiting continuation of the collection in the staged view. The scan reserves
    /// its buffer and the decode of each envelope, and only the head is copied.
    pub fn head_waiter<D: Discipline>(
        &mut self,
        discipline: &D,
    ) -> Result<Option<ParWaiter>, SpaceError> {
        let meter = self.meter();
        let waiters = self
            .txn
            .continuations(std::slice::from_ref(self.plan.data))?;
        let head = Self::scan_head(discipline, Side::Waiters, waiters, waiter_ord, meter)?;
        match head.and_then(|(index, _)| waiters.get(index)) {
            Some(waiter) => Ok(Some(meter.copy(waiter)?)),
            None => Ok(None),
        }
    }

    /// The head record of the collection in the staged view, with its index in the view. The
    /// scan reserves its buffer and the decode of each envelope, and only the head is copied.
    pub fn head_datum<D: Discipline>(
        &mut self,
        discipline: &D,
    ) -> Result<Option<(usize, ParDatum)>, SpaceError> {
        let meter = self.meter();
        let records = self.txn.data(self.plan.data)?;
        let head = Self::scan_head(discipline, Side::Data, records, datum_ord, meter)?;
        match head.and_then(|(index, _)| records.get(index).map(|datum| (index, datum))) {
            Some((index, datum)) => Ok(Some((index, meter.copy(datum)?))),
            None => Ok(None),
        }
    }

    /// The order of the head of `side` of the collection in the staged view, or `None` for an
    /// empty side. The scan copies nothing.
    pub fn head_ord<D: Discipline>(
        &mut self,
        discipline: &D,
        side: Side,
    ) -> Result<Option<i64>, SpaceError> {
        let meter = self.meter();
        let data = self.plan.data;
        let head = match side {
            Side::Data => {
                Self::scan_head(discipline, side, self.txn.data(data)?, datum_ord, meter)?
            }
            Side::Waiters => Self::scan_head(
                discipline,
                side,
                self.txn.continuations(std::slice::from_ref(data))?,
                waiter_ord,
                meter,
            )?,
        };
        Ok(head.map(|(_, ord)| ord))
    }

    /// The number of entries of `side` of the collection in the staged view.
    pub fn side_len(&mut self, side: Side) -> Result<usize, SpaceError> {
        let data = self.plan.data;
        Ok(match side {
            Side::Data => self.txn.data(data)?.len(),
            Side::Waiters => self.txn.continuations(std::slice::from_ref(data))?.len(),
        })
    }

    /// True when the element of an operation reaches the head of `side` of the collection. A
    /// fresh element (`reissued` is `None`) joins the tail, so it reaches the head only of an
    /// empty side. A re-issued persistent element with the first-round order `reissued`
    /// re-enters at its place (decision 43): at the head when its side is empty or its order
    /// precedes the order of the head. An element in flight is stored nowhere, so a stored entry
    /// with its order is `Inconsistent`. Each comparison is reserved before it is made.
    pub fn at_head<D: Discipline>(
        &mut self,
        discipline: &D,
        side: Side,
        reissued: Option<i64>,
    ) -> Result<bool, SpaceError> {
        let meter = self.meter();
        let Some(ord) = reissued else {
            let queued = self.side_len(side)?;
            return Ok(discipline.reaches_head(side, queued));
        };
        let Some(head) = self.head_ord(discipline, side)? else {
            return Ok(true);
        };
        meter.compare(size_of::<i64>(), size_of::<i64>())?;
        match ord == head {
            true => Err(SpaceError::Inconsistent(
                "a re-issued element shares its order with a stored entry",
            )),
            false => discipline.precedes(side, ord, head, meter),
        }
    }

    /// Fails with `CellOccupied` when `side` of the collection already holds `D::CAPACITY`
    /// entries (F15). The check reads the size of the side and stages nothing, so the dropped
    /// transaction of a failed store or park applies nothing (decision 19, D6). A discipline
    /// without a capacity does no work.
    pub fn check_capacity<D: Discipline>(&mut self, side: Side) -> Result<(), SpaceError> {
        let Some(capacity) = D::CAPACITY else {
            return Ok(());
        };
        let held = self.side_len(side)?;
        self.meter()
            .compare(size_of::<usize>(), size_of::<usize>())?;
        match held >= capacity {
            true => Err(SpaceError::CellOccupied),
            false => Ok(()),
        }
    }

    /// The one-channel group of `channel`, with its buffer and its copy reserved.
    fn channel_list(&self, channel: &Par) -> Result<Vec<Par>, SpaceError> {
        let meter = self.meter();
        let mut channels = meter.buffer::<Par>(1)?;
        channels.push(meter.copy(channel)?);
        Ok(channels)
    }

    /// The one-pattern list of `pattern`, with its buffer and its copy reserved.
    fn pattern_list(&self, pattern: &BindPattern) -> Result<Vec<BindPattern>, SpaceError> {
        let meter = self.meter();
        let mut patterns = meter.buffer::<BindPattern>(1)?;
        patterns.push(meter.copy(pattern)?);
        Ok(patterns)
    }

    /// Appends `kind` to the staged list.
    fn push_staged(&mut self, kind: Staged) -> Result<(), SpaceError> {
        let meter = self.meter();
        push_reserved(&mut self.plan.staged, kind, meter)
    }

    /// Stages the store of `record` on the data channel of the collection, with the
    /// introduction of the operation.
    pub fn store(&mut self, record: ListParWithRandom, persist: bool) -> Result<(), SpaceError> {
        let channel = self.plan.data;
        let introduction = self.plan.introduction;
        self.store_on(channel, record, persist, introduction)
    }

    /// Stages the store of `record` on `channel` with the introduction `introduction`: a record
    /// of the collection, a PathMap marker (P5e), or a re-store with its record's own authority
    /// (decision 45, Rule E).
    pub fn store_on(
        &mut self,
        channel: &Par,
        record: ListParWithRandom,
        persist: bool,
        introduction: &Arc<Introduction>,
    ) -> Result<(), SpaceError> {
        let meter = self.meter();
        let op = SpaceProduce {
            channel: meter.copy(channel)?,
            data: record,
            persist,
            outcome: ProduceOutcome::Store,
            before_observe: Some(record::register_introduction(introduction, meter)?),
        };
        self.plan.legacy.produce(self.txn, op)?;
        self.push_staged(Staged::Store)
    }

    /// Stages the park of the continuation of the receive `op` on the data channel of the
    /// collection.
    pub fn park(
        &mut self,
        op: &ConsumeOp,
        continuation: TaggedContinuation,
        peeks: BTreeSet<i32>,
    ) -> Result<(), SpaceError> {
        let channels = self.channel_list(self.plan.data)?;
        let patterns = self.pattern_list(&op.pattern)?;
        self.park_on(channels, patterns, continuation, op.persistent, peeks)
    }

    /// Stages the park of `continuation` on the channel group `channels` with `patterns`, with
    /// the introduction of the operation: a waiter on a prefix (P5e) or on a join (P5c).
    pub fn park_on(
        &mut self,
        channels: Vec<Par>,
        patterns: Vec<BindPattern>,
        continuation: TaggedContinuation,
        persist: bool,
        peeks: BTreeSet<i32>,
    ) -> Result<(), SpaceError> {
        let meter = self.meter();
        let staged = SpaceConsume {
            channels,
            patterns,
            continuation,
            persist,
            peeks,
            outcome: ConsumeOutcome::Park,
            before_observe: Some(record::register_introduction(
                self.plan.introduction,
                meter,
            )?),
        };
        self.plan.legacy.consume(self.txn, staged)?;
        self.push_staged(Staged::Park)
    }

    /// Stages the re-store of a record that a consume-side peek removed (decision 45, Rule E) on
    /// the data channel of the collection: the same record bytes, so the same produce identity,
    /// stored again as a non-persistent record, as the host reducer's `produce_peeks` sends a
    /// peeked datum again. The staged produce registers the record's own cost authority, and
    /// the operation bumps no counter for it.
    pub fn restore(&mut self, record: ListParWithRandom) -> Result<(), SpaceError> {
        let channel = self.plan.data;
        self.restore_on(channel, record)
    }

    /// [`Cx::restore`] on `channel`: a re-stored PathMap marker (P5e) or a record of a join
    /// (P5c).
    pub fn restore_on(
        &mut self,
        channel: &Par,
        record: ListParWithRandom,
    ) -> Result<(), SpaceError> {
        let meter = self.meter();
        let introduction = self.plan.introduction.for_record(&record, meter)?;
        self.store_on(channel, record, false, &introduction)
    }

    /// Stages a produce-side fire on the data channel of the collection, with the introduction
    /// of the operation: `data` is the record of the staged produce, `removed` the record of the
    /// incoming candidate and `source` the identity of the produce.
    pub fn fire_offer(
        &mut self,
        fire: OfferFire,
        data: ListParWithRandom,
        removed: ListParWithRandom,
        source: Produce,
    ) -> Result<(), SpaceError> {
        let channel = self.plan.data;
        let introduction = self.plan.introduction;
        self.fire_offer_on(channel, fire, data, removed, source, introduction)
    }

    /// [`Cx::fire_offer`] on `channel` with the introduction `introduction`: a re-offer of a
    /// lifted or restored record keeps the identity and the authority of that record (P5b, F2).
    pub fn fire_offer_on(
        &mut self,
        channel: &Par,
        fire: OfferFire,
        data: ListParWithRandom,
        removed: ListParWithRandom,
        source: Produce,
        introduction: &Arc<Introduction>,
    ) -> Result<(), SpaceError> {
        let meter = self.meter();
        let channels = self.channel_list(channel)?;
        let mut candidates = meter.buffer::<ParCandidate>(1)?;
        candidates.push(incoming_candidate(
            meter.copy(channel)?,
            source,
            removed,
            fire.persistent,
            fire.matched,
        ));
        let op = SpaceProduce {
            channel: meter.copy(channel)?,
            data,
            persist: fire.persistent,
            outcome: ProduceOutcome::Fire {
                channels,
                continuation: fire.waiter,
                data: candidates,
            },
            before_observe: Some(record::register_introduction(introduction, meter)?),
        };
        self.plan.legacy.produce(self.txn, op)?;
        self.push_staged(Staged::Fire)
    }

    /// Stages a consume-side fire of the receive `op` over one record of the collection, with
    /// the parked continuation `continuation` and the peek set `peeks`. The record is the plan's
    /// own copy, so it moves into the candidate.
    pub fn fire_receive(
        &mut self,
        op: &ConsumeOp,
        fire: ReceiveFire,
        continuation: TaggedContinuation,
        peeks: BTreeSet<i32>,
    ) -> Result<(), SpaceError> {
        let meter = self.meter();
        let channels = self.channel_list(self.plan.data)?;
        let patterns = self.pattern_list(&op.pattern)?;
        let mut candidates = meter.buffer::<ParCandidate>(1)?;
        candidates.push(receive_candidate(meter.copy(self.plan.data)?, fire));
        self.fire_receive_group(
            channels,
            patterns,
            continuation,
            op.persistent,
            peeks,
            candidates,
            1,
        )
    }

    /// Stages a consume-side fire of `continuation` over the channel group `channels` with the
    /// candidates `candidates`, one for each channel, in group order. The outcome dispatches the
    /// first `binds` results: a set consume over `[D; n]` binds one member and retires the other
    /// members of its class with the same COMM (F3, F20).
    #[allow(clippy::too_many_arguments)]
    pub fn fire_receive_group(
        &mut self,
        channels: Vec<Par>,
        patterns: Vec<BindPattern>,
        continuation: TaggedContinuation,
        persist: bool,
        peeks: BTreeSet<i32>,
        candidates: Vec<ParCandidate>,
        binds: usize,
    ) -> Result<(), SpaceError> {
        let width = candidates.len();
        if channels.len() != width || patterns.len() != width || binds == 0 || binds > width {
            return Err(SpaceError::Inconsistent(
                "a fire group does not line up its channels, patterns, candidates and binds",
            ));
        }
        let meter = self.meter();
        let staged = SpaceConsume {
            channels,
            patterns,
            continuation,
            persist,
            peeks,
            outcome: ConsumeOutcome::Fire { data: candidates },
            before_observe: Some(record::register_introduction(
                self.plan.introduction,
                meter,
            )?),
        };
        self.plan.legacy.consume(self.txn, staged)?;
        match binds == width {
            true => self.push_staged(Staged::Fire),
            false => {
                push_reserved(&mut self.plan.prefixes, binds, meter)?;
                self.push_staged(Staged::FirePrefix)
            }
        }
    }

    /// Stages the retirement of stored members of a set class (P5a, F3): a consume fire of
    /// `continuation` over the channel group `channels` with the stored candidates `candidates`,
    /// one for each channel. The SET discipline builds the `K_retire` shape. The fire is never
    /// dispatched and is not a fire round.
    pub fn retire(
        &mut self,
        channels: Vec<Par>,
        patterns: Vec<BindPattern>,
        continuation: TaggedContinuation,
        candidates: Vec<ParCandidate>,
    ) -> Result<(), SpaceError> {
        self.fire_internal(Staged::Retire, channels, patterns, continuation, candidates)
    }

    /// Stages the lift of a stored record out of its collection during a settle (P5b, F2): a
    /// consume fire of `continuation` over `channels` with the stored candidates `candidates`.
    /// The STACK discipline builds the `K_lift` shape. The fire is never dispatched.
    pub fn lift(
        &mut self,
        channels: Vec<Par>,
        patterns: Vec<BindPattern>,
        continuation: TaggedContinuation,
        candidates: Vec<ParCandidate>,
    ) -> Result<(), SpaceError> {
        self.fire_internal(Staged::Lift, channels, patterns, continuation, candidates)
    }

    /// Stages a non-persistent consume fire that removes stored records and is never
    /// dispatched, with the introduction of the operation.
    fn fire_internal(
        &mut self,
        kind: Staged,
        channels: Vec<Par>,
        patterns: Vec<BindPattern>,
        continuation: TaggedContinuation,
        candidates: Vec<ParCandidate>,
    ) -> Result<(), SpaceError> {
        let width = candidates.len();
        if width == 0 || channels.len() != width || patterns.len() != width {
            return Err(SpaceError::Inconsistent(
                "an internal fire does not line up its channels, patterns and candidates",
            ));
        }
        let meter = self.meter();
        let staged = SpaceConsume {
            channels,
            patterns,
            continuation,
            persist: false,
            peeks: BTreeSet::new(),
            outcome: ConsumeOutcome::Fire { data: candidates },
            before_observe: Some(record::register_introduction(
                self.plan.introduction,
                meter,
            )?),
        };
        self.plan.legacy.consume(self.txn, staged)?;
        self.push_staged(kind)
    }

    /// The re-issue of `op` with the cause `cause`: a plan that ends at the round cap (I-21) or
    /// at a divergent pair (A10) runs the rest as a new `Intent::Space`.
    pub fn reissue(&self, op: SpaceOp, cause: ReissueCause) -> SpaceReissue {
        SpaceReissue { op, cause }
    }

    // ── The primitives of the join engine (P5c) ───────────────────────────────────────────

    /// The join groups of `channel` in the staged view. The caller reserves its reads of the
    /// groups.
    pub fn joins_on(&mut self, channel: &Par) -> Result<&[Vec<Par>], SpaceError> {
        Ok(self.txn.joins(channel)?)
    }

    /// The waiting continuations of the join group `group` in the staged view. The lock keys
    /// of the operation must hold every channel of the group.
    pub fn group_waiters(&mut self, group: &[Par]) -> Result<&[ParWaiter], SpaceError> {
        Ok(self.txn.continuations(group)?)
    }

    /// The indices of the waiting continuations of the join group `group` in the canonical
    /// order of DR-75, with the work reserved as the branch reserves it.
    pub fn canonical_group_order(&mut self, group: &[Par]) -> Result<Vec<usize>, SpaceError> {
        let meter = self.meter();
        Ok(canonical_continuation_order(
            self.txn.continuations(group)?,
            meter,
        )?)
    }

    /// Bumps the counters `counters` of a collection that the transaction touches (decision
    /// 31): the staged operations of the bump of the collection of the operation, through the
    /// legacy chain of the plan. It returns the value of the collection counter before the bump.
    pub fn bump_on(
        &mut self,
        counters: CounterChannels<'_>,
        random: &Blake2b512Random,
        authority: OperationAuthority<'_>,
    ) -> Result<i64, SpaceError> {
        stage_bump(self.txn, self.plan, counters, random, authority)
    }

    /// Stages a produce-side fire of the group `channels`: the datum `data` on `channel` fires
    /// `waiter` with `candidates`, one for each channel of the group in group order, of which
    /// exactly one is the incoming candidate (`incoming_candidate`). A group of two or more
    /// channels stages a `JoinFire`, and a group of one channel a `Fire`. The decode of the fired
    /// continuation by the outcome is prepaid here, for the record version of the group.
    #[allow(clippy::too_many_arguments)]
    pub fn fire_offer_group(
        &mut self,
        channel: &Par,
        data: ListParWithRandom,
        persist: bool,
        waiter: ParWaiter,
        channels: Vec<Par>,
        candidates: Vec<ParCandidate>,
        introduction: &Arc<Introduction>,
    ) -> Result<(), SpaceError> {
        let width = candidates.len();
        if width == 0 || channels.len() != width {
            return Err(SpaceError::Inconsistent(
                "a group fire does not line up its channels and candidates",
            ));
        }
        let meter = self.meter();
        let kind = match width {
            1 => {
                record::prepay_continuation_unwrap(&waiter.continuation, meter)?;
                Staged::Fire
            }
            _ => {
                record::prepay_join_continuation_unwrap(&waiter.continuation, width, meter)?;
                Staged::JoinFire
            }
        };
        let op = SpaceProduce {
            channel: meter.copy(channel)?,
            data,
            persist,
            outcome: ProduceOutcome::Fire {
                channels,
                continuation: waiter,
                data: candidates,
            },
            before_observe: Some(record::register_introduction(introduction, meter)?),
        };
        self.plan.legacy.produce(self.txn, op)?;
        self.push_staged(kind)
    }

    /// Stages a consume-side fire of a join waiter: `continuation`, a record of version 2, over
    /// the group `channels` with `patterns` and the stored candidates `candidates`, one for each
    /// channel in group order, with the introduction of the operation. It stages a `JoinFire`,
    /// and the decode of the continuation by the outcome is prepaid here.
    pub fn fire_join(
        &mut self,
        channels: Vec<Par>,
        patterns: Vec<BindPattern>,
        continuation: TaggedContinuation,
        persist: bool,
        peeks: BTreeSet<i32>,
        candidates: Vec<ParCandidate>,
    ) -> Result<(), SpaceError> {
        let width = candidates.len();
        if width < 2 || channels.len() != width || patterns.len() != width {
            return Err(SpaceError::Inconsistent(
                "a join fire does not line up its channels, patterns and candidates",
            ));
        }
        let meter = self.meter();
        record::prepay_join_continuation_unwrap(&continuation, width, meter)?;
        let staged = SpaceConsume {
            channels,
            patterns,
            continuation,
            persist,
            peeks,
            outcome: ConsumeOutcome::Fire { data: candidates },
            before_observe: Some(record::register_introduction(
                self.plan.introduction,
                meter,
            )?),
        };
        self.plan.legacy.consume(self.txn, staged)?;
        self.push_staged(Staged::JoinFire)
    }
}

/// Stages the bump of the counters `counters` through the legacy chain of `plan`, and lists its
/// staged operations in the plan. It returns the value of the collection counter before the
/// bump. The bump of the collection of an operation and the bump of each collection that a join
/// transaction touches stage the same operations.
fn stage_bump(
    txn: &mut ParSpaceTxn,
    plan: &mut Plan<'_>,
    counters: CounterChannels<'_>,
    random: &Blake2b512Random,
    authority: OperationAuthority<'_>,
) -> Result<i64, SpaceError> {
    let bump = counter::bump(
        txn,
        counters,
        &BumpContext {
            matcher: plan.matcher,
            random,
            authority,
        },
        &mut plan.legacy,
    )?;
    let meter = plan.matcher.meter();
    for kind in bump.staged.iter().copied() {
        push_reserved(&mut plan.staged, Staged::Counter(kind), meter)?;
    }
    Ok(bump.ord)
}

/// The candidate of a consume-side fire of one record on `channel`: the record moves into the
/// candidate, with the values that it binds.
pub(crate) fn receive_candidate(channel: Par, fire: ReceiveFire) -> ParCandidate {
    let ReceiveFire {
        head: Datum { a, persist, source },
        matched,
        view_index,
    } = fire;
    ConsumeCandidate {
        channel,
        datum: Datum {
            a: matched,
            persist,
            source,
        },
        removed_datum: a,
        datum_index: view_index,
    }
}

/// Appends `value` to `values`. A list at its capacity reserves its growth on `meter` before it
/// grows, as `Vec` doubles its buffer (decision 42). A head-only plan starts with room for all
/// of its operations (`MAX_STAGED`), so it reserves no growth.
pub(crate) fn push_reserved<T>(
    values: &mut Vec<T>,
    value: T,
    meter: &ScanMeter,
) -> Result<(), SpaceError> {
    if values.len() == values.capacity() {
        let grown = values
            .capacity()
            .checked_mul(2)
            .ok_or(RSpaceError::HostWorkRejected)?
            .max(1);
        meter.reserve_buffer::<T>(grown)?;
        values
            .try_reserve_exact(grown - values.len())
            .map_err(|_| RSpaceError::HostWorkRejected)?;
    }
    values.push(value);
    Ok(())
}

/// The peek set of a receive: `{0}` for a peek, with its tree node reserved, or the empty set.
pub fn peek_set(peek: bool, meter: &ScanMeter) -> Result<BTreeSet<i32>, SpaceError> {
    match peek {
        true => {
            let (operations, bytes) =
                tree_backing::<i32, ()>(1).ok_or(RSpaceError::HostWorkRejected)?;
            meter.charge(operations, 0, bytes)?;
            Ok(BTreeSet::from([0]))
        }
        false => Ok(BTreeSet::new()),
    }
}

/// The peek set of a join receive that peeks (P5c): `{0, …, n-1}`, because one arrow covers
/// every bind of a receive. Its tree nodes are reserved first.
pub fn peek_set_all(n: usize, meter: &ScanMeter) -> Result<BTreeSet<i32>, SpaceError> {
    let last = i32::try_from(n)
        .map_err(|_| SpaceError::Inconsistent("a join has more binds than a peek set holds"))?;
    let (operations, bytes) = tree_backing::<i32, ()>(n).ok_or(RSpaceError::HostWorkRejected)?;
    meter.charge(operations, 0, bytes)?;
    Ok((0..last).collect())
}

/// The error of a missing config record: `DanglingName` for a space name and `DanglingHandle`
/// for every other channel. The read of the id of `channel` is reserved first.
fn missing_config(channel: &Par, meter: &ScanMeter) -> SpaceError {
    match meter.inspect(channel) {
        Err(error) => error.into(),
        Ok(()) => match namespace::reified_id_of(channel) {
            Some(ReifiedId::Name(..)) => SpaceError::DanglingName,
            _ => SpaceError::DanglingHandle,
        },
    }
}

/// The start of a plan of `op` (I-21): runs the settle that `D` owes before the plan, and then
/// checks the round cap of the transaction. One space transaction makes at most `FIRE_ROUND_CAP`
/// fires, its settles included. When the settle leaves the transaction at the cap, the plan
/// stages nothing, and the operation re-issues as the program issued it, with the order rule of
/// decision 43 (`settle::reissued_operation`).
pub fn open_plan<D: Discipline>(
    discipline: &D,
    cx: &mut Cx<'_, '_>,
    op: OpView<'_>,
) -> Result<Option<SpaceReissue>, SpaceError> {
    if let Some(reissue) = discipline.settle_before(cx, op)? {
        return Ok(Some(reissue));
    }
    if cx.rounds().fired() < FIRE_ROUND_CAP {
        return Ok(None);
    }
    let reissued = reissued_operation(op, cx.bumped(), cx.meter())?;
    Ok(Some(cx.reissue(reissued, ReissueCause::RoundCap)))
}

/// The head driver of the produce plan (decision 11, decision 30): stages the store or the fires
/// of the datum of `op` with the order `ord`, and returns the re-issue. The datum fires only
/// from the head of the records ([`Cx::at_head`]), and otherwise it is stored at its place. A
/// re-issue of a persistent datum carries `ord` (decision 43). A fire that ends the loop moves
/// the record and its source into the staged produce, and a re-issue moves `op`, so only a
/// re-offer copies them. The settle hooks of `D` run before and after the plan.
pub fn offer_head<D: Discipline>(
    discipline: &D,
    cx: &mut Cx<'_, '_>,
    op: ProduceOp,
    ord: i64,
) -> Result<Option<SpaceReissue>, SpaceError> {
    if let Some(reissue) = open_plan(discipline, cx, OpView::Produce(&op))? {
        return Ok(Some(reissue));
    }
    let meter = cx.meter();
    let record = record::wrap_datum_metered(ord, &op.data, meter)?;
    if !cx.at_head(discipline, Side::Data, op.reissued_ord)? {
        cx.check_capacity::<D>(Side::Data)?;
        cx.store(record, op.persistent)?;
        return discipline.settle_after(cx, OpView::Produce(&op));
    }
    let source = Produce::create_metered(cx.data_channel(), &record, op.persistent, meter)?;
    // Disabled by I-21 (one fire count for each transaction, `Cx::rounds`):
    // let mut rounds = FireRounds::new();
    loop {
        let Some(waiter) = cx.head_waiter(discipline)? else {
            cx.check_capacity::<D>(Side::Data)?;
            cx.store(record, op.persistent)?;
            return discipline.settle_after(cx, OpView::Produce(&op));
        };
        let [pattern] = waiter.patterns.as_slice() else {
            return Err(SpaceError::Inconsistent(
                "a waiting continuation of a collection has several binds",
            ));
        };
        let Some(matched) = cx.matches(pattern, &op.data, &waiter.continuation)? else {
            cx.check_capacity::<D>(Side::Data)?;
            cx.store(record, op.persistent)?;
            return discipline.settle_after(cx, OpView::Produce(&op));
        };
        let spent = !op.persistent && waiter.peeks.is_empty();
        let stays = waiter.persist;
        record::prepay_continuation_unwrap(&waiter.continuation, meter)?;
        let fire = OfferFire {
            persistent: op.persistent,
            waiter,
            matched,
        };
        let step = cx.rounds().after_fire(spent, stays);
        match step {
            FireStep::Continue => {
                let (data, removed) = (meter.copy(&record)?, meter.copy(&record)?);
                cx.fire_offer(fire, data, removed, meter.copy(&source)?)?;
            }
            FireStep::Done => {
                let removed = meter.copy(&record)?;
                cx.fire_offer(fire, record, removed, source)?;
                return discipline.settle_after(cx, OpView::Produce(&op));
            }
            FireStep::Reissue(cause) => {
                let removed = meter.copy(&record)?;
                cx.fire_offer(fire, record, removed, source)?;
                return Ok(Some(cx.reissue(
                    SpaceOp::Produce(ProduceOp {
                        reissued_ord: op.persistent.then_some(ord),
                        ..op
                    }),
                    cause,
                )));
            }
        }
    }
}

/// The head driver of the consume plan: stages the park or the fires of the continuation of
/// `op` with the order `ord`, and returns the re-issue. The continuation fires only from the head
/// of the waiters ([`Cx::at_head`]), and otherwise it is parked at its place. A re-issue of a
/// persistent continuation carries `ord` (decision 43). A fire that ends the loop moves the
/// parked continuation and the peek set into the staged consume, and a re-issue moves `op`, so
/// only a re-fire copies them. After a peek fire of a non-persistent record, `D::restore` stores
/// the record again (decision 45, Rule E). The settle hooks of `D` run before and after the plan.
pub fn receive_head<D: Discipline>(
    discipline: &D,
    cx: &mut Cx<'_, '_>,
    op: ConsumeOp,
    ord: i64,
) -> Result<Option<SpaceReissue>, SpaceError> {
    if let Some(reissue) = open_plan(discipline, cx, OpView::Consume(&op))? {
        return Ok(Some(reissue));
    }
    let meter = cx.meter();
    let continuation = record::wrap_continuation_metered(
        ord,
        &op.body,
        &op.guard,
        record::stamped(&op.authority),
        meter,
    )?;
    let peeks = peek_set(op.peek, meter)?;
    if !cx.at_head(discipline, Side::Waiters, op.reissued_ord)? {
        cx.check_capacity::<D>(Side::Waiters)?;
        cx.park(&op, continuation, peeks)?;
        return discipline.settle_after(cx, OpView::Consume(&op));
    }
    // Disabled by I-21 (one fire count for each transaction, `Cx::rounds`):
    // let mut rounds = FireRounds::new();
    loop {
        let Some((index, head)) = cx.head_datum(discipline)? else {
            cx.check_capacity::<D>(Side::Waiters)?;
            cx.park(&op, continuation, peeks)?;
            return discipline.settle_after(cx, OpView::Consume(&op));
        };
        let payload = record::unwrap_datum_metered(&head.a, meter)?.datum;
        let Some(matched) = cx.matches(&op.pattern, &payload, &continuation)? else {
            cx.check_capacity::<D>(Side::Waiters)?;
            cx.park(&op, continuation, peeks)?;
            return discipline.settle_after(cx, OpView::Consume(&op));
        };
        let spent = !op.persistent;
        let stays = head.persist || op.peek;
        let view_index = i32::try_from(index)
            .map_err(|_| SpaceError::Inconsistent("a record index exceeds i32"))?;
        record::prepay_continuation_unwrap(&continuation, meter)?;
        // Rule E (decision 45): the peek fire removes a non-persistent record, as the host
        // COMM does, and the record is stored again right after the fire.
        let restored = match op.peek && !head.persist {
            true => Some(meter.copy(&head.a)?),
            false => None,
        };
        let fire = ReceiveFire {
            head,
            matched,
            view_index,
        };
        let step = cx.rounds().after_fire(spent, stays);
        match step {
            FireStep::Continue => {
                let fired = meter.copy(&continuation)?;
                cx.fire_receive(&op, fire, fired, meter.copy(&peeks)?)?;
                if let Some(reissue) = restore_peeked(discipline, cx, restored)? {
                    return Ok(Some(reissue));
                }
            }
            FireStep::Done => {
                cx.fire_receive(&op, fire, continuation, peeks)?;
                if let Some(reissue) = restore_peeked(discipline, cx, restored)? {
                    return Ok(Some(reissue));
                }
                return discipline.settle_after(cx, OpView::Consume(&op));
            }
            FireStep::Reissue(cause) => {
                cx.fire_receive(&op, fire, continuation, peeks)?;
                if let Some(reissue) = restore_peeked(discipline, cx, restored)? {
                    return Ok(Some(reissue));
                }
                return Ok(Some(cx.reissue(
                    SpaceOp::Consume(ConsumeOp {
                        reissued_ord: op.persistent.then_some(ord),
                        ..op
                    }),
                    cause,
                )));
            }
        }
    }
}

/// Stores again `restored`, the record that a peek fire removed, when there is one.
fn restore_peeked<D: Discipline>(
    discipline: &D,
    cx: &mut Cx<'_, '_>,
    restored: Option<ListParWithRandom>,
) -> Result<Option<SpaceReissue>, SpaceError> {
    match restored {
        Some(record) => discipline.restore(cx, record),
        None => Ok(None),
    }
}

/// The order of a record in the staged view, with the decode of its envelope reserved.
pub(crate) fn datum_ord(datum: &ParDatum, meter: &ScanMeter) -> Result<i64, SpaceError> {
    record::datum_ord_metered(&datum.a, meter)
}

/// The order of a waiting continuation in the staged view, with the decode of its envelope
/// reserved.
pub(crate) fn waiter_ord(waiter: &ParWaiter, meter: &ScanMeter) -> Result<i64, SpaceError> {
    record::continuation_ord_metered(&waiter.continuation, meter)
}

/// The order of an operation whose bump returned `bumped` (decision 43). With a counter, it is
/// the first-round order `reissued` of a re-issued persistent operation, and `bumped` for every
/// other operation. A bump value is never taken twice, so a first-round order lies in
/// `0..bumped`, and any other value is `Inconsistent`. The two comparisons are reserved first.
/// Without a counter every order is 0, so a re-issued order other than 0 is `Inconsistent`, and
/// its comparison is reserved first.
pub(crate) fn operation_ord(
    reissued: Option<i64>,
    bumped: Option<i64>,
    meter: &ScanMeter,
) -> Result<i64, SpaceError> {
    match (reissued, bumped) {
        (None, Some(bumped)) => Ok(bumped),
        (None, None) => Ok(0),
        (Some(ord), Some(bumped)) => {
            meter.compare(size_of::<i64>(), size_of::<i64>())?;
            meter.compare(size_of::<i64>(), size_of::<i64>())?;
            match (0..bumped).contains(&ord) {
                true => Ok(ord),
                false => Err(SpaceError::Inconsistent(
                    "a re-issued order is not an order of the counter",
                )),
            }
        }
        (Some(ord), None) => {
            meter.compare(size_of::<i64>(), size_of::<i64>())?;
            match ord == 0 {
                true => Ok(0),
                false => Err(SpaceError::Inconsistent(
                    "a re-issued order of a collection without a counter is not 0",
                )),
            }
        }
    }
}

/// Reads and decodes the config record of `space` on `channel`, and returns the config and the
/// map in the staged view. A missing record fails with `missing()`. The decode reserves its
/// work first: the read of the envelope, the strict decode of the map
/// (`SpaceConfig::from_par_metered`) and the comparison of the space ids.
fn read_space_config<'t>(
    txn: &'t mut ParSpaceTxn,
    channel: &Par,
    space: NativeSpace,
    missing: impl FnOnce() -> SpaceError,
    meter: &ScanMeter,
) -> Result<(SpaceConfig, &'t Par), SpaceError> {
    let map = match txn.data(channel)? {
        [] => return Err(missing()),
        [datum] => record::config_map_metered(&datum.a, meter)?,
        _ => return Err(SpaceError::Inconsistent("a space has two config records")),
    };
    let config = SpaceConfig::from_par_metered(map, meter)?;
    meter.compare(namespace::SID_LEN, namespace::SID_LEN)?;
    match config.space_id == space.sid {
        true => Ok((config, map)),
        false => Err(SpaceError::Inconsistent(
            "the config record names another space",
        )),
    }
}

/// The fires that `staged` lists: the fires that the outcome dispatches.
fn fire_count(staged: &[Staged]) -> usize {
    staged
        .iter()
        .filter(|kind| matches!(kind, Staged::Fire | Staged::FirePrefix | Staged::JoinFire))
        .count()
}

/// Reserves the buffer of the fires of the outcome of `staged`. [`outcome`] runs after the
/// commit, so the operation pays for its buffer before the commit, as it prepays the decode of
/// each fired continuation (`record::prepay_continuation_unwrap`). A host-work rejection then
/// never follows a commit.
fn prepay_outcome(staged: &[Staged], meter: &ScanMeter) -> Result<(), SpaceError> {
    Ok(meter.reserve_buffer::<SpaceFire>(fire_count(staged))?)
}

/// The outcome of a committed collection operation. It moves each fired continuation out of its
/// receipt and copies nothing. A `FirePrefix` fire keeps the first results of its receipt, as
/// many as the next entry of `prefixes`. A counter operation, a retirement and a lift dispatch
/// nothing.
fn outcome(
    receipt: ParSpaceReceipt,
    staged: Vec<Staged>,
    prefixes: Vec<usize>,
    reissue: Option<SpaceReissue>,
) -> Result<SpaceOutcome, SpaceError> {
    if receipt.ops.len() != staged.len() {
        return Err(SpaceError::Inconsistent(
            "the receipt does not list the staged operations",
        ));
    }
    let mut fires = Vec::with_capacity(fire_count(&staged));
    let mut prefixes = prefixes.into_iter();
    for (receipt, kind) in receipt.ops.into_iter().zip(staged) {
        match (kind, receipt) {
            (
                Staged::Counter(CounterOp::Take) | Staged::Retire | Staged::Lift,
                SpaceOpReceipt::Fired { .. },
            )
            | (Staged::Counter(CounterOp::Put) | Staged::Store, SpaceOpReceipt::Stored { .. })
            | (Staged::Park, SpaceOpReceipt::Parked { .. }) => {}
            (
                Staged::Fire,
                SpaceOpReceipt::Fired {
                    continuation,
                    results,
                    ..
                },
            ) => fires.push(SpaceFire {
                continuation: record::into_continuation(continuation.continuation)?.continuation,
                results,
            }),
            (
                Staged::FirePrefix,
                SpaceOpReceipt::Fired {
                    continuation,
                    mut results,
                    ..
                },
            ) => {
                let binds = prefixes.next().ok_or(SpaceError::Inconsistent(
                    "a prefix fire has no dispatched count",
                ))?;
                if binds > results.len() {
                    return Err(SpaceError::Inconsistent(
                        "a fire dispatches more results than its receipt holds",
                    ));
                }
                results.truncate(binds);
                fires.push(SpaceFire {
                    continuation: record::into_continuation(continuation.continuation)?
                        .continuation,
                    results,
                })
            }
            (
                Staged::JoinFire,
                SpaceOpReceipt::Fired {
                    continuation,
                    results,
                    ..
                },
            ) => {
                let record::JoinContinuationRecord {
                    slots,
                    continuation,
                } = record::into_join_continuation(continuation.continuation)?;
                fires.push(SpaceFire {
                    continuation,
                    results: record::restore_bind_order(&slots, results)?,
                })
            }
            _ => {
                return Err(SpaceError::Inconsistent(
                    "a receipt differs from its staged operation",
                ))
            }
        }
    }
    match prefixes.next() {
        None => Ok(SpaceOutcome { fires, reissue }),
        Some(_) => Err(SpaceError::Inconsistent(
            "a dispatched count has no prefix fire",
        )),
    }
}

/// The most operations that one head-only operation stages: the bump, `FIRE_ROUND_CAP` fires,
/// and one store, park or re-store. The staged list starts with this room, and a plan that
/// stages more reserves each growth (`push_reserved`).
const MAX_STAGED: usize = OPEN_BUMP_OPERATIONS + FIRE_ROUND_CAP + 1;

/// The operations of the longest bump (`counter::bump` on a new collection).
const OPEN_BUMP_OPERATIONS: usize = 3;

/// The open transaction and the config of one collection operation after steps 1 and 2.
pub struct CollectionTxn {
    /// The transaction over the lock keys of the collection.
    pub txn: ParSpaceTxn,
    /// The channels of the collection.
    pub channels: CollectionChannels,
    /// The config of the space.
    pub config: SpaceConfig,
}

/// Steps 1 and 2 of a collection operation on `channel`, the collection `key` of `space`, with
/// the channels of the outer store `outer`: the transaction over the lock keys, and the config,
/// read once (decision 42).
pub async fn begin_collection<S: OuterStore>(
    outer: &S,
    env: &SpaceEnv,
    space: NativeSpace,
    key: &CollectionKey,
    channel: &Par,
) -> Result<CollectionTxn, SpaceError> {
    let meter = &env.meter;
    let channels = CollectionChannels::new_metered(outer, space, key, meter)?;
    let mut txn = env.store.begin(channels.lock_keys()).await?;
    let (config, _) = read_space_config(
        &mut txn,
        channels.config(),
        space,
        || missing_config(channel, meter),
        meter,
    )?;
    Ok(CollectionTxn {
        txn,
        channels,
        config,
    })
}

/// Runs the collection operation `op` with the family agent `agent` in the open transaction
/// `opened` ("scope, then lock, then plan", joins plan section 4.1). The agent classifies the
/// operation. A `Single` operation runs `run_with` in `opened`. A `Join` operation runs the join
/// engine. When the lock keys of `opened` do not cover the closure of the join, the agent drops
/// `opened`, which stages nothing, and begins one new transaction over the lock keys of the
/// closure: the scheduler footprint `[2]‖sid` serializes every job of the space, so the state
/// does not change between the two transactions, and the closure is complete, so one escalation
/// is enough. The config of `opened` is read once (decision 42).
pub async fn prepare_collection_op(
    agent: &dyn FamilySpace,
    env: &SpaceEnv,
    mut opened: CollectionTxn,
    op: SpaceOp,
) -> Result<Prepared<SpaceOutcome>, SpaceError> {
    match agent.classify(env, &mut opened, &op)? {
        JoinRoute::Single => agent.run_with(env, opened, op),
        JoinRoute::Join(scope) => match agent.always_escalates() {
            false if scope.covers(&scope.own, &env.meter)? => {
                agent.run_join(env, opened, scope, op)
            }
            _ => {
                let CollectionTxn {
                    txn,
                    channels,
                    config,
                } = opened;
                drop(txn);
                let txn = env.store.begin(scope.lock_keys()).await?;
                agent.run_join(
                    env,
                    CollectionTxn {
                        txn,
                        channels,
                        config,
                    },
                    scope,
                    op,
                )
            }
        },
    }
}

/// The plan of one transaction of the join engine (P5c) over the open transaction `txn`, whose
/// own data channel is `data`: the staged list, the metered matcher, the introduction of
/// `introduction_authority`, the legacy chain of decision 45 and the fire count of the
/// transaction (decision 49), as `run_with` makes them. `stage` stages the operations through
/// [`Cx`] and returns the re-issue. The outcome after the commit is the outcome of `run_with`,
/// and its buffer is prepaid here.
pub(crate) fn run_plan(
    env: &SpaceEnv,
    mut txn: ParSpaceTxn,
    data: &Par,
    introduction_authority: &CostAuthority,
    stage: impl FnOnce(&mut Cx<'_, '_>) -> Result<Option<SpaceReissue>, SpaceError>,
) -> Result<Prepared<SpaceOutcome>, SpaceError> {
    let meter = &env.meter;
    let staged = meter.buffer::<Staged>(MAX_STAGED)?;
    let matcher = SpaceMatcher::metered(env)?;
    let introduction = Introduction::new(&env.budget, introduction_authority, meter)?;
    let mut plan = Plan {
        data,
        matcher: &matcher,
        introduction: &introduction,
        staged,
        prefixes: Vec::new(),
        bumped: None,
        legacy: LegacyChain::new(&env.budget),
        rounds: FireRounds::new(),
    };
    let reissue = stage(&mut Cx {
        txn: &mut txn,
        plan: &mut plan,
    })?;
    let Plan {
        staged,
        prefixes,
        legacy,
        ..
    } = plan;
    prepay_outcome(&staged, meter)?;
    Ok(Prepared::new(txn, move |receipt| {
        legacy.finish()?;
        outcome(receipt, staged, prefixes, reissue)
    }))
}

/// Steps 1 and 2 of the join `op` (P5c): the transaction over the lock keys `[C(sid), S(sid)]`
/// and `[S(sid,k), D(sid,k)]` of each distinct collection `k` of its binds, in ascending key
/// order, and the config, read once (decision 42). The channels of the result are the channels
/// of the collection of the first bind. A join without a bind is `Inconsistent`. The names of
/// the lock keys are reserved before they are made.
pub async fn begin_join<S: OuterStore>(
    outer: &S,
    env: &SpaceEnv,
    op: &JoinOp,
) -> Result<CollectionTxn, SpaceError> {
    let meter = &env.meter;
    let Some(first) = op.binds.first() else {
        return Err(SpaceError::Inconsistent("a join has no bind"));
    };
    let channels = CollectionChannels::new_metered(outer, op.space, &first.key, meter)?;
    let keys = join::distinct_bind_keys(op, meter)?;
    let length = keys
        .len()
        .checked_mul(2)
        .and_then(|length| length.checked_add(2))
        .ok_or(RSpaceError::HostWorkRejected)?;
    let mut lock_keys = meter.buffer::<Par>(length)?;
    lock_keys.push(meter.copy(channels.config())?);
    lock_keys.push(meter.copy(channels.space_counter())?);
    for key in keys {
        let collection = join::scoped_collection(outer, op.space, key, meter)?;
        lock_keys.push(collection.counter);
        lock_keys.push(collection.data);
    }
    let mut txn = env.store.begin(&lock_keys).await?;
    let (config, _) = read_space_config(
        &mut txn,
        channels.config(),
        op.space,
        || missing_config(&first.channel, meter),
        meter,
    )?;
    Ok(CollectionTxn {
        txn,
        channels,
        config,
    })
}

/// Reserves the comparison of the family of `config` with the family `S:D`, and compares them.
/// The comparison reads the names of the config and of the family.
pub fn family_matches<S: OuterStore, D: Discipline>(
    config: &SpaceConfig,
    meter: &ScanMeter,
) -> Result<bool, SpaceError> {
    let scanned = config
        .outer
        .len()
        .checked_add(config.inner.len())
        .ok_or(RSpaceError::HostWorkRejected)?;
    meter.compare(scanned, S::NAME.len() + D::NAME.len())?;
    Ok(config.is_family(S::NAME, D::NAME))
}

/// The `FamilyUnavailable` error of `config`, with the names of the config and no copy.
pub fn family_unavailable(config: SpaceConfig) -> SpaceError {
    SpaceError::FamilyUnavailable {
        outer: config.outer,
        inner: config.inner,
    }
}

/// The open transaction of an allocation (P5d): the transaction over the config channel and the
/// allocator channel `A(sid)` of a space, the allocator channel, and the config.
pub struct AllocationTxn {
    /// The transaction over `[C(sid), A(sid)]`.
    pub txn: ParSpaceTxn,
    /// The allocator channel `A(sid)`.
    pub allocator: Par,
    /// The config of the space.
    pub config: SpaceConfig,
}

/// Opens the transaction of an allocation in `space` and reads the config once. Every
/// allocation locks `A(sid)`, because the family is known only after the config read. A missing
/// config fails with `DanglingHandle`. The two names are reserved before they are made.
pub async fn begin_allocation(
    env: &SpaceEnv,
    space: NativeSpace,
) -> Result<AllocationTxn, SpaceError> {
    let meter = &env.meter;
    for _ in 0..2 {
        meter.reserve_copy(&*GPRIVATE)?;
        meter.reserve_buffer::<u8>(namespace::SPACE_ID_PREFIX_LEN)?;
    }
    let keys = [
        namespace::gprivate(namespace::config_channel(space)),
        namespace::gprivate(namespace::allocator_channel(space)),
    ];
    let mut txn = env.store.begin(&keys).await?;
    let [config_channel, allocator] = keys;
    let (config, _) = read_space_config(
        &mut txn,
        &config_channel,
        space,
        || SpaceError::DanglingHandle,
        meter,
    )?;
    Ok(AllocationTxn {
        txn,
        allocator,
        config,
    })
}

/// Reads the config map of `space` in a transaction that commits nothing. A missing config
/// record fails with `DanglingHandle`.
pub async fn read_config_map(env: &SpaceEnv, space: NativeSpace) -> Result<Par, SpaceError> {
    let meter = &env.meter;
    meter.reserve_copy(&*GPRIVATE)?;
    meter.reserve_buffer::<u8>(namespace::SPACE_ID_PREFIX_LEN)?;
    let channel = namespace::gprivate(namespace::config_channel(space));
    let mut txn = env.store.begin(std::slice::from_ref(&channel)).await?;
    let (_, map) = read_space_config(
        &mut txn,
        &channel,
        space,
        || SpaceError::DanglingHandle,
        meter,
    )?;
    Ok(meter.copy(map)?)
}

/// Checks each key of `keys` against `schema`. Each comparison reads the key and the schema
/// entry and is reserved first. An unknown key fails with `UnknownParam` and the closest schema
/// key.
fn check_keys<'k>(
    keys: impl Iterator<Item = &'k String>,
    schema: &[&str],
    meter: &ScanMeter,
) -> Result<(), SpaceError> {
    for key in keys {
        let mut known = false;
        for entry in schema {
            meter.compare(key.len(), entry.len())?;
            if key == entry {
                known = true;
                break;
            }
        }
        if !known {
            return Err(SpaceError::UnknownParam {
                key: key.clone(),
                suggestion: suggestion(key, schema).map(str::to_owned),
            });
        }
    }
    Ok(())
}

impl<S: OuterStore, D: Discipline> GenericSpace<S, D> {
    /// Fails with `FamilyUnavailable` unless the config names the family of this agent, and
    /// otherwise returns the config. The comparison of the names is reserved first, and the
    /// error takes the names of the config without a copy.
    fn check_family(
        &self,
        config: SpaceConfig,
        meter: &ScanMeter,
    ) -> Result<SpaceConfig, SpaceError> {
        // Disabled by decision 29: joins into ordered collections arrive in P5c, so the agent no
        // longer gates a family on `(USES_COUNTER, ¬JOINS)`. The family match stays.
        // match (config.is_family(S::NAME, D::NAME), D::USES_COUNTER, D::JOINS) {
        //     (true, true, false) => Ok(()),
        //     _ => Err(SpaceError::FamilyUnavailable {
        //         outer: config.outer,
        //         inner: config.inner,
        //     }),
        // }
        match family_matches::<S, D>(&config, meter)? {
            true => Ok(config),
            false => Err(family_unavailable(config)),
        }
    }

    // Disabled by decision 29 (P5c): `FamilySpace::classify` makes the same reads and
    // reservations, and it sends an operation on a collection with a join group to the join
    // engine (`spaces::join`) instead of failing with `Inconsistent`.
    // /// Step 3: the data channel holds no join other than its own group. The check reads the
    // /// stored group and the data channel, so it reserves both reads and one comparison, as the
    // /// branch's channel position check does.
    // fn check_joins(
    //     &self,
    //     txn: &mut ParSpaceTxn,
    //     channels: &CollectionChannels,
    //     meter: &ScanMeter,
    // ) -> Result<(), SpaceError> {
    //     let data = channels.data();
    //     match txn.joins(data)? {
    //         [] => Ok(()),
    //         [group] => {
    //             meter.inspect(group)?;
    //             meter.inspect(data)?;
    //             meter.charge(1, 0, 0)?;
    //             match group.as_slice() == std::slice::from_ref(data) {
    //                 true => Ok(()),
    //                 false => Err(SpaceError::Inconsistent(
    //                     "a collection channel is part of a join",
    //                 )),
    //             }
    //         }
    //         _ => Err(SpaceError::Inconsistent(
    //             "a collection channel is part of a join",
    //         )),
    //     }
    // }

    /// Step 4: the named params `params` of a send and the query `query` of a receive against
    /// the schema of `D`, before any staging. Each key is compared with the schema, and an
    /// unknown key fails with `UnknownParam` and the closest schema key. A query on a discipline
    /// without query params fails with `QuerySpecUnsupported`. Then `D::check_values` checks the
    /// values. An operation without params and without a query does no work.
    pub(crate) fn check_params(
        &self,
        config: &SpaceConfig,
        params: &BTreeMap<String, Par>,
        query: Option<&QueryParams>,
        meter: &ScanMeter,
    ) -> Result<(), SpaceError> {
        check_keys(params.keys(), D::SEND_PARAMS, meter)?;
        if let Some(query) = query {
            if D::QUERY_PARAMS.is_empty() {
                return Err(SpaceError::QuerySpecUnsupported);
            }
            check_keys(query.named.keys(), D::QUERY_PARAMS, meter)?;
        }
        match params.is_empty() && query.is_none() {
            true => Ok(()),
            false => self.discipline.check_values(config, params, query, meter),
        }
    }

    /// Step 5: bumps the collection counter when `D` uses one, and returns the value before the
    /// bump. The bump stages through the legacy chain of the plan.
    fn bump(
        &self,
        txn: &mut ParSpaceTxn,
        plan: &mut Plan<'_>,
        channels: &CollectionChannels,
        random_state: &[u8],
        authority: OperationAuthority<'_>,
    ) -> Result<Option<i64>, SpaceError> {
        if !D::USES_COUNTER {
            return Ok(None);
        }
        let random = Blake2b512Random::from_bytes(random_state);
        stage_bump(txn, plan, channels.counters(), &random, authority).map(Some)
    }
}

impl<S: OuterStore, D: Discipline> FamilySpace for GenericSpace<S, D> {
    fn always_escalates(&self) -> bool { S::ALWAYS_ESCALATES }

    fn classify(
        &self,
        env: &SpaceEnv,
        opened: &mut CollectionTxn,
        op: &SpaceOp,
    ) -> Result<JoinRoute, SpaceError> {
        S::classify(self, env, opened, op)
    }

    /// Every check happens before the bump, so a rejected operation leaves the counter
    /// unchanged. A settle bumps the counter, as every round of an operation does (decision 31,
    /// decision 43).
    fn run_with(
        &self,
        env: &SpaceEnv,
        opened: CollectionTxn,
        op: SpaceOp,
    ) -> Result<Prepared<SpaceOutcome>, SpaceError> {
        let meter = &env.meter;
        let CollectionTxn {
            mut txn,
            channels,
            config,
        } = opened;
        // Disabled by decision 29 (P5c): `classify` reads the join groups before this step.
        // self.check_joins(&mut txn, &channels, meter)?;
        let no_params = BTreeMap::new();
        let (channel, key, params, query, introduction_authority) = match &op {
            SpaceOp::Produce(op) => (&op.channel, &op.key, &op.params, None, &op.authority),
            SpaceOp::Consume(op) => (
                &op.channel,
                &op.key,
                &no_params,
                op.query.as_ref(),
                &op.introduction_authority,
            ),
            SpaceOp::Settle(op) => match op.cascade {
                None => (
                    &op.channel,
                    &op.key,
                    &no_params,
                    None,
                    &op.introduction_authority,
                ),
                Some(_) => {
                    return Err(SpaceError::Inconsistent(
                        "a settle with a cascade runs in the join engine",
                    ))
                }
            },
            SpaceOp::Join(_) => {
                return Err(SpaceError::Inconsistent("a join runs in the join engine"))
            }
        };
        self.outer.check_key(&config, channel, key, meter)?;
        self.check_params(&config, params, query, meter)?;
        let staged = meter.buffer::<Staged>(MAX_STAGED)?;
        let matcher = SpaceMatcher::metered(env)?;
        let introduction = Introduction::new(&env.budget, introduction_authority, meter)?;
        let mut plan = Plan {
            data: channels.data(),
            matcher: &matcher,
            introduction: &introduction,
            staged,
            prefixes: Vec::new(),
            bumped: None,
            legacy: LegacyChain::new(&env.budget),
            rounds: FireRounds::new(),
        };
        let reissue = match op {
            SpaceOp::Produce(op) => {
                let bumped = self.bump(
                    &mut txn,
                    &mut plan,
                    &channels,
                    &op.data.random_state,
                    OperationAuthority::of_produce(&op, &introduction),
                )?;
                plan.bumped = bumped;
                let ord = operation_ord(op.reissued_ord, bumped, meter)?;
                let mut cx = Cx {
                    txn: &mut txn,
                    plan: &mut plan,
                };
                self.discipline.offer(&mut cx, op, ord)?
            }
            SpaceOp::Consume(op) => {
                let bumped = self.bump(
                    &mut txn,
                    &mut plan,
                    &channels,
                    &op.body.random_state,
                    OperationAuthority::of_consume(&op, &introduction),
                )?;
                plan.bumped = bumped;
                let ord = operation_ord(op.reissued_ord, bumped, meter)?;
                let mut cx = Cx {
                    txn: &mut txn,
                    plan: &mut plan,
                };
                self.discipline.receive(&mut cx, op, ord)?
            }
            SpaceOp::Settle(op) => {
                plan.bumped = self.bump(
                    &mut txn,
                    &mut plan,
                    &channels,
                    &op.random_state,
                    OperationAuthority {
                        continuation: record::stamped(&op.authority),
                        introduction: &introduction,
                    },
                )?;
                let mut cx = Cx {
                    txn: &mut txn,
                    plan: &mut plan,
                };
                self.discipline.settle_job(&mut cx, op)?
            }
            SpaceOp::Join(_) => {
                return Err(SpaceError::Inconsistent("a join runs in the join engine"))
            }
        };
        let Plan {
            staged,
            prefixes,
            legacy,
            ..
        } = plan;
        prepay_outcome(&staged, meter)?;
        Ok(Prepared::new(txn, move |receipt| {
            legacy.finish()?;
            outcome(receipt, staged, prefixes, reissue)
        }))
    }

    fn run_join(
        &self,
        env: &SpaceEnv,
        opened: CollectionTxn,
        scope: JoinScope,
        op: SpaceOp,
    ) -> Result<Prepared<SpaceOutcome>, SpaceError> {
        S::run_join(self, env, opened, scope, op)
    }

    /// A store without an allocator answers `Baseline` with no further work, and the dropped
    /// transaction commits nothing.
    fn run_allocate(
        &self,
        env: &SpaceEnv,
        opened: AllocationTxn,
        op: &AllocOp,
    ) -> Result<Allocation, SpaceError> {
        if !S::ALLOCATOR {
            return Ok(Allocation::Baseline);
        }
        let meter = &env.meter;
        let AllocationTxn {
            mut txn,
            allocator,
            config,
        } = opened;
        let matcher = SpaceMatcher::metered(env)?;
        let introduction = Introduction::new(&env.budget, &op.authority, meter)?;
        let random = Blake2b512Random::from_bytes(&op.random_state);
        let mut legacy = LegacyChain::new(&env.budget);
        let (indices, staged) = {
            let mut cx = AllocCx {
                txn: &mut txn,
                context: BumpContext {
                    matcher: &matcher,
                    random: &random,
                    authority: OperationAuthority {
                        continuation: record::stamped(&op.authority),
                        introduction: &introduction,
                    },
                },
                allocator: &allocator,
                legacy: &mut legacy,
                staged: 0,
            };
            let indices = self.outer.allocate(&mut cx, &config, op)?;
            (indices, cx.staged)
        };
        Ok(Allocation::Indices(Prepared::new(txn, move |receipt| {
            legacy.finish()?;
            match receipt.ops.len() == staged {
                true => Ok(indices),
                false => Err(SpaceError::Inconsistent(
                    "the receipt of an allocation does not list its staged operations",
                )),
            }
        })))
    }
}

/// The view and the staging primitives of an allocation (P5d, F11): the allocator record, and
/// the take and the put of the allocator through the legacy chain of decision 45, with the
/// random states of `RandomPurpose::Allocator` (`counter::take`, `counter::put`).
pub struct AllocCx<'c> {
    txn: &'c mut ParSpaceTxn,
    context: BumpContext<'c>,
    allocator: &'c Par,
    legacy: &'c mut LegacyChain,
    staged: usize,
}

impl AllocCx<'_> {
    /// The meter of the job.
    pub fn meter(&self) -> &ScanMeter { self.context.matcher.meter() }

    /// The allocator channel `A(sid)` of the space.
    pub fn allocator_channel(&self) -> &Par { self.allocator }

    /// The only record of the allocator in the staged view, copied, or `None`. Two records are
    /// `Inconsistent`.
    pub fn allocator_record(&mut self) -> Result<Option<ParDatum>, SpaceError> {
        let meter = self.context.matcher.meter();
        match self.txn.data(self.allocator)? {
            [] => Ok(None),
            [stored] => Ok(Some(meter.copy(stored)?)),
            _ => Err(SpaceError::Inconsistent(
                "a space has two allocator records",
            )),
        }
    }

    /// Consumes the allocator record `stored`, whose value is `value`.
    pub fn take(&mut self, stored: ParDatum, value: i64) -> Result<(), SpaceError> {
        counter::take(
            self.txn,
            self.allocator,
            stored,
            value,
            RandomPurpose::Allocator,
            &self.context,
            self.legacy,
        )?;
        self.staged += 1;
        Ok(())
    }

    /// Stores the allocator record with the value `value`.
    pub fn put(&mut self, value: i64) -> Result<(), SpaceError> {
        counter::put(
            self.txn,
            self.allocator,
            value,
            RandomPurpose::Allocator,
            &self.context,
            self.legacy,
        )?;
        self.staged += 1;
        Ok(())
    }
}

#[async_trait]
impl<S: OuterStore, D: Discipline> SpaceAgent for GenericSpace<S, D> {
    async fn prepare_produce(
        &self,
        env: &SpaceEnv,
        op: ProduceOp,
    ) -> Result<Prepared<SpaceOutcome>, SpaceError> {
        let opened = begin_collection(&self.outer, env, op.space, &op.key, &op.channel).await?;
        let config = self.check_family(opened.config, &env.meter)?;
        prepare_collection_op(
            self,
            env,
            CollectionTxn { config, ..opened },
            SpaceOp::Produce(op),
        )
        .await
    }

    async fn prepare_consume(
        &self,
        env: &SpaceEnv,
        op: ConsumeOp,
    ) -> Result<Prepared<SpaceOutcome>, SpaceError> {
        let opened = begin_collection(&self.outer, env, op.space, &op.key, &op.channel).await?;
        let config = self.check_family(opened.config, &env.meter)?;
        prepare_collection_op(
            self,
            env,
            CollectionTxn { config, ..opened },
            SpaceOp::Consume(op),
        )
        .await
    }

    async fn prepare_settle(
        &self,
        env: &SpaceEnv,
        op: SettleOp,
    ) -> Result<Prepared<SpaceOutcome>, SpaceError> {
        let opened = begin_collection(&self.outer, env, op.space, &op.key, &op.channel).await?;
        let config = self.check_family(opened.config, &env.meter)?;
        prepare_collection_op(
            self,
            env,
            CollectionTxn { config, ..opened },
            SpaceOp::Settle(op),
        )
        .await
    }

    async fn prepare_join(
        &self,
        env: &SpaceEnv,
        op: JoinOp,
    ) -> Result<Prepared<SpaceOutcome>, SpaceError> {
        let opened = begin_join(&self.outer, env, &op).await?;
        let config = self.check_family(opened.config, &env.meter)?;
        prepare_collection_op(
            self,
            env,
            CollectionTxn { config, ..opened },
            SpaceOp::Join(op),
        )
        .await
    }

    async fn allocate(&self, env: &SpaceEnv, op: AllocOp) -> Result<Allocation, SpaceError> {
        let opened = begin_allocation(env, op.space).await?;
        let config = self.check_family(opened.config, &env.meter)?;
        self.run_allocate(env, AllocationTxn { config, ..opened }, &op)
    }

    /// Stages the records of a new space (P4 design C.7, steps 4 to 7): the check that `C(sid)`
    /// holds no config (`SpaceAlreadyExists`), the strict decode of the new config, the config
    /// datum, the space counter `S(sid) = 0`, the records of the outer store, and the handle
    /// continuation. The channels of the records of the outer store join the lock keys.
    async fn prepare_create(
        &self,
        env: &SpaceEnv,
        creation: SpaceCreation,
    ) -> Result<Prepared<()>, SpaceError> {
        let meter = &env.meter;
        let space = creation.space;
        let prefix = namespace::SPACE_ID_PREFIX_LEN;
        for _ in 0..3 {
            meter.reserve_copy(&*GPRIVATE)?;
            meter.reserve_buffer::<u8>(prefix)?;
        }
        let keys = [
            namespace::gprivate(namespace::config_channel(space)),
            namespace::gprivate(namespace::space_counter(space)),
            namespace::gprivate(namespace::handle_channel(space)),
        ];
        let records =
            self.outer
                .creation_records(space, &creation.random, &creation.authority, meter)?;
        let mut txn = match records.is_empty() {
            true => env.store.begin(&keys).await?,
            false => {
                let mut locked = meter.buffer::<Par>(keys.len() + records.len())?;
                for channel in keys
                    .iter()
                    .chain(records.iter().map(|(channel, _)| channel))
                {
                    locked.push(meter.copy(channel)?);
                }
                env.store.begin(&locked).await?
            }
        };
        let [config_channel, space_counter, handle] = keys;
        if !txn.data(&config_channel)?.is_empty() {
            return Err(SpaceError::SpaceAlreadyExists);
        }
        let config = SpaceConfig::from_par_metered(&creation.config, meter)?;
        meter.compare(namespace::SID_LEN, namespace::SID_LEN)?;
        if config.space_id != space.sid {
            return Err(SpaceError::Inconsistent(
                "the config of a new space names another space",
            ));
        }
        self.check_family(config, meter)?;
        let introduction = Introduction::new(&env.budget, &creation.authority, meter)?;
        let mut legacy = LegacyChain::new(&env.budget);
        let config = SpaceProduce {
            channel: config_channel,
            data: record::config_record_metered(
                creation.config,
                &RandomPurpose::Config.derive(&creation.random),
                meter,
            )?,
            persist: true,
            outcome: ProduceOutcome::Store,
            before_observe: Some(record::register_introduction(&introduction, meter)?),
        };
        legacy.produce(&mut txn, config)?;
        let cost_authority = record::cost_authority_metered(&creation.authority, meter)?;
        let counter = SpaceProduce {
            channel: space_counter,
            data: record::counter_record_metered(
                0,
                &RandomPurpose::SpaceCounter
                    .derive(&creation.random)
                    .split_byte(1),
                cost_authority,
                meter,
            )?,
            persist: false,
            outcome: ProduceOutcome::Store,
            before_observe: Some(record::register_introduction(&introduction, meter)?),
        };
        legacy.produce(&mut txn, counter)?;
        let extra = records.len();
        for (channel, data) in records {
            let staged = SpaceProduce {
                channel,
                data,
                persist: false,
                outcome: ProduceOutcome::Store,
                before_observe: Some(record::register_introduction(&introduction, meter)?),
            };
            legacy.produce(&mut txn, staged)?;
        }
        let mut channels = meter.buffer::<Par>(1)?;
        channels.push(handle);
        let mut patterns = meter.buffer::<BindPattern>(1)?;
        patterns.push(creation.handle_pattern);
        let handle = SpaceConsume {
            channels,
            patterns,
            continuation: creation.handle_continuation,
            persist: true,
            peeks: BTreeSet::new(),
            outcome: ConsumeOutcome::Park,
            before_observe: Some(record::register_introduction(&introduction, meter)?),
        };
        legacy.consume(&mut txn, handle)?;
        Ok(Prepared::new(txn, move |receipt| {
            legacy.finish()?;
            let committed = match receipt.ops.as_slice() {
                [SpaceOpReceipt::Stored { .. }, SpaceOpReceipt::Stored { .. }, stored @ .., SpaceOpReceipt::Parked { .. }] => {
                    stored.len() == extra
                        && stored
                            .iter()
                            .all(|op| matches!(op, SpaceOpReceipt::Stored { .. }))
                }
                _ => false,
            };
            match committed {
                true => Ok(()),
                false => Err(SpaceError::Inconsistent(
                    "a space creation committed other operations",
                )),
            }
        }))
    }

    async fn read_config(&self, env: &SpaceEnv, space: NativeSpace) -> Result<Par, SpaceError> {
        read_config_map(env, space).await
    }

    fn gensym(&self, space: NativeSpace, random: &Blake2b512Random) -> Result<Par, SpaceError> {
        self.outer.gensym(space, random)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use models::rhoapi::cost_signature::Value as CostSignatureValue;
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::tagged_continuation::TaggedCont;
    use models::rhoapi::{CostAuthority, CostRegion, CostSignature, Expr, ParWithRandom};
    use models::rust::host_work::{
        HostWorkDimension, HostWorkLimit, HostWorkLimits, HostWorkUsage,
    };
    use models::rust::space_factory_urn::{SpaceFactoryUrn, SpaceInner, SpaceOuter};
    use models::rust::utils::{
        new_freevar_par, new_freevar_var, new_gbool_par, new_gint_par, new_gstring_par,
    };
    use rspace_plus_plus::rspace::checkpoint::Checkpoint;
    use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
    use rspace_plus_plus::rspace::hashing::stable_hash_provider;
    use rspace_plus_plus::rspace::operation_context::{self, CausalPath, OperationOrder};
    use rspace_plus_plus::rspace::replay_rspace::ReplayRSpace;
    use rspace_plus_plus::rspace::rspace::RSpace;
    use rspace_plus_plus::rspace::rspace_interface::{ContResult, ISpace, RSpaceResult};
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    use rspace_plus_plus::rspace::space_store_handle::{
        SpaceStoreHandle, SpaceTxnError, SpaceTxnHost,
    };
    use rspace_plus_plus::rspace::trace::event::{Consume, Event, IOEvent, COMM};

    use super::*;
    use crate::rust::interpreter::accounting::authority::AuthorityByteEventKind;
    use crate::rust::interpreter::accounting::byte_accounting::{
        consume_introduction_identity, produce_introduction_identity,
    };
    use crate::rust::interpreter::accounting::costs::Cost;
    use crate::rust::interpreter::accounting::RuntimeBudget;
    use crate::rust::interpreter::host_work::HostWorkBudget;
    use crate::rust::interpreter::matcher::r#match::Matcher;
    use crate::rust::interpreter::reified::costs::{ReissueCause, ScanMeter, FIRE_ROUND_CAP};
    use crate::rust::interpreter::spaces::agent::run_op;
    use crate::rust::interpreter::spaces::counter::{counter_continuation, counter_pattern};
    use crate::rust::interpreter::spaces::disciplines::queue::QueueDiscipline;
    use crate::rust::interpreter::spaces::families::FamilyAgent;
    use crate::rust::interpreter::spaces::namespace::{Durability, SID_LEN};
    use crate::rust::interpreter::spaces::outer::hashmap::HashMapOuter;
    use crate::rust::interpreter::spaces::types::qualifier::SpaceQualifier;

    type Queue = GenericSpace<HashMapOuter, QueueDiscipline>;
    type ParRSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;
    type ParReplay = ReplayRSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

    const SPACE: NativeSpace = NativeSpace {
        sid: [7; SID_LEN],
        durability: Durability::Durable,
    };

    fn budget() -> RuntimeBudget { RuntimeBudget::new(Cost::create(1_000_000, "queue agent test")) }

    fn env_over<H>(host: H, budget: &RuntimeBudget) -> SpaceEnv
    where H: SpaceTxnHost<Par, BindPattern, ListParWithRandom, TaggedContinuation> + 'static {
        SpaceEnv {
            store: SpaceStoreHandle::new(host),
            budget: budget.clone(),
            meter: ScanMeter::new(None),
        }
    }

    async fn play() -> ParRSpace {
        let mut stores = InMemoryStoreManager::new();
        ParRSpace::create(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(Matcher)),
        )
        .expect("an in-memory space")
    }

    async fn play_and_replay() -> (ParRSpace, ParReplay) {
        let mut stores = InMemoryStoreManager::new();
        RSpace::create_with_replay(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(Matcher)),
        )
        .expect("an in-memory play and replay space")
    }

    fn int(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

    fn string(value: &str) -> Par { new_gstring_par(value.to_string(), Vec::new(), false) }

    fn region(byte: u8) -> CostAuthority {
        CostAuthority {
            regions: vec![CostRegion {
                instance_id: vec![byte; 32],
                signature: Some(CostSignature {
                    value: Some(CostSignatureValue::Ground(vec![byte])),
                }),
            }],
        }
    }

    fn config() -> Par {
        SpaceConfig::new(
            SPACE.sid,
            HashMapOuter::NAME,
            QueueDiscipline::NAME,
            SpaceQualifier::Default,
        )
        .to_par()
    }

    fn handle() -> Par { namespace::gprivate(namespace::handle_channel(SPACE)) }

    fn named(name: &str, guard: Option<Par>) -> TaggedContinuation {
        TaggedContinuation {
            tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                body: Some(string(name)),
                random_state: Blake2b512Random::create_from_bytes(name.as_bytes()).to_bytes(),
            })),
            guard,
            cost_authority: None,
        }
    }

    fn creation(config: Par) -> SpaceCreation {
        SpaceCreation {
            space: SPACE,
            config,
            handle_pattern: BindPattern {
                patterns: Vec::new(),
                remainder: Some(new_freevar_var(0)),
                free_count: 1,
            },
            handle_continuation: named("handle", None),
            random: Blake2b512Random::create_from_bytes(b"factory call"),
            authority: CostAuthority::default(),
            family: SpaceFactoryUrn::HASHMAP_QUEUE,
        }
    }

    async fn create(env: &SpaceEnv, creation: SpaceCreation) {
        Queue::default()
            .prepare_create(env, creation)
            .await
            .expect("the creation stages")
            .commit()
            .await
            .expect("the creation commits");
    }

    struct Fixture {
        space: ParRSpace,
        env: SpaceEnv,
    }

    async fn fixture() -> Fixture {
        let space = play().await;
        let env = env_over(space.clone(), &budget());
        create(&env, creation(config())).await;
        space.take_event_log().await;
        Fixture { space, env }
    }

    fn channel() -> Par { string("queue") }

    fn key_of(channel: &Par) -> CollectionKey { CollectionKey::of_channel(channel).expect("a key") }

    fn channels_of(channel: &Par) -> CollectionChannels {
        CollectionChannels::new(&HashMapOuter, SPACE, &key_of(channel))
    }

    fn channels() -> CollectionChannels { channels_of(&channel()) }

    fn send_on(channel: &Par, value: i64, persistent: bool) -> ProduceOp {
        ProduceOp {
            space: SPACE,
            key: key_of(channel),
            channel: channel.clone(),
            data: ListParWithRandom {
                pars: vec![int(value)],
                random_state: Blake2b512Random::create_from_bytes(
                    format!("send {value} {persistent}").as_bytes(),
                )
                .to_bytes(),
                cost_authority: None,
                cost_stack: None,
            },
            persistent,
            params: BTreeMap::new(),
            authority: CostAuthority::default(),
            reissued_ord: None,
        }
    }

    fn send(value: i64) -> ProduceOp { send_on(&channel(), value, false) }

    fn send_forever(value: i64) -> ProduceOp { send_on(&channel(), value, true) }

    fn any() -> BindPattern {
        BindPattern {
            patterns: vec![new_freevar_par(0, Vec::new())],
            remainder: None,
            free_count: 1,
        }
    }

    fn exactly(value: i64) -> BindPattern {
        BindPattern {
            patterns: vec![int(value)],
            remainder: None,
            free_count: 0,
        }
    }

    fn receive_on(channel: &Par, name: &str, pattern: BindPattern) -> ConsumeOp {
        let continuation = named(name, None);
        let body = match continuation.tagged_cont {
            Some(TaggedCont::ParBody(body)) => body,
            Some(TaggedCont::ScalaBodyRef(_)) | None => ParWithRandom::default(),
        };
        ConsumeOp {
            space: SPACE,
            key: key_of(channel),
            channel: channel.clone(),
            pattern,
            body,
            persistent: false,
            peek: false,
            guard: None,
            query: None,
            authority: CostAuthority::default(),
            introduction_authority: CostAuthority::default(),
            reissued_ord: None,
        }
    }

    fn receive(name: &str, pattern: BindPattern) -> ConsumeOp {
        receive_on(&channel(), name, pattern)
    }

    fn peek(name: &str) -> ConsumeOp {
        ConsumeOp {
            peek: true,
            ..receive(name, any())
        }
    }

    fn receive_forever(name: &str) -> ConsumeOp {
        ConsumeOp {
            persistent: true,
            ..receive(name, any())
        }
    }

    async fn run(env: &SpaceEnv, op: SpaceOp) -> Result<SpaceOutcome, SpaceError> {
        run_op(&Queue::default(), env, op).await
    }

    async fn produce(env: &SpaceEnv, op: ProduceOp) -> SpaceOutcome {
        run(env, SpaceOp::Produce(op))
            .await
            .expect("the produce commits")
    }

    async fn consume(env: &SpaceEnv, op: ConsumeOp) -> SpaceOutcome {
        run(env, SpaceOp::Consume(op))
            .await
            .expect("the consume commits")
    }

    fn body_name(continuation: &TaggedContinuation) -> String {
        match &continuation.tagged_cont {
            Some(TaggedCont::ParBody(ParWithRandom {
                body: Some(body), ..
            })) => match body.exprs.as_slice() {
                [Expr {
                    expr_instance: Some(ExprInstance::GString(name)),
                }] => name.clone(),
                _ => String::new(),
            },
            _ => String::new(),
        }
    }

    fn delivery(name: &str, value: i64) -> (String, Vec<Par>) {
        (name.to_string(), vec![int(value)])
    }

    /// The fire of the waiter `name` whose pattern is a literal, so it binds no value.
    fn literal_delivery(name: &str) -> (String, Vec<Par>) { (name.to_string(), Vec::new()) }

    /// The body name and the bound values of each fire.
    fn fired(outcome: &SpaceOutcome) -> Vec<(String, Vec<Par>)> {
        outcome
            .fires
            .iter()
            .map(|fire| {
                (
                    body_name(&fire.continuation),
                    fire.results
                        .iter()
                        .flat_map(|result| result.matched_datum.pars.clone())
                        .collect(),
                )
            })
            .collect()
    }

    /// The records of a collection as `(ord, payload, persistent)`, by `ord`.
    async fn records_on(space: &ParRSpace, channel: &Par) -> Vec<(i64, Vec<Par>, bool)> {
        let mut records: Vec<_> = space
            .get_data(channels_of(channel).data())
            .await
            .into_iter()
            .map(|datum| {
                let record = record::unwrap_datum(&datum.a).expect("a datum record");
                (record.ord, record.datum.pars, datum.persist)
            })
            .collect();
        records.sort_by_key(|(ord, _, _)| *ord);
        records
    }

    async fn records(space: &ParRSpace) -> Vec<(i64, Vec<Par>, bool)> {
        records_on(space, &channel()).await
    }

    /// The waiting continuations of a collection as `(ord, body, persistent, peek)`, by `ord`.
    async fn waiters_on(space: &ParRSpace, channel: &Par) -> Vec<(i64, String, bool, bool)> {
        let mut waiters: Vec<_> = space
            .get_waiting_continuations(vec![channels_of(channel).data().clone()])
            .await
            .into_iter()
            .map(|waiter| {
                let parked = record::unwrap_continuation(&waiter.continuation)
                    .expect("a continuation record");
                (
                    parked.ord,
                    body_name(&parked.continuation),
                    waiter.persist,
                    !waiter.peeks.is_empty(),
                )
            })
            .collect();
        waiters.sort_by_key(|(ord, ..)| *ord);
        waiters
    }

    async fn waiters(space: &ParRSpace) -> Vec<(i64, String, bool, bool)> {
        waiters_on(space, &channel()).await
    }

    fn waiter(ord: i64, name: &str, persistent: bool, peek: bool) -> (i64, String, bool, bool) {
        (ord, name.to_string(), persistent, peek)
    }

    async fn counter(space: &ParRSpace, channel: &Par) -> Vec<i64> {
        space
            .get_data(channel)
            .await
            .iter()
            .map(|datum| record::counter_value(&datum.a).expect("a counter record"))
            .collect()
    }

    fn name_of(hash: &Blake2b256Hash) -> &'static str {
        let ch = channels();
        let named = [
            ("C", ch.config()),
            ("S", ch.space_counter()),
            ("K", ch.counter()),
            ("D", ch.data()),
            ("H", &handle()),
        ];
        named
            .iter()
            .find(|(_, channel)| stable_hash_provider::hash(channel) == *hash)
            .map(|(name, _)| *name)
            .unwrap_or("?")
    }

    /// The events of a log as `kind channel` labels. A COMM with peeks is marked `peek`.
    fn labels(log: &[Event]) -> Vec<String> {
        log.iter()
            .map(|event| match event {
                Event::IoEvent(IOEvent::Produce(produce)) => {
                    format!("produce {}", name_of(&produce.channel_hash))
                }
                Event::IoEvent(IOEvent::Consume(consume)) => {
                    let names: Vec<&str> = consume.channel_hashes.iter().map(name_of).collect();
                    format!("consume {}", names.join(","))
                }
                Event::Comm(comm) => {
                    let names: Vec<&str> =
                        comm.consume.channel_hashes.iter().map(name_of).collect();
                    match comm.peeks.is_empty() {
                        true => format!("comm {}", names.join(",")),
                        false => format!("comm {} peek", names.join(",")),
                    }
                }
            })
            .collect()
    }

    const BUMP: [&str; 3] = ["consume K", "comm K", "produce K"];

    fn bumped(rest: &[&str]) -> Vec<String> {
        BUMP.iter()
            .chain(rest)
            .map(|label| label.to_string())
            .collect()
    }

    #[tokio::test]
    async fn space_queue_creation_stages_the_config_counter_and_handle() {
        let space = play().await;
        let env = env_over(space.clone(), &budget());
        create(&env, creation(config())).await;
        assert_eq!(labels(&space.take_event_log().await), vec![
            "produce C",
            "produce S",
            "consume H"
        ]);
        let ch = channels();
        let config_records = space.get_data(ch.config()).await;
        assert_eq!(config_records.len(), 1);
        assert!(config_records[0].persist);
        assert_eq!(record::config_map(&config_records[0].a), Ok(&config()));
        assert_eq!(
            config_records[0].a.random_state,
            RandomPurpose::Config
                .derive(&creation(config()).random)
                .to_bytes()
        );
        let counters = space.get_data(ch.space_counter()).await;
        assert_eq!(counter(&space, ch.space_counter()).await, vec![0]);
        assert!(!counters[0].persist);
        assert_eq!(
            counters[0].a.random_state,
            RandomPurpose::SpaceCounter
                .derive(&creation(config()).random)
                .split_byte(1)
                .to_bytes()
        );
        let handles = space.get_waiting_continuations(vec![handle()]).await;
        assert_eq!(handles.len(), 1);
        assert!(handles[0].persist);
        assert!(handles[0].peeks.is_empty());
        assert_eq!(handles[0].continuation, named("handle", None));
        assert_eq!(handles[0].patterns, vec![creation(config()).handle_pattern]);
        assert_eq!(
            Queue::default().read_config(&env, SPACE).await,
            Ok(config())
        );

        let again = Queue::default()
            .prepare_create(&env, creation(config()))
            .await;
        assert_eq!(again.err(), Some(SpaceError::SpaceAlreadyExists));
        assert!(space.take_event_log().await.is_empty());
        assert_eq!(space.get_data(ch.config()).await.len(), 1);
    }

    #[tokio::test]
    async fn space_queue_creation_rejects_a_foreign_or_malformed_config() {
        let space = play().await;
        let env = env_over(space.clone(), &budget());
        let bag = SpaceConfig::new(SPACE.sid, "hashmap", "bag", SpaceQualifier::Default).to_par();
        let elsewhere =
            SpaceConfig::new([9; SID_LEN], "hashmap", "queue", SpaceQualifier::Default).to_par();
        for (config, expected) in [
            (bag, SpaceError::FamilyUnavailable {
                outer: "hashmap".to_string(),
                inner: "bag".to_string(),
            }),
            (
                elsewhere,
                SpaceError::Inconsistent("the config of a new space names another space"),
            ),
            (
                int(1),
                SpaceError::Inconsistent("a malformed config record"),
            ),
        ] {
            let prepared = Queue::default()
                .prepare_create(&env, creation(config))
                .await;
            assert_eq!(prepared.err(), Some(expected));
        }
        assert!(space.take_event_log().await.is_empty());
        assert!(space.get_data(channels().config()).await.is_empty());
        assert_eq!(
            Queue::default().read_config(&env, SPACE).await,
            Err(SpaceError::DanglingHandle)
        );
    }

    #[tokio::test]
    async fn space_queue_first_operation_opens_the_collection_counter() {
        let Fixture { space, env } = fixture().await;
        let ch = channels();
        assert_eq!(produce(&env, send(1)).await, SpaceOutcome::default());
        assert_eq!(labels(&space.take_event_log().await), vec![
            "consume S",
            "comm S",
            "produce S",
            "produce K",
            "produce D"
        ]);
        assert_eq!(counter(&space, ch.space_counter()).await, vec![1]);
        assert_eq!(counter(&space, ch.counter()).await, vec![1]);
        assert_eq!(records(&space).await, vec![(0, vec![int(1)], false)]);

        produce(&env, send(2)).await;
        assert_eq!(
            labels(&space.take_event_log().await),
            bumped(&["produce D"])
        );
        assert_eq!(counter(&space, ch.counter()).await, vec![2]);
        assert_eq!(counter(&space, ch.space_counter()).await, vec![1]);
        assert_eq!(records(&space).await, vec![
            (0, vec![int(1)], false),
            (1, vec![int(2)], false)
        ]);

        let other = string("other");
        produce(&env, send_on(&other, 3, false)).await;
        assert_eq!(counter(&space, ch.space_counter()).await, vec![2]);
        assert_eq!(counter(&space, channels_of(&other).counter()).await, vec![
            1
        ]);
        assert_eq!(counter(&space, ch.counter()).await, vec![2]);
        assert_eq!(records_on(&space, &other).await, vec![(
            0,
            vec![int(3)],
            false
        )]);
    }

    #[tokio::test]
    async fn space_queue_delivers_records_and_waiters_in_fifo_order() {
        let Fixture { space, env } = fixture().await;
        for value in [1, 2, 3] {
            produce(&env, send(value)).await;
        }
        let mut delivered = Vec::new();
        for name in ["a", "b", "c"] {
            delivered.extend(fired(&consume(&env, receive(name, any())).await));
        }
        assert_eq!(delivered, vec![
            delivery("a", 1),
            delivery("b", 2),
            delivery("c", 3)
        ]);
        assert!(records(&space).await.is_empty());

        for name in ["x", "y", "z"] {
            consume(&env, receive(name, any())).await;
        }
        assert_eq!(waiters(&space).await, vec![
            waiter(6, "x", false, false),
            waiter(7, "y", false, false),
            waiter(8, "z", false, false)
        ]);
        let mut delivered = Vec::new();
        for value in [4, 5, 6] {
            delivered.extend(fired(&produce(&env, send(value)).await));
        }
        assert_eq!(delivered, vec![
            delivery("x", 4),
            delivery("y", 5),
            delivery("z", 6)
        ]);
        assert!(waiters(&space).await.is_empty());
        assert!(records(&space).await.is_empty());
    }

    #[tokio::test]
    async fn space_queue_data_head_blocks_later_records() {
        let Fixture { space, env } = fixture().await;
        produce(&env, send(1)).await;
        produce(&env, send(2)).await;
        space.take_event_log().await;
        let second = ListParWithRandom {
            pars: vec![int(2)],
            ..ListParWithRandom::default()
        };
        assert!(
            SpaceMatcher::new(&env)
                .bind(&exactly(2), &second)
                .expect("the match fits the budget")
                .is_some(),
            "negative control: a scan of the records would fire the second record"
        );
        let blocked = consume(&env, receive("w", exactly(2))).await;
        assert_eq!(blocked, SpaceOutcome::default());
        assert_eq!(
            labels(&space.take_event_log().await),
            bumped(&["consume D"])
        );
        assert_eq!(records(&space).await.len(), 2);
        assert_eq!(waiters(&space).await, vec![waiter(2, "w", false, false)]);

        let queued = consume(&env, receive("v", exactly(1))).await;
        assert!(
            queued.fires.is_empty(),
            "a receive waits behind the head waiter, although the head record matches it"
        );
        assert_eq!(waiters(&space).await, vec![
            waiter(2, "w", false, false),
            waiter(3, "v", false, false)
        ]);
    }

    #[tokio::test]
    async fn space_queue_waiter_head_blocks_later_waiters() {
        let Fixture { space, env } = fixture().await;
        consume(&env, receive("w1", exactly(1))).await;
        consume(&env, receive("w2", any())).await;
        space.take_event_log().await;
        let sent = send(2);
        assert!(
            SpaceMatcher::new(&env)
                .bind(&any(), &sent.data)
                .expect("the match fits the budget")
                .is_some(),
            "negative control: a scan of the waiters would fire w2"
        );
        assert_eq!(produce(&env, sent).await, SpaceOutcome::default());
        assert_eq!(
            labels(&space.take_event_log().await),
            bumped(&["produce D"])
        );
        assert_eq!(records(&space).await, vec![(2, vec![int(2)], false)]);
        assert_eq!(waiters(&space).await.len(), 2);

        assert_eq!(produce(&env, send(1)).await, SpaceOutcome::default());
        assert_eq!(
            records(&space).await,
            vec![(2, vec![int(2)], false), (3, vec![int(1)], false)],
            "a send joins the tail of the records, although the head waiter matches it"
        );
    }

    #[tokio::test]
    async fn space_queue_guard_of_the_head_waiter_blocks() {
        let Fixture { space, env } = fixture().await;
        let refusing = ConsumeOp {
            guard: Some(new_gbool_par(false, Vec::new(), false)),
            ..receive("w", any())
        };
        consume(&env, refusing).await;
        consume(&env, receive("v", any())).await;
        assert_eq!(produce(&env, send(1)).await, SpaceOutcome::default());
        assert_eq!(records(&space).await, vec![(2, vec![int(1)], false)]);
        assert_eq!(waiters(&space).await.len(), 2);

        let Fixture { space, env } = fixture().await;
        let admitting = ConsumeOp {
            guard: Some(new_gbool_par(true, Vec::new(), false)),
            ..receive("w", any())
        };
        consume(&env, admitting).await;
        assert_eq!(fired(&produce(&env, send(1)).await), vec![delivery("w", 1)]);
        assert!(records(&space).await.is_empty());
        assert!(waiters(&space).await.is_empty());
    }

    #[tokio::test]
    async fn space_queue_peek_restores_the_head_record() {
        let Fixture { space, env } = fixture().await;
        produce(&env, send(5)).await;
        let stored = data_produces(&space.take_event_log().await);
        assert_eq!(stored.len(), 1);
        let peeked = consume(&env, peek("p")).await;
        assert_eq!(fired(&peeked), vec![delivery("p", 5)]);
        assert!(
            !peeked.fires[0].results[0].persistent,
            "the result keeps the flag of the host"
        );
        assert_eq!(peeked.reissue, None);
        let log = space.take_event_log().await;
        assert_eq!(
            labels(&log),
            bumped(&["consume D", "comm D peek", "produce D"]),
            "the peek fire removes the record, and the re-store produces it again (Rule E)"
        );
        assert_eq!(
            data_produces(&log),
            stored,
            "the re-store has the identity of the first produce"
        );
        assert_eq!(records(&space).await, vec![(0, vec![int(5)], false)]);
        assert!(waiters(&space).await.is_empty());

        assert_eq!(fired(&consume(&env, receive("t", any())).await), vec![
            delivery("t", 5)
        ]);
        assert!(records(&space).await.is_empty());
    }

    #[tokio::test]
    async fn space_queue_peek_restore_registers_the_record_authority() {
        let space = play().await;
        let budget = budget();
        let _scope = budget.enter_comm_accounting_scope();
        let env = env_over(space.clone(), &budget);
        create(&env, creation(config())).await;
        let mut sending = send(5);
        sending.authority = region(2);
        sending.data.cost_authority = Some(region(2));
        produce(&env, sending).await;
        space.take_event_log().await;
        let peeking = ConsumeOp {
            introduction_authority: region(4),
            ..peek("p")
        };
        consume(&env, peeking).await;
        let log = space.take_event_log().await;
        let restore = log
            .iter()
            .rev()
            .find(|event| matches!(event, Event::IoEvent(IOEvent::Produce(_))))
            .expect("the re-store produces the record again");
        assert_eq!(
            registered(&budget, restore),
            Some(region(2)),
            "the re-store registers the authority of the record, not of the peek"
        );
        let peek_consume = log
            .iter()
            .find(|event| match event {
                Event::IoEvent(IOEvent::Consume(consume)) => {
                    consume.channel_hashes == vec![stable_hash_provider::hash(channels().data())]
                }
                _ => false,
            })
            .expect("the peek consume");
        assert_eq!(registered(&budget, peek_consume), Some(region(4)));
    }

    #[tokio::test]
    async fn space_queue_peek_of_a_persistent_record_restores_nothing() {
        let Fixture { space, env } = fixture().await;
        produce(&env, send_forever(5)).await;
        space.take_event_log().await;
        let peeked = consume(&env, peek("p")).await;
        assert_eq!(fired(&peeked), vec![delivery("p", 5)]);
        assert!(peeked.fires[0].results[0].persistent);
        assert_eq!(
            labels(&space.take_event_log().await),
            bumped(&["consume D", "comm D peek"]),
            "the fire keeps a persistent record, so nothing is stored again"
        );
        assert_eq!(records(&space).await, vec![(0, vec![int(5)], true)]);
    }

    #[tokio::test]
    async fn space_queue_peek_waiter_fire_reoffers_the_datum() {
        let Fixture { space, env } = fixture().await;
        consume(&env, peek("p")).await;
        consume(&env, receive("t", any())).await;
        space.take_event_log().await;
        let outcome = produce(&env, send(3)).await;
        assert_eq!(fired(&outcome), vec![delivery("p", 3), delivery("t", 3)]);
        assert!(
            !outcome.fires[0].results[0].persistent,
            "the incoming datum of a peek fire is not retained"
        );
        let log = space.take_event_log().await;
        assert_eq!(
            labels(&log),
            bumped(&["produce D", "comm D peek", "produce D", "comm D"])
        );
        assert_eq!(log[3], log[5], "the re-offer has the identity of the send");
        assert!(records(&space).await.is_empty());
        assert!(waiters(&space).await.is_empty());

        consume(&env, peek("q")).await;
        space.take_event_log().await;
        let outcome = produce(&env, send(4)).await;
        assert_eq!(fired(&outcome), vec![delivery("q", 4)]);
        assert_eq!(
            labels(&space.take_event_log().await),
            bumped(&["produce D", "comm D peek", "produce D"])
        );
        assert_eq!(records(&space).await, vec![(4, vec![int(4)], false)]);
    }

    #[tokio::test]
    async fn space_queue_persistent_send_fires_each_waiter_then_stores() {
        let Fixture { space, env } = fixture().await;
        consume(&env, receive("w1", any())).await;
        consume(&env, receive("w2", any())).await;
        space.take_event_log().await;
        let outcome = produce(&env, send_forever(4)).await;
        assert_eq!(fired(&outcome), vec![delivery("w1", 4), delivery("w2", 4)]);
        assert_eq!(outcome.reissue, None);
        assert_eq!(
            labels(&space.take_event_log().await),
            bumped(&["produce D", "comm D", "produce D", "comm D", "produce D"])
        );
        assert_eq!(records(&space).await, vec![(2, vec![int(4)], true)]);
        assert!(waiters(&space).await.is_empty());
    }

    #[tokio::test]
    async fn space_queue_persistent_receive_drains_then_parks() {
        let Fixture { space, env } = fixture().await;
        produce(&env, send(1)).await;
        produce(&env, send(2)).await;
        space.take_event_log().await;
        let outcome = consume(&env, receive_forever("r")).await;
        assert_eq!(fired(&outcome), vec![delivery("r", 1), delivery("r", 2)]);
        assert_eq!(outcome.reissue, None);
        assert_eq!(
            labels(&space.take_event_log().await),
            bumped(&["consume D", "comm D", "consume D", "comm D", "consume D"])
        );
        assert!(records(&space).await.is_empty());
        assert_eq!(waiters(&space).await, vec![waiter(2, "r", true, false)]);

        assert_eq!(fired(&produce(&env, send(3)).await), vec![delivery("r", 3)]);
        assert_eq!(waiters(&space).await, vec![waiter(2, "r", true, false)]);
        assert!(records(&space).await.is_empty());
    }

    #[tokio::test]
    async fn space_queue_divergent_pairs_reissue_after_one_round() {
        let Fixture { space, env } = fixture().await;
        consume(&env, receive_forever("r")).await;
        space.take_event_log().await;
        let outcome = produce(&env, send_forever(9)).await;
        assert_eq!(fired(&outcome), vec![delivery("r", 9)]);
        assert_eq!(
            outcome.reissue,
            Some(SpaceReissue {
                op: reissued_send(send_forever(9), 1),
                cause: ReissueCause::DivergentPair,
            })
        );
        assert_eq!(
            labels(&space.take_event_log().await),
            bumped(&["produce D", "comm D"])
        );
        assert!(records(&space).await.is_empty());
        assert_eq!(waiters(&space).await, vec![waiter(0, "r", true, false)]);
        let reissue = outcome.reissue.expect("a divergent pair re-issues");
        let again = run(&env, reissue.op).await.expect("the re-issue commits");
        assert_eq!(fired(&again), vec![delivery("r", 9)]);
        assert_eq!(
            again.reissue,
            Some(SpaceReissue {
                op: reissued_send(send_forever(9), 1),
                cause: ReissueCause::DivergentPair,
            }),
            "every round keeps the order of the first round"
        );
        assert_eq!(counter(&space, channels().counter()).await, vec![3]);

        let Fixture { space, env } = fixture().await;
        assert_eq!(
            produce(&env, send_forever(8)).await,
            SpaceOutcome::default()
        );
        let outcome = consume(&env, receive_forever("s")).await;
        assert_eq!(fired(&outcome), vec![delivery("s", 8)]);
        assert_eq!(
            outcome.reissue,
            Some(SpaceReissue {
                op: SpaceOp::Consume(ConsumeOp {
                    reissued_ord: Some(1),
                    ..receive_forever("s")
                }),
                cause: ReissueCause::DivergentPair,
            })
        );
        assert_eq!(records(&space).await, vec![(0, vec![int(8)], true)]);
        assert!(waiters(&space).await.is_empty());
        let peeked = consume(&env, peek("p")).await;
        assert_eq!(fired(&peeked), vec![delivery("p", 8)]);
        assert_eq!(peeked.reissue, None, "a non-persistent receive is spent");
    }

    #[tokio::test]
    async fn space_queue_round_cap_reissues_the_rest() {
        let Fixture { space, env } = fixture().await;
        let waiting = FIRE_ROUND_CAP + 1;
        for index in 0..waiting {
            consume(&env, receive(&format!("w{index}"), any())).await;
        }
        space.take_event_log().await;
        let outcome = produce(&env, send_forever(1)).await;
        assert_eq!(outcome.fires.len(), FIRE_ROUND_CAP);
        let expected: Vec<_> = (0..FIRE_ROUND_CAP)
            .map(|index| delivery(&format!("w{index}"), 1))
            .collect();
        assert_eq!(fired(&outcome), expected);
        let last = i64::try_from(FIRE_ROUND_CAP).expect("a small cap");
        assert_eq!(
            outcome.reissue,
            Some(SpaceReissue {
                op: reissued_send(send_forever(1), last + 1),
                cause: ReissueCause::RoundCap,
            })
        );
        assert_eq!(space.take_event_log().await.len(), 3 + 2 * FIRE_ROUND_CAP);
        assert!(records(&space).await.is_empty());
        assert_eq!(waiters(&space).await, vec![waiter(
            last,
            &format!("w{FIRE_ROUND_CAP}"),
            false,
            false
        )]);
        let rest = run(
            &env,
            outcome.reissue.expect("the chain re-issues the rest").op,
        )
        .await
        .expect("the re-issue commits");
        assert_eq!(fired(&rest), vec![delivery(
            &format!("w{FIRE_ROUND_CAP}"),
            1
        )]);
        assert_eq!(rest.reissue, None);
        assert_eq!(
            records(&space).await,
            vec![(last + 1, vec![int(1)], true)],
            "the persistent record keeps the order of its first round"
        );

        let Fixture { space, env } = fixture().await;
        for value in 0..=last {
            produce(&env, send(value)).await;
        }
        let outcome = consume(&env, receive_forever("r")).await;
        let expected: Vec<_> = (0..last).map(|value| delivery("r", value)).collect();
        assert_eq!(fired(&outcome), expected);
        assert_eq!(
            outcome.reissue.as_ref().map(|reissue| reissue.cause),
            Some(ReissueCause::RoundCap)
        );
        assert_eq!(records(&space).await, vec![(last, vec![int(last)], false)]);
        assert!(waiters(&space).await.is_empty());
        let rest = run(&env, outcome.reissue.expect("a re-issue").op)
            .await
            .expect("the re-issue commits");
        assert_eq!(fired(&rest), vec![delivery("r", last)]);
        assert!(records(&space).await.is_empty());
        assert_eq!(waiters(&space).await, vec![waiter(
            last + 1,
            "r",
            true,
            false
        )]);
    }

    /// Parks `count` plain waiters `w{first}` .. on the collection. Each takes every value.
    async fn park_plain(env: &SpaceEnv, first: usize, count: usize) {
        for index in first..first + count {
            consume(env, receive(&format!("w{index}"), any())).await;
        }
    }

    /// Parks `count` peek waiters `p0` .. on the collection. Each takes every value.
    async fn park_peeks(env: &SpaceEnv, count: usize) {
        for index in 0..count {
            consume(env, ConsumeOp {
                peek: true,
                ..receive(&format!("p{index}"), any())
            })
            .await;
        }
    }

    /// `FIRE_ROUND_CAP` as an order.
    fn cap() -> i64 { i64::try_from(FIRE_ROUND_CAP).expect("a small cap") }

    /// The re-issue of the persistent send `op` with the first-round order `ord` (decision 43).
    fn reissued_send(op: ProduceOp, ord: i64) -> SpaceOp {
        SpaceOp::Produce(ProduceOp {
            reissued_ord: Some(ord),
            ..op
        })
    }

    /// The produce events of a log on the data channel of the collection.
    fn data_produces(log: &[Event]) -> Vec<Produce> {
        let data = stable_hash_provider::hash(channels().data());
        log.iter()
            .filter_map(|event| match event {
                Event::IoEvent(IOEvent::Produce(produce)) if produce.channel_hash == data => {
                    Some(produce.clone())
                }
                _ => None,
            })
            .collect()
    }

    #[tokio::test]
    async fn space_queue_reissue_keeps_the_first_round_ord_and_identity() {
        let Fixture { space, env } = fixture().await;
        park_plain(&env, 0, FIRE_ROUND_CAP + 1).await;
        let first = cap() + 1;
        space.take_event_log().await;
        let outcome = produce(&env, send_forever(1)).await;
        assert_eq!(
            outcome.reissue,
            Some(SpaceReissue {
                op: reissued_send(send_forever(1), first),
                cause: ReissueCause::RoundCap,
            })
        );
        let round_one = data_produces(&space.take_event_log().await);
        assert_eq!(round_one.len(), FIRE_ROUND_CAP);
        let rest = run(&env, outcome.reissue.expect("a re-issue").op)
            .await
            .expect("the re-issue commits");
        assert_eq!(fired(&rest), vec![delivery(
            &format!("w{FIRE_ROUND_CAP}"),
            1
        )]);
        assert_eq!(rest.reissue, None);
        let round_two = data_produces(&space.take_event_log().await);
        assert_eq!(round_two.len(), 2, "the fire and the store");
        assert!(
            round_one
                .iter()
                .chain(&round_two)
                .all(|produce| *produce == round_one[0]),
            "every round stages the same record, so the same identity"
        );
        assert_eq!(records(&space).await, vec![(first, vec![int(1)], true)]);
        assert_eq!(
            counter(&space, channels().counter()).await,
            vec![first + 2],
            "each round bumps the counter"
        );
    }

    #[tokio::test]
    async fn space_queue_reissue_reenters_ahead_of_younger_records() {
        let Fixture { space, env } = fixture().await;
        park_plain(&env, 0, FIRE_ROUND_CAP).await;
        consume(&env, receive("one", exactly(1))).await;
        let first = cap() + 1;
        let outcome = produce(&env, send_forever(1)).await;
        assert_eq!(outcome.fires.len(), FIRE_ROUND_CAP);
        let reissue = outcome.reissue.expect("the cap re-issues the send");
        // A younger datum arrives between the rounds. The head waiter takes only 1, so the datum
        // is stored while the send is in flight.
        assert_eq!(produce(&env, send(7)).await, SpaceOutcome::default());
        assert_eq!(records(&space).await, vec![(
            first + 1,
            vec![int(7)],
            false
        )]);
        let rest = run(&env, reissue.op).await.expect("the re-issue commits");
        assert_eq!(
            fired(&rest),
            vec![literal_delivery("one")],
            "the send re-enters at its place, ahead of 7"
        );
        assert_eq!(rest.reissue, None);
        assert_eq!(records(&space).await, vec![
            (first, vec![int(1)], true),
            (first + 1, vec![int(7)], false)
        ]);
        assert!(waiters(&space).await.is_empty());
    }

    #[tokio::test]
    async fn space_queue_reissue_stores_behind_an_older_reentry() {
        let Fixture { space, env } = fixture().await;
        park_plain(&env, 0, 2 * FIRE_ROUND_CAP).await;
        consume(&env, receive("two", exactly(2))).await;
        let (older_ord, younger_ord) = (2 * cap() + 1, 2 * cap() + 2);
        let older = produce(&env, send_forever(1))
            .await
            .reissue
            .expect("the first send re-issues");
        let younger = produce(&env, send_forever(2))
            .await
            .reissue
            .expect("the second send re-issues");
        assert_eq!(older.op, reissued_send(send_forever(1), older_ord));
        assert_eq!(younger.op, reissued_send(send_forever(2), younger_ord));
        // The older send re-enters first. The head waiter takes only 2, so the older send is
        // stored at the head. The younger send then finds an older record at the head, so it
        // is stored at its place behind that record and fires nothing, although the head
        // waiter takes 2.
        let stored = run(&env, older.op)
            .await
            .expect("the older re-entry commits");
        assert_eq!(stored, SpaceOutcome::default());
        let behind = run(&env, younger.op)
            .await
            .expect("the younger re-entry commits");
        assert_eq!(behind, SpaceOutcome::default());
        assert_eq!(records(&space).await, vec![
            (older_ord, vec![int(1)], true),
            (younger_ord, vec![int(2)], true)
        ]);
        assert_eq!(waiters(&space).await, vec![waiter(
            2 * cap(),
            "two",
            false,
            false
        )]);
    }

    #[tokio::test]
    async fn space_queue_shadowed_pair_resolves_at_reentry() {
        let Fixture { space, env } = fixture().await;
        park_peeks(&env, FIRE_ROUND_CAP).await;
        consume(&env, receive("one", exactly(1))).await;
        let first = cap() + 1;
        let outcome = produce(&env, send_forever(1)).await;
        assert_eq!(outcome.fires.len(), FIRE_ROUND_CAP);
        let reissue = outcome.reissue.expect("the peek waiters reach the cap");
        produce(&env, send(2)).await;
        for index in 0..FIRE_ROUND_CAP {
            consume(&env, receive(&format!("z{index}"), any())).await;
        }
        // Round 2 re-enters ahead of 2. It fires the head waiter and 63 waiters behind it, and
        // it reaches the cap again.
        let outcome = run(&env, reissue.op).await.expect("round 2 commits");
        assert_eq!(outcome.fires.len(), FIRE_ROUND_CAP);
        assert_eq!(fired(&outcome)[0], literal_delivery("one"));
        let reissue = outcome.reissue.expect("round 2 reaches the cap");
        assert_eq!(reissue.op, reissued_send(send_forever(1), first));
        // The heads 2 and z63 match. The send in flight precedes the head record, so it shadows
        // the pair.
        let last = format!("z{}", FIRE_ROUND_CAP - 1);
        assert_eq!(records(&space).await, vec![(
            first + 1,
            vec![int(2)],
            false
        )]);
        assert_eq!(waiters(&space).await, vec![waiter(
            first + 1 + cap(),
            &last,
            false,
            false
        )]);
        // Round 3 re-enters ahead of 2 again, fires z63, and stores the send at its place.
        let outcome = run(&env, reissue.op).await.expect("round 3 commits");
        assert_eq!(fired(&outcome), vec![delivery(&last, 1)]);
        assert_eq!(outcome.reissue, None);
        assert_eq!(records(&space).await, vec![
            (first, vec![int(1)], true),
            (first + 1, vec![int(2)], false)
        ]);
        assert!(waiters(&space).await.is_empty());
    }

    #[tokio::test]
    async fn space_queue_nonpersistent_reissue_takes_a_fresh_ord() {
        let Fixture { space, env } = fixture().await;
        park_peeks(&env, FIRE_ROUND_CAP).await;
        consume(&env, receive("one", exactly(1))).await;
        let outcome = produce(&env, send(1)).await;
        assert_eq!(outcome.fires.len(), FIRE_ROUND_CAP);
        let reissue = outcome.reissue.expect("the peek waiters reach the cap");
        assert_eq!(
            reissue.op,
            SpaceOp::Produce(send(1)),
            "a datum that is not persistent keeps no order"
        );
        produce(&env, send(2)).await;
        consume(&env, receive("z", any())).await;
        // The re-issued datum takes the order of its new bump, so it joins the tail behind 2. A
        // datum that kept its first order would fire the head waiter and leave the matching
        // heads 2 and z with nothing in flight.
        let rest = run(&env, reissue.op).await.expect("the re-issue commits");
        assert_eq!(rest, SpaceOutcome::default());
        assert_eq!(records(&space).await, vec![
            (cap() + 2, vec![int(2)], false),
            (cap() + 4, vec![int(1)], false)
        ]);
        assert_eq!(waiters(&space).await, vec![
            waiter(cap(), "one", false, false),
            waiter(cap() + 3, "z", false, false)
        ]);
    }

    #[tokio::test]
    async fn space_queue_reissued_receive_keeps_its_first_round_ord() {
        let Fixture { space, env } = fixture().await;
        for value in 0..cap() {
            produce(&env, send(value)).await;
        }
        produce(&env, send(100)).await;
        let first = cap() + 1;
        let outcome = consume(&env, receive_forever("r")).await;
        assert_eq!(outcome.fires.len(), FIRE_ROUND_CAP);
        let reissue = outcome.reissue.expect("the drain reaches the cap");
        assert_eq!(
            reissue.op,
            SpaceOp::Consume(ConsumeOp {
                reissued_ord: Some(first),
                ..receive_forever("r")
            })
        );
        // A younger waiter parks behind the record 100 while the receive is in flight.
        consume(&env, receive("young", exactly(7))).await;
        let rest = run(&env, reissue.op).await.expect("the re-issue commits");
        assert_eq!(
            fired(&rest),
            vec![delivery("r", 100)],
            "the receive re-enters ahead of the younger waiter"
        );
        assert_eq!(rest.reissue, None);
        assert!(records(&space).await.is_empty());
        assert_eq!(waiters(&space).await, vec![
            waiter(first, "r", true, false),
            waiter(first + 1, "young", false, false)
        ]);
    }

    #[tokio::test]
    async fn space_queue_reentry_rejects_an_inconsistent_order() {
        let Fixture { space, env } = fixture().await;
        produce(&env, send(5)).await;
        for ord in [-1, 2, i64::MAX] {
            assert_eq!(
                run(&env, reissued_send(send_forever(1), ord)).await,
                Err(SpaceError::Inconsistent(
                    "a re-issued order is not an order of the counter"
                )),
                "the order {ord}"
            );
        }
        assert_eq!(
            run(&env, reissued_send(send_forever(1), 0)).await,
            Err(SpaceError::Inconsistent(
                "a re-issued element shares its order with a stored entry"
            ))
        );
        assert_eq!(
            records(&space).await,
            vec![(0, vec![int(5)], false)],
            "a failed operation changes nothing"
        );
        assert_eq!(counter(&space, channels().counter()).await, vec![1]);
    }

    /// Runs `op`, then runs its re-issues while `rounds` allows, and returns every outcome.
    async fn run_rounds(
        env: &SpaceEnv,
        op: SpaceOp,
        rounds: usize,
    ) -> Result<Vec<SpaceOutcome>, SpaceError> {
        let mut outcomes = Vec::with_capacity(rounds);
        let mut next = Some(op);
        for _ in 0..rounds {
            let Some(op) = next.take() else {
                break;
            };
            let outcome = run(env, op).await?;
            next = outcome.reissue.as_ref().map(|reissue| reissue.op.clone());
            outcomes.push(outcome);
        }
        Ok(outcomes)
    }

    /// A script of re-entries: a divergent pair for three rounds, and a capped send that
    /// re-enters ahead of a younger record.
    async fn run_reentry_script(env: &SpaceEnv) -> Result<Vec<SpaceOutcome>, SpaceError> {
        let divergent = string("divergent");
        let capped = string("capped");
        let mut outcomes = Vec::new();
        outcomes.push(
            run(
                env,
                SpaceOp::Consume(ConsumeOp {
                    persistent: true,
                    ..receive_on(&divergent, "s", any())
                }),
            )
            .await?,
        );
        outcomes.extend(run_rounds(env, SpaceOp::Produce(send_on(&divergent, 8, true)), 3).await?);
        for index in 0..FIRE_ROUND_CAP {
            outcomes.push(
                run(
                    env,
                    SpaceOp::Consume(receive_on(&capped, &format!("w{index}"), any())),
                )
                .await?,
            );
        }
        outcomes.push(
            run(
                env,
                SpaceOp::Consume(receive_on(&capped, "one", exactly(1))),
            )
            .await?,
        );
        let first = run(env, SpaceOp::Produce(send_on(&capped, 1, true))).await?;
        let reissue = first.reissue.clone().ok_or(SpaceError::Inconsistent(
            "the capped send does not re-issue",
        ))?;
        outcomes.push(first);
        outcomes.push(run(env, SpaceOp::Produce(send_on(&capped, 7, false))).await?);
        outcomes.push(run(env, reissue.op).await?);
        Ok(outcomes)
    }

    #[tokio::test]
    async fn space_queue_replay_reproduces_the_reentries() {
        let (play, replay) = play_and_replay().await;
        let play_env = env_over(play.clone(), &budget());
        create(&play_env, creation(config())).await;
        let start = play
            .create_checkpoint()
            .await
            .expect("a checkpoint after the creation")
            .root;
        let played = run_reentry_script(&play_env)
            .await
            .expect("the script plays");
        let divergent: Vec<_> = played[1..4]
            .iter()
            .map(|outcome| outcome.reissue.as_ref().map(|reissue| reissue.cause))
            .collect();
        assert_eq!(divergent, vec![Some(ReissueCause::DivergentPair); 3]);
        assert_eq!(fired(played.last().expect("the re-entry")), vec![
            literal_delivery("one")
        ]);
        let Checkpoint { root, log } = play.create_checkpoint().await.expect("a checkpoint");
        let divergent_comms: Vec<&COMM> = log
            .iter()
            .filter_map(|event| match event {
                Event::Comm(comm)
                    if comm.consume.channel_hashes
                        == vec![stable_hash_provider::hash(
                            channels_of(&string("divergent")).data(),
                        )] =>
                {
                    Some(comm)
                }
                _ => None,
            })
            .collect();
        assert_eq!(divergent_comms.len(), 3);
        assert!(
            divergent_comms
                .iter()
                .all(|comm| *comm == divergent_comms[0]),
            "the rounds of a divergent pair repeat one COMM"
        );

        replay
            .rig_and_reset(start, log)
            .await
            .expect("the replay rigs the play log");
        let replay_env = env_over(replay.clone(), &budget());
        let replayed = run_reentry_script(&replay_env)
            .await
            .expect("the script replays");
        assert_eq!(replayed, played);
        replay
            .check_replay_data()
            .await
            .expect("every recorded COMM is paired, the repeated ones included");
        assert_eq!(
            replay.create_checkpoint().await.expect("a checkpoint").root,
            root
        );
    }

    #[tokio::test]
    async fn space_queue_every_operation_bumps_the_counter() {
        let Fixture { space, env } = fixture().await;
        let ch = channels();
        produce(&env, send(1)).await;
        assert_eq!(counter(&space, ch.counter()).await, vec![1]);
        space.take_event_log().await;
        let operations = [
            SpaceOp::Consume(peek("p")),
            SpaceOp::Consume(receive("t", any())),
            SpaceOp::Consume(receive("w", exactly(4))),
            SpaceOp::Produce(send(2)),
        ];
        for (index, op) in operations.into_iter().enumerate() {
            run(&env, op).await.expect("the operation commits");
            let log = labels(&space.take_event_log().await);
            assert_eq!(&log[..3], &BUMP.map(str::to_string), "operation {index}");
            let expected = i64::try_from(index).expect("a small index") + 2;
            assert_eq!(counter(&space, ch.counter()).await, vec![expected]);
        }
        assert_eq!(counter(&space, ch.space_counter()).await, vec![1]);
    }

    #[tokio::test]
    async fn space_queue_dangling_space_fails_closed() {
        let space = play().await;
        let env = env_over(space.clone(), &budget());
        let name = HashMapOuter
            .gensym(SPACE, &Blake2b512Random::create_from_bytes(b"a name"))
            .expect("a name");
        for (op, expected) in [
            (
                SpaceOp::Produce(send_on(&name, 1, false)),
                SpaceError::DanglingName,
            ),
            (SpaceOp::Produce(send(1)), SpaceError::DanglingHandle),
            (
                SpaceOp::Consume(receive_on(&name, "w", any())),
                SpaceError::DanglingName,
            ),
            (
                SpaceOp::Consume(receive("w", any())),
                SpaceError::DanglingHandle,
            ),
        ] {
            assert_eq!(run(&env, op).await, Err(expected));
        }
        assert_eq!(
            Queue::default().read_config(&env, SPACE).await,
            Err(SpaceError::DanglingHandle)
        );
        assert!(space.take_event_log().await.is_empty());
        assert!(space.get_data(channels().space_counter()).await.is_empty());
    }

    // Stages `config_records` on `C(sid)` and the space counter 0, outside the agent.
    async fn install(env: &SpaceEnv, config_records: Vec<ListParWithRandom>) {
        let ch = channels();
        let random = Blake2b512Random::create_from_bytes(b"install");
        let mut txn = env
            .store
            .begin(&[ch.config().clone(), ch.space_counter().clone()])
            .await
            .expect("a transaction");
        for data in config_records {
            txn.produce(SpaceProduce {
                channel: ch.config().clone(),
                data,
                persist: true,
                outcome: ProduceOutcome::Store,
                before_observe: None,
            })
            .expect("the config stages");
        }
        txn.produce(SpaceProduce {
            channel: ch.space_counter().clone(),
            data: record::counter_record(0, &random, None),
            persist: false,
            outcome: ProduceOutcome::Store,
            before_observe: None,
        })
        .expect("the counter stages");
        txn.commit().await.expect("the records commit");
    }

    #[tokio::test]
    async fn space_queue_rejects_a_foreign_family_or_a_corrupt_config() {
        let random = Blake2b512Random::create_from_bytes(b"config");
        let bag = SpaceConfig::new(SPACE.sid, "hashmap", "bag", SpaceQualifier::Default).to_par();
        let elsewhere =
            SpaceConfig::new([1; SID_LEN], "hashmap", "queue", SpaceQualifier::Default).to_par();
        let cases = [
            (
                vec![record::config_record(bag, &random)],
                SpaceError::FamilyUnavailable {
                    outer: "hashmap".to_string(),
                    inner: "bag".to_string(),
                },
            ),
            (
                vec![record::config_record(elsewhere, &random)],
                SpaceError::Inconsistent("the config record names another space"),
            ),
            (
                vec![
                    record::config_record(config(), &random),
                    record::config_record(config(), &random.split_byte(1)),
                ],
                SpaceError::Inconsistent("a space has two config records"),
            ),
            (
                vec![record::counter_record(0, &random, None)],
                SpaceError::Inconsistent("a malformed config record"),
            ),
        ];
        for (config_records, expected) in cases {
            let space = play().await;
            let env = env_over(space.clone(), &budget());
            install(&env, config_records).await;
            space.take_event_log().await;
            assert_eq!(
                run(&env, SpaceOp::Produce(send(1))).await,
                Err(expected.clone())
            );
            assert_eq!(
                run(&env, SpaceOp::Consume(receive("w", any()))).await,
                Err(expected)
            );
            assert!(space.take_event_log().await.is_empty());
        }
    }

    #[tokio::test]
    /// A join group that holds a channel outside the namespace of the space is foreign: the
    /// classification of P5c finds it while it computes the closure, and every operation on the
    /// collection fails with `Inconsistent` before it stages anything (decision 29). The P4
    /// version of this test expected the rejection of every join group.
    async fn space_queue_foreign_join_is_inconsistent() {
        let Fixture { space, env } = fixture().await;
        produce(&env, send(1)).await;
        let ch = channels();
        let mut group = vec![ch.data().clone(), int(5)];
        group.sort_by_key(stable_hash_provider::hash);
        let mut txn = env.store.begin(&group).await.expect("a transaction");
        txn.consume(SpaceConsume {
            channels: group.clone(),
            patterns: vec![any(), any()],
            continuation: named("join", None),
            persist: false,
            peeks: BTreeSet::new(),
            outcome: ConsumeOutcome::Park,
            before_observe: None,
        })
        .expect("the join parks");
        txn.commit().await.expect("the join commits");
        space.take_event_log().await;
        let joined = SpaceError::Inconsistent("a collection channel is part of a foreign join");
        assert_eq!(
            run(&env, SpaceOp::Produce(send(2))).await,
            Err(joined.clone())
        );
        assert_eq!(
            run(&env, SpaceOp::Consume(receive("w", any()))).await,
            Err(joined)
        );
        assert!(space.take_event_log().await.is_empty());
        assert_eq!(records(&space).await, vec![(0, vec![int(1)], false)]);
    }

    #[tokio::test]
    async fn space_queue_reads_stage_nothing_and_never_commit() {
        let Fixture { space, env } = fixture().await;
        let order = OperationOrder {
            session: [3; 32],
            path: CausalPath::from(vec![(0, 0)]),
        };
        let read =
            operation_context::scope(order.clone(), Queue::default().read_config(&env, SPACE))
                .await;
        assert_eq!(read, Ok(config()));
        assert!(space.take_event_log().await.is_empty());
        let first =
            operation_context::scope(order.clone(), run(&env, SpaceOp::Produce(send(1)))).await;
        assert_eq!(
            first,
            Ok(SpaceOutcome::default()),
            "the read claimed no parent order"
        );
        let second = operation_context::scope(order, run(&env, SpaceOp::Produce(send(2)))).await;
        assert_eq!(
            second,
            Err(SpaceError::Substrate(SpaceTxnError::ParentOrderReused)),
            "negative control: a second commit under one parent order is refused"
        );
    }

    #[tokio::test]
    async fn space_queue_names_route_to_their_space() {
        let Fixture { space, env } = fixture().await;
        let random = Blake2b512Random::create_from_bytes(b"gensym call");
        let name = Queue::default()
            .gensym(SPACE, &random)
            .expect("a name of the space");
        assert_eq!(HashMapOuter.gensym(SPACE, &random), Ok(name.clone()));
        produce(&env, send_on(&name, 5, false)).await;
        assert_eq!(records_on(&space, &name).await, vec![(
            0,
            vec![int(5)],
            false
        )]);
        assert_eq!(
            fired(&consume(&env, receive_on(&name, "n", any())).await),
            vec![delivery("n", 5)]
        );
        assert!(records_on(&space, &name).await.is_empty());
    }

    #[tokio::test]
    async fn space_queue_fires_dispatch_the_original_continuation_and_datum() {
        let Fixture { env, .. } = fixture().await;
        let waiting = ConsumeOp {
            guard: Some(new_gbool_par(true, Vec::new(), false)),
            authority: region(5),
            ..receive("w", any())
        };
        consume(&env, waiting.clone()).await;
        let sent = send(7);
        let outcome = produce(&env, sent.clone()).await;
        let [fire] = outcome.fires.as_slice() else {
            panic!("one fire: {outcome:?}");
        };
        assert_eq!(fire.continuation, TaggedContinuation {
            tagged_cont: Some(TaggedCont::ParBody(waiting.body.clone())),
            guard: waiting.guard.clone(),
            cost_authority: Some(region(5)),
        });
        let [result] = fire.results.as_slice() else {
            panic!("one result: {fire:?}");
        };
        assert_eq!(result.matched_datum, sent.data);
        assert_eq!(result.removed_datum, record::wrap_datum(1, &sent.data));
        assert_eq!(result.channel, *channels().data());
        assert!(!result.persistent);

        let stored = send(8);
        produce(&env, stored.clone()).await;
        let outcome = consume(&env, receive("t", any())).await;
        let [fire] = outcome.fires.as_slice() else {
            panic!("one fire: {outcome:?}");
        };
        assert_eq!(fire.continuation, named("t", None));
        assert_eq!(fire.results[0].matched_datum, stored.data);
        assert_eq!(
            fire.results[0].removed_datum,
            record::wrap_datum(2, &stored.data)
        );
    }

    fn registered(budget: &RuntimeBudget, event: &Event) -> Option<CostAuthority> {
        match event {
            Event::IoEvent(IOEvent::Produce(produce)) => budget
                .introduction_authority(
                    produce_introduction_identity(produce),
                    AuthorityByteEventKind::ProduceIntroduction,
                )
                .ok(),
            Event::IoEvent(IOEvent::Consume(consume)) => budget
                .introduction_authority(
                    consume_introduction_identity(consume),
                    AuthorityByteEventKind::ConsumeIntroduction,
                )
                .ok(),
            Event::Comm(_) => None,
        }
    }

    fn assert_registered(budget: &RuntimeBudget, log: &[Event], authority: &CostAuthority) {
        let mut checked = 0;
        for event in log {
            if let Event::IoEvent(_) = event {
                assert_eq!(
                    registered(budget, event).as_ref(),
                    Some(authority),
                    "{event:?}"
                );
                checked += 1;
            }
        }
        assert!(checked > 0);
    }

    #[tokio::test]
    async fn space_queue_records_carry_and_register_the_b7_authorities() {
        let space = play().await;
        let budget = budget();
        let _scope = budget.enter_comm_accounting_scope();
        let env = env_over(space.clone(), &budget);
        let ch = channels();
        create(&env, SpaceCreation {
            authority: region(1),
            ..creation(config())
        })
        .await;
        assert_registered(&budget, &space.take_event_log().await, &region(1));
        assert_eq!(space.get_data(ch.config()).await[0].a.cost_authority, None);
        assert_eq!(
            space.get_data(ch.space_counter()).await[0].a.cost_authority,
            Some(region(1))
        );
        let handles = space.get_waiting_continuations(vec![handle()]).await;
        assert_eq!(handles[0].continuation.cost_authority, None);

        let mut sending = send(1);
        sending.authority = region(2);
        sending.data.cost_authority = Some(region(2));
        produce(&env, sending.clone()).await;
        let log = space.take_event_log().await;
        assert_registered(&budget, &log, &region(2));
        let records = space.get_data(ch.data()).await;
        assert_eq!(records[0].a.cost_authority, Some(region(2)));
        assert_eq!(
            space.get_data(ch.counter()).await[0].a.cost_authority,
            Some(region(2))
        );
        let opening = Consume::create(
            &vec![ch.space_counter().clone()],
            &vec![counter_pattern()],
            &counter_continuation(
                0,
                &RandomPurpose::SpaceCounter
                    .derive(&Blake2b512Random::from_bytes(&sending.data.random_state))
                    .split_byte(0),
                Some(region(2)),
            ),
            false,
        );
        assert_eq!(log[0], Event::IoEvent(IOEvent::Consume(opening)));

        let parking = ConsumeOp {
            authority: region(3),
            introduction_authority: region(4),
            ..receive("w", exactly(99))
        };
        consume(&env, parking.clone()).await;
        let log = space.take_event_log().await;
        assert_registered(&budget, &log, &region(4));
        let parked = space
            .get_waiting_continuations(vec![ch.data().clone()])
            .await;
        assert_eq!(parked[0].continuation.cost_authority, Some(region(3)));
        assert_eq!(
            space.get_data(ch.counter()).await[0].a.cost_authority,
            Some(region(4))
        );
        let bump = Consume::create(
            &vec![ch.counter().clone()],
            &vec![counter_pattern()],
            &counter_continuation(
                1,
                &RandomPurpose::Counter
                    .derive(&Blake2b512Random::from_bytes(&parking.body.random_state))
                    .split_byte(0),
                Some(region(3)),
            ),
            false,
        );
        assert_eq!(log[0], Event::IoEvent(IOEvent::Consume(bump)));
    }

    fn script() -> Vec<SpaceOp> {
        let fifo = string("fifo");
        let reoffer = string("reoffer");
        let fan = string("fan");
        let drain = string("drain");
        let divergent = string("divergent");
        let peeking = ConsumeOp {
            peek: true,
            ..receive_on(&fifo, "p", any())
        };
        let peek_waiter = ConsumeOp {
            peek: true,
            ..receive_on(&reoffer, "q", any())
        };
        let draining = ConsumeOp {
            persistent: true,
            ..receive_on(&drain, "r", any())
        };
        let forever = ConsumeOp {
            persistent: true,
            ..receive_on(&divergent, "s", any())
        };
        vec![
            SpaceOp::Produce(send_on(&fifo, 1, false)),
            SpaceOp::Produce(send_on(&fifo, 2, false)),
            SpaceOp::Consume(peeking),
            SpaceOp::Consume(receive_on(&fifo, "a", any())),
            SpaceOp::Consume(receive_on(&fifo, "b", exactly(9))),
            SpaceOp::Consume(peek_waiter),
            SpaceOp::Consume(receive_on(&reoffer, "t", any())),
            SpaceOp::Produce(send_on(&reoffer, 3, false)),
            SpaceOp::Consume(receive_on(&fan, "w1", any())),
            SpaceOp::Consume(receive_on(&fan, "w2", any())),
            SpaceOp::Produce(send_on(&fan, 4, true)),
            SpaceOp::Produce(send_on(&drain, 5, false)),
            SpaceOp::Produce(send_on(&drain, 6, false)),
            SpaceOp::Consume(draining),
            SpaceOp::Produce(send_on(&drain, 7, false)),
            SpaceOp::Consume(forever),
            SpaceOp::Produce(send_on(&divergent, 8, true)),
        ]
    }

    async fn run_script(env: &SpaceEnv) -> Result<Vec<SpaceOutcome>, SpaceError> {
        let mut outcomes = Vec::new();
        for op in script() {
            outcomes.push(run(env, op).await?);
        }
        Ok(outcomes)
    }

    #[tokio::test]
    async fn space_queue_replay_stages_the_play_transactions() {
        let (play, replay) = play_and_replay().await;
        let play_env = env_over(play.clone(), &budget());
        create(&play_env, creation(config())).await;
        let start = play
            .create_checkpoint()
            .await
            .expect("a checkpoint after the creation")
            .root;
        let played = run_script(&play_env).await.expect("the script plays");
        assert_eq!(
            played
                .iter()
                .map(|outcome| outcome.fires.len())
                .sum::<usize>(),
            10
        );
        assert!(played[16].reissue.is_some());
        let Checkpoint { root, log } = play.create_checkpoint().await.expect("a checkpoint");

        replay
            .rig_and_reset(start.clone(), log)
            .await
            .expect("the replay rigs the play log");
        let replay_env = env_over(replay.clone(), &budget());
        let replayed = run_script(&replay_env).await.expect("the script replays");
        assert_eq!(replayed, played);
        replay
            .check_replay_data()
            .await
            .expect("every recorded COMM is paired");
        assert_eq!(
            replay.create_checkpoint().await.expect("a checkpoint").root,
            root
        );

        replay
            .rig_and_reset(start, Vec::new())
            .await
            .expect("the replay rigs an empty log");
        match run(&replay_env, script().remove(0)).await {
            Err(error @ SpaceError::ReplayDivergence(_)) => {
                assert_eq!(error.code(), "ReplayDivergence")
            }
            other => panic!("negative control: a log without the COMMs diverges: {other:?}"),
        }
    }

    #[test]
    fn space_queue_collection_lock_keys_cover_every_touched_channel() {
        let ch = channels();
        let key = key_of(&channel());
        assert_eq!(*ch.lock_keys(), [
            ch.config().clone(),
            ch.space_counter().clone(),
            ch.counter().clone(),
            ch.data().clone()
        ]);
        assert_eq!(
            namespace::reified_id_of(ch.config()),
            Some(ReifiedId::Config(SPACE))
        );
        assert_eq!(
            namespace::reified_id_of(ch.space_counter()),
            Some(ReifiedId::SpaceCounter(SPACE))
        );
        assert_eq!(
            namespace::reified_id_of(ch.counter()),
            Some(ReifiedId::CollectionCounter(SPACE, key.as_bytes()))
        );
        assert_eq!(
            namespace::reified_id_of(ch.data()),
            Some(ReifiedId::Data(SPACE, key.as_bytes()))
        );
    }

    #[test]
    fn space_queue_outcome_matches_receipts_to_staged_operations() {
        let stored = SpaceOpReceipt::Stored {
            produce: rspace_plus_plus::rspace::trace::event::Produce::create(
                &int(1),
                &int(2),
                false,
            ),
        };
        let consume_source = Consume::create(&vec![int(1)], &vec![any()], &named("k", None), false);
        let parked = SpaceOpReceipt::Parked {
            consume: consume_source.clone(),
        };
        let fired_with = |continuation: TaggedContinuation| SpaceOpReceipt::Fired {
            comm: COMM {
                consume: consume_source.clone(),
                produces: Vec::new(),
                peeks: BTreeSet::new(),
                times_repeated: BTreeMap::new(),
            },
            continuation: ContResult {
                continuation,
                persistent: false,
                channels: vec![int(1)],
                patterns: vec![any()],
                peek: false,
            },
            results: Vec::new(),
            produce: None,
        };
        let wrapped = record::wrap_continuation(3, &named("k", None)).expect("a record");
        let receipt = |ops| ParSpaceReceipt { ops };
        assert_eq!(
            outcome(
                receipt(vec![stored.clone()]),
                vec![Staged::Store],
                Vec::new(),
                None
            ),
            Ok(SpaceOutcome::default())
        );
        assert_eq!(
            outcome(
                receipt(vec![
                    fired_with(named("counter", None)),
                    stored.clone(),
                    fired_with(wrapped)
                ]),
                vec![
                    Staged::Counter(CounterOp::Take),
                    Staged::Counter(CounterOp::Put),
                    Staged::Fire
                ],
                Vec::new(),
                None
            ),
            Ok(SpaceOutcome {
                fires: vec![SpaceFire {
                    continuation: named("k", None),
                    results: Vec::new(),
                }],
                reissue: None,
            })
        );
        let differs = Err(SpaceError::Inconsistent(
            "a receipt differs from its staged operation",
        ));
        assert_eq!(
            outcome(
                receipt(vec![stored.clone()]),
                vec![Staged::Park],
                Vec::new(),
                None
            ),
            differs
        );
        assert_eq!(
            outcome(
                receipt(vec![parked]),
                vec![Staged::Counter(CounterOp::Put)],
                Vec::new(),
                None
            ),
            differs
        );
        assert_eq!(
            outcome(receipt(vec![stored]), vec![Staged::Fire], Vec::new(), None),
            differs
        );
        assert_eq!(
            outcome(receipt(Vec::new()), vec![Staged::Store], Vec::new(), None),
            Err(SpaceError::Inconsistent(
                "the receipt does not list the staged operations"
            ))
        );
        assert_eq!(
            outcome(
                receipt(vec![fired_with(named("bare", None))]),
                vec![Staged::Fire],
                Vec::new(),
                None
            ),
            Err(SpaceError::Inconsistent("a malformed continuation record"))
        );
    }

    /// A head-only test discipline with one slot on each side (F15), and a named send param and
    /// a query param whose values must be non-negative integers. The engine tests of S1 use it.
    /// The CELL and PQ batches own the real disciplines.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct Slot;

    impl Discipline for Slot {
        const NAME: &'static str = "slot";
        const USES_COUNTER: bool = true;
        const SEND_PARAMS: &'static [&'static str] = &["priority"];
        const QUERY_PARAMS: &'static [&'static str] = &["priority"];
        const CAPACITY: Option<usize> = Some(1);

        fn head_metered<T: Serialize>(
            &self,
            side: Side,
            entries: &[Ordered<'_, T>],
            meter: &ScanMeter,
        ) -> Result<Option<usize>, SpaceError> {
            QueueDiscipline.head_metered(side, entries, meter)
        }

        fn reaches_head(&self, side: Side, queued: usize) -> bool {
            QueueDiscipline.reaches_head(side, queued)
        }

        fn precedes(
            &self,
            side: Side,
            ord: i64,
            head: i64,
            meter: &ScanMeter,
        ) -> Result<bool, SpaceError> {
            QueueDiscipline.precedes(side, ord, head, meter)
        }

        fn check_values(
            &self,
            _config: &SpaceConfig,
            params: &BTreeMap<String, Par>,
            query: Option<&QueryParams>,
            _meter: &ScanMeter,
        ) -> Result<(), SpaceError> {
            let named = params
                .iter()
                .chain(query.into_iter().flat_map(|query| query.named.iter()));
            for (key, value) in named {
                match value.exprs.as_slice() {
                    [Expr {
                        expr_instance: Some(ExprInstance::GInt(level)),
                    }] if *level >= 0 => {}
                    _ => return Err(SpaceError::InvalidParam { key: key.clone() }),
                }
            }
            Ok(())
        }
    }

    type SlotSpace = GenericSpace<HashMapOuter, Slot>;

    /// What the plans of the probe discipline saw: the counter value of each round, and the
    /// kind of the operation of each settle hook.
    static PROBED: LazyLock<std::sync::Mutex<Vec<(Option<i64>, &'static str)>>> =
        LazyLock::new(|| std::sync::Mutex::new(Vec::new()));

    /// A queue that records what its plans see, for the engine test of `Cx::bumped` and of the
    /// views of the settle hooks.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct Probe;

    impl Probe {
        fn record(cx: &Cx<'_, '_>, kind: &'static str) {
            PROBED
                .lock()
                .expect("the probe log is not poisoned")
                .push((cx.bumped(), kind));
        }
    }

    impl Discipline for Probe {
        const NAME: &'static str = "probe";
        const USES_COUNTER: bool = true;
        const SEND_PARAMS: &'static [&'static str] = &[];

        fn head_metered<T: Serialize>(
            &self,
            side: Side,
            entries: &[Ordered<'_, T>],
            meter: &ScanMeter,
        ) -> Result<Option<usize>, SpaceError> {
            QueueDiscipline.head_metered(side, entries, meter)
        }

        fn reaches_head(&self, side: Side, queued: usize) -> bool {
            QueueDiscipline.reaches_head(side, queued)
        }

        fn precedes(
            &self,
            side: Side,
            ord: i64,
            head: i64,
            meter: &ScanMeter,
        ) -> Result<bool, SpaceError> {
            QueueDiscipline.precedes(side, ord, head, meter)
        }

        fn settle_before(
            &self,
            cx: &mut Cx<'_, '_>,
            op: OpView<'_>,
        ) -> Result<Option<SpaceReissue>, SpaceError> {
            Probe::record(cx, match op {
                OpView::Produce(_) => "before produce",
                OpView::Consume(_) => "before consume",
            });
            Ok(None)
        }

        fn settle_after(
            &self,
            cx: &mut Cx<'_, '_>,
            op: OpView<'_>,
        ) -> Result<Option<SpaceReissue>, SpaceError> {
            Probe::record(cx, match op {
                OpView::Produce(_) => "after produce",
                OpView::Consume(_) => "after consume",
            });
            Ok(None)
        }
    }

    /// A HashMap store with an allocator: the creation stages `A(sid) = 0`, and an allocation of
    /// `k` names takes the value `n` and puts `n + k`. The engine tests of P5d use it. The ARRAY
    /// batch owns the Array and Vector stores.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct Counted;

    impl OuterStore for Counted {
        const NAME: &'static str = "counted";
        const ALLOCATOR: bool = true;

        fn key(&self, channel: &Par) -> Result<CollectionKey, SpaceError> {
            HashMapOuter.key(channel)
        }

        fn data_channel(&self, space: NativeSpace, key: &CollectionKey) -> Par {
            HashMapOuter.data_channel(space, key)
        }

        fn counter_channel(&self, space: NativeSpace, key: &CollectionKey) -> Par {
            HashMapOuter.counter_channel(space, key)
        }

        fn creation_records(
            &self,
            space: NativeSpace,
            random: &Blake2b512Random,
            authority: &CostAuthority,
            meter: &ScanMeter,
        ) -> Result<Vec<(Par, ListParWithRandom)>, SpaceError> {
            let record = record::counter_record_metered(
                0,
                &RandomPurpose::Allocator.derive(random).split_byte(1),
                record::cost_authority_metered(authority, meter)?,
                meter,
            )?;
            Ok(vec![(
                namespace::gprivate(namespace::allocator_channel(space)),
                record,
            )])
        }

        fn allocate(
            &self,
            cx: &mut AllocCx<'_>,
            _config: &SpaceConfig,
            op: &AllocOp,
        ) -> Result<Vec<u64>, SpaceError> {
            let stored = cx.allocator_record()?.ok_or(SpaceError::Inconsistent(
                "a space without an allocator record",
            ))?;
            let value = record::counter_value_metered(&stored.a, cx.meter())?;
            let count = i64::try_from(op.count).map_err(|_| SpaceError::OutOfNames)?;
            let next = value.checked_add(count).ok_or(SpaceError::OutOfNames)?;
            cx.take(stored, value)?;
            cx.put(next)?;
            Ok((value..next)
                .map(|index| u64::try_from(index).expect("a non-negative index"))
                .collect())
        }

        fn gensym(&self, space: NativeSpace, random: &Blake2b512Random) -> Result<Par, SpaceError> {
            HashMapOuter.gensym(space, random)
        }
    }

    fn config_of(outer: &str, inner: &str) -> Par {
        SpaceConfig::new(SPACE.sid, outer, inner, SpaceQualifier::Default).to_par()
    }

    async fn slot_fixture() -> Fixture {
        let space = play().await;
        let env = env_over(space.clone(), &budget());
        SlotSpace::default()
            .prepare_create(&env, creation(config_of(HashMapOuter::NAME, Slot::NAME)))
            .await
            .expect("the creation stages")
            .commit()
            .await
            .expect("the creation commits");
        space.take_event_log().await;
        Fixture { space, env }
    }

    async fn run_slot(env: &SpaceEnv, op: SpaceOp) -> Result<SpaceOutcome, SpaceError> {
        run_op(&SlotSpace::default(), env, op).await
    }

    fn params(entries: &[(&str, Par)]) -> BTreeMap<String, Par> {
        entries
            .iter()
            .map(|(key, value)| (key.to_string(), value.clone()))
            .collect()
    }

    fn query(entries: &[(&str, Par)]) -> Option<QueryParams> {
        Some(QueryParams {
            payload: Vec::new(),
            named: params(entries),
        })
    }

    fn alloc_op(count: u64) -> AllocOp {
        AllocOp {
            space: SPACE,
            count,
            random_state: Blake2b512Random::create_from_bytes(format!("alloc {count}").as_bytes())
                .to_bytes(),
            authority: CostAuthority::default(),
        }
    }

    fn unbounded() -> HostWorkBudget {
        HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX)))
    }

    fn wrapped(name: &str) -> TaggedContinuation {
        record::wrap_continuation(0, &named(name, None)).expect("a continuation record")
    }

    /// Stages `stage` in a plan of its own over the collection on `channel()`, with the
    /// introduction of the empty authority, and commits it. It returns the outcome.
    async fn staged_plan(
        env: &SpaceEnv,
        stage: impl FnOnce(&mut Cx<'_, '_>) -> Result<(), SpaceError>,
    ) -> Result<SpaceOutcome, SpaceError> {
        let ch = channels();
        let mut txn = env.store.begin(ch.lock_keys()).await?;
        let matcher = SpaceMatcher::metered(env)?;
        let introduction = Introduction::new(&env.budget, &CostAuthority::default(), &env.meter)?;
        let mut plan = Plan {
            data: ch.data(),
            matcher: &matcher,
            introduction: &introduction,
            staged: env.meter.buffer::<Staged>(MAX_STAGED)?,
            prefixes: Vec::new(),
            bumped: None,
            legacy: LegacyChain::new(&env.budget),
            rounds: FireRounds::new(),
        };
        stage(&mut Cx {
            txn: &mut txn,
            plan: &mut plan,
        })?;
        let Plan {
            staged,
            prefixes,
            legacy,
            ..
        } = plan;
        Prepared::new(txn, move |receipt| {
            legacy.finish()?;
            outcome(receipt, staged, prefixes, None)
        })
        .commit()
        .await
    }

    /// The candidates of a consume fire over the records of the collection in `cx`, bound by
    /// `any()`, in the order of their `ord`.
    fn candidates_by_ord(cx: &mut Cx<'_, '_>) -> Result<Vec<ParCandidate>, SpaceError> {
        let data = cx.data_channel();
        let mut stored: Vec<(i64, usize, ParDatum)> = cx
            .data()?
            .iter()
            .enumerate()
            .map(|(index, datum)| {
                let ord = record::unwrap_datum(&datum.a).expect("a datum record").ord;
                (ord, index, datum.clone())
            })
            .collect();
        stored.sort_by_key(|(ord, ..)| *ord);
        let mut candidates = Vec::with_capacity(stored.len());
        for (_, index, head) in stored {
            let payload = record::unwrap_datum(&head.a).expect("a datum record").datum;
            let matched = cx
                .bind(&any(), &payload)?
                .expect("any() binds every payload");
            let view_index = i32::try_from(index).expect("a small index");
            candidates.push(receive_candidate(data.clone(), ReceiveFire {
                head,
                matched,
                view_index,
            }));
        }
        Ok(candidates)
    }

    #[test]
    fn space_engine_staged_kinds_stay_one_byte() {
        // A head-only plan reserves room for `MAX_STAGED` kinds before it stages, so the host
        // work of a queue operation depends on this size. The queue parity golden pins it.
        assert_eq!(size_of::<Staged>(), 1);
    }

    #[tokio::test]
    async fn space_engine_checks_params_and_queries_before_the_bump() {
        let ch = channels();
        // The queue takes no params and no query.
        let Fixture { space, env } = fixture().await;
        let unknown = ProduceOp {
            params: params(&[("prority", int(1))]),
            ..send(1)
        };
        assert_eq!(
            run(&env, SpaceOp::Produce(unknown)).await,
            Err(SpaceError::UnknownParam {
                key: "prority".to_string(),
                suggestion: None,
            })
        );
        let queried = ConsumeOp {
            query: query(&[]),
            ..receive("w", any())
        };
        assert_eq!(
            run(&env, SpaceOp::Consume(queried)).await,
            Err(SpaceError::QuerySpecUnsupported)
        );
        assert!(space.take_event_log().await.is_empty());
        assert!(counter(&space, ch.counter()).await.is_empty());
        assert_eq!(counter(&space, ch.space_counter()).await, vec![0]);
        produce(&env, send(1)).await;
        assert_eq!(counter(&space, ch.counter()).await, vec![1]);
        // A discipline with params checks the keys and then the values, before the bump.
        let Fixture { space, env } = slot_fixture().await;
        let cases = [
            (
                SpaceOp::Produce(ProduceOp {
                    params: params(&[("prority", int(1))]),
                    ..send(1)
                }),
                SpaceError::UnknownParam {
                    key: "prority".to_string(),
                    suggestion: Some("priority".to_string()),
                },
            ),
            (
                SpaceOp::Produce(ProduceOp {
                    params: params(&[("priority", string("high"))]),
                    ..send(1)
                }),
                SpaceError::InvalidParam {
                    key: "priority".to_string(),
                },
            ),
            (
                SpaceOp::Consume(ConsumeOp {
                    query: query(&[("level", int(1))]),
                    ..receive("w", any())
                }),
                SpaceError::UnknownParam {
                    key: "level".to_string(),
                    suggestion: None,
                },
            ),
            (
                SpaceOp::Consume(ConsumeOp {
                    query: query(&[("priority", int(-1))]),
                    ..receive("w", any())
                }),
                SpaceError::InvalidParam {
                    key: "priority".to_string(),
                },
            ),
        ];
        for (op, expected) in cases {
            assert_eq!(run_slot(&env, op).await, Err(expected));
        }
        assert!(space.take_event_log().await.is_empty());
        assert!(counter(&space, ch.counter()).await.is_empty());
        let accepted = ProduceOp {
            params: params(&[("priority", int(2))]),
            ..send(1)
        };
        run_slot(&env, SpaceOp::Produce(accepted))
            .await
            .expect("an accepted param commits");
        assert_eq!(counter(&space, ch.counter()).await, vec![1]);
    }

    #[tokio::test]
    async fn space_engine_plans_see_the_round_counter_and_the_operation() {
        let space = play().await;
        let env = env_over(space.clone(), &budget());
        let probe = GenericSpace::<HashMapOuter, Probe>::default();
        probe
            .prepare_create(&env, creation(config_of(HashMapOuter::NAME, Probe::NAME)))
            .await
            .expect("the creation stages")
            .commit()
            .await
            .expect("the creation commits");
        PROBED
            .lock()
            .expect("the probe log is not poisoned")
            .clear();
        for op in [
            SpaceOp::Produce(send(1)),
            SpaceOp::Consume(receive("w", any())),
            SpaceOp::Consume(receive("x", any())),
        ] {
            run_op(&probe, &env, op)
                .await
                .expect("the operation commits");
        }
        // Each round bumps the counter once, and each settle hook sees its operation.
        assert_eq!(
            PROBED
                .lock()
                .expect("the probe log is not poisoned")
                .clone(),
            vec![
                (Some(0), "before produce"),
                (Some(0), "after produce"),
                (Some(1), "before consume"),
                (Some(1), "after consume"),
                (Some(2), "before consume"),
                (Some(2), "after consume"),
            ]
        );
        // A plan of its own over a staged view has no bump.
        let Fixture { env, .. } = fixture().await;
        staged_plan(&env, |cx| {
            assert_eq!(cx.bumped(), None);
            Ok(())
        })
        .await
        .expect("an empty plan commits nothing");
    }

    #[tokio::test]
    async fn space_engine_capacity_abort_stages_nothing() {
        let Fixture { space, env } = slot_fixture().await;
        let ch = channels();
        run_slot(&env, SpaceOp::Produce(send(1)))
            .await
            .expect("the first record stores");
        space.take_event_log().await;
        assert_eq!(
            run_slot(&env, SpaceOp::Produce(send(2))).await,
            Err(SpaceError::CellOccupied)
        );
        assert!(space.take_event_log().await.is_empty());
        assert_eq!(records(&space).await, vec![(0, vec![int(1)], false)]);
        assert_eq!(counter(&space, ch.counter()).await, vec![1]);
        // The waiter side of another collection holds one waiter too.
        let other = string("other");
        let waiting = |name: &str| receive_on(&other, name, exactly(9));
        run_slot(&env, SpaceOp::Consume(waiting("w1")))
            .await
            .expect("the first waiter parks");
        space.take_event_log().await;
        assert_eq!(
            run_slot(&env, SpaceOp::Consume(waiting("w2"))).await,
            Err(SpaceError::CellOccupied)
        );
        assert!(space.take_event_log().await.is_empty());
        assert_eq!(waiters_on(&space, &other).await, vec![waiter(
            0, "w1", false, false
        )]);
        assert_eq!(counter(&space, channels_of(&other).counter()).await, vec![
            1
        ]);
    }

    #[tokio::test]
    async fn space_engine_group_fire_lines_up_and_dispatches_its_prefix() {
        let Fixture { space, env } = fixture().await;
        for value in 1..=3 {
            produce(&env, send(value)).await;
        }
        space.take_event_log().await;
        // A group over `[D; 3]` that binds one result: the outcome dispatches the result of the
        // first candidate, and the fire removes all three records.
        let outcome = staged_plan(&env, |cx| {
            let data = cx.data_channel();
            let candidates = candidates_by_ord(cx)?;
            let width = candidates.len();
            cx.fire_receive_group(
                vec![data.clone(); width],
                vec![any(); width],
                wrapped("k"),
                false,
                BTreeSet::new(),
                candidates,
                1,
            )
        })
        .await
        .expect("the group fire commits");
        assert_eq!(fired(&outcome), vec![delivery("k", 1)]);
        assert!(records(&space).await.is_empty());
        assert_eq!(labels(&space.take_event_log().await), vec![
            "consume D,D,D",
            "comm D,D,D"
        ]);
        // A group that does not line up, or binds no result or too many, stages nothing.
        produce(&env, send(4)).await;
        space.take_event_log().await;
        let misaligned = |binds: usize, extra_channel: bool| {
            let env = &env;
            async move {
                staged_plan(env, |cx| {
                    let data = cx.data_channel();
                    let candidates = candidates_by_ord(cx)?;
                    let width = candidates.len() + usize::from(extra_channel);
                    cx.fire_receive_group(
                        vec![data.clone(); width],
                        vec![any(); candidates.len()],
                        wrapped("k"),
                        false,
                        BTreeSet::new(),
                        candidates,
                        binds,
                    )
                })
                .await
            }
        };
        let rejected = Err(SpaceError::Inconsistent(
            "a fire group does not line up its channels, patterns, candidates and binds",
        ));
        assert_eq!(misaligned(1, true).await, rejected);
        assert_eq!(misaligned(0, false).await, rejected);
        assert_eq!(misaligned(2, false).await, rejected);
        assert!(space.take_event_log().await.is_empty());
        assert_eq!(records(&space).await, vec![(3, vec![int(4)], false)]);
    }

    #[tokio::test]
    async fn space_engine_retire_and_lift_fire_without_dispatch() {
        let Fixture { space, env } = fixture().await;
        produce(&env, send(1)).await;
        produce(&env, send(2)).await;
        space.take_event_log().await;
        for (round, value) in [(0, 1), (1, 2)] {
            let outcome = staged_plan(&env, |cx| {
                let data = cx.data_channel();
                let candidates = candidates_by_ord(cx)?;
                let first = candidates.into_iter().take(1).collect();
                match round {
                    0 => cx.retire(vec![data.clone()], vec![any()], wrapped("retire"), first),
                    _ => cx.lift(vec![data.clone()], vec![any()], wrapped("lift"), first),
                }
            })
            .await
            .expect("the internal fire commits");
            assert!(outcome.fires.is_empty(), "{value}");
            assert_eq!(labels(&space.take_event_log().await), vec![
                "consume D",
                "comm D"
            ]);
        }
        assert!(records(&space).await.is_empty());
        // An internal fire with no candidate, or one that does not line up, stages nothing.
        let rejected = Err(SpaceError::Inconsistent(
            "an internal fire does not line up its channels, patterns and candidates",
        ));
        assert_eq!(
            staged_plan(&env, |cx| cx.retire(
                Vec::new(),
                Vec::new(),
                wrapped("r"),
                Vec::new()
            ))
            .await,
            rejected
        );
        produce(&env, send(3)).await;
        space.take_event_log().await;
        assert_eq!(
            staged_plan(&env, |cx| {
                let candidates = candidates_by_ord(cx)?;
                cx.lift(Vec::new(), vec![any()], wrapped("l"), candidates)
            })
            .await,
            rejected
        );
        assert!(space.take_event_log().await.is_empty());
    }

    #[test]
    fn space_engine_outcome_dispatches_the_prefix_of_a_group_fire() {
        let result = |value: i64| RSpaceResult {
            channel: int(1),
            matched_datum: ListParWithRandom {
                pars: vec![int(value)],
                ..ListParWithRandom::default()
            },
            removed_datum: ListParWithRandom::default(),
            persistent: false,
        };
        let fired_with =
            |results: Vec<RSpaceResult<Par, ListParWithRandom>>| SpaceOpReceipt::Fired {
                comm: COMM {
                    consume: Consume::create(&vec![int(1)], &vec![any()], &named("k", None), false),
                    produces: Vec::new(),
                    peeks: BTreeSet::new(),
                    times_repeated: BTreeMap::new(),
                },
                continuation: ContResult {
                    continuation: wrapped("k"),
                    persistent: false,
                    channels: vec![int(1); results.len()],
                    patterns: vec![any(); results.len()],
                    peek: false,
                },
                results,
                produce: None,
            };
        let three = || vec![result(1), result(2), result(3)];
        let receipt = |ops| ParSpaceReceipt { ops };
        let dispatched = outcome(
            receipt(vec![fired_with(three())]),
            vec![Staged::FirePrefix],
            vec![1],
            None,
        )
        .expect("a prefix fire");
        assert_eq!(fired(&dispatched), vec![delivery("k", 1)]);
        let all = outcome(
            receipt(vec![fired_with(three())]),
            vec![Staged::Fire],
            Vec::new(),
            None,
        )
        .expect("a fire");
        assert_eq!(fired(&all), vec![("k".to_string(), vec![
            int(1),
            int(2),
            int(3)
        ])]);
        for kind in [Staged::Retire, Staged::Lift] {
            let internal = outcome(
                receipt(vec![fired_with(three())]),
                vec![kind],
                Vec::new(),
                None,
            )
            .expect("an internal fire");
            assert!(internal.fires.is_empty());
        }
        let cases = [
            (
                vec![Staged::FirePrefix],
                Vec::new(),
                "a prefix fire has no dispatched count",
            ),
            (
                vec![Staged::FirePrefix],
                vec![4],
                "a fire dispatches more results than its receipt holds",
            ),
            (
                vec![Staged::Fire],
                vec![1],
                "a dispatched count has no prefix fire",
            ),
        ];
        for (staged, prefixes, message) in cases {
            assert_eq!(
                outcome(receipt(vec![fired_with(three())]), staged, prefixes, None),
                Err(SpaceError::Inconsistent(message))
            );
        }
    }

    // ── The primitives of the join engine (P5c) ─────────────────────────────────────────────

    fn join_slot(bind: usize) -> record::JoinSlot {
        record::JoinSlot {
            ord: 0,
            bind,
            priority: None,
        }
    }

    #[test]
    fn space_engine_join_fire_outcome_restores_bind_order() {
        let result = |value: i64| RSpaceResult {
            channel: int(value),
            matched_datum: ListParWithRandom {
                pars: vec![int(value)],
                ..ListParWithRandom::default()
            },
            removed_datum: ListParWithRandom::default(),
            persistent: false,
        };
        let fired_with =
            |continuation: TaggedContinuation,
             results: Vec<RSpaceResult<Par, ListParWithRandom>>| {
                SpaceOpReceipt::Fired {
                    comm: COMM {
                        consume: Consume::create(
                            &vec![int(1); results.len()],
                            &vec![any(); results.len()],
                            &named("k", None),
                            false,
                        ),
                        produces: Vec::new(),
                        peeks: BTreeSet::new(),
                        times_repeated: BTreeMap::new(),
                    },
                    continuation: ContResult {
                        continuation,
                        persistent: false,
                        channels: vec![int(1); results.len()],
                        patterns: vec![any(); results.len()],
                        peek: false,
                    },
                    results,
                    produce: None,
                }
            };
        let receipt = |ops| ParSpaceReceipt { ops };
        // The receipt lists the results in group order, and slot i names the bind of result i:
        // result 10 belongs to bind 2, result 20 to bind 0 and result 30 to bind 1.
        let join = record::wrap_join_continuation(
            &[join_slot(2), join_slot(0), join_slot(1)],
            &named("k", None),
        )
        .expect("a join record");
        let three = || vec![result(10), result(20), result(30)];
        let dispatched = outcome(
            receipt(vec![fired_with(join.clone(), three())]),
            vec![Staged::JoinFire],
            Vec::new(),
            None,
        )
        .expect("a join fire");
        assert_eq!(dispatched.fires.len(), 1);
        assert_eq!(dispatched.fires[0].continuation, named("k", None));
        assert_eq!(fired(&dispatched), vec![("k".to_string(), vec![
            int(20),
            int(30),
            int(10)
        ])]);
        // A join fire decodes a record of version 2 only, and a fire a record of version 1 only.
        let malformed = Err(SpaceError::Inconsistent("a malformed continuation record"));
        assert_eq!(
            outcome(
                receipt(vec![fired_with(wrapped("k"), three())]),
                vec![Staged::JoinFire],
                Vec::new(),
                None,
            ),
            malformed
        );
        assert_eq!(
            outcome(
                receipt(vec![fired_with(join.clone(), three())]),
                vec![Staged::Fire],
                Vec::new(),
                None,
            ),
            malformed
        );
        // A receipt with another number of results than the record has slots is Inconsistent.
        assert_eq!(
            outcome(
                receipt(vec![fired_with(join, vec![result(1), result(2)])]),
                vec![Staged::JoinFire],
                Vec::new(),
                None,
            ),
            Err(SpaceError::Inconsistent(
                "a join fire has not one result for each slot"
            ))
        );
    }

    /// The data channels of the collections `a` and `b` in canonical group order (FR-1'), and the
    /// bind index of each group position, for a receive that binds `a` first and `b` second.
    fn join_group() -> (Vec<Par>, Vec<usize>) {
        let a = channels_of(&string("a")).data().clone();
        let b = channels_of(&string("b")).data().clone();
        match stable_hash_provider::hash(&a) <= stable_hash_provider::hash(&b) {
            true => (vec![a, b], vec![0, 1]),
            false => (vec![b, a], vec![1, 0]),
        }
    }

    /// The join waiter `name` of the receive `for (x <- a & y <- b)`, with a record of version 2.
    fn join_waiter(name: &str, binds: &[usize]) -> TaggedContinuation {
        let slots: Vec<record::JoinSlot> = binds.iter().map(|&bind| join_slot(bind)).collect();
        record::wrap_join_continuation(&slots, &named(name, None)).expect("a join record")
    }

    /// Stages `stage` in a plan of its own with the lock keys `keys`, whose own collection is
    /// the collection of the first data channel of `keys`, and commits it.
    async fn staged_group_plan(
        env: &SpaceEnv,
        keys: &[Par],
        stage: impl FnOnce(&mut Cx<'_, '_>) -> Result<(), SpaceError>,
    ) -> Result<SpaceOutcome, SpaceError> {
        let mut txn = env.store.begin(keys).await?;
        let matcher = SpaceMatcher::metered(env)?;
        let introduction = Introduction::new(&env.budget, &CostAuthority::default(), &env.meter)?;
        let mut plan = Plan {
            data: &keys[0],
            matcher: &matcher,
            introduction: &introduction,
            staged: env.meter.buffer::<Staged>(MAX_STAGED)?,
            prefixes: Vec::new(),
            bumped: None,
            legacy: LegacyChain::new(&env.budget),
            rounds: FireRounds::new(),
        };
        stage(&mut Cx {
            txn: &mut txn,
            plan: &mut plan,
        })?;
        let Plan {
            staged,
            prefixes,
            legacy,
            ..
        } = plan;
        Prepared::new(txn, move |receipt| {
            legacy.finish()?;
            outcome(receipt, staged, prefixes, None)
        })
        .commit()
        .await
    }

    /// The datum record of the payload `value` with the order 0, and its stored copy on `channel`.
    fn stored_on(channel: &Par, value: i64) -> (ListParWithRandom, ParDatum) {
        let record = record::wrap_datum(0, &send_on(channel, value, false).data);
        let datum = Datum {
            a: record.clone(),
            persist: false,
            source: Produce::create(channel, &record, false),
        };
        (record, datum)
    }

    #[tokio::test]
    async fn space_engine_join_group_fires_line_up_and_dispatch_in_bind_order() {
        let Fixture { space, env } = fixture().await;
        let (group, binds) = join_group();
        let a = channels_of(&string("a")).data().clone();
        let b = channels_of(&string("b")).data().clone();
        // Park the join waiter `k` on the group, and store 2 on b.
        staged_group_plan(&env, &group, |cx| {
            cx.park_on(
                group.clone(),
                vec![any(), any()],
                join_waiter("k", &binds),
                false,
                BTreeSet::new(),
            )?;
            let (record, _) = stored_on(&b, 2);
            let introduction = Arc::clone(cx.introduction());
            cx.store_on(&b, record, false, &introduction)
        })
        .await
        .expect("the park and the store commit");
        // The produce of 1 on a fires the waiter with the incoming datum and the stored record.
        let outcome = staged_group_plan(&env, &group, |cx| {
            let waiter = cx.group_waiters(&group)?[0].clone();
            let stored = cx.data_on(&b)?[0].clone();
            let (incoming, _) = stored_on(&a, 1);
            let source = Produce::create(&a, &incoming, false);
            let payload = |record: &ListParWithRandom| {
                record::unwrap_datum(record).expect("a datum record").datum
            };
            let incoming_match = cx.bind(&any(), &payload(&incoming))?.expect("any() binds");
            let stored_match = cx.bind(&any(), &payload(&stored.a))?.expect("any() binds");
            let candidates = group
                .iter()
                .map(|channel| match *channel == a {
                    true => incoming_candidate(
                        a.clone(),
                        source.clone(),
                        incoming.clone(),
                        false,
                        incoming_match.clone(),
                    ),
                    false => receive_candidate(b.clone(), ReceiveFire {
                        head: stored.clone(),
                        matched: stored_match.clone(),
                        view_index: 0,
                    }),
                })
                .collect();
            let introduction = Arc::clone(cx.introduction());
            cx.fire_offer_group(
                &a,
                incoming,
                false,
                waiter,
                group.clone(),
                candidates,
                &introduction,
            )
        })
        .await
        .expect("the group fire commits");
        // The body sees the value of a first and the value of b second, whatever the group order.
        assert_eq!(fired(&outcome), vec![("k".to_string(), vec![
            int(1),
            int(2)
        ])]);
        assert!(space.get_data(&b).await.is_empty());
        assert!(space
            .get_waiting_continuations(group.clone())
            .await
            .is_empty());
        assert!(space.get_joins(a.clone()).await.is_empty());
        // The consume side: store 3 on a and 4 on b, then fire a new join waiter over both.
        staged_group_plan(&env, &group, |cx| {
            let introduction = Arc::clone(cx.introduction());
            for (channel, value) in [(&a, 3), (&b, 4)] {
                let (record, _) = stored_on(channel, value);
                cx.store_on(channel, record, false, &introduction)?;
            }
            Ok(())
        })
        .await
        .expect("the stores commit");
        let outcome = staged_group_plan(&env, &group, |cx| {
            let mut candidates = Vec::new();
            for channel in &group {
                let stored = cx.data_on(channel)?[0].clone();
                let payload = record::unwrap_datum(&stored.a)
                    .expect("a datum record")
                    .datum;
                let matched = cx.bind(&any(), &payload)?.expect("any() binds");
                candidates.push(receive_candidate(channel.clone(), ReceiveFire {
                    head: stored,
                    matched,
                    view_index: 0,
                }));
            }
            cx.fire_join(
                group.clone(),
                vec![any(), any()],
                join_waiter("j", &binds),
                false,
                BTreeSet::new(),
                candidates,
            )
        })
        .await
        .expect("the join fire commits");
        assert_eq!(fired(&outcome), vec![("j".to_string(), vec![
            int(3),
            int(4)
        ])]);
        assert!(space.get_data(&a).await.is_empty());
        // A group fire that does not line up stages nothing.
        space.take_event_log().await;
        let misaligned = staged_group_plan(&env, &group, |cx| {
            cx.fire_join(
                vec![a.clone()],
                vec![any()],
                join_waiter("x", &binds),
                false,
                BTreeSet::new(),
                Vec::new(),
            )
        })
        .await;
        assert_eq!(
            misaligned,
            Err(SpaceError::Inconsistent(
                "a join fire does not line up its channels, patterns and candidates"
            ))
        );
        let misaligned = staged_group_plan(&env, &group, |cx| {
            let waiter = ParWaiter {
                patterns: vec![any(), any()],
                continuation: join_waiter("x", &binds),
                persist: false,
                peeks: BTreeSet::new(),
                source: Consume::create(&group, &vec![any(), any()], &named("x", None), false),
            };
            let (incoming, _) = stored_on(&a, 5);
            let introduction = Arc::clone(cx.introduction());
            cx.fire_offer_group(
                &a,
                incoming,
                false,
                waiter,
                group.clone(),
                Vec::new(),
                &introduction,
            )
        })
        .await;
        assert_eq!(
            misaligned,
            Err(SpaceError::Inconsistent(
                "a group fire does not line up its channels and candidates"
            ))
        );
        assert!(space.take_event_log().await.is_empty());
    }

    #[tokio::test]
    async fn space_engine_bump_on_stages_the_bump_of_the_collection() {
        let Fixture { space, env } = fixture().await;
        let ch = channels();
        let random = Blake2b512Random::create_from_bytes(b"touch");
        // Two bumps of the queue collection in one plan: the first opens the counter, the second
        // steps it, as the bumps of two operations do.
        staged_group_plan(&env, ch.lock_keys(), |cx| {
            let introduction = Arc::clone(cx.introduction());
            let authority = OperationAuthority {
                continuation: None,
                introduction: &introduction,
            };
            assert_eq!(cx.bump_on(ch.counters(), &random, authority)?, 0);
            assert_eq!(
                cx.bump_on(ch.counters(), &random.split_byte(1), authority)?,
                1
            );
            Ok(())
        })
        .await
        .expect("the bumps commit");
        assert_eq!(labels(&space.take_event_log().await), vec![
            "consume S",
            "comm S",
            "produce S",
            "produce K",
            "consume K",
            "comm K",
            "produce K"
        ]);
        assert_eq!(counter(&space, ch.counter()).await, vec![2]);
        assert_eq!(counter(&space, ch.space_counter()).await, vec![1]);
        // A produce on the collection now takes the next order.
        produce(&env, send(7)).await;
        assert_eq!(records(&space).await, vec![(2, vec![int(7)], false)]);
    }

    #[test]
    fn space_engine_join_peek_set_covers_every_bind() {
        let host_work = unbounded();
        let meter = ScanMeter::new(Some(host_work.clone()));
        assert_eq!(peek_set_all(3, &meter), Ok(BTreeSet::from([0, 1, 2])));
        assert!(host_work.usage(HostWorkDimension::SearchStateBytes).get() > 0);
        assert_eq!(peek_set_all(0, &meter), Ok(BTreeSet::new()));
        assert_eq!(peek_set_all(1, &meter), peek_set(true, &meter));
    }

    #[test]
    fn space_engine_staged_list_growth_is_reserved_before_it_grows() {
        let operations = HostWorkDimension::VerificationOperations;
        let bytes = HostWorkDimension::SearchStateBytes;
        let host_work = unbounded();
        let meter = ScanMeter::new(Some(host_work.clone()));
        let mut staged = meter.buffer::<Staged>(2).expect("room for two kinds");
        let reserved = (host_work.usage(operations), host_work.usage(bytes));
        // A push within the reserved room reserves nothing.
        push_reserved(&mut staged, Staged::Store, &meter).expect("a push");
        push_reserved(&mut staged, Staged::Park, &meter).expect("a push");
        assert_eq!(
            (host_work.usage(operations), host_work.usage(bytes)),
            reserved
        );
        // A push at the capacity reserves the doubled buffer before the list grows, as
        // `reserve_buffer` reserves a vector: `length + 1` operations and the bytes.
        let grown = staged.capacity() * 2;
        push_reserved(&mut staged, Staged::Fire, &meter).expect("the growth fits");
        assert_eq!(
            host_work.usage(operations),
            HostWorkUsage::new(reserved.0.get() + (grown as u64 + 1))
        );
        assert_eq!(
            host_work.usage(bytes),
            HostWorkUsage::new(reserved.1.get() + (grown * size_of::<Staged>()) as u64)
        );
        assert_eq!(staged, vec![Staged::Store, Staged::Park, Staged::Fire]);
        // A budget one operation short of the growth rejects it and leaves the list as it was.
        let mut full = ScanMeter::new(None)
            .buffer::<Staged>(2)
            .expect("room for two kinds");
        full.extend([Staged::Store, Staged::Park]);
        let mut limits = HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX));
        limits.set(operations, HostWorkLimit::new(full.capacity() as u64 * 2));
        let cut = HostWorkBudget::new(limits);
        assert_eq!(
            push_reserved(&mut full, Staged::Fire, &ScanMeter::new(Some(cut.clone()))),
            Err(SpaceError::Host(RSpaceError::HostWorkRejected))
        );
        assert!(cut.is_rejected());
        assert_eq!(full, vec![Staged::Store, Staged::Park]);
        // An empty list grows to one entry.
        let mut empty: Vec<usize> = Vec::new();
        push_reserved(&mut empty, 7, &ScanMeter::new(None)).expect("a push");
        assert_eq!(empty, vec![7]);
    }

    #[test]
    fn space_engine_operation_ord_without_a_counter_is_zero() {
        let meter = ScanMeter::new(None);
        assert_eq!(operation_ord(None, None, &meter), Ok(0));
        assert_eq!(operation_ord(Some(0), None, &meter), Ok(0));
        assert_eq!(
            operation_ord(Some(3), None, &meter),
            Err(SpaceError::Inconsistent(
                "a re-issued order of a collection without a counter is not 0"
            ))
        );
        assert_eq!(operation_ord(None, Some(5), &meter), Ok(5));
        assert_eq!(operation_ord(Some(2), Some(5), &meter), Ok(2));
        assert_eq!(
            operation_ord(Some(5), Some(5), &meter),
            Err(SpaceError::Inconsistent(
                "a re-issued order is not an order of the counter"
            ))
        );
    }

    #[tokio::test]
    async fn space_engine_allocation_without_an_allocator_is_baseline() {
        let Fixture { space, env } = fixture().await;
        let queue = Queue::default();
        for agent in [&queue as &dyn SpaceAgent, &FamilyAgent] {
            match agent.allocate(&env, alloc_op(2)).await {
                Ok(Allocation::Baseline) => {}
                Ok(Allocation::Indices(_)) => panic!("a HashMap space has no allocator"),
                Err(error) => panic!("the allocation fails: {error}"),
            }
            assert!(space.take_event_log().await.is_empty());
        }
        let allocator = namespace::gprivate(namespace::allocator_channel(SPACE));
        assert!(space.get_data(&allocator).await.is_empty());
        // A space without a config is a dangling handle.
        let empty = play().await;
        let env = env_over(empty.clone(), &budget());
        for agent in [&queue as &dyn SpaceAgent, &FamilyAgent] {
            assert!(matches!(
                agent.allocate(&env, alloc_op(1)).await,
                Err(SpaceError::DanglingHandle)
            ));
        }
        assert!(empty.take_event_log().await.is_empty());
    }

    #[tokio::test]
    async fn space_engine_allocator_store_creates_and_allocates_its_names() {
        let space = play().await;
        let env = env_over(space.clone(), &budget());
        let agent = GenericSpace::<Counted, QueueDiscipline>::default();
        agent
            .prepare_create(
                &env,
                creation(config_of(Counted::NAME, QueueDiscipline::NAME)),
            )
            .await
            .expect("the creation stages")
            .commit()
            .await
            .expect("the creation commits");
        let allocator = namespace::gprivate(namespace::allocator_channel(SPACE));
        let created = space.take_event_log().await;
        assert_eq!(
            created.len(),
            4,
            "the config, the counter, the allocator and the handle"
        );
        assert_eq!(counter(&space, &allocator).await, vec![0]);
        for (count, expected) in [(3, vec![0, 1, 2]), (2, vec![3, 4])] {
            let indices = match agent
                .allocate(&env, alloc_op(count))
                .await
                .expect("the allocation stages")
            {
                Allocation::Indices(prepared) => {
                    prepared.commit().await.expect("the allocation commits")
                }
                Allocation::Baseline => panic!("the store has an allocator"),
            };
            assert_eq!(indices, expected);
            assert_eq!(
                space.take_event_log().await.len(),
                3,
                "the take of the allocator and the put of its next value"
            );
        }
        assert_eq!(counter(&space, &allocator).await, vec![5]);
    }

    #[tokio::test]
    async fn space_engine_family_agent_runs_a_queue_as_its_own_agent_does() {
        let script = || {
            vec![
                SpaceOp::Produce(send(1)),
                SpaceOp::Consume(peek("p")),
                SpaceOp::Consume(receive("w", any())),
                SpaceOp::Consume(receive("x", exactly(2))),
                SpaceOp::Produce(send(2)),
                SpaceOp::Produce(send_forever(3)),
            ]
        };
        let queue = Queue::default();
        let mut runs = Vec::new();
        for agent in [&queue as &dyn SpaceAgent, &FamilyAgent] {
            let space = play().await;
            let budget = budget();
            let host_work = unbounded();
            let env = SpaceEnv {
                store: SpaceStoreHandle::new(space.clone()),
                budget: budget.clone(),
                meter: ScanMeter::new(Some(host_work.clone())),
            };
            agent
                .prepare_create(&env, creation(config()))
                .await
                .expect("the creation stages")
                .commit()
                .await
                .expect("the creation commits");
            let mut fires = Vec::new();
            for op in script() {
                let outcome = run_op(agent, &env, op)
                    .await
                    .expect("the operation commits");
                fires.push(fired(&outcome));
            }
            let usage: Vec<HostWorkUsage> = HostWorkDimension::ALL
                .into_iter()
                .map(|dimension| host_work.usage(dimension))
                .collect();
            runs.push((
                fires,
                labels(&space.take_event_log().await),
                records(&space).await,
                waiters(&space).await,
                usage,
            ));
        }
        assert_eq!(runs[0], runs[1]);
    }

    #[tokio::test]
    async fn space_engine_family_agent_rejects_a_family_that_is_not_wired() {
        let random = Blake2b512Random::create_from_bytes(b"config");
        // The config names no family, so no arm is ever wired for it.
        let unknown = config_of("hashmap", "deque");
        let unavailable = SpaceError::FamilyUnavailable {
            outer: "hashmap".to_string(),
            inner: "deque".to_string(),
        };
        let space = play().await;
        let env = env_over(space.clone(), &budget());
        install(&env, vec![record::config_record(unknown.clone(), &random)]).await;
        space.take_event_log().await;
        for op in [
            SpaceOp::Produce(send(1)),
            SpaceOp::Consume(receive("w", any())),
        ] {
            assert_eq!(
                run_op(&FamilyAgent, &env, op).await,
                Err(unavailable.clone())
            );
        }
        assert!(matches!(
            FamilyAgent.allocate(&env, alloc_op(1)).await,
            Err(error) if error == unavailable
        ));
        assert!(space.take_event_log().await.is_empty());
        // A creation goes to the agent of the family of its factory, which checks the config.
        let fresh = play().await;
        let env = env_over(fresh.clone(), &budget());
        // HashSet is wired in P7, so its factory has no agent yet.
        let unwired_factory = SpaceCreation {
            family: SpaceFactoryUrn {
                outer: SpaceOuter::HashSet,
                inner: SpaceInner::Set,
            },
            ..creation(config())
        };
        assert_eq!(
            FamilyAgent
                .prepare_create(&env, unwired_factory)
                .await
                .err(),
            Some(SpaceError::FamilyUnavailable {
                outer: "hashset".to_string(),
                inner: "set".to_string(),
            })
        );
        let mislabeled = SpaceCreation {
            family: SpaceFactoryUrn::HASHMAP_QUEUE,
            ..creation(unknown)
        };
        assert_eq!(
            FamilyAgent.prepare_create(&env, mislabeled).await.err(),
            Some(unavailable)
        );
        assert!(fresh.take_event_log().await.is_empty());
    }
}
