//! The VectorDB discipline of the native spaces (P6, design v3 4.2-4.6; decisions 54, 60 and 66;
//! the families `hashmap:vectordb` and `vector:vectordb`).
//!
//! A VectorDB collection keeps records with vectors, and a receive selects records by
//! similarity. Every decision is exact (decision 54): the numeric core of
//! `interpreter::tensor` scores each record with exact rationals, and the selection order is the
//! better score first, then the lower rank in the canonical order of DR-75 (decisions 41 and
//! 60). The collection has no counter, so every record has the `ord` 0 (M-1), and a re-issued
//! element keeps its record and its identity (decision 43).
//!
//! This module holds every VectorDB rule. The driver of this module runs the operations of a
//! collection without a join group, and the join engine (`spaces::join`) runs the others with
//! the same functions: [`vdb_config`], [`query_of`], [`policy_of_query`], [`policy_query`],
//! [`vdb_record`], [`eligible`], [`flight_eligibility`], [`single_data`], [`list_data`],
//! [`list_selection`], [`list_offer`], [`retire_members`] and [`lift_record`].
//!
//! **Records (design v3 4.3).** A datum record keeps envelope version 1 and fills the embedding
//! slot: the `embedding` param of the send, or else the payload itself when it is exactly one
//! non-empty list of numbers (decision 66). A numeric list of the wrong length fails with
//! `DimensionMismatch`, and a non-finite element with `NonFiniteValue`. Any other payload has no
//! vector, and it is never eligible for a query with a vector. The `capacity` of the space bounds
//! the records of each collection (`CapacityExceeded`), and nothing bounds the waiters. A parked
//! waiter keeps its query in its policy slot: the map of the named params, with the query vector
//! under `q`.
//!
//! **Queries (design v3 4.4).** A receive with a query `(q; named)` has the metric `metric` or
//! the metric of the space. Without `q`, the keys `ids`, `metric`, `radius`, `scores` and
//! `threshold` fail with `QueryNeedsVector`, and the receive reads the records in canonical
//! order. The eligible records `E` are the records whose score is defined, that pass the bound
//! (`threshold` on a similarity, `radius` on a distance) and whose id is in `ids`. A record id
//! is the hash of its source produce. Single mode binds the first record of `E` whose value the
//! pattern binds. List mode (`k` or `results = "all"`) binds ONE value: the list of the first
//! `min(k, |E|)` payload values, each `[score, value]` with `scores = true`.
//!
//! ```text
//! RECEIVE-VDB(K)
//!   loop
//!     single: r ← the first record of E(K) whose value the pattern of K binds
//!     list:   L ← the first min(k, |E(K)|) records of E(K); the pattern binds the list of L
//!     plain:  r ← the first record in canonical order whose payload the pattern binds
//!     none, or the guard of K refuses (one check, decision 37): park K with its query, stop
//!     fire K over [D] or [D; m]; the result of the first member is dispatched
//!     K peeks: the removed non-persistent members are owed second sends (Rule E)
//!     Done: stop. Continue: loop. Reissue(c): K re-issues
//!   TAIL
//!
//! OFFER-VDB(x)
//!   loop
//!     W ← the first waiter, in canonical order, that x fires:
//!         plain:  the pattern of W binds x and the guard admits
//!         single: x ∈ E(W), the pattern binds the value of x, the guard admits
//!         list:   x ∈ L(W, R + x), the pattern binds the list, the guard admits
//!     none: store x (CapacityExceeded on a full collection), stop
//!     fire W with x over [D]
//!     list and W does not peek: K_retire for each other non-persistent member of the list
//!     Done: stop. Continue: offer x to the next waiter (decision 30). Reissue(c): x re-issues
//!
//! TAIL                                      ▷ the DRAIN-AND-CASCADE of the join engine
//!   loop
//!     owed second sends: at the cap stop; else OFFER-VDB of the next one; continue
//!     no removal since the last search: done
//!     W ← the first parked list waiter, in canonical order, whose list L(W, R) is not empty,
//!         binds its pattern and passes its guard; none: done
//!     at the cap: stop
//!     lift the first member v of L when v is not persistent (K_lift), re-offer v to W, and
//!     retire the other non-persistent members when W does not peek; a lifted v that a peek
//!     waiter did not spend is an owed second send (decision 30)
//!   stop: one settle job carries the owed second sends and the re-issue of the operation
//! ```
//!
//! A single-mode waiter needs no cascade: a removal makes no other record eligible for it, and
//! it does not change the binding of a record. A waiter refused by its guard on a better record
//! can later fire with a worse one (decision 37, until P12).
//!
//! **Settle jobs.** A transaction stops at the round cap (I-21) or at a divergent pair of the
//! cascade (A10). It then re-issues one `SettleOp` whose cascade state holds the owed second
//! sends and the re-issue of the operation (arbitration A7). A settle with a cascade state always
//! runs in the join engine (`join::classify`), which offers the second sends first, then
//! cascades, then re-issues the operation. A second send keeps its record, so it keeps its vector
//! and its id.
//!
//! **Metering (decision 42).** Every decode, kernel, comparison, sort step, list construction
//! and copy is reserved on the meter of the job before it runs, through the numeric core and the
//! primitives of [`Cx`].

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, LazyLock};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{
    BindPattern, CostAuthority, EList, Expr, ListParWithRandom, Par, TaggedContinuation,
};
use models::rust::reified::validation::{
    K_KEY, METRIC_KEY, RADIUS_KEY, RESULTS_ALL, RESULTS_KEY, THRESHOLD_KEY,
};
use models::rust::utils::union;
use num_traits::{One, Signed};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::space_store_handle::canonical_data_order_with;
use rspace_plus_plus::rspace::trace::event::Produce;
use serde::Serialize;

use super::queue::QueueDiscipline;
use super::scan::{
    located, retire_continuation, stored_candidate, view_index, Member, RETIRE_CONTINUATION,
    RETIRE_PATTERN,
};
use crate::rust::interpreter::reified::costs::{FireStep, ReissueCause, ScanMeter, FIRE_ROUND_CAP};
use crate::rust::interpreter::spaces::agent::{
    CascadeState, ConsumeOp, PendingProduce, ProduceOp, QueryParams, SettleOp, SpaceOp,
    SpaceReissue,
};
use crate::rust::interpreter::spaces::errors::SpaceError;
use crate::rust::interpreter::spaces::factory::defaults::{
    CAPACITY_PARAM, DIMENSIONS_PARAM, MAX_CAPACITY, MAX_DIMENSIONS, METRIC_PARAM,
};
use crate::rust::interpreter::spaces::generic::{
    open_plan, peek_set, peek_set_all, push_reserved, Cx, Discipline, JoinWindow, OfferFire,
    OpView, Ordered, ParCandidate, ParDatum, ParWaiter, Side, SideBound,
};
use crate::rust::interpreter::spaces::namespace::{CollectionKey, RandomPurpose};
use crate::rust::interpreter::spaces::record::{self, sole_expr, Introduction, WaiterPolicy};
use crate::rust::interpreter::spaces::settle::{lift_pattern, settle_operation};
use crate::rust::interpreter::spaces::types::config::SpaceConfig;
use crate::rust::interpreter::tensor::exact::{self, Bound, Candidate, Metric, Query, Score};
use crate::rust::interpreter::tensor::num::{self, Exact, ExactVector, Number};

/// The send param that gives the vector of a record (P6, decision 56).
pub const EMBEDDING_PARAM: &str = "embedding";

/// The key of the query vector in the policy of a parked waiter.
pub const QUERY_VECTOR_KEY: &str = "q";

/// The query param that delivers the score of each record with its value.
pub const SCORES_KEY: &str = "scores";

/// The query param that keeps only the records whose ids it lists.
pub const IDS_KEY: &str = "ids";

/// The length of a record id: the hash of the source produce of the record.
pub const ID_LEN: usize = 32;

/// The keys of a query that need a query vector, in bytewise order.
const VECTOR_KEYS: [&str; 5] = [IDS_KEY, METRIC_KEY, RADIUS_KEY, SCORES_KEY, THRESHOLD_KEY];

/// The template of the lift pattern: one remainder that binds the whole record, as the settle
/// and the join engine bind it.
static LIFT_PATTERN: LazyLock<BindPattern> = LazyLock::new(lift_pattern);

/// The VectorDB discipline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct VectorDbDiscipline;

