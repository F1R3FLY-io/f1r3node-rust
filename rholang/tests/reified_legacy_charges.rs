//! The legacy storage charges of space operations (decision 45, Option P) against the host.
//!
//! `legacy_charges` computes, for each staged space operation, what `ChargingRSpace` charges for
//! the equivalent host call. These properties drive the real `ChargingRSpace` over an in-memory
//! RSpace with generated host programs. For each host call they build the staged operation that
//! the call is equivalent to, from its inputs and its result, and they compare the charges of
//! `legacy_charges` with the entries that the call added to the legacy log, value and label.

#[path = "support/mod.rs"]
mod support;

use std::collections::{BTreeSet, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, ParWithRandom, TaggedContinuation};
use models::rust::utils::{new_freevar_par, new_gint_par, new_gstring_par};
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::test_runner::{RngSeed, TestCaseError, TestRunner};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::RuntimeBudget;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::reified::legacy_charges::{
    consume_charges, produce_charges, LegacyChain, LegacyCharges, ParSpaceConsume, ParSpaceProduce,
};
use rholang::rust::interpreter::spaces::errors::SpaceError;
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::internal::{ConsumeCandidate, Datum, WaitingContinuation};
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::{ContResult, ISpace, RSpaceResult};
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::space_store_handle::{
    ConsumeOutcome, ProduceOutcome, SpaceStoreHandle, SpaceTxnError,
};
use rspace_plus_plus::rspace::trace::event::{Consume, Event, Produce};
use support::legacy::{charging_host, host_consume, host_produce, number, split};

type ParRSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

/// The fixed seed of every property.
const PROPERTY_SEED: u64 = 0x0E0D_2026_1006_0045;

fn config() -> ProptestConfig {
    ProptestConfig {
        cases: 64,
        rng_seed: RngSeed::Fixed(PROPERTY_SEED),
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

fn check<S>(strategy: S, test: impl Fn(S::Value) -> Result<(), TestCaseError>)
where
    S: Strategy,
    S::Value: std::fmt::Debug,
{
    TestRunner::new(config())
        .run(&strategy, test)
        .unwrap_or_else(|failure| panic!("{failure}"));
}

fn tokio_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("build the tokio runtime of a case")
}

const CHANNELS: [&str; 2] = ["rrs-legacy-a", "rrs-legacy-b"];

fn channel(index: usize) -> Par { new_gstring_par(CHANNELS[index].to_string(), Vec::new(), false) }

/// A pattern of one bind: any value, or one literal value.
#[derive(Clone, Copy, Debug)]
enum Pat {
    Any,
    Lit(i64),
}

impl Pat {
    fn bind_pattern(self) -> BindPattern {
        match self {
            Pat::Any => BindPattern {
                patterns: vec![new_freevar_par(0, Vec::new())],
                remainder: None,
                free_count: 1,
            },
            Pat::Lit(value) => BindPattern {
                patterns: vec![new_gint_par(value, Vec::new(), false)],
                remainder: None,
                free_count: 0,
            },
        }
    }
}

/// One host call of a generated program.
#[derive(Clone, Debug)]
enum HostOp {
    Produce {
        channel: usize,
        value: i64,
        persist: bool,
    },
    Consume {
        channels: Vec<usize>,
        patterns: Vec<Pat>,
        persist: bool,
        peek: bool,
    },
}

fn arb_pat() -> BoxedStrategy<Pat> {
    prop_oneof![2 => Just(Pat::Any), 1 => (0i64..3).prop_map(Pat::Lit)].boxed()
}

fn arb_host_op() -> BoxedStrategy<HostOp> {
    let produce = (
        0usize..2,
        0i64..3,
        prop_oneof![3 => Just(false), 1 => Just(true)],
    )
        .prop_map(|(channel, value, persist)| HostOp::Produce {
            channel,
            value,
            persist,
        });
    let one = (0usize..2, arb_pat()).prop_map(|(channel, pattern)| (vec![channel], vec![pattern]));
    let two = (arb_pat(), arb_pat()).prop_map(|(first, second)| (vec![0, 1], vec![first, second]));
    let consume =
        (prop_oneof![3 => one, 1 => two], 0u8..4).prop_map(|((channels, patterns), kind)| {
            HostOp::Consume {
                channels,
                patterns,
                persist: kind == 2,
                peek: kind == 3,
            }
        });
    prop_oneof![3 => produce, 2 => consume].boxed()
}

fn datum(value: i64, salt: u64) -> ListParWithRandom {
    ListParWithRandom {
        pars: vec![new_gint_par(value, Vec::new(), false)],
        random_state: [b"rrs-legacy-datum-".as_slice(), &salt.to_be_bytes()].concat(),
        cost_authority: None,
        cost_stack: None,
    }
}

fn continuation(salt: u64) -> TaggedContinuation {
    TaggedContinuation {
        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(new_gstring_par(format!("body-{salt}"), Vec::new(), false)),
            random_state: [b"rrs-legacy-body-".as_slice(), &salt.to_be_bytes()].concat(),
        })),
        guard: None,
        cost_authority: None,
    }
}

