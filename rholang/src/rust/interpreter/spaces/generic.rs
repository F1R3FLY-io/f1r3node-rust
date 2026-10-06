//! The generic native space (P4 design B.4): an outer store and a discipline over the substrate.
//!
//! [`GenericSpace<S, D>`](GenericSpace) is the [`SpaceAgent`] of the family whose outer store is
//! `S` and whose inner collection is `D`. P4 has one family, `GenericSpace<HashMapOuter,
//! QueueDiscipline>`.
//!
//! A collection operation runs in one transaction:
//!
//! 1. `begin` with the lock keys `[C(sid), S(sid), S(sid,k), D(sid,k)]`: every channel that the
//!    operation can touch (readiness review S2).
//! 2. Read the config record on `C(sid)`. Without it the operation fails with `DanglingName` for
//!    a space name and with `DanglingHandle` for every other channel.
//! 3. Check the family of the config (`FamilyUnavailable`) and that `D(sid,k)` holds no join
//!    other than `[D(sid,k)]` (`Inconsistent`).
//! 4. Bump the collection counter (`counter::bump`). The value before the bump is the `ord` of the
//!    operation.
//! 5. Plan and stage the operation, head-only (decision 11): `offer` for a produce, `receive`
//!    for a consume.
//! 6. The job commits the transaction exactly once (`Prepared::commit`). Every operation stages
//!    the bump, so no commit is empty (readiness review S3).
//! 7. The receipts become the `SpaceOutcome`: the fired continuations without their envelopes,
//!    in staging order, and the re-issue. A counter fire is never dispatched.
//!
//! The produce plan is the `OFFER` of the substrate design (decision 30):
//!
//! ```text
//! if the datum does not reach the head of the records: stage Store, stop    ▷ FIFO tail
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
//! when it is persistent or the consume peeks. A divergent pair re-issues after one round (A10),
//! and a chain re-issues after `FIRE_ROUND_CAP` rounds (I-21). Each round makes at most two
//! metered matcher calls, and the bump makes one more.
//!
//! The plans read the staged view, so play and replay stage the same operations: the heads
//! come from the `ord` of the envelopes, never from the list order of the store. Each staged
//! operation registers the introduction authority of its operation (P4 design B.7). A re-offer
//! and a re-fire repeat an identity with its first authority (readiness review S6).
//!
//! An operation reserves each of its reads and copies on the meter of the job before it makes
//! it (decision 42): the ids of its channels, the config decode, the envelope decode of each
//! entry of a head scan, each comparison, the copy of the head, and the copies of each staged
//! operation. It copies only the head that it meets, and a fire that ends the loop moves its
//! values instead of copying them. The outcome after the commit only moves values out of its
//! receipt, and the operation prepays that work at staging, so a host-work rejection never
//! follows the commit (`AgentReadCoverage.v`).

use std::collections::BTreeSet;
use std::sync::{Arc, LazyLock};

use async_trait::async_trait;
use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::internal::{ConsumeCandidate, Datum, WaitingContinuation};
use rspace_plus_plus::rspace::space_store_handle::{
    incoming_candidate, ConsumeOutcome, ProduceOutcome, SpaceConsume, SpaceOpReceipt, SpaceProduce,
};
use rspace_plus_plus::rspace::trace::event::Produce;
use serde::Serialize;
use shared::rust::collection_backing::tree_backing;

use super::agent::{
    ConsumeOp, ParSpaceReceipt, ParSpaceTxn, Prepared, ProduceOp, SpaceAgent, SpaceCreation,
    SpaceEnv, SpaceFire, SpaceOp, SpaceOutcome, SpaceReissue,
};
use super::counter::{self, BumpContext, CounterChannels, CounterOp};
use super::errors::SpaceError;
use super::matching::SpaceMatcher;
use super::namespace::{self, CollectionKey, NativeSpace, RandomPurpose, ReifiedId};
use super::record::{self, Introduction, OperationAuthority};
use super::types::config::SpaceConfig;
use crate::rust::interpreter::reified::costs::{FireRounds, FireStep, ScanMeter, FIRE_ROUND_CAP};

/// A record of a collection in the staged view.
pub type ParDatum = Datum<ListParWithRandom>;

/// A waiting continuation of a collection in the staged view.
pub type ParWaiter = WaitingContinuation<BindPattern, TaggedContinuation>;

