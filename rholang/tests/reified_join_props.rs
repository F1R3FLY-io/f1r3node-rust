//! The properties of the join engine of the native spaces (P5c, joins plan section 7.3).
//!
//! Each property extracts one statement of joins plan section 8: a headline of `JoinCascade.v`,
//! an invariant of `JoinCascade.tla`, or an addition to `RecordEnvelope.v` or `CounterConflict.v`.
//! The engine anchor is the last property. The doc comment of each property names its statement.
//! The properties run the family agent over an in-memory play space. They use the runner of the
//! P5 suites, with 64 cases and the fixed seed of `support::props`.
//!
//! The reference of the properties is the model of joins plan section 3. The model has the
//! windows of each discipline, the head conditions, the greedy search `GREEDY` and the
//! produce-side search `SEARCH-PRODUCE`. It also has the invariants `INV_head` and `INV_scan`. The
//! model reads the store with the decoders of the records. It computes each choice from the
//! records, the orders and the source hashes alone, and it shares no code with the engine.
//!
//! Added by decision 102 (plan v4 C5.3, rule CR-1): on the bag and the set, play picks the record
//! of a collection that the group of a join names once at random, and OFFER meets the waiters of
//! a collection in a random order (decision 124, LP-1). So the model gives the set of outcomes
//! that play can reach ([`choices`], [`offer_outcomes`]), and a property asserts that the outcome
//! of the engine lies in that set. No property makes a probabilistic assertion. A collection that
//! the group names two or more times keeps the canonical order of DR-75 (E1, rule CR-8), and so
//! does the search of a cascade (rule F3).
//!
//! Added by decision 103 (decision 118, rule CR-9): the search of the model follows rule (a) of
//! the claim rule ([`search`]). A persistent entry claims nothing, so it can bind two or more
//! positions. A non-persistent entry claims its record, and on a set also its class. A later
//! non-persistent member of a claimed class binds only when the class has a persistent member. A
//! persistent element in flight also binds the later positions of its collection.
//!
//! | Property | Statement |
//! |---|---|
//! | `prop_join_search_is_head_greedy` | `join_search_is_head_greedy` (headline 1) |
//! | `prop_join_produce_search_agrees_with_head` | `join_produce_search_agrees_with_head` (headline 2) |
// Changed by decision 103 (decision 118, rule CR-9, decision-c6 §6 item 4): a persistent element
// in flight can bind two or more positions. The row before:
// //! | `prop_join_incoming_binds_once` | `join_incoming_binds_once` (headline 3), TLA `IncomingBoundOnce` |
//! | `prop_join_incoming_held` | `join_incoming_held` (headline 18), TLA `IncomingHeld` |
//! | `prop_join_fires_heads_only` | `join_fires_heads_only` (headline 4), TLA `JoinFiresHeadsOnly` |
// Changed by decision 103 (decision 118, rule CR-9, decision-c6 §6 item 4): a persistent record
// can bind two or more positions. The row before:
// //! | `prop_join_repeated_channel_distinct_records` | `join_repeated_channel_distinct_records` (headline 5), TLA `DistinctRecordsPerBind` |
//! | `prop_join_nonpersistent_record_binds_once` | `join_nonpersistent_bound_once` (headline 19), TLA `NonPersistentBoundOnce` |
//! | `prop_join_set_bind_retires_class` | `join_set_bind_retires_class` (headline 6) |
//! | `prop_cascade_terminates_any_schedule` | `cascade_terminates` (headline 7) |
//! | `prop_join_txn_fires_bounded_at_the_cap` | `join_txn_fires_bounded` (headline 8), TLA `TxnFiresBounded` |
//! | `prop_cascade_restores_inv_any_schedule` | `cascade_restores_inv` (headline 9) |
//! | `prop_cascade_order_independent` | `cascade_order_independent` (headline 10) |
//! | `prop_lift_reoffer_equals_direct_comm` | `lift_reoffer_equals_direct_comm` (headline 11) |
//! | `prop_cascade_bumps_touched_counters` | `cascade_bumps_touched_counters` (headline 12), TLA `TouchedCountersBumped` |
//! | `prop_bind_permutation_restores_order` | `bind_permutation_restores_order` (headline 13) |
//! | `prop_plan_then_lock_covers_every_staged_channel` | `plan_then_lock_stable` (headline 14) |
//! | `prop_join_reissue_keeps_bind_ords` | `join_reissue_keeps_bind_ords` (headline 15), TLA `ReissueKeepsBindOrds` |
//! | `prop_peek_join_restores_stored_candidates` | `peek_join_restores_stored_candidates` (headline 16) |
//! | `prop_greedy_join_limit` | `greedy_join_can_miss_assignment` (headline 17) |
//! | `prop_continuation_v2_roundtrip` | `continuation_v2_roundtrip` (`RecordEnvelope.v`) |
//! | `prop_join_ops_race_on_every_counter` | `join_ops_race_on_every_counter` (`CounterConflict.v`) |
//! | `prop_no_record_lost_any_schedule` | TLA `NoRecordLost` |
//! | `prop_quiescent_no_pending_match_any_schedule` | TLA `QuiescentNoPendingMatch` |
//! | `prop_cascade_measure_decreases` | TLA `CascadeMeasureDecreases` |
//! | `prop_racing_jobs_equal_a_serial_order` | TLA `NoStaleScope`, `NoFalseRefusal`, `NoDeadlock` (`JoinScopeLock.tla`, decision 94) |
//! | `prop_join_engine_agrees_with_drivers_without_joins` | the engine anchor (joins plan 7.3) |
// Retired by decision 125 (the step D restatement at the failing step): the golden and the stored
// logs of `join_seams_keyed_differential` predate decision 118, its replay half needs the legacy
// replay mode that step D retires, and the oracle properties of this file now serve its purpose,
// the guard of the store seams of joins plan 9. The row and the paragraph before:
// //! | `join_seams_keyed_differential` | the keyed engine is unchanged by the store seams (joins plan 9) |
// //!
// //! The keyed differential is not a property. It runs a fixed corpus of generated programs over a
// //! recording host and compares the digest of each run with the golden
// //! `test-vectors/reified-join-seams-v1.json`. To capture the golden, set
// //! `RRS_CAPTURE_JOIN_SEAMS_OUT` to an absolute directory under `target/` and copy the file that
// //! the run writes there.

#[path = "support/mod.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet};
// Retired by decision 125 (the step D restatement at the failing step): only the retired seams
// differential used these imports.
// use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{BindPattern, EEq, EGt, Expr, ListParWithRandom, Par};
use models::rust::space_factory_urn::{SpaceFactoryUrn, SpaceInner, SpaceOuter};
use models::rust::utils::new_boundvar_par;
use proptest::prelude::*;
// Changed by decision 125 (the step D restatement at the failing step): only the retired seams
// differential drew programs with a `TestRunner` and a `ValueTree`. The text before:
// use proptest::strategy::ValueTree;
// use proptest::test_runner::{TestCaseError, TestRunner};
use proptest::test_runner::TestCaseError;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::RuntimeBudget;
use rholang::rust::interpreter::reified::costs::{ReissueCause, FIRE_ROUND_CAP};
use rholang::rust::interpreter::spaces::agent::{
    CascadeState, ConsumeOp, JoinOp, ProduceOp, QueryParams, SettleOp, SpaceEnv, SpaceOp,
    SpaceOutcome, SpaceReissue,
};
use rholang::rust::interpreter::spaces::disciplines::bag::BagDiscipline;
use rholang::rust::interpreter::spaces::disciplines::cell::CellDiscipline;
use rholang::rust::interpreter::spaces::disciplines::priority_queue::{
    PriorityQueueDiscipline, PRIORITY_PARAM,
};
use rholang::rust::interpreter::spaces::disciplines::queue::QueueDiscipline;
use rholang::rust::interpreter::spaces::disciplines::set::SetDiscipline;
use rholang::rust::interpreter::spaces::disciplines::stack::StackDiscipline;
use rholang::rust::interpreter::spaces::errors::SpaceError;
use rholang::rust::interpreter::spaces::families::FamilyAgent;
use rholang::rust::interpreter::spaces::generic::{
    begin_collection, Discipline, FamilySpace, GenericSpace,
};
use rholang::rust::interpreter::spaces::join::JoinRoute;
use rholang::rust::interpreter::spaces::namespace::{
    CollectionKey, Durability, NativeSpace, SID_LEN,
};
use rholang::rust::interpreter::spaces::outer::hashmap::HashMapOuter;
use rholang::rust::interpreter::spaces::record::{self, JoinSlot, WaiterPolicy};
use rholang::rust::interpreter::spaces::types::qualifier::SpaceQualifier;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex;
use rspace_plus_plus::rspace::merger::merging_logic::{conflicts, NumberChannelsDiff};
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::space_store_handle::{ConsumeOutcome, ProduceOutcome};
use rspace_plus_plus::rspace::trace::event::{Event, Produce};
use support::agent::{
    body_name, budget, config_of, counter_channel, create, creation, data_channel, env_over, int,
    join_on, play, receive_on, run, send_on, string, HostRecord, ParRSpace, RecordingHost,
    SharedRecord, StagedCopy,
};
use support::observed::observed;
use support::props::{check, pure_config, tokio_runtime};

// ─── The space ───────────────────────────────────────────────────────────────────────────────

/// The space of every property.
const SPACE: NativeSpace = NativeSpace {
    sid: [0x5A; SID_LEN],
    durability: Durability::Durable,
};

/// The number of collections of a generated program.
const COLLS: usize = 3;

/// The inner channel of the collection `coll`.
fn ch(coll: usize) -> Par { string(&format!("rrs-join-prop-{coll}")) }

/// The data channel `D(sid,k)` of the collection `coll`.
fn data(coll: usize) -> Par { data_channel(SPACE, &ch(coll)) }

/// The key of the collection `coll`.
fn key(coll: usize) -> CollectionKey {
    CollectionKey::of_channel(&ch(coll)).expect("a channel has a collection key")
}

/// The discipline of a generated space, with the shape of its join windows (joins plan 3.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// `hashmap:queue`: the head window, the oldest entry first.
    Queue,
    /// `hashmap:stack`: the head window, the newest entry first.
    Stack,
    /// `hashmap:cell`: the head window of one record and one waiter.
    Cell,
    /// `hashmap:bag`: the scan window, in the canonical order of DR-75.
    Bag,
    /// `hashmap:set`: the class window, one representative for each class.
    Set,
    /// `hashmap:priorityqueue`: the lane window, the first records of each level.
    Pq,
}

/// The join window of a discipline (joins plan 3.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Win {
    /// The first `m` entries of the head order.
    Head,
    /// For each level that the lane of a bind sees, the first `m` records of the level.
    Lanes,
    /// Every record in the canonical order of DR-75.
    /// Added by decision 102 (plan v4 C5.3, rule CR-1): in play, a bind on a collection that the
    /// group names once takes a random record.
    Scan,
    /// One representative for each class, the first member in the canonical order of DR-75.
    /// Added by decision 102 (plan v4 C5.3, rule CR-1): in play, a bind on a collection that the
    /// group names once takes a random member.
    Classes,
}

impl Kind {
    /// Every discipline.
    const ALL: [Kind; 6] = [
        Kind::Queue,
        Kind::Stack,
        Kind::Cell,
        Kind::Bag,
        Kind::Set,
        Kind::Pq,
    ];

    /// The inner discipline of `hashmap:<inner>`.
    fn inner(self) -> SpaceInner {
        match self {
            Kind::Queue => SpaceInner::Queue,
            Kind::Stack => SpaceInner::Stack,
            Kind::Cell => SpaceInner::Cell,
            Kind::Bag => SpaceInner::Bag,
            Kind::Set => SpaceInner::Set,
            Kind::Pq => SpaceInner::PriorityQueue,
        }
    }

    /// The join window of the discipline.
    fn win(self) -> Win {
        match self {
            Kind::Queue | Kind::Stack | Kind::Cell => Win::Head,
            Kind::Pq => Win::Lanes,
            Kind::Bag => Win::Scan,
            Kind::Set => Win::Classes,
        }
    }

    /// True when the discipline keeps a counter for each collection (decision 31).
    fn counted(self) -> bool { matches!(self, Kind::Queue | Kind::Stack | Kind::Cell | Kind::Pq) }

    /// True when the invariant after a quiescent transaction is the strong `INV_head`: the head
    /// shapes and the set (decision 50). The bag keeps the weak `INV_scan`.
    fn strong(self) -> bool { !matches!(self, Kind::Bag) }
}

/// A space of one discipline over a fresh in-memory play space, run by the family agent.
struct Rig {
    /// The play space.
    host: ParRSpace,
    /// The environment of the jobs.
    env: SpaceEnv,
    /// The discipline.
    kind: Kind,
}

impl Rig {
    /// A rig of `hashmap:<kind>` over a fresh play space.
    async fn new(kind: Kind) -> Rig { Rig::over(play().await, kind).await }

    /// A rig of `hashmap:<kind>` over the play space `host`, which must be fresh.
    async fn over(host: ParRSpace, kind: Kind) -> Rig {
        let env = env_over(host.clone(), &budget());
        let config = config_of(
            SPACE,
            SpaceOuter::HashMap.as_str(),
            kind.inner().as_str(),
            SpaceQualifier::Default,
        );
        let family = SpaceFactoryUrn {
            outer: SpaceOuter::HashMap,
            inner: kind.inner(),
        };
        create(&FamilyAgent, &env, creation(SPACE, config, family)).await;
        host.take_event_log().await;
        Rig { host, env, kind }
    }

    /// A rig of `hashmap:<kind>` whose agent works over a recording host, and the record of that
    /// host. The record starts after the creation of the space.
    async fn recording(kind: Kind) -> (Rig, SharedRecord) {
        let host = play().await;
        let (recording, record) = RecordingHost::new(host.clone());
        let env = env_over(recording, &budget());
        Rig::recorded(host, env, record, kind).await
    }

    /// Creates the space of `hashmap:<kind>` in `env` over the play space `host`, and clears the
    /// event log and the record `record` after the creation.
    async fn recorded(
        host: ParRSpace,
        env: SpaceEnv,
        record: SharedRecord,
        kind: Kind,
    ) -> (Rig, SharedRecord) {
        let config = config_of(
            SPACE,
            SpaceOuter::HashMap.as_str(),
            kind.inner().as_str(),
            SpaceQualifier::Default,
        );
        let family = SpaceFactoryUrn {
            outer: SpaceOuter::HashMap,
            inner: kind.inner(),
        };
        create(&FamilyAgent, &env, creation(SPACE, config, family)).await;
        host.take_event_log().await;
        *record.lock().expect("the record lock") = HostRecord::default();
        (Rig { host, env, kind }, record)
    }

    /// Runs `op` in one job.
    async fn run(&self, op: SpaceOp) -> Result<SpaceOutcome, SpaceError> {
        run(&FamilyAgent, &self.env, op).await
    }

    /// The events since the last call.
    async fn log(&self) -> Vec<Event> { self.host.take_event_log().await }

    /// The value of the counter of the collection `coll`, or `None` before its first operation.
    async fn counter(&self, coll: usize) -> Option<i64> {
        match self
            .host
            .get_data(&counter_channel(SPACE, &ch(coll)))
            .await
            .as_slice()
        {
            [] => None,
            [datum] => Some(record::counter_value(&datum.a).expect("a counter record")),
            other => panic!("the counter channel holds {} records", other.len()),
        }
    }

    /// The root of a checkpoint of the play space. The checkpoint takes the event log.
    async fn root(&self) -> Blake2b256Hash {
        self.host
            .create_checkpoint()
            .await
            .expect("a checkpoint")
            .root
    }
}

// ─── Programs ────────────────────────────────────────────────────────────────────────────────

/// The pattern of a bind: any one value, or one number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pat {
    /// Binds any one value.
    Any,
    /// Matches exactly this number.
    Is(i64),
}

impl Pat {
    /// The bind pattern.
    fn pattern(self) -> BindPattern {
        match self {
            Pat::Any => support::agent::any(),
            Pat::Is(value) => support::agent::exactly(value),
        }
    }

    /// True when the pattern binds the number `value`.
    fn binds(self, value: i64) -> bool {
        match self {
            Pat::Any => true,
            Pat::Is(own) => own == value,
        }
    }
}

/// One bind of a receive: its collection, its pattern and its lane (a priority queue only).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Bind {
    coll: usize,
    pat: Pat,
    lane: Option<i64>,
}

/// The guard of a join whose patterns all bind any value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Guard {
    /// No guard.
    Open,
    /// The value of the last bind is greater than the value of the first bind.
    LastAboveFirst,
    /// The value of the last bind is the number.
    LastIs(i64),
}

impl Guard {
    /// The guard of a join of `binds` binds, as a process. The join has one variable in each
    /// bind. So the variable of the last bind is `0`, and the variable of the first bind is
    /// `binds - 1`.
    fn par(self, binds: usize) -> Option<Par> {
        let bound = |index: usize| {
            new_boundvar_par(i32::try_from(index).expect("few binds"), Vec::new(), false)
        };
        let expr = |instance: ExprInstance| Par {
            exprs: vec![Expr {
                expr_instance: Some(instance),
            }],
            ..Par::default()
        };
        match self {
            Guard::Open => None,
            Guard::LastAboveFirst => Some(expr(ExprInstance::EGtBody(EGt {
                p1: Some(bound(0)),
                p2: Some(bound(binds - 1)),
            }))),
            Guard::LastIs(value) => Some(expr(ExprInstance::EEqBody(EEq {
                p1: Some(bound(0)),
                p2: Some(int(value)),
            }))),
        }
    }

    /// The guard of the process `guard` of a waiter with `binds` binds.
    fn of(guard: Option<&Par>, binds: usize) -> Guard {
        let Some(guard) = guard else {
            return Guard::Open;
        };
        for candidate in (0..4).map(Guard::LastIs).chain([Guard::LastAboveFirst]) {
            if candidate.par(binds).as_ref() == Some(guard) {
                return candidate;
            }
        }
        panic!("a guard that the generator does not make: {guard:?}");
    }

    /// True when the guard admits the values `values`, in bind order.
    fn admits(self, values: &[i64]) -> bool {
        match (self, values.first(), values.last()) {
            (Guard::Open, _, _) => true,
            (Guard::LastAboveFirst, Some(first), Some(last)) => last > first,
            (Guard::LastIs(value), _, Some(last)) => *last == value,
            (Guard::LastAboveFirst | Guard::LastIs(_), _, _) => false,
        }
    }
}

/// One step of a generated program.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Step {
    /// A send of `value` on `coll`, at the level `level` on a priority queue.
    Send {
        coll: usize,
        value: i64,
        persistent: bool,
        level: Option<i64>,
    },
    /// A receive with one bind.
    Receive {
        bind: Bind,
        peek: bool,
        persistent: bool,
    },
    /// A join: a receive with two or more binds.
    Join {
        binds: Vec<Bind>,
        peek: bool,
        persistent: bool,
        guard: Guard,
    },
}

/// Which side of a program may be persistent. A program never has both, so no divergent pair
/// re-issues forever (A10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Persistence {
    /// Neither sends nor receives are persistent.
    Neither,
    /// Sends may be persistent.
    Sends,
    /// Receives and joins that do not peek may be persistent.
    Receives,
}

/// A generated program: its discipline and its steps.
#[derive(Clone, Debug)]
struct Program {
    kind: Kind,
    steps: Vec<Step>,
}

/// The body name of the receive or the join of the step `index`.
fn name(index: usize) -> String { format!("w{index}") }

/// The step index of a body name, or `None` for another name.
fn step_of(name: &str) -> Option<usize> { name.strip_prefix('w')?.parse().ok() }

/// The query of a bind in the lane `lane` of a priority queue.
fn lane_query(lane: Option<i64>) -> Option<QueryParams> {
    lane.map(|level| QueryParams {
        payload: Vec::new(),
        named: BTreeMap::from([(PRIORITY_PARAM.to_string(), int(level))]),
    })
}

/// A send of `value` on `coll` with a random state that `salt` fixes.
fn send(coll: usize, value: i64, persistent: bool, level: Option<i64>, salt: u64) -> ProduceOp {
    let mut op = send_on(SPACE, &ch(coll), value, persistent);
    op.data.random_state = Blake2b512Random::create_from_bytes(
        format!("rrs-join-prop {coll} {value} {salt}").as_bytes(),
    )
    .to_bytes();
    ProduceOp {
        params: level
            .map(|level| BTreeMap::from([(PRIORITY_PARAM.to_string(), int(level))]))
            .unwrap_or_default(),
        ..op
    }
}

/// The join of `binds` whose body is `name`.
fn join_of(binds: &[Bind], name: &str, peek: bool, persistent: bool, guard: Guard) -> JoinOp {
    let channels: Vec<Par> = binds.iter().map(|bind| ch(bind.coll)).collect();
    let pairs: Vec<(&Par, BindPattern)> = channels
        .iter()
        .zip(binds)
        .map(|(channel, bind)| (channel, bind.pat.pattern()))
        .collect();
    let mut op = join_on(SPACE, &pairs, name);
    for (join_bind, bind) in op.binds.iter_mut().zip(binds) {
        join_bind.query = lane_query(bind.lane);
    }
    JoinOp {
        peek,
        persistent,
        guard: guard.par(binds.len()),
        ..op
    }
}

/// The space operation of the step `step` with the index `index`.
fn space_op(step: &Step, index: usize) -> SpaceOp {
    match step {
        Step::Send {
            coll,
            value,
            persistent,
            level,
        } => SpaceOp::Produce(send(
            *coll,
            *value,
            *persistent,
            *level,
            u64::try_from(index).expect("a small index"),
        )),
        Step::Receive {
            bind,
            peek,
            persistent,
        } => SpaceOp::Consume(ConsumeOp {
            peek: *peek,
            persistent: *persistent,
            query: lane_query(bind.lane),
            ..receive_on(SPACE, &ch(bind.coll), &name(index), bind.pat.pattern())
        }),
        Step::Join {
            binds,
            peek,
            persistent,
            guard,
        } => SpaceOp::Join(join_of(binds, &name(index), *peek, *persistent, *guard)),
    }
}

/// The levels of a send and the lanes of a bind on the discipline `kind`.
fn arb_level(kind: Kind) -> BoxedStrategy<Option<i64>> {
    match kind {
        Kind::Pq => prop_oneof![Just(None), (0i64..2).prop_map(Some)].boxed(),
        Kind::Queue | Kind::Stack | Kind::Cell | Kind::Bag | Kind::Set => Just(None).boxed(),
    }
}

/// A pattern over the values `0..values`.
fn arb_pat(values: i64) -> impl Strategy<Value = Pat> {
    prop_oneof![2 => Just(Pat::Any), 1 => (0..values).prop_map(Pat::Is)]
}

/// A bind on one of `colls` collections.
fn arb_bind(kind: Kind, colls: usize, values: i64) -> impl Strategy<Value = Bind> {
    (0..colls, arb_pat(values), arb_level(kind)).prop_map(|(coll, pat, lane)| Bind {
        coll,
        pat,
        lane,
    })
}

/// A step of a program of `kind` over `colls` collections and the values `0..values`. A join
/// whose patterns all bind any value may have a guard when `guards` holds.
fn arb_step(
    kind: Kind,
    persistence: Persistence,
    colls: usize,
    values: i64,
    guards: bool,
) -> impl Strategy<Value = Step> {
    let send = (0..colls, 0..values, any::<bool>(), arb_level(kind)).prop_map(
        move |(coll, value, persistent, level)| Step::Send {
            coll,
            value,
            persistent: persistent && persistence == Persistence::Sends,
            level,
        },
    );
    let receive = (arb_bind(kind, colls, values), any::<bool>(), any::<bool>()).prop_map(
        move |(bind, peek, persistent)| Step::Receive {
            bind,
            peek,
            persistent: persistent && !peek && persistence == Persistence::Receives,
        },
    );
    let join = (
        prop::collection::vec(arb_bind(kind, colls, values), 2..=3),
        any::<bool>(),
        any::<bool>(),
        0u8..4,
        0..values,
    )
        .prop_map(move |(binds, peek, persistent, guard, value)| {
            let open = guards && binds.iter().all(|bind| bind.pat == Pat::Any);
            Step::Join {
                binds,
                peek,
                persistent: persistent && !peek && persistence == Persistence::Receives,
                guard: match (open, guard) {
                    (true, 1) => Guard::LastAboveFirst,
                    (true, 2) => Guard::LastIs(value),
                    _ => Guard::Open,
                },
            }
        });
    prop_oneof![3 => send, 1 => receive, 2 => join]
}

/// A program of one of `kinds` with `steps` steps over `colls` collections and the values
/// `0..values`. Its joins may have guards when `guards` holds.
fn arb_program(
    kinds: &'static [Kind],
    steps: std::ops::Range<usize>,
    colls: usize,
    values: i64,
    guards: bool,
) -> impl Strategy<Value = Program> {
    (prop::sample::select(kinds), prop_oneof![
        Just(Persistence::Neither),
        Just(Persistence::Sends),
        Just(Persistence::Receives)
    ])
        .prop_flat_map(move |(kind, persistence)| {
            prop::collection::vec(
                arb_step(kind, persistence, colls, values, guards),
                steps.clone(),
            )
            .prop_map(move |steps| Program { kind, steps })
        })
}

/// A failure of a property case.
fn failure(reason: impl std::fmt::Display) -> TestCaseError {
    TestCaseError::fail(reason.to_string())
}

/// The result of one job of a chain.
struct Job {
    result: Result<SpaceOutcome, SpaceError>,
}

/// The most jobs of one chain. A chain that runs longer does not terminate.
const MAX_CHAIN: usize = 4096;

/// Runs `op` and then every re-issue that follows it, as the jobs of one deploy do, and returns
/// each job. A job that fails ends the chain.
async fn chain(rig: &Rig, op: SpaceOp) -> Result<Vec<Job>, TestCaseError> {
    let mut jobs = Vec::new();
    let mut next = Some(op);
    while let Some(op) = next.take() {
        if jobs.len() >= MAX_CHAIN {
            return Err(failure(format!("a chain runs past {MAX_CHAIN} jobs")));
        }
        let result = rig.run(op).await;
        next = result
            .as_ref()
            .ok()
            .and_then(|outcome| outcome.reissue.as_ref())
            .map(|reissue| reissue.op.clone());
        jobs.push(Job { result });
    }
    Ok(jobs)
}

/// The fires of every job of `jobs` that committed.
fn fire_counts(jobs: &[Job]) -> Vec<usize> {
    jobs.iter()
        .map(|job| job.result.as_ref().map_or(0, |outcome| outcome.fires.len()))
        .collect()
}

/// Checks that a failed job of a program is an expected refusal with no effect. A cell refuses a
/// second record or a second waiter with `CellOccupied`.
fn expected_refusal(kind: Kind, error: &SpaceError) -> Result<(), TestCaseError> {
    match (kind, error) {
        (Kind::Cell, SpaceError::CellOccupied) => Ok(()),
        _ => Err(failure(format!(
            "a job of a {kind:?} program fails: {error:?}"
        ))),
    }
}

// ─── The model (joins plan section 3) ────────────────────────────────────────────────────────

/// A stored record as the model sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Rec {
    /// The order of the record.
    ord: i64,
    /// The level of a record of a priority queue: `Some(p)`, or `None` for no priority.
    level: Option<i64>,
    /// The number that the record holds.
    value: i64,
    /// True for a persistent record.
    persist: bool,
    /// The source hash of the record: its key in the canonical order of DR-75.
    hash: Blake2b256Hash,
}

/// A waiter as the model sees it: one entry for each position of its group.
#[derive(Clone, Debug, PartialEq)]
struct Wait {
    /// The source hash of the waiter.
    hash: Blake2b256Hash,
    /// The collection of each group position.
    colls: Vec<usize>,
    /// The pattern of each group position.
    pats: Vec<Pat>,
    /// The slot of each group position: its order, its bind index and its lane.
    slots: Vec<JoinSlot>,
    /// True for a persistent waiter.
    persist: bool,
    /// True for a waiter that peeks.
    peek: bool,
    /// The guard.
    guard: Guard,
    /// The body name.
    name: String,
}

/// The records and the waiters of the collections of a space.
#[derive(Clone, Debug)]
struct View {
    kind: Kind,
    /// The records of each collection.
    recs: Vec<Vec<Rec>>,
    /// Every waiter, each once.
    waits: Vec<Wait>,
}

/// The number of a payload of one number.
fn number(pars: &[Par]) -> i64 {
    match pars {
        [Par { exprs, .. }] => match exprs.as_slice() {
            [Expr {
                expr_instance: Some(ExprInstance::GInt(value)),
            }] => *value,
            other => panic!("a payload of one number: {other:?}"),
        },
        other => panic!("a payload of one number: {other:?}"),
    }
}

/// The pattern of a bind pattern that the generator makes.
fn pat_of(pattern: &BindPattern) -> Pat {
    match (pattern.patterns.as_slice(), pattern.free_count) {
        ([single], 1)
            if matches!(single.exprs.as_slice(), [Expr {
                expr_instance: Some(ExprInstance::EVarBody(_)),
            }]) =>
        {
            Pat::Any
        }
        ([single], 0) => Pat::Is(number(std::slice::from_ref(single))),
        _ => panic!("a pattern that the generator does not make: {pattern:?}"),
    }
}

