//! The host side of the host differentials of the P5 disciplines (decision 90, decision 69). A
//! host program makes the host calls that a space operation is equivalent to, on an RSpace whose
//! accounting observer is a [`Recorder`] (`support/observed.rs`). A test compares the observer
//! calls of the agent with the observer calls of its host program: the start of each operation,
//! its introduction, each COMM with its participants, and the finish, with their arguments. The
//! module was the host side of the legacy-charges differential of decision 45 (Option P), which
//! compared legacy charges. The legacy meter is deleted (DR-134), and Option P with it.
//!
//! [`host_produce`] and [`host_consume`] return the observer calls that a host call made, the
//! result of the call, and the staged operation that the call is equivalent to, built from its
//! inputs and its result. They also return the re-produces that the host reducer makes after a
//! peek fire (`produce_peeks`, decision 45, Rule E).
//!
//! [`lone_host_produce`] and [`lone_host_consume`] run one staged operation of an agent alone on a
//! fresh host. The host matches with a [`PinnedPayloadMatcher`], so it binds exactly the records
//! that the staged operation binds, by their payloads, as the agent matches them. The observer
//! calls of a lone host equal those of the agent only when the call does not depend on the
//! history of the store of the agent. A start names the join groups of the channel of a produce,
//! and a COMM names in `times_repeated` how often the store produced each bound record since its
//! last event log. A lone host holds only the records and the waiter of the operation.
//!
//! Host play picks at random among the candidates of a call since DR-129. A suite that runs a
//! whole host program on one host pins each call to the canonical choice of decision 41
//! ([`WaiterPin`], [`waiter_choice`], [`drawn_consume`]), so the program makes the choices of
//! the agent (decision 89).

use std::collections::BTreeSet;
use std::future::Future;
use std::sync::{Arc, Mutex};

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{BindPattern, Expr, ListParWithRandom, Par, TaggedContinuation};
// Changed by decision 90 (decision 69): the legacy meter, its `ChargingRSpace` and the module
// `reified::legacy_charges` of Option P are deleted (DR-134). A host records its observer calls.
// use rholang::rust::interpreter::accounting::costs::Cost;
// use rholang::rust::interpreter::accounting::RuntimeBudget;
use rholang::rust::interpreter::matcher::r#match::Matcher;
// use rholang::rust::interpreter::reified::legacy_charges::{
//     LegacyCharges, ParSpaceConsume, ParSpaceProduce,
// };
// use rholang::rust::interpreter::rho_runtime::RhoISpace;
use rholang::rust::interpreter::spaces::record;
// use rholang::rust::interpreter::storage::charging_rspace::ChargingRSpace;
use rspace_plus_plus::rspace::internal::{ConsumeCandidate, Datum, WaitingContinuation};
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::{ContResult, ISpace, RSpaceResult};
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::space_store_handle::{
    canonical_continuation_order, ConsumeOutcome, ProduceOutcome,
};
use rspace_plus_plus::rspace::trace::event::{Consume, Produce};

use super::agent::{ParSpaceConsume, ParSpaceProduce};
use super::observed::{observe, Call, Recorder};

type ParRSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

/// The continuation result of a host fire.
pub type ParCont = ContResult<Par, BindPattern, TaggedContinuation>;

/// One bound record of a host fire.
pub type ParResult = RSpaceResult<Par, ListParWithRandom>;

// Changed by decision 90 (decision 69): the legacy meter and `ChargingRSpace` are deleted
// (DR-134), so a host is an in-memory RSpace whose accounting observer records its calls
// (`observed_host`).
//
// /// A `ChargingRSpace` over a fresh in-memory RSpace that charges `budget`.
// pub async fn charging_host(
//     budget: &RuntimeBudget,
// ) -> impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation> {
//     let mut stores = InMemoryStoreManager::new();
//     let space = ParRSpace::create(
//         stores.r_space_stores().await.expect("in-memory stores"),
//         Arc::new(Box::new(Matcher)),
//     )
//     .expect("an in-memory space");
//     let host: RhoISpace = Arc::new(Box::new(space));
//     ChargingRSpace::charging_rspace(host, budget.clone())
// }

