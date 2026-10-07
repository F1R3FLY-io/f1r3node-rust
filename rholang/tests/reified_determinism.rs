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
//! The three runs must give the same results, the same events, the same legacy charges and the
//! same root. The head scan of an ordered discipline does the same work in every list order, so
//! the three runs of a queue, a stack or a cell must also charge the same host work. The canonical
//! order of a bag sorts with a heapsort that reserves each comparison, and the number of
//! comparisons depends on the list order. So the host work of a bag operation can differ between
//! list orders, and the bag tests do not compare it. A cell holds at most one entry in each list
//! (F15), so the three stores list a cell in one order. Each test then runs one Rholang program
//! twice, in two fresh runtimes, and compares the canonical state (the root), the event log and
//! the cost (N9, `run_twice_assert_canonical_equal`).
//!
//! **Serialization failure.** An agent serializes candidates only to order a tie. A bag orders
//! records with equal source hashes by their digests (DR-75, decision 41), and an ordered
//! discipline orders two entries with one `ord` by their bincode bytes. The seam `CanonicalBytes`
//! of P5 design v2 no longer exists, so the injection is a host-work cut (decision 42). A
//! `VerificationBytes` budget that ends exactly at the first read of the serialization step makes
//! the operation fail with `HostWorkRejected` before it stages anything. The rows, the legacy
//! charges (no observer call), the events and the root stay unchanged, on a play space, on a
//! rigged replay space and after a checkpoint reset. A failing candidate never wins by its
//! arrival order: with enough budget the bytes decide, in both arrival orders.
//!
//! A tie of an ordered discipline needs two entries with one `ord`. The agent never makes them
//! (`HeadOnlyDisciplines.v`, `wf`), so the ordered tests store the tied entries with raw host
//! operations. That is the only state in which the metered tie-break of `head_metered` runs.
//!
//! The tests restate these formal statements:
//!
//! | Test | Formal statement |
//! |---|---|
//! | `space_canonical_iteration_bag_views`, `prop_canonical_iteration_bag_any_insertion_order` | `bag_visit_order_canonical`, `waiter_visit_order_canonical` (`DataCollection.v`, `ContinuationCollection.v`), `hashmap_iteration_permutation_invariant` (`HashMapStore.v`) |
//! | `space_canonical_iteration_{queue,stack}_order`, `space_canonical_iteration_cell_views`, `prop_canonical_iteration_ordered_any_list_order` | `queue_head_is_min_ord`, `queue_plan_permutation_invariant` (`HeadOnlyDisciplines.v`), `scan_charge_permutation_invariant`, `operation_charge_permutation_invariant` (`AgentReadCoverage.v`), `stack_head_is_max_ord`, `stack_plan_permutation_invariant` (`StackDiscipline.v`), `hashmap_iteration_permutation_invariant` |
//! | `space_candidate_serialize_failure_{bag,ordered}`, `prop_candidate_serialize_failure_cut_is_exact` | `scan_trace_covered` (`DataCollection.v`), `settle_trace_covered` (`StackDiscipline.v`), `operation_trace_covered`, `engine_primitives_trace_covered` (`AgentReadCoverage.v`) |

#[path = "support/mod.rs"]
mod support;

use std::cell::RefCell;
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
use models::rust::host_work::{
    HostWorkDimension, HostWorkLimit, HostWorkLimits, HostWorkReservationError, HostWorkUnits,
    HostWorkUsage,
};
use models::rust::space_factory_urn::{SpaceFactoryUrn, SpaceInner, SpaceOuter};
use proptest::arbitrary::any as arbitrary;
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::sample::select;
use proptest::test_runner::TestCaseError;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::RuntimeBudget;
use rholang::rust::interpreter::host_work::HostWorkBudget;
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
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::native_source::SourceMeter;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::space_store_handle::{canonical_data_order, metered_encode};
use rspace_plus_plus::rspace::trace::event::{Event, Produce};
use support::agent::{
    any, budget, config_of, counter_channel, create, creation, data_channel, env_over, exactly,
    int, metered_env, play, play_and_replay, receive_on, run, send_on, space_counter_channel,
    string, unbounded_host_work, ParRSpace,
};
use support::props::{check, pure_config, tokio_runtime};
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

/// The phlo of a legacy scope: far above every script of the suite.
const LEGACY_PHLO: i64 = 1_000_000_000_000;

/// The host-work dimensions that a space agent charges (decision 42), in the order of
/// `ScanMeter::charge`.
const AGENT_DIMENSIONS: [HostWorkDimension; 3] = [
    HostWorkDimension::VerificationOperations,
    HostWorkDimension::VerificationBytes,
    HostWorkDimension::SearchStateBytes,
];

/// The inner channel of the first collection of a canonical-iteration rig.
fn left() -> Par { string("rrs-det-left") }

/// The inner channel of the second collection of a canonical-iteration rig.
fn right() -> Par { string("rrs-det-right") }

/// The inner channel of the collection whose two candidates tie.
fn tied() -> Par { string("rrs-det-tied") }