/// The collection whose data channel is `channel`.
fn coll_of(channel: &Par) -> usize {
    (0..COLLS)
        .find(|coll| data(*coll) == *channel)
        .unwrap_or_else(|| panic!("a channel outside the collections: {channel:?}"))
}

/// Reads the view of the collections `0..COLLS` of `host`.
async fn read_view(host: &ParRSpace, kind: Kind) -> View {
    let mut recs = Vec::with_capacity(COLLS);
    let mut group_list: Vec<Vec<Par>> = Vec::new();
    for coll in 0..COLLS {
        let records = host
            .get_data(&data(coll))
            .await
            .into_iter()
            .map(|datum| {
                let decoded = record::unwrap_pq_datum(&datum.a).expect("a datum record");
                Rec {
                    ord: decoded.ord,
                    level: decoded.priority,
                    value: number(&decoded.datum.pars),
                    persist: datum.persist,
                    hash: datum.source.hash.clone(),
                }
            })
            .collect();
        recs.push(records);
        for group in host.get_joins(data(coll)).await {
            if !group_list.contains(&group) {
                group_list.push(group);
            }
        }
    }
    let mut waits = Vec::new();
    for group in group_list {
        let colls: Vec<usize> = group.iter().map(coll_of).collect();
        for waiter in host.get_waiting_continuations(group.clone()).await {
            let (slots, continuation) = match group.len() {
                1 => {
                    let parked = record::unwrap_pq_continuation(&waiter.continuation)
                        .expect("a continuation record");
                    (
                        vec![JoinSlot {
                            ord: parked.ord,
                            bind: 0,
                            policy: WaiterPolicy::of_level(parked.priority),
                        }],
                        parked.continuation,
                    )
                }
                _ => {
                    let parked = record::unwrap_join_continuation(&waiter.continuation)
                        .expect("a join continuation record");
                    (parked.slots, parked.continuation)
                }
            };
            let guard = waiter
                .continuation
                .guard
                .as_ref()
                .or(continuation.guard.as_ref());
            waits.push(Wait {
                hash: waiter.source.hash.clone(),
                colls: colls.clone(),
                pats: waiter.patterns.iter().map(pat_of).collect(),
                slots,
                persist: waiter.persist,
                peek: !waiter.peeks.is_empty(),
                guard: Guard::of(guard, group.len()),
                name: body_name(&continuation),
            });
        }
    }
    View { kind, recs, waits }
}

/// Where a window entry is: a stored record of the collection, or the element in flight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum At {
    /// The stored record at this index of the records of the collection.
    Stored(usize),
    /// The element in flight.
    Incoming,
}

/// The element in flight of a produce: its collection, its order, its level and its value.
/// Added by decision 103 (decision 118, rule CR-9): and its persist flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct In {
    coll: usize,
    ord: i64,
    level: Option<i64>,
    value: i64,
    /// Added by decision 103 (decision 118, rule CR-9): true for a persistent element. The search
    /// never claims a persistent element, so it can also bind the later positions of the
    /// collection of its fixed position.
    persist: bool,
}

/// The order of the levels of a priority queue: the priority levels in ascending order, and no
/// priority last (F6).
fn level_rank(level: Option<i64>) -> (bool, i64) {
    match level {
        Some(level) => (false, level),
        None => (true, 0),
    }
}

/// True when a bind in the lane `lane` sees a record of the level `level` (decision 38). The
/// unrestricted lane sees every level, and a level lane sees its own level.
fn sees(kind: Kind, lane: Option<i64>, level: Option<i64>) -> bool {
    match (kind, lane) {
        (Kind::Pq, Some(lane)) => level == Some(lane),
        _ => true,
    }
}

/// The head order of a list of `(ord, at)` entries of a head shape. The element in flight takes
/// its place by its order. The order ascends for a queue, a cell and a level of a priority queue,
/// and descends for a stack. A cell places the element in flight first, because a fresh or a
/// re-entering element always reaches the head of a cell.
fn head_order(kind: Kind, mut entries: Vec<(i64, At)>) -> Vec<At> {
    entries.sort_by_key(|(ord, at)| {
        let first = match (kind, at) {
            (Kind::Cell, At::Incoming) => 0,
            _ => 1,
        };
        let ord = match kind {
            Kind::Stack => -ord,
            Kind::Queue | Kind::Cell | Kind::Pq | Kind::Bag | Kind::Set => *ord,
        };
        (first, ord)
    });
    entries.into_iter().map(|(_, at)| at).collect()
}

/// The entries of each level of a priority queue, by the rank of the level. Each value holds the
/// level, and the order and the place of each entry.
type Levels = BTreeMap<(bool, i64), (Option<i64>, Vec<(i64, At)>)>;

/// The window `WIN(c, need, x)` of the collection `coll` (joins plan 3.2): each entry with its
/// level.
/// Added by decision 102 (plan v4 C5.3, rule CR-1): the scan and the class windows list their
/// entries in the canonical order of DR-75. A collection that the group names two or more times
/// keeps that order in play (E1, rule CR-8), and so does the search of a cascade (rule F3). In
/// play, a collection that the group names once takes its entries in a random order, so
/// [`choices`] takes each entry of its window that a pick can reach.
/// Added by decision 103 (decision 118 E1, rule CR-8): a collection that the group names two or
/// more times puts the element in flight first, in every shape ([`x_first`]), as the engine does.
fn window(view: &View, coll: usize, need: usize, incoming: Option<&In>) -> Vec<(At, Option<i64>)> {
    let recs = &view.recs[coll];
    let incoming = incoming.filter(|x| x.coll == coll);
    // Changed by decision 103 (decision 118 E1, rule CR-8): the match gives the window, and E1
    // orders it after the match. The match ended with `}` before, and now with `};`. The line
    // before:
    // match view.kind.win() {
    let window = match view.kind.win() {
        Win::Head => {
            let mut entries: Vec<(i64, At)> = recs
                .iter()
                .enumerate()
                .map(|(index, rec)| (rec.ord, At::Stored(index)))
                .collect();
            if let Some(x) = incoming {
                entries.push((x.ord, At::Incoming));
            }
            head_order(view.kind, entries)
                .into_iter()
                .take(need)
                .map(|at| (at, None))
                .collect()
        }
        Win::Lanes => {
            let mut levels: Levels = BTreeMap::new();
            for (index, rec) in recs.iter().enumerate() {
                levels
                    .entry(level_rank(rec.level))
                    .or_insert_with(|| (rec.level, Vec::new()))
                    .1
                    .push((rec.ord, At::Stored(index)));
            }
            if let Some(x) = incoming {
                levels
                    .entry(level_rank(x.level))
                    .or_insert_with(|| (x.level, Vec::new()))
                    .1
                    .push((x.ord, At::Incoming));
            }
            let mut window = Vec::new();
            for (_, (level, entries)) in levels {
                for at in head_order(Kind::Pq, entries).into_iter().take(need) {
                    window.push((at, level));
                }
            }
            window
        }
        Win::Scan | Win::Classes => {
            let mut order: Vec<usize> = (0..recs.len()).collect();
            order.sort_by(|left, right| recs[*left].hash.cmp(&recs[*right].hash));
            let mut window: Vec<(At, Option<i64>)> = order
                .into_iter()
                .map(|index| (At::Stored(index), None))
                .collect();
            if incoming.is_some() {
                window.push((At::Incoming, None));
            }
            window
        }
    };
    // Added by decision 103 (decision 118 E1, rule CR-8): a collection that the group names two
    // or more times puts the element in flight first, in every shape.
    match need {
        0 | 1 => window,
        _ => x_first(window),
    }
}

/// Added by decision 103 (decision 118 E1, rule CR-8): the window `window` with the element in
/// flight first, and the other entries in their order, as `x_first` of the engine gives it.
fn x_first(window: Vec<(At, Option<i64>)>) -> Vec<(At, Option<i64>)> {
    let mut ordered: Vec<(At, Option<i64>)> = Vec::with_capacity(window.len());
    ordered.extend(
        window
            .iter()
            .filter(|(at, _)| matches!(at, At::Incoming))
            .copied(),
    );
    ordered.extend(
        window
            .iter()
            .filter(|(at, _)| !matches!(at, At::Incoming))
            .copied(),
    );
    ordered
}

/// A claim of the search (decision 28): a record of a collection, or a class of a set
/// collection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Claim {
    Record(usize, At),
    Class(usize, i64),
}

/// The claim of the entry `at` of the collection `coll` with the value `value`.
fn claim_of(kind: Kind, coll: usize, at: At, value: i64) -> Claim {
    match kind.win() {
        Win::Classes => Claim::Class(coll, value),
        Win::Head | Win::Lanes | Win::Scan => Claim::Record(coll, at),
    }
}

/// One position of a search: its collection, its pattern and its lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Position {
    coll: usize,
    pat: Pat,
    lane: Option<i64>,
}

/// The positions of the waiter `wait`, in group order.
fn positions_of(wait: &Wait) -> Vec<Position> {
    wait.colls
        .iter()
        .zip(&wait.pats)
        .zip(&wait.slots)
        .map(|((coll, pat), slot)| Position {
            coll: *coll,
            pat: *pat,
            lane: slot.policy.level(),
        })
        .collect()
}

/// The search of joins plan 3.4 and 3.5 over `positions`. Without `fixed` it is `GREEDY`. With
/// `fixed`, it is the completion of `SEARCH-PRODUCE` with the element in flight `fixed.1` at the
/// position `fixed.0`. It returns the chosen entry of each position, or `None`.
/// Added by decision 102 (plan v4 C5.3, rule CR-1): `pins` holds one entry for each position, or
/// no entry at all. A pinned position takes its pinned entry. The search fails when that entry is
/// not in the window, is not visible, is claimed, or does not bind the pattern. An unpinned
/// position keeps the rule of `GREEDY`.
/// Added by decision 103 (decision 118, rules CR-9 and CR-11): the claims follow rule (a). A
/// persistent entry claims nothing. A non-persistent entry claims its record, and on a set also
/// its class. A later position skips an entry whose record is claimed. It also skips a
/// non-persistent entry whose class is claimed, unless the class has a persistent member
/// ([`class_free`]). A persistent `fixed.1` is never claimed, so it also binds the later
/// positions of the collection of the fixed position through its `Incoming` entry, which comes
/// first in each E1 list. Every other `Incoming` entry is skipped.
// Changed by decision 102 (plan v4 C5.3, rule CR-1): the search takes the pins of [`choices`].
// The signature before:
// fn search(view: &View, positions: &[Position], fixed: Option<(usize, &In)>) -> Option<Vec<At>> {
fn search(
    view: &View,
    positions: &[Position],
    fixed: Option<(usize, &In)>,
    pins: &[Option<At>],
) -> Option<Vec<At>> {
    let kind = view.kind;
    let incoming = fixed.map(|(_, x)| x);
    // Added by decision 103 (decision 118, rule CR-11, U3 (i)): the collection of the fixed
    // position. A persistent x binds later positions on this collection only.
    let fired = fixed.map(|(position, _)| positions[position].coll);
    let mut chosen: Vec<Option<At>> = vec![None; positions.len()];
    let mut claimed: Vec<Claim> = Vec::new();
    if let Some((position, x)) = fixed {
        chosen[position] = Some(At::Incoming);
        // Changed by decision 103 (decision 118, rule CR-9): a persistent x is never claimed. The
        // line before:
        // claimed.push(claim_of(kind, x.coll, At::Incoming, x.value));
        match x.persist {
            true => {}
            false => claimed.push(claim_of(kind, x.coll, At::Incoming, x.value)),
        }
    }
    for (index, position) in positions.iter().enumerate() {
        if chosen[index].is_some() {
            continue;
        }
        let need = positions
            .iter()
            .filter(|other| other.coll == position.coll)
            .count();
        // Changed by decision 103 (decision 118, rule CR-9): the entry and the claims that it
        // makes. The line before:
        // let mut found = None;
        let mut found: Option<(At, Vec<Claim>)> = None;
        // Added by decision 102 (plan v4 C5.3, rule CR-1): the pinned entry of the position.
        let pin = pins.get(index).copied().flatten();
        for (at, level) in window(view, position.coll, need, incoming) {
            // Added by decision 102 (plan v4 C5.3, rule CR-1): a pinned position takes only its
            // pinned entry.
            match pin {
                Some(pinned) if pinned != at => continue,
                Some(_) | None => {}
            }
            // Changed by decision 103 (decision 118, rules CR-9 and CR-11): a persistent x binds a
            // later position of the collection of the fixed position, and it claims nothing.
            // Every other `Incoming` entry is skipped. The lines before:
            // let At::Stored(stored) = at else {
            //     continue;
            // };
            let stored = match (at, incoming) {
                (At::Stored(stored), _) => stored,
                (At::Incoming, Some(x)) if x.persist && fired == Some(position.coll) => {
                    if !sees(kind, position.lane, level) {
                        continue;
                    }
                    if position.pat.binds(x.value) {
                        found = Some((at, Vec::new()));
                        break;
                    }
                    continue;
                }
                (At::Incoming, _) => continue,
            };
            if !sees(kind, position.lane, level) {
                continue;
            }
            let rec = &view.recs[position.coll][stored];
            let claim = claim_of(kind, position.coll, at, rec.value);
            // Changed by decision 103 (decision 118, rule CR-9): rule (a). An entry whose record
            // is claimed is skipped. A non-persistent entry whose class is claimed is skipped
            // too, unless the class has a persistent member. The lines before:
            // if claimed.contains(&claim) {
            //     continue;
            // }
            let record = Claim::Record(position.coll, at);
            if claimed.contains(&record) {
                continue;
            }
            if !rec.persist
                && claimed.contains(&claim)
                && !class_free(view, position.coll, rec.value, incoming)
            {
                continue;
            }
            if position.pat.binds(rec.value) {
                // Changed by decision 103 (decision 118, rule CR-9): a persistent entry claims
                // nothing. A non-persistent entry claims its record, and on a set also its class.
                // The line before:
                // found = Some((at, claim));
                let claims = match (rec.persist, claim == record) {
                    (true, _) => Vec::new(),
                    (false, true) => vec![record],
                    (false, false) => vec![claim, record],
                };
                found = Some((at, claims));
                break;
            }
        }
        // Changed by decision 103 (decision 118, rule CR-9): the entry adds each of its claims.
        // The lines before:
        // let (at, claim) = found?;
        // claimed.push(claim);
        let (at, claims) = found?;
        claimed.extend(claims);
        chosen[index] = Some(at);
    }
    chosen.into_iter().collect()
}

/// Added by decision 103 (decision 118, rule CR-9, K12): true when the set class of the value
/// `value` on the collection `coll` has a persistent member: a persistent stored record of the
/// class, or the persistent element in flight `incoming` of the class. The class window of the
/// engine holds every record of the collection, so the model reads every record.
fn class_free(view: &View, coll: usize, value: i64, incoming: Option<&In>) -> bool {
    incoming.is_some_and(|x| x.persist && x.coll == coll && x.value == value)
        || view.recs[coll]
            .iter()
            .any(|rec| rec.persist && rec.value == value)
}

/// Added by decision 102 (plan v4 C5.3, rule CR-1): the choices that play can reach for the search
/// of `positions` with `fixed`, each in the form of [`search`]. A position is random when the
/// group names its collection once, the window is a scan or classes, and it is not the fixed
/// position. The candidates of a random position are the entries of its window that are visible
/// and bind its pattern. The choices are the results of [`search`] with the pins of each
/// combination of candidates, without `None`. With no random position, they are the one choice of
/// [`search`], or none. Positions with need 1 lie on distinct collections, so their claims never
/// meet, and so each choice exists or none does.
fn choices(view: &View, positions: &[Position], fixed: Option<(usize, &In)>) -> Vec<Vec<At>> {
    let kind = view.kind;
    let incoming = fixed.map(|(_, x)| x);
    let fixed_position = fixed.map(|(position, _)| position);
    let mut pin_lists: Vec<Vec<Option<At>>> = vec![Vec::with_capacity(positions.len())];
    for (index, position) in positions.iter().enumerate() {
        let need = positions
            .iter()
            .filter(|other| other.coll == position.coll)
            .count();
        let random = need == 1
            && matches!(kind.win(), Win::Scan | Win::Classes)
            && fixed_position != Some(index);
        let candidates: Vec<Option<At>> = match random {
            true => window(view, position.coll, need, incoming)
                .into_iter()
                .filter_map(|(at, level)| match at {
                    At::Stored(stored)
                        if sees(kind, position.lane, level)
                            && position.pat.binds(view.recs[position.coll][stored].value) =>
                    {
                        Some(Some(at))
                    }
                    At::Stored(_) | At::Incoming => None,
                })
                .collect(),
            false => vec![None],
        };
        let mut extended: Vec<Vec<Option<At>>> =
            Vec::with_capacity(pin_lists.len() * candidates.len());
        for pins in &pin_lists {
            for candidate in &candidates {
                let mut next: Vec<Option<At>> = Vec::with_capacity(positions.len());
                next.extend_from_slice(pins);
                next.push(*candidate);
                extended.push(next);
            }
        }
        pin_lists = extended;
    }
    pin_lists
        .iter()
        .filter_map(|pins| search(view, positions, fixed, pins))
        .collect()
}

/// The value of the chosen entry `at` of the collection `coll`.
fn value_at(view: &View, coll: usize, at: At, incoming: Option<&In>) -> i64 {
    match at {
        At::Stored(index) => view.recs[coll][index].value,
        At::Incoming => incoming.expect("an element in flight").value,
    }
}

/// The values of the choice `chosen` of the waiter positions `positions` with the slots
/// `slots`, in bind order.
fn bind_values(
    view: &View,
    positions: &[Position],
    slots: &[JoinSlot],
    chosen: &[At],
    incoming: Option<&In>,
) -> Vec<i64> {
    let mut by_bind: Vec<(usize, i64)> = slots
        .iter()
        .zip(positions)
        .zip(chosen)
        .map(|((slot, position), at)| (slot.bind, value_at(view, position.coll, *at, incoming)))
        .collect();
    by_bind.sort();
    by_bind.into_iter().map(|(_, value)| value).collect()
}

/// Added by decision 102 (plan v4 C5.3, rule CR-1): the source hashes of the records of the
/// choice `chosen` of the waiter positions `positions` with the slots `slots`, in bind order. The
/// element in flight has the source hash `own`.
fn bind_hashes(
    view: &View,
    positions: &[Position],
    slots: &[JoinSlot],
    chosen: &[At],
    own: Option<&Blake2b256Hash>,
) -> Vec<Blake2b256Hash> {
    let mut by_bind: Vec<(usize, Blake2b256Hash)> = slots
        .iter()
        .zip(positions)
        .zip(chosen)
        .map(|((slot, position), at)| match at {
            At::Stored(stored) => (slot.bind, view.recs[position.coll][*stored].hash.clone()),
            At::Incoming => (
                slot.bind,
                own.expect("a choice with the element in flight has its source hash")
                    .clone(),
            ),
        })
        .collect();
    by_bind.sort();
    by_bind.into_iter().map(|(_, hash)| hash).collect()
}

/// The seats of the collection `coll`. A seat is a waiter with a position on `coll`, its order on
/// `coll` and the lanes of its positions on `coll`.
fn seats(view: &View, coll: usize) -> Vec<(&Wait, i64, Vec<Option<i64>>)> {
    view.waits
        .iter()
        .filter_map(|wait| {
            let on: Vec<usize> = (0..wait.colls.len())
                .filter(|index| wait.colls[*index] == coll)
                .collect();
            let first = *on.first()?;
            let lanes = on
                .iter()
                .map(|index| wait.slots[*index].policy.level())
                .collect();
            Some((wait, wait.slots[first].ord, lanes))
        })
        .collect()
}

/// The head seat of the seats `seats` in the lane `lane` of a priority queue, or of all seats.
/// The head has the least order in a queue, a cell and a lane, and the greatest in a stack.
fn head_seat<'v>(
    kind: Kind,
    seats: &[(&'v Wait, i64, Vec<Option<i64>>)],
    lane: Option<Option<i64>>,
) -> Option<&'v Wait> {
    seats
        .iter()
        .filter(|(_, _, lanes)| lane.is_none_or(|lane| lanes.contains(&lane)))
        .min_by_key(|(_, ord, _)| match kind {
            Kind::Stack => -ord,
            Kind::Queue | Kind::Cell | Kind::Pq | Kind::Bag | Kind::Set => *ord,
        })
        .map(|(wait, _, _)| *wait)
}

/// `AT-HEAD(W, c)` of joins plan 3.3 for the stored waiter `wait`. On a head shape, the waiter is
/// the head of the waiters of `coll`. On a priority queue, it is the head of each of its lanes on
/// `coll`. On the bag and the set, the condition always holds.
fn at_head(view: &View, wait: &Wait, coll: usize) -> bool {
    let all = seats(view, coll);
    match view.kind.win() {
        Win::Scan | Win::Classes => true,
        Win::Head => head_seat(view.kind, &all, None).is_some_and(|head| head.hash == wait.hash),
        Win::Lanes => (0..wait.colls.len())
            .filter(|index| wait.colls[*index] == coll)
            .all(|index| {
                head_seat(view.kind, &all, Some(wait.slots[index].policy.level()))
                    .is_some_and(|head| head.hash == wait.hash)
            }),
    }
}

/// `AT-HEAD-INCOMING(W, c)` of joins plan 3.3 for a fresh waiter whose positions on `coll` have
/// the lanes `lanes`. A queue lane must be empty, and a stack or a cell always reaches its head.
fn at_head_incoming(view: &View, coll: usize, lanes: &[Option<i64>]) -> bool {
    let all = seats(view, coll);
    match view.kind {
        Kind::Bag | Kind::Set | Kind::Stack | Kind::Cell => true,
        Kind::Queue => all.is_empty(),
        Kind::Pq => lanes.iter().all(|lane| {
            all.iter()
                .all(|(_, _, seat_lanes)| !seat_lanes.contains(lane))
        }),
    }
}

// Changed by decision 102 (plan v4 C5.3, rule CR-1): on the bag and the set, play binds a random
// choice, so the doc below replaces this one.
// /// `FIREABLE(W)` of joins plan 3.4 for the stored waiter `wait`. The waiter is at the head of each
// /// collection, it has a greedy choice, and its guard admits the values in bind order.
/// `FIREABLE(W)` of joins plan 3.4 for the stored waiter `wait`. The waiter is at the head of each
/// collection, it has a choice of [`choices`], and its guard admits the values of every choice in
/// bind order. On a head shape and a priority queue, the one choice is the choice of `GREEDY`. On
/// the bag and the set, play binds a random choice, and a waiter whose guard refuses one choice
/// can park. So the waiter is owed only when its guard admits every choice.
fn fireable(view: &View, wait: &Wait) -> bool {
    let distinct: BTreeSet<usize> = wait.colls.iter().copied().collect();
    if !distinct.iter().all(|coll| at_head(view, wait, *coll)) {
        return false;
    }
    let positions = positions_of(wait);
    // Changed by decision 102 (plan v4 C5.3, rule CR-1): the guard must admit every choice that
    // play can reach. The body before:
    // match search(view, &positions, None) {
    //     Some(chosen) => {
    //         wait.guard
    //             .admits(&bind_values(view, &positions, &wait.slots, &chosen, None))
    //     }
    //     None => false,
    // }
    match choices(view, &positions, None).as_slice() {
        [] => false,
        options => options.iter().all(|chosen| {
            wait.guard
                .admits(&bind_values(view, &positions, &wait.slots, chosen, None))
        }),
    }
}

/// Checks the invariant of the discipline of `view` (joins plan 3.6). The strong `INV_head` holds
/// on the queue, the stack, the cell, the priority queue and the set: no waiter is fireable,
/// guarded waiters included. The weak `INV_scan` holds on the bag: no waiter without a guard has a
/// greedy choice.
fn check_inv(view: &View) -> Result<(), TestCaseError> {
    for wait in &view.waits {
        let pending = match view.kind.strong() {
            true => fireable(view, wait),
            // Changed by decision 102 (plan v4 C5.3, rule CR-1): `search` takes pins. A check of
            // existence takes none, because existence does not depend on a pick. The arm before:
            // false => wait.guard == Guard::Open && search(view, &positions_of(wait), None).is_some(),
            false => {
                wait.guard == Guard::Open && search(view, &positions_of(wait), None, &[]).is_some()
            }
        };
        if pending {
            return Err(failure(format!(
                "the waiter {} keeps a pending match on a {:?} space: {view:?}",
                wait.name, view.kind
            )));
        }
    }
    Ok(())
}

// ─── Running programs ────────────────────────────────────────────────────────────────────────

// Changed by decision 92: a failure at a position of the state commits the anchor of its job, so
// a refused job logs the events of its anchor.
// /// Runs the step `step` with the index `index` as one chain. A cell may refuse the first job of
// /// the chain with `CellOccupied`, and then the job logs no event. Every other failure fails the
// /// case. It returns the jobs.
/// Runs the step `step` with the index `index` as one chain. A cell may refuse the first job of
/// the chain with `CellOccupied`, and then the job logs the events of its anchor only
/// ([`logs_anchor_only`]). Every other failure fails the case. It returns the jobs.
async fn run_step(rig: &Rig, step: &Step, index: usize) -> Result<Vec<Job>, TestCaseError> {
    rig.log().await;
    let jobs = chain(rig, space_op(step, index)).await?;
    for (position, job) in jobs.iter().enumerate() {
        if let Err(error) = &job.result {
            expected_refusal(rig.kind, error)?;
            if position != 0 {
                return Err(failure(format!("a re-issued job fails: {error:?}")));
            }
            // Changed by decision 92: the refused job logs its anchor.
            // if !rig.log().await.is_empty() {
            //     return Err(failure("a refused operation logs events"));
            // }
            let log = rig.log().await;
            if !logs_anchor_only(&log) {
                return Err(failure(format!(
                    "a refused operation logs more than its anchor: {log:?}"
                )));
            }
        }
    }
    Ok(jobs)
}

/// Added by decision 92: true when `log` holds the events of an anchor only (`spaces::anchor`):
/// a consume on the config channel `C(sid)` of the space, and the COMM of that consume.
fn logs_anchor_only(log: &[Event]) -> bool {
    let config = rspace_plus_plus::rspace::hashing::stable_hash_provider::hash(
        &support::session::config_channel(SPACE),
    );
    match log {
        [Event::IoEvent(rspace_plus_plus::rspace::trace::event::IOEvent::Consume(consume)), Event::Comm(comm)] => {
            consume.channel_hashes == [config.clone()] && comm.consume.channel_hashes == [config]
        }
        _ => false,
    }
}

/// The order of the element of the produce `op` in its next round. It is the carried order, or
/// the value of the counter of its collection before the bump, or 0 without a counter.
async fn own_ord(rig: &Rig, op: &ProduceOp, coll: usize) -> i64 {
    match op.reissued_ord {
        Some(ord) => ord,
        None => rig.counter(coll).await.unwrap_or(0),
    }
}

/// The record of the element of the produce `op` with the order `ord`.
fn own_record(kind: Kind, op: &ProduceOp, ord: i64) -> ListParWithRandom {
    match kind {
        Kind::Pq => {
            let level = op
                .params
                .get(PRIORITY_PARAM)
                .map(|level| number(std::slice::from_ref(level)));
            record::wrap_pq_datum(ord, level, &op.data)
        }
        Kind::Queue | Kind::Stack | Kind::Cell | Kind::Bag | Kind::Set => {
            record::wrap_datum(ord, &op.data)
        }
    }
}

/// The collection of the inner channel `channel`.
fn coll_of_inner(channel: &Par) -> usize {
    (0..COLLS)
        .find(|coll| ch(*coll) == *channel)
        .unwrap_or_else(|| panic!("a channel outside the collections: {channel:?}"))
}

// ─── Headline 9 and the TLA invariant QuiescentNoPendingMatch ────────────────────────────────

/// `cascade_restores_inv` (`JoinCascade.v`, headline 9). A transaction that ends with no stop
/// and no outstanding work leaves the strong `INV_head`, guarded waiters included. This holds on
/// the queue, the stack, the cell, the priority queue and the joined set collections (decision
/// 50). The bag keeps only the weak `INV_scan`. Every step of a generated program runs as one
/// chain, as a deploy runs it. The model checks the invariant when the chain ends.
///
/// The model checks the strong form on every set collection. A set collection without joins
/// keeps the driver of the set. There, a guarded waiter that a removal exposes stays parked
/// (decision 37). But the generated receives with one bind have no guard, and a guarded join
/// makes its collections joined. So the two forms agree on every generated set collection.
///
/// Added by decision 102 (plan v4 C5.3, rule CR-1): on the set, play binds a random choice, so a
/// guarded waiter can park when its guard refuses the choice of play. So the model calls a waiter
/// of the set owed only when its guard admits every choice that play can reach ([`fireable`]).
/// This is the allowed-set form of `INV_head`. On the other shapes, the one choice is the choice
/// of `GREEDY`, as before.
#[test]
fn prop_cascade_restores_inv_any_schedule() {
    check(
        pure_config(),
        arb_program(&Kind::ALL, 1..12, COLLS, 4, true),
        |program| tokio_runtime().block_on(inv_case(program)),
    );
}

