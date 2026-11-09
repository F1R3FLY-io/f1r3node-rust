//! The prefix source of the join engine (P5e, Phase 2 of wave W3; joins plan section 9; the
//! seams design with its arbitration; decision 51).
//!
//! Every operation of a PathMap space runs on the join engine of `spaces::join`, also a
//! single-bind one. [`PrefixSource`] implements the store seam [`JoinSource`] with the pure plans
//! of `pathmap::delivery` and `pathmap::markers`. A join over prefixes of one space (wave W4) runs
//! on the same engine.
//!
//! | Role in the engine | PathMap space |
//! |---|---|
//! | a collection of the scope | a prefix `q`, keyed `[0x49] ‖ prefix_key(q)`, on its index channel `I(q)` |
//! | a window of a bind on `q` | for each path `p ⪰ q` in path-key order, the window of the discipline on `D(p)` |
//! | the payload of a stored entry | `R(p∖q, payload(d))` of the datum `d` of the marker |
//! | the incoming side of a fire on `q` | the vehicle `v_q(x)` on `I(q)`, which no store keeps |
//! | the home of an element | the data channel `D(p)` of its path `p` |
//! | a store | STORE-MARKERS: `D(p)`, then one marker on each `I(q′)`, `q′ ⪯ p`, in depth order |
//! | the spend of a stored datum | RETIRE-PLAN: `K_retire` of `D(p)` and of each other marker |
//! | the second send of a peek | the datum, offered to PRODUCE-WAITERS(`p`), else its marker restored |
//! | the counter | the token `S(sid)`, one counter for every prefix |
//!
//! ```text
//! PREFIX-SCOPE(op):
//!   own ← the prefix of each channel of op (a send also adds every prefix of its path)
//!   repeat until no prefix is added:
//!     each channel of each join group of I(q), q in the scope
//!     when a prefix of the scope holds a marker: every prefix of every stored datum
//!   lock keys ← C(sid), S(sid), then D(p) of each data path and I(q) of each prefix
//! WINDOW(q, need):                                  ▷ the in-flight datums are left out
//!   for each path p ⪰ q in path-key order: the discipline window of need entries on D(p)
//! ```
//!
//! **In flight.** A second send of a peek and a lifted element keep their record and the other
//! markers in the store while they are in flight. So every window, every member list and every
//! count leaves out the element in flight and every owed second send with a `restore` depth
//! (seams arbitration, section 5).
//!
//! **The cap.** A transaction that stops at `FIRE_ROUND_CAP` restores in `finish` the marker of
//! each second send that it still owes. The settle job then offers a datum that the store holds
//! in full. When another job of the deploy spent that datum first, the settle job drops the
//! offer. So no other job sees a datum without one of its markers (open item O-2).
//!
//! **The token.** An ordered discipline bumps the token once in each transaction (decision 31).
//! A bag or a set bumps it once in `finish`, and only when the transaction changed the state
//! (decision 51).

use std::borrow::Cow;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::BTreeMap;
// Changed by decision 124 (plan v4 C7.9, ruling RADD 1.5): the PathMap row search shares the
// inputs of the run. The text before:
// use std::sync::LazyLock;
use std::sync::{Arc, LazyLock};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
// Changed by decision 124 (plan v4 C7.9, ruling RADD 1.5): the PathMap row search names the
// waiter of its row. The text before:
// use models::rhoapi::{BindPattern, CostAuthority, ListParWithRandom, Par};
use models::rhoapi::{BindPattern, CostAuthority, ListParWithRandom, Par, TaggedContinuation};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::internal::{ConsumeCandidate, Datum};
// Added by decision 124 (plan v4 C7.9): a row search matches in the store of the commit.
use rspace_plus_plus::rspace::r#match::Match;
// Added by decision 102 (plan v4 C5.5): the source of a waiter pick inside one prefix.
use rspace_plus_plus::rspace::rspace_interface::RSpaceOperationSource;
// Added by decision 124 (plan v4 C7.9, ruling RADD 1.5): the PathMap row search of a fire.
use rspace_plus_plus::rspace::space_store_handle::{
    canonical_continuation_order, ActualFire, FinishRow, RowClaims, RowItemRule, RowReissue,
    RowRun, RowSearch, SpaceTxnError, TailRead,
};
use rspace_plus_plus::rspace::trace::event::Produce;

use super::adapter::HeadEPathMapAdapter;
use super::delivery::{
    consume_candidates_excluding, datum_marker, datum_markers, is_excluded, rerooted, retire_plan,
    PathCandidates, PrefixView, Retired,
};
use super::markers::{decode_marker, vehicle};
use super::path_codec::{channel_path, compare_keys, encode_path, is_prefix_key, path_key};
use super::EncodedPath;
use crate::rust::interpreter::spaces::agent::{
    ConsumeOp, JoinOp, Prepared, ProduceOp, SettleOp, SpaceEnv, SpaceOp, SpaceOutcome,
};
// Changed by decision 124 (plan v4 C7.11, rule CR-33, ruling RADD2v2 6.6): the finish rows of a
// PathMap cut row build the bump of the token (P1). The text before:
// use crate::rust::interpreter::spaces::counter::CounterChannels;
use crate::rust::interpreter::spaces::counter::{
    collection_bump, CounterChannels, COUNTER_PATTERN,
};
// Changed by decision 128 (kind routing): the lanes window owns the level and the lane. The text
// before:
// use crate::rust::interpreter::spaces::disciplines::priority_queue::{Lane, Level};
use crate::rust::interpreter::spaces::disciplines::lanes::{Lane, Level};
// Changed by decision 124 (plan v4 C7.9, ruling RADD 1.5): the PathMap row search names the
// members that it retires. The text before:
// ```
// use crate::rust::interpreter::spaces::disciplines::scan::{
//     retire_continuation, retire_pattern, same_class,
// };
// ```
use crate::rust::interpreter::spaces::disciplines::scan::{
    retire_continuation, retire_pattern, same_class, Member,
};
// Added by decision 124 (plan v4 C7.9, ruling RADD 1.8): the claims of the PathMap retires.
use crate::rust::interpreter::spaces::ephemeral::{ClaimRole, SeqReads};
use crate::rust::interpreter::spaces::errors::SpaceError;
// Changed by decision 124 (plan v4 C7.9, ruling OP-J): the planner view reads any read view.
// The text before:
// ```
// use crate::rust::interpreter::spaces::generic::{
//     CollectionTxn, Cx, Discipline, GenericSpace, JoinWindow, Ordered, OuterStore, ParCandidate,
//     ParDatum, ParWaiter, Side, SideBound,
// };
// ```
// Changed by decision 124 (plan v4 C7.9, ruling RADD 1.5): the join row searches name these too.
use crate::rust::interpreter::spaces::generic::{
    store_request, CollectionTxn, CommitView, Cx, Discipline, GenericSpace, JoinWindow, Ordered,
    OuterStore, ParCandidate, ParDatum, ParFinishRow, ParRowLists, ParRowRequest, ParRowSearch,
    ParWaiter, RowEnd, Side, SideBound, StoreView,
};
// Changed by decision 124 (plan v4 C7.9 and C7.11, rulings RADD 1.5 and RADD2v2 6.6): the
// PathMap row search shares the parts of the join row. The text before:
// ```
// use crate::rust::interpreter::spaces::join::{
//     self, Bound, Claim, Entry, EntryAt, FireSite, Incoming, JoinRoute, JoinScope, JoinSource,
//     Placement, ScopedCollection, StoredEntry, WaiterRef, Work, PREFIX_KEY_TAG,
// };
// ```
use crate::rust::interpreter::spaces::join::{
    self, commit_members, join_commit_error, refused_lift_end, Bound, Claim, EngineInputs, Entry,
    EntryAt, FireSite, Incoming, JoinRoute, JoinRow, JoinScope, JoinSource, LiftCut, LiftRow,
    Placement, RetiredMember, RowUnit, ScopedCollection, StoredEntry, WaiterRef, Work,
    PREFIX_KEY_TAG,
};
use crate::rust::interpreter::spaces::namespace::{
    self, CollectionKey, NativeSpace, RandomPurpose, ReifiedId,
};
use crate::rust::interpreter::spaces::outer::pathmap::PathMapOuter;
use crate::rust::interpreter::spaces::record::{
    self, bincode_digest, JoinSlot, OperationAuthority, WaiterPolicy,
};
// Added by decision 124 (plan v2 C7.10): the reader skips inert machinery waiters.
use crate::rust::interpreter::spaces::settle::machinery_shape;
use crate::rust::interpreter::spaces::types::config::SpaceConfig;

/// The terminator byte of a path key.
const TERMINATOR: u8 = 0x00;

/// The template of the retire pattern.
static RETIRE_PATTERN: LazyLock<BindPattern> = LazyLock::new(retire_pattern);

// ── Routing ─────────────────────────────────────────────────────────────────────────────────

/// CLASSIFY of a PathMap operation (joins plan section 9): the checks of the operation, then
/// `Join` with its prefix scope. Every PathMap operation runs on the join engine, a single-bind
/// one included. Changed by decision 94: the classification runs under phase A, and phase B
/// covers the own lock keys and the lock keys of the scope (`generic::prepare_collection_op`).
/// Before, `PathMapOuter::ALWAYS_ESCALATES` ran the operation in a new transaction over the lock
/// keys of the scope, because the first transaction locked only the channels of the routed key.
pub(crate) fn classify<D: Discipline>(
    agent: &GenericSpace<PathMapOuter, D>,
    opened: &mut CollectionTxn,
    op: &SpaceOp,
) -> Result<JoinRoute, SpaceError> {
    validate(agent, &opened.config, op)?;
    prefix_scope(opened, op).map(JoinRoute::Join)
}

/// RUN-JOIN of a PathMap operation (joins plan section 9): the key of each channel of `op`
/// becomes the scope key of its path, and the engine runs `op` with the prefix source.
pub(crate) fn run_join<D: Discipline>(
    agent: &GenericSpace<PathMapOuter, D>,
    env: &SpaceEnv,
    opened: CollectionTxn,
    scope: JoinScope,
    op: SpaceOp,
) -> Result<Prepared<SpaceOutcome>, SpaceError> {
    let op = scoped_op(op)?;
    let source = PrefixSource::new(agent.discipline(), &scope, &op)?;
    join::run_join_with(env, opened, scope, op, &source)
}

/// The checks of a PathMap operation before any staging: the path rules of each channel
/// (`PathMapOuter::check_key`), the payload and pattern hooks of the outer store (which accept
/// everything in a PathMap store), and the params and the query with the schema of the
/// discipline. A join needs two or more binds, one arrow, and one carried order for each bind
/// when it carries orders. They are the checks that the keyed engine makes (`spaces::join`).
fn validate<D: Discipline>(
    agent: &GenericSpace<PathMapOuter, D>,
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
            }
            Ok(())
        }
    }
}

/// The operation `op` with the scope key of the path of each of its channels as its key. The
/// engine finds the own collections of the operation in the scope by these keys (seams
/// arbitration, section 2). A re-issue carries them, and the classification computes them again
/// from the channels.
fn scoped_op(op: SpaceOp) -> Result<SpaceOp, SpaceError> {
    Ok(match op {
        SpaceOp::Produce(produce) => {
            let key = channel_key(&produce.channel)?;
            SpaceOp::Produce(ProduceOp { key, ..produce })
        }
        SpaceOp::Consume(consume) => {
            let key = channel_key(&consume.channel)?;
            SpaceOp::Consume(ConsumeOp { key, ..consume })
        }
        SpaceOp::Settle(settle) => {
            let key = channel_key(&settle.channel)?;
            SpaceOp::Settle(SettleOp { key, ..settle })
        }
        SpaceOp::Join(join) => {
            let mut binds = join.binds;
            for bind in binds.iter_mut() {
                bind.key = channel_key(&bind.channel)?;
            }
            SpaceOp::Join(JoinOp { binds, ..join })
        }
    })
}

// ── Keys ────────────────────────────────────────────────────────────────────────────────────

/// The scope key `[0x49] ‖ prefix_key(path)` of the path of `channel`.
fn channel_key(channel: &Par) -> Result<CollectionKey, SpaceError> {
    let encoded = encode_path(channel_path(channel)?)?;
    scope_key(full_prefix(&encoded)?)
}

/// The prefix key of the whole path that `encoded` holds: the path key without its terminator.
fn full_prefix(encoded: &EncodedPath) -> Result<&[u8], SpaceError> {
    encoded
        .prefix_key(encoded.depth())
        .ok_or(SpaceError::Inconsistent(
            "an encoded path has no prefix key of its depth",
        ))
}

/// The scope key `[0x49] ‖ prefix_key` of a prefix. The key of the prefix `ε` is `[0x49]`, so no
/// scope key is empty.
fn scope_key(prefix_key: &[u8]) -> Result<CollectionKey, SpaceError> {
    let length = prefix_key.len() + 1;
    let mut bytes = Vec::<u8>::with_capacity(length);
    bytes.push(PREFIX_KEY_TAG);
    bytes.extend_from_slice(prefix_key);
    CollectionKey::from_bytes(bytes).ok_or(SpaceError::Inconsistent("a scope key is empty"))
}

/// The prefix key of the scope key `key`: the key without its tag. A key without the tag is not
/// a key of a PathMap scope, so it is `Inconsistent`.
fn prefix_of_key(key: &CollectionKey) -> Result<&[u8], SpaceError> {
    match key.as_bytes().split_first() {
        Some((&PREFIX_KEY_TAG, rest)) => Ok(rest),
        Some(_) | None => Err(SpaceError::Inconsistent(
            "a scope key of a PathMap space has no prefix tag",
        )),
    }
}

/// The scoped collection `coll`.
fn collection(scope: &JoinScope, coll: usize) -> Result<&ScopedCollection, SpaceError> {
    scope.collections.get(coll).ok_or(SpaceError::Inconsistent(
        "a collection is outside its join scope",
    ))
}

/// The prefix key of the scoped collection `coll`.
fn prefix_at(scope: &JoinScope, coll: usize) -> Result<&[u8], SpaceError> {
    prefix_of_key(&collection(scope, coll)?.key)
}

/// The index in the scope of the prefix whose prefix key is `prefix_key`. The collections are in
/// ascending key order, so the lookup is a binary search.
fn coll_of(scope: &JoinScope, prefix_key: &[u8]) -> Result<usize, SpaceError> {
    scope
        .collections
        .binary_search_by(|collection| match collection.key.as_bytes().split_first() {
            Some((&PREFIX_KEY_TAG, rest)) => rest.cmp(prefix_key),
            Some((&first, _)) => first.cmp(&PREFIX_KEY_TAG),
            None => Ordering::Less,
        })
        .map_err(|_| SpaceError::Inconsistent("a prefix is outside its join scope"))
}

/// The path key `prefix_key ‖ 0x00` of the path whose prefix key is `prefix_key`.
fn terminated(prefix_key: &[u8]) -> Vec<u8> {
    let length = prefix_key.len() + 1;
    let mut key = Vec::<u8>::with_capacity(length);
    key.extend_from_slice(prefix_key);
    key.push(TERMINATOR);
    key
}

/// The key `left ‖ right` in one buffer: the path key of `p = q ++ s` from the prefix key
/// of `q` and the path key of `s` (`PathMapPrefix.v`, `path_key_app`).
fn joined(left: &[u8], right: &[u8]) -> Vec<u8> {
    let length = left.len() + right.len();
    let mut key = Vec::<u8>::with_capacity(length);
    key.extend_from_slice(left);
    key.extend_from_slice(right);
    key
}

/// The depth of the prefix of the path that `encoded` holds whose prefix key is `prefix_key`.
/// A key that is not a prefix key of the path is `Inconsistent`.
fn depth_of(encoded: &EncodedPath, prefix_key: &[u8]) -> Result<usize, SpaceError> {
    for depth in 0..=encoded.depth() {
        let key = encoded.prefix_key(depth).ok_or(SpaceError::Inconsistent(
            "an encoded path has no prefix key of a depth",
        ))?;
        if key == prefix_key {
            return Ok(depth);
        }
    }
    Err(SpaceError::Inconsistent(
        "a prefix key is not a prefix of its path",
    ))
}

/// The suffix `p∖q` of the path `path` under its prefix whose key is `prefix_key`.
fn suffix_under(path: &[Par], prefix_key: &[u8]) -> Result<Vec<Par>, SpaceError> {
    let encoded = encode_path(path)?;
    let depth = depth_of(&encoded, prefix_key)?;
    let suffix = path
        .get(depth..)
        .ok_or(SpaceError::Inconsistent("a prefix is deeper than its path"))?;
    Ok(suffix.to_vec())
}

/// The depth `depth` as the `restore` slot of a second send. A prefix is at most
/// `PATH_DEPTH_CAP` deep, so a depth that does not fit a byte is `Inconsistent`.
fn restore_depth(depth: usize) -> Result<u8, SpaceError> {
    u8::try_from(depth).map_err(|_| SpaceError::Inconsistent("a marker depth exceeds a byte"))
}

/// The view index of the record at `index`.
fn view_index(index: usize) -> Result<i32, SpaceError> {
    i32::try_from(index).map_err(|_| SpaceError::Inconsistent("a record index exceeds i32"))
}

/// The index of the record whose source hash is `source` among `records`. A missing record is
/// `Inconsistent`.
fn locate(records: &[ParDatum], source: &Blake2b256Hash) -> Result<usize, SpaceError> {
    for (index, record) in records.iter().enumerate() {
        if record.source.hash == *source {
            return Ok(index);
        }
    }
    Err(SpaceError::Inconsistent(
        "a marker names no datum of its path",
    ))
}

/// The index of the marker of the datum `source` among the records `markers` of one index
/// channel. A missing marker breaks the marker invariant and is `Inconsistent`.
fn locate_marker(markers: &[ParDatum], source: &Blake2b256Hash) -> Result<usize, SpaceError> {
    for (index, record) in markers.iter().enumerate() {
        let view = decode_marker(&record.a)?;
        if view.is_of(source) {
            return Ok(index);
        }
    }
    Err(SpaceError::Inconsistent(
        "a stored datum has no marker on a prefix of its path",
    ))
}

// ── The prefix scope ────────────────────────────────────────────────────────────────────────

/// The prefixes of a scope while the closure grows: each prefix key with a flag that is true when
/// the prefix is also the path of a stored datum or of the operation, so its data channel `D(p)`
/// joins the lock keys.
struct Closure {
    space: NativeSpace,
    prefixes: BTreeMap<Vec<u8>, bool>,
    todo: Vec<Vec<u8>>,
    marked: bool,
    rooted: bool,
}

impl Closure {
    fn new(space: NativeSpace) -> Self {
        Closure {
            space,
            prefixes: BTreeMap::new(),
            todo: Vec::new(),
            marked: false,
            rooted: false,
        }
    }

    /// Adds the prefix `prefix_key`, and marks it as a data path when `data` is true.
    fn add_prefix(&mut self, prefix_key: &[u8], data: bool) {
        match self.prefixes.get_mut(prefix_key) {
            Some(flag) => {
                *flag = *flag || data;
            }
            None => {
                let key = prefix_key.to_vec();
                self.todo.push(prefix_key.to_vec());
                self.prefixes.insert(key, data);
            }
        }
    }

    /// Adds every prefix of the path `path`, and marks the whole path as a data path.
    fn add_path(&mut self, path: &[Par]) -> Result<(), SpaceError> {
        let encoded = encode_path(path)?;
        for prefix_key in encoded.prefix_keys() {
            self.add_prefix(prefix_key?, false);
        }
        self.add_prefix(full_prefix(&encoded)?, true);
        Ok(())
    }

    /// The own prefixes of `op` in canonical group order, each once, and the positions of its
    /// binds in canonical group order (FR-1'): ascending by the stable hash of the index channel
    /// of each bind, then by bind index. A produce adds every prefix of its path, and a settle job
    /// adds its seeds and the paths of its owed second sends.
    fn own(&mut self, op: &SpaceOp) -> Result<(Vec<Vec<u8>>, Vec<usize>), SpaceError> {
        let mut single = Vec::<usize>::with_capacity(1);
        single.push(0);
        match op {
            SpaceOp::Produce(produce) => {
                let path = channel_path(&produce.channel)?;
                let encoded = encode_path(path)?;
                self.add_path(path)?;
                let mut own = Vec::<Vec<u8>>::with_capacity(1);
                own.push(full_prefix(&encoded)?.to_vec());
                Ok((own, single))
            }
            SpaceOp::Consume(consume) => {
                let own = self.own_channel(&consume.channel)?;
                Ok((own, single))
            }
            SpaceOp::Settle(settle) => {
                let own = self.own_channel(&settle.channel)?;
                if let Some(cascade) = &settle.cascade {
                    for seed in &cascade.seeds {
                        self.add_prefix(prefix_of_key(seed)?, false);
                    }
                    for pending in &cascade.pending {
                        self.add_prefix(prefix_of_key(&pending.key)?, true);
                    }
                }
                Ok((own, single))
            }
            SpaceOp::Join(join) => {
                let mut prefixes = Vec::<Vec<u8>>::with_capacity(join.binds.len());
                let mut hashed = Vec::<(Blake2b256Hash, usize)>::with_capacity(join.binds.len());
                for (index, bind) in join.binds.iter().enumerate() {
                    let encoded = encode_path(channel_path(&bind.channel)?)?;
                    let prefix_key = full_prefix(&encoded)?;
                    self.add_prefix(prefix_key, false);
                    let channel = PathMapOuter::index(self.space, prefix_key);
                    hashed.push((bincode_digest(&channel)?, index));
                    prefixes.push(prefix_key.to_vec());
                }
                hashed.sort();
                let mut group = Vec::<usize>::with_capacity(hashed.len());
                group.extend(hashed.into_iter().map(|(_, index)| index));
                let mut own = Vec::<Vec<u8>>::with_capacity(group.len());
                for &position in &group {
                    let prefix_key = prefixes
                        .get(position)
                        .ok_or(SpaceError::Inconsistent("a group position names no bind"))?;
                    let mut seen = false;
                    for known in own.iter() {
                        if known == prefix_key {
                            seen = true;
                            break;
                        }
                    }
                    if !seen {
                        own.push(prefix_key.to_vec());
                    }
                }
                Ok((own, group))
            }
        }
    }