impl Discipline for VectorDbDiscipline {
    const NAME: &'static str = "vectordb";
    const USES_COUNTER: bool = false;
    const SEND_PARAMS: &'static [&'static str] = &[EMBEDDING_PARAM];
    const QUERY_PARAMS: &'static [&'static str] = &[
        IDS_KEY,
        K_KEY,
        METRIC_KEY,
        RADIUS_KEY,
        RESULTS_KEY,
        SCORES_KEY,
        THRESHOLD_KEY,
    ];
    const JOIN_WINDOW: JoinWindow = JoinWindow::Similarity;

    /// The `capacity` of the space bounds the records of each collection (design v3 4.3), with
    /// `CapacityExceeded`. Nothing bounds the waiters.
    fn bound(
        &self,
        config: &SpaceConfig,
        side: Side,
        meter: &ScanMeter,
    ) -> Result<Option<SideBound>, SpaceError> {
        match side {
            Side::Data => Ok(Some(SideBound::Records(
                vdb_config(config, meter)?.capacity,
            ))),
            Side::Waiters => Ok(None),
        }
    }

    /// The entry with the least `(ord, bincode bytes)`, as for the queue. Only the head driver
    /// asks for a head, and a VectorDB collection does not use the head driver.
    fn head_metered<T: Serialize>(
        &self,
        side: Side,
        entries: &[Ordered<'_, T>],
        meter: &ScanMeter,
    ) -> Result<Option<usize>, SpaceError> {
        QueueDiscipline.head_metered(side, entries, meter)
    }

    /// Every entry can meet an incoming element. Only the head driver asks.
    fn reaches_head(&self, _side: Side, _queued: usize) -> bool { true }

    /// The lesser order interacts first, as for the queue. Only the head driver asks.
    fn precedes(
        &self,
        side: Side,
        ord: i64,
        head: i64,
        meter: &ScanMeter,
    ) -> Result<bool, SpaceError> {
        QueueDiscipline.precedes(side, ord, head, meter)
    }

    /// Checks the `embedding` param of a send and the query of a receive before any staging:
    /// the vector of the space dimension, the ranges, the exclusive pairs and the metric rules
    /// (design v3 4.4).
    fn check_values(
        &self,
        config: &SpaceConfig,
        params: &BTreeMap<String, Par>,
        query: Option<&QueryParams>,
        meter: &ScanMeter,
    ) -> Result<(), SpaceError> {
        let cfg = vdb_config(config, meter)?;
        if let Some(embedding) = lookup(params, EMBEDDING_PARAM, meter)? {
            num::vector_of(embedding, Some(cfg.dimensions), meter)
                .map_err(|error| SpaceError::of_number(error, EMBEDDING_PARAM))?;
        }
        match query {
            Some(query) => query_of(&cfg, query, meter).map(|_| ()),
            None => Ok(()),
        }
    }

    /// OFFER-VDB of the datum of `op`, then the tail (see the module documentation).
    fn offer(
        &self,
        cx: &mut Cx<'_, '_>,
        op: ProduceOp,
        _ord: i64,
    ) -> Result<Option<SpaceReissue>, SpaceError> {
        offer_plan(self, cx, op)
    }

    /// RECEIVE-VDB of the continuation of `op`, then the tail (see the module documentation).
    fn receive(
        &self,
        cx: &mut Cx<'_, '_>,
        op: ConsumeOp,
        _ord: i64,
    ) -> Result<Option<SpaceReissue>, SpaceError> {
        receive_plan(self, cx, op)
    }

    /// A restore outside a plan of this driver: OFFER-VDB of the record, then the tail. The
    /// engine calls `Discipline::restore` only from the head driver, which a VectorDB collection
    /// does not use. The restore has no operation to carry its re-issue, so a restore that must
    /// stop fails with `Inconsistent` before the commit, and the dropped transaction applies
    /// nothing (as `scan::restore`). Its lifts take the random state and the authority of the
    /// record.
    fn restore(
        &self,
        cx: &mut Cx<'_, '_>,
        record: ListParWithRandom,
    ) -> Result<Option<SpaceReissue>, SpaceError> {
        let meter = cx.meter();
        let cfg = vdb_config(cx.config(), meter)?;
        let origin = Origin::of(&record.random_state, record.cost_authority.as_ref(), meter)?;
        let mut tail = Tail::new();
        tail.owe(record, meter)?;
        match run_tail(cx, &cfg, &mut tail, &origin)? {
            None => Ok(None),
            Some(_) => Err(SpaceError::Inconsistent(
                "a restore outside a receive cannot re-issue its record",
            )),
        }
    }
}

// ── The config ───────────────────────────────────────────────────────────────

/// The params of a VectorDB space (design v3 4.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VdbConfig {
    /// The most records of each collection.
    pub capacity: usize,
    /// The length of every vector of the space.
    pub dimensions: usize,
    /// The metric of a query without `metric`.
    pub metric: Metric,
}

/// The params of the VectorDB space with the config `config`, each read reserved first. The
/// factory stored canonical params, so a malformed param is `Inconsistent`.
pub fn vdb_config(config: &SpaceConfig, meter: &ScanMeter) -> Result<VdbConfig, SpaceError> {
    let capacity = config_count(config, CAPACITY_PARAM, MAX_CAPACITY, meter)?;
    let dimensions = config_count(config, DIMENSIONS_PARAM, MAX_DIMENSIONS, meter)?;
    let metric = match lookup(&config.params, METRIC_PARAM, meter)? {
        None => Metric::Cosine,
        Some(value) => metric_of(value).ok_or(SpaceError::Inconsistent(
            "the metric of a VectorDB config is not a metric",
        ))?,
    };
    Ok(VdbConfig {
        capacity,
        dimensions,
        metric,
    })
}

/// The value of `key` in `map`, with the lookup and the read reserved first.
fn lookup<'m>(
    map: &'m BTreeMap<String, Par>,
    key: &str,
    meter: &ScanMeter,
) -> Result<Option<&'m Par>, SpaceError> {
    if map.is_empty() {
        return Ok(None);
    }
    meter.ordered_lookup(map.len(), key.len())?;
    let value = map.get(key);
    if let Some(value) = value {
        meter.inspect(value)?;
    }
    Ok(value)
}

/// A count param of a config: a `GInt` in `[1, max]`.
fn config_count(
    config: &SpaceConfig,
    key: &str,
    max: i64,
    meter: &ScanMeter,
) -> Result<usize, SpaceError> {
    let malformed = SpaceError::Inconsistent("a VectorDB config lacks a valid count");
    match lookup(&config.params, key, meter)? {
        Some(value) => match (sole_expr(value), value.connective_used) {
            (Some(ExprInstance::GInt(count)), false) if (1..=max).contains(count) => {
                usize::try_from(*count).map_err(|_| malformed)
            }
            _ => Err(malformed),
        },
        None => Err(malformed),
    }
}

/// The metric that a `GString` names, or `None`.
fn metric_of(value: &Par) -> Option<Metric> {
    match (sole_expr(value), value.connective_used) {
        (Some(ExprInstance::GString(name)), false) => Metric::parse(name),
        _ => None,
    }
}

// ── The query ────────────────────────────────────────────────────────────────

/// The mode of a query: one record, or a list of `k` records (`None` for all of them).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// One record.
    Single,
    /// The list of the first `k` eligible records, or of all of them for `None`.
    List(Option<usize>),
}

/// A checked query of a receive (design v3 4.4).
pub struct VdbQuery {
    /// The query vector with its metric, or `None` for a read in canonical order.
    pub exact: Option<Query>,
    /// The `threshold` or the `radius`, or `None`.
    pub bound: Option<Bound>,
    /// Single mode or list mode.
    pub mode: Mode,
    /// True for `scores = true`.
    pub scores: bool,
    /// The ids of `ids`, sorted and without duplicates, or `None`.
    pub ids: Option<Vec<[u8; ID_LEN]>>,
}

/// The checked query of the receive query `query`: its payload holds the query vector, or
/// nothing.
pub fn query_of(
    cfg: &VdbConfig,
    query: &QueryParams,
    meter: &ScanMeter,
) -> Result<VdbQuery, SpaceError> {
    let vector = match query.payload.as_slice() {
        [] => None,
        [vector] => Some(vector),
        _ => return Err(SpaceError::PositionalParamsUnsupported),
    };
    parse_query(cfg, vector, &query.named, meter)
}

/// True when the receive query `query` is in list mode: it names `k` or `results`. A join bind
/// in list mode fails with `ListModeInJoin` (decision 66). The lookups are reserved first.
pub fn is_list_query(query: &QueryParams, meter: &ScanMeter) -> Result<bool, SpaceError> {
    Ok(lookup(&query.named, K_KEY, meter)?.is_some()
        || lookup(&query.named, RESULTS_KEY, meter)?.is_some())
}

