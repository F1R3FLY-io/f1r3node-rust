//! The join engine of the native spaces (P5c, decision 29, joins plan sections 3 to 5).
//!
//! # What the engine runs
//!
//! The drivers of one collection (`generic::offer_head`, `generic::receive_head`, the scan driver
//! and the settle of a stack or a priority queue) see only the waiters of the group `[D(sid,k)]`
//! of their own collection. The join engine runs every operation that a join can reach:
//!
//! * every receive with two or more binds on one space (`SpaceOp::Join`);
//! * every produce, consume or settle whose collection has a join group other than its own;
//! * every settle with a cascade state (`SettleOp::cascade`).
//!
//! [`classify`] decides the route. An operation on a collection without a join group keeps its
//! driver, and the classification reserves exactly the reads of the join check of P4, so the host
//! work of such an operation does not change.
//!
//! # Scope, then lock, then plan
//!
//! A produce on a joined collection can fire a waiter whose group holds other collections, and a
//! fire can expose heads on those collections. The substrate checks every staged channel against
//! the lock keys, so the engine locks the closure of the collections that the joins connect:
//!
//! ```text
//! PREPARE(op):
//!   opened ← begin over the own collections of op; the config is read once
//!   match CLASSIFY(op):
//!     Single → run_with(opened, op)                       ▷ the driver, unchanged
//!     Join(scope) →
//!       if scope covers the own collections: RUN-JOIN(opened, scope, op)
//!       else: drop opened (nothing is staged); begin over scope.lock_keys; RUN-JOIN
//! ```
//!
//! The scheduler footprint `[2]‖sid` serializes every job of a space, so no transaction on the
//! space commits between the two transactions, and the closure is complete, so one escalation is
//! enough. If that premise failed, a staging on a channel outside the lock keys would fail with
//! `UnlockedChannel`, deterministically.
//!
//! # The transaction
//!
//! ```text
//! RUN-JOIN(op, scope):
//!   TOUCH each own collection                      ▷ decision 31: bump its counter once
//!   ords ← the order of the operation on each own collection   ▷ decision 43 for a re-issue
//!   if op is a settle job:                         ▷ it has no plan (joins plan 4.10, step 3)
//!     work ← its pending second sends; every collection is dirty
//!     DRAIN-AND-CASCADE; COMMIT
//!   if the discipline cascades: DRAIN-AND-CASCADE over the scope       ▷ the settle before
//!   if stopped, or the transaction is at the round cap: COMMIT   ▷ the operation re-issues
//!   match op:
//!     Produce → OFFER(its datum)
//!     Consume, Join → RECEIVE(its waiter)
//!   DRAIN-AND-CASCADE
//!   COMMIT
//! ```
//!
//! # Windows and searches (joins plan section 3)
//!
//! A waiter `W` parks on its group `G` (the channels of its binds in canonical order, FR-1').
//! `m_W(c)` is the number of positions of `W` on the collection `c`. A bind on `c` binds an entry
//! of the window of `c` ([`JoinWindow`]): the first `m` entries of the head order (queue, stack,
//! cell), the first `m` records of each level that its lane sees (priority queue), or every record
//! in the canonical order of DR-75 (bag, set). An element in flight takes its place in the window
//! of its collection. The binds of one fire bind distinct records, and a set bind claims its whole
//! class (decision 28).
//!
//! ```text
//! GREEDY(W):                                       ▷ decision 37: no backtracking
//!   for each position i in group order:
//!     σ[i] ← the first unclaimed entry of the window of coll(i) that the lane of i sees and
//!            that the pattern of i binds; none → no fire
//!   the guard of W once, over the bound values in bind order
//! SEARCH-PRODUCE(W, x):                            ▷ decision 28: x binds exactly once
//!   W must be at the head of each head-only collection of its group (decision 11)
//!   for each position p on the collection of x, in group order, where x is in the window:
//!     if the pattern of p binds x: σ[p] ← x; GREEDY over the other positions; done when it fills
//! ```
//!
//! # Rule E and the cascade
//!
//! A peek fire removes the non-persistent records that it binds, as the host COMM does, and the
//! same transaction sends each of them again through OFFER, in receipt order (decision 45, Rule
//! E). The element in flight stays in flight after a peek fire and is offered again, whether or
//! not it is persistent (decision 30, as `generic::offer_head` continues with it). A fire on a
//! head-only collection can expose a pair of heads that matches. On a set, a fire can move a
//! class: a class sits at the place of its first member, and when that member leaves while a
//! persistent member stays, the class takes the place of the persistent member (finding F-JF-1).
//! The cascade fires every pair that a fire exposes, with lift and re-offer (`spaces::settle`),
//! oldest first, for the head-only disciplines and for the set (decision 50). A bag needs no
//! cascade, because a removal exposes nothing on a bag. Every fire counts on the one fire count of
//! the transaction (decision 49). At the cap the transaction stops and re-issues one `SettleOp`
//! that carries the pending second sends, the dirty collections and the re-issue of the operation
//! (`CascadeState`).
//!
//! ```text
//! DRAIN-AND-CASCADE:
//!   loop
//!     if stopped: return
//!     if work is not empty:
//!       if the transaction is at the cap: stop; return      ▷ carry, do not search
//!       OFFER(pop(work)); continue
//!     if the discipline is the bag: return                  ▷ a removal exposes nothing
//!     F ← the oldest fireable head of the least dirty collection (every waiter of a set,
//!         in canonical order); none → return
//!     if the transaction is at the cap: stop; return
//!     LIFT-AND-REOFFER(F)
//! ```
//!
//! # The seam of the PathMap store (W4)
//!
//! The engine reaches the discipline and the windows of a store through the trait
//! [`JoinSource`]. P5c implements it for the keyed stores (HashMap, Array, Vector) with
//! [`KeyedSource`]. The PathMap store adds its own source in W4.
//!
//! # Metering (decision 42)
//!
//! Every read and copy of the engine is reserved on the meter of the job before it is made: the
//! closure reads, the names of the lock keys, the envelope decodes of the windows and heads, each
//! payload decode and match, each class key, each comparison, each buffer and each copy of a
//! staged operation. The outcome after the commit only moves values out of its receipt, and the
//! engine prepays that work at staging.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::mem::size_of;
use std::sync::{Arc, LazyLock};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{
    BindPattern, CostAuthority, ListParWithRandom, Par, ParWithRandom, TaggedContinuation,
};
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::internal::{ConsumeCandidate, Datum};
use rspace_plus_plus::rspace::space_store_handle::{incoming_candidate, metered_hash};
use rspace_plus_plus::rspace::trace::event::Produce;

use super::agent::{
    CascadeState, ConsumeOp, JoinOp, PendingProduce, Prepared, ProduceOp, SettleOp, SpaceEnv,
    SpaceOp, SpaceOutcome, SpaceReissue,
};
use super::counter::CounterChannels;
use super::disciplines::priority_queue::{receive_lane, send_level, Lane, Level};
use super::disciplines::scan::{retire_class_on, retire_continuation, same_class};
use super::disciplines::vectordb::{self, Flight, ListFire, Mode, VdbConfig};
use super::errors::SpaceError;
use super::generic::{
    self, operation_ord, peek_set, peek_set_all, receive_candidate, CollectionTxn, Cx, Discipline,
    GenericSpace, JoinWindow, Ordered, OuterStore, ParCandidate, ParDatum, ParWaiter, ReceiveFire,
    Side, SideBound, GPRIVATE,
};
use super::namespace::{self, CollectionKey, NativeSpace, RandomPurpose, ReifiedId};
use super::record::{self, Introduction, JoinSlot, OperationAuthority, WaiterPolicy};
use super::settle::{lift_body, lift_continuation, lift_pattern, settle_operation};
use super::types::config::SpaceConfig;
use crate::rust::interpreter::reified::costs::{FireStep, ReissueCause, ScanMeter, FIRE_ROUND_CAP};
use crate::rust::interpreter::tensor::exact::Score;
use crate::rust::interpreter::tensor::num::ExactVector;

// ── Routes and scopes ───────────────────────────────────────────────────────────────────────

/// The route of a collection operation (joins plan section 4.1).
#[derive(Clone, Debug, PartialEq)]
pub enum JoinRoute {
    /// The driver of the collection runs the operation (`FamilySpace::run_with`).
    Single,
    /// The join engine runs the operation over the closure of the scope.
    Join(JoinScope),
}

/// One collection of a join closure: its key, its data channel `D(sid,k)` and its counter
/// channel `S(sid,k)`.
#[derive(Clone, Debug, PartialEq)]
pub struct ScopedCollection {
    /// The key of the collection.
    pub key: CollectionKey,
    /// The data channel `D(sid,k)`: the records, the waiters and the join groups.
    pub data: Par,
    /// The counter channel `S(sid,k)`.
    pub counter: Par,
}

/// The closure of a join operation (joins plan section 4.1): the collections that the join
/// groups connect to the own collections of the operation, and the lock keys of the transaction
/// that runs it.
#[derive(Clone, Debug, PartialEq)]
pub struct JoinScope {
    /// The space of the collections.
    pub space: NativeSpace,
    /// Every collection of the closure, the own collections included, in ascending key order.
    pub collections: Vec<ScopedCollection>,
    /// The own collections of the operation: the collections of its binds in canonical group
    /// order, each once, or the collection of a produce, a consume or a settle.
    pub own: Vec<CollectionKey>,
    /// The positions of the binds in canonical group order: `group[j]` is the bind at group
    /// position `j` (FR-1'). A produce, a consume and a settle have the one position 0.
    group: Vec<usize>,
    /// The space counter `S(sid)`.
    space_counter: Par,
    /// `[C(sid), S(sid)]`, then `[S(sid,k), D(sid,k)]` for each collection in ascending key
    /// order.
    lock_keys: Vec<Par>,
    /// True for the scope of a PathMap operation: its collections are the prefixes `q`, keyed
    /// `[0x49] ‖ prefix_key(q)`, whose index channels `I(q)` hold the waiters and the join
    /// groups.
    prefixes: bool,
}

impl JoinScope {
    /// The scope of an operation of a store with a source of its own (joins plan section 9): the
    /// collections of the closure in ascending key order, the own collections, the bind positions
    /// in canonical group order, the space counter and the lock keys. The caller reserves the
    /// buffers and the copies, as `join_scope` reserves them.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        space: NativeSpace,
        collections: Vec<ScopedCollection>,
        own: Vec<CollectionKey>,
        group: Vec<usize>,
        space_counter: Par,
        lock_keys: Vec<Par>,
        prefixes: bool,
    ) -> JoinScope {
        JoinScope {
            space,
            collections,
            own,
            group,
            space_counter,
            lock_keys,
            prefixes,
        }
    }

    /// The lock keys of the transaction that runs the operation.
    pub fn lock_keys(&self) -> &[Par] { &self.lock_keys }

    /// True when every collection of the scope is one of `locked`: a transaction that locked
    /// `locked` covers the closure, so the engine runs in it with no escalation. Each comparison
    /// of two keys is reserved first.
    pub fn covers(&self, locked: &[CollectionKey], meter: &ScanMeter) -> Result<bool, SpaceError> {
        if self.collections.len() > locked.len() {
            return Ok(false);
        }
        for collection in &self.collections {
            let mut found = false;
            for key in locked {
                meter.compare(collection.key.as_bytes().len(), key.as_bytes().len())?;
                if collection.key == *key {
                    found = true;
                    break;
                }
            }
            if !found {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// The index of the collection `key` in the scope. The lookup is reserved first.
    fn index_of_key(&self, key: &CollectionKey, meter: &ScanMeter) -> Result<usize, SpaceError> {
        meter.ordered_lookup(self.collections.len(), key.as_bytes().len())?;
        self.collections
            .binary_search_by(|collection| collection.key.cmp(key))
            .map_err(|_| SpaceError::Inconsistent("a collection is outside its join scope"))
    }

    /// The index of the collection whose data channel is `channel`: a data channel of the space,
    /// or in the scope of a PathMap operation an index channel `I(q)` of the space, whose key is
    /// `[0x49] ‖ prefix_key(q)`. The read of the channel and the lookup are reserved first.
    /// Another channel is `Inconsistent`.
    fn index_of_channel(&self, channel: &Par, meter: &ScanMeter) -> Result<usize, SpaceError> {
        meter.inspect(channel)?;
        match namespace::reified_id_of(channel) {
            Some(ReifiedId::PathIndex(owner, tail)) if self.prefixes && owner == self.space => {
                let length = tail
                    .len()
                    .checked_add(1)
                    .ok_or(RSpaceError::HostWorkRejected)?;
                meter.ordered_lookup(self.collections.len(), length)?;
                self.collections
                    .binary_search_by(|collection| match collection.key.as_bytes().split_first() {
                        Some((&PREFIX_KEY_TAG, rest)) => rest.cmp(tail),
                        Some((&first, _)) => first.cmp(&PREFIX_KEY_TAG),
                        None => std::cmp::Ordering::Less,
                    })
                    .map_err(|_| {
                        SpaceError::Inconsistent(
                            "a join group holds a collection outside its scope",
                        )
                    })
            }
            Some(ReifiedId::Data(owner, tail)) if !self.prefixes && owner == self.space => {
                meter.ordered_lookup(self.collections.len(), tail.len())?;
                self.collections
                    .binary_search_by(|collection| collection.key.as_bytes().cmp(tail))
                    .map_err(|_| {
                        SpaceError::Inconsistent(
                            "a join group holds a collection outside its scope",
                        )
                    })
            }
            _ => Err(SpaceError::Inconsistent(
                "a collection channel is part of a foreign join",
            )),
        }
    }
}

/// The first byte of the scope key of a PathMap prefix: `[0x49] ‖ prefix_key(q)`, so the key of
/// the prefix `ε` is not empty (joins plan section 9).
pub(crate) const PREFIX_KEY_TAG: u8 = 0x49;

/// The channels of the collection `key` of `space` in the outer store `outer`: the counter and
/// the data channel, with their names reserved before they are made (decision 42).
pub(crate) fn scoped_collection<S: OuterStore>(
    outer: &S,
    space: NativeSpace,
    key: CollectionKey,
    meter: &ScanMeter,
) -> Result<ScopedCollection, SpaceError> {
    let keyed = namespace::SPACE_ID_PREFIX_LEN
        .checked_add(key.as_bytes().len())
        .ok_or(RSpaceError::HostWorkRejected)?;
    for _ in 0..2 {
        meter.reserve_copy(&*GPRIVATE)?;
        meter.reserve_buffer::<u8>(keyed)?;
    }
    Ok(ScopedCollection {
        data: outer.data_channel(space, &key),
        counter: outer.counter_channel(space, &key),
        key,
    })
}

/// The distinct collections of the binds of the join `op`, in ascending key order. Each copy of
/// a key, each lookup and each growth of the set is reserved first.
pub(crate) fn distinct_bind_keys(
    op: &JoinOp,
    meter: &ScanMeter,
) -> Result<Vec<CollectionKey>, SpaceError> {
    let mut keys: BTreeSet<CollectionKey> = BTreeSet::new();
    for bind in &op.binds {
        meter.ordered_lookup(keys.len(), bind.key.as_bytes().len())?;
        if !keys.contains(&bind.key) {
            meter.tree_growth::<CollectionKey, ()>(keys.len(), 1)?;
            meter.reserve_copy_slice(bind.key.as_bytes())?;
            keys.insert(bind.key.clone());
        }
    }
    let mut ordered = meter.buffer::<CollectionKey>(keys.len())?;
    ordered.extend(keys);
    Ok(ordered)
}

/// The key `key` copied, with the copy of its bytes reserved.
fn copy_key(key: &CollectionKey, meter: &ScanMeter) -> Result<CollectionKey, SpaceError> {
    meter.reserve_copy_slice(key.as_bytes())?;
    Ok(key.clone())
}

/// CLASSIFY (joins plan section 4.1): `Single` for a produce, a consume or a settle without a
/// cascade whose data channel holds no join group other than `[D(sid,k)]`, and otherwise `Join`
/// with the closure. A collection with no group reserves nothing, and a collection with its own
/// group only reserves the read of the group, the read of the channel and one comparison, as the
/// join check of P4 did. A collection with more groups reserves the read of the channel and one
/// comparison, and the closure then reads every group, so the reservation does not depend on the
/// list order of the groups in the store.
pub(crate) fn classify<S: OuterStore, D: Discipline>(
    agent: &GenericSpace<S, D>,
    env: &SpaceEnv,
    opened: &mut CollectionTxn,
    op: &SpaceOp,
) -> Result<JoinRoute, SpaceError> {
    let meter = &env.meter;
    let always = match op {
        SpaceOp::Join(_) => true,
        SpaceOp::Settle(settle) => settle.cascade.is_some(),
        SpaceOp::Produce(_) | SpaceOp::Consume(_) => false,
    };
    if !always {
        let data = opened.channels.data();
        let single = match opened.txn.joins(data)? {
            [] => true,
            [group] => {
                meter.inspect(group)?;
                meter.inspect(data)?;
                meter.charge(1, 0, 0)?;
                group.as_slice() == std::slice::from_ref(data)
            }
            [_, _, ..] => {
                meter.inspect(data)?;
                meter.charge(1, 0, 0)?;
                false
            }
        };
        if single {
            return Ok(JoinRoute::Single);
        }
    }
    join_scope(agent, opened, op, meter).map(JoinRoute::Join)
}

/// The checks of an operation before any staging: the routed key of each bind with the outer
/// store (`OuterStore::check_key`), and the params and the query with the schema of the
/// discipline. A join needs two or more binds, one arrow, and one carried order for each bind
/// when it carries orders.
fn validate<S: OuterStore, D: Discipline>(
    agent: &GenericSpace<S, D>,
    config: &SpaceConfig,
    op: &SpaceOp,
    meter: &ScanMeter,
) -> Result<(), SpaceError> {
    let no_params = BTreeMap::new();
    match op {
        SpaceOp::Produce(op) => {
            agent
                .outer()
                .check_key(config, &op.channel, &op.key, meter)?;
            agent.check_params(config, &op.params, None, meter)
        }
        SpaceOp::Consume(op) => {
            agent
                .outer()
                .check_key(config, &op.channel, &op.key, meter)?;
            agent.check_params(config, &no_params, op.query.as_ref(), meter)
        }
        SpaceOp::Settle(op) => {
            agent
                .outer()
                .check_key(config, &op.channel, &op.key, meter)?;
            agent.check_params(config, &no_params, None, meter)
        }
        SpaceOp::Join(op) => {
            if op.binds.len() < 2 {
                return Err(SpaceError::Inconsistent("a join has fewer than two binds"));
            }
            if op.persistent && op.peek {
                return Err(SpaceError::Inconsistent("a join is persistent and peeks"));
            }
            if let Some(ords) = &op.reissued_ords {
                if ords.len() != op.binds.len() {
                    return Err(SpaceError::Inconsistent(
                        "a re-issued join has not one order for each bind",
                    ));
                }
            }
            for bind in &op.binds {
                agent
                    .outer()
                    .check_key(config, &bind.channel, &bind.key, meter)?;
                agent.check_params(config, &no_params, bind.query.as_ref(), meter)?;
                // A list is one value of one bind (decision 66): a VectorDB bind of a join is in
                // single mode.
                if let (JoinWindow::Similarity, Some(query)) = (D::JOIN_WINDOW, &bind.query) {
                    if vectordb::is_list_query(query, meter)? {
                        return Err(SpaceError::ListModeInJoin);
                    }
                }
            }
            Ok(())
        }
    }
}

/// The positions of the binds of `op` in canonical group order (FR-1'): ascending by the stable
/// hash of the data channel of the collection of each bind, then by bind index. Each hash and
/// each comparison is reserved first. A produce, a consume and a settle have one position.
fn group_order<S: OuterStore>(
    outer: &S,
    op: &SpaceOp,
    meter: &ScanMeter,
) -> Result<Vec<usize>, SpaceError> {
    let SpaceOp::Join(join) = op else {
        let mut group = meter.buffer::<usize>(1)?;
        group.push(0);
        return Ok(group);
    };
    let mut hashed = meter.buffer::<(Blake2b256Hash, usize)>(join.binds.len())?;
    for (index, bind) in join.binds.iter().enumerate() {
        let keyed = namespace::SPACE_ID_PREFIX_LEN
            .checked_add(bind.key.as_bytes().len())
            .ok_or(RSpaceError::HostWorkRejected)?;
        meter.reserve_copy(&*GPRIVATE)?;
        meter.reserve_buffer::<u8>(keyed)?;
        let data = outer.data_channel(join.space, &bind.key);
        hashed.push((metered_hash(&data, meter)?, index));
    }
    sort_reserved(&mut hashed, |(hash, _)| hash.0.len(), meter)?;
    let mut group = meter.buffer::<usize>(hashed.len())?;
    group.extend(hashed.into_iter().map(|(_, index)| index));
    Ok(group)
}

/// Sorts `values` ascending with a reserved comparison for each comparison that the sort makes:
/// an insertion sort, whose comparisons and moves depend on the values only. `bytes` gives the
/// bytes that one comparison of a value reads.
fn sort_reserved<T: Ord>(
    values: &mut [T],
    bytes: impl Fn(&T) -> usize,
    meter: &ScanMeter,
) -> Result<(), SpaceError> {
    for next in 1..values.len() {
        let mut at = next;
        while at > 0 {
            meter.compare(bytes(&values[at - 1]), bytes(&values[at]))?;
            match values[at - 1] > values[at] {
                true => {
                    values.swap(at - 1, at);
                    at -= 1;
                }
                false => break,
            }
        }
    }
    Ok(())
}

/// JOIN-SCOPE (joins plan section 4.1): the checks of the operation, then the closure of the own
/// collections and the seeds under the join groups of their data channels, and the lock keys of
/// the closure. Every read of a group and of a channel, every key copy, every lookup and growth
/// of the sets, and every name is reserved first. A group channel that is not a data channel of
/// the space is `Inconsistent`.
fn join_scope<S: OuterStore, D: Discipline>(
    agent: &GenericSpace<S, D>,
    opened: &mut CollectionTxn,
    op: &SpaceOp,
    meter: &ScanMeter,
) -> Result<JoinScope, SpaceError> {
    validate(agent, &opened.config, op, meter)?;
    let space = op.space();
    let group = group_order(agent.outer(), op, meter)?;
    let mut own = meter.buffer::<CollectionKey>(group.len())?;
    let mut seeds: Vec<CollectionKey> = Vec::new();
    match op {
        SpaceOp::Produce(produce) => own.push(copy_key(&produce.key, meter)?),
        SpaceOp::Consume(consume) => own.push(copy_key(&consume.key, meter)?),
        SpaceOp::Settle(settle) => {
            own.push(copy_key(&settle.key, meter)?);
            if let Some(cascade) = &settle.cascade {
                let extra = cascade
                    .seeds
                    .len()
                    .checked_add(cascade.pending.len())
                    .ok_or(RSpaceError::HostWorkRejected)?;
                seeds = meter.buffer::<CollectionKey>(extra)?;
                for key in &cascade.seeds {
                    seeds.push(copy_key(key, meter)?);
                }
                for pending in &cascade.pending {
                    seeds.push(copy_key(&pending.key, meter)?);
                }
            }
        }
        SpaceOp::Join(join) => {
            for &position in &group {
                let bind = join
                    .binds
                    .get(position)
                    .ok_or(SpaceError::Inconsistent("a group position names no bind"))?;
                let mut seen = false;
                for key in own.iter() {
                    meter.compare(key.as_bytes().len(), bind.key.as_bytes().len())?;
                    if *key == bind.key {
                        seen = true;
                        break;
                    }
                }
                if !seen {
                    own.push(copy_key(&bind.key, meter)?);
                }
            }
        }
    }
    let mut todo: BTreeSet<CollectionKey> = BTreeSet::new();
    for key in own.iter().chain(seeds.iter()) {
        meter.ordered_lookup(todo.len(), key.as_bytes().len())?;
        if !todo.contains(key) {
            meter.tree_growth::<CollectionKey, ()>(todo.len(), 1)?;
            todo.insert(copy_key(key, meter)?);
        }
    }
    let mut closure: BTreeMap<CollectionKey, ScopedCollection> = BTreeMap::new();
    while let Some(key) = todo.pop_first() {
        meter.ordered_lookup(closure.len(), key.as_bytes().len())?;
        if closure.contains_key(&key) {
            continue;
        }
        let collection = scoped_collection(agent.outer(), space, copy_key(&key, meter)?, meter)?;
        let mut reached: Vec<CollectionKey> = Vec::new();
        for group in opened.txn.joins(&collection.data)? {
            meter.inspect(group)?;
            for channel in group {
                meter.inspect(channel)?;
                match namespace::reified_id_of(channel) {
                    Some(ReifiedId::Data(owner, tail)) if owner == space => {
                        meter.reserve_copy_slice(tail)?;
                        let reached_key = CollectionKey::from_bytes(tail.to_vec())
                            .ok_or(SpaceError::Inconsistent("a collection key is empty"))?;
                        generic::push_reserved(&mut reached, reached_key, meter)?;
                    }
                    _ => {
                        return Err(SpaceError::Inconsistent(
                            "a collection channel is part of a foreign join",
                        ))
                    }
                }
            }
        }
        meter.tree_growth::<CollectionKey, ScopedCollection>(closure.len(), 1)?;
        closure.insert(key, collection);
        for reached_key in reached {
            meter.ordered_lookup(closure.len(), reached_key.as_bytes().len())?;
            if closure.contains_key(&reached_key) {
                continue;
            }
            meter.ordered_lookup(todo.len(), reached_key.as_bytes().len())?;
            if !todo.contains(&reached_key) {
                meter.tree_growth::<CollectionKey, ()>(todo.len(), 1)?;
                todo.insert(reached_key);
            }
        }
    }
    let lock_length = closure
        .len()
        .checked_mul(2)
        .and_then(|length| length.checked_add(2))
        .ok_or(RSpaceError::HostWorkRejected)?;
    let mut lock_keys = meter.buffer::<Par>(lock_length)?;
    lock_keys.push(meter.copy(opened.channels.config())?);
    lock_keys.push(meter.copy(opened.channels.space_counter())?);
    let mut collections = meter.buffer::<ScopedCollection>(closure.len())?;
    for (_, collection) in closure {
        lock_keys.push(meter.copy(&collection.counter)?);
        lock_keys.push(meter.copy(&collection.data)?);
        collections.push(collection);
    }
    let space_counter = meter.copy(opened.channels.space_counter())?;
    Ok(JoinScope::new(
        space,
        collections,
        own,
        group,
        space_counter,
        lock_keys,
        false,
    ))
}

/// The introduction authority of the operation `op`.
fn introduction_authority(op: &SpaceOp) -> &CostAuthority {
    match op {
        SpaceOp::Produce(op) => &op.authority,
        SpaceOp::Consume(op) => &op.introduction_authority,
        SpaceOp::Settle(op) => &op.introduction_authority,
        SpaceOp::Join(op) => &op.introduction_authority,
    }
}

/// RUN-JOIN (joins plan section 4.2): the transaction of the join engine for `op` over the
/// closure `scope`, in the open transaction `opened`, whose lock keys cover the closure.
pub(crate) fn run_join<S: OuterStore, D: Discipline>(
    agent: &GenericSpace<S, D>,
    env: &SpaceEnv,
    opened: CollectionTxn,
    scope: JoinScope,
    op: SpaceOp,
) -> Result<Prepared<SpaceOutcome>, SpaceError> {
    let source: KeyedSource<'_, S, D> = KeyedSource {
        outer: std::marker::PhantomData,
        discipline: agent.discipline(),
    };
    run_join_with(env, opened, scope, op, &source)
}

/// RUN-JOIN with the store seam `source` (joins plan section 9): the transaction of the join
/// engine for `op` over the closure `scope`, in the open transaction `opened`, whose lock keys
/// cover the closure. The keyed stores run it with a `KeyedSource`.
pub(crate) fn run_join_with<Src: JoinSource>(
    env: &SpaceEnv,
    opened: CollectionTxn,
    scope: JoinScope,
    op: SpaceOp,
    source: &Src,
) -> Result<Prepared<SpaceOutcome>, SpaceError> {
    let meter = &env.meter;
    let CollectionTxn { txn, config, .. } = opened;
    let authority = meter.copy(introduction_authority(&op))?;
    let first = scope.own.first().ok_or(SpaceError::Inconsistent(
        "a join scope has no own collection",
    ))?;
    let own_index = scope.index_of_key(first, meter)?;
    let data = &scope
        .collections
        .get(own_index)
        .ok_or(SpaceError::Inconsistent(
            "a collection is outside its join scope",
        ))?
        .data;
    generic::run_plan(env, txn, data, &config, &authority, |cx| {
        let engine = Engine::new(source, &scope, &op, cx.config(), cx.meter())?;
        engine.run(cx, op)
    })
}

// ── The seam of a store ─────────────────────────────────────────────────────────────────────

/// Where an entry of a window is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EntryAt {
    /// The record at this index of the staged view of the data channel of its collection.
    Stored(usize),
    /// The element in flight.
    Incoming,
}

