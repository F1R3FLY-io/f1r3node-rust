//! The determinism suite of the native spaces (P5 design v2 section 2.6, as P5 design v3 C.1
//! item 7 corrects it): the leaves `rrs-spaces-determinism-canonical-iteration` and
//! `rrs-spaces-determinism-serialization-failure`, for the families that the family agent wires.
//! Wave W2 covered `hashmap:bag`, `hashmap:queue`, `hashmap:stack` and `hashmap:cell`. Wave W3
//! adds these rows:
//!
//! * `hashmap:set` and `hashmap:priorityqueue`,
//! * the bag, stack and cell rows of the Array and Vector stores,
//! * the join groups of P5c: no choice of the join engine depends on the list order of the join
//!   groups or of their waiters,
//! * the ties of a priority query.
//!
//! **Canonical iteration.** The choices of an agent are a function of the entry set of its
//! collection, never of the order in which the hot store lists the entries. The hot store puts
//! each new entry at the front of its list. A checkpoint reloads every list from the history,
//! which keeps the entries sorted by their encoded bytes. Each test builds one entry set in three
//! list orders and runs one probe script on each:
//!
//! 1. The order of the agent operations that built it, newest first.
//! 2. The same entries, stored again by raw host operations in the reverse order.
//! 3. The order of the history, after a checkpoint.
//!
//! The three runs must give the same results, the same events, the same observer calls and the
//! same root. The runs compared the legacy charges of Option P until DR-134 deleted the legacy
//! meter, and they compare the calls of the accounting observer since decision 90 (decision 69). A cell holds at most one entry in each list (F15), so the three stores list a cell
//! in one order. Each test then runs one Rholang program in fresh runtimes, three runs on each
//! racing runtime. Each run certifies its deploys, pays phlo and reaches an outcome that the
//! program allows (N9, decisions 93 and 111).
// Changed by decision 93 (decision 111): each run certifies itself and reaches an allowed outcome.
// The old text follows.
// //! in one order. Each test then runs one Rholang program twice, in two fresh runtimes, and
// //! compares the canonical state (the root), the event log and the cost (N9,
// //! `run_twice_assert_canonical_equal`).
//!
//! Changed by decision 102 (plan v2 pins): play picks a record and a waiter of a bag or a set at
//! random (rule CR-1). The bag and set tests checked the claim on replay, over the rigged replay
//! modes. Changed by decision 90 (C6, step D): those modes retire, so the bag and set rows keep
//! their racing runs, and native replay is certified by `space_native_replay_equal_hashmap` and
//! `space_pick_helpers_put_the_named_candidates_first_in_replay`. The ordered disciplines pick
//! their heads, so their tests keep the comparison of three plays.
//!
//! **Tie-break.** An agent compares the bytes of two candidates only to order a tie. A bag orders
//! records with equal source hashes by their digests (DR-75, decision 41), and an ordered
//! discipline orders two entries with one `ord` by their bincode bytes. The bytes decide in both
//! arrival orders, so a candidate never wins by its arrival order. The leaf
//! `rrs-spaces-determinism-serialization-failure` injected a failure of the tie encode with a
//! host-work cut (decision 42). DR-131 removed host work, so the suite has no injection for that
//! failure, and it tests the tie-break itself.
//!
//! A tie of an ordered discipline needs two entries with one `ord`. The agent never makes them
//! (`HeadOnlyDisciplines.v`, `wf`), so the ordered tests store the tied entries with raw host
//! operations. That is the only state in which the tie-break of the head scan runs.
//!
//! The tests restate these formal statements:
//!
//! | Test | Formal statement |
//! |---|---|
//! | `space_canonical_iteration_bag_views`, `prop_canonical_iteration_bag_any_insertion_order` | `bag_visit_order_canonical`, `waiter_visit_order_canonical` (`DataCollection.v`, `ContinuationCollection.v`), `hashmap_iteration_permutation_invariant` (`HashMapStore.v`) |
//! | `space_canonical_iteration_{queue,stack}_order`, `space_canonical_iteration_cell_views`, `prop_canonical_iteration_ordered_any_list_order` | `queue_head_is_min_ord`, `queue_plan_permutation_invariant` (`HeadOnlyDisciplines.v`), `stack_head_is_max_ord`, `stack_plan_permutation_invariant` (`StackDiscipline.v`), `hashmap_iteration_permutation_invariant` |
//! | `space_candidate_tie_{bag,ordered,set,priority}`, `prop_candidate_tie_bytes_select_in_any_arrival_order` | `bag_visit_order_canonical` (`DataCollection.v`), `queue_head_is_min_ord` (`HeadOnlyDisciplines.v`), `stack_head_is_max_ord` (`StackDiscipline.v`) |

#[path = "support/mod.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicUsize, Ordering};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    BindPattern, CostAuthority, EEq, EVar, Expr, ListParWithRandom, Par, ParWithRandom,
    TaggedContinuation, Var,
};
use models::rust::space_factory_urn::{SpaceFactoryUrn, SpaceInner, SpaceOuter};
use proptest::arbitrary::any as arbitrary;
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::sample::select;
use proptest::test_runner::TestCaseError;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::RuntimeBudget;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::spaces::agent::{
    AllocOp, Allocation, ConsumeOp, ProduceOp, QueryParams, SpaceAgent, SpaceEnv, SpaceOp,
    SpaceOutcome,
};
use rholang::rust::interpreter::spaces::disciplines::priority_queue::PRIORITY_PARAM;
use rholang::rust::interpreter::spaces::errors::SpaceError;
use rholang::rust::interpreter::spaces::families::FamilyAgent;
use rholang::rust::interpreter::spaces::generic::{ParDatum, ParWaiter};
use rholang::rust::interpreter::spaces::namespace::{self, Durability, NativeSpace, SID_LEN};
use rholang::rust::interpreter::spaces::outer::array::SIZE_PARAM;
use rholang::rust::interpreter::spaces::types::config::SpaceConfig;
use rholang::rust::interpreter::spaces::types::qualifier::SpaceQualifier;
use rholang::rust::interpreter::spaces::{allocator, record};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::trace::event::Event;
use support::agent::{
    any, budget, config_of, counter_channel, create, creation, data_channel, env_over, exactly,
    int, play, receive_on, run, send_on, space_counter_channel, string, ParRSpace,
};
use support::observed::{canonical_join_order, observed, unobserve, Call};
use support::props::{
    check, pure_config, racing_runtime, tokio_runtime, RACING_RUNS, RACING_WORKERS,
};
use support::session::{native, setup_source, with_space, Session, Space};

// ─── The disciplines and the space ───────────────────────────────────────────────────────────

/// A collection discipline of this wave whose agent tests run here, on the HashMap store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// `hashmap:bag`: the scan driver, with no counter.
    Bag,
    /// `hashmap:queue`: the head driver, the least `ord` first.
    Queue,
    /// `hashmap:stack`: the head driver, the greatest `ord` first, with the settle.
    Stack,
    /// `hashmap:cell`: the head driver, one record and one waiter.
    Cell,
    /// `hashmap:set`: the scan driver over classes, with no counter (wave W3).
    Set,
    /// `hashmap:priorityqueue`: the lane driver, the least `ord` first in each level (wave W3).
    Pq,
}

impl Kind {
    /// The disciplines that order their entries by the `ord` of the envelope.
    const ORDERED: [Kind; 3] = [Kind::Queue, Kind::Stack, Kind::Cell];

    /// The inner collection of the discipline.
    fn inner(self) -> SpaceInner {
        match self {
            Kind::Bag => SpaceInner::Bag,
            Kind::Queue => SpaceInner::Queue,
            Kind::Stack => SpaceInner::Stack,
            Kind::Cell => SpaceInner::Cell,
            Kind::Set => SpaceInner::Set,
            Kind::Pq => SpaceInner::PriorityQueue,
        }
    }

    /// The family `hashmap:<inner>` of the discipline.
    fn family(self) -> SpaceFactoryUrn {
        SpaceFactoryUrn {
            outer: SpaceOuter::HashMap,
            inner: self.inner(),
        }
    }
}

/// The space of every agent test.
const SPACE: NativeSpace = NativeSpace {
    sid: [0x3D; SID_LEN],
    durability: Durability::Durable,
};

/// The phlo of the budget of a probe: far above every script of the suite. It was the phlo of a
/// legacy scope until DR-134 deleted the legacy meter (decision 90, decision 69).
const PROBE_PHLO: i64 = 1_000_000_000_000;

/// The inner channel of the first collection of a canonical-iteration rig.
fn left() -> Par { string("rrs-det-left") }

/// The inner channel of the second collection of a canonical-iteration rig.
fn right() -> Par { string("rrs-det-right") }

/// The inner channel of the collection whose two candidates tie.
fn tied() -> Par { string("rrs-det-tied") }

/// The inner channel of the third collection of the join-group rig.
fn context() -> Par { string("rrs-det-context") }

/// The channels of the space and of the collections `collections`: `C(sid)`, `S(sid)` and
/// `H(sid)`, then `S(sid,k)` and `D(sid,k)` of each collection. In an agent rig they hold the
/// whole state of the store.
fn space_channels(collections: &[Par]) -> Vec<Par> {
    let mut channels = Vec::with_capacity(3 + 2 * collections.len());
    channels.push(namespace::gprivate(namespace::config_channel(SPACE)));
    channels.push(space_counter_channel(SPACE));
    channels.push(namespace::gprivate(namespace::handle_channel(SPACE)));
    for collection in collections {
        channels.push(counter_channel(SPACE, collection));
        channels.push(data_channel(SPACE, collection));
    }
    channels
}

/// Creates the space of the discipline `kind` in `host` with the family agent, and returns the
/// environment of a job over `host` that charges `budget`.
async fn create_space(kind: Kind, host: &ParRSpace, budget: &RuntimeBudget) -> SpaceEnv {
    let env = env_over(host.clone(), budget);
    let config = config_of(
        SPACE,
        SpaceOuter::HashMap.as_str(),
        kind.inner().as_str(),
        SpaceQualifier::Default,
    );
    create(&FamilyAgent, &env, creation(SPACE, config, kind.family())).await;
    host.take_event_log().await;
    env
}

/// The outer store of a canonical-iteration rig (wave W3 adds the Array and Vector rows).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Store {
    /// The HashMap store: any channel names a collection.
    HashMap,
    /// A fixed Array store of four index names.
    Array,
    /// A Vector store.
    Vector,
}

impl Store {
    /// The outer store.
    fn outer(self) -> SpaceOuter {
        match self {
            Store::HashMap => SpaceOuter::HashMap,
            Store::Array => SpaceOuter::Array,
            Store::Vector => SpaceOuter::Vector,
        }
    }

    /// The family `<store>:<kind>`.
    fn family(self, kind: Kind) -> SpaceFactoryUrn {
        SpaceFactoryUrn {
            outer: self.outer(),
            inner: kind.inner(),
        }
    }

