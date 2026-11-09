//! Integration tests of the stack discipline (P5b, F2 of decision 39, P5 design v3 A.8).
//!
//! The tests run the agent of `hashmap:stack` directly over an in-memory RSpace, with the shared
//! harness of the P5 suites (`support/`). They cover these rules:
//!
//! * LIFO order and head-only matching (decision 11): a top that does not match blocks the
//!   entries below it, and so does a top whose guard refuses (decision 37).
//! * The settle (F2): an operation that exposes a matching pair below the tops fires it with a
//!   lift and a re-offer. The tests pin the events and the identities of the lift (`K_lift`) and
//!   of the re-offer.
//! * Rule E (decision 45): a peek restores the top record with no settle step, and a waiter that
//!   peeks leaves a lifted record in flight, which the settle stores again with its identity.
//! * Decision 43: a re-issued persistent operation keeps its first-round order and re-enters at
//!   its LIFO place.
//! * The round cap (I-21): a settle re-issues a settle job, and a settle before the plan
//!   re-issues the operation. Divergent pairs re-issue after each round (A10).
//! * Every operation and every settle job bumps the counter (decision 31), so two branches race
//!   on it (merge check #1). Replay reproduces play, and the order survives a restart.
//! * The observer calls of the settle: each staged operation makes the observer calls of its host
//!   call (decision 90, decision 69). They replace the legacy charges of Option P (decision 45),
//!   which DR-134 deleted with the legacy meter.
//!
//! Each property below states an invariant of `StackDiscipline.v` or `StackDiscipline.tla`:
//!
//! | Property | Formal statement |
//! |---|---|
//! | `prop_stack_head_is_max_ord` | `stack_head_is_max_ord` |
//! | `prop_stack_lifo_order_any_schedule` | `LifoOrder`, `stack_lifo`, `stack_head_blocks`, `NoRecordLost`, `stack_settle_keeps_unspent_records` |
//! | `prop_stack_no_pending_match_any_schedule` | `StackNoPendingMatch`, `stack_produce_preserves_inv`, `stack_consume_preserves_inv`, `stack_peek_restore_needs_no_settle` |
//! | `prop_stack_settle_bounded_any_schedule` | `stack_settle_terminates` |
//! | `prop_stack_plan_permutation_invariant` | `stack_plan_permutation_invariant` |
//! | `prop_stack_reissue_keeps_ord_any_schedule` | `ReissueKeepsOrd`, `stack_reissue_keeps_ord`, `stack_reentry_at_lifo_place` |

#[path = "support/mod.rs"]
mod support;