/// An entry of a window (joins plan section 3.2): where it is, and its level in a priority
/// queue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    /// Where the entry is.
    pub at: EntryAt,
    /// The level of the record in a priority queue, and `None` in every other discipline.
    pub level: Option<Level>,
}

/// How an element in flight enters the head order of its side (joins plan section 3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Placement {
    /// A fresh element: the head of its side when `Discipline::reaches_head` says so, else the
    /// tail.
    Fresh,
    /// An element with a carried order: a re-issued persistent element (decision 43) or a second
    /// send. It takes its place by `Discipline::precedes`.
    Carried,
}

/// An element in flight: the datum of a produce, a second send of Rule E, or a record that the
/// cascade lifted or re-offers.
pub(crate) struct Incoming {
    /// The index of its collection in the scope.
    pub coll: usize,
    /// The record: the envelope, then the payload.
    pub record: ListParWithRandom,
    /// The payload without the envelope, for matching.
    pub payload: ListParWithRandom,
    /// The identity of its produce.
    pub source: Produce,
    /// True for a persistent element.
    pub persist: bool,
    /// The order of its envelope.
    pub ord: i64,
    /// Its level in a priority queue.
    pub level: Option<Level>,
    /// How it enters the head order of the records.
    pub placement: Placement,
    /// The introduction that its staged operations register.
    pub introduction: Arc<Introduction>,
    /// True for the element of the operation.
    pub own: bool,
    /// True when the element is stored nowhere: a fire that does not spend it leaves it in
    /// flight. A persistent record that the cascade re-offers stays stored, so it is not in
    /// flight.
    pub in_flight: bool,
    /// The depth of the marker that a peek COMM removed, for a second send of a PathMap datum
    /// (joins plan section 9): a store of the element restores that marker instead. A keyed
    /// store always carries `None`.
    pub restore: Option<u8>,
}

/// What a bound entry claims, so that no other bind of the fire binds it (decision 28): one
/// record, or one class of a set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Claim {
    /// The record at `at` of the collection `coll`.
    Record { coll: usize, at: EntryAt },
    /// The class `key` of the collection `coll`, with the metered hash of the key (DR-90).
    Class {
        coll: usize,
        key: Vec<u8>,
        hash: Blake2b256Hash,
    },
}

/// A stored waiter of a collection: its group and its index in the waiters of the group, with
/// the source hash of its consume, which tells it apart.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct WaiterRef {
    /// The channels of its group, in group order.
    pub group: Vec<Par>,
    /// Its index in the waiters of the group in the staged view.
    pub index: usize,
    /// The source hash of its consume.
    pub source: Blake2b256Hash,
}

/// A stored entry of a window as a bind sees it (joins plan section 9).
pub(crate) struct StoredEntry {
    /// The payload of the entry, for matching.
    pub(crate) payload: ListParWithRandom,
    /// True for a persistent entry.
    pub(crate) persist: bool,
    /// What the entry claims (decision 28).
    pub(crate) claim: Claim,
}

/// The fire sites of the engine that spend stored entries (joins plan section 9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FireSite {
    /// A produce-side fire of the element in flight (OFFER).
    Offer,
    /// A consume-side fire of a join waiter of the operation (RECEIVE).
    Join,
    /// A consume-side fire of a single-bind waiter of the operation (RECEIVE, F3 and F20).
    Single,
    /// A fire of a stored pair by the cascade (LIFT-AND-REOFFER).
    Cascade,
}

/// Marks every collection of `colls` dirty in `dirty`.
pub(crate) fn mark(dirty: &mut [bool], colls: &[usize]) {
    for &coll in colls {
        if let Some(entry) = dirty.get_mut(coll) {
            *entry = true;
        }
    }
}

/// The access of the join engine to a store and its discipline (joins plan section 9). The
/// engine core (the searches, the fires, Rule E, the cascade, the stop and the re-issue) is the
/// same for every store. A source gives the windows, the heads, the waiters and the counters of
/// its collections, and the entries, the elements and the second sends of its records.
pub(crate) trait JoinSource {
    /// The window shape of the discipline.
    fn shape(&self) -> JoinWindow;

    // Replaced by P6 (design v3 4.0): the bound of a side may depend on the config of the
    // space, so the source gives the bound of each side.
    // /// The most entries of each side of a collection, or `None` (F15).
    // fn capacity(&self) -> Option<usize>;

    /// The bound of `side` of a collection of a space with the config `config`, or `None` (F15,
    /// P6). The reads of the bound are reserved on `meter`.
    fn bound(
        &self,
        config: &SpaceConfig,
        side: Side,
        meter: &ScanMeter,
    ) -> Result<Option<SideBound>, SpaceError>;

    /// Bumps the counter of the collection `coll` (decision 31) with the random state `random`
    /// and the authorities `authority`, and returns the value before the bump, or `None` for a
    /// discipline without a counter.
    fn touch(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        coll: usize,
        random: &Blake2b512Random,
        authority: OperationAuthority<'_>,
    ) -> Result<Option<i64>, SpaceError>;

    /// The record of the datum `data` with the order `ord` and the level `level`.
    fn wrap_record(
        &self,
        ord: i64,
        level: Option<Level>,
        data: &ListParWithRandom,
        meter: &ScanMeter,
    ) -> Result<ListParWithRandom, SpaceError>;

    /// The order and the level of the record `record`.
    fn record_slots(
        &self,
        record: &ListParWithRandom,
        meter: &ScanMeter,
    ) -> Result<(i64, Option<Level>), SpaceError>;

    /// The payload of the record `record`, for matching.
    fn payload_of(
        &self,
        record: &ListParWithRandom,
        meter: &ScanMeter,
    ) -> Result<ListParWithRandom, SpaceError>;

    /// The slots of a stored waiter whose group has `width` channels: one slot for each group
    /// position.
    fn waiter_slots(
        &self,
        waiter: &ParWaiter,
        width: usize,
        meter: &ScanMeter,
    ) -> Result<Vec<JoinSlot>, SpaceError>;

    /// The window of `need` binds on the collection `coll` (joins plan section 3.2), with the
    /// element in flight `incoming` at its place when it belongs to `coll`. `pending` are the
    /// second sends that the transaction still owes: a source whose records stay stored while
    /// they are in flight leaves them out of every window (the PathMap source).
    fn window(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        coll: usize,
        need: usize,
        incoming: Option<&Incoming>,
        pending: &[Work],
    ) -> Result<Vec<Entry>, SpaceError>;

    /// The claim of the entry `at` of `coll` with the payload `payload`.
    fn claim(
        &self,
        coll: usize,
        at: EntryAt,
        payload: &ListParWithRandom,
        meter: &ScanMeter,
    ) -> Result<Claim, SpaceError>;

    /// The waiters that the element `incoming` meets, in the order in which it meets them: the
    /// head waiter (head-only), the lane heads older first (priority queue), or every waiter in
    /// the canonical order of DR-75 (bag, set).
    fn offer_order(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        incoming: &Incoming,
    ) -> Result<Vec<WaiterRef>, SpaceError>;

    /// The heads of the collection `coll` that a cascade tests, oldest first. A discipline that
    /// scans has none.
    fn cascade_order(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        coll: usize,
    ) -> Result<Vec<WaiterRef>, SpaceError>;

    /// AT-HEAD (joins plan section 3.3): true when the stored waiter `waiter`, with the slots
    /// `slots`, is the head of the waiters of `coll` in each of its lanes there.
    fn at_head(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        waiter: &WaiterRef,
        slots: &[JoinSlot],
        colls: &[usize],
        coll: usize,
    ) -> Result<bool, SpaceError>;

    /// AT-HEAD-INCOMING (joins plan section 3.3): true when a waiter that is not stored, with the
    /// slots `slots` over the collections `colls`, reaches the head of the waiters of `coll` in
    /// each of its lanes there. `carried` is the first-round order of a re-issued persistent
    /// waiter (decision 43).
    fn at_head_incoming(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        slots: &[JoinSlot],
        colls: &[usize],
        coll: usize,
        carried: Option<i64>,
    ) -> Result<bool, SpaceError>;

    /// The number of waiters of the collection `coll`, each counted once.
    fn waiter_count(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        coll: usize,
    ) -> Result<usize, SpaceError>;

    /// The class of the payload `payload` in a join window, or `None` when every record is a
    /// class of its own.
    fn class_key(&self, payload: &[Par], meter: &ScanMeter) -> Result<Option<Vec<u8>>, SpaceError>;

    /// The stored entry `index` of the window of `coll` as a bind sees it: its payload, its
    /// persistence and its claim. `missing` is the error of an index outside the staged view.
    fn stored_entry(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        coll: usize,
        index: usize,
        missing: &'static str,
    ) -> Result<StoredEntry, SpaceError>;

    /// The stored entries of `coll` that a single-bind class fire visits, in canonical order
    /// (F3, F20).
    fn members(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        coll: usize,
    ) -> Result<Vec<usize>, SpaceError>;

    /// The channel of the produce identity of an element of `coll`.
    fn home<'a>(&'a self, scope: &'a JoinScope, coll: usize) -> Result<&'a Par, SpaceError>;

    /// The element of the stored entry `index` of `coll` that a cascade lifts or re-offers, and
    /// the stored record that the lift removes. `in_flight` is true when the lift removes the
    /// record, so the element is stored nowhere.
    fn stored_element(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        coll: usize,
        index: usize,
        in_flight: bool,
    ) -> Result<(Incoming, ParDatum), SpaceError>;

    /// The second send of Rule E (decision 45) of the stored entry `index` of `coll` that a peek
    /// fire binds as `entry`, or `None` when the entry is persistent.
    fn second_send(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        coll: usize,
        index: usize,
        entry: &Bound,
    ) -> Result<Option<Work>, SpaceError>;

    /// The identity of the incoming side of a fire of the element `x` at a position on `coll`:
    /// the produce that the fire stages there. The copy is reserved first.
    fn incoming_source(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        x: &Incoming,
        coll: usize,
    ) -> Result<Produce, SpaceError>;

    /// The data of the incoming side of a fire of the element `x` at a position on `coll`. The
    /// copy is reserved first.
    fn incoming_record(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        x: &Incoming,
        coll: usize,
    ) -> Result<ListParWithRandom, SpaceError>;

    /// True when the element `x` enters the window of `coll`: its own collection, or on a
    /// PathMap space every prefix of its path. A check that reads keys reserves its work.
    fn enters(
        &self,
        scope: &JoinScope,
        x: &Incoming,
        coll: usize,
        meter: &ScanMeter,
    ) -> Result<bool, SpaceError>;

    /// The payload of the element `x` as a bind on `coll` sees it: its own payload, or on a
    /// PathMap space the payload re-rooted under the suffix of its path below the prefix `coll`.
    fn incoming_payload<'x>(
        &self,
        scope: &JoinScope,
        x: &'x Incoming,
        coll: usize,
        meter: &ScanMeter,
    ) -> Result<Cow<'x, ListParWithRandom>, SpaceError>;

    /// True when a refusal by a waiter of the offer order stores the element: the head of a
    /// head-only collection blocks (decision 11). A source whose offer order lists one head for
    /// each of several collections moves on to the next head (the PathMap source).
    fn refusal_blocks(&self) -> bool;

    /// True when every collection of the scope bumps one counter, so a transaction bumps it once
    /// (the token of a PathMap space).
    fn shares_counter(&self) -> bool;

    /// FINISH: the last staging of a transaction before its commit. `changed` is true when the
    /// engine staged a change of state: a store that is not a restore, a park, or the removal of
    /// a stored waiter or of a stored datum that is not persistent. A source adds the changes
    /// that only it stages. A keyed source stages nothing. The PathMap source bumps the token of a
    /// bag or a set space when the transaction changed the state (decision 51).
    fn finish(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        changed: bool,
        random: &Blake2b512Random,
        authority: OperationAuthority<'_>,
    ) -> Result<(), SpaceError>;

    /// What a fire spends, as `before_fire` records it before the fire moves the bound entries.
    type Spent;

    /// The entries that the side of the element `x` holds, for the capacity check (F15).
    fn held(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        x: &Incoming,
    ) -> Result<usize, SpaceError>;

    /// STORE (joins plan section 4.5): stages the store of the element `x`, and marks in `dirty`
    /// the collections that the store changes. The engine checks the capacity and touches the
    /// collection first.
    fn land(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        x: Incoming,
        dirty: &mut [bool],
    ) -> Result<(), SpaceError>;

    /// What a fire at `site` spends, recorded before the fire moves its bound entries `bound` on
    /// the positions `colls`. `members` are the candidates of a single-bind class fire, and `x` is
    /// the element in flight of a produce-side fire.
    #[allow(clippy::too_many_arguments)]
    fn before_fire(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        site: FireSite,
        colls: &[usize],
        bound: &[Bound],
        members: &[ParCandidate],
        x: Option<&Incoming>,
    ) -> Result<Self::Spent, SpaceError>;

    /// Retires after a fire that does not peek what the fire spent, with the random state
    /// `random` and the authority `authority` of its retire continuations, and marks in `dirty`
    /// the collections that the retirement changes. A keyed set retires its bound classes
    /// (RETIRE-SET-BINDS, joins plan section 4.7).
    #[allow(clippy::too_many_arguments)]
    fn retire_spent(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        spent: Self::Spent,
        colls: &[usize],
        random: &Blake2b512Random,
        authority: Option<&CostAuthority>,
        dirty: &mut [bool],
    ) -> Result<(), SpaceError>;
}

/// The source of the keyed stores (HashMap, Array, Vector) with the discipline `D` (P5c). The
/// scope holds the channels of the collections, so the source needs only the discipline. `S`
/// names the outer store of the family.
pub(crate) struct KeyedSource<'a, S, D> {
    outer: std::marker::PhantomData<&'a S>,
    discipline: &'a D,
}

/// One waiter of a collection with its order and its lanes there.
struct Seat {
    /// The index of its group in the groups of the collection.
    group: usize,
    /// Its index in the waiters of its group.
    index: usize,
    /// The source hash of its consume.
    source: Blake2b256Hash,
    /// Its order in the collection.
    ord: i64,
    /// Its lanes in the collection: one lane for each distinct lane of its positions there, or
    /// the one lane `None` outside a priority queue.
    lanes: Vec<Option<Lane>>,
}

/// The waiters of a collection: the groups of its data channel and one seat for each waiter.
struct Seats {
    groups: Vec<Vec<Par>>,
    seats: Vec<Seat>,
}

impl<S: OuterStore, D: Discipline> KeyedSource<'_, S, D> {
    /// The lane of the slot `slot` in this discipline: `Some` in a priority queue, `None`
    /// otherwise.
    fn lane_of(&self, slot: &JoinSlot) -> Option<Lane> {
        match D::JOIN_WINDOW {
            JoinWindow::Lanes => Some(Lane::of_slot(slot.policy.level())),
            JoinWindow::Head | JoinWindow::Scan | JoinWindow::Classes | JoinWindow::Similarity => {
                None
            }
        }
    }

    /// The collection of each channel of `group`.
    fn group_colls(
        &self,
        scope: &JoinScope,
        group: &[Par],
        meter: &ScanMeter,
    ) -> Result<Vec<usize>, SpaceError> {
        let mut colls = meter.buffer::<usize>(group.len())?;
        for channel in group {
            colls.push(scope.index_of_channel(channel, meter)?);
        }
        Ok(colls)
    }

    /// The seats of the waiters of `coll`: each waiter of each join group of its data channel,
    /// with its order and its lanes on `coll`. Each copy, decode and comparison is reserved.
    fn seats(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        coll: usize,
    ) -> Result<Seats, SpaceError> {
        let meter = cx.meter();
        let channel = &collection(scope, coll)?.data;
        let groups: Vec<Vec<Par>> = {
            let joins = cx.joins_on(channel)?;
            meter.copy_slice(joins)?
        };
        let mut seats: Vec<Seat> = Vec::new();
        for (group_index, group) in groups.iter().enumerate() {
            let colls = self.group_colls(scope, group, meter)?;
            let mut positions = meter.buffer::<usize>(colls.len())?;
            positions.extend(
                colls
                    .iter()
                    .enumerate()
                    .filter(|(_, &other)| other == coll)
                    .map(|(position, _)| position),
            );
            let Some(&first) = positions.first() else {
                return Err(SpaceError::Inconsistent(
                    "a join group of a collection does not hold its channel",
                ));
            };
            let count = cx.group_waiters(group)?.len();
            for index in 0..count {
                let (slots, source) = {
                    let waiters = cx.group_waiters(group)?;
                    let waiter = waiters.get(index).ok_or(SpaceError::Inconsistent(
                        "a waiter index is outside the staged view",
                    ))?;
                    let slots = self.waiter_slots(waiter, group.len(), meter)?;
                    meter.reserve_copy(&waiter.source.hash)?;
                    (slots, waiter.source.hash.clone())
                };
                let ord = slot_at(&slots, first)?.ord;
                let mut lanes: Vec<Option<Lane>> = Vec::new();
                for &position in &positions {
                    let slot = slot_at(&slots, position)?;
                    meter.compare(size_of::<i64>(), size_of::<i64>())?;
                    if slot.ord != ord {
                        return Err(SpaceError::Inconsistent(
                            "a waiter has two orders in one collection",
                        ));
                    }
                    let lane = self.lane_of(slot);
                    if !lanes.contains(&lane) {
                        generic::push_reserved(&mut lanes, lane, meter)?;
                    }
                }
                generic::push_reserved(
                    &mut seats,
                    Seat {
                        group: group_index,
                        index,
                        source,
                        ord,
                        lanes,
                    },
                    meter,
                )?;
            }
        }
        Ok(Seats { groups, seats })
    }

    /// The head of the seats in the lane `lane` (every seat for `None`): the seat whose order
    /// interacts first by the head rule of the discipline (`head_metered`).
    fn head_seat(
        &self,
        seats: &[Seat],
        lane: Option<Lane>,
        meter: &ScanMeter,
    ) -> Result<Option<usize>, SpaceError> {
        let mut members = meter.buffer::<usize>(seats.len())?;
        members.extend(
            seats
                .iter()
                .enumerate()
                .filter(|(_, seat)| lane.is_none() || seat.lanes.contains(&lane))
                .map(|(index, _)| index),
        );
        let mut ordered = meter.buffer::<Ordered<'_, usize>>(members.len())?;
        for member in &members {
            let seat = seats
                .get(*member)
                .ok_or(SpaceError::Inconsistent("a seat index is outside its list"))?;
            ordered.push(Ordered {
                ord: seat.ord,
                entry: member,
            });
        }
        let head = self
            .discipline
            .head_metered(Side::Waiters, &ordered, meter)?;
        Ok(head.and_then(|position| members.get(position).copied()))
    }

    /// The reference of the seat `seat`.
    fn reference(
        &self,
        seats: &Seats,
        seat: usize,
        meter: &ScanMeter,
    ) -> Result<WaiterRef, SpaceError> {
        let seat = seats
            .seats
            .get(seat)
            .ok_or(SpaceError::Inconsistent("a seat index is outside its list"))?;
        let group = seats
            .groups
            .get(seat.group)
            .ok_or(SpaceError::Inconsistent("a seat names no group"))?;
        Ok(WaiterRef {
            group: meter.copy(group)?,
            index: seat.index,
            source: meter.copy(&seat.source)?,
        })
    }

    /// The lanes of the slots `slots` on the collection `coll` of the positions `colls`, each
    /// once.
    fn lanes_on(
        &self,
        slots: &[JoinSlot],
        colls: &[usize],
        coll: usize,
        meter: &ScanMeter,
    ) -> Result<Vec<Option<Lane>>, SpaceError> {
        let mut lanes: Vec<Option<Lane>> = Vec::new();
        for (slot, &other) in slots.iter().zip(colls) {
            if other != coll {
                continue;
            }
            let lane = self.lane_of(slot);
            if !lanes.contains(&lane) {
                generic::push_reserved(&mut lanes, lane, meter)?;
            }
        }
        Ok(lanes)
    }

    /// The order of the place of an element with the order `ord` in a side whose stored orders
    /// are `stored` (joins plan section 3.1): a fresh element is the head when
    /// `Discipline::reaches_head` says so, else the tail. An element with a carried order is
    /// placed after every stored entry that it does not precede. An equal order is
    /// `Inconsistent`. Each comparison is reserved.
    fn place(
        &self,
        side: Side,
        placement: Placement,
        ord: i64,
        stored: &[i64],
        meter: &ScanMeter,
    ) -> Result<usize, SpaceError> {
        match placement {
            Placement::Fresh => Ok(match self.discipline.reaches_head(side, stored.len()) {
                true => 0,
                false => stored.len(),
            }),
            Placement::Carried => {
                let mut ahead = 0;
                for &other in stored {
                    meter.compare(size_of::<i64>(), size_of::<i64>())?;
                    if other == ord {
                        return Err(SpaceError::Inconsistent(
                            "an element in flight shares its order with a stored entry",
                        ));
                    }
                    if !self.discipline.precedes(side, ord, other, meter)? {
                        ahead += 1;
                    }
                }
                Ok(ahead)
            }
        }
    }

    /// The first `need` entries of `entries` (order, index) in the head order of the discipline:
    /// the head, then the head of the rest, and so on. Each scan is reserved.
    fn head_order(
        &self,
        entries: &[(i64, usize)],
        need: usize,
        meter: &ScanMeter,
    ) -> Result<Vec<usize>, SpaceError> {
        let rounds = need.min(entries.len());
        let mut taken = meter.buffer::<bool>(entries.len())?;
        taken.resize(entries.len(), false);
        let mut order = meter.buffer::<usize>(rounds)?;
        for _ in 0..rounds {
            let mut remaining = meter.buffer::<usize>(entries.len())?;
            remaining.extend(
                (0..entries.len())
                    .filter(|&position| !taken.get(position).copied().unwrap_or(true)),
            );
            let mut ordered = meter.buffer::<Ordered<'_, usize>>(remaining.len())?;
            for position in &remaining {
                let (ord, index) = entries.get(*position).ok_or(SpaceError::Inconsistent(
                    "an entry index is outside its list",
                ))?;
                ordered.push(Ordered {
                    ord: *ord,
                    entry: index,
                });
            }
            let Some(head) = self.discipline.head_metered(Side::Data, &ordered, meter)? else {
                break;
            };
            let position = *remaining
                .get(head)
                .ok_or(SpaceError::Inconsistent("a head is outside its list"))?;
            if let Some(mark) = taken.get_mut(position) {
                *mark = true;
            }
            let (_, index) = entries.get(position).ok_or(SpaceError::Inconsistent(
                "an entry index is outside its list",
            ))?;
            order.push(*index);
        }
        Ok(order)
    }

    /// The head window of `need` binds on the records `records` (decision 11): the first `need`
    /// entries of the head order, with `incoming` at its place.
    fn head_window(
        &self,
        records: &[(i64, usize)],
        need: usize,
        incoming: Option<&Incoming>,
        meter: &ScanMeter,
    ) -> Result<Vec<Entry>, SpaceError> {
        let order = self.head_order(records, need, meter)?;
        let length = order
            .len()
            .checked_add(1)
            .ok_or(RSpaceError::HostWorkRejected)?;
        let mut window = meter.buffer::<Entry>(length)?;
        window.extend(order.into_iter().map(|index| Entry {
            at: EntryAt::Stored(index),
            level: None,
        }));
        if let Some(x) = incoming {
            let mut ords = meter.buffer::<i64>(records.len())?;
            ords.extend(records.iter().map(|(ord, _)| *ord));
            let place = self.place(Side::Data, x.placement, x.ord, &ords, meter)?;
            if place < need && place <= window.len() {
                window.insert(place, Entry {
                    at: EntryAt::Incoming,
                    level: None,
                });
                window.truncate(need);
            }
        }
        Ok(window)
    }
}