/// One case of the invariant properties.
async fn inv_case(program: Program) -> Result<(), TestCaseError> {
    let rig = Rig::new(program.kind).await;
    for (index, step) in program.steps.iter().enumerate() {
        run_step(&rig, step, index).await?;
        rig.log().await;
        let view = read_view(&rig.host, program.kind).await;
        check_inv(&view)
            .map_err(|error| failure(format!("after step {index} ({step:?}): {error}")))?;
    }
    Ok(())
}

/// The kinds of the model of `JoinCascade.tla`: the queue, the stack and the bag.
const TLA_KINDS: [Kind; 3] = [Kind::Queue, Kind::Stack, Kind::Bag];

/// A step of the model of `JoinCascade.tla`. The model has two collections and the values `a = 0`
/// and `b = 1`. It has the patterns `a`, `b` and any, and the guards none and only `b`. Its groups
/// are `⟨c1⟩`, `⟨c2⟩`, `⟨c1, c2⟩` and `⟨c1, c1⟩`.
fn arb_tla_step(persistence: Persistence) -> impl Strategy<Value = Step> {
    let pat = prop_oneof![Just(Pat::Is(0)), Just(Pat::Is(1)), Just(Pat::Any)];
    let send =
        (0..2usize, 0..2i64, any::<bool>()).prop_map(move |(coll, value, persistent)| Step::Send {
            coll,
            value,
            persistent: persistent && persistence == Persistence::Sends,
            level: None,
        });
    let group = prop_oneof![
        Just(vec![0usize]),
        Just(vec![1usize]),
        Just(vec![0usize, 1]),
        Just(vec![0usize, 0])
    ];
    let receive = (
        group,
        prop::collection::vec(pat, 2),
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(move |(colls, pats, peek, persistent, only_b)| {
            let persistent = persistent && !peek && persistence == Persistence::Receives;
            let binds: Vec<Bind> = colls
                .iter()
                .zip(&pats)
                .map(|(coll, pat)| Bind {
                    coll: *coll,
                    pat: *pat,
                    lane: None,
                })
                .collect();
            match binds.as_slice() {
                [bind] => Step::Receive {
                    bind: *bind,
                    peek,
                    persistent,
                },
                _ => {
                    let open = binds.iter().all(|bind| bind.pat == Pat::Any);
                    Step::Join {
                        binds,
                        peek,
                        persistent,
                        guard: match open && only_b {
                            true => Guard::LastIs(1),
                            false => Guard::Open,
                        },
                    }
                }
            }
        });
    prop_oneof![send, receive]
}

/// `QuiescentNoPendingMatch` (`JoinCascade.tla`): when no round is in progress and no re-issue is
/// outstanding, the queue and the stack satisfy `INV_head` and the bag satisfies `INV_scan`. The
/// programs are the programs of the model: two collections, two values, the guard that admits
/// only `b`, and the groups of the model. The cap of the engine is the fixed `FIRE_ROUND_CAP`.
#[test]
fn prop_quiescent_no_pending_match_any_schedule() {
    let strategy = (prop::sample::select(&TLA_KINDS[..]), prop_oneof![
        Just(Persistence::Neither),
        Just(Persistence::Sends),
        Just(Persistence::Receives)
    ])
        .prop_flat_map(|(kind, persistence)| {
            prop::collection::vec(arb_tla_step(persistence), 1..10)
                .prop_map(move |steps| Program { kind, steps })
        });
    check(pure_config(), strategy, |program| {
        tokio_runtime().block_on(inv_case(program))
    });
}

// ─── Headlines 7 and 8, and the TLA invariants TxnFiresBounded and CascadeMeasureDecreases ───

/// A program whose prefix owes more fires than the cap. The prefix parks `waiters` receives of
/// any value on the collection 0. A join on the collections 0 and 1 never fires, so it keeps the
/// collection 0 joined. A persistent send on the collection 0 fires one receive after another.
/// The join waits behind the receives on a queue and a priority queue, and below them on a stack.
/// The program goes on with `tail`.
fn heavy_program(kind: Kind, waiters: usize, tail: Vec<Step>) -> Program {
    let receive = Step::Receive {
        bind: Bind {
            coll: 0,
            pat: Pat::Any,
            lane: None,
        },
        peek: false,
        persistent: false,
    };
    let guard = Step::Join {
        binds: vec![
            Bind {
                coll: 0,
                pat: Pat::Is(-1),
                lane: None,
            },
            Bind {
                coll: 1,
                pat: Pat::Is(-1),
                lane: None,
            },
        ],
        peek: false,
        persistent: false,
        guard: Guard::Open,
    };
    let mut steps = Vec::with_capacity(waiters + 2 + tail.len());
    match kind {
        Kind::Stack => {
            steps.push(guard);
            steps.extend(std::iter::repeat_n(receive, waiters));
        }
        Kind::Queue | Kind::Cell | Kind::Bag | Kind::Set | Kind::Pq => {
            steps.extend(std::iter::repeat_n(receive, waiters));
            steps.push(guard);
        }
    }
    steps.push(Step::Send {
        coll: 0,
        value: 2,
        persistent: true,
        level: match kind {
            Kind::Pq => Some(0),
            Kind::Queue | Kind::Stack | Kind::Cell | Kind::Bag | Kind::Set => None,
        },
    });
    steps.extend(tail);
    Program { kind, steps }
}

/// The kinds of the heavy programs: every discipline but the cell, which holds one waiter.
const HEAVY_KINDS: [Kind; 5] = [Kind::Queue, Kind::Stack, Kind::Bag, Kind::Set, Kind::Pq];

/// A program that is heavy in one case of four.
fn arb_maybe_heavy_program() -> impl Strategy<Value = Program> {
    prop_oneof![
        3 => arb_program(&Kind::ALL, 1..12, COLLS, 4, true),
        1 => (
            prop::sample::select(&HEAVY_KINDS[..]),
            (FIRE_ROUND_CAP - 2)..=(2 * FIRE_ROUND_CAP + 2),
        )
            .prop_flat_map(|(kind, waiters)| {
                prop::collection::vec(arb_step(kind, Persistence::Sends, COLLS, 4, true), 0..4)
                    .prop_map(move |tail| heavy_program(kind, waiters, tail))
            }),
    ]
}

/// `cascade_terminates` (`JoinCascade.v`, headline 7): every chain of a generated program ends,
/// and no job of it fires more than `FIRE_ROUND_CAP` times. A heavy program runs a send past the
/// cap, and its chain still fires every receive once.
#[test]
fn prop_cascade_terminates_any_schedule() {
    check(pure_config(), arb_maybe_heavy_program(), |program| {
        tokio_runtime().block_on(terminates_case(program))
    });
}

/// One case of `prop_cascade_terminates_any_schedule`.
async fn terminates_case(program: Program) -> Result<(), TestCaseError> {
    let rig = Rig::new(program.kind).await;
    let mut fired: BTreeMap<String, usize> = BTreeMap::new();
    for (index, step) in program.steps.iter().enumerate() {
        let jobs = run_step(&rig, step, index).await?;
        for count in fire_counts(&jobs) {
            prop_assert!(count <= FIRE_ROUND_CAP, "a job fires {} times", count);
        }
        for job in &jobs {
            if let Ok(outcome) = &job.result {
                for fire in &outcome.fires {
                    *fired.entry(body_name(&fire.continuation)).or_default() += 1;
                }
            }
        }
        rig.log().await;
    }
    for (name, count) in &fired {
        let Some(step) = step_of(name).and_then(|index| program.steps.get(index)) else {
            return Err(failure(format!("an unknown body fires: {name}")));
        };
        let persistent = match step {
            Step::Receive { persistent, .. } | Step::Join { persistent, .. } => *persistent,
            Step::Send { .. } => return Err(failure("a send fires as a body")),
        };
        prop_assert!(
            persistent || *count == 1,
            "the waiter {} fires {} times",
            name,
            count
        );
    }
    Ok(())
}

/// The scenario of the cap before the plan (decision 49, DV-2) on a queue. A first send on `q1`
/// fires the join w on `[q1, q2]` and the receives behind it, and stops at the cap. It leaves
/// `owed` pairs of `q2`. A send on `q2` then settles them first, on its own fire count. The join u
/// on `[q2, q3]` keeps `q2` joined and binds the value -1 with the record 7 on `q3`. When
/// `fires_u` holds, the send on `q2` is the persistent -1, which u can bind.
async fn settle_before_case(owed: usize, fires_u: bool) -> Result<(), TestCaseError> {
    let rig = Rig::new(Kind::Queue).await;
    let first = FIRE_ROUND_CAP - 1;
    let records = 1 + first + owed;
    let waiters = first + owed;
    for value in 0..records {
        let value = i64::try_from(value).expect("a small value");
        rig.run(SpaceOp::Produce(send(1, value, false, None, 0)))
            .await
            .map_err(failure)?;
    }
    let w = Bind {
        coll: 0,
        pat: Pat::Any,
        lane: None,
    };
    let on_q2 = Bind {
        coll: 1,
        pat: Pat::Any,
        lane: None,
    };
    rig.run(SpaceOp::Join(join_of(
        &[w, on_q2],
        "w",
        false,
        false,
        Guard::Open,
    )))
    .await
    .map_err(failure)?;
    for index in 0..waiters {
        rig.run(space_op(
            &Step::Receive {
                bind: on_q2,
                peek: false,
                persistent: false,
            },
            1000 + index,
        ))
        .await
        .map_err(failure)?;
    }
    let u = [
        Bind {
            coll: 1,
            pat: Pat::Is(-1),
            lane: None,
        },
        Bind {
            coll: 2,
            pat: Pat::Any,
            lane: None,
        },
    ];
    rig.run(SpaceOp::Join(join_of(&u, "u", false, false, Guard::Open)))
        .await
        .map_err(failure)?;
    rig.run(SpaceOp::Produce(send(2, 7, false, None, 0)))
        .await
        .map_err(failure)?;
    let first_send = rig
        .run(SpaceOp::Produce(send(0, -5, false, None, 0)))
        .await
        .map_err(failure)?;
    prop_assert_eq!(first_send.fires.len(), FIRE_ROUND_CAP);
    let op = match fires_u {
        true => SpaceOp::Produce(send(1, -1, true, None, 1)),
        false => SpaceOp::Produce(send(1, 99, false, None, 1)),
    };
    let jobs = chain(&rig, op).await?;
    let counts = fire_counts(&jobs);
    prop_assert!(
        counts.iter().all(|count| *count <= FIRE_ROUND_CAP),
        "the fires of each job: {:?}",
        counts
    );
    let total: usize = counts.iter().sum();
    prop_assert_eq!(
        total,
        owed + usize::from(fires_u),
        "the fires of the chain: {:?}",
        counts
    );
    Ok(())
}

/// `join_txn_fires_bounded` (`JoinCascade.v`, headline 8) and `TxnFiresBounded`
/// (`JoinCascade.tla`). The settle before the plan, the plan, the second sends and the cascade
/// share one fire count. So no transaction fires more than `FIRE_ROUND_CAP` times, and a settle
/// job starts its count at 0. The cases are the generated and the heavy programs. They also
/// include a settle before the plan that ends one fire below, at, or one fire above the cap. In
/// that case the plan could fire the operation (DV-2).
#[test]
fn prop_join_txn_fires_bounded_at_the_cap() {
    let strategy = prop_oneof![
        3 => arb_maybe_heavy_program().prop_map(Ok),
        1 => ((FIRE_ROUND_CAP - 2)..=(FIRE_ROUND_CAP + 2), any::<bool>()).prop_map(Err),
    ];
    check(pure_config(), strategy, |case| {
        tokio_runtime().block_on(async move {
            match case {
                Ok(program) => terminates_case(program).await,
                Err((owed, fires_u)) => settle_before_case(owed, fires_u).await,
            }
        })
    });
}

/// The measure `μ` of joins plan 4.9 of the stored state `view` with the operation `flight` still
/// outstanding. Its parts are the non-persistent waiters, the non-persistent records plus the
/// pending second sends, and the pending second sends. The element of an outstanding operation is
/// in flight. A settle job carries its pending sends and its resume.
fn measure(view: &View, flight: Option<&SpaceOp>) -> (usize, usize, usize) {
    let waiters = view.waits.iter().filter(|wait| !wait.persist).count();
    let records = view
        .recs
        .iter()
        .flatten()
        .filter(|rec| !rec.persist)
        .count();
    let (in_waiters, in_records, pending) = flight.map_or((0, 0, 0), in_flight);
    (
        waiters + in_waiters,
        records + in_records + pending,
        pending,
    )
}

/// The elements of the outstanding operation `op`. They are its waiter or its datum when it is
/// not persistent. For a settle job, they are its pending second sends and the elements of its
/// resume.
fn in_flight(op: &SpaceOp) -> (usize, usize, usize) {
    match op {
        SpaceOp::Produce(op) => (0, usize::from(!op.persistent), 0),
        SpaceOp::Consume(op) => (usize::from(!op.persistent), 0, 0),
        SpaceOp::Join(op) => (usize::from(!op.persistent), 0, 0),
        SpaceOp::Settle(settle) => match &settle.cascade {
            None => (0, 0, 0),
            Some(cascade) => {
                let (waiters, records, pending) = cascade
                    .resume
                    .as_ref()
                    .map_or((0, 0, 0), |resume| in_flight(&resume.op));
                (waiters, records, pending + cascade.pending.len())
            }
        },
    }
}

/// `CascadeMeasureDecreases` (`JoinCascade.tla`). Every step of a transaction lowers the measure
/// `μ` in lexicographic order or stops. So a job that fires lowers `μ` from its start to its end.
/// A job that fires nothing keeps `μ`: its element is stored or parked, or it stays in flight.
#[test]
fn prop_cascade_measure_decreases() {
    check(pure_config(), arb_maybe_heavy_program(), |program| {
        tokio_runtime().block_on(measure_case(program))
    });
}

/// One case of `prop_cascade_measure_decreases`.
async fn measure_case(program: Program) -> Result<(), TestCaseError> {
    let rig = Rig::new(program.kind).await;
    for (index, step) in program.steps.iter().enumerate() {
        let mut next = Some(space_op(step, index));
        let mut jobs = 0;
        while let Some(op) = next.take() {
            jobs += 1;
            prop_assert!(jobs <= MAX_CHAIN, "a chain runs past {} jobs", MAX_CHAIN);
            let before = measure(&read_view(&rig.host, program.kind).await, Some(&op));
            let result = rig.run(op).await;
            rig.log().await;
            let outcome = match result {
                Ok(outcome) => outcome,
                Err(error) => {
                    expected_refusal(program.kind, &error)?;
                    break;
                }
            };
            let after = measure(
                &read_view(&rig.host, program.kind).await,
                outcome.reissue.as_ref().map(|reissue| &reissue.op),
            );
            match outcome.fires.is_empty() {
                true => prop_assert_eq!(after, before, "a job without a fire keeps μ"),
                false => prop_assert!(
                    after < before,
                    "a job that fires lowers μ: {:?} to {:?}",
                    before,
                    after
                ),
            }
            next = outcome.reissue.map(|reissue| reissue.op);
        }
    }
    Ok(())
}

// ─── Headlines 1, 2 and 17 ───────────────────────────────────────────────────────────────────

/// The group positions of the join `binds`: for each position in canonical group order, its
/// bind index and its position (FR-1').
fn group_of(binds: &[Bind]) -> Vec<(usize, Position)> {
    let channels: Vec<Par> = binds.iter().map(|bind| ch(bind.coll)).collect();
    let refs: Vec<&Par> = channels.iter().collect();
    support::agent::group_positions(SPACE, &refs)
        .into_iter()
        .map(|bind| {
            (bind, Position {
                coll: binds[bind].coll,
                pat: binds[bind].pat,
                lane: binds[bind].lane,
            })
        })
        .collect()
}

/// The slots of the positions `group` with the order 0: the model compares bind indices and
/// lanes only.
fn slots_of(group: &[(usize, Position)]) -> Vec<JoinSlot> {
    group
        .iter()
        .map(|(bind, position)| JoinSlot {
            ord: 0,
            bind: *bind,
            policy: WaiterPolicy::of_level(position.lane),
        })
        .collect()
}

/// The identities of the results of the fire `fire`, in bind order: the source hash of each
/// bound record.
fn result_hashes(
    fire: &rholang::rust::interpreter::spaces::agent::SpaceFire,
) -> Vec<Blake2b256Hash> {
    fire.results
        .iter()
        .map(|result| {
            Produce::create(&result.channel, &result.removed_datum, result.persistent).hash
        })
        .collect()
}

/// The values of the records that the fire `fire` binds, in bind order. Each value is the payload
/// of the removed record, also where the pattern binds no variable.
fn result_values(fire: &rholang::rust::interpreter::spaces::agent::SpaceFire) -> Vec<i64> {
    fire.results
        .iter()
        .map(|result| {
            let decoded = record::unwrap_pq_datum(&result.removed_datum).expect("a datum record");
            number(&decoded.datum.pars)
        })
        .collect()
}

/// Added by decision 102 (plan v4 C5.3, rule CR-1): non-vacuity. The disciplines whose join
/// windows have a random pick in play.
const RANDOM_PICK_KINDS: [Kind; 2] = [Kind::Bag, Kind::Set];

/// Added by decision 102 (plan v4 C5.3, rule CR-1): non-vacuity. The least count of cases that a
/// count of [`KindCounts`] must reach.
const MIN_RANDOM_CASES: usize = 1;

/// Added by decision 102 (plan v4 C5.3, rule CR-1): non-vacuity. A count of property cases for
/// each discipline of [`RANDOM_PICK_KINDS`]. Only these disciplines have a random pick, so a case
/// of another discipline has no count.
struct KindCounts {
    bag: AtomicUsize,
    set: AtomicUsize,
}

impl KindCounts {
    /// No case of either discipline.
    const fn new() -> KindCounts {
        KindCounts {
            bag: AtomicUsize::new(0),
            set: AtomicUsize::new(0),
        }
    }

    /// Counts one case of the discipline `kind`.
    fn add(&self, kind: Kind) {
        let count = match kind {
            Kind::Bag => &self.bag,
            Kind::Set => &self.set,
            Kind::Queue | Kind::Stack | Kind::Cell | Kind::Pq => return,
        };
        count.fetch_add(1, Ordering::Relaxed);
    }

    /// The count of each discipline of [`RANDOM_PICK_KINDS`], in its order.
    fn counts(&self) -> [(Kind, usize); 2] {
        [
            (Kind::Bag, self.bag.load(Ordering::Relaxed)),
            (Kind::Set, self.set.load(Ordering::Relaxed)),
        ]
    }

    /// Prints the count of each discipline for the property `property`, and checks that each
    /// count reaches [`MIN_RANDOM_CASES`]. `cases` describes the counted cases.
    fn assert_each_kind(&self, property: &str, cases: &str) {
        for (kind, count) in self.counts() {
            eprintln!("{property}: {count} {kind:?} cases {cases}");
            assert!(
                count >= MIN_RANDOM_CASES,
                "{property}: {count} {kind:?} cases {cases}, fewer than {MIN_RANDOM_CASES}, so \
                 the random case of the {kind:?} is vacuous"
            );
        }
    }

    /// Prints the count of each discipline for the property `property`, and checks that the sum
    /// of the counts reaches [`MIN_RANDOM_CASES`]. `cases` describes the counted cases.
    fn assert_total(&self, property: &str, cases: &str) {
        let mut total = 0;
        for (kind, count) in self.counts() {
            eprintln!("{property}: {count} {kind:?} cases {cases}");
            total += count;
        }
        assert!(
            total >= MIN_RANDOM_CASES,
            "{property}: {total} cases {cases}, fewer than {MIN_RANDOM_CASES}, so the case is \
             vacuous"
        );
    }
}

/// Added by decision 102 (plan v4 C5.3, rule CR-1): non-vacuity. A program on the bag or the set
/// that starts with two or three sends on each collection. So a bind on a collection that a join
/// names once often has two or more records to pick from. The steps of [`arb_program`] follow
/// the sends.
fn arb_sends_first_program() -> impl Strategy<Value = Program> {
    (
        arb_program(&RANDOM_PICK_KINDS, 0..10, COLLS, 4, false),
        prop::collection::vec(prop::collection::vec(0i64..4, 2..=3), COLLS),
    )
        .prop_map(|(program, values)| {
            let sends: usize = values.iter().map(Vec::len).sum();
            let mut steps: Vec<Step> = Vec::with_capacity(sends + program.steps.len());
            for (coll, coll_values) in values.iter().enumerate() {
                steps.extend(coll_values.iter().map(|value| Step::Send {
                    coll,
                    value: *value,
                    persistent: false,
                    level: None,
                }));
            }
            steps.extend(program.steps);
            Program {
                kind: program.kind,
                steps,
            }
        })
}

/// A program prefix, and a join after it.
/// Added by decision 102 (plan v4 C5.3, rule CR-1): non-vacuity. One prefix in four starts with
/// sends on each collection of a bag or a set ([`arb_sends_first_program`]).
fn arb_prefix_and_join() -> impl Strategy<Value = (Program, Step)> {
    // Changed by decision 102 (plan v4 C5.3, rule CR-1): non-vacuity. The line before:
    // arb_program(&Kind::ALL, 0..10, COLLS, 4, false).prop_flat_map(|program| {
    prop_oneof![
        3 => arb_program(&Kind::ALL, 0..10, COLLS, 4, false),
        1 => arb_sends_first_program(),
    ]
    .prop_flat_map(|program| {
        let kind = program.kind;
        (
            Just(program),
            (
                prop::collection::vec(arb_bind(kind, COLLS, 4), 2..=3),
                any::<bool>(),
                0u8..4,
                0i64..4,
            )
                .prop_map(|(binds, peek, guard, value)| {
                    let open = binds.iter().all(|bind| bind.pat == Pat::Any);
                    Step::Join {
                        binds,
                        peek,
                        persistent: false,
                        guard: match (open, guard) {
                            (true, 1) => Guard::LastAboveFirst,
                            (true, 2) => Guard::LastIs(value),
                            _ => Guard::Open,
                        },
                    }
                }),
        )
    })
}

/// Added by decision 102 (plan v4 C5.3, rule CR-1): non-vacuity. The cases of
/// `prop_join_search_is_head_greedy` that reach the random case: [`choices`] gives two or more
/// choices, and the engine runs the join, for each kind. The seed is fixed, so the counts are
/// deterministic.
static GREEDY_RANDOM_CASES: KindCounts = KindCounts::new();

// Changed by decision 102 (plan v4 C5.3, rule CR-1): on the bag and the set, play binds a random
// choice, so the doc below replaces this one.
// /// `join_search_is_head_greedy` (`JoinCascade.v`, headline 1). A new join fires from the windows
// /// exactly when three conditions hold. It reaches the head of each head-only collection
// /// (`AT-HEAD-INCOMING`). `GREEDY` over the windows in group order finds a choice. The guard
// /// admits the values in bind order. The guard runs once, after every bind matched (decision 37).
// /// The fire binds the records of `GREEDY`, so each bound record matches its bind.
/// `join_search_is_head_greedy` (`JoinCascade.v`, headline 1). The model gives the choices that
/// play can reach ([`choices`]). On a head shape and a priority queue, the one choice is the
/// choice of `GREEDY` over the windows in group order. On the bag and the set, a bind on a
/// collection that the group names once can take each record of its window that it binds. The
/// model has choices only when the join reaches the head of each head-only collection
/// (`AT-HEAD-INCOMING`). The guard runs once, after every bind matched (decision 37). So it runs
/// once when a choice exists and the join has a guard.
///
/// The check is an allowed-set check, with no probabilistic assertion. A fire must bind the
/// records of one choice that the guard admits: the same values in bind order and the same source
/// hashes. So each bound record matches its bind. A park needs no choice, or a choice that the
/// guard refuses. On a head shape and a priority queue, the allowed set has one outcome, as
/// before.
///
/// Added by decision 102 (plan v4 C5.3, rule CR-1): non-vacuity. On the bag and on the set, some
/// case reaches the random case: [`choices`] gives two or more choices ([`GREEDY_RANDOM_CASES`]).
#[test]
fn prop_join_search_is_head_greedy() {
    check(pure_config(), arb_prefix_and_join(), |(program, join)| {
        tokio_runtime().block_on(greedy_case(program, join))
    });
    // Added by decision 102 (plan v4 C5.3, rule CR-1): non-vacuity.
    GREEDY_RANDOM_CASES.assert_each_kind(
        "prop_join_search_is_head_greedy",
        "with two or more choices",
    );
}

/// One case of `prop_join_search_is_head_greedy`.
async fn greedy_case(program: Program, join: Step) -> Result<(), TestCaseError> {
    let matcher = support::agent::CountingMatcher::default();
    let rig = Rig::over(
        support::agent::play_with(matcher.clone()).await,
        program.kind,
    )
    .await;
    for (index, step) in program.steps.iter().enumerate() {
        run_step(&rig, step, index).await?;
    }
    rig.log().await;
    let Step::Join { binds, guard, .. } = &join else {
        return Err(failure("the last step is a join"));
    };
    let view = read_view(&rig.host, program.kind).await;
    let group = group_of(binds);
    let positions: Vec<Position> = group.iter().map(|(_, position)| *position).collect();
    let slots = slots_of(&group);
    let distinct: BTreeSet<usize> = positions.iter().map(|position| position.coll).collect();
    let heads = distinct.iter().all(|coll| {
        let lanes: Vec<Option<i64>> = positions
            .iter()
            .filter(|position| position.coll == *coll)
            .map(|position| position.lane)
            .collect();
        at_head_incoming(&view, *coll, &lanes)
    });
    // Changed by decision 102 (plan v4 C5.3, rule CR-1): on the bag and the set, play binds a
    // random choice. So the model takes every choice that play can reach ([`choices`]). The lines
    // before:
    // let chosen = match heads {
    //     true => search(&view, &positions, None),
    //     false => None,
    // };
    // let admitted = chosen
    //     .as_ref()
    //     .map(|chosen| guard.admits(&bind_values(&view, &positions, &slots, chosen, None)));
    let options: Vec<Vec<At>> = match heads {
        true => choices(&view, &positions, None),
        false => Vec::new(),
    };
    let admitted: Vec<bool> = options
        .iter()
        .map(|chosen| guard.admits(&bind_values(&view, &positions, &slots, chosen, None)))
        .collect();
    let before = matcher.count();
    let index = program.steps.len();
    let result = rig.run(space_op(&join, index)).await;
    let outcome = match result {
        Ok(outcome) => outcome,
        Err(error) => return expected_refusal(program.kind, &error),
    };
    // Added by decision 102 (plan v4 C5.3, rule CR-1): non-vacuity. The case reaches the random
    // case when [`choices`] gives two or more choices.
    match options.len() {
        0 | 1 => {}
        _ => GREEDY_RANDOM_CASES.add(program.kind),
    }
    let checks = matcher.count() - before;
    // Changed by decision 102 (plan v4 C5.3, rule CR-1): each choice exists or none does, so the
    // count stays deterministic. The line before:
    // let expected_checks = usize::from(admitted.is_some() && *guard != Guard::Open);
    let expected_checks = usize::from(!options.is_empty() && *guard != Guard::Open);
    prop_assert_eq!(
        checks,
        expected_checks,
        "the guard runs once after every bind matched"
    );
    // Changed by decision 102 (plan v4 C5.3, rule CR-1): a fire must bind one choice that the
    // guard admits, and a park needs no choice or a choice that the guard refuses. The match
    // before:
    // match (chosen, admitted) {
    //     (Some(chosen), Some(true)) => {
    //         let fire = outcome
    //             .fires
    //             .first()
    //             .ok_or_else(|| failure("the model fires the join, and the engine parks it"))?;
    //         prop_assert_eq!(body_name(&fire.continuation), name(index));
    //         prop_assert_eq!(
    //             result_values(fire),
    //             bind_values(&view, &positions, &slots, &chosen, None)
    //         );
    //         let mut expected: Vec<(usize, Blake2b256Hash)> = slots
    //             .iter()
    //             .zip(&positions)
    //             .zip(&chosen)
    //             .map(|((slot, position), at)| match at {
    //                 At::Stored(stored) => {
    //                     (slot.bind, view.recs[position.coll][*stored].hash.clone())
    //                 }
    //                 At::Incoming => {
    //                     unreachable!("a consume-side choice holds no element in flight")
    //                 }
    //             })
    //             .collect();
    //         expected.sort();
    //         let expected: Vec<Blake2b256Hash> =
    //             expected.into_iter().map(|(_, hash)| hash).collect();
    //         prop_assert_eq!(
    //             result_hashes(fire),
    //             expected,
    //             "the fire binds the records of GREEDY"
    //         );
    //     }
    //     _ => {
    //         prop_assert!(
    //             outcome.fires.is_empty(),
    //             "the model parks the join, and the engine fires"
    //         );
    //         let parked = read_view(&rig.host, program.kind).await;
    //         prop_assert!(
    //             parked.waits.iter().any(|wait| wait.name == name(index)),
    //             "the join parks"
    //         );
    //     }
    // }
    match outcome.fires.first() {
        Some(fire) => {
            prop_assert_eq!(body_name(&fire.continuation), name(index));
            let values = result_values(fire);
            let hashes = result_hashes(fire);
            let allowed = options.iter().zip(&admitted).any(|(chosen, admits)| {
                *admits
                    && bind_values(&view, &positions, &slots, chosen, None) == values
                    && bind_hashes(&view, &positions, &slots, chosen, None) == hashes
            });
            prop_assert!(
                allowed,
                "the fire binds {:?}, and no choice of play that the guard admits gives it: {:?}",
                values,
                options
            );
        }
        None => {
            prop_assert!(
                options.is_empty() || admitted.iter().any(|admits| !admits),
                "the guard admits every choice of play, and the engine parks the join"
            );
            let parked = read_view(&rig.host, program.kind).await;
            prop_assert!(
                parked.waits.iter().any(|wait| wait.name == name(index)),
                "the join parks"
            );
        }
    }
    Ok(())
}

/// The waiters that meet an element in flight `x`, in the order of OFFER (joins plan 4.5). On a
/// head shape, this is the head waiter of its collection. On a priority queue, these are the
/// heads of the lanes that see its level, oldest first. On the bag and the set, these are all
/// the waiters of its collection in the canonical order of DR-75.
/// Added by decision 102 (plan v4 C5.3, rule CR-1) and decision 124 (LP-1, L5): in play, OFFER
/// meets the waiters of the bag and the set in a random order. So on the bag and the set, the
/// order of this list is no claim: [`offer_outcomes`] takes the list as the set of waiters that
/// OFFER can meet.
fn offer_order<'v>(view: &'v View, x: &In) -> Vec<&'v Wait> {
    let all = seats(view, x.coll);
    match view.kind.win() {
        Win::Head => head_seat(view.kind, &all, None).into_iter().collect(),
        Win::Lanes => {
            let mut heads: Vec<(&Wait, i64)> = Vec::new();
            let lanes = [x.level, None];
            for lane in lanes
                .iter()
                .filter(|lane| lane.is_none() || x.level.is_some())
            {
                if let Some(head) = head_seat(view.kind, &all, Some(*lane)) {
                    let ord = all
                        .iter()
                        .find(|(wait, _, _)| wait.hash == head.hash)
                        .map(|(_, ord, _)| *ord)
                        .expect("a head is a seat");
                    if !heads.iter().any(|(wait, _)| wait.hash == head.hash) {
                        heads.push((head, ord));
                    }
                }
            }
            heads.sort_by_key(|(_, ord)| *ord);
            heads.into_iter().map(|(wait, _)| wait).collect()
        }
        Win::Scan | Win::Classes => {
            let mut waits: Vec<&Wait> = all.into_iter().map(|(wait, _, _)| wait).collect();
            waits.sort_by(|left, right| left.hash.cmp(&right.hash));
            waits
        }
    }
}

