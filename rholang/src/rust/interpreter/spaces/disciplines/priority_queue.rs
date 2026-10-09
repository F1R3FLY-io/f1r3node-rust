//! The priority queue discipline of the native spaces (decisions 11 and 38, P5b, P5 design v3
//! A.8).
//!
//! # Levels and lanes
//!
//! A priority queue keeps its records in levels. A send with `priority = p` puts its datum in
//! the level `p`, and a send without a priority puts it in the unprioritized level, which comes
//! after every priority level (F6). The levels ascend: `0, 1, 2, …, unprioritized`. The record
//! holds its level in the priority slot of its envelope (`record::wrap_pq_datum`), so there is no
//! storage for each level (I-10): a level exists exactly when a record holds it, and the bound
//! `priorities` of the factory only limits the values of the param.
//!
//! The waiting continuations wait in lanes (decision 38). A receive with `priority = p` waits in
//! the waiter queue of the level `p`, and a receive without a priority waits in the unrestricted
//! queue. The continuation holds its lane in the policy slot of its envelope
//! (`record::wrap_pq_continuation`). One counter serves the whole priority queue (plan:375), so
//! every order is unique and two orders of different lanes compare.
//!
//! Each level and each lane is a queue: its head is its entry with the least `(ord, bincode
//! bytes)`, the rule of the queue discipline. Only heads interact (decision 11). A level lane
//! sees the head of its level. The unrestricted lane sees the heads of all levels in ascending
//! level order.
//!
//! # The plans
//!
//! ```text
//! PQ-OFFER(op, ord):
//!   open_plan                                   ▷ settle before the plan, then the cap check (I-21)
//!   ℓ ← Priority(p) for priority = p, else Unprioritized
//!   if the datum is not at the head of level ℓ: store it, settle after, stop   ▷ decision 43
//!   refused ← ∅
//!   loop:
//!     C ← the heads of the lane Level(p) (for ℓ = Priority(p)) and of the lane Any, without
//!         the refused lanes, in ascending ord   ▷ decision 38: the older waiter fires first
//!     W ← the first W ∈ C that matches the datum (the pattern, then the guard). Each refusal
//!         joins refused.                         ▷ decision 37: a refusal moves on
//!     if W = none: store the datum, settle after, stop
//!     fire W. Done: settle after, stop. Continue: offer the datum again (decision 30).
//!       Reissue: re-issue the send (decision 43 order).
//!
//! PQ-RECEIVE(op, ord):
//!   open_plan
//!   L ← Level(p) for priority = p, else Any
//!   if the continuation is not at the head of lane L: park it, settle after, stop
//!   loop:
//!     h ← the first level head that L sees, in ascending level order, whose payload matches
//!         the pattern
//!     if h = none, or the guard refuses the match: park, settle after, stop   ▷ decision 38
//!     fire h. After a peek of a record that is not persistent, store h again (Rule E).
//!       Done: settle after, stop. Continue: the next head. Reissue: re-issue the receive.
//! ```
//!
//! A refused lane keeps its head during the plan, because the plan only fires the heads of the
//! other lane. The same head and the same datum give the same result, so the plan does not test
//! a refused head again.
//!
//! # The settle
//!
//! A fire exposes the next entry of a lane, and that entry can match the head of another lane.
//! The priority queue then settles (F2): it fires the pairs that its heads owe until no head of a
//! waiter lane can fire ([`settle`](crate::rust::interpreter::spaces::settle)). Strict confluence
//! fails for a priority queue, because two waiter lanes can compete for one record (readiness
//! review S4). So the settle fires the fireable waiter-lane head with the least order first
//! ([`PriorityQueueView`]). A waiter-lane head is fireable when the first level head that it sees
//! and whose payload matches its pattern passes its guard (decision 38). The settle runs before
//! the plan of each operation, after it, and in a settle job.
//!
//! # Rules from other decisions
//!
//! * Decision 43: a re-issued persistent element keeps its first-round order, so it takes its
//!   FIFO place in its lane: at the head when its lane is empty or its order is the least, and
//!   otherwise behind the head, with no fire. A re-entered waiter competes with its first-round
//!   order, so it keeps its seniority in the rule "the older waiter fires first".
//! * Decision 45, Rule E: a consume-side peek removes the head record and stores it again with
//!   the same bytes, so with the same order and level. It is then the head of its level again,
//!   and the waiters that did not match it before the peek do not match it after the peek.
//! * Decision 42: every scan reserves the decode of each envelope, each lookup and growth of its
//!   map of heads, each comparison, each payload decode and each copy before it makes them. The
//!   matcher reserves its own reads.
//!
//! The legacy priority queue (`spaces/collections/basic.rs` of the legacy branch) allocated a
//! queue for each level up to the priority of a send, in its capacity routine, and kept its
//! waiters in one bag. Both are replaced.

use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::mem::size_of;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{BindPattern, ListParWithRandom, Par};
use rspace_plus_plus::rspace::trace::event::Produce;
use serde::Serialize;

use crate::rust::interpreter::reified::costs::{FireStep, ScanMeter};
use crate::rust::interpreter::spaces::agent::{
    ConsumeOp, ProduceOp, QueryParams, SettleOp, SpaceOp, SpaceReissue,
};
use crate::rust::interpreter::spaces::disciplines::queue::QueueDiscipline;
use crate::rust::interpreter::spaces::errors::SpaceError;
use crate::rust::interpreter::spaces::factory::defaults::PRIORITIES_PARAM;
use crate::rust::interpreter::spaces::generic::{
    open_plan, peek_set, Cx, Discipline, JoinWindow, OfferFire, OpView, Ordered, ParDatum,
    ParWaiter, ReceiveFire, Side,
};
use crate::rust::interpreter::spaces::record;
use crate::rust::interpreter::spaces::settle::{
    self, Bound, BoundRecord, FirePlan, InFlight, SettleView,
};
use crate::rust::interpreter::spaces::types::config::SpaceConfig;

/// The send param and the query param of a priority queue: the level of a send, and the waiter
/// queue of a receive. It is the key of the policy slot of a waiter (`record::PRIORITY_KEY`).
pub const PRIORITY_PARAM: &str = record::PRIORITY_KEY;

/// The named send params and the named query params of a priority queue.
const PRIORITY_PARAMS: &[&str] = &[PRIORITY_PARAM];

/// The level of a record of a priority queue. The derived order puts the priority levels in
/// ascending order and the unprioritized level after them (F6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Level {
    /// The level `p ≥ 0` of a send with `priority = p`.
    Priority(i64),
    /// The level of a send without a priority.
    Unprioritized,
}

impl Level {
    /// The level of the priority slot `slot` of a datum record: `Some(p)` is the level `p`, and
    /// `None` is the unprioritized level.
    pub const fn of_slot(slot: Option<i64>) -> Self {
        match slot {
            Some(level) => Level::Priority(level),
            None => Level::Unprioritized,
        }
    }

    /// The priority slot of the level.
    pub const fn slot(self) -> Option<i64> {
        match self {
            Level::Priority(level) => Some(level),
            Level::Unprioritized => None,
        }
    }

    /// The waiter lanes whose heads a datum of this level meets in a produce plan: the waiter
    /// queue of the level for a priority level, and the unrestricted queue (decision 38). The
    /// unprioritized level has no waiter queue of its own.
    pub const fn candidate_lanes(self) -> [Option<Lane>; 2] {
        match self {
            Level::Priority(level) => [Some(Lane::Level(level)), Some(Lane::Any)],
            Level::Unprioritized => [None, Some(Lane::Any)],
        }
    }
}