    /// The config map of [`SPACE`] for the discipline `kind`. An Array space has four index
    /// names.
    fn config(self, kind: Kind) -> Par {
        let mut config = SpaceConfig::new(
            SPACE.sid,
            self.outer().as_str(),
            kind.inner().as_str(),
            SpaceQualifier::Default,
        );
        if self == Store::Array {
            config.params.insert(SIZE_PARAM.to_string(), int(4));
        }
        config.to_par()
    }

    /// The constructor arguments of the factory, as Rholang source.
    fn ctor(self) -> &'static str {
        match self {
            Store::HashMap | Store::Vector => "\"default\"",
            Store::Array => "\"default\"; size=4",
        }
    }

    /// The inner channels of the two collections of a rig. They are two strings on a HashMap
    /// space, and the index names 0 and 1 on an Array or a Vector space.
    fn collections(self) -> [Par; 2] {
        match self {
            Store::HashMap => [left(), right()],
            Store::Array | Store::Vector => [
                allocator::index_name(SPACE, 0),
                allocator::index_name(SPACE, 1),
            ],
        }
    }

    /// The channels that hold the whole state of a rig: [`space_channels`] of the collections,
    /// and the allocator `A(sid)` of an Array or a Vector space.
    fn channels(self) -> Vec<Par> {
        let mut channels = space_channels(&self.collections());
        if self != Store::HashMap {
            channels.push(allocator::allocator_channel(SPACE));
        }
        channels
    }
}

/// Creates the space of `store` and `kind` in `host` with the family agent. On an Array or a
/// Vector space, it allocates the index names 0 and 1. It returns the environment of a job over
/// `host` that charges `budget`.
async fn create_space_in(
    store: Store,
    kind: Kind,
    host: &ParRSpace,
    budget: &RuntimeBudget,
) -> SpaceEnv {
    if store == Store::HashMap {
        return create_space(kind, host, budget).await;
    }
    let env = env_over(host.clone(), budget);
    create(
        &FamilyAgent,
        &env,
        creation(SPACE, store.config(kind), store.family(kind)),
    )
    .await;
    let op = AllocOp {
        space: SPACE,
        count: 2,
        random_state: Blake2b512Random::create_from_bytes(b"rrs-det-allocation").to_bytes(),
        authority: CostAuthority::default(),
    };
    match FamilyAgent
        .allocate(&env, op)
        .await
        .expect("the allocation of two index names runs")
    {
        Allocation::Indices(prepared) => {
            let indices = prepared.commit().await.expect("the allocation commits");
            assert_eq!(
                indices,
                vec![0, 1],
                "the space hands out the index names 0 and 1"
            );
        }
        Allocation::Baseline => panic!("an Array or a Vector space has an allocator"),
    }
    host.take_event_log().await;
    env
}

// ─── Operations ──────────────────────────────────────────────────────────────────────────────

/// A send of the payload `pars` on `channel`, with a random state that the text `salt` fixes.
fn send_pars(channel: &Par, pars: Vec<Par>, salt: &str, persistent: bool) -> SpaceOp {
    let mut op = send_on(SPACE, channel, 0, persistent);
    op.data.pars = pars;
    op.data.random_state =
        Blake2b512Random::create_from_bytes(format!("rrs-det-send {salt}").as_bytes()).to_bytes();
    SpaceOp::Produce(op)
}

/// A send of the number `value` on `channel` with the salt `salt`.
fn send(channel: &Par, value: i64, salt: u64) -> SpaceOp {
    send_pars(channel, vec![int(value)], &format!("{value} {salt}"), false)
}

/// A persistent send of the number `value` on `channel` with the salt `salt`.
fn send_forever(channel: &Par, value: i64, salt: u64) -> SpaceOp {
    send_pars(
        channel,
        vec![int(value)],
        &format!("{value} {salt} !!"),
        true,
    )
}

/// A receive on `channel` whose body is the string `name`.
fn receive(channel: &Par, name: &str, pattern: BindPattern) -> SpaceOp {
    SpaceOp::Consume(receive_on(SPACE, channel, name, pattern))
}

/// The named params of a send or of a query with the level `level` of a priority queue.
fn priority(level: i64) -> BTreeMap<String, Par> {
    BTreeMap::from([(PRIORITY_PARAM.to_string(), int(level))])
}

/// A send of `value` on `channel` of a priority queue at the level `level`, with the salt `salt`.
fn send_at(channel: &Par, value: i64, salt: u64, level: i64) -> SpaceOp {
    match send(channel, value, salt) {
        SpaceOp::Produce(op) => SpaceOp::Produce(ProduceOp {
            params: priority(level),
            ..op
        }),
        other => other,
    }
}

/// A receive on `channel` of a priority queue in the lane of the level `level`.
fn receive_in(channel: &Par, name: &str, pattern: BindPattern, level: i64) -> SpaceOp {
    SpaceOp::Consume(ConsumeOp {
        query: Some(QueryParams {
            payload: Vec::new(),
            named: priority(level),
        }),
        ..receive_on(SPACE, channel, name, pattern)
    })
}

/// `op`, a receive, as a peek (`<<-`).
fn peeking(op: SpaceOp) -> SpaceOp {
    match op {
        SpaceOp::Consume(receive) => SpaceOp::Consume(ConsumeOp {
            peek: true,
            ..receive
        }),
        other => other,
    }
}

/// `op`, a receive, as a persistent receive (`<=`).
fn persisting(op: SpaceOp) -> SpaceOp {
    match op {
        SpaceOp::Consume(receive) => SpaceOp::Consume(ConsumeOp {
            persistent: true,
            ..receive
        }),
        other => other,
    }
}

