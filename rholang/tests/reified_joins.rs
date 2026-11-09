//! The same-space joins of the native spaces (P5c, decision 29, joins plan sections 3 to 7).
//!
//! A receive with two or more binds whose channels all route to one native space is a join
//! (`SpaceOp::Join`). The join engine (`spaces/join.rs`) runs every join. It also runs every
//! produce, consume or settle whose collection has a join group, and every settle with a cascade
// Changed by decision 90 (C6, step D): no test of this file takes a replay space. The text
// before:
// ```
// //! state. The tests drive the family agent over an in-memory play space and its replay space.
// ```
//! state. The tests drive the family agent over an in-memory play space.
//! The Rholang tests run deploys through the runtime session of the P5 suites.
//!
//! The tests of the joins plan, section 7.1:
//!
//! | Test | What it shows |
//! |---|---|
// Changed by decision 102 (plan v4 C5.3, rule CR-1): play picks the window of a collection that
// the group names once at random. The row before:
// //! | `space_joins_bag_greedy` | the first spatial match of each bind in DR-75 order, and the computed greedy limit that parks (decision 37) |
//! | `space_joins_bag_greedy` | a random match of each bind on a collection that the group names once (decision 102), and the computed greedy limit that parks (decision 37) |
//! | `space_joins_set_classes` | two binds on one set take two classes, and a class is never bound twice |
//! | `space_joins_set_bind_retires_class` | a fire that does not peek retires the other members of each bound class with `K_retire` |
//! | `space_joins_cell_join` | a join over two cells, `CellOccupied` for a second waiter, and `[c, c]` with the incoming and the stored record |
//! | `space_joins_queue_heads_only` | a join waiter fires only as the head of each queue, and a head that does not match blocks |
//! | `space_joins_stack_heads_only` | tops only, and a fresh join becomes the top waiter |
//! | `space_joins_pq_heads_only` | one lane for each bind, and the older lane head fires first (decision 38) |
// Changed by decision 103 (decision 118, rule CR-9, decision-c6 §6.1 and §6.2): a persistent
// entry claims nothing, so it can bind several positions. The rows before:
// //! | `space_joins_repeated_channel_distinct_records` | k binds on one channel bind k distinct records, persistent ones included (decision 28) |
// //! | `space_joins_incoming_binds_once` | the incoming datum binds exactly one position, with the position retry |
//! | `space_joins_persistent_record_binds_two_positions` | a persistent record binds several positions, and a non-persistent record binds one (decision 118, rule (a)) |
//! | `space_joins_incoming_binds_once` | a non-persistent incoming datum binds one position, with the position retry, and a persistent one binds two |
//! | `space_joins_multi_ord_envelope_roundtrip` | the record of version 2 survives the commit, a checkpoint and a reset |
//! | `space_joins_bind_order_restored_for_body` | the body gets the values and the random states in bind order, as a host join |
//! | `space_joins_guard_checked_once_after_all_binds` | one guard check over the values in bind order for each attempt |
//! | `space_joins_cascade_after_produce` | the exposure of a queue head after a produce-side join fire |
//! | `space_joins_cascade_after_consume` | the exposure of a stack head after a consume-side join fire |
//! | `space_joins_cascade_round_cap_reissues_settle` | one fire count, and the settle that carries the seeds (decision 49) |
//! | `space_joins_cascade_order_independent` | two disjoint fireable heads in either order give one state |
//! | `space_joins_cross_space_join_aborts` | `CrossSpaceJoin` for two spaces |
//! | `space_joins_default_and_space_join_aborts` | `CrossSpaceJoin` for the default space and a space |
//! | `space_joins_persisted_waiter_check3_conflict` | a parked join in the pre-state makes a produce on its channels conflict (merge check #3) |
//! | `space_joins_escalation_relocks_once` | two transactions, the second over the closure, and one commit |
// Changed by decision 90 (C6, step D, ruling R-11): the test runs native replay.
//! | `space_joins_replay_equal` | native replay certifies the play of the join script on each discipline |
//! | `space_joins_restart` | join waiters, records of version 2 and join groups survive a checkpoint and a reset |
//!
//! The tests of the joins plan, section 7.2 (decisions 43, 45, 46, 49 and 50):
//!
//! | Test | What it shows |
//! |---|---|
//! | `space_joins_peek_join_restores_stored_candidates` | Rule E on the consume side: each removed record is sent again with its identity and its own authority |
//! | `space_joins_produce_side_peek_reproduces_in_receipt_order` | Rule E on the produce side, equal to the host program with `produce_peeks` |
//! | `space_joins_reissued_join_keeps_bind_ords` | decision 43 for a persistent join |
//! | `space_joins_cap_carries_pending_second_sends` | the settle of a stop carries the pending second sends and the operation |
//! | `space_joins_settle_before_cap_resumes_the_operation` | the settle before the plan fills the transaction, and the operation resumes later |
//! | `space_joins_txn_fires_bounded_at_the_cap` | no transaction makes more than `FIRE_ROUND_CAP` fires |
//! | `space_joins_legacy_charges_equal_host` | retired by decision 90 (decision 69) with Option P: each staged shape charged what `ChargingRSpace` charged for its host call |
//! | `space_joins_classification_fires_as_check_joins` | a collection without joins fires as it fired at the second-wave commit |
//! | `space_joins_native_execution_fails_closed` | decision 46: a join fails before any space work |
// Changed by decision 118: the trace of F-JF-1 is vacuous (NoSetCascadeUnsafe.cfg:8-14). The row
// before:
// //! | `space_joins_set_join_cascade_fires_exposed_waiter` | decision 50: a set waiter that a class move exposes fires in the same transaction |
//! | `space_joins_set_join_cascade_fires_exposed_waiter` | decision 118: the waiter of the F-JF-1 trace fires at its arrival, because its class has a persistent member |
//!
//! Added by decision 103 (decision 118, rule CR-9, decision-c6 §6): the tests of rule (a), the
//! claim rule of the join search:
//!
//! | Test | What it shows |
//! |---|---|
//! | `space_joins_persistent_incoming_binds_two_positions` | a persistent incoming datum binds two positions of its collection in one COMM |
//! | `space_joins_peek_persistent_incoming_is_sent_once` | a peek fire sends a persistent incoming datum again once |
//! | `space_joins_set_persistent_class_binds_twice` | a set class with a persistent member binds two positions, on both sides |
//! | `space_joins_set_free_class_binds_members_before_the_persistent_member` | the non-persistent members of a free class bind before its persistent member |
//! | `space_joins_cascade_binds_a_persistent_record_at_two_positions` | on a queue, the cascade gives a waiter its turn, and it binds a persistent record at two positions |
//!
//! `space_joins_queue_programs_run_as_joins` runs the five programs of the P4 test
//! `space_queue_join_rejected` (`reified_queue.rs`), which decision 29 disabled, as joins.
//!
//! Added by decision 102 (plan v4 C5.3): play picks at random among the records of the window of
//! a collection that the group of a join names once (rule CR-1), and among the join waiters of an
//! offer (decision 124, LP-1). Four tests check such picks: `space_joins_bag_greedy`,
//! `space_joins_set_bind_retires_class`, `space_joins_guard_checked_once_after_all_binds` and
//! `space_joins_produce_side_peek_reproduces_in_receipt_order`. Each part of a pick runs
//! [`PICK_RUNS`] times, each run on a fresh rig, and checks that its outcome is in the allowed set.
//! A coverage set checks that each allowed outcome occurs, and the doc of each test gives the miss
//! bound. A run that reads its event log plays on a [`PairRig`]. The parts that no pick touches
//! run once. Native replay of the join programs is certified by `space_joins_replay_equal`
//! (decision 90, C6, step D).

#[path = "support/mod.rs"]
mod support;

// Changed by decision 90 (C6): the native session holds the runtime and plays the deploys.
// use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::cost_signature::Value as CostSignatureValue;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{
    BindPattern, CostAuthority, CostRegion, CostSignature, EGt, EList, ELt, Expr,
    ListParWithRandom, Par, TaggedContinuation,
};
use models::rust::phlo_schedule::{PhloGenesisPolicy, PhloScheduleV1};
use models::rust::space_factory_urn::{SpaceFactoryUrn, SpaceInner, SpaceOuter};
use models::rust::utils::{new_boundvar_par, new_gbool_par};
use rholang::rust::interpreter::accounting::authority::AuthorityByteEventKind;
use rholang::rust::interpreter::accounting::byte_accounting::{
    consume_introduction_identity, produce_introduction_identity,
};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::native_phlo_rules::{
    native_resource_compatibility_rule, NativeBoundSource, NativeBudgetTraceLimits,
    NativePhloDimension, NativePhloExecutionContract, NativePhloRegionLimits,
};
use rholang::rust::interpreter::accounting::phlo_controls::{
    check_phlo_controls, PhloScheduleBinding, SignedPhloControls,
};
// Changed by decision 90 (C6): the native session holds the runtime and plays the deploys.
// use rholang::rust::interpreter::accounting::{
//     funding_sig_compound, NativeRuntimeConfig, RuntimeBudget, Sig,
// };
use rholang::rust::interpreter::accounting::{
    funding_sig_compound, NativeRuntimeConfig, RuntimeBudget,
};
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::reified::costs::{ReissueCause, FIRE_ROUND_CAP};
// Changed by decision 90 (decision 69): the module `reified::legacy_charges` of Option P is
// deleted with the legacy meter (DR-134).
// use rholang::rust::interpreter::reified::legacy_charges::{consume_charges, produce_charges};
use rholang::rust::interpreter::reified::runtime;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::spaces::agent::{
    CascadeState, ConsumeOp, JoinOp, OpOrigin, PendingProduce, ProduceOp, QueryParams, SettleOp,
    SpaceEnv, SpaceOp, SpaceOutcome, SpaceReissue,
};
use rholang::rust::interpreter::spaces::disciplines::priority_queue::PRIORITY_PARAM;
use rholang::rust::interpreter::spaces::disciplines::scan::{retire_continuation, retire_pattern};
use rholang::rust::interpreter::spaces::errors::SpaceError;
use rholang::rust::interpreter::spaces::families::FamilyAgent;
use rholang::rust::interpreter::spaces::namespace::{
    self, CollectionKey, Durability, NativeSpace, RandomPurpose, SID_LEN,
};
use rholang::rust::interpreter::spaces::record::{self, JoinSlot, WaiterPolicy};
use rholang::rust::interpreter::spaces::settle::{lift_continuation, lift_pattern};
use rholang::rust::interpreter::spaces::types::qualifier::SpaceQualifier;
// Disabled by decision 90 (C6): the native session runs no legacy deploy.
// use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::checkpoint::Checkpoint;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex;
use rspace_plus_plus::rspace::merger::merging_logic::{
    conflict_reason, conflicts, NumberChannelsDiff,
};
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
// Disabled by decision 90 (C6): the native session runs no legacy deploy.
// use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
// use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
// Changed by decision 90 (decision 69): only the retired lone host differential used these
// imports (see the note of `lone_calls_step`).
// use rspace_plus_plus::rspace::space_store_handle::{
//     canonical_data_order, ConsumeOutcome, ProduceOutcome,
// };
use rspace_plus_plus::rspace::space_store_handle::canonical_data_order;
use rspace_plus_plus::rspace::trace::event::{Consume, Event, IOEvent, Produce, COMM};
// Changed by decision 90 (decision 69): only the retired lone host differential used these
// imports (see the note of `lone_calls_step`).
// use support::agent::{
//     any, budget, config_of, counter_channel, create, creation, data_channel, env_over, exactly,
//     fired, int, join_group, join_on, join_waiters_on, joins_on, play, play_and_replay, play_with,
//     receive_on, run, send_on, string, AnyRecord, AnyWaiter, CountingMatcher, HostRecord,
//     JoinWaiterRow, ParRSpace, RecordingHost, SharedRecord, StagedCopy,
// };
// Changed by decision 90 (C6, step D): the rigged replay modes retire, so no test of this file
// takes a play and replay pair. The text before:
// ```
// use support::agent::{
//     any, budget, config_of, counter_channel, create, creation, data_channel, env_over, exactly,
//     fired, int, join_group, join_on, join_waiters_on, joins_on, play, play_and_replay, play_with,
//     receive_on, run, send_on, string, AnyRecord, AnyWaiter, CountingMatcher, HostRecord,
//     JoinWaiterRow, ParRSpace, RecordingHost, SharedRecord,
// };
// ```
use support::agent::{
    any, budget, config_of, counter_channel, create, creation, data_channel, env_over, exactly,
    fired, int, join_group, join_on, join_waiters_on, joins_on, play, play_with, receive_on, run,
    send_on, string, AnyRecord, AnyWaiter, CountingMatcher, HostRecord, JoinWaiterRow, ParRSpace,
    RecordingHost, SharedRecord,
};
use support::family_programs::space_program;
use support::native::{assert_reproduces, certify, native_play, AMPLE_LIMIT};
// Changed by decision 90 (decision 69): the lone host calls return observer calls, and the
// legacy charges of Option P are deleted with the legacy meter (DR-134).
// use support::legacy::{lone_host_consume, lone_host_produce, split};
// Changed by decision 90 (decision 69): only the retired lone host differential ran lone host
// calls (see the note of `lone_calls_step`).
use support::observed::{observed, Call, Recorder};
use support::props::{racing_runtime, RACING_WORKERS};
use support::session::{with_space, Session};

// ─── The rig ─────────────────────────────────────────────────────────────────────────────────

/// The space of every agent rig.
const SPACE: NativeSpace = NativeSpace {
    sid: [0x4A; SID_LEN],
    durability: Durability::Durable,
};

// Changed by decision 90 (decision 69): no budget of the suite is a legacy budget, because DR-134
// deleted the legacy meter (`STEP_PHLO`).
//
// /// The phlo of a legacy budget: far above every script of the suite.
// const LEGACY_PHLO: i64 = 1_000_000_000_000;

// Retired by decision 90 (C6, step D): the replay scripts of this suite were its only users, and
// they retire (decision 125).

/// The factory of the bag family.
const BAG: &str = "rho:space:hashmap:bag";

/// The factory of the queue family.
const QUEUE: &str = "rho:space:hashmap:queue";

/// The constructor arguments of a family without params.
const DEFAULT: &str = "\"default\"";

/// The public channel of the outputs of the receive bodies of the Rholang tests.
const OUT: &str = "rrs-join-out";

/// The family `hashmap:<inner>`.
fn family(inner: SpaceInner) -> SpaceFactoryUrn {
    SpaceFactoryUrn {
        outer: SpaceOuter::HashMap,
        inner,
    }
}

/// The inner channel of the collection named `name`.
fn ch(name: &str) -> Par { string(&format!("rrs-join-{name}")) }

/// The data channel `D(sid,k)` of the collection of `channel`.
fn data(channel: &Par) -> Par { data_channel(SPACE, channel) }

/// The key of the collection of `channel`.
fn key(channel: &Par) -> CollectionKey {
    CollectionKey::of_channel(channel).expect("a channel has a collection key")
}

/// Creates the space of `hashmap:<inner>` with the family agent in the environment `env`.
async fn create_space(env: &SpaceEnv, inner: SpaceInner) {
    let config = config_of(
        SPACE,
        SpaceOuter::HashMap.as_str(),
        inner.as_str(),
        SpaceQualifier::Default,
    );
    create(&FamilyAgent, env, creation(SPACE, config, family(inner))).await;
}

/// A space of one family over an in-memory play space, run by the family agent.
struct Rig {
    /// The play space.
    host: ParRSpace,
    /// The environment of the jobs, with the phlo budget that they charge.
    env: SpaceEnv,
}

impl Rig {
    /// A rig of `hashmap:<inner>` over a fresh play space.
    async fn new(inner: SpaceInner) -> Rig { Rig::over(play().await, inner, &budget()).await }

    /// A rig of `hashmap:<inner>` over `host`, a fresh space, whose jobs charge `budget`.
    async fn over(host: ParRSpace, inner: SpaceInner, budget: &RuntimeBudget) -> Rig {
        let env = env_over(host.clone(), budget);
        create_space(&env, inner).await;
        host.take_event_log().await;
        Rig { host, env }
    }

    /// A rig of `hashmap:<inner>` whose agent works over a recording host, and the record of
    /// that host. The record starts after the creation of the space.
    async fn recording(inner: SpaceInner, budget: &RuntimeBudget) -> (Rig, SharedRecord) {
        let host = play().await;
        let (recording, record) = RecordingHost::new(host.clone());
        let env = env_over(recording, budget);
        create_space(&env, inner).await;
        host.take_event_log().await;
        *record.lock().expect("the record lock") = HostRecord::default();
        (Rig { host, env }, record)
    }

    /// Runs `op` in one job.
    async fn run(&self, op: SpaceOp) -> Result<SpaceOutcome, SpaceError> {
        run(&FamilyAgent, &self.env, op).await
    }

    /// Runs `op`, which must commit.
    async fn ok(&self, op: SpaceOp) -> SpaceOutcome {
        let label = format!("{op:?}");
        self.run(op)
            .await
            .unwrap_or_else(|error| panic!("the operation commits: {error:?}: {label}"))
    }

    /// Runs `op` and then every re-issue that follows it, as the jobs of one deploy do, and
    /// returns the outcome of each job.
    async fn chain(&self, op: SpaceOp) -> Vec<SpaceOutcome> {
        let mut outcomes = Vec::new();
        let mut next = Some(op);
        while let Some(op) = next.take() {
            let outcome = self.ok(op).await;
            next = outcome.reissue.as_ref().map(|reissue| reissue.op.clone());
            outcomes.push(outcome);
        }
        outcomes
    }

    /// The events since the last call.
    async fn log(&self) -> Vec<Event> { self.host.take_event_log().await }

    /// The records of the collection of `channel`.
    async fn records(&self, channel: &Par) -> Vec<AnyRecord> {
        support::agent::any_records_on(&self.host, &data(channel)).await
    }

    /// The values of the records of the collection of `channel`, sorted.
    async fn values(&self, channel: &Par) -> Vec<i64> {
        support::agent::values_on(&self.host, &data(channel)).await
    }

    /// The single-bind waiters of the collection of `channel`.
    async fn waiters(&self, channel: &Par) -> Vec<AnyWaiter> {
        support::agent::any_waiters_on(&self.host, &data(channel)).await
    }

    /// The join groups of the collection of `channel`.
    async fn joins(&self, channel: &Par) -> Vec<Vec<Par>> {
        joins_on(&self.host, &data(channel)).await
    }

    /// The join waiters of the group of the bind channels `channels`.
    async fn join_waiters(&self, channels: &[&Par]) -> Vec<JoinWaiterRow> {
        join_waiters_on(&self.host, &join_group(SPACE, channels)).await
    }

    /// The value of the counter of the collection of `channel`.
    async fn counter(&self, channel: &Par) -> i64 {
        match self
            .host
            .get_data(&counter_channel(SPACE, channel))
            .await
            .as_slice()
        {
            [datum] => record::counter_value(&datum.a).expect("a counter record"),
            other => panic!("the counter channel holds {} records", other.len()),
        }
    }

    /// The same space for the jobs of another deploy, whose jobs charge `budget`.
    fn deploy(&self, budget: &RuntimeBudget) -> Rig {
        Rig {
            host: self.host.clone(),
            env: SpaceEnv {
                budget: budget.clone(),
                ..self.env.clone()
            },
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

// ─── Operations ──────────────────────────────────────────────────────────────────────────────

/// A send of `value` on `channel` whose random state the channel, the value and `salt` fix.
fn send_salted(channel: &Par, value: i64, salt: u64) -> ProduceOp {
    let mut op = send_on(SPACE, channel, value, false);
    op.data.random_state = Blake2b512Random::create_from_bytes(
        format!("rrs-join-send {channel:?} {value} {salt}").as_bytes(),
    )
    .to_bytes();
    op
}

/// A send of `value` on `channel`.
fn put(channel: &Par, value: i64) -> SpaceOp { SpaceOp::Produce(send_salted(channel, value, 0)) }

/// A send of `value` on `channel` with the salt `salt`.
fn put_salted(channel: &Par, value: i64, salt: u64) -> SpaceOp {
    SpaceOp::Produce(send_salted(channel, value, salt))
}

/// A persistent send (`!!`) of `value` on `channel`.
fn put_forever(channel: &Par, value: i64) -> SpaceOp {
    SpaceOp::Produce(ProduceOp {
        persistent: true,
        ..send_salted(channel, value, 1_000_000)
    })
}

/// The named params of a send or of a query with the level `level` of a priority queue.
fn priority(level: i64) -> BTreeMap<String, Par> {
    BTreeMap::from([(PRIORITY_PARAM.to_string(), int(level))])
}

/// A send of `value` on `channel` of a priority queue at the level `level`.
fn put_at(channel: &Par, value: i64, level: Option<i64>) -> SpaceOp {
    SpaceOp::Produce(ProduceOp {
        params: level.map(priority).unwrap_or_default(),
        ..send_salted(channel, value, 7)
    })
}

/// The query of a receive in the lane `lane` of a priority queue: `None` is the unrestricted
/// lane.
fn lane_query(lane: Option<i64>) -> Option<QueryParams> {
    lane.map(|level| QueryParams {
        payload: Vec::new(),
        named: priority(level),
    })
}

/// A single-bind receive on `channel` whose body is the string `name`.
fn take(channel: &Par, name: &str, pattern: BindPattern) -> SpaceOp {
    SpaceOp::Consume(receive_on(SPACE, channel, name, pattern))
}

/// A single-bind receive on `channel` of a priority queue in the lane `lane`.
fn take_in(channel: &Par, name: &str, pattern: BindPattern, lane: Option<i64>) -> SpaceOp {
    SpaceOp::Consume(ConsumeOp {
        query: lane_query(lane),
        ..receive_on(SPACE, channel, name, pattern)
    })
}

/// A join on the space whose binds are `binds`, in bind order, and whose body is `name`.
fn join(binds: &[(&Par, BindPattern)], name: &str) -> JoinOp { join_on(SPACE, binds, name) }

/// `op` as a space operation.
fn joined(op: JoinOp) -> SpaceOp { SpaceOp::Join(op) }

/// `op` with the arrow `<<-` on every bind.
fn peeking(op: JoinOp) -> JoinOp { JoinOp { peek: true, ..op } }

/// `op` with the arrow `<=` on every bind.
fn persisting(op: JoinOp) -> JoinOp {
    JoinOp {
        persistent: true,
        ..op
    }
}

/// `op` with the guard `guard`.
fn guarded(op: JoinOp, guard: Par) -> JoinOp {
    JoinOp {
        guard: Some(guard),
        ..op
    }
}

/// `op` with the lane `lanes[i]` on its bind `i` (a priority queue).
fn in_lanes(op: JoinOp, lanes: &[Option<i64>]) -> JoinOp {
    let mut op = op;
    for (bind, lane) in op.binds.iter_mut().zip(lanes) {
        bind.query = lane_query(*lane);
    }
    op
}

/// `op` with the random state of its body taken from `salt`. The body stays the same.
fn resalted(op: JoinOp, salt: u64) -> JoinOp {
    let mut op = op;
    op.body.random_state = Blake2b512Random::create_from_bytes(
        format!("rrs-join-body {:?} {salt}", op.body.body).as_bytes(),
    )
    .to_bytes();
    op
}

/// The bound variable `index` of a guard. A join can have one variable in each bind. Then the
/// variable of the last bind is `0`, and the variable of the first bind has the highest index.
fn bound(index: i32) -> Par { new_boundvar_par(index, Vec::new(), false) }

/// The guard `left > right`.
fn gt(left: Par, right: Par) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EGtBody(EGt {
                p1: Some(left),
                p2: Some(right),
            })),
        }],
        ..Par::default()
    }
}

/// The guard `left < right`.
fn lt(left: Par, right: Par) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::ELtBody(ELt {
                p1: Some(left),
                p2: Some(right),
            })),
        }],
        ..Par::default()
    }
}

/// The guard that refuses every match.
fn refusing() -> Par { new_gbool_par(false, Vec::new(), false) }

/// A fire of the body `name` with the bound values `values`, as `fired` reports it.
fn delivery(name: &str, values: &[i64]) -> (String, Vec<Par>) {
    (
        name.to_string(),
        values.iter().map(|value| int(*value)).collect(),
    )
}

/// A signed cost authority of one region.
fn region(byte: u8) -> CostAuthority {
    CostAuthority {
        regions: vec![CostRegion {
            instance_id: vec![byte; 32],
            signature: Some(CostSignature {
                value: Some(CostSignatureValue::Ground(vec![byte])),
            }),
            // Added by decision 90 (DR-135): an unpriced region, as outside native
            // execution.
            price: 0,
            caller: None,
        }],
    }
}

// ─── The canonical orders of DR-75 ───────────────────────────────────────────────────────────

/// The source hash of the record of the send `op` in a bag or a set, whose orders are 0.
fn record_hash(op: &ProduceOp) -> Blake2b256Hash {
    Produce::create(
        &data(&op.channel),
        &record::wrap_datum(0, &op.data),
        op.persistent,
    )
    .hash
}

/// Sends of `(value, persistent)` on `channel` whose records follow each other in the canonical
/// order of a bag or a set. The salts are searched in a fixed order, so every run makes the same
/// sends.
fn ordered_sends(channel: &Par, entries: &[(i64, bool)]) -> Vec<ProduceOp> {
    let mut sends: Vec<ProduceOp> = Vec::with_capacity(entries.len());
    let mut salt = 0;
    for (value, persistent) in entries {
        loop {
            salt += 1;
            let candidate = ProduceOp {
                persistent: *persistent,
                ..send_salted(channel, *value, salt)
            };
            if sends
                .last()
                .is_none_or(|last| record_hash(last) < record_hash(&candidate))
            {
                sends.push(candidate);
                break;
            }
        }
    }
    sends
}

/// The values of the records of the collection of `channel` in the canonical order of DR-75.
async fn canonical_values(host: &ParRSpace, channel: &Par) -> Vec<i64> {
    let records = host.get_data(&data(channel)).await;
    canonical_data_order(&records)
        .into_iter()
        .map(|index| number_of(&records[index].a))
        .collect()
}

/// The number of a `GInt` par.
fn number(par: &Par) -> i64 {
    match par.exprs.as_slice() {
        [Expr {
            expr_instance: Some(ExprInstance::GInt(value)),
        }] => *value,
        other => panic!("a par of one number: {other:?}"),
    }
}

/// The number of a datum record whose payload is one number.
fn number_of(record_bytes: &ListParWithRandom) -> i64 {
    let decoded = record::unwrap_pq_datum(record_bytes).expect("a datum record");
    match decoded.datum.pars.as_slice() {
        [par] => number(par),
        other => panic!("a payload of one number: {other:?}"),
    }
}

/// The slots of a join on the space with the bind channels `channels`, in group order and
/// without lanes. The collection of bind `i` has the order `ords[i]`.
fn slots_of(channels: &[&Par], ords: &[i64]) -> Vec<JoinSlot> {
    support::agent::group_positions(SPACE, channels)
        .into_iter()
        .map(|bind| JoinSlot {
            ord: ords[bind],
            bind,
            policy: WaiterPolicy::None,
        })
        .collect()
}

/// The continuation of the join `op` as the program gives it.
fn join_body(op: &JoinOp) -> TaggedContinuation {
    TaggedContinuation {
        tagged_cont: Some(TaggedCont::ParBody(op.body.clone())),
        guard: op.guard.clone(),
        cost_authority: record::cost_authority(&op.authority),
    }
}

/// The source hash of the parked waiter of the join `op` in a bag or a set. The hash covers the
/// group, the patterns in group order, and the record of version 2 with the orders 0.
fn join_waiter_hash(op: &JoinOp) -> Blake2b256Hash {
    let channels: Vec<&Par> = op.binds.iter().map(|bind| &bind.channel).collect();
    let positions = support::agent::group_positions(SPACE, &channels);
    let group: Vec<Par> = positions
        .iter()
        .map(|bind| data(&op.binds[*bind].channel))
        .collect();
    let patterns: Vec<BindPattern> = positions
        .iter()
        .map(|bind| op.binds[*bind].pattern.clone())
        .collect();
    let zeros = vec![0; op.binds.len()];
    let parked = record::wrap_join_continuation(&slots_of(&channels, &zeros), &join_body(op))
        .expect("a join has a body");
    Consume::create(&group, &patterns, &parked, op.persistent).hash
}

/// The joins `ops` with salted random states of their bodies. With the salts, their waiters
/// follow each other in the canonical order of a bag or a set. The search of the salts has a
/// fixed order.
fn ordered_joins(ops: Vec<JoinOp>) -> Vec<JoinOp> {
    let mut joins: Vec<JoinOp> = Vec::with_capacity(ops.len());
    let mut salt = 0;
    for op in ops {
        loop {
            salt += 1;
            let candidate = resalted(op.clone(), salt);
            if joins
                .last()
                .is_none_or(|last| join_waiter_hash(last) < join_waiter_hash(&candidate))
            {
                joins.push(candidate);
                break;
            }
        }
    }
    joins
}

/// Two inner channels whose group order reverses their bind order: the data channel of the
/// first has the greater stable hash.
fn reversed_pair() -> (Par, Par) {
    for index in 0..64 {
        let (first, second) = (
            ch(&format!("order-{index}")),
            ch(&format!("order-{index}-b")),
        );
        if support::agent::group_positions(SPACE, &[&first, &second]) == [1, 0] {
            return (first, second);
        }
    }
    panic!("no pair of the 64 candidates reverses its bind order");
}

// ─── Events ──────────────────────────────────────────────────────────────────────────────────

/// The consume events of `log` on the channel `channel` alone.
fn consumes_on(log: &[Event], channel: &Par) -> Vec<Consume> {
    let key = rspace_plus_plus::rspace::hashing::stable_hash_provider::hash(channel);
    log.iter()
        .filter_map(|event| match event {
            Event::IoEvent(IOEvent::Consume(consume))
                if consume.channel_hashes.as_slice() == std::slice::from_ref(&key) =>
            {
                Some(consume.clone())
            }
            _ => None,
        })
        .collect()
}