/// The outer store of a family: how a channel names a collection, and where the records and the
/// counter of a collection live.
pub trait OuterStore: Default + Send + Sync + 'static {
    /// The `outer` name of the family in the config record.
    const NAME: &'static str;

    /// The key of the collection that `channel` names. Routing computes the same key.
    fn key(&self, channel: &Par) -> Result<CollectionKey, SpaceError>;

    /// The channel of the records and waiting continuations of the collection `key`.
    fn data_channel(&self, space: NativeSpace, key: &CollectionKey) -> Par;

    /// The counter channel of the collection `key`.
    fn counter_channel(&self, space: NativeSpace, key: &CollectionKey) -> Par;

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

/// The inner collection of a family: the order in which the entries of a collection interact.
pub trait Discipline: Default + Send + Sync + 'static {
    /// The `inner` name of the family in the config record.
    const NAME: &'static str;

    /// True when every operation bumps the collection counter (M-1, decision 31).
    const USES_COUNTER: bool;

    /// True when a receive may join this collection with other channels (P5).
    const JOINS: bool;

    /// The named send params of the discipline.
    const SEND_PARAMS: &'static [&'static str];

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

    /// True when an incoming element of `side` becomes the head of its side, which holds
    /// `queued` entries.
    fn reaches_head(&self, side: Side, queued: usize) -> bool;
}

/// The native space agent of the family `S:D`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct GenericSpace<S, D> {
    outer: S,
    discipline: D,
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
static GPRIVATE: LazyLock<Par> = LazyLock::new(|| namespace::gprivate(Vec::new()));

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

/// The kind of a staged operation, to check its receipt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Staged {
    Counter(CounterOp),
    Store,
    Park,
    Fire,
}

/// What the plans of one operation share.
struct Plan<'a> {
    data: &'a Par,
    matcher: &'a SpaceMatcher<'a>,
    /// The introduction of the operation, which every staged operation registers.
    introduction: &'a Arc<Introduction>,
    staged: Vec<Staged>,
}

impl<'a> Plan<'a> {
    /// The meter of the job. Its lifetime is the lifetime of the job, not of a borrow of the
    /// plan, so a plan step can stage while the caller holds the meter.
    fn meter(&self) -> &'a ScanMeter { self.matcher.meter() }

    fn store(
        &mut self,
        txn: &mut ParSpaceTxn,
        record: ListParWithRandom,
        persist: bool,
    ) -> Result<(), SpaceError> {
        let meter = self.meter();
        txn.produce(SpaceProduce {
            channel: meter.copy(self.data)?,
            data: record,
            persist,
            outcome: ProduceOutcome::Store,
            before_observe: Some(record::register_introduction(self.introduction, meter)?),
        })?;
        self.staged.push(Staged::Store);
        Ok(())
    }

    fn park(
        &mut self,
        txn: &mut ParSpaceTxn,
        op: &ConsumeOp,
        continuation: TaggedContinuation,
        peeks: BTreeSet<i32>,
    ) -> Result<(), SpaceError> {
        let meter = self.meter();
        txn.consume(SpaceConsume {
            channels: self.channel_list()?,
            patterns: self.pattern_list(op)?,
            continuation,
            persist: op.persistent,
            peeks,
            outcome: ConsumeOutcome::Park,
            before_observe: Some(record::register_introduction(self.introduction, meter)?),
        })?;
        self.staged.push(Staged::Park);
        Ok(())
    }

    /// The one-channel group of the collection, with its buffer and its copy reserved.
    fn channel_list(&self) -> Result<Vec<Par>, SpaceError> {
        let meter = self.meter();
        let mut channels = meter.buffer::<Par>(1)?;
        channels.push(meter.copy(self.data)?);
        Ok(channels)
    }

    /// The one-pattern list of the receive `op`, with its buffer and its copy reserved.
    fn pattern_list(&self, op: &ConsumeOp) -> Result<Vec<BindPattern>, SpaceError> {
        let meter = self.meter();
        let mut patterns = meter.buffer::<BindPattern>(1)?;
        patterns.push(meter.copy(&op.pattern)?);
        Ok(patterns)
    }

