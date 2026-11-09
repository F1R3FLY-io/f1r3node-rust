//! Integration tests of the priority queue discipline (P5b, decisions 11, 37, 38, 43, 45 and 49,
//! P5 design v3 A.8).
//!
//! The family arm `hashmap:priorityqueue` is not wired yet, so the tests run the agent
//! `GenericSpace<HashMapOuter, PriorityQueueDiscipline>` directly over an in-memory RSpace, with
//! the shared harness of the P5 suites (`support/`). They cover these rules:
//!
//! * Levels ascend, and the unprioritized level comes last (F6). Each level is head-only and FIFO
//!   (decision 11). There is no storage for each level (I-10).
//! * A receive with `priority = p` waits in the waiter queue of the level `p` and sees that level
//!   only. A receive without a priority waits in the unrestricted queue and takes the first
//!   level head, in ascending order, that matches its pattern. The guard is checked once on that
//!   head, and a refusal parks (decision 38). When one datum fits waiters of both kinds, the
//!   older waiter fires.
//! * The settle (F2) fires the fireable waiter-lane head with the least order first (readiness
//!   review S4), with a lift and a re-offer. The round cap of the transaction (I-21, decision 49)
//!   and divergent pairs (A10) re-issue.
//! * Rule E (decision 45): a consume-side peek restores the head at its order and level. A waiter
//!   that peeks leaves the datum in flight (decision 30).
//! * Decision 43: a re-issued element takes its FIFO place in its level, and a re-entered waiter
//!   keeps its seniority.
//! * One counter serves the whole priority queue (decision 31), so two branches race on it.
//!   Replay reproduces play, and the order survives a restart.
//! * The observer calls of the settle: each staged operation makes the observer calls of its host
//!   call (decision 90, decision 69). They replace the legacy charges of Option P (decision 45),
//!   which DR-134 deleted with the legacy meter.
//!
//! Each property below states an invariant of `PriorityQueue.v` or `PriorityQueue.tla`:
//!
//! | Property | Formal statement |
//! |---|---|
//! | `prop_pq_level_order` | `pq_level_order`, `pq_unprioritized_lowest` |
//! | `prop_pq_fifo_order_any_schedule` | `PqLevelOrder`, `pq_fifo_within_level`, `pq_head_blocks_within_level`, `pq_restricted_waiter_in_level_queue`, `pq_unrestricted_first_matching_level`, `NoRecordLost`, `pq_settle_keeps_unspent_records` |
//! | `prop_pq_no_pending_match_any_schedule` | `PqNoPendingMatch`, `pq_produce_preserves_inv`, `pq_consume_preserves_inv`, `pq_peek_restores_head` |
//! | `prop_pq_oldest_first_any_schedule` | `PqOldestFirst`, `pq_settle_oldest_first`, `pq_settle_order_matters` |
//! | `prop_pq_settle_bounded_any_schedule` | `pq_settle_terminates` |
//! | `prop_pq_txn_fires_bounded_at_the_cap` | `TxnFiresBounded`, `pq_settle_terminates` |
//! | `prop_pq_plan_permutation_invariant` | `pq_plan_permutation_invariant` |
//! | `prop_pq_reissue_keeps_ord_any_schedule` | `ReissueKeepsOrd`, `pq_reissue_takes_level_fifo_place`, `pq_reentered_waiter_keeps_seniority` |
//! | `prop_pq_priority_domain` | `pq_priority_domain` |
//! | `prop_pq_no_level_storage` | `pq_no_level_storage` |
//! | `prop_pq_record_slots_roundtrip` | `datum_priority_slot_roundtrip`, `continuation_policy_slot_roundtrip` (`RecordEnvelope.v`) |

#[path = "support/mod.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::cost_signature::Value as CostSignatureValue;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    BindPattern, CostAuthority, CostRegion, CostSignature, EEq, EVar, Expr, ListParWithRandom, Par,
    ParWithRandom, TaggedContinuation, Var,
};
use models::rust::space_factory_urn::{SpaceFactoryUrn, SpaceInner, SpaceOuter};
use models::rust::utils::new_gbool_par;
use proptest::arbitrary::any as arbitrary;
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;
use rholang::rust::interpreter::accounting::authority::AuthorityByteEventKind;
use rholang::rust::interpreter::accounting::byte_accounting::produce_introduction_identity;
// Changed by decision 90 (decision 69): the legacy meter, its budgets and the module
// `reified::legacy_charges` of Option P are deleted (DR-134). The settle differential records
// observer calls, and the harness names the staged produce over `Par` (`support::agent`).
// use rholang::rust::interpreter::accounting::costs::Cost;
// use rholang::rust::interpreter::accounting::RuntimeBudget;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::reified::costs::{ReissueCause, FIRE_ROUND_CAP};
// use rholang::rust::interpreter::reified::legacy_charges::{
//     consume_charges, produce_charges, ParSpaceConsume, ParSpaceProduce,
// };
use rholang::rust::interpreter::spaces::agent::{
    ConsumeOp, ProduceOp, QueryParams, SettleOp, SpaceCreation, SpaceEnv, SpaceOp, SpaceOutcome,
    SpaceReissue,
};
use rholang::rust::interpreter::spaces::counter::{counter_continuation, counter_pattern};
use rholang::rust::interpreter::spaces::disciplines::bag::BagDiscipline;
use rholang::rust::interpreter::spaces::disciplines::cell::CellDiscipline;
use rholang::rust::interpreter::spaces::disciplines::priority_queue::{
    Lane, Level, PriorityQueueDiscipline, PRIORITY_PARAM,
};
use rholang::rust::interpreter::spaces::disciplines::queue::QueueDiscipline;
use rholang::rust::interpreter::spaces::disciplines::stack::StackDiscipline;
use rholang::rust::interpreter::spaces::errors::SpaceError;
use rholang::rust::interpreter::spaces::factory::defaults::PRIORITIES_PARAM;
use rholang::rust::interpreter::spaces::generic::{
    Discipline, GenericSpace, OuterStore, ParWaiter, Side,
};
use rholang::rust::interpreter::spaces::namespace::{
    Durability, NativeSpace, RandomPurpose, SID_LEN,
};
use rholang::rust::interpreter::spaces::outer::hashmap::HashMapOuter;
use rholang::rust::interpreter::spaces::record;
use rholang::rust::interpreter::spaces::settle::{lift_continuation, lift_pattern};
use rholang::rust::interpreter::spaces::types::config::SpaceConfig;
use rholang::rust::interpreter::spaces::types::qualifier::SpaceQualifier;
// Retired by decision 90 (C6, step D): the retired replay test was the last user of `Checkpoint`.
// The text before:
// use rspace_plus_plus::rspace::checkpoint::Checkpoint;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider;
// Changed by decision 90 (decision 69): the staged take and lift of the retired Option P test
// built their candidates by hand.
// use rspace_plus_plus::rspace::internal::{ConsumeCandidate, Datum};
use rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex;
use rspace_plus_plus::rspace::merger::merging_logic::{
    conflict_reason, conflicts, NumberChannelsDiff,
};
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
// Changed by decision 90 (decision 69): the staged take and lift of the retired Option P test
// named their outcomes.
// use rspace_plus_plus::rspace::space_store_handle::{
//     incoming_candidate, ConsumeOutcome, ProduceOutcome,
// };
use rspace_plus_plus::rspace::space_store_handle::{incoming_candidate, ProduceOutcome};
use rspace_plus_plus::rspace::trace::event::{Consume, Event, IOEvent, Produce};
// Changed by decision 90 (C6, step D): the rigged replay modes retire, so this file no longer uses
// `play_and_replay`. The text before:
// ```
// use support::agent::{
//     any, body_name, budget, counter, counter_channel, create, creation, data_channel, env_over,
//     exactly, fired, int, key_of, named, play, play_and_replay, receive_on, run, send_on,
//     space_counter_channel, string, ParRSpace,
// };
// ```
use support::agent::{
    any, body_name, budget, counter, counter_channel, create, creation, data_channel, env_over,
    exactly, fired, int, key_of, named, play, receive_on, run, send_on, space_counter_channel,
    string, ParRSpace, ParSpaceProduce,
};
// Changed by decision 90 (decision 69): the hosts record their observer calls.
// use support::legacy::{charging_host, host_consume, host_produce, split};
use support::legacy::{
    host_consume, host_produce, lone_host_produce, observed_host, open_waiter,
    pinned_observed_host, Pin,
};
// Changed by decision 92: the host differential removes the footprint group of the space.
use support::observed::{observed, operations, without_footprint, Recorder};
use support::props::{check, pure_config, tokio_runtime};
use support::session::{with_space, Injections, Session};

/// The agent of `hashmap:priorityqueue`.
type Pq = GenericSpace<HashMapOuter, PriorityQueueDiscipline>;

/// The space of every test.
const SPACE: NativeSpace = NativeSpace {
    sid: [0x5C; SID_LEN],
    durability: Durability::Durable,
};

fn pq() -> Pq { Pq::default() }

/// The inner channel of the collection of most tests.
fn channel() -> Par { string("pq") }

/// The data channel `D(sid,k)` of the collection of `channel()`.
fn data() -> Par { data_channel(SPACE, &channel()) }

/// The counter channel `S(sid,k)` of the collection of `channel()`.
fn counter_ch() -> Par { counter_channel(SPACE, &channel()) }

/// The config map of the space with the level bound `priorities`.
fn pq_config(space: NativeSpace, priorities: Option<i64>) -> Par {
    let mut config = SpaceConfig::new(
        space.sid,
        HashMapOuter::NAME,
        PriorityQueueDiscipline::NAME,
        SpaceQualifier::Default,
    );
    if let Some(bound) = priorities {
        config
            .params
            .insert(PRIORITIES_PARAM.to_string(), int(bound));
    }
    config.to_par()
}

/// The creation of the priority queue space by the factory of `hashmap:priorityqueue`.
fn pq_creation(priorities: Option<i64>) -> SpaceCreation {
    creation(SPACE, pq_config(SPACE, priorities), SpaceFactoryUrn {
        outer: SpaceOuter::HashMap,
        inner: SpaceInner::PriorityQueue,
    })
}

struct Fixture {
    space: ParRSpace,
    env: SpaceEnv,
}

/// A play space that holds the priority queue space with the level bound `priorities`, with an
/// empty event log.
async fn fixture_with(priorities: Option<i64>) -> Fixture {
    let space = play().await;
    let env = env_over(space.clone(), &budget());
    create(&pq(), &env, pq_creation(priorities)).await;
    space.take_event_log().await;
    Fixture { space, env }
}

/// A play space that holds the priority queue space without a level bound.
async fn fixture() -> Fixture { fixture_with(None).await }

/// The named params of a send or of a query with the level `level`.
fn priority(level: i64) -> BTreeMap<String, Par> {
    BTreeMap::from([(PRIORITY_PARAM.to_string(), int(level))])
}

/// A send of `value` at the level `level`: `None` is the unprioritized level.
fn send_at(value: i64, level: Option<i64>) -> ProduceOp {
    let base = send_on(SPACE, &channel(), value, false);
    ProduceOp {
        params: level.map(priority).unwrap_or_default(),
        data: ListParWithRandom {
            random_state: Blake2b512Random::create_from_bytes(
                format!("pq send {value} {level:?}").as_bytes(),
            )
            .to_bytes(),
            ..base.data.clone()
        },
        ..base
    }
}

/// A persistent send (`!!`) of `value` at the level `level`.
fn send_forever_at(value: i64, level: Option<i64>) -> ProduceOp {
    ProduceOp {
        persistent: true,
        ..send_at(value, level)
    }
}

/// The query of a receive in the lane `lane`: `None` is the unrestricted queue.
fn lane_query(lane: Option<i64>) -> Option<QueryParams> {
    lane.map(|level| QueryParams {
        payload: Vec::new(),
        named: priority(level),
    })
}

/// A receive named `name` with the pattern `pattern` in the lane `lane`.
fn receive_at(name: &str, pattern: BindPattern, lane: Option<i64>) -> ConsumeOp {
    ConsumeOp {
        query: lane_query(lane),
        ..receive_on(SPACE, &channel(), name, pattern)
    }
}

/// A persistent receive (`<=`).
fn receive_forever_at(name: &str, pattern: BindPattern, lane: Option<i64>) -> ConsumeOp {
    ConsumeOp {
        persistent: true,
        ..receive_at(name, pattern, lane)
    }
}

/// A peek receive (`<<-`) of every value.
fn peek_at(name: &str, lane: Option<i64>) -> ConsumeOp {
    ConsumeOp {
        peek: true,
        ..receive_at(name, any(), lane)
    }
}

/// The guard `x == value` over the first bound variable.
fn bound_equals(value: i64) -> Par {
    let variable = Par {
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
                p1: Some(variable),
                p2: Some(int(value)),
            })),
        }],
        ..Par::default()
    }
}

/// `op` with the guard `x == value`.
fn guarded(op: ConsumeOp, value: i64) -> ConsumeOp {
    ConsumeOp {
        guard: Some(bound_equals(value)),
        ..op
    }
}

/// Runs `op` with the priority queue agent and returns its outcome. A failure fails the test.
async fn step(env: &SpaceEnv, op: SpaceOp) -> SpaceOutcome {
    run(&pq(), env, op).await.expect("the operation commits")
}

async fn produce(env: &SpaceEnv, op: ProduceOp) -> SpaceOutcome {
    step(env, SpaceOp::Produce(op)).await
}

async fn consume(env: &SpaceEnv, op: ConsumeOp) -> SpaceOutcome {
    step(env, SpaceOp::Consume(op)).await
}

/// The fire of the waiter `name` that binds the value `value`.
fn delivery(name: &str, value: i64) -> (String, Vec<Par>) { (name.to_string(), vec![int(value)]) }

/// The fire of the waiter `name` whose pattern is a literal, so it binds no value.
fn literal(name: &str) -> (String, Vec<Par>) { (name.to_string(), Vec::new()) }

/// A record as the tests read it: `(ord, level, payload, persistent)`.
type RecordRow = (i64, Option<i64>, Vec<Par>, bool);

/// A waiter as the tests read it: `(ord, lane, body name, persistent, peek)`.
type WaiterRow = (i64, Option<i64>, String, bool, bool);

fn rec(ord: i64, level: Option<i64>, value: i64, persistent: bool) -> RecordRow {
    (ord, level, vec![int(value)], persistent)
}

fn wtr(ord: i64, lane: Option<i64>, name: &str, persistent: bool, peek: bool) -> WaiterRow {
    (ord, lane, name.to_string(), persistent, peek)
}

/// The records on the data channel `data`, by order.
async fn records_on(space: &ParRSpace, data: &Par) -> Vec<RecordRow> {
    let mut records: Vec<_> = space
        .get_data(data)
        .await
        .into_iter()
        .map(|datum| {
            let decoded = record::unwrap_pq_datum(&datum.a).expect("a datum record");
            (
                decoded.ord,
                decoded.priority,
                decoded.datum.pars,
                datum.persist,
            )
        })
        .collect();
    records.sort_by_key(|(ord, ..)| *ord);
    records
}

async fn records(space: &ParRSpace) -> Vec<RecordRow> { records_on(space, &data()).await }

/// The waiters on the data channel `data`, by order.
async fn waiters_on(space: &ParRSpace, data: &Par) -> Vec<WaiterRow> {
    let mut waiters: Vec<_> = space
        .get_waiting_continuations(vec![data.clone()])
        .await
        .into_iter()
        .map(|waiter| {
            let parked = record::unwrap_pq_continuation(&waiter.continuation)
                .expect("a continuation record");
            (
                parked.ord,
                parked.priority,
                body_name(&parked.continuation),
                waiter.persist,
                !waiter.peeks.is_empty(),
            )
        })
        .collect();
    waiters.sort_by_key(|(ord, ..)| *ord);
    waiters
}

async fn waiters(space: &ParRSpace) -> Vec<WaiterRow> { waiters_on(space, &data()).await }

async fn counter_value(space: &ParRSpace) -> Vec<i64> { counter(space, &counter_ch()).await }

/// The name of a channel of the collection of `channel()` in a label.
fn name_of(hash: &Blake2b256Hash) -> &'static str {
    let named = [
        ("S", space_counter_channel(SPACE)),
        ("K", counter_ch()),
        ("D", data()),
    ];
    named
        .iter()
        .find(|(_, channel)| stable_hash_provider::hash(channel) == *hash)
        .map(|(name, _)| *name)
        .unwrap_or("?")
}