/// Added by decision 90 (decision 69): a fresh in-memory RSpace with the Rholang matcher, whose
/// accounting observer is `recorder`.
pub async fn observed_host(recorder: &Arc<Recorder>) -> ParRSpace {
    let mut stores = InMemoryStoreManager::new();
    let space = ParRSpace::create(
        stores.r_space_stores().await.expect("in-memory stores"),
        Arc::new(Box::new(Matcher)),
    )
    .expect("an in-memory space");
    observe(&space, recorder);
    space
}

/// The candidates of a fire from the host results, in channel order. A host result does not name
/// the position of its record, so each datum index is 0.
pub fn candidates(results: &[ParResult]) -> Vec<ConsumeCandidate<Par, ListParWithRandom>> {
    results
        .iter()
        .map(|result| ConsumeCandidate {
            channel: result.channel.clone(),
            datum: Datum {
                a: result.matched_datum.clone(),
                persist: result.persistent,
                source: Produce::create(&result.channel, &result.removed_datum, result.persistent),
            },
            removed_datum: result.removed_datum.clone(),
            datum_index: 0,
        })
        .collect()
}

/// The peek set of a consume of `binds` binds: every bind for a peek, and none otherwise.
pub fn peeks_of(peek: bool, binds: usize) -> BTreeSet<i32> {
    match peek {
        true => (0..i32::try_from(binds).expect("few binds")).collect(),
        false => BTreeSet::new(),
    }
}

/// The waiting continuation that a host fire bound, from its `ContResult`.
pub fn waiter(cont: &ParCont) -> WaitingContinuation<BindPattern, TaggedContinuation> {
    WaitingContinuation {
        source: Consume::create(
            &cont.channels,
            &cont.patterns,
            &cont.continuation,
            cont.persistent,
        ),
        patterns: cont.patterns.clone(),
        continuation: cont.continuation.clone(),
        persist: cont.persistent,
        peeks: peeks_of(cont.peek, cont.channels.len()),
    }
}

// Changed by decision 90 (decision 69): these helpers served only the legacy charges of Option P,
// which DR-134 deleted. A host call returns its observer calls (`Recorder::since`).
//
// /// The new legacy log entries of one host call: the entries after the first `before`.
// pub fn new_entries(budget: &RuntimeBudget, before: usize) -> Vec<Cost> {
//     budget.get_log()[before..].to_vec()
// }
//
// /// The legacy log entries of `charges`: the pre-charge, then the post-charges.
// pub fn split(charges: LegacyCharges) -> Vec<Cost> {
//     std::iter::once(charges.pre).chain(charges.post).collect()
// }

/// A sent value of a host result, or `None` when it is not one number.
pub fn number(data: &ListParWithRandom) -> Option<i64> {
    match data.pars.as_slice() {
        [Par { exprs, .. }] => match exprs.as_slice() {
            [Expr {
                expr_instance: Some(ExprInstance::GInt(value)),
            }] => Some(*value),
            _ => None,
        },
        _ => None,
    }
}

/// The re-produces of a fire whose continuation peeks: every result that is not persistent, in
/// result order, as `produce_peeks` sends them, as `(channel, record)`.
pub fn peek_reproduces(cont: &ParCont, results: &[ParResult]) -> Vec<(Par, ListParWithRandom)> {
    match cont.peek {
        true => results
            .iter()
            .filter(|result| !result.persistent)
            .map(|result| (result.channel.clone(), result.removed_datum.clone()))
            .collect(),
        false => Vec::new(),
    }
}

/// One host produce and its equivalent staged produce.
pub struct HostProduce {
    // Changed by decision 90 (decision 69): a host call returns its observer calls.
    // /// The legacy log entries that the produce added.
    // pub entries: Vec<Cost>,
    /// The observer calls that the produce made, in call order.
    pub calls: Vec<Call>,
    /// The fire of the produce, or `None` when it stored its datum.
    pub fired: Option<(ParCont, Vec<ParResult>)>,
    /// The staged produce that the host produce is equivalent to.
    pub staged: ParSpaceProduce,
    /// The re-produces that the host reducer makes after the fire.
    pub reproduced: Vec<(Par, ListParWithRandom)>,
}