/// The inner channel of the collection of the operation that runs before the failed operation in
/// the replay of a serialization test.
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
async fn store_raw(
    target: &ParRSpace,
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

/// What one operation of a probe script shows: its result, its events, its legacy charges, and
/// its host work in the three dimensions that an agent charges.
#[derive(Clone, Debug, PartialEq)]
struct Observed {
    /// The outcome of the operation, or its error.
    result: Result<SpaceOutcome, SpaceError>,
    /// The events of the operation, in log order.
    events: Vec<Event>,
    /// The legacy charges of the operation (decision 45, Option P).
    charges: Vec<Cost>,
    /// The host work of the operation, in the order of `AGENT_DIMENSIONS`.
    work: [u64; 3],
}

/// One run of a probe script: each step, and the root after the last step.
#[derive(Clone, Debug, PartialEq)]
struct Run {
    /// The steps, in script order.
    steps: Vec<Observed>,
    /// The root of the checkpoint after the last step.
    root: Blake2b256Hash,
}

/// One step of a run without its host work: the result, the events and the legacy charges.
type Step = (Result<SpaceOutcome, SpaceError>, Vec<Event>, Vec<Cost>);

impl Run {
    /// The run without its host work.
    fn without_work(&self) -> Vec<Step> {
        self.steps
            .iter()
            .map(|step| {
                (
                    step.result.clone(),
                    step.events.clone(),
                    step.charges.clone(),
                )
            })
            .collect()
    }

    /// The host work of each step.
    fn work(&self) -> Vec<[u64; 3]> { self.steps.iter().map(|step| step.work).collect() }
}

/// Runs `script` over `host` inside a legacy scope (decision 45, Option P), with one unbounded
/// host-work budget for each operation, and checkpoints at the end.
async fn observe(host: &ParRSpace, script: &[SpaceOp]) -> Run {
    let budget = RuntimeBudget::new(Cost::create(LEGACY_PHLO, "rrs determinism probe"));
    let _scope = budget.enter_legacy_scope();
    host.take_event_log().await;
    let mut steps = Vec::with_capacity(script.len());
    for op in script {
        let work = unbounded_host_work();
        let env = metered_env(host.clone(), &budget, &work);
        let before = budget.get_log().len();
        let result = run(&FamilyAgent, &env, op.clone()).await;
        steps.push(Observed {
            result,
            events: host.take_event_log().await,
            charges: budget.get_log()[before..].to_vec(),
            work: AGENT_DIMENSIONS.map(|dimension| work.usage(dimension).get()),
        });
    }
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

/// Asserts that the three runs of `orders` give the same results, events, legacy charges and
/// roots. With `same_work`, the host work of each step is equal too.
fn assert_same_runs(kind: Kind, orders: &ListOrders, same_work: bool) {
    let agent = orders.agent.without_work();
    assert!(
        orders.agent.steps.iter().any(|step| step
            .result
            .as_ref()
            .is_ok_and(|outcome| !outcome.fires.is_empty())),
        "{kind:?}: the probe fires"
    );
    for (label, other) in [("reversed", &orders.reversed), ("history", &orders.history)] {
        assert_eq!(
            other.without_work(),
            agent,
            "{kind:?}: the {label} list order changes a step"
        );
        assert_eq!(
            other.root, orders.agent.root,
            "{kind:?}: the {label} list order changes the root"
        );
        if same_work {
            assert_eq!(
                other.work(),
                orders.agent.work(),
                "{kind:?}: the {label} list order changes the host work"
            );
        }
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

/// The canonical-iteration check of an ordered discipline: three list orders, the same steps,
/// the same root and the same host work.
async fn check_ordered_list_orders(kind: Kind) {
    let (build, probe) = script_of(kind);
    let orders = list_orders(kind, &build, &probe).await;
    assert_same_runs(kind, &orders, true);
}

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

/// Runs the program of `kind` twice, in two fresh runtimes, and compares the root, the event
/// logs and the costs (N9, `run_twice_assert_canonical_equal`).
async fn check_runs_twice(kind: Kind) { check_runs_twice_in(Store::HashMap, kind).await }

/// [`check_runs_twice`] on the space of `store` and `kind`.
async fn check_runs_twice_in(store: Store, kind: Kind) {
    let program = program_of(kind);
    let first = fresh_run_in(store, kind, program).await;
    let second = fresh_run_in(store, kind, program).await;
    assert!(!first.events.is_empty(), "{kind:?}: the program has events");
    assert!(!first.outputs.is_empty(), "{kind:?}: the program fires");
    assert!(first.cost.value > 0, "{kind:?}: the program costs phlo");
    assert_eq!(
        second, first,
        "{kind:?}: two fresh runtimes give the same run"
    );
}

/// Bag views are a function of the entry set (`rrs-spaces-determinism-canonical-iteration`). The
/// agent visits the records and the waiters of a bag in the canonical order of DR-75. So the list
/// order of the store changes no choice, no event, no legacy charge and no root. The test uses
/// the order of the agent, the reverse order, the order of the history, and a permuted order of
/// the build operations. The host work is not compared, because the heapsort of the canonical
/// order makes a number of comparisons that depends on the list order.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_canonical_iteration_bag_views() {
    let (build, probe) = script_of(Kind::Bag);
    let orders = list_orders(Kind::Bag, &build, &probe).await;
    assert_same_runs(Kind::Bag, &orders, false);

    // A bag record has the `ord` 0 and no counter, so the build operations commute: the agent
    // builds the same entry set in any order.
    let channels = space_channels(&[left(), right()]);
    let mut permuted = build.clone();
    permuted.reverse();
    permuted.rotate_left(3);
    let permuted_host = built(Kind::Bag, &permuted).await;
    let agent_host = built(Kind::Bag, &build).await;
    let permuted_rows = rows_of(&permuted_host, &channels).await;
    let agent_rows = rows_of(&agent_host, &channels).await;
    assert_eq!(entry_set(&permuted_rows), entry_set(&agent_rows));
    assert_ne!(list_order(&permuted_rows), list_order(&agent_rows));
    let permuted_run = observe(&permuted_host, &probe).await;
    assert_eq!(permuted_run.without_work(), orders.agent.without_work());
    assert_eq!(permuted_run.root, orders.agent.root);

    check_runs_twice(Kind::Bag).await;
}

/// The queue keeps its event order whatever the list order of the store: the head on each side is
/// the entry with the least `ord`, and the head scan charges the same host work in every list
/// order.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_canonical_iteration_queue_order() {
    check_ordered_list_orders(Kind::Queue).await;
    check_runs_twice(Kind::Queue).await;
}

/// The stack keeps its LIFO order whatever the list order of the store: the head on each side is
/// the entry with the greatest `ord`, the settle fires the same pairs, and the head scan charges
/// the same host work in every list order.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_canonical_iteration_stack_order() {
    check_ordered_list_orders(Kind::Stack).await;
    check_runs_twice(Kind::Stack).await;
}

/// A cell holds at most one record and one waiter (F15), so each of its lists has one order only.
/// The agent build, the raw store and the history give the same steps, the same `CellOccupied`,
/// the same host work and the same root.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_canonical_iteration_cell_views() {
    check_ordered_list_orders(Kind::Cell).await;
    check_runs_twice(Kind::Cell).await;
}

