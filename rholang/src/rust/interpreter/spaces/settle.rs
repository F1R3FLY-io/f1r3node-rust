//! The settle of a collection (P5b, F2 of decision 39, P5 design v3 A.8).
//!
//! # Why a collection settles
//!
//! A head-only collection lets only its two heads interact: the head record and the head waiter
//! (decision 11). The invariant INV of such a collection says that the two heads do not match. A
//! queue keeps INV with no extra work, because a fresh element joins the tail of its side. A stack
//! puts a fresh element on top of its side, so a fire can expose an older pair:
//!
//! ```text
//! consume W2(a), consume W(b)     waiters W2(a) < W(b), W(b) is the top waiter
//! produce h(a)                    W(b) does not match a: h is stored, the top record
//! produce d(b)                    d fires W(b), and d and W leave
//! now the top record h(a) and the top waiter W2(a) match: INV fails
//! ```
//!
//! A priority queue has the same problem across its levels. The settle of a collection fires
//! such pairs until no pair of heads matches. The disciplines with a settle rule call it in their
//! hooks: before the plan of an operation (`Discipline::settle_before`), after it
//! (`Discipline::settle_after`), and in a settle job (`Discipline::settle_job`).
//!
//! # Lift and re-offer
//!
//! The substrate fires a waiter only with an incoming side: an incoming datum (a produce-side
//! fire) or an incoming continuation (a consume-side fire). In a settle both sides of the pair are
//! stored. So the settle lifts the record out of the store and offers it again:
//!
//! 1. The lift: a consume fire of the continuation `K_lift` over the channel of the record. The
//!    COMM removes the record, as the host removes a record that a continuation consumes. The
//!    lift is never dispatched (`Staged::Lift`).
//! 2. The re-offer: a produce of the same record bytes on the same channel, with the waiter as
//!    the fired continuation. The bytes, the channel and the persist flag fix the produce
//!    identity, so the re-offer has the identity of the stored record. It registers the record's
//!    own cost authority (`Introduction::for_record`), as a re-store does (decision 45, Rule E).
//!
//! A persistent record is never removed by a COMM, so the settle does not lift it. Its re-offer
//! is a persistent produce with the same identity, and the stored record stays in place.
//!
//! `K_lift` has the pattern [`lift_pattern`] and the body `EList[GInt(c), GInt(step)]`. `c` is
//! the counter value of the round (`Cx::bumped`). `step` is the fire count of the transaction at
//! the lift (`Cx::rounds`, I-21): each lift comes right before its own fire, so no two lifts of a
//! round share a step. Its random state is `RandomPurpose::Lift.derive(r)`, where `r` is the
//! random state of the operation. A re-issued operation keeps `r` in every round (decision 43),
//! so the body holds the counter value: each round bumps the counter, so no two lifts of a deploy
//! share an identity.
//!
//! # The loop
//!
//! The settle carries at most one element in flight ([`InFlight`]): a lifted record whose fire of
//! a waiter that peeks did not spend it (decision 30). The [`SettleView`] of a discipline gives the
//! next fire that the collection owes, in the canonical order of the discipline. For a stack, it
//! is the top pair.
//!
//! ```text
//! SETTLE(view, cx, origin, x):                      ▷ x: the element in flight, or none
//!   rounds ← cx.rounds                              ▷ the fires of the transaction (I-21)
//!   loop
//!     F ← view.next_fire(cx, x)                     ▷ the next fire that the collection owes
//!     if F = none:
//!       land x                                      ▷ store it at its ord, same identity
//!       return Quiescent
//!     if F binds a stored record and x is in flight:
//!       land x; x ← none; continue                  ▷ x has no fire: its produce stores it
//!     if rounds.fired ≥ FIRE_ROUND_CAP:
//!       land x; return Reissue(RoundCap)            ▷ I-21, for the whole transaction
//!     v ← the record of F                           ▷ x itself, or a stored record
//!     if v is stored and not persistent:
//!       lift v                                      ▷ Consume K_lift, COMM: Staged::Lift
//!     re-offer v to F.waiter                        ▷ the identity and the authority of v
//!     match rounds.after_fire(spent = ¬v.persist ∧ F.waiter does not peek,
//!                             stays = F.waiter.persist):
//!       Done      → x ← none                        ▷ v is spent
//!       Continue  → x ← v when v was lifted, else none   ▷ a persistent v stays stored
//!       Reissue c → x ← v when v was lifted; land x; return Reissue(c)
//! ```
//!
//! The settle ends `Quiescent` when no pair of heads matches and no element is in flight. A
//! caller turns `Reissue(c)` into a re-issue ([`settle_before_plan`], [`settle_after_plan`],
//! [`settle_job`]):
//!
//! * Before the plan, the element of the operation is not inserted yet, so the operation itself
//!   re-issues. A persistent operation keeps its first-round `ord` (decision 43, S2).
//! * After the plan and in a settle job, the element of the operation is spent, stored or
//!   parked. So a `SpaceOp::Settle` re-issues the rest of the settle.
//!
//! A stored divergent pair (a persistent record and a persistent waiter that match) re-issues a
//! settle after each round, as a divergent pair of an operation re-issues the operation (A10). It
//! repeats until the phlo of the deploy runs out, as the host repeats such a pair.
//!
//! # Termination
//!
//! Let `μ` be the stored records that are not persistent, plus the stored waiters that are not
//! persistent, plus one for an element in flight. Each fire of a settle lowers `μ` by at least
//! one, or the settle stops at a divergent pair. The settle shares the fire count of its
//! transaction (I-21), so a call that starts after `f` fires of the transaction makes at most
//! `min(μ, FIRE_ROUND_CAP - f)` fires (`StackDiscipline.v`, `stack_settle_terminates`).
//!
//! # Rule E (decision 45)
//!
//! A fire of a waiter that peeks removes the non-persistent records that it binds, as the host
//! does. The incoming record of a re-offer stays in flight (decision 30), and the settle stores it
//! again at its order with its identity when it has no further fire. This settle runs only on a
//! collection without a join group: the join engine of P5c (`spaces::join`) runs every collection
//! that a join reaches, with its own cascade and its own second sends for the other stored records
//! of a fire. So a plan over a group of more than one channel is `Inconsistent` here, and a
//! one-channel plan binds no other stored record.
//!
//! # Fire rounds
//!
//! A settle counts its fires on the fire count of its transaction (`Cx::rounds`, I-21, decision
//! 49). The settle before the plan, the plan and the settle after the plan share that count, so
//! one transaction of a stack operation fires at most `FIRE_ROUND_CAP` times. `open_plan`
//! re-issues an operation whose settle filled the transaction. A settle job starts at the count 0,
//! because it is a transaction of its own. The bound is deterministic, so play and replay agree.
//!
//! # Metering
//!
//! The legacy charges of decision 45 (Option P) apply to each staged operation, because a settle
//! stages only through `Cx`.