/// The events of a log as `kind channel` labels. A COMM with peeks is marked `peek`.
fn labels(log: &[Event]) -> Vec<String> {
    log.iter()
        .map(|event| match event {
            Event::IoEvent(IOEvent::Produce(produce)) => {
                format!("produce {}", name_of(&produce.channel_hash))
            }
            Event::IoEvent(IOEvent::Consume(consume)) => {
                let names: Vec<&str> = consume.channel_hashes.iter().map(name_of).collect();
                format!("consume {}", names.join(","))
            }
            Event::Comm(comm) => {
                let names: Vec<&str> = comm.consume.channel_hashes.iter().map(name_of).collect();
                match comm.peeks.is_empty() {
                    true => format!("comm {}", names.join(",")),
                    false => format!("comm {} peek", names.join(",")),
                }
            }
        })
        .collect()
}

/// The events of the counter bump of an operation on an open collection.
const BUMP: [&str; 3] = ["consume K", "comm K", "produce K"];

fn bumped(rest: &[&str]) -> Vec<String> {
    BUMP.iter()
        .chain(rest)
        .map(|label| label.to_string())
        .collect()
}

/// The produce events of a log on the data channel of `channel()`.
fn data_produces(log: &[Event]) -> Vec<Produce> {
    let data = stable_hash_provider::hash(&data());
    log.iter()
        .filter_map(|event| match event {
            Event::IoEvent(IOEvent::Produce(produce)) if produce.channel_hash == data => {
                Some(produce.clone())
            }
            _ => None,
        })
        .collect()
}

/// The consume events of a log on the data channel of `channel()`.
fn data_consumes(log: &[Event]) -> Vec<Consume> {
    let data = vec![stable_hash_provider::hash(&data())];
    log.iter()
        .filter_map(|event| match event {
            Event::IoEvent(IOEvent::Consume(consume)) if consume.channel_hashes == data => {
                Some(consume.clone())
            }
            _ => None,
        })
        .collect()
}

/// The random state of `K_lift` for an operation with the random state bytes `random_state`.
fn lift_random(random_state: &[u8]) -> Blake2b512Random {
    RandomPurpose::Lift.derive(&Blake2b512Random::from_bytes(random_state))
}

/// The consume event of the `K_lift` of the round with the counter value `counter`, the step
/// `step` and the operation random state `random_state`.
fn lift_consume(counter: i64, step: i64, random_state: &[u8]) -> Consume {
    Consume::create(
        &vec![data()],
        &vec![lift_pattern()],
        &lift_continuation(counter, step, &lift_random(random_state), None),
        false,
    )
}

/// The settle job that continues the settle after the send `op`.
fn settle_after_send(op: &ProduceOp) -> SettleOp {
    SettleOp {
        space: SPACE,
        key: key_of(&channel()),
        channel: channel(),
        random_state: op.data.random_state.clone(),
        authority: op.data.cost_authority.clone().unwrap_or_default(),
        introduction_authority: op.authority.clone(),
        cascade: None,
    }
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

/// `FIRE_ROUND_CAP` as an order.
fn cap() -> i64 { i64::try_from(FIRE_ROUND_CAP).expect("a small cap") }

/// Runs `op` and returns its error, which it must have. Nothing is staged, so the log stays
/// empty and the counter does not change.
async fn rejected(space: &ParRSpace, env: &SpaceEnv, op: SpaceOp) -> SpaceError {
    let counter_before = counter_value(space).await;
    let records_before = records(space).await;
    space.take_event_log().await;
    let error = run(&pq(), env, op)
        .await
        .expect_err("the operation is rejected");
    assert!(
        space.take_event_log().await.is_empty(),
        "a rejected operation stages nothing"
    );
    assert_eq!(counter_value(space).await, counter_before, "no bump");
    assert_eq!(records(space).await, records_before);
    error
}

// ─── Levels, lanes and head-only matching ────────────────────────────────────────────────────

#[tokio::test]
async fn space_disciplines_pq_ascending_levels() {
    let Fixture { space, env } = fixture().await;
    for (value, level) in [(30, None), (20, Some(2)), (0, Some(0)), (10, Some(1))] {
        assert_eq!(
            produce(&env, send_at(value, level)).await,
            SpaceOutcome::default()
        );
    }
    assert_eq!(records(&space).await, vec![
        rec(0, None, 30, false),
        rec(1, Some(2), 20, false),
        rec(2, Some(0), 0, false),
        rec(3, Some(1), 10, false),
    ]);
    // An unrestricted receive takes the levels in ascending order, the unprioritized one last.
    for (name, value) in [("a", 0), ("b", 10), ("c", 20), ("d", 30)] {
        assert_eq!(
            fired(&consume(&env, receive_at(name, any(), None)).await),
            vec![delivery(name, value)],
            "the receive {name} takes the head of the least level"
        );
    }
    assert!(records(&space).await.is_empty());
}

#[tokio::test]
async fn space_disciplines_pq_fifo_within_level() {
    let Fixture { space, env } = fixture().await;
    for value in 1..=3 {
        produce(&env, send_at(value, Some(1))).await;
    }
    for value in 4..=5 {
        produce(&env, send_at(value, Some(0))).await;
    }
    // The waiter of the level 1 sees that level only, and takes its oldest record.
    assert_eq!(
        fired(&consume(&env, receive_at("r", any(), Some(1))).await),
        vec![delivery("r", 1)]
    );
    for (name, value) in [("a", 4), ("b", 5), ("c", 2), ("d", 3)] {
        assert_eq!(
            fired(&consume(&env, receive_at(name, any(), None)).await),
            vec![delivery(name, value)],
            "FIFO inside each level"
        );
    }
    // The waiters of one lane fire in FIFO order too.
    for name in ["w1", "w2", "w3"] {
        assert_eq!(
            consume(&env, receive_at(name, any(), Some(2))).await,
            SpaceOutcome::default()
        );
    }
    for (value, name) in [(7, "w1"), (8, "w2"), (9, "w3")] {
        assert_eq!(fired(&produce(&env, send_at(value, Some(2))).await), vec![
            delivery(name, value)
        ]);
    }
    assert!(records(&space).await.is_empty());
    assert!(waiters(&space).await.is_empty());
}

#[tokio::test]
async fn space_disciplines_pq_head_blocks_within_level() {
    let Fixture { space, env } = fixture().await;
    produce(&env, send_at(1, Some(0))).await;
    produce(&env, send_at(2, Some(0))).await;
    // The head 1 of the level 0 blocks the receive of 2, although the record behind it matches.
    assert_eq!(
        consume(&env, receive_at("two", exactly(2), Some(0))).await,
        SpaceOutcome::default()
    );
    assert_eq!(records(&space).await, vec![
        rec(0, Some(0), 1, false),
        rec(1, Some(0), 2, false)
    ]);
    assert_eq!(waiters(&space).await, vec![wtr(
        2,
        Some(0),
        "two",
        false,
        false
    )]);
    // On the waiter side, the head of the lane of the level 5 blocks a datum that the waiter
    // behind it would take.
    consume(&env, receive_at("one", exactly(1), Some(5))).await;
    consume(&env, receive_at("all", any(), Some(5))).await;
    assert_eq!(
        produce(&env, send_at(2, Some(5))).await,
        SpaceOutcome::default()
    );
    assert_eq!(
        records(&space).await.last(),
        Some(&rec(5, Some(5), 2, false))
    );
    assert_eq!(waiters(&space).await.len(), 3);
}

#[tokio::test]
async fn space_disciplines_pq_lower_level_not_blocked() {
    let Fixture { space, env } = fixture().await;
    produce(&env, send_at(9, Some(0))).await;
    produce(&env, send_at(1, Some(1))).await;
    // The head 9 of the level 0 does not match, and it does not block the level 1.
    assert_eq!(
        fired(&consume(&env, receive_at("one", exactly(1), None)).await),
        vec![literal("one")]
    );
    assert_eq!(records(&space).await, vec![rec(0, Some(0), 9, false)]);
    // A waiter of another level does not see the blocked level at all.
    produce(&env, send_at(4, Some(3))).await;
    assert_eq!(
        fired(&consume(&env, receive_at("three", any(), Some(3))).await),
        vec![delivery("three", 4)]
    );
    assert_eq!(records(&space).await, vec![rec(0, Some(0), 9, false)]);
}

#[tokio::test]
async fn space_disciplines_pq_restricted_waiter_waits_in_level_queue() {
    let Fixture { space, env } = fixture().await;
    assert_eq!(
        consume(&env, receive_at("w", any(), Some(1))).await,
        SpaceOutcome::default()
    );
    // The waiter of the level 1 does not see the level 0 or the unprioritized level.
    assert_eq!(
        produce(&env, send_at(5, Some(0))).await,
        SpaceOutcome::default()
    );
    assert_eq!(
        produce(&env, send_at(6, None)).await,
        SpaceOutcome::default()
    );
    assert_eq!(waiters(&space).await, vec![wtr(
        0,
        Some(1),
        "w",
        false,
        false
    )]);
    assert_eq!(fired(&produce(&env, send_at(7, Some(1))).await), vec![
        delivery("w", 7)
    ]);
    assert_eq!(records(&space).await, vec![
        rec(1, Some(0), 5, false),
        rec(2, None, 6, false)
    ]);
    assert!(waiters(&space).await.is_empty());
}

#[tokio::test]
async fn space_disciplines_pq_unrestricted_waiter_takes_first_matching_level() {
    let Fixture { space, env } = fixture().await;
    produce(&env, send_at(1, Some(0))).await;
    produce(&env, send_at(2, Some(1))).await;
    produce(&env, send_at(3, None)).await;
    assert_eq!(
        fired(&consume(&env, receive_at("three", exactly(3), None)).await),
        vec![literal("three")],
        "the receive skips the heads of the levels 0 and 1, which do not match"
    );
    assert_eq!(
        fired(&consume(&env, receive_at("two", exactly(2), None)).await),
        vec![literal("two")]
    );
    assert_eq!(records(&space).await, vec![rec(0, Some(0), 1, false)]);
    // An unrestricted receive that matches no level head parks in the unrestricted queue.
    assert_eq!(
        consume(&env, receive_at("none", exactly(8), None)).await,
        SpaceOutcome::default()
    );
    assert_eq!(waiters(&space).await, vec![wtr(
        5, None, "none", false, false
    )]);
}

#[tokio::test]
async fn space_disciplines_pq_guarded_unrestricted_receive_parks() {
    let Fixture { space, env } = fixture().await;
    produce(&env, send_at(1, Some(0))).await;
    produce(&env, send_at(2, Some(1))).await;
    // The first level head that the pattern matches is 1. The guard x == 2 refuses it, so the
    // receive parks (decision 38, "Park"), although the head 2 of the level 1 passes the guard.
    assert_eq!(
        consume(&env, guarded(receive_at("g", any(), None), 2)).await,
        SpaceOutcome::default()
    );
    assert_eq!(waiters(&space).await, vec![wtr(2, None, "g", false, false)]);
    assert_eq!(records(&space).await, vec![
        rec(0, Some(0), 1, false),
        rec(1, Some(1), 2, false)
    ]);
    // On the produce side the datum meets the waiter itself (decision 37): a send of 2 at the
    // level 3 fits the pattern and the guard, so it fires the guarded waiter.
    assert_eq!(fired(&produce(&env, send_at(2, Some(3))).await), vec![
        delivery("g", 2)
    ]);
    // The same guard on a waiter of the level 1 sees only that level, so it takes the head 2.
    assert_eq!(
        fired(&consume(&env, guarded(receive_at("h", any(), Some(1)), 2)).await),
        vec![delivery("h", 2)]
    );
    assert_eq!(records(&space).await, vec![rec(0, Some(0), 1, false)]);
    assert!(waiters(&space).await.is_empty());
}

#[tokio::test]
async fn space_disciplines_pq_older_waiter_wins() {
    for unrestricted_first in [true, false] {
        let Fixture { space, env } = fixture().await;
        let (older, younger) = match unrestricted_first {
            true => (("u", None), ("l", Some(3))),
            false => (("l", Some(3)), ("u", None)),
        };
        for (name, lane) in [older, younger] {
            assert_eq!(
                consume(&env, receive_at(name, any(), lane)).await,
                SpaceOutcome::default()
            );
        }
        // One datum of the level 3 fits both waiters (decision 38): the older fires.
        assert_eq!(fired(&produce(&env, send_at(8, Some(3))).await), vec![
            delivery(older.0, 8)
        ]);
        assert_eq!(waiters(&space).await, vec![wtr(
            1, younger.1, younger.0, false, false
        )]);
        // A datum of another level fits only the unrestricted waiter.
        if !unrestricted_first {
            assert_eq!(fired(&produce(&env, send_at(9, Some(4))).await), vec![
                delivery("u", 9)
            ]);
        }
    }
}

#[tokio::test]
async fn space_disciplines_pq_unprioritized_lowest() {
    let Fixture { space, env } = fixture().await;
    produce(&env, send_at(5, None)).await;
    produce(&env, send_at(6, Some(7))).await;
    // The unprioritized level comes after every priority level (F6).
    assert_eq!(
        fired(&consume(&env, receive_at("a", any(), None)).await),
        vec![delivery("a", 6)]
    );
    // A waiter of a level never sees the unprioritized level.
    assert_eq!(
        consume(&env, receive_at("w", any(), Some(0))).await,
        SpaceOutcome::default()
    );
    assert_eq!(records(&space).await, vec![rec(0, None, 5, false)]);
    assert_eq!(
        fired(&consume(&env, receive_at("b", any(), None)).await),
        vec![delivery("b", 5)]
    );
    // An unprioritized datum meets only the unrestricted queue.
    assert_eq!(
        produce(&env, send_at(4, None)).await,
        SpaceOutcome::default()
    );
    assert_eq!(waiters(&space).await, vec![wtr(
        3,
        Some(0),
        "w",
        false,
        false
    )]);
    assert_eq!(records(&space).await, vec![rec(5, None, 4, false)]);
}

// ─── Params and queries ──────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn space_disciplines_pq_priorities_bound_rejects() {
    let Fixture { space, env } = fixture_with(Some(3)).await;
    assert_eq!(
        produce(&env, send_at(1, Some(2))).await,
        SpaceOutcome::default()
    );
    let invalid = SpaceError::InvalidParam {
        key: "priority".to_string(),
    };
    assert_eq!(invalid.code(), "InvalidParam");
    for op in [
        SpaceOp::Produce(send_at(1, Some(3))),
        SpaceOp::Produce(send_forever_at(1, Some(9))),
        SpaceOp::Consume(receive_at("q", any(), Some(3))),
    ] {
        assert_eq!(rejected(&space, &env, op).await, invalid);
    }
    assert_eq!(
        fired(&consume(&env, receive_at("r", any(), Some(2))).await),
        vec![delivery("r", 1)]
    );
    // Without a bound every level is accepted, and a far level stores one record only.
    let Fixture { space, env } = fixture().await;
    assert_eq!(
        produce(&env, send_at(1, Some(1_000_000_000))).await,
        SpaceOutcome::default()
    );
    assert_eq!(records(&space).await, vec![rec(
        0,
        Some(1_000_000_000),
        1,
        false
    )]);
}

#[tokio::test]
async fn space_disciplines_pq_priority_param_type_checked() {
    let Fixture { space, env } = fixture().await;
    let mut flagged = int(1);
    flagged.connective_used = true;
    let invalid = SpaceError::InvalidParam {
        key: "priority".to_string(),
    };
    for value in [
        string("1"),
        int(-1),
        new_gbool_par(true, Vec::new(), false),
        flagged,
        Par::default(),
    ] {
        let send = ProduceOp {
            params: BTreeMap::from([(PRIORITY_PARAM.to_string(), value.clone())]),
            ..send_at(1, None)
        };
        assert_eq!(
            rejected(&space, &env, SpaceOp::Produce(send)).await,
            invalid,
            "{value:?}"
        );
        let receive = ConsumeOp {
            query: Some(QueryParams {
                payload: Vec::new(),
                named: BTreeMap::from([(PRIORITY_PARAM.to_string(), value)]),
            }),
            ..receive_at("q", any(), None)
        };
        assert_eq!(
            rejected(&space, &env, SpaceOp::Consume(receive)).await,
            invalid
        );
    }
    // An unknown key fails before the values, with the nearest key of the schema.
    let unknown = ProduceOp {
        params: BTreeMap::from([("priorty".to_string(), string("x"))]),
        ..send_at(1, None)
    };
    assert_eq!(
        rejected(&space, &env, SpaceOp::Produce(unknown)).await,
        SpaceError::UnknownParam {
            key: "priorty".to_string(),
            suggestion: Some("priority".to_string()),
        }
    );
    // A positional query part has no meaning on a priority queue.
    let positional = ConsumeOp {
        query: Some(QueryParams {
            payload: vec![int(1)],
            named: BTreeMap::new(),
        }),
        ..receive_at("q", any(), None)
    };
    assert_eq!(
        rejected(&space, &env, SpaceOp::Consume(positional)).await,
        SpaceError::PositionalParamsUnsupported
    );
    assert!(records(&space).await.is_empty());
    assert!(waiters(&space).await.is_empty());
}