/// Set views are a function of the entry set (wave W3). The agent visits the records and the
/// waiters of a set in the canonical order of DR-75. A class sits at the place of its first
/// member. So the list order of the store changes no choice, no event, no legacy charge and no
/// root. The host work is not compared, because the canonical order sorts with the heapsort of
/// the bag.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_canonical_iteration_set_views() {
    let (build, probe) = script_of(Kind::Set);
    let orders = list_orders(Kind::Set, &build, &probe).await;
    assert_same_runs(Kind::Set, &orders, false);
    check_runs_twice(Kind::Set).await;
}

/// The priority queue keeps its order whatever the list order of the store (wave W3). In each
/// level, the record with the least `ord` is the head. In each lane, the waiter with the least
/// `ord` is the head. The oldest lane head meets a send first. The lane scan charges the same host
/// work in every list order.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_canonical_iteration_pq_order() {
    check_ordered_list_orders(Kind::Pq).await;
    check_runs_twice(Kind::Pq).await;
}

/// The canonical-iteration rows of the bag, the stack and the cell on the store `store` (wave W3).
/// Three list orders give the same steps and roots. For the stack and the cell, they also give
/// the same host work. One program gives the same run in two fresh runtimes.
async fn check_store_rows(store: Store) {
    let [a, b] = store.collections();
    for kind in [Kind::Bag, Kind::Stack, Kind::Cell] {
        let (build, probe) = script_on(kind, &a, &b);
        let orders = list_orders_in(store, kind, &build, &probe).await;
        assert_same_runs(kind, &orders, kind != Kind::Bag);
        check_runs_twice_in(store, kind).await;
    }
}

/// `array:bag`, `array:stack` and `array:cell`: the collections are the index names 0 and 1 of a
/// fixed Array space. The list order of the store changes no step and no root.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_canonical_iteration_array_rows() { check_store_rows(Store::Array).await; }

/// `vector:bag`, `vector:stack` and `vector:cell`: the collections are the index names 0 and 1
/// of a Vector space. The list order of the store changes no step and no root.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_canonical_iteration_vector_rows() { check_store_rows(Store::Vector).await; }

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
async fn store_raw_groups(
    target: &ParRSpace,
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

/// The join groups of P5c and the list order of the store (wave W3). The choices of the join
/// engine are a function of the entry set. They never depend on the order in which the store
/// lists the join groups of a channel or the waiters of a group. The test uses a bag and a queue.
///
/// * Three join waiters wait on the group of the left and the right collection.
/// * One join waiter waits on the group of the left and the context collection.
/// * One waiter waits on the right collection.
///
/// They wait in the order of the agent, in the reverse order of raw host consumes, and in the
/// order of the history. A probe of sends fires joins. The three runs give the same steps and
/// roots, and on the queue the same host work.
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
    for kind in [Kind::Bag, Kind::Queue] {
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
        assert_same_runs(kind, &orders, kind != Kind::Bag);
    }
}

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

// ─── Serialization failure ───────────────────────────────────────────────────────────────────

/// The length of the string payload of a tied candidate in the example tests.
const TIE_LENGTH: usize = 64;

/// How much longer the payloads of the second tie of [`find_cut`] are.
const LONGER_BY: usize = 32;

