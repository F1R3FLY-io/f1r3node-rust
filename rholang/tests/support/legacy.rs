//! The host side of the legacy-charges differential of decision 45 (Option P), moved here from
//! `reified_legacy_charges.rs` so that every P5 discipline can compare its staged operations
//! with the equivalent host program.
//!
//! A host call runs on the real `ChargingRSpace` over an in-memory RSpace. [`host_produce`] and
//! [`host_consume`] return the legacy log entries that the call added, the result of the call,
//! the staged operation that the call is equivalent to, built from its inputs and its result,
//! and the re-produces that the host reducer makes after a peek fire (`produce_peeks`, decision
//! 45, Rule E). A test compares the charges of the staged operation (`produce_charges`,
//! `consume_charges`) with the entries.
//!
//! The join suites of P5c run each staged operation of an agent alone on a fresh host
//! ([`lone_host_produce`], [`lone_host_consume`]). The host matches with a
//! [`PinnedPayloadMatcher`], so it binds exactly the records that the staged operation binds,
//! by their payloads, as the agent matches them.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{BindPattern, Expr, ListParWithRandom, Par, TaggedContinuation};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::RuntimeBudget;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::reified::legacy_charges::{
    LegacyCharges, ParSpaceConsume, ParSpaceProduce,
};
use rholang::rust::interpreter::rho_runtime::RhoISpace;
use rholang::rust::interpreter::spaces::record;
use rholang::rust::interpreter::storage::charging_rspace::ChargingRSpace;
use rspace_plus_plus::rspace::internal::{ConsumeCandidate, Datum, WaitingContinuation};
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::{ContResult, ISpace, RSpaceResult};
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::space_store_handle::{ConsumeOutcome, ProduceOutcome};
use rspace_plus_plus::rspace::trace::event::{Consume, Produce};

type ParRSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

/// The continuation result of a host fire.
pub type ParCont = ContResult<Par, BindPattern, TaggedContinuation>;

/// One bound record of a host fire.
pub type ParResult = RSpaceResult<Par, ListParWithRandom>;

/// A `ChargingRSpace` over a fresh in-memory RSpace that charges `budget`.
pub async fn charging_host(
    budget: &RuntimeBudget,
) -> impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation> {
    let mut stores = InMemoryStoreManager::new();
    let space = ParRSpace::create(
        stores.r_space_stores().await.expect("in-memory stores"),
        Arc::new(Box::new(Matcher)),
    )
    .expect("an in-memory space");
    let host: RhoISpace = Arc::new(Box::new(space));
    ChargingRSpace::charging_rspace(host, budget.clone())
}

/// The candidates of a fire from the host results, in channel order. The datum index plays no
/// part in the charges.
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

/// The new legacy log entries of one host call: the entries after the first `before`.
pub fn new_entries(budget: &RuntimeBudget, before: usize) -> Vec<Cost> {
    budget.get_log()[before..].to_vec()
}

/// The legacy log entries of `charges`: the pre-charge, then the post-charges.
pub fn split(charges: LegacyCharges) -> Vec<Cost> {
    std::iter::once(charges.pre).chain(charges.post).collect()
}

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
    /// The legacy log entries that the produce added.
    pub entries: Vec<Cost>,
    /// The fire of the produce, or `None` when it stored its datum.
    pub fired: Option<(ParCont, Vec<ParResult>)>,
    /// The staged produce that the host produce is equivalent to.
    pub staged: ParSpaceProduce,
    /// The re-produces that the host reducer makes after the fire.
    pub reproduced: Vec<(Par, ListParWithRandom)>,
}

/// One host consume and its equivalent staged consume.
pub struct HostConsume {
    /// The legacy log entries that the consume added.
    pub entries: Vec<Cost>,
    /// The fire of the consume, or `None` when it parked its continuation.
    pub fired: Option<(ParCont, Vec<ParResult>)>,
    /// The staged consume that the host consume is equivalent to.
    pub staged: ParSpaceConsume,
    /// The re-produces that the host reducer makes after the fire.
    pub reproduced: Vec<(Par, ListParWithRandom)>,
}