/// `op`, a receive, with the guard `guard`.
fn guarded(op: SpaceOp, guard: Par) -> SpaceOp {
    match op {
        SpaceOp::Consume(receive) => SpaceOp::Consume(ConsumeOp {
            guard: Some(guard),
            ..receive
        }),
        other => other,
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

/// Runs `ops` in order with the family agent and the environment `env`. An operation that fails
/// applies nothing, so the state is the state of the committed operations.
async fn run_all(env: &SpaceEnv, ops: &[SpaceOp]) -> Vec<Result<SpaceOutcome, SpaceError>> {
    let mut results = Vec::with_capacity(ops.len());
    for op in ops {
        results.push(run(&FamilyAgent, env, op.clone()).await);
    }
    results
}

// ─── Snapshots of the store ──────────────────────────────────────────────────────────────────

/// The rows of one channel in list order: its records, its waiting continuations and its joins.
#[derive(Clone, Debug, PartialEq)]
struct Rows {
    /// The records of the channel.
    data: Vec<ParDatum>,
    /// The waiting continuations whose channel group is the channel alone.
    waiters: Vec<ParWaiter>,
    /// The join groups of the channel.
    joins: Vec<Vec<Par>>,
}

/// The rows of `channels` in `space`, each in the list order of the hot store.
async fn rows_of(
    space: &impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    channels: &[Par],
) -> Vec<Rows> {
    let mut rows = Vec::with_capacity(channels.len());
    for channel in channels {
        rows.push(Rows {
            data: space.get_data(channel).await,
            waiters: space.get_waiting_continuations(vec![channel.clone()]).await,
            joins: space.get_joins(channel.clone()).await,
        });
    }
    rows
}

/// The bincode bytes of each value of `values`, in list order.
fn encoded<T: serde::Serialize>(values: &[T]) -> Vec<Vec<u8>> {
    values
        .iter()
        .map(|value| bincode::serialize(value).expect("a store entry serializes"))
        .collect()
}

/// The entries of `rows` in list order: for each channel, the bytes of its records, of its
/// waiting continuations and of its joins.
fn list_order(rows: &[Rows]) -> Vec<[Vec<Vec<u8>>; 3]> {
    rows.iter()
        .map(|row| {
            [
                encoded(&row.data),
                encoded(&row.waiters),
                encoded(&row.joins),
            ]
        })
        .collect()
}

/// The entry set of `rows`: [`list_order`] with each list sorted. Two snapshots with one entry
/// set hold the same entries with the same multiplicities, whatever their list orders.
fn entry_set(rows: &[Rows]) -> Vec<[Vec<Vec<u8>>; 3]> {
    list_order(rows)
        .into_iter()
        .map(|lists| {
            lists.map(|mut list| {
                list.sort();
                list
            })
        })
        .collect()
}

/// Stores the entries of `rows`, the rows of `channels`, in the empty store `target` with raw
/// host operations. `order(n)` is the insertion order of a list of `n` entries. No parked waiter
/// of a space takes a raw record, because a record holds its envelope before its payload and a
/// waiter of a collection binds one value. So each raw produce stores its record, and each raw
/// consume parks its waiter. The hot store puts each entry at the front of its list.
///
/// Changed by decision 102 (plan v2 pins): `target` can be a replay space with no rigged log. A
/// raw operation on such a space stores its entry too, so a replay space can hold a list order.
// Changed by decision 102 (plan v2 pins): the target is any space, so that a replay space holds
// a list order. The old lines follow.
// async fn store_raw(
//     target: &ParRSpace,
//     channels: &[Par],
async fn store_raw(
    target: &impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    channels: &[Par],
    rows: &[Rows],
    order: &dyn Fn(usize) -> Vec<usize>,
) {
    for (channel, row) in channels.iter().zip(rows) {
        for index in order(row.data.len()) {
            let datum = &row.data[index];
            let fired = target
                .produce(channel.clone(), datum.a.clone(), datum.persist)
                .await
                .expect("a raw produce runs");
            assert!(fired.is_none(), "a raw produce of a record fires nothing");
        }
        for index in order(row.waiters.len()) {
            let waiter = &row.waiters[index];
            let fired = target
                .consume(
                    vec![channel.clone()],
                    waiter.patterns.clone(),
                    waiter.continuation.clone(),
                    waiter.persist,
                    waiter.peeks.clone(),
                )
                .await
                .expect("a raw consume runs");
            assert!(fired.is_none(), "a raw consume of a waiter fires nothing");
        }
    }
    target.take_event_log().await;
}

/// The insertion order that reverses the list order of `rows`.
fn reverse_order(length: usize) -> Vec<usize> { (0..length).collect() }

// ─── Canonical iteration ─────────────────────────────────────────────────────────────────────

/// What one operation of a probe script shows: its result, its events and its observer calls.
#[derive(Clone, Debug, PartialEq)]
struct Observed {
    /// The outcome of the operation, or its error.
    result: Result<SpaceOutcome, SpaceError>,
    /// The events of the operation, in log order.
    events: Vec<Event>,
    // Changed by decision 90 (decision 69): a step shows its observer calls, because DR-134
    // deleted the legacy meter.
    // /// The legacy charges of the operation (decision 45, Option P).
    // charges: Vec<Cost>,
    /// The calls of the accounting observer of the host in the operation, in call order, with
    /// the join groups of each start in canonical order (`canonical_join_order`). The list order of
    /// the join groups is the list order of the store, which the runs change on purpose, and the
    /// accounting observer reads the join groups as a set.
    calls: Vec<Call>,
}

/// One run of a probe script: each step, and the root after the last step.
#[derive(Clone, Debug, PartialEq)]
struct Run {
    /// The steps, in script order.
    steps: Vec<Observed>,
    /// The root of the checkpoint after the last step.
    root: Blake2b256Hash,
}

/// Runs `script` over `host` with a recorder as the accounting observer of `host`, and
/// checkpoints at the end. Each step shows the observer calls of its operation (decision 90,
/// decision 69). The run removes the observer before the checkpoint.
// Changed by decision 90 (decision 69): a step records the observer calls of the host instead of
// the legacy charges of a legacy scope (decision 45, Option P), because DR-134 deleted the legacy
// meter.
// async fn observe(host: &ParRSpace, script: &[SpaceOp]) -> Run {
//     let budget = RuntimeBudget::new(Cost::create(LEGACY_PHLO, "rrs determinism probe"));
//     let _scope = budget.enter_legacy_scope();
//     host.take_event_log().await;
//     let mut steps = Vec::with_capacity(script.len());
//     for op in script {
//         let env = env_over(host.clone(), &budget);
//         let before = budget.get_log().len();
//         let result = run(&FamilyAgent, &env, op.clone()).await;
//         steps.push(Observed {
//             result,
//             events: host.take_event_log().await,
//             charges: budget.get_log()[before..].to_vec(),
//         });
//     }
async fn observe(host: &ParRSpace, script: &[SpaceOp]) -> Run {
    let budget = RuntimeBudget::new(Cost::create(PROBE_PHLO, "rrs determinism probe"));
    let recorder = observed(host);
    host.take_event_log().await;
    let mut steps = Vec::with_capacity(script.len());
    for op in script {
        let env = env_over(host.clone(), &budget);
        let before = recorder.count();
        let result = run(&FamilyAgent, &env, op.clone()).await;
        steps.push(Observed {
            result,
            events: host.take_event_log().await,
            calls: canonical_join_order(&recorder.since(before)),
        });
    }
    unobserve(host);
    let root = host
        .create_checkpoint()
        .await
        .expect("a checkpoint after the probe")
        .root;
    Run { steps, root }
}

/// Builds the space of `kind` in a fresh store and runs the agent operations `build` in order.
async fn built(kind: Kind, build: &[SpaceOp]) -> ParRSpace {
    built_in(Store::HashMap, kind, build).await
}

/// Builds the space of `store` and `kind` in a fresh store and runs the agent operations `build`
/// in order.
async fn built_in(store: Store, kind: Kind, build: &[SpaceOp]) -> ParRSpace {
    let host = play().await;
    let env = create_space_in(store, kind, &host, &budget()).await;
    run_all(&env, build).await;
    host.take_event_log().await;
    host
}

/// The three runs of one entry set: in the order of the agent operations, stored again in the
/// reverse order, and in the order of the history after a checkpoint.
struct ListOrders {
    /// The run on the store that the agent built.
    agent: Run,
    /// The run on the raw store in the reverse order.
    reversed: Run,
    /// The run on the store that the history reloaded.
    history: Run,
}

/// Builds the entry set of `build` for `kind` in the three list orders of this suite, checks that
/// the entry sets are equal, and runs `probe` on each.
///
/// A list of two or more distinct entries has more than one order, and then the raw store and
/// the history must list it in another order than the agent: the check is not vacuous. A cell
/// holds at most one entry in each list (F15), so its lists have one order only, and the three
/// stores list the cell in the same order.
async fn list_orders(kind: Kind, build: &[SpaceOp], probe: &[SpaceOp]) -> ListOrders {
    list_orders_in(Store::HashMap, kind, build, probe).await
}

/// [`list_orders`] on the space of `store` and `kind`.
async fn list_orders_in(
    store: Store,
    kind: Kind,
    build: &[SpaceOp],
    probe: &[SpaceOp],
) -> ListOrders {
    let channels = store.channels();
    let agent_host = built_in(store, kind, build).await;
    let agent_rows = rows_of(&agent_host, &channels).await;
    let reorderable = agent_rows
        .iter()
        .any(|row| row.data.len() >= 2 || row.waiters.len() >= 2);
    assert_eq!(
        reorderable,
        kind != Kind::Cell,
        "{kind:?}: a list of two entries exists exactly when the discipline allows it"
    );

    let reversed_host = play().await;
    store_raw(&reversed_host, &channels, &agent_rows, &reverse_order).await;
    let reversed_rows = rows_of(&reversed_host, &channels).await;
    assert_eq!(
        entry_set(&reversed_rows),
        entry_set(&agent_rows),
        "{kind:?}: the raw store holds the entries of the agent"
    );
    assert_eq!(
        list_order(&reversed_rows) != list_order(&agent_rows),
        reorderable,
        "{kind:?}: the raw store lists the entries in another order"
    );

    let history_host = built_in(store, kind, build).await;
    history_host
        .create_checkpoint()
        .await
        .expect("a checkpoint of the entry set");
    let history_rows = rows_of(&history_host, &channels).await;
    assert_eq!(
        entry_set(&history_rows),
        entry_set(&agent_rows),
        "{kind:?}: the history holds the entries of the agent"
    );
    assert_eq!(
        list_order(&history_rows) != list_order(&agent_rows),
        reorderable,
        "{kind:?}: the history lists the entries in another order"
    );

    ListOrders {
        agent: observe(&agent_host, probe).await,
        reversed: observe(&reversed_host, probe).await,
        history: observe(&history_host, probe).await,
    }
}

/// Asserts that the three runs of `orders` give the same results, events, observer calls and
/// roots.
fn assert_same_runs(kind: Kind, orders: &ListOrders) {
    let agent = &orders.agent.steps;
    assert!(
        orders.agent.steps.iter().any(|step| step
            .result
            .as_ref()
            .is_ok_and(|outcome| !outcome.fires.is_empty())),
        "{kind:?}: the probe fires"
    );
    for (label, other) in [("reversed", &orders.reversed), ("history", &orders.history)] {
        assert_eq!(
            &other.steps, agent,
            "{kind:?}: the {label} list order changes a step"
        );
        assert_eq!(
            other.root, orders.agent.root,
            "{kind:?}: the {label} list order changes the root"
        );
    }
}

/// The entry set and the probe script of the canonical-iteration test of `kind`.
///
/// * Bag: five records on the left collection (two of them hold 1), a waiter for 7 and a waiter
///   whose guard admits only 9 on the left collection, and three waiters on the right
///   collection. No send fires, because no waiter takes a stored value. So the order of the
///   build operations does not change the entry set. The probe takes a record in canonical
///   order, fires the guarded waiter, peeks, drains the records of 1 and fires the waiters of the
///   right collection.
/// * Queue and stack: three records on the left collection and three waiters on the right
///   collection. The probe takes the head record, peeks, and sends 7, 6 and 5 on the right
///   collection. A queue fires its head waiter and then blocks. A stack stores 7, fires the
///   waiters for 6 and 5, and then settles the buried pair of 7 and the waiter of any value.
/// * Cell: a record and a waiter that does not take it on the left collection, and a waiter on
///   the right collection. The probe fires both waiters, peeks, takes the record, and fills the
///   cell, so the last send fails with `CellOccupied`.
fn script_of(kind: Kind) -> (Vec<SpaceOp>, Vec<SpaceOp>) { script_on(kind, &left(), &right()) }

/// [`script_of`] on the collections of `a` and `b`.
///
/// * Set: the left collection holds five records (two members of the class of 1) and a waiter
///   for 7. The right collection holds three waiters. No send fires. The probe takes the class of
///   1 with one class fire, fires the waiter for 7, peeks, and fires the waiters of the right
///   collection.
/// * Priority queue: the left collection holds three records at the levels 1, 0 and none. The
///   right collection holds three waiters: one of any value, one for 5 in the lane 0 and one for
///   6 in the lane 1. The probe takes the least level and peeks in the lane 1. It sends 7, 6 and 5
///   at the levels 0, 1 and 0, and the oldest lane head fires first.
fn script_on(kind: Kind, a: &Par, b: &Par) -> (Vec<SpaceOp>, Vec<SpaceOp>) {
    let (a, b) = (a.clone(), b.clone());
    match kind {
        Kind::Bag => (
            vec![
                send(&a, 5, 1),
                send(&a, 1, 1),
                send(&a, 4, 1),
                send(&a, 1, 2),
                send(&a, 3, 1),
                receive(&a, "w7", exactly(7)),
                guarded(receive(&a, "g9", any()), equals(9)),
                receive(&b, "x", exactly(2)),
                receive(&b, "y", any()),
                receive(&b, "z", exactly(3)),
            ],
            vec![
                receive(&a, "p1", any()),
                send(&a, 9, 1),
                peeking(receive(&a, "p3", exactly(1))),
                persisting(receive(&a, "p4", exactly(1))),
                send(&a, 1, 3),
                send(&b, 3, 1),
                send(&b, 2, 1),
                send(&b, 6, 1),
            ],
        ),
        Kind::Queue | Kind::Stack => (
            vec![
                send(&a, 1, 1),
                send(&a, 2, 1),
                send(&a, 3, 1),
                receive(&b, "w1", any()),
                receive(&b, "w5", exactly(5)),
                receive(&b, "w6", exactly(6)),
            ],
            vec![
                receive(&a, "p1", any()),
                peeking(receive(&a, "p2", any())),
                send(&b, 7, 1),
                send(&b, 6, 1),
                send(&b, 5, 1),
                receive(&a, "p7", exactly(9)),
                send(&a, 9, 1),
            ],
        ),
        Kind::Cell => (
            vec![
                send(&a, 1, 1),
                receive(&a, "w2", exactly(2)),
                receive(&b, "w", any()),
            ],
            vec![
                send(&b, 3, 1),
                send(&a, 2, 1),
                peeking(receive(&a, "p3", any())),
                receive(&a, "p4", any()),
                send(&a, 4, 1),
                send(&a, 5, 1),
            ],
        ),
        Kind::Set => (
            vec![
                send(&a, 5, 1),
                send(&a, 1, 1),
                send(&a, 4, 1),
                send(&a, 1, 2),
                send(&a, 3, 1),
                receive(&a, "w7", exactly(7)),
                receive(&b, "x", exactly(2)),
                receive(&b, "y", any()),
                receive(&b, "z", exactly(3)),
            ],
            vec![
                receive(&a, "p1", exactly(1)),
                send(&a, 7, 1),
                peeking(receive(&a, "p3", exactly(4))),
                persisting(receive(&a, "p4", exactly(5))),
                send(&a, 5, 3),
                send(&b, 3, 1),
                send(&b, 2, 1),
                send(&b, 6, 1),
            ],
        ),
        Kind::Pq => (
            vec![
                send_at(&a, 1, 1, 1),
                send_at(&a, 2, 1, 0),
                send(&a, 3, 1),
                receive(&b, "w1", any()),
                receive_in(&b, "w5", exactly(5), 0),
                receive_in(&b, "w6", exactly(6), 1),
            ],
            vec![
                receive(&a, "p1", any()),
                peeking(receive_in(&a, "p2", any(), 1)),
                send_at(&b, 7, 1, 0),
                send_at(&b, 6, 1, 1),
                send_at(&b, 5, 1, 0),
                receive(&a, "p7", exactly(9)),
                send(&a, 9, 1),
            ],
        ),
    }
}

/// The canonical-iteration check of an ordered discipline: three list orders, the same steps and
/// the same root.
async fn check_ordered_list_orders(kind: Kind) {
    let (build, probe) = script_of(kind);
    let orders = list_orders(kind, &build, &probe).await;
    assert_same_runs(kind, &orders);
}

// Retired by decision 90 (C6, step D): the replay-order checks of the bag and set rows ran through
// the rigged replay modes, which retire (decision 125). `space_native_replay_equal_hashmap`, the
// pick helpers tests of `spaces/generic.rs`, the racing runs of this file.

// ─── One program twice, in fresh runtimes ────────────────────────────────────────────────────

/// What one fresh runtime shows for the setup deploy and the program deploy of a family: the
/// events and the cost of each deploy, the outputs of the program, the rows of the two names
/// and the root after a checkpoint.
#[derive(Clone, Debug, PartialEq)]
struct RuntimeRun {
    /// The events of the setup deploy.
    setup_events: Vec<Event>,
    /// The cost of the setup deploy.
    setup_cost: Cost,
    /// The events of the program deploy.
    events: Vec<Event>,
    /// The cost of the program deploy.
    cost: Cost,
    /// Added by decision 93 (decision 111): the native phlo usage of the program deploy.
    phlo: Option<u64>,
    /// The data on the output channel.
    outputs: Vec<Vec<Par>>,
    /// The records of the names `a` and `b`: `(ord, payload, persistent)`.
    records: [Vec<(i64, Vec<Par>, bool)>; 2],
    /// The parked continuations of the names `a` and `b`: `(ord, persistent, peek)`.
    parked: [Vec<(i64, bool, bool)>; 2],
    /// The root of the checkpoint after the program.
    root: Blake2b256Hash,
}

/// The output channel of a determinism program.
const OUTPUT: &str = "rrs-det-out";

/// The records of the collection of `name` in `session`, in `ord` order: `(ord, payload,
/// persistent)`. The decoder of the priority queue reads the records of every discipline.
async fn records_any(
    session: &Session,
    space: NativeSpace,
    name: &Par,
) -> Vec<(i64, Vec<Par>, bool)> {
    let mut records: Vec<(i64, Vec<Par>, bool)> = session
        .data(&support::session::data_channel(space, name))
        .await
        .into_iter()
        .map(|datum| {
            let decoded = record::unwrap_pq_datum(&datum.a).expect("a datum record decodes");
            (decoded.ord, decoded.datum.pars, datum.persist)
        })
        .collect();
    records.sort_by(|left, right| {
        (left.0, format!("{:?}", left.1)).cmp(&(right.0, format!("{:?}", right.1)))
    });
    records
}

/// The parked continuations of the collection of `name` in `session`, in `ord` order: `(ord,
/// persistent, peek)`. The decoder of the priority queue reads the waiters of every discipline.
async fn parked_any(session: &Session, space: NativeSpace, name: &Par) -> Vec<(i64, bool, bool)> {
    let mut parked: Vec<(i64, bool, bool)> = session
        .waiters(&support::session::data_channel(space, name))
        .await
        .into_iter()
        .map(|waiter| {
            let decoded = record::unwrap_pq_continuation(&waiter.continuation)
                .expect("a parked continuation decodes");
            (decoded.ord, waiter.persist, !waiter.peeks.is_empty())
        })
        .collect();
    parked.sort();
    parked
}

/// Runs the setup of `store` and `kind` and then `program` in a fresh runtime.
async fn fresh_run_in(store: Store, kind: Kind, program: &str) -> RuntimeRun {
    let mut session = Session::new().await;
    let urn = store.family(kind).to_string();
    let setup = session
        .run(
            &setup_source(&urn, store.ctor()),
            &format!("rrs-det-setup {urn}"),
        )
        .await;
    let space = native(&session.sole("rrs-handle").await);
    let names = match <[Vec<Par>; 1]>::try_from(session.values("rrs-names").await) {
        Ok([names]) => names,
        Err(data) => panic!("@\"rrs-names\" holds {} data", data.len()),
    };
    let [a, b] = <[Par; 2]>::try_from(names).expect("the setup makes two names");
    let space = Space { space, a, b };
    let deploy = session
        .run(&with_space(program), &format!("rrs-det-program {urn}"))
        .await;
    let records = [
        records_any(&session, space.space, &space.a).await,
        records_any(&session, space.space, &space.b).await,
    ];
    let parked = [
        parked_any(&session, space.space, &space.a).await,
        parked_any(&session, space.space, &space.b).await,
    ];
    let outputs = session.values(OUTPUT).await;
    let root = session.play.create_checkpoint().await.root;
    RuntimeRun {
        setup_events: setup.log,
        setup_cost: setup.result.cost,
        events: deploy.log,
        cost: deploy.result.cost,
        phlo: deploy.result.native_phlo_usage,
        outputs,
        records,
        parked,
        root,
    }
}

/// The Rholang program of the runtime check of `kind`, over the names `a` and `b` of the setup.
fn program_of(kind: Kind) -> &'static str {
    match kind {
        Kind::Bag => {
            r#"a!(3) | a!(1) | a!(2) | a!(1) | b!!(7) |
for (@x <- a) { @"rrs-det-out"!(["take", x]) } |
for (@1 <- a) { @"rrs-det-out"!(["one", 1]) } |
for (@y <<- a) { @"rrs-det-out"!(["peek", y]) } |
for (@z <- b) { @"rrs-det-out"!(["persistent", z]) } |
for (@w <<- b) { @"rrs-det-out"!(["peek persistent", w]) }"#
        }
        Kind::Queue | Kind::Stack => {
            r#"a!(1) | a!(2) | a!(3) |
for (@x <- a) { @"rrs-det-out"!(["take", x]) } |
for (@y <<- a) { @"rrs-det-out"!(["peek", y]) } |
for (@2 <- b) { @"rrs-det-out"!(["two", 2]) } | b!(1) | b!(2)"#
        }
        Kind::Cell => {
            r#"a!(1) | for (@x <- a) { @"rrs-det-out"!(["take", x]) | a!(x + 1) } |
for (@y <<- b) { @"rrs-det-out"!(["peek", y]) } | b!(5)"#
        }
        Kind::Set => {
            r#"a!(3) | a!(1) | a!(2) | a!(1) | b!!(7) |
for (@1 <- a) { @"rrs-det-out"!(["class", 1]) } |
for (@x <- a) { @"rrs-det-out"!(["take", x]) } |
for (@y <<- a) { @"rrs-det-out"!(["peek", y]) } |
for (@z <- b) { @"rrs-det-out"!(["persistent", z]) }"#
        }
        Kind::Pq => {
            r#"a!(1; priority=1) | a!(2; priority=0) | a!(3) |
for (@x <- a) { @"rrs-det-out"!(["take", x]) } |
for (@y <<- a) { @"rrs-det-out"!(["peek", y]) } |
for (@z <- a ~ (; priority=1)) { @"rrs-det-out"!(["lane", z]) } |
for (@2 <- b ~ (; priority=0)) { @"rrs-det-out"!(["two", 2]) } | b!(1) | b!(2; priority=0)"#
        }
    }
}

// Changed by decision 93 (decision 111): two fresh runtimes give other runs under racing
// evaluation, and native execution has no legacy cost. `check_racing_runs` replaces them. The old
// helpers follow.
//
// /// Runs the program of `kind` twice, in two fresh runtimes, and compares the root, the event
// /// logs and the costs (N9, `run_twice_assert_canonical_equal`).
// async fn check_runs_twice(kind: Kind) { check_runs_twice_in(Store::HashMap, kind).await }
//
// /// [`check_runs_twice`] on the space of `store` and `kind`.
// async fn check_runs_twice_in(store: Store, kind: Kind) {
//     let program = program_of(kind);
//     let first = fresh_run_in(store, kind, program).await;
//     let second = fresh_run_in(store, kind, program).await;
//     assert!(!first.events.is_empty(), "{kind:?}: the program has events");
//     assert!(!first.outputs.is_empty(), "{kind:?}: the program fires");
//     assert!(first.cost.value > 0, "{kind:?}: the program costs phlo");
//     assert_eq!(
//         second, first,
//         "{kind:?}: two fresh runtimes give the same run"
//     );
// }

// ─── One program on each racing runtime (decisions 93 and 111) ──────────────────────────────

/// Added by decision 93 (decision 111): what one run of the program of a kind delivers and
/// leaves, without the orders of racing evaluation. The deliveries are the pairs `(label,
/// value)` in bytewise order. The records are the pairs `(value, persistent)` of the names `a`
/// and `b`, in `ord` order. The parked continuations are the pairs `(persistent, peek)`.
#[derive(Debug, PartialEq, Eq)]
struct RunOutcome {
    deliveries: Vec<(String, i64)>,
    records: [Vec<(i64, bool)>; 2],
    parked: [Vec<(bool, bool)>; 2],
}

/// Added by decision 93 (decision 111): the integer of `par`.
fn int_of(par: &Par) -> i64 {
    match par.exprs.as_slice() {
        [Expr {
            expr_instance: Some(ExprInstance::GInt(value)),
        }] => *value,
        other => panic!("an integer: {other:?}"),
    }
}

/// Added by decision 93 (decision 111): the delivery `[label, value]` of the output `datum`.
fn delivery(datum: &[Par]) -> (String, i64) {
    let [pair] = datum else {
        panic!("an output of one value: {datum:?}")
    };
    match pair.exprs.as_slice() {
        [Expr {
            expr_instance: Some(ExprInstance::EListBody(list)),
        }] => match list.ps.as_slice() {
            [label, value] => match label.exprs.as_slice() {
                [Expr {
                    expr_instance: Some(ExprInstance::GString(text)),
                }] => (text.clone(), int_of(value)),
                other => panic!("a label: {other:?}"),
            },
            other => panic!("a delivery [label, value]: {other:?}"),
        },
        other => panic!("a delivery list: {other:?}"),
    }
}

/// Added by decision 93 (decision 111): the [`RunOutcome`] of `ran`.
fn outcome_of(ran: &RuntimeRun) -> RunOutcome {
    let mut deliveries: Vec<(String, i64)> = ran
        .outputs
        .iter()
        .map(|datum| delivery(datum.as_slice()))
        .collect();
    deliveries.sort();
    let records = |side: &[(i64, Vec<Par>, bool)]| -> Vec<(i64, bool)> {
        side.iter()
            .map(|(_, payload, persistent)| match payload.as_slice() {
                [value] => (int_of(value), *persistent),
                other => panic!("a record of one value: {other:?}"),
            })
            .collect()
    };
    let parked = |side: &[(i64, bool, bool)]| -> Vec<(bool, bool)> {
        side.iter()
            .map(|(_, persistent, peek)| (*persistent, *peek))
            .collect()
    };
    RunOutcome {
        deliveries,
        records: [records(&ran.records[0]), records(&ran.records[1])],
        parked: [parked(&ran.parked[0]), parked(&ran.parked[1])],
    }
}

/// Added by decision 93 (decision 111): the non-persistent records of `values`, in order.
fn linear(values: &[i64]) -> Vec<(i64, bool)> {
    values.iter().map(|value| (*value, false)).collect()
}

/// Added by decision 93 (decision 111): every order of `values`. Racing sends take their orders
/// in arrival order, so a store can list the records in any of these orders.
fn arrival_orders(values: &[i64]) -> Vec<Vec<i64>> {
    match values {
        [] => vec![Vec::new()],
        _ => {
            let mut orders = Vec::new();
            for index in 0..values.len() {
                let mut rest = values.to_vec();
                let first = rest.remove(index);
                for mut order in arrival_orders(&rest) {
                    order.insert(0, first);
                    orders.push(order);
                }
            }
            orders
        }
    }
}

/// Added by decision 93 (decision 111): the values 1 to 3 without `taken`.
fn others(taken: i64) -> Vec<i64> { (1..=3).filter(|value| *value != taken).collect() }

/// Added by decision 93 (decision 111): the [`RunOutcome`] of `deliveries`, `records` and
/// `parked`.
fn run_outcome(
    deliveries: &[(&str, i64)],
    records: [Vec<(i64, bool)>; 2],
    parked: [Vec<(bool, bool)>; 2],
) -> RunOutcome {
    let mut deliveries: Vec<(String, i64)> = deliveries
        .iter()
        .map(|(label, value)| ((*label).to_string(), *value))
        .collect();
    deliveries.sort();
    RunOutcome {
        deliveries,
        records,
        parked,
    }
}

/// Added by decision 93 (decision 111): every outcome that the program of `kind` allows. A take
/// and a peek see any record that has arrived. A queue head is the record that arrived first,
/// a stack head the last one. A pattern of a head-only collection matches only its head.
fn allowed_outcomes(kind: Kind) -> Vec<RunOutcome> {
    let none = || [Vec::new(), Vec::new()];
    let waits_on_a = || [vec![(false, false)], Vec::new()];
    let mut allowed = Vec::new();
    match kind {
        Kind::Bag => {
            for take in 1..=3 {
                for rest in arrival_orders(&others(take)) {
                    for peek in 1..=3 {
                        allowed.push(run_outcome(
                            &[
                                ("one", 1),
                                ("peek", peek),
                                ("peek persistent", 7),
                                ("persistent", 7),
                                ("take", take),
                            ],
                            [linear(&rest), vec![(7, true)]],
                            none(),
                        ));
                    }
                }
            }
        }
        Kind::Set => {
            for peek in 1..=3 {
                // The take retires every 1 that has arrived, so the class waits.
                for rest in arrival_orders(&[2, 3]) {
                    allowed.push(run_outcome(
                        &[("peek", peek), ("persistent", 7), ("take", 1)],
                        [linear(&rest), vec![(7, true)]],
                        waits_on_a(),
                    ));
                }
                // The class takes a 1. A 1 that arrives after a retirement is a new class.
                for (take, sets) in [
                    (1, vec![vec![2, 3]]),
                    (2, vec![vec![3], vec![1, 3]]),
                    (3, vec![vec![2], vec![1, 2]]),
                ] {
                    for set in sets {
                        for rest in arrival_orders(&set) {
                            allowed.push(run_outcome(
                                &[
                                    ("class", 1),
                                    ("peek", peek),
                                    ("persistent", 7),
                                    ("take", take),
                                ],
                                [linear(&rest), vec![(7, true)]],
                                none(),
                            ));
                        }
                    }
                }
            }
        }
        Kind::Queue | Kind::Stack => {
            let waiting = match kind {
                Kind::Queue => [1, 2],
                _ => [2, 1],
            };
            for take in 1..=3 {
                for rest in arrival_orders(&others(take)) {
                    let peeks = match (kind, rest.as_slice()) {
                        (Kind::Queue, [head, _]) => vec![take, *head],
                        _ => vec![1, 2, 3],
                    };
                    for peek in peeks {
                        // `two` fires when the 2 is the head of `b`.
                        allowed.push(run_outcome(
                            &[("peek", peek), ("take", take), ("two", 2)],
                            [linear(&rest), linear(&[1])],
                            none(),
                        ));
                        // Else the 1 is the head of `b`, and `two` waits.
                        allowed.push(run_outcome(
                            &[("peek", peek), ("take", take)],
                            [linear(&rest), linear(&waiting)],
                            [Vec::new(), vec![(false, false)]],
                        ));
                    }
                }
            }
        }
        Kind::Cell => allowed.push(run_outcome(
            &[("peek", 5), ("take", 1)],
            [vec![(2, false)], vec![(5, false)]],
            none(),
        )),
        Kind::Pq => {
            for peek in 1..=3 {
                // The lane takes the 1, so the take takes the 2 or the 3.
                for take in [2, 3] {
                    allowed.push(run_outcome(
                        &[("lane", 1), ("peek", peek), ("take", take), ("two", 2)],
                        [linear(&[5 - take]), vec![(1, false)]],
                        none(),
                    ));
                }
                // The take takes the 1, so the lane waits.
                for rest in arrival_orders(&[2, 3]) {
                    allowed.push(run_outcome(
                        &[("peek", peek), ("take", 1), ("two", 2)],
                        [linear(&rest), vec![(1, false)]],
                        waits_on_a(),
                    ));
                }
            }
        }
    }
    allowed
}

/// Added by decision 93 (decision 111): [`check_racing_runs_in`] on the HashMap store.
fn check_racing_runs(kind: Kind) { check_racing_runs_in(Store::HashMap, kind) }

/// Added by decision 93 (decision 111): runs the program of `kind` on the store `store` in fresh
/// runtimes, [`RACING_RUNS`] runs on each runtime of [`RACING_WORKERS`]. `Session::run`
/// certifies each deploy. Each run must log events, fire, pay phlo and reach an outcome that
/// [`allowed_outcomes`] holds.
fn check_racing_runs_in(store: Store, kind: Kind) {
    let program = program_of(kind);
    let allowed = allowed_outcomes(kind);
    for workers in RACING_WORKERS {
        let runtime = racing_runtime(workers);
        for run in 0..RACING_RUNS {
            let label = format!("{store:?} {kind:?} on {workers} workers, run {run}");
            let ran = runtime.block_on(fresh_run_in(store, kind, program));
            assert!(!ran.events.is_empty(), "{label}: the program has events");
            assert!(!ran.outputs.is_empty(), "{label}: the program fires");
            assert!(
                ran.phlo.is_some_and(|phlo| phlo > 0),
                "{label}: the program pays phlo"
            );
            let outcome = outcome_of(&ran);
            assert!(
                allowed.contains(&outcome),
                "{label}: {outcome:?} is not an allowed outcome"
            );
        }
    }
}

/// Added by decision 93 (decision 111): [`check_racing_runs_in`] for the bag, the stack and the
/// cell on the store `store`.
fn check_store_runs(store: Store) {
    for kind in [Kind::Bag, Kind::Stack, Kind::Cell] {
        check_racing_runs_in(store, kind);
    }
}

// Changed by decision 102 (plan v2 pins): play picks a record and a waiter of a bag at random
// (rule CR-1), so the list-order checks of play compare two random plays. The test now replays
// the log of each play over each list order. The old doc and test follow.
// /// Bag views are a function of the entry set (`rrs-spaces-determinism-canonical-iteration`). The
// /// agent visits the records and the waiters of a bag in the canonical order of DR-75. So the list
// /// order of the store changes no choice, no event, no legacy charge and no root. The test uses
// /// the order of the agent, the reverse order, the order of the history, and a permuted order of
// /// the build operations.
// #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// async fn space_canonical_iteration_bag_views() {
//     let (build, probe) = script_of(Kind::Bag);
//     let orders = list_orders(Kind::Bag, &build, &probe).await;
//     assert_same_runs(Kind::Bag, &orders);
//
//     // A bag record has the `ord` 0 and no counter, so the build operations commute: the agent
//     // builds the same entry set in any order.
//     let channels = space_channels(&[left(), right()]);
//     let mut permuted = build.clone();
//     permuted.reverse();
//     permuted.rotate_left(3);
//     let permuted_host = built(Kind::Bag, &permuted).await;
//     let agent_host = built(Kind::Bag, &build).await;
//     let permuted_rows = rows_of(&permuted_host, &channels).await;
//     let agent_rows = rows_of(&agent_host, &channels).await;
//     assert_eq!(entry_set(&permuted_rows), entry_set(&agent_rows));
//     assert_ne!(list_order(&permuted_rows), list_order(&agent_rows));
//     let permuted_run = observe(&permuted_host, &probe).await;
//     assert_eq!(permuted_run.steps, orders.agent.steps);
//     assert_eq!(permuted_run.root, orders.agent.root);
//
//     check_runs_twice(Kind::Bag).await;
// }
/// Bag views are a function of the entry set (`rrs-spaces-determinism-canonical-iteration`).
///
/// Changed by decision 102 (plan v2 pins): play picks a record and a waiter of a bag at random
/// (rule CR-1). So this test checked the claim on replay. Changed by decision 90 (C6, step D): the
/// replay ran over the rigged replay modes, which retire, so the test keeps the racing runs.
/// Native replay of a bag is certified by `space_native_replay_equal_hashmap`, and replay takes
/// the recorded candidates first
/// (`space_pick_helpers_put_the_named_candidates_first_in_replay` in `spaces/generic.rs`).
///
/// The text before: "The agent visits the records and the waiters of a bag in the canonical order
/// of DR-75. So the list order of the store changes no choice, no event, no legacy charge and no
/// root."
#[test]
fn space_canonical_iteration_bag_views() {
    // Retired by decision 90 (C6, step D): the replay-order check ran through the rigged replay
    // modes, which retire (decision 125).
    check_racing_runs(Kind::Bag);
}

// Retired by decision 90 (C6, step D): the replay-order checks of
// `space_canonical_iteration_bag_views` ran through the rigged replay modes, which retire (decision
// 125). `space_native_replay_equal_hashmap`, the pick helpers tests of `spaces/generic.rs`, the
// racing runs of this file.

/// The queue keeps its event order whatever the list order of the store: the head on each side is
/// the entry with the least `ord`.
// Changed by decision 93 (decision 111): the test runs the list orders on one runtime and the
// program on each racing runtime. The old test follows.
// #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// async fn space_canonical_iteration_queue_order() {
//     check_ordered_list_orders(Kind::Queue).await;
//     check_runs_twice(Kind::Queue).await;
// }
#[test]
fn space_canonical_iteration_queue_order() {
    tokio_runtime().block_on(check_ordered_list_orders(Kind::Queue));
    check_racing_runs(Kind::Queue);
}

/// The stack keeps its LIFO order whatever the list order of the store: the head on each side is
/// the entry with the greatest `ord`, and the settle fires the same pairs.
// Changed by decision 93 (decision 111): the test runs the list orders on one runtime and the
// program on each racing runtime. The old test follows.
// #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// async fn space_canonical_iteration_stack_order() {
//     check_ordered_list_orders(Kind::Stack).await;
//     check_runs_twice(Kind::Stack).await;
// }
#[test]
fn space_canonical_iteration_stack_order() {
    tokio_runtime().block_on(check_ordered_list_orders(Kind::Stack));
    check_racing_runs(Kind::Stack);
}

/// A cell holds at most one record and one waiter (F15), so each of its lists has one order only.
/// The agent build, the raw store and the history give the same steps, the same `CellOccupied`
/// and the same root.
// Changed by decision 93 (decision 111): the test runs the list orders on one runtime and the
// program on each racing runtime. The old test follows.
// #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// async fn space_canonical_iteration_cell_views() {
//     check_ordered_list_orders(Kind::Cell).await;
//     check_runs_twice(Kind::Cell).await;
// }
#[test]
fn space_canonical_iteration_cell_views() {
    tokio_runtime().block_on(check_ordered_list_orders(Kind::Cell));
    check_racing_runs(Kind::Cell);
}

/// Set views are a function of the entry set (wave W3).
///
/// Changed by decision 102 (plan v2 pins): play picks a record and a waiter of a set at random
/// (rule CR-1), and a class fire binds the first member of the class of its pick in DR-75 order
/// (decision 101, rule CR-13). So this test checked the claim on replay. Changed by decision 90
/// (C6, step D): the replay ran over the rigged replay modes, which retire, so the test keeps
/// the racing runs. Native replay of a set is certified by `space_native_replay_equal_hashmap`.
///
/// The text before: "The agent visits the records and the waiters of a set in the canonical order
/// of DR-75. A class sits at the place of its first member. So the list order of the store
/// changes no choice, no event, no legacy charge and no root."
// Changed by decision 93 (decision 111): the test runs the list orders on one runtime and the
// program on each racing runtime. The old test follows.
// #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// async fn space_canonical_iteration_set_views() {
//     let (build, probe) = script_of(Kind::Set);
//     let orders = list_orders(Kind::Set, &build, &probe).await;
//     assert_same_runs(Kind::Set, &orders);
//     check_runs_twice(Kind::Set).await;
// }
#[test]
fn space_canonical_iteration_set_views() {
    // Retired by decision 90 (C6, step D): the replay half ran through the rigged replay modes,
    // which retire (decision 125). `space_native_replay_equal_hashmap`, the pick helpers tests of
    // `spaces/generic.rs`, the racing runs of this file.
    check_racing_runs(Kind::Set);
}

/// The priority queue keeps its order whatever the list order of the store (wave W3). In each
/// level, the record with the least `ord` is the head. In each lane, the waiter with the least
/// `ord` is the head. The oldest lane head meets a send first.
// Changed by decision 93 (decision 111): the test runs the list orders on one runtime and the
// program on each racing runtime. The old test follows.
// #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// async fn space_canonical_iteration_pq_order() {
//     check_ordered_list_orders(Kind::Pq).await;
//     check_runs_twice(Kind::Pq).await;
// }
#[test]
fn space_canonical_iteration_pq_order() {
    tokio_runtime().block_on(check_ordered_list_orders(Kind::Pq));
    check_racing_runs(Kind::Pq);
}

/// The canonical-iteration rows of the bag, the stack and the cell on the store `store` (wave W3).
/// Three list orders give the same steps and roots. [`check_store_runs`] runs the program of each
/// row on each racing runtime (decisions 93 and 111).
// Changed by decision 93 (decision 111): the racing runs of the rows are `check_store_runs`. The
// old text follows.
// /// Three list orders give the same steps and roots. One program gives the same run in two fresh
// /// runtimes.
///
/// Changed by decision 90 (C6, step D): the bag row replayed the log of each play over each list
/// order through the rigged replay modes, which retire. The stack and the cell pick their heads,
/// so their rows keep the checks of play.
async fn check_store_rows(store: Store) {
    let [a, b] = store.collections();
    // Changed by decision 90 (C6, step D): the bag row retires with the rigged replay modes. The
    // text before:
    // for kind in [Kind::Bag, Kind::Stack, Kind::Cell] {
    for kind in [Kind::Stack, Kind::Cell] {
        let (build, probe) = script_on(kind, &a, &b);
        let orders = list_orders_in(store, kind, &build, &probe).await;
        assert_same_runs(kind, &orders);
        // Changed by decision 93 (decision 111): `check_store_runs` runs the program of each
        // row on each racing runtime. The old call follows.
        // check_runs_twice_in(store, kind).await;
    }
}

/// `array:bag`, `array:stack` and `array:cell`: the collections are the index names 0 and 1 of a
/// fixed Array space. The list order of the store changes no step and no root.
// Changed by decision 93 (decision 111): the program of each row runs on each racing runtime. The
// old test follows.
// #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// async fn space_canonical_iteration_array_rows() { check_store_rows(Store::Array).await; }
#[test]
fn space_canonical_iteration_array_rows() {
    tokio_runtime().block_on(check_store_rows(Store::Array));
    check_store_runs(Store::Array);
}

/// `vector:bag`, `vector:stack` and `vector:cell`: the collections are the index names 0 and 1
/// of a Vector space. The list order of the store changes no step and no root.
// Changed by decision 93 (decision 111): the program of each row runs on each racing runtime. The
// old test follows.
// #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// async fn space_canonical_iteration_vector_rows() { check_store_rows(Store::Vector).await; }
#[test]
fn space_canonical_iteration_vector_rows() {
    tokio_runtime().block_on(check_store_rows(Store::Vector));
    check_store_runs(Store::Vector);
}

/// The join waiters of the groups `groups` in `space`, each group in the list order of the store.
async fn group_rows(
    space: &impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    groups: &[Vec<Par>],
) -> Vec<Vec<ParWaiter>> {
    let mut rows = Vec::with_capacity(groups.len());
    for group in groups {
        rows.push(space.get_waiting_continuations(group.clone()).await);
    }
    rows
}

/// Parks the join waiters `rows` of the groups `groups` in `target` with raw host consumes, in the
/// order `order(n)` of each list. No raw join waiter takes a raw record, because a record holds
/// its envelope before its payload.
///
/// Changed by decision 102 (plan v4 C5.3, rule CR-1): `target` can be a replay space with no
/// rigged log, as for [`store_raw`]. So a replay space can hold the list order of a join group.
// Changed by decision 102 (plan v4 C5.3, rule CR-1): the target is any space, so that a replay
// space holds the list order of a join group. The old lines follow.
// async fn store_raw_groups(
//     target: &ParRSpace,
async fn store_raw_groups(
    target: &impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    groups: &[Vec<Par>],
    rows: &[Vec<ParWaiter>],
    order: &dyn Fn(usize) -> Vec<usize>,
) {
    for (group, waiters) in groups.iter().zip(rows) {
        for index in order(waiters.len()) {
            let waiter = &waiters[index];
            let fired = target
                .consume(
                    group.clone(),
                    waiter.patterns.clone(),
                    waiter.continuation.clone(),
                    waiter.persist,
                    waiter.peeks.clone(),
                )
                .await
                .expect("a raw consume runs");
            assert!(fired.is_none(), "a raw join waiter parks");
        }
    }
    target.take_event_log().await;
}

// Changed by decision 102 (plan v4 C5.3, rule CR-1) and decision 124 (LP-1, L5): on the bag,
// play picks the record of a collection that a group names once and the order of the waiters of
// a group at random, so the doc below replaces this one.
// /// The join groups of P5c and the list order of the store (wave W3). The choices of the join
// /// engine are a function of the entry set. They never depend on the order in which the store
// /// lists the join groups of a channel or the waiters of a group. The test uses a bag and a queue.
// ///
// /// * Three join waiters wait on the group of the left and the right collection.
// /// * One join waiter waits on the group of the left and the context collection.
// /// * One waiter waits on the right collection.
// ///
// /// They wait in the order of the agent, in the reverse order of raw host consumes, and in the
// /// order of the history. A probe of sends fires joins. The three runs give the same steps and
// /// roots.
/// The join groups of P5c and the list order of the store (wave W3). The choices of the join
/// engine are a function of the entry set. They never depend on the order in which the store
// Changed by decision 90 (C6, step D): the bag part retires. The text before:
// ```
// /// lists the join groups of a channel or the waiters of a group. The test uses a bag and a queue.
// ```
/// lists the join groups of a channel or the waiters of a group. The test uses a queue.
///
/// * Three join waiters wait on the group of the left and the right collection.
/// * One join waiter waits on the group of the left and the context collection.
/// * One waiter waits on the right collection.
///
/// They wait in the order of the agent, in the reverse order of raw host consumes, and in the
/// order of the history. A probe of sends fires joins. On the queue, the three runs give the same
/// steps and roots. Changed by decision 90 (C6, step D): the bag part replayed the log of each
/// play over each list order through the rigged replay modes, which retire. Native replay of a
/// bag join program is certified by `space_joins_replay_equal`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_canonical_iteration_join_groups() {
    let (a, b, c) = (left(), right(), context());
    let join = |binds: &[(&Par, BindPattern)], name: &str| {
        SpaceOp::Join(support::agent::join_on(SPACE, binds, name))
    };
    let build = vec![
        send(&a, 1, 1),
        send(&a, 2, 1),
        send(&a, 3, 1),
        join(&[(&a, any()), (&b, any())], "j1"),
        join(&[(&a, any()), (&b, any())], "j2"),
        join(&[(&a, exactly(2)), (&b, any())], "j3"),
        join(&[(&a, any()), (&c, any())], "k"),
        receive(&b, "r9", exactly(9)),
    ];
    let probe = vec![
        send(&b, 5, 1),
        send(&c, 7, 1),
        send(&b, 6, 1),
        send(&b, 9, 1),
        send(&b, 8, 1),
    ];
    let channels = space_channels(&[a.clone(), b.clone(), c.clone()]);
    let groups = vec![
        support::agent::join_group(SPACE, &[&a, &b]),
        support::agent::join_group(SPACE, &[&a, &c]),
    ];
    // Changed by decision 90 (C6, step D): the bag part retires with the rigged replay modes. The
    // text before:
    // for kind in [Kind::Bag, Kind::Queue] {
    for kind in [Kind::Queue] {
        let agent_host = built(kind, &build).await;
        let agent_rows = rows_of(&agent_host, &channels).await;
        let agent_groups = group_rows(&agent_host, &groups).await;
        assert_eq!(
            agent_groups.iter().map(Vec::len).collect::<Vec<_>>(),
            vec![3, 1],
            "{kind:?}: the joins park"
        );

        let reversed_host = play().await;
        store_raw(&reversed_host, &channels, &agent_rows, &reverse_order).await;
        store_raw_groups(&reversed_host, &groups, &agent_groups, &reverse_order).await;
        let reversed_rows = rows_of(&reversed_host, &channels).await;
        let reversed_groups = group_rows(&reversed_host, &groups).await;
        assert_eq!(entry_set(&reversed_rows), entry_set(&agent_rows));
        assert_eq!(
            sorted_groups(&reversed_groups),
            sorted_groups(&agent_groups),
            "{kind:?}: the raw store holds the join waiters of the agent"
        );
        assert_ne!(
            encoded(&reversed_groups[0]),
            encoded(&agent_groups[0]),
            "{kind:?}: the raw store lists the join waiters in another order"
        );

        let history_host = built(kind, &build).await;
        history_host
            .create_checkpoint()
            .await
            .expect("a checkpoint of the entry set");
        let history_groups = group_rows(&history_host, &groups).await;
        assert_eq!(
            sorted_groups(&history_groups),
            sorted_groups(&agent_groups),
            "{kind:?}: the history holds the join waiters of the agent"
        );

        let orders = ListOrders {
            agent: observe(&agent_host, &probe).await,
            reversed: observe(&reversed_host, &probe).await,
            history: observe(&history_host, &probe).await,
        };
        assert_same_runs(kind, &orders);
    }
}