/// The lane of a waiting continuation of a priority queue (decision 38).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Lane {
    /// The waiter queue of the level `p`: a receive with `priority = p`.
    Level(i64),
    /// The unrestricted waiter queue: a receive without a priority.
    Any,
}

impl Lane {
    /// The lane of the policy slot `slot` of a parked continuation: `Some(p)` is the waiter queue
    /// of the level `p`, and `None` is the unrestricted queue.
    pub const fn of_slot(slot: Option<i64>) -> Self {
        match slot {
            Some(level) => Lane::Level(level),
            None => Lane::Any,
        }
    }

    /// The policy slot of the lane.
    pub const fn slot(self) -> Option<i64> {
        match self {
            Lane::Level(level) => Some(level),
            Lane::Any => None,
        }
    }

    /// True when a waiter of this lane sees the records of `level`. A level lane sees its own
    /// level only, and the unrestricted lane sees every level (decision 38).
    pub const fn sees(self, level: Level) -> bool {
        match (self, level) {
            (Lane::Any, _) => true,
            (Lane::Level(own), Level::Priority(other)) => own == other,
            (Lane::Level(_), Level::Unprioritized) => false,
        }
    }
}

/// The head of one lane of a side in the staged view: its index in the view of its channel, its
/// order, and the number of entries of its lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LaneHead {
    /// The index of the head in the staged view of its channel.
    pub index: usize,
    /// The order of the head.
    pub ord: i64,
    /// The number of entries of the lane.
    pub len: usize,
}

/// Where the head of a level is: stored in the staged view, or the record in flight of a
/// settle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HeadAt {
    /// The record at this index of the staged view.
    Stored(usize),
    /// The record in flight of the settle.
    InFlight,
}

/// The head of one level of the records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RecordHead {
    /// The level.
    level: Level,
    /// Where the head is.
    at: HeadAt,
    /// The order of the head.
    ord: i64,
}

/// The priority queue discipline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PriorityQueueDiscipline;

impl Discipline for PriorityQueueDiscipline {
    const NAME: &'static str = "priorityqueue";
    const USES_COUNTER: bool = true;
    const SEND_PARAMS: &'static [&'static str] = PRIORITY_PARAMS;
    const QUERY_PARAMS: &'static [&'static str] = PRIORITY_PARAMS;
    /// A join bind sees, for each level that its lane sees, the first records of the level
    /// (P5c, decision 38).
    const JOIN_WINDOW: JoinWindow = JoinWindow::Lanes;

    /// The head of one lane: the entry with the least `(ord, bincode bytes)`, the rule of the
    /// queue discipline. The plans of the priority queue apply it to each lane.
    fn head_metered<T: Serialize>(
        &self,
        side: Side,
        entries: &[Ordered<'_, T>],
        meter: &ScanMeter,
    ) -> Result<Option<usize>, SpaceError> {
        QueueDiscipline.head_metered(side, entries, meter)
    }

    /// A fresh element joins the tail of its lane, so it is the head only of an empty lane.
    /// `queued` is the number of entries of the lane.
    fn reaches_head(&self, side: Side, queued: usize) -> bool {
        QueueDiscipline.reaches_head(side, queued)
    }

    /// The least order interacts first in a lane, on both sides. The comparison is reserved
    /// first.
    fn precedes(
        &self,
        side: Side,
        ord: i64,
        head: i64,
        meter: &ScanMeter,
    ) -> Result<bool, SpaceError> {
        QueueDiscipline.precedes(side, ord, head, meter)
    }

    /// The value of a `priority` param of a send or of a query is a `GInt(p)` with `p ≥ 0` and
    /// no connective flag, and `p` is below the bound `priorities` when the space sets one
    /// (I-10). Any other value fails with `InvalidParam`. A query with a positional part fails
    /// with `PositionalParamsUnsupported`, because a priority queue gives positional query parts
    /// no meaning.
    fn check_values(
        &self,
        config: &SpaceConfig,
        params: &BTreeMap<String, Par>,
        query: Option<&QueryParams>,
        meter: &ScanMeter,
    ) -> Result<(), SpaceError> {
        if let Some(value) = priority_value(params, meter)? {
            check_priority(config, value, meter)?;
        }
        match query {
            None => Ok(()),
            Some(QueryParams { payload, named }) => match payload.is_empty() {
                false => Err(SpaceError::PositionalParamsUnsupported),
                true => match priority_value(named, meter)? {
                    Some(value) => check_priority(config, value, meter).map(|_| ()),
                    None => Ok(()),
                },
            },
        }
    }

    /// PQ-OFFER (see the module documentation).
    fn offer(
        &self,
        cx: &mut Cx<'_, '_>,
        op: ProduceOp,
        ord: i64,
    ) -> Result<Option<SpaceReissue>, SpaceError> {
        offer_pq(self, cx, op, ord)
    }

    /// PQ-RECEIVE (see the module documentation).
    fn receive(
        &self,
        cx: &mut Cx<'_, '_>,
        op: ConsumeOp,
        ord: i64,
    ) -> Result<Option<SpaceReissue>, SpaceError> {
        receive_pq(self, cx, op, ord)
    }

    /// Fires the pairs that the priority queue still owes before the plan of `op`. A settle that
    /// cannot complete re-issues `op` itself, with the order rule of decision 43.
    fn settle_before(
        &self,
        cx: &mut Cx<'_, '_>,
        op: OpView<'_>,
    ) -> Result<Option<SpaceReissue>, SpaceError> {
        settle::settle_before_plan(&PriorityQueueView::new(self), cx, op)
    }

    /// Fires the pairs that the plan of `op` exposed. A settle that cannot complete re-issues a
    /// settle job.
    fn settle_after(
        &self,
        cx: &mut Cx<'_, '_>,
        op: OpView<'_>,
    ) -> Result<Option<SpaceReissue>, SpaceError> {
        settle::settle_after_plan(&PriorityQueueView::new(self), cx, op)
    }

    /// Fires the pairs that a settle job continues. A settle that cannot complete re-issues the
    /// same job.
    fn settle_job(
        &self,
        cx: &mut Cx<'_, '_>,
        op: SettleOp,
    ) -> Result<Option<SpaceReissue>, SpaceError> {
        settle::settle_job(&PriorityQueueView::new(self), cx, op)
    }
}