/// One reservation of a meter: operations, scanned bytes and backing bytes.
type Reservation = (usize, usize, usize);

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

    /// Creates the space of the tie in `host`, parks a waiter on the context collection, and
    /// stores the tied candidates. A send on the context collection fires that waiter, so the
    /// replay test has a COMM to pair.
    async fn build(&self, host: &ParRSpace) {
        let env = create_space(self.kind, host, &budget()).await;
        run(&FamilyAgent, &env, receive(&context(), "context", any()))
            .await
            .expect("the context waiter parks");
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

    /// The reservations of the serialization step of [`Tie::op`] in `host`, as a recording meter
    /// sees them, and the index of the first reservation of the step.
    ///
    /// * Bag: the canonical order of DR-75 over the stored records. Its first reservations, up to
    ///   the last scan of the source hashes, equal those of the order of two records with
    ///   distinct source hashes. The first reservation that differs is the first of the tie
    ///   digest.
    /// * Ordered: the metered encode of the first entry in list order, which the tie-break of
    ///   `head_metered` encodes first.
    async fn step(&self, host: &ParRSpace) -> (Vec<Reservation>, usize) {
        let data = data_channel(SPACE, &tied());
        match (self.kind, self.side) {
            (Kind::Bag | Kind::Set, _) => {
                let view = host.get_data(&data).await;
                let tied_order = recorded_order(&view);
                let mut distinct = view.clone();
                let [_, second] = distinct.as_mut_slice() else {
                    panic!("the bag holds {} tied records", view.len());
                };
                second.a.random_state[0] ^= 1;
                second.source = Produce::create(&data, &second.a, second.persist);
                let distinct_order = recorded_order(&distinct);
                let start = tied_order
                    .iter()
                    .zip(&distinct_order)
                    .take_while(|(tied, distinct)| tied == distinct)
                    .count();
                assert_eq!(
                    distinct_order.len(),
                    start + 2,
                    "after the scans, the distinct order only allocates its two result buffers"
                );
                (tied_order, start)
            }
            (_, Side::Records) => {
                let view = host.get_data(&data).await;
                (recorded_encode(&view[0]), 0)
            }
            (_, Side::Waiters) => {
                let view = host.get_waiting_continuations(vec![data]).await;
                (recorded_encode(&view[0]), 0)
            }
        }
    }
}

/// The reservations of the canonical order of `view` (`canonical_data_order`), as a recording
/// meter sees them.
fn recorded_order(view: &[ParDatum]) -> Vec<Reservation> {
    let trace = RefCell::new(Vec::new());
    {
        let meter = |operations: usize, scanned: usize, backing: usize| {
            trace.borrow_mut().push((operations, scanned, backing));
            Ok::<(), RSpaceError>(())
        };
        canonical_data_order(view, &meter as &dyn SourceMeter)
            .expect("an unlimited meter accepts every reservation");
    }
    trace.into_inner()
}

/// The reservations of the metered encode of `value`, as a recording meter sees them.
fn recorded_encode<T: serde::Serialize>(value: &T) -> Vec<Reservation> {
    let trace = RefCell::new(Vec::new());
    {
        let meter = |operations: usize, scanned: usize, backing: usize| {
            trace.borrow_mut().push((operations, scanned, backing));
            Ok::<(), RSpaceError>(())
        };
        metered_encode(value, &meter).expect("an unlimited meter accepts every reservation");
    }
    trace.into_inner()
}

/// The `VerificationBytes` bytes that the reservations `range` of `step` read.
fn read_bytes(step: &[Reservation], range: std::ops::Range<usize>) -> u64 {
    step[range]
        .iter()
        .map(|(_, scanned, _)| u64::try_from(*scanned).expect("a small read"))
        .sum()
}

/// The exact cut of an operation at its serialization step: the `VerificationBytes` usage of the
/// operation before the first reservation of the step that reads bytes, and the bytes of that
/// reservation. A budget of `usage` bytes ends exactly at that reservation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Cut {
    /// The `VerificationBytes` usage before the first read of the step.
    usage: u64,
    /// The bytes of the first read of the step.
    requested: u64,
}

/// The rejection that a budget of `limit` `VerificationBytes` reports when the reservation of
/// `requested` bytes after a usage of `usage` bytes exceeds it.
fn rejection(limit: u64, usage: u64, requested: u64) -> HostWorkReservationError {
    HostWorkReservationError::LimitExceeded {
        dimension: HostWorkDimension::VerificationBytes,
        limit: HostWorkLimit::new(limit),
        usage: HostWorkUsage::new(usage),
        requested: HostWorkUnits::new(requested),
    }
}

/// A host-work budget whose `VerificationBytes` limit is `limit`. Every other dimension is
/// unbounded.
fn bytes_budget(limit: u64) -> HostWorkBudget {
    let mut limits = HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX));
    limits.set(
        HostWorkDimension::VerificationBytes,
        HostWorkLimit::new(limit),
    );
    HostWorkBudget::new(limits)
}

/// The `VerificationBytes` that `tie.op()` reads on a fresh tie with an unbounded budget.
async fn bytes_read(tie: Tie) -> u64 {
    let host = play().await;
    tie.build(&host).await;
    let work = unbounded_host_work();
    let env = metered_env(host.clone(), &budget(), &work);
    run(&FamilyAgent, &env, tie.op())
        .await
        .expect("the selection of a tie commits");
    work.usage(HostWorkDimension::VerificationBytes).get()
}