// Retired by decision 90 (C6, step D): the replay-order check of the join groups ran through the
// rigged replay modes, which retire (decision 125). `space_native_replay_equal_hashmap`, the pick
// helpers tests of `spaces/generic.rs`, the racing runs of this file.

/// The join waiters of `groups` as sorted bytes: the entry set of the groups.
fn sorted_groups(groups: &[Vec<ParWaiter>]) -> Vec<Vec<Vec<u8>>> {
    groups
        .iter()
        .map(|waiters| {
            let mut bytes = encoded(waiters);
            bytes.sort();
            bytes
        })
        .collect()
}

// ─── Tie-break ───────────────────────────────────────────────────────────────────────────────

/// The length of the string payload of a tied candidate in the example tests.
const TIE_LENGTH: usize = 64;

/// The side of a collection whose two candidates tie.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    /// Two records: a receive selects one.
    Records,
    /// Two waiting continuations: a send selects one.
    Waiters,
}

/// The level of the tied entries of a priority queue: a priority query selects one of them
/// (wave W3).
const TIE_LEVEL: i64 = 0;

/// A collection of the discipline `kind` whose two candidates on `side` tie, with payloads of
/// `length` bytes.
///
/// * Bag and set: two byte-identical records, stored by two equal sends. Their source hashes are
///   equal, so DR-75 orders them by their digests.
/// * Priority queue: as the queue, at the level [`TIE_LEVEL`]. A receive with the priority query
///   of that level selects one record, and a send at that level selects one waiter of its lane.
/// * Queue, stack, cell: two entries with the `ord` 0, stored by raw host operations: records
///   with the payloads `"a…"` and `"b…"`, or waiters with the bodies `"a…"` and `"b…"`. With
///   `reversed`, the `"b…"` entry arrives first.
#[derive(Clone, Copy, Debug)]
struct Tie {
    /// The discipline of the collection.
    kind: Kind,
    /// The side whose candidates tie.
    side: Side,
    /// The length of the string payload or body of each candidate.
    length: usize,
    /// True when the `"b…"` candidate arrives first.
    reversed: bool,
}