/// The produce events of `log` on `channel`.
fn produces_on(log: &[Event], channel: &Par) -> Vec<Produce> {
    let key = rspace_plus_plus::rspace::hashing::stable_hash_provider::hash(channel);
    log.iter()
        .filter_map(|event| match event {
            Event::IoEvent(IOEvent::Produce(produce)) if produce.channel_hash == key => {
                Some(produce.clone())
            }
            _ => None,
        })
        .collect()
}

/// The consume of `K_retire` on the collection of `channel` for the stored member with the
/// source hash `member`. The element of the fire has the random state bytes `random_state` and
/// the cost authority `authority`.
fn retire_consume(
    channel: &Par,
    random_state: &[u8],
    authority: Option<CostAuthority>,
    member: &Blake2b256Hash,
) -> Consume {
    let random = RandomPurpose::Retire.derive(&Blake2b512Random::from_bytes(random_state));
    Consume::create(
        &vec![data(channel)],
        &vec![retire_pattern()],
        &retire_continuation(member, &random, authority),
        false,
    )
}

/// The consume of `K_lift` on the collection of `channel` with the counter value `counter` and
/// the step `step`. The operation has the random state bytes `random_state` and no cost
/// authority.
fn lift_consume(channel: &Par, counter: i64, step: i64, random_state: &[u8]) -> Consume {
    let random = RandomPurpose::Lift.derive(&Blake2b512Random::from_bytes(random_state));
    Consume::create(
        &vec![data(channel)],
        &vec![lift_pattern()],
        &lift_continuation(counter, step, &random, None),
        false,
    )
}

/// The introduction authority that `budget` registered for the produce `produce`.
fn registered(budget: &RuntimeBudget, produce: &Produce) -> Option<CostAuthority> {
    budget
        .introduction_authority(
            produce_introduction_identity(produce),
            AuthorityByteEventKind::ProduceIntroduction,
        )
        .ok()
}

/// The introduction authority that `budget` registered for the consume `consume`.
fn registered_consume(budget: &RuntimeBudget, consume: &Consume) -> Option<CostAuthority> {
    budget
        .introduction_authority(
            consume_introduction_identity(consume),
            AuthorityByteEventKind::ConsumeIntroduction,
        )
        .ok()
}

// ─── Random picks and the replay check (decision 102, rules CR-1 and CR-4) ───────────────────

/// Added by decision 102 (plan v4 C5.3): the runs of a play test of a random pick, each on a
/// fresh rig. An outcome with a chance of `p` or more is missing from all runs with a chance of
/// `(1 - p)^64` or less. So one of two outcomes with a chance of 1/2 each is missing with a chance
/// of `2·2^-64 = 2^-63`. One of three outcomes with a chance of 1/3 each is missing with a chance
/// of `3·(2/3)^64`, which is less than `2·10^-11`.
const PICK_RUNS: usize = 64;

/// Added by decision 102 (plan v4 C5.3, rule CR-4): a rig over a fresh play space. Its first
/// checkpoint empties the event log of the creation, so the log of a test holds every event of
/// the test.
struct PairRig {
    /// The rig over the play space.
    rig: Rig,
}

impl PairRig {
    /// A rig of `hashmap:<inner>` over a fresh play space.
    async fn new(inner: SpaceInner) -> PairRig {
        let play_space = play().await;
        let rig = Rig::over(play_space, inner, &budget()).await;
        // The checkpoint empties the event log of the creation.
        rig.root().await;
        PairRig { rig }
    }

    /// Runs `op`, which must commit.
    async fn ok(&mut self, op: SpaceOp) -> SpaceOutcome { self.rig.ok(op).await }

    /// The log of play of the operations so far.
    async fn play_log(&self) -> Vec<Event> {
        let Checkpoint { log, .. } = self
            .rig
            .host
            .create_checkpoint()
            .await
            .expect("a checkpoint of play");
        log
    }

    // Retired by decision 90 (C6, step D): the replay half ran through the rigged replay modes,
    // which retire (decision 125). `space_joins_replay_equal`,
    // `space_pick_helpers_put_the_named_candidates_first_in_replay`.
}

/// Added by decision 102 (plan v4 C5.3): the source hashes of the records on `channel` that the
/// join COMMs of `log` bind, in log order.
fn join_binds_on(log: &[Event], channel: &Par) -> Vec<Blake2b256Hash> {
    let key = rspace_plus_plus::rspace::hashing::stable_hash_provider::hash(channel);
    log.iter()
        .filter_map(|event| match event {
            Event::Comm(comm) if comm.consume.channel_hashes.len() >= 2 => Some(comm),
            _ => None,
        })
        .flat_map(|comm| comm.produces.iter())
        .filter(|produce| produce.channel_hash == key)
        .map(|produce| produce.hash.clone())
        .collect()
}

// ─── 7.1: windows and searches ───────────────────────────────────────────────────────────────

// Changed by decision 102 (plan v4 C5.3, rule CR-1): play picks the window of a collection that
// the group names once at random, so the doc below replaces this one.
// /// GREEDY on a bag (joins plan 3.4, decision 37). Each bind takes the first record of its
// /// window that its pattern binds, in the canonical order of DR-75. There is no backtracking.
// ///
// /// * Two binds on two bags take the first record of each bag in canonical order.
// /// * A literal pattern skips the records that it does not bind: the first spatial match wins.
// /// * The computed greedy limit: the join `[c, c]` has the patterns `any` and `1`, and the records
// ///   1 and 2 come in this canonical order. Greedy binds 1 to the first bind and finds no 1 for
// ///   the second bind. So the join parks, although the assignment of 2 and 1 exists. With 2
// ///   first in canonical order, greedy finds that assignment and fires.
/// GREEDY on a bag (joins plan 3.4, decision 37). Each bind takes the first record of its
/// window that its pattern binds. There is no backtracking. Play picks the window of a collection
/// that the group names once at random (decision 102, rule CR-1). A collection that the group
/// names two or more times keeps the canonical order of DR-75 (E1, rule CR-8).
///
/// * Two binds on two bags take a random record of each bag. This part runs [`PICK_RUNS`] times,
///   each run on a fresh rig. Each run checks that `g` binds a record of each bag. Over the runs,
///   `g` binds each of the 3 records of `a` and each of the 2 records of
///   `b`. One of these outcomes is missing with a chance of `3·(2/3)^64 + 2·2^-64`, which is less
///   than `2·10^-11`.
/// * A literal pattern binds only the record that it matches. So `h` binds the other record of
///   `b`, and `a` keeps the record that neither join binds.
/// * The computed greedy limit: the join `[c, c]` has the patterns `any` and `1`, and the records
///   1 and 2 come in this canonical order. Greedy binds 1 to the first bind and finds no 1 for
///   the second bind. So the join parks, although the assignment of 2 and 1 exists. With 2
///   first in canonical order, greedy finds that assignment and fires.
#[tokio::test]
async fn space_joins_bag_greedy() {
    let rig = Rig::new(SpaceInner::Bag).await;
    let (a, b) = (ch("a"), ch("b"));
    // Changed by decision 102 (plan v4 C5.3, rule CR-1): play picks the records of g at random.
    // So this part runs PICK_RUNS times, each run on a fresh rig with its replay, and checks the
    // allowed set and a coverage set. The body before:
    // for value in [1, 2, 3] {
    //     rig.ok(put_salted(&a, value, 1)).await;
    // }
    // for value in [10, 20] {
    //     rig.ok(put(&b, value)).await;
    // }
    // let order_a = canonical_values(&rig.host, &a).await;
    // let order_b = canonical_values(&rig.host, &b).await;
    // let outcome = rig.ok(joined(join(&[(&a, any()), (&b, any())], "g"))).await;
    // assert_eq!(fired(&outcome), vec![delivery("g", &[
    //     order_a[0], order_b[0]
    // ])]);
    // let rest = canonical_values(&rig.host, &a).await;
    // assert_eq!(rest, order_a[1..].to_vec());
    // let target = rest[1];
    // let outcome = rig
    //     .ok(joined(join(&[(&a, exactly(target)), (&b, any())], "h")))
    //     .await;
    // assert_eq!(
    //     fired(&outcome),
    //     vec![delivery("h", &[order_b[1]])],
    //     "the literal skips the first record of the bag"
    // );
    // assert_eq!(canonical_values(&rig.host, &a).await, vec![rest[0]]);
    let mut bound_a = BTreeSet::<i64>::new();
    let mut bound_b = BTreeSet::<i64>::new();
    for _ in 0..PICK_RUNS {
        let mut pair = PairRig::new(SpaceInner::Bag).await;
        for value in [1, 2, 3] {
            pair.ok(put_salted(&a, value, 1)).await;
        }
        for value in [10, 20] {
            pair.ok(put(&b, value)).await;
        }
        let outcome = pair
            .ok(joined(join(&[(&a, any()), (&b, any())], "g")))
            .await;
        let [(name, values)] =
            <[(String, Vec<Par>); 1]>::try_from(fired(&outcome)).expect("one fire of the join");
        assert_eq!(name, "g");
        let [value_a, value_b] =
            <[i64; 2]>::try_from(values.iter().map(number).collect::<Vec<_>>())
                .expect("g binds two values");
        assert!(
            [1, 2, 3].contains(&value_a) && [10, 20].contains(&value_b),
            "g binds a record of each bag, not {value_a} and {value_b}"
        );
        bound_a.insert(value_a);
        bound_b.insert(value_b);
        let unbound_a: Vec<i64> = [1, 2, 3]
            .into_iter()
            .filter(|value| *value != value_a)
            .collect();
        assert_eq!(
            pair.rig.values(&a).await,
            unbound_a,
            "a keeps the records that g does not bind"
        );
        let unbound_b: Vec<i64> = [10, 20]
            .into_iter()
            .filter(|value| *value != value_b)
            .collect();
        let rest = canonical_values(&pair.rig.host, &a).await;
        let target = rest[1];
        let outcome = pair
            .ok(joined(join(&[(&a, exactly(target)), (&b, any())], "h")))
            .await;
        assert_eq!(
            fired(&outcome),
            vec![delivery("h", &unbound_b)],
            "the literal binds only the record that it matches"
        );
        assert_eq!(canonical_values(&pair.rig.host, &a).await, vec![rest[0]]);
        // Retired by decision 90 (C6, step D): the replay over the rigged replay modes, which
        // retire.
    }
    assert_eq!(
        bound_a,
        BTreeSet::from([1, 2, 3]),
        "g binds each record of a in some run"
    );
    assert_eq!(
        bound_b,
        BTreeSet::from([10, 20]),
        "g binds each record of b in some run"
    );

    // The greedy limit.
    let c = ch("c");
    for send in ordered_sends(&c, &[(1, false), (2, false)]) {
        rig.ok(SpaceOp::Produce(send)).await;
    }
    assert_eq!(canonical_values(&rig.host, &c).await, vec![1, 2]);
    let parked = rig
        .ok(joined(join(&[(&c, any()), (&c, exactly(1))], "limit")))
        .await;
    assert!(
        parked.fires.is_empty(),
        "greedy binds 1 first and finds no 1 for the second bind"
    );
    assert_eq!(rig.values(&c).await, vec![1, 2]);
    assert_eq!(rig.join_waiters(&[&c, &c]).await.len(), 1);
    let payload = |value: i64| ListParWithRandom {
        pars: vec![int(value)],
        ..ListParWithRandom::default()
    };
    assert!(
        Matcher.get(&any(), &payload(2)).is_some()
            && Matcher.get(&exactly(1), &payload(1)).is_some(),
        "the assignment of 2 and 1 exists"
    );
    let d = ch("d");
    for send in ordered_sends(&d, &[(2, false), (1, false)]) {
        rig.ok(SpaceOp::Produce(send)).await;
    }
    let control = rig
        .ok(joined(join(&[(&d, any()), (&d, exactly(1))], "control")))
        .await;
    assert_eq!(fired(&control), vec![delivery("control", &[2])]);
    assert!(rig.values(&d).await.is_empty());
}

/// Set windows (joins plan 3.2): a bind claims a whole class. So two binds on one set take two
/// classes, and no class is bound twice. The fire retires the other members of each bound class
/// (joins plan 4.7).
#[tokio::test]
async fn space_joins_set_classes() {
    let rig = Rig::new(SpaceInner::Set).await;
    let a = ch("a");
    for (value, salt) in [(1, 1), (1, 2), (2, 1)] {
        rig.ok(put_salted(&a, value, salt)).await;
    }
    let outcome = rig
        .ok(joined(join(&[(&a, any()), (&a, any())], "two")))
        .await;
    let [(name, values)] =
        <[(String, Vec<Par>); 1]>::try_from(fired(&outcome)).expect("one fire of the join");
    assert_eq!(name, "two");
    let mut values: Vec<i64> = values.iter().map(number).collect();
    values.sort();
    assert_eq!(values, vec![1, 2], "the binds take two classes");
    assert!(
        rig.values(&a).await.is_empty(),
        "the second member of the class of 1 is retired"
    );

    let b = ch("b");
    for salt in [1, 2] {
        rig.ok(put_salted(&b, 5, salt)).await;
    }
    let parked = rig
        .ok(joined(join(&[(&b, any()), (&b, any())], "once")))
        .await;
    assert!(parked.fires.is_empty(), "one class never binds two binds");
    assert_eq!(rig.values(&b).await, vec![5, 5]);
    let outcome = rig.ok(put_salted(&b, 6, 1)).await;
    assert_eq!(fired(&outcome), vec![delivery("once", &[6, 5])]);
    assert!(
        rig.values(&b).await.is_empty(),
        "the incoming 6 and the class of 5 leave the set"
    );
}

// Changed by decision 102 (plan v4 C5.3, rule CR-1): play picks the bound member of a class at
// random, so the doc below replaces this one.
// /// RETIRE-SET-BINDS (joins plan 4.7). After a fire that does not peek, the other stored members
// /// of each bound class leave with single-channel `K_retire` COMMs, unless they are persistent.
// /// The consume side retires with the random state of the join. The produce side retires with the
// /// random state of the incoming element. A persistent member keeps its class and stays.
/// RETIRE-SET-BINDS (joins plan 4.7). After a fire that does not peek, the other stored members
/// of each bound class leave with single-channel `K_retire` COMMs in canonical order, unless they
/// are persistent. The consume side retires with the random state of the join. The produce side
/// retires with the random state of the incoming element. A persistent member keeps its class and
/// stays.
///
/// Play picks the bound member of a class at random (decision 102, rule CR-1). So the test runs
/// [`PICK_RUNS`] times, each run on a fresh rig. Each run reads the bound member from the COMM of
/// each fire and checks the `K_retire` COMMs of the other members. When the
/// join binds the persistent member, both other members retire. Over the runs, the join binds
/// each of the 3 members on the consume side and each of the 2 members on the produce side. One
/// of these outcomes is missing with a chance of `3·(2/3)^64 + 2·2^-64`, which is less than
/// `2·10^-11`.
#[tokio::test]
async fn space_joins_set_bind_retires_class() {
    // Changed by decision 102 (plan v4 C5.3, rule CR-1): play picks the bound member of each class
    // at random. So the test runs PICK_RUNS times, each run on a fresh rig with its replay, and
    // reads the bound members from the join COMMs of the play log. The body before:
    // let rig = Rig::new(SpaceInner::Set).await;
    // let (a, b) = (ch("a"), ch("b"));
    // let members = ordered_sends(&a, &[(5, false), (5, false), (5, true)]);
    // for member in &members {
    //     rig.ok(SpaceOp::Produce(member.clone())).await;
    // }
    // rig.ok(put(&b, 7)).await;
    // rig.log().await;
    // let op = join(&[(&a, exactly(5)), (&b, any())], "j");
    // let outcome = rig.ok(joined(op.clone())).await;
    // assert_eq!(fired(&outcome), vec![delivery("j", &[7])]);
    // let log = rig.log().await;
    // assert_eq!(
    //     consumes_on(&log, &data(&a)),
    //     vec![retire_consume(
    //         &a,
    //         &op.body.random_state,
    //         None,
    //         &record_hash(&members[1])
    //     )],
    //     "one K_retire retires the second member, with the random state of the join"
    // );
    // assert_eq!(
    //     rig.records(&a).await,
    //     vec![(0, None, vec![int(5)], true)],
    //     "the persistent member keeps its class"
    // );
    // rig.ok(put(&b, 8)).await;
    // let again = rig
    //     .ok(joined(join(&[(&a, exactly(5)), (&b, any())], "k")))
    //     .await;
    // assert_eq!(fired(&again), vec![delivery("k", &[8])]);
    // assert_eq!(rig.records(&a).await, vec![(0, None, vec![int(5)], true)]);
    //
    // // The produce side: the incoming 9 fires the waiter. The second member of the class of 3
    // // retires with the random state of the incoming element.
    // let (c, d) = (ch("c"), ch("d"));
    // let threes = ordered_sends(&c, &[(3, false), (3, false)]);
    // for member in &threes {
    //     rig.ok(SpaceOp::Produce(member.clone())).await;
    // }
    // let parked = rig
    //     .ok(joined(join(&[(&c, exactly(3)), (&d, any())], "w")))
    //     .await;
    // assert!(parked.fires.is_empty());
    // rig.log().await;
    // let incoming = send_salted(&d, 9, 1);
    // let outcome = rig.ok(SpaceOp::Produce(incoming.clone())).await;
    // assert_eq!(fired(&outcome), vec![delivery("w", &[9])]);
    // assert_eq!(consumes_on(&rig.log().await, &data(&c)), vec![
    //     retire_consume(
    //         &c,
    //         &incoming.data.random_state,
    //         None,
    //         &record_hash(&threes[1])
    //     )
    // ]);
    // assert!(rig.values(&c).await.is_empty());
    // assert!(rig.values(&d).await.is_empty());
    let (a, b) = (ch("a"), ch("b"));
    let members = ordered_sends(&a, &[(5, false), (5, false), (5, true)]);
    let op = join(&[(&a, exactly(5)), (&b, any())], "j");
    let (c, d) = (ch("c"), ch("d"));
    let threes = ordered_sends(&c, &[(3, false), (3, false)]);
    let incoming = send_salted(&d, 9, 1);
    let mut bound_fives = BTreeSet::<usize>::new();
    let mut bound_threes = BTreeSet::<usize>::new();
    for _ in 0..PICK_RUNS {
        let mut pair = PairRig::new(SpaceInner::Set).await;
        for member in &members {
            pair.ok(SpaceOp::Produce(member.clone())).await;
        }
        pair.ok(put(&b, 7)).await;
        let outcome = pair.ok(joined(op.clone())).await;
        assert_eq!(fired(&outcome), vec![delivery("j", &[7])]);
        assert_eq!(
            pair.rig.records(&a).await,
            vec![(0, None, vec![int(5)], true)],
            "the persistent member keeps its class"
        );
        pair.ok(put(&b, 8)).await;
        let again = pair
            .ok(joined(join(&[(&a, exactly(5)), (&b, any())], "k")))
            .await;
        assert_eq!(fired(&again), vec![delivery("k", &[8])]);
        assert_eq!(
            pair.rig.records(&a).await,
            vec![(0, None, vec![int(5)], true)],
            "the persistent member stays after k"
        );

        // The produce side: the incoming 9 fires the waiter. The other member of the class of 3
        // retires with the random state of the incoming element.
        for member in &threes {
            pair.ok(SpaceOp::Produce(member.clone())).await;
        }
        let parked = pair
            .ok(joined(join(&[(&c, exactly(3)), (&d, any())], "w")))
            .await;
        assert!(parked.fires.is_empty());
        let outcome = pair.ok(SpaceOp::Produce(incoming.clone())).await;
        assert_eq!(fired(&outcome), vec![delivery("w", &[9])]);
        assert!(pair.rig.values(&c).await.is_empty());
        assert!(pair.rig.values(&d).await.is_empty());

        // The COMMs of the fires name the bound members: j and k on a, and w on c.
        let log = pair.play_log().await;
        let [bound_j, bound_k] = <[Blake2b256Hash; 2]>::try_from(join_binds_on(&log, &data(&a)))
            .expect("j and k each bind one member of the class of 5");
        assert_eq!(
            bound_k,
            record_hash(&members[2]),
            "k binds the persistent member"
        );
        let bound = members
            .iter()
            .position(|member| record_hash(member) == bound_j)
            .expect("j binds a member of the class of 5");
        // Each other member that is not persistent retires in canonical order, with the random
        // state of the join. When j binds the persistent member, both other members retire.
        let retired: &[usize] = match bound {
            0 => &[1],
            1 => &[0],
            2 => &[0, 1],
            other => panic!("the class of 5 has three members, not {other}"),
        };
        let expected: Vec<Consume> = retired
            .iter()
            .map(|index| {
                retire_consume(
                    &a,
                    &op.body.random_state,
                    None,
                    &record_hash(&members[*index]),
                )
            })
            .collect();
        assert_eq!(
            consumes_on(&log, &data(&a)),
            expected,
            "j binds the member {bound}, and the other members that are not persistent retire"
        );
        bound_fives.insert(bound);
        let [bound_w] = <[Blake2b256Hash; 1]>::try_from(join_binds_on(&log, &data(&c)))
            .expect("w binds one member of the class of 3");
        let bound = threes
            .iter()
            .position(|member| record_hash(member) == bound_w)
            .expect("w binds a member of the class of 3");
        let other = match bound {
            0 => 1,
            1 => 0,
            other => panic!("the class of 3 has two members, not {other}"),
        };
        assert_eq!(
            consumes_on(&log, &data(&c)),
            vec![retire_consume(
                &c,
                &incoming.data.random_state,
                None,
                &record_hash(&threes[other])
            )],
            "w binds the member {bound}, and the other member retires"
        );
        bound_threes.insert(bound);
    }
    assert_eq!(
        bound_fives,
        BTreeSet::from([0, 1, 2]),
        "j binds each member of the class of 5 in some run"
    );
    assert_eq!(
        bound_threes,
        BTreeSet::from([0, 1]),
        "w binds each member of the class of 3 in some run"
    );
}

// Changed by decision 92: a failed join commits its anchor only, so the doc below replaces this
// one.
// /// A join over cells (F15): a cell holds one record and one waiter. A join over two cells parks
// /// and fires. A second waiter on a cell fails with `CellOccupied` before it stages anything. A
// /// join `[c, c]` needs two records. So it fires with the incoming record and the stored record,
// /// because the incoming record reaches the head of a cell.
/// A join over cells (F15): a cell holds one record and one waiter. A join over two cells parks
/// and fires. A second waiter on a cell fails with `CellOccupied`, and its transaction commits its
/// anchor only (decision 92). A join `[c, c]` needs two records. So it fires with the incoming
/// record and the stored record, because the incoming record reaches the head of a cell.
#[tokio::test]
async fn space_joins_cell_join() {
    let rig = Rig::new(SpaceInner::Cell).await;
    let (a, b, c) = (ch("a"), ch("b"), ch("c"));
    let parked = rig.ok(joined(join(&[(&a, any()), (&b, any())], "w"))).await;
    assert!(parked.fires.is_empty());
    assert_eq!(rig.joins(&a).await, vec![join_group(SPACE, &[&a, &b])]);
    rig.log().await;
    assert_eq!(
        rig.run(joined(join(&[(&a, any()), (&c, any())], "second")))
            .await,
        Err(SpaceError::CellOccupied)
    );
    // Changed by decision 92: the failed join commits its anchor only.
    // assert!(rig.log().await.is_empty(), "the failed join stages nothing");
    assert!(
        support::agent::logs_anchors_only(&rig.log().await, SPACE, 1),
        "the failed join logs its anchor only"
    );
    assert_eq!(rig.join_waiters(&[&a, &b]).await.len(), 1);
    assert!(rig.ok(put(&a, 1)).await.fires.is_empty());
    let outcome = rig.ok(put(&b, 2)).await;
    assert_eq!(fired(&outcome), vec![delivery("w", &[1, 2])]);
    assert!(rig.records(&a).await.is_empty() && rig.records(&b).await.is_empty());
    assert!(rig.joins(&a).await.is_empty());

    let e = ch("e");
    rig.ok(put(&e, 3)).await;
    let pair = rig.ok(joined(join(&[(&e, any()), (&e, any())], "u"))).await;
    assert!(pair.fires.is_empty(), "one record cannot bind two binds");
    let outcome = rig.ok(put(&e, 4)).await;
    assert_eq!(fired(&outcome), vec![delivery("u", &[4, 3])]);
    assert!(rig.records(&e).await.is_empty());
    assert!(rig.join_waiters(&[&e, &e]).await.is_empty());
}

/// Decision 11 for each collection of a join (joins plan 3.3). A join waiter fires only as the
/// head waiter of each queue of its group. A first record of a queue that does not match blocks
/// the join.
#[tokio::test]
async fn space_joins_queue_heads_only() {
    let rig = Rig::new(SpaceInner::Queue).await;
    let (a, b) = (ch("a"), ch("b"));
    assert!(rig.ok(take(&a, "v", any())).await.fires.is_empty());
    let parked = rig.ok(joined(join(&[(&a, any()), (&b, any())], "w"))).await;
    assert!(parked.fires.is_empty(), "w waits behind v on a");
    assert!(
        rig.ok(put(&b, 2)).await.fires.is_empty(),
        "w is not the head waiter of a"
    );
    assert_eq!(rig.values(&b).await, vec![2]);
    assert_eq!(fired(&rig.ok(put(&a, 1)).await), vec![delivery("v", &[1])]);
    assert_eq!(
        rig.join_waiters(&[&a, &b]).await.len(),
        1,
        "the cascade leaves w: a holds no record"
    );
    assert_eq!(fired(&rig.ok(put(&a, 3)).await), vec![delivery("w", &[
        3, 2
    ])]);
    assert!(rig.values(&a).await.is_empty() && rig.values(&b).await.is_empty());

    // The head record of c does not match, so the join blocks. As the head waiter of c, the join
    // also blocks a later receive of any value.
    let (c, d) = (ch("c"), ch("d"));
    for value in [1, 2] {
        rig.ok(put(&c, value)).await;
    }
    rig.ok(put(&d, 5)).await;
    let blocked = rig
        .ok(joined(join(&[(&c, exactly(2)), (&d, any())], "x")))
        .await;
    assert!(blocked.fires.is_empty(), "the head 1 does not match 2");
    let behind = rig.ok(take(&c, "y", any())).await;
    assert!(behind.fires.is_empty(), "the join is the head waiter of c");
    assert_eq!(rig.values(&c).await, vec![1, 2]);
    assert_eq!(rig.values(&d).await, vec![5]);
}

/// Decision 11 on stacks: only the tops interact. A fresh join is the top waiter of each stack
/// of its group, so it fires before an older waiter. A top record that does not match blocks.
#[tokio::test]
async fn space_joins_stack_heads_only() {
    let rig = Rig::new(SpaceInner::Stack).await;
    let (a, b) = (ch("a"), ch("b"));
    assert!(rig.ok(take(&a, "v", any())).await.fires.is_empty());
    rig.ok(put(&b, 3)).await;
    let parked = rig.ok(joined(join(&[(&a, any()), (&b, any())], "w"))).await;
    assert!(parked.fires.is_empty());
    assert_eq!(
        fired(&rig.ok(put(&a, 5)).await),
        vec![delivery("w", &[5, 3])],
        "the fresh join is the top waiter of a"
    );
    assert_eq!(fired(&rig.ok(put(&a, 6)).await), vec![delivery("v", &[6])]);

    let (c, d) = (ch("c"), ch("d"));
    for value in [1, 2] {
        rig.ok(put(&c, value)).await;
    }
    rig.ok(put(&d, 4)).await;
    let blocked = rig
        .ok(joined(join(&[(&c, exactly(1)), (&d, any())], "x")))
        .await;
    assert!(blocked.fires.is_empty(), "the top 2 does not match 1");
    let outcome = rig.ok(put(&c, 1)).await;
    assert_eq!(
        fired(&outcome),
        vec![delivery("x", &[4])],
        "a fresh 1 is the top of c"
    );
    assert_eq!(rig.values(&c).await, vec![1, 2]);
    assert!(rig.values(&d).await.is_empty());
}

/// Decision 38 in joins (joins plan 3.2, 3.3): each bind of a priority queue has the lane of its
/// query. A bind with `priority = p` sees the level `p` only, and a bind without a priority sees
/// every level. When a record can fire waiters of two lanes, the older waiter fires first, for a
/// join and for a single-bind waiter alike.
#[tokio::test]
async fn space_joins_pq_heads_only() {
    let rig = Rig::new(SpaceInner::PriorityQueue).await;
    let (a, b) = (ch("a"), ch("b"));
    rig.ok(put_at(&a, 10, Some(0))).await;
    rig.ok(put_at(&a, 11, Some(1))).await;
    rig.ok(put_at(&b, 20, None)).await;
    let outcome = rig
        .ok(joined(in_lanes(join(&[(&a, any()), (&b, any())], "w"), &[
            Some(1),
            None,
        ])))
        .await;
    assert_eq!(
        fired(&outcome),
        vec![delivery("w", &[11, 20])],
        "the bind on a sees the level 1 only"
    );
    assert_eq!(rig.values(&a).await, vec![10]);

    // The single-bind waiter v is older than the join w2, so it takes the record of level 1.
    let (c, d) = (ch("c"), ch("d"));
    assert!(rig.ok(take_in(&c, "v", any(), None)).await.fires.is_empty());
    let parked = rig
        .ok(joined(in_lanes(
            join(&[(&c, any()), (&d, any())], "w2"),
            &[Some(1), None],
        )))
        .await;
    assert!(parked.fires.is_empty());
    assert!(rig.ok(put_at(&d, 5, None)).await.fires.is_empty());
    assert_eq!(
        fired(&rig.ok(put_at(&c, 7, Some(1))).await),
        vec![delivery("v", &[7])],
        "the older waiter fires first"
    );
    assert_eq!(fired(&rig.ok(put_at(&c, 8, Some(1))).await), vec![
        delivery("w2", &[8, 5])
    ]);

    // Here the join w3 is older than the single-bind waiter v3, so the join fires first.
    let (e, f) = (ch("e"), ch("f"));
    let parked = rig
        .ok(joined(in_lanes(
            join(&[(&e, any()), (&f, any())], "w3"),
            &[Some(2), None],
        )))
        .await;
    assert!(parked.fires.is_empty());
    assert!(rig
        .ok(take_in(&e, "v3", any(), None))
        .await
        .fires
        .is_empty());
    rig.ok(put_at(&f, 1, None)).await;
    assert_eq!(fired(&rig.ok(put_at(&e, 9, Some(2))).await), vec![
        delivery("w3", &[9, 1])
    ]);
    assert_eq!(rig.waiters(&e).await.len(), 1, "v3 still waits");
}

