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
//! driver.
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

use std::borrow::Cow;
// Changed by decision 124 (plan v4 C7.9, ruling RADD 1.5): a join row search finds a waiter by
// its source hash. The text before:
// use std::collections::{BTreeMap, BTreeSet};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, LazyLock};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::{BindPattern, CostAuthority, ListParWithRandom, Par, TaggedContinuation};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::internal::{ConsumeCandidate, Datum};
// Added by decision 124 (plan v4 C7.9): a row search matches in the store of the commit.
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::rspace_interface::RSpaceOperationSource;
// Changed by decision 124 (plan v4 C7.9, ruling OP-J): a join row search maps its errors to the
// errors of the commit. The text before:
// use rspace_plus_plus::rspace::space_store_handle::incoming_candidate;
// Changed by decision 124 (plan v4 C7.9, ruling RADD 1.5): the join row searches name these too.
use rspace_plus_plus::rspace::space_store_handle::{
    canonical_continuation_order, incoming_candidate, ActualFire, RowClaims, RowItemRule,
    RowReissue, RowRun, RowSearch, SpaceTxnError, TailRead,
};
// Changed by decision 102 (plan v4 C5.6): the consume of a waiter decides E3-a.
// use rspace_plus_plus::rspace::trace::event::Produce;
use rspace_plus_plus::rspace::trace::event::{Consume, Produce};

use super::agent::{
    CascadeState, ConsumeOp, JoinOp, OpOrigin, PendingProduce, Prepared, ProduceOp, SettleOp,
    SpaceEnv, SpaceOp, SpaceOutcome, SpaceReissue,
};
use super::anchor::SpaceJob;
use super::counter::CounterChannels;
// Changed by decision 128 (kind routing): the window owns the type. The text before:
// use super::disciplines::priority_queue::{receive_lane, send_level, Lane, Level};
use super::disciplines::lanes::{Lane, Level};
// Changed by decision 124 (plan v4 C7.9, ruling OP-J): a join row search maps its other errors
// as the discipline searches do. The text before:
// use super::disciplines::scan::{retire_class_on, retire_continuation, same_class};
// Changed by decision 124 (plan v4 C7.9, ruling RADD 1.5): the join row searches name these too.
use super::disciplines::scan::{
    class_members, commit_error, retire_class_on, retire_continuation, same_class, ClassKey,
    Member, ParConsume, RetireRows,
};
// Changed by decision 128 (kind routing): the window owns the types and the error texts. The
// text before:
// use super::disciplines::vectordb::{self, Flight, ListFire, Mode, VdbConfig};
use super::disciplines::similarity::{
    EligibleOrder, Flight, ListFire, Mode, SimilarityConfig, SimilarityQuery, BIND_OUTSIDE_WINDOW,
    LIST_OUTSIDE_WINDOW, RECORD_FROM_SEND_PARAMS,
};
// Changed by decision 124 (plan v4 C7.9, ruling RADD 1.5): the join row searches name these too.
// The text before:
// use super::ephemeral::{ClaimRole, SeqClaims};
use super::ephemeral::{BookPairs, ClaimRole, SeqClaims, SeqReads};
use super::errors::SpaceError;
// Changed by decision 102 (plan v4 C7.22): a produce-side fire names the datum that it offers.
// use super::generic::{
//     self, operation_ord, peek_set, peek_set_all, receive_candidate, Admission, CollectionTxn, Cx,
//     Discipline, GenericSpace, JoinWindow, Ordered, OuterStore, ParCandidate, ParDatum, ParWaiter,
//     ReceiveFire, Side, SideBound, StoredMatch,
// };
// Changed by decision 124 (plan v4 C7.9, ruling RADD 1.5): the join row searches name these too.
use super::generic::{
    self, operation_ord, peek_set, peek_set_all, receive_candidate, Admission, CollectionTxn,
    CommitView, Cx, Discipline, GenericSpace, JoinWindow, Offered, Ordered, OuterStore,
    ParCandidate, ParDatum, ParFinishRow, ParRowLists, ParRowRequest, ParRowSearch, ParWaiter,
    ReceiveFire, RowEnd, Side, SideBound, StagedRowSearch, StoreView, StoredMatch, TailEnd,
};
// Added by decision 124 (plan v4 C7.9, ruling RADD 1.5): a join row search reads the same veto
// rule as the staging.
use super::matching::may_veto;
use super::namespace::{self, CollectionKey, NativeSpace, RandomPurpose, ReifiedId};
use super::record::{self, Introduction, JoinSlot, OperationAuthority, WaiterPolicy};
// Added by decision 124 (plan v2 C7.10): the reader skips inert machinery waiters.
use super::settle::machinery_shape;
use super::settle::{lift_continuation, lift_pattern, settle_operation};
// Added by decision 124 (plan v4 C7.9, ruling RADD 1.5): a join row search runs the theory of the
// space (CR-16).
use super::theory::SpaceTheory;
use super::types::config::SpaceConfig;
// Changed by decision 124 (plan v4 C7.11, ruling RADD 1.2): the tail read counts the rounds of
// its fires. The text before:
// use crate::rust::interpreter::reified::costs::{FireStep, ReissueCause, FIRE_ROUND_CAP};
use crate::rust::interpreter::reified::costs::{
    FireRounds, FireStep, ReissueCause, FIRE_ROUND_CAP,
};
use crate::rust::interpreter::spaces::record::bincode_digest;
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
    /// in canonical group order, the space counter and the lock keys.
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
    /// `locked` covers the closure, so the engine runs in it with no escalation.
    pub fn covers(&self, locked: &[CollectionKey]) -> bool {
        if self.collections.len() > locked.len() {
            return false;
        }
        for collection in &self.collections {
            let mut found = false;
            for key in locked {
                if collection.key == *key {
                    found = true;
                    break;
                }
            }
            if !found {
                return false;
            }
        }
        true
    }

    /// The index of the collection `key` in the scope.
    fn index_of_key(&self, key: &CollectionKey) -> Result<usize, SpaceError> {
        self.collections
            .binary_search_by(|collection| collection.key.cmp(key))
            .map_err(|_| SpaceError::Inconsistent("a collection is outside its join scope"))
    }

    /// The index of the collection whose data channel is `channel`: a data channel of the space,
    /// or in the scope of a PathMap operation an index channel `I(q)` of the space, whose key is
    /// `[0x49] ‖ prefix_key(q)`.
    /// Another channel is `Inconsistent`.
    fn index_of_channel(&self, channel: &Par) -> Result<usize, SpaceError> {
        match namespace::reified_id_of(channel) {
            Some(ReifiedId::PathIndex(owner, tail)) if self.prefixes && owner == self.space => self
                .collections
                .binary_search_by(|collection| match collection.key.as_bytes().split_first() {
                    Some((&PREFIX_KEY_TAG, rest)) => rest.cmp(tail),
                    Some((&first, _)) => first.cmp(&PREFIX_KEY_TAG),
                    None => std::cmp::Ordering::Less,
                })
                .map_err(|_| {
                    SpaceError::Inconsistent("a join group holds a collection outside its scope")
                }),
            Some(ReifiedId::Data(owner, tail)) if !self.prefixes && owner == self.space => self
                .collections
                .binary_search_by(|collection| collection.key.as_bytes().cmp(tail))
                .map_err(|_| {
                    SpaceError::Inconsistent("a join group holds a collection outside its scope")
                }),
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
/// the data channel.
pub(crate) fn scoped_collection<S: OuterStore>(
    outer: &S,
    space: NativeSpace,
    key: CollectionKey,
) -> ScopedCollection {
    ScopedCollection {
        data: outer.data_channel(space, &key),
        counter: outer.counter_channel(space, &key),
        key,
    }
}

/// The distinct collections of the binds of the join `op`, in ascending key order.
pub(crate) fn distinct_bind_keys(op: &JoinOp) -> Vec<CollectionKey> {
    let mut keys: BTreeSet<CollectionKey> = BTreeSet::new();
    for bind in &op.binds {
        if !keys.contains(&bind.key) {
            keys.insert(bind.key.clone());
        }
    }
    let mut ordered = Vec::<CollectionKey>::with_capacity(keys.len());
    ordered.extend(keys);
    ordered
}

/// CLASSIFY (joins plan section 4.1): `Single` for a produce, a consume or a settle without a
/// cascade whose data channel holds no join group other than `[D(sid,k)]`, and otherwise `Join`
/// with the closure.
pub(crate) fn classify<S: OuterStore, D: Discipline>(
    agent: &GenericSpace<S, D>,
    opened: &mut CollectionTxn,
    op: &SpaceOp,
) -> Result<JoinRoute, SpaceError> {
    let always = match op {
        SpaceOp::Join(_) => true,
        SpaceOp::Settle(settle) => settle.cascade.is_some(),
        SpaceOp::Produce(_) | SpaceOp::Consume(_) => false,
    };
    if !always {
        let data = opened.channels.data();
        let single = match opened.txn.joins(data)? {
            [] => true,
            [group] => group.as_slice() == std::slice::from_ref(data),
            [_, _, ..] => false,
        };
        if single {
            return Ok(JoinRoute::Single);
        }
    }
    join_scope(agent, opened, op).map(JoinRoute::Join)
}

/// The checks of an operation before any staging: the routed key of each bind with the outer
/// store (`OuterStore::check_key`), the payload of a send and the pattern of each bind with the
/// outer store (`OuterStore::check_payload`, `OuterStore::check_patterns`: the unit rules of a
/// HashSet store, P7 HS-2 and HS-3), and the params and the query with the schema of the
/// discipline. A join needs two or more binds, one arrow, and one carried order for each bind
/// when it carries orders.
fn validate<S: OuterStore, D: Discipline>(
    agent: &GenericSpace<S, D>,
    config: &SpaceConfig,
    op: &SpaceOp,
) -> Result<(), SpaceError> {
    let no_params = BTreeMap::new();
    match op {
        SpaceOp::Produce(op) => {
            agent.outer().check_key(config, &op.channel, &op.key)?;
            agent.outer().check_payload(&op.data)?;
            agent.check_params(config, &op.params, None)
        }
        SpaceOp::Consume(op) => {
            agent.outer().check_key(config, &op.channel, &op.key)?;
            agent.outer().check_patterns(&op.pattern)?;
            agent.check_params(config, &no_params, op.query.as_ref())
        }
        SpaceOp::Settle(op) => {
            agent.outer().check_key(config, &op.channel, &op.key)?;
            agent.check_params(config, &no_params, None)
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
                agent.outer().check_key(config, &bind.channel, &bind.key)?;
                agent.outer().check_patterns(&bind.pattern)?;
                agent.check_params(config, &no_params, bind.query.as_ref())?;
                // A list is one value of one bind (decision 66): a VectorDB bind of a join is in
                // single mode.
                if let (JoinWindow::Similarity, Some(query)) = (D::JOIN_WINDOW, &bind.query) {
                    // Changed by decision 128 (kind routing): the discipline of the source gives
                    // the step. The text before:
                    // if vectordb::is_list_query(query) {
                    if D::is_list_query(query) {
                        return Err(SpaceError::ListModeInJoin);
                    }
                }
            }
            Ok(())
        }
    }
}

/// The positions of the binds of `op` in canonical group order (FR-1'): ascending by the stable
/// hash of the data channel of the collection of each bind, then by bind index. A produce, a
/// consume and a settle have one position.
fn group_order<S: OuterStore>(outer: &S, op: &SpaceOp) -> Result<Vec<usize>, SpaceError> {
    let SpaceOp::Join(join) = op else {
        let mut group = Vec::<usize>::with_capacity(1);
        group.push(0);
        return Ok(group);
    };
    let mut hashed = Vec::<(Blake2b256Hash, usize)>::with_capacity(join.binds.len());
    for (index, bind) in join.binds.iter().enumerate() {
        let data = outer.data_channel(join.space, &bind.key);
        hashed.push((bincode_digest(&data)?, index));
    }
    hashed.sort();
    let mut group = Vec::<usize>::with_capacity(hashed.len());
    group.extend(hashed.into_iter().map(|(_, index)| index));
    Ok(group)
}

/// JOIN-SCOPE (joins plan section 4.1): the checks of the operation, then the closure of the own
/// collections and the seeds under the join groups of their data channels, and the lock keys of
/// the closure. A group channel that is not a data channel of the space is `Inconsistent`.
fn join_scope<S: OuterStore, D: Discipline>(
    agent: &GenericSpace<S, D>,
    opened: &mut CollectionTxn,
    op: &SpaceOp,
) -> Result<JoinScope, SpaceError> {
    validate(agent, &opened.config, op)?;
    let space = op.space();
    let group = group_order(agent.outer(), op)?;
    let mut own = Vec::<CollectionKey>::with_capacity(group.len());
    let mut seeds: Vec<CollectionKey> = Vec::new();
    match op {
        SpaceOp::Produce(produce) => own.push(produce.key.clone()),
        SpaceOp::Consume(consume) => own.push(consume.key.clone()),
        SpaceOp::Settle(settle) => {
            own.push(settle.key.clone());
            if let Some(cascade) = &settle.cascade {
                let extra = cascade.seeds.len() + cascade.pending.len();
                seeds = Vec::<CollectionKey>::with_capacity(extra);
                for key in &cascade.seeds {
                    seeds.push(key.clone());
                }
                for pending in &cascade.pending {
                    seeds.push(pending.key.clone());
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
                    if *key == bind.key {
                        seen = true;
                        break;
                    }
                }
                if !seen {
                    own.push(bind.key.clone());
                }
            }
        }
    }
    let mut todo: BTreeSet<CollectionKey> = BTreeSet::new();
    for key in own.iter().chain(seeds.iter()) {
        if !todo.contains(key) {
            todo.insert(key.clone());
        }
    }
    let mut closure: BTreeMap<CollectionKey, ScopedCollection> = BTreeMap::new();
    while let Some(key) = todo.pop_first() {
        if closure.contains_key(&key) {
            continue;
        }
        let collection = scoped_collection(agent.outer(), space, key.clone());
        let mut reached: Vec<CollectionKey> = Vec::new();
        for group in opened.txn.joins(&collection.data)? {
            for channel in group {
                match namespace::reified_id_of(channel) {
                    Some(ReifiedId::Data(owner, tail)) if owner == space => {
                        let reached_key = CollectionKey::from_bytes(tail.to_vec())
                            .ok_or(SpaceError::Inconsistent("a collection key is empty"))?;
                        reached.push(reached_key);
                    }
                    _ => {
                        return Err(SpaceError::Inconsistent(
                            "a collection channel is part of a foreign join",
                        ))
                    }
                }
            }
        }
        closure.insert(key, collection);
        for reached_key in reached {
            if closure.contains_key(&reached_key) {
                continue;
            }
            if !todo.contains(&reached_key) {
                todo.insert(reached_key);
            }
        }
    }
    let lock_length = closure.len() * 2 + 2;
    let mut lock_keys = Vec::<Par>::with_capacity(lock_length);
    lock_keys.push(opened.channels.config().clone());
    lock_keys.push(opened.channels.space_counter().clone());
    let mut collections = Vec::<ScopedCollection>::with_capacity(closure.len());
    for (_, collection) in closure {
        lock_keys.push(collection.counter.clone());
        lock_keys.push(collection.data.clone());
        collections.push(collection);
    }
    let space_counter = opened.channels.space_counter().clone();
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
    let CollectionTxn { txn, config, .. } = opened;
    let authority = introduction_authority(&op).clone();
    let first = scope.own.first().ok_or(SpaceError::Inconsistent(
        "a join scope has no own collection",
    ))?;
    let own_index = scope.index_of_key(first)?;
    let data = &scope
        .collections
        .get(own_index)
        .ok_or(SpaceError::Inconsistent(
            "a collection is outside its join scope",
        ))?
        .data;
    // TH-5: the theory admits the values of a send before any staging or bump.
    let admission = Admission::of(&config, &op)?;
    let seq = own_claims(env, &config, &scope, &op)?;
    // Changed by decision 92: the plan takes the job of the transaction (`spaces::anchor`).
    // generic::run_plan(env, txn, data, &config, &authority, seq, admission, |cx| {
    //     let engine = Engine::new(source, &scope, &op, cx.config())?;
    //     engine.run(cx, op)
    // })
    let job = SpaceJob::of(&op);
    // Changed by decision 102 (plan v4 C4.3, rule CR-14): R-A can stage the operation twice. The
    // rule of `may_restage` is the rule of `GenericSpace::run_with`. The first call of the
    // closure takes the operation, and the restage takes the spare copy after the source reset
    // its state of the first staging (F6).
    let may_restage = source.random_picks()
        && (source.bound(&config, Side::Data)?.is_some()
            || admission.theory.is_some()
            || seq.is_some());
    let mut spare = may_restage.then(|| op.clone());
    let mut first = Some(op);
    generic::run_plan(
        env,
        txn,
        data,
        &config,
        &authority,
        seq,
        admission,
        &job,
        may_restage,
        |cx| {
            let op = match first.take() {
                Some(op) => op,
                None => {
                    source.restart();
                    spare.take().ok_or(SpaceError::Inconsistent(
                        "a join stages again without a spare operation",
                    ))?
                }
            };
            let engine = Engine::new(source, &scope, &op, cx.config())?;
            engine.run(cx, op)
        },
    )
}

/// SQ-3 (P7): the claims of a program operation on a `seq` space with its own channels claimed
/// first, or `None` for every other job. A send claims the home of its datum, the data channel
/// `D(sid, key)` of its key (the data channel of its path in a PathMap space). A receive or a
/// join claims the channel of each own collection as a waiter channel: `D(sid, k)` of a keyed
/// store, `I(sid, q)` of a PathMap prefix. A settle job is internal.
fn own_claims(
    env: &SpaceEnv,
    config: &SpaceConfig,
    scope: &JoinScope,
    op: &SpaceOp,
) -> Result<Option<SeqClaims>, SpaceError> {
    // Changed by decision 124 (plan v4 C7.14, ruling OP2-25 option B): a re-issue is internal
    // with or without its anchor first. The text before:
    // let internal = matches!(op, SpaceOp::Settle(_)) || env.origin == OpOrigin::Internal;
    let internal =
        matches!(op, SpaceOp::Settle(_)) || matches!(env.origin, OpOrigin::Internal { .. });
    // Changed by decision 91: the claims also take the random state of the own item of the
    // operation, whose history learns the uses at the commit.
    // let Some(mut claims) = SeqClaims::open(&env.volatile, op.space(), config.qualifier, internal)?
    let item: &[u8] = match op {
        SpaceOp::Produce(produce) => &produce.data.random_state,
        SpaceOp::Consume(consume) => &consume.body.random_state,
        SpaceOp::Join(join) => &join.body.random_state,
        SpaceOp::Settle(settle) => &settle.random_state,
    };
    let Some(mut claims) =
        SeqClaims::open(&env.volatile, op.space(), config.qualifier, internal, item)?
    else {
        return Ok(None);
    };
    match op {
        SpaceOp::Produce(produce) => {
            let home = namespace::gprivate(namespace::data_channel(produce.space, &produce.key));
            claims.claim(&home, ClaimRole::Data)?;
        }
        SpaceOp::Consume(_) | SpaceOp::Join(_) => {
            for key in &scope.own {
                let index = scope.index_of_key(key)?;
                let collection = scope
                    .collections
                    .get(index)
                    .ok_or(SpaceError::Inconsistent(
                        "a collection is outside its join scope",
                    ))?;
                claims.claim(&collection.data, ClaimRole::Waiter)?;
            }
        }
        SpaceOp::Settle(_) => {}
    }
    Ok(Some(claims))
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
// Added by decision 124 (plan v4 C7.9, ruling OP-J): a row search keeps a copy of the element
// that its fire offers.
#[derive(Clone)]
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

impl Incoming {
    /// Added by decision 102 (plan v4 C7.22, rule CR-19 case 3): the datum that a fire of the
    /// element offers. An element in flight is [`Offered::InFlight`]. A persistent record that
    /// the cascade re-offers stays stored, so it is [`Offered::StoredPersistent`], and its fire
    /// is not the job's own (`SpaceJob::owns_first_reoffer`).
    pub(crate) fn offered(&self) -> Offered {
        match self.in_flight {
            true => Offered::InFlight,
            false => Offered::StoredPersistent,
        }
    }
}

/// What a bound entry claims, so that no other bind of the fire binds it (decision 28): one
/// record, or one class of a set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Claim {
    /// The record at `at` of the collection `coll`.
    Record { coll: usize, at: EntryAt },
    /// The class `key` of the collection `coll`, with the hash of the key (DR-90).
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
    /// Added by decision 103 (decision 118, rule CR-9): the claim of this record alone. Every
    /// non-persistent entry claims it when it binds, and a non-persistent set member claims its
    /// class too. A persistent entry claims nothing.
    pub(crate) record: Claim,
}

/// The fire sites of the engine that spend stored entries (joins plan section 9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FireSite {
    /// A produce-side fire of the element in flight (OFFER).
    Offer,
    /// A consume-side fire of a join waiter of the operation (RECEIVE).
    Join,
    /// A consume-side fire of a single-bind waiter of the operation (RECEIVE). Changed by
    /// decision 101 (rule CR-13): the fire binds one entry over `[D]`, the first member in DR-75
    /// order of a set class, and RETIRE-SET-BINDS retires the other members of the class after
    /// it. The text before: "(RECEIVE, F3 and F20)."
    Single,
    /// A fire of a stored pair by the cascade (LIFT-AND-REOFFER).
    Cascade,
}

/// Added by decision 102 (plan v4 C5.3, rules CR-1, CR-3 and CR-30 F3): the order of a window.
/// A receive and an offer pick with the source of the operation that their fire stages. The
/// cascade search and the membership test of SEARCH-PRODUCE keep DR-75.
#[derive(Clone, Copy, Debug)]
pub(crate) enum WindowOrder<'s> {
    Canonical,
    Pick(RSpaceOperationSource<'s>),
}

impl<'s> WindowOrder<'s> {
    /// Added by decision 124 (J1 review D4, L4): the order of a search that picks with the
    /// produce `source`, or DR-75 when the source of the windows builds no produce
    /// ([`JoinSource::WINDOWS_READ_ORDER`]).
    fn of_produce(source: Option<&'s Produce>) -> WindowOrder<'s> {
        match source {
            Some(source) => WindowOrder::Pick(RSpaceOperationSource::Produce(source)),
            None => WindowOrder::Canonical,
        }
    }

    /// Added by decision 102 (plan v4 C5.3): the order of the eligible records of a similarity
    /// bind, with the same name.
    fn eligible(self) -> EligibleOrder<'s> {
        match self {
            WindowOrder::Canonical => EligibleOrder::Canonical,
            WindowOrder::Pick(source) => EligibleOrder::Pick(source),
        }
    }
}

/// Added by decision 103 (decision 118 E1, rule CR-8, plan v4:1130-1134): `window` with the
/// element in flight first, and the other entries in their order.
pub(crate) fn x_first(window: Vec<Entry>) -> Vec<Entry> {
    let mut ordered = Vec::<Entry>::with_capacity(window.len());
    ordered.extend(
        window
            .iter()
            .filter(|entry| matches!(entry.at, EntryAt::Incoming))
            .copied(),
    );
    ordered.extend(
        window
            .iter()
            .filter(|entry| !matches!(entry.at, EntryAt::Incoming))
            .copied(),
    );
    ordered
}

/// Added by decision 102 (plan v4 C5.3) and decision 124 (LP-1, L5; J1 ruling 3): the waiters
/// `waiters` of the data channel `channel`, which `union_order` gives in the canonical order of
/// DR-75 of the union of its join groups, in the order of a pick of the operation `source`
/// ([`Cx::pick_union_order`]).
pub(crate) fn pick_waiters(
    cx: &mut Cx<'_, '_>,
    channel: &Par,
    waiters: Vec<WaiterRef>,
    source: RSpaceOperationSource<'_>,
) -> Result<Vec<WaiterRef>, SpaceError> {
    let groups: Vec<Vec<Par>> = cx.joins_on(channel)?.to_vec();
    let mut union = Vec::<(usize, usize)>::with_capacity(waiters.len());
    for waiter in &waiters {
        let group = groups
            .iter()
            .position(|group| *group == waiter.group)
            .ok_or(SpaceError::Inconsistent(
                "a waiter names no join group of its channel",
            ))?;
        union.push((group, waiter.index));
    }
    let order = cx.pick_union_order(&groups, &union, source)?;
    let mut slots: Vec<Option<WaiterRef>> = waiters.into_iter().map(Some).collect();
    let mut picked = Vec::<WaiterRef>::with_capacity(order.len());
    for position in order {
        let waiter =
            slots
                .get_mut(position)
                .and_then(Option::take)
                .ok_or(SpaceError::Inconsistent(
                    "a pick names a waiter twice or outside the union",
                ))?;
        picked.push(waiter);
    }
    Ok(picked)
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
    /// Added by decision 128 (kind routing): the discipline of the source. The engine calls the
    /// steps of the window of the source through it.
    type Discipline: Discipline;

    /// The window shape of the discipline.
    fn shape(&self) -> JoinWindow;

    // Replaced by P6 (design v3 4.0): the bound of a side may depend on the config of the
    // space, so the source gives the bound of each side.
    // /// The most entries of each side of a collection, or `None` (F15).
    // fn capacity(&self) -> Option<usize>;

    /// The bound of `side` of a collection of a space with the config `config`, or `None` (F15,
    /// P6).
    fn bound(&self, config: &SpaceConfig, side: Side) -> Result<Option<SideBound>, SpaceError>;

    /// Added by decision 102 (plan v4 C4.3): [`Discipline::RANDOM_PICKS`] of the discipline of
    /// the source.
    fn random_picks(&self) -> bool;

    /// Added by decision 124 (J1 review D4, L4): true when the windows of the source read their
    /// [`WindowOrder`], so a search of SEARCH-PRODUCE picks with the produce that its fire
    /// stages. A source whose windows keep DR-75 in both roles answers false. SEARCH-PRODUCE then
    /// builds no produce for the pick and passes [`WindowOrder::Canonical`]. The default is true.
    const WINDOWS_READ_ORDER: bool = true;

    /// Added by decision 102 (plan v4 C4.3, rule CR-14, F6): resets the state that the source
    /// keeps for one staging, before the restage of R-A. The default keeps no state.
    fn restart(&self) {}

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
    ) -> Result<ListParWithRandom, SpaceError>;

    /// The order and the level of the record `record`.
    fn record_slots(&self, record: &ListParWithRandom) -> Result<(i64, Option<Level>), SpaceError>;

    /// The payload of the record `record`, for matching.
    fn payload_of(&self, record: &ListParWithRandom) -> Result<ListParWithRandom, SpaceError>;

    /// The slots of a stored waiter whose group has `width` channels: one slot for each group
    /// position.
    fn waiter_slots(&self, waiter: &ParWaiter, width: usize) -> Result<Vec<JoinSlot>, SpaceError>;

    /// The window of `need` binds on the collection `coll` (joins plan section 3.2), with the
    /// element in flight `incoming` at its place when it belongs to `coll`. `pending` are the
    /// second sends that the transaction still owes: a source whose records stay stored while
    /// they are in flight leaves them out of every window (the PathMap source).
    // Added by decision 102 (plan v4 C5.3): eight inputs with `self`.
    #[allow(clippy::too_many_arguments)]
    // Changed by decision 124 (plan v4 C7.9, ruling OP-J): the reads of this fn go through the
    // view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn window(
    fn window<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        scope: &JoinScope,
        coll: usize,
        need: usize,
        incoming: Option<&Incoming>,
        pending: &[Work],
        // Added by decision 102 (plan v4 C5.3): the order of the window.
        order: WindowOrder<'_>,
    ) -> Result<Vec<Entry>, SpaceError>;

    /// The claim of the entry `at` of `coll` with the payload `payload`.
    fn claim(
        &self,
        coll: usize,
        at: EntryAt,
        payload: &ListParWithRandom,
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

    /// Added by decision 124 (F2, base E3-a, selection.rs:747-751, and one row source for each
    /// produce, replay_rspace.rs:1058-1082): true when the recorded row of the offer of the
    /// element `x` is a Stored row, so replay skips each waiter whose commit may veto (rule
    /// CR-7). The default reads the row of `x.source`, the one source of the fires and of the
    /// store of an offer whose `incoming_source` is `x.source`. A source whose offer spreads its
    /// rows over several sources overrides it.
    fn offer_stored(
        &self,
        cx: &mut Cx<'_, '_>,
        _scope: &JoinScope,
        x: &Incoming,
    ) -> Result<bool, SpaceError> {
        Ok(cx.recorded_stored(RSpaceOperationSource::Produce(&x.source)))
    }

    /// The heads of the collection `coll` that a cascade tests, oldest first. A discipline that
    /// scans has none.
    // Changed by decision 124 (plan v4 C7.11, ruling RADD 1.2): the reads of this fn go through
    // the view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn cascade_order(
    fn cascade_order<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        scope: &JoinScope,
        coll: usize,
    ) -> Result<Vec<WaiterRef>, SpaceError>;

    /// AT-HEAD (joins plan section 3.3): true when the stored waiter `waiter`, with the slots
    /// `slots`, is the head of the waiters of `coll` in each of its lanes there.
    // Changed by decision 124 (plan v4 C7.9, ruling OP-J): the reads of this fn go through the
    // view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn at_head(
    fn at_head<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
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
    fn class_key(&self, payload: &[Par]) -> Result<Option<Vec<u8>>, SpaceError>;

    /// The stored entry `index` of the window of `coll` as a bind sees it: its payload, its
    /// persistence and its claim. `missing` is the error of an index outside the staged view.
    // Changed by decision 124 (plan v4 C7.9, ruling OP-J): the reads of this fn go through the
    // view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn stored_entry(
    fn stored_entry<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        scope: &JoinScope,
        coll: usize,
        index: usize,
        missing: &'static str,
    ) -> Result<StoredEntry, SpaceError>;

    /// The payload of the stored entry `index` of `coll` as its send carried it (P7, TH-6): the
    /// payload that the theory of the space checks after the guard. On a PathMap space it is the
    /// own payload of the datum of the marker, not re-rooted. `missing` is the error of an index
    /// outside the staged view.
    // Changed by decision 124 (plan v4 C7.9, ruling OP-J): the reads of this fn go through the
    // view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn stored_payload(
    fn stored_payload<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        scope: &JoinScope,
        coll: usize,
        index: usize,
        missing: &'static str,
    ) -> Result<ListParWithRandom, SpaceError>;

    /// Added by decision 124 (rule CR-20): the identity that an offer of the stored entry `index`
    /// of `coll` names at its position, when the record on the collection has another identity.
    /// A refusal of that offer books its pair under the identity of the offer
    /// (`incoming_source`), so a later fire of the entry tests this identity too. The default
    /// answers none: a stored record is the record that its offer named.
    fn stored_offer_source<V: StoreView>(
        &self,
        _view: &mut V,
        _scope: &JoinScope,
        _coll: usize,
        _index: usize,
    ) -> Result<Option<Produce>, SpaceError> {
        Ok(None)
    }

    /// The stored entries of `coll` that a single-bind class fire visits, in canonical order
    /// (F3, F20).
    // Changed by decision 124 (plan v4 C7.9, rulings OP-J and RADD 1.5): the reads of this fn go
    // through the view `view` in place of `cx`. The comment above a changed line keeps its text
    // before.
    // fn members(
    fn members<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
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
    // Changed by decision 124 (plan v4 C7.11, ruling RADD 1.2): the reads of this fn go through
    // the view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn second_send(
    fn second_send<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        scope: &JoinScope,
        coll: usize,
        index: usize,
        entry: &Bound,
    ) -> Result<Option<Work>, SpaceError>;

    /// The identity of the incoming side of a fire of the element `x` at a position on `coll`:
    /// the produce that the fire stages there.
    // Changed by decision 124 (plan v4 C7.9, ruling OP-J): the reads of this fn go through the
    // view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn incoming_source(
    fn incoming_source<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        scope: &JoinScope,
        x: &Incoming,
        coll: usize,
    ) -> Result<Produce, SpaceError>;

    /// The data of the incoming side of a fire of the element `x` at a position on `coll`.
    // Changed by decision 124 (plan v4 C7.9, ruling OP-J): the reads of this fn go through the
    // view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn incoming_record(
    fn incoming_record<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        scope: &JoinScope,
        x: &Incoming,
        coll: usize,
    ) -> Result<ListParWithRandom, SpaceError>;

    /// True when the element `x` enters the window of `coll`: its own collection, or on a
    /// PathMap space every prefix of its path.
    fn enters(&self, scope: &JoinScope, x: &Incoming, coll: usize) -> Result<bool, SpaceError>;

    /// The payload of the element `x` as a bind on `coll` sees it: its own payload, or on a
    /// PathMap space the payload re-rooted under the suffix of its path below the prefix `coll`.
    fn incoming_payload<'x>(
        &self,
        scope: &JoinScope,
        x: &'x Incoming,
        coll: usize,
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
    /// Added by decision 124 (errata V4-17): the row of the identity of `x` carries the search
    /// `search` of an E3-a store. Every other store passes `None`.
    fn land(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        x: Incoming,
        dirty: &mut [bool],
        // Added by decision 124 (errata V4-17): the search of an E3-a store.
        search: Option<ParRowSearch>,
    ) -> Result<(), SpaceError>;

    /// What a fire at `site` spends, recorded before the fire moves its bound entries `bound` on
    /// the positions `colls`. `x` is the element in flight of a produce-side fire. Changed by
    /// decision 101 (rule CR-13): a single-bind fire binds one entry, so its `bound` holds it, and
    /// the `members` argument is retired. The text before: "`members` are the candidates of a
    /// single-bind class fire, and `x` is the element in flight of a produce-side fire."
    // Retired by decision 101 (rule CR-13): the hook takes seven arguments now.
    // #[allow(clippy::too_many_arguments)]
    // Changed by decision 124 (plan v4 C7.9, ruling OP-J): the reads of this fn go through the
    // view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn before_fire(
    fn before_fire<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        scope: &JoinScope,
        site: FireSite,
        colls: &[usize],
        bound: &[Bound],
        // Retired by decision 101 (rule CR-13): a single-bind fire binds one entry.
        // members: &[ParCandidate],
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

    /// Added by decision 124 (plan v4 C7.9, ruling RADD 1.5): the row search of a fire of this
    /// kind, over the parts `row` that the engine copied at the fire, with what the fire spends
    /// (`spent`, `None` for a fire that peeks). The search builds the source of this kind at the
    /// commit and the `K_retire` requests of the actual fire. The engine calls it only for an
    /// owner-funded run (RC2 D-10). No default: each kind builds a search of its own type.
    fn row_search(
        &self,
        cx: &mut Cx<'_, '_>,
        row: JoinRow,
        spent: Option<&Self::Spent>,
    ) -> Result<ParRowSearch, SpaceError>;

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD2v2 6.4 and 6.6): the row
    /// search of a cascade lift of this kind, over the parts `lift` that the engine copied at the
    /// lift. The search is the machinery search of the lift with the end of this kind
    /// (`LiftRow::into_parts`, `StagedRowSearch::with_end`, `RowEnd`), so it tests the lift as
    /// that search does (CR-16). The search ends Tail(actual) of a refused lift with the stop of
    /// the engine at the lift (`refused_lift_end`), and it gives the finish rows of the source at
    /// the lift (P1). The engine calls it only for an owner-funded run (RC2 D-10).
    fn lift_search(&self, lift: LiftRow<Self::Discipline>) -> ParRowSearch;
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
    fn group_colls(&self, scope: &JoinScope, group: &[Par]) -> Result<Vec<usize>, SpaceError> {
        let mut colls = Vec::<usize>::with_capacity(group.len());
        for channel in group {
            colls.push(scope.index_of_channel(channel)?);
        }
        Ok(colls)
    }

    /// The seats of the waiters of `coll`: each waiter of each join group of its data channel,
    /// with its order and its lanes on `coll`.
    // Changed by decision 124 (plan v4 C7.9, ruling OP-J): the reads of this fn go through the
    // view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn seats(
    fn seats<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        scope: &JoinScope,
        coll: usize,
    ) -> Result<Seats, SpaceError> {
        let channel = &collection(scope, coll)?.data;
        let groups: Vec<Vec<Par>> = {
            // let joins = cx.joins_on(channel)?;
            let joins = view.joins_on(channel)?;
            joins.to_vec()
        };
        let mut seats: Vec<Seat> = Vec::new();
        for (group_index, group) in groups.iter().enumerate() {
            let colls = self.group_colls(scope, group)?;
            let mut positions = Vec::<usize>::with_capacity(colls.len());
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
            // let count = cx.group_waiters(group)?.len();
            let count = view.group_waiters(group)?.len();
            for index in 0..count {
                let (slots, source) = {
                    // let waiters = cx.group_waiters(group)?;
                    let waiters = view.group_waiters(group)?;
                    let waiter = waiters.get(index).ok_or(SpaceError::Inconsistent(
                        "a waiter index is outside the staged view",
                    ))?;
                    // Added by decision 124 (plan v2 C7.10, decision 100): an inert machinery
                    // waiter is no candidate.
                    match machinery_shape(waiter) {
                        false => {}
                        true => continue,
                    }
                    let slots = self.waiter_slots(waiter, group.len())?;
                    (slots, waiter.source.hash.clone())
                };
                let ord = slot_at(&slots, first)?.ord;
                let mut lanes: Vec<Option<Lane>> = Vec::new();
                for &position in &positions {
                    let slot = slot_at(&slots, position)?;
                    if slot.ord != ord {
                        return Err(SpaceError::Inconsistent(
                            "a waiter has two orders in one collection",
                        ));
                    }
                    let lane = self.lane_of(slot);
                    if !lanes.contains(&lane) {
                        lanes.push(lane);
                    }
                }
                seats.push(Seat {
                    group: group_index,
                    index,
                    source,
                    ord,
                    lanes,
                });
            }
        }
        Ok(Seats { groups, seats })
    }

    /// The head of the seats in the lane `lane` (every seat for `None`): the seat whose order
    /// interacts first by the head rule of the discipline (`head`).
    fn head_seat(&self, seats: &[Seat], lane: Option<Lane>) -> Result<Option<usize>, SpaceError> {
        let mut members = Vec::<usize>::with_capacity(seats.len());
        members.extend(
            seats
                .iter()
                .enumerate()
                .filter(|(_, seat)| lane.is_none() || seat.lanes.contains(&lane))
                .map(|(index, _)| index),
        );
        let mut ordered = Vec::<Ordered<'_, usize>>::with_capacity(members.len());
        for member in &members {
            let seat = seats
                .get(*member)
                .ok_or(SpaceError::Inconsistent("a seat index is outside its list"))?;
            ordered.push(Ordered {
                ord: seat.ord,
                entry: member,
            });
        }
        let head = self.discipline.head(Side::Waiters, &ordered)?;
        Ok(head.and_then(|position| members.get(position).copied()))
    }

    /// The reference of the seat `seat`.
    fn reference(&self, seats: &Seats, seat: usize) -> Result<WaiterRef, SpaceError> {
        let seat = seats
            .seats
            .get(seat)
            .ok_or(SpaceError::Inconsistent("a seat index is outside its list"))?;
        let group = seats
            .groups
            .get(seat.group)
            .ok_or(SpaceError::Inconsistent("a seat names no group"))?;
        Ok(WaiterRef {
            group: group.clone(),
            index: seat.index,
            source: seat.source.clone(),
        })
    }

    /// The lanes of the slots `slots` on the collection `coll` of the positions `colls`, each
    /// once.
    fn lanes_on(&self, slots: &[JoinSlot], colls: &[usize], coll: usize) -> Vec<Option<Lane>> {
        let mut lanes: Vec<Option<Lane>> = Vec::new();
        for (slot, &other) in slots.iter().zip(colls) {
            if other != coll {
                continue;
            }
            let lane = self.lane_of(slot);
            if !lanes.contains(&lane) {
                lanes.push(lane);
            }
        }
        lanes
    }

    /// The order of the place of an element with the order `ord` in a side whose stored orders
    /// are `stored` (joins plan section 3.1): a fresh element is the head when
    /// `Discipline::reaches_head` says so, else the tail. An element with a carried order is
    /// placed after every stored entry that it does not precede. An equal order is
    /// `Inconsistent`.
    fn place(
        &self,
        side: Side,
        placement: Placement,
        ord: i64,
        stored: &[i64],
    ) -> Result<usize, SpaceError> {
        match placement {
            Placement::Fresh => Ok(match self.discipline.reaches_head(side, stored.len()) {
                true => 0,
                false => stored.len(),
            }),
            Placement::Carried => {
                let mut ahead = 0;
                for &other in stored {
                    if other == ord {
                        return Err(SpaceError::Inconsistent(
                            "an element in flight shares its order with a stored entry",
                        ));
                    }
                    if !self.discipline.precedes(side, ord, other)? {
                        ahead += 1;
                    }
                }
                Ok(ahead)
            }
        }
    }

    /// The first `need` entries of `entries` (order, index) in the head order of the discipline:
    /// the head, then the head of the rest, and so on.
    fn head_order(&self, entries: &[(i64, usize)], need: usize) -> Result<Vec<usize>, SpaceError> {
        let rounds = need.min(entries.len());
        let mut taken = Vec::<bool>::with_capacity(entries.len());
        taken.resize(entries.len(), false);
        let mut order = Vec::<usize>::with_capacity(rounds);
        for _ in 0..rounds {
            let mut remaining = Vec::<usize>::with_capacity(entries.len());
            remaining.extend(
                (0..entries.len())
                    .filter(|&position| !taken.get(position).copied().unwrap_or(true)),
            );
            let mut ordered = Vec::<Ordered<'_, usize>>::with_capacity(remaining.len());
            for position in &remaining {
                let (ord, index) = entries.get(*position).ok_or(SpaceError::Inconsistent(
                    "an entry index is outside its list",
                ))?;
                ordered.push(Ordered {
                    ord: *ord,
                    entry: index,
                });
            }
            let Some(head) = self.discipline.head(Side::Data, &ordered)? else {
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
    ) -> Result<Vec<Entry>, SpaceError> {
        let order = self.head_order(records, need)?;
        let length = order.len() + 1;
        let mut window = Vec::<Entry>::with_capacity(length);
        window.extend(order.into_iter().map(|index| Entry {
            at: EntryAt::Stored(index),
            level: None,
        }));
        if let Some(x) = incoming {
            let mut ords = Vec::<i64>::with_capacity(records.len());
            ords.extend(records.iter().map(|(ord, _)| *ord));
            let place = self.place(Side::Data, x.placement, x.ord, &ords)?;
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
    // Added by decision 128 (kind routing).
    type Discipline = D;

    fn shape(&self) -> JoinWindow { D::JOIN_WINDOW }

    // Replaced by P6 (design v3 4.0): the bound of the discipline under the config.
    // fn capacity(&self) -> Option<usize> { D::CAPACITY }

    fn bound(&self, config: &SpaceConfig, side: Side) -> Result<Option<SideBound>, SpaceError> {
        self.discipline.bound(config, side)
    }

    fn random_picks(&self) -> bool { D::RANDOM_PICKS }

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
    ) -> Result<ListParWithRandom, SpaceError> {
        match (D::JOIN_WINDOW, level) {
            (JoinWindow::Lanes, Some(level)) => Ok(record::wrap_pq_datum(ord, level.slot(), data)),
            (JoinWindow::Head | JoinWindow::Scan | JoinWindow::Classes, None) => {
                Ok(record::wrap_datum(ord, data))
            }
            // A VectorDB record holds the vector of its send (P6, design v3 4.3), so the engine
            // Changed by decision 128 (kind routing): the discipline gives the record step. The
            // text before:
            // ```
            // // builds it from the params of the send (`vectordb::vdb_record`), never from the data
            // // alone.
            // ```
            // builds it from the params of the send (the record step of the discipline), never from
            // the data alone.
            (JoinWindow::Similarity, _) => Err(SpaceError::Inconsistent(
                // Changed by decision 128 (kind routing): the window owns the text of the error.
                // The text before:
                // "a VectorDB record is built from the params of its send",
                RECORD_FROM_SEND_PARAMS,
            )),
            _ => Err(SpaceError::Inconsistent(
                "a record level does not fit its discipline",
            )),
        }
    }

    fn record_slots(&self, record: &ListParWithRandom) -> Result<(i64, Option<Level>), SpaceError> {
        match D::JOIN_WINDOW {
            JoinWindow::Lanes => {
                let (ord, slot) = record::pq_datum_slots(record)?;
                Ok((ord, Some(Level::of_slot(slot))))
            }
            JoinWindow::Head | JoinWindow::Scan | JoinWindow::Classes => {
                Ok((record::datum_ord(record)?, None))
            }
            JoinWindow::Similarity => Ok((record::vdb_datum_slots(record)?.0, None)),
        }
    }

    fn payload_of(&self, record: &ListParWithRandom) -> Result<ListParWithRandom, SpaceError> {
        match D::JOIN_WINDOW {
            JoinWindow::Lanes => Ok(record::unwrap_pq_datum(record)?.datum),
            JoinWindow::Head | JoinWindow::Scan | JoinWindow::Classes => {
                Ok(record::unwrap_datum(record)?.datum)
            }
            // Changed by decision 128 (kind routing): the discipline of the source gives the step.
            // The text before:
            // JoinWindow::Similarity => vectordb::payload_of(record),
            JoinWindow::Similarity => D::similarity_payload(record),
        }
    }

    fn waiter_slots(&self, waiter: &ParWaiter, width: usize) -> Result<Vec<JoinSlot>, SpaceError> {
        match width {
            1 => {
                let slot = match D::JOIN_WINDOW {
                    JoinWindow::Lanes => {
                        let (ord, priority) = record::pq_continuation_slots(&waiter.continuation)?;
                        JoinSlot {
                            ord,
                            bind: 0,
                            policy: WaiterPolicy::of_level(priority),
                        }
                    }
                    JoinWindow::Head | JoinWindow::Scan | JoinWindow::Classes => JoinSlot {
                        ord: record::continuation_ord(&waiter.continuation)?,
                        bind: 0,
                        policy: WaiterPolicy::None,
                    },
                    JoinWindow::Similarity => {
                        let (ord, policy) =
                            record::continuation_policy_slots(&waiter.continuation)?;
                        JoinSlot {
                            ord,
                            bind: 0,
                            policy,
                        }
                    }
                };
                let mut slots = Vec::<JoinSlot>::with_capacity(1);
                slots.push(slot);
                Ok(slots)
            }
            _ => {
                let slots = record::join_continuation_slots(&waiter.continuation)?;
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

    // Changed by decision 124 (plan v4 C7.9, ruling OP-J): the reads of this fn go through the
    // view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn window(
    fn window<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        scope: &JoinScope,
        coll: usize,
        need: usize,
        incoming: Option<&Incoming>,
        _pending: &[Work],
        // Added by decision 102 (plan v4 C5.3): the order of the window.
        order: WindowOrder<'_>,
    ) -> Result<Vec<Entry>, SpaceError> {
        let channel = &collection(scope, coll)?.data;
        let incoming = incoming.filter(|x| x.coll == coll);
        // Changed by decision 103 (decision 118 E1, rule CR-8): the match gives the window, and
        // E1 orders it after the match. The match ended with `}` before, and now with `};`.
        // match D::JOIN_WINDOW {
        let window = match D::JOIN_WINDOW {
            JoinWindow::Head => {
                let records: Vec<(i64, usize)> = {
                    // let stored = cx.data_on(channel)?;
                    let stored = view.data_on(channel)?;
                    let mut records = Vec::<(i64, usize)>::with_capacity(stored.len());
                    for (index, datum) in stored.iter().enumerate() {
                        records.push((self.record_slots(&datum.a)?.0, index));
                    }
                    records
                };
                // Changed by decision 103 (decision 118 E1, rule CR-8): the arm gives the window.
                // self.head_window(&records, need, incoming)
                self.head_window(&records, need, incoming)?
            }
            JoinWindow::Lanes => {
                let mut levels: BTreeMap<Level, Vec<(i64, usize)>> = BTreeMap::new();
                {
                    // let stored = cx.data_on(channel)?;
                    let stored = view.data_on(channel)?;
                    for (index, datum) in stored.iter().enumerate() {
                        let (ord, level) = self.record_slots(&datum.a)?;
                        let level = level.ok_or(SpaceError::Inconsistent(
                            "a record of a priority queue has no level",
                        ))?;
                        levels.entry(level).or_default().push((ord, index));
                    }
                }
                if let Some(x) = incoming {
                    let level = x.level.ok_or(SpaceError::Inconsistent(
                        "an element of a priority queue has no level",
                    ))?;
                    if !levels.contains_key(&level) {
                        levels.insert(level, Vec::new());
                    }
                }
                let mut window = Vec::new();
                for (level, records) in &levels {
                    let in_level = incoming.filter(|x| x.level == Some(*level));
                    let entries = self.head_window(records, need, in_level)?;
                    for entry in entries {
                        window.push(Entry {
                            at: entry.at,
                            level: Some(*level),
                        });
                    }
                }
                // Changed by decision 103 (decision 118 E1, rule CR-8): the arm gives the window.
                // Ok(window)
                window
            }
            // A similarity bind sees its own eligible records (`Engine::similarity_bind`); this
            // window of every record is the window of a bind without a query.
            JoinWindow::Scan | JoinWindow::Classes | JoinWindow::Similarity => {
                // Every stored record is in the window. An element in flight always lies in the
                // window of a scan, so it joins it at the end: no stored record takes its place.
                // Changed by decision 102 (plan v4 C5.3, rules CR-1 and CR-3): a collection that
                // the group names once picks with the source of the operation in a discipline
                // that picks at random. Every other window keeps DR-75.
                // let order = cx.canonical_data_order(channel)?;
                let order = match (need, order, D::RANDOM_PICKS) {
                    (1, WindowOrder::Pick(source), true) => {
                        view.pick_data_order(channel, source)?
                    }
                    _ => view.canonical_data_order(channel)?,
                };
                let length = order.len() + usize::from(incoming.is_some());
                let mut window = Vec::<Entry>::with_capacity(length);
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
                // Changed by decision 103 (decision 118 E1, rule CR-8): the arm gives the window.
                // Ok(window)
                window
            }
        };
        // Changed by decision 103 (decision 118 E1, rule CR-8): a collection that the group
        // names two or more times puts x first, in every shape.
        match need {
            0 | 1 => Ok(window),
            _ => Ok(x_first(window)),
        }
    }

    fn claim(
        &self,
        coll: usize,
        at: EntryAt,
        payload: &ListParWithRandom,
    ) -> Result<Claim, SpaceError> {
        match D::JOIN_WINDOW {
            JoinWindow::Classes => match self.discipline.join_class_key(&payload.pars)? {
                Some(key) => {
                    let hash = bincode_digest(&key)?;
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
        match D::JOIN_WINDOW {
            JoinWindow::Head => {
                let seats = self.seats(cx, scope, incoming.coll)?;
                match self.head_seat(&seats.seats, None)? {
                    Some(head) => Ok(vec![self.reference(&seats, head)?]),
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
                    if let Some(head) = self.head_seat(&seats.seats, Some(lane))? {
                        if !heads.contains(&head) {
                            heads.push(head);
                        }
                    }
                }
                self.oldest_first(&seats, heads)
            }
            JoinWindow::Scan | JoinWindow::Classes | JoinWindow::Similarity => {
                // Changed by decision 102 (plan v4 C5.3, rules CR-1 and CR-4) and decision 124
                // (LP-1, L5): an offer picks its waiters with the source of its produce in a
                // discipline that picks at random.
                // self.union_order(cx, scope, incoming.coll)
                let order = self.union_order(cx, scope, incoming.coll)?;
                match D::RANDOM_PICKS {
                    true => pick_waiters(
                        cx,
                        &collection(scope, incoming.coll)?.data,
                        order,
                        RSpaceOperationSource::Produce(&incoming.source),
                    ),
                    false => Ok(order),
                }
            }
        }
    }

    // Changed by decision 124 (plan v4 C7.11, ruling RADD 1.2): the reads of this fn go through
    // the view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn cascade_order(
    fn cascade_order<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        scope: &JoinScope,
        coll: usize,
    ) -> Result<Vec<WaiterRef>, SpaceError> {
        match D::JOIN_WINDOW {
            JoinWindow::Head => {
                // let seats = self.seats(cx, scope, coll)?;
                let seats = self.seats(view, scope, coll)?;
                match self.head_seat(&seats.seats, None)? {
                    Some(head) => Ok(vec![self.reference(&seats, head)?]),
                    None => Ok(Vec::new()),
                }
            }
            JoinWindow::Lanes => {
                // let seats = self.seats(cx, scope, coll)?;
                let seats = self.seats(view, scope, coll)?;
                let mut lanes: Vec<Option<Lane>> = Vec::new();
                for seat in &seats.seats {
                    for lane in &seat.lanes {
                        if !lanes.contains(lane) {
                            lanes.push(*lane);
                        }
                    }
                }
                let mut heads: Vec<usize> = Vec::new();
                for lane in lanes {
                    if let Some(head) = self.head_seat(&seats.seats, lane)? {
                        if !heads.contains(&head) {
                            heads.push(head);
                        }
                    }
                }
                self.oldest_first(&seats, heads)
            }
            // Decision 50: a set cascades over every waiter of the collection, in canonical order,
            // because a class can move when its first member leaves (F-JF-1). A bag needs none.
            // A VectorDB cascades over its list waiters (decision 66, LM-b), which the engine
            // picks out of every waiter of the collection.
            // Added by decision 102 (plan v4 C5.7): kept DR-75, because the cascade keeps DR-75
            // in both roles (F3, decision 124 L1).
            // JoinWindow::Classes | JoinWindow::Similarity => self.union_order(cx, scope, coll),
            JoinWindow::Classes | JoinWindow::Similarity => self.union_order(view, scope, coll),
            JoinWindow::Scan => Ok(Vec::new()),
        }
    }

    // Changed by decision 124 (plan v4 C7.9, ruling OP-J): the reads of this fn go through the
    // view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn at_head(
    fn at_head<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        scope: &JoinScope,
        waiter: &WaiterRef,
        slots: &[JoinSlot],
        colls: &[usize],
        coll: usize,
    ) -> Result<bool, SpaceError> {
        match D::JOIN_WINDOW {
            JoinWindow::Scan | JoinWindow::Classes | JoinWindow::Similarity => Ok(true),
            JoinWindow::Head | JoinWindow::Lanes => {
                // let seats = self.seats(cx, scope, coll)?;
                let seats = self.seats(view, scope, coll)?;
                for lane in self.lanes_on(slots, colls, coll) {
                    let filter = match D::JOIN_WINDOW {
                        JoinWindow::Lanes => lane,
                        JoinWindow::Head
                        | JoinWindow::Scan
                        | JoinWindow::Classes
                        | JoinWindow::Similarity => None,
                    };
                    let Some(head) = self.head_seat(&seats.seats, filter)? else {
                        return Ok(false);
                    };
                    let seat = seats
                        .seats
                        .get(head)
                        .ok_or(SpaceError::Inconsistent("a seat index is outside its list"))?;
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
        match D::JOIN_WINDOW {
            JoinWindow::Scan | JoinWindow::Classes | JoinWindow::Similarity => Ok(true),
            JoinWindow::Head | JoinWindow::Lanes => {
                let seats = self.seats(cx, scope, coll)?;
                for lane in self.lanes_on(slots, colls, coll) {
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
                        Some(ord) => match self.head_seat(&seats.seats, filter)? {
                            None => true,
                            Some(head) => {
                                let head_ord = seats
                                    .seats
                                    .get(head)
                                    .ok_or(SpaceError::Inconsistent(
                                        "a seat index is outside its list",
                                    ))?
                                    .ord;
                                match head_ord == ord {
                                    true => return Err(SpaceError::Inconsistent(
                                        "a re-issued element shares its order with a stored entry",
                                    )),
                                    false => {
                                        self.discipline.precedes(Side::Waiters, ord, head_ord)?
                                    }
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
        let channel = &collection(scope, coll)?.data;
        let groups: Vec<Vec<Par>> = {
            let joins = cx.joins_on(channel)?;
            joins.to_vec()
        };
        let mut count = 0usize;
        for group in &groups {
            count = count.checked_add(cx.group_waiters(group)?.len()).ok_or(
                SpaceError::Inconsistent("a collection holds too many waiters"),
            )?;
        }
        Ok(count)
    }

    fn class_key(&self, payload: &[Par]) -> Result<Option<Vec<u8>>, SpaceError> {
        self.discipline.join_class_key(payload)
    }

    // Changed by decision 124 (plan v4 C7.9, ruling OP-J): the reads of this fn go through the
    // view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn stored_entry(
    fn stored_entry<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        scope: &JoinScope,
        coll: usize,
        index: usize,
        missing: &'static str,
    ) -> Result<StoredEntry, SpaceError> {
        let (payload, persist) = {
            // let records = cx.data_on(&collection(scope, coll)?.data)?;
            let records = view.data_on(&collection(scope, coll)?.data)?;
            let datum = records
                .get(index)
                .ok_or(SpaceError::Inconsistent(missing))?;
            (self.payload_of(&datum.a)?, datum.persist)
        };
        let claim = self.claim(coll, EntryAt::Stored(index), &payload)?;
        Ok(StoredEntry {
            payload,
            persist,
            claim,
            // Added by decision 103 (decision 118, rule CR-9).
            record: Claim::Record {
                coll,
                at: EntryAt::Stored(index),
            },
        })
    }

    // Changed by decision 124 (plan v4 C7.9, ruling OP-J): the reads of this fn go through the
    // view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn stored_payload(
    fn stored_payload<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        scope: &JoinScope,
        coll: usize,
        index: usize,
        missing: &'static str,
    ) -> Result<ListParWithRandom, SpaceError> {
        // let records = cx.data_on(&collection(scope, coll)?.data)?;
        let records = view.data_on(&collection(scope, coll)?.data)?;
        let datum = records
            .get(index)
            .ok_or(SpaceError::Inconsistent(missing))?;
        self.payload_of(&datum.a)
    }

    // Changed by decision 124 (plan v4 C7.9, rulings OP-J and RADD 1.5): the reads of this fn go
    // through the view `view` in place of `cx`. The comment above a changed line keeps its text
    // before.
    // fn members(
    fn members<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        scope: &JoinScope,
        coll: usize,
    ) -> Result<Vec<usize>, SpaceError> {
        // Added by decision 102 (plan v4 C5.7): kept DR-75, because G1 and C5.4 bind the
        // DR-75-first member of a class.
        // cx.canonical_data_order(&collection(scope, coll)?.data)
        view.canonical_data_order(&collection(scope, coll)?.data)
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
        let datum = {
            let records = cx.data_on(&collection(scope, coll)?.data)?;
            let datum = records.get(index).ok_or(SpaceError::Inconsistent(
                "a bound index is outside the staged view",
            ))?;
            datum.clone()
        };
        let (ord, level) = self.record_slots(&datum.a)?;
        let payload = self.payload_of(&datum.a)?;
        let introduction = cx.introduction().for_record(&datum.a)?;
        let x = Incoming {
            coll,
            record: datum.a.clone(),
            payload,
            source: datum.source.clone(),
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

    // Changed by decision 124 (plan v4 C7.11, ruling RADD 1.2): the reads of this fn go through
    // the view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn second_send(
    fn second_send<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        scope: &JoinScope,
        coll: usize,
        index: usize,
        entry: &Bound,
    ) -> Result<Option<Work>, SpaceError> {
        if entry.persist {
            return Ok(None);
        }
        let record = {
            // let records = cx.data_on(&collection(scope, coll)?.data)?;
            let records = view.data_on(&collection(scope, coll)?.data)?;
            let datum = records.get(index).ok_or(SpaceError::Inconsistent(
                "a bound index is outside the staged view",
            ))?;
            datum.a.clone()
        };
        Ok(Some(Work {
            coll,
            record,
            persist: false,
            own: false,
            restore: None,
        }))
    }

    // Changed by decision 124 (plan v4 C7.9, ruling OP-J): the reads of this fn go through the
    // view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn incoming_source(
    fn incoming_source<V: StoreView>(
        &self,
        // _cx: &mut Cx<'_, '_>,
        _view: &mut V,
        _scope: &JoinScope,
        x: &Incoming,
        _coll: usize,
    ) -> Result<Produce, SpaceError> {
        Ok(x.source.clone())
    }

    // Changed by decision 124 (plan v4 C7.9, ruling OP-J): the reads of this fn go through the
    // view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn incoming_record(
    fn incoming_record<V: StoreView>(
        &self,
        // _cx: &mut Cx<'_, '_>,
        _view: &mut V,
        _scope: &JoinScope,
        x: &Incoming,
        _coll: usize,
    ) -> Result<ListParWithRandom, SpaceError> {
        Ok(x.record.clone())
    }

    fn enters(&self, _scope: &JoinScope, x: &Incoming, coll: usize) -> Result<bool, SpaceError> {
        Ok(coll == x.coll)
    }

    fn incoming_payload<'x>(
        &self,
        _scope: &JoinScope,
        x: &'x Incoming,
        _coll: usize,
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
        // Added by decision 124 (errata V4-17): the search of an E3-a store.
        search: Option<ParRowSearch>,
    ) -> Result<(), SpaceError> {
        let channel = &collection(scope, x.coll)?.data;
        // Changed by decision 124 (errata V4-17): an E3-a store carries its row search. The text
        // before:
        // cx.store_on(channel, x.record, x.persist, &x.introduction)?;
        cx.store_searched(channel, x.record, x.persist, &x.introduction, search)?;
        mark(dirty, &[x.coll]);
        Ok(())
    }

    // Changed by decision 124 (plan v4 C7.9, ruling OP-J): the reads of this fn go through the
    // view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn before_fire(
    fn before_fire<V: StoreView>(
        &self,
        // _cx: &mut Cx<'_, '_>,
        _view: &mut V,
        _scope: &JoinScope,
        site: FireSite,
        _colls: &[usize],
        bound: &[Bound],
        // Retired by decision 101 (rule CR-13): a single-bind fire binds one entry.
        // _members: &[ParCandidate],
        _x: Option<&Incoming>,
    ) -> Result<Vec<Claim>, SpaceError> {
        // Changed by decision 101 (rule CR-13): a single-bind fire binds the first member of its
        // class over `[D]`, so it retires the other members as the other sites do. The text
        // before: "A single-bind class fire removes the whole class over [D; n], so it retires
        // nothing afterwards (F3, F20)."
        // match site {
        //     FireSite::Single => Ok(Vec::new()),
        //     FireSite::Offer | FireSite::Join | FireSite::Cascade => Ok(self.class_claims(bound)),
        // }
        match site {
            FireSite::Single | FireSite::Offer | FireSite::Join | FireSite::Cascade => {
                Ok(self.class_claims(bound))
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

    /// Added by decision 124 (plan v4 C7.9, ruling RADD 1.5): the keyed row search of a fire. The
    /// count of its staged retires is the count of the class members that `retire_classes`
    /// stages after the fire, read before it (`class_retires`).
    fn row_search(
        &self,
        cx: &mut Cx<'_, '_>,
        row: JoinRow,
        spent: Option<&Self::Spent>,
    ) -> Result<ParRowSearch, SpaceError> {
        let staged = spent.cloned();
        let staged_retires = match &staged {
            None => 0,
            Some(claims) => self
                .class_retires(cx, &row.inputs.scope, &row.colls, claims, &row.tuple)?
                .len(),
        };
        Ok(Box::new(KeyedRowSearch::<S, D> {
            row,
            discipline: D::default(),
            outer: std::marker::PhantomData,
            staged,
            staged_retires,
            retires: Vec::new(),
        }))
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD2v2 6.4 and 6.6): the
    /// machinery search of the lift with the keyed end (`KeyedLiftEnd`).
    fn lift_search(&self, lift: LiftRow<D>) -> ParRowSearch {
        let (machinery, cut) = lift.into_parts();
        Box::new(machinery.with_end(KeyedLiftEnd::<S, D> {
            cut,
            discipline: D::default(),
            outer: std::marker::PhantomData,
            end: None,
        }))
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
        let retire_random = RandomPurpose::Retire.derive(random);
        let mut retired: Vec<Claim> = Vec::new();
        for (claim, &coll) in claims.iter().zip(colls) {
            let Claim::Class { key, hash, .. } = claim else {
                continue;
            };
            if is_claimed(&retired, claim)? {
                continue;
            }
            retired.push(claim.clone());
            // Changed by decision 124 (plan v4 C7.11, ruling RADD2 1.6): the retire names the
            // discipline `D` of the source, which its machinery search keeps. The text
            // before:
            // retire_class_on(
            retire_class_on::<D>(
                cx,
                &collection(scope, coll)?.data,
                key,
                hash,
                &retire_random,
                authority,
                |payload| self.class_key(payload),
            )?;
        }
        Ok(())
    }

    /// The claims of the bound entries `bound`, for RETIRE-SET-BINDS after the fire moved the
    /// entries into its candidates. Only a discipline with classes retires, so the other
    /// disciplines copy nothing.
    fn class_claims(&self, bound: &[Bound]) -> Vec<Claim> {
        if D::JOIN_WINDOW != JoinWindow::Classes {
            return Vec::new();
        }
        let mut claims = Vec::<Claim>::with_capacity(bound.len());
        for entry in bound {
            claims.push(entry.claim.clone());
        }
        claims
    }

    /// Added by decision 124 (plan v4 C7.9, ruling RADD 1.5, ADD A1.10 (c) 7): RETIRE-SET-BINDS
    /// over the store `view` before the fire of `tuple`. For each bound class of `claims` on
    /// the positions `colls`, once: its stored members that are not persistent, in DR-75 order,
    /// without the stored records that the fire moves. These are the records that
    /// `retire_classes` retires after the fire, in its order. A discipline without classes
    /// retires nothing.
    fn class_retires<V: StoreView>(
        &self,
        view: &mut V,
        scope: &JoinScope,
        colls: &[usize],
        claims: &[Claim],
        tuple: &[ParCandidate],
    ) -> Result<Vec<RetiredMember>, SpaceError> {
        match D::JOIN_WINDOW == JoinWindow::Classes {
            true => {}
            false => return Ok(Vec::new()),
        }
        let mut retired = Vec::<RetiredMember>::new();
        let mut classes = Vec::<Claim>::with_capacity(claims.len());
        for (claim, &coll) in claims.iter().zip(colls) {
            let Claim::Class { key, hash, .. } = claim else {
                continue;
            };
            match is_claimed(&classes, claim)? {
                false => {}
                true => continue,
            }
            classes.push(claim.clone());
            let channel = &collection(scope, coll)?.data;
            let mut moved = Vec::<&Blake2b256Hash>::with_capacity(tuple.len());
            for candidate in tuple {
                let stored = candidate.datum_index >= 0;
                match (
                    stored,
                    candidate.datum.persist,
                    candidate.channel == *channel,
                ) {
                    (true, false, true) => moved.push(&candidate.datum.source.hash),
                    (false, _, _) | (true, true, _) | (true, false, false) => {}
                }
            }
            let records = view.data_on(channel)?;
            let class_key: ClassKey = |payload: &[Par]| D::default().join_class_key(payload);
            for (index, payload) in class_members(records, key, hash, class_key)? {
                let stored = records.get(index).ok_or(SpaceError::Inconsistent(
                    "a class member is outside its channel",
                ))?;
                match stored.persist {
                    false => {}
                    true => continue,
                }
                match moved
                    .iter()
                    .position(|source| **source == stored.source.hash)
                {
                    None => {}
                    Some(at) => {
                        moved.swap_remove(at);
                        continue;
                    }
                }
                retired.push(RetiredMember {
                    channel: channel.clone(),
                    stored: stored.clone(),
                    payload,
                    index,
                });
            }
        }
        Ok(retired)
    }

    /// Every waiter of the collection `coll`, in the canonical order of DR-75 of the union of its
    /// join groups: a merge of the canonical orders of the groups by the source hash of each
    /// consume (decision 41). Two waiters of distinct groups have distinct consumes, so a tie
    /// across groups is `Inconsistent`.
    // Changed by decision 124 (plan v4 C7.11, ruling RADD 1.2): the reads of this fn go through
    // the view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn union_order(
    fn union_order<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        scope: &JoinScope,
        coll: usize,
    ) -> Result<Vec<WaiterRef>, SpaceError> {
        let channel = &collection(scope, coll)?.data;
        let groups: Vec<Vec<Par>> = {
            // let joins = cx.joins_on(channel)?;
            let joins = view.joins_on(channel)?;
            joins.to_vec()
        };
        let mut sequences: Vec<Vec<(Blake2b256Hash, usize)>> = Vec::with_capacity(groups.len());
        for group in &groups {
            // Added by decision 102 (plan v4 C5.7): kept DR-75, because it is the base of the
            // pick in `offer_order`, and `cascade_order` keeps it (F3).
            // let order = cx.canonical_group_order(group)?;
            let order = canonical_continuation_order(view.group_waiters(group)?);
            // let waiters = cx.group_waiters(group)?;
            let waiters = view.group_waiters(group)?;
            let mut sequence = Vec::<(Blake2b256Hash, usize)>::with_capacity(order.len());
            for index in order {
                let waiter = waiters.get(index).ok_or(SpaceError::Inconsistent(
                    "a canonical index is outside the staged view",
                ))?;
                // Added by decision 124 (plan v2 C7.10, decision 100): an inert machinery waiter
                // is no candidate.
                match machinery_shape(waiter) {
                    false => {}
                    true => continue,
                }
                sequence.push((waiter.source.hash.clone(), index));
            }
            sequences.push(sequence);
        }
        // The union of the canonical orders of the groups: a merge by the source hash of
        // each consume. Two waiters of distinct groups have distinct consumes, so a tie
        // across groups is `Inconsistent`.
        let total: usize = sequences.iter().map(Vec::len).sum();
        let mut merged = Vec::<WaiterRef>::with_capacity(total);
        let mut cursors = vec![0usize; sequences.len()];
        for _ in 0..total {
            let mut least: Option<(usize, &Blake2b256Hash)> = None;
            for (group, sequence) in sequences.iter().enumerate() {
                let cursor = cursors.get(group).copied().unwrap_or(usize::MAX);
                let Some((hash, _)) = sequence.get(cursor) else {
                    continue;
                };
                least = match least {
                    None => Some((group, hash)),
                    Some((best, best_hash)) => match hash.cmp(best_hash) {
                        std::cmp::Ordering::Less => Some((group, hash)),
                        std::cmp::Ordering::Greater => Some((best, best_hash)),
                        std::cmp::Ordering::Equal => {
                            return Err(SpaceError::Inconsistent(
                                "two waiters of distinct groups share a consume",
                            ))
                        }
                    },
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
                group: channels.clone(),
                index: *index,
                source: hash.clone(),
            });
        }
        Ok(merged)
    }

    /// The references of the seats `heads`, in ascending order of the seats (the older first).
    fn oldest_first(
        &self,
        seats: &Seats,
        mut heads: Vec<usize>,
    ) -> Result<Vec<WaiterRef>, SpaceError> {
        let mut keyed = Vec::<(i64, usize)>::with_capacity(heads.len());
        for head in heads.drain(..) {
            let seat = seats
                .seats
                .get(head)
                .ok_or(SpaceError::Inconsistent("a seat index is outside its list"))?;
            keyed.push((seat.ord, head));
        }
        keyed.sort();
        let mut references = Vec::<WaiterRef>::with_capacity(keyed.len());
        for (_, head) in keyed {
            references.push(self.reference(seats, head)?);
        }
        Ok(references)
    }
}

/// Added by decision 124 (plan v4 C7.9, ruling RADD 1.5): the row search of a keyed join fire
/// (a source of keyed collections). It holds the parts that the kinds share, a copy of the
/// discipline, the class claims that c0's fire spends, and the count of the class retires that
/// the staging stages after c0's fire (RETIRE-SET-BINDS, CR-33).
struct KeyedRowSearch<S, D> {
    row: JoinRow,
    discipline: D,
    outer: std::marker::PhantomData<fn() -> S>,
    /// What c0's fire spends: the claim of each position. `None` for a fire that peeks.
    staged: Option<Vec<Claim>>,
    staged_retires: usize,
    /// The class retires of the last tuple, each on its channel.
    retires: Vec<(Par, Member)>,
}

impl<S: OuterStore, D: Discipline>
    RowSearch<Par, BindPattern, ListParWithRandom, TaggedContinuation> for KeyedRowSearch<S, D>
{
    fn runs(&self) -> Vec<RowRun<Par>> { self.row.runs() }

    fn continuations(
        &mut self,
        run: usize,
        lists: &ParRowLists<'_>,
    ) -> Result<Vec<(Arc<ParWaiter>, i32)>, SpaceTxnError> {
        self.row.continuations(run, lists)
    }

    fn tuple(
        &mut self,
        _run: usize,
        lists: &ParRowLists<'_>,
        request: ParRowRequest<'_>,
    ) -> Result<Option<Vec<ParCandidate>>, SpaceTxnError> {
        self.retires.clear();
        let source = KeyedSource::<'_, S, D> {
            outer: std::marker::PhantomData,
            discipline: &self.discipline,
        };
        let inputs = Arc::clone(self.row.inputs());
        let theory = self.row.theory();
        let width = inputs.scope.collections.len();
        let mut view = CommitView::new(
            lists,
            request.matcher,
            theory.as_deref(),
            request.item,
            width,
        );
        let found = self
            .row
            .tuple(&source, &mut view, &request)
            .map_err(join_commit_error)?;
        let Some(found) = found else {
            return Ok(None);
        };
        let spent = match (found.staged, &self.staged, &found.spent) {
            (true, Some(claims), _) => Some((self.row.colls.as_slice(), claims.as_slice())),
            (false, _, Some((colls, claims))) => Some((colls.as_slice(), claims.as_slice())),
            (true, None, _) | (false, _, None) => None,
        };
        let retired = match spent {
            None => Vec::new(),
            Some((colls, claims)) => source
                .class_retires(&mut view, &inputs.scope, colls, claims, &found.tuple)
                .map_err(join_commit_error)?,
        };
        self.retires = commit_members(request.matcher, retired)?;
        Ok(Some(found.tuple))
    }

    fn restore(&mut self) {
        self.row.restore();
        self.retires.clear();
    }

    fn commit_check(
        &mut self,
        _request: &ParRowRequest<'_>,
        _tuple: &[ParCandidate],
    ) -> Result<bool, SpaceTxnError> {
        self.row.commit_check()
    }

    fn may_veto(
        &self,
        matcher: &dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
        continuation: &TaggedContinuation,
    ) -> bool {
        self.row.may_veto(matcher, continuation)
    }

    fn staged_retires(&self) -> usize { self.staged_retires + self.row.staged_list_retires() }

    fn actual_fire(
        &mut self,
        request: &ParRowRequest<'_>,
        tuple: &[ParCandidate],
    ) -> ActualFire<Par, BindPattern, ListParWithRandom, TaggedContinuation> {
        let retires = std::mem::take(&mut self.retires);
        self.row.fire::<D>(request, tuple, retires)
    }

    fn actual_claims(&self) -> Option<RowClaims<Par>> { Some(self.row.actual_claims()) }

    fn item_failures(
        &self,
        lists: &ParRowLists<'_>,
        matcher: &dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
        rule: &dyn RowItemRule<TaggedContinuation, ListParWithRandom>,
    ) -> Result<Vec<(Blake2b256Hash, Blake2b256Hash)>, SpaceTxnError> {
        let source = KeyedSource::<'_, S, D> {
            outer: std::marker::PhantomData,
            discipline: &self.discipline,
        };
        self.row
            .item_failures(&source, lists, matcher, rule)
            .map_err(join_commit_error)
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD 1.2 and RADD2 1.1, ADD
    /// A2.8): the tail read of a keyed join row over the store of the commit, with the end
    /// of Tail(actual) that the engine gives. The store of the element marks its collection
    /// dirty (`land`), and the finish of a keyed source stages nothing.
    fn tail_read(
        &mut self,
        lists: &ParRowLists<'_>,
        matcher: &dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
    ) -> Result<TailRead, SpaceTxnError> {
        let source = KeyedSource::<'_, S, D> {
            outer: std::marker::PhantomData,
            discipline: &self.discipline,
        };
        let mut marks = Vec::<usize>::with_capacity(1);
        match self.row.unit() {
            RowUnit::Stored(x) => marks.push(x.coll),
            RowUnit::Fired | RowUnit::Parked => {}
        }
        self.row
            .tail_read(&source, lists, matcher, &marks)
            .map_err(join_commit_error)
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.1): the end of
    /// Tail(actual) that the last tail read built.
    fn tail_reissue(&mut self) -> Option<RowReissue> { self.row.take_reissue() }
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD2v2 6.4 and 6.6): the end of a
/// cascade lift of a keyed source: the stop of the engine at the lift for an owner refusal,
/// built at the commit over a keyed source of the discipline `D`. A keyed source stages no
/// finish, so the end has no finish rows.
pub(crate) struct KeyedLiftEnd<S, D> {
    cut: LiftCut,
    discipline: D,
    outer: std::marker::PhantomData<fn() -> S>,
    end: Option<RowReissue>,
}

impl<S: OuterStore, D: Discipline> RowEnd for KeyedLiftEnd<S, D> {
    fn tail_read(
        &mut self,
        _lists: &ParRowLists<'_>,
        _matcher: &dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
    ) -> Result<TailRead, SpaceTxnError> {
        let source: KeyedSource<'_, S, D> = KeyedSource {
            outer: std::marker::PhantomData,
            discipline: &self.discipline,
        };
        let end = refused_lift_end(&source, self.cut.inputs(), self.cut.before())
            .map_err(join_commit_error)?;
        self.end = Some(RowReissue::new(end));
        Ok(TailRead::Stages)
    }

    fn tail_reissue(&mut self) -> Option<RowReissue> { self.end.take() }

    fn finish_rows(&mut self) -> Vec<ParFinishRow> { Vec::new() }
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
// Added by decision 124 (plan v4 C7.9, ruling OP-J): a row search keeps a copy of the second
// sends that the windows of its commit leave out (`JoinSource::window`).
#[derive(Clone)]
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
    // Changed by decision 128 (kind routing): the window owns the type, and the field takes its
    // name. The text before:
    // vdb: Option<VdbConfig>,
    similarity: Option<SimilarityConfig>,
    /// Added by decision 124 (plan v4 C7.9, ruling OP-J): the inputs that the row searches of an
    /// owner-funded run share, or `None` without owner funding (RC2 D-10).
    commit_inputs: Option<Arc<EngineInputs>>,
}

/// Added by decision 124 (plan v4 C7.9, rulings OP-J and RADD 1.3): the inputs of `Engine::new`
/// that the row searches of one owner-funded run share (RC2 D-10). A search builds the engine of
/// its commit from them, as `run_join_with` builds the engine of the staging. The operation is the
/// copy that the plan keeps for its re-issue, so a run copies it once.
pub(crate) struct EngineInputs {
    /// The operation of the run.
    pub(crate) op: Arc<SpaceOp>,
    /// The closure of the run.
    pub(crate) scope: JoinScope,
    /// The config of the space.
    pub(crate) config: SpaceConfig,
    /// Added by decision 124 (plan v4 C7.11, rule CR-33, ruling RADD2v2 6.6): the random state of
    /// the run, for the finish rows that a search builds at the commit (P1).
    pub(crate) random: Blake2b512Random,
    /// The cost authority of the run, for the finish rows (P1).
    pub(crate) authority: Option<CostAuthority>,
    /// The introduction of the operation, for the finish rows (P1).
    pub(crate) introduction: Arc<Introduction>,
    /// The job of the operation, for the finish rows (P1).
    pub(crate) job: SpaceJob,
}

/// Added by decision 124 (plan v4 C7.9, ruling OP-J): the error of a join row search at the
/// commit. A read of the lists keeps its own error, so a read outside the keys still fails with
/// `UnlockedChannel` (CR-36). Every other error fails closed, as in the discipline searches.
pub(crate) fn join_commit_error(error: SpaceError) -> SpaceTxnError {
    match error {
        SpaceError::Substrate(error) => error,
        SpaceError::Host(error) => SpaceTxnError::Host(error),
        SpaceError::ReplayDivergence(comm) => SpaceTxnError::ReplayDivergence(comm),
        other => commit_error(other),
    }
}

/// Added by decision 124 (plan v4 C7.9, ruling RADD 1.5, ADD A1.10 (b)): the side of a join row.
enum JoinSide {
    /// An offer of the element `x` with its vector `vector` to the waiters of the pick order
    /// `order` of the staging (CR-1), c0 among them.
    Offer {
        x: Incoming,
        vector: Option<ExactVector>,
        order: Vec<WaiterRef>,
    },
    /// A receive or its E3-a park: the group of the waiter of the operation, the collection and
    /// the slot of each position, and its copy with its consume.
    Receive {
        group: Vec<Par>,
        colls: Vec<usize>,
        slots: Vec<JoinSlot>,
        waiting: ParWaiter,
    },
}

/// Added by decision 124 (plan v4 C7.9, rulings RADD 1.5 and 1.7): the list of a staged list fire
/// (decision 66): the values that the pattern binds, the members in list order, each with its
/// index in the staged view or `None` for the element in flight, and the stored members that the
/// fire retires after it (CR-13), empty for a fire that peeks.
struct StagedList {
    matched: ListParWithRandom,
    members: Vec<Option<(ParDatum, usize)>>,
    others: Vec<RetiredMember>,
}

/// Added by decision 124 (plan v4 C7.9 and C7.11, rulings RADD 1.5, 1.2 and RADD2 1.1, N3): the
/// staged fire of a join row (c0): its consume, the channel of the row, the collection of each
/// position, its tuple, the position and the class of each stored entry that binds one (C4v2
/// B.4), its list, whether it peeks, its site, the engine state before it, what it decides for
/// the steps after it, and the second sends that it pushes on the work stack.
struct StagedFire<'f> {
    consume: &'f Consume,
    channel: &'f Par,
    colls: &'f [usize],
    tuple: &'f [ParCandidate],
    classes: Vec<(usize, Claim)>,
    list: Option<StagedList>,
    peeks: bool,
    site: FireSite,
    before: Option<Before>,
    facts: Option<FireFacts>,
    sends: &'f [Work],
}

/// Added by decision 124 (plan v4 C7.9, ruling RADD 1.5, ADD A1.10 (b)): the parts of a join row
/// search that both kinds share. The search owns them, because `RowSearch` has no lifetime. The
/// engine copies them at the fire (`Engine::row_search`), and the search of the kind runs the
/// engine search over the store of the commit with them.
pub(crate) struct JoinRow {
    /// The inputs of `Engine::new`, shared by the searches of the run.
    inputs: Arc<EngineInputs>,
    /// The second sends that the staging owes at the fire. The windows leave them out.
    work: Vec<Work>,
    side: JoinSide,
    /// The consume of c0.
    staged: Consume,
    /// The channel of the row: the data channel of the collection of c0's fire.
    channel: Par,
    /// The collection of each position of c0.
    colls: Vec<usize>,
    /// c0's staged tuple (N3). Empty for an E3-a park, which lists no tuple (V4-17).
    tuple: Vec<ParCandidate>,
    /// The position and the class of each stored entry of c0 that binds one (C4v2 B.4).
    classes: Vec<(usize, Claim)>,
    /// c0's list (decision 66).
    list: Option<StagedList>,
    /// True when c0's fire peeks, so it retires nothing.
    peeks: bool,
    theory: Option<Arc<dyn SpaceTheory>>,
    /// False under a leasing theory: the row lists c0 only (C4v2 G8).
    alternatives: bool,
    /// The retire rows of the fire (CR-33).
    rows: RetireRows,
    /// The answer of the last tuple.
    found: Option<Found>,
    /// The list retires of the last tuple, each on its channel (CR-13).
    retires: Vec<(Par, Member)>,
    /// The claims of the actual fire (RADD 1.8).
    fired: Option<RowClaims<Par>>,
    /// The site of c0's fire, for the rule of its step (`site_step`).
    site: FireSite,
    /// The engine state before c0's fire (`Engine::before`).
    before: Before,
    /// What c0's fire decides for the steps after it. `None` for an E3-a park.
    facts: Option<FireFacts>,
    /// The second sends that c0's fire pushes on the work stack, for its re-issue.
    sends: Vec<Work>,
    /// The fire of the last tuple, for the tail read of the unit.
    last: Option<RowFire>,
    /// The snapshot of the book that the staging read (errata V4-13), for the cascade search of
    /// the tail read (C2v3 4.7, ADD A1.13 (a)).
    book: Arc<BookPairs>,
    /// The end of Tail(actual) that the last tail read built, which `RowSearch::tail_reissue`
    /// takes out (RADD2 1.1).
    reissue: Option<RowReissue>,
}

/// Added by decision 124 (plan v4 C7.9, ruling RADD 1.5, ADD A1.10 (c) 4): the answer of the
/// last tuple of a join row: c0's staged tuple, or a searched tuple and the answer of its guard.
#[derive(Clone, Copy)]
struct Found {
    staged: bool,
    admitted: bool,
}

/// Added by decision 124 (plan v4 C7.9, ruling RADD 1.5): the tuple that a join row found at the
/// commit, and what its fire spends for the retires of the kind (`JoinSource::before_fire` over
/// the view): `None` for c0, whose kind keeps the spend of its staging, and for a fire that
/// peeks or that its guard refused.
pub(crate) struct CommitTuple<T> {
    pub(crate) tuple: Vec<ParCandidate>,
    pub(crate) staged: bool,
    pub(crate) spent: Option<(Vec<usize>, T)>,
}

/// Added by decision 124 (plan v4 C7.9, ruling RADD 1.5): a stored record that a retire of a fire
/// removes: its channel, its copy, the payload that the retire pattern binds, and its index in
/// the view. The commit locates it again before it stages the row (C2 E2.8).
pub(crate) struct RetiredMember {
    pub(crate) channel: Par,
    pub(crate) stored: ParDatum,
    pub(crate) payload: ListParWithRandom,
    pub(crate) index: usize,
}

/// Added by decision 124 (plan v4 C7.9 and C7.11, rulings RADD 1.5 and 1.2): what the engine
/// search of a join row found at the commit: the tuple, the answer of its guard, what its fire
/// spends, the list members that its fire retires, and its fire for the tail read.
struct Searched<T> {
    tuple: Vec<ParCandidate>,
    admitted: bool,
    spent: Option<(Vec<usize>, T)>,
    others: Vec<RetiredMember>,
    fire: RowFire,
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD 1.2 and RADD2 1.1): the engine
/// state that the tail read of a join row starts from, read at the staged fire before the fire
/// changes it: the fire count, whether the run changed the state, the dirty collections, the
/// state of the element of the operation and its cause (`Own::NotStarted` is the settle before
/// the plan), and the order of the operation on each own collection. The re-issue of the row
/// reads the last three (`Engine::reissue_of`). The lift end of a kind names it through
/// `LiftCut::before` (ruling RADD2v2 6.6), so it is `pub(crate)`, and its fields stay private.
#[derive(Clone)]
pub(crate) struct Before {
    fired: usize,
    changed: bool,
    dirty: Vec<bool>,
    own: Own,
    own_cause: Option<ReissueCause>,
    ords: Vec<Option<i64>>,
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD2v2 6.4 and 6.6): the parts of
/// a cascade lift that the engine copies for the row search of the lift
/// (`JoinSource::lift_search`): the machinery search of the lift over the discipline `D` of the
/// source, and the cut of the lift.
pub(crate) struct LiftRow<D: Discipline> {
    machinery: StagedRowSearch<D>,
    cut: LiftCut,
}

impl<D: Discipline> LiftRow<D> {
    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2v2 6.6): the machinery
    /// search and the cut, for the end of a kind (`StagedRowSearch::with_end`).
    pub(crate) fn into_parts(self) -> (StagedRowSearch<D>, LiftCut) { (self.machinery, self.cut) }
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2v2 6.6): the state of a run at a
/// cascade lift that the end of a kind reads at the commit: the inputs of the engine of the run
/// and the engine state before the lift. A refused lift parks inert (CR-16), so the rest of the
/// operation starts from this state.
pub(crate) struct LiftCut {
    inputs: Arc<EngineInputs>,
    before: Before,
}

impl LiftCut {
    /// The inputs of the engine of the run.
    pub(crate) fn inputs(&self) -> &EngineInputs { &self.inputs }

    /// The engine state before the lift, for `refused_lift_end`.
    pub(crate) fn before(&self) -> &Before { &self.before }

    /// True when the engine staged a change of state before the lift (`Engine::changed`).
    pub(crate) fn changed(&self) -> bool { self.before.changed }
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.2): what a fire of a join row
/// decides for the steps after it: the two inputs of `FireRounds::after_fire`, whether its waiter
/// peeks, and whether the state is changed after it.
#[derive(Clone, Copy)]
struct FireFacts {
    spent: bool,
    stays: bool,
    peeks: bool,
    changed: bool,
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD 1.2 and RADD2 1.1): a fire of a
/// join row: the collections that it marks dirty, its facts, and the second sends that it pushes
/// on the work stack, in the order in which `push_work` pushes them.
#[derive(Clone)]
struct RowFire {
    colls: Vec<usize>,
    facts: FireFacts,
    sends: Vec<Work>,
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.2): the engine part of the tail
/// read of a join row. `Stages`: one more step of the engine follows. `Capped`: the round cap
/// stops the engine before a step that it owes. `Commits`: the engine reaches its commit with
/// nothing more to stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TailStep {
    Stages,
    Capped,
    Commits,
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.2): the actual unit of a join
/// row at the commit: its fire, the store of the element of an offer, or the park of a receive.
#[derive(Clone, Copy)]
pub(crate) enum RowUnit<'r> {
    Fired,
    Stored(&'r Incoming),
    Parked,
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD 1.2 and RADD2v2 6.6): the tail
/// read of a join row from the engine part `step`. The finish rows of the source join the unit of
/// the row (P1), so the finish of the source adds no step to the read.
pub(crate) fn finish_read(step: TailStep) -> TailRead {
    match step {
        TailStep::Stages => TailRead::Stages,
        TailStep::Capped => TailRead::Reissues,
        TailStep::Commits => TailRead::Ends,
    }
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.2): the engine part of a step
/// that the engine owes after `fired` fires. The drain and the cascade check the round cap
/// before the step, as the tail of a discipline plan does.
pub(crate) fn round_cap(fired: usize) -> TailStep {
    match fired >= FIRE_ROUND_CAP {
        false => TailStep::Stages,
        true => TailStep::Capped,
    }
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD 1.2 and RADD2 1.1): part 3 of
/// the tail read of a join row, when the drain and the cascade of `engine` owe no step
/// after `fired` fires. After the settle before the plan, the run clears the dirty marks, and
/// the plan follows below the cap. After the plan, or in a settle job, the run
/// commits.
fn after_drain<Src: JoinSource>(engine: &mut Engine<'_, Src>, fired: usize) -> TailStep {
    match engine.own {
        Own::InFlight | Own::Settled => TailStep::Commits,
        Own::NotStarted => {
            engine.dirty.iter_mut().for_each(|mark| *mark = false);
            round_cap(fired)
        }
    }
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2v2 6.4, C2 E4.5): the end of
/// Tail(actual) of a cascade lift that the commit refuses (CR-16), over the source `source` of
/// the kind. The engine of the run starts from the state `before` at the lift with no owed work,
/// because the drain empties the work stack before every cascade fire (`drain_and_cascade`), and
/// it stops there for the owner refusal (`Engine::refusal_stop`).
pub(crate) fn refused_lift_end<Src: JoinSource>(
    source: &Src,
    inputs: &EngineInputs,
    before: &Before,
) -> Result<TailEnd, SpaceError> {
    let mut engine = Engine::new(source, &inputs.scope, &inputs.op, &inputs.config)?;
    engine.dirty = before.dirty.clone();
    engine.own = before.own;
    engine.own_cause = before.own_cause;
    engine.ords = before.ords.clone();
    engine.refusal_stop(SpaceOp::clone(&inputs.op))
}

impl JoinRow {
    /// The inputs of the engine of the commit.
    pub(crate) fn inputs(&self) -> &Arc<EngineInputs> { &self.inputs }

    /// The checker of the theory of the space, for the view of the commit.
    pub(crate) fn theory(&self) -> Option<Arc<dyn SpaceTheory>> { self.theory.clone() }

    /// The runs of the row order (N5): each stretch of waiters of one group in the pick order of
    /// an offer, or the one run of the group of a receive, with no candidate for a park.
    pub(crate) fn runs(&self) -> Vec<RowRun<Par>> {
        match &self.side {
            JoinSide::Offer { order, .. } => {
                let mut runs = Vec::<RowRun<Par>>::with_capacity(order.len());
                let mut start = 0;
                for stretch in order.chunk_by(|first, next| first.group == next.group) {
                    let Some(first) = stretch.first() else {
                        continue;
                    };
                    runs.push(RowRun {
                        channels: first.group.clone(),
                        candidates: start..start + stretch.len(),
                    });
                    start += stretch.len();
                }
                runs
            }
            JoinSide::Receive { group, .. } => {
                let mut runs = Vec::<RowRun<Par>>::with_capacity(1);
                runs.push(RowRun {
                    channels: group.clone(),
                    candidates: 0..usize::from(!self.tuple.is_empty()),
                });
                runs
            }
        }
    }

    /// The waiters of the run `run` of an offer from the lists of the commit, matched by the
    /// source hash of their consume, in the row order (ADD A1.10 (c) 2). Without alternatives
    /// the list holds c0 only (C4v2 G8). A receive lists none.
    pub(crate) fn continuations(
        &self,
        run: usize,
        lists: &ParRowLists<'_>,
    ) -> Result<Vec<(Arc<ParWaiter>, i32)>, SpaceTxnError> {
        let JoinSide::Offer { order, .. } = &self.side else {
            return Ok(Vec::new());
        };
        let runs = self.runs();
        let Some(run) = runs.get(run) else {
            return Err(SpaceTxnError::Inconsistent(
                "a join row has no run at the index",
            ));
        };
        let Some(stretch) = order.get(run.candidates.clone()) else {
            return Err(SpaceTxnError::Inconsistent(
                "a run is outside its row order",
            ));
        };
        let waiters = lists.continuations(&run.channels)?;
        let mut positions = HashMap::<&Blake2b256Hash, usize>::with_capacity(waiters.len());
        for (index, waiter) in waiters.iter().enumerate() {
            positions.entry(&waiter.source.hash).or_insert(index);
        }
        let mut listed = Vec::<(Arc<ParWaiter>, i32)>::with_capacity(stretch.len());
        for at in stretch {
            match (self.alternatives, at.source == self.staged.hash) {
                (true, _) | (false, true) => {}
                (false, false) => continue,
            }
            let Some(&index) = positions.get(&at.source) else {
                continue;
            };
            let Some(waiter) = waiters.get(index) else {
                continue;
            };
            let position = i32::try_from(index)
                .map_err(|_| SpaceTxnError::Inconsistent("a waiter index overflows i32"))?;
            listed.push((Arc::clone(waiter), position));
        }
        Ok(listed)
    }

    /// The tuple of `request` at the commit over `view`, with the source `source` of the kind
    /// (ADD A1.10 (c) 3 and 4). c0 gives its staged tuple when the recorded row names that tuple
    /// (CR-4, R13) and it passes the item test (N3, C4v2 B.4). Another waiter of an offer, and
    /// with alternatives a receive whose staged tuple fails, runs the engine search over `view`.
    /// Its guard runs once, and `commit_check` reports the answer. An E3-a park holds no staged
    /// tuple, so its search is the only way that the row finds the tuple of a recorded refusal
    /// (V4-17). Replay can also stage another tuple than the pick of play that a refusal names,
    /// because a Stored row stages in DR-75 order. That staged tuple is not named, so the row
    /// runs the search, whose picks read the named records first.
    pub(crate) fn tuple<Src: JoinSource>(
        &mut self,
        source: &Src,
        view: &mut CommitView<'_, '_>,
        request: &ParRowRequest<'_>,
    ) -> Result<Option<CommitTuple<Src::Spent>>, SpaceError> {
        self.restore();
        // Disabled by decision 124 (plan v4 C7.9, errata V4-17): step 1 of the commit runs on
        // every row that has recorded refusals, an E3-a park included, so a row without a staged
        // tuple runs its search too.
        let inputs = Arc::clone(&self.inputs);
        let mut engine = Engine::new(source, &inputs.scope, &inputs.op, &inputs.config)?;
        engine.work = self.work.clone();
        let staged = *request.source == self.staged;
        let named = view.names_tuple(&self.tuple)?;
        let passes = match (staged, named, self.tuple.is_empty()) {
            (true, true, false) => self.staged_admits(&engine, view, request)?,
            (true, true, true) | (true, false, _) | (false, _, _) => false,
        };
        match (passes, self.alternatives) {
            (true, _) => return self.staged_tuple(request).map(Some),
            (false, false) => return Ok(None),
            (false, true) => {}
        }
        let searched = match (&self.side, staged, named) {
            (JoinSide::Offer { .. }, true, true) => return Ok(None),
            (JoinSide::Offer { x, vector, order }, _, _) => {
                let Some(at) = order.iter().find(|at| at.source == request.source.hash) else {
                    return Ok(None);
                };
                engine.commit_offer(view, at, x, vector.as_ref(), request)?
            }
            (
                JoinSide::Receive {
                    colls,
                    slots,
                    waiting,
                    ..
                },
                true,
                _,
            ) => engine.commit_receive(view, colls, slots, waiting, request)?,
            (JoinSide::Receive { .. }, false, _) => {
                return Err(SpaceError::Inconsistent(
                    "a join receive lists one continuation",
                ))
            }
        };
        let Some(searched) = searched else {
            return Ok(None);
        };
        self.found = Some(Found {
            staged: false,
            admitted: searched.admitted,
        });
        self.last = Some(searched.fire);
        self.retires = commit_members(request.matcher, searched.others)?;
        Ok(Some(CommitTuple {
            tuple: searched.tuple,
            staged: false,
            spent: searched.spent,
        }))
    }

    /// True when c0's staged tuple passes the item test of `request` at the commit (N3, C4v2
    /// B.4): each member of its list, with the element in flight at its place, or else each bound
    /// datum and each member of each bound class. Without an item test it passes.
    fn staged_admits<Src: JoinSource>(
        &self,
        engine: &Engine<'_, Src>,
        view: &mut CommitView<'_, '_>,
        request: &ParRowRequest<'_>,
    ) -> Result<bool, SpaceError> {
        let Some(item) = &request.item else {
            return Ok(true);
        };
        match (&self.list, &self.side) {
            (None, _) => {}
            (Some(list), JoinSide::Offer { x, .. }) => {
                let element = incoming_datum(x);
                let pairs =
                    Src::Discipline::list_pairs(&list.members, Some((&element, &x.payload)))?;
                return Ok(Src::Discipline::members_admit(
                    request,
                    &list.matched,
                    &pairs,
                )?);
            }
            (Some(list), JoinSide::Receive { .. }) => {
                let pairs = Src::Discipline::list_pairs(&list.members, None)?;
                return Ok(Src::Discipline::members_admit(
                    request,
                    &list.matched,
                    &pairs,
                )?);
            }
        }
        for candidate in &self.tuple {
            match item.admits(&candidate.datum.a, candidate.datum.persist)? {
                true => {}
                false => return Ok(false),
            }
        }
        let mut tested = Vec::<(Claim, bool)>::with_capacity(self.classes.len());
        for (position, class) in &self.classes {
            let Claim::Class { coll, .. } = class else {
                continue;
            };
            let pattern = request
                .patterns
                .get(*position)
                .ok_or(SpaceError::Inconsistent("a position has no pattern"))?;
            match engine.class_items(view, *coll, pattern, class, &mut tested)? {
                true => {}
                false => return Ok(false),
            }
        }
        Ok(true)
    }

    /// c0's staged tuple, with the list retires of its fire.
    fn staged_tuple<T>(
        &mut self,
        request: &ParRowRequest<'_>,
    ) -> Result<CommitTuple<T>, SpaceError> {
        let others = match (&self.list, self.peeks) {
            (Some(list), false) => {
                let mut others = Vec::<RetiredMember>::with_capacity(list.others.len());
                others.extend(list.others.iter().map(|other| RetiredMember {
                    channel: other.channel.clone(),
                    stored: other.stored.clone(),
                    payload: other.payload.clone(),
                    index: other.index,
                }));
                others
            }
            (None, _) | (Some(_), true) => Vec::new(),
        };
        self.found = Some(Found {
            staged: true,
            admitted: true,
        });
        let Some(facts) = self.facts else {
            return Err(SpaceError::Inconsistent(
                "a join row with a tuple has no facts of its fire",
            ));
        };
        self.last = Some(RowFire {
            colls: self.colls.clone(),
            facts,
            sends: self.sends.clone(),
        });
        self.retires = commit_members(request.matcher, others)?;
        Ok(CommitTuple {
            tuple: self.tuple.clone(),
            staged: true,
            spent: None,
        })
    }

    /// The answer for the last tuple: c0 admits with no new run (N3), and a searched tuple gives
    /// the answer of its guard (ADD A1.10 (c) 4).
    pub(crate) fn commit_check(&self) -> Result<bool, SpaceTxnError> {
        let Some(found) = self.found else {
            return Err(SpaceTxnError::Inconsistent(
                "a join row checks a tuple that it did not read",
            ));
        };
        Ok(found.staged || found.admitted)
    }

    /// Clears what the last tuple read. The search takes nothing at the commit.
    pub(crate) fn restore(&mut self) {
        self.found = None;
        self.retires.clear();
        self.fired = None;
        self.last = None;
    }

    /// E3-a (R15) by the one rule of MATCHING, as the discipline searches answer it (ADD A1.11).
    pub(crate) fn may_veto(
        &self,
        matcher: &dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
        continuation: &TaggedContinuation,
    ) -> bool {
        may_veto(matcher, continuation, self.theory.is_some())
    }

    /// The count of the list retires that the staging stages after c0's fire (CR-33).
    pub(crate) fn staged_list_retires(&self) -> usize {
        match &self.list {
            None => 0,
            Some(list) => list.others.len(),
        }
    }

    /// The actual fire of `tuple` for `request`: the retires `retires` of the kind, then the list
    /// retires of the tuple, each on its channel (CR-33). It keeps the claims of the actual unit
    /// (RADD 1.8): the channels of the tuple and of the retires as `Data`, and the group of the
    /// request as `Waiter`. The machinery search of each retire names the discipline `D` of the
    /// search (RADD2 1.6).
    pub(crate) fn fire<D: Discipline>(
        &mut self,
        request: &ParRowRequest<'_>,
        tuple: &[ParCandidate],
        mut retires: Vec<(Par, Member)>,
    ) -> ActualFire<Par, BindPattern, ListParWithRandom, TaggedContinuation> {
        retires.append(&mut self.retires);
        let mut data = Vec::<Par>::with_capacity(tuple.len() + retires.len());
        data.extend(tuple.iter().map(|candidate| candidate.channel.clone()));
        data.extend(retires.iter().map(|(channel, _)| channel.clone()));
        let mut waiters = Vec::<Par>::with_capacity(request.group.len());
        waiters.extend(request.group.iter().cloned());
        self.fired = Some(RowClaims { data, waiters });
        let mut requests = Vec::<ParConsume>::with_capacity(retires.len());
        for (channel, member) in retires {
            requests.push(self.rows.request_on::<D>(&channel, member));
        }
        ActualFire {
            retires: requests,
            dispatched: None,
        }
    }

    /// The claims of the actual unit of the row (RADD 1.8): those of its fire, or else those of
    /// the store of the element of an offer on the channel of the row, or of the park of a
    /// receive on its group (`store_on`, `park_on`).
    pub(crate) fn actual_claims(&self) -> RowClaims<Par> {
        let Some(fired) = &self.fired else {
            return match &self.side {
                JoinSide::Offer { .. } => {
                    let mut data = Vec::<Par>::with_capacity(1);
                    data.push(self.channel.clone());
                    RowClaims {
                        data,
                        waiters: Vec::new(),
                    }
                }
                JoinSide::Receive { group, .. } => RowClaims {
                    data: Vec::new(),
                    waiters: group.clone(),
                },
            };
        };
        fired.clone()
    }

    /// Keeps c0 and each other waiter of an offer whose group the job may claim as `Waiter`
    /// (CR-24 bullet 3, ADD A1.10 (c) 11). The fire of an alternative claims its group as
    /// `Waiter`, and its candidates and the retires of a keyed fire as `Data` on channels of
    /// that group. The read inserts nothing.
    fn keep_admissible(&mut self, seq: &SeqReads) {
        let staged = &self.staged.hash;
        let JoinSide::Offer { order, .. } = &mut self.side else {
            return;
        };
        order.retain(|at| {
            at.source == *staged
                || at
                    .group
                    .iter()
                    .all(|channel| seq.admits(channel, ClaimRole::Waiter))
        });
    }

    /// The allowed pairs of the row that fail `rule` (CR-20 item 2, ADD A1.10 (c) 8): for each
    /// waiter of the row order, or the waiter of a receive, each record of the window of each
    /// collection of its group once, with the element in flight at its place, in DR-75 order.
    pub(crate) fn item_failures<Src: JoinSource>(
        &self,
        source: &Src,
        lists: &ParRowLists<'_>,
        matcher: &dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
        rule: &dyn RowItemRule<TaggedContinuation, ListParWithRandom>,
    ) -> Result<Vec<(Blake2b256Hash, Blake2b256Hash)>, SpaceError> {
        let inputs = Arc::clone(&self.inputs);
        let mut engine = Engine::new(source, &inputs.scope, &inputs.op, &inputs.config)?;
        engine.work = self.work.clone();
        let theory = self.theory.clone();
        let width = inputs.scope.collections.len();
        let mut view = CommitView::new(lists, matcher, theory.as_deref(), None, width);
        let mut failed = Vec::<(Blake2b256Hash, Blake2b256Hash)>::new();
        match &self.side {
            JoinSide::Offer { x, vector, order } => {
                for at in order {
                    let Some(at) = commit_ref(&mut view, at)? else {
                        continue;
                    };
                    let waiter = engine.load(&mut view, at)?;
                    let waiting = &waiter.waiting;
                    // Added by decision 124 (plan v4 C7.9, rules CR-20 item 2 and R5): the one
                    // candidate of a list waiter is its list.
                    match engine.list_waiter(&waiter.slots)? {
                        Some(list) => engine.list_failures(
                            &mut view,
                            &waiter.colls,
                            waiting,
                            Some((x, vector.as_ref())),
                            list,
                            rule,
                            &mut failed,
                        )?,
                        None => engine.waiter_failures(
                            &mut view,
                            &waiter.colls,
                            waiting,
                            Some(x),
                            rule,
                            &mut failed,
                        )?,
                    }
                }
            }
            JoinSide::Receive {
                colls,
                slots,
                waiting,
                ..
            } => match engine.list_waiter(slots)? {
                Some(list) => engine.list_failures(
                    &mut view,
                    colls,
                    waiting,
                    None,
                    list,
                    rule,
                    &mut failed,
                )?,
                None => {
                    engine.waiter_failures(&mut view, colls, waiting, None, rule, &mut failed)?
                }
            },
        }
        Ok(failed)
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.2): the fire of the actual
    /// unit of the row: the fire of the last tuple when the commit fired it, or `None` when the
    /// row stored or parked. The search keeps the claims of the actual fire only (`fire`), so a
    /// fire without the facts of its tuple fails closed.
    fn unit_fire(&self) -> Result<Option<&RowFire>, SpaceError> {
        match (&self.fired, &self.last) {
            (None, _) => Ok(None),
            (Some(_), Some(last)) => Ok(Some(last)),
            (Some(_), None) => Err(SpaceError::Inconsistent(
                "a join row fired a tuple that it did not read",
            )),
        }
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.2): the actual unit of the
    /// row at the commit: its fire, or else the store of the element of an offer, or the park of
    /// a receive, as `actual_claims` reads it.
    pub(crate) fn unit(&self) -> RowUnit<'_> {
        match (&self.fired, &self.side) {
            (Some(_), _) => RowUnit::Fired,
            (None, JoinSide::Offer { x, .. }) => RowUnit::Stored(x),
            (None, JoinSide::Receive { .. }) => RowUnit::Parked,
        }
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.2): true when the run has
    /// changed the state after the actual unit of the row (`Engine::changed`, which the finish
    /// reads): before c0's fire, or by the unit. A fire gives its facts. The store of a fresh
    /// element changes the state, and a second send does not (`store`). A park
    /// changes it (`park_with`).
    pub(crate) fn changed_after(&self) -> Result<bool, SpaceError> {
        let changed = match (self.unit_fire()?, &self.side) {
            (Some(fire), _) => fire.facts.changed,
            (None, JoinSide::Offer { x, .. }) => x.restore.is_none(),
            (None, JoinSide::Receive { .. }) => true,
        };
        Ok(self.before.changed || changed)
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.2): the count of the second
    /// sends that c0's fire owes to its source: each stored record of its tuple that is not
    /// persistent, when the fire peeks (`JoinSource::second_send`). The element in
    /// flight is sent again with no owe.
    pub(crate) fn staged_sends(&self) -> usize {
        match self.peeks {
            false => 0,
            true => self
                .tuple
                .iter()
                .filter(|candidate| candidate.datum_index >= 0 && !candidate.datum.persist)
                .count(),
        }
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.2): the count of the second
    /// sends that c0's cascade fire owes to its source after its search: the lift of the record
    /// that it offers again, which a source owes and marks with its `restore` depth
    /// (`stored_element`).
    pub(crate) fn staged_lift(&self) -> usize {
        match (self.site, &self.side) {
            (FireSite::Cascade, JoinSide::Offer { x, .. }) => usize::from(x.restore.is_some()),
            (FireSite::Offer | FireSite::Single | FireSite::Join, _)
            | (FireSite::Cascade, JoinSide::Receive { .. }) => 0,
        }
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.2, F-JSTEP): part 1 of the
    /// read, the rest of the site of the fire after its step `step`, or `None` when the
    /// site returns to the drain. An offer that peeks returns before its step. An
    /// offer and a receive loop on `Continue`. The round cap stops every site.
    /// A divergent pair stops a cascade
    /// and the offer of a second send, and an own offer and a receive keep their
    /// element in flight with no stop.
    fn site_step(&self, peeks: bool, step: FireStep) -> Option<TailStep> {
        let own = match &self.side {
            JoinSide::Offer { x, .. } => x.own,
            JoinSide::Receive { .. } => true,
        };
        match (self.site, peeks, step) {
            (FireSite::Offer, true, _) => None,
            (_, _, FireStep::Reissue(ReissueCause::RoundCap)) => Some(TailStep::Capped),
            (FireSite::Offer, false, FireStep::Continue)
            | (FireSite::Single | FireSite::Join, _, FireStep::Continue) => Some(TailStep::Stages),
            (
                FireSite::Offer,
                false,
                FireStep::Reissue(ReissueCause::DivergentPair | ReissueCause::OwnerRefusal),
            ) => match own {
                true => None,
                false => Some(TailStep::Commits),
            },
            (
                FireSite::Cascade,
                _,
                FireStep::Reissue(ReissueCause::DivergentPair | ReissueCause::OwnerRefusal),
            ) => Some(TailStep::Commits),
            (FireSite::Offer, false, FireStep::Done)
            | (
                FireSite::Single | FireSite::Join,
                _,
                FireStep::Done
                | FireStep::Reissue(ReissueCause::DivergentPair | ReissueCause::OwnerRefusal),
            )
            | (FireSite::Cascade, _, FireStep::Done | FireStep::Continue) => None,
        }
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD 1.2, RADD2 1.1 and RADD2v2
    /// 6.6, ADD A2.8): the tail read of the row over the lists `lists` of the commit, with
    /// the source `source` of the kind after the unit and the collections `marks` that the unit
    /// marks dirty in its kind. The row keeps the end of Tail(actual) that the engine gives, for
    /// `RowSearch::tail_reissue`. The finish rows of the source join the unit of the row (P1), so
    /// the row keeps the end. It stages nothing.
    pub(crate) fn tail_read<Src: JoinSource>(
        &mut self,
        source: &Src,
        lists: &ParRowLists<'_>,
        matcher: &dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
        marks: &[usize],
    ) -> Result<TailRead, SpaceError> {
        let (step, end) = self.tail_step(source, lists, matcher, marks)?;
        self.reissue = Some(RowReissue::new(end));
        Ok(finish_read(step))
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.1): the end of
    /// Tail(actual) that the last tail read built, taken out of the row.
    pub(crate) fn take_reissue(&mut self) -> Option<RowReissue> { self.reissue.take() }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD 1.2 and RADD2 1.1, ADD
    /// A2.8): parts 1 to 3 of the tail read of the row and the end of Tail(actual). The
    /// engine of the kind starts from the state before c0's fire (`Before`) with the owed work
    /// of the staging (`work`), over the source `source` after the unit, and the unit marks the
    /// collections `marks` dirty in its kind. A store or a park settles the element of the
    /// operation (`settle_unit`). A fire marks its collections, counts one round, pushes its
    /// second sends and runs the rule of its site (`site_state`, `site_step`). Then the drain
    /// and the cascade (`drain_step`), and the end from the re-issue of the engine (`end_of`).
    /// It stages nothing.
    fn tail_step<Src: JoinSource>(
        &self,
        source: &Src,
        lists: &ParRowLists<'_>,
        matcher: &dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
        marks: &[usize],
    ) -> Result<(TailStep, TailEnd), SpaceError> {
        let inputs = Arc::clone(&self.inputs);
        let mut engine = Engine::new(source, &inputs.scope, &inputs.op, &inputs.config)?;
        engine.dirty = self.before.dirty.clone();
        engine.own = self.before.own;
        engine.own_cause = self.before.own_cause;
        engine.ords = self.before.ords.clone();
        engine.work = self.work.clone();
        mark(&mut engine.dirty, marks);
        let mut fired = self.before.fired;
        let site = match self.unit_fire()? {
            None => {
                self.settle_unit(&mut engine);
                None
            }
            Some(fire) => {
                mark(&mut engine.dirty, &fire.colls);
                let mut rounds = FireRounds::at(fired);
                let step = rounds.after_fire(fire.facts.spent, fire.facts.stays);
                fired = rounds.fired();
                engine.work.extend(fire.sends.iter().cloned());
                self.site_state(&mut engine, fire.facts.peeks, step)?;
                self.site_step(fire.facts.peeks, step)
            }
        };
        let step = match site {
            Some(step) => step,
            None => self.drain_step(&mut engine, lists, matcher, fired)?,
        };
        let end = self.end_of(engine, step)?;
        Ok((step, end))
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.1, S8-MIRROR): the state
    /// of the element of the operation after a unit with no fire, as the engine leaves it: the
    /// store of the element of the operation settles it (`store`), and so does
    /// every park, which parks the waiter of the operation (`park`).
    fn settle_unit<Src: JoinSource>(&self, engine: &mut Engine<'_, Src>) {
        let settles = match &self.side {
            JoinSide::Offer { x, .. } => x.own,
            JoinSide::Receive { .. } => true,
        };
        match settles {
            false => {}
            true => engine.own = Own::Settled,
        }
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.1, S8-MIRROR): the fields
    /// of the engine that the site of the fire of the row sets after its step `step`, as the
    /// sites set them. An offer that peeks returns before its step. An offer
    /// that is done settles the element of the operation. An offer that
    /// re-issues keeps the element of the operation in flight with its cause, or sends a second
    /// send back to the work stack, and stops. An offer of a second send that
    /// goes on keeps it in flight, so at the stop of C2 E4.5 (`end_of`) it goes back to the work
    /// stack, as the stop of the offer sends it back. A receive is done or re-issues as
    /// `after_receive` sets. A cascade that re-issues stops.
    /// Every other step sets no field.
    fn site_state<Src: JoinSource>(
        &self,
        engine: &mut Engine<'_, Src>,
        peeks: bool,
        step: FireStep,
    ) -> Result<(), SpaceError> {
        match (self.site, peeks, step) {
            (FireSite::Offer, true, _)
            | (FireSite::Single | FireSite::Join | FireSite::Cascade, _, FireStep::Continue)
            | (FireSite::Cascade, _, FireStep::Done) => {}
            (FireSite::Offer, false, step) => {
                let JoinSide::Offer { x, .. } = &self.side else {
                    return Err(SpaceError::Inconsistent("an offer row has no element"));
                };
                match (step, x.own) {
                    (FireStep::Done, false) | (FireStep::Continue, true) => {}
                    (FireStep::Done, true) => engine.own = Own::Settled,
                    (FireStep::Reissue(cause), true) => {
                        engine.own = Own::InFlight;
                        engine.own_cause = Some(cause);
                        engine.stop = match cause {
                            ReissueCause::RoundCap => Some(cause),
                            ReissueCause::DivergentPair | ReissueCause::OwnerRefusal => engine.stop,
                        };
                    }
                    (FireStep::Continue | FireStep::Reissue(_), false) => {
                        engine.work.push(Work {
                            coll: x.coll,
                            record: x.record.clone(),
                            persist: x.persist,
                            own: false,
                            restore: x.restore,
                        });
                        engine.stop = match step {
                            FireStep::Reissue(cause) => Some(cause),
                            FireStep::Done | FireStep::Continue => engine.stop,
                        };
                    }
                }
            }
            (FireSite::Single | FireSite::Join, _, FireStep::Done) => engine.own = Own::Settled,
            (FireSite::Single | FireSite::Join, _, FireStep::Reissue(cause)) => {
                engine.own = Own::InFlight;
                engine.own_cause = Some(cause);
                engine.stop = match cause {
                    ReissueCause::RoundCap => Some(cause),
                    ReissueCause::DivergentPair | ReissueCause::OwnerRefusal => engine.stop,
                };
            }
            (FireSite::Cascade, _, FireStep::Reissue(cause)) => engine.stop = Some(cause),
        }
        Ok(())
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD 1.2 and RADD2 1.1): parts 2
    /// and 3 of the read after `fired` fires, from the state `engine` after the site, as
    /// `drain_and_cascade` and `run` run them: owed work is a step below the cap,
    /// a fireable head is a step below the cap, and otherwise
    /// the run ends (`after_drain`). The cascade search reads the book of the staging (C2v3 4.7).
    fn drain_step<Src: JoinSource>(
        &self,
        engine: &mut Engine<'_, Src>,
        lists: &ParRowLists<'_>,
        matcher: &dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
        fired: usize,
    ) -> Result<TailStep, SpaceError> {
        match engine.work.is_empty() {
            true => {}
            false => return Ok(round_cap(fired)),
        }
        match engine.cascades() {
            true => {}
            false => return Ok(after_drain(engine, fired)),
        }
        let width = self.inputs.scope.collections.len();
        let mut view = CommitView::of_tail(
            lists,
            matcher,
            self.theory.as_deref(),
            None,
            &self.book,
            width,
        );
        match engine.next_fire(&mut view)? {
            None => Ok(after_drain(engine, fired)),
            Some(_) => Ok(round_cap(fired)),
        }
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.1, C2 E4.5): the end of
    /// Tail(actual) of the row from the engine `engine` at the end `step` of its read, by the
    /// re-issue of the engine (`Engine::reissue_of`). `Commits`: the run commits as it stands,
    /// so the end returns its re-issue. `Capped`: the cap stops the run,
    /// so the end returns its re-issue with the stop of the cap.
    /// `Stages`: the run would stage one more step, so the end stops the operation there for an
    /// owner refusal with the re-issue of the rest (CR-19 case 2, `Engine::refusal_stop`).
    fn end_of<Src: JoinSource>(
        &self,
        mut engine: Engine<'_, Src>,
        step: TailStep,
    ) -> Result<TailEnd, SpaceError> {
        let op = SpaceOp::clone(&self.inputs.op);
        match step {
            TailStep::Commits => Ok(TailEnd::Return(engine.reissue_of(op)?)),
            TailStep::Capped => {
                engine.stop = Some(ReissueCause::RoundCap);
                Ok(TailEnd::Return(engine.reissue_of(op)?))
            }
            TailStep::Stages => engine.refusal_stop(op),
        }
    }
}

/// Added by decision 124 (plan v4 C7.9, ruling RADD 1.5): the waiter `at` of the staging at its
/// place in the store of the commit, found by the source hash of its consume, or `None` when the
/// store of the commit does not hold it.
fn commit_ref<V: StoreView>(view: &mut V, at: &WaiterRef) -> Result<Option<WaiterRef>, SpaceError> {
    let waiters = view.group_waiters(&at.group)?;
    let Some(index) = waiters
        .iter()
        .position(|waiter| waiter.source.hash == at.source)
    else {
        return Ok(None);
    };
    Ok(Some(WaiterRef {
        group: at.group.clone(),
        index,
        source: at.source.clone(),
    }))
}

/// Added by decision 124 (plan v4 C7.9, ruling RADD 1.7): the element in flight `x` as a record of
/// its collection, for the item test of the members of a list (C4v2 B.4).
fn incoming_datum(x: &Incoming) -> ParDatum {
    Datum {
        a: x.record.clone(),
        persist: x.persist,
        source: x.source.clone(),
    }
}

/// Added by decision 124 (plan v4 C7.9, rulings RADD 1.5 and 1.7, CR-13): the members of the list
/// `members` on `channel` that its fire retires after it: each stored member that is not
/// persistent, in list order, other than the member that the fire binds, as
/// `retire_members` retires them. The fire binds the element in flight at its place
/// (an offer), or else the first member (a receive, or a cascade, which lifts it).
fn list_retires<D: Discipline>(
    members: &[Option<(ParDatum, usize)>],
    channel: &Par,
) -> Result<Vec<RetiredMember>, SpaceError> {
    let lead = members.iter().position(Option::is_none).unwrap_or(0);
    let mut retired = Vec::<RetiredMember>::with_capacity(members.len());
    for (place, member) in members.iter().enumerate() {
        let Some((stored, index)) = member else {
            continue;
        };
        match (place == lead, stored.persist) {
            (false, false) => {}
            (true, _) | (false, true) => continue,
        }
        retired.push(RetiredMember {
            channel: channel.clone(),
            stored: stored.clone(),
            payload: D::similarity_payload(&stored.a)?,
            index: *index,
        });
    }
    Ok(retired)
}

/// Added by decision 124 (plan v4 C7.9, ruling RADD 1.5): the member of each record of `retired`
/// with the value that the retire pattern binds (`RetireRows::member`), with its channel.
pub(crate) fn commit_members(
    matcher: &dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
    retired: Vec<RetiredMember>,
) -> Result<Vec<(Par, Member)>, SpaceTxnError> {
    let mut members = Vec::<(Par, Member)>::with_capacity(retired.len());
    for retired in retired {
        let member = RetireRows::member(matcher, &retired.stored, &retired.payload, retired.index)?;
        members.push((retired.channel, member));
    }
    Ok(members)
}

impl<'r, Src: JoinSource> Engine<'r, Src> {
    fn new(
        source: &'r Src,
        scope: &'r JoinScope,
        op: &SpaceOp,
        config: &SpaceConfig,
    ) -> Result<Self, SpaceError> {
        // Changed by decision 128 (kind routing): the discipline of the source gives the step,
        // and the local takes the name of the window. The text before:
        // ```
        // let vdb = match source.shape() {
        //     JoinWindow::Similarity => Some(vectordb::vdb_config(config)?),
        // ```
        let similarity = match source.shape() {
            JoinWindow::Similarity => Some(Src::Discipline::similarity_config(config)?),
            JoinWindow::Head | JoinWindow::Lanes | JoinWindow::Scan | JoinWindow::Classes => None,
        };
        let (random_state, authority): (&[u8], Option<&CostAuthority>) = match op {
            SpaceOp::Produce(op) => (&op.data.random_state, op.data.cost_authority.as_ref()),
            SpaceOp::Consume(op) => (&op.body.random_state, record::stamped(&op.authority)),
            SpaceOp::Settle(op) => (&op.random_state, record::stamped(&op.authority)),
            SpaceOp::Join(op) => (&op.body.random_state, record::stamped(&op.authority)),
        };
        let width = scope.collections.len();
        let mut bumps = Vec::<Option<Option<i64>>>::with_capacity(width);
        bumps.resize(width, None);
        let mut dirty = Vec::<bool>::with_capacity(width);
        dirty.resize(width, false);
        let mut ords = Vec::<Option<i64>>::with_capacity(width);
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
            authority: authority.cloned(),
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
            // Changed by decision 128 (kind routing): the field takes the name of the window. The
            // text before:
            // vdb,
            similarity,
            // Added by decision 124 (plan v4 C7.9, ruling OP-J): `run` sets it.
            commit_inputs: None,
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
                distinct.push(coll);
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
        // Added by decision 124 (plan v4 C7.9, rulings OP-J and RADD 1.3, RC2 D-10): the row
        // searches of an owner-funded run share one copy of the inputs of `Engine::new`, with the
        // copy of the operation that the plan keeps. A run without owner funding copies nothing.
        // Changed by decision 124 (plan v4 C7.11, rule CR-33, ruling RADD2v2 6.6): the copy also
        // holds the parts of the run that the finish rows of a search need (P1).
        self.commit_inputs = match cx.keep_operation(&op) {
            None => None,
            Some(op) => Some(Arc::new(EngineInputs {
                op,
                scope: self.scope.clone(),
                config: cx.config().clone(),
                random: self.random.clone(),
                authority: self.authority.clone(),
                introduction: Arc::clone(cx.introduction()),
                job: cx.job().clone(),
            })),
        };
        let mut own_colls = Vec::<usize>::with_capacity(self.scope.own.len());
        for key in &self.scope.own {
            own_colls.push(self.scope.index_of_key(key)?);
        }
        for &coll in &own_colls {
            self.touch(cx, coll)?;
        }
        let carried = self.carried_ords(&op, &own_colls)?;
        for (&coll, &carried_ord) in own_colls.iter().zip(&carried) {
            let ord = operation_ord(carried_ord, self.bumped(coll)?)?;
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
                let mut work = Vec::<Work>::with_capacity(pending.len());
                for pending in pending.into_iter().rev() {
                    let coll = self.scope.index_of_key(&pending.key)?;
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
                let waiter = self.consume_waiter(&consume, &own_colls, &carried)?;
                self.receive(cx, waiter)?;
                SpaceOp::Consume(consume)
            }
            SpaceOp::Join(join) => {
                self.own = Own::InFlight;
                let waiter = self.join_waiter(&join, &own_colls, &carried)?;
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
    ) -> Result<Vec<Option<i64>>, SpaceError> {
        let mut carried = Vec::<Option<i64>>::with_capacity(own_colls.len());
        match op {
            SpaceOp::Produce(produce) => carried.push(produce.reissued_ord),
            SpaceOp::Consume(consume) => carried.push(consume.reissued_ord),
            SpaceOp::Settle(_) => carried.push(None),
            SpaceOp::Join(join) => {
                carried.resize(own_colls.len(), None);
                if let Some(ords) = &join.reissued_ords {
                    for (bind, ord) in join.binds.iter().zip(ords) {
                        let coll = self.scope.index_of_key(&bind.key)?;
                        let position = own_colls
                            .iter()
                            .position(|&own| own == coll)
                            .ok_or(SpaceError::Inconsistent("a bind is not an own collection"))?;
                        let slot = carried
                            .get_mut(position)
                            .ok_or(SpaceError::Inconsistent("a carried order has no slot"))?;
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
        let ord = self.ord_on(coll)?;
        let level = match self.source.shape() {
            // Changed by decision 128 (kind routing): the discipline of the source gives the step.
            // The text before:
            // JoinWindow::Lanes => Some(send_level(&op.params)?),
            JoinWindow::Lanes => Some(Src::Discipline::send_level(&op.params)?),
            JoinWindow::Head | JoinWindow::Scan | JoinWindow::Classes | JoinWindow::Similarity => {
                None
            }
        };
        let record = match self.source.shape() {
            // The record of a VectorDB send holds its vector (P6, design v3 4.3).
            JoinWindow::Similarity => {
                // Changed by decision 128 (kind routing): the discipline of the source gives the
                // step. The text before:
                // ```
                // let cfg = vectordb::vdb_config(cx.config())?;
                // vectordb::vdb_record(&cfg, &op.params, &op.data)?.0
                // ```
                let cfg = Src::Discipline::similarity_config(cx.config())?;
                Src::Discipline::similarity_record(&cfg, &op.params, &op.data)?.0
            }
            JoinWindow::Head | JoinWindow::Lanes | JoinWindow::Scan | JoinWindow::Classes => {
                self.source.wrap_record(ord, level, &op.data)?
            }
        };
        let source = Produce::create(self.source.home(self.scope, coll)?, &record, op.persistent);
        Ok(Incoming {
            coll,
            payload: op.data.clone(),
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
        op: &ConsumeOp,
        own_colls: &[usize],
        carried: &[Option<i64>],
    ) -> Result<NewWaiter, SpaceError> {
        let coll = *own_colls
            .first()
            .ok_or(SpaceError::Inconsistent("a consume has no collection"))?;
        let ord = self.ord_on(coll)?;
        let lane = match self.source.shape() {
            // Changed by decision 128 (kind routing): the discipline of the source gives the step.
            // The text before:
            // JoinWindow::Lanes => Some(receive_lane(&op.query)?),
            JoinWindow::Lanes => Some(Src::Discipline::receive_lane(&op.query)?),
            JoinWindow::Head | JoinWindow::Scan | JoinWindow::Classes | JoinWindow::Similarity => {
                None
            }
        };
        // A VectorDB waiter keeps its query in its policy slot (P6, design v3 4.3).
        let query_policy = match self.source.shape() {
            // Changed by decision 128 (kind routing): the discipline of the source gives the step.
            // The text before:
            // JoinWindow::Similarity => Some(vectordb::policy_of_query(op.query.as_ref())),
            JoinWindow::Similarity => Some(Src::Discipline::policy_of_query(op.query.as_ref())),
            JoinWindow::Head | JoinWindow::Lanes | JoinWindow::Scan | JoinWindow::Classes => None,
        };
        let continuation = match (lane, &query_policy) {
            (Some(lane), _) => record::wrap_pq_continuation_from_parts(
                ord,
                lane.slot(),
                &op.body,
                &op.guard,
                record::stamped(&op.authority),
            )?,
            (None, Some(policy)) => record::wrap_policy_continuation_from_parts(
                ord,
                policy,
                &op.body,
                &op.guard,
                record::stamped(&op.authority),
            )?,
            (None, None) => record::wrap_continuation_from_parts(
                ord,
                &op.body,
                &op.guard,
                record::stamped(&op.authority),
            )?,
        };
        let mut group = Vec::<Par>::with_capacity(1);
        group.push(self.channel(coll)?.clone());
        let mut colls = Vec::<usize>::with_capacity(1);
        colls.push(coll);
        let mut slots = Vec::<JoinSlot>::with_capacity(1);
        slots.push(JoinSlot {
            ord,
            bind: 0,
            policy: match query_policy {
                Some(policy) => policy,
                None => WaiterPolicy::of_level(lane.and_then(Lane::slot)),
            },
        });
        let mut patterns = Vec::<BindPattern>::with_capacity(1);
        patterns.push(op.pattern.clone());
        let mut carried_by_coll = Vec::<Option<i64>>::with_capacity(self.scope.collections.len());
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
            peeks: peek_set(op.peek),
            carried: carried_by_coll,
        })
    }

    /// The waiter of the join `op`: its positions in canonical group order, a record of version
    /// 2 with one slot for each position.
    fn join_waiter(
        &self,
        op: &JoinOp,
        own_colls: &[usize],
        carried: &[Option<i64>],
    ) -> Result<NewWaiter, SpaceError> {
        let width = self.scope.group.len();
        let mut group = Vec::<Par>::with_capacity(width);
        let mut colls = Vec::<usize>::with_capacity(width);
        let mut slots = Vec::<JoinSlot>::with_capacity(width);
        let mut patterns = Vec::<BindPattern>::with_capacity(width);
        for &position in &self.scope.group {
            let bind = op
                .binds
                .get(position)
                .ok_or(SpaceError::Inconsistent("a group position names no bind"))?;
            let coll = self.scope.index_of_key(&bind.key)?;
            let lane = match self.source.shape() {
                // Changed by decision 128 (kind routing): the discipline of the source gives the
                // step. The text before:
                // JoinWindow::Lanes => Some(receive_lane(&bind.query)?),
                JoinWindow::Lanes => Some(Src::Discipline::receive_lane(&bind.query)?),
                JoinWindow::Head
                | JoinWindow::Scan
                | JoinWindow::Classes
                | JoinWindow::Similarity => None,
            };
            // A VectorDB bind keeps its query in its slot (P6, design v3 4.6).
            let policy = match self.source.shape() {
                // Changed by decision 128 (kind routing): the discipline of the source gives the
                // step. The text before:
                // JoinWindow::Similarity => vectordb::policy_of_query(bind.query.as_ref()),
                JoinWindow::Similarity => Src::Discipline::policy_of_query(bind.query.as_ref()),
                JoinWindow::Head | JoinWindow::Lanes | JoinWindow::Scan | JoinWindow::Classes => {
                    WaiterPolicy::of_level(lane.and_then(Lane::slot))
                }
            };
            group.push(self.channel(coll)?.clone());
            colls.push(coll);
            slots.push(JoinSlot {
                ord: self.ord_on(coll)?,
                bind: position,
                policy,
            });
            patterns.push(bind.pattern.clone());
        }
        let continuation = record::wrap_join_continuation_from_parts(
            &slots,
            &op.body,
            &op.guard,
            record::stamped(&op.authority),
        )?;
        let peeks = match op.peek {
            true => peek_set_all(width)?,
            false => BTreeSet::new(),
        };
        let mut carried_by_coll = Vec::<Option<i64>>::with_capacity(self.scope.collections.len());
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
    // Changed by decision 124 (plan v4 C7.9, ruling OP-J): the reads of this fn go through the
    // view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn load(&self, cx: &mut Cx<'_, '_>, at: WaiterRef) -> Result<Waiter, SpaceError> {
    fn load<V: StoreView>(&self, view: &mut V, at: WaiterRef) -> Result<Waiter, SpaceError> {
        let mut colls = Vec::<usize>::with_capacity(at.group.len());
        for channel in &at.group {
            colls.push(self.scope.index_of_channel(channel)?);
        }
        let waiting = {
            // let waiters = cx.group_waiters(&at.group)?;
            let waiters = view.group_waiters(&at.group)?;
            let waiter = waiters.get(at.index).ok_or(SpaceError::Inconsistent(
                "a waiter index is outside the staged view",
            ))?;
            waiter.clone()
        };
        let slots = self.source.waiter_slots(&waiting, at.group.len())?;
        Ok(Waiter {
            at,
            colls,
            slots,
            waiting,
        })
    }

    /// The lanes of the positions of a waiter with the slots `slots`.
    fn lanes(&self, slots: &[JoinSlot]) -> Vec<Option<Lane>> {
        let mut lanes = Vec::<Option<Lane>>::with_capacity(slots.len());
        lanes.extend(slots.iter().map(|slot| match self.source.shape() {
            JoinWindow::Lanes => Some(Lane::of_slot(slot.policy.level())),
            JoinWindow::Head | JoinWindow::Scan | JoinWindow::Classes | JoinWindow::Similarity => {
                None
            }
        }));
        lanes
    }

    /// True when the stored waiter `waiter` is the head of each head-only collection of its
    /// group (decision 11).
    // Changed by decision 124 (plan v4 C7.9, ruling OP-J): the reads of this fn go through the
    // view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn all_at_head(&self, cx: &mut Cx<'_, '_>, waiter: &Waiter) -> Result<bool, SpaceError> {
    fn all_at_head<V: StoreView>(&self, view: &mut V, waiter: &Waiter) -> Result<bool, SpaceError> {
        let mut checked: Vec<usize> = Vec::new();
        for &coll in &waiter.colls {
            if checked.contains(&coll) {
                continue;
            }
            checked.push(coll);
            if !self.source.at_head(
                // cx,
                view,
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
    /// Added by decision 102 (plan v4 C5.3): the lanes come from `slots`, and `order` gives the
    /// order of each window ([`Engine::window_order`]).
    // Changed by decision 124 (plan v4 C7.9, ruling OP-J): the reads of this fn go through the
    // view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // Each bind of a datum runs the item test of the view (rule R5): a refused datum is a miss.
    // fn search(
    fn search<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        colls: &[usize],
        patterns: &[BindPattern],
        // Changed by decision 102 (plan v4 C5.3): the lanes come from the slots.
        // lanes: &[Option<Lane>],
        slots: &[JoinSlot],
        // Changed by decision 103 (decision 118, rule CR-9, decision-c6 §4.1): the tuple carries
        // the vector of x.
        // fixed: Option<(usize, &Incoming, ListParWithRandom)>,
        fixed: Option<(usize, &Incoming, ListParWithRandom, Option<&ExactVector>)>,
        // Added by decision 102 (plan v4 C5.3): the order of the windows.
        order: WindowOrder<'_>,
    ) -> Result<Option<Vec<Bound>>, SpaceError> {
        // Changed by decision 102 (plan v4 C5.3): the lanes come from the slots, so the order
        // fits seven inputs.
        let lanes = self.lanes(slots);
        let width = colls.len();
        if patterns.len() != width || lanes.len() != width {
            return Err(SpaceError::Inconsistent(
                "a waiter has not one pattern and one lane for each position",
            ));
        }
        let mut bound: Vec<Option<Bound>> = Vec::with_capacity(width);
        bound.resize(width, None);
        // Changed by decision 103 (decision 118, rule CR-9): a non-persistent set member claims its
        // class and its record.
        // let mut claimed: Vec<Claim> = Vec::with_capacity(width);
        let mut claimed: Vec<Claim> = Vec::with_capacity(2 * width);
        // Added by decision 103 (decision 118, rule CR-9, K12): each claimed set class that a
        // later position met, with whether the class has a persistent member.
        let mut free: Vec<(Claim, bool)> = Vec::new();
        // Added by decision 124 (plan v4 C7.9, ruling RADD 1.5, C4v2 B.4): the answer of each
        // class whose members the commit tested.
        let mut items: Vec<(Claim, bool)> = Vec::new();
        // Changed by decision 103 (decision 118, rule CR-9): the tuple carries the vector of x.
        // let incoming = fixed.as_ref().map(|(_, x, _)| *x);
        let incoming = fixed.as_ref().map(|(_, x, _, _)| *x);
        // Added by decision 103 (decision 118, rule CR-9, decision-c6 §4.1): the vector of x.
        let x_vector = fixed.as_ref().and_then(|(_, _, _, vector)| *vector);
        // Added by decision 103 (decision 118, rule CR-11, U3 (i)): the collection of the pinned
        // position. A persistent x binds a later position only there.
        let fired = fixed
            .as_ref()
            .and_then(|(position, _, _, _)| colls.get(*position).copied());
        // Added by decision 103 (decision 118, rule CR-9): the claim of x.
        let mut x_claim: Option<Claim> = None;
        // Changed by decision 103 (decision 118, rule CR-9): the tuple carries the vector of x.
        // if let Some((position, x, matched)) = fixed {
        if let Some((position, x, matched, _)) = fixed {
            let claim = self.source.claim(x.coll, EntryAt::Incoming, &x.payload)?;
            // Changed by decision 103 (decision 118, rule CR-9): a persistent x is never claimed.
            // claimed.push(claim.clone());
            match x.persist {
                true => {}
                false => claimed.push(claim.clone()),
            }
            x_claim = Some(claim.clone());
            if let Some(slot) = bound.get_mut(position) {
                *slot = Some(Bound {
                    at: EntryAt::Incoming,
                    matched,
                    persist: x.persist,
                    claim,
                });
            }
        }
        // Added by decision 103 (decision 118, rule CR-9): x and its claim.
        let x_entry = incoming.zip(x_claim.as_ref());
        let mut windows: BTreeMap<usize, Vec<Entry>> = BTreeMap::new();
        for position in 0..width {
            if bound.get(position).is_some_and(Option::is_some) {
                continue;
            }
            let coll = *colls
                .get(position)
                .ok_or(SpaceError::Inconsistent("a position has no collection"))?;
            // Changed by decision 102 (plan v4 C5.3): `similarity_bind` reads the config itself.
            // if let Some(cfg) = &self.vdb {
            if self.similarity.is_some() {
                let pattern = patterns
                    .get(position)
                    .ok_or(SpaceError::Inconsistent("a position has no pattern"))?;
                let slot = slot_at(slots, position)?;
                // Added by decision 102 (plan v4 C5.3 and C5.5, rule CR-8): the order of the
                // eligible records of the bind.
                let need = colls.iter().filter(|&&other| other == coll).count();
                let pick = Self::window_order(need, order);
                // Added by decision 103 (decision 118, rules CR-8 and CR-9): a persistent x comes
                // first at a later position of its fired collection, through its flight bind.
                let flight = match x_entry {
                    Some((x, claim)) if x.persist && fired == Some(coll) => {
                        let cfg = self
                            .similarity
                            .ok_or(SpaceError::Inconsistent(BIND_OUTSIDE_WINDOW))?;
                        self.similarity_flight_bind(view, &cfg, &slot.policy, pattern, x, x_vector)?
                            .map(|matched| Bound {
                                at: EntryAt::Incoming,
                                matched,
                                persist: true,
                                claim: claim.clone(),
                            })
                    }
                    Some(_) | None => None,
                };
                // Changed by decision 102 (plan v4 C5.3) and decision 103 (decision 118, rules
                // CR-8 and CR-9): the bind of the stored records takes the order, and it runs only
                // without a flight bind. The text before:
                // let Some(found) =
                //     self.similarity_bind(cx, cfg, coll, &slot.policy, pattern, &claimed)?
                // else {
                //     return Ok(None);
                // };
                let found = match flight {
                    Some(found) => Some(found),
                    None => {
                        self.similarity_bind(view, coll, &slot.policy, pattern, &claimed, pick)?
                    }
                };
                let Some(found) = found else {
                    return Ok(None);
                };
                // Changed by decision 103 (decision 118, rule CR-9): a persistent record is never
                // claimed.
                // claimed.push(found.claim.clone());
                match found.persist {
                    true => {}
                    false => claimed.push(found.claim.clone()),
                }
                if let Some(entry) = bound.get_mut(position) {
                    *entry = Some(found);
                }
                continue;
            }
            if !windows.contains_key(&coll) {
                let need = colls.iter().filter(|&&other| other == coll).count();
                // Added by decision 102 (plan v4 C5.3 and C5.5, rule CR-8): the order of the
                // window.
                let pick = Self::window_order(need, order);
                // Changed by decision 102 (plan v4 C5.3): the window takes the order.
                // let window = self
                //     .source
                //     .window(cx, self.scope, coll, need, incoming, &self.work)?;
                let window = self
                    .source
                    .window(view, self.scope, coll, need, incoming, &self.work, pick)?;
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
            // Changed by decision 103 (decision 118, rule CR-9): the entry and its record claim.
            // let mut found: Option<Bound> = None;
            let mut found: Option<(Bound, Claim)> = None;
            for entry in window.iter() {
                // Changed by decision 103 (decision 118, rules CR-9 and CR-11): a persistent x
                // binds a later position of its fired collection. Elsewhere x counts as claimed.
                // let EntryAt::Stored(index) = entry.at else {
                //     continue;
                // };
                let index = match (entry.at, x_entry) {
                    (EntryAt::Stored(index), _) => index,
                    (EntryAt::Incoming, Some((x, claim))) if x.persist && fired == Some(coll) => {
                        if !visible(lane, entry.level)? {
                            continue;
                        }
                        let payload = self.source.incoming_payload(self.scope, x, coll)?;
                        if let Some(matched) = view.bind_item(pattern, &payload, x.persist)? {
                            let bound = Bound {
                                at: EntryAt::Incoming,
                                matched,
                                persist: true,
                                claim: claim.clone(),
                            };
                            found = Some((bound, claim.clone()));
                            break;
                        }
                        continue;
                    }
                    (EntryAt::Incoming, _) => continue,
                };
                if !visible(lane, entry.level)? {
                    continue;
                }
                let stored = self.source.stored_entry(
                    // cx,
                    view,
                    self.scope,
                    coll,
                    index,
                    "a window index is outside the staged view",
                )?;
                // Changed by decision 103 (decision 118, rule CR-9): a non-persistent record binds
                // once, and a claimed class blocks a member only when no member is persistent.
                // if is_claimed(&claimed, &stored.claim)? {
                //     continue;
                // }
                if is_claimed(&claimed, &stored.record)? {
                    continue;
                }
                if !stored.persist
                    && is_claimed(&claimed, &stored.claim)?
                    && !self.class_free(view, coll, window, &stored.claim, x_entry, &mut free)?
                {
                    continue;
                }
                // if let Some(matched) = cx.bind(pattern, &stored.payload) {
                if let Some(matched) = view.bind_item(pattern, &stored.payload, stored.persist)? {
                    // Added by decision 124 (plan v4 C7.9, ruling RADD 1.5, C4v2 B.4): at the
                    // commit, an entry of a class binds only when every member of its class
                    // passes the item test. The staging tests no item.
                    match (view.tests_items(), &stored.claim) {
                        (false, _) | (true, Claim::Record { .. }) => {}
                        (true, Claim::Class { .. }) => {
                            match self.class_items(
                                view,
                                coll,
                                pattern,
                                &stored.claim,
                                &mut items,
                            )? {
                                true => {}
                                false => continue,
                            }
                        }
                    }
                    // Changed by decision 103 (decision 118, rule CR-9): the entry and its record
                    // claim.
                    // found = Some(Bound {
                    //     at: entry.at,
                    //     matched,
                    //     persist: stored.persist,
                    //     claim: stored.claim,
                    // });
                    found = Some((
                        Bound {
                            at: entry.at,
                            matched,
                            persist: stored.persist,
                            claim: stored.claim,
                        },
                        stored.record,
                    ));
                    break;
                }
            }
            // Changed by decision 103 (decision 118, rule CR-9): a persistent entry is never
            // claimed. A non-persistent entry claims its record, and on a set its class too.
            // let Some(found) = found else {
            //     return Ok(None);
            // };
            // claimed.push(found.claim.clone());
            let Some((found, record)) = found else {
                return Ok(None);
            };
            match (found.persist, found.claim == record) {
                (true, _) => {}
                (false, true) => claimed.push(record),
                (false, false) => {
                    claimed.push(found.claim.clone());
                    claimed.push(record);
                }
            }
            if let Some(slot) = bound.get_mut(position) {
                *slot = Some(found);
            }
        }
        Ok(bound.into_iter().collect())
    }

    /// Added by decision 103 (decision 118, rule CR-9, K12): true when the set class `class` has
    /// a persistent member in `window` of `coll`, or is the class of a persistent x in flight.
    /// `free` keeps each answer for the rest of one search.
    // Changed by decision 124 (plan v4 C7.9, ruling OP-J): the reads of this fn go through the
    // view `view` in place of `cx`. The comment above a changed line keeps its text before.
    fn class_free<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        coll: usize,
        window: &[Entry],
        class: &Claim,
        x: Option<(&Incoming, &Claim)>,
        free: &mut Vec<(Claim, bool)>,
    ) -> Result<bool, SpaceError> {
        for (known, answer) in free.iter() {
            if is_claimed(std::slice::from_ref(known), class)? {
                return Ok(*answer);
            }
        }
        let mut answer = match x {
            Some((x, claim)) => x.persist && is_claimed(std::slice::from_ref(claim), class)?,
            None => false,
        };
        for entry in window {
            if answer {
                break;
            }
            let EntryAt::Stored(index) = entry.at else {
                continue;
            };
            let stored = self.source.stored_entry(
                // cx,
                view,
                self.scope,
                coll,
                index,
                "a window index is outside the staged view",
            )?;
            answer = stored.persist && is_claimed(std::slice::from_ref(class), &stored.claim)?;
        }
        free.push((class.clone(), answer));
        Ok(answer)
    }

    /// Added by decision 102 (plan v4 C5.3 and C5.5, rule CR-8): the order of the window of a
    /// collection that the group names `need` times. A collection that the group names two or
    /// more times takes one deterministic list (E1). Every other collection takes `order`.
    fn window_order<'s>(need: usize, order: WindowOrder<'s>) -> WindowOrder<'s> {
        match need {
            1 => order,
            _ => WindowOrder::Canonical,
        }
    }

    /// The guard of `continuation` over the values of `bound` in the bind order of `slots`
    /// (decision 37: once, after every bind matched), and then the check of the theory of the
    /// space on each stored entry of `bound`, whose collection `colls` gives at its position
    /// (P7, TH-6). The element in flight was admitted at its send, so it is not checked. The
    /// stored payloads are read only for a space with a theory and only after the guard admitted
    /// the match, so a space without a theory does no work for them.
    // Changed by decision 124 (plan v4 C7.9, ruling OP-J): the reads of this fn go through the
    // view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn admits(
    fn admits<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        continuation: &TaggedContinuation,
        slots: &[JoinSlot],
        colls: &[usize],
        bound: &[Bound],
    ) -> Result<bool, SpaceError> {
        let mut by_bind: Vec<Option<usize>> = Vec::with_capacity(bound.len());
        by_bind.resize(bound.len(), None);
        for (position, slot) in slots.iter().enumerate() {
            let place = by_bind.get_mut(slot.bind).ok_or(SpaceError::Inconsistent(
                "a bind index is outside the binds",
            ))?;
            *place = Some(position);
        }
        let mut values = Vec::<ListParWithRandom>::with_capacity(bound.len());
        for position in by_bind {
            let position = position.ok_or(SpaceError::Inconsistent("a bind has no position"))?;
            let entry = bound
                .get(position)
                .ok_or(SpaceError::Inconsistent("a position has no bound entry"))?;
            values.push(entry.matched.clone());
        }
        // if !cx.admits_all(continuation, &values, &[])? {
        if !view.admits_all(continuation, &values, &[])? {
            return Ok(false);
        }
        // if !cx.has_theory() {
        if !view.has_theory() {
            return Ok(true);
        }
        // Changed by decision 124 (L6): one count for the check and the fire. The text before:
        // let count = bound
        //     .iter()
        //     .filter(|entry| matches!(entry.at, EntryAt::Stored(_)))
        //     .count();
        let count = stored_count(bound);
        let mut payloads = Vec::<(ListParWithRandom, usize)>::with_capacity(count);
        for (position, entry) in bound.iter().enumerate() {
            let EntryAt::Stored(index) = entry.at else {
                continue;
            };
            let coll = *colls
                .get(position)
                .ok_or(SpaceError::Inconsistent("a position has no collection"))?;
            let payload = self.source.stored_payload(
                // cx,
                view,
                self.scope,
                coll,
                index,
                "a bound entry is outside the staged view",
            )?;
            payloads.push((payload, position));
        }
        let mut stored = Vec::<StoredMatch<'_>>::with_capacity(payloads.len());
        for (payload, position) in &payloads {
            let entry = bound
                .get(*position)
                .ok_or(SpaceError::Inconsistent("a position has no bound entry"))?;
            stored.push(StoredMatch {
                payload,
                captures: &entry.matched,
            });
        }
        // cx.theory_admits(&stored)
        view.theory_admits(&stored)
    }

    /// Added by decision 124 (J1 review D4, L4): the produce that a fire of the element `x` at a
    /// position on `coll` stages, for the pick of a search of SEARCH-PRODUCE. A source whose
    /// windows keep DR-75 ([`JoinSource::WINDOWS_READ_ORDER`] false) gets `None` and builds no
    /// produce.
    // Changed by decision 124 (plan v4 C7.9, ruling OP-J): the reads of this fn go through the
    // view `view` in place of `cx`. The comment above a changed line keeps its text before.
    fn produce_pick<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        x: &Incoming,
        coll: usize,
    ) -> Result<Option<Produce>, SpaceError> {
        match Src::WINDOWS_READ_ORDER {
            true => self
                .source
                .incoming_source(view, self.scope, x, coll)
                .map(Some),
            false => Ok(None),
        }
    }

    /// SEARCH-PRODUCE (joins plan section 3.5): the fire of the stored waiter `waiter` with the
    /// element in flight `x` at one position, and the position. The waiter must be the head of
    /// each head-only collection of its group, and `x` must lie in the window of its collection.
    /// Changed by decision 103 (decision 118, rules CR-9 and CR-10): the retry pins `x` at each of
    /// its positions in group order. A persistent `x` is never claimed, so the search can also
    /// bind it at the later positions of the collection of the pinned position, first in each E1
    /// list. A non-persistent `x` is claimed.
    // Changed by decision 124 (plan v4 C7.9, ruling OP-J): the reads of this fn go through the
    // view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // Each bind of a datum runs the item test of the view (rule R5): a refused datum is a miss.
    // fn search_produce(
    fn search_produce<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        waiter: &Waiter,
        x: &Incoming,
        x_vector: Option<&ExactVector>,
    ) -> Result<Option<(usize, Vec<Bound>)>, SpaceError> {
        // if !self.all_at_head(cx, waiter)? {
        if !self.all_at_head(view, waiter)? {
            return Ok(None);
        }
        let lanes = self.lanes(&waiter.slots);
        // Whether `x` lies in the window of each collection that it enters, computed at the first
        // position on the collection. A keyed element enters its own collection only, so it
        // asks for one window, as the search of P5c always did. The list of the other
        // collections grows when a second collection is entered.
        let mut first: Option<(usize, bool)> = None;
        let mut others: Vec<(usize, bool)> = Vec::new();
        for position in 0..waiter.colls.len() {
            let coll = *waiter
                .colls
                .get(position)
                .ok_or(SpaceError::Inconsistent("a position has no collection"))?;
            if !self.source.enters(self.scope, x, coll)? {
                continue;
            }
            // P6: `x` enters a VectorDB position when the query of the position makes it
            // eligible, and the position binds the value that its query delivers.
            // Changed by decision 128 (kind routing): the field takes the name of the window. The
            // text before:
            // if let Some(cfg) = &self.vdb {
            if let Some(cfg) = &self.similarity {
                let pattern = waiter
                    .waiting
                    .patterns
                    .get(position)
                    .ok_or(SpaceError::Inconsistent("a position has no pattern"))?;
                let slot = slot_at(&waiter.slots, position)?;
                let Some(matched) =
                    // self.similarity_flight_bind(cx, cfg, &slot.policy, pattern, x, x_vector)?
                    self.similarity_flight_bind(view, cfg, &slot.policy, pattern, x, x_vector)?
                else {
                    continue;
                };
                // Added by decision 102 (plan v4 C5.3, rules CR-1 and CR-4): the fire stages this
                // produce, and its COMM lists every bound record, so the search picks with it.
                // Added by decision 124 (J1 review D4, L4): a source whose windows keep DR-75
                // builds no produce, and the search keeps DR-75.
                let source = self.produce_pick(view, x, coll)?;
                if let Some(bound) = self.search(
                    // cx,
                    view,
                    &waiter.colls,
                    &waiter.waiting.patterns,
                    // Changed by decision 102 (plan v4 C5.3): the lanes come from the slots.
                    // &lanes,
                    &waiter.slots,
                    // Changed by decision 103 (decision 118, rule CR-9): the tuple carries the
                    // vector of x.
                    // Some((position, x, matched)),
                    Some((position, x, matched, x_vector)),
                    WindowOrder::of_produce(source.as_ref()),
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
                    // Changed by decision 102 (plan v4 C5.3): a membership test keeps DR-75. It
                    // does not pick, set the pick flag or look up a row.
                    // let window =
                    //     self.source
                    //         .window(cx, self.scope, coll, need, Some(x), &self.work)?;
                    let window = self.source.window(
                        // cx,
                        view,
                        self.scope,
                        coll,
                        need,
                        Some(x),
                        &self.work,
                        WindowOrder::Canonical,
                    )?;
                    let in_window = window.iter().any(|entry| entry.at == EntryAt::Incoming);
                    match first {
                        None => first = Some((coll, in_window)),
                        Some(_) => others.push((coll, in_window)),
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
            let payload = self.source.incoming_payload(self.scope, x, coll)?;
            // let Some(matched) = cx.bind(pattern, &payload) else {
            let Some(matched) = view.bind_item(pattern, &payload, x.persist)? else {
                continue;
            };
            // Added by decision 102 (plan v4 C5.3, rules CR-1 and CR-4): the fire stages this
            // produce, and its COMM lists every bound record, so the search picks with it.
            // Added by decision 124 (J1 review D4, L4): a source whose windows keep DR-75 builds
            // no produce, and the search keeps DR-75.
            let source = self.produce_pick(view, x, coll)?;
            if let Some(bound) = self.search(
                // cx,
                view,
                &waiter.colls,
                &waiter.waiting.patterns,
                // Changed by decision 102 (plan v4 C5.3): the lanes come from the slots.
                // &lanes,
                &waiter.slots,
                // Changed by decision 103 (decision 118, rule CR-9): the tuple carries the vector
                // of x.
                // Some((position, x, matched)),
                Some((position, x, matched, x_vector)),
                WindowOrder::of_produce(source.as_ref()),
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
    /// Added by decision 102 (plan v4 C5.3): `order` orders the records of a bind without a
    /// query and the ties of a score. The bind reads the config of the space itself.
    // Changed by decision 124 (plan v4 C7.9, ruling OP-J): the reads of this fn go through the
    // view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // Each bind of a datum runs the item test of the view (rule R5): a refused datum is a miss.
    // fn similarity_bind(
    fn similarity_bind<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        // Changed by decision 102 (plan v4 C5.3): the bind reads the config, so the order fits
        // seven inputs.
        // cfg: &VdbConfig,
        coll: usize,
        policy: &WaiterPolicy,
        pattern: &BindPattern,
        claimed: &[Claim],
        // Added by decision 102 (plan v4 C5.3): the order of the records and of the ties.
        order: WindowOrder<'_>,
    ) -> Result<Option<Bound>, SpaceError> {
        // Added by decision 102 (plan v4 C5.3): the config of the space. `SimilarityConfig` is
        // `Copy`.
        let cfg = self
            .similarity
            .ok_or(SpaceError::Inconsistent(BIND_OUTSIDE_WINDOW))?;
        let channel = self.channel(coll)?;
        // Changed by decision 102 (plan v4 C5.3): the config is a local.
        // let query = vectordb::policy_query(cfg, policy)?;
        let query = Src::Discipline::policy_query(&cfg, policy)?;
        let entries: Vec<(usize, Option<Score>)> = match &query {
            None => {
                // Changed by decision 102 (plan v4 C5.3, rules CR-1 and CR-3): a collection that
                // the group names once picks with the source of the operation.
                // let order = cx.canonical_data_order(channel)?;
                let order = match order {
                    WindowOrder::Pick(source) => view.pick_data_order(channel, source)?,
                    WindowOrder::Canonical => view.canonical_data_order(channel)?,
                };
                let mut entries = Vec::<(usize, Option<Score>)>::with_capacity(order.len());
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
                    // Changed by decision 102 (plan v4 C5.2): the order is an argument.
                    // let eligible = vectordb::eligible(cx, cfg, query, channel, None)?;
                    let eligible = Src::Discipline::eligible(
                        // cx,
                        view,
                        // Changed by decision 102 (plan v4 C5.3): the config is a local.
                        // cfg,
                        &cfg,
                        query,
                        channel,
                        None,
                        // Changed by decision 102 (plan v4 C5.3, rules CR-1 and CR-3): random
                        // ties for a collection named once, and score then DR-75 for a repeated
                        // one.
                        // vectordb::EligibleOrder::Canonical,
                        order.eligible(),
                    )?;
                    let mut entries = Vec::<(usize, Option<Score>)>::with_capacity(eligible.len());
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
                // cx,
                view,
                self.scope,
                coll,
                index,
                "a window index is outside the staged view",
            )?;
            if is_claimed(claimed, &stored.claim)? {
                continue;
            }
            let data = match &query {
                // Changed by decision 128 (kind routing): the discipline of the source gives the
                // step. The text before:
                // Some(query) => Cow::Owned(vectordb::single_data(
                Some(query) => Cow::Owned(Src::Discipline::single_data(
                    query,
                    &stored.payload,
                    score.as_ref(),
                )?),
                None => Cow::Borrowed(&stored.payload),
            };
            // if let Some(matched) = cx.bind(pattern, &data) {
            if let Some(matched) = view.bind_item(pattern, &data, stored.persist)? {
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
    // Changed by decision 124 (plan v4 C7.9, ruling OP-J): the reads of this fn go through the
    // view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // Each bind of a datum runs the item test of the view (rule R5): a refused datum is a miss.
    // fn similarity_flight_bind(
    fn similarity_flight_bind<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        // Changed by decision 128 (kind routing): the window owns the type. The text before:
        // cfg: &VdbConfig,
        cfg: &SimilarityConfig,
        policy: &WaiterPolicy,
        pattern: &BindPattern,
        x: &Incoming,
        x_vector: Option<&ExactVector>,
    ) -> Result<Option<ListParWithRandom>, SpaceError> {
        // Changed by decision 128 (kind routing): the discipline of the source gives the step. The
        // text before:
        // let Some(query) = vectordb::policy_query(cfg, policy)? else {
        let Some(query) = Src::Discipline::policy_query(cfg, policy)? else {
            // return Ok(cx.bind(pattern, &x.payload));
            return view.bind_item(pattern, &x.payload, x.persist);
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
        // Changed by decision 128 (kind routing): the discipline of the source gives the step. The
        // text before:
        // match vectordb::flight_eligibility(&query, &flight)? {
        match Src::Discipline::flight_eligibility(&query, &flight)? {
            None => Ok(None),
            Some(score) => {
                // Changed by decision 128 (kind routing): the discipline of the source gives the
                // step. The text before:
                // let data = vectordb::single_data(&query, &x.payload, score.as_ref())?;
                let data = Src::Discipline::single_data(&query, &x.payload, score.as_ref())?;
                // Ok(cx.bind(pattern, &data))
                view.bind_item(pattern, &data, x.persist)
            }
        }
    }

    /// The query and the length of the list of the stored waiter `waiter` when it is a list
    /// waiter of a VectorDB collection: one bind whose policy holds a list-mode query (decision
    /// 66, a list waiter always parks on `[D]`). Every other waiter is `None`.
    fn list_waiter(
        &self,
        slots: &[JoinSlot],
        // Changed by decision 128 (kind routing): the window owns the type, and the field takes its
        // name. The text before:
        // ```
        // ) -> Result<Option<(vectordb::VdbQuery, Option<usize>)>, SpaceError> {
        //     let (Some(cfg), [slot]) = (&self.vdb, slots) else {
        // ```
    ) -> Result<Option<(SimilarityQuery, Option<usize>)>, SpaceError> {
        let (Some(cfg), [slot]) = (&self.similarity, slots) else {
            return Ok(None);
        };
        // Changed by decision 128 (kind routing): the discipline of the source gives the step. The
        // text before:
        // match vectordb::policy_query(cfg, &slot.policy)? {
        match Src::Discipline::policy_query(cfg, &slot.policy)? {
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
    // Changed by decision 124 (plan v4 C7.9, ruling OP-J): the reads of this fn go through the
    // view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn candidates(
    fn candidates<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        colls: &[usize],
        bound: Vec<Bound>,
        x: Option<&Incoming>,
        shift: Option<(usize, usize)>,
    ) -> Result<Vec<ParCandidate>, SpaceError> {
        let mut candidates = Vec::<ParCandidate>::with_capacity(bound.len());
        for (entry, &coll) in bound.into_iter().zip(colls) {
            let channel = self.channel(coll)?.clone();
            match entry.at {
                EntryAt::Incoming => {
                    let x = x.ok_or(SpaceError::Inconsistent(
                        "a fire binds an element in flight, but none is in flight",
                    ))?;
                    // let source = self.source.incoming_source(cx, self.scope, x, coll)?;
                    let source = self.source.incoming_source(view, self.scope, x, coll)?;
                    // let record = self.source.incoming_record(cx, self.scope, x, coll)?;
                    let record = self.source.incoming_record(view, self.scope, x, coll)?;
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
                        // let records = cx.data_on(self.channel(coll)?)?;
                        let records = view.data_on(self.channel(coll)?)?;
                        let datum = records.get(index).ok_or(SpaceError::Inconsistent(
                            "a bound index is outside the staged view",
                        ))?;
                        datum.clone()
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
    /// continues with it). Added by decision 103 (decision 118, rule CR-9, C6.5): a persistent `x`
    /// at several positions is sent again once, at `incoming_at`.
    // Changed by decision 124 (plan v4 C7.11, ruling RADD 1.2): the reads of this fn go through
    // the view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn second_sends(
    fn second_sends<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        colls: &[usize],
        bound: &[Bound],
        x: Option<&Incoming>,
        incoming_at: Option<usize>,
    ) -> Result<Vec<Work>, SpaceError> {
        let mut sends = Vec::<Work>::with_capacity(bound.len());
        for (position, (entry, &coll)) in bound.iter().zip(colls).enumerate().rev() {
            let at = match incoming_at == Some(position) {
                true => EntryAt::Incoming,
                false => entry.at,
            };
            match at {
                // Added by decision 103 (decision 118, rule CR-9, C6.5): a persistent x that binds
                // another position of its collection too is sent again once, at `incoming_at`.
                EntryAt::Incoming if incoming_at.is_some_and(|pinned| pinned != position) => {}
                EntryAt::Incoming => {
                    let x = x.ok_or(SpaceError::Inconsistent(
                        "a fire binds an element in flight, but none is in flight",
                    ))?;
                    if x.in_flight {
                        sends.push(Work {
                            coll: x.coll,
                            record: x.record.clone(),
                            persist: x.persist,
                            own: x.own,
                            restore: x.restore,
                        });
                    }
                }
                EntryAt::Stored(index) => {
                    if let Some(send) = self
                        .source
                        // .second_send(cx, self.scope, coll, index, entry)?
                        .second_send(view, self.scope, coll, index, entry)?
                    {
                        sends.push(send);
                    }
                }
            }
        }
        Ok(sends)
    }

    /// Pushes the second sends `sends` onto the work stack.
    fn push_work(&mut self, sends: Vec<Work>) {
        for send in sends {
            self.work.push(send);
        }
    }

    /// The element of the work item `work` in flight: its slots, its payload, its identity, and
    /// its introduction. The element of the operation keeps the introduction and the placement of
    /// the operation. A second send registers the record's own authority
    /// (`Introduction::for_record`) and takes its place by its carried order.
    fn incoming_of(&self, cx: &mut Cx<'_, '_>, work: Work) -> Result<Incoming, SpaceError> {
        let (ord, level) = self.source.record_slots(&work.record)?;
        let payload = self.source.payload_of(&work.record)?;
        let source = Produce::create(
            self.source.home(self.scope, work.coll)?,
            &work.record,
            work.persist,
        );
        let (introduction, placement) = match work.own {
            true => (Arc::clone(cx.introduction()), self.own_placement),
            false => (
                cx.introduction().for_record(&work.record)?,
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
    /// after the capacity check (F15) and its touch. A store with the row search `search` is the
    /// E3-a store of replay. It is the host outcome of a deviation of play, so it makes no
    /// capacity check (CR-22), and its first row carries the search (V4-17).
    // Changed by decision 124 (rules CR-7 and CR-22, errata V4-17): the search `search`. The text
    // before:
    // fn store(&mut self, cx: &mut Cx<'_, '_>, x: Incoming) -> Result<(), SpaceError> {
    fn store(
        &mut self,
        cx: &mut Cx<'_, '_>,
        x: Incoming,
        search: Option<ParRowSearch>,
    ) -> Result<(), SpaceError> {
        // Replaced by P6 (design v3 4.0): the bound of the data side under the config.
        // if let Some(capacity) = self.source.capacity() {
        if let Some(bound) = self.source.bound(cx.config(), Side::Data)? {
            let held = self.source.held(cx, self.scope, &x)?;
            // Changed by decision 124 (rules CR-7 and CR-22): a store past a vetoed waiter of
            // replay goes above the bound as the host outcome. The text before:
            // if held >= bound.limit() {
            //     return Err(bound.full());
            // }
            match (held >= bound.limit(), search.is_some()) {
                (true, false) => return Err(bound.full()),
                (true, true) | (false, _) => {}
            }
        }
        self.touch(cx, x.coll)?;
        let own = x.own;
        if x.restore.is_none() {
            self.changed = true;
        }
        // Changed by decision 124 (errata V4-17): the search of an E3-a store goes to the store of
        // the source. The text before:
        // self.source.land(cx, self.scope, x, &mut self.dirty)?;
        self.source
            .land(cx, self.scope, x, &mut self.dirty, search)?;
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
        // P6: the vector of a VectorDB element, read once for every waiter that it meets.
        // Changed by decision 128 (kind routing): the discipline of the source gives the step,
        // and the field takes the name of the window. The text before:
        // ```
        // let x_vector = match &self.vdb {
        //     Some(cfg) => vectordb::record_vector(&x.record, cfg)?,
        // ```
        let x_vector = match &self.similarity {
            Some(cfg) => Src::Discipline::record_vector(&x.record, cfg)?,
            None => None,
        };
        loop {
            let order = self.source.offer_order(cx, self.scope, &x)?;
            // Added by decision 124 (plan v4 C7.9, rulings RADD 1.5 and 1.7): the row order of the
            // staging (CR-1), which the row search lists at the commit, copied only for an
            // owner-funded run (RC2 D-10), and the list of a list fire.
            let row_order = self.commit_inputs.as_ref().map(|_| order.clone());
            let mut list: Option<StagedList> = None;
            let mut fire: Option<(Waiter, usize, Vec<Bound>, Option<Vec<ParDatum>>)> = None;
            // Added by decision 124 (F2, base one row source for each produce,
            // replay_rspace.rs:1058-1082): the Stored answer of the offer of `x`
            // (`JoinSource::offer_stored`). It is read once in each round, at the first waiter
            // whose commit may veto. The waiter loop makes no stage call, so the cursor stays put
            // until the round ends. F2 assumes that no veto depends on a recorded row
            // (`Cx::may_veto` is pure). A rule that changes this needs its own design.
            let mut stored: Option<bool> = None;
            for at in order {
                let waiter = self.load(cx, at)?;
                // Added by decision 124 (L3 and F2, base E3-a, selection.rs:747-751, and one row
                // source for each produce, replay_rspace.rs:1058-1082): on a recorded Stored offer,
                // replay skips a waiter whose commit may veto, before its search. The pure test
                // runs first, so the row lookups run only for a vetoing waiter. Play reads no row,
                // so it never skips.
                let skip = match (cx.may_veto(&waiter.waiting.continuation), stored) {
                    (false, _) => false,
                    (true, Some(answer)) => answer,
                    (true, None) => {
                        let answer = self.source.offer_stored(cx, self.scope, &x)?;
                        stored = Some(answer);
                        answer
                    }
                };
                // Added by decision 124 (plan v4 C7.9, rulings RADD 1.7 and OP-F13): each waiter
                // starts with no list. The book reader of ADD A1.13 (b) can refuse a list waiter
                // after its guard admitted it and set `list`. The round then goes on to the next
                // waiter (`refusal_blocks`), and the fire of that waiter must not carry the list.
                list = None;
                let found = match self.list_waiter(&waiter.slots)? {
                    // Added by decision 124 (L3 and F2, base E3-a, selection.rs:747-751, and one
                    // row source for each produce, replay_rspace.rs:1058-1082): the skip arm comes
                    // first, so a vetoing list waiter is skipped too. The skip is a refusal
                    // (`refusal_blocks`).
                    _ if skip => None,
                    // Changed by decision 124 (plan v4 C7.9, ruling RADD 1.7): `offer_list` binds
                    // only. The guard runs here, before the claim of the entry, as it ran inside
                    // `offer_list`, and the list stays for the row search. The text before:
                    // Some((query, k)) => {
                    //     self.offer_list(cx, &waiter, &x, x_vector.as_ref(), &query, k)?
                    // }
                    Some((query, k)) => {
                        match self.offer_list(cx, &waiter, &x, x_vector.as_ref(), &query, k)? {
                            None => None,
                            // The theory of the space checks the stored members of the list, and
                            // not the element in flight (P7, TH-6).
                            Some(fire) => match cx.admits_records(
                                &waiter.waiting.continuation,
                                &fire.matched,
                                &fire.members,
                                |member| member.as_ref().map(|(record, _)| &record.a),
                                Src::Discipline::similarity_payload,
                            )? {
                                false => None,
                                true => {
                                    list = self.staged_list(
                                        &fire.matched,
                                        &fire.members,
                                        x.coll,
                                        !waiter.waiting.peeks.is_empty(),
                                    )?;
                                    let (matched, others) = fire.into_stored();
                                    Some((0, self.list_bound(&x, matched)?, Some(others)))
                                }
                            },
                        }
                    }
                    None => match self.search_produce(cx, &waiter, &x, x_vector.as_ref())? {
                        Some((position, bound)) => {
                            match self.admits(
                                cx,
                                &waiter.waiting.continuation,
                                &waiter.slots,
                                &waiter.colls,
                                &bound,
                            )? {
                                true => Some((position, bound, None)),
                                false => None,
                            }
                        }
                        None => None,
                    },
                };
                // Added by decision 124 (plan v4 C7.8, rule CR-20, ruling OP-F13): a waiter whose
                // pair with the element or with a record of its tuple the book holds does not fire
                // again. It fails as a guard refusal fails at this site (`refusal_blocks`).
                let found = match found {
                    Some((position, bound, others)) => {
                        match self.offer_booked(
                            cx,
                            &waiter,
                            &x,
                            position,
                            &bound,
                            others.as_deref(),
                        )? {
                            false => Some((position, bound, others)),
                            true => None,
                        }
                    }
                    None => None,
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
                // Added by decision 124 (errata V4-17): the store carries its row search, as an
                // E3-a park does. The row order of the round holds the waiters that replay
                // skipped.
                // Changed by decision 124 (rules CR-7 and CR-22): replay skipped the vetoing
                // waiters of a recorded Stored offer, so the store may take the side above its
                // bound, as the commit of play took it. The text before:
                // return self.store(cx, x);
                let search = match (stored, row_order) {
                    (Some(true), Some(order)) => {
                        self.store_search(cx, &x, x_vector.clone(), order)?
                    }
                    (_, _) => None,
                };
                return self.store(cx, x, search);
            };
            // Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.2): the engine state
            // before the fire, for the tail read of its row, only for an owner-funded run.
            let before = self.before(cx);
            // Added by decision 124 (L6, CR30 D-b): the fire takes one lease for each stored
            // datum that its check proved: the stored entries of `bound` and the other members
            // of a list (`offer_list`).
            let listed = match &list_others {
                Some(others) => others.len(),
                None => 0,
            };
            cx.lease_stored(stored_count(&bound) + listed);
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
                // Changed by decision 103 (decision 118, rule CR-9, C6.5): the pinned position.
                // true => self.second_sends(cx, &waiter.colls, &bound, Some(&x), None)?,
                true => self.second_sends(cx, &waiter.colls, &bound, Some(&x), Some(position))?,
                false => Vec::new(),
            };
            let fired = match peeks {
                true => None,
                // Changed by decision 101 (rule CR-13): the hook takes no members argument.
                false => Some(self.source.before_fire(
                    cx,
                    self.scope,
                    FireSite::Offer,
                    &waiter.colls,
                    &bound,
                    // &[],
                    Some(&x),
                )?),
            };
            let colls = waiter.colls.to_vec();
            let channel = self.channel(fired_coll)?;
            let step = cx.rounds().after_fire(spent, stays);
            // Added by decision 124 (plan v4 C7.9, ruling RADD 1.5, C4v2 B.4): the classes of
            // the bound entries, for the item test of c0 at the commit.
            let classes = self.stored_classes(&bound);
            let candidates = self.candidates(cx, &waiter.colls, bound, Some(&x), None)?;
            let group = waiter.at.group.clone();
            let data = self
                .source
                .incoming_record(cx, self.scope, &x, fired_coll)?;
            // Added by decision 124 (plan v4 C7.9 and C7.11, rulings RADD 1.5 and 1.2): the row
            // search of the fire, built only for an owner-funded run (RC2 D-10), with what the
            // fire decides for the steps after it.
            let side = row_order.map(|order| JoinSide::Offer {
                x: x.clone(),
                vector: x_vector.clone(),
                order,
            });
            let retires_list = match &list {
                None => false,
                Some(list) => !list.others.is_empty(),
            };
            let facts = FireFacts {
                spent,
                stays,
                peeks,
                changed: self.changed || retires_list,
            };
            let fire = StagedFire {
                consume: &waiter.waiting.source,
                channel,
                colls: &colls,
                tuple: &candidates,
                classes,
                list,
                peeks,
                site: FireSite::Offer,
                before,
                facts: Some(facts),
                sends: &sends,
            };
            let search = self.row_search(cx, side, fire, fired.as_ref())?;
            cx.fire_offer_group(
                channel,
                data,
                x.persist,
                waiter.waiting,
                group,
                candidates,
                &x.introduction,
                // Added by decision 102 (plan v4 C7.22): the element of an offer is in flight.
                x.offered(),
                // Added by decision 124 (plan v4 C7.9, ruling RADD 1.6): the row search of the
                // fire, which the source of the kind built (`JoinSource::row_search`).
                search,
            )?;
            self.mark_dirty(&colls);
            if peeks {
                self.push_work(sends);
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
                // Changed by decision 101 (rule CR-13): the random state and the authority of
                // the element are arguments.
                // if vectordb::retire_members(cx, channel, others, &x.record)? {
                let element_random = Blake2b512Random::from_bytes(&x.record.random_state);
                let authority = x.record.cost_authority.as_ref();
                if Src::Discipline::retire_list_members(
                    cx,
                    channel,
                    others,
                    &element_random,
                    authority,
                )? {
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
                            self.work.push(send);
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
    /// members of the list, which a fire without a peek retires. Changed by decision 124 (plan v4
    /// C7.9, ruling RADD 1.7): the bind only, over any view: the values that the pattern binds
    /// and the members of the list in list order with the place of `x`. The list arm of `offer`
    /// runs the guard and the claim (`list_bound`), and the commit reads the same bind.
    // Changed by decision 124 (plan v4 C7.9, rulings OP-J and RADD 1.7): the reads of this fn go
    // through the view `view` in place of `cx`. The comment above a changed line keeps its text
    // before.
    // #[allow(clippy::type_complexity)]
    // fn offer_list(
    fn offer_list<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        waiter: &Waiter,
        x: &Incoming,
        x_vector: Option<&ExactVector>,
        // Changed by decision 128 (kind routing): the window owns the type. The text before:
        // query: &vectordb::VdbQuery,
        query: &SimilarityQuery,
        k: Option<usize>,
        // ) -> Result<Option<(usize, Vec<Bound>, Option<Vec<ParDatum>>)>, SpaceError> {
    ) -> Result<Option<ListFire>, SpaceError> {
        // Changed by decision 128 (kind routing): the field takes the name of the window, and the
        // window owns the text of the error. The text before:
        // ```
        // let cfg = self.vdb.as_ref().ok_or(SpaceError::Inconsistent(
        //     "a list waiter outside a VectorDB space",
        // ))?;
        // ```
        let cfg = self
            .similarity
            .as_ref()
            .ok_or(SpaceError::Inconsistent(LIST_OUTSIDE_WINDOW))?;
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
        // let Some((matched, others)) =
        //     vectordb::list_offer(cx, cfg, query, k, pattern, channel, &flight)?
        // else {
        //     return Ok(None);
        // };
        // // The theory of the space checks the stored members of the list, and not the element in
        // // flight (P7, TH-6).
        // if !cx.admits_records(
        //     &waiter.waiting.continuation,
        //     &matched,
        //     &others,
        //     |record| Some(&record.a),
        //     vectordb::payload_of,
        // )? {
        //     return Ok(None);
        // }
        // let claim = self.source.claim(x.coll, EntryAt::Incoming, &x.payload)?;
        // let mut bound = Vec::<Bound>::with_capacity(1);
        // bound.push(Bound {
        //     at: EntryAt::Incoming,
        //     matched,
        //     persist: x.persist,
        //     claim,
        // });
        // Ok(Some((0, bound, Some(others))))
        Src::Discipline::list_offer(view, cfg, query, k, pattern, channel, &flight)
    }

    /// Added by decision 124 (plan v4 C7.9, ruling RADD 1.7): the bound entry of the element `x`
    /// of a list fire, with the values `matched` of its list: the claim of the entry, then the
    /// entry (the element in flight). The staging and the commit build it alike.
    fn list_bound(
        &self,
        x: &Incoming,
        matched: ListParWithRandom,
    ) -> Result<Vec<Bound>, SpaceError> {
        let claim = self.source.claim(x.coll, EntryAt::Incoming, &x.payload)?;
        let mut bound = Vec::<Bound>::with_capacity(1);
        bound.push(Bound {
            at: EntryAt::Incoming,
            matched,
            persist: x.persist,
            claim,
        });
        Ok(bound)
    }

    // Retired by decision 102 (decision 119 G1, rule CR-12) and decision 101 (rule CR-13): a
    // single-bind fire on a set binds the first member of the class over `[D]`, and `K_retire`
    // rows retire the other members (`class_first`).
    // /// The candidates of a single-bind consume fire on a set: the representative `bound` and the
    // /// other stored members of its class in canonical order (F3, F20). Each member binds the
    // /// pattern with its own values. It returns the candidates and whether a member is persistent.
    // fn class_candidates(
    //     &self,
    //     cx: &mut Cx<'_, '_>,
    //     coll: usize,
    //     pattern: &BindPattern,
    //     representative: Bound,
    // ) -> Result<(Vec<ParCandidate>, bool), SpaceError> {
    //     let EntryAt::Stored(rep_index) = representative.at else {
    //         return Err(SpaceError::Inconsistent(
    //             "a consume binds an element in flight",
    //         ));
    //     };
    //     let class = representative.claim.clone();
    //     let mut persistent = representative.persist;
    //     let mut candidates = Vec::new();
    //     let rep = self.candidates(cx, &[coll], vec![representative], None, None)?;
    //     for candidate in rep {
    //         candidates.push(candidate);
    //     }
    //     if !matches!(class, Claim::Class { .. }) {
    //         return Ok((candidates, persistent));
    //     }
    //     let order = self.source.members(cx, self.scope, coll)?;
    //     for index in order {
    //         if index == rep_index {
    //             continue;
    //         }
    //         let stored = self.source.stored_entry(
    //             cx,
    //             self.scope,
    //             coll,
    //             index,
    //             "a canonical index is outside the staged view",
    //         )?;
    //         if !is_claimed(std::slice::from_ref(&class), &stored.claim)? {
    //             continue;
    //         }
    //         let matched = cx
    //             .bind(pattern, &stored.payload)
    //             .ok_or(SpaceError::Inconsistent(
    //                 "a member of a class does not bind the pattern of its representative",
    //             ))?;
    //         persistent = persistent || stored.persist;
    //         let member = self.candidates(
    //             cx,
    //             &[coll],
    //             vec![Bound {
    //                 at: EntryAt::Stored(index),
    //                 matched,
    //                 persist: stored.persist,
    //                 claim: stored.claim,
    //             }],
    //             None,
    //             None,
    //         )?;
    //         for candidate in member {
    //             candidates.push(candidate);
    //         }
    //     }
    //     Ok((candidates, persistent))
    // }

    /// Added by decision 124 (plan v4 C7.9 and C7.11, rulings RADD 1.5 and 1.2, ADD A1.10 (c) 3
    /// and 4): the tuple of the waiter `at` of an offer of `x` at the commit, over `view`:
    /// OFFER-LIST for a list waiter (decision 66), else SEARCH-PRODUCE, then the guard once. A
    /// list with a member that fails the item test is a miss (R5, C4v2 B.4). What the fire spends
    /// and the list members that it retires are read only when the guard admits it and the waiter
    /// does not peek. The fire of the tuple takes the facts that `offer` derives.
    fn commit_offer(
        &self,
        view: &mut CommitView<'_, '_>,
        at: &WaiterRef,
        x: &Incoming,
        vector: Option<&ExactVector>,
        request: &ParRowRequest<'_>,
    ) -> Result<Option<Searched<Src::Spent>>, SpaceError> {
        let Some(at) = commit_ref(view, at)? else {
            return Ok(None);
        };
        let waiter = self.load(view, at)?;
        let peeks = !waiter.waiting.peeks.is_empty();
        let (position, bound, admitted, others) = match self.list_waiter(&waiter.slots)? {
            None => {
                let Some((position, bound)) = self.search_produce(view, &waiter, x, vector)? else {
                    return Ok(None);
                };
                let admitted = self.admits(
                    view,
                    &waiter.waiting.continuation,
                    &waiter.slots,
                    &waiter.colls,
                    &bound,
                )?;
                (position, bound, admitted, Vec::new())
            }
            Some((query, k)) => {
                let Some(fire) = self.offer_list(view, &waiter, x, vector, &query, k)? else {
                    return Ok(None);
                };
                let element = incoming_datum(x);
                let pairs =
                    Src::Discipline::list_pairs(&fire.members, Some((&element, &x.payload)))?;
                match Src::Discipline::members_admit(request, &fire.matched, &pairs)? {
                    true => {}
                    false => return Ok(None),
                }
                let admitted = view.admits_records(
                    &waiter.waiting.continuation,
                    &fire.matched,
                    &fire.members,
                    |member| member.as_ref().map(|(record, _)| &record.a),
                    Src::Discipline::similarity_payload,
                )?;
                let others = list_retires::<Src::Discipline>(&fire.members, self.channel(x.coll)?)?;
                (0, self.list_bound(x, fire.matched)?, admitted, others)
            }
        };
        let fires = admitted && !peeks;
        let spent = match fires {
            false => None,
            true => Some((
                waiter.colls.clone(),
                self.source.before_fire(
                    view,
                    self.scope,
                    FireSite::Offer,
                    &waiter.colls,
                    &bound,
                    Some(x),
                )?,
            )),
        };
        let others = match fires {
            false => Vec::new(),
            true => others,
        };
        let sends = match (admitted, peeks) {
            (true, true) => {
                self.second_sends(view, &waiter.colls, &bound, Some(x), Some(position))?
            }
            (false, _) | (true, false) => Vec::new(),
        };
        let x_spent = !x.persist && !peeks;
        let stays = waiter.waiting.persist;
        let spends_stored = bound
            .iter()
            .any(|entry| matches!(entry.at, EntryAt::Stored(_)) && !entry.persist);
        let changed = match (stays, peeks) {
            (false, _) => true,
            (true, true) => false,
            (true, false) => {
                (x_spent && x.restore.is_some()) || spends_stored || !others.is_empty()
            }
        };
        let fire = RowFire {
            colls: waiter.colls.clone(),
            facts: FireFacts {
                spent: x_spent,
                stays,
                peeks,
                changed,
            },
            sends,
        };
        let tuple = self.candidates(view, &waiter.colls, bound, Some(x), None)?;
        Ok(Some(Searched {
            tuple,
            admitted,
            spent,
            others,
            fire,
        }))
    }

    /// Added by decision 124 (plan v4 C7.9 and C7.11, rulings RADD 1.5 and 1.2, ADD A1.10 (c) 3
    /// and 4): the tuple of the receive `waiting` on the collections `colls` at the commit, over
    /// `view`, when its staged tuple fails the item test: RECEIVE-LIST for a list waiter
    /// (decision 66), else GREEDY, then the guard once. A single bind that fires takes the first
    /// member of its class (`class_first`), as the staging does. What the fire spends and the
    /// list members that it retires are read only when the guard admits it and the waiter does
    /// not peek. The fire of the tuple takes the facts that `receive` and `receive_list` derive.
    fn commit_receive(
        &self,
        view: &mut CommitView<'_, '_>,
        colls: &[usize],
        slots: &[JoinSlot],
        waiting: &ParWaiter,
        request: &ParRowRequest<'_>,
    ) -> Result<Option<Searched<Src::Spent>>, SpaceError> {
        let peek = !waiting.peeks.is_empty();
        let coll = *colls
            .first()
            .ok_or(SpaceError::Inconsistent("a consume has no collection"))?;
        let pattern = waiting
            .patterns
            .first()
            .ok_or(SpaceError::Inconsistent("a consume has no pattern"))?;
        match self.list_waiter(slots)? {
            None => {}
            Some((query, k)) => {
                let cfg = self
                    .similarity
                    .as_ref()
                    .ok_or(SpaceError::Inconsistent(LIST_OUTSIDE_WINDOW))?;
                let channel = self.channel(coll)?;
                let Some(fire) =
                    Src::Discipline::list_selection(view, cfg, &query, k, pattern, channel, None)?
                else {
                    return Ok(None);
                };
                let pairs = Src::Discipline::list_pairs(&fire.members, None)?;
                match Src::Discipline::members_admit(request, &fire.matched, &pairs)? {
                    true => {}
                    false => return Ok(None),
                }
                let admitted = view.admits_records(
                    &waiting.continuation,
                    &fire.matched,
                    &fire.members,
                    |member| member.as_ref().map(|(record, _)| &record.a),
                    Src::Discipline::similarity_payload,
                )?;
                let others = match admitted && !peek {
                    false => Vec::new(),
                    true => list_retires::<Src::Discipline>(&fire.members, channel)?,
                };
                let Some(Some((head, index))) = fire.members.first() else {
                    return Err(SpaceError::Inconsistent(
                        "a selected list has no stored first member",
                    ));
                };
                let persistent = fire
                    .members
                    .iter()
                    .flatten()
                    .any(|(record, _)| record.persist);
                let spends = fire
                    .members
                    .iter()
                    .flatten()
                    .any(|(record, _)| !record.persist);
                let coll = *colls
                    .first()
                    .ok_or(SpaceError::Inconsistent("a list waiter has no collection"))?;
                let mut sends = Vec::<Work>::with_capacity(1);
                match (peek, head.persist) {
                    (true, false) => sends.push(Work {
                        coll,
                        record: head.a.clone(),
                        persist: false,
                        own: false,
                        restore: None,
                    }),
                    (true, true) | (false, _) => {}
                }
                let facts = FireFacts {
                    spent: !waiting.persist,
                    stays: peek || persistent,
                    peeks: peek,
                    changed: !peek && spends,
                };
                let view_index = i32::try_from(*index)
                    .map_err(|_| SpaceError::Inconsistent("a record index exceeds i32"))?;
                let mut tuple = Vec::<ParCandidate>::with_capacity(1);
                tuple.push(receive_candidate(channel.clone(), ReceiveFire {
                    head: head.clone(),
                    matched: fire.matched,
                    view_index,
                }));
                return Ok(Some(Searched {
                    tuple,
                    admitted,
                    spent: None,
                    others,
                    fire: RowFire {
                        colls: colls.to_vec(),
                        facts,
                        sends,
                    },
                }));
            }
        }
        let Some(bound) = self.search(
            view,
            colls,
            &waiting.patterns,
            slots,
            None,
            WindowOrder::Pick(RSpaceOperationSource::Consume(&waiting.source)),
        )?
        else {
            return Ok(None);
        };
        let admitted = self.admits(view, &waiting.continuation, slots, colls, &bound)?;
        let fires = admitted && !peek;
        let sends = match (admitted, peek) {
            (true, true) => self.second_sends(view, colls, &bound, None, None)?,
            (false, _) | (true, false) => Vec::new(),
        };
        let (bound, site, stays, changed) = match (colls.len(), fires) {
            (1, true) => {
                let representative = bound
                    .into_iter()
                    .next()
                    .ok_or(SpaceError::Inconsistent("a consume binds no entry"))?;
                let (first, persistent, spends) =
                    self.class_first(view, coll, pattern, representative)?;
                let mut bound = Vec::<Bound>::with_capacity(1);
                bound.push(first);
                (bound, FireSite::Single, persistent, spends)
            }
            (1, false) => {
                let persist = bound.first().is_some_and(|entry| entry.persist);
                (bound, FireSite::Single, peek || persist, !peek && !persist)
            }
            _ => {
                let stays = peek || bound.iter().all(|entry| entry.persist);
                (bound, FireSite::Join, stays, !stays)
            }
        };
        let spent = match fires {
            false => None,
            true => Some((
                colls.to_vec(),
                self.source
                    .before_fire(view, self.scope, site, colls, &bound, None)?,
            )),
        };
        let fire = RowFire {
            colls: colls.to_vec(),
            facts: FireFacts {
                spent: !waiting.persist,
                stays,
                peeks: peek,
                changed,
            },
            sends,
        };
        let tuple = self.candidates(view, colls, bound, None, None)?;
        Ok(Some(Searched {
            tuple,
            admitted,
            spent,
            others: Vec::new(),
            fire,
        }))
    }

    /// Added by decision 124 (plan v4 C7.9, ruling RADD 1.5, C4v2 B.4): true when every member of
    /// the class `class` on the collection `coll` passes the item test of the view under
    /// `pattern`. `tested` keeps the answer of each class that the search tested.
    fn class_items<V: StoreView>(
        &self,
        view: &mut V,
        coll: usize,
        pattern: &BindPattern,
        class: &Claim,
        tested: &mut Vec<(Claim, bool)>,
    ) -> Result<bool, SpaceError> {
        for (known, admitted) in tested.iter() {
            match is_claimed(std::slice::from_ref(known), class)? {
                false => {}
                true => return Ok(*admitted),
            }
        }
        let order = self.source.members(view, self.scope, coll)?;
        let mut admitted = true;
        for index in order {
            let stored = self.source.stored_entry(
                view,
                self.scope,
                coll,
                index,
                "a canonical index is outside the view",
            )?;
            match is_claimed(std::slice::from_ref(class), &stored.claim)? {
                true => {}
                false => continue,
            }
            match view.bind_item(pattern, &stored.payload, stored.persist)? {
                Some(_) => {}
                None => {
                    admitted = false;
                    break;
                }
            }
        }
        tested.push((class.clone(), admitted));
        Ok(admitted)
    }

    /// Added by decision 124 (plan v4 C7.9, ruling RADD 1.5, ADD A1.10 (c) 8, CR-20 item 2): the
    /// allowed pairs of the waiter `waiting` on the collections `colls` that fail `rule`. Each
    /// collection once: the entries of its window over `view` in DR-75 order, with the element
    /// `x` at its place. The first position on that collection whose pattern binds a record
    /// gives its value. No guard or theory runs.
    fn waiter_failures(
        &self,
        view: &mut CommitView<'_, '_>,
        colls: &[usize],
        waiting: &ParWaiter,
        x: Option<&Incoming>,
        rule: &dyn RowItemRule<TaggedContinuation, ListParWithRandom>,
        failed: &mut Vec<(Blake2b256Hash, Blake2b256Hash)>,
    ) -> Result<(), SpaceError> {
        let mut visited = Vec::<usize>::with_capacity(colls.len());
        for &coll in colls {
            match visited.contains(&coll) {
                false => {}
                true => continue,
            }
            visited.push(coll);
            let need = colls.iter().filter(|&&other| other == coll).count();
            let channel = self.channel(coll)?;
            let entries = self.source.window(
                view,
                self.scope,
                coll,
                need,
                x,
                &self.work,
                WindowOrder::Canonical,
            )?;
            for entry in entries {
                let (record, payload, persist) = match entry.at {
                    EntryAt::Incoming => {
                        let Some(x) = x else {
                            continue;
                        };
                        let source = self.source.incoming_source(view, self.scope, x, coll)?;
                        let payload = self.source.incoming_payload(self.scope, x, coll)?;
                        (source.hash, payload.into_owned(), x.persist)
                    }
                    EntryAt::Stored(index) => {
                        let stored = self.source.stored_entry(
                            view,
                            self.scope,
                            coll,
                            index,
                            "a window index is outside the view",
                        )?;
                        let records = view.data_on(channel)?;
                        let record = records.get(index).ok_or(SpaceError::Inconsistent(
                            "a window index is outside the view",
                        ))?;
                        (record.source.hash.clone(), stored.payload, stored.persist)
                    }
                };
                let mut value = None;
                for (position, &on) in colls.iter().enumerate() {
                    match on == coll {
                        true => {}
                        false => continue,
                    }
                    let pattern = waiting
                        .patterns
                        .get(position)
                        .ok_or(SpaceError::Inconsistent("a position has no pattern"))?;
                    match view.bind(pattern, &payload) {
                        None => {}
                        Some(matched) => {
                            value = Some(matched);
                            break;
                        }
                    }
                }
                let Some(value) = value else {
                    continue;
                };
                let width = colls.len();
                match rule.admit_item(
                    &waiting.continuation,
                    waiting.persist,
                    width,
                    &value,
                    persist,
                )? {
                    true => {}
                    false => failed.push((record, waiting.source.hash.clone())),
                }
            }
        }
        Ok(())
    }

    /// Added by decision 124 (plan v4 C7.9, rules CR-20 item 2 and R5, ruling RADD 1.5): the
    /// allowed pairs of the list waiter `waiting` with the query and the length `list` that
    /// fail `rule`. The one candidate of a list waiter is its list over `view`. For an element
    /// in flight it is the list of [`Self::offer_list`], with the element `x` and its vector at
    /// its place. Otherwise it is the list of the store: for a receive, and for a stored
    /// persistent record that a cascade offers again. An offer to a waiter of another collection
    /// has no list.
    #[allow(clippy::too_many_arguments)]
    fn list_failures(
        &self,
        view: &mut CommitView<'_, '_>,
        colls: &[usize],
        waiting: &ParWaiter,
        x: Option<(&Incoming, Option<&ExactVector>)>,
        list: (SimilarityQuery, Option<usize>),
        rule: &dyn RowItemRule<TaggedContinuation, ListParWithRandom>,
        failed: &mut Vec<(Blake2b256Hash, Blake2b256Hash)>,
    ) -> Result<(), SpaceError> {
        let (query, k) = list;
        let cfg = self
            .similarity
            .as_ref()
            .ok_or(SpaceError::Inconsistent(LIST_OUTSIDE_WINDOW))?;
        let coll = *colls
            .first()
            .ok_or(SpaceError::Inconsistent("a list waiter has no collection"))?;
        let pattern = waiting
            .patterns
            .first()
            .ok_or(SpaceError::Inconsistent("a list waiter has no pattern"))?;
        let channel = self.channel(coll)?;
        let fire = match x {
            Some((x, vector)) => match (coll == x.coll, x.in_flight) {
                (false, _) => None,
                // A stored persistent record that a cascade offers again stays in the store,
                // so its list is the list of the store, as `list_cascade_fire` reads it.
                (true, false) => {
                    Src::Discipline::list_selection(view, cfg, &query, k, pattern, channel, None)?
                }
                (true, true) => {
                    let flight = Flight {
                        record: &x.record,
                        payload: &x.payload,
                        source: &x.source,
                        persist: x.persist,
                        vector,
                    };
                    Src::Discipline::list_offer(view, cfg, &query, k, pattern, channel, &flight)?
                }
            },
            None => Src::Discipline::list_selection(view, cfg, &query, k, pattern, channel, None)?,
        };
        let Some(fire) = fire else {
            return Ok(());
        };
        let element = x.map(|(x, _)| (incoming_datum(x), &x.payload));
        let pairs = Src::Discipline::list_pairs(
            &fire.members,
            element.as_ref().map(|(datum, payload)| (datum, *payload)),
        )?;
        Src::Discipline::member_failures(
            rule,
            (&waiting.source, &waiting.continuation, waiting.persist),
            &fire.matched,
            &pairs,
            failed,
        )?;
        Ok(())
    }

    /// Added by decision 102 (decision 119 G1, rule CR-12) and decision 101 (rule CR-13): the
    /// one bind of a single-bind consume fire on a set. It is the first member of the class of
    /// `representative` in the canonical order of DR-75, the representative included, with the
    /// values that `pattern` binds in its payload. A record without a class is its own first
    /// member. The members of a class have equal payloads (F17), so the guard that admitted the
    /// representative admits the first member. It returns that entry, whether a member of the
    /// class is persistent, and whether a member is not persistent. Without a peek, the fire
    /// then retires the other members (`before_fire`, RETIRE-SET-BINDS).
    // Changed by decision 124 (plan v4 C7.9, rulings OP-J and RADD 1.5): the reads of this fn go
    // through the view `view` in place of `cx`.
    fn class_first<V: StoreView>(
        &self,
        view: &mut V,
        coll: usize,
        pattern: &BindPattern,
        representative: Bound,
    ) -> Result<(Bound, bool, bool), SpaceError> {
        let EntryAt::Stored(rep_index) = representative.at else {
            return Err(SpaceError::Inconsistent(
                "a consume binds an element in flight",
            ));
        };
        if !matches!(representative.claim, Claim::Class { .. }) {
            let persistent = representative.persist;
            return Ok((representative, persistent, !persistent));
        }
        let class = representative.claim.clone();
        let order = self.source.members(view, self.scope, coll)?;
        let mut first: Option<Bound> = None;
        let mut persistent = false;
        let mut spends = false;
        for index in order {
            let stored = self.source.stored_entry(
                view,
                self.scope,
                coll,
                index,
                "a canonical index is outside the staged view",
            )?;
            if !is_claimed(std::slice::from_ref(&class), &stored.claim)? {
                continue;
            }
            persistent = persistent || stored.persist;
            spends = spends || !stored.persist;
            if first.is_some() {
                continue;
            }
            first = Some(match index == rep_index {
                true => representative.clone(),
                false => {
                    let matched =
                        view.bind(pattern, &stored.payload)
                            .ok_or(SpaceError::Inconsistent(
                            "a member of a class does not bind the pattern of its representative",
                        ))?;
                    Bound {
                        at: EntryAt::Stored(index),
                        matched,
                        persist: stored.persist,
                        claim: stored.claim,
                    }
                }
            });
        }
        let first = first.ok_or(SpaceError::Inconsistent(
            "a representative is not a member of its own class",
        ))?;
        Ok((first, persistent, spends))
    }

    /// RECEIVE (joins plan section 4.4): the waiter `w` of the operation fires from the windows
    /// of its collections when it reaches the head of each head-only collection and the search
    /// and the guard pass. Otherwise it parks. A fire that ends the loop moves the continuation,
    /// the group, the patterns and the peek set into the staged consume, and a fire that
    /// continues the loop (a persistent waiter whose partner left) copies them.
    fn receive(&mut self, cx: &mut Cx<'_, '_>, mut w: NewWaiter) -> Result<(), SpaceError> {
        // Added by decision 102 (plan v4 C5.3 and C5.6): the consume that the waiter stages, its
        // park and its fire alike. Its recorded row orders the picks of replay (CR-4) and decides
        // E3-a (CR-7).
        let source = Consume::create(&w.group, &w.patterns, &w.continuation, w.persist);
        // P6 (decision 66): a list-mode receive binds one list from the records of its one bind.
        if let Some((query, k)) = self.list_waiter(&w.slots)? {
            // Changed by decision 102 (plan v4 C5.6): the list receive takes the source too.
            // return self.receive_list(cx, w, &query, k);
            return self.receive_list(cx, w, &query, k, &source);
        }
        // Changed by decision 102 (plan v4 C5.3): `search` takes the lanes from the slots.
        // let lanes = self.lanes(&w.slots);
        let width = w.colls.len();
        loop {
            // Added by decision 102 (plan v4 C5.6, rule CR-7, base E3-a): replay parks the waiter
            // without a search when its recorded row is Stored and its commit may veto. Play
            // reads no row, so it always searches.
            if cx.recorded_stored(RSpaceOperationSource::Consume(&source))
                && cx.may_veto(&w.continuation)
            {
                // Added by decision 124 (plan v4 C7.9, ruling RADD 1.5, ADD A1.11, V4-17): the
                // park carries the row search that answers its item failures at the commit.
                let search = self.park_search(cx, &w, &source)?;
                return self.park_with(cx, w, search);
            }
            let mut reaches = true;
            let mut checked: Vec<usize> = Vec::new();
            for &coll in &w.colls {
                if checked.contains(&coll) {
                    continue;
                }
                checked.push(coll);
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
            // Changed by decision 102 (plan v4 C5.3, rules CR-1 and CR-4): the receive picks
            // with the consume that its fire stages.
            // let Some(bound) = self.search(cx, &w.colls, &w.patterns, &lanes, &w.slots, None)?
            let Some(bound) = self.search(
                cx,
                &w.colls,
                &w.patterns,
                &w.slots,
                None,
                WindowOrder::Pick(RSpaceOperationSource::Consume(&source)),
            )?
            else {
                return self.park(cx, w);
            };
            // Added by decision 124 (plan v4 C7.8, rule CR-20, ruling OP-F13): the consume does not
            // fire again with a stored record whose pair the book holds, so it parks, as when the
            // guard refuses.
            match self.pair_booked(cx, &w.colls, &source, &bound)? {
                false => {}
                true => return self.park(cx, w),
            }
            if !self.admits(cx, &w.continuation, &w.slots, &w.colls, &bound)? {
                return self.park(cx, w);
            }
            // Added by decision 124 (plan v4 C7.9 and C7.11, rulings RADD 1.5 and 1.2): the side
            // of the row, copied before the fire takes the waiter, and the engine state before
            // the fire, only for an owner-funded run (RC2 D-10).
            let side = self.receive_side(&w, &source);
            let before = self.before(cx);
            // Added by decision 124 (L6, CR30 D-b): the fire takes one lease for each stored
            // datum that its check proved.
            cx.lease_stored(stored_count(&bound));
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
                    let pattern = (w
                        .patterns
                        .first()
                        .ok_or(SpaceError::Inconsistent("a consume has no pattern"))?)
                    .clone();
                    let representative = bound
                        .into_iter()
                        .next()
                        .ok_or(SpaceError::Inconsistent("a consume binds no entry"))?;
                    // A peek binds the representative alone (Rule E sends it again). Changed by
                    // decision 102 (decision 119 G1, rule CR-12) and decision 101 (rule CR-13):
                    // otherwise the fire binds the first member of the class of the
                    // representative in DR-75 order over `[D]`, and RETIRE-SET-BINDS retires the
                    // other members after it. The text before: "Otherwise the fire binds the class
                    // of the representative over `[D; n]` (F3, F20)."
                    // let (candidates, persistent) = match peek {
                    //     true => {
                    //         let persist = representative.persist;
                    //         (
                    //             self.candidates(cx, &w.colls, vec![representative], None, None)?,
                    //             persist,
                    //         )
                    //     }
                    //     false => self.class_candidates(cx, coll, &pattern, representative)?,
                    // };
                    let (first, persistent, spends) = match peek {
                        true => {
                            let persist = representative.persist;
                            (representative, persist, !persist)
                        }
                        false => self.class_first(cx, coll, &pattern, representative)?,
                    };
                    let step = cx.rounds().after_fire(spent, peek || persistent);
                    // Changed by decision 101 (rule CR-13): one channel and one pattern.
                    // let members = candidates.len();
                    // let mut channels = Vec::<Par>::with_capacity(members);
                    // let mut patterns = Vec::<BindPattern>::with_capacity(members);
                    // for _ in 0..members {
                    //     channels.push(self.channel(coll)?.clone());
                    //     patterns.push(pattern.clone());
                    // }
                    let mut channels = Vec::<Par>::with_capacity(1);
                    channels.push(self.channel(coll)?.clone());
                    let mut patterns = Vec::<BindPattern>::with_capacity(1);
                    patterns.push(pattern);
                    let ends = !matches!(step, FireStep::Continue);
                    let (continuation, peeks) = match ends {
                        true => (
                            std::mem::take(&mut w.continuation),
                            std::mem::take(&mut w.peeks),
                        ),
                        false => (w.continuation.clone(), w.peeks.clone()),
                    };
                    // Changed by decision 101 (rule CR-13): the fire and the retires remove every
                    // member of the class that is not persistent.
                    // if !peek && candidates.iter().any(|candidate| !candidate.datum.persist) {
                    if !peek && spends {
                        self.changed = true;
                    }
                    // Changed by decision 101 (rule CR-13): the spent entry is the first member,
                    // so a set retires its class after the fire.
                    // let fired = match peek {
                    //     true => None,
                    //     false => Some(self.source.before_fire(
                    //         cx,
                    //         self.scope,
                    //         FireSite::Single,
                    //         &w.colls,
                    //         &[],
                    //         &candidates,
                    //         None,
                    //     )?),
                    // };
                    let fired = match peek {
                        true => None,
                        false => Some(self.source.before_fire(
                            cx,
                            self.scope,
                            FireSite::Single,
                            &w.colls,
                            std::slice::from_ref(&first),
                            None,
                        )?),
                    };
                    // Added by decision 124 (plan v4 C7.9, ruling RADD 1.5, C4v2 B.4): the
                    // class of the bound entry, for the item test of c0 at the commit.
                    let classes = self.stored_classes(std::slice::from_ref(&first));
                    let candidates = self.candidates(cx, &w.colls, vec![first], None, None)?;
                    // Added by decision 124 (plan v4 C7.9 and C7.11, rulings RADD 1.5 and 1.2): the
                    // row search of the fire, built only for an owner-funded run (RC2 D-10), with
                    // what the fire decides for the steps after it.
                    let facts = FireFacts {
                        spent,
                        stays: peek || persistent,
                        peeks: peek,
                        changed: self.changed,
                    };
                    let fire = StagedFire {
                        consume: &source,
                        channel: self.channel(coll)?,
                        colls: &w.colls,
                        tuple: &candidates,
                        classes,
                        list: None,
                        peeks: peek,
                        site: FireSite::Single,
                        before,
                        facts: Some(facts),
                        sends: &sends,
                    };
                    let search = self.row_search(cx, side, fire, fired.as_ref())?;
                    cx.fire_receive_group(
                        channels,
                        patterns,
                        continuation,
                        w.persist,
                        peeks,
                        candidates,
                        1,
                        // Added by decision 124 (plan v4 C7.9, ruling RADD 1.6): the row search of
                        // the fire, which the source of the kind built (`JoinSource::row_search`).
                        search,
                    )?;
                    self.mark_dirty(&w.colls);
                    if peek {
                        self.push_work(sends);
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
                        // Changed by decision 101 (rule CR-13): the hook takes no members
                        // argument.
                        false => Some(self.source.before_fire(
                            cx,
                            self.scope,
                            FireSite::Join,
                            &w.colls,
                            &bound,
                            // &[],
                            None,
                        )?),
                    };
                    // Added by decision 124 (plan v4 C7.9, ruling RADD 1.5, C4v2 B.4): the
                    // classes of the bound entries, for the item test of c0 at the commit.
                    let classes = self.stored_classes(&bound);
                    let candidates = self.candidates(cx, &w.colls, bound, None, None)?;
                    // Added by decision 124 (plan v4 C7.9 and C7.11, rulings RADD 1.5 and 1.2): the
                    // row search of the fire, built only for an owner-funded run (RC2 D-10), with
                    // what the fire decides for the steps after it. Its channel is that of the
                    // first position.
                    let first = *w
                        .colls
                        .first()
                        .ok_or(SpaceError::Inconsistent("a consume has no collection"))?;
                    let facts = FireFacts {
                        spent,
                        stays,
                        peeks: peek,
                        changed: self.changed,
                    };
                    let fire = StagedFire {
                        consume: &source,
                        channel: self.channel(first)?,
                        colls: &w.colls,
                        tuple: &candidates,
                        classes,
                        list: None,
                        peeks: peek,
                        site: FireSite::Join,
                        before,
                        facts: Some(facts),
                        sends: &sends,
                    };
                    let search = self.row_search(cx, side, fire, retire.as_ref())?;
                    let ends = !matches!(step, FireStep::Continue);
                    let (channels, patterns, continuation, peeks) = match ends {
                        true => (
                            std::mem::take(&mut w.group),
                            std::mem::take(&mut w.patterns),
                            std::mem::take(&mut w.continuation),
                            std::mem::take(&mut w.peeks),
                        ),
                        false => (
                            w.group.clone(),
                            w.patterns.clone(),
                            w.continuation.clone(),
                            w.peeks.clone(),
                        ),
                    };
                    cx.fire_join(
                        channels,
                        patterns,
                        continuation,
                        w.persist,
                        peeks,
                        candidates,
                        // Added by decision 124 (plan v4 C7.9, ruling RADD 1.6): the row search of
                        // the fire, which the source of the kind built (`JoinSource::row_search`).
                        search,
                    )?;
                    self.mark_dirty(&w.colls);
                    match retire {
                        None => self.push_work(sends),
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
    /// `query` and the length `k`, fires when its list `L(W, R)` is not empty, its pattern binds
    /// the list and its guard admits it. Otherwise the waiter parks with its query. Changed by
    /// decision 101 (rule CR-13): the fire binds the first member over `[D]`, with the values of
    /// the list. Without a peek, RETIRE-MEMBERS then retires each other member that is not
    /// persistent with one `K_retire` COMM. A peek binds the first member and retires nothing,
    /// and the transaction sends the first member again when it is not persistent (Rule E).
    /// Added by decision 102 (plan v4 C5.6, rule CR-7): replay parks the waiter without a search
    /// when the recorded row of its consume `source` is Stored and its commit may veto. The text
    /// before: "fires over `[D; m]` when its list `L(W, R)` is not empty, its pattern binds the
    /// list and its guard admits it. The outcome dispatches the result of the first member,
    /// which carries the values of the list, and each other member carries its own payload
    /// (arbitration A11). A peek removes the non-persistent members, and the transaction sends
    /// them again in delivery order (Rule E). Otherwise the waiter parks with its query."
    fn receive_list(
        &mut self,
        cx: &mut Cx<'_, '_>,
        mut w: NewWaiter,
        // Changed by decision 128 (kind routing): the window owns the type. The text before:
        // query: &vectordb::VdbQuery,
        query: &SimilarityQuery,
        k: Option<usize>,
        // Added by decision 102 (plan v4 C5.6): the consume of the waiter decides E3-a.
        source: &Consume,
    ) -> Result<(), SpaceError> {
        // Changed by decision 128 (kind routing): the field takes the name of the window, and the
        // window owns the text of the error. The text before:
        // ```
        // let cfg = self.vdb.ok_or(SpaceError::Inconsistent(
        //     "a list waiter outside a VectorDB space",
        // ))?;
        // ```
        let cfg = self
            .similarity
            .ok_or(SpaceError::Inconsistent(LIST_OUTSIDE_WINDOW))?;
        let coll = *w
            .colls
            .first()
            .ok_or(SpaceError::Inconsistent("a consume has no collection"))?;
        let channel = self.channel(coll)?;
        loop {
            // Added by decision 102 (plan v4 C5.6, rule CR-7, base E3-a): replay parks the waiter
            // without a search when its recorded row is Stored and its commit may veto. Play
            // reads no row, so it always searches.
            if cx.recorded_stored(RSpaceOperationSource::Consume(source))
                && cx.may_veto(&w.continuation)
            {
                // Added by decision 124 (plan v4 C7.9, ruling RADD 1.5, ADD A1.11, V4-17): the
                // park carries the row search that answers its item failures at the commit.
                let search = self.park_search(cx, &w, source)?;
                return self.park_with(cx, w, search);
            }
            let pattern = w
                .patterns
                .first()
                .ok_or(SpaceError::Inconsistent("a consume has no pattern"))?;
            let Some(ListFire { matched, members }) =
                // Changed by decision 128 (kind routing): the discipline of the source gives the
                // step. The text before:
                // vectordb::list_selection(cx, &cfg, query, k, pattern, channel, None)?
                Src::Discipline::list_selection(cx, &cfg, query, k, pattern, channel, None)?
            else {
                return self.park(cx, w);
            };
            // Added by decision 124 (plan v4 C7.8, rule CR-20, rulings OP-F13 and RAMD 1.5): a
            // list with a member whose pair the book holds does not fire again, so the consume
            // parks, as when the guard refuses.
            let booked = cx.list_booked(
                None,
                members.iter().flatten().map(|(record, _)| record),
                source,
            );
            match booked {
                false => {}
                true => return self.park(cx, w),
            }
            if !cx.admits_records(
                &w.continuation,
                &matched,
                &members,
                |member| member.as_ref().map(|(record, _)| &record.a),
                // Changed by decision 128 (kind routing): the discipline of the source gives the
                // step. The text before:
                // vectordb::payload_of,
                Src::Discipline::similarity_payload,
            )? {
                return self.park(cx, w);
            }
            // Added by decision 124 (plan v4 C7.9 and C7.11, rulings RADD 1.5, 1.7 and 1.2): c0's
            // list, the side of its row and the engine state before its fire, before the fire
            // takes the members, only for an owner-funded run (RC2 D-10). The first member leads
            // the list (CR-13).
            let list = self.staged_list(&matched, &members, coll, !w.peeks.is_empty())?;
            let side = self.receive_side(&w, source);
            let before = self.before(cx);
            // Added by decision 124 (L6, CR30 D-b): the fire takes one lease for each stored
            // datum that its check proved.
            cx.lease_stored(members.iter().flatten().count());
            let pattern = pattern.clone();
            let width = members.len();
            let mut records = Vec::<(ParDatum, usize)>::with_capacity(width);
            for member in members {
                records.push(member.ok_or(SpaceError::Inconsistent(
                    "a receive met an element in flight",
                ))?);
            }
            let peek = !w.peeks.is_empty();
            // Rule E (decision 45): the second send of the first member when the peek removes
            // it. Changed by decision 101 (rule CR-13): a peek binds the first member only and
            // retires nothing, so it owes one second send at most. The text before: "the second
            // sends of the non-persistent members, pushed in reverse, so the first member runs
            // first."
            // let mut sends = Vec::<Work>::with_capacity(width);
            // if peek {
            //     for (record, _) in records.iter().rev() {
            //         if !record.persist {
            //             sends.push(Work {
            //                 coll,
            //                 record: record.a.clone(),
            //                 persist: false,
            //                 own: false,
            //                 restore: None,
            //             });
            //         }
            //     }
            // }
            let mut sends = Vec::<Work>::with_capacity(1);
            match (peek, records.first()) {
                (true, Some((first, _))) if !first.persist => sends.push(Work {
                    coll,
                    record: first.a.clone(),
                    persist: false,
                    own: false,
                    restore: None,
                }),
                (true, _) | (false, _) => {}
            }
            let spent = !w.persist;
            let persistent = records.iter().any(|(record, _)| record.persist);
            let step = cx.rounds().after_fire(spent, peek || persistent);
            // Without a peek, the fire and the retires remove every member that is not
            // persistent.
            if !peek && records.iter().any(|(record, _)| !record.persist) {
                self.changed = true;
            }
            // Changed by decision 101 (rule CR-13): one bind of the first member over `[D]`,
            // with the values of the list. The retires follow the fire.
            // let mut channels = Vec::<Par>::with_capacity(width);
            // let mut patterns = Vec::<BindPattern>::with_capacity(width);
            // let mut candidates = Vec::<ParCandidate>::with_capacity(width);
            // let mut first = Some(matched);
            // for (record, index) in records {
            //     let matched = match first.take() {
            //         Some(matched) => matched,
            //         None => vectordb::payload_of(&record.a)?,
            //     };
            //     let view_index = i32::try_from(index)
            //         .map_err(|_| SpaceError::Inconsistent("a record index exceeds i32"))?;
            //     channels.push(channel.clone());
            //     patterns.push(pattern.clone());
            //     candidates.push(receive_candidate(channel.clone(), ReceiveFire {
            //         head: record,
            //         matched,
            //         view_index,
            //     }));
            // }
            // let peeks = match peek {
            //     true => peek_set_all(width)?,
            //     false => BTreeSet::new(),
            // };
            let mut records = records.into_iter();
            let (record, index) = records
                .next()
                .ok_or(SpaceError::Inconsistent("a list fire has no member"))?;
            let view_index = i32::try_from(index)
                .map_err(|_| SpaceError::Inconsistent("a record index exceeds i32"))?;
            let mut channels = Vec::<Par>::with_capacity(1);
            channels.push(channel.clone());
            let mut patterns = Vec::<BindPattern>::with_capacity(1);
            patterns.push(pattern);
            let mut candidates = Vec::<ParCandidate>::with_capacity(1);
            candidates.push(receive_candidate(channel.clone(), ReceiveFire {
                head: record,
                matched,
                view_index,
            }));
            let peeks = peek_set(peek);
            let continuation = match step {
                FireStep::Continue => w.continuation.clone(),
                FireStep::Done | FireStep::Reissue(_) => std::mem::take(&mut w.continuation),
            };
            // Added by decision 124 (plan v4 C7.9 and C7.11, rulings RADD 1.5, 1.7 and 1.2): the
            // row search of the fire. A list fire spends nothing of the kind: its retires are its
            // list.
            let facts = FireFacts {
                spent,
                stays: peek || persistent,
                peeks: peek,
                changed: self.changed,
            };
            let fire = StagedFire {
                consume: source,
                channel,
                colls: &w.colls,
                tuple: &candidates,
                classes: Vec::new(),
                list,
                peeks: peek,
                site: FireSite::Single,
                before,
                facts: Some(facts),
                sends: &sends,
            };
            let search = self.row_search(cx, side, fire, None)?;
            cx.fire_receive_group(
                channels,
                patterns,
                continuation,
                w.persist,
                peeks,
                candidates,
                1,
                // Added by decision 124 (plan v4 C7.9, ruling RADD 1.6): the row search of the
                // fire, which the source of the kind built (`JoinSource::row_search`).
                search,
            )?;
            // Added by decision 101 (rule CR-13): RETIRE-MEMBERS after a fire that does not
            // peek, with the random state and the authority of the operation.
            if !peek {
                let mut others = Vec::<ParDatum>::with_capacity(records.len());
                others.extend(records.map(|(record, _)| record));
                Src::Discipline::retire_list_members(
                    cx,
                    channel,
                    others,
                    &self.random,
                    self.authority.as_ref(),
                )?;
            }
            self.mark_dirty(&w.colls);
            self.push_work(sends);
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
        // Changed by decision 124 (plan v4 C7.9, ruling RADD 1.5, V4-17): the body moved to
        // `park_with`, which the E3-a parks call with their row search. The text before:
        // // Replaced by P6 (design v3 4.0): the bound of the waiter side under the config.
        // // if let Some(capacity) = self.source.capacity() {
        // if let Some(bound) = self.source.bound(cx.config(), Side::Waiters)? {
        //     let mut checked: Vec<usize> = Vec::new();
        //     for &coll in &w.colls {
        //         if checked.contains(&coll) {
        //             continue;
        //         }
        //         checked.push(coll);
        //         let held = self.source.waiter_count(cx, self.scope, coll)?;
        //         if held >= bound.limit() {
        //             return Err(bound.full());
        //         }
        //     }
        // }
        // self.touch_all(cx, &w.colls)?;
        // let NewWaiter {
        //     group,
        //     patterns,
        //     continuation,
        //     persist,
        //     peeks,
        //     ..
        // } = w;
        // cx.park_on(group, patterns, continuation, persist, peeks)?;
        // self.changed = true;
        // self.own = Own::Settled;
        // Ok(())
        self.park_with(cx, w, None)
    }

    /// Added by decision 124 (plan v4 C7.9, ruling RADD 1.5, C4v2 B.4): the position and the
    /// class of each stored entry of `bound` that binds one, for the item test of c0 at the
    /// commit. Empty without owner funding (RC2 D-10).
    fn stored_classes(&self, bound: &[Bound]) -> Vec<(usize, Claim)> {
        let Some(_inputs) = &self.commit_inputs else {
            return Vec::new();
        };
        let mut classes = Vec::<(usize, Claim)>::with_capacity(bound.len());
        for (position, entry) in bound.iter().enumerate() {
            match (&entry.at, &entry.claim) {
                (EntryAt::Incoming, _) | (EntryAt::Stored(_), Claim::Record { .. }) => {}
                (EntryAt::Stored(_), Claim::Class { .. }) => {
                    classes.push((position, entry.claim.clone()));
                }
            }
        }
        classes
    }

    /// Added by decision 124 (plan v4 C7.9, ruling RADD 1.5): the receive side of the row of the
    /// waiter `w` with the consume `source`: its group, the collection and the slot of each
    /// position, and its copy. `None` without owner funding (RC2 D-10).
    fn receive_side(&self, w: &NewWaiter, source: &Consume) -> Option<JoinSide> {
        let Some(_inputs) = &self.commit_inputs else {
            return None;
        };
        Some(JoinSide::Receive {
            group: w.group.clone(),
            colls: w.colls.clone(),
            slots: w.slots.clone(),
            waiting: ParWaiter {
                patterns: w.patterns.clone(),
                continuation: w.continuation.clone(),
                persist: w.persist,
                peeks: w.peeks.clone(),
                source: source.clone(),
            },
        })
    }

    /// Added by decision 124 (plan v4 C7.9, ruling RADD 1.5): the offer side of the row of a
    /// cascade fire of the lifted element `x` to the waiter `at`. A cascade fires the one waiter
    /// that `next_fire` found, so the order holds c0 alone and the row has no
    /// alternative. The vector of the element gives it its place in the list of a list waiter
    /// (`list_failures`, CR-20 item 2). `None` without owner funding (RC2 D-10).
    fn cascade_side(&self, x: &Incoming, at: &WaiterRef) -> Result<Option<JoinSide>, SpaceError> {
        let Some(_inputs) = &self.commit_inputs else {
            return Ok(None);
        };
        let vector = match &self.similarity {
            Some(cfg) => Src::Discipline::record_vector(&x.record, cfg)?,
            None => None,
        };
        let mut order = Vec::<WaiterRef>::with_capacity(1);
        order.push(at.clone());
        Ok(Some(JoinSide::Offer {
            x: x.clone(),
            vector,
            order,
        }))
    }

    /// Added by decision 124 (plan v4 C7.9, rulings RADD 1.5 and 1.7): c0's list on the
    /// collection `coll`: the values `matched` that its pattern binds, its members `members`,
    /// and the members that its fire retires after it (`list_retires`), none when it peeks.
    /// `None` without owner funding (RC2 D-10), before any read.
    fn staged_list(
        &self,
        matched: &ListParWithRandom,
        members: &[Option<(ParDatum, usize)>],
        coll: usize,
        peeks: bool,
    ) -> Result<Option<StagedList>, SpaceError> {
        let Some(_inputs) = &self.commit_inputs else {
            return Ok(None);
        };
        let others = match peeks {
            true => Vec::new(),
            false => list_retires::<Src::Discipline>(members, self.channel(coll)?)?,
        };
        Ok(Some(StagedList {
            matched: matched.clone(),
            members: members.to_vec(),
            others,
        }))
    }

    /// Added by decision 124 (plan v4 C7.9, ruling RADD 1.5, ADD A1.10 (b)): the row search of
    /// the staged fire `fire` with the side `side`, or `None` without owner funding (RC2 D-10).
    /// The engine copies the parts that the kinds share, and the source of the kind builds the
    /// search of its type with what the fire spends (`spent`). The retire rows take the random
    /// state and the authority of the retires of the staging: those of the element of an offer,
    /// else those of the operation (`RandomPurpose::Retire`).
    fn row_search(
        &self,
        cx: &mut Cx<'_, '_>,
        side: Option<JoinSide>,
        fire: StagedFire<'_>,
        spent: Option<&Src::Spent>,
    ) -> Result<Option<ParRowSearch>, SpaceError> {
        let Some(inputs) = &self.commit_inputs else {
            return Ok(None);
        };
        let Some(side) = side else {
            return Ok(None);
        };
        let Some(before) = fire.before else {
            return Err(SpaceError::Inconsistent(
                "an owner-funded fire has no engine state",
            ));
        };
        let (retire_random, authority) = match &side {
            JoinSide::Offer { x, .. } => (
                RandomPurpose::Retire.derive(&Blake2b512Random::from_bytes(&x.record.random_state)),
                x.record.cost_authority.as_ref(),
            ),
            JoinSide::Receive { .. } => (
                RandomPurpose::Retire.derive(&self.random),
                self.authority.as_ref(),
            ),
        };
        let rows = RetireRows::of(cx, fire.channel, &retire_random, authority)?;
        let mut row = JoinRow {
            inputs: Arc::clone(inputs),
            work: self.work.clone(),
            side,
            staged: fire.consume.clone(),
            channel: fire.channel.clone(),
            colls: fire.colls.to_vec(),
            tuple: fire.tuple.to_vec(),
            classes: fire.classes,
            list: fire.list,
            peeks: fire.peeks,
            theory: cx.theory(),
            alternatives: !cx.leases(),
            rows,
            found: None,
            retires: Vec::new(),
            fired: None,
            site: fire.site,
            before,
            facts: fire.facts,
            sends: fire.sends.to_vec(),
            last: None,
            book: cx.book(),
            reissue: None,
        };
        match cx.seq_reads() {
            None => {}
            Some(seq) => row.keep_admissible(&seq),
        }
        self.source.row_search(cx, row, spent).map(Some)
    }

    /// Added by decision 124 (plan v4 C7.9, ruling RADD 1.5, ADD A1.11, V4-17): the row search of
    /// the E3-a park of the waiter `w` with the consume `source`. It holds no staged tuple, so
    /// the commit reads its item failures (CR-20 item 2) and its search verifies a recorded
    /// refusal (V4-17). `None` without owner funding.
    fn park_search(
        &self,
        cx: &mut Cx<'_, '_>,
        w: &NewWaiter,
        source: &Consume,
    ) -> Result<Option<ParRowSearch>, SpaceError> {
        let coll = *w
            .colls
            .first()
            .ok_or(SpaceError::Inconsistent("a consume has no collection"))?;
        let side = self.receive_side(w, source);
        let fire = StagedFire {
            consume: source,
            channel: self.channel(coll)?,
            colls: &w.colls,
            tuple: &[],
            classes: Vec::new(),
            list: None,
            peeks: !w.peeks.is_empty(),
            site: match w.colls.len() {
                1 => FireSite::Single,
                _ => FireSite::Join,
            },
            before: self.before(cx),
            facts: None,
            sends: &[],
        };
        self.row_search(cx, side, fire, None)
    }

    /// Added by decision 124 (errata V4-17, rules CR-7 and CR-18): the row search of the E3-a
    /// store of the element `x`, over the waiters `order` of its round. It holds no staged tuple,
    /// so the commit verifies a recorded refusal over it and reads its item failures (CR-20 item
    /// 2). `None` without owner funding.
    fn store_search(
        &self,
        cx: &mut Cx<'_, '_>,
        x: &Incoming,
        vector: Option<ExactVector>,
        order: Vec<WaiterRef>,
    ) -> Result<Option<ParRowSearch>, SpaceError> {
        let side = JoinSide::Offer {
            x: x.clone(),
            vector,
            order,
        };
        let consume = Consume::default();
        let fire = StagedFire {
            consume: &consume,
            channel: self.channel(x.coll)?,
            colls: std::slice::from_ref(&x.coll),
            tuple: &[],
            classes: Vec::new(),
            list: None,
            peeks: false,
            site: FireSite::Offer,
            before: self.before(cx),
            facts: None,
            sends: &[],
        };
        self.row_search(cx, Some(side), fire, None)
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD 1.2 and RADD2 1.1): the
    /// engine state that the tail read of the row of the next fire starts from, read before the
    /// fire changes it: the fire count, `changed`, the dirty collections, the state and the cause
    /// of the element of the operation, and its order on each own collection. `None` without
    /// owner funding (RC2 D-10).
    fn before(&self, cx: &mut Cx<'_, '_>) -> Option<Before> {
        let Some(_inputs) = &self.commit_inputs else {
            return None;
        };
        Some(Before {
            fired: cx.rounds().fired(),
            changed: self.changed,
            dirty: self.dirty.clone(),
            own: self.own,
            own_cause: self.own_cause,
            ords: self.ords.clone(),
        })
    }

    /// Added by decision 124 (plan v4 C7.9, ruling RADD 1.5, V4-17): PARK of the waiter `w`, with
    /// the row search `search` of an E3-a park, or `None` for every other park. It holds the body
    /// of `park`, whose HEAD signature stays (RADD section 2).
    fn park_with(
        &mut self,
        cx: &mut Cx<'_, '_>,
        w: NewWaiter,
        search: Option<ParRowSearch>,
    ) -> Result<(), SpaceError> {
        // A park with a row search is the E3-a park of a recorded Stored row, and it takes the
        // side above its bound as the host outcome (rules CR-7 and CR-22).
        match (self.source.bound(cx.config(), Side::Waiters)?, &search) {
            (None, _) | (Some(_), Some(_)) => {}
            (Some(bound), None) => {
                let mut checked = Vec::<usize>::with_capacity(w.colls.len());
                for &coll in &w.colls {
                    match checked.contains(&coll) {
                        false => {}
                        true => continue,
                    }
                    checked.push(coll);
                    let held = self.source.waiter_count(cx, self.scope, coll)?;
                    match held >= bound.limit() {
                        false => {}
                        true => return Err(bound.full()),
                    }
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
        cx.park_on(group, patterns, continuation, persist, peeks, search)?;
        self.changed = true;
        self.own = Own::Settled;
        Ok(())
    }

    /// NEXT-FIRE (joins plan section 4.9): the oldest fireable head of the least dirty
    /// collection, with its search.
    // Changed by decision 124 (plan v4 C7.11, ruling RADD 1.2): the reads of this fn go through
    // the view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn next_fire(&self, cx: &mut Cx<'_, '_>) -> Result<Option<Fireable>, SpaceError> {
    fn next_fire<V: StoreView>(&self, view: &mut V) -> Result<Option<Fireable>, SpaceError> {
        for coll in 0..self.dirty.len() {
            if !self.dirty.get(coll).copied().unwrap_or(false) {
                continue;
            }
            // for at in self.source.cascade_order(cx, self.scope, coll)? {
            for at in self.source.cascade_order(view, self.scope, coll)? {
                // let waiter = self.load(cx, at)?;
                let waiter = self.load(view, at)?;
                // P6 (decision 66, LM-b): a VectorDB collection cascades over its list waiters
                // only. A removal never exposes a single-mode waiter (`ref_fill_mono`).
                // Changed by decision 128 (kind routing): the field takes the name of the window.
                // The text before:
                // if self.vdb.is_some() {
                if self.similarity.is_some() {
                    // if let Some(fire) = self.list_cascade_fire(cx, &waiter)? {
                    if let Some(fire) = self.list_cascade_fire(view, &waiter)? {
                        return Ok(Some(Fireable::List(waiter, fire)));
                    }
                    continue;
                }
                // if !self.all_at_head(cx, &waiter)? {
                if !self.all_at_head(view, &waiter)? {
                    continue;
                }
                // Changed by decision 102 (plan v4 C5.3): `search` takes the lanes from the slots.
                // let lanes = self.lanes(&waiter.slots);
                let Some(bound) = self.search(
                    // cx,
                    view,
                    &waiter.colls,
                    &waiter.waiting.patterns,
                    // Changed by decision 102 (plan v4 C5.3): the lanes come from the slots.
                    // &lanes,
                    &waiter.slots,
                    None,
                    // Added by decision 102 (plan v4 C5.3, rule F3): the cascade search keeps
                    // DR-75.
                    WindowOrder::Canonical,
                )?
                else {
                    continue;
                };
                // Added by decision 124 (plan v4 C7.8, rule CR-20): a pair whose stored record the
                // book holds with the waiter does not fire again.
                match self.pair_booked(view, &waiter.colls, &waiter.waiting.source, &bound)? {
                    false => {}
                    true => continue,
                }
                if self.admits(
                    // cx,
                    view,
                    &waiter.waiting.continuation,
                    &waiter.slots,
                    &waiter.colls,
                    &bound,
                )? {
                    return Ok(Some(Fireable::Pair(waiter, bound)));
                }
            }
        }
        Ok(None)
    }

    /// The list of the stored list waiter `waiter` of a VectorDB collection when the cascade owes
    /// its fire (P6, decision 66, LM-b): the list `L(W, R)` is not empty, the pattern binds it and
    /// the guard admits it. Every other waiter is `None`.
    // Changed by decision 124 (plan v4 C7.11, ruling RADD 1.2): the reads of this fn go through
    // the view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn list_cascade_fire(
    fn list_cascade_fire<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        waiter: &Waiter,
    ) -> Result<Option<ListFire>, SpaceError> {
        let Some((query, k)) = self.list_waiter(&waiter.slots)? else {
            return Ok(None);
        };
        // Changed by decision 128 (kind routing): the field takes the name of the window, and the
        // window owns the text of the error. The text before:
        // ```
        // let cfg = self.vdb.ok_or(SpaceError::Inconsistent(
        //     "a list waiter outside a VectorDB space",
        // ))?;
        // ```
        let cfg = self
            .similarity
            .ok_or(SpaceError::Inconsistent(LIST_OUTSIDE_WINDOW))?;
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
        // let Some(fire) = vectordb::list_selection(cx, &cfg, &query, k, pattern, channel, None)?
        let Some(fire) =
            Src::Discipline::list_selection(view, &cfg, &query, k, pattern, channel, None)?
        else {
            return Ok(None);
        };
        // Added by decision 124 (plan v4 C7.8, rule CR-20, ruling RAMD 1.5): a list with a member
        // whose pair the book holds does not fire again.
        let booked = view.list_booked(
            None,
            fire.members.iter().flatten().map(|(record, _)| record),
            &waiter.waiting.source,
        );
        match booked {
            false => {}
            true => return Ok(None),
        }
        // match cx.admits_records(
        match view.admits_records(
            &waiter.waiting.continuation,
            &fire.matched,
            &fire.members,
            |member| member.as_ref().map(|(record, _)| &record.a),
            // Changed by decision 128 (kind routing): the discipline of the source gives the step.
            // The text before:
            // vectordb::payload_of,
            Src::Discipline::similarity_payload,
        )? {
            true => Ok(Some(fire)),
            false => Ok(None),
        }
    }

    /// Added by decision 124 (plan v4 C7.8 and C7.11, rule CR-20, rulings OP-F13 and RADD 1.2):
    /// true when the book of the view holds the pair of a stored record of `bound` on the
    /// collections `colls` and the consume `waiter`. The record is the one that the fire binds
    /// (`candidates`), so the pair is the pair that a refusal of the fire books. A record also
    /// answers for the pair that a refusal of its own offer booked under the identity of that
    /// offer (`JoinSource::stored_offer_source`). It reads through the view, so the tail read of
    /// a join row runs it over the store of the commit with the book of the staging (ADD A1.13
    /// (a) in the form of the view).
    fn pair_booked<V: StoreView>(
        &self,
        view: &mut V,
        colls: &[usize],
        waiter: &Consume,
        bound: &[Bound],
    ) -> Result<bool, SpaceError> {
        for (entry, &coll) in bound.iter().zip(colls) {
            let EntryAt::Stored(index) = entry.at else {
                continue;
            };
            let source = {
                let records = view.data_on(self.channel(coll)?)?;
                let record = records.get(index).ok_or(SpaceError::Inconsistent(
                    "a bound index is outside the staged view",
                ))?;
                record.source.clone()
            };
            match view.booked(&source, waiter) {
                false => {}
                true => return Ok(true),
            }
            match self
                .source
                .stored_offer_source(view, self.scope, coll, index)?
            {
                Some(offered) if view.booked(&offered, waiter) => return Ok(true),
                Some(_) | None => {}
            }
        }
        Ok(false)
    }

    /// Added by decision 124 (plan v4 C7.8, rule CR-20, rulings OP-F13 and RAMD 1.5): true when
    /// the book holds a pair of the stored waiter `waiter` and a record of the offer of `x`: the
    /// element, another member `others` of a list fire, or a stored record of `bound`. The pair
    /// of the element names the identity of the incoming side of the fire at `position`, as the
    /// row of a refusal of the fire names it (`JoinSource::incoming_source`).
    fn offer_booked(
        &self,
        cx: &mut Cx<'_, '_>,
        waiter: &Waiter,
        x: &Incoming,
        position: usize,
        bound: &[Bound],
        others: Option<&[ParDatum]>,
    ) -> Result<bool, SpaceError> {
        let consume = &waiter.waiting.source;
        let coll = *waiter.colls.get(position).ok_or(SpaceError::Inconsistent(
            "a fire position is outside its waiter",
        ))?;
        let incoming = self.source.incoming_source(cx, self.scope, x, coll)?;
        match cx.list_booked(Some(&incoming), others.unwrap_or_default(), consume) {
            false => self.pair_booked(cx, &waiter.colls, consume, bound),
            true => Ok(true),
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
            // Added by decision 124 (L6, CR30 D-b): the fire takes one lease for each stored
            // datum that its check proved (VJ V11): after the cap check, so a round that the
            // cap stops keeps no lease.
            let stored = match &fireable {
                Fireable::Pair(_, bound) => stored_count(bound),
                Fireable::List(_, fire) => fire.members.iter().flatten().count(),
            };
            cx.lease_stored(stored);
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
        let ListFire { matched, members } = fire;
        let coll = *waiter
            .colls
            .first()
            .ok_or(SpaceError::Inconsistent("a list waiter has no collection"))?;
        // Added by decision 124 (plan v4 C7.9 and C7.11, rulings RADD 1.5, 1.7 and 1.2): c0's
        // list and the engine state before its fire, before the records take its members, only
        // for an owner-funded run (RC2 D-10). The first member leads: the cascade lifts it and
        // offers it again.
        let list = self.staged_list(&matched, &members, coll, !waiter.waiting.peeks.is_empty())?;
        let before = self.before(cx);
        let mut records = Vec::<(ParDatum, usize)>::with_capacity(members.len());
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
            // Added by decision 124 (plan v2 C7.5, F5, rule CR-16, ruling RAMD 1.6): the lift
            // records the pair of the lifted record and the waiter of the re-offer, for the book
            // of a refused lift (CR-20).
            cx.record_lift((
                datum.source.hash.clone(),
                waiter.waiting.source.hash.clone(),
            ));
        }
        let spent = !x.persist && !peeks;
        let stays = waiter.waiting.persist;
        let step = cx.rounds().after_fire(spent, stays);
        let claim = self.source.claim(coll, EntryAt::Incoming, &x.payload)?;
        let mut bound = Vec::<Bound>::with_capacity(1);
        bound.push(Bound {
            at: EntryAt::Incoming,
            matched,
            persist: x.persist,
            claim,
        });
        let candidates = self.candidates(cx, &waiter.colls, bound, Some(&x), None)?;
        let colls = waiter.colls.to_vec();
        let group = waiter.at.group.clone();
        let channel = self.channel(coll)?;
        let data = self.source.incoming_record(cx, self.scope, &x, coll)?;
        // Added by decision 124 (plan v4 C7.9 and C7.11, rulings RADD 1.5, 1.7 and 1.2): the row
        // search of the cascade fire. A list fire spends nothing of the kind: its retires are its
        // list.
        let side = self.cascade_side(&x, &waiter.at)?;
        // Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.1): the second send
        // that the fire pushes on a peek (below), for the re-issue of its row, only for an
        // owner-funded run.
        let lift_sends = match (&before, peeks && x.in_flight) {
            (None, _) | (Some(_), false) => Vec::new(),
            (Some(_), true) => vec![Work {
                coll,
                record: x.record.clone(),
                persist: x.persist,
                own: false,
                restore: None,
            }],
        };
        let facts = FireFacts {
            spent,
            stays,
            peeks,
            changed: self.changed,
        };
        let fire = StagedFire {
            consume: &waiter.waiting.source,
            channel,
            colls: &colls,
            tuple: &candidates,
            classes: Vec::new(),
            list,
            peeks,
            site: FireSite::Cascade,
            before,
            facts: Some(facts),
            sends: &lift_sends,
        };
        let search = self.row_search(cx, side, fire, None)?;
        cx.fire_offer_group(
            channel,
            data,
            x.persist,
            waiter.waiting,
            group,
            candidates,
            &x.introduction,
            // Added by decision 102 (plan v4 C7.22, rule CR-19 case 3): a persistent lead stays
            // stored, so its re-offer is not the job's own.
            x.offered(),
            // Added by decision 124 (plan v4 C7.9, ruling RADD 1.6): the row search of the fire,
            // which the source of the kind built (`JoinSource::row_search`).
            search,
        )?;
        self.mark_dirty(&colls);
        match peeks {
            true => {
                if x.in_flight {
                    let send = Work {
                        coll,
                        record: x.record.clone(),
                        persist: x.persist,
                        own: false,
                        restore: None,
                    };
                    self.work.push(send);
                }
            }
            false => {
                let mut others = Vec::<ParDatum>::with_capacity(records.len());
                others.extend(records.into_iter().map(|(record, _)| record));
                // Changed by decision 101 (rule CR-13): the random state and the authority of
                // the element are arguments.
                // vectordb::retire_members(cx, channel, others, &x.record)?;
                let element_random = Blake2b512Random::from_bytes(&x.record.random_state);
                let authority = x.record.cost_authority.as_ref();
                Src::Discipline::retire_list_members(
                    cx,
                    channel,
                    others,
                    &element_random,
                    authority,
                )?;
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
        // Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.2): the engine state
        // before the fire, for the tail read of its row, only for an owner-funded run.
        let before = self.before(cx);
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
            // Changed by decision 101 (rule CR-13): the hook takes no members argument.
            false => Some(self.source.before_fire(
                cx,
                self.scope,
                FireSite::Cascade,
                &waiter.colls,
                &bound,
                // &[],
                Some(&x),
            )?),
        };
        let shift = match lifted {
            Some(_) => {
                let step = i64::try_from(cx.rounds().fired())
                    .map_err(|_| SpaceError::Inconsistent("a lift step overflows i64"))?;
                let counter = self.bumped(coll)?;
                self.lift(cx, coll, &datum, index, counter, step)?;
                // Added by decision 124 (plan v2 C7.5, F5, rule CR-16, ruling RAMD 1.6): the lift
                // records the pair of the lifted record and the waiter of the re-offer, for the
                // book of a refused lift (CR-20).
                cx.record_lift((
                    datum.source.hash.clone(),
                    waiter.waiting.source.hash.clone(),
                ));
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
        // Added by decision 124 (plan v4 C7.9, ruling RADD 1.5, C4v2 B.4): the classes of
        // the bound entries, for the item test of c0 at the commit.
        let classes = self.stored_classes(&fired);
        let candidates = self.candidates(cx, &waiter.colls, fired, Some(&x), shift)?;
        let colls = waiter.colls.to_vec();
        let group = waiter.at.group.clone();
        let channel = self.channel(coll)?;
        let data = self.source.incoming_record(cx, self.scope, &x, coll)?;
        // Added by decision 124 (plan v4 C7.9 and C7.11, rulings RADD 1.5 and 1.2): the row search
        // of the cascade fire, built only for an owner-funded run (RC2 D-10), with what the fire
        // decides for the steps after it.
        let side = self.cascade_side(&x, &waiter.at)?;
        let facts = FireFacts {
            spent,
            stays,
            peeks,
            changed: self.changed,
        };
        let fire = StagedFire {
            consume: &waiter.waiting.source,
            channel,
            colls: &colls,
            tuple: &candidates,
            classes,
            list: None,
            peeks,
            site: FireSite::Cascade,
            before,
            facts: Some(facts),
            sends: &sends,
        };
        let search = self.row_search(cx, side, fire, retiring.as_ref())?;
        cx.fire_offer_group(
            channel,
            data,
            x.persist,
            waiter.waiting,
            group,
            candidates,
            &x.introduction,
            // Added by decision 102 (plan v4 C7.22, rule CR-19 case 3): a persistent record that
            // the cascade re-offers without a lift stays stored, so its re-offer is not the job's
            // own.
            x.offered(),
            // Added by decision 124 (plan v4 C7.9, ruling RADD 1.6): the row search of the fire,
            // which the source of the kind built (`JoinSource::row_search`).
            search,
        )?;
        self.mark_dirty(&colls);
        match retiring {
            None => self.push_work(sends),
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
    /// the random state and the step.
    fn lift(
        &mut self,
        cx: &mut Cx<'_, '_>,
        coll: usize,
        datum: &ParDatum,
        index: usize,
        counter: Option<i64>,
        step: i64,
    ) -> Result<(), SpaceError> {
        let channel = self.channel(coll)?;
        let matched = cx
            .bind(&LIFT_PATTERN, &datum.a)
            .ok_or(SpaceError::Inconsistent(
                "a stored record does not match the lift pattern",
            ))?;
        let random = self
            .lift_random
            .get_or_insert_with(|| RandomPurpose::Lift.derive(&self.random))
            .clone();
        // Changed by decision 124 (plan v2 C7.6, decision 99): the `K_lift` of a join takes the
        // machinery seal. The text before:
        // let cost_authority = self.authority.as_ref().cloned();
        let cost_authority = cx.introduction().seal_of(self.authority.as_ref())?;
        let continuation = match counter {
            Some(counter) => lift_continuation(counter, step, &random, cost_authority),
            None => retire_continuation(&datum.source.hash, &random, cost_authority),
        };
        let view_index = i32::try_from(index)
            .map_err(|_| SpaceError::Inconsistent("a record index exceeds i32"))?;
        let mut channels = Vec::<Par>::with_capacity(1);
        channels.push(channel.clone());
        let mut patterns = Vec::<BindPattern>::with_capacity(1);
        patterns.push((*LIFT_PATTERN).clone());
        let mut candidates = Vec::<ParCandidate>::with_capacity(1);
        candidates.push(ConsumeCandidate {
            channel: channel.clone(),
            datum: Datum {
                a: matched,
                persist: datum.persist,
                source: datum.source.clone(),
            },
            removed_datum: datum.a.clone(),
            datum_index: view_index,
        });
        // Changed by decision 124 (plan v4 C7.11, rule CR-16, rulings RADD2 1.6, RADD2v2 6.4 and
        // 6.6): an owner-funded lift carries the lift search of the kind of the source. It holds
        // the machinery search of the discipline of the source (`JoinSource::Discipline`)
        // and the cut of the lift (`LiftCut`): the inputs of the run and the engine state before
        // the lift, from which the commit ends Tail(actual) of a refused lift. A run without
        // owner funding builds no search (RC2 D-10). The text before:
        // cx.lift(channels, patterns, continuation, candidates)
        let search = match (self.commit_inputs.as_ref(), self.before(cx)) {
            (None, _) | (_, None) => None,
            (Some(inputs), Some(before)) => Some(self.source.lift_search(LiftRow {
                machinery: StagedRowSearch::<Src::Discipline>::of_machinery(&channels, &candidates),
                cut: LiftCut {
                    inputs: Arc::clone(inputs),
                    before,
                },
            })),
        };
        cx.lift_with(channels, patterns, continuation, candidates, search)
    }

    /// COMMIT (joins plan section 4.10): the re-issue of the transaction. A quiescent transaction
    /// re-issues the operation when its element is still in flight. A stopped transaction with
    /// owed work re-issues one `SettleOp` with the cascade state, and otherwise the operation.
    fn commit(
        // Changed by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.1): the commit
        // stages the finish and moves the engine into `reissue_of`, so it changes no field. The
        // text before:
        // mut self,
        self,
        cx: &mut Cx<'_, '_>,
        op: SpaceOp,
    ) -> Result<Option<SpaceReissue>, SpaceError> {
        let authority = OperationAuthority {
            continuation: self.authority.as_ref(),
            introduction: cx.introduction(),
        };
        self.source
            .finish(cx, self.scope, self.changed, &self.random, authority)?;
        // Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.1): the rest of the
        // commit reads no `cx`, so the tail read of a join row runs the same code (`end_of`).
        self.reissue_of(op)
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.1): the re-issue of the
    /// transaction of `op` after its finish (COMMIT, joins plan section 4.10), from the fields of
    /// the engine alone. The commit calls it after the finish, and the tail read of a join row
    /// calls it on the state at the end of the read (`JoinRow::end_of`).
    fn reissue_of(mut self, op: SpaceOp) -> Result<Option<SpaceReissue>, SpaceError> {
        let pending: Vec<Work> = {
            let mut pending = Vec::new();
            for work in std::mem::take(&mut self.work).into_iter().rev() {
                match work.own {
                    true => self.own = Own::InFlight,
                    false => pending.push(work),
                }
            }
            pending
        };
        let owed = !pending.is_empty() || (self.cascades() && self.dirty.iter().any(|&mark| mark));
        let settle_fields = match (&self.stop, owed) {
            (Some(_), true) => Some(self.settle_fields(&op)?),
            (Some(_), false) | (None, _) => None,
        };
        // Changed by decision 124 (plan v4 C7.11, ruling RADD2 1.1): the resume reads no `cx`.
        // The text before:
        // let resume = self.resume(cx, op)?;
        let resume = self.resume(op)?;
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
                        if *key == settle.key {
                            continue;
                        }
                        seeds.push(key.clone());
                    }
                }
                let mut pending_produces = Vec::<PendingProduce>::with_capacity(pending.len());
                for work in pending {
                    pending_produces.push(PendingProduce {
                        key: collection(self.scope, work.coll)?.key.clone(),
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
                // Changed by decision 124 (plan v4 C7.11, ruling RADD2 1.1): the value of
                // `Cx::reissue`, built without `cx`. The text before:
                // Ok(Some(cx.reissue(SpaceOp::Settle(settle), cause)))
                Ok(Some(SpaceReissue {
                    op: SpaceOp::Settle(settle),
                    cause,
                }))
            }
        }
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD2 1.1 and RADD2v2 6.4, C2
    /// E4.5): the end of Tail(actual) that stops the operation `op` at the state of the engine
    /// for an owner refusal (CR-19 case 2): the re-issue of the rest, as the stop of the engine
    /// gives it (`reissue_of`). A step that the run owes always leaves a re-issue, so a stop
    /// without one fails closed.
    fn refusal_stop(mut self, op: SpaceOp) -> Result<TailEnd, SpaceError> {
        self.stop = Some(ReissueCause::OwnerRefusal);
        let Some(reissue) = self.reissue_of(op)? else {
            return Err(SpaceError::Inconsistent(
                "a join row owes a step that no re-issue carries",
            ));
        };
        Ok(TailEnd::Stop(reissue.op))
    }

    /// The fields of the settle job that continues a stopped transaction of `op`: the space, the
    /// key and the channel of the operation (of the first bind of a join), its random state, its
    /// cost authority and its introduction authority.
    fn settle_fields(&self, op: &SpaceOp) -> Result<SettleOp, SpaceError> {
        match op {
            SpaceOp::Produce(produce) => Ok(settle_operation(generic::OpView::Produce(produce))),
            SpaceOp::Consume(consume) => Ok(settle_operation(generic::OpView::Consume(consume))),
            SpaceOp::Settle(settle) => Ok(SettleOp {
                space: settle.space,
                key: settle.key.clone(),
                channel: settle.channel.clone(),
                random_state: settle.random_state.clone(),
                authority: settle.authority.clone(),
                introduction_authority: settle.introduction_authority.clone(),
                cascade: None,
            }),
            SpaceOp::Join(join) => {
                let first = join
                    .binds
                    .first()
                    .ok_or(SpaceError::Inconsistent("a join has no bind"))?;
                Ok(SettleOp {
                    space: join.space,
                    key: first.key.clone(),
                    channel: first.channel.clone(),
                    random_state: join.body.random_state.clone(),
                    authority: join.authority.clone(),
                    introduction_authority: join.introduction_authority.clone(),
                    cascade: None,
                })
            }
        }
    }

    /// The re-issue of the operation (decision 43 order): `None` when its element settled. A
    /// produce and a consume keep the order of their first round when they are persistent, and
    /// a join keeps the order of each bind. A settle re-issues the re-issue that it carries.
    // Changed by decision 124 (plan v4 C7.11, ruling RADD2 1.1): the resume reads no `cx`, so the
    // tail read of a join row runs it in `reissue_of`. The text before, one line of 100 columns,
    // here split after `op: SpaceOp)`:
    // fn resume(&self, cx: &mut Cx<'_, '_>, op: SpaceOp)
    //     -> Result<Option<SpaceReissue>, SpaceError> {
    fn resume(&self, op: SpaceOp) -> Result<Option<SpaceReissue>, SpaceError> {
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
                            let coll = self.scope.index_of_key(&produce.key)?;
                            let ord = self.ord_on(coll)?;
                            SpaceOp::Produce(ProduceOp {
                                reissued_ord: produce.persistent.then_some(ord),
                                ..produce
                            })
                        }
                        SpaceOp::Consume(consume) => {
                            let coll = self.scope.index_of_key(&consume.key)?;
                            let ord = self.ord_on(coll)?;
                            SpaceOp::Consume(ConsumeOp {
                                reissued_ord: consume.persistent.then_some(ord),
                                ..consume
                            })
                        }
                        SpaceOp::Join(join) => {
                            let reissued_ords = match join.persistent {
                                true => {
                                    let mut ords = Vec::<i64>::with_capacity(join.binds.len());
                                    for bind in &join.binds {
                                        let coll = self.scope.index_of_key(&bind.key)?;
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
                    // Changed by decision 124 (plan v4 C7.11, ruling RADD2 1.1): the value of
                    // `Cx::reissue`, built without `cx`. The text before:
                    // Ok(Some(cx.reissue(reissued, cause)))
                    Ok(Some(SpaceReissue {
                        op: reissued,
                        cause,
                    }))
                }
            },
        }
    }
}

/// Added by decision 124 (L6): the number of stored entries of `bound`. The check of a fire
/// proves each one (`Engine::admits`, TH-6), and the fire takes one lease for each (TH-9).
fn stored_count(bound: &[Bound]) -> usize {
    bound
        .iter()
        .filter(|entry| matches!(entry.at, EntryAt::Stored(_)))
        .count()
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
/// claims of one collection compare their keys, the hashes first (DR-90).
fn is_claimed(claimed: &[Claim], claim: &Claim) -> Result<bool, SpaceError> {
    for other in claimed {
        let same = match (other, claim) {
            (Claim::Record { coll, at }, Claim::Record { coll: c, at: a }) => coll == c && at == a,
            (
                Claim::Class { coll, key, hash },
                Claim::Class {
                    coll: c, key: k, ..
                },
            ) => *coll == *c && same_class(key, hash, k)?,
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
    use models::rhoapi::tagged_continuation::TaggedCont;
    use models::rhoapi::{Expr, ParWithRandom};
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
    // Added by decision 128 (kind routing): the engine no longer imports the kind module, and
    // the tests name it.
    use crate::rust::interpreter::spaces::disciplines::vectordb;
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

    // Changed by decision 124 (F2, base one row source for each produce,
    // replay_rspace.rs:1058-1082): the space takes its outer store, so a test of a PathMap offer
    // gets a PathMap space. `space_of` below keeps the HashMap space. The line before:
    // async fn space_of(inner: SpaceInner) -> (ParRSpace, SpaceEnv) {
    /// A space of the family `outer:inner` over a fresh play space.
    async fn space_of_outer(outer: SpaceOuter, inner: SpaceInner) -> (ParRSpace, SpaceEnv) {
        let mut stores = InMemoryStoreManager::new();
        let host = ParRSpace::create(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(Matcher)),
        )
        .expect("an in-memory space");
        let env = SpaceEnv {
            store: SpaceStoreHandle::new(host.clone()),
            budget: RuntimeBudget::new(Cost::create(10_000_000, "join engine test")),
            volatile: Default::default(),
            origin: Default::default(),
        };
        // Changed by decision 124 (F2): the family of `outer`. The line before:
        //     .find(|urn| urn.outer == SpaceOuter::HashMap && urn.inner == inner)
        let family = SpaceFactoryUrn::all()
            .find(|urn| urn.outer == outer && urn.inner == inner)
            .expect("a factory family");
        // Changed by decision 124 (F2): the config names `outer`. The line before:
        //     SpaceOuter::HashMap.as_str(),
        let config = SpaceConfig::new(
            SPACE.sid,
            outer.as_str(),
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

    /// Added by decision 124 (F2, base one row source for each produce,
    /// replay_rspace.rs:1058-1082): a space of the family `hashmap:inner`, as before F2.
    async fn space_of(inner: SpaceInner) -> (ParRSpace, SpaceEnv) {
        space_of_outer(SpaceOuter::HashMap, inner).await
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

    use crate::rust::interpreter::spaces::disciplines::queue::QueueDiscipline;
    use crate::rust::interpreter::spaces::disciplines::stack::StackDiscipline;
    use crate::rust::interpreter::spaces::generic::begin_collection;
    use crate::rust::interpreter::spaces::outer::hashmap::HashMapOuter;

    type Queue = GenericSpace<HashMapOuter, QueueDiscipline>;

    /// The route of `op` on the collection of `channel`.
    async fn classified(env: &SpaceEnv, channel: &Par, op: &SpaceOp) -> JoinRoute {
        let mut opened = begin_collection(&HashMapOuter, env, SPACE, &key_of(channel), channel)
            .await
            .expect("the collection opens");
        classify(&Queue::default(), &mut opened, op).expect("the operation classifies")
    }

    #[tokio::test]
    async fn space_join_engine_classification_routes_a_collection_without_other_groups_alone() {
        let (_host, env) = space_of(SpaceInner::Queue).await;
        let a = string("a");
        // No join group: the single route.
        assert_eq!(
            classified(&env, &a, &send(&a, 1, false)).await,
            JoinRoute::Single
        );
        // The own group only: the single route.
        run(&env, receive(&a, "w", exactly(9))).await;
        assert_eq!(
            classified(&env, &a, &send(&a, 1, false)).await,
            JoinRoute::Single
        );
    }

    #[tokio::test]
    async fn space_join_engine_scope_closes_over_the_join_groups() {
        let (_host, env) = space_of(SpaceInner::Queue).await;
        let (a, b, c, d) = (string("a"), string("b"), string("c"), string("d"));
        // W1 joins a and b, and W2 joins b and c. A send on a reaches c through b.
        run(&env, join(&[(&a, any()), (&b, exactly(5))], "w1", false)).await;
        run(&env, join(&[(&b, exactly(6)), (&c, any())], "w2", false)).await;
        let route = classified(&env, &a, &send(&a, 1, false)).await;
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
        assert!(!scope.covers(&scope.own));
        assert!(scope.covers(&keys));
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
        let route = classified(&env, &d, &send(&d, 1, false)).await;
        assert_eq!(route, JoinRoute::Single);
        // A join always classifies as a join, with its binds as its own collections.
        let route = classified(&env, &d, &join(&[(&d, any()), (&a, any())], "j", false)).await;
        let JoinRoute::Join(scope) = route else {
            panic!("a join runs in the join engine");
        };
        assert_eq!(scope.own.len(), 2);
        assert_eq!(scope.collections.len(), 4);
    }

    #[test]
    fn space_join_engine_place_follows_the_head_rule() {
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
        assert_eq!(queue.place(Side::Data, Placement::Fresh, 9, &stored), Ok(3));
        assert_eq!(queue.place(Side::Data, Placement::Fresh, 9, &[]), Ok(0));
        assert_eq!(stack.place(Side::Data, Placement::Fresh, 9, &stored), Ok(0));
        // A carried order takes its place: behind the lesser orders of a queue, below the
        // greater orders of a stack.
        assert_eq!(
            queue.place(Side::Data, Placement::Carried, 6, &stored),
            Ok(2)
        );
        assert_eq!(
            queue.place(Side::Data, Placement::Carried, 1, &stored),
            Ok(0)
        );
        assert_eq!(
            stack.place(Side::Data, Placement::Carried, 6, &stored),
            Ok(1)
        );
        assert_eq!(
            stack.place(Side::Data, Placement::Carried, 9, &stored),
            Ok(0)
        );
        assert_eq!(
            queue.place(Side::Data, Placement::Carried, 5, &stored),
            Err(SpaceError::Inconsistent(
                "an element in flight shares its order with a stored entry"
            ))
        );
    }

    #[test]
    fn space_join_engine_head_window_places_the_element_in_flight() {
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
            queue.head_window(&records, 2, None),
            Ok(vec![stored(1), stored(2)])
        );
        assert_eq!(
            queue.head_window(&records, 5, None),
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
            ),
            own: true,
            in_flight: true,
            restore: None,
        };
        // A fresh element of a queue is at the tail: it is in a window of four entries only.
        assert_eq!(
            queue.head_window(&records, 2, Some(&element(9, Placement::Fresh))),
            Ok(vec![stored(1), stored(2)])
        );
        assert_eq!(
            queue.head_window(&records, 4, Some(&element(9, Placement::Fresh))),
            Ok(vec![stored(1), stored(2), stored(0), incoming])
        );
        // A carried order 3 takes the second place of the head order.
        assert_eq!(
            queue.head_window(&records, 2, Some(&element(3, Placement::Carried))),
            Ok(vec![stored(1), incoming])
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

    // Changed by decision 118: the trace of F-JF-1 is vacuous (NoSetCascadeUnsafe.cfg:8-14).
    // Under rule (a) (decision-c6 §3, plan-join-batch Step 5 item 12 and C-18), W fires at its
    // arrival with a1 and a2: a1 claims its record, and a2 is persistent, so the class of 1 is
    // free. V then fires with a2 through a one-bind consume on [D(s)]. The body before:
    // #[tokio::test]
    // async fn space_join_engine_set_cascade_fires_a_waiter_that_a_class_move_exposes() {
    //     let (host, env) = space_of(SpaceInner::Set).await;
    //     let s = string("s");
    //     // The class of 1 has a non-persistent member a1 and a persistent member a2, and b is the
    //     // class of 2. The canonical order is a1, b, a2, so the class of 1 sits at the place of
    //     // a1 first.
    //     let (a1, a1_hash) = seeded_send(&s, 1, "a1", false);
    //     let (b, b_hash) = (0..)
    //         .map(|seed| seeded_send(&s, 2, &format!("b{seed}"), false))
    //         .find(|(_, hash)| *hash > a1_hash)
    //         .expect("a seed orders b after a1");
    //     let (a2, _) = (0..)
    //         .map(|seed| seeded_send(&s, 1, &format!("a2-{seed}"), true))
    //         .find(|(_, hash)| *hash > b_hash)
    //         .expect("a seed orders a2 after b");
    //     for op in [a1, b, a2] {
    //         run(&env, op).await;
    //     }
    //     // W binds any value and then 1. The greedy search gives the first class (1) to the first
    //     // bind, and the second bind finds no other class of 1: W parks (decision 37).
    //     let parked = run(&env, join(&[(&s, any()), (&s, exactly(1))], "w", false)).await;
    //     assert!(parked.fires.is_empty());
    //     // V takes the class of 1: a1 leaves, and the persistent a2 stays. The class of 1 now
    //     // sits at the place of a2, after b, so the greedy search of W succeeds: the cascade of
    //     // the set fires W with b and a2 (decision 50).
    //     let outcome = run(&env, receive(&s, "v", exactly(1))).await;
    //     assert_eq!(fired(&outcome), vec![fire("v", &[]), fire("w", &[2])]);
    //     assert_eq!(payloads(&host, &s).await, vec![vec![int(1)]]);
    //     assert!(host
    //         .get_waiting_continuations(vec![data_of(&s), data_of(&s)])
    //         .await
    //         .is_empty());
    // }
    #[tokio::test]
    async fn space_join_engine_set_cascade_fires_a_waiter_that_a_class_move_exposes() {
        let (host, env) = space_of(SpaceInner::Set).await;
        let s = string("s");
        // The class of 1 has a non-persistent member a1 and a persistent member a2, and b is the
        // class of 2. The canonical order is a1, b, a2.
        let (a1, a1_hash) = seeded_send(&s, 1, "a1", false);
        let (b, b_hash) = (0..)
            .map(|seed| seeded_send(&s, 2, &format!("b{seed}"), false))
            .find(|(_, hash)| *hash > a1_hash)
            .expect("a seed orders b after a1");
        let (a2, a2_hash) = (0..)
            .map(|seed| seeded_send(&s, 1, &format!("a2-{seed}"), true))
            .find(|(_, hash)| *hash > b_hash)
            .expect("a seed orders a2 after b");
        for op in [a1, b, a2] {
            run(&env, op).await;
        }
        host.take_event_log().await;
        // W binds any value and then 1. The first bind takes a1, which claims its record and the
        // class of 1. The class holds the persistent a2, so the second bind takes a2: W fires at
        // its arrival, with no `K_lift` and no `K_retire`.
        let outcome = run(&env, join(&[(&s, any()), (&s, exactly(1))], "w", false)).await;
        assert_eq!(fired(&outcome), vec![fire("w", &[1])]);
        let log = host.take_event_log().await;
        let comms = comms_of(&log);
        let [comm] = comms.as_slice() else {
            panic!("one COMM of W, not {}", comms.len());
        };
        assert_eq!(bound_sources(comm), vec![a1_hash, a2_hash.clone()]);
        // V then fires with a2, which stays. The set keeps b and a2.
        let outcome = run(&env, receive(&s, "v", exactly(1))).await;
        assert_eq!(fired(&outcome), vec![fire("v", &[])]);
        let log = host.take_event_log().await;
        let comms = comms_of(&log);
        let [comm] = comms.as_slice() else {
            panic!("one COMM of V, not {}", comms.len());
        };
        assert_eq!(bound_sources(comm), vec![a2_hash.clone()]);
        let mut stored: Vec<Blake2b256Hash> = dr75_records(&host, &s)
            .await
            .into_iter()
            .map(|(produce, _)| produce.hash)
            .collect();
        stored.sort();
        let mut expected = vec![b_hash, a2_hash];
        expected.sort();
        assert_eq!(stored, expected, "the set keeps b and a2");
        assert!(host
            .get_waiting_continuations(vec![data_of(&s), data_of(&s)])
            .await
            .is_empty());
    }

    // ── Decision 102 (decision 119 G1, E3-a) and decision 101: the receive of a set ──────────

    // Added by decision 102 (plan v4 C5.4 and C5.6): the tests of the one-bind fire of a set
    // and of E3-a read the COMMs of the log, guard a receive and replay from a script.
    use models::rhoapi::var::VarInstance;
    use models::rhoapi::{EEq, EVar, Var};
    // Added by decision 102 (plan v4 C5.3): the pick tests read the DR-75 order of the store.
    use rspace_plus_plus::rspace::space_store_handle::{
        canonical_continuation_order, canonical_data_order,
    };
    use rspace_plus_plus::rspace::trace::event::{Event, COMM};

    // Added by decision 124 (L3, J1 ruling 2): the E3-a offer test runs on a bag.
    use crate::rust::interpreter::spaces::disciplines::bag::BagDiscipline;
    use crate::rust::interpreter::spaces::disciplines::set::SetDiscipline;
    use crate::rust::interpreter::spaces::generic::prepare_collection_op;
    // Changed by decision 102 (plan v4 C5.3): the pick tests replay from per-source rows.
    use crate::rust::interpreter::spaces::test_support::{
        recorded, replaying, replaying_rows, ScriptedRow, ScriptedSource,
    };

    /// Added by decision 102 (rule CR-1): the runs of a play test of a random pick, each on a
    /// fresh space. A pick of two candidates keeps one order in all runs with probability
    /// 2^-63.
    const PICK_RUNS: usize = 64;

    /// Added by decision 101 (rule CR-13): the COMMs of the event log `log`, in log order.
    fn comms_of(log: &[Event]) -> Vec<&COMM> {
        log.iter()
            .filter_map(|event| match event {
                Event::Comm(comm) => Some(comm),
                Event::IoEvent(_) => None,
            })
            .collect()
    }

    /// Added by decision 101 (rule CR-13): the source hashes of the produces that `comm` binds.
    fn bound_sources(comm: &COMM) -> Vec<Blake2b256Hash> {
        comm.produces
            .iter()
            .map(|produce| produce.hash.clone())
            .collect()
    }

    /// Added by decision 101 (rule CR-13): a peek receive whose body is the string `name`.
    fn peek_receive(channel: &Par, name: &str, pattern: BindPattern) -> SpaceOp {
        let SpaceOp::Consume(op) = receive(channel, name, pattern) else {
            panic!("a receive is a consume");
        };
        SpaceOp::Consume(ConsumeOp { peek: true, ..op })
    }

    /// Added by decision 102 (plan v4 C5.6): a receive with the guard `guard`.
    fn guarded_receive(
        channel: &Par,
        name: &str,
        pattern: BindPattern,
        guard: Option<Par>,
    ) -> SpaceOp {
        let SpaceOp::Consume(op) = receive(channel, name, pattern) else {
            panic!("a receive is a consume");
        };
        SpaceOp::Consume(ConsumeOp { guard, ..op })
    }

    /// Added by decision 124 (L3): a join with the guard `guard`.
    fn guarded_join(binds: &[(&Par, BindPattern)], name: &str, guard: Option<Par>) -> SpaceOp {
        let SpaceOp::Join(op) = join(binds, name, false) else {
            panic!("a join is a join");
        };
        SpaceOp::Join(JoinOp { guard, ..op })
    }

    /// Added by decision 102 (plan v4 C5.6): the guard `x == value` over the first bound
    /// variable.
    fn equals(value: i64) -> Par {
        let bound = Par {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EVarBody(EVar {
                    v: Some(Var {
                        var_instance: Some(VarInstance::BoundVar(0)),
                    }),
                })),
            }],
            ..Par::default()
        };
        Par {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EEqBody(EEq {
                    p1: Some(bound),
                    p2: Some(int(value)),
                })),
            }],
            ..Par::default()
        }
    }

    /// Added by decision 124 (L3, J1 ruling 2): a fresh space of the discipline `inner` where the
    /// join `w` over the collections `s` and `t` parks, so an operation on `s` runs in the join
    /// engine. The patterns of `w` refuse every send of these tests. Each send of `stored` is
    /// stored, and the log starts empty.
    async fn joined_space(inner: SpaceInner, stored: &[SpaceOp]) -> (ParRSpace, SpaceEnv) {
        let (host, env) = space_of(inner).await;
        let (s, t) = (string("s"), string("t"));
        let parked = run(
            &env,
            join(&[(&s, exactly(-1)), (&t, exactly(-1))], "w", false),
        )
        .await;
        assert!(parked.fires.is_empty(), "the join parks");
        for op in stored {
            let outcome = run(&env, op.clone()).await;
            assert!(outcome.fires.is_empty(), "the send is stored");
        }
        host.take_event_log().await;
        (host, env)
    }

    /// Added by decision 102 (decision 119 G1) and decision 101 (rule CR-13): a fresh set space
    /// where the join `w` over the collections `s` and `t` parks, so a receive on `s` runs in
    /// the join engine. The patterns of `w` refuse every send of these tests. Each send of
    /// `stored` is stored, and the log starts empty.
    async fn joined_set(stored: &[SpaceOp]) -> (ParRSpace, SpaceEnv) {
        joined_space(SpaceInner::Set, stored).await
    }

    /// Added by decision 124 (L3, J1 ruling 2): runs `op` on the collection of `channel` with the
    /// discipline `D` in a scripted replay whose recorded row is `comms` for every source, and
    /// commits it in the play store.
    async fn run_replaying<D: Discipline>(
        env: &SpaceEnv,
        channel: &Par,
        comms: Option<Vec<COMM>>,
        op: SpaceOp,
    ) -> SpaceOutcome {
        let mut opened = begin_collection(&HashMapOuter, env, SPACE, &key_of(channel), channel)
            .await
            .expect("phase A");
        opened.txn = replaying(opened.txn, comms);
        prepare_collection_op(&GenericSpace::<HashMapOuter, D>::default(), env, opened, op)
            .await
            .expect("the operation stages")
            .commit()
            .await
            .expect("the operation commits")
    }

    /// Added by decision 102 (plan v4 C5.6): runs `op` on the set collection of `channel` in a
    /// scripted replay whose recorded row is `comms` for every source, and commits it in the
    /// play store.
    async fn run_replaying_set(
        env: &SpaceEnv,
        channel: &Par,
        comms: Option<Vec<COMM>>,
        op: SpaceOp,
    ) -> SpaceOutcome {
        run_replaying::<SetDiscipline>(env, channel, comms, op).await
    }

    /// Added by decision 102 (decision 119 G1, rule CR-12) and decision 101 (rule CR-13): a
    /// single-bind receive on a set collection that a join names runs in the join engine.
    /// Under every pick, its fire binds the first member of the class in DR-75 order over
    /// `[D]`, and one `K_retire` COMM over `[D]` retires each other member, in DR-75 order.
    #[tokio::test]
    async fn space_join_engine_set_receive_binds_the_first_member_and_retires_the_others() {
        let s = string("s");
        let mut stored = Vec::<SpaceOp>::with_capacity(4);
        let mut order = Vec::<Blake2b256Hash>::with_capacity(3);
        for seed in ["m1", "m2", "m3"] {
            let (op, hash) = seeded_send(&s, 1, seed, false);
            stored.push(op);
            order.push(hash);
        }
        stored.push(send(&s, 2, false));
        order.sort();
        let mut expected = Vec::<Vec<Blake2b256Hash>>::with_capacity(order.len());
        expected.extend(order.iter().map(|hash| vec![hash.clone()]));
        for _ in 0..PICK_RUNS {
            let (host, env) = joined_set(&stored).await;
            let outcome = run(&env, receive(&s, "k", exactly(1))).await;
            assert_eq!(fired(&outcome), vec![fire("k", &[])]);
            let log = host.take_event_log().await;
            let comms = comms_of(&log);
            for comm in &comms {
                assert_eq!(comm.consume.channel_hashes.len(), 1, "over [D]");
                assert!(comm.peeks.is_empty(), "no peek");
            }
            let bound: Vec<Vec<Blake2b256Hash>> =
                comms.iter().map(|comm| bound_sources(comm)).collect();
            assert_eq!(
                bound, expected,
                "the fire binds the first member in DR-75 order, then the others retire in DR-75 \
                 order"
            );
            assert_eq!(payloads(&host, &s).await, vec![vec![int(2)]]);
        }
    }

    /// Added by decision 101 (rule CR-13): a peek receive on a set collection that a join names
    /// binds one record over `[D]` and retires nothing. The transaction sends the record again
    /// (Rule E), so every record stays with its identity.
    #[tokio::test]
    async fn space_join_engine_set_peek_binds_one_record_and_retires_nothing() {
        let s = string("s");
        let mut stored = Vec::<SpaceOp>::with_capacity(4);
        for seed in ["m1", "m2", "m3"] {
            stored.push(seeded_send(&s, 1, seed, false).0);
        }
        stored.push(send(&s, 2, false));
        let (host, env) = joined_set(&stored).await;
        let mut before = host.get_data(&data_of(&s)).await;
        before.sort_by(|left, right| left.source.hash.cmp(&right.source.hash));
        let outcome = run(&env, peek_receive(&s, "p", exactly(1))).await;
        assert_eq!(fired(&outcome), vec![fire("p", &[])]);
        let log = host.take_event_log().await;
        let comms = comms_of(&log);
        let [comm] = comms.as_slice() else {
            panic!("one COMM, and no retire: {comms:?}");
        };
        assert_eq!(comm.consume.channel_hashes.len(), 1, "over [D]");
        assert_eq!(comm.produces.len(), 1, "one bind");
        assert!(!comm.peeks.is_empty(), "a peek");
        let mut after = host.get_data(&data_of(&s)).await;
        after.sort_by(|left, right| left.source.hash.cmp(&right.source.hash));
        assert_eq!(after, before);
    }

    /// Added by decision 102 (plan v4 C5.6, rule CR-7, base E3-a): in replay a receive of the
    /// join engine whose recorded row is Stored and whose commit may veto parks without a
    /// search, although a stored record passes its guard. Without the Stored row, or with a
    /// commit that cannot veto, the receive searches and fires. Play reads no row, so it
    /// searches.
    #[tokio::test]
    async fn space_join_engine_receive_parks_without_a_search_on_a_stored_row_that_may_veto() {
        let s = string("s");
        let cases: [(Option<Vec<COMM>>, Option<Par>, bool, bool); 4] = [
            (Some(Vec::new()), Some(equals(1)), true, true),
            (None, Some(equals(1)), true, false),
            (Some(Vec::new()), None, true, false),
            (None, Some(equals(1)), false, false),
        ];
        for (comms, guard, replay, parks) in cases {
            let (host, env) = joined_set(&[send(&s, 1, false)]).await;
            let op = guarded_receive(&s, "k", any(), guard.clone());
            let outcome = match replay {
                true => run_replaying_set(&env, &s, comms.clone(), op).await,
                false => run(&env, op).await,
            };
            let case = format!("row {comms:?}, guard {}, replay {replay}", guard.is_some());
            let waiting = host
                .get_waiting_continuations(vec![data_of(&s)])
                .await
                .len();
            match parks {
                true => {
                    assert!(outcome.fires.is_empty(), "{case}");
                    assert_eq!(waiting, 1, "{case}");
                    assert_eq!(payloads(&host, &s).await, vec![vec![int(1)]], "{case}");
                    let log = host.take_event_log().await;
                    assert!(comms_of(&log).is_empty(), "{case}: no search, so no fire");
                }
                false => {
                    assert_eq!(fired(&outcome), vec![fire("k", &[1])], "{case}");
                    assert_eq!(waiting, 0, "{case}");
                    assert!(payloads(&host, &s).await.is_empty(), "{case}");
                }
            }
        }
    }

    /// Added by decision 124 (L3, base E3-a, selection.rs:747-751; J1 ruling 2): in replay a send
    /// of the join engine whose recorded row is Stored skips each waiter whose commit may veto,
    /// before its search, with no bind count. So the send is stored and the log has no COMM,
    /// although the guard of the waiter passes. Without the row, in play, or with a waiter that
    /// cannot veto, the send searches and fires the waiter. The waiter is a one-bind receive on
    /// `s`, or a join on `s` and `t` with 2 stored on `t`. The space is a bag, which does not
    /// cascade, so the store ends the send.
    #[tokio::test]
    async fn space_join_engine_offer_skips_a_vetoing_join_waiter_on_a_stored_row() {
        let (s, t) = (string("s"), string("t"));
        let cases: [(usize, Option<Vec<COMM>>, bool, bool, bool); 8] = [
            (1, Some(Vec::new()), true, true, true),
            (2, Some(Vec::new()), true, true, true),
            (1, None, true, true, false),
            (2, None, true, true, false),
            (1, None, true, false, false),
            (2, None, true, false, false),
            (1, Some(Vec::new()), false, true, false),
            (2, Some(Vec::new()), false, true, false),
        ];
        for (binds, comms, guarded, replay, stores) in cases {
            let (stored, waiter, expected) = match binds {
                1 => {
                    let guard = match guarded {
                        true => Some(equals(1)),
                        false => None,
                    };
                    let waiter = guarded_receive(&s, "k", any(), guard);
                    (Vec::new(), waiter, fire("k", &[1]))
                }
                _ => {
                    // The variable of the last bind is 0, so the guard reads the value of t.
                    let guard = match guarded {
                        true => Some(equals(2)),
                        false => None,
                    };
                    let waiter = guarded_join(&[(&s, any()), (&t, any())], "v", guard);
                    (vec![send(&t, 2, false)], waiter, fire("v", &[1, 2]))
                }
            };
            let (host, env) = joined_space(SpaceInner::Bag, &stored).await;
            let parked = run(&env, waiter).await;
            assert!(parked.fires.is_empty(), "the waiter parks");
            host.take_event_log().await;
            let op = send(&s, 1, false);
            let outcome = match replay {
                true => run_replaying::<BagDiscipline>(&env, &s, comms.clone(), op).await,
                false => run(&env, op).await,
            };
            let case = format!("binds {binds}, row {comms:?}, guard {guarded}, replay {replay}");
            let log = host.take_event_log().await;
            match stores {
                true => {
                    assert!(
                        outcome.fires.is_empty(),
                        "{case}: fires {:?}, COMMs {}",
                        fired(&outcome),
                        comms_of(&log).len()
                    );
                    assert_eq!(payloads(&host, &s).await, vec![vec![int(1)]], "{case}");
                    assert!(comms_of(&log).is_empty(), "{case}: no search, so no fire");
                }
                false => {
                    assert_eq!(fired(&outcome), vec![expected], "{case}");
                    assert!(payloads(&host, &s).await.is_empty(), "{case}");
                    assert!(!comms_of(&log).is_empty(), "{case}: the fire is a COMM");
                }
            }
        }
    }

    // ── Added by decision 124 (F2, base one row source for each produce,
    // replay_rspace.rs:1058-1082): the Stored check of a PathMap offer over all its sources ──

    use models::rust::utils::{new_elist_par, new_gbool_par};

    use crate::rust::interpreter::spaces::outer::pathmap::PathMapOuter;

    /// Added by decision 124 (F2): the sources of the records on the data channel of the path of
    /// `channel` in a PathMap space, the home where `land` stores an element.
    async fn home_sources(host: &ParRSpace, channel: &Par) -> Vec<Produce> {
        let key = PathMapOuter.key(channel).expect("a path key");
        host.get_data(&PathMapOuter.data_channel(SPACE, &key))
            .await
            .into_iter()
            .map(|datum| datum.source)
            .collect()
    }

    /// Added by decision 124 (F2): a `pathmap:bag` space in which the waiter `w` peeks the path
    /// of `s`. Its guard is the literal `true`, so its commit may veto (match.rs:403-406), and
    /// the guard admits every value in play.
    async fn pathmap_peeked(s: &Par) -> (ParRSpace, SpaceEnv) {
        let (host, env) = space_of_outer(SpaceOuter::PathMap, SpaceInner::Bag).await;
        let SpaceOp::Consume(op) = peek_receive(s, "w", any()) else {
            panic!("a receive is a consume");
        };
        let guard = Some(new_gbool_par(true, Vec::new(), false));
        let parked = run(&env, SpaceOp::Consume(ConsumeOp { guard, ..op })).await;
        assert!(fired(&parked).is_empty(), "the peek waiter parks");
        host.take_event_log().await;
        (host, env)
    }

    /// Added by decision 124 (F2, base one row source for each produce,
    /// replay_rspace.rs:1058-1082): a PathMap send records its fire under the vehicle of the
    /// element on the fired prefix and its store under the element. Replay skips the waiter
    /// whose commit may veto only when the Stored row of the element comes before the row of
    /// each vehicle (`JoinSource::offer_stored`). The rows of the script list the journal order.
    /// With the fire row first, the waiter fires. With the Stored row first, the waiter is
    /// skipped and the element is stored. With no row, the waiter fires.
    #[tokio::test]
    async fn space_pathmap_offer_reads_the_first_row_over_its_sources() {
        let s = string("s");
        let path = new_elist_par(
            vec![s.clone(), int(1)],
            Vec::new(),
            false,
            None,
            Vec::new(),
            false,
        );
        let (host, env) = pathmap_peeked(&s).await;
        let played = run(&env, send(&path, 1, false)).await;
        let names: Vec<String> = fired(&played).into_iter().map(|(name, _)| name).collect();
        assert_eq!(names, vec!["w".to_string()], "the send fires w in play");
        let log = host.take_event_log().await;
        let comm = comms_of(&log)
            .into_iter()
            .find(|comm| !comm.peeks.is_empty())
            .expect("the fire of w is a peek COMM")
            .clone();
        let vehicle = comm
            .produces
            .first()
            .expect("the COMM binds the vehicle")
            .clone();
        let [element]: [Produce; 1] = home_sources(&host, &path)
            .await
            .try_into()
            .expect("the send stores x once");
        assert_ne!(vehicle, element, "the fire and the store have two sources");
        let fire_row = || ScriptedRow {
            source: ScriptedSource::Produce(vehicle.clone()),
            comms: Some(vec![comm.clone()]),
        };
        let store_row = || ScriptedRow {
            source: ScriptedSource::Produce(element.clone()),
            comms: Some(Vec::new()),
        };
        let cases: [(&str, Vec<ScriptedRow>, bool); 3] = [
            ("the fire row first", vec![fire_row(), store_row()], true),
            ("the Stored row first", vec![store_row(), fire_row()], false),
            ("no row", Vec::new(), true),
        ];
        for (case, rows, fires) in cases {
            let (host, env) = pathmap_peeked(&s).await;
            let outcome = run_replaying_rows::<PathMapOuter, BagDiscipline>(
                &env,
                &path,
                rows,
                send(&path, 1, false),
            )
            .await;
            let log = host.take_event_log().await;
            let binds_vehicle = comms_of(&log)
                .iter()
                .any(|logged| logged.produces.contains(&vehicle));
            let names: Vec<String> = fired(&outcome).into_iter().map(|(name, _)| name).collect();
            match fires {
                true => {
                    assert_eq!(names, vec!["w".to_string()], "{case}: w fires");
                    assert!(binds_vehicle, "{case}: the fire binds the vehicle");
                }
                false => {
                    assert!(names.is_empty(), "{case}: w is skipped: {names:?}");
                    assert!(!binds_vehicle, "{case}: no COMM binds the vehicle");
                }
            }
            assert_eq!(
                home_sources(&host, &path).await,
                vec![element.clone()],
                "{case}: x is stored"
            );
        }
    }

    // ── Decision 102 (plan v4 C5.3, rules CR-1, CR-3, CR-4 and F3): the picks of the windows ──

    /// Added by decision 102 (plan v4 C5.3): runs `op` on the collection of `channel` with the
    /// discipline `D` in a scripted replay that answers from the per-source rows `rows`, and
    /// commits it in the play store. Added by decision 124 (F2, base one row source for each
    /// produce, replay_rspace.rs:1058-1082): the agent has the outer store `S`. The transaction
    /// opens over the HashMap channels, as `FamilyAgent` opens it for every family
    /// (families.rs:546), because a PathMap operation derives its keys from its channel.
    async fn run_replaying_rows<S: OuterStore, D: Discipline>(
        env: &SpaceEnv,
        channel: &Par,
        rows: Vec<ScriptedRow>,
        op: SpaceOp,
    ) -> SpaceOutcome {
        let mut opened = begin_collection(&HashMapOuter, env, SPACE, &key_of(channel), channel)
            .await
            .expect("phase A");
        opened.txn = replaying_rows(opened.txn, rows);
        prepare_collection_op(&GenericSpace::<S, D>::default(), env, opened, op)
            .await
            .expect("the operation stages")
            .commit()
            .await
            .expect("the operation commits")
    }

    /// Added by decision 102 (plan v4 C5.3): the records of the collection of `channel` in the
    /// canonical order of DR-75, each with its produce and its payload.
    async fn dr75_records(host: &ParRSpace, channel: &Par) -> Vec<(Produce, Vec<Par>)> {
        let stored = host.get_data(&data_of(channel)).await;
        let order = canonical_data_order(&stored);
        let mut records = Vec::<(Produce, Vec<Par>)>::with_capacity(order.len());
        for index in order {
            let datum = stored.get(index).expect("a canonical index of the store");
            let payload = record::unwrap_pq_datum(&datum.a)
                .expect("a datum record")
                .datum
                .pars;
            records.push((datum.source.clone(), payload));
        }
        records
    }

    /// Added by decision 102 (plan v4 C5.3): the values of `fires` with the values of each fire
    /// sorted, for a fire whose bind order of one collection the test does not fix.
    fn sorted_values(fires: Vec<(String, Vec<Par>)>) -> Vec<(String, Vec<Par>)> {
        let mut sorted = Vec::<(String, Vec<Par>)>::with_capacity(fires.len());
        for (name, mut values) in fires {
            values.sort_by_key(|par| format!("{par:?}"));
            sorted.push((name, values));
        }
        sorted
    }

    /// Added by decision 102 (plan v4 C5.3, rules CR-1 and CR-4): a receive of need 1 on a bag
    /// with two matching records binds the record that the recorded COMM names, here the
    /// DR-75-second record. The per-source row of the consume of the receive names it. That
    /// consume is the consume of the same receive in play over the same records.
    #[tokio::test]
    async fn space_join_engine_receive_window_names_the_recorded_record_first() {
        let s = string("s");
        let stored = vec![send(&s, 1, false), send(&s, 2, false)];
        let (host, env) = joined_space(SpaceInner::Bag, &stored).await;
        let played = run(&env, receive(&s, "k", any())).await;
        assert_eq!(played.fires.len(), 1, "the receive fires in play");
        let log = host.take_event_log().await;
        let consume = comms_of(&log)
            .first()
            .expect("the fire is a COMM")
            .consume
            .clone();
        let (host, env) = joined_space(SpaceInner::Bag, &stored).await;
        let records = dr75_records(&host, &s).await;
        let [(_, first), (second, second_payload)] = records.as_slice() else {
            panic!("two records: {records:?}");
        };
        let rows = vec![ScriptedRow {
            source: ScriptedSource::Consume(consume.clone()),
            comms: Some(vec![recorded(consume, vec![second.clone()])]),
        }];
        let outcome = run_replaying_rows::<HashMapOuter, BagDiscipline>(
            &env,
            &s,
            rows,
            receive(&s, "k", any()),
        )
        .await;
        assert_eq!(fired(&outcome), vec![(
            "k".to_string(),
            second_payload.clone()
        )]);
        assert_eq!(payloads(&host, &s).await, vec![first.clone()]);
        let log = host.take_event_log().await;
        assert_eq!(
            comms_of(&log).first().map(|comm| bound_sources(comm)),
            Some(vec![second.hash.clone()])
        );
    }

    /// Added by decision 103 (decision 118 E1, rule CR-8): a join that names the bag collection
    /// `s` twice takes one deterministic list, the DR-75 order, in both roles. It binds the two
    /// DR-75-first records in play. In replay a COMM that names the DR-75-third record does not
    /// change the bound pair.
    #[tokio::test]
    async fn space_join_engine_repeated_collection_window_keeps_dr75_in_replay() {
        let s = string("s");
        let stored = vec![send(&s, 1, false), send(&s, 2, false), send(&s, 3, false)];
        for replay in [false, true] {
            let (host, env) = joined_space(SpaceInner::Bag, &stored).await;
            let records = dr75_records(&host, &s).await;
            let [(_, first), (_, second), (third, third_payload)] = records.as_slice() else {
                panic!("three records: {records:?}");
            };
            let op = join(&[(&s, any()), (&s, any())], "d", false);
            let outcome = match replay {
                true => {
                    let consume =
                        Consume::create(&vec![data_of(&s)], &vec![any()], &named("d"), false);
                    let comms = Some(vec![recorded(consume, vec![third.clone()])]);
                    run_replaying::<BagDiscipline>(&env, &s, comms, op).await
                }
                false => run(&env, op).await,
            };
            let mut pair = first.clone();
            pair.extend(second.iter().cloned());
            assert_eq!(
                sorted_values(fired(&outcome)),
                sorted_values(vec![("d".to_string(), pair)]),
                "replay {replay}"
            );
            assert_eq!(
                payloads(&host, &s).await,
                vec![third_payload.clone()],
                "replay {replay}"
            );
        }
    }

    /// Added by decision 102 (plan v4 C5.3, rules CR-1 and CR-4): two waiters of a bag can fire
    /// with a send. The per-source row of the produce of the send names the DR-75-second waiter,
    /// and it fires. That produce is the produce of the same send in play after the same parks.
    #[tokio::test]
    async fn space_join_engine_offer_meets_the_named_waiter_first() {
        let s = string("s");
        let names = ["k1", "k2"];
        let (host, env) = joined_space(SpaceInner::Bag, &[]).await;
        for name in names {
            let parked = run(&env, receive(&s, name, any())).await;
            assert!(parked.fires.is_empty(), "the receive parks");
        }
        host.take_event_log().await;
        run(&env, send(&s, 5, false)).await;
        let log = host.take_event_log().await;
        let produce = comms_of(&log)
            .first()
            .and_then(|comm| comm.produces.first())
            .expect("the send fires")
            .clone();
        let (host, env) = joined_space(SpaceInner::Bag, &[]).await;
        let mut consumes = Vec::<(&str, Consume)>::with_capacity(names.len());
        for name in names {
            let parked = run(&env, receive(&s, name, any())).await;
            assert!(parked.fires.is_empty(), "the receive parks");
            let waiting = host.get_waiting_continuations(vec![data_of(&s)]).await;
            let source = waiting
                .into_iter()
                .map(|waiter| waiter.source)
                .find(|source| consumes.iter().all(|(_, known)| known != source))
                .expect("the new waiter");
            consumes.push((name, source));
        }
        host.take_event_log().await;
        let waiting = host.get_waiting_continuations(vec![data_of(&s)]).await;
        let order = canonical_continuation_order(&waiting);
        let second = order
            .get(1)
            .and_then(|index| waiting.get(*index))
            .expect("the DR-75-second waiter")
            .source
            .clone();
        let (name, _) = consumes
            .iter()
            .find(|(_, source)| *source == second)
            .expect("a parked waiter");
        let rows = vec![ScriptedRow {
            source: ScriptedSource::Produce(produce.clone()),
            comms: Some(vec![recorded(second.clone(), vec![produce])]),
        }];
        let outcome =
            run_replaying_rows::<HashMapOuter, BagDiscipline>(&env, &s, rows, send(&s, 5, false))
                .await;
        assert_eq!(fired(&outcome), vec![fire(name, &[5])]);
        let log = host.take_event_log().await;
        assert_eq!(
            comms_of(&log).first().map(|comm| comm.consume.clone()),
            Some(second)
        );
    }

    /// Added by decision 102 (plan v4 C5.3, rules CR-1 and CR-3): in play a receive of need 1 on
    /// a bag picks its record at random. Over `PICK_RUNS` fresh spaces with two matching records,
    /// it binds each record in some run.
    #[tokio::test]
    async fn space_join_engine_play_window_pick_varies() {
        let s = string("s");
        let stored = vec![send(&s, 1, false), send(&s, 2, false)];
        let mut runs = Vec::<Vec<(String, Vec<Par>)>>::with_capacity(PICK_RUNS);
        for _ in 0..PICK_RUNS {
            let (_, env) = joined_space(SpaceInner::Bag, &stored).await;
            runs.push(fired(&run(&env, receive(&s, "k", any())).await));
        }
        for fires in &runs {
            assert!(
                *fires == vec![fire("k", &[1])] || *fires == vec![fire("k", &[2])],
                "the receive binds one record: {fires:?}"
            );
        }
        for value in [1, 2] {
            assert!(
                runs.contains(&vec![fire("k", &[value])]),
                "some run binds {value}"
            );
        }
    }

    /// Added by decision 102 (plan v4 C5.3, rule F3): the cascade search keeps DR-75 in play. The
    /// set collection `s` holds two classes, and the guarded join `w` over `t` and `s` admits
    /// only the DR-75-first record of `s`. A send on `t` offers x to `w`. The offer picks the
    /// window of `s` at random, so in some runs the guard refuses its tuple, and x is stored.
    /// The cascade then searches `w` over the DR-75 window of `s` and fires it after a lift. So
    /// `w` fires with the DR-75-first record in every one of `PICK_RUNS` play runs, and some
    /// runs fire it in the cascade.
    #[tokio::test]
    async fn cascade_search_keeps_dr75_windows() {
        let (s, t) = (string("s"), string("t"));
        let mut cascaded = 0usize;
        for _ in 0..PICK_RUNS {
            let (host, env) = space_of(SpaceInner::Set).await;
            for value in [1, 2] {
                run(&env, send(&s, value, false)).await;
            }
            let records = dr75_records(&host, &s).await;
            let [(_, first), (_, other)] = records.as_slice() else {
                panic!("two records: {records:?}");
            };
            let first_value = match *first == vec![int(1)] {
                true => 1,
                false => 2,
            };
            // The variable of the last bind is 0, so the guard reads the value of s.
            let waiter = guarded_join(&[(&t, any()), (&s, any())], "w", Some(equals(first_value)));
            let parked = run(&env, waiter).await;
            assert!(parked.fires.is_empty(), "t holds no record, so w parks");
            host.take_event_log().await;
            let outcome = run(&env, send(&t, 5, false)).await;
            assert_eq!(fired(&outcome), vec![fire("w", &[5, first_value])]);
            assert_eq!(payloads(&host, &s).await, vec![other.clone()]);
            let log = host.take_event_log().await;
            cascaded += usize::from(comms_of(&log).len() >= 2);
        }
        assert!(
            cascaded >= 1,
            "some run fires w in the cascade, after a lift"
        );
    }

    /// Added by decision 103 (decision 118 E1, rule CR-8): `x_first` moves the element in flight
    /// to the front and keeps the order of the other entries.
    #[test]
    fn space_join_engine_x_first_moves_the_element_in_flight_to_the_front() {
        let stored = |index: usize| Entry {
            at: EntryAt::Stored(index),
            level: None,
        };
        let incoming = Entry {
            at: EntryAt::Incoming,
            level: None,
        };
        assert_eq!(x_first(vec![stored(2), stored(0), incoming]), vec![
            incoming,
            stored(2),
            stored(0)
        ]);
        assert_eq!(x_first(vec![stored(1), incoming, stored(3)]), vec![
            incoming,
            stored(1),
            stored(3)
        ]);
        assert_eq!(x_first(vec![incoming, stored(4)]), vec![
            incoming,
            stored(4)
        ]);
        assert_eq!(x_first(vec![stored(1), stored(3)]), vec![
            stored(1),
            stored(3)
        ]);
        assert_eq!(x_first(Vec::new()), Vec::<Entry>::new());
    }

    // ── Decision 118 (C6, rule (a)): the claims of persistent entries ─────────────────────────

    /// Added by decision 103 (decision 118, rule CR-9): sends of `value` on the set collection
    /// `channel`, one for each persistence of `persists`, whose records follow each other in the
    /// canonical order of DR-75, each with the source hash of its record.
    fn ordered_class(
        channel: &Par,
        value: i64,
        persists: &[bool],
    ) -> Vec<(SpaceOp, Blake2b256Hash)> {
        let mut sends = Vec::<(SpaceOp, Blake2b256Hash)>::with_capacity(persists.len());
        for (member, &persistent) in persists.iter().enumerate() {
            let send = (0..)
                .map(|seed| seeded_send(channel, value, &format!("m{member}-{seed}"), persistent))
                .find(|(_, hash)| sends.last().is_none_or(|(_, last)| last < hash))
                .expect("a seed orders the member after the member before");
            sends.push(send);
        }
        sends
    }

    /// Added by decision 103 (decision 118, rule CR-9): the source hashes of the records of the
    /// collection of `channel`, sorted.
    async fn stored_sources(host: &ParRSpace, channel: &Par) -> Vec<Blake2b256Hash> {
        let mut sources: Vec<Blake2b256Hash> = host
            .get_data(&data_of(channel))
            .await
            .into_iter()
            .map(|datum| datum.source.hash)
            .collect();
        sources.sort();
        sources
    }

    /// Added by decision 103 (decision 118, rule CR-9, decision-c6 §6.13): a later position of
    /// the search skips a non-persistent member of a claimed set class only when the class has no
    /// persistent member. One class holds [a1, a2!!] or [a1, a2, a3!!] in the canonical order of
    /// DR-75. The join [c, c] keeps the DR-75 window (E1), and it binds the window entries
    /// Stored(0) and Stored(1): a1 and a2. No rig of this module builds an engine with its
    /// transaction context, so the test reads the bound entries from the COMM of the fire.
    #[tokio::test]
    async fn search_claimed_class_skips_only_non_persistent_members() {
        for persists in [&[false, true][..], &[false, false, true][..]] {
            let (host, env) = space_of(SpaceInner::Set).await;
            let c = string("c");
            let members = ordered_class(&c, 1, persists);
            for (op, _) in &members {
                run(&env, op.clone()).await;
            }
            let window: Vec<Blake2b256Hash> = dr75_records(&host, &c)
                .await
                .into_iter()
                .map(|(produce, _)| produce.hash)
                .collect();
            let expected: Vec<Blake2b256Hash> =
                members.iter().map(|(_, hash)| hash.clone()).collect();
            assert_eq!(window, expected, "the members follow each other in DR-75");
            host.take_event_log().await;
            let outcome = run(&env, join(&[(&c, any()), (&c, any())], "w", false)).await;
            assert_eq!(fired(&outcome), vec![fire("w", &[1, 1])]);
            let log = host.take_event_log().await;
            let comms = comms_of(&log);
            let [comm] = comms.as_slice() else {
                panic!("one COMM and no K_retire, not {} COMMs", comms.len());
            };
            assert_eq!(
                bound_sources(comm),
                window[..2].to_vec(),
                "the search binds Stored(0) and Stored(1) of the window"
            );
        }
    }

    /// Added by decision 103 (decision 118, rule CR-9, C6.5, decision-c6 §6.13): a peek fire
    /// sends a persistent element in flight again once, also when it binds two positions. The
    /// peek join [(a, any), (b, any), (a, any)] over the stored 2 on b parks. The persistent 1
    /// on a binds both positions of a. The second sends then store 1 once on a and 2 once on b.
    #[tokio::test]
    async fn second_sends_send_a_persistent_incoming_once() {
        let (host, env) = space_of(SpaceInner::Bag).await;
        let (a, b) = (string("a"), string("b"));
        run(&env, send(&b, 2, false)).await;
        let parked = run(
            &env,
            join(&[(&a, any()), (&b, any()), (&a, any())], "p", true),
        )
        .await;
        assert!(parked.fires.is_empty());
        let before_b = host.get_data(&data_of(&b)).await;
        let outcome = run(&env, send(&a, 1, true)).await;
        assert_eq!(fired(&outcome), vec![fire("p", &[1, 2, 1])]);
        let records = host.get_data(&data_of(&a)).await;
        let [record] = records.as_slice() else {
            panic!("a holds the element once, not {} records", records.len());
        };
        assert!(record.persist, "the element stays persistent");
        assert_eq!(payloads(&host, &a).await, vec![vec![int(1)]]);
        assert_eq!(
            host.get_data(&data_of(&b)).await,
            before_b,
            "b holds its record once, with its identity"
        );
    }

    /// Added by decision 103 (decision 118, rule CR-9, decision-c6 §6.13): a1 and the persistent
    /// a2 hold 1 in one class of a set, in this canonical order. The join [s, s] fires with a1
    /// and a2. The set then holds a2 only, and no `K_retire` occurs.
    #[tokio::test]
    async fn space_join_engine_set_free_class_binds_the_member_and_the_persistent_member() {
        let (host, env) = space_of(SpaceInner::Set).await;
        let s = string("s");
        let members = ordered_class(&s, 1, &[false, true]);
        for (op, _) in &members {
            run(&env, op.clone()).await;
        }
        host.take_event_log().await;
        let outcome = run(&env, join(&[(&s, any()), (&s, any())], "w", false)).await;
        assert_eq!(fired(&outcome), vec![fire("w", &[1, 1])]);
        let log = host.take_event_log().await;
        let comms = comms_of(&log);
        let [comm] = comms.as_slice() else {
            panic!("one COMM and no K_retire, not {} COMMs", comms.len());
        };
        let expected: Vec<Blake2b256Hash> = members.iter().map(|(_, hash)| hash.clone()).collect();
        assert_eq!(bound_sources(comm), expected);
        assert_eq!(stored_sources(&host, &s).await, vec![members[1].1.clone()]);
    }

    /// Added by decision 103 (decision 118, rule CR-9, decision-c6 §6.13): the regression of
    /// F-118-2. The setup of the test before, with a peek join. The fire stages with no
    /// `NotInView`, and its COMM binds a1 and a2. The set then holds a1 and a2, once each.
    #[tokio::test]
    async fn space_join_engine_set_peek_join_on_a_free_class_binds_two_records() {
        let (host, env) = space_of(SpaceInner::Set).await;
        let s = string("s");
        let members = ordered_class(&s, 1, &[false, true]);
        for (op, _) in &members {
            run(&env, op.clone()).await;
        }
        host.take_event_log().await;
        let outcome = run(&env, join(&[(&s, any()), (&s, any())], "p", true)).await;
        assert_eq!(fired(&outcome), vec![fire("p", &[1, 1])]);
        let log = host.take_event_log().await;
        let comms = comms_of(&log);
        let [comm] = comms.as_slice() else {
            panic!("one COMM, not {}", comms.len());
        };
        let mut expected: Vec<Blake2b256Hash> =
            members.iter().map(|(_, hash)| hash.clone()).collect();
        assert_eq!(bound_sources(comm), expected);
        expected.sort();
        assert_eq!(stored_sources(&host, &s).await, expected);
    }

    /// Added by decision 103 (decision 118, rule CR-9, C6.5, decision-c6 §6.13): the peek join
    /// [(a, any), (a, any)] parks on a bag. The persistent send of 1 fires it with [1, 1]. The
    /// element is sent again once, so a then holds it once.
    #[tokio::test]
    async fn space_join_engine_peek_join_sends_a_persistent_element_once() {
        let (host, env) = space_of(SpaceInner::Bag).await;
        let a = string("a");
        let parked = run(&env, join(&[(&a, any()), (&a, any())], "p", true)).await;
        assert!(parked.fires.is_empty());
        let outcome = run(&env, send(&a, 1, true)).await;
        assert_eq!(fired(&outcome), vec![fire("p", &[1, 1])]);
        let records = host.get_data(&data_of(&a)).await;
        let [record] = records.as_slice() else {
            panic!("a holds the element once, not {} records", records.len());
        };
        assert!(record.persist, "the element stays persistent");
        assert_eq!(payloads(&host, &a).await, vec![vec![int(1)]]);
    }

    // ── Added by decision 124 (plan v2 C7.6 and C7.10, plan v4 C7.8): the seal of a lift, the
    // inert machinery waiters and the book ──────────────────────────────────────────────────────

    // Added by decision 124 (plan v2 C7.6 and C7.10): the seal test builds an authority with a
    // region and its residue and reads the consume events of the log, and the reader test parks
    // an inert `K_retire`.
    use models::rhoapi::cost_signature::Value as CostSignatureValue;
    use models::rhoapi::{CostRegion, CostSignature};
    use rspace_plus_plus::rspace::trace::event::IOEvent;

    use crate::rust::interpreter::accounting::authority::system_residue_authority_with_caller;
    use crate::rust::interpreter::spaces::disciplines::scan::retire_pattern;

    /// Added by decision 124 (plan v2 C7.6, decision 99, RC2v2 M-1, C2v3 2.14): the `K_lift` of
    /// a cascade lift carries the machinery seal of the cost authority of the operation: the
    /// system residue of the authority under the deploy id of the budget
    /// (`system_residue_authority_with_caller`). The authority has a region, so its seal is not
    /// the authority itself. The setup is the one of
    /// `space_join_engine_cascade_fires_an_exposed_head`: the send on q1 fires w with the head 10
    /// of q2, and the cascade lifts 20 with the step 1 and the counter value of the bump of q2.
    #[tokio::test]
    async fn join_lift_takes_the_seal() {
        let (host, env) = space_of(SpaceInner::Queue).await;
        let (q1, q2) = (string("q1"), string("q2"));
        run(&env, send(&q2, 10, false)).await;
        run(&env, send(&q2, 20, false)).await;
        run(&env, join(&[(&q1, any()), (&q2, any())], "w", false)).await;
        run(&env, receive(&q2, "v", any())).await;
        let counter_channel =
            namespace::gprivate(namespace::collection_counter(SPACE, &key_of(&q2)));
        let counter = match host.get_data(&counter_channel).await.as_slice() {
            [datum] => record::counter_value(&datum.a).expect("a counter record"),
            other => panic!("the counter channel holds {} records", other.len()),
        };
        let authority = CostAuthority {
            regions: vec![CostRegion {
                instance_id: vec![7; 32],
                signature: Some(CostSignature {
                    value: Some(CostSignatureValue::Ground(vec![7])),
                }),
                price: 0,
                caller: None,
            }],
        };
        let SpaceOp::Produce(op) = send(&q1, 1, false) else {
            panic!("a send is a produce");
        };
        let op = ProduceOp {
            data: ListParWithRandom {
                cost_authority: Some(authority.clone()),
                ..op.data
            },
            ..op
        };
        host.take_event_log().await;
        run(&env, SpaceOp::Produce(op.clone())).await;
        let seal = system_residue_authority_with_caller(&authority, &env.budget.deploy_id(), None)
            .expect("the seal of a region with a signature builds");
        let random =
            RandomPurpose::Lift.derive(&Blake2b512Random::from_bytes(&op.data.random_state));
        let lift = Consume::create(
            &vec![data_of(&q2)],
            &vec![lift_pattern()],
            &lift_continuation(counter, 1, &random, Some(seal)),
            false,
        );
        let log = host.take_event_log().await;
        let lifts: Vec<&Consume> = log
            .iter()
            .filter_map(|event| match event {
                Event::Comm(_) | Event::IoEvent(IOEvent::Produce(_)) => None,
                Event::IoEvent(IOEvent::Consume(consume)) => Some(consume),
            })
            .filter(|consume| consume.channel_hashes == lift.channel_hashes)
            .collect();
        assert_eq!(lifts, vec![&lift], "the K_lift of 20 carries the seal");
    }

    // Disabled by decision 132: the user withdrew plan v2 C7.12, so the resume of the join
    // engine re-issues a send that is not persistent with a fresh ord again (decision 43). This
    // test pinned the rule of C7.12.

    /// Added by decision 124 (plan v2 C7.10, decision 100, C2v3 3.2): the J case of the reader
    /// test of C2v3 3.2 to 3.4. A `K_retire` and a `K_lift` wait on the collection of s before
    /// the send, as a refused machinery row parks them (CR-16). `union_order` skips both, so the
    /// send of 1 meets only the join w, which refuses it, and the bag stores it. Both inert
    /// waiters stay.
    #[tokio::test]
    async fn readers_skip_an_inert_machinery_waiter() {
        let s = string("s");
        let (host, env) = joined_space(SpaceInner::Bag, &[]).await;
        let random = Blake2b512Random::create_from_bytes(b"inert machinery");
        let inert = [
            (
                retire_pattern(),
                retire_continuation(&Blake2b256Hash::new(b"retired"), &random, None),
            ),
            (lift_pattern(), lift_continuation(0, 1, &random, None)),
        ];
        for (pattern, continuation) in inert {
            host.consume(
                vec![data_of(&s)],
                vec![pattern],
                continuation,
                false,
                BTreeSet::new(),
            )
            .await
            .expect("the inert waiter parks");
        }
        let outcome = run(&env, send(&s, 1, false)).await;
        assert!(
            outcome.fires.is_empty(),
            "no reader offers the send to an inert waiter"
        );
        assert_eq!(
            payloads(&host, &s).await,
            vec![vec![int(1)]],
            "the bag stores the send"
        );
        let waiting = host
            .get_waiting_continuations(vec![data_of(&s)])
            .await
            .len();
        assert_eq!(waiting, 2, "both inert waiters stay");
    }

    /// Added by decision 124 (plan v4 C7.8, rule CR-20, C2v3 4.7 (f)): the cascade of the join
    /// engine does not fire a pair whose stored record the book holds with the waiter. The setup
    /// is the one of `space_join_engine_cascade_fires_an_exposed_head`, and the book holds the
    /// pair of 20 and v before the send on q1. The send fires w with 10, and the cascade skips v,
    /// so 20 stays stored and v stays parked.
    #[tokio::test]
    async fn join_cascade_skips_a_booked_pair() {
        let (host, env) = space_of(SpaceInner::Queue).await;
        let (q1, q2) = (string("q1"), string("q2"));
        run(&env, send(&q2, 10, false)).await;
        run(&env, send(&q2, 20, false)).await;
        run(&env, join(&[(&q1, any()), (&q2, any())], "w", false)).await;
        run(&env, receive(&q2, "v", any())).await;
        let twenty = host
            .get_data(&data_of(&q2))
            .await
            .into_iter()
            .find(|datum| {
                record::unwrap_pq_datum(&datum.a)
                    .expect("a datum record")
                    .datum
                    .pars
                    == vec![int(20)]
            })
            .expect("q2 holds 20")
            .source;
        let waiting = host.get_waiting_continuations(vec![data_of(&q2)]).await;
        let [v] = waiting.as_slice() else {
            panic!("v waits on q2 alone, not {} waiters", waiting.len());
        };
        env.volatile
            .book_pairs([(twenty.hash, v.source.hash.clone())]);
        let outcome = run(&env, send(&q1, 1, false)).await;
        assert_eq!(
            fired(&outcome),
            vec![fire("w", &[1, 10])],
            "the cascade skips v"
        );
        assert_eq!(
            payloads(&host, &q2).await,
            vec![vec![int(20)]],
            "20 stays stored"
        );
        let waiting = host
            .get_waiting_continuations(vec![data_of(&q2)])
            .await
            .len();
        assert_eq!(waiting, 1, "v stays parked");
    }

    /// Added by decision 124 (plan v4 C7.8, rule CR-20, ruling OP-F13, ADD A1.13 (b)): an offer
    /// of the join engine skips a stored waiter when the book holds its pair with the element or
    /// with a stored record of its tuple. The join W, with the body v, waits on s and t, and t
    /// holds r. With (x, W) booked, the send x on s does not fire W, and the bag stores x. With
    /// (r, W) booked, the same. A probe space with an empty book fires W with x and r, and its
    /// COMM gives the sources of x and W.
    #[tokio::test]
    async fn join_offer_skips_a_booked_pair() {
        let (s, t) = (string("s"), string("t"));
        let stored = [send(&t, 2, false)];
        let waiter = || join(&[(&s, any()), (&t, any())], "v", false);
        let (probe, probe_env) = joined_space(SpaceInner::Bag, &stored).await;
        run(&probe_env, waiter()).await;
        let [r]: [Produce; 1] = probe
            .get_data(&data_of(&t))
            .await
            .into_iter()
            .map(|datum| datum.source)
            .collect::<Vec<Produce>>()
            .try_into()
            .expect("t holds r once");
        probe.take_event_log().await;
        run(&probe_env, send(&s, 1, false)).await;
        let log = probe.take_event_log().await;
        let comm = comms_of(&log)
            .into_iter()
            .find(|comm| comm.consume.channel_hashes.len() == 2)
            .expect("the probe fires W");
        let x = comm
            .produces
            .iter()
            .find(|produce| produce.hash != r.hash)
            .expect("the COMM binds x")
            .hash
            .clone();
        let w = comm.consume.hash.clone();
        for (pair, case) in [((x, w.clone()), "(x, W)"), ((r.hash, w), "(r, W)")] {
            let (host, env) = joined_space(SpaceInner::Bag, &stored).await;
            run(&env, waiter()).await;
            env.volatile.book_pairs([pair]);
            let outcome = run(&env, send(&s, 1, false)).await;
            assert!(outcome.fires.is_empty(), "{case}: the offer skips W");
            assert_eq!(
                payloads(&host, &s).await,
                vec![vec![int(1)]],
                "{case}: the bag stores x"
            );
        }
    }

    /// Added by decision 124 (plan v4 C7.8, rule CR-20, ruling OP-F13, ADD A1.13 (c)): a receive
    /// of the join engine parks when the book holds the pair of the stored record that its
    /// search binds and its consume. s holds r, and the book holds (r, C) for the consume C of
    /// the receive. A probe space with an empty book fires the same receive with r, and its COMM
    /// gives C.
    #[tokio::test]
    async fn join_receive_parks_on_a_booked_pair() {
        let s = string("s");
        let stored = [send(&s, 1, false)];
        let (probe, probe_env) = joined_space(SpaceInner::Bag, &stored).await;
        let [r]: [Produce; 1] = probe
            .get_data(&data_of(&s))
            .await
            .into_iter()
            .map(|datum| datum.source)
            .collect::<Vec<Produce>>()
            .try_into()
            .expect("s holds r once");
        run(&probe_env, receive(&s, "k", any())).await;
        let log = probe.take_event_log().await;
        let consume = comms_of(&log)
            .into_iter()
            .find(|comm| comm.produces.iter().any(|produce| produce.hash == r.hash))
            .expect("the probe fires the receive with r")
            .consume
            .hash
            .clone();
        let (host, env) = joined_space(SpaceInner::Bag, &stored).await;
        env.volatile.book_pairs([(r.hash, consume)]);
        let outcome = run(&env, receive(&s, "k", any())).await;
        assert!(outcome.fires.is_empty(), "the receive does not fire with r");
        let waiting = host
            .get_waiting_continuations(vec![data_of(&s)])
            .await
            .len();
        assert_eq!(waiting, 1, "the receive parks");
    }

    /// Added by decision 124 (plan v4 C7.8, rule CR-20, rulings OP-F13 and RAMD 1.5, ADD
    /// A1.13 (d)): a fresh `hashmap:vectordb` space of two dimensions where the join w over the
    /// collections s and t parks, so a receive on s runs in the join engine, as `joined_space`
    /// builds it for the other disciplines. A VectorDB config needs its params `capacity` and
    /// `dimensions` (`vectordb::vdb_config`), which `space_of_outer` does not set. The patterns
    /// of w refuse every send of the test, and the log starts empty.
    async fn joined_vectordb_space() -> (ParRSpace, SpaceEnv) {
        let mut stores = InMemoryStoreManager::new();
        let host = ParRSpace::create(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(Matcher)),
        )
        .expect("an in-memory space");
        let env = SpaceEnv {
            store: SpaceStoreHandle::new(host.clone()),
            budget: RuntimeBudget::new(Cost::create(10_000_000, "join engine test")),
            volatile: Default::default(),
            origin: Default::default(),
        };
        let family = SpaceFactoryUrn::all()
            .find(|urn| urn.outer == SpaceOuter::HashMap && urn.inner == SpaceInner::VectorDb)
            .expect("a factory family");
        let mut config = SpaceConfig::new(
            SPACE.sid,
            SpaceOuter::HashMap.as_str(),
            SpaceInner::VectorDb.as_str(),
            SpaceQualifier::Default,
        );
        config.params.insert("capacity".to_string(), int(100));
        config.params.insert("dimensions".to_string(), int(2));
        FamilyAgent
            .prepare_create(&env, SpaceCreation {
                space: SPACE,
                config: config.to_par(),
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
        let (s, t) = (string("s"), string("t"));
        let parked = run(
            &env,
            join(&[(&s, exactly(-1)), (&t, exactly(-1))], "w", false),
        )
        .await;
        assert!(parked.fires.is_empty(), "the join parks");
        host.take_event_log().await;
        (host, env)
    }

    /// Added by decision 124 (plan v4 C7.8, rule CR-20, rulings OP-F13 and RAMD 1.5, ADD
    /// A1.13 (d)): a list receive on a VectorDB collection of the join engine parks when the book
    /// holds the pair of a member of its list and its consume. s holds a = [1, 0] and b = [1, 1],
    /// the list of the query [1, 0] with k 2 is [a, b], and the book holds the pair of the second
    /// member b. A probe space with an empty book fires the same receive over the same records,
    /// and its first COMM gives the consume.
    #[tokio::test]
    async fn join_receive_list_parks_on_a_booked_member() {
        let s = string("s");
        let vector = |values: [f64; 2]| {
            let mut elements = Vec::<Par>::with_capacity(values.len());
            for value in values {
                elements.push(Par {
                    exprs: vec![Expr {
                        expr_instance: Some(ExprInstance::GDouble(value.to_bits())),
                    }],
                    ..Par::default()
                });
            }
            new_elist_par(elements, Vec::new(), false, None, Vec::new(), false)
        };
        let vector_send = |values: [f64; 2], value: i64| {
            let SpaceOp::Produce(op) = send(&s, value, false) else {
                panic!("a send is a produce");
            };
            SpaceOp::Produce(ProduceOp {
                data: ListParWithRandom {
                    pars: vec![vector(values)],
                    ..op.data
                },
                ..op
            })
        };
        let list_receive = || {
            let SpaceOp::Consume(op) = receive(&s, "k", any()) else {
                panic!("a receive is a consume");
            };
            SpaceOp::Consume(ConsumeOp {
                query: Some(QueryParams {
                    payload: vec![vector([1.0, 0.0])],
                    named: BTreeMap::from([("k".to_string(), int(2))]),
                }),
                ..op
            })
        };
        let (probe, probe_env) = joined_vectordb_space().await;
        run(&probe_env, vector_send([1.0, 0.0], 1)).await;
        run(&probe_env, vector_send([1.0, 1.0], 2)).await;
        probe.take_event_log().await;
        run(&probe_env, list_receive()).await;
        let log = probe.take_event_log().await;
        let consume = comms_of(&log)
            .first()
            .expect("the probe fires the receive")
            .consume
            .hash
            .clone();
        let (host, env) = joined_vectordb_space().await;
        run(&env, vector_send([1.0, 0.0], 1)).await;
        run(&env, vector_send([1.0, 1.0], 2)).await;
        let second = host
            .get_data(&data_of(&s))
            .await
            .into_iter()
            .find(|datum| {
                record::unwrap_vdb_datum(&datum.a)
                    .expect("a VectorDB record")
                    .datum
                    .pars
                    == vec![vector([1.0, 1.0])]
            })
            .expect("s holds b")
            .source
            .hash;
        env.volatile.book_pairs([(second, consume)]);
        let outcome = run(&env, list_receive()).await;
        assert!(
            outcome.fires.is_empty(),
            "b is a booked member of the list [a, b]"
        );
        let waiting = host
            .get_waiting_continuations(vec![data_of(&s)])
            .await
            .len();
        assert_eq!(waiting, 1, "the receive parks");
    }

    // ── Decision 124: the row searches and the tail end of the join engine ─────────────────

    // Added by decision 124 (ruling RADD2v4 7.7): the item test of a row request.
    use rspace_plus_plus::rspace::space_store_handle::RowItemTest;

    // Added by decision 124 (T-C4-24): the claims of the jobs of a `seq` space.
    use crate::rust::interpreter::reified::seq::{self as seq_history, SeqHistory};
    use crate::rust::interpreter::spaces::agent::ParSpaceTxn;
    // Added by decision 124 (rulings RADD2v4 6.1, 7.7 and 7.10): the capture of the staged
    // operations, the owner-funded env of a plan, and the observer and the probe of a commit.
    use crate::rust::interpreter::spaces::test_support::{
        capturing_ops, owner_funded, CapturedOp, Funded, ListsProbe,
    };

    /// Added by decision 124 (ruling RADD2v4 6.1): stages `op` on the collection of `channel` of
    /// the space `space` with the discipline `D`, over the transaction `wrap` of play that keeps
    /// the row search and the fields of each stage call. It drops the staged work, so nothing
    /// commits, and returns the searches and the staged operations, each in staging order.
    async fn staged_rows_over<D: Discipline>(
        env: &SpaceEnv,
        space: NativeSpace,
        channel: &Par,
        wrap: impl FnOnce(ParSpaceTxn) -> ParSpaceTxn,
        op: SpaceOp,
    ) -> (Vec<ParRowSearch>, Vec<CapturedOp>) {
        let mut opened = begin_collection(&HashMapOuter, env, space, &key_of(channel), channel)
            .await
            .expect("phase A");
        let (kept_searches, searches) = std::sync::mpsc::channel::<ParRowSearch>();
        let (kept_ops, ops) = std::sync::mpsc::channel::<CapturedOp>();
        opened.txn = capturing_ops(wrap(opened.txn), kept_searches, kept_ops);
        let prepared =
            prepare_collection_op(&GenericSpace::<HashMapOuter, D>::default(), env, opened, op)
                .await
                .expect("the operation stages");
        drop(prepared);
        (searches.try_iter().collect(), ops.try_iter().collect())
    }

    /// Added by decision 124 (ruling RADD2v4 6.1): `staged_rows_over` in play.
    async fn staged_rows<D: Discipline>(
        env: &SpaceEnv,
        space: NativeSpace,
        channel: &Par,
        op: SpaceOp,
    ) -> (Vec<ParRowSearch>, Vec<CapturedOp>) {
        staged_rows_over::<D>(env, space, channel, |txn| txn, op).await
    }

    /// Added by decision 124 (ruling RADD2v4 6.1): runs `stage` in a plan of play over the join
    /// scope of `op` on the collection of `channel`, after phase B over the scope, with a
    /// transaction that keeps the fields of each stage call. It drops the staged work, so
    /// nothing commits, and returns the operations that `stage` staged, in order.
    async fn join_plan<D: Discipline>(
        env: &SpaceEnv,
        channel: &Par,
        op: &SpaceOp,
        mut stage: impl FnMut(&mut Cx<'_, '_>, &JoinScope) -> Result<(), SpaceError>,
    ) -> Vec<CapturedOp> {
        let mut opened = begin_collection(&HashMapOuter, env, SPACE, &key_of(channel), channel)
            .await
            .expect("phase A");
        let route = classify(&GenericSpace::<HashMapOuter, D>::default(), &mut opened, op)
            .expect("the operation classifies");
        let JoinRoute::Join(scope) = route else {
            panic!("the operation does not run in the join engine");
        };
        env.lock_phase_b(&mut opened.txn, scope.lock_keys())
            .await
            .expect("phase B");
        let CollectionTxn { txn, config, .. } = opened;
        let (kept_searches, _searches) = std::sync::mpsc::channel::<ParRowSearch>();
        let (kept_ops, ops) = std::sync::mpsc::channel::<CapturedOp>();
        let txn = capturing_ops(txn, kept_searches, kept_ops);
        let data = data_of(channel);
        let authority = CostAuthority::default();
        let admission = Admission::of(&config, op).expect("the admission of the operation");
        let job = SpaceJob::of(op);
        let mut staged = Vec::<CapturedOp>::new();
        let prepared = generic::run_plan(
            env,
            txn,
            &data,
            &config,
            &authority,
            None,
            admission,
            &job,
            false,
            |cx| {
                stage(cx, &scope)?;
                staged.extend(ops.try_iter());
                Ok(None)
            },
        )
        .expect("the plan stages");
        drop(prepared);
        staged
    }

    /// The fields of a staged row that name it: its channels, its patterns, its continuation,
    /// whether it persists and whether it is the own row of its job.
    type RetireFields = (Vec<Par>, Vec<BindPattern>, TaggedContinuation, bool, bool);

    /// Added by decision 124 (ruling RADD 1.5): the `K_retire` rows of the staged operations
    /// `ops`, each with the fields that name it, in staging order. The outcome stays out,
    /// because the place of the record in the staged view moves with each retire before it.
    fn retire_fields(ops: &[CapturedOp]) -> Vec<RetireFields> {
        let retire = [retire_pattern()];
        let mut rows = Vec::<RetireFields>::with_capacity(ops.len());
        for op in ops {
            let CapturedOp::Take {
                channels,
                patterns,
                continuation,
                persist,
                own,
                ..
            } = op
            else {
                continue;
            };
            match patterns.as_slice() == retire.as_slice() {
                false => continue,
                true => rows.push((
                    channels.clone(),
                    patterns.clone(),
                    continuation.clone(),
                    *persist,
                    *own,
                )),
            }
        }
        rows
    }

    /// The list value of the vector `values`.
    fn vector_par(values: [f64; 2]) -> Par {
        let mut elements = Vec::<Par>::with_capacity(values.len());
        for value in values {
            elements.push(Par {
                exprs: vec![Expr {
                    expr_instance: Some(ExprInstance::GDouble(value.to_bits())),
                }],
                ..Par::default()
            });
        }
        new_elist_par(elements, Vec::new(), false, None, Vec::new(), false)
    }

    /// A send of the vector `values` on the VectorDB collection of `channel`, with the identity
    /// of a send of `value`.
    fn vector_send(channel: &Par, values: [f64; 2], value: i64, persistent: bool) -> SpaceOp {
        let SpaceOp::Produce(op) = send(channel, value, persistent) else {
            panic!("a send is a produce");
        };
        SpaceOp::Produce(ProduceOp {
            data: ListParWithRandom {
                pars: vec![vector_par(values)],
                ..op.data
            },
            ..op
        })
    }

    /// A list receive on the VectorDB collection of `channel` with the query `[1, 0]` and the
    /// length 2, as in `join_receive_list_parks_on_a_booked_member`.
    fn list_receive(channel: &Par, name: &str) -> SpaceOp {
        let SpaceOp::Consume(op) = receive(channel, name, any()) else {
            panic!("a receive is a consume");
        };
        SpaceOp::Consume(ConsumeOp {
            query: Some(QueryParams {
                payload: vec![vector_par([1.0, 0.0])],
                named: BTreeMap::from([("k".to_string(), int(2))]),
            }),
            ..op
        })
    }

    /// Added by decision 124 (plan v4 C7.13, ADD A1.12): a fresh `hashmap:vectordb` space of two
    /// dimensions with the param `mirror`, so each plan over it captures its stores, as
    /// `joined_vectordb_space` builds its space, without a parked join.
    async fn mirrored_vectordb_space() -> (ParRSpace, SpaceEnv) {
        let mut stores = InMemoryStoreManager::new();
        let host = ParRSpace::create(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(Matcher)),
        )
        .expect("an in-memory space");
        let env = SpaceEnv {
            store: SpaceStoreHandle::new(host.clone()),
            budget: RuntimeBudget::new(Cost::create(10_000_000, "join engine test")),
            volatile: Default::default(),
            origin: Default::default(),
        };
        let family = SpaceFactoryUrn::all()
            .find(|urn| urn.outer == SpaceOuter::HashMap && urn.inner == SpaceInner::VectorDb)
            .expect("a factory family");
        let mut config = SpaceConfig::new(
            SPACE.sid,
            SpaceOuter::HashMap.as_str(),
            SpaceInner::VectorDb.as_str(),
            SpaceQualifier::Default,
        );
        config.params.insert("capacity".to_string(), int(100));
        config.params.insert("dimensions".to_string(), int(2));
        config.params.insert("mirror".to_string(), string("chroma"));
        FamilyAgent
            .prepare_create(&env, SpaceCreation {
                space: SPACE,
                config: config.to_par(),
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

    /// Added by decision 124 (plan v4 C7.13, ADD A1.12): on a mirrored space, a two-channel join
    /// fire of a send commits with no write of the mirror: the element is in flight and its
    /// partner is persistent, so no record leaves the space. The control is the store of the
    /// partner, which writes. The capture of the fire row at its index is private state of
    /// the plan, so the test reads what a commit of the row as a fire writes.
    #[tokio::test]
    async fn join_group_fire_captures_its_datum_in_the_mirror() {
        let (_host, env) = mirrored_vectordb_space().await;
        let (s, t) = (string("s"), string("t"));
        let stored = run(&env, vector_send(&t, [1.0, 1.0], 2, true)).await;
        let Some(_) = stored.mirror else {
            panic!("a store of a mirrored space writes to the mirror");
        };
        let parked = run(&env, join(&[(&s, any()), (&t, any())], "k", false)).await;
        assert!(parked.fires.is_empty(), "the join parks on the empty s");
        let outcome = run(&env, vector_send(&s, [1.0, 0.0], 1, false)).await;
        assert_eq!(outcome.fires.len(), 1, "the send fires the join");
        match outcome.mirror {
            None => {}
            Some(writes) => panic!("the fire of the join writes to the mirror: {writes:?}"),
        }
    }

    /// Added by decision 124 (T-C4-24, keyed J): the native space of the tests of the claims of
    /// the jobs of a `seq` space.
    const SEQ_SPACE: NativeSpace = NativeSpace {
        sid: [12; SID_LEN],
        durability: Durability::Ephemeral,
    };

    /// `op` on the native space `space`. A settle operation names its space itself.
    fn on_space(op: SpaceOp, space: NativeSpace) -> SpaceOp {
        match op {
            SpaceOp::Produce(op) => SpaceOp::Produce(ProduceOp { space, ..op }),
            SpaceOp::Consume(op) => SpaceOp::Consume(ConsumeOp { space, ..op }),
            SpaceOp::Join(op) => SpaceOp::Join(JoinOp { space, ..op }),
            SpaceOp::Settle(op) => SpaceOp::Settle(op),
        }
    }

    /// The data channel of `channel` in the space `space`.
    fn data_on(space: NativeSpace, channel: &Par) -> Par {
        namespace::gprivate(namespace::data_channel(space, &key_of(channel)))
    }

    /// Added by decision 124 (T-C4-24, keyed J): a fresh `seq` bag space over a fresh play space.
    /// It is the space of `space_of_outer` with the qualifier `seq`, on the space `SEQ_SPACE`.
    async fn seq_bag() -> (ParRSpace, SpaceEnv) {
        let mut stores = InMemoryStoreManager::new();
        let host = ParRSpace::create(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(Matcher)),
        )
        .expect("an in-memory space");
        let env = SpaceEnv {
            store: SpaceStoreHandle::new(host.clone()),
            budget: RuntimeBudget::new(Cost::create(10_000_000, "join engine test")),
            volatile: Default::default(),
            origin: Default::default(),
        };
        let family = SpaceFactoryUrn::all()
            .find(|urn| urn.outer == SpaceOuter::HashMap && urn.inner == SpaceInner::Bag)
            .expect("a factory family");
        let config = SpaceConfig::new(
            SEQ_SPACE.sid,
            SpaceOuter::HashMap.as_str(),
            SpaceInner::Bag.as_str(),
            SpaceQualifier::Seq,
        )
        .to_par();
        FamilyAgent
            .prepare_create(&env, SpaceCreation {
                space: SEQ_SPACE,
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

    /// Added by decision 124 (T-C4-24 for keyed J, rule CR-24 bullet 3, ADD A1.10 (c) 11): a `seq`
    /// space with two join waiters of the pick on different groups. The waiter on `[s, a]` binds
    /// the send of 5 as c0. The waiter on `[s, b]` never binds 5, so it is never c0, and the
    /// search lists it as an alternative. After another job claims `b`, the second waiter is
    /// inadmissible for the job of the send, and the search lists the first waiter only. Each
    /// staging drops its work, so nothing commits.
    #[tokio::test]
    async fn join_row_search_skips_an_alternative_with_an_inadmissible_claim() {
        seq_history::with_history(SeqHistory::default(), async {
            let (s, a, b) = (string("s"), string("a"), string("b"));
            let (_host, env) = seq_bag().await;
            for op in [send(&a, 1, false), send(&b, 2, false)] {
                run(&env, on_space(op, SEQ_SPACE)).await;
            }
            for op in [
                join(&[(&s, any()), (&a, any())], "w1", false),
                join(&[(&s, exactly(6)), (&b, any())], "w2", false),
            ] {
                let parked = run(&env, on_space(op, SEQ_SPACE)).await;
                assert!(parked.fires.is_empty(), "the join parks on the empty s");
            }
            let owner = owner_funded(&env);
            let offer = on_space(send(&s, 5, false), SEQ_SPACE);
            let (searches, _ops) =
                staged_rows::<BagDiscipline>(&owner, SEQ_SPACE, &s, offer.clone()).await;
            let search = searches
                .first()
                .expect("the fire of the send carries its row search");
            assert_eq!(search.runs().len(), 2, "both groups are candidates");
            // The other job claims the channel of the second group with no causal link to the
            // job of the send: its history is a copy, so the use stays unknown to the send.
            let claimed = data_on(SEQ_SPACE, &b);
            seq_history::inherit(async {
                let mut claims = SeqClaims::open(
                    &env.volatile,
                    SEQ_SPACE,
                    SpaceQualifier::Seq,
                    false,
                    b"another job",
                )
                .expect("the claims of the other job open")
                .expect("a job on a seq space claims");
                claims
                    .claim(&claimed, ClaimRole::Data)
                    .expect("the other job claims the channel");
                claims.close_row(0);
                claims.store(1, None);
            })
            .await;
            let (searches, _ops) = staged_rows::<BagDiscipline>(&owner, SEQ_SPACE, &s, offer).await;
            let search = searches
                .first()
                .expect("the fire of the send carries its row search");
            let runs = search.runs();
            let [kept_run] = runs.as_slice() else {
                panic!(
                    "the search lists the first waiter only: {} runs",
                    runs.len()
                );
            };
            assert!(
                kept_run.channels.contains(&data_on(SEQ_SPACE, &a)),
                "the kept run is the run of the first waiter"
            );
        })
        .await;
    }

    /// Added by decision 124 (plan v4 C7.9, ruling RADD 1.5, ADD A1.11, V4-17): the receive of a
    /// join whose commit may veto and whose recorded row is Stored parks with a row search, and
    /// the search answers E3-a by the one rule of MATCHING (R15), as the staging skip does
    /// (`Cx::may_veto`): a guard may veto, and a waiter without a guard may not. No theory is
    /// at work in this space, so the third input of the rule is false.
    #[tokio::test]
    async fn join_row_search_answers_e3a_by_r15() {
        let s = string("s");
        let (_host, env) = joined_set(&[send(&s, 1, false)]).await;
        let (searches, _ops) = staged_rows_over::<SetDiscipline>(
            &owner_funded(&env),
            SPACE,
            &s,
            |txn| replaying(txn, Some(Vec::new())),
            guarded_receive(&s, "k", any(), Some(equals(1))),
        )
        .await;
        let search = searches
            .first()
            .expect("the E3-a park carries its row search");
        let guarded = TaggedContinuation {
            guard: Some(equals(1)),
            ..named("k")
        };
        let unguarded = named("k");
        assert!(search.may_veto(&Matcher, &guarded), "a guard may veto");
        assert!(
            !search.may_veto(&Matcher, &unguarded),
            "no guard and no theory cannot veto"
        );
        assert_eq!(
            search.may_veto(&Matcher, &guarded),
            may_veto(&Matcher, &guarded, false),
            "the search answers by the one rule of the staging"
        );
    }

    /// Added by decision 124 (errata V4-17, rules CR-7 and CR-22): a send whose offer replay
    /// skips past a vetoing waiter on a recorded Stored row stores with a row search, as an E3-a
    /// park does. Without owner funding the store carries none (RC2 D-10).
    #[tokio::test]
    async fn join_e3a_store_carries_its_row_search() {
        let s = string("s");
        let (_host, env) = joined_space(SpaceInner::Bag, &[]).await;
        let parked = run(&env, guarded_receive(&s, "k", any(), Some(equals(1)))).await;
        assert!(parked.fires.is_empty(), "the guarded receive parks");
        let (searches, _ops) = staged_rows_over::<BagDiscipline>(
            &owner_funded(&env),
            SPACE,
            &s,
            |txn| replaying(txn, Some(Vec::new())),
            send(&s, 1, false),
        )
        .await;
        let [search] = searches.as_slice() else {
            panic!("the E3-a store carries one row search: {}", searches.len());
        };
        assert!(
            !search.runs().is_empty(),
            "the search lists the waiters of its round"
        );
        let (searches, _ops) = staged_rows_over::<BagDiscipline>(
            &env,
            SPACE,
            &s,
            |txn| replaying(txn, Some(Vec::new())),
            send(&s, 1, false),
        )
        .await;
        assert!(searches.is_empty(), "no owner funding gives no search");
    }

    /// Added by decision 124 (plan v4 C7.9, ruling RADD 1.5): T-S1-KEYED-RETIRES. For a class of
    /// three stored members, the rows that the commit readies from `class_retires` through
    /// `RetireRows` name the `K_retire` rows that `retire_spent` stages after the fire, in
    /// order. `class_retires` reads through `StoreView`, so it runs over the plan here (its
    /// substitute: the commit reads the same function over `CommitView`), and it hands its
    /// members to the builder of the rows of `actual_fire`.
    #[tokio::test]
    async fn keyed_commit_retires_equal_the_staged_retires() {
        let s = string("s");
        let mut stored = Vec::<SpaceOp>::with_capacity(3);
        for seed in ["m1", "m2", "m3"] {
            stored.push(seeded_send(&s, 1, seed, false).0);
        }
        let (_host, env) = joined_set(&stored).await;
        let source: KeyedSource<'_, HashMapOuter, SetDiscipline> = KeyedSource {
            outer: std::marker::PhantomData,
            discipline: &SetDiscipline,
        };
        let op = receive(&s, "k", exactly(-1));
        let payload = ListParWithRandom {
            pars: vec![int(1)],
            ..ListParWithRandom::default()
        };
        let random = Blake2b512Random::create_from_bytes(b"class fire");
        let retire_random = RandomPurpose::Retire.derive(&random);
        let mut readied = Vec::<RetireFields>::new();
        let staged = join_plan::<SetDiscipline>(&env, &s, &op, |cx, scope| {
            let coll = scope.index_of_key(&key_of(&s))?;
            let channel = data_of(&s);
            let claim = source.claim(coll, EntryAt::Incoming, &payload)?;
            let retired =
                source.class_retires(cx, scope, &[coll], std::slice::from_ref(&claim), &[])?;
            let members = commit_members(&Matcher, retired)?;
            let rows = RetireRows::of(cx, &channel, &retire_random, None)?;
            let mut requests = Vec::<CapturedOp>::with_capacity(members.len());
            for (channel, member) in members {
                let request = rows.request_on::<SetDiscipline>(&channel, member);
                requests.push(CapturedOp::of_take(&request));
            }
            readied = retire_fields(&requests);
            let mut dirty = Vec::<bool>::with_capacity(scope.collections.len());
            dirty.resize(scope.collections.len(), false);
            source.retire_spent(cx, scope, vec![claim], &[coll], &random, None, &mut dirty)
        })
        .await;
        assert_eq!(readied.len(), 3, "the class has three members to retire");
        assert_eq!(retire_fields(&staged), readied);
    }

    /// Added by decision 124 (plan v4 C7.9, ruling RADD 1.7): T-S1-LIST. A list receive on a
    /// VectorDB collection of the join engine fires the list `[a, b]`, and the staging stages
    /// one `K_retire` for b, which the row search of the fire counts: the retires of the J
    /// search equal the retires that the staging stages.
    #[tokio::test]
    async fn join_list_fire_retires_through_the_join_search() {
        let s = string("s");
        let (_host, env) = joined_vectordb_space().await;
        run(&env, vector_send(&s, [1.0, 0.0], 1, false)).await;
        run(&env, vector_send(&s, [1.0, 1.0], 2, false)).await;
        let (searches, ops) = staged_rows::<vectordb::VectorDbDiscipline>(
            &owner_funded(&env),
            SPACE,
            &s,
            list_receive(&s, "k"),
        )
        .await;
        let search = searches
            .first()
            .expect("the list fire carries its row search");
        let staged = retire_fields(&ops).len();
        assert_eq!(staged, 1, "the fire binds a and retires b");
        assert_eq!(search.staged_retires(), staged);
    }

    /// Added by decision 124 (plan v4 C7.9, ruling RADD 1.7): `offer_list` binds only. For a list
    /// waiter whose guard refuses the list, `offer_list` returns the bound list, and the list
    /// arm of `offer` refuses the waiter: the send is stored and the waiter stays. The guard
    /// `x == 99` over the first bound variable refuses every list.
    #[tokio::test]
    async fn offer_list_guard_runs_in_the_list_arm() {
        let s = string("s");
        let (host, env) = joined_vectordb_space().await;
        let SpaceOp::Consume(waiter) = list_receive(&s, "k") else {
            panic!("a list receive is a consume");
        };
        let guarded = SpaceOp::Consume(ConsumeOp {
            guard: Some(equals(99)),
            ..waiter
        });
        let parked = run(&env, guarded).await;
        assert!(
            parked.fires.is_empty(),
            "the list waiter parks on the empty s"
        );
        let op = vector_send(&s, [1.0, 0.0], 1, false);
        let SpaceOp::Produce(element) = op.clone() else {
            panic!("a send is a produce");
        };
        let source: KeyedSource<'_, HashMapOuter, vectordb::VectorDbDiscipline> = KeyedSource {
            outer: std::marker::PhantomData,
            discipline: &vectordb::VectorDbDiscipline,
        };
        join_plan::<vectordb::VectorDbDiscipline>(&env, &s, &op, |cx, scope| {
            let mut engine = Engine::new(&source, scope, &op, cx.config())?;
            let coll = scope.index_of_key(&key_of(&s))?;
            *engine.ords.get_mut(coll).expect("s has an order slot") = Some(0);
            let x = engine.own_datum(cx, &element, coll)?;
            let Some(cfg) = engine.similarity.as_ref() else {
                panic!("a VectorDB engine holds its config");
            };
            let x_vector = vectordb::record_vector(&x.record, cfg)?;
            let order = source.offer_order(cx, scope, &x)?;
            let Some(at) = order.into_iter().find(|at| at.group.len() == 1) else {
                panic!("the list waiter is a waiter of the offer");
            };
            let waiter = engine.load(cx, at)?;
            let Some((query, k)) = engine.list_waiter(&waiter.slots)? else {
                panic!("the waiter of the one-bind group is a list waiter");
            };
            let Some(_) = engine.offer_list(cx, &waiter, &x, x_vector.as_ref(), &query, k)? else {
                panic!("offer_list binds the list, and the guard runs in the list arm");
            };
            Ok(())
        })
        .await;
        let outcome = run(&env, op).await;
        assert!(outcome.fires.is_empty(), "the guard refuses the list");
        assert_eq!(
            host.get_data(&data_of(&s)).await.len(),
            1,
            "the send is stored"
        );
        let waiting = host
            .get_waiting_continuations(vec![data_of(&s)])
            .await
            .len();
        assert_eq!(waiting, 1, "the list waiter stays");
    }

    /// Added by decision 124 (plan v4 C7.9, rulings RADD 1.7 and OP-F13): T-S3-LIST-RESET. The
    /// list waiter L holds the stored record a in its list, and the book holds the pair of a and
    /// L, so L parks and the book reader refuses it at the offer of x. The waiter P binds only
    /// x. In a pick of L before P, L sets its list and the book refuses it, and the fire of P
    /// must not carry that list. Over `PICK_RUNS` stagings, each one drawn anew, the search of
    /// the fire counts no list retire. The control is the staging with an empty book and no P:
    /// the settle before the plan fires L with the list [a, b], and the fire retires b.
    #[tokio::test]
    async fn offer_list_refused_by_the_book_leaves_no_list() {
        let s = string("s");
        // A probe with an empty book fires the list receive over the same record, and its COMM
        // gives the consume of L.
        let (probe, probe_env) = joined_vectordb_space().await;
        run(&probe_env, vector_send(&s, [1.0, 0.0], 1, false)).await;
        probe.take_event_log().await;
        run(&probe_env, list_receive(&s, "l")).await;
        let log = probe.take_event_log().await;
        let consume = comms_of(&log)
            .first()
            .expect("the probe fires L")
            .consume
            .hash
            .clone();
        let (host, env) = joined_vectordb_space().await;
        run(&env, vector_send(&s, [1.0, 0.0], 1, false)).await;
        let a = host
            .get_data(&data_of(&s))
            .await
            .into_iter()
            .next()
            .expect("s holds a")
            .source
            .hash;
        // The record b ranks below a and x for the query of L, so the list of the control is
        // [a, b], and the list at the offer of x stays [a, x]. P binds neither a nor b.
        run(&env, vector_send(&s, [0.0, 1.0], 3, false)).await;
        env.volatile.book_pairs([(a, consume)]);
        let parked = run(&env, list_receive(&s, "l")).await;
        assert!(parked.fires.is_empty(), "the book parks L");
        let x = vector_send(&s, [1.0, 1.0], 2, false);
        let control = SpaceEnv {
            volatile: Default::default(),
            ..owner_funded(&env)
        };
        let (searches, _ops) =
            staged_rows::<vectordb::VectorDbDiscipline>(&control, SPACE, &s, x.clone()).await;
        let staged_retires = searches
            .iter()
            .map(|search| search.staged_retires())
            .sum::<usize>();
        assert_eq!(staged_retires, 1, "L retires the other member");
        let literal = BindPattern {
            patterns: vec![vector_par([1.0, 1.0])],
            remainder: None,
            free_count: 0,
        };
        let parked = run(&env, receive(&s, "p", literal)).await;
        assert!(parked.fires.is_empty(), "P binds no stored record");
        let owner = owner_funded(&env);
        for _ in 0..PICK_RUNS {
            let (searches, _ops) =
                staged_rows::<vectordb::VectorDbDiscipline>(&owner, SPACE, &s, x.clone()).await;
            let search = searches
                .first()
                .expect("the fire of P carries its row search");
            assert_eq!(search.staged_retires(), 0, "the fire of P carries no list");
        }
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.2): the inputs of an engine
    /// of the operation `op` over the scope `scope`, in the plan `cx`.
    fn inputs_of(cx: &Cx<'_, '_>, scope: &JoinScope, op: &SpaceOp) -> Arc<EngineInputs> {
        let random = Blake2b512Random::create_from_bytes(b"join row test");
        let job = SpaceJob {
            space: SPACE,
            random_state: random.to_bytes(),
            authority: None,
        };
        Arc::new(EngineInputs {
            op: Arc::new(op.clone()),
            scope: scope.clone(),
            config: cx.config().clone(),
            random,
            authority: None,
            introduction: Arc::clone(cx.introduction()),
            job,
        })
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.2): the state of an engine
    /// before a fire: three fires, a changed state, no dirty collection, the element of the
    /// operation in the state `own` with the cause `own_cause`, and the order 7 on the
    /// collection `at` of the scope of width `width`.
    fn before_at(width: usize, at: usize, own: Own, own_cause: Option<ReissueCause>) -> Before {
        let mut dirty = Vec::<bool>::with_capacity(width);
        dirty.resize(width, false);
        let mut ords = Vec::<Option<i64>>::with_capacity(width);
        ords.resize(width, None);
        *ords.get_mut(at).expect("the collection has an order slot") = Some(7);
        Before {
            fired: 3,
            changed: true,
            dirty,
            own,
            own_cause,
            ords,
        }
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.2): the side of a row of an
    /// offer, whose element is the element of the operation when `own` holds.
    fn offer_side(own: bool) -> JoinSide {
        JoinSide::Offer {
            x: Incoming {
                coll: 0,
                record: ListParWithRandom::default(),
                payload: ListParWithRandom::default(),
                source: Produce::create(&int(0), &ListParWithRandom::default(), false),
                persist: false,
                ord: 0,
                level: None,
                placement: Placement::Fresh,
                introduction: Introduction::new(
                    &RuntimeBudget::new(Cost::create(1_000, "join row test")),
                    &CostAuthority::default(),
                ),
                own,
                in_flight: true,
                restore: None,
            },
            vector: None,
            order: Vec::new(),
        }
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.2): the side of a row of a
    /// receive.
    fn receive_side() -> JoinSide {
        let waiting = ParWaiter {
            patterns: Vec::new(),
            continuation: named("k"),
            persist: false,
            peeks: BTreeSet::new(),
            source: Consume::create(&vec![int(0)], &vec![any()], &named("k"), false),
        };
        JoinSide::Receive {
            group: Vec::new(),
            colls: Vec::new(),
            slots: Vec::new(),
            waiting,
        }
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.2): a row at the site
    /// `site` with the side `side` and the engine state `before`, in the plan `cx`. The search of a
    /// staged fire is opaque to a test (`Box<dyn RowSearch>`), so a test of the site table or of
    /// the end of a read builds the row by hand, with what the read of its part takes.
    fn row_at(
        cx: &Cx<'_, '_>,
        inputs: &Arc<EngineInputs>,
        side: JoinSide,
        site: FireSite,
        before: Before,
    ) -> JoinRow {
        let channel = data_of(&string("a"));
        let staged = Consume::create(&vec![channel.clone()], &vec![any()], &named("k"), false);
        let random = Blake2b512Random::create_from_bytes(b"join row retire");
        let rows = RetireRows::of(cx, &channel, &random, None).expect("the retire rows of a row");
        JoinRow {
            inputs: Arc::clone(inputs),
            work: Vec::new(),
            side,
            staged,
            channel,
            colls: Vec::new(),
            tuple: Vec::new(),
            classes: Vec::new(),
            list: None,
            peeks: false,
            theory: None,
            alternatives: true,
            rows,
            found: None,
            retires: Vec::new(),
            fired: None,
            site,
            before,
            facts: None,
            sends: Vec::new(),
            last: None,
            book: cx.book(),
            reissue: None,
        }
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.2): the engine of the row
    /// inputs `inputs` at the state `before`, as `JoinRow::tail_step` starts it.
    fn engine_at<'r, Src: JoinSource>(
        source: &'r Src,
        inputs: &'r EngineInputs,
        before: &Before,
    ) -> Engine<'r, Src> {
        let mut engine = Engine::new(source, &inputs.scope, &inputs.op, &inputs.config)
            .expect("the engine of the row");
        engine.dirty = before.dirty.clone();
        engine.own = before.own;
        engine.own_cause = before.own_cause;
        engine.ords = before.ords.clone();
        engine
    }

    /// The fields of the engine `engine` that a site sets: the state of the element, its cause,
    /// the stop and the count of the owed sends.
    fn state_of<Src: JoinSource>(
        engine: &Engine<'_, Src>,
    ) -> (Own, Option<ReissueCause>, Option<ReissueCause>, usize) {
        (engine.own, engine.own_cause, engine.stop, engine.work.len())
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.2): T-S8-SITE. For each row
    /// of the table of `site_step`, the rest of the site after the fire of the row: an offer that
    /// peeks returns before its step, the round cap stops every other site, an offer and a
    /// receive loop on `Continue`, a divergent pair stops a cascade and the offer of a second
    /// send, and every other step returns to the drain. The rows are built by hand (`row_at`),
    /// because the search of a staged fire is opaque to a test.
    #[tokio::test]
    async fn site_step_follows_each_engine_site() {
        let (a, b) = (string("a"), string("b"));
        let (_host, env) = space_of(SpaceInner::Queue).await;
        let parked = run(
            &env,
            join(&[(&a, exactly(-1)), (&b, exactly(-1))], "w", false),
        )
        .await;
        assert!(parked.fires.is_empty(), "the join parks");
        let op = send(&a, 1, false);
        let divergent = FireStep::Reissue(ReissueCause::DivergentPair);
        let refused = FireStep::Reissue(ReissueCause::OwnerRefusal);
        let capped = FireStep::Reissue(ReissueCause::RoundCap);
        let stages = Some(TailStep::Stages);
        let capped_step = Some(TailStep::Capped);
        let commits = Some(TailStep::Commits);
        // The first column is `Some(own)` for an offer whose element is the element of the
        // operation when `own` holds, and `None` for a receive.
        let table = [
            (Some(true), FireSite::Offer, true, FireStep::Done, None),
            (Some(true), FireSite::Offer, true, capped, None),
            (Some(true), FireSite::Offer, false, capped, capped_step),
            (Some(false), FireSite::Offer, false, capped, capped_step),
            (None, FireSite::Single, false, capped, capped_step),
            (None, FireSite::Join, false, capped, capped_step),
            (Some(true), FireSite::Cascade, false, capped, capped_step),
            (
                Some(true),
                FireSite::Offer,
                false,
                FireStep::Continue,
                stages,
            ),
            (None, FireSite::Single, false, FireStep::Continue, stages),
            (None, FireSite::Join, true, FireStep::Continue, stages),
            (Some(true), FireSite::Offer, false, divergent, None),
            (Some(false), FireSite::Offer, false, divergent, commits),
            (Some(true), FireSite::Offer, false, refused, None),
            (Some(false), FireSite::Offer, false, refused, commits),
            (Some(true), FireSite::Cascade, false, divergent, commits),
            (Some(false), FireSite::Cascade, true, refused, commits),
            (Some(true), FireSite::Offer, false, FireStep::Done, None),
            (None, FireSite::Single, false, FireStep::Done, None),
            (None, FireSite::Join, false, divergent, None),
            (Some(true), FireSite::Cascade, false, FireStep::Done, None),
            (
                Some(true),
                FireSite::Cascade,
                false,
                FireStep::Continue,
                None,
            ),
        ];
        join_plan::<QueueDiscipline>(&env, &a, &op, |cx, scope| {
            let inputs = inputs_of(cx, scope, &op);
            let at_a = scope.index_of_key(&key_of(&a))?;
            let before = before_at(scope.collections.len(), at_a, Own::InFlight, None);
            for (case, (side, site, peeks, step, expected)) in table.iter().copied().enumerate() {
                let side = match side {
                    Some(own) => offer_side(own),
                    None => receive_side(),
                };
                let row = row_at(cx, &inputs, side, site, before.clone());
                assert_eq!(row.site_step(peeks, step), expected, "case {case}");
            }
            Ok(())
        })
        .await;
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD 1.2 and RADD2 1.1):
    /// T-S8-REISSUE. The parts of a tail read that take no lists give the end that the engine
    /// gives at the same state: `settle_unit`, the arms of `site_state`, `after_drain`, and
    /// `end_of` for `Commits`, `Capped` and `Stages`. The expected re-issue carries no order
    /// for a send that is not persistent and the order 7 for a persistent one (decision 132).
    /// `drain_step`, `tail_read` and the list receive read the real lists of a commit, in the
    /// tests `join_tail_end_equals_the_engine_end_in_drain_step`, `.._in_tail_read` and
    /// `.._for_a_list_receive` (ruling RADD2v4 7.7). Here the rows are built by hand (`row_at`).
    #[tokio::test]
    async fn join_tail_end_equals_the_engine_end() {
        let (a, b) = (string("a"), string("b"));
        let (_host, env) = space_of(SpaceInner::Queue).await;
        let parked = run(
            &env,
            join(&[(&a, exactly(-1)), (&b, exactly(-1))], "w", false),
        )
        .await;
        assert!(parked.fires.is_empty(), "the join parks");
        let plain = send(&a, 1, false);
        let persistent = send(&a, 1, true);
        let SpaceOp::Produce(plain_send) = plain.clone() else {
            panic!("a send is a produce");
        };
        let SpaceOp::Produce(persistent_send) = persistent.clone() else {
            panic!("a send is a produce");
        };
        let key_b = key_of(&b);
        let queue: KeyedSource<'_, HashMapOuter, QueueDiscipline> = KeyedSource {
            outer: std::marker::PhantomData,
            discipline: &QueueDiscipline,
        };
        let divergent = FireStep::Reissue(ReissueCause::DivergentPair);
        let capped = FireStep::Reissue(ReissueCause::RoundCap);
        let again = |send: &ProduceOp, ord: Option<i64>, cause: ReissueCause| SpaceReissue {
            op: SpaceOp::Produce(ProduceOp {
                reissued_ord: ord,
                ..send.clone()
            }),
            cause,
        };
        let settle_job = |seeds: Vec<CollectionKey>, resume: Option<SpaceReissue>| {
            SpaceOp::Settle(SettleOp {
                cascade: Some(Box::new(CascadeState {
                    seeds,
                    pending: Vec::new(),
                    resume: resume.map(Box::new),
                })),
                ..settle_operation(generic::OpView::Produce(&plain_send))
            })
        };
        join_plan::<QueueDiscipline>(&env, &a, &plain, |cx, scope| {
            let at_a = scope.index_of_key(&key_of(&a))?;
            let at_b = scope.index_of_key(&key_b)?;
            let width = scope.collections.len();
            let plain_inputs = inputs_of(cx, scope, &plain);
            let persistent_inputs = inputs_of(cx, scope, &persistent);
            let start = before_at(width, at_a, Own::NotStarted, None);

            // `settle_unit`: a store or a park settles the element of the operation.
            for (side, expected) in [
                (offer_side(true), Own::Settled),
                (offer_side(false), Own::NotStarted),
                (receive_side(), Own::Settled),
            ] {
                let row = row_at(cx, &plain_inputs, side, FireSite::Offer, start.clone());
                let mut engine = engine_at(&queue, &plain_inputs, &start);
                row.settle_unit(&mut engine);
                assert_eq!(engine.own, expected, "settle_unit");
            }

            // The arms of `site_state`: the state after the site of the fire, from the state
            // of the engine before the plan.
            let unchanged = (Own::NotStarted, None, None, 0);
            let settled = (Own::Settled, None, None, 0);
            let in_flight = (Own::InFlight, Some(ReissueCause::DivergentPair), None, 0);
            let at_cap = (
                Own::InFlight,
                Some(ReissueCause::RoundCap),
                Some(ReissueCause::RoundCap),
                0,
            );
            let table = [
                (Some(true), FireSite::Offer, true, FireStep::Done, unchanged),
                (Some(true), FireSite::Offer, false, FireStep::Done, settled),
                (
                    Some(false),
                    FireSite::Offer,
                    false,
                    FireStep::Done,
                    unchanged,
                ),
                (
                    Some(true),
                    FireSite::Offer,
                    false,
                    FireStep::Continue,
                    unchanged,
                ),
                (Some(true), FireSite::Offer, false, divergent, in_flight),
                (Some(true), FireSite::Offer, false, capped, at_cap),
                (
                    Some(false),
                    FireSite::Offer,
                    false,
                    FireStep::Continue,
                    (Own::NotStarted, None, None, 1),
                ),
                (
                    Some(false),
                    FireSite::Offer,
                    false,
                    divergent,
                    (Own::NotStarted, None, Some(ReissueCause::DivergentPair), 1),
                ),
                (None, FireSite::Join, false, FireStep::Done, settled),
                (None, FireSite::Single, false, FireStep::Done, settled),
                (None, FireSite::Join, false, divergent, in_flight),
                (None, FireSite::Single, false, capped, at_cap),
                (None, FireSite::Join, true, FireStep::Continue, unchanged),
                (
                    None,
                    FireSite::Cascade,
                    false,
                    divergent,
                    (Own::NotStarted, None, Some(ReissueCause::DivergentPair), 0),
                ),
                (None, FireSite::Cascade, false, FireStep::Done, unchanged),
                (
                    None,
                    FireSite::Cascade,
                    false,
                    FireStep::Continue,
                    unchanged,
                ),
            ];
            for (case, (side, site, peeks, step, expected)) in table.into_iter().enumerate() {
                let side = match side {
                    Some(own) => offer_side(own),
                    None => receive_side(),
                };
                let row = row_at(cx, &plain_inputs, side, site, start.clone());
                let mut engine = engine_at(&queue, &plain_inputs, &start);
                row.site_state(&mut engine, peeks, step)?;
                assert_eq!(state_of(&engine), expected, "site_state, case {case}");
            }

            // `after_drain`: the settle before the plan clears its marks, and the plan follows
            // below the cap. Otherwise the run commits.
            let mut engine = engine_at(&queue, &plain_inputs, &start);
            *engine.dirty.get_mut(at_a).expect("a has a mark") = true;
            assert_eq!(after_drain(&mut engine, 0), TailStep::Stages);
            assert!(!engine.dirty.contains(&true), "the settle clears the marks");
            let mut engine = engine_at(&queue, &plain_inputs, &start);
            assert_eq!(after_drain(&mut engine, FIRE_ROUND_CAP), TailStep::Capped);
            for own in [Own::InFlight, Own::Settled] {
                let state = before_at(width, at_a, own, None);
                let mut engine = engine_at(&queue, &plain_inputs, &state);
                *engine.dirty.get_mut(at_a).expect("a has a mark") = true;
                assert_eq!(after_drain(&mut engine, 0), TailStep::Commits);
                assert!(engine.dirty.contains(&true), "the marks stay");
            }

            // `end_of`: the end from the re-issue of the engine at the end of the read.
            let row = row_at(
                cx,
                &plain_inputs,
                offer_side(true),
                FireSite::Offer,
                start.clone(),
            );
            let persistent_row = row_at(
                cx,
                &persistent_inputs,
                offer_side(true),
                FireSite::Offer,
                start.clone(),
            );
            let settled = before_at(width, at_a, Own::Settled, None);
            let in_flight = before_at(width, at_a, Own::InFlight, None);
            let refused = before_at(width, at_a, Own::InFlight, Some(ReissueCause::OwnerRefusal));
            // Commits: the end returns the re-issue of the state.
            let engine = engine_at(&queue, &plain_inputs, &settled);
            assert_eq!(
                row.end_of(engine, TailStep::Commits),
                Ok(TailEnd::Return(None))
            );
            let engine = engine_at(&queue, &plain_inputs, &start);
            assert_eq!(
                row.end_of(engine, TailStep::Commits),
                Ok(TailEnd::Return(Some(again(
                    &plain_send,
                    None,
                    ReissueCause::DivergentPair
                ))))
            );
            let engine = engine_at(&queue, &persistent_inputs, &refused);
            assert_eq!(
                persistent_row.end_of(engine, TailStep::Commits),
                Ok(TailEnd::Return(Some(again(
                    &persistent_send,
                    Some(7),
                    ReissueCause::OwnerRefusal
                ))))
            );
            // Capped: the cap stops the run, and the re-issue carries the stop of the cap.
            let engine = engine_at(&queue, &plain_inputs, &settled);
            assert_eq!(
                row.end_of(engine, TailStep::Capped),
                Ok(TailEnd::Return(None))
            );
            let engine = engine_at(&queue, &plain_inputs, &in_flight);
            assert_eq!(
                row.end_of(engine, TailStep::Capped),
                Ok(TailEnd::Return(Some(again(
                    &plain_send,
                    None,
                    ReissueCause::RoundCap
                ))))
            );
            let mut engine = engine_at(&queue, &plain_inputs, &in_flight);
            *engine.dirty.get_mut(at_b).expect("b has a mark") = true;
            assert_eq!(
                row.end_of(engine, TailStep::Capped),
                Ok(TailEnd::Return(Some(SpaceReissue {
                    op: settle_job(
                        vec![key_b.clone()],
                        Some(again(&plain_send, None, ReissueCause::RoundCap)),
                    ),
                    cause: ReissueCause::RoundCap,
                })))
            );
            // Stages: the end stops the operation for an owner refusal with the re-issue of
            // the rest.
            let engine = engine_at(&queue, &plain_inputs, &in_flight);
            assert_eq!(
                row.end_of(engine, TailStep::Stages),
                Ok(TailEnd::Stop(SpaceOp::Produce(ProduceOp {
                    reissued_ord: None,
                    ..plain_send.clone()
                })))
            );
            let mut engine = engine_at(&queue, &plain_inputs, &settled);
            *engine.dirty.get_mut(at_b).expect("b has a mark") = true;
            assert_eq!(
                row.end_of(engine, TailStep::Stages),
                Ok(TailEnd::Stop(settle_job(vec![key_b.clone()], None)))
            );
            let engine = engine_at(&queue, &plain_inputs, &settled);
            assert_eq!(
                row.end_of(engine, TailStep::Stages),
                Err(SpaceError::Inconsistent(
                    "a join row owes a step that no re-issue carries"
                ))
            );
            Ok(())
        })
        .await;
    }

    /// Added by decision 124 (plan v4 C7.11, rulings RADD 1.2 and RADD2v2 6.6): T-S8-FINISH. The
    /// engine part decides the read, because the finish rows of a source join the unit (P1).
    #[test]
    fn finish_read_follows_the_finish_of_the_engine() {
        assert_eq!(finish_read(TailStep::Stages), TailRead::Stages);
        assert_eq!(finish_read(TailStep::Capped), TailRead::Reissues);
        assert_eq!(finish_read(TailStep::Commits), TailRead::Ends);
    }

    /// Added by decision 124 (plan v4 C7.11, ruling RADD 1.2): T-S8-FINISH. The engine owes a step
    /// below the cap, and the cap stops it before the step.
    #[test]
    fn round_cap_owes_a_step_below_the_cap() {
        assert_eq!(round_cap(0), TailStep::Stages);
        assert_eq!(round_cap(FIRE_ROUND_CAP - 1), TailStep::Stages);
        assert_eq!(round_cap(FIRE_ROUND_CAP), TailStep::Capped);
    }

    // ── Decision 124: a join row over the lists of a real commit ───────────────────────────

    /// Added by decision 124 (ruling RADD2v4 7.7): commits a fire of the first record of the
    /// collection of `carrier`, whose discipline is `D`, under the observer `Funded`, in a
    /// transaction that also locks `keys`, the lock keys of a join scope. It runs `probe` once,
    /// over the lists of that commit and before the fire changes the store, and returns the
    /// answer of `probe`. Each call needs a record of its own on `carrier`.
    async fn commit_lists<D, T, F>(
        host: &ParRSpace,
        env: &SpaceEnv,
        carrier: &Par,
        keys: &[Par],
        probe: F,
    ) -> T
    where
        D: Discipline,
        T: Send + 'static,
        F: FnOnce(&ParRowLists<'_>) -> T + Send + 'static,
    {
        host.set_accounting_observer(Some(Arc::new(Funded::admitting())));
        let mut opened = begin_collection(&HashMapOuter, env, SPACE, &key_of(carrier), carrier)
            .await
            .expect("phase A of the carrier");
        env.lock_phase_b(&mut opened.txn, keys)
            .await
            .expect("phase B of the carrier");
        let CollectionTxn { txn, config, .. } = opened;
        let data = data_of(carrier);
        let op = receive(carrier, "carrier", any());
        let authority = CostAuthority::default();
        let admission = Admission::of(&config, &op).expect("the admission of the carrier");
        let job = SpaceJob::of(&op);
        let discipline = D::default();
        let source: KeyedSource<'_, HashMapOuter, D> = KeyedSource {
            outer: std::marker::PhantomData,
            discipline: &discipline,
        };
        let (sender, answers) = std::sync::mpsc::channel::<T>();
        let mut probe = Some(probe);
        let prepared = generic::run_plan(
            env,
            txn,
            &data,
            &config,
            &authority,
            None,
            admission,
            &job,
            false,
            |cx| {
                let probe = probe.take().expect("the carrier stages once");
                let sender = sender.clone();
                let stored = cx
                    .data()?
                    .first()
                    .cloned()
                    .expect("the record of the carrier");
                let channel = cx.data_channel().clone();
                let payload = source.payload_of(&stored.a)?;
                let continuation =
                    record::wrap_continuation(0, &named("carrier")).expect("a continuation record");
                let group = vec![channel.clone()];
                let patterns = vec![any()];
                let consume = Consume::create(&group, &patterns, &continuation, false);
                let candidate = receive_candidate(channel.clone(), ReceiveFire {
                    head: stored,
                    matched: payload,
                    view_index: 0,
                });
                let inner = StagedRowSearch::<D>::of_receive(
                    cx,
                    &consume,
                    &continuation,
                    false,
                    &channel,
                    candidate.clone(),
                    0,
                );
                let search = ListsProbe::new(inner, move |lists| {
                    let _ = sender.send(probe(lists));
                });
                cx.fire_receive_group(
                    group,
                    patterns,
                    continuation,
                    false,
                    BTreeSet::new(),
                    vec![candidate],
                    1,
                    Some(Box::new(search)),
                )?;
                Ok(None)
            },
        )
        .expect("the carrier stages");
        prepared.commit().await.expect("the carrier commits");
        answers.try_recv().expect("the commit ran the probe")
    }

    /// Added by decision 124 (ruling RADD2v4 7.7): the lock keys of the join scope of `op` on the
    /// collection of `channel`, which a carrier locks so that its probe may read the scope.
    async fn scope_keys<D: Discipline>(env: &SpaceEnv, channel: &Par, op: &SpaceOp) -> Vec<Par> {
        let mut keys = Vec::<Par>::new();
        join_plan::<D>(env, channel, op, |_cx, scope| {
            keys = scope.lock_keys().to_vec();
            Ok(())
        })
        .await;
        keys
    }

    /// Added by decision 124 (ruling RADD2v4 7.7): the item test that answers with `self.0` on the
    /// value and the persistence of a datum.
    struct ItemRule<F>(F);

    impl<F> RowItemRule<TaggedContinuation, ListParWithRandom> for ItemRule<F>
    where F: Fn(&ListParWithRandom, bool) -> bool
    {
        fn admit_item(
            &self,
            _continuation: &TaggedContinuation,
            _continuation_persistent: bool,
            _channel_count: usize,
            datum: &ListParWithRandom,
            datum_persistent: bool,
        ) -> Result<bool, SpaceTxnError> {
            Ok((self.0)(datum, datum_persistent))
        }
    }

    /// Added by decision 124 (ruling RADD2v4 7.7): the terms of a staged row that the request of
    /// a probe names.
    struct StagedRowTerms {
        group: Vec<Par>,
        patterns: Vec<BindPattern>,
        continuation: TaggedContinuation,
        persist: bool,
        source: Consume,
    }

    /// Added by decision 124 (ruling RADD2v4 7.7): the request of the row with the terms `terms`,
    /// with the item test `rule` or none.
    fn request_of<'r>(
        matcher: &'r Matcher,
        terms: &'r StagedRowTerms,
        rule: Option<&'r dyn RowItemRule<TaggedContinuation, ListParWithRandom>>,
    ) -> ParRowRequest<'r> {
        ParRowRequest {
            matcher,
            group: &terms.group,
            patterns: &terms.patterns,
            continuation: &terms.continuation,
            persistent: terms.persist,
            source: &terms.source,
            own: None,
            item: rule.map(|rule| RowItemTest {
                rule,
                continuation: &terms.continuation,
                persistent: terms.persist,
                channel_count: terms.group.len(),
            }),
        }
    }

    /// Added by decision 124 (ruling RADD2v4 7.7): the terms of the first staged `Take` whose
    /// channels and patterns `wanted` accepts.
    fn take_terms(
        ops: &[CapturedOp],
        wanted: impl Fn(&[Par], &[BindPattern]) -> bool,
    ) -> StagedRowTerms {
        for op in ops {
            let CapturedOp::Take {
                channels,
                patterns,
                continuation,
                persist,
                ..
            } = op
            else {
                continue;
            };
            match wanted(channels, patterns) {
                false => continue,
                true => {
                    return StagedRowTerms {
                        group: channels.clone(),
                        patterns: patterns.clone(),
                        continuation: continuation.clone(),
                        persist: *persist,
                        source: Consume::create(channels, patterns, continuation, *persist),
                    }
                }
            }
        }
        panic!("the staged operations hold the row: {ops:?}");
    }

    /// Added by decision 124 (ruling RADD2v4 7.7): the terms of a parked waiter of the group
    /// `group`.
    fn terms_of_waiter(group: &[Par], waiter: &ParWaiter) -> StagedRowTerms {
        StagedRowTerms {
            group: group.to_vec(),
            patterns: waiter.patterns.clone(),
            continuation: waiter.continuation.clone(),
            persist: waiter.persist,
            source: waiter.source.clone(),
        }
    }

    /// Added by decision 124 (ruling RADD2v4 7.7): the first of the row searches `searches` over
    /// the channels `group`.
    fn search_on(searches: Vec<ParRowSearch>, group: &[Par]) -> ParRowSearch {
        searches
            .into_iter()
            .find(|search| matches!(search.runs().first(), Some(run) if run.channels == group))
            .expect("a staged row search over the group")
    }

    /// Added by decision 124 (ruling RADD2v4 7.7): the search and the terms of the fire of a
    /// join over two channels, among the rows staged by an operation.
    fn join_row(
        searches: Vec<ParRowSearch>,
        ops: Vec<CapturedOp>,
    ) -> (ParRowSearch, StagedRowTerms) {
        let terms = take_terms(&ops, |channels, _| channels.len() == 2);
        (search_on(searches, &terms.group), terms)
    }

    /// Added by decision 124 (ruling RADD2v4 7.7): the search and the terms of the fire on the
    /// one channel `data`, among the rows staged by an operation. The pattern of its row tells
    /// it apart from the retire rows on the same channel.
    fn own_row(
        data: &Par,
        searches: Vec<ParRowSearch>,
        ops: Vec<CapturedOp>,
    ) -> (ParRowSearch, StagedRowTerms) {
        let terms = take_terms(&ops, |channels, patterns| {
            channels == std::slice::from_ref(data) && patterns == [any()]
        });
        (search_on(searches, &terms.group), terms)
    }

    /// The fields of a bound entry that a test compares.
    type BoundFields = (EntryAt, ListParWithRandom, bool, Claim);

    /// Added by decision 124 (ADD A1.16): the fields of the bound entries `bound`, in order.
    fn bound_fields(bound: Vec<Bound>) -> Vec<BoundFields> {
        let mut fields = Vec::<BoundFields>::with_capacity(bound.len());
        for entry in bound {
            fields.push((entry.at, entry.matched, entry.persist, entry.claim));
        }
        fields
    }

    /// The fields of an owed send that a test compares.
    type WorkFields = (usize, ListParWithRandom, bool, bool, Option<u8>);

    /// Added by decision 124 (ruling RADD2v4 7.7): the fields of the owed sends `works`, in order.
    fn work_fields(works: &[Work]) -> Vec<WorkFields> {
        let mut fields = Vec::<WorkFields>::with_capacity(works.len());
        for work in works {
            fields.push((
                work.coll,
                work.record.clone(),
                work.persist,
                work.own,
                work.restore,
            ));
        }
        fields
    }

    /// Added by decision 124 (ruling RADD2v4 7.7): the first join group that parks on the
    /// collection of `channel`, and its first waiter.
    async fn parked_on(host: &ParRSpace, channel: &Par) -> (Vec<Par>, ParWaiter) {
        let group = host
            .get_joins(data_of(channel))
            .await
            .into_iter()
            .next()
            .expect("a join group parks on the collection");
        let waiter = host
            .get_waiting_continuations(group.clone())
            .await
            .into_iter()
            .next()
            .expect("the group holds a waiter");
        (group, waiter)
    }

    /// Added by decision 124 (ruling RADD2v4 7.7): a queue space where a holds 1, b holds 2 and
    /// the join over them with those values parks, so its pair is free for a cascade. The join
    /// parks behind the head 5 of a, and the test then takes that head out of the store. It
    /// returns the space, its env, the group of the join and its waiter.
    async fn fireable_pair(a: &Par, b: &Par) -> (ParRSpace, SpaceEnv, Vec<Par>, ParWaiter) {
        let (host, env) = space_of(SpaceInner::Queue).await;
        for (channel, value) in [(a, 5), (a, 1), (b, 2)] {
            run(&env, send(channel, value, false)).await;
        }
        let parked = run(&env, join(&[(a, exactly(1)), (b, exactly(2))], "w", false)).await;
        assert!(parked.fires.is_empty(), "the head 5 blocks the join");
        let data_a = data_of(a);
        let records = host.get_data(&data_a).await;
        let head = records
            .iter()
            .position(|datum| {
                record::unwrap_datum(&datum.a)
                    .expect("a datum record")
                    .datum
                    .pars
                    == vec![int(5)]
            })
            .expect("the head of a is stored");
        let head = i32::try_from(head).expect("a record index fits i32");
        host.get_store()
            .remove_datum(&data_a, head)
            .expect("the head leaves the store");
        let (group, waiter) = parked_on(&host, a).await;
        (host, env, group, waiter)
    }

    /// Added by decision 124 (rule CR-31, ruling RADD2v4 7.7): the answer and the end of the tail
    /// read of `row` over the lists `lists`, with the marks `marks`.
    fn tail_answer<Src: JoinSource>(
        row: &mut JoinRow,
        source: &Src,
        lists: &ParRowLists<'_>,
        marks: &[usize],
    ) -> (TailRead, TailEnd) {
        let read = row
            .tail_read(source, lists, &Matcher, marks)
            .expect("the tail read of the row");
        let reissue = row.take_reissue().expect("a tail read keeps the end");
        let end = reissue
            .get::<TailEnd>()
            .expect("the end of the row")
            .clone();
        (read, end)
    }

    /// Added by decision 124 (rule CR-31, ruling RADD2v4 7.7): the step of `drain_step` of `row`
    /// over the lists `lists`, for an engine at the state `before` that owes the sends `work`
    /// after `fired` fires.
    fn drain_answer<Src: JoinSource>(
        row: &JoinRow,
        source: &Src,
        lists: &ParRowLists<'_>,
        before: &Before,
        work: &[Work],
        fired: usize,
    ) -> TailStep {
        let inputs = Arc::clone(&row.inputs);
        let mut engine = engine_at(source, &inputs, before);
        engine.work = work.to_vec();
        row.drain_step(&mut engine, lists, &Matcher, fired)
            .expect("the drain step of the row")
    }

    /// Added by decision 124 (rule CR-31, ruling RADD2v4 7.7): the answer and the end of the tail
    /// read of the row search and the terms `row`, read over the lists of a carrier commit, when
    /// its commit fires c0 as staged. No item test runs, so c0 passes with no read.
    async fn staged_tail_end<D: Discipline>(
        host: &ParRSpace,
        env: &SpaceEnv,
        carrier: &Par,
        keys: &[Par],
        row: (ParRowSearch, StagedRowTerms),
    ) -> (TailRead, TailEnd) {
        let (mut search, terms) = row;
        commit_lists::<D, _, _>(host, env, carrier, keys, move |lists| {
            let matcher = Matcher;
            let request = request_of(&matcher, &terms, None);
            let tuple = search
                .tuple(0, lists, request)
                .expect("the tuple of the row reads the lists")
                .expect("c0 passes without an item test");
            let _fired = search.actual_fire(&request, &tuple);
            let read = search
                .tail_read(lists, &matcher)
                .expect("the tail read of the row");
            let reissue = search.tail_reissue().expect("a tail read keeps the end");
            let end = reissue
                .get::<TailEnd>()
                .expect("the end of the row")
                .clone();
            (read, end)
        })
        .await
    }

    /// Added by decision 124 (rule CR-31, ruling RADD2v4 7.7): stages `op` on the collection of
    /// `channel` over an owner-funded plan that commits nothing, keeps the row that `select`
    /// picks, then commits `op` in the space and reads the tail of the row over the lists of a
    /// carrier commit. It returns the answer of the read, its end and the outcome of `op`.
    async fn read_after_commit<D: Discipline>(
        (host, env): (&ParRSpace, &SpaceEnv),
        carrier: &Par,
        channel: &Par,
        op: SpaceOp,
        select: impl FnOnce(Vec<ParRowSearch>, Vec<CapturedOp>) -> (ParRowSearch, StagedRowTerms),
    ) -> (TailRead, TailEnd, SpaceOutcome) {
        let keys = scope_keys::<D>(env, channel, &op).await;
        let (searches, ops) =
            staged_rows::<D>(&owner_funded(env), SPACE, channel, op.clone()).await;
        let row = select(searches, ops);
        let outcome = run(env, op).await;
        let (read, end) = staged_tail_end::<D>(host, env, carrier, &keys, row).await;
        (read, end, outcome)
    }

    /// Added by decision 124 (rule CR-31, ruling RADD2v4 7.7): asserts that the tail read `answer`
    /// of a fire of c0 as staged ends with the re-issue of the staging engine, and that the
    /// operation made `fires` fires.
    fn assert_ends_as_staged(answer: (TailRead, TailEnd, SpaceOutcome), fires: usize, case: &str) {
        let (read, end, outcome) = answer;
        assert_eq!(
            (read, end),
            (TailRead::Ends, TailEnd::Return(outcome.reissue.clone())),
            "{case}: the end of the engine"
        );
        assert_eq!(
            outcome.fires.len(),
            fires,
            "{case}: the fires of the operation"
        );
    }

    /// Added by decision 124 (ADD A1.16, rule CR-36, ruling RADD2v4 7.7): one join scope, one
    /// waiter and the same store, with no item rule: `Engine::search` over the staged view and
    /// over the view of a commit binds the same entries. The substitute of the commit of the
    /// join is a carrier fire on another collection. Its transaction locks the join scope, and
    /// its probe runs the search over the real lists of that commit, before the fire changes
    /// the store.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn staged_view_and_commit_view_find_the_same_join_tuple() {
        let (a, b, carrier) = (string("a"), string("b"), string("carrier"));
        let (host, env) = space_of(SpaceInner::Queue).await;
        for (channel, value) in [(&a, 1), (&a, 2), (&b, 3), (&carrier, 9)] {
            run(&env, send(channel, value, false)).await;
        }
        let op = join(&[(&a, any()), (&b, any())], "w", false);
        let SpaceOp::Join(join_op) = op.clone() else {
            panic!("the operation is a join");
        };
        let queue: KeyedSource<'_, HashMapOuter, QueueDiscipline> = KeyedSource {
            outer: std::marker::PhantomData,
            discipline: &QueueDiscipline,
        };
        let mut staged = None;
        join_plan::<QueueDiscipline>(&env, &a, &op, |cx, scope| {
            let mut engine = Engine::new(&queue, scope, &op, cx.config())?;
            engine.ords.fill(Some(0));
            let mut own = Vec::<usize>::with_capacity(scope.own.len());
            for key in &scope.own {
                own.push(scope.index_of_key(key)?);
            }
            let waiter = engine.join_waiter(&join_op, &own, &[])?;
            let bound = engine.search(
                cx,
                &waiter.colls,
                &waiter.patterns,
                &waiter.slots,
                None,
                WindowOrder::Canonical,
            )?;
            staged = Some((
                scope.clone(),
                cx.config().clone(),
                waiter,
                bound.map(bound_fields),
            ));
            Ok(())
        })
        .await;
        let Some((scope, config, waiter, staged)) = staged else {
            panic!("the plan ran the staged search");
        };
        let keys = scope.lock_keys().to_vec();
        let committed =
            commit_lists::<QueueDiscipline, _, _>(&host, &env, &carrier, &keys, move |lists| {
                let queue: KeyedSource<'_, HashMapOuter, QueueDiscipline> = KeyedSource {
                    outer: std::marker::PhantomData,
                    discipline: &QueueDiscipline,
                };
                let engine =
                    Engine::new(&queue, &scope, &op, &config).expect("the engine of the commit");
                let matcher = Matcher;
                let mut view =
                    CommitView::new(lists, &matcher, None, None, scope.collections.len());
                engine
                    .search(
                        &mut view,
                        &waiter.colls,
                        &waiter.patterns,
                        &waiter.slots,
                        None,
                        WindowOrder::Canonical,
                    )
                    .expect("the search over the view of the commit")
                    .map(bound_fields)
            })
            .await;
        assert_eq!(committed, staged, "the two views bind the same entries");
        let Some(entries) = committed else {
            panic!("the join finds a tuple in the store");
        };
        assert_eq!(
            entries.len(),
            2,
            "the tuple binds one entry of each channel"
        );
    }

    /// Added by decision 124 (rule CR-36, C4v2 B.4, ruling RADD2v4 7.7): T-S1-C0-ITEM. c0 of a
    /// set join binds a class, and the commit tests the item of each member of that class, not
    /// only of the member that c0 binds. The class of c holds a1 and a2!!. An item test that
    /// admits every member gives the tuple of c0, the control. An item test that refuses only a
    /// persistent member leaves no tuple, although the member that c0 binds, a1, passes. The
    /// substitute of the commit of the join is a carrier fire: its probe asks the staged search
    /// for the tuple over the real lists of that commit.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn join_commit_tests_the_item_of_each_class_member_of_c0() {
        let (c, d, carrier) = (string("c"), string("d"), string("carrier"));
        let (host, env) = space_of(SpaceInner::Set).await;
        for (member, _) in ordered_class(&c, 1, &[false, true]) {
            run(&env, member).await;
        }
        run(&env, send(&d, 7, false)).await;
        run(&env, send(&carrier, 9, false)).await;
        let op = join(&[(&c, any()), (&d, any())], "w", false);
        let keys = scope_keys::<SetDiscipline>(&env, &c, &op).await;
        let (searches, ops) =
            staged_rows::<SetDiscipline>(&owner_funded(&env), SPACE, &c, op).await;
        let terms = take_terms(&ops, |channels, _| channels.len() == 2);
        let mut search = search_on(searches, &terms.group);
        let (control, refused) =
            commit_lists::<SetDiscipline, _, _>(&host, &env, &carrier, &keys, move |lists| {
                let matcher = Matcher;
                let admit = ItemRule(|_: &ListParWithRandom, _: bool| true);
                let control = search
                    .tuple(0, lists, request_of(&matcher, &terms, Some(&admit)))
                    .expect("the tuple under the item test that admits");
                let refuse = ItemRule(|_: &ListParWithRandom, persistent: bool| !persistent);
                let refused = search
                    .tuple(0, lists, request_of(&matcher, &terms, Some(&refuse)))
                    .expect("the tuple under the item test that refuses");
                (control, refused)
            })
            .await;
        let Some(control) = control else {
            panic!("the item test that admits every member keeps the tuple of c0");
        };
        assert_eq!(control.len(), 2, "c0 binds one record of each channel");
        assert!(
            matches!(refused, None),
            "a refused member of the class of c0 leaves no tuple: {refused:?}"
        );
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-20, errata V4-13, ruling RADD2v4 7.7):
    /// T-S8-BOOK. The cascade search of a tail read skips a pair that the book of the staging
    /// holds. The pair of a and b is free for a cascade. With an empty book the read finds it,
    /// so it stages one more step and its end is a stop. With the pair in the book of the row,
    /// the read finds nothing, so it ends. The substitute of the staged row is a row built by
    /// hand (`row_at`), and of the commit of the join a carrier fire whose probe reads the real
    /// lists of its commit.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn join_tail_read_reads_the_staging_book() {
        let (a, b, carrier) = (string("a"), string("b"), string("carrier"));
        let (host, env, _group, waiter) = fireable_pair(&a, &b).await;
        run(&env, send(&carrier, 9, false)).await;
        let record = host
            .get_data(&data_of(&a))
            .await
            .first()
            .expect("the record of a")
            .source
            .hash
            .clone();
        let pair = (record, waiter.source.hash.clone());
        let op = send(&a, 7, false);
        let mut built = None;
        join_plan::<QueueDiscipline>(&env, &a, &op, |cx, scope| {
            let inputs = inputs_of(cx, scope, &op);
            let at_a = scope.index_of_key(&key_of(&a))?;
            let before = before_at(scope.collections.len(), at_a, Own::InFlight, None);
            let row = row_at(cx, &inputs, offer_side(true), FireSite::Offer, before);
            built = Some((row, scope.lock_keys().to_vec(), at_a));
            Ok(())
        })
        .await;
        let Some((mut row, keys, at_a)) = built else {
            panic!("the plan built the row");
        };
        let volatile = Arc::clone(&env.volatile);
        let (unbooked, booked) =
            commit_lists::<QueueDiscipline, _, _>(&host, &env, &carrier, &keys, move |lists| {
                let queue: KeyedSource<'_, HashMapOuter, QueueDiscipline> = KeyedSource {
                    outer: std::marker::PhantomData,
                    discipline: &QueueDiscipline,
                };
                let unbooked = tail_answer(&mut row, &queue, lists, &[at_a]);
                volatile.book_pairs([pair]);
                row.book = volatile.book();
                let booked = tail_answer(&mut row, &queue, lists, &[at_a]);
                (unbooked, booked)
            })
            .await;
        let (TailRead::Stages, TailEnd::Stop(_)) = &unbooked else {
            panic!("the cascade search of the read finds the free pair: {unbooked:?}");
        };
        assert_eq!(
            booked,
            (TailRead::Ends, TailEnd::Return(None)),
            "the cascade search of the read skips the booked pair"
        );
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2v4 7.7): T-S8-REISSUE, the
    /// part of `drain_step` of `join_tail_end_equals_the_engine_end`. A step of the engine that
    /// owes a send stops at the round cap, a dirty collection with a free pair of a discipline
    /// with a cascade stages one more step, and a state with nothing owed gives the step of
    /// `after_drain`. The substitute of the staged row is a row built by hand (`row_at`), and of
    /// the commit of the join a carrier fire whose probe reads the real lists of its commit.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn join_tail_end_equals_the_engine_end_in_drain_step() {
        let (a, b, carrier) = (string("a"), string("b"), string("carrier"));
        let (host, env, _group, _waiter) = fireable_pair(&a, &b).await;
        run(&env, send(&carrier, 9, false)).await;
        let op = send(&a, 7, false);
        let mut built = None;
        join_plan::<QueueDiscipline>(&env, &a, &op, |cx, scope| {
            let inputs = inputs_of(cx, scope, &op);
            let at_a = scope.index_of_key(&key_of(&a))?;
            let width = scope.collections.len();
            let before = before_at(width, at_a, Own::InFlight, None);
            let row = row_at(cx, &inputs, offer_side(true), FireSite::Offer, before);
            built = Some((row, scope.lock_keys().to_vec(), width, at_a));
            Ok(())
        })
        .await;
        let Some((row, keys, width, at_a)) = built else {
            panic!("the plan built the row");
        };
        let answers =
            commit_lists::<QueueDiscipline, _, _>(&host, &env, &carrier, &keys, move |lists| {
                let queue: KeyedSource<'_, HashMapOuter, QueueDiscipline> = KeyedSource {
                    outer: std::marker::PhantomData,
                    discipline: &QueueDiscipline,
                };
                let bag: KeyedSource<'_, HashMapOuter, BagDiscipline> = KeyedSource {
                    outer: std::marker::PhantomData,
                    discipline: &BagDiscipline,
                };
                let in_flight = before_at(width, at_a, Own::InFlight, None);
                let not_started = before_at(width, at_a, Own::NotStarted, None);
                let marked = |before: &Before| {
                    let mut dirty = before.clone();
                    *dirty
                        .dirty
                        .get_mut(at_a)
                        .expect("the collection has a dirty mark") = true;
                    dirty
                };
                let marked_in_flight = marked(&in_flight);
                let marked_not_started = marked(&not_started);
                let owed = [Work {
                    coll: at_a,
                    record: ListParWithRandom::default(),
                    persist: false,
                    own: false,
                    restore: None,
                }];
                [
                    (
                        "an owed send owes a step",
                        drain_answer(&row, &queue, lists, &in_flight, &owed, 3),
                    ),
                    (
                        "the round cap stops an owed step",
                        drain_answer(&row, &queue, lists, &in_flight, &owed, FIRE_ROUND_CAP),
                    ),
                    (
                        "a dirty collection exposes the free pair",
                        drain_answer(&row, &queue, lists, &marked_in_flight, &[], 3),
                    ),
                    (
                        "the round cap stops the cascade",
                        drain_answer(&row, &queue, lists, &marked_in_flight, &[], FIRE_ROUND_CAP),
                    ),
                    (
                        "nothing is owed after the plan",
                        drain_answer(&row, &queue, lists, &in_flight, &[], 3),
                    ),
                    (
                        "the plan follows the settle",
                        drain_answer(&row, &queue, lists, &not_started, &[], 3),
                    ),
                    (
                        "the round cap stops the plan",
                        drain_answer(&row, &queue, lists, &not_started, &[], FIRE_ROUND_CAP),
                    ),
                    (
                        "a discipline without a cascade ends after the plan",
                        drain_answer(&row, &bag, lists, &marked_in_flight, &[], 3),
                    ),
                    (
                        "a discipline without a cascade goes on to the plan",
                        drain_answer(&row, &bag, lists, &marked_not_started, &[], 3),
                    ),
                ]
            })
            .await;
        let expected = [
            ("an owed send owes a step", TailStep::Stages),
            ("the round cap stops an owed step", TailStep::Capped),
            ("a dirty collection exposes the free pair", TailStep::Stages),
            ("the round cap stops the cascade", TailStep::Capped),
            ("nothing is owed after the plan", TailStep::Commits),
            ("the plan follows the settle", TailStep::Stages),
            ("the round cap stops the plan", TailStep::Capped),
            (
                "a discipline without a cascade ends after the plan",
                TailStep::Commits,
            ),
            (
                "a discipline without a cascade goes on to the plan",
                TailStep::Stages,
            ),
        ];
        assert_eq!(answers, expected, "the step of each state of `drain_step`");
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2v4 7.7): T-S8-REISSUE, the
    /// part of `tail_read` of `join_tail_end_equals_the_engine_end`. For a staged fire of the
    /// join engine at each site (an offer, a join receive, a single receive, a list receive,
    /// the pair cascade and the list cascade), a tail read after its commit fires c0 as staged
    /// gives the end of the engine: `Return` of the re-issue that the staging engine gave (a
    /// persistent send offered to a persistent waiter re-issues itself as a divergent pair), or
    /// `Stop` of the rest that it staged, for a fire that peeks and for the settle before the
    /// plan. The substitute of the commit of the fire: the staged rows come from a plan that
    /// commits nothing, the same operation then commits, and a carrier fire reads the store
    /// after that commit. For the list cascade a twin space gives the record and `put_datum`
    /// lays it beside the parked list waiter, because in a space a record never rests beside a
    /// parked list waiter that it could fire.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn join_tail_end_equals_the_engine_end_in_tail_read() {
        let (a, b, s, carrier) = (string("a"), string("b"), string("s"), string("carrier"));

        // An offer fires a parked join.
        {
            let (host, env) = space_of(SpaceInner::Queue).await;
            run(&env, join(&[(&a, any()), (&b, any())], "w", false)).await;
            run(&env, send(&b, 2, false)).await;
            run(&env, send(&carrier, 9, false)).await;
            let (group, waiter) = parked_on(&host, &a).await;
            let answer = read_after_commit::<QueueDiscipline>(
                (&host, &env),
                &carrier,
                &a,
                send(&a, 1, false),
                |searches, _| {
                    (
                        search_on(searches, &group),
                        terms_of_waiter(&group, &waiter),
                    )
                },
            )
            .await;
            assert_ends_as_staged(answer, 1, "an offer");
        }

        // A persistent send offered to a persistent waiter re-issues itself as a divergent pair.
        // The waiter parks first, so it holds the head of the queue. The join behind it keeps
        // the send in the join engine.
        {
            let (host, env) = space_of(SpaceInner::Queue).await;
            let SpaceOp::Consume(listening) = receive(&s, "k", any()) else {
                panic!("a receive is a consume");
            };
            let persistent = ConsumeOp {
                persistent: true,
                ..listening
            };
            let parked = run(&env, SpaceOp::Consume(persistent)).await;
            assert!(
                parked.fires.is_empty(),
                "the persistent waiter parks on the empty s"
            );
            let t = string("t");
            let behind = run(
                &env,
                join(&[(&s, exactly(-1)), (&t, exactly(-1))], "w", false),
            )
            .await;
            assert!(behind.fires.is_empty(), "the join parks behind the waiter");
            run(&env, send(&carrier, 9, false)).await;
            let group = vec![data_of(&s)];
            let waiter = host
                .get_waiting_continuations(group.clone())
                .await
                .into_iter()
                .next()
                .expect("the persistent waiter parks on s");
            let (read, end, outcome) = read_after_commit::<QueueDiscipline>(
                (&host, &env),
                &carrier,
                &s,
                send(&s, 1, true),
                |searches, _| {
                    (
                        search_on(searches, &group),
                        terms_of_waiter(&group, &waiter),
                    )
                },
            )
            .await;
            let Some(_) = &outcome.reissue else {
                panic!("the divergent pair re-issues the send");
            };
            assert_ends_as_staged((read, end, outcome), 1, "a divergent pair");
        }

        // A join receive fires the heads of its channels.
        {
            let (host, env) = space_of(SpaceInner::Queue).await;
            for (channel, value) in [(&a, 1), (&b, 2), (&carrier, 9)] {
                run(&env, send(channel, value, false)).await;
            }
            let answer = read_after_commit::<QueueDiscipline>(
                (&host, &env),
                &carrier,
                &a,
                join(&[(&a, any()), (&b, any())], "j", false),
                join_row,
            )
            .await;
            assert_ends_as_staged(answer, 1, "a join receive");
        }

        // A single receive of the join engine fires the head of its collection. The join parks
        // on s first, so the receive reaches the head on a stack, and never on a queue.
        {
            let stored = [send(&s, 1, false), send(&carrier, 9, false)];
            let (host, env) = joined_space(SpaceInner::Stack, &stored).await;
            let data = data_of(&s);
            let answer = read_after_commit::<StackDiscipline>(
                (&host, &env),
                &carrier,
                &s,
                receive(&s, "k", any()),
                |searches, ops| own_row(&data, searches, ops),
            )
            .await;
            assert_ends_as_staged(answer, 1, "a single receive");
        }

        // A list receive fires the list of its query.
        {
            let (host, env) = joined_vectordb_space().await;
            run(&env, vector_send(&s, [1.0, 0.0], 1, false)).await;
            run(&env, vector_send(&s, [1.0, 1.0], 2, false)).await;
            run(&env, vector_send(&carrier, [1.0, 0.0], 9, false)).await;
            let data = data_of(&s);
            let answer = read_after_commit::<vectordb::VectorDbDiscipline>(
                (&host, &env),
                &carrier,
                &s,
                list_receive(&s, "k"),
                |searches, ops| own_row(&data, searches, ops),
            )
            .await;
            assert_ends_as_staged(answer, 1, "a list receive");
        }

        // A receive spends the head of a, and the cascade fires the pair that it exposes. The
        // join parks on a first, so the receive reaches the head on a stack only. The top of a
        // is 5, so the join parks, and the fire of 5 exposes 1.
        {
            let (host, env) = space_of(SpaceInner::Stack).await;
            for (channel, value) in [(&a, 1), (&a, 5), (&b, 2), (&carrier, 9)] {
                run(&env, send(channel, value, false)).await;
            }
            run(
                &env,
                join(&[(&a, exactly(1)), (&b, exactly(2))], "w", false),
            )
            .await;
            let (group, waiter) = parked_on(&host, &a).await;
            let answer = read_after_commit::<StackDiscipline>(
                (&host, &env),
                &carrier,
                &a,
                receive(&a, "k", exactly(5)),
                |searches, _| {
                    (
                        search_on(searches, &group),
                        terms_of_waiter(&group, &waiter),
                    )
                },
            )
            .await;
            assert_ends_as_staged(answer, 2, "the pair cascade");
        }

        // A join that peeks owes the second sends of its records, so its end is a stop.
        {
            let (host, env) = space_of(SpaceInner::Queue).await;
            for (channel, value) in [(&a, 1), (&b, 2), (&carrier, 9)] {
                run(&env, send(channel, value, false)).await;
            }
            let (read, end, _outcome) = read_after_commit::<QueueDiscipline>(
                (&host, &env),
                &carrier,
                &a,
                join(&[(&a, any()), (&b, any())], "p", true),
                join_row,
            )
            .await;
            let (TailRead::Stages, TailEnd::Stop(SpaceOp::Settle(settle))) = (&read, &end) else {
                panic!("a join that peeks stages its second sends: {read:?} {end:?}");
            };
            let Some(cascade) = &settle.cascade else {
                panic!("the settle job carries the rest of the cascade");
            };
            assert_eq!(cascade.pending.len(), 2, "both second sends are owed");
        }

        // The settle before the plan fires a list waiter over the record beside it, and the
        // send of the operation is the rest that the fire leaves.
        {
            let (twin_host, twin) = joined_vectordb_space().await;
            run(&twin, vector_send(&s, [1.0, 0.0], 1, false)).await;
            let stored = twin_host
                .get_data(&data_of(&s))
                .await
                .into_iter()
                .next()
                .expect("the twin stores the record");
            let (host, env) = joined_vectordb_space().await;
            let parked = run(&env, list_receive(&s, "k")).await;
            assert!(
                parked.fires.is_empty(),
                "the list waiter parks on the empty s"
            );
            host.get_store().put_datum(&data_of(&s), stored);
            run(&env, vector_send(&carrier, [1.0, 0.0], 9, false)).await;
            let group = vec![data_of(&s)];
            let waiter = host
                .get_waiting_continuations(group.clone())
                .await
                .into_iter()
                .next()
                .expect("the list waiter parks on s");
            let op = vector_send(&s, [1.0, 1.0], 2, false);
            let (read, end, outcome) = read_after_commit::<vectordb::VectorDbDiscipline>(
                (&host, &env),
                &carrier,
                &s,
                op.clone(),
                |mut searches, _| {
                    (
                        searches
                            .pop()
                            .expect("the last search is the fire of the cascade"),
                        terms_of_waiter(&group, &waiter),
                    )
                },
            )
            .await;
            assert_eq!(
                (read, end),
                (TailRead::Stages, TailEnd::Stop(op)),
                "the settle before the plan leaves the send of the operation"
            );
            assert_eq!(outcome.fires.len(), 1, "the settle fires the list waiter");
        }
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD2 1.1 and RADD2v4 7.7):
    /// T-S8-REISSUE, the list-receive case of `join_tail_end_equals_the_engine_end`. A list
    /// receive whose commit fires another tuple than the staged one keeps in its `RowFire` the
    /// sends that `receive_list` pushes for that tuple. The commit view lists the records that
    /// the staging read, so the tuple that the commit finds is the one that the staging pushes
    /// its sends for. The substitute of a row whose staged tuple differs from the tuple of its
    /// commit is a row built by hand (`row_at`), with a staged tuple of the value 99 that the
    /// item test refuses, so the commit searches the list again over the real lists of a
    /// carrier commit.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn join_tail_end_equals_the_engine_end_for_a_list_receive() {
        let (s, carrier) = (string("s"), string("carrier"));
        let (host, env) = joined_vectordb_space().await;
        run(&env, vector_send(&s, [1.0, 0.0], 1, false)).await;
        run(&env, vector_send(&s, [1.0, 1.0], 2, false)).await;
        run(&env, vector_send(&carrier, [1.0, 0.0], 9, false)).await;
        let SpaceOp::Consume(listed) = list_receive(&s, "k") else {
            panic!("a list receive is a consume");
        };
        let consume = ConsumeOp {
            peek: true,
            ..listed
        };
        let op = SpaceOp::Consume(consume.clone());
        let source: KeyedSource<'_, HashMapOuter, vectordb::VectorDbDiscipline> = KeyedSource {
            outer: std::marker::PhantomData,
            discipline: &vectordb::VectorDbDiscipline,
        };
        let mut built = None;
        join_plan::<vectordb::VectorDbDiscipline>(&env, &s, &op, |cx, scope| {
            let mut engine = Engine::new(&source, scope, &op, cx.config())?;
            let coll = scope.index_of_key(&key_of(&s))?;
            *engine.ords.get_mut(coll).expect("s has an order slot") = Some(0);
            let waiter = engine.consume_waiter(&consume, &[coll], &[])?;
            let staged = Consume::create(
                &waiter.group,
                &waiter.patterns,
                &waiter.continuation,
                waiter.persist,
            );
            let terms = StagedRowTerms {
                group: waiter.group.clone(),
                patterns: waiter.patterns.clone(),
                continuation: waiter.continuation.clone(),
                persist: waiter.persist,
                source: staged.clone(),
            };
            let side = JoinSide::Receive {
                group: waiter.group.clone(),
                colls: waiter.colls.clone(),
                slots: waiter.slots.clone(),
                waiting: ParWaiter {
                    patterns: waiter.patterns.clone(),
                    continuation: waiter.continuation.clone(),
                    persist: waiter.persist,
                    peeks: waiter.peeks.clone(),
                    source: staged.clone(),
                },
            };
            let inputs = inputs_of(cx, scope, &op);
            let before = before_at(scope.collections.len(), coll, Own::InFlight, None);
            let mut row = row_at(cx, &inputs, side, FireSite::Single, before);
            let channel = data_of(&s);
            let refused = ListParWithRandom {
                pars: vec![int(99)],
                ..ListParWithRandom::default()
            };
            row.staged = staged;
            row.tuple = vec![receive_candidate(channel.clone(), ReceiveFire {
                head: Datum {
                    a: refused.clone(),
                    persist: false,
                    source: Produce::create(&channel, &refused, false),
                },
                matched: refused,
                view_index: 0,
            })];
            engine.receive(cx, waiter)?;
            built = Some((
                row,
                terms,
                scope.lock_keys().to_vec(),
                work_fields(&engine.work),
            ));
            Ok(())
        })
        .await;
        let Some((mut row, terms, keys, staged_sends)) = built else {
            panic!("the plan built the row");
        };
        let (found, fire_sends) = commit_lists::<vectordb::VectorDbDiscipline, _, _>(
            &host,
            &env,
            &carrier,
            &keys,
            move |lists| {
                let matcher = Matcher;
                let refuses = ItemRule(|datum: &ListParWithRandom, _: bool| {
                    datum.pars.first() != Some(&int(99))
                });
                let request = request_of(&matcher, &terms, Some(&refuses));
                let keyed: KeyedSource<'_, HashMapOuter, vectordb::VectorDbDiscipline> =
                    KeyedSource {
                        outer: std::marker::PhantomData,
                        discipline: &vectordb::VectorDbDiscipline,
                    };
                let width = row.inputs.scope.collections.len();
                let mut view = CommitView::new(lists, &matcher, None, request.item, width);
                let found = row
                    .tuple(&keyed, &mut view, &request)
                    .expect("the tuple of the row")
                    .map(|found| (found.tuple.len(), found.staged));
                let Some(last) = &row.last else {
                    panic!("the row kept the fire of the tuple that it found");
                };
                (found, work_fields(&last.sends))
            },
        )
        .await;
        assert_eq!(
            found,
            Some((1, false)),
            "the commit fires another tuple than the staged one"
        );
        assert_eq!(
            fire_sends, staged_sends,
            "the sends of the fire equal the sends that `receive_list` pushes"
        );
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2v2 6.4): T-LIFT-END. The
    /// end of Tail(actual) of a refused cascade lift is the stop of the engine at the lift for an
    /// owner refusal: one settle job whose seeds are the dirty collections but the own one, with
    /// no owed send, and with the re-issue that the commit gives for the state of the element of
    /// the operation. The cases: a divergent pair after the plan, the settle before the plan, a
    /// settled element, and a settle job that carries the re-issue of its operation. A state
    /// that owes nothing fails closed.
    #[test]
    fn refused_lift_end_is_the_stop_of_the_engine_at_the_lift() {
        let (a, b) = (string("a"), string("b"));
        let (key_a, key_b) = (key_of(&a), key_of(&b));
        let mut collections = Vec::<ScopedCollection>::with_capacity(2);
        collections.push(scoped_collection(&HashMapOuter, SPACE, key_a.clone()));
        collections.push(scoped_collection(&HashMapOuter, SPACE, key_b.clone()));
        collections.sort_by(|left, right| left.key.cmp(&right.key));
        let scope = JoinScope::new(
            SPACE,
            collections,
            vec![key_a.clone()],
            vec![0],
            Par::default(),
            Vec::new(),
            false,
        );
        let at_a = scope.index_of_key(&key_a).expect("a is in the scope");
        let at_b = scope.index_of_key(&key_b).expect("b is in the scope");
        let config = SpaceConfig::new(
            SPACE.sid,
            SpaceOuter::HashMap.as_str(),
            SpaceInner::Queue.as_str(),
            SpaceQualifier::Default,
        );
        let queue: KeyedSource<'_, HashMapOuter, QueueDiscipline> = KeyedSource {
            outer: std::marker::PhantomData,
            discipline: &QueueDiscipline,
        };
        let send_op = send(&a, 1, true);
        let SpaceOp::Produce(produce) = send_op.clone() else {
            panic!("a send is a produce");
        };
        let mut ords = Vec::<Option<i64>>::with_capacity(2);
        ords.resize(2, None);
        *ords.get_mut(at_a).expect("a has an order slot") = Some(7);
        let state = |colls: &[usize], own: Own, own_cause: Option<ReissueCause>| {
            let mut dirty = Vec::<bool>::with_capacity(2);
            dirty.resize(2, false);
            mark(&mut dirty, colls);
            Before {
                fired: 3,
                changed: true,
                dirty,
                own,
                own_cause,
                ords: ords.clone(),
            }
        };
        let again = |cause: ReissueCause| SpaceReissue {
            op: SpaceOp::Produce(ProduceOp {
                reissued_ord: Some(7),
                ..produce.clone()
            }),
            cause,
        };
        let settle_job = |seeds: Vec<CollectionKey>, resume: Option<SpaceReissue>| {
            SpaceOp::Settle(SettleOp {
                cascade: Some(Box::new(CascadeState {
                    seeds,
                    pending: Vec::new(),
                    resume: resume.map(Box::new),
                })),
                ..settle_operation(generic::OpView::Produce(&produce))
            })
        };
        // Added by decision 124 (plan v4 C7.11, rule CR-33, ruling RADD2v2 6.6): the parts of
        // the run that the finish rows read (item J-E). The end of a refused lift reads none
        // of them.
        let random = Blake2b512Random::create_from_bytes(b"lift end test");
        let introduction = Introduction::new(
            &RuntimeBudget::new(Cost::create(1_000, "lift end test")),
            &CostAuthority::default(),
        );
        let job = SpaceJob {
            space: SPACE,
            random_state: random.to_bytes(),
            authority: None,
        };
        let inputs = EngineInputs {
            op: Arc::new(send_op),
            scope: scope.clone(),
            config: config.clone(),
            random: random.clone(),
            authority: None,
            introduction: Arc::clone(&introduction),
            job: job.clone(),
        };
        // A divergent pair after the plan (A10): the settle job re-issues the send with its cause.
        assert_eq!(
            refused_lift_end(
                &queue,
                &inputs,
                &state(
                    &[at_a, at_b],
                    Own::InFlight,
                    Some(ReissueCause::DivergentPair)
                ),
            ),
            Ok(TailEnd::Stop(settle_job(
                vec![key_b.clone()],
                Some(again(ReissueCause::DivergentPair)),
            )))
        );
        // The settle before the plan: the settle job re-issues the send for the owner refusal.
        assert_eq!(
            refused_lift_end(
                &queue,
                &inputs,
                &state(&[at_a, at_b], Own::NotStarted, None)
            ),
            Ok(TailEnd::Stop(settle_job(
                vec![key_b.clone()],
                Some(again(ReissueCause::OwnerRefusal)),
            )))
        );
        // A settled element: the settle job carries the cascade alone.
        assert_eq!(
            refused_lift_end(&queue, &inputs, &state(&[at_a], Own::Settled, None)),
            Ok(TailEnd::Stop(settle_job(Vec::new(), None)))
        );
        // A settle job that carries the re-issue of its operation carries it on.
        let carried = settle_job(
            vec![key_b.clone()],
            Some(again(ReissueCause::DivergentPair)),
        );
        let settle_inputs = EngineInputs {
            op: Arc::new(carried.clone()),
            scope,
            config,
            random,
            authority: None,
            introduction,
            job,
        };
        assert_eq!(
            refused_lift_end(
                &queue,
                &settle_inputs,
                &state(&[at_a, at_b], Own::Settled, None)
            ),
            Ok(TailEnd::Stop(carried))
        );
        // A state that owes nothing fails closed.
        assert_eq!(
            refused_lift_end(&queue, &inputs, &state(&[], Own::Settled, None)),
            Err(SpaceError::Inconsistent(
                "a join row owes a step that no re-issue carries"
            ))
        );
    }
}