/// A string of `length` copies of `fill`.
fn text(fill: char, length: usize) -> Par { string(&fill.to_string().repeat(length)) }

/// The random state of a raw tied entry.
fn tie_random(fill: char) -> Vec<u8> {
    Blake2b512Random::create_from_bytes(format!("rrs-det-tie {fill}").as_bytes()).to_bytes()
}

impl Tie {
    /// The tied record with the payload of `fill`, as the store holds it.
    fn record(&self, fill: char) -> ListParWithRandom {
        let datum = ListParWithRandom {
            pars: vec![text(fill, self.length)],
            random_state: tie_random(fill),
            cost_authority: None,
            cost_stack: None,
        };
        match self.kind {
            Kind::Pq => record::wrap_pq_datum(0, Some(TIE_LEVEL), &datum),
            Kind::Bag | Kind::Queue | Kind::Stack | Kind::Cell | Kind::Set => {
                record::wrap_datum(0, &datum)
            }
        }
    }

    /// The parked continuation of the tied waiter with the body of `fill`, as the store holds it.
    fn parked(&self, fill: char) -> TaggedContinuation {
        match self.kind {
            Kind::Pq => record::wrap_pq_continuation(0, Some(TIE_LEVEL), &self.continuation(fill)),
            Kind::Bag | Kind::Queue | Kind::Stack | Kind::Cell | Kind::Set => {
                record::wrap_continuation(0, &self.continuation(fill))
            }
        }
        .expect("a continuation record")
    }