// Changed by decision 103 (decision 118, rule CR-9, decision-c6 §6 item 1): a persistent entry
// claims nothing (rule (a)), so a persistent record binds several positions of one join. The test
// is renamed to `space_joins_persistent_record_binds_two_positions`. The test before:
// /// Decision 28: k binds on one channel bind k distinct records. A persistent record binds one
// /// bind only, so a join `[a, a]` over one persistent record parks, where HEAD would bind the
// /// record twice. A second record completes the join, and the persistent record stays.
// #[tokio::test]
// async fn space_joins_repeated_channel_distinct_records() {
//     let rig = Rig::new(SpaceInner::Bag).await;
//     let a = ch("a");
//     rig.ok(put_forever(&a, 1)).await;
//     let parked = rig.ok(joined(join(&[(&a, any()), (&a, any())], "w"))).await;
//     assert!(
//         parked.fires.is_empty(),
//         "one persistent record binds one bind only"
//     );
//     let outcome = rig.ok(put(&a, 2)).await;
//     assert_eq!(fired(&outcome), vec![delivery("w", &[2, 1])]);
//     assert_eq!(rig.records(&a).await, vec![(0, None, vec![int(1)], true)]);
//
//     let c = ch("c");
//     rig.ok(put_forever(&c, 1)).await;
//     rig.ok(put(&c, 3)).await;
//     let three = rig
//         .ok(joined(join(&[(&c, any()), (&c, any()), (&c, any())], "t")))
//         .await;
//     assert!(
//         three.fires.is_empty(),
//         "two records cannot bind three binds"
//     );
//     let outcome = rig.ok(put(&c, 4)).await;
//     let [(name, values)] = <[(String, Vec<Par>); 1]>::try_from(fired(&outcome)).expect("one fire");
//     assert_eq!(name, "t");
//     assert_eq!(
//         values[0],
//         int(4),
//         "the incoming record binds the first bind"
//     );
//     let mut rest = values[1..].to_vec();
//     rest.sort_by_key(|value| format!("{value:?}"));
//     let mut expected = vec![int(1), int(3)];
//     expected.sort_by_key(|value| format!("{value:?}"));
//     assert_eq!(rest, expected);
//     assert_eq!(rig.records(&c).await, vec![(0, None, vec![int(1)], true)]);
//
//     // On a queue the window of two binds is the first two records.
//     let queue = Rig::new(SpaceInner::Queue).await;
//     let e = ch("e");
//     for value in [1, 2, 3] {
//         queue.ok(put(&e, value)).await;
//     }
//     let outcome = queue
//         .ok(joined(join(&[(&e, any()), (&e, any())], "q")))
//         .await;
//     assert_eq!(fired(&outcome), vec![delivery("q", &[1, 2])]);
//     assert_eq!(queue.values(&e).await, vec![3]);
// }
/// Decision 118, rule (a) of the claim rule (decision-c6.md section 3). A persistent record
/// claims nothing, so it binds several positions of one join. A non-persistent record claims its
/// record, so it binds one position (decision 28). A collection that a group names two or more
/// times keeps the canonical order of DR-75 (E1, rule CR-8), so each part is deterministic.
///
/// * A join `[a, a]` over one persistent record 1 fires at its arrival with `[1, 1]`, and 1
///   stays.
/// * A join `[c, c, c]` over 3 and a persistent 1, in this canonical order (`ordered_sends`),
///   binds 3 at the first position and 1 at the other two. Then c holds 1 only.
/// * A join `[d, d, d]` over a persistent 1 and 3, in this canonical order, binds 1 at each
///   position, the first entry of each E1 list. Then d holds 1 and 3.
/// * On a queue the window of two binds is the first two records.
#[tokio::test]
async fn space_joins_persistent_record_binds_two_positions() {
    let mut pair = PairRig::new(SpaceInner::Bag).await;
    let a = ch("a");
    pair.ok(put_forever(&a, 1)).await;
    let outcome = pair
        .ok(joined(join(&[(&a, any()), (&a, any())], "w")))
        .await;
    assert_eq!(
        fired(&outcome),
        vec![delivery("w", &[1, 1])],
        "the persistent 1 binds both binds at the arrival of the join"
    );
    assert_eq!(pair.rig.records(&a).await, vec![(
        0,
        None,
        vec![int(1)],
        true
    )]);

    let c = ch("c");
    for send in ordered_sends(&c, &[(3, false), (1, true)]) {
        pair.ok(SpaceOp::Produce(send)).await;
    }
    let outcome = pair
        .ok(joined(join(&[(&c, any()), (&c, any()), (&c, any())], "t")))
        .await;
    assert_eq!(
        fired(&outcome),
        vec![delivery("t", &[3, 1, 1])],
        "3 claims its record at the first bind, and the persistent 1 binds the other two"
    );
    assert_eq!(pair.rig.records(&c).await, vec![(
        0,
        None,
        vec![int(1)],
        true
    )]);

    let d = ch("d");
    for send in ordered_sends(&d, &[(1, true), (3, false)]) {
        pair.ok(SpaceOp::Produce(send)).await;
    }
    let outcome = pair
        .ok(joined(join(&[(&d, any()), (&d, any()), (&d, any())], "u")))
        .await;
    assert_eq!(
        fired(&outcome),
        vec![delivery("u", &[1, 1, 1])],
        "the persistent 1 is the first unclaimed entry of each E1 list"
    );
    assert_eq!(pair.rig.records(&d).await, vec![
        (0, None, vec![int(1)], true),
        (0, None, vec![int(3)], false)
    ]);
    // Retired by decision 90 (C6, step D): the replay over the rigged replay modes, which
    // retire.

    // On a queue the window of two binds is the first two records.
    let queue = Rig::new(SpaceInner::Queue).await;
    let e = ch("e");
    for value in [1, 2, 3] {
        queue.ok(put(&e, value)).await;
    }
    let outcome = queue
        .ok(joined(join(&[(&e, any()), (&e, any())], "q")))
        .await;
    assert_eq!(fired(&outcome), vec![delivery("q", &[1, 2])]);
    assert_eq!(queue.values(&e).await, vec![3]);
}

// Changed by decision 103 (decision 118, rule CR-9, decision-c6 §6 item 2): a persistent
// incoming datum claims nothing, so it binds two positions of its collection. The doc before:
// /// Decision 28 on the produce side (joins plan 3.5): the incoming datum binds exactly one
// /// position. The search tries each position of its collection in group order. On `[c, c]` with
// /// the patterns `any` and `9`, the incoming 9 first binds the first position, and then no stored
// /// record binds `9`. So the search retries the second position, where the stored 5 completes the
// /// join. Without the retry the 9 would be stored next to a waiter that it completes.
/// Decision 28 on the produce side (joins plan 3.5), restated by decision 118 (rule (a),
/// decision-c6.md section 6, item 2). The search pins the incoming datum at each position of its
/// collection in group order (rule CR-10). On `[c, c]` with the patterns `any` and `9` over the
/// stored 5:
///
/// * A non-persistent incoming 9 claims its record, so it binds one position. Pinned at the first
///   position, it leaves no record for `9`. So the search retries the second position, where the
///   stored 5 completes the join, and the fire is (5, 9). Without the retry the 9 would be stored
///   next to a waiter that it completes.
/// * A persistent incoming 9 claims nothing. Pinned at the first position, it also binds the
///   second position through its incoming entry, first in the E1 list of its collection
///   (decision-c6.md section 2, point 3). So the fire is (9, 9) with two incoming candidates on
///   the produce channel. The stored 5 stays, and the 9 is stored after the fire.
#[tokio::test]
async fn space_joins_incoming_binds_once() {
    // Changed by decision 103 (decision 118, rule CR-9, decision-c6 §6 item 2): a persistent
    // incoming 9 claims nothing, so it binds both positions of its collection, and each case
    // replays its play. The body before:
    // for persistent in [false, true] {
    //     let rig = Rig::new(SpaceInner::Bag).await;
    //     let c = ch("c");
    //     let stored = send_salted(&c, 5, 1);
    //     rig.ok(SpaceOp::Produce(stored.clone())).await;
    //     let parked = rig
    //         .ok(joined(join(&[(&c, any()), (&c, exactly(9))], "w")))
    //         .await;
    //     assert!(parked.fires.is_empty());
    //     let incoming = ProduceOp {
    //         persistent,
    //         ..send_salted(&c, 9, 2)
    //     };
    //     let outcome = rig.ok(SpaceOp::Produce(incoming.clone())).await;
    //     assert_eq!(fired(&outcome), vec![delivery("w", &[5])]);
    //     let [fire] = outcome.fires.as_slice() else {
    //         panic!("one fire");
    //     };
    //     let removed: Vec<ListParWithRandom> = fire
    //         .results
    //         .iter()
    //         .map(|result| result.removed_datum.clone())
    //         .collect();
    //     assert_eq!(
    //         removed,
    //         vec![
    //             record::wrap_datum(0, &stored.data),
    //             record::wrap_datum(0, &incoming.data)
    //         ],
    //         "the stored 5 binds the first bind and the incoming 9 binds the second"
    //     );
    //     match persistent {
    //         false => assert!(rig.values(&c).await.is_empty()),
    //         true => assert_eq!(
    //             rig.records(&c).await,
    //             vec![(0, None, vec![int(9)], true)],
    //             "a persistent incoming record is stored after the fire"
    //         ),
    //     }
    // }
    for persistent in [false, true] {
        let mut pair = PairRig::new(SpaceInner::Bag).await;
        let c = ch("c");
        let stored = send_salted(&c, 5, 1);
        pair.ok(SpaceOp::Produce(stored.clone())).await;
        let parked = pair
            .ok(joined(join(&[(&c, any()), (&c, exactly(9))], "w")))
            .await;
        assert!(parked.fires.is_empty());
        let incoming = ProduceOp {
            persistent,
            ..send_salted(&c, 9, 2)
        };
        let outcome = pair.ok(SpaceOp::Produce(incoming.clone())).await;
        let [fire] = outcome.fires.as_slice() else {
            panic!("one fire");
        };
        let removed: Vec<ListParWithRandom> = fire
            .results
            .iter()
            .map(|result| result.removed_datum.clone())
            .collect();
        let x = record::wrap_datum(0, &incoming.data);
        let candidates_of_x = match persistent {
            false => {
                assert_eq!(fired(&outcome), vec![delivery("w", &[5])]);
                assert_eq!(
                    removed,
                    vec![record::wrap_datum(0, &stored.data), x],
                    "the retry pins the 9 at the second bind, and the stored 5 binds the first"
                );
                assert!(pair.rig.values(&c).await.is_empty());
                1
            }
            true => {
                assert_eq!(fired(&outcome), vec![delivery("w", &[9])]);
                assert_eq!(
                    removed,
                    vec![x.clone(), x],
                    "the persistent 9 binds both binds"
                );
                assert_eq!(
                    pair.rig.records(&c).await,
                    vec![
                        (0, None, vec![int(5)], false),
                        (0, None, vec![int(9)], true)
                    ],
                    "the stored 5 stays, and the persistent 9 is stored after the fire"
                );
                2
            }
        };
        let log = pair.play_log().await;
        let comm = log
            .iter()
            .find_map(|event| match event {
                Event::Comm(comm) if comm.consume.channel_hashes.len() >= 2 => Some(comm),
                _ => None,
            })
            .expect("the join COMM of the fire");
        let channel_key = rspace_plus_plus::rspace::hashing::stable_hash_provider::hash(&data(&c));
        let x_hash = record_hash(&incoming);
        assert_eq!(
            comm.produces
                .iter()
                .filter(|produce| produce.hash == x_hash)
                .count(),
            candidates_of_x,
            "the incoming candidates of the COMM (persistent: {persistent})"
        );
        assert!(
            comm.produces
                .iter()
                .all(|produce| produce.channel_hash == channel_key),
            "each candidate lies on the produce channel"
        );
    }
}

// ─── 7.1: decision 118, rule (a) of the claim rule ───────────────────────────────────────────

/// Added by decision 103 (decision 118, rule CR-9): the join COMMs of `log`, in log order.
fn join_comms(log: &[Event]) -> Vec<&COMM> {
    log.iter()
        .filter_map(|event| match event {
            Event::Comm(comm) if comm.consume.channel_hashes.len() >= 2 => Some(comm),
            _ => None,
        })
        .collect()
}

/// Added by decision 103 (decision 118, rule CR-9): `hashes`, sorted.
fn sorted_hashes(mut hashes: Vec<Blake2b256Hash>) -> Vec<Blake2b256Hash> {
    hashes.sort();
    hashes
}

/// Added by decision 103 (decision 118, rule CR-9): the source hashes of the records that the
/// COMM `comm` binds, one for each candidate, sorted.
fn bound_sources(comm: &COMM) -> Vec<Blake2b256Hash> {
    sorted_hashes(
        comm.produces
            .iter()
            .map(|produce| produce.hash.clone())
            .collect(),
    )
}

/// Added by decision 103 (decision 118, rule CR-9): the source hashes of the records of the
/// collection of `channel` in `rig`, sorted.
async fn stored_sources(rig: &Rig, channel: &Par) -> Vec<Blake2b256Hash> {
    sorted_hashes(
        rig.host
            .get_data(&data(channel))
            .await
            .into_iter()
            .map(|datum| datum.source.hash)
            .collect(),
    )
}

/// Added by decision 103 (decision 118, rule CR-9, decision-c6 §6 item 5): a persistent incoming
/// datum claims nothing, so it binds two positions of its collection in one COMM, on a bag and on
/// a queue. The join `[c, c]` parks on the empty collection. The persistent send of 7 fires it
/// with `[7, 7]`: the search pins 7 at the first position, and the incoming entry of 7, first in
/// the E1 list of the second position, binds that position too. The COMM holds two incoming
/// candidates, both on the produce channel. The collection keeps the persistent 7 once.
#[tokio::test]
async fn space_joins_persistent_incoming_binds_two_positions() {
    for inner in [SpaceInner::Bag, SpaceInner::Queue] {
        let mut pair = PairRig::new(inner).await;
        let c = ch("c");
        let parked = pair
            .ok(joined(join(&[(&c, any()), (&c, any())], "w")))
            .await;
        assert!(
            parked.fires.is_empty(),
            "{inner:?}: the join parks on the empty collection"
        );
        let x = ProduceOp {
            persistent: true,
            ..send_salted(&c, 7, 1)
        };
        let outcome = pair.ok(SpaceOp::Produce(x)).await;
        assert_eq!(
            fired(&outcome),
            vec![delivery("w", &[7, 7])],
            "{inner:?}: the persistent 7 binds both binds"
        );
        let held: Vec<(Vec<Par>, bool)> = pair
            .rig
            .records(&c)
            .await
            .into_iter()
            .map(|(_, _, payload, persistent)| (payload, persistent))
            .collect();
        assert_eq!(
            held,
            vec![(vec![int(7)], true)],
            "{inner:?}: c holds the persistent 7 once"
        );
        assert!(
            pair.rig.join_waiters(&[&c, &c]).await.is_empty(),
            "{inner:?}: the join leaves"
        );
        let log = pair.play_log().await;
        let [comm] = <[&COMM; 1]>::try_from(join_comms(&log)).expect("one join COMM");
        let channel_key = rspace_plus_plus::rspace::hashing::stable_hash_provider::hash(&data(&c));
        let [first, second] = comm.produces.as_slice() else {
            panic!("{inner:?}: the COMM binds two candidates");
        };
        assert_eq!(
            first, second,
            "{inner:?}: both candidates are the persistent 7"
        );
        assert!(
            first.persistent && first.channel_hash == channel_key,
            "{inner:?}: the candidates are the persistent produce on c"
        );
    }
}

/// Added by decision 103 (decision 118, rule CR-9, C6.5, decision-c6 §6 item 6): a peek fire
/// sends a persistent incoming datum again once, at its pinned position. The peek join `[c, c]`
/// parks on the empty bag. The persistent send of 7 fires it with `[7, 7]`. The datum is stored
/// nowhere, so the fire sends it again (decision 30), once, although it binds two positions. So c
/// holds the persistent 7 once.
#[tokio::test]
async fn space_joins_peek_persistent_incoming_is_sent_once() {
    let mut pair = PairRig::new(SpaceInner::Bag).await;
    let c = ch("c");
    let parked = pair
        .ok(joined(peeking(join(&[(&c, any()), (&c, any())], "p"))))
        .await;
    assert!(
        parked.fires.is_empty(),
        "the peek join parks on the empty bag"
    );
    let x = ProduceOp {
        persistent: true,
        ..send_salted(&c, 7, 1)
    };
    let outcome = pair.ok(SpaceOp::Produce(x.clone())).await;
    assert_eq!(fired(&outcome), vec![delivery("p", &[7, 7])]);
    assert_eq!(
        pair.rig.records(&c).await,
        vec![(0, None, vec![int(7)], true)],
        "the peek fire sends the persistent 7 again once"
    );
    assert_eq!(
        stored_sources(&pair.rig, &c).await,
        vec![record_hash(&x)],
        "c holds the record of the send itself"
    );
    assert!(pair.rig.join_waiters(&[&c, &c]).await.is_empty());
    let log = pair.play_log().await;
    let [comm] = <[&COMM; 1]>::try_from(join_comms(&log)).expect("one join COMM");
    assert_eq!(
        bound_sources(comm),
        vec![record_hash(&x), record_hash(&x)],
        "the peek fire binds the persistent 7 at both binds"
    );
}

/// Added by decision 103 (decision 118, rule CR-9, decision-c6 §6 item 7): a set class with a
/// persistent member binds two positions of one join. The class 1 holds a1 and the persistent
/// a2, in this canonical order.
///
/// * Consume side: the join `[s, s]` fires at its arrival with a1 and a2. The non-persistent a1
///   claims its record and its class. The persistent a2 claims nothing, and its class has a
///   persistent member, so a2 binds the second bind (rule (a)). Then s holds a2 only, with no
///   `K_retire`.
/// * The peek variant: the peek join binds a1 and a2 too. The fire sends a1 again once (Rule E),
///   and it sends no persistent record again. So s holds a1 and a2, each once.
/// * Produce side: the join W on `[s, s, t]` parks over a2, because t is empty. The send of a1
///   does not fire W, so a1 is stored. The send of 5 on t then fires W with a1, a2 and 5. The
///   commit check FR-1' passes with no `NotInView`, because a1 binds once.
#[tokio::test]
async fn space_joins_set_persistent_class_binds_twice() {
    let s = ch("s");
    let members = ordered_sends(&s, &[(1, false), (1, true)]);
    let (a1, a2) = (&members[0], &members[1]);
    let class_pair = sorted_hashes(vec![record_hash(a1), record_hash(a2)]);

    // The consume side.
    let mut pair = PairRig::new(SpaceInner::Set).await;
    for member in &members {
        pair.ok(SpaceOp::Produce(member.clone())).await;
    }
    let outcome = pair
        .ok(joined(join(&[(&s, any()), (&s, any())], "w")))
        .await;
    assert_eq!(fired(&outcome), vec![delivery("w", &[1, 1])]);
    assert_eq!(
        stored_sources(&pair.rig, &s).await,
        vec![record_hash(a2)],
        "a1 leaves, and the persistent a2 stays"
    );
    let log = pair.play_log().await;
    let bound: Vec<Vec<Blake2b256Hash>> = join_comms(&log).into_iter().map(bound_sources).collect();
    assert_eq!(bound, vec![class_pair.clone()], "the COMM binds a1 and a2");
    assert!(
        consumes_on(&log, &data(&s)).is_empty(),
        "the fire makes no K_retire"
    );

    // The peek variant.
    let mut pair = PairRig::new(SpaceInner::Set).await;
    for member in &members {
        pair.ok(SpaceOp::Produce(member.clone())).await;
    }
    let outcome = pair
        .ok(joined(peeking(join(&[(&s, any()), (&s, any())], "p"))))
        .await;
    assert_eq!(fired(&outcome), vec![delivery("p", &[1, 1])]);
    assert_eq!(
        stored_sources(&pair.rig, &s).await,
        class_pair,
        "the peek fire sends a1 again once, so s holds a1 and a2"
    );
    let log = pair.play_log().await;
    let bound: Vec<Vec<Blake2b256Hash>> = join_comms(&log).into_iter().map(bound_sources).collect();
    assert_eq!(
        bound,
        vec![class_pair.clone()],
        "the peek COMM binds a1 and a2"
    );

    // The produce side.
    let t = ch("t");
    let five = send_salted(&t, 5, 1);
    let mut pair = PairRig::new(SpaceInner::Set).await;
    pair.ok(SpaceOp::Produce(a2.clone())).await;
    let parked = pair
        .ok(joined(join(&[(&s, any()), (&s, any()), (&t, any())], "w")))
        .await;
    assert!(
        parked.fires.is_empty(),
        "W parks over a2, because t is empty"
    );
    let stored = pair.ok(SpaceOp::Produce(a1.clone())).await;
    assert!(stored.fires.is_empty(), "a1 is stored, because t is empty");
    let outcome = pair.ok(SpaceOp::Produce(five.clone())).await;
    assert_eq!(fired(&outcome), vec![delivery("w", &[1, 1, 5])]);
    assert_eq!(
        stored_sources(&pair.rig, &s).await,
        vec![record_hash(a2)],
        "a1 leaves, and the persistent a2 stays"
    );
    assert!(pair.rig.values(&t).await.is_empty());
    assert!(pair.rig.join_waiters(&[&s, &s, &t]).await.is_empty());
    let log = pair.play_log().await;
    let bound: Vec<Vec<Blake2b256Hash>> = join_comms(&log).into_iter().map(bound_sources).collect();
    assert_eq!(
        bound,
        vec![sorted_hashes(vec![
            record_hash(a1),
            record_hash(a2),
            record_hash(&five)
        ])],
        "the COMM of the send of 5 binds a1, a2 and 5"
    );
}

/// Added by decision 103 (decision 118, rule CR-9, decision-c6 §3 row 2 and §6 item 8): the
/// non-persistent members of a free set class bind before its persistent member. The class 1
/// holds a1, a2 and the persistent a3, in this canonical order. The join `[s, s]` binds a1 at the
/// first bind, and a1 claims its record and its class. The class has the persistent member a3,
/// so the claimed class does not block a2, and a2 binds the second bind. The fire makes no
/// `K_retire`, because the class has no other member that is not persistent. Then s holds a3, and
/// a second join `[s, s]` binds a3 at both binds.
#[tokio::test]
async fn space_joins_set_free_class_binds_members_before_the_persistent_member() {
    let s = ch("s");
    let members = ordered_sends(&s, &[(1, false), (1, false), (1, true)]);
    let mut pair = PairRig::new(SpaceInner::Set).await;
    for member in &members {
        pair.ok(SpaceOp::Produce(member.clone())).await;
    }
    let first = pair
        .ok(joined(join(&[(&s, any()), (&s, any())], "w")))
        .await;
    assert_eq!(fired(&first), vec![delivery("w", &[1, 1])]);
    assert_eq!(
        stored_sources(&pair.rig, &s).await,
        vec![record_hash(&members[2])],
        "a1 and a2 leave, and the persistent a3 stays"
    );
    let second = pair
        .ok(joined(join(&[(&s, any()), (&s, any())], "v")))
        .await;
    assert_eq!(fired(&second), vec![delivery("v", &[1, 1])]);
    assert_eq!(
        stored_sources(&pair.rig, &s).await,
        vec![record_hash(&members[2])],
        "the persistent a3 stays"
    );
    let log = pair.play_log().await;
    let bound: Vec<Vec<Blake2b256Hash>> = join_comms(&log).into_iter().map(bound_sources).collect();
    assert_eq!(
        bound,
        vec![
            sorted_hashes(vec![record_hash(&members[0]), record_hash(&members[1])]),
            vec![record_hash(&members[2]), record_hash(&members[2])]
        ],
        "the first join binds a1 and a2, and the second binds a3 twice"
    );
    assert!(
        consumes_on(&log, &data(&s)).is_empty(),
        "no fire makes a K_retire"
    );
}

/// Added by decision 103 (decision 118, rule CR-9, decision-c6 §6 item 9): on a queue, a cascade
/// gives a waiter its turn, and that waiter binds one persistent record at two positions. The
/// queue holds the persistent 1. The join W0 `[exactly(1), exactly(5)]` reaches the head and
/// parks, because the window `[1!!]` has no 5. The join W `[exactly(1), exactly(1)]` parks behind
/// W0. The send of 5 fires W0 with `[1, 5]`. Then the cascade gives W the window `[1!!]`. The
/// persistent 1 claims nothing, so W fires with `[1, 1]`. The queue keeps only the persistent 1.
#[tokio::test]
async fn space_joins_cascade_binds_a_persistent_record_at_two_positions() {
    let q = ch("q");
    let mut pair = PairRig::new(SpaceInner::Queue).await;
    let stored = pair.ok(put_forever(&q, 1)).await;
    assert!(
        stored.fires.is_empty(),
        "no waiter, so the persistent 1 is stored"
    );
    let head = pair
        .ok(joined(join(&[(&q, exactly(1)), (&q, exactly(5))], "w0")))
        .await;
    assert!(
        head.fires.is_empty(),
        "the window [1!!] has no 5, so W0 parks at the head"
    );
    let behind = pair
        .ok(joined(join(&[(&q, exactly(1)), (&q, exactly(1))], "w")))
        .await;
    assert!(behind.fires.is_empty(), "W parks behind W0");
    let [one] = <[Blake2b256Hash; 1]>::try_from(stored_sources(&pair.rig, &q).await)
        .expect("the queue holds the persistent 1");
    let outcome = pair.ok(put(&q, 5)).await;
    // The patterns `exactly` capture nothing, so the records that each fire removes give the
    // value of each position.
    let bound: Vec<(String, Vec<Vec<Par>>)> = fired(&outcome)
        .into_iter()
        .zip(&outcome.fires)
        .map(|((name, _), fire)| {
            let values = fire
                .results
                .iter()
                .map(|result| {
                    record::unwrap_datum(&result.removed_datum)
                        .expect("a queue fire removes a datum record")
                        .datum
                        .pars
                })
                .collect();
            (name, values)
        })
        .collect();
    assert_eq!(
        bound,
        vec![
            ("w0".to_string(), vec![vec![int(1)], vec![int(5)]]),
            ("w".to_string(), vec![vec![int(1)], vec![int(1)]]),
        ],
        "the send of 5 fires W0 with [1, 5], and the cascade then fires W with [1, 1]"
    );
    let held: Vec<(Vec<Par>, bool)> = pair
        .rig
        .records(&q)
        .await
        .into_iter()
        .map(|(_, _, payload, persistent)| (payload, persistent))
        .collect();
    assert_eq!(
        held,
        vec![(vec![int(1)], true)],
        "the queue holds only the persistent 1"
    );
    let log = pair.play_log().await;
    let [first, second] = <[&COMM; 2]>::try_from(join_comms(&log)).expect("two join COMMs");
    // A COMM sorts its produces by channel and source hash, so the checks below do not read the
    // positions from the order of the produces.
    let (bound_one, bound_five): (Vec<&Produce>, Vec<&Produce>) = first
        .produces
        .iter()
        .partition(|produce| produce.hash == one);
    assert!(
        matches!(bound_one.as_slice(), [produce] if produce.persistent),
        "the COMM of W0 binds the persistent 1 once"
    );
    assert!(
        matches!(bound_five.as_slice(), [produce] if !produce.persistent),
        "the COMM of W0 binds the 5 in flight once"
    );
    assert_eq!(
        bound_sources(second),
        vec![one.clone(), one],
        "the COMM of W binds the persistent 1 at both positions"
    );
}

/// The continuation record of version 2 (joins plan 2.2). A join waiter holds one slot for each
/// group position. A slot holds the order of the collection of the position, the bind index and
/// the lane. The decoders of version 1 reject the record. The decoder of version 2 gives back
/// the slots and the continuation of the program. The record survives the commit, a checkpoint
/// and a reset, and the join fires after the reset as before.
#[tokio::test]
async fn space_joins_multi_ord_envelope_roundtrip() {
    let rig = Rig::new(SpaceInner::Queue).await;
    let (a, b) = (ch("a"), ch("b"));
    rig.ok(put(&a, 1)).await;
    rig.ok(take(&a, "ta", any())).await;
    for _ in 0..2 {
        rig.ok(put(&b, 1)).await;
        rig.ok(take(&b, "tb", any())).await;
    }
    assert_eq!(rig.counter(&a).await, 2);
    assert_eq!(rig.counter(&b).await, 4);
    let op = join(&[(&b, any()), (&a, exactly(42))], "w");
    assert!(rig.ok(joined(op.clone())).await.fires.is_empty());
    let channels = [&b, &a];
    let expected = slots_of(&channels, &[4, 2]);
    let rows = rig.join_waiters(&channels).await;
    assert_eq!(rows, vec![JoinWaiterRow {
        slots: expected.clone(),
        name: "w".to_string(),
        persist: false,
        peek: false,
    }]);
    let stored = rig
        .host
        .get_waiting_continuations(join_group(SPACE, &channels))
        .await;
    let [waiter] = stored.as_slice() else {
        panic!("one join waiter on the group, not {}", stored.len());
    };
    assert!(record::continuation_ord(&waiter.continuation).is_err());
    assert!(record::unwrap_continuation(&waiter.continuation).is_err());
    assert!(record::pq_continuation_slots(&waiter.continuation).is_err());
    assert_eq!(
        record::join_continuation_slots(&waiter.continuation).expect("the slots decode"),
        expected
    );
    assert_eq!(
        record::unwrap_join_continuation(&waiter.continuation)
            .expect("the record decodes")
            .continuation,
        join_body(&op)
    );
    assert_eq!(
        waiter.continuation,
        record::wrap_join_continuation(&expected, &join_body(&op)).expect("a record of version 2")
    );
    let root = rig.root().await;
    rig.host
        .reset(&root)
        .await
        .expect("the reset to the checkpoint");
    assert_eq!(rig.join_waiters(&channels).await, rows);
    assert_eq!(rig.joins(&a).await, vec![join_group(SPACE, &channels)]);
    assert_eq!(rig.joins(&b).await, vec![join_group(SPACE, &channels)]);
    assert!(rig.ok(put(&b, 5)).await.fires.is_empty());
    assert_eq!(fired(&rig.ok(put(&a, 42)).await), vec![delivery("w", &[5])]);

    // A bind of a priority queue holds its lane in the policy slot of its position.
    let pq = Rig::new(SpaceInner::PriorityQueue).await;
    let (p, q) = (ch("p"), ch("q"));
    pq.ok(joined(in_lanes(
        join(&[(&p, any()), (&q, any())], "lanes"),
        &[Some(3), None],
    )))
    .await;
    let lanes: Vec<(usize, Option<i64>)> = pq
        .join_waiters(&[&p, &q])
        .await
        .into_iter()
        .flat_map(|row| row.slots)
        .map(|slot| (slot.bind, slot.policy.level()))
        .collect();
    let mut by_bind = lanes.clone();
    by_bind.sort();
    assert_eq!(by_bind, vec![(0, Some(3)), (1, None)]);
}