/// The run of `tie.op()` on a fresh tie under a `VerificationBytes` budget of `limit`: the
/// rejection of the budget, or `None` when the operation commits, and the usage of the three
/// dimensions that an agent charges when the run ends. A rejected operation fails with
/// `HostWorkRejected`.
async fn run_under(tie: Tie, limit: u64) -> (Option<HostWorkReservationError>, [u64; 3]) {
    let host = play().await;
    tie.build(&host).await;
    let work = bytes_budget(limit);
    let env = metered_env(host.clone(), &budget(), &work);
    let result = run(&FamilyAgent, &env, tie.op()).await;
    let usages = AGENT_DIMENSIONS.map(|dimension| work.usage(dimension).get());
    match (result, work.rejection()) {
        (Ok(_), None) => (None, usages),
        (Err(SpaceError::Host(RSpaceError::HostWorkRejected)), Some(rejected)) => {
            (Some(rejected), usages)
        }
        (result, rejected) => panic!(
            "{tie:?}: a run under a byte budget commits or is rejected: {result:?}, {rejected:?}"
        ),
    }
}

/// The rejection of `tie.op()` on a fresh tie under a `VerificationBytes` budget of `limit`, or
/// `None` when the operation commits.
async fn rejection_at(tie: Tie, limit: u64) -> Option<HostWorkReservationError> {
    run_under(tie, limit).await.0
}

/// The recorded serialization step of `tie.op()` on a fresh tie, and the index of its first
/// reservation.
async fn recorded_step(tie: Tie) -> (Vec<Reservation>, usize) {
    let host = play().await;
    tie.build(&host).await;
    tie.step(&host).await
}

/// Finds the exact cut of `tie.op()` at its serialization step.
///
/// 1. Two ties that differ only in the length of their payloads make the same reservations up to
///    the first read of a payload. The prefix of the operation reads no payload. The order work
///    of a bag reads only source hashes before the tie, and an envelope decode reads only the
///    envelope.
/// 2. The first payload read lies in the serialization step: in the digest of the bag tie, or in
///    the encode of the ordered tie-break.
/// 3. A budget below the usage before that read stops both runs at one reservation, with equal
///    usages in all three dimensions. From that read on, the two runs differ.
/// 4. A rejection alone can agree after that read. An encode writes a byte vector one byte at a
///    time, so both runs can stop at a one-byte write at one usage. Their usages of operations
///    differ there, because the run with the shorter payload made more writes to reach that
///    usage.
/// 5. So a binary search over the limit, which compares the rejections and the usages, finds the
///    usage before the payload read. The rejections of both ties at that limit confirm it: each
///    run stops exactly at its recorded payload read.
/// 6. The recorded step gives the reads of the step before its payload read. So the cut is the
///    usage before the first read of the step, and the bytes of that read.
async fn find_cut(tie: Tie) -> Cut {
    let longer = Tie {
        length: tie.length + LONGER_BY,
        ..tie
    };
    let (step, start) = recorded_step(tie).await;
    let (longer_step, longer_start) = recorded_step(longer).await;
    assert_eq!(
        longer_start, start,
        "{tie:?}: the step starts at one reservation"
    );
    let payload_read = (start..step.len().min(longer_step.len()))
        .find(|index| step[*index] != longer_step[*index])
        .expect("the step reads the payload");
    assert_ne!(
        step[payload_read].1, longer_step[payload_read].1,
        "{tie:?}: the first reservation that depends on the payload reads it"
    );
    let first_read = (start..step.len())
        .find(|index| step[*index].1 > 0)
        .expect("the step reads bytes");
    assert!(first_read <= payload_read);

    let mut equal = 0;
    let mut differ = bytes_read(tie).await.min(bytes_read(longer).await);
    assert_eq!(
        run_under(tie, equal).await,
        run_under(longer, equal).await,
        "{tie:?}: an empty budget stops both ties at their first read"
    );
    assert_ne!(
        run_under(tie, differ).await,
        run_under(longer, differ).await,
        "{tie:?}: the budget of the shorter tie stops only the longer tie"
    );
    while differ - equal > 1 {
        let middle = equal + (differ - equal) / 2;
        match run_under(tie, middle).await == run_under(longer, middle).await {
            true => equal = middle,
            false => differ = middle,
        }
    }
    let bytes_of = |reservation: &Reservation| u64::try_from(reservation.1).expect("a small read");
    assert_eq!(
        rejection_at(tie, differ).await,
        Some(rejection(differ, differ, bytes_of(&step[payload_read]))),
        "{tie:?}: the search stops the tie at its payload read"
    );
    assert_eq!(
        rejection_at(longer, differ).await,
        Some(rejection(
            differ,
            differ,
            bytes_of(&longer_step[payload_read])
        )),
        "{tie:?}: the search stops the longer tie at its payload read"
    );
    Cut {
        usage: differ - read_bytes(&step, first_read..payload_read),
        requested: bytes_of(&step[first_read]),
    }
}

/// What a run of an operation under a budget shows: its result and the rejection of the budget.
#[derive(Debug, PartialEq)]
struct CutRun {
    /// The outcome of the operation, or its error.
    result: Result<SpaceOutcome, SpaceError>,
    /// The first rejection of the budget, or `None`.
    rejection: Option<HostWorkReservationError>,
}