/// A query with `priority = 0` on a collection of `D` in a fresh space of the family `D`: the
/// result of the receive, and true when it staged nothing (no event and no counter record).
async fn query_on<D: Discipline>(
    inner: SpaceInner,
    sid: u8,
) -> (Result<SpaceOutcome, SpaceError>, bool) {
    let space_id = NativeSpace {
        sid: [sid; SID_LEN],
        durability: Durability::Durable,
    };
    let host = play().await;
    let env = env_over(host.clone(), &budget());
    let agent = GenericSpace::<HashMapOuter, D>::default();
    let config = SpaceConfig::new(
        space_id.sid,
        HashMapOuter::NAME,
        D::NAME,
        SpaceQualifier::Default,
    )
    .to_par();
    create(
        &agent,
        &env,
        creation(space_id, config, SpaceFactoryUrn {
            outer: SpaceOuter::HashMap,
            inner,
        }),
    )
    .await;
    host.take_event_log().await;
    let op = ConsumeOp {
        query: lane_query(Some(0)),
        ..receive_on(space_id, &channel(), "q", any())
    };
    let outcome = run(&agent, &env, SpaceOp::Consume(op)).await;
    let staged_nothing = host.take_event_log().await.is_empty()
        && counter(&host, &counter_channel(space_id, &channel()))
            .await
            .is_empty();
    (outcome, staged_nothing)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_disciplines_pq_query_on_non_pq_rejected() {
    let unsupported = (Err(SpaceError::QuerySpecUnsupported), true);
    assert_eq!(
        query_on::<QueueDiscipline>(SpaceInner::Queue, 0x61).await,
        unsupported
    );
    assert_eq!(
        query_on::<BagDiscipline>(SpaceInner::Bag, 0x62).await,
        unsupported
    );
    assert_eq!(
        query_on::<StackDiscipline>(SpaceInner::Stack, 0x63).await,
        unsupported
    );
    assert_eq!(
        query_on::<CellDiscipline>(SpaceInner::Cell, 0x64).await,
        unsupported
    );
    // The priority queue takes the same query: the receive parks in the waiter queue of the
    // level 0, after the bump.
    assert_eq!(
        query_on::<PriorityQueueDiscipline>(SpaceInner::PriorityQueue, 0x65).await,
        (Ok(SpaceOutcome::default()), false)
    );
    // From source, a query with a priority on a queue space fails with the same code.
    let mut session = Session::new().await;
    let space = session
        .setup("rho:space:hashmap:queue", "\"default\"")
        .await;
    session
        .fail_with(
            &with_space(r#"for (@x <- a ~ (; priority=0)) { @"rrs-pq-query"!(x) }"#),
            "rrs-pq-query-on-queue",
            &Injections::new(),
            &["QuerySpecUnsupported"],
        )
        .await;
    assert!(session.values("rrs-pq-query").await.is_empty());
    assert!(session.parked(space.space, &space.a).await.is_empty());
}

// ─── One counter for the whole priority queue ────────────────────────────────────────────────

#[tokio::test]
async fn space_disciplines_pq_one_counter_whole_pq() {
    let Fixture { space, env } = fixture().await;
    let operations = [
        SpaceOp::Produce(send_at(1, Some(0))),
        SpaceOp::Produce(send_at(2, Some(5))),
        SpaceOp::Produce(send_at(3, None)),
        SpaceOp::Consume(receive_at("w", exactly(9), Some(9))),
        SpaceOp::Consume(receive_at("u", exactly(9), None)),
        SpaceOp::Consume(peek_at("p", None)),
    ];
    for (index, op) in operations.into_iter().enumerate() {
        step(&env, op).await;
        let log = labels(&space.take_event_log().await);
        // The first operation opens the collection counter from the space counter.
        let expected_bump = match index {
            0 => vec!["consume S", "comm S", "produce S", "produce K"],
            _ => BUMP.to_vec(),
        };
        assert_eq!(
            &log[..expected_bump.len()],
            expected_bump.as_slice(),
            "operation {index}"
        );
        let expected = i64::try_from(index).expect("a small index") + 1;
        assert_eq!(counter_value(&space).await, vec![expected]);
    }
    // The orders are unique and increasing across the levels and the lanes, and every record
    // and every waiter is on the one data channel of the collection.
    assert_eq!(records(&space).await, vec![
        rec(0, Some(0), 1, false),
        rec(1, Some(5), 2, false),
        rec(2, None, 3, false)
    ]);
    assert_eq!(waiters(&space).await, vec![
        wtr(3, Some(9), "w", false, false),
        wtr(4, None, "u", false, false),
        wtr(5, None, "p", false, true),
    ]);
}

// ─── The settle (F2) ─────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn space_disciplines_pq_removal_exposes_next_waiter_settles() {
    // The waiter side: the fire of b exposes u, the next waiter of the unrestricted queue, and u
    // fits the head 1 of the level 1.
    let Fixture { space, env } = fixture().await;
    produce(&env, send_at(1, Some(1))).await;
    consume(&env, receive_at("b", exactly(0), None)).await;
    consume(&env, receive_at("u", exactly(1), None)).await;
    space.take_event_log().await;
    let outcome = produce(&env, send_at(0, Some(0))).await;
    assert_eq!(fired(&outcome), vec![literal("b"), literal("u")]);
    assert_eq!(outcome.reissue, None);
    assert_eq!(
        labels(&space.take_event_log().await),
        bumped(&[
            "produce D",
            "comm D",
            "consume D",
            // Changed by decision 124 (plan v2 C7.5, rule CR-21): the COMM of a `K_lift` fire
            // peeks its channel. The text before:
            // "comm D",
            "comm D peek",
            "produce D",
            "comm D"
        ])
    );
    assert!(records(&space).await.is_empty());
    assert!(waiters(&space).await.is_empty());
    assert_eq!(counter_value(&space).await, vec![4]);

    // The record side: the receive x takes the head g of the level 0 and exposes 1, which the
    // waiter w of the level 0 fits.
    let Fixture { space, env } = fixture().await;
    produce(&env, send_at(5, Some(0))).await;
    produce(&env, send_at(1, Some(0))).await;
    consume(&env, receive_at("w", exactly(1), Some(0))).await;
    space.take_event_log().await;
    let outcome = consume(&env, receive_at("x", exactly(5), None)).await;
    assert_eq!(fired(&outcome), vec![literal("x"), literal("w")]);
    assert_eq!(
        labels(&space.take_event_log().await),
        bumped(&[
            "consume D",
            "comm D",
            "consume D",
            // Changed by decision 124 (plan v2 C7.5, rule CR-21): the COMM of a `K_lift` fire
            // peeks its channel. The text before:
            // "comm D",
            "comm D peek",
            "produce D",
            "comm D"
        ])
    );
    assert!(records(&space).await.is_empty());
    assert!(waiters(&space).await.is_empty());
}

/// The readiness review S4: the records 1 and 2 wait at the level 1. The unrestricted waiters
/// b (only 0), u (only 1) and u2 (only 2) and the waiter r of the level 1 (only 2) wait. The send
/// of 0 fires b and exposes u, which the settle fires with 1. That fire exposes the record 2 and
/// the waiter u2, so both u2 and r fit 2: the older of the two takes it. The two arrival orders
/// give two outcomes, so the order of the settle matters (strict confluence fails).
async fn contested(env: &SpaceEnv, restricted_first: bool) -> SpaceOutcome {
    produce(env, send_at(1, Some(1))).await;
    produce(env, send_at(2, Some(1))).await;
    consume(env, receive_at("b", exactly(0), None)).await;
    consume(env, receive_at("u", exactly(1), None)).await;
    let competitors = match restricted_first {
        false => [
            receive_at("u2", exactly(2), None),
            receive_at("r", exactly(2), Some(1)),
        ],
        true => [
            receive_at("r", exactly(2), Some(1)),
            receive_at("u2", exactly(2), None),
        ],
    };
    for op in competitors {
        assert_eq!(consume(env, op).await, SpaceOutcome::default());
    }
    produce(env, send_at(0, Some(0))).await
}

#[tokio::test]
async fn space_disciplines_pq_settle_oldest_first() {
    for restricted_first in [false, true] {
        let Fixture { space, env } = fixture().await;
        let outcome = contested(&env, restricted_first).await;
        let (winner, loser) = match restricted_first {
            false => ("u2", wtr(5, Some(1), "r", false, false)),
            true => ("r", wtr(5, None, "u2", false, false)),
        };
        assert_eq!(fired(&outcome), vec![
            literal("b"),
            literal("u"),
            literal(winner)
        ]);
        assert_eq!(outcome.reissue, None);
        assert!(records(&space).await.is_empty());
        assert_eq!(waiters(&space).await, vec![loser]);
    }
    // The events and the identities of the two lifts: the steps are the fire counts of the
    // transaction (I-21), and each re-offer keeps the identity of its record. The record 1
    // carries the cost authority of its sender, and its re-offer registers that authority.
    let space = play().await;
    let budget = budget();
    let _scope = budget.enter_comm_accounting_scope();
    let env = env_over(space.clone(), &budget);
    create(&pq(), &env, pq_creation(None)).await;
    space.take_event_log().await;
    let signed = ProduceOp {
        authority: region(2),
        data: ListParWithRandom {
            cost_authority: Some(region(2)),
            ..send_at(1, Some(1)).data
        },
        ..send_at(1, Some(1))
    };
    produce(&env, signed).await;
    produce(&env, send_at(2, Some(1))).await;
    let stored = data_produces(&space.take_event_log().await);
    assert_eq!(stored.len(), 2);
    for op in [
        receive_at("b", exactly(0), None),
        receive_at("u", exactly(1), None),
        receive_at("u2", exactly(2), None),
        receive_at("r", exactly(2), Some(1)),
    ] {
        consume(&env, op).await;
    }
    space.take_event_log().await;
    let d = send_at(0, Some(0));
    produce(&env, d.clone()).await;
    let log = space.take_event_log().await;
    assert_eq!(
        labels(&log),
        bumped(&[
            "produce D",
            "comm D",
            "consume D",
            // Changed by decision 124 (plan v2 C7.5, rule CR-21): the COMM of a `K_lift` fire
            // peeks its channel. The text before:
            // "comm D",
            "comm D peek",
            "produce D",
            "comm D",
            "consume D",
            // Changed by decision 124 (plan v2 C7.5, rule CR-21): the COMM of a `K_lift` fire
            // peeks its channel. The text before:
            // "comm D",
            "comm D peek",
            "produce D",
            "comm D"
        ])
    );
    assert_eq!(data_consumes(&log), vec![
        lift_consume(6, 1, &d.data.random_state),
        lift_consume(6, 2, &d.data.random_state)
    ]);
    let produces = data_produces(&log);
    assert_eq!(
        produces[1], stored[0],
        "the re-offer of 1 keeps its identity"
    );
    assert_eq!(
        produces[2], stored[1],
        "the re-offer of 2 keeps its identity"
    );
    assert_eq!(
        budget
            .introduction_authority(
                produce_introduction_identity(&stored[0]),
                AuthorityByteEventKind::ProduceIntroduction
            )
            .ok(),
        Some(region(2)),
        "the re-offer registers the record's own authority"
    );
}

#[tokio::test]
async fn space_disciplines_pq_settle_job_with_nothing_owed_only_bumps() {
    let Fixture { space, env } = fixture().await;
    produce(&env, send_at(1, Some(0))).await;
    space.take_event_log().await;
    let outcome = step(&env, SpaceOp::Settle(settle_after_send(&send_at(7, None)))).await;
    assert_eq!(outcome, SpaceOutcome::default());
    assert_eq!(labels(&space.take_event_log().await), bumped(&[]));
    assert_eq!(records(&space).await, vec![rec(0, Some(0), 1, false)]);
    assert_eq!(counter_value(&space).await, vec![2]);
}

// ─── Rule E (decision 45) and decision 30 ────────────────────────────────────────────────────

#[tokio::test]
async fn space_disciplines_pq_peek_restores_head() {
    let Fixture { space, env } = fixture().await;
    consume(&env, receive_at("w", exactly(9), Some(1))).await;
    produce(&env, send_at(5, Some(1))).await;
    produce(&env, send_at(6, Some(1))).await;
    let stored = data_produces(&space.take_event_log().await);
    assert_eq!(stored.len(), 2);
    let peeked = consume(&env, peek_at("p", None)).await;
    assert_eq!(fired(&peeked), vec![delivery("p", 5)]);
    assert!(!peeked.fires[0].results[0].persistent);
    assert_eq!(peeked.reissue, None);
    let log = space.take_event_log().await;
    // The peek fire removes the head and the re-store produces it again: the same identity,
    // so the same order and level. It is the head again, and no settle step follows.
    assert_eq!(
        labels(&log),
        bumped(&["consume D", "comm D peek", "produce D"])
    );
    assert_eq!(data_produces(&log), vec![stored[0].clone()]);
    assert_eq!(records(&space).await, vec![
        rec(1, Some(1), 5, false),
        rec(2, Some(1), 6, false)
    ]);
    assert_eq!(waiters(&space).await, vec![wtr(
        0,
        Some(1),
        "w",
        false,
        false
    )]);
    assert_eq!(
        fired(&consume(&env, receive_at("t", any(), None)).await),
        vec![delivery("t", 5)]
    );

    // A persistent head stays in place, so nothing is stored again.
    let Fixture { space, env } = fixture().await;
    produce(&env, send_forever_at(5, Some(0))).await;
    space.take_event_log().await;
    let peeked = consume(&env, peek_at("p", Some(0))).await;
    assert!(peeked.fires[0].results[0].persistent);
    assert_eq!(
        labels(&space.take_event_log().await),
        bumped(&["consume D", "comm D peek"])
    );
    assert_eq!(records(&space).await, vec![rec(0, Some(0), 5, true)]);
}

#[tokio::test]
async fn space_disciplines_pq_produce_peek_waiter_reoffers() {
    let Fixture { space, env } = fixture().await;
    consume(&env, peek_at("p", Some(0))).await;
    consume(&env, receive_at("u", any(), None)).await;
    space.take_event_log().await;
    // The older waiter p peeks, so the datum stays in flight (decision 30) and the next head, u,
    // takes it.
    let outcome = produce(&env, send_at(4, Some(0))).await;
    assert_eq!(fired(&outcome), vec![delivery("p", 4), delivery("u", 4)]);
    assert_eq!(
        labels(&space.take_event_log().await),
        bumped(&["produce D", "comm D peek", "produce D", "comm D"])
    );
    assert!(records(&space).await.is_empty());
    assert!(waiters(&space).await.is_empty());

    // With no other waiter, the datum is stored at its order and level after the peek fire.
    let Fixture { space, env } = fixture().await;
    consume(&env, peek_at("p", Some(0))).await;
    let outcome = produce(&env, send_at(4, Some(0))).await;
    assert_eq!(fired(&outcome), vec![delivery("p", 4)]);
    assert_eq!(records(&space).await, vec![rec(1, Some(0), 4, false)]);
}

// ─── Decision 43 ─────────────────────────────────────────────────────────────────────────────

/// Parks `count` waiters of the level `level` that take `value`, with names from `prefix`.
async fn park_takers(env: &SpaceEnv, prefix: &str, value: i64, level: i64, count: i64) {
    for index in 0..count {
        assert_eq!(
            consume(
                env,
                receive_at(&format!("{prefix}{index}"), exactly(value), Some(level))
            )
            .await,
            SpaceOutcome::default()
        );
    }
}

/// A persistent send of `value` at the level 0 that fires `FIRE_ROUND_CAP` waiters and
/// re-issues at the cap.
async fn capped_send(env: &SpaceEnv, prefix: &str, value: i64) -> SpaceReissue {
    park_takers(env, prefix, value, 0, cap()).await;
    let outcome = produce(env, send_forever_at(value, Some(0))).await;
    assert_eq!(outcome.fires.len(), FIRE_ROUND_CAP);
    let reissue = outcome.reissue.expect("the cap re-issues the send");
    assert_eq!(reissue.cause, ReissueCause::RoundCap);
    reissue
}

#[tokio::test]
async fn space_disciplines_pq_reissue_takes_level_fifo_place() {
    let Fixture { space, env } = fixture().await;
    let first = capped_send(&env, "a", 1).await;
    let second = capped_send(&env, "b", 2).await;
    let (first_ord, second_ord) = match (&first.op, &second.op) {
        (SpaceOp::Produce(a), SpaceOp::Produce(b)) => (
            a.reissued_ord.expect("a persistent send carries its order"),
            b.reissued_ord.expect("a persistent send carries its order"),
        ),
        other => panic!("two re-issued sends: {other:?}"),
    };
    assert_eq!(first_ord, cap());
    assert_eq!(second_ord, 2 * cap() + 1);
    // The unrestricted waiter u takes only 2.
    consume(&env, receive_at("u", exactly(2), None)).await;
    // The older send re-enters first: the level 0 is empty, so it is at the head. u does not fit
    // 1, so it is stored at its first-round order.
    assert_eq!(step(&env, first.op).await, SpaceOutcome::default());
    space.take_event_log().await;
    // The younger send re-enters behind the older record of its level: it takes its FIFO place
    // (decision 43), with no fire, although u fits 2.
    let rest = step(&env, second.op.clone()).await;
    assert_eq!(rest, SpaceOutcome::default());
    let round = space.take_event_log().await;
    assert_eq!(labels(&round), bumped(&["produce D"]));
    assert_eq!(records(&space).await, vec![
        rec(first_ord, Some(0), 1, true),
        rec(second_ord, Some(0), 2, true)
    ]);
    assert_eq!(waiters(&space).await, vec![wtr(
        2 * cap() + 2,
        None,
        "u",
        false,
        false
    )]);
    // Each round bumps the counter.
    assert_eq!(counter_value(&space).await, vec![2 * cap() + 5]);
}

#[tokio::test]
async fn space_disciplines_pq_reentered_waiter_keeps_seniority() {
    let Fixture { space, env } = fixture().await;
    for _ in 0..cap() {
        produce(&env, send_at(1, Some(1))).await;
    }
    // The persistent receive r drains FIRE_ROUND_CAP records and re-issues at the cap with its
    // first-round order.
    let receiving = receive_forever_at("r", exactly(1), None);
    let outcome = consume(&env, receiving.clone()).await;
    assert_eq!(outcome.fires.len(), FIRE_ROUND_CAP);
    let reissue = outcome.reissue.expect("the cap re-issues the receive");
    assert_eq!(
        reissue.op,
        SpaceOp::Consume(ConsumeOp {
            reissued_ord: Some(cap()),
            ..receiving
        })
    );
    // A waiter of the level 0 parks while r is in flight.
    consume(&env, receive_at("x", any(), Some(0))).await;
    // r re-enters and parks with its first-round order, after x parked.
    assert_eq!(step(&env, reissue.op).await, SpaceOutcome::default());
    assert_eq!(waiters(&space).await, vec![
        wtr(cap(), None, "r", true, false),
        wtr(cap() + 1, Some(0), "x", false, false),
    ]);
    // A datum of the level 0 fits both. r is older by its first-round order, so it fires first
    // (decision 38 and decision 43), although it parked after x.
    assert_eq!(fired(&produce(&env, send_at(1, Some(0))).await), vec![
        literal("r")
    ]);
    assert_eq!(waiters(&space).await, vec![
        wtr(cap(), None, "r", true, false),
        wtr(cap() + 1, Some(0), "x", false, false),
    ]);
}

// ─── The round cap (I-21) and divergent pairs (A10) ──────────────────────────────────────────

/// The blocker b of the unrestricted queue (only 999), a persistent waiter w of every value
/// behind it, the records `0..count` at the level 0, and the send of 999 at the level 1, which
/// fires b and exposes the pairs of each record with w. It takes the event log before the send
/// of 999, and it returns the outcome of that send.
async fn exposed_pairs(space: &ParRSpace, env: &SpaceEnv, count: i64) -> SpaceOutcome {
    consume(env, receive_at("b", exactly(999), None)).await;
    consume(env, receive_forever_at("w", any(), None)).await;
    for value in 0..count {
        assert_eq!(
            produce(env, send_at(value, Some(0))).await,
            SpaceOutcome::default()
        );
    }
    space.take_event_log().await;
    produce(env, send_at(999, Some(1))).await
}

#[tokio::test]
async fn space_disciplines_pq_round_cap_reissues_a_settle() {
    let Fixture { space, env } = fixture().await;
    let outcome = exposed_pairs(&space, &env, cap()).await;
    // The plan fires b, and the settle after it fires FIRE_ROUND_CAP - 1 pairs (I-21).
    assert_eq!(outcome.fires.len(), FIRE_ROUND_CAP);
    let mut expected = vec![literal("b")];
    expected.extend((0..cap() - 1).map(|value| delivery("w", value)));
    assert_eq!(fired(&outcome), expected);
    assert_eq!(
        outcome.reissue,
        Some(SpaceReissue {
            op: SpaceOp::Settle(settle_after_send(&send_at(999, Some(1)))),
            cause: ReissueCause::RoundCap,
        })
    );
    assert_eq!(records(&space).await, vec![rec(
        cap() + 1,
        Some(0),
        cap() - 1,
        false
    )]);
    space.take_event_log().await;
    let rest = step(&env, outcome.reissue.expect("a settle job").op).await;
    assert_eq!(fired(&rest), vec![delivery("w", cap() - 1)]);
    assert_eq!(rest.reissue, None);
    assert_eq!(
        labels(&space.take_event_log().await),
        // Changed by decision 124 (plan v2 C7.5, rule CR-21): the COMM of a `K_lift` fire peeks
        // its channel. The text before:
        // bumped(&["consume D", "comm D", "produce D", "comm D"])
        bumped(&["consume D", "comm D peek", "produce D", "comm D"])
    );
    assert!(records(&space).await.is_empty());
    assert_eq!(waiters(&space).await, vec![wtr(1, None, "w", true, false)]);
}

#[tokio::test]
async fn space_disciplines_pq_settle_before_reissues_the_operation_at_the_cap() {
    let Fixture { space, env } = fixture().await;
    let outcome = exposed_pairs(&space, &env, 2 * cap()).await;
    assert_eq!(outcome.fires.len(), FIRE_ROUND_CAP);
    let settle_job = outcome
        .reissue
        .expect("the settle after the plan re-issues");
    // A persistent receive of another lane runs before the settle job. Its settle before the
    // plan fires the next FIRE_ROUND_CAP pairs and reaches the cap, so the receive re-issues
    // itself with its first-round order (decision 43), before its continuation is parked.
    let first = 2 * cap() + 3;
    let receiving = receive_forever_at("y", exactly(-1), Some(7));
    let outcome = consume(&env, receiving.clone()).await;
    let expected: Vec<_> = (cap() - 1..2 * cap() - 1)
        .map(|value| delivery("w", value))
        .collect();
    assert_eq!(fired(&outcome), expected);
    assert_eq!(
        outcome.reissue,
        Some(SpaceReissue {
            op: SpaceOp::Consume(ConsumeOp {
                reissued_ord: Some(first),
                ..receiving
            }),
            cause: ReissueCause::RoundCap,
        })
    );
    assert_eq!(records(&space).await, vec![rec(
        2 * cap() + 1,
        Some(0),
        2 * cap() - 1,
        false
    )]);
    let rest = step(&env, settle_job.op).await;
    assert_eq!(fired(&rest), vec![delivery("w", 2 * cap() - 1)]);
    let parked = step(&env, outcome.reissue.expect("the re-issue").op).await;
    assert_eq!(parked, SpaceOutcome::default());
    assert_eq!(waiters(&space).await, vec![
        wtr(1, None, "w", true, false),
        wtr(first, Some(7), "y", true, false)
    ]);
    assert!(records(&space).await.is_empty());
}

#[tokio::test]
async fn space_disciplines_pq_divergent_pairs_reissue() {
    // A persistent send in flight and a persistent waiter: the send re-issues itself.
    let Fixture { space, env } = fixture().await;
    consume(&env, receive_forever_at("w", any(), None)).await;
    let outcome = produce(&env, send_forever_at(9, Some(2))).await;
    assert_eq!(fired(&outcome), vec![delivery("w", 9)]);
    let expected = SpaceReissue {
        op: SpaceOp::Produce(ProduceOp {
            reissued_ord: Some(1),
            ..send_forever_at(9, Some(2))
        }),
        cause: ReissueCause::DivergentPair,
    };
    assert_eq!(outcome.reissue, Some(expected.clone()));
    let again = step(&env, expected.op.clone()).await;
    assert_eq!(fired(&again), vec![delivery("w", 9)]);
    assert_eq!(
        again.reissue,
        Some(expected),
        "every round keeps the first-round order"
    );
    assert!(records(&space).await.is_empty());
    assert_eq!(counter_value(&space).await, vec![3]);

    // A stored persistent record and a stored persistent waiter: a settle job re-issues.
    let Fixture { space, env } = fixture().await;
    consume(&env, receive_at("b", exactly(999), None)).await;
    consume(&env, receive_forever_at("w", any(), None)).await;
    produce(&env, send_forever_at(1, Some(0))).await;
    let stored = data_produces(&space.take_event_log().await);
    let outcome = produce(&env, send_at(999, Some(1))).await;
    assert_eq!(fired(&outcome), vec![literal("b"), delivery("w", 1)]);
    let expected = SpaceReissue {
        op: SpaceOp::Settle(settle_after_send(&send_at(999, Some(1)))),
        cause: ReissueCause::DivergentPair,
    };
    assert_eq!(outcome.reissue, Some(expected.clone()));
    space.take_event_log().await;
    let again = step(&env, expected.op.clone()).await;
    assert_eq!(fired(&again), vec![delivery("w", 1)]);
    assert_eq!(again.reissue, Some(expected));
    let log = space.take_event_log().await;
    assert_eq!(labels(&log), bumped(&["produce D", "comm D"]));
    assert_eq!(
        data_produces(&log),
        stored,
        "the persistent record is offered again with its identity and not lifted"
    );
    assert_eq!(records(&space).await, vec![rec(2, Some(0), 1, true)]);
    assert_eq!(waiters(&space).await, vec![wtr(1, None, "w", true, false)]);
}

// ─── Restart, replay and merge ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn space_disciplines_pq_restart_order() {
    let Fixture { space, env } = fixture().await;
    for (value, level) in [(3, Some(1)), (1, Some(0)), (2, Some(0)), (4, None)] {
        produce(&env, send_at(value, level)).await;
    }
    consume(&env, receive_at("x", exactly(9), Some(5))).await;
    let root = space.create_checkpoint().await.expect("a checkpoint").root;
    space
        .reset(&root)
        .await
        .expect("the reset to the checkpoint");
    assert_eq!(records(&space).await, vec![
        rec(0, Some(1), 3, false),
        rec(1, Some(0), 1, false),
        rec(2, Some(0), 2, false),
        rec(3, None, 4, false)
    ]);
    // The heads come from the orders and levels of the envelopes, whatever the list order of the
    // reloaded store.
    for (name, value) in [("a", 1), ("b", 2), ("c", 3), ("d", 4)] {
        assert_eq!(
            fired(&consume(&env, receive_at(name, any(), None)).await),
            vec![delivery(name, value)]
        );
    }
    assert_eq!(waiters(&space).await, vec![wtr(
        4,
        Some(5),
        "x",
        false,
        false
    )]);
}