/// The outcome of a join fire puts the results back into bind order (joins plan 2.10, F3). The
/// receipt lists the results in group order, and the slots name the bind of each position. So the
/// body sees the values of its binds in bind order on both sides of a fire. Through Rholang, the
/// body of a space join gets the values of a host join of the same program. It also gets the
/// merged random states of that join, so a name that the body makes is the same name.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_joins_bind_order_restored_for_body() {
    let (x, y) = reversed_pair();
    let rig = Rig::new(SpaceInner::Bag).await;
    assert!(rig
        .ok(joined(join(&[(&x, any()), (&y, any())], "o")))
        .await
        .fires
        .is_empty());
    let binds: Vec<usize> = rig.join_waiters(&[&x, &y]).await[0]
        .slots
        .iter()
        .map(|slot| slot.bind)
        .collect();
    assert_eq!(binds, vec![1, 0], "the group order reverses the bind order");
    rig.ok(put(&x, 1)).await;
    let produced = rig.ok(put(&y, 2)).await;
    rig.ok(put(&x, 3)).await;
    rig.ok(put(&y, 4)).await;
    let consumed = rig.ok(joined(join(&[(&x, any()), (&y, any())], "p"))).await;
    for (outcome, name, values) in [(&produced, "o", [1, 2]), (&consumed, "p", [3, 4])] {
        assert_eq!(fired(outcome), vec![delivery(name, &values)]);
        let channels: Vec<Par> = outcome.fires[0]
            .results
            .iter()
            .map(|result| result.channel.clone())
            .collect();
        assert_eq!(
            channels,
            vec![data(&x), data(&y)],
            "result i is the result of bind i"
        );
    }

    // Through Rholang: one of the two bind orders reverses the group, and both give the outputs
    // of the host join.
    let space_session = |binds: &'static str| async move {
        let mut session = Session::new().await;
        let space = session.setup(BAG, DEFAULT).await;
        let program = format!(
            r#"new ha, hb in {{ a!(1) | b!(2) | for ({binds}) {{ new n in {{ @"{OUT}"!([x, y, *n]) }} }} }}"#
        );
        session.run(&with_space(&program), "rrs-join-body").await;
        let reversed = support::agent::group_positions(space.space, &[&space.a, &space.b]);
        (session.values(OUT).await, reversed)
    };
    let host_session = |binds: &'static str| async move {
        let mut session = Session::new().await;
        session.setup(BAG, DEFAULT).await;
        let program = format!(
            r#"new ha, hb in {{ ha!(1) | hb!(2) | for ({binds}) {{ new n in {{ @"{OUT}"!([x, y, *n]) }} }} }}"#
        );
        session.run(&with_space(&program), "rrs-join-body").await;
        session.values(OUT).await
    };
    let (forward, positions) = space_session("@x <- a & @y <- b").await;
    let (backward, _) = space_session("@y <- b & @x <- a").await;
    assert_eq!(forward, host_session("@x <- ha & @y <- hb").await);
    assert_eq!(backward, host_session("@y <- hb & @x <- ha").await);
    assert_eq!(forward.len(), 1);
    assert!(
        positions == [1, 0] || positions == [0, 1],
        "one of the two programs reverses its group"
    );
}

// Changed by decision 102 (plan v4 C5.3, rule CR-1) and decision 124 (LP-1, L5): play picks the
// order of the join waiters of an offer at random, so the doc below replaces this one.
// /// Decision 37 in joins (joins plan 3.4, 4.4, 4.5): the guard is checked once, after every bind
// /// matched, over the values in bind order. On the consume side a refusal parks. On the produce
// /// side a refusal of a bag moves on to the next waiter, and the head of a queue blocks. A counting
// /// matcher shows one guard check for each attempt.
/// Decision 37 in joins (joins plan 3.4, 4.4, 4.5): the guard is checked once, after every bind
/// matched, over the values in bind order. On the consume side a refusal parks. On the produce
/// side a refusal of a bag moves on to the next waiter, and the head of a queue blocks. A counting
/// matcher shows one guard check for each attempt.
///
/// Play picks the order of the join waiters of an offer at random (decision 102, rule CR-1, and
/// decision 124, LP-1). So the part of the bag waiters `w1` and `w2` runs [`PICK_RUNS`] times,
/// each run on a fresh rig. In each run `w2` fires and `w1` stays. The count is 2 when `w1` comes
/// first and refuses, and 1 when `w2` comes first. Over the runs, both counts occur. One of them
/// is missing with a chance of `2^-63`.
#[tokio::test]
async fn space_joins_guard_checked_once_after_all_binds() {
    let counting = CountingMatcher::default();
    let rig = Rig::over(
        play_with(counting.clone()).await,
        SpaceInner::Bag,
        &budget(),
    )
    .await;
    let (x, y) = reversed_pair();
    let first_greater = || gt(bound(1), bound(0));
    rig.ok(put(&x, 200)).await;
    rig.ok(put(&y, 50)).await;
    let before = counting.count();
    let outcome = rig
        .ok(joined(guarded(
            join(&[(&x, any()), (&y, any())], "g"),
            first_greater(),
        )))
        .await;
    assert_eq!(
        fired(&outcome),
        vec![delivery("g", &[200, 50])],
        "the guard sees 200 and 50 in bind order, although the group order reverses them"
    );
    assert_eq!(counting.count() - before, 1);
    rig.ok(put(&x, 1)).await;
    rig.ok(put(&y, 2)).await;
    let before = counting.count();
    let parked = rig
        .ok(joined(guarded(
            join(&[(&x, any()), (&y, any())], "r"),
            first_greater(),
        )))
        .await;
    assert!(parked.fires.is_empty(), "a consume-side refusal parks");
    assert_eq!(counting.count() - before, 1);
    assert_eq!(rig.values(&x).await, vec![1]);
    assert_eq!(rig.values(&y).await, vec![2]);

    let (c, d) = (ch("c"), ch("d"));
    let ordered = ordered_joins(vec![
        guarded(
            join(&[(&c, any()), (&d, any())], "w1"),
            lt(bound(1), bound(0)),
        ),
        guarded(join(&[(&c, any()), (&d, any())], "w2"), first_greater()),
    ]);
    // Changed by decision 102 (plan v4 C5.3, rule CR-1) and decision 124 (LP-1, L5): play picks
    // the order of w1 and w2 at random. So this part runs PICK_RUNS times, each run on a fresh
    // counting rig and a fresh play and replay pair, and checks the allowed set and a coverage
    // set. The body before:
    // for op in &ordered {
    //     assert!(rig.ok(joined(op.clone())).await.fires.is_empty());
    // }
    // let before = counting.count();
    // assert!(rig.ok(put(&d, 1)).await.fires.is_empty());
    // assert_eq!(
    //     counting.count(),
    //     before,
    //     "no search completes, so no guard runs"
    // );
    // let outcome = rig.ok(put(&c, 3)).await;
    // assert_eq!(fired(&outcome), vec![delivery("w2", &[3, 1])]);
    // assert_eq!(
    //     counting.count() - before,
    //     2,
    //     "w1 refuses and w2 admits: one check for each attempt"
    // );
    // let left: Vec<String> = rig
    //     .join_waiters(&[&c, &d])
    //     .await
    //     .into_iter()
    //     .map(|row| row.name)
    //     .collect();
    // assert_eq!(left, vec!["w1".to_string()]);
    let mut counts = BTreeSet::<usize>::new();
    for _ in 0..PICK_RUNS {
        let bag_counting = CountingMatcher::default();
        let bag = Rig::over(
            play_with(bag_counting.clone()).await,
            SpaceInner::Bag,
            &budget(),
        )
        .await;
        for op in &ordered {
            assert!(bag.ok(joined(op.clone())).await.fires.is_empty());
        }
        let before = bag_counting.count();
        assert!(bag.ok(put(&d, 1)).await.fires.is_empty());
        assert_eq!(
            bag_counting.count(),
            before,
            "no search completes, so no guard runs"
        );
        let outcome = bag.ok(put(&c, 3)).await;
        assert_eq!(fired(&outcome), vec![delivery("w2", &[3, 1])]);
        let checks = bag_counting.count() - before;
        assert!(
            matches!(checks, 1 | 2),
            "one check for each attempt: w2 alone, or w1 and then w2, not {checks}"
        );
        counts.insert(checks);
        let left: Vec<String> = bag
            .join_waiters(&[&c, &d])
            .await
            .into_iter()
            .map(|row| row.name)
            .collect();
        assert_eq!(left, vec!["w1".to_string()]);

        // Retired by decision 90 (C6, step D): the replay half ran through the rigged replay modes,
        // which retire (decision 125). `space_joins_replay_equal`,
        // `space_pick_helpers_put_the_named_candidates_first_in_replay`.
    }
    assert_eq!(
        counts,
        BTreeSet::from([1, 2]),
        "w1 comes first in some runs, and w2 in the others"
    );

    let queue_counting = CountingMatcher::default();
    let queue = Rig::over(
        play_with(queue_counting.clone()).await,
        SpaceInner::Queue,
        &budget(),
    )
    .await;
    let (e, f) = (ch("e"), ch("f"));
    for (name, guard) in [("q1", lt(bound(1), bound(0))), ("q2", first_greater())] {
        let op = guarded(join(&[(&e, any()), (&f, any())], name), guard);
        assert!(queue.ok(joined(op)).await.fires.is_empty());
    }
    assert!(queue.ok(put(&f, 1)).await.fires.is_empty());
    assert_eq!(queue_counting.count(), 0);
    assert!(
        queue.ok(put(&e, 3)).await.fires.is_empty(),
        "the head q1 refuses, so it blocks q2"
    );
    assert_eq!(
        queue_counting.count(),
        2,
        "the plan offers 3 to q1, and the cascade then tests the head q1 again"
    );
    assert_eq!(queue.join_waiters(&[&e, &f]).await.len(), 2);
}

/// The queue exposure witness (joins plan 7.1, `NoLiftUnsafe` of `JoinCascade.tla`). The join w
/// waits on `[q1, q2]`, and v waits behind w on `q2`. A send on `q1` fires w with the head of
/// `q2`. Then the cascade fires v with the next head of `q2`, with lift and re-offer. The lift is
/// one `K_lift` on `q2` with the step 1 and the counter value of the bump of `q2`.
#[tokio::test]
async fn space_joins_cascade_after_produce() {
    let rig = Rig::new(SpaceInner::Queue).await;
    let (q1, q2) = (ch("q1"), ch("q2"));
    rig.ok(put(&q2, 10)).await;
    rig.ok(put(&q2, 20)).await;
    assert!(rig
        .ok(joined(join(&[(&q1, any()), (&q2, any())], "w")))
        .await
        .fires
        .is_empty());
    assert!(
        rig.ok(take(&q2, "v", any())).await.fires.is_empty(),
        "v waits behind w"
    );
    let counter = rig.counter(&q2).await;
    rig.log().await;
    let send = send_salted(&q1, 1, 0);
    let outcome = rig.ok(SpaceOp::Produce(send.clone())).await;
    assert_eq!(fired(&outcome), vec![
        delivery("w", &[1, 10]),
        delivery("v", &[20])
    ]);
    assert_eq!(consumes_on(&rig.log().await, &data(&q2)), vec![
        lift_consume(&q2, counter, 1, &send.data.random_state)
    ]);
    assert!(rig.values(&q2).await.is_empty());
    assert!(rig.waiters(&q2).await.is_empty());
    assert!(rig.joins(&q1).await.is_empty() && rig.joins(&q2).await.is_empty());
}

/// A consume-side join fire exposes a stack head (joins plan 7.1): v waits on `c` for 1 under the
/// top 2. The join w takes the top 2 of `c` and the record of `d`. So the top of `c` is 1 again,
/// and the cascade fires v with lift and re-offer.
#[tokio::test]
async fn space_joins_cascade_after_consume() {
    let rig = Rig::new(SpaceInner::Stack).await;
    let (c, d) = (ch("c"), ch("d"));
    rig.ok(put(&c, 1)).await;
    rig.ok(put(&c, 2)).await;
    assert!(
        rig.ok(take(&c, "v", exactly(1))).await.fires.is_empty(),
        "the top 2 blocks v"
    );
    rig.ok(put(&d, 5)).await;
    let counter = rig.counter(&c).await;
    rig.log().await;
    let op = join(&[(&c, exactly(2)), (&d, any())], "w");
    let outcome = rig.ok(joined(op.clone())).await;
    assert_eq!(fired(&outcome), vec![
        delivery("w", &[5]),
        delivery("v", &[])
    ]);
    assert_eq!(consumes_on(&rig.log().await, &data(&c)), vec![
        lift_consume(&c, counter, 1, &op.body.random_state)
    ]);
    assert!(rig.values(&c).await.is_empty() && rig.values(&d).await.is_empty());
    assert!(rig.waiters(&c).await.is_empty());
}

/// Builds the capped queue of the cap tests in `rig`. The collection `q2` gets `records` records
/// with the values `0, 1, …`. The join w on `[q1, q2]` waits at the head of the waiters of `q2`.
/// The receives `v0, v1, …` of any value wait behind w. With `tail`, the join u on `[q2, q3]`
/// for the value -1 waits at the tail of `q2`. Then `q2` keeps a join group after w fires.
async fn capped_queue(rig: &Rig, records: usize, waiters: usize, tail: bool) {
    let (q1, q2, q3) = (ch("q1"), ch("q2"), ch("q3"));
    for value in 0..records {
        rig.ok(put(&q2, i64::try_from(value).expect("a small value")))
            .await;
    }
    rig.ok(joined(join(&[(&q1, any()), (&q2, any())], "w")))
        .await;
    for index in 0..waiters {
        rig.ok(take(&q2, &format!("v{index}"), any())).await;
    }
    if tail {
        rig.ok(joined(join(&[(&q2, exactly(-1)), (&q3, any())], "u")))
            .await;
    }
    rig.log().await;
}

/// The settle job of a cascade over the collections of `key_channel` and `seeds`, with the random
/// state of the operation `random_state`.
fn settle_job(key_channel: &Par, seeds: &[&Par], random_state: &[u8]) -> SpaceOp {
    SpaceOp::Settle(SettleOp {
        space: SPACE,
        key: key(key_channel),
        channel: key_channel.clone(),
        random_state: random_state.to_vec(),
        authority: CostAuthority::default(),
        introduction_authority: CostAuthority::default(),
        cascade: Some(Box::new(CascadeState {
            seeds: seeds.iter().map(|seed| key(seed)).collect(),
            pending: Vec::new(),
            resume: None,
        })),
    })
}

/// Decision 49 in the join engine (joins plan 4.9, 4.10): the settle before the plan, the plan and
/// the cascade count on one fire count. A send fires w and exposes 65 more pairs. It fires
/// `FIRE_ROUND_CAP` times, and its transaction re-issues one `SettleOp`. The settle has the dirty
/// collection `q2` as its seed, no pending send and no resume. The settle job starts its count
/// at 0: its first `K_lift` has the step 0, and it fires the two pairs that are left.
#[tokio::test]
async fn space_joins_cascade_round_cap_reissues_settle() {
    let rig = Rig::new(SpaceInner::Queue).await;
    let (q1, q2) = (ch("q1"), ch("q2"));
    capped_queue(&rig, FIRE_ROUND_CAP + 2, FIRE_ROUND_CAP + 1, false).await;
    let send = send_salted(&q1, -5, 0);
    let outcome = rig.ok(SpaceOp::Produce(send.clone())).await;
    assert_eq!(outcome.fires.len(), FIRE_ROUND_CAP);
    let mut expected = vec![delivery("w", &[-5, 0])];
    for index in 0..FIRE_ROUND_CAP - 1 {
        expected.push(delivery(&format!("v{index}"), &[
            i64::try_from(index + 1).expect("a small value")
        ]));
    }
    assert_eq!(fired(&outcome), expected);
    assert_eq!(
        outcome.reissue,
        Some(SpaceReissue {
            op: SpaceOp::Settle(SettleOp {
                space: SPACE,
                key: key(&q1),
                channel: q1.clone(),
                random_state: send.data.random_state.clone(),
                authority: CostAuthority::default(),
                introduction_authority: send.authority.clone(),
                cascade: Some(Box::new(CascadeState {
                    seeds: vec![key(&q2)],
                    pending: Vec::new(),
                    resume: None,
                })),
            }),
            cause: ReissueCause::RoundCap,
        })
    );
    rig.log().await;
    let counter = rig.counter(&q2).await;
    let reissue = outcome.reissue.expect("the cap re-issues a settle");
    let settled = rig.ok(reissue.op).await;
    let last = i64::try_from(FIRE_ROUND_CAP).expect("a small cap");
    assert_eq!(fired(&settled), vec![
        delivery(&format!("v{}", FIRE_ROUND_CAP - 1), &[last]),
        delivery(&format!("v{FIRE_ROUND_CAP}"), &[last + 1]),
    ]);
    assert_eq!(settled.reissue, None);
    assert_eq!(
        consumes_on(&rig.log().await, &data(&q2)),
        vec![
            lift_consume(&q2, counter, 0, &send.data.random_state),
            lift_consume(&q2, counter, 1, &send.data.random_state),
        ],
        "the settle job starts its fire count at 0"
    );
    assert!(rig.values(&q2).await.is_empty() && rig.waiters(&q2).await.is_empty());
}

/// Builds the owed state of the order test in a fresh stack rig. The records are 1 on `a`, 2 on
/// `b`, 3 on `c` and 4 on `d`. The join waiter va waits on `[a, c]`, and the join waiter vb waits
/// on `[b, d]`. Raw host operations store them, so each pair is fireable and no cascade fired it
/// yet. The plain matcher of the host does not bind a record with its envelope, so the raw
/// waiters park.
async fn owed_pairs() -> Rig {
    let rig = Rig::new(SpaceInner::Stack).await;
    for name in ["a", "b", "c", "d"] {
        let channel = ch(name);
        rig.ok(put(&channel, 100)).await;
        rig.ok(take(&channel, &format!("open-{name}"), any())).await;
    }
    for (name, value) in [("a", 1), ("b", 2), ("c", 3), ("d", 4)] {
        let record = record::wrap_datum(0, &ListParWithRandom {
            pars: vec![int(value)],
            random_state: Blake2b512Random::create_from_bytes(
                format!("rrs-join-owed {name}").as_bytes(),
            )
            .to_bytes(),
            cost_authority: None,
            cost_stack: None,
        });
        let fired = rig
            .host
            .produce(data(&ch(name)), record, false)
            .await
            .expect("a raw produce");
        assert!(fired.is_none());
    }
    for (first, second, name) in [("a", "c", "va"), ("b", "d", "vb")] {
        let (x, y) = (ch(first), ch(second));
        let channels = [&x, &y];
        let op = join(&[(&x, any()), (&y, any())], name);
        let parked = record::wrap_join_continuation(&slots_of(&channels, &[1, 1]), &join_body(&op))
            .expect("a record of version 2");
        let fired = rig
            .host
            .consume(
                join_group(SPACE, &channels),
                vec![any(), any()],
                parked,
                false,
                BTreeSet::new(),
            )
            .await
            .expect("a raw consume");
        assert!(fired.is_none(), "the raw join waiter parks");
    }
    rig.log().await;
    rig
}

/// `cascade_order_independent` (joins plan 4.9): on a stack, two fireable join waiters have
/// disjoint collection sets, so their fires commute. One settle job fires both in the order of the
/// collection keys, and two settle jobs fire them in either order. The three runs reach one root.
#[tokio::test]
async fn space_joins_cascade_order_independent() {
    let (a, b) = (ch("a"), ch("b"));
    let random = Blake2b512Random::create_from_bytes(b"rrs-join-owed settle").to_bytes();
    let both = owed_pairs().await;
    let outcome = both.ok(settle_job(&a, &[&b], &random)).await;
    let mut names = fired(&outcome);
    names.sort();
    assert_eq!(names, vec![
        delivery("va", &[1, 3]),
        delivery("vb", &[2, 4])
    ]);
    let group_a = join_group(SPACE, &[&a, &ch("c")]);
    let group_b = join_group(SPACE, &[&b, &ch("d")]);
    assert!(group_a.iter().all(|channel| !group_b.contains(channel)));
    let root = both.root().await;
    for order in [[&a, &b], [&b, &a]] {
        let rig = owed_pairs().await;
        for key_channel in order {
            let outcome = rig.ok(settle_job(key_channel, &[], &random)).await;
            assert_eq!(outcome.fires.len(), 1, "one pair in the closure of the job");
        }
        assert_eq!(
            rig.root().await,
            root,
            "the order of the fires changes no state"
        );
    }
}

// ─── 7.1: routing ────────────────────────────────────────────────────────────────────────────

/// The factory of the set family.
const SET: &str = "rho:space:hashmap:set";

/// The factory of the stack family.
const STACK: &str = "rho:space:hashmap:stack";

/// The factory of the cell family.
const CELL: &str = "rho:space:hashmap:cell";

/// The factory of the priority-queue family.
const PQ: &str = "rho:space:hashmap:priorityqueue";

/// The data channel of the collection of `name` in the session space `space`.
fn session_data(space: NativeSpace, name: &Par) -> Par {
    support::session::data_channel(space, name)
}

/// The joins of the two names of a session space are untouched: no join waiter and no join group
/// on their collections.
async fn assert_no_join_state(session: &Session, space: &support::session::Space) {
    for name in [&space.a, &space.b] {
        let data = session_data(space.space, name);
        assert!(session.play.get_joins(data.clone()).await.is_empty());
        assert!(session.parked(space.space, name).await.is_empty());
    }
}

/// `CrossSpaceJoin` (joins plan 2.7): a receive whose channels route to two native spaces fails
/// before any space work. This holds for every family and for a pair of families. A `use` block
/// does not change that.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_joins_cross_space_join_aborts() {
    for (urn, other) in [(BAG, SET), (STACK, STACK), (PQ, CELL), (QUEUE, BAG)] {
        let mut session = Session::new().await;
        let space = session.setup(urn, DEFAULT).await;
        session
            .run(
                &format!(
                    r#"new factory(`{other}`) in {{ for (other <- factory!?("default")) {{ for (c <- other!?("gensym")) {{ @"rrs-other"!(*c) }} }} }}"#
                ),
                &format!("rrs-join-cross-other {urn} {other}"),
            )
            .await;
        for (label, body) in [
            (
                "two-spaces",
                r#"for (c <<- @"rrs-other") { for (@x <- a & @y <- c) { Nil } }"#,
            ),
            (
                "other-first",
                r#"for (c <<- @"rrs-other") { for (@y <- c & @x <- b) { Nil } }"#,
            ),
            (
                "use-and-other",
                r#"for (c <<- @"rrs-other") { use space { for (@x <- @"left" & @y <- c) { Nil } } }"#,
            ),
        ] {
            session
                .fail_with(
                    &with_space(body),
                    &format!("rrs-join-cross {urn} {label}"),
                    &support::session::Injections::new(),
                    &["CrossSpaceJoin"],
                )
                .await;
        }
        assert_no_join_state(&session, &space).await;
    }
}

/// A join of a default channel and a space channel fails with `CrossSpaceJoin` before any space
/// work. This holds for every family and for both orders of the binds. A join of host channels
/// outside every `use` block stays an ordinary host join.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_joins_default_and_space_join_aborts() {
    for urn in [BAG, SET, CELL, QUEUE, STACK, PQ] {
        let mut session = Session::new().await;
        let space = session.setup(urn, DEFAULT).await;
        for (label, body) in [
            (
                "space-and-host",
                r#"for (@x <- a & @y <- @"rrs-host") { Nil }"#,
            ),
            (
                "host-and-space",
                r#"new h in { for (@y <- h & @x <- b) { Nil } }"#,
            ),
            (
                "three",
                r#"new h in { for (@x <- a & @y <- b & @z <- h) { Nil } }"#,
            ),
        ] {
            session
                .fail_with(
                    &with_space(body),
                    &format!("rrs-join-default {urn} {label}"),
                    &support::session::Injections::new(),
                    &["CrossSpaceJoin"],
                )
                .await;
        }
        assert_no_join_state(&session, &space).await;
        session
            .run(
                &format!(
                    r#"@"rrs-l"!(1) | @"rrs-r"!(2) | for (@x <- @"rrs-l" & @y <- @"rrs-r") {{ @"{OUT}"!(x + y) }}"#
                ),
                &format!("rrs-join-default {urn} host"),
            )
            .await;
        assert_eq!(session.values(OUT).await, vec![vec![int(3)]]);
    }
}

// ─── 7.1: merge check #3 ─────────────────────────────────────────────────────────────────────

// Changed by decision 90 (C6, decision 69): the node runs native deploys through the native
// session, and native replay certifies each deploy (`support::session`). The legacy node
// follows.
//
// /// A play runtime with its history, as a validator evaluates the deploys of a block.
// struct MergeNode {
//     play: rholang::rust::interpreter::rho_runtime::RhoRuntimeImpl,
//     history: rholang::rust::interpreter::rho_runtime::RhoHistoryRepository,
// }
//
// impl MergeNode {
//     /// A node over a fresh in-memory store, with a funded deploy signature.
//     async fn new() -> MergeNode {
//         let mut manager = InMemoryStoreManager::new();
//         let stores = manager
//             .r_space_stores()
//             .await
//             .expect("create the in-memory RSpace stores");
//         let (play, _replay, history) = create_runtimes(stores, false, &mut Vec::new()).await;
//         play.cost.set_deploy_signature_funded(
//             support::session::DEPLOY_SIGNATURE,
//             Sig::Ground(support::session::FUNDING_SIGNATURE.to_vec()),
//         );
//         MergeNode { play, history }
//     }
//
//     /// Evaluates one deploy that must succeed, and returns its event log.
//     async fn run(&mut self, source: &str, label: &str) -> Vec<Event> {
//         let result = self
//             .play
//             .evaluate(
//                 source,
//                 Cost::create(support::session::PHLO, label.to_owned()),
//                 HashMap::new(),
//                 Blake2b512Random::create_from_bytes(label.as_bytes()),
//             )
//             .await
//             .unwrap_or_else(|error| panic!("{label}: the evaluation returns no result: {error:?}"));
//         assert!(
//             result.errors.is_empty(),
//             "{label}: the deploy fails: {:?}",
//             result.errors
//         );
//         self.play.take_event_log().await
//     }
//
//     /// Checkpoints the play runtime and returns the root.
//     async fn checkpoint(&mut self) -> Blake2b256Hash { self.play.create_checkpoint().await.root }
//
//     /// Resets the play runtime to `root`.
//     async fn reset(&mut self, root: &Blake2b256Hash) {
//         self.play
//             .reset(root)
//             .await
//             .expect("reset the play runtime to a checkpoint root");
//     }

/// A native session with its history, as a validator evaluates the deploys of a block
/// (decision 90, C6).
struct MergeNode {
    session: Session,
    history: rholang::rust::interpreter::rho_runtime::RhoHistoryRepository,
}

impl MergeNode {
    /// A node over a fresh in-memory store, with a funded deploy signature.
    async fn new() -> MergeNode {
        let session = Session::new().await;
        let history = session.history();
        MergeNode { session, history }
    }

    /// Plays one deploy that must succeed, certifies it, and returns its event log.
    async fn run(&mut self, source: &str, label: &str) -> Vec<Event> {
        self.session.run(source, label).await.log
    }

    /// Checkpoints the runtime and returns the root.
    async fn checkpoint(&mut self) -> Blake2b256Hash { self.session.checkpoint().await }

    /// Resets the runtime to `root`.
    async fn reset(&mut self, root: &Blake2b256Hash) { self.session.reset(root).await }