    /// The continuation of the tied waiter with the body of `fill`, as the program gives it.
    fn continuation(&self, fill: char) -> TaggedContinuation {
        TaggedContinuation {
            tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                body: Some(text(fill, self.length)),
                random_state: tie_random(fill),
            })),
            guard: None,
            cost_authority: None,
        }
    }

    /// The fills of the raw entries in arrival order.
    fn arrivals(&self) -> [char; 2] {
        match self.reversed {
            false => ['a', 'b'],
            true => ['b', 'a'],
        }
    }

    /// The fill of the candidate that the bytes select: the least bytes for a queue and a cell,
    /// the greatest for a stack. Two bag records are byte-identical.
    fn winner(&self) -> char {
        match self.kind {
            Kind::Stack => 'b',
            Kind::Bag | Kind::Queue | Kind::Cell | Kind::Set | Kind::Pq => 'a',
        }
    }

    /// Creates the space of the tie in `host` and stores the tied candidates.
    async fn build(&self, host: &ParRSpace) {
        let env = create_space(self.kind, host, &budget()).await;
        let data = data_channel(SPACE, &tied());
        match (self.kind, self.side) {
            (Kind::Bag | Kind::Set, _) => {
                let twin = send_pars(
                    &tied(),
                    vec![text('t', self.length)],
                    &format!("twin {}", self.length),
                    false,
                );
                for _ in 0..2 {
                    run(&FamilyAgent, &env, twin.clone())
                        .await
                        .expect("a send of the twin stores it");
                }
            }
            (_, Side::Records) => {
                for fill in self.arrivals() {
                    let fired = host
                        .produce(data.clone(), self.record(fill), false)
                        .await
                        .expect("a raw produce runs");
                    assert!(fired.is_none(), "a raw tied record is stored");
                }
            }
            (_, Side::Waiters) => {
                for fill in self.arrivals() {
                    let parked = self.parked(fill);
                    let fired = host
                        .consume(
                            vec![data.clone()],
                            vec![any()],
                            parked,
                            false,
                            BTreeSet::new(),
                        )
                        .await
                        .expect("a raw consume runs");
                    assert!(fired.is_none(), "a raw tied waiter parks");
                }
            }
        }
        host.take_event_log().await;
    }

    /// The operation that selects one of the tied candidates: a receive of any one value for
    /// records, and a send of 1 for waiters.
    fn op(&self) -> SpaceOp {
        match (self.side, self.kind) {
            (Side::Records, Kind::Pq) => receive_in(&tied(), "select", any(), TIE_LEVEL),
            (Side::Waiters, Kind::Pq) => send_at(&tied(), 1, 1, TIE_LEVEL),
            (Side::Records, _) => receive(&tied(), "select", any()),
            (Side::Waiters, _) => send(&tied(), 1, 1),
        }
    }
}