/// Runs `op` over `host` (a play or a replay space, through `env_of`) with a `VerificationBytes`
/// budget of `limit`, inside a legacy scope, and asserts that a rejected run changes nothing: no
/// row, no event, no legacy charge and no phlo.
async fn cut_run(
    host: &impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    env_of: impl Fn(&RuntimeBudget, &HostWorkBudget) -> SpaceEnv,
    op: SpaceOp,
    limit: u64,
) -> CutRun {
    let channels = space_channels(&[tied(), context()]);
    let budget = RuntimeBudget::new(Cost::create(LEGACY_PHLO, "rrs determinism cut"));
    let _scope = budget.enter_legacy_scope();
    let work = bytes_budget(limit);
    let env = env_of(&budget, &work);
    host.take_event_log().await;
    let rows = rows_of(host, &channels).await;
    let (charges, phlo) = (budget.get_log().len(), budget.get());
    let result = run(&FamilyAgent, &env, op).await;
    if result.is_err() {
        assert!(
            host.take_event_log().await.is_empty(),
            "a rejected selection makes no event"
        );
        assert_eq!(
            rows_of(host, &channels).await,
            rows,
            "a rejected selection changes no row"
        );
        assert_eq!(
            budget.get_log().len(),
            charges,
            "a rejected selection makes no observer call"
        );
        assert_eq!(budget.get(), phlo, "a rejected selection costs no phlo");
    }
    CutRun {
        result,
        rejection: work.rejection(),
    }
}

/// Runs `tie.op()` on a fresh play space with a budget that ends at `cut`, and asserts the exact
/// rejection, no change of the store and the root of the tie. The control run allows the first
/// read of the step, and it stops at a later read.
async fn assert_cut_on_play(tie: Tie, cut: Cut) {
    let host = play().await;
    tie.build(&host).await;
    let twin = play().await;
    tie.build(&twin).await;
    let expected_root = twin
        .create_checkpoint()
        .await
        .expect("a checkpoint of the tie")
        .root;
    let env_of =
        |phlo: &RuntimeBudget, work: &HostWorkBudget| metered_env(host.clone(), phlo, work);
    assert_eq!(
        cut_run(&host, env_of, tie.op(), cut.usage).await,
        CutRun {
            result: Err(SpaceError::Host(RSpaceError::HostWorkRejected)),
            rejection: Some(rejection(cut.usage, cut.usage, cut.requested)),
        },
        "{tie:?}: the budget ends exactly at the first read of the serialization step"
    );
    assert_eq!(
        host.create_checkpoint()
            .await
            .expect("a checkpoint after the rejection")
            .root,
        expected_root,
        "{tie:?}: the rejection keeps the root"
    );
    let control = play().await;
    tie.build(&control).await;
    let env_of =
        |phlo: &RuntimeBudget, work: &HostWorkBudget| metered_env(control.clone(), phlo, work);
    let passed = cut_run(&control, env_of, tie.op(), cut.usage + cut.requested).await;
    match passed.rejection {
        Some(HostWorkReservationError::LimitExceeded { usage, .. }) => assert!(
            usage.get() > cut.usage,
            "{tie:?}: one more read passes the first read of the step"
        ),
        other => panic!("{tie:?}: the control stops on its bytes: {other:?}"),
    }
}

/// The fill of the candidate that `tie.op()` binds on a fresh tie with an unbounded budget. The
/// operation fires exactly once.
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