    /// The merge index of a deploy log over the pre-state `root`, with the pre-state closures of
    /// `block_index.rs::create_event_log_index`.
    fn index(&self, root: &Blake2b256Hash, log: &[Event]) -> EventLogIndex {
        let reader = self
            .history
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

    // Changed by decision 90 (C6): the session reads the data. The legacy reader follows.
    //
    // /// The pars of each datum on the public channel `channel`.
    // async fn values(&self, channel: &str) -> Vec<Vec<Par>> {
    //     self.play
    //         .get_data(&string(channel))
    //         .await
    //         .into_iter()
    //         .map(|datum| datum.a.pars)
    //         .collect()
    // }
    /// The pars of each datum on the public channel `channel`.
    async fn values(&self, channel: &str) -> Vec<Vec<Par>> { self.session.values(channel).await }
}

/// The source of the deploy that creates a space with the factory `urn` and three names of it.
/// The handle goes to `@"rrs-handle"` and the names go to `@"rrs-names"`.
fn three_names(urn: &str) -> String {
    format!(
        r#"
new factory(`{urn}`) in {{
  for (space <- factory!?("default")) {{
    @"rrs-handle"!(*space) |
    for (a <- space!?("gensym")) {{
      for (b <- space!?("gensym")) {{
        for (c <- space!?("gensym")) {{
          @"rrs-names"!(*a, *b, *c)
        }}
      }}
    }}
  }}
}}
"#
    )
}

/// `body` with the handle bound to `space` and the three names bound to `a`, `b` and `c`.
fn with_three(body: &str) -> String {
    format!("for (space <<- @\"rrs-handle\") {{ for (a, b, c <<- @\"rrs-names\") {{\n{body}\n}} }}")
}

/// Merge check #3 with a persisted space join (joins plan 5). A join waiter that is parked in the
/// pre-state puts a join of two channels on `D(a)` and `D(b)`. A produce on either of them then
/// conflicts with every other branch, as a HEAD join does. Here the other branch uses the
/// unrelated collection `c`. From the same pre-state without the join, the same branches merge.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_joins_persisted_waiter_check3_conflict() {
    for urn in [BAG, QUEUE] {
        let mut node = MergeNode::new().await;
        node.run(&three_names(urn), &format!("rrs-join-c3 {urn} setup"))
            .await;
        let names = match <[Vec<Par>; 1]>::try_from(node.values("rrs-names").await) {
            Ok([names]) => <[Par; 3]>::try_from(names).expect("three names"),
            Err(data) => panic!("@\"rrs-names\" holds {} data", data.len()),
        };
        let handle = match <[Vec<Par>; 1]>::try_from(node.values("rrs-handle").await) {
            Ok([pars]) => pars[0].clone(),
            Err(data) => panic!("@\"rrs-handle\" holds {} data", data.len()),
        };
        let space = support::session::native(&handle);
        let [a, b, c] = &names;
        // Open the three collections, so a branch does not open one and race on S(sid).
        node.run(
            &with_three(
                "a!(0) | b!(0) | c!(0) | for (@x <- a) { Nil } | for (@y <- b) { Nil } | for (@z <- c) { Nil }",
            ),
            &format!("rrs-join-c3 {urn} open"),
        )
        .await;
        let control = node.checkpoint().await;
        node.run(
            &with_three("for (@x <- a & @y <- b) { Nil }"),
            &format!("rrs-join-c3 {urn} seed"),
        )
        .await;
        let root = node.checkpoint().await;
        let hash = |name: &Par| {
            rspace_plus_plus::rspace::hashing::stable_hash_provider::hash(&session_data(
                space, name,
            ))
        };
        let reader = node
            .history
            .get_history_reader(&root)
            .expect("the pre-state reader");
        assert!(
            reader
                .get_joins(&hash(a))
                .expect("the joins of D(a)")
                .iter()
                .any(|join| join.len() == 2),
            "{urn}: the pre-state holds the join of a and b"
        );
        assert!(
            reader
                .get_joins(&hash(c))
                .expect("the joins of D(c)")
                .is_empty(),
            "{urn}: the branch on c touches no join"
        );
        for (body, touched) in [("a!(1)", a), ("b!(2)", b)] {
            let mut branches = Vec::with_capacity(2);
            for pre in [&root, &control] {
                node.reset(pre).await;
                let left = node
                    .run(&with_three(body), &format!("rrs-join-c3 {urn} {body} left"))
                    .await;
                node.reset(pre).await;
                let right = node
                    .run(&with_three("c!(5)"), &format!("rrs-join-c3 {urn} right"))
                    .await;
                branches.push((node.index(pre, &left), node.index(pre, &right)));
            }
            let (joined_left, joined_right) = &branches[0];
            let found: BTreeSet<Blake2b256Hash> =
                conflicts(joined_left, joined_right).0.into_iter().collect();
            assert_eq!(
                found,
                BTreeSet::from([hash(touched)]),
                "{urn} {body}: the produce on a joined collection conflicts with an unrelated branch"
            );
            assert_eq!(
                conflict_reason(joined_left, joined_right),
                Some("produceTouchBaseJoin: count=1".to_string()),
                "{urn} {body}: merge check #3"
            );
            let (control_left, control_right) = &branches[1];
            assert!(
                conflicts(control_left, control_right).0.is_empty(),
                "{urn} {body}: without the join the branches merge"
            );
        }
    }
}

// ─── 7.1: escalation, replay and restart ─────────────────────────────────────────────────────

/// The lock keys of a collection operation on `channel`: `[C(sid), S(sid), S(sid,k), D(sid,k)]`.
fn collection_keys(channel: &Par) -> Vec<Par> {
    vec![
        namespace::gprivate(namespace::config_channel(SPACE)),
        support::agent::space_counter_channel(SPACE),
        counter_channel(SPACE, channel),
        data(channel),
    ]
}

/// The lock keys of the closure `channels`: `[C(sid), S(sid)]`, then `[S(sid,k), D(sid,k)]` for
/// each collection in ascending key order.
fn closure_keys(channels: &[&Par]) -> Vec<Par> {
    let mut sorted: Vec<&Par> = channels.to_vec();
    sorted.sort_by_key(|channel| key(channel));
    sorted.dedup();
    let mut keys = vec![
        namespace::gprivate(namespace::config_channel(SPACE)),
        support::agent::space_counter_channel(SPACE),
    ];
    for channel in sorted {
        keys.push(counter_channel(SPACE, channel));
        keys.push(data(channel));
    }
    keys
}

/// The begins, the staged operations and the commits that `op` adds to `record`.
async fn recorded(
    rig: &Rig,
    record: &SharedRecord,
    op: SpaceOp,
) -> (Vec<Vec<Par>>, Vec<(usize, Vec<Par>)>, Vec<usize>) {
    let (begins, staged, commits) = {
        let record = record.lock().expect("the record lock");
        (
            record.begins.len(),
            record.staged.len(),
            record.commits.len(),
        )
    };
    rig.ok(op).await;
    let record = record.lock().expect("the record lock");
    (
        record.begins[begins..].to_vec(),
        record.staged[staged..]
            .iter()
            .map(|(txn, copy)| (*txn - begins, copy.channels()))
            .collect(),
        record.commits[commits..]
            .iter()
            .map(|txn| txn - begins)
            .collect(),
    )
}

// Disabled by decision 94: two-phase locking replaced the escalation. An operation takes phase
// B over its scope keys in its one transaction, so no first transaction is dropped and no second
// one begins. `space_joins_phase_b_locks_the_closure_once` below replaces this test.
// /// "Plan, then lock" (joins plan 4.1). An operation can have a closure that is larger than its
// /// own collections. Then it drops its first transaction, which stages nothing, and begins one
// /// second transaction over the lock keys of the closure. It commits once. A join that a second
// /// join reaches is in the closure. Every staged channel lies in the lock keys of its transaction
// /// (L1). A join whose closure is its own collections begins once, as a collection without a join
// /// does.
// #[tokio::test]
// async fn space_joins_escalation_relocks_once() {
//     let (rig, record) = Rig::recording(SpaceInner::Queue, &budget()).await;
//     let (a, b, c, d) = (ch("a"), ch("b"), ch("c"), ch("d"));
//     let (begins, staged, commits) = recorded(
//         &rig,
//         &record,
//         joined(join(&[(&a, any()), (&b, exactly(5))], "w1")),
//     )
//     .await;
//     assert_eq!(
//         begins,
//         vec![closure_keys(&[&a, &b])],
//         "a closure of its own binds"
//     );
//     assert_eq!(commits, vec![0]);
//     assert!(staged.iter().all(|(txn, _)| *txn == 0));
//
//     let (begins, staged, commits) = recorded(
//         &rig,
//         &record,
//         joined(join(&[(&b, exactly(6)), (&c, any())], "w2")),
//     )
//     .await;
//     assert_eq!(begins, vec![
//         closure_keys(&[&b, &c]),
//         closure_keys(&[&a, &b, &c])
//     ]);
//     assert_eq!(commits, vec![1], "one commit, on the second transaction");
//     assert!(
//         staged.iter().all(|(txn, _)| *txn == 1),
//         "the first transaction stages nothing"
//     );
//
//     let (begins, staged, commits) = recorded(&rig, &record, put(&a, 1)).await;
//     assert_eq!(begins, vec![
//         collection_keys(&a),
//         closure_keys(&[&a, &b, &c])
//     ]);
//     assert_eq!(commits, vec![1]);
//     assert!(!staged.is_empty());
//     for (txn, channels) in &staged {
//         assert_eq!(*txn, 1);
//         for channel in channels {
//             assert!(
//                 begins[1].contains(channel),
//                 "L1: a staged channel lies in the lock keys"
//             );
//         }
//     }
//     assert!(
//         begins[1].contains(&data(&c)),
//         "the join of b and c is reached through the join of a and b"
//     );
//
//     let (begins, _, commits) = recorded(&rig, &record, put(&d, 1)).await;
//     assert_eq!(begins, vec![collection_keys(&d)], "no join, no escalation");
//     assert_eq!(commits, vec![0]);
// }

/// Added by decision 94: "plan, then lock" in two phases (joins plan 4.1, `JoinScopeLock.tla`).
/// An operation takes phase A over its own keys and classifies its route under phase A. Then it
/// takes phase B over its own keys and the keys of its join scope, in one sorted call of the same
/// transaction. The record of a begin holds the keys of both phases. So an operation begins once
/// and commits once, also when its closure is larger than its own collections. A join that a
/// second join reaches is in the closure. Every staged channel lies in the lock keys of its
/// transaction (L1). An operation without a join takes phase B over no scope key. This test
/// replaces `space_joins_escalation_relocks_once`.
#[tokio::test]
async fn space_joins_phase_b_locks_the_closure_once() {
    let (rig, record) = Rig::recording(SpaceInner::Queue, &budget()).await;
    let (a, b, c, d) = (ch("a"), ch("b"), ch("c"), ch("d"));
    let both_phases = |own: Vec<Par>, scope: Vec<Par>| {
        let mut keys = Vec::with_capacity(own.len() + scope.len());
        keys.extend(own);
        keys.extend(scope);
        keys
    };
    let (begins, staged, commits) = recorded(
        &rig,
        &record,
        joined(join(&[(&a, any()), (&b, exactly(5))], "w1")),
    )
    .await;
    assert_eq!(
        begins,
        vec![both_phases(
            closure_keys(&[&a, &b]),
            closure_keys(&[&a, &b])
        )],
        "a closure of its own binds, and phase B takes its own scope"
    );
    assert_eq!(commits, vec![0]);
    assert!(staged.iter().all(|(txn, _)| *txn == 0));

    let (begins, staged, commits) = recorded(
        &rig,
        &record,
        joined(join(&[(&b, exactly(6)), (&c, any())], "w2")),
    )
    .await;
    assert_eq!(
        begins,
        vec![both_phases(
            closure_keys(&[&b, &c]),
            closure_keys(&[&a, &b, &c])
        )],
        "one begin, whose phase B takes the closure"
    );
    assert_eq!(commits, vec![0], "one commit, on the one transaction");
    assert!(staged.iter().all(|(txn, _)| *txn == 0));

    let (begins, staged, commits) = recorded(&rig, &record, put(&a, 1)).await;
    assert_eq!(begins, vec![both_phases(
        collection_keys(&a),
        closure_keys(&[&a, &b, &c])
    )]);
    assert_eq!(commits, vec![0]);
    assert!(!staged.is_empty());
    for (txn, channels) in &staged {
        assert_eq!(*txn, 0);
        for channel in channels {
            assert!(
                begins[0].contains(channel),
                "L1: a staged channel lies in the lock keys"
            );
        }
    }
    assert!(
        begins[0].contains(&data(&c)),
        "the join of b and c is reached through the join of a and b"
    );

    let (begins, _, commits) = recorded(&rig, &record, put(&d, 1)).await;
    assert_eq!(begins, vec![collection_keys(&d)], "no join, no scope key");
    assert_eq!(commits, vec![0]);
}

// Changed by decision 90 (C6, step D): the replay test no longer runs these scripts. The text
// before:
// ```
// /// The scripts of the replay and restart tests for `inner`: joins that park, fire on both
// /// sides, peek, cascade, persist and refuse. A cell fails some operations with `CellOccupied`,
// /// and a failed operation replays as the same failure.
// ```
/// The scripts of the restart test for `inner`: joins that park, fire on both
/// sides, peek, cascade, persist and refuse. A cell fails some operations with `CellOccupied`.
fn join_script(inner: SpaceInner) -> Vec<SpaceOp> {
    let (a, b, c) = (ch("a"), ch("b"), ch("c"));
    let send = |channel: &Par, value: i64| match inner {
        SpaceInner::PriorityQueue => put_at(channel, value, None),
        _ => put(channel, value),
    };
    vec![
        joined(join(&[(&a, any()), (&b, any())], "j1")),
        send(&a, 1),
        send(&b, 2),
        send(&a, 3),
        send(&b, 4),
        joined(peeking(join(&[(&a, any()), (&b, any())], "p"))),
        joined(join(&[(&a, any()), (&b, any())], "j2")),
        joined(join(&[(&b, any()), (&c, any())], "j3")),
        take(&c, "v", any()),
        send(&c, 5),
        send(&c, 6),
        send(&b, 7),
        joined(guarded(join(&[(&a, any()), (&c, any())], "g"), refusing())),
        send(&a, 9),
        put_forever(&a, 8),
        put_forever(&b, 10),
        joined(persisting(join(
            &[(&a, exactly(8)), (&b, exactly(10))],
            "f",
        ))),
    ]
}

/// The disciplines of the replay and restart tests.
const INNERS: [SpaceInner; 6] = [
    SpaceInner::Bag,
    SpaceInner::Set,
    SpaceInner::Cell,
    SpaceInner::Queue,
    SpaceInner::Stack,
    SpaceInner::PriorityQueue,
];

/// Runs `script` in `rig` and returns the result of each operation.
async fn run_script(rig: &Rig, script: &[SpaceOp]) -> Vec<Result<SpaceOutcome, SpaceError>> {
    let mut results = Vec::with_capacity(script.len());
    for op in script {
        results.push(rig.run(op.clone()).await);
    }
    results
}

// Changed by decision 92: every start of a play row names the space counter `S(sid)` as its last
// join group, and the replay of `SpaceMode::Replay` names no such group.
// /// Replay stages the transactions of play (decision 36, joins plan 4.9 "Determinism"). For each
// /// discipline, the join script replays with the checkpoint log of an undrained play run. The
// /// results, the roots, the legacy logs and the phlo of the two budgets are equal. Every recorded
// /// COMM is paired. The capped queue of the cap tests replays too, with its settle job. A replay
// /// without the log diverges at the first COMM.
// Changed by decision 90 (C6, step D): the test replayed over the rigged replay modes, which
// retire, so it runs as a native deploy (decision 125). The capped queue case of the old test has
// its native successor in the capped chain of `pathmap_replay_equal` (the settle job and the
// restored offer), and the replay without the log has its native successor in the cut journal of
// that test. The text before:
// ```
// /// Replay stages the transactions of play (decision 36, joins plan 4.9 "Determinism"). For each
// /// discipline, the join script replays with the checkpoint log of an undrained play run. The
// /// results, the roots, the legacy logs and the phlo of the two budgets are equal. Every recorded
// /// COMM is paired. The capped queue of the cap tests replays too, with its settle job. A replay
// /// without the log diverges at the first COMM.
// #[tokio::test]
// async fn space_joins_replay_equal() {
//     for inner in INNERS {
//         replay_case(inner, join_script(inner)).await;
//     }
//     let (q1, q2) = (ch("q1"), ch("q2"));
//     let mut capped = Vec::new();
//     for value in 0..FIRE_ROUND_CAP + 2 {
//         capped.push(put(&q2, i64::try_from(value).expect("a small value")));
//     }
//     capped.push(joined(join(&[(&q1, any()), (&q2, any())], "w")));
//     for index in 0..=FIRE_ROUND_CAP {
//         capped.push(take(&q2, &format!("v{index}"), any()));
//     }
//     capped.push(put(&q1, -5));
//     let settle = SpaceOp::Settle(SettleOp {
//         space: SPACE,
//         key: key(&q1),
//         channel: q1.clone(),
//         random_state: send_salted(&q1, -5, 0).data.random_state,
//         authority: CostAuthority::default(),
//         introduction_authority: CostAuthority::default(),
//         cascade: Some(Box::new(CascadeState {
//             seeds: vec![key(&q2)],
//             pending: Vec::new(),
//             resume: None,
//         })),
//     });
//     capped.push(settle);
//     replay_case(SpaceInner::Queue, capped).await;
// }
// ```

/// Added by decision 90 (C6, step D): the join script of [`space_joins_replay_equal`] as the body
/// of a space program (`support::family_programs::space_program`, which binds `a` to `e` and
/// `out`): joins that park, fire on both sides, peek, persist, cascade and refuse. A cell refuses
/// some of the sends with `CellOccupied`, and the failure is part of what the certification
/// reproduces. The persistent join has its own channels. A persistent send that meets a persistent
/// waiter is a divergent pair, and phlo cannot end its rounds on a kind without a counter.
const JOIN_SCRIPT_BODY: &str = r#"for (@x <- a & @y <- b) { out!(["j1", x, y]) } |
a!(1) |
b!(2) |
a!(3) |
b!(4) |
for (@x <<- a & @y <<- b) { out!(["p", x, y]) } |
for (@x <- a & @y <- b) { out!(["j2", x, y]) } |
for (@x <- b & @y <- c) { out!(["j3", x, y]) } |
for (@v <- c) { out!(["v", v]) } |
c!(5) |
c!(6) |
b!(7) |
for (@x <- a & @y <- c where false) { out!(["g", x, y]) } |
a!(9) |
a!!(8) |
b!!(10) |
for (@8 <- a & @10 <- b) { out!(["f", 8, 10]) } |
for (@x <= d & @y <= e) { out!(["r", x, y]) } |
d!(11) |
e!(12)"#;