/// One host consume and its equivalent staged consume.
pub struct HostConsume {
    // Changed by decision 90 (decision 69): a host call returns its observer calls.
    // /// The legacy log entries that the consume added.
    // pub entries: Vec<Cost>,
    /// The observer calls that the consume made, in call order.
    pub calls: Vec<Call>,
    /// The fire of the consume, or `None` when it parked its continuation.
    pub fired: Option<(ParCont, Vec<ParResult>)>,
    /// The staged consume that the host consume is equivalent to.
    pub staged: ParSpaceConsume,
    /// The re-produces that the host reducer makes after the fire.
    pub reproduced: Vec<(Par, ListParWithRandom)>,
}

// Changed by decision 90 (decision 69): the host records its observer calls in `recorder`, so the
// call returns them instead of the legacy log entries of a budget.
//
// /// Runs one produce on `host`, which charges `budget`, and returns its entries, its fire, its
// /// equivalent staged produce and its re-produces.
// pub async fn host_produce(
//     host: &impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
//     budget: &RuntimeBudget,
//     channel: Par,
//     data: ListParWithRandom,
//     persist: bool,
// ) -> HostProduce {
//     let before = budget.get_log().len();
//     ...
//     let entries = new_entries(budget, before);

/// Runs one produce on `host`, whose accounting observer is `recorder`. It returns the observer
/// calls of the produce, its fire, its equivalent staged produce and its re-produces.
pub async fn host_produce(
    host: &impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    recorder: &Recorder,
    channel: Par,
    data: ListParWithRandom,
    persist: bool,
) -> HostProduce {
    let before = recorder.count();
    let result = host
        .produce(channel.clone(), data.clone(), persist)
        .await
        .expect("the host produce runs");
    let calls = recorder.since(before);
    let fired = result.map(|(cont, results, _)| (cont, results));
    let (outcome, reproduced) = match &fired {
        None => (ProduceOutcome::Store, Vec::new()),
        Some((cont, results)) => (
            ProduceOutcome::Fire {
                channels: cont.channels.clone(),
                continuation: waiter(cont),
                data: candidates(results),
            },
            peek_reproduces(cont, results),
        ),
    };
    HostProduce {
        calls,
        fired,
        staged: ParSpaceProduce {
            channel,
            data,
            persist,
            outcome,
            before_observe: None,
            own: false,
            // Added by decision 123 (plan v4, V3-9): no row search.
            search: None,
        },
        reproduced,
    }
}

// Changed by decision 90 (decision 69): the host records its observer calls in `recorder`, so the
// call returns them instead of the legacy log entries of a budget.
//
// /// Runs one consume on `host`, which charges `budget`, and returns its entries, its fire, its
// /// equivalent staged consume and its re-produces.
// pub async fn host_consume(
//     host: &impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
//     budget: &RuntimeBudget,
//     channels: Vec<Par>,
//     patterns: Vec<BindPattern>,
//     continuation: TaggedContinuation,
//     persist: bool,
//     peek: bool,
// ) -> HostConsume {
//     let peeks = peeks_of(peek, channels.len());
//     let before = budget.get_log().len();
//     ...
//     let entries = new_entries(budget, before);

/// Runs one consume on `host`, whose accounting observer is `recorder`. It returns the observer
/// calls of the consume, its fire, its equivalent staged consume and its re-produces.
pub async fn host_consume(
    host: &impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    recorder: &Recorder,
    channels: Vec<Par>,
    patterns: Vec<BindPattern>,
    continuation: TaggedContinuation,
    persist: bool,
    peek: bool,
) -> HostConsume {
    let peeks = peeks_of(peek, channels.len());
    let before = recorder.count();
    let result = host
        .consume(
            channels.clone(),
            patterns.clone(),
            continuation.clone(),
            persist,
            peeks.clone(),
        )
        .await
        .expect("the host consume runs");
    let calls = recorder.since(before);
    let (outcome, reproduced) = match &result {
        None => (ConsumeOutcome::Park, Vec::new()),
        Some((cont, results)) => (
            ConsumeOutcome::Fire {
                data: candidates(results),
            },
            peek_reproduces(cont, results),
        ),
    };
    HostConsume {
        calls,
        fired: result,
        staged: ParSpaceConsume {
            channels,
            patterns,
            continuation,
            persist,
            peeks,
            outcome,
            before_observe: None,
            own: false,
            // Added by decision 123 (plan v4, V3-9): no row search.
            search: None,
        },
        reproduced,
    }
}

// ─── The canonical choice of host play (decision 89, DR-129) ─────────────────────────────────