impl PriorityQueueDiscipline {
    /// The heads of the lanes of `entries`, one side of a collection, by lane in ascending lane
    /// order. `slots` decodes the order and the lane of an entry. The scan reserves the decode
    /// of each envelope, one ordered lookup for each entry, the growth of the map for each new
    /// lane, and for each other entry the comparison with the head of its lane
    /// (`head_metered` on the pair, with its tie-break). On equal keys the earlier entry stays
    /// the head, as in a queue scan.
    pub fn lane_heads<T: Serialize, K: Ord + Copy>(
        &self,
        side: Side,
        entries: &[T],
        slots: fn(&T, &ScanMeter) -> Result<(i64, K), SpaceError>,
        meter: &ScanMeter,
    ) -> Result<BTreeMap<K, LaneHead>, SpaceError> {
        let mut heads: BTreeMap<K, LaneHead> = BTreeMap::new();
        for (index, entry) in entries.iter().enumerate() {
            let (ord, lane) = slots(entry, meter)?;
            let size = heads.len();
            meter.ordered_lookup(size, size_of::<K>())?;
            match heads.entry(lane) {
                Entry::Vacant(vacant) => {
                    meter.tree_growth::<K, LaneHead>(size, 1)?;
                    vacant.insert(LaneHead { index, ord, len: 1 });
                }
                Entry::Occupied(mut occupied) => {
                    let best = *occupied.get();
                    let len = best
                        .len
                        .checked_add(1)
                        .ok_or(SpaceError::Inconsistent("a lane holds too many entries"))?;
                    let current = entries.get(best.index).ok_or(SpaceError::Inconsistent(
                        "a lane head is not an entry of its side",
                    ))?;
                    let pair = [
                        Ordered {
                            ord: best.ord,
                            entry: current,
                        },
                        Ordered { ord, entry },
                    ];
                    let head = match self.head_metered(side, &pair, meter)? {
                        Some(1) => LaneHead { index, ord, len },
                        Some(_) | None => LaneHead { len, ..best },
                    };
                    occupied.insert(head);
                }
            }
        }
        Ok(heads)
    }

    /// The heads of the levels of the records of the collection in the staged view.
    fn record_heads(&self, cx: &mut Cx<'_, '_>) -> Result<BTreeMap<Level, LaneHead>, SpaceError> {
        let meter = cx.meter();
        let records = cx.data()?;
        self.lane_heads(Side::Data, records, record_slots, meter)
    }

    /// The heads of the lanes of the waiting continuations of the collection in the staged view.
    fn waiter_heads(&self, cx: &mut Cx<'_, '_>) -> Result<BTreeMap<Lane, LaneHead>, SpaceError> {
        let meter = cx.meter();
        let waiters = cx.waiters()?;
        self.lane_heads(Side::Waiters, waiters, waiter_slots, meter)
    }

    /// True when the element of an operation reaches the head of its lane, whose head is `head`
    /// (decision 43), as `Cx::at_head` decides it for a whole side. A fresh element (`reissued`
    /// is `None`) joins the tail of its lane, so it reaches the head only of an empty lane. A
    /// re-issued persistent element re-enters at its FIFO place: at the head when its lane is
    /// empty or its order precedes the order of the head. An element in flight is stored
    /// nowhere, so a stored entry with its order is `Inconsistent`. Each comparison is reserved.
    fn at_lane_head(
        &self,
        side: Side,
        head: Option<&LaneHead>,
        reissued: Option<i64>,
        meter: &ScanMeter,
    ) -> Result<bool, SpaceError> {
        match (reissued, head) {
            (None, None) => Ok(self.reaches_head(side, 0)),
            (None, Some(head)) => Ok(self.reaches_head(side, head.len)),
            (Some(_), None) => Ok(true),
            (Some(ord), Some(head)) => {
                meter.compare(size_of::<i64>(), size_of::<i64>())?;
                match ord == head.ord {
                    true => Err(SpaceError::Inconsistent(
                        "a re-issued element shares its order with a stored entry",
                    )),
                    false => self.precedes(side, ord, head.ord, meter),
                }
            }
        }
    }

    /// The heads of the levels in ascending level order. A settle passes the level and the order
    /// of its record in flight (`flight`). That record counts as the head of its level when its
    /// order is the least of the level: it was the head when the settle lifted it, and the settle
    /// stores no record while one is in flight. The buffer and each comparison are reserved.
    fn record_head_list(
        &self,
        cx: &mut Cx<'_, '_>,
        flight: Option<(Level, i64)>,
    ) -> Result<Vec<RecordHead>, SpaceError> {
        let meter = cx.meter();
        let stored = self.record_heads(cx)?;
        let length = stored.len().checked_add(1).ok_or(SpaceError::Inconsistent(
            "a collection holds too many levels",
        ))?;
        let mut heads = meter.buffer::<RecordHead>(length)?;
        let mut pending = flight;
        for (level, head) in &stored {
            match pending {
                Some((flight_level, flight_ord)) if flight_level < *level => {
                    heads.push(RecordHead {
                        level: flight_level,
                        at: HeadAt::InFlight,
                        ord: flight_ord,
                    });
                    pending = None;
                }
                Some((flight_level, flight_ord)) if flight_level == *level => {
                    meter.compare(size_of::<i64>(), size_of::<i64>())?;
                    heads.push(match flight_ord.cmp(&head.ord) {
                        std::cmp::Ordering::Less => RecordHead {
                            level: flight_level,
                            at: HeadAt::InFlight,
                            ord: flight_ord,
                        },
                        std::cmp::Ordering::Greater => RecordHead {
                            level: *level,
                            at: HeadAt::Stored(head.index),
                            ord: head.ord,
                        },
                        std::cmp::Ordering::Equal => {
                            return Err(SpaceError::Inconsistent(
                                "a record in flight shares its order with a stored record",
                            ))
                        }
                    });
                    pending = None;
                    continue;
                }
                Some(_) | None => {}
            }
            heads.push(RecordHead {
                level: *level,
                at: HeadAt::Stored(head.index),
                ord: head.ord,
            });
        }
        if let Some((level, ord)) = pending {
            heads.push(RecordHead {
                level,
                at: HeadAt::InFlight,
                ord,
            });
        }
        Ok(heads)
    }

    /// The first head of `heads`, in ascending level order, of a level that `lane` sees and whose
    /// payload matches `pattern`: its position in `heads` and the values of the match. The
    /// payload of each head is decoded at most once (`payloads`), with the decode reserved, and
    /// the metered matcher reserves each match. The guard is not part of this test (decision
    /// 38: the caller checks the guard once on the result).
    fn first_match(
        &self,
        cx: &mut Cx<'_, '_>,
        lane: Lane,
        pattern: &BindPattern,
        heads: &[RecordHead],
        payloads: &mut [Option<ListParWithRandom>],
        in_flight: &InFlight,
    ) -> Result<Option<(usize, ListParWithRandom)>, SpaceError> {
        let meter = cx.meter();
        for (position, head) in heads.iter().enumerate() {
            if !lane.sees(head.level) {
                continue;
            }
            let slot = payloads
                .get_mut(position)
                .ok_or(SpaceError::Inconsistent("a level head has no payload slot"))?;
            let payload = match slot {
                Some(payload) => payload,
                None => slot.insert(head_payload(cx, head.at, in_flight, meter)?),
            };
            if let Some(matched) = cx.bind(pattern, payload)? {
                return Ok(Some((position, matched)));
            }
        }
        Ok(None)
    }

