//! The VectorDB discipline of the native spaces (P6, design v3 4.2-4.6; decisions 54, 60 and 66;
//! the families `hashmap:vectordb` and `vector:vectordb`).
//!
//! A VectorDB collection keeps records with vectors, and a receive selects records by
//! similarity. Every decision is exact (decision 54): the numeric core of
//! `interpreter::tensor` scores each record with exact rationals, and the selection order is the
//! better score first. Changed by decision 102 (rules CR-1, CR-3 and CR-4, Q-102-1 and
//! Q-102-2): the records of equal scores, a read without a query and the waiter of a send come
//! in the pick order of the operation. The waiter of the cascade keeps the canonical order of
//! DR-75 (decision 124). Play shuffles the canonical order of DR-75 once for each search, and
//! replay puts first the candidates that the recorded row names. The members and the ties of a
//! list keep the canonical order of DR-75. The text before: "then the lower rank in the
//! canonical order of DR-75 (decisions 41 and 60)." The collection has no counter, so every
//! record has the `ord` 0 (M-1), and a re-issued element keeps its record and its identity
//! (decision 43).
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
//! `threshold` fail with `QueryNeedsVector`, and the receive reads the records in its pick order
//! (changed by decision 102, Q-102-2; the text before was "in canonical order"). The eligible
//! records `E` are the records whose score is defined, that pass the bound
//! (`threshold` on a similarity, `radius` on a distance) and whose id is in `ids`. A record id
//! is the hash of its source produce. Single mode binds the first record of `E` whose value the
//! pattern binds. List mode (`k` or `results = "all"`) binds ONE value: the list of the first
//! `min(k, |E|)` payload values, each `[score, value]` with `scores = true`.
//!
//! ```text
//! RECEIVE-VDB(K)                                       ▷ changed by decision 102 and 101
//!   loop
//!     the recorded row of K is Stored and the commit of K may veto: park K, stop     ▷ E3-a
//!     single: r ← the first record of E(K) whose value the pattern of K binds; E(K) is in
//!             score order, and the ties of a score keep the pick order of K
//!     list:   L ← the first min(k, |E(K)|) records of E(K), ties in DR-75 order; the
//!             pattern binds the list of L
//!     plain:  r ← the first record in the pick order of K whose payload the pattern binds
//!     none, or the guard of K refuses (one check, decision 37): park K with its query, stop
//!     fire K over [D] with the first member, which carries the value of the bind
//!     K does not peek: K_retire for each other non-persistent member of L, in list order
//!     K peeks: the first member, when it is not persistent, is owed a second send (Rule E)
//!     Done: stop. Continue: loop. Reissue(c): K re-issues
//!   TAIL
//!
//! OFFER-VDB(x)
//!   loop
//!     W ← the first waiter, in the pick order of x, that x fires:            ▷ decision 102
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
//! The text before of RECEIVE-VDB, retired by decision 102 (rules CR-1, CR-4 and CR-7) and
//! decision 101 (rule CR-13):
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
//! ```
//!
//! The text before of the first line of OFFER-VDB: "W ← the first waiter, in canonical order,
//! that x fires:".
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

// Changed by decision 124 (plan v4 C7.9, ruling B-1, RIBA 2.1): the commit finds a waiter by
// its source. The text before:
// use std::collections::{BTreeMap, BTreeSet};
use std::collections::{BTreeMap, BTreeSet, HashMap};
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
// Added by decision 124 (plan v4 C7.9, ruling B-1): the row searches match in the commit.
use rspace_plus_plus::rspace::r#match::Match;
// Added by decision 102 (plan v4 C5.2): a pick names the operation that picks.
use rspace_plus_plus::rspace::rspace_interface::RSpaceOperationSource;
// Changed by decision 124 (plan v4 C7.9, ruling B-1): the row searches read the commit. The text
// before:
// use rspace_plus_plus::rspace::space_store_handle::canonical_data_order_with;
use rspace_plus_plus::rspace::space_store_handle::{
    canonical_continuation_order, canonical_data_order, canonical_data_order_with,
    incoming_candidate, ActualFire, RowItemRule, RowReissue, RowRun, RowSearch, SpaceTxnError,
    TailRead,
};
// Changed by decision 102 (plan v4 C5.2): a receive names the consume that it stages.
// use rspace_plus_plus::rspace::trace::event::Produce;
use rspace_plus_plus::rspace::trace::event::{Consume, Produce};
use serde::Serialize;

use super::queue::QueueDiscipline;
// Added by decision 124 (plan v4 C7.9, ruling B-1): the shared parts of the kind searches.
use super::scan::{commit_error, source_positions, RetireRows};
use super::scan::{
    located, retire_continuation, stored_candidate, view_index, Member, RETIRE_PATTERN,
};
// Added by decision 128 (kind routing): the similarity window owns the types of its steps
// (`disciplines::similarity`). The VectorDB discipline is the one discipline of that window, and
// it keeps its names for them.
pub use super::similarity::{
    Eligible, EligibleOrder, Flight, ListFire, Mode, SimilarityConfig as VdbConfig,
    SimilarityQuery as VdbQuery, ID_LEN, QUERY_VECTOR_KEY,
};
// Changed by decision 124 (plan v4 C7.11, ruling RADD 1.2): the tail read counts the rounds of
// its fires. The text before:
// use crate::rust::interpreter::reified::costs::{FireStep, ReissueCause, FIRE_ROUND_CAP};
use crate::rust::interpreter::reified::costs::{
    FireRounds, FireStep, ReissueCause, FIRE_ROUND_CAP,
};
use crate::rust::interpreter::spaces::agent::{
    CascadeState, ConsumeOp, PendingProduce, ProduceOp, QueryParams, SettleOp, SpaceOp,
    SpaceReissue,
};
// Added by decision 124 (plan v4 C7.11, ruling RADD 1.2): the snapshot of the book that the
// tail read of a VectorDB row reads.
use crate::rust::interpreter::spaces::ephemeral::BookPairs;
use crate::rust::interpreter::spaces::errors::SpaceError;
use crate::rust::interpreter::spaces::factory::defaults::{
    CAPACITY_PARAM, DIMENSIONS_PARAM, MAX_CAPACITY, MAX_DIMENSIONS, METRIC_PARAM,
};
// Changed by decision 101 (rule CR-13): a receive fires one bind, so no fire peeks a group.
// use crate::rust::interpreter::spaces::generic::{
//     open_plan, peek_set, peek_set_all, Cx, Discipline, JoinWindow, OfferFire, OpView, Ordered,
//     ParCandidate, ParDatum, ParWaiter, Side, SideBound,
// };
// Changed by decision 102 (plan v4 C7.22): a fire names the datum that it offers.
use crate::rust::interpreter::spaces::generic::{
    open_plan, peek_set, Cx, Discipline, JoinWindow, OfferFire, Offered, OpView, Ordered,
    ParCandidate, ParDatum, ParWaiter, Side, SideBound, StoreView,
};
// Added by decision 124 (plan v4 C7.9, ruling B-1): the carriers of the row searches.
use crate::rust::interpreter::spaces::generic::{
    CommitView, ParRowLists, ParRowRequest, ParRowSearch, ReceiveFire as StoredFire, TailEnd,
};
// Added by decision 124 (plan v4 C7.11, ruling RADD 1.2): the shared rules of the tail read of a
// join row, which the VectorDB rows follow.
use crate::rust::interpreter::spaces::join::{finish_read, join_commit_error, round_cap, TailStep};
use crate::rust::interpreter::spaces::matching::may_veto;
use crate::rust::interpreter::spaces::namespace::RandomPurpose;
use crate::rust::interpreter::spaces::record::{self, sole_expr, Introduction, WaiterPolicy};
// Added by decision 124 (plan v2 C7.10): the reader skips inert machinery waiters.
use crate::rust::interpreter::spaces::settle::machinery_shape;
// Changed by decision 124 (plan v4 C7.11, ruling RADD2v2 6.1): the plans re-issue their element
// with the SETTLE fn that E4.5 reads too. The text before:
// use crate::rust::interpreter::spaces::settle::{lift_pattern, settle_operation};
use crate::rust::interpreter::spaces::settle::{
    lift_pattern, reissued_without_order, settle_operation,
};
// Added by decision 124 (plan v4 C7.9, ruling B-1): the theory of the commit checks a tuple.
use crate::rust::interpreter::spaces::theory::SpaceTheory;
use crate::rust::interpreter::spaces::types::config::SpaceConfig;
use crate::rust::interpreter::tensor::exact::{self, Bound, Candidate, Metric, Query, Score};
use crate::rust::interpreter::tensor::num::{self, Exact, ExactVector, Number};

/// The send param that gives the vector of a record (P6, decision 56).
pub const EMBEDDING_PARAM: &str = "embedding";

// Changed by decision 128 (kind routing): the similarity window owns this item
// (`disciplines::similarity`), and the re-export above keeps its name here. The text before:
// ```
// /// The key of the query vector in the policy of a parked waiter.
// pub const QUERY_VECTOR_KEY: &str = "q";
// ```

/// The query param that delivers the score of each record with its value.
pub const SCORES_KEY: &str = "scores";

/// The query param that keeps only the records whose ids it lists.
pub const IDS_KEY: &str = "ids";