/// The scoped collection `coll`.
fn collection(scope: &JoinScope, coll: usize) -> Result<&ScopedCollection, SpaceError> {
    scope.collections.get(coll).ok_or(SpaceError::Inconsistent(
        "a collection is outside its join scope",
    ))
}

/// The slot at the group position `position`.
fn slot_at(slots: &[JoinSlot], position: usize) -> Result<&JoinSlot, SpaceError> {
    slots.get(position).ok_or(SpaceError::Inconsistent(
        "a waiter has not one slot for each position",
    ))
}

impl<S: OuterStore, D: Discipline> JoinSource for KeyedSource<'_, S, D> {
    fn shape(&self) -> JoinWindow { D::JOIN_WINDOW }

    // Replaced by P6 (design v3 4.0): the bound of the discipline under the config.
    // fn capacity(&self) -> Option<usize> { D::CAPACITY }

    fn bound(
        &self,
        config: &SpaceConfig,
        side: Side,
        meter: &ScanMeter,
    ) -> Result<Option<SideBound>, SpaceError> {
        self.discipline.bound(config, side, meter)
    }

    fn touch(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        coll: usize,
        random: &Blake2b512Random,
        authority: OperationAuthority<'_>,
    ) -> Result<Option<i64>, SpaceError> {
        if !D::USES_COUNTER {
            return Ok(None);
        }
        let counter = &collection(scope, coll)?.counter;
        let counters = CounterChannels {
            space: &scope.space_counter,
            collection: counter,
        };
        cx.bump_on(counters, random, authority).map(Some)
    }

    fn wrap_record(
        &self,
        ord: i64,
        level: Option<Level>,
        data: &ListParWithRandom,
        meter: &ScanMeter,
    ) -> Result<ListParWithRandom, SpaceError> {
        match (D::JOIN_WINDOW, level) {
            (JoinWindow::Lanes, Some(level)) => Ok(record::wrap_pq_datum_metered(
                ord,
                level.slot(),
                data,
                meter,
            )?),
            (JoinWindow::Head | JoinWindow::Scan | JoinWindow::Classes, None) => {
                Ok(record::wrap_datum_metered(ord, data, meter)?)
            }
            // A VectorDB record holds the vector of its send (P6, design v3 4.3), so the engine
            // builds it from the params of the send (`vectordb::vdb_record`), never from the data
            // alone.
            (JoinWindow::Similarity, _) => Err(SpaceError::Inconsistent(
                "a VectorDB record is built from the params of its send",
            )),
            _ => Err(SpaceError::Inconsistent(
                "a record level does not fit its discipline",
            )),
        }
    }

    fn record_slots(
        &self,
        record: &ListParWithRandom,
        meter: &ScanMeter,
    ) -> Result<(i64, Option<Level>), SpaceError> {
        match D::JOIN_WINDOW {
            JoinWindow::Lanes => {
                let (ord, slot) = record::pq_datum_slots_metered(record, meter)?;
                Ok((ord, Some(Level::of_slot(slot))))
            }
            JoinWindow::Head | JoinWindow::Scan | JoinWindow::Classes => {
                Ok((record::datum_ord_metered(record, meter)?, None))
            }
            JoinWindow::Similarity => Ok((record::vdb_datum_slots_metered(record, meter)?.0, None)),
        }
    }

    fn payload_of(
        &self,
        record: &ListParWithRandom,
        meter: &ScanMeter,
    ) -> Result<ListParWithRandom, SpaceError> {
        match D::JOIN_WINDOW {
            JoinWindow::Lanes => Ok(record::unwrap_pq_datum_metered(record, meter)?.datum),
            JoinWindow::Head | JoinWindow::Scan | JoinWindow::Classes => {
                Ok(record::unwrap_datum_metered(record, meter)?.datum)
            }
            JoinWindow::Similarity => vectordb::payload_of(record, meter),
        }
    }

    fn waiter_slots(
        &self,
        waiter: &ParWaiter,
        width: usize,
        meter: &ScanMeter,
    ) -> Result<Vec<JoinSlot>, SpaceError> {
        match width {
            1 => {
                let slot = match D::JOIN_WINDOW {
                    JoinWindow::Lanes => {
                        let (ord, priority) =
                            record::pq_continuation_slots_metered(&waiter.continuation, meter)?;
                        JoinSlot {
                            ord,
                            bind: 0,
                            policy: WaiterPolicy::of_level(priority),
                        }
                    }
                    JoinWindow::Head | JoinWindow::Scan | JoinWindow::Classes => JoinSlot {
                        ord: record::continuation_ord_metered(&waiter.continuation, meter)?,
                        bind: 0,
                        policy: WaiterPolicy::None,
                    },
                    JoinWindow::Similarity => {
                        let (ord, policy) =
                            record::continuation_policy_slots_metered(&waiter.continuation, meter)?;
                        JoinSlot {
                            ord,
                            bind: 0,
                            policy,
                        }
                    }
                };
                let mut slots = meter.buffer::<JoinSlot>(1)?;
                slots.push(slot);
                Ok(slots)
            }
            _ => {
                let slots = record::join_continuation_slots_metered(&waiter.continuation, meter)?;
                let lanes_fit = match D::JOIN_WINDOW {
                    JoinWindow::Lanes => true,
                    JoinWindow::Head | JoinWindow::Scan | JoinWindow::Classes => {
                        slots.iter().all(|slot| slot.policy == WaiterPolicy::None)
                    }
                    // A VectorDB bind keeps its query map, or Nil without a query.
                    JoinWindow::Similarity => {
                        slots.iter().all(|slot| slot.policy.level().is_none())
                    }
                };
                match (slots.len() == width, lanes_fit) {
                    (true, true) => Ok(slots),
                    _ => Err(SpaceError::Inconsistent(
                        "the slots of a join waiter do not fit its group",
                    )),
                }
            }
        }
    }

    fn window(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        coll: usize,
        need: usize,
        incoming: Option<&Incoming>,
        _pending: &[Work],
    ) -> Result<Vec<Entry>, SpaceError> {
        let meter = cx.meter();
        let channel = &collection(scope, coll)?.data;
        let incoming = incoming.filter(|x| x.coll == coll);
        match D::JOIN_WINDOW {
            JoinWindow::Head => {
                let records: Vec<(i64, usize)> = {
                    let stored = cx.data_on(channel)?;
                    let mut records = meter.buffer::<(i64, usize)>(stored.len())?;
                    for (index, datum) in stored.iter().enumerate() {
                        records.push((self.record_slots(&datum.a, meter)?.0, index));
                    }
                    records
                };
                self.head_window(&records, need, incoming, meter)
            }
            JoinWindow::Lanes => {
                let mut levels: BTreeMap<Level, Vec<(i64, usize)>> = BTreeMap::new();
                {
                    let stored = cx.data_on(channel)?;
                    for (index, datum) in stored.iter().enumerate() {
                        let (ord, level) = self.record_slots(&datum.a, meter)?;
                        let level = level.ok_or(SpaceError::Inconsistent(
                            "a record of a priority queue has no level",
                        ))?;
                        meter.ordered_lookup(levels.len(), size_of::<Level>())?;
                        if !levels.contains_key(&level) {
                            meter.tree_growth::<Level, Vec<(i64, usize)>>(levels.len(), 1)?;
                        }
                        generic::push_reserved(
                            levels.entry(level).or_default(),
                            (ord, index),
                            meter,
                        )?;
                    }
                }
                if let Some(x) = incoming {
                    let level = x.level.ok_or(SpaceError::Inconsistent(
                        "an element of a priority queue has no level",
                    ))?;
                    meter.ordered_lookup(levels.len(), size_of::<Level>())?;
                    if !levels.contains_key(&level) {
                        meter.tree_growth::<Level, Vec<(i64, usize)>>(levels.len(), 1)?;
                        levels.insert(level, Vec::new());
                    }
                }
                let mut window = Vec::new();
                for (level, records) in &levels {
                    let in_level = incoming.filter(|x| x.level == Some(*level));
                    let entries = self.head_window(records, need, in_level, meter)?;
                    for entry in entries {
                        generic::push_reserved(
                            &mut window,
                            Entry {
                                at: entry.at,
                                level: Some(*level),
                            },
                            meter,
                        )?;
                    }
                }
                Ok(window)
            }
            // A similarity bind sees its own eligible records (`Engine::similarity_bind`); this
            // window of every record is the window of a bind without a query.
            JoinWindow::Scan | JoinWindow::Classes | JoinWindow::Similarity => {
                // Every stored record is in the window. An element in flight always lies in the
                // window of a scan, so it joins it at the end: no stored record takes its place.
                let order = cx.canonical_data_order(channel)?;
                let length = order
                    .len()
                    .checked_add(usize::from(incoming.is_some()))
                    .ok_or(RSpaceError::HostWorkRejected)?;
                let mut window = meter.buffer::<Entry>(length)?;
                window.extend(order.into_iter().map(|index| Entry {
                    at: EntryAt::Stored(index),
                    level: None,
                }));
                if incoming.is_some() {
                    window.push(Entry {
                        at: EntryAt::Incoming,
                        level: None,
                    });
                }
                Ok(window)
            }
        }
    }

    fn claim(
        &self,
        coll: usize,
        at: EntryAt,
        payload: &ListParWithRandom,
        meter: &ScanMeter,
    ) -> Result<Claim, SpaceError> {
        match D::JOIN_WINDOW {
            JoinWindow::Classes => match self.discipline.join_class_key(&payload.pars, meter)? {
                Some(key) => {
                    let hash = metered_hash(&key, meter)?;
                    Ok(Claim::Class { coll, key, hash })
                }
                None => Ok(Claim::Record { coll, at }),
            },
            JoinWindow::Head | JoinWindow::Lanes | JoinWindow::Scan | JoinWindow::Similarity => {
                Ok(Claim::Record { coll, at })
            }
        }
    }

    fn offer_order(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        incoming: &Incoming,
    ) -> Result<Vec<WaiterRef>, SpaceError> {
        let meter = cx.meter();
        match D::JOIN_WINDOW {
            JoinWindow::Head => {
                let seats = self.seats(cx, scope, incoming.coll)?;
                match self.head_seat(&seats.seats, None, meter)? {
                    Some(head) => Ok(vec![self.reference(&seats, head, meter)?]),
                    None => Ok(Vec::new()),
                }
            }
            JoinWindow::Lanes => {
                let level = incoming.level.ok_or(SpaceError::Inconsistent(
                    "an element of a priority queue has no level",
                ))?;
                let seats = self.seats(cx, scope, incoming.coll)?;
                let mut heads: Vec<usize> = Vec::new();
                for lane in level.candidate_lanes().into_iter().flatten() {
                    if let Some(head) = self.head_seat(&seats.seats, Some(lane), meter)? {
                        if !heads.contains(&head) {
                            generic::push_reserved(&mut heads, head, meter)?;
                        }
                    }
                }
                self.oldest_first(&seats, heads, meter)
            }
            JoinWindow::Scan | JoinWindow::Classes | JoinWindow::Similarity => {
                self.union_order(cx, scope, incoming.coll)
            }
        }
    }

    fn cascade_order(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        coll: usize,
    ) -> Result<Vec<WaiterRef>, SpaceError> {
        let meter = cx.meter();
        match D::JOIN_WINDOW {
            JoinWindow::Head => {
                let seats = self.seats(cx, scope, coll)?;
                match self.head_seat(&seats.seats, None, meter)? {
                    Some(head) => Ok(vec![self.reference(&seats, head, meter)?]),
                    None => Ok(Vec::new()),
                }
            }
            JoinWindow::Lanes => {
                let seats = self.seats(cx, scope, coll)?;
                let mut lanes: Vec<Option<Lane>> = Vec::new();
                for seat in &seats.seats {
                    for lane in &seat.lanes {
                        if !lanes.contains(lane) {
                            generic::push_reserved(&mut lanes, *lane, meter)?;
                        }
                    }
                }
                let mut heads: Vec<usize> = Vec::new();
                for lane in lanes {
                    if let Some(head) = self.head_seat(&seats.seats, lane, meter)? {
                        if !heads.contains(&head) {
                            generic::push_reserved(&mut heads, head, meter)?;
                        }
                    }
                }
                self.oldest_first(&seats, heads, meter)
            }
            // Decision 50: a set cascades over every waiter of the collection, in canonical order,
            // because a class can move when its first member leaves (F-JF-1). A bag needs none.
            // A VectorDB cascades over its list waiters (decision 66, LM-b), which the engine
            // picks out of every waiter of the collection.
            JoinWindow::Classes | JoinWindow::Similarity => self.union_order(cx, scope, coll),
            JoinWindow::Scan => Ok(Vec::new()),
        }
    }

    fn at_head(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        waiter: &WaiterRef,
        slots: &[JoinSlot],
        colls: &[usize],
        coll: usize,
    ) -> Result<bool, SpaceError> {
        let meter = cx.meter();
        match D::JOIN_WINDOW {
            JoinWindow::Scan | JoinWindow::Classes | JoinWindow::Similarity => Ok(true),
            JoinWindow::Head | JoinWindow::Lanes => {
                let seats = self.seats(cx, scope, coll)?;
                for lane in self.lanes_on(slots, colls, coll, meter)? {
                    let filter = match D::JOIN_WINDOW {
                        JoinWindow::Lanes => lane,
                        JoinWindow::Head
                        | JoinWindow::Scan
                        | JoinWindow::Classes
                        | JoinWindow::Similarity => None,
                    };
                    let Some(head) = self.head_seat(&seats.seats, filter, meter)? else {
                        return Ok(false);
                    };
                    let seat = seats
                        .seats
                        .get(head)
                        .ok_or(SpaceError::Inconsistent("a seat index is outside its list"))?;
                    meter.compare(seat.source.0.len(), waiter.source.0.len())?;
                    if seat.source != waiter.source {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
        }
    }

    fn at_head_incoming(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        slots: &[JoinSlot],
        colls: &[usize],
        coll: usize,
        carried: Option<i64>,
    ) -> Result<bool, SpaceError> {
        let meter = cx.meter();
        match D::JOIN_WINDOW {
            JoinWindow::Scan | JoinWindow::Classes | JoinWindow::Similarity => Ok(true),
            JoinWindow::Head | JoinWindow::Lanes => {
                let seats = self.seats(cx, scope, coll)?;
                for lane in self.lanes_on(slots, colls, coll, meter)? {
                    let filter = match D::JOIN_WINDOW {
                        JoinWindow::Lanes => lane,
                        JoinWindow::Head
                        | JoinWindow::Scan
                        | JoinWindow::Classes
                        | JoinWindow::Similarity => None,
                    };
                    let reaches = match carried {
                        None => {
                            let queued = seats
                                .seats
                                .iter()
                                .filter(|seat| filter.is_none() || seat.lanes.contains(&filter))
                                .count();
                            self.discipline.reaches_head(Side::Waiters, queued)
                        }
                        Some(ord) => match self.head_seat(&seats.seats, filter, meter)? {
                            None => true,
                            Some(head) => {
                                let head_ord = seats
                                    .seats
                                    .get(head)
                                    .ok_or(SpaceError::Inconsistent(
                                        "a seat index is outside its list",
                                    ))?
                                    .ord;
                                meter.compare(size_of::<i64>(), size_of::<i64>())?;
                                match head_ord == ord {
                                    true => return Err(SpaceError::Inconsistent(
                                        "a re-issued element shares its order with a stored entry",
                                    )),
                                    false => self.discipline.precedes(
                                        Side::Waiters,
                                        ord,
                                        head_ord,
                                        meter,
                                    )?,
                                }
                            }
                        },
                    };
                    if !reaches {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
        }
    }

    fn waiter_count(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        coll: usize,
    ) -> Result<usize, SpaceError> {
        let meter = cx.meter();
        let channel = &collection(scope, coll)?.data;
        let groups: Vec<Vec<Par>> = {
            let joins = cx.joins_on(channel)?;
            meter.copy_slice(joins)?
        };
        let mut count = 0usize;
        for group in &groups {
            count = count.checked_add(cx.group_waiters(group)?.len()).ok_or(
                SpaceError::Inconsistent("a collection holds too many waiters"),
            )?;
        }
        Ok(count)
    }

    fn class_key(&self, payload: &[Par], meter: &ScanMeter) -> Result<Option<Vec<u8>>, SpaceError> {
        self.discipline.join_class_key(payload, meter)
    }

    fn stored_entry(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        coll: usize,
        index: usize,
        missing: &'static str,
    ) -> Result<StoredEntry, SpaceError> {
        let meter = cx.meter();
        let (payload, persist) = {
            let records = cx.data_on(&collection(scope, coll)?.data)?;
            let datum = records
                .get(index)
                .ok_or(SpaceError::Inconsistent(missing))?;
            (self.payload_of(&datum.a, meter)?, datum.persist)
        };
        let claim = self.claim(coll, EntryAt::Stored(index), &payload, meter)?;
        Ok(StoredEntry {
            payload,
            persist,
            claim,
        })
    }

    fn members(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        coll: usize,
    ) -> Result<Vec<usize>, SpaceError> {
        cx.canonical_data_order(&collection(scope, coll)?.data)
    }

    fn home<'a>(&'a self, scope: &'a JoinScope, coll: usize) -> Result<&'a Par, SpaceError> {
        Ok(&collection(scope, coll)?.data)
    }

    fn stored_element(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        coll: usize,
        index: usize,
        in_flight: bool,
    ) -> Result<(Incoming, ParDatum), SpaceError> {
        let meter = cx.meter();
        let datum = {
            let records = cx.data_on(&collection(scope, coll)?.data)?;
            let datum = records.get(index).ok_or(SpaceError::Inconsistent(
                "a bound index is outside the staged view",
            ))?;
            meter.copy(datum)?
        };
        let (ord, level) = self.record_slots(&datum.a, meter)?;
        let payload = self.payload_of(&datum.a, meter)?;
        let introduction = cx.introduction().for_record(&datum.a, meter)?;
        let x = Incoming {
            coll,
            record: meter.copy(&datum.a)?,
            payload,
            source: meter.copy(&datum.source)?,
            persist: datum.persist,
            ord,
            level,
            placement: Placement::Carried,
            introduction,
            own: false,
            in_flight,
            restore: None,
        };
        Ok((x, datum))
    }

    fn second_send(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        coll: usize,
        index: usize,
        entry: &Bound,
    ) -> Result<Option<Work>, SpaceError> {
        if entry.persist {
            return Ok(None);
        }
        let meter = cx.meter();
        let record = {
            let records = cx.data_on(&collection(scope, coll)?.data)?;
            let datum = records.get(index).ok_or(SpaceError::Inconsistent(
                "a bound index is outside the staged view",
            ))?;
            meter.copy(&datum.a)?
        };
        Ok(Some(Work {
            coll,
            record,
            persist: false,
            own: false,
            restore: None,
        }))
    }

    fn incoming_source(
        &self,
        cx: &mut Cx<'_, '_>,
        _scope: &JoinScope,
        x: &Incoming,
        _coll: usize,
    ) -> Result<Produce, SpaceError> {
        Ok(cx.meter().copy(&x.source)?)
    }

    fn incoming_record(
        &self,
        cx: &mut Cx<'_, '_>,
        _scope: &JoinScope,
        x: &Incoming,
        _coll: usize,
    ) -> Result<ListParWithRandom, SpaceError> {
        Ok(cx.meter().copy(&x.record)?)
    }

    fn enters(
        &self,
        _scope: &JoinScope,
        x: &Incoming,
        coll: usize,
        _meter: &ScanMeter,
    ) -> Result<bool, SpaceError> {
        Ok(coll == x.coll)
    }

    fn incoming_payload<'x>(
        &self,
        _scope: &JoinScope,
        x: &'x Incoming,
        _coll: usize,
        _meter: &ScanMeter,
    ) -> Result<Cow<'x, ListParWithRandom>, SpaceError> {
        Ok(Cow::Borrowed(&x.payload))
    }

    fn refusal_blocks(&self) -> bool { D::JOIN_WINDOW == JoinWindow::Head }

    fn shares_counter(&self) -> bool { false }

    fn finish(
        &self,
        _cx: &mut Cx<'_, '_>,
        _scope: &JoinScope,
        _changed: bool,
        _random: &Blake2b512Random,
        _authority: OperationAuthority<'_>,
    ) -> Result<(), SpaceError> {
        Ok(())
    }

    type Spent = Vec<Claim>;

    fn held(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        x: &Incoming,
    ) -> Result<usize, SpaceError> {
        Ok(cx.data_on(&collection(scope, x.coll)?.data)?.len())
    }

    fn land(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        x: Incoming,
        dirty: &mut [bool],
    ) -> Result<(), SpaceError> {
        let channel = &collection(scope, x.coll)?.data;
        cx.store_on(channel, x.record, x.persist, &x.introduction)?;
        mark(dirty, &[x.coll]);
        Ok(())
    }

    fn before_fire(
        &self,
        cx: &mut Cx<'_, '_>,
        _scope: &JoinScope,
        site: FireSite,
        _colls: &[usize],
        bound: &[Bound],
        _members: &[ParCandidate],
        _x: Option<&Incoming>,
    ) -> Result<Vec<Claim>, SpaceError> {
        match site {
            // A single-bind class fire removes the whole class over [D; n], so it retires
            // nothing afterwards (F3, F20).
            FireSite::Single => Ok(Vec::new()),
            FireSite::Offer | FireSite::Join | FireSite::Cascade => {
                self.class_claims(bound, cx.meter())
            }
        }
    }

    fn retire_spent(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        spent: Vec<Claim>,
        colls: &[usize],
        random: &Blake2b512Random,
        authority: Option<&CostAuthority>,
        _dirty: &mut [bool],
    ) -> Result<(), SpaceError> {
        if spent.is_empty() {
            return Ok(());
        }
        self.retire_classes(cx, scope, colls, &spent, random, authority)
    }
}

impl<S: OuterStore, D: Discipline> KeyedSource<'_, S, D> {
    /// RETIRE-SET-BINDS (joins plan section 4.7): after a fire of a waiter that does not peek, the
    /// other stored members of each bound set class that are not persistent leave with
    /// single-channel `K_retire` COMMs, in canonical order. `random` and `authority` are those of
    /// the element whose fire retires the classes. A discipline without classes retires nothing.
    fn retire_classes(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        colls: &[usize],
        claims: &[Claim],
        random: &Blake2b512Random,
        authority: Option<&CostAuthority>,
    ) -> Result<(), SpaceError> {
        if D::JOIN_WINDOW != JoinWindow::Classes {
            return Ok(());
        }
        let meter = cx.meter();
        let retire_random = RandomPurpose::Retire.derive(random);
        let mut retired: Vec<Claim> = Vec::new();
        for (claim, &coll) in claims.iter().zip(colls) {
            let Claim::Class { key, hash, .. } = claim else {
                continue;
            };
            if is_claimed(&retired, claim, meter)? {
                continue;
            }
            generic::push_reserved(&mut retired, claim.clone(), meter)?;
            retire_class_on(
                cx,
                &collection(scope, coll)?.data,
                key,
                hash,
                &retire_random,
                authority,
                |payload, meter| self.class_key(payload, meter),
            )?;
        }
        Ok(())
    }

    /// The claims of the bound entries `bound`, for RETIRE-SET-BINDS after the fire moved the
    /// entries into its candidates. Only a discipline with classes retires, so the other
    /// disciplines copy nothing. The copy of each class key and hash is reserved.
    fn class_claims(&self, bound: &[Bound], meter: &ScanMeter) -> Result<Vec<Claim>, SpaceError> {
        if D::JOIN_WINDOW != JoinWindow::Classes {
            return Ok(Vec::new());
        }
        let mut claims = meter.buffer::<Claim>(bound.len())?;
        for entry in bound {
            if let Claim::Class { key, hash, .. } = &entry.claim {
                meter.reserve_copy_slice(key)?;
                meter.reserve_copy(hash)?;
            }
            claims.push(entry.claim.clone());
        }
        Ok(claims)
    }

    /// Every waiter of the collection `coll`, in the canonical order of DR-75 of the union of its
    /// join groups: a merge of the canonical orders of the groups by the source hash of each
    /// consume (decision 41). Two waiters of distinct groups have distinct consumes, so a tie
    /// across groups is `Inconsistent`. Each copy and comparison is reserved.
    fn union_order(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        coll: usize,
    ) -> Result<Vec<WaiterRef>, SpaceError> {
        let meter = cx.meter();
        let channel = &collection(scope, coll)?.data;
        let groups: Vec<Vec<Par>> = {
            let joins = cx.joins_on(channel)?;
            meter.copy_slice(joins)?
        };
        let mut sequences: Vec<Vec<(Blake2b256Hash, usize)>> = meter.buffer(groups.len())?;
        for group in &groups {
            let order = cx.canonical_group_order(group)?;
            let waiters = cx.group_waiters(group)?;
            let mut sequence = meter.buffer::<(Blake2b256Hash, usize)>(order.len())?;
            for index in order {
                let waiter = waiters.get(index).ok_or(SpaceError::Inconsistent(
                    "a canonical index is outside the staged view",
                ))?;
                meter.reserve_copy(&waiter.source.hash)?;
                sequence.push((waiter.source.hash.clone(), index));
            }
            sequences.push(sequence);
        }
        // The union of the canonical orders of the groups: a merge by the source hash of
        // each consume. Two waiters of distinct groups have distinct consumes, so a tie
        // across groups is `Inconsistent`.
        let total: usize = sequences.iter().map(Vec::len).sum();
        let mut merged = meter.buffer::<WaiterRef>(total)?;
        let mut cursors = meter.buffer::<usize>(sequences.len())?;
        cursors.resize(sequences.len(), 0);
        for _ in 0..total {
            let mut least: Option<(usize, &Blake2b256Hash)> = None;
            for (group, sequence) in sequences.iter().enumerate() {
                let cursor = cursors.get(group).copied().unwrap_or(usize::MAX);
                let Some((hash, _)) = sequence.get(cursor) else {
                    continue;
                };
                least = match least {
                    None => Some((group, hash)),
                    Some((best, best_hash)) => {
                        meter.compare(best_hash.0.len(), hash.0.len())?;
                        match hash.cmp(best_hash) {
                            std::cmp::Ordering::Less => Some((group, hash)),
                            std::cmp::Ordering::Greater => Some((best, best_hash)),
                            std::cmp::Ordering::Equal => {
                                return Err(SpaceError::Inconsistent(
                                    "two waiters of distinct groups share a consume",
                                ))
                            }
                        }
                    }
                };
            }
            let (group, _) = least.ok_or(SpaceError::Inconsistent(
                "a merge of the waiters ran out of waiters",
            ))?;
            let cursor = cursors
                .get_mut(group)
                .ok_or(SpaceError::Inconsistent("a merge cursor is missing"))?;
            let (hash, index) = sequences
                .get(group)
                .and_then(|sequence| sequence.get(*cursor))
                .ok_or(SpaceError::Inconsistent(
                    "a merge cursor is outside its list",
                ))?;
            *cursor += 1;
            let channels = groups
                .get(group)
                .ok_or(SpaceError::Inconsistent("a merge names no group"))?;
            merged.push(WaiterRef {
                group: meter.copy(channels)?,
                index: *index,
                source: meter.copy(hash)?,
            });
        }
        Ok(merged)
    }

    /// The references of the seats `heads`, in ascending order of the seats (the older first).
    /// Each comparison is reserved.
    fn oldest_first(
        &self,
        seats: &Seats,
        mut heads: Vec<usize>,
        meter: &ScanMeter,
    ) -> Result<Vec<WaiterRef>, SpaceError> {
        let mut keyed = meter.buffer::<(i64, usize)>(heads.len())?;
        for head in heads.drain(..) {
            let seat = seats
                .seats
                .get(head)
                .ok_or(SpaceError::Inconsistent("a seat index is outside its list"))?;
            keyed.push((seat.ord, head));
        }
        sort_reserved(&mut keyed, |_| size_of::<i64>(), meter)?;
        let mut references = meter.buffer::<WaiterRef>(keyed.len())?;
        for (_, head) in keyed {
            references.push(self.reference(seats, head, meter)?);
        }
        Ok(references)
    }
}

// ── The engine ──────────────────────────────────────────────────────────────────────────────

/// A stored waiter, loaded for a search: where it is, the collection of each position, its slots
/// and its copy.
struct Waiter {
    at: WaiterRef,
    colls: Vec<usize>,
    slots: Vec<JoinSlot>,
    waiting: ParWaiter,
}

/// The waiter of a consume or of a join of the operation, before it fires or parks.
struct NewWaiter {
    /// The channels of its group, in group order.
    group: Vec<Par>,
    /// The collection of each position.
    colls: Vec<usize>,
    /// The slot of each position.
    slots: Vec<JoinSlot>,
    /// The pattern of each position.
    patterns: Vec<BindPattern>,
    /// The continuation record: version 1 for one bind, version 2 for a join.
    continuation: TaggedContinuation,
    /// True for `<=`.
    persist: bool,
    /// The peek set: every position for `<<-`, or empty.
    peeks: BTreeSet<i32>,
    /// The first-round order of each own collection of a re-issued persistent waiter, by scope
    /// index (decision 43).
    carried: Vec<Option<i64>>,
}

/// A bound entry of a search.
#[derive(Clone, Debug)]
pub(crate) struct Bound {
    /// Where the entry is.
    pub(crate) at: EntryAt,
    /// The values that the pattern of its position binds.
    pub(crate) matched: ListParWithRandom,
    /// True for a persistent entry.
    pub(crate) persist: bool,
    /// What the entry claims.
    pub(crate) claim: Claim,
}

/// A second send of Rule E, or the element of the operation, that the transaction still owes.
pub(crate) struct Work {
    /// The index of its collection in the scope.
    pub(crate) coll: usize,
    /// The record: the envelope, then the payload.
    pub(crate) record: ListParWithRandom,
    /// True for a persistent element.
    pub(crate) persist: bool,
    /// True for the element of the operation.
    pub(crate) own: bool,
    /// The depth of the marker that a peek COMM removed, for a second send of a PathMap datum
    /// (joins plan section 9): a store of the element restores that marker instead. A keyed
    /// store always carries `None`.
    pub(crate) restore: Option<u8>,
}

/// A fire that the cascade owes (joins plan section 4.9): a stored pair with one bound entry for
/// each position, or a VectorDB list waiter with its list (P6, decision 66, LM-b).
enum Fireable {
    /// A stored pair.
    Pair(Waiter, Vec<Bound>),
    /// A list waiter and the list that it binds.
    List(Waiter, ListFire),
}

/// The state of the element of the operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Own {
    /// The plan has not started: the settle before the plan stopped the transaction.
    NotStarted,
    /// The element is in flight: it re-issues with the operation (decision 43).
    InFlight,
    /// The element is stored, parked or spent.
    Settled,
}

/// The template of the lift pattern.
static LIFT_PATTERN: LazyLock<BindPattern> = LazyLock::new(lift_pattern);

/// The template of a `K_lift` continuation without random state bytes and without a cost
/// authority: a reserved copy of it covers the allocations of a new one.
static LIFT_CONTINUATION: LazyLock<TaggedContinuation> = LazyLock::new(|| TaggedContinuation {
    tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
        body: Some(lift_body(0, 0)),
        random_state: Vec::new(),
    })),
    guard: None,
    cost_authority: None,
});

