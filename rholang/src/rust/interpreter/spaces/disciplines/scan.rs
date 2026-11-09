//! The scan driver of the unordered disciplines (P5 design v3, A.7): the bag, and the set.
//!
//! A scan discipline has no head. Every record and every waiting continuation of a collection
//! can meet an incoming element. Changed by decision 102 (rules CR-1, CR-3 and CR-4): the driver
//! visits the candidates of each search in the pick order of the operation
//! ([`Cx::pick_data_order`] and [`Cx::pick_waiter_order`]). Play shuffles the canonical order of
//! DR-75 once for each search. Replay puts first the candidates that the recorded row of the
//! operation names, and then the other candidates in DR-75 order. The members of a class and
//! every retire list keep the DR-75 order: by the hash of the source produce of each entry, then
//! by the digest of the entry inside a run of equal hashes, then by its index. The text before:
//! "The driver visits the candidates in the canonical order of DR-75 (decision 41): by the hash
//! of the source produce or consume of each entry, then by the digest of the entry inside a run
//! of equal hashes, then by its index. So the choices of the driver depend on the entry set of
//! the collection, not on the list order of the store."
//!
//! The driver has four plans. Each plan stages only through [`Cx`]. So every staged operation
// Changed by decision 90 (decision 69): the legacy chain is removed. The text before:
// //! goes through the legacy chain of decision 45 (Option P) and registers an introduction.
//! registers an introduction.
//!
//! ```text
//! OFFER-SCAN(x)                     ▷ x: the datum of a produce, or a record that a peek removed
//!   loop
//!     W, m ← the first waiter in the pick order of x whose pattern binds x and whose guard
//!            admits m                                                       ▷ decision 102
//!            ▷ a guard refusal moves to the next waiter (decision 37)
//!     if there is no W: store x, stop
//!     stage Fire(x, W)                                             ▷ the fire never stores x
//!     if W does not peek and x has the class κ: RETIRE-CLASS(κ)              ▷ the set only
//!     after_fire(x spent, W stays):
//!       Done: stop
//!       Continue: offer x to the next waiter                                 ▷ decision 30
//!       Reissue(cause): re-issue x                                           ▷ A10, I-21
//!
//! RECEIVE-SCAN(K)                                  ▷ changed by decision 102 (119 G1) and 101
//!   loop
//!     if the recorded row of K is Stored and the commit of K may veto: park K, stop  ▷ E3-a
//!     r ← the first record in the pick order of K whose payload the pattern of K binds
//!     if there is no r: park K, stop
//!     C ← [r] for a peek or for a record without a class, else every stored member of the
//!         class of r in DR-75 order, r included                                      ▷ G1
//!     m ← the values that the pattern of K binds in c₁, the first member of C
//!     if the guard of K refuses m: park K, stop                        ▷ one guard check (37)
//!     after_fire(K spent, C stays)                                  ▷ the fire counts first
//!     stage Fire(K, c₁) over [D]                                     ▷ one bind (decision 101)
//!     if K does not peek and C has a class κ: RETIRE-CLASS(κ)                ▷ the set only
//!     if K peeks and r is not persistent: RESTORE-SCAN(r)              ▷ decision 45, Rule E
//!     Done: stop. Continue: the next record. Reissue(cause): re-issue K.
//!
//! RESTORE-SCAN(r)
//!   OFFER-SCAN of the record bytes of r: not persistent, with the authority of r
//!
//! RETIRE-CLASS(κ)
//!   for each stored member m of κ that is not persistent, in canonical order:
//!     stage the consume of K_retire(m) over [D] with the record m        ▷ never dispatched
//! ```
//!
//! **Fire rounds.** [`FireRounds`] decides the end of each plan (A10, I-21). The datum `x` is
//! spent when it is not persistent and `W` does not peek. `W` stays when it is persistent. The
//! continuation `K` is spent when it is not persistent. The class `C` stays when `K` peeks or a
//! member of `C` is persistent. A restore shares the rounds of its receive, so one operation
//! makes at most `FIRE_ROUND_CAP` fires. A retire is not a fire round.
//!
//! **Identity (decision 43).** A scan discipline has no counter, so every `ord` is 0. A
//! re-issued element therefore has the record and the identity of its first round, and its
//! re-issue carries no order (`reissued_ord: None`). A discipline that uses a counter fails to
//! compile with this driver.
//!
//! **Rule E (decision 45).** A consume-side peek fire removes a non-persistent record, as the
//! host COMM does. RESTORE-SCAN then sends the same record bytes again, as the host reducer's
//! `produce_peeks` does. The second send has the identity of the record and registers the
//! authority of the record. It is offered to the waiters first, so a guarded waiter that admits
//! the record can fire. At the round cap, the restore re-issues the produce of the payload of
//! the record, which has the same identity because its `ord` is 0.
//!
//! **Copies.** A plan decodes the payload of each record that it tests and copies the pattern of
//! each waiter that it tests. It also copies the entries that it selects and the operations that
//! it stages. A fire that ends a plan moves its values.
//!
//! **The pick order.** Changed by decision 102 (decision 119 S4): each search of a plan takes a
//! new pick order and scans it from its head. A fire that continues a plan removes its partner
//! (A10), so a later search cannot meet that partner again. A candidate that failed to match
//! the incoming element fails again, because the element does not change. There is no resume
//! position, because the random order of play is new for each search. The text before:
//! "**The tested prefix.** Within one plan the incoming element does not change, so a candidate
//! that failed to match it fails again. A fire that continues the plan removes its partner, and
//! the canonical order of the other entries does not change. So the next round recomputes the
//! order and continues at the position of the removed partner. This gives the same choices as
//! a scan from the first candidate, with fewer matcher calls."

// Retired by decision 102 (decision 119 G1 and S4, decision 101): the text before of
// RECEIVE-SCAN in the header above.
// ```text
// RECEIVE-SCAN(K)
//   loop
//     r, m ← the first record in canonical order whose payload the pattern of K binds
//     if there is no r, or the guard of K refuses m: park K, stop      ▷ one guard check (37)
//     C ← [r] for a peek or for a record without a class, else r and the other stored
//         members of its class in canonical order                                    ▷ F3
//     after_fire(K spent, C stays)                                  ▷ the fire counts first
//     stage Fire(K, C) over [D; |C|], and dispatch the result of r only
//     if K peeks and r is not persistent: RESTORE-SCAN(r)              ▷ decision 45, Rule E
//     Done: stop. Continue: the next record. Reissue(cause): re-issue K.
// ```

// Changed by decision 124 (plan v4 C7.9 and C7.11, rulings B-1 and RADD2 1.3): the commit finds
// a waiter by its source, and `restored_send`, the one map of the plans, moved to SETTLE. The
// text before:
// use std::collections::{BTreeMap, BTreeSet};
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, LazyLock};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{
    BindPattern, CostAuthority, ListParWithRandom, Par, ParWithRandom, TaggedContinuation,
};
use models::rust::utils::{new_freevar_var, new_gbytearray_par};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::internal::{ConsumeCandidate, Datum};
// Added by decision 124 (plan v4 C7.9, ruling B-1): the row searches match in the commit.
use rspace_plus_plus::rspace::r#match::Match;
// Added by decision 102 (plan v4 C5.1 and C5.6): a pick and the E3-a check read the recorded
// row of their operation.
use rspace_plus_plus::rspace::rspace_interface::RSpaceOperationSource;
// Added by decision 124 (plan v4 C7.9 and C7.11, rule CR-31, rulings B-1, RADD2 1.1,
// RADD2v2 7.1 and 7.2): the row searches of a scan offer and a scan receive read the store of
// the commit, and they hand out the end of Tail(actual) of a fire that the commit binds in place
// of c0.
use rspace_plus_plus::rspace::space_store_handle::{
    canonical_continuation_order, canonical_data_order, incoming_candidate, ActualFire,
    ConsumeOutcome, RowItemRule, RowReissue, RowRun, RowSearch, SpaceConsume, SpaceTxnError,
};
// Changed by decision 102 (plan v4 C5.1): a receive names its consume.
// use rspace_plus_plus::rspace::trace::event::Produce;
use rspace_plus_plus::rspace::trace::event::{Consume, Produce};

// Changed by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD2 1.1, RADD2v2 7.1 and 7.2):
// the searches of a scan offer and a scan receive rebuild the step of a fire that the commit
// binds in place of c0. The text before:
// use crate::rust::interpreter::reified::costs::{FireStep, ReissueCause, FIRE_ROUND_CAP};
use crate::rust::interpreter::reified::costs::{
    FireRounds, FireStep, ReissueCause, FIRE_ROUND_CAP,
};
use crate::rust::interpreter::spaces::agent::{ConsumeOp, ProduceOp, SpaceOp, SpaceReissue};
use crate::rust::interpreter::spaces::errors::SpaceError;
// Changed by decision 102 (plan v4 C7.22): OFFER-SCAN names the datum that a fire offers.
// use crate::rust::interpreter::spaces::generic::{
//     open_plan, peek_set, Cx, Discipline, OfferFire, OpView, ParCandidate, ParDatum, ParWaiter,
//     Side, StoredMatch,
// };
use crate::rust::interpreter::spaces::generic::{
    open_plan, peek_set, Cx, Discipline, OfferFire, Offered, OpView, ParCandidate, ParDatum,
    ParWaiter, Side, StoredMatch,
};
// Added by decision 124 (plan v4 C7.9 and C7.11, rule CR-31, rulings B-1, RADD2 1.1,
// RADD2v2 7.1 and 7.2): the carriers of the row searches, and the end of Tail(actual).
use crate::rust::interpreter::spaces::generic::{
    ParRowLists, ParRowRequest, ParRowSearch, ReceiveFire, StagedRowSearch, TailEnd,
};
use crate::rust::interpreter::spaces::matching::may_veto;
// Changed by decision 124 (plan v4 C7.11, ruling RADD2 1.3): `restored_send` moved to SETTLE,
// so the plans name no collection key. The text before:
// use crate::rust::interpreter::spaces::namespace::{CollectionKey, RandomPurpose};
use crate::rust::interpreter::spaces::namespace::RandomPurpose;
use crate::rust::interpreter::spaces::record::{self, bincode_digest, Introduction};
// Added by decision 124 (plan v2 C7.10, plan v4 C7.11, rule CR-31, rulings RADD2 1.1 and 1.3,
// RADD2v2 7.1 to 7.3): the reader skips inert machinery waiters, RESTORE-SCAN re-issues its
// record with the send of SETTLE, the receive search re-issues the operation as B.6 row 5 does,
// and the plans and the searches of a scan re-issue an operation with no order by one text.
use crate::rust::interpreter::spaces::settle::{
    machinery_shape, reissued_operation, reissued_without_order, restored_send,
};
// Added by decision 124 (plan v4 C7.9, ruling B-1): the theory of the commit checks a tuple.
use crate::rust::interpreter::spaces::theory::SpaceTheory;

/// A discipline whose plans are the scan driver of this module: the bag and the set (P5a).
///
/// The interface is frozen for the set (P5 design v3, A.3.3). A scan discipline keeps
/// `USES_COUNTER = false`. Its `offer`, `receive` and `restore` call [`offer`], [`receive`] and
/// [`restore`].
pub trait ScanDiscipline: Discipline {
    /// The class key of the payload `payload` of a record or of a datum, or `None` when every
    /// record is a class of its own (the bag).
    ///
    /// Two records with equal keys are members of one class. A non-peek receive binds every
    /// member of the class of the record that it takes, and a non-peek produce-side fire
    /// retires the stored members of the class of its datum (F3, decision 37). The driver
    /// compares two keys by their hashes, and it compares the full keys only when the hashes
    /// are equal (DR-90).
    ///
    /// The key depends on the payload only, never on the random state, the cost authority or
    /// the cost stack of the record (F5). A payload without a key fails with a typed error
    /// before any staging, never with empty bytes. The default gives `None`.
    fn class_key(&self, _payload: &[Par]) -> Result<Option<Vec<u8>>, SpaceError> { Ok(None) }
}

/// The pattern of a retire consume: one remainder that binds the whole payload, as the handle
/// pattern does (`reified::factory::handle_pattern`). It binds every payload without a cost
/// stack.
pub fn retire_pattern() -> BindPattern {
    BindPattern {
        patterns: Vec::new(),
        remainder: Some(new_freevar_var(0)),
        free_count: 1,
    }
}

/// The continuation `K_retire(m)` that retires the stored record `m` of a set class (P5a, F3,
/// decision 37). Its body is the source hash `source` of `m`, so its identity differs for each
/// member, also when two retires share the random state `random`. It has no guard, and it
/// carries the cost authority `cost_authority` of the element whose fire retires the class.
pub fn retire_continuation(
    source: &Blake2b256Hash,
    random: &Blake2b512Random,
    cost_authority: Option<CostAuthority>,
) -> TaggedContinuation {
    continuation_with(source.bytes(), random.to_bytes(), cost_authority)
}

/// The retire continuation with the body bytes `body`, the random state bytes `random_state`
/// and the cost authority `cost_authority`.
fn continuation_with(
    body: Vec<u8>,
    random_state: Vec<u8>,
    cost_authority: Option<CostAuthority>,
) -> TaggedContinuation {
    TaggedContinuation {
        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(new_gbytearray_par(body, Vec::new(), false)),
            random_state,
        })),
        guard: None,
        cost_authority,
    }
}

/// The template of the retire pattern.
pub(crate) static RETIRE_PATTERN: LazyLock<BindPattern> = LazyLock::new(retire_pattern);

/// The produce plan of a scan discipline: OFFER-SCAN of the datum of `op` with the order `ord`,
/// which is 0. A re-issue moves `op` and carries no order (decision 43). The settle hooks of `D`
/// run before and after the plan, as in the head driver.
pub fn offer<D: ScanDiscipline>(
    discipline: &D,
    cx: &mut Cx<'_, '_>,
    op: ProduceOp,
    ord: i64,
) -> Result<Option<SpaceReissue>, SpaceError> {
    // The driver gives every element the `ord` 0 and re-issues it with no carried order, which
    // keeps the identity only without a counter (decision 43).
    // A scan discipline has no counter. The check fails closed, because the P5 completeness
    // rule allows no assertion in code that is not a test.
    if D::USES_COUNTER {
        return Err(SpaceError::Inconsistent(
            "a scan discipline uses no counter",
        ));
    }
    if let Some(reissue) = open_plan(discipline, cx, OpView::Produce(&op))? {
        return Ok(Some(reissue));
    }
    let record = record::wrap_datum(ord, &op.data);
    let source = Produce::create(cx.data_channel(), &record, op.persistent);
    let element = InFlight {
        record,
        source,
        persist: op.persistent,
        introduction: cx.introduction(),
    };
    // Disabled by I-21 (one fire count for each transaction, `Cx::rounds`):
    // let mut rounds = FireRounds::new();
    match offer_scan(discipline, cx, element, &op.data)? {
        OfferEnd::Settled => discipline.settle_after(cx, OpView::Produce(&op)),
        // Changed by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD2 1.1,
        // RADD2v2 7.2 and 7.3): the search of a fire that the commit binds in place of c0 builds
        // the same re-issue, so both call one fn (decision 123). The text before:
        // ```
        // OfferEnd::Reissue(cause) => Ok(Some(cx.reissue(
        //     SpaceOp::Produce(ProduceOp {
        //         reissued_ord: None,
        //         ..op
        //     }),
        //     cause,
        // ))),
        // ```
        OfferEnd::Reissue(cause) => Ok(Some(offer_reissue(op, cause))),
    }
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD2 1.1, RADD2v2 7.2 and 7.3):
/// the re-issue of the send `op` for `cause`, at the round cap or at a divergent pair. The send
/// runs again as the program issued it. A scan discipline has no counter, so the re-issue
/// carries no order (decision 43, `reissued_without_order`). The fire loop of `offer` and the
/// search of a fire that the commit binds in place of c0 (`ScanOfferSearch::tail_reissue`)
/// build it here.
fn offer_reissue(op: ProduceOp, cause: ReissueCause) -> SpaceReissue {
    SpaceReissue {
        op: reissued_without_order(SpaceOp::Produce(op)),
        cause,
    }
}