/// `legacy_charges_equal_charging_rspace`: for every host call of a generated program, the
/// charges of the equivalent staged operation equal the legacy log entries that
/// `ChargingRSpace` adds for the call: the pre-charge, then the refunds and the event fees of
/// its result. The program includes stores, parks, fires on one and two channels, persistent
/// data and continuations, peeks, and the re-produce of each peeked record that is not
/// persistent, which the host reducer makes with `produce_peeks` (decision 45, Rule E).
#[test]
fn prop_legacy_charges_equal_charging_rspace() {
    check(vec(arb_host_op(), 1..16), |ops| {
        tokio_runtime().block_on(legacy_charges_equal_charging_rspace_case(ops))
    });
    for (shape, count) in Shape::ALL.iter().zip(COVERAGE.iter()) {
        assert!(
            count.load(Ordering::Relaxed) > 0,
            "the generated programs never made a {shape:?}"
        );
    }
}

/// The shapes of host calls that the property must meet at least once over its cases.
#[derive(Clone, Copy, Debug)]
enum Shape {
    Store,
    Park,
    ProduceFire,
    ConsumeFire,
    JoinFire,
    PeekReproduce,
    PersistentRecordBound,
    PersistentContinuationFired,
}

impl Shape {
    const ALL: [Shape; 8] = [
        Shape::Store,
        Shape::Park,
        Shape::ProduceFire,
        Shape::ConsumeFire,
        Shape::JoinFire,
        Shape::PeekReproduce,
        Shape::PersistentRecordBound,
        Shape::PersistentContinuationFired,
    ];
}

static COVERAGE: [AtomicUsize; 8] = [const { AtomicUsize::new(0) }; 8];

fn met(shape: Shape) { COVERAGE[shape as usize].fetch_add(1, Ordering::Relaxed); }

/// Counts the shapes of a fire.
fn met_fire(
    cont: &ContResult<Par, BindPattern, TaggedContinuation>,
    results: &[RSpaceResult<Par, ListParWithRandom>],
) {
    if cont.channels.len() > 1 {
        met(Shape::JoinFire);
    }
    if cont.persistent {
        met(Shape::PersistentContinuationFired);
    }
    if results.iter().any(|result| result.persistent) {
        met(Shape::PersistentRecordBound);
    }
}