    /// The waiter of a produce plan: the first head of the candidate lanes `lanes` of the datum
    /// `data`, in ascending order and without the refused lanes, that matches the datum (the
    /// pattern, then the guard). Each head that refuses marks its lane in `refused`. Only the
    /// tested heads are copied, and each copy, lookup, comparison and match is reserved.
    fn offer_target(
        &self,
        cx: &mut Cx<'_, '_>,
        lanes: &[Option<Lane>; 2],
        refused: &mut [bool; 2],
        data: &ListParWithRandom,
    ) -> Result<Option<(ParWaiter, ListParWithRandom)>, SpaceError> {
        let meter = cx.meter();
        let heads = self.waiter_heads(cx)?;
        let mut candidates: [Option<(usize, LaneHead)>; 2] = [None, None];
        for (slot, (lane, is_refused)) in lanes.iter().zip(refused.iter()).enumerate() {
            if let (Some(lane), false) = (lane, *is_refused) {
                if let Some(head) = lookup_head(&heads, lane, meter)? {
                    if let Some(candidate) = candidates.get_mut(slot) {
                        *candidate = Some((slot, *head));
                    }
                }
            }
        }
        let order = match candidates {
            [Some(first), Some(second)] => {
                meter.compare(size_of::<i64>(), size_of::<i64>())?;
                match second.1.ord < first.1.ord {
                    true => [Some(second), Some(first)],
                    false => [Some(first), Some(second)],
                }
            }
            other => other,
        };
        for (slot, head) in order.into_iter().flatten() {
            let waiter = {
                let waiters = cx.waiters()?;
                let stored = waiters.get(head.index).ok_or(SpaceError::Inconsistent(
                    "a lane head is not an entry of its side",
                ))?;
                meter.copy(stored)?
            };
            let [pattern] = waiter.patterns.as_slice() else {
                return Err(SpaceError::Inconsistent(
                    "a waiting continuation of a collection has several binds",
                ));
            };
            match cx.matches(pattern, data, &waiter.continuation)? {
                Some(matched) => return Ok(Some((waiter, matched))),
                None => {
                    if let Some(mark) = refused.get_mut(slot) {
                        *mark = true;
                    }
                }
            }
        }
        Ok(None)
    }
}

/// The order and the level of a record in the staged view, with the decode of its envelope
/// reserved.
fn record_slots(datum: &ParDatum, meter: &ScanMeter) -> Result<(i64, Level), SpaceError> {
    let (ord, slot) = record::pq_datum_slots_metered(&datum.a, meter)?;
    Ok((ord, Level::of_slot(slot)))
}

/// The order and the lane of a waiting continuation in the staged view, with the decode of its
/// envelope reserved.
fn waiter_slots(waiter: &ParWaiter, meter: &ScanMeter) -> Result<(i64, Lane), SpaceError> {
    let (ord, slot) = record::pq_continuation_slots_metered(&waiter.continuation, meter)?;
    Ok((ord, Lane::of_slot(slot)))
}

/// The head of the lane `lane` in `heads`, with the lookup reserved.
fn lookup_head<'h, K: Ord>(
    heads: &'h BTreeMap<K, LaneHead>,
    lane: &K,
    meter: &ScanMeter,
) -> Result<Option<&'h LaneHead>, SpaceError> {
    meter.ordered_lookup(heads.len(), size_of::<K>())?;
    Ok(heads.get(lane))
}

/// The payload of the head record at `at`, decoded from the staged view or from the record in
/// flight, with the decode reserved.
fn head_payload(
    cx: &mut Cx<'_, '_>,
    at: HeadAt,
    in_flight: &InFlight,
    meter: &ScanMeter,
) -> Result<ListParWithRandom, SpaceError> {
    match at {
        HeadAt::Stored(index) => {
            let records = cx.data()?;
            let stored = records.get(index).ok_or(SpaceError::Inconsistent(
                "a level head is not a record of the collection",
            ))?;
            Ok(record::unwrap_pq_datum_metered(&stored.a, meter)?.datum)
        }
        HeadAt::InFlight => match in_flight.record() {
            Some(lifted) => Ok(record::unwrap_pq_datum_metered(&lifted.datum.a, meter)?.datum),
            None => Err(SpaceError::Inconsistent(
                "a level head is in flight, but no record is in flight",
            )),
        },
    }
}

/// A copy of the record at the index `index` of the staged view, with the copy reserved.
fn stored_record(cx: &mut Cx<'_, '_>, index: usize) -> Result<ParDatum, SpaceError> {
    let meter = cx.meter();
    let records = cx.data()?;
    let stored = records.get(index).ok_or(SpaceError::Inconsistent(
        "a level head is not a record of the collection",
    ))?;
    Ok(meter.copy(stored)?)
}

/// The value of the param `priority` in `params`, with the lookup reserved. An empty map does no
/// work.
fn priority_value<'p>(
    params: &'p BTreeMap<String, Par>,
    meter: &ScanMeter,
) -> Result<Option<&'p Par>, SpaceError> {
    match params.is_empty() {
        true => Ok(None),
        false => {
            meter.ordered_lookup(params.len(), PRIORITY_PARAM.len())?;
            Ok(params.get(PRIORITY_PARAM))
        }
    }
}

/// The `InvalidParam` error of the param `priority`.
fn invalid_priority() -> SpaceError {
    SpaceError::InvalidParam {
        key: PRIORITY_PARAM.to_owned(),
    }
}

/// The level `p` of the value of a `priority` param: a `GInt(p)` with `p ≥ 0` and no connective
/// flag. Any other value fails with `InvalidParam`. The read of the value and the comparison with
/// 0 are reserved first.
fn priority_of(value: &Par, meter: &ScanMeter) -> Result<i64, SpaceError> {
    meter.inspect(value)?;
    meter.compare(size_of::<i64>(), size_of::<i64>())?;
    match (record::sole_expr(value), value.connective_used) {
        (Some(ExprInstance::GInt(level @ 0..=i64::MAX)), false) => Ok(*level),
        _ => Err(invalid_priority()),
    }
}

/// The bound of the levels of a space: `Some(n)` when its config sets `priorities = n`, and
/// `None` without the param (FACTORY D3: an absent `priorities` means no bound). The lookup and
/// the read of the value are reserved first. A value that is not a `GInt` of at least 1 is
/// `Inconsistent`, because the factory accepts no other value (I-10).
fn priorities_bound(config: &SpaceConfig, meter: &ScanMeter) -> Result<Option<i64>, SpaceError> {
    match config.params.is_empty() {
        true => Ok(None),
        false => {
            meter.ordered_lookup(config.params.len(), PRIORITIES_PARAM.len())?;
            match config.params.get(PRIORITIES_PARAM) {
                None => Ok(None),
                Some(value) => {
                    meter.inspect(value)?;
                    match (record::sole_expr(value), value.connective_used) {
                        (Some(ExprInstance::GInt(bound @ 1..=i64::MAX)), false) => Ok(Some(*bound)),
                        _ => Err(SpaceError::Inconsistent(
                            "the priorities param of a space config is not a positive integer",
                        )),
                    }
                }
            }
        }
    }
}

/// The level of the value of a `priority` param, checked against the bound of the space: below
/// the bound when the space sets one (I-10). The comparison with the bound is reserved first.
fn check_priority(config: &SpaceConfig, value: &Par, meter: &ScanMeter) -> Result<i64, SpaceError> {
    let level = priority_of(value, meter)?;
    match priorities_bound(config, meter)? {
        None => Ok(level),
        Some(bound) => {
            meter.compare(size_of::<i64>(), size_of::<i64>())?;
            match level < bound {
                true => Ok(level),
                false => Err(invalid_priority()),
            }
        }
    }
}

/// The level of the datum of a send: `Priority(p)` for `priority = p`, and `Unprioritized`
/// without the param. `check_values` checked the value before the bump.
pub(crate) fn send_level(
    params: &BTreeMap<String, Par>,
    meter: &ScanMeter,
) -> Result<Level, SpaceError> {
    match priority_value(params, meter)? {
        Some(value) => Ok(Level::Priority(priority_of(value, meter)?)),
        None => Ok(Level::Unprioritized),
    }
}