/// The transaction of the join engine.
struct Engine<'r, Src> {
    source: &'r Src,
    scope: &'r JoinScope,
    /// The random state of the operation: its counter continuations take it, and its lifts and
    /// retires derive theirs from it.
    random: Blake2b512Random,
    /// The cost authority of the counter continuations, the lifts and the consume-side retires.
    authority: Option<CostAuthority>,
    /// The random state of the lifts, derived once.
    lift_random: Option<Blake2b512Random>,
    /// The bump of each collection of the scope in this transaction: `None` when it is not
    /// touched, `Some(value)` with the value before the bump (or `None` without a counter).
    bumps: Vec<Option<Option<i64>>>,
    /// The bump of the counter that every collection shares, when the source shares one
    /// (`JoinSource::shares_counter`): the transaction bumps it once.
    shared_bump: Option<Option<i64>>,
    /// True when the transaction staged a change of state that the engine sees (`finish`).
    changed: bool,
    /// The collections that the transaction staged on.
    dirty: Vec<bool>,
    /// The second sends that the transaction owes (a stack: the last pushed runs first).
    work: Vec<Work>,
    own: Own,
    own_cause: Option<ReissueCause>,
    own_placement: Placement,
    stop: Option<ReissueCause>,
    /// The order of the operation on each own collection, by scope index.
    ords: Vec<Option<i64>>,
    /// The params of a VectorDB space (P6), read once, or `None` for every other discipline.
    vdb: Option<VdbConfig>,
}

impl<'r, Src: JoinSource> Engine<'r, Src> {
    fn new(
        source: &'r Src,
        scope: &'r JoinScope,
        op: &SpaceOp,
        config: &SpaceConfig,
        meter: &ScanMeter,
    ) -> Result<Self, SpaceError> {
        let vdb = match source.shape() {
            JoinWindow::Similarity => Some(vectordb::vdb_config(config, meter)?),
            JoinWindow::Head | JoinWindow::Lanes | JoinWindow::Scan | JoinWindow::Classes => None,
        };
        let (random_state, authority): (&[u8], Option<&CostAuthority>) = match op {
            SpaceOp::Produce(op) => (&op.data.random_state, op.data.cost_authority.as_ref()),
            SpaceOp::Consume(op) => (&op.body.random_state, record::stamped(&op.authority)),
            SpaceOp::Settle(op) => (&op.random_state, record::stamped(&op.authority)),
            SpaceOp::Join(op) => (&op.body.random_state, record::stamped(&op.authority)),
        };
        let width = scope.collections.len();
        let mut bumps = meter.buffer::<Option<Option<i64>>>(width)?;
        bumps.resize(width, None);
        let mut dirty = meter.buffer::<bool>(width)?;
        dirty.resize(width, false);
        let mut ords = meter.buffer::<Option<i64>>(width)?;
        ords.resize(width, None);
        let own_placement = match op {
            SpaceOp::Produce(ProduceOp {
                reissued_ord: Some(_),
                ..
            }) => Placement::Carried,
            SpaceOp::Produce(_) | SpaceOp::Consume(_) | SpaceOp::Settle(_) | SpaceOp::Join(_) => {
                Placement::Fresh
            }
        };
        Ok(Engine {
            source,
            scope,
            random: Blake2b512Random::from_bytes(random_state),
            authority: authority
                .map(|authority| meter.copy(authority))
                .transpose()?,
            lift_random: None,
            bumps,
            shared_bump: None,
            changed: false,
            dirty,
            work: Vec::new(),
            own: Own::NotStarted,
            own_cause: None,
            own_placement,
            stop: None,
            ords,
            vdb,
        })
    }

    /// True when the discipline cascades: the head-only disciplines (decision 11) and the set
    /// (decision 50). A removal can expose a fire there: a new head, or a set class that moves
    /// when its first member leaves and a persistent member stays (finding F-JF-1). The bag needs
    /// no cascade, because a removal exposes nothing on a bag (`ref_fill_mono` in
    /// `JoinCascade.v`).
    fn cascades(&self) -> bool {
        matches!(
            self.source.shape(),
            JoinWindow::Head | JoinWindow::Lanes | JoinWindow::Classes | JoinWindow::Similarity
        )
    }