/// `SEARCH-PRODUCE(W, x)` of joins plan 3.5. It takes the first position on the collection of `x`
/// in group order that sees `x` and binds it. The completion of that position must fill every
/// other position.
/// Added by decision 102 (plan v4 C5.3, rule CR-1): the position of `x` does not depend on a
/// pick. A random pick takes a position that the group names once, and no other position names
/// its collection. So a completion with random picks exists exactly when the completion of
/// `GREEDY` exists. The choice here is the choice of `GREEDY`, with no pins. [`offer_outcomes`]
/// takes every choice of [`choices`] at the position.
fn search_produce(view: &View, wait: &Wait, x: &In) -> Option<(usize, Vec<At>)> {
    let distinct: BTreeSet<usize> = wait.colls.iter().copied().collect();
    if !distinct.iter().all(|coll| at_head(view, wait, *coll)) {
        return None;
    }
    let need = wait.colls.iter().filter(|coll| **coll == x.coll).count();
    if need == 0
        || !window(view, x.coll, need, Some(x))
            .iter()
            .any(|(at, _)| *at == At::Incoming)
    {
        return None;
    }
    let positions = positions_of(wait);
    for (index, position) in positions.iter().enumerate() {
        if position.coll != x.coll
            || !sees(view.kind, position.lane, x.level)
            || !position.pat.binds(x.value)
        {
            continue;
        }
        // Changed by decision 102 (plan v4 C5.3, rule CR-1): `search` takes pins, and this search
        // takes none. The line before:
        // if let Some(chosen) = search(view, &positions, Some((index, x))) {
        if let Some(chosen) = search(view, &positions, Some((index, x)), &[]) {
            return Some((index, chosen));
        }
    }
    None
}

/// The first fire of OFFER for the element in flight `x`: the waiter and its choice. A refusal
/// of the head waiter of a head shape stores `x`.
/// Added by decision 102 (plan v4 C5.3, rule CR-1): [`offer_outcomes`] takes this one outcome on
/// a head shape and a priority queue. On the bag and the set, this fire is one outcome of play
/// among others.
fn offer_fire<'v>(view: &'v View, x: &In) -> Option<(&'v Wait, Vec<At>)> {
    for wait in offer_order(view, x) {
        let found = search_produce(view, wait, x).filter(|(_, chosen)| {
            wait.guard.admits(&bind_values(
                view,
                &positions_of(wait),
                &wait.slots,
                chosen,
                Some(x),
            ))
        });
        match (found, view.kind.win()) {
            (Some((_, chosen)), _) => return Some((wait, chosen)),
            (None, Win::Head) => return None,
            (None, Win::Lanes | Win::Scan | Win::Classes) => continue,
        }
    }
    None
}

/// Added by decision 102 (plan v4 C5.3, rule CR-1) and decision 124 (LP-1, L5): the outcomes of
/// OFFER that play can reach for an element in flight. `fires` holds each waiter with each
/// choice that it can fire, and `stores` is true when play can store the element.
struct OfferOutcomes<'v> {
    fires: Vec<(&'v Wait, Vec<At>)>,
    stores: bool,
}

/// Added by decision 102 (plan v4 C5.3, rule CR-1) and decision 124 (LP-1, L5): the outcomes of
/// OFFER that play can reach for the element in flight `x`. A head shape and a priority queue
/// keep the one outcome of [`offer_fire`]. On the bag and the set, OFFER meets the join groups of
/// the collection of `x` in the order of `joins_on`, and the waiters of each group in any order
/// (`Cx::pick_union_order`). For each waiter, [`search_produce`] gives the position of `x`, and
/// that position does not depend on a pick. The choices of the waiter are the choices of
/// [`choices`] at that position. A waiter passes when it has no choice or has a choice that its
/// guard refuses. A fire of a waiter with a choice is allowed when its guard admits the choice
/// and each waiter of each earlier group passes. A store of `x` is allowed when every waiter
/// passes.
/// Added by decision 124 (LP-1, L5; J1 review D2): `groups` holds the join groups of the
/// collection of `x` in the order of `joins_on`, each as the collections of its positions.
fn offer_outcomes<'v>(view: &'v View, x: &In, groups: &[Vec<usize>]) -> OfferOutcomes<'v> {
    match view.kind.win() {
        Win::Head | Win::Lanes => match offer_fire(view, x) {
            Some(fire) => OfferOutcomes {
                fires: vec![fire],
                stores: false,
            },
            None => OfferOutcomes {
                fires: Vec::new(),
                stores: true,
            },
        },
        Win::Scan | Win::Classes => {
            let waits = offer_order(view, x);
            let mut fires: Vec<(&Wait, Vec<At>)> = Vec::with_capacity(waits.len());
            // Added by decision 124 (LP-1, L5; J1 review D2): true while each waiter of each
            // earlier group passes, and the count of the waiters that the groups list.
            let mut reached = true;
            let mut listed = 0;
            for group in groups {
                let mut passed = true;
                for wait in waits.iter().copied().filter(|wait| wait.colls == *group) {
                    listed += 1;
                    let Some((position, _)) = search_produce(view, wait, x) else {
                        continue;
                    };
                    let positions = positions_of(wait);
                    let options = choices(view, &positions, Some((position, x)));
                    let mut refused = options.is_empty();
                    for chosen in options {
                        let values = bind_values(view, &positions, &wait.slots, &chosen, Some(x));
                        match (wait.guard.admits(&values), reached) {
                            (true, true) => fires.push((wait, chosen)),
                            (true, false) => {}
                            (false, _) => refused = true,
                        }
                    }
                    passed = passed && refused;
                }
                reached = reached && passed;
            }
            assert_eq!(
                listed,
                waits.len(),
                "each waiter of the collection of x lies in one join group of `joins_on`: {groups:?}"
            );
            OfferOutcomes {
                fires,
                stores: reached,
            }
        }
    }
}

/// Added by decision 102 (plan v4 C5.3, rule CR-1): non-vacuity. True when OFFER of the element
/// in flight `x` reaches a random pick of play. On the bag and the set, OFFER meets two or more
/// waiters, or a waiter that it meets has two or more choices at the position of
/// `SEARCH-PRODUCE` ([`offer_has_choices`]). A head shape and a priority queue have no random
/// pick.
fn offer_is_random(view: &View, x: &In) -> bool {
    match view.kind.win() {
        Win::Head | Win::Lanes => false,
        Win::Scan | Win::Classes => offer_order(view, x).len() >= 2 || offer_has_choices(view, x),
    }
}

/// Added by decision 102 (plan v4 C5.3, rule CR-1): non-vacuity. True when a waiter that OFFER of
/// the element in flight `x` meets on the bag or the set has two or more choices at the position
/// of `SEARCH-PRODUCE`: [`choices`] with a fixed position. A head shape and a priority queue have
/// no random pick.
fn offer_has_choices(view: &View, x: &In) -> bool {
    match view.kind.win() {
        Win::Head | Win::Lanes => false,
        Win::Scan | Win::Classes => {
            offer_order(view, x)
                .iter()
                .any(|wait| match search_produce(view, wait, x) {
                    Some((position, _)) => {
                        choices(view, &positions_of(wait), Some((position, x))).len() >= 2
                    }
                    None => false,
                })
        }
    }
}

/// A program prefix, and a send after it.
/// Added by decision 102 (plan v4 C5.3, rule CR-1): non-vacuity. One case in four registers a
/// join on two collections of a bag or a set first ([`arb_join_then_sends`]).
fn arb_prefix_and_send() -> impl Strategy<Value = (Program, Step)> {
    // Changed by decision 102 (plan v4 C5.3, rule CR-1): non-vacuity. The line before:
    // arb_program(&Kind::ALL, 1..12, COLLS, 4, true).prop_flat_map(|program| {
    prop_oneof![
        3 => arb_program(&Kind::ALL, 1..12, COLLS, 4, true).prop_flat_map(|program| {
            let kind = program.kind;
            (
                Just(program),
                // Changed by decision 103 (decision 118, rule CR-9): the send `x` draws its
                // persistence ([`arb_x_persistent`]). The text before:
                // (0..COLLS, 0i64..4, arb_level(kind)).prop_map(|(coll, value, level)| Step::Send {
                //     coll,
                //     value,
                //     persistent: false,
                //     level,
                // }),
                (0..COLLS, 0i64..4, arb_level(kind), arb_x_persistent()).prop_map(
                    |(coll, value, level, persistent)| Step::Send {
                        coll,
                        value,
                        persistent,
                        level,
                    },
                ),
            )
        }),
        1 => arb_join_then_sends(),
    ]
}

/// Added by decision 103 (decision 118, rule CR-9): the share of the sends `x` of
/// `prop_join_produce_search_agrees_with_head` that are persistent.
const PERSISTENT_X_SHARE: f64 = 0.25;

/// Added by decision 103 (decision 118, rule CR-9): the persistence of the send `x` of
/// `prop_join_produce_search_agrees_with_head`, true in [`PERSISTENT_X_SHARE`] of the cases. So
/// the property compares the produce rule of a persistent element in flight with the engine.
fn arb_x_persistent() -> impl Strategy<Value = bool> { prop::bool::weighted(PERSISTENT_X_SHARE) }

/// Added by decision 102 (plan v4 C5.3, rule CR-1): non-vacuity. A program of a bag or a set that
/// registers one join on two collections, then sends two or three values on the collection that
/// is not the collection of `x`, and then the send `x`, which is persistent in
/// [`PERSISTENT_X_SHARE`] of the cases. The join names the collection of `x` once, and the other
/// collection once or twice. So `x` meets a waiter whose other collection holds two or three
/// records, and a bind on a collection that the group names once takes a random record. The
/// waiter then often has two or more choices at the position of `SEARCH-PRODUCE`
/// ([`offer_has_choices`]).
fn arb_join_then_sends() -> impl Strategy<Value = (Program, Step)> {
    let kinds: &'static [Kind] = &RANDOM_PICK_KINDS;
    let colls = (0..COLLS, 1..COLLS, any::<bool>());
    let pats = (arb_pat(4), prop::collection::vec(arb_pat(4), 1..=2));
    let join = (any::<bool>(), 0u8..4, 0i64..4);
    let sends = (
        prop::collection::vec(0i64..4, 2..=3),
        0i64..4,
        arb_x_persistent(),
    );
    (prop::sample::select(kinds), colls, pats, join, sends).prop_map(
        |(
            kind,
            (x_coll, offset, x_last),
            (x_pat, other_pats),
            join,
            (values, x_value, x_persistent),
        )| {
            let (peek, guard, guard_value) = join;
            let other = (x_coll + offset) % COLLS;
            let x_bind = Bind {
                coll: x_coll,
                pat: x_pat,
                lane: None,
            };
            let mut binds: Vec<Bind> = Vec::with_capacity(1 + other_pats.len());
            binds.extend(other_pats.iter().map(|pat| Bind {
                coll: other,
                pat: *pat,
                lane: None,
            }));
            match x_last {
                true => binds.push(x_bind),
                false => binds.insert(0, x_bind),
            }
            let open = binds.iter().all(|bind| bind.pat == Pat::Any);
            let mut steps: Vec<Step> = Vec::with_capacity(1 + values.len());
            steps.push(Step::Join {
                binds,
                peek,
                persistent: false,
                guard: match (open, guard) {
                    (true, 1) => Guard::LastAboveFirst,
                    (true, 2) => Guard::LastIs(guard_value),
                    _ => Guard::Open,
                },
            });
            steps.extend(values.iter().map(|value| Step::Send {
                coll: other,
                value: *value,
                persistent: false,
                level: None,
            }));
            let x = Step::Send {
                coll: x_coll,
                value: x_value,
                persistent: x_persistent,
                level: None,
            };
            (Program { kind, steps }, x)
        },
    )
}

/// Added by decision 102 (plan v4 C5.3, rule CR-1): non-vacuity. The cases of
/// `prop_join_produce_search_agrees_with_head` that reach the random case
/// ([`offer_is_random`]), and the engine runs the send, for each kind. The seed is fixed, so the
/// counts are deterministic.
static PRODUCE_RANDOM_CASES: KindCounts = KindCounts::new();

/// Added by decision 102 (plan v4 C5.3, rule CR-1): non-vacuity. The cases of
/// `prop_join_produce_search_agrees_with_head` in which a waiter has two or more choices at the
/// position of `SEARCH-PRODUCE` ([`offer_has_choices`]), and the engine runs the send, for each
/// kind. The seed is fixed, so the counts are deterministic.
static PRODUCE_CHOICE_CASES: KindCounts = KindCounts::new();

/// Added by decision 102 (plan v4 C5.3, rule CR-1): non-vacuity. The cases of
/// `prop_join_produce_search_agrees_with_head` in which the send `x` is persistent and the
/// allowed fire that the engine makes binds `x` at two or more positions, for each kind. The seed
/// is fixed, so the counts are deterministic.
static PRODUCE_PERSISTENT_X_CASES: KindCounts = KindCounts::new();

// Changed by decision 102 (plan v4 C5.3, rule CR-1) and decision 124 (LP-1, L5): on the bag and
// the set, OFFER meets the waiters in a random order and play binds a random choice, so the doc
// below replaces this one.
// /// `join_produce_search_agrees_with_head` (`JoinCascade.v`, headline 2). A send fires the first
// /// waiter in the order of OFFER whose `SEARCH-PRODUCE` completes and whose guard admits. The fire
// /// has the choice of the search. `GREEDY` over the state with the sent record stored can bind
// /// that record at a position. Then the completion at that position is the choice of `GREEDY`, and
// /// the fire completes at that position or before it.
/// `join_produce_search_agrees_with_head` (`JoinCascade.v`, headline 2). The model gives the
/// outcomes of the send that play can reach ([`offer_outcomes`]). On a head shape and a priority
/// queue, the send fires the first waiter in the order of OFFER whose `SEARCH-PRODUCE` completes
/// and whose guard admits, with the choice of the search. When no such waiter exists, the send is
/// stored. On the bag and the set, OFFER meets the join groups of the collection of the send in
/// the order of `joins_on`, and the waiters of each group in a random order. A bind on a
/// collection that the group names once takes a random record. A waiter passes when it has no
/// choice or has a choice that its guard refuses. So a fire of a waiter is allowed with each
/// choice at the position of `SEARCH-PRODUCE` that its guard admits, when each waiter of each
/// earlier group passes. A store is allowed when every waiter passes.
///
/// The check is an allowed-set check, with no probabilistic assertion. A store of the engine must
/// be an allowed store. A fire of the engine must be an allowed fire: the same waiter, the same
/// values in bind order and the same source hashes. Then, for the waiter that the engine fired,
/// `GREEDY` over the state with the sent record stored can bind that record at a position. Then
/// the completion at that position is the choice of `GREEDY`, and the fire completes at that
/// position or before it.
///
/// Added by decision 102 (plan v4 C5.3, rule CR-1): non-vacuity. On the bag and on the set, some
/// case reaches the random case: OFFER meets two or more waiters, or a waiter has two or more
/// choices ([`PRODUCE_RANDOM_CASES`]). Some case has a waiter with two or more choices at the
/// position of `SEARCH-PRODUCE` ([`PRODUCE_CHOICE_CASES`]). Some case fires a persistent `x` at
/// two or more positions ([`PRODUCE_PERSISTENT_X_CASES`]).
#[test]
fn prop_join_produce_search_agrees_with_head() {
    check(pure_config(), arb_prefix_and_send(), |(program, send)| {
        tokio_runtime().block_on(produce_case(program, send))
    });
    // Added by decision 102 (plan v4 C5.3, rule CR-1): non-vacuity.
    PRODUCE_RANDOM_CASES.assert_each_kind(
        "prop_join_produce_search_agrees_with_head",
        "with two or more waiters or choices",
    );
    PRODUCE_CHOICE_CASES.assert_total(
        "prop_join_produce_search_agrees_with_head",
        "with a waiter that has two or more choices",
    );
    // Added by decision 102 (plan v4 C5.3, rule CR-1): non-vacuity.
    PRODUCE_PERSISTENT_X_CASES.assert_total(
        "prop_join_produce_search_agrees_with_head",
        "with a persistent x at two or more positions",
    );
}

/// One case of `prop_join_produce_search_agrees_with_head`.
async fn produce_case(program: Program, send_step: Step) -> Result<(), TestCaseError> {
    let rig = Rig::new(program.kind).await;
    for (index, step) in program.steps.iter().enumerate() {
        run_step(&rig, step, index).await?;
    }
    rig.log().await;
    let index = program.steps.len();
    let SpaceOp::Produce(op) = space_op(&send_step, index) else {
        return Err(failure("the last step is a send"));
    };
    let coll = coll_of_inner(&op.channel);
    let view = read_view(&rig.host, program.kind).await;
    let ord = own_ord(&rig, &op, coll).await;
    let level = op
        .params
        .get(PRIORITY_PARAM)
        .map(|level| number(std::slice::from_ref(level)));
    let x = In {
        coll,
        ord,
        level,
        value: number(&op.data.pars),
        // Added by decision 103 (decision 118, rule CR-9): the persist flag of the element in
        // flight.
        persist: op.persistent,
    };
    // Changed by decision 103 (decision 118, rule CR-9): the identity of `x` carries its
    // persistence. The line before:
    // let own = Produce::create(&data(coll), &own_record(program.kind, &op, ord), false).hash;
    let own = Produce::create(
        &data(coll),
        &own_record(program.kind, &op, ord),
        op.persistent,
    )
    .hash;
    // Added by decision 124 (LP-1, L5; J1 review D2): the join groups of the collection of `x` in
    // the order of `joins_on`, each as the collections of its positions. The engine reads the
    // same list of the hot store (`Cx::joins_on`), and `read_view` reads it too.
    let groups: Vec<Vec<usize>> = rig
        .host
        .get_joins(data(coll))
        .await
        .iter()
        .map(|group| group.iter().map(coll_of).collect())
        .collect();
    // Changed by decision 102 (plan v4 C5.3, rule CR-1) and decision 124 (LP-1, L5): the model
    // gives every outcome that play can reach. The line before:
    // let expected = offer_fire(&view, &x);
    let allowed = offer_outcomes(&view, &x, &groups);
    let outcome = match rig.run(SpaceOp::Produce(op.clone())).await {
        Ok(outcome) => outcome,
        Err(error) => return expected_refusal(program.kind, &error),
    };
    // Added by decision 102 (plan v4 C5.3, rule CR-1): non-vacuity. The case reaches the random
    // case when OFFER on the bag or the set meets two or more waiters, or when a waiter has two or
    // more choices ([`offer_is_random`]). A waiter with two or more choices counts on its own
    // ([`offer_has_choices`]).
    if offer_is_random(&view, &x) {
        PRODUCE_RANDOM_CASES.add(program.kind);
    }
    if offer_has_choices(&view, &x) {
        PRODUCE_CHOICE_CASES.add(program.kind);
    }
    // Changed by decision 102 (plan v4 C5.3, rule CR-1) and decision 124 (LP-1, L5): a store of
    // the engine must be an allowed store, and a fire of the engine must be an allowed fire: the
    // same waiter, the same values in bind order and the same source hashes. The text before:
    // let Some((wait, chosen)) = expected else {
    //     prop_assert!(
    //         outcome.fires.is_empty(),
    //         "the model stores the send, and the engine fires"
    //     );
    //     return Ok(());
    // };
    // let fire = outcome.fires.first().ok_or_else(|| {
    //     failure(format!(
    //         "the model fires {}, and the engine stores the send",
    //         wait.name
    //     ))
    // })?;
    // prop_assert_eq!(body_name(&fire.continuation), wait.name.clone());
    // let positions = positions_of(wait);
    // prop_assert_eq!(
    //     result_values(fire),
    //     bind_values(&view, &positions, &wait.slots, &chosen, Some(&x))
    // );
    // let hash_at = |position: &Position, at: &At, extended: &View| match at {
    //     At::Stored(stored) => extended.recs[position.coll][*stored].hash.clone(),
    //     At::Incoming => own.clone(),
    // };
    // let mut by_bind: Vec<(usize, Blake2b256Hash)> = wait
    //     .slots
    //     .iter()
    //     .zip(&positions)
    //     .zip(&chosen)
    //     .map(|((slot, position), at)| (slot.bind, hash_at(position, at, &view)))
    //     .collect();
    // by_bind.sort();
    // let hashes: Vec<Blake2b256Hash> = by_bind.into_iter().map(|(_, hash)| hash).collect();
    // prop_assert_eq!(
    //     result_hashes(fire),
    //     hashes.clone(),
    //     "the fire binds the choice of SEARCH-PRODUCE"
    // );
    let Some(fire) = outcome.fires.first() else {
        prop_assert!(
            allowed.stores,
            "a waiter fires in every outcome of play, and the engine stores the send"
        );
        return Ok(());
    };
    let fired = body_name(&fire.continuation);
    let values = result_values(fire);
    let hashes = result_hashes(fire);
    let Some(&(wait, ref chosen)) = allowed.fires.iter().find(|(wait, chosen)| {
        let positions = positions_of(wait);
        wait.name == fired
            && bind_values(&view, &positions, &wait.slots, chosen, Some(&x)) == values
            && bind_hashes(&view, &positions, &wait.slots, chosen, Some(&own)) == hashes
    }) else {
        let reachable: Vec<(&str, &[At])> = allowed
            .fires
            .iter()
            .map(|(wait, chosen)| (wait.name.as_str(), chosen.as_slice()))
            .collect();
        return Err(failure(format!(
            "the engine fires {fired} with {values:?}, and play can reach only {reachable:?}"
        )));
    };
    // Added by decision 102 (plan v4 C5.3, rule CR-1): non-vacuity. The allowed fire binds a
    // persistent `x` at two or more positions.
    if x.persist && chosen.iter().filter(|at| **at == At::Incoming).count() >= 2 {
        PRODUCE_PERSISTENT_X_CASES.add(program.kind);
    }
    let positions = positions_of(wait);
    let mut stored = view.clone();
    stored.recs[coll].push(Rec {
        ord,
        level,
        value: x.value,
        persist: false,
        hash: own.clone(),
    });
    let x_index = stored.recs[coll].len() - 1;
    let incoming_bind = fire
        .results
        .iter()
        .zip(result_hashes(fire))
        .position(|(_, hash)| hash == own)
        .ok_or_else(|| failure("the fire binds the element in flight"))?;
    let fired_at = wait
        .slots
        .iter()
        .position(|slot| slot.bind == incoming_bind)
        .ok_or_else(|| failure("the bind of the element in flight has a slot"))?;
    // Added by decision 103 (decision 118, rule CR-9): the GREEDY part below mirrors headline 2
    // (`join_produce_search_agrees_with_head`), a theorem of the search before decision 118. Its
    // GREEDY claims x, so the equality holds for a non-persistent x only. A persistent x comes
    // first in the window at the other positions of its collection (E1, CR-8), and no headline of
    // the second search restates headline 2. The allowed set above checks a persistent x.
    if x.persist {
        return Ok(());
    }
    // Changed by decision 102 (plan v4 C5.3, rule CR-1): `search` takes pins. `GREEDY` of the
    // model takes none, here for the waiter that the engine fired. The line before:
    // if let Some(greedy) = search(&stored, &positions, None) {
    if let Some(greedy) = search(&stored, &positions, None, &[]) {
        if let Some(p_star) = greedy
            .iter()
            .zip(&positions)
            .position(|(at, position)| position.coll == coll && *at == At::Stored(x_index))
        {
            prop_assert!(
                fired_at <= p_star,
                "SEARCH-PRODUCE completes at or before the position of GREEDY: {} > {}",
                fired_at,
                p_star
            );
            // Changed by decision 102 (plan v4 C5.3, rule CR-1): `search` takes pins, and the
            // completion of the model takes none. The text before:
            // let completion: Option<Vec<At>> =
            //     search(&view, &positions, Some((p_star, &x))).map(|chosen| {
            //         chosen
            //             .into_iter()
            //             .map(|at| match at {
            //                 At::Incoming => At::Stored(x_index),
            //                 stored_at => stored_at,
            //             })
            //             .collect()
            //     });
            let completion: Option<Vec<At>> = search(&view, &positions, Some((p_star, &x)), &[])
                .map(|chosen| {
                    chosen
                        .into_iter()
                        .map(|at| match at {
                            At::Incoming => At::Stored(x_index),
                            stored_at => stored_at,
                        })
                        .collect()
                });
            prop_assert_eq!(
                completion,
                Some(greedy),
                "at the position of GREEDY, the completion gives the choice of GREEDY"
            );
        }
    }
    Ok(())
}

/// The sends of `(value, persistent)` on the collection `coll` of a bag whose records follow each
/// other in the canonical order of DR-75. The salts are searched in a fixed order.
fn canonical_sends(coll: usize, entries: &[(i64, bool)]) -> Vec<ProduceOp> {
    let mut sends: Vec<ProduceOp> = Vec::with_capacity(entries.len());
    let mut salt = 0;
    for (value, persistent) in entries {
        loop {
            salt += 1;
            let candidate = send(coll, *value, *persistent, None, salt);
            let hash = Produce::create(
                &data(coll),
                &record::wrap_datum(0, &candidate.data),
                *persistent,
            )
            .hash;
            if sends.last().is_none_or(|last| {
                Produce::create(
                    &data(coll),
                    &record::wrap_datum(0, &last.data),
                    last.persistent,
                )
                .hash
                    < hash
            }) {
                sends.push(candidate);
                break;
            }
        }
    }
    sends
}

/// `greedy_join_can_miss_assignment` (`JoinCascade.v`, headline 17), the negative result. On a
/// bag, the join `[c, c]` with the patterns any and `v` binds the first record of the canonical
/// order to its first bind. It has no backtracking. With the one record `v` first, the join parks,
/// although the assignment of another record and `v` exists. With `v` at a later place, it
/// fires. INV is relative to this search until P12.
#[test]
fn prop_greedy_join_limit() {
    let strategy = (2usize..6).prop_flat_map(|records| (Just(records), 0..records, 10i64..20));
    check(pure_config(), strategy, |(records, place, v)| {
        tokio_runtime().block_on(async move {
            let rig = Rig::new(Kind::Bag).await;
            let entries: Vec<(i64, bool)> = (0..records)
                .map(|index| match index == place {
                    true => (v, false),
                    false => (i64::try_from(index).expect("a small value"), false),
                })
                .collect();
            for op in canonical_sends(0, &entries) {
                rig.run(SpaceOp::Produce(op)).await.map_err(failure)?;
            }
            let binds = [
                Bind {
                    coll: 0,
                    pat: Pat::Any,
                    lane: None,
                },
                Bind {
                    coll: 0,
                    pat: Pat::Is(v),
                    lane: None,
                },
            ];
            let outcome = rig
                .run(SpaceOp::Join(join_of(
                    &binds,
                    "g",
                    false,
                    false,
                    Guard::Open,
                )))
                .await
                .map_err(failure)?;
            match place {
                0 => prop_assert!(
                    outcome.fires.is_empty(),
                    "greedy gives v to the first bind and parks, although an assignment exists"
                ),
                _ => {
                    prop_assert_eq!(outcome.fires.len(), 1);
                    prop_assert_eq!(result_values(&outcome.fires[0]), vec![0, v]);
                }
            }
            Ok(())
        })
    });
}