use std::sync::LazyLock;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{
    BindPattern, CostAuthority, ListParWithRandom, Par, ParWithRandom, TaggedContinuation,
};
use models::rust::utils::{new_elist_par, new_freevar_var, new_gint_par};
use rspace_plus_plus::rspace::internal::{ConsumeCandidate, Datum};

use super::agent::{ConsumeOp, ProduceOp, SettleOp, SpaceOp, SpaceReissue};
use super::errors::SpaceError;
use super::generic::{Cx, OfferFire, OpView, ParCandidate, ParDatum, ParWaiter};
use super::namespace::RandomPurpose;
use super::record;
use crate::rust::interpreter::reified::costs::{FireStep, ReissueCause, FIRE_ROUND_CAP};

// Disabled by I-21 (one fire count for each transaction): the step of a lift is the fire count
// of the transaction (`Cx::rounds`), which the settles before and after the plan share. So the
// two settles of one round never share a step, and no offset is needed.
// /// The first `K_lift` step of a settle before the plan of an operation, and of a settle job.
// pub const FIRST_STEP_BEFORE: i64 = 0;
//
// /// The first `K_lift` step of a settle after the plan of an operation. A settle call lifts at
// /// most once for each of its at most `FIRE_ROUND_CAP` fires, so the steps of the settle before
// /// the plan stay below this value, and the two settles of one round never share a step.
// pub const FIRST_STEP_AFTER: i64 = FIRE_ROUND_CAP as i64;

/// The pattern of a `K_lift` continuation: a remainder that binds the whole record as one list,
/// the shape of the handle pattern (`reified::factory::handle_pattern`). It matches every record
/// of every arity.
pub fn lift_pattern() -> BindPattern {
    BindPattern {
        patterns: Vec::new(),
        remainder: Some(new_freevar_var(0)),
        free_count: 1,
    }
}

/// The body of a `K_lift` continuation: `EList[GInt(counter), GInt(step)]`. `counter` is the
/// value of the collection counter before the bump of the round, and `step` is unique among the
/// lifts of the round.
pub fn lift_body(counter: i64, step: i64) -> Par {
    new_elist_par(
        vec![
            new_gint_par(counter, Vec::new(), false),
            new_gint_par(step, Vec::new(), false),
        ],
        Vec::new(),
        false,
        None,
        Vec::new(),
        false,
    )
}

/// The continuation `K_lift` of the lift with the counter value `counter` and the step `step`.
/// `random` is `RandomPurpose::Lift.derive(r)` for the random state `r` of the operation, and
/// `cost_authority` is the cost authority of the operation. It has no guard.
pub fn lift_continuation(
    counter: i64,
    step: i64,
    random: &Blake2b512Random,
    cost_authority: Option<CostAuthority>,
) -> TaggedContinuation {
    TaggedContinuation {
        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(lift_body(counter, step)),
            random_state: random.to_bytes(),
        })),
        guard: None,
        cost_authority,
    }
}