async fn legacy_charges_equal_charging_rspace_case(ops: Vec<HostOp>) -> Result<(), TestCaseError> {
    let budget = RuntimeBudget::new(Cost::create(i64::MAX / 4, "rrs legacy differential"));
    let _legacy = budget.enter_legacy_scope();
    let host = charging_host(&budget).await;
    let mut calls: VecDeque<HostCall> = ops.into_iter().map(HostCall::Generated).collect();
    let mut salt = 0u64;
    while let Some(call) = calls.pop_front() {
        salt += 1;
        let reproduced = match call {
            HostCall::Generated(HostOp::Produce {
                channel: index,
                value,
                persist,
            }) => {
                check_produce(&host, &budget, channel(index), datum(value, salt), persist).await?
            }
            HostCall::Reproduce { channel, data } => {
                met(Shape::PeekReproduce);
                check_produce(&host, &budget, *channel, data, false).await?
            }
            HostCall::Generated(HostOp::Consume {
                channels,
                patterns,
                persist,
                peek,
            }) => {
                let channels: Vec<Par> = channels.into_iter().map(channel).collect();
                let patterns: Vec<BindPattern> =
                    patterns.into_iter().map(Pat::bind_pattern).collect();
                check_consume(
                    &host,
                    &budget,
                    channels,
                    patterns,
                    continuation(salt),
                    persist,
                    peek,
                )
                .await?
            }
        };
        for (index, again) in reproduced.into_iter().enumerate() {
            calls.insert(index, again);
        }
    }
    Ok(())
}

/// A call of the host program: a generated operation, or the re-produce of a peeked record
/// that the host reducer makes with `produce_peeks`. The channel is boxed, so the two variants
/// have similar sizes.
#[derive(Debug)]
enum HostCall {
    Generated(HostOp),
    Reproduce {
        channel: Box<Par>,
        data: ListParWithRandom,
    },
}

/// The host calls of the re-produces that the host reducer makes after a peek fire.
fn reproduce_calls(reproduced: Vec<(Par, ListParWithRandom)>) -> Vec<HostCall> {
    reproduced
        .into_iter()
        .map(|(channel, data)| HostCall::Reproduce {
            channel: Box::new(channel),
            data,
        })
        .collect()
}

/// Runs one produce on the host and checks its legacy log entries against the charges of the
/// equivalent staged produce. Returns the re-produces that the host reducer would make.
async fn check_produce(
    host: &impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    budget: &RuntimeBudget,
    channel: Par,
    data: ListParWithRandom,
    persist: bool,
) -> Result<Vec<HostCall>, TestCaseError> {
    let run = host_produce(host, budget, channel, data, persist).await;
    match &run.fired {
        None => met(Shape::Store),
        Some((cont, results)) => {
            met(Shape::ProduceFire);
            met_fire(cont, results);
        }
    }
    let charges = produce_charges(&run.staged).expect("the produce charges");
    prop_assert_eq!(
        split(charges),
        run.entries,
        "produce, persist {}, bound values {:?}",
        persist,
        run.fired.as_ref().map(|(_, results)| results
            .iter()
            .map(|r| number(&r.matched_datum))
            .collect::<Vec<_>>())
    );
    Ok(reproduce_calls(run.reproduced))
}

/// Runs one consume on the host and checks its legacy log entries against the charges of the
/// equivalent staged consume. Returns the re-produces that the host reducer would make.
async fn check_consume(
    host: &impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    budget: &RuntimeBudget,
    channels: Vec<Par>,
    patterns: Vec<BindPattern>,
    continuation: TaggedContinuation,
    persist: bool,
    peek: bool,
) -> Result<Vec<HostCall>, TestCaseError> {
    let run = host_consume(
        host,
        budget,
        channels,
        patterns,
        continuation,
        persist,
        peek,
    )
    .await;
    match &run.fired {
        None => met(Shape::Park),
        Some((cont, results)) => {
            met(Shape::ConsumeFire);
            met_fire(cont, results);
        }
    }
    let charges = consume_charges(&run.staged).expect("the consume charges");
    prop_assert_eq!(
        split(charges),
        run.entries,
        "consume, persist {}, peek {}",
        persist,
        peek
    );
    Ok(reproduce_calls(run.reproduced))
}

// ─── The chain of one transaction (Q79 and Q80 of the P4 map) ────────────────────────────────