/// Native replay certifies the play of [`JOIN_SCRIPT_BODY`] on each discipline of [`INNERS`]
/// (decision 36, joins plan 4.9 "Determinism", decision 90, C6, step D). The script plays as one
/// native deploy over a fresh `hashmap:<inner>` space on each runtime of `RACING_WORKERS`. The
/// certification of the play (`certify`) reproduces its errors, a complete journal, its phlo, its
/// authority events, its byte observations and its root (`assert_reproduces`). On a discipline
/// that fails no operation, the play fires a join of the script and delivers its label.
#[test]
fn space_joins_replay_equal() {
    for workers in RACING_WORKERS {
        racing_runtime(workers).block_on(async {
            for inner in INNERS {
                let label = format!("hashmap:{}, {workers} workers", inner.as_str());
                let term =
                    space_program(&family(inner).to_string(), r#""default""#, JOIN_SCRIPT_BODY);
                let played = native_play(&term, AMPLE_LIMIT).await;
                if played.result.errors.is_empty() {
                    assert!(!played.out.is_empty(), "{label}: the script fires");
                }
                let certification = certify(&played, &term, AMPLE_LIMIT).await;
                assert_reproduces(&label, &played, &certification);
            }
        });
    }
}

// Retired by decision 90 (C6, step D): `Metered`, `run_metered` and `replay_case` ran through the
// rigged replay modes, which retire (decision 125). `space_joins_replay_equal`,
// `pathmap_replay_equal`. The text before:
// ```
// /// What one operation of a replay script shows: its result, its legacy log and its phlo.
// type Metered = (Result<SpaceOutcome, SpaceError>, Vec<Cost>, i64);
//
// /// Runs `script` over the host of `env` and returns what each operation shows. Each operation has
// /// a legacy budget of its own (decision 45, Option P). A budget keeps at most 1024 log entries,
// /// so one budget for each operation keeps every entry of a long script.
// async fn run_metered(env: &SpaceEnv, script: &[SpaceOp]) -> Vec<Metered> {
//     let mut steps = Vec::with_capacity(script.len());
//     for op in script {
//         let budget = RuntimeBudget::new(Cost::create(LEGACY_PHLO, "rrs join replay step"));
//         let _scope = budget.enter_legacy_scope();
//         let env = SpaceEnv {
//             budget: budget.clone(),
//             ..env.clone()
//         };
//         let before = budget.get().value;
//         let result = run(&FamilyAgent, &env, op.clone()).await;
//         steps.push((result, budget.get_log(), before - budget.get().value));
//     }
//     steps
// }
//
// /// One case of `space_joins_replay_equal`.
// async fn replay_case(inner: SpaceInner, script: Vec<SpaceOp>) {
//     let (play_space, replay_space) = play_and_replay().await;
//     let play_rig = Rig::over(play_space.clone(), inner, &budget()).await;
//     let start = play_space
//         .create_checkpoint()
//         .await
//         .expect("a checkpoint after the creation")
//         .root;
//     let played = run_metered(&play_rig.env, &script).await;
//     assert!(
//         played.iter().any(|(result, _, _)| result
//             .as_ref()
//             .is_ok_and(|outcome| !outcome.fires.is_empty())),
//         "{inner:?}: the script fires"
//     );
//     assert!(
//         played
//             .iter()
//             .all(|(_, log, spent)| !log.is_empty() || *spent == 0),
//         "{inner:?}: a step that costs phlo logs its charges"
//     );
//     let Checkpoint { root, log } = play_space.create_checkpoint().await.expect("a checkpoint");
//     assert!(
//         log.iter().any(
//             |event| matches!(event, Event::Comm(comm) if comm.consume.channel_hashes.len() >= 2)
//         ),
//         "{inner:?}: the play log holds a join COMM"
//     );
//     replay_space
//         .rig_and_reset(start.clone(), log)
//         .await
//         .expect("the replay rigs the play log");
//     let replay_env = env_over(replay_space.clone(), &budget());
//     let replayed = run_metered(&replay_env, &script).await;
//     assert_eq!(
//         replayed, played,
//         "{inner:?}: the results, the legacy logs and the costs"
//     );
//     // A replay space logs no events of its own, so the event check is the pairing. Every
//     // recorded event of play is used, and no fire of the replay is unpaired.
//     replay_space.take_event_log().await;
//     replay_space
//         .check_replay_data()
//         .await
//         .expect("every recorded COMM is paired");
//     let replay_root = replay_space
//         .create_checkpoint()
//         .await
//         .expect("a checkpoint")
//         .root;
//     assert_eq!(replay_root, root, "{inner:?}: the root");
//
//     replay_space
//         .rig_and_reset(start, Vec::new())
//         .await
//         .expect("the replay rigs an empty log");
//     let mut diverged = false;
//     for op in &script {
//         if let Err(SpaceError::ReplayDivergence(_)) =
//             run(&FamilyAgent, &replay_env, op.clone()).await
//         {
//             diverged = true;
//             break;
//         }
//     }
//     assert!(diverged, "{inner:?}: a replay without the log diverges");
// }
// ```

/// The rows of the collections of `channels` in `host`, as sorted bytes. The rows of a channel
/// are its records, its single-bind waiters, its join waiters and its join groups.
async fn entry_set(host: &ParRSpace, channels: &[&Par]) -> Vec<Vec<Vec<u8>>> {
    let mut rows = Vec::new();
    for channel in channels {
        let data_channel = data(channel);
        let mut records: Vec<Vec<u8>> = host
            .get_data(&data_channel)
            .await
            .iter()
            .map(|datum| bincode::serialize(datum).expect("a record serializes"))
            .collect();
        records.sort();
        rows.push(records);
        let mut joins = host.get_joins(data_channel.clone()).await;
        joins.sort_by_key(|group| bincode::serialize(group).expect("a group serializes"));
        let mut waiters: Vec<Vec<u8>> = Vec::new();
        for group in &joins {
            for waiter in host.get_waiting_continuations(group.clone()).await {
                waiters.push(bincode::serialize(&waiter).expect("a waiter serializes"));
            }
        }
        waiters.sort();
        rows.push(waiters);
        rows.push(
            joins
                .iter()
                .map(|group| bincode::serialize(group).expect("a group serializes"))
                .collect(),
        );
    }
    rows
}

/// Join waiters, records of version 2 and join groups survive a checkpoint and a reset (joins
/// plan 7.1). After a reset to the checkpoint, the store holds the entries of a twin store that
/// did not reset. The next operations give the same results and the same root.
#[tokio::test]
async fn space_joins_restart() {
    let (a, b, c) = (ch("a"), ch("b"), ch("c"));
    for inner in INNERS {
        let script = join_script(inner);
        let (built, next) = script.split_at(9);
        let restarted = Rig::new(inner).await;
        let twin = Rig::new(inner).await;
        for rig in [&restarted, &twin] {
            run_script(rig, built).await;
        }
        let root = restarted.root().await;
        assert_eq!(twin.root().await, root, "{inner:?}: one state");
        let before = entry_set(&restarted.host, &[&a, &b, &c]).await;
        assert!(
            restarted
                .host
                .get_joins(data(&c))
                .await
                .iter()
                .any(|group| group.len() == 2),
            "{inner:?}: a join group survives on c"
        );
        restarted
            .host
            .reset(&root)
            .await
            .expect("the reset to the checkpoint");
        assert_eq!(
            entry_set(&restarted.host, &[&a, &b, &c]).await,
            before,
            "{inner:?}: the reset keeps every entry"
        );
        let after = run_script(&restarted, next).await;
        assert_eq!(
            after,
            run_script(&twin, next).await,
            "{inner:?}: the results"
        );
        assert_eq!(
            restarted.root().await,
            twin.root().await,
            "{inner:?}: the root"
        );
    }
}

// ─── The five programs of the P4 join rejection, now joins ───────────────────────────────────

// Changed by decision 90 (C6): the programs below run on the native session, which plays and
// certifies each deploy. The legacy evaluation follows.
//
// /// Evaluates the deploy `source` with the label `label` on `runtime`, which must give a result.
// async fn evaluate_deploy(
//     runtime: &rholang::rust::interpreter::rho_runtime::RhoRuntimeImpl,
//     source: &str,
//     label: &str,
// ) -> rholang::rust::interpreter::interpreter::EvaluateResult {
//     runtime
//         .evaluate(
//             source,
//             Cost::create(support::session::PHLO, label.to_owned()),
//             HashMap::new(),
//             Blake2b512Random::create_from_bytes(label.as_bytes()),
//         )
//         .await
//         .unwrap_or_else(|error| panic!("{label}: the evaluation returns no result: {error:?}"))
// }

/// The values that a host join `[h, h]` binds when it parks, then 3 arrives, then 4. This is the
/// repeated-channel program of the P4 test on a host channel. The normalizer may order the binds
/// of a receive, so the test compares the space join with the host join of the same program.
async fn host_repeated_join() -> [i64; 2] {
    let mut session = Session::new().await;
    session.setup(QUEUE, DEFAULT).await;
    for (label, body) in [
        (
            "host-repeated-park",
            format!(
                r#"new dup in {{ dup!("rrs-host-a", "rrs-host-a") | for (c, d <- dup) {{ for (@x <- c & @y <- d) {{ @"{OUT}"!([x, y]) }} }} }}"#
            ),
        ),
        ("host-repeated-first", r#"@"rrs-host-a"!(3)"#.to_string()),
        ("host-repeated-second", r#"@"rrs-host-a"!(4)"#.to_string()),
    ] {
        session.run(&with_space(&body), label).await;
    }
    let outputs = session.values(OUT).await;
    let [output] = outputs.as_slice() else {
        panic!("the host join fires once, not {} times", outputs.len());
    };
    match output.as_slice() {
        [Par { exprs, .. }] => match exprs.as_slice() {
            [Expr {
                expr_instance: Some(ExprInstance::EListBody(EList { ps, .. })),
            }] => match ps.as_slice() {
                [x, y] => [number(x), number(y)],
                other => panic!("an output of two values: {other:?}"),
            },
            other => panic!("an output list: {other:?}"),
        },
        other => panic!("an output of one par: {other:?}"),
    }
}

/// The five programs of the P4 test `space_queue_join_rejected` run as joins of a queue space
/// (decision 29). The programs are two names, a repeated channel, forged channels under `use`, a
/// peek, and a persistent join. Native replay certifies each deploy with its own record (decision
/// 90, C6). Replay uses every journal row, and it reaches the phlo usage and the root of play.
// Changed by decision 90 (C6): the legacy doc follows.
//
// /// The five programs of the P4 test `space_queue_join_rejected` run as joins of a queue space
// /// (decision 29). The programs are two names, a repeated channel, forged channels under `use`, a
// /// peek, and a persistent join. Each deploy replays with its own log (protocol D). Replay uses
// /// every event and reaches the cost and the root of play.
///
/// * Two names: the join parks, and the two sends fire it with the values in bind order.
/// * A repeated channel: the join of `[a, a]` parks. The second record fires it with the values
///   that a host join of the same program binds.
/// * Forged channels in a `use` block route to the space, and the body reports through a host
///   name.
/// * A peek join fires, and Rule E stores the two records again.
/// * A persistent join fires once on the two restored records, parks, and fires again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_joins_queue_programs_run_as_joins() {
    let mut session = Session::new().await;
    let space = session.setup(QUEUE, DEFAULT).await;
    // Changed by decision 90 (C6): the session certifies each deploy from its own pre-state, so
    // the test keeps no replay runtime.
    // let start = session.play.create_checkpoint().await.root;
    // session
    //     .replay
    //     .reset(&start)
    //     .await
    //     .expect("reset the replay runtime to the setup");
    let host_repeated = host_repeated_join().await;
    let steps: Vec<(&str, String, Option<[i64; 2]>)> = vec![
        (
            "two-names-park",
            format!(r#"for (@x <- a & @y <- b) {{ @"{OUT}"!([x, y]) }}"#),
            None,
        ),
        ("two-names-fire", "a!(1) | b!(2)".to_string(), Some([1, 2])),
        (
            "repeated-park",
            format!(
                r#"new dup in {{ dup!(*a, *a) | for (c, d <- dup) {{ for (@x <- c & @y <- d) {{ @"{OUT}"!([x, y]) }} }} }}"#
            ),
            None,
        ),
        ("repeated-first", "a!(3)".to_string(), None),
        ("repeated-second", "a!(4)".to_string(), Some(host_repeated)),
        (
            "forged-park",
            format!(
                r#"new out in {{ use space {{ for (@x <- @"left" & @y <- @"right") {{ out!([x, y]) }} }} | for (@pair <- out) {{ @"{OUT}"!(pair) }} }}"#
            ),
            None,
        ),
        (
            "forged-fire",
            r#"use space { @"left"!(5) | @"right"!(6) }"#.to_string(),
            Some([5, 6]),
        ),
        ("peek-records", "a!(7) | b!(8)".to_string(), None),
        (
            "peek",
            format!(r#"for (@x <<- a & @y <<- b) {{ @"{OUT}"!([x, y]) }}"#),
            Some([7, 8]),
        ),
        (
            "persistent",
            format!(r#"for (@x <= a & @y <= b) {{ @"{OUT}"!([x, y]) }}"#),
            Some([7, 8]),
        ),
        (
            "persistent-fire",
            "a!(9) | b!(10)".to_string(),
            Some([9, 10]),
        ),
    ];
    let pair = |[x, y]: [i64; 2]| {
        vec![Par {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EListBody(EList {
                    ps: vec![int(x), int(y)],
                    ..EList::default()
                })),
            }],
            ..Par::default()
        }]
    };
    let sorted = |mut outputs: Vec<Vec<Par>>| {
        outputs.sort_by_key(|output| format!("{output:?}"));
        outputs
    };
    let mut expected: Vec<Vec<Par>> = Vec::new();
    for (label, body, output) in steps {
        let source = with_space(&body);
        // Changed by decision 90 (C6): the session plays the deploy natively, checkpoints it and
        // certifies it by native replay. The legacy play and rigged replay follow.
        // let played = evaluate_deploy(&session.play, &source, label).await;
        // assert!(played.errors.is_empty(), "{label}: {:?}", played.errors);
        // let Checkpoint { root, log } = session.play.create_checkpoint().await;
        // session
        //     .replay
        //     .rig(log.clone())
        //     .await
        //     .expect("rig the replay runtime with the play log");
        // let replayed = evaluate_deploy(&session.replay, &source, label).await;
        // assert!(replayed.errors.is_empty(), "{label}: {:?}", replayed.errors);
        // // The replay takes its log after each deploy, as the replay suites of P4 and P5 do. The
        // // take also clears the produce counters of the replay space, which its checkpoint keeps.
        // // A replay space logs no events of its own, so the event check is the pairing below.
        // // Every recorded event of play is used, and no fire is unpaired.
        // session.replay.take_event_log().await;
        // session
        //     .replay
        //     .check_replay_data()
        //     .await
        //     .unwrap_or_else(|error| panic!("{label}: the replay leaves events: {error:?}"));
        // assert_eq!(replayed.cost, played.cost, "{label}: the cost");
        // assert_eq!(
        //     session.replay.create_checkpoint().await.root,
        //     root,
        //     "{label}: the root"
        // );
        session.run(&source, label).await;
        expected.extend(output.map(pair));
        assert_eq!(
            sorted(session.values(OUT).await),
            sorted(expected.clone()),
            "{label}: the outputs"
        );
    }
    assert!(session.records(space.space, &space.a).await.is_empty());
    assert!(session.records(space.space, &space.b).await.is_empty());
    let group = support::agent::join_group(space.space, &[&space.a, &space.b]);
    let waiters = session.play.get_continuations(group).await;
    assert_eq!(waiters.len(), 1, "the persistent join waits");
    assert!(waiters[0].persist);
}

// ─── 7.2: Rule E in joins ────────────────────────────────────────────────────────────────────

/// A send of `value` on `channel` whose introduction authority and record authority are the
/// region of `byte`.
fn put_signed(channel: &Par, value: i64, byte: u8, persistent: bool) -> SpaceOp {
    let op = send_salted(channel, value, u64::from(byte));
    SpaceOp::Produce(ProduceOp {
        persistent,
        authority: region(byte),
        data: ListParWithRandom {
            cost_authority: Some(region(byte)),
            ..op.data
        },
        ..op
    })
}

/// The index of the first COMM of `log` whose consume names two or more channels.
fn join_comm_index(log: &[Event]) -> usize {
    log.iter()
        .position(
            |event| matches!(event, Event::Comm(comm) if comm.consume.channel_hashes.len() >= 2),
        )
        .expect("the log holds a join COMM")
}

/// Rule E on the consume side (decision 45, joins plan 4.8). A peek join fire removes each
/// non-persistent record that it binds. The same transaction sends the record again through
/// OFFER, with its record bytes and so with its identity. The second send has the authority of
/// the record. A persistent record stays and is not sent again. Nothing is retired on a set, and
/// a queue keeps its records at their places.
///
/// The join runs in a later deploy, whose budget counts its COMMs. So the registry of that
/// deploy holds only the registrations of the join transaction. The join consume registers the
/// authority of the operation, and the second send registers the authority of the record.
#[tokio::test]
async fn space_joins_peek_join_restores_stored_candidates() {
    let rig = Rig::new(SpaceInner::Bag).await;
    let (a, b) = (ch("a"), ch("b"));
    let SpaceOp::Produce(first) = put_signed(&a, 1, 1, false) else {
        panic!("a send is a produce");
    };
    rig.ok(SpaceOp::Produce(first.clone())).await;
    rig.ok(put_signed(&b, 2, 2, true)).await;
    let before = (rig.records(&a).await, rig.records(&b).await);
    let original = Produce::create(&data(&a), &record::wrap_datum(0, &first.data), false);
    rig.log().await;
    let deploy = budget();
    let _scope = deploy.enter_comm_accounting_scope();
    let op = JoinOp {
        introduction_authority: region(9),
        ..peeking(join(&[(&a, any()), (&b, any())], "p"))
    };
    let outcome = rig.deploy(&deploy).ok(joined(op)).await;
    assert_eq!(fired(&outcome), vec![delivery("p", &[1, 2])]);
    let log = rig.log().await;
    let at = join_comm_index(&log);
    let after_comm = &log[at + 1..];
    assert_eq!(
        produces_on(after_comm, &data(&a)),
        vec![original.clone()],
        "the removed record of a is sent again with its identity"
    );
    assert!(
        produces_on(after_comm, &data(&b)).is_empty(),
        "the persistent record of b is not sent again"
    );
    let Event::Comm(comm) = &log[at] else {
        panic!("the join fire is a COMM");
    };
    assert_eq!(
        registered_consume(&deploy, &comm.consume),
        Some(region(9)),
        "the join consume registers the authority of the operation"
    );
    assert_eq!(
        registered(&deploy, &original),
        Some(region(1)),
        "the second send registers the record's own authority"
    );
    assert_eq!((rig.records(&a).await, rig.records(&b).await), before);
    assert!(consumes_on(&log, &data(&a)).is_empty() && consumes_on(&log, &data(&b)).is_empty());

    // A set: the peek binds the first member of the class and sends it again. Nothing retires.
    let set = Rig::new(SpaceInner::Set).await;
    for salt in [1, 2] {
        set.ok(put_salted(&a, 5, salt)).await;
    }
    set.ok(put(&b, 6)).await;
    let before = (set.records(&a).await, set.records(&b).await);
    set.log().await;
    let outcome = set
        .ok(joined(peeking(join(&[(&a, exactly(5)), (&b, any())], "q"))))
        .await;
    assert_eq!(fired(&outcome), vec![delivery("q", &[6])]);
    let log = set.log().await;
    assert!(
        consumes_on(&log, &data(&a)).is_empty(),
        "a peek retires nothing"
    );
    assert_eq!((set.records(&a).await, set.records(&b).await), before);

    // A queue: the second sends take their places again, so the heads stay the heads.
    let queue = Rig::new(SpaceInner::Queue).await;
    for (channel, values) in [(&a, [1, 3]), (&b, [2, 4])] {
        for value in values {
            queue.ok(put(channel, value)).await;
        }
    }
    let before = (queue.records(&a).await, queue.records(&b).await);
    let outcome = queue
        .ok(joined(peeking(join(&[(&a, any()), (&b, any())], "r"))))
        .await;
    assert_eq!(fired(&outcome), vec![delivery("r", &[1, 2])]);
    assert_eq!((queue.records(&a).await, queue.records(&b).await), before);
    let again = queue
        .ok(joined(join(&[(&a, any()), (&b, any())], "s")))
        .await;
    assert_eq!(fired(&again), vec![delivery("s", &[1, 2])]);
}

/// Two inner channels whose group order keeps their bind order: the data channel of the first
/// has the lesser stable hash.
fn forward_pair() -> (Par, Par) {
    let (first, second) = reversed_pair();
    (second, first)
}

// Changed by decision 102 (plan v4 C5.3, rule CR-1) and decision 124 (LP-1, L5): play picks the
// order of the join waiters of an offer at random, so the doc below replaces this one.
// /// The A.9 correction case (joins plan 4.8). A peek join P on `[b, c]` fires on the produce side
// /// with the incoming `x` on `b` and the stored `r` on `c`. Both are sent again in receipt order.
// /// The store keeps `x`, and the second send of `r` completes the join W on `[b, c]`. The
// /// sequential host program does the same with `produce_peeks` in result order. It gives the same
// /// fires, the same legacy charges for each call, and the same stores.
/// The A.9 correction case (joins plan 4.8). A peek join P on `[b, c]` fires on the produce side
/// with the incoming `x` on `b` and the stored `r` on `c`. Both are sent again in receipt order.
/// The store keeps `x`, and the second send of `r` completes the join W on `[b, c]`. The
/// sequential host program does the same with `produce_peeks` in result order. It gives the same
/// fires, the same observer calls for each call (decision 90, decision 69), the same stores and
/// the same waiters. Each row of the space names the footprint group of decision 92, which the
/// host program does not have. So the comparison removes that group from the calls of the agent.
///
/// Play picks the order of the join waiters of an offer at random (decision 102, rule CR-1, and
/// decision 124, LP-1). So `x` fires P first, which is the A.9 case, or W first. W first binds `x`
/// and `r`, and P stays. The test runs [`PICK_RUNS`] times, each run on a fresh rig and a fresh
/// host. Each run pins the host waiter to the waiter that the agent fired first. Over the runs,
/// both orders occur. One of them is missing with a chance of `2^-63`.
#[tokio::test]
async fn space_joins_produce_side_peek_reproduces_in_receipt_order() {
    let (b, c) = forward_pair();
    let ordered = ordered_joins(vec![
        peeking(join(&[(&b, any()), (&c, any())], "p")),
        join(&[(&b, any()), (&c, any())], "w"),
    ]);
    let stored = send_salted(&c, 2, 1);
    let incoming = send_salted(&b, 1, 1);
    // Changed by decision 102 (plan v4 C5.3, rule CR-1) and decision 124 (LP-1, L5): play picks
    // the order of the join waiters of the offer of x at random. So the rest of the test runs
    // PICK_RUNS times, each run on a fresh rig, a fresh host and a fresh play and replay pair,
    // and checks the allowed set and a coverage set. The body before:
    // let agent_budget = RuntimeBudget::new(Cost::create(LEGACY_PHLO, "rrs join a9 agent"));
    // let _agent_scope = agent_budget.enter_legacy_scope();
    // let rig = Rig::over(play().await, SpaceInner::Bag, &agent_budget).await;
    // let host_budget = RuntimeBudget::new(Cost::create(LEGACY_PHLO, "rrs join a9 host"));
    // let _host_scope = host_budget.enter_legacy_scope();
    // let pin: support::legacy::Pin = Arc::new(std::sync::Mutex::new(None));
    // // Changed by decision 89 (DR-129): host play picks a waiter at random, so the matcher of the
    // // host also holds a waiter pin.
    // // let host = support::legacy::pinned_charging_host(&host_budget, &pin).await;
    // let waiter = support::legacy::open_waiter();
    // let host = support::legacy::pinned_charging_host(&host_budget, &pin, &waiter).await;
    // let group = join_group(SPACE, &[&b, &c]);
    // let channels = [&b, &c];
    // let parked = |op: &JoinOp| {
    //     record::wrap_join_continuation(&slots_of(&channels, &[0, 0]), &join_body(op))
    //         .expect("a record of version 2")
    // };
    // let mut agent_logs = Vec::new();
    // let mut host_logs = Vec::new();
    // let mut agent_fires = Vec::new();
    // let mut host_fires = Vec::new();
    // // 1: the stored record r on c.
    // let mark = agent_budget.get_log().len();
    // rig.ok(SpaceOp::Produce(stored.clone())).await;
    // agent_logs.push(agent_budget.get_log()[mark..].to_vec());
    // let run = support::legacy::host_produce(
    //     &host,
    //     &host_budget,
    //     data(&c),
    //     record::wrap_datum(0, &stored.data),
    //     false,
    // )
    // .await;
    // assert!(run.fired.is_none());
    // host_logs.push(run.entries);
    // // 2 and 3: the peek join P and the join W park.
    // for op in &ordered {
    //     let mark = agent_budget.get_log().len();
    //     assert!(rig.ok(joined(op.clone())).await.fires.is_empty());
    //     agent_logs.push(agent_budget.get_log()[mark..].to_vec());
    //     let run = support::legacy::host_consume(
    //         &host,
    //         &host_budget,
    //         group.clone(),
    //         vec![any(), any()],
    //         parked(op),
    //         false,
    //         op.peek,
    //     )
    //     .await;
    //     assert!(run.fired.is_none());
    //     host_logs.push(run.entries);
    // }
    // // 4: the incoming x on b fires P, and the second send of r fires W.
    // let mark = agent_budget.get_log().len();
    // let outcome = rig.ok(SpaceOp::Produce(incoming.clone())).await;
    // agent_logs.push(agent_budget.get_log()[mark..].to_vec());
    // agent_fires.extend(fired(&outcome));
    // let mut calls = vec![(data(&b), record::wrap_datum(0, &incoming.data))];
    // let mut entries = Vec::new();
    // // Added by decision 89 (DR-129): the incoming x completes both P and W, and P comes first in
    // // canonical order. So the first call fires only P. The later calls have one waiter at most.
    // support::legacy::set_waiter(
    //     &waiter,
    //     support::legacy::WaiterPin::Only(Box::new(parked(&ordered[0]))),
    // );
    // while !calls.is_empty() {
    //     let (channel, record) = calls.remove(0);
    //     let run = support::legacy::host_produce(&host, &host_budget, channel, record, false).await;
    //     support::legacy::set_waiter(&waiter, support::legacy::WaiterPin::Any);
    //     entries.extend(run.entries);
    //     if let Some((cont, results)) = &run.fired {
    //         let name = support::agent::body_name(
    //             &record::unwrap_join_continuation(&cont.continuation)
    //                 .expect("a join continuation record")
    //                 .continuation,
    //         );
    //         let mut by_bind: Vec<(usize, Vec<Par>)> = slots_of(&channels, &[0, 0])
    //             .iter()
    //             .zip(results)
    //             .map(|(slot, result)| (slot.bind, result.matched_datum.pars.clone()))
    //             .collect();
    //         by_bind.sort_by_key(|(bind, _)| *bind);
    //         host_fires.push((
    //             name,
    //             by_bind.into_iter().flat_map(|(_, pars)| pars).collect(),
    //         ));
    //     }
    //     calls.extend(run.reproduced);
    // }
    // host_logs.push(entries);
    // assert_eq!(agent_fires, vec![
    //     delivery("p", &[1, 2]),
    //     delivery("w", &[1, 2])
    // ]);
    // assert_eq!(agent_fires, host_fires, "the fires of the host program");
    // assert_eq!(agent_logs, host_logs, "the legacy charges of each call");
    // for channel in [&b, &c] {
    //     assert!(rig.records(channel).await.is_empty());
    //     assert!(host.get_data(&data(channel)).await.is_empty());
    // }
    // assert!(rig.join_waiters(&channels).await.is_empty());
    // assert!(host.get_waiting_continuations(group).await.is_empty());
    let group = join_group(SPACE, &[&b, &c]);
    let channels = [&b, &c];
    let parked = |op: &JoinOp| {
        record::wrap_join_continuation(&slots_of(&channels, &[0, 0]), &join_body(op))
            .expect("a record of version 2")
    };
    let footprint = support::agent::space_counter_channel(SPACE);
    let mut firsts = BTreeSet::<String>::new();
    for _ in 0..PICK_RUNS {
        let rig = Rig::over(play().await, SpaceInner::Bag, &budget()).await;
        let agent_recorder = observed(&rig.host);
        let host_recorder = Recorder::new();
        let pin: support::legacy::Pin = Arc::new(std::sync::Mutex::new(None));
        let waiter = support::legacy::open_waiter();
        let host = support::legacy::pinned_observed_host(&host_recorder, &pin, &waiter).await;
        let mut agent_calls: Vec<Vec<Call>> = Vec::with_capacity(4);
        let mut host_calls: Vec<Vec<Call>> = Vec::with_capacity(4);
        let mut host_fires = Vec::with_capacity(2);
        // 1: the stored record r on c.
        let mark = agent_recorder.count();
        rig.ok(SpaceOp::Produce(stored.clone())).await;
        agent_calls.push(agent_recorder.since(mark));
        let run = support::legacy::host_produce(
            &host,
            &host_recorder,
            data(&c),
            record::wrap_datum(0, &stored.data),
            false,
        )
        .await;
        assert!(run.fired.is_none());
        host_calls.push(run.calls);
        // 2 and 3: the peek join P and the join W park.
        for op in &ordered {
            let mark = agent_recorder.count();
            assert!(rig.ok(joined(op.clone())).await.fires.is_empty());
            agent_calls.push(agent_recorder.since(mark));
            let run = support::legacy::host_consume(
                &host,
                &host_recorder,
                group.clone(),
                vec![any(), any()],
                parked(op),
                false,
                op.peek,
            )
            .await;
            assert!(run.fired.is_none());
            host_calls.push(run.calls);
        }
        // 4: the incoming x on b fires P or W first. P first is the A.9 case: P fires, and the
        // second send of r fires W. W first binds x and r, and P stays.
        let mark = agent_recorder.count();
        let outcome = rig.ok(SpaceOp::Produce(incoming.clone())).await;
        agent_calls.push(agent_recorder.since(mark));
        let agent_fires = fired(&outcome);
        let first = agent_fires
            .first()
            .map(|(name, _)| name.clone())
            .expect("the incoming x fires a waiter");
        let (first_op, expected_fires, expected_waiters) = match first.as_str() {
            "p" => (
                &ordered[0],
                vec![delivery("p", &[1, 2]), delivery("w", &[1, 2])],
                Vec::<String>::new(),
            ),
            "w" => (&ordered[1], vec![delivery("w", &[1, 2])], vec![
                "p".to_string()
            ]),
            other => panic!("the incoming x fires P or W first, not {other}"),
        };
        assert_eq!(agent_fires, expected_fires);
        firsts.insert(first);
        let mut calls = vec![(data(&b), record::wrap_datum(0, &incoming.data))];
        let mut produced_calls = Vec::new();
        // The first call fires only the waiter that the agent fired first. The later calls have
        // one waiter at most.
        support::legacy::set_waiter(
            &waiter,
            support::legacy::WaiterPin::Only(Box::new(parked(first_op))),
        );
        while !calls.is_empty() {
            let (channel, record) = calls.remove(0);
            let run =
                support::legacy::host_produce(&host, &host_recorder, channel, record, false).await;
            support::legacy::set_waiter(&waiter, support::legacy::WaiterPin::Any);
            produced_calls.extend(run.calls);
            if let Some((cont, results)) = &run.fired {
                let name = support::agent::body_name(
                    &record::unwrap_join_continuation(&cont.continuation)
                        .expect("a join continuation record")
                        .continuation,
                );
                let mut by_bind: Vec<(usize, Vec<Par>)> = slots_of(&channels, &[0, 0])
                    .iter()
                    .zip(results)
                    .map(|(slot, result)| (slot.bind, result.matched_datum.pars.clone()))
                    .collect();
                by_bind.sort_by_key(|(bind, _)| *bind);
                host_fires.push((
                    name,
                    by_bind.into_iter().flat_map(|(_, pars)| pars).collect(),
                ));
            }
            calls.extend(run.reproduced);
        }
        host_calls.push(produced_calls);
        assert_eq!(agent_fires, host_fires, "the fires of the host program");
        assert!(
            agent_calls.iter().all(|calls| !calls.is_empty()),
            "each call makes observer calls"
        );
        let agent_calls: Vec<Vec<Call>> = agent_calls
            .iter()
            .map(|calls| support::observed::without_footprint(calls, &footprint))
            .collect();
        assert_eq!(agent_calls, host_calls, "the observer calls of each call");
        for channel in [&b, &c] {
            assert!(rig.records(channel).await.is_empty());
            assert!(host.get_data(&data(channel)).await.is_empty());
        }
        let agent_waiters: Vec<String> = rig
            .join_waiters(&channels)
            .await
            .into_iter()
            .map(|row| row.name)
            .collect();
        let host_waiters: Vec<String> = host
            .get_waiting_continuations(group.clone())
            .await
            .iter()
            .map(|waiting| {
                support::agent::body_name(
                    &record::unwrap_join_continuation(&waiting.continuation)
                        .expect("a join continuation record")
                        .continuation,
                )
            })
            .collect();
        assert_eq!(agent_waiters, expected_waiters, "the waiters of the agent");
        assert_eq!(
            host_waiters, expected_waiters,
            "the waiters of the host program"
        );

        // Retired by decision 90 (C6, step D): the replay half ran through the rigged replay modes,
        // which retire (decision 125). `space_joins_replay_equal`,
        // `space_pick_helpers_put_the_named_candidates_first_in_replay`.
    }
    assert_eq!(
        firsts,
        BTreeSet::from(["p".to_string(), "w".to_string()]),
        "the incoming x fires P first in some runs and W first in others"
    );
}

// ─── 7.2: decisions 43 and 49 ────────────────────────────────────────────────────────────────

/// The source hashes of the consumes of the join COMMs of `log`, in log order.
fn join_consumes(log: &[Event]) -> Vec<Blake2b256Hash> {
    log.iter()
        .filter_map(|event| match event {
            Event::Comm(comm) if comm.consume.channel_hashes.len() >= 2 => {
                Some(comm.consume.hash.clone())
            }
            _ => None,
        })
        .collect()
}

/// Decision 43 for joins (joins plan 4.11): a persistent join keeps the order of each collection
/// of its first round.
///
/// * A divergent pair on two queues re-issues the join with the first-round orders, one for each
///   bind, and no stop (DV-4). Each round bumps both counters again and fires the same COMM.
/// * At the cap, the join re-issues as the resume of the settle job, with its orders. After the
///   job, the re-issued join parks with its first-round orders, although the counters went on.
/// * A bag has the order 0 everywhere, so a re-issued join of a bag carries only zeros, and its
///   identity stays.
#[tokio::test]
async fn space_joins_reissued_join_keeps_bind_ords() {
    let rig = Rig::new(SpaceInner::Queue).await;
    let (a, b) = (ch("a"), ch("b"));
    rig.ok(put_forever(&a, 1)).await;
    rig.ok(put_forever(&b, 2)).await;
    let (ord_a, ord_b) = (rig.counter(&a).await, rig.counter(&b).await);
    rig.log().await;
    let op = persisting(join(&[(&a, any()), (&b, any())], "f"));
    let mut next = SpaceOp::Join(op.clone());
    let mut comms = Vec::new();
    let expected = SpaceReissue {
        op: SpaceOp::Join(JoinOp {
            reissued_ords: Some(vec![ord_a, ord_b]),
            ..op.clone()
        }),
        cause: ReissueCause::DivergentPair,
    };
    for round in 0..3 {
        let counters = (rig.counter(&a).await, rig.counter(&b).await);
        let outcome = rig.ok(next.clone()).await;
        assert_eq!(
            fired(&outcome),
            vec![delivery("f", &[1, 2])],
            "round {round}"
        );
        assert_eq!(outcome.reissue.as_ref(), Some(&expected), "round {round}");
        assert_eq!(
            (rig.counter(&a).await, rig.counter(&b).await),
            (counters.0 + 1, counters.1 + 1),
            "round {round} bumps every own counter"
        );
        comms.push(join_consumes(&rig.log().await));
        next = expected.op.clone();
    }
    assert!(
        comms
            .iter()
            .all(|round| round.len() == 1 && *round == comms[0]),
        "every round fires one COMM of one identity"
    );

    // At the cap: a persistent join meets FIRE_ROUND_CAP + 1 records of c and the persistent d.
    let (c, d) = (ch("c"), ch("d"));
    for value in 0..=FIRE_ROUND_CAP {
        rig.ok(put(&c, i64::try_from(value).expect("a small value")))
            .await;
    }
    rig.ok(put_forever(&d, 7)).await;
    let (ord_c, ord_d) = (rig.counter(&c).await, rig.counter(&d).await);
    let op = persisting(join(&[(&c, any()), (&d, any())], "g"));
    let outcome = rig.ok(SpaceOp::Join(op.clone())).await;
    assert_eq!(outcome.fires.len(), FIRE_ROUND_CAP);
    let resumed = SpaceOp::Join(JoinOp {
        reissued_ords: Some(vec![ord_c, ord_d]),
        ..op.clone()
    });
    let reissue = outcome.reissue.expect("the cap re-issues");
    assert_eq!(reissue.cause, ReissueCause::RoundCap);
    let SpaceOp::Settle(settle) = &reissue.op else {
        panic!("a head-shaped cap re-issues a settle job: {:?}", reissue.op);
    };
    let cascade = settle
        .cascade
        .as_ref()
        .expect("the settle carries a cascade");
    assert_eq!(cascade.pending, Vec::new());
    assert_eq!(
        cascade.resume.as_deref().map(|resume| &resume.op),
        Some(&resumed),
        "the resume carries the first-round orders"
    );
    let job = rig.ok(reissue.op.clone()).await;
    assert!(job.fires.is_empty(), "nothing is owed");
    let job_reissue = job.reissue.expect("the job re-issues its resume");
    assert_eq!(job_reissue.op, resumed);
    let last = rig.ok(job_reissue.op).await;
    assert_eq!(fired(&last), vec![delivery("g", &[
        i64::try_from(FIRE_ROUND_CAP).expect("a small cap"),
        7
    ])]);
    assert_eq!(last.reissue, None, "the join parks after the last record");
    let rows = rig.join_waiters(&[&c, &d]).await;
    let mut ords: Vec<(usize, i64)> = rows[0]
        .slots
        .iter()
        .map(|slot| (slot.bind, slot.ord))
        .collect();
    ords.sort();
    assert_eq!(
        ords,
        vec![(0, ord_c), (1, ord_d)],
        "the parked join keeps its first-round orders"
    );
    assert!(rig.counter(&c).await > ord_c + 1, "the counters went on");

    // A bag at the cap: every carried order is 0, and every round has one identity.
    let bag = Rig::new(SpaceInner::Bag).await;
    for value in 0..=FIRE_ROUND_CAP {
        bag.ok(put_salted(
            &c,
            i64::try_from(value).expect("a small value"),
            3,
        ))
        .await;
    }
    bag.ok(put_forever(&d, 7)).await;
    bag.log().await;
    let outcome = bag.ok(SpaceOp::Join(op.clone())).await;
    assert_eq!(outcome.fires.len(), FIRE_ROUND_CAP);
    let first_round = join_consumes(&bag.log().await);
    let reissue = outcome.reissue.expect("the cap re-issues");
    let SpaceOp::Join(reissued) = &reissue.op else {
        panic!(
            "a bag owes nothing at the cap, so the join itself re-issues: {:?}",
            reissue.op
        );
    };
    assert!(reissued
        .reissued_ords
        .as_deref()
        .unwrap_or_default()
        .iter()
        .all(|ord| *ord == 0));
    let second = bag.ok(reissue.op.clone()).await;
    assert_eq!(second.fires.len(), 1);
    let second_round = join_consumes(&bag.log().await);
    assert!(
        first_round
            .iter()
            .chain(&second_round)
            .all(|consume| *consume == first_round[0]),
        "a re-issued join of a bag keeps its identity"
    );
}

// ─── 7.2: decision 49, the settle job and the cap ────────────────────────────────────────────

/// `value` as a number of a record.
fn small(value: usize) -> i64 { i64::try_from(value).expect("a small value") }

/// The inner channels of `names` in the group order of their join, which sorts the data channels
/// by their stable hashes.
fn group_ordered<const N: usize>(names: [&str; N]) -> [Par; N] {
    let channels = names.map(ch);
    let binds: Vec<&Par> = channels.iter().collect();
    let positions = support::agent::group_positions(SPACE, &binds);
    std::array::from_fn(|at| channels[positions[at]].clone())
}

/// The produce of the record of the send `op` in a bag or a set, whose order is 0.
fn stored_produce(op: &ProduceOp) -> Produce {
    Produce::create(
        &data(&op.channel),
        &record::wrap_datum(0, &op.data),
        op.persistent,
    )
}

/// The pending second send of the record of the send `op` in a bag or a set.
fn pending_of(op: &ProduceOp) -> PendingProduce {
    PendingProduce {
        key: key(&op.channel),
        record: record::wrap_datum(0, &op.data),
        persist: op.persistent,
        restore: None,
    }
}

/// The settle job that continues a stopped transaction of the send `send`, with the seeds
/// `seeds`, the pending second sends `pending` and the resume `resume`.
fn settle_after(
    send: &ProduceOp,
    seeds: &[&Par],
    pending: Vec<PendingProduce>,
    resume: Option<SpaceReissue>,
) -> SpaceOp {
    SpaceOp::Settle(SettleOp {
        space: SPACE,
        key: key(&send.channel),
        channel: send.channel.clone(),
        random_state: send.data.random_state.clone(),
        authority: CostAuthority::default(),
        introduction_authority: send.authority.clone(),
        cascade: Some(Box::new(CascadeState {
            seeds: seeds.iter().map(|seed| key(seed)).collect(),
            pending,
            resume: resume.map(Box::new),
        })),
    })
}

/// Builds the peek chain of the cap tests in a bag rig. The chain has `joins` peek joins
/// `p0, p1, …` on `[b, c, d]`, then the stored records 2 on `c` and 3 on `d`. It returns the sends
/// of the two records and the persistent send of 1 on `b` that starts the chain.
async fn peek_chain(
    rig: &Rig,
    [b, c, d]: [&Par; 3],
    joins: usize,
) -> (ProduceOp, ProduceOp, ProduceOp) {
    for index in 0..joins {
        let op = peeking(join(
            &[(b, any()), (c, any()), (d, any())],
            &format!("p{index}"),
        ));
        assert!(rig.ok(joined(op)).await.fires.is_empty());
    }
    let (on_c, on_d) = (send_salted(c, 2, 1), send_salted(d, 3, 1));
    for send in [&on_c, &on_d] {
        assert!(rig
            .ok(SpaceOp::Produce(send.clone()))
            .await
            .fires
            .is_empty());
    }
    rig.log().await;
    let x = ProduceOp {
        persistent: true,
        ..send_salted(b, 1, 1)
    };
    (on_c, on_d, x)
}

/// The names of the bodies that the jobs `outcomes` fired, sorted.
fn fired_names(outcomes: &[SpaceOutcome]) -> Vec<String> {
    let mut names: Vec<String> = outcomes
        .iter()
        .flat_map(|outcome| fired(outcome).into_iter().map(|(name, _)| name))
        .collect();
    names.sort();
    names
}

/// The names `prefix0, prefix1, …` of `count` bodies, sorted.
fn names(prefix: &str, count: usize) -> Vec<String> {
    let mut names: Vec<String> = (0..count).map(|index| format!("{prefix}{index}")).collect();
    names.sort();
    names
}

/// The produce events of `log`, in log order.
fn io_produces(log: &[Event]) -> Vec<Produce> {
    log.iter()
        .filter_map(|event| match event {
            Event::IoEvent(IOEvent::Produce(produce)) => Some(produce.clone()),
            _ => None,
        })
        .collect()
}

/// The number of fires of each job of `outcomes`.
fn fire_counts(outcomes: &[SpaceOutcome]) -> Vec<usize> {
    outcomes.iter().map(|outcome| outcome.fires.len()).collect()
}

/// The bodies of a peek chain that holds the records 2 on `c`, 3 on `d` and the persistent 1 on
/// `b` when it ends.
async fn assert_peek_chain_ends(rig: &Rig, [b, c, d]: [&Par; 3]) {
    assert_eq!(rig.records(b).await, vec![(0, None, vec![int(1)], true)]);
    assert_eq!(rig.records(c).await, vec![(0, None, vec![int(2)], false)]);
    assert_eq!(rig.records(d).await, vec![(0, None, vec![int(3)], false)]);
    assert!(rig.join_waiters(&[b, c, d]).await.is_empty());
}

/// Decision 49 with Rule E (joins plan 4.10, DV-1): a stop at the cap that owes second sends
/// re-issues one `SettleOp`. Its cascade carries the sends in receipt order, and the operation as
/// its resume when the element of the operation is still in flight. Peek joins on a bag
/// `[b, c, d]` and the stored records of `c` and `d` meet a persistent send on `b`.
///
/// * With `b` last in group order, the second sends of `c` and `d` run first and store. Then the
///   send of `b` fires the next join. At the cap, the transaction owes the two sends and its own
///   send. The settle job stores the two records in order and fires nothing. Its first operation
///   is not its own, so it stages its anchor first (decision 92). It re-issues the send with its
///   order (decision 43), and the send fires the joins that are left.
/// * With `b` first in group order, the send of `b` stores, and the second send of `d` fires the
///   next join. At the cap, the transaction owes the two sends and nothing resumes. The settle
///   job puts them on its work stack before any fire. So the stop of the job at the cap carries
///   them again, in one equal `SettleOp`.
#[tokio::test]
async fn space_joins_cap_carries_pending_second_sends() {
    let cap = FIRE_ROUND_CAP;
    let rig = Rig::new(SpaceInner::Bag).await;
    let [c, d, b] = group_ordered(["cap-a", "cap-b", "cap-c"]);
    let (on_c, on_d, x) = peek_chain(&rig, [&b, &c, &d], cap + 3).await;
    let first = rig.ok(SpaceOp::Produce(x.clone())).await;
    assert_eq!(first.fires.len(), cap);
    assert!(fired(&first)
        .iter()
        .all(|(_, values)| *values == [int(1), int(2), int(3)]));
    let resume = SpaceReissue {
        op: SpaceOp::Produce(ProduceOp {
            reissued_ord: Some(0),
            ..x.clone()
        }),
        cause: ReissueCause::RoundCap,
    };
    assert_eq!(
        first.reissue,
        Some(SpaceReissue {
            op: settle_after(
                &x,
                &[],
                vec![pending_of(&on_c), pending_of(&on_d)],
                Some(resume.clone())
            ),
            cause: ReissueCause::RoundCap,
        })
    );
    rig.log().await;
    let settle = first.reissue.clone().expect("the cap re-issues a settle");
    let job = rig.ok(settle.op).await;
    assert!(job.fires.is_empty(), "the job only stores the two records");
    // Changed by decision 92: the first operation of the job is not its own, so the job stages
    // its anchor first.
    // assert_eq!(
    //     rig.log().await,
    //     vec![
    //         Event::IoEvent(IOEvent::Produce(stored_produce(&on_c))),
    //         Event::IoEvent(IOEvent::Produce(stored_produce(&on_d))),
    //     ],
    //     "the job sends in receipt order, with the identities of the records"
    // );
    let log = rig.log().await;
    assert_eq!(log.len(), 4, "the anchor and the two sends: {log:?}");
    assert!(
        support::agent::logs_anchors_only(&log[..2], SPACE, 1),
        "the job stages its anchor first"
    );
    assert_eq!(
        log[2..],
        [
            Event::IoEvent(IOEvent::Produce(stored_produce(&on_c))),
            Event::IoEvent(IOEvent::Produce(stored_produce(&on_d))),
        ],
        "the job sends in receipt order, with the identities of the records"
    );
    assert_eq!(job.reissue, Some(resume.clone()), "then the send resumes");
    let last = rig.ok(resume.op).await;
    assert_eq!(last.fires.len(), 3);
    assert_eq!(last.reissue, None);
    assert_eq!(fired_names(&[first, job, last]), names("p", cap + 3));
    assert_peek_chain_ends(&rig, [&b, &c, &d]).await;

    let rig = Rig::new(SpaceInner::Bag).await;
    let [b, c, d] = group_ordered(["cap-d", "cap-e", "cap-f"]);
    let (on_c, on_d, x) = peek_chain(&rig, [&b, &c, &d], 2 * cap + 2).await;
    let first = rig.ok(SpaceOp::Produce(x.clone())).await;
    assert_eq!(first.fires.len(), cap);
    let settle = SpaceReissue {
        op: settle_after(&x, &[], vec![pending_of(&on_c), pending_of(&on_d)], None),
        cause: ReissueCause::RoundCap,
    };
    assert_eq!(
        first.reissue,
        Some(settle.clone()),
        "the send of b is stored, so nothing resumes"
    );
    rig.log().await;
    let job = rig.ok(settle.op.clone()).await;
    assert_eq!(job.fires.len(), cap);
    assert_eq!(
        io_produces(&rig.log().await)[..2],
        [stored_produce(&on_c), stored_produce(&on_d)],
        "the job sends its pending records first, in order"
    );
    assert_eq!(
        job.reissue,
        Some(settle.clone()),
        "the stop of the job carries the two sends again"
    );
    let last = rig.ok(settle.op).await;
    assert_eq!(last.fires.len(), 2);
    assert_eq!(last.reissue, None);
    assert_eq!(fired_names(&[first, job, last]), names("p", 2 * cap + 2));
    assert_peek_chain_ends(&rig, [&b, &c, &d]).await;
}

/// Decision 49 before the plan (joins plan 4.2, 4.10, DV-2, DV-5). An operation on a collection
/// that owes fires settles that collection first, on the one fire count of its transaction. A
/// first send on `q1` stops at the cap and leaves pairs of `q2` owed. A send on `q2` then comes
/// before the settle job. The join u on `[q2, q3]` keeps `q2` joined.
///
/// * The settle before the plan fills the transaction. The send re-issues one `SettleOp` with
///   the dirty collection `q3` as its seed and the send as its resume. The job fires the rest,
///   and the send then runs as the program issued it.
/// * The settle before the plan ends exactly at the cap: the plan does not open, so u does not
///   fire as fire `FIRE_ROUND_CAP + 1`. Nothing is owed, so the persistent send re-issues alone,
///   with the order of its first round (decision 43). Its next round fires u and stores the send
///   at that order.
#[tokio::test]
async fn space_joins_settle_before_cap_resumes_the_operation() {
    let cap = FIRE_ROUND_CAP;
    let (q1, q2, q3) = (ch("q1"), ch("q2"), ch("q3"));
    let owed = |from: usize, to: usize| -> Vec<(String, Vec<Par>)> {
        (from..to)
            .map(|index| delivery(&format!("v{index}"), &[small(index + 1)]))
            .collect()
    };

    let rig = Rig::new(SpaceInner::Queue).await;
    capped_queue(&rig, 2 * cap + 2, 2 * cap + 1, true).await;
    assert_eq!(rig.ok(put(&q1, -5)).await.fires.len(), cap);
    rig.log().await;
    let send = send_salted(&q2, 99, 3);
    let outcome = rig.ok(SpaceOp::Produce(send.clone())).await;
    assert_eq!(
        fired(&outcome),
        owed(cap - 1, 2 * cap - 1),
        "the settle before the plan fills the transaction"
    );
    let resume = SpaceReissue {
        op: SpaceOp::Produce(send.clone()),
        cause: ReissueCause::RoundCap,
    };
    assert_eq!(
        outcome.reissue,
        Some(SpaceReissue {
            op: settle_after(&send, &[&q3], Vec::new(), Some(resume.clone())),
            cause: ReissueCause::RoundCap,
        })
    );
    let job = rig
        .ok(outcome.reissue.expect("the cap re-issues a settle").op)
        .await;
    assert_eq!(fired(&job), owed(2 * cap - 1, 2 * cap + 1));
    assert_eq!(
        job.reissue,
        Some(resume.clone()),
        "the job resumes the send"
    );
    let last = rig.ok(resume.op).await;
    assert!(last.fires.is_empty() && last.reissue.is_none());
    assert_eq!(rig.values(&q2).await, vec![99]);
    assert_eq!(rig.join_waiters(&[&q2, &q3]).await.len(), 1, "u waits");

    let rig = Rig::new(SpaceInner::Queue).await;
    capped_queue(&rig, 2 * cap, 2 * cap - 1, true).await;
    assert!(rig.ok(put(&q3, 7)).await.fires.is_empty());
    assert_eq!(rig.ok(put(&q1, -5)).await.fires.len(), cap);
    let counter = rig.counter(&q2).await;
    let send = ProduceOp {
        persistent: true,
        ..send_salted(&q2, -1, 4)
    };
    let outcome = rig.ok(SpaceOp::Produce(send.clone())).await;
    assert_eq!(
        fired(&outcome),
        owed(cap - 1, 2 * cap - 1),
        "the plan does not open at the cap, so u does not fire"
    );
    let resume = SpaceReissue {
        op: SpaceOp::Produce(ProduceOp {
            reissued_ord: Some(counter),
            ..send.clone()
        }),
        cause: ReissueCause::RoundCap,
    };
    assert_eq!(
        outcome.reissue,
        Some(resume.clone()),
        "nothing is owed, so the send re-issues alone"
    );
    let last = rig.ok(resume.op).await;
    assert_eq!(fired(&last), vec![delivery("u", &[7])]);
    assert_eq!(last.reissue, None);
    assert_eq!(
        rig.records(&q2).await,
        vec![(counter, None, vec![int(-1)], true)],
        "the send keeps the order of its first round"
    );
    assert_eq!(rig.counter(&q2).await, counter + 2, "each round bumps q2");
    assert!(rig.records(&q3).await.is_empty());
    assert!(rig.join_waiters(&[&q2, &q3]).await.is_empty());
}

/// Runs `op` and its re-issues as one chain in `rig`, and checks the bound of decision 49: no job
/// fires more than `FIRE_ROUND_CAP` times. It returns the outcome of each job.
async fn capped_chain(rig: &Rig, op: SpaceOp, label: &str) -> Vec<SpaceOutcome> {
    let outcomes = rig.chain(op).await;
    let counts = fire_counts(&outcomes);
    assert!(
        counts.iter().all(|count| *count <= FIRE_ROUND_CAP),
        "{label}: the fires of each job: {counts:?}"
    );
    outcomes
}

/// Decision 49 (joins plan 4.9): no transaction of the join engine makes more than
/// `FIRE_ROUND_CAP` fires. Each case owes more fires than the cap. Every job of its chain stays
/// at or below the cap, some job reaches it, and the chain fires every waiter once.
///
/// * A queue cascade and a stack cascade: lifts and re-offers after a join fire.
/// * A persistent send on a joined bag, set and priority queue, which fires one waiter after
///   another. The bag owes nothing at the stop and re-issues the send. The set and the priority
///   queue cascade, so they re-issue a settle job with the send as its resume.
/// * A peek chain of second sends on a bag.
/// * A persistent join on a queue, which fires one record after another.
/// * A settle before the plan that ends exactly at the cap, while the operation could fire.
#[tokio::test]
async fn space_joins_txn_fires_bounded_at_the_cap() {
    let cap = FIRE_ROUND_CAP;
    let rig = Rig::new(SpaceInner::Queue).await;
    capped_queue(&rig, 3 * cap, 3 * cap - 1, false).await;
    let outcomes = capped_chain(&rig, put(&ch("q1"), -5), "queue cascade").await;
    assert_eq!(fire_counts(&outcomes), vec![cap, cap, cap]);
    let mut expected = names("v", 3 * cap - 1);
    expected.push("w".to_string());
    expected.sort();
    assert_eq!(fired_names(&outcomes), expected);

    let rig = Rig::new(SpaceInner::Stack).await;
    let (s1, s2) = (ch("s1"), ch("s2"));
    for index in 0..2 * cap {
        rig.ok(take(&s2, &format!("v{index}"), any())).await;
    }
    rig.ok(joined(join(&[(&s1, any()), (&s2, any())], "w")))
        .await;
    for value in 0..=2 * cap {
        assert!(rig.ok(put(&s2, small(value))).await.fires.is_empty());
    }
    let outcomes = capped_chain(&rig, put(&s1, -5), "stack cascade").await;
    assert_eq!(fire_counts(&outcomes), vec![cap, cap, 1]);
    let mut expected = names("v", 2 * cap);
    expected.push("w".to_string());
    expected.sort();
    assert_eq!(fired_names(&outcomes), expected);

    for inner in [SpaceInner::Bag, SpaceInner::Set, SpaceInner::PriorityQueue] {
        let rig = Rig::new(inner).await;
        let (p, z) = (ch("p"), ch("z"));
        for index in 0..2 * cap + 3 {
            rig.ok(take(&p, &format!("r{index}"), any())).await;
        }
        rig.ok(joined(join(&[(&p, any()), (&z, any())], "j"))).await;
        let x = ProduceOp {
            persistent: true,
            params: match inner {
                SpaceInner::PriorityQueue => priority(0),
                _ => BTreeMap::new(),
            },
            ..send_salted(&p, 1, 1)
        };
        let label = format!("a persistent send on {}", inner.as_str());
        let outcomes = capped_chain(&rig, SpaceOp::Produce(x), &label).await;
        let expected = match inner {
            SpaceInner::Bag => vec![cap, cap, 3],
            _ => vec![cap, 0, cap, 0, 3],
        };
        assert_eq!(fire_counts(&outcomes), expected, "{label}");
        assert_eq!(fired_names(&outcomes), names("r", 2 * cap + 3), "{label}");
        assert_eq!(rig.values(&p).await, vec![1], "{label}");
        assert_eq!(rig.join_waiters(&[&p, &z]).await.len(), 1, "{label}");
    }

    let rig = Rig::new(SpaceInner::Bag).await;
    let [b, c, d] = group_ordered(["cap-d", "cap-e", "cap-f"]);
    let (_, _, x) = peek_chain(&rig, [&b, &c, &d], 2 * cap + 2).await;
    let outcomes = capped_chain(&rig, SpaceOp::Produce(x), "bag peek chain").await;
    assert_eq!(fire_counts(&outcomes), vec![cap, cap, 2]);
    assert_eq!(fired_names(&outcomes), names("p", 2 * cap + 2));

    let rig = Rig::new(SpaceInner::Queue).await;
    let (c, d) = (ch("c"), ch("d"));
    for value in 0..2 * cap + 3 {
        rig.ok(put(&c, small(value))).await;
    }
    rig.ok(put_forever(&d, 7)).await;
    let op = joined(persisting(join(&[(&c, any()), (&d, any())], "g")));
    let outcomes = capped_chain(&rig, op, "persistent join").await;
    assert_eq!(fire_counts(&outcomes), vec![cap, 0, cap, 0, 3]);
    assert!(rig.values(&c).await.is_empty());
    assert_eq!(rig.join_waiters(&[&c, &d]).await.len(), 1);

    let rig = Rig::new(SpaceInner::Queue).await;
    let (q1, q2, q3) = (ch("q1"), ch("q2"), ch("q3"));
    capped_queue(&rig, 2 * cap, 2 * cap - 1, true).await;
    rig.ok(put(&q3, 7)).await;
    assert_eq!(rig.ok(put(&q1, -5)).await.fires.len(), cap);
    let send = ProduceOp {
        persistent: true,
        ..send_salted(&q2, -1, 4)
    };
    let outcomes = capped_chain(
        &rig,
        SpaceOp::Produce(send),
        "settle before the plan at the cap",
    )
    .await;
    assert_eq!(fire_counts(&outcomes), vec![cap, 1]);
}

// ─── 7.2: Option P ───────────────────────────────────────────────────────────────────────────
//
// Changed by decision 90 (decision 69): Option P (decision 45) is removed with the legacy meter
// (DR-134). Its lone host calls cannot compare observer calls (see the note of `lone_calls_step`).

// Changed by decision 90 (decision 69): Option P (the legacy charges of decision 45) is removed with the legacy meter (DR-134).
//
// /// The legacy log entries of the charges of the staged operation `staged` (decision 45,
// /// Option P): the pre-charge, then the post-charges.
// fn charges_of(staged: &StagedCopy) -> Vec<Cost> {
//     split(match staged {
//         StagedCopy::Produce(op) => produce_charges(op).expect("the charges of a staged produce"),
//         StagedCopy::Consume(op) => consume_charges(op).expect("the charges of a staged consume"),
//     })
// }

// Changed by decision 90 (decision 69): only the retired lone host differential classified its
// staged operations (see the note of `lone_calls_step`).
//
// /// The shape of a staged operation of the Option P differential.
// #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
// enum Shape {
//     /// A take or a put of a counter: the touch of a collection.
//     Counter,
//     /// A store of a record: a send, a second send or a re-offer that fires nothing.
//     Store,
//     /// A single-bind waiter that parks.
//     Park,
//     /// A join waiter that parks.
//     JoinPark,
//     /// A consume-side join fire.
//     JoinFire,
//     /// A produce-side fire of a join waiter.
//     GroupFire,
//     /// A produce-side fire of a single-bind waiter: a re-offer, a send or a second send.
//     SingleFire,
//     /// A single-bind consume fire on a data channel: a `K_lift`, a `K_retire` or a receive.
//     Removal,
// }

// Changed by decision 90 (decision 69): only the retired lone host differential classified its
// staged operations (see the note of `lone_calls_step`).
//
// /// The shape of the staged operation `staged`. The counter channels of the collections are
// /// `counters`.
// fn shape_of(staged: &StagedCopy, counters: &[Par]) -> Shape {
//     match staged {
//         StagedCopy::Produce(op) if counters.contains(&op.channel) => Shape::Counter,
//         StagedCopy::Consume(op) if op.channels.iter().any(|channel| counters.contains(channel)) => {
//             Shape::Counter
//         }
//         StagedCopy::Produce(op) => match &op.outcome {
//             ProduceOutcome::Store => Shape::Store,
//             ProduceOutcome::Fire { channels, .. } if channels.len() >= 2 => Shape::GroupFire,
//             ProduceOutcome::Fire { .. } => Shape::SingleFire,
//         },
//         StagedCopy::Consume(op) => match (&op.outcome, op.channels.len() >= 2) {
//             (ConsumeOutcome::Park, true) => Shape::JoinPark,
//             (ConsumeOutcome::Park, false) => Shape::Park,
//             (ConsumeOutcome::Fire { .. }, true) => Shape::JoinFire,
//             (ConsumeOutcome::Fire { .. }, false) => Shape::Removal,
//         },
//     }
// }

// Disabled by decision 90 (decision 69): Option P (the legacy charges of decision 45) is removed with the legacy meter (DR-134).
//
// /// Runs `op` in the recording rig `rig`, whose host records into `record`, with a fresh legacy
// /// budget, and checks Option P (decision 45). The legacy log of the job is the concatenation of
// /// the charges of its committed staged operations, in staging order. A fresh host that runs one
// /// staged operation alone logs exactly the charges of that operation. It returns the shapes of
// /// the staged operations. The counter channels of the collections are `counters`.
// async fn option_p_step(
//     rig: &Rig,
//     record: &SharedRecord,
//     op: SpaceOp,
//     counters: &[Par],
// ) -> Vec<Shape> {
//     let budget = RuntimeBudget::new(Cost::create(LEGACY_PHLO, "rrs join option p"));
//     let _scope = budget.enter_legacy_scope();
//     let env = SpaceEnv {
//         budget: budget.clone(),
//         ..rig.env.clone()
//     };
//     let label = format!("{op:?}");
//     run(&FamilyAgent, &env, op)
//         .await
//         .unwrap_or_else(|error| panic!("the operation commits: {error:?}: {label}"));
//     let seen = std::mem::take(&mut *record.lock().expect("the record lock"));
//     let staged: Vec<&StagedCopy> = seen
//         .staged
//         .iter()
//         .filter(|(txn, _)| seen.commits.contains(txn))
//         .map(|(_, staged)| staged)
//         .collect();
//     let expected: Vec<Cost> = staged
//         .iter()
//         .flat_map(|staged| charges_of(staged))
//         .collect();
//     assert_eq!(budget.get_log(), expected, "the legacy log of {label}");
//     let mut shapes = Vec::with_capacity(staged.len());
//     for staged in staged {
//         let lone = match staged {
//             StagedCopy::Produce(op) => lone_host_produce(op).await,
//             StagedCopy::Consume(op) => lone_host_consume(op).await,
//         };
//         assert_eq!(lone, charges_of(staged), "a lone host call of {label}");
//         shapes.push(shape_of(staged, counters));
//     }
//     shapes
// }

// Disabled by decision 90 (decision 69): Option P (the legacy charges of decision 45) is removed with the legacy meter (DR-134).
//
// /// Runs the operations `ops` in a fresh recording rig of `inner` through [`option_p_step`] and
// /// returns the shapes of all their staged operations. `channels` are the inner channels of the
// /// scenario.
// async fn option_p_scenario(
//     inner: SpaceInner,
//     channels: &[&Par],
//     ops: Vec<SpaceOp>,
// ) -> BTreeSet<Shape> {
//     let (rig, record) = Rig::recording(inner, &budget()).await;
//     let counters: Vec<Par> = channels
//         .iter()
//         .map(|channel| counter_channel(SPACE, channel))
//         .collect();
//     let mut shapes = BTreeSet::new();
//     for op in ops {
//         shapes.extend(option_p_step(&rig, &record, op, &counters).await);
//     }
//     shapes
// }

// Disabled by decision 90 (decision 69): Option P (the legacy charges of decision 45) is removed with the legacy meter (DR-134).
// Its lone host calls cannot move to the observer calls (see the note of `lone_calls_step`).
//
// /// Option P for joins (decision 45, joins plan 4.12). Every operation that the join engine stages
// /// charges what `ChargingRSpace` charges for the equal host call. A recording host shows that the
// /// legacy log of each job equals the charges of its staged operations. Each staged operation alone
// /// on a fresh host logs its charges.
// ///
// /// * A queue: a join park, a produce-side group fire, a lift, a re-offer and the counters.
// /// * A stack: a consume-side join fire, whose lift and re-offer fire a receive.
// /// * A bag: a consume-side peek join and its second send. Also the A.9 case: a produce-side peek
// ///   fire whose second sends store one record and fire a join.
// /// * A set: a consume-side join fire and the `K_retire` of the other member of its class.
// #[tokio::test]
// async fn space_joins_legacy_charges_equal_host() {
//     let (q1, q2) = (ch("q1"), ch("q2"));
//     let queue = option_p_scenario(SpaceInner::Queue, &[&q1, &q2], vec![
//         put(&q2, 10),
//         joined(join(&[(&q1, any()), (&q2, any())], "w")),
//         take(&q2, "v", any()),
//         put(&q2, 20),
//         put(&q1, 1),
//     ])
//     .await;
//     for shape in [
//         Shape::Counter,
//         Shape::Store,
//         Shape::Park,
//         Shape::JoinPark,
//         Shape::GroupFire,
//         Shape::Removal,
//         Shape::SingleFire,
//     ] {
//         assert!(
//             queue.contains(&shape),
//             "the queue stages {shape:?}: {queue:?}"
//         );
//     }
//
//     let (c, d) = (ch("c"), ch("d"));
//     let stack = option_p_scenario(SpaceInner::Stack, &[&c, &d], vec![
//         put(&c, 1),
//         put(&c, 2),
//         take(&c, "v", exactly(1)),
//         put(&d, 5),
//         joined(join(&[(&c, exactly(2)), (&d, any())], "w")),
//     ])
//     .await;
//     for shape in [Shape::JoinFire, Shape::Removal, Shape::SingleFire] {
//         assert!(
//             stack.contains(&shape),
//             "the stack stages {shape:?}: {stack:?}"
//         );
//     }
//
//     let (a, b) = (ch("a"), ch("b"));
//     let (pb, pc) = forward_pair();
//     let ordered = ordered_joins(vec![
//         peeking(join(&[(&pb, any()), (&pc, any())], "p")),
//         join(&[(&pb, any()), (&pc, any())], "w"),
//     ]);
//     let mut ops = vec![
//         put_salted(&a, 1, 1),
//         put_forever(&b, 2),
//         joined(peeking(join(&[(&a, any()), (&b, any())], "q"))),
//         SpaceOp::Produce(send_salted(&pc, 2, 1)),
//     ];
//     ops.extend(ordered.into_iter().map(joined));
//     ops.push(SpaceOp::Produce(send_salted(&pb, 1, 1)));
//     let bag = option_p_scenario(SpaceInner::Bag, &[&a, &b, &pb, &pc], ops).await;
//     for shape in [
//         Shape::JoinFire,
//         Shape::Store,
//         Shape::JoinPark,
//         Shape::GroupFire,
//     ] {
//         assert!(bag.contains(&shape), "the bag stages {shape:?}: {bag:?}");
//     }
//
//     let (s, t) = (ch("s"), ch("t"));
//     let set = option_p_scenario(SpaceInner::Set, &[&s, &t], vec![
//         put_salted(&s, 1, 1),
//         put_salted(&s, 1, 2),
//         put(&t, 5),
//         joined(join(&[(&s, exactly(1)), (&t, any())], "r")),
//     ])
//     .await;
//     for shape in [Shape::JoinFire, Shape::Removal] {
//         assert!(set.contains(&shape), "the set stages {shape:?}: {set:?}");
//     }
// }

// Disabled by decision 90 (decision 69): a lone host cannot make the observer calls of the agent,
// because the calls name the history of the store of the agent. A run on 2026-10-26 found 9
// staged operations whose calls differ:
// * 5 differ only in the `times_repeated` of a COMM (lone 1, agent 0 or 2). The counts follow the
//   produces of the store of the agent since its last event log. A lone host stores each record once.
// * 2 differ only in the join groups of a start (lone none, agent 1 or 2). A lone host holds only the
//   waiter of the operation.
// * 2 `K_lift` consumes differ in the bound value. The agent binds the whole record. The
//   `PinnedPayloadMatcher` of a lone host binds the payload of a datum record.
// The legacy charges of Option P did not depend on these arguments. The exact observer-call
// differential of the joins is `space_joins_produce_side_peek_reproduces_in_receipt_order`, whose
// host program runs the whole history on one host.

// Disabled by decision 90 (decision 69): see the note of `lone_calls_step`.

// Disabled by decision 90 (decision 69): see the note of `lone_calls_step`.

// ─── 7.2: decision 46 ────────────────────────────────────────────────────────────────────────

/// The phlo schedule of a native deploy of the test.
fn native_schedule() -> PhloScheduleV1<'static> {
    PhloScheduleV1 {
        protocol_version: 6,
        network: b"native-test",
        shard: b"root",
        settlement_asset: b"REV",
        settlement_unit: b"atomic-REV",
        decimal_scale: 8,
        classes: NativePhloDimension::ALL
            .into_iter()
            .zip([0, 1, 1, 0])
            .zip([b"c".as_slice(), b"i", b"t", b"r"])
            .map(|((dimension, weight), name)| dimension.resource_class(name, weight))
            .collect(),
        actual_price: 3,
        compatibility_rule: native_resource_compatibility_rule(),
    }
}

/// The native runtime config of a deploy with the phlo limit 1,000,000.
fn native_config() -> NativeRuntimeConfig {
    let descriptor = native_schedule();
    let binding = PhloScheduleBinding::new(&descriptor, PhloGenesisPolicy::LIMITS)
        .expect("the test schedule binds");
    let selected = binding.schedule();
    let schedules = [selected];
    let owner_ceilings = [3];
    let controls = check_phlo_controls(
        selected.environment,
        0,
        u64::MAX,
        SignedPhloControls {
            limit: 1_000_000,
            price_ceiling: 3,
            required_owner_ceilings: &owner_ceilings,
            permitted_schedules: &schedules,
        },
        selected,
        1_000_000,
    )
    .expect("the test controls check");
    let contract =
        NativePhloExecutionContract::new(controls, &binding, NativeBoundSource::Certificate)
            .expect("the test contract builds");
    NativeRuntimeConfig::new(contract, NativeBudgetTraceLimits {
        attempts: 100_000,
        path_segments: 4096,
        regions: NativePhloRegionLimits {
            regions: 1000,
            encoded_authority_bytes: 10_000_000,
        },
    })
}

// Disabled by decision 47: native execution runs joins, so no operation fails with
// `SpacesUnsupportedInNativeExecution`. Its successor is
// `space_joins_native_play_commits_as_legacy`. Decision 90 (C6) restates that successor as
// `space_joins_native_play_commits_and_legacy_fails_closed`.
//
// ```
// /// Decision 46: native play and native replay fail closed before any space work. A join, a send
// /// on a joined collection and a settle job with a cascade fail with
// /// `SpacesUnsupportedInNativeExecution`. They fail in a host runtime whose deploy runs natively,
// /// and in the native replay runtime. The recording host sees no transaction, and the space keeps
// /// its events and its root. A deploy routes a join to this gate with no read of the space. The
// /// route of a channel is a function of its name (`reified/route.rs`).
// #[tokio::test]
// async fn space_joins_native_execution_fails_closed() {
//     let (rig, record) = Rig::recording(SpaceInner::Queue, &budget()).await;
//     let (a, b, c) = (ch("a"), ch("b"), ch("c"));
//     rig.ok(put(&a, 1)).await;
//     rig.ok(put(&b, 2)).await;
//     rig.ok(joined(join(&[(&a, exactly(9)), (&c, any())], "parked")))
//         .await;
//     rig.log().await;
//     let before = rig.root().await;
//     *record.lock().expect("the record lock") = HostRecord::default();
//     let native = RuntimeBudget::new(Cost::create(1_000_000, "rrs join native"));
//     native.set_deploy_signature_funded(
//         b"rrs-join-native",
//         funding_sig_compound(&[&1u64.to_be_bytes()]),
//     );
//     native
//         .reset_for_native_execution(native_config())
//         .expect("the budget enters native execution");
//     assert!(native.native_execution_active());
//     let send = send_salted(&a, 3, 0);
//     let ops = [
//         joined(join(&[(&a, any()), (&b, any())], "w")),
//         SpaceOp::Produce(send.clone()),
//         settle_after(&send, &[&c], Vec::new(), None),
//     ];
//     let (recording, _) = RecordingHost::new(rig.host.clone());
//     let recorded = Arc::clone(&record);
//     let host_runtime = runtime::for_host(recording, native);
//     for reified in [host_runtime, runtime::native_replay()] {
//         for op in &ops {
//             match reified.run_op(Arc::new(FamilyAgent), op.clone()).await {
//                 Err(InterpreterError::ReduceError(message)) => assert_eq!(
//                     message,
//                     "reified:SpacesUnsupportedInNativeExecution: native execution does not \
//                      support Reified RSpaces",
//                     "{op:?}"
//                 ),
//                 other => panic!("a space operation ran in native execution: {other:?}: {op:?}"),
//             }
//         }
//     }
//     assert!(recorded.lock().expect("the record lock").begins.is_empty());
//     assert!(rig.log().await.is_empty(), "no space work");
//     assert_eq!(rig.root().await, before);
// }
// ```

// Changed by decision 90 (C6): the test that compared native play with legacy execution names
// its native form. The legacy text follows.
//
// /// The outcomes of the operations of `space_joins_native_play_commits_as_legacy`, the events of
// /// the play space, its root, and the number of transactions that the host of the runtime saw.
/// The outcomes of the operations of `space_joins_native_play_commits_and_legacy_fails_closed`,
/// the events of the play space, its root, and the number of transactions that the host of the
/// runtime saw.
type JoinRun = (Vec<SpaceOutcome>, Vec<Event>, Blake2b256Hash, usize);

/// Runs a join, a send on a joined collection and a settle job with a cascade over a fresh queue
/// space, in a host runtime whose deploy charges `deploy`. The space holds 1 on `a`, 2 on `b`
/// and a parked join on `a` and `c` before the operations.
async fn join_run(deploy: RuntimeBudget) -> JoinRun {
    let (rig, _) = Rig::recording(SpaceInner::Queue, &budget()).await;
    let (a, b, c) = (ch("a"), ch("b"), ch("c"));
    rig.ok(put(&a, 1)).await;
    rig.ok(put(&b, 2)).await;
    rig.ok(joined(join(&[(&a, exactly(9)), (&c, any())], "parked")))
        .await;
    rig.log().await;
    let send = send_salted(&a, 3, 0);
    let ops = [
        joined(join(&[(&a, any()), (&b, any())], "w")),
        SpaceOp::Produce(send.clone()),
        settle_after(&send, &[&c], Vec::new(), None),
    ];
    let (recording, record) = RecordingHost::new(rig.host.clone());
    let reified = runtime::for_host(recording, deploy);
    let mut outcomes = Vec::with_capacity(ops.len());
    for op in ops {
        let label = format!("{op:?}");
        outcomes.push(
            reified
                .run_op(Arc::new(FamilyAgent), op, OpOrigin::Program)
                .await
                .unwrap_or_else(|error| panic!("the operation commits: {error:?}: {label}")),
        );
    }
    let log = rig.log().await;
    let root = rig.root().await;
    let transactions = record.lock().expect("the record lock").begins.len();
    (outcomes, log, root, transactions)
}

// Changed by decision 90 (C6, decision 69): spaces have one execution model, so a host runtime
// outside native execution fails closed, and the comparison of native play with legacy execution
// has no legacy side. The legacy test follows.
//
// /// Decision 47: native play runs joins. A join, a send on a joined collection and a settle job
// /// with a cascade commit in a host runtime whose deploy runs natively as they commit in legacy
// /// execution: the same outcomes, the same events and the same root, with as many transactions.
// /// A native replay runtime that is not bound yet fails each operation closed with
// /// `RuntimeUnavailable`.
// #[tokio::test]
// async fn space_joins_native_play_commits_as_legacy() {
//     let native = RuntimeBudget::new(Cost::create(1_000_000, "rrs join native"));
//     native.set_deploy_signature_funded(
//         b"rrs-join-native",
//         funding_sig_compound(&[&1u64.to_be_bytes()]),
//     );
//     native
//         .reset_for_native_execution(native_config())
//         .expect("the budget enters native execution");
//     assert!(native.native_execution_active());
//     let (legacy_outcomes, legacy_log, legacy_root, legacy_transactions) = join_run(budget()).await;
//     let (native_outcomes, native_log, native_root, native_transactions) = join_run(native).await;
//     assert_eq!(native_outcomes, legacy_outcomes);
//     assert_eq!(native_log, legacy_log);
//     assert_eq!(native_root, legacy_root);
//     assert!(legacy_transactions > 0);
//     assert_eq!(native_transactions, legacy_transactions);
//
//     let (rig, _) = Rig::recording(SpaceInner::Queue, &budget()).await;
//     let a = ch("a");
//     rig.ok(put(&a, 1)).await;
//     rig.log().await;
//     let before = rig.root().await;
//     let unbound = runtime::native_replay();
//     let send = send_salted(&a, 3, 0);
//     for op in [
//         joined(join(&[(&a, any()), (&ch("b"), any())], "w")),
//         SpaceOp::Produce(send.clone()),
//         settle_after(&send, &[&ch("c")], Vec::new(), None),
//     ] {
//         match unbound
//             .run_op(Arc::new(FamilyAgent), op.clone(), OpOrigin::Program)
//             .await
//         {
//             Err(InterpreterError::ReduceError(message)) => assert!(
//                 message.starts_with("reified:RuntimeUnavailable: "),
//                 "{message}: {op:?}"
//             ),
//             other => panic!("an unbound native replay runtime ran {op:?}: {other:?}"),
//         }
//     }
//     assert!(rig.log().await.is_empty(), "no space work");
//     assert_eq!(rig.root().await, before);
// }

/// Decision 47: native play runs joins. A join, a send on a joined collection and a settle job
/// with a cascade commit in a host runtime whose deploy runs natively. The join waits behind the
/// older join on `a`, so no operation fires a waiter. The play log parks the join on `a` and `b`,
/// and the host sees one transaction for each operation. Decision 90 (C6): a host runtime whose
/// deploy is not in native execution and a native replay runtime that is not bound yet fail
/// each operation closed with `RuntimeUnavailable`. They open no transaction, do no space work
/// and keep the root.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_joins_native_play_commits_and_legacy_fails_closed() {
    let native = RuntimeBudget::new(Cost::create(1_000_000, "rrs join native"));
    native.set_deploy_signature_funded(
        b"rrs-join-native",
        funding_sig_compound(&[&1u64.to_be_bytes()]),
    );
    native
        .reset_for_native_execution(native_config())
        .expect("the budget enters native execution");
    assert!(native.native_execution_active());
    let (outcomes, log, _, transactions) = join_run(native).await;
    assert!(
        outcomes.iter().all(|outcome| outcome.fires.is_empty()),
        "the join waits behind the older join on `a`, and the send and the settle job fire no \
         waiter: {outcomes:?}"
    );
    let joins = |event: &Event| match event {
        Event::IoEvent(IOEvent::Consume(consume)) => consume.channel_hashes.len() == 2,
        Event::Comm(comm) => comm.consume.channel_hashes.len() == 2,
        Event::IoEvent(IOEvent::Produce(_)) => false,
    };
    assert!(
        matches!(
            log.iter()
                .filter(|event| joins(event))
                .collect::<Vec<_>>()
                .as_slice(),
            [Event::IoEvent(IOEvent::Consume(_))]
        ),
        "the play log parks the join on its two collections and holds no join COMM"
    );
    assert_eq!(
        transactions, 3,
        "the host sees one transaction for each operation"
    );

    let (rig, _) = Rig::recording(SpaceInner::Queue, &budget()).await;
    let a = ch("a");
    rig.ok(put(&a, 1)).await;
    rig.log().await;
    let before = rig.root().await;
    let send = send_salted(&a, 3, 0);
    let ops = [
        joined(join(&[(&a, any()), (&ch("b"), any())], "w")),
        SpaceOp::Produce(send.clone()),
        settle_after(&send, &[&ch("c")], Vec::new(), None),
    ];
    let (recording, record) = RecordingHost::new(rig.host.clone());
    for reified in [
        runtime::for_host(recording, budget()),
        runtime::native_replay(),
    ] {
        for op in &ops {
            match reified
                .run_op(Arc::new(FamilyAgent), op.clone(), OpOrigin::Program)
                .await
            {
                Err(InterpreterError::ReduceError(message)) => assert!(
                    message.starts_with("reified:RuntimeUnavailable: "),
                    "{message}: {op:?}"
                ),
                other => panic!("a runtime outside native execution ran {op:?}: {other:?}"),
            }
        }
    }
    assert!(
        record.lock().expect("the record lock").begins.is_empty(),
        "no transaction"
    );
    assert!(rig.log().await.is_empty(), "no space work");
    assert_eq!(rig.root().await, before);
}