/// Runs one produce on `host`, which charges `budget`, and returns its entries, its fire, its
/// equivalent staged produce and its re-produces.
pub async fn host_produce(
    host: &impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    budget: &RuntimeBudget,
    channel: Par,
    data: ListParWithRandom,
    persist: bool,
) -> HostProduce {
    let before = budget.get_log().len();
    let result = host
        .produce(channel.clone(), data.clone(), persist)
        .await
        .expect("the host produce runs");
    let entries = new_entries(budget, before);
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
        entries,
        fired,
        staged: ParSpaceProduce {
            channel,
            data,
            persist,
            outcome,
            before_observe: None,
        },
        reproduced,
    }
}

/// Runs one consume on `host`, which charges `budget`, and returns its entries, its fire, its
/// equivalent staged consume and its re-produces.
pub async fn host_consume(
    host: &impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    budget: &RuntimeBudget,
    channels: Vec<Par>,
    patterns: Vec<BindPattern>,
    continuation: TaggedContinuation,
    persist: bool,
    peek: bool,
) -> HostConsume {
    let peeks = peeks_of(peek, channels.len());
    let before = budget.get_log().len();
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
    let entries = new_entries(budget, before);
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
        entries,
        fired: result,
        staged: ParSpaceConsume {
            channels,
            patterns,
            continuation,
            persist,
            peeks,
            outcome,
            before_observe: None,
        },
        reproduced,
    }
}

// ─── Lone host calls (P5c) ───────────────────────────────────────────────────────────────────

/// The phlo of the budget of a lone host call: far above the charges of one operation.
const LONE_PHLO: i64 = 1_000_000_000_000;

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
        Matcher.check_commit(continuation, matched)
    }
}

/// A `ChargingRSpace` over a fresh in-memory space with the [`PinnedPayloadMatcher`] of `pin`,
/// which charges `budget`.
pub async fn pinned_charging_host(
    budget: &RuntimeBudget,
    pin: &Pin,
) -> impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation> {
    let mut stores = InMemoryStoreManager::new();
    let space = ParRSpace::create(
        stores.r_space_stores().await.expect("in-memory stores"),
        Arc::new(Box::new(PinnedPayloadMatcher { pin: pin.clone() })),
    )
    .expect("an in-memory space");
    let host: RhoISpace = Arc::new(Box::new(space));
    ChargingRSpace::charging_rspace(host, budget.clone())
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

/// The legacy log entries of the staged produce `op` when a fresh host runs it alone.
///
/// 1. The host stores the stored candidates of a fire with their persist flags and parks its
///    waiter, with a closed pin, so nothing fires.
/// 2. The pin opens to the records of the operation: the produced record and the candidates.
/// 3. The host runs the produce, and the call fires exactly when `op` fires.
///
/// A fire whose group repeats a channel must not bind a persistent record there. The host binds
/// a persistent record again for each occurrence of its channel, and decision 28 forbids that for
/// spaces.
pub async fn lone_host_produce(op: &ParSpaceProduce) -> Vec<Cost> {
    let budget = RuntimeBudget::new(Cost::create(LONE_PHLO, "rrs lone host produce"));
    let _scope = budget.enter_legacy_scope();
    let pin: Pin = Arc::new(Mutex::new(Some(Vec::new())));
    let host = pinned_charging_host(&budget, &pin).await;
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
    let run = host_produce(
        &host,
        &budget,
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
    run.entries
}

/// The legacy log entries of the staged consume `op` when a fresh host runs it alone. The host
/// stores the candidates of a fire with a closed pin and opens the pin to them. Then it runs the
/// consume with the peeks of `op`, which are empty or every bind. The call fires exactly when
/// `op` fires. The rule on repeated channels of [`lone_host_produce`] holds here too.
pub async fn lone_host_consume(op: &ParSpaceConsume) -> Vec<Cost> {
    let budget = RuntimeBudget::new(Cost::create(LONE_PHLO, "rrs lone host consume"));
    let _scope = budget.enter_legacy_scope();
    let pin: Pin = Arc::new(Mutex::new(Some(Vec::new())));
    let host = pinned_charging_host(&budget, &pin).await;
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
    let run = host_consume(
        &host,
        &budget,
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
    run.entries
}