/// Checks a query with the query vector `vector` and the named params `named`, in a fixed order
/// so that a query with several faults reports one code: the keys that need a vector, the
/// metric, the vector, the bound, the mode, `scores` and `ids`. `named` may hold the key `q` of
/// a policy map, which this function does not read.
fn parse_query(
    cfg: &VdbConfig,
    vector: Option<&Par>,
    named: &BTreeMap<String, Par>,
    meter: &ScanMeter,
) -> Result<VdbQuery, SpaceError> {
    if vector.is_none() {
        for key in VECTOR_KEYS {
            if lookup(named, key, meter)?.is_some() {
                return Err(SpaceError::QueryNeedsVector {
                    key: key.to_string(),
                });
            }
        }
    }
    let metric = match lookup(named, METRIC_KEY, meter)? {
        None => cfg.metric,
        Some(value) => metric_of(value).ok_or(SpaceError::InvalidParam {
            key: METRIC_KEY.to_string(),
        })?,
    };
    let exact = match vector {
        None => None,
        Some(par) => {
            let vector = num::vector_of(par, Some(cfg.dimensions), meter)
                .map_err(|error| SpaceError::of_number(error, QUERY_VECTOR_KEY))?;
            match Query::new(vector, metric, meter)
                .map_err(|error| SpaceError::of_number(error, QUERY_VECTOR_KEY))?
            {
                Some(query) => Some(query),
                None => return Err(SpaceError::ZeroQueryVector),
            }
        }
    };
    let bound = match (
        lookup(named, THRESHOLD_KEY, meter)?,
        lookup(named, RADIUS_KEY, meter)?,
    ) {
        (Some(_), Some(_)) => return Err(SpaceError::ConflictingParams),
        (Some(value), None) => {
            let threshold = exact_number(value, THRESHOLD_KEY, meter)?;
            if threshold.is_negative() || threshold > Exact::one() {
                return Err(SpaceError::InvalidParam {
                    key: THRESHOLD_KEY.to_string(),
                });
            }
            if !metric.is_similarity() {
                return Err(SpaceError::ThresholdNeedsSimilarity);
            }
            Some(Bound::Threshold(threshold))
        }
        (None, Some(value)) => {
            let radius = exact_number(value, RADIUS_KEY, meter)?;
            if radius.is_negative() {
                return Err(SpaceError::InvalidParam {
                    key: RADIUS_KEY.to_string(),
                });
            }
            if metric.is_similarity() {
                return Err(SpaceError::RadiusNeedsDistance);
            }
            Some(Bound::Radius(radius))
        }
        (None, None) => None,
    };
    let mode = match (
        lookup(named, K_KEY, meter)?,
        lookup(named, RESULTS_KEY, meter)?,
    ) {
        (Some(_), Some(_)) => {
            return Err(SpaceError::InvalidParam {
                key: RESULTS_KEY.to_string(),
            })
        }
        (Some(value), None) => Mode::List(Some(k_of(cfg, value)?)),
        (None, Some(value)) => match (sole_expr(value), value.connective_used) {
            (Some(ExprInstance::GString(all)), false) if all == RESULTS_ALL => Mode::List(None),
            _ => {
                return Err(SpaceError::InvalidParam {
                    key: RESULTS_KEY.to_string(),
                })
            }
        },
        (None, None) => Mode::Single,
    };
    let scores = match lookup(named, SCORES_KEY, meter)? {
        None => false,
        Some(value) => match (sole_expr(value), value.connective_used) {
            (Some(ExprInstance::GBool(flag)), false) => *flag,
            _ => {
                return Err(SpaceError::InvalidParam {
                    key: SCORES_KEY.to_string(),
                })
            }
        },
    };
    let ids = match lookup(named, IDS_KEY, meter)? {
        None => None,
        Some(value) => Some(ids_of(value, meter)?),
    };
    Ok(VdbQuery {
        exact,
        bound,
        mode,
        scores,
        ids,
    })
}

/// The exact value of the number `value` of the param `key`. A value that is not a finite
/// number is an invalid value of its param.
fn exact_number(value: &Par, key: &str, meter: &ScanMeter) -> Result<Exact, SpaceError> {
    let invalid = |error| match error {
        num::NumError::HostWork(error) => SpaceError::Host(error),
        _ => SpaceError::InvalidParam {
            key: key.to_string(),
        },
    };
    match num::number_of(value, meter).map_err(invalid)? {
        Number::Dyadic(form) => form.value(meter).map_err(invalid),
        Number::Rational(value) => Ok(value),
    }
}

/// The `k` of a list-mode query: a `GInt` in `[1, capacity]`.
fn k_of(cfg: &VdbConfig, value: &Par) -> Result<usize, SpaceError> {
    let invalid = || SpaceError::InvalidParam {
        key: K_KEY.to_string(),
    };
    match (sole_expr(value), value.connective_used) {
        (Some(ExprInstance::GInt(k)), false) if *k >= 1 => match usize::try_from(*k) {
            Ok(k) if k <= cfg.capacity => Ok(k),
            Ok(_) | Err(_) => Err(invalid()),
        },
        _ => Err(invalid()),
    }
}

/// The ids of an `ids` param: a list of 32-byte byte arrays, sorted and without duplicates. Each
/// lookup and growth of the set is reserved first.
fn ids_of(value: &Par, meter: &ScanMeter) -> Result<Vec<[u8; ID_LEN]>, SpaceError> {
    let invalid = || SpaceError::InvalidParam {
        key: IDS_KEY.to_string(),
    };
    let elements = match (sole_expr(value), value.connective_used) {
        (
            Some(ExprInstance::EListBody(EList {
                ps,
                remainder: None,
                ..
            })),
            false,
        ) => ps,
        _ => return Err(invalid()),
    };
    let mut ids: BTreeSet<[u8; ID_LEN]> = BTreeSet::new();
    for element in elements {
        let id: [u8; ID_LEN] = match (sole_expr(element), element.connective_used) {
            (Some(ExprInstance::GByteArray(bytes)), false) => {
                bytes.as_slice().try_into().map_err(|_| invalid())?
            }
            _ => return Err(invalid()),
        };
        meter.ordered_lookup(ids.len(), ID_LEN)?;
        if !ids.contains(&id) {
            meter.tree_growth::<[u8; ID_LEN], ()>(ids.len(), 1)?;
            ids.insert(id);
        }
    }
    let mut sorted = meter.buffer::<[u8; ID_LEN]>(ids.len())?;
    sorted.extend(ids);
    Ok(sorted)
}

/// True when `ids` holds the id `hash`. The search is reserved first.
fn holds_id(
    ids: &[[u8; ID_LEN]],
    hash: &Blake2b256Hash,
    meter: &ScanMeter,
) -> Result<bool, SpaceError> {
    meter.ordered_lookup(ids.len(), ID_LEN)?;
    Ok(ids
        .binary_search_by(|id| id.as_slice().cmp(hash.0.as_slice()))
        .is_ok())
}

/// The policy that a parked waiter with the query `query` keeps: the map of its named params,
/// with the query vector under `q`, or `None` without a query. Each copy is reserved first.
pub fn policy_of_query(
    query: Option<&QueryParams>,
    meter: &ScanMeter,
) -> Result<WaiterPolicy, SpaceError> {
    let Some(query) = query else {
        return Ok(WaiterPolicy::None);
    };
    let mut map = meter.copy(&query.named)?;
    if let Some(vector) = query.payload.first() {
        meter.tree_growth::<String, Par>(map.len(), 1)?;
        meter.reserve_copy_slice(QUERY_VECTOR_KEY.as_bytes())?;
        map.insert(QUERY_VECTOR_KEY.to_string(), meter.copy(vector)?);
    }
    match map.is_empty() {
        true => Ok(WaiterPolicy::None),
        false => Ok(WaiterPolicy::Query(Box::new(map))),
    }
}

/// The query of the policy `policy` of a parked waiter or of a join slot, or `None` for a waiter
/// without a query.
pub fn policy_query(
    cfg: &VdbConfig,
    policy: &WaiterPolicy,
    meter: &ScanMeter,
) -> Result<Option<VdbQuery>, SpaceError> {
    match policy {
        WaiterPolicy::None => Ok(None),
        WaiterPolicy::Query(map) => {
            let vector = lookup(map, QUERY_VECTOR_KEY, meter)?;
            parse_query(cfg, vector, map, meter).map(Some)
        }
        WaiterPolicy::Level(_) => Err(SpaceError::Inconsistent(
            "a waiter of a VectorDB collection has a priority level",
        )),
    }
}

// ── Records ──────────────────────────────────────────────────────────────────

/// The record of a send with the params `params` and the data `data`, and its vector (design v3
/// 4.3): the vector is the `embedding` param, or else the payload itself when it is exactly one
/// non-empty list of numbers, or none. Each check and the wrap are reserved first.
pub fn vdb_record(
    cfg: &VdbConfig,
    params: &BTreeMap<String, Par>,
    data: &ListParWithRandom,
    meter: &ScanMeter,
) -> Result<(ListParWithRandom, Option<ExactVector>), SpaceError> {
    let embedding = match lookup(params, EMBEDDING_PARAM, meter)? {
        Some(embedding) => Some(embedding),
        None => match data.pars.as_slice() {
            [single] if is_numeric_list(single, meter)? => Some(single),
            _ => None,
        },
    };
    let vector = match embedding {
        Some(par) => Some(
            num::vector_of(par, Some(cfg.dimensions), meter)
                .map_err(|error| SpaceError::of_number(error, EMBEDDING_PARAM))?,
        ),
        None => None,
    };
    let record = record::wrap_vdb_datum_metered(0, embedding, data, meter)?;
    Ok((record, vector))
}

/// True when `par` is one non-empty list of numbers. The test of each element is reserved
/// first.
fn is_numeric_list(par: &Par, meter: &ScanMeter) -> Result<bool, SpaceError> {
    match (sole_expr(par), par.connective_used) {
        (
            Some(ExprInstance::EListBody(EList {
                ps,
                remainder: None,
                ..
            })),
            false,
        ) => {
            meter.charge(ps.len(), 0, 0)?;
            Ok(!ps.is_empty() && ps.iter().all(num::is_number))
        }
        _ => Ok(false),
    }
}