// Retired by decision 90 (C6, step D): `space_disciplines_pq_replay_equal` and its script helpers
// ran through the rigged replay modes, which retire (decision 125). Native replay: the family
// matrix `space_native_replay_equal_{hashmap,pathmap,array,vector}`. Its negative controls:
// `space_native_forged_predecessor_fails_closed`,
// `native_space_replay_rejects_a_tampered_candidate`,
// `native_space_replay_rejects_an_outcome_that_does_not_fit`. The text before:
// ```
// /// Runs `op` of a script with the priority queue agent.
// async fn script_op(env: &SpaceEnv, op: SpaceOp) -> Result<SpaceOutcome, SpaceError> {
//     run(&pq(), env, op).await
// }
//
// /// A send of `value` at the level `level` on the channel `name`.
// fn send_to(name: &str, value: i64, level: Option<i64>, persistent: bool) -> SpaceOp {
//     let channel = string(name);
//     SpaceOp::Produce(ProduceOp {
//         params: level.map(priority).unwrap_or_default(),
//         ..send_on(SPACE, &channel, value, persistent)
//     })
// }
//
// /// A receive on the channel `name`.
// fn receive_to(
//     name: &str,
//     body: &str,
//     pattern: BindPattern,
//     lane: Option<i64>,
//     persistent: bool,
//     peek: bool,
// ) -> SpaceOp {
//     SpaceOp::Consume(ConsumeOp {
//         query: lane_query(lane),
//         persistent,
//         peek,
//         ..receive_on(SPACE, &string(name), body, pattern)
//     })
// }
//
// /// A script over six collections: the ascending levels, the contested settle of S4, a peek, a
// /// produce-side peek, a guarded unrestricted receive, and a stored divergent pair for three
// /// rounds.
// async fn run_script(env: &SpaceEnv) -> Result<Vec<SpaceOutcome>, SpaceError> {
//     let mut ops = Vec::new();
//     for (value, level) in [(30, None), (20, Some(2)), (0, Some(0))] {
//         ops.push(send_to("levels", value, level, false));
//     }
//     for name in ["a", "b", "c"] {
//         ops.push(receive_to("levels", name, any(), None, false, false));
//     }
//     ops.push(send_to("contest", 1, Some(1), false));
//     ops.push(send_to("contest", 2, Some(1), false));
//     ops.push(receive_to("contest", "b", exactly(0), None, false, false));
//     ops.push(receive_to("contest", "u", exactly(1), None, false, false));
//     ops.push(receive_to(
//         "contest",
//         "r",
//         exactly(2),
//         Some(1),
//         false,
//         false,
//     ));
//     ops.push(receive_to("contest", "u2", exactly(2), None, false, false));
//     ops.push(send_to("contest", 0, Some(0), false));
//     ops.push(send_to("peeking", 5, Some(3), false));
//     ops.push(receive_to("peeking", "p", any(), None, false, true));
//     ops.push(receive_to("flight", "p", any(), Some(0), false, true));
//     ops.push(send_to("flight", 4, Some(0), false));
//     ops.push(send_to("guard", 1, Some(0), false));
//     ops.push(send_to("guard", 2, Some(1), false));
//     ops.push(SpaceOp::Consume(ConsumeOp {
//         guard: Some(bound_equals(2)),
//         ..receive_on(SPACE, &string("guard"), "g", any())
//     }));
//     let mut outcomes = Vec::with_capacity(ops.len() + 6);
//     for op in ops {
//         outcomes.push(script_op(env, op).await?);
//     }
//     for op in [
//         receive_to("divergent", "b", exactly(999), None, false, false),
//         receive_to("divergent", "w", any(), None, true, false),
//         send_to("divergent", 1, Some(0), true),
//     ] {
//         outcomes.push(script_op(env, op).await?);
//     }
//     let mut next = Some(send_to("divergent", 999, Some(1), false));
//     for _ in 0..3 {
//         let Some(op) = next.take() else {
//             break;
//         };
//         let outcome = script_op(env, op).await?;
//         next = outcome.reissue.as_ref().map(|reissue| reissue.op.clone());
//         outcomes.push(outcome);
//     }
//     Ok(outcomes)
// }
//
// #[tokio::test]
// async fn space_disciplines_pq_replay_equal() {
//     let (play, replay) = play_and_replay().await;
//     let play_env = env_over(play.clone(), &budget());
//     create(&pq(), &play_env, pq_creation(None)).await;
//     let start = play
//         .create_checkpoint()
//         .await
//         .expect("a checkpoint after the creation")
//         .root;
//     let played = run_script(&play_env).await.expect("the script plays");
//     assert_eq!(
//         played
//             .iter()
//             .map(|outcome| outcome.fires.len())
//             .sum::<usize>(),
//         12,
//         "the script fires the levels, the contest, the peeks and the divergent rounds"
//     );
//     assert!(played
//         .last()
//         .is_some_and(|outcome| outcome.reissue.is_some()));
//     let Checkpoint { root, log } = play.create_checkpoint().await.expect("a checkpoint");
//
//     replay
//         .rig_and_reset(start.clone(), log)
//         .await
//         .expect("the replay rigs the play log");
//     let replay_env = env_over(replay.clone(), &budget());
//     let replayed = run_script(&replay_env).await.expect("the script replays");
//     assert_eq!(replayed, played);
//     replay
//         .check_replay_data()
//         .await
//         .expect("every recorded COMM is paired, the lifts and the repeated ones included");
//     assert_eq!(
//         replay.create_checkpoint().await.expect("a checkpoint").root,
//         root
//     );
//
//     replay
//         .rig_and_reset(start, Vec::new())
//         .await
//         .expect("the replay rigs an empty log");
//     match run_script(&replay_env).await {
//         Err(error @ SpaceError::ReplayDivergence(_)) => {
//             assert_eq!(error.code(), "ReplayDivergence")
//         }
//         other => panic!("negative control: a log without the COMMs diverges: {other:?}"),
//     }
// }
// ```