    /// The data channel of the collection `coll`.
    fn channel(&self, coll: usize) -> Result<&'r Par, SpaceError> {
        Ok(&collection(self.scope, coll)?.data)
    }

    /// TOUCH (joins plan section 4.3): bumps the counter of `coll` once in the transaction,
    /// before its first staging.
    fn touch(&mut self, cx: &mut Cx<'_, '_>, coll: usize) -> Result<(), SpaceError> {
        let slot = self.bumps.get(coll).ok_or(SpaceError::Inconsistent(
            "a collection is outside its join scope",
        ))?;
        if slot.is_some() {
            return Ok(());
        }
        let shares = self.source.shares_counter();
        if let (true, Some(bumped)) = (shares, self.shared_bump) {
            if let Some(slot) = self.bumps.get_mut(coll) {
                *slot = Some(bumped);
            }
            return Ok(());
        }
        let introduction = cx.introduction();
        let authority = OperationAuthority {
            continuation: self.authority.as_ref(),
            introduction,
        };
        let bumped = self
            .source
            .touch(cx, self.scope, coll, &self.random, authority)?;
        if let Some(slot) = self.bumps.get_mut(coll) {
            *slot = Some(bumped);
        }
        if shares {
            self.shared_bump = Some(bumped);
        }
        Ok(())
    }

    /// TOUCH of every distinct collection of `colls`, in the order of the scope.
    fn touch_all(&mut self, cx: &mut Cx<'_, '_>, colls: &[usize]) -> Result<(), SpaceError> {
        let mut distinct: Vec<usize> = Vec::new();
        for &coll in colls {
            if !distinct.contains(&coll) {
                generic::push_reserved(&mut distinct, coll, cx.meter())?;
            }
        }
        distinct.sort_unstable();
        for coll in distinct {
            self.touch(cx, coll)?;
        }
        Ok(())
    }

    /// Marks every collection of `colls` dirty.
    fn mark_dirty(&mut self, colls: &[usize]) { mark(&mut self.dirty, colls) }

    /// The bump value of the collection `coll` in this transaction.
    fn bumped(&self, coll: usize) -> Result<Option<i64>, SpaceError> {
        match self.bumps.get(coll) {
            Some(Some(bumped)) => Ok(*bumped),
            Some(None) | None => Err(SpaceError::Inconsistent(
                "a collection is staged on before its touch",
            )),
        }
    }

    /// RUN-JOIN.
    fn run(mut self, cx: &mut Cx<'_, '_>, op: SpaceOp) -> Result<Option<SpaceReissue>, SpaceError> {
        let meter = cx.meter();
        let mut own_colls = meter.buffer::<usize>(self.scope.own.len())?;
        for key in &self.scope.own {
            own_colls.push(self.scope.index_of_key(key, meter)?);
        }
        for &coll in &own_colls {
            self.touch(cx, coll)?;
        }
        let carried = self.carried_ords(&op, &own_colls, meter)?;
        for (&coll, &carried_ord) in own_colls.iter().zip(&carried) {
            let ord = operation_ord(carried_ord, self.bumped(coll)?, meter)?;
            if let Some(slot) = self.ords.get_mut(coll) {
                *slot = Some(ord);
            }
        }
        // A settle job offers the second sends that it carries first, then cascades over the
        // whole scope: it has no plan, so its drain is its settle before the plan (joins plan
        // section 4.10, step 3). Its pending sends are on the work stack before any fire, so a
        // stop at the cap carries them again.
        let mut op = op;
        if let SpaceOp::Settle(settle) = &mut op {
            self.own = Own::Settled;
            if let Some(cascade) = settle.cascade.as_mut() {
                let pending = std::mem::take(&mut cascade.pending);
                let mut work = meter.buffer::<Work>(pending.len())?;
                for pending in pending.into_iter().rev() {
                    let coll = self.scope.index_of_key(&pending.key, meter)?;
                    work.push(Work {
                        coll,
                        record: pending.record,
                        persist: pending.persist,
                        own: false,
                        restore: pending.restore,
                    });
                }
                self.work = work;
            }
            self.dirty.iter_mut().for_each(|mark| *mark = true);
            self.drain_and_cascade(cx)?;
            return self.commit(cx, op);
        }
        if self.cascades() {
            self.dirty.iter_mut().for_each(|mark| *mark = true);
            self.drain_and_cascade(cx)?;
            if self.stop.is_some() {
                return self.commit(cx, op);
            }
        }
        self.dirty.iter_mut().for_each(|mark| *mark = false);
        // The plan opens below the round cap (decision 49, as `generic::open_plan`). A settle
        // before the plan that fills the transaction leaves the plan no fire, so the operation
        // re-issues as the program issued it, with the order rule of decision 43.
        if cx.rounds().fired() >= FIRE_ROUND_CAP {
            self.stop = Some(ReissueCause::RoundCap);
            return self.commit(cx, op);
        }
        let op = match op {
            SpaceOp::Produce(produce) => {
                self.own = Own::InFlight;
                let coll = *own_colls
                    .first()
                    .ok_or(SpaceError::Inconsistent("a produce has no collection"))?;
                let x = self.own_datum(cx, &produce, coll)?;
                self.offer(cx, x)?;
                SpaceOp::Produce(produce)
            }
            SpaceOp::Consume(consume) => {
                self.own = Own::InFlight;
                let waiter = self.consume_waiter(cx, &consume, &own_colls, &carried)?;
                self.receive(cx, waiter)?;
                SpaceOp::Consume(consume)
            }
            SpaceOp::Join(join) => {
                self.own = Own::InFlight;
                let waiter = self.join_waiter(cx, &join, &own_colls, &carried)?;
                self.receive(cx, waiter)?;
                SpaceOp::Join(join)
            }
            SpaceOp::Settle(_) => {
                return Err(SpaceError::Inconsistent(
                    "a settle job has no plan: it commits after its drain",
                ))
            }
        };
        self.drain_and_cascade(cx)?;
        self.commit(cx, op)
    }

    /// The first-round order of the operation on each own collection `own_colls` (decision 43),
    /// or `None` for a fresh operation. Repeated binds on one collection must carry one order.
    fn carried_ords(
        &self,
        op: &SpaceOp,
        own_colls: &[usize],
        meter: &ScanMeter,
    ) -> Result<Vec<Option<i64>>, SpaceError> {
        let mut carried = meter.buffer::<Option<i64>>(own_colls.len())?;
        match op {
            SpaceOp::Produce(produce) => carried.push(produce.reissued_ord),
            SpaceOp::Consume(consume) => carried.push(consume.reissued_ord),
            SpaceOp::Settle(_) => carried.push(None),
            SpaceOp::Join(join) => {
                carried.resize(own_colls.len(), None);
                if let Some(ords) = &join.reissued_ords {
                    for (bind, ord) in join.binds.iter().zip(ords) {
                        let coll = self.scope.index_of_key(&bind.key, meter)?;
                        let position = own_colls
                            .iter()
                            .position(|&own| own == coll)
                            .ok_or(SpaceError::Inconsistent("a bind is not an own collection"))?;
                        let slot = carried
                            .get_mut(position)
                            .ok_or(SpaceError::Inconsistent("a carried order has no slot"))?;
                        meter.compare(size_of::<i64>(), size_of::<i64>())?;
                        match slot {
                            None => *slot = Some(*ord),
                            Some(other) if *other == *ord => {}
                            Some(_) => {
                                return Err(SpaceError::Inconsistent(
                                    "two binds of one collection carry two orders",
                                ))
                            }
                        }
                    }
                }
            }
        }
        Ok(carried)
    }

    /// The order of the operation on the own collection `coll`.
    fn ord_on(&self, coll: usize) -> Result<i64, SpaceError> {
        self.ords
            .get(coll)
            .copied()
            .flatten()
            .ok_or(SpaceError::Inconsistent("an own collection has no order"))
    }

    /// The datum of the produce `op` in flight, with the order of its collection `coll`.
    fn own_datum(
        &self,
        cx: &mut Cx<'_, '_>,
        op: &ProduceOp,
        coll: usize,
    ) -> Result<Incoming, SpaceError> {
        let meter = cx.meter();
        let ord = self.ord_on(coll)?;
        let level = match self.source.shape() {
            JoinWindow::Lanes => Some(send_level(&op.params, meter)?),
            JoinWindow::Head | JoinWindow::Scan | JoinWindow::Classes | JoinWindow::Similarity => {
                None
            }
        };
        let record = match self.source.shape() {
            // The record of a VectorDB send holds its vector (P6, design v3 4.3).
            JoinWindow::Similarity => {
                let cfg = vectordb::vdb_config(cx.config(), meter)?;
                vectordb::vdb_record(&cfg, &op.params, &op.data, meter)?.0
            }
            JoinWindow::Head | JoinWindow::Lanes | JoinWindow::Scan | JoinWindow::Classes => {
                self.source.wrap_record(ord, level, &op.data, meter)?
            }
        };
        let source = Produce::create_metered(
            self.source.home(self.scope, coll)?,
            &record,
            op.persistent,
            meter,
        )?;
        meter.shared(1)?;
        Ok(Incoming {
            coll,
            payload: meter.copy(&op.data)?,
            record,
            source,
            persist: op.persistent,
            ord,
            level,
            placement: self.own_placement,
            introduction: Arc::clone(cx.introduction()),
            own: true,
            in_flight: true,
            restore: None,
        })
    }

    /// The waiter of the consume `op` on its own collection: one position, a record of version
    /// 1.
    fn consume_waiter(
        &self,
        cx: &mut Cx<'_, '_>,
        op: &ConsumeOp,
        own_colls: &[usize],
        carried: &[Option<i64>],
    ) -> Result<NewWaiter, SpaceError> {
        let meter = cx.meter();
        let coll = *own_colls
            .first()
            .ok_or(SpaceError::Inconsistent("a consume has no collection"))?;
        let ord = self.ord_on(coll)?;
        let lane = match self.source.shape() {
            JoinWindow::Lanes => Some(receive_lane(&op.query, meter)?),
            JoinWindow::Head | JoinWindow::Scan | JoinWindow::Classes | JoinWindow::Similarity => {
                None
            }
        };
        // A VectorDB waiter keeps its query in its policy slot (P6, design v3 4.3).
        let query_policy = match self.source.shape() {
            JoinWindow::Similarity => Some(vectordb::policy_of_query(op.query.as_ref(), meter)?),
            JoinWindow::Head | JoinWindow::Lanes | JoinWindow::Scan | JoinWindow::Classes => None,
        };
        let continuation = match (lane, &query_policy) {
            (Some(lane), _) => record::wrap_pq_continuation_metered(
                ord,
                lane.slot(),
                &op.body,
                &op.guard,
                record::stamped(&op.authority),
                meter,
            )?,
            (None, Some(policy)) => record::wrap_policy_continuation_metered(
                ord,
                policy,
                &op.body,
                &op.guard,
                record::stamped(&op.authority),
                meter,
            )?,
            (None, None) => record::wrap_continuation_metered(
                ord,
                &op.body,
                &op.guard,
                record::stamped(&op.authority),
                meter,
            )?,
        };
        let mut group = meter.buffer::<Par>(1)?;
        group.push(meter.copy(self.channel(coll)?)?);
        let mut colls = meter.buffer::<usize>(1)?;
        colls.push(coll);
        let mut slots = meter.buffer::<JoinSlot>(1)?;
        slots.push(JoinSlot {
            ord,
            bind: 0,
            policy: match query_policy {
                Some(policy) => policy,
                None => WaiterPolicy::of_level(lane.and_then(Lane::slot)),
            },
        });
        let mut patterns = meter.buffer::<BindPattern>(1)?;
        patterns.push(meter.copy(&op.pattern)?);
        let mut carried_by_coll = meter.buffer::<Option<i64>>(self.scope.collections.len())?;
        carried_by_coll.resize(self.scope.collections.len(), None);
        if let (Some(slot), Some(ord)) = (carried_by_coll.get_mut(coll), carried.first()) {
            *slot = *ord;
        }
        Ok(NewWaiter {
            group,
            colls,
            slots,
            patterns,
            continuation,
            persist: op.persistent,
            peeks: peek_set(op.peek, meter)?,
            carried: carried_by_coll,
        })
    }

    /// The waiter of the join `op`: its positions in canonical group order, a record of version
    /// 2 with one slot for each position.
    fn join_waiter(
        &self,
        cx: &mut Cx<'_, '_>,
        op: &JoinOp,
        own_colls: &[usize],
        carried: &[Option<i64>],
    ) -> Result<NewWaiter, SpaceError> {
        let meter = cx.meter();
        let width = self.scope.group.len();
        let mut group = meter.buffer::<Par>(width)?;
        let mut colls = meter.buffer::<usize>(width)?;
        let mut slots = meter.buffer::<JoinSlot>(width)?;
        let mut patterns = meter.buffer::<BindPattern>(width)?;
        for &position in &self.scope.group {
            let bind = op
                .binds
                .get(position)
                .ok_or(SpaceError::Inconsistent("a group position names no bind"))?;
            let coll = self.scope.index_of_key(&bind.key, meter)?;
            let lane = match self.source.shape() {
                JoinWindow::Lanes => Some(receive_lane(&bind.query, meter)?),
                JoinWindow::Head
                | JoinWindow::Scan
                | JoinWindow::Classes
                | JoinWindow::Similarity => None,
            };
            // A VectorDB bind keeps its query in its slot (P6, design v3 4.6).
            let policy = match self.source.shape() {
                JoinWindow::Similarity => vectordb::policy_of_query(bind.query.as_ref(), meter)?,
                JoinWindow::Head | JoinWindow::Lanes | JoinWindow::Scan | JoinWindow::Classes => {
                    WaiterPolicy::of_level(lane.and_then(Lane::slot))
                }
            };
            group.push(meter.copy(self.channel(coll)?)?);
            colls.push(coll);
            slots.push(JoinSlot {
                ord: self.ord_on(coll)?,
                bind: position,
                policy,
            });
            patterns.push(meter.copy(&bind.pattern)?);
        }
        let continuation = record::wrap_join_continuation_metered(
            &slots,
            &op.body,
            &op.guard,
            record::stamped(&op.authority),
            meter,
        )?;
        let peeks = match op.peek {
            true => peek_set_all(width, meter)?,
            false => BTreeSet::new(),
        };
        let mut carried_by_coll = meter.buffer::<Option<i64>>(self.scope.collections.len())?;
        carried_by_coll.resize(self.scope.collections.len(), None);
        for (&coll, &ord) in own_colls.iter().zip(carried) {
            if let Some(slot) = carried_by_coll.get_mut(coll) {
                *slot = ord;
            }
        }
        Ok(NewWaiter {
            group,
            colls,
            slots,
            patterns,
            continuation,
            persist: op.persistent,
            peeks,
            carried: carried_by_coll,
        })
    }

    /// Loads the stored waiter `at`: its copy, the collection of each position and its slots.
    fn load(&self, cx: &mut Cx<'_, '_>, at: WaiterRef) -> Result<Waiter, SpaceError> {
        let meter = cx.meter();
        let mut colls = meter.buffer::<usize>(at.group.len())?;
        for channel in &at.group {
            colls.push(self.scope.index_of_channel(channel, meter)?);
        }
        let waiting = {
            let waiters = cx.group_waiters(&at.group)?;
            let waiter = waiters.get(at.index).ok_or(SpaceError::Inconsistent(
                "a waiter index is outside the staged view",
            ))?;
            meter.copy(waiter)?
        };
        let slots = self.source.waiter_slots(&waiting, at.group.len(), meter)?;
        Ok(Waiter {
            at,
            colls,
            slots,
            waiting,
        })
    }

    /// The lanes of the positions of a waiter with the slots `slots`. The buffer is reserved.
    fn lanes(
        &self,
        slots: &[JoinSlot],
        meter: &ScanMeter,
    ) -> Result<Vec<Option<Lane>>, SpaceError> {
        let mut lanes = meter.buffer::<Option<Lane>>(slots.len())?;
        lanes.extend(slots.iter().map(|slot| match self.source.shape() {
            JoinWindow::Lanes => Some(Lane::of_slot(slot.policy.level())),
            JoinWindow::Head | JoinWindow::Scan | JoinWindow::Classes | JoinWindow::Similarity => {
                None
            }
        }));
        Ok(lanes)
    }

    /// True when the stored waiter `waiter` is the head of each head-only collection of its
    /// group (decision 11).
    fn all_at_head(&self, cx: &mut Cx<'_, '_>, waiter: &Waiter) -> Result<bool, SpaceError> {
        let mut checked: Vec<usize> = Vec::new();
        for &coll in &waiter.colls {
            if checked.contains(&coll) {
                continue;
            }
            generic::push_reserved(&mut checked, coll, cx.meter())?;
            if !self.source.at_head(
                cx,
                self.scope,
                &waiter.at,
                &waiter.slots,
                &waiter.colls,
                coll,
            )? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// The search of the positions `colls` with `patterns`, `lanes` and the slots `slots` (joins
    /// plan sections 3.4 and 3.5). `fixed` binds the element in flight at one position first. It
    /// returns one bound entry for each position, or `None`. A position on a VectorDB collection
    /// sees the records that the query of its slot makes eligible (P6, design v3 4.6).
    fn search(
        &self,
        cx: &mut Cx<'_, '_>,
        colls: &[usize],
        patterns: &[BindPattern],
        lanes: &[Option<Lane>],
        slots: &[JoinSlot],
        fixed: Option<(usize, &Incoming, ListParWithRandom)>,
    ) -> Result<Option<Vec<Bound>>, SpaceError> {
        let meter = cx.meter();
        let width = colls.len();
        if patterns.len() != width || lanes.len() != width {
            return Err(SpaceError::Inconsistent(
                "a waiter has not one pattern and one lane for each position",
            ));
        }
        let mut bound: Vec<Option<Bound>> = meter.buffer(width)?;
        bound.resize(width, None);
        let mut claimed: Vec<Claim> = meter.buffer(width)?;
        let incoming = fixed.as_ref().map(|(_, x, _)| *x);
        if let Some((position, x, matched)) = fixed {
            let claim = self
                .source
                .claim(x.coll, EntryAt::Incoming, &x.payload, meter)?;
            claimed.push(claim.clone());
            if let Some(slot) = bound.get_mut(position) {
                *slot = Some(Bound {
                    at: EntryAt::Incoming,
                    matched,
                    persist: x.persist,
                    claim,
                });
            }
        }
        let mut windows: BTreeMap<usize, Vec<Entry>> = BTreeMap::new();
        for position in 0..width {
            if bound.get(position).is_some_and(Option::is_some) {
                continue;
            }
            let coll = *colls
                .get(position)
                .ok_or(SpaceError::Inconsistent("a position has no collection"))?;
            if let Some(cfg) = &self.vdb {
                let pattern = patterns
                    .get(position)
                    .ok_or(SpaceError::Inconsistent("a position has no pattern"))?;
                let slot = slot_at(slots, position)?;
                let Some(found) =
                    self.similarity_bind(cx, cfg, coll, &slot.policy, pattern, &claimed)?
                else {
                    return Ok(None);
                };
                generic::push_reserved(&mut claimed, found.claim.clone(), meter)?;
                if let Some(entry) = bound.get_mut(position) {
                    *entry = Some(found);
                }
                continue;
            }
            meter.ordered_lookup(windows.len(), size_of::<usize>())?;
            if !windows.contains_key(&coll) {
                let need = colls.iter().filter(|&&other| other == coll).count();
                let window = self
                    .source
                    .window(cx, self.scope, coll, need, incoming, &self.work)?;
                meter.tree_growth::<usize, Vec<Entry>>(windows.len(), 1)?;
                windows.insert(coll, window);
            }
            let window = windows
                .get(&coll)
                .ok_or(SpaceError::Inconsistent("a window is missing"))?;
            let lane = *lanes
                .get(position)
                .ok_or(SpaceError::Inconsistent("a position has no lane"))?;
            let pattern = patterns
                .get(position)
                .ok_or(SpaceError::Inconsistent("a position has no pattern"))?;
            let mut found: Option<Bound> = None;
            for entry in window.iter() {
                let EntryAt::Stored(index) = entry.at else {
                    continue;
                };
                if !visible(lane, entry.level)? {
                    continue;
                }
                let stored = self.source.stored_entry(
                    cx,
                    self.scope,
                    coll,
                    index,
                    "a window index is outside the staged view",
                )?;
                if is_claimed(&claimed, &stored.claim, meter)? {
                    continue;
                }
                if let Some(matched) = cx.bind(pattern, &stored.payload)? {
                    found = Some(Bound {
                        at: entry.at,
                        matched,
                        persist: stored.persist,
                        claim: stored.claim,
                    });
                    break;
                }
            }
            let Some(found) = found else {
                return Ok(None);
            };
            generic::push_reserved(&mut claimed, found.claim.clone(), meter)?;
            if let Some(slot) = bound.get_mut(position) {
                *slot = Some(found);
            }
        }
        Ok(bound.into_iter().collect())
    }

    /// The guard of `continuation` over the values of `bound` in the bind order of `slots`
    /// (decision 37: once, after every bind matched).
    fn admits(
        &self,
        cx: &mut Cx<'_, '_>,
        continuation: &TaggedContinuation,
        slots: &[JoinSlot],
        bound: &[Bound],
    ) -> Result<bool, SpaceError> {
        let meter = cx.meter();
        let mut by_bind: Vec<Option<usize>> = meter.buffer(bound.len())?;
        by_bind.resize(bound.len(), None);
        for (position, slot) in slots.iter().enumerate() {
            let place = by_bind.get_mut(slot.bind).ok_or(SpaceError::Inconsistent(
                "a bind index is outside the binds",
            ))?;
            *place = Some(position);
        }
        let mut values = meter.buffer::<ListParWithRandom>(bound.len())?;
        for position in by_bind {
            let position = position.ok_or(SpaceError::Inconsistent("a bind has no position"))?;
            let entry = bound
                .get(position)
                .ok_or(SpaceError::Inconsistent("a position has no bound entry"))?;
            values.push(meter.copy(&entry.matched)?);
        }
        cx.admits_all(continuation, &values)
    }

    /// SEARCH-PRODUCE (joins plan section 3.5): the fire of the stored waiter `waiter` with the
    /// element in flight `x` at one position, and the position. The waiter must be the head of
    /// each head-only collection of its group, and `x` must lie in the window of its collection.
    fn search_produce(
        &self,
        cx: &mut Cx<'_, '_>,
        waiter: &Waiter,
        x: &Incoming,
        x_vector: Option<&ExactVector>,
    ) -> Result<Option<(usize, Vec<Bound>)>, SpaceError> {
        if !self.all_at_head(cx, waiter)? {
            return Ok(None);
        }
        let meter = cx.meter();
        let lanes = self.lanes(&waiter.slots, meter)?;
        // Whether `x` lies in the window of each collection that it enters, computed at the first
        // position on the collection. A keyed element enters its own collection only, so it
        // asks for one window, as the search of P5c always did. The list of the other
        // collections is reserved when a second collection is entered.
        let mut first: Option<(usize, bool)> = None;
        let mut others: Vec<(usize, bool)> = Vec::new();
        for position in 0..waiter.colls.len() {
            let coll = *waiter
                .colls
                .get(position)
                .ok_or(SpaceError::Inconsistent("a position has no collection"))?;
            if !self.source.enters(self.scope, x, coll, meter)? {
                continue;
            }
            // P6: `x` enters a VectorDB position when the query of the position makes it
            // eligible, and the position binds the value that its query delivers.
            if let Some(cfg) = &self.vdb {
                let pattern = waiter
                    .waiting
                    .patterns
                    .get(position)
                    .ok_or(SpaceError::Inconsistent("a position has no pattern"))?;
                let slot = slot_at(&waiter.slots, position)?;
                let Some(matched) =
                    self.similarity_flight_bind(cx, cfg, &slot.policy, pattern, x, x_vector)?
                else {
                    continue;
                };
                if let Some(bound) = self.search(
                    cx,
                    &waiter.colls,
                    &waiter.waiting.patterns,
                    &lanes,
                    &waiter.slots,
                    Some((position, x, matched)),
                )? {
                    return Ok(Some((position, bound)));
                }
                continue;
            }
            let known = match first {
                Some((seen, in_window)) if seen == coll => Some(in_window),
                Some(_) => {
                    let mut known = None;
                    for &(seen, in_window) in &others {
                        meter.compare(size_of::<usize>(), size_of::<usize>())?;
                        if seen == coll {
                            known = Some(in_window);
                            break;
                        }
                    }
                    known
                }
                None => None,
            };
            let in_window = match known {
                Some(in_window) => in_window,
                None => {
                    let need = waiter.colls.iter().filter(|&&other| other == coll).count();
                    let window =
                        self.source
                            .window(cx, self.scope, coll, need, Some(x), &self.work)?;
                    let in_window = window.iter().any(|entry| entry.at == EntryAt::Incoming);
                    match first {
                        None => first = Some((coll, in_window)),
                        Some(_) => generic::push_reserved(&mut others, (coll, in_window), meter)?,
                    }
                    in_window
                }
            };
            if !in_window {
                continue;
            }
            let lane = *lanes
                .get(position)
                .ok_or(SpaceError::Inconsistent("a position has no lane"))?;
            if !visible(lane, x.level)? {
                continue;
            }
            let pattern = waiter
                .waiting
                .patterns
                .get(position)
                .ok_or(SpaceError::Inconsistent("a position has no pattern"))?;
            let payload = self.source.incoming_payload(self.scope, x, coll, meter)?;
            let Some(matched) = cx.bind(pattern, &payload)? else {
                continue;
            };
            if let Some(bound) = self.search(
                cx,
                &waiter.colls,
                &waiter.waiting.patterns,
                &lanes,
                &waiter.slots,
                Some((position, x, matched)),
            )? {
                return Ok(Some((position, bound)));
            }
        }
        Ok(None)
    }

    /// The first stored record of the VectorDB collection `coll` that a bind with the policy
    /// `policy` and the pattern `pattern` binds, skipping the records of `claimed` (P6, design v3
    /// 4.6): in the selection order of the query of the policy, with the value that the query
    /// delivers, or in canonical order without a query. A list-mode bind takes the list paths of
    /// the engine, never this search.
    fn similarity_bind(
        &self,
        cx: &mut Cx<'_, '_>,
        cfg: &VdbConfig,
        coll: usize,
        policy: &WaiterPolicy,
        pattern: &BindPattern,
        claimed: &[Claim],
    ) -> Result<Option<Bound>, SpaceError> {
        let meter = cx.meter();
        let channel = self.channel(coll)?;
        let query = vectordb::policy_query(cfg, policy, meter)?;
        let entries: Vec<(usize, Option<Score>)> = match &query {
            None => {
                let order = cx.canonical_data_order(channel)?;
                let mut entries = meter.buffer::<(usize, Option<Score>)>(order.len())?;
                entries.extend(order.into_iter().map(|index| (index, None)));
                entries
            }
            Some(query) => match query.mode {
                Mode::List(_) => {
                    return Err(SpaceError::Inconsistent(
                        "a list-mode bind reached the search of the engine",
                    ))
                }
                Mode::Single => {
                    let eligible = vectordb::eligible(cx, cfg, query, channel, None)?;
                    let mut entries = meter.buffer::<(usize, Option<Score>)>(eligible.len())?;
                    for entry in eligible {
                        let index = entry.index.ok_or(SpaceError::Inconsistent(
                            "a search met an element in flight",
                        ))?;
                        entries.push((index, entry.score));
                    }
                    entries
                }
            },
        };
        for (index, score) in entries {
            let stored = self.source.stored_entry(
                cx,
                self.scope,
                coll,
                index,
                "a window index is outside the staged view",
            )?;
            if is_claimed(claimed, &stored.claim, meter)? {
                continue;
            }
            let data = match &query {
                Some(query) => Cow::Owned(vectordb::single_data(
                    query,
                    &stored.payload,
                    score.as_ref(),
                    meter,
                )?),
                None => Cow::Borrowed(&stored.payload),
            };
            if let Some(matched) = cx.bind(pattern, &data)? {
                return Ok(Some(Bound {
                    at: EntryAt::Stored(index),
                    matched,
                    persist: stored.persist,
                    claim: stored.claim,
                }));
            }
        }
        Ok(None)
    }

    /// The values that the element in flight `x`, with the vector `x_vector`, binds at a VectorDB
    /// position with the policy `policy` and the pattern `pattern` (P6, design v3 4.6), or `None`
    /// when the query of the policy does not make it eligible or the pattern refuses its value.
    fn similarity_flight_bind(
        &self,
        cx: &mut Cx<'_, '_>,
        cfg: &VdbConfig,
        policy: &WaiterPolicy,
        pattern: &BindPattern,
        x: &Incoming,
        x_vector: Option<&ExactVector>,
    ) -> Result<Option<ListParWithRandom>, SpaceError> {
        let meter = cx.meter();
        let Some(query) = vectordb::policy_query(cfg, policy, meter)? else {
            return cx.bind(pattern, &x.payload);
        };
        if let Mode::List(_) = query.mode {
            return Err(SpaceError::Inconsistent(
                "a list-mode bind reached the search of the engine",
            ));
        }
        let flight = Flight {
            record: &x.record,
            payload: &x.payload,
            source: &x.source,
            persist: x.persist,
            vector: x_vector,
        };
        match vectordb::flight_eligibility(&query, &flight, meter)? {
            None => Ok(None),
            Some(score) => {
                let data = vectordb::single_data(&query, &x.payload, score.as_ref(), meter)?;
                cx.bind(pattern, &data)
            }
        }
    }

    /// The query and the length of the list of the stored waiter `waiter` when it is a list
    /// waiter of a VectorDB collection: one bind whose policy holds a list-mode query (decision
    /// 66, a list waiter always parks on `[D]`). Every other waiter is `None`.
    fn list_waiter(
        &self,
        cx: &mut Cx<'_, '_>,
        slots: &[JoinSlot],
    ) -> Result<Option<(vectordb::VdbQuery, Option<usize>)>, SpaceError> {
        let (Some(cfg), [slot]) = (&self.vdb, slots) else {
            return Ok(None);
        };
        match vectordb::policy_query(cfg, &slot.policy, cx.meter())? {
            Some(query) => match query.mode {
                Mode::List(k) => Ok(Some((query, k))),
                Mode::Single => Ok(None),
            },
            None => Ok(None),
        }
    }

    /// The candidates of a fire of the group of `colls` from `bound`, in group order: the
    /// incoming candidate of `x` at its position, and a copy of each stored record with the
    /// values that it binds. `shift` is the index of a record that a lift removed from the
    /// collection `shift.0` before the fire: the later records of that collection moved down by
    /// one (`SpaceTxn::remove_records`).
    fn candidates(
        &self,
        cx: &mut Cx<'_, '_>,
        colls: &[usize],
        bound: Vec<Bound>,
        x: Option<&Incoming>,
        shift: Option<(usize, usize)>,
    ) -> Result<Vec<ParCandidate>, SpaceError> {
        let meter = cx.meter();
        let mut candidates = meter.buffer::<ParCandidate>(bound.len())?;
        for (entry, &coll) in bound.into_iter().zip(colls) {
            let channel = meter.copy(self.channel(coll)?)?;
            match entry.at {
                EntryAt::Incoming => {
                    let x = x.ok_or(SpaceError::Inconsistent(
                        "a fire binds an element in flight, but none is in flight",
                    ))?;
                    let source = self.source.incoming_source(cx, self.scope, x, coll)?;
                    let record = self.source.incoming_record(cx, self.scope, x, coll)?;
                    candidates.push(incoming_candidate(
                        channel,
                        source,
                        record,
                        x.persist,
                        entry.matched,
                    ));
                }
                EntryAt::Stored(index) => {
                    let index = match shift {
                        Some((removed_coll, removed))
                            if removed_coll == coll && index > removed =>
                        {
                            index - 1
                        }
                        Some(_) | None => index,
                    };
                    let head = {
                        let records = cx.data_on(self.channel(coll)?)?;
                        let datum = records.get(index).ok_or(SpaceError::Inconsistent(
                            "a bound index is outside the staged view",
                        ))?;
                        meter.copy(datum)?
                    };
                    let view_index = i32::try_from(index)
                        .map_err(|_| SpaceError::Inconsistent("a record index exceeds i32"))?;
                    candidates.push(receive_candidate(channel, ReceiveFire {
                        head,
                        matched: entry.matched,
                        view_index,
                    }));
                }
            }
        }
        Ok(candidates)
    }

    /// The second sends of a peek fire (decision 45, Rule E), in receipt order: each stored
    /// record that is not persistent, and the element in flight `x`. They are pushed in reverse,
    /// so the last pushed runs first. The records are copied before the fire removes them. The
    /// entry at `incoming_at` is the element in flight, also when the search found it stored (the
    /// record that the cascade re-offers). An element in flight is sent again whether or not it
    /// is persistent, because it is stored nowhere (decision 30, as `generic::offer_head`
    /// continues with it).
    fn second_sends(
        &self,
        cx: &mut Cx<'_, '_>,
        colls: &[usize],
        bound: &[Bound],
        x: Option<&Incoming>,
        incoming_at: Option<usize>,
    ) -> Result<Vec<Work>, SpaceError> {
        let meter = cx.meter();
        let mut sends = meter.buffer::<Work>(bound.len())?;
        for (position, (entry, &coll)) in bound.iter().zip(colls).enumerate().rev() {
            let at = match incoming_at == Some(position) {
                true => EntryAt::Incoming,
                false => entry.at,
            };
            match at {
                EntryAt::Incoming => {
                    let x = x.ok_or(SpaceError::Inconsistent(
                        "a fire binds an element in flight, but none is in flight",
                    ))?;
                    if x.in_flight {
                        sends.push(Work {
                            coll: x.coll,
                            record: meter.copy(&x.record)?,
                            persist: x.persist,
                            own: x.own,
                            restore: x.restore,
                        });
                    }
                }
                EntryAt::Stored(index) => {
                    if let Some(send) = self
                        .source
                        .second_send(cx, self.scope, coll, index, entry)?
                    {
                        sends.push(send);
                    }
                }
            }
        }
        Ok(sends)
    }

    /// Pushes the second sends `sends` onto the work stack.
    fn push_work(&mut self, sends: Vec<Work>, meter: &ScanMeter) -> Result<(), SpaceError> {
        for send in sends {
            generic::push_reserved(&mut self.work, send, meter)?;
        }
        Ok(())
    }

    /// The element of the work item `work` in flight: its slots, its payload, its identity, and
    /// its introduction. The element of the operation keeps the introduction and the placement of
    /// the operation. A second send registers the record's own authority
    /// (`Introduction::for_record`) and takes its place by its carried order.
    fn incoming_of(&self, cx: &mut Cx<'_, '_>, work: Work) -> Result<Incoming, SpaceError> {
        let meter = cx.meter();
        let (ord, level) = self.source.record_slots(&work.record, meter)?;
        let payload = self.source.payload_of(&work.record, meter)?;
        let source = Produce::create_metered(
            self.source.home(self.scope, work.coll)?,
            &work.record,
            work.persist,
            meter,
        )?;
        let (introduction, placement) = match work.own {
            true => {
                meter.shared(1)?;
                (Arc::clone(cx.introduction()), self.own_placement)
            }
            false => (
                cx.introduction().for_record(&work.record, meter)?,
                Placement::Carried,
            ),
        };
        Ok(Incoming {
            coll: work.coll,
            record: work.record,
            payload,
            source,
            persist: work.persist,
            ord,
            level,
            placement,
            introduction,
            own: work.own,
            in_flight: true,
            restore: work.restore,
        })
    }

    /// STORE (joins plan section 4.5): the element in flight `x` is stored on its collection,
    /// after the capacity check (F15) and its touch.
    fn store(&mut self, cx: &mut Cx<'_, '_>, x: Incoming) -> Result<(), SpaceError> {
        // Replaced by P6 (design v3 4.0): the bound of the data side under the config.
        // if let Some(capacity) = self.source.capacity() {
        if let Some(bound) = self.source.bound(cx.config(), Side::Data, cx.meter())? {
            let held = self.source.held(cx, self.scope, &x)?;
            cx.meter().compare(size_of::<usize>(), size_of::<usize>())?;
            if held >= bound.limit() {
                return Err(bound.full());
            }
        }
        self.touch(cx, x.coll)?;
        let own = x.own;
        if x.restore.is_none() {
            self.changed = true;
        }
        self.source.land(cx, self.scope, x, &mut self.dirty)?;
        if own {
            self.own = Own::Settled;
        }
        Ok(())
    }

    /// OFFER (joins plan section 4.5): the element in flight `x` meets the waiters of its
    /// collection in the order of the source. The first waiter whose search and guard pass
    /// fires. On a head-only collection the head blocks: a refusal stores `x`. Otherwise a refusal
    /// moves on to the next waiter (decisions 37 and 38).
    fn offer(&mut self, cx: &mut Cx<'_, '_>, mut x: Incoming) -> Result<(), SpaceError> {
        let meter = cx.meter();
        // P6: the vector of a VectorDB element, read once for every waiter that it meets.
        let x_vector = match &self.vdb {
            Some(cfg) => vectordb::record_vector(&x.record, cfg, meter)?,
            None => None,
        };
        loop {
            let order = self.source.offer_order(cx, self.scope, &x)?;
            let mut fire: Option<(Waiter, usize, Vec<Bound>, Option<Vec<ParDatum>>)> = None;
            for at in order {
                let waiter = self.load(cx, at)?;
                let found = match self.list_waiter(cx, &waiter.slots)? {
                    Some((query, k)) => {
                        self.offer_list(cx, &waiter, &x, x_vector.as_ref(), &query, k)?
                    }
                    None => match self.search_produce(cx, &waiter, &x, x_vector.as_ref())? {
                        Some((position, bound)) => {
                            match self.admits(
                                cx,
                                &waiter.waiting.continuation,
                                &waiter.slots,
                                &bound,
                            )? {
                                true => Some((position, bound, None)),
                                false => None,
                            }
                        }
                        None => None,
                    },
                };
                match found {
                    Some((position, bound, others)) => {
                        fire = Some((waiter, position, bound, others));
                        break;
                    }
                    None => match self.source.refusal_blocks() {
                        true => break,
                        false => continue,
                    },
                }
            }
            let Some((waiter, position, bound, list_others)) = fire else {
                return self.store(cx, x);
            };
            let fired_coll = *waiter
                .colls
                .get(position)
                .ok_or(SpaceError::Inconsistent("a position has no collection"))?;
            self.touch_all(cx, &waiter.colls)?;
            let peeks = !waiter.waiting.peeks.is_empty();
            let spent = !x.persist && !peeks;
            let stays = waiter.waiting.persist;
            // A stored waiter that is not persistent leaves. Without a peek, the fire spends the
            // stored entries that are not persistent, and an element that is still stored (a
            // second send or a lifted element of a PathMap space).
            if !stays
                || (!peeks
                    && ((spent && x.restore.is_some())
                        || bound
                            .iter()
                            .any(|entry| matches!(entry.at, EntryAt::Stored(_)) && !entry.persist)))
            {
                self.changed = true;
            }
            let sends = match peeks {
                true => self.second_sends(cx, &waiter.colls, &bound, Some(&x), None)?,
                false => Vec::new(),
            };
            let fired = match peeks {
                true => None,
                false => Some(self.source.before_fire(
                    cx,
                    self.scope,
                    FireSite::Offer,
                    &waiter.colls,
                    &bound,
                    &[],
                    Some(&x),
                )?),
            };
            let colls = meter.copy_slice(&waiter.colls)?;
            let channel = self.channel(fired_coll)?;
            let step = cx.rounds().after_fire(spent, stays);
            let candidates = self.candidates(cx, &waiter.colls, bound, Some(&x), None)?;
            let group = meter.copy(&waiter.at.group)?;
            let data = self
                .source
                .incoming_record(cx, self.scope, &x, fired_coll)?;
            cx.fire_offer_group(
                channel,
                data,
                x.persist,
                waiter.waiting,
                group,
                candidates,
                &x.introduction,
            )?;
            self.mark_dirty(&colls);
            if peeks {
                self.push_work(sends, meter)?;
                return Ok(());
            }
            if let Some(fired) = fired {
                let element_random = Blake2b512Random::from_bytes(&x.record.random_state);
                self.source.retire_spent(
                    cx,
                    self.scope,
                    fired,
                    &colls,
                    &element_random,
                    x.record.cost_authority.as_ref(),
                    &mut self.dirty,
                )?;
            }
            // P6 (decision 66): a list fire retires the other non-persistent members of its list.
            if let Some(others) = list_others {
                if vectordb::retire_members(cx, channel, others, &x.record)? {
                    self.changed = true;
                    self.mark_dirty(&colls);
                }
            }
            match step {
                FireStep::Done => {
                    if x.own {
                        self.own = Own::Settled;
                    }
                    return Ok(());
                }
                FireStep::Continue => continue,
                FireStep::Reissue(cause) => {
                    match x.own {
                        true => {
                            self.own = Own::InFlight;
                            self.own_cause = Some(cause);
                            if cause == ReissueCause::RoundCap {
                                self.stop = Some(cause);
                            }
                        }
                        false => {
                            // A second send that is still in flight goes back to the work stack,
                            // so it is never lost (decision 49 carries it).
                            let send = Work {
                                coll: x.coll,
                                record: std::mem::take(&mut x.record),
                                persist: x.persist,
                                own: false,
                                restore: x.restore,
                            };
                            generic::push_reserved(&mut self.work, send, meter)?;
                            self.stop = Some(cause);
                        }
                    }
                    return Ok(());
                }
            }
        }
    }

    /// OFFER-LIST (P6, decision 66): the list waiter `waiter`, with the list-mode query `query`
    /// and the length `k`, against the element `x` with the vector `x_vector`. When `x` is in the
    /// list `L(W, R + x)`, the pattern binds the list and the guard admits it, the position 0, its
    /// bound entry (the element in flight, with the values of the list) and the other stored
    /// members of the list, which a fire without a peek retires.
    #[allow(clippy::type_complexity)]
    fn offer_list(
        &self,
        cx: &mut Cx<'_, '_>,
        waiter: &Waiter,
        x: &Incoming,
        x_vector: Option<&ExactVector>,
        query: &vectordb::VdbQuery,
        k: Option<usize>,
    ) -> Result<Option<(usize, Vec<Bound>, Option<Vec<ParDatum>>)>, SpaceError> {
        let meter = cx.meter();
        let cfg = self.vdb.as_ref().ok_or(SpaceError::Inconsistent(
            "a list waiter outside a VectorDB space",
        ))?;
        let coll = *waiter
            .colls
            .first()
            .ok_or(SpaceError::Inconsistent("a list waiter has no collection"))?;
        if coll != x.coll {
            return Ok(None);
        }
        let pattern = waiter
            .waiting
            .patterns
            .first()
            .ok_or(SpaceError::Inconsistent("a list waiter has no pattern"))?;
        let flight = Flight {
            record: &x.record,
            payload: &x.payload,
            source: &x.source,
            persist: x.persist,
            vector: x_vector,
        };
        let channel = self.channel(coll)?;
        let Some((matched, others)) =
            vectordb::list_offer(cx, cfg, query, k, pattern, channel, &flight)?
        else {
            return Ok(None);
        };
        if !cx.admits(&waiter.waiting.continuation, &matched)? {
            return Ok(None);
        }
        let claim = self
            .source
            .claim(x.coll, EntryAt::Incoming, &x.payload, meter)?;
        let mut bound = meter.buffer::<Bound>(1)?;
        bound.push(Bound {
            at: EntryAt::Incoming,
            matched,
            persist: x.persist,
            claim,
        });
        Ok(Some((0, bound, Some(others))))
    }

    /// The candidates of a single-bind consume fire on a set: the representative `bound` and the
    /// other stored members of its class in canonical order (F3, F20). Each member binds the
    /// pattern with its own values. It returns the candidates and whether a member is persistent.
    fn class_candidates(
        &self,
        cx: &mut Cx<'_, '_>,
        coll: usize,
        pattern: &BindPattern,
        representative: Bound,
    ) -> Result<(Vec<ParCandidate>, bool), SpaceError> {
        let meter = cx.meter();
        let EntryAt::Stored(rep_index) = representative.at else {
            return Err(SpaceError::Inconsistent(
                "a consume binds an element in flight",
            ));
        };
        let class = representative.claim.clone();
        let mut persistent = representative.persist;
        let mut candidates = Vec::new();
        let rep = self.candidates(cx, &[coll], vec![representative], None, None)?;
        for candidate in rep {
            generic::push_reserved(&mut candidates, candidate, meter)?;
        }
        if !matches!(class, Claim::Class { .. }) {
            return Ok((candidates, persistent));
        }
        let order = self.source.members(cx, self.scope, coll)?;
        for index in order {
            meter.compare(size_of::<usize>(), size_of::<usize>())?;
            if index == rep_index {
                continue;
            }
            let stored = self.source.stored_entry(
                cx,
                self.scope,
                coll,
                index,
                "a canonical index is outside the staged view",
            )?;
            if !is_claimed(std::slice::from_ref(&class), &stored.claim, meter)? {
                continue;
            }
            let matched = cx
                .bind(pattern, &stored.payload)?
                .ok_or(SpaceError::Inconsistent(
                    "a member of a class does not bind the pattern of its representative",
                ))?;
            persistent = persistent || stored.persist;
            let member = self.candidates(
                cx,
                &[coll],
                vec![Bound {
                    at: EntryAt::Stored(index),
                    matched,
                    persist: stored.persist,
                    claim: stored.claim,
                }],
                None,
                None,
            )?;
            for candidate in member {
                generic::push_reserved(&mut candidates, candidate, meter)?;
            }
        }
        Ok((candidates, persistent))
    }

    /// RECEIVE (joins plan section 4.4): the waiter `w` of the operation fires from the windows
    /// of its collections when it reaches the head of each head-only collection and the search
    /// and the guard pass. Otherwise it parks. A fire that ends the loop moves the continuation,
    /// the group, the patterns and the peek set into the staged consume, and a fire that
    /// continues the loop (a persistent waiter whose partner left) copies them.
    fn receive(&mut self, cx: &mut Cx<'_, '_>, mut w: NewWaiter) -> Result<(), SpaceError> {
        // P6 (decision 66): a list-mode receive binds one list from the records of its one bind.
        if let Some((query, k)) = self.list_waiter(cx, &w.slots)? {
            return self.receive_list(cx, w, &query, k);
        }
        let meter = cx.meter();
        let lanes = self.lanes(&w.slots, meter)?;
        let width = w.colls.len();
        loop {
            let mut reaches = true;
            let mut checked: Vec<usize> = Vec::new();
            for &coll in &w.colls {
                if checked.contains(&coll) {
                    continue;
                }
                generic::push_reserved(&mut checked, coll, meter)?;
                let carried = w.carried.get(coll).copied().flatten();
                if !self
                    .source
                    .at_head_incoming(cx, self.scope, &w.slots, &w.colls, coll, carried)?
                {
                    reaches = false;
                    break;
                }
            }
            if !reaches {
                return self.park(cx, w);
            }
            let Some(bound) = self.search(cx, &w.colls, &w.patterns, &lanes, &w.slots, None)?
            else {
                return self.park(cx, w);
            };
            if !self.admits(cx, &w.continuation, &w.slots, &bound)? {
                return self.park(cx, w);
            }
            let peek = !w.peeks.is_empty();
            let sends = match peek {
                true => self.second_sends(cx, &w.colls, &bound, None, None)?,
                false => Vec::new(),
            };
            let spent = !w.persist;
            match width {
                1 => {
                    let coll = *w
                        .colls
                        .first()
                        .ok_or(SpaceError::Inconsistent("a consume has no collection"))?;
                    let pattern = meter.copy(
                        w.patterns
                            .first()
                            .ok_or(SpaceError::Inconsistent("a consume has no pattern"))?,
                    )?;
                    let representative = bound
                        .into_iter()
                        .next()
                        .ok_or(SpaceError::Inconsistent("a consume binds no entry"))?;
                    // A peek binds the representative alone (Rule E sends it again). Otherwise the
                    // fire binds the class of the representative over `[D; n]` (F3, F20).
                    let (candidates, persistent) = match peek {
                        true => {
                            let persist = representative.persist;
                            (
                                self.candidates(cx, &w.colls, vec![representative], None, None)?,
                                persist,
                            )
                        }
                        false => self.class_candidates(cx, coll, &pattern, representative)?,
                    };
                    let step = cx.rounds().after_fire(spent, peek || persistent);
                    record::prepay_continuation_unwrap(&w.continuation, meter)?;
                    let members = candidates.len();
                    let mut channels = meter.buffer::<Par>(members)?;
                    let mut patterns = meter.buffer::<BindPattern>(members)?;
                    for _ in 0..members {
                        channels.push(meter.copy(self.channel(coll)?)?);
                        patterns.push(meter.copy(&pattern)?);
                    }
                    let ends = !matches!(step, FireStep::Continue);
                    let (continuation, peeks) = match ends {
                        true => (
                            std::mem::take(&mut w.continuation),
                            std::mem::take(&mut w.peeks),
                        ),
                        false => (meter.copy(&w.continuation)?, meter.copy(&w.peeks)?),
                    };
                    if !peek && candidates.iter().any(|candidate| !candidate.datum.persist) {
                        self.changed = true;
                    }
                    let fired = match peek {
                        true => None,
                        false => Some(self.source.before_fire(
                            cx,
                            self.scope,
                            FireSite::Single,
                            &w.colls,
                            &[],
                            &candidates,
                            None,
                        )?),
                    };
                    cx.fire_receive_group(
                        channels,
                        patterns,
                        continuation,
                        w.persist,
                        peeks,
                        candidates,
                        1,
                    )?;
                    self.mark_dirty(&w.colls);
                    if peek {
                        self.push_work(sends, meter)?;
                    }
                    if let Some(fired) = fired {
                        self.source.retire_spent(
                            cx,
                            self.scope,
                            fired,
                            &w.colls,
                            &self.random,
                            self.authority.as_ref(),
                            &mut self.dirty,
                        )?;
                    }
                    if !self.after_receive(step) {
                        return Ok(());
                    }
                }
                _ => {
                    let stays = peek || bound.iter().all(|entry| entry.persist);
                    if !stays {
                        self.changed = true;
                    }
                    let step = cx.rounds().after_fire(spent, stays);
                    let retire = match peek {
                        true => None,
                        false => Some(self.source.before_fire(
                            cx,
                            self.scope,
                            FireSite::Join,
                            &w.colls,
                            &bound,
                            &[],
                            None,
                        )?),
                    };
                    let candidates = self.candidates(cx, &w.colls, bound, None, None)?;
                    let ends = !matches!(step, FireStep::Continue);
                    let (channels, patterns, continuation, peeks) = match ends {
                        true => (
                            std::mem::take(&mut w.group),
                            std::mem::take(&mut w.patterns),
                            std::mem::take(&mut w.continuation),
                            std::mem::take(&mut w.peeks),
                        ),
                        false => (
                            meter.copy(&w.group)?,
                            meter.copy(&w.patterns)?,
                            meter.copy(&w.continuation)?,
                            meter.copy(&w.peeks)?,
                        ),
                    };
                    cx.fire_join(
                        channels,
                        patterns,
                        continuation,
                        w.persist,
                        peeks,
                        candidates,
                    )?;
                    self.mark_dirty(&w.colls);
                    match retire {
                        None => self.push_work(sends, meter)?,
                        Some(fired) => {
                            let random = self.random.clone();
                            let authority = self.authority.clone();
                            self.source.retire_spent(
                                cx,
                                self.scope,
                                fired,
                                &w.colls,
                                &random,
                                authority.as_ref(),
                                &mut self.dirty,
                            )?;
                        }
                    }
                    if !self.after_receive(step) {
                        return Ok(());
                    }
                }
            }
        }
    }

    /// RECEIVE-LIST (P6, decision 66): the list-mode waiter `w` of the operation, with the query
    /// `query` and the length `k`, fires over `[D; m]` when its list `L(W, R)` is not empty, its
    /// pattern binds the list and its guard admits it. The outcome dispatches the result of the
    /// first member, which carries the values of the list, and each other member carries its own
    /// payload (arbitration A11). A peek removes the non-persistent members, and the transaction
    /// sends them again in delivery order (Rule E). Otherwise the waiter parks with its query.
    fn receive_list(
        &mut self,
        cx: &mut Cx<'_, '_>,
        mut w: NewWaiter,
        query: &vectordb::VdbQuery,
        k: Option<usize>,
    ) -> Result<(), SpaceError> {
        let meter = cx.meter();
        let cfg = self.vdb.ok_or(SpaceError::Inconsistent(
            "a list waiter outside a VectorDB space",
        ))?;
        let coll = *w
            .colls
            .first()
            .ok_or(SpaceError::Inconsistent("a consume has no collection"))?;
        let channel = self.channel(coll)?;
        loop {
            let pattern = w
                .patterns
                .first()
                .ok_or(SpaceError::Inconsistent("a consume has no pattern"))?;
            let Some(ListFire { matched, members }) =
                vectordb::list_selection(cx, &cfg, query, k, pattern, channel, None)?
            else {
                return self.park(cx, w);
            };
            if !cx.admits(&w.continuation, &matched)? {
                return self.park(cx, w);
            }
            let pattern = meter.copy(pattern)?;
            let width = members.len();
            let mut records = meter.buffer::<(ParDatum, usize)>(width)?;
            for member in members {
                records.push(member.ok_or(SpaceError::Inconsistent(
                    "a receive met an element in flight",
                ))?);
            }
            let peek = !w.peeks.is_empty();
            // Rule E (decision 45): the second sends of the non-persistent members, pushed in
            // reverse, so the first member runs first.
            let mut sends = meter.buffer::<Work>(width)?;
            if peek {
                for (record, _) in records.iter().rev() {
                    if !record.persist {
                        sends.push(Work {
                            coll,
                            record: meter.copy(&record.a)?,
                            persist: false,
                            own: false,
                            restore: None,
                        });
                    }
                }
            }
            let spent = !w.persist;
            let persistent = records.iter().any(|(record, _)| record.persist);
            let step = cx.rounds().after_fire(spent, peek || persistent);
            record::prepay_continuation_unwrap(&w.continuation, meter)?;
            if !peek && records.iter().any(|(record, _)| !record.persist) {
                self.changed = true;
            }
            let mut channels = meter.buffer::<Par>(width)?;
            let mut patterns = meter.buffer::<BindPattern>(width)?;
            let mut candidates = meter.buffer::<ParCandidate>(width)?;
            let mut first = Some(matched);
            for (record, index) in records {
                let matched = match first.take() {
                    Some(matched) => matched,
                    None => vectordb::payload_of(&record.a, meter)?,
                };
                let view_index = i32::try_from(index)
                    .map_err(|_| SpaceError::Inconsistent("a record index exceeds i32"))?;
                channels.push(meter.copy(channel)?);
                patterns.push(meter.copy(&pattern)?);
                candidates.push(receive_candidate(meter.copy(channel)?, ReceiveFire {
                    head: record,
                    matched,
                    view_index,
                }));
            }
            let peeks = match peek {
                true => peek_set_all(width, meter)?,
                false => BTreeSet::new(),
            };
            let continuation = match step {
                FireStep::Continue => meter.copy(&w.continuation)?,
                FireStep::Done | FireStep::Reissue(_) => std::mem::take(&mut w.continuation),
            };
            cx.fire_receive_group(
                channels,
                patterns,
                continuation,
                w.persist,
                peeks,
                candidates,
                1,
            )?;
            self.mark_dirty(&w.colls);
            self.push_work(sends, meter)?;
            if !self.after_receive(step) {
                return Ok(());
            }
        }
    }

    /// The state after a consume-side fire of the waiter of the operation. It returns `true`
    /// when the waiter fires again: a persistent waiter whose partner left. A divergent pair and
    /// the round cap leave the waiter in flight, and the round cap stops the transaction.
    fn after_receive(&mut self, step: FireStep) -> bool {
        match step {
            FireStep::Done => {
                self.own = Own::Settled;
                false
            }
            FireStep::Continue => true,
            FireStep::Reissue(cause) => {
                self.own = Own::InFlight;
                self.own_cause = Some(cause);
                if cause == ReissueCause::RoundCap {
                    self.stop = Some(cause);
                }
                false
            }
        }
    }

    /// PARK (joins plan section 4.4): the waiter of the operation parks on its group, after the
    /// capacity check of each of its collections (F15) and their touch.
    fn park(&mut self, cx: &mut Cx<'_, '_>, w: NewWaiter) -> Result<(), SpaceError> {
        // Replaced by P6 (design v3 4.0): the bound of the waiter side under the config.
        // if let Some(capacity) = self.source.capacity() {
        if let Some(bound) = self.source.bound(cx.config(), Side::Waiters, cx.meter())? {
            let mut checked: Vec<usize> = Vec::new();
            for &coll in &w.colls {
                if checked.contains(&coll) {
                    continue;
                }
                generic::push_reserved(&mut checked, coll, cx.meter())?;
                let held = self.source.waiter_count(cx, self.scope, coll)?;
                cx.meter().compare(size_of::<usize>(), size_of::<usize>())?;
                if held >= bound.limit() {
                    return Err(bound.full());
                }
            }
        }
        self.touch_all(cx, &w.colls)?;
        let NewWaiter {
            group,
            patterns,
            continuation,
            persist,
            peeks,
            ..
        } = w;
        cx.park_on(group, patterns, continuation, persist, peeks)?;
        self.changed = true;
        self.own = Own::Settled;
        Ok(())
    }

    /// NEXT-FIRE (joins plan section 4.9): the oldest fireable head of the least dirty
    /// collection, with its search.
    fn next_fire(&self, cx: &mut Cx<'_, '_>) -> Result<Option<Fireable>, SpaceError> {
        for coll in 0..self.dirty.len() {
            if !self.dirty.get(coll).copied().unwrap_or(false) {
                continue;
            }
            for at in self.source.cascade_order(cx, self.scope, coll)? {
                let waiter = self.load(cx, at)?;
                // P6 (decision 66, LM-b): a VectorDB collection cascades over its list waiters
                // only. A removal never exposes a single-mode waiter (`ref_fill_mono`).
                if self.vdb.is_some() {
                    if let Some(fire) = self.list_cascade_fire(cx, &waiter)? {
                        return Ok(Some(Fireable::List(waiter, fire)));
                    }
                    continue;
                }
                if !self.all_at_head(cx, &waiter)? {
                    continue;
                }
                let lanes = self.lanes(&waiter.slots, cx.meter())?;
                let Some(bound) = self.search(
                    cx,
                    &waiter.colls,
                    &waiter.waiting.patterns,
                    &lanes,
                    &waiter.slots,
                    None,
                )?
                else {
                    continue;
                };
                if self.admits(cx, &waiter.waiting.continuation, &waiter.slots, &bound)? {
                    return Ok(Some(Fireable::Pair(waiter, bound)));
                }
            }
        }
        Ok(None)
    }

    /// The list of the stored list waiter `waiter` of a VectorDB collection when the cascade owes
    /// its fire (P6, decision 66, LM-b): the list `L(W, R)` is not empty, the pattern binds it and
    /// the guard admits it. Every other waiter is `None`.
    fn list_cascade_fire(
        &self,
        cx: &mut Cx<'_, '_>,
        waiter: &Waiter,
    ) -> Result<Option<ListFire>, SpaceError> {
        let Some((query, k)) = self.list_waiter(cx, &waiter.slots)? else {
            return Ok(None);
        };
        let cfg = self.vdb.ok_or(SpaceError::Inconsistent(
            "a list waiter outside a VectorDB space",
        ))?;
        let coll = *waiter
            .colls
            .first()
            .ok_or(SpaceError::Inconsistent("a list waiter has no collection"))?;
        let pattern = waiter
            .waiting
            .patterns
            .first()
            .ok_or(SpaceError::Inconsistent("a list waiter has no pattern"))?;
        let channel = self.channel(coll)?;
        let Some(fire) = vectordb::list_selection(cx, &cfg, &query, k, pattern, channel, None)?
        else {
            return Ok(None);
        };
        match cx.admits(&waiter.waiting.continuation, &fire.matched)? {
            true => Ok(Some(fire)),
            false => Ok(None),
        }
    }

    /// DRAIN-AND-CASCADE (joins plan section 4.9).
    fn drain_and_cascade(&mut self, cx: &mut Cx<'_, '_>) -> Result<(), SpaceError> {
        loop {
            if self.stop.is_some() {
                return Ok(());
            }
            if !self.work.is_empty() {
                if cx.rounds().fired() >= FIRE_ROUND_CAP {
                    self.stop = Some(ReissueCause::RoundCap);
                    return Ok(());
                }
                let Some(work) = self.work.pop() else {
                    return Ok(());
                };
                let x = self.incoming_of(cx, work)?;
                self.offer(cx, x)?;
                continue;
            }
            if !self.cascades() {
                return Ok(());
            }
            let Some(fireable) = self.next_fire(cx)? else {
                return Ok(());
            };
            if cx.rounds().fired() >= FIRE_ROUND_CAP {
                self.stop = Some(ReissueCause::RoundCap);
                return Ok(());
            }
            match fireable {
                Fireable::Pair(waiter, bound) => self.lift_and_reoffer(cx, waiter, bound)?,
                Fireable::List(waiter, fire) => self.lift_and_reoffer_list(cx, waiter, fire)?,
            }
        }
    }

    /// LIFT-AND-REOFFER-LIST (P6, decision 66, LM-b): the first member `v` of the list of the
    /// list waiter `waiter` is lifted out of its collection with `K_lift` when it is not
    /// persistent, and offered again to the waiter, which binds the list `fire`. Without a peek,
    /// the other non-persistent members of the list leave through `K_retire`. A lifted `v` that a
    /// peek waiter did not spend is a second send (decision 30). A persistent `v` stays stored.
    fn lift_and_reoffer_list(
        &mut self,
        cx: &mut Cx<'_, '_>,
        waiter: Waiter,
        fire: ListFire,
    ) -> Result<(), SpaceError> {
        let meter = cx.meter();
        let ListFire { matched, members } = fire;
        let coll = *waiter
            .colls
            .first()
            .ok_or(SpaceError::Inconsistent("a list waiter has no collection"))?;
        let mut records = meter.buffer::<(ParDatum, usize)>(members.len())?;
        for member in members {
            records.push(member.ok_or(SpaceError::Inconsistent(
                "a cascade list holds an element in flight",
            ))?);
        }
        if records.is_empty() {
            return Err(SpaceError::Inconsistent("a cascade list has no member"));
        }
        let (lead, lead_index) = records.remove(0);
        self.touch_all(cx, &waiter.colls)?;
        let peeks = !waiter.waiting.peeks.is_empty();
        if !waiter.waiting.persist
            || (!peeks && (!lead.persist || records.iter().any(|(record, _)| !record.persist)))
        {
            self.changed = true;
        }
        let lifted = !lead.persist;
        let (x, datum) = self
            .source
            .stored_element(cx, self.scope, coll, lead_index, lifted)?;
        if lifted {
            let step = i64::try_from(cx.rounds().fired())
                .map_err(|_| SpaceError::Inconsistent("a lift step overflows i64"))?;
            let counter = self.bumped(coll)?;
            self.lift(cx, coll, &datum, lead_index, counter, step)?;
        }
        let spent = !x.persist && !peeks;
        let stays = waiter.waiting.persist;
        let step = cx.rounds().after_fire(spent, stays);
        let claim = self
            .source
            .claim(coll, EntryAt::Incoming, &x.payload, meter)?;
        let mut bound = meter.buffer::<Bound>(1)?;
        bound.push(Bound {
            at: EntryAt::Incoming,
            matched,
            persist: x.persist,
            claim,
        });
        let candidates = self.candidates(cx, &waiter.colls, bound, Some(&x), None)?;
        let colls = meter.copy_slice(&waiter.colls)?;
        let group = meter.copy(&waiter.at.group)?;
        let channel = self.channel(coll)?;
        let data = self.source.incoming_record(cx, self.scope, &x, coll)?;
        cx.fire_offer_group(
            channel,
            data,
            x.persist,
            waiter.waiting,
            group,
            candidates,
            &x.introduction,
        )?;
        self.mark_dirty(&colls);
        match peeks {
            true => {
                if x.in_flight {
                    let send = Work {
                        coll,
                        record: meter.copy(&x.record)?,
                        persist: x.persist,
                        own: false,
                        restore: None,
                    };
                    generic::push_reserved(&mut self.work, send, meter)?;
                }
            }
            false => {
                let mut others = meter.buffer::<ParDatum>(records.len())?;
                others.extend(records.into_iter().map(|(record, _)| record));
                vectordb::retire_members(cx, channel, others, &x.record)?;
            }
        }
        if let FireStep::Reissue(cause) = step {
            self.stop = Some(cause);
        }
        Ok(())
    }

    /// LIFT-AND-REOFFER (joins plan section 4.6): a stored pair fires with both sides stored.
    /// The least group position whose record is not persistent is lifted out of its collection
    /// with `K_lift` and offered again to the waiter, with its identity and its own authority. A
    /// persistent record is never lifted: its re-offer is a persistent produce with its identity,
    /// and it stays stored.
    fn lift_and_reoffer(
        &mut self,
        cx: &mut Cx<'_, '_>,
        waiter: Waiter,
        bound: Vec<Bound>,
    ) -> Result<(), SpaceError> {
        let meter = cx.meter();
        let lifted = bound.iter().position(|entry| !entry.persist);
        let position = lifted.unwrap_or(0);
        let entry = bound
            .get(position)
            .ok_or(SpaceError::Inconsistent("a cascade fire binds no record"))?;
        let EntryAt::Stored(index) = entry.at else {
            return Err(SpaceError::Inconsistent(
                "a cascade fire binds an element in flight",
            ));
        };
        let coll = *waiter
            .colls
            .get(position)
            .ok_or(SpaceError::Inconsistent("a position has no collection"))?;
        self.touch_all(cx, &waiter.colls)?;
        let peeks = !waiter.waiting.peeks.is_empty();
        // A stored pair fires: a waiter that is not persistent leaves, and without a peek the
        // fire spends the bound entries that are not persistent.
        if !waiter.waiting.persist || (!peeks && bound.iter().any(|entry| !entry.persist)) {
            self.changed = true;
        }
        let (x, datum) =
            self.source
                .stored_element(cx, self.scope, coll, index, lifted.is_some())?;
        let sends = match peeks {
            true => self.second_sends(cx, &waiter.colls, &bound, Some(&x), Some(position))?,
            false => Vec::new(),
        };
        let retiring = match peeks {
            true => None,
            false => Some(self.source.before_fire(
                cx,
                self.scope,
                FireSite::Cascade,
                &waiter.colls,
                &bound,
                &[],
                Some(&x),
            )?),
        };
        let shift = match lifted {
            Some(_) => {
                let step = i64::try_from(cx.rounds().fired())
                    .map_err(|_| SpaceError::Inconsistent("a lift step overflows i64"))?;
                let counter = self.bumped(coll)?;
                self.lift(cx, coll, &datum, index, counter, step)?;
                Some((coll, index))
            }
            None => None,
        };
        let mut fired = bound;
        if let Some(slot) = fired.get_mut(position) {
            slot.at = EntryAt::Incoming;
        }
        let spent = !x.persist && !peeks;
        let stays = waiter.waiting.persist;
        let step = cx.rounds().after_fire(spent, stays);
        let candidates = self.candidates(cx, &waiter.colls, fired, Some(&x), shift)?;
        let colls = meter.copy_slice(&waiter.colls)?;
        let group = meter.copy(&waiter.at.group)?;
        let channel = self.channel(coll)?;
        let data = self.source.incoming_record(cx, self.scope, &x, coll)?;
        cx.fire_offer_group(
            channel,
            data,
            x.persist,
            waiter.waiting,
            group,
            candidates,
            &x.introduction,
        )?;
        self.mark_dirty(&colls);
        match retiring {
            None => self.push_work(sends, meter)?,
            Some(retiring) => {
                // The re-offered record is the element whose fire retires the bound set classes,
                // as the datum of a produce-side fire is (decision 50).
                let element_random = Blake2b512Random::from_bytes(&x.record.random_state);
                self.source.retire_spent(
                    cx,
                    self.scope,
                    retiring,
                    &colls,
                    &element_random,
                    x.record.cost_authority.as_ref(),
                    &mut self.dirty,
                )?;
            }
        }
        if let FireStep::Reissue(cause) = step {
            self.stop = Some(cause);
        }
        Ok(())
    }

    /// The lift of the stored record `datum` at the index `index` of the collection `coll`: a
    /// consume fire of `K_lift`, which removes the record and is never dispatched
    /// (`spaces::settle`). A collection with a counter keys `K_lift` by the counter value
    /// `counter` of the transaction and the step `step`, as the settle does. A collection without
    /// a counter (the set, decision 50) keys it by the source hash of the record, as `K_retire` is
    /// keyed (F3), so every lift of a set has an identity of its own also when a re-issue repeats
    /// the random state and the step. Each copy and the random bytes are reserved first.
    fn lift(
        &mut self,
        cx: &mut Cx<'_, '_>,
        coll: usize,
        datum: &ParDatum,
        index: usize,
        counter: Option<i64>,
        step: i64,
    ) -> Result<(), SpaceError> {
        let meter = cx.meter();
        let channel = self.channel(coll)?;
        let matched = cx
            .bind(&LIFT_PATTERN, &datum.a)?
            .ok_or(SpaceError::Inconsistent(
                "a stored record does not match the lift pattern",
            ))?;
        let random = self
            .lift_random
            .get_or_insert_with(|| RandomPurpose::Lift.derive(&self.random))
            .clone();
        meter.reserve_copy(&*LIFT_CONTINUATION)?;
        meter.random_bytes(&random)?;
        let cost_authority = self
            .authority
            .as_ref()
            .map(|authority| meter.copy(authority))
            .transpose()?;
        let continuation = match counter {
            Some(counter) => lift_continuation(counter, step, &random, cost_authority),
            None => {
                meter.reserve_buffer::<u8>(datum.source.hash.0.len())?;
                retire_continuation(&datum.source.hash, &random, cost_authority)
            }
        };
        let view_index = i32::try_from(index)
            .map_err(|_| SpaceError::Inconsistent("a record index exceeds i32"))?;
        let mut channels = meter.buffer::<Par>(1)?;
        channels.push(meter.copy(channel)?);
        let mut patterns = meter.buffer::<BindPattern>(1)?;
        patterns.push(meter.copy(&*LIFT_PATTERN)?);
        let mut candidates = meter.buffer::<ParCandidate>(1)?;
        candidates.push(ConsumeCandidate {
            channel: meter.copy(channel)?,
            datum: Datum {
                a: matched,
                persist: datum.persist,
                source: meter.copy(&datum.source)?,
            },
            removed_datum: meter.copy(&datum.a)?,
            datum_index: view_index,
        });
        cx.lift(channels, patterns, continuation, candidates)
    }

    /// COMMIT (joins plan section 4.10): the re-issue of the transaction. A quiescent transaction
    /// re-issues the operation when its element is still in flight. A stopped transaction with
    /// owed work re-issues one `SettleOp` with the cascade state, and otherwise the operation.
    fn commit(
        mut self,
        cx: &mut Cx<'_, '_>,
        op: SpaceOp,
    ) -> Result<Option<SpaceReissue>, SpaceError> {
        let authority = OperationAuthority {
            continuation: self.authority.as_ref(),
            introduction: cx.introduction(),
        };
        self.source
            .finish(cx, self.scope, self.changed, &self.random, authority)?;
        let meter = cx.meter();
        let pending: Vec<Work> = {
            let mut pending = Vec::new();
            for work in std::mem::take(&mut self.work).into_iter().rev() {
                match work.own {
                    true => self.own = Own::InFlight,
                    false => generic::push_reserved(&mut pending, work, meter)?,
                }
            }
            pending
        };
        let owed = !pending.is_empty() || (self.cascades() && self.dirty.iter().any(|&mark| mark));
        let settle_fields = match (&self.stop, owed) {
            (Some(_), true) => Some(self.settle_fields(&op, meter)?),
            (Some(_), false) | (None, _) => None,
        };
        let resume = self.resume(cx, op)?;
        match (self.stop, settle_fields) {
            (None, _) | (Some(_), None) => Ok(resume),
            (Some(cause), Some(mut settle)) => {
                let mut seeds: Vec<CollectionKey> = Vec::new();
                if self.cascades() {
                    for (coll, &mark) in self.dirty.iter().enumerate() {
                        if !mark {
                            continue;
                        }
                        let key = &collection(self.scope, coll)?.key;
                        meter.compare(key.as_bytes().len(), settle.key.as_bytes().len())?;
                        if *key == settle.key {
                            continue;
                        }
                        generic::push_reserved(&mut seeds, copy_key(key, meter)?, meter)?;
                    }
                }
                let mut pending_produces = meter.buffer::<PendingProduce>(pending.len())?;
                for work in pending {
                    pending_produces.push(PendingProduce {
                        key: copy_key(&collection(self.scope, work.coll)?.key, meter)?,
                        record: work.record,
                        persist: work.persist,
                        restore: work.restore,
                    });
                }
                settle.cascade = Some(Box::new(CascadeState {
                    seeds,
                    pending: pending_produces,
                    resume: resume.map(Box::new),
                }));
                Ok(Some(cx.reissue(SpaceOp::Settle(settle), cause)))
            }
        }
    }

    /// The fields of the settle job that continues a stopped transaction of `op`: the space, the
    /// key and the channel of the operation (of the first bind of a join), its random state, its
    /// cost authority and its introduction authority. Each copy is reserved first.
    fn settle_fields(&self, op: &SpaceOp, meter: &ScanMeter) -> Result<SettleOp, SpaceError> {
        match op {
            SpaceOp::Produce(produce) => settle_operation(generic::OpView::Produce(produce), meter),
            SpaceOp::Consume(consume) => settle_operation(generic::OpView::Consume(consume), meter),
            SpaceOp::Settle(settle) => Ok(SettleOp {
                space: settle.space,
                key: copy_key(&settle.key, meter)?,
                channel: meter.copy(&settle.channel)?,
                random_state: meter.copy(&settle.random_state)?,
                authority: meter.copy(&settle.authority)?,
                introduction_authority: meter.copy(&settle.introduction_authority)?,
                cascade: None,
            }),
            SpaceOp::Join(join) => {
                let first = join
                    .binds
                    .first()
                    .ok_or(SpaceError::Inconsistent("a join has no bind"))?;
                Ok(SettleOp {
                    space: join.space,
                    key: copy_key(&first.key, meter)?,
                    channel: meter.copy(&first.channel)?,
                    random_state: meter.copy(&join.body.random_state)?,
                    authority: meter.copy(&join.authority)?,
                    introduction_authority: meter.copy(&join.introduction_authority)?,
                    cascade: None,
                })
            }
        }
    }

    /// The re-issue of the operation (decision 43 order): `None` when its element settled. A
    /// produce and a consume keep the order of their first round when they are persistent, and
    /// a join keeps the order of each bind. A settle re-issues the re-issue that it carries.
    fn resume(&self, cx: &mut Cx<'_, '_>, op: SpaceOp) -> Result<Option<SpaceReissue>, SpaceError> {
        let meter = cx.meter();
        let cause = self
            .own_cause
            .or(self.stop)
            .unwrap_or(ReissueCause::DivergentPair);
        match op {
            SpaceOp::Settle(settle) => Ok(settle
                .cascade
                .and_then(|cascade| cascade.resume)
                .map(|resume| *resume)),
            op => match self.own {
                Own::Settled => Ok(None),
                Own::NotStarted | Own::InFlight => {
                    let reissued = match op {
                        SpaceOp::Produce(produce) => {
                            let coll = self.scope.index_of_key(&produce.key, meter)?;
                            let ord = self.ord_on(coll)?;
                            SpaceOp::Produce(ProduceOp {
                                reissued_ord: produce.persistent.then_some(ord),
                                ..produce
                            })
                        }
                        SpaceOp::Consume(consume) => {
                            let coll = self.scope.index_of_key(&consume.key, meter)?;
                            let ord = self.ord_on(coll)?;
                            SpaceOp::Consume(ConsumeOp {
                                reissued_ord: consume.persistent.then_some(ord),
                                ..consume
                            })
                        }
                        SpaceOp::Join(join) => {
                            let reissued_ords = match join.persistent {
                                true => {
                                    let mut ords = meter.buffer::<i64>(join.binds.len())?;
                                    for bind in &join.binds {
                                        let coll = self.scope.index_of_key(&bind.key, meter)?;
                                        ords.push(self.ord_on(coll)?);
                                    }
                                    Some(ords)
                                }
                                false => None,
                            };
                            SpaceOp::Join(JoinOp {
                                reissued_ords,
                                ..join
                            })
                        }
                        SpaceOp::Settle(settle) => SpaceOp::Settle(settle),
                    };
                    Ok(Some(cx.reissue(reissued, cause)))
                }
            },
        }
    }
}

/// True when the lane `lane` of a bind sees an entry of the level `level` (decision 38).
fn visible(lane: Option<Lane>, level: Option<Level>) -> Result<bool, SpaceError> {
    match (lane, level) {
        (Some(lane), Some(level)) => Ok(lane.sees(level)),
        (None, None) => Ok(true),
        (Some(_), None) | (None, Some(_)) => Err(SpaceError::Inconsistent(
            "a lane and a level do not belong to one discipline",
        )),
    }
}

/// True when `claim` is one of `claimed`. Two record claims compare their places. Two class
/// claims of one collection compare their keys, the hashes first (DR-90). Each comparison is
/// reserved.
fn is_claimed(claimed: &[Claim], claim: &Claim, meter: &ScanMeter) -> Result<bool, SpaceError> {
    for other in claimed {
        meter.compare(size_of::<Claim>(), size_of::<Claim>())?;
        let same = match (other, claim) {
            (Claim::Record { coll, at }, Claim::Record { coll: c, at: a }) => coll == c && at == a,
            (
                Claim::Class { coll, key, hash },
                Claim::Class {
                    coll: c, key: k, ..
                },
            ) => *coll == *c && same_class(key, hash, k, meter)?,
            (Claim::Record { .. }, Claim::Class { .. })
            | (Claim::Class { .. }, Claim::Record { .. }) => false,
        };
        if same {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::Expr;
    use models::rust::space_factory_urn::{SpaceFactoryUrn, SpaceInner, SpaceOuter};
    use models::rust::utils::{new_freevar_par, new_freevar_var, new_gint_par, new_gstring_par};
    use rspace_plus_plus::rspace::rspace::RSpace;
    use rspace_plus_plus::rspace::rspace_interface::ISpace;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    use rspace_plus_plus::rspace::space_store_handle::SpaceStoreHandle;

    use super::*;
    use crate::rust::interpreter::accounting::costs::Cost;
    use crate::rust::interpreter::accounting::RuntimeBudget;
    use crate::rust::interpreter::matcher::r#match::Matcher;
    use crate::rust::interpreter::spaces::agent::{
        run_op, JoinBind, SpaceAgent, SpaceCreation, SpaceOutcome,
    };
    use crate::rust::interpreter::spaces::families::FamilyAgent;
    use crate::rust::interpreter::spaces::namespace::{Durability, SID_LEN};
    use crate::rust::interpreter::spaces::types::qualifier::SpaceQualifier;

    type ParRSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

    const SPACE: NativeSpace = NativeSpace {
        sid: [11; SID_LEN],
        durability: Durability::Durable,
    };

    fn int(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

    fn string(value: &str) -> Par { new_gstring_par(value.to_string(), Vec::new(), false) }

    fn key_of(channel: &Par) -> CollectionKey {
        CollectionKey::of_channel(channel).expect("a channel has a key")
    }

    fn data_of(channel: &Par) -> Par {
        namespace::gprivate(namespace::data_channel(SPACE, &key_of(channel)))
    }

    fn named(name: &str) -> TaggedContinuation {
        TaggedContinuation {
            tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                body: Some(string(name)),
                random_state: Blake2b512Random::create_from_bytes(name.as_bytes()).to_bytes(),
            })),
            guard: None,
            cost_authority: None,
        }
    }

    fn body_of(name: &str) -> ParWithRandom {
        match named(name).tagged_cont {
            Some(TaggedCont::ParBody(body)) => body,
            Some(TaggedCont::ScalaBodyRef(_)) | None => ParWithRandom::default(),
        }
    }

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

    /// A space of the family `outer:inner` over a fresh play space.
    async fn space_of(inner: SpaceInner) -> (ParRSpace, SpaceEnv) {
        let mut stores = InMemoryStoreManager::new();
        let host = ParRSpace::create(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(Matcher)),
        )
        .expect("an in-memory space");
        let env = SpaceEnv {
            store: SpaceStoreHandle::new(host.clone()),
            budget: RuntimeBudget::new(Cost::create(10_000_000, "join engine test")),
            meter: ScanMeter::new(None),
            volatile: Default::default(),
        };
        let family = SpaceFactoryUrn::all()
            .find(|urn| urn.outer == SpaceOuter::HashMap && urn.inner == inner)
            .expect("a factory family");
        let config = SpaceConfig::new(
            SPACE.sid,
            SpaceOuter::HashMap.as_str(),
            inner.as_str(),
            SpaceQualifier::Default,
        )
        .to_par();
        FamilyAgent
            .prepare_create(&env, SpaceCreation {
                space: SPACE,
                config,
                handle_pattern: BindPattern {
                    patterns: Vec::new(),
                    remainder: Some(new_freevar_var(0)),
                    free_count: 1,
                },
                handle_continuation: named("handle"),
                random: Blake2b512Random::create_from_bytes(b"join engine factory"),
                authority: CostAuthority::default(),
                family,
            })
            .await
            .expect("the creation stages")
            .commit()
            .await
            .expect("the creation commits");
        host.take_event_log().await;
        (host, env)
    }

    fn send(channel: &Par, value: i64, persistent: bool) -> SpaceOp {
        SpaceOp::Produce(ProduceOp {
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
        })
    }

    fn receive(channel: &Par, name: &str, pattern: BindPattern) -> SpaceOp {
        SpaceOp::Consume(ConsumeOp {
            space: SPACE,
            key: key_of(channel),
            channel: channel.clone(),
            pattern,
            body: body_of(name),
            persistent: false,
            peek: false,
            guard: None,
            query: None,
            authority: CostAuthority::default(),
            introduction_authority: CostAuthority::default(),
            reissued_ord: None,
        })
    }

    fn join(binds: &[(&Par, BindPattern)], name: &str, peek: bool) -> SpaceOp {
        SpaceOp::Join(JoinOp {
            space: SPACE,
            binds: binds
                .iter()
                .map(|(channel, pattern)| JoinBind {
                    key: key_of(channel),
                    channel: (*channel).clone(),
                    pattern: pattern.clone(),
                    query: None,
                })
                .collect(),
            body: body_of(name),
            persistent: false,
            peek,
            guard: None,
            authority: CostAuthority::default(),
            introduction_authority: CostAuthority::default(),
            reissued_ords: None,
        })
    }

    async fn run(env: &SpaceEnv, op: SpaceOp) -> SpaceOutcome {
        run_op(&FamilyAgent, env, op)
            .await
            .expect("the operation commits")
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

    fn fire(name: &str, values: &[i64]) -> (String, Vec<Par>) {
        (
            name.to_string(),
            values.iter().map(|&value| int(value)).collect(),
        )
    }

    /// The payloads of the records of the collection of `channel`, in the order of the store.
    async fn payloads(host: &ParRSpace, channel: &Par) -> Vec<Vec<Par>> {
        let mut values: Vec<(i64, Vec<Par>)> = host
            .get_data(&data_of(channel))
            .await
            .into_iter()
            .map(|datum| {
                let (ord, _) = record::pq_datum_slots(&datum.a).expect("a datum record");
                let payload = record::unwrap_pq_datum(&datum.a)
                    .expect("a datum record")
                    .datum
                    .pars;
                (ord, payload)
            })
            .collect();
        values.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.len().cmp(&right.1.len())));
        values.into_iter().map(|(_, payload)| payload).collect()
    }

    #[tokio::test]
    async fn space_join_engine_queue_join_parks_then_fires_on_the_second_send() {
        let (host, env) = space_of(SpaceInner::Queue).await;
        let (a, b) = (string("a"), string("b"));
        let parked = run(&env, join(&[(&a, any()), (&b, any())], "j", false)).await;
        assert!(parked.fires.is_empty());
        let stored = run(&env, send(&a, 1, false)).await;
        assert!(stored.fires.is_empty());
        assert_eq!(payloads(&host, &a).await, vec![vec![int(1)]]);
        let outcome = run(&env, send(&b, 2, false)).await;
        assert_eq!(fired(&outcome), vec![fire("j", &[1, 2])]);
        assert!(host.get_data(&data_of(&a)).await.is_empty());
        assert!(host.get_data(&data_of(&b)).await.is_empty());
        assert!(host.get_joins(data_of(&a)).await.is_empty());
        assert!(host.get_joins(data_of(&b)).await.is_empty());
    }

    #[tokio::test]
    async fn space_join_engine_queue_join_fires_from_both_heads() {
        let (host, env) = space_of(SpaceInner::Queue).await;
        let (a, b) = (string("a"), string("b"));
        for (channel, value) in [(&a, 1), (&a, 3), (&b, 2)] {
            run(&env, send(channel, value, false)).await;
        }
        let outcome = run(&env, join(&[(&b, any()), (&a, any())], "j", false)).await;
        // The body sees the binds in bind order: b first, then a. The head of a is 1.
        assert_eq!(fired(&outcome), vec![fire("j", &[2, 1])]);
        assert_eq!(payloads(&host, &a).await, vec![vec![int(3)]]);
        // A head that does not match blocks the join: the head of a is 3, not 5.
        let blocked = run(&env, join(&[(&a, exactly(5)), (&b, any())], "k", false)).await;
        assert!(blocked.fires.is_empty());
        run(&env, send(&b, 4, false)).await;
        assert_eq!(payloads(&host, &b).await, vec![vec![int(4)]]);
    }

    #[tokio::test]
    async fn space_join_engine_cascade_fires_an_exposed_head() {
        let (host, env) = space_of(SpaceInner::Queue).await;
        let (q1, q2) = (string("q1"), string("q2"));
        run(&env, send(&q2, 10, false)).await;
        run(&env, send(&q2, 20, false)).await;
        // W waits on q1 and q2, and V waits behind W on q2 (decision 11).
        run(&env, join(&[(&q1, any()), (&q2, any())], "w", false)).await;
        let behind = run(&env, receive(&q2, "v", any())).await;
        assert!(behind.fires.is_empty());
        // A send on q1 fires W with the head 10 of q2, and the cascade then fires V with 20.
        let outcome = run(&env, send(&q1, 1, false)).await;
        assert_eq!(fired(&outcome), vec![fire("w", &[1, 10]), fire("v", &[20])]);
        assert!(host.get_data(&data_of(&q2)).await.is_empty());
        assert!(host
            .get_waiting_continuations(vec![data_of(&q2)])
            .await
            .is_empty());
    }

    #[tokio::test]
    async fn space_join_engine_peek_join_sends_the_stored_records_again() {
        let (host, env) = space_of(SpaceInner::Bag).await;
        let (a, b) = (string("a"), string("b"));
        run(&env, send(&a, 1, false)).await;
        run(&env, send(&b, 2, false)).await;
        let before_a = host.get_data(&data_of(&a)).await;
        let before_b = host.get_data(&data_of(&b)).await;
        let outcome = run(&env, join(&[(&a, any()), (&b, any())], "p", true)).await;
        assert_eq!(fired(&outcome), vec![fire("p", &[1, 2])]);
        // Rule E: the peek fire removes both records, and the transaction sends them again with
        // their bytes, so with their identities.
        assert_eq!(host.get_data(&data_of(&a)).await, before_a);
        assert_eq!(host.get_data(&data_of(&b)).await, before_b);
    }

    #[tokio::test]
    async fn space_join_engine_repeated_channel_binds_distinct_records() {
        let (host, env) = space_of(SpaceInner::Bag).await;
        let a = string("a");
        run(&env, send(&a, 1, false)).await;
        let parked = run(&env, join(&[(&a, any()), (&a, any())], "d", false)).await;
        assert!(parked.fires.is_empty(), "one record cannot bind two binds");
        let outcome = run(&env, send(&a, 2, false)).await;
        let values = fired(&outcome);
        assert_eq!(values.len(), 1);
        let (_, mut bound) = values.into_iter().next().expect("one fire");
        bound.sort_by_key(|par| format!("{par:?}"));
        let mut expected = vec![int(1), int(2)];
        expected.sort_by_key(|par| format!("{par:?}"));
        assert_eq!(bound, expected);
        assert!(host.get_data(&data_of(&a)).await.is_empty());
    }

    // ── White-box tests of the classification, the scope and the windows ────────────────────

    use models::rust::host_work::{HostWorkDimension, HostWorkLimit, HostWorkLimits};

    use crate::rust::interpreter::host_work::HostWorkBudget;
    use crate::rust::interpreter::spaces::disciplines::queue::QueueDiscipline;
    use crate::rust::interpreter::spaces::disciplines::stack::StackDiscipline;
    use crate::rust::interpreter::spaces::generic::begin_collection;
    use crate::rust::interpreter::spaces::outer::hashmap::HashMapOuter;

    type Queue = GenericSpace<HashMapOuter, QueueDiscipline>;

    fn unbounded() -> HostWorkBudget {
        HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX)))
    }

    /// The usage of the three dimensions that a space job charges.
    fn usage(host_work: &HostWorkBudget) -> [u64; 3] {
        [
            HostWorkDimension::VerificationOperations,
            HostWorkDimension::VerificationBytes,
            HostWorkDimension::SearchStateBytes,
        ]
        .map(|dimension| host_work.usage(dimension).get())
    }

    /// The route of `op` on the collection of `channel`, and the host work of the
    /// classification alone.
    async fn classified(env: &SpaceEnv, channel: &Par, op: &SpaceOp) -> (JoinRoute, [u64; 3]) {
        let host_work = unbounded();
        let metered = SpaceEnv {
            meter: ScanMeter::new(Some(host_work.clone())),
            ..env.clone()
        };
        let mut opened =
            begin_collection(&HashMapOuter, &metered, SPACE, &key_of(channel), channel)
                .await
                .expect("the collection opens");
        let before = usage(&host_work);
        let route = classify(&Queue::default(), &metered, &mut opened, op)
            .expect("the operation classifies");
        let after = usage(&host_work);
        (route, [
            after[0] - before[0],
            after[1] - before[1],
            after[2] - before[2],
        ])
    }

    #[tokio::test]
    async fn space_join_engine_classification_reserves_the_join_check_of_p4() {
        let (_host, env) = space_of(SpaceInner::Queue).await;
        let a = string("a");
        // No join group: the single route, and nothing is reserved.
        let (route, spent) = classified(&env, &a, &send(&a, 1, false)).await;
        assert_eq!(route, JoinRoute::Single);
        assert_eq!(spent, [0, 0, 0]);
        // The own group only: the single route, and the reads of the P4 join check.
        run(&env, receive(&a, "w", exactly(9))).await;
        let (route, spent) = classified(&env, &a, &send(&a, 1, false)).await;
        assert_eq!(route, JoinRoute::Single);
        let expected = {
            let host_work = unbounded();
            let meter = ScanMeter::new(Some(host_work.clone()));
            let data = data_of(&a);
            let group = vec![data.clone()];
            meter.inspect(&group).expect("an unbounded read");
            meter.inspect(&data).expect("an unbounded read");
            meter.charge(1, 0, 0).expect("an unbounded charge");
            usage(&host_work)
        };
        assert_eq!(spent, expected);
    }

    #[tokio::test]
    async fn space_join_engine_scope_closes_over_the_join_groups() {
        let (_host, env) = space_of(SpaceInner::Queue).await;
        let (a, b, c, d) = (string("a"), string("b"), string("c"), string("d"));
        // W1 joins a and b, and W2 joins b and c. A send on a reaches c through b.
        run(&env, join(&[(&a, any()), (&b, exactly(5))], "w1", false)).await;
        run(&env, join(&[(&b, exactly(6)), (&c, any())], "w2", false)).await;
        let (route, _) = classified(&env, &a, &send(&a, 1, false)).await;
        let JoinRoute::Join(scope) = route else {
            panic!("a send on a joined collection runs in the join engine");
        };
        let mut keys = vec![key_of(&a), key_of(&b), key_of(&c)];
        keys.sort();
        assert_eq!(
            scope
                .collections
                .iter()
                .map(|collection| collection.key.clone())
                .collect::<Vec<_>>(),
            keys
        );
        assert_eq!(scope.own, vec![key_of(&a)]);
        let meter = ScanMeter::new(None);
        assert_eq!(scope.covers(&scope.own, &meter), Ok(false));
        assert_eq!(scope.covers(&keys, &meter), Ok(true));
        // The lock keys: the config and the space counter, then the counter and the data channel
        // of each collection in ascending key order.
        assert_eq!(scope.lock_keys().len(), 2 + 2 * keys.len());
        for (pair, key) in scope.lock_keys()[2..].chunks(2).zip(&keys) {
            assert_eq!(pair, [
                namespace::gprivate(namespace::collection_counter(SPACE, key)),
                namespace::gprivate(namespace::data_channel(SPACE, key)),
            ]);
        }
        // A collection outside every join keeps the single route.
        let (route, _) = classified(&env, &d, &send(&d, 1, false)).await;
        assert_eq!(route, JoinRoute::Single);
        // A join always classifies as a join, with its binds as its own collections.
        let (route, _) = classified(&env, &d, &join(&[(&d, any()), (&a, any())], "j", false)).await;
        let JoinRoute::Join(scope) = route else {
            panic!("a join runs in the join engine");
        };
        assert_eq!(scope.own.len(), 2);
        assert_eq!(scope.collections.len(), 4);
    }

    #[test]
    fn space_join_engine_place_follows_the_head_rule() {
        let meter = ScanMeter::new(None);
        let queue: KeyedSource<'_, HashMapOuter, QueueDiscipline> = KeyedSource {
            outer: std::marker::PhantomData,
            discipline: &QueueDiscipline,
        };
        let stack: KeyedSource<'_, HashMapOuter, StackDiscipline> = KeyedSource {
            outer: std::marker::PhantomData,
            discipline: &StackDiscipline,
        };
        let stored = [2, 5, 8];
        // A fresh element joins the tail of a queue and the top of a stack.
        assert_eq!(
            queue.place(Side::Data, Placement::Fresh, 9, &stored, &meter),
            Ok(3)
        );
        assert_eq!(
            queue.place(Side::Data, Placement::Fresh, 9, &[], &meter),
            Ok(0)
        );
        assert_eq!(
            stack.place(Side::Data, Placement::Fresh, 9, &stored, &meter),
            Ok(0)
        );
        // A carried order takes its place: behind the lesser orders of a queue, below the
        // greater orders of a stack.
        assert_eq!(
            queue.place(Side::Data, Placement::Carried, 6, &stored, &meter),
            Ok(2)
        );
        assert_eq!(
            queue.place(Side::Data, Placement::Carried, 1, &stored, &meter),
            Ok(0)
        );
        assert_eq!(
            stack.place(Side::Data, Placement::Carried, 6, &stored, &meter),
            Ok(1)
        );
        assert_eq!(
            stack.place(Side::Data, Placement::Carried, 9, &stored, &meter),
            Ok(0)
        );
        assert_eq!(
            queue.place(Side::Data, Placement::Carried, 5, &stored, &meter),
            Err(SpaceError::Inconsistent(
                "an element in flight shares its order with a stored entry"
            ))
        );
    }

    #[test]
    fn space_join_engine_head_window_places_the_element_in_flight() {
        let meter = ScanMeter::new(None);
        let queue: KeyedSource<'_, HashMapOuter, QueueDiscipline> = KeyedSource {
            outer: std::marker::PhantomData,
            discipline: &QueueDiscipline,
        };
        // The stored orders 8, 2 and 5 at the indices 0, 1 and 2 of the view.
        let records = [(8, 0), (2, 1), (5, 2)];
        let stored = |index| Entry {
            at: EntryAt::Stored(index),
            level: None,
        };
        let incoming = Entry {
            at: EntryAt::Incoming,
            level: None,
        };
        assert_eq!(
            queue.head_window(&records, 2, None, &meter),
            Ok(vec![stored(1), stored(2)])
        );
        assert_eq!(
            queue.head_window(&records, 5, None, &meter),
            Ok(vec![stored(1), stored(2), stored(0)])
        );
        let element = |ord, placement| Incoming {
            coll: 0,
            record: ListParWithRandom::default(),
            payload: ListParWithRandom::default(),
            source: Produce::create(&int(0), &ListParWithRandom::default(), false),
            persist: false,
            ord,
            level: None,
            placement,
            introduction: Introduction::new(
                &RuntimeBudget::new(Cost::create(1_000, "window test")),
                &CostAuthority::default(),
                &ScanMeter::new(None),
            )
            .expect("an unmetered introduction"),
            own: true,
            in_flight: true,
            restore: None,
        };
        // A fresh element of a queue is at the tail: it is in a window of four entries only.
        assert_eq!(
            queue.head_window(&records, 2, Some(&element(9, Placement::Fresh)), &meter),
            Ok(vec![stored(1), stored(2)])
        );
        assert_eq!(
            queue.head_window(&records, 4, Some(&element(9, Placement::Fresh)), &meter),
            Ok(vec![stored(1), stored(2), stored(0), incoming])
        );
        // A carried order 3 takes the second place of the head order.
        assert_eq!(
            queue.head_window(&records, 2, Some(&element(3, Placement::Carried)), &meter),
            Ok(vec![stored(1), incoming])
        );
    }

    #[test]
    fn space_join_engine_reserved_sort_orders_and_reserves_each_comparison() {
        let host_work = unbounded();
        let meter = ScanMeter::new(Some(host_work.clone()));
        let mut values = vec![5, 1, 4, 2, 3];
        sort_reserved(&mut values, |_| 8, &meter).expect("an unbounded sort");
        assert_eq!(values, vec![1, 2, 3, 4, 5]);
        assert!(
            host_work
                .usage(HostWorkDimension::VerificationOperations)
                .get()
                > 0
        );
        let mut sorted = vec![1, 2, 3];
        let before = host_work
            .usage(HostWorkDimension::VerificationOperations)
            .get();
        sort_reserved(&mut sorted, |_| 8, &meter).expect("an unbounded sort");
        // A sorted list costs one comparison for each adjacent pair.
        assert_eq!(
            host_work
                .usage(HostWorkDimension::VerificationOperations)
                .get()
                - before,
            2
        );
    }

    // ── Smoke tests of the other window shapes ───────────────────────────────────────────────

    use crate::rust::interpreter::spaces::agent::QueryParams;

    /// A join with a query `priority = level` on the binds of `levels` (`None`: no query).
    fn join_with_lanes(binds: &[(&Par, BindPattern, Option<i64>)], name: &str) -> SpaceOp {
        SpaceOp::Join(JoinOp {
            space: SPACE,
            binds: binds
                .iter()
                .map(|(channel, pattern, level)| JoinBind {
                    key: key_of(channel),
                    channel: (*channel).clone(),
                    pattern: pattern.clone(),
                    query: level.map(|level| QueryParams {
                        payload: Vec::new(),
                        named: BTreeMap::from([("priority".to_string(), int(level))]),
                    }),
                })
                .collect(),
            body: body_of(name),
            persistent: false,
            peek: false,
            guard: None,
            authority: CostAuthority::default(),
            introduction_authority: CostAuthority::default(),
            reissued_ords: None,
        })
    }

    /// A send of `value` on `channel` with the param `priority = level`.
    fn send_level(channel: &Par, value: i64, level: i64) -> SpaceOp {
        match send(channel, value, false) {
            SpaceOp::Produce(produce) => SpaceOp::Produce(ProduceOp {
                params: BTreeMap::from([("priority".to_string(), int(level))]),
                ..produce
            }),
            other => other,
        }
    }

    #[tokio::test]
    async fn space_join_engine_priority_lanes_see_their_levels() {
        let (host, env) = space_of(SpaceInner::PriorityQueue).await;
        let (p, q) = (string("p"), string("q"));
        run(&env, send_level(&p, 10, 0)).await;
        run(&env, send_level(&p, 20, 1)).await;
        run(&env, send(&q, 30, false)).await;
        // The lane of level 1 sees the head of level 1 only, and the bind on q sees every level.
        let outcome = run(
            &env,
            join_with_lanes(&[(&p, any(), Some(1)), (&q, any(), None)], "lanes"),
        )
        .await;
        assert_eq!(fired(&outcome), vec![fire("lanes", &[20, 30])]);
        assert_eq!(payloads(&host, &p).await, vec![vec![int(10)]]);
        // An unrestricted bind takes the head of the least level.
        run(&env, send(&q, 40, false)).await;
        let outcome = run(
            &env,
            join_with_lanes(&[(&p, any(), None), (&q, any(), None)], "any"),
        )
        .await;
        assert_eq!(fired(&outcome), vec![fire("any", &[10, 40])]);
    }

    #[tokio::test]
    async fn space_join_engine_set_bind_retires_its_class() {
        let (host, env) = space_of(SpaceInner::Set).await;
        let (s, t) = (string("s"), string("t"));
        for value in [1, 2] {
            run(&env, send(&s, value, false)).await;
        }
        // A second member of the class of 1, with another random state.
        let SpaceOp::Produce(member) = send(&s, 1, false) else {
            panic!("a send is a produce");
        };
        run(
            &env,
            SpaceOp::Produce(ProduceOp {
                data: ListParWithRandom {
                    random_state: Blake2b512Random::create_from_bytes(b"another member").to_bytes(),
                    ..member.data
                },
                ..member
            }),
        )
        .await;
        run(&env, send(&t, 9, false)).await;
        assert_eq!(host.get_data(&data_of(&s)).await.len(), 3);
        let outcome = run(&env, join(&[(&s, exactly(1)), (&t, any())], "j", false)).await;
        assert_eq!(fired(&outcome), vec![fire("j", &[9])]);
        // The fire binds one member of the class of 1 and retires the other one.
        assert_eq!(payloads(&host, &s).await, vec![vec![int(2)]]);
        assert!(host.get_data(&data_of(&t)).await.is_empty());
    }

    #[tokio::test]
    async fn space_join_engine_cell_holds_one_waiter() {
        let (host, env) = space_of(SpaceInner::Cell).await;
        let (c1, c2) = (string("c1"), string("c2"));
        run(&env, send(&c1, 1, false)).await;
        let parked = run(&env, join(&[(&c1, any()), (&c2, any())], "w", false)).await;
        assert!(parked.fires.is_empty());
        // The waiter of the join holds the waiter slot of c1 (F15).
        assert_eq!(
            run_op(
                &FamilyAgent,
                &env,
                join(&[(&c1, any()), (&c2, exactly(7))], "x", false)
            )
            .await,
            Err(SpaceError::CellOccupied)
        );
        let outcome = run(&env, send(&c2, 2, false)).await;
        assert_eq!(fired(&outcome), vec![fire("w", &[1, 2])]);
        assert!(host.get_data(&data_of(&c1)).await.is_empty());
    }

    #[tokio::test]
    async fn space_join_engine_stack_cascade_fires_after_a_pop() {
        let (host, env) = space_of(SpaceInner::Stack).await;
        let (s1, s2) = (string("s1"), string("s2"));
        run(&env, send(&s1, 1, false)).await;
        run(&env, send(&s1, 2, false)).await;
        // W needs 1 on s1, under the top 2: it waits.
        let parked = run(&env, join(&[(&s1, exactly(1)), (&s2, any())], "w", false)).await;
        assert!(parked.fires.is_empty());
        let stored = run(&env, send(&s2, 5, false)).await;
        assert!(stored.fires.is_empty(), "the top of s1 blocks W");
        // A pop of s1 takes 2. Then 1 is the top of s1, and the cascade fires W. The literal
        // pattern of W on s1 binds no value, so W binds 5 only.
        let outcome = run(&env, receive(&s1, "v", any())).await;
        assert_eq!(fired(&outcome), vec![fire("v", &[2]), fire("w", &[5])]);
        assert!(host.get_data(&data_of(&s1)).await.is_empty());
        assert!(host.get_data(&data_of(&s2)).await.is_empty());
    }

    // ── Decision 50: the set cascade (finding F-JF-1) ────────────────────────────────────────

    /// A send of `value` on the set collection `channel` with the random state of `seed`, and the
    /// source hash of its record, which places it in the canonical order of DR-75.
    fn seeded_send(
        channel: &Par,
        value: i64,
        seed: &str,
        persistent: bool,
    ) -> (SpaceOp, Blake2b256Hash) {
        let SpaceOp::Produce(op) = send(channel, value, persistent) else {
            panic!("a send is a produce");
        };
        let op = ProduceOp {
            data: ListParWithRandom {
                random_state: Blake2b512Random::create_from_bytes(seed.as_bytes()).to_bytes(),
                ..op.data
            },
            ..op
        };
        let record = record::wrap_datum(0, &op.data);
        let hash = Produce::create(&data_of(channel), &record, persistent).hash;
        (SpaceOp::Produce(op), hash)
    }

    #[tokio::test]
    async fn space_join_engine_set_cascade_fires_a_waiter_that_a_class_move_exposes() {
        let (host, env) = space_of(SpaceInner::Set).await;
        let s = string("s");
        // The class of 1 has a non-persistent member a1 and a persistent member a2, and b is the
        // class of 2. The canonical order is a1, b, a2, so the class of 1 sits at the place of
        // a1 first.
        let (a1, a1_hash) = seeded_send(&s, 1, "a1", false);
        let (b, b_hash) = (0..)
            .map(|seed| seeded_send(&s, 2, &format!("b{seed}"), false))
            .find(|(_, hash)| *hash > a1_hash)
            .expect("a seed orders b after a1");
        let (a2, _) = (0..)
            .map(|seed| seeded_send(&s, 1, &format!("a2-{seed}"), true))
            .find(|(_, hash)| *hash > b_hash)
            .expect("a seed orders a2 after b");
        for op in [a1, b, a2] {
            run(&env, op).await;
        }
        // W binds any value and then 1. The greedy search gives the first class (1) to the first
        // bind, and the second bind finds no other class of 1: W parks (decision 37).
        let parked = run(&env, join(&[(&s, any()), (&s, exactly(1))], "w", false)).await;
        assert!(parked.fires.is_empty());
        // V takes the class of 1: a1 leaves, and the persistent a2 stays. The class of 1 now
        // sits at the place of a2, after b, so the greedy search of W succeeds: the cascade of
        // the set fires W with b and a2 (decision 50).
        let outcome = run(&env, receive(&s, "v", exactly(1))).await;
        assert_eq!(fired(&outcome), vec![fire("v", &[]), fire("w", &[2])]);
        assert_eq!(payloads(&host, &s).await, vec![vec![int(1)]]);
        assert!(host
            .get_waiting_continuations(vec![data_of(&s), data_of(&s)])
            .await
            .is_empty());
    }

    /// The types that the engine reserves by size keep their sizes. `ScanMeter::buffer::<T>(n)`
    /// reserves `n * size_of::<T>()` bytes, and `is_claimed` reserves comparisons of
    /// `size_of::<Claim>()` bytes. So a field that grows one of these types changes the host work
    /// of every keyed join, also when the field is never used there. The store seams of the
    /// PathMap engine (joins plan section 9) must keep these values.
    #[cfg(target_pointer_width = "64")]
    #[test]
    fn space_join_engine_reserved_types_keep_their_sizes() {
        use std::mem::size_of;

        use crate::rust::interpreter::spaces::agent::PendingProduce;

        assert_eq!(size_of::<Work>(), 112, "Work");
        assert_eq!(size_of::<Incoming>(), 320, "Incoming");
        assert_eq!(size_of::<Bound>(), 176, "Bound");
        assert_eq!(size_of::<Option<Bound>>(), 176, "Option<Bound>");
        assert_eq!(size_of::<Claim>(), 56, "Claim");
        assert_eq!(size_of::<Entry>(), 32, "Entry");
        assert_eq!(size_of::<WaiterRef>(), 56, "WaiterRef");
        assert_eq!(size_of::<ScopedCollection>(), 616, "ScopedCollection");
        assert_eq!(size_of::<Seat>(), 72, "Seat");
        assert_eq!(size_of::<PendingProduce>(), 128, "PendingProduce");
    }
}
