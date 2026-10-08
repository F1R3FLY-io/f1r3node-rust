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
//! PREFIX-SCOPE(op):                                 ▷ every read and copy reserved first
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
use std::mem::size_of;
use std::sync::LazyLock;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{
    BindPattern, CostAuthority, ListParWithRandom, Par, ParWithRandom, TaggedContinuation,
};
use models::rust::utils::new_gbytearray_par;
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::internal::{ConsumeCandidate, Datum};
use rspace_plus_plus::rspace::space_store_handle::metered_hash;
use rspace_plus_plus::rspace::trace::event::Produce;

use super::adapter::HeadEPathMapAdapter;
use super::delivery::{
    consume_candidates_excluding, datum_marker, datum_markers, is_excluded, rerooted, retire_plan,
    PathCandidates, PrefixView, Retired,
};
use super::markers::{decode_marker_metered, vehicle_metered};
use super::path_codec::{channel_path, compare_keys, encode_path, is_prefix_key, path_key};
use super::EncodedPath;
use crate::rust::interpreter::reified::costs::ScanMeter;
use crate::rust::interpreter::spaces::agent::{
    ConsumeOp, JoinOp, Prepared, ProduceOp, SettleOp, SpaceEnv, SpaceOp, SpaceOutcome,
};
use crate::rust::interpreter::spaces::counter::CounterChannels;
use crate::rust::interpreter::spaces::disciplines::priority_queue::{Lane, Level};
use crate::rust::interpreter::spaces::disciplines::scan::{
    retire_continuation, retire_pattern, same_class,
};
use crate::rust::interpreter::spaces::errors::SpaceError;
use crate::rust::interpreter::spaces::generic::{
    self, CollectionTxn, Cx, Discipline, GenericSpace, JoinWindow, Ordered, OuterStore,
    ParCandidate, ParDatum, ParWaiter, Side, SideBound,
};
use crate::rust::interpreter::spaces::join::{
    self, Bound, Claim, Entry, EntryAt, FireSite, Incoming, JoinRoute, JoinScope, JoinSource,
    Placement, ScopedCollection, StoredEntry, WaiterRef, Work, PREFIX_KEY_TAG,
};
use crate::rust::interpreter::spaces::namespace::{
    self, CollectionKey, NativeSpace, RandomPurpose, ReifiedId,
};
use crate::rust::interpreter::spaces::outer::pathmap::PathMapOuter;
use crate::rust::interpreter::spaces::record::{self, JoinSlot, OperationAuthority, WaiterPolicy};
use crate::rust::interpreter::spaces::types::config::SpaceConfig;

/// The terminator byte of a path key.
const TERMINATOR: u8 = 0x00;

/// The template of the retire pattern. A reserved copy of it covers its allocations.
static RETIRE_PATTERN: LazyLock<BindPattern> = LazyLock::new(retire_pattern);

/// The template of a `K_retire` continuation: an empty body, no random state bytes and no cost
/// authority. A reserved copy of it, the reserved buffer of the hash bytes, the reserved random
/// bytes and the reserved copy of the authority cover the allocations of a new continuation, as
/// for the retire of a set class (`disciplines::scan`).
static RETIRE_TEMPLATE: LazyLock<TaggedContinuation> = LazyLock::new(|| TaggedContinuation {
    tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
        body: Some(new_gbytearray_par(Vec::new(), Vec::new(), false)),
        random_state: Vec::new(),
    })),
    guard: None,
    cost_authority: None,
});

// ── Routing ─────────────────────────────────────────────────────────────────────────────────

/// CLASSIFY of a PathMap operation (joins plan section 9): the checks of the operation, then
/// `Join` with its prefix scope. Every PathMap operation runs on the join engine, a single-bind
/// one included. `PathMapOuter::ALWAYS_ESCALATES` runs it in a new transaction over the lock keys
/// of the scope, because the first transaction locks only the channels of the routed key.
pub(crate) fn classify<D: Discipline>(
    agent: &GenericSpace<PathMapOuter, D>,
    env: &SpaceEnv,
    opened: &mut CollectionTxn,
    op: &SpaceOp,
) -> Result<JoinRoute, SpaceError> {
    let meter = &env.meter;
    validate(agent, &opened.config, op, meter)?;
    prefix_scope(opened, op, meter).map(JoinRoute::Join)
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
    let meter = &env.meter;
    let op = scoped_op(op, meter)?;
    let source = PrefixSource::new(agent.discipline(), &scope, &op, meter)?;
    join::run_join_with(env, opened, scope, op, &source)
}

/// The checks of a PathMap operation before any staging: the path rules of each channel
/// (`PathMapOuter::check_key`), and the params and the query with the schema of the discipline.
/// A join needs two or more binds, one arrow, and one carried order for each bind when it carries
/// orders. They are the checks that the keyed engine makes (`spaces::join`).
fn validate<D: Discipline>(
    agent: &GenericSpace<PathMapOuter, D>,
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
            }
            Ok(())
        }
    }
}

/// The operation `op` with the scope key of the path of each of its channels as its key. The
/// engine finds the own collections of the operation in the scope by these keys (seams
/// arbitration, section 2). A re-issue carries them, and the classification computes them again
/// from the channels.
fn scoped_op(op: SpaceOp, meter: &ScanMeter) -> Result<SpaceOp, SpaceError> {
    Ok(match op {
        SpaceOp::Produce(produce) => {
            let key = channel_key(&produce.channel, meter)?;
            SpaceOp::Produce(ProduceOp { key, ..produce })
        }
        SpaceOp::Consume(consume) => {
            let key = channel_key(&consume.channel, meter)?;
            SpaceOp::Consume(ConsumeOp { key, ..consume })
        }
        SpaceOp::Settle(settle) => {
            let key = channel_key(&settle.channel, meter)?;
            SpaceOp::Settle(SettleOp { key, ..settle })
        }
        SpaceOp::Join(join) => {
            let mut binds = join.binds;
            for bind in binds.iter_mut() {
                bind.key = channel_key(&bind.channel, meter)?;
            }
            SpaceOp::Join(JoinOp { binds, ..join })
        }
    })
}

// ── Keys ────────────────────────────────────────────────────────────────────────────────────

/// The scope key `[0x49] ‖ prefix_key(path)` of the path of `channel`. The read of the channel and
/// the encode of the path are reserved first.
fn channel_key(channel: &Par, meter: &ScanMeter) -> Result<CollectionKey, SpaceError> {
    let encoded = encode_path(channel_path(channel, meter)?, meter)?;
    scope_key(full_prefix(&encoded)?, meter)
}

/// The prefix key of the whole path that `encoded` holds: the path key without its terminator.
fn full_prefix(encoded: &EncodedPath) -> Result<&[u8], SpaceError> {
    encoded
        .prefix_key(encoded.depth())
        .ok_or(SpaceError::Inconsistent(
            "an encoded path has no prefix key of its depth",
        ))
}