/// The merge index of a branch log over the pre-state `root`, with the pre-state closures of
/// Casper (`block_index.rs`, `create_event_log_index`).
fn merge_index(space: &ParRSpace, root: &Blake2b256Hash, log: Vec<Event>) -> EventLogIndex {
    let reader = space
        .get_history_repository()
        .get_history_reader(root)
        .expect("read the history at the pre-state root");
    EventLogIndex::new(
        log,
        |produce: &Produce| {
            reader
                .get_data(&produce.channel_hash)
                .is_ok_and(|data| data.iter().any(|datum| datum.source == *produce))
        },
        |produce: &Produce| {
            reader
                .get_joins(&produce.channel_hash)
                .is_ok_and(|joins| joins.iter().any(|join| join.len() > 1))
        },
        NumberChannelsDiff::new(),
    )
}

/// The log of one branch: `op` from the pre-state `root`.
async fn branch(
    space: &ParRSpace,
    env: &SpaceEnv,
    root: &Blake2b256Hash,
    op: SpaceOp,
) -> Vec<Event> {
    space.reset(root).await.expect("the reset to the pre-state");
    space.take_event_log().await;
    step(env, op).await;
    space.take_event_log().await
}

/// `log` without the events of the counter channels.
fn without_counters(log: &[Event]) -> Vec<Event> {
    let counters = [
        stable_hash_provider::hash(&counter_ch()),
        stable_hash_provider::hash(&space_counter_channel(SPACE)),
    ];
    log.iter()
        .filter(|event| match event {
            Event::IoEvent(IOEvent::Produce(produce)) => !counters.contains(&produce.channel_hash),
            Event::IoEvent(IOEvent::Consume(consume)) => !consume
                .channel_hashes
                .iter()
                .any(|hash| counters.contains(hash)),
            Event::Comm(comm) => !comm
                .consume
                .channel_hashes
                .iter()
                .any(|hash| counters.contains(hash)),
        })
        .cloned()
        .collect()
}

#[tokio::test]
async fn space_disciplines_pq_branches_race_on_the_counter() {
    let Fixture { space, env } = fixture().await;
    produce(&env, send_at(1, Some(0))).await;
    let root = space.create_checkpoint().await.expect("a checkpoint").root;
    // Two branches on two different levels and lanes still race: one counter serves the whole
    // priority queue.
    let push = branch(&space, &env, &root, SpaceOp::Produce(send_at(2, Some(5)))).await;
    let pop = branch(
        &space,
        &env,
        &root,
        SpaceOp::Consume(receive_at("x", any(), Some(0))),
    )
    .await;
    let settle = branch(
        &space,
        &env,
        &root,
        SpaceOp::Settle(settle_after_send(&send_at(3, None))),
    )
    .await;
    let counter = stable_hash_provider::hash(&counter_ch());
    for (left, right, label) in [
        (&push, &pop, "push and pop"),
        (&push, &settle, "push and settle"),
    ] {
        let found: BTreeSet<Blake2b256Hash> = conflicts(
            &merge_index(&space, &root, left.clone()),
            &merge_index(&space, &root, right.clone()),
        )
        .0
        .into_iter()
        .collect();
        assert_eq!(
            found,
            BTreeSet::from([counter.clone()]),
            "{label}: the counter race"
        );
        let reason = conflict_reason(
            &merge_index(&space, &root, left.clone()),
            &merge_index(&space, &root, right.clone()),
        );
        assert!(
            reason
                .as_deref()
                .is_some_and(|reason| reason.starts_with("racesForSameIOEvent")),
            "{label}: merge check #1 finds the race: {reason:?}"
        );
        let found = conflicts(
            &merge_index(&space, &root, without_counters(left)),
            &merge_index(&space, &root, without_counters(right)),
        );
        assert!(
            found.0.is_empty(),
            "{label}: without the counter the branches merge"
        );
    }
}

// ─── The legacy charges (decision 45, Option P) ──────────────────────────────────────────────

/// The state before the send of 0 that settles: the record 1 at the level 1, and the
/// unrestricted waiters b (only 0) and u (only 1).
async fn exposure(env: &SpaceEnv) {
    produce(env, send_at(1, Some(1))).await;
    consume(env, receive_at("b", exactly(0), None)).await;
    consume(env, receive_at("u", exactly(1), None)).await;
}

/// After [`exposure`], the send of 0 fires the waiter b in its plan, and its settle then fires the
/// waiter u with the record 1, in one operation.
#[tokio::test]
async fn space_disciplines_pq_send_fires_the_plan_and_the_settle() {
    let Fixture { env, .. } = fixture().await;
    exposure(&env).await;
    let outcome = produce(&env, send_at(0, Some(0))).await;
    assert_eq!(outcome.fires.len(), 2, "the plan and the settle fire");
    assert_eq!(fired(&outcome), vec![literal("b"), literal("u")]);
}

// Changed by decision 90 (decision 69): a fresh host records its observer calls, because DR-134
// deleted the legacy meter and `ChargingRSpace` (`recorded_host`).
//
// /// A fresh legacy budget and a charging host over it.
// async fn legacy_host() -> (
//     RuntimeBudget,
//     impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
// ) {
//     let budget = RuntimeBudget::new(Cost::create(i64::MAX / 4, "rrs pq legacy host"));
//     let host = charging_host(&budget).await;
//     (budget, host)
// }

/// Added by decision 90 (decision 69): a fresh host with the Rholang matcher, and the recorder of
/// its observer calls.
async fn recorded_host() -> (Arc<Recorder>, ParRSpace) {
    let recorder = Recorder::new();
    let host = observed_host(&recorder).await;
    (recorder, host)
}

/// The waiting continuation of the receive `name` with the pattern `pattern`, parked in the
/// unrestricted queue with the order `ord` on the data channel of `channel()`.
fn parked_waiter(ord: i64, name: &str, pattern: BindPattern) -> ParWaiter {
    let continuation =
        record::wrap_pq_continuation(ord, None, &named(name, None)).expect("a continuation record");
    ParWaiter {
        source: Consume::create(&vec![data()], &vec![pattern.clone()], &continuation, false),
        patterns: vec![pattern],
        continuation,
        persist: false,
        peeks: BTreeSet::new(),
    }
}

/// The produce-side fire of the non-persistent record `record` on the data channel of
/// `channel()`, which fires `waiter` and binds `matched`.
fn offer_fire(
    record: ListParWithRandom,
    waiter: ParWaiter,
    matched: ListParWithRandom,
) -> ParSpaceProduce {
    let source = Produce::create(&data(), &record, false);
    ParSpaceProduce {
        channel: data(),
        data: record.clone(),
        persist: false,
        outcome: ProduceOutcome::Fire {
            channels: vec![data()],
            continuation: waiter,
            data: vec![incoming_candidate(data(), source, record, false, matched)],
        },
        before_observe: None,
        own: false,
        // Added by decision 123 (plan v4, V3-9): no row search.
        search: None,
    }
}

// Disabled by decision 90 (decision 69): Option P (the legacy charges of decision 45) is removed with the legacy meter (DR-134).
// Its host differential continues in `space_disciplines_pq_observer_calls_equal_host`.
//
// /// `legacy_rebracket_eq_host` for the settle of a priority queue (decision 45, Option P): in a
// /// legacy deploy each staged operation pays what `ChargingRSpace` charges for the equivalent host
// /// call, in staging order. The send of 0 stages five operations: the take and the put of the
// /// counter, its fire of b, the lift of the record 1 and its re-offer to u. The host runs the
// /// three shapes that a host can run alone (the take, the put and the lift), and its entries equal
// /// the charges of the same shapes. The host cannot run a produce-side fire of a space record
// /// alone, because the host matches the whole record and the agent matches its payload.
// #[tokio::test]
// async fn space_disciplines_pq_legacy_charges_equal_host() {
//     let space = play().await;
//     let budget = RuntimeBudget::new(Cost::create(i64::MAX / 4, "rrs pq legacy agent"));
//     let _legacy = budget.enter_legacy_scope();
//     let env = env_over(space.clone(), &budget);
//     create(&pq(), &env, pq_creation(None)).await;
//     let h = ProduceOp {
//         authority: region(2),
//         data: ListParWithRandom {
//             cost_authority: Some(region(2)),
//             ..send_at(1, Some(1)).data
//         },
//         ..send_at(1, Some(1))
//     };
//     produce(&env, h.clone()).await;
//     let b = receive_at("b", exactly(0), None);
//     let u = receive_at("u", exactly(1), None);
//     consume(&env, b).await;
//     consume(&env, u.clone()).await;
//     let d = send_at(0, Some(0));
//     let before = budget.get_log().len();
//     assert_eq!(fired(&produce(&env, d.clone()).await), vec![
//         literal("b"),
//         literal("u")
//     ]);
//     let agent_log = budget.get_log()[before..].to_vec();
//
//     // The counter record that the bump of u stored, and the records of the round of d.
//     let random_u = Blake2b512Random::from_bytes(&u.body.random_state);
//     let random_d = Blake2b512Random::from_bytes(&d.data.random_state);
//     let stored_counter = record::counter_record(
//         3,
//         &RandomPurpose::Counter.derive(&random_u).split_byte(1),
//         None,
//     );
//     let take_continuation = counter_continuation(
//         3,
//         &RandomPurpose::Counter.derive(&random_d).split_byte(0),
//         None,
//     );
//     let next_counter = record::counter_record(
//         4,
//         &RandomPurpose::Counter.derive(&random_d).split_byte(1),
//         None,
//     );
//     let record_d = record::wrap_pq_datum(3, Some(0), &d.data);
//     let record_h = record::wrap_pq_datum(0, Some(1), &h.data);
//     let k_lift = lift_continuation(3, 1, &lift_random(&d.data.random_state), None);
//
//     let counter_source = Produce::create(&counter_ch(), &stored_counter, false);
//     let take = ParSpaceConsume {
//         channels: vec![counter_ch()],
//         patterns: vec![counter_pattern()],
//         continuation: take_continuation.clone(),
//         persist: false,
//         peeks: BTreeSet::new(),
//         outcome: ConsumeOutcome::Fire {
//             data: vec![ConsumeCandidate {
//                 channel: counter_ch(),
//                 datum: Datum {
//                     a: ListParWithRandom::default(),
//                     persist: false,
//                     source: counter_source,
//                 },
//                 removed_datum: stored_counter.clone(),
//                 datum_index: 0,
//             }],
//         },
//         before_observe: None,
//     };
//     let put = ParSpaceProduce {
//         channel: counter_ch(),
//         data: next_counter.clone(),
//         persist: false,
//         outcome: ProduceOutcome::Store,
//         before_observe: None,
//     };
//     let plan_fire = offer_fire(
//         record_d,
//         parked_waiter(1, "b", exactly(0)),
//         ListParWithRandom::default(),
//     );
//     let lift = ParSpaceConsume {
//         channels: vec![data()],
//         patterns: vec![lift_pattern()],
//         continuation: k_lift.clone(),
//         persist: false,
//         peeks: BTreeSet::new(),
//         outcome: ConsumeOutcome::Fire {
//             data: vec![ConsumeCandidate {
//                 channel: data(),
//                 datum: Datum {
//                     a: ListParWithRandom::default(),
//                     persist: false,
//                     source: Produce::create(&data(), &record_h, false),
//                 },
//                 removed_datum: record_h.clone(),
//                 datum_index: 0,
//             }],
//         },
//         before_observe: None,
//     };
//     let reoffer = offer_fire(
//         record_h.clone(),
//         parked_waiter(2, "u", exactly(1)),
//         ListParWithRandom::default(),
//     );
//     let charges = [
//         consume_charges(&take).expect("the charges of the take"),
//         produce_charges(&put).expect("the charges of the put"),
//         produce_charges(&plan_fire).expect("the charges of the plan fire"),
//         consume_charges(&lift).expect("the charges of the lift"),
//         produce_charges(&reoffer).expect("the charges of the re-offer"),
//     ];
//     let expected: Vec<Cost> = charges.iter().cloned().flat_map(split).collect();
//     assert_eq!(
//         agent_log, expected,
//         "the chain of the staged operations, in staging order"
//     );
//
//     // The host charges the three shapes that it can run alone as the formulas do.
//     {
//         let (host_budget, host) = legacy_host().await;
//         let _scope = host_budget.enter_legacy_scope();
//         host.produce(counter_ch(), stored_counter, false)
//             .await
//             .expect("the counter record");
//         let run = host_consume(
//             &host,
//             &host_budget,
//             vec![counter_ch()],
//             vec![counter_pattern()],
//             take_continuation,
//             false,
//             false,
//         )
//         .await;
//         assert!(run.fired.is_some(), "the take fires");
//         assert_eq!(run.entries, split(charges[0].clone()), "the take");
//     }
//     {
//         let (host_budget, host) = legacy_host().await;
//         let _scope = host_budget.enter_legacy_scope();
//         let run = host_produce(&host, &host_budget, counter_ch(), next_counter, false).await;
//         assert!(run.fired.is_none(), "the put stores");
//         assert_eq!(run.entries, split(charges[1].clone()), "the put");
//     }
//     {
//         let (host_budget, host) = legacy_host().await;
//         let _scope = host_budget.enter_legacy_scope();
//         host.produce(data(), record_h, false)
//             .await
//             .expect("the store of h");
//         let run = host_consume(
//             &host,
//             &host_budget,
//             vec![data()],
//             vec![lift_pattern()],
//             k_lift,
//             false,
//             false,
//         )
//         .await;
//         assert!(run.fired.is_some(), "the lift fires");
//         assert_eq!(run.entries, split(charges[3].clone()), "the lift");
//     }
// }

/// Added by decision 90 (decision 69): the host differential of the retired
/// `space_disciplines_pq_legacy_charges_equal_host` compares the observer calls. The send of 0
/// stages five operations: the take and the put of the counter, its fire of b, the lift of the
/// record 1 and its re-offer to u. The agent makes the observer calls of five operations in
/// staging order. Each operation makes the observer calls of its host call, with the same
/// arguments:
///
/// * the take, the put and the lift: on a fresh host that holds the record that the call binds;
/// * the plan fire: on a lone host that holds b (`lone_host_produce`);
/// * the re-offer: on a host that stored the record 1, lifted it and holds u. The COMM of the
///   re-offer counts the earlier store of the record in its `times_repeated`, so the host makes
///   that store too.
///
/// Each row of the space names the footprint group of decision 92, which the host calls do not
/// have. So the comparison removes that group from the calls of the agent (`without_footprint`).
#[tokio::test]
async fn space_disciplines_pq_observer_calls_equal_host() {
    let space = play().await;
    let env = env_over(space.clone(), &budget());
    create(&pq(), &env, pq_creation(None)).await;
    let h = ProduceOp {
        authority: region(2),
        data: ListParWithRandom {
            cost_authority: Some(region(2)),
            ..send_at(1, Some(1)).data
        },
        ..send_at(1, Some(1))
    };
    produce(&env, h.clone()).await;
    let b = receive_at("b", exactly(0), None);
    let u = receive_at("u", exactly(1), None);
    consume(&env, b).await;
    consume(&env, u.clone()).await;
    let d = send_at(0, Some(0));
    let recorder = observed(&space);
    assert_eq!(fired(&produce(&env, d.clone()).await), vec![
        literal("b"),
        literal("u")
    ]);
    // Changed by decision 92: each row of the space names the footprint group, which the host
    // calls do not have.
    let agent = operations(&without_footprint(
        &recorder.take(),
        &space_counter_channel(SPACE),
    ));
    assert_eq!(agent.len(), 5, "the send stages five operations");

    // The counter record that the bump of u stored, and the records of the round of d.
    let random_u = Blake2b512Random::from_bytes(&u.body.random_state);
    let random_d = Blake2b512Random::from_bytes(&d.data.random_state);
    let stored_counter = record::counter_record(
        3,
        &RandomPurpose::Counter.derive(&random_u).split_byte(1),
        None,
    );
    let take_continuation = counter_continuation(
        3,
        &RandomPurpose::Counter.derive(&random_d).split_byte(0),
        None,
    );
    let next_counter = record::counter_record(
        4,
        &RandomPurpose::Counter.derive(&random_d).split_byte(1),
        None,
    );
    let record_d = record::wrap_pq_datum(3, Some(0), &d.data);
    let record_h = record::wrap_pq_datum(0, Some(1), &h.data);
    let k_lift = lift_continuation(3, 1, &lift_random(&d.data.random_state), None);

    // The take of the counter record.
    {
        let (host_recorder, host) = recorded_host().await;
        host.produce(counter_ch(), stored_counter, false)
            .await
            .expect("the counter record");
        let run = host_consume(
            &host,
            &host_recorder,
            vec![counter_ch()],
            vec![counter_pattern()],
            take_continuation,
            false,
            false,
        )
        .await;
        assert!(run.fired.is_some(), "the take fires");
        assert_eq!(run.calls, agent[0], "the take");
    }
    // The put of the next counter record.
    {
        let (host_recorder, host) = recorded_host().await;
        let run = host_produce(&host, &host_recorder, counter_ch(), next_counter, false).await;
        assert!(run.fired.is_none(), "the put stores");
        assert_eq!(run.calls, agent[1], "the put");
    }
    // The plan fire of b.
    let plan_fire = offer_fire(
        record_d,
        parked_waiter(1, "b", exactly(0)),
        ListParWithRandom::default(),
    );
    assert_eq!(
        lone_host_produce(&plan_fire).await,
        agent[2],
        "the plan fire"
    );
    // The lift of the record 1.
    {
        let (host_recorder, host) = recorded_host().await;
        host.produce(data(), record_h.clone(), false)
            .await
            .expect("the store of h");
        let run = host_consume(
            &host,
            &host_recorder,
            vec![data()],
            vec![lift_pattern()],
            k_lift.clone(),
            false,
            // Added by decision 124 (plan v2 C7.5, rule CR-21): the COMM of a `K_lift` fire
            // peeks its channel, so the host lift peeks too.
            true,
        )
        .await;
        assert!(run.fired.is_some(), "the lift fires");
        assert_eq!(run.calls, agent[3], "the lift");
    }
    // The re-offer of the record 1 to u. The host matches payloads, as the agent matches them.
    {
        let host_recorder = Recorder::new();
        let pin: Pin = Arc::new(Mutex::new(None));
        let host = pinned_observed_host(&host_recorder, &pin, &open_waiter()).await;
        host.produce(data(), record_h.clone(), false)
            .await
            .expect("the store of h");
        let lifted = host
            .consume(
                vec![data()],
                vec![lift_pattern()],
                k_lift,
                false,
                BTreeSet::new(),
            )
            .await
            .expect("the lift of h");
        assert!(lifted.is_some(), "the lift fires");
        let waiter_u = parked_waiter(2, "u", exactly(1));
        let parked = host
            .consume(
                vec![data()],
                waiter_u.patterns,
                waiter_u.continuation,
                false,
                BTreeSet::new(),
            )
            .await
            .expect("the park of u");
        assert!(parked.is_none(), "u parks");
        let run = host_produce(&host, &host_recorder, data(), record_h, false).await;
        assert!(run.fired.is_some(), "the re-offer fires u");
        assert_eq!(run.calls, agent[4], "the re-offer");
    }
}