/// The lane of the continuation of a receive: `Level(p)` for a query with `priority = p`, and
/// `Any` otherwise. `check_values` checked the query before the bump.
pub(crate) fn receive_lane(
    query: &Option<QueryParams>,
    meter: &ScanMeter,
) -> Result<Lane, SpaceError> {
    match query {
        Some(QueryParams { named, .. }) => match priority_value(named, meter)? {
            Some(value) => Ok(Lane::Level(priority_of(value, meter)?)),
            None => Ok(Lane::Any),
        },
        None => Ok(Lane::Any),
    }
}

/// PQ-OFFER: stages the store or the fires of the datum of `op` with the order `ord`, and returns
/// the re-issue (see the module documentation). The settle hooks run before and after the plan.
/// A fire that ends the loop moves the record and its source into the staged produce, and a
/// re-issue moves `op`, so only a re-offer copies them.
fn offer_pq(
    discipline: &PriorityQueueDiscipline,
    cx: &mut Cx<'_, '_>,
    op: ProduceOp,
    ord: i64,
) -> Result<Option<SpaceReissue>, SpaceError> {
    if let Some(reissue) = open_plan(discipline, cx, OpView::Produce(&op))? {
        return Ok(Some(reissue));
    }
    let meter = cx.meter();
    let level = send_level(&op.params, meter)?;
    let record = record::wrap_pq_datum_metered(ord, level.slot(), &op.data, meter)?;
    let record_heads = discipline.record_heads(cx)?;
    let head = lookup_head(&record_heads, &level, meter)?;
    if !discipline.at_lane_head(Side::Data, head, op.reissued_ord, meter)? {
        cx.store(record, op.persistent)?;
        return discipline.settle_after(cx, OpView::Produce(&op));
    }
    let source = Produce::create_metered(cx.data_channel(), &record, op.persistent, meter)?;
    let lanes = level.candidate_lanes();
    let mut refused = [false; 2];
    loop {
        let Some((waiter, matched)) =
            discipline.offer_target(cx, &lanes, &mut refused, &op.data)?
        else {
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
        match cx.rounds().after_fire(spent, stays) {
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

/// PQ-RECEIVE: stages the park or the fires of the continuation of `op` with the order `ord`, and
/// returns the re-issue (see the module documentation). After a peek fire of a record that is
/// not persistent, `Discipline::restore` stores the record again (decision 45, Rule E). The
/// settle hooks run before and after the plan.
fn receive_pq(
    discipline: &PriorityQueueDiscipline,
    cx: &mut Cx<'_, '_>,
    op: ConsumeOp,
    ord: i64,
) -> Result<Option<SpaceReissue>, SpaceError> {
    if let Some(reissue) = open_plan(discipline, cx, OpView::Consume(&op))? {
        return Ok(Some(reissue));
    }
    let meter = cx.meter();
    let lane = receive_lane(&op.query, meter)?;
    let continuation = record::wrap_pq_continuation_metered(
        ord,
        lane.slot(),
        &op.body,
        &op.guard,
        record::stamped(&op.authority),
        meter,
    )?;
    let peeks = peek_set(op.peek, meter)?;
    let waiter_heads = discipline.waiter_heads(cx)?;
    let head = lookup_head(&waiter_heads, &lane, meter)?;
    if !discipline.at_lane_head(Side::Waiters, head, op.reissued_ord, meter)? {
        cx.park(&op, continuation, peeks)?;
        return discipline.settle_after(cx, OpView::Consume(&op));
    }
    loop {
        let heads = discipline.record_head_list(cx, None)?;
        let mut payloads = meter.buffer::<Option<ListParWithRandom>>(heads.len())?;
        payloads.resize(heads.len(), None);
        let target = discipline.first_match(
            cx,
            lane,
            &op.pattern,
            &heads,
            &mut payloads,
            &InFlight::none(),
        )?;
        let Some((position, matched)) = target else {
            cx.park(&op, continuation, peeks)?;
            return discipline.settle_after(cx, OpView::Consume(&op));
        };
        // Decision 38: the guard is checked once, on the first level head that matches the
        // pattern, and a refusal parks the continuation.
        if !cx.admits(&continuation, &matched)? {
            cx.park(&op, continuation, peeks)?;
            return discipline.settle_after(cx, OpView::Consume(&op));
        }
        let index = match heads.get(position).map(|head| head.at) {
            Some(HeadAt::Stored(index)) => index,
            Some(HeadAt::InFlight) | None => {
                return Err(SpaceError::Inconsistent(
                    "the target of a receive is not a stored record",
                ))
            }
        };
        let head = stored_record(cx, index)?;
        let spent = !op.persistent;
        let stays = head.persist || op.peek;
        let view_index = i32::try_from(index)
            .map_err(|_| SpaceError::Inconsistent("a record index exceeds i32"))?;
        record::prepay_continuation_unwrap(&continuation, meter)?;
        // Rule E (decision 45): the peek fire removes a non-persistent record, as the host COMM
        // does, and the record is stored again right after the fire.
        let restored = match op.peek && !head.persist {
            true => Some(meter.copy(&head.a)?),
            false => None,
        };
        let fire = ReceiveFire {
            head,
            matched,
            view_index,
        };
        match cx.rounds().after_fire(spent, stays) {
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

/// Stores again `restored`, the record that a peek fire removed, when there is one (decision
/// 45, Rule E). The default restore of a discipline is a plain store with the record's own
/// authority and no counter bump: the record keeps its order and its level.
fn restore_peeked(
    discipline: &PriorityQueueDiscipline,
    cx: &mut Cx<'_, '_>,
    restored: Option<ListParWithRandom>,
) -> Result<Option<SpaceReissue>, SpaceError> {
    match restored {
        Some(record) => discipline.restore(cx, record),
        None => Ok(None),
    }
}

/// The settle order of a priority queue: the fireable waiter-lane head with the least order
/// (readiness review S4).
#[derive(Clone, Copy, Debug)]
pub struct PriorityQueueView<'d> {
    discipline: &'d PriorityQueueDiscipline,
}

impl<'d> PriorityQueueView<'d> {
    /// The settle order of the priority queue `discipline`.
    pub const fn new(discipline: &'d PriorityQueueDiscipline) -> Self {
        PriorityQueueView { discipline }
    }
}

/// The best fire of a settle round so far: the order of its waiter, the waiter, the position of
/// its record in the list of level heads, and the values of the match.
struct Chosen {
    ord: i64,
    waiter: ParWaiter,
    position: usize,
    matched: ListParWithRandom,
}

impl SettleView for PriorityQueueView<'_> {
    /// The fireable waiter-lane head with the least order, and the record that it fires. A head
    /// of a waiter lane is fireable when the first level head that it sees and whose payload
    /// matches its pattern passes its guard (decision 38). A record in flight counts as the head
    /// of its level. An empty side plans nothing and reads nothing. The scans, each copy, each
    /// payload decode, each comparison and each match are reserved.
    fn next_fire(
        &self,
        cx: &mut Cx<'_, '_>,
        in_flight: &InFlight,
    ) -> Result<Option<FirePlan>, SpaceError> {
        if cx.side_len(Side::Waiters)? == 0 {
            return Ok(None);
        }
        if !in_flight.is_some() && cx.side_len(Side::Data)? == 0 {
            return Ok(None);
        }
        let meter = cx.meter();
        let flight = match in_flight.record() {
            Some(lifted) => {
                let (ord, slot) = record::pq_datum_slots_metered(&lifted.datum.a, meter)?;
                Some((Level::of_slot(slot), ord))
            }
            None => None,
        };
        let heads = self.discipline.record_head_list(cx, flight)?;
        let waiter_heads = self.discipline.waiter_heads(cx)?;
        let mut payloads = meter.buffer::<Option<ListParWithRandom>>(heads.len())?;
        payloads.resize(heads.len(), None);
        let mut chosen: Option<Chosen> = None;
        for (lane, head) in &waiter_heads {
            let waiter = {
                let waiters = cx.waiters()?;
                let stored = waiters.get(head.index).ok_or(SpaceError::Inconsistent(
                    "a lane head is not an entry of its side",
                ))?;
                meter.copy(stored)?
            };
            let [pattern] = waiter.patterns.as_slice() else {
                return Err(SpaceError::Inconsistent(
                    "a waiting continuation of a collection has several binds",
                ));
            };
            let found = self.discipline.first_match(
                cx,
                *lane,
                pattern,
                &heads,
                &mut payloads,
                in_flight,
            )?;
            let Some((position, matched)) = found else {
                continue;
            };
            if !cx.admits(&waiter.continuation, &matched)? {
                continue;
            }
            let older = match &chosen {
                None => true,
                Some(best) => {
                    meter.compare(size_of::<i64>(), size_of::<i64>())?;
                    head.ord < best.ord
                }
            };
            if older {
                chosen = Some(Chosen {
                    ord: head.ord,
                    waiter,
                    position,
                    matched,
                });
            }
        }
        let Some(Chosen {
            waiter,
            position,
            matched,
            ..
        }) = chosen
        else {
            return Ok(None);
        };
        let record = match heads.get(position).map(|head| head.at) {
            Some(HeadAt::InFlight) => BoundRecord::InFlight,
            Some(HeadAt::Stored(index)) => BoundRecord::Stored {
                datum: stored_record(cx, index)?,
                view_index: i32::try_from(index)
                    .map_err(|_| SpaceError::Inconsistent("a record index exceeds i32"))?,
            },
            None => return Err(SpaceError::Inconsistent("a settle fire has no level head")),
        };
        let mut group = meter.buffer::<Par>(1)?;
        group.push(meter.copy(cx.data_channel())?);
        let mut bound = meter.buffer::<Bound>(1)?;
        bound.push(Bound { record, matched });
        Ok(Some(FirePlan {
            group,
            waiter,
            bound,
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crypto::rust::hash::blake2b512_random::Blake2b512Random;
    use models::rhoapi::tagged_continuation::TaggedCont;
    use models::rhoapi::{CostAuthority, ParWithRandom, TaggedContinuation};
    use models::rust::host_work::{HostWorkDimension, HostWorkLimit, HostWorkLimits};
    use models::rust::space_factory_urn::{SpaceFactoryUrn, SpaceInner, SpaceOuter};
    use models::rust::utils::{new_freevar_var, new_gint_par, new_gstring_par};
    use rspace_plus_plus::rspace::errors::RSpaceError;
    use rspace_plus_plus::rspace::rspace::RSpace;
    use rspace_plus_plus::rspace::rspace_interface::ISpace;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    use rspace_plus_plus::rspace::space_store_handle::SpaceStoreHandle;

    use super::*;
    use crate::rust::interpreter::accounting::costs::Cost;
    use crate::rust::interpreter::accounting::RuntimeBudget;
    use crate::rust::interpreter::host_work::HostWorkBudget;
    use crate::rust::interpreter::matcher::r#match::Matcher;
    use crate::rust::interpreter::spaces::agent::{
        run_op, SpaceAgent, SpaceCreation, SpaceEnv, SpaceOutcome,
    };
    use crate::rust::interpreter::spaces::generic::{CollectionChannels, GenericSpace, OuterStore};
    use crate::rust::interpreter::spaces::namespace::{
        CollectionKey, Durability, NativeSpace, SID_LEN,
    };
    use crate::rust::interpreter::spaces::outer::hashmap::HashMapOuter;
    use crate::rust::interpreter::spaces::types::qualifier::SpaceQualifier;

    type PriorityQueue = GenericSpace<HashMapOuter, PriorityQueueDiscipline>;
    type ParRSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

    const SPACE: NativeSpace = NativeSpace {
        sid: [0x5B; SID_LEN],
        durability: Durability::Durable,
    };

    fn int(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

    fn string(value: &str) -> Par { new_gstring_par(value.to_string(), Vec::new(), false) }

    fn ordered(entries: &[(i64, String)]) -> Vec<Ordered<'_, String>> {
        entries
            .iter()
            .map(|(ord, entry)| Ordered { ord: *ord, entry })
            .collect()
    }

    #[test]
    fn space_pq_discipline_takes_the_priority_param_on_both_sides() {
        assert_eq!(PriorityQueueDiscipline::NAME, "priorityqueue");
        assert_eq!(
            PriorityQueueDiscipline::NAME,
            SpaceInner::PriorityQueue.as_str()
        );
        assert!(PriorityQueueDiscipline::USES_COUNTER);
        assert_eq!(PriorityQueueDiscipline::SEND_PARAMS, &["priority"]);
        assert_eq!(PriorityQueueDiscipline::QUERY_PARAMS, &["priority"]);
        assert_eq!(PriorityQueueDiscipline::CAPACITY, None);
        assert_eq!(PRIORITY_PARAM, record::PRIORITY_KEY);
    }

    #[test]
    fn space_pq_levels_ascend_with_the_unprioritized_level_last() {
        let mut levels = vec![
            Level::Unprioritized,
            Level::Priority(3),
            Level::Priority(0),
            Level::Priority(i64::MAX),
            Level::Priority(1),
        ];
        levels.sort();
        assert_eq!(levels, vec![
            Level::Priority(0),
            Level::Priority(1),
            Level::Priority(3),
            Level::Priority(i64::MAX),
            Level::Unprioritized,
        ]);
        for slot in [None, Some(0), Some(7)] {
            assert_eq!(Level::of_slot(slot).slot(), slot);
            assert_eq!(Lane::of_slot(slot).slot(), slot);
        }
        assert_eq!(Level::Priority(2).candidate_lanes(), [
            Some(Lane::Level(2)),
            Some(Lane::Any)
        ]);
        assert_eq!(Level::Unprioritized.candidate_lanes(), [
            None,
            Some(Lane::Any)
        ]);
    }

    #[test]
    fn space_pq_lanes_see_their_levels() {
        for level in [Level::Priority(0), Level::Priority(4), Level::Unprioritized] {
            assert!(
                Lane::Any.sees(level),
                "the unrestricted lane sees {level:?}"
            );
        }
        assert!(Lane::Level(4).sees(Level::Priority(4)));
        assert!(!Lane::Level(4).sees(Level::Priority(3)));
        assert!(!Lane::Level(0).sees(Level::Unprioritized));
    }

    #[test]
    fn space_pq_lane_head_is_the_least_ord_with_the_queue_tie_break() {
        let entries = vec![
            (4, "d".to_string()),
            (1, "a".to_string()),
            (7, "g".to_string()),
        ];
        for side in [Side::Data, Side::Waiters] {
            assert_eq!(
                PriorityQueueDiscipline.head(side, &ordered(&entries)),
                QueueDiscipline.head(side, &ordered(&entries))
            );
            assert_eq!(
                PriorityQueueDiscipline.head(side, &ordered(&entries)),
                Ok(Some(1))
            );
        }
        for side in [Side::Data, Side::Waiters] {
            for queued in [0, 1, 5] {
                assert_eq!(
                    PriorityQueueDiscipline.reaches_head(side, queued),
                    queued == 0
                );
            }
            let meter = ScanMeter::new(None);
            assert_eq!(
                PriorityQueueDiscipline.precedes(side, 2, 5, &meter),
                Ok(true)
            );
            assert_eq!(
                PriorityQueueDiscipline.precedes(side, 5, 2, &meter),
                Ok(false)
            );
        }
    }

    /// The lane of a test entry, and its order: `(lane, ord, payload)`.
    type Entry3 = (u8, i64, String);

    fn entry_slots(entry: &Entry3, _meter: &ScanMeter) -> Result<(i64, u8), SpaceError> {
        Ok((entry.1, entry.0))
    }

    #[test]
    fn space_pq_lane_heads_group_by_lane_and_ignore_the_list_order() {
        let entries: Vec<Entry3> = vec![
            (1, 9, "x".into()),
            (0, 4, "y".into()),
            (1, 3, "z".into()),
            (2, 8, "w".into()),
            (0, 6, "v".into()),
        ];
        let heads = PriorityQueueDiscipline
            .lane_heads(Side::Data, &entries, entry_slots, &ScanMeter::new(None))
            .expect("the heads");
        let summary: Vec<(u8, i64, usize)> = heads
            .iter()
            .map(|(lane, head)| (*lane, head.ord, head.len))
            .collect();
        assert_eq!(summary, vec![(0, 4, 2), (1, 3, 2), (2, 8, 1)]);
        for (lane, head) in &heads {
            assert_eq!(entries[head.index].0, *lane);
            assert_eq!(entries[head.index].1, head.ord);
        }
        // Every rotation of the list gives the same heads by entry.
        for shift in 0..entries.len() {
            let mut rotated = entries.clone();
            rotated.rotate_left(shift);
            let rotated_heads = PriorityQueueDiscipline
                .lane_heads(Side::Waiters, &rotated, entry_slots, &ScanMeter::new(None))
                .expect("the heads");
            for (lane, head) in &rotated_heads {
                let original = heads.get(lane).expect("the same lanes");
                assert_eq!(rotated[head.index], entries[original.index]);
                assert_eq!(head.len, original.len);
            }
        }
    }

    #[test]
    fn space_pq_lane_heads_reserve_their_work_first() {
        let entries: Vec<Entry3> = vec![(1, 9, "x".into()), (0, 4, "y".into()), (1, 3, "z".into())];
        let budget = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX)));
        let meter = ScanMeter::new(Some(budget.clone()));
        PriorityQueueDiscipline
            .lane_heads(Side::Data, &entries, entry_slots, &meter)
            .expect("the heads");
        let operations = HostWorkDimension::VerificationOperations;
        let usage = budget.usage(operations).get();
        assert!(
            usage > 0,
            "the lookups, the growth and the comparison are reserved"
        );
        let mut limits = HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX));
        limits.set(operations, HostWorkLimit::new(usage - 1));
        let cut = ScanMeter::new(Some(HostWorkBudget::new(limits)));
        assert_eq!(
            PriorityQueueDiscipline.lane_heads(Side::Data, &entries, entry_slots, &cut),
            Err(SpaceError::Host(RSpaceError::HostWorkRejected))
        );
    }

    fn config(priorities: Option<i64>) -> SpaceConfig {
        let mut config = SpaceConfig::new(
            SPACE.sid,
            HashMapOuter::NAME,
            PriorityQueueDiscipline::NAME,
            SpaceQualifier::Default,
        );
        if let Some(bound) = priorities {
            config
                .params
                .insert(PRIORITIES_PARAM.to_string(), int(bound));
        }
        config
    }

    fn priority(value: Par) -> BTreeMap<String, Par> {
        BTreeMap::from([(PRIORITY_PARAM.to_string(), value)])
    }

    fn check(
        config: &SpaceConfig,
        params: &BTreeMap<String, Par>,
        query: Option<&QueryParams>,
    ) -> Result<(), SpaceError> {
        PriorityQueueDiscipline.check_values(config, params, query, &ScanMeter::new(None))
    }

    #[test]
    fn space_pq_check_values_accepts_exactly_the_levels_below_the_bound() {
        let invalid = Err(SpaceError::InvalidParam {
            key: "priority".to_string(),
        });
        let no_params = BTreeMap::new();
        for (bound, level, accepted) in [
            (None, 0, true),
            (None, i64::MAX, true),
            (None, -1, false),
            (Some(3), 2, true),
            (Some(3), 3, false),
            (Some(1), 0, true),
            (Some(1), 1, false),
        ] {
            let config = config(bound);
            let expected = match accepted {
                true => Ok(()),
                false => invalid.clone(),
            };
            assert_eq!(
                check(&config, &priority(int(level)), None),
                expected,
                "send, bound {bound:?}, level {level}"
            );
            let query = QueryParams {
                payload: Vec::new(),
                named: priority(int(level)),
            };
            assert_eq!(
                check(&config, &no_params, Some(&query)),
                expected,
                "query, bound {bound:?}, level {level}"
            );
        }
        let mut flagged = int(1);
        flagged.connective_used = true;
        let mut two = int(1);
        two.exprs.extend(int(2).exprs);
        for value in [string("1"), flagged, two, Par::default()] {
            assert_eq!(
                check(&config(None), &priority(value.clone()), None),
                invalid,
                "{value:?}"
            );
        }
        // A query without a priority and a send without params are accepted.
        let unrestricted = QueryParams::default();
        assert_eq!(
            check(&config(Some(2)), &no_params, Some(&unrestricted)),
            Ok(())
        );
        assert_eq!(check(&config(Some(2)), &no_params, None), Ok(()));
        // A positional query part has no meaning on a priority queue.
        let positional = QueryParams {
            payload: vec![int(1)],
            named: BTreeMap::new(),
        };
        assert_eq!(
            check(&config(None), &no_params, Some(&positional)),
            Err(SpaceError::PositionalParamsUnsupported)
        );
        // A malformed bound in a config is inconsistent, because the factory accepts none.
        let mut broken = config(None);
        broken
            .params
            .insert(PRIORITIES_PARAM.to_string(), string("3"));
        assert_eq!(
            check(&broken, &priority(int(0)), None),
            Err(SpaceError::Inconsistent(
                "the priorities param of a space config is not a positive integer"
            ))
        );
    }

    // ── Agent-level checks ──────────────────────────────────────────────────────────────────

    fn channel() -> Par { string("priority queue") }

    fn key() -> CollectionKey { CollectionKey::of_channel(&channel()).expect("a key") }

    fn channels() -> CollectionChannels { CollectionChannels::new(&HashMapOuter, SPACE, &key()) }

    fn named(name: &str) -> ParWithRandom {
        ParWithRandom {
            body: Some(string(name)),
            random_state: Blake2b512Random::create_from_bytes(name.as_bytes()).to_bytes(),
        }
    }

    async fn fixture() -> (ParRSpace, SpaceEnv) {
        let mut stores = InMemoryStoreManager::new();
        let space = ParRSpace::create(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(Matcher)),
        )
        .expect("an in-memory space");
        let env = SpaceEnv {
            store: SpaceStoreHandle::new(space.clone()),
            budget: RuntimeBudget::new(Cost::create(1_000_000, "priority queue agent test")),
            meter: ScanMeter::new(None),
            volatile: Default::default(),
            origin: Default::default(),
        };
        PriorityQueue::default()
            .prepare_create(&env, SpaceCreation {
                space: SPACE,
                config: config(None).to_par(),
                handle_pattern: BindPattern {
                    patterns: Vec::new(),
                    remainder: Some(new_freevar_var(0)),
                    free_count: 1,
                },
                handle_continuation: TaggedContinuation {
                    tagged_cont: Some(TaggedCont::ParBody(named("handle"))),
                    guard: None,
                    cost_authority: None,
                },
                random: Blake2b512Random::create_from_bytes(b"priority queue factory call"),
                authority: CostAuthority::default(),
                family: SpaceFactoryUrn {
                    outer: SpaceOuter::HashMap,
                    inner: SpaceInner::PriorityQueue,
                },
            })
            .await
            .expect("the creation stages")
            .commit()
            .await
            .expect("the creation commits");
        space.take_event_log().await;
        (space, env)
    }

    fn send(value: i64, level: Option<i64>) -> ProduceOp {
        ProduceOp {
            space: SPACE,
            key: key(),
            channel: channel(),
            data: ListParWithRandom {
                pars: vec![int(value)],
                random_state: Blake2b512Random::create_from_bytes(
                    format!("pq send {value} {level:?}").as_bytes(),
                )
                .to_bytes(),
                cost_authority: None,
                cost_stack: None,
            },
            persistent: false,
            params: level.map(|level| priority(int(level))).unwrap_or_default(),
            authority: CostAuthority::default(),
            reissued_ord: None,
        }
    }

    fn receive(name: &str, value: Option<i64>, level: Option<i64>) -> ConsumeOp {
        let pattern = match value {
            Some(value) => BindPattern {
                patterns: vec![int(value)],
                remainder: None,
                free_count: 0,
            },
            None => BindPattern {
                patterns: vec![models::rust::utils::new_freevar_par(0, Vec::new())],
                remainder: None,
                free_count: 1,
            },
        };
        ConsumeOp {
            space: SPACE,
            key: key(),
            channel: channel(),
            pattern,
            body: named(name),
            persistent: false,
            peek: false,
            guard: None,
            query: level.map(|level| QueryParams {
                payload: Vec::new(),
                named: priority(int(level)),
            }),
            authority: CostAuthority::default(),
            introduction_authority: CostAuthority::default(),
            reissued_ord: None,
        }
    }

    async fn run(env: &SpaceEnv, op: SpaceOp) -> SpaceOutcome {
        run_op(&PriorityQueue::default(), env, op)
            .await
            .expect("the operation commits")
    }

    /// The levels and payloads of the stored records, by order.
    async fn records(space: &ParRSpace) -> Vec<(i64, Option<i64>, Vec<Par>)> {
        let mut records: Vec<_> = space
            .get_data(channels().data())
            .await
            .into_iter()
            .map(|datum| {
                let decoded = record::unwrap_pq_datum(&datum.a).expect("a datum record");
                (decoded.ord, decoded.priority, decoded.datum.pars)
            })
            .collect();
        records.sort_by_key(|(ord, ..)| *ord);
        records
    }

    #[tokio::test]
    async fn space_pq_unrestricted_receive_takes_the_levels_in_ascending_order() {
        let (space, env) = fixture().await;
        for (value, level) in [(30, None), (20, Some(2)), (0, Some(0)), (10, Some(1))] {
            assert_eq!(
                run(&env, SpaceOp::Produce(send(value, level))).await,
                SpaceOutcome::default()
            );
        }
        assert_eq!(records(&space).await, vec![
            (0, None, vec![int(30)]),
            (1, Some(2), vec![int(20)]),
            (2, Some(0), vec![int(0)]),
            (3, Some(1), vec![int(10)]),
        ]);
        for expected in [0, 10, 20, 30] {
            let outcome = run(&env, SpaceOp::Consume(receive("r", None, None))).await;
            assert_eq!(outcome.fires.len(), 1);
            let bound: Vec<Par> = outcome.fires[0]
                .results
                .iter()
                .flat_map(|result| result.matched_datum.pars.clone())
                .collect();
            assert_eq!(bound, vec![int(expected)]);
        }
        assert!(records(&space).await.is_empty());
    }

    /// The body name of a continuation whose body is a string, or the empty string.
    fn body_name(continuation: &TaggedContinuation) -> String {
        match &continuation.tagged_cont {
            Some(TaggedCont::ParBody(ParWithRandom {
                body: Some(body), ..
            })) => match record::sole_expr(body) {
                Some(ExprInstance::GString(name)) => name.clone(),
                _ => String::new(),
            },
            _ => String::new(),
        }
    }

    /// The body names of the fires of `outcome`, in staging order.
    fn fired_names(outcome: &SpaceOutcome) -> Vec<String> {
        outcome
            .fires
            .iter()
            .map(|fire| body_name(&fire.continuation))
            .collect()
    }

    /// The order, the lane slot and the body name of each parked continuation, by order.
    async fn parked_names(space: &ParRSpace) -> Vec<(i64, Option<i64>, String)> {
        let mut parked: Vec<_> = space
            .get_waiting_continuations(vec![channels().data().clone()])
            .await
            .into_iter()
            .map(|waiter| {
                let decoded = record::unwrap_pq_continuation(&waiter.continuation)
                    .expect("a continuation record");
                (
                    decoded.ord,
                    decoded.priority,
                    body_name(&decoded.continuation),
                )
            })
            .collect();
        parked.sort_by_key(|(ord, ..)| *ord);
        parked
    }

    /// The readiness review S4: two waiter-lane heads can compete for one record, so the settle
    /// fires the oldest fireable head first. The records 1 and 2 wait at level 1. The
    /// unrestricted waiters b (only 0), u (only 1) and u2 (only 2) and the waiter r of the level 1
    /// (only 2) wait. The send of 0 at level 0 fires b and exposes u, which the settle fires with
    /// 1. That fire exposes both the record 2 and the waiter u2, so u2 and r both fit 2. The older
    /// of the two takes it, in both orders of their arrival.
    #[tokio::test]
    async fn space_pq_settle_fires_the_oldest_fireable_waiter_first() {
        for restricted_first in [false, true] {
            let (space, env) = fixture().await;
            run(&env, SpaceOp::Produce(send(1, Some(1)))).await;
            run(&env, SpaceOp::Produce(send(2, Some(1)))).await;
            for op in [receive("b", Some(0), None), receive("u", Some(1), None)] {
                assert_eq!(
                    run(&env, SpaceOp::Consume(op)).await,
                    SpaceOutcome::default()
                );
            }
            let competitors = match restricted_first {
                false => [receive("u2", Some(2), None), receive("r", Some(2), Some(1))],
                true => [receive("r", Some(2), Some(1)), receive("u2", Some(2), None)],
            };
            for op in competitors {
                assert_eq!(
                    run(&env, SpaceOp::Consume(op)).await,
                    SpaceOutcome::default()
                );
            }
            let outcome = run(&env, SpaceOp::Produce(send(0, Some(0)))).await;
            let (winner, loser) = match restricted_first {
                false => ("u2", ("r", Some(1))),
                true => ("r", ("u2", None)),
            };
            assert_eq!(fired_names(&outcome), vec!["b", "u", winner]);
            assert_eq!(outcome.reissue, None);
            assert!(records(&space).await.is_empty());
            assert_eq!(parked_names(&space).await, vec![(
                5,
                loser.1,
                loser.0.to_string()
            )]);
        }
    }
}