    /// The own prefix of the channel `channel` of a receive or a settle: the prefix of its whole
    /// path.
    fn own_channel(&mut self, channel: &Par) -> Result<Vec<Vec<u8>>, SpaceError> {
        let encoded = encode_path(channel_path(channel)?)?;
        let prefix_key = full_prefix(&encoded)?;
        self.add_prefix(prefix_key, false);
        let mut own = Vec::<Vec<u8>>::with_capacity(1);
        own.push(prefix_key.to_vec());
        Ok(own)
    }

    /// The closure: each channel of each join group of each prefix, and, once a prefix of the
    /// closure holds a marker, every prefix of every stored datum. The root `I(ε)` holds one
    /// marker for each stored datum with its whole path (the marker invariant), so one read of
    /// the root gives every datum.
    fn close(&mut self, opened: &mut CollectionTxn) -> Result<(), SpaceError> {
        loop {
            while let Some(prefix_key) = self.todo.pop() {
                let channel = PathMapOuter::index(self.space, &prefix_key);
                let groups: Vec<Vec<Par>> = {
                    let joins = opened.txn.joins(&channel)?;
                    joins.to_vec()
                };
                for group in &groups {
                    for member in group {
                        match namespace::reified_id_of(member) {
                            Some(ReifiedId::PathIndex(owner, tail)) if owner == self.space => {
                                let tail = tail.to_vec();
                                self.add_prefix(&tail, false);
                            }
                            _ => {
                                return Err(SpaceError::Inconsistent(
                                    "a collection channel is part of a foreign join",
                                ))
                            }
                        }
                    }
                }
                if !opened.txn.data(&channel)?.is_empty() {
                    self.marked = true;
                }
            }
            match self.marked && !self.rooted {
                true => {
                    self.rooted = true;
                    let root = PathMapOuter::index(self.space, &[]);
                    let paths: Vec<Vec<Par>> = {
                        let markers = opened.txn.data(&root)?;
                        let mut paths = Vec::<Vec<Par>>::with_capacity(markers.len());
                        for marker in markers {
                            let view = decode_marker(&marker.a)?;
                            paths.push(view.suffix.to_vec());
                        }
                        paths
                    };
                    self.add_prefix(&[], false);
                    for path in &paths {
                        self.add_path(path)?;
                    }
                }
                false => return Ok(()),
            }
        }
    }

    /// The scope of the closure: one collection for each prefix in ascending key order, with its
    /// index channel and the token, and the lock keys `C(sid)`, `S(sid)`, then for each prefix
    /// its data channel when it is a data path and its index channel.
    fn scope(
        self,
        opened: &CollectionTxn,
        own: Vec<Vec<u8>>,
        group: Vec<usize>,
    ) -> Result<JoinScope, SpaceError> {
        let data_paths = self.prefixes.values().filter(|data| **data).count();
        let length = self.prefixes.len() + data_paths + 2;
        let mut lock_keys = Vec::<Par>::with_capacity(length);
        lock_keys.push(opened.channels.config().clone());
        lock_keys.push(opened.channels.space_counter().clone());
        let token = PathMapOuter::token(self.space);
        let mut collections = Vec::<ScopedCollection>::with_capacity(self.prefixes.len());
        for (prefix_key, data) in &self.prefixes {
            if *data {
                let path_key = terminated(prefix_key);
                lock_keys.push(PathMapOuter::path_data(self.space, &path_key));
            }
            let index = PathMapOuter::index(self.space, prefix_key);
            lock_keys.push(index.clone());
            collections.push(ScopedCollection {
                key: scope_key(prefix_key)?,
                data: index,
                counter: token.clone(),
            });
        }
        let mut own_keys = Vec::<CollectionKey>::with_capacity(own.len());
        for prefix_key in &own {
            own_keys.push(scope_key(prefix_key)?);
        }
        Ok(JoinScope::new(
            self.space,
            collections,
            own_keys,
            group,
            token,
            lock_keys,
            true,
        ))
    }
}

/// PREFIX-SCOPE (seams design, section 4): the own prefixes of `op`, the closure of their join
/// groups and of the stored datums, and the lock keys of the closure.
fn prefix_scope(opened: &mut CollectionTxn, op: &SpaceOp) -> Result<JoinScope, SpaceError> {
    let mut closure = Closure::new(op.space());
    let (own, group) = closure.own(op)?;
    closure.close(opened)?;
    closure.scope(opened, own, group)
}

// ── The prefix source ───────────────────────────────────────────────────────────────────────

/// A second send of this transaction whose marker of depth `depth` a peek COMM or a lift removed.
/// Its datum lives on the path of the scoped collection `home`.
// Added by decision 124 (plan v4 C7.9, ruling RADD 1.5): a row search keeps a copy of the owed
// second sends.
#[derive(Clone)]
struct Owed {
    source: Blake2b256Hash,
    depth: usize,
    home: usize,
}

/// The state of a prefix source in one transaction.
#[derive(Default)]
struct SourceState {
    /// The whole path of each datum that the transaction met, by its source hash.
    paths: BTreeMap<Blake2b256Hash, Vec<Par>>,
    /// True when the markers of the root `I(ε)` are in `paths`.
    rooted: bool,
    /// The second sends of this transaction whose marker is not in the store.
    owed: Vec<Owed>,
    /// True when the source staged a change that the engine does not see: the retire of a
    /// member of a set class.
    changed: bool,
}

/// The error of a PathMap source whose discipline has the similarity window: no PathMap space
/// has a VectorDB collection (P6, design v3 4.2), so the source fails closed.
const NO_VECTORDB: &str = "a PathMap space has no VectorDB collection";

/// The store seam of a PathMap space with the discipline `D` (joins plan section 9).
pub(crate) struct PrefixSource<'a, D> {
    discipline: &'a D,
    space: NativeSpace,
    /// The token `S(sid)`.
    token: Par,
    /// The root index channel `I(ε)`.
    root: Par,
    /// The data channel `D(p)` of each scoped prefix `p` read as a path.
    homes: Vec<Par>,
    /// The path of the element of a produce.
    own_path: Option<Vec<Par>>,
    state: RefCell<SourceState>,
}

/// One stored datum that a fire spends: the scoped collection of its path, its source hash, and
/// the depth of the marker that the fire or an earlier peek or lift removed.
// Added by decision 124 (plan v4 C7.9, ruling RADD 1.5): a row search keeps a copy of the owed
// second sends.
#[derive(Clone)]
pub(crate) struct SpentDatum {
    home: usize,
    source: Blake2b256Hash,
    delivered: Option<usize>,
}

/// A set class that a fire binds: its path, its key and the hash of its key.
// Added by decision 124 (plan v4 C7.9, ruling RADD 1.5): a row search keeps a copy of the owed
// second sends.
#[derive(Clone)]
pub(crate) struct SpentClass {
    home: usize,
    key: Vec<u8>,
    hash: Blake2b256Hash,
}

/// What a fire spends on a PathMap space: the datums to retire, then the set classes whose other
/// members retire.
// Changed by decision 124 (plan v4 C7.9, ruling RADD 1.5): a row search keeps a copy of what
// c0's fire spends. The text before:
// #[derive(Default)]
#[derive(Clone, Default)]
pub(crate) struct PrefixSpent {
    datums: Vec<SpentDatum>,
    classes: Vec<SpentClass>,
}

/// Added by decision 124 (plan v4 C7.9, ruling RADD 1.5): one `K_retire` of a retire plan: the
/// channel of the retired record, and whether it is the datum record on its data channel (else
/// a marker on an index channel).
struct RetireStep {
    channel: Par,
    datum: bool,
}

/// Added by decision 124 (plan v4 C7.9, ruling RADD 1.5): RETIRE-PLAN of one retired datum: its
/// source hash, whether it is a member of a bound class, the `K_retire` of each of its
/// records in plan order (`delivery::retire_plan`), and the collection of each prefix of its
/// path, which the retire marks dirty.
struct RetirePlan {
    source: Blake2b256Hash,
    member: bool,
    steps: Vec<RetireStep>,
    prefixes: Vec<usize>,
}

/// A marker of a window with its datum: the source hash, the suffix `p∖q` of the marker, the
/// scoped collection of the path `p`, the index of the record on `D(p)`, and the persistence.
struct MarkerDatum {
    source: Blake2b256Hash,
    suffix: Vec<Par>,
    home: usize,
    record_index: usize,
    persist: bool,
}

/// One waiter of an index channel with its order and its lanes there.
struct Seat {
    group: usize,
    index: usize,
    source: Blake2b256Hash,
    ord: i64,
    lanes: Vec<Option<Lane>>,
}

/// The waiters of an index channel: its join groups and one seat for each waiter.
struct Seats {
    groups: Vec<Vec<Par>>,
    seats: Vec<Seat>,
}

/// The staged view of the transaction as the pure planner reads it (`delivery::PrefixView`).
// Changed by decision 124 (plan v4 C7.9, ruling OP-J): the view of the planner reads any
// read view (`StoreView`): the staged view of `Cx` or the store of the commit. The comment
// above a changed line keeps its text before.
// struct CxView<'v, 'c, 'p> {
struct CxView<'v, V> {
    // cx: &'v mut Cx<'c, 'p>,
    view: &'v mut V,
    space: NativeSpace,
}

// impl PrefixView for CxView<'_, '_, '_> {
impl<V: StoreView> PrefixView for CxView<'_, V> {
    fn markers(&mut self, prefix_key: &[u8]) -> Result<&[ParDatum], SpaceError> {
        let channel = PathMapOuter::index(self.space, prefix_key);
        // self.cx.data_on(&channel)
        self.view.data_on(&channel)
    }

    fn records(&mut self, path_key: &[u8]) -> Result<&[ParDatum], SpaceError> {
        let channel = PathMapOuter::path_data(self.space, path_key);
        // self.cx.data_on(&channel)
        self.view.data_on(&channel)
    }

    fn waiters(&mut self, prefix_key: &[u8]) -> Result<&[ParWaiter], SpaceError> {
        let channel = PathMapOuter::index(self.space, prefix_key);
        // self.cx.waiters_on(&channel)
        self.view.waiters_on(&channel)
    }
}

impl<'a, D: Discipline> PrefixSource<'a, D> {
    /// The source of `op` over the scope `scope`, with the names of the token, of the root and of
    /// the data channel of each scoped prefix.
    fn new(discipline: &'a D, scope: &JoinScope, op: &SpaceOp) -> Result<Self, SpaceError> {
        if D::JOIN_WINDOW == JoinWindow::Similarity {
            return Err(SpaceError::Inconsistent(NO_VECTORDB));
        }
        let space = scope.space;
        let token = PathMapOuter::token(space);
        let root = PathMapOuter::index(space, &[]);
        let mut homes = Vec::<Par>::with_capacity(scope.collections.len());
        for scoped in &scope.collections {
            let path_key = terminated(prefix_of_key(&scoped.key)?);
            homes.push(PathMapOuter::path_data(space, &path_key));
        }
        let own_path = match op {
            SpaceOp::Produce(produce) => Some(channel_path(&produce.channel)?.to_vec()),
            SpaceOp::Consume(_) | SpaceOp::Settle(_) | SpaceOp::Join(_) => None,
        };
        Ok(PrefixSource {
            discipline,
            space,
            token,
            root,
            homes,
            own_path,
            state: RefCell::new(SourceState::default()),
        })
    }

    /// The data channel `D(p)` of the scoped prefix `coll` read as a path.
    fn home_at(&self, coll: usize) -> Result<&Par, SpaceError> {
        self.homes.get(coll).ok_or(SpaceError::Inconsistent(
            "a collection is outside its join scope",
        ))
    }

    /// The lane of the slot `slot` in this discipline.
    fn lane_of(&self, slot: &JoinSlot) -> Option<Lane> {
        match D::JOIN_WINDOW {
            JoinWindow::Similarity => None,
            JoinWindow::Lanes => Some(Lane::of_slot(slot.policy.level())),
            JoinWindow::Head | JoinWindow::Scan | JoinWindow::Classes => None,
        }
    }

    // ── Paths ──

    /// The whole path of the datum `source` from the paths that the transaction knows, copied.
    fn known_path(&self, source: &Blake2b256Hash) -> Option<Vec<Par>> {
        let state = self.state.borrow();
        state.paths.get(source).map(|path| path.to_vec())
    }

    /// Records the whole path `path` of the datum `source`.
    fn remember(&self, source: &Blake2b256Hash, path: &[Par]) {
        let mut state = self.state.borrow_mut();
        if !state.paths.contains_key(source) {
            let source = source.clone();
            let path = path.to_vec();
            state.paths.insert(source, path);
        }
    }