/// The maximum number of draws of one host consume in [`drawn_consume`]. A host consume picks
/// uniformly at random among the stored records that its matcher admits. A draw therefore binds
/// a record with the persist flag of the record that the agent binds, which the host program
/// reads from the receipt of the agent, with a chance of at least `1/n` for `n` admitted records.
/// A generated program stores at most twelve records, so all draws miss with a chance below
/// `(11/12)^1024`, which is less than `1e-38`.
///
/// Changed by decision 102 (plan v2 pins): the host program follows the pick of the agent. The
/// text before: "A draw therefore binds a record with the persist flag of the canonical record
/// with a chance of at least `1/n` for `n` admitted records."
pub const HOST_DRAWS: usize = 1024;

/// The number of runs of a host program in a test of the draws of [`drawn_consume`]. One run
/// of an oracle without the draws binds the persist flag of the pick of the agent with a chance
/// of one half. So all runs of such an oracle pass with a chance of `2^-16`.
///
/// Changed by decision 102 (plan v2 pins): the host program follows the pick of the agent. The
/// text before: "One run of an oracle without the draws makes the canonical choice with a chance
/// of one half."
pub const TWIN_RUNS: usize = 16;

/// The waiting continuation that a host produce may fire under a pinned matcher.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WaiterPin {
    /// Every waiter. A consume has this pin, because it fires only its own continuation.
    Any,
    /// Only this waiting continuation, boxed because a continuation is large.
    Only(Box<TaggedContinuation>),
    /// No waiter. The produce stores its record.
    Nothing,
}

impl WaiterPin {
    /// Whether the pin lets a fire of `continuation` commit. Host play skips a waiter that the
    /// pin refuses, as it skips a waiter whose guard refuses (`extract_first_match`).
    pub fn admits(&self, continuation: &TaggedContinuation) -> bool {
        match self {
            WaiterPin::Any => true,
            WaiterPin::Only(chosen) => chosen.as_ref() == continuation,
            WaiterPin::Nothing => false,
        }
    }
}

/// The waiter pin of a host matcher, shared with the host program.
pub type WaiterSlot = Arc<Mutex<WaiterPin>>;

/// A waiter slot that admits every waiter.
pub fn open_waiter() -> WaiterSlot { Arc::new(Mutex::new(WaiterPin::Any)) }

/// Sets the waiter pin of `waiter` to `choice`.
pub fn set_waiter(waiter: &WaiterSlot, choice: WaiterPin) {
    *waiter.lock().expect("the waiter lock") = choice;
}

/// The first waiting continuation of `host` on `channel` in the DR-75 order whose pattern binds
/// the payload of the datum record `record` and whose guard admits the match (decision 37).
/// `Nothing` means that no waiter binds and admits the record, so a produce of it stores the
/// record. Host play picks at random since DR-129, and the agent picks at random since decision
/// 102. So a host program uses this function only to check that the agent may store a record.
/// It reads each waiter pin from the receipts of the agent (plan v2 section 5).
///
/// Changed by decision 102 (plan v2 pins): the waiter of a produce is a random pick. The text
/// before: "The waiter that a produce of the datum record `record` on `channel` fires under the
/// canonical order of decision 41. It is the first waiting continuation of `host` in canonical
/// order whose pattern binds the payload and whose guard admits the match (decision 37).
/// `Nothing` means that the produce stores the record."
pub async fn waiter_choice(
    host: &impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    channel: &Par,
    record: &ListParWithRandom,
) -> WaiterPin {
    let payload = record::unwrap_datum(record).expect("a datum record").datum;
    let waiters = host.get_waiting_continuations(vec![channel.clone()]).await;
    canonical_continuation_order(&waiters)
        .into_iter()
        .map(|index| &waiters[index])
        .find(|waiter| {
            waiter
                .patterns
                .first()
                .and_then(|pattern| Matcher.get(pattern, &payload))
                .is_some_and(|matched| {
                    Matcher.check_commit(&waiter.continuation, std::slice::from_ref(&matched))
                })
        })
        .map_or(WaiterPin::Nothing, |waiter| {
            WaiterPin::Only(Box::new(waiter.continuation.clone()))
        })
}