/// The scope key `[0x49] ‖ prefix_key` of a prefix, with its buffer and the copy reserved first.
/// The key of the prefix `ε` is `[0x49]`, so no scope key is empty.
fn scope_key(prefix_key: &[u8], meter: &ScanMeter) -> Result<CollectionKey, SpaceError> {
    let length = prefix_key
        .len()
        .checked_add(1)
        .ok_or(RSpaceError::HostWorkRejected)?;
    let mut bytes = meter.buffer::<u8>(length)?;
    meter.reserve_copy_slice(prefix_key)?;
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
/// ascending key order, so the lookup is a binary search, and it is reserved first.
fn coll_of(scope: &JoinScope, prefix_key: &[u8], meter: &ScanMeter) -> Result<usize, SpaceError> {
    let length = prefix_key
        .len()
        .checked_add(1)
        .ok_or(RSpaceError::HostWorkRejected)?;
    meter.ordered_lookup(scope.collections.len(), length)?;
    scope
        .collections
        .binary_search_by(|collection| match collection.key.as_bytes().split_first() {
            Some((&PREFIX_KEY_TAG, rest)) => rest.cmp(prefix_key),
            Some((&first, _)) => first.cmp(&PREFIX_KEY_TAG),
            None => Ordering::Less,
        })
        .map_err(|_| SpaceError::Inconsistent("a prefix is outside its join scope"))
}

/// The path key `prefix_key ‖ 0x00` of the path whose prefix key is `prefix_key`, with its buffer
/// and the copy reserved first.
fn terminated(prefix_key: &[u8], meter: &ScanMeter) -> Result<Vec<u8>, SpaceError> {
    let length = prefix_key
        .len()
        .checked_add(1)
        .ok_or(RSpaceError::HostWorkRejected)?;
    let mut key = meter.buffer::<u8>(length)?;
    meter.reserve_copy_slice(prefix_key)?;
    key.extend_from_slice(prefix_key);
    key.push(TERMINATOR);
    Ok(key)
}

/// The key `left ‖ right` in one reserved buffer: the path key of `p = q ++ s` from the prefix key
/// of `q` and the path key of `s` (`PathMapPrefix.v`, `path_key_app`).
fn joined(left: &[u8], right: &[u8], meter: &ScanMeter) -> Result<Vec<u8>, SpaceError> {
    let length = left
        .len()
        .checked_add(right.len())
        .ok_or(RSpaceError::HostWorkRejected)?;
    let mut key = meter.buffer::<u8>(length)?;
    meter.charge(1, length, 0)?;
    key.extend_from_slice(left);
    key.extend_from_slice(right);
    Ok(key)
}

/// The depth of the prefix of the path that `encoded` holds whose prefix key is `prefix_key`.
/// Each comparison is reserved first. A key that is not a prefix key of the path is
/// `Inconsistent`.
fn depth_of(
    encoded: &EncodedPath,
    prefix_key: &[u8],
    meter: &ScanMeter,
) -> Result<usize, SpaceError> {
    for depth in 0..=encoded.depth() {
        let key = encoded.prefix_key(depth).ok_or(SpaceError::Inconsistent(
            "an encoded path has no prefix key of a depth",
        ))?;
        meter.compare(key.len(), prefix_key.len())?;
        if key == prefix_key {
            return Ok(depth);
        }
    }
    Err(SpaceError::Inconsistent(
        "a prefix key is not a prefix of its path",
    ))
}

/// The suffix `p∖q` of the path `path` under its prefix whose key is `prefix_key`. The encode of
/// the path and the copy of the suffix are reserved first.
fn suffix_under(
    path: &[Par],
    prefix_key: &[u8],
    meter: &ScanMeter,
) -> Result<Vec<Par>, SpaceError> {
    let encoded = encode_path(path, meter)?;
    let depth = depth_of(&encoded, prefix_key, meter)?;
    let suffix = path
        .get(depth..)
        .ok_or(SpaceError::Inconsistent("a prefix is deeper than its path"))?;
    Ok(meter.copy_slice(suffix)?)
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

/// The index of the record whose source hash is `source` among `records`. Each comparison is
/// reserved first. A missing record is `Inconsistent`.
fn locate(
    records: &[ParDatum],
    source: &Blake2b256Hash,
    meter: &ScanMeter,
) -> Result<usize, SpaceError> {
    for (index, record) in records.iter().enumerate() {
        meter.compare(source.0.len(), record.source.hash.0.len())?;
        if record.source.hash == *source {
            return Ok(index);
        }
    }
    Err(SpaceError::Inconsistent(
        "a marker names no datum of its path",
    ))
}

/// The index of the marker of the datum `source` among the records `markers` of one index
/// channel. Each decode and each comparison is reserved first. A missing marker breaks the marker
/// invariant and is `Inconsistent`.
fn locate_marker(
    markers: &[ParDatum],
    source: &Blake2b256Hash,
    meter: &ScanMeter,
) -> Result<usize, SpaceError> {
    for (index, record) in markers.iter().enumerate() {
        let view = decode_marker_metered(&record.a, meter)?;
        meter.compare(source.0.len(), source.0.len())?;
        if view.is_of(source) {
            return Ok(index);
        }
    }
    Err(SpaceError::Inconsistent(
        "a stored datum has no marker on a prefix of its path",
    ))
}

/// Sorts `values` ascending with a reserved comparison for each comparison that the sort makes:
/// an insertion sort, whose comparisons and moves depend on the values only, as the keyed engine
/// sorts.
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

    /// Adds the prefix `prefix_key`, and marks it as a data path when `data` is true. Each
    /// lookup, growth and copy is reserved first.
    fn add_prefix(
        &mut self,
        prefix_key: &[u8],
        data: bool,
        meter: &ScanMeter,
    ) -> Result<(), SpaceError> {
        meter.ordered_lookup(self.prefixes.len(), prefix_key.len())?;
        match self.prefixes.get_mut(prefix_key) {
            Some(flag) => {
                *flag = *flag || data;
            }
            None => {
                meter.tree_growth::<Vec<u8>, bool>(self.prefixes.len(), 1)?;
                let key = meter.copy_slice(prefix_key)?;
                generic::push_reserved(&mut self.todo, meter.copy_slice(prefix_key)?, meter)?;
                self.prefixes.insert(key, data);
            }
        }
        Ok(())
    }

    /// Adds every prefix of the path `path`, and marks the whole path as a data path.
    fn add_path(&mut self, path: &[Par], meter: &ScanMeter) -> Result<(), SpaceError> {
        let encoded = encode_path(path, meter)?;
        for prefix_key in encoded.prefix_keys() {
            self.add_prefix(prefix_key?, false, meter)?;
        }
        self.add_prefix(full_prefix(&encoded)?, true, meter)
    }

    /// The own prefixes of `op` in canonical group order, each once, and the positions of its
    /// binds in canonical group order (FR-1'): ascending by the stable hash of the index channel
    /// of each bind, then by bind index. A produce adds every prefix of its path, and a settle job
    /// adds its seeds and the paths of its owed second sends.
    fn own(
        &mut self,
        op: &SpaceOp,
        meter: &ScanMeter,
    ) -> Result<(Vec<Vec<u8>>, Vec<usize>), SpaceError> {
        let mut single = meter.buffer::<usize>(1)?;
        single.push(0);
        match op {
            SpaceOp::Produce(produce) => {
                let path = channel_path(&produce.channel, meter)?;
                let encoded = encode_path(path, meter)?;
                self.add_path(path, meter)?;
                let mut own = meter.buffer::<Vec<u8>>(1)?;
                own.push(meter.copy_slice(full_prefix(&encoded)?)?);
                Ok((own, single))
            }
            SpaceOp::Consume(consume) => {
                let own = self.own_channel(&consume.channel, meter)?;
                Ok((own, single))
            }
            SpaceOp::Settle(settle) => {
                let own = self.own_channel(&settle.channel, meter)?;
                if let Some(cascade) = &settle.cascade {
                    for seed in &cascade.seeds {
                        self.add_prefix(prefix_of_key(seed)?, false, meter)?;
                    }
                    for pending in &cascade.pending {
                        self.add_prefix(prefix_of_key(&pending.key)?, true, meter)?;
                    }
                }
                Ok((own, single))
            }
            SpaceOp::Join(join) => {
                let mut prefixes = meter.buffer::<Vec<u8>>(join.binds.len())?;
                let mut hashed = meter.buffer::<(Blake2b256Hash, usize)>(join.binds.len())?;
                for (index, bind) in join.binds.iter().enumerate() {
                    let encoded = encode_path(channel_path(&bind.channel, meter)?, meter)?;
                    let prefix_key = full_prefix(&encoded)?;
                    self.add_prefix(prefix_key, false, meter)?;
                    let channel = PathMapOuter::index_metered(self.space, prefix_key, meter)?;
                    hashed.push((metered_hash(&channel, meter)?, index));
                    prefixes.push(meter.copy_slice(prefix_key)?);
                }
                sort_reserved(&mut hashed, |(hash, _)| hash.0.len(), meter)?;
                let mut group = meter.buffer::<usize>(hashed.len())?;
                group.extend(hashed.into_iter().map(|(_, index)| index));
                let mut own = meter.buffer::<Vec<u8>>(group.len())?;
                for &position in &group {
                    let prefix_key = prefixes
                        .get(position)
                        .ok_or(SpaceError::Inconsistent("a group position names no bind"))?;
                    let mut seen = false;
                    for known in own.iter() {
                        meter.compare(known.len(), prefix_key.len())?;
                        if known == prefix_key {
                            seen = true;
                            break;
                        }
                    }
                    if !seen {
                        own.push(meter.copy_slice(prefix_key)?);
                    }
                }
                Ok((own, group))
            }
        }
    }

    /// The own prefix of the channel `channel` of a receive or a settle: the prefix of its whole
    /// path.
    fn own_channel(
        &mut self,
        channel: &Par,
        meter: &ScanMeter,
    ) -> Result<Vec<Vec<u8>>, SpaceError> {
        let encoded = encode_path(channel_path(channel, meter)?, meter)?;
        let prefix_key = full_prefix(&encoded)?;
        self.add_prefix(prefix_key, false, meter)?;
        let mut own = meter.buffer::<Vec<u8>>(1)?;
        own.push(meter.copy_slice(prefix_key)?);
        Ok(own)
    }

    /// The closure: each channel of each join group of each prefix, and, once a prefix of the
    /// closure holds a marker, every prefix of every stored datum. The root `I(ε)` holds one
    /// marker for each stored datum with its whole path (the marker invariant), so one read of
    /// the root gives every datum. Each read of a group, a channel and a record is reserved
    /// first.
    fn close(&mut self, opened: &mut CollectionTxn, meter: &ScanMeter) -> Result<(), SpaceError> {
        loop {
            while let Some(prefix_key) = self.todo.pop() {
                let channel = PathMapOuter::index_metered(self.space, &prefix_key, meter)?;
                let groups: Vec<Vec<Par>> = {
                    let joins = opened.txn.joins(&channel)?;
                    meter.copy_slice(joins)?
                };
                for group in &groups {
                    meter.inspect(group)?;
                    for member in group {
                        meter.inspect(member)?;
                        match namespace::reified_id_of(member) {
                            Some(ReifiedId::PathIndex(owner, tail)) if owner == self.space => {
                                let tail = meter.copy_slice(tail)?;
                                self.add_prefix(&tail, false, meter)?;
                            }
                            _ => {
                                return Err(SpaceError::Inconsistent(
                                    "a collection channel is part of a foreign join",
                                ))
                            }
                        }
                    }
                }
                meter.charge(1, 0, 0)?;
                if !opened.txn.data(&channel)?.is_empty() {
                    self.marked = true;
                }
            }
            match self.marked && !self.rooted {
                true => {
                    self.rooted = true;
                    let root = PathMapOuter::index_metered(self.space, &[], meter)?;
                    let paths: Vec<Vec<Par>> = {
                        let markers = opened.txn.data(&root)?;
                        let mut paths = meter.buffer::<Vec<Par>>(markers.len())?;
                        for marker in markers {
                            let view = decode_marker_metered(&marker.a, meter)?;
                            paths.push(meter.copy_slice(view.suffix)?);
                        }
                        paths
                    };
                    self.add_prefix(&[], false, meter)?;
                    for path in &paths {
                        self.add_path(path, meter)?;
                    }
                }
                false => return Ok(()),
            }
        }
    }

    /// The scope of the closure: one collection for each prefix in ascending key order, with its
    /// index channel and the token, and the lock keys `C(sid)`, `S(sid)`, then for each prefix
    /// its data channel when it is a data path and its index channel. Each name and each copy is
    /// reserved first.
    fn scope(
        self,
        opened: &CollectionTxn,
        own: Vec<Vec<u8>>,
        group: Vec<usize>,
        meter: &ScanMeter,
    ) -> Result<JoinScope, SpaceError> {
        let data_paths = self.prefixes.values().filter(|data| **data).count();
        let length = self
            .prefixes
            .len()
            .checked_add(data_paths)
            .and_then(|length| length.checked_add(2))
            .ok_or(RSpaceError::HostWorkRejected)?;
        let mut lock_keys = meter.buffer::<Par>(length)?;
        lock_keys.push(meter.copy(opened.channels.config())?);
        lock_keys.push(meter.copy(opened.channels.space_counter())?);
        let token = PathMapOuter::token_metered(self.space, meter)?;
        let mut collections = meter.buffer::<ScopedCollection>(self.prefixes.len())?;
        for (prefix_key, data) in &self.prefixes {
            if *data {
                let path_key = terminated(prefix_key, meter)?;
                lock_keys.push(PathMapOuter::path_data_metered(
                    self.space, &path_key, meter,
                )?);
            }
            let index = PathMapOuter::index_metered(self.space, prefix_key, meter)?;
            lock_keys.push(meter.copy(&index)?);
            collections.push(ScopedCollection {
                key: scope_key(prefix_key, meter)?,
                data: index,
                counter: meter.copy(&token)?,
            });
        }
        let mut own_keys = meter.buffer::<CollectionKey>(own.len())?;
        for prefix_key in &own {
            own_keys.push(scope_key(prefix_key, meter)?);
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
fn prefix_scope(
    opened: &mut CollectionTxn,
    op: &SpaceOp,
    meter: &ScanMeter,
) -> Result<JoinScope, SpaceError> {
    let mut closure = Closure::new(op.space());
    let (own, group) = closure.own(op, meter)?;
    closure.close(opened, meter)?;
    closure.scope(opened, own, group, meter)
}

// ── The prefix source ───────────────────────────────────────────────────────────────────────

/// A second send of this transaction whose marker of depth `depth` a peek COMM or a lift removed.
/// Its datum lives on the path of the scoped collection `home`.
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
pub(crate) struct SpentDatum {
    home: usize,
    source: Blake2b256Hash,
    delivered: Option<usize>,
}

/// A set class that a fire binds: its path, its key and the hash of its key.
pub(crate) struct SpentClass {
    home: usize,
    key: Vec<u8>,
    hash: Blake2b256Hash,
}

/// What a fire spends on a PathMap space: the datums to retire, then the set classes whose other
/// members retire.
#[derive(Default)]
pub(crate) struct PrefixSpent {
    datums: Vec<SpentDatum>,
    classes: Vec<SpentClass>,
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
/// Each name of a channel is reserved before it is made.
struct CxView<'v, 'c, 'p> {
    cx: &'v mut Cx<'c, 'p>,
    space: NativeSpace,
}

impl PrefixView for CxView<'_, '_, '_> {
    fn markers(&mut self, prefix_key: &[u8]) -> Result<&[ParDatum], SpaceError> {
        let channel = PathMapOuter::index_metered(self.space, prefix_key, self.cx.meter())?;
        self.cx.data_on(&channel)
    }

    fn records(&mut self, path_key: &[u8]) -> Result<&[ParDatum], SpaceError> {
        let channel = PathMapOuter::path_data_metered(self.space, path_key, self.cx.meter())?;
        self.cx.data_on(&channel)
    }

    fn waiters(&mut self, prefix_key: &[u8]) -> Result<&[ParWaiter], SpaceError> {
        let channel = PathMapOuter::index_metered(self.space, prefix_key, self.cx.meter())?;
        self.cx.waiters_on(&channel)
    }
}

impl<'a, D: Discipline> PrefixSource<'a, D> {
    /// The source of `op` over the scope `scope`. The names of the token, of the root and of the
    /// data channel of each scoped prefix are reserved first.
    fn new(
        discipline: &'a D,
        scope: &JoinScope,
        op: &SpaceOp,
        meter: &ScanMeter,
    ) -> Result<Self, SpaceError> {
        let space = scope.space;
        let token = PathMapOuter::token_metered(space, meter)?;
        let root = PathMapOuter::index_metered(space, &[], meter)?;
        let mut homes = meter.buffer::<Par>(scope.collections.len())?;
        for scoped in &scope.collections {
            let path_key = terminated(prefix_of_key(&scoped.key)?, meter)?;
            homes.push(PathMapOuter::path_data_metered(space, &path_key, meter)?);
        }
        let own_path = match op {
            SpaceOp::Produce(produce) => {
                Some(meter.copy_slice(channel_path(&produce.channel, meter)?)?)
            }
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
            JoinWindow::Lanes => Some(Lane::of_slot(slot.policy.level())),
            JoinWindow::Head | JoinWindow::Scan | JoinWindow::Classes => None,
        }
    }

    // ── Paths ──

    /// The whole path of the datum `source` from the paths that the transaction knows, copied.
    fn known_path(
        &self,
        source: &Blake2b256Hash,
        meter: &ScanMeter,
    ) -> Result<Option<Vec<Par>>, SpaceError> {
        let state = self.state.borrow();
        meter.ordered_lookup(state.paths.len(), source.0.len())?;
        match state.paths.get(source) {
            Some(path) => Ok(Some(meter.copy_slice(path)?)),
            None => Ok(None),
        }
    }

    /// Records the whole path `path` of the datum `source`.
    fn remember(
        &self,
        source: &Blake2b256Hash,
        path: &[Par],
        meter: &ScanMeter,
    ) -> Result<(), SpaceError> {
        let mut state = self.state.borrow_mut();
        meter.ordered_lookup(state.paths.len(), source.0.len())?;
        if !state.paths.contains_key(source) {
            meter.tree_growth::<Blake2b256Hash, Vec<Par>>(state.paths.len(), 1)?;
            let source = meter.copy(source)?;
            let path = meter.copy_slice(path)?;
            state.paths.insert(source, path);
        }
        Ok(())
    }

    /// The whole path of the stored datum `source`. The root `I(ε)` holds one marker for each
    /// stored datum with its whole path as the suffix, so one read of the root gives every path.
    /// A datum without a root marker breaks the marker invariant and is `Inconsistent`.
    fn datum_path(
        &self,
        cx: &mut Cx<'_, '_>,
        source: &Blake2b256Hash,
    ) -> Result<Vec<Par>, SpaceError> {
        let meter = cx.meter();
        if let Some(path) = self.known_path(source, meter)? {
            return Ok(path);
        }
        let rooted = self.state.borrow().rooted;
        if !rooted {
            let entries: Vec<(Blake2b256Hash, Vec<Par>)> = {
                let markers = cx.data_on(&self.root)?;
                let mut entries = meter.buffer::<(Blake2b256Hash, Vec<Par>)>(markers.len())?;
                for marker in markers {
                    let view = decode_marker_metered(&marker.a, meter)?;
                    meter.reserve_copy_slice(view.source)?;
                    entries.push((view.source_hash(), meter.copy_slice(view.suffix)?));
                }
                entries
            };
            for (known, path) in &entries {
                self.remember(known, path, meter)?;
            }
            self.state.borrow_mut().rooted = true;
            if let Some(path) = self.known_path(source, meter)? {
                return Ok(path);
            }
        }
        Err(SpaceError::Inconsistent(
            "a stored datum has no marker on the root prefix",
        ))
    }

    /// The path of the element `x`: the path of the produce for the element of the operation, and
    /// the path of its datum for a stored element.
    fn element_path(&self, cx: &mut Cx<'_, '_>, x: &Incoming) -> Result<Vec<Par>, SpaceError> {
        match (x.own, &self.own_path) {
            (true, Some(path)) => Ok(cx.meter().copy_slice(path)?),
            (true, None) => Err(SpaceError::Inconsistent(
                "the element of an operation has no path",
            )),
            (false, _) => self.datum_path(cx, &x.source.hash),
        }
    }

    /// [`PrefixSource::element_path`] without the staged view: the path of a stored element must
    /// be known already. A window of the element learns it before the engine asks.
    fn known_element_path(&self, x: &Incoming, meter: &ScanMeter) -> Result<Vec<Par>, SpaceError> {
        match (x.own, &self.own_path) {
            (true, Some(path)) => Ok(meter.copy_slice(path)?),
            (true, None) => Err(SpaceError::Inconsistent(
                "the element of an operation has no path",
            )),
            (false, _) => self
                .known_path(&x.source.hash, meter)?
                .ok_or(SpaceError::Inconsistent(
                    "the path of an element in flight is not known",
                )),
        }
    }

    // ── Markers and datums ──

    /// The marker at `index` of the index channel of `coll` with its datum on `D(p)`. `missing` is
    /// the error of an index outside the staged view. Each decode, key, name and comparison is
    /// reserved first.
    fn marker_datum(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        coll: usize,
        index: usize,
        missing: &'static str,
    ) -> Result<MarkerDatum, SpaceError> {
        let meter = cx.meter();
        let (source, suffix) = {
            let markers = cx.data_on(&collection(scope, coll)?.data)?;
            let marker = markers
                .get(index)
                .ok_or(SpaceError::Inconsistent(missing))?;
            let view = decode_marker_metered(&marker.a, meter)?;
            meter.reserve_copy_slice(view.source)?;
            (view.source_hash(), meter.copy_slice(view.suffix)?)
        };
        let suffix_bytes = path_key(&suffix, meter)?;
        let path_key = joined(prefix_at(scope, coll)?, &suffix_bytes, meter)?;
        let prefix_key = path_key
            .split_last()
            .map(|(_, rest)| rest)
            .ok_or(SpaceError::Inconsistent("a path key is empty"))?;
        let home = coll_of(scope, prefix_key, meter)?;
        let (record_index, persist) = {
            let records = cx.data_on(self.home_at(home)?)?;
            let record_index = locate(records, &source, meter)?;
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
    fn marker_depth(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        coll: usize,
        found: &MarkerDatum,
    ) -> Result<usize, SpaceError> {
        let meter = cx.meter();
        if prefix_at(scope, coll)?.is_empty() {
            self.remember(&found.source, &found.suffix, meter)?;
            return Ok(0);
        }
        let path = self.datum_path(cx, &found.source)?;
        path.len()
            .checked_sub(found.suffix.len())
            .ok_or(SpaceError::Inconsistent(
                "a marker suffix is longer than its path",
            ))
    }

    /// The record of the datum `source` on its data channel `D(p)`, copied.
    fn datum_record(
        &self,
        cx: &mut Cx<'_, '_>,
        home: usize,
        source: &Blake2b256Hash,
    ) -> Result<ParDatum, SpaceError> {
        let meter = cx.meter();
        let records = cx.data_on(self.home_at(home)?)?;
        let index = locate(records, source, meter)?;
        let record = records.get(index).ok_or(SpaceError::Inconsistent(
            "a record index is outside its path",
        ))?;
        Ok(meter.copy(record)?)
    }

    /// The claim of the datum at `record_index` of the path `home` with the payload `payload`:
    /// its class on its path for a set, and its record otherwise (seams design, open point 1).
    fn datum_claim(
        &self,
        home: usize,
        at: EntryAt,
        payload: &ListParWithRandom,
        meter: &ScanMeter,
    ) -> Result<Claim, SpaceError> {
        match D::JOIN_WINDOW {
            JoinWindow::Classes => match self.discipline.join_class_key(&payload.pars, meter)? {
                Some(key) => {
                    let hash = metered_hash(&key, meter)?;
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
    fn vehicle(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        x: &Incoming,
        coll: usize,
    ) -> Result<ListParWithRandom, SpaceError> {
        let meter = cx.meter();
        let path = self.element_path(cx, x)?;
        let suffix = suffix_under(&path, prefix_at(scope, coll)?, meter)?;
        vehicle_metered(&x.source.hash, suffix, &x.payload, meter)
    }

    // ── In flight ──

    /// Records the second send of the datum `source` whose marker of depth `depth` a peek COMM or
    /// a lift removes.
    fn owe(
        &self,
        source: &Blake2b256Hash,
        depth: usize,
        home: usize,
        meter: &ScanMeter,
    ) -> Result<(), SpaceError> {
        let mut state = self.state.borrow_mut();
        let owed = Owed {
            source: meter.copy(source)?,
            depth,
            home,
        };
        generic::push_reserved(&mut state.owed, owed, meter)
    }

    /// True when this transaction owes the marker of depth `depth` of the datum `source`, and
    /// forgets it. Each comparison is reserved first.
    fn settle_owed(
        &self,
        source: &Blake2b256Hash,
        depth: Option<usize>,
        meter: &ScanMeter,
    ) -> Result<bool, SpaceError> {
        let mut state = self.state.borrow_mut();
        let mut found = None;
        for (position, owed) in state.owed.iter().enumerate() {
            meter.compare(owed.source.0.len(), source.0.len())?;
            if owed.source == *source && depth.is_none_or(|depth| owed.depth == depth) {
                found = Some(position);
                break;
            }
        }
        match found {
            Some(position) => {
                state.owed.remove(position);
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// True when this transaction owes a marker of depth `depth` of the datum `source`. Each
    /// comparison is reserved first.
    fn owes(
        &self,
        source: &Blake2b256Hash,
        depth: usize,
        meter: &ScanMeter,
    ) -> Result<bool, SpaceError> {
        let state = self.state.borrow();
        for owed in &state.owed {
            meter.compare(owed.source.0.len(), source.0.len())?;
            if owed.source == *source && owed.depth == depth {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// The source hashes of the datums in flight: the second sends that this transaction owes,
    /// the element `incoming` when it is a stored datum, and each owed second send of `pending`
    /// with a `restore` depth. Each hash and each copy is reserved first.
    fn in_flight(
        &self,
        incoming: Option<&Incoming>,
        pending: &[Work],
        meter: &ScanMeter,
    ) -> Result<Vec<Blake2b256Hash>, SpaceError> {
        let mut excluded = Vec::new();
        {
            let state = self.state.borrow();
            for owed in &state.owed {
                generic::push_reserved(&mut excluded, meter.copy(&owed.source)?, meter)?;
            }
        }
        if let Some(x) = incoming.filter(|x| x.restore.is_some()) {
            generic::push_reserved(&mut excluded, meter.copy(&x.source.hash)?, meter)?;
        }
        for work in pending.iter().filter(|work| work.restore.is_some()) {
            let source = Produce::create_metered(
                self.home_at(work.coll)?,
                &work.record,
                work.persist,
                meter,
            )?;
            generic::push_reserved(&mut excluded, source.hash, meter)?;
        }
        Ok(excluded)
    }

    /// True when the data channel of the element `x` still holds its record. A second send that
    /// a settle job carries can find its datum spent by another job of the deploy.
    fn is_stored(&self, cx: &mut Cx<'_, '_>, x: &Incoming) -> Result<bool, SpaceError> {
        let meter = cx.meter();
        let records = cx.data_on(self.home_at(x.coll)?)?;
        for record in records {
            meter.compare(record.source.hash.0.len(), x.source.hash.0.len())?;
            if record.source.hash == x.source.hash {
                return Ok(true);
            }
        }
        Ok(false)
    }

    // ── Windows ──

    /// The stored datums under the prefix `coll` without the datums `excluded`, path by path in
    /// path-key order, each in the record order of its path (CONSUME-CANDIDATES).
    fn paths_under(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        coll: usize,
        excluded: &[Blake2b256Hash],
    ) -> Result<Vec<PathCandidates>, SpaceError> {
        let meter = cx.meter();
        let prefix_key = meter.copy_slice(prefix_at(scope, coll)?)?;
        let mut view = CxView {
            cx,
            space: self.space,
        };
        consume_candidates_excluding(
            &mut view,
            &prefix_key,
            excluded,
            &mut |records: &[ParDatum], meter: &ScanMeter| -> Result<Vec<usize>, SpaceError> {
                let mut order = meter.buffer::<usize>(records.len())?;
                order.extend(0..records.len());
                Ok(order)
            },
            meter,
        )
    }

    /// The marker of each record of the path `path`, by record index: `Some` for a datum of the
    /// window, `None` for a datum in flight.
    fn markers_by_record(
        &self,
        path: &PathCandidates,
        records: usize,
        meter: &ScanMeter,
    ) -> Result<Vec<Option<usize>>, SpaceError> {
        let mut markers = meter.buffer::<Option<usize>>(records)?;
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
    fn path_window(
        &self,
        cx: &mut Cx<'_, '_>,
        path_key: &[u8],
        path: Option<&PathCandidates>,
        need: usize,
        x: Option<&Incoming>,
        window: &mut Vec<Entry>,
    ) -> Result<(), SpaceError> {
        let meter = cx.meter();
        let data = PathMapOuter::path_data_metered(self.space, path_key, meter)?;
        let count = match path {
            Some(_) => cx.data_on(&data)?.len(),
            None => 0,
        };
        let markers = match path {
            Some(path) => self.markers_by_record(path, count, meter)?,
            None => Vec::new(),
        };
        match D::JOIN_WINDOW {
            JoinWindow::Scan | JoinWindow::Classes => {
                if path.is_some() {
                    for record in cx.canonical_data_order(&data)? {
                        if let Some(Some(marker)) = markers.get(record) {
                            generic::push_reserved(
                                window,
                                Entry {
                                    at: EntryAt::Stored(*marker),
                                    level: None,
                                },
                                meter,
                            )?;
                        }
                    }
                }
                if x.is_some() {
                    generic::push_reserved(
                        window,
                        Entry {
                            at: EntryAt::Incoming,
                            level: None,
                        },
                        meter,
                    )?;
                }
                Ok(())
            }
            JoinWindow::Head => {
                let mut records = meter.buffer::<(i64, usize)>(count)?;
                for (record, marker) in markers.iter().enumerate() {
                    let Some(marker) = marker else {
                        continue;
                    };
                    let ord = {
                        let stored = cx.data_on(&data)?;
                        let datum = stored.get(record).ok_or(SpaceError::Inconsistent(
                            "a record index is outside its path",
                        ))?;
                        self.record_slots(&datum.a, meter)?.0
                    };
                    records.push((ord, *marker));
                }
                for entry in self.head_window(&records, need, x, meter)? {
                    generic::push_reserved(window, entry, meter)?;
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
                        let stored = cx.data_on(&data)?;
                        let datum = stored.get(record).ok_or(SpaceError::Inconsistent(
                            "a record index is outside its path",
                        ))?;
                        self.record_slots(&datum.a, meter)?
                    };
                    let level = level.ok_or(SpaceError::Inconsistent(
                        "a record of a priority queue has no level",
                    ))?;
                    meter.ordered_lookup(levels.len(), size_of::<Level>())?;
                    if !levels.contains_key(&level) {
                        meter.tree_growth::<Level, Vec<(i64, usize)>>(levels.len(), 1)?;
                    }
                    generic::push_reserved(
                        levels.entry(level).or_default(),
                        (ord, *marker),
                        meter,
                    )?;
                }
                if let Some(x) = x {
                    let level = x.level.ok_or(SpaceError::Inconsistent(
                        "an element of a priority queue has no level",
                    ))?;
                    meter.ordered_lookup(levels.len(), size_of::<Level>())?;
                    if !levels.contains_key(&level) {
                        meter.tree_growth::<Level, Vec<(i64, usize)>>(levels.len(), 1)?;
                        levels.insert(level, Vec::new());
                    }
                }
                for (level, records) in &levels {
                    let in_level = x.filter(|x| x.level == Some(*level));
                    for entry in self.head_window(records, need, in_level, meter)? {
                        generic::push_reserved(
                            window,
                            Entry {
                                at: entry.at,
                                level: Some(*level),
                            },
                            meter,
                        )?;
                    }
                }
                Ok(())
            }
        }
    }

    /// The order of the place of an element with the order `ord` in a side whose stored orders
    /// are `stored` (joins plan section 3.1), as the keyed source places it. An equal order is
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

    /// The first `need` entries of `entries` (order, marker index) in the head order of the
    /// discipline: the head, then the head of the rest, and so on. Each scan is reserved.
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

    // ── Waiters ──

    /// The collection of each channel of `group`: each channel is an index channel of the space.
    fn group_colls(
        &self,
        scope: &JoinScope,
        group: &[Par],
        meter: &ScanMeter,
    ) -> Result<Vec<usize>, SpaceError> {
        let mut colls = meter.buffer::<usize>(group.len())?;
        for channel in group {
            meter.inspect(channel)?;
            match namespace::reified_id_of(channel) {
                Some(ReifiedId::PathIndex(owner, tail)) if owner == self.space => {
                    colls.push(coll_of(scope, tail, meter)?);
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
    /// with its order and its lanes there. Each copy, decode and comparison is reserved.
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

    /// The head of the seats in the lane `lane` (every seat for `None`) by the head rule of the
    /// discipline.
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

    /// The lanes of the slots `slots` on the collection `coll` of the positions `colls`, each once.
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

    /// The references of the seats `heads` in ascending order of the seats, the older first.
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

    /// Every waiter of the index channel of `coll` in the canonical order of DR-75 of the union
    /// of its join groups: a merge of the canonical orders of the groups by the source hash of
    /// each consume (decision 41). A tie across groups is `Inconsistent`.
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
                            Ordering::Less => Some((group, hash)),
                            Ordering::Greater => Some((best, best_hash)),
                            Ordering::Equal => {
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
                let level = level.ok_or(SpaceError::Inconsistent(
                    "an element of a priority queue has no level",
                ))?;
                let seats = self.seats(cx, scope, coll)?;
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
            JoinWindow::Scan | JoinWindow::Classes => self.union_order(cx, scope, coll),
        }
    }

    // ── Stores and retires ──

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
        let meter = cx.meter();
        let path = self.datum_path(cx, source)?;
        let record = self.datum_record(cx, home, source)?;
        let marker = datum_marker(&path, source, &record.a, depth, meter)?;
        let encoded = encode_path(&path, meter)?;
        let prefix_key = encoded.prefix_key(depth).ok_or(SpaceError::Inconsistent(
            "an encoded path has no prefix key of a depth",
        ))?;
        let channel = PathMapOuter::index_metered(self.space, prefix_key, meter)?;
        cx.restore_on(&channel, marker)?;
        coll_of(scope, prefix_key, meter)
    }

    /// The `K_retire` of the record of the datum `source` on `channel`: the datum record on its
    /// data channel, or one of its markers on an index channel. The continuation carries the
    /// random state `random` and the authority `authority`, and its body is the source hash of the
    /// retired record, so each retire has an identity of its own. Each copy and decode is reserved.
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
        let meter = cx.meter();
        let (index, stored) = {
            let records = cx.data_on(channel)?;
            let index = match datum {
                true => locate(records, source, meter)?,
                false => locate_marker(records, source, meter)?,
            };
            let stored = records.get(index).ok_or(SpaceError::Inconsistent(
                "a retired index is outside its channel",
            ))?;
            (index, meter.copy(stored)?)
        };
        let payload = match datum {
            true => self.payload_of(&stored.a, meter)?,
            false => meter.copy(&stored.a)?,
        };
        let matched = cx
            .bind(&RETIRE_PATTERN, &payload)?
            .ok_or(SpaceError::Inconsistent(
                "a retired record does not bind the retire pattern",
            ))?;
        meter.reserve_copy(&*RETIRE_TEMPLATE)?;
        meter.reserve_buffer::<u8>(stored.source.hash.0.len())?;
        meter.random_bytes(random)?;
        let authority = authority
            .map(|authority| meter.copy(authority))
            .transpose()?;
        let continuation = retire_continuation(&stored.source.hash, random, authority);
        let mut channels = meter.buffer::<Par>(1)?;
        channels.push(meter.copy(channel)?);
        let mut patterns = meter.buffer::<BindPattern>(1)?;
        patterns.push(meter.copy(&*RETIRE_PATTERN)?);
        let mut candidates = meter.buffer::<ParCandidate>(1)?;
        let Datum {
            a,
            persist,
            source: produce,
        } = stored;
        candidates.push(ConsumeCandidate {
            channel: meter.copy(channel)?,
            datum: Datum {
                a: matched,
                persist,
                source: produce,
            },
            removed_datum: a,
            datum_index: view_index(index)?,
        });
        cx.retire(channels, patterns, continuation, candidates)
    }

    /// RETIRE-PLAN of the spent datum `spent`: the `K_retire` of its record on `D(p)`, then of its
    /// marker on each prefix of `p` in depth order, without the marker of the delivered depth
    /// (`delivery::retire_plan`). It marks every prefix of `p` dirty (`PathMapPrefix.v`,
    /// `dirty_of`).
    #[allow(clippy::too_many_arguments)]
    fn retire_datum(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        spent: &SpentDatum,
        random: &Blake2b512Random,
        authority: Option<&CostAuthority>,
        dirty: &mut [bool],
    ) -> Result<(), SpaceError> {
        let meter = cx.meter();
        let path = self.datum_path(cx, &spent.source)?;
        let encoded = encode_path(&path, meter)?;
        for step in retire_plan(path.len(), spent.delivered) {
            match step {
                Retired::Datum => {
                    let channel = meter.copy(self.home_at(spent.home)?)?;
                    self.retire_record(cx, &channel, &spent.source, true, random, authority)?;
                }
                Retired::Marker(depth) => {
                    let prefix_key = encoded.prefix_key(depth).ok_or(SpaceError::Inconsistent(
                        "an encoded path has no prefix key of a depth",
                    ))?;
                    let channel = PathMapOuter::index_metered(self.space, prefix_key, meter)?;
                    self.retire_record(cx, &channel, &spent.source, false, random, authority)?;
                }
            }
        }
        for prefix_key in encoded.prefix_keys() {
            let coll = coll_of(scope, prefix_key?, meter)?;
            join::mark(dirty, &[coll]);
        }
        self.settle_owed(&spent.source, None, meter)?;
        Ok(())
    }

    /// The stored members of the set class `class` on its path that are not persistent, not
    /// retired already and not in flight, in the canonical order of DR-75.
    fn class_members(
        &self,
        cx: &mut Cx<'_, '_>,
        class: &SpentClass,
        retired: &[Blake2b256Hash],
    ) -> Result<Vec<Blake2b256Hash>, SpaceError> {
        let meter = cx.meter();
        let excluded = self.in_flight(None, &[], meter)?;
        let channel = meter.copy(self.home_at(class.home)?)?;
        let order = cx.canonical_data_order(&channel)?;
        let mut members = Vec::new();
        for index in order {
            let candidate = {
                let records = cx.data_on(&channel)?;
                let record = records.get(index).ok_or(SpaceError::Inconsistent(
                    "a canonical index is outside the staged view",
                ))?;
                match record.persist {
                    true => None,
                    false => Some((
                        meter.copy(&record.source.hash)?,
                        self.payload_of(&record.a, meter)?,
                    )),
                }
            };
            let Some((source, payload)) = candidate else {
                continue;
            };
            if is_excluded(&source, retired, meter)? || is_excluded(&source, &excluded, meter)? {
                continue;
            }
            let Some(key) = self.discipline.join_class_key(&payload.pars, meter)? else {
                continue;
            };
            if same_class(&class.key, &class.hash, &key, meter)? {
                generic::push_reserved(&mut members, source, meter)?;
            }
        }
        Ok(members)
    }

    /// The spent datum of the marker `marker` that a fire on the prefix `coll` removes, or `None`
    /// for a persistent marker, which a fire never removes.
    fn spent_marker(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        coll: usize,
        index: usize,
    ) -> Result<Option<SpentDatum>, SpaceError> {
        let found = self.marker_datum(
            cx,
            scope,
            coll,
            index,
            "a bound index is outside the staged view",
        )?;
        if found.persist {
            return Ok(None);
        }
        let depth = self.marker_depth(cx, scope, coll, &found)?;
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
    fn spent_element(
        &self,
        x: &Incoming,
        meter: &ScanMeter,
    ) -> Result<Option<SpentDatum>, SpaceError> {
        if x.persist {
            return Ok(None);
        }
        let Some(depth) = x.restore else {
            return Ok(None);
        };
        let depth = usize::from(depth);
        let delivered = match self.owes(&x.source.hash, depth, meter)? {
            true => Some(depth),
            false => None,
        };
        Ok(Some(SpentDatum {
            home: x.coll,
            source: meter.copy(&x.source.hash)?,
            delivered,
        }))
    }

    /// Records the class of the claim `claim` in `classes`, once for each class of a path.
    fn spent_class(
        &self,
        claim: &Claim,
        classes: &mut Vec<SpentClass>,
        meter: &ScanMeter,
    ) -> Result<(), SpaceError> {
        let Claim::Class { coll, key, hash } = claim else {
            return Ok(());
        };
        for known in classes.iter() {
            meter.compare(size_of::<usize>(), size_of::<usize>())?;
            if known.home == *coll && same_class(&known.key, &known.hash, key, meter)? {
                return Ok(());
            }
        }
        meter.reserve_copy_slice(key)?;
        meter.reserve_copy(hash)?;
        generic::push_reserved(
            classes,
            SpentClass {
                home: *coll,
                key: key.clone(),
                hash: hash.clone(),
            },
            meter,
        )
    }
}

impl<D: Discipline> JoinSource for PrefixSource<'_, D> {
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
    fn window(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        coll: usize,
        need: usize,
        incoming: Option<&Incoming>,
        pending: &[Work],
    ) -> Result<Vec<Entry>, SpaceError> {
        let meter = cx.meter();
        let excluded = self.in_flight(incoming, pending, meter)?;
        let entering = match incoming {
            Some(x) => match self.enters(scope, x, coll, meter)? {
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
                    self.datum_path(cx, &x.source.hash)?;
                }
                Some(terminated(prefix_at(scope, x.coll)?, meter)?)
            }
            None => None,
        };
        let paths = self.paths_under(cx, scope, coll, &excluded)?;
        let mut window: Vec<Entry> = Vec::new();
        let mut placed = x_key.is_none();
        for path in &paths {
            if let (false, Some(x_key)) = (placed, &x_key) {
                match compare_keys(x_key, &path.path_key, meter)? {
                    Ordering::Less => {
                        self.path_window(cx, x_key, None, need, entering, &mut window)?;
                        placed = true;
                    }
                    Ordering::Equal => {
                        self.path_window(
                            cx,
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
            self.path_window(cx, &path.path_key, Some(path), need, None, &mut window)?;
        }
        if let (false, Some(x_key)) = (placed, &x_key) {
            self.path_window(cx, x_key, None, need, entering, &mut window)?;
        }
        Ok(window)
    }

    fn claim(
        &self,
        coll: usize,
        at: EntryAt,
        payload: &ListParWithRandom,
        meter: &ScanMeter,
    ) -> Result<Claim, SpaceError> {
        self.datum_claim(coll, at, payload, meter)
    }

    /// PRODUCE-WAITERS of the path of `incoming`: the waiters of each prefix of the path in the
    /// scope, the most specific prefix first, each prefix in the waiter order of the discipline.
    /// A second send that a settle job carries meets no waiter when another job spent its datum
    /// first, and its store then stages nothing.
    fn offer_order(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        incoming: &Incoming,
    ) -> Result<Vec<WaiterRef>, SpaceError> {
        let meter = cx.meter();
        if let Some(depth) = incoming.restore {
            let owed = self.owes(&incoming.source.hash, usize::from(depth), meter)?;
            if !owed && !self.is_stored(cx, incoming)? {
                return Ok(Vec::new());
            }
        }
        let path_prefix = meter.copy_slice(prefix_at(scope, incoming.coll)?)?;
        let mut prefixes: Vec<usize> = Vec::new();
        for (coll, scoped) in scope.collections.iter().enumerate() {
            if is_prefix_key(prefix_of_key(&scoped.key)?, &path_prefix, meter)? {
                generic::push_reserved(&mut prefixes, coll, meter)?;
            }
        }
        // The prefixes of one path are nested byte prefixes in ascending key order, so the
        // reverse order is the most specific prefix first.
        prefixes.reverse();
        let mut order = Vec::new();
        for coll in prefixes {
            for waiter in self.waiters_of(cx, scope, coll, incoming.level)? {
                generic::push_reserved(&mut order, waiter, meter)?;
            }
        }
        Ok(order)
    }

    /// The heads of the index channel of `coll` that a cascade tests: the head waiter
    /// (head-only), the lane heads older first (priority queue), or every waiter in the canonical
    /// order for a set (decision 50). A bag has none.
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
            JoinWindow::Classes => self.union_order(cx, scope, coll),
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
            JoinWindow::Scan | JoinWindow::Classes => Ok(true),
            JoinWindow::Head | JoinWindow::Lanes => {
                let seats = self.seats(cx, scope, coll)?;
                for lane in self.lanes_on(slots, colls, coll, meter)? {
                    let filter = match D::JOIN_WINDOW {
                        JoinWindow::Lanes => lane,
                        JoinWindow::Head | JoinWindow::Scan | JoinWindow::Classes => None,
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
            JoinWindow::Scan | JoinWindow::Classes => Ok(true),
            JoinWindow::Head | JoinWindow::Lanes => {
                let seats = self.seats(cx, scope, coll)?;
                for lane in self.lanes_on(slots, colls, coll, meter)? {
                    let filter = match D::JOIN_WINDOW {
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

    /// The marker at `index` of `I(q)` as a bind sees it: the payload of its datum re-rooted under
    /// the suffix of the marker, the persistence of the datum, and the claim of the datum on its
    /// path, so two markers of one datum on nested prefixes claim one entry.
    fn stored_entry(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        coll: usize,
        index: usize,
        missing: &'static str,
    ) -> Result<StoredEntry, SpaceError> {
        let meter = cx.meter();
        let found = self.marker_datum(cx, scope, coll, index, missing)?;
        let own = {
            let records = cx.data_on(self.home_at(found.home)?)?;
            let record = records
                .get(found.record_index)
                .ok_or(SpaceError::Inconsistent(
                    "a record index is outside its path",
                ))?;
            self.payload_of(&record.a, meter)?
        };
        let claim =
            self.datum_claim(found.home, EntryAt::Stored(found.record_index), &own, meter)?;
        let payload = rerooted(&found.suffix, own, &HeadEPathMapAdapter, meter)?;
        Ok(StoredEntry {
            payload,
            persist: found.persist,
            claim,
        })
    }

    /// The markers of `I(q)` in the order of CONSUME-CANDIDATES (path-key order, then the
    /// canonical order of DR-75 within a path), without the datums in flight.
    fn members(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        coll: usize,
    ) -> Result<Vec<usize>, SpaceError> {
        let meter = cx.meter();
        let excluded = self.in_flight(None, &[], meter)?;
        let paths = self.paths_under(cx, scope, coll, &excluded)?;
        let mut order = Vec::new();
        for path in &paths {
            let data = PathMapOuter::path_data_metered(self.space, &path.path_key, meter)?;
            let count = cx.data_on(&data)?.len();
            let markers = self.markers_by_record(path, count, meter)?;
            for record in cx.canonical_data_order(&data)? {
                if let Some(Some(marker)) = markers.get(record) {
                    generic::push_reserved(&mut order, *marker, meter)?;
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
        let meter = cx.meter();
        let marker = {
            let markers = cx.data_on(&collection(scope, coll)?.data)?;
            let marker = markers.get(index).ok_or(SpaceError::Inconsistent(
                "a bound index is outside the staged view",
            ))?;
            meter.copy(marker)?
        };
        let found = self.marker_datum(
            cx,
            scope,
            coll,
            index,
            "a bound index is outside the staged view",
        )?;
        let stored = self.datum_record(cx, found.home, &found.source)?;
        let (ord, level) = self.record_slots(&stored.a, meter)?;
        let payload = self.payload_of(&stored.a, meter)?;
        let introduction = cx.introduction().for_record(&stored.a, meter)?;
        let restore = match in_flight {
            true => {
                let depth = self.marker_depth(cx, scope, coll, &found)?;
                self.owe(&found.source, depth, found.home, meter)?;
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
        let found = self.marker_datum(
            cx,
            scope,
            coll,
            index,
            "a bound index is outside the staged view",
        )?;
        let depth = self.marker_depth(cx, scope, coll, &found)?;
        let stored = self.datum_record(cx, found.home, &found.source)?;
        self.owe(&found.source, depth, found.home, meter)?;
        Ok(Some(Work {
            coll: found.home,
            record: stored.a,
            persist: false,
            own: false,
            restore: Some(restore_depth(depth)?),
        }))
    }

    /// The identity of the vehicle of `x` on the index channel of `coll`.
    fn incoming_source(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        x: &Incoming,
        coll: usize,
    ) -> Result<Produce, SpaceError> {
        let vehicle = self.vehicle(cx, scope, x, coll)?;
        let meter = cx.meter();
        Ok(Produce::create_metered(
            &collection(scope, coll)?.data,
            &vehicle,
            x.persist,
            meter,
        )?)
    }

    /// The vehicle of `x` on the index channel of `coll`, which no store keeps.
    fn incoming_record(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        x: &Incoming,
        coll: usize,
    ) -> Result<ListParWithRandom, SpaceError> {
        self.vehicle(cx, scope, x, coll)
    }

    /// True when the prefix `coll` is a prefix of the path of `x` (`prefix_key_iff_prefix`).
    fn enters(
        &self,
        scope: &JoinScope,
        x: &Incoming,
        coll: usize,
        meter: &ScanMeter,
    ) -> Result<bool, SpaceError> {
        is_prefix_key(prefix_at(scope, coll)?, prefix_at(scope, x.coll)?, meter)
    }

    /// The payload of `x` re-rooted under the suffix of its path below the prefix `coll`.
    fn incoming_payload<'x>(
        &self,
        scope: &JoinScope,
        x: &'x Incoming,
        coll: usize,
        meter: &ScanMeter,
    ) -> Result<Cow<'x, ListParWithRandom>, SpaceError> {
        if coll == x.coll {
            return Ok(Cow::Borrowed(&x.payload));
        }
        let path = self.known_element_path(x, meter)?;
        let suffix = suffix_under(&path, prefix_at(scope, coll)?, meter)?;
        let payload = meter.copy(&x.payload)?;
        Ok(Cow::Owned(rerooted(
            &suffix,
            payload,
            &HeadEPathMapAdapter,
            meter,
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
    ) -> Result<(), SpaceError> {
        let meter = cx.meter();
        match x.restore {
            None => {
                let path = self.element_path(cx, &x)?;
                let encoded = encode_path(&path, meter)?;
                let markers = datum_markers(&path, &x.source.hash, &x.record, meter)?;
                self.remember(&x.source.hash, &path, meter)?;
                let home = meter.copy(self.home_at(x.coll)?)?;
                cx.store_on(&home, x.record, x.persist, &x.introduction)?;
                for planned in markers {
                    let prefix_key =
                        encoded
                            .prefix_key(planned.depth)
                            .ok_or(SpaceError::Inconsistent(
                                "an encoded path has no prefix key of a depth",
                            ))?;
                    let channel = PathMapOuter::index_metered(self.space, prefix_key, meter)?;
                    let introduction = cx.introduction().for_record(&planned.record, meter)?;
                    cx.store_on(&channel, planned.record, x.persist, &introduction)?;
                    join::mark(dirty, &[coll_of(scope, prefix_key, meter)?]);
                }
                Ok(())
            }
            Some(depth) => {
                let depth = usize::from(depth);
                if self.settle_owed(&x.source.hash, Some(depth), meter)? {
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
    fn before_fire(
        &self,
        cx: &mut Cx<'_, '_>,
        scope: &JoinScope,
        site: FireSite,
        colls: &[usize],
        bound: &[Bound],
        members: &[ParCandidate],
        x: Option<&Incoming>,
    ) -> Result<PrefixSpent, SpaceError> {
        let meter = cx.meter();
        let mut spent = PrefixSpent::default();
        match site {
            FireSite::Single => {
                let coll = *colls
                    .first()
                    .ok_or(SpaceError::Inconsistent("a consume has no collection"))?;
                for candidate in members {
                    if candidate.datum.persist {
                        continue;
                    }
                    let index = usize::try_from(candidate.datum_index)
                        .map_err(|_| SpaceError::Inconsistent("a candidate has no view index"))?;
                    if let Some(datum) = self.spent_marker(cx, scope, coll, index)? {
                        generic::push_reserved(&mut spent.datums, datum, meter)?;
                    }
                }
            }
            FireSite::Offer | FireSite::Join | FireSite::Cascade => {
                let lifted = match site {
                    FireSite::Cascade => {
                        Some(bound.iter().position(|entry| !entry.persist).unwrap_or(0))
                    }
                    FireSite::Offer | FireSite::Join | FireSite::Single => None,
                };
                for (position, (entry, &coll)) in bound.iter().zip(colls).enumerate() {
                    let datum = match (entry.at, lifted == Some(position)) {
                        (EntryAt::Incoming, _) | (EntryAt::Stored(_), true) => match x {
                            Some(x) => self.spent_element(x, meter)?,
                            None => {
                                return Err(SpaceError::Inconsistent(
                                    "a fire binds an element in flight, but none is in flight",
                                ))
                            }
                        },
                        (EntryAt::Stored(index), false) => {
                            self.spent_marker(cx, scope, coll, index)?
                        }
                    };
                    if let Some(datum) = datum {
                        generic::push_reserved(&mut spent.datums, datum, meter)?;
                    }
                    self.spent_class(&entry.claim, &mut spent.classes, meter)?;
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
        let meter = cx.meter();
        let retire_random = RandomPurpose::Retire.derive(random);
        let mut retired: Vec<Blake2b256Hash> = Vec::new();
        for datum in &spent.datums {
            self.retire_datum(cx, scope, datum, &retire_random, authority, dirty)?;
            generic::push_reserved(&mut retired, meter.copy(&datum.source)?, meter)?;
        }
        for class in &spent.classes {
            for source in self.class_members(cx, class, &retired)? {
                let member = SpentDatum {
                    home: class.home,
                    source,
                    delivered: None,
                };
                self.retire_datum(cx, scope, &member, &retire_random, authority, dirty)?;
                self.state.borrow_mut().changed = true;
                generic::push_reserved(&mut retired, member.source, meter)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use models::rust::utils::new_gint_par;

    use super::*;

    fn unmetered() -> ScanMeter { ScanMeter::new(None) }

    fn ints(values: &[i64]) -> Vec<Par> {
        values
            .iter()
            .map(|value| new_gint_par(*value, Vec::new(), false))
            .collect()
    }

    #[test]
    fn a_scope_key_tags_its_prefix_and_is_never_empty() {
        let meter = unmetered();
        let root = scope_key(&[], &meter).expect("the scope key of the root");
        assert_eq!(root.as_bytes(), &[PREFIX_KEY_TAG]);
        assert_eq!(prefix_of_key(&root).expect("a tagged key"), &[] as &[u8]);
        let encoded = encode_path(&ints(&[0, 1]), &meter).expect("a path");
        let key = scope_key(full_prefix(&encoded).expect("the prefix"), &meter).expect("a key");
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
        let meter = unmetered();
        let path = ints(&[3, 4, 5]);
        let encoded = encode_path(&path, &meter).expect("a path");
        let prefix = full_prefix(&encoded).expect("the prefix");
        assert_eq!(
            terminated(prefix, &meter).expect("a key"),
            path_key(&path, &meter).expect("the path key")
        );
    }

    #[test]
    fn a_suffix_under_a_prefix_is_the_rest_of_the_path() {
        let meter = unmetered();
        let path = ints(&[0, 1, 2]);
        let encoded = encode_path(&path, &meter).expect("a path");
        for depth in 0..=path.len() {
            let prefix = encoded.prefix_key(depth).expect("a prefix key");
            assert_eq!(depth_of(&encoded, prefix, &meter).expect("a depth"), depth);
            assert_eq!(
                suffix_under(&path, prefix, &meter).expect("a suffix"),
                path[depth..].to_vec()
            );
        }
        let other = encode_path(&ints(&[9]), &meter).expect("a path");
        assert!(depth_of(&encoded, full_prefix(&other).expect("a prefix"), &meter).is_err());
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
        let meter = unmetered();
        let mut closure = Closure::new(NativeSpace {
            sid: [7; namespace::SID_LEN],
            durability: namespace::Durability::Durable,
        });
        let path = ints(&[0, 1]);
        closure.add_path(&path, &meter).expect("a path");
        let encoded = encode_path(&path, &meter).expect("a path");
        let keys: Vec<Vec<u8>> = encoded
            .prefix_keys()
            .map(|key| key.expect("a key").to_vec())
            .collect();
        assert_eq!(closure.prefixes.keys().cloned().collect::<Vec<_>>(), keys);
        let data: Vec<bool> = closure.prefixes.values().copied().collect();
        assert_eq!(data, vec![false, false, true]);
        assert_eq!(closure.todo.len(), 3);
        closure
            .add_prefix(&keys[0], true, &meter)
            .expect("a prefix");
        assert_eq!(closure.prefixes.get(&keys[0]), Some(&true));
        assert_eq!(closure.todo.len(), 3, "a known prefix is not visited again");
    }
}
