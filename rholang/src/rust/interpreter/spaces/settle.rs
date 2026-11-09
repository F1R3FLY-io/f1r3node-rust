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

// Added by decision 124 (plan v4 C7.11, ruling RADD2 1.3): `restored_send` re-issues a send
// with no params.
use std::collections::BTreeMap;
use std::sync::LazyLock;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
// Added by decision 124 (plan v2 C7.10): the body test of `machinery_shape` reads one expression.
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{
    BindPattern, CostAuthority, ListParWithRandom, Par, ParWithRandom, TaggedContinuation,
};
use models::rust::utils::{new_elist_par, new_freevar_var, new_gint_par};
use rspace_plus_plus::rspace::internal::{ConsumeCandidate, Datum};
// Added by decision 124 (plan v4 C7.11, ruling RADD 1.4): the tail read of a head row.
use rspace_plus_plus::rspace::space_store_handle::TailRead;

use super::agent::{ConsumeOp, ProduceOp, SettleOp, SpaceOp, SpaceReissue};
// Added by decision 124 (plan v2 C7.10): `machinery_shape` tests the retire pattern.
use super::disciplines::scan::RETIRE_PATTERN;
use super::errors::SpaceError;
// Changed by decision 102 (plan v4 C7.22): the settle names the datum that a fire offers.
// use super::generic::{Cx, OfferFire, OpView, ParCandidate, ParDatum, ParWaiter};
// Changed by decision 124 (plan v4 C7.11, ruling RADD 1.4): the settle views read the store
// through `StoreView` and name their discipline.
use super::generic::{
    Cx, Discipline, OfferFire, Offered, OpView, ParCandidate, ParDatum, ParWaiter, StoreView,
};
// Added by decision 124 (plan v4 C7.11, ruling RADD2 1.3): `restored_send` names the collection
// of the receive.
use super::namespace::CollectionKey;
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

/// Added by decision 124 (plan v2 C7.10, decision 100): true when `waiter` is an inert
/// machinery waiter, a `K_retire` or a `K_lift` that a refused machinery row parked (CR-16). Its
/// one pattern is the retire or the lift pattern, and its body is the body of a `K_retire` or
/// of a `K_lift`. A reader of waiters skips it. Any other waiter that a reader cannot decode
/// still fails.
pub fn machinery_shape(waiter: &ParWaiter) -> bool {
    match waiter.patterns.as_slice() {
        [pattern] if *pattern == *RETIRE_PATTERN || *pattern == *LIFT_PATTERN => {
            machinery_body(&waiter.continuation)
        }
        _ => false,
    }
}