/// One staged step of a generated transaction. A fire step binds the record or the waiter that
/// the transaction put last on its channel, and it becomes a store or a park when there is none.
#[derive(Clone, Debug)]
enum Step {
    Store {
        channel: usize,
        value: i64,
        persist: bool,
    },
    Park {
        channel: usize,
        persist: bool,
    },
    ConsumeFire {
        channel: usize,
        persist: bool,
    },
    ProduceFire {
        channel: usize,
        value: i64,
        persist: bool,
    },
}

fn arb_step() -> BoxedStrategy<Step> {
    let flag = || prop_oneof![3 => Just(false), 1 => Just(true)];
    prop_oneof![
        3 => (0usize..2, 0i64..3, flag()).prop_map(|(channel, value, persist)| Step::Store {
            channel,
            value,
            persist,
        }),
        2 => (0usize..2, flag()).prop_map(|(channel, persist)| Step::Park { channel, persist }),
        3 => (0usize..2, flag()).prop_map(|(channel, persist)| Step::ConsumeFire { channel, persist }),
        3 => (0usize..2, 0i64..3, flag()).prop_map(|(channel, value, persist)| Step::ProduceFire {
            channel,
            value,
            persist,
        }),
    ]
    .boxed()
}

/// A staged operation of a generated transaction. Both operations are boxed, because a staged
/// produce is much larger than a staged consume.
enum StagedOp {
    Produce(Box<ParSpaceProduce>),
    Consume(Box<ParSpaceConsume>),
}

impl StagedOp {
    fn charges(&self) -> LegacyCharges {
        match self {
            StagedOp::Produce(op) => produce_charges(op).expect("the produce charges"),
            StagedOp::Consume(op) => consume_charges(op).expect("the consume charges"),
        }
    }
}

/// The waiter that a park of `continuation` on `channel` leaves in the staged view.
fn parked_waiter(
    channel: &Par,
    continuation: &TaggedContinuation,
    persist: bool,
) -> WaitingContinuation<BindPattern, TaggedContinuation> {
    let channels = vec![channel.clone()];
    let patterns = vec![Pat::Any.bind_pattern()];
    WaitingContinuation {
        source: Consume::create(&channels, &patterns, continuation, persist),
        patterns,
        continuation: continuation.clone(),
        persist,
        peeks: BTreeSet::new(),
    }
}