/// The consume plan of a scan discipline: RECEIVE-SCAN of the continuation of `op` with the
/// order `ord`, which is 0. A re-issue moves `op` and carries no order (decision 43). After a
/// peek fire of a non-persistent record, RESTORE-SCAN sends the record again with the rounds of
/// the receive (decision 45, Rule E). The settle hooks of `D` run before and after the plan.
///
/// The restore of a receive that is persistent and peeks can re-issue while the receive itself
/// re-issues. The plan then returns the re-issue of the restore, as the head driver does
/// (`generic::receive_head`). No Rholang receive is persistent and peeks: the normalizer gives
/// each receive one arrow.
pub fn receive<D: ScanDiscipline>(
    discipline: &D,
    cx: &mut Cx<'_, '_>,
    op: ConsumeOp,
    ord: i64,
) -> Result<Option<SpaceReissue>, SpaceError> {
    // The driver gives every element the `ord` 0 and re-issues it with no carried order, which
    // keeps the identity only without a counter (decision 43).
    // A scan discipline has no counter. The check fails closed, because the P5 completeness
    // rule allows no assertion in code that is not a test.
    if D::USES_COUNTER {
        return Err(SpaceError::Inconsistent(
            "a scan discipline uses no counter",
        ));
    }
    if let Some(reissue) = open_plan(discipline, cx, OpView::Consume(&op))? {
        return Ok(Some(reissue));
    }
    let continuation = record::wrap_continuation_from_parts(
        ord,
        &op.body,
        &op.guard,
        record::stamped(&op.authority),
    )?;
    let peeks = peek_set(op.peek);
    // Added by decision 102 (plan v4 C5.1 and C5.6): the consume that the receive stages, its
    // park and its one-bind fire alike. Its recorded row orders the picks of replay (CR-4) and
    // decides E3-a (CR-7).
    let source = cx.receive_source(&op, &continuation);
    // Disabled by I-21 (one fire count for each transaction, `Cx::rounds`):
    // let mut rounds = FireRounds::new();
    // Retired by decision 102 (decision 119 S4): each search scans its pick order from the head.
    // let mut tested = 0;
    loop {
        // Added by decision 102 (plan v4 C5.6, rule CR-7, base E3-a): replay parks the receive
        // without a search when its recorded row is Stored and its commit may veto. Play reads
        // no row, so it always searches.
        if cx.recorded_stored(RSpaceOperationSource::Consume(&source)) && cx.may_veto(&continuation)
        {
            cx.check_capacity::<D>(Side::Waiters)?;
            // Changed by decision 124 (plan v4 C7.9, errata V4-17, ruling B-1): the E3-a park
            // carries the receive search for `verify_row_refusals`, CanStop and the enumeration.
            // The text before:
            // cx.park(&op, continuation, peeks)?;
            let data = cx.data_channel();
            let receive = (&op, &continuation, &source);
            let search = receive_search(discipline, cx, receive, None, None)?;
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
            return discipline.settle_after(cx, OpView::Consume(&op));
        }
        // Changed by decision 102 (plan v4 C5.1): the search takes the pick order of the receive.
        // let Some(found) = first_bound_record(cx, &op.pattern, tested)? else {
        let Some((found, order)) = first_bound_record(cx, &op.pattern, &source)? else {
            cx.check_capacity::<D>(Side::Waiters)?;
            cx.park(&op, continuation, peeks)?;
            return discipline.settle_after(cx, OpView::Consume(&op));
        };
        // Changed by decision 102 (decision 119 G1): the fire binds the first member of the
        // class in DR-75 order, so the guard and the theory check that member.
        // // The theory of the space checks the stored record after the guard (P7, TH-6). A set
        // // class is one value (F5), so the check of its first member decides the class.
        // let stored = StoredMatch {
        //     payload: &found.payload,
        //     captures: &found.matched,
        // };
        // if !cx.admits(&continuation, &found.matched, &[stored])? {
        //     cx.check_capacity::<D>(Side::Waiters)?;
        //     cx.park(&op, continuation, peeks)?;
        //     return discipline.settle_after(cx, OpView::Consume(&op));
        // }
        // let position = found.position;
        // let class = class_of(discipline, cx, &op, found)?;
        let class = class_of(discipline, cx, &op, found)?;
        // Added by decision 124 (plan v4 C7.8, rule CR-20, ruling OP-F13): the fire binds the
        // first member of the class, so a first member whose pair with the consume the book
        // holds parks the continuation, as a guard refusal does.
        match cx.booked(&class.first.record.source, &source) {
            false => {}
            true => {
                cx.check_capacity::<D>(Side::Waiters)?;
                cx.park(&op, continuation, peeks)?;
                return discipline.settle_after(cx, OpView::Consume(&op));
            }
        }
        // The theory of the space checks the stored record after the guard (P7, TH-6). A set
        // class is one value (F5), so the check of its first member decides the class.
        let stored = StoredMatch {
            payload: &class.payload,
            captures: &class.first.matched,
        };
        if !cx.admits(&continuation, &class.first.matched, &[stored])? {
            cx.check_capacity::<D>(Side::Waiters)?;
            cx.park(&op, continuation, peeks)?;
            return discipline.settle_after(cx, OpView::Consume(&op));
        }
        // Added by decision 124 (L6, CR30 D-b): the fire takes the lease of the stored datum
        // that the check proved.
        cx.lease_stored(1);
        // Added by decision 124 (plan v4 C7.9, ruling B-1): the row search of the fire, over the
        // pick order of the receive. Its staged candidate is the member that the fire binds.
        let staged = Some(&class.first.record.source);
        let receive = (&op, &continuation, &source);
        let search = receive_search(discipline, cx, receive, Some(order.as_slice()), staged)?;
        let spent = !op.persistent;
        // Changed by decision 102 (decision 119 G1): the class stays when a member of it is
        // persistent (CR-9).
        // let stays = op.peek || class.iter().any(|member| member.record.persist);
        let stays = op.peek || class.persistent;
        // Rule E (decision 45): the peek fire removes a non-persistent record, as the host COMM
        // does, and RESTORE-SCAN sends the record again right after the fire.
        // Changed by decision 102 (decision 119 G1): a peek binds the found record alone, so it
        // is the first member.
        // let restored = match class.as_slice() {
        //     [representative] if op.peek && !representative.record.persist => {
        //         Some(representative.record.a.clone())
        //     }
        //     _ => None,
        // };
        let restored = match (op.peek, class.first.record.persist) {
            (true, false) => Some(class.first.record.a.clone()),
            (true, true) | (false, _) => None,
        };
        let step = cx.rounds().after_fire(spent, stays);
        match step {
            FireStep::Continue => {
                let fired = continuation.clone();
                // Changed by decision 102 (decision 119 G1, decision 101): one bind, and then
                // the retire of the class.
                // fire_class(cx, &op, fired, peeks.clone(), class)?;
                // Changed by decision 124 (plan v4 C7.9, ruling B-1): the fire takes its search.
                fire_class(discipline, cx, &op, fired, peeks.clone(), class, search)?;
                if let Some(reissue) = restore_in(discipline, cx, restored, &op)? {
                    return Ok(Some(reissue));
                }
                // Retired by decision 102 (decision 119 S4): the next search scans from the head.
                // tested = position;
            }
            FireStep::Done => {
                // Changed by decision 102 (decision 119 G1, decision 101).
                // fire_class(cx, &op, continuation, peeks, class)?;
                // Changed by decision 124 (plan v4 C7.9, ruling B-1): the fire takes its search.
                fire_class(discipline, cx, &op, continuation, peeks, class, search)?;
                if let Some(reissue) = restore_in(discipline, cx, restored, &op)? {
                    return Ok(Some(reissue));
                }
                return discipline.settle_after(cx, OpView::Consume(&op));
            }
            FireStep::Reissue(cause) => {
                // Changed by decision 102 (decision 119 G1, decision 101).
                // fire_class(cx, &op, continuation, peeks, class)?;
                // Changed by decision 124 (plan v4 C7.9, ruling B-1): the fire takes its search.
                fire_class(discipline, cx, &op, continuation, peeks, class, search)?;
                if let Some(reissue) = restore_in(discipline, cx, restored, &op)? {
                    return Ok(Some(reissue));
                }
                // Changed by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD2 1.1,
                // RADD2v2 7.1 and 7.3): the search of a fire that the commit binds in place of
                // c0 builds the same re-issue, so both call one fn (decision 123). The text
                // before:
                // ```
                // return Ok(Some(cx.reissue(
                //     SpaceOp::Consume(ConsumeOp {
                //         reissued_ord: None,
                //         ..op
                //     }),
                //     cause,
                // )));
                // ```
                return Ok(Some(receive_reissue(op, cause)));
            }
        }
    }
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD2 1.1, RADD2v2 7.1 and 7.3):
/// the re-issue of the receive `op` for `cause`, at the round cap or at a divergent pair. The
/// receive runs again as the program issued it. A scan discipline has no counter, so the
/// re-issue carries no order (decision 43, `reissued_without_order`). The fire loop of
/// `receive` and the search of a fire that the commit binds in place of c0
/// (`ScanReceiveSearch::tail_reissue`) build it here.
fn receive_reissue(op: ConsumeOp, cause: ReissueCause) -> SpaceReissue {
    SpaceReissue {
        op: reissued_without_order(SpaceOp::Consume(op)),
        cause,
    }
}

/// The restore plan of a scan discipline outside a receive of this driver: RESTORE-SCAN of
/// `record` with the fire count of its transaction (decision 45, Rule E, I-21).
///
/// The engine calls `Discipline::restore` only from the head driver, which a scan discipline
/// does not use. The plan has no operation, so it cannot build the re-issue of the record. It
/// fails with `Inconsistent` at the round cap or at a divergent pair, before the commit, so the
/// dropped transaction applies nothing.
pub fn restore<D: ScanDiscipline>(
    discipline: &D,
    cx: &mut Cx<'_, '_>,
    record: ListParWithRandom,
) -> Result<Option<SpaceReissue>, SpaceError> {
    // The driver gives every element the `ord` 0 and re-issues it with no carried order, which
    // keeps the identity only without a counter (decision 43).
    // A scan discipline has no counter. The check fails closed, because the P5 completeness
    // rule allows no assertion in code that is not a test.
    if D::USES_COUNTER {
        return Err(SpaceError::Inconsistent(
            "a scan discipline uses no counter",
        ));
    }
    // Disabled by I-21 (one fire count for each transaction, `Cx::rounds`):
    // let mut rounds = FireRounds::new();
    restore_scan(discipline, cx, record, None)
}

/// The element in flight of OFFER-SCAN: the datum of a produce, or a record that a consume-side
/// peek removed. Every fire of the offer stages its record and its identity and registers its
/// introduction.
struct InFlight<'i> {
    /// The record: the envelope with the order, then the payload.
    record: ListParWithRandom,
    /// The identity of the produce of the record.
    source: Produce,
    /// True for a persistent datum.
    persist: bool,
    /// The introduction that the staged operations of the element register.
    introduction: &'i Arc<Introduction>,
}

/// The end of OFFER-SCAN.
enum OfferEnd {
    /// The element is stored, or a fire spent it.
    Settled,
    /// The element is still in flight, and its operation re-issues with this cause.
    Reissue(ReissueCause),
}

/// A waiter that an offer fires: its copy, and the values that it binds. Changed by decision 102
/// (decision 119 S4): the text before: "its position in the canonical order, its copy, and the
/// values that it binds."
struct FoundWaiter {
    // Retired by decision 102 (decision 119 S4): no search resumes at a position.
    // position: usize,
    waiter: ParWaiter,
    matched: ListParWithRandom,
}

/// A record that the pattern of a receive binds: its index in the staged view, its payload, and
/// the values that it binds. Changed by decision 102 (decision 119 S4): the text before: "its
/// position in the canonical order, its index in the staged view, its payload, and the values
/// that it binds."
struct FoundRecord {
    // Retired by decision 102 (decision 119 S4): no search resumes at a position.
    // position: usize,
    index: usize,
    payload: ListParWithRandom,
    matched: ListParWithRandom,
}

/// Added by decision 102 (decision 119 G1, decision 101): the class of a receive fire.
struct ReceiveClass {
    /// The member that the fire binds: the first member of the class in DR-75 order, or the
    /// found record for a peek or for a record without a class.
    first: Member,
    /// The payload of `first`, which the theory of the space checks (P7, TH-6).
    payload: ListParWithRandom,
    /// True when a member of the class is persistent, so the class stays (CR-9).
    persistent: bool,
    /// The retire of the other members of the class: `Some` for a receive that does not peek a
    /// record with a class (RETIRE-CLASS), and `None` otherwise.
    retiring: Option<Retiring>,
}

/// A member of the class that a receive fire binds: the copy of the record, the values that it
/// binds, and its index in the staged view. The VectorDB discipline fires the members of a list
/// with it (P6).
pub(crate) struct Member {
    pub(crate) record: ParDatum,
    pub(crate) matched: ListParWithRandom,
    pub(crate) view_index: i32,
}

/// The class that an offer or a receive retires, and what its retire continuations need: the
/// key, the hash of the key, the random state of the retires, and the cost authority of the
/// element or of the receive. Changed by decision 102 (decision 119 G1, decision 101): a receive
/// that does not peek retires the other members of its class too. The text before: "The class
/// of the datum of an offer, and what its retire continuations need: the key, the hash of the
/// key, the random state of the retires and the cost authority of the element."
struct Retiring {
    key: Vec<u8>,
    key_hash: Blake2b256Hash,
    random: Blake2b512Random,
    authority: Option<CostAuthority>,
}

/// OFFER-SCAN of `element`, whose payload is `payload`, on the fire count of its transaction
/// (`Cx::rounds`, I-21). It returns whether the element settled or must re-issue.
fn offer_scan<D: ScanDiscipline>(
    discipline: &D,
    cx: &mut Cx<'_, '_>,
    element: InFlight<'_>,
    payload: &ListParWithRandom,
) -> Result<OfferEnd, SpaceError> {
    let data = cx.data_channel();
    let mut class: Option<Option<Retiring>> = None;
    // Retired by decision 102 (decision 119 S4): each search scans its pick order from the head.
    // let mut tested = 0;
    loop {
        // Changed by decision 102 (plan v4 C5.1): the search takes the pick order of the
        // element, whose produce is `element.source`. A Rule E restore sends its record through
        // here too (G5).
        // let Some(found) = first_admitting_waiter(cx, payload, tested)? else {
        let Some((found, order)) = first_admitting_waiter(cx, payload, &element.source)? else {
            cx.check_capacity::<D>(Side::Data)?;
            cx.store_on(data, element.record, element.persist, element.introduction)?;
            return Ok(OfferEnd::Settled);
        };
        // Changed by decision 102 (decision 119 S4): a found waiter has no resume position.
        // let FoundWaiter {
        //     position,
        //     waiter,
        //     matched,
        // } = found;
        let FoundWaiter { waiter, matched } = found;
        let peeks = !waiter.peeks.is_empty();
        let spent = !element.persist && !peeks;
        let stays = waiter.persist;
        // A peek waiter takes no datum, so its fire retires nothing (decision 37).
        let retiring = match peeks {
            true => None,
            false => {
                let cached = match class.take() {
                    Some(cached) => cached,
                    None => retiring_of(discipline, payload, &element.record)?,
                };
                class.insert(cached).as_ref()
            }
        };
        // Changed by decision 124 (plan v4 C7.9, ruling B-1): the fire carries the row search of
        // the scan offer, which the line after the literal sets. The text before:
        // let fire = OfferFire {
        let mut fire = OfferFire {
            persistent: element.persist,
            waiter,
            matched,
            // Added by decision 102 (plan v4 C7.22): OFFER-SCAN offers an element in flight: the
            // element of a send or a restored record.
            offered: Offered::InFlight,
            search: None,
        };
        fire.search = offer_search(discipline, cx, &element, payload, &fire.waiter, &order)?;
        let step = cx.rounds().after_fire(spent, stays);
        match step {
            FireStep::Continue => {
                let staged = element.record.clone();
                let removed = element.record.clone();
                let source = element.source.clone();
                cx.fire_offer_on(data, fire, staged, removed, source, element.introduction)?;
                if let Some(retiring) = retiring {
                    retire_class(discipline, cx, retiring)?;
                }
                // Retired by decision 102 (decision 119 S4): the next search scans from the head.
                // tested = position;
            }
            FireStep::Done => {
                let removed = element.record.clone();
                cx.fire_offer_on(
                    data,
                    fire,
                    element.record,
                    removed,
                    element.source,
                    element.introduction,
                )?;
                if let Some(retiring) = retiring {
                    retire_class(discipline, cx, retiring)?;
                }
                return Ok(OfferEnd::Settled);
            }
            FireStep::Reissue(cause) => {
                let removed = element.record.clone();
                cx.fire_offer_on(
                    data,
                    fire,
                    element.record,
                    removed,
                    element.source,
                    element.introduction,
                )?;
                if let Some(retiring) = retiring {
                    retire_class(discipline, cx, retiring)?;
                }
                return Ok(OfferEnd::Reissue(cause));
            }
        }
    }
}

/// The class of the payload `payload` of the element `record` of an offer, with the random state
/// and the cost authority of its retires, or `None` without a class. The retires of an offer
/// derive their random state from the element (`RandomPurpose::Retire`) and carry its cost
/// authority: for a produce these are the random state and the cost authority of the datum.
fn retiring_of<D: ScanDiscipline>(
    discipline: &D,
    payload: &ListParWithRandom,
    record: &ListParWithRandom,
) -> Result<Option<Retiring>, SpaceError> {
    let Some(key) = discipline.class_key(&payload.pars)? else {
        return Ok(None);
    };
    let key_hash = bincode_digest(&key)?;
    let random = RandomPurpose::Retire.derive(&Blake2b512Random::from_bytes(&record.random_state));
    let authority = record.cost_authority.as_ref().cloned();
    Ok(Some(Retiring {
        key,
        key_hash,
        random,
        authority,
    }))
}

/// Added by decision 124 (plan v4 C7.9, ruling B-1): the row search of the offer of `element`
/// with the payload `payload` to `waiter`, over the pick order `order` of the staged view. None
/// without owner funding (ruling D-10).
fn offer_search<D: ScanDiscipline>(
    discipline: &D,
    cx: &mut Cx<'_, '_>,
    element: &InFlight<'_>,
    payload: &ListParWithRandom,
    waiter: &ParWaiter,
    order: &[usize],
) -> Result<Option<ParRowSearch>, SpaceError> {
    match cx.introduction().owner_funded() {
        true => {}
        false => return Ok(None),
    }
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
    let class = match retiring_of(discipline, payload, &element.record)? {
        None => None,
        Some(retiring) => {
            let rows = RetireRows::of(cx, data, &retiring.random, retiring.authority.as_ref())?;
            Some(OfferClass {
                key: retiring.key,
                key_hash: retiring.key_hash,
                class_key: |payload: &[Par]| D::default().class_key(payload),
                rows,
                members: None,
            })
        }
    };
    let staged_retires = match (&class, waiter.peeks.is_empty()) {
        (Some(class), true) => {
            let records = cx.data()?;
            let members = class_members(records, &class.key, &class.key_hash, class.class_key)?;
            let mut count = 0;
            for (index, _) in &members {
                match records.get(*index) {
                    Some(stored) if !stored.persist => count += 1,
                    Some(_) | None => {}
                }
            }
            count
        }
        (Some(_), false) | (None, _) => 0,
    };
    // Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD2 1.1 and RADD2v2 7.2): the
    // fire keeps the fire count before it and the copy of the operation, for the end of a Return
    // of a fire that the commit binds in place of c0. Without the copy the search keeps no tail,
    // and the finish builds the end from the receipt.
    let fired = cx.rounds().fired();
    let tail = cx
        .operation()
        .map(|operation| ScanTail { fired, operation });
    let search = ScanOfferSearch::<D> {
        channel: data.clone(),
        waiters,
        staged: waiter.source.clone(),
        element: Datum {
            a: element.record.clone(),
            persist: element.persist,
            source: element.source.clone(),
        },
        payload: payload.clone(),
        theory: cx.has_theory(),
        alternatives: !cx.leases(),
        class,
        staged_retires,
        tail,
        fired_waiter: None,
        discipline: std::marker::PhantomData,
    };
    Ok(Some(Box::new(search)))
}

/// The first waiter of the collection, in the pick order of the element whose produce is
/// `source`, whose pattern binds `payload` and whose guard admits the bound values. A waiter
/// whose guard refuses is skipped (decision 37), so replay, which tries the recorded waiter
/// first, never fires a waiter whose guard refused in play (CR-14). The pattern of each tested
/// waiter is copied and matched once, and only a waiter that binds is copied whole. Changed by
/// decision 102 (plan v4 C5.1, rules CR-1 and CR-4): the text before: "The first waiter of the
/// collection, in canonical order from the position `tested`, whose pattern binds `payload`
/// and whose guard admits the bound values."
fn first_admitting_waiter(
    cx: &mut Cx<'_, '_>,
    payload: &ListParWithRandom,
    // Changed by decision 102 (decision 119 S4): the search takes the source of its pick.
    // tested: usize,
    source: &Produce,
    // Changed by decision 124 (plan v4 C7.9, ruling B-1): the pick hands out its pick order,
    // which the row search of the fire lists. The text before:
    // ) -> Result<Option<FoundWaiter>, SpaceError> {
) -> Result<Option<(FoundWaiter, Vec<usize>)>, SpaceError> {
    let data = cx.data_channel();
    // Changed by decision 102 (plan v4 C5.1): the pick order, scanned from its head.
    // let order = cx.canonical_waiter_order(data)?;
    // for (position, &index) in order.iter().enumerate().skip(tested) {
    let order = cx.pick_waiter_order(data, RSpaceOperationSource::Produce(source))?;
    for &index in &order {
        let pattern = {
            let waiter = waiter_at(cx, index)?;
            // Added by decision 124 (plan v2 C7.10, decision 100): an inert machinery waiter is
            // no candidate.
            match machinery_shape(waiter) {
                false => {}
                true => continue,
            }
            let [pattern] = waiter.patterns.as_slice() else {
                return Err(SpaceError::Inconsistent(
                    "a waiting continuation of a collection has several binds",
                ));
            };
            pattern.clone()
        };
        let Some(matched) = cx.bind(&pattern, payload) else {
            continue;
        };
        let waiter = waiter_at(cx, index)?.clone();
        // Added by decision 124 (plan v4 C7.8, rule CR-20, ruling OP-F13): a waiter whose pair
        // with the element the book holds is skipped, as a waiter whose guard refuses
        // (decision 37).
        match cx.booked(source, &waiter.source) {
            false => {}
            true => continue,
        }
        // The element is in flight, so the theory checks no stored data here (P7, TH-6).
        if cx.admits(&waiter.continuation, &matched, &[])? {
            // Changed by decision 102 (decision 119 S4): no resume position.
            // return Ok(Some(FoundWaiter {
            //     position,
            //     waiter,
            //     matched,
            // }));
            return Ok(Some((FoundWaiter { waiter, matched }, order)));
        }
    }
    Ok(None)
}