/// The vector of a stored record, or `None` for a record without one.
pub fn record_vector(
    record: &ListParWithRandom,
    cfg: &VdbConfig,
    meter: &ScanMeter,
) -> Result<Option<ExactVector>, SpaceError> {
    match record::vdb_datum_slots_metered(record, meter)?.1 {
        Some(embedding) => num::vector_of(embedding, Some(cfg.dimensions), meter)
            .map(Some)
            .map_err(|error| SpaceError::of_number(error, EMBEDDING_PARAM)),
        None => Ok(None),
    }
}

/// The payload of a stored record: the datum as the program sent it.
pub fn payload_of(
    record: &ListParWithRandom,
    meter: &ScanMeter,
) -> Result<ListParWithRandom, SpaceError> {
    Ok(record::unwrap_vdb_datum_metered(record, meter)?.datum)
}

// ── Delivered values ─────────────────────────────────────────────────────────

/// The value of a payload: the datum of a send of one datum, or the list of the data of a send
/// of several. Each copy is reserved first.
fn payload_value(payload: &ListParWithRandom, meter: &ScanMeter) -> Result<Par, SpaceError> {
    match payload.pars.as_slice() {
        [single] => Ok(meter.copy(single)?),
        many => list_of(meter.copy_slice(many)?, meter),
    }
}

/// The list of `values` with the bit sets of the normalizer: the union of the `locally_free`
/// sets and the disjunction of the `connective_used` flags of the elements, as the records of a
/// space build their lists. The caller reserved `values`. Each read and each result of the
/// union, the copies of the bit set and the expression list are reserved first (decision 42).
fn list_of(values: Vec<Par>, meter: &ScanMeter) -> Result<Par, SpaceError> {
    let mut locally_free: Vec<u8> = Vec::new();
    let mut connective_used = false;
    for value in &values {
        if !value.locally_free.is_empty() {
            meter.compare(locally_free.len(), value.locally_free.len())?;
            meter.reserve_buffer::<u8>(locally_free.len().max(value.locally_free.len()))?;
            locally_free = union(locally_free, meter.copy(&value.locally_free)?);
        }
        connective_used = connective_used || value.connective_used;
    }
    let mut exprs = meter.buffer::<Expr>(1)?;
    exprs.push(Expr {
        expr_instance: Some(ExprInstance::EListBody(EList {
            ps: values,
            locally_free: meter.copy(&locally_free)?,
            connective_used,
            remainder: None,
        })),
    });
    Ok(Par {
        exprs,
        locally_free,
        connective_used,
        ..Par::default()
    })
}

/// `[score, value]` with the delivered score `score` (RN_53 of the exact score). The lists and
/// the expression list of the score are reserved first.
fn scored(score: f64, value: Par, meter: &ScanMeter) -> Result<Par, SpaceError> {
    let mut exprs = meter.buffer::<Expr>(1)?;
    exprs.push(Expr {
        expr_instance: Some(ExprInstance::GDouble(score.to_bits())),
    });
    let mut pair = meter.buffer::<Par>(2)?;
    pair.push(Par {
        exprs,
        ..Par::default()
    });
    pair.push(value);
    list_of(pair, meter)
}

/// The data of one value `value`: a list of one `Par`, reserved first.
fn one_value(value: Par, meter: &ScanMeter) -> Result<Vec<Par>, SpaceError> {
    let mut pars = meter.buffer::<Par>(1)?;
    pars.push(value);
    Ok(pars)
}

/// The delivered score of `score` for the query `query`.
fn delivered_score(score: &Score, query: &Query, meter: &ScanMeter) -> Result<f64, SpaceError> {
    exact::delivered(score, query, meter).map_err(|error| SpaceError::of_number(error, SCORES_KEY))
}

/// The data that a single-mode bind sees for a record with the payload `payload` and the score
/// `score`: the payload itself, or `[score, value]` with `scores = true` (design v3 4.4).
pub fn single_data(
    query: &VdbQuery,
    payload: &ListParWithRandom,
    score: Option<&Score>,
    meter: &ScanMeter,
) -> Result<ListParWithRandom, SpaceError> {
    match (query.scores, &query.exact, score) {
        (true, Some(exact), Some(score)) => {
            let value = payload_value(payload, meter)?;
            let delivered = delivered_score(score, exact, meter)?;
            Ok(ListParWithRandom {
                pars: one_value(scored(delivered, value, meter)?, meter)?,
                random_state: meter.copy(&payload.random_state)?,
                cost_authority: payload
                    .cost_authority
                    .as_ref()
                    .map(|authority| meter.copy(authority))
                    .transpose()?,
                cost_stack: payload
                    .cost_stack
                    .as_ref()
                    .map(|stack| meter.copy(stack))
                    .transpose()?,
            })
        }
        _ => Ok(meter.copy(payload)?),
    }
}

/// A member of a list: its payload and its score.
pub struct ListMember<'p> {
    /// The payload of the member.
    pub payload: &'p ListParWithRandom,
    /// The score of the member, or `None` for a query without a vector.
    pub score: Option<&'p Score>,
}

/// The data that a list-mode bind sees: ONE value, the list of the values of `members` in
/// delivery order, each `[score, value]` with `scores = true`. Its random state merges the random
/// states of the members in delivery order, and one member keeps its own (arbitration A11). The
/// list owns no cost authority and no cost stack.
pub fn list_data(
    query: &VdbQuery,
    members: &[ListMember<'_>],
    meter: &ScanMeter,
) -> Result<ListParWithRandom, SpaceError> {
    let random_state = match members {
        [] => return Err(SpaceError::Inconsistent("a list has no member")),
        [single] => meter.copy(&single.payload.random_state)?,
        _ => {
            let mut randoms = meter.buffer::<Blake2b512Random>(members.len())?;
            for member in members {
                let random = Blake2b512Random::from_bytes(&member.payload.random_state);
                meter.random_bytes(&random)?;
                randoms.push(random);
            }
            let merged = Blake2b512Random::merge(randoms);
            meter.random_bytes(&merged)?;
            merged.to_bytes()
        }
    };
    let mut values = meter.buffer::<Par>(members.len())?;
    for member in members {
        let value = payload_value(member.payload, meter)?;
        values.push(match (query.scores, &query.exact, member.score) {
            (true, Some(exact), Some(score)) => {
                scored(delivered_score(score, exact, meter)?, value, meter)?
            }
            _ => value,
        });
    }
    Ok(ListParWithRandom {
        pars: one_value(list_of(values, meter)?, meter)?,
        random_state,
        cost_authority: None,
        cost_stack: None,
    })
}

// ── Selection ────────────────────────────────────────────────────────────────

/// An element in flight as a selection sees it: its record, its payload, the identity of its
/// produce, its persistence and its vector.
pub struct Flight<'a> {
    /// The record: the envelope with its vector, then the payload.
    pub record: &'a ListParWithRandom,
    /// The payload of the record.
    pub payload: &'a ListParWithRandom,
    /// The identity of the produce of the record.
    pub source: &'a Produce,
    /// True for a persistent element.
    pub persist: bool,
    /// The vector of the record, or `None`.
    pub vector: Option<&'a ExactVector>,
}

/// An eligible record: its index in the staged view of its channel, or `None` for the element
/// in flight, and its score (`None` for a query without a vector).
pub struct Eligible {
    /// The index in the staged view, or `None` for the element in flight.
    pub index: Option<usize>,
    /// The score, or `None` for a query without a vector.
    pub score: Option<Score>,
}

/// The eligible records of `query` on `channel` in selection order (design v3 4.4): the stored
/// records, and the element `flight` when it is given. A query with a vector orders by the better
/// score, then by the canonical order of DR-75. A query without one keeps the canonical order.
pub fn eligible(
    cx: &mut Cx<'_, '_>,
    cfg: &VdbConfig,
    query: &VdbQuery,
    channel: &Par,
    flight: Option<&Flight<'_>>,
) -> Result<Vec<Eligible>, SpaceError> {
    let meter = cx.meter();
    let stored = cx.data_on(channel)?.len();
    let order = match flight {
        None => cx.canonical_data_order(channel)?,
        Some(flight) => {
            let extra = Datum {
                a: meter.copy(flight.record)?,
                persist: flight.persist,
                source: meter.copy(flight.source)?,
            };
            let records = cx.data_on(channel)?;
            canonical_data_order_with(records, &extra, meter)?
        }
    };
    let mut entries = meter.buffer::<Eligible>(order.len())?;
    for &index in &order {
        let in_flight = match (flight, index == stored) {
            (Some(flight), true) => Some(flight),
            _ => None,
        };
        if let Some(ids) = &query.ids {
            let held = match in_flight {
                Some(flight) => holds_id(ids, &flight.source.hash, meter)?,
                None => holds_id(ids, &stored_at(cx, channel, index)?.source.hash, meter)?,
            };
            if !held {
                continue;
            }
        }
        let score = match &query.exact {
            None => None,
            Some(exact) => {
                let score = match in_flight {
                    Some(flight) => match flight.vector {
                        Some(vector) => score_of(exact, vector, query, meter)?,
                        None => None,
                    },
                    None => {
                        let record = &stored_at(cx, channel, index)?.a;
                        match record_vector(record, cfg, meter)? {
                            Some(vector) => score_of(exact, &vector, query, meter)?,
                            None => None,
                        }
                    }
                };
                let Some(score) = score else {
                    continue;
                };
                Some(score)
            }
        };
        push_reserved(
            &mut entries,
            Eligible {
                index: in_flight.is_none().then_some(index),
                score,
            },
            meter,
        )?;
    }
    match query.exact {
        None => Ok(entries),
        Some(_) => ranked(entries, meter),
    }
}