/// The fill of the candidate that `tie.op()` binds on a fresh tie. The operation fires exactly
/// once.
async fn selected(tie: Tie) -> char {
    let host = play().await;
    tie.build(&host).await;
    let outcome = run(&FamilyAgent, &env_over(host.clone(), &budget()), tie.op())
        .await
        .expect("the selection commits");
    let [fire] = outcome.fires.as_slice() else {
        panic!("{tie:?}: the selection fires {} times", outcome.fires.len());
    };
    let fill_of = |par: &Par| match par.exprs.as_slice() {
        [Expr {
            expr_instance: Some(ExprInstance::GString(text)),
        }] => text.chars().next().expect("a payload of one fill"),
        other => panic!("{tie:?}: a payload of one string: {other:?}"),
    };
    match tie.side {
        Side::Records => {
            let [result] = fire.results.as_slice() else {
                panic!("{tie:?}: the fire binds {} records", fire.results.len());
            };
            let payload = record::unwrap_pq_datum(&result.removed_datum)
                .expect("a datum record")
                .datum;
            fill_of(&payload.pars[0])
        }
        Side::Waiters => match &fire.continuation.tagged_cont {
            Some(TaggedCont::ParBody(ParWithRandom {
                body: Some(body), ..
            })) => fill_of(body),
            other => panic!("{tie:?}: a fired continuation with a body: {other:?}"),
        },
    }
}

/// The bag orders two byte-identical records by their digests (DR-75, decision 41), so a receive
/// fires one of the twins.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_candidate_tie_bag() {
    let tie = Tie {
        kind: Kind::Bag,
        side: Side::Records,
        length: TIE_LENGTH,
        reversed: false,
    };
    assert_eq!(selected(tie).await, 't', "the twin record fires");
}

/// A queue, a stack and a cell order two entries with one `ord` by their bincode bytes. On each
/// side and in each arrival order, the bytes select the candidate: the least bytes for a queue
/// and a cell, the greatest for a stack. So a candidate never wins by its arrival order.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_candidate_tie_ordered() {
    for kind in Kind::ORDERED {
        for side in [Side::Records, Side::Waiters] {
            for reversed in [false, true] {
                let tie = Tie {
                    kind,
                    side,
                    length: TIE_LENGTH,
                    reversed,
                };
                assert_eq!(
                    selected(tie).await,
                    tie.winner(),
                    "{tie:?}: the bytes select the candidate"
                );
            }
        }
    }
}

/// A set orders two byte-identical records by their digests (DR-75), as the bag does (wave W3),
/// so a receive fires the class of the twins.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_candidate_tie_set() {
    let tie = Tie {
        kind: Kind::Set,
        side: Side::Records,
        length: TIE_LENGTH,
        reversed: false,
    };
    assert_eq!(selected(tie).await, 't', "the class of the twins fires");
}

/// A priority queue orders two entries with one `ord` in one level by their bincode bytes, as
/// the queue does. These are the ties of a priority query (wave W3). On each side and in each
/// arrival order, the least bytes select the candidate.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_candidate_tie_priority() {
    for side in [Side::Records, Side::Waiters] {
        for reversed in [false, true] {
            let tie = Tie {
                kind: Kind::Pq,
                side,
                length: TIE_LENGTH,
                reversed,
            };
            assert_eq!(
                selected(tie).await,
                tie.winner(),
                "{tie:?}: the bytes select the candidate"
            );
        }
    }
}

// ─── Properties ──────────────────────────────────────────────────────────────────────────────

/// A generated operation of a canonical-iteration property.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Generated {
    /// A send of `value` with `salt` on the left or the right collection.
    Send {
        on_left: bool,
        value: i64,
        salt: u64,
        persistent: bool,
    },
    /// A receive with the body `name` on the left or the right collection. `pattern` is a value
    /// for a literal pattern, or `None` for a free variable.
    Receive {
        on_left: bool,
        name: u64,
        pattern: Option<i64>,
        peek: bool,
        persistent: bool,
    },
}

impl Generated {
    /// The space operation.
    fn op(self) -> SpaceOp {
        let collection = |on_left: bool| match on_left {
            true => left(),
            false => right(),
        };
        match self {
            Generated::Send {
                on_left,
                value,
                salt,
                persistent: false,
            } => send(&collection(on_left), value, salt),
            Generated::Send {
                on_left,
                value,
                salt,
                persistent: true,
            } => send_forever(&collection(on_left), value, salt),
            Generated::Receive {
                on_left,
                name,
                pattern,
                peek,
                persistent,
            } => {
                let pattern = match pattern {
                    Some(value) => exactly(value),
                    None => any(),
                };
                let mut op = receive(&collection(on_left), &format!("r{name}"), pattern);
                if peek {
                    op = peeking(op);
                }
                if persistent {
                    op = persisting(op);
                }
                op
            }
        }
    }
}