    /// The whole path of the stored datum `source`. The root `I(ε)` holds one marker for each
    /// stored datum with its whole path as the suffix, so one read of the root gives every path.
    /// A datum without a root marker breaks the marker invariant and is `Inconsistent`.
    // Changed by decision 124 (plan v4 C7.9, ruling OP-J): the reads of this fn go through the
    // view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn datum_path(
    fn datum_path<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        source: &Blake2b256Hash,
    ) -> Result<Vec<Par>, SpaceError> {
        if let Some(path) = self.known_path(source) {
            return Ok(path);
        }
        let rooted = self.state.borrow().rooted;
        if !rooted {
            let entries: Vec<(Blake2b256Hash, Vec<Par>)> = {
                // let markers = cx.data_on(&self.root)?;
                let markers = view.data_on(&self.root)?;
                let mut entries = Vec::<(Blake2b256Hash, Vec<Par>)>::with_capacity(markers.len());
                for marker in markers {
                    let view = decode_marker(&marker.a)?;
                    entries.push((view.source_hash(), view.suffix.to_vec()));
                }
                entries
            };
            for (known, path) in &entries {
                self.remember(known, path);
            }
            self.state.borrow_mut().rooted = true;
            if let Some(path) = self.known_path(source) {
                return Ok(path);
            }
        }
        Err(SpaceError::Inconsistent(
            "a stored datum has no marker on the root prefix",
        ))
    }

    /// The path of the element `x`: the path of the produce for the element of the operation, and
    /// the path of its datum for a stored element.
    // Changed by decision 124 (plan v4 C7.9, ruling OP-J): the reads of this fn go through the
    // view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn element_path(&self, cx: &mut Cx<'_, '_>, x: &Incoming) -> Result<Vec<Par>, SpaceError> {
    fn element_path<V: StoreView>(
        &self,
        view: &mut V,
        x: &Incoming,
    ) -> Result<Vec<Par>, SpaceError> {
        match (x.own, &self.own_path) {
            (true, Some(path)) => Ok(path.to_vec()),
            (true, None) => Err(SpaceError::Inconsistent(
                "the element of an operation has no path",
            )),
            // (false, _) => self.datum_path(cx, &x.source.hash),
            (false, _) => self.datum_path(view, &x.source.hash),
        }
    }

    /// [`PrefixSource::element_path`] without the staged view: the path of a stored element must
    /// be known already. A window of the element learns it before the engine asks.
    fn known_element_path(&self, x: &Incoming) -> Result<Vec<Par>, SpaceError> {
        match (x.own, &self.own_path) {
            (true, Some(path)) => Ok(path.to_vec()),
            (true, None) => Err(SpaceError::Inconsistent(
                "the element of an operation has no path",
            )),
            (false, _) => self
                .known_path(&x.source.hash)
                .ok_or(SpaceError::Inconsistent(
                    "the path of an element in flight is not known",
                )),
        }
    }

    // ── Markers and datums ──

    /// The marker at `index` of the index channel of `coll` with its datum on `D(p)`. `missing` is
    /// the error of an index outside the staged view.
    // Changed by decision 124 (plan v4 C7.9, ruling OP-J): the reads of this fn go through the
    // view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn marker_datum(
    fn marker_datum<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        scope: &JoinScope,
        coll: usize,
        index: usize,
        missing: &'static str,
    ) -> Result<MarkerDatum, SpaceError> {
        let (source, suffix) = {
            // let markers = cx.data_on(&collection(scope, coll)?.data)?;
            let markers = view.data_on(&collection(scope, coll)?.data)?;
            let marker = markers
                .get(index)
                .ok_or(SpaceError::Inconsistent(missing))?;
            let view = decode_marker(&marker.a)?;
            (view.source_hash(), view.suffix.to_vec())
        };
        let suffix_bytes = path_key(&suffix)?;
        let path_key = joined(prefix_at(scope, coll)?, &suffix_bytes);
        let prefix_key = path_key
            .split_last()
            .map(|(_, rest)| rest)
            .ok_or(SpaceError::Inconsistent("a path key is empty"))?;
        let home = coll_of(scope, prefix_key)?;
        let (record_index, persist) = {
            // let records = cx.data_on(self.home_at(home)?)?;
            let records = view.data_on(self.home_at(home)?)?;
            let record_index = locate(records, &source)?;
            let persist = records
                .get(record_index)
                .map(|record| record.persist)
                .ok_or(SpaceError::Inconsistent(
                    "a record index is outside its path",
                ))?;
            (record_index, persist)
        };
        Ok(MarkerDatum {
            source,
            suffix,
            home,
            record_index,
            persist,
        })
    }

    /// The depth of the prefix `coll` on the path of the datum of `found`: the length of the
    /// path less the length of the suffix of the marker. A marker of the root holds the whole
    /// path, which the transaction then knows.
    // Changed by decision 124 (plan v4 C7.9, ruling OP-J): the reads of this fn go through the
    // view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn marker_depth(
    fn marker_depth<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        scope: &JoinScope,
        coll: usize,
        found: &MarkerDatum,
    ) -> Result<usize, SpaceError> {
        if prefix_at(scope, coll)?.is_empty() {
            self.remember(&found.source, &found.suffix);
            return Ok(0);
        }
        // let path = self.datum_path(cx, &found.source)?;
        let path = self.datum_path(view, &found.source)?;
        path.len()
            .checked_sub(found.suffix.len())
            .ok_or(SpaceError::Inconsistent(
                "a marker suffix is longer than its path",
            ))
    }

    /// The record of the datum `source` on its data channel `D(p)`, copied.
    // Changed by decision 124 (plan v4 C7.11, ruling RADD 1.2): the reads of this fn go through
    // the view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn datum_record(
    fn datum_record<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        home: usize,
        source: &Blake2b256Hash,
    ) -> Result<ParDatum, SpaceError> {
        // let records = cx.data_on(self.home_at(home)?)?;
        let records = view.data_on(self.home_at(home)?)?;
        let index = locate(records, source)?;
        let record = records.get(index).ok_or(SpaceError::Inconsistent(
            "a record index is outside its path",
        ))?;
        Ok(record.clone())
    }

    /// The claim of the datum at `record_index` of the path `home` with the payload `payload`:
    /// its class on its path for a set, and its record otherwise (seams design, open point 1).
    fn datum_claim(
        &self,
        home: usize,
        at: EntryAt,
        payload: &ListParWithRandom,
    ) -> Result<Claim, SpaceError> {
        match D::JOIN_WINDOW {
            JoinWindow::Similarity => Err(SpaceError::Inconsistent(NO_VECTORDB)),
            JoinWindow::Classes => match self.discipline.join_class_key(&payload.pars)? {
                Some(key) => {
                    let hash = bincode_digest(&key)?;
                    Ok(Claim::Class {
                        coll: home,
                        key,
                        hash,
                    })
                }
                None => Ok(Claim::Record { coll: home, at }),
            },
            JoinWindow::Head | JoinWindow::Lanes | JoinWindow::Scan => {
                Ok(Claim::Record { coll: home, at })
            }
        }
    }

    /// The vehicle of the element `x` on the index channel of `coll`: the marker of its datum on
    /// that prefix, then its payload (decision 40, Q3).
    // Changed by decision 124 (plan v4 C7.9, ruling OP-J): the reads of this fn go through the
    // view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn vehicle(
    fn vehicle<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        scope: &JoinScope,
        x: &Incoming,
        coll: usize,
    ) -> Result<ListParWithRandom, SpaceError> {
        // let path = self.element_path(cx, x)?;
        let path = self.element_path(view, x)?;
        let suffix = suffix_under(&path, prefix_at(scope, coll)?)?;
        Ok(vehicle(&x.source.hash, suffix, &x.payload))
    }

    // ── In flight ──

    /// Records the second send of the datum `source` whose marker of depth `depth` a peek COMM or
    /// a lift removes.
    fn owe(&self, source: &Blake2b256Hash, depth: usize, home: usize) {
        let mut state = self.state.borrow_mut();
        let owed = Owed {
            source: source.clone(),
            depth,
            home,
        };
        state.owed.push(owed);
    }

    /// True when this transaction owes the marker of depth `depth` of the datum `source`, and
    /// forgets it.
    fn settle_owed(&self, source: &Blake2b256Hash, depth: Option<usize>) -> bool {
        let mut state = self.state.borrow_mut();
        let mut found = None;
        for (position, owed) in state.owed.iter().enumerate() {
            if owed.source == *source && depth.is_none_or(|depth| owed.depth == depth) {
                found = Some(position);
                break;
            }
        }
        match found {
            Some(position) => {
                state.owed.remove(position);
                true
            }
            None => false,
        }
    }

    /// True when this transaction owes a marker of depth `depth` of the datum `source`.
    fn owes(&self, source: &Blake2b256Hash, depth: usize) -> bool {
        let state = self.state.borrow();
        for owed in &state.owed {
            if owed.source == *source && owed.depth == depth {
                return true;
            }
        }
        false
    }

    /// The source hashes of the datums in flight: the second sends that this transaction owes,
    /// the element `incoming` when it is a stored datum, and each owed second send of `pending`
    /// with a `restore` depth.
    fn in_flight(
        &self,
        incoming: Option<&Incoming>,
        pending: &[Work],
    ) -> Result<Vec<Blake2b256Hash>, SpaceError> {
        let mut excluded = Vec::new();
        {
            let state = self.state.borrow();
            for owed in &state.owed {
                excluded.push(owed.source.clone());
            }
        }
        if let Some(x) = incoming.filter(|x| x.restore.is_some()) {
            excluded.push(x.source.hash.clone());
        }
        for work in pending.iter().filter(|work| work.restore.is_some()) {
            let source = Produce::create(self.home_at(work.coll)?, &work.record, work.persist);
            excluded.push(source.hash);
        }
        Ok(excluded)
    }

    /// True when the data channel of the element `x` still holds its record. A second send that
    /// a settle job carries can find its datum spent by another job of the deploy.
    // Changed by decision 124 (plan v4 C7.9, ruling OP-J): the reads of this fn go through the
    // view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn is_stored(&self, cx: &mut Cx<'_, '_>, x: &Incoming) -> Result<bool, SpaceError> {
    fn is_stored<V: StoreView>(&self, view: &mut V, x: &Incoming) -> Result<bool, SpaceError> {
        // let records = cx.data_on(self.home_at(x.coll)?)?;
        let records = view.data_on(self.home_at(x.coll)?)?;
        for record in records {
            if record.source.hash == x.source.hash {
                return Ok(true);
            }
        }
        Ok(false)
    }

    // ── Windows ──

    /// The stored datums under the prefix `coll` without the datums `excluded`, path by path in
    /// path-key order, each in the record order of its path (CONSUME-CANDIDATES).
    // Changed by decision 124 (plan v4 C7.9, ruling OP-J): the reads of this fn go through the
    // view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn paths_under(
    fn paths_under<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        scope: &JoinScope,
        coll: usize,
        excluded: &[Blake2b256Hash],
    ) -> Result<Vec<PathCandidates>, SpaceError> {
        let prefix_key = prefix_at(scope, coll)?.to_vec();
        let mut view = CxView {
            // cx,
            view,
            space: self.space,
        };
        consume_candidates_excluding(
            &mut view,
            &prefix_key,
            excluded,
            &mut |records: &[ParDatum]| -> Result<Vec<usize>, SpaceError> {
                let mut order = Vec::<usize>::with_capacity(records.len());
                order.extend(0..records.len());
                Ok(order)
            },
        )
    }

    /// The marker of each record of the path `path`, by record index: `Some` for a datum of the
    /// window, `None` for a datum in flight.
    fn markers_by_record(
        &self,
        path: &PathCandidates,
        records: usize,
    ) -> Result<Vec<Option<usize>>, SpaceError> {
        let mut markers = Vec::<Option<usize>>::with_capacity(records);
        markers.resize(records, None);
        for entry in &path.entries {
            let slot = markers
                .get_mut(entry.record_index)
                .ok_or(SpaceError::Inconsistent(
                    "a record index is outside its path",
                ))?;
            *slot = Some(entry.marker_index);
        }
        Ok(markers)
    }

    /// The window of `need` binds on the path `path` (the records of `D(p)` with their markers),
    /// with the element `x` at its place when it lies on the path. A scan or a set takes every
    /// datum in the canonical order of DR-75, and the element in flight joins it at the end. A
    /// head-only discipline takes the first `need` entries of its head order, and a priority
    /// queue the first `need` records of each level.
    // Changed by decision 124 (plan v4 C7.9, ruling OP-J): the reads of this fn go through the
    // view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn path_window(
    fn path_window<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        path_key: &[u8],
        path: Option<&PathCandidates>,
        need: usize,
        x: Option<&Incoming>,
        window: &mut Vec<Entry>,
    ) -> Result<(), SpaceError> {
        let data = PathMapOuter::path_data(self.space, path_key);
        let count = match path {
            // Some(_) => cx.data_on(&data)?.len(),
            Some(_) => view.data_on(&data)?.len(),
            None => 0,
        };
        let markers = match path {
            Some(path) => self.markers_by_record(path, count)?,
            None => Vec::new(),
        };
        match D::JOIN_WINDOW {
            JoinWindow::Similarity => Err(SpaceError::Inconsistent(NO_VECTORDB)),
            JoinWindow::Scan | JoinWindow::Classes => {
                if path.is_some() {
                    // Added by decision 102 (plan v4 C5.7): kept DR-75, because it is the DR-75
                    // base of the PathMap join windows, which keep DR-75 in both roles and have
                    // no pick (decision 124, L4).
                    // for record in cx.canonical_data_order(&data)? {
                    for record in view.canonical_data_order(&data)? {
                        if let Some(Some(marker)) = markers.get(record) {
                            window.push(Entry {
                                at: EntryAt::Stored(*marker),
                                level: None,
                            });
                        }
                    }
                }
                if x.is_some() {
                    window.push(Entry {
                        at: EntryAt::Incoming,
                        level: None,
                    });
                }
                Ok(())
            }
            JoinWindow::Head => {
                let mut records = Vec::<(i64, usize)>::with_capacity(count);
                for (record, marker) in markers.iter().enumerate() {
                    let Some(marker) = marker else {
                        continue;
                    };
                    let ord = {
                        // let stored = cx.data_on(&data)?;
                        let stored = view.data_on(&data)?;
                        let datum = stored.get(record).ok_or(SpaceError::Inconsistent(
                            "a record index is outside its path",
                        ))?;
                        self.record_slots(&datum.a)?.0
                    };
                    records.push((ord, *marker));
                }
                for entry in self.head_window(&records, need, x)? {
                    window.push(entry);
                }
                Ok(())
            }
            JoinWindow::Lanes => {
                let mut levels: BTreeMap<Level, Vec<(i64, usize)>> = BTreeMap::new();
                for (record, marker) in markers.iter().enumerate() {
                    let Some(marker) = marker else {
                        continue;
                    };
                    let (ord, level) = {
                        // let stored = cx.data_on(&data)?;
                        let stored = view.data_on(&data)?;
                        let datum = stored.get(record).ok_or(SpaceError::Inconsistent(
                            "a record index is outside its path",
                        ))?;
                        self.record_slots(&datum.a)?
                    };
                    let level = level.ok_or(SpaceError::Inconsistent(
                        "a record of a priority queue has no level",
                    ))?;
                    levels.entry(level).or_default().push((ord, *marker));
                }
                if let Some(x) = x {
                    let level = x.level.ok_or(SpaceError::Inconsistent(
                        "an element of a priority queue has no level",
                    ))?;
                    if !levels.contains_key(&level) {
                        levels.insert(level, Vec::new());
                    }
                }
                for (level, records) in &levels {
                    let in_level = x.filter(|x| x.level == Some(*level));
                    for entry in self.head_window(records, need, in_level)? {
                        window.push(Entry {
                            at: entry.at,
                            level: Some(*level),
                        });
                    }
                }
                Ok(())
            }
        }
    }

    /// The order of the place of an element with the order `ord` in a side whose stored orders
    /// are `stored` (joins plan section 3.1), as the keyed source places it. An equal order is
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

    /// The first `need` entries of `entries` (order, marker index) in the head order of the
    /// discipline: the head, then the head of the rest, and so on.
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

    // ── Waiters ──

    /// The collection of each channel of `group`: each channel is an index channel of the space.
    fn group_colls(&self, scope: &JoinScope, group: &[Par]) -> Result<Vec<usize>, SpaceError> {
        let mut colls = Vec::<usize>::with_capacity(group.len());
        for channel in group {
            match namespace::reified_id_of(channel) {
                Some(ReifiedId::PathIndex(owner, tail)) if owner == self.space => {
                    colls.push(coll_of(scope, tail)?);
                }
                _ => {
                    return Err(SpaceError::Inconsistent(
                        "a collection channel is part of a foreign join",
                    ))
                }
            }
        }
        Ok(colls)
    }

    /// The seats of the waiters of the index channel of `coll`: each waiter of each join group,
    /// with its order and its lanes there.
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
                let ord = slots
                    .get(first)
                    .ok_or(SpaceError::Inconsistent(
                        "a waiter has not one slot for each position",
                    ))?
                    .ord;
                let mut lanes: Vec<Option<Lane>> = Vec::new();
                for &position in &positions {
                    let slot = slots.get(position).ok_or(SpaceError::Inconsistent(
                        "a waiter has not one slot for each position",
                    ))?;
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

    /// The head of the seats in the lane `lane` (every seat for `None`) by the head rule of the
    /// discipline.
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

    /// The lanes of the slots `slots` on the collection `coll` of the positions `colls`, each once.
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

    /// The references of the seats `heads` in ascending order of the seats, the older first.
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

    /// Every waiter of the index channel of `coll` in the canonical order of DR-75 of the union
    /// of its join groups: a merge of the canonical orders of the groups by the source hash of
    /// each consume (decision 41). A tie across groups is `Inconsistent`.
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
            // pick inside one prefix in `offer_order`, and `cascade_order` keeps it (F3).
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
                        Ordering::Less => Some((group, hash)),
                        Ordering::Greater => Some((best, best_hash)),
                        Ordering::Equal => {
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

    /// The waiters of the index channel of `coll` that an element of the level `level` meets in
    /// the waiter order of the discipline: the head waiter (head-only), the lane heads older first
    /// (priority queue), or every waiter in the canonical order of DR-75 (bag, set).
    fn waiters_of(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        coll: usize,
        level: Option<Level>,
    ) -> Result<Vec<WaiterRef>, SpaceError> {
        match D::JOIN_WINDOW {
            JoinWindow::Similarity => Err(SpaceError::Inconsistent(NO_VECTORDB)),
            JoinWindow::Head => {
                let seats = self.seats(cx, scope, coll)?;
                match self.head_seat(&seats.seats, None)? {
                    Some(head) => Ok(vec![self.reference(&seats, head)?]),
                    None => Ok(Vec::new()),
                }
            }
            JoinWindow::Lanes => {
                let level = level.ok_or(SpaceError::Inconsistent(
                    "an element of a priority queue has no level",
                ))?;
                let seats = self.seats(cx, scope, coll)?;
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
            JoinWindow::Scan | JoinWindow::Classes => self.union_order(cx, scope, coll),
        }
    }

    // ── Stores and retires ──

    /// Added by decision 124 (plan v4 C7.11, rule CR-33, ruling RADD2v2 6.6): the marker of depth
    /// `depth` of the datum `source` on the path of `home` over the store `view`, built again with
    /// the bytes of the stored marker (decision 45, Rule E): its index channel, the marker and the
    /// scoped collection of its prefix. It stages nothing.
    fn restore_record<V: StoreView>(
        &self,
        view: &mut V,
        scope: &JoinScope,
        source: &Blake2b256Hash,
        depth: usize,
        home: usize,
    ) -> Result<(Par, ListParWithRandom, usize), SpaceError> {
        let path = self.datum_path(view, source)?;
        let record = self.datum_record(view, home, source)?;
        let marker = datum_marker(&path, source, &record.a, depth)?;
        let encoded = encode_path(&path)?;
        let prefix_key = encoded.prefix_key(depth).ok_or(SpaceError::Inconsistent(
            "an encoded path has no prefix key of a depth",
        ))?;
        let coll = coll_of(scope, prefix_key)?;
        Ok((PathMapOuter::index(self.space, prefix_key), marker, coll))
    }

    /// Restores the marker of depth `depth` of the datum `source` on the path of `home`: the
    /// marker that a peek COMM or a lift removed, built again with the bytes of the stored
    /// marker, so it keeps its produce identity (decision 45, Rule E). It returns the scoped
    /// collection of the restored prefix.
    fn restore_marker(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        source: &Blake2b256Hash,
        depth: usize,
        home: usize,
    ) -> Result<usize, SpaceError> {
        // Changed by decision 124 (plan v4 C7.11, rule CR-33, ruling RADD2v2 6.6): the reads come
        // from `restore_record`, which a finish row shares (P1). The text before:
        // ```
        // let path = self.datum_path(cx, source)?;
        // let record = self.datum_record(cx, home, source)?;
        // let marker = datum_marker(&path, source, &record.a, depth)?;
        // let encoded = encode_path(&path)?;
        // let prefix_key = encoded.prefix_key(depth).ok_or(SpaceError::Inconsistent(
        //     "an encoded path has no prefix key of a depth",
        // ))?;
        // let channel = PathMapOuter::index(self.space, prefix_key);
        // cx.restore_on(&channel, marker)?;
        // coll_of(scope, prefix_key)
        // ```
        let (channel, marker, coll) = self.restore_record(cx, scope, source, depth, home)?;
        cx.restore_on(&channel, marker)?;
        Ok(coll)
    }

    /// Added by decision 124 (plan v4 C7.9, ruling RADD 1.5): the retire plans of what a fire
    /// spends (`spent`) over the store `view`: RETIRE-PLAN of each spent datum in group order,
    /// then RETIRE-CLASS, the other stored members of each bound class on its path that are
    /// not persistent. The staging stages them after the fire (`retire_spent`), and the commit
    /// reads them before it. A fire moves only markers of the delivered depths, which no plan
    /// lists, so both reads give the same plans.
    fn retire_list<V: StoreView>(
        &self,
        view: &mut V,
        scope: &JoinScope,
        spent: &PrefixSpent,
    ) -> Result<Vec<RetirePlan>, SpaceError> {
        let mut plans = Vec::<RetirePlan>::with_capacity(spent.datums.len());
        let mut retired = Vec::<Blake2b256Hash>::with_capacity(spent.datums.len());
        for datum in &spent.datums {
            plans.push(self.retire_steps(view, scope, datum, false)?);
            retired.push(datum.source.clone());
        }
        for class in &spent.classes {
            for source in self.class_members(view, class, &retired)? {
                let member = SpentDatum {
                    home: class.home,
                    source,
                    delivered: None,
                };
                plans.push(self.retire_steps(view, scope, &member, true)?);
                retired.push(member.source);
            }
        }
        Ok(plans)
    }

    /// Added by decision 124 (plan v4 C7.9, ruling RADD 1.5): RETIRE-PLAN of the spent datum
    /// `spent` over the store `view`: the `K_retire` of its record on `D(p)`, then of its marker
    /// on each prefix of `p` in depth order, without the marker of the delivered depth
    /// (`delivery::retire_plan`), and the collection of every prefix of `p`.
    fn retire_steps<V: StoreView>(
        &self,
        view: &mut V,
        scope: &JoinScope,
        spent: &SpentDatum,
        member: bool,
    ) -> Result<RetirePlan, SpaceError> {
        let path = self.datum_path(view, &spent.source)?;
        let encoded = encode_path(&path)?;
        let plan = retire_plan(path.len(), spent.delivered);
        let mut steps = Vec::<RetireStep>::with_capacity(plan.len());
        for step in plan {
            match step {
                Retired::Datum => steps.push(RetireStep {
                    channel: self.home_at(spent.home)?.clone(),
                    datum: true,
                }),
                Retired::Marker(depth) => {
                    let prefix_key = encoded.prefix_key(depth).ok_or(SpaceError::Inconsistent(
                        "an encoded path has no prefix key of a depth",
                    ))?;
                    steps.push(RetireStep {
                        channel: PathMapOuter::index(self.space, prefix_key),
                        datum: false,
                    });
                }
            }
        }
        let mut prefixes = Vec::<usize>::with_capacity(path.len() + 1);
        for prefix_key in encoded.prefix_keys() {
            prefixes.push(coll_of(scope, prefix_key?)?);
        }
        Ok(RetirePlan {
            source: spent.source.clone(),
            member,
            steps,
            prefixes,
        })
    }

    /// Added by decision 124 (plan v4 C7.9, ruling RADD 1.5): the record of the datum `source` on
    /// `channel` in the store `view`: the datum record on its data channel, or one of its
    /// markers on an index channel, with its index and the payload that the retire pattern
    /// binds. The read of `retire_record`, which the commit reads too.
    fn retired_record<V: StoreView>(
        &self,
        view: &mut V,
        channel: &Par,
        source: &Blake2b256Hash,
        datum: bool,
    ) -> Result<(usize, ParDatum, ListParWithRandom), SpaceError> {
        let (index, stored) = {
            let records = view.data_on(channel)?;
            let index = match datum {
                true => locate(records, source)?,
                false => locate_marker(records, source)?,
            };
            let stored = records.get(index).ok_or(SpaceError::Inconsistent(
                "a retired index is outside its channel",
            ))?;
            (index, stored.clone())
        };
        let payload = match datum {
            true => self.payload_of(&stored.a)?,
            false => stored.a.clone(),
        };
        Ok((index, stored, payload))
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.2): the collections that
    /// the store of the element `x` marks dirty, over the store `view`, as `land` marks them:
    /// every prefix of the path of a fresh element, or the prefix of the owed
    /// marker that a second send restores. A second send that the source does not owe marks
    /// nothing. It settles the owed send as `land` does, and stages nothing.
    fn land_marks<V: StoreView>(
        &self,
        view: &mut V,
        scope: &JoinScope,
        x: &Incoming,
    ) -> Result<Vec<usize>, SpaceError> {
        let Some(depth) = x.restore else {
            let path = self.element_path(view, x)?;
            let encoded = encode_path(&path)?;
            let mut marks = Vec::<usize>::with_capacity(path.len() + 1);
            for prefix_key in encoded.prefix_keys() {
                marks.push(coll_of(scope, prefix_key?)?);
            }
            return Ok(marks);
        };
        let depth = usize::from(depth);
        let mut marks = Vec::<usize>::with_capacity(1);
        match self.settle_owed(&x.source.hash, Some(depth)) {
            false => {}
            true => {
                let path = self.datum_path(view, &x.source.hash)?;
                let encoded = encode_path(&path)?;
                let prefix_key = encoded.prefix_key(depth).ok_or(SpaceError::Inconsistent(
                    "an encoded path has no prefix key of a depth",
                ))?;
                marks.push(coll_of(scope, prefix_key)?);
            }
        }
        Ok(marks)
    }

    /// Added by decision 124 (plan v4 C7.11, rulings RADD 1.8 and RADD2 1.2, J-STORE): the
    /// channels of the store of the element `x` over the store `view`, as `land` stages it:
    /// the record on the data channel of its prefix and one marker on the index
    /// channel of each prefix of its path for a fresh element, or the marker of its depth for a
    /// second send that the owed list `owed` holds. It stages nothing.
    fn land_channels<V: StoreView>(
        &self,
        view: &mut V,
        x: &Incoming,
        owed: &[Owed],
    ) -> Result<Vec<Par>, SpaceError> {
        let Some(depth) = x.restore else {
            let path = self.element_path(view, x)?;
            let encoded = encode_path(&path)?;
            let mut channels = Vec::<Par>::with_capacity(path.len() + 2);
            channels.push(self.home_at(x.coll)?.clone());
            for prefix_key in encoded.prefix_keys() {
                channels.push(PathMapOuter::index(self.space, prefix_key?));
            }
            return Ok(channels);
        };
        let depth = usize::from(depth);
        let owes = owed
            .iter()
            .any(|owed| owed.source == x.source.hash && owed.depth == depth);
        let mut channels = Vec::<Par>::with_capacity(1);
        match owes {
            false => {}
            true => {
                let path = self.datum_path(view, &x.source.hash)?;
                let encoded = encode_path(&path)?;
                let prefix_key = encoded.prefix_key(depth).ok_or(SpaceError::Inconsistent(
                    "an encoded path has no prefix key of a depth",
                ))?;
                channels.push(PathMapOuter::index(self.space, prefix_key));
            }
        }
        Ok(channels)
    }

    /// Added by decision 124 (ruling RADD2 1.2, rule CR-33, J-STORE): the store of the element
    /// `x` as the rows that the commit runs for an offer row that deviates to that store, over
    /// the store `view`, as `land` stages it: the record on its path and one marker on each
    /// prefix for a fresh element, or the marker of its depth for a second send that the owed
    /// list `owed` holds. A persistent record that a cascade offers again stays stored, so it
    /// has no row. It stages nothing.
    fn land_rows<V: StoreView>(
        &self,
        view: &mut V,
        inputs: &EngineInputs,
        x: &Incoming,
        owed: &[Owed],
    ) -> Result<Vec<ParFinishRow>, SpaceError> {
        match (x.in_flight, x.restore) {
            (false, _) => Ok(Vec::new()),
            (true, None) => {
                let path = self.element_path(view, x)?;
                let encoded = encode_path(&path)?;
                let markers = datum_markers(&path, &x.source.hash, &x.record)?;
                let mut rows = Vec::<ParFinishRow>::with_capacity(markers.len() + 1);
                let own = inputs.job.owns(&x.record.random_state);
                rows.push(FinishRow::Store(store_request(
                    self.home_at(x.coll)?,
                    x.record.clone(),
                    x.persist,
                    &x.introduction,
                    own,
                )));
                for planned in markers {
                    let prefix_key =
                        encoded
                            .prefix_key(planned.depth)
                            .ok_or(SpaceError::Inconsistent(
                                "an encoded path has no prefix key of a depth",
                            ))?;
                    let channel = PathMapOuter::index(self.space, prefix_key);
                    let introduction = inputs.introduction.for_record(&planned.record)?;
                    let own = inputs.job.owns(&planned.record.random_state);
                    rows.push(FinishRow::Store(store_request(
                        &channel,
                        planned.record,
                        x.persist,
                        &introduction,
                        own,
                    )));
                }
                Ok(rows)
            }
            (true, Some(depth)) => {
                let depth = usize::from(depth);
                let owes = owed
                    .iter()
                    .any(|owed| owed.source == x.source.hash && owed.depth == depth);
                match owes {
                    false => Ok(Vec::new()),
                    true => {
                        let (channel, record, _) = self.restore_record(
                            view,
                            &inputs.scope,
                            &x.source.hash,
                            depth,
                            x.coll,
                        )?;
                        let introduction = inputs.introduction.for_record(&record)?;
                        let own = inputs.job.owns(&record.random_state);
                        let mut rows = Vec::<ParFinishRow>::with_capacity(1);
                        rows.push(FinishRow::Store(store_request(
                            &channel,
                            record,
                            false,
                            &introduction,
                            own,
                        )));
                        Ok(rows)
                    }
                }
            }
        }
    }

    /// Added by decision 124 (plan v4 C7.11, rules CR-31 and CR-33, ruling RADD2v2 6.6): the
    /// finish of this source as the rows that the commit stages after the group of a cut row
    /// (P1), over the store `view`: the restore of each marker that the source owes, in owed
    /// order, then the bump of the token when `bumps` holds. The rows equal what `finish` stages.
    /// It stages nothing.
    pub(crate) fn finish_rows<V: StoreView>(
        &self,
        view: &mut V,
        inputs: &EngineInputs,
        bumps: bool,
    ) -> Result<Vec<ParFinishRow>, SpaceError> {
        let owed = self.state.borrow().owed.clone();
        let mut rows = Vec::<ParFinishRow>::with_capacity(owed.len() + 2);
        for marker in &owed {
            let (channel, record, _) = self.restore_record(
                view,
                &inputs.scope,
                &marker.source,
                marker.depth,
                marker.home,
            )?;
            let introduction = inputs.introduction.for_record(&record)?;
            let own = inputs.job.owns(&record.random_state);
            rows.push(FinishRow::Store(store_request(
                &channel,
                record,
                false,
                &introduction,
                own,
            )));
        }
        match bumps {
            false => Ok(rows),
            true => {
                let authority = OperationAuthority {
                    continuation: inputs.authority.as_ref(),
                    introduction: &inputs.introduction,
                };
                let stored = self.token_record(view)?;
                let matched = view.bind(&COUNTER_PATTERN, &stored.a);
                let (take, put) =
                    collection_bump(&self.token, stored, matched, &inputs.random, authority)?;
                rows.push(FinishRow::Take(take));
                rows.push(FinishRow::Store(put));
                Ok(rows)
            }
        }
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-33, ruling RADD2v2 6.6): the only counter
    /// record of the token of this source over the store `view`, copied, as `bump` reads it with
    /// the token as both counter channels (`only_record`, counter.rs). No record and two records
    /// are `Inconsistent`, with the texts that `bump` gives for the token. It stages nothing.
    fn token_record<V: StoreView>(&self, view: &mut V) -> Result<ParDatum, SpaceError> {
        match view.data_on(&self.token)? {
            [] => Err(SpaceError::Inconsistent("a space has no space counter")),
            [stored] => Ok(stored.clone()),
            _ => Err(SpaceError::Inconsistent(
                "a collection has two counter records",
            )),
        }
    }

    /// The `K_retire` of the record of the datum `source` on `channel`: the datum record on its
    /// data channel, or one of its markers on an index channel. The continuation carries the
    /// random state `random` and the authority `authority`, and its body is the source hash of the
    /// retired record, so each retire has an identity of its own.
    #[allow(clippy::too_many_arguments)]
    fn retire_record(
        &self,
        cx: &mut Cx<'_, '_>,
        channel: &Par,
        source: &Blake2b256Hash,
        datum: bool,
        random: &Blake2b512Random,
        authority: Option<&CostAuthority>,
    ) -> Result<(), SpaceError> {
        // Changed by decision 124 (plan v4 C7.9, ruling RADD 1.5): the read moved to
        // `retired_record`, which the commit reads too. The text before:
        // let (index, stored) = {
        //     let records = cx.data_on(channel)?;
        //     let index = match datum {
        //         true => locate(records, source)?,
        //         false => locate_marker(records, source)?,
        //     };
        //     let stored = records.get(index).ok_or(SpaceError::Inconsistent(
        //         "a retired index is outside its channel",
        //     ))?;
        //     (index, stored.clone())
        // };
        // let payload = match datum {
        //     true => self.payload_of(&stored.a)?,
        //     false => stored.a.clone(),
        // };
        let (index, stored, payload) = self.retired_record(cx, channel, source, datum)?;
        let matched = cx
            .bind(&RETIRE_PATTERN, &payload)
            .ok_or(SpaceError::Inconsistent(
                "a retired record does not bind the retire pattern",
            ))?;
        // Changed by decision 124 (plan v2 C7.6, decision 99): `K_retire` takes the machinery
        // seal. The text before:
        // let authority = authority.cloned();
        let authority = cx.introduction().seal_of(authority)?;
        let continuation = retire_continuation(&stored.source.hash, random, authority);
        let mut channels = Vec::<Par>::with_capacity(1);
        channels.push(channel.clone());
        let mut patterns = Vec::<BindPattern>::with_capacity(1);
        patterns.push((*RETIRE_PATTERN).clone());
        let mut candidates = Vec::<ParCandidate>::with_capacity(1);
        let Datum {
            a,
            persist,
            source: produce,
        } = stored;
        candidates.push(ConsumeCandidate {
            channel: channel.clone(),
            datum: Datum {
                a: matched,
                persist,
                source: produce,
            },
            removed_datum: a,
            datum_index: view_index(index)?,
        });
        // Changed by decision 124 (plan v4 C7.11, ruling RADD2 1.6): the retire names the
        // discipline `D` of the source, which its machinery search keeps. The text
        // before:
        // cx.retire(channels, patterns, continuation, candidates)
        cx.retire::<D>(channels, patterns, continuation, candidates)
    }

    /// RETIRE-PLAN of the spent datum `spent`: the `K_retire` of its record on `D(p)`, then of its
    /// marker on each prefix of `p` in depth order, without the marker of the delivered depth
    /// (`delivery::retire_plan`). It marks every prefix of `p` dirty (`PathMapPrefix.v`,
    /// `dirty_of`). Changed by decision 124 (plan v4 C7.9, ruling RADD 1.5): it stages the plan
    /// `plan` that `retire_list` read, so the staging and the commit share one read. A member of
    /// a class marks the transaction changed.
    // Retired by decision 124 (plan v4 C7.9, ruling RADD 1.5): the fn takes six inputs now.
    // #[allow(clippy::too_many_arguments)]
    // fn retire_datum(
    //     &self,
    //     cx: &mut Cx<'_, '_>,
    //     scope: &JoinScope,
    //     spent: &SpentDatum,
    //     random: &Blake2b512Random,
    //     authority: Option<&CostAuthority>,
    //     dirty: &mut [bool],
    // ) -> Result<(), SpaceError> {
    //     let path = self.datum_path(cx, &spent.source)?;
    //     let encoded = encode_path(&path)?;
    //     for step in retire_plan(path.len(), spent.delivered) {
    //         match step {
    //             Retired::Datum => {
    //                 let channel = self.home_at(spent.home)?.clone();
    //                 self.retire_record(cx, &channel, &spent.source, true, random, authority)?;
    //             }
    //             Retired::Marker(depth) => {
    //                 let prefix_key = encoded.prefix_key(depth).ok_or(SpaceError::Inconsistent(
    //                     "an encoded path has no prefix key of a depth",
    //                 ))?;
    //                 let channel = PathMapOuter::index(self.space, prefix_key);
    //                 self.retire_record(cx, &channel, &spent.source, false, random, authority)?;
    //             }
    //         }
    //     }
    //     for prefix_key in encoded.prefix_keys() {
    //         let coll = coll_of(scope, prefix_key?)?;
    //         join::mark(dirty, &[coll]);
    //     }
    //     self.settle_owed(&spent.source, None);
    //     Ok(())
    // }
    fn retire_datum(
        &self,
        cx: &mut Cx<'_, '_>,
        plan: &RetirePlan,
        random: &Blake2b512Random,
        authority: Option<&CostAuthority>,
        dirty: &mut [bool],
    ) -> Result<(), SpaceError> {
        for step in &plan.steps {
            self.retire_record(
                cx,
                &step.channel,
                &plan.source,
                step.datum,
                random,
                authority,
            )?;
        }
        join::mark(dirty, &plan.prefixes);
        self.settle_owed(&plan.source, None);
        match plan.member {
            false => {}
            true => self.state.borrow_mut().changed = true,
        }
        Ok(())
    }

    /// The stored members of the set class `class` on its path that are not persistent, not
    /// retired already and not in flight, in the canonical order of DR-75.
    // Changed by decision 124 (plan v4 C7.9, rulings OP-J and RADD 1.5): the reads of this fn go
    // through the view `view` in place of `cx`. The comment above a changed line keeps its text
    // before.
    // fn class_members(
    fn class_members<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        class: &SpentClass,
        retired: &[Blake2b256Hash],
    ) -> Result<Vec<Blake2b256Hash>, SpaceError> {
        let excluded = self.in_flight(None, &[])?;
        let channel = self.home_at(class.home)?.clone();
        // Added by decision 102 (plan v4 C5.7): kept DR-75, because RETIRE-CLASS reads the store
        // after the fire (G1).
        // let order = cx.canonical_data_order(&channel)?;
        let order = view.canonical_data_order(&channel)?;
        let mut members = Vec::new();
        for index in order {
            let candidate = {
                // let records = cx.data_on(&channel)?;
                let records = view.data_on(&channel)?;
                let record = records.get(index).ok_or(SpaceError::Inconsistent(
                    "a canonical index is outside the staged view",
                ))?;
                match record.persist {
                    true => None,
                    false => Some((record.source.hash.clone(), self.payload_of(&record.a)?)),
                }
            };
            let Some((source, payload)) = candidate else {
                continue;
            };
            if is_excluded(&source, retired) || is_excluded(&source, &excluded) {
                continue;
            }
            let Some(key) = self.discipline.join_class_key(&payload.pars)? else {
                continue;
            };
            if same_class(&class.key, &class.hash, &key)? {
                members.push(source);
            }
        }
        Ok(members)
    }

    /// The spent datum of the marker `marker` that a fire on the prefix `coll` removes, or `None`
    /// for a persistent marker, which a fire never removes.
    // Changed by decision 124 (plan v4 C7.9, ruling OP-J): the reads of this fn go through the
    // view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn spent_marker(
    fn spent_marker<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        scope: &JoinScope,
        coll: usize,
        index: usize,
    ) -> Result<Option<SpentDatum>, SpaceError> {
        let found = self.marker_datum(
            // cx,
            view,
            scope,
            coll,
            index,
            "a bound index is outside the staged view",
        )?;
        if found.persist {
            return Ok(None);
        }
        // let depth = self.marker_depth(cx, scope, coll, &found)?;
        let depth = self.marker_depth(view, scope, coll, &found)?;
        Ok(Some(SpentDatum {
            home: found.home,
            source: found.source,
            delivered: Some(depth),
        }))
    }

    /// The spent datum of the element `x` of a fire that does not peek: none for a persistent
    /// element and for a fresh element, which no store holds. A stored element misses the marker
    /// that its peek or its lift removed when this transaction owes it, and holds every marker
    /// when a capped transaction restored it.
    fn spent_element(&self, x: &Incoming) -> Option<SpentDatum> {
        if x.persist {
            return None;
        }
        let depth = x.restore?;
        let depth = usize::from(depth);
        let delivered = match self.owes(&x.source.hash, depth) {
            true => Some(depth),
            false => None,
        };
        Some(SpentDatum {
            home: x.coll,
            source: x.source.hash.clone(),
            delivered,
        })
    }

    /// Records the class of the claim `claim` in `classes`, once for each class of a path.
    fn spent_class(&self, claim: &Claim, classes: &mut Vec<SpentClass>) -> Result<(), SpaceError> {
        let Claim::Class { coll, key, hash } = claim else {
            return Ok(());
        };
        for known in classes.iter() {
            if known.home == *coll && same_class(&known.key, &known.hash, key)? {
                return Ok(());
            }
        }
        classes.push(SpentClass {
            home: *coll,
            key: key.clone(),
            hash: hash.clone(),
        });
        Ok(())
    }
}