/// The first record of the collection, in the pick order of the receive whose consume is
/// `source`, whose payload `pattern` binds. The payload of each tested record is decoded and
/// matched once. Nothing is copied whole. Changed by decision 102 (plan v4 C5.1, rules CR-1 and
/// CR-4): the text before: "The first record of the collection, in canonical order from the
/// position `tested`, whose payload `pattern` binds."
fn first_bound_record(
    cx: &mut Cx<'_, '_>,
    pattern: &BindPattern,
    // Changed by decision 102 (decision 119 S4): the search takes the source of its pick.
    // tested: usize,
    source: &Consume,
    // Changed by decision 124 (plan v4 C7.9, ruling B-1): the pick hands out its pick order,
    // which the row search of the fire lists. The text before:
    // ) -> Result<Option<FoundRecord>, SpaceError> {
) -> Result<Option<(FoundRecord, Vec<usize>)>, SpaceError> {
    let data = cx.data_channel();
    // Changed by decision 102 (plan v4 C5.1): the pick order, scanned from its head.
    // let order = cx.canonical_data_order(data)?;
    // for (position, &index) in order.iter().enumerate().skip(tested) {
    let order = cx.pick_data_order(data, RSpaceOperationSource::Consume(source))?;
    for &index in &order {
        let payload = record::unwrap_datum(&record_at(cx, index)?.a)?.datum;
        if let Some(matched) = cx.bind(pattern, &payload) {
            // Changed by decision 102 (decision 119 S4): no resume position.
            // return Ok(Some(FoundRecord {
            //     position,
            //     index,
            //     payload,
            //     matched,
            // }));
            return Ok(Some((
                FoundRecord {
                    index,
                    payload,
                    matched,
                },
                order,
            )));
        }
    }
    Ok(None)
}

/// The class that the receive `op` binds with the record `found`. For a peek or for a record
/// without a class it is `found` alone. Otherwise it is every stored member of the class of
/// `found`, `found` included, in DR-75 order. The fire binds the first member with its own
/// values, and RETIRE-CLASS retires the other members that are not persistent (CR-13). Only
/// the first member is bound and copied. The retires derive their random state from the random
/// state of the receive (`RandomPurpose::Retire`) and carry its stamped authority, as the join
/// engine does for a consume (`join::Engine::new`).
///
/// Changed by decision 102 (decision 119 G1, rule CR-12, decision 101). The text before: "The
/// class that the receive `op` binds with the record `found`: `[found]` for a peek or for a
/// record without a class, and otherwise `found` and the other stored members of its class, in
/// canonical order (F3, decision 41). The record of `found` is the first member in canonical
/// order, because every member binds the pattern of `op` as `found` does. So the other members
/// follow it. Each member binds the pattern of `op` with its own values."
fn class_of<D: ScanDiscipline>(
    discipline: &D,
    cx: &mut Cx<'_, '_>,
    op: &ConsumeOp,
    found: FoundRecord,
) -> Result<ReceiveClass, SpaceError> {
    let FoundRecord {
        index,
        payload,
        matched,
    } = found;
    let key = match op.peek {
        true => None,
        false => discipline.class_key(&payload.pars)?,
    };
    let Some(key) = key else {
        let first = member_at(cx, index, matched)?;
        let persistent = first.record.persist;
        return Ok(ReceiveClass {
            first,
            payload,
            persistent,
            retiring: None,
        });
    };
    let key_hash = bincode_digest(&key)?;
    let data = cx.data_channel();
    // Added by decision 102 (plan v4 C5.7): kept DR-75, because G1 fires the DR-75-first member.
    let order = cx.canonical_data_order(data)?;
    let mut first_index: Option<usize> = None;
    let mut persistent = false;
    for &other in &order {
        let (member_persist, member_key) = {
            let stored = record_at(cx, other)?;
            let member_payload = record::unwrap_datum(&stored.a)?.datum;
            (stored.persist, discipline.class_key(&member_payload.pars)?)
        };
        let Some(member_key) = member_key else {
            continue;
        };
        if !same_class(&key, &key_hash, &member_key)? {
            continue;
        }
        persistent = persistent || member_persist;
        first_index = first_index.or(Some(other));
    }
    let (first, payload) = match first_index {
        Some(first) if first == index => (member_at(cx, index, matched)?, payload),
        Some(first) => {
            let member_payload = record::unwrap_datum(&record_at(cx, first)?.a)?.datum;
            let member_matched =
                cx.bind(&op.pattern, &member_payload)
                    .ok_or(SpaceError::Inconsistent(
                        "a member of a class does not bind the pattern of its representative",
                    ))?;
            (member_at(cx, first, member_matched)?, member_payload)
        }
        None => {
            return Err(SpaceError::Inconsistent(
                "a found record is not a member of its own class",
            ));
        }
    };
    let random = RandomPurpose::Retire.derive(&Blake2b512Random::from_bytes(&op.body.random_state));
    let retiring = Retiring {
        key,
        key_hash,
        random,
        authority: record::stamped(&op.authority).cloned(),
    };
    Ok(ReceiveClass {
        first,
        payload,
        persistent,
        retiring: Some(retiring),
    })
}

/// Added by decision 124 (plan v4 C7.9, errata V4-17, ruling B-1, RIBA 3.2): the row search of
/// a receive `op` with `continuation` and the consume `source`, over the pick order `order` of
/// the staged view. A fire passes its pick order and `staged`, the source of the member that it
/// binds. An E3-a park passes None for both, so the search takes the pick order of the receive.
/// None without owner funding (ruling D-10): before that check the search copies nothing and
/// computes no pick order.
fn receive_search<D: ScanDiscipline>(
    discipline: &D,
    cx: &mut Cx<'_, '_>,
    receive: (&ConsumeOp, &TaggedContinuation, &Consume),
    order: Option<&[usize]>,
    staged: Option<&Produce>,
) -> Result<Option<ParRowSearch>, SpaceError> {
    match cx.introduction().owner_funded() {
        true => {}
        false => return Ok(None),
    }
    let (op, continuation, source) = receive;
    let data = cx.data_channel();
    let parked: Vec<usize>;
    let order = match order {
        Some(order) => order,
        None => {
            parked = cx.pick_data_order(data, RSpaceOperationSource::Consume(source))?;
            parked.as_slice()
        }
    };
    let mut records = Vec::<Produce>::with_capacity(order.len());
    for &index in order {
        records.push(record_at(cx, index)?.source.clone());
    }
    let classes = match (op.peek, staged) {
        (false, Some(first)) => {
            let index = located(cx, data, &first.hash)?;
            let payload = record::unwrap_datum(&record_at(cx, index)?.a)?.datum;
            match discipline.class_key(&payload.pars)? {
                Some(_) => {
                    let random = RandomPurpose::Retire
                        .derive(&Blake2b512Random::from_bytes(&op.body.random_state));
                    let authority = record::stamped(&op.authority);
                    let rows = RetireRows::of(cx, data, &random, authority)?;
                    let class_key: ClassKey = |payload: &[Par]| D::default().class_key(payload);
                    Some((class_key, rows))
                }
                None => None,
            }
        }
        (true, _) | (false, None) => None,
    };
    let staged_retires = staged_retires_of(cx, staged, classes.as_ref())?;
    // Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD2 1.1 and RADD2v2 7.1): the
    // fire of a persistent receive that does not peek keeps the fire count before it and the copy
    // of the operation, for the end of Tail(actual) of a fire that the commit binds in place of
    // c0. Without the copy the search keeps no tail, and the finish builds the end from the
    // receipt.
    let tail = match (op.peek, op.persistent, staged) {
        (true, _, _) | (false, false, _) | (false, true, None) => None,
        (false, true, Some(_)) => {
            let fired = cx.rounds().fired();
            cx.operation()
                .map(|operation| ScanTail { fired, operation })
        }
    };
    let search = ScanReceiveSearch::<D> {
        channel: data.clone(),
        records,
        staged: staged.cloned(),
        source: source.clone(),
        continuation: continuation.clone(),
        persistent: op.persistent,
        pattern: op.pattern.clone(),
        theory: cx.theory(),
        alternatives: !cx.leases(),
        classes,
        staged_retires,
        found: None,
        tail,
        fired_class: None,
        discipline: std::marker::PhantomData,
    };
    Ok(Some(Box::new(search)))
}

/// Added by decision 124 (plan v4 C7.9, ruling B-1): the count of the retire rows that
/// RETIRE-CLASS stages after the fire of the member `staged` (CR-33): the members of its class
/// that are not persistent, without the bound member, which the fire removes.
fn staged_retires_of(
    cx: &mut Cx<'_, '_>,
    staged: Option<&Produce>,
    classes: Option<&(ClassKey, RetireRows)>,
) -> Result<usize, SpaceError> {
    let (Some(first), Some((class_key, _))) = (staged, classes) else {
        return Ok(0);
    };
    let data = cx.data_channel();
    let index = located(cx, data, &first.hash)?;
    let records = cx.data()?;
    let stored = records.get(index).ok_or(SpaceError::Inconsistent(
        "a staged record is outside the view",
    ))?;
    let payload = record::unwrap_datum(&stored.a)?.datum;
    let Some(key) = class_key(&payload.pars)? else {
        return Ok(0);
    };
    let key_hash = bincode_digest(&key)?;
    let members = class_members(records, &key, &key_hash, *class_key)?;
    let mut count: usize = 0;
    for (position, (member, _)) in members.iter().enumerate() {
        match (position, records.get(*member)) {
            (0, _) => {}
            (_, Some(stored)) if !stored.persist => count += 1,
            (_, Some(_) | None) => {}
        }
    }
    Ok(count)
}

// Retired by decision 102 (decision 119 G1, rule CR-12, decision 101): the class of a receive
// with every member bound for the fire over `[D; |class|]`.
// fn class_of<D: ScanDiscipline>(
//     discipline: &D,
//     cx: &mut Cx<'_, '_>,
//     op: &ConsumeOp,
//     found: FoundRecord,
// ) -> Result<Vec<Member>, SpaceError> {
//     let FoundRecord {
//         position,
//         index,
//         payload,
//         matched,
//     } = found;
//     let representative = Member {
//         record: record_at(cx, index)?.clone(),
//         matched,
//         view_index: view_index(index)?,
//     };
//     let key = match op.peek {
//         true => None,
//         false => discipline.class_key(&payload.pars)?,
//     };
//     let Some(key) = key else {
//         let mut class = Vec::<Member>::with_capacity(1);
//         class.push(representative);
//         return Ok(class);
//     };
//     let key_hash = bincode_digest(&key)?;
//     let data = cx.data_channel();
//     let order = cx.canonical_data_order(data)?;
//     let mut class = Vec::<Member>::with_capacity(1);
//     class.push(representative);
//     let next = position.checked_add(1).ok_or(SpaceError::Inconsistent(
//         "a candidate position overflows usize",
//     ))?;
//     for &other in order.iter().skip(next) {
//         let payload = record::unwrap_datum(&record_at(cx, other)?.a)?.datum;
//         let Some(other_key) = discipline.class_key(&payload.pars)? else {
//             continue;
//         };
//         if !same_class(&key, &key_hash, &other_key)? {
//             continue;
//         }
//         let matched = cx
//             .bind(&op.pattern, &payload)
//             .ok_or(SpaceError::Inconsistent(
//                 "a member of a class does not bind the pattern of its representative",
//             ))?;
//         let member = Member {
//             record: record_at(cx, other)?.clone(),
//             matched,
//             view_index: view_index(other)?,
//         };
//         class.push(member);
//     }
//     Ok(class)
// }

/// Added by decision 102 (decision 119 G1): the member at `index` of the staged view, which
/// binds the values `matched`.
fn member_at(
    cx: &mut Cx<'_, '_>,
    index: usize,
    matched: ListParWithRandom,
) -> Result<Member, SpaceError> {
    Ok(Member {
        record: record_at(cx, index)?.clone(),
        matched,
        view_index: view_index(index)?,
    })
}

/// Stages the consume fire of the receive `op` with the continuation `continuation` and the
/// peek set `peeks` over `[D]`, with the first member of `class` as its one bind. Then a
/// receive that does not peek a record with a class retires the other members of the class
/// that are not persistent, in DR-75 order (RETIRE-CLASS), as the offer side does. A retire is
/// not a fire round.
///
/// Changed by decision 102 (decision 119 G1, decision 101, rule CR-13). The text before:
/// "Stages the consume fire of the receive `op` over the class `class` with the continuation
/// `continuation` and the peek set `peeks`: the group `[D; |class|]`, whose outcome dispatches
/// the result of the first member only (F3, F20). For one member it is the fire of one record."
fn fire_class<D: ScanDiscipline>(
    discipline: &D,
    cx: &mut Cx<'_, '_>,
    op: &ConsumeOp,
    continuation: TaggedContinuation,
    peeks: BTreeSet<i32>,
    class: ReceiveClass,
    // Changed by decision 124 (plan v4 C7.9, ruling B-1): the fire takes its search.
    search: Option<ParRowSearch>,
) -> Result<(), SpaceError> {
    let ReceiveClass {
        first,
        persistent,
        retiring,
        ..
    } = class;
    // Changed by decision 124 (plan v4 C7.9, rulings B-1 and OP-R): the one-bind fire takes the
    // receive search as the sixth input of `fire_receive`, which stages the same group.
    let Member {
        record,
        matched,
        view_index,
    } = first;
    // Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.3): the class rule of the
    // fire (CR-9). The receipt lists the bound member alone, so its row carries whether another
    // member of its class is persistent.
    let others_persist = persistent && !record.persist;
    let fire = ReceiveFire {
        head: record,
        matched,
        view_index,
    };
    cx.fire_receive(op, fire, continuation, peeks, search)?;
    // Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.3): the fire row takes
    // the class of a class fire.
    cx.class_fire(others_persist)?;
    match retiring {
        Some(retiring) => retire_class(discipline, cx, &retiring),
        None => Ok(()),
    }
}

// Retired by decision 102 (decision 119 G1, decision 101): the fire of a whole class over
// `[D; |class|]`.
// fn fire_class(
//     cx: &mut Cx<'_, '_>,
//     op: &ConsumeOp,
//     continuation: TaggedContinuation,
//     peeks: BTreeSet<i32>,
//     class: Vec<Member>,
// ) -> Result<(), SpaceError> {
//     let data = cx.data_channel();
//     let width = class.len();
//     let mut channels = Vec::<Par>::with_capacity(width);
//     let mut patterns = Vec::<BindPattern>::with_capacity(width);
//     let mut candidates = Vec::<ParCandidate>::with_capacity(width);
//     for member in class {
//         channels.push(data.clone());
//         patterns.push(op.pattern.clone());
//         candidates.push(stored_candidate(data.clone(), member));
//     }
//     cx.fire_receive_group(
//         channels,
//         patterns,
//         continuation,
//         op.persistent,
//         peeks,
//         candidates,
//         1,
//     )
// }

/// The candidate of a stored member on `channel`: the record moves into the candidate, with the
/// values that it binds and its index in the staged view.
pub(crate) fn stored_candidate(channel: Par, member: Member) -> ParCandidate {
    let Member {
        record: Datum { a, persist, source },
        matched,
        view_index,
    } = member;
    ConsumeCandidate {
        channel,
        datum: Datum {
            a: matched,
            persist,
            source,
        },
        removed_datum: a,
        datum_index: view_index,
    }
}

/// RESTORE-SCAN of `restored`, the record that a peek fire of the receive `op` removed, on the
/// fire count of the transaction of the receive (I-21). No restore is a no-op.
fn restore_in<D: ScanDiscipline>(
    discipline: &D,
    cx: &mut Cx<'_, '_>,
    restored: Option<ListParWithRandom>,
    op: &ConsumeOp,
) -> Result<Option<SpaceReissue>, SpaceError> {
    match restored {
        Some(record) => restore_scan(discipline, cx, record, Some(op)),
        None => Ok(None),
    }
}

/// RESTORE-SCAN: OFFER-SCAN of `record`, which a consume-side peek removed, as the host sends a
/// peeked datum again (decision 45, Rule E). The second send is not persistent. Its identity
/// is `Produce::create(D, record, false)`, which equals the identity of the stored record, and
/// it registers the authority of the record (`Introduction::for_record`). At the round cap or a
/// divergent pair it re-issues the produce of the payload of the record, with the channel of
/// the receive `origin`. Without a receive it fails with `Inconsistent`. A plan opens below the
/// round cap and continues only below it, so the second send is the only element that can meet
/// a full transaction: the fire of the receive can be the last one (I-21). Then the second send
/// re-issues before it fires.
fn restore_scan<D: ScanDiscipline>(
    discipline: &D,
    cx: &mut Cx<'_, '_>,
    record: ListParWithRandom,
    origin: Option<&ConsumeOp>,
) -> Result<Option<SpaceReissue>, SpaceError> {
    let payload = record::unwrap_datum(&record)?.datum;
    let source = Produce::create(cx.data_channel(), &record, false);
    let introduction = cx.introduction().for_record(&record)?;
    let element = InFlight {
        record,
        source,
        persist: false,
        introduction: &introduction,
    };
    // Changed by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.3): the rows of the
    // second send take a class of their own, also when a record end stops the send. The text
    // before:
    // let end = match cx.rounds().fired() < FIRE_ROUND_CAP {
    //     true => offer_scan(discipline, cx, element, &payload)?,
    //     false => OfferEnd::Reissue(ReissueCause::RoundCap),
    // };
    let start = cx.staged_rows();
    let offered = match cx.rounds().fired() < FIRE_ROUND_CAP {
        true => offer_scan(discipline, cx, element, &payload),
        false => Ok(OfferEnd::Reissue(ReissueCause::RoundCap)),
    };
    cx.restore_rows(start);
    let end = offered?;
    match end {
        OfferEnd::Settled => Ok(None),
        OfferEnd::Reissue(cause) => match origin {
            Some(op) => {
                let send = restored_send(op, payload)?;
                Ok(Some(cx.reissue(SpaceOp::Produce(send), cause)))
            }
            None => Err(SpaceError::Inconsistent(
                "a restore outside a receive cannot re-issue its record",
            )),
        },
    }
}

// Moved by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.3) to SETTLE, beside
// `settle_operation`, so the finish of a deviating commit re-issues the second send of Rule E in
// the form of RESTORE-SCAN (`settle::restored_send`). The text before:
// /// The produce that re-issues a restored record whose payload is `payload`: the collection and
// /// the channel of the receive `op`, not persistent, no params, the authority of the record, and
// /// no carried order. Its record is the restored record, because the `ord` is 0.
// fn restored_send(op: &ConsumeOp, payload: ListParWithRandom) -> Result<ProduceOp, SpaceError> {
//     let key = CollectionKey::from_bytes(op.key.as_bytes().to_vec())
//         .ok_or(SpaceError::Inconsistent("a collection key is empty"))?;
//     let authority = match &payload.cost_authority {
//         Some(authority) => authority.clone(),
//         None => CostAuthority::default(),
//     };
//     Ok(ProduceOp {
//         space: op.space,
//         key,
//         channel: op.channel.clone(),
//         data: payload,
//         persistent: false,
//         params: BTreeMap::new(),
//         authority,
//         reissued_ord: None,
//     })
// }

/// RETIRE-CLASS: stages the retire of each stored member of the class of `retiring` that is not
/// persistent, in canonical order (decision 37, decision 41), on the collection of the operation
/// ([`retire_class_on`]).
fn retire_class<D: ScanDiscipline>(
    discipline: &D,
    cx: &mut Cx<'_, '_>,
    retiring: &Retiring,
) -> Result<(), SpaceError> {
    let data = cx.data_channel();
    // Changed by decision 124 (plan v4 C7.11, ruling RADD2 1.6): the retire names the discipline
    // `D` of the collection, which its machinery search keeps. The text before:
    // retire_class_on(
    retire_class_on::<D>(
        cx,
        data,
        &retiring.key,
        &retiring.key_hash,
        &retiring.random,
        retiring.authority.as_ref(),
        |payload| discipline.class_key(payload),
    )
}