/// The score of `vector` for the query `exact` when it is defined and passes the bound of
/// `query`, or `None`.
fn score_of(
    exact: &Query,
    vector: &ExactVector,
    query: &VdbQuery,
    meter: &ScanMeter,
) -> Result<Option<Score>, SpaceError> {
    let Some(score) = exact::score(exact, vector, meter)
        .map_err(|error| SpaceError::of_number(error, QUERY_VECTOR_KEY))?
    else {
        return Ok(None);
    };
    match &query.bound {
        None => Ok(Some(score)),
        Some(bound) => {
            let within = exact::within(&score, bound, exact, meter)
                .map_err(|error| SpaceError::of_number(error, QUERY_VECTOR_KEY))?
                .ok_or(SpaceError::Inconsistent(
                    "a bound does not fit the metric of its query",
                ))?;
            Ok(within.then_some(score))
        }
    }
}

/// The eligibility of the element `flight` for the single-mode query `query`: `None` when it is
/// not eligible, and otherwise its score (`None` for a query without a vector).
pub fn flight_eligibility(
    query: &VdbQuery,
    flight: &Flight<'_>,
    meter: &ScanMeter,
) -> Result<Option<Option<Score>>, SpaceError> {
    if let Some(ids) = &query.ids {
        if !holds_id(ids, &flight.source.hash, meter)? {
            return Ok(None);
        }
    }
    match &query.exact {
        None => Ok(Some(None)),
        Some(exact) => match flight.vector {
            None => Ok(None),
            Some(vector) => Ok(score_of(exact, vector, query, meter)?.map(Some)),
        },
    }
}

/// `entries` in the selection order of their scores, ties in their current order, which is the
/// canonical order (`tensor::exact::rank`).
fn ranked(entries: Vec<Eligible>, meter: &ScanMeter) -> Result<Vec<Eligible>, SpaceError> {
    let mut indices = meter.buffer::<Option<usize>>(entries.len())?;
    let mut candidates = meter.buffer::<Candidate>(entries.len())?;
    for (position, entry) in entries.into_iter().enumerate() {
        let score = entry.score.ok_or(SpaceError::Inconsistent(
            "a record of a query with a vector has no score",
        ))?;
        indices.push(entry.index);
        candidates.push(Candidate {
            score,
            rank: position,
        });
    }
    exact::rank(&mut candidates, meter)
        .map_err(|error| SpaceError::of_number(error, QUERY_VECTOR_KEY))?;
    let mut sorted = meter.buffer::<Eligible>(candidates.len())?;
    for candidate in candidates {
        let index = *indices.get(candidate.rank).ok_or(SpaceError::Inconsistent(
            "a ranked candidate lost its record",
        ))?;
        sorted.push(Eligible {
            index,
            score: Some(candidate.score),
        });
    }
    Ok(sorted)
}

/// The record at `index` of the staged view of `channel`.
fn stored_at<'v>(
    cx: &'v mut Cx<'_, '_>,
    channel: &Par,
    index: usize,
) -> Result<&'v ParDatum, SpaceError> {
    cx.data_on(channel)?
        .get(index)
        .ok_or(SpaceError::Inconsistent(
            "a canonical index is outside the staged view",
        ))
}

/// The one pattern of a waiter of a collection.
pub fn waiter_pattern(waiter: &ParWaiter) -> Result<&BindPattern, SpaceError> {
    match waiter.patterns.as_slice() {
        [pattern] => Ok(pattern),
        _ => Err(SpaceError::Inconsistent(
            "a waiting continuation of a collection has several binds",
        )),
    }
}

/// A list that a list-mode bind binds: the values that the pattern binds, and the members of
/// the list in delivery order, each with its index in the staged view, or `None` for the element
/// in flight.
pub struct ListFire {
    /// The values that the pattern binds.
    pub matched: ListParWithRandom,
    /// The members of the list in delivery order: the stored record and its index, or `None`
    /// for the element in flight.
    pub members: Vec<Option<(ParDatum, usize)>>,
}

/// LIST of the list-mode query `query` with the length `k` on `channel`: the first `min(k, |E|)`
/// eligible records, with the element `flight` among them when it is given, when the list is not
/// empty and `pattern` binds it. Only the binding is checked, not the guard.
pub fn list_selection(
    cx: &mut Cx<'_, '_>,
    cfg: &VdbConfig,
    query: &VdbQuery,
    k: Option<usize>,
    pattern: &BindPattern,
    channel: &Par,
    flight: Option<&Flight<'_>>,
) -> Result<Option<ListFire>, SpaceError> {
    let meter = cx.meter();
    let entries = eligible(cx, cfg, query, channel, flight)?;
    let length = k.map_or(entries.len(), |k| k.min(entries.len()));
    if length == 0 {
        return Ok(None);
    }
    let list = entries.get(..length).ok_or(SpaceError::Inconsistent(
        "a list is longer than its eligible records",
    ))?;
    let mut members = meter.buffer::<Option<(ParDatum, usize)>>(length)?;
    let mut payloads = meter.buffer::<ListParWithRandom>(length)?;
    for entry in list {
        match entry.index {
            Some(index) => {
                let record = meter.copy(stored_at(cx, channel, index)?)?;
                payloads.push(payload_of(&record.a, meter)?);
                members.push(Some((record, index)));
            }
            None => {
                let flight = flight.ok_or(SpaceError::Inconsistent(
                    "a list holds an element in flight, but none is in flight",
                ))?;
                payloads.push(meter.copy(flight.payload)?);
                members.push(None);
            }
        }
    }
    let mut views = meter.buffer::<ListMember<'_>>(length)?;
    views.extend(
        list.iter()
            .zip(&payloads)
            .map(|(entry, payload)| ListMember {
                payload,
                score: entry.score.as_ref(),
            }),
    );
    let data = list_data(query, &views, meter)?;
    Ok(cx
        .bind(pattern, &data)?
        .map(|matched| ListFire { matched, members }))
}

/// OFFER-LIST: the list-mode waiter with the query `query`, the length `k` and the pattern
/// `pattern` against the element `flight` on `channel`. When the element is in the list
/// `L(W, R + x)` and the pattern binds the list, the values that it binds and the other stored
/// members of the list in delivery order. Only the binding is checked, not the guard.
pub fn list_offer(
    cx: &mut Cx<'_, '_>,
    cfg: &VdbConfig,
    query: &VdbQuery,
    k: Option<usize>,
    pattern: &BindPattern,
    channel: &Par,
    flight: &Flight<'_>,
) -> Result<Option<(ListParWithRandom, Vec<ParDatum>)>, SpaceError> {
    let meter = cx.meter();
    let Some(fire) = list_selection(cx, cfg, query, k, pattern, channel, Some(flight))? else {
        return Ok(None);
    };
    if !fire.members.iter().any(Option::is_none) {
        return Ok(None);
    }
    let mut others = meter.buffer::<ParDatum>(fire.members.len())?;
    others.extend(fire.members.into_iter().flatten().map(|(record, _)| record));
    Ok(Some((fire.matched, others)))
}

// ── Retires and lifts ────────────────────────────────────────────────────────

/// Retires the stored members `members` of `channel` that are not persistent: one consume of
/// `K_retire(m)` over `[channel]` with the record `m` for each, in delivery order (the set
/// precedent, F3). The retires derive their random state from the element `element` that fired
/// and carry its cost authority (as `scan::retiring_of`). A COMM never removes a persistent
/// record, so a persistent member stays. Returns true when a record was retired.
pub fn retire_members(
    cx: &mut Cx<'_, '_>,
    channel: &Par,
    members: Vec<ParDatum>,
    element: &ListParWithRandom,
) -> Result<bool, SpaceError> {
    let meter = cx.meter();
    let random = RandomPurpose::Retire.derive(&Blake2b512Random::from_bytes(&element.random_state));
    let mut retired = false;
    for member in members {
        if member.persist {
            continue;
        }
        let index = located(cx, channel, &member.source.hash)?;
        let payload = payload_of(&member.a, meter)?;
        let matched = cx
            .bind(&RETIRE_PATTERN, &payload)?
            .ok_or(SpaceError::Inconsistent(
                "a retired member does not bind the retire pattern",
            ))?;
        meter.reserve_copy(&*RETIRE_CONTINUATION)?;
        meter.reserve_buffer::<u8>(member.source.hash.0.len())?;
        meter.random_bytes(&random)?;
        let authority = element
            .cost_authority
            .as_ref()
            .map(|authority| meter.copy(authority))
            .transpose()?;
        let continuation = retire_continuation(&member.source.hash, &random, authority);
        let mut channels = meter.buffer::<Par>(1)?;
        channels.push(meter.copy(channel)?);
        let mut patterns = meter.buffer::<BindPattern>(1)?;
        patterns.push(meter.copy(&*RETIRE_PATTERN)?);
        let mut candidates = meter.buffer::<ParCandidate>(1)?;
        candidates.push(stored_candidate(meter.copy(channel)?, Member {
            record: member,
            matched,
            view_index: view_index(index)?,
        }));
        cx.retire(channels, patterns, continuation, candidates)?;
        retired = true;
    }
    Ok(retired)
}