/// The staged operations of `steps`, valid against the staged view of an empty store: each
/// fire binds the record or the waiter that the transaction put last on its channel.
fn staged_ops(steps: &[Step]) -> Vec<StagedOp> {
    let mut records: [Vec<(ListParWithRandom, bool)>; 2] = [Vec::new(), Vec::new()];
    let mut waiters: [Vec<WaitingContinuation<BindPattern, TaggedContinuation>>; 2] =
        [Vec::new(), Vec::new()];
    let mut ops = Vec::with_capacity(steps.len());
    for (salt, step) in (1u64..).zip(steps) {
        let op = match step.clone() {
            Step::ConsumeFire {
                channel: index,
                persist,
            } if !records[index].is_empty() => {
                let on = channel(index);
                let (record, record_persist) = records[index][0].clone();
                if !record_persist {
                    records[index].remove(0);
                }
                StagedOp::Consume(Box::new(ParSpaceConsume {
                    channels: vec![on.clone()],
                    patterns: vec![Pat::Any.bind_pattern()],
                    continuation: continuation(salt),
                    persist,
                    peeks: BTreeSet::new(),
                    outcome: ConsumeOutcome::Fire {
                        data: vec![ConsumeCandidate {
                            channel: on.clone(),
                            datum: Datum {
                                a: record.clone(),
                                persist: record_persist,
                                source: Produce::create(&on, &record, record_persist),
                            },
                            removed_datum: record,
                            datum_index: 0,
                        }],
                    },
                    before_observe: None,
                }))
            }
            Step::ProduceFire {
                channel: index,
                value,
                persist,
            } if !waiters[index].is_empty() => {
                let on = channel(index);
                let waiter = waiters[index][0].clone();
                if !waiter.persist {
                    waiters[index].remove(0);
                }
                let data = datum(value, salt);
                StagedOp::Produce(Box::new(ParSpaceProduce {
                    channel: on.clone(),
                    data: data.clone(),
                    persist,
                    outcome: ProduceOutcome::Fire {
                        channels: vec![on.clone()],
                        continuation: waiter,
                        data: vec![ConsumeCandidate {
                            channel: on.clone(),
                            datum: Datum {
                                a: data.clone(),
                                persist,
                                source: Produce::create(&on, &data, persist),
                            },
                            removed_datum: data,
                            datum_index: -1,
                        }],
                    },
                    before_observe: None,
                }))
            }
            Step::Store {
                channel: index,
                value,
                persist,
            }
            | Step::ProduceFire {
                channel: index,
                value,
                persist,
            } => {
                let data = datum(value, salt);
                records[index].insert(0, (data.clone(), persist));
                StagedOp::Produce(Box::new(ParSpaceProduce {
                    channel: channel(index),
                    data,
                    persist,
                    outcome: ProduceOutcome::Store,
                    before_observe: None,
                }))
            }
            Step::Park {
                channel: index,
                persist,
            }
            | Step::ConsumeFire {
                channel: index,
                persist,
            } => {
                let on = channel(index);
                let body = continuation(salt);
                waiters[index].insert(0, parked_waiter(&on, &body, persist));
                StagedOp::Consume(Box::new(ParSpaceConsume {
                    channels: vec![on],
                    patterns: vec![Pat::Any.bind_pattern()],
                    continuation: body,
                    persist,
                    peeks: BTreeSet::new(),
                    outcome: ConsumeOutcome::Park,
                    before_observe: None,
                }))
            }
        };
        ops.push(op);
    }
    ops
}

/// Where a run of the chain stops.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Stop {
    /// Every charge passes.
    None,
    /// A charge of a callback fails: the operations before `applied` stay applied.
    Commit { applied: usize },
    /// A charge of the tail fails after the full commit.
    Tail,
}

/// The legacy log and the stop of the host program of `charges` with the budget `budget`: each
/// pre-charge, then its post-charges, under the rule of `RuntimeBudget::charge_legacy`.
fn host_run(charges: &[LegacyCharges], budget: i64) -> (Vec<Cost>, Stop) {
    let mut remaining = budget;
    let mut log = Vec::new();
    let last = charges.len().saturating_sub(1);
    for (index, charge) in charges.iter().enumerate() {
        let entries =
            std::iter::once((true, &charge.pre)).chain(charge.post.iter().map(|c| (false, c)));
        for (is_pre, cost) in entries {
            let stop = match (is_pre, index == last) {
                (true, _) => Stop::Commit { applied: index },
                (false, false) => Stop::Commit { applied: index + 1 },
                (false, true) => Stop::Tail,
            };
            if remaining < 0 {
                return (log, stop);
            }
            remaining -= cost.value;
            log.push(cost.clone());
            if remaining < 0 {
                return (log, stop);
            }
        }
    }
    (log, Stop::None)
}

/// Stages `ops` through a chain over `budget` in one transaction of `handle`, commits it and
/// finishes the chain. Returns where the run stopped.
async fn chain_run(
    handle: &SpaceStoreHandle<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    budget: &RuntimeBudget,
    ops: Vec<StagedOp>,
) -> Stop {
    let mut chain = LegacyChain::new(budget);
    let channels = vec![channel(0), channel(1)];
    let mut txn = handle.begin(&channels).await.expect("a transaction");
    let count = ops.len();
    for op in ops {
        match op {
            StagedOp::Produce(op) => chain.produce(&mut txn, *op).expect("the produce stages"),
            StagedOp::Consume(op) => chain.consume(&mut txn, *op).expect("the consume stages"),
        }
    }
    match txn.commit().await {
        Ok(receipt) => {
            assert_eq!(receipt.ops.len(), count, "a commit applies every operation");
            match chain.finish() {
                Ok(()) => Stop::None,
                Err(error) => {
                    assert_eq!(error, SpaceError::Host(RSpaceError::OutOfPhlogistons));
                    Stop::Tail
                }
            }
        }
        Err(SpaceTxnError::Host(RSpaceError::OutOfPhlogistons)) => Stop::Commit {
            applied: usize::MAX,
        },
        Err(other) => panic!("the commit fails with {other:?}"),
    }
}