/// A send or a receive of any kind, with values in `0..6`.
fn arb_generated() -> BoxedStrategy<Generated> {
    prop_oneof![
        3 => (arbitrary::<bool>(), 0i64..6, 0u64..3, prop_oneof![4 => Just(false), 1 => Just(true)])
            .prop_map(|(on_left, value, salt, persistent)| Generated::Send {
                on_left,
                value,
                salt,
                persistent,
            }),
        2 => (
            arbitrary::<bool>(),
            0u64..1_000,
            proptest::option::of(0i64..6),
            0u8..4,
        )
            .prop_map(|(on_left, name, pattern, kind)| Generated::Receive {
                on_left,
                name,
                pattern,
                peek: kind == 1,
                persistent: kind == 2,
            }),
    ]
    .boxed()
}

// Retired by decision 90 (C6, step D): the replay-order property that this strategy feeds ran
// through the rigged replay modes, which retire (decision 125).
// `space_native_replay_equal_hashmap`, the pick helpers tests of `spaces/generic.rs`, the racing
// runs of this file. The text before:
// ```
// /// A build of a bag whose operations commute: sends of values in `0..6`, and receives that no
// /// stored value fires, with literal patterns for values in `10..13`.
// fn arb_commuting_bag_build() -> BoxedStrategy<Vec<Generated>> {
//     let store = (arbitrary::<bool>(), 0i64..6, 0u64..3).prop_map(|(on_left, value, salt)| {
//         Generated::Send {
//             on_left,
//             value,
//             salt,
//             persistent: false,
//         }
//     });
//     let park = (
//         arbitrary::<bool>(),
//         0u64..1_000,
//         10i64..13,
//         arbitrary::<bool>(),
//     )
//         .prop_map(|(on_left, name, value, peek)| Generated::Receive {
//             on_left,
//             name,
//             pattern: Some(value),
//             peek,
//             persistent: false,
//         });
//     vec(prop_oneof![3 => store, 2 => park], 2..10).boxed()
// }
// ```

/// The insertion order of a list of `length` entries for the shuffle `seed`: the indices sorted by
/// a hash of the seed and the index.
fn shuffled(seed: u64, length: usize) -> Vec<usize> {
    let mut keyed: Vec<(Vec<u8>, usize)> = (0..length)
        .map(|index| {
            (
                Blake2b512Random::create_from_bytes(
                    format!("rrs-det-shuffle {seed} {index}").as_bytes(),
                )
                .to_bytes(),
                index,
            )
        })
        .collect();
    keyed.sort();
    keyed.into_iter().map(|(_, index)| index).collect()
}

/// The canonical-iteration check of one generated case: the entry set of `build` for `kind`, in
/// the order of the agent, stored again by raw host operations in the order of `seed`, and in the
/// order of the history. The probe gives the same steps and the same root in all three.
async fn generated_list_orders(
    kind: Kind,
    build: &[SpaceOp],
    seed: u64,
    probe: &[SpaceOp],
    coverage: &Coverage,
) -> Result<(), TestCaseError> {
    let channels = space_channels(&[left(), right()]);
    let agent_host = built(kind, build).await;
    let agent_rows = rows_of(&agent_host, &channels).await;
    let raw_host = play().await;
    store_raw(&raw_host, &channels, &agent_rows, &|length| {
        shuffled(seed, length)
    })
    .await;
    let raw_rows = rows_of(&raw_host, &channels).await;
    prop_assert_eq!(entry_set(&raw_rows), entry_set(&agent_rows));
    let history_host = built(kind, build).await;
    history_host
        .create_checkpoint()
        .await
        .expect("a checkpoint of the entry set");
    let history_rows = rows_of(&history_host, &channels).await;
    prop_assert_eq!(entry_set(&history_rows), entry_set(&agent_rows));
    if list_order(&raw_rows) != list_order(&agent_rows)
        && list_order(&history_rows) != list_order(&agent_rows)
    {
        coverage.reordered.fetch_add(1, Ordering::Relaxed);
    }
    let agent = observe(&agent_host, probe).await;
    if agent.steps.iter().any(|step| {
        step.result
            .as_ref()
            .is_ok_and(|outcome| !outcome.fires.is_empty())
    }) {
        coverage.fired.fetch_add(1, Ordering::Relaxed);
    }
    for other in [
        observe(&raw_host, probe).await,
        observe(&history_host, probe).await,
    ] {
        prop_assert_eq!(&other.steps, &agent.steps);
        prop_assert_eq!(&other.root, &agent.root);
    }
    Ok(())
}

// Retired by decision 90 (C6, step D): the replay-order check of one generated bag case ran through
// the rigged replay modes, which retire (decision 125). `space_native_replay_equal_hashmap`, the
// pick helpers tests of `spaces/generic.rs`, the racing runs of this file.

/// The coverage of one canonical-iteration property: the generated cases whose raw store and
/// history both list some entries in another order than the agent, and the cases whose probe
/// fired at least once.
struct Coverage {
    /// The cases whose raw store and history list some entries in another order.
    reordered: AtomicUsize,
    /// The cases whose probe fired.
    fired: AtomicUsize,
}

impl Coverage {
    /// No case yet.
    const fn new() -> Self {
        Coverage {
            reordered: AtomicUsize::new(0),
            fired: AtomicUsize::new(0),
        }
    }

    /// Asserts that the property met reordered lists and fires, so it is not vacuous.
    fn assert_not_vacuous(&self) {
        assert!(
            self.reordered.load(Ordering::Relaxed) > 0,
            "no generated case listed its entries in another order"
        );
        assert!(
            self.fired.load(Ordering::Relaxed) > 0,
            "no generated probe fired"
        );
    }
}

// Retired by decision 90 (C6, step D): the property that read this coverage retires (decision
// 125). The text before:
// ```
// /// The coverage of `prop_canonical_iteration_bag_any_insertion_order`.
// static BAG_COVERAGE: Coverage = Coverage::new();
// ```

/// The coverage of `prop_canonical_iteration_ordered_any_list_order`.
static ORDERED_COVERAGE: Coverage = Coverage::new();

// Retired by decision 90 (C6, step D): this property ran through the rigged replay modes, which
// retire (decision 125). `space_native_replay_equal_hashmap`, the pick helpers tests of
// `spaces/generic.rs`, the racing runs of this file. The text before:
// ```
// /// `bag_visit_order_canonical`, `waiter_visit_order_canonical` and
// /// `hashmap_iteration_permutation_invariant`, over generated bags: a build whose operations
// /// commute gives one entry set in every order of its operations, and a generated probe gives the
// /// same results, events, legacy charges and root for the order of the agent, for a permuted order
// /// of the build, for a shuffled raw store and for the order of the history.
// #[test]
// fn prop_canonical_iteration_bag_any_insertion_order() {
//     let strategy = (
//         arb_commuting_bag_build().prop_flat_map(|build| {
//             let shuffled = Just(build.clone()).prop_shuffle();
//             (Just(build), shuffled)
//         }),
//         arbitrary::<u64>(),
//         vec(arb_generated(), 1..8),
//     );
//     check(
//         pure_config(),
//         strategy,
//         |((build, permuted), seed, probe)| {
//             let build: Vec<SpaceOp> = build.into_iter().map(Generated::op).collect();
//             let permuted: Vec<SpaceOp> = permuted.into_iter().map(Generated::op).collect();
//             let probe: Vec<SpaceOp> = probe.into_iter().map(Generated::op).collect();
//             tokio_runtime().block_on(async {
//                 let channels = space_channels(&[left(), right()]);
//                 let ordered = built(Kind::Bag, &build).await;
//                 let reordered = built(Kind::Bag, &permuted).await;
//                 prop_assert_eq!(
//                     entry_set(&rows_of(&reordered, &channels).await),
//                     entry_set(&rows_of(&ordered, &channels).await)
//                 );
//                 let first = observe(&ordered, &probe).await;
//                 let second = observe(&reordered, &probe).await;
//                 prop_assert_eq!(&second.steps, &first.steps);
//                 prop_assert_eq!(&second.root, &first.root);
//                 generated_list_orders(Kind::Bag, &build, seed, &probe, &BAG_COVERAGE).await
//             })
//         },
//     );
//     BAG_COVERAGE.assert_not_vacuous();
// }
// ```

/// `stack_head_is_max_ord`, `stack_plan_permutation_invariant`, the head rules of the queue and
/// of the cell, and `hashmap_iteration_permutation_invariant`, over generated queues, stacks and
/// cells: the entry set of a generated build gives the same results, events, observer calls and
/// root for a generated probe in the order of the agent, in a shuffled raw store
/// and in the order of the history.
#[test]
fn prop_canonical_iteration_ordered_any_list_order() {
    let strategy = (
        select(Kind::ORDERED.to_vec()),
        vec(arb_generated(), 1..10),
        arbitrary::<u64>(),
        vec(arb_generated(), 1..8),
    );
    check(pure_config(), strategy, |(kind, build, seed, probe)| {
        let build: Vec<SpaceOp> = build.into_iter().map(Generated::op).collect();
        let probe: Vec<SpaceOp> = probe.into_iter().map(Generated::op).collect();
        tokio_runtime().block_on(generated_list_orders(
            kind,
            &build,
            seed,
            &probe,
            &ORDERED_COVERAGE,
        ))
    });
    ORDERED_COVERAGE.assert_not_vacuous();
}

/// `bag_visit_order_canonical`, `queue_head_is_min_ord` and `stack_head_is_max_ord`, at the
/// tie-break: for generated disciplines, sides, arrival orders and payload lengths, the bytes
/// select the candidate of a tie. Wave W3 adds the set (the tie digest of DR-75) and the priority
/// queue (the ties of a priority query).
#[test]
fn prop_candidate_tie_bytes_select_in_any_arrival_order() {
    let strategy = (
        select(vec![
            Kind::Bag,
            Kind::Queue,
            Kind::Stack,
            Kind::Cell,
            Kind::Set,
            Kind::Pq,
        ]),
        arbitrary::<bool>(),
        arbitrary::<bool>(),
        1usize..256,
    );
    check(
        support::props::runtime_config(),
        strategy,
        |(kind, waiters, reversed, length)| {
            let side = match (kind, waiters) {
                (Kind::Bag | Kind::Set, _) | (_, false) => Side::Records,
                (_, true) => Side::Waiters,
            };
            let tie = Tie {
                kind,
                side,
                length,
                reversed,
            };
            let expected = match kind {
                Kind::Bag | Kind::Set => 't',
                Kind::Queue | Kind::Stack | Kind::Cell | Kind::Pq => tie.winner(),
            };
            tokio_runtime().block_on(async {
                prop_assert_eq!(selected(tie).await, expected, "{:?}", tie);
                Ok(())
            })
        },
    );
}