/// Stages the lift of the stored record `lead` of `channel` (P5b, F2): a consume of `K_lift`
/// over `[channel]` that removes it and is never dispatched. A VectorDB collection has no
/// counter, so `K_lift` has the shape of `K_retire` with the source hash of the record, as the
/// join engine lifts a set record. The random state is `random` (the lift random state of the
/// operation) and the cost authority is `authority` (the authority of the operation).
pub fn lift_record(
    cx: &mut Cx<'_, '_>,
    channel: &Par,
    lead: &ParDatum,
    random: &Blake2b512Random,
    authority: Option<&CostAuthority>,
) -> Result<(), SpaceError> {
    let meter = cx.meter();
    let index = located(cx, channel, &lead.source.hash)?;
    let matched = cx
        .bind(&LIFT_PATTERN, &lead.a)?
        .ok_or(SpaceError::Inconsistent(
            "a stored record does not match the lift pattern",
        ))?;
    meter.reserve_copy(&*RETIRE_CONTINUATION)?;
    meter.reserve_buffer::<u8>(lead.source.hash.0.len())?;
    meter.random_bytes(random)?;
    let authority = authority
        .map(|authority| meter.copy(authority))
        .transpose()?;
    let continuation = retire_continuation(&lead.source.hash, random, authority);
    let mut channels = meter.buffer::<Par>(1)?;
    channels.push(meter.copy(channel)?);
    let mut patterns = meter.buffer::<BindPattern>(1)?;
    patterns.push(meter.copy(&*LIFT_PATTERN)?);
    let mut candidates = meter.buffer::<ParCandidate>(1)?;
    candidates.push(stored_candidate(meter.copy(channel)?, Member {
        record: meter.copy(lead)?,
        matched,
        view_index: view_index(index)?,
    }));
    cx.lift(channels, patterns, continuation, candidates)
}

// ── The driver: the element in flight ────────────────────────────────────────

/// The element in flight of an offer: the datum of a produce, a second send of Rule E, or a
/// lifted record of the cascade.
struct Element {
    /// The record: the envelope with its vector, then the payload.
    record: ListParWithRandom,
    /// The identity of the produce of the record.
    source: Produce,
    /// True for a persistent datum.
    persist: bool,
    /// The introduction that the staged operations of the element register.
    introduction: Arc<Introduction>,
    /// The payload of the record.
    payload: ListParWithRandom,
    /// The vector of the record, or `None`.
    vector: Option<ExactVector>,
}

impl Element {
    /// The element as a selection sees it.
    fn flight(&self) -> Flight<'_> {
        Flight {
            record: &self.record,
            payload: &self.payload,
            source: &self.source,
            persist: self.persist,
            vector: self.vector.as_ref(),
        }
    }

    /// The second send of Rule E of the record `record` (decision 45): not persistent, with the
    /// identity and the authority of the record (`Introduction::for_record`), and its vector.
    fn second_send(
        cx: &mut Cx<'_, '_>,
        cfg: &VdbConfig,
        record: ListParWithRandom,
    ) -> Result<Element, SpaceError> {
        let meter = cx.meter();
        let payload = payload_of(&record, meter)?;
        let vector = record_vector(&record, cfg, meter)?;
        let source = Produce::create_metered(cx.data_channel(), &record, false, meter)?;
        let introduction = cx.introduction().for_record(&record, meter)?;
        Ok(Element {
            record,
            source,
            persist: false,
            introduction,
            payload,
            vector,
        })
    }
}

/// The end of an offer.
enum OfferEnd {
    /// The element is stored, or a fire spent it.
    Settled,
    /// The element is still in flight, and it re-issues with this cause.
    Reissue(ReissueCause),
}

/// The lift random state and the cost authority of the operation of a plan: its lifts derive
/// their random state from the random state of the operation and carry its authority.
struct Origin {
    lift: Blake2b512Random,
    authority: Option<CostAuthority>,
}

impl Origin {
    /// The origin with the random state `random_state` and the cost authority `authority`.
    fn of(
        random_state: &[u8],
        authority: Option<&CostAuthority>,
        meter: &ScanMeter,
    ) -> Result<Self, SpaceError> {
        Ok(Self {
            lift: RandomPurpose::Lift.derive(&Blake2b512Random::from_bytes(random_state)),
            authority: authority
                .map(|authority| meter.copy(authority))
                .transpose()?,
        })
    }
}

/// The work of the tail of a plan: the owed second sends (a stack: the last pushed runs first),
/// and whether a record left the collection since the last cascade search.
struct Tail {
    work: Vec<ListParWithRandom>,
    removed: bool,
}

impl Tail {
    fn new() -> Self {
        Self {
            work: Vec::new(),
            removed: false,
        }
    }

    /// Owes `record` as the next second send.
    fn owe(&mut self, record: ListParWithRandom, meter: &ScanMeter) -> Result<(), SpaceError> {
        push_reserved(&mut self.work, record, meter)
    }

    /// Owes `records` as second sends, the first of `records` first.
    fn owe_all(
        &mut self,
        records: Vec<ListParWithRandom>,
        meter: &ScanMeter,
    ) -> Result<(), SpaceError> {
        for record in records.into_iter().rev() {
            self.owe(record, meter)?;
        }
        Ok(())
    }

    /// The stop of a transaction with the cause `cause`: it owes the second sends of the tail, in
    /// the order in which they run.
    fn stop(&mut self, cause: ReissueCause) -> Stop {
        Stop {
            pending: std::mem::take(&mut self.work).into_iter().rev().collect(),
            cause,
        }
    }
}

/// A stopped transaction: the owed second sends in the order in which they run, and the cause.
struct Stop {
    pending: Vec<ListParWithRandom>,
    cause: ReissueCause,
}

// ── The driver: OFFER-VDB ────────────────────────────────────────────────────

/// The produce plan: OFFER-VDB of the datum of `op`, then the tail. A re-issue of the datum
/// carries no order (decision 43): the record has the `ord` 0, so it keeps its identity.
fn offer_plan(
    discipline: &VectorDbDiscipline,
    cx: &mut Cx<'_, '_>,
    op: ProduceOp,
) -> Result<Option<SpaceReissue>, SpaceError> {
    if let Some(reissue) = open_plan(discipline, cx, OpView::Produce(&op))? {
        return Ok(Some(reissue));
    }
    let meter = cx.meter();
    let cfg = vdb_config(cx.config(), meter)?;
    let (record, vector) = vdb_record(&cfg, &op.params, &op.data, meter)?;
    let source = Produce::create_metered(cx.data_channel(), &record, op.persistent, meter)?;
    meter.shared(1)?;
    let element = Element {
        record,
        source,
        persist: op.persistent,
        introduction: Arc::clone(cx.introduction()),
        payload: meter.copy(&op.data)?,
        vector,
    };
    let origin = Origin::of(
        &op.data.random_state,
        op.data.cost_authority.as_ref(),
        meter,
    )?;
    let mut tail = Tail::new();
    let own = match offer_vdb(cx, &cfg, element, &mut tail)? {
        OfferEnd::Settled => None,
        OfferEnd::Reissue(cause) => Some(cause),
    };
    let stop = finish_tail(cx, &cfg, &mut tail, &origin, own)?;
    let settle = match &stop {
        Some(_) => Some(settle_operation(OpView::Produce(&op), meter)?),
        None => None,
    };
    let resume = own.map(|cause| {
        (
            SpaceOp::Produce(ProduceOp {
                reissued_ord: None,
                ..op
            }),
            cause,
        )
    });
    conclude(cx, settle, stop, resume)
}

/// OFFER-VDB of `element` on the fire count of its transaction (I-21). It returns whether the
/// element settled or must re-issue. A retire records its removal in `tail`.
fn offer_vdb(
    cx: &mut Cx<'_, '_>,
    cfg: &VdbConfig,
    element: Element,
    tail: &mut Tail,
) -> Result<OfferEnd, SpaceError> {
    let meter = cx.meter();
    let data = cx.data_channel();
    let mut tested = 0;
    loop {
        let Some(target) = first_offer_target(cx, cfg, &element, tested)? else {
            cx.check_capacity::<VectorDbDiscipline>(Side::Data)?;
            cx.store_on(data, element.record, element.persist, &element.introduction)?;
            return Ok(OfferEnd::Settled);
        };
        let OfferTarget {
            position,
            waiter,
            matched,
            others,
        } = target;
        let peeks = !waiter.peeks.is_empty();
        let spent = !element.persist && !peeks;
        let stays = waiter.persist;
        record::prepay_continuation_unwrap(&waiter.continuation, meter)?;
        let fire = OfferFire {
            persistent: element.persist,
            waiter,
            matched,
        };
        let step = cx.rounds().after_fire(spent, stays);
        let staged = meter.copy(&element.record)?;
        let removed = meter.copy(&element.record)?;
        let source = meter.copy(&element.source)?;
        cx.fire_offer_on(data, fire, staged, removed, source, &element.introduction)?;
        // A peek waiter takes no datum, so its fire retires nothing (decision 37).
        let retired = match peeks {
            true => false,
            false => retire_members(cx, data, others, &element.record)?,
        };
        tail.removed |= retired;
        match step {
            // A retire changes the lists of the other list waiters, so the next round tests every
            // waiter again. Otherwise the waiters before the fired one fail again.
            FireStep::Continue => tested = if retired { 0 } else { position },
            FireStep::Done => return Ok(OfferEnd::Settled),
            FireStep::Reissue(cause) => return Ok(OfferEnd::Reissue(cause)),
        }
    }
}