// ─── Headline 13 and the record of version 2 ─────────────────────────────────────────────────

/// `bind_permutation_restores_order` (`JoinCascade.v`, headline 13). The canonical group order is
/// a function of the multiset of the binds (FR-1'). The slots put the results of the receipt back
/// into bind order for every permutation. A join over collections with one value each delivers
/// the value of the collection of each bind in bind order. This holds for every order of its
/// binds.
#[test]
fn prop_bind_permutation_restores_order() {
    let strategy = (
        prop::collection::vec(0..COLLS, 2..=4),
        any::<prop::sample::Index>(),
        prop::sample::select(&[Kind::Queue, Kind::Stack, Kind::Bag, Kind::Set, Kind::Pq][..]),
    );
    check(pure_config(), strategy, |(colls, seed, kind)| {
        let mut permuted = colls.clone();
        let rotation = seed.index(permuted.len());
        permuted.rotate_left(rotation);
        let last = permuted.len() - 1;
        permuted.swap(0, last);
        let channels: Vec<Par> = colls.iter().map(|coll| ch(*coll)).collect();
        let shuffled: Vec<Par> = permuted.iter().map(|coll| ch(*coll)).collect();
        let refs: Vec<&Par> = channels.iter().collect();
        let shuffled_refs: Vec<&Par> = shuffled.iter().collect();
        prop_assert_eq!(
            support::agent::join_group(SPACE, &refs),
            support::agent::join_group(SPACE, &shuffled_refs),
            "the group is a function of the bind multiset"
        );
        let positions = support::agent::group_positions(SPACE, &shuffled_refs);
        let slots: Vec<JoinSlot> = positions
            .iter()
            .map(|bind| JoinSlot {
                ord: 0,
                bind: *bind,
                policy: WaiterPolicy::None,
            })
            .collect();
        let results: Vec<usize> = positions.clone();
        let restored = record::restore_bind_order(&slots, results).map_err(failure)?;
        prop_assert_eq!(restored, (0..permuted.len()).collect::<Vec<_>>());
        tokio_runtime().block_on(async move {
            let rig = Rig::new(kind).await;
            let level = match kind {
                Kind::Pq => Some(0),
                Kind::Queue | Kind::Stack | Kind::Cell | Kind::Bag | Kind::Set => None,
            };
            for coll in 0..COLLS {
                for copy in 0..4 {
                    let value = i64::try_from(10 * coll).expect("a small value");
                    rig.run(SpaceOp::Produce(send(coll, value, false, level, copy)))
                        .await
                        .map_err(failure)?;
                }
            }
            let binds: Vec<Bind> = permuted
                .iter()
                .map(|coll| Bind {
                    coll: *coll,
                    pat: Pat::Any,
                    lane: None,
                })
                .collect();
            let outcome = rig
                .run(SpaceOp::Join(join_of(
                    &binds,
                    "p",
                    false,
                    false,
                    Guard::Open,
                )))
                .await
                .map_err(failure)?;
            let expected: Vec<i64> = permuted
                .iter()
                .map(|coll| i64::try_from(10 * coll).expect("a small value"))
                .collect();
            match kind {
                Kind::Set if permuted.len() > BTreeSet::from_iter(permuted.iter()).len() => {
                    prop_assert!(outcome.fires.is_empty(), "a set binds each class once");
                }
                _ => {
                    prop_assert_eq!(outcome.fires.len(), 1);
                    prop_assert_eq!(result_values(&outcome.fires[0]), expected);
                }
            }
            Ok(())
        })
    });
}

/// `continuation_v2_roundtrip` (`RecordEnvelope.v`). Take two or more slots whose bind indices are
/// a permutation, whose orders are natural and whose lanes are absent or a level. The decoder of
/// version 2 gives back the slots and the continuation of the wrap. The decoders of version 1
/// reject the image of version 2. The decoder of version 2 rejects the image of version 1. A slot
/// list that is not a permutation has no record.
#[test]
fn prop_continuation_v2_roundtrip() {
    let strategy = (2usize..6).prop_flat_map(|width| {
        (
            Just(width),
            Just((0..width).collect::<Vec<usize>>()).prop_shuffle(),
            prop::collection::vec(0i64..1_000_000, width),
            prop::collection::vec(prop_oneof![Just(None), (0i64..8).prop_map(Some)], width),
            0i64..1_000,
            any::<bool>(),
        )
    });
    check(
        pure_config(),
        strategy,
        |(width, binds, ords, lanes, ord, guarded)| {
            let slots: Vec<JoinSlot> = binds
                .iter()
                .zip(&ords)
                .zip(&lanes)
                .map(|((bind, ord), lane)| JoinSlot {
                    ord: *ord,
                    bind: *bind,
                    policy: WaiterPolicy::of_level(*lane),
                })
                .collect();
            let guard = Guard::LastAboveFirst.par(width).filter(|_| guarded);
            let continuation = support::agent::named("v2", guard);
            let wrapped = record::wrap_join_continuation(&slots, &continuation).map_err(failure)?;
            let decoded = record::unwrap_join_continuation(&wrapped).map_err(failure)?;
            prop_assert_eq!(&decoded.slots, &slots);
            prop_assert_eq!(&decoded.continuation, &continuation);
            prop_assert!(
                record::unwrap_continuation(&wrapped).is_err(),
                "version 1 rejects version 2"
            );
            prop_assert!(
                record::unwrap_pq_continuation(&wrapped).is_err(),
                "the lane decoder rejects version 2"
            );
            let v1 = record::wrap_continuation(ord, &continuation).map_err(failure)?;
            prop_assert!(
                record::unwrap_join_continuation(&v1).is_err(),
                "version 2 rejects version 1"
            );
            let mut broken = slots.clone();
            broken[0].bind = broken[1].bind;
            prop_assert!(
                record::wrap_join_continuation(&broken, &continuation).is_err(),
                "a slot list that is not a permutation has no record"
            );
            Ok(())
        },
    );
}

// ─── The staged operations: headlines 3, 4, 5, 12 and 14 ─────────────────────────────────────

/// The collection of the data channel `channel`, or `None` for another channel of the space (a
/// counter channel).
fn data_coll(channel: &Par) -> Option<usize> { (0..COLLS).find(|coll| data(*coll) == *channel) }

/// The collection of the counter channel `channel`, or `None` for another channel.
fn counter_coll(channel: &Par) -> Option<usize> {
    (0..COLLS).find(|coll| counter_channel(SPACE, &ch(*coll)) == *channel)
}

/// The data channels that the staged operation `staged` stages on. For a produce, these are its
/// channel and the group of its fire. For a consume, these are its channels.
fn staged_colls(staged: &StagedCopy) -> BTreeSet<usize> {
    staged.channels().iter().filter_map(data_coll).collect()
}

/// The record of a stored datum as the model sees it.
fn rec_of(channel: &Par, record_bytes: &ListParWithRandom, persist: bool) -> Rec {
    let decoded = record::unwrap_pq_datum(record_bytes).expect("a datum record");
    Rec {
        ord: decoded.ord,
        level: decoded.priority,
        value: number(&decoded.datum.pars),
        persist,
        hash: Produce::create(channel, record_bytes, persist).hash,
    }
}

/// The waiter of a parked or fired continuation as the model sees it. A continuation that is not
/// a record of a user waiter gives `None`: `K_lift`, `K_retire` or a counter take.
fn wait_of(
    channels: &[Par],
    patterns: &[BindPattern],
    continuation: &models::rhoapi::TaggedContinuation,
    persist: bool,
    peek: bool,
    hash: Blake2b256Hash,
) -> Option<Wait> {
    let colls: Vec<usize> = channels
        .iter()
        .map(data_coll)
        .collect::<Option<Vec<usize>>>()?;
    let (slots, inner) = match channels.len() {
        1 => {
            let parked = record::unwrap_pq_continuation(continuation).ok()?;
            (
                vec![JoinSlot {
                    ord: parked.ord,
                    bind: 0,
                    policy: WaiterPolicy::of_level(parked.priority),
                }],
                parked.continuation,
            )
        }
        _ => match record::unwrap_join_continuation(continuation) {
            Ok(parked) => (parked.slots, parked.continuation),
            Err(_) => {
                let parked = record::unwrap_pq_continuation(continuation).ok()?;
                (
                    channels
                        .iter()
                        .map(|_| JoinSlot {
                            ord: parked.ord,
                            bind: 0,
                            policy: WaiterPolicy::of_level(parked.priority),
                        })
                        .collect(),
                    parked.continuation,
                )
            }
        },
    };
    let name = body_name(&inner);
    step_of(&name)?;
    let guard = continuation.guard.as_ref().or(inner.guard.as_ref());
    Some(Wait {
        hash,
        colls,
        pats: patterns.iter().map(pat_of).collect(),
        slots,
        persist,
        peek,
        guard: Guard::of(guard, channels.len()),
        name,
    })
}

/// True when the order `ord` of an entry precedes the order `head` of the head of its side
/// (`Discipline::precedes`). A queue and a lane of a priority queue keep the least order first. A
/// stack keeps the greatest order first. A cell lets a re-entering entry reach the head.
fn precedes(kind: Kind, ord: i64, head: i64) -> bool {
    match kind {
        Kind::Queue | Kind::Pq => ord < head,
        Kind::Stack => ord > head,
        Kind::Cell | Kind::Bag | Kind::Set => true,
    }
}

/// `AT-HEAD-INCOMING(W, c)` of joins plan 3.3 for the waiter `wait` of a consume that fires
/// without a park. Take each lane of the waiter on each of its collections. The lane is empty,
/// or the order of the waiter precedes the order of the lane head. A fresh waiter has the newest order,
/// so the rule is the fresh rule. A re-entering waiter keeps the order of its first round.
fn reaches_heads(view: &View, wait: &Wait) -> bool {
    match view.kind.win() {
        Win::Scan | Win::Classes => true,
        Win::Head | Win::Lanes => wait.colls.iter().enumerate().all(|(index, coll)| {
            let all = seats(view, *coll);
            let lane = match view.kind.win() {
                Win::Lanes => Some(wait.slots[index].policy.level()),
                Win::Head | Win::Scan | Win::Classes => None,
            };
            match head_seat(view.kind, &all, lane) {
                None => true,
                Some(head) => {
                    let head_ord = all
                        .iter()
                        .find(|(seat, _, _)| seat.hash == head.hash)
                        .map(|(_, ord, _)| *ord)
                        .expect("a head is a seat");
                    precedes(view.kind, wait.slots[index].ord, head_ord)
                }
            }
        }),
    }
}

/// The model of the staged view of one transaction. It starts as the view before the
/// transaction. Each staged operation changes it in staging order, as the host applies them at
/// the commit.
struct Shadow {
    view: View,
}

impl Shadow {
    /// Removes the stored records that the candidates `candidates` bind: each record that is not
    /// persistent, by its identity.
    fn remove(
        &mut self,
        candidates: &[rspace_plus_plus::rspace::internal::ConsumeCandidate<
            Par,
            ListParWithRandom,
        >],
    ) {
        for candidate in candidates {
            if candidate.datum_index < 0 || candidate.datum.persist {
                continue;
            }
            let Some(coll) = data_coll(&candidate.channel) else {
                continue;
            };
            let recs = &mut self.view.recs[coll];
            if let Some(index) = recs
                .iter()
                .position(|rec| rec.hash == candidate.datum.source.hash)
            {
                recs.remove(index);
            }
        }
    }

    /// Checks the windows of the fire of the waiter `wait` with the candidates `candidates`, in
    /// group order (joins plan 3.2). Each bound stored record lies inside the window of its
    /// collection, visible to its lane. The element in flight `x` lies inside the window of its
    /// collection.
    fn check_windows(
        &self,
        wait: &Wait,
        candidates: &[rspace_plus_plus::rspace::internal::ConsumeCandidate<
            Par,
            ListParWithRandom,
        >],
        x: Option<&In>,
    ) -> Result<(), TestCaseError> {
        let view = &self.view;
        for (index, candidate) in candidates.iter().enumerate() {
            let coll = wait.colls[index];
            let need = wait.colls.iter().filter(|other| **other == coll).count();
            let window = window(view, coll, need, x);
            let at = match candidate.datum_index {
                -1 => At::Incoming,
                _ => At::Stored(
                    view.recs[coll]
                        .iter()
                        .position(|rec| rec.hash == candidate.datum.source.hash)
                        .ok_or_else(|| {
                            failure(format!("{} binds a record outside the view", wait.name))
                        })?,
                ),
            };
            let Some((_, level)) = window.iter().find(|(entry, _)| *entry == at) else {
                return Err(failure(format!(
                    "{} binds an entry outside its window: {at:?}",
                    wait.name
                )));
            };
            prop_assert!(
                sees(view.kind, wait.slots[index].policy.level(), *level),
                "{} binds a level that its lane does not see",
                wait.name
            );
        }
        Ok(())
    }

    /// Applies the staged operation `staged` and checks the fires of user waiters on the way with
    /// `join_fires_heads_only`. Each waiter is at the head of each head-only collection, and each
    /// bound entry is in its window.
    fn apply(&mut self, staged: &StagedCopy) -> Result<(), TestCaseError> {
        match staged {
            StagedCopy::Produce(op) => {
                let Some(coll) = data_coll(&op.channel) else {
                    return Ok(());
                };
                match &op.outcome {
                    ProduceOutcome::Store => {
                        self.view.recs[coll].push(rec_of(&op.channel, &op.data, op.persist));
                    }
                    ProduceOutcome::Fire {
                        continuation, data, ..
                    } => {
                        let incoming = rec_of(&op.channel, &op.data, op.persist);
                        let x = In {
                            coll,
                            ord: incoming.ord,
                            level: incoming.level,
                            value: incoming.value,
                            // Added by decision 103 (decision 118, rule CR-9): the persist flag
                            // of the element in flight.
                            persist: op.persist,
                        };
                        let stored = self
                            .view
                            .waits
                            .iter()
                            .position(|wait| wait.hash == continuation.source.hash);
                        if let Some(position) = stored {
                            let wait = self.view.waits[position].clone();
                            let distinct: BTreeSet<usize> = wait.colls.iter().copied().collect();
                            for each in distinct {
                                prop_assert!(
                                    at_head(&self.view, &wait, each),
                                    "{} fires off the head of a collection",
                                    wait.name
                                );
                            }
                            self.check_windows(&wait, data, Some(&x))?;
                            if !continuation.persist {
                                self.view.waits.remove(position);
                            }
                        }
                        self.remove(data);
                    }
                }
            }
            StagedCopy::Consume(op) => {
                if op
                    .channels
                    .iter()
                    .any(|channel| data_coll(channel).is_none())
                {
                    return Ok(());
                }
                let source =
                    Consume::create(&op.channels, &op.patterns, &op.continuation, op.persist);
                let wait = wait_of(
                    &op.channels,
                    &op.patterns,
                    &op.continuation,
                    op.persist,
                    !op.peeks.is_empty(),
                    source.hash,
                );
                match &op.outcome {
                    ConsumeOutcome::Park => {
                        let wait =
                            wait.ok_or_else(|| failure("a parked waiter is not a user waiter"))?;
                        self.view.waits.push(wait);
                    }
                    ConsumeOutcome::Fire { data } => {
                        if let Some(wait) = wait {
                            prop_assert!(
                                reaches_heads(&self.view, &wait),
                                "{} fires off the head of a collection",
                                wait.name
                            );
                            self.check_windows(&wait, data, None)?;
                        }
                        self.remove(data);
                    }
                }
            }
        }
        Ok(())
    }
}

use rspace_plus_plus::rspace::trace::event::Consume;

/// The staged operations of the committed transactions of a record, with the lock keys of their
/// transaction.
fn committed(seen: &HostRecord) -> Vec<(&[Par], &StagedCopy)> {
    seen.staged
        .iter()
        .filter(|(txn, _)| seen.commits.contains(txn))
        .map(|(txn, staged)| (seen.begins[*txn].as_slice(), staged))
        .collect()
}

/// Runs the program `program` through a recording host and checks each committed transaction
/// with `check`. The check sees the view before the transaction, the staged operations of the
/// transaction with its lock keys, and the discipline.
async fn staged_case(
    program: Program,
    check: impl Fn(&View, &[(&[Par], &StagedCopy)], Kind) -> Result<(), TestCaseError>,
) -> Result<(), TestCaseError> {
    let (rig, record) = Rig::recording(program.kind).await;
    for (index, step) in program.steps.iter().enumerate() {
        let mut next = Some(space_op(step, index));
        let mut jobs = 0;
        while let Some(op) = next.take() {
            jobs += 1;
            prop_assert!(jobs <= MAX_CHAIN, "a chain runs past {} jobs", MAX_CHAIN);
            let before = read_view(&rig.host, program.kind).await;
            *record.lock().expect("the record lock") = HostRecord::default();
            rig.log().await;
            let result = rig.run(op).await;
            let seen = std::mem::take(&mut *record.lock().expect("the record lock"));
            let outcome = match result {
                Ok(outcome) => outcome,
                Err(error) => {
                    expected_refusal(program.kind, &error)?;
                    // Changed by decision 92: a failure at a position of the state commits the
                    // anchor of its job.
                    // prop_assert!(seen.commits.is_empty(), "a refused job commits");
                    prop_assert!(
                        support::agent::commits_anchor_only(&seen, SPACE),
                        "a refused job commits more than its anchor"
                    );
                    break;
                }
            };
            prop_assert_eq!(seen.commits.len(), 1, "a job commits one transaction");
            prop_assert!(
                seen.begins.len() <= 2,
                "a job escalates at most once: {} begins",
                seen.begins.len()
            );
            check(&before, &committed(&seen), program.kind)?;
            next = outcome.reissue.map(|reissue| reissue.op);
        }
    }
    Ok(())
}

/// `join_fires_heads_only` (`JoinCascade.v`, headline 4) and `JoinFiresHeadsOnly`
/// (`JoinCascade.tla`). A fire of a user waiter fires a waiter that is the head of each head-only
/// collection of its group at that moment. It is the head in each of its lanes. Every record that
/// the fire binds lies in the window of its collection. The model replays the staged operations of
/// each transaction on the view before it. So each fire meets the staged view of its moment.
#[test]
fn prop_join_fires_heads_only() {
    check(pure_config(), arb_maybe_heavy_program(), |program| {
        tokio_runtime().block_on(staged_case(program, |before, staged, _| {
            let mut shadow = Shadow {
                view: before.clone(),
            };
            for (_, op) in staged {
                shadow.apply(op)?;
            }
            Ok(())
        }))
    });
}

// Changed by decision 103 (decision 118, rule CR-9, decision-c6 §6 item 4): a persistent element
// in flight claims nothing, so it can bind two or more positions of its collection.
// `prop_join_incoming_held` below replaces this property. The property before:
// /// `join_incoming_binds_once` (`JoinCascade.v`, headline 3) and `IncomingBoundOnce`
// /// (`JoinCascade.tla`). Every produce-side fire binds its element in flight at exactly one
// /// position of the group, on the collection of the produce (decision 28).
// #[test]
// fn prop_join_incoming_binds_once() {
//     check(pure_config(), arb_maybe_heavy_program(), |program| {
//         tokio_runtime().block_on(staged_case(program, |_, staged, _| {
//             for (_, op) in staged {
//                 let StagedCopy::Produce(op) = op else {
//                     continue;
//                 };
//                 let ProduceOutcome::Fire { channels, data, .. } = &op.outcome else {
//                     continue;
//                 };
//                 let incoming: Vec<usize> = data
//                     .iter()
//                     .enumerate()
//                     .filter(|(_, candidate)| candidate.datum_index == -1)
//                     .map(|(index, _)| index)
//                     .collect();
//                 prop_assert_eq!(incoming.len(), 1, "the element in flight binds once");
//                 let at = incoming[0];
//                 prop_assert_eq!(&channels[at], &op.channel);
//                 prop_assert_eq!(&data[at].removed_datum, &op.data);
//             }
//             Ok(())
//         }))
//     });
// }

/// Added by decision 103 (decision 118, rule CR-9, decision-c6 §6 item 4): `join_incoming_held`
/// (`JoinCascade.v`, headline 18) and `IncomingHeld` (`JoinCascade.tla`). Every produce-side fire
/// binds its element in flight at one position of the group, or at two or more positions only
/// for a persistent element, which claims nothing (rule (a)). Each of these positions lies on the
/// collection of the produce and holds the data of the produce.
#[test]
fn prop_join_incoming_held() {
    check(pure_config(), arb_maybe_heavy_program(), |program| {
        tokio_runtime().block_on(staged_case(program, |_, staged, _| {
            for (_, op) in staged {
                let StagedCopy::Produce(op) = op else {
                    continue;
                };
                let ProduceOutcome::Fire { channels, data, .. } = &op.outcome else {
                    continue;
                };
                let incoming: Vec<usize> = data
                    .iter()
                    .enumerate()
                    .filter(|(_, candidate)| candidate.datum_index == -1)
                    .map(|(index, _)| index)
                    .collect();
                match (incoming.len(), op.persist) {
                    (1, _) => {}
                    (held, true) if held >= 2 => {}
                    (held, persist) => {
                        return Err(failure(format!(
                            "the element in flight binds {held} positions, and its persist flag \
                             is {persist}"
                        )));
                    }
                }
                for at in incoming {
                    prop_assert_eq!(&channels[at], &op.channel);
                    prop_assert_eq!(&data[at].removed_datum, &op.data);
                }
            }
            Ok(())
        }))
    });
}

// Changed by decision 103 (decision 118, rule CR-9, decision-c6 §6 item 4): a persistent record
// claims nothing, so it can bind two or more positions, and on a set two members of a class with
// a persistent member can bind one fire. `prop_join_nonpersistent_record_binds_once` below
// replaces this property. The property before:
// /// `join_repeated_channel_distinct_records` (`JoinCascade.v`, headline 5) and
// /// `DistinctRecordsPerBind` (`JoinCascade.tla`). The binds of one join fire bind pairwise distinct
// /// records, persistent records included (decision 28). On a set, they bind pairwise distinct
// /// classes of each collection. The single-bind class fire `[D; n]` of a set binds one class, so
// /// it is outside the statement.
// #[test]
// fn prop_join_repeated_channel_distinct_records() {
//     let strategy = arb_program(&Kind::ALL, 1..12, 2, 3, true);
//     check(pure_config(), strategy, |program| {
//         tokio_runtime().block_on(staged_case(program, |_, staged, kind| {
//             for (_, op) in staged {
//                 let (channels, continuation, data) = match op {
//                     StagedCopy::Produce(op) => match &op.outcome {
//                         ProduceOutcome::Fire {
//                             channels,
//                             continuation,
//                             data,
//                         } => (channels, &continuation.continuation, data),
//                         ProduceOutcome::Store => continue,
//                     },
//                     StagedCopy::Consume(op) => match &op.outcome {
//                         ConsumeOutcome::Fire { data } => (&op.channels, &op.continuation, data),
//                         ConsumeOutcome::Park => continue,
//                     },
//                 };
//                 if channels.len() < 2 || record::unwrap_join_continuation(continuation).is_err() {
//                     continue;
//                 }
//                 let identities: BTreeSet<(usize, Blake2b256Hash)> = data
//                     .iter()
//                     .map(|candidate| {
//                         (
//                             data_coll(&candidate.channel).expect("a data channel"),
//                             candidate.datum.source.hash.clone(),
//                         )
//                     })
//                     .collect();
//                 prop_assert_eq!(
//                     identities.len(),
//                     data.len(),
//                     "a join binds one record twice"
//                 );
//                 if kind == Kind::Set {
//                     let classes: BTreeSet<(usize, i64)> = data
//                         .iter()
//                         .map(|candidate| {
//                             let rec = rec_of(
//                                 &candidate.channel,
//                                 &candidate.removed_datum,
//                                 candidate.datum.persist,
//                             );
//                             (
//                                 data_coll(&candidate.channel).expect("a data channel"),
//                                 rec.value,
//                             )
//                         })
//                         .collect();
//                     prop_assert_eq!(
//                         classes.len(),
//                         data.len(),
//                         "a set join binds one class twice"
//                     );
//                 }
//             }
//             Ok(())
//         }))
//     });
// }

/// Added by decision 103 (decision 118, rule CR-9, decision-c6 §6 item 4): checks
/// `join_nonpersistent_bound_once` for the candidates `data` of one join fire over the view `view`
/// before the fire, on the discipline `kind`. `x` is the collection and the record of the element
/// in flight of a produce-side fire. No two candidates bind one non-persistent record. On a set,
/// two candidates of one class need a persistent member of the class in `view`, or a persistent
/// `x` of the class.
fn check_bound_once(
    view: &View,
    kind: Kind,
    data: &[rspace_plus_plus::rspace::internal::ConsumeCandidate<Par, ListParWithRandom>],
    x: Option<&(usize, Rec)>,
) -> Result<(), TestCaseError> {
    let mut identities: BTreeSet<(usize, Blake2b256Hash)> = BTreeSet::new();
    for candidate in data.iter().filter(|candidate| !candidate.datum.persist) {
        let coll = data_coll(&candidate.channel).expect("a data channel");
        prop_assert!(
            identities.insert((coll, candidate.datum.source.hash.clone())),
            "a join binds one non-persistent record twice"
        );
    }
    if kind == Kind::Set {
        let mut classes: BTreeMap<(usize, i64), usize> = BTreeMap::new();
        for candidate in data {
            let rec = rec_of(
                &candidate.channel,
                &candidate.removed_datum,
                candidate.datum.persist,
            );
            let coll = data_coll(&candidate.channel).expect("a data channel");
            *classes.entry((coll, rec.value)).or_insert(0) += 1;
        }
        for ((coll, value), bound) in classes {
            if bound < 2 {
                continue;
            }
            let stored = view.recs[coll]
                .iter()
                .any(|rec| rec.persist && rec.value == value);
            let flight =
                x.is_some_and(|(x_coll, rec)| rec.persist && *x_coll == coll && rec.value == value);
            prop_assert!(
                stored || flight,
                "a set join binds two members of the class {} of the collection {}, and the \
                 class has no persistent member",
                value,
                coll
            );
        }
    }
    Ok(())
}

/// Added by decision 103 (decision 118, rule CR-9, decision-c6 §6 item 4):
/// `join_nonpersistent_bound_once` (`JoinCascade.v`, headline 19) and `NonPersistentBoundOnce`
/// (`JoinCascade.tla`). The binds of one join fire never bind one non-persistent record twice. A
/// persistent record claims nothing, so it can bind two or more positions (rule (a)). On a set,
/// two binds of one fire bind members of one class only when the class has a persistent member
/// in the view before the fire, the element in flight included. The model replays the staged
/// operations of each transaction on the view before it ([`Shadow`]), so each fire meets the
/// staged view of its moment. The single-bind class fire `[D; n]` of a set binds one class, so it
/// is outside the statement.
#[test]
fn prop_join_nonpersistent_record_binds_once() {
    let strategy = arb_program(&Kind::ALL, 1..12, 2, 3, true);
    check(pure_config(), strategy, |program| {
        tokio_runtime().block_on(staged_case(program, |before, staged, kind| {
            let mut shadow = Shadow {
                view: before.clone(),
            };
            for (_, op) in staged {
                let fire = match op {
                    StagedCopy::Produce(produce) => match &produce.outcome {
                        ProduceOutcome::Fire {
                            channels,
                            continuation,
                            data,
                        } => {
                            let x = data_coll(&produce.channel).map(|coll| {
                                (
                                    coll,
                                    rec_of(&produce.channel, &produce.data, produce.persist),
                                )
                            });
                            Some((channels, &continuation.continuation, data, x))
                        }
                        ProduceOutcome::Store => None,
                    },
                    StagedCopy::Consume(consume) => match &consume.outcome {
                        ConsumeOutcome::Fire { data } => {
                            Some((&consume.channels, &consume.continuation, data, None))
                        }
                        ConsumeOutcome::Park => None,
                    },
                };
                match fire {
                    Some((channels, continuation, data, x))
                        if channels.len() >= 2
                            && record::unwrap_join_continuation(continuation).is_ok() =>
                    {
                        check_bound_once(&shadow.view, kind, data, x.as_ref())?;
                    }
                    Some(_) | None => {}
                }
                shadow.apply(op)?;
            }
            Ok(())
        }))
    });
}