// ─── Properties ──────────────────────────────────────────────────────────────────────────────

/// The slots of a test entry for `lane_heads`: `(level slot, ord, payload)`.
type LevelEntry = (Option<i64>, i64, String);

fn level_entry_slots(entry: &LevelEntry) -> Result<(i64, Level), SpaceError> {
    Ok((entry.1, Level::of_slot(entry.0)))
}

/// `pq_level_order` and `pq_unprioritized_lowest`: the head of each level is its entry with the
/// least order, the heads come in ascending level order with the unprioritized level last, the
/// list order does not matter, and a lane sees exactly its levels.
#[test]
fn prop_pq_level_order() {
    let entries = vec(
        (
            prop_oneof![Just(None), (0i64..4).prop_map(Some)],
            "[a-c]{0,2}",
        ),
        0..10,
    );
    check(
        pure_config(),
        (entries, arbitrary::<u64>()),
        |(entries, seed)| {
            // Distinct orders in a scrambled list order, as one counter and a reloaded store make
            // them: 7 is coprime with 13, so the orders of the at most ten entries differ.
            let entries: Vec<LevelEntry> = entries
                .into_iter()
                .enumerate()
                .map(|(index, (level, payload))| {
                    let ord = i64::try_from(index * 7 % 13).expect("a small order");
                    (level, ord, payload)
                })
                .collect();
            let heads = PriorityQueueDiscipline
                .lane_heads(Side::Data, &entries, level_entry_slots)
                .map_err(|error| TestCaseError::fail(format!("{error}")))?;
            let levels: Vec<Level> = heads.keys().copied().collect();
            let expected_levels: Vec<Level> = entries
                .iter()
                .map(|entry| Level::of_slot(entry.0))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            prop_assert_eq!(&levels, &expected_levels);
            for window in levels.windows(2) {
                prop_assert!(window[0] < window[1]);
            }
            if let Some(last) = levels.last() {
                if levels.contains(&Level::Unprioritized) {
                    prop_assert_eq!(*last, Level::Unprioritized);
                }
            }
            for (level, head) in &heads {
                let least = entries
                    .iter()
                    .filter(|entry| Level::of_slot(entry.0) == *level)
                    .map(|entry| entry.1)
                    .min();
                prop_assert_eq!(Some(head.ord), least);
                prop_assert_eq!(Level::of_slot(entries[head.index].0), *level);
            }
            // A rotation of the list gives the same heads.
            let mut rotated = entries.clone();
            if !rotated.is_empty() {
                let shift = usize::try_from(seed % rotated.len() as u64).expect("a small shift");
                rotated.rotate_left(shift);
            }
            let rotated_heads = PriorityQueueDiscipline
                .lane_heads(Side::Data, &rotated, level_entry_slots)
                .map_err(|error| TestCaseError::fail(format!("{error}")))?;
            for (level, head) in &rotated_heads {
                let original = heads
                    .get(level)
                    .ok_or_else(|| TestCaseError::fail("a level"))?;
                prop_assert_eq!(&rotated[head.index], &entries[original.index]);
            }
            // A lane sees its own level, and the unrestricted lane sees every level.
            for level in &levels {
                prop_assert!(Lane::Any.sees(*level));
                for lane_level in 0..4 {
                    prop_assert_eq!(
                        Lane::Level(lane_level).sees(*level),
                        *level == Level::Priority(lane_level)
                    );
                }
            }
            Ok(())
        },
    );
}

/// A pattern of a generated receive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pat {
    Lit(i64),
    Any,
}

impl Pat {
    fn matches(self, value: i64) -> bool {
        match self {
            Pat::Lit(expected) => expected == value,
            Pat::Any => true,
        }
    }

    fn bind_pattern(self) -> BindPattern {
        match self {
            Pat::Lit(value) => exactly(value),
            Pat::Any => any(),
        }
    }
}

/// One operation of a generated schedule. A guard `x == 2` comes only with the pattern of every
/// value, which binds `x`.
#[derive(Clone, Debug)]
enum PqStep {
    Push {
        value: i64,
        level: Option<i64>,
        persistent: bool,
    },
    Pop {
        pattern: Pat,
        guard: bool,
        lane: Option<i64>,
        persistent: bool,
        peek: bool,
    },
}

/// A fresh operation of a schedule: a send of 1 or 2 at the level 0, 1 or the unprioritized
/// level, persistent or not, or a receive of 1, of 2 or of every value (with or without the guard
/// `x == 2`) in the lane of the level 0, 1 or the unrestricted lane, persistent, peeking or plain.
/// A persistent receive never peeks.
fn arb_fresh_step() -> BoxedStrategy<PqStep> {
    let level = prop_oneof![Just(None), Just(Some(0)), Just(Some(1))];
    prop_oneof![
        (1i64..=2, level.clone(), arbitrary::<bool>()).prop_map(|(value, level, persistent)| {
            PqStep::Push {
                value,
                level,
                persistent,
            }
        }),
        (
            prop_oneof![Just(Pat::Lit(1)), Just(Pat::Lit(2)), Just(Pat::Any)],
            arbitrary::<bool>(),
            level,
            arbitrary::<bool>(),
            arbitrary::<bool>()
        )
            .prop_map(|(pattern, guard, lane, persistent, peek)| PqStep::Pop {
                pattern,
                guard: guard && pattern == Pat::Any,
                lane,
                persistent: persistent && !peek,
                peek,
            }),
    ]
    .boxed()
}

/// `steps` with persistent sends or persistent receives, never both, so it has no divergent pair
/// and no re-issue.
fn without_divergence(persistent_sends: bool, steps: Vec<PqStep>) -> Vec<PqStep> {
    steps
        .into_iter()
        .map(|step| match step {
            PqStep::Push {
                value,
                level,
                persistent,
            } => PqStep::Push {
                value,
                level,
                persistent: persistent && persistent_sends,
            },
            PqStep::Pop {
                pattern,
                guard,
                lane,
                persistent,
                peek,
            } => PqStep::Pop {
                pattern,
                guard,
                lane,
                persistent: persistent && !persistent_sends,
                peek,
            },
        })
        .collect()
}

/// A schedule of up to eleven operations on one priority queue, with no divergent pair.
fn arb_schedule() -> BoxedStrategy<Vec<PqStep>> {
    (arbitrary::<bool>(), vec(arb_fresh_step(), 1..12))
        .prop_map(|(persistent_sends, steps)| without_divergence(persistent_sends, steps))
        .boxed()
}

/// The contested settle of S4 as a schedule prefix, in one of its two arrival orders.
fn contested_prefix(restricted_first: bool) -> Vec<PqStep> {
    let pop = |value: i64, lane: Option<i64>| PqStep::Pop {
        pattern: Pat::Lit(value),
        guard: false,
        lane,
        persistent: false,
        peek: false,
    };
    let push = |value: i64, level: Option<i64>| PqStep::Push {
        value,
        level,
        persistent: false,
    };
    let mut steps = vec![
        push(1, Some(1)),
        push(2, Some(1)),
        pop(0, None),
        pop(1, None),
    ];
    match restricted_first {
        false => steps.extend([pop(2, None), pop(2, Some(1))]),
        true => steps.extend([pop(2, Some(1)), pop(2, None)]),
    }
    steps.push(push(0, Some(0)));
    steps
}

/// A schedule that often contests a settle: no prefix or one of the two orders of the S4 prefix,
/// then up to eight generated operations, with no divergent pair.
fn arb_contested_schedule() -> BoxedStrategy<Vec<PqStep>> {
    let prefix = prop_oneof![
        Just(Vec::new()),
        Just(contested_prefix(false)),
        Just(contested_prefix(true))
    ];
    (prefix, arbitrary::<bool>(), vec(arb_fresh_step(), 0..9))
        .prop_map(|(mut schedule, persistent_sends, steps)| {
            schedule.extend(steps);
            without_divergence(persistent_sends, schedule)
        })
        .boxed()
}

/// A record of the reference model.
#[derive(Clone, Debug)]
struct ModelRecord {
    ord: i64,
    level: Level,
    value: i64,
    persistent: bool,
}

/// A waiter of the reference model.
#[derive(Clone, Debug)]
struct ModelWaiter {
    ord: i64,
    lane: Lane,
    name: String,
    pattern: Pat,
    guard: bool,
    persistent: bool,
    peek: bool,
}

impl ModelWaiter {
    /// True when the guard of the waiter admits `value`.
    fn admits(&self, value: i64) -> bool { !self.guard || value == 2 }
}

/// The reference model of a priority queue: the rules of the module documentation of
/// `priority_queue.rs`, written without the engine. Each level and each lane is FIFO, a level
/// lane sees its level and the unrestricted lane sees every level in ascending order, a peek
/// keeps a non-persistent record by its re-store, and the settle fires the fireable waiter-lane
/// head with the least order while one exists. It counts the fires of each settle, and the settle
/// rounds where two or more heads could fire.
#[derive(Clone, Debug, Default)]
struct Model {
    records: Vec<ModelRecord>,
    waiters: Vec<ModelWaiter>,
    counter: i64,
}

/// The fires of one operation of the model, the fires of its settles, and its contested rounds.
#[derive(Debug, Default)]
struct ModelRun {
    fires: Vec<(String, Vec<Par>)>,
    settle_fires: usize,
    contested: usize,
}

impl Model {
    /// The index of the head of the level `level`: its record with the least order.
    fn record_head(&self, level: Level) -> Option<usize> {
        (0..self.records.len())
            .filter(|index| self.records[*index].level == level)
            .min_by_key(|index| self.records[*index].ord)
    }

    /// The index of the head of the lane `lane`.
    fn waiter_head(&self, lane: Lane) -> Option<usize> {
        (0..self.waiters.len())
            .filter(|index| self.waiters[*index].lane == lane)
            .min_by_key(|index| self.waiters[*index].ord)
    }