/// RETIRE-CLASS on the collection whose data channel is `channel`: stages the retire of each
/// stored member of the class `key`, whose hash is `key_hash`, that is not persistent,
/// in canonical order. `class_key` gives the class of a stored payload. Each retire is the
/// consume of `K_retire(m)` over `[channel]` with the record `m`, with the random state `random`
/// and the cost authority `authority`. It is never dispatched, and it is not a fire round. A
/// persistent member stays, because a COMM never removes a persistent record. A retire locates
/// its member again before it stages, because each retire removes a record. The join engine
/// retires the classes of the binds of a join fire with it (P5c).
// Changed by decision 124 (plan v4 C7.11, ruling RADD2 1.6): the retire names the discipline
// `D` of the collection, which its machinery search keeps. The text before:
// pub(crate) fn retire_class_on(
pub(crate) fn retire_class_on<D: Discipline>(
    cx: &mut Cx<'_, '_>,
    channel: &Par,
    key: &[u8],
    key_hash: &Blake2b256Hash,
    random: &Blake2b512Random,
    authority: Option<&CostAuthority>,
    class_key: impl Fn(&[Par]) -> Result<Option<Vec<u8>>, SpaceError>,
) -> Result<(), SpaceError> {
    // Added by decision 102 (plan v4 C5.7): kept DR-75, because RETIRE-CLASS reads the store
    // after the fire.
    let order = cx.canonical_data_order(channel)?;
    let mut members: Vec<(ParDatum, ListParWithRandom)> = Vec::new();
    for &index in &order {
        // A COMM never removes a persistent record, so a persistent member is not decoded.
        let payload = {
            let stored = record_on(cx, channel, index)?;
            match stored.persist {
                true => None,
                false => Some(record::unwrap_datum(&stored.a)?.datum),
            }
        };
        let Some(payload) = payload else {
            continue;
        };
        let Some(member_key) = class_key(&payload.pars)? else {
            continue;
        };
        if !same_class(key, key_hash, &member_key)? {
            continue;
        }
        let member = record_on(cx, channel, index)?.clone();
        members.push((member, payload));
    }
    // Added by decision 124 (plan v2 C7.6, decision 99): `K_retire` takes the machinery seal.
    // The seal is the same for each member, so it is built once.
    let seal = cx.introduction().seal_of(authority)?;
    for (member, payload) in members {
        let index = located(cx, channel, &member.source.hash)?;
        let matched = cx
            .bind(&RETIRE_PATTERN, &payload)
            .ok_or(SpaceError::Inconsistent(
                "a retired member does not bind the retire pattern",
            ))?;
        // Changed by decision 124 (plan v2 C7.6, decision 99): the machinery seal. The join
        // engine retires through here too. The text before:
        // let authority = authority.cloned();
        let authority = seal.clone();
        let continuation = retire_continuation(&member.source.hash, random, authority);
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
        // Changed by decision 124 (plan v4 C7.11, ruling RADD2 1.6): the retire names `D`. The
        // text before:
        // cx.retire(channels, patterns, continuation, candidates)?;
        cx.retire::<D>(channels, patterns, continuation, candidates)?;
    }
    Ok(())
}

/// True when the key `other` equals `key`, whose hash is `key_hash` (DR-90): the hashes are
/// compared first, and the full keys only when the hashes are equal.
pub(crate) fn same_class(
    key: &[u8],
    key_hash: &Blake2b256Hash,
    other: &[u8],
) -> Result<bool, SpaceError> {
    let other_hash = bincode_digest(other)?;
    match *key_hash == other_hash {
        false => Ok(false),
        true => Ok(key == other),
    }
}

/// The index in the staged view of `channel` of the first record whose source hash is `hash`.
/// Two records with one source hash are byte-identical, so either one is the member.
pub(crate) fn located(
    cx: &mut Cx<'_, '_>,
    channel: &Par,
    hash: &Blake2b256Hash,
) -> Result<usize, SpaceError> {
    let records = cx.data_on(channel)?;
    for (index, stored) in records.iter().enumerate() {
        if stored.source.hash == *hash {
            return Ok(index);
        }
    }
    Err(SpaceError::Inconsistent(
        "a member of a class is absent from the staged view",
    ))
}

/// The record at `index` of the staged view of the collection.
fn record_at<'v>(cx: &'v mut Cx<'_, '_>, index: usize) -> Result<&'v ParDatum, SpaceError> {
    cx.data()?.get(index).ok_or(SpaceError::Inconsistent(
        "a canonical index is outside the staged view",
    ))
}

/// The record at `index` of the staged view of `channel`.
fn record_on<'v>(
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

/// The waiting continuation at `index` of the staged view of the collection.
fn waiter_at<'v>(cx: &'v mut Cx<'_, '_>, index: usize) -> Result<&'v ParWaiter, SpaceError> {
    cx.waiters()?.get(index).ok_or(SpaceError::Inconsistent(
        "a canonical index is outside the staged view",
    ))
}

/// The index `index` of the staged view as the index of a candidate.
pub(crate) fn view_index(index: usize) -> Result<i32, SpaceError> {
    i32::try_from(index).map_err(|_| SpaceError::Inconsistent("a record index exceeds i32"))
}

/// Added by decision 124 (plan v4 C7.9, ruling B-1): the class key of a payload by the
/// discipline of a scan row (`ScanDiscipline::class_key`).
pub(crate) type ClassKey = fn(&[Par]) -> Result<Option<Vec<u8>>, SpaceError>;

/// Added by decision 124 (plan v4 C7.9 and C7.21, ruling B-1): a `K_retire` request of the
/// commit.
pub(crate) type ParConsume = SpaceConsume<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

/// Added by decision 124 (plan v4 C7.9, ruling B-1): a read error of a row search at the commit.
/// The search fails closed (CR-36).
pub(crate) fn commit_error(_: SpaceError) -> SpaceTxnError {
    SpaceTxnError::Inconsistent("a row search cannot read a record at the commit")
}

/// Added by decision 124 (plan v4 C7.9, ruling B-1): the index of each record of `records` by
/// the hash of its source. Two records with one source hash are byte-identical, so the first
/// one stands for both.
pub(crate) fn source_positions(records: &[ParDatum]) -> HashMap<&Blake2b256Hash, usize> {
    let mut positions = HashMap::<&Blake2b256Hash, usize>::with_capacity(records.len());
    for (index, stored) in records.iter().enumerate() {
        positions.entry(&stored.source.hash).or_insert(index);
    }
    positions
}

/// Added by decision 124 (plan v4 C7.9, ruling B-1): the members of the class `key`, whose hash
/// is `key_hash`, among `records`, in DR-75 order, with the payload of each. The commit reads a
/// class through it, as `class_of` and [`retire_class_on`] read the staged view.
pub(crate) fn class_members(
    records: &[ParDatum],
    key: &[u8],
    key_hash: &Blake2b256Hash,
    class_key: ClassKey,
) -> Result<Vec<(usize, ListParWithRandom)>, SpaceError> {
    let order = canonical_data_order(records);
    let mut members = Vec::<(usize, ListParWithRandom)>::with_capacity(order.len());
    for index in order {
        let Some(stored) = records.get(index) else {
            return Err(SpaceError::Inconsistent(
                "a canonical index is outside the records",
            ));
        };
        let payload = record::unwrap_datum(&stored.a)?.datum;
        let Some(member_key) = class_key(&payload.pars)? else {
            continue;
        };
        match same_class(key, key_hash, &member_key)? {
            false => {}
            true => members.push((index, payload)),
        }
    }
    Ok(members)
}

/// Added by decision 124 (plan v4 C7.9 and C7.21, section 2.4, errata V4-5, ruling B-1): the
/// `K_retire` rows that the commit readies for the actual fire of a row (CR-13, CR-33). They
/// take what [`retire_class_on`] takes from the plan, so they equal the rows that the staging of
/// that fire stages. Each row carries the machinery search (V4-5).
pub(crate) struct RetireRows {
    channel: Par,
    random: Blake2b512Random,
    authority: Option<CostAuthority>,
    introduction: Arc<Introduction>,
    own: bool,
}

impl RetireRows {
    /// The rows on `channel` with the random state `random` and the machinery seal of the cost
    /// authority `authority` of the fire (C7.6).
    pub(crate) fn of(
        cx: &Cx<'_, '_>,
        channel: &Par,
        random: &Blake2b512Random,
        authority: Option<&CostAuthority>,
    ) -> Result<Self, SpaceError> {
        let authority = cx.introduction().seal_of(authority)?;
        Ok(Self {
            channel: channel.clone(),
            random: random.clone(),
            authority,
            introduction: Arc::clone(cx.introduction()),
            own: cx.owns_random_state(&random.to_bytes()),
        })
    }

    /// The member `stored` at the index `index` of the store of the commit, with the value
    /// that the retire pattern binds in `payload`.
    pub(crate) fn member(
        matcher: &dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
        stored: &ParDatum,
        payload: &ListParWithRandom,
        index: usize,
    ) -> Result<Member, SpaceTxnError> {
        let matched = matcher
            .get(&RETIRE_PATTERN, payload)
            .ok_or(SpaceTxnError::Inconsistent(
                "a retired member does not bind the retire pattern",
            ))?;
        let view_index = i32::try_from(index)
            .map_err(|_| SpaceTxnError::Inconsistent("a record index overflows i32"))?;
        Ok(Member {
            record: stored.clone(),
            matched,
            view_index,
        })
    }

    /// The retire request of each member of `members`, in their order. The commit locates
    /// each member again before it stages the row (C2 E2.8). The machinery search of each
    /// request names the discipline `D` of the collection (ruling RADD2 1.6).
    pub(crate) fn requests<D: Discipline>(&self, members: Vec<Member>) -> Vec<ParConsume> {
        let mut requests = Vec::<ParConsume>::with_capacity(members.len());
        for member in members {
            requests.push(self.request_on::<D>(&self.channel, member));
        }
        requests
    }

    /// Added by decision 124 (plan v4 C7.9 and C7.11, rulings RADD 1.5 and RADD2 1.6, CR-33): the
    /// retire request of the member `member` on `channel`, with the random state, the authority
    /// and the introduction of these rows, and the machinery search of the discipline `D`.
    pub(crate) fn request_on<D: Discipline>(&self, channel: &Par, member: Member) -> ParConsume {
        let hash = member.record.source.hash.clone();
        let continuation = retire_continuation(&hash, &self.random, self.authority.clone());
        let mut channels = Vec::<Par>::with_capacity(1);
        channels.push(channel.clone());
        let mut patterns = Vec::<BindPattern>::with_capacity(1);
        patterns.push((*RETIRE_PATTERN).clone());
        let mut candidates = Vec::<ParCandidate>::with_capacity(1);
        candidates.push(stored_candidate(channel.clone(), member));
        let search = StagedRowSearch::<D>::of_machinery(&channels, &candidates);
        SpaceConsume {
            channels,
            patterns,
            continuation,
            persist: false,
            peeks: BTreeSet::new(),
            outcome: ConsumeOutcome::Fire { data: candidates },
            before_observe: Some(record::register_introduction(&self.introduction)),
            own: self.own,
            search: Some(Box::new(search)),
        }
    }
}

/// Added by decision 124 (plan v4 C7.9, section 2.4 rows "Bag" and "Set", ruling B-1): the row
/// search of a scan offer. The row order is the waiter order of the pick (CR-1). Each waiter
/// binds the element in flight with one bind. A fire that takes a set element retires the
/// stored members of its class, unless its waiter peeks (decision 37). A leasing theory gives
/// no alternative (C4v2 G8).
pub(crate) struct ScanOfferSearch<D> {
    channel: Par,
    waiters: Vec<(Consume, bool)>,
    staged: Consume,
    element: ParDatum,
    payload: ListParWithRandom,
    theory: bool,
    alternatives: bool,
    class: Option<OfferClass>,
    staged_retires: usize,
    /// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD2 1.1 and RADD2v2 7.2): what
    /// the fire keeps for the end of a Return of Tail(actual), or None.
    tail: Option<ScanTail>,
    /// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD2 1.1 and RADD2v2 7.2):
    /// whether the waiter that the commit bound in place of c0 peeks, and whether it is
    /// persistent (`actual_fire`), until `tail_reissue`.
    fired_waiter: Option<(bool, bool)>,
    /// Added by decision 124 (plan v4 C7.11, ruling RADD2 1.6): the discipline of the collection,
    /// which the machinery search of each retire row names. The search holds no value of it.
    discipline: std::marker::PhantomData<fn() -> D>,
}

/// Added by decision 124 (plan v4 C7.9, ruling B-1): the class of the element of a scan offer,
/// its retire rows, and its members that are not persistent, once the commit read them.
struct OfferClass {
    key: Vec<u8>,
    key_hash: Blake2b256Hash,
    class_key: ClassKey,
    rows: RetireRows,
    members: Option<Vec<Member>>,
}

impl OfferClass {
    /// Reads the members of the class that are not persistent from the store of the commit, once
    /// (CR-13).
    fn read(
        &mut self,
        lists: &ParRowLists<'_>,
        matcher: &dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
        channel: &Par,
    ) -> Result<(), SpaceTxnError> {
        match self.members {
            None => {}
            Some(_) => return Ok(()),
        }
        let records = lists.data(channel)?;
        let found = class_members(&records, &self.key, &self.key_hash, self.class_key)
            .map_err(commit_error)?;
        let mut members = Vec::<Member>::with_capacity(found.len());
        for (index, payload) in found {
            let stored = records.get(index).ok_or(SpaceTxnError::Inconsistent(
                "a member is outside the records",
            ))?;
            match stored.persist {
                true => {}
                false => members.push(RetireRows::member(matcher, stored, &payload, index)?),
            }
        }
        self.members = Some(members);
        Ok(())
    }
}

impl<D: ScanDiscipline> RowSearch<Par, BindPattern, ListParWithRandom, TaggedContinuation>
    for ScanOfferSearch<D>
{
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
        let [pattern] = request.patterns else {
            return Err(SpaceTxnError::Inconsistent(
                "a waiting continuation of a collection has several binds",
            ));
        };
        let Some(matched) = request.matcher.get(pattern, &self.payload) else {
            return Ok(None);
        };
        let candidate = incoming_candidate(
            self.channel.clone(),
            self.element.source.clone(),
            self.element.a.clone(),
            self.element.persist,
            matched,
        );
        if let Some(item) = &request.item {
            match item.admits(&candidate.datum.a, candidate.datum.persist)? {
                true => {}
                false => return Ok(None),
            }
        }
        if let Some(class) = &mut self.class {
            class.read(lists, request.matcher, &self.channel)?;
        }
        let mut tuple = Vec::<ParCandidate>::with_capacity(1);
        tuple.push(candidate);
        Ok(Some(tuple))
    }

    fn restore(&mut self) {}

    fn commit_check(
        &mut self,
        request: &ParRowRequest<'_>,
        tuple: &[ParCandidate],
    ) -> Result<bool, SpaceTxnError> {
        match *request.source == self.staged {
            true => Ok(true),
            false => {
                let [candidate] = tuple else {
                    return Err(SpaceTxnError::Inconsistent("a scan offer binds one datum"));
                };
                // The element is in flight, so the theory checks no stored data (P7, TH-6).
                Ok(request
                    .matcher
                    .check_commit_by_reference(request.continuation, &[&candidate.datum.a]))
            }
        }
    }

    fn may_veto(
        &self,
        matcher: &dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
        continuation: &TaggedContinuation,
    ) -> bool {
        may_veto(matcher, continuation, self.theory)
    }

    fn staged_retires(&self) -> usize { self.staged_retires }

    fn actual_fire(
        &mut self,
        request: &ParRowRequest<'_>,
        _tuple: &[ParCandidate],
    ) -> ActualFire<Par, BindPattern, ListParWithRandom, TaggedContinuation> {
        let peeks = match self
            .waiters
            .iter()
            .find(|(source, _)| source == request.source)
        {
            Some((_, peeks)) => *peeks,
            None => false,
        };
        // Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD2 1.1 and RADD2v2 7.2):
        // the search keeps whether the waiter that the commit binds in place of c0 peeks, and
        // whether it is persistent, for its end of Tail(actual) (`tail_reissue`).
        self.fired_waiter = Some((peeks, request.persistent));
        let retires = match (peeks, &mut self.class) {
            (false, Some(class)) => match class.members.take() {
                Some(members) => class.rows.requests::<D>(members),
                None => Vec::new(),
            },
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

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD2 1.1 and RADD2v2 7.2): the
    /// end of Tail(actual) of a fire that the commit bound in place of c0, when the fire re-issues
    /// at the fire count before it: the Return of the re-issue of the offer loop
    /// (`offer_reissue`). The receipt lists the waiter that the commit bound, so at every other
    /// step the finish builds the exact end from the receipt. The copy of a receive, whose plan
    /// offers the second send of RESTORE-SCAN, gives no end either: the finish re-issues that
    /// send as `restore_scan` does (`restore_reissue`).
    fn tail_reissue(&mut self) -> Option<RowReissue> {
        let Some((peeks, persistent)) = self.fired_waiter.take() else {
            return None;
        };
        let Some(tail) = &self.tail else {
            return None;
        };
        let SpaceOp::Produce(op) = &*tail.operation else {
            return None;
        };
        let spent = !self.element.persist && !peeks;
        match FireRounds::at(tail.fired).after_fire(spent, persistent) {
            FireStep::Done | FireStep::Continue => None,
            FireStep::Reissue(cause) => {
                let end = TailEnd::Return(Some(offer_reissue(op.clone(), cause)));
                Some(RowReissue::new(end))
            }
        }
    }
}

/// Added by decision 124 (plan v4 C7.9, section 2.4 rows "Bag" and "Set", errata V4-17, ruling
/// B-1): the row search of a scan receive and of its E3-a park. The row order is the record
/// order of the pick (CR-1). A set candidate is a class, and the fire binds its DR-75-first
/// member (CR-12). The item test covers every member (C4v2 B.4), and the fire retires the other
/// members that are not persistent (CR-13). A park lists no record (V4-17). A leasing theory
/// gives no alternative (C4v2 G8).
pub(crate) struct ScanReceiveSearch<D> {
    channel: Par,
    records: Vec<Produce>,
    staged: Option<Produce>,
    source: Consume,
    continuation: TaggedContinuation,
    persistent: bool,
    pattern: BindPattern,
    theory: Option<Arc<dyn SpaceTheory>>,
    alternatives: bool,
    classes: Option<(ClassKey, RetireRows)>,
    staged_retires: usize,
    found: Option<FoundClass>,
    /// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD2 1.1 and RADD2v2 7.1): what
    /// the fire of a persistent receive that does not peek keeps for the end of Tail(actual), or
    /// None.
    tail: Option<ScanTail>,
    /// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD2 1.1 and RADD2v2 7.1): the
    /// class rule of the fire that the commit bound in place of c0 (`actual_fire`), until
    /// `tail_reissue`.
    fired_class: Option<bool>,
    /// Added by decision 124 (plan v4 C7.11, ruling RADD2 1.6): the discipline of the collection,
    /// which the machinery search of each retire row names. The search holds no value of it.
    discipline: std::marker::PhantomData<fn() -> D>,
}

/// Added by decision 124 (plan v4 C7.9, ruling B-1): what the last tuple of a scan receive
/// read: the payload of the bound member and the values that it binds, for the theory, and the
/// members that the fire retires.
struct FoundClass {
    payload: ListParWithRandom,
    matched: ListParWithRandom,
    retires: Vec<Member>,
    /// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD2 1.1 and RADD2v2 7.1):
    /// whether a member of the class is persistent, the class rule of CR-9 that `class_of` reads
    /// at the staging. A record without a class is its class alone.
    persistent: bool,
}

/// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD2 1.1, RADD2v2 7.1 and 7.2):
/// what the search of a scan fire keeps for the end of Tail(actual) of a fire that the commit
/// binds in place of c0: the fire count before the fire (`FireRounds::at`) and the copy of the
/// operation (`Cx::operation`). The fire of a persistent receive that does not peek keeps it
/// (`receive_search`), and so does every fire of an offer (`offer_search`).
struct ScanTail {
    fired: usize,
    operation: Arc<SpaceOp>,
}