/// A waiter that an offer fires: its position in the canonical order, its copy, the values that
/// it binds, and the other stored members of its list that the fire retires.
struct OfferTarget {
    position: usize,
    waiter: ParWaiter,
    matched: ListParWithRandom,
    others: Vec<ParDatum>,
}

/// The first waiter of the collection, in canonical order from the position `tested`, that
/// `element` fires (OFFER-VDB). A guard refusal moves to the next waiter (decision 37).
fn first_offer_target(
    cx: &mut Cx<'_, '_>,
    cfg: &VdbConfig,
    element: &Element,
    tested: usize,
) -> Result<Option<OfferTarget>, SpaceError> {
    let meter = cx.meter();
    let data = cx.data_channel();
    let order = cx.canonical_waiter_order(data)?;
    let flight = element.flight();
    for (position, &index) in order.iter().enumerate().skip(tested) {
        let (pattern, policy) = {
            let waiter = waiter_at(cx, index)?;
            let pattern = meter.copy(waiter_pattern(waiter)?)?;
            let (_, policy) =
                record::continuation_policy_slots_metered(&waiter.continuation, meter)?;
            (pattern, policy)
        };
        let bound = match policy_query(cfg, &policy, meter)? {
            None => cx
                .bind(&pattern, &element.payload)?
                .map(|matched| (matched, Vec::new())),
            Some(query) => match query.mode {
                Mode::Single => match flight_eligibility(&query, &flight, meter)? {
                    None => None,
                    Some(score) => {
                        let data = single_data(&query, &element.payload, score.as_ref(), meter)?;
                        cx.bind(&pattern, &data)?
                            .map(|matched| (matched, Vec::new()))
                    }
                },
                Mode::List(k) => list_offer(cx, cfg, &query, k, &pattern, data, &flight)?,
            },
        };
        let Some((matched, others)) = bound else {
            continue;
        };
        let waiter = meter.copy(waiter_at(cx, index)?)?;
        if cx.admits(&waiter.continuation, &matched)? {
            return Ok(Some(OfferTarget {
                position,
                waiter,
                matched,
                others,
            }));
        }
    }
    Ok(None)
}

/// The waiting continuation at `index` of the staged view of the collection.
fn waiter_at<'v>(cx: &'v mut Cx<'_, '_>, index: usize) -> Result<&'v ParWaiter, SpaceError> {
    cx.waiters()?.get(index).ok_or(SpaceError::Inconsistent(
        "a canonical index is outside the staged view",
    ))
}

// ── The driver: RECEIVE-VDB ──────────────────────────────────────────────────

/// The consume plan: RECEIVE-VDB of the continuation of `op`, then the tail. A re-issue moves
/// `op` and carries no order (decision 43). After a peek fire, the removed non-persistent members
/// are owed second sends, in delivery order (decision 45, Rule E).
fn receive_plan(
    discipline: &VectorDbDiscipline,
    cx: &mut Cx<'_, '_>,
    op: ConsumeOp,
) -> Result<Option<SpaceReissue>, SpaceError> {
    if let Some(reissue) = open_plan(discipline, cx, OpView::Consume(&op))? {
        return Ok(Some(reissue));
    }
    let meter = cx.meter();
    let data = cx.data_channel();
    let cfg = vdb_config(cx.config(), meter)?;
    let query = match &op.query {
        Some(query) => Some(query_of(&cfg, query, meter)?),
        None => None,
    };
    let policy = policy_of_query(op.query.as_ref(), meter)?;
    let continuation = record::wrap_policy_continuation_metered(
        0,
        &policy,
        &op.body,
        &op.guard,
        record::stamped(&op.authority),
        meter,
    )?;
    let origin = Origin::of(&op.body.random_state, record::stamped(&op.authority), meter)?;
    let mut tail = Tail::new();
    let own = loop {
        let found = receive_selection(cx, &cfg, query.as_ref(), &op.pattern, data)?;
        let fire = match found {
            Some(fire) if cx.admits(&continuation, &fire.matched)? => fire,
            Some(_) | None => {
                cx.check_capacity::<VectorDbDiscipline>(Side::Waiters)?;
                let peeks = peek_set(op.peek, meter)?;
                cx.park(&op, continuation, peeks)?;
                break None;
            }
        };
        let spent = !op.persistent;
        let stays = op.peek || fire.members.iter().any(|(record, _)| record.persist);
        // Rule E (decision 45): the peek fire removes each non-persistent member, as the host
        // COMM does, and the transaction sends each one again after the fire.
        if op.peek {
            let mut sends = meter.buffer::<ListParWithRandom>(fire.members.len())?;
            for (record, _) in &fire.members {
                if !record.persist {
                    sends.push(meter.copy(&record.a)?);
                }
            }
            tail.owe_all(sends, meter)?;
        }
        record::prepay_continuation_unwrap(&continuation, meter)?;
        let step = cx.rounds().after_fire(spent, stays);
        tail.removed = true;
        match step {
            FireStep::Continue => fire_members(cx, &op, meter.copy(&continuation)?, fire)?,
            FireStep::Done => {
                fire_members(cx, &op, continuation, fire)?;
                break None;
            }
            FireStep::Reissue(cause) => {
                fire_members(cx, &op, continuation, fire)?;
                break Some(cause);
            }
        }
    };
    let stop = finish_tail(cx, &cfg, &mut tail, &origin, own)?;
    let settle = match &stop {
        Some(_) => Some(settle_operation(OpView::Consume(&op), meter)?),
        None => None,
    };
    let resume = own.map(|cause| {
        (
            SpaceOp::Consume(ConsumeOp {
                reissued_ord: None,
                ..op
            }),
            cause,
        )
    });
    conclude(cx, settle, stop, resume)
}

/// A consume-side fire: the values that the first member binds, and the members in delivery
/// order with their indices in the staged view.
struct ReceiveFire {
    matched: ListParWithRandom,
    members: Vec<(ParDatum, usize)>,
}

/// The fire of a receive with the query `query` and the pattern `pattern` on `channel`: plain
/// (no query), single mode or list mode. Only the binding is checked, not the guard.
fn receive_selection(
    cx: &mut Cx<'_, '_>,
    cfg: &VdbConfig,
    query: Option<&VdbQuery>,
    pattern: &BindPattern,
    channel: &Par,
) -> Result<Option<ReceiveFire>, SpaceError> {
    let meter = cx.meter();
    match query {
        None => {
            let order = cx.canonical_data_order(channel)?;
            for &index in &order {
                let payload = payload_of(&stored_at(cx, channel, index)?.a, meter)?;
                if let Some(matched) = cx.bind(pattern, &payload)? {
                    let record = meter.copy(stored_at(cx, channel, index)?)?;
                    return Ok(Some(one_member(matched, record, index, meter)?));
                }
            }
            Ok(None)
        }
        Some(query) => match query.mode {
            Mode::Single => {
                for entry in eligible(cx, cfg, query, channel, None)? {
                    let index = entry.index.ok_or(SpaceError::Inconsistent(
                        "a receive met an element in flight",
                    ))?;
                    let payload = payload_of(&stored_at(cx, channel, index)?.a, meter)?;
                    let data = single_data(query, &payload, entry.score.as_ref(), meter)?;
                    if let Some(matched) = cx.bind(pattern, &data)? {
                        let record = meter.copy(stored_at(cx, channel, index)?)?;
                        return Ok(Some(one_member(matched, record, index, meter)?));
                    }
                }
                Ok(None)
            }
            Mode::List(k) => {
                let Some(fire) = list_selection(cx, cfg, query, k, pattern, channel, None)? else {
                    return Ok(None);
                };
                let mut members = meter.buffer::<(ParDatum, usize)>(fire.members.len())?;
                for member in fire.members {
                    members.push(member.ok_or(SpaceError::Inconsistent(
                        "a receive met an element in flight",
                    ))?);
                }
                Ok(Some(ReceiveFire {
                    matched: fire.matched,
                    members,
                }))
            }
        },
    }
}

/// The fire of one record.
fn one_member(
    matched: ListParWithRandom,
    record: ParDatum,
    index: usize,
    meter: &ScanMeter,
) -> Result<ReceiveFire, SpaceError> {
    let mut members = meter.buffer::<(ParDatum, usize)>(1)?;
    members.push((record, index));
    Ok(ReceiveFire { matched, members })
}