/// The template of the lift pattern.
static LIFT_PATTERN: LazyLock<BindPattern> = LazyLock::new(lift_pattern);

/// The operation that runs a settle: what each `K_lift` of the settle takes from it.
#[derive(Clone, Copy, Debug)]
pub struct SettleOrigin<'a> {
    /// The random state of the operation. Each `K_lift` takes the random state
    /// `RandomPurpose::Lift.derive(r)` of the random state `r` that these bytes encode.
    pub random_state: &'a [u8],
    /// The cost authority of each `K_lift`: the cost authority of the datum of a produce, or the
    /// stamped cost authority of a consume or of a settle job, as the counter continuation of
    /// the operation takes it (`record::OperationAuthority`).
    pub authority: Option<&'a CostAuthority>,
    // Disabled by I-21: the step of a lift is the fire count of the transaction (`Cx::rounds`).
    // /// The step of the first lift of the settle ([`FIRST_STEP_BEFORE`] or
    // /// [`FIRST_STEP_AFTER`]).
    // pub first_step: i64,
}

impl<'a> SettleOrigin<'a> {
    /// The origin of a settle around the operation `op`.
    pub fn of_operation(op: OpView<'a>) -> Self {
        match op {
            OpView::Produce(op) => SettleOrigin {
                random_state: &op.data.random_state,
                authority: op.data.cost_authority.as_ref(),
            },
            OpView::Consume(op) => SettleOrigin {
                random_state: &op.body.random_state,
                authority: record::stamped(&op.authority),
            },
        }
    }

    /// The origin of the settle job `op`. A settle job runs one settle and no plan, in a
    /// transaction of its own, so its lifts take the steps from 0 on.
    pub fn of_settle(op: &'a SettleOp) -> Self {
        SettleOrigin {
            random_state: &op.random_state,
            authority: record::stamped(&op.authority),
        }
    }
}

/// A record that a settle lifted out of its collection and offered again, and that the fire of
/// a waiter that peeks did not spend (decision 30). It is not persistent, because a settle lifts
/// only records that are not persistent.
#[derive(Clone, Debug)]
pub struct Lifted {
    /// The channel of the record.
    pub channel: Par,
    /// The record as the store held it: its envelope and payload bytes, its persist flag and its
    /// produce identity. The settle stores it again with these bytes, so with this identity.
    pub datum: ParDatum,
}

/// The element that a settle carries between two of its fires: a [`Lifted`] record, or nothing.
/// A settle starts with nothing in flight from every hook of a discipline. It ends with nothing
/// in flight, because it stores a lifted record again before it returns.
#[derive(Clone, Debug, Default)]
pub struct InFlight {
    lifted: Option<Lifted>,
}

impl InFlight {
    /// Nothing in flight.
    pub const fn none() -> Self { InFlight { lifted: None } }

    /// The record `lifted` in flight.
    pub fn lifted(lifted: Lifted) -> Self {
        InFlight {
            lifted: Some(lifted),
        }
    }

    /// The record in flight, or `None`.
    pub fn record(&self) -> Option<&Lifted> { self.lifted.as_ref() }

    /// True when a record is in flight.
    pub fn is_some(&self) -> bool { self.lifted.is_some() }

    /// Takes the record in flight and leaves nothing in flight.
    fn take(&mut self) -> Option<Lifted> { self.lifted.take() }
}

/// Where the record of a planned fire is: stored in the staged view, or in flight.
#[derive(Clone, Debug)]
pub enum BoundRecord {
    /// A stored record: a copy of it, and its index in the staged view of its channel, which is
    /// the channel of its place in the group of the plan.
    Stored {
        /// The copy of the stored record.
        datum: ParDatum,
        /// The index of the record in the staged view of its channel.
        view_index: i32,
    },
    /// The record in flight of the settle ([`InFlight`]).
    InFlight,
}

/// One record that a planned fire binds, with the values that the waiter binds from it.
#[derive(Clone, Debug)]
pub struct Bound {
    /// The record.
    pub record: BoundRecord,
    /// The values that the waiter binds from the payload of the record: the spatial match that
    /// the guard of the waiter admitted (decision 37).
    pub matched: ListParWithRandom,
}

/// One fire that a collection owes: a stored waiter and the records that it binds, in the order
/// of the channel group of the waiter. A [`SettleView`] gives it, and [`settle`] stages it.
#[derive(Clone, Debug)]
pub struct FirePlan {
    /// The channel group of the waiter, in group order. A plan of a single collection has one
    /// channel: the data channel of the collection.
    pub group: Vec<Par>,
    /// A copy of the waiting continuation in the staged view of the group.
    pub waiter: ParWaiter,
    /// One bound record for each channel of the group, in group order: `bound[i]` binds
    /// `group[i]`.
    pub bound: Vec<Bound>,
}