/// The IO events of a log: one for each applied operation.
fn applied_operations(log: &[Event]) -> usize {
    log.iter()
        .filter(|event| matches!(event, Event::IoEvent(_)))
        .count()
}

fn arb_program() -> BoxedStrategy<(Vec<Step>, u8)> { (vec(arb_step(), 1..10), 0u8..=100).boxed() }

/// The budget of a case: `percent` of the total positive charges, or ample at 100.
fn budget_of(charges: &[LegacyCharges], percent: u8) -> i64 {
    let total: i64 = charges
        .iter()
        .flat_map(|charge| std::iter::once(&charge.pre).chain(&charge.post))
        .map(|cost| cost.value.max(0))
        .sum();
    match percent {
        100 => total + 1,
        _ => total * i64::from(percent) / 100,
    }
}

/// `legacy_rebracket_eq_host` (Q79): a transaction that stages its operations through the
/// chain makes the legacy log of its host program, and it stops where the host program stops.
/// A stop at a pre-charge or at a carried post-charge fails the commit with the operations
/// before it applied, and a stop at a post-charge of the last operation fails after the commit.
#[test]
fn prop_legacy_chain_log_is_host_order() {
    check(arb_program(), |(steps, percent)| {
        tokio_runtime().block_on(legacy_chain_log_is_host_order_case(steps, percent))
    });
    for (kind, count) in ChainCase::ALL.iter().zip(CHAIN_COVERAGE.iter()) {
        assert!(
            count.load(Ordering::Relaxed) > 0,
            "the generated transactions never made a {kind:?}"
        );
    }
}

/// The kinds of transaction that the chain property must meet at least once over its cases.
#[derive(Clone, Copy, Debug)]
enum ChainCase {
    ConsumeFire,
    ProduceFire,
    NoStop,
    StopBeforeAnyOperation,
    StopAfterSomeOperations,
    StopInTheTail,
}

impl ChainCase {
    const ALL: [ChainCase; 6] = [
        ChainCase::ConsumeFire,
        ChainCase::ProduceFire,
        ChainCase::NoStop,
        ChainCase::StopBeforeAnyOperation,
        ChainCase::StopAfterSomeOperations,
        ChainCase::StopInTheTail,
    ];
}

static CHAIN_COVERAGE: [AtomicUsize; 6] = [const { AtomicUsize::new(0) }; 6];

fn met_case(kind: ChainCase) { CHAIN_COVERAGE[kind as usize].fetch_add(1, Ordering::Relaxed); }

/// Counts the kinds of a transaction and of its stop.
fn met_chain(ops: &[StagedOp], stop: &Stop) {
    for op in ops {
        let fire = match op {
            StagedOp::Produce(op) => match &op.outcome {
                ProduceOutcome::Fire { .. } => Some(ChainCase::ProduceFire),
                ProduceOutcome::Store => None,
            },
            StagedOp::Consume(op) => match &op.outcome {
                ConsumeOutcome::Fire { .. } => Some(ChainCase::ConsumeFire),
                ConsumeOutcome::Park => None,
            },
        };
        if let Some(case) = fire {
            met_case(case);
        }
    }
    met_case(match stop {
        Stop::None => ChainCase::NoStop,
        Stop::Commit { applied: 0 } => ChainCase::StopBeforeAnyOperation,
        Stop::Commit { .. } => ChainCase::StopAfterSomeOperations,
        Stop::Tail => ChainCase::StopInTheTail,
    });
}

