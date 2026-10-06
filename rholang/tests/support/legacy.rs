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

use std::collections::BTreeSet;
use std::sync::Arc;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{BindPattern, Expr, ListParWithRandom, Par, TaggedContinuation};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::RuntimeBudget;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::reified::legacy_charges::{
    LegacyCharges, ParSpaceConsume, ParSpaceProduce,
};
use rholang::rust::interpreter::rho_runtime::RhoISpace;
use rholang::rust::interpreter::storage::charging_rspace::ChargingRSpace;
use rspace_plus_plus::rspace::internal::{ConsumeCandidate, Datum, WaitingContinuation};
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