impl<D: ScanDiscipline> ScanReceiveSearch<D> {
    /// The class of the record `stored` at `index` with the payload `payload`, which the
    /// pattern binds with `matched`: its DR-75-first member, bound, with the members that its
    /// fire retires, or None when a member fails its item test. `tried` holds the classes that
    /// the search met before.
    fn class_tuple(
        &self,
        records: &[ParDatum],
        found: (usize, ListParWithRandom, ListParWithRandom),
        request: &ParRowRequest<'_>,
        tried: &mut Vec<Vec<u8>>,
    ) -> Result<Option<(Member, FoundClass)>, SpaceTxnError> {
        let class_key = match &self.classes {
            Some((class_key, _)) => *class_key,
            None => {
                let (index, payload, matched) = found;
                let stored = records.get(index).ok_or(SpaceTxnError::Inconsistent(
                    "a record is outside the records",
                ))?;
                let first = member_of(stored, matched.clone(), index)?;
                let found = FoundClass {
                    payload,
                    matched,
                    retires: Vec::new(),
                    // Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD2 1.1 and
                    // RADD2v2 7.1): a record without a class is its class alone (`class_of`).
                    persistent: stored.persist,
                };
                return Ok(Some((first, found)));
            }
        };
        // The set binds the DR-75-first member with its own values (CR-12), so the values of
        // the found record are not kept.
        let (_, payload, _) = found;
        let Some(key) = class_key(&payload.pars).map_err(commit_error)? else {
            return Err(SpaceTxnError::Inconsistent("a set record has no class"));
        };
        match tried.contains(&key) {
            true => return Ok(None),
            false => tried.push(key.clone()),
        }
        let key_hash = bincode_digest(&key).map_err(commit_error)?;
        let members = class_members(records, &key, &key_hash, class_key).map_err(commit_error)?;
        let mut first: Option<(Member, FoundClass)> = None;
        let mut retires = Vec::<Member>::with_capacity(members.len());
        // Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD2 1.1 and RADD2v2 7.1):
        // the class rule of CR-9 over the members, as `class_of` reads it at the staging.
        let mut persistent = false;
        for (member, member_payload) in members {
            let stored = records.get(member).ok_or(SpaceTxnError::Inconsistent(
                "a member is outside the records",
            ))?;
            let member_matched = request.matcher.get(&self.pattern, &member_payload).ok_or(
                SpaceTxnError::Inconsistent(
                    "a member of a class does not bind the pattern of its representative",
                ),
            )?;
            if let Some(item) = &request.item {
                match item.admits(&member_matched, stored.persist)? {
                    true => {}
                    false => return Ok(None),
                }
            }
            // Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD2 1.1 and
            // RADD2v2 7.1): see above.
            persistent = persistent || stored.persist;
            match (first.is_some(), stored.persist) {
                (false, _) => {
                    let bound = member_of(stored, member_matched.clone(), member)?;
                    let found = FoundClass {
                        payload: member_payload,
                        matched: member_matched,
                        retires: Vec::new(),
                        // Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD2 1.1
                        // and RADD2v2 7.1): the class rule so far, which the map after the loop
                        // completes.
                        persistent,
                    };
                    first = Some((bound, found));
                }
                (true, true) => {}
                (true, false) => {
                    let retire =
                        RetireRows::member(request.matcher, stored, &member_payload, member)?;
                    retires.push(retire);
                }
            }
        }
        Ok(first.map(|(bound, mut found)| {
            found.retires = retires;
            // Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD2 1.1 and
            // RADD2v2 7.1): the class rule over every member, read after the loop.
            found.persistent = persistent;
            (bound, found)
        }))
    }
}

/// Added by decision 124 (plan v4 C7.9, ruling B-1): the stored member `stored` at `index` of
/// the store of the commit, which binds `matched`.
fn member_of(
    stored: &ParDatum,
    matched: ListParWithRandom,
    index: usize,
) -> Result<Member, SpaceTxnError> {
    let view_index = i32::try_from(index)
        .map_err(|_| SpaceTxnError::Inconsistent("a record index overflows i32"))?;
    Ok(Member {
        record: stored.clone(),
        matched,
        view_index,
    })
}