async fn legacy_chain_log_is_host_order_case(
    steps: Vec<Step>,
    percent: u8,
) -> Result<(), TestCaseError> {
    let charges: Vec<LegacyCharges> = staged_ops(&steps).iter().map(StagedOp::charges).collect();
    let limit = budget_of(&charges, percent);
    let (expected_log, expected_stop) = host_run(&charges, limit);
    let mut stores = InMemoryStoreManager::new();
    let (play, _replay) = ParRSpace::create_with_replay(
        stores.r_space_stores().await.expect("in-memory stores"),
        Arc::new(Box::new(Matcher)),
    )
    .expect("the play and replay spaces");
    let budget = RuntimeBudget::new(Cost::create(limit, "rrs legacy chain"));
    let _legacy = budget.enter_legacy_scope();
    let stop = chain_run(
        &SpaceStoreHandle::new(play.clone()),
        &budget,
        staged_ops(&steps),
    )
    .await;
    let applied = applied_operations(&play.take_event_log().await);
    let stop = match stop {
        Stop::Commit { .. } => Stop::Commit { applied },
        other => {
            prop_assert_eq!(
                applied,
                steps.len(),
                "a run that commits applies every operation"
            );
            other
        }
    };
    prop_assert_eq!(
        budget.get_log(),
        expected_log,
        "the legacy log of the host program"
    );
    prop_assert_eq!(&stop, &expected_stop, "the stop of the host program");
    met_chain(&staged_ops(&steps), &stop);
    Ok(())
}

/// `legacy_replay_stops_with_play` (Q80): replay stages the operations of play and runs the
/// same callbacks, so with the log of play it makes the legacy log of play, it stops at the
/// stop of play, and it reaches the store of play.
#[test]
fn prop_legacy_replay_stops_with_play() {
    check(arb_program(), |(steps, percent)| {
        tokio_runtime().block_on(legacy_replay_stops_with_play_case(steps, percent))
    });
}

async fn legacy_replay_stops_with_play_case(
    steps: Vec<Step>,
    percent: u8,
) -> Result<(), TestCaseError> {
    let charges: Vec<LegacyCharges> = staged_ops(&steps).iter().map(StagedOp::charges).collect();
    let limit = budget_of(&charges, percent);
    let mut stores = InMemoryStoreManager::new();
    let (play, replay) = ParRSpace::create_with_replay(
        stores.r_space_stores().await.expect("in-memory stores"),
        Arc::new(Box::new(Matcher)),
    )
    .expect("the play and replay spaces");
    let play_budget = RuntimeBudget::new(Cost::create(limit, "rrs legacy chain play"));
    let _play_legacy = play_budget.enter_legacy_scope();
    let play_stop = chain_run(
        &SpaceStoreHandle::new(play.clone()),
        &play_budget,
        staged_ops(&steps),
    )
    .await;
    let log = play.take_event_log().await;
    replay
        .rig(log)
        .await
        .expect("rig the replay space with the play log");
    let replay_budget = RuntimeBudget::new(Cost::create(limit, "rrs legacy chain replay"));
    let _replay_legacy = replay_budget.enter_legacy_scope();
    let replay_stop = chain_run(
        &SpaceStoreHandle::new(replay.clone()),
        &replay_budget,
        staged_ops(&steps),
    )
    .await;
    prop_assert_eq!(
        replay_budget.get_log(),
        play_budget.get_log(),
        "the legacy log of play"
    );
    prop_assert_eq!(replay_stop, play_stop, "the stop of play");
    for index in 0..2 {
        let on = channel(index);
        prop_assert_eq!(
            replay.get_data(&on).await,
            play.get_data(&on).await,
            "the records of play"
        );
        prop_assert_eq!(
            replay.get_waiting_continuations(vec![on.clone()]).await,
            play.get_waiting_continuations(vec![on]).await,
            "the waiters of play"
        );
    }
    prop_assert!(
        replay.check_replay_data().await.is_ok(),
        "replay uses every recorded event"
    );
    Ok(())
}