/// Makes the host consume `consume` on `host` until it binds records with the persist flags
/// `expected`, in bind order, and returns that run. A suite pins the records of a consume by
/// their bytes. A persistent record and a non-persistent record can have equal bytes, and host
/// play picks among them at random. So a draw that binds a record with the other persist flag
/// is reverted to a soft checkpoint and made again, at most [`HOST_DRAWS`] times. A run that
/// parks returns at once. The observer calls of a reverted draw stay in the record of the
/// recorder of the host, so a caller keeps the calls of each returned run.
///
/// Changed by decision 90 (decision 69): a soft checkpoint takes the event log and the produce
/// counters of the host. The counters give the `times_repeated` of each later COMM, which the
/// observer calls carry. So each draw first restores the state of the checkpoint, and the
/// program keeps the counters of the agent.
pub async fn drawn_consume<F, Fut>(
    host: &impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    expected: &[bool],
    mut consume: F,
) -> Result<HostConsume, String>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = HostConsume>,
{
    for _ in 0..HOST_DRAWS {
        let checkpoint = host.create_soft_checkpoint().await;
        // Added by decision 90 (decision 69): see the note of the function.
        host.revert_to_soft_checkpoint(checkpoint.clone())
            .await
            .expect("the host restores the state of its checkpoint");
        let run = consume().await;
        let bound: Option<Vec<bool>> = run
            .fired
            .as_ref()
            .map(|(_, results)| results.iter().map(|result| result.persistent).collect());
        match bound {
            Some(flags) if flags.as_slice() != expected => host
                .revert_to_soft_checkpoint(checkpoint)
                .await
                .expect("the host reverts a draw"),
            _ => return Ok(run),
        }
    }
    Err(format!(
        "no host consume of {HOST_DRAWS} draws binds records with the persist flags {expected:?}"
    ))
}

// ─── Lone host calls (P5c) ───────────────────────────────────────────────────────────────────

// Changed by decision 90 (decision 69): a lone host call has no legacy budget, because DR-134
// deleted the legacy meter.
//
// /// The phlo of the budget of a lone host call: far above the charges of one operation.
// const LONE_PHLO: i64 = 1_000_000_000_000;

/// The records that a [`PinnedPayloadMatcher`] may bind: `None` for every record, and
/// `Some(records)` for these records only.
pub type Pin = Arc<Mutex<Option<Vec<ListParWithRandom>>>>;

/// The matcher of a lone host call. It matches the payload of a datum record without its
/// envelope, as a space agent matches it (`spaces/matching.rs`). It matches every other record,
/// for example a counter record, whole. While its pin is set, it binds only the pinned records. The
/// guard of a continuation lies outside the envelope, so the guard check is the check of the
/// Rholang matcher.
#[derive(Clone)]
pub struct PinnedPayloadMatcher {
    /// The records that the matcher may bind.
    pub pin: Pin,
    /// Added by decision 89 (DR-129): the waiter that a produce may fire. Host play picks a
    /// waiter at random, so a host program with two waiters pins the canonical one.
    pub waiter: WaiterSlot,
}

impl Match<BindPattern, ListParWithRandom, TaggedContinuation> for PinnedPayloadMatcher {
    fn get(&self, pattern: &BindPattern, data: &ListParWithRandom) -> Option<ListParWithRandom> {
        let allowed = match self.pin.lock().expect("the pin lock").as_ref() {
            Some(pinned) => pinned.contains(data),
            None => true,
        };
        match (allowed, record::unwrap_pq_datum(data)) {
            (false, _) => None,
            (true, Ok(decoded)) => Matcher.get(pattern, &decoded.datum),
            (true, Err(_)) => Matcher.get(pattern, data),
        }
    }

    fn check_commit(
        &self,
        continuation: &TaggedContinuation,
        matched: &[ListParWithRandom],
    ) -> bool {
        // Changed by decision 89 (DR-129): the waiter pin admits the fire first.
        // Matcher.check_commit(continuation, matched)
        let allowed = self
            .waiter
            .lock()
            .expect("the waiter lock")
            .admits(continuation);
        allowed && Matcher.check_commit(continuation, matched)
    }
}

