//! The scan driver of the unordered disciplines (P5 design v3, A.7): the bag, and the set.
//!
//! A scan discipline has no head. Every record and every waiting continuation of a collection
//! can meet an incoming element. The driver visits the candidates in the canonical order of
//! DR-75 (decision 41): by the hash of the source produce or consume of each entry, then by the
//! digest of the entry inside a run of equal hashes, then by its index. So the choices of the
//! driver depend on the entry set of the collection, not on the list order of the store.
//!
//! The driver has four plans. Each plan stages only through [`Cx`]. So every staged operation
//! goes through the legacy chain of decision 45 (Option P) and registers an introduction.
//!
//! ```text
//! OFFER-SCAN(x)                     ▷ x: the datum of a produce, or a record that a peek removed
//!   loop
//!     W, m ← the first waiter in canonical order whose pattern binds x and whose guard admits m
//!            ▷ a guard refusal moves to the next waiter (decision 37)
//!     if there is no W: store x, stop
//!     stage Fire(x, W)                                             ▷ the fire never stores x
//!     if W does not peek and x has the class κ: RETIRE-CLASS(κ)              ▷ the set only
//!     after_fire(x spent, W stays):
//!       Done: stop
//!       Continue: offer x to the next waiter                                 ▷ decision 30
//!       Reissue(cause): re-issue x                                           ▷ A10, I-21
//!
//! RECEIVE-SCAN(K)
//!   loop
//!     r, m ← the first record in canonical order whose payload the pattern of K binds
//!     if there is no r, or the guard of K refuses m: park K, stop      ▷ one guard check (37)
//!     C ← [r] for a peek or for a record without a class, else r and the other stored
//!         members of its class in canonical order                                    ▷ F3
//!     after_fire(K spent, C stays)                                  ▷ the fire counts first
//!     stage Fire(K, C) over [D; |C|], and dispatch the result of r only
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
//! **Metering (decision 42).** Each plan reserves every read and copy before it makes it: the
//! canonical order (as the substrate reserves it), one payload decode for each tested record,
//! one pattern copy for each tested waiter, the metered matcher calls, and the copies of the
//! selected entries and of the staged operations. A fire that ends a plan moves its values.
//!
//! **The tested prefix.** Within one plan the incoming element does not change, so a candidate
//! that failed to match it fails again. A fire that continues the plan removes its partner, and
//! the canonical order of the other entries does not change. So the next round recomputes the
//! order and continues at the position of the removed partner. This gives the same choices as
//! a scan from the first candidate, with fewer matcher calls.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, LazyLock};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{
    BindPattern, CostAuthority, ListParWithRandom, Par, ParWithRandom, TaggedContinuation,
};
use models::rust::utils::{new_freevar_var, new_gbytearray_par};
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::internal::{ConsumeCandidate, Datum};
use rspace_plus_plus::rspace::space_store_handle::metered_hash;
use rspace_plus_plus::rspace::trace::event::Produce;

use crate::rust::interpreter::reified::costs::{FireStep, ReissueCause, ScanMeter, FIRE_ROUND_CAP};
use crate::rust::interpreter::spaces::agent::{ConsumeOp, ProduceOp, SpaceOp, SpaceReissue};
use crate::rust::interpreter::spaces::errors::SpaceError;
use crate::rust::interpreter::spaces::generic::{
    open_plan, peek_set, Cx, Discipline, OfferFire, OpView, ParCandidate, ParDatum, ParWaiter,
    Side, StoredMatch,
};
use crate::rust::interpreter::spaces::namespace::{CollectionKey, RandomPurpose};
use crate::rust::interpreter::spaces::record::{self, Introduction};

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
    /// compares two keys by their metered hashes, and it compares the full keys only when the
    /// hashes are equal (DR-90).
    ///
    /// The key depends on the payload only, never on the random state, the cost authority or
    /// the cost stack of the record (F5). The method reserves each read and each encode on
    /// `meter` before it makes them. A payload without a key fails with a typed error before any
    /// staging, never with empty bytes. The default gives `None` and does no work.
    fn class_key(
        &self,
        _payload: &[Par],
        _meter: &ScanMeter,
    ) -> Result<Option<Vec<u8>>, SpaceError> {
        Ok(None)
    }
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