/// The cut of `tie` on a rigged replay space and after a checkpoint reset. The play space runs
/// the send of the context collection, which fires and commits, and then the cut selection,
/// which fails. The replay space is rigged with the log of that play. It runs the two operations
/// again: the send pairs its COMM, and the cut selection fails before it stages anything, so
/// every rigged event is used and the replay reaches the root of the play. Then the play space
/// resets to the checkpoint of the tie, where the cut selection fails again and the selection
/// with enough budget commits.
async fn assert_cut_on_replay_and_reset(tie: Tie, cut: Cut) {
    let (play_space, replay_space) = play_and_replay().await;
    tie.build(&play_space).await;
    let start = play_space
        .create_checkpoint()
        .await
        .expect("a checkpoint of the tie")
        .root;
    let context_send = send(&context(), 1, 1);
    let fired = run(
        &FamilyAgent,
        &env_over(play_space.clone(), &budget()),
        context_send.clone(),
    )
    .await
    .expect("the context send commits");
    assert_eq!(fired.fires.len(), 1, "the context send fires its waiter");
    let context_events = play_space.take_event_log().await;
    assert!(
        context_events
            .iter()
            .any(|event| matches!(event, Event::Comm(_))),
        "the play log holds the COMM of the context send"
    );
    let env_of =
        |phlo: &RuntimeBudget, work: &HostWorkBudget| metered_env(play_space.clone(), phlo, work);
    let expected = CutRun {
        result: Err(SpaceError::Host(RSpaceError::HostWorkRejected)),
        rejection: Some(rejection(cut.usage, cut.usage, cut.requested)),
    };
    assert_eq!(
        cut_run(&play_space, env_of, tie.op(), cut.usage).await,
        expected,
        "{tie:?}: play after the context send"
    );
    let played = play_space
        .create_checkpoint()
        .await
        .expect("a checkpoint of the play");
    assert!(
        played.log.is_empty(),
        "the rejected selection adds no event to the play log"
    );

    replay_space
        .rig_and_reset(start.clone(), context_events)
        .await
        .expect("the replay rigs the play log");
    let replayed = run(
        &FamilyAgent,
        &env_over(replay_space.clone(), &budget()),
        context_send,
    )
    .await
    .expect("the context send replays");
    assert_eq!(replayed, fired, "the replay of the context send");
    let env_of =
        |phlo: &RuntimeBudget, work: &HostWorkBudget| metered_env(replay_space.clone(), phlo, work);
    assert_eq!(
        cut_run(&replay_space, env_of, tie.op(), cut.usage).await,
        expected,
        "{tie:?}: the rigged replay"
    );
    replay_space
        .check_replay_data()
        .await
        .expect("the replay uses every rigged event");
    assert_eq!(
        replay_space
            .create_checkpoint()
            .await
            .expect("a checkpoint of the replay")
            .root,
        played.root,
        "{tie:?}: the replay reaches the root of the play"
    );

    play_space
        .reset(&start)
        .await
        .expect("the reset to the checkpoint of the tie");
    let env_of =
        |phlo: &RuntimeBudget, work: &HostWorkBudget| metered_env(play_space.clone(), phlo, work);
    assert_eq!(
        cut_run(&play_space, env_of, tie.op(), cut.usage).await,
        expected,
        "{tie:?}: after the checkpoint reset"
    );
    assert_eq!(
        play_space
            .create_checkpoint()
            .await
            .expect("a checkpoint after the reset")
            .root,
        start,
        "{tie:?}: the rejection after the reset keeps the root"
    );
    let outcome = run(
        &FamilyAgent,
        &env_over(play_space.clone(), &budget()),
        tie.op(),
    )
    .await
    .expect("the selection with enough budget commits after the reset");
    assert_eq!(outcome.fires.len(), 1, "{tie:?}: the selection fires");
}

/// The bag orders two byte-identical records by their digests (DR-75, decision 41). A budget that
/// ends exactly at the first read of the tie digest makes the receive fail with
/// `HostWorkRejected` before it stages anything: the rows, the legacy charges, the events and the
/// root stay unchanged, on a play space, on a rigged replay space and after a checkpoint reset.
/// The failure is typed, and no index order takes over (`rrs-spaces-determinism-serialization-
/// failure`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_candidate_serialize_failure_bag() {
    let tie = Tie {
        kind: Kind::Bag,
        side: Side::Records,
        length: TIE_LENGTH,
        reversed: false,
    };
    let cut = find_cut(tie).await;
    println!("{tie:?}: {cut:?}");
    assert!(cut.requested > 0);
    assert_cut_on_play(tie, cut).await;
    assert_cut_on_replay_and_reset(tie, cut).await;
    assert_eq!(selected(tie).await, 't', "the twin record fires");
}

/// A queue, a stack and a cell order two entries with one `ord` by their bincode bytes. On each
/// side and in each arrival order, a budget that ends exactly at the first read of the metered
/// encode of the tie-break makes the selection fail with `HostWorkRejected` before it stages
/// anything, on a play space, on a rigged replay space and after a checkpoint reset. With enough
/// budget the bytes select the candidate in both arrival orders: the least bytes for a queue and
/// a cell, the greatest for a stack. So a failing candidate never wins by its arrival order.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_candidate_serialize_failure_ordered() {
    for kind in Kind::ORDERED {
        for side in [Side::Records, Side::Waiters] {
            let mut cuts = Vec::with_capacity(2);
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
                let cut = find_cut(tie).await;
                println!("{tie:?}: {cut:?}");
                assert_cut_on_play(tie, cut).await;
                cuts.push(cut);
            }
            assert_eq!(
                cuts[0], cuts[1],
                "{kind:?} {side:?}: the cut does not depend on the arrival order"
            );
            let tie = Tie {
                kind,
                side,
                length: TIE_LENGTH,
                reversed: false,
            };
            assert_cut_on_replay_and_reset(tie, cuts[0]).await;
        }
    }
}

/// A set orders two byte-identical records by their digests (DR-75), as the bag does (wave W3).
/// A budget can end exactly at the first read of the tie digest. Then the class fire fails with
/// `HostWorkRejected` before it stages anything. This holds on a play space, on a rigged replay
/// space and after a checkpoint reset.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_candidate_serialize_failure_set() {
    let tie = Tie {
        kind: Kind::Set,
        side: Side::Records,
        length: TIE_LENGTH,
        reversed: false,
    };
    let cut = find_cut(tie).await;
    println!("{tie:?}: {cut:?}");
    assert!(cut.requested > 0);
    assert_cut_on_play(tie, cut).await;
    assert_cut_on_replay_and_reset(tie, cut).await;
    assert_eq!(selected(tie).await, 't', "the class of the twins fires");
}