    /// The levels of the records, ascending.
    fn levels(&self) -> Vec<Level> {
        self.records
            .iter()
            .map(|record| record.level)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// The lanes of the waiters.
    fn lanes(&self) -> Vec<Lane> {
        self.waiters
            .iter()
            .map(|waiter| waiter.lane)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// The first level head, in ascending level order, that `lane` sees and whose value
    /// `pattern` matches.
    fn first_match(&self, lane: Lane, pattern: Pat) -> Option<usize> {
        self.levels()
            .into_iter()
            .filter(|level| lane.sees(*level))
            .filter_map(|level| self.record_head(level))
            .find(|index| pattern.matches(self.records[*index].value))
    }

    /// The record that the waiter head `waiter` fires in a settle: its first match, when its
    /// guard admits it (decision 38).
    fn fireable(&self, waiter: &ModelWaiter) -> Option<usize> {
        self.first_match(waiter.lane, waiter.pattern)
            .filter(|index| waiter.admits(self.records[*index].value))
    }

    /// True when some waiter head can fire: a pending match (INV fails).
    fn pending(&self) -> bool {
        self.lanes().into_iter().any(|lane| {
            self.waiter_head(lane)
                .and_then(|waiter| self.fireable(&self.waiters[waiter]))
                .is_some()
        })
    }

    /// The settle: the fireable waiter head with the least order fires, while one exists.
    fn settle(&mut self, run: &mut ModelRun) {
        loop {
            let candidates: Vec<(i64, usize, usize)> = self
                .lanes()
                .into_iter()
                .filter_map(|lane| self.waiter_head(lane))
                .filter_map(|waiter| {
                    self.fireable(&self.waiters[waiter])
                        .map(|record| (self.waiters[waiter].ord, waiter, record))
                })
                .collect();
            if candidates.len() >= 2 {
                run.contested += 1;
            }
            let Some((_, waiter, record)) = candidates.into_iter().min_by_key(|(ord, ..)| *ord)
            else {
                return;
            };
            let fired = self.waiters[waiter].clone();
            let stored = self.records[record].clone();
            run.fires.push(fire_of(&fired, stored.value));
            run.settle_fires += 1;
            if !stored.persistent && !fired.peek {
                self.records.retain(|kept| kept.ord != stored.ord);
            }
            if !fired.persistent {
                self.waiters.retain(|kept| kept.ord != fired.ord);
            }
        }
    }

    fn bump(&mut self) -> i64 {
        let ord = self.counter;
        self.counter += 1;
        ord
    }

    fn push(&mut self, value: i64, level: Option<i64>, persistent: bool) -> ModelRun {
        let mut run = ModelRun::default();
        self.settle(&mut run);
        let ord = self.bump();
        let level = Level::of_slot(level);
        let record = ModelRecord {
            ord,
            level,
            value,
            persistent,
        };
        if self.record_head(level).is_some() {
            self.records.push(record);
            self.settle(&mut run);
            return run;
        }
        let lanes = level.candidate_lanes();
        let mut refused = [false; 2];
        loop {
            let mut candidates: Vec<(i64, usize, usize)> = lanes
                .iter()
                .enumerate()
                .filter(|(slot, _)| !refused[*slot])
                .filter_map(|(slot, lane)| {
                    lane.and_then(|lane| self.waiter_head(lane))
                        .map(|waiter| (self.waiters[waiter].ord, slot, waiter))
                })
                .collect();
            candidates.sort_by_key(|(ord, ..)| *ord);
            let mut chosen = None;
            for (_, slot, waiter) in candidates {
                let candidate = &self.waiters[waiter];
                match candidate.pattern.matches(value) && candidate.admits(value) {
                    true => {
                        chosen = Some(waiter);
                        break;
                    }
                    false => refused[slot] = true,
                }
            }
            let Some(waiter) = chosen else {
                self.records.push(record);
                self.settle(&mut run);
                return run;
            };
            let fired = self.waiters[waiter].clone();
            run.fires.push(fire_of(&fired, value));
            if !fired.persistent {
                self.waiters.retain(|kept| kept.ord != fired.ord);
            }
            if !persistent && !fired.peek {
                self.settle(&mut run);
                return run;
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn pop(
        &mut self,
        name: &str,
        pattern: Pat,
        guard: bool,
        lane: Option<i64>,
        persistent: bool,
        peek: bool,
    ) -> ModelRun {
        let mut run = ModelRun::default();
        self.settle(&mut run);
        let ord = self.bump();
        let lane = Lane::of_slot(lane);
        let waiter = ModelWaiter {
            ord,
            lane,
            name: name.to_string(),
            pattern,
            guard,
            persistent,
            peek,
        };
        if self.waiter_head(lane).is_some() {
            self.waiters.push(waiter);
            self.settle(&mut run);
            return run;
        }
        loop {
            let target = self
                .first_match(lane, pattern)
                .filter(|index| waiter.admits(self.records[*index].value));
            let Some(record) = target else {
                self.waiters.push(waiter);
                self.settle(&mut run);
                return run;
            };
            let stored = self.records[record].clone();
            run.fires.push(fire_of(&waiter, stored.value));
            if !stored.persistent && !peek {
                self.records.retain(|kept| kept.ord != stored.ord);
            }
            if !persistent {
                self.settle(&mut run);
                return run;
            }
        }
    }

    fn records(&self) -> Vec<RecordRow> {
        let mut records: Vec<_> = self
            .records
            .iter()
            .map(|record| {
                (
                    record.ord,
                    record.level.slot(),
                    vec![int(record.value)],
                    record.persistent,
                )
            })
            .collect();
        records.sort_by_key(|(ord, ..)| *ord);
        records
    }

    fn waiters(&self) -> Vec<WaiterRow> {
        let mut waiters: Vec<_> = self
            .waiters
            .iter()
            .map(|waiter| {
                (
                    waiter.ord,
                    waiter.lane.slot(),
                    waiter.name.clone(),
                    waiter.persistent,
                    waiter.peek,
                )
            })
            .collect();
        waiters.sort_by_key(|(ord, ..)| *ord);
        waiters
    }

    /// The stored non-persistent entries: the measure of the termination proof.
    fn measure(&self) -> usize {
        self.records
            .iter()
            .filter(|record| !record.persistent)
            .count()
            + self
                .waiters
                .iter()
                .filter(|waiter| !waiter.persistent)
                .count()
    }
}

/// The fire of the model waiter `waiter` with the value `value`, as `fired` reads an outcome.
fn fire_of(waiter: &ModelWaiter, value: i64) -> (String, Vec<Par>) {
    match waiter.pattern {
        Pat::Any => delivery(&waiter.name, value),
        Pat::Lit(_) => literal(&waiter.name),
    }
}

/// The operation of the step `index` of a schedule.
fn op_of(index: usize, step_of: &PqStep) -> SpaceOp {
    match step_of {
        PqStep::Push {
            value,
            level,
            persistent,
        } => SpaceOp::Produce(ProduceOp {
            data: ListParWithRandom {
                random_state: Blake2b512Random::create_from_bytes(
                    format!("pq schedule push {index}").as_bytes(),
                )
                .to_bytes(),
                ..send_at(*value, *level).data
            },
            persistent: *persistent,
            ..send_at(*value, *level)
        }),
        PqStep::Pop {
            pattern,
            guard,
            lane,
            persistent,
            peek,
        } => {
            let op = ConsumeOp {
                persistent: *persistent,
                peek: *peek,
                ..receive_at(&format!("w{index}"), pattern.bind_pattern(), *lane)
            };
            SpaceOp::Consume(match guard {
                true => guarded(op, 2),
                false => op,
            })
        }
    }
}

/// Applies the step `index` to the model and returns its run.
fn model_step(model: &mut Model, index: usize, step_of: &PqStep) -> ModelRun {
    match step_of {
        PqStep::Push {
            value,
            level,
            persistent,
        } => model.push(*value, *level, *persistent),
        PqStep::Pop {
            pattern,
            guard,
            lane,
            persistent,
            peek,
        } => model.pop(
            &format!("w{index}"),
            *pattern,
            *guard,
            *lane,
            *persistent,
            *peek,
        ),
    }
}

/// True when a head of a waiter lane in the store can fire on the level heads that it sees: its
/// first level head that its pattern matches passes its guard (INV fails). The test matches with
/// the matcher of the host.
async fn pending_match(space: &ParRSpace) -> bool {
    let stored: Vec<(i64, Level, ListParWithRandom)> = space
        .get_data(&data())
        .await
        .into_iter()
        .map(|datum| {
            let decoded = record::unwrap_pq_datum(&datum.a).expect("a datum record");
            (decoded.ord, Level::of_slot(decoded.priority), decoded.datum)
        })
        .collect();
    let parked: Vec<(i64, Lane, BindPattern, TaggedContinuation)> = space
        .get_waiting_continuations(vec![data()])
        .await
        .into_iter()
        .map(|waiter| {
            let (ord, lane) =
                record::pq_continuation_slots(&waiter.continuation).expect("a continuation");
            let pattern = waiter.patterns.first().cloned().expect("one bind");
            (ord, Lane::of_slot(lane), pattern, waiter.continuation)
        })
        .collect();
    let mut level_heads: BTreeMap<Level, (i64, ListParWithRandom)> = BTreeMap::new();
    for (ord, level, payload) in stored {
        let replace = level_heads.get(&level).is_none_or(|(best, _)| ord < *best);
        if replace {
            level_heads.insert(level, (ord, payload));
        }
    }
    let mut lane_heads: BTreeMap<Lane, (i64, BindPattern, TaggedContinuation)> = BTreeMap::new();
    for (ord, lane, pattern, continuation) in parked {
        let replace = lane_heads.get(&lane).is_none_or(|(best, ..)| ord < *best);
        if replace {
            lane_heads.insert(lane, (ord, pattern, continuation));
        }
    }
    lane_heads.iter().any(|(lane, (_, pattern, continuation))| {
        level_heads
            .iter()
            .filter(|(level, _)| lane.sees(**level))
            .find_map(|(_, (_, payload))| Matcher.get(pattern, payload))
            .is_some_and(|matched| Matcher.check_commit(continuation, &[matched]))
    })
}

/// Runs `schedule` on a fresh priority queue and on the model, and checks `check_step` after each
/// step. With `check_store` it also compares the stores and checks INV after each step.
async fn schedule_case(
    schedule: Vec<PqStep>,
    mut check_step: impl FnMut(&ModelRun, &SpaceOutcome, usize) -> Result<(), TestCaseError>,
    check_store: bool,
) -> Result<(), TestCaseError> {
    let Fixture { space, env } = fixture().await;
    let mut model = Model::default();
    for (index, step_of) in schedule.iter().enumerate() {
        let measure = model.measure();
        let expected = model_step(&mut model, index, step_of);
        let outcome = run(&pq(), &env, op_of(index, step_of))
            .await
            .map_err(|error| TestCaseError::fail(format!("step {index}: {error}")))?;
        prop_assert_eq!(&outcome.reissue, &None, "step {}: no re-issue", index);
        check_step(&expected, &outcome, measure)?;
        if check_store {
            prop_assert_eq!(records(&space).await, model.records(), "step {}", index);
            prop_assert_eq!(waiters(&space).await, model.waiters(), "step {}", index);
            prop_assert!(
                !model.pending(),
                "step {}: the model has a pending match",
                index
            );
            prop_assert!(
                !pending_match(&space).await,
                "step {}: a waiter head can fire",
                index
            );
        }
    }
    Ok(())
}

/// `PqLevelOrder`, `pq_fifo_within_level`, `pq_head_blocks_within_level`,
/// `pq_restricted_waiter_in_level_queue` and `pq_unrestricted_first_matching_level`: the agent
/// fires exactly the fires of the reference model, in its order, and its store equals the store
/// of the model after every operation.
///
/// `NoRecordLost` and `pq_settle_keeps_unspent_records`: the reference model removes a record only
/// when a waiter that does not peek spends it. So the equal stores also show that no settle loses
/// a lifted record, also when a waiter that peeks leaves the record in flight.
#[test]
fn prop_pq_fifo_order_any_schedule() {
    check(pure_config(), arb_schedule(), |schedule| {
        tokio_runtime().block_on(schedule_case(
            schedule,
            |expected, outcome, _| {
                prop_assert_eq!(fired(outcome), expected.fires.clone());
                Ok(())
            },
            true,
        ))
    });
}

/// `PqNoPendingMatch`, `pq_produce_preserves_inv`, `pq_consume_preserves_inv` and
/// `pq_peek_restores_head`: after every completed operation no head of a waiter lane can fire on
/// the level heads that it sees.
#[test]
fn prop_pq_no_pending_match_any_schedule() {
    check(pure_config(), arb_schedule(), |schedule| {
        tokio_runtime().block_on(async {
            let Fixture { space, env } = fixture().await;
            for (index, step_of) in schedule.iter().enumerate() {
                let outcome = run(&pq(), &env, op_of(index, step_of))
                    .await
                    .map_err(|error| TestCaseError::fail(format!("step {index}: {error}")))?;
                prop_assert_eq!(&outcome.reissue, &None);
                prop_assert!(
                    !pending_match(&space).await,
                    "step {}: a waiter head can fire",
                    index
                );
            }
            Ok(())
        })
    });
}

/// `PqOldestFirst`, `pq_settle_oldest_first` and `pq_settle_order_matters`: the agent fires the
/// fires of the reference model, whose settle fires the fireable waiter head with the least order
/// first, on schedules that start with the contested settle of S4 in either arrival order. The
/// count at the end shows that the schedules reach settle rounds where two heads can fire.
#[test]
fn prop_pq_oldest_first_any_schedule() {
    let contested = AtomicUsize::new(0);
    check(pure_config(), arb_contested_schedule(), |schedule| {
        tokio_runtime().block_on(schedule_case(
            schedule,
            |expected, outcome, _| {
                contested.fetch_add(expected.contested, Ordering::Relaxed);
                prop_assert_eq!(fired(outcome), expected.fires.clone());
                Ok(())
            },
            true,
        ))
    });
    let reached = contested.load(Ordering::Relaxed);
    eprintln!("prop_pq_oldest_first_any_schedule reached {reached} contested settle rounds");
    assert!(reached > 0, "the schedules never contest a settle");
}

/// `pq_settle_terminates`: the settles of an operation fire at most once for each stored entry
/// that is not persistent, and the operation fires at most once more for its own element. One
/// transaction makes at most `FIRE_ROUND_CAP` fires, its settles included (I-21).
#[test]
fn prop_pq_settle_bounded_any_schedule() {
    check(pure_config(), arb_schedule(), |schedule| {
        tokio_runtime().block_on(schedule_case(
            schedule,
            |expected, outcome, measure| {
                prop_assert!(expected.settle_fires <= measure);
                prop_assert!(outcome.fires.len() <= measure + 1);
                prop_assert!(outcome.fires.len() <= FIRE_ROUND_CAP);
                Ok(())
            },
            false,
        ))
    });
}

/// `pq_plan_permutation_invariant`: a restart reloads the store in another list order, and the
/// plans do not change: a run with a checkpoint and a reset after every operation fires and
/// stores what a run without them does.
#[test]
fn prop_pq_plan_permutation_invariant() {
    check(pure_config(), arb_schedule(), |schedule| {
        tokio_runtime().block_on(async {
            let plain = fixture().await;
            let restarted = fixture().await;
            for (index, step_of) in schedule.iter().enumerate() {
                let expected = run(&pq(), &plain.env, op_of(index, step_of))
                    .await
                    .map_err(|error| TestCaseError::fail(format!("{error}")))?;
                let root = restarted
                    .space
                    .create_checkpoint()
                    .await
                    .map_err(|error| TestCaseError::fail(format!("{error}")))?
                    .root;
                restarted
                    .space
                    .reset(&root)
                    .await
                    .map_err(|error| TestCaseError::fail(format!("{error}")))?;
                let outcome = run(&pq(), &restarted.env, op_of(index, step_of))
                    .await
                    .map_err(|error| TestCaseError::fail(format!("{error}")))?;
                prop_assert_eq!(&outcome, &expected, "step {}", index);
            }
            prop_assert_eq!(records(&restarted.space).await, records(&plain.space).await);
            prop_assert_eq!(waiters(&restarted.space).await, waiters(&plain.space).await);
            Ok(())
        })
    });
}

/// One round of a generated schedule with re-issues: a fresh operation, or the next round of the
/// oldest re-issued operation or settle job.
#[derive(Clone, Debug)]
enum RoundStep {
    Fresh(PqStep),
    Reenter,
}

/// A stored divergent pair behind a blocker: the blocker of the unrestricted queue (only 2), a
/// persistent waiter of every value behind it, the persistent send of 1 at the level 0 (stored,
/// because the blocker does not fit it), and the send of 2 at the level 1, which fires the
/// blocker and exposes the pair. The settle after that send re-issues a settle job. The settle
/// before the plan of each later operation meets the pair, so it re-issues that operation.
fn exposed_divergent_pair() -> Vec<RoundStep> {
    vec![
        RoundStep::Fresh(PqStep::Pop {
            pattern: Pat::Lit(2),
            guard: false,
            lane: None,
            persistent: false,
            peek: false,
        }),
        RoundStep::Fresh(PqStep::Pop {
            pattern: Pat::Any,
            guard: false,
            lane: None,
            persistent: true,
            peek: false,
        }),
        RoundStep::Fresh(PqStep::Push {
            value: 1,
            level: Some(0),
            persistent: true,
        }),
        RoundStep::Fresh(PqStep::Push {
            value: 2,
            level: Some(1),
            persistent: false,
        }),
    ]
}

/// A re-entry ahead of a younger head of its lane: the persistent send of 1 at the level 0, the
/// persistent receive of every value of the level 0 (a divergent pair, so the receive re-issues
/// with its first-round order), the receive of 2 of the level 0 (it parks while the persistent
/// receive is in flight), and the re-entry of the persistent receive, which is older.
fn reentry_ahead_of_younger_head() -> Vec<RoundStep> {
    vec![
        RoundStep::Fresh(PqStep::Push {
            value: 1,
            level: Some(0),
            persistent: true,
        }),
        RoundStep::Fresh(PqStep::Pop {
            pattern: Pat::Any,
            guard: false,
            lane: Some(0),
            persistent: true,
            peek: false,
        }),
        RoundStep::Fresh(PqStep::Pop {
            pattern: Pat::Lit(2),
            guard: false,
            lane: Some(0),
            persistent: false,
            peek: false,
        }),
        RoundStep::Reenter,
    ]
}

/// A schedule of up to fifteen rounds: no prefix or one of the two prefixes above, then up to
/// eleven generated rounds. Persistent sends and persistent receives can occur in one schedule,
/// so divergent pairs re-issue operations and settle jobs (A10), and the re-issued rounds
/// interleave with fresh operations. A persistent receive never peeks.
fn arb_reissue_schedule() -> BoxedStrategy<Vec<RoundStep>> {
    let prefix = prop_oneof![
        Just(Vec::new()),
        Just(exposed_divergent_pair()),
        Just(reentry_ahead_of_younger_head())
    ];
    let rounds = vec(
        prop_oneof![3 => arb_fresh_step().prop_map(RoundStep::Fresh), 2 => Just(RoundStep::Reenter)],
        1..12,
    );
    (prefix, rounds)
        .prop_map(|(mut schedule, rounds)| {
            schedule.extend(rounds);
            schedule
        })
        .boxed()
}

/// The persistence and the carried order of the operation `op`, or `None` for a settle job.
fn carried_ord(op: &SpaceOp) -> Option<(bool, Option<i64>)> {
    match op {
        SpaceOp::Produce(produce) => Some((produce.persistent, produce.reissued_ord)),
        SpaceOp::Consume(consume) => Some((consume.persistent, consume.reissued_ord)),
        _ => None,
    }
}

/// The operation `op` with the carried order `reissued_ord`.
fn carrying(op: &SpaceOp, reissued_ord: Option<i64>) -> SpaceOp {
    match op {
        SpaceOp::Produce(produce) => SpaceOp::Produce(ProduceOp {
            reissued_ord,
            ..produce.clone()
        }),
        SpaceOp::Consume(consume) => SpaceOp::Consume(ConsumeOp {
            reissued_ord,
            ..consume.clone()
        }),
        other => other.clone(),
    }
}

/// The integer of a `Par` that holds exactly one `GInt`.
fn gint_of(par: &Par) -> Option<i64> {
    match par.exprs.as_slice() {
        [Expr {
            expr_instance: Some(ExprInstance::GInt(value)),
        }] => Some(*value),
        _ => None,
    }
}

/// The orders of the entries of the lane of the operation `op`: the records of the level of a
/// send, or the waiters of the lane of a receive.
async fn own_lane(space: &ParRSpace, op: &SpaceOp) -> Vec<i64> {
    match op {
        SpaceOp::Produce(produce) => {
            let level = produce.params.get(PRIORITY_PARAM).and_then(gint_of);
            records(space)
                .await
                .into_iter()
                .filter(|(_, stored, ..)| *stored == level)
                .map(|(ord, ..)| ord)
                .collect()
        }
        SpaceOp::Consume(consume) => {
            let lane = consume
                .query
                .as_ref()
                .and_then(|query| query.named.get(PRIORITY_PARAM))
                .and_then(gint_of);
            waiters(space)
                .await
                .into_iter()
                .filter(|(_, stored, ..)| *stored == lane)
                .map(|(ord, ..)| ord)
                .collect()
        }
        SpaceOp::Settle(_) => Vec::new(),
        SpaceOp::Join(_) => Vec::new(),
    }
}

/// `ReissueKeepsOrd`, `pq_reissue_takes_level_fifo_place` and
/// `pq_reentered_waiter_keeps_seniority` (decision 43). On every schedule with divergent pairs, a
/// re-issued persistent operation carries the order of its first round, and a re-issued
/// operation that is not persistent carries none. A settle job re-issues itself. Every round
/// bumps the counter once. A persistent re-entry whose first-round order precedes every entry of
/// its lane takes the head of its lane: when it stays stored or parked, it is the oldest entry of
/// its lane. The counts at the end show that the schedules reach each of these cases.
#[test]
fn prop_pq_reissue_keeps_ord_any_schedule() {
    let persistent_reissues = AtomicUsize::new(0);
    let plain_reissues = AtomicUsize::new(0);
    let settle_reissues = AtomicUsize::new(0);
    let reentries_ahead = AtomicUsize::new(0);
    check(pure_config(), arb_reissue_schedule(), |schedule| {
        tokio_runtime().block_on(async {
            let Fixture { space, env } = fixture().await;
            let mut in_flight: VecDeque<SpaceOp> = VecDeque::with_capacity(schedule.len());
            let mut rounds = 0i64;
            for (index, round) in schedule.iter().enumerate() {
                let op = match round {
                    RoundStep::Fresh(step_of) => op_of(index, step_of),
                    RoundStep::Reenter => match in_flight.pop_front() {
                        Some(op) => op,
                        None => continue,
                    },
                };
                let carried = carried_ord(&op);
                // The first-round order: the carried order of a re-entry, else this round.
                let first = carried.and_then(|(_, ord)| ord).unwrap_or(rounds);
                let ahead = match carried {
                    Some((true, Some(ord))) => {
                        let lane = own_lane(&space, &op).await;
                        !lane.is_empty() && lane.iter().all(|other| ord < *other)
                    }
                    _ => false,
                };
                let outcome = run(&pq(), &env, op.clone())
                    .await
                    .map_err(|error| TestCaseError::fail(format!("round {index}: {error}")))?;
                rounds += 1;
                prop_assert_eq!(
                    counter_value(&space).await,
                    vec![rounds],
                    "round {}: one bump for each round",
                    index
                );
                if ahead {
                    reentries_ahead.fetch_add(1, Ordering::Relaxed);
                    let lane = own_lane(&space, &op).await;
                    if lane.contains(&first) {
                        prop_assert_eq!(
                            lane.iter().min().copied(),
                            Some(first),
                            "round {}: the re-entry keeps its seniority in its lane",
                            index
                        );
                    }
                }
                let Some(reissue) = outcome.reissue else {
                    continue;
                };
                match (&reissue.op, carried) {
                    (SpaceOp::Settle(_), Some(_)) => {
                        settle_reissues.fetch_add(1, Ordering::Relaxed);
                    }
                    (SpaceOp::Settle(_), None) => {
                        settle_reissues.fetch_add(1, Ordering::Relaxed);
                        prop_assert_eq!(
                            &reissue.op,
                            &op,
                            "round {}: a settle job re-issues itself",
                            index
                        );
                    }
                    (_, Some((persistent, _))) => {
                        let count = match persistent {
                            true => &persistent_reissues,
                            false => &plain_reissues,
                        };
                        count.fetch_add(1, Ordering::Relaxed);
                        prop_assert_eq!(
                            &reissue.op,
                            &carrying(&op, persistent.then_some(first)),
                            "round {}",
                            index
                        );
                    }
                    (_, None) => {
                        return Err(TestCaseError::fail(format!(
                            "round {index}: a settle job re-issued an operation"
                        )));
                    }
                }
                in_flight.push_back(reissue.op);
            }
            Ok(())
        })
    });
    let reached = [
        (
            "a persistent re-issue",
            persistent_reissues.load(Ordering::Relaxed),
        ),
        (
            "a re-issue that is not persistent",
            plain_reissues.load(Ordering::Relaxed),
        ),
        (
            "a re-issued settle job",
            settle_reissues.load(Ordering::Relaxed),
        ),
        (
            "a re-entry ahead of a younger head of its lane",
            reentries_ahead.load(Ordering::Relaxed),
        ),
    ];
    eprintln!("prop_pq_reissue_keeps_ord_any_schedule reached: {reached:?}");
    for (case, count) in reached {
        assert!(count > 0, "the schedules never reach {case}");
    }
}

/// The number of pairs that the settle of a cap case exposes: one or two caps of pairs, and the
/// counts around them, so that every case reaches a cap stop that the counts decide.
fn arb_cap_case() -> BoxedStrategy<(i64, Vec<RoundStep>)> {
    let cap = cap();
    let counts = vec![
        cap - 2,
        cap - 1,
        cap,
        cap + 1,
        2 * cap - 2,
        2 * cap - 1,
        2 * cap,
        2 * cap + 1,
        2 * cap + 2,
    ];
    let tail = vec(
        prop_oneof![3 => arb_fresh_step().prop_map(RoundStep::Fresh), 2 => Just(RoundStep::Reenter)],
        0..6,
    );
    (proptest::sample::select(counts), tail).boxed()
}

/// I-21 for one transaction: at most `FIRE_ROUND_CAP` fires, and exactly `FIRE_ROUND_CAP` when the
/// transaction re-issues at the cap.
fn check_txn_fires(outcome: &SpaceOutcome) -> Result<(), TestCaseError> {
    prop_assert!(outcome.fires.len() <= FIRE_ROUND_CAP);
    if let Some(reissue) = &outcome.reissue {
        if reissue.cause == ReissueCause::RoundCap {
            prop_assert_eq!(outcome.fires.len(), FIRE_ROUND_CAP);
        }
    }
    Ok(())
}

/// `TxnFiresBounded` and `pq_settle_terminates` at the cap (I-21, decision 49). A blocker and a
/// persistent waiter of every value expose `count` pairs. The send that fires the blocker settles
/// them after its plan, and a fresh receive of another lane settles the rest before its plan. So
/// the counts decide which cap stop each case reaches:
///
/// * the settle after a plan re-issues a settle job (`count >= cap`);
/// * the settle before a plan stops at the cap with a pair left, and re-issues the operation
///   (`count >= 2 * cap`);
/// * the settle before a plan ends quiescent at the cap, and `open_plan` re-issues the operation
///   before the plan stages anything (`count = 2 * cap - 1`).
///
/// A random tail of fresh operations and re-entries then runs. Every transaction makes at most
/// `FIRE_ROUND_CAP` fires, and exactly `FIRE_ROUND_CAP` when it re-issues at the cap. The counts
/// at the end show that the cases reach each cap stop.
#[test]
fn prop_pq_txn_fires_bounded_at_the_cap() {
    let settle_after = AtomicUsize::new(0);
    let settle_before = AtomicUsize::new(0);
    let plan_open = AtomicUsize::new(0);
    let full_transactions = AtomicUsize::new(0);
    check(pure_config(), arb_cap_case(), |(count, tail)| {
        tokio_runtime().block_on(async {
            let Fixture { space, env } = fixture().await;
            let mut in_flight: VecDeque<SpaceOp> = VecDeque::with_capacity(tail.len() + 2);
            let observe = |outcome: &SpaceOutcome| -> Result<(), TestCaseError> {
                check_txn_fires(outcome)?;
                if outcome.fires.len() == FIRE_ROUND_CAP {
                    full_transactions.fetch_add(1, Ordering::Relaxed);
                }
                Ok(())
            };
            // The send of 999 fires the blocker, and its settle fires up to cap - 1 pairs.
            let blocker = exposed_pairs(&space, &env, count).await;
            observe(&blocker)?;
            let remaining = (count - (cap() - 1)).max(0);
            match &blocker.reissue {
                Some(reissue) => {
                    prop_assert_eq!(reissue.cause, ReissueCause::RoundCap);
                    prop_assert!(matches!(reissue.op, SpaceOp::Settle(_)));
                    prop_assert!(remaining > 0);
                    settle_after.fetch_add(1, Ordering::Relaxed);
                    in_flight.push_back(reissue.op.clone());
                }
                None => prop_assert_eq!(remaining, 0),
            }
            // A fresh receive of another lane, which no record fits, settles the remaining pairs
            // before its plan.
            let probe = consume(&env, receive_at("probe", exactly(-1), Some(7))).await;
            observe(&probe)?;
            match &probe.reissue {
                Some(reissue) => {
                    prop_assert_eq!(reissue.cause, ReissueCause::RoundCap);
                    prop_assert!(matches!(reissue.op, SpaceOp::Consume(_)));
                    prop_assert!(remaining >= cap());
                    match remaining == cap() {
                        true => plan_open.fetch_add(1, Ordering::Relaxed),
                        false => settle_before.fetch_add(1, Ordering::Relaxed),
                    };
                    in_flight.push_back(reissue.op.clone());
                }
                None => prop_assert!(remaining < cap()),
            }
            for (index, round) in tail.iter().enumerate() {
                let op = match round {
                    RoundStep::Fresh(step_of) => op_of(index + 100, step_of),
                    RoundStep::Reenter => match in_flight.pop_front() {
                        Some(op) => op,
                        None => continue,
                    },
                };
                let outcome = step(&env, op).await;
                observe(&outcome)?;
                if let Some(reissue) = outcome.reissue {
                    in_flight.push_back(reissue.op);
                }
            }
            Ok(())
        })
    });
    let reached = [
        (
            "a settle job re-issued at the cap",
            settle_after.load(Ordering::Relaxed),
        ),
        (
            "an operation re-issued by its settle at the cap",
            settle_before.load(Ordering::Relaxed),
        ),
        (
            "an operation re-issued when its plan opens at the cap",
            plan_open.load(Ordering::Relaxed),
        ),
        (
            "a transaction with FIRE_ROUND_CAP fires",
            full_transactions.load(Ordering::Relaxed),
        ),
    ];
    eprintln!("prop_pq_txn_fires_bounded_at_the_cap reached: {reached:?}");
    for (case, count) in reached {
        assert!(count > 0, "the cases never reach {case}");
    }
}

/// A value of a `priority` param for the domain property.
fn arb_priority_value() -> BoxedStrategy<Par> {
    prop_oneof![
        3 => (-3i64..12).prop_map(int),
        1 => "[a-c0-9]{0,2}".prop_map(|text| string(&text)),
        1 => arbitrary::<bool>().prop_map(|flag| new_gbool_par(flag, Vec::new(), false)),
        1 => (0i64..4).prop_map(|value| {
            let mut flagged = int(value);
            flagged.connective_used = true;
            flagged
        }),
    ]
    .boxed()
}

/// `pq_priority_domain`: `check_values` accepts a `priority` param of a send or of a query exactly
/// when it is a `GInt(p)` with `p ≥ 0`, no connective flag, and `p` below the bound of the space
/// when the space sets one (I-10). Every other value is `InvalidParam`.
#[test]
fn prop_pq_priority_domain() {
    check(
        pure_config(),
        (arb_priority_value(), prop_oneof![
            Just(None),
            (1i64..6).prop_map(Some)
        ]),
        |(value, bound)| {
            let mut config = SpaceConfig::new(
                SPACE.sid,
                HashMapOuter::NAME,
                PriorityQueueDiscipline::NAME,
                SpaceQualifier::Default,
            );
            if let Some(bound) = bound {
                config
                    .params
                    .insert(PRIORITIES_PARAM.to_string(), int(bound));
            }
            let level = match (value.exprs.as_slice(), value.connective_used) {
                (
                    [Expr {
                        expr_instance: Some(ExprInstance::GInt(level)),
                    }],
                    false,
                ) => Some(*level),
                _ => None,
            };
            let accepted =
                level.is_some_and(|level| level >= 0 && bound.is_none_or(|bound| level < bound));
            let expected = match accepted {
                true => Ok(()),
                false => Err(SpaceError::InvalidParam {
                    key: "priority".to_string(),
                }),
            };
            let params = BTreeMap::from([(PRIORITY_PARAM.to_string(), value.clone())]);
            prop_assert_eq!(
                PriorityQueueDiscipline.check_values(&config, &params, None),
                expected.clone()
            );
            let query = QueryParams {
                payload: Vec::new(),
                named: params,
            };
            prop_assert_eq!(
                PriorityQueueDiscipline.check_values(&config, &BTreeMap::new(), Some(&query),),
                expected
            );
            Ok(())
        },
    );
}

/// `pq_no_level_storage`: there is no storage for each level (I-10). Sends at any levels, the far
/// ones included, store exactly one record each on the one data channel of the collection, with
/// its level in its envelope, and the space holds no other channel with data than its config, its
/// counters and that data channel.
#[test]
fn prop_pq_no_level_storage() {
    let levels = vec(
        prop_oneof![
            Just(None),
            (0i64..3).prop_map(Some),
            (1_000_000i64..i64::MAX).prop_map(Some)
        ],
        1..8,
    );
    check(pure_config(), levels, |levels| {
        tokio_runtime().block_on(async {
            let Fixture { space, env } = fixture().await;
            for (index, level) in levels.iter().enumerate() {
                let value = i64::try_from(index).expect("a small index");
                let outcome = run(&pq(), &env, SpaceOp::Produce(send_at(value, *level)))
                    .await
                    .map_err(|error| TestCaseError::fail(format!("{error}")))?;
                prop_assert_eq!(outcome, SpaceOutcome::default());
            }
            let stored = records(&space).await;
            prop_assert_eq!(stored.len(), levels.len());
            for (index, (ord, level, payload, _)) in stored.iter().enumerate() {
                prop_assert_eq!(*ord, i64::try_from(index).expect("a small index"));
                prop_assert_eq!(*level, levels[index]);
                prop_assert_eq!(payload, &vec![int(*ord)]);
            }
            // Every event of the sends is on the counters or on the one data channel: no send
            // touches a channel of its own level.
            let touched: BTreeSet<Blake2b256Hash> = space
                .take_event_log()
                .await
                .iter()
                .flat_map(|event| match event {
                    Event::IoEvent(IOEvent::Produce(produce)) => vec![produce.channel_hash.clone()],
                    Event::IoEvent(IOEvent::Consume(consume)) => consume.channel_hashes.clone(),
                    Event::Comm(comm) => comm.consume.channel_hashes.clone(),
                })
                .collect();
            let allowed: BTreeSet<Blake2b256Hash> =
                [space_counter_channel(SPACE), counter_ch(), data()]
                    .iter()
                    .map(stable_hash_provider::hash)
                    .collect();
            prop_assert!(
                touched.is_subset(&allowed),
                "the sends touch only the counters and the data channel"
            );
            prop_assert!(touched.contains(&stable_hash_provider::hash(&data())));
            Ok(())
        })
    });
}

/// `datum_priority_slot_roundtrip` and `continuation_policy_slot_roundtrip` (`RecordEnvelope.v`):
/// the encoders of the two slots and their decoders are inverse, for every order, level and
/// payload. The unprioritized datum and the unrestricted continuation are the P4 records, and
/// the outcome decoder reads every fired waiter.
#[test]
fn prop_pq_record_slots_roundtrip() {
    let slot = prop_oneof![Just(None), (0i64..=i64::MAX).prop_map(Some)];
    check(
        pure_config(),
        (0i64..=i64::MAX, slot, vec(-5i64..5, 0..4), "[a-z]{1,3}"),
        |(ord, priority, payload, body)| {
            let datum = ListParWithRandom {
                pars: payload.iter().map(|value| int(*value)).collect(),
                random_state: vec![1, 2],
                cost_authority: None,
                cost_stack: None,
            };
            let wrapped = record::wrap_pq_datum(ord, priority, &datum);
            let decoded = record::unwrap_pq_datum(&wrapped)
                .map_err(|error| TestCaseError::fail(format!("{error}")))?;
            prop_assert_eq!(decoded.ord, ord);
            prop_assert_eq!(decoded.priority, priority);
            prop_assert_eq!(&decoded.datum, &datum);
            prop_assert_eq!(
                priority.is_none(),
                wrapped == record::wrap_datum(ord, &datum)
            );
            let continuation = TaggedContinuation {
                tagged_cont: Some(models::rhoapi::tagged_continuation::TaggedCont::ParBody(
                    ParWithRandom {
                        body: Some(string(&body)),
                        random_state: vec![3],
                    },
                )),
                guard: None,
                cost_authority: None,
            };
            let parked = record::wrap_pq_continuation(ord, priority, &continuation)
                .map_err(|error| TestCaseError::fail(format!("{error}")))?;
            prop_assert_eq!(record::pq_continuation_slots(&parked), Ok((ord, priority)));
            let unwrapped = record::unwrap_pq_continuation(&parked)
                .map_err(|error| TestCaseError::fail(format!("{error}")))?;
            prop_assert_eq!(&unwrapped.continuation, &continuation);
            let fired = record::into_continuation(parked.clone())
                .map_err(|error| TestCaseError::fail(format!("{error}")))?;
            prop_assert_eq!(fired.continuation, continuation.clone());
            prop_assert_eq!(
                priority.is_none(),
                Ok(parked) == record::wrap_continuation(ord, &continuation)
            );
            Ok(())
        },
    );
}