/// Stages the consume fire of the receive `op` over its members: the group `[D; m]` with the
/// pattern of `op` on each channel, whose outcome dispatches the result of the first member only
/// (arbitration A11, the set's F3 shape). The first member carries the values of the bind, and
/// each other member carries its own payload, which the outcome never dispatches. A peek peeks
/// every member.
fn fire_members(
    cx: &mut Cx<'_, '_>,
    op: &ConsumeOp,
    continuation: TaggedContinuation,
    fire: ReceiveFire,
) -> Result<(), SpaceError> {
    let meter = cx.meter();
    let data = cx.data_channel();
    let width = fire.members.len();
    let peeks = match op.peek {
        true => peek_set_all(width, meter)?,
        false => peek_set(false, meter)?,
    };
    let mut channels = meter.buffer::<Par>(width)?;
    let mut patterns = meter.buffer::<BindPattern>(width)?;
    let mut candidates = meter.buffer::<ParCandidate>(width)?;
    let mut first = Some(fire.matched);
    for (record, index) in fire.members {
        let matched = match first.take() {
            Some(matched) => matched,
            None => payload_of(&record.a, meter)?,
        };
        channels.push(meter.copy(data)?);
        patterns.push(meter.copy(&op.pattern)?);
        candidates.push(stored_candidate(meter.copy(data)?, Member {
            record,
            matched,
            view_index: view_index(index)?,
        }));
    }
    cx.fire_receive_group(
        channels,
        patterns,
        continuation,
        op.persistent,
        peeks,
        candidates,
        1,
    )
}

// ── The driver: the tail ─────────────────────────────────────────────────────

/// The tail of a plan whose element ended with `own` (`None` when it settled). A plan that
/// reached the round cap owes its second sends, and a cascade search when a record left the
/// collection. Otherwise the tail runs.
fn finish_tail(
    cx: &mut Cx<'_, '_>,
    cfg: &VdbConfig,
    tail: &mut Tail,
    origin: &Origin,
    own: Option<ReissueCause>,
) -> Result<Option<Stop>, SpaceError> {
    match own {
        Some(ReissueCause::RoundCap) => match tail.work.is_empty() && !tail.removed {
            true => Ok(None),
            false => Ok(Some(tail.stop(ReissueCause::RoundCap))),
        },
        Some(ReissueCause::DivergentPair) | None => run_tail(cx, cfg, tail, origin),
    }
}

/// TAIL (the DRAIN-AND-CASCADE of the join engine): the owed second sends first, then
/// LIST-CASCADE, until no parked list waiter can fire. It returns the stop of the transaction,
/// or `None` when nothing is owed.
fn run_tail(
    cx: &mut Cx<'_, '_>,
    cfg: &VdbConfig,
    tail: &mut Tail,
    origin: &Origin,
) -> Result<Option<Stop>, SpaceError> {
    let meter = cx.meter();
    loop {
        if let Some(record) = tail.work.pop() {
            if cx.rounds().fired() >= FIRE_ROUND_CAP {
                tail.owe(record, meter)?;
                return Ok(Some(tail.stop(ReissueCause::RoundCap)));
            }
            let kept = meter.copy(&record)?;
            let element = Element::second_send(cx, cfg, record)?;
            if let OfferEnd::Reissue(cause) = offer_vdb(cx, cfg, element, tail)? {
                tail.owe(kept, meter)?;
                return Ok(Some(tail.stop(cause)));
            }
            continue;
        }
        if !tail.removed {
            return Ok(None);
        }
        let Some(target) = first_cascade_target(cx, cfg)? else {
            tail.removed = false;
            return Ok(None);
        };
        if cx.rounds().fired() >= FIRE_ROUND_CAP {
            return Ok(Some(tail.stop(ReissueCause::RoundCap)));
        }
        if let FireStep::Reissue(cause) = lift_and_reoffer(cx, target, origin, tail)? {
            return Ok(Some(tail.stop(cause)));
        }
    }
}

/// The re-issue at the end of a plan. Without a stop, the operation re-issues when its element
/// is still in flight (`resume`). With a stop, one settle job carries the owed second sends and
/// the re-issue of the operation (arbitration A7). `settle` holds the fields of the settle job
/// (`settle::settle_operation`).
fn conclude(
    cx: &mut Cx<'_, '_>,
    settle: Option<SettleOp>,
    stop: Option<Stop>,
    resume: Option<(SpaceOp, ReissueCause)>,
) -> Result<Option<SpaceReissue>, SpaceError> {
    let meter = cx.meter();
    let resume = resume.map(|(op, cause)| cx.reissue(op, cause));
    let (Some(mut settle), Some(stop)) = (settle, stop) else {
        return Ok(resume);
    };
    let mut pending = meter.buffer::<PendingProduce>(stop.pending.len())?;
    for record in stop.pending {
        pending.push(PendingProduce {
            key: copy_key(&settle.key, meter)?,
            record,
            persist: false,
            restore: None,
        });
    }
    settle.cascade = Some(Box::new(CascadeState {
        seeds: Vec::new(),
        pending,
        resume: resume.map(Box::new),
    }));
    Ok(Some(cx.reissue(SpaceOp::Settle(settle), stop.cause)))
}

/// The key `key` copied, with the copy of its bytes reserved.
fn copy_key(key: &CollectionKey, meter: &ScanMeter) -> Result<CollectionKey, SpaceError> {
    meter.reserve_copy_slice(key.as_bytes())?;
    Ok(key.clone())
}

// ── The driver: LIST-CASCADE ─────────────────────────────────────────────────

/// A parked list waiter that the cascade fires: its copy, the values that it binds, and the
/// members of its list in delivery order.
struct CascadeTarget {
    waiter: ParWaiter,
    matched: ListParWithRandom,
    members: Vec<ParDatum>,
}

/// The first parked list waiter of the collection, in canonical order, whose list `L(W, R)` is
/// not empty, binds its pattern and passes its guard.
fn first_cascade_target(
    cx: &mut Cx<'_, '_>,
    cfg: &VdbConfig,
) -> Result<Option<CascadeTarget>, SpaceError> {
    let meter = cx.meter();
    let data = cx.data_channel();
    let order = cx.canonical_waiter_order(data)?;
    for &index in &order {
        let (pattern, policy) = {
            let waiter = waiter_at(cx, index)?;
            let pattern = meter.copy(waiter_pattern(waiter)?)?;
            let (_, policy) =
                record::continuation_policy_slots_metered(&waiter.continuation, meter)?;
            (pattern, policy)
        };
        let Some(query) = policy_query(cfg, &policy, meter)? else {
            continue;
        };
        let Mode::List(k) = query.mode else {
            continue;
        };
        let Some(fire) = list_selection(cx, cfg, &query, k, &pattern, data, None)? else {
            continue;
        };
        let waiter = meter.copy(waiter_at(cx, index)?)?;
        if !cx.admits(&waiter.continuation, &fire.matched)? {
            continue;
        }
        let mut members = meter.buffer::<ParDatum>(fire.members.len())?;
        for member in fire.members {
            let (record, _) = member.ok_or(SpaceError::Inconsistent(
                "a cascade list holds an element in flight",
            ))?;
            members.push(record);
        }
        return Ok(Some(CascadeTarget {
            waiter,
            matched: fire.matched,
            members,
        }));
    }
    Ok(None)
}

/// LIFT-AND-REOFFER of a list target: the first member `v` of the list is lifted when it is not
/// persistent (`K_lift`) and offered again to the waiter, which binds the list. Without a peek,
/// the other non-persistent members leave through `K_retire`. A lifted `v` that a peek waiter
/// did not spend stays in flight and is owed as a second send (decision 30). It returns the step
/// of the fire.
fn lift_and_reoffer(
    cx: &mut Cx<'_, '_>,
    target: CascadeTarget,
    origin: &Origin,
    tail: &mut Tail,
) -> Result<FireStep, SpaceError> {
    let meter = cx.meter();
    let data = cx.data_channel();
    let CascadeTarget {
        waiter,
        matched,
        mut members,
    } = target;
    if members.is_empty() {
        return Err(SpaceError::Inconsistent("a cascade list has no member"));
    }
    let lead = members.remove(0);
    let peeks = !waiter.peeks.is_empty();
    let lifted = !lead.persist;
    if lifted {
        lift_record(cx, data, &lead, &origin.lift, origin.authority.as_ref())?;
    }
    let introduction = cx.introduction().for_record(&lead.a, meter)?;
    let spent = !lead.persist && !peeks;
    let stays = waiter.persist;
    record::prepay_continuation_unwrap(&waiter.continuation, meter)?;
    let fire = OfferFire {
        persistent: lead.persist,
        waiter,
        matched,
    };
    let step = cx.rounds().after_fire(spent, stays);
    let staged = meter.copy(&lead.a)?;
    let removed = meter.copy(&lead.a)?;
    let source = meter.copy(&lead.source)?;
    cx.fire_offer_on(data, fire, staged, removed, source, &introduction)?;
    tail.removed = true;
    match (peeks, lifted) {
        (true, true) => tail.owe(lead.a, meter)?,
        (true, false) => {}
        (false, _) => {
            retire_members(cx, data, members, &lead.a)?;
        }
    }
    Ok(step)
}