/// The template of the retire pattern. A reserved copy of it covers its allocations.
pub(crate) static RETIRE_PATTERN: LazyLock<BindPattern> = LazyLock::new(retire_pattern);

/// The template of a retire continuation: its body without the bytes of the source hash, no
/// random state bytes and no cost authority. The reserved copy of the template, the reserved
/// buffer of the hash bytes, the reserved random bytes and the reserved copy of the authority
/// cover the allocations of a retire continuation.
pub(crate) static RETIRE_CONTINUATION: LazyLock<TaggedContinuation> =
    LazyLock::new(|| continuation_with(Vec::new(), Vec::new(), None));

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
    let meter = cx.meter();
    let record = record::wrap_datum_metered(ord, &op.data, meter)?;
    let source = Produce::create_metered(cx.data_channel(), &record, op.persistent, meter)?;
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
        OfferEnd::Reissue(cause) => Ok(Some(cx.reissue(
            SpaceOp::Produce(ProduceOp {
                reissued_ord: None,
                ..op
            }),
            cause,
        ))),
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
    let meter = cx.meter();
    let continuation = record::wrap_continuation_metered(
        ord,
        &op.body,
        &op.guard,
        record::stamped(&op.authority),
        meter,
    )?;
    let peeks = peek_set(op.peek, meter)?;
    // Disabled by I-21 (one fire count for each transaction, `Cx::rounds`):
    // let mut rounds = FireRounds::new();
    let mut tested = 0;
    loop {
        let Some(found) = first_bound_record(cx, &op.pattern, tested)? else {
            cx.check_capacity::<D>(Side::Waiters)?;
            cx.park(&op, continuation, peeks)?;
            return discipline.settle_after(cx, OpView::Consume(&op));
        };
        // The theory of the space checks the stored record after the guard (P7, TH-6). A set
        // class is one value (F5), so the check of its first member decides the class.
        let stored = StoredMatch {
            payload: &found.payload,
            captures: &found.matched,
        };
        if !cx.admits(&continuation, &found.matched, &[stored])? {
            cx.check_capacity::<D>(Side::Waiters)?;
            cx.park(&op, continuation, peeks)?;
            return discipline.settle_after(cx, OpView::Consume(&op));
        }
        let position = found.position;
        let class = class_of(discipline, cx, &op, found)?;
        let spent = !op.persistent;
        let stays = op.peek || class.iter().any(|member| member.record.persist);
        // Rule E (decision 45): the peek fire removes a non-persistent record, as the host COMM
        // does, and RESTORE-SCAN sends the record again right after the fire.
        let restored = match class.as_slice() {
            [representative] if op.peek && !representative.record.persist => {
                Some(meter.copy(&representative.record.a)?)
            }
            _ => None,
        };
        record::prepay_continuation_unwrap(&continuation, meter)?;
        let step = cx.rounds().after_fire(spent, stays);
        match step {
            FireStep::Continue => {
                let fired = meter.copy(&continuation)?;
                fire_class(cx, &op, fired, meter.copy(&peeks)?, class)?;
                if let Some(reissue) = restore_in(discipline, cx, restored, &op)? {
                    return Ok(Some(reissue));
                }
                tested = position;
            }
            FireStep::Done => {
                fire_class(cx, &op, continuation, peeks, class)?;
                if let Some(reissue) = restore_in(discipline, cx, restored, &op)? {
                    return Ok(Some(reissue));
                }
                return discipline.settle_after(cx, OpView::Consume(&op));
            }
            FireStep::Reissue(cause) => {
                fire_class(cx, &op, continuation, peeks, class)?;
                if let Some(reissue) = restore_in(discipline, cx, restored, &op)? {
                    return Ok(Some(reissue));
                }
                return Ok(Some(cx.reissue(
                    SpaceOp::Consume(ConsumeOp {
                        reissued_ord: None,
                        ..op
                    }),
                    cause,
                )));
            }
        }
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

/// A waiter that an offer fires: its position in the canonical order, its copy, and the values
/// that it binds.
struct FoundWaiter {
    position: usize,
    waiter: ParWaiter,
    matched: ListParWithRandom,
}

/// A record that the pattern of a receive binds: its position in the canonical order, its index
/// in the staged view, its payload, and the values that it binds.
struct FoundRecord {
    position: usize,
    index: usize,
    payload: ListParWithRandom,
    matched: ListParWithRandom,
}

/// A member of the class that a receive fire binds: the copy of the record, the values that it
/// binds, and its index in the staged view. The VectorDB discipline fires the members of a list
/// with it (P6).
pub(crate) struct Member {
    pub(crate) record: ParDatum,
    pub(crate) matched: ListParWithRandom,
    pub(crate) view_index: i32,
}

/// The class of the datum of an offer, and what its retire continuations need: the key, the
/// hash of the key, the random state of the retires and the cost authority of the element.
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
    let meter = cx.meter();
    let data = cx.data_channel();
    let mut class: Option<Option<Retiring>> = None;
    let mut tested = 0;
    loop {
        let Some(found) = first_admitting_waiter(cx, payload, tested)? else {
            // A metered job stores only a record whose class key exists under its metering, so
            // a later class operation never meets a record without a class. Without a budget the
            // host sorter keys every payload, and a fire above already computed the key.
            if class.is_none() && meter.is_metered() {
                discipline.class_key(&payload.pars, meter)?;
            }
            cx.check_capacity::<D>(Side::Data)?;
            cx.store_on(data, element.record, element.persist, element.introduction)?;
            return Ok(OfferEnd::Settled);
        };
        let FoundWaiter {
            position,
            waiter,
            matched,
        } = found;
        let peeks = !waiter.peeks.is_empty();
        let spent = !element.persist && !peeks;
        let stays = waiter.persist;
        // A peek waiter takes no datum, so its fire retires nothing (decision 37).
        let retiring = match peeks {
            true => None,
            false => {
                let cached = match class.take() {
                    Some(cached) => cached,
                    None => retiring_of(discipline, payload, &element.record, meter)?,
                };
                class.insert(cached).as_ref()
            }
        };
        record::prepay_continuation_unwrap(&waiter.continuation, meter)?;
        let fire = OfferFire {
            persistent: element.persist,
            waiter,
            matched,
        };
        let step = cx.rounds().after_fire(spent, stays);
        match step {
            FireStep::Continue => {
                let staged = meter.copy(&element.record)?;
                let removed = meter.copy(&element.record)?;
                let source = meter.copy(&element.source)?;
                cx.fire_offer_on(data, fire, staged, removed, source, element.introduction)?;
                if let Some(retiring) = retiring {
                    retire_class(discipline, cx, retiring)?;
                }
                tested = position;
            }
            FireStep::Done => {
                let removed = meter.copy(&element.record)?;
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
                let removed = meter.copy(&element.record)?;
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
    meter: &ScanMeter,
) -> Result<Option<Retiring>, SpaceError> {
    let Some(key) = discipline.class_key(&payload.pars, meter)? else {
        return Ok(None);
    };
    let key_hash = metered_hash(&key, meter)?;
    let random = RandomPurpose::Retire.derive(&Blake2b512Random::from_bytes(&record.random_state));
    let authority = record
        .cost_authority
        .as_ref()
        .map(|authority| meter.copy(authority))
        .transpose()?;
    Ok(Some(Retiring {
        key,
        key_hash,
        random,
        authority,
    }))
}

/// The first waiter of the collection, in canonical order from the position `tested`, whose
/// pattern binds `payload` and whose guard admits the bound values. A waiter whose guard
/// refuses is skipped (decision 37). Each tested waiter costs one reserved copy of its pattern
/// and one metered match, and only a waiter that binds is copied whole.
fn first_admitting_waiter(
    cx: &mut Cx<'_, '_>,
    payload: &ListParWithRandom,
    tested: usize,
) -> Result<Option<FoundWaiter>, SpaceError> {
    let meter = cx.meter();
    let data = cx.data_channel();
    let order = cx.canonical_waiter_order(data)?;
    for (position, &index) in order.iter().enumerate().skip(tested) {
        let pattern = {
            let waiter = waiter_at(cx, index)?;
            let [pattern] = waiter.patterns.as_slice() else {
                return Err(SpaceError::Inconsistent(
                    "a waiting continuation of a collection has several binds",
                ));
            };
            meter.copy(pattern)?
        };
        let Some(matched) = cx.bind(&pattern, payload)? else {
            continue;
        };
        let waiter = meter.copy(waiter_at(cx, index)?)?;
        // The element is in flight, so the theory checks no stored data here (P7, TH-6).
        if cx.admits(&waiter.continuation, &matched, &[])? {
            return Ok(Some(FoundWaiter {
                position,
                waiter,
                matched,
            }));
        }
    }
    Ok(None)
}

/// The first record of the collection, in canonical order from the position `tested`, whose
/// payload `pattern` binds. Each tested record costs one reserved payload decode and one
/// metered match. Nothing is copied whole.
fn first_bound_record(
    cx: &mut Cx<'_, '_>,
    pattern: &BindPattern,
    tested: usize,
) -> Result<Option<FoundRecord>, SpaceError> {
    let meter = cx.meter();
    let data = cx.data_channel();
    let order = cx.canonical_data_order(data)?;
    for (position, &index) in order.iter().enumerate().skip(tested) {
        let payload = record::unwrap_datum_metered(&record_at(cx, index)?.a, meter)?.datum;
        if let Some(matched) = cx.bind(pattern, &payload)? {
            return Ok(Some(FoundRecord {
                position,
                index,
                payload,
                matched,
            }));
        }
    }
    Ok(None)
}

/// The class that the receive `op` binds with the record `found`: `[found]` for a peek or for
/// a record without a class, and otherwise `found` and the other stored members of its class,
/// in canonical order (F3, decision 41). The record of `found` is the first member in canonical
/// order, because every member binds the pattern of `op` as `found` does. So the other members
/// follow it. Each member binds the pattern of `op` with its own values.
fn class_of<D: ScanDiscipline>(
    discipline: &D,
    cx: &mut Cx<'_, '_>,
    op: &ConsumeOp,
    found: FoundRecord,
) -> Result<Vec<Member>, SpaceError> {
    let meter = cx.meter();
    let FoundRecord {
        position,
        index,
        payload,
        matched,
    } = found;
    let representative = Member {
        record: meter.copy(record_at(cx, index)?)?,
        matched,
        view_index: view_index(index)?,
    };
    let key = match op.peek {
        true => None,
        false => discipline.class_key(&payload.pars, meter)?,
    };
    let Some(key) = key else {
        let mut class = meter.buffer::<Member>(1)?;
        class.push(representative);
        return Ok(class);
    };
    let key_hash = metered_hash(&key, meter)?;
    let data = cx.data_channel();
    let order = cx.canonical_data_order(data)?;
    let mut class = meter.buffer::<Member>(1)?;
    class.push(representative);
    let next = position.checked_add(1).ok_or(SpaceError::Inconsistent(
        "a candidate position overflows usize",
    ))?;
    for &other in order.iter().skip(next) {
        let payload = record::unwrap_datum_metered(&record_at(cx, other)?.a, meter)?.datum;
        let Some(other_key) = discipline.class_key(&payload.pars, meter)? else {
            continue;
        };
        if !same_class(&key, &key_hash, &other_key, meter)? {
            continue;
        }
        let matched = cx
            .bind(&op.pattern, &payload)?
            .ok_or(SpaceError::Inconsistent(
                "a member of a class does not bind the pattern of its representative",
            ))?;
        let member = Member {
            record: meter.copy(record_at(cx, other)?)?,
            matched,
            view_index: view_index(other)?,
        };
        push_reserved(&mut class, member, meter)?;
    }
    Ok(class)
}

/// Stages the consume fire of the receive `op` over the class `class` with the continuation
/// `continuation` and the peek set `peeks`: the group `[D; |class|]`, whose outcome dispatches
/// the result of the first member only (F3, F20). For one member it is the fire of one record.
fn fire_class(
    cx: &mut Cx<'_, '_>,
    op: &ConsumeOp,
    continuation: TaggedContinuation,
    peeks: BTreeSet<i32>,
    class: Vec<Member>,
) -> Result<(), SpaceError> {
    let meter = cx.meter();
    let data = cx.data_channel();
    let width = class.len();
    let mut channels = meter.buffer::<Par>(width)?;
    let mut patterns = meter.buffer::<BindPattern>(width)?;
    let mut candidates = meter.buffer::<ParCandidate>(width)?;
    for member in class {
        channels.push(meter.copy(data)?);
        patterns.push(meter.copy(&op.pattern)?);
        candidates.push(stored_candidate(meter.copy(data)?, member));
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
    let meter = cx.meter();
    let payload = record::unwrap_datum_metered(&record, meter)?.datum;
    let source = Produce::create_metered(cx.data_channel(), &record, false, meter)?;
    let introduction = cx.introduction().for_record(&record, meter)?;
    let element = InFlight {
        record,
        source,
        persist: false,
        introduction: &introduction,
    };
    let end = match cx.rounds().fired() < FIRE_ROUND_CAP {
        true => offer_scan(discipline, cx, element, &payload)?,
        false => OfferEnd::Reissue(ReissueCause::RoundCap),
    };
    match end {
        OfferEnd::Settled => Ok(None),
        OfferEnd::Reissue(cause) => match origin {
            Some(op) => {
                let send = restored_send(op, payload, meter)?;
                Ok(Some(cx.reissue(SpaceOp::Produce(send), cause)))
            }
            None => Err(SpaceError::Inconsistent(
                "a restore outside a receive cannot re-issue its record",
            )),
        },
    }
}

/// The produce that re-issues a restored record whose payload is `payload`: the collection and
/// the channel of the receive `op`, not persistent, no params, the authority of the record, and
/// no carried order. Its record is the restored record, because the `ord` is 0. The copies are
/// reserved before they are made.
fn restored_send(
    op: &ConsumeOp,
    payload: ListParWithRandom,
    meter: &ScanMeter,
) -> Result<ProduceOp, SpaceError> {
    let key = CollectionKey::from_bytes(meter.copy_slice(op.key.as_bytes())?)
        .ok_or(SpaceError::Inconsistent("a collection key is empty"))?;
    let authority = match &payload.cost_authority {
        Some(authority) => meter.copy(authority)?,
        None => CostAuthority::default(),
    };
    Ok(ProduceOp {
        space: op.space,
        key,
        channel: meter.copy(&op.channel)?,
        data: payload,
        persistent: false,
        params: BTreeMap::new(),
        authority,
        reissued_ord: None,
    })
}

/// RETIRE-CLASS: stages the retire of each stored member of the class of `retiring` that is not
/// persistent, in canonical order (decision 37, decision 41), on the collection of the operation
/// ([`retire_class_on`]).
fn retire_class<D: ScanDiscipline>(
    discipline: &D,
    cx: &mut Cx<'_, '_>,
    retiring: &Retiring,
) -> Result<(), SpaceError> {
    let data = cx.data_channel();
    retire_class_on(
        cx,
        data,
        &retiring.key,
        &retiring.key_hash,
        &retiring.random,
        retiring.authority.as_ref(),
        |payload, meter| discipline.class_key(payload, meter),
    )
}

/// RETIRE-CLASS on the collection whose data channel is `channel`: stages the retire of each
/// stored member of the class `key`, whose metered hash is `key_hash`, that is not persistent,
/// in canonical order. `class_key` gives the class of a stored payload. Each retire is the
/// consume of `K_retire(m)` over `[channel]` with the record `m`, with the random state `random`
/// and the cost authority `authority`. It is never dispatched, and it is not a fire round. A
/// persistent member stays, because a COMM never removes a persistent record. A retire locates
/// its member again before it stages, because each retire removes a record. The join engine
/// retires the classes of the binds of a join fire with it (P5c).
pub(crate) fn retire_class_on(
    cx: &mut Cx<'_, '_>,
    channel: &Par,
    key: &[u8],
    key_hash: &Blake2b256Hash,
    random: &Blake2b512Random,
    authority: Option<&CostAuthority>,
    class_key: impl Fn(&[Par], &ScanMeter) -> Result<Option<Vec<u8>>, SpaceError>,
) -> Result<(), SpaceError> {
    let meter = cx.meter();
    let order = cx.canonical_data_order(channel)?;
    let mut members: Vec<(ParDatum, ListParWithRandom)> = Vec::new();
    for &index in &order {
        // A COMM never removes a persistent record, so a persistent member is not decoded.
        let payload = {
            let stored = record_on(cx, channel, index)?;
            match stored.persist {
                true => None,
                false => Some(record::unwrap_datum_metered(&stored.a, meter)?.datum),
            }
        };
        let Some(payload) = payload else {
            continue;
        };
        let Some(member_key) = class_key(&payload.pars, meter)? else {
            continue;
        };
        if !same_class(key, key_hash, &member_key, meter)? {
            continue;
        }
        let member = meter.copy(record_on(cx, channel, index)?)?;
        push_reserved(&mut members, (member, payload), meter)?;
    }
    for (member, payload) in members {
        let index = located(cx, channel, &member.source.hash)?;
        let matched = cx
            .bind(&RETIRE_PATTERN, &payload)?
            .ok_or(SpaceError::Inconsistent(
                "a retired member does not bind the retire pattern",
            ))?;
        meter.reserve_copy(&*RETIRE_CONTINUATION)?;
        meter.reserve_buffer::<u8>(member.source.hash.0.len())?;
        meter.random_bytes(random)?;
        let authority = authority
            .map(|authority| meter.copy(authority))
            .transpose()?;
        let continuation = retire_continuation(&member.source.hash, random, authority);
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
    }
    Ok(())
}

/// True when the key `other` equals `key`, whose metered hash is `key_hash` (DR-90): the hashes
/// are compared first, and the full keys only when the hashes are equal. Each hash and each
/// comparison is reserved before it is made.
pub(crate) fn same_class(
    key: &[u8],
    key_hash: &Blake2b256Hash,
    other: &[u8],
    meter: &ScanMeter,
) -> Result<bool, SpaceError> {
    let other_hash = metered_hash(other, meter)?;
    meter.compare(key_hash.0.len(), other_hash.0.len())?;
    match *key_hash == other_hash {
        false => Ok(false),
        true => {
            meter.compare(key.len(), other.len())?;
            Ok(key == other)
        }
    }
}

/// The index in the staged view of `channel` of the first record whose source hash is `hash`.
/// Each comparison of two hashes is reserved first. Two records with one source hash are
/// byte-identical, so either one is the member.
pub(crate) fn located(
    cx: &mut Cx<'_, '_>,
    channel: &Par,
    hash: &Blake2b256Hash,
) -> Result<usize, SpaceError> {
    let meter = cx.meter();
    let records = cx.data_on(channel)?;
    for (index, stored) in records.iter().enumerate() {
        meter.compare(hash.0.len(), stored.source.hash.0.len())?;
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

/// Appends `value` to `values`. A list at its capacity reserves its growth on `meter` before it
/// grows, as `Vec` doubles its buffer (decision 42), with the rule of the staged list of the
/// engine.
fn push_reserved<T>(values: &mut Vec<T>, value: T, meter: &ScanMeter) -> Result<(), SpaceError> {
    if values.len() == values.capacity() {
        let grown = values
            .capacity()
            .checked_mul(2)
            .ok_or(RSpaceError::HostWorkRejected)?
            .max(1);
        meter.reserve_buffer::<T>(grown)?;
        values
            .try_reserve_exact(grown - values.len())
            .map_err(|_| RSpaceError::HostWorkRejected)?;
    }
    values.push(value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::var::VarInstance;
    use models::rhoapi::{EEq, EVar, Expr, Var};
    use models::rust::host_work::{HostWorkDimension, HostWorkLimit, HostWorkLimits};
    use models::rust::space_factory_urn::{SpaceFactoryUrn, SpaceInner, SpaceOuter};
    use models::rust::utils::{new_freevar_par, new_gint_par, new_gstring_par};
    use rspace_plus_plus::rspace::rspace::RSpace;
    use rspace_plus_plus::rspace::rspace_interface::ISpace;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    use rspace_plus_plus::rspace::space_store_handle::SpaceStoreHandle;
    use rspace_plus_plus::rspace::trace::event::{Consume, Event, IOEvent};
    use serde::Serialize;

    use super::*;
    use crate::rust::interpreter::accounting::costs::Cost;
    use crate::rust::interpreter::accounting::RuntimeBudget;
    use crate::rust::interpreter::host_work::HostWorkBudget;
    use crate::rust::interpreter::matcher::r#match::Matcher;
    use crate::rust::interpreter::reified::costs::FIRE_ROUND_CAP;
    use crate::rust::interpreter::spaces::agent::{
        run_op, SpaceAgent, SpaceCreation, SpaceEnv, SpaceOutcome,
    };
    use crate::rust::interpreter::spaces::disciplines::queue::QueueDiscipline;
    use crate::rust::interpreter::spaces::generic::{GenericSpace, Ordered, OuterStore};
    use crate::rust::interpreter::spaces::namespace::{self, Durability, NativeSpace, SID_LEN};
    use crate::rust::interpreter::spaces::outer::hashmap::HashMapOuter;
    use crate::rust::interpreter::spaces::types::config::SpaceConfig;
    use crate::rust::interpreter::spaces::types::qualifier::SpaceQualifier;

    /// The head of a test discipline: the least `(ord, bytes)`, which the scan driver never uses.
    fn least_head<T: Serialize>(
        side: Side,
        entries: &[Ordered<'_, T>],
        meter: &ScanMeter,
    ) -> Result<Option<usize>, SpaceError> {
        QueueDiscipline.head_metered(side, entries, meter)
    }

    /// A scan discipline whose class key is the metered bincode of the payload, so two records
    /// with equal payloads are one class. It exercises RETIRE-CLASS and the `[D; n]` fire of the
    /// driver, which the set discipline reuses (P5a, F3). The SET batch owns the real set key.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct Classed;

    impl Discipline for Classed {
        const NAME: &'static str = "classed";
        const USES_COUNTER: bool = false;
        const SEND_PARAMS: &'static [&'static str] = &[];

        fn head_metered<T: Serialize>(
            &self,
            side: Side,
            entries: &[Ordered<'_, T>],
            meter: &ScanMeter,
        ) -> Result<Option<usize>, SpaceError> {
            least_head(side, entries, meter)
        }

        fn reaches_head(&self, _side: Side, _queued: usize) -> bool { true }

        fn precedes(
            &self,
            side: Side,
            ord: i64,
            head: i64,
            meter: &ScanMeter,
        ) -> Result<bool, SpaceError> {
            QueueDiscipline.precedes(side, ord, head, meter)
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
        fn class_key(
            &self,
            payload: &[Par],
            meter: &ScanMeter,
        ) -> Result<Option<Vec<u8>>, SpaceError> {
            Ok(Some(meter.encode(payload)?))
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

        fn head_metered<T: Serialize>(
            &self,
            side: Side,
            entries: &[Ordered<'_, T>],
            meter: &ScanMeter,
        ) -> Result<Option<usize>, SpaceError> {
            least_head(side, entries, meter)
        }

        fn reaches_head(&self, _side: Side, _queued: usize) -> bool { true }

        fn precedes(
            &self,
            side: Side,
            ord: i64,
            head: i64,
            meter: &ScanMeter,
        ) -> Result<bool, SpaceError> {
            QueueDiscipline.precedes(side, ord, head, meter)
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
            meter: ScanMeter::new(None),
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
    fn scan_same_class_compares_full_keys_only_on_equal_hashes() {
        let key = vec![1, 2, 3];
        let meter = ScanMeter::new(None);
        let key_hash = metered_hash(&key, &meter).expect("an unmetered hash");
        assert_eq!(same_class(&key, &key_hash, &[1, 2, 3], &meter), Ok(true));
        assert_eq!(same_class(&key, &key_hash, &[1, 2, 4], &meter), Ok(false));
        let operations = |other: &[u8]| {
            let host_work =
                HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX)));
            let metered = ScanMeter::new(Some(host_work.clone()));
            same_class(&key, &key_hash, other, &metered).expect("the comparison fits");
            host_work
                .usage(HostWorkDimension::VerificationOperations)
                .get()
        };
        assert_eq!(
            operations(&[1, 2, 3]),
            operations(&[9, 9, 9]) + 1,
            "equal hashes add exactly the one comparison of the full keys"
        );
    }

    #[tokio::test]
    async fn scan_class_fire_binds_every_member_and_dispatches_the_first() {
        let (space, env) = fixture::<Classed>().await;
        let members = ordered_sends(&[(1, false), (1, false)]);
        store_all::<Classed>(&env, &members).await;
        store_all::<Classed>(&env, &[send(2, 99, false)]).await;
        space.take_event_log().await;
        let outcome = run::<Classed>(
            &env,
            SpaceOp::Consume(receive_op("k", exactly(1), None, false)),
        )
        .await
        .expect("the receive commits");
        let [fire] = outcome.fires.as_slice() else {
            panic!("one fire: {outcome:?}");
        };
        let [result] = fire.results.as_slice() else {
            panic!("the fire dispatches the result of the first member only: {fire:?}");
        };
        assert_eq!(
            result.removed_datum,
            record::wrap_datum(0, &members[0].data),
            "the first member in canonical order is the representative"
        );
        assert_eq!(labels(&space.take_event_log().await), vec![
            "consume 2",
            "comm 2"
        ]);
        assert_eq!(values(&space).await, vec![(2, false)]);
    }

    #[tokio::test]
    async fn scan_class_peek_binds_and_restores_the_representative_only() {
        let (space, env) = fixture::<Classed>().await;
        let members = ordered_sends(&[(1, false), (1, false)]);
        store_all::<Classed>(&env, &members).await;
        space.take_event_log().await;
        let outcome = run::<Classed>(
            &env,
            SpaceOp::Consume(receive_op("p", exactly(1), None, true)),
        )
        .await
        .expect("the peek commits");
        assert_eq!(outcome.fires.len(), 1);
        let log = space.take_event_log().await;
        assert_eq!(labels(&log), vec!["consume 1", "comm 1 peek", "produce"]);
        let restored = log
            .iter()
            .find_map(|event| match event {
                Event::IoEvent(IOEvent::Produce(produce)) => Some(produce.hash.clone()),
                _ => None,
            })
            .expect("the restore sends the record again");
        assert_eq!(restored, record_hash(&members[0]));
        assert_eq!(values(&space).await, vec![(1, false), (1, false)]);
    }

    #[tokio::test]
    async fn scan_produce_fire_retires_the_stored_members_of_its_class() {
        let (space, env) = fixture::<Classed>().await;
        // The stored 1 precedes the stored 2, so the guarded waiter meets 1 first and parks
        // (decision 37: the guard is checked once).
        let stored = ordered_sends(&[(1, false), (2, false)]);
        store_all::<Classed>(&env, &stored).await;
        let parked = run::<Classed>(
            &env,
            SpaceOp::Consume(receive_op("w", any(), Some(equals(2)), false)),
        )
        .await
        .expect("the receive commits");
        assert!(parked.fires.is_empty());
        assert_eq!(waiting(&space).await, 1);
        space.take_event_log().await;
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
        let (space, env) = fixture::<Classed>().await;
        let stored = ordered_sends(&[(1, false), (2, false), (2, true)]);
        store_all::<Classed>(&env, &stored).await;
        run::<Classed>(
            &env,
            SpaceOp::Consume(receive_op("w", any(), Some(equals(2)), false)),
        )
        .await
        .expect("the waiter parks");
        space.take_event_log().await;
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
        let (space, env) = fixture::<Classed>().await;
        let stored = ordered_sends(&[(1, false), (2, false)]);
        store_all::<Classed>(&env, &stored).await;
        run::<Classed>(
            &env,
            SpaceOp::Consume(receive_op("p", any(), Some(equals(2)), true)),
        )
        .await
        .expect("the peek waiter parks");
        space.take_event_log().await;
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
}