/// A plan over one channel, as the engine stages it.
struct ChannelFire {
    channel: Par,
    waiter: ParWaiter,
    record: BoundRecord,
    matched: ListParWithRandom,
}

impl FirePlan {
    /// The plan as a fire over one channel. The engine stages a produce-side fire over one
    /// channel only (`Cx::fire_offer_on`), so another group, a group that does not line up with
    /// its bound records, or a waiter with several binds is `Inconsistent`.
    fn into_channel_fire(self) -> Result<ChannelFire, SpaceError> {
        let FirePlan {
            group,
            waiter,
            bound,
        } = self;
        match (
            <[Par; 1]>::try_from(group),
            <[Bound; 1]>::try_from(bound),
            waiter.patterns.len(),
        ) {
            (Ok([channel]), Ok([Bound { record, matched }]), 1) => Ok(ChannelFire {
                channel,
                waiter,
                record,
                matched,
            }),
            _ => Err(SpaceError::Inconsistent(
                "a settle fire binds one record on one channel",
            )),
        }
    }
}

/// The canonical order in which a collection settles: the next fire that it owes.
pub trait SettleView {
    /// The next fire that the collection owes, or `None` when no pair of heads matches. When a
    /// record is in flight (`in_flight`), the view treats it as an entry of its side, at its order.
    /// A plan that binds that record marks it [`BoundRecord::InFlight`]. The view stages nothing.
    fn next_fire(
        &self,
        cx: &mut Cx<'_, '_>,
        in_flight: &InFlight,
    ) -> Result<Option<FirePlan>, SpaceError>;
}

/// How a settle ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettleEnd {
    /// No pair of heads matches, and no element is in flight.
    Quiescent,
    /// The settle stopped after a fire with this cause, and it owes more fires: the caller
    /// re-issues the rest (A10, I-21).
    Reissue(ReissueCause),
}

/// The record of a fire, as the re-offer offers it.
struct Incoming {
    datum: ParDatum,
    lifted: bool,
}

/// Runs the settle of the collection of `cx` in the canonical order of `view`, from the element
/// in flight `in_flight` (see the module documentation for the loop). Each staged operation goes
/// through `Cx`, so it takes the legacy charges of decision 45 and registers its introduction.
pub fn settle<V: SettleView>(
    view: &V,
    cx: &mut Cx<'_, '_>,
    origin: &SettleOrigin<'_>,
    mut in_flight: InFlight,
) -> Result<SettleEnd, SpaceError> {
    // Disabled by I-21 (one fire count for each transaction, `Cx::rounds`):
    // let mut rounds = FireRounds::new();
    let mut lift_random: Option<Blake2b512Random> = None;
    loop {
        let Some(plan) = view.next_fire(cx, &in_flight)? else {
            land(cx, in_flight)?;
            return Ok(SettleEnd::Quiescent);
        };
        let ChannelFire {
            channel,
            waiter,
            record,
            matched,
        } = plan.into_channel_fire()?;
        if matches!(record, BoundRecord::Stored { .. }) && in_flight.is_some() {
            // The record in flight has no fire, so its produce stores it. The view then plans
            // again with the record in its place.
            land(cx, std::mem::take(&mut in_flight))?;
            continue;
        }
        if cx.rounds().fired() >= FIRE_ROUND_CAP {
            land(cx, in_flight)?;
            return Ok(SettleEnd::Reissue(ReissueCause::RoundCap));
        }
        let incoming = match record {
            BoundRecord::InFlight => match in_flight.take() {
                Some(lifted) => Incoming {
                    datum: lifted.datum,
                    lifted: true,
                },
                None => {
                    return Err(SpaceError::Inconsistent(
                        "a settle fire binds the element in flight, but none is in flight",
                    ))
                }
            },
            BoundRecord::Stored { datum, view_index } => {
                let lifted = !datum.persist;
                if lifted {
                    let step = lift_step(cx.rounds().fired())?;
                    lift(
                        cx,
                        &channel,
                        &datum,
                        view_index,
                        origin,
                        step,
                        &mut lift_random,
                    )?;
                }
                Incoming { datum, lifted }
            }
        };
        let Incoming { datum, lifted } = incoming;
        let spent = !datum.persist && waiter.peeks.is_empty();
        let stays = waiter.persist;
        let introduction = cx.introduction().for_record(&datum.a)?;
        let step = cx.rounds().after_fire(spent, stays);
        let fire = OfferFire {
            persistent: datum.persist,
            waiter,
            matched,
        };
        match lifted && !spent {
            true => {
                // The lifted record stays in flight after this fire, so the fire takes copies.
                let data = datum.a.clone();
                let removed = datum.a.clone();
                let source = datum.source.clone();
                cx.fire_offer_on(&channel, fire, data, removed, source, &introduction)?;
                in_flight = InFlight::lifted(Lifted { channel, datum });
            }
            false => {
                let removed = datum.a.clone();
                let Datum { a, source, .. } = datum;
                cx.fire_offer_on(&channel, fire, a, removed, source, &introduction)?;
            }
        }
        match step {
            FireStep::Done | FireStep::Continue => {}
            FireStep::Reissue(cause) => {
                land(cx, in_flight)?;
                return Ok(SettleEnd::Reissue(cause));
            }
        }
    }
}