use std::collections::{BTreeSet, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::cost_signature::Value as CostSignatureValue;
// Changed by decision 90 (decision 69): only the legacy host of the retired Option P test named
// `TaggedContinuation` (DR-134 deleted the legacy meter).
// use models::rhoapi::{
//     BindPattern, CostAuthority, CostRegion, CostSignature, ListParWithRandom, Par,
//     TaggedContinuation,
// };
use models::rhoapi::{
    BindPattern, CostAuthority, CostRegion, CostSignature, ListParWithRandom, Par,
};
use models::rust::space_factory_urn::{SpaceFactoryUrn, SpaceInner, SpaceOuter};
use models::rust::utils::new_gbool_par;
use proptest::arbitrary::any as arbitrary;
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;
// Changed by decision 124 (plan v2 C7.2, decision 99): the expected counter rows take the
// machinery seal. The text before:
// use rholang::rust::interpreter::accounting::authority::AuthorityByteEventKind;
use rholang::rust::interpreter::accounting::authority::{
    system_residue_authority_with_caller, AuthorityByteEventKind,
};
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
    ConsumeOp, ProduceOp, SettleOp, SpaceCreation, SpaceEnv, SpaceOp, SpaceOutcome, SpaceReissue,
};
use rholang::rust::interpreter::spaces::counter::{counter_continuation, counter_pattern};
use rholang::rust::interpreter::spaces::disciplines::stack::StackDiscipline;
// Retired by decision 90 (C6, step D): the retired replay tests of this file were its only users
// (decision 125). The text before:
// use rholang::rust::interpreter::spaces::errors::SpaceError;
use rholang::rust::interpreter::spaces::generic::{
    Discipline, GenericSpace, Ordered, OuterStore, ParWaiter, Side,
};
use rholang::rust::interpreter::spaces::namespace::{
    Durability, NativeSpace, RandomPurpose, SID_LEN,
};
use rholang::rust::interpreter::spaces::outer::hashmap::HashMapOuter;
use rholang::rust::interpreter::spaces::record;
use rholang::rust::interpreter::spaces::settle::{lift_continuation, lift_pattern};
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
//     any, body_name, budget, config_of, counter, counter_channel, create, creation, data_channel,
//     env_over, exactly, fired, int, key_of, named, play, play_and_replay, receive_on, records_on,
//     run, send_on, space_counter_channel, string, ParRSpace,
// };
// ```
use support::agent::{
    any, body_name, budget, config_of, counter, counter_channel, create, creation, data_channel,
    env_over, exactly, fired, int, key_of, named, play, receive_on, records_on, run, send_on,
    space_counter_channel, string, ParRSpace, ParSpaceProduce,
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

/// The agent of `hashmap:stack`.
type Stack = GenericSpace<HashMapOuter, StackDiscipline>;

/// The space of every test.
const SPACE: NativeSpace = NativeSpace {
    sid: [0x5A; SID_LEN],
    durability: Durability::Durable,
};

fn stack() -> Stack { Stack::default() }

/// The inner channel of the collection of most tests.
fn channel() -> Par { string("stack") }

/// The data channel `D(sid,k)` of the collection of `channel()`.
fn data() -> Par { data_channel(SPACE, &channel()) }

/// The counter channel `S(sid,k)` of the collection of `channel()`.
fn counter_ch() -> Par { counter_channel(SPACE, &channel()) }

/// The creation of the stack space by the factory of `hashmap:stack`.
fn stack_creation() -> SpaceCreation {
    creation(
        SPACE,
        config_of(
            SPACE,
            HashMapOuter::NAME,
            StackDiscipline::NAME,
            SpaceQualifier::Default,
        ),
        SpaceFactoryUrn {
            outer: SpaceOuter::HashMap,
            inner: SpaceInner::Stack,
        },
    )
}

struct Fixture {
    space: ParRSpace,
    env: SpaceEnv,
}

/// A play space that holds the stack space, with an empty event log.
async fn fixture() -> Fixture {
    let space = play().await;
    let env = env_over(space.clone(), &budget());
    create(&stack(), &env, stack_creation()).await;
    space.take_event_log().await;
    Fixture { space, env }
}

fn send(value: i64) -> ProduceOp { send_on(SPACE, &channel(), value, false) }

fn send_forever(value: i64) -> ProduceOp { send_on(SPACE, &channel(), value, true) }

fn receive(name: &str, pattern: BindPattern) -> ConsumeOp {
    receive_on(SPACE, &channel(), name, pattern)
}

fn receive_forever(name: &str, pattern: BindPattern) -> ConsumeOp {
    ConsumeOp {
        persistent: true,
        ..receive(name, pattern)
    }
}

fn peek(name: &str) -> ConsumeOp {
    ConsumeOp {
        peek: true,
        ..receive(name, any())
    }
}

/// Runs `op` with the stack agent and returns its outcome. A failure fails the test.
async fn step(env: &SpaceEnv, op: SpaceOp) -> SpaceOutcome {
    run(&stack(), env, op).await.expect("the operation commits")
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

/// The records of the collection of `channel()` as `(ord, payload, persistent)`, by `ord`.
async fn records(space: &ParRSpace) -> Vec<(i64, Vec<Par>, bool)> {
    records_on(space, &data()).await
}

/// The waiters on the data channel `data` as `(ord, body name, persistent, peek)`, by `ord`.
async fn waiters_on(space: &ParRSpace, data: &Par) -> Vec<(i64, String, bool, bool)> {
    let mut waiters: Vec<_> = space
        .get_waiting_continuations(vec![data.clone()])
        .await
        .into_iter()
        .map(|waiter| {
            let parked =
                record::unwrap_continuation(&waiter.continuation).expect("a continuation record");
            (
                parked.ord,
                body_name(&parked.continuation),
                waiter.persist,
                !waiter.peeks.is_empty(),
            )
        })
        .collect();
    waiters.sort_by_key(|(ord, ..)| *ord);
    waiters
}

async fn waiters(space: &ParRSpace) -> Vec<(i64, String, bool, bool)> {
    waiters_on(space, &data()).await
}

fn waiter(ord: i64, name: &str, persistent: bool, peek: bool) -> (i64, String, bool, bool) {
    (ord, name.to_string(), persistent, peek)
}

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
/// `step`, the operation random state `random_state` and the cost authority `authority`.
fn lift_consume(
    counter: i64,
    step: i64,
    random_state: &[u8],
    authority: Option<CostAuthority>,
) -> Consume {
    Consume::create(
        &vec![data()],
        &vec![lift_pattern()],
        &lift_continuation(counter, step, &lift_random(random_state), authority),
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

// ─── LIFO order and head-only matching ───────────────────────────────────────────────────────

#[tokio::test]
async fn space_stack_lifo_delivery() {
    let Fixture { space, env } = fixture().await;
    for value in 1..=3 {
        assert_eq!(produce(&env, send(value)).await, SpaceOutcome::default());
    }
    assert_eq!(records(&space).await, vec![
        (0, vec![int(1)], false),
        (1, vec![int(2)], false),
        (2, vec![int(3)], false)
    ]);
    for (name, value) in [("a", 3), ("b", 2), ("c", 1)] {
        assert_eq!(
            fired(&consume(&env, receive(name, any())).await),
            vec![delivery(name, value)],
            "the receive {name} takes the top record"
        );
    }
    assert!(records(&space).await.is_empty());
    for name in ["w1", "w2", "w3"] {
        assert_eq!(
            consume(&env, receive(name, any())).await,
            SpaceOutcome::default()
        );
    }
    assert_eq!(waiters(&space).await, vec![
        waiter(6, "w1", false, false),
        waiter(7, "w2", false, false),
        waiter(8, "w3", false, false)
    ]);
    for (value, name) in [(9, "w3"), (8, "w2"), (7, "w1")] {
        assert_eq!(
            fired(&produce(&env, send(value)).await),
            vec![delivery(name, value)],
            "the send {value} fires the top waiter"
        );
    }
    assert!(waiters(&space).await.is_empty());
    assert!(records(&space).await.is_empty());
}

#[tokio::test]
async fn space_stack_data_head_blocks() {
    let Fixture { space, env } = fixture().await;
    produce(&env, send(1)).await;
    produce(&env, send(2)).await;
    // The top record 2 blocks the receive for 1, although the record below it matches.
    assert_eq!(
        consume(&env, receive("one", exactly(1))).await,
        SpaceOutcome::default()
    );
    assert_eq!(records(&space).await, vec![
        (0, vec![int(1)], false),
        (1, vec![int(2)], false)
    ]);
    assert_eq!(waiters(&space).await, vec![waiter(2, "one", false, false)]);
}

#[tokio::test]
async fn space_stack_waiter_head_blocks() {
    let Fixture { space, env } = fixture().await;
    consume(&env, receive("one", exactly(1))).await;
    consume(&env, receive("two", exactly(2))).await;
    // The top waiter for 2 blocks the send of 1, although the waiter below it matches.
    assert_eq!(produce(&env, send(1)).await, SpaceOutcome::default());
    assert_eq!(records(&space).await, vec![(2, vec![int(1)], false)]);
    assert_eq!(waiters(&space).await, vec![
        waiter(0, "one", false, false),
        waiter(1, "two", false, false)
    ]);
}

#[tokio::test]
async fn space_stack_guard_of_the_top_waiter_blocks() {
    let Fixture { space, env } = fixture().await;
    consume(&env, receive("v", any())).await;
    let refusing = ConsumeOp {
        guard: Some(new_gbool_par(false, Vec::new(), false)),
        ..receive("w", any())
    };
    consume(&env, refusing).await;
    // The guard of the top waiter refuses (decision 37), so the send is stored, and the settle
    // after the plan does not fire the refused pair either.
    assert_eq!(produce(&env, send(1)).await, SpaceOutcome::default());
    assert_eq!(records(&space).await, vec![(2, vec![int(1)], false)]);
    assert_eq!(waiters(&space).await, vec![
        waiter(0, "v", false, false),
        waiter(1, "w", false, false)
    ]);

    let Fixture { space, env } = fixture().await;
    let admitting = ConsumeOp {
        guard: Some(new_gbool_par(true, Vec::new(), false)),
        ..receive("w", any())
    };
    consume(&env, admitting).await;
    assert_eq!(fired(&produce(&env, send(1)).await), vec![delivery("w", 1)]);
    assert!(records(&space).await.is_empty());
    assert!(waiters(&space).await.is_empty());
}

#[tokio::test]
async fn space_stack_persistent_drain_lifo() {
    let Fixture { space, env } = fixture().await;
    for value in 1..=3 {
        produce(&env, send(value)).await;
    }
    space.take_event_log().await;
    let outcome = consume(&env, receive_forever("r", any())).await;
    assert_eq!(fired(&outcome), vec![
        delivery("r", 3),
        delivery("r", 2),
        delivery("r", 1)
    ]);
    assert_eq!(outcome.reissue, None);
    assert_eq!(
        labels(&space.take_event_log().await),
        bumped(&[
            "consume D",
            "comm D",
            "consume D",
            "comm D",
            "consume D",
            "comm D",
            "consume D"
        ])
    );
    assert!(records(&space).await.is_empty());
    assert_eq!(waiters(&space).await, vec![waiter(3, "r", true, false)]);
    assert_eq!(fired(&produce(&env, send(4)).await), vec![delivery("r", 4)]);
    assert!(records(&space).await.is_empty());
    assert_eq!(waiters(&space).await, vec![waiter(3, "r", true, false)]);
}

#[tokio::test]
async fn space_stack_persistent_send_fires_each_top_waiter_then_stores() {
    let Fixture { space, env } = fixture().await;
    consume(&env, receive("w1", any())).await;
    consume(&env, receive("w2", any())).await;
    space.take_event_log().await;
    let outcome = produce(&env, send_forever(5)).await;
    assert_eq!(fired(&outcome), vec![delivery("w2", 5), delivery("w1", 5)]);
    assert_eq!(outcome.reissue, None);
    assert_eq!(
        labels(&space.take_event_log().await),
        bumped(&["produce D", "comm D", "produce D", "comm D", "produce D"])
    );
    assert_eq!(records(&space).await, vec![(2, vec![int(5)], true)]);
    assert!(waiters(&space).await.is_empty());
}

// ─── The settle cascade (F2) ─────────────────────────────────────────────────────────────────

/// The counterexample of v2 section 3.1: the waiters w2 and w, then the record h under the top
/// waiter w. The send of 2 then fires w, and the tops h and w2 match.
async fn counterexample(env: &SpaceEnv, w2: BindPattern) {
    consume(env, receive("w2", w2)).await;
    consume(env, receive("w", exactly(2))).await;
    assert_eq!(
        produce(env, send(1)).await,
        SpaceOutcome::default(),
        "the top waiter blocks h"
    );
}

#[tokio::test]
async fn space_stack_push_exposes_a_buried_waiter_and_settles() {
    let Fixture { space, env } = fixture().await;
    counterexample(&env, exactly(1)).await;
    assert_eq!(records(&space).await, vec![(2, vec![int(1)], false)]);
    space.take_event_log().await;
    let outcome = produce(&env, send(2)).await;
    // The plan fires w. Without a settle the tops h and w2 would then match (INV fails). The
    // settle after the plan lifts h and offers it again, and w2 fires.
    assert_eq!(fired(&outcome), vec![literal("w"), literal("w2")]);
    assert_eq!(outcome.reissue, None);
    assert_eq!(
        labels(&space.take_event_log().await),
        bumped(&[
            "produce D",
            "comm D",
            "consume D",
            "comm D",
            "produce D",
            "comm D"
        ])
    );
    assert!(records(&space).await.is_empty());
    assert!(waiters(&space).await.is_empty());
    assert_eq!(counter_value(&space).await, vec![4]);
}

#[tokio::test]
async fn space_stack_pop_exposes_a_buried_record_and_settles() {
    let Fixture { space, env } = fixture().await;
    produce(&env, send(1)).await;
    consume(&env, receive("x", exactly(9))).await;
    produce(&env, send(3)).await;
    consume(&env, receive("w", exactly(1))).await;
    assert_eq!(records(&space).await, vec![
        (0, vec![int(1)], false),
        (2, vec![int(3)], false)
    ]);
    assert_eq!(waiters(&space).await, vec![
        waiter(1, "x", false, false),
        waiter(3, "w", false, false)
    ]);
    space.take_event_log().await;
    // The receive takes the top record 3. The tops are then the record 1 and the waiter w, which
    // match, so the settle fires them.
    let outcome = consume(&env, receive("y", exactly(3))).await;
    assert_eq!(fired(&outcome), vec![literal("y"), literal("w")]);
    assert_eq!(
        labels(&space.take_event_log().await),
        bumped(&[
            "consume D",
            "comm D",
            "consume D",
            "comm D",
            "produce D",
            "comm D"
        ])
    );
    assert!(records(&space).await.is_empty());
    assert_eq!(waiters(&space).await, vec![waiter(1, "x", false, false)]);
}

#[tokio::test]
async fn space_stack_settle_lift_events_shape() {
    let space = play().await;
    let budget = budget();
    let _scope = budget.enter_comm_accounting_scope();
    let env = env_over(space.clone(), &budget);
    create(&stack(), &env, stack_creation()).await;
    consume(&env, receive("w2", any())).await;
    let parked_w2 = data_consumes(&space.take_event_log().await)
        .pop()
        .expect("the park of w2");
    consume(&env, receive("w", exactly(2))).await;
    // h carries the cost authority of its sender.
    let h = ProduceOp {
        authority: region(2),
        data: ListParWithRandom {
            cost_authority: Some(region(2)),
            ..send(1).data
        },
        ..send(1)
    };
    space.take_event_log().await;
    produce(&env, h.clone()).await;
    let stored_h = data_produces(&space.take_event_log().await)
        .pop()
        .expect("the store of h");
    let d = send(2);
    let outcome = produce(&env, d.clone()).await;
    assert_eq!(fired(&outcome), vec![literal("w"), delivery("w2", 1)]);
    let log = space.take_event_log().await;
    assert_eq!(log.len(), 9);
    // The lift: K_lift of the round of d (counter value 3), with the random state of d and its
    // cost authority. Its step is the fire count of the transaction: the plan fired w once.
    // Disabled by I-21 (one fire count for each transaction): the settle after the plan had the first step FIRST_STEP_AFTER.
    // let lift = lift_consume(3, FIRST_STEP_AFTER, &d.data.random_state, None);
    let lift = lift_consume(3, 1, &d.data.random_state, None);
    assert_eq!(log[5], Event::IoEvent(IOEvent::Consume(lift.clone())));
    match &log[6] {
        Event::Comm(comm) => {
            assert_eq!(comm.consume, lift);
            assert_eq!(comm.produces, vec![stored_h.clone()]);
            assert!(comm.peeks.is_empty());
        }
        other => panic!("the lift is a COMM: {other:?}"),
    }
    // The re-offer has the identity of the stored record h, and it fires w2 with h.
    assert_eq!(log[7], Event::IoEvent(IOEvent::Produce(stored_h.clone())));
    match &log[8] {
        Event::Comm(comm) => {
            assert_eq!(comm.consume, parked_w2);
            assert_eq!(comm.produces, vec![stored_h.clone()]);
        }
        other => panic!("the re-offer fires w2: {other:?}"),
    }
    // The re-offer registers the record's own authority, and the lift registers the authority
    // of the operation (the empty authority of d).
    let registered = budget.introduction_authority(
        produce_introduction_identity(&stored_h),
        AuthorityByteEventKind::ProduceIntroduction,
    );
    assert_eq!(registered.ok(), Some(region(2)));
}

#[tokio::test]
async fn space_stack_settle_job_on_a_settled_stack_only_bumps() {
    let Fixture { space, env } = fixture().await;
    counterexample(&env, any()).await;
    space.take_event_log().await;
    // A settle job on a settled stack only bumps the counter.
    let settle_op = settle_after_send(&send(7));
    let outcome = step(&env, SpaceOp::Settle(settle_op.clone())).await;
    assert_eq!(outcome, SpaceOutcome::default());
    assert_eq!(labels(&space.take_event_log().await), bumped(&[]));
    assert_eq!(counter_value(&space).await, vec![4]);
    assert_eq!(records(&space).await, vec![(2, vec![int(1)], false)]);
}

// ─── Rule E (decision 45) ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn space_stack_peek_restores_top() {
    let Fixture { space, env } = fixture().await;
    consume(&env, receive("w", exactly(9))).await;
    produce(&env, send(5)).await;
    let stored = data_produces(&space.take_event_log().await);
    assert_eq!(stored.len(), 1);
    let peeked = consume(&env, peek("p")).await;
    assert_eq!(fired(&peeked), vec![delivery("p", 5)]);
    assert!(!peeked.fires[0].results[0].persistent);
    assert_eq!(peeked.reissue, None);
    let log = space.take_event_log().await;
    // The peek fire removes the top record and the re-store produces it again at its order.
    // The tops are then the tops before the peek, so no settle step (no lift) follows.
    assert_eq!(
        labels(&log),
        bumped(&["consume D", "comm D peek", "produce D"])
    );
    assert_eq!(
        data_produces(&log),
        stored,
        "the re-store keeps the identity"
    );
    assert_eq!(records(&space).await, vec![(1, vec![int(5)], false)]);
    assert_eq!(waiters(&space).await, vec![waiter(0, "w", false, false)]);
    assert_eq!(fired(&consume(&env, receive("t", any())).await), vec![
        delivery("t", 5)
    ]);
    assert!(records(&space).await.is_empty());

    // A persistent top record stays in place, so nothing is stored again.
    let Fixture { space, env } = fixture().await;
    produce(&env, send_forever(5)).await;
    space.take_event_log().await;
    let peeked = consume(&env, peek("p")).await;
    assert!(peeked.fires[0].results[0].persistent);
    assert_eq!(
        labels(&space.take_event_log().await),
        bumped(&["consume D", "comm D peek"])
    );
    assert_eq!(records(&space).await, vec![(0, vec![int(5)], true)]);
}

#[tokio::test]
async fn space_stack_peek_waiter_leaves_the_lifted_record_in_flight() {
    let Fixture { space, env } = fixture().await;
    consume(&env, peek("p")).await;
    consume(&env, receive("b", exactly(9))).await;
    produce(&env, send(1)).await;
    let stored = data_produces(&space.take_event_log().await);
    assert_eq!(stored.len(), 1);
    // The send of 9 fires b. The tops are then 1 and the peek waiter p: the settle lifts 1, and
    // its re-offer fires p, which does not spend it (decision 30). No waiter is left, so the
    // settle stores 1 again at its order with its identity (Rule E).
    let outcome = produce(&env, send(9)).await;
    assert_eq!(fired(&outcome), vec![literal("b"), delivery("p", 1)]);
    let log = space.take_event_log().await;
    assert_eq!(
        labels(&log),
        bumped(&[
            "produce D",
            "comm D",
            "consume D",
            "comm D",
            "produce D",
            "comm D peek",
            "produce D"
        ])
    );
    let produces = data_produces(&log);
    assert_eq!(produces.len(), 3);
    assert_eq!(
        produces[1], stored[0],
        "the re-offer has the identity of the record"
    );
    assert_eq!(
        produces[2], stored[0],
        "the landing has the identity of the record"
    );
    assert_eq!(records(&space).await, vec![(2, vec![int(1)], false)]);
    assert!(waiters(&space).await.is_empty());

    // With a waiter below the peek waiter, the record in flight fires it and is spent.
    let Fixture { space, env } = fixture().await;
    consume(&env, receive("w", any())).await;
    consume(&env, peek("p")).await;
    consume(&env, receive("b", exactly(9))).await;
    produce(&env, send(1)).await;
    space.take_event_log().await;
    let outcome = produce(&env, send(9)).await;
    assert_eq!(fired(&outcome), vec![
        literal("b"),
        delivery("p", 1),
        delivery("w", 1)
    ]);
    assert_eq!(
        labels(&space.take_event_log().await),
        bumped(&[
            "produce D",
            "comm D",
            "consume D",
            "comm D",
            "produce D",
            "comm D peek",
            "produce D",
            "comm D"
        ])
    );
    assert!(records(&space).await.is_empty());
    assert!(waiters(&space).await.is_empty());
}

// ─── Decision 43 ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn space_stack_reissue_reenters_at_its_lifo_place() {
    let Fixture { space, env } = fixture().await;
    consume(&env, receive("one", exactly(1))).await;
    for index in 0..FIRE_ROUND_CAP {
        consume(&env, receive(&format!("w{index}"), any())).await;
    }
    let first = cap() + 1;
    space.take_event_log().await;
    let outcome = produce(&env, send_forever(1)).await;
    assert_eq!(outcome.fires.len(), FIRE_ROUND_CAP);
    let reissue = outcome.reissue.expect("the cap re-issues the send");
    assert_eq!(reissue, SpaceReissue {
        op: SpaceOp::Produce(ProduceOp {
            reissued_ord: Some(first),
            ..send_forever(1)
        }),
        cause: ReissueCause::RoundCap,
    });
    let round_one = data_produces(&space.take_event_log().await);
    assert_eq!(round_one.len(), FIRE_ROUND_CAP);
    // A younger record arrives between the rounds. The top waiter takes only 1, so it is stored
    // on top while the send is in flight.
    assert_eq!(produce(&env, send(7)).await, SpaceOutcome::default());
    space.take_event_log().await;
    // The send re-enters with its first-round order. The younger record is on top, so the send
    // is stored at its LIFO place below it and fires nothing, although the top waiter takes 1.
    let rest = step(&env, reissue.op).await;
    assert_eq!(rest, SpaceOutcome::default());
    let round_two = data_produces(&space.take_event_log().await);
    assert_eq!(round_two.len(), 1, "the store");
    assert!(
        round_one
            .iter()
            .chain(&round_two)
            .all(|produce| *produce == round_one[0]),
        "every round stages the same record, so the same identity"
    );
    assert_eq!(records(&space).await, vec![
        (first, vec![int(1)], true),
        (first + 1, vec![int(7)], false)
    ]);
    assert_eq!(waiters(&space).await, vec![waiter(0, "one", false, false)]);
    assert_eq!(
        counter_value(&space).await,
        vec![first + 3],
        "each round bumps the counter"
    );
}

#[tokio::test]
async fn space_stack_reissue_reenters_at_the_top_of_an_empty_side() {
    let Fixture { space, env } = fixture().await;
    consume(&env, receive("last", any())).await;
    for index in 0..FIRE_ROUND_CAP {
        consume(&env, receive(&format!("w{index}"), any())).await;
    }
    let first = cap() + 1;
    let reissue = produce(&env, send_forever(3))
        .await
        .reissue
        .expect("the cap re-issues the send");
    // No record arrives between the rounds, so the send re-enters at the top and fires the
    // last waiter, then it is stored at its first-round order.
    let rest = step(&env, reissue.op).await;
    assert_eq!(fired(&rest), vec![delivery("last", 3)]);
    assert_eq!(rest.reissue, None);
    assert_eq!(records(&space).await, vec![(first, vec![int(3)], true)]);
    assert!(waiters(&space).await.is_empty());
}

// ─── The round cap (I-21) and divergent pairs (A10) ──────────────────────────────────────────

/// A persistent waiter w for every value, a blocker b on top of it, the records `0..count` under
/// b, and the send of 999, which fires b and exposes the pairs of each record with w. It takes
/// the event log before the send of 999, and it returns the outcome of that send.
async fn exposed_pairs(space: &ParRSpace, env: &SpaceEnv, count: i64) -> SpaceOutcome {
    consume(env, receive_forever("w", any())).await;
    consume(env, receive("b", exactly(999))).await;
    for value in 0..count {
        assert_eq!(produce(env, send(value)).await, SpaceOutcome::default());
    }
    space.take_event_log().await;
    produce(env, send(999)).await
}

#[tokio::test]
async fn space_stack_round_cap_reissues_a_settle() {
    let Fixture { space, env } = fixture().await;
    // The plan of the send of 999 fires b, and the settle after it fires the next
    // FIRE_ROUND_CAP - 1 pairs, so the transaction makes FIRE_ROUND_CAP fires (I-21).
    // Disabled by I-21 (one fire count for each transaction): the settle after the plan fired FIRE_ROUND_CAP pairs of its own.
    // let outcome = exposed_pairs(&space, &env, cap() + 1).await;
    // let mut expected = vec![literal("b")];
    // expected.extend((1..=cap()).rev().map(|value| delivery("w", value)));
    let outcome = exposed_pairs(&space, &env, cap()).await;
    assert_eq!(outcome.fires.len(), FIRE_ROUND_CAP);
    let mut expected = vec![literal("b")];
    expected.extend((1..cap()).rev().map(|value| delivery("w", value)));
    assert_eq!(fired(&outcome), expected);
    assert_eq!(
        outcome.reissue,
        Some(SpaceReissue {
            op: SpaceOp::Settle(settle_after_send(&send(999))),
            cause: ReissueCause::RoundCap,
        })
    );
    assert_eq!(records(&space).await, vec![(2, vec![int(0)], false)]);
    assert_eq!(waiters(&space).await, vec![waiter(0, "w", true, false)]);
    // Disabled by I-21 (one fire count for each transaction): the setup sent cap() + 1 records.
    // assert_eq!(counter_value(&space).await, vec![cap() + 4]);
    assert_eq!(counter_value(&space).await, vec![cap() + 3]);
    space.take_event_log().await;
    let rest = step(&env, outcome.reissue.expect("a settle job").op).await;
    assert_eq!(fired(&rest), vec![delivery("w", 0)]);
    assert_eq!(rest.reissue, None);
    assert_eq!(
        labels(&space.take_event_log().await),
        bumped(&["consume D", "comm D", "produce D", "comm D"])
    );
    assert!(records(&space).await.is_empty());
    // Disabled by I-21 (one fire count for each transaction): the setup sent cap() + 1 records.
    // assert_eq!(counter_value(&space).await, vec![cap() + 5]);
    assert_eq!(counter_value(&space).await, vec![cap() + 4]);
}

#[tokio::test]
async fn space_stack_settle_before_reissues_the_operation_at_the_cap() {
    let Fixture { space, env } = fixture().await;
    // Disabled by I-21 (one fire count for each transaction): the send of 999 made 1 + FIRE_ROUND_CAP fires.
    // let outcome = exposed_pairs(&space, &env, 2 * cap() + 1).await;
    // assert_eq!(outcome.fires.len(), FIRE_ROUND_CAP + 1);
    let outcome = exposed_pairs(&space, &env, 2 * cap()).await;
    assert_eq!(outcome.fires.len(), FIRE_ROUND_CAP);
    let settle_job = outcome
        .reissue
        .expect("the settle after the plan re-issues");
    assert_eq!(settle_job.cause, ReissueCause::RoundCap);
    // A persistent receive runs before the settle job. Its settle before the plan fires the
    // next FIRE_ROUND_CAP pairs and reaches the cap, so the receive re-issues itself with the
    // order of its first round (decision 43), before its element is parked.
    // Disabled by I-21 (one fire count for each transaction): the setup sent 2 * cap() + 1 records.
    // let first = 2 * cap() + 4;
    let first = 2 * cap() + 3;
    let receiving = receive_forever("y", exactly(-1));
    let outcome = consume(&env, receiving.clone()).await;
    let expected: Vec<_> = (1..=cap())
        .rev()
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
    assert_eq!(waiters(&space).await, vec![waiter(0, "w", true, false)]);
    assert_eq!(records(&space).await, vec![(2, vec![int(0)], false)]);
    let rest = step(&env, settle_job.op).await;
    assert_eq!(fired(&rest), vec![delivery("w", 0)]);
    assert_eq!(rest.reissue, None);
    let parked = step(&env, outcome.reissue.expect("the re-issue").op).await;
    assert_eq!(parked, SpaceOutcome::default());
    assert_eq!(waiters(&space).await, vec![
        waiter(0, "w", true, false),
        waiter(first, "y", true, false)
    ]);
    assert!(records(&space).await.is_empty());
}

/// I-21: one transaction makes at most `FIRE_ROUND_CAP` fires, its settles included. A settle
/// before the plan that ends quiescent exactly at the cap leaves no fire for the plan, so the
/// operation re-issues before it stages anything (`open_plan`).
#[tokio::test]
async fn space_stack_plan_after_a_full_settle_reissues_the_operation() {
    let Fixture { space, env } = fixture().await;
    let outcome = exposed_pairs(&space, &env, 2 * cap() - 1).await;
    assert_eq!(outcome.fires.len(), FIRE_ROUND_CAP);
    let settle_job = outcome
        .reissue
        .expect("the settle after the plan re-issues");
    // FIRE_ROUND_CAP pairs are pending. The settle before the plan of the receive fires all of
    // them and ends quiescent at the cap, so the receive re-issues as the program issued it.
    let receiving = receive("y", exactly(-1));
    let outcome = consume(&env, receiving.clone()).await;
    let expected: Vec<_> = (0..cap()).rev().map(|value| delivery("w", value)).collect();
    assert_eq!(fired(&outcome), expected);
    assert_eq!(
        outcome.reissue,
        Some(SpaceReissue {
            op: SpaceOp::Consume(ConsumeOp {
                reissued_ord: None,
                ..receiving
            }),
            cause: ReissueCause::RoundCap,
        })
    );
    assert!(records(&space).await.is_empty());
    assert_eq!(waiters(&space).await, vec![waiter(0, "w", true, false)]);
    // The settle job finds no pair, and the re-issued receive parks in its own round.
    assert_eq!(step(&env, settle_job.op).await, SpaceOutcome::default());
    let parked = step(&env, outcome.reissue.expect("the re-issue").op).await;
    assert_eq!(parked, SpaceOutcome::default());
    assert_eq!(waiters(&space).await, vec![
        waiter(0, "w", true, false),
        waiter(2 * cap() + 4, "y", false, false)
    ]);
}

#[tokio::test]
async fn space_stack_divergent_pairs_reissue() {
    // A persistent send in flight and a persistent waiter: the send re-issues itself.
    let Fixture { space, env } = fixture().await;
    consume(&env, receive_forever("w", any())).await;
    let outcome = produce(&env, send_forever(9)).await;
    assert_eq!(fired(&outcome), vec![delivery("w", 9)]);
    let expected = SpaceReissue {
        op: SpaceOp::Produce(ProduceOp {
            reissued_ord: Some(1),
            ..send_forever(9)
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
    consume(&env, receive_forever("w", any())).await;
    consume(&env, receive("b", exactly(999))).await;
    produce(&env, send_forever(1)).await;
    let stored = data_produces(&space.take_event_log().await);
    let outcome = produce(&env, send(999)).await;
    assert_eq!(fired(&outcome), vec![literal("b"), delivery("w", 1)]);
    let expected = SpaceReissue {
        op: SpaceOp::Settle(settle_after_send(&send(999))),
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
    assert_eq!(records(&space).await, vec![(2, vec![int(1)], true)]);
    assert_eq!(waiters(&space).await, vec![waiter(0, "w", true, false)]);
}

#[tokio::test]
async fn space_stack_settle_lift_identities_unique() {
    let Fixture { space, env } = fixture().await;
    // Disabled by I-21 (one fire count for each transaction): the setup sent cap() + 1 records.
    // let outcome = exposed_pairs(&space, &env, cap() + 1).await;
    let outcome = exposed_pairs(&space, &env, cap()).await;
    let push_log = space.take_event_log().await;
    let rest = step(&env, outcome.reissue.expect("a settle job").op).await;
    assert_eq!(rest.reissue, None);
    let job_log = space.take_event_log().await;
    let random_state = send(999).data.random_state;
    // The settle after the plan of the send of 999 lifts in the round with the counter value
    // cap + 2. Each lift takes the fire count of the transaction as its step: the plan fired b,
    // so the steps run from 1 to cap - 1. The settle job lifts in the next round, a transaction
    // of its own, from the step 0 on.
    // Disabled by I-21 (one fire count for each transaction): the settle after the plan lifted from FIRST_STEP_AFTER on, and the settle job from
    // FIRST_STEP_BEFORE on.
    // let mut expected: Vec<Consume> = (0..cap())
    //     .map(|index| lift_consume(cap() + 3, FIRST_STEP_AFTER + index, &random_state, None))
    //     .collect();
    // expected.push(lift_consume(cap() + 4, FIRST_STEP_BEFORE, &random_state, None));
    let mut expected: Vec<Consume> = (1..cap())
        .map(|step| lift_consume(cap() + 2, step, &random_state, None))
        .collect();
    expected.push(lift_consume(cap() + 3, 0, &random_state, None));
    let lifts: Vec<Consume> = data_consumes(&push_log)
        .into_iter()
        .chain(data_consumes(&job_log))
        .collect();
    assert_eq!(lifts, expected);
    let identities: BTreeSet<Vec<u8>> = lifts.iter().map(|lift| lift.hash.bytes()).collect();
    // Disabled by I-21 (one fire count for each transaction): the setup sent cap() + 1 records.
    // assert_eq!(identities.len(), FIRE_ROUND_CAP + 1, "every lift has its own identity");
    assert_eq!(
        identities.len(),
        FIRE_ROUND_CAP,
        "every lift has its own identity"
    );
}

/// Both settles of one round lift. A capped settle leaves the pair (100, w0) pending. The receive
/// of 7 then settles it before its plan, takes the top record 7, and exposes the pair (5, a),
/// which it settles after its plan. The two lifts share the counter value and the random state
/// of the round, so only their steps keep their identities apart. The steps are the fire counts
/// of the transaction (I-21), so the fire of the receive between the two lifts separates them.
#[tokio::test]
async fn space_stack_two_settles_of_one_round_lift_with_distinct_steps() {
    let Fixture { space, env } = fixture().await;
    produce(&env, send(5)).await;
    produce(&env, send(7)).await;
    consume(&env, receive("a", exactly(5))).await;
    // Disabled by I-21 (one fire count for each transaction): the plan of the capped send and its settle made 1 + FIRE_ROUND_CAP fires, so the
    // setup needed cap() + 1 pairs to leave one pending.
    // for index in 0..=cap() {
    for index in 0..cap() {
        consume(&env, receive(&format!("w{index}"), exactly(100 + index))).await;
    }
    consume(&env, receive("b", exactly(999))).await;
    // for index in 0..=cap() {
    for index in 0..cap() {
        assert_eq!(
            produce(&env, send(100 + index)).await,
            SpaceOutcome::default()
        );
    }
    let capped = produce(&env, send(999)).await;
    // assert_eq!(capped.fires.len(), FIRE_ROUND_CAP + 1);
    assert_eq!(capped.fires.len(), FIRE_ROUND_CAP);
    assert_eq!(
        capped.reissue.as_ref().map(|reissue| reissue.cause),
        Some(ReissueCause::RoundCap)
    );
    space.take_event_log().await;
    let receiving = receive("y", exactly(7));
    let outcome = consume(&env, receiving.clone()).await;
    assert_eq!(fired(&outcome), vec![
        literal("w0"),
        literal("y"),
        literal("a")
    ]);
    assert_eq!(outcome.reissue, None);
    // The round of the receive has the counter value 2 * cap + 5, because the setup ran
    // 2 * cap + 5 operations. Its consumes on the data channel are the lift of the settle before
    // the plan (step 0), the fire of the receive, and the lift of the settle after the plan
    // (step 2, after the two fires of the transaction).
    // Disabled by I-21 (one fire count for each transaction): the setup ran 2 * cap + 7 operations.
    // let counter = 2 * cap() + 7;
    let counter = 2 * cap() + 5;
    let random_state = &receiving.body.random_state;
    let fired_receive = Consume::create(
        &vec![data()],
        &vec![exactly(7)],
        &record::wrap_continuation(counter, &named("y", None)).expect("a continuation record"),
        false,
    );
    let consumes = data_consumes(&space.take_event_log().await);
    // Disabled by I-21 (one fire count for each transaction): the two settles lifted from FIRST_STEP_BEFORE and FIRST_STEP_AFTER on.
    // assert_eq!(consumes, vec![
    //     lift_consume(counter, FIRST_STEP_BEFORE, random_state, None),
    //     fired_receive,
    //     lift_consume(counter, FIRST_STEP_AFTER, random_state, None),
    // ]);
    assert_eq!(consumes, vec![
        lift_consume(counter, 0, random_state, None),
        fired_receive,
        lift_consume(counter, 2, random_state, None),
    ]);
    assert_ne!(
        consumes[0].hash, consumes[2].hash,
        "the two lifts of one round differ by their steps"
    );
    let rest = step(&env, capped.reissue.expect("the settle job").op).await;
    assert_eq!(
        rest,
        SpaceOutcome::default(),
        "the receive settled the pending pair"
    );
    assert!(records(&space).await.is_empty());
    assert!(waiters(&space).await.is_empty());
}

#[tokio::test]
async fn space_stack_every_operation_bumps_the_counter() {
    let Fixture { space, env } = fixture().await;
    produce(&env, send(1)).await;
    assert_eq!(counter_value(&space).await, vec![1]);
    space.take_event_log().await;
    let operations = [
        SpaceOp::Consume(peek("p")),
        SpaceOp::Consume(receive("t", any())),
        SpaceOp::Consume(receive("w", exactly(4))),
        SpaceOp::Produce(send(2)),
        SpaceOp::Settle(settle_after_send(&send(2))),
    ];
    for (index, op) in operations.into_iter().enumerate() {
        step(&env, op).await;
        let log = labels(&space.take_event_log().await);
        assert_eq!(&log[..3], &BUMP.map(str::to_string), "operation {index}");
        let expected = i64::try_from(index).expect("a small index") + 2;
        assert_eq!(counter_value(&space).await, vec![expected]);
    }
}

#[tokio::test]
async fn space_stack_order_survives_a_restart() {
    let Fixture { space, env } = fixture().await;
    for value in 1..=3 {
        produce(&env, send(value)).await;
    }
    consume(&env, receive("x", exactly(9))).await;
    let root = space.create_checkpoint().await.expect("a checkpoint").root;
    space
        .reset(&root)
        .await
        .expect("the reset to the checkpoint");
    assert_eq!(records(&space).await, vec![
        (0, vec![int(1)], false),
        (1, vec![int(2)], false),
        (2, vec![int(3)], false)
    ]);
    // The top waiter x blocks every value. A fresh receive reaches the top of the waiters and
    // takes the top record by its order, whatever the list order of the reloaded store.
    for (name, value) in [("a", 3), ("b", 2), ("c", 1)] {
        assert_eq!(fired(&consume(&env, receive(name, any())).await), vec![
            delivery(name, value)
        ]);
    }
    assert_eq!(waiters(&space).await, vec![waiter(3, "x", false, false)]);
}

// ─── Replay and merge ────────────────────────────────────────────────────────────────────────

// Retired by decision 90 (C6, step D): `space_stack_replay_reproduces_play` and its script helpers
// ran through the rigged replay modes, which retire (decision 125). Native replay: the family
// matrix `space_native_replay_equal_{hashmap,pathmap,array,vector}`. Its negative controls:
// `space_native_forged_predecessor_fails_closed`,
// `native_space_replay_rejects_a_tampered_candidate`,
// `native_space_replay_rejects_an_outcome_that_does_not_fit`. The text before:
// ```
// /// Runs `op` of a script with the stack agent.
// async fn script_op(env: &SpaceEnv, op: SpaceOp) -> Result<SpaceOutcome, SpaceError> {
//     run(&stack(), env, op).await
// }
//
// /// A script over five collections: the counterexample, a peek, a record in flight after a
// /// peek waiter, a persistent drain, and a stored divergent pair for three rounds.
// async fn run_script(env: &SpaceEnv) -> Result<Vec<SpaceOutcome>, SpaceError> {
//     let mut outcomes = Vec::new();
//     let cascade = string("cascade");
//     outcomes.push(
//         script_op(
//             env,
//             SpaceOp::Consume(receive_on(SPACE, &cascade, "w2", any())),
//         )
//         .await?,
//     );
//     outcomes.push(
//         script_op(
//             env,
//             SpaceOp::Consume(receive_on(SPACE, &cascade, "w", exactly(2))),
//         )
//         .await?,
//     );
//     outcomes.push(script_op(env, SpaceOp::Produce(send_on(SPACE, &cascade, 1, false))).await?);
//     outcomes.push(script_op(env, SpaceOp::Produce(send_on(SPACE, &cascade, 2, false))).await?);
//     let peeking = string("peeking");
//     outcomes.push(script_op(env, SpaceOp::Produce(send_on(SPACE, &peeking, 5, false))).await?);
//     outcomes.push(
//         script_op(
//             env,
//             SpaceOp::Consume(ConsumeOp {
//                 peek: true,
//                 ..receive_on(SPACE, &peeking, "p", any())
//             }),
//         )
//         .await?,
//     );
//     let flight = string("flight");
//     outcomes.push(
//         script_op(
//             env,
//             SpaceOp::Consume(ConsumeOp {
//                 peek: true,
//                 ..receive_on(SPACE, &flight, "p", any())
//             }),
//         )
//         .await?,
//     );
//     outcomes.push(
//         script_op(
//             env,
//             SpaceOp::Consume(receive_on(SPACE, &flight, "b", exactly(9))),
//         )
//         .await?,
//     );
//     outcomes.push(script_op(env, SpaceOp::Produce(send_on(SPACE, &flight, 1, false))).await?);
//     outcomes.push(script_op(env, SpaceOp::Produce(send_on(SPACE, &flight, 9, false))).await?);
//     let drain = string("drain");
//     for value in 1..=3 {
//         outcomes
//             .push(script_op(env, SpaceOp::Produce(send_on(SPACE, &drain, value, false))).await?);
//     }
//     outcomes.push(
//         script_op(
//             env,
//             SpaceOp::Consume(ConsumeOp {
//                 persistent: true,
//                 ..receive_on(SPACE, &drain, "r", any())
//             }),
//         )
//         .await?,
//     );
//     let divergent = string("divergent");
//     outcomes.push(
//         script_op(
//             env,
//             SpaceOp::Consume(ConsumeOp {
//                 persistent: true,
//                 ..receive_on(SPACE, &divergent, "w", any())
//             }),
//         )
//         .await?,
//     );
//     outcomes.push(
//         script_op(
//             env,
//             SpaceOp::Consume(receive_on(SPACE, &divergent, "b", exactly(999))),
//         )
//         .await?,
//     );
//     outcomes.push(script_op(env, SpaceOp::Produce(send_on(SPACE, &divergent, 1, true))).await?);
//     let mut next = Some(SpaceOp::Produce(send_on(SPACE, &divergent, 999, false)));
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
// async fn space_stack_replay_reproduces_play() {
//     let (play, replay) = play_and_replay().await;
//     let play_env = env_over(play.clone(), &budget());
//     create(&stack(), &play_env, stack_creation()).await;
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
//         "the script fires the cascade, the peeks, the drain and the divergent rounds"
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
async fn space_stack_branches_race_on_the_counter() {
    let Fixture { space, env } = fixture().await;
    produce(&env, send(1)).await;
    let root = space.create_checkpoint().await.expect("a checkpoint").root;
    let push = branch(&space, &env, &root, SpaceOp::Produce(send(2))).await;
    let pop = branch(&space, &env, &root, SpaceOp::Consume(receive("x", any()))).await;
    let settle = branch(
        &space,
        &env,
        &root,
        SpaceOp::Settle(settle_after_send(&send(3))),
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

// ─── The observer calls of the settle (decision 90, decision 69) ─────────────────────────────
//
// Changed by decision 90 (decision 69): this section held the legacy charges of Option P
// (decision 45), which DR-134 deleted with the legacy meter.

// Changed by decision 90 (decision 69): a fresh host records its observer calls, because DR-134
// deleted the legacy meter and `ChargingRSpace` (`recorded_host`).
//
// /// A fresh legacy budget and a charging host over it.
// async fn legacy_host() -> (
//     RuntimeBudget,
//     impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
// ) {
//     let budget = RuntimeBudget::new(Cost::create(i64::MAX / 4, "rrs stack legacy host"));
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

/// The waiting continuation of the receive `name` with the pattern `pattern`, parked with the
/// order `ord` on the data channel of `channel()`.
fn parked_waiter(ord: i64, name: &str, pattern: BindPattern) -> ParWaiter {
    let continuation =
        record::wrap_continuation(ord, &named(name, None)).expect("a continuation record");
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
// Its host differential continues in `space_stack_settle_observer_calls_equal_host`.
//
// /// `legacy_rebracket_eq_host` for the settle (decision 45, Option P): in a legacy deploy the
// /// settle of a stack charges its staged operations what `ChargingRSpace` charges for the
// /// equivalent host calls, in staging order. The send of 2 stages five operations: the take and
// /// the put of the counter, its fire of the top waiter w, the lift of h, and the re-offer of h
// /// to w2. The legacy log of the agent is the chain of the charges of these operations. The host
// /// runs the three shapes that a host can run alone (the take, the put and the lift), and its
// /// entries equal the charges of the same shapes. The host cannot run the two produce-side fires
// /// alone, because the host matches the whole record and the agent matches its payload. The
// /// property `prop_legacy_charges_equal_charging_rspace` covers the charges of that shape.
// #[tokio::test]
// async fn space_stack_settle_legacy_charges_equal_host() {
//     let space = play().await;
//     let budget = RuntimeBudget::new(Cost::create(i64::MAX / 4, "rrs stack legacy agent"));
//     let _legacy = budget.enter_legacy_scope();
//     let env = env_over(space.clone(), &budget);
//     create(&stack(), &env, stack_creation()).await;
//     consume(&env, receive("w2", any())).await;
//     consume(&env, receive("w", exactly(2))).await;
//     let h = ProduceOp {
//         authority: region(2),
//         data: ListParWithRandom {
//             cost_authority: Some(region(2)),
//             ..send(1).data
//         },
//         ..send(1)
//     };
//     produce(&env, h.clone()).await;
//     let d = send(2);
//     let before = budget.get_log().len();
//     assert_eq!(fired(&produce(&env, d.clone()).await), vec![
//         literal("w"),
//         delivery("w2", 1)
//     ]);
//     let agent_log = budget.get_log()[before..].to_vec();
//
//     let random_d = Blake2b512Random::from_bytes(&d.data.random_state);
//     let random_h = Blake2b512Random::from_bytes(&h.data.random_state);
//     let stored_counter = record::counter_record(
//         3,
//         &RandomPurpose::Counter.derive(&random_h).split_byte(1),
//         Some(region(2)),
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
//     let record_d = record::wrap_datum(3, &d.data);
//     let record_h = record::wrap_datum(2, &h.data);
//     // Disabled by I-21 (one fire count for each transaction): the settle after the plan had the first step FIRST_STEP_AFTER. The step is now the
//     // fire count of the transaction: the plan fired w once.
//     // let k_lift = lift_continuation(3, FIRST_STEP_AFTER, &lift_random(&d.data.random_state), None);
//     let k_lift = lift_continuation(3, 1, &lift_random(&d.data.random_state), None);
//
//     // The five staged operations of the send of 2, as the agent stages them.
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
//         parked_waiter(1, "w", exactly(2)),
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
//         parked_waiter(0, "w2", any()),
//         ListParWithRandom {
//             pars: vec![int(1)],
//             ..ListParWithRandom::default()
//         },
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
/// `space_stack_settle_legacy_charges_equal_host` compares the observer calls. The send of 2
/// stages five operations: the take and the put of the counter, its fire of the top waiter w, the
/// lift of h, and the re-offer of h to w2. The agent makes the observer calls of five operations
/// in staging order. Each operation makes the observer calls of its host call, with the same
/// arguments:
///
/// * the take, the put and the lift: on a fresh host that holds the record that the call binds;
/// * the plan fire: on a lone host that holds w (`lone_host_produce`);
/// * the re-offer: on a host that stored h, lifted it and holds w2. The COMM of the re-offer
///   counts the earlier store of h in its `times_repeated`, so the host makes that store too.
///
/// Each row of the space names the footprint group of decision 92, which the host calls do not
/// have. So the comparison removes that group from the calls of the agent (`without_footprint`).
#[tokio::test]
async fn space_stack_settle_observer_calls_equal_host() {
    let space = play().await;
    let env = env_over(space.clone(), &budget());
    create(&stack(), &env, stack_creation()).await;
    consume(&env, receive("w2", any())).await;
    consume(&env, receive("w", exactly(2))).await;
    let h = ProduceOp {
        authority: region(2),
        data: ListParWithRandom {
            cost_authority: Some(region(2)),
            ..send(1).data
        },
        ..send(1)
    };
    produce(&env, h.clone()).await;
    let d = send(2);
    let recorder = observed(&space);
    assert_eq!(fired(&produce(&env, d.clone()).await), vec![
        literal("w"),
        delivery("w2", 1)
    ]);
    // Changed by decision 92: each row of the space names the footprint group, which the host
    // calls do not have.
    let agent = operations(&without_footprint(
        &recorder.take(),
        &space_counter_channel(SPACE),
    ));
    assert_eq!(agent.len(), 5, "the send stages five operations");

    let random_d = Blake2b512Random::from_bytes(&d.data.random_state);
    let random_h = Blake2b512Random::from_bytes(&h.data.random_state);
    let stored_counter = record::counter_record(
        3,
        &RandomPurpose::Counter.derive(&random_h).split_byte(1),
        // Changed by decision 124 (plan v2 C7.2, decision 99): the machinery seal. The text before:
        // Some(region(2)),
        Some(
            system_residue_authority_with_caller(&region(2), &env.budget.deploy_id(), None)
                .expect("the machinery seal builds"),
        ),
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
    let record_d = record::wrap_datum(3, &d.data);
    let record_h = record::wrap_datum(2, &h.data);
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
    // The plan fire of w.
    let plan_fire = offer_fire(
        record_d,
        parked_waiter(1, "w", exactly(2)),
        ListParWithRandom::default(),
    );
    assert_eq!(
        lone_host_produce(&plan_fire).await,
        agent[2],
        "the plan fire"
    );
    // The lift of h.
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
    // The re-offer of h to w2. The host matches payloads, as the agent matches them.
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
        let w2 = parked_waiter(0, "w2", any());
        let parked = host
            .consume(
                vec![data()],
                w2.patterns,
                w2.continuation,
                false,
                BTreeSet::new(),
            )
            .await
            .expect("the park of w2");
        assert!(parked.is_none(), "w2 parks");
        let run = host_produce(&host, &host_recorder, data(), record_h, false).await;
        assert!(run.fired.is_some(), "the re-offer fires w2");
        assert_eq!(run.calls, agent[4], "the re-offer");
    }
}

// ─── Properties ──────────────────────────────────────────────────────────────────────────────

/// `stack_head_is_max_ord`: the head is the entry with the greatest `(ord, bincode bytes)`, and
/// it does not depend on the list order.
#[test]
fn prop_stack_head_is_max_ord() {
    let entries = vec((0i64..6, "[a-c]{0,2}"), 0..8);
    check(
        pure_config(),
        (entries, arbitrary::<u64>()),
        |(entries, seed)| {
            let ordered: Vec<Ordered<'_, String>> = entries
                .iter()
                .map(|(ord, entry)| Ordered { ord: *ord, entry })
                .collect();
            let head = StackDiscipline
                .head(Side::Data, &ordered)
                .map_err(|error| TestCaseError::fail(format!("{error}")))?;
            let key = |(ord, entry): &(i64, String)| {
                (*ord, bincode::serialize(entry).expect("strings serialize"))
            };
            let expected = entries.iter().map(key).max();
            prop_assert_eq!(head.map(|index| key(&entries[index])), expected.clone());
            // A rotation of the list gives an entry with the same key.
            let mut rotated = entries.clone();
            if !rotated.is_empty() {
                let shift = usize::try_from(seed % rotated.len() as u64).expect("a small shift");
                rotated.rotate_left(shift);
            }
            let ordered: Vec<Ordered<'_, String>> = rotated
                .iter()
                .map(|(ord, entry)| Ordered { ord: *ord, entry })
                .collect();
            let head = StackDiscipline
                .head(Side::Waiters, &ordered)
                .map_err(|error| TestCaseError::fail(format!("{error}")))?;
            prop_assert_eq!(head.map(|index| key(&rotated[index])), expected);
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

/// One operation of a generated schedule.
#[derive(Clone, Debug)]
enum StackStep {
    Push {
        value: i64,
        persistent: bool,
    },
    Pop {
        pattern: Pat,
        persistent: bool,
        peek: bool,
    },
}

/// A schedule of up to nine operations on one stack. A schedule has persistent sends or
/// persistent receives, never both, so it has no divergent pair and no re-issue. A persistent
/// receive never peeks (`CONSUME_ARROWS`).
fn arb_schedule() -> BoxedStrategy<Vec<StackStep>> {
    let raw = prop_oneof![
        (1i64..=2, arbitrary::<bool>())
            .prop_map(|(value, persistent)| StackStep::Push { value, persistent }),
        (
            prop_oneof![Just(Pat::Lit(1)), Just(Pat::Lit(2)), Just(Pat::Any)],
            arbitrary::<bool>(),
            arbitrary::<bool>()
        )
            .prop_map(|(pattern, persistent, peek)| StackStep::Pop {
                pattern,
                persistent: persistent && !peek,
                peek,
            }),
    ];
    (arbitrary::<bool>(), vec(raw, 1..10))
        .prop_map(|(persistent_sends, steps)| {
            steps
                .into_iter()
                .map(|step| match step {
                    StackStep::Push { value, persistent } => StackStep::Push {
                        value,
                        persistent: persistent && persistent_sends,
                    },
                    StackStep::Pop {
                        pattern,
                        persistent,
                        peek,
                    } => StackStep::Pop {
                        pattern,
                        persistent: persistent && !persistent_sends,
                        peek,
                    },
                })
                .collect()
        })
        .boxed()
}

/// A record of the reference model.
#[derive(Clone, Debug)]
struct ModelRecord {
    ord: i64,
    value: i64,
    persistent: bool,
}

/// A waiter of the reference model.
#[derive(Clone, Debug)]
struct ModelWaiter {
    ord: i64,
    name: String,
    pattern: Pat,
    persistent: bool,
    peek: bool,
}

/// The reference model of a stack: the rules of the module documentation of `stack.rs`, written
/// without the engine. Only the tops interact, a fresh element reaches its top, a peek keeps a
/// non-persistent record by its re-store, and the settle fires the tops while they match. It
/// counts the fires of each settle.
#[derive(Clone, Debug, Default)]
struct Model {
    records: Vec<ModelRecord>,
    waiters: Vec<ModelWaiter>,
    counter: i64,
}

/// The fires of one operation of the model, and the fires of its settles.
#[derive(Debug, Default)]
struct ModelRun {
    fires: Vec<(String, Vec<Par>)>,
    settle_fires: usize,
}

impl Model {
    fn top_record(&self) -> Option<usize> {
        (0..self.records.len()).max_by_key(|index| self.records[*index].ord)
    }

    fn top_waiter(&self) -> Option<usize> {
        (0..self.waiters.len()).max_by_key(|index| self.waiters[*index].ord)
    }

    /// The settle: the tops fire while they match.
    fn settle(&mut self, run: &mut ModelRun) {
        while let (Some(record), Some(waiter)) = (self.top_record(), self.top_waiter()) {
            let (value, record_persistent) =
                (self.records[record].value, self.records[record].persistent);
            let fired = self.waiters[waiter].clone();
            if !fired.pattern.matches(value) {
                return;
            }
            run.fires.push(fire_of(&fired, value));
            run.settle_fires += 1;
            if !record_persistent && !fired.peek {
                self.records.remove(record);
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

    fn push(&mut self, value: i64, persistent: bool) -> ModelRun {
        let mut run = ModelRun::default();
        self.settle(&mut run);
        let ord = self.bump();
        while let Some(waiter) = self.top_waiter() {
            let fired = self.waiters[waiter].clone();
            if !fired.pattern.matches(value) {
                break;
            }
            run.fires.push(fire_of(&fired, value));
            if !fired.persistent {
                self.waiters.retain(|kept| kept.ord != fired.ord);
            }
            if !persistent && !fired.peek {
                self.settle(&mut run);
                return run;
            }
        }
        self.records.push(ModelRecord {
            ord,
            value,
            persistent,
        });
        self.settle(&mut run);
        run
    }

    fn pop(&mut self, name: &str, pattern: Pat, persistent: bool, peek: bool) -> ModelRun {
        let mut run = ModelRun::default();
        self.settle(&mut run);
        let ord = self.bump();
        let waiter = ModelWaiter {
            ord,
            name: name.to_string(),
            pattern,
            persistent,
            peek,
        };
        while let Some(record) = self.top_record() {
            let (value, record_persistent) =
                (self.records[record].value, self.records[record].persistent);
            if !pattern.matches(value) {
                break;
            }
            run.fires.push(fire_of(&waiter, value));
            if !record_persistent && !peek {
                self.records.remove(record);
            }
            if !persistent {
                self.settle(&mut run);
                return run;
            }
        }
        self.waiters.push(waiter);
        self.settle(&mut run);
        run
    }

    fn records(&self) -> Vec<(i64, Vec<Par>, bool)> {
        let mut records: Vec<_> = self
            .records
            .iter()
            .map(|record| (record.ord, vec![int(record.value)], record.persistent))
            .collect();
        records.sort_by_key(|(ord, ..)| *ord);
        records
    }

    fn waiters(&self) -> Vec<(i64, String, bool, bool)> {
        let mut waiters: Vec<_> = self
            .waiters
            .iter()
            .map(|waiter| {
                (
                    waiter.ord,
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
fn op_of(index: usize, step: &StackStep) -> SpaceOp {
    match step {
        StackStep::Push { value, persistent } => SpaceOp::Produce(ProduceOp {
            data: ListParWithRandom {
                random_state: Blake2b512Random::create_from_bytes(
                    format!("stack schedule push {index}").as_bytes(),
                )
                .to_bytes(),
                ..send(*value).data
            },
            ..send_on(SPACE, &channel(), *value, *persistent)
        }),
        StackStep::Pop {
            pattern,
            persistent,
            peek,
        } => SpaceOp::Consume(ConsumeOp {
            persistent: *persistent,
            peek: *peek,
            ..receive(&format!("w{index}"), pattern.bind_pattern())
        }),
    }
}

/// Applies the step `index` to the model and returns its run.
fn model_step(model: &mut Model, index: usize, step: &StackStep) -> ModelRun {
    match step {
        StackStep::Push { value, persistent } => model.push(*value, *persistent),
        StackStep::Pop {
            pattern,
            persistent,
            peek,
        } => model.pop(&format!("w{index}"), *pattern, *persistent, *peek),
    }
}

/// True when the top record of the store and its top waiter match: a pending match (INV fails).
async fn pending_match(space: &ParRSpace) -> bool {
    let records = records(space).await;
    let parked: Vec<(i64, BindPattern, bool)> = space
        .get_waiting_continuations(vec![data()])
        .await
        .into_iter()
        .map(|waiter| {
            let ord =
                record::continuation_ord(&waiter.continuation).expect("a continuation record");
            let pattern = waiter.patterns.first().cloned().expect("one bind");
            (ord, pattern, waiter.persist)
        })
        .collect();
    let top_record = records.iter().max_by_key(|(ord, ..)| *ord);
    let top_waiter = parked.iter().max_by_key(|(ord, ..)| *ord);
    match (top_record, top_waiter) {
        (Some((_, payload, _)), Some((_, pattern, _))) => {
            let value = ListParWithRandom {
                pars: payload.clone(),
                ..ListParWithRandom::default()
            };
            Matcher.get(pattern, &value).is_some()
        }
        _ => false,
    }
}

/// Runs `schedule` on a fresh stack and on the model, and checks `check_step` after each step.
async fn schedule_case(
    schedule: Vec<StackStep>,
    mut check_step: impl FnMut(&ModelRun, &SpaceOutcome, usize) -> Result<(), TestCaseError>,
    check_store: bool,
) -> Result<(), TestCaseError> {
    let Fixture { space, env } = fixture().await;
    let mut model = Model::default();
    for (index, step_of) in schedule.iter().enumerate() {
        let measure = model.measure();
        let expected = model_step(&mut model, index, step_of);
        let outcome = run(&stack(), &env, op_of(index, step_of))
            .await
            .map_err(|error| TestCaseError::fail(format!("step {index}: {error}")))?;
        prop_assert_eq!(&outcome.reissue, &None, "step {}: no re-issue", index);
        check_step(&expected, &outcome, measure)?;
        if check_store {
            prop_assert_eq!(records(&space).await, model.records(), "step {}", index);
            prop_assert_eq!(waiters(&space).await, model.waiters(), "step {}", index);
            prop_assert!(
                !pending_match(&space).await,
                "step {}: the tops match",
                index
            );
        }
    }
    Ok(())
}

/// `LifoOrder`, `stack_lifo` and `stack_head_blocks`: every fire binds the top record and the
/// top waiter. The agent fires exactly the fires of the reference model, in its order, and its
/// store equals the store of the model after every operation.
///
/// `NoRecordLost` and `stack_settle_keeps_unspent_records`: the reference model removes a record
/// only when a waiter that does not peek spends it. So the equal stores also show that no settle
/// loses a lifted record, also when a waiter that peeks leaves the record in flight.
#[test]
fn prop_stack_lifo_order_any_schedule() {
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

/// `StackNoPendingMatch`, `stack_produce_preserves_inv`, `stack_consume_preserves_inv` and
/// `stack_peek_restore_needs_no_settle`: after every completed operation the top record and the
/// top waiter of the store do not match.
#[test]
fn prop_stack_no_pending_match_any_schedule() {
    check(pure_config(), arb_schedule(), |schedule| {
        tokio_runtime().block_on(async {
            let Fixture { space, env } = fixture().await;
            for (index, step_of) in schedule.iter().enumerate() {
                let outcome = run(&stack(), &env, op_of(index, step_of))
                    .await
                    .map_err(|error| TestCaseError::fail(format!("step {index}: {error}")))?;
                prop_assert_eq!(&outcome.reissue, &None);
                prop_assert!(
                    !pending_match(&space).await,
                    "step {}: the tops match",
                    index
                );
            }
            Ok(())
        })
    });
}

/// `stack_settle_terminates`: the settles of an operation fire at most once for each stored
/// entry that is not persistent, and the operation fires at most once more for its own element.
/// One transaction makes at most `FIRE_ROUND_CAP` fires, its settles included (I-21).
#[test]
fn prop_stack_settle_bounded_any_schedule() {
    check(pure_config(), arb_schedule(), |schedule| {
        tokio_runtime().block_on(schedule_case(
            schedule,
            |expected, outcome, measure| {
                prop_assert!(expected.settle_fires <= measure);
                prop_assert!(outcome.fires.len() <= measure + 1);
                // Disabled by I-21 (one fire count for each transaction): each settle and the plan had a cap of their own.
                // prop_assert!(outcome.fires.len() <= 3 * FIRE_ROUND_CAP);
                prop_assert!(outcome.fires.len() <= FIRE_ROUND_CAP);
                Ok(())
            },
            false,
        ))
    });
}

/// `stack_plan_permutation_invariant`: a restart reloads the store in another list order, and
/// the plans do not change: a run with a checkpoint and a reset after every operation fires and
/// stores what a run without them does.
#[test]
fn prop_stack_plan_permutation_invariant() {
    check(pure_config(), arb_schedule(), |schedule| {
        tokio_runtime().block_on(async {
            let plain = fixture().await;
            let restarted = fixture().await;
            for (index, step_of) in schedule.iter().enumerate() {
                let expected = run(&stack(), &plain.env, op_of(index, step_of))
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
                let outcome = run(&stack(), &restarted.env, op_of(index, step_of))
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
    Fresh(StackStep),
    Reenter,
}

/// A stored divergent pair below a blocker: the persistent receive of every value, the receive
/// of 2 on top of it, the persistent send of 1 (the top waiter does not fit it, so it is stored),
/// and the send of 2, which fires the top waiter and exposes the pair. The settle after that
/// send re-issues a settle job. The settle before the plan of each later operation meets the
/// pair, so it re-issues that operation, also an operation that is not persistent.
fn exposed_divergent_pair() -> Vec<RoundStep> {
    vec![
        RoundStep::Fresh(StackStep::Pop {
            pattern: Pat::Any,
            persistent: true,
            peek: false,
        }),
        RoundStep::Fresh(StackStep::Pop {
            pattern: Pat::Lit(2),
            persistent: false,
            peek: false,
        }),
        RoundStep::Fresh(StackStep::Push {
            value: 1,
            persistent: true,
        }),
        RoundStep::Fresh(StackStep::Push {
            value: 2,
            persistent: false,
        }),
    ]
}

/// A re-entry below a younger top: the persistent receive of 1, the persistent send of 1 (a
/// divergent pair, so the send re-issues with its first-round order), the send of 2 (the top
/// waiter does not fit it, so it is stored on top), and the re-entry of the send of 1.
fn reentry_below_top() -> Vec<RoundStep> {
    vec![
        RoundStep::Fresh(StackStep::Pop {
            pattern: Pat::Lit(1),
            persistent: true,
            peek: false,
        }),
        RoundStep::Fresh(StackStep::Push {
            value: 1,
            persistent: true,
        }),
        RoundStep::Fresh(StackStep::Push {
            value: 2,
            persistent: false,
        }),
        RoundStep::Reenter,
    ]
}

/// A schedule of up to fifteen rounds: no prefix or one of the two prefixes above, then up to
/// eleven generated rounds. Persistent sends and persistent receives can occur in one schedule,
/// so divergent pairs re-issue operations and settle jobs (A10), and the re-issued rounds
/// interleave with fresh operations. A persistent receive never peeks.
/// A fresh operation of a schedule: a send of 1 or 2, persistent or not, or a receive of 1, of 2
/// or of every value, persistent, peeking, or plain. A persistent receive never peeks.
fn arb_fresh_step() -> BoxedStrategy<StackStep> {
    prop_oneof![
        (1i64..=2, arbitrary::<bool>())
            .prop_map(|(value, persistent)| StackStep::Push { value, persistent }),
        (
            prop_oneof![Just(Pat::Lit(1)), Just(Pat::Lit(2)), Just(Pat::Any)],
            arbitrary::<bool>(),
            arbitrary::<bool>()
        )
            .prop_map(|(pattern, persistent, peek)| StackStep::Pop {
                pattern,
                persistent: persistent && !peek,
                peek,
            }),
    ]
    .boxed()
}

fn arb_reissue_schedule() -> BoxedStrategy<Vec<RoundStep>> {
    let fresh = arb_fresh_step();
    let prefix = prop_oneof![
        Just(Vec::new()),
        Just(exposed_divergent_pair()),
        Just(reentry_below_top())
    ];
    let rounds = vec(
        prop_oneof![3 => fresh.prop_map(RoundStep::Fresh), 2 => Just(RoundStep::Reenter)],
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

/// The side of the operation `op` (the records for a send, the waiters for a receive): the
/// orders of its persistent entries, and its greatest order.
async fn own_side(space: &ParRSpace, op: &SpaceOp) -> (Vec<i64>, Option<i64>) {
    let entries: Vec<(i64, bool)> = match op {
        SpaceOp::Produce(_) => records(space)
            .await
            .into_iter()
            .map(|(ord, _, persistent)| (ord, persistent))
            .collect(),
        SpaceOp::Consume(_) => waiters(space)
            .await
            .into_iter()
            .map(|(ord, _, persistent, _)| (ord, persistent))
            .collect(),
        _ => Vec::new(),
    };
    let top = entries.iter().map(|(ord, _)| *ord).max();
    let persistent = entries
        .into_iter()
        .filter_map(|(ord, persistent)| persistent.then_some(ord))
        .collect();
    (persistent, top)
}

/// `ReissueKeepsOrd`, `stack_reissue_keeps_ord` and `stack_reentry_at_lifo_place` (decision
/// 43). On every schedule with divergent pairs, a re-issued persistent operation carries the
/// order of its first round, and a re-issued operation that is not persistent carries none. A
/// settle job re-issues itself. Every round bumps the counter once. A persistent re-entry below
/// the younger top of its side fires nothing and is stored or parked at its first-round order.
/// The counts at the end show that the schedules reach each of these cases.
#[test]
fn prop_stack_reissue_keeps_ord_any_schedule() {
    let persistent_reissues = AtomicUsize::new(0);
    let plain_reissues = AtomicUsize::new(0);
    let settle_reissues = AtomicUsize::new(0);
    let reentries_below_top = AtomicUsize::new(0);
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
                let below_top = match carried {
                    Some((true, Some(ord))) => {
                        let quiet = !pending_match(&space).await;
                        let (_, top) = own_side(&space, &op).await;
                        quiet && top.is_some_and(|top| ord < top)
                    }
                    _ => false,
                };
                let outcome = run(&stack(), &env, op.clone())
                    .await
                    .map_err(|error| TestCaseError::fail(format!("round {index}: {error}")))?;
                rounds += 1;
                prop_assert_eq!(
                    counter_value(&space).await,
                    vec![rounds],
                    "round {}: one bump for each round",
                    index
                );
                if below_top {
                    reentries_below_top.fetch_add(1, Ordering::Relaxed);
                    prop_assert!(
                        outcome.fires.is_empty(),
                        "round {}: a re-entry below the top fires",
                        index
                    );
                    prop_assert_eq!(&outcome.reissue, &None, "round {}", index);
                    let (persistent, _) = own_side(&space, &op).await;
                    prop_assert!(
                        persistent.contains(&first),
                        "round {}: the re-entry is not at its LIFO place",
                        index
                    );
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
            "a re-entry below the top",
            reentries_below_top.load(Ordering::Relaxed),
        ),
    ];
    eprintln!("prop_stack_reissue_keeps_ord_any_schedule reached: {reached:?}");
    for (case, count) in reached {
        assert!(count > 0, "the schedules never reach {case}");
    }
}

/// The number of pairs that the settle of a cap case exposes: one or two caps of pairs, and
/// the counts around them, so that every case reaches a cap stop that the counts decide.
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

/// I-21 for one transaction: at most `FIRE_ROUND_CAP` fires, and exactly `FIRE_ROUND_CAP` when
/// the transaction re-issues at the cap.
fn check_txn_fires(outcome: &SpaceOutcome) -> Result<(), TestCaseError> {
    prop_assert!(outcome.fires.len() <= FIRE_ROUND_CAP);
    if let Some(reissue) = &outcome.reissue {
        if reissue.cause == ReissueCause::RoundCap {
            prop_assert_eq!(outcome.fires.len(), FIRE_ROUND_CAP);
        }
    }
    Ok(())
}

/// `TxnFiresBounded` and `stack_settle_terminates` at the cap (I-21, decision 49). A blocker
/// and a persistent waiter of every value expose `count` pairs. The send that fires the blocker
/// settles them after its plan, and a fresh receive settles the rest before its plan. So the
/// counts decide which cap stop each case reaches:
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
fn prop_stack_txn_fires_bounded_at_the_cap() {
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
            // A fresh receive that no record fits settles the remaining pairs before its plan.
            let probe = consume(&env, receive("probe", exactly(-1))).await;
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
    eprintln!("prop_stack_txn_fires_bounded_at_the_cap reached: {reached:?}");
    for (case, count) in reached {
        assert!(count > 0, "the cases never reach {case}");
    }
}