impl<D: Discipline> JoinSource for PrefixSource<'_, D> {
    // Added by decision 128 (kind routing).
    type Discipline = D;

    fn shape(&self) -> JoinWindow { D::JOIN_WINDOW }

    // Replaced by P6 (design v3 4.0): the bound of the discipline under the config.
    // fn capacity(&self) -> Option<usize> { D::CAPACITY }

    fn bound(&self, config: &SpaceConfig, side: Side) -> Result<Option<SideBound>, SpaceError> {
        self.discipline.bound(config, side)
    }

    fn random_picks(&self) -> bool { D::RANDOM_PICKS }

    /// Added by decision 124 (J1 review D4, L4): the PathMap join windows keep DR-75 in both
    /// roles, so no window reads its order, and SEARCH-PRODUCE builds no produce for a pick.
    const WINDOWS_READ_ORDER: bool = false;

    /// Added by decision 102 (plan v4 C4.3, F6): the paths, the root flag, the owed second
    /// sends and the change flag of a failed staging must not reach the restage of R-A.
    fn restart(&self) { self.state.replace(SourceState::default()); }

    /// The token `S(sid)` orders every prefix of the space (decision 31, D-2). Both counter
    /// channels of the bump are the token, so `counter::bump` takes its branch of a stored
    /// collection counter: one take and one put on `S(sid)`. This is intended, because the token
    /// is the collection counter of every prefix. The engine bumps it once in a transaction,
    /// because the source shares one counter. A bag or a set bumps it in `finish` instead.
    fn touch(
        &self,
        cx: &mut Cx<'_, '_>,
        _scope: &JoinScope,
        _coll: usize,
        random: &Blake2b512Random,
        authority: OperationAuthority<'_>,
    ) -> Result<Option<i64>, SpaceError> {
        if !D::USES_COUNTER {
            return Ok(None);
        }
        let counters = CounterChannels {
            space: &self.token,
            collection: &self.token,
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
            _ => Err(SpaceError::Inconsistent(
                "a record level does not fit its discipline",
            )),
        }
    }

    fn record_slots(&self, record: &ListParWithRandom) -> Result<(i64, Option<Level>), SpaceError> {
        match D::JOIN_WINDOW {
            JoinWindow::Similarity => Err(SpaceError::Inconsistent(NO_VECTORDB)),
            JoinWindow::Lanes => {
                let (ord, slot) = record::pq_datum_slots(record)?;
                Ok((ord, Some(Level::of_slot(slot))))
            }
            JoinWindow::Head | JoinWindow::Scan | JoinWindow::Classes => {
                Ok((record::datum_ord(record)?, None))
            }
        }
    }

    fn payload_of(&self, record: &ListParWithRandom) -> Result<ListParWithRandom, SpaceError> {
        match D::JOIN_WINDOW {
            JoinWindow::Similarity => Err(SpaceError::Inconsistent(NO_VECTORDB)),
            JoinWindow::Lanes => Ok(record::unwrap_pq_datum(record)?.datum),
            JoinWindow::Head | JoinWindow::Scan | JoinWindow::Classes => {
                Ok(record::unwrap_datum(record)?.datum)
            }
        }
    }

    fn waiter_slots(&self, waiter: &ParWaiter, width: usize) -> Result<Vec<JoinSlot>, SpaceError> {
        match width {
            1 => {
                let slot = match D::JOIN_WINDOW {
                    JoinWindow::Similarity => return Err(SpaceError::Inconsistent(NO_VECTORDB)),
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
                };
                let mut slots = Vec::<JoinSlot>::with_capacity(1);
                slots.push(slot);
                Ok(slots)
            }
            _ => {
                let slots = record::join_continuation_slots(&waiter.continuation)?;
                let lanes_fit = match D::JOIN_WINDOW {
                    JoinWindow::Similarity => return Err(SpaceError::Inconsistent(NO_VECTORDB)),
                    JoinWindow::Lanes => true,
                    JoinWindow::Head | JoinWindow::Scan | JoinWindow::Classes => {
                        slots.iter().all(|slot| slot.policy == WaiterPolicy::None)
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

    /// For each path `p ⪰ q` in path-key order, the window of the discipline on the datums of
    /// `D(p)` that are not in flight, with the element `incoming` at its place on its path when
    /// it enters `q` (seams design, section 3, and open point 4). A bind on `q` sees the first
    /// `need` entries of each path, not the first `need` entries of the prefix.
    /// Changed by decision 103 (decision 118 E1, rule CR-8): when the group names `q` two or more
    /// times, the element in flight comes first in the window (`join::x_first`).
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
        // Added by decision 102 (plan v4 C5.3): the order of the window. PathMap join windows
        // keep DR-75 in both roles (decision 124, L4), so no arm reads it.
        _order: join::WindowOrder<'_>,
    ) -> Result<Vec<Entry>, SpaceError> {
        let excluded = self.in_flight(incoming, pending)?;
        let entering = match incoming {
            Some(x) => match self.enters(scope, x, coll)? {
                true => Some(x),
                false => None,
            },
            None => None,
        };
        let x_key = match entering {
            Some(x) => {
                // The engine asks for the payload of the element on this prefix next, without
                // the staged view, so its path must be known now.
                if !x.own {
                    // self.datum_path(cx, &x.source.hash)?;
                    self.datum_path(view, &x.source.hash)?;
                }
                Some(terminated(prefix_at(scope, x.coll)?))
            }
            None => None,
        };
        // let paths = self.paths_under(cx, scope, coll, &excluded)?;
        let paths = self.paths_under(view, scope, coll, &excluded)?;
        let mut window: Vec<Entry> = Vec::new();
        let mut placed = x_key.is_none();
        for path in &paths {
            if let (false, Some(x_key)) = (placed, &x_key) {
                match compare_keys(x_key, &path.path_key) {
                    Ordering::Less => {
                        // self.path_window(cx, x_key, None, need, entering, &mut window)?;
                        self.path_window(view, x_key, None, need, entering, &mut window)?;
                        placed = true;
                    }
                    Ordering::Equal => {
                        self.path_window(
                            // cx,
                            view,
                            &path.path_key,
                            Some(path),
                            need,
                            entering,
                            &mut window,
                        )?;
                        placed = true;
                        continue;
                    }
                    Ordering::Greater => {}
                }
            }
            // self.path_window(cx, &path.path_key, Some(path), need, None, &mut window)?;
            self.path_window(view, &path.path_key, Some(path), need, None, &mut window)?;
        }
        if let (false, Some(x_key)) = (placed, &x_key) {
            // self.path_window(cx, x_key, None, need, entering, &mut window)?;
            self.path_window(view, x_key, None, need, entering, &mut window)?;
        }
        // Changed by decision 103 (decision 118 E1, rule CR-8): a prefix that the group names two
        // or more times puts x first, in every shape. The line before:
        // Ok(window)
        match need {
            0 | 1 => Ok(window),
            _ => Ok(join::x_first(window)),
        }
    }

    fn claim(
        &self,
        coll: usize,
        at: EntryAt,
        payload: &ListParWithRandom,
    ) -> Result<Claim, SpaceError> {
        self.datum_claim(coll, at, payload)
    }

    /// PRODUCE-WAITERS of the path of `incoming`: the waiters of each prefix of the path in the
    /// scope, the most specific prefix first, each prefix in the waiter order of the discipline.
    /// A second send that a settle job carries meets no waiter when another job spent its datum
    /// first, and its store then stages nothing.
    /// Added by decision 102 (plan v4 C5.5) and decision 124 (LP-1, L5, L4): on a bag and a set,
    /// play meets the waiters of one prefix in a random order (`join::pick_waiters`), with the
    /// source of the vehicle of the element on that prefix. Replay meets the waiter that a COMM of
    /// that vehicle names first. The prefixes keep the most specific prefix first in both roles.
    fn offer_order(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        incoming: &Incoming,
    ) -> Result<Vec<WaiterRef>, SpaceError> {
        if let Some(depth) = incoming.restore {
            let owed = self.owes(&incoming.source.hash, usize::from(depth));
            if !owed && !self.is_stored(cx, incoming)? {
                return Ok(Vec::new());
            }
        }
        let path_prefix = prefix_at(scope, incoming.coll)?.to_vec();
        let mut prefixes: Vec<usize> = Vec::new();
        for (coll, scoped) in scope.collections.iter().enumerate() {
            if is_prefix_key(prefix_of_key(&scoped.key)?, &path_prefix) {
                prefixes.push(coll);
            }
        }
        // The prefixes of one path are nested byte prefixes in ascending key order, so the
        // reverse order is the most specific prefix first.
        prefixes.reverse();
        // Changed by decision 102 (plan v4 C5.5, rules CR-1 and CR-4) and decision 124 (LP-1, L5,
        // L4): in a discipline that picks at random, an offer picks the waiters inside each prefix
        // with the source of the vehicle of the element on that prefix. The groups keep the most
        // specific prefix first in both roles. The lines before:
        // let mut order = Vec::new();
        // for coll in prefixes {
        //     for waiter in self.waiters_of(cx, scope, coll, incoming.level)? {
        //         order.push(waiter);
        //     }
        // }
        // Ok(order)
        let mut order = Vec::new();
        for coll in prefixes {
            let waiters = self.waiters_of(cx, scope, coll, incoming.level)?;
            let waiters = match (D::JOIN_WINDOW, D::RANDOM_PICKS) {
                (JoinWindow::Scan | JoinWindow::Classes, true) => {
                    let vehicle = self.incoming_source(cx, scope, incoming, coll)?;
                    join::pick_waiters(
                        cx,
                        &collection(scope, coll)?.data,
                        waiters,
                        RSpaceOperationSource::Produce(&vehicle),
                    )?
                }
                _ => waiters,
            };
            order.extend(waiters);
        }
        Ok(order)
    }

    /// Added by decision 124 (F2, base one row source for each produce,
    /// replay_rspace.rs:1058-1082): the offer of `x` records each fire under the vehicle of `x` on
    /// the fired prefix (`incoming_source`) and its store under `x.source` (`land`). A base produce
    /// records every row under its one source, so the first open row of that source is the first
    /// row of the offer. Here the first row of the offer is the row with the least journal index
    /// over `x.source` and the vehicles. So the offer is Stored only when the row of `x.source` is
    /// Stored and comes before the row of each vehicle (`Cx::recorded_first`). The vehicles are
    /// those of every collection that `x` enters, so the answer needs no claim about the waiters
    /// that replay holds. The vehicle lookups run only when the row of `x.source` is Stored.
    /// F2 assumes that no veto depends on a recorded row (`Cx::may_veto` is pure). A rule that
    /// changes this needs its own design.
    fn offer_stored(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        x: &Incoming,
    ) -> Result<bool, SpaceError> {
        let first = RSpaceOperationSource::Produce(&x.source);
        match cx.recorded_stored(first) {
            false => Ok(false),
            true => {
                let mut vehicles = Vec::<Produce>::with_capacity(scope.collections.len());
                for coll in 0..scope.collections.len() {
                    match self.enters(scope, x, coll)? {
                        true => vehicles.push(self.incoming_source(cx, scope, x, coll)?),
                        false => continue,
                    }
                }
                Ok(cx.recorded_first(first, &vehicles))
            }
        }
    }

    /// The heads of the index channel of `coll` that a cascade tests: the head waiter
    /// (head-only), the lane heads older first (priority queue), or every waiter in the canonical
    /// order for a set (decision 50). A bag has none.
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
            JoinWindow::Similarity => Err(SpaceError::Inconsistent(NO_VECTORDB)),
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
            // JoinWindow::Classes => self.union_order(cx, scope, coll),
            JoinWindow::Classes => self.union_order(view, scope, coll),
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
            JoinWindow::Similarity => Err(SpaceError::Inconsistent(NO_VECTORDB)),
            JoinWindow::Scan | JoinWindow::Classes => Ok(true),
            JoinWindow::Head | JoinWindow::Lanes => {
                // let seats = self.seats(cx, scope, coll)?;
                let seats = self.seats(view, scope, coll)?;
                for lane in self.lanes_on(slots, colls, coll) {
                    let filter = match D::JOIN_WINDOW {
                        JoinWindow::Similarity => None,
                        JoinWindow::Lanes => lane,
                        JoinWindow::Head | JoinWindow::Scan | JoinWindow::Classes => None,
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
            JoinWindow::Similarity => Err(SpaceError::Inconsistent(NO_VECTORDB)),
            JoinWindow::Scan | JoinWindow::Classes => Ok(true),
            JoinWindow::Head | JoinWindow::Lanes => {
                let seats = self.seats(cx, scope, coll)?;
                for lane in self.lanes_on(slots, colls, coll) {
                    let filter = match D::JOIN_WINDOW {
                        JoinWindow::Similarity => None,
                        JoinWindow::Lanes => lane,
                        JoinWindow::Head | JoinWindow::Scan | JoinWindow::Classes => None,
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

    /// The marker at `index` of `I(q)` as a bind sees it: the payload of its datum re-rooted under
    /// the suffix of the marker, the persistence of the datum, and the claim of the datum on its
    /// path, so two markers of one datum on nested prefixes claim one entry.
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
        // let found = self.marker_datum(cx, scope, coll, index, missing)?;
        let found = self.marker_datum(view, scope, coll, index, missing)?;
        let own = {
            // let records = cx.data_on(self.home_at(found.home)?)?;
            let records = view.data_on(self.home_at(found.home)?)?;
            let record = records
                .get(found.record_index)
                .ok_or(SpaceError::Inconsistent(
                    "a record index is outside its path",
                ))?;
            self.payload_of(&record.a)?
        };
        let claim = self.datum_claim(found.home, EntryAt::Stored(found.record_index), &own)?;
        let payload = rerooted(&found.suffix, own, &HeadEPathMapAdapter)?;
        Ok(StoredEntry {
            payload,
            persist: found.persist,
            claim,
            // Added by decision 103 (decision 118, rule CR-9): the datum on its path, so two
            // markers of one datum on nested prefixes claim one record.
            record: Claim::Record {
                coll: found.home,
                at: EntryAt::Stored(found.record_index),
            },
        })
    }

    /// The own payload of the datum of the marker at `index` of `I(q)`, not re-rooted (P7,
    /// TH-6): the payload that its send carried and that the theory admitted.
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
        // let found = self.marker_datum(cx, scope, coll, index, missing)?;
        let found = self.marker_datum(view, scope, coll, index, missing)?;
        // let records = cx.data_on(self.home_at(found.home)?)?;
        let records = view.data_on(self.home_at(found.home)?)?;
        let record = records
            .get(found.record_index)
            .ok_or(SpaceError::Inconsistent(
                "a record index is outside its path",
            ))?;
        self.payload_of(&record.a)
    }

    /// Added by decision 124 (rule CR-20): the identity of the vehicle of the datum of the marker
    /// at `index` of `I(q)`. The offer of that datum named this identity on the prefix, and the
    /// marker has its own identity, so a pair that a refusal of the offer booked names the
    /// vehicle.
    fn stored_offer_source<V: StoreView>(
        &self,
        view: &mut V,
        scope: &JoinScope,
        coll: usize,
        index: usize,
    ) -> Result<Option<Produce>, SpaceError> {
        let found = self.marker_datum(
            view,
            scope,
            coll,
            index,
            "a bound index is outside the staged view",
        )?;
        let payload = {
            let records = view.data_on(self.home_at(found.home)?)?;
            let record = records
                .get(found.record_index)
                .ok_or(SpaceError::Inconsistent(
                    "a record index is outside its path",
                ))?;
            self.payload_of(&record.a)?
        };
        let vehicle = vehicle(&found.source, found.suffix, &payload);
        Ok(Some(Produce::create(
            &collection(scope, coll)?.data,
            &vehicle,
            found.persist,
        )))
    }

    /// The markers of `I(q)` in the order of CONSUME-CANDIDATES (path-key order, then the
    /// canonical order of DR-75 within a path), without the datums in flight.
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
        let excluded = self.in_flight(None, &[])?;
        // let paths = self.paths_under(cx, scope, coll, &excluded)?;
        let paths = self.paths_under(view, scope, coll, &excluded)?;
        let mut order = Vec::new();
        for path in &paths {
            let data = PathMapOuter::path_data(self.space, &path.path_key);
            // let count = cx.data_on(&data)?.len();
            let count = view.data_on(&data)?.len();
            let markers = self.markers_by_record(path, count)?;
            // Added by decision 102 (plan v4 C5.7): kept DR-75, because G1 and C5.4 bind the
            // DR-75-first member of a class.
            // for record in cx.canonical_data_order(&data)? {
            for record in view.canonical_data_order(&data)? {
                if let Some(Some(marker)) = markers.get(record) {
                    order.push(*marker);
                }
            }
        }
        Ok(order)
    }

    fn home<'h>(&'h self, _scope: &'h JoinScope, coll: usize) -> Result<&'h Par, SpaceError> {
        self.home_at(coll)
    }

    /// The datum of the marker at `index` of `I(q)` as an element: its record on `D(p)`, its
    /// identity and its own authority, homed on its path. The lift removes the marker, so the
    /// returned record is the marker, and a lifted datum that is not persistent owes its marker
    /// back (`restore`).
    fn stored_element(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        coll: usize,
        index: usize,
        in_flight: bool,
    ) -> Result<(Incoming, ParDatum), SpaceError> {
        let marker = {
            let markers = cx.data_on(&collection(scope, coll)?.data)?;
            let marker = markers.get(index).ok_or(SpaceError::Inconsistent(
                "a bound index is outside the staged view",
            ))?;
            marker.clone()
        };
        let found = self.marker_datum(
            cx,
            scope,
            coll,
            index,
            "a bound index is outside the staged view",
        )?;
        let stored = self.datum_record(cx, found.home, &found.source)?;
        let (ord, level) = self.record_slots(&stored.a)?;
        let payload = self.payload_of(&stored.a)?;
        let introduction = cx.introduction().for_record(&stored.a)?;
        let restore = match in_flight {
            true => {
                let depth = self.marker_depth(cx, scope, coll, &found)?;
                self.owe(&found.source, depth, found.home);
                Some(restore_depth(depth)?)
            }
            false => None,
        };
        let Datum { a, persist, source } = stored;
        let x = Incoming {
            coll: found.home,
            record: a,
            payload,
            source,
            persist,
            ord,
            level,
            placement: Placement::Carried,
            introduction,
            own: false,
            in_flight,
            restore,
        };
        Ok((x, marker))
    }

    /// The second send of the datum of the marker at `index` of `I(q)`: its record on `D(p)`,
    /// homed on its path, with the depth of `q` as its `restore` depth. The peek COMM removes the
    /// marker, and the record and the other markers stay in the store. A persistent datum has no
    /// second send.
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
        let found = self.marker_datum(
            // cx,
            view,
            scope,
            coll,
            index,
            "a bound index is outside the staged view",
        )?;
        // let depth = self.marker_depth(cx, scope, coll, &found)?;
        let depth = self.marker_depth(view, scope, coll, &found)?;
        // let stored = self.datum_record(cx, found.home, &found.source)?;
        let stored = self.datum_record(view, found.home, &found.source)?;
        self.owe(&found.source, depth, found.home);
        Ok(Some(Work {
            coll: found.home,
            record: stored.a,
            persist: false,
            own: false,
            restore: Some(restore_depth(depth)?),
        }))
    }

    /// The identity of the vehicle of `x` on the index channel of `coll`.
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
    ) -> Result<Produce, SpaceError> {
        // let vehicle = self.vehicle(cx, scope, x, coll)?;
        let vehicle = self.vehicle(view, scope, x, coll)?;
        Ok(Produce::create(
            &collection(scope, coll)?.data,
            &vehicle,
            x.persist,
        ))
    }

    /// The vehicle of `x` on the index channel of `coll`, which no store keeps.
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
    ) -> Result<ListParWithRandom, SpaceError> {
        // self.vehicle(cx, scope, x, coll)
        self.vehicle(view, scope, x, coll)
    }

    /// True when the prefix `coll` is a prefix of the path of `x` (`prefix_key_iff_prefix`).
    fn enters(&self, scope: &JoinScope, x: &Incoming, coll: usize) -> Result<bool, SpaceError> {
        Ok(is_prefix_key(
            prefix_at(scope, coll)?,
            prefix_at(scope, x.coll)?,
        ))
    }

    /// The payload of `x` re-rooted under the suffix of its path below the prefix `coll`.
    fn incoming_payload<'x>(
        &self,
        scope: &JoinScope,
        x: &'x Incoming,
        coll: usize,
    ) -> Result<Cow<'x, ListParWithRandom>, SpaceError> {
        if coll == x.coll {
            return Ok(Cow::Borrowed(&x.payload));
        }
        let path = self.known_element_path(x)?;
        let suffix = suffix_under(&path, prefix_at(scope, coll)?)?;
        let payload = x.payload.clone();
        Ok(Cow::Owned(rerooted(
            &suffix,
            payload,
            &HeadEPathMapAdapter,
        )?))
    }

    /// False: the offer order lists one head waiter for each prefix, so a refusal moves to the
    /// head waiter of the next prefix, and the head of each prefix still blocks its own later
    /// waiters (seams design, S-15).
    fn refusal_blocks(&self) -> bool { false }

    fn shares_counter(&self) -> bool { D::USES_COUNTER }

    /// FINISH: the markers of the second sends that the transaction still owes go back first.
    /// Only a transaction that stops at the cap owes them, and its settle job then offers datums
    /// that the store holds in full. Then a bag or a set bumps the token once when the transaction
    /// changed the state (decision 51).
    fn finish(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        changed: bool,
        random: &Blake2b512Random,
        authority: OperationAuthority<'_>,
    ) -> Result<(), SpaceError> {
        let owed = std::mem::take(&mut self.state.borrow_mut().owed);
        for marker in &owed {
            self.restore_marker(cx, scope, &marker.source, marker.depth, marker.home)?;
        }
        let source_changed = self.state.borrow().changed;
        if !D::USES_COUNTER && (changed || source_changed) {
            let counters = CounterChannels {
                space: &self.token,
                collection: &self.token,
            };
            cx.bump_on(counters, random, authority)?;
        }
        Ok(())
    }

    type Spent = PrefixSpent;

    /// A second send counts no slot, because its datum is still in the store or spent. A fresh
    /// element counts the records of its path (F15).
    fn held(
        &self,
        cx: &mut Cx<'_, '_>,
        _scope: &JoinScope,
        x: &Incoming,
    ) -> Result<usize, SpaceError> {
        match x.restore {
            Some(_) => Ok(0),
            None => Ok(cx.data_on(self.home_at(x.coll)?)?.len()),
        }
    }

    /// STORE-MARKERS of a fresh element: its record on `D(p)`, then its marker on each prefix in
    /// depth order. A second send restores the marker that this transaction owes. A second send
    /// that a settle job carries stages nothing: the capped transaction restored its marker, or
    /// another job spent its datum.
    fn land(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        x: Incoming,
        dirty: &mut [bool],
        // Added by decision 124 (errata V4-17): the search of an E3-a store, which the record on
        // the path carries. A second send never reaches E3-a.
        search: Option<ParRowSearch>,
    ) -> Result<(), SpaceError> {
        match x.restore {
            None => {
                let path = self.element_path(cx, &x)?;
                let encoded = encode_path(&path)?;
                let markers = datum_markers(&path, &x.source.hash, &x.record)?;
                self.remember(&x.source.hash, &path);
                let home = self.home_at(x.coll)?.clone();
                // Changed by decision 124 (errata V4-17): the record on the path is the row of the
                // identity of `x`, so it carries the search of an E3-a store. The text before:
                // cx.store_on(&home, x.record, x.persist, &x.introduction)?;
                cx.store_searched(&home, x.record, x.persist, &x.introduction, search)?;
                for planned in markers {
                    let prefix_key =
                        encoded
                            .prefix_key(planned.depth)
                            .ok_or(SpaceError::Inconsistent(
                                "an encoded path has no prefix key of a depth",
                            ))?;
                    let channel = PathMapOuter::index(self.space, prefix_key);
                    let introduction = cx.introduction().for_record(&planned.record)?;
                    cx.store_on(&channel, planned.record, x.persist, &introduction)?;
                    join::mark(dirty, &[coll_of(scope, prefix_key)?]);
                }
                Ok(())
            }
            Some(depth) => {
                let depth = usize::from(depth);
                if self.settle_owed(&x.source.hash, Some(depth)) {
                    let coll = self.restore_marker(cx, scope, &x.source.hash, depth, x.coll)?;
                    join::mark(dirty, &[coll]);
                }
                Ok(())
            }
        }
    }

    /// The datums that a fire spends, read before the fire moves its entries: each stored marker
    /// that the fire removes, and a stored element in flight. A set also records the class of each
    /// bound entry, whose other members retire after the fire.
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
    ) -> Result<PrefixSpent, SpaceError> {
        let mut spent = PrefixSpent::default();
        // Changed by decision 101 (rule CR-13): a single-bind fire binds the first member of its
        // class over `[D]`, so it spends the marker of that member and records its class, as the
        // other sites do. RETIRE-CLASS then retires the other members.
        // match site {
        //     FireSite::Single => {
        //         let coll = *colls
        //             .first()
        //             .ok_or(SpaceError::Inconsistent("a consume has no collection"))?;
        //         for candidate in members {
        //             if candidate.datum.persist {
        //                 continue;
        //             }
        //             let index = usize::try_from(candidate.datum_index)
        //                 .map_err(|_| SpaceError::Inconsistent("a candidate has no view index"))?;
        //             if let Some(datum) = self.spent_marker(cx, scope, coll, index)? {
        //                 spent.datums.push(datum);
        //             }
        //         }
        //     }
        //     FireSite::Offer | FireSite::Join | FireSite::Cascade => {
        match site {
            FireSite::Single | FireSite::Offer | FireSite::Join | FireSite::Cascade => {
                let lifted = match site {
                    FireSite::Cascade => {
                        Some(bound.iter().position(|entry| !entry.persist).unwrap_or(0))
                    }
                    FireSite::Offer | FireSite::Join | FireSite::Single => None,
                };
                for (position, (entry, &coll)) in bound.iter().zip(colls).enumerate() {
                    let datum = match (entry.at, lifted == Some(position)) {
                        (EntryAt::Incoming, _) | (EntryAt::Stored(_), true) => match x {
                            Some(x) => self.spent_element(x),
                            None => {
                                return Err(SpaceError::Inconsistent(
                                    "a fire binds an element in flight, but none is in flight",
                                ))
                            }
                        },
                        (EntryAt::Stored(index), false) => {
                            // self.spent_marker(cx, scope, coll, index)?
                            self.spent_marker(view, scope, coll, index)?
                        }
                    };
                    if let Some(datum) = datum {
                        spent.datums.push(datum);
                    }
                    self.spent_class(&entry.claim, &mut spent.classes)?;
                }
            }
        }
        Ok(spent)
    }

    /// RETIRE-PLAN of each spent datum in group order, then RETIRE-CLASS: the other stored members
    /// of each bound set class on its path that are not persistent. The retire continuations take
    /// the random state of `RandomPurpose::Retire` under `random`.
    fn retire_spent(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        spent: PrefixSpent,
        _colls: &[usize],
        random: &Blake2b512Random,
        authority: Option<&CostAuthority>,
        dirty: &mut [bool],
    ) -> Result<(), SpaceError> {
        if spent.datums.is_empty() && spent.classes.is_empty() {
            return Ok(());
        }
        let retire_random = RandomPurpose::Retire.derive(random);
        // Changed by decision 124 (plan v4 C7.9, ruling RADD 1.5): the reads moved to
        // `retire_list`, which the commit reads too, and each plan is staged in its order. The
        // text before:
        // let mut retired: Vec<Blake2b256Hash> = Vec::new();
        // for datum in &spent.datums {
        //     self.retire_datum(cx, scope, datum, &retire_random, authority, dirty)?;
        //     retired.push(datum.source.clone());
        // }
        // for class in &spent.classes {
        //     for source in self.class_members(cx, class, &retired)? {
        //         let member = SpentDatum {
        //             home: class.home,
        //             source,
        //             delivered: None,
        //         };
        //         self.retire_datum(cx, scope, &member, &retire_random, authority, dirty)?;
        //         self.state.borrow_mut().changed = true;
        //         retired.push(member.source);
        //     }
        // }
        for plan in self.retire_list(cx, scope, &spent)? {
            self.retire_datum(cx, &plan, &retire_random, authority, dirty)?;
        }
        Ok(())
    }

    /// Added by decision 124 (plan v4 C7.9 and C7.11, rulings RADD 1.5 and 1.2): the PathMap row
    /// search of a fire. The count of its staged retires is the count of the steps of the plans
    /// of `retire_list`, which `retire_spent` stages after the fire. The owed list of the source
    /// ends with the owes of the fire: the lift of a cascade, then the sends of a fire that
    /// peeks. The commit search takes the list without them (F-PMJ-OWED), and the tail read the
    /// list at each point of the fire (F-PMJ-OWEDSNAP).
    fn row_search(
        &self,
        cx: &mut Cx<'_, '_>,
        row: JoinRow,
        spent: Option<&Self::Spent>,
    ) -> Result<ParRowSearch, SpaceError> {
        let staged = spent.cloned();
        let staged_retires = match &staged {
            None => 0,
            Some(spent) => self
                .retire_list(cx, &row.inputs().scope, spent)?
                .iter()
                .map(|plan| plan.steps.len())
                .sum::<usize>(),
        };
        let (staged_owed, source_changed) = {
            let state = self.state.borrow();
            (state.owed.clone(), state.changed)
        };
        let unfired = staged_owed.len().checked_sub(row.staged_sends());
        let kept = unfired.and_then(|unfired| unfired.checked_sub(row.staged_lift()));
        let owed = owed_prefix(&staged_owed, kept)?;
        let unfired_owed = owed_prefix(&staged_owed, unfired)?;
        let (store_claims, store_rows) = match row.unit() {
            RowUnit::Stored(x) => (
                self.land_channels(cx, x, &unfired_owed)?,
                self.land_rows(cx, row.inputs(), x, &unfired_owed)?,
            ),
            RowUnit::Fired | RowUnit::Parked => (Vec::new(), Vec::new()),
        };
        Ok(Box::new(PrefixRowSearch::<D> {
            row,
            discipline: D::default(),
            owed,
            staged_owed,
            unfired_owed,
            source_changed,
            seq: cx.seq_reads(),
            staged,
            staged_retires,
            retires: Vec::new(),
            last_owed: Vec::new(),
            last_plans: Vec::new(),
            store_claims,
            store_rows,
            rows: Vec::new(),
        }))
    }

    /// Added by decision 124 (plan v4 C7.11, rules CR-31 and CR-33, ruling RADD2v2 6.6): the
    /// machinery search of the lift with the PathMap end (`PrefixLiftEnd`), over a copy of the
    /// owed list and of the change flag of this source at the lift.
    fn lift_search(&self, lift: LiftRow<D>) -> ParRowSearch {
        let (machinery, cut) = lift.into_parts();
        let state = self.state.borrow();
        Box::new(machinery.with_end(PrefixLiftEnd {
            cut,
            discipline: D::default(),
            owed: state.owed.clone(),
            source_changed: state.changed,
            end: None,
            rows: Vec::new(),
        }))
    }
}