/// The step of the lift of the next fire: the fires of the transaction so far (I-21).
fn lift_step(fired: usize) -> Result<i64, SpaceError> {
    i64::try_from(fired).map_err(|_| SpaceError::Inconsistent("a lift step overflows i64"))
}

/// Stores the record in flight again on its channel, with its own bytes, so with its produce
/// identity, and with its own cost authority (`Cx::restore_on`). Nothing in flight stages
/// nothing.
fn land(cx: &mut Cx<'_, '_>, mut in_flight: InFlight) -> Result<(), SpaceError> {
    match in_flight.take() {
        Some(Lifted { channel, datum }) => cx.restore_on(&channel, datum.a),
        None => Ok(()),
    }
}

/// Stages the lift of the stored record `datum` at the index `view_index` of the staged view of
/// `channel`: a consume fire of `K_lift` with the step `step`, which removes the record and is
/// never dispatched. The random state of `K_lift` is derived once for a settle and kept in
/// `lift_random`.
fn lift(
    cx: &mut Cx<'_, '_>,
    channel: &Par,
    datum: &ParDatum,
    view_index: i32,
    origin: &SettleOrigin<'_>,
    step: i64,
    lift_random: &mut Option<Blake2b512Random>,
) -> Result<(), SpaceError> {
    let counter = cx.bumped().ok_or(SpaceError::Inconsistent(
        "a settle runs in a round without a collection counter",
    ))?;
    let matched = cx
        .bind(&LIFT_PATTERN, &datum.a)
        .ok_or(SpaceError::Inconsistent(
            "a stored record does not match the lift pattern",
        ))?;
    let random = lift_random.get_or_insert_with(|| {
        RandomPurpose::Lift.derive(&Blake2b512Random::from_bytes(origin.random_state))
    });
    let cost_authority = origin.authority.cloned();
    let continuation = lift_continuation(counter, step, random, cost_authority);
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
    cx.lift(channels, patterns, continuation, candidates)
}

/// The settle that a discipline owes before the plan of the operation `op`, in the order of
/// `view`. It starts the fire count of the transaction (I-21). A settle that reaches the round
/// cap or a divergent pair re-issues the operation itself, because its element is not inserted
/// yet ([`reissued_operation`]).
pub fn settle_before_plan<V: SettleView>(
    view: &V,
    cx: &mut Cx<'_, '_>,
    op: OpView<'_>,
) -> Result<Option<SpaceReissue>, SpaceError> {
    let origin = SettleOrigin::of_operation(op);
    match settle(view, cx, &origin, InFlight::none())? {
        SettleEnd::Quiescent => Ok(None),
        SettleEnd::Reissue(cause) => {
            let reissued = reissued_operation(op, cx.bumped());
            Ok(Some(cx.reissue(reissued, cause)))
        }
    }
}

/// The settle that a discipline owes after the plan of the operation `op`, in the order of
/// `view`. It continues the fire count of the transaction after the plan (I-21). The element of
/// the operation is spent, stored or parked, so a settle that reaches the round cap or a
/// divergent pair re-issues a `SpaceOp::Settle` ([`settle_operation`]).
pub fn settle_after_plan<V: SettleView>(
    view: &V,
    cx: &mut Cx<'_, '_>,
    op: OpView<'_>,
) -> Result<Option<SpaceReissue>, SpaceError> {
    let origin = SettleOrigin::of_operation(op);
    match settle(view, cx, &origin, InFlight::none())? {
        SettleEnd::Quiescent => Ok(None),
        SettleEnd::Reissue(cause) => {
            let settle_op = settle_operation(op);
            Ok(Some(cx.reissue(SpaceOp::Settle(settle_op), cause)))
        }
    }
}

/// The plan of the settle job `op`, in the order of `view`: one settle, which starts the fire
/// count of its own transaction (I-21). A settle that reaches the round cap or a divergent pair
/// re-issues the same settle, with no copy.
pub fn settle_job<V: SettleView>(
    view: &V,
    cx: &mut Cx<'_, '_>,
    op: SettleOp,
) -> Result<Option<SpaceReissue>, SpaceError> {
    let end = settle(view, cx, &SettleOrigin::of_settle(&op), InFlight::none())?;
    match end {
        SettleEnd::Quiescent => Ok(None),
        SettleEnd::Reissue(cause) => Ok(Some(cx.reissue(SpaceOp::Settle(op), cause))),
    }
}