// ─── 7.2: decision 50 ────────────────────────────────────────────────────────────────────────

// Changed by decision 118: the trace of F-JF-1 is vacuous (NoSetCascadeUnsafe.cfg:8-14). Rule
// (a) of decision 118 makes W fire at its arrival, so the case below replaces this one. The case
// before:
// /// One case of `space_joins_set_join_cascade_fires_exposed_waiter`. The set holds a1 and the
// /// persistent a2 of the class 1 and b of the class 2, in the canonical order a1, b, a2. With
// /// `second_member`, the set also holds b2 of the class 2 after b.
// async fn set_cascade_case(second_member: bool) {
//     let s = ch("s");
//     let entries: Vec<(i64, bool)> = match second_member {
//         false => vec![(1, false), (2, false), (1, true)],
//         true => vec![(1, false), (2, false), (2, false), (1, true)],
//     };
//     let members = ordered_sends(&s, &entries);
//     let (a1, b, a2) = (&members[0], &members[1], &members[members.len() - 1]);
//     let w = join(&[(&s, any()), (&s, exactly(1))], "w");
//     let v = receive_on(SPACE, &s, "v", exactly(1));
//     let mut script: Vec<SpaceOp> = members.iter().cloned().map(SpaceOp::Produce).collect();
//     script.push(joined(w.clone()));
//     script.push(SpaceOp::Consume(v.clone()));
//
//     let rig = Rig::new(SpaceInner::Set).await;
//     for member in &members {
//         rig.ok(SpaceOp::Produce(member.clone())).await;
//     }
//     assert!(
//         rig.ok(joined(w.clone())).await.fires.is_empty(),
//         "the greedy search parks W"
//     );
//     rig.log().await;
//     let outcome = rig.ok(SpaceOp::Consume(v.clone())).await;
//     assert_eq!(
//         fired(&outcome),
//         vec![delivery("v", &[]), delivery("w", &[2])],
//         "W fires in the transaction of V"
//     );
//     assert_eq!(outcome.reissue, None);
//     let log = rig.log().await;
//     let lift_random =
//         RandomPurpose::Lift.derive(&Blake2b512Random::from_bytes(&v.body.random_state));
//     let lift = Consume::create(
//         &vec![data(&s)],
//         &vec![lift_pattern()],
//         &retire_continuation(&record_hash(b), &lift_random, None),
//         false,
//     );
//     let mut expected = vec![lift];
//     if second_member {
//         expected.push(retire_consume(
//             &s,
//             &b.data.random_state,
//             None,
//             &record_hash(&members[2]),
//         ));
//     }
//     assert_eq!(
//         consumes_on(&log, &data(&s)),
//         expected,
//         "the K_lift of b by its source hash, and the K_retire of b2 when b2 exists"
//     );
//     let comms: Vec<&COMM> = log
//         .iter()
//         .filter_map(|event| match event {
//             Event::Comm(comm) => Some(comm),
//             _ => None,
//         })
//         .collect();
//     let bound_of = |comm: &COMM| {
//         let mut bound: Vec<Blake2b256Hash> = comm
//             .produces
//             .iter()
//             .map(|produce| produce.hash.clone())
//             .collect();
//         bound.sort();
//         bound
//     };
//     let sorted = |mut hashes: Vec<Blake2b256Hash>| {
//         hashes.sort();
//         hashes
//     };
//     let class_key = rspace_plus_plus::rspace::hashing::stable_hash_provider::hash(&data(&s));
//     assert_eq!(
//         comms[0].consume.channel_hashes,
//         vec![class_key.clone(), class_key],
//         "V fires first, as the class fire [D;2]"
//     );
//     assert_eq!(
//         bound_of(comms[0]),
//         sorted(vec![record_hash(a1), record_hash(a2)]),
//         "V binds the whole class 1: a1 and the persistent a2"
//     );
//     let w_comm = comms
//         .iter()
//         .find(|comm| comm.consume.hash == join_waiter_hash(&w))
//         .expect("W fires in the transaction of V");
//     assert_eq!(
//         bound_of(w_comm),
//         sorted(vec![record_hash(b), record_hash(a2)]),
//         "W binds the re-offered b and the persistent a2"
//     );
//     assert_eq!(rig.records(&s).await, vec![(0, None, vec![int(1)], true)]);
//     assert!(rig.join_waiters(&[&s, &s]).await.is_empty());
//     assert!(rig.waiters(&s).await.is_empty());
//     replay_case(SpaceInner::Set, script).await;
// }
/// One case of `space_joins_set_join_cascade_fires_exposed_waiter`. The set holds a1 and the
/// persistent a2 of the class 1 and b of the class 2, in the canonical order a1, b, a2. With
/// `second_member`, the set also holds b2 of the class 2 after b. W fires at its arrival with a1
/// and a2, with no `K_lift` and no `K_retire`. V then fires with a2 in its own transaction,
/// through a one-bind consume on `[D(s)]`.
async fn set_cascade_case(second_member: bool) {
    let s = ch("s");
    let entries: Vec<(i64, bool)> = match second_member {
        false => vec![(1, false), (2, false), (1, true)],
        true => vec![(1, false), (2, false), (2, false), (1, true)],
    };
    let members = ordered_sends(&s, &entries);
    let (a1, a2) = (&members[0], &members[members.len() - 1]);
    let w = join(&[(&s, any()), (&s, exactly(1))], "w");
    let v = receive_on(SPACE, &s, "v", exactly(1));
    // Retired by decision 90 (C6, step D): the replay half ran through the rigged replay modes,
    // which retire (decision 125). `space_joins_replay_equal`, `pathmap_replay_equal`. The text
    // before:
    // ```
    // let mut script: Vec<SpaceOp> = members.iter().cloned().map(SpaceOp::Produce).collect();
    // script.push(joined(w.clone()));
    // script.push(SpaceOp::Consume(v.clone()));
    // ```
    let class_key = rspace_plus_plus::rspace::hashing::stable_hash_provider::hash(&data(&s));
    let comms_of = |log: &[Event]| -> Vec<COMM> {
        log.iter()
            .filter_map(|event| match event {
                Event::Comm(comm) => Some(comm.clone()),
                _ => None,
            })
            .collect()
    };

    let rig = Rig::new(SpaceInner::Set).await;
    for member in &members {
        rig.ok(SpaceOp::Produce(member.clone())).await;
    }
    rig.log().await;
    let arrival = rig.ok(joined(w.clone())).await;
    assert_eq!(
        fired(&arrival),
        vec![delivery("w", &[1])],
        "W fires at its arrival, with a1 at the bind any"
    );
    assert_eq!(arrival.reissue, None);
    let log = rig.log().await;
    assert!(
        consumes_on(&log, &data(&s)).is_empty(),
        "the fire of W makes no K_lift and no K_retire"
    );
    let [w_comm] = <[COMM; 1]>::try_from(comms_of(&log)).expect("one COMM, the fire of W");
    assert_eq!(
        w_comm.consume.channel_hashes,
        vec![class_key.clone(), class_key.clone()],
        "W fires on [D(s), D(s)]"
    );
    assert_eq!(
        bound_sources(&w_comm),
        sorted_hashes(vec![record_hash(a1), record_hash(a2)]),
        "W binds a1, which claims its record and its class, and the persistent a2, which claims \
         nothing"
    );

    let outcome = rig.ok(SpaceOp::Consume(v.clone())).await;
    assert_eq!(
        fired(&outcome),
        vec![delivery("v", &[])],
        "V fires in its own transaction"
    );
    assert_eq!(outcome.reissue, None);
    let log = rig.log().await;
    let [v_comm] = <[COMM; 1]>::try_from(comms_of(&log))
        .expect("one COMM, the fire of V, with no K_lift and no K_retire");
    assert_eq!(
        v_comm.consume.channel_hashes,
        vec![class_key],
        "V fires through a one-bind consume on [D(s)]"
    );
    assert_eq!(
        bound_sources(&v_comm),
        vec![record_hash(a2)],
        "V binds the persistent a2, the only member of the class 1"
    );
    assert_eq!(
        stored_sources(&rig, &s).await,
        sorted_hashes(members[1..].iter().map(record_hash).collect()),
        "a1 leaves, and b, b2 when it exists, and a2 stay"
    );
    let expected: Vec<AnyRecord> = match second_member {
        false => vec![
            (0, None, vec![int(1)], true),
            (0, None, vec![int(2)], false),
        ],
        true => vec![
            (0, None, vec![int(1)], true),
            (0, None, vec![int(2)], false),
            (0, None, vec![int(2)], false),
        ],
    };
    assert_eq!(rig.records(&s).await, expected);
    assert!(rig.join_waiters(&[&s, &s]).await.is_empty());
    assert!(rig.waiters(&s).await.is_empty());
    // Retired by decision 90 (C6, step D): the replay half ran through the rigged replay modes,
    // which retire (decision 125). `space_joins_replay_equal`, `pathmap_replay_equal`. The text
    // before:
    // replay_case(SpaceInner::Set, script).await;
}