/// `cascade_bumps_touched_counters` (`JoinCascade.v`, headline 12) and `TouchedCountersBumped`
/// (`JoinCascade.tla`). Take a transaction of a discipline with counters. Each collection that the
/// transaction stages on has its counter bumped exactly once, before the first staging on the
/// collection (decision 31). A bump takes and puts the counter of the collection. The first bump
/// of a new collection takes and puts the space counter `S(sid)` and puts the new counter.
#[test]
fn prop_cascade_bumps_touched_counters() {
    check(pure_config(), arb_maybe_heavy_program(), |program| {
        tokio_runtime().block_on(staged_case(program, |_, staged, kind| {
            if !kind.counted() {
                return Ok(());
            }
            let space_counter = support::agent::space_counter_channel(SPACE);
            let mut takes = [0usize; COLLS];
            let mut puts = [0usize; COLLS];
            let mut after_space_put = false;
            for (_, op) in staged {
                let first = op.channels().first().cloned();
                let counter = first.as_ref().and_then(counter_coll);
                let space = first.as_ref() == Some(&space_counter);
                match (counter, op, space) {
                    (Some(coll), StagedCopy::Consume(_), _) => takes[coll] += 1,
                    (Some(coll), StagedCopy::Produce(_), _) => {
                        prop_assert!(
                            takes[coll] == 1 || after_space_put,
                            "the counter of {} is put without a take outside the opening of a collection",
                            coll
                        );
                        puts[coll] += 1;
                    }
                    (None, _, true) => {}
                    (None, _, false) => {
                        for coll in staged_colls(op) {
                            prop_assert!(
                                puts[coll] == 1,
                                "the collection {} is staged on before its bump",
                                coll
                            );
                        }
                    }
                }
                after_space_put = space && matches!(op, StagedCopy::Produce(_));
            }
            for coll in 0..COLLS {
                prop_assert!(
                    puts[coll] <= 1 && takes[coll] <= puts[coll],
                    "the counter of {} bumps {} times",
                    coll,
                    puts[coll]
                );
            }
            Ok(())
        }))
    });
}

/// `plan_then_lock_stable` (`JoinCascade.v`, headline 14). Every channel that a committed
/// transaction stages on lies in the lock keys of that transaction. A job escalates at most once
/// ("scope, then lock, then plan", joins plan 4.1). The serial schedule of the in-memory rig
/// gives the footprint premise of the theorem.
#[test]
fn prop_plan_then_lock_covers_every_staged_channel() {
    check(pure_config(), arb_maybe_heavy_program(), |program| {
        tokio_runtime().block_on(staged_case(program, |_, staged, _| {
            for (locks, op) in staged {
                for channel in op.channels() {
                    prop_assert!(
                        locks.contains(&channel),
                        "a staged channel is outside the lock keys"
                    );
                }
            }
            Ok(())
        }))
    });
}

// ─── Decision 94 and the TLA invariants of JoinScopeLock ─────────────────────────────────────

// Changed by decision 111: the runtimes also hold the current-thread runtime, as the base runs
// its racing tests (`random_candidate_order.rs`, P-1 and P-6 of DR-132).
/// The worker threads of the runtimes of the racing property, where 0 is the current-thread
/// runtime (decisions 93 and 111). No case needs the current-thread runtime to pass.
const RACING_WORKERS: [usize; 3] = [0, 2, 8];

/// The most steps of a case that race.
const RACING_STEPS: usize = 4;

/// The time limit of the racing jobs of one case. Jobs that do not end within it deadlock.
const RACING_LIMIT: std::time::Duration = std::time::Duration::from_secs(30);

/// The host channel `h{index}` of the racing property. It is not a storage channel of a space.
fn host_channel(index: usize) -> Par { string(&format!("rrs-join-prop-host-{index}")) }

/// The datum of the host produce of the racing property.
fn host_datum() -> ListParWithRandom {
    ListParWithRandom {
        pars: vec![int(7)],
        random_state: Blake2b512Random::create_from_bytes(b"rrs join prop host produce").to_bytes(),
        cost_authority: None,
        cost_stack: None,
    }
}

/// The step `step` without its peek.
fn without_peek(step: Step) -> Step {
    match step {
        Step::Receive {
            bind, persistent, ..
        } => Step::Receive {
            bind,
            peek: false,
            persistent,
        },
        Step::Join {
            binds,
            persistent,
            guard,
            ..
        } => Step::Join {
            binds,
            peek: false,
            persistent,
            guard,
        },
        send @ Step::Send { .. } => send,
    }
}

/// A program of the racing property and the number of its setup steps. After the setup, 2 to
/// `RACING_STEPS` steps race. The program has no persistence and no peek, so each step is one
/// job: no fire leaves both of its sides in the store, and a program has fewer fires than
/// `FIRE_ROUND_CAP`.
fn arb_racing_program() -> impl Strategy<Value = (Program, usize)> {
    let kinds: &'static [Kind] = &Kind::ALL;
    prop::sample::select(kinds).prop_flat_map(|kind| {
        let step =
            move || arb_step(kind, Persistence::Neither, COLLS, 4, true).prop_map(without_peek);
        (
            prop::collection::vec(step(), 0..6),
            prop::collection::vec(step(), 2..=RACING_STEPS),
        )
            .prop_map(move |(setup, racing)| {
                let split = setup.len();
                let mut steps = Vec::with_capacity(split + racing.len());
                steps.extend(setup);
                steps.extend(racing);
                (Program { kind, steps }, split)
            })
    })
}

/// The collections of the step `step`.
fn step_colls(step: &Step) -> BTreeSet<usize> {
    match step {
        Step::Send { coll, .. } => BTreeSet::from([*coll]),
        Step::Receive { bind, .. } => BTreeSet::from([bind.coll]),
        Step::Join { binds, .. } => binds.iter().map(|bind| bind.coll).collect(),
    }
}

/// True when a join of `steps` shares a collection with another step of `steps`. Such a join
/// can grow or shrink the join scope of the other step.
fn contested(steps: &[Step]) -> bool {
    steps.iter().enumerate().any(|(at, step)| {
        matches!(step, Step::Join { .. })
            && steps.iter().enumerate().any(|(other, peer)| {
                other != at && !step_colls(step).is_disjoint(&step_colls(peer))
            })
    })
}

// Retired by decision 90 (C6, step D): the serial-order check, the only caller of `orders`, ran
// through the rigged replay modes, which retire (decision 125).
// `space_racing_replay_agrees_with_play`, `space_racing_replay_completes_every_row`, the TLA
// `MutualExclusion` of `JoinScopeLock.tla`.

/// The end of the racing steps of a case: the result of each racing job in step order, true
/// when the host produce fired, and the root of the play space.
#[derive(Debug, PartialEq)]
struct RaceEnd {
    results: Vec<Result<SpaceOutcome, SpaceError>>,
    host_fired: bool,
    root: Blake2b256Hash,
}

// Changed by decision 102 (pick tests, R1): the rig works over the play half of a play and replay
// pair, and the setup also returns the replay half, so the case can replay the race log.
/// The rig of a racing case over a fresh play space. The rig holds the space of `program`, its
/// steps before `split` one after another, each as one chain, and a parked host join over h1
/// and h2.
async fn racing_setup(program: &Program, split: usize) -> Result<Rig, TestCaseError> {
    let play_space = support::agent::play().await;
    let rig = Rig::over(play_space, program.kind).await;
    for (index, step) in program.steps[..split].iter().enumerate() {
        for job in chain(&rig, space_op(step, index)).await? {
            if let Err(error) = &job.result {
                expected_refusal(program.kind, error)?;
            }
        }
    }
    let parked = rig
        .host
        .consume(
            vec![host_channel(1), host_channel(2)],
            vec![support::agent::any(), support::agent::any()],
            support::agent::named("rrs join prop host join", None),
            false,
            BTreeSet::new(),
        )
        .await
        .expect("the host consume runs");
    prop_assert!(parked.is_none(), "the host join parks");
    Ok(rig)
}

// Changed by decision 102 (pick tests, R1): the race also returns its log, so the case can replay
// it.
/// Runs the racing steps of `program` after `split` as concurrent jobs, with the host produce on
/// h1. The jobs and the produce start behind one barrier. The race fails when they do not end
/// within `RACING_LIMIT`. It returns the end of the race and the race log. It takes the log
/// before the checkpoint at the end, because the checkpoint takes the log.
async fn race(
    rig: &Rig,
    program: &Program,
    split: usize,
) -> Result<(RaceEnd, Vec<Event>), TestCaseError> {
    let racing = &program.steps[split..];
    let barrier = Arc::new(tokio::sync::Barrier::new(racing.len() + 1));
    let jobs: Vec<_> = racing
        .iter()
        .enumerate()
        .map(|(offset, step)| {
            let env = rig.env.clone();
            let barrier = Arc::clone(&barrier);
            let op = space_op(step, split + offset);
            tokio::spawn(async move {
                barrier.wait().await;
                run(&FamilyAgent, &env, op).await
            })
        })
        .collect();
    let host = rig.host.clone();
    let host_job = tokio::spawn(async move {
        barrier.wait().await;
        host.produce(host_channel(1), host_datum(), false).await
    });
    let ended = tokio::time::timeout(RACING_LIMIT, async move {
        let mut results = Vec::with_capacity(jobs.len());
        for job in jobs {
            results.push(job.await.expect("a racing job runs to its end"));
        }
        let fired = host_job
            .await
            .expect("the host produce runs to its end")
            .expect("the host produce succeeds");
        (results, fired.is_some())
    })
    .await;
    let Ok((results, host_fired)) = ended else {
        return Err(failure(format!(
            "the racing jobs do not end within {RACING_LIMIT:?}"
        )));
    };
    // Changed by decision 102 (pick tests, R1): the race takes its log before the checkpoint,
    // which takes the log.
    let log = rig.log().await;
    Ok((
        RaceEnd {
            results,
            host_fired,
            root: rig.root().await,
        },
        log,
    ))
}

// Changed by decision 102 (pick tests, R1): a fresh rig plays the setup and each serial order with
// random picks (rule CR-1), so a serial order can end in another state than the race only
// because of a pick. `replayed_serial_end` replays the race log instead.

// Retired by decision 90 (C6, step D): `replayed_serial_end` ran through the rigged replay modes,
// which retire (decision 125). `space_racing_replay_agrees_with_play`,
// `space_racing_replay_completes_every_row`, the TLA `MutualExclusion` of `JoinScopeLock.tla`.

/// One run of a racing case.
async fn racing_case(program: &Program, split: usize) -> Result<(), TestCaseError> {
    // Changed by decision 102 (pick tests, R1): the race starts from one checkpoint after the
    // setup, and it also returns its log.
    let rig = racing_setup(program, split).await?;
    let (raced, _log) = race(&rig, program, split).await?;
    for (offset, result) in raced.results.iter().enumerate() {
        let index = split + offset;
        match result {
            Ok(outcome) => {
                prop_assert!(
                    outcome.reissue.is_none(),
                    "the racing job of step {} re-issues: {:?}",
                    index,
                    outcome.reissue
                );
            }
            Err(error) => {
                expected_refusal(program.kind, error).map_err(|refusal| {
                    failure(format!("the racing job of step {index}: {refusal}"))
                })?;
            }
        }
    }
    prop_assert!(
        !raced.host_fired,
        "the host produce on h1 fires the join of h1 and h2 without h2"
    );
    // Retired by decision 90 (C6, step D): the replay half ran through the rigged replay modes,
    // which retire (decision 125). `space_racing_replay_agrees_with_play`,
    // `space_racing_replay_completes_every_row`, the TLA `MutualExclusion` of `JoinScopeLock.tla`.
    Ok(())
}

/// `NoStaleScope`, `NoFalseRefusal` and `NoDeadlock` (`JoinScopeLock.tla`,
/// decision 94). Since DR-132, the jobs of one space race, and a job reads its join scope from
/// the state of the space. Each case runs its setup steps one after another. Then its last two to
/// four steps race as concurrent jobs, with a host produce on h1 whose join group is {h1, h2}.
/// The jobs start behind one barrier, on each runtime of `RACING_WORKERS` (decision 93).
///
/// - `NoDeadlock`: every job ends within `RACING_LIMIT`.
/// - `NoStaleScope` and `NoFalseRefusal`: every racing job commits, or a cell refuses it with
///   `CellOccupied`. A stale scope fails a job with `Inconsistent`, and a channel outside the
///   lock keys fails it with `UnlockedChannel`.
// Retired by decision 90 (C6, step D): the check of `MutualExclusion` replayed the race log over
// the rigged replay modes, which retire (decision 125). The TLA invariant `MutualExclusion` of
// `JoinScopeLock.tla` and the native racing properties (`space_racing_replay_agrees_with_play`,
// `space_racing_replay_completes_every_row`) keep the claim.
///
/// The stripes of the channels come from the hashes in the code, so the property checks one
/// stripe map. The model checks every map. Non-vacuity: some case races a join with a job that
/// shares a collection.
#[test]
fn prop_racing_jobs_equal_a_serial_order() {
    let contested_cases = AtomicUsize::new(0);
    check(pure_config(), arb_racing_program(), |(program, split)| {
        if contested(&program.steps[split..]) {
            contested_cases.fetch_add(1, Ordering::Relaxed);
        }
        for workers in RACING_WORKERS {
            // Changed by decision 111: 0 workers give the current-thread runtime, as in the
            // base.
            let runtime = match workers {
                0 => tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("a current-thread racing runtime"),
                _ => tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(workers)
                    .enable_all()
                    .build()
                    .expect("a racing runtime"),
            };
            runtime.block_on(racing_case(&program, split))?;
        }
        Ok(())
    });
    assert!(
        contested_cases.load(Ordering::Relaxed) > 0,
        "no case races a join with a job that shares a collection"
    );
}

// ─── Headline 6 ──────────────────────────────────────────────────────────────────────────────

/// A record that a fire binds, as the model sees it: its collection, its value, its identity
/// and its persist flag.
#[derive(Clone, Debug, PartialEq, Eq)]
struct BoundRec {
    coll: usize,
    value: i64,
    hash: Blake2b256Hash,
    persist: bool,
}

/// A fire of a user waiter as a staged operation shows it.
struct UserFire {
    /// True when the waiter peeks.
    peek: bool,
    /// True for a waiter with one bind: its continuation is a record of version 1.
    single: bool,
    /// True for a consume-side fire, false for a produce-side fire.
    consume_side: bool,
    /// The bound records, in group order.
    bound: Vec<BoundRec>,
}

/// The fire of a user waiter that the staged operation `staged` makes. Other operations, the
/// fires of `K_lift`, `K_retire` and the counter takes, give `None`.
fn user_fire(staged: &StagedCopy) -> Option<UserFire> {
    let (continuation, peek, data, consume_side) = match staged {
        StagedCopy::Produce(op) => match &op.outcome {
            ProduceOutcome::Fire {
                continuation, data, ..
            } => (
                &continuation.continuation,
                !continuation.peeks.is_empty(),
                data,
                false,
            ),
            ProduceOutcome::Store => return None,
        },
        StagedCopy::Consume(op) => match &op.outcome {
            ConsumeOutcome::Fire { data } => (&op.continuation, !op.peeks.is_empty(), data, true),
            ConsumeOutcome::Park => return None,
        },
    };
    let (inner, single) = match record::unwrap_join_continuation(continuation) {
        Ok(parked) => (parked.continuation, false),
        Err(_) => (
            record::unwrap_pq_continuation(continuation)
                .ok()?
                .continuation,
            true,
        ),
    };
    step_of(&body_name(&inner))?;
    let bound = data
        .iter()
        .map(|candidate| {
            let rec = rec_of(
                &candidate.channel,
                &candidate.removed_datum,
                candidate.datum.persist,
            );
            BoundRec {
                coll: data_coll(&candidate.channel).expect("a data channel"),
                value: rec.value,
                hash: candidate.datum.source.hash.clone(),
                persist: candidate.datum.persist,
            }
        })
        .collect();
    Some(UserFire {
        peek,
        single,
        consume_side,
        bound,
    })
}

/// The record that the staged operation `staged` removes, with its collection. The operation
/// must be a single-channel consume of `K_retire` or `K_lift`, not of a user waiter.
fn system_removal(staged: &StagedCopy) -> Option<(usize, Blake2b256Hash)> {
    let StagedCopy::Consume(op) = staged else {
        return None;
    };
    let ConsumeOutcome::Fire { data } = &op.outcome else {
        return None;
    };
    let [candidate] = data.as_slice() else {
        return None;
    };
    let [channel] = op.channels.as_slice() else {
        return None;
    };
    let coll = data_coll(channel)?;
    match record::unwrap_pq_continuation(&op.continuation) {
        Ok(_) => None,
        Err(_) => Some((coll, candidate.datum.source.hash.clone())),
    }
}

/// `join_set_bind_retires_class` (`JoinCascade.v`, headline 6). Take a join fire that does not
/// peek. Right after it, the other stored members of each bound set class leave with
/// single-channel `K_retire` COMMs, unless they are persistent. The retires go class after class in group order,
/// and each class in the canonical order of DR-75. Then no such member remains. A persistent
/// member keeps its class. After a peek, nothing is retired.
///
/// The fires of single-bind waiters meet the same statement. Changed by decision 101 (rule
/// CR-13) and decision 102 (decision 119 G1, rule CR-12): a consume-side single-bind fire that
/// does not peek binds one record over `[D]`, the first member of the class of its pick in the
/// canonical order of DR-75. The other members that are not persistent then leave with
/// `K_retire` COMMs, as after a join fire, and a persistent member of the class stays. The text
/// before: "The fires of single-bind waiters meet the same statement, with the alignment note
/// A2. A consume-side single-bind fire that does not peek binds the whole class with the class
/// fire `[D; n]`. So no retire follows it, and a persistent member of the class stays."
#[test]
fn prop_join_set_bind_retires_class() {
    check(
        pure_config(),
        arb_program(&[Kind::Set], 1..14, 2, 3, true),
        |program| {
            tokio_runtime().block_on(staged_case(program, |before, staged, _| {
                let mut shadow = Shadow {
                    view: before.clone(),
                };
                let ops: Vec<&StagedCopy> = staged.iter().map(|(_, op)| *op).collect();
                let mut index = 0;
                while index < ops.len() {
                    let op = ops[index];
                    index += 1;
                    let Some(UserFire {
                        peek,
                        single,
                        consume_side,
                        bound,
                    }) = user_fire(op)
                    else {
                        shadow.apply(op)?;
                        continue;
                    };
                    let bound_hashes: BTreeSet<Blake2b256Hash> =
                        bound.iter().map(|rec| rec.hash.clone()).collect();
                    // Changed by decision 101 (rule CR-13) and decision 102 (decision 119 G1,
                    // rule CR-12): the fire binds the first member of its class over `[D]`, and
                    // the checks below expect the `K_retire` COMMs of the other members.
                    // if single && consume_side && !peek {
                    //     let (coll, value) = (bound[0].coll, bound[0].value);
                    //     let class: BTreeSet<Blake2b256Hash> = shadow.view.recs[coll]
                    //         .iter()
                    //         .filter(|rec| rec.value == value)
                    //         .map(|rec| rec.hash.clone())
                    //         .collect();
                    //     prop_assert_eq!(
                    //         &bound_hashes,
                    //         &class,
                    //         "a single-bind fire binds the whole class (A2)"
                    //     );
                    // }
                    if single && consume_side && !peek {
                        let [first] = bound.as_slice() else {
                            return Err(failure("a single-bind fire binds one record (CR-13)"));
                        };
                        let dispatched = shadow.view.recs[first.coll]
                            .iter()
                            .filter(|rec| rec.value == first.value)
                            .map(|rec| &rec.hash)
                            .min();
                        prop_assert_eq!(
                            dispatched,
                            Some(&first.hash),
                            "the fire binds the DR-75-first member of its class (G1)"
                        );
                    }
                    let kept: Vec<Blake2b256Hash> = bound
                        .iter()
                        .filter(|rec| {
                            rec.persist
                                && shadow.view.recs[rec.coll]
                                    .iter()
                                    .any(|stored| stored.hash == rec.hash)
                        })
                        .map(|rec| rec.hash.clone())
                        .collect();
                    let mut classes: Vec<(usize, i64)> = Vec::new();
                    for rec in &bound {
                        if !classes.contains(&(rec.coll, rec.value)) {
                            classes.push((rec.coll, rec.value));
                        }
                    }
                    let mut members: Vec<(usize, Blake2b256Hash)> = Vec::new();
                    for (coll, value) in &classes {
                        let mut class: Vec<&Rec> = shadow.view.recs[*coll]
                            .iter()
                            .filter(|rec| {
                                rec.value == *value
                                    && !rec.persist
                                    && !bound_hashes.contains(&rec.hash)
                            })
                            .collect();
                        class.sort_by(|left, right| left.hash.cmp(&right.hash));
                        members.extend(class.into_iter().map(|rec| (*coll, rec.hash.clone())));
                    }
                    shadow.apply(op)?;
                    if peek {
                        if let Some(next) = ops.get(index) {
                            prop_assert!(
                                !system_removal(next)
                                    .is_some_and(|removed| members.contains(&removed)),
                                "a peek fire retires a member of a bound class"
                            );
                        }
                        continue;
                    }
                    for member in &members {
                        let next = ops
                            .get(index)
                            .ok_or_else(|| failure("a retire is missing"))?;
                        let removed = system_removal(next);
                        prop_assert_eq!(
                            removed.as_ref(),
                            Some(member),
                            "the retires follow the fire, class after class in canonical order"
                        );
                        shadow.apply(next)?;
                        index += 1;
                    }
                    for (coll, value) in &classes {
                        prop_assert!(
                            !shadow.view.recs[*coll]
                                .iter()
                                .any(|rec| rec.value == *value && !rec.persist),
                            "a member of a bound class that is not persistent remains"
                        );
                    }
                    for hash in &kept {
                        prop_assert!(
                            shadow
                                .view
                                .recs
                                .iter()
                                .flatten()
                                .any(|rec| rec.hash == *hash),
                            "a persistent member keeps its class"
                        );
                    }
                }
                Ok(())
            }))
        },
    );
}

// ─── Headlines 10 and 11: owed pairs ─────────────────────────────────────────────────────────

/// Opens the collections `colls` of `rig` with a send and a receive each, so each holds a
/// counter, and leaves them empty.
async fn open_collections(rig: &Rig, colls: &[usize]) -> Result<(), TestCaseError> {
    for coll in colls {
        let level = match rig.kind {
            Kind::Pq => Some(0),
            Kind::Queue | Kind::Stack | Kind::Cell | Kind::Bag | Kind::Set => None,
        };
        rig.run(SpaceOp::Produce(send(*coll, 100, false, level, 9_000)))
            .await
            .map_err(failure)?;
        rig.run(SpaceOp::Consume(receive_on(
            SPACE,
            &ch(*coll),
            &format!("open-{coll}"),
            Pat::Any.pattern(),
        )))
        .await
        .map_err(failure)?;
    }
    rig.log().await;
    Ok(())
}

/// Stores the record `value` on the collection `coll` with a raw host produce, with the order
/// `ord`, so no plan runs.
async fn raw_record(
    rig: &Rig,
    coll: usize,
    value: i64,
    ord: i64,
    salt: u64,
) -> Result<Blake2b256Hash, TestCaseError> {
    let op = send(coll, value, false, None, 50_000 + salt);
    let record_bytes = record::wrap_datum(ord, &op.data);
    let hash = Produce::create(&data(coll), &record_bytes, false).hash;
    let fired = rig
        .host
        .produce(data(coll), record_bytes, false)
        .await
        .map_err(failure)?;
    prop_assert!(fired.is_none(), "a raw produce fires nothing");
    Ok(hash)
}

/// Parks the join of `binds` named `name` with a raw host consume, with the orders `ords` (by
/// bind), so no plan runs. It returns the identity of the waiter.
async fn raw_join(
    rig: &Rig,
    binds: &[Bind],
    name: &str,
    ords: &[i64],
) -> Result<Blake2b256Hash, TestCaseError> {
    let op = join_of(binds, name, false, false, Guard::Open);
    let group = group_of(binds);
    let slots: Vec<JoinSlot> = group
        .iter()
        .map(|(bind, position)| JoinSlot {
            ord: ords[*bind],
            bind: *bind,
            policy: WaiterPolicy::of_level(position.lane),
        })
        .collect();
    let channels: Vec<Par> = group
        .iter()
        .map(|(_, position)| data(position.coll))
        .collect();
    let patterns: Vec<BindPattern> = group
        .iter()
        .map(|(_, position)| position.pat.pattern())
        .collect();
    let body = models::rhoapi::TaggedContinuation {
        tagged_cont: Some(models::rhoapi::tagged_continuation::TaggedCont::ParBody(
            op.body.clone(),
        )),
        guard: None,
        cost_authority: None,
    };
    let parked = record::wrap_join_continuation(&slots, &body).map_err(failure)?;
    let hash = Consume::create(&channels, &patterns, &parked, false).hash;
    let fired = rig
        .host
        .consume(channels, patterns, parked, false, BTreeSet::new())
        .await
        .map_err(failure)?;
    prop_assert!(fired.is_none(), "a raw consume parks");
    Ok(hash)
}

/// The settle job of a cascade over the collection `key_coll` and the seeds `seeds`.
fn settle_job(key_coll: usize, seeds: &[usize], salt: &str) -> SpaceOp {
    SpaceOp::Settle(SettleOp {
        space: SPACE,
        key: key(key_coll),
        channel: ch(key_coll),
        random_state: Blake2b512Random::create_from_bytes(salt.as_bytes()).to_bytes(),
        authority: models::rhoapi::CostAuthority::default(),
        introduction_authority: models::rhoapi::CostAuthority::default(),
        cascade: Some(Box::new(CascadeState {
            seeds: seeds.iter().map(|seed| key(*seed)).collect(),
            pending: Vec::new(),
            resume: None,
        })),
    })
}

/// Builds two owed pairs on four collections of a fresh rig of `kind`. The collection `i` holds
/// the record `values[i]`. The join `va` waits on `[0, 2]`, and the join `vb` waits on `[1, 3]`,
/// with the patterns `pats`. Raw host operations store them, so both pairs are fireable and owed.
async fn owed_pairs(kind: Kind, values: [i64; 4], pats: [Pat; 4]) -> Result<Rig, TestCaseError> {
    let rig = Rig::new(kind).await;
    open_collections(&rig, &[0, 1, 2, 3]).await?;
    for (coll, value) in values.iter().enumerate() {
        raw_record(
            &rig,
            coll,
            *value,
            0,
            u64::try_from(coll).expect("a small index"),
        )
        .await?;
    }
    for (name, first, second) in [("va", 0usize, 2usize), ("vb", 1, 3)] {
        let binds = [
            Bind {
                coll: first,
                pat: pats[first],
                lane: None,
            },
            Bind {
                coll: second,
                pat: pats[second],
                lane: None,
            },
        ];
        raw_join(&rig, &binds, name, &[1, 1]).await?;
    }
    rig.log().await;
    Ok(rig)
}

/// `cascade_order_independent` (`JoinCascade.v`, headline 10): on a single-head discipline, two
/// fireable waiters that do not peek have disjoint collection sets, so their fires commute. One
/// settle job over the four collections and two settle jobs in either order fire both pairs and
/// reach one root.
#[test]
fn prop_cascade_order_independent() {
    let strategy = (
        prop::sample::select(&[Kind::Queue, Kind::Stack][..]),
        prop::array::uniform4(0i64..4),
        prop::array::uniform4(any::<bool>()),
    );
    check(pure_config(), strategy, |(kind, values, exact)| {
        tokio_runtime().block_on(async move {
            let pats: [Pat; 4] = std::array::from_fn(|coll| match exact[coll] {
                true => Pat::Is(values[coll]),
                false => Pat::Any,
            });
            let fired_names = |outcomes: &[SpaceOutcome]| {
                let mut names: Vec<String> = outcomes
                    .iter()
                    .flat_map(|outcome| {
                        outcome
                            .fires
                            .iter()
                            .map(|fire| body_name(&fire.continuation))
                    })
                    .collect();
                names.sort();
                names
            };
            let together = owed_pairs(kind, values, pats).await?;
            let one = together
                .run(settle_job(0, &[1, 2, 3], "rrs-join-prop order"))
                .await
                .map_err(failure)?;
            prop_assert_eq!(fired_names(std::slice::from_ref(&one)), vec![
                "va".to_string(),
                "vb".to_string()
            ]);
            let root = together.root().await;
            for order in [[0usize, 1], [1, 0]] {
                let rig = owed_pairs(kind, values, pats).await?;
                let mut outcomes = Vec::new();
                for first in order {
                    let partner = first + 2;
                    outcomes.push(
                        rig.run(settle_job(first, &[partner], "rrs-join-prop order"))
                            .await
                            .map_err(failure)?,
                    );
                }
                prop_assert_eq!(fired_names(&outcomes), vec![
                    "va".to_string(),
                    "vb".to_string()
                ]);
                prop_assert_eq!(
                    rig.root().await,
                    root.clone(),
                    "the order {:?} reaches the same root",
                    order
                );
            }
            Ok(())
        })
    });
}