/// Added by decision 124 (plan v2 C7.10): true when the body of `continuation` is the body of a
/// `K_retire`, one byte array (`retire_continuation`), or of a `K_lift`, a list of two integers
/// ([`lift_body`]). The body of a wrapped continuation record is a list of four items.
fn machinery_body(continuation: &TaggedContinuation) -> bool {
    let body = match &continuation.tagged_cont {
        Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(body), ..
        })) => body,
        Some(TaggedCont::ParBody(ParWithRandom { body: None, .. }))
        | Some(TaggedCont::ScalaBodyRef(_))
        | None => return false,
    };
    match record::sole_expr(body) {
        Some(ExprInstance::GByteArray(_)) => true,
        Some(ExprInstance::EListBody(list)) => match list.ps.as_slice() {
            [counter, step] => matches!(
                (record::sole_expr(counter), record::sole_expr(step)),
                (Some(ExprInstance::GInt(_)), Some(ExprInstance::GInt(_)))
            ),
            _ => false,
        },
        _ => false,
    }
}

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
    /// Added by decision 124 (plan v4 C7.11, ruling RADD 1.4): the discipline of the collection.
    /// The settle offer gives it to its head search (`Cx::head_offer_search`).
    type Discipline: Discipline;

    /// The next fire that the collection owes, or `None` when no pair of heads matches. When a
    /// record is in flight (`in_flight`), the view treats it as an entry of its side, at its order.
    /// A plan that binds that record marks it [`BoundRecord::InFlight`]. The view stages nothing.
    // Changed by decision 124 (plan v4 C7.11, ruling RADD 1.4): the reads of this fn go through
    // the view `view` in place of `cx`: the staged view in the settle loop, and the store of the
    // commit in a tail read. The comment above a changed line keeps its text before.
    // fn next_fire(
    fn next_fire<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
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
                // Added by decision 124 (L6, CR30 D-b): the fire takes the lease of the stored
                // datum that the check proved (VJ V11): after the re-plan and the cap
                // (the checks in `next_fire` of stack.rs and priority_queue.rs).
                cx.lease_stored(1);
                let lifted = !datum.persist;
                if lifted {
                    let step = lift_step(cx.rounds().fired())?;
                    // Changed by decision 124 (plan v4 C7.11, ruling RADD2 1.6): the lift names
                    // the discipline of the view. The text before:
                    // lift(
                    lift::<V::Discipline>(
                        cx,
                        &channel,
                        &datum,
                        view_index,
                        origin,
                        step,
                        &mut lift_random,
                    )?;
                    // Added by decision 124 (plan v2 C7.5, F5): the lift records the pair of its
                    // record and the waiter of the settle (CR-20).
                    cx.record_lift((datum.source.hash.clone(), waiter.source.hash.clone()));
                }
                Incoming { datum, lifted }
            }
        };
        let Incoming { datum, lifted } = incoming;
        let spent = !datum.persist && waiter.peeks.is_empty();
        let stays = waiter.persist;
        let introduction = cx.introduction().for_record(&datum.a)?;
        // Added by decision 124 (plan v4 C7.11, ruling RADD 1.4): the fire count before this
        // fire, for the tail read of its head search.
        let fires_before = cx.rounds().fired();
        let step = cx.rounds().after_fire(spent, stays);
        // Changed by decision 124 (plan v4 C7.9 and C7.11, rulings D-1 and RADD 1.4): the fire
        // carries the head search of the discipline of the settle view, which the line after the
        // literal sets. The text before:
        // let fire = OfferFire {
        let mut fire = OfferFire {
            persistent: datum.persist,
            waiter,
            matched,
            // Added by decision 102 (plan v4 C7.22, rule CR-19 case 3): a record that is not
            // lifted is a stored persistent record, which the re-offer leaves in place. Every
            // other record of a fire is in flight.
            offered: match lifted {
                true => Offered::InFlight,
                false => Offered::StoredPersistent,
            },
            search: None,
        };
        fire.search = cx.head_offer_search::<V::Discipline>(
            &channel,
            &fire,
            &datum.source,
            &datum.a,
            fires_before,
        );
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
// Changed by decision 124 (plan v4 C7.11, ruling RADD2 1.6): the lift names the discipline `D` of
// the collection, which its machinery search keeps. The text before:
// fn lift(
fn lift<D: Discipline>(
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
    // Changed by decision 124 (plan v2 C7.5, decision 99): `K_lift` takes the machinery seal.
    // The text before:
    // let cost_authority = origin.authority.cloned();
    let cost_authority = cx.introduction().seal_of(origin.authority)?;
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
    // Changed by decision 124 (plan v4 C7.11, ruling RADD2 1.6): the lift names `D`. The text
    // before:
    // cx.lift(channels, patterns, continuation, candidates)
    cx.lift::<D>(channels, patterns, continuation, candidates)
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
    // Changed by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.3): the rows of the
    // settle before the plan take the class of B.6 row 1, also when a record end stops the
    // settle. The text before:
    // match settle(view, cx, &origin, InFlight::none())? {
    let start = cx.staged_rows();
    let end = settle(view, cx, &origin, InFlight::none());
    cx.settle_rows(start, true);
    match end? {
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
    // Changed by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.3): the rows of the
    // settle after the plan take the class of B.6 row 9, also when a record end stops the
    // settle. The text before:
    // match settle(view, cx, &origin, InFlight::none())? {
    let start = cx.staged_rows();
    let end = settle(view, cx, &origin, InFlight::none());
    cx.settle_rows(start, false);
    match end? {
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
    // Changed by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.3): the rows of the
    // settle job take the class of B.6 row 9, also when a record end stops the settle. The text
    // before:
    // let end = settle(view, cx, &SettleOrigin::of_settle(&op), InFlight::none())?;
    // match end {
    let start = cx.staged_rows();
    let end = settle(view, cx, &SettleOrigin::of_settle(&op), InFlight::none());
    cx.settle_rows(start, false);
    match end? {
        SettleEnd::Quiescent => Ok(None),
        SettleEnd::Reissue(cause) => Ok(Some(cx.reissue(SpaceOp::Settle(op), cause))),
    }
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.4): the tail read of a head
/// row of a discipline that settles: the first step of [`settle`] in the order of `settle_view`,
/// over the store view `view`, from the element in flight `in_flight`, with the fire count
/// `fired` (the table of ADD A2.8). It runs `next_fire` and the form check of the loop, and it
/// stages nothing. The step that follows is `land`, the round cap or a fire. `Ends` means that
/// no step follows, `Reissues` that the cap re-issues with nothing to land, and `Stages` that
/// one more step follows, whether it stages or fails (CR-31). An error of a read fails the read,
/// as it fails the loop.
pub(crate) fn tail_read<S: SettleView, V: StoreView>(
    settle_view: &S,
    view: &mut V,
    in_flight: &InFlight,
    fired: usize,
) -> Result<TailRead, SpaceError> {
    let Some(plan) = settle_view.next_fire(view, in_flight)? else {
        return match in_flight.is_some() {
            false => Ok(TailRead::Ends),
            true => Ok(TailRead::Stages),
        };
    };
    plan.into_channel_fire()?;
    match (in_flight.is_some(), fired >= FIRE_ROUND_CAP) {
        (true, _) => Ok(TailRead::Stages),
        (false, false) => Ok(TailRead::Stages),
        (false, true) => Ok(TailRead::Reissues),
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

/// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD2 1.1 and RADD2v2 6.1): the
/// operation `op` again with no order, as a plan of a collection whose records hold the order 0
/// re-issues an element that did not settle: a produce and a consume carry no order (decision
/// 43), so the record keeps its identity. The plans and the tail read of such a row, and E4.5
/// at the lift of a cascade, build their re-issue with it, so the rule has one text.
pub fn reissued_without_order(op: SpaceOp) -> SpaceOp {
    match op {
        SpaceOp::Produce(op) => SpaceOp::Produce(ProduceOp {
            reissued_ord: None,
            ..op
        }),
        SpaceOp::Consume(op) => SpaceOp::Consume(ConsumeOp {
            reissued_ord: None,
            ..op
        }),
        SpaceOp::Settle(op) => SpaceOp::Settle(op),
        SpaceOp::Join(op) => SpaceOp::Join(op),
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

/// The produce that re-issues a restored record whose payload is `payload`: the collection and
/// the channel of the receive `op`, not persistent, no params, the authority of the record, and
/// no carried order. Its record is the restored record, because the `ord` of a discipline
/// without a counter is 0. Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.3):
/// shared here, so the restore rule of a discipline and the finish of a deviating commit (C2
/// E4.5) re-issue the second send of Rule E in one form.
pub fn restored_send(op: &ConsumeOp, payload: ListParWithRandom) -> Result<ProduceOp, SpaceError> {
    let key = CollectionKey::from_bytes(op.key.as_bytes().to_vec())
        .ok_or(SpaceError::Inconsistent("a collection key is empty"))?;
    let authority = match &payload.cost_authority {
        Some(authority) => authority.clone(),
        None => CostAuthority::default(),
    };
    Ok(ProduceOp {
        space: op.space,
        key,
        channel: op.channel.clone(),
        data: payload,
        persistent: false,
        params: BTreeMap::new(),
        authority,
        reissued_ord: None,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::{CostRegion, EList, Expr};
    use models::rust::utils::{new_freevar_par, new_gstring_par};
    use rspace_plus_plus::rspace::r#match::Match;
    // Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.4): the store view of the
    // test of the tail read names the source of an operation.
    use rspace_plus_plus::rspace::rspace_interface::RSpaceOperationSource;
    use rspace_plus_plus::rspace::trace::event::{Consume, Produce};

    use super::*;
    use crate::rust::interpreter::matcher::r#match::Matcher;
    use crate::rust::interpreter::spaces::agent::QueryParams;
    // Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.4): see the view above.
    use crate::rust::interpreter::spaces::generic::StoredMatch;
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
                // Added by decision 90 (DR-135): an unpriced region, as outside native
                // execution.
                price: 0,
                caller: None,
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

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2v2 6.1): the moved rule of the
    /// plans of a collection whose records hold the order 0. A produce and a consume lose their
    /// order and keep every other field, and a settle job stays as it is.
    #[test]
    fn reissued_without_order_keeps_the_identity_of_the_operation() {
        for persistent in [false, true] {
            let produce = produce_op(persistent, Some(4));
            assert_eq!(
                reissued_without_order(SpaceOp::Produce(produce.clone())),
                SpaceOp::Produce(ProduceOp {
                    reissued_ord: None,
                    ..produce
                })
            );
            let consume = consume_op(persistent, Some(4));
            assert_eq!(
                reissued_without_order(SpaceOp::Consume(consume.clone())),
                SpaceOp::Consume(ConsumeOp {
                    reissued_ord: None,
                    ..consume
                })
            );
        }
        let settle = SpaceOp::Settle(settle_operation(OpView::Produce(&produce_op(false, None))));
        assert_eq!(reissued_without_order(settle.clone()), settle);
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.3): the send that
    /// re-issues a restored record moved here from the scan discipline unchanged. For a consume
    /// that is persistent or not, it is the send of the payload on the collection and the channel
    /// of the consume, not persistent, with no params, the authority of the record and no carried
    /// order. A payload without an authority takes the default authority.
    #[test]
    fn restored_send_moved_unchanged() {
        for persistent in [false, true] {
            for (cost_authority, authority) in [
                (Some(region(8)), region(8)),
                (None, CostAuthority::default()),
            ] {
                let payload = ListParWithRandom {
                    pars: vec![int(5)],
                    random_state: vec![1, 2, 3],
                    cost_authority,
                    cost_stack: None,
                };
                assert_eq!(
                    restored_send(&consume_op(persistent, Some(4)), payload.clone())
                        .expect("the collection key of the consume is not empty"),
                    ProduceOp {
                        space: SPACE,
                        key: key(),
                        channel: channel(),
                        data: payload,
                        persistent: false,
                        params: BTreeMap::new(),
                        authority,
                        reissued_ord: None,
                    }
                );
            }
        }
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

    // ── Decision 124: the machinery shape and the machinery seal of a lift ─────────────────

    // Added by decision 124 (plan v2 C7.5 and C7.10, decision 99): the check of the lift seal
    // runs a stack space, whose settle lifts, and reads its event log. SETTLE has no agent
    // harness, so the helpers of that check follow those of the stack tests.
    use std::sync::Arc;

    use models::rhoapi::cost_signature::Value as CostSignatureValue;
    use models::rhoapi::CostSignature;
    use models::rust::space_factory_urn::{SpaceFactoryUrn, SpaceInner, SpaceOuter};
    use rspace_plus_plus::rspace::rspace::RSpace;
    use rspace_plus_plus::rspace::rspace_interface::ISpace;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    use rspace_plus_plus::rspace::space_store_handle::SpaceStoreHandle;
    use rspace_plus_plus::rspace::trace::event::{Event, IOEvent};

    // Added by decision 124 (plan v2 C7.5 and C7.10, decision 99): the check of the machinery
    // shape builds a `K_retire` waiter, and the check of the lift seal builds the seal.
    use crate::rust::interpreter::accounting::authority::system_residue_authority_with_caller;
    use crate::rust::interpreter::accounting::costs::Cost;
    use crate::rust::interpreter::accounting::RuntimeBudget;
    use crate::rust::interpreter::spaces::agent::{run_op, SpaceAgent, SpaceCreation, SpaceEnv};
    use crate::rust::interpreter::spaces::disciplines::scan::{
        retire_continuation, retire_pattern,
    };
    use crate::rust::interpreter::spaces::disciplines::stack::StackDiscipline;
    use crate::rust::interpreter::spaces::generic::{
        CollectionChannels, Discipline, GenericSpace, OuterStore,
    };
    use crate::rust::interpreter::spaces::outer::hashmap::HashMapOuter;
    use crate::rust::interpreter::spaces::types::config::SpaceConfig;
    use crate::rust::interpreter::spaces::types::qualifier::SpaceQualifier;

    /// Added by decision 124 (plan v2 C7.10, RC2v2 m-9): `machinery_shape` is true for a
    /// `K_retire` waiter and a `K_lift` waiter. It is false for a user waiter whose receive has
    /// only a remainder, with a PQ level record or with a VectorDB policy record, and for a join
    /// waiter.
    #[test]
    fn machinery_shape_needs_a_machinery_body() {
        let waiter = |patterns: Vec<BindPattern>, continuation: TaggedContinuation| ParWaiter {
            source: Consume::create(
                &vec![channel(); patterns.len()],
                &patterns,
                &continuation,
                false,
            ),
            patterns,
            continuation,
            persist: false,
            peeks: Default::default(),
        };
        let random = Blake2b512Random::create_from_bytes(b"machinery shape");
        let member = Produce::create(&channel(), &record_of(1, vec![int(1)]), false);
        let retire = retire_continuation(&member.hash, &random, None);
        assert!(machinery_shape(&waiter(vec![retire_pattern()], retire)));
        let lift = lift_continuation(1, 2, &random, None);
        assert!(machinery_shape(&waiter(vec![lift_pattern()], lift)));
        // A receive with only a remainder has the pattern of a `K_retire` and of a `K_lift`.
        let user = TaggedContinuation {
            tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                body: Some(string("body")),
                random_state: vec![4, 5],
            })),
            guard: None,
            cost_authority: None,
        };
        let leveled = record::wrap_pq_continuation(0, Some(1), &user).expect("a PQ level record");
        assert!(!machinery_shape(&waiter(vec![lift_pattern()], leveled)));
        let query =
            record::WaiterPolicy::Query(Box::new(BTreeMap::from([("k".to_string(), int(2))])));
        let queried =
            record::wrap_policy_continuation(0, &query, &user).expect("a VectorDB policy record");
        assert!(!machinery_shape(&waiter(vec![lift_pattern()], queried)));
        let slots = [
            record::JoinSlot {
                ord: 0,
                bind: 0,
                policy: record::WaiterPolicy::None,
            },
            record::JoinSlot {
                ord: 0,
                bind: 1,
                policy: record::WaiterPolicy::None,
            },
        ];
        let join = record::wrap_join_continuation(&slots, &user).expect("a join record");
        assert!(!machinery_shape(&waiter(vec![lift_pattern(); 2], join)));
    }

    /// Added by decision 124 (plan v2 C7.5, decision 99): the stack space of the check of the
    /// lift seal.
    type Stack = GenericSpace<HashMapOuter, StackDiscipline>;

    /// Added by decision 124 (plan v2 C7.5, decision 99): the in-memory play space of the check
    /// of the lift seal.
    type ParRSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

    /// Added by decision 124 (plan v2 C7.5, decision 99): an unpriced region with a ground
    /// signature, so its system residue builds. The region of `region` has no signature.
    fn signed_region(byte: u8) -> CostAuthority {
        CostAuthority {
            regions: vec![CostRegion {
                instance_id: vec![byte; 32],
                signature: Some(CostSignature {
                    value: Some(CostSignatureValue::Ground(vec![byte])),
                }),
                price: 0,
                caller: None,
            }],
        }
    }

    /// Added by decision 124 (plan v2 C7.5, decision 99): the channels of the collection.
    fn channels() -> CollectionChannels { CollectionChannels::new(&HashMapOuter, SPACE, &key()) }

    /// Added by decision 124 (plan v2 C7.5, decision 99): the body `name`, with a random state of
    /// its own.
    fn named(name: &str) -> ParWithRandom {
        ParWithRandom {
            body: Some(string(name)),
            random_state: Blake2b512Random::create_from_bytes(name.as_bytes()).to_bytes(),
        }
    }

    /// Added by decision 124 (plan v2 C7.5, decision 99): a fresh stack space over an in-memory
    /// play space, with an empty event log.
    async fn stack_fixture() -> (ParRSpace, SpaceEnv) {
        let mut stores = InMemoryStoreManager::new();
        let space = ParRSpace::create(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(Matcher)),
        )
        .expect("an in-memory space");
        let env = SpaceEnv {
            store: SpaceStoreHandle::new(space.clone()),
            budget: RuntimeBudget::new(Cost::create(1_000_000, "settle agent test")),
            volatile: Default::default(),
            origin: Default::default(),
        };
        let config = SpaceConfig::new(
            SPACE.sid,
            HashMapOuter::NAME,
            StackDiscipline::NAME,
            SpaceQualifier::Default,
        )
        .to_par();
        Stack::default()
            .prepare_create(&env, SpaceCreation {
                space: SPACE,
                config,
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
                random: Blake2b512Random::create_from_bytes(b"settle factory call"),
                authority: CostAuthority::default(),
                family: SpaceFactoryUrn {
                    outer: SpaceOuter::HashMap,
                    inner: SpaceInner::Stack,
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

    /// Added by decision 124 (plan v2 C7.5, decision 99): a send of `value` with no cost
    /// authority.
    fn stack_send(value: i64) -> ProduceOp {
        ProduceOp {
            space: SPACE,
            key: key(),
            channel: channel(),
            data: ListParWithRandom {
                pars: vec![int(value)],
                random_state: Blake2b512Random::create_from_bytes(
                    format!("settle send {value}").as_bytes(),
                )
                .to_bytes(),
                cost_authority: None,
                cost_stack: None,
            },
            persistent: false,
            params: BTreeMap::new(),
            authority: CostAuthority::default(),
            reissued_ord: None,
        }
    }

    /// Added by decision 124 (plan v2 C7.5, decision 99): the pattern that binds `value` only.
    fn exactly(value: i64) -> BindPattern {
        BindPattern {
            patterns: vec![int(value)],
            remainder: None,
            free_count: 0,
        }
    }

    /// Added by decision 124 (plan v2 C7.5, decision 99): a receive of `pattern` with the body
    /// `name`.
    fn stack_receive(name: &str, pattern: BindPattern) -> ConsumeOp {
        ConsumeOp {
            space: SPACE,
            key: key(),
            channel: channel(),
            pattern,
            body: named(name),
            persistent: false,
            peek: false,
            guard: None,
            query: None,
            authority: CostAuthority::default(),
            introduction_authority: CostAuthority::default(),
            reissued_ord: None,
        }
    }

    /// Added by decision 124 (plan v2 C7.5, decision 99): runs `op` on the stack space, and the
    /// operation commits.
    async fn stack_run(env: &SpaceEnv, op: SpaceOp) {
        run_op(&Stack::default(), env, op)
            .await
            .expect("the operation commits");
    }

    /// Added by decision 124 (plan v2 C7.5, decision 99): the `K_lift` of a lift carries the
    /// machinery seal of the authority of its operation, an authority with a region. W2(1) and
    /// W(2) park, h(1) is stored under the top waiter W, and the send d(2), whose datum carries
    /// the authority, fires W. The settle after the plan lifts h with the counter value 3 of the
    /// round of d and the step 1.
    #[tokio::test]
    async fn settle_lift_takes_the_seal() {
        let (space, env) = stack_fixture().await;
        stack_run(&env, SpaceOp::Consume(stack_receive("w2", exactly(1)))).await;
        stack_run(&env, SpaceOp::Consume(stack_receive("w", exactly(2)))).await;
        stack_run(&env, SpaceOp::Produce(stack_send(1))).await;
        let authority = signed_region(2);
        let seal = system_residue_authority_with_caller(&authority, &env.budget.deploy_id(), None)
            .expect("the residue of a signed region");
        let d = ProduceOp {
            data: ListParWithRandom {
                cost_authority: Some(authority),
                ..stack_send(2).data
            },
            ..stack_send(2)
        };
        let random =
            RandomPurpose::Lift.derive(&Blake2b512Random::from_bytes(&d.data.random_state));
        space.take_event_log().await;
        stack_run(&env, SpaceOp::Produce(d)).await;
        let k_lift = Consume::create(
            &vec![channels().data().clone()],
            &vec![lift_pattern()],
            &lift_continuation(3, 1, &random, Some(seal)),
            false,
        );
        let log = space.take_event_log().await;
        assert!(
            log.contains(&Event::IoEvent(IOEvent::Consume(k_lift))),
            "the lift stages K_lift with the seal"
        );
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.4): the answer of the next
    /// fire of a scripted settle.
    type SettleScript = fn() -> Result<Option<FirePlan>, SpaceError>;

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.4): a settle view whose
    /// next fire answers from the script. It reads no store.
    struct ScriptedSettle(SettleScript);

    impl SettleView for ScriptedSettle {
        type Discipline = StackDiscipline;

        fn next_fire<V: StoreView>(
            &self,
            _view: &mut V,
            _in_flight: &InFlight,
        ) -> Result<Option<FirePlan>, SpaceError> {
            (self.0)()
        }
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.4): a read of the store
    /// view, which the scripted settle does not make.
    fn unread<T>() -> Result<T, SpaceError> { Err(SpaceError::Inconsistent("an unexpected read")) }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.4): a store view that the
    /// scripted settle does not read.
    struct UnreadView;

    impl StoreView for UnreadView {
        fn data_on(&mut self, _channel: &Par) -> Result<&[ParDatum], SpaceError> { unread() }

        fn waiters_on(&mut self, _channel: &Par) -> Result<&[ParWaiter], SpaceError> { unread() }

        fn joins_on(&mut self, _channel: &Par) -> Result<&[Vec<Par>], SpaceError> { unread() }

        fn group_waiters(&mut self, _group: &[Par]) -> Result<&[ParWaiter], SpaceError> { unread() }

        fn canonical_data_order(&mut self, _channel: &Par) -> Result<Vec<usize>, SpaceError> {
            unread()
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
            _data: &ListParWithRandom,
        ) -> Option<ListParWithRandom> {
            None
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
            unread()
        }

        fn theory_admits(&self, _stored: &[StoredMatch<'_>]) -> Result<bool, SpaceError> {
            unread()
        }

        fn booked(&self, _record: &Produce, _waiter: &Consume) -> bool { false }

        fn data_channel(&self) -> Result<&Par, SpaceError> { unread() }

        fn data(&mut self) -> Result<&[ParDatum], SpaceError> { unread() }

        fn waiters(&mut self) -> Result<&[ParWaiter], SpaceError> { unread() }
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.4): the stored record of
    /// a planned fire, which also serves as the record in flight.
    fn planned_record() -> ParDatum {
        let record = record_of(3, vec![int(1)]);
        let source = Produce::create(&channel(), &record, false);
        ParDatum {
            a: record,
            persist: false,
            source,
        }
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.4): a plan of one fire
    /// that binds `record`.
    fn plan_of(record: BoundRecord) -> FirePlan {
        FirePlan {
            group: vec![channel()],
            waiter: waiter_of(1),
            bound: vec![Bound {
                record,
                matched: ListParWithRandom::default(),
            }],
        }
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.4): the tail read of a
    /// head row answers the first step of the settle loop by the table of ADD A2.8. A read that
    /// fails and a plan that the loop refuses fail it, as they fail the loop.
    #[test]
    fn settle_tail_read_follows_the_a2_8_table() {
        let no_plan: SettleScript = || Ok(None);
        let binds_stored: SettleScript = || {
            Ok(Some(plan_of(BoundRecord::Stored {
                datum: planned_record(),
                view_index: 0,
            })))
        };
        let binds_flying: SettleScript = || Ok(Some(plan_of(BoundRecord::InFlight)));
        for (script, flying, fired, expected) in [
            // No plan and nothing in flight: the loop ends.
            (no_plan, false, 0, TailRead::Ends),
            // No plan and a record in flight: `land` stores it.
            (no_plan, true, 0, TailRead::Stages),
            // A stored record bound with a record in flight: `land`, then the loop plans again.
            (binds_stored, true, 0, TailRead::Stages),
            // The cap with nothing in flight: the loop re-issues and stages nothing.
            (binds_stored, false, FIRE_ROUND_CAP, TailRead::Reissues),
            // The cap with a record in flight: `land` stores it before the re-issue.
            (binds_flying, true, FIRE_ROUND_CAP, TailRead::Stages),
            // A fire below the cap.
            (binds_stored, false, 0, TailRead::Stages),
            (binds_stored, false, FIRE_ROUND_CAP - 1, TailRead::Stages),
            (binds_flying, true, 0, TailRead::Stages),
        ] {
            let element = match flying {
                false => InFlight::none(),
                true => InFlight::lifted(Lifted {
                    channel: channel(),
                    datum: planned_record(),
                }),
            };
            assert_eq!(
                tail_read(&ScriptedSettle(script), &mut UnreadView, &element, fired),
                Ok(expected),
                "a record in flight: {flying}, fires before: {fired}"
            );
        }
        // The form check of the loop fails the read, and so does a read that fails.
        let malformed: SettleScript = || {
            Ok(Some(FirePlan {
                group: vec![channel(); 2],
                waiter: waiter_of(2),
                bound: vec![bound(), bound()],
            }))
        };
        let failing: SettleScript = || Err(SpaceError::Inconsistent("a read of the settle fails"));
        for (script, error) in [
            (malformed, "a settle fire binds one record on one channel"),
            (failing, "a read of the settle fails"),
        ] {
            assert_eq!(
                tail_read(
                    &ScriptedSettle(script),
                    &mut UnreadView,
                    &InFlight::none(),
                    0
                ),
                Err(SpaceError::Inconsistent(error))
            );
        }
    }
}