impl<D: ScanDiscipline> RowSearch<Par, BindPattern, ListParWithRandom, TaggedContinuation>
    for ScanReceiveSearch<D>
{
    fn runs(&self) -> Vec<RowRun<Par>> {
        let mut channels = Vec::<Par>::with_capacity(1);
        channels.push(self.channel.clone());
        vec![RowRun {
            channels,
            candidates: 0..self.records.len(),
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
        let mut tried = Vec::<Vec<u8>>::with_capacity(self.records.len());
        for produce in &self.records {
            let Some(&index) = positions.get(&produce.hash) else {
                continue;
            };
            let Some(stored) = records.get(index) else {
                continue;
            };
            // Added by decision 124 (rules CR-4 and R13): the search of a recorded refusal binds
            // only a record that the refusal names, and the staging of replay can meet another
            // record first. In play and in a selection the lists name every record.
            match lists.named(stored, false)? {
                true => {}
                false => continue,
            }
            let payload = record::unwrap_datum(&stored.a)
                .map_err(|error| commit_error(error.into()))?
                .datum;
            let Some(matched) = request.matcher.get(&self.pattern, &payload) else {
                continue;
            };
            match (&self.classes, &request.item) {
                (None, None) | (Some(_), _) => {}
                (None, Some(item)) => match item.admits(&matched, stored.persist)? {
                    true => {}
                    false => continue,
                },
            }
            let found = (index, payload, matched);
            let Some((first, class)) = self.class_tuple(&records, found, &request, &mut tried)?
            else {
                continue;
            };
            // C4v2 G8: without alternatives only the staged member fires.
            match (
                self.alternatives,
                Some(&first.record.source) == self.staged.as_ref(),
            ) {
                (true, _) | (false, true) => {}
                (false, false) => continue,
            }
            self.found = Some(class);
            let mut tuple = Vec::<ParCandidate>::with_capacity(1);
            tuple.push(stored_candidate(self.channel.clone(), first));
            return Ok(Some(tuple));
        }
        Ok(None)
    }

    fn restore(&mut self) { self.found = None; }

    fn commit_check(
        &mut self,
        request: &ParRowRequest<'_>,
        tuple: &[ParCandidate],
    ) -> Result<bool, SpaceTxnError> {
        let [candidate] = tuple else {
            return Err(SpaceTxnError::Inconsistent(
                "a scan receive binds one record",
            ));
        };
        match Some(&candidate.datum.source) == self.staged.as_ref() {
            false => {}
            true => return Ok(true),
        }
        let admitted = request
            .matcher
            .check_commit_by_reference(request.continuation, &[&candidate.datum.a]);
        match (admitted, &self.theory, &self.found) {
            (false, _, _) => Ok(false),
            (true, None, _) => Ok(true),
            (true, Some(theory), Some(found)) => theory
                .check_match(&found.payload, &found.matched)
                .map(|verdict| verdict.is_proven())
                .map_err(commit_error),
            (true, Some(_), None) => Err(SpaceTxnError::Inconsistent(
                "a scan receive checks a tuple that it did not read",
            )),
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
        // Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD2 1.1 and RADD2v2 7.1):
        // the search keeps the class rule of the fire that the commit binds in place of c0, for
        // its end of Tail(actual) (`tail_reissue`).
        let found = self.found.take();
        self.fired_class = found.as_ref().map(|class| class.persistent);
        let retires = match (&self.classes, found) {
            (Some((_, rows)), Some(found)) => rows.requests::<D>(found.retires),
            (Some(_), None) | (None, _) => Vec::new(),
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
        let records = lists.data(&self.channel)?;
        let mut failed = Vec::<(Blake2b256Hash, Blake2b256Hash)>::with_capacity(records.len());
        for index in canonical_data_order(&records) {
            let Some(stored) = records.get(index) else {
                continue;
            };
            let payload = record::unwrap_datum(&stored.a)
                .map_err(|error| commit_error(error.into()))?
                .datum;
            let Some(matched) = matcher.get(&self.pattern, &payload) else {
                continue;
            };
            let persistent = self.persistent;
            match rule.admit_item(&self.continuation, persistent, 1, &matched, stored.persist)? {
                true => {}
                false => failed.push((stored.source.hash.clone(), self.source.hash.clone())),
            }
        }
        Ok(failed)
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD2 1.1 and RADD2v2 7.1): the
    /// end of Tail(actual) of a fire that the commit bound in place of c0, by the class rule of
    /// the class that it bound (CR-9) at the fire count before it. A fire that continues stops
    /// the operation, which runs again as B.6 row 5 re-issues it (`reissued_operation`). A fire
    /// that re-issues returns the re-issue of the fire loop (`receive_reissue`). Otherwise the
    /// finish builds the end from the receipt.
    fn tail_reissue(&mut self) -> Option<RowReissue> {
        let Some(class_persistent) = self.fired_class.take() else {
            return None;
        };
        let Some(tail) = &self.tail else {
            return None;
        };
        let SpaceOp::Consume(op) = &*tail.operation else {
            return None;
        };
        let spent = !self.persistent;
        let end = match FireRounds::at(tail.fired).after_fire(spent, class_persistent) {
            FireStep::Done => return None,
            FireStep::Continue => TailEnd::Stop(reissued_operation(OpView::Consume(op), None)),
            FireStep::Reissue(cause) => TailEnd::Return(Some(receive_reissue(op.clone(), cause))),
        };
        Some(RowReissue::new(end))
    }
}

#[cfg(test)]
mod tests {
    // Added by decision 124 (plan v4 C7.11, ruling RADD2 1.3): the plans no longer import the
    // map and the collection key (`restored_send` moved to SETTLE), and the tests use both.
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::var::VarInstance;
    use models::rhoapi::{EEq, EVar, Expr, Var};
    use models::rust::space_factory_urn::{SpaceFactoryUrn, SpaceInner, SpaceOuter};
    use models::rust::utils::{new_freevar_par, new_gint_par, new_gstring_par};
    use rspace_plus_plus::rspace::rspace::RSpace;
    use rspace_plus_plus::rspace::rspace_interface::ISpace;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    use rspace_plus_plus::rspace::space_store_handle::SpaceStoreHandle;
    // Changed by decision 102 (plan v4 C5): the replay tests name a recorded COMM.
    // use rspace_plus_plus::rspace::trace::event::{Consume, Event, IOEvent};
    use rspace_plus_plus::rspace::trace::event::{Consume, Event, IOEvent, COMM};
    use serde::Serialize;

    use super::*;
    use crate::rust::interpreter::accounting::costs::Cost;
    use crate::rust::interpreter::accounting::RuntimeBudget;
    use crate::rust::interpreter::matcher::r#match::Matcher;
    use crate::rust::interpreter::reified::costs::FIRE_ROUND_CAP;
    use crate::rust::interpreter::spaces::agent::{
        run_op, SpaceAgent, SpaceCreation, SpaceEnv, SpaceOutcome,
    };
    use crate::rust::interpreter::spaces::disciplines::queue::QueueDiscipline;
    // Changed by decision 102 (plan v4 C5): the replay tests open the collection in a scripted
    // replay.
    // use crate::rust::interpreter::spaces::generic::{GenericSpace, Ordered, OuterStore};
    use crate::rust::interpreter::spaces::generic::{
        begin_collection, prepare_collection_op, CollectionTxn, GenericSpace, Ordered, OuterStore,
    };
    // Added by decision 124 (plan v4 C7.11, ruling RADD2 1.3): see the map above.
    use crate::rust::interpreter::spaces::namespace::CollectionKey;
    use crate::rust::interpreter::spaces::namespace::{self, Durability, NativeSpace, SID_LEN};
    use crate::rust::interpreter::spaces::outer::hashmap::HashMapOuter;
    use crate::rust::interpreter::spaces::record::bincode_bytes;
    // Added by decision 124 (plan v4 C7.11, ruling RADD2v2 7.4): the capture of the staged row
    // searches.
    use crate::rust::interpreter::spaces::test_support::capturing;
    // Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.3): the double of the test
    // of the classes of the rows.
    use crate::rust::interpreter::spaces::test_support::DeviatingCommit;
    // Added by decision 124 (plan v4 C7.11, ruling RADD2v2 7.4, and ruling RADD2v4 7.10): the
    // owner-funded env and the observer of the commit tests.
    use crate::rust::interpreter::spaces::test_support::{owner_funded, Funded};
    // Added by decision 102 (plan v4 C5): the scripted replay of the pick tests.
    use crate::rust::interpreter::spaces::test_support::{recorded, replaying};
    // Added by decision 124 (rules CR-4 and R13): the lists of a recorded refusal.
    use crate::rust::interpreter::spaces::test_support::{with_named_lists, NamedRecords};
    use crate::rust::interpreter::spaces::types::config::SpaceConfig;
    use crate::rust::interpreter::spaces::types::qualifier::SpaceQualifier;

    /// The head of a test discipline: the least `(ord, bytes)`, which the scan driver never uses.
    fn least_head<T: Serialize>(
        side: Side,
        entries: &[Ordered<'_, T>],
    ) -> Result<Option<usize>, SpaceError> {
        QueueDiscipline.head(side, entries)
    }

    /// A scan discipline whose class key is the bincode of the payload, so two records
    /// with equal payloads are one class. It exercises RETIRE-CLASS and the class fire of the
    /// driver, which the set discipline reuses (P5a). The SET batch owns the real set key.
    /// Changed by decision 102 (decision 119 G1, decision 101): the class fire is one bind of the
    /// first member, and RETIRE-CLASS follows. The text before: "It exercises RETIRE-CLASS and
    /// the `[D; n]` fire of the driver, which the set discipline reuses (P5a, F3)."
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct Classed;

    impl Discipline for Classed {
        const NAME: &'static str = "classed";
        const USES_COUNTER: bool = false;
        const SEND_PARAMS: &'static [&'static str] = &[];

        fn head<T: Serialize>(
            &self,
            side: Side,
            entries: &[Ordered<'_, T>],
        ) -> Result<Option<usize>, SpaceError> {
            least_head(side, entries)
        }

        fn reaches_head(&self, _side: Side, _queued: usize) -> bool { true }

        fn precedes(&self, side: Side, ord: i64, head: i64) -> Result<bool, SpaceError> {
            QueueDiscipline.precedes(side, ord, head)
        }

        fn offer(
            &self,
            cx: &mut Cx<'_, '_>,
            op: ProduceOp,
            ord: i64,
        ) -> Result<Option<SpaceReissue>, SpaceError> {
            offer(self, cx, op, ord)
        }

        fn receive(
            &self,
            cx: &mut Cx<'_, '_>,
            op: ConsumeOp,
            ord: i64,
        ) -> Result<Option<SpaceReissue>, SpaceError> {
            receive(self, cx, op, ord)
        }

        fn restore(
            &self,
            cx: &mut Cx<'_, '_>,
            record: ListParWithRandom,
        ) -> Result<Option<SpaceReissue>, SpaceError> {
            restore(self, cx, record)
        }
    }

    impl ScanDiscipline for Classed {
        fn class_key(&self, payload: &[Par]) -> Result<Option<Vec<u8>>, SpaceError> {
            Ok(Some(bincode_bytes(payload)?))
        }
    }

    /// A scan discipline that calls the standalone [`restore`] from its receive plan. A receive
    /// whose body is the integer `v` restores the record of `v`, with the random state of the
    /// body, instead of a receive. Every other receive is RECEIVE-SCAN. The engine never calls
    /// the standalone restore of a scan discipline, so only this harness reaches it.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct Restorer;

    impl Discipline for Restorer {
        const NAME: &'static str = "restorer";
        const USES_COUNTER: bool = false;
        const SEND_PARAMS: &'static [&'static str] = &[];

        fn head<T: Serialize>(
            &self,
            side: Side,
            entries: &[Ordered<'_, T>],
        ) -> Result<Option<usize>, SpaceError> {
            least_head(side, entries)
        }

        fn reaches_head(&self, _side: Side, _queued: usize) -> bool { true }

        fn precedes(&self, side: Side, ord: i64, head: i64) -> Result<bool, SpaceError> {
            QueueDiscipline.precedes(side, ord, head)
        }

        fn offer(
            &self,
            cx: &mut Cx<'_, '_>,
            op: ProduceOp,
            ord: i64,
        ) -> Result<Option<SpaceReissue>, SpaceError> {
            offer(self, cx, op, ord)
        }

        fn receive(
            &self,
            cx: &mut Cx<'_, '_>,
            op: ConsumeOp,
            ord: i64,
        ) -> Result<Option<SpaceReissue>, SpaceError> {
            let restored = match op.body.body.as_ref().map(|body| body.exprs.as_slice()) {
                Some(
                    [Expr {
                        expr_instance: Some(ExprInstance::GInt(value)),
                    }],
                ) => Some(*value),
                _ => None,
            };
            match restored {
                Some(value) => {
                    let datum = ListParWithRandom {
                        pars: vec![int(value)],
                        random_state: op.body.random_state.clone(),
                        cost_authority: None,
                        cost_stack: None,
                    };
                    self.restore(cx, record::wrap_datum(0, &datum))
                }
                None => receive(self, cx, op, ord),
            }
        }

        fn restore(
            &self,
            cx: &mut Cx<'_, '_>,
            record: ListParWithRandom,
        ) -> Result<Option<SpaceReissue>, SpaceError> {
            restore(self, cx, record)
        }
    }

    impl ScanDiscipline for Restorer {}

    type ParRSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

    const SPACE: NativeSpace = NativeSpace {
        sid: [13; SID_LEN],
        durability: Durability::Durable,
    };

    fn int(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

    fn channel() -> Par { new_gstring_par("scan".to_string(), Vec::new(), false) }

    fn key() -> CollectionKey { CollectionKey::of_channel(&channel()).expect("a key") }

    fn data() -> Par { namespace::gprivate(namespace::data_channel(SPACE, &key())) }

    /// A fresh space of the family `hashmap:D` over an in-memory play space.
    async fn fixture<D: Discipline>() -> (ParRSpace, SpaceEnv) {
        let mut stores = InMemoryStoreManager::new();
        let space = ParRSpace::create(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(Matcher)),
        )
        .expect("an in-memory space");
        let env = SpaceEnv {
            store: SpaceStoreHandle::new(space.clone()),
            budget: RuntimeBudget::new(Cost::create(10_000_000, "scan driver test")),
            volatile: Default::default(),
            origin: Default::default(),
        };
        let config = SpaceConfig::new(
            SPACE.sid,
            HashMapOuter::NAME,
            D::NAME,
            SpaceQualifier::Default,
        )
        .to_par();
        let creation = SpaceCreation {
            space: SPACE,
            config,
            handle_pattern: retire_pattern(),
            handle_continuation: TaggedContinuation::default(),
            random: Blake2b512Random::create_from_bytes(b"scan factory call"),
            authority: CostAuthority::default(),
            family: SpaceFactoryUrn {
                outer: SpaceOuter::HashMap,
                inner: SpaceInner::Bag,
            },
        };
        GenericSpace::<HashMapOuter, D>::default()
            .prepare_create(&env, creation)
            .await
            .expect("the creation stages")
            .commit()
            .await
            .expect("the creation commits");
        space.take_event_log().await;
        (space, env)
    }

    async fn run<D: Discipline>(env: &SpaceEnv, op: SpaceOp) -> Result<SpaceOutcome, SpaceError> {
        run_op(&GenericSpace::<HashMapOuter, D>::default(), env, op).await
    }

    /// A send of `value` whose random state comes from `salt`.
    fn send(value: i64, salt: u64, persistent: bool) -> ProduceOp {
        ProduceOp {
            space: SPACE,
            key: key(),
            channel: channel(),
            data: ListParWithRandom {
                pars: vec![int(value)],
                random_state: Blake2b512Random::create_from_bytes(
                    format!("scan send {value} {salt}").as_bytes(),
                )
                .to_bytes(),
                cost_authority: None,
                cost_stack: None,
            },
            persistent,
            params: BTreeMap::new(),
            authority: CostAuthority::default(),
            reissued_ord: None,
        }
    }

    /// The source hash of the record of `op`, the key of the canonical order of the records.
    fn record_hash(op: &ProduceOp) -> Blake2b256Hash {
        Produce::create(&data(), &record::wrap_datum(0, &op.data), op.persistent).hash
    }

    /// Sends of `(value, persistent)` whose records follow each other in canonical order. The
    /// salts are searched in a fixed order, so the sends are the same in every run.
    fn ordered_sends(entries: &[(i64, bool)]) -> Vec<ProduceOp> {
        let mut sends: Vec<ProduceOp> = Vec::with_capacity(entries.len());
        let mut salt = 0;
        for (value, persistent) in entries {
            loop {
                salt += 1;
                let candidate = send(*value, salt, *persistent);
                let follows = sends
                    .last()
                    .is_none_or(|last| record_hash(last) < record_hash(&candidate));
                if follows {
                    sends.push(candidate);
                    break;
                }
            }
        }
        sends
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

    /// The guard `x == value` over the one bound value `x`.
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

    /// A receive whose body is the string `name`.
    fn receive_op(name: &str, pattern: BindPattern, guard: Option<Par>, peek: bool) -> ConsumeOp {
        ConsumeOp {
            space: SPACE,
            key: key(),
            channel: channel(),
            pattern,
            body: ParWithRandom {
                body: Some(new_gstring_par(name.to_string(), Vec::new(), false)),
                random_state: Blake2b512Random::create_from_bytes(name.as_bytes()).to_bytes(),
            },
            persistent: false,
            peek,
            guard,
            query: None,
            authority: CostAuthority::default(),
            introduction_authority: CostAuthority::default(),
            reissued_ord: None,
        }
    }

    /// The receive of [`Restorer`] that restores the record of `value`.
    fn restore_op(value: i64) -> ConsumeOp {
        ConsumeOp {
            body: ParWithRandom {
                body: Some(int(value)),
                random_state: Blake2b512Random::create_from_bytes(b"restored").to_bytes(),
            },
            ..receive_op("unused", any(), None, false)
        }
    }

    /// The values of the records of the collection with their persist flags, sorted.
    async fn values(space: &ParRSpace) -> Vec<(i64, bool)> {
        let mut values: Vec<(i64, bool)> = space
            .get_data(&data())
            .await
            .iter()
            .map(|datum| {
                let record = record::unwrap_datum(&datum.a).expect("a datum record");
                let value = match record.datum.pars.as_slice() {
                    [Par { exprs, .. }] => match exprs.as_slice() {
                        [Expr {
                            expr_instance: Some(ExprInstance::GInt(value)),
                        }] => *value,
                        _ => panic!("a record of one number"),
                    },
                    _ => panic!("a record of one number"),
                };
                (value, datum.persist)
            })
            .collect();
        values.sort();
        values
    }

    /// The number of waiting continuations of the collection.
    async fn waiting(space: &ParRSpace) -> usize {
        space.get_waiting_continuations(vec![data()]).await.len()
    }

    /// The events of a log as `kind width` labels, with `peek` for a COMM with peeks.
    fn labels(log: &[Event]) -> Vec<String> {
        log.iter()
            .map(|event| match event {
                Event::IoEvent(IOEvent::Produce(_)) => "produce".to_string(),
                Event::IoEvent(IOEvent::Consume(consume)) => {
                    format!("consume {}", consume.channel_hashes.len())
                }
                Event::Comm(comm) => match comm.peeks.is_empty() {
                    true => format!("comm {}", comm.consume.channel_hashes.len()),
                    false => format!("comm {} peek", comm.consume.channel_hashes.len()),
                },
            })
            .collect()
    }

    /// Runs each send of `sends` with the discipline `D`.
    async fn store_all<D: Discipline>(env: &SpaceEnv, sends: &[ProduceOp]) {
        for op in sends {
            run::<D>(env, SpaceOp::Produce(op.clone()))
                .await
                .expect("the send commits");
        }
    }

    /// Added by decision 102 (rule CR-1): the runs of a play test of a random pick, each on a
    /// fresh space. A pick of two candidates keeps one order in all runs with probability
    /// 2^-63.
    const PICK_RUNS: usize = 64;

    /// Added by decision 102 (rule CR-1, decision 37): the fresh spaces that
    /// [`parked_by_a_refused_pick`] tries. Each try parks with probability 1/3 or more, so no
    /// try parks with probability (2/3)^64, about 10^-11, or less.
    const PARK_TRIES: usize = 64;

    /// Added by decision 102 (rule CR-1, decision 37): a fresh space of the family `hashmap:D`
    /// that stores `stored` and holds the guarded receive `op` as a waiter. Play picks the
    /// record of a receive at random and checks the guard once, on the class of the pick. So
    /// the receive parks when its pick meets a record that the guard refuses, and it fires
    /// otherwise. A fire is the other allowed outcome, and the next try starts on a new space.
    async fn parked_by_a_refused_pick<D: Discipline>(
        stored: &[ProduceOp],
        op: &ConsumeOp,
    ) -> (ParRSpace, SpaceEnv) {
        for _ in 0..PARK_TRIES {
            let (space, env) = fixture::<D>().await;
            store_all::<D>(&env, stored).await;
            let outcome = run::<D>(&env, SpaceOp::Consume(op.clone()))
                .await
                .expect("the receive commits");
            match outcome.fires.as_slice() {
                [] => {
                    assert_eq!(waiting(&space).await, 1, "the receive parks");
                    space.take_event_log().await;
                    return (space, env);
                }
                [_] => continue,
                fires => panic!("a receive fires once or parks: {fires:?}"),
            }
        }
        panic!("no try of {PARK_TRIES} picks a record that the guard refuses");
    }

    /// Added by decision 102 (plan v4 C5): the open collection of the test space in a scripted
    /// replay ([`replaying`]) whose recorded row is `comms` for every source.
    async fn replay_collection(env: &SpaceEnv, comms: Option<Vec<COMM>>) -> CollectionTxn {
        let mut opened = begin_collection(&HashMapOuter, env, SPACE, &key(), &channel())
            .await
            .expect("phase A");
        opened.txn = replaying(opened.txn, comms);
        opened
    }

    /// Added by decision 102 (plan v4 C5): runs `op` with the discipline `D` in a scripted
    /// replay whose recorded row is `comms`, and commits it in the play store.
    async fn run_replaying<D: Discipline>(
        env: &SpaceEnv,
        comms: Option<Vec<COMM>>,
        op: SpaceOp,
    ) -> Result<SpaceOutcome, SpaceError> {
        let opened = replay_collection(env, comms).await;
        prepare_collection_op(&GenericSpace::<HashMapOuter, D>::default(), env, opened, op)
            .await?
            .commit()
            .await
    }

    /// Added by decision 102 (plan v4 C5): the consume of a recorded COMM that names records
    /// only. A data pick reads the produces of the COMM, and not its consume.
    fn unnamed_consume() -> Consume {
        Consume::create(
            &vec![data()],
            &vec![any()],
            &TaggedContinuation::default(),
            false,
        )
    }

    /// Added by decision 102 (plan v4 C5): the string body of `continuation`, which is the name
    /// of its receive in [`receive_op`].
    fn body_name(continuation: &TaggedContinuation) -> String {
        match &continuation.tagged_cont {
            Some(TaggedCont::ParBody(body)) => match body.body.as_ref() {
                Some(Par { exprs, .. }) => match exprs.as_slice() {
                    [Expr {
                        expr_instance: Some(ExprInstance::GString(name)),
                    }] => name.clone(),
                    _ => panic!("a body of one string: {continuation:?}"),
                },
                None => panic!("a body of one string: {continuation:?}"),
            },
            other => panic!("a body continuation: {other:?}"),
        }
    }

    #[test]
    fn scan_retire_continuation_has_the_member_body_and_the_retire_shape() {
        let source = Blake2b256Hash::new(b"member");
        let random = RandomPurpose::Retire.derive(&Blake2b512Random::create_from_bytes(b"op"));
        let authority = CostAuthority::default();
        let continuation = retire_continuation(&source, &random, Some(authority.clone()));
        assert_eq!(continuation.guard, None);
        assert_eq!(continuation.cost_authority, Some(authority));
        match &continuation.tagged_cont {
            Some(TaggedCont::ParBody(body)) => {
                assert_eq!(
                    body.body,
                    Some(new_gbytearray_par(source.bytes(), Vec::new(), false))
                );
                assert_eq!(body.random_state, random.to_bytes());
            }
            other => panic!("a body continuation: {other:?}"),
        }
        let pattern = retire_pattern();
        assert!(pattern.patterns.is_empty());
        assert_eq!(pattern.free_count, 1);
        let other = retire_continuation(&Blake2b256Hash::new(b"other"), &random, None);
        assert_ne!(
            Consume::create(&vec![data()], &vec![pattern.clone()], &continuation, false).hash,
            Consume::create(&vec![data()], &vec![pattern], &other, false).hash,
            "two members give two retire identities under one random state"
        );
    }

    #[test]
    fn scan_same_class_is_the_equality_of_the_full_keys() {
        let key = vec![1, 2, 3];
        let key_hash = bincode_digest(&key).expect("the hash of the key");
        assert_eq!(same_class(&key, &key_hash, &[1, 2, 3]), Ok(true));
        assert_eq!(same_class(&key, &key_hash, &[1, 2, 4]), Ok(false));
        assert_eq!(same_class(&key, &key_hash, &[9, 9, 9]), Ok(false));
    }

    // Retired by decision 102 (decision 119 G1, decision 101, rules CR-12 and CR-13): the class
    // fire is one bind of the first member over `[D]`, then the retire rows of the other members,
    // and play picks at random. `scan_class_fire_binds_the_first_member_and_retires_the_others`
    // checks the new rule.
    // #[tokio::test]
    // async fn scan_class_fire_binds_every_member_and_dispatches_the_first() {
    //     let (space, env) = fixture::<Classed>().await;
    //     let members = ordered_sends(&[(1, false), (1, false)]);
    //     store_all::<Classed>(&env, &members).await;
    //     store_all::<Classed>(&env, &[send(2, 99, false)]).await;
    //     space.take_event_log().await;
    //     let outcome = run::<Classed>(
    //         &env,
    //         SpaceOp::Consume(receive_op("k", exactly(1), None, false)),
    //     )
    //     .await
    //     .expect("the receive commits");
    //     let [fire] = outcome.fires.as_slice() else {
    //         panic!("one fire: {outcome:?}");
    //     };
    //     let [result] = fire.results.as_slice() else {
    //         panic!("the fire dispatches the result of the first member only: {fire:?}");
    //     };
    //     assert_eq!(
    //         result.removed_datum,
    //         record::wrap_datum(0, &members[0].data),
    //         "the first member in canonical order is the representative"
    //     );
    //     assert_eq!(labels(&space.take_event_log().await), vec![
    //         "consume 2",
    //         "comm 2"
    //     ]);
    //     assert_eq!(values(&space).await, vec![(2, false)]);
    // }

    /// Added by decision 102 (decision 119 G1, decision 101, rules CR-12 and CR-13): a receive
    /// that does not peek binds the first member of the class in DR-75 order under every pick.
    /// The fire is one bind over `[D]`. The retire rows of the other members follow in DR-75
    /// order, with the retire random state of the receive and its stamped authority. A retire is
    /// not dispatched. Each run picks at random, on a fresh space.
    #[tokio::test]
    async fn scan_class_fire_binds_the_first_member_and_retires_the_others() {
        let members = ordered_sends(&[(1, false), (1, false), (1, false)]);
        let op = receive_op("k", exactly(1), None, false);
        let random =
            RandomPurpose::Retire.derive(&Blake2b512Random::from_bytes(&op.body.random_state));
        let retire = |member: &ProduceOp| {
            Event::IoEvent(IOEvent::Consume(Consume::create(
                &vec![data()],
                &vec![retire_pattern()],
                &retire_continuation(&record_hash(member), &random, None),
                false,
            )))
        };
        for _ in 0..PICK_RUNS {
            let (space, env) = fixture::<Classed>().await;
            store_all::<Classed>(&env, &members).await;
            store_all::<Classed>(&env, &[send(2, 99, false)]).await;
            space.take_event_log().await;
            let outcome = run::<Classed>(&env, SpaceOp::Consume(op.clone()))
                .await
                .expect("the receive commits");
            let [fire] = outcome.fires.as_slice() else {
                panic!("one fire, and no retire is dispatched: {outcome:?}");
            };
            let [result] = fire.results.as_slice() else {
                panic!("the fire binds one member: {fire:?}");
            };
            assert_eq!(
                result.removed_datum,
                record::wrap_datum(0, &members[0].data),
                "the first member in DR-75 order is dispatched"
            );
            let log = space.take_event_log().await;
            assert_eq!(labels(&log), vec![
                "consume 1",
                "comm 1",
                "consume 1",
                "comm 1",
                "consume 1",
                "comm 1"
            ]);
            assert_eq!(log[2], retire(&members[1]));
            assert_eq!(log[4], retire(&members[2]));
            assert_eq!(values(&space).await, vec![(2, false)]);
        }
    }

    /// Added by decision 102 (decision 119 G1, rules CR-9 and CR-13): the fire binds the first
    /// member in DR-75 order also when it is persistent, and it stays. A persistent member has
    /// no retire row. Only the other members that are not persistent are retired.
    #[tokio::test]
    async fn scan_class_fire_retires_only_the_members_that_are_not_persistent() {
        let cases = [
            (ordered_sends(&[(1, false), (1, true), (1, false)]), 2),
            (ordered_sends(&[(1, true), (1, false)]), 1),
        ];
        let op = receive_op("k", exactly(1), None, false);
        let random =
            RandomPurpose::Retire.derive(&Blake2b512Random::from_bytes(&op.body.random_state));
        for (members, retired) in &cases {
            for _ in 0..PICK_RUNS {
                let (space, env) = fixture::<Classed>().await;
                store_all::<Classed>(&env, members).await;
                space.take_event_log().await;
                let outcome = run::<Classed>(&env, SpaceOp::Consume(op.clone()))
                    .await
                    .expect("the receive commits");
                let [fire] = outcome.fires.as_slice() else {
                    panic!("one fire: {outcome:?}");
                };
                let [result] = fire.results.as_slice() else {
                    panic!("the fire binds one member: {fire:?}");
                };
                assert_eq!(
                    result.removed_datum,
                    record::wrap_datum(0, &members[0].data),
                    "the first member in DR-75 order is dispatched"
                );
                let log = space.take_event_log().await;
                assert_eq!(labels(&log), vec![
                    "consume 1",
                    "comm 1",
                    "consume 1",
                    "comm 1"
                ]);
                let retire = Consume::create(
                    &vec![data()],
                    &vec![retire_pattern()],
                    &retire_continuation(&record_hash(&members[*retired]), &random, None),
                    false,
                );
                assert_eq!(log[2], Event::IoEvent(IOEvent::Consume(retire)));
                assert_eq!(
                    values(&space).await,
                    vec![(1, true)],
                    "the persistent member stays"
                );
            }
        }
    }

    // Retired by decision 102 (rules CR-1 and CR-13): play picks at random, so a peek binds a
    // random member. `scan_class_peek_binds_and_restores_the_pick_only` checks the new rule.
    // #[tokio::test]
    // async fn scan_class_peek_binds_and_restores_the_representative_only() {
    //     let (space, env) = fixture::<Classed>().await;
    //     let members = ordered_sends(&[(1, false), (1, false)]);
    //     store_all::<Classed>(&env, &members).await;
    //     space.take_event_log().await;
    //     let outcome = run::<Classed>(
    //         &env,
    //         SpaceOp::Consume(receive_op("p", exactly(1), None, true)),
    //     )
    //     .await
    //     .expect("the peek commits");
    //     assert_eq!(outcome.fires.len(), 1);
    //     let log = space.take_event_log().await;
    //     assert_eq!(labels(&log), vec!["consume 1", "comm 1 peek", "produce"]);
    //     let restored = log
    //         .iter()
    //         .find_map(|event| match event {
    //             Event::IoEvent(IOEvent::Produce(produce)) => Some(produce.hash.clone()),
    //             _ => None,
    //         })
    //         .expect("the restore sends the record again");
    //     assert_eq!(restored, record_hash(&members[0]));
    //     assert_eq!(values(&space).await, vec![(1, false), (1, false)]);
    // }

    /// Added by decision 102 (rules CR-1 and CR-13, decision 45 Rule E): a peek binds its pick
    /// alone and retires nothing, and RESTORE-SCAN sends that record again. Play picks at
    /// random, so over the runs the peek binds and sends again each member of the class.
    #[tokio::test]
    async fn scan_class_peek_binds_and_restores_the_pick_only() {
        let members = ordered_sends(&[(1, false), (1, false)]);
        let mut restored = BTreeSet::<Blake2b256Hash>::new();
        for _ in 0..PICK_RUNS {
            let (space, env) = fixture::<Classed>().await;
            store_all::<Classed>(&env, &members).await;
            space.take_event_log().await;
            let outcome = run::<Classed>(
                &env,
                SpaceOp::Consume(receive_op("p", exactly(1), None, true)),
            )
            .await
            .expect("the peek commits");
            let [fire] = outcome.fires.as_slice() else {
                panic!("one fire: {outcome:?}");
            };
            let [result] = fire.results.as_slice() else {
                panic!("the peek binds one member: {fire:?}");
            };
            let log = space.take_event_log().await;
            assert_eq!(labels(&log), vec!["consume 1", "comm 1 peek", "produce"]);
            let sent = log
                .iter()
                .find_map(|event| match event {
                    Event::IoEvent(IOEvent::Produce(produce)) => Some(produce.hash.clone()),
                    _ => None,
                })
                .expect("the restore sends the record again");
            assert_eq!(
                sent,
                Produce::create(&data(), &result.removed_datum, false).hash,
                "the restore sends the bound record again"
            );
            assert_eq!(values(&space).await, vec![(1, false), (1, false)]);
            restored.insert(sent);
        }
        let every_member: BTreeSet<Blake2b256Hash> = members.iter().map(record_hash).collect();
        assert_eq!(restored, every_member, "the peek binds a random member");
    }

    /// Added by decision 102 (plan v4 C5.1, rule CR-4): in replay a receive binds first the
    /// record that the recorded COMM names, and then the other records in DR-75 order. Each
    /// record is named once, so one case binds a record that is not first in DR-75 order.
    #[tokio::test]
    async fn scan_receive_binds_the_named_record_first_in_replay() {
        let stored = ordered_sends(&[(1, false), (2, false)]);
        for (named, left) in [(0, 2), (1, 1)] {
            let (space, env) = fixture::<Restorer>().await;
            store_all::<Restorer>(&env, &stored).await;
            space.take_event_log().await;
            let produce =
                Produce::create(&data(), &record::wrap_datum(0, &stored[named].data), false);
            let comms = vec![recorded(unnamed_consume(), vec![produce])];
            let outcome = run_replaying::<Restorer>(
                &env,
                Some(comms),
                SpaceOp::Consume(receive_op("k", any(), None, false)),
            )
            .await
            .expect("the receive commits");
            let [fire] = outcome.fires.as_slice() else {
                panic!("one fire: {outcome:?}");
            };
            let [result] = fire.results.as_slice() else {
                panic!("the fire binds one record: {fire:?}");
            };
            assert_eq!(
                result.removed_datum,
                record::wrap_datum(0, &stored[named].data),
                "the receive binds the named record"
            );
            assert_eq!(
                values(&space).await,
                vec![(left, false)],
                "the other record stays"
            );
        }
    }

    /// Added by decision 102 (plan v4 C5.1, rule CR-4): in replay an offer fires first the
    /// waiter whose source is the consume of the recorded COMM. Each waiter is named once, so
    /// one case fires a waiter that is not first in DR-75 order.
    #[tokio::test]
    async fn scan_offer_fires_the_named_waiter_first_in_replay() {
        for named in ["a", "b"] {
            let (space, env) = fixture::<Restorer>().await;
            for name in ["a", "b"] {
                let parked =
                    run::<Restorer>(&env, SpaceOp::Consume(receive_op(name, any(), None, false)))
                        .await
                        .expect("the receive parks");
                assert!(parked.fires.is_empty());
            }
            space.take_event_log().await;
            let waiters = space.get_waiting_continuations(vec![data()]).await;
            let source = waiters
                .iter()
                .find(|waiter| {
                    let unwrapped =
                        record::unwrap_continuation(&waiter.continuation).expect("a waiter record");
                    body_name(&unwrapped.continuation) == named
                })
                .expect("the named waiter")
                .source
                .clone();
            let comms = vec![recorded(source, Vec::new())];
            let outcome =
                run_replaying::<Restorer>(&env, Some(comms), SpaceOp::Produce(send(5, 1, false)))
                    .await
                    .expect("the send commits");
            let [fire] = outcome.fires.as_slice() else {
                panic!("one fire: {outcome:?}");
            };
            assert_eq!(
                body_name(&fire.continuation),
                named,
                "the offer fires the named waiter"
            );
            assert_eq!(waiting(&space).await, 1, "the other waiter stays");
            assert!(values(&space).await.is_empty());
        }
    }

    /// Added by decision 102 (plan v4 C5.6, rule CR-7, base E3-a): in replay a receive whose
    /// recorded row is Stored and whose commit may veto parks without a search, although a
    /// stored record passes its guard. Without the Stored row, or with a commit that cannot
    /// veto, the receive searches and fires. Play reads no row, so it searches.
    #[tokio::test]
    async fn scan_receive_parks_without_a_search_on_a_stored_row_that_may_veto() {
        let cases: [(Option<Vec<COMM>>, Option<Par>, bool, bool); 4] = [
            (Some(Vec::new()), Some(equals(1)), true, true),
            (None, Some(equals(1)), true, false),
            (Some(Vec::new()), None, true, false),
            (None, Some(equals(1)), false, false),
        ];
        for (comms, guard, replay, parks) in cases {
            let (space, env) = fixture::<Restorer>().await;
            store_all::<Restorer>(&env, &[send(1, 1, false)]).await;
            space.take_event_log().await;
            let op = SpaceOp::Consume(receive_op("k", any(), guard.clone(), false));
            let outcome = match replay {
                true => run_replaying::<Restorer>(&env, comms.clone(), op).await,
                false => run::<Restorer>(&env, op).await,
            }
            .expect("the receive commits");
            let case = format!("row {comms:?}, guard {}, replay {replay}", guard.is_some());
            match parks {
                true => {
                    assert!(outcome.fires.is_empty(), "{case}");
                    assert_eq!(waiting(&space).await, 1, "{case}");
                    assert_eq!(values(&space).await, vec![(1, false)], "{case}");
                    assert_eq!(labels(&space.take_event_log().await), vec!["consume 1"]);
                }
                false => {
                    assert_eq!(outcome.fires.len(), 1, "{case}");
                    assert_eq!(waiting(&space).await, 0, "{case}");
                    assert!(values(&space).await.is_empty(), "{case}");
                }
            }
        }
    }

    #[tokio::test]
    async fn scan_produce_fire_retires_the_stored_members_of_its_class() {
        // Changed by decision 102 (rule CR-1): play picks the record of the receive at random,
        // so the waiter parks when its pick meets the 1 (decision 37: the guard is checked
        // once). The text before:
        // let (space, env) = fixture::<Classed>().await;
        // // The stored 1 precedes the stored 2, so the guarded waiter meets 1 first and parks
        // // (decision 37: the guard is checked once).
        // let stored = ordered_sends(&[(1, false), (2, false)]);
        // store_all::<Classed>(&env, &stored).await;
        // let parked = run::<Classed>(
        //     &env,
        //     SpaceOp::Consume(receive_op("w", any(), Some(equals(2)), false)),
        // )
        // .await
        // .expect("the receive commits");
        // assert!(parked.fires.is_empty());
        // assert_eq!(waiting(&space).await, 1);
        // space.take_event_log().await;
        let stored = ordered_sends(&[(1, false), (2, false)]);
        let (space, env) = parked_by_a_refused_pick::<Classed>(
            &stored,
            &receive_op("w", any(), Some(equals(2)), false),
        )
        .await;
        // An equal send fires the waiter with the incoming 2, and the retire takes the stored 2.
        let incoming = send(2, 1_000, false);
        let outcome = run::<Classed>(&env, SpaceOp::Produce(incoming.clone()))
            .await
            .expect("the send commits");
        assert_eq!(outcome.fires.len(), 1, "the retire is not dispatched");
        let log = space.take_event_log().await;
        assert_eq!(labels(&log), vec![
            "produce",
            "comm 1",
            "consume 1",
            "comm 1"
        ]);
        let retire = Consume::create(
            &vec![data()],
            &vec![retire_pattern()],
            &retire_continuation(
                &record_hash(&stored[1]),
                &RandomPurpose::Retire
                    .derive(&Blake2b512Random::from_bytes(&incoming.data.random_state)),
                None,
            ),
            false,
        );
        assert_eq!(log[2], Event::IoEvent(IOEvent::Consume(retire)));
        assert_eq!(values(&space).await, vec![(1, false)]);
        assert_eq!(waiting(&space).await, 0);
    }

    #[tokio::test]
    async fn scan_retire_keeps_persistent_members_and_other_classes() {
        // Changed by decision 102 (rule CR-1): the waiter parks when its random pick meets the
        // 1. The text before:
        // let (space, env) = fixture::<Classed>().await;
        // let stored = ordered_sends(&[(1, false), (2, false), (2, true)]);
        // store_all::<Classed>(&env, &stored).await;
        // run::<Classed>(
        //     &env,
        //     SpaceOp::Consume(receive_op("w", any(), Some(equals(2)), false)),
        // )
        // .await
        // .expect("the waiter parks");
        // space.take_event_log().await;
        let stored = ordered_sends(&[(1, false), (2, false), (2, true)]);
        let (space, env) = parked_by_a_refused_pick::<Classed>(
            &stored,
            &receive_op("w", any(), Some(equals(2)), false),
        )
        .await;
        let outcome = run::<Classed>(&env, SpaceOp::Produce(send(2, 1_000, false)))
            .await
            .expect("the send commits");
        assert_eq!(outcome.fires.len(), 1);
        assert_eq!(labels(&space.take_event_log().await), vec![
            "produce",
            "comm 1",
            "consume 1",
            "comm 1"
        ]);
        assert_eq!(
            values(&space).await,
            vec![(1, false), (2, true)],
            "the persistent member and the other class stay"
        );
    }

    #[tokio::test]
    async fn scan_peek_waiter_fire_retires_nothing() {
        // Changed by decision 102 (rule CR-1): the peek waiter parks when its random pick meets
        // the 1. The text before:
        // let (space, env) = fixture::<Classed>().await;
        // let stored = ordered_sends(&[(1, false), (2, false)]);
        // store_all::<Classed>(&env, &stored).await;
        // run::<Classed>(
        //     &env,
        //     SpaceOp::Consume(receive_op("p", any(), Some(equals(2)), true)),
        // )
        // .await
        // .expect("the peek waiter parks");
        // space.take_event_log().await;
        let stored = ordered_sends(&[(1, false), (2, false)]);
        let (space, env) = parked_by_a_refused_pick::<Classed>(
            &stored,
            &receive_op("p", any(), Some(equals(2)), true),
        )
        .await;
        let outcome = run::<Classed>(&env, SpaceOp::Produce(send(2, 1_000, false)))
            .await
            .expect("the send commits");
        assert_eq!(outcome.fires.len(), 1);
        assert_eq!(
            labels(&space.take_event_log().await),
            vec!["produce", "comm 1 peek", "produce"],
            "the datum stays in flight and is stored, and no retire runs"
        );
        assert_eq!(values(&space).await, vec![
            (1, false),
            (2, false),
            (2, false)
        ]);
    }

    #[tokio::test]
    async fn scan_restore_outside_a_receive_offers_then_stores() {
        let (space, env) = fixture::<Restorer>().await;
        store_all::<Restorer>(&env, &[send(2, 1, false)]).await;
        // The guarded waiter meets the 2 and parks.
        run::<Restorer>(
            &env,
            SpaceOp::Consume(receive_op("w", any(), Some(equals(1)), false)),
        )
        .await
        .expect("the waiter parks");
        space.take_event_log().await;
        let outcome = run::<Restorer>(&env, SpaceOp::Consume(restore_op(1)))
            .await
            .expect("the restore commits");
        assert_eq!(
            outcome.fires.len(),
            1,
            "the restored 1 fires the guarded waiter"
        );
        assert_eq!(labels(&space.take_event_log().await), vec![
            "produce", "comm 1"
        ]);
        assert_eq!(values(&space).await, vec![(2, false)]);
        let outcome = run::<Restorer>(&env, SpaceOp::Consume(restore_op(3)))
            .await
            .expect("the restore commits");
        assert!(outcome.fires.is_empty());
        assert_eq!(labels(&space.take_event_log().await), vec!["produce"]);
        assert_eq!(values(&space).await, vec![(2, false), (3, false)]);
    }

    #[tokio::test]
    async fn scan_restore_outside_a_receive_fails_closed_at_the_cap() {
        let (space, env) = fixture::<Restorer>().await;
        store_all::<Restorer>(&env, &[send(2, 1, false)]).await;
        for index in 0..FIRE_ROUND_CAP {
            run::<Restorer>(
                &env,
                SpaceOp::Consume(receive_op(
                    &format!("p{index}"),
                    any(),
                    Some(equals(1)),
                    true,
                )),
            )
            .await
            .expect("the guarded peek waiter parks");
        }
        space.take_event_log().await;
        assert_eq!(
            run::<Restorer>(&env, SpaceOp::Consume(restore_op(1))).await,
            Err(SpaceError::Inconsistent(
                "a restore outside a receive cannot re-issue its record"
            ))
        );
        assert!(space.take_event_log().await.is_empty(), "nothing commits");
        assert_eq!(values(&space).await, vec![(2, false)]);
        assert_eq!(waiting(&space).await, FIRE_ROUND_CAP);
    }

    // ── Decision 124: the machinery seal, the inert machinery waiters and the book ──────────

    // Added by decision 124 (plan v2 C7.6 and C7.10, decisions 99 and 100): the check of the
    // retire seal builds the seal of an authority with a region, and the check of the inert
    // machinery waiters parks a `K_lift`.
    use models::rhoapi::cost_signature::Value as CostSignatureValue;
    use models::rhoapi::{CostRegion, CostSignature};

    use crate::rust::interpreter::accounting::authority::system_residue_authority_with_caller;
    use crate::rust::interpreter::spaces::settle::{lift_continuation, lift_pattern};

    /// Added by decision 124 (plan v2 C7.6, decision 99): an unpriced region with a ground
    /// signature, so its system residue builds.
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

    /// Added by decision 124 (plan v2 C7.6, decision 99): the retire rows of a class fire take
    /// the machinery seal of the authority of the receive, an authority with a region. Three
    /// members of one class are stored, and the receive k binds the first member and retires the
    /// other two. Each retire row carries the seal.
    #[tokio::test]
    async fn scan_retire_rows_take_the_seal() {
        let (space, env) = fixture::<Classed>().await;
        let members = ordered_sends(&[(1, false), (1, false), (1, false)]);
        store_all::<Classed>(&env, &members).await;
        let authority = signed_region(2);
        let seal = system_residue_authority_with_caller(&authority, &env.budget.deploy_id(), None)
            .expect("the residue of a signed region");
        let op = ConsumeOp {
            authority,
            ..receive_op("k", exactly(1), None, false)
        };
        let random =
            RandomPurpose::Retire.derive(&Blake2b512Random::from_bytes(&op.body.random_state));
        space.take_event_log().await;
        run::<Classed>(&env, SpaceOp::Consume(op))
            .await
            .expect("the receive commits");
        let log = space.take_event_log().await;
        for member in &members[1..] {
            let retire = Consume::create(
                &vec![data()],
                &vec![retire_pattern()],
                &retire_continuation(&record_hash(member), &random, Some(seal.clone())),
                false,
            );
            assert!(
                log.contains(&Event::IoEvent(IOEvent::Consume(retire))),
                "the retire row of a member carries the seal"
            );
        }
    }

    /// Added by decision 124 (plan v2 C7.10, decision 100): a `K_retire` and a `K_lift` that sit
    /// in the store before a send are inert machinery waiters. The scan of the offer skips both,
    /// although their patterns bind every payload, so neither fires and both stay.
    #[tokio::test]
    async fn readers_skip_an_inert_machinery_waiter() {
        let (space, env) = fixture::<Classed>().await;
        let random = Blake2b512Random::create_from_bytes(b"inert machinery");
        let retire = retire_continuation(&record_hash(&send(1, 0, false)), &random, None);
        let lift = lift_continuation(0, 0, &random, None);
        let waiters = [(retire_pattern(), retire), (lift_pattern(), lift)];
        for (pattern, continuation) in waiters {
            space
                .consume(
                    vec![data()],
                    vec![pattern],
                    continuation,
                    false,
                    BTreeSet::new(),
                )
                .await
                .expect("an inert machinery waiter parks");
        }
        let outcome = run::<Classed>(&env, SpaceOp::Produce(send(1, 0, false)))
            .await
            .expect("the send commits");
        assert!(outcome.fires.is_empty(), "the send fires no inert waiter");
        assert_eq!(waiting(&space).await, 2, "both inert waiters stay");
    }

    /// Added by decision 124 (plan v4 C7.8, rule CR-20, ruling OP-F13): the pair of the one COMM
    /// of the probe log `log`: the hash of its first produce and the hash of its consume.
    fn probe_pair(log: &[Event]) -> (Blake2b256Hash, Blake2b256Hash) {
        let comms: Vec<&COMM> = log
            .iter()
            .filter_map(|event| match event {
                Event::Comm(comm) => Some(comm),
                Event::IoEvent(_) => None,
            })
            .collect();
        match comms.as_slice() {
            [comm] => match comm.produces.first() {
                Some(produce) => (produce.hash.clone(), comm.consume.hash.clone()),
                None => panic!("the COMM of the probe binds a record: {comm:?}"),
            },
            other => panic!("the probe fires once: {other:?}"),
        }
    }

    /// Added by decision 124 (plan v4 C7.8, rule CR-20, ruling OP-F13): the scan of an offer
    /// skips a waiter whose pair with the element the book holds, as a waiter whose guard
    /// refuses. A probe space parks the receive r and sends 1, and its COMM gives the pair. A
    /// fresh space parks r, books the pair and sends 1: the send stores its datum, and r stays
    /// parked.
    #[tokio::test]
    async fn scan_offer_skips_a_waiter_whose_pair_is_booked() {
        let parked = receive_op("r", any(), None, false);
        let (probe, env) = fixture::<Classed>().await;
        run::<Classed>(&env, SpaceOp::Consume(parked.clone()))
            .await
            .expect("the receive parks");
        run::<Classed>(&env, SpaceOp::Produce(send(1, 0, false)))
            .await
            .expect("the send fires");
        let pair = probe_pair(&probe.take_event_log().await);
        let (space, env) = fixture::<Classed>().await;
        run::<Classed>(&env, SpaceOp::Consume(parked))
            .await
            .expect("the receive parks");
        env.volatile.book_pairs([pair]);
        run::<Classed>(&env, SpaceOp::Produce(send(1, 0, false)))
            .await
            .expect("the send commits");
        let log = space.take_event_log().await;
        assert!(
            log.iter().all(|event| matches!(event, Event::IoEvent(_))),
            "the booked pair does not fire: {log:?}"
        );
        assert_eq!(
            values(&space).await,
            vec![(1, false)],
            "the send stores its datum"
        );
        assert_eq!(waiting(&space).await, 1, "the receive stays parked");
    }

    /// Added by decision 124 (plan v4 C7.8, rule CR-20, ruling OP-F13): a receive parks when the
    /// book holds the pair of the record that it binds and its consume, as when its guard
    /// refuses. A probe space stores 1 and runs the receive r, and its COMM gives the pair. A
    /// fresh space stores 1, books the pair and runs r: r parks, and the record stays.
    #[tokio::test]
    async fn scan_receive_parks_on_a_booked_bound_record() {
        let taking = receive_op("r", any(), None, false);
        let (probe, env) = fixture::<Classed>().await;
        store_all::<Classed>(&env, &[send(1, 0, false)]).await;
        run::<Classed>(&env, SpaceOp::Consume(taking.clone()))
            .await
            .expect("the receive fires");
        let pair = probe_pair(&probe.take_event_log().await);
        let (space, env) = fixture::<Classed>().await;
        store_all::<Classed>(&env, &[send(1, 0, false)]).await;
        env.volatile.book_pairs([pair]);
        run::<Classed>(&env, SpaceOp::Consume(taking))
            .await
            .expect("the receive parks");
        let log = space.take_event_log().await;
        assert!(
            log.iter().all(|event| matches!(event, Event::IoEvent(_))),
            "the booked pair does not fire: {log:?}"
        );
        assert_eq!(
            waiting(&space).await,
            1,
            "the receive parks its continuation"
        );
        assert_eq!(
            values(&space).await,
            vec![(1, false)],
            "the record stays stored"
        );
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.3): runs `op` with the
    /// discipline `D` in a play transaction whose commit deviates at row `index`.
    async fn run_deviating<D: Discipline>(
        env: &SpaceEnv,
        index: usize,
        op: SpaceOp,
    ) -> Result<SpaceOutcome, SpaceError> {
        let mut opened = begin_collection(&HashMapOuter, env, SPACE, &key(), &channel())
            .await
            .expect("phase A");
        opened.txn = Box::new(DeviatingCommit {
            inner: opened.txn,
            index,
            claims: None,
        });
        prepare_collection_op(&GenericSpace::<HashMapOuter, D>::default(), env, opened, op)
            .await?
            .commit()
            .await
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2 1.3): the class of a row
    /// acts where the finish reads it, at a deviating commit (C2 E4.5). The receipt of a class
    /// fire lists the bound member alone, so the fire row of a scan receive carries that another
    /// member of the class is persistent. The rows of the second send of Rule E carry that their
    /// re-issue is the send of the removed record, and not the receive.
    #[tokio::test]
    async fn scan_marks_its_class_fire_and_its_second_send() {
        let receive = ConsumeOp {
            persistent: true,
            ..receive_op("r", any(), None, false)
        };
        let reissue_of = |cause: ReissueCause| {
            Some(SpaceReissue {
                op: reissued_operation(OpView::Consume(&receive), None),
                cause,
            })
        };
        // The fire binds the first member in DR-75 order, which is not persistent. With a
        // persistent member after it the class stays, so the persistent receive meets a
        // divergent pair. Without one the receive goes on, and Tail(actual) stops it.
        for (members, cause) in [
            ([(5, false), (5, true)], ReissueCause::DivergentPair),
            ([(5, false), (5, false)], ReissueCause::OwnerRefusal),
        ] {
            let (_space, env) = fixture::<Classed>().await;
            store_all::<Classed>(&env, &ordered_sends(&members)).await;
            let outcome =
                run_deviating::<Classed>(&owner_funded(&env), 0, SpaceOp::Consume(receive.clone()))
                    .await
                    .expect("the deviating receive commits");
            assert_eq!(outcome.reissue, reissue_of(cause));
        }
        // The second send of a peek fires the waiter "w" and then stores its record. A store of
        // that send ends as the store of an element does, so its fire shows the mark: the waiter
        // peeks, so the send goes on, and the peek receive is spent. Without the mark the fire
        // would re-issue the receive.
        let stored = ordered_sends(&[(1, false), (2, false)]);
        let waiter = receive_op("w", any(), Some(equals(2)), true);
        let (_space, env) = parked_by_a_refused_pick::<Classed>(&stored, &waiter).await;
        let peek = receive_op("p", exactly(2), None, true);
        let outcome =
            run_deviating::<Classed>(&owner_funded(&env), 1, SpaceOp::Consume(peek.clone()))
                .await
                .expect("the deviating peek commits");
        assert_eq!(
            outcome.reissue,
            Some(SpaceReissue {
                op: SpaceOp::Produce(
                    restored_send(&peek, stored[1].data.clone())
                        .expect("the send of the removed record")
                ),
                cause: ReissueCause::OwnerRefusal,
            })
        );
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD2 1.1 and RADD2v2 7.1): the
    /// search of the fire of a persistent receive that does not peek ends Tail(actual) of a fire
    /// that the commit binds in place of c0 by the class rule of the bound class, at the fire
    /// count before the fire (CR-9, `FireRounds::at`). The search hands out its end once.
    #[test]
    fn scan_receive_search_ends_the_tail_of_a_fire_in_place_of_c0() {
        let op = ConsumeOp {
            persistent: true,
            ..receive_op("k", any(), None, false)
        };
        let matcher = Matcher;
        let group = [data()];
        let patterns = [any()];
        let continuation = TaggedContinuation::default();
        let source = unnamed_consume();
        let request = ParRowRequest {
            matcher: &matcher,
            group: &group,
            patterns: &patterns,
            continuation: &continuation,
            persistent: true,
            source: &source,
            own: None,
            item: None,
        };
        // The end that the search hands out after `fired` fires, when the commit binds a member
        // of a class with the class rule `class` in place of c0. `keeps` is false for a fire
        // that keeps no tail.
        let end_of = |fired: usize, keeps: bool, class: Option<bool>| {
            let tail = match keeps {
                false => None,
                true => Some(ScanTail {
                    fired,
                    operation: Arc::new(SpaceOp::Consume(op.clone())),
                }),
            };
            let found = class.map(|persistent| FoundClass {
                payload: ListParWithRandom::default(),
                matched: ListParWithRandom::default(),
                retires: Vec::new(),
                persistent,
            });
            let mut search = ScanReceiveSearch::<Classed> {
                channel: data(),
                records: Vec::new(),
                staged: None,
                source: unnamed_consume(),
                continuation: TaggedContinuation::default(),
                persistent: true,
                pattern: any(),
                theory: None,
                alternatives: true,
                classes: None,
                staged_retires: 0,
                found,
                tail,
                fired_class: None,
                discipline: std::marker::PhantomData,
            };
            let actual = search.actual_fire(&request, &[]);
            assert!(
                actual.retires.is_empty(),
                "a search without a retire plan retires nothing"
            );
            let end = search
                .tail_reissue()
                .map(|reissue| reissue.get::<TailEnd>().cloned().expect("an end of Tail"));
            assert_eq!(
                search.tail_reissue(),
                None,
                "the search hands out its end once"
            );
            end
        };
        let reissued = |cause: ReissueCause| {
            TailEnd::Return(Some(SpaceReissue {
                op: SpaceOp::Consume(ConsumeOp {
                    reissued_ord: None,
                    ..op.clone()
                }),
                cause,
            }))
        };
        // A member of the bound class is persistent: the class stays, a divergent pair.
        assert_eq!(
            end_of(0, true, Some(true)),
            Some(reissued(ReissueCause::DivergentPair))
        );
        // No member is persistent: the receive goes on, so the operation stops and runs again.
        assert_eq!(
            end_of(0, true, Some(false)),
            Some(TailEnd::Stop(reissued_operation(
                OpView::Consume(&op),
                None
            )))
        );
        // The fire after `FIRE_ROUND_CAP - 1` fires reaches the round cap.
        assert_eq!(
            end_of(FIRE_ROUND_CAP - 1, true, Some(false)),
            Some(reissued(ReissueCause::RoundCap))
        );
        // The commit fired c0 or found no tuple, so `actual_fire` did not run with a class.
        assert_eq!(end_of(0, true, None), None);
        // A fire that keeps no tail.
        assert_eq!(end_of(0, false, Some(true)), None);
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD2 1.1 and RADD2v2 7.2): the
    /// search of the offer of a send ends Tail(actual) of a fire that the commit binds in place
    /// of c0 with the Return of the re-issue of the offer loop, when the fire re-issues at the
    /// fire count before it (`FireRounds::at`). At every other step, and for the copy of a
    /// receive, it leaves the end to the finish. The search hands out its end once.
    #[test]
    fn scan_offer_search_ends_the_return_of_a_fire_in_place_of_c0() {
        let matcher = Matcher;
        let group = [data()];
        let patterns = [any()];
        let continuation = TaggedContinuation::default();
        let waiter = unnamed_consume();
        // The end that the search of the offer of `op` hands out after `fired` fires. `copy` is
        // the copy of the operation that the fire keeps, or `None` for a fire that keeps no
        // tail. `bound` tells whether the waiter that the commit binds in place of c0 peeks and
        // whether it is persistent, or is `None` when the commit fires c0.
        let end_of =
            |op: &ProduceOp, copy: Option<SpaceOp>, fired: usize, bound: Option<(bool, bool)>| {
                let record = record::wrap_datum(0, &op.data);
                let source = Produce::create(&data(), &record, op.persistent);
                let peeks = match bound {
                    None => false,
                    Some((peeks, _)) => peeks,
                };
                let mut waiters = Vec::<(Consume, bool)>::with_capacity(1);
                waiters.push((waiter.clone(), peeks));
                let mut search = ScanOfferSearch::<Classed> {
                    channel: data(),
                    waiters,
                    staged: Consume::create(&vec![data()], &vec![exactly(1)], &continuation, false),
                    element: Datum {
                        a: record,
                        persist: op.persistent,
                        source,
                    },
                    payload: op.data.clone(),
                    theory: false,
                    alternatives: true,
                    class: None,
                    staged_retires: 0,
                    tail: copy.map(|operation| ScanTail {
                        fired,
                        operation: Arc::new(operation),
                    }),
                    fired_waiter: None,
                    discipline: std::marker::PhantomData,
                };
                match bound {
                    None => {}
                    Some((_, persistent)) => {
                        let request = ParRowRequest {
                            matcher: &matcher,
                            group: &group,
                            patterns: &patterns,
                            continuation: &continuation,
                            persistent,
                            source: &waiter,
                            own: None,
                            item: None,
                        };
                        let actual = search.actual_fire(&request, &[]);
                        assert!(
                            actual.retires.is_empty(),
                            "an offer without a class retires nothing"
                        );
                    }
                }
                let end = search
                    .tail_reissue()
                    .map(|reissue| reissue.get::<TailEnd>().cloned().expect("an end of Tail"));
                assert_eq!(
                    search.tail_reissue(),
                    None,
                    "the search hands out its end once"
                );
                end
            };
        // A persistent send that a Stop re-issued carries the order 0 (`reissued_operation`).
        let persistent_send = ProduceOp {
            reissued_ord: Some(0),
            ..send(1, 1, true)
        };
        let send_once = send(1, 1, false);
        let copy_of = |op: &ProduceOp| Some(SpaceOp::Produce(op.clone()));
        let returned = |op: &ProduceOp, cause: ReissueCause| {
            Some(TailEnd::Return(Some(offer_reissue(op.clone(), cause))))
        };
        // A persistent send and a persistent waiter stay: a divergent pair. The Return carries
        // no order, as the offer loop re-issues the send.
        assert_eq!(
            end_of(
                &persistent_send,
                copy_of(&persistent_send),
                0,
                Some((false, true))
            ),
            Some(TailEnd::Return(Some(SpaceReissue {
                op: SpaceOp::Produce(ProduceOp {
                    reissued_ord: None,
                    ..persistent_send.clone()
                }),
                cause: ReissueCause::DivergentPair,
            })))
        );
        // A waiter that is not persistent: the send goes on, and the finish builds the Stop.
        assert_eq!(
            end_of(
                &persistent_send,
                copy_of(&persistent_send),
                0,
                Some((false, false))
            ),
            None
        );
        // The fire after `FIRE_ROUND_CAP - 1` fires reaches the round cap.
        assert_eq!(
            end_of(
                &persistent_send,
                copy_of(&persistent_send),
                FIRE_ROUND_CAP - 1,
                Some((false, false))
            ),
            returned(&persistent_send, ReissueCause::RoundCap)
        );
        // A waiter that does not peek spends a send that is not persistent.
        assert_eq!(
            end_of(&send_once, copy_of(&send_once), 0, Some((false, true))),
            None
        );
        // A waiter that peeks leaves the send in flight, so the round cap re-issues it by the
        // code of the offer loop.
        assert_eq!(
            end_of(
                &send_once,
                copy_of(&send_once),
                FIRE_ROUND_CAP - 1,
                Some((true, false))
            ),
            returned(&send_once, ReissueCause::RoundCap)
        );
        // The commit fired c0, so `actual_fire` did not run.
        assert_eq!(
            end_of(&persistent_send, copy_of(&persistent_send), 0, None),
            None
        );
        // A fire that keeps no tail.
        assert_eq!(end_of(&persistent_send, None, 0, Some((false, true))), None);
        // The second send of RESTORE-SCAN keeps the copy of its receive, so the finish builds
        // its end.
        assert_eq!(
            end_of(
                &send_once,
                Some(SpaceOp::Consume(restore_op(1))),
                FIRE_ROUND_CAP - 1,
                Some((true, false))
            ),
            None
        );
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD2v2 7.4): stages `op` with
    /// the discipline `D` over `env` in a play transaction that takes the row search of each
    /// stage call (`capturing`). It drops the staged work, so nothing commits, and returns the
    /// searches in staging order.
    async fn staged_searches<D: Discipline>(env: &SpaceEnv, op: SpaceOp) -> Vec<ParRowSearch> {
        let mut opened = begin_collection(&HashMapOuter, env, SPACE, &key(), &channel())
            .await
            .expect("phase A");
        let (kept, searches) = std::sync::mpsc::channel::<ParRowSearch>();
        opened.txn = capturing(opened.txn, kept);
        let prepared =
            prepare_collection_op(&GenericSpace::<HashMapOuter, D>::default(), env, opened, op)
                .await
                .expect("the operation stages");
        drop(prepared);
        searches.try_iter().collect()
    }

    /// Added by decision 124 (rules CR-4 and R13): the search of a recorded refusal binds the
    /// record that the refusal names, wherever that record stands in the pick order of the
    /// row. Each of the two records is named once, so one case names a record that the order
    /// meets second.
    #[tokio::test]
    async fn receive_search_binds_the_record_that_the_row_names() {
        let (space, env) = fixture::<Restorer>().await;
        store_all::<Restorer>(&env, &ordered_sends(&[(1, false), (2, false)])).await;
        let op = receive_op("k", any(), None, false);
        let mut searches =
            staged_searches::<Restorer>(&owner_funded(&env), SpaceOp::Consume(op.clone())).await;
        assert_eq!(
            searches.len(),
            1,
            "the fire of the receive carries its search"
        );
        let mut search = searches.pop().expect("the search of the fire");
        let records = space.get_data(&data()).await;
        assert_eq!(records.len(), 2, "the records of the collection");
        let group = vec![data()];
        let patterns = vec![op.pattern.clone()];
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

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, rulings RADD2 1.1, RADD2v2 7.2 and
    /// 7.4): the search of the fire of an owner-funded send keeps at the staging the fire count
    /// before the fire and the copy of the operation (O4). So when the commit binds a persistent
    /// waiter in place of c0, the search of a persistent send ends Tail(actual) with the Return
    /// of the `DivergentPair` re-issue of the offer loop, which carries no order. Without owner
    /// funding the fire carries no search.
    #[tokio::test]
    async fn owner_funded_scan_offer_keeps_its_tail_at_the_staging() {
        let (space, env) = fixture::<Restorer>().await;
        // The waiter "w" binds 5, so it fires as c0. The persistent waiter "p" binds no 5, so
        // it is never c0, and the search lists it.
        let persistent = ConsumeOp {
            persistent: true,
            ..receive_op("p", exactly(6), None, false)
        };
        for waiter in [receive_op("w", any(), None, false), persistent] {
            let parked = run::<Restorer>(&env, SpaceOp::Consume(waiter))
                .await
                .expect("the receive parks");
            assert!(parked.fires.is_empty());
        }
        let other = space
            .get_waiting_continuations(vec![data()])
            .await
            .into_iter()
            .find(|waiter| waiter.persist)
            .expect("the persistent waiter")
            .source;
        let op = send(5, 1, true);
        assert!(
            staged_searches::<Restorer>(&env, SpaceOp::Produce(op.clone()))
                .await
                .is_empty(),
            "a fire without owner funding carries no search"
        );
        let searches =
            staged_searches::<Restorer>(&owner_funded(&env), SpaceOp::Produce(op.clone())).await;
        let Ok([mut search]) = <[ParRowSearch; 1]>::try_from(searches) else {
            panic!("the fire of 5 is the one staged operation with a search");
        };
        let matcher = Matcher;
        let group = [data()];
        let patterns = [any()];
        let continuation = TaggedContinuation::default();
        let request = ParRowRequest {
            matcher: &matcher,
            group: &group,
            patterns: &patterns,
            continuation: &continuation,
            persistent: true,
            source: &other,
            own: None,
            item: None,
        };
        let actual = search.actual_fire(&request, &[]);
        assert!(
            actual.retires.is_empty(),
            "an offer without a class retires nothing"
        );
        let end = search
            .tail_reissue()
            .map(|reissue| reissue.get::<TailEnd>().cloned().expect("an end of Tail"));
        assert_eq!(
            end,
            Some(TailEnd::Return(Some(SpaceReissue {
                op: SpaceOp::Produce(ProduceOp {
                    reissued_ord: None,
                    ..op
                }),
                cause: ReissueCause::DivergentPair,
            })))
        );
        assert_eq!(
            search.tail_reissue(),
            None,
            "the search hands out its end once"
        );
    }

    // Added by decision 124 (plan v4 C7.9, C4v2 G8, ruling RADD2v4 7.6): the imports of the test
    // of a leasing theory. The test checkers exist only in a test build, so the test is a library
    // test.
    use crate::rust::interpreter::spaces::theory::test_theories::{self, TestBehavior};

    /// Added by decision 124 (plan v4 C7.9, C4v2 G8, ruling RADD2v4 7.6): the item test of the
    /// observer of the test of a leasing theory: true when the waiter `continuation` may not
    /// take the value `matched`.
    type ItemRule = fn(&TaggedContinuation, &ListParWithRandom) -> bool;

    /// Added by decision 124 (plan v4 C7.9, C4v2 G8, ruling RADD2v4 7.6): the item rule that
    /// refuses the value 2.
    fn refuses_two(_continuation: &TaggedContinuation, matched: &ListParWithRandom) -> bool {
        matched.pars == [int(2)]
    }

    /// Added by decision 124 (plan v4 C7.9, C4v2 G8, ruling RADD2v4 7.6): the item rule that
    /// refuses the waiter whose body is the string "refused".
    fn refuses_waiter(continuation: &TaggedContinuation, _matched: &ListParWithRandom) -> bool {
        let waiter = record::unwrap_continuation(continuation).expect("a waiter record");
        body_name(&waiter.continuation) == "refused"
    }

    /// Added by decision 124 (plan v4 C7.9, C4v2 G8, ruling RADD2v4 7.6): the commit of the
    /// owner-funded `op` with the discipline [`Restorer`] over a fresh space that first commits
    /// `setup`, under the observer of `rule`. The plan of `op` reads the space as one with the
    /// test theory `behavior`. The test sets it on the config that the plan reads, because
    /// [`fixture`] takes no config.
    async fn refused_commit(
        setup: &[SpaceOp],
        rule: ItemRule,
        behavior: Option<TestBehavior>,
        op: SpaceOp,
    ) -> (SpaceOutcome, ParRSpace) {
        let (space, env) = fixture::<Restorer>().await;
        for step in setup {
            run::<Restorer>(&env, step.clone())
                .await
                .expect("the setup commits");
        }
        space.set_accounting_observer(Some(Arc::new(Funded::with_item_test(
            move |continuation, matched| !rule(continuation, matched),
        ))));
        let env = owner_funded(&env);
        let mut opened = begin_collection(&HashMapOuter, &env, SPACE, &key(), &channel())
            .await
            .expect("phase A");
        opened.config.theory = behavior.map(test_theories::identity);
        let agent = GenericSpace::<HashMapOuter, Restorer>::default();
        let outcome = prepare_collection_op(&agent, &env, opened, op)
            .await
            .expect("the operation stages")
            .commit()
            .await
            .expect("the operation commits");
        (outcome, space)
    }

    /// Added by decision 124 (plan v4 C7.9, C4v2 G8, ruling RADD2v4 7.6): under a leasing theory
    /// the search of a SCAN row lists only c0, so a row whose c0 the commit refuses parks or
    /// stores, and it binds no other record or waiter. Without a theory the same commit binds an
    /// alternative. Only a commit builds the lists of a search, so the test runs the searches of
    /// the receive and of the offer in the commit of an owner-funded operation. Play picks c0 at
    /// random, so each case runs on `PICK_RUNS` fresh spaces. A run refuses c0 with probability
    /// 1/2, so no run does with probability 2^-64.
    #[tokio::test]
    async fn leasing_theory_search_gives_no_alternative() {
        let leasing = Some(TestBehavior::Leasing);
        // The receive: the owners refuse the record of 2, and both records bind its pattern.
        let records = [
            SpaceOp::Produce(send(1, 1, false)),
            SpaceOp::Produce(send(2, 1, false)),
        ];
        let receive = SpaceOp::Consume(receive_op("k", any(), None, false));
        let mut parked = 0;
        for _ in 0..PICK_RUNS {
            // Without a theory the search lists each record. The fire binds 1 in place of a
            // refused c0, so the receive never parks.
            let (outcome, space) =
                refused_commit(&records, refuses_two, None, receive.clone()).await;
            assert_eq!(outcome.fires.len(), 1, "an alternative fires: {outcome:?}");
            assert_eq!(values(&space).await, vec![(2, false)]);
            assert_eq!(waiting(&space).await, 0);
            // Under the leasing theory the search lists c0 alone. A refused c0 parks the receive,
            // and both records stay.
            let (outcome, space) =
                refused_commit(&records, refuses_two, leasing, receive.clone()).await;
            match outcome.fires.as_slice() {
                [] => {
                    parked += 1;
                    assert_eq!(values(&space).await, vec![(1, false), (2, false)]);
                    assert_eq!(waiting(&space).await, 1, "the receive parks");
                }
                [_] => {
                    assert_eq!(
                        values(&space).await,
                        vec![(2, false)],
                        "c0 is the record of 1"
                    );
                    assert_eq!(waiting(&space).await, 0);
                }
                fires => panic!("a receive fires once or parks: {fires:?}"),
            }
        }
        assert!(parked > 0, "a refused c0 parks a receive in some run");
        // The offer: the owners refuse the waiter "refused", and both waiters bind the send.
        let waiters = [
            SpaceOp::Consume(receive_op("refused", any(), None, false)),
            SpaceOp::Consume(receive_op("admitted", any(), None, false)),
        ];
        let offer = SpaceOp::Produce(send(5, 1, false));
        let mut stored = 0;
        for _ in 0..PICK_RUNS {
            // Without a theory the search lists each waiter. The fire binds "admitted" in place
            // of a refused c0, so the send is never stored.
            let (outcome, space) =
                refused_commit(&waiters, refuses_waiter, None, offer.clone()).await;
            let [fire] = outcome.fires.as_slice() else {
                panic!("the send fires a waiter: {outcome:?}");
            };
            assert_eq!(body_name(&fire.continuation), "admitted");
            assert!(values(&space).await.is_empty(), "the send is spent");
            assert_eq!(waiting(&space).await, 1, "the refused waiter stays");
            // Under the leasing theory the search lists c0 alone. A refused c0 stores the send,
            // and both waiters stay.
            let (outcome, space) =
                refused_commit(&waiters, refuses_waiter, leasing, offer.clone()).await;
            match outcome.fires.as_slice() {
                [] => {
                    stored += 1;
                    assert_eq!(values(&space).await, vec![(5, false)], "the send is stored");
                    assert_eq!(waiting(&space).await, 2, "no waiter fires in place of c0");
                }
                [fire] => {
                    assert_eq!(body_name(&fire.continuation), "admitted", "c0 fires");
                    assert!(values(&space).await.is_empty(), "the send is spent");
                    assert_eq!(waiting(&space).await, 1, "the refused waiter stays");
                }
                fires => panic!("an offer fires once or stores: {fires:?}"),
            }
        }
        assert!(stored > 0, "a refused c0 stores a send in some run");
    }
}