/// The re-issue of the operation `op`, whose round bumped the counter from `bumped`, when a
/// settle before its plan cannot complete. The operation re-issues as the program issued it,
/// with the order rule of decision 43: a persistent operation carries the order of its first
/// round, which is its carried order or else `bumped`, and an operation that is not persistent
/// carries none.
pub fn reissued_operation(op: OpView<'_>, bumped: Option<i64>) -> SpaceOp {
    let first_round = |reissued: Option<i64>| reissued.or(bumped).unwrap_or(0);
    match op {
        OpView::Produce(op) => SpaceOp::Produce(ProduceOp {
            space: op.space,
            key: op.key.clone(),
            channel: op.channel.clone(),
            data: op.data.clone(),
            persistent: op.persistent,
            params: op.params.clone(),
            authority: op.authority.clone(),
            reissued_ord: op.persistent.then(|| first_round(op.reissued_ord)),
        }),
        OpView::Consume(op) => SpaceOp::Consume(ConsumeOp {
            space: op.space,
            key: op.key.clone(),
            channel: op.channel.clone(),
            pattern: op.pattern.clone(),
            body: op.body.clone(),
            persistent: op.persistent,
            peek: op.peek,
            guard: op.guard.clone(),
            query: op.query.clone(),
            authority: op.authority.clone(),
            introduction_authority: op.introduction_authority.clone(),
            reissued_ord: op.persistent.then(|| first_round(op.reissued_ord)),
        }),
    }
}