    /// Stages a produce-side fire: `data` is the record of the staged produce, `removed` the
    /// record of the incoming candidate and `source` the identity of the produce.
    fn fire_offer(
        &mut self,
        txn: &mut ParSpaceTxn,
        fire: OfferFire,
        data: ListParWithRandom,
        removed: ListParWithRandom,
        source: Produce,
    ) -> Result<(), SpaceError> {
        let meter = self.meter();
        let channels = self.channel_list()?;
        let mut candidates = meter.buffer::<ParCandidate>(1)?;
        candidates.push(incoming_candidate(
            meter.copy(self.data)?,
            source,
            removed,
            fire.persistent,
            fire.matched,
        ));
        txn.produce(SpaceProduce {
            channel: meter.copy(self.data)?,
            data,
            persist: fire.persistent,
            outcome: ProduceOutcome::Fire {
                channels,
                continuation: fire.waiter,
                data: candidates,
            },
            before_observe: Some(record::register_introduction(self.introduction, meter)?),
        })?;
        self.staged.push(Staged::Fire);
        Ok(())
    }

    /// Stages a consume-side fire of the receive `op` with the parked continuation
    /// `continuation` and the peek set `peeks`. The head record is the plan's own copy, so it
    /// moves into the candidate.
    fn fire_receive(
        &mut self,
        txn: &mut ParSpaceTxn,
        op: &ConsumeOp,
        fire: ReceiveFire,
        continuation: TaggedContinuation,
        peeks: BTreeSet<i32>,
    ) -> Result<(), SpaceError> {
        let meter = self.meter();
        let channels = self.channel_list()?;
        let patterns = self.pattern_list(op)?;
        let mut candidates = meter.buffer::<ParCandidate>(1)?;
        let ReceiveFire {
            head: Datum { a, persist, source },
            matched,
            view_index,
        } = fire;
        candidates.push(ConsumeCandidate {
            channel: meter.copy(self.data)?,
            datum: Datum {
                a: matched,
                persist,
                source,
            },
            removed_datum: a,
            datum_index: view_index,
        });
        txn.consume(SpaceConsume {
            channels,
            patterns,
            continuation,
            persist: op.persistent,
            peeks,
            outcome: ConsumeOutcome::Fire { data: candidates },
            before_observe: Some(record::register_introduction(self.introduction, meter)?),
        })?;
        self.staged.push(Staged::Fire);
        Ok(())
    }
}

/// A candidate of a staged fire.
type ParCandidate = ConsumeCandidate<Par, ListParWithRandom>;

/// A produce-side fire: the head waiter that the datum fires, and the values that it binds.
struct OfferFire {
    /// True for a persistent send.
    persistent: bool,
    waiter: ParWaiter,
    matched: ListParWithRandom,
}

/// A consume-side fire: the head record (the plan's copy), the values that it binds, and its
/// index in the staged view.
struct ReceiveFire {
    head: ParDatum,
    matched: ListParWithRandom,
    view_index: i32,
}