/// A priority queue orders two entries with one `ord` in one level by their bincode bytes, as
/// the queue does. These are the ties of a priority query (wave W3). Take each side and each arrival order.
/// A budget can end exactly at the first read of the metered encode of the tie-break. Then the
/// selection fails with `HostWorkRejected` before it stages anything. This holds on a play space,
/// on a rigged replay space and after a checkpoint reset. With enough budget, the least bytes
/// select the candidate in both arrival orders.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_candidate_serialize_failure_priority() {
    for side in [Side::Records, Side::Waiters] {
        let mut cuts = Vec::with_capacity(2);
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
            let cut = find_cut(tie).await;
            println!("{tie:?}: {cut:?}");
            assert_cut_on_play(tie, cut).await;
            cuts.push(cut);
        }
        assert_eq!(
            cuts[0], cuts[1],
            "{side:?}: the cut does not depend on the arrival order"
        );
        let tie = Tie {
            kind: Kind::Pq,
            side,
            length: TIE_LENGTH,
            reversed: false,
        };
        assert_cut_on_replay_and_reset(tie, cuts[0]).await;
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

/// A build of a bag whose operations commute: sends of values in `0..6`, and receives that no
/// stored value fires, with literal patterns for values in `10..13`.
fn arb_commuting_bag_build() -> BoxedStrategy<Vec<Generated>> {
    let store = (arbitrary::<bool>(), 0i64..6, 0u64..3).prop_map(|(on_left, value, salt)| {
        Generated::Send {
            on_left,
            value,
            salt,
            persistent: false,
        }
    });
    let park = (
        arbitrary::<bool>(),
        0u64..1_000,
        10i64..13,
        arbitrary::<bool>(),
    )
        .prop_map(|(on_left, name, value, peek)| Generated::Receive {
            on_left,
            name,
            pattern: Some(value),
            peek,
            persistent: false,
        });
    vec(prop_oneof![3 => store, 2 => park], 2..10).boxed()
}

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
/// order of the history. The probe gives the same steps and the same root in all three, and the
/// same host work for an ordered discipline.
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
        prop_assert_eq!(other.without_work(), agent.without_work());
        prop_assert_eq!(&other.root, &agent.root);
        if kind != Kind::Bag {
            prop_assert_eq!(other.work(), agent.work());
        }
    }
    Ok(())
}

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

/// The coverage of `prop_canonical_iteration_bag_any_insertion_order`.
static BAG_COVERAGE: Coverage = Coverage::new();

/// The coverage of `prop_canonical_iteration_ordered_any_list_order`.
static ORDERED_COVERAGE: Coverage = Coverage::new();

/// `bag_visit_order_canonical`, `waiter_visit_order_canonical` and
/// `hashmap_iteration_permutation_invariant`, over generated bags: a build whose operations
/// commute gives one entry set in every order of its operations, and a generated probe gives the
/// same results, events, legacy charges and root for the order of the agent, for a permuted order
/// of the build, for a shuffled raw store and for the order of the history.
#[test]
fn prop_canonical_iteration_bag_any_insertion_order() {
    let strategy = (
        arb_commuting_bag_build().prop_flat_map(|build| {
            let shuffled = Just(build.clone()).prop_shuffle();
            (Just(build), shuffled)
        }),
        arbitrary::<u64>(),
        vec(arb_generated(), 1..8),
    );
    check(
        pure_config(),
        strategy,
        |((build, permuted), seed, probe)| {
            let build: Vec<SpaceOp> = build.into_iter().map(Generated::op).collect();
            let permuted: Vec<SpaceOp> = permuted.into_iter().map(Generated::op).collect();
            let probe: Vec<SpaceOp> = probe.into_iter().map(Generated::op).collect();
            tokio_runtime().block_on(async {
                let channels = space_channels(&[left(), right()]);
                let ordered = built(Kind::Bag, &build).await;
                let reordered = built(Kind::Bag, &permuted).await;
                prop_assert_eq!(
                    entry_set(&rows_of(&reordered, &channels).await),
                    entry_set(&rows_of(&ordered, &channels).await)
                );
                let first = observe(&ordered, &probe).await;
                let second = observe(&reordered, &probe).await;
                prop_assert_eq!(second.without_work(), first.without_work());
                prop_assert_eq!(&second.root, &first.root);
                generated_list_orders(Kind::Bag, &build, seed, &probe, &BAG_COVERAGE).await
            })
        },
    );
    BAG_COVERAGE.assert_not_vacuous();
}

/// `stack_head_is_max_ord`, `stack_plan_permutation_invariant`, the head rules of the queue and
/// of the cell, and `hashmap_iteration_permutation_invariant`, over generated queues, stacks and
/// cells: the entry set of a generated build gives the same results, events, legacy charges,
/// host work and root for a generated probe in the order of the agent, in a shuffled raw store
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

/// `scan_trace_covered`, `settle_trace_covered` and `engine_primitives_trace_covered`, at the
/// serialization step: for generated disciplines, sides, arrival orders and payload lengths, a
/// budget that ends exactly at the first read of the step rejects the selection before it stages
/// anything, and one more read lets it pass that read. Wave W3 adds the set (the tie digest of
/// DR-75) and the priority queue (the ties of a priority query).
#[test]
fn prop_candidate_serialize_failure_cut_is_exact() {
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
            tokio_runtime().block_on(async {
                let cut = find_cut(tie).await;
                prop_assert!(cut.requested > 0);
                assert_cut_on_play(tie, cut).await;
                Ok(())
            })
        },
    );
}