/// The settle job that continues the settle after the operation `op`: the space, the key and
/// the channel of the operation, its random state, its cost authority and its introduction
/// authority. The settle job bumps the counter with this random state and these authorities,
/// as a round of the operation does.
pub fn settle_operation(op: OpView<'_>) -> SettleOp {
    match op {
        OpView::Produce(op) => SettleOp {
            space: op.space,
            key: op.key.clone(),
            channel: op.channel.clone(),
            random_state: op.data.random_state.clone(),
            authority: match &op.data.cost_authority {
                Some(authority) => authority.clone(),
                None => CostAuthority::default(),
            },
            introduction_authority: op.authority.clone(),
            cascade: None,
        },
        OpView::Consume(op) => SettleOp {
            space: op.space,
            key: op.key.clone(),
            channel: op.channel.clone(),
            random_state: op.body.random_state.clone(),
            authority: op.authority.clone(),
            introduction_authority: op.introduction_authority.clone(),
            cascade: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::{CostRegion, EList, Expr};
    use models::rust::utils::{new_freevar_par, new_gstring_par};
    use rspace_plus_plus::rspace::r#match::Match;
    use rspace_plus_plus::rspace::trace::event::{Consume, Produce};

    use super::*;
    use crate::rust::interpreter::matcher::r#match::Matcher;
    use crate::rust::interpreter::spaces::agent::QueryParams;
    use crate::rust::interpreter::spaces::namespace::{
        CollectionKey, Durability, NativeSpace, SID_LEN,
    };

    const SPACE: NativeSpace = NativeSpace {
        sid: [9; SID_LEN],
        durability: Durability::Durable,
    };

    fn int(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

    fn string(value: &str) -> Par { new_gstring_par(value.to_string(), Vec::new(), false) }

    fn region(byte: u8) -> CostAuthority {
        CostAuthority {
            regions: vec![CostRegion {
                instance_id: vec![byte; 32],
                signature: None,
            }],
        }
    }

    fn channel() -> Par { string("stack") }

    fn key() -> CollectionKey { CollectionKey::of_channel(&channel()).expect("a key") }

    fn produce_op(persistent: bool, reissued_ord: Option<i64>) -> ProduceOp {
        ProduceOp {
            space: SPACE,
            key: key(),
            channel: channel(),
            data: ListParWithRandom {
                pars: vec![int(5)],
                random_state: vec![1, 2, 3],
                cost_authority: Some(region(2)),
                cost_stack: None,
            },
            persistent,
            params: BTreeMap::from([("priority".to_string(), int(1))]),
            authority: region(3),
            reissued_ord,
        }
    }

    fn consume_op(persistent: bool, reissued_ord: Option<i64>) -> ConsumeOp {
        ConsumeOp {
            space: SPACE,
            key: key(),
            channel: channel(),
            pattern: BindPattern {
                patterns: vec![new_freevar_par(0, Vec::new())],
                remainder: None,
                free_count: 1,
            },
            body: ParWithRandom {
                body: Some(string("body")),
                random_state: vec![4, 5],
            },
            persistent,
            peek: false,
            guard: Some(int(1)),
            query: Some(QueryParams {
                payload: vec![int(7)],
                named: BTreeMap::from([("priority".to_string(), int(0))]),
            }),
            authority: region(4),
            introduction_authority: region(5),
            reissued_ord,
        }
    }

    /// A stored datum record with the order `ord` and the payload `payload`.
    fn record_of(ord: i64, payload: Vec<Par>) -> ListParWithRandom {
        record::wrap_datum(ord, &ListParWithRandom {
            pars: payload,
            random_state: vec![8],
            cost_authority: None,
            cost_stack: None,
        })
    }

    #[test]
    fn space_settle_lift_pattern_binds_every_record() {
        for payload in [Vec::new(), vec![int(1)], vec![int(1), string("x"), int(3)]] {
            let record = record_of(4, payload);
            let matched = Matcher
                .get(&lift_pattern(), &record)
                .expect("the lift pattern binds every record");
            // The remainder binds the whole record, the envelope included, as one list.
            match matched.pars.as_slice() {
                [Par { exprs, .. }] => match exprs.as_slice() {
                    [Expr {
                        expr_instance: Some(ExprInstance::EListBody(EList { ps, .. })),
                    }] => assert_eq!(ps, &record.pars),
                    other => panic!("the remainder binds a list: {other:?}"),
                },
                other => panic!("the lift pattern binds one value: {other:?}"),
            }
        }
        assert_eq!(*LIFT_PATTERN, lift_pattern());
    }

    #[test]
    fn space_settle_lift_identity_is_unique_per_counter_and_step() {
        let random = RandomPurpose::Lift.derive(&Blake2b512Random::from_bytes(&[1, 2, 3]));
        let data = string("data channel");
        let identity = |counter: i64, step: i64| {
            Consume::create(
                &vec![data.clone()],
                &vec![lift_pattern()],
                &lift_continuation(counter, step, &random, Some(region(1))),
                false,
            )
            .hash
        };
        let mut seen = std::collections::BTreeSet::new();
        for counter in 0..4 {
            // Disabled by I-21: the steps of a round are the fire counts of its transaction.
            // for step in [0, 1, FIRST_STEP_AFTER, FIRST_STEP_AFTER + 1] {
            for step in [0, 1, 2, 3] {
                assert!(
                    seen.insert(identity(counter, step).bytes()),
                    "the counter {counter} and the step {step} make a new identity"
                );
            }
        }
        assert_eq!(
            identity(2, 1),
            identity(2, 1),
            "the identity is deterministic"
        );
        // The body holds the two values and no other part, and the continuation has no guard.
        let continuation = lift_continuation(7, 3, &random, None);
        assert_eq!(continuation.guard, None);
        assert_eq!(continuation.cost_authority, None);
        match continuation.tagged_cont {
            Some(TaggedCont::ParBody(ParWithRandom { body, random_state })) => {
                assert_eq!(body, Some(lift_body(7, 3)));
                assert_eq!(random_state, random.to_bytes());
            }
            other => panic!("K_lift has a body: {other:?}"),
        }
    }

    #[test]
    fn space_settle_origin_takes_the_random_state_and_authority_of_its_operation() {
        let produce = produce_op(false, None);
        let origin = SettleOrigin::of_operation(OpView::Produce(&produce));
        assert_eq!(origin.random_state, produce.data.random_state.as_slice());
        assert_eq!(origin.authority, Some(&region(2)));
        // Disabled by I-21: a settle has no first step, its lifts take the fire count of the
        // transaction.
        // assert_eq!(origin.first_step, FIRST_STEP_AFTER);
        let consume = consume_op(true, None);
        let origin = SettleOrigin::of_operation(OpView::Consume(&consume));
        assert_eq!(origin.random_state, consume.body.random_state.as_slice());
        assert_eq!(origin.authority, Some(&region(4)));
        // Disabled by I-21 (see above):
        // assert_eq!(origin.first_step, FIRST_STEP_BEFORE);
        let unsigned = ConsumeOp {
            authority: CostAuthority::default(),
            ..consume_op(false, None)
        };
        assert_eq!(
            SettleOrigin::of_operation(OpView::Consume(&unsigned)).authority,
            None,
            "an empty authority is not stamped"
        );
        let settle_op = settle_operation(OpView::Produce(&produce));
        let origin = SettleOrigin::of_settle(&settle_op);
        assert_eq!(origin.random_state, produce.data.random_state.as_slice());
        assert_eq!(origin.authority, Some(&region(2)));
        // Disabled by I-21: the two settles of one round share the fire count of the
        // transaction, so they never share a step without an offset.
        // assert_eq!(origin.first_step, FIRST_STEP_BEFORE);
        // // The two settles of one round never share a step.
        // assert_eq!(FIRST_STEP_AFTER, i64::try_from(FIRE_ROUND_CAP).expect("a small cap"));
    }

    #[test]
    fn space_settle_reissued_operation_keeps_the_first_round_ord_of_a_persistent_operation() {
        // Decision 43: a persistent operation carries its first-round order: its carried order,
        // or else the order of its bump. An operation that is not persistent carries none.
        let cases = [
            (true, None, Some(4)),
            (true, Some(2), Some(2)),
            (false, None, None),
            (false, Some(2), None),
        ];
        for (persistent, carried, expected) in cases {
            let produce = produce_op(persistent, carried);
            assert_eq!(
                reissued_operation(OpView::Produce(&produce), Some(4)),
                SpaceOp::Produce(ProduceOp {
                    reissued_ord: expected,
                    ..produce.clone()
                }),
                "produce, persistent {persistent}, carried {carried:?}"
            );
            let consume = consume_op(persistent, carried);
            assert_eq!(
                reissued_operation(OpView::Consume(&consume), Some(4)),
                SpaceOp::Consume(ConsumeOp {
                    reissued_ord: expected,
                    ..consume.clone()
                }),
                "consume, persistent {persistent}, carried {carried:?}"
            );
        }
        // Without a counter every order is 0.
        let produce = produce_op(true, None);
        assert_eq!(
            reissued_operation(OpView::Produce(&produce), None),
            SpaceOp::Produce(ProduceOp {
                reissued_ord: Some(0),
                ..produce.clone()
            })
        );
    }

    #[test]
    fn space_settle_operation_carries_the_settle_inputs_of_its_operation() {
        let produce = produce_op(true, Some(1));
        assert_eq!(settle_operation(OpView::Produce(&produce)), SettleOp {
            space: SPACE,
            key: key(),
            channel: channel(),
            random_state: produce.data.random_state.clone(),
            authority: region(2),
            introduction_authority: region(3),
            cascade: None,
        });
        let unsigned = ProduceOp {
            data: ListParWithRandom {
                cost_authority: None,
                ..produce.data.clone()
            },
            ..produce
        };
        assert_eq!(
            settle_operation(OpView::Produce(&unsigned)).authority,
            CostAuthority::default()
        );
        let consume = consume_op(false, None);
        assert_eq!(settle_operation(OpView::Consume(&consume)), SettleOp {
            space: SPACE,
            key: key(),
            channel: channel(),
            random_state: consume.body.random_state.clone(),
            authority: region(4),
            introduction_authority: region(5),
            cascade: None,
        });
    }

    fn waiter_of(binds: usize) -> ParWaiter {
        let patterns = vec![lift_pattern(); binds];
        let continuation = TaggedContinuation::default();
        ParWaiter {
            source: Consume::create(&vec![channel(); binds], &patterns, &continuation, false),
            patterns,
            continuation,
            persist: false,
            peeks: Default::default(),
        }
    }

    fn bound() -> Bound {
        Bound {
            record: BoundRecord::InFlight,
            matched: ListParWithRandom::default(),
        }
    }

    #[test]
    fn space_settle_plan_must_bind_one_record_on_one_channel() {
        let plan = |channels: usize, records: usize, binds: usize| FirePlan {
            group: vec![channel(); channels],
            waiter: waiter_of(binds),
            bound: (0..records).map(|_| bound()).collect(),
        };
        assert!(plan(1, 1, 1).into_channel_fire().is_ok());
        for (channels, records, binds) in [(2, 2, 2), (1, 2, 1), (2, 1, 1), (1, 1, 2), (0, 0, 1)] {
            assert_eq!(
                plan(channels, records, binds).into_channel_fire().err(),
                Some(SpaceError::Inconsistent(
                    "a settle fire binds one record on one channel"
                )),
                "{channels} channels, {records} records, {binds} binds"
            );
        }
    }

    #[test]
    fn space_settle_in_flight_holds_at_most_one_lifted_record() {
        let mut in_flight = InFlight::none();
        assert!(!in_flight.is_some());
        assert!(in_flight.record().is_none());
        let datum = ParDatum {
            a: record_of(3, vec![int(1)]),
            persist: false,
            source: Produce::create(&channel(), &record_of(3, vec![int(1)]), false),
        };
        in_flight = InFlight::lifted(Lifted {
            channel: channel(),
            datum: datum.clone(),
        });
        assert!(in_flight.is_some());
        assert_eq!(
            in_flight.record().map(|lifted| lifted.datum.a.clone()),
            Some(datum.a.clone())
        );
        let taken = in_flight.take().expect("a lifted record");
        assert_eq!(taken.datum.source, datum.source);
        assert!(!in_flight.is_some(), "taking leaves nothing in flight");
        assert!(!InFlight::default().is_some());
    }
}