/// The peek set of a receive: `{0}` for a peek, with its tree node reserved, or the empty set.
fn peek_set(peek: bool, meter: &ScanMeter) -> Result<BTreeSet<i32>, SpaceError> {
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

impl<S: OuterStore, D: Discipline> GenericSpace<S, D> {
    /// Fails with `FamilyUnavailable` unless the config names the family of this agent. The agent
    /// runs counter disciplines without joins only. The comparison of the names is reserved
    /// first, and the error takes the names of the config without a copy.
    fn check_family(&self, config: SpaceConfig, meter: &ScanMeter) -> Result<(), SpaceError> {
        let scanned = config
            .outer
            .len()
            .checked_add(config.inner.len())
            .ok_or(RSpaceError::HostWorkRejected)?;
        meter.compare(scanned, S::NAME.len() + D::NAME.len())?;
        match (
            config.is_family(S::NAME, D::NAME),
            D::USES_COUNTER,
            D::JOINS,
        ) {
            (true, true, false) => Ok(()),
            _ => Err(SpaceError::FamilyUnavailable {
                outer: config.outer,
                inner: config.inner,
            }),
        }
    }

    /// Steps 2 and 3 of a collection operation. The join check reads the stored group and the
    /// data channel, so it reserves both reads and one comparison, as the branch's channel
    /// position check does.
    fn check_collection(
        &self,
        txn: &mut ParSpaceTxn,
        channels: &CollectionChannels,
        space: NativeSpace,
        channel: &Par,
        meter: &ScanMeter,
    ) -> Result<(), SpaceError> {
        let (config, _) = read_space_config(
            txn,
            channels.config(),
            space,
            || missing_config(channel, meter),
            meter,
        )?;
        self.check_family(config, meter)?;
        let data = channels.data();
        match txn.joins(data)? {
            [] => Ok(()),
            [group] => {
                meter.inspect(group)?;
                meter.inspect(data)?;
                meter.charge(1, 0, 0)?;
                match group.as_slice() == std::slice::from_ref(data) {
                    true => Ok(()),
                    false => Err(SpaceError::Inconsistent(
                        "a collection channel is part of a join",
                    )),
                }
            }
            _ => Err(SpaceError::Inconsistent(
                "a collection channel is part of a join",
            )),
        }
    }

    /// The head waiting continuation of the collection in the staged view. The scan reserves
    /// its buffer and the decode of each envelope, and only the head is copied.
    fn head_waiter(
        &self,
        txn: &mut ParSpaceTxn,
        data: &Par,
        meter: &ScanMeter,
    ) -> Result<Option<ParWaiter>, SpaceError> {
        let waiters = txn.continuations(std::slice::from_ref(data))?;
        let mut ordered = meter.buffer::<Ordered<'_, ParWaiter>>(waiters.len())?;
        for waiter in waiters {
            ordered.push(Ordered {
                ord: record::continuation_ord_metered(&waiter.continuation, meter)?,
                entry: waiter,
            });
        }
        let head = self
            .discipline
            .head_metered(Side::Waiters, &ordered, meter)?;
        match head.and_then(|index| waiters.get(index)) {
            Some(waiter) => Ok(Some(meter.copy(waiter)?)),
            None => Ok(None),
        }
    }

    /// The head record of the collection in the staged view, with its index in the view. The
    /// scan reserves its buffer and the decode of each envelope, and only the head is copied.
    fn head_datum(
        &self,
        txn: &mut ParSpaceTxn,
        data: &Par,
        meter: &ScanMeter,
    ) -> Result<Option<(usize, ParDatum)>, SpaceError> {
        let records = txn.data(data)?;
        let mut ordered = meter.buffer::<Ordered<'_, ParDatum>>(records.len())?;
        for datum in records {
            ordered.push(Ordered {
                ord: record::datum_ord_metered(&datum.a, meter)?,
                entry: datum,
            });
        }
        let head = self.discipline.head_metered(Side::Data, &ordered, meter)?;
        match head.and_then(|index| records.get(index).map(|datum| (index, datum))) {
            Some((index, datum)) => Ok(Some((index, meter.copy(datum)?))),
            None => Ok(None),
        }
    }

    /// The produce plan: stages the store or the fires of the datum of `op` with the order `ord`,
    /// and returns the re-issue. A fire that ends the loop moves the record and its source into
    /// the staged produce, and a re-issue moves `op`, so only a re-offer copies them.
    fn offer(
        &self,
        txn: &mut ParSpaceTxn,
        plan: &mut Plan<'_>,
        op: ProduceOp,
        ord: i64,
    ) -> Result<Option<SpaceReissue>, SpaceError> {
        let meter = plan.meter();
        let record = record::wrap_datum_metered(ord, &op.data, meter)?;
        let queued = txn.data(plan.data)?.len();
        if !self.discipline.reaches_head(Side::Data, queued) {
            plan.store(txn, record, op.persistent)?;
            return Ok(None);
        }
        let source = Produce::create_metered(plan.data, &record, op.persistent, meter)?;
        let mut rounds = FireRounds::new();
        loop {
            let Some(waiter) = self.head_waiter(txn, plan.data, meter)? else {
                plan.store(txn, record, op.persistent)?;
                return Ok(None);
            };
            let [pattern] = waiter.patterns.as_slice() else {
                return Err(SpaceError::Inconsistent(
                    "a waiting continuation of a collection has several binds",
                ));
            };
            let Some(matched) = plan
                .matcher
                .matches(pattern, &op.data, &waiter.continuation)?
            else {
                plan.store(txn, record, op.persistent)?;
                return Ok(None);
            };
            let spent = !op.persistent && waiter.peeks.is_empty();
            let stays = waiter.persist;
            record::prepay_continuation_unwrap(&waiter.continuation, meter)?;
            let fire = OfferFire {
                persistent: op.persistent,
                waiter,
                matched,
            };
            match rounds.after_fire(spent, stays) {
                FireStep::Continue => {
                    let (data, removed) = (meter.copy(&record)?, meter.copy(&record)?);
                    plan.fire_offer(txn, fire, data, removed, meter.copy(&source)?)?;
                }
                FireStep::Done => {
                    let removed = meter.copy(&record)?;
                    plan.fire_offer(txn, fire, record, removed, source)?;
                    return Ok(None);
                }
                FireStep::Reissue(cause) => {
                    let removed = meter.copy(&record)?;
                    plan.fire_offer(txn, fire, record, removed, source)?;
                    return Ok(Some(SpaceReissue {
                        op: SpaceOp::Produce(op),
                        cause,
                    }));
                }
            }
        }
    }

    /// The consume plan: stages the park or the fires of the continuation of `op` with the
    /// order `ord`, and returns the re-issue. A fire that ends the loop moves the parked
    /// continuation and the peek set into the staged consume, and a re-issue moves `op`, so only
    /// a re-fire copies them.
    fn receive(
        &self,
        txn: &mut ParSpaceTxn,
        plan: &mut Plan<'_>,
        op: ConsumeOp,
        ord: i64,
    ) -> Result<Option<SpaceReissue>, SpaceError> {
        let meter = plan.meter();
        let continuation = record::wrap_continuation_metered(
            ord,
            &op.body,
            &op.guard,
            record::stamped(&op.authority),
            meter,
        )?;
        let peeks = peek_set(op.peek, meter)?;
        let queued = txn.continuations(std::slice::from_ref(plan.data))?.len();
        if !self.discipline.reaches_head(Side::Waiters, queued) {
            plan.park(txn, &op, continuation, peeks)?;
            return Ok(None);
        }
        let mut rounds = FireRounds::new();
        loop {
            let Some((index, head)) = self.head_datum(txn, plan.data, meter)? else {
                plan.park(txn, &op, continuation, peeks)?;
                return Ok(None);
            };
            let payload = record::unwrap_datum_metered(&head.a, meter)?.datum;
            let Some(matched) = plan.matcher.matches(&op.pattern, &payload, &continuation)? else {
                plan.park(txn, &op, continuation, peeks)?;
                return Ok(None);
            };
            let spent = !op.persistent;
            let stays = head.persist || op.peek;
            let view_index = i32::try_from(index)
                .map_err(|_| SpaceError::Inconsistent("a record index exceeds i32"))?;
            record::prepay_continuation_unwrap(&continuation, meter)?;
            let fire = ReceiveFire {
                head,
                matched,
                view_index,
            };
            match rounds.after_fire(spent, stays) {
                FireStep::Continue => {
                    let fired = meter.copy(&continuation)?;
                    plan.fire_receive(txn, &op, fire, fired, meter.copy(&peeks)?)?;
                }
                FireStep::Done => {
                    plan.fire_receive(txn, &op, fire, continuation, peeks)?;
                    return Ok(None);
                }
                FireStep::Reissue(cause) => {
                    plan.fire_receive(txn, &op, fire, continuation, peeks)?;
                    return Ok(Some(SpaceReissue {
                        op: SpaceOp::Consume(op),
                        cause,
                    }));
                }
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

/// The fires that `staged` lists.
fn fire_count(staged: &[Staged]) -> usize {
    staged.iter().filter(|kind| **kind == Staged::Fire).count()
}

/// Reserves the buffer of the fires of the outcome of `staged`. [`outcome`] runs after the
/// commit, so the operation pays for its buffer before the commit, as it prepays the decode of
/// each fired continuation (`record::prepay_continuation_unwrap`). A host-work rejection then
/// never follows a commit.
fn prepay_outcome(staged: &[Staged], meter: &ScanMeter) -> Result<(), SpaceError> {
    Ok(meter.reserve_buffer::<SpaceFire>(fire_count(staged))?)
}

/// Step 7: the outcome of a committed collection operation. It moves each fired continuation out
/// of its receipt and copies nothing.
fn outcome(
    receipt: ParSpaceReceipt,
    staged: Vec<Staged>,
    reissue: Option<SpaceReissue>,
) -> Result<SpaceOutcome, SpaceError> {
    if receipt.ops.len() != staged.len() {
        return Err(SpaceError::Inconsistent(
            "the receipt does not list the staged operations",
        ));
    }
    let mut fires = Vec::with_capacity(fire_count(&staged));
    for (receipt, kind) in receipt.ops.into_iter().zip(staged) {
        match (kind, receipt) {
            (Staged::Counter(CounterOp::Take), SpaceOpReceipt::Fired { .. })
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
            _ => {
                return Err(SpaceError::Inconsistent(
                    "a receipt differs from its staged operation",
                ))
            }
        }
    }
    Ok(SpaceOutcome { fires, reissue })
}

/// The most operations that one collection operation stages: the bump, `FIRE_ROUND_CAP` fires,
/// and one store or park.
const MAX_STAGED: usize = OPEN_BUMP_OPERATIONS + FIRE_ROUND_CAP + 1;

/// The operations of the longest bump (`counter::bump` on a new collection).
const OPEN_BUMP_OPERATIONS: usize = 3;

/// The setup of one collection operation: the transaction over its lock keys, after steps 2 and
/// 3, and its staged list.
struct Setup {
    txn: ParSpaceTxn,
    staged: Vec<Staged>,
}

impl<S: OuterStore, D: Discipline> GenericSpace<S, D> {
    /// Steps 1 to 3 of a collection operation on `channel`, the collection `key` of `space`.
    async fn begin_collection(
        &self,
        env: &SpaceEnv,
        channels: &CollectionChannels,
        space: NativeSpace,
        channel: &Par,
    ) -> Result<Setup, SpaceError> {
        let mut txn = env.store.begin(channels.lock_keys()).await?;
        self.check_collection(&mut txn, channels, space, channel, &env.meter)?;
        let staged = env.meter.buffer::<Staged>(MAX_STAGED)?;
        Ok(Setup { txn, staged })
    }
}

#[async_trait]
impl<S: OuterStore, D: Discipline> SpaceAgent for GenericSpace<S, D> {
    async fn prepare_produce(
        &self,
        env: &SpaceEnv,
        op: ProduceOp,
    ) -> Result<Prepared<SpaceOutcome>, SpaceError> {
        let meter = &env.meter;
        let channels = CollectionChannels::new_metered(&self.outer, op.space, &op.key, meter)?;
        let Setup { mut txn, staged } = self
            .begin_collection(env, &channels, op.space, &op.channel)
            .await?;
        let matcher = SpaceMatcher::metered(env)?;
        let introduction = Introduction::new(&env.budget, &op.authority, meter)?;
        let random = Blake2b512Random::from_bytes(&op.data.random_state);
        let bump = counter::bump(&mut txn, channels.counters(), &BumpContext {
            matcher: &matcher,
            random: &random,
            authority: OperationAuthority::of_produce(&op, &introduction),
        })?;
        let mut plan = Plan {
            data: channels.data(),
            matcher: &matcher,
            introduction: &introduction,
            staged,
        };
        plan.staged
            .extend(bump.staged.iter().copied().map(Staged::Counter));
        let reissue = self.offer(&mut txn, &mut plan, op, bump.ord)?;
        let staged = plan.staged;
        prepay_outcome(&staged, meter)?;
        Ok(Prepared::new(txn, move |receipt| {
            outcome(receipt, staged, reissue)
        }))
    }

    async fn prepare_consume(
        &self,
        env: &SpaceEnv,
        op: ConsumeOp,
    ) -> Result<Prepared<SpaceOutcome>, SpaceError> {
        let meter = &env.meter;
        let channels = CollectionChannels::new_metered(&self.outer, op.space, &op.key, meter)?;
        let Setup { mut txn, staged } = self
            .begin_collection(env, &channels, op.space, &op.channel)
            .await?;
        let matcher = SpaceMatcher::metered(env)?;
        let introduction = Introduction::new(&env.budget, &op.introduction_authority, meter)?;
        let random = Blake2b512Random::from_bytes(&op.body.random_state);
        let bump = counter::bump(&mut txn, channels.counters(), &BumpContext {
            matcher: &matcher,
            random: &random,
            authority: OperationAuthority::of_consume(&op, &introduction),
        })?;
        let mut plan = Plan {
            data: channels.data(),
            matcher: &matcher,
            introduction: &introduction,
            staged,
        };
        plan.staged
            .extend(bump.staged.iter().copied().map(Staged::Counter));
        let reissue = self.receive(&mut txn, &mut plan, op, bump.ord)?;
        let staged = plan.staged;
        prepay_outcome(&staged, meter)?;
        Ok(Prepared::new(txn, move |receipt| {
            outcome(receipt, staged, reissue)
        }))
    }

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
        let mut txn = env.store.begin(&keys).await?;
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
        txn.produce(SpaceProduce {
            channel: config_channel,
            data: record::config_record_metered(
                creation.config,
                &RandomPurpose::Config.derive(&creation.random),
                meter,
            )?,
            persist: true,
            outcome: ProduceOutcome::Store,
            before_observe: Some(record::register_introduction(&introduction, meter)?),
        })?;
        let cost_authority = record::cost_authority_metered(&creation.authority, meter)?;
        txn.produce(SpaceProduce {
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
        })?;
        let mut channels = meter.buffer::<Par>(1)?;
        channels.push(handle);
        let mut patterns = meter.buffer::<BindPattern>(1)?;
        patterns.push(creation.handle_pattern);
        txn.consume(SpaceConsume {
            channels,
            patterns,
            continuation: creation.handle_continuation,
            persist: true,
            peeks: BTreeSet::new(),
            outcome: ConsumeOutcome::Park,
            before_observe: Some(record::register_introduction(&introduction, meter)?),
        })?;
        Ok(Prepared::new(txn, |receipt| match receipt.ops.as_slice() {
            [SpaceOpReceipt::Stored { .. }, SpaceOpReceipt::Stored { .. }, SpaceOpReceipt::Parked { .. }] => {
                Ok(())
            }
            _ => Err(SpaceError::Inconsistent(
                "a space creation committed other operations",
            )),
        }))
    }

    async fn read_config(&self, env: &SpaceEnv, space: NativeSpace) -> Result<Par, SpaceError> {
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
    use models::rust::utils::{
        new_freevar_par, new_freevar_var, new_gbool_par, new_gint_par, new_gstring_par,
    };
    use rspace_plus_plus::rspace::checkpoint::Checkpoint;
    use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
    use rspace_plus_plus::rspace::hashing::stable_hash_provider;
    use rspace_plus_plus::rspace::operation_context::{self, CausalPath, OperationOrder};
    use rspace_plus_plus::rspace::replay_rspace::ReplayRSpace;
    use rspace_plus_plus::rspace::rspace::RSpace;
    use rspace_plus_plus::rspace::rspace_interface::{ContResult, ISpace};
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
    use crate::rust::interpreter::matcher::r#match::Matcher;
    use crate::rust::interpreter::reified::costs::{ReissueCause, ScanMeter, FIRE_ROUND_CAP};
    use crate::rust::interpreter::spaces::agent::run_op;
    use crate::rust::interpreter::spaces::counter::{counter_continuation, counter_pattern};
    use crate::rust::interpreter::spaces::disciplines::queue::QueueDiscipline;
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
            authority: CostAuthority::default(),
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
            authority: CostAuthority::default(),
            introduction_authority: CostAuthority::default(),
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
            .get_data(&channels_of(channel).data())
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
        let config_records = space.get_data(&ch.config()).await;
        assert_eq!(config_records.len(), 1);
        assert!(config_records[0].persist);
        assert_eq!(record::config_map(&config_records[0].a), Ok(&config()));
        assert_eq!(
            config_records[0].a.random_state,
            RandomPurpose::Config
                .derive(&creation(config()).random)
                .to_bytes()
        );
        let counters = space.get_data(&ch.space_counter()).await;
        assert_eq!(counter(&space, &ch.space_counter()).await, vec![0]);
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
        assert_eq!(space.get_data(&ch.config()).await.len(), 1);
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
        assert!(space.get_data(&channels().config()).await.is_empty());
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
        assert_eq!(counter(&space, &ch.space_counter()).await, vec![1]);
        assert_eq!(counter(&space, &ch.counter()).await, vec![1]);
        assert_eq!(records(&space).await, vec![(0, vec![int(1)], false)]);

        produce(&env, send(2)).await;
        assert_eq!(
            labels(&space.take_event_log().await),
            bumped(&["produce D"])
        );
        assert_eq!(counter(&space, &ch.counter()).await, vec![2]);
        assert_eq!(counter(&space, &ch.space_counter()).await, vec![1]);
        assert_eq!(records(&space).await, vec![
            (0, vec![int(1)], false),
            (1, vec![int(2)], false)
        ]);

        let other = string("other");
        produce(&env, send_on(&other, 3, false)).await;
        assert_eq!(counter(&space, &ch.space_counter()).await, vec![2]);
        assert_eq!(counter(&space, &channels_of(&other).counter()).await, vec![
            1
        ]);
        assert_eq!(counter(&space, &ch.counter()).await, vec![2]);
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
    async fn space_queue_peek_keeps_the_head_record() {
        let Fixture { space, env } = fixture().await;
        produce(&env, send(5)).await;
        space.take_event_log().await;
        let peeked = consume(&env, peek("p")).await;
        assert_eq!(fired(&peeked), vec![delivery("p", 5)]);
        assert!(peeked.fires[0].results[0].persistent);
        assert_eq!(peeked.reissue, None);
        assert_eq!(
            labels(&space.take_event_log().await),
            bumped(&["consume D", "comm D peek"])
        );
        assert_eq!(records(&space).await, vec![(0, vec![int(5)], false)]);
        assert!(waiters(&space).await.is_empty());

        assert_eq!(fired(&consume(&env, receive("t", any())).await), vec![
            delivery("t", 5)
        ]);
        assert!(records(&space).await.is_empty());
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
                op: SpaceOp::Produce(send_forever(9)),
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
            again.reissue.map(|reissue| reissue.cause),
            Some(ReissueCause::DivergentPair)
        );
        assert_eq!(counter(&space, &channels().counter()).await, vec![3]);

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
                op: SpaceOp::Consume(receive_forever("s")),
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
        assert_eq!(
            outcome.reissue,
            Some(SpaceReissue {
                op: SpaceOp::Produce(send_forever(1)),
                cause: ReissueCause::RoundCap,
            })
        );
        assert_eq!(space.take_event_log().await.len(), 3 + 2 * FIRE_ROUND_CAP);
        assert!(records(&space).await.is_empty());
        let last = i64::try_from(FIRE_ROUND_CAP).expect("a small cap");
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
        assert_eq!(records(&space).await, vec![(last + 2, vec![int(1)], true)]);

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
            last + 2,
            "r",
            true,
            false
        )]);
    }

    #[tokio::test]
    async fn space_queue_every_operation_bumps_the_counter() {
        let Fixture { space, env } = fixture().await;
        let ch = channels();
        produce(&env, send(1)).await;
        assert_eq!(counter(&space, &ch.counter()).await, vec![1]);
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
            assert_eq!(counter(&space, &ch.counter()).await, vec![expected]);
        }
        assert_eq!(counter(&space, &ch.space_counter()).await, vec![1]);
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
        assert!(space.get_data(&channels().space_counter()).await.is_empty());
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
    async fn space_queue_collection_join_is_inconsistent() {
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
        let joined = SpaceError::Inconsistent("a collection channel is part of a join");
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
        assert_eq!(space.get_data(&ch.config()).await[0].a.cost_authority, None);
        assert_eq!(
            space.get_data(&ch.space_counter()).await[0]
                .a
                .cost_authority,
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
        let records = space.get_data(&ch.data()).await;
        assert_eq!(records[0].a.cost_authority, Some(region(2)));
        assert_eq!(
            space.get_data(&ch.counter()).await[0].a.cost_authority,
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
            space.get_data(&ch.counter()).await[0].a.cost_authority,
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
            outcome(receipt(vec![stored.clone()]), vec![Staged::Store], None),
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
            outcome(receipt(vec![stored.clone()]), vec![Staged::Park], None),
            differs
        );
        assert_eq!(
            outcome(
                receipt(vec![parked]),
                vec![Staged::Counter(CounterOp::Put)],
                None
            ),
            differs
        );
        assert_eq!(
            outcome(receipt(vec![stored]), vec![Staged::Fire], None),
            differs
        );
        assert_eq!(
            outcome(receipt(Vec::new()), vec![Staged::Store], None),
            Err(SpaceError::Inconsistent(
                "the receipt does not list the staged operations"
            ))
        );
        assert_eq!(
            outcome(
                receipt(vec![fired_with(named("bare", None))]),
                vec![Staged::Fire],
                None
            ),
            Err(SpaceError::Inconsistent("a malformed continuation record"))
        );
    }
}