// Changed by decision 118: the trace of F-JF-1 is vacuous (NoSetCascadeUnsafe.cfg:8-14). The doc
// before:
// /// Decision 50 (finding F-JF-1, joins plan 4.9). A fire on a joined set can remove the
// /// non-persistent first member of a class while a persistent member stays. Then the class moves to
// /// the place of the persistent member. The waiter that the move exposes fires in the same
// /// transaction.
// ///
// /// The first case is the computed trace of `set_join_class_shift_breaks_inv_scan`
// /// (`JoinCascade.v`) and of `NoSetCascadeUnsafe` (`JoinCascade.tla`). It has three sends on one
// /// set collection: a, b and a persistent a, in this canonical order. The join W on `[s, s]` with
// /// the patterns any and a parks. The greedy search gives the class of a to the first bind
// /// (decision 37). The receive V of a takes the class of a with the class fire `[D; 2]`. The first
// /// a leaves and the persistent a stays, so the class of a now sits after b. In the same
// /// transaction, the cascade lifts b with `K_lift`, whose continuation has the source hash of b as
// /// its key. It offers b again and fires W with b and the persistent a. The set holds the
// /// persistent a, and replay equals play.
// ///
// /// The second case adds a second member b2 of the class of b. The fire of W then retires b2 with
// /// `K_retire` and the random state of b (RETIRE-SET-BINDS).
/// Decision 50 (finding F-JF-1, joins plan 4.9), restated by decision 118. The trace of F-JF-1 is
/// vacuous (NoSetCascadeUnsafe.cfg:8-14): a set class with a persistent member never blocks as a
/// class, so the join of that trace fires at its arrival and never parks.
///
/// The first case has three sends on one set collection: a1, b and a persistent a2 of the class of
/// a1, in this canonical order. The join W on `[s, s]` with the patterns any and 1 fires at its
/// arrival under rule (a) of decision 118 (decision-c6.md section 3). The non-persistent a1
/// binds the first bind and claims its record and its class. The persistent a2 claims nothing,
/// and its class has a persistent member, so a2 binds the second bind. The fire makes no `K_lift`
/// and no `K_retire`, because the class of a1 has no other member that is not persistent. The
/// receive V of 1 then fires in its own transaction with a2, through a one-bind consume on
/// `[D(s)]`. The set ends with b and a2.
///
/// The second case adds a second member b2 of the class of b. The class of b stays whole, so the
/// set also keeps b2.
///
/// The Rust test of the NoSetCascadeUnsafe trace itself is
/// `set_cascade_fires_the_guarded_receive_that_a_vetoed_pick_parked` in
/// `rholang/tests/reified_native_random_picks.rs`.
#[tokio::test]
async fn space_joins_set_join_cascade_fires_exposed_waiter() {
    set_cascade_case(false).await;
    set_cascade_case(true).await;
}

// ─── 7.2: the parity of the classification ───────────────────────────────────────────────────

/// The join-free scripts of the classification parity (joins plan 4.1 and 7.2) and their runner.
/// The golden fires come from a capture that ran this module at the commit of the second wave,
/// with the support harness of that commit.
mod classification_scripts {
    use std::collections::BTreeMap;

    use crypto::rust::hash::blake2b512_random::Blake2b512Random;
    use models::rhoapi::{BindPattern, Par};
    use models::rust::space_factory_urn::{SpaceFactoryUrn, SpaceInner, SpaceOuter};
    use rholang::rust::interpreter::spaces::agent::{ConsumeOp, ProduceOp, SpaceOp};
    use rholang::rust::interpreter::spaces::disciplines::priority_queue::PRIORITY_PARAM;
    use rholang::rust::interpreter::spaces::families::FamilyAgent;
    use rholang::rust::interpreter::spaces::namespace::{Durability, NativeSpace, SID_LEN};
    use rholang::rust::interpreter::spaces::types::qualifier::SpaceQualifier;

    use crate::support::agent::{
        any, budget, config_of, create, creation, env_over, exactly, int, play, receive_on, run,
        send_on, string,
    };

    /// The space of every script.
    const SPACE: NativeSpace = NativeSpace {
        sid: [0x4B; SID_LEN],
        durability: Durability::Durable,
    };

    /// The inner channel of every script.
    fn channel() -> Par { string("rrs-join-classification") }

    /// A send of `value` at the level `level` (a priority queue only), with a random state that
    /// `salt` fixes.
    fn put(value: i64, level: Option<i64>, salt: u64, persistent: bool) -> SpaceOp {
        let mut op = send_on(SPACE, &channel(), value, persistent);
        op.data.random_state = Blake2b512Random::create_from_bytes(
            format!("rrs-join-golden {value} {salt}").as_bytes(),
        )
        .to_bytes();
        SpaceOp::Produce(ProduceOp {
            params: level
                .map(|level| BTreeMap::from([(PRIORITY_PARAM.to_string(), int(level))]))
                .unwrap_or_default(),
            ..op
        })
    }

    /// A receive whose body is `name`, with the pattern `pattern`, that peeks when `peek` holds.
    fn take(name: &str, pattern: BindPattern, peek: bool) -> SpaceOp {
        SpaceOp::Consume(ConsumeOp {
            peek,
            ..receive_on(SPACE, &channel(), name, pattern)
        })
    }

    /// The scripts: a label, the inner discipline of `hashmap:<inner>`, and the operations. Each
    /// script stores, fires on both sides, peeks, parks and sends a persistent record.
    pub fn scripts() -> Vec<(&'static str, SpaceInner, Vec<SpaceOp>)> {
        let head = |inner| {
            (inner, vec![
                put(1, None, 0, false),
                put(2, None, 0, false),
                take("t1", any(), false),
                take("p2", any(), true),
                take("t2", any(), false),
                take("t3", any(), false),
                put(3, None, 0, false),
                put(4, None, 0, true),
                take("t4", any(), false),
            ])
        };
        let (queue, queue_ops) = head(SpaceInner::Queue);
        let (stack, stack_ops) = head(SpaceInner::Stack);
        vec![
            ("queue", queue, queue_ops),
            ("stack", stack, stack_ops),
            ("bag", SpaceInner::Bag, vec![
                put(1, None, 0, false),
                put(2, None, 0, false),
                take("t2", exactly(2), false),
                take("t9", exactly(9), false),
                put(9, None, 0, false),
                take("p1", any(), true),
                put(4, None, 0, true),
                take("t4", exactly(4), false),
            ]),
            ("set", SpaceInner::Set, vec![
                put(1, None, 1, false),
                put(1, None, 2, false),
                put(2, None, 0, false),
                take("t1", exactly(1), false),
                take("t2", any(), false),
                take("t7", exactly(7), false),
                put(7, None, 0, false),
                put(3, None, 0, false),
                take("p3", any(), true),
            ]),
            ("cell", SpaceInner::Cell, vec![
                put(1, None, 0, false),
                take("t1", any(), false),
                take("t2", any(), false),
                put(2, None, 0, false),
                put(3, None, 0, false),
                take("p3", any(), true),
                take("t3", any(), false),
            ]),
            ("priorityqueue", SpaceInner::PriorityQueue, vec![
                put(1, Some(0), 0, false),
                put(2, Some(1), 0, false),
                take("ta", any(), false),
                take("tb", any(), false),
                take("tc", any(), false),
                put(3, Some(0), 0, false),
                put(4, Some(2), 0, false),
                take("p4", any(), true),
                take("t4", any(), false),
            ]),
        ]
    }

    /// Runs `script` on a fresh space of `hashmap:<inner>`, each operation in an environment of its
    /// own. It returns the number of fires of each operation.
    pub async fn fires(inner: SpaceInner, script: &[SpaceOp]) -> Vec<u64> {
        let host = play().await;
        let budget = budget();
        let env = env_over(host.clone(), &budget);
        let config = config_of(
            SPACE,
            SpaceOuter::HashMap.as_str(),
            inner.as_str(),
            SpaceQualifier::Default,
        );
        let family = SpaceFactoryUrn {
            outer: SpaceOuter::HashMap,
            inner,
        };
        create(&FamilyAgent, &env, creation(SPACE, config, family)).await;
        let mut fires = Vec::with_capacity(script.len());
        for op in script {
            let op_env = env_over(host.clone(), &budget);
            let outcome = run(&FamilyAgent, &op_env, op.clone())
                .await
                .unwrap_or_else(|error| panic!("a join-free operation commits: {error:?}: {op:?}"));
            fires.push(u64::try_from(outcome.fires.len()).expect("a small count"));
        }
        fires
    }
}

/// The number of fires of each operation of the classification scripts at the commit of the
/// second wave (`5b45574a9`), before the join engine. The capture harness of the batch ran the
/// module `classification_scripts` at that commit, with the host-work usage of each operation in
/// three more columns. DR-131 removed host work, so the table keeps the column of the fires.
const CLASSIFICATION_GOLDEN: &[(&str, u64)] = &[
    ("queue 0", 0),
    ("queue 1", 0),
    ("queue 2", 1),
    ("queue 3", 1),
    ("queue 4", 1),
    ("queue 5", 0),
    ("queue 6", 1),
    ("queue 7", 0),
    ("queue 8", 1),
    ("stack 0", 0),
    ("stack 1", 0),
    ("stack 2", 1),
    ("stack 3", 1),
    ("stack 4", 1),
    ("stack 5", 0),
    ("stack 6", 1),
    ("stack 7", 0),
    ("stack 8", 1),
    ("bag 0", 0),
    ("bag 1", 0),
    ("bag 2", 1),
    ("bag 3", 0),
    ("bag 4", 1),
    ("bag 5", 1),
    ("bag 6", 0),
    ("bag 7", 1),
    ("set 0", 0),
    ("set 1", 0),
    ("set 2", 0),
    ("set 3", 1),
    ("set 4", 1),
    ("set 5", 0),
    ("set 6", 1),
    ("set 7", 0),
    ("set 8", 1),
    ("cell 0", 0),
    ("cell 1", 1),
    ("cell 2", 0),
    ("cell 3", 1),
    ("cell 4", 0),
    ("cell 5", 1),
    ("cell 6", 1),
    ("priorityqueue 0", 0),
    ("priorityqueue 1", 0),
    ("priorityqueue 2", 1),
    ("priorityqueue 3", 1),
    ("priorityqueue 4", 0),
    ("priorityqueue 5", 1),
    ("priorityqueue 6", 0),
    ("priorityqueue 7", 1),
    ("priorityqueue 8", 1),
];

/// The parity guard of the classification (joins plan 4.1, risk table of section 10). On a
/// collection without joins, every join-free operation of every `hashmap` family fires as it fired
/// at the second wave, before the join engine. The scripts store, fire on both sides, peek, park
/// and send persistent records.
#[tokio::test]
async fn space_joins_classification_fires_as_check_joins() {
    let mut measured: Vec<(String, u64)> = Vec::with_capacity(CLASSIFICATION_GOLDEN.len());
    for (label, inner, script) in classification_scripts::scripts() {
        let fires = classification_scripts::fires(inner, &script).await;
        for (index, fired) in fires.into_iter().enumerate() {
            measured.push((format!("{label} {index}"), fired));
        }
    }
    let golden: Vec<(String, u64)> = CLASSIFICATION_GOLDEN
        .iter()
        .map(|(label, fired)| (label.to_string(), *fired))
        .collect();
    assert_eq!(measured, golden);
}