/// Added by decision 124 (plan v4 C7.9, ruling RADD 1.5): the PathMap source of a commit: a new
/// source over the inputs of the run, with the second sends `owed` that the staging owed at the
/// fire, so the windows of the commit leave them out as the staging did (`in_flight`).
fn commit_source<'a, D: Discipline>(
    discipline: &'a D,
    owed: &[Owed],
    inputs: &EngineInputs,
) -> Result<PrefixSource<'a, D>, SpaceError> {
    let source = PrefixSource::new(discipline, &inputs.scope, &inputs.op)?;
    source.state.borrow_mut().owed = owed.to_vec();
    Ok(source)
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.2): the first `len` second
/// sends of `owed`, the owed list before the last owes of a fire. `None`, or a length over the
/// list, fails closed.
fn owed_prefix(owed: &[Owed], len: Option<usize>) -> Result<Vec<Owed>, SpaceError> {
    let Some(prefix) = len.and_then(|len| owed.get(..len)) else {
        return Err(SpaceError::Inconsistent(
            "a fire owes more second sends than its source holds",
        ));
    };
    Ok(prefix.to_vec())
}

/// Added by decision 124 (plan v4 C7.9 and C7.11, rulings RADD 1.5 and 1.2): the row search of a
/// PathMap join fire. It holds the parts that the kinds share, a copy of the discipline, the
/// owed second sends of the source at three points of c0's fire, whether the source had changed
/// the state, the read handle of the claims of the job (CR-24 bullet 3), what c0's fire spends,
/// the count of the retires that the staging stages after c0's fire (CR-33), and what the fire
/// of the last tuple owes and retires.
struct PrefixRowSearch<D> {
    row: JoinRow,
    discipline: D,
    /// The owed second sends when the search of c0's fire ran: the source of the commit search.
    owed: Vec<Owed>,
    /// The owed second sends after c0's fire at the staging.
    staged_owed: Vec<Owed>,
    /// The owed second sends before c0's fire, after the lift of a cascade.
    unfired_owed: Vec<Owed>,
    /// True when the source had changed the state before c0's fire (`finish`).
    source_changed: bool,
    seq: Option<SeqReads>,
    /// What c0's fire spends. `None` for a fire that peeks.
    staged: Option<PrefixSpent>,
    staged_retires: usize,
    /// The retires of the last tuple, each on its channel.
    retires: Vec<(Par, Member)>,
    /// The owed second sends after the fire of the last tuple.
    last_owed: Vec<Owed>,
    /// The retire plans of the fire of the last tuple.
    last_plans: Vec<RetirePlan>,
    /// The channels of the store of the element of an offer row (`land_channels`), which its
    /// claims name when the commit fires no tuple (J-STORE). Empty for a receive.
    store_claims: Vec<Par>,
    /// Added by decision 124 (ruling RADD2 1.2, J-STORE): the rows of the store of the element of
    /// an offer row (`land_rows`), which the commit runs when the commit fires no tuple. Empty
    /// for a receive.
    store_rows: Vec<ParFinishRow>,
    /// Added by decision 124 (plan v4 C7.11, rule CR-33, ruling RADD2v2 6.6): the finish rows
    /// of the last tail read (P1).
    rows: Vec<ParFinishRow>,
}