// Changed by decision 128 (kind routing): the similarity window owns this item
// (`disciplines::similarity`), and the re-export above keeps its name here. The text before:
// ```
// /// The length of a record id: the hash of the source produce of the record.
// pub const ID_LEN: usize = 32;
// ```

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
    /// Added by decision 102 (plan v4 C4.3, rule CR-1): a VectorDB picks at random in play:
    /// a tie of equal best scores, a read without a query and some waiters.
    const RANDOM_PICKS: bool = true;

    /// The `capacity` of the space bounds the records of each collection (design v3 4.3), with
    /// `CapacityExceeded`. Nothing bounds the waiters.
    fn bound(&self, config: &SpaceConfig, side: Side) -> Result<Option<SideBound>, SpaceError> {
        match side {
            Side::Data => Ok(Some(SideBound::Records(vdb_config(config)?.capacity))),
            Side::Waiters => Ok(None),
        }
    }

    /// The entry with the least `(ord, bincode bytes)`, as for the queue. Only the head driver
    /// asks for a head, and a VectorDB collection does not use the head driver.
    fn head<T: Serialize>(
        &self,
        side: Side,
        entries: &[Ordered<'_, T>],
    ) -> Result<Option<usize>, SpaceError> {
        QueueDiscipline.head(side, entries)
    }

    /// Every entry can meet an incoming element. Only the head driver asks.
    fn reaches_head(&self, _side: Side, _queued: usize) -> bool { true }

    /// The lesser order interacts first, as for the queue. Only the head driver asks.
    fn precedes(&self, side: Side, ord: i64, head: i64) -> Result<bool, SpaceError> {
        QueueDiscipline.precedes(side, ord, head)
    }

    /// Checks the `embedding` param of a send and the query of a receive before any staging:
    /// the vector of the space dimension, the ranges, the exclusive pairs and the metric rules
    /// (design v3 4.4).
    fn check_values(
        &self,
        config: &SpaceConfig,
        params: &BTreeMap<String, Par>,
        query: Option<&QueryParams>,
    ) -> Result<(), SpaceError> {
        let cfg = vdb_config(config)?;
        if let Some(embedding) = lookup(params, EMBEDDING_PARAM) {
            num::vector_of(embedding, Some(cfg.dimensions))
                .map_err(|error| SpaceError::of_number(error, EMBEDDING_PARAM))?;
        }
        match query {
            Some(query) => query_of(&cfg, query).map(|_| ()),
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
        let cfg = vdb_config(cx.config())?;
        let origin = Origin::of(&record.random_state, record.cost_authority.as_ref());
        let mut tail = Tail::new();
        tail.owe(record);
        match run_tail(cx, &cfg, &mut tail, &origin)? {
            None => Ok(None),
            Some(_) => Err(SpaceError::Inconsistent(
                "a restore outside a receive cannot re-issue its record",
            )),
        }
    }

    // Added by decision 128 (kind routing): the steps of the similarity window. Each one calls
    // the function of this module with the same arguments.

    fn similarity_config(config: &SpaceConfig) -> Result<VdbConfig, SpaceError> {
        vdb_config(config)
    }

    fn similarity_record(
        cfg: &VdbConfig,
        params: &BTreeMap<String, Par>,
        data: &ListParWithRandom,
    ) -> Result<(ListParWithRandom, Option<ExactVector>), SpaceError> {
        vdb_record(cfg, params, data)
    }

    fn record_vector(
        record: &ListParWithRandom,
        cfg: &VdbConfig,
    ) -> Result<Option<ExactVector>, SpaceError> {
        self::record_vector(record, cfg)
    }

    fn similarity_payload(record: &ListParWithRandom) -> Result<ListParWithRandom, SpaceError> {
        payload_of(record)
    }

    fn is_list_query(query: &QueryParams) -> bool { self::is_list_query(query) }

    fn policy_of_query(query: Option<&QueryParams>) -> WaiterPolicy { self::policy_of_query(query) }

    fn policy_query(
        cfg: &VdbConfig,
        policy: &WaiterPolicy,
    ) -> Result<Option<VdbQuery>, SpaceError> {
        self::policy_query(cfg, policy)
    }

    fn eligible<V: StoreView>(
        view: &mut V,
        cfg: &VdbConfig,
        query: &VdbQuery,
        channel: &Par,
        flight: Option<&Flight<'_>>,
        order: EligibleOrder<'_>,
    ) -> Result<Vec<Eligible>, SpaceError> {
        self::eligible(view, cfg, query, channel, flight, order)
    }

    fn flight_eligibility(
        query: &VdbQuery,
        flight: &Flight<'_>,
    ) -> Result<Option<Option<Score>>, SpaceError> {
        self::flight_eligibility(query, flight)
    }

    fn single_data(
        query: &VdbQuery,
        payload: &ListParWithRandom,
        score: Option<&Score>,
    ) -> Result<ListParWithRandom, SpaceError> {
        self::single_data(query, payload, score)
    }

    fn list_selection<V: StoreView>(
        view: &mut V,
        cfg: &VdbConfig,
        query: &VdbQuery,
        k: Option<usize>,
        pattern: &BindPattern,
        channel: &Par,
        flight: Option<&Flight<'_>>,
    ) -> Result<Option<ListFire>, SpaceError> {
        self::list_selection(view, cfg, query, k, pattern, channel, flight)
    }

    fn list_offer<V: StoreView>(
        view: &mut V,
        cfg: &VdbConfig,
        query: &VdbQuery,
        k: Option<usize>,
        pattern: &BindPattern,
        channel: &Par,
        flight: &Flight<'_>,
    ) -> Result<Option<ListFire>, SpaceError> {
        self::list_offer(view, cfg, query, k, pattern, channel, flight)
    }

    fn list_pairs(
        members: &[Option<(ParDatum, usize)>],
        element: Option<(&ParDatum, &ListParWithRandom)>,
    ) -> Result<Vec<(ParDatum, ListParWithRandom)>, SpaceError> {
        self::list_pairs(members, element)
    }

    fn members_admit(
        request: &ParRowRequest<'_>,
        matched: &ListParWithRandom,
        members: &[(ParDatum, ListParWithRandom)],
    ) -> Result<bool, SpaceTxnError> {
        self::members_admit(request, matched, members)
    }

    fn member_failures(
        rule: &dyn RowItemRule<TaggedContinuation, ListParWithRandom>,
        waiter: (&Consume, &TaggedContinuation, bool),
        matched: &ListParWithRandom,
        members: &[(ParDatum, ListParWithRandom)],
        failed: &mut Vec<(Blake2b256Hash, Blake2b256Hash)>,
    ) -> Result<(), SpaceTxnError> {
        self::member_failures(rule, waiter, matched, members, failed)
    }

    fn retire_list_members(
        cx: &mut Cx<'_, '_>,
        channel: &Par,
        members: Vec<ParDatum>,
        random: &Blake2b512Random,
        authority: Option<&CostAuthority>,
    ) -> Result<bool, SpaceError> {
        retire_members(cx, channel, members, random, authority)
    }
}

// ── The config ───────────────────────────────────────────────────────────────

// Changed by decision 128 (kind routing): the similarity window owns this item
// (`disciplines::similarity`), and the re-export above keeps its name here. The text before:
// ```
// /// The params of a VectorDB space (design v3 4.2).
// #[derive(Clone, Copy, Debug, PartialEq, Eq)]
// pub struct VdbConfig {
//     /// The most records of each collection.
//     pub capacity: usize,
//     /// The length of every vector of the space.
//     pub dimensions: usize,
//     /// The metric of a query without `metric`.
//     pub metric: Metric,
// }
// ```

/// The params of the VectorDB space with the config `config`. The factory stored canonical
/// params, so a malformed param is `Inconsistent`.
pub fn vdb_config(config: &SpaceConfig) -> Result<VdbConfig, SpaceError> {
    let capacity = config_count(config, CAPACITY_PARAM, MAX_CAPACITY)?;
    let dimensions = config_count(config, DIMENSIONS_PARAM, MAX_DIMENSIONS)?;
    let metric = match lookup(&config.params, METRIC_PARAM) {
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

/// The value of `key` in `map`.
fn lookup<'m>(map: &'m BTreeMap<String, Par>, key: &str) -> Option<&'m Par> { map.get(key) }

/// A count param of a config: a `GInt` in `[1, max]`.
fn config_count(config: &SpaceConfig, key: &str, max: i64) -> Result<usize, SpaceError> {
    let malformed = SpaceError::Inconsistent("a VectorDB config lacks a valid count");
    match lookup(&config.params, key) {
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

// Changed by decision 128 (kind routing): the similarity window owns this item
// (`disciplines::similarity`), and the re-export above keeps its name here. The text before:
// ```
// /// The mode of a query: one record, or a list of `k` records (`None` for all of them).
// #[derive(Clone, Copy, Debug, PartialEq, Eq)]
// pub enum Mode {
//     /// One record.
//     Single,
//     /// The list of the first `k` eligible records, or of all of them for `None`.
//     List(Option<usize>),
// }
// ```

// Changed by decision 128 (kind routing): the similarity window owns this item
// (`disciplines::similarity`), and the re-export above keeps its name here. The text before:
// ```
// /// A checked query of a receive (design v3 4.4).
// pub struct VdbQuery {
//     /// The query vector with its metric, or `None` for a read in canonical order.
//     pub exact: Option<Query>,
//     /// The `threshold` or the `radius`, or `None`.
//     pub bound: Option<Bound>,
//     /// Single mode or list mode.
//     pub mode: Mode,
//     /// True for `scores = true`.
//     pub scores: bool,
//     /// The ids of `ids`, sorted and without duplicates, or `None`.
//     pub ids: Option<Vec<[u8; ID_LEN]>>,
// }
// ```

/// The checked query of the receive query `query`: its payload holds the query vector, or
/// nothing.
pub fn query_of(cfg: &VdbConfig, query: &QueryParams) -> Result<VdbQuery, SpaceError> {
    let vector = match query.payload.as_slice() {
        [] => None,
        [vector] => Some(vector),
        _ => return Err(SpaceError::PositionalParamsUnsupported),
    };
    parse_query(cfg, vector, &query.named)
}

/// True when the receive query `query` is in list mode: it names `k` or `results`. A join bind
/// in list mode fails with `ListModeInJoin` (decision 66).
pub fn is_list_query(query: &QueryParams) -> bool {
    lookup(&query.named, K_KEY).is_some() || lookup(&query.named, RESULTS_KEY).is_some()
}

/// Checks a query with the query vector `vector` and the named params `named`, in a fixed order
/// so that a query with several faults reports one code: the keys that need a vector, the
/// metric, the vector, the bound, the mode, `scores` and `ids`. `named` may hold the key `q` of
/// a policy map, which this function does not read.
fn parse_query(
    cfg: &VdbConfig,
    vector: Option<&Par>,
    named: &BTreeMap<String, Par>,
) -> Result<VdbQuery, SpaceError> {
    if vector.is_none() {
        for key in VECTOR_KEYS {
            if lookup(named, key).is_some() {
                return Err(SpaceError::QueryNeedsVector {
                    key: key.to_string(),
                });
            }
        }
    }
    let metric = match lookup(named, METRIC_KEY) {
        None => cfg.metric,
        Some(value) => metric_of(value).ok_or(SpaceError::InvalidParam {
            key: METRIC_KEY.to_string(),
        })?,
    };
    let exact = match vector {
        None => None,
        Some(par) => {
            let vector = num::vector_of(par, Some(cfg.dimensions))
                .map_err(|error| SpaceError::of_number(error, QUERY_VECTOR_KEY))?;
            match Query::new(vector, metric)
                .map_err(|error| SpaceError::of_number(error, QUERY_VECTOR_KEY))?
            {
                Some(query) => Some(query),
                None => return Err(SpaceError::ZeroQueryVector),
            }
        }
    };
    let bound = match (lookup(named, THRESHOLD_KEY), lookup(named, RADIUS_KEY)) {
        (Some(_), Some(_)) => return Err(SpaceError::ConflictingParams),
        (Some(value), None) => {
            let threshold = exact_number(value, THRESHOLD_KEY)?;
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
            let radius = exact_number(value, RADIUS_KEY)?;
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
    let mode = match (lookup(named, K_KEY), lookup(named, RESULTS_KEY)) {
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
    let scores = match lookup(named, SCORES_KEY) {
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
    let ids = match lookup(named, IDS_KEY) {
        None => None,
        Some(value) => Some(ids_of(value)?),
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
fn exact_number(value: &Par, key: &str) -> Result<Exact, SpaceError> {
    let invalid = |_| SpaceError::InvalidParam {
        key: key.to_string(),
    };
    match num::number_of(value).map_err(invalid)? {
        Number::Dyadic(form) => Ok(form.value()),
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

/// The ids of an `ids` param: a list of 32-byte byte arrays, sorted and without duplicates.
fn ids_of(value: &Par) -> Result<Vec<[u8; ID_LEN]>, SpaceError> {
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
        if !ids.contains(&id) {
            ids.insert(id);
        }
    }
    let mut sorted = Vec::<[u8; ID_LEN]>::with_capacity(ids.len());
    sorted.extend(ids);
    Ok(sorted)
}

/// True when `ids` holds the id `hash`.
fn holds_id(ids: &[[u8; ID_LEN]], hash: &Blake2b256Hash) -> bool {
    ids.binary_search_by(|id| id.as_slice().cmp(hash.0.as_slice()))
        .is_ok()
}

/// The policy that a parked waiter with the query `query` keeps: the map of its named params,
/// with the query vector under `q`, or `None` without a query.
pub fn policy_of_query(query: Option<&QueryParams>) -> WaiterPolicy {
    let Some(query) = query else {
        return WaiterPolicy::None;
    };
    let mut map = query.named.clone();
    if let Some(vector) = query.payload.first() {
        map.insert(QUERY_VECTOR_KEY.to_string(), vector.clone());
    }
    match map.is_empty() {
        true => WaiterPolicy::None,
        false => WaiterPolicy::Query(Box::new(map)),
    }
}

/// The query of the policy `policy` of a parked waiter or of a join slot, or `None` for a waiter
/// without a query.
pub fn policy_query(
    cfg: &VdbConfig,
    policy: &WaiterPolicy,
) -> Result<Option<VdbQuery>, SpaceError> {
    match policy {
        WaiterPolicy::None => Ok(None),
        WaiterPolicy::Query(map) => {
            let vector = lookup(map, QUERY_VECTOR_KEY);
            parse_query(cfg, vector, map).map(Some)
        }
        WaiterPolicy::Level(_) => Err(SpaceError::Inconsistent(
            "a waiter of a VectorDB collection has a priority level",
        )),
    }
}

// ── Records ──────────────────────────────────────────────────────────────────

/// The record of a send with the params `params` and the data `data`, and its vector (design v3
/// 4.3): the vector is the `embedding` param, or else the payload itself when it is exactly one
/// non-empty list of numbers, or none.
pub fn vdb_record(
    cfg: &VdbConfig,
    params: &BTreeMap<String, Par>,
    data: &ListParWithRandom,
) -> Result<(ListParWithRandom, Option<ExactVector>), SpaceError> {
    let embedding = match lookup(params, EMBEDDING_PARAM) {
        Some(embedding) => Some(embedding),
        None => match data.pars.as_slice() {
            [single] if is_numeric_list(single) => Some(single),
            _ => None,
        },
    };
    let vector = match embedding {
        Some(par) => Some(
            num::vector_of(par, Some(cfg.dimensions))
                .map_err(|error| SpaceError::of_number(error, EMBEDDING_PARAM))?,
        ),
        None => None,
    };
    let record = record::wrap_vdb_datum(0, embedding, data)?;
    Ok((record, vector))
}

/// True when `par` is one non-empty list of numbers.
fn is_numeric_list(par: &Par) -> bool {
    match (sole_expr(par), par.connective_used) {
        (
            Some(ExprInstance::EListBody(EList {
                ps,
                remainder: None,
                ..
            })),
            false,
        ) => !ps.is_empty() && ps.iter().all(num::is_number),
        _ => false,
    }
}

/// The vector of a stored record, or `None` for a record without one.
pub fn record_vector(
    record: &ListParWithRandom,
    cfg: &VdbConfig,
) -> Result<Option<ExactVector>, SpaceError> {
    match record::vdb_datum_slots(record)?.1 {
        Some(embedding) => num::vector_of(embedding, Some(cfg.dimensions))
            .map(Some)
            .map_err(|error| SpaceError::of_number(error, EMBEDDING_PARAM)),
        None => Ok(None),
    }
}

/// The payload of a stored record: the datum as the program sent it.
pub fn payload_of(record: &ListParWithRandom) -> Result<ListParWithRandom, SpaceError> {
    Ok(record::unwrap_vdb_datum(record)?.datum)
}

// ── Delivered values ─────────────────────────────────────────────────────────

/// The value of a payload: the datum of a send of one datum, or the list of the data of a send
/// of several.
fn payload_value(payload: &ListParWithRandom) -> Par {
    match payload.pars.as_slice() {
        [single] => single.clone(),
        many => list_of(many.to_vec()),
    }
}

/// The list of `values` with the bit sets of the normalizer: the union of the `locally_free`
/// sets and the disjunction of the `connective_used` flags of the elements, as the records of a
/// space build their lists.
fn list_of(values: Vec<Par>) -> Par {
    let mut locally_free: Vec<u8> = Vec::new();
    let mut connective_used = false;
    for value in &values {
        if !value.locally_free.is_empty() {
            locally_free = union(locally_free, value.locally_free.clone());
        }
        connective_used = connective_used || value.connective_used;
    }
    let mut exprs = Vec::<Expr>::with_capacity(1);
    exprs.push(Expr {
        expr_instance: Some(ExprInstance::EListBody(EList {
            ps: values,
            locally_free: locally_free.clone(),
            connective_used,
            remainder: None,
        })),
    });
    Par {
        exprs,
        locally_free,
        connective_used,
        ..Par::default()
    }
}

/// `[score, value]` with the delivered score `score` (RN_53 of the exact score).
fn scored(score: f64, value: Par) -> Par {
    let mut exprs = Vec::<Expr>::with_capacity(1);
    exprs.push(Expr {
        expr_instance: Some(ExprInstance::GDouble(score.to_bits())),
    });
    let mut pair = Vec::<Par>::with_capacity(2);
    pair.push(Par {
        exprs,
        ..Par::default()
    });
    pair.push(value);
    list_of(pair)
}

/// The data of one value `value`: a list of one `Par`.
fn one_value(value: Par) -> Vec<Par> {
    let mut pars = Vec::<Par>::with_capacity(1);
    pars.push(value);
    pars
}

/// The delivered score of `score` for the query `query`.
fn delivered_score(score: &Score, query: &Query) -> Result<f64, SpaceError> {
    exact::delivered(score, query).map_err(|error| SpaceError::of_number(error, SCORES_KEY))
}

/// The data that a single-mode bind sees for a record with the payload `payload` and the score
/// `score`: the payload itself, or `[score, value]` with `scores = true` (design v3 4.4).
pub fn single_data(
    query: &VdbQuery,
    payload: &ListParWithRandom,
    score: Option<&Score>,
) -> Result<ListParWithRandom, SpaceError> {
    match (query.scores, &query.exact, score) {
        (true, Some(exact), Some(score)) => {
            let value = payload_value(payload);
            let delivered = delivered_score(score, exact)?;
            Ok(ListParWithRandom {
                pars: one_value(scored(delivered, value)),
                random_state: payload.random_state.clone(),
                cost_authority: payload.cost_authority.as_ref().cloned(),
                cost_stack: payload.cost_stack.as_ref().cloned(),
            })
        }
        _ => Ok(payload.clone()),
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
) -> Result<ListParWithRandom, SpaceError> {
    let random_state = match members {
        [] => return Err(SpaceError::Inconsistent("a list has no member")),
        [single] => single.payload.random_state.clone(),
        _ => {
            let mut randoms = Vec::<Blake2b512Random>::with_capacity(members.len());
            for member in members {
                let random = Blake2b512Random::from_bytes(&member.payload.random_state);
                randoms.push(random);
            }
            let merged = Blake2b512Random::merge(randoms);
            merged.to_bytes()
        }
    };
    let mut values = Vec::<Par>::with_capacity(members.len());
    for member in members {
        let value = payload_value(member.payload);
        values.push(match (query.scores, &query.exact, member.score) {
            (true, Some(exact), Some(score)) => scored(delivered_score(score, exact)?, value),
            _ => value,
        });
    }
    Ok(ListParWithRandom {
        pars: one_value(list_of(values)),
        random_state,
        cost_authority: None,
        cost_stack: None,
    })
}

// ── Selection ────────────────────────────────────────────────────────────────

// Changed by decision 128 (kind routing): the similarity window owns this item
// (`disciplines::similarity`), and the re-export above keeps its name here. The text before:
// ```
// /// An element in flight as a selection sees it: its record, its payload, the identity of its
// /// produce, its persistence and its vector.
// pub struct Flight<'a> {
//     /// The record: the envelope with its vector, then the payload.
//     pub record: &'a ListParWithRandom,
//     /// The payload of the record.
//     pub payload: &'a ListParWithRandom,
//     /// The identity of the produce of the record.
//     pub source: &'a Produce,
//     /// True for a persistent element.
//     pub persist: bool,
//     /// The vector of the record, or `None`.
//     pub vector: Option<&'a ExactVector>,
// }
// ```

// Changed by decision 128 (kind routing): the similarity window owns this item
// (`disciplines::similarity`), and the re-export above keeps its name here. The text before:
// ```
// /// An eligible record: its index in the staged view of its channel, or `None` for the element
// /// in flight, and its score (`None` for a query without a vector).
// pub struct Eligible {
//     /// The index in the staged view, or `None` for the element in flight.
//     pub index: Option<usize>,
//     /// The score, or `None` for a query without a vector.
//     pub score: Option<Score>,
// }
// ```

/// The eligible records of `query` on `channel` in selection order (design v3 4.4): the stored
/// records, and the element `flight` when it is given. A query with a vector orders by the better
/// score, then by `order`. A query without one keeps `order`. Changed by decision 102 (plan v4
/// C5.2): the order is an argument. The text before: "then by the canonical order of DR-75. A
/// query without one keeps the canonical order."
// Changed by decision 124 (plan v4 C7.9, ruling OP-J): the reads of this fn go through the
// view `view` in place of `cx`. The comment above a changed line keeps its text before.
// pub fn eligible(
pub fn eligible<V: StoreView>(
    // cx: &mut Cx<'_, '_>,
    view: &mut V,
    cfg: &VdbConfig,
    query: &VdbQuery,
    channel: &Par,
    flight: Option<&Flight<'_>>,
    order: EligibleOrder<'_>,
) -> Result<Vec<Eligible>, SpaceError> {
    // Changed by decision 124 (plan v4 C7.9, ruling B-1): the body moved to `eligible_in`, which
    // the commit reads too. The text before:
    // let stored = cx.data_on(channel)?.len();
    // Changed by decision 102 (plan v4 C5.2): a single-mode query takes the pick order.
    // let order = match flight {
    //     None => cx.canonical_data_order(channel)?,
    //     Some(flight) => {
    //         let extra = Datum {
    //             a: flight.record.clone(),
    //             persist: flight.persist,
    //             source: flight.source.clone(),
    //         };
    //         let records = cx.data_on(channel)?;
    //         canonical_data_order_with(records, &extra)
    //     }
    // };
    let order = match (flight, order) {
        // Added by decision 102 (plan v4 C5.7): kept DR-75, because a list and a repeated join
        // collection keep one deterministic order (Q-102-1, E1).
        (None, EligibleOrder::Canonical) => view.canonical_data_order(channel)?,
        (None, EligibleOrder::Pick(source)) => view.pick_data_order(channel, source)?,
        (Some(flight), order) => {
            let extra = Datum {
                a: flight.record.clone(),
                persist: flight.persist,
                source: flight.source.clone(),
            };
            match order {
                // Added by decision 102 (plan v4 C5.7): kept DR-75, because a list and a repeated
                // join collection keep one deterministic order (Q-102-1, E1).
                EligibleOrder::Canonical => {
                    let records = view.data_on(channel)?;
                    canonical_data_order_with(records, &extra)
                }
                EligibleOrder::Pick(source) => {
                    view.pick_data_order_with(channel, &extra, source)?
                }
            }
        }
    };
    // Changed by decision 124 (plan v4 C7.9, ruling B-1): the body moved to `eligible_in`, which
    // the commit reads too. The text before:
    // ```
    // let mut entries = Vec::<Eligible>::with_capacity(order.len());
    // for &index in &order {
    //     let in_flight = match (flight, index == stored) {
    //         (Some(flight), true) => Some(flight),
    //         _ => None,
    //     };
    //     if let Some(ids) = &query.ids {
    //         let held = match in_flight {
    //             Some(flight) => holds_id(ids, &flight.source.hash),
    //             None => holds_id(ids, &stored_at(cx, channel, index)?.source.hash),
    //         };
    //         if !held {
    //             continue;
    //         }
    //     }
    //     let score = match &query.exact {
    //         None => None,
    //         Some(exact) => {
    //             let score = match in_flight {
    //                 Some(flight) => match flight.vector {
    //                     Some(vector) => score_of(exact, vector, query)?,
    //                     None => None,
    //                 },
    //                 None => {
    //                     let record = &stored_at(cx, channel, index)?.a;
    //                     match record_vector(record, cfg)? {
    //                         Some(vector) => score_of(exact, &vector, query)?,
    //                         None => None,
    //                     }
    //                 }
    //             };
    //             let Some(score) = score else {
    //                 continue;
    //             };
    //             Some(score)
    //         }
    //     };
    //     entries.push(Eligible {
    //         index: in_flight.is_none().then_some(index),
    //         score,
    //     });
    // }
    // match query.exact {
    //     None => Ok(entries),
    //     Some(_) => ranked(entries),
    // }
    // ```
    let records = view.data_on(channel)?;
    eligible_in(records, &order, cfg, query, flight)
}

/// Added by decision 124 (plan v4 C7.9, ruling B-1): the eligible records of `query` among
/// `records` in the order `order`, with the element `flight` at the index `records.len()`. It is
/// the body of [`eligible`], which staging and the commit share.
fn eligible_in(
    records: &[ParDatum],
    order: &[usize],
    cfg: &VdbConfig,
    query: &VdbQuery,
    flight: Option<&Flight<'_>>,
) -> Result<Vec<Eligible>, SpaceError> {
    let stored = records.len();
    let mut entries = Vec::<Eligible>::with_capacity(order.len());
    for &index in order {
        let in_flight = match (flight, index == stored) {
            (Some(flight), true) => Some(flight),
            _ => None,
        };
        let record = match in_flight {
            Some(_) => None,
            None => Some(records.get(index).ok_or(SpaceError::Inconsistent(
                "a canonical index is outside the records",
            ))?),
        };
        if let Some(ids) = &query.ids {
            let held = match (in_flight, record) {
                (Some(flight), _) => holds_id(ids, &flight.source.hash),
                (None, Some(record)) => holds_id(ids, &record.source.hash),
                (None, None) => false,
            };
            if !held {
                continue;
            }
        }
        let score = match &query.exact {
            None => None,
            Some(exact) => {
                let score = match (in_flight, record) {
                    (Some(flight), _) => match flight.vector {
                        Some(vector) => score_of(exact, vector, query)?,
                        None => None,
                    },
                    (None, Some(record)) => match record_vector(&record.a, cfg)? {
                        Some(vector) => score_of(exact, &vector, query)?,
                        None => None,
                    },
                    (None, None) => None,
                };
                let Some(score) = score else {
                    continue;
                };
                Some(score)
            }
        };
        entries.push(Eligible {
            index: in_flight.is_none().then_some(index),
            score,
        });
    }
    match query.exact {
        None => Ok(entries),
        Some(_) => ranked(entries),
    }
}

/// Added by decision 124 (plan v4 C7.9, ruling B-1): LIST of the list-mode query `query` with
/// the length `k` over the eligible records `entries` of `records`: the list value that the
/// pattern binds and the members in delivery order, or None for an empty list. It is the body of
/// [`list_selection`] before its bind, which staging and the commit share.
fn list_window(
    records: &[ParDatum],
    entries: &[Eligible],
    k: Option<usize>,
    query: &VdbQuery,
    flight: Option<&Flight<'_>>,
) -> Result<Option<(ListParWithRandom, Vec<Option<(ParDatum, usize)>>)>, SpaceError> {
    let length = k.map_or(entries.len(), |k| k.min(entries.len()));
    if length == 0 {
        return Ok(None);
    }
    let list = entries.get(..length).ok_or(SpaceError::Inconsistent(
        "a list is longer than its eligible records",
    ))?;
    let mut members = Vec::<Option<(ParDatum, usize)>>::with_capacity(length);
    let mut payloads = Vec::<ListParWithRandom>::with_capacity(length);
    for entry in list {
        match entry.index {
            Some(index) => {
                let record = records
                    .get(index)
                    .ok_or(SpaceError::Inconsistent(
                        "a canonical index is outside the records",
                    ))?
                    .clone();
                payloads.push(payload_of(&record.a)?);
                members.push(Some((record, index)));
            }
            None => {
                let flight = flight.ok_or(SpaceError::Inconsistent(
                    "a list holds an element in flight, but none is in flight",
                ))?;
                payloads.push(flight.payload.clone());
                members.push(None);
            }
        }
    }
    let mut views = Vec::<ListMember<'_>>::with_capacity(length);
    views.extend(
        list.iter()
            .zip(&payloads)
            .map(|(entry, payload)| ListMember {
                payload,
                score: entry.score.as_ref(),
            }),
    );
    let data = list_data(query, &views)?;
    Ok(Some((data, members)))
}

/// The score of `vector` for the query `exact` when it is defined and passes the bound of
/// `query`, or `None`.
fn score_of(
    exact: &Query,
    vector: &ExactVector,
    query: &VdbQuery,
) -> Result<Option<Score>, SpaceError> {
    let Some(score) = exact::score(exact, vector)
        .map_err(|error| SpaceError::of_number(error, QUERY_VECTOR_KEY))?
    else {
        return Ok(None);
    };
    match &query.bound {
        None => Ok(Some(score)),
        Some(bound) => {
            let within = exact::within(&score, bound, exact)
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
) -> Result<Option<Option<Score>>, SpaceError> {
    if let Some(ids) = &query.ids {
        if !holds_id(ids, &flight.source.hash) {
            return Ok(None);
        }
    }
    match &query.exact {
        None => Ok(Some(None)),
        Some(exact) => match flight.vector {
            None => Ok(None),
            Some(vector) => Ok(score_of(exact, vector, query)?.map(Some)),
        },
    }
}

/// `entries` in the selection order of their scores, ties in their current order
/// (`tensor::exact::rank`): the order of [`eligible`]. Changed by decision 102 (plan v4 C5.2).
/// The text before: "ties in their current order, which is the canonical order".
fn ranked(entries: Vec<Eligible>) -> Result<Vec<Eligible>, SpaceError> {
    let mut indices = Vec::<Option<usize>>::with_capacity(entries.len());
    let mut candidates = Vec::<Candidate>::with_capacity(entries.len());
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
    exact::rank(&mut candidates).map_err(|error| SpaceError::of_number(error, QUERY_VECTOR_KEY))?;
    let mut sorted = Vec::<Eligible>::with_capacity(candidates.len());
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

// Changed by decision 128 (kind routing): the similarity window owns this item
// (`disciplines::similarity`), and the re-export above keeps its name here. The text before:
// ```
// /// A list that a list-mode bind binds: the values that the pattern binds, and the members of
// /// the list in delivery order, each with its index in the staged view, or `None` for the element
// /// in flight.
// pub struct ListFire {
//     /// The values that the pattern binds.
//     pub matched: ListParWithRandom,
//     /// The members of the list in delivery order: the stored record and its index, or `None`
//     /// for the element in flight.
//     pub members: Vec<Option<(ParDatum, usize)>>,
// }
// ```

/// LIST of the list-mode query `query` with the length `k` on `channel`: the first `min(k, |E|)`
/// eligible records, with the element `flight` among them when it is given, when the list is not
/// empty and `pattern` binds it. Only the binding is checked, not the guard.
// Changed by decision 124 (plan v4 C7.9, rulings OP-J and RADD 1.7): the reads of this fn go
// through the view `view` in place of `cx`, so the commit reads the same list. The comment above
// a changed line keeps its text before.
// pub fn list_selection(
pub fn list_selection<V: StoreView>(
    // cx: &mut Cx<'_, '_>,
    view: &mut V,
    cfg: &VdbConfig,
    query: &VdbQuery,
    k: Option<usize>,
    pattern: &BindPattern,
    channel: &Par,
    flight: Option<&Flight<'_>>,
) -> Result<Option<ListFire>, SpaceError> {
    // Changed by decision 102 (plan v4 C5.2, Q-102-1): a list keeps the DR-75 order.
    // let entries = eligible(cx, cfg, query, channel, flight)?;
    let entries = eligible(view, cfg, query, channel, flight, EligibleOrder::Canonical)?;
    // Changed by decision 124 (plan v4 C7.9, ruling B-1): the body moved to `list_window`, which
    // the commit reads too. The text before:
    // ```
    // let length = k.map_or(entries.len(), |k| k.min(entries.len()));
    // if length == 0 {
    //     return Ok(None);
    // }
    // let list = entries.get(..length).ok_or(SpaceError::Inconsistent(
    //     "a list is longer than its eligible records",
    // ))?;
    // let mut members = Vec::<Option<(ParDatum, usize)>>::with_capacity(length);
    // let mut payloads = Vec::<ListParWithRandom>::with_capacity(length);
    // for entry in list {
    //     match entry.index {
    //         Some(index) => {
    //             let record = stored_at(cx, channel, index)?.clone();
    //             payloads.push(payload_of(&record.a)?);
    //             members.push(Some((record, index)));
    //         }
    //         None => {
    //             let flight = flight.ok_or(SpaceError::Inconsistent(
    //                 "a list holds an element in flight, but none is in flight",
    //             ))?;
    //             payloads.push(flight.payload.clone());
    //             members.push(None);
    //         }
    //     }
    // }
    // let mut views = Vec::<ListMember<'_>>::with_capacity(length);
    // views.extend(
    //     list.iter()
    //         .zip(&payloads)
    //         .map(|(entry, payload)| ListMember {
    //             payload,
    //             score: entry.score.as_ref(),
    //         }),
    // );
    // let data = list_data(query, &views)?;
    // ```
    let records = view.data_on(channel)?;
    let Some((data, members)) = list_window(records, &entries, k, query, flight)? else {
        return Ok(None);
    };
    // Ok(cx
    Ok(view
        .bind(pattern, &data)
        .map(|matched| ListFire { matched, members }))
}

/// OFFER-LIST: the list-mode waiter with the query `query`, the length `k` and the pattern
/// `pattern` against the element `flight` on `channel`. When the element is in the list
/// `L(W, R + x)` and the pattern binds the list, the values that it binds and the other stored
/// members of the list in delivery order. Only the binding is checked, not the guard. Changed by
/// decision 124 (plan v4 C7.9, ruling RADD 1.7): the members of the list in list order, with
/// `None` at the place of the element, so the item test of each member (C4v2 B.4) reads the
/// element at its place. `ListFire::into_stored` gives the form before.
// Changed by decision 124 (plan v4 C7.9, rulings OP-J and RADD 1.7): the fn reads any view and
// returns the list. The comment above a changed line keeps its text before.
// pub fn list_offer(
pub fn list_offer<V: StoreView>(
    // cx: &mut Cx<'_, '_>,
    view: &mut V,
    cfg: &VdbConfig,
    query: &VdbQuery,
    k: Option<usize>,
    pattern: &BindPattern,
    channel: &Par,
    flight: &Flight<'_>,
    // ) -> Result<Option<(ListParWithRandom, Vec<ParDatum>)>, SpaceError> {
) -> Result<Option<ListFire>, SpaceError> {
    // let Some(fire) = list_selection(cx, cfg, query, k, pattern, channel, Some(flight))? else {
    let Some(fire) = list_selection(view, cfg, query, k, pattern, channel, Some(flight))? else {
        return Ok(None);
    };
    if !fire.members.iter().any(Option::is_none) {
        return Ok(None);
    }
    // let mut others = Vec::<ParDatum>::with_capacity(fire.members.len());
    // others.extend(fire.members.into_iter().flatten().map(|(record, _)| record));
    // Ok(Some((fire.matched, others)))
    Ok(Some(fire))
}

// ── Retires and lifts ────────────────────────────────────────────────────────

/// Retires the stored members `members` of `channel` that are not persistent: one consume of
/// `K_retire(m)` over `[channel]` with the record `m` for each, in delivery order (the set
/// precedent, F3). The retires derive their random state from `random`, the random state of the
/// element or of the receive that fired, and carry its cost authority `authority`, as the join
/// engine retires (`RandomPurpose::Retire`). A COMM never removes a persistent record, so a
/// persistent member stays. Returns true when a record was retired. Changed by decision 101
/// (rule CR-13): a receive that does not peek retires the other members of its list too, so the
/// random state and the authority are arguments. The text before: "The retires derive their
/// random state from the element `element` that fired and carry its cost authority (as
/// `scan::retiring_of`)."
pub fn retire_members(
    cx: &mut Cx<'_, '_>,
    channel: &Par,
    members: Vec<ParDatum>,
    // Changed by decision 101 (rule CR-13): the random state and the authority of the fire.
    // element: &ListParWithRandom,
    random: &Blake2b512Random,
    authority: Option<&CostAuthority>,
) -> Result<bool, SpaceError> {
    // Changed by decision 101 (rule CR-13): the caller gives the random state of the fire.
    // let random = RandomPurpose::Retire.derive(&Blake2b512Random::from_bytes(&element.random_state));
    let random = RandomPurpose::Retire.derive(random);
    let mut retired = false;
    // Added by decision 124 (plan v2 C7.6, decision 99): `K_retire` takes the machinery seal.
    // The seal is the same for each member, so it is built once.
    let seal = cx.introduction().seal_of(authority)?;
    for member in members {
        if member.persist {
            continue;
        }
        let index = located(cx, channel, &member.source.hash)?;
        let payload = payload_of(&member.a)?;
        let matched = cx
            .bind(&RETIRE_PATTERN, &payload)
            .ok_or(SpaceError::Inconsistent(
                "a retired member does not bind the retire pattern",
            ))?;
        // Changed by decision 101 (rule CR-13): the authority of the fire is an argument.
        // let authority = element.cost_authority.as_ref().cloned();
        // let continuation = retire_continuation(&member.source.hash, &random, authority);
        let continuation = retire_continuation(&member.source.hash, &random, seal.clone());
        let mut channels = Vec::<Par>::with_capacity(1);
        channels.push(channel.clone());
        let mut patterns = Vec::<BindPattern>::with_capacity(1);
        patterns.push((*RETIRE_PATTERN).clone());
        let mut candidates = Vec::<ParCandidate>::with_capacity(1);
        candidates.push(stored_candidate(channel.clone(), Member {
            record: member,
            matched,
            view_index: view_index(index)?,
        }));
        // Changed by decision 124 (plan v4 C7.11, ruling RADD2 1.6): the retire names the
        // VectorDB discipline, which its machinery search keeps. The text before:
        // cx.retire(channels, patterns, continuation, candidates)?;
        cx.retire::<VectorDbDiscipline>(channels, patterns, continuation, candidates)?;
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
    let index = located(cx, channel, &lead.source.hash)?;
    let matched = cx
        .bind(&LIFT_PATTERN, &lead.a)
        .ok_or(SpaceError::Inconsistent(
            "a stored record does not match the lift pattern",
        ))?;
    // Changed by decision 124 (plan v2 C7.6, decision 99): the `K_lift` of a VectorDB lift takes
    // the machinery seal. The text before:
    // let authority = authority.cloned();
    let authority = cx.introduction().seal_of(authority)?;
    let continuation = retire_continuation(&lead.source.hash, random, authority);
    let mut channels = Vec::<Par>::with_capacity(1);
    channels.push(channel.clone());
    let mut patterns = Vec::<BindPattern>::with_capacity(1);
    patterns.push((*LIFT_PATTERN).clone());
    let mut candidates = Vec::<ParCandidate>::with_capacity(1);
    candidates.push(stored_candidate(channel.clone(), Member {
        record: lead.clone(),
        matched,
        view_index: view_index(index)?,
    }));
    // Changed by decision 124 (plan v4 C7.11, ruling RADD2 1.6): the lift names the VectorDB
    // discipline, which its machinery search keeps. The text before:
    // cx.lift(channels, patterns, continuation, candidates)
    cx.lift::<VectorDbDiscipline>(channels, patterns, continuation, candidates)
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
        let payload = payload_of(&record)?;
        let vector = record_vector(&record, cfg)?;
        let source = Produce::create(cx.data_channel(), &record, false);
        let introduction = cx.introduction().for_record(&record)?;
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
    fn of(random_state: &[u8], authority: Option<&CostAuthority>) -> Self {
        Self {
            lift: RandomPurpose::Lift.derive(&Blake2b512Random::from_bytes(random_state)),
            authority: authority.cloned(),
        }
    }
}

/// The work of the tail of a plan: the owed second sends (a stack: the last pushed runs first),
/// and whether a record left the collection since the last cascade search.
struct Tail {
    work: Vec<ListParWithRandom>,
    removed: bool,
    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.2): true from the start of
    /// `run_tail`, so a search that a fire builds after it reads the drain (`Tail::at`).
    draining: bool,
    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.1): the end of the
    /// element of the operation when the tail runs (`finish_tail`): `None` when it settled, or
    /// the cause of its re-issue. `None` in the plan, whose fire decides it.
    own: Option<ReissueCause>,
}

impl Tail {
    fn new() -> Self {
        Self {
            work: Vec::new(),
            removed: false,
            // Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.2): the plan comes
            // before the drain.
            draining: false,
            // Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.1): the plan
            // decides the end of its element.
            own: None,
        }
    }

    /// Owes `record` as the next second send.
    fn owe(&mut self, record: ListParWithRandom) { self.work.push(record); }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD 1.2 and RADD2 1.1): the
    /// state of the tail at a fire, for the tail read of its row: the phase (`cascade` for the
    /// re-offer of a cascade), a copy of the owed second sends, whether a record left the
    /// collection, and the end of the element of the operation.
    fn at(&self, cascade: bool) -> TailAt {
        let phase = match (cascade, self.draining) {
            (true, _) => VdbPhase::Cascade,
            (false, true) => VdbPhase::Drain,
            (false, false) => VdbPhase::Plan,
        };
        TailAt {
            phase,
            work: self.work.clone(),
            removed: self.removed,
            own: self.own,
        }
    }

    // Retired by decision 101 (rule CR-13): a peek receive binds one member, so it owes one
    // second send at most.
    // /// Owes `records` as second sends, the first of `records` first.
    // fn owe_all(&mut self, records: Vec<ListParWithRandom>) {
    //     for record in records.into_iter().rev() {
    //         self.owe(record);
    //     }
    // }

    /// The stop of a transaction with the cause `cause`: it owes the second sends of the tail, in
    /// the order in which they run.
    fn stop(&mut self, cause: ReissueCause) -> Stop {
        Stop {
            pending: std::mem::take(&mut self.work).into_iter().rev().collect(),
            cause,
        }
    }
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.2): the phase of a VectorDB
/// plan at a fire: the offer or the receive of the operation (`Plan`), the offer of an owed
/// second send in `run_tail` (`Drain`), or the re-offer of a cascade (`Cascade`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VdbPhase {
    Plan,
    Drain,
    Cascade,
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD 1.2 and RADD2 1.1): the state
/// of the tail at a fire (`Tail::at`): the phase, the owed second sends in the order of the
/// stack, whether a record left the collection, and the end of the element of the operation.
#[derive(Clone, Debug)]
struct TailAt {
    phase: VdbPhase,
    work: Vec<ListParWithRandom>,
    removed: bool,
    own: Option<ReissueCause>,
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD 1.2 and RADD2 1.1): what the
/// tail read of a VectorDB row starts from: the state of the tail at c0's fire, the fire count
/// before it, the snapshot of the book that the staging read (errata V4-13), and the copy of
/// the operation that the plan keeps (`Cx::operation`), for the re-issue.
struct VdbTail {
    at: TailAt,
    fired: usize,
    book: Arc<BookPairs>,
    operation: Arc<SpaceOp>,
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD 1.2 and RADD2 1.1): what a fire
/// of a VectorDB row decides for the steps after it: the two inputs of `FireRounds::after_fire`,
/// the record of the second send that it owes, and whether a record leaves the collection.
#[derive(Clone, Debug)]
struct VdbFire {
    spent: bool,
    stays: bool,
    owed: Option<ListParWithRandom>,
    removes: bool,
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
    let cfg = vdb_config(cx.config())?;
    let (record, vector) = vdb_record(&cfg, &op.params, &op.data)?;
    let source = Produce::create(cx.data_channel(), &record, op.persistent);
    let element = Element {
        record,
        source,
        persist: op.persistent,
        introduction: Arc::clone(cx.introduction()),
        payload: op.data.clone(),
        vector,
    };
    let origin = Origin::of(&op.data.random_state, op.data.cost_authority.as_ref());
    let mut tail = Tail::new();
    let own = match offer_vdb(cx, &cfg, element, &mut tail)? {
        OfferEnd::Settled => None,
        OfferEnd::Reissue(cause) => Some(cause),
    };
    let stop = finish_tail(cx, &cfg, &mut tail, &origin, own)?;
    let settle = stop
        .as_ref()
        .map(|_| settle_operation(OpView::Produce(&op)));
    // Changed by decision 124 (plan v4 C7.11, rulings RADD2 1.1 and RADD2v2 6.1): the re-issue
    // of the operation is `settle::reissued_without_order`, which the tail read of a row and
    // E4.5 call too. The text before:
    // ```
    // let resume = own.map(|cause| {
    //     (
    //         SpaceOp::Produce(ProduceOp {
    //             reissued_ord: None,
    //             ..op
    //         }),
    //         cause,
    //     )
    // });
    // ```
    let resume = own.map(|cause| (reissued_without_order(SpaceOp::Produce(op)), cause));
    // Changed by decision 124 (plan v4 C7.11, ruling RADD2 1.1): `conclude` reads no `cx`. The
    // text before:
    // Ok(conclude(cx, settle, stop, resume))
    Ok(conclude(settle, stop, resume))
}

/// OFFER-VDB of `element` on the fire count of its transaction (I-21). It returns whether the
/// element settled or must re-issue. A retire records its removal in `tail`.
fn offer_vdb(
    cx: &mut Cx<'_, '_>,
    cfg: &VdbConfig,
    element: Element,
    tail: &mut Tail,
) -> Result<OfferEnd, SpaceError> {
    let data = cx.data_channel();
    // Retired by decision 102 (decision 119 S4): each search scans its pick order from the head.
    // let mut tested = 0;
    loop {
        // Changed by decision 102 (plan v4 C5.2): the search takes the pick order of the
        // element, whose produce is `element.source`. A Rule E second send offers its record
        // through here too.
        // let Some(target) = first_offer_target(cx, cfg, &element, tested)? else {
        let Some((target, order)) = first_offer_target(cx, cfg, &element)? else {
            cx.check_capacity::<VectorDbDiscipline>(Side::Data)?;
            cx.store_on(data, element.record, element.persist, &element.introduction)?;
            return Ok(OfferEnd::Settled);
        };
        // Changed by decision 102 (decision 119 S4): a found waiter has no resume position.
        // let OfferTarget {
        //     position,
        //     waiter,
        //     matched,
        //     others,
        // } = target;
        let OfferTarget {
            waiter,
            matched,
            others,
        } = target;
        // Added by decision 124 (L6, CR30 D-b): the fire takes one lease for each stored member
        // of its list.
        cx.lease_stored(others.len());
        let peeks = !waiter.peeks.is_empty();
        let spent = !element.persist && !peeks;
        let stays = waiter.persist;
        // Changed by decision 124 (plan v4 C7.9, ruling B-1, RIBA 2.6): the fire carries the row
        // search of the VectorDB offer, which the two lines after the literal set. The text
        // before:
        // let fire = OfferFire {
        let mut fire = OfferFire {
            persistent: element.persist,
            waiter,
            matched,
            // Added by decision 102 (plan v4 C7.22): the offer offers an element in flight.
            offered: Offered::InFlight,
            search: None,
        };
        let datum = (&element.record, &element.source, element.persist);
        fire.search = vdb_offer_search(
            cx,
            cfg,
            datum,
            &fire.waiter,
            &order,
            (&*tail, false),
            &others,
        )?;
        let step = cx.rounds().after_fire(spent, stays);
        let staged = element.record.clone();
        let removed = element.record.clone();
        let source = element.source.clone();
        cx.fire_offer_on(data, fire, staged, removed, source, &element.introduction)?;
        // A peek waiter takes no datum, so its fire retires nothing (decision 37).
        let retired = match peeks {
            true => false,
            // Changed by decision 101 (rule CR-13): the random state and the authority of the
            // element are arguments.
            // false => retire_members(cx, data, others, &element.record)?,
            false => {
                let random = Blake2b512Random::from_bytes(&element.record.random_state);
                let authority = element.record.cost_authority.as_ref();
                retire_members(cx, data, others, &random, authority)?
            }
        };
        tail.removed |= retired;
        match step {
            // Changed by decision 102 (decision 119 S4): the next search scans its pick order
            // from the head. The text before: "A retire changes the lists of the other list
            // waiters, so the next round tests every waiter again. Otherwise the waiters before
            // the fired one fail again."
            // FireStep::Continue => tested = if retired { 0 } else { position },
            FireStep::Continue => {}
            FireStep::Done => return Ok(OfferEnd::Settled),
            FireStep::Reissue(cause) => return Ok(OfferEnd::Reissue(cause)),
        }
    }
}

/// A waiter that an offer fires: its copy, the values that it binds, and the other stored
/// members of its list that the fire retires. Changed by decision 102 (decision 119 S4): the
/// text before: "its position in the canonical order, its copy, the values that it binds, and
/// the other stored members of its list that the fire retires."
struct OfferTarget {
    // Retired by decision 102 (decision 119 S4): no search resumes at a position.
    // position: usize,
    waiter: ParWaiter,
    matched: ListParWithRandom,
    others: Vec<ParDatum>,
}

/// The first waiter of the collection, in the pick order of `element`, that `element` fires
/// (OFFER-VDB). A guard refusal moves to the next waiter (decision 37). Changed by decision 102
/// (plan v4 C5.2, rules CR-1 and CR-4, Q-102-2): the text before: "The first waiter of the
/// collection, in canonical order from the position `tested`, that `element` fires
/// (OFFER-VDB)."
fn first_offer_target(
    cx: &mut Cx<'_, '_>,
    cfg: &VdbConfig,
    element: &Element,
    // Retired by decision 102 (decision 119 S4): each search scans its pick order from the head.
    // tested: usize,
    // Changed by decision 124 (plan v4 C7.9, ruling B-1): the pick hands out its pick order,
    // which the row search of the fire lists. The text before:
    // ) -> Result<Option<OfferTarget>, SpaceError> {
) -> Result<Option<(OfferTarget, Vec<usize>)>, SpaceError> {
    let data = cx.data_channel();
    // Changed by decision 102 (plan v4 C5.2): the pick order of the produce of the element,
    // scanned from its head.
    // let order = cx.canonical_waiter_order(data)?;
    // let flight = element.flight();
    // for (position, &index) in order.iter().enumerate().skip(tested) {
    let order = cx.pick_waiter_order(data, RSpaceOperationSource::Produce(&element.source))?;
    let flight = element.flight();
    for &index in &order {
        let (pattern, policy) = {
            let waiter = waiter_at(cx, index)?;
            // Added by decision 124 (plan v2 C7.10, decision 100): an inert machinery waiter is
            // no candidate.
            match machinery_shape(waiter) {
                false => {}
                true => continue,
            }
            let pattern = waiter_pattern(waiter)?.clone();
            let (_, policy) = record::continuation_policy_slots(&waiter.continuation)?;
            (pattern, policy)
        };
        let bound = match policy_query(cfg, &policy)? {
            None => cx
                .bind(&pattern, &element.payload)
                .map(|matched| (matched, Vec::new())),
            Some(query) => match query.mode {
                Mode::Single => match flight_eligibility(&query, &flight)? {
                    None => None,
                    Some(score) => {
                        let data = single_data(&query, &element.payload, score.as_ref())?;
                        cx.bind(&pattern, &data)
                            .map(|matched| (matched, Vec::new()))
                    }
                },
                // Changed by decision 124 (plan v4 C7.9, ruling RADD 1.7): `list_offer` keeps the
                // place of the element. The text before:
                // Mode::List(k) => list_offer(cx, cfg, &query, k, &pattern, data, &flight)?,
                Mode::List(k) => list_offer(cx, cfg, &query, k, &pattern, data, &flight)?
                    .map(ListFire::into_stored),
            },
        };
        let Some((matched, others)) = bound else {
            continue;
        };
        let waiter = waiter_at(cx, index)?.clone();
        // Added by decision 124 (plan v4 C7.8, rule CR-20, rulings OP-F13 and RAMD 1.5): a
        // waiter whose pair with the element or with a stored member of its list the book holds
        // is skipped, as a waiter whose guard refuses (decision 37).
        match cx.list_booked(Some(&element.source), &others, &waiter.source) {
            false => {}
            true => continue,
        }
        // The theory of the space checks the stored members of a list, and not the element in
        // flight (P7, TH-6).
        if cx.admits_records(
            &waiter.continuation,
            &matched,
            &others,
            |record| Some(&record.a),
            payload_of,
        )? {
            // Changed by decision 102 (decision 119 S4): no resume position.
            // return Ok(Some(OfferTarget {
            //     position,
            //     waiter,
            //     matched,
            //     others,
            // }));
            return Ok(Some((
                OfferTarget {
                    waiter,
                    matched,
                    others,
                },
                order,
            )));
        }
    }
    Ok(None)
}

/// The waiting continuation at `index` of the staged view of the collection.
// Changed by decision 124 (plan v4 C7.11, ruling RADD 1.2): the reads of this fn go through
// the view `view` in place of `cx`. The comment above a changed line keeps its text before.
// fn waiter_at<'v>(cx: &'v mut Cx<'_, '_>, index: usize) -> Result<&'v ParWaiter, SpaceError> {
fn waiter_at<'v, V: StoreView>(view: &'v mut V, index: usize) -> Result<&'v ParWaiter, SpaceError> {
    // cx.waiters()?.get(index).ok_or(SpaceError::Inconsistent(
    view.waiters()?.get(index).ok_or(SpaceError::Inconsistent(
        "a canonical index is outside the staged view",
    ))
}

// ── The driver: RECEIVE-VDB ──────────────────────────────────────────────────

/// The consume plan: RECEIVE-VDB of the continuation of `op`, then the tail. A re-issue moves
/// `op` and carries no order (decision 43). After a peek fire, the first member is owed a second
/// send when it is not persistent (decision 45, Rule E). Changed by decision 101 (rule CR-13):
/// the text before: "After a peek fire, the removed non-persistent members are owed second
/// sends, in delivery order (decision 45, Rule E)."
fn receive_plan(
    discipline: &VectorDbDiscipline,
    cx: &mut Cx<'_, '_>,
    op: ConsumeOp,
) -> Result<Option<SpaceReissue>, SpaceError> {
    if let Some(reissue) = open_plan(discipline, cx, OpView::Consume(&op))? {
        return Ok(Some(reissue));
    }
    let data = cx.data_channel();
    let cfg = vdb_config(cx.config())?;
    let query = match &op.query {
        Some(query) => Some(query_of(&cfg, query)?),
        None => None,
    };
    let policy = policy_of_query(op.query.as_ref());
    let continuation = record::wrap_policy_continuation_from_parts(
        0,
        &policy,
        &op.body,
        &op.guard,
        record::stamped(&op.authority),
    )?;
    let origin = Origin::of(&op.body.random_state, record::stamped(&op.authority));
    // Added by decision 102 (plan v4 C5.2 and C5.6): the consume that the receive stages, its
    // park and its one-bind fire alike. Its recorded row orders the picks of replay (CR-4) and
    // decides E3-a (CR-7).
    let source = cx.receive_source(&op, &continuation);
    let mut tail = Tail::new();
    let own = loop {
        // Added by decision 102 (plan v4 C5.6, rule CR-7, base E3-a): replay parks the receive
        // without a search when its recorded row is Stored and its commit may veto. Play reads
        // no row, so it always searches.
        if cx.recorded_stored(RSpaceOperationSource::Consume(&source)) && cx.may_veto(&continuation)
        {
            cx.check_capacity::<VectorDbDiscipline>(Side::Waiters)?;
            let peeks = peek_set(op.peek);
            // Changed by decision 124 (plan v4 C7.9, errata V4-17, ruling B-1): the E3-a park
            // carries the receive search for `verify_row_refusals`, CanStop and the enumeration.
            // The text before:
            // cx.park(&op, continuation, peeks)?;
            let receive = (&source, &continuation);
            let search = vdb_receive_search(cx, &cfg, &op, receive, query.as_ref(), None, &tail)?;
            let mut channels = Vec::<Par>::with_capacity(1);
            channels.push(data.clone());
            let mut patterns = Vec::<BindPattern>::with_capacity(1);
            patterns.push(op.pattern.clone());
            cx.park_on(
                channels,
                patterns,
                continuation,
                op.persistent,
                peeks,
                search,
            )?;
            break None;
        }
        // Changed by decision 102 (plan v4 C5.2): the search takes the pick order of the receive.
        // let found = receive_selection(cx, &cfg, query.as_ref(), &op.pattern, data)?;
        let found = receive_selection(cx, &cfg, query.as_ref(), &op.pattern, data, &source)?;
        // Added by decision 124 (plan v4 C7.8, rule CR-20, rulings OP-F13 and RAMD 1.5): a fire
        // with a member whose pair with the consume the book holds parks the continuation, as a
        // guard refusal does. A plain or single-mode fire has one member, and a list fire reads
        // each member of its list.
        let booked = match found.as_ref() {
            Some((fire, _)) => {
                cx.list_booked(None, fire.members.iter().map(|(record, _)| record), &source)
            }
            None => false,
        };
        match booked {
            false => {}
            true => {
                cx.check_capacity::<VectorDbDiscipline>(Side::Waiters)?;
                let peeks = peek_set(op.peek);
                cx.park(&op, continuation, peeks)?;
                break None;
            }
        }
        // Changed by decision 124 (plan v4 C7.9, ruling B-1): the pick hands out its pick order,
        // which the row search of the fire lists. The text before:
        // ```
        // let fire = match found {
        //     Some(fire)
        //         if cx.admits_records(
        //             &continuation,
        //             &fire.matched,
        //             &fire.members,
        //             |(record, _)| Some(&record.a),
        //             payload_of,
        //         )? =>
        //     {
        //         fire
        //     }
        // ```
        let (fire, entries) = match found {
            Some((fire, entries))
                if cx.admits_records(
                    &continuation,
                    &fire.matched,
                    &fire.members,
                    |(record, _)| Some(&record.a),
                    payload_of,
                )? =>
            {
                (fire, entries)
            }
            Some(_) | None => {
                cx.check_capacity::<VectorDbDiscipline>(Side::Waiters)?;
                let peeks = peek_set(op.peek);
                cx.park(&op, continuation, peeks)?;
                break None;
            }
        };
        // Added by decision 124 (L6, CR30 D-b): the fire takes one lease for each stored member
        // of its list.
        cx.lease_stored(fire.members.len());
        // Added by decision 124 (plan v4 C7.9, ruling B-1): the row search of the fire, over the
        // pick order of the receive. Its staged candidate is the member that the fire binds.
        let receive = (&source, &continuation);
        let pick = Some((&fire, entries.as_slice()));
        let search = vdb_receive_search(cx, &cfg, &op, receive, query.as_ref(), pick, &tail)?;
        let spent = !op.persistent;
        let stays = op.peek || fire.members.iter().any(|(record, _)| record.persist);
        // Rule E (decision 45): the peek fire removes the first member when it is not
        // persistent, as the host COMM does, and the transaction sends it again after the fire.
        // Changed by decision 101 (rule CR-13): a peek binds the first member only and retires
        // nothing, so it owes one second send at most. The text before: "the peek fire removes
        // each non-persistent member, as the host COMM does, and the transaction sends each one
        // again after the fire."
        // if op.peek {
        //     let mut sends = Vec::<ListParWithRandom>::with_capacity(fire.members.len());
        //     for (record, _) in &fire.members {
        //         if !record.persist {
        //             sends.push(record.a.clone());
        //         }
        //     }
        //     tail.owe_all(sends);
        // }
        match (op.peek, fire.members.first()) {
            (true, Some((first, _))) if !first.persist => tail.owe(first.a.clone()),
            (true, _) | (false, _) => {}
        }
        let step = cx.rounds().after_fire(spent, stays);
        tail.removed = true;
        match step {
            // Changed by decision 124 (plan v4 C7.9, ruling B-1): the fire takes its search. The
            // text before:
            // FireStep::Continue => fire_members(cx, &op, continuation.clone(), fire)?,
            FireStep::Continue => fire_members(cx, &op, continuation.clone(), fire, search)?,
            FireStep::Done => {
                // Changed by decision 124 (plan v4 C7.9, ruling B-1): the fire takes its search.
                // The text before:
                // fire_members(cx, &op, continuation, fire)?;
                fire_members(cx, &op, continuation, fire, search)?;
                break None;
            }
            FireStep::Reissue(cause) => {
                // Changed by decision 124 (plan v4 C7.9, ruling B-1): the fire takes its search.
                // The text before:
                // fire_members(cx, &op, continuation, fire)?;
                fire_members(cx, &op, continuation, fire, search)?;
                break Some(cause);
            }
        }
    };
    let stop = finish_tail(cx, &cfg, &mut tail, &origin, own)?;
    let settle = stop
        .as_ref()
        .map(|_| settle_operation(OpView::Consume(&op)));
    // Changed by decision 124 (plan v4 C7.11, rulings RADD2 1.1 and RADD2v2 6.1): the re-issue
    // of the operation is `settle::reissued_without_order`, which the tail read of a row and
    // E4.5 call too. The text before:
    // ```
    // let resume = own.map(|cause| {
    //     (
    //         SpaceOp::Consume(ConsumeOp {
    //             reissued_ord: None,
    //             ..op
    //         }),
    //         cause,
    //     )
    // });
    // ```
    let resume = own.map(|cause| (reissued_without_order(SpaceOp::Consume(op)), cause));
    // Changed by decision 124 (plan v4 C7.11, ruling RADD2 1.1): `conclude` reads no `cx`. The
    // text before:
    // Ok(conclude(cx, settle, stop, resume))
    Ok(conclude(settle, stop, resume))
}

/// A consume-side fire: the values that the first member binds, and the members in delivery
/// order with their indices in the staged view.
struct ReceiveFire {
    matched: ListParWithRandom,
    members: Vec<(ParDatum, usize)>,
}

/// The fire of a receive with the query `query` and the pattern `pattern` on `channel`: plain
/// (no query), single mode or list mode. Only the binding is checked, not the guard. Changed by
/// decision 102 (plan v4 C5.2, rules CR-1, CR-3 and CR-4, Q-102-1 and Q-102-2): a plain read
/// and the ties of a single-mode score take the pick order of the receive whose consume is
/// `source`. A list keeps the canonical order of DR-75.
fn receive_selection(
    cx: &mut Cx<'_, '_>,
    cfg: &VdbConfig,
    query: Option<&VdbQuery>,
    pattern: &BindPattern,
    channel: &Par,
    // Added by decision 102 (plan v4 C5.2): the consume of the receive names its picks.
    source: &Consume,
    // Changed by decision 124 (plan v4 C7.9, ruling B-1): the pick hands out its pick order,
    // which the row search of the fire lists. The text before:
    // ) -> Result<Option<ReceiveFire>, SpaceError> {
) -> Result<Option<(ReceiveFire, Vec<Eligible>)>, SpaceError> {
    match query {
        None => {
            // Changed by decision 102 (plan v4 C5.2, Q-102-2): the pick order of the receive.
            // let order = cx.canonical_data_order(channel)?;
            let order = cx.pick_data_order(channel, RSpaceOperationSource::Consume(source))?;
            for &index in &order {
                let payload = payload_of(&stored_at(cx, channel, index)?.a)?;
                if let Some(matched) = cx.bind(pattern, &payload) {
                    let record = stored_at(cx, channel, index)?.clone();
                    // Changed by decision 124 (plan v4 C7.9, ruling B-1): the pick hands out its
                    // pick order, which the row search of the fire lists. The text before:
                    // return Ok(Some(one_member(matched, record, index)));
                    let mut entries = Vec::<Eligible>::with_capacity(order.len());
                    entries.extend(order.iter().map(|&listed| Eligible {
                        index: Some(listed),
                        score: None,
                    }));
                    return Ok(Some((one_member(matched, record, index), entries)));
                }
            }
            Ok(None)
        }
        Some(query) => match query.mode {
            Mode::Single => {
                // Changed by decision 102 (plan v4 C5.2): the ties of a score take the pick
                // order of the receive.
                // for entry in eligible(cx, cfg, query, channel, None)? {
                let order = EligibleOrder::Pick(RSpaceOperationSource::Consume(source));
                let entries = eligible(cx, cfg, query, channel, None, order)?;
                for entry in &entries {
                    let index = entry.index.ok_or(SpaceError::Inconsistent(
                        "a receive met an element in flight",
                    ))?;
                    let payload = payload_of(&stored_at(cx, channel, index)?.a)?;
                    let data = single_data(query, &payload, entry.score.as_ref())?;
                    if let Some(matched) = cx.bind(pattern, &data) {
                        let record = stored_at(cx, channel, index)?.clone();
                        // Changed by decision 124 (plan v4 C7.9, ruling B-1): the pick hands out
                        // its pick order, which the row search of the fire lists. The text
                        // before:
                        // return Ok(Some(one_member(matched, record, index)));
                        return Ok(Some((one_member(matched, record, index), entries)));
                    }
                }
                Ok(None)
            }
            Mode::List(k) => {
                let Some(fire) = list_selection(cx, cfg, query, k, pattern, channel, None)? else {
                    return Ok(None);
                };
                let mut members = Vec::<(ParDatum, usize)>::with_capacity(fire.members.len());
                for member in fire.members {
                    members.push(member.ok_or(SpaceError::Inconsistent(
                        "a receive met an element in flight",
                    ))?);
                }
                // Changed by decision 124 (plan v4 C7.9, ruling B-1): the pick hands out its pick
                // order, which the row search of the fire lists. The text before:
                // ```
                // Ok(Some(ReceiveFire {
                //     matched: fire.matched,
                //     members,
                // }))
                // ```
                let mut entries = Vec::<Eligible>::with_capacity(members.len());
                entries.extend(members.iter().map(|(_, index)| Eligible {
                    index: Some(*index),
                    score: None,
                }));
                Ok(Some((
                    ReceiveFire {
                        matched: fire.matched,
                        members,
                    },
                    entries,
                )))
            }
        },
    }
}

/// The fire of one record.
fn one_member(matched: ListParWithRandom, record: ParDatum, index: usize) -> ReceiveFire {
    let mut members = Vec::<(ParDatum, usize)>::with_capacity(1);
    members.push((record, index));
    ReceiveFire { matched, members }
}

/// Added by decision 124 (plan v4 C7.9 and C7.11, rulings B-1 and RADD 1.2): what the last tuple
/// of a VectorDB search read: each bound stored member with its payload, for the theory (P7,
/// TH-6), the values that the pattern binds, the members that the fire retires (CR-13), and what
/// its fire decides for the steps after it.
struct FoundList {
    stored: Vec<ListParWithRandom>,
    matched: ListParWithRandom,
    retires: Vec<Member>,
    step: VdbFire,
}

/// Added by decision 124 (plan v4 C7.9, ruling B-1): the candidates of a VectorDB receive.
enum ReceiveWindow {
    /// A plain read or a single-mode query: each record of the row order with the score that
    /// `single_data` delivers. The ties of a score keep the pick order (CR-1).
    Ranked(Vec<(Produce, Option<Score>)>),
    /// A list: the members of the staged list in delivery order and the values that the list
    /// binds. The list is the one candidate.
    List(Vec<Produce>, ListParWithRandom),
}

/// Added by decision 124 (plan v4 C7.9, section 2.4 rows "VectorDB single mode" and "VectorDB
/// list mode", errata V4-17, ruling B-1): the row search of a VectorDB receive and of its E3-a
/// park. A park lists no candidate (V4-17). A leasing theory gives no alternative (C4v2 G8).
pub(crate) struct VdbReceiveSearch {
    channel: Par,
    cfg: VdbConfig,
    params: Option<QueryParams>,
    window: ReceiveWindow,
    staged: Option<Produce>,
    receive: (Consume, TaggedContinuation, bool),
    pattern: BindPattern,
    peek: bool,
    theory: Option<Arc<dyn SpaceTheory>>,
    alternatives: bool,
    rows: RetireRows,
    staged_retires: usize,
    found: Option<FoundList>,
    /// The state that the tail read of the row starts from.
    tail: VdbTail,
    /// What the actual fire of the row decides, `None` when the row did not fire.
    fired: Option<VdbFire>,
    /// The end of Tail(actual) that the last tail read built (RADD2 1.1).
    reissue: Option<RowReissue>,
}

/// Added by decision 124 (plan v4 C7.9, ruling RADD 1.7, C4v2 B.4): the record and the payload
/// of each member of a list in list order, with the element `element` at its place (`None`), for
/// the item test of the members (`members_admit`).
pub(crate) fn list_pairs(
    members: &[Option<(ParDatum, usize)>],
    element: Option<(&ParDatum, &ListParWithRandom)>,
) -> Result<Vec<(ParDatum, ListParWithRandom)>, SpaceError> {
    let mut pairs = Vec::<(ParDatum, ListParWithRandom)>::with_capacity(members.len());
    for member in members {
        match (member, element) {
            (Some((stored, _)), _) => pairs.push((stored.clone(), payload_of(&stored.a)?)),
            (None, Some((datum, payload))) => pairs.push((datum.clone(), payload.clone())),
            (None, None) => {
                return Err(SpaceError::Inconsistent(
                    "a list holds an element in flight, but none is in flight",
                ))
            }
        }
    }
    Ok(pairs)
}

/// Added by decision 124 (plan v4 C7.9, ruling B-1): the item test of each member of a list
/// (C4v2 B.4): the first member with the values of the bind, each other member with its payload.
pub(crate) fn members_admit(
    request: &ParRowRequest<'_>,
    matched: &ListParWithRandom,
    members: &[(ParDatum, ListParWithRandom)],
) -> Result<bool, SpaceTxnError> {
    let Some(item) = &request.item else {
        return Ok(true);
    };
    for (position, (stored, payload)) in members.iter().enumerate() {
        let value = match position {
            0 => matched,
            _ => payload,
        };
        match item.admits(value, stored.persist)? {
            true => {}
            false => return Ok(false),
        }
    }
    Ok(true)
}

/// Added by decision 124 (plan v4 C7.9, rules CR-20 item 2 and R5, ruling B-1): the pairs of the
/// members of a list that the waiter `waiter` (its consume, its continuation, and whether it
/// persists) may not take, by the item test of [`members_admit`]: the first member with the
/// values of the bind, each other member with its payload. Each pair is the source hash of the
/// member and the hash of the consume.
pub(crate) fn member_failures(
    rule: &dyn RowItemRule<TaggedContinuation, ListParWithRandom>,
    waiter: (&Consume, &TaggedContinuation, bool),
    matched: &ListParWithRandom,
    members: &[(ParDatum, ListParWithRandom)],
    failed: &mut Vec<(Blake2b256Hash, Blake2b256Hash)>,
) -> Result<(), SpaceTxnError> {
    let (consume, continuation, persistent) = waiter;
    for (position, (stored, payload)) in members.iter().enumerate() {
        let value = match position {
            0 => matched,
            _ => payload,
        };
        match rule.admit_item(continuation, persistent, 1, value, stored.persist)? {
            true => {}
            false => failed.push((stored.source.hash.clone(), consume.hash.clone())),
        }
    }
    Ok(())
}

/// Added by decision 124 (plan v4 C7.9, ruling B-1): the guard once, then the theory on each
/// stored member (F12). A leasing theory never reaches here, because its search gives no
/// alternative (C4v2 G8).
fn alternative_admits(
    request: &ParRowRequest<'_>,
    theory: Option<&Arc<dyn SpaceTheory>>,
    found: Option<&FoundList>,
) -> Result<bool, SpaceTxnError> {
    let Some(found) = found else {
        return Err(SpaceTxnError::Inconsistent(
            "a VectorDB search checks a tuple that it did not read",
        ));
    };
    let matched = &found.matched;
    match request
        .matcher
        .check_commit_by_reference(request.continuation, &[matched])
    {
        false => return Ok(false),
        true => {}
    }
    let Some(theory) = theory else {
        return Ok(true);
    };
    for payload in &found.stored {
        let verdict = theory.check_match(payload, matched).map_err(commit_error)?;
        match verdict.is_proven() {
            true => {}
            false => return Ok(false),
        }
    }
    Ok(true)
}

/// Added by decision 124 (plan v4 C7.9, ruling B-1): the members that a fire retires: those
/// after the first that are not persistent, in list order (CR-13).
fn list_retires(
    request: &ParRowRequest<'_>,
    members: &[(ParDatum, ListParWithRandom, usize)],
) -> Result<Vec<Member>, SpaceTxnError> {
    let mut retires = Vec::<Member>::with_capacity(members.len());
    for (stored, payload, index) in members.iter().skip(1) {
        match stored.persist {
            true => {}
            false => retires.push(RetireRows::member(
                request.matcher,
                stored,
                payload,
                *index,
            )?),
        }
    }
    Ok(retires)
}

impl RowSearch<Par, BindPattern, ListParWithRandom, TaggedContinuation> for VdbReceiveSearch {
    fn runs(&self) -> Vec<RowRun<Par>> {
        let count = match &self.window {
            ReceiveWindow::Ranked(records) => records.len(),
            ReceiveWindow::List(..) => 1,
        };
        let mut channels = Vec::<Par>::with_capacity(1);
        channels.push(self.channel.clone());
        vec![RowRun {
            channels,
            candidates: 0..count,
        }]
    }

    fn continuations(
        &mut self,
        _run: usize,
        _lists: &ParRowLists<'_>,
    ) -> Result<Vec<(Arc<ParWaiter>, i32)>, SpaceTxnError> {
        Ok(Vec::new())
    }

    fn tuple(
        &mut self,
        _run: usize,
        lists: &ParRowLists<'_>,
        request: ParRowRequest<'_>,
    ) -> Result<Option<Vec<ParCandidate>>, SpaceTxnError> {
        self.found = None;
        let records = lists.data(&self.channel)?;
        let positions = source_positions(&records);
        let mut read = Vec::<(ParDatum, ListParWithRandom, usize)>::with_capacity(1);
        let matched = match &self.window {
            ReceiveWindow::List(members, matched) => {
                for produce in members {
                    let Some(&index) = positions.get(&produce.hash) else {
                        return Ok(None);
                    };
                    let Some(stored) = records.get(index) else {
                        return Ok(None);
                    };
                    let payload = payload_of(&stored.a).map_err(commit_error)?;
                    read.push((stored.clone(), payload, index));
                }
                let mut pairs = Vec::<(ParDatum, ListParWithRandom)>::with_capacity(read.len());
                pairs.extend(
                    read.iter()
                        .map(|(stored, payload, _)| (stored.clone(), payload.clone())),
                );
                match members_admit(&request, matched, &pairs)? {
                    true => {}
                    false => return Ok(None),
                }
                matched.clone()
            }
            ReceiveWindow::Ranked(ranked) => {
                let query = match &self.params {
                    Some(params) => Some(query_of(&self.cfg, params).map_err(commit_error)?),
                    None => None,
                };
                let mut bound: Option<ListParWithRandom> = None;
                for (produce, score) in ranked {
                    let Some(&index) = positions.get(&produce.hash) else {
                        continue;
                    };
                    let Some(stored) = records.get(index) else {
                        continue;
                    };
                    // Added by decision 124 (rules CR-4 and R13): the search of a recorded
                    // refusal binds only a record that the refusal names, and an earlier rank
                    // can hold another record. In play and in a selection the lists name every
                    // record.
                    match lists.named(stored, false)? {
                        true => {}
                        false => continue,
                    }
                    match (
                        self.alternatives,
                        Some(&stored.source) == self.staged.as_ref(),
                    ) {
                        (true, _) | (false, true) => {}
                        (false, false) => continue,
                    }
                    let payload = payload_of(&stored.a).map_err(commit_error)?;
                    let data = match &query {
                        None => payload.clone(),
                        Some(query) => {
                            single_data(query, &payload, score.as_ref()).map_err(commit_error)?
                        }
                    };
                    let Some(matched) = request.matcher.get(&self.pattern, &data) else {
                        continue;
                    };
                    let one = [(stored.clone(), payload.clone())];
                    match members_admit(&request, &matched, &one)? {
                        true => {}
                        false => continue,
                    }
                    read.push((stored.clone(), payload, index));
                    bound = Some(matched);
                    break;
                }
                let Some(matched) = bound else {
                    return Ok(None);
                };
                matched
            }
        };
        let retires = match self.peek {
            true => Vec::new(),
            false => list_retires(&request, &read)?,
        };
        let Some((first, _, index)) = read.first() else {
            return Ok(None);
        };
        let owed = match self.peek && !first.persist {
            false => None,
            true => Some(first.a.clone()),
        };
        let step = VdbFire {
            spent: !self.receive.2,
            stays: self.peek || read.iter().any(|(stored, _, _)| stored.persist),
            owed,
            removes: true,
        };
        let view_index = i32::try_from(*index)
            .map_err(|_| SpaceTxnError::Inconsistent("a record index overflows i32"))?;
        let candidate = stored_candidate(self.channel.clone(), Member {
            record: first.clone(),
            matched: matched.clone(),
            view_index,
        });
        let mut stored = Vec::<ListParWithRandom>::with_capacity(read.len());
        stored.extend(read.into_iter().map(|(_, payload, _)| payload));
        self.found = Some(FoundList {
            stored,
            matched,
            retires,
            step,
        });
        let mut tuple = Vec::<ParCandidate>::with_capacity(1);
        tuple.push(candidate);
        Ok(Some(tuple))
    }

    fn restore(&mut self) { self.found = None; }

    fn commit_check(
        &mut self,
        request: &ParRowRequest<'_>,
        tuple: &[ParCandidate],
    ) -> Result<bool, SpaceTxnError> {
        let [candidate] = tuple else {
            return Err(SpaceTxnError::Inconsistent(
                "a VectorDB receive binds one record",
            ));
        };
        match Some(&candidate.datum.source) == self.staged.as_ref() {
            true => Ok(true),
            false => alternative_admits(request, self.theory.as_ref(), self.found.as_ref()),
        }
    }

    fn may_veto(
        &self,
        matcher: &dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
        continuation: &TaggedContinuation,
    ) -> bool {
        may_veto(matcher, continuation, self.theory.is_some())
    }

    fn staged_retires(&self) -> usize { self.staged_retires }

    fn actual_fire(
        &mut self,
        _request: &ParRowRequest<'_>,
        _tuple: &[ParCandidate],
    ) -> ActualFire<Par, BindPattern, ListParWithRandom, TaggedContinuation> {
        self.fired = self.found.as_ref().map(|found| found.step.clone());
        let retires = match self.found.take() {
            Some(found) => self.rows.requests::<VectorDbDiscipline>(found.retires),
            None => Vec::new(),
        };
        ActualFire {
            retires,
            dispatched: None,
        }
    }

    fn item_failures(
        &self,
        lists: &ParRowLists<'_>,
        matcher: &dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
        rule: &dyn RowItemRule<TaggedContinuation, ListParWithRandom>,
    ) -> Result<Vec<(Blake2b256Hash, Blake2b256Hash)>, SpaceTxnError> {
        let (source, continuation, persistent) = &self.receive;
        // Added by decision 124 (plan v4 C7.9, rules CR-20 item 2 and R5, ruling B-1): the one
        // candidate of a list query is its list over the lists of the commit.
        let query = match &self.params {
            Some(params) => Some(query_of(&self.cfg, params).map_err(commit_error)?),
            None => None,
        };
        match query.as_ref().map(|query| (query, query.mode)) {
            Some((query, Mode::List(k))) => {
                let mut view = CommitView::new(lists, matcher, self.theory.as_deref(), None, 1);
                let fire = list_selection(
                    &mut view,
                    &self.cfg,
                    query,
                    k,
                    &self.pattern,
                    &self.channel,
                    None,
                )
                .map_err(commit_error)?;
                let Some(fire) = fire else {
                    return Ok(Vec::new());
                };
                let pairs = list_pairs(&fire.members, None).map_err(commit_error)?;
                let mut failed =
                    Vec::<(Blake2b256Hash, Blake2b256Hash)>::with_capacity(pairs.len());
                member_failures(
                    rule,
                    (source, continuation, *persistent),
                    &fire.matched,
                    &pairs,
                    &mut failed,
                )?;
                return Ok(failed);
            }
            Some((_, Mode::Single)) | None => {}
        }
        let records = lists.data(&self.channel)?;
        let mut failed = Vec::<(Blake2b256Hash, Blake2b256Hash)>::with_capacity(records.len());
        for index in canonical_data_order(&records) {
            let Some(stored) = records.get(index) else {
                continue;
            };
            let payload = payload_of(&stored.a).map_err(commit_error)?;
            let Some(matched) = matcher.get(&self.pattern, &payload) else {
                continue;
            };
            match rule.admit_item(continuation, *persistent, 1, &matched, stored.persist)? {
                true => {}
                false => failed.push((stored.source.hash.clone(), source.hash.clone())),
            }
        }
        Ok(failed)
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD 1.2 and RADD2 1.1, ADD
    /// A2.8): the tail read of a VectorDB row over the store of the commit (`vdb_tail_read`),
    /// with the end of Tail(actual) that the plan gives. `conclude` builds a re-issue and stages
    /// nothing.
    fn tail_read(
        &mut self,
        lists: &ParRowLists<'_>,
        matcher: &dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
    ) -> Result<TailRead, SpaceTxnError> {
        let mut view = CommitView::of_tail(
            lists,
            matcher,
            self.theory.as_deref(),
            Some(&self.channel),
            &self.tail.book,
            1,
        );
        let (step, end) = vdb_tail_read(
            &mut view,
            &self.cfg,
            &self.channel,
            &self.tail,
            self.fired.as_ref(),
            None,
        )
        .map_err(join_commit_error)?;
        self.reissue = Some(RowReissue::new(end));
        Ok(finish_read(step))
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.1): the end of
    /// Tail(actual) that the last tail read built.
    fn tail_reissue(&mut self) -> Option<RowReissue> { self.reissue.take() }
}

/// Added by decision 124 (plan v4 C7.9, section 2.4, ruling B-1): the row search of a VectorDB
/// offer and of a cascade re-offer. The row order is the waiter order of the pick, or the DR-75
/// order of the cascade. Each waiter takes the element in flight by its policy: a plain bind, a
/// single-mode eligibility, or a list that holds the element. A cascade (`lead`) reaches only
/// list waiters whose list starts with the element (LIFT-AND-REOFFER). A leasing theory gives
/// no alternative (C4v2 G8).
pub(crate) struct VdbOfferSearch {
    channel: Par,
    cfg: VdbConfig,
    waiters: Vec<(Consume, bool)>,
    staged: Consume,
    element: ParDatum,
    payload: ListParWithRandom,
    vector: Option<ExactVector>,
    lead: bool,
    theory: Option<Arc<dyn SpaceTheory>>,
    alternatives: bool,
    rows: RetireRows,
    staged_retires: usize,
    found: Option<FoundList>,
    /// The state that the tail read of the row starts from.
    tail: VdbTail,
    /// What the actual fire of the row decides, `None` when the row did not fire.
    fired: Option<VdbFire>,
    /// The end of Tail(actual) that the last tail read built (RADD2 1.1).
    reissue: Option<RowReissue>,
}

impl VdbOfferSearch {
    /// The values that a waiter with the patterns `patterns` and the continuation `continuation`
    /// binds with the element, and the members of its list in list order, with `None` for the
    /// element in flight, or None. A plain or a single-mode bind lists the element alone.
    fn bound(
        &self,
        lists: &ParRowLists<'_>,
        matcher: &dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
        patterns: &[BindPattern],
        continuation: &TaggedContinuation,
    ) -> Result<Option<(ListParWithRandom, Vec<Option<(ParDatum, usize)>>)>, SpaceTxnError> {
        let [pattern] = patterns else {
            return Err(SpaceTxnError::Inconsistent(
                "a waiting continuation of a collection has several binds",
            ));
        };
        let (_, policy) = record::continuation_policy_slots(continuation)
            .map_err(|error| commit_error(error.into()))?;
        let flight = Flight {
            record: &self.element.a,
            payload: &self.payload,
            source: &self.element.source,
            persist: self.element.persist,
            vector: self.vector.as_ref(),
        };
        let query = policy_query(&self.cfg, &policy).map_err(commit_error)?;
        let k = match (&query, self.lead) {
            (Some(query), _) => match query.mode {
                Mode::List(k) => Some(k),
                Mode::Single => None,
            },
            (None, _) => None,
        };
        match (query, k, self.lead) {
            (None, _, false) => Ok(matcher
                .get(pattern, &self.payload)
                .map(|matched| (matched, vec![None]))),
            (Some(query), None, false) => {
                match flight_eligibility(&query, &flight).map_err(commit_error)? {
                    None => Ok(None),
                    Some(score) => {
                        let data = single_data(&query, &self.payload, score.as_ref())
                            .map_err(commit_error)?;
                        Ok(matcher
                            .get(pattern, &data)
                            .map(|matched| (matched, vec![None])))
                    }
                }
            }
            (Some(query), Some(k), _) => {
                let records = lists.data(&self.channel)?;
                let positions = source_positions(&records);
                let stored = positions.contains_key(&self.element.source.hash);
                let in_flight = match stored {
                    true => None,
                    false => Some(&flight),
                };
                let order = match in_flight {
                    Some(_) => canonical_data_order_with(&records, &self.element),
                    None => canonical_data_order(&records),
                };
                let entries = eligible_in(&records, &order, &self.cfg, &query, in_flight)
                    .map_err(commit_error)?;
                let window =
                    list_window(&records, &entries, k, &query, in_flight).map_err(commit_error)?;
                let Some((data, members)) = window else {
                    return Ok(None);
                };
                let holds = match (self.lead, members.first()) {
                    (true, Some(None)) => true,
                    (true, Some(Some((first, _)))) => first.source == self.element.source,
                    (true, None) => false,
                    (false, _) => members.iter().any(Option::is_none),
                };
                match holds {
                    false => Ok(None),
                    true => Ok(matcher
                        .get(pattern, &data)
                        .map(|matched| (matched, members))),
                }
            }
            (None, _, true) | (Some(_), None, true) => Ok(None),
        }
    }
}

impl RowSearch<Par, BindPattern, ListParWithRandom, TaggedContinuation> for VdbOfferSearch {
    fn runs(&self) -> Vec<RowRun<Par>> {
        let mut channels = Vec::<Par>::with_capacity(1);
        channels.push(self.channel.clone());
        vec![RowRun {
            channels,
            candidates: 0..self.waiters.len(),
        }]
    }

    fn continuations(
        &mut self,
        _run: usize,
        lists: &ParRowLists<'_>,
    ) -> Result<Vec<(Arc<ParWaiter>, i32)>, SpaceTxnError> {
        let waiters = lists.continuations(std::slice::from_ref(&self.channel))?;
        let mut positions = HashMap::<&Blake2b256Hash, usize>::with_capacity(waiters.len());
        for (index, waiter) in waiters.iter().enumerate() {
            positions.entry(&waiter.source.hash).or_insert(index);
        }
        let mut listed = Vec::<(Arc<ParWaiter>, i32)>::with_capacity(self.waiters.len());
        for (source, _) in &self.waiters {
            match (self.alternatives, *source == self.staged) {
                (true, _) | (false, true) => {}
                (false, false) => continue,
            }
            let Some(&index) = positions.get(&source.hash) else {
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

    fn tuple(
        &mut self,
        _run: usize,
        lists: &ParRowLists<'_>,
        request: ParRowRequest<'_>,
    ) -> Result<Option<Vec<ParCandidate>>, SpaceTxnError> {
        self.found = None;
        let bound = self.bound(
            lists,
            request.matcher,
            request.patterns,
            request.continuation,
        )?;
        let Some((matched, members)) = bound else {
            return Ok(None);
        };
        let pairs =
            list_pairs(&members, Some((&self.element, &self.payload))).map_err(commit_error)?;
        match members_admit(&request, &matched, &pairs)? {
            true => {}
            false => return Ok(None),
        }
        let mut retires = Vec::<Member>::with_capacity(members.len());
        let mut stored = Vec::<ListParWithRandom>::with_capacity(members.len());
        for (member, (_, payload)) in members.iter().zip(&pairs) {
            let Some((record, index)) = member else {
                continue;
            };
            match record.source == self.element.source {
                false => {}
                true => continue,
            }
            stored.push(payload.clone());
            match record.persist {
                true => {}
                false => retires.push(RetireRows::member(
                    request.matcher,
                    record,
                    payload,
                    *index,
                )?),
            }
        }
        let peeks = match self
            .waiters
            .iter()
            .find(|(source, _)| source == request.source)
        {
            Some((_, peeks)) => *peeks,
            None => false,
        };
        let owed = match self.lead && peeks && !self.element.persist {
            false => None,
            true => Some(self.element.a.clone()),
        };
        let step = VdbFire {
            spent: !self.element.persist && !peeks,
            stays: request.persistent,
            owed,
            removes: self.lead || (!peeks && !retires.is_empty()),
        };
        let candidate = incoming_candidate(
            self.channel.clone(),
            self.element.source.clone(),
            self.element.a.clone(),
            self.element.persist,
            matched.clone(),
        );
        self.found = Some(FoundList {
            stored,
            matched,
            retires,
            step,
        });
        let mut tuple = Vec::<ParCandidate>::with_capacity(1);
        tuple.push(candidate);
        Ok(Some(tuple))
    }

    fn restore(&mut self) {}

    fn commit_check(
        &mut self,
        request: &ParRowRequest<'_>,
        _tuple: &[ParCandidate],
    ) -> Result<bool, SpaceTxnError> {
        match *request.source == self.staged {
            true => Ok(true),
            false => alternative_admits(request, self.theory.as_ref(), self.found.as_ref()),
        }
    }

    fn may_veto(
        &self,
        matcher: &dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
        continuation: &TaggedContinuation,
    ) -> bool {
        may_veto(matcher, continuation, self.theory.is_some())
    }

    fn staged_retires(&self) -> usize { self.staged_retires }

    fn actual_fire(
        &mut self,
        request: &ParRowRequest<'_>,
        _tuple: &[ParCandidate],
    ) -> ActualFire<Par, BindPattern, ListParWithRandom, TaggedContinuation> {
        self.fired = self.found.as_ref().map(|found| found.step.clone());
        let peeks = match self
            .waiters
            .iter()
            .find(|(source, _)| source == request.source)
        {
            Some((_, peeks)) => *peeks,
            None => false,
        };
        let retires = match (peeks, self.found.take()) {
            (false, Some(found)) => self.rows.requests::<VectorDbDiscipline>(found.retires),
            (true, _) | (false, None) => Vec::new(),
        };
        ActualFire {
            retires,
            dispatched: None,
        }
    }

    fn item_failures(
        &self,
        lists: &ParRowLists<'_>,
        matcher: &dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
        rule: &dyn RowItemRule<TaggedContinuation, ListParWithRandom>,
    ) -> Result<Vec<(Blake2b256Hash, Blake2b256Hash)>, SpaceTxnError> {
        let waiters = lists.continuations(std::slice::from_ref(&self.channel))?;
        let mut owned = Vec::<ParWaiter>::with_capacity(waiters.len());
        owned.extend(waiters.iter().map(|waiter| (**waiter).clone()));
        let mut failed = Vec::<(Blake2b256Hash, Blake2b256Hash)>::with_capacity(owned.len());
        for index in canonical_continuation_order(&owned) {
            let Some(waiter) = owned.get(index) else {
                continue;
            };
            // Added by decision 124 (plan v2 C7.10, decision 100): an inert machinery waiter
            // has no item test.
            match machinery_shape(waiter) {
                false => {}
                true => continue,
            }
            let [pattern] = waiter.patterns.as_slice() else {
                continue;
            };
            // Added by decision 124 (plan v4 C7.9, rules CR-20 item 2 and R5, ruling B-1): the
            // one candidate of a list waiter is its list, with the element at its place.
            let (_, policy) = record::continuation_policy_slots(&waiter.continuation)
                .map_err(|error| commit_error(error.into()))?;
            let mode = policy_query(&self.cfg, &policy)
                .map_err(commit_error)?
                .map(|query| query.mode);
            match mode {
                Some(Mode::List(_)) => {
                    let bound =
                        self.bound(lists, matcher, &waiter.patterns, &waiter.continuation)?;
                    let Some((matched, members)) = bound else {
                        continue;
                    };
                    let pairs = list_pairs(&members, Some((&self.element, &self.payload)))
                        .map_err(commit_error)?;
                    member_failures(
                        rule,
                        (&waiter.source, &waiter.continuation, waiter.persist),
                        &matched,
                        &pairs,
                        &mut failed,
                    )?;
                    continue;
                }
                Some(Mode::Single) | None => {}
            }
            let Some(matched) = matcher.get(pattern, &self.payload) else {
                continue;
            };
            let persist = self.element.persist;
            match rule.admit_item(&waiter.continuation, waiter.persist, 1, &matched, persist)? {
                true => {}
                false => {
                    let pair = (self.element.source.hash.clone(), waiter.source.hash.clone());
                    failed.push(pair);
                }
            }
        }
        Ok(failed)
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD 1.2 and RADD2 1.1, ADD
    /// A2.8): the tail read of a VectorDB row over the store of the commit (`vdb_tail_read`),
    /// with the end of Tail(actual) that the plan gives. `conclude` builds a re-issue and stages
    /// nothing.
    fn tail_read(
        &mut self,
        lists: &ParRowLists<'_>,
        matcher: &dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
    ) -> Result<TailRead, SpaceTxnError> {
        let mut view = CommitView::of_tail(
            lists,
            matcher,
            self.theory.as_deref(),
            Some(&self.channel),
            &self.tail.book,
            1,
        );
        let (step, end) = vdb_tail_read(
            &mut view,
            &self.cfg,
            &self.channel,
            &self.tail,
            self.fired.as_ref(),
            Some(&self.element.a),
        )
        .map_err(join_commit_error)?;
        self.reissue = Some(RowReissue::new(end));
        Ok(finish_read(step))
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.1): the end of
    /// Tail(actual) that the last tail read built.
    fn tail_reissue(&mut self) -> Option<RowReissue> { self.reissue.take() }
}

/// Added by decision 124 (plan v4 C7.9, ruling B-1, RIBA 2.3 and 2.6): the row search of a
/// VectorDB offer of the element `element` (its record, the source of its produce and its
/// persistence) to `waiter` over the order `order` of the staged view. `lead` marks a cascade
/// re-offer. `others` holds the stored members of the list of the fire, and the fire retires
/// those that are not persistent unless `waiter` peeks. After the check of owner funding, the
/// search decodes the payload and the vector of the record, copies its inputs and counts those
/// retires. The search keeps the state of the tail `at` at the fire, with whether the offer is a
/// cascade re-offer, and the copy of the operation, for its tail read and its re-issue. None
/// without owner funding (ruling D-10).
fn vdb_offer_search(
    cx: &mut Cx<'_, '_>,
    cfg: &VdbConfig,
    element: (&ListParWithRandom, &Produce, bool),
    waiter: &ParWaiter,
    order: &[usize],
    at: (&Tail, bool),
    others: &[ParDatum],
) -> Result<Option<ParRowSearch>, SpaceError> {
    match cx.introduction().owner_funded() {
        true => {}
        false => return Ok(None),
    }
    let (tail, lead) = at;
    let Some(operation) = cx.operation() else {
        return Err(SpaceError::Inconsistent(
            "an owner-funded plan keeps no operation",
        ));
    };
    let (record, source, persist) = element;
    let mut waiters = Vec::<(Consume, bool)>::with_capacity(order.len());
    for &index in order {
        let listed = waiter_at(cx, index)?;
        // Added by decision 124 (plan v2 C7.10, decision 100): an inert machinery waiter is no
        // alternative.
        match machinery_shape(listed) {
            false => {}
            true => continue,
        }
        waiters.push((listed.source.clone(), !listed.peeks.is_empty()));
    }
    let data = cx.data_channel();
    let random = RandomPurpose::Retire.derive(&Blake2b512Random::from_bytes(&record.random_state));
    let rows = RetireRows::of(cx, data, &random, record.cost_authority.as_ref())?;
    let staged_retires = match waiter.peeks.is_empty() {
        false => 0,
        true => others.iter().filter(|member| !member.persist).count(),
    };
    let payload = payload_of(record)?;
    let vector = record_vector(record, cfg)?;
    let search = VdbOfferSearch {
        channel: data.clone(),
        cfg: *cfg,
        waiters,
        staged: waiter.source.clone(),
        element: Datum {
            a: record.clone(),
            persist,
            source: source.clone(),
        },
        payload,
        vector,
        lead,
        theory: cx.theory(),
        alternatives: !cx.leases(),
        rows,
        staged_retires,
        found: None,
        tail: VdbTail {
            at: tail.at(lead),
            fired: cx.rounds().fired(),
            book: cx.book(),
            operation,
        },
        fired: None,
        reissue: None,
    };
    Ok(Some(Box::new(search)))
}

/// Added by decision 124 (plan v4 C7.9, errata V4-17, ruling B-1, RIBA 2.3): the row search of
/// a VectorDB receive `op` with the consume and the continuation `receive` and the query
/// `query`, over the fire `fire` of the staged view with the entries of its pick order. `fire`
/// is None for an E3-a park, which lists no candidate, binds no member and stages no retire.
/// The search copies its candidates and the member that the fire binds, and counts the retire
/// rows of the fire, after the check of owner funding. The search keeps the state of the tail
/// at the fire and the copy of the operation, for its tail read and its re-issue. None without
/// owner funding (ruling D-10).
fn vdb_receive_search(
    cx: &mut Cx<'_, '_>,
    cfg: &VdbConfig,
    op: &ConsumeOp,
    receive: (&Consume, &TaggedContinuation),
    query: Option<&VdbQuery>,
    fire: Option<(&ReceiveFire, &[Eligible])>,
    tail: &Tail,
) -> Result<Option<ParRowSearch>, SpaceError> {
    match cx.introduction().owner_funded() {
        true => {}
        false => return Ok(None),
    }
    let Some(operation) = cx.operation() else {
        return Err(SpaceError::Inconsistent(
            "an owner-funded plan keeps no operation",
        ));
    };
    let (source, continuation) = receive;
    let data = cx.data_channel();
    let (window, staged, staged_retires) = match fire {
        None => (ReceiveWindow::Ranked(Vec::new()), None, 0),
        Some((fire, entries)) => {
            let staged = fire
                .members
                .first()
                .map(|(record, _)| record.source.clone());
            let window = match query.map(|query| query.mode) {
                Some(Mode::List(_)) => {
                    let mut members = Vec::<Produce>::with_capacity(fire.members.len());
                    members.extend(fire.members.iter().map(|(record, _)| record.source.clone()));
                    ReceiveWindow::List(members, fire.matched.clone())
                }
                Some(Mode::Single) | None => {
                    let mut ranked = Vec::<(Produce, Option<Score>)>::with_capacity(entries.len());
                    for entry in entries {
                        let index = entry.index.ok_or(SpaceError::Inconsistent(
                            "a receive met an element in flight",
                        ))?;
                        let stored = stored_at(cx, data, index)?;
                        ranked.push((stored.source.clone(), entry.score.clone()));
                    }
                    ReceiveWindow::Ranked(ranked)
                }
            };
            let staged_retires = match op.peek {
                true => 0,
                false => fire
                    .members
                    .iter()
                    .skip(1)
                    .filter(|(record, _)| !record.persist)
                    .count(),
            };
            (window, staged, staged_retires)
        }
    };
    let random = RandomPurpose::Retire.derive(&Blake2b512Random::from_bytes(&op.body.random_state));
    let rows = RetireRows::of(cx, data, &random, record::stamped(&op.authority))?;
    let search = VdbReceiveSearch {
        channel: data.clone(),
        cfg: *cfg,
        params: op.query.clone(),
        window,
        staged,
        receive: (source.clone(), continuation.clone(), op.persistent),
        pattern: op.pattern.clone(),
        peek: op.peek,
        theory: cx.theory(),
        alternatives: !cx.leases(),
        rows,
        staged_retires,
        found: None,
        tail: VdbTail {
            at: tail.at(false),
            fired: cx.rounds().fired(),
            book: cx.book(),
            operation,
        },
        fired: None,
        reissue: None,
    };
    Ok(Some(Box::new(search)))
}

/// Stages the consume fire of the receive `op` over its members (decision 101, rule CR-13): one
/// bind over `[D]` of the first member, which carries the values of the bind. A receive that
/// does not peek then retires each other member that is not persistent with `K_retire`, in list
/// order. The retires derive their random state from the receive and carry its cost authority,
/// as the join engine retires for a consume. A peek binds the first member and retires nothing.
/// Changed by decision 101 (rule CR-13): the text before: "the group `[D; m]` with the pattern
/// of `op` on each channel, whose outcome dispatches the result of the first member only
/// (arbitration A11, the set's F3 shape). The first member carries the values of the bind, and
/// each other member carries its own payload, which the outcome never dispatches. A peek peeks
/// every member."
fn fire_members(
    cx: &mut Cx<'_, '_>,
    op: &ConsumeOp,
    continuation: TaggedContinuation,
    fire: ReceiveFire,
    // Added by decision 124 (plan v4 C7.9, rulings B-1 and OP-R): the fire takes its search.
    search: Option<ParRowSearch>,
) -> Result<(), SpaceError> {
    let data = cx.data_channel();
    // Changed by decision 101 (rule CR-13): one bind over `[D]`, then the retires.
    // let width = fire.members.len();
    // let peeks = match op.peek {
    //     true => peek_set_all(width)?,
    //     false => peek_set(false),
    // };
    // let mut channels = Vec::<Par>::with_capacity(width);
    // let mut patterns = Vec::<BindPattern>::with_capacity(width);
    // let mut candidates = Vec::<ParCandidate>::with_capacity(width);
    // let mut first = Some(fire.matched);
    // for (record, index) in fire.members {
    //     let matched = match first.take() {
    //         Some(matched) => matched,
    //         None => payload_of(&record.a)?,
    //     };
    //     channels.push(data.clone());
    //     patterns.push(op.pattern.clone());
    //     candidates.push(stored_candidate(data.clone(), Member {
    //         record,
    //         matched,
    //         view_index: view_index(index)?,
    //     }));
    // }
    // cx.fire_receive_group(
    //     channels,
    //     patterns,
    //     continuation,
    //     op.persistent,
    //     peeks,
    //     candidates,
    //     1,
    // )
    let ReceiveFire { matched, members } = fire;
    let mut members = members.into_iter();
    let (record, index) = members
        .next()
        .ok_or(SpaceError::Inconsistent("a receive fire has no member"))?;
    // Changed by decision 124 (plan v4 C7.9, rulings B-1 and OP-R): the one-bind fire takes the
    // receive search as the sixth input of `fire_receive`, which stages the same group. The text
    // before:
    // ```
    // let mut channels = Vec::<Par>::with_capacity(1);
    // channels.push(data.clone());
    // let mut patterns = Vec::<BindPattern>::with_capacity(1);
    // patterns.push(op.pattern.clone());
    // let mut candidates = Vec::<ParCandidate>::with_capacity(1);
    // candidates.push(stored_candidate(data.clone(), Member {
    //     record,
    //     matched,
    //     view_index: view_index(index)?,
    // }));
    // cx.fire_receive_group(
    //     channels,
    //     patterns,
    //     continuation,
    //     op.persistent,
    //     peek_set(op.peek),
    //     candidates,
    //     1,
    // )?;
    // ```
    cx.fire_receive(
        op,
        StoredFire {
            head: record,
            matched,
            view_index: view_index(index)?,
        },
        continuation,
        peek_set(op.peek),
        search,
    )?;
    match op.peek {
        true => Ok(()),
        false => {
            let mut others = Vec::<ParDatum>::with_capacity(members.len());
            others.extend(members.map(|(record, _)| record));
            let random = Blake2b512Random::from_bytes(&op.body.random_state);
            retire_members(cx, data, others, &random, record::stamped(&op.authority))?;
            Ok(())
        }
    }
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
        // Changed by decision 124 (plan v4 C7.7): only a finish gives the cause of an owner
        // refusal, never the own element of a plan, so it runs the tail as a divergent pair does.
        // Some(ReissueCause::DivergentPair) | None => run_tail(cx, cfg, tail, origin),
        Some(ReissueCause::DivergentPair | ReissueCause::OwnerRefusal) | None => {
            // Added by decision 124 (plan v4 C7.11, ruling RADD2 1.1): the searches of the tail
            // read the end of the element, for the re-issue of their rows.
            tail.own = own;
            run_tail(cx, cfg, tail, origin)
        }
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
    // Added by decision 124 (plan v4 C7.11, ruling RADD 1.2): the second sends and the cascade of
    // the tail run from here, so a search that a fire builds after this line reads the drain.
    tail.draining = true;
    loop {
        if let Some(record) = tail.work.pop() {
            if cx.rounds().fired() >= FIRE_ROUND_CAP {
                tail.owe(record);
                return Ok(Some(tail.stop(ReissueCause::RoundCap)));
            }
            let kept = record.clone();
            let element = Element::second_send(cx, cfg, record)?;
            if let OfferEnd::Reissue(cause) = offer_vdb(cx, cfg, element, tail)? {
                tail.owe(kept);
                return Ok(Some(tail.stop(cause)));
            }
            continue;
        }
        if !tail.removed {
            return Ok(None);
        }
        // Changed by decision 124 (plan v4 C7.9 and C7.11, rulings B-1 and RADD 1.2): the pick
        // hands out its pick order, which the row search of the fire lists, and it reads through
        // the view, so the data channel is an input. The text before:
        // let Some(target) = first_cascade_target(cx, cfg)? else {
        let data = cx.data_channel();
        let Some((target, order)) = first_cascade_target(cx, cfg, data)? else {
            tail.removed = false;
            return Ok(None);
        };
        if cx.rounds().fired() >= FIRE_ROUND_CAP {
            return Ok(Some(tail.stop(ReissueCause::RoundCap)));
        }
        // Added by decision 124 (L6, CR30 D-b): the fire takes one lease for each stored member
        // of its list (VJ V11): after the cap check.
        cx.lease_stored(target.members.len());
        // Changed by decision 124 (plan v4 C7.9, ruling B-1, RIBA 2.6): the re-offer builds its
        // row search with the config and the order of the cascade pick. The text before:
        // if let FireStep::Reissue(cause) = lift_and_reoffer(cx, target, origin, tail)? {
        if let FireStep::Reissue(cause) = lift_and_reoffer(cx, cfg, &order, target, origin, tail)? {
            return Ok(Some(tail.stop(cause)));
        }
    }
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD 1.2 and RADD2 1.1, ADD A2.8):
/// the engine part of the tail read of a VectorDB row over the store `view`, with the data
/// channel `data`, from the state `tail` at c0's fire, the fire `fire` of the unit (`None` for a
/// store or a park) and the record `element` of an offer, with the end of Tail(actual) that the
/// plan gives (`vdb_end`). The fire counts one round, owes its second send and runs the rule of
/// its phase (`vdb_site`). Then the rest of `run_tail` (`vdb_drain`). It stages nothing.
fn vdb_tail_read<V: StoreView>(
    view: &mut V,
    cfg: &VdbConfig,
    data: &Par,
    tail: &VdbTail,
    fire: Option<&VdbFire>,
    element: Option<&ListParWithRandom>,
) -> Result<(TailStep, TailEnd), SpaceError> {
    let mut state = Tail::new();
    state.work = tail.at.work.clone();
    state.removed = tail.at.removed;
    let mut own = tail.at.own;
    let mut fired = tail.fired;
    let site = match fire {
        None => None,
        Some(fire) => {
            let mut rounds = FireRounds::at(fired);
            let step = rounds.after_fire(fire.spent, fire.stays);
            fired = rounds.fired();
            match &fire.owed {
                None => {}
                Some(record) => state.owe(record.clone()),
            }
            state.removed |= fire.removes;
            vdb_site(tail.at.phase, step, &mut state, &mut own, element)?
        }
    };
    let (step, stop) = match site {
        Some(end) => end,
        None => vdb_drain(view, cfg, data, &state, fired)?,
    };
    let end = vdb_end(&tail.operation, state, step, stop, own)?;
    Ok((step, end))
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.1, S8-MIRROR): the rest of
/// the phase `phase` of a VectorDB fire after its step `step`, with the tail `state` and the end
/// `own` of the element of the operation, as the plans, `run_tail` and `lift_and_reoffer` run
/// it: the engine part of the read and the cause of the stop of the tail,
/// or `None` when the tail runs on. A fire of the plan that goes on keeps the element in flight,
/// so at the stop of C2 E4.5 the operation re-issues. A fire of
/// the plan at the cap stops the tail only when it owes work or a cascade (`finish_tail`).
/// An offer of the drain that goes on or re-issues owes its record `element`
/// again. A cascade that re-issues stops.
fn vdb_site(
    phase: VdbPhase,
    step: FireStep,
    state: &mut Tail,
    own: &mut Option<ReissueCause>,
    element: Option<&ListParWithRandom>,
) -> Result<Option<(TailStep, Option<ReissueCause>)>, SpaceError> {
    match (phase, step) {
        (VdbPhase::Plan, FireStep::Continue) => {
            *own = Some(ReissueCause::OwnerRefusal);
            Ok(Some((TailStep::Stages, None)))
        }
        (VdbPhase::Plan, FireStep::Done) => {
            *own = None;
            Ok(None)
        }
        (VdbPhase::Plan, FireStep::Reissue(ReissueCause::RoundCap)) => {
            *own = Some(ReissueCause::RoundCap);
            let stop = match state.work.is_empty() && !state.removed {
                true => None,
                false => Some(ReissueCause::RoundCap),
            };
            Ok(Some((TailStep::Capped, stop)))
        }
        (VdbPhase::Plan, FireStep::Reissue(cause)) => {
            *own = Some(cause);
            Ok(None)
        }
        (VdbPhase::Drain, FireStep::Done)
        | (VdbPhase::Cascade, FireStep::Done | FireStep::Continue) => Ok(None),
        (VdbPhase::Drain, FireStep::Continue | FireStep::Reissue(_)) => {
            let Some(record) = element else {
                return Err(SpaceError::Inconsistent(
                    "an offer of the drain has no record",
                ));
            };
            state.owe(record.clone());
            Ok(Some(match step {
                FireStep::Reissue(ReissueCause::RoundCap) => {
                    (TailStep::Capped, Some(ReissueCause::RoundCap))
                }
                FireStep::Reissue(cause) => (TailStep::Commits, Some(cause)),
                FireStep::Done | FireStep::Continue => (TailStep::Stages, None),
            }))
        }
        (VdbPhase::Cascade, FireStep::Reissue(ReissueCause::RoundCap)) => {
            Ok(Some((TailStep::Capped, Some(ReissueCause::RoundCap))))
        }
        (VdbPhase::Cascade, FireStep::Reissue(cause)) => Ok(Some((TailStep::Commits, Some(cause)))),
    }
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD 1.2 and RADD2 1.1): the rest
/// of `run_tail` after `fired` fires with the tail `state`: owed work is a
/// step, a removal with a cascade target is a step, the cap stops the tail before a step with
/// `RoundCap`, and otherwise the plan concludes with no stop.
fn vdb_drain<V: StoreView>(
    view: &mut V,
    cfg: &VdbConfig,
    data: &Par,
    state: &Tail,
    fired: usize,
) -> Result<(TailStep, Option<ReissueCause>), SpaceError> {
    let owed = match (state.work.is_empty(), state.removed) {
        (true, false) => false,
        (false, _) => true,
        (true, true) => first_cascade_target(view, cfg, data)?.is_some(),
    };
    match (owed, round_cap(fired)) {
        (false, _) => Ok((TailStep::Commits, None)),
        (true, TailStep::Capped) => Ok((TailStep::Capped, Some(ReissueCause::RoundCap))),
        (true, step) => Ok((step, None)),
    }
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.1, C2 E4.5): the end of
/// Tail(actual) of a VectorDB row of the operation `operation` at the end `step` of its read,
/// with the tail `state`, the cause `stop` of its stop and the end `own` of the element, by the
/// re-issue of the plan (`conclude`). `Commits` and `Capped` return that re-issue. `Stages`
/// stops the operation for an owner refusal with the re-issue of the rest: the tail stops with
/// `OwnerRefusal` when it owes work or a cascade, as a plan at the cap stops (`finish_tail`).
/// A `Stages` without a re-issue fails closed.
fn vdb_end(
    operation: &SpaceOp,
    mut state: Tail,
    step: TailStep,
    stop: Option<ReissueCause>,
    own: Option<ReissueCause>,
) -> Result<TailEnd, SpaceError> {
    let stop = match step {
        TailStep::Commits | TailStep::Capped => stop,
        TailStep::Stages => match state.work.is_empty() && !state.removed {
            true => None,
            false => Some(ReissueCause::OwnerRefusal),
        },
    };
    let settle = match (stop, operation) {
        (None, _) => None,
        (Some(_), SpaceOp::Produce(produce)) => Some(settle_operation(OpView::Produce(produce))),
        (Some(_), SpaceOp::Consume(consume)) => Some(settle_operation(OpView::Consume(consume))),
        (Some(_), SpaceOp::Settle(_) | SpaceOp::Join(_)) => {
            return Err(SpaceError::Inconsistent(
                "a VectorDB plan runs a produce or a consume",
            ))
        }
    };
    let stop = stop.map(|cause| state.stop(cause));
    let resume = own.map(|cause| (reissued_without_order(operation.clone()), cause));
    let reissue = conclude(settle, stop, resume);
    match (step, reissue) {
        (TailStep::Commits | TailStep::Capped, reissue) => Ok(TailEnd::Return(reissue)),
        (TailStep::Stages, Some(reissue)) => Ok(TailEnd::Stop(reissue.op)),
        (TailStep::Stages, None) => Err(SpaceError::Inconsistent(
            "a VectorDB row owes a step that no re-issue carries",
        )),
    }
}

/// The re-issue at the end of a plan. Without a stop, the operation re-issues when its element
/// is still in flight (`resume`). With a stop, one settle job carries the owed second sends and
/// the re-issue of the operation (arbitration A7). `settle` holds the fields of the settle job
/// (`settle::settle_operation`).
fn conclude(
    // Retired by decision 124 (plan v4 C7.11, ruling RADD2 1.1): the re-issue reads no `cx`, so
    // the tail read of a VectorDB row runs it too (`vdb_end`). The text before:
    // cx: &mut Cx<'_, '_>,
    settle: Option<SettleOp>,
    stop: Option<Stop>,
    resume: Option<(SpaceOp, ReissueCause)>,
) -> Option<SpaceReissue> {
    // Changed by decision 124 (plan v4 C7.11, ruling RADD2 1.1): the value of `Cx::reissue`,
    // built without `cx`. The text before:
    // let resume = resume.map(|(op, cause)| cx.reissue(op, cause));
    let resume = resume.map(|(op, cause)| SpaceReissue { op, cause });
    let (Some(mut settle), Some(stop)) = (settle, stop) else {
        return resume;
    };
    let mut pending = Vec::<PendingProduce>::with_capacity(stop.pending.len());
    for record in stop.pending {
        pending.push(PendingProduce {
            key: settle.key.clone(),
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
    // Changed by decision 124 (plan v4 C7.11, ruling RADD2 1.1): the value of `Cx::reissue`,
    // built without `cx`. The text before:
    // Some(cx.reissue(SpaceOp::Settle(settle), stop.cause))
    Some(SpaceReissue {
        op: SpaceOp::Settle(settle),
        cause: stop.cause,
    })
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
// Changed by decision 124 (plan v4 C7.11, ruling RADD 1.2): the reads of this fn go through
// the view `view` in place of `cx`. The comment above a changed line keeps its text before.
// fn first_cascade_target(
fn first_cascade_target<V: StoreView>(
    // cx: &mut Cx<'_, '_>,
    view: &mut V,
    cfg: &VdbConfig,
    // Added by decision 124 (plan v4 C7.11, ruling RADD 1.2): the data channel of the collection,
    // which no read of the view borrows.
    data: &Par,
    // Changed by decision 124 (plan v4 C7.9, ruling B-1): the pick hands out its pick order, which
    // the row search of the fire lists. The text before:
    // ) -> Result<Option<CascadeTarget>, SpaceError> {
) -> Result<Option<(CascadeTarget, Vec<usize>)>, SpaceError> {
    // Changed by decision 124 (plan v4 C7.11, ruling RADD 1.2): the caller gives the data channel
    // `data`. The text before:
    // let data = cx.data_channel();
    // Added by decision 102 (plan v4 C5.7): kept DR-75, because the cascade keeps DR-75 in both
    // roles (F3, decision 124 L1).
    // let order = cx.canonical_waiter_order(data)?;
    let order = canonical_continuation_order(view.waiters_on(data)?);
    for &index in &order {
        let (pattern, policy) = {
            // let waiter = waiter_at(cx, index)?;
            let waiter = waiter_at(view, index)?;
            // Added by decision 124 (plan v2 C7.10, decision 100): an inert machinery waiter is
            // no candidate.
            match machinery_shape(waiter) {
                false => {}
                true => continue,
            }
            let pattern = waiter_pattern(waiter)?.clone();
            let (_, policy) = record::continuation_policy_slots(&waiter.continuation)?;
            (pattern, policy)
        };
        let Some(query) = policy_query(cfg, &policy)? else {
            continue;
        };
        let Mode::List(k) = query.mode else {
            continue;
        };
        // let Some(fire) = list_selection(cx, cfg, &query, k, &pattern, data, None)? else {
        let Some(fire) = list_selection(view, cfg, &query, k, &pattern, data, None)? else {
            continue;
        };
        // let waiter = waiter_at(cx, index)?.clone();
        let waiter = waiter_at(view, index)?.clone();
        // Added by decision 124 (plan v4 C7.8, rule CR-20, ruling RAMD 1.5): a list with a member
        // whose pair the book holds does not fire again.
        let booked = view.list_booked(
            None,
            fire.members.iter().flatten().map(|(record, _)| record),
            &waiter.source,
        );
        match booked {
            false => {}
            true => continue,
        }
        // if !cx.admits_records(
        if !view.admits_records(
            &waiter.continuation,
            &fire.matched,
            &fire.members,
            |member| member.as_ref().map(|(record, _)| &record.a),
            payload_of,
        )? {
            continue;
        }
        let mut members = Vec::<ParDatum>::with_capacity(fire.members.len());
        for member in fire.members {
            let (record, _) = member.ok_or(SpaceError::Inconsistent(
                "a cascade list holds an element in flight",
            ))?;
            members.push(record);
        }
        // Changed by decision 124 (plan v4 C7.9, ruling B-1): the pick hands out its pick order,
        // which the row search of the fire lists. The text before:
        // ```
        // return Ok(Some(CascadeTarget {
        //     waiter,
        //     matched: fire.matched,
        //     members,
        // }));
        // ```
        return Ok(Some((
            CascadeTarget {
                waiter,
                matched: fire.matched,
                members,
            },
            order,
        )));
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
    // Added by decision 124 (plan v4 C7.9, ruling B-1, RIBA 2.6): the config of the space and the
    // order of the cascade pick, over which the re-offer builds its row search.
    cfg: &VdbConfig,
    order: &[usize],
    target: CascadeTarget,
    origin: &Origin,
    tail: &mut Tail,
) -> Result<FireStep, SpaceError> {
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
        // Added by decision 124 (plan v2 C7.5, F5, rule CR-16, ruling RAMD 1.6): the lift records
        // the pair of the lifted record and the waiter of the re-offer, for the book of a refused
        // lift (CR-20).
        cx.record_lift((lead.source.hash.clone(), waiter.source.hash.clone()));
        // Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2v2 6.1): the lift row
        // takes the class of the lift of a cascade, with the end of the element of the operation,
        // so E4.5 re-issues a refused lift as the settle job of the cascade (`conclude`).
        cx.cascade_lift(tail.own)?;
    }
    let introduction = cx.introduction().for_record(&lead.a)?;
    let spent = !lead.persist && !peeks;
    let stays = waiter.persist;
    // Changed by decision 124 (plan v4 C7.9, ruling B-1, RIBA 2.6): the fire carries the row
    // search of the cascade re-offer, which the two lines after the literal set. The text before:
    // let fire = OfferFire {
    let mut fire = OfferFire {
        persistent: lead.persist,
        waiter,
        matched,
        // Added by decision 102 (plan v4 C7.22, rule CR-19 case 3): a persistent lead is never
        // lifted. It stays stored, and its re-offer is not the job's own.
        offered: match lifted {
            true => Offered::InFlight,
            false => Offered::StoredPersistent,
        },
        search: None,
    };
    let datum = (&lead.a, &lead.source, lead.persist);
    fire.search = vdb_offer_search(
        cx,
        cfg,
        datum,
        &fire.waiter,
        order,
        (&*tail, true),
        &members,
    )?;
    let step = cx.rounds().after_fire(spent, stays);
    let staged = lead.a.clone();
    let removed = lead.a.clone();
    let source = lead.source.clone();
    cx.fire_offer_on(data, fire, staged, removed, source, &introduction)?;
    tail.removed = true;
    match (peeks, lifted) {
        (true, true) => tail.owe(lead.a),
        (true, false) => {}
        (false, _) => {
            // Changed by decision 101 (rule CR-13): the random state and the authority of the
            // lifted record are arguments.
            // retire_members(cx, data, members, &lead.a)?;
            let random = Blake2b512Random::from_bytes(&lead.a.random_state);
            retire_members(cx, data, members, &random, lead.a.cost_authority.as_ref())?;
        }
    }
    Ok(step)
}

/// Added by decision 124 (plan v4 C7.11, ruling RADD 1.2): the unit tests of the VectorDB plan.
#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicBool, Ordering};

    use models::rhoapi::tagged_continuation::TaggedCont;
    use models::rhoapi::ParWithRandom;
    use models::rust::space_factory_urn::{SpaceFactoryUrn, SpaceInner, SpaceOuter};
    use models::rust::utils::{new_freevar_par, new_freevar_var, new_gint_par, new_gstring_par};
    use rspace_plus_plus::rspace::rspace::RSpace;
    use rspace_plus_plus::rspace::rspace_interface::ISpace;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    use rspace_plus_plus::rspace::space_store_handle::{RowItemTest, SpaceStoreHandle};

    use super::*;
    use crate::rust::interpreter::accounting::costs::Cost;
    use crate::rust::interpreter::accounting::RuntimeBudget;
    use crate::rust::interpreter::matcher::r#match::Matcher;
    use crate::rust::interpreter::spaces::agent::{run_op, SpaceAgent, SpaceCreation, SpaceEnv};
    use crate::rust::interpreter::spaces::families::FamilyAgent;
    use crate::rust::interpreter::spaces::generic::StoredMatch;
    // Added by decision 124 (rules CR-4 and R13): the capture of a staged search.
    use crate::rust::interpreter::spaces::generic::{
        begin_collection, prepare_collection_op, GenericSpace,
    };
    use crate::rust::interpreter::spaces::namespace;
    // Added by decision 124 (rules CR-4 and R13): the capture of a staged search.
    use crate::rust::interpreter::spaces::outer::hashmap::HashMapOuter;
    // Added by decision 124 (rules CR-4 and R13): the capture of a staged search and the lists
    // of a recorded refusal.
    use crate::rust::interpreter::spaces::test_support::{
        capturing, with_named_lists, NamedRecords,
    };
    use crate::rust::interpreter::spaces::test_support::{owner_funded, Funded};
    use crate::rust::interpreter::spaces::theory::identity::TheoryIdentity;
    use crate::rust::interpreter::spaces::theory::test_theories::{self, TestBehavior};
    use crate::rust::interpreter::spaces::types::qualifier::SpaceQualifier;

    /// T-S8-VDB-PHASE: the phase of a fire follows the tail, and the state of the tail is read at
    /// the fire.
    #[test]
    fn tail_at_names_the_phase_of_each_fire() {
        let mut tail = Tail::new();
        assert_eq!(tail.at(false).phase, VdbPhase::Plan);
        assert_eq!(tail.at(true).phase, VdbPhase::Cascade);
        tail.owe(ListParWithRandom::default());
        tail.removed = true;
        tail.draining = true;
        let at = tail.at(false);
        assert_eq!(at.phase, VdbPhase::Drain);
        assert_eq!((at.work.len(), at.removed), (1, true));
        assert_eq!(tail.at(true).phase, VdbPhase::Cascade);
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2v2 6.1): S8-MIRROR of the
    /// refused lift of a cascade. At a lift the tail owes no send and a record left the
    /// collection, so `vdb_end` stops with the settle job of the cascade, with the operation again
    /// without its order when the element met a divergent pair. E4.5 builds the same value from
    /// the class of the lift (`tail_end_gives_a_cascade_lift_the_settle_of_its_cascade`).
    #[test]
    fn vdb_end_of_a_refused_cascade_lift_is_its_tail_end() {
        use crate::rust::interpreter::spaces::namespace::{
            CollectionKey, Durability, NativeSpace, SID_LEN,
        };
        let produce = ProduceOp {
            space: NativeSpace {
                sid: [7; SID_LEN],
                durability: Durability::Durable,
            },
            key: CollectionKey::from_bytes(vec![7]).expect("a key that is not empty"),
            channel: Par::default(),
            data: ListParWithRandom::default(),
            persistent: true,
            params: BTreeMap::new(),
            authority: CostAuthority::default(),
            reissued_ord: Some(4),
        };
        let operation = SpaceOp::Produce(produce.clone());
        let unordered = SpaceOp::Produce(ProduceOp {
            reissued_ord: None,
            ..produce.clone()
        });
        let settle = settle_operation(OpView::Produce(&produce));
        for own in [None, Some(ReissueCause::DivergentPair)] {
            let mut tail = Tail::new();
            tail.removed = true;
            tail.own = own;
            let resume = own.map(|cause| {
                Box::new(SpaceReissue {
                    op: unordered.clone(),
                    cause,
                })
            });
            let expected = SpaceOp::Settle(SettleOp {
                cascade: Some(Box::new(CascadeState {
                    seeds: Vec::new(),
                    pending: Vec::new(),
                    resume,
                })),
                ..settle.clone()
            });
            let end = vdb_end(&operation, tail, TailStep::Stages, None, own)
                .expect("a refused cascade lift ends");
            assert_eq!(end, TailEnd::Stop(expected), "the end {own:?}");
        }
    }

    /// Added by decision 124 (plan v4 C7.9, ruling RADD 1.7): a number as the value of a send.
    fn int(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

    /// Added by decision 124 (plan v4 C7.9, ruling RADD 1.7): the channel of the collection.
    fn channel() -> Par { int(100) }

    /// Added by decision 124 (plan v4 C7.9, ruling RADD 1.7): the payload of a send of `value`.
    fn payload(value: i64) -> ListParWithRandom {
        ListParWithRandom {
            pars: vec![int(value)],
            ..Default::default()
        }
    }

    /// Added by decision 124 (plan v4 C7.9, ruling RADD 1.7): the record of `payload` as the
    /// collection stores it, with its source.
    fn stored(payload: &ListParWithRandom) -> ParDatum {
        let record = record::wrap_vdb_datum(0, None, payload).expect("a VectorDB record");
        ParDatum {
            source: Produce::create(&channel(), &record, false),
            a: record,
            persist: false,
        }
    }

    /// Added by decision 124 (plan v4 C7.9, ruling RADD 1.7, C4v2 B.4): a list keeps the place of
    /// the element in flight. `list_pairs` over a list with the element at position 1 gives the
    /// pairs in list order, with the element at position 1, and `into_stored` gives the stored
    /// members in order, without the element.
    #[test]
    fn list_offer_keeps_the_place_of_the_element() {
        let (first_payload, element_payload, last_payload) = (payload(1), payload(2), payload(3));
        let (first, last) = (stored(&first_payload), stored(&last_payload));
        let element = stored(&element_payload);
        let members = vec![Some((first.clone(), 0)), None, Some((last.clone(), 1))];
        let pairs = list_pairs(&members, Some((&element, &element_payload)))
            .expect("the pairs of a list with the element in flight");
        assert_eq!(
            pairs,
            vec![
                (first.clone(), first_payload),
                (element, element_payload),
                (last.clone(), last_payload),
            ],
            "the pairs follow the list, and the element keeps its position",
        );
        let fire = ListFire {
            matched: ListParWithRandom::default(),
            members,
        };
        assert_eq!(
            fire.into_stored().1,
            vec![first, last],
            "the stored members in order, without the element",
        );
    }

    /// Added by decision 124 (plan v4 C7.9, rulings RADD 1.7 and RADD2 1.7): an item rule that
    /// records each value that it tests and refuses the value `refused`.
    struct RecordingRule {
        refused: ListParWithRandom,
        tested: RefCell<Vec<ListParWithRandom>>,
    }

    impl RowItemRule<TaggedContinuation, ListParWithRandom> for RecordingRule {
        fn admit_item(
            &self,
            _continuation: &TaggedContinuation,
            _continuation_persistent: bool,
            _channel_count: usize,
            datum: &ListParWithRandom,
            _datum_persistent: bool,
        ) -> Result<bool, SpaceTxnError> {
            self.tested.borrow_mut().push(datum.clone());
            Ok(*datum != self.refused)
        }
    }

    /// Added by decision 124 (plan v4 C7.9, rulings RADD 1.7 and RADD2 1.7, C4v2 B.4): the item
    /// test of a list whose first member is stored and whose element is at position 1. The first
    /// member takes the values of the bind, so a rule that refuses them fails at that member and
    /// tests nothing more, and a rule that refuses only the payload of that member passes. The
    /// element at position 1 takes its payload. `VdbOfferSearch::tuple` tests its list with
    /// `list_pairs` and then `members_admit`. Its `RowLists` come only from the commit of the
    /// store, so the test drives those two calls.
    #[test]
    fn vdb_offer_search_tests_the_first_member_with_the_bind() {
        let (first_payload, element_payload) = (payload(1), payload(2));
        let element = stored(&element_payload);
        let members = vec![Some((stored(&first_payload), 0)), None];
        let pairs = list_pairs(&members, Some((&element, &element_payload)))
            .expect("the pairs of a list whose first member is stored");
        let matched = payload(9);
        let continuation = TaggedContinuation::default();
        let source = Consume::create(
            &vec![channel()],
            &vec![BindPattern::default()],
            &continuation,
            false,
        );
        let matcher = Matcher;
        let both = vec![matched.clone(), element_payload.clone()];
        let cases = [
            (matched.clone(), false, vec![matched.clone()]),
            (element_payload.clone(), false, both.clone()),
            (first_payload.clone(), true, both),
        ];
        for (refused, admits, tested) in cases {
            let rule = RecordingRule {
                refused,
                tested: RefCell::new(Vec::with_capacity(2)),
            };
            let request = ParRowRequest {
                matcher: &matcher,
                group: &[],
                patterns: &[],
                continuation: &continuation,
                persistent: false,
                source: &source,
                own: None,
                item: Some(RowItemTest {
                    rule: &rule,
                    continuation: &continuation,
                    persistent: false,
                    channel_count: 1,
                }),
            };
            let verdict =
                members_admit(&request, &matched, &pairs).expect("the item test of the list");
            assert_eq!(verdict, admits, "the verdict of the item test");
            assert_eq!(
                rule.tested.into_inner(),
                tested,
                "the values that it tested"
            );
        }
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.2): the config of the
    /// collection of the tests of the tail.
    const CONFIG: VdbConfig = VdbConfig {
        capacity: 8,
        dimensions: 1,
        metric: Metric::Cosine,
    };

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.2): a read of the view that
    /// the cascade pick does not make.
    fn unread<T>() -> Result<T, SpaceError> { Err(SpaceError::Inconsistent("an unexpected read")) }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.2): a view of one collection
    /// with the waiters `waiters` and the records `records`. Each bind matches, each guard admits
    /// and the book is empty, so the cascade pick reads a target when a list waiter has a record.
    struct CollectionView {
        waiters: Vec<ParWaiter>,
        records: Vec<ParDatum>,
    }

    impl StoreView for CollectionView {
        fn data_on(&mut self, _channel: &Par) -> Result<&[ParDatum], SpaceError> {
            Ok(self.records.as_slice())
        }

        fn waiters_on(&mut self, _channel: &Par) -> Result<&[ParWaiter], SpaceError> {
            Ok(self.waiters.as_slice())
        }

        fn joins_on(&mut self, _channel: &Par) -> Result<&[Vec<Par>], SpaceError> { unread() }

        fn group_waiters(&mut self, _group: &[Par]) -> Result<&[ParWaiter], SpaceError> { unread() }

        fn canonical_data_order(&mut self, _channel: &Par) -> Result<Vec<usize>, SpaceError> {
            Ok((0..self.records.len()).collect())
        }

        fn pick_data_order(
            &mut self,
            _channel: &Par,
            _source: RSpaceOperationSource<'_>,
        ) -> Result<Vec<usize>, SpaceError> {
            unread()
        }

        fn pick_data_order_with(
            &mut self,
            _channel: &Par,
            _extra: &ParDatum,
            _source: RSpaceOperationSource<'_>,
        ) -> Result<Vec<usize>, SpaceError> {
            unread()
        }

        fn bind(
            &self,
            _pattern: &BindPattern,
            data: &ListParWithRandom,
        ) -> Option<ListParWithRandom> {
            Some(data.clone())
        }

        fn bind_item(
            &self,
            _pattern: &BindPattern,
            _data: &ListParWithRandom,
            _persist: bool,
        ) -> Result<Option<ListParWithRandom>, SpaceError> {
            unread()
        }

        fn has_theory(&self) -> bool { false }

        fn tests_items(&self) -> bool { false }

        fn admits_all(
            &mut self,
            _continuation: &TaggedContinuation,
            _matched: &[ListParWithRandom],
            _stored: &[StoredMatch<'_>],
        ) -> Result<bool, SpaceError> {
            unread()
        }

        fn admits_records<T>(
            &mut self,
            _continuation: &TaggedContinuation,
            _matched: &ListParWithRandom,
            _members: &[T],
            _stored: impl Fn(&T) -> Option<&ListParWithRandom>,
            _payload_of: fn(&ListParWithRandom) -> Result<ListParWithRandom, SpaceError>,
        ) -> Result<bool, SpaceError> {
            Ok(true)
        }

        fn theory_admits(&self, _stored: &[StoredMatch<'_>]) -> Result<bool, SpaceError> {
            unread()
        }

        fn booked(&self, _record: &Produce, _waiter: &Consume) -> bool { false }

        fn data_channel(&self) -> Result<&Par, SpaceError> { unread() }

        fn data(&mut self) -> Result<&[ParDatum], SpaceError> { unread() }

        fn waiters(&mut self) -> Result<&[ParWaiter], SpaceError> { Ok(self.waiters.as_slice()) }
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.2): a parked waiter of a
    /// list of one record, which the cascade of a collection with a record fires.
    fn parked_list_waiter() -> ParWaiter {
        let policy = WaiterPolicy::Query(Box::new(BTreeMap::from([(K_KEY.to_string(), int(1))])));
        let body = TaggedContinuation {
            tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                body: Some(Par::default()),
                random_state: Vec::new(),
            })),
            ..Default::default()
        };
        let continuation =
            record::wrap_policy_continuation(0, &policy, &body).expect("a parked continuation");
        let patterns = vec![BindPattern::default()];
        ParWaiter {
            source: Consume::create(&vec![channel()], &patterns, &continuation, false),
            patterns,
            continuation,
            persist: false,
            peeks: BTreeSet::new(),
        }
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.1): a tail that owes one
    /// second send when `owes` and from which a record left the collection when `removed`.
    fn tail_of(owes: bool, removed: bool) -> Tail {
        let mut tail = Tail::new();
        match owes {
            false => {}
            true => tail.owe(payload(8)),
        }
        tail.removed = removed;
        tail
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.1): the answer of
    /// `vdb_site`: the step and the stop, or `None` when the tail runs on.
    type SiteAnswer = Option<(TailStep, Option<ReissueCause>)>;

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.1): `vdb_site` of a fire
    /// with its record over the tail `(owes, removed)` and the end `before` of the element: its
    /// answer, the end of the element after it, and the second sends that the tail owes after it.
    fn site_of(
        phase: VdbPhase,
        step: FireStep,
        (owes, removed): (bool, bool),
        before: Option<ReissueCause>,
    ) -> (SiteAnswer, Option<ReissueCause>, usize) {
        let mut tail = tail_of(owes, removed);
        let mut own = before;
        let record = payload(9);
        let answer = vdb_site(phase, step, &mut tail, &mut own, Some(&record))
            .expect("a fire with its record has a rule");
        (answer, own, tail.work.len())
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.1): `vdb_drain` over
    /// `view` after `fired` fires, with the tail `(owes, removed)`.
    fn drain_of(
        view: &mut CollectionView,
        (owes, removed): (bool, bool),
        fired: usize,
    ) -> (TailStep, Option<ReissueCause>) {
        vdb_drain(view, &CONFIG, &channel(), &tail_of(owes, removed), fired)
            .expect("the cascade pick reads its view")
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.1): a persistent produce
    /// with an order, which a plan re-issues without it.
    fn ordered_produce() -> ProduceOp {
        use crate::rust::interpreter::spaces::namespace::{
            CollectionKey, Durability, NativeSpace, SID_LEN,
        };
        ProduceOp {
            space: NativeSpace {
                sid: [7; SID_LEN],
                durability: Durability::Durable,
            },
            key: CollectionKey::from_bytes(vec![7]).expect("a key that is not empty"),
            channel: Par::default(),
            data: ListParWithRandom::default(),
            persistent: true,
            params: BTreeMap::new(),
            authority: CostAuthority::default(),
            reissued_ord: Some(4),
        }
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.1): the settle job of
    /// `settle` that owes the second sends `pending` and re-issues `resume` after them.
    fn settle_job(
        settle: &SettleOp,
        pending: Vec<PendingProduce>,
        resume: Option<SpaceReissue>,
    ) -> SpaceOp {
        SpaceOp::Settle(SettleOp {
            cascade: Some(Box::new(CascadeState {
                seeds: Vec::new(),
                pending,
                resume: resume.map(Box::new),
            })),
            ..settle.clone()
        })
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.1): T-S8-REISSUE. Each arm
    /// of `vdb_site` and `vdb_drain` gives the step and the stop of the rule of its phase. Then
    /// `vdb_end` gives the re-issue of `conclude`: `Commits` and `Capped` return it, and `Stages`
    /// stops with the settle job when the tail owes work or a cascade, and else with the
    /// operation again. `Stages` with nothing to re-issue fails closed.
    #[test]
    fn vdb_end_concludes_the_tail() {
        use FireStep::{Continue, Done, Reissue};
        use VdbPhase::{Cascade, Drain, Plan};
        let (cap, pair, refusal) = (
            ReissueCause::RoundCap,
            ReissueCause::DivergentPair,
            ReissueCause::OwnerRefusal,
        );

        // Each arm of `vdb_site`: the phase, the step of the fire, the tail, the end of the
        // element before it, then the answer, the end of the element and the owed sends after it.
        let (empty, owes, removed) = ((false, false), (true, false), (false, true));
        let stages = Some((TailStep::Stages, None));
        let capped = Some((TailStep::Capped, Some(cap)));
        let capped_alone = Some((TailStep::Capped, None));
        let commits = |cause| Some((TailStep::Commits, Some(cause)));
        let cases = [
            (Plan, Continue, empty, None, stages, Some(refusal), 0),
            (Plan, Done, empty, Some(pair), None, None, 0),
            (Plan, Reissue(cap), empty, None, capped_alone, Some(cap), 0),
            (Plan, Reissue(cap), owes, None, capped, Some(cap), 1),
            (Plan, Reissue(cap), removed, None, capped, Some(cap), 0),
            (Plan, Reissue(pair), empty, None, None, Some(pair), 0),
            (Plan, Reissue(refusal), empty, None, None, Some(refusal), 0),
            (Drain, Done, empty, Some(pair), None, Some(pair), 0),
            (Drain, Continue, empty, None, stages, None, 1),
            (Drain, Reissue(cap), empty, None, capped, None, 1),
            (Drain, Reissue(pair), empty, None, commits(pair), None, 1),
            (Drain, Reissue(pair), owes, None, commits(pair), None, 2),
            (Cascade, Done, empty, Some(pair), None, Some(pair), 0),
            (Cascade, Continue, empty, None, None, None, 0),
            (Cascade, Reissue(cap), empty, None, capped, None, 0),
            (Cascade, Reissue(pair), empty, None, commits(pair), None, 0),
        ];
        for (phase, step, tail, before, answer, after, sends) in cases {
            assert_eq!(
                site_of(phase, step, tail, before),
                (answer, after, sends),
                "{phase:?} {step:?}",
            );
        }
        assert_eq!(
            vdb_site(Drain, Continue, &mut Tail::new(), &mut None, None),
            Err(SpaceError::Inconsistent(
                "an offer of the drain has no record",
            )),
            "an offer of the drain without its record fails closed",
        );

        // Each arm of `vdb_drain`: no work, owed work, and a removal with and without a target,
        // below the cap and at it.
        let mut quiet = CollectionView {
            waiters: Vec::new(),
            records: Vec::new(),
        };
        let mut target = CollectionView {
            waiters: vec![parked_list_waiter()],
            records: vec![stored(&payload(1))],
        };
        let stepped = (TailStep::Stages, None);
        let stopped = (TailStep::Capped, Some(cap));
        let ended = (TailStep::Commits, None);
        assert_eq!(drain_of(&mut quiet, (false, false), 0), ended);
        assert_eq!(drain_of(&mut quiet, (false, false), FIRE_ROUND_CAP), ended);
        assert_eq!(drain_of(&mut quiet, (true, false), 0), stepped);
        assert_eq!(drain_of(&mut quiet, (true, false), FIRE_ROUND_CAP), stopped);
        assert_eq!(drain_of(&mut quiet, (true, true), 0), stepped);
        assert_eq!(drain_of(&mut quiet, (false, true), 0), ended);
        assert_eq!(drain_of(&mut quiet, (false, true), FIRE_ROUND_CAP), ended);
        assert_eq!(drain_of(&mut target, (false, true), 0), stepped);
        assert_eq!(
            drain_of(&mut target, (false, true), FIRE_ROUND_CAP),
            stopped
        );

        // `vdb_end` over a persistent produce with an order.
        let produce = ordered_produce();
        let operation = SpaceOp::Produce(produce.clone());
        let again = SpaceOp::Produce(ProduceOp {
            reissued_ord: None,
            ..produce.clone()
        });
        let settle = settle_operation(OpView::Produce(&produce));
        let (first, second) = (payload(1), payload(2));
        let owed = |record: &ListParWithRandom| PendingProduce {
            key: settle.key.clone(),
            record: record.clone(),
            persist: false,
            restore: None,
        };
        // The owed sends in the order in which they run: the last owed first.
        let pending = vec![owed(&second), owed(&first)];
        let owing = || {
            let mut tail = Tail::new();
            tail.owe(first.clone());
            tail.owe(second.clone());
            tail
        };
        let end = |tail, step, stop, own| {
            vdb_end(&operation, tail, step, stop, own).expect("a VectorDB row concludes")
        };

        // `Commits` and `Capped` return the re-issue of `conclude`.
        assert_eq!(
            end(Tail::new(), TailStep::Commits, None, None),
            TailEnd::Return(None),
            "an element that settled re-issues nothing",
        );
        assert_eq!(
            end(Tail::new(), TailStep::Commits, None, Some(pair)),
            TailEnd::Return(Some(SpaceReissue {
                op: again.clone(),
                cause: pair,
            })),
            "an element in flight re-issues the operation without its order",
        );
        assert_eq!(
            end(owing(), TailStep::Commits, Some(pair), None),
            TailEnd::Return(Some(SpaceReissue {
                op: settle_job(&settle, pending.clone(), None),
                cause: pair,
            })),
            "a stop owes its second sends in the settle job",
        );
        assert_eq!(
            end(Tail::new(), TailStep::Capped, None, Some(cap)),
            TailEnd::Return(Some(SpaceReissue {
                op: again.clone(),
                cause: cap,
            })),
            "a plan at the cap with nothing owed re-issues the operation",
        );
        let resume = SpaceReissue {
            op: again.clone(),
            cause: cap,
        };
        assert_eq!(
            end(owing(), TailStep::Capped, Some(cap), Some(cap)),
            TailEnd::Return(Some(SpaceReissue {
                op: settle_job(&settle, pending.clone(), Some(resume)),
                cause: cap,
            })),
            "a stop at the cap carries the operation after the second sends",
        );

        // `Stages` stops with the re-issue of `conclude`: the settle job when the tail owes work
        // or a cascade, and else the operation again.
        assert_eq!(
            end(owing(), TailStep::Stages, None, None),
            TailEnd::Stop(settle_job(&settle, pending, None)),
            "a step over owed work stops with the settle job",
        );
        let resume = SpaceReissue {
            op: again.clone(),
            cause: refusal,
        };
        assert_eq!(
            end(tail_of(false, true), TailStep::Stages, None, Some(refusal)),
            TailEnd::Stop(settle_job(&settle, Vec::new(), Some(resume))),
            "a step over a cascade stops with the settle job and the operation after it",
        );
        assert_eq!(
            end(Tail::new(), TailStep::Stages, None, Some(refusal)),
            TailEnd::Stop(again),
            "a step with nothing owed stops with the operation again",
        );

        // `Stages` with nothing to re-issue fails closed.
        assert_eq!(
            vdb_end(&operation, Tail::new(), TailStep::Stages, None, None),
            Err(SpaceError::Inconsistent(
                "a VectorDB row owes a step that no re-issue carries",
            )),
            "a step with nothing to re-issue is inconsistent",
        );
    }

    /// Added by decision 124 (C4v2 G8, ruling RADD2v4 7.6): the play store of the test of a
    /// leasing theory.
    type ParRSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

    /// Added by decision 124 (C4v2 G8, ruling RADD2v4 7.6): the space of that test.
    const LEASED_SPACE: namespace::NativeSpace = namespace::NativeSpace {
        sid: [23; namespace::SID_LEN],
        durability: namespace::Durability::Durable,
    };

    /// Added by decision 124 (C4v2 G8, ruling RADD2v4 7.6): the channel of the collection of that
    /// test.
    fn leased_channel() -> Par { new_gstring_par("leased".to_string(), Vec::new(), false) }

    /// Added by decision 124 (C4v2 G8, ruling RADD2v4 7.6): the key of that collection.
    fn leased_key() -> namespace::CollectionKey {
        namespace::CollectionKey::of_channel(&leased_channel()).expect("a channel has a key")
    }

    /// Added by decision 124 (C4v2 G8, ruling RADD2v4 7.6): the data channel of that collection.
    fn leased_data() -> Par {
        namespace::gprivate(namespace::data_channel(LEASED_SPACE, &leased_key()))
    }

    /// Added by decision 124 (C4v2 G8, ruling RADD2v4 7.6): the body `name` with its random state.
    fn leased_body(name: &str) -> ParWithRandom {
        ParWithRandom {
            body: Some(new_gstring_par(name.to_string(), Vec::new(), false)),
            random_state: Blake2b512Random::create_from_bytes(name.as_bytes()).to_bytes(),
        }
    }

    /// Added by decision 124 (C4v2 G8, ruling RADD2v4 7.6): a send of `value` to that collection.
    fn leased_send(value: i64) -> SpaceOp {
        SpaceOp::Produce(ProduceOp {
            space: LEASED_SPACE,
            key: leased_key(),
            channel: leased_channel(),
            data: ListParWithRandom {
                pars: vec![int(value)],
                random_state: Blake2b512Random::create_from_bytes(
                    format!("leased send {value}").as_bytes(),
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

    /// Added by decision 124 (C4v2 G8, ruling RADD2v4 7.6): a plain receive of any one value from
    /// that collection, with the body `name`.
    fn leased_receive(name: &str) -> SpaceOp {
        SpaceOp::Consume(ConsumeOp {
            space: LEASED_SPACE,
            key: leased_key(),
            channel: leased_channel(),
            pattern: BindPattern {
                patterns: vec![new_freevar_par(0, Vec::new())],
                remainder: None,
                free_count: 1,
            },
            body: leased_body(name),
            persistent: false,
            peek: false,
            guard: None,
            query: None,
            authority: CostAuthority::default(),
            introduction_authority: CostAuthority::default(),
            reissued_ord: None,
        })
    }

    /// Added by decision 124 (C4v2 G8, ruling RADD2v4 7.6): a fresh play store with a VectorDB
    /// space whose config names the theory `theory`, and an environment over it.
    async fn leased_space(theory: Option<TheoryIdentity>) -> (ParRSpace, SpaceEnv) {
        let mut stores = InMemoryStoreManager::new();
        let host = ParRSpace::create(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(Matcher)),
        )
        .expect("an in-memory space");
        let env = SpaceEnv {
            store: SpaceStoreHandle::new(host.clone()),
            budget: RuntimeBudget::new(Cost::create(10_000_000, "leasing theory test")),
            volatile: Default::default(),
            origin: Default::default(),
        };
        let mut params = BTreeMap::<String, Par>::new();
        params.insert(CAPACITY_PARAM.to_string(), int(64));
        params.insert(DIMENSIONS_PARAM.to_string(), int(2));
        let config = SpaceConfig {
            params,
            theory,
            ..SpaceConfig::new(
                LEASED_SPACE.sid,
                SpaceOuter::HashMap.as_str(),
                SpaceInner::VectorDb.as_str(),
                SpaceQualifier::Default,
            )
        }
        .to_par();
        let family = SpaceFactoryUrn::all()
            .find(|urn| urn.outer == SpaceOuter::HashMap && urn.inner == SpaceInner::VectorDb)
            .expect("a factory family");
        FamilyAgent
            .prepare_create(&env, SpaceCreation {
                space: LEASED_SPACE,
                config,
                handle_pattern: BindPattern {
                    patterns: Vec::new(),
                    remainder: Some(new_freevar_var(0)),
                    free_count: 1,
                },
                handle_continuation: TaggedContinuation {
                    tagged_cont: Some(TaggedCont::ParBody(leased_body("handle"))),
                    guard: None,
                    cost_authority: None,
                },
                random: Blake2b512Random::create_from_bytes(b"leasing theory factory"),
                authority: CostAuthority::default(),
                family,
            })
            .await
            .expect("the creation stages")
            .commit()
            .await
            .expect("the creation commits");
        (host, env)
    }

    /// Added by decision 124 (C4v2 G8, ruling RADD2v4 7.6): runs `op` under owner funding on
    /// `host`, with an observer that turns on the owner checks, so the commit runs the row search
    /// of the plan over the lists of the commit. The observer fails the first item that the
    /// commit tests, which is the item of the staged candidate c0, and it admits each later item.
    /// It gives the records and the waiters of the collection after `op`, and the fires of `op`.
    async fn leased_run(host: &ParRSpace, env: &SpaceEnv, op: SpaceOp) -> ((usize, usize), usize) {
        let armed = AtomicBool::new(true);
        host.set_accounting_observer(Some(Arc::new(Funded::with_item_test(move |_, _| {
            !armed.swap(false, Ordering::SeqCst)
        }))));
        let outcome = run_op(&FamilyAgent, &owner_funded(env), op)
            .await
            .expect("the operation commits");
        let data = leased_data();
        let records = host.get_data(&data).await.len();
        let waiters = host.get_waiting_continuations(vec![data]).await.len();
        ((records, waiters), outcome.fires.len())
    }

    /// Added by decision 124 (rules CR-4 and R13): the search of a recorded refusal binds the
    /// record that the refusal names, wherever that record stands in the rank order of the
    /// row. Each of the two records is named once, so one case names a record that the order
    /// meets second.
    #[tokio::test]
    async fn ranked_receive_search_binds_the_record_that_the_row_names() {
        let (host, env) = leased_space(None).await;
        for value in [1, 2] {
            run_op(&FamilyAgent, &env, leased_send(value))
                .await
                .expect("the send stores its record");
        }
        let env = owner_funded(&env);
        let mut opened = begin_collection(
            &HashMapOuter,
            &env,
            LEASED_SPACE,
            &leased_key(),
            &leased_channel(),
        )
        .await
        .expect("phase A");
        let (kept, searches) = std::sync::mpsc::channel::<ParRowSearch>();
        opened.txn = capturing(opened.txn, kept);
        let op = leased_receive("r");
        let prepared = prepare_collection_op(
            &GenericSpace::<HashMapOuter, VectorDbDiscipline>::default(),
            &env,
            opened,
            op.clone(),
        )
        .await
        .expect("the receive stages");
        drop(prepared);
        let mut search = searches
            .try_recv()
            .expect("the fire of the receive carries its search");
        let records = host.get_data(&leased_data()).await;
        assert_eq!(records.len(), 2, "the records of the collection");
        let SpaceOp::Consume(receive) = op else {
            panic!("a receive is a consume");
        };
        let group = vec![leased_data()];
        let patterns = vec![receive.pattern.clone()];
        let continuation = TaggedContinuation::default();
        let consume = Consume::create(&group, &patterns, &continuation, false);
        let matcher = Matcher;
        let request = ParRowRequest {
            matcher: &matcher,
            group: &group,
            patterns: &patterns,
            continuation: &continuation,
            persistent: false,
            source: &consume,
            own: None,
            item: None,
        };
        for named in &records {
            let identity = NamedRecords {
                named: vec![named.source.clone()],
            };
            let tuple = with_named_lists(&records, &identity, |lists| {
                search
                    .tuple(0, lists, request)
                    .expect("the search reads the lists")
            })
            .expect("the record that the row names binds");
            let bound: Vec<_> = tuple
                .iter()
                .map(|candidate| &candidate.datum.source)
                .collect();
            assert_eq!(
                bound,
                vec![&named.source],
                "the search binds the named record"
            );
        }
    }

    /// Added by decision 124 (C4v2 G8, ruling RADD2v4 7.6): a row search gives no alternative
    /// under a theory that gives a lease (`alternatives` is `!cx.leases()`). The search reads the
    /// lists that only a commit builds, so the test runs a commit: its observer fails the item of
    /// the staged candidate c0, and the row deviates. Without a theory the row takes the next
    /// candidate: the send fires the other waiter, and the receive takes the other record. Under
    /// a leasing theory the search lists only c0: the send stores its record, and the receive
    /// parks. Each row of the table gives the records and the waiters after the operation, and
    /// its fires.
    #[tokio::test]
    async fn leasing_theory_search_gives_no_alternative() {
        let leasing = test_theories::identity(TestBehavior::Leasing);
        let cases = [
            ("no theory", None, ((0, 1), 1), ((1, 0), 1)),
            ("a leasing theory", Some(leasing), ((1, 2), 0), ((2, 1), 0)),
        ];
        for (label, theory, send_end, receive_end) in cases {
            // The offer search: two receives wait, so the send has an alternative to c0.
            let (host, env) = leased_space(theory.clone()).await;
            for name in ["w0", "w1"] {
                run_op(&FamilyAgent, &env, leased_receive(name))
                    .await
                    .expect("the receive parks");
            }
            assert_eq!(
                leased_run(&host, &env, leased_send(5)).await,
                send_end,
                "the send with {label}",
            );

            // The receive search: two records are stored, so the receive has an alternative to c0.
            let (host, env) = leased_space(theory).await;
            for value in [1, 2] {
                run_op(&FamilyAgent, &env, leased_send(value))
                    .await
                    .expect("the send stores its record");
            }
            assert_eq!(
                leased_run(&host, &env, leased_receive("r")).await,
                receive_end,
                "the receive with {label}",
            );
        }
    }
}