/// `lift_reoffer_equals_direct_comm` (`JoinCascade.v`, headline 11). A settle job fires an owed
/// stored pair with lift and re-offer. It removes the records of `GREEDY` that are not
/// persistent, spends the waiter, and gives the results of `GREEDY` in bind order. One direct COMM
/// of the stored pair does the same: the COMM of the waiter binds the stored identities. The
/// other events on the data channels are the `K_lift` consume and its COMM, and the produce of
/// the re-offer. `K_lift` lifts the least position whose record is not persistent. Here each
/// class of a set has one member, so no retire follows the fire (headline 6 covers retires).
#[test]
fn prop_lift_reoffer_equals_direct_comm() {
    let strategy = (
        prop::sample::select(&[Kind::Queue, Kind::Stack, Kind::Set][..]),
        prop::collection::vec(0i64..3, 1..4),
        prop::collection::vec(0i64..3, 1..3),
        any::<bool>(),
        any::<bool>(),
    );
    check(
        pure_config(),
        strategy,
        |(kind, mut on_a, mut on_b, exact_a, exact_b)| {
            if kind == Kind::Set {
                on_a.sort_unstable();
                on_a.dedup();
                on_b.sort_unstable();
                on_b.dedup();
            }
            tokio_runtime().block_on(async move {
            let rig = Rig::new(kind).await;
            open_collections(&rig, &[0, 1]).await?;
            for (index, value) in on_a.iter().enumerate() {
                let ord = i64::try_from(index).expect("a small index");
                raw_record(&rig, 0, *value, ord, u64::try_from(index).expect("a small index")).await?;
            }
            for (index, value) in on_b.iter().enumerate() {
                let ord = i64::try_from(index).expect("a small index");
                raw_record(&rig, 1, 10 + *value, ord, 100 + u64::try_from(index).expect("a small index")).await?;
            }
            let view = read_view(&rig.host, kind).await;
            let head_value = |coll: usize| -> i64 {
                let positions = [Position {
                    coll,
                    pat: Pat::Any,
                    lane: None,
                }];
                // Changed by decision 102 (plan v4 C5.3, rule CR-1): `search` takes pins. The
                // fixed pattern takes the first record of the collection in the canonical order
                // of DR-75, as before, so the search takes none. The line before:
                // let chosen = search(&view, &positions, None).expect("a collection holds a record");
                let chosen =
                    search(&view, &positions, None, &[]).expect("a collection holds a record");
                value_at(&view, coll, chosen[0], None)
            };
            let binds = [
                Bind {
                    coll: 0,
                    pat: match exact_a {
                        true => Pat::Is(head_value(0)),
                        false => Pat::Any,
                    },
                    lane: None,
                },
                Bind {
                    coll: 1,
                    pat: match exact_b {
                        true => Pat::Is(head_value(1)),
                        false => Pat::Any,
                    },
                    lane: None,
                },
            ];
            let waiter = raw_join(&rig, &binds, "w", &[1_000, 1_000]).await?;
            rig.log().await;
            let view = read_view(&rig.host, kind).await;
            let wait = view
                .waits
                .iter()
                .find(|wait| wait.hash == waiter)
                .ok_or_else(|| failure("the raw waiter is in the view"))?
                .clone();
            prop_assert!(fireable(&view, &wait), "the stored pair is owed");
            let positions = positions_of(&wait);
            // Changed by decision 102 (plan v4 C5.3, rule CR-1): `search` takes pins. The settle
            // cascade searches in the canonical order of DR-75 (rule F3), so the search takes
            // none. The line before:
            // let chosen = search(&view, &positions, None).expect("a fireable waiter has a choice");
            let chosen =
                search(&view, &positions, None, &[]).expect("a fireable waiter has a choice");
            let group_hashes: Vec<Blake2b256Hash> = positions
                .iter()
                .zip(&chosen)
                .map(|(position, at)| match at {
                    At::Stored(index) => view.recs[position.coll][*index].hash.clone(),
                    At::Incoming => unreachable!("a stored pair holds no element in flight"),
                })
                .collect();
            let outcome = rig
                .run(settle_job(0, &[1], "rrs-join-prop lift"))
                .await
                .map_err(failure)?;
            prop_assert_eq!(outcome.fires.len(), 1);
            let mut by_bind: Vec<(usize, Blake2b256Hash)> = wait
                .slots
                .iter()
                .zip(&group_hashes)
                .map(|(slot, hash)| (slot.bind, hash.clone()))
                .collect();
            by_bind.sort();
            prop_assert_eq!(
                result_hashes(&outcome.fires[0]),
                by_bind.into_iter().map(|(_, hash)| hash).collect::<Vec<_>>(),
                "the results of GREEDY in bind order"
            );
            let data_events: Vec<Event> = rig
                .log()
                .await
                .into_iter()
                .filter(|event| match event {
                    Event::IoEvent(rspace_plus_plus::rspace::trace::event::IOEvent::Produce(produce)) => {
                        (0..2).any(|coll| produce.channel_hash == rspace_plus_plus::rspace::hashing::stable_hash_provider::hash(&data(coll)))
                    }
                    Event::IoEvent(rspace_plus_plus::rspace::trace::event::IOEvent::Consume(consume)) => consume
                        .channel_hashes
                        .iter()
                        .all(|hash| (0..2).any(|coll| *hash == rspace_plus_plus::rspace::hashing::stable_hash_provider::hash(&data(coll)))),
                    Event::Comm(comm) => comm
                        .consume
                        .channel_hashes
                        .iter()
                        .all(|hash| (0..2).any(|coll| *hash == rspace_plus_plus::rspace::hashing::stable_hash_provider::hash(&data(coll)))),
                })
                .collect();
            let lifted = group_hashes[0].clone();
            match data_events.as_slice() {
                [Event::IoEvent(rspace_plus_plus::rspace::trace::event::IOEvent::Consume(lift)), Event::Comm(lift_comm), Event::IoEvent(rspace_plus_plus::rspace::trace::event::IOEvent::Produce(reoffer)), Event::Comm(comm)] =>
                {
                    prop_assert_eq!(lift.channel_hashes.len(), 1, "K_lift is a single-channel consume");
                    prop_assert_eq!(&lift_comm.consume, lift);
                    prop_assert_eq!(
                        lift_comm.produces.iter().map(|produce| produce.hash.clone()).collect::<Vec<_>>(),
                        vec![lifted.clone()],
                        "K_lift lifts the least position whose record is not persistent"
                    );
                    prop_assert_eq!(&reoffer.hash, &lifted, "the re-offer keeps the identity");
                    prop_assert_eq!(&comm.consume.hash, &waiter);
                    prop_assert_eq!(
                        comm.produces.iter().map(|produce| produce.hash.clone()).collect::<Vec<_>>(),
                        group_hashes.clone(),
                        "the COMM of the waiter binds the stored identities, as a direct COMM"
                    );
                }
                other => return Err(failure(format!("the events of lift and re-offer: {other:?}"))),
            }
            let after = read_view(&rig.host, kind).await;
            let mut expected = view.clone();
            expected.waits.retain(|other| other.hash != waiter);
            for (position, at) in positions.iter().zip(&chosen) {
                if let At::Stored(index) = at {
                    let hash = view.recs[position.coll][*index].hash.clone();
                    expected.recs[position.coll].retain(|rec| rec.hash != hash);
                }
            }
            let sorted = |mut recs: Vec<Rec>| {
                recs.sort_by(|left, right| left.hash.cmp(&right.hash));
                recs
            };
            for coll in 0..2 {
                prop_assert_eq!(
                    sorted(after.recs[coll].clone()),
                    sorted(expected.recs[coll].clone()),
                    "the records that remain"
                );
            }
            prop_assert_eq!(after.waits.len(), expected.waits.len(), "the waiter is spent");
            Ok(())
        })
        },
    );
}

// ─── Headline 15 ─────────────────────────────────────────────────────────────────────────────

/// The join operations in the re-issue `reissue`: the operation itself, or the resume of a
/// settle job.
fn reissued_joins(reissue: &SpaceReissue) -> Vec<&JoinOp> {
    match &reissue.op {
        SpaceOp::Join(op) => vec![op],
        SpaceOp::Settle(settle) => settle
            .cascade
            .as_ref()
            .and_then(|cascade| cascade.resume.as_ref())
            .map(|resume| reissued_joins(resume))
            .unwrap_or_default(),
        SpaceOp::Produce(_) | SpaceOp::Consume(_) => Vec::new(),
    }
}

/// `join_reissue_keeps_bind_ords` (`JoinCascade.v`, headline 15) and `ReissueKeepsBindOrds`
/// (`JoinCascade.tla`). A persistent join re-issues at a divergent pair or at the cap. Then it
/// carries the orders of its first round, one for each bind and equal for repeated binds
/// (decision 43). Each round of the join bumps the counter of each of its collections once.
///
/// * A divergent pair: each collection holds persistent records for the binds on it, so each
///   round fires once and re-issues. The case runs three rounds.
/// * The cap: the collection of the first bind holds records that are not persistent, enough for
///   more fires than the cap. The other collections hold persistent records. The chain runs to
///   its end.
#[test]
fn prop_join_reissue_keeps_bind_ords() {
    let strategy = (
        prop::sample::select(&[Kind::Queue, Kind::Stack, Kind::Pq][..]),
        prop::collection::vec(0..COLLS, 2..=3),
        prop_oneof![Just(0usize), 1usize..4],
    );
    check(pure_config(), strategy, |(kind, colls, extra)| {
        tokio_runtime().block_on(async move {
            let rig = Rig::new(kind).await;
            let level = match kind {
                Kind::Pq => Some(0),
                Kind::Queue | Kind::Stack | Kind::Cell | Kind::Bag | Kind::Set => None,
            };
            let capped = colls[0];
            let distinct: BTreeSet<usize> = colls.iter().copied().collect();
            for coll in &distinct {
                let copies = colls.iter().filter(|other| *other == coll).count();
                let index_of = |copy: usize| u64::try_from(copy).expect("a small index");
                match (extra, *coll == capped) {
                    (0, _) | (_, false) => {
                        for copy in 0..copies {
                            rig.run(SpaceOp::Produce(send(
                                *coll,
                                7,
                                true,
                                level,
                                index_of(copy),
                            )))
                            .await
                            .map_err(failure)?;
                        }
                    }
                    (_, true) => {
                        for index in 0..(FIRE_ROUND_CAP + extra) * copies {
                            rig.run(SpaceOp::Produce(send(
                                capped,
                                1,
                                false,
                                level,
                                10_000 + index_of(index),
                            )))
                            .await
                            .map_err(failure)?;
                        }
                    }
                }
            }
            let binds: Vec<Bind> = colls
                .iter()
                .map(|coll| Bind {
                    coll: *coll,
                    pat: Pat::Any,
                    lane: None,
                })
                .collect();
            let first: BTreeMap<usize, i64> = {
                let mut first = BTreeMap::new();
                for coll in &distinct {
                    first.insert(*coll, rig.counter(*coll).await.unwrap_or(0));
                }
                first
            };
            let expected: Vec<i64> = colls.iter().map(|coll| first[coll]).collect();
            let mut next = Some(SpaceOp::Join(join_of(
                &binds,
                "f",
                false,
                true,
                Guard::Open,
            )));
            let mut rounds = 0;
            let mut jobs = 0;
            while let Some(op) = next.take() {
                jobs += 1;
                prop_assert!(jobs <= MAX_CHAIN, "a chain runs past {} jobs", MAX_CHAIN);
                let is_join = matches!(op, SpaceOp::Join(_));
                let mut counters = BTreeMap::new();
                for coll in &distinct {
                    counters.insert(*coll, rig.counter(*coll).await.unwrap_or(0));
                }
                let outcome = rig.run(op).await.map_err(failure)?;
                if is_join {
                    rounds += 1;
                    for coll in &distinct {
                        prop_assert_eq!(
                            rig.counter(*coll).await.unwrap_or(0),
                            counters[coll] + 1,
                            "each round bumps the counter of each collection once"
                        );
                    }
                }
                if let Some(reissue) = &outcome.reissue {
                    for join in reissued_joins(reissue) {
                        prop_assert_eq!(
                            join.reissued_ords.as_ref(),
                            Some(&expected),
                            "the orders of the first round"
                        );
                    }
                }
                next = match (extra, outcome.reissue) {
                    (0, Some(reissue)) if rounds >= 3 => {
                        prop_assert_eq!(reissue.cause, ReissueCause::DivergentPair);
                        None
                    }
                    (_, reissue) => reissue.map(|reissue| reissue.op),
                };
            }
            prop_assert!(rounds >= 2, "the join re-issues: {} rounds", rounds);
            Ok(())
        })
    });
}

// ─── Headline 16 and the TLA invariant NoRecordLost ──────────────────────────────────────────

/// A program of peek joins that runs past the cap with second sends in flight (DV-1). It parks
/// `joins` peek joins on the collections 0, 1 and 2. It stores the records 2 and 3 on the two
/// collections other than `x_coll`, and then makes a persistent send of 1 on `x_coll`. The place
/// of `x_coll` in the group order decides whether the send stores first or fires the next join.
fn peek_heavy_program(kind: Kind, joins: usize, x_coll: usize) -> Program {
    let binds: Vec<Bind> = (0..COLLS)
        .map(|coll| Bind {
            coll,
            pat: Pat::Any,
            lane: None,
        })
        .collect();
    let mut steps: Vec<Step> = std::iter::repeat_n(
        Step::Join {
            binds,
            peek: true,
            persistent: false,
            guard: Guard::Open,
        },
        joins,
    )
    .collect();
    let mut value = 2;
    for coll in (0..COLLS).filter(|coll| *coll != x_coll) {
        steps.push(Step::Send {
            coll,
            value,
            persistent: false,
            level: None,
        });
        value += 1;
    }
    steps.push(Step::Send {
        coll: x_coll,
        value: 1,
        persistent: true,
        level: None,
    });
    Program { kind, steps }
}

/// The peek chain of `kind` in one of the two shapes of the cap tests. With `send_last`, the
/// persistent send is on the collection that is last in group order. Then the chain stops at the
/// cap with that send on the work stack (A1). Otherwise the send is first in group order, and it
/// stores. Then the settle job of the stop fires so many joins that its own drain stops at the cap.
/// That stop carries the second sends again (DV-1).
fn peek_heavy_shape(kind: Kind, send_last: bool) -> Program {
    let colls: Vec<Par> = (0..COLLS).map(ch).collect();
    let refs: Vec<&Par> = colls.iter().collect();
    let order = support::agent::group_positions(SPACE, &refs);
    match send_last {
        true => peek_heavy_program(kind, FIRE_ROUND_CAP + 2, order[COLLS - 1]),
        false => peek_heavy_program(kind, 2 * FIRE_ROUND_CAP + 2, order[0]),
    }
}

/// A program for the properties of Rule E. It is a generated program, a heavy program, or a
/// persistent send past the cap on a bag (a stop that owes nothing, DV-5). It can also be a peek
/// chain past the cap on a bag or a queue, in one of the two shapes of [`peek_heavy_shape`].
fn arb_rule_e_program() -> impl Strategy<Value = Program> {
    prop_oneof![
        5 => arb_program(&Kind::ALL, 1..12, COLLS, 4, true),
        1 => arb_maybe_heavy_program(),
        1 => Just(()).prop_map(|_| heavy_program(Kind::Bag, FIRE_ROUND_CAP + 2, Vec::new())),
        2 => (prop::sample::select(&[Kind::Bag, Kind::Queue][..]), any::<bool>())
            .prop_map(|(kind, send_last)| peek_heavy_shape(kind, send_last)),
    ]
}

/// The second sends that a peek fire still owes (decision 45, Rule E). They are its bound records
/// in receipt order, without the stored records that are persistent. The element in flight is
/// owed also when it is persistent (decision 30, DV-3). A send leaves the list when it runs, and
/// the first owed send must run first.
struct Owed {
    /// The identities of the owed sends, in receipt order.
    items: Vec<Blake2b256Hash>,
}

/// The owed second sends of the staged operation `staged` when it is a peek fire of a user
/// waiter.
fn owed_of(staged: &StagedCopy) -> Option<Owed> {
    let (continuation, peek, data) = match staged {
        StagedCopy::Produce(op) => match &op.outcome {
            ProduceOutcome::Fire {
                continuation, data, ..
            } => (
                &continuation.continuation,
                !continuation.peeks.is_empty(),
                data,
            ),
            ProduceOutcome::Store => return None,
        },
        StagedCopy::Consume(op) => match &op.outcome {
            ConsumeOutcome::Fire { data } => (&op.continuation, !op.peeks.is_empty(), data),
            ConsumeOutcome::Park => return None,
        },
    };
    let inner = record::unwrap_join_continuation(continuation)
        .map(|parked| parked.continuation)
        .or_else(|_| record::unwrap_pq_continuation(continuation).map(|parked| parked.continuation))
        .ok()?;
    step_of(&body_name(&inner))?;
    if !peek {
        return None;
    }
    let items = data
        .iter()
        .filter(|candidate| candidate.datum_index == -1 || !candidate.datum.persist)
        .map(|candidate| candidate.datum.source.hash.clone())
        .collect();
    Some(Owed { items })
}

/// The identity of the send that the staged operation `staged` makes on a data channel.
fn send_identity(staged: &StagedCopy) -> Option<Blake2b256Hash> {
    let StagedCopy::Produce(op) = staged else {
        return None;
    };
    data_coll(&op.channel)?;
    Some(Produce::create(&op.channel, &op.data, op.persist).hash)
}

/// The identities of the pending second sends of the re-issue `reissue`.
fn pending_identities(reissue: Option<&SpaceReissue>) -> Vec<Blake2b256Hash> {
    pending_sends(reissue)
        .into_iter()
        .map(|(identity, _)| identity)
        .collect()
}