impl<D: Discipline> RowSearch<Par, BindPattern, ListParWithRandom, TaggedContinuation>
    for PrefixRowSearch<D>
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
        self.last_owed.clear();
        self.last_plans.clear();
        let inputs = Arc::clone(self.row.inputs());
        let source =
            commit_source(&self.discipline, &self.owed, &inputs).map_err(join_commit_error)?;
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
        let fired_owed = match found.staged {
            true => self.staged_owed.clone(),
            false => source.state.borrow().owed.clone(),
        };
        let spent = match (found.staged, &self.staged, &found.spent) {
            (true, Some(spent), _) | (false, _, Some((_, spent))) => Some(spent),
            (true, None, _) | (false, _, None) => None,
        };
        let Some(spent) = spent else {
            self.last_owed = fired_owed;
            return Ok(Some(found.tuple));
        };
        let plans = source
            .retire_list(&mut view, &inputs.scope, spent)
            .map_err(join_commit_error)?;
        // CR-24 bullet 3: an alternative retires on index channels outside its group, so the
        // commit checks each of them (ADD A1.10 (c) 11). c0's claims were taken at the staging.
        match (found.staged, &self.seq) {
            (true, _) | (false, None) => {}
            (false, Some(seq)) => {
                for plan in &plans {
                    for step in &plan.steps {
                        match seq.admits(&step.channel, ClaimRole::Data) {
                            true => {}
                            false => {
                                self.row.restore();
                                return Ok(None);
                            }
                        }
                    }
                }
            }
        }
        let count = plans.iter().map(|plan| plan.steps.len()).sum::<usize>();
        let mut retired = Vec::<RetiredMember>::with_capacity(count);
        for plan in &plans {
            for step in &plan.steps {
                let (index, stored, payload) = source
                    .retired_record(&mut view, &step.channel, &plan.source, step.datum)
                    .map_err(join_commit_error)?;
                retired.push(RetiredMember {
                    channel: step.channel.clone(),
                    stored,
                    payload,
                    index,
                });
            }
        }
        self.retires = commit_members(request.matcher, retired)?;
        self.last_owed = fired_owed;
        self.last_plans = plans;
        Ok(Some(found.tuple))
    }

    fn restore(&mut self) {
        self.row.restore();
        self.retires.clear();
        self.last_owed.clear();
        self.last_plans.clear();
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

    /// Added by decision 124 (plan v4 C7.9 and C7.11, rulings RADD 1.8 and RADD2 1.2, J-STORE): the
    /// claims of the actual unit of the row. An offer row that fires no tuple stores its element,
    /// whose claims are the channels of `land` (`land_channels`).
    fn actual_claims(&self) -> Option<RowClaims<Par>> {
        match self.row.unit() {
            RowUnit::Stored(_) => Some(RowClaims {
                data: self.store_claims.clone(),
                waiters: Vec::new(),
            }),
            RowUnit::Fired | RowUnit::Parked => Some(self.row.actual_claims()),
        }
    }

    fn item_failures(
        &self,
        lists: &ParRowLists<'_>,
        matcher: &dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
        rule: &dyn RowItemRule<TaggedContinuation, ListParWithRandom>,
    ) -> Result<Vec<(Blake2b256Hash, Blake2b256Hash)>, SpaceTxnError> {
        let source = commit_source(&self.discipline, &self.owed, self.row.inputs())
            .map_err(join_commit_error)?;
        self.row
            .item_failures(&source, lists, matcher, rule)
            .map_err(join_commit_error)
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD 1.2 and RADD2 1.1, ADD
    /// A2.8): the tail read of a PathMap join row over the store of the commit, with the
    /// end of Tail(actual) that the engine gives. The source after the unit starts from the owed
    /// list after the fire, before the fire for a store, or of the staging for a park. A fire
    /// settles the owed send of each datum that it retires and marks every prefix of its path
    /// (`retire_datum`). A store marks what `land` marks (`land_marks`). The finish
    /// restores each owed marker that stays, and an unordered discipline bumps the token after
    /// a change. The finish rows of the source join the unit, so the row keeps the end (P1).
    fn tail_read(
        &mut self,
        lists: &ParRowLists<'_>,
        matcher: &dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
    ) -> Result<TailRead, SpaceTxnError> {
        let inputs = Arc::clone(self.row.inputs());
        let unit = self.row.unit();
        let base = match unit {
            RowUnit::Fired => &self.last_owed,
            RowUnit::Stored(_) => &self.unfired_owed,
            RowUnit::Parked => &self.owed,
        };
        let source = commit_source(&self.discipline, base, &inputs).map_err(join_commit_error)?;
        let theory = self.row.theory();
        let width = inputs.scope.collections.len();
        let mut view = CommitView::new(lists, matcher, theory.as_deref(), None, width);
        let (marks, member) = match unit {
            RowUnit::Fired => {
                let count = self
                    .last_plans
                    .iter()
                    .map(|plan| plan.prefixes.len())
                    .sum::<usize>();
                let mut marks = Vec::<usize>::with_capacity(count);
                let mut member = false;
                for plan in &self.last_plans {
                    source.settle_owed(&plan.source, None);
                    marks.extend_from_slice(&plan.prefixes);
                    member |= plan.member;
                }
                (marks, member)
            }
            RowUnit::Stored(x) => {
                let marks = source
                    .land_marks(&mut view, &inputs.scope, x)
                    .map_err(join_commit_error)?;
                (marks, false)
            }
            RowUnit::Parked => (Vec::new(), false),
        };
        let bumps = match D::USES_COUNTER {
            true => false,
            false => {
                self.row.changed_after().map_err(join_commit_error)?
                    || self.source_changed
                    || member
            }
        };
        self.rows = source
            .finish_rows(&mut view, &inputs, bumps)
            .map_err(join_commit_error)?;
        self.row
            .tail_read(&source, lists, matcher, &marks)
            .map_err(join_commit_error)
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.1): the end of
    /// Tail(actual) that the last tail read built.
    fn tail_reissue(&mut self) -> Option<RowReissue> { self.row.take_reissue() }

    /// Added by decision 124 (plan v4 C7.11, rule CR-33, ruling RADD2v2 6.6): the finish rows of
    /// the last tail read (P1).
    fn finish_rows(&mut self) -> Vec<ParFinishRow> { std::mem::take(&mut self.rows) }

    /// Added by decision 124 (ruling RADD2 1.2, J-STORE): the record of an offer row is the
    /// vehicle, which no store keeps.
    fn keeps_staged_record(&self) -> bool { false }

    /// Added by decision 124 (ruling RADD2 1.2, rule CR-33, J-STORE): the store of the element of
    /// a row that fired no tuple (`land_rows`), once.
    fn store_unit(&mut self) -> Vec<ParFinishRow> {
        match self.row.unit() {
            RowUnit::Stored(_) => std::mem::take(&mut self.store_rows),
            RowUnit::Fired | RowUnit::Parked => Vec::new(),
        }
    }
}

/// Added by decision 124 (plan v4 C7.11, rules CR-31 and CR-33, ruling RADD2v2 6.6): the end of
/// a cascade lift of a PathMap source: the stop of the engine at the lift for an owner refusal,
/// and the finish rows of the source at the lift (P1). A refused lift parks inert (CR-16), so
/// the source of the commit starts from the owed list `owed` and the change flag
/// `source_changed` of the source at the lift.
struct PrefixLiftEnd<D> {
    cut: LiftCut,
    discipline: D,
    owed: Vec<Owed>,
    source_changed: bool,
    end: Option<RowReissue>,
    rows: Vec<ParFinishRow>,
}

impl<D: Discipline> RowEnd for PrefixLiftEnd<D> {
    fn tail_read(
        &mut self,
        lists: &ParRowLists<'_>,
        matcher: &dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
    ) -> Result<TailRead, SpaceTxnError> {
        let inputs = self.cut.inputs();
        let source =
            commit_source(&self.discipline, &self.owed, inputs).map_err(join_commit_error)?;
        let end =
            refused_lift_end(&source, inputs, self.cut.before()).map_err(join_commit_error)?;
        let bumps = match D::USES_COUNTER {
            true => false,
            false => self.cut.changed() || self.source_changed,
        };
        let width = inputs.scope.collections.len();
        let mut view = CommitView::new(lists, matcher, None, None, width);
        self.rows = source
            .finish_rows(&mut view, inputs, bumps)
            .map_err(join_commit_error)?;
        self.end = Some(RowReissue::new(end));
        Ok(TailRead::Stages)
    }

    fn tail_reissue(&mut self) -> Option<RowReissue> { self.end.take() }

    fn finish_rows(&mut self) -> Vec<ParFinishRow> { std::mem::take(&mut self.rows) }
}

#[cfg(test)]
mod tests {
    use models::rust::utils::new_gint_par;

    use super::*;

    fn ints(values: &[i64]) -> Vec<Par> {
        values
            .iter()
            .map(|value| new_gint_par(*value, Vec::new(), false))
            .collect()
    }

    #[test]
    fn a_scope_key_tags_its_prefix_and_is_never_empty() {
        let root = scope_key(&[]).expect("the scope key of the root");
        assert_eq!(root.as_bytes(), &[PREFIX_KEY_TAG]);
        assert_eq!(prefix_of_key(&root).expect("a tagged key"), &[] as &[u8]);
        let encoded = encode_path(&ints(&[0, 1])).expect("a path");
        let key = scope_key(full_prefix(&encoded).expect("the prefix")).expect("a key");
        assert_eq!(key.as_bytes().first(), Some(&PREFIX_KEY_TAG));
        assert_eq!(
            prefix_of_key(&key).expect("a tagged key"),
            full_prefix(&encoded).expect("the prefix")
        );
        let untagged = CollectionKey::from_bytes(vec![0x01]).expect("a key");
        assert!(prefix_of_key(&untagged).is_err());
    }

    #[test]
    fn the_terminated_key_of_a_prefix_is_its_path_key() {
        let path = ints(&[3, 4, 5]);
        let encoded = encode_path(&path).expect("a path");
        let prefix = full_prefix(&encoded).expect("the prefix");
        assert_eq!(terminated(prefix), path_key(&path).expect("the path key"));
    }

    #[test]
    fn a_suffix_under_a_prefix_is_the_rest_of_the_path() {
        let path = ints(&[0, 1, 2]);
        let encoded = encode_path(&path).expect("a path");
        for depth in 0..=path.len() {
            let prefix = encoded.prefix_key(depth).expect("a prefix key");
            assert_eq!(depth_of(&encoded, prefix).expect("a depth"), depth);
            assert_eq!(
                suffix_under(&path, prefix).expect("a suffix"),
                path[depth..].to_vec()
            );
        }
        let other = encode_path(&ints(&[9])).expect("a path");
        assert!(depth_of(&encoded, full_prefix(&other).expect("a prefix")).is_err());
    }

    #[test]
    fn a_restore_depth_fits_a_byte_for_every_prefix_under_the_cap() {
        for depth in 0..=super::super::PATH_DEPTH_CAP {
            assert_eq!(usize::from(restore_depth(depth).expect("a depth")), depth);
        }
        assert!(restore_depth(usize::from(u8::MAX) + 1).is_err());
    }

    #[test]
    fn the_closure_adds_every_prefix_of_a_path_and_marks_the_path() {
        let mut closure = Closure::new(NativeSpace {
            sid: [7; namespace::SID_LEN],
            durability: namespace::Durability::Durable,
        });
        let path = ints(&[0, 1]);
        closure.add_path(&path).expect("a path");
        let encoded = encode_path(&path).expect("a path");
        let keys: Vec<Vec<u8>> = encoded
            .prefix_keys()
            .map(|key| key.expect("a key").to_vec())
            .collect();
        assert_eq!(closure.prefixes.keys().cloned().collect::<Vec<_>>(), keys);
        let data: Vec<bool> = closure.prefixes.values().copied().collect();
        assert_eq!(data, vec![false, false, true]);
        assert_eq!(closure.todo.len(), 3);
        closure.add_prefix(&keys[0], true);
        assert_eq!(closure.prefixes.get(&keys[0]), Some(&true));
        assert_eq!(closure.todo.len(), 3, "a known prefix is not visited again");
    }

    // ── Added by decision 124 (plan v2 C7.6 and C7.10, plan v4 C7.8): the seal of a retire, the
    // inert machinery waiters and the book, on a PathMap space of the join engine ──────────────

    // Added by decision 124 (plan v2 C7.6 and C7.10, plan v4 C7.8): the engine tests below run a
    // `pathmap:bag` space over a fresh play space, as `space_of_outer` of the tests of
    // `spaces::join` builds it, read its log and park inert machinery waiters. No helper of this
    // module runs an operation, so the rig below is private to these tests (brief 4.1).
    use std::collections::BTreeSet;
    use std::sync::Arc;

    use models::rhoapi::cost_signature::Value as CostSignatureValue;
    use models::rhoapi::tagged_continuation::TaggedCont;
    use models::rhoapi::{CostRegion, CostSignature, ParWithRandom, TaggedContinuation};
    use models::rust::space_factory_urn::{SpaceFactoryUrn, SpaceInner, SpaceOuter};
    use models::rust::utils::{new_freevar_par, new_freevar_var, new_gstring_par};
    use rspace_plus_plus::rspace::rspace::RSpace;
    use rspace_plus_plus::rspace::rspace_interface::ISpace;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    use rspace_plus_plus::rspace::space_store_handle::SpaceStoreHandle;
    use rspace_plus_plus::rspace::trace::event::{Consume, Event, IOEvent};

    use super::super::path_codec::prefix_key;
    use crate::rust::interpreter::accounting::authority::system_residue_authority_with_caller;
    use crate::rust::interpreter::accounting::costs::Cost;
    use crate::rust::interpreter::accounting::RuntimeBudget;
    use crate::rust::interpreter::matcher::r#match::Matcher;
    use crate::rust::interpreter::spaces::agent::{run_op, JoinBind, SpaceAgent, SpaceCreation};
    use crate::rust::interpreter::spaces::families::FamilyAgent;
    use crate::rust::interpreter::spaces::settle::{lift_continuation, lift_pattern};
    use crate::rust::interpreter::spaces::types::qualifier::SpaceQualifier;

    type ParRSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

    /// Added by decision 124 (plan v2 C7.6 and C7.10, plan v4 C7.8): the space of the engine
    /// tests of this module.
    const SPACE: NativeSpace = NativeSpace {
        sid: [13; namespace::SID_LEN],
        durability: namespace::Durability::Durable,
    };

    fn string(value: &str) -> Par { new_gstring_par(value.to_string(), Vec::new(), false) }

    /// Added by decision 124 (plan v2 C7.6 and C7.10, plan v4 C7.8): the body `name` with the
    /// random state of `name`, as `body_of` of the tests of `spaces::join` gives it.
    fn body_of(name: &str) -> ParWithRandom {
        ParWithRandom {
            body: Some(string(name)),
            random_state: Blake2b512Random::create_from_bytes(name.as_bytes()).to_bytes(),
        }
    }

    fn any() -> BindPattern {
        BindPattern {
            patterns: vec![new_freevar_par(0, Vec::new())],
            remainder: None,
            free_count: 1,
        }
    }

    /// Added by decision 124 (plan v2 C7.6 and C7.10, plan v4 C7.8): the data channel `D(p)` of
    /// the path `p` of `channel`, where a send stores its record.
    fn data_of(channel: &Par) -> Par {
        let key = PathMapOuter.key(channel).expect("a path key");
        PathMapOuter.data_channel(SPACE, &key)
    }

    /// Added by decision 124 (plan v2 C7.6 and C7.10, plan v4 C7.8): the index channel `I(q)` of
    /// the prefix `q`, where a send stores its marker and a receive on `q` waits.
    fn index_of(prefix: &[Par]) -> Par {
        PathMapOuter::index(SPACE, &prefix_key(prefix).expect("a prefix key"))
    }

    /// Added by decision 124 (plan v2 C7.6 and C7.10, plan v4 C7.8): a fresh `pathmap:bag`
    /// space, built as `space_of_outer` of the tests of `spaces::join` builds it. The log starts
    /// empty.
    async fn pathmap_bag() -> (ParRSpace, SpaceEnv) {
        let mut stores = InMemoryStoreManager::new();
        let host = ParRSpace::create(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(Matcher)),
        )
        .expect("an in-memory space");
        let env = SpaceEnv {
            store: SpaceStoreHandle::new(host.clone()),
            budget: RuntimeBudget::new(Cost::create(10_000_000, "pathmap engine test")),
            volatile: Default::default(),
            origin: Default::default(),
        };
        let family = SpaceFactoryUrn::all()
            .find(|urn| urn.outer == SpaceOuter::PathMap && urn.inner == SpaceInner::Bag)
            .expect("a factory family");
        let config = SpaceConfig::new(
            SPACE.sid,
            SpaceOuter::PathMap.as_str(),
            SpaceInner::Bag.as_str(),
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
                handle_continuation: TaggedContinuation {
                    tagged_cont: Some(TaggedCont::ParBody(body_of("handle"))),
                    guard: None,
                    cost_authority: None,
                },
                random: Blake2b512Random::create_from_bytes(b"pathmap engine factory"),
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

    /// Added by decision 124 (plan v2 C7.6 and C7.10, plan v4 C7.8): a send of `value` on the
    /// path of `channel` that is not persistent.
    fn send(channel: &Par, value: i64) -> SpaceOp {
        SpaceOp::Produce(ProduceOp {
            space: SPACE,
            key: CollectionKey::of_channel(channel).expect("a channel has a key"),
            channel: channel.clone(),
            data: ListParWithRandom {
                pars: ints(&[value]),
                random_state: Blake2b512Random::create_from_bytes(
                    format!("send {value}").as_bytes(),
                )
                .to_bytes(),
                cost_authority: None,
                cost_stack: None,
            },
            persistent: false,
            params: BTreeMap::new(),
            authority: CostAuthority::default(),
            reissued_ord: None,
        })
    }

    async fn run(env: &SpaceEnv, op: SpaceOp) -> SpaceOutcome {
        run_op(&FamilyAgent, env, op)
            .await
            .expect("the operation commits")
    }

    /// Added by decision 124 (plan v2 C7.6 and C7.10, plan v4 C7.8): the source hashes of the
    /// records of `channel`, in the order of the store.
    async fn sources_on(host: &ParRSpace, channel: &Par) -> Vec<Blake2b256Hash> {
        host.get_data(channel)
            .await
            .into_iter()
            .map(|datum| datum.source.hash)
            .collect()
    }

    /// Added by decision 124 (plan v2 C7.6 and C7.10, plan v4 C7.8): the payloads of the datum
    /// records of the data channel `channel`, in the order of the store.
    async fn payloads_on(host: &ParRSpace, channel: &Par) -> Vec<Vec<Par>> {
        host.get_data(channel)
            .await
            .iter()
            .map(|datum| {
                record::unwrap_datum(&datum.a)
                    .expect("a datum record")
                    .datum
                    .pars
            })
            .collect()
    }

    /// Added by decision 124 (plan v2 C7.6, decision 99, RC2v2 M-1, C2v3 2.15): `retire_record`
    /// builds each `K_retire` with the machinery seal of the cost authority of the operation:
    /// the system residue of the authority under the deploy id of the budget
    /// (`system_residue_authority_with_caller`). The authority has a region, so its seal is not
    /// the authority itself. A receive on s binds the marker of 1 on `I([s])`, the delivered
    /// depth, so it retires the record of 1 on `D([s])` and its marker on `I(ε)`
    /// (`delivery::retire_plan`). The body of each `K_retire` is the source hash of the record
    /// that it retires.
    #[tokio::test]
    async fn pathmap_retire_takes_the_seal() {
        let s = string("s");
        let (host, env) = pathmap_bag().await;
        run(&env, send(&s, 1)).await;
        let (data, root) = (data_of(&s), index_of(&[]));
        let [record]: [Blake2b256Hash; 1] = sources_on(&host, &data)
            .await
            .try_into()
            .expect("the path of s holds the record of 1");
        let [marker]: [Blake2b256Hash; 1] = sources_on(&host, &root)
            .await
            .try_into()
            .expect("the root prefix holds the marker of 1");
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
        let op = ConsumeOp {
            space: SPACE,
            key: CollectionKey::of_channel(&s).expect("a channel has a key"),
            channel: s.clone(),
            pattern: any(),
            body: body_of("k"),
            persistent: false,
            peek: false,
            guard: None,
            query: None,
            authority: authority.clone(),
            introduction_authority: CostAuthority::default(),
            reissued_ord: None,
        };
        host.take_event_log().await;
        run(&env, SpaceOp::Consume(op.clone())).await;
        let seal = system_residue_authority_with_caller(&authority, &env.budget.deploy_id(), None)
            .expect("the seal of a region with a signature builds");
        let random =
            RandomPurpose::Retire.derive(&Blake2b512Random::from_bytes(&op.body.random_state));
        let log = host.take_event_log().await;
        for (channel, source) in [(data, record), (root, marker)] {
            let retire = Consume::create(
                &vec![channel],
                &vec![retire_pattern()],
                &retire_continuation(&source, &random, Some(seal.clone())),
                false,
            );
            let retires: Vec<&Consume> = log
                .iter()
                .filter_map(|event| match event {
                    Event::Comm(_) | Event::IoEvent(IOEvent::Produce(_)) => None,
                    Event::IoEvent(IOEvent::Consume(consume)) => Some(consume),
                })
                .filter(|consume| consume.channel_hashes == retire.channel_hashes)
                .collect();
            assert_eq!(retires, vec![&retire], "the K_retire carries the seal");
        }
    }

    /// Added by decision 124 (plan v2 C7.10, decision 100, C2v3 3.2): the PMJ case of the reader
    /// test of C2v3 3.2 to 3.4. A `K_retire` and a `K_lift` wait on the index channel `I([s])`
    /// before the send, as a refused machinery row parks them (CR-16). `union_order` skips both,
    /// so the send of 1 on s meets no waiter, and the bag stores it. Both inert waiters stay.
    #[tokio::test]
    async fn readers_skip_an_inert_machinery_waiter() {
        let s = string("s");
        let (host, env) = pathmap_bag().await;
        let prefix = index_of(std::slice::from_ref(&s));
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
                vec![prefix.clone()],
                vec![pattern],
                continuation,
                false,
                BTreeSet::new(),
            )
            .await
            .expect("the inert waiter parks");
        }
        let outcome = run(&env, send(&s, 1)).await;
        assert!(
            outcome.fires.is_empty(),
            "no reader offers the send to an inert waiter"
        );
        assert_eq!(
            payloads_on(&host, &data_of(&s)).await,
            vec![ints(&[1])],
            "the bag stores the send"
        );
        let waiting = host.get_waiting_continuations(vec![prefix]).await.len();
        assert_eq!(waiting, 2, "both inert waiters stay");
    }

    /// Added by decision 124 (plan v4 C7.8, rule CR-20, ruling OP-F13, ADD A1.13): an offer of a
    /// PathMap space skips a stored waiter when the book holds its pair with the marker that its
    /// tuple binds. The join W waits on the paths a and b, and a holds 2. The bind of W on a
    /// reads the marker of 2 on `I([a])` (`path_window`). With the pair of that marker and W
    /// booked, the send of 1 on b does not fire W, and the bag stores 1. A probe space with an
    /// empty book fires W with the same records, and its COMM gives the consume of W.
    #[tokio::test]
    async fn pathmap_join_offer_skips_a_booked_marker() {
        let (a, b) = (string("a"), string("b"));
        let waiter = || {
            SpaceOp::Join(JoinOp {
                space: SPACE,
                binds: [&a, &b]
                    .into_iter()
                    .map(|channel| JoinBind {
                        key: CollectionKey::of_channel(channel).expect("a channel has a key"),
                        channel: channel.clone(),
                        pattern: any(),
                        query: None,
                    })
                    .collect(),
                body: body_of("w"),
                persistent: false,
                peek: false,
                guard: None,
                authority: CostAuthority::default(),
                introduction_authority: CostAuthority::default(),
                reissued_ords: None,
            })
        };
        let (probe, probe_env) = pathmap_bag().await;
        run(&probe_env, send(&a, 2)).await;
        run(&probe_env, waiter()).await;
        probe.take_event_log().await;
        run(&probe_env, send(&b, 1)).await;
        let log = probe.take_event_log().await;
        let consume = log
            .iter()
            .filter_map(|event| match event {
                Event::IoEvent(_) => None,
                Event::Comm(comm) => Some(comm),
            })
            .find(|comm| comm.consume.channel_hashes.len() == 2)
            .expect("the probe fires W")
            .consume
            .hash
            .clone();
        let (host, env) = pathmap_bag().await;
        run(&env, send(&a, 2)).await;
        run(&env, waiter()).await;
        let [marker]: [Blake2b256Hash; 1] = sources_on(&host, &index_of(std::slice::from_ref(&a)))
            .await
            .try_into()
            .expect("the prefix a holds the marker of 2");
        env.volatile.book_pairs([(marker, consume)]);
        let outcome = run(&env, send(&b, 1)).await;
        assert!(outcome.fires.is_empty(), "the offer skips W");
        assert_eq!(
            payloads_on(&host, &data_of(&b)).await,
            vec![ints(&[1])],
            "the bag stores 1"
        );
    }

    // ── Decision 124: the row searches and the finish rows of the prefix source ────────────────

    // Added by decision 124 (rulings RADD2v4 6.1, 7.7 and 7.10): the plan of a test over a prefix
    // scope, the capture of what it stages, and the owner-funded env, the observer and the probe
    // of a commit.
    use crate::rust::interpreter::spaces::anchor::SpaceJob;
    use crate::rust::interpreter::spaces::disciplines::bag::BagDiscipline;
    use crate::rust::interpreter::spaces::disciplines::queue::QueueDiscipline;
    use crate::rust::interpreter::spaces::disciplines::set::SetDiscipline;
    use crate::rust::interpreter::spaces::generic::{
        begin_collection, prepare_collection_op, receive_candidate, run_plan, Admission,
        ReceiveFire, StagedRowSearch, TailEnd,
    };
    use crate::rust::interpreter::spaces::outer::hashmap::HashMapOuter;
    use crate::rust::interpreter::spaces::test_support::{
        capturing, capturing_ops, owner_funded, CapturedOp, Funded, ListsProbe,
    };

    /// Added by decision 124 (ruling RADD2v4 6.1): a fresh PathMap space of the inner collection
    /// `inner`, built as `pathmap_bag` builds its bag. The log starts empty.
    async fn pathmap_of(inner: SpaceInner) -> (ParRSpace, SpaceEnv) {
        let mut stores = InMemoryStoreManager::new();
        let host = ParRSpace::create(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(Matcher)),
        )
        .expect("an in-memory space");
        let env = SpaceEnv {
            store: SpaceStoreHandle::new(host.clone()),
            budget: RuntimeBudget::new(Cost::create(10_000_000, "pathmap engine test")),
            volatile: Default::default(),
            origin: Default::default(),
        };
        let family = SpaceFactoryUrn::all()
            .find(|urn| urn.outer == SpaceOuter::PathMap && urn.inner == inner)
            .expect("a factory family");
        let config = SpaceConfig::new(
            SPACE.sid,
            SpaceOuter::PathMap.as_str(),
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
                handle_continuation: TaggedContinuation {
                    tagged_cont: Some(TaggedCont::ParBody(body_of("handle"))),
                    guard: None,
                    cost_authority: None,
                },
                random: Blake2b512Random::create_from_bytes(b"pathmap engine factory"),
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

    /// Added by decision 124 (ruling RADD2v4 6.1): the row searches of `op` on the path of
    /// `channel` with the discipline `D`, staged in play. It drops the staged work, so nothing
    /// commits.
    async fn staged_searches<D: Discipline>(
        env: &SpaceEnv,
        channel: &Par,
        op: SpaceOp,
    ) -> Vec<ParRowSearch> {
        let key = CollectionKey::of_channel(channel).expect("a channel has a key");
        let mut opened = begin_collection(&HashMapOuter, env, SPACE, &key, channel)
            .await
            .expect("phase A");
        let (kept_searches, searches) = std::sync::mpsc::channel::<ParRowSearch>();
        opened.txn = capturing(opened.txn, kept_searches);
        let prepared =
            prepare_collection_op(&GenericSpace::<PathMapOuter, D>::default(), env, opened, op)
                .await
                .expect("the operation stages");
        drop(prepared);
        searches.try_iter().collect()
    }

    /// Added by decision 124 (ruling RADD2v4 6.1): runs `stage` in a plan of play over the prefix
    /// scope of `op` on the path of `channel`, after phase B over the scope, with the job `job`
    /// and a transaction that keeps the fields of each stage call. It drops the staged work, so
    /// nothing commits, and returns the operations that `stage` staged, in order.
    async fn prefix_plan(
        env: &SpaceEnv,
        channel: &Par,
        op: &SpaceOp,
        job: &SpaceJob,
        mut stage: impl FnMut(&mut Cx<'_, '_>, &JoinScope) -> Result<(), SpaceError>,
    ) -> Vec<CapturedOp> {
        let key = CollectionKey::of_channel(channel).expect("a channel has a key");
        let mut opened = begin_collection(&HashMapOuter, env, SPACE, &key, channel)
            .await
            .expect("phase A");
        let scope = prefix_scope(&mut opened, op).expect("the prefix scope of the operation");
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
        let mut staged = Vec::<CapturedOp>::new();
        let prepared = run_plan(
            env,
            txn,
            &data,
            &config,
            &authority,
            None,
            admission,
            job,
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

    /// Added by decision 124 (ruling RADD2v4 6.1): a send of `value` on the path of `channel`
    /// with the random state of `seed`, so two sends of one value are two records.
    fn seeded_send(channel: &Par, value: i64, seed: &str) -> SpaceOp {
        let SpaceOp::Produce(op) = send(channel, value) else {
            panic!("a send is a produce");
        };
        SpaceOp::Produce(ProduceOp {
            data: ListParWithRandom {
                random_state: Blake2b512Random::create_from_bytes(seed.as_bytes()).to_bytes(),
                ..op.data
            },
            ..op
        })
    }

    /// Added by decision 124 (plan v4 C7.11, ruling RADD2 1.2): a receive on the path of `channel`
    /// with the body `name`, which is not persistent and does not peek.
    fn receive(channel: &Par, name: &str) -> SpaceOp {
        SpaceOp::Consume(ConsumeOp {
            space: SPACE,
            key: CollectionKey::of_channel(channel).expect("a channel has a key"),
            channel: channel.clone(),
            pattern: any(),
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

    /// Added by decision 124 (ruling RADD2v4 6.1): the only record on `channel` in the store.
    async fn only_record(host: &ParRSpace, channel: &Par) -> ParDatum {
        let [record]: [ParDatum; 1] = host
            .get_data(channel)
            .await
            .try_into()
            .expect("the channel holds one record");
        record
    }

    /// Added by decision 124 (ruling RADD2v4 6.1): the job whose own record is the marker
    /// `marker`, so a plan that first stages the restore of that marker stages no anchor.
    fn job_of_marker(marker: &ParDatum) -> SpaceJob {
        SpaceJob {
            space: SPACE,
            random_state: marker.a.random_state.clone(),
            authority: None,
        }
    }

    /// Added by decision 124 (ruling RADD2v4 6.1): the index in the scope of the collection of
    /// the path `[channel]`, the home of a datum on that path.
    fn home_of(scope: &JoinScope, channel: &Par) -> Result<usize, SpaceError> {
        let encoded = encode_path(std::slice::from_ref(channel))?;
        let prefix_key = encoded.prefix_key(1).ok_or(SpaceError::Inconsistent(
            "a path of one element has no prefix key of its depth",
        ))?;
        coll_of(scope, prefix_key)
    }

    /// The dirty marks of a scope, none of them set.
    fn unmarked(scope: &JoinScope) -> Vec<bool> {
        let mut dirty = Vec::<bool>::with_capacity(scope.collections.len());
        dirty.resize(scope.collections.len(), false);
        dirty
    }

    /// The collections that the dirty marks `dirty` hold, in ascending order.
    fn marked_of(dirty: &[bool]) -> Vec<usize> {
        let mut marked = Vec::<usize>::with_capacity(dirty.len());
        for (coll, mark) in dirty.iter().enumerate() {
            match mark {
                false => {}
                true => marked.push(coll),
            }
        }
        marked
    }

    /// Added by decision 124 (plan v4 C7.11, ruling RADD 1.2): the send `op` as the fresh element
    /// of its operation on the collection `coll`, as the engine builds it (`own_datum`).
    fn fresh_element<D: Discipline>(
        cx: &Cx<'_, '_>,
        source: &PrefixSource<'_, D>,
        op: &ProduceOp,
        coll: usize,
    ) -> Result<Incoming, SpaceError> {
        let record = source.wrap_record(0, None, &op.data)?;
        let produce = Produce::create(source.home_at(coll)?, &record, op.persistent);
        Ok(Incoming {
            coll,
            record,
            payload: op.data.clone(),
            source: produce,
            persist: op.persistent,
            ord: 0,
            level: None,
            placement: Placement::Fresh,
            introduction: Arc::clone(cx.introduction()),
            own: true,
            in_flight: true,
            restore: None,
        })
    }

    /// Added by decision 124 (plan v4 C7.11, ruling RADD 1.2): the stored datum `stored` of the
    /// collection `coll` as a second send, whose store restores its marker of depth `depth`.
    fn owed_element(cx: &Cx<'_, '_>, stored: &ParDatum, coll: usize, depth: u8) -> Incoming {
        Incoming {
            coll,
            record: stored.a.clone(),
            payload: stored.a.clone(),
            source: stored.source.clone(),
            persist: false,
            ord: 0,
            level: None,
            placement: Placement::Carried,
            introduction: Arc::clone(cx.introduction()),
            own: false,
            in_flight: true,
            restore: Some(depth),
        }
    }

    /// Added by decision 124 (ruling RADD 1.5): the count of the `K_retire` rows of the staged
    /// operations `ops`. An anchor takes another pattern, so it does not count.
    fn retire_count(ops: &[CapturedOp]) -> usize {
        let retire = [retire_pattern()];
        let mut count = 0;
        for op in ops {
            let CapturedOp::Take { patterns, .. } = op else {
                continue;
            };
            match patterns.as_slice() == retire.as_slice() {
                false => {}
                true => count += 1,
            }
        }
        count
    }

    /// Added by decision 124 (rulings RADD 1.8 and RADD2 1.2): the channels of the stores of the
    /// staged operations `ops`, in staging order.
    fn stored_channels(ops: &[CapturedOp]) -> Vec<Par> {
        let mut channels = Vec::<Par>::with_capacity(ops.len());
        for op in ops {
            let CapturedOp::Store { channel, .. } = op else {
                continue;
            };
            channels.push(channel.clone());
        }
        channels
    }

    /// Added by decision 124 (plan v4 C7.9, ruling RADD 1.5): T-S1-PMJ-COUNT. A fire on a set
    /// spends a member of a class at a delivered depth and an owed second send, and it retires
    /// the other member of the class. The count of the staged retires of its row search is the
    /// count of the steps of the plans of `retire_list` (`row_search`). It equals the count of
    /// the `K_retire` rows that `retire_spent` stages: each spent datum loses its record and its
    /// marker of depth 0, because the depth 1 is delivered, and the other member loses its record
    /// and its two markers. The search is opaque to a test, so the test reads `retire_list`.
    #[tokio::test]
    async fn pathmap_staged_count_equals_the_retire_rows() {
        let s = string("s");
        let (host, env) = pathmap_of(SpaceInner::Set).await;
        for op in [
            seeded_send(&s, 1, "m1"),
            seeded_send(&s, 1, "m2"),
            send(&s, 2),
        ] {
            run(&env, op).await;
        }
        let mut members = Vec::<Blake2b256Hash>::with_capacity(2);
        let mut owed = Vec::<Blake2b256Hash>::with_capacity(1);
        for datum in host.get_data(&data_of(&s)).await {
            let payload = record::unwrap_datum(&datum.a)
                .expect("a datum record")
                .datum;
            match payload.pars == ints(&[1]) {
                false => owed.push(datum.source.hash),
                true => members.push(datum.source.hash),
            }
        }
        let [bound, _other] = members.as_slice() else {
            panic!("the class holds two members");
        };
        let [owed] = owed.as_slice() else {
            panic!("the path holds one record of 2");
        };
        let op = send(&s, 9);
        let random = Blake2b512Random::create_from_bytes(b"pathmap retire count");
        let mut planned = 0_usize;
        let staged = prefix_plan(&env, &s, &op, &SpaceJob::of(&op), |cx, scope| {
            let source = PrefixSource::new(&SetDiscipline, scope, &op)?;
            let home = home_of(scope, &s)?;
            source.owe(owed, 1, home);
            let key = SetDiscipline
                .join_class_key(&ints(&[1]))?
                .ok_or(SpaceError::Inconsistent("a payload of a set has a class"))?;
            let hash = bincode_digest(&key)?;
            let spent = PrefixSpent {
                datums: vec![
                    SpentDatum {
                        home,
                        source: bound.clone(),
                        delivered: Some(1),
                    },
                    SpentDatum {
                        home,
                        source: owed.clone(),
                        delivered: Some(1),
                    },
                ],
                classes: vec![SpentClass { home, key, hash }],
            };
            planned = source
                .retire_list(cx, scope, &spent)?
                .iter()
                .map(|plan| plan.steps.len())
                .sum();
            let mut dirty = unmarked(scope);
            source.retire_spent(cx, scope, spent, &[], &random, None, &mut dirty)
        })
        .await;
        assert_eq!(planned, 7, "two plans of two rows and one plan of three");
        assert_eq!(retire_count(&staged), planned);
    }

    /// Added by decision 124 (plan v4 C7.11, ruling RADD 1.2): T-S8-OWEDSNAP. The owed list before
    /// the last owes of a fire is a prefix, and a length past the list fails closed.
    #[test]
    fn owed_prefix_keeps_the_owes_before_the_fire() {
        let mut owed = Vec::<Owed>::with_capacity(3);
        for depth in 0..3 {
            owed.push(Owed {
                source: Blake2b256Hash::new(&[1]),
                depth,
                home: 0,
            });
        }
        let kept = owed_prefix(&owed, Some(2)).expect("a prefix of the owed list");
        let depths: Vec<usize> = kept.iter().map(|marker| marker.depth).collect();
        assert_eq!(depths, vec![0, 1]);
        assert!(owed_prefix(&owed, Some(4)).is_err());
        assert!(owed_prefix(&owed, None).is_err());
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.2): T-S8-OWEDSNAP. The
    /// source of the search of a fire holds the owed list without the owes of the fire, for a peek
    /// fire and for a cascade lift (F-PMJ-OWED): the staging owes the second sends of depths 0 and
    /// 1 before the fire, the lift of a cascade owes depth 2, and the sends of a fire that peeks
    /// owe depth 3. `commit_source` over `owed_prefix` of the owes before the fire owes only the
    /// first two. The tail read starts from the list of its unit, which keeps the owe of the lift
    /// (F-PMJ-OWEDSNAP). The search is a private type behind `Box<dyn RowSearch>`, and its row is
    /// built only by the engine, so the test reads the two parts that `row_search` composes.
    #[tokio::test]
    async fn pathmap_search_reads_the_owes_before_the_fire() {
        let s = string("s");
        let (_host, env) = pathmap_bag().await;
        let op = send(&s, 1);
        let job = SpaceJob::of(&op);
        let hash = Blake2b256Hash::new(&[1]);
        prefix_plan(&env, &s, &op, &job, |cx, scope| {
            let random = Blake2b512Random::create_from_bytes(b"owes before the fire");
            let inputs = EngineInputs {
                op: Arc::new(op.clone()),
                scope: scope.clone(),
                config: cx.config().clone(),
                random,
                authority: None,
                introduction: Arc::clone(cx.introduction()),
                job: job.clone(),
            };
            let mut owed = Vec::<Owed>::with_capacity(4);
            for depth in 0..4 {
                owed.push(Owed {
                    source: hash.clone(),
                    depth,
                    home: 0,
                });
            }
            let commit = commit_source(&BagDiscipline, &owed_prefix(&owed, Some(2))?, &inputs)?;
            let depths: Vec<usize> = commit
                .state
                .borrow()
                .owed
                .iter()
                .map(|marker| marker.depth)
                .collect();
            assert_eq!(
                depths,
                vec![0, 1],
                "the commit keeps the owes before the fire"
            );
            assert!(commit.owes(&hash, 1), "an owe before the fire stays");
            assert!(
                !commit.owes(&hash, 2),
                "the owe of the lift is not a commit owe"
            );
            assert!(
                !commit.owes(&hash, 3),
                "the owe of a peek is not a commit owe"
            );
            let unit = commit_source(&BagDiscipline, &owed_prefix(&owed, Some(3))?, &inputs)?;
            assert!(
                unit.owes(&hash, 2),
                "the read starts from the list of its unit"
            );
            assert!(
                !unit.owes(&hash, 3),
                "the sends of the fire come after its unit"
            );
            Ok(())
        })
        .await;
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.2): T-S8-PMJ-MARKS.
    /// `land_marks` gives the collections that `land` marks dirty, in order: every prefix of the
    /// path of a fresh element, and the prefix of the marker that an owed second send restores.
    /// `land_marks` settles the owed send as `land` does, so the test owes it again before `land`.
    #[tokio::test]
    async fn land_marks_mark_what_land_marks() {
        let s = string("s");
        let (host, env) = pathmap_bag().await;
        run(&env, send(&s, 1)).await;
        let stored = only_record(&host, &data_of(&s)).await;
        let op = send(&s, 2);
        let SpaceOp::Produce(produce) = op.clone() else {
            panic!("a send is a produce");
        };
        prefix_plan(&env, &s, &op, &SpaceJob::of(&op), |cx, scope| {
            let source = PrefixSource::new(&BagDiscipline, scope, &op)?;
            let home = home_of(scope, &s)?;
            // A fresh element marks each prefix of its path, in depth order.
            let x = fresh_element(cx, &source, &produce, home)?;
            let marks = source.land_marks(cx, scope, &x)?;
            assert_eq!(marks.len(), 2, "the root and the prefix of s");
            assert_eq!(marks.last(), Some(&home), "the deepest prefix is the home");
            let mut dirty = unmarked(scope);
            source.land(cx, scope, x, &mut dirty, None)?;
            assert_eq!(marks, marked_of(&dirty), "a fresh element");
            // An owed second send marks the prefix of its restored marker.
            source.owe(&stored.source.hash, 1, home);
            let x = owed_element(cx, &stored, home, 1);
            let marks = source.land_marks(cx, scope, &x)?;
            assert_eq!(marks, vec![home], "the prefix of the owed marker");
            source.owe(&stored.source.hash, 1, home);
            let mut dirty = unmarked(scope);
            source.land(cx, scope, x, &mut dirty, None)?;
            assert_eq!(marks, marked_of(&dirty), "an owed second send");
            // A second send that the source does not owe marks nothing.
            let x = owed_element(cx, &stored, home, 0);
            assert!(source.land_marks(cx, scope, &x)?.is_empty(), "not owed");
            Ok(())
        })
        .await;
    }

    /// Added by decision 124 (plan v4 C7.11, rulings RADD 1.8 and RADD2 1.2, J-STORE): T-S8-JSTORE.
    /// The claims of an offer row name the channels of the store of its element, which `land`
    /// stages: for a fresh element the record on its path and one marker on each prefix of the
    /// path, read from the search of an owner-funded fire (`actual_claims`), and for an owed
    /// second send the marker of its depth, read from `land_channels`, the list that the row
    /// search reports (`row_search`): a row search of a second send needs a chain of fires in one
    /// plan. Each plan drops its work, so nothing commits.
    #[tokio::test]
    async fn pathmap_store_claims_name_the_channels_of_land() {
        let s = string("s");
        // The fresh element: its offer fires the waiter of the path, so its row carries a search.
        let (_other_host, other) = pathmap_bag().await;
        let parked = run(&other, receive(&s, "k")).await;
        assert!(
            parked.fires.is_empty(),
            "the receive parks on the empty path"
        );
        let searches =
            staged_searches::<BagDiscipline>(&owner_funded(&other), &s, send(&s, 2)).await;
        let search = searches
            .first()
            .expect("the fire of the send carries its row search");
        let Some(RowClaims {
            data: claims,
            waiters,
        }) = search.actual_claims()
        else {
            panic!("the search of an offer row reports the claims of its store");
        };
        assert!(
            waiters.is_empty(),
            "the store of an element claims no waiter"
        );
        let (host, env) = pathmap_bag().await;
        let op = send(&s, 2);
        let SpaceOp::Produce(produce) = op.clone() else {
            panic!("a send is a produce");
        };
        let mut listed = Vec::<Par>::new();
        let staged = prefix_plan(&env, &s, &op, &SpaceJob::of(&op), |cx, scope| {
            let source = PrefixSource::new(&BagDiscipline, scope, &op)?;
            let x = fresh_element(cx, &source, &produce, home_of(scope, &s)?)?;
            listed = source.land_channels(cx, &x, &[])?;
            let mut dirty = unmarked(scope);
            source.land(cx, scope, x, &mut dirty, None)
        })
        .await;
        let landed = stored_channels(&staged);
        assert_eq!(
            landed.len(),
            3,
            "the record and one marker on each of two prefixes"
        );
        assert_eq!(
            listed, landed,
            "the list of a fresh element is the store of `land`"
        );
        assert_eq!(
            claims, landed,
            "the claims of the row name the store of `land`"
        );
        // The owed second send: the marker of depth 1 of the stored datum goes back.
        run(&env, send(&s, 1)).await;
        let stored = only_record(&host, &data_of(&s)).await;
        let marker = only_record(&host, &index_of(std::slice::from_ref(&s))).await;
        let job = job_of_marker(&marker);
        let op = send(&s, 3);
        let mut listed = Vec::<Par>::new();
        let staged = prefix_plan(&env, &s, &op, &job, |cx, scope| {
            let source = PrefixSource::new(&BagDiscipline, scope, &op)?;
            let home = home_of(scope, &s)?;
            source.owe(&stored.source.hash, 1, home);
            let owed = source.state.borrow().owed.clone();
            let x = owed_element(cx, &stored, home, 1);
            listed = source.land_channels(cx, &x, &owed)?;
            let mut dirty = unmarked(scope);
            source.land(cx, scope, x, &mut dirty, None)
        })
        .await;
        let landed = stored_channels(&staged);
        assert_eq!(
            landed,
            vec![index_of(std::slice::from_ref(&s))],
            "the store of an owed second send is the marker of its depth"
        );
        assert_eq!(
            listed, landed,
            "the list of a second send is the store of `land`"
        );
    }

    /// Added by decision 124 (ruling RADD2 1.2, J-STORE): the fields of the rows `rows`.
    fn land_fields(rows: &[ParFinishRow]) -> Vec<CapturedOp> {
        let mut fields = Vec::<CapturedOp>::with_capacity(rows.len());
        for row in rows {
            fields.push(match row {
                FinishRow::Store(store) => CapturedOp::of_store(store),
                FinishRow::Take(take) => CapturedOp::of_take(take),
            });
        }
        fields
    }

    /// Added by decision 124 (ruling RADD2 1.2, J-STORE): the inputs of `land_rows` in the plan
    /// of `cx`, with the job `job`.
    fn land_inputs(
        cx: &Cx<'_, '_>,
        scope: &JoinScope,
        op: &SpaceOp,
        job: &SpaceJob,
    ) -> EngineInputs {
        EngineInputs {
            op: Arc::new(op.clone()),
            scope: scope.clone(),
            config: cx.config().clone(),
            random: Blake2b512Random::create_from_bytes(b"land rows"),
            authority: None,
            introduction: Arc::clone(cx.introduction()),
            job: job.clone(),
        }
    }

    /// Added by decision 124 (ruling RADD2 1.2, rule CR-33, J-STORE): `land_rows` equals, field
    /// by field and in order, the stores that `land` stages: the record and one marker on each
    /// prefix of a fresh element, and the marker of the depth of an owed second send. A second
    /// send that the source does not owe and a record that a cascade offers again have no row.
    #[tokio::test]
    async fn land_rows_equal_the_stores_of_land() {
        let s = string("s");
        let (_host, env) = pathmap_bag().await;
        let op = send(&s, 2);
        let SpaceOp::Produce(produce) = op.clone() else {
            panic!("a send is a produce");
        };
        let job = SpaceJob::of(&op);
        let mut rows = Vec::<CapturedOp>::new();
        let staged = prefix_plan(&env, &s, &op, &job, |cx, scope| {
            let source = PrefixSource::new(&BagDiscipline, scope, &op)?;
            let x = fresh_element(cx, &source, &produce, home_of(scope, &s)?)?;
            let inputs = land_inputs(cx, scope, &op, &job);
            rows = land_fields(&source.land_rows(cx, &inputs, &x, &[])?);
            let mut dirty = unmarked(scope);
            source.land(cx, scope, x, &mut dirty, None)
        })
        .await;
        assert_eq!(
            rows.len(),
            3,
            "the record and one marker on each of two prefixes"
        );
        assert_eq!(
            rows, staged,
            "the rows of a fresh element are the stores of `land`"
        );

        let (host, env) = pathmap_bag().await;
        run(&env, send(&s, 1)).await;
        let stored = only_record(&host, &data_of(&s)).await;
        let marker = only_record(&host, &index_of(std::slice::from_ref(&s))).await;
        let job = job_of_marker(&marker);
        let op = send(&s, 3);
        let mut rows = Vec::<CapturedOp>::new();
        let mut not_owed = Vec::<CapturedOp>::new();
        let mut kept = Vec::<CapturedOp>::new();
        let staged = prefix_plan(&env, &s, &op, &job, |cx, scope| {
            let source = PrefixSource::new(&BagDiscipline, scope, &op)?;
            let home = home_of(scope, &s)?;
            source.owe(&stored.source.hash, 1, home);
            let owed = source.state.borrow().owed.clone();
            let x = owed_element(cx, &stored, home, 1);
            let inputs = land_inputs(cx, scope, &op, &job);
            rows = land_fields(&source.land_rows(cx, &inputs, &x, &owed)?);
            not_owed = land_fields(&source.land_rows(cx, &inputs, &x, &[])?);
            let stays = Incoming {
                in_flight: false,
                restore: None,
                ..owed_element(cx, &stored, home, 1)
            };
            kept = land_fields(&source.land_rows(cx, &inputs, &stays, &owed)?);
            let mut dirty = unmarked(scope);
            source.land(cx, scope, x, &mut dirty, None)
        })
        .await;
        assert_eq!(rows.len(), 1, "the marker of the depth of the send");
        assert_eq!(
            rows, staged,
            "the row of an owed second send is the store of `land`"
        );
        assert!(
            not_owed.is_empty(),
            "the source does not owe this second send"
        );
        assert!(
            kept.is_empty(),
            "a record that a cascade offers again stays stored"
        );
    }

    /// Added by decision 124 (ruling RADD2 1.2, J-STORE): the search of an offer row keeps no
    /// record on the channel of its vehicle, and it hands out the rows of its store once.
    #[tokio::test]
    async fn pathmap_deviating_row_keeps_no_vehicle() {
        let s = string("s");
        let (_host, env) = pathmap_bag().await;
        let parked = run(&env, receive(&s, "k")).await;
        assert!(
            parked.fires.is_empty(),
            "the receive parks on the empty path"
        );
        let mut searches =
            staged_searches::<BagDiscipline>(&owner_funded(&env), &s, send(&s, 2)).await;
        let search = searches
            .first_mut()
            .expect("the fire of the send carries its row search");
        assert!(!search.keeps_staged_record(), "no store keeps the vehicle");
        assert_eq!(
            search.store_unit().len(),
            3,
            "the record and one marker on each of two prefixes"
        );
        assert!(
            search.store_unit().is_empty(),
            "the search hands out its unit once"
        );
    }

    /// Added by decision 124 (rule CR-20): on each prefix of the path of a stored datum, the
    /// identity of the offer of its marker is the identity that the offer of the datum names
    /// there, so a pair that a refusal of the offer booked also blocks a later fire of the marker.
    #[tokio::test]
    async fn stored_offer_source_is_the_identity_of_the_offer() {
        let s = string("s");
        let (host, env) = pathmap_bag().await;
        run(&env, send(&s, 1)).await;
        let stored = only_record(&host, &data_of(&s)).await;
        let marker = only_record(&host, &index_of(std::slice::from_ref(&s))).await;
        let job = job_of_marker(&marker);
        let op = send(&s, 3);
        let mut identities = Vec::<(Blake2b256Hash, Option<Blake2b256Hash>)>::with_capacity(2);
        prefix_plan(&env, &s, &op, &job, |cx, scope| {
            let source = PrefixSource::new(&BagDiscipline, scope, &op)?;
            let x = Incoming {
                payload: source.payload_of(&stored.a)?,
                ..owed_element(cx, &stored, home_of(scope, &s)?, 1)
            };
            for coll in 0..scope.collections.len() {
                let offer = source.incoming_source(cx, scope, &x, coll)?;
                let marker = source.stored_offer_source(cx, scope, coll, 0)?;
                identities.push((offer.hash, marker.map(|marker| marker.hash)));
            }
            Ok(())
        })
        .await;
        assert_eq!(identities.len(), 2, "the two prefixes of the path");
        for (offer, marker) in identities {
            assert_eq!(
                marker,
                Some(offer),
                "the marker answers with the identity of the offer"
            );
        }
    }

    /// Added by decision 124 (ruling RADD2v4 6.1): the fields of the rows of `finish_rows` on a
    /// PathMap space of the inner collection `inner` with the discipline `discipline`, and the
    /// check that `finish` stages them. The owed marker of depth 1 of a stored datum on `s` is
    /// owed when `owes` holds, and the state has changed when `changed` holds. The own record of
    /// the job is that marker, so `finish` stages no anchor before the rows.
    async fn finish_rows_of<D: Discipline>(
        discipline: &D,
        inner: SpaceInner,
        s: &Par,
        owes: bool,
        changed: bool,
    ) -> Vec<CapturedOp> {
        let (host, env) = pathmap_of(inner).await;
        run(&env, send(s, 1)).await;
        let stored = only_record(&host, &data_of(s)).await;
        let marker = only_record(&host, &index_of(std::slice::from_ref(s))).await;
        let job = job_of_marker(&marker);
        let op = send(s, 2);
        let mut rows = Vec::<CapturedOp>::new();
        let staged = prefix_plan(&env, s, &op, &job, |cx, scope| {
            let source = PrefixSource::new(discipline, scope, &op)?;
            match owes {
                false => {}
                true => source.owe(&stored.source.hash, 1, home_of(scope, s)?),
            }
            let inputs = EngineInputs {
                op: Arc::new(op.clone()),
                scope: scope.clone(),
                config: cx.config().clone(),
                random: Blake2b512Random::create_from_bytes(b"finish rows"),
                authority: None,
                introduction: Arc::clone(cx.introduction()),
                job: job.clone(),
            };
            // The bump arm of `finish`: a bag or a set bumps its token after a change.
            let bumps = !D::USES_COUNTER && changed;
            let finish_rows = source.finish_rows(cx, &inputs, bumps)?;
            rows.reserve(finish_rows.len());
            for row in &finish_rows {
                rows.push(match row {
                    FinishRow::Store(store) => CapturedOp::of_store(store),
                    FinishRow::Take(take) => CapturedOp::of_take(take),
                });
            }
            let authority = OperationAuthority {
                continuation: inputs.authority.as_ref(),
                introduction: &inputs.introduction,
            };
            source.finish(cx, scope, changed, &inputs.random, authority)
        })
        .await;
        assert_eq!(
            staged, rows,
            "the rows are the operations that `finish` stages"
        );
        rows
    }

    /// Added by decision 124 (plan v4 C7.11, rules CR-31 and CR-33, rulings RADD2v2 6.6 and
    /// RADD2v4 6.1, S8-MIRROR): U4. The rows of `finish_rows` equal, field by field and in order,
    /// the operations that `finish` stages. With one owed marker and a changed state, a bag
    /// gives the store of the marker on `I(q)`, the take of the token at its value and the
    /// store of the next value. A queue gives only the store, and no owe and no change give no
    /// rows.
    #[tokio::test]
    async fn prefix_finish_rows_equal_the_rows_of_finish() {
        let s = string("s");
        let rows = finish_rows_of(&BagDiscipline, SpaceInner::Bag, &s, true, true).await;
        let [CapturedOp::Store { channel, .. }, CapturedOp::Take { .. }, CapturedOp::Store { .. }] =
            rows.as_slice()
        else {
            panic!("the rows are the marker, the take of the token and the next value: {rows:?}");
        };
        assert_eq!(
            channel,
            &index_of(std::slice::from_ref(&s)),
            "the first row restores the marker on its prefix"
        );
        let rows = finish_rows_of(&QueueDiscipline, SpaceInner::Queue, &s, true, true).await;
        let [CapturedOp::Store { .. }] = rows.as_slice() else {
            panic!("a queue bumps no token in `finish`: {rows:?}");
        };
        let rows = finish_rows_of(&BagDiscipline, SpaceInner::Bag, &s, false, false).await;
        assert!(rows.is_empty(), "no owe and no change give no rows");
    }

    // ── Decision 124: the finish rows of a PathMap read over the lists of a real commit ────────

    /// Added by decision 124 (ruling RADD2v4 7.7): commits a fire of the first record on the path
    /// of `carrier`, whose discipline is `D`, under the observer `Funded`, in a transaction that
    /// also locks `keys`, the lock keys of a prefix scope, and the data channel of the path. It
    /// runs `probe` once, over the lists of that commit and before the fire changes the store,
    /// and returns the answer of `probe`. Each call needs a record of its own on the path.
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
        let key = CollectionKey::of_channel(carrier).expect("a channel has a key");
        let mut opened = begin_collection(&HashMapOuter, env, SPACE, &key, carrier)
            .await
            .expect("phase A of the carrier");
        let data = data_of(carrier);
        let mut locked = Vec::<Par>::with_capacity(keys.len() + 1);
        locked.extend_from_slice(keys);
        locked.push(data.clone());
        env.lock_phase_b(&mut opened.txn, &locked)
            .await
            .expect("phase B of the carrier");
        let CollectionTxn { txn, config, .. } = opened;
        let op = receive(carrier, "carrier");
        let authority = CostAuthority::default();
        let admission = Admission::of(&config, &op).expect("the admission of the carrier");
        let job = SpaceJob::of(&op);
        let (sender, answers) = std::sync::mpsc::channel::<T>();
        let mut probe = Some(probe);
        let prepared = run_plan(
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
                let payload = record::unwrap_datum(&stored.a)
                    .expect("a datum record")
                    .datum;
                let continuation = record::wrap_continuation(0, &TaggedContinuation {
                    tagged_cont: Some(TaggedCont::ParBody(body_of("carrier"))),
                    guard: None,
                    cost_authority: None,
                })
                .expect("a continuation record");
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

    /// Added by decision 124 (ruling RADD2v4 7.7): the answer, the end and the fields of the
    /// finish rows of the tail read of the fire of a send on a path where a receive waits, in a
    /// fresh PathMap space of the inner collection `inner`. The search of the send is staged
    /// over an owner-funded plan that commits nothing, and read over the lists of a carrier
    /// commit, so the store is the store before the send.
    async fn finish_read_of<D: Discipline>(
        inner: SpaceInner,
    ) -> (TailRead, TailEnd, Vec<CapturedOp>) {
        let (s, carrier) = (string("s"), string("carrier"));
        let (host, env) = pathmap_of(inner).await;
        run(&env, send(&carrier, 9)).await;
        let parked = run(&env, receive(&s, "k")).await;
        assert!(
            parked.fires.is_empty(),
            "the receive parks on the empty path"
        );
        let group = vec![index_of(std::slice::from_ref(&s))];
        let waiter = host
            .get_waiting_continuations(group.clone())
            .await
            .into_iter()
            .next()
            .expect("the receive waits on its path");
        let op = send(&s, 2);
        let mut keys = Vec::<Par>::new();
        prefix_plan(&env, &s, &op, &SpaceJob::of(&op), |_cx, scope| {
            keys = scope.lock_keys().to_vec();
            Ok(())
        })
        .await;
        let searches = staged_searches::<D>(&owner_funded(&env), &s, op).await;
        let mut search = searches
            .into_iter()
            .find(|search| matches!(search.runs().first(), Some(run) if run.channels == group))
            .expect("the fire of the send carries its row search");
        commit_lists::<D, _, _>(&host, &env, &carrier, &keys, move |lists| {
            let matcher = Matcher;
            let request = ParRowRequest {
                matcher: &matcher,
                group: &group,
                patterns: &waiter.patterns,
                continuation: &waiter.continuation,
                persistent: waiter.persist,
                source: &waiter.source,
                own: None,
                item: None,
            };
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
            let rows = search.finish_rows();
            let mut fields = Vec::<CapturedOp>::with_capacity(rows.len());
            for row in &rows {
                fields.push(match row {
                    FinishRow::Store(store) => CapturedOp::of_store(store),
                    FinishRow::Take(take) => CapturedOp::of_take(take),
                });
            }
            (read, end, fields)
        })
        .await
    }

    /// Added by decision 124 (plan v4 C7.11, rules CR-31 and CR-33, rulings RADD2v2 6.6 and
    /// RADD2v4 7.7): T-S8-PMJ-O2, the half that U4 does not cover. A read whose finish stages
    /// rows and a read whose finish stages none keep the end of the engine and answer the step
    /// of the engine. The bag finishes with the take and the store of its token, and the queue
    /// finishes with no row. The substitute of the commit of the send: the search of its fire is
    /// staged and read over the lists of a carrier commit, before the send, when the waiter still
    /// waits and no record of the send rests, so the search of a cascade finds no record to bind.
    /// U4 checks that these rows equal the rows of `finish`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pathmap_finish_rows_keep_the_end() {
        let commits = join::finish_read(join::TailStep::Commits);
        let (read, end, rows) = finish_read_of::<BagDiscipline>(SpaceInner::Bag).await;
        assert_eq!(read, commits, "the read answers the step of the engine");
        assert_eq!(
            end,
            TailEnd::Return(None),
            "the read keeps the end of the engine"
        );
        let [CapturedOp::Take { .. }, CapturedOp::Store { .. }] = rows.as_slice() else {
            panic!("the finish of the bag takes and stores its token: {rows:?}");
        };
        let (read, end, rows) = finish_read_of::<QueueDiscipline>(SpaceInner::Queue).await;
        assert_eq!(
            read, commits,
            "a read that finishes nothing answers the same step"
        );
        assert_eq!(
            end,
            TailEnd::Return(None),
            "a read that finishes nothing keeps the end"
        );
        assert!(
            rows.is_empty(),
            "the finish of the queue stages no row: {rows:?}"
        );
    }

    /// Added by decision 102 (rule CR-14, F6): R-A resets the state of the
    /// PathMap source. A failed staging can leave paths, the root flag, owed second sends and
    /// the change flag in the source. None of them reaches the restage.
    #[tokio::test]
    async fn space_r_a_resets_the_pathmap_source_state() {
        let s = string("s");
        let (_, env) = pathmap_of(SpaceInner::Bag).await;
        let op = send(&s, 9);
        prefix_plan(&env, &s, &op, &SpaceJob::of(&op), |_cx, scope| {
            let source = PrefixSource::new(&BagDiscipline, scope, &op)?;
            let home = home_of(scope, &s)?;
            let datum = Blake2b256Hash::new(&[1]);
            source.owe(&datum, 1, home);
            {
                let mut state = source.state.borrow_mut();
                state.paths.insert(datum.clone(), ints(&[1]));
                state.rooted = true;
                state.changed = true;
            }
            source.restart();
            let state = source.state.borrow();
            assert!(state.paths.is_empty(), "restart empties the paths");
            assert!(!state.rooted, "restart clears the root flag");
            assert!(state.owed.is_empty(), "restart empties the owed sends");
            assert!(!state.changed, "restart clears the change flag");
            Ok(())
        })
        .await;
    }
}