// Changed by decision 89 (DR-129): the matcher of the host also holds a waiter pin.
//
// /// A `ChargingRSpace` over a fresh in-memory space with the [`PinnedPayloadMatcher`] of `pin`,
// /// which charges `budget`.
// pub async fn pinned_charging_host(
//     budget: &RuntimeBudget,
//     pin: &Pin,
// ) -> impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation> {
//     let mut stores = InMemoryStoreManager::new();
//     let space = ParRSpace::create(
//         stores.r_space_stores().await.expect("in-memory stores"),
//         Arc::new(Box::new(PinnedPayloadMatcher { pin: pin.clone() })),
//     )
//     .expect("an in-memory space");
//     let host: RhoISpace = Arc::new(Box::new(space));
//     ChargingRSpace::charging_rspace(host, budget.clone())
// }

// Changed by decision 90 (decision 69): `ChargingRSpace` is deleted with the legacy meter
// (DR-134), so the pinned host records its observer calls (`pinned_observed_host`).
//
// /// A `ChargingRSpace` over a fresh in-memory space with the [`PinnedPayloadMatcher`] of `pin`
// /// and `waiter`, which charges `budget`.
// pub async fn pinned_charging_host(
//     budget: &RuntimeBudget,
//     pin: &Pin,
//     waiter: &WaiterSlot,
// ) -> impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation> {
//     let mut stores = InMemoryStoreManager::new();
//     let space = ParRSpace::create(
//         stores.r_space_stores().await.expect("in-memory stores"),
//         Arc::new(Box::new(PinnedPayloadMatcher {
//             pin: pin.clone(),
//             waiter: waiter.clone(),
//         })),
//     )
//     .expect("an in-memory space");
//     let host: RhoISpace = Arc::new(Box::new(space));
//     ChargingRSpace::charging_rspace(host, budget.clone())
// }

/// A fresh in-memory space with the [`PinnedPayloadMatcher`] of `pin` and `waiter`, whose
/// accounting observer is `recorder`.
pub async fn pinned_observed_host(
    recorder: &Arc<Recorder>,
    pin: &Pin,
    waiter: &WaiterSlot,
) -> ParRSpace {
    let mut stores = InMemoryStoreManager::new();
    let space = ParRSpace::create(
        stores.r_space_stores().await.expect("in-memory stores"),
        Arc::new(Box::new(PinnedPayloadMatcher {
            pin: pin.clone(),
            waiter: waiter.clone(),
        })),
    )
    .expect("an in-memory space");
    observe(&space, recorder);
    space
}

/// Sets the pin of a lone host call to `records`.
fn set_pin(pin: &Pin, records: Vec<ListParWithRandom>) {
    *pin.lock().expect("the pin lock") = Some(records);
}

/// Stores `candidate`, the stored record of a fire, on its channel of a lone host, with its
/// persist flag. The pin of the host is closed, so the store fires nothing.
async fn store_candidate(
    host: &impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    candidate: &ConsumeCandidate<Par, ListParWithRandom>,
) {
    let fired = host
        .produce(
            candidate.channel.clone(),
            candidate.removed_datum.clone(),
            candidate.datum.persist,
        )
        .await
        .expect("a lone host stores a candidate record");
    assert!(fired.is_none(), "a closed pin fires nothing");
}