/// The pending second sends of the re-issue `reissue`: the identity and the persist flag of each.
fn pending_sends(reissue: Option<&SpaceReissue>) -> Vec<(Blake2b256Hash, bool)> {
    match reissue.map(|reissue| &reissue.op) {
        Some(SpaceOp::Settle(SettleOp {
            space,
            cascade: Some(cascade),
            ..
        })) => cascade
            .pending
            .iter()
            .map(|pending| {
                let channel = data_channel(*space, &string_of_key(&pending.key));
                (
                    Produce::create(&channel, &pending.record, pending.persist).hash,
                    pending.persist,
                )
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The identity of the element of the operation `op` in its next round in `rig`. For a produce,
/// it is the identity of the record with the order that the round gives it. Another operation
/// gives `None`.
async fn own_identity(rig: &Rig, op: &SpaceOp) -> Option<Blake2b256Hash> {
    match op {
        SpaceOp::Produce(produce) => {
            let coll = coll_of_inner(&produce.channel);
            let ord = own_ord(rig, produce, coll).await;
            Some(
                Produce::create(
                    &data(coll),
                    &own_record(rig.kind, produce, ord),
                    produce.persistent,
                )
                .hash,
            )
        }
        SpaceOp::Consume(_) | SpaceOp::Join(_) | SpaceOp::Settle(_) => None,
    }
}

/// The inner channel of the collection key `key` of a generated program.
fn string_of_key(key: &CollectionKey) -> Par {
    (0..COLLS)
        .map(ch)
        .find(|channel| CollectionKey::of_channel(channel).is_ok_and(|own| own == *key))
        .expect("a key of a generated collection")
}

/// True when the re-issue `reissue` carries a produce: the operation itself, or the resume of a
/// settle job.
fn carries_produce(reissue: Option<&SpaceReissue>) -> bool {
    match reissue.map(|reissue| &reissue.op) {
        Some(SpaceOp::Produce(_)) => true,
        Some(SpaceOp::Settle(SettleOp {
            cascade: Some(cascade),
            ..
        })) => carries_produce(cascade.resume.as_deref()),
        _ => false,
    }
}

/// The cases of `prop_peek_join_restores_stored_candidates` that met each shape that the
/// property must cover.
struct RuleECoverage {
    /// A persistent element in flight that a peek fire sent again (DV-3).
    persistent_resent: AtomicUsize,
    /// A stop with the own element on the work stack: the operation re-issues, and the settle job
    /// does not carry the element (A1).
    own_reissued: AtomicUsize,
    /// A stop that owes nothing: the re-issue is the resume only (DV-5).
    resume_only: AtomicUsize,
    /// A stop that carries second sends in the pending sends of a settle job (DV-1).
    carried: AtomicUsize,
    /// A settle job whose drain stops at the cap and carries its pending sends again (DV-1).
    job_carried: AtomicUsize,
}

impl RuleECoverage {
    /// No case yet.
    const fn new() -> Self {
        RuleECoverage {
            persistent_resent: AtomicUsize::new(0),
            own_reissued: AtomicUsize::new(0),
            resume_only: AtomicUsize::new(0),
            carried: AtomicUsize::new(0),
            job_carried: AtomicUsize::new(0),
        }
    }

    /// Asserts that the property met every shape, so it is not vacuous.
    fn assert_not_vacuous(&self) {
        for (shape, count) in [
            (
                "a persistent element in flight sent again",
                &self.persistent_resent,
            ),
            ("a stop that re-issues the own element", &self.own_reissued),
            ("a stop that owes nothing", &self.resume_only),
            ("a stop that carries second sends", &self.carried),
            ("a settle job that stops at the cap", &self.job_carried),
        ] {
            assert!(count.load(Ordering::Relaxed) > 0, "no case met {shape}");
        }
    }
}

/// The coverage of `prop_peek_join_restores_stored_candidates`.
static RULE_E_COVERAGE: RuleECoverage = RuleECoverage::new();

/// `peek_join_restores_stored_candidates` (`JoinCascade.v`, headline 16). A peek fire of a user
/// waiter removes the stored records that it binds. Each of them that is not persistent is sent
/// again through OFFER, with its identity, in receipt order (decision 45, Rule E). The element in
/// flight is sent again too (decision 30, DV-3). A stop at the cap carries the owed second sends
/// in the pending sends of one settle job. The job sends them before any fire (DV-1).
///
/// The notes of the final alignment of `JoinCascade.v` with the code hold too:
///
/// * The element of the operation is never a pending second send (A5). A stop with that element
///   on the work stack re-issues the operation instead (A1).
/// * Only the element of the operation can be persistent. A pending second send and a second
///   send of a stored record are never persistent.
/// * A stop that owes nothing re-issues its resume only (DV-5). On a bag, a stop at the cap with
///   no owed second send re-issues the operation itself, not a settle job.
///
/// The cases meet each of these shapes, and the property asserts that it met them:
///
/// * a persistent element in flight that a peek fire sends again,
/// * a stop that re-issues the own element, and a stop that owes nothing,
/// * a stop that carries second sends, and a settle job whose own drain stops at the cap.
#[test]
fn prop_peek_join_restores_stored_candidates() {
    check(pure_config(), arb_rule_e_program(), |program| {
        tokio_runtime().block_on(peek_case(program))
    });
    RULE_E_COVERAGE.assert_not_vacuous();
}

/// One case of `prop_peek_join_restores_stored_candidates`.
async fn peek_case(program: Program) -> Result<(), TestCaseError> {
    let (rig, record) = Rig::recording(program.kind).await;
    for (index, step) in program.steps.iter().enumerate() {
        let mut owed: Vec<Owed> = Vec::new();
        let mut next = Some(space_op(step, index));
        let mut jobs = 0;
        while let Some(op) = next.take() {
            jobs += 1;
            prop_assert!(jobs <= MAX_CHAIN, "a chain runs past {} jobs", MAX_CHAIN);
            let own = own_identity(&rig, &op).await;
            let settle_job = matches!(&op, SpaceOp::Settle(_));
            *record.lock().expect("the record lock") = HostRecord::default();
            let result = rig.run(op).await;
            let seen = std::mem::take(&mut *record.lock().expect("the record lock"));
            let outcome = match result {
                Ok(outcome) => outcome,
                Err(error) => {
                    expected_refusal(program.kind, &error)?;
                    break;
                }
            };
            for (_, staged) in committed(&seen) {
                if let (Some(identity), StagedCopy::Produce(produce)) =
                    (send_identity(staged), staged)
                {
                    if let Some(entry) = owed
                        .iter_mut()
                        .rev()
                        .find(|entry| entry.items.first() == Some(&identity))
                    {
                        entry.items.remove(0);
                        if produce.persist {
                            prop_assert_eq!(
                                Some(&identity),
                                own.as_ref(),
                                "only the element of the operation is a persistent second send"
                            );
                            RULE_E_COVERAGE
                                .persistent_resent
                                .fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                if let Some(entry) = owed_of(staged) {
                    owed.push(entry);
                }
            }
            let pending = pending_sends(outcome.reissue.as_ref());
            for (identity, persist) in &pending {
                prop_assert!(!*persist, "a pending second send is not persistent");
                prop_assert!(
                    Some(identity) != own.as_ref(),
                    "the element of the operation is never a pending second send (A5)"
                );
            }
            let pending: Vec<Blake2b256Hash> =
                pending.into_iter().map(|(identity, _)| identity).collect();
            let resumed = carries_produce(outcome.reissue.as_ref());
            let mut carried = false;
            let mut own_left = false;
            for entry in &mut owed {
                let mut cursor = 0;
                let mut kept = Vec::with_capacity(entry.items.len());
                for item in std::mem::take(&mut entry.items) {
                    match (
                        pending[cursor..].iter().position(|sent| *sent == item),
                        Some(&item) == own.as_ref(),
                    ) {
                        (Some(found), _) => {
                            cursor += found + 1;
                            carried = true;
                            kept.push(item);
                        }
                        (None, true) => {
                            prop_assert!(
                                resumed,
                                "a stop with the element of the operation on the work stack re-issues the operation (A1)"
                            );
                            own_left = true;
                        }
                        (None, false) => {
                            return Err(failure(
                                "an owed second send is neither sent nor pending in the settle job",
                            ));
                        }
                    }
                }
                entry.items = kept;
            }
            if carried {
                RULE_E_COVERAGE.carried.fetch_add(1, Ordering::Relaxed);
                if settle_job {
                    RULE_E_COVERAGE.job_carried.fetch_add(1, Ordering::Relaxed);
                }
            }
            if own_left {
                RULE_E_COVERAGE.own_reissued.fetch_add(1, Ordering::Relaxed);
            }
            if let Some(reissue) = &outcome.reissue {
                if reissue.cause == ReissueCause::RoundCap && pending.is_empty() && !carried {
                    if program.kind == Kind::Bag {
                        prop_assert!(
                            !matches!(reissue.op, SpaceOp::Settle(_)),
                            "a stop that owes nothing re-issues its resume only (DV-5)"
                        );
                    }
                    if !matches!(reissue.op, SpaceOp::Settle(_)) {
                        RULE_E_COVERAGE.resume_only.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            next = outcome.reissue.map(|reissue| reissue.op);
        }
        for entry in &owed {
            prop_assert!(
                entry.items.is_empty(),
                "a peek fire leaves a second send owed at the end of its chain"
            );
        }
        rig.log().await;
    }
    Ok(())
}

/// The program `program` with unique sends on a set. So each class has one member, and no
/// `K_retire` removes a record outside the outcome of a job.
fn unique_set_values(mut program: Program) -> Program {
    if program.kind == Kind::Set {
        for (index, step) in program.steps.iter_mut().enumerate() {
            if let Step::Send { value, .. } = step {
                *value = 10 + i64::try_from(index).expect("a small index");
            }
        }
    }
    program
}

/// The identities of every record of the view `view`, as a multiset.
fn stored_identities(view: &View) -> BTreeMap<Blake2b256Hash, usize> {
    let mut identities = BTreeMap::new();
    for rec in view.recs.iter().flatten() {
        *identities.entry(rec.hash.clone()).or_insert(0) += 1;
    }
    identities
}

/// Adds `items` to the multiset `set`.
fn add_all(
    set: &mut BTreeMap<Blake2b256Hash, usize>,
    items: impl IntoIterator<Item = Blake2b256Hash>,
) {
    for item in items {
        *set.entry(item).or_insert(0) += 1;
    }
}

/// `NoRecordLost` (`JoinCascade.tla`). No job loses or duplicates a record. Before a job, take the
/// stored records, the pending second sends, and the element of the operation when it settles.
/// After the job, take the stored records, the pending second sends, and the records that the
/// fires spend. The two multisets are equal. A fire that does not peek spends its bound records
/// that are not persistent, and a peek fire spends none. The element of the operation settles
/// unless the re-issue still carries it. The cases cross the cap with second sends in flight
/// (DV-1).
#[test]
fn prop_no_record_lost_any_schedule() {
    check(
        pure_config(),
        arb_rule_e_program().prop_map(unique_set_values),
        |program| tokio_runtime().block_on(lost_case(program)),
    );
}

/// One case of `prop_no_record_lost_any_schedule`.
async fn lost_case(program: Program) -> Result<(), TestCaseError> {
    let rig = Rig::new(program.kind).await;
    let peeks: BTreeMap<String, bool> = program
        .steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| match step {
            Step::Receive { peek, .. } | Step::Join { peek, .. } => Some((name(index), *peek)),
            Step::Send { .. } => None,
        })
        .collect();
    for (index, step) in program.steps.iter().enumerate() {
        let mut next = Some(space_op(step, index));
        let mut jobs = 0;
        while let Some(op) = next.take() {
            jobs += 1;
            prop_assert!(jobs <= MAX_CHAIN, "a chain runs past {} jobs", MAX_CHAIN);
            let before_view = read_view(&rig.host, program.kind).await;
            let own = own_identity(&rig, &op).await;
            let mut before = stored_identities(&before_view);
            add_all(
                &mut before,
                pending_identities(Some(&SpaceReissue {
                    op: op.clone(),
                    cause: ReissueCause::RoundCap,
                })),
            );
            let result = rig.run(op).await;
            rig.log().await;
            let outcome = match result {
                Ok(outcome) => outcome,
                Err(error) => {
                    expected_refusal(program.kind, &error)?;
                    break;
                }
            };
            let after_view = read_view(&rig.host, program.kind).await;
            let mut after = stored_identities(&after_view);
            let pending = pending_sends(outcome.reissue.as_ref());
            for (identity, persist) in &pending {
                prop_assert!(!*persist, "a pending second send is not persistent");
                prop_assert!(
                    Some(identity) != own.as_ref(),
                    "the element of the operation is never a pending second send (A5)"
                );
            }
            add_all(
                &mut after,
                pending.into_iter().map(|(identity, _)| identity),
            );
            for fire in &outcome.fires {
                let name = body_name(&fire.continuation);
                let peek = *peeks
                    .get(&name)
                    .ok_or_else(|| failure(format!("an unknown body fires: {name}")))?;
                if peek {
                    continue;
                }
                add_all(
                    &mut after,
                    fire.results
                        .iter()
                        .filter(|result| !result.persistent)
                        .map(|result| {
                            Produce::create(&result.channel, &result.removed_datum, false).hash
                        }),
                );
            }
            let settled = match (&own, carries_produce(outcome.reissue.as_ref())) {
                (Some(own), false) => Some(own.clone()),
                (Some(_), true) | (None, _) => None,
            };
            add_all(&mut before, settled);
            prop_assert_eq!(after, before, "the records before and after a job");
            next = outcome.reissue.map(|reissue| reissue.op);
        }
    }
    Ok(())
}

// ─── CounterConflict.v: join_ops_race_on_every_counter ───────────────────────────────────────

/// The merge index of the event log `log` of a branch over the pre-state `root` of `host`, with
/// the pre-state closures of `block_index.rs::create_event_log_index`.
fn merge_index(host: &ParRSpace, root: &Blake2b256Hash, log: &[Event]) -> EventLogIndex {
    let reader = host
        .get_history_repository()
        .get_history_reader(root)
        .expect("read the history at the pre-state root");
    let produce_exists_in_pre_state = |produce: &Produce| {
        reader
            .get_data(&produce.channel_hash)
            .is_ok_and(|data| data.iter().any(|datum| datum.source == *produce))
    };
    let produce_touches_pre_state_join = |produce: &Produce| {
        reader
            .get_joins(&produce.channel_hash)
            .is_ok_and(|joins| joins.iter().any(|join| join.len() > 1))
    };
    EventLogIndex::new(
        log.to_vec(),
        produce_exists_in_pre_state,
        produce_touches_pre_state_join,
        NumberChannelsDiff::new(),
    )
}

/// Runs `op` as one chain over the pre-state `root` of the rig and returns the event log of the
/// branch.
async fn branch(
    rig: &Rig,
    root: &Blake2b256Hash,
    op: SpaceOp,
) -> Result<Vec<Event>, TestCaseError> {
    rig.host.reset(root).await.map_err(failure)?;
    rig.log().await;
    for job in chain(rig, op).await? {
        if let Err(error) = job.result {
            expected_refusal(rig.kind, &error)?;
        }
    }
    Ok(rig.log().await)
}

/// `join_ops_race_on_every_counter` (`CounterConflict.v`). Branch A runs a join over the
/// collections `ks`, and branch B runs any operation on a collection `k` of `ks`. Then merge check
/// #1 reports the counter `S(sid,k)` of `k`. When `k` opens in both branches, it reports the space
/// counter `S(sid)`. The pre-state is a generated program on a discipline with counters.
#[test]
fn prop_join_ops_race_on_every_counter() {
    let strategy = arb_program(&[Kind::Queue, Kind::Stack, Kind::Pq], 0..8, COLLS, 4, false)
        .prop_flat_map(|program| {
            let kind = program.kind;
            (
                Just(program),
                prop::collection::vec(arb_bind(kind, COLLS, 4), 2..=3),
                any::<prop::sample::Index>(),
                arb_step(kind, Persistence::Neither, 1, 4, false),
            )
        });
    check(pure_config(), strategy, |(program, binds, pick, other)| {
        tokio_runtime().block_on(async move {
            let rig = Rig::new(program.kind).await;
            for (index, step) in program.steps.iter().enumerate() {
                run_step(&rig, step, index).await?;
            }
            let root = rig.root().await;
            let k = binds[pick.index(binds.len())].coll;
            let opened = rig.counter(k).await.is_some();
            let other = match other {
                Step::Send {
                    value,
                    persistent,
                    level,
                    ..
                } => Step::Send {
                    coll: k,
                    value,
                    persistent,
                    level,
                },
                Step::Receive {
                    bind,
                    peek,
                    persistent,
                } => Step::Receive {
                    bind: Bind { coll: k, ..bind },
                    peek,
                    persistent,
                },
                Step::Join {
                    binds,
                    peek,
                    persistent,
                    guard,
                } => Step::Join {
                    binds: binds
                        .into_iter()
                        .map(|bind| Bind { coll: k, ..bind })
                        .collect(),
                    peek,
                    persistent,
                    guard,
                },
            };
            let join = Step::Join {
                binds,
                peek: false,
                persistent: false,
                guard: Guard::Open,
            };
            let left = branch(&rig, &root, space_op(&join, 100)).await?;
            let right = branch(&rig, &root, space_op(&other, 200)).await?;
            let found: BTreeSet<Blake2b256Hash> = conflicts(
                &merge_index(&rig.host, &root, &left),
                &merge_index(&rig.host, &root, &right),
            )
            .0
            .into_iter()
            .collect();
            let counter =
                rspace_plus_plus::rspace::hashing::stable_hash_provider::hash(&match opened {
                    true => counter_channel(SPACE, &ch(k)),
                    false => support::agent::space_counter_channel(SPACE),
                });
            prop_assert!(
                found.contains(&counter),
                "check #1 reports the counter of the shared collection: {:?}",
                found
            );
            Ok(())
        })
    });
}

// ─── The engine anchor ───────────────────────────────────────────────────────────────────────

// Changed by decision 94: the transaction of `begin_collection` holds phase A only, so the
// helper takes phase B over the keys of the scope before `run_join`, as `prepare_collection_op`
// does.
// /// Runs the operation `op` on the collection `coll` with the join engine of `hashmap:<D>`, forced
// /// on a collection without joins. The classification of a settle job with a cascade gives the
// /// scope of the collection, and `run_join` runs `op` in it.
/// Runs the operation `op` on the collection `coll` with the join engine of `hashmap:<D>`, forced
/// on a collection without joins. The classification of a settle job with a cascade gives the
/// scope of the collection under phase A. The transaction takes phase B over the keys of the
/// scope, and `run_join` runs `op` in it (decision 94).
async fn forced<D: Discipline>(
    env: &SpaceEnv,
    op: SpaceOp,
    coll: usize,
) -> Result<SpaceOutcome, SpaceError>
where
    GenericSpace<HashMapOuter, D>: FamilySpace,
{
    let agent = GenericSpace::<HashMapOuter, D>::default();
    let settle = SpaceOp::Settle(SettleOp {
        space: SPACE,
        key: key(coll),
        channel: ch(coll),
        random_state: Vec::new(),
        authority: models::rhoapi::CostAuthority::default(),
        introduction_authority: models::rhoapi::CostAuthority::default(),
        cascade: Some(Box::new(CascadeState {
            seeds: Vec::new(),
            pending: Vec::new(),
            resume: None,
        })),
    });
    let mut opened = begin_collection(&HashMapOuter, env, SPACE, &key(coll), &ch(coll)).await?;
    let JoinRoute::Join(scope) = agent.classify(&mut opened, &settle)? else {
        return Err(SpaceError::Inconsistent(
            "a settle with a cascade classifies as a join",
        ));
    };
    // Added by decision 94.
    env.lock_phase_b(&mut opened.txn, scope.lock_keys()).await?;
    agent.run_join(env, opened, scope, op)?.commit().await
}

/// A join-free program: the generated steps with each join replaced by a receive of its first
/// bind.
fn arb_join_free_program() -> impl Strategy<Value = Program> {
    arb_program(&Kind::ALL, 0..10, COLLS, 4, false).prop_map(|mut program| {
        for step in &mut program.steps {
            if let Step::Join {
                binds,
                peek,
                persistent,
                ..
            } = step
            {
                *step = Step::Receive {
                    bind: binds[0],
                    peek: *peek,
                    persistent: *persistent,
                };
            }
        }
        program
    })
}

/// Added by decision 102 (plan v2 pins): the most tries of the driver in one case of the engine
/// anchor, until it binds the records that the join engine bound. Play picks the partners of a
/// bag and a set at random (rule CR-1).
const PARK_TRIES: usize = 128;

/// Added by decision 102 (plan v2 pins): what one job of the engine anchor shows.
struct AnchorJob {
    /// The outcome of the job, or its error.
    result: Result<SpaceOutcome, SpaceError>,
    /// The events of the job.
    log: Vec<Event>,
    /// The observer calls of the job.
    calls: Vec<support::observed::Call>,
    /// The root of a checkpoint after the job.
    root: Blake2b256Hash,
}

/// Added by decision 102 (plan v2 pins): the budget of one job of the engine anchor.
fn anchor_budget() -> RuntimeBudget {
    RuntimeBudget::new(Cost::create(1_000_000_000_000, "rrs join anchor"))
}

/// Added by decision 102 (plan v2 pins): runs `op` on the collection `coll` of `rig` in one job.
/// The join engine of the discipline, forced on the collection, runs the job when `engine`
/// holds, and the driver of the discipline runs it otherwise. The job starts from the current
/// state of the play space of `rig`, and it ends with a checkpoint.
async fn anchor_job(rig: &Rig, engine: bool, op: &SpaceOp, coll: usize) -> AnchorJob {
    let budget = anchor_budget();
    let recorder = observed(&rig.host);
    let env = SpaceEnv {
        budget: budget.clone(),
        ..rig.env.clone()
    };
    let result = match (engine, rig.kind) {
        (false, _) => run(&FamilyAgent, &env, op.clone()).await,
        (true, Kind::Queue) => forced::<QueueDiscipline>(&env, op.clone(), coll).await,
        (true, Kind::Stack) => forced::<StackDiscipline>(&env, op.clone(), coll).await,
        (true, Kind::Cell) => forced::<CellDiscipline>(&env, op.clone(), coll).await,
        (true, Kind::Bag) => forced::<BagDiscipline>(&env, op.clone(), coll).await,
        (true, Kind::Set) => forced::<SetDiscipline>(&env, op.clone(), coll).await,
        (true, Kind::Pq) => forced::<PriorityQueueDiscipline>(&env, op.clone(), coll).await,
    };
    let log = rig.log().await;
    let calls = recorder.take();
    let root = rig.root().await;
    AnchorJob {
        result,
        log,
        calls,
        root,
    }
}

// Retired by decision 90 (C6, step D): `replayed_driver_job` ran through the rigged replay modes,
// which retire (decision 125). The step 2 of `prop_join_engine_agrees_with_drivers_without_joins`,
// `space_native_replay_equal_hashmap`.

// Retired by decision 90 (C6, step D): `fires_in_debug_order`, a helper of the driver replay, ran
// through the rigged replay modes, which retire (decision 125). The step 2 of
// `prop_join_engine_agrees_with_drivers_without_joins`, `space_native_replay_equal_hashmap`.

// Retired by decision 90 (C6, step D): `operations_in_debug_order`, a helper of the driver replay,
// ran through the rigged replay modes, which retire (decision 125). The step 2 of
// `prop_join_engine_agrees_with_drivers_without_joins`, `space_native_replay_equal_hashmap`.

/// Added by decision 102 (plan v2 pins): the continuation and the removed records of each fire
/// of `result`, in fire order. An error binds no record.
fn bound_records(
    result: &Result<SpaceOutcome, SpaceError>,
) -> Vec<(models::rhoapi::TaggedContinuation, Vec<ListParWithRandom>)> {
    match result {
        Ok(outcome) => outcome
            .fires
            .iter()
            .map(|fire| {
                (
                    fire.continuation.clone(),
                    fire.results
                        .iter()
                        .map(|result| result.removed_datum.clone())
                        .collect(),
                )
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// The engine anchor (joins plan 7.3). Take a collection without joins, below the cap. The join
/// engine, forced on the state, stages what the driver of the discipline stages. It gives the
/// same outcome, the same events, the same observer calls (decision 90, decision 69) and the same
/// root. On the set, the
/// pre-state has no fireable waiter, because the receives of the programs have no guard
/// (decision 50).
///
/// Changed by decision 102 (plan v2 pins): play picks the partners of a bag and a set at random
/// (rule CR-1), and plan items C5.3 and C5.5 make the windows of the join engine random too. So
/// the program runs once, and both sides of the last job start from one checkpoint after it:
///
/// 1. The join engine runs the job in play. Its log is the log of the case.
/// 2. The driver plays the job from the checkpoint again, up to [`PARK_TRIES`] times, until it
///    binds the records that the engine bound. Then its outcome, events, observer calls and
///    root must equal those of the engine.
///
/// The step of the driver in replay retired with the rigged replay modes (decision 90, C6, step
/// D). Step 2 adds the full observer calls of play. A try of step 2 finds the binding of the
/// engine with probability 1/n or more when the job has n possible bindings. So a case whose job
/// has at most nine possible bindings misses all tries with probability (8/9)^128, about
/// 2.8·10^-7. A case that misses all tries skips step 2 and fails nothing. So the property has no
/// probabilistic assertion.
#[test]
fn prop_join_engine_agrees_with_drivers_without_joins() {
    let strategy = arb_join_free_program().prop_flat_map(|program| {
        let kind = program.kind;
        (
            Just(program),
            arb_step(kind, Persistence::Neither, COLLS, 4, false),
        )
    });
    check(pure_config(), strategy, |(program, last)| {
        tokio_runtime().block_on(async move {
            let last = match last {
                Step::Join { binds, peek, .. } => Step::Receive {
                    bind: binds[0],
                    peek,
                    persistent: false,
                },
                step => step,
            };
            let index = program.steps.len();
            let op = space_op(&last, index);
            let coll = match &op {
                SpaceOp::Produce(produce) => coll_of_inner(&produce.channel),
                SpaceOp::Consume(consume) => coll_of_inner(&consume.channel),
                SpaceOp::Join(_) | SpaceOp::Settle(_) => {
                    return Err(failure("the last step is a send or a receive"))
                }
            };
            // Changed by decision 102 (plan v2 pins): play picks at random (rule CR-1), so two rigs
            // that run one program can reach two states. The program runs once, and both sides
            // of the last job start from one checkpoint. The old lines follow.
            // let mut results = Vec::with_capacity(2);
            // for engine in [false, true] {
            //     let rig = Rig::new(program.kind).await;
            //     for (step_index, step) in program.steps.iter().enumerate() {
            //         run_step(&rig, step, step_index).await?;
            //     }
            //     rig.log().await;
            //     let budget = rholang::rust::interpreter::accounting::RuntimeBudget::new(
            //         rholang::rust::interpreter::accounting::costs::Cost::create(
            //             1_000_000_000_000,
            //             "rrs join anchor",
            //         ),
            //     );
            //     let _scope = budget.enter_legacy_scope();
            //     let env = SpaceEnv {
            //         budget: budget.clone(),
            //         ..rig.env.clone()
            //     };
            //     let result = match (engine, program.kind) {
            //         (false, _) => run(&FamilyAgent, &env, op.clone()).await,
            //         (true, Kind::Queue) => forced::<QueueDiscipline>(&env, op.clone(), coll).await,
            //         (true, Kind::Stack) => forced::<StackDiscipline>(&env, op.clone(), coll).await,
            //         (true, Kind::Cell) => forced::<CellDiscipline>(&env, op.clone(), coll).await,
            //         (true, Kind::Bag) => forced::<BagDiscipline>(&env, op.clone(), coll).await,
            //         (true, Kind::Set) => forced::<SetDiscipline>(&env, op.clone(), coll).await,
            //         (true, Kind::Pq) => {
            //             forced::<PriorityQueueDiscipline>(&env, op.clone(), coll).await
            //         }
            //     };
            //     let log = rig.log().await;
            //     let charges = budget.get_log();
            //     let root = rig.root().await;
            //     results.push((result, log, charges, root));
            // }
            // let (driver, engine) = (&results[0], &results[1]);
            // prop_assert_eq!(&engine.0, &driver.0, "the outcome");
            // prop_assert_eq!(&engine.1, &driver.1, "the events");
            // prop_assert_eq!(&engine.2, &driver.2, "the legacy charges");
            // prop_assert_eq!(&engine.3, &driver.3, "the root");
            // Changed by decision 90 (C6, step D): the driver no longer replays the log of the
            // engine.
            let play_space = support::agent::play().await;
            let rig = Rig::over(play_space, program.kind).await;
            for (step_index, step) in program.steps.iter().enumerate() {
                run_step(&rig, step, step_index).await?;
            }
            rig.log().await;
            let start = rig.root().await;

            // 1. The join engine plays the job.
            let engine = anchor_job(&rig, true, &op, coll).await;
            rig.host
                .reset(&start)
                .await
                .expect("the play space goes back to the checkpoint");

            // Retired by decision 90 (C6, step D): the replay half ran through the rigged replay
            // modes, which retire (decision 125). The step 2 of this property,
            // `space_native_replay_equal_hashmap`.

            // 2. The driver plays the job until it binds the records of the engine.
            let wanted = bound_records(&engine.result);
            for _ in 0..PARK_TRIES {
                let driver = anchor_job(&rig, false, &op, coll).await;
                rig.host
                    .reset(&start)
                    .await
                    .expect("the play space goes back to the checkpoint");
                if bound_records(&driver.result) == wanted {
                    prop_assert_eq!(&engine.result, &driver.result, "the outcome");
                    prop_assert_eq!(&engine.log, &driver.log, "the events");
                    prop_assert_eq!(&engine.calls, &driver.calls, "the observer calls");
                    prop_assert_eq!(&engine.root, &driver.root, "the root");
                    break;
                }
            }
            Ok(())
        })
    });
}

// ─── A negative control of the model ─────────────────────────────────────────────────────────

/// The model is not vacuous. Raw host operations store an owed pair with no plan, and the model
/// reports a pending match. `INV_head` fails on a queue, a stack and a set, and `INV_scan` fails
/// on a bag.
#[tokio::test]
async fn reference_model_reports_owed_pairs() {
    for kind in [Kind::Queue, Kind::Stack, Kind::Set, Kind::Bag] {
        let rig = Rig::new(kind).await;
        raw_record(&rig, 0, 1, 0, 1).await.expect("a raw record");
        raw_record(&rig, 1, 2, 0, 2).await.expect("a raw record");
        let binds = [
            Bind {
                coll: 0,
                pat: Pat::Any,
                lane: None,
            },
            Bind {
                coll: 1,
                pat: Pat::Any,
                lane: None,
            },
        ];
        raw_join(&rig, &binds, "owed", &[0, 0])
            .await
            .expect("a raw join");
        let view = read_view(&rig.host, kind).await;
        assert!(
            check_inv(&view).is_err(),
            "{kind:?}: the model reports the owed pair"
        );
    }
}

// ─── The keyed differential of the store seams (joins plan section 9) ────────────────────────

// Retired by decision 125 (the step D restatement at the failing step): the golden and the stored
// logs predate decision 118, and no play of the new rule reaches programs 44 and 78, so a capture
// cannot give a new baseline. The replay half needs the legacy replay mode (`play_and_replay`,
// `rig_and_reset`, `check_replay_data`), which step D retires, and the native harness plays
// Rholang terms, not generated space operations. The purpose of the test, the guard of the store
// seams of joins plan 9, is served: the seams landed, the differential passed with them before
// decision 118, and the oracle properties of this file check the engine rules now. The vector
// files stay on disk. The items below served only this test.
// /// The golden of the keyed differential, relative to the workspace root.
// const SEAMS_GOLDEN: &str = "test-vectors/reified-join-seams-v1.json";
//
// /// The environment variable that switches the differential to capture mode. Its value is an
// /// absolute directory under `target/`.
// const SEAMS_CAPTURE: &str = "RRS_CAPTURE_JOIN_SEAMS_OUT";
//
// /// The general programs of the corpus.
// const SEAMS_PROGRAMS: usize = 64;
//
// /// The programs of the corpus that may be heavy: a send past the fire cap.
// const SEAMS_HEAVY: usize = 16;
//
// /// The phlo of the legacy budget of one job of the differential.
// const SEAMS_PHLO: i64 = 100_000_000;
//
// /// The corpus of the differential, drawn with the fixed seed of the P5 properties: 64 general
// /// programs of every discipline, then 16 programs that may be heavy.
// fn seams_corpus() -> Vec<Program> {
//     let mut runner = TestRunner::new(pure_config());
//     let general = arb_program(&Kind::ALL, 1..9, COLLS, 3, true);
//     let heavy = arb_maybe_heavy_program();
//     let mut programs = Vec::with_capacity(SEAMS_PROGRAMS + SEAMS_HEAVY);
//     for _ in 0..SEAMS_PROGRAMS {
//         let tree = general
//             .new_tree(&mut runner)
//             .expect("draw a general program");
//         programs.push(tree.current());
//     }
//     for _ in 0..SEAMS_HEAVY {
//         let tree = heavy
//             .new_tree(&mut runner)
//             .expect("draw a program that may be heavy");
//         programs.push(tree.current());
//     }
//     programs
// }
//
// /// The text of the staged operation `staged`: its channels, its data or its continuation, and its
// /// outcome.
// fn staged_text(staged: &StagedCopy) -> String {
//     match staged {
//         StagedCopy::Produce(op) => format!(
//             "produce on {:?} data {:?} persist {} outcome {:?}",
//             op.channel, op.data, op.persist, op.outcome
//         ),
//         StagedCopy::Consume(op) => format!(
//             "consume on {:?} patterns {:?} continuation {:?} persist {} peeks {:?} outcome {:?}",
//             op.channels, op.patterns, op.continuation, op.persist, op.peeks, op.outcome
//         ),
//     }
// }
//
// /// Runs `program` over a recording host, each step with every re-issue that follows it, and
// /// returns the text of the run. For each job, the text holds its result, its legacy log, its phlo
// /// and its events. Then it holds each begin, each staged operation and each commit, and the root
// /// at the end.
// async fn seams_text(program: &Program) -> String {
//     let (rig, record) = Rig::recording(program.kind).await;
//     let mut text = format!("{program:?}\n");
//     for (index, step) in program.steps.iter().enumerate() {
//         let mut next = Some(space_op(step, index));
//         let mut jobs = 0;
//         while let Some(op) = next.take() {
//             assert!(
//                 jobs < MAX_CHAIN,
//                 "a chain of the differential runs past {MAX_CHAIN} jobs"
//             );
//             jobs += 1;
//             let legacy = RuntimeBudget::new(Cost::create(SEAMS_PHLO, "rrs join seams job"));
//             let _scope = legacy.enter_legacy_scope();
//             let env = SpaceEnv {
//                 budget: legacy.clone(),
//                 ..rig.env.clone()
//             };
//             let before = legacy.get().value;
//             let result = run(&FamilyAgent, &env, op).await;
//             next = result
//                 .as_ref()
//                 .ok()
//                 .and_then(|outcome| outcome.reissue.as_ref())
//                 .map(|reissue| reissue.op.clone());
//             text.push_str(&format!(
//                 "step {index} job {jobs}: {}\n",
//                 seams_result_text(&result)
//             ));
//             text.push_str(&format!(
//                 "  legacy {:?} phlo {}\n",
//                 legacy.get_log(),
//                 before - legacy.get().value
//             ));
//             for event in rig.log().await {
//                 text.push_str(&format!("  event {event:?}\n"));
//             }
//         }
//     }
//     {
//         let seen = record.lock().expect("the record lock");
//         for (txn, keys) in seen.begins.iter().enumerate() {
//             text.push_str(&format!("begin {txn} {keys:?}\n"));
//         }
//         for (position, (txn, staged)) in seen.staged.iter().enumerate() {
//             text.push_str(&format!(
//                 "staged {position} txn {txn}: {}\n",
//                 staged_text(staged)
//             ));
//         }
//         text.push_str(&format!("commits {:?}\n", seen.commits));
//     }
//     text.push_str(&format!("root {:?}\n", rig.root().await));
//     text
// }
//
// /// The text of the result of one job, as the golden holds it: the fires and the re-issue of an
// /// outcome, or the error. The golden predates the mirror writes of an outcome (P6 step 7b), and
// /// no space of the corpus has a mirror, so the text leaves them out and the run checks that they
// /// are absent.
// fn seams_result_text(result: &Result<SpaceOutcome, SpaceError>) -> String {
//     match result {
//         Ok(outcome) => {
//             assert!(
//                 outcome.mirror.is_none(),
//                 "no space of the seams corpus has a mirror"
//             );
//             format!(
//                 "Ok(SpaceOutcome {{ fires: {:?}, reissue: {:?} }})",
//                 outcome.fires, outcome.reissue
//             )
//         }
//         Err(error) => format!("Err({error:?})"),
//     }
// }
//
// /// The root of the workspace.
// fn workspace_root() -> PathBuf {
//     Path::new(env!("CARGO_MANIFEST_DIR"))
//         .parent()
//         .expect("the rholang crate lives inside the workspace")
//         .to_path_buf()
// }
//
// /// The capture directory of the differential, when the run captures the golden. It must be an
// /// absolute directory under `target/`, so a capture never writes to the source tree.
// fn seams_capture_directory() -> Option<PathBuf> {
//     let directory = PathBuf::from(std::env::var_os(SEAMS_CAPTURE)?);
//     let target = workspace_root().join("target");
//     let canonical_target = target.canonicalize().unwrap_or_else(|_| target.clone());
//     let inside_target = directory.is_absolute()
//         && directory
//             .components()
//             .all(|component| !matches!(component, Component::ParentDir))
//         && (directory.starts_with(&target) || directory.starts_with(&canonical_target));
//     assert!(
//         inside_target,
//         "{SEAMS_CAPTURE} must be an absolute path under {}, got {}",
//         target.display(),
//         directory.display()
//     );
//     Some(directory)
// }
//
// /// The keyed differential of the store seams (joins plan section 9). The store seams of the
// /// PathMap source must leave the join engine of the keyed stores unchanged. For every program of
// /// a fixed corpus, the engine must stage the same operations in the same order, with the same
// /// results, legacy charges, phlo, events, lock keys and root. The golden holds the digest of the
// /// text of each run (`seams_text`), captured at the commit `930b1d64c`, before the seams. The
// /// capture also held the host-work usage of the agent at each begin, staged operation and commit.
// /// DR-131 removed host work, so the golden holds the digests of the captured texts without their
// /// `usage` lines.
// #[test]
// fn join_seams_keyed_differential() {
//     let runtime = tokio_runtime();
//     let capture = seams_capture_directory();
//     if let Some(directory) = &capture {
//         std::fs::create_dir_all(directory.join("programs")).expect("create the capture directory");
//     }
//     let programs = seams_corpus();
//     let mut actual = Vec::with_capacity(programs.len());
//     for (index, program) in programs.iter().enumerate() {
//         let text = runtime.block_on(seams_text(program));
//         let digest = hex::encode(Blake2b256Hash::new(text.as_bytes()).bytes());
//         if let Some(directory) = &capture {
//             std::fs::write(directory.join(format!("programs/{index:03}.txt")), &text)
//                 .expect("write the text of a program");
//         }
//         actual.push(serde_json::json!({
//             "index": index,
//             "kind": format!("{:?}", program.kind),
//             "steps": program.steps.len(),
//             "digest": digest,
//         }));
//     }
//     let document = serde_json::json!({
//         "schema": "reified-join-seams-v1",
//         "captured_at": "930b1d64c",
//         "seed": format!("{:#018x}", support::props::PROPERTY_SEED),
//         "programs": actual,
//     });
//     if let Some(directory) = &capture {
//         let mut text =
//             serde_json::to_string_pretty(&document).expect("serialize the golden of the seams");
//         text.push('\n');
//         std::fs::write(directory.join("reified-join-seams-v1.json"), text)
//             .expect("write the golden of the seams");
//         return;
//     }
//     let path = workspace_root().join(SEAMS_GOLDEN);
//     let golden: serde_json::Value =
//         serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|error| {
//             panic!(
//                 "read {}: {error}; capture the golden with {SEAMS_CAPTURE}",
//                 path.display()
//             )
//         }))
//         .expect("parse the golden of the seams");
//     let expected = golden["programs"]
//         .as_array()
//         .expect("the golden lists its programs");
//     assert_eq!(
//         expected.len(),
//         actual.len(),
//         "the corpus has the size of the golden"
//     );
//     let differing: Vec<String> = expected
//         .iter()
//         .zip(&actual)
//         .filter(|(left, right)| left != right)
//         .map(|(left, right)| format!("golden {left}, run {right}"))
//         .collect();
//     assert!(
//         differing.is_empty(),
//         "the keyed engine differs from {SEAMS_GOLDEN} on {} programs:\n{}",
//         differing.len(),
//         differing.join("\n")
//     );
// }