/// The observer calls of the staged produce `op` when a fresh host runs it alone.
///
/// 1. The host stores the stored candidates of a fire with their persist flags and parks its
///    waiter, with a closed pin, so nothing fires.
/// 2. The pin opens to the records of the operation: the produced record and the candidates.
/// 3. The host runs the produce, and the call fires exactly when `op` fires.
///
/// The calls are those of step 3. A fire whose group repeats a channel must not bind a
/// persistent record there. The host binds a persistent record again for each occurrence of its
/// channel, and decision 28 forbids that for spaces.
///
/// The fresh host holds only the records and the waiter of the fire. So its calls equal the
/// calls of an agent only when the store of the agent holds no other join group on the channel,
/// and has produced each bound record once since its last event log (the `times_repeated` of
/// the COMM).
// Changed by decision 90 (decision 69): a lone host call returns its observer calls.
// pub async fn lone_host_produce(op: &ParSpaceProduce) -> Vec<Cost> {
pub async fn lone_host_produce(op: &ParSpaceProduce) -> Vec<Call> {
    // Changed by decision 90 (decision 69): the host records its observer calls.
    // let budget = RuntimeBudget::new(Cost::create(LONE_PHLO, "rrs lone host produce"));
    // let _scope = budget.enter_legacy_scope();
    let recorder = Recorder::new();
    let pin: Pin = Arc::new(Mutex::new(Some(Vec::new())));
    // Changed by decision 89 (DR-129): a lone host holds at most one waiter, so its waiter pin
    // is open.
    // let host = pinned_charging_host(&budget, &pin).await;
    // Changed by decision 90 (decision 69): the pinned host records its observer calls.
    // let host = pinned_charging_host(&budget, &pin, &open_waiter()).await;
    let host = pinned_observed_host(&recorder, &pin, &open_waiter()).await;
    let mut pinned = vec![op.data.clone()];
    let fires = match &op.outcome {
        ProduceOutcome::Store => false,
        ProduceOutcome::Fire {
            channels,
            continuation,
            data,
        } => {
            for candidate in data.iter().filter(|candidate| candidate.datum_index >= 0) {
                store_candidate(&host, candidate).await;
                pinned.push(candidate.removed_datum.clone());
            }
            let parked = host
                .consume(
                    channels.clone(),
                    continuation.patterns.clone(),
                    continuation.continuation.clone(),
                    continuation.persist,
                    continuation.peeks.clone(),
                )
                .await
                .expect("a lone host parks the waiter of a fire");
            assert!(parked.is_none(), "a closed pin parks the waiter");
            true
        }
    };
    set_pin(&pin, pinned);
    // let run = host_produce(
    //     &host,
    //     &budget,
    let run = host_produce(
        &host,
        &recorder,
        op.channel.clone(),
        op.data.clone(),
        op.persist,
    )
    .await;
    assert_eq!(
        run.fired.is_some(),
        fires,
        "the lone host produce fires exactly when the staged produce fires"
    );
    // run.entries
    run.calls
}

/// The observer calls of the staged consume `op` when a fresh host runs it alone. The host
/// stores the candidates of a fire with a closed pin and opens the pin to them. Then it runs the
/// consume with the peeks of `op`, which are empty or every bind. The call fires exactly when
/// `op` fires. The rule on repeated channels of [`lone_host_produce`] holds here too. The fresh
/// host binds the payload of a datum record, so its calls equal the calls of an agent for a
/// consume that binds payloads, and not for a machinery consume (`K_lift`, `K_retire`), which
/// binds a whole record.
// Changed by decision 90 (decision 69): a lone host call returns its observer calls.
// pub async fn lone_host_consume(op: &ParSpaceConsume) -> Vec<Cost> {
pub async fn lone_host_consume(op: &ParSpaceConsume) -> Vec<Call> {
    // Changed by decision 90 (decision 69): the host records its observer calls.
    // let budget = RuntimeBudget::new(Cost::create(LONE_PHLO, "rrs lone host consume"));
    // let _scope = budget.enter_legacy_scope();
    let recorder = Recorder::new();
    let pin: Pin = Arc::new(Mutex::new(Some(Vec::new())));
    // Changed by decision 89 (DR-129): a lone host holds at most one waiter, so its waiter pin
    // is open.
    // let host = pinned_charging_host(&budget, &pin).await;
    // Changed by decision 90 (decision 69): the pinned host records its observer calls.
    // let host = pinned_charging_host(&budget, &pin, &open_waiter()).await;
    let host = pinned_observed_host(&recorder, &pin, &open_waiter()).await;
    let mut pinned = Vec::new();
    let fires = match &op.outcome {
        ConsumeOutcome::Park => false,
        ConsumeOutcome::Fire { data } => {
            for candidate in data {
                store_candidate(&host, candidate).await;
                pinned.push(candidate.removed_datum.clone());
            }
            true
        }
    };
    let peek = !op.peeks.is_empty();
    assert_eq!(
        op.peeks,
        peeks_of(peek, op.channels.len()),
        "the peeks of a staged consume are empty or every bind"
    );
    set_pin(&pin, pinned);
    // let run = host_consume(
    //     &host,
    //     &budget,
    let run = host_consume(
        &host,
        &recorder,
        op.channels.clone(),
        op.patterns.clone(),
        op.continuation.clone(),
        op.persist,
        peek,
    )
    .await;
    assert_eq!(
        run.fired.is_some(),
        fires,
        "the lone host consume fires exactly when the staged consume fires"
    );
    // run.entries
    run.calls
}
