//! The bag discipline of the native spaces (P5a): the agent `GenericSpace<HashMapOuter,
//! BagDiscipline>` runs directly over an in-memory play space and its replay space (P5 design
//! v3, A.7 and A.12).
//!
//! The example tests are the bag tests of the P5 design v2 (section 2.8.4) with the changes of
//! v3 A.12: `peek_keeps_record` became `space_disciplines_bag_peek_restores_record`, and the
//! suite adds `peek_restore_fires_guarded_waiter`, `legacy_charges_equal_host` and
//! `persistent_reissue_keeps_identity`. Decision 89 adds
//! `persistent_twin_binds_the_canonical_record`. Decision 90 (decision 69) retires
//! `legacy_charges_equal_host` with Option P, and `observer_calls_equal_host` keeps its host
//! differential over the observer calls. Decision 102 renames
//! `persistent_twin_binds_the_canonical_record` to `persistent_twin_binds_the_pick`.
//!
//! The property tests check the invariants of the formal model on the agent. Each property
//! names its headline in `DataCollection.v` or `ContinuationCollection.v`:
//!
//! | Headline | Property |
//! |---|---|
//! | F1 `bag_visit_order_canonical` | `prop_bag_visit_order_canonical` |
//! | F2 `bag_consume_first_spatial_match` | `prop_bag_consume_first_spatial_match` |
//! | F3 `bag_multiplicity_preserved` | `prop_bag_multiplicity_preserved` |
//! | F4 `bag_removal_keeps_equal_duplicate` | `prop_bag_removal_keeps_equal_duplicate` |
//! | F5 `bag_persistent_match_not_removed` | `prop_bag_persistent_match_not_removed` |
//! | `bag_peek_restore_is_host_second_send` | `prop_bag_observer_calls_equal_host`, `prop_bag_restore_registers_the_record_authority` |
//! | F12 `waiter_removal_by_record_identity` | `prop_bag_waiter_removal_by_record_identity` |
//! | F13 `pattern_equality_removal_can_remove_another_waiter` | `prop_bag_equal_pattern_waiters_stay_distinct` |
//! | F14 `park_byte_identity_idempotent` | `prop_bag_park_byte_identity_idempotent` |
//! | F15 `waiter_visit_order_canonical` | `prop_bag_waiter_visit_order_canonical` |
//! | F16 `head_waiter_unique` | `prop_head_waiter_unique` |
//!
//! Play picks at random among the candidates of each search (decision 102, rule CR-1). Each test
//! of a pick checks that its outcome is in the allowed set. A coverage set checks that each
//! allowed outcome occurs over [`PICK_RUNS`] runs on fresh rigs. A setup that needs one outcome of
//! a pick tries fresh rigs, or retries from a checkpoint, until the outcome occurs
//! ([`PARK_TRIES`]). The doc of each test gives the miss bound of its probabilistic assertions.
//! F1 and F15 are claims on the replay order (rule CR-4). The replay of a recorded play log takes
//! the candidates of the recorded COMM first (`spaces/generic.rs`, the test
//! `space_pick_helpers_put_the_named_candidates_first_in_replay`). Changed by decision 90 (C6,
//! step D): the replay half of F1 and F15 ran through the rigged replay modes, which retire, so
//! each case keeps its play half. The unit test
//! `space_pick_helpers_shuffle_in_play_and_keep_dr75_in_the_restage` of `spaces/generic.rs` checks
//! that the restage of R-A keeps the DR-75 order. The model of F3 and the host differential follow
//! the picks of the agent.
//!
//! The host differential runs the equivalent host program on an in-memory RSpace whose
//! accounting observer records its calls (decision 90, decision 69). The program makes the host
//! call of each operation, and the calls that the reducer makes after a fire: the re-produce of a
//! persistent send, the re-consume of a persistent receive, and `produce_peeks`. The host space
//! stores the same record bytes on the same channel. Its matcher matches the payload of a record
//! without its envelope, as the agent does. Host play picks at random among candidates since
//! DR-129. So before each host call, the program pins the host to the pick of the agent. It reads
//! the pick from the receipt of the same operation of the agent, which is its observer calls
//! (plan v2 section 5). The COMM of a send names the waiter that the send fires. The COMM of a
//! receive names the record that the receive binds. Without a COMM, the program checks that the
//! plans allow the store or the park. The agent and the program then make the same observer calls
//! with the same arguments, and so the same COMMs, with the same fires and the same stores. A pin
//! names a record by its bytes only. So the program makes a consume again until it binds the
//! record with the persist flag of the agent (decision 89). Before decision 90 the differential
//! compared the legacy charges of `ChargingRSpace`, which DR-134 deleted with the legacy meter.

// Changed by decision 102 (plan v2 pins): the host program follows the picks of the agent, which
// it reads from the receipts of the agent. The text before of the paragraph of the host
// differential in the module doc above:
// The host differential runs the equivalent host program on a real `ChargingRSpace`: the host
// call of each operation, and the calls that the reducer makes after a fire (the re-produce of
// a persistent send, the re-consume of a persistent receive, and `produce_peeks`). The host
// space stores the same record bytes on the same channel, and its matcher matches the payload
// of a record without its envelope, as the agent does. Host play picks at random among
// candidates since DR-129. So before each host call, the program pins the matcher to the
// choice of the canonical order of decision 41 on the host records. A consume binds only the
// first record that its pattern binds, and a produce fires only the first waiter that takes
// its record. A pin names a record by its bytes only. So the program makes a consume again
// until it binds the record with the canonical persist flag (decision 89).

#[path = "support/mod.rs"]
mod support;

// Changed by decision 102 (plan v2 pins): the coverage sets of the tests of random picks.
// use std::collections::{BTreeMap, VecDeque};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::cost_signature::Value as CostSignatureValue;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    BindPattern, CostAuthority, CostRegion, CostSignature, EEq, EVar, Expr, ListParWithRandom, Par,
    TaggedContinuation, Var,
};
use models::rust::space_factory_urn::{SpaceFactoryUrn, SpaceInner, SpaceOuter};
use models::rust::utils::new_gbool_par;
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;
use rholang::rust::interpreter::accounting::authority::AuthorityByteEventKind;
use rholang::rust::interpreter::accounting::byte_accounting::{
    consume_introduction_identity, produce_introduction_identity,
};
// Changed by decision 90 (decision 69): the host differential compares observer calls, not the
// legacy charges of a budget (DR-134 deleted the legacy meter).
// use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::RuntimeBudget;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::reified::costs::{ReissueCause, FIRE_ROUND_CAP};
// Changed by decision 90 (decision 69): the host of a program is a plain RSpace, because
// `ChargingRSpace` is deleted with the legacy meter (DR-134).
// use rholang::rust::interpreter::rho_runtime::RhoISpace;
use rholang::rust::interpreter::spaces::agent::{
    ConsumeOp, ProduceOp, SpaceEnv, SpaceOp, SpaceOutcome, SpaceReissue,
};
use rholang::rust::interpreter::spaces::disciplines::bag::BagDiscipline;
use rholang::rust::interpreter::spaces::disciplines::queue::QueueDiscipline;
use rholang::rust::interpreter::spaces::errors::SpaceError;
use rholang::rust::interpreter::spaces::generic::GenericSpace;
use rholang::rust::interpreter::spaces::namespace::{Durability, NativeSpace, SID_LEN};
use rholang::rust::interpreter::spaces::outer::hashmap::HashMapOuter;
use rholang::rust::interpreter::spaces::record;
use rholang::rust::interpreter::spaces::types::qualifier::SpaceQualifier;
// Changed by decision 90 (decision 69): `ChargingRSpace` is deleted with the legacy meter
// (DR-134).
// use rholang::rust::interpreter::storage::charging_rspace::ChargingRSpace;
// Retired by decision 90 (C6, step D): the retired replay tests of this file were its only users
// (decision 125). The text before:
// use rspace_plus_plus::rspace::checkpoint::Checkpoint;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider;
// Changed by decision 102 (plan v2 pins): the host program reads the record of each consume from
// the receipt of the agent, so it makes no canonical choice of a record.
// use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
// Changed by decision 102 (plan v2 pins): see the change of the import of `Datum`.
// use rspace_plus_plus::rspace::space_store_handle::canonical_data_order;
use rspace_plus_plus::rspace::trace::event::{Consume, Event, IOEvent, Produce, COMM};
// Changed by decision 102 (plan v2 pins): the tests of the replay order replay over the replay
// space of a play.
// use support::agent::{
//     any, body_name, budget, config_of, counter, counter_channel, create, creation, data_channel,
//     env_over, exactly, fired, int, play, play_and_replay, receive_on, records_on, run, send_on,
//     space_counter_channel, string, ParRSpace,
// };
// Changed by decision 90 (C6, step D): the rigged replay modes retire, so this file no longer uses
// `play_and_replay`. The text before:
// ```
// use support::agent::{
//     any, body_name, budget, config_of, counter, counter_channel, create, creation, data_channel,
//     env_over, exactly, fired, int, play, play_and_replay, receive_on, records_on, run, send_on,
//     space_counter_channel, string, ParRSpace,
// };
// ```
use support::agent::{
    any, body_name, budget, config_of, counter, counter_channel, create, creation, data_channel,
    env_over, exactly, fired, int, play, receive_on, records_on, run, send_on,
    space_counter_channel, string, ParRSpace,
};
// Changed by decision 102 (plan v2 pins): the twin test runs `PICK_RUNS` times, because the host
// program follows the pick of the agent.
// use support::legacy::{
//     drawn_consume, host_consume, host_produce, waiter_choice, WaiterPin, TWIN_RUNS,
// };
use support::legacy::{drawn_consume, host_consume, host_produce, waiter_choice, WaiterPin};
// Changed by decision 92: the host differential removes the footprint group of the space.
// Changed by decision 102 (plan v2 pins): the host program reads the receipts of the agent.
use support::observed::{observe, observed, operations, without_footprint, Call, Recorder};
use support::props::{check, pure_config, tokio_runtime};

// ─── The rig ─────────────────────────────────────────────────────────────────────────────

/// The bag agent.
type Bag = GenericSpace<HashMapOuter, BagDiscipline>;

/// The queue agent, for the head-only contrast of F16.
type Queue = GenericSpace<HashMapOuter, QueueDiscipline>;

/// The space of every rig.
const SPACE: NativeSpace = NativeSpace {
    sid: [21; SID_LEN],
    durability: Durability::Durable,
};

// Changed by decision 90 (decision 69): the host differential compares observer calls, so it has
// no legacy budget (DR-134 deleted the legacy meter).
//
// /// The phlo of a legacy differential: far above every program of the suite.
// const LEGACY_PHLO: i64 = 1_000_000_000_000;

/// The family `hashmap:bag`.
fn bag_family() -> SpaceFactoryUrn {
    SpaceFactoryUrn {
        outer: SpaceOuter::HashMap,
        inner: SpaceInner::Bag,
    }
}

/// The channel of the collection of every rig.
fn channel() -> Par { string("rrs-bag") }

/// The data channel `D(sid,k)` of the collection.
fn data() -> Par { data_channel(SPACE, &channel()) }

/// A bag space over a fresh in-memory play space.
struct Rig {
    host: ParRSpace,
    env: SpaceEnv,
}

impl Rig {
    /// A rig whose operations charge the phlo budget of the suite.
    async fn new() -> Rig { Rig::with_budget(&budget()).await }

    /// A rig whose operations charge `budget`.
    async fn with_budget(budget: &RuntimeBudget) -> Rig { Rig::over(play().await, budget).await }

    /// A rig over `host`, a fresh space, whose operations charge `budget`.
    async fn over(host: ParRSpace, budget: &RuntimeBudget) -> Rig {
        let env = env_over(host.clone(), budget);
        let config = config_of(SPACE, "hashmap", "bag", SpaceQualifier::Default);
        create(&Bag::default(), &env, creation(SPACE, config, bag_family())).await;
        host.take_event_log().await;
        Rig { host, env }
    }

    async fn run(&self, op: SpaceOp) -> Result<SpaceOutcome, SpaceError> {
        run(&Bag::default(), &self.env, op).await
    }

    async fn send(&self, op: ProduceOp) -> SpaceOutcome {
        self.run(SpaceOp::Produce(op))
            .await
            .expect("the send commits")
    }

    async fn receive(&self, op: ConsumeOp) -> SpaceOutcome {
        self.run(SpaceOp::Consume(op))
            .await
            .expect("the receive commits")
    }

    /// The values of the records of the collection with their persist flags, sorted.
    async fn values(&self) -> Vec<(i64, bool)> { values_of(&self.host).await }

    /// The body names of the waiting continuations of the collection, sorted.
    async fn waiters(&self) -> Vec<String> { waiter_names(&self.host).await }

    /// The events since the last call.
    async fn log(&self) -> Vec<Event> { self.host.take_event_log().await }
}

/// The value of a payload of one number.
fn value_of(pars: &[Par]) -> i64 {
    match pars {
        [Par { exprs, .. }] => match exprs.as_slice() {
            [Expr {
                expr_instance: Some(ExprInstance::GInt(value)),
            }] => *value,
            _ => panic!("a payload of one number: {pars:?}"),
        },
        _ => panic!("a payload of one number: {pars:?}"),
    }
}

/// The values of the records of the collection on `space`, with their persist flags, sorted.
async fn values_of(
    space: &impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
) -> Vec<(i64, bool)> {
    let mut values: Vec<(i64, bool)> = space
        .get_data(&data())
        .await
        .iter()
        .map(|datum| {
            let record = record::unwrap_datum(&datum.a).expect("a datum record");
            assert_eq!(record.ord, 0, "every record of a bag has the ord 0");
            (value_of(&record.datum.pars), datum.persist)
        })
        .collect();
    values.sort();
    values
}

/// The body names of the waiting continuations of the collection on `space`, sorted.
async fn waiter_names(
    space: &impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
) -> Vec<String> {
    let mut names: Vec<String> = space
        .get_waiting_continuations(vec![data()])
        .await
        .iter()
        .map(|waiter| {
            let parked =
                record::unwrap_continuation(&waiter.continuation).expect("a continuation record");
            assert_eq!(parked.ord, 0, "every waiter of a bag has the ord 0");
            body_name(&parked.continuation)
        })
        .collect();
    names.sort();
    names
}

/// The events of a log as labels: `produce`, `consume`, `comm` or `comm peek`, each with `D`
/// for the data channel of the collection and `other` for any other channel.
fn labels(log: &[Event]) -> Vec<String> {
    let data = stable_hash_provider::hash(&data());
    let on = |hash: &Blake2b256Hash| match *hash == data {
        true => "D",
        false => "other",
    };
    log.iter()
        .map(|event| match event {
            Event::IoEvent(IOEvent::Produce(produce)) => {
                format!("produce {}", on(&produce.channel_hash))
            }
            Event::IoEvent(IOEvent::Consume(consume)) => {
                let names: Vec<&str> = consume.channel_hashes.iter().map(on).collect();
                format!("consume {}", names.join(","))
            }
            Event::Comm(comm) => {
                let names: Vec<&str> = comm.consume.channel_hashes.iter().map(on).collect();
                match comm.peeks.is_empty() {
                    true => format!("comm {}", names.join(",")),
                    false => format!("comm {} peek", names.join(",")),
                }
            }
        })
        .collect()
}

/// The produce events of a log.
fn produces(log: &[Event]) -> Vec<Produce> {
    log.iter()
        .filter_map(|event| match event {
            Event::IoEvent(IOEvent::Produce(produce)) => Some(produce.clone()),
            _ => None,
        })
        .collect()
}

/// The COMM events of a log.
fn comms(log: &[Event]) -> Vec<COMM> {
    log.iter()
        .filter_map(|event| match event {
            Event::Comm(comm) => Some(comm.clone()),
            _ => None,
        })
        .collect()
}

/// A delivery of `values` to the body `name`.
fn delivery(name: &str, values: &[i64]) -> (String, Vec<Par>) {
    (
        name.to_string(),
        values.iter().map(|value| int(*value)).collect(),
    )
}

// ─── Operations and the canonical order ───────────────────────────────────────────────────

/// A send of `value` whose random state comes from `salt`.
fn salted(value: i64, salt: u64, persistent: bool) -> ProduceOp {
    let mut op = send_on(SPACE, &channel(), value, persistent);
    op.data.random_state =
        Blake2b512Random::create_from_bytes(format!("rrs-bag-send {value} {salt}").as_bytes())
            .to_bytes();
    op
}

/// A plain receive whose body is the string `name`.
fn receiving(name: &str, pattern: BindPattern) -> ConsumeOp {
    receive_on(SPACE, &channel(), name, pattern)
}

/// `op` with the guard `guard`.
fn guarded(op: ConsumeOp, guard: Par) -> ConsumeOp {
    ConsumeOp {
        guard: Some(guard),
        ..op
    }
}

/// `op` as a peek (`<<-`).
fn peeking(op: ConsumeOp) -> ConsumeOp { ConsumeOp { peek: true, ..op } }

/// `op` as a persistent receive (`<=`).
fn persisting(op: ConsumeOp) -> ConsumeOp {
    ConsumeOp {
        persistent: true,
        ..op
    }
}

/// `op` with the random state of its body taken from `salt`. The body stays the same.
fn resalted(op: ConsumeOp, salt: u64) -> ConsumeOp {
    let mut op = op;
    op.body.random_state = Blake2b512Random::create_from_bytes(
        format!("rrs-bag-receive {} {salt}", body_name(&parked_body(&op))).as_bytes(),
    )
    .to_bytes();
    op
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

/// The guard that refuses every match.
fn refusing() -> Par { new_gbool_par(false, Vec::new(), false) }

/// The guard that admits every match.
fn admitting() -> Par { new_gbool_par(true, Vec::new(), false) }

/// The source hash of the record of `op`: the key of the canonical order of the records.
fn record_hash(op: &ProduceOp) -> Blake2b256Hash {
    Produce::create(&data(), &record::wrap_datum(0, &op.data), op.persistent).hash
}

/// The continuation of `op` as the program gives it.
fn parked_body(op: &ConsumeOp) -> TaggedContinuation {
    TaggedContinuation {
        tagged_cont: Some(TaggedCont::ParBody(op.body.clone())),
        guard: op.guard.clone(),
        cost_authority: record::cost_authority(&op.authority),
    }
}

/// The parked continuation of `op`: the continuation inside the envelope with the ord 0.
fn parked(op: &ConsumeOp) -> TaggedContinuation {
    record::wrap_continuation(0, &parked_body(op)).expect("a receive has a body")
}

/// The source hash of the waiter of `op`: the key of the canonical order of the waiters.
fn waiter_hash(op: &ConsumeOp) -> Blake2b256Hash {
    Consume::create(
        &vec![data()],
        &vec![op.pattern.clone()],
        &parked(op),
        op.persistent,
    )
    .hash
}

/// Sends of `(value, persistent)` whose records follow each other in canonical order. The
/// salts are searched in a fixed order, so every run makes the same sends.
fn ordered_sends(entries: &[(i64, bool)]) -> Vec<ProduceOp> {
    let mut sends: Vec<ProduceOp> = Vec::with_capacity(entries.len());
    let mut salt = 0;
    for (value, persistent) in entries {
        loop {
            salt += 1;
            let candidate = salted(*value, salt, *persistent);
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

/// The receives `ops` with their random states salted so that their waiters follow each other
/// in canonical order. The salts are searched in a fixed order.
fn ordered_receives(ops: Vec<ConsumeOp>) -> Vec<ConsumeOp> {
    let mut receives: Vec<ConsumeOp> = Vec::with_capacity(ops.len());
    let mut salt = 0;
    for op in ops {
        loop {
            salt += 1;
            let candidate = resalted(op.clone(), salt);
            if receives
                .last()
                .is_none_or(|last| waiter_hash(last) < waiter_hash(&candidate))
            {
                receives.push(candidate);
                break;
            }
        }
    }
    receives
}

/// A cost authority with one region of `byte`.
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

/// The introduction authority that `budget` registered for the operation of `event`.
fn registered(budget: &RuntimeBudget, event: &Event) -> Option<CostAuthority> {
    match event {
        Event::IoEvent(IOEvent::Produce(produce)) => budget
            .introduction_authority(
                produce_introduction_identity(produce),
                AuthorityByteEventKind::ProduceIntroduction,
            )
            .ok(),
        Event::IoEvent(IOEvent::Consume(consume)) => budget
            .introduction_authority(
                consume_introduction_identity(consume),
                AuthorityByteEventKind::ConsumeIntroduction,
            )
            .ok(),
        Event::Comm(_) => None,
    }
}

// ─── Random picks and the replay order (decision 102, rules CR-1 and CR-4) ─────────────────

/// Added by decision 102 (plan v2 pins): the runs of a play test of a random pick, each on a
/// fresh rig. An outcome with a chance of `p` or more is missing from all runs with a chance of
/// `(1 - p)^64` or less. So one of two outcomes with a chance of 1/2 each is missing with a chance
/// of `2·2^-64 = 2^-63`. One of three outcomes with a chance of 1/3 each is missing with a chance
/// of `3·(2/3)^64`, which is less than `2·10^-11`.
const PICK_RUNS: usize = 64;

/// Added by decision 102 (plan v2 pins, decision 37): the tries of a setup that needs one outcome
/// of a pick ([`parked_by_a_refused_pick`], [`parked_from_a_checkpoint`]). A pick of this suite has
/// at most six candidates, so each try gets an allowed outcome with a chance of 1/6 or more. So
/// all tries miss the outcome with a chance of `(5/6)^128`, which is less than `8·10^-11`. An
/// outcome with a chance of 1/2 is missing from all tries with a chance of `2^-128`.
const PARK_TRIES: usize = 128;

/// Added by decision 102 (plan v2 pins): the runs of each generated program of
/// `prop_bag_multiplicity_preserved`, each on a fresh rig, so that the model meets more picks of
/// the agent.
const CASE_RUNS: usize = 4;

/// Added by decision 102 (plan v2 pins, decision 37): a fresh rig that stores `stored` and holds
/// the guarded receive `waiter` as a waiter. Play picks the record of a receive at random and
/// checks the guard once, on the pick. So the receive parks when its pick is a record that the
/// guard refuses. Otherwise it makes the fire `fire`, which is the other allowed outcome, and the
/// next try starts on a fresh rig. A try of this suite parks with a chance of 1/2, so all
/// [`PARK_TRIES`] tries fire with a chance of `2^-128`.
async fn parked_by_a_refused_pick(
    stored: &[ProduceOp],
    waiter: &ConsumeOp,
    fire: &(String, Vec<Par>),
) -> Rig {
    for _ in 0..PARK_TRIES {
        let rig = Rig::new().await;
        for send in stored {
            rig.send(send.clone()).await;
        }
        let outcome = rig.receive(waiter.clone()).await;
        let fires = fired(&outcome);
        match fires.as_slice() {
            [] => {
                assert_eq!(
                    rig.waiters().await,
                    vec![body_name(&parked_body(waiter))],
                    "the receive parks"
                );
                return rig;
            }
            [only] => assert_eq!(only, fire, "the other allowed outcome of the pick"),
            more => panic!("a receive fires once or parks: {more:?}"),
        }
    }
    panic!("no try of {PARK_TRIES} picks a record that the guard refuses");
}

/// Added by decision 102 (plan v2 pins, decision 37): makes the guarded receive `waiter` park on
/// `rig`. Each try takes a checkpoint and runs the receive. When the receive fires, the try resets
/// the rig to the checkpoint, and the next try runs the receive again. A try of this suite parks
/// with a chance of 1/2, so all [`PARK_TRIES`] tries fire with a chance of `2^-128`.
async fn parked_from_a_checkpoint(rig: &Rig, waiter: &ConsumeOp) {
    for _ in 0..PARK_TRIES {
        let checkpoint = rig
            .host
            .create_checkpoint()
            .await
            .expect("a checkpoint before the receive")
            .root;
        match rig.receive(waiter.clone()).await.fires.as_slice() {
            [] => return,
            _ => rig
                .host
                .reset(&checkpoint)
                .await
                .expect("the reset to the checkpoint before the receive"),
        }
    }
    panic!("no try of {PARK_TRIES} picks a record that the guard refuses");
}

/// Added by decision 102 (rule CR-4): the source hashes of the records and of the waiters of the
/// collection, in the list order of the store.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Listed {
    records: Vec<Blake2b256Hash>,
    waiters: Vec<Blake2b256Hash>,
}

/// Added by decision 102 (rule CR-4): the list order of the collection on `space`.
async fn listed(
    space: &impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
) -> Listed {
    Listed {
        records: space
            .get_data(&data())
            .await
            .iter()
            .map(|datum| datum.source.hash.clone())
            .collect(),
        waiters: space
            .get_waiting_continuations(vec![data()])
            .await
            .iter()
            .map(|waiter| waiter.source.hash.clone())
            .collect(),
    }
}

/// Added by decision 102 (rule CR-4): a play of the operations `build` and then `probe` on a
/// fresh rig. It keeps the rig, the list order of the store after `build` and the outcomes of
/// `probe`.
struct PlayedOrder {
    rig: Rig,
    listed: Listed,
    outcomes: Vec<SpaceOutcome>,
}

/// Added by decision 102 (rule CR-4): plays `build` and then `probe` on a fresh rig. The
/// operations of `build` make no fire.
async fn played_order(build: &[SpaceOp], probe: &[SpaceOp]) -> Result<PlayedOrder, TestCaseError> {
    let budget = budget();
    let play = play().await;
    let rig = Rig::over(play, &budget).await;
    rig.host
        .create_checkpoint()
        .await
        .expect("a checkpoint after the creation");
    for op in build {
        let outcome = rig
            .run(op.clone())
            .await
            .map_err(|error| failure(format!("the build fails: {error}")))?;
        prop_assert!(outcome.fires.is_empty(), "the build makes no fire");
    }
    let listed = listed(&rig.host).await;
    let mut outcomes = Vec::with_capacity(probe.len());
    for op in probe {
        outcomes.push(
            rig.run(op.clone())
                .await
                .map_err(|error| failure(format!("the probe fails: {error}")))?,
        );
    }
    // Retired by decision 90 (C6, step D): only the replay of a `PlayedOrder` read the root and
    // the log, and that replay retires.
    Ok(PlayedOrder {
        rig,
        listed,
        outcomes,
    })
}

// Retired by decision 90 (C6, step D): `ReplayedOrder` and `replayed_order` ran through the rigged
// replay modes, which retire (decision 125).
// `space_pick_helpers_put_the_named_candidates_first_in_replay`,
// `space_native_replay_equal_hashmap`.

/// Added by decision 102 (plan v2 pins): one fire of the agent as the model reads it: the body
/// name of the fired continuation and the source hash of the record that the fire binds.
#[derive(Clone, Debug)]
struct AgentFire {
    name: String,
    hash: Blake2b256Hash,
}

/// Added by decision 102 (plan v2 pins): the fires of the agent in `outcome`, in staging order. A
/// fire of a bag binds one record. Its source hash is the hash of the produce of the record that
/// the fire removed or bound, with the persist flag of the result.
fn agent_fires(outcome: &SpaceOutcome) -> Result<Vec<AgentFire>, TestCaseError> {
    let mut fires = Vec::with_capacity(outcome.fires.len());
    for fire in &outcome.fires {
        let [result] = fire.results.as_slice() else {
            return Err(failure(format!("a bag fire binds one record: {fire:?}")));
        };
        fires.push(AgentFire {
            name: body_name(&fire.continuation),
            hash: Produce::create(&data(), &result.removed_datum, result.persistent).hash,
        });
    }
    Ok(fires)
}

/// Added by decision 102 (plan v2 pins): the fires of the agent that the model has not met yet.
type Picks<'a> = std::iter::Peekable<std::slice::Iter<'a, AgentFire>>;

// ─── The model (DataCollection.v and ContinuationCollection.v) ─────────────────────────────

/// The pattern of a generated receive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pat {
    /// One free variable: every value.
    Any,
    /// One literal: the value itself.
    Lit(i64),
}

impl Pat {
    fn fits(self, value: i64) -> bool {
        match self {
            Pat::Any => true,
            Pat::Lit(literal) => literal == value,
        }
    }

    fn pattern(self) -> BindPattern {
        match self {
            Pat::Any => any(),
            Pat::Lit(literal) => exactly(literal),
        }
    }

    /// The values that the pattern binds from `value`.
    fn bound(self, value: i64) -> Vec<Par> {
        match self {
            Pat::Any => vec![int(value)],
            Pat::Lit(_) => Vec::new(),
        }
    }
}

/// The guard of a generated receive. A guard over the bound value goes only with
/// [`Pat::Any`], so the bound value is the matched value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Guard {
    Free,
    Admit,
    Refuse,
    Equals(i64),
}

impl Guard {
    fn admits(self, value: i64) -> bool {
        match self {
            Guard::Free | Guard::Admit => true,
            Guard::Refuse => false,
            Guard::Equals(expected) => expected == value,
        }
    }

    fn par(self) -> Option<Par> {
        match self {
            Guard::Free => None,
            Guard::Admit => Some(admitting()),
            Guard::Refuse => Some(refusing()),
            Guard::Equals(expected) => Some(equals(expected)),
        }
    }
}

/// A generated operation on the collection.
#[derive(Clone, Debug, PartialEq, Eq)]
enum BagOp {
    Send {
        value: i64,
        salt: u64,
        persistent: bool,
    },
    Receive {
        name: u64,
        pat: Pat,
        guard: Guard,
        persistent: bool,
        peek: bool,
    },
}

impl BagOp {
    fn space_op(&self) -> SpaceOp {
        match *self {
            BagOp::Send {
                value,
                salt,
                persistent,
            } => SpaceOp::Produce(salted(value, salt, persistent)),
            BagOp::Receive {
                name,
                pat,
                guard,
                persistent,
                peek,
            } => SpaceOp::Consume(ConsumeOp {
                persistent,
                peek,
                guard: guard.par(),
                ..receiving(&format!("w{name}"), pat.pattern())
            }),
        }
    }
}

/// A record of the model: its value, its persist flag and its source hash.
#[derive(Clone, Debug)]
struct ModelRecord {
    value: i64,
    persist: bool,
    hash: Blake2b256Hash,
}

/// A waiter of the model.
#[derive(Clone, Debug)]
struct ModelWaiter {
    name: String,
    pat: Pat,
    guard: Guard,
    persist: bool,
    peek: bool,
    hash: Blake2b256Hash,
}

/// The executable model of a bag collection: the plans of `DataCollection.v` and
/// `ContinuationCollection.v` over values. Play picks at random among the candidates of each
/// search (decision 102, rule CR-1). So the model follows the agent: it reads each pick of the
/// agent from its fires ([`AgentFire`]), checks that the plans allow the pick, and applies it. A
/// consume checks the guard once, on its pick (decision 37), and it parks when the guard refuses.
/// A produce fires a waiter whose pattern binds the datum and whose guard admits it, and the
/// datum is stored only when no waiter takes it. A peek of a non-persistent record removes it and
/// offers it again (decision 45, Rule E). The generators never make a divergent pair.
///
/// Changed by decision 102 (plan v2 pins): the model follows the picks of the agent. The text
/// before: "The candidates of each side are visited by source hash (decision 41). A consume checks
/// the guard once on its first spatial match (decision 37). A produce skips a waiter whose guard
/// refuses."
#[derive(Clone, Debug, Default)]
struct BagModel {
    records: Vec<ModelRecord>,
    waiters: Vec<ModelWaiter>,
}

impl BagModel {
    // Changed by decision 102 (plan v2 pins): the model follows the picks of the agent, so it has
    // no canonical order. The methods before:
    // /// Applies `op` and returns its fires, as `fired` reports them.
    // fn apply(&mut self, op: &BagOp) -> Vec<(String, Vec<Par>)> {
    //     let mut fires = Vec::new();
    //     match (op, op.space_op()) {
    //         (
    //             BagOp::Send {
    //                 value, persistent, ..
    //             },
    //             SpaceOp::Produce(send),
    //         ) => self.offer(*value, *persistent, record_hash(&send), &mut fires),
    //         (
    //             BagOp::Receive {
    //                 pat,
    //                 guard,
    //                 persistent,
    //                 peek,
    //                 ..
    //             },
    //             SpaceOp::Consume(receive),
    //         ) => {
    //             let waiter = ModelWaiter {
    //                 name: body_name(&parked_body(&receive)),
    //                 pat: *pat,
    //                 guard: *guard,
    //                 persist: *persistent,
    //                 peek: *peek,
    //                 hash: waiter_hash(&receive),
    //             };
    //             self.receive(waiter, &mut fires)
    //         }
    //         _ => panic!("an operation and its space operation disagree"),
    //     }
    //     fires
    // }
    //
    // /// The indices of `hashes` in canonical order. Equal hashes are byte-identical entries, so
    // /// their order does not matter.
    // fn canonical(hashes: impl Iterator<Item = Blake2b256Hash>) -> Vec<usize> {
    //     let mut indexed: Vec<(Blake2b256Hash, usize)> = hashes
    //         .enumerate()
    //         .map(|(index, hash)| (hash, index))
    //         .collect();
    //     indexed.sort();
    //     indexed.into_iter().map(|(_, index)| index).collect()
    // }
    //
    // /// OFFER: the datum fires the first waiter in canonical order that it fits and whose guard
    // /// admits it, again while it is not spent, and it is stored when no waiter takes it.
    // fn offer(
    //     &mut self,
    //     value: i64,
    //     persist: bool,
    //     hash: Blake2b256Hash,
    //     fires: &mut Vec<(String, Vec<Par>)>,
    // ) {
    //     loop {
    //         let order = Self::canonical(self.waiters.iter().map(|waiter| waiter.hash.clone()));
    //         let found = order.into_iter().find(|index| {
    //             let waiter = &self.waiters[*index];
    //             waiter.pat.fits(value) && waiter.guard.admits(value)
    //         });
    //         let Some(index) = found else {
    //             self.records.push(ModelRecord {
    //                 value,
    //                 persist,
    //                 hash,
    //             });
    //             return;
    //         };
    //         let waiter = self.waiters[index].clone();
    //         fires.push((waiter.name.clone(), waiter.pat.bound(value)));
    //         if !waiter.persist {
    //             self.waiters.remove(index);
    //         }
    //         let spent = !persist && !waiter.peek;
    //         assert!(
    //             spent || !waiter.persist,
    //             "the generators never make a divergent pair"
    //         );
    //         if spent {
    //             return;
    //         }
    //     }
    // }
    //
    // /// RECEIVE: the first record in canonical order that the pattern fits, the guard checked
    // /// once on it, a park on a refusal, and the re-offer of a peeked record.
    // fn receive(&mut self, waiter: ModelWaiter, fires: &mut Vec<(String, Vec<Par>)>) {
    //     loop {
    //         let order = Self::canonical(self.records.iter().map(|record| record.hash.clone()));
    //         let found = order
    //             .into_iter()
    //             .find(|index| waiter.pat.fits(self.records[*index].value));
    //         let admitted = found.filter(|index| waiter.guard.admits(self.records[*index].value));
    //         let Some(index) = admitted else {
    //             if !self.waiters.iter().any(|parked| parked.hash == waiter.hash) {
    //                 self.waiters.push(waiter);
    //             }
    //             return;
    //         };
    //         let record = self.records[index].clone();
    //         fires.push((waiter.name.clone(), waiter.pat.bound(record.value)));
    //         if !record.persist {
    //             self.records.remove(index);
    //         }
    //         if waiter.peek && !record.persist {
    //             self.offer(record.value, false, record.hash.clone(), fires);
    //         }
    //         let spent = !waiter.persist;
    //         assert!(
    //             spent || !(waiter.peek || record.persist),
    //             "the generators never make a divergent pair"
    //         );
    //         if spent {
    //             return;
    //         }
    //     }
    // }

    /// Applies `op` with the picks that the agent made in its fires `fires`, and returns the fires
    /// of the operation, as `fired` reports them. It fails when the plans do not allow a pick of
    /// the agent, or when the agent makes a fire that the plans do not make.
    fn apply(
        &mut self,
        op: &BagOp,
        fires: &[AgentFire],
    ) -> Result<Vec<(String, Vec<Par>)>, TestCaseError> {
        let mut out = Vec::with_capacity(fires.len());
        let mut picks = fires.iter().peekable();
        match (op, op.space_op()) {
            (
                BagOp::Send {
                    value, persistent, ..
                },
                SpaceOp::Produce(send),
            ) => self.offer(
                *value,
                *persistent,
                record_hash(&send),
                &mut picks,
                &mut out,
            )?,
            (
                BagOp::Receive {
                    pat,
                    guard,
                    persistent,
                    peek,
                    ..
                },
                SpaceOp::Consume(receive),
            ) => {
                let waiter = ModelWaiter {
                    name: body_name(&parked_body(&receive)),
                    pat: *pat,
                    guard: *guard,
                    persist: *persistent,
                    peek: *peek,
                    hash: waiter_hash(&receive),
                };
                self.receive(waiter, &mut picks, &mut out)?
            }
            _ => panic!("an operation and its space operation disagree"),
        }
        prop_assert!(
            picks.next().is_none(),
            "the agent makes a fire that the plans do not make"
        );
        Ok(out)
    }

    /// OFFER-SCAN: while a waiter fits the datum and its guard admits it, the next fire of the
    /// agent must fire such a waiter with the datum. The datum fires again while it is not spent,
    /// and it is stored when no waiter takes it.
    fn offer(
        &mut self,
        value: i64,
        persist: bool,
        hash: Blake2b256Hash,
        picks: &mut Picks<'_>,
        out: &mut Vec<(String, Vec<Par>)>,
    ) -> Result<(), TestCaseError> {
        loop {
            let admitting = self
                .waiters
                .iter()
                .any(|waiter| waiter.pat.fits(value) && waiter.guard.admits(value));
            if !admitting {
                self.records.push(ModelRecord {
                    value,
                    persist,
                    hash,
                });
                return Ok(());
            }
            let fire = picks
                .next()
                .ok_or_else(|| failure(format!("a waiter takes {value}, so the datum fires")))?;
            prop_assert_eq!(&fire.hash, &hash, "the fire binds the datum");
            let index = self
                .waiters
                .iter()
                .position(|waiter| waiter.name == fire.name)
                .ok_or_else(|| failure(format!("the fired waiter {} waits", fire.name)))?;
            let waiter = self.waiters[index].clone();
            prop_assert!(
                waiter.pat.fits(value) && waiter.guard.admits(value),
                "the fired waiter {} takes {}",
                waiter.name,
                value
            );
            out.push((waiter.name.clone(), waiter.pat.bound(value)));
            if !waiter.persist {
                self.waiters.remove(index);
            }
            let spent = !persist && !waiter.peek;
            prop_assert!(
                spent || !waiter.persist,
                "the generators never make a divergent pair"
            );
            if spent {
                return Ok(());
            }
        }
    }

    /// RECEIVE-SCAN: the next fire of the agent is the fire of the receive when it names the
    /// receive. Its record fits the pattern, and the guard admits it. A non-persistent record
    /// leaves the store, and a peeked non-persistent record is offered again (Rule E). Without
    /// its fire the receive parks. The plans allow a park when no record fits, or when a record
    /// that fits is refused: the pick can be such a record, and the guard is checked once
    /// (decision 37).
    fn receive(
        &mut self,
        waiter: ModelWaiter,
        picks: &mut Picks<'_>,
        out: &mut Vec<(String, Vec<Par>)>,
    ) -> Result<(), TestCaseError> {
        loop {
            let Some(fire) = picks.next_if(|fire| fire.name == waiter.name) else {
                let fitting: Vec<i64> = self
                    .records
                    .iter()
                    .map(|record| record.value)
                    .filter(|value| waiter.pat.fits(*value))
                    .collect();
                prop_assert!(
                    fitting.is_empty() || fitting.iter().any(|value| !waiter.guard.admits(*value)),
                    "{} parks, so no record fits or a record that fits is refused",
                    waiter.name
                );
                if !self.waiters.iter().any(|parked| parked.hash == waiter.hash) {
                    self.waiters.push(waiter);
                }
                return Ok(());
            };
            let index = self
                .records
                .iter()
                .position(|record| record.hash == fire.hash)
                .ok_or_else(|| failure(format!("{} binds a stored record", waiter.name)))?;
            let record = self.records[index].clone();
            prop_assert!(
                waiter.pat.fits(record.value) && waiter.guard.admits(record.value),
                "{} binds a record that fits and is admitted",
                waiter.name
            );
            out.push((waiter.name.clone(), waiter.pat.bound(record.value)));
            if !record.persist {
                self.records.remove(index);
            }
            if waiter.peek && !record.persist {
                self.offer(record.value, false, record.hash.clone(), picks, out)?;
            }
            let spent = !waiter.persist;
            prop_assert!(
                spent || !(waiter.peek || record.persist),
                "the generators never make a divergent pair"
            );
            if spent {
                return Ok(());
            }
        }
    }

    /// The values of the records with their persist flags, sorted.
    fn values(&self) -> Vec<(i64, bool)> {
        let mut values: Vec<(i64, bool)> = self
            .records
            .iter()
            .map(|record| (record.value, record.persist))
            .collect();
        values.sort();
        values
    }

    /// The names of the waiters, sorted.
    fn waiter_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .waiters
            .iter()
            .map(|waiter| waiter.name.clone())
            .collect();
        names.sort();
        names
    }
}

// ─── Generators ───────────────────────────────────────────────────────────────────────────

/// Which side may be persistent. Persistent sends and persistent receives never meet, so a
/// generated program never makes a divergent pair, and its fire chains stay below the cap.
#[derive(Clone, Copy, Debug)]
enum Mode {
    PersistentSends,
    PersistentReceives,
}

fn arb_pat() -> BoxedStrategy<Pat> {
    prop_oneof![2 => Just(Pat::Any), 1 => (0i64..4).prop_map(Pat::Lit)].boxed()
}

/// A guard for `pat`. A guard over the bound value goes only with [`Pat::Any`].
fn arb_guard(pat: Pat) -> BoxedStrategy<Guard> {
    match pat {
        Pat::Any => prop_oneof![
            4 => Just(Guard::Free),
            1 => Just(Guard::Admit),
            1 => Just(Guard::Refuse),
            3 => (0i64..4).prop_map(Guard::Equals),
        ]
        .boxed(),
        Pat::Lit(_) => prop_oneof![
            4 => Just(Guard::Free),
            1 => Just(Guard::Admit),
            1 => Just(Guard::Refuse),
        ]
        .boxed(),
    }
}

/// One operation of a program of `mode`. The receive names are filled in by
/// [`arb_program`].
fn arb_op(mode: Mode) -> BoxedStrategy<BagOp> {
    let persistent = |weight_true: u32| prop_oneof![3 => Just(false), weight_true => Just(true)];
    let send =
        (0i64..4, 0u64..3, persistent(1)).prop_map(move |(value, salt, persistent)| BagOp::Send {
            value,
            salt,
            persistent: persistent && matches!(mode, Mode::PersistentSends),
        });
    let receive = (arb_pat(), 0u8..4)
        .prop_flat_map(|(pat, kind)| (Just(pat), arb_guard(pat), Just(kind)))
        .prop_map(move |(pat, guard, kind)| BagOp::Receive {
            name: 0,
            pat,
            guard,
            persistent: kind == 1 && matches!(mode, Mode::PersistentReceives),
            peek: kind == 2,
        });
    prop_oneof![3 => send, 2 => receive].boxed()
}

/// A program of up to `max` operations. Every receive has a name of its own, and sends may
/// repeat a record (the salts are few), so the collection is a multiset.
fn arb_program(max: usize) -> BoxedStrategy<Vec<BagOp>> {
    prop_oneof![Just(Mode::PersistentSends), Just(Mode::PersistentReceives)]
        .prop_flat_map(move |mode| vec(arb_op(mode), 1..=max))
        .prop_map(|ops| {
            ops.into_iter()
                .enumerate()
                .map(|(index, op)| match op {
                    BagOp::Receive {
                        pat,
                        guard,
                        persistent,
                        peek,
                        ..
                    } => BagOp::Receive {
                        name: u64::try_from(index).expect("a small index"),
                        pat,
                        guard,
                        persistent,
                        peek,
                    },
                    send => send,
                })
                .collect()
        })
        .boxed()
}

/// A program that builds the scenario of Rule E (decision 45): stored records, waiters with a
/// guard over the bound value that meet a refused record first and park, then peeks that remove
/// a record and send it again, and a few more sends. The second send of a peeked record can
/// then fire a parked waiter.
fn arb_rule_e_program() -> BoxedStrategy<Vec<BagOp>> {
    (
        vec((0i64..3, 0u64..3), 1..5),
        vec((0i64..3, proptest::bool::ANY), 1..5),
        vec(
            prop_oneof![2 => (0i64..3).prop_map(Pat::Lit), 1 => Just(Pat::Any)],
            1..4,
        ),
        vec((0i64..3, 0u64..3), 0..3),
    )
        .prop_map(|(records, waiters, peeks, tail)| {
            let mut ops =
                Vec::with_capacity(records.len() + waiters.len() + peeks.len() + tail.len());
            let mut name = 0u64;
            for (value, salt) in records {
                ops.push(BagOp::Send {
                    value,
                    salt,
                    persistent: false,
                });
            }
            for (value, peek) in waiters {
                ops.push(BagOp::Receive {
                    name,
                    pat: Pat::Any,
                    guard: Guard::Equals(value),
                    persistent: false,
                    peek,
                });
                name += 1;
            }
            for pat in peeks {
                ops.push(BagOp::Receive {
                    name,
                    pat,
                    guard: Guard::Free,
                    persistent: false,
                    peek: true,
                });
                name += 1;
            }
            for (value, salt) in tail {
                ops.push(BagOp::Send {
                    value,
                    salt: salt + 10,
                    persistent: false,
                });
            }
            ops
        })
        .boxed()
}

/// A general program or a program of the Rule E scenario.
fn arb_any_program() -> BoxedStrategy<Vec<BagOp>> {
    prop_oneof![arb_program(12), arb_rule_e_program()].boxed()
}

// ─── The host differential ────────────────────────────────────────────────────────────────

/// The matcher of the host program: the Rholang matcher over the payload of a datum record
/// without its envelope, as the space agent matches (`spaces/matching.rs`). The guard of a
/// parked continuation lies outside the envelope, so the guard check needs no change.
#[derive(Clone, Copy, Debug)]
struct EnvelopeMatcher;

impl Match<BindPattern, ListParWithRandom, TaggedContinuation> for EnvelopeMatcher {
    fn get(&self, pattern: &BindPattern, data: &ListParWithRandom) -> Option<ListParWithRandom> {
        match record::unwrap_datum(data) {
            Ok(record) => Matcher.get(pattern, &record.datum),
            Err(_) => Matcher.get(pattern, data),
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

/// The pins of a [`ChosenMatcher`]: the records that a consume may bind, or `None` for every
/// record, and the waiter that a produce may fire.
struct Pins {
    records: Option<Vec<ListParWithRandom>>,
    waiter: WaiterPin,
}

/// The pins of a host program, shared with its matcher.
type HostPins = Arc<Mutex<Pins>>;

/// The [`EnvelopeMatcher`] under the pins of the pick of the agent (plan v2 section 5, DR-129).
/// Host play picks at random among candidates, so the host program pins each call to the pick
/// that the receipt of the agent names. A consume binds only the pinned records, and a produce
/// fires only the pinned waiter.
///
/// Changed by decision 102 (plan v2 pins): the pins follow the picks of the agent. The text
/// before: "The [`EnvelopeMatcher`] under the pins of the canonical choice (decision 89, DR-129).
/// Host play picks at random among candidates, so the host program pins each call to the choice
/// of the canonical order of decision 41."
#[derive(Clone)]
struct ChosenMatcher {
    pins: HostPins,
}

impl Match<BindPattern, ListParWithRandom, TaggedContinuation> for ChosenMatcher {
    fn get(&self, pattern: &BindPattern, data: &ListParWithRandom) -> Option<ListParWithRandom> {
        let allowed = match &self.pins.lock().expect("the pin lock").records {
            Some(pinned) => pinned.contains(data),
            None => true,
        };
        match allowed {
            true => EnvelopeMatcher.get(pattern, data),
            false => None,
        }
    }

    fn check_commit(
        &self,
        continuation: &TaggedContinuation,
        matched: &[ListParWithRandom],
    ) -> bool {
        let allowed = self
            .pins
            .lock()
            .expect("the pin lock")
            .waiter
            .admits(continuation);
        allowed && EnvelopeMatcher.check_commit(continuation, matched)
    }
}

/// The pins of a host call that has no choice: every record and every waiter.
fn open_pins() -> Pins {
    Pins {
        records: None,
        waiter: WaiterPin::Any,
    }
}

/// Sets the pins of a host program to `records` and `waiter`.
fn set_pins(pins: &HostPins, records: Option<Vec<ListParWithRandom>>, waiter: WaiterPin) {
    *pins.lock().expect("the pin lock") = Pins { records, waiter };
}

// Changed by decision 89 (DR-129): host play picks at random among candidates, so the host of a
// program matches with the pins of the canonical choice (`chosen_host`).
//
// /// A `ChargingRSpace` over a fresh in-memory space with the [`EnvelopeMatcher`], which charges
// /// `budget`.
// async fn envelope_host(
//     budget: &RuntimeBudget,
// ) -> impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation> {
//     let mut stores = InMemoryStoreManager::new();
//     let space = ParRSpace::create(
//         stores.r_space_stores().await.expect("in-memory stores"),
//         Arc::new(Box::new(EnvelopeMatcher)),
//     )
//     .expect("an in-memory space");
//     let host: RhoISpace = Arc::new(Box::new(space));
//     ChargingRSpace::charging_rspace(host, budget.clone())
// }

// Changed by decision 90 (decision 69): the host records its observer calls, because
// `ChargingRSpace` is deleted with the legacy meter (DR-134).
//
// /// A `ChargingRSpace` over a fresh in-memory space with the [`ChosenMatcher`] of `pins`, which
// /// charges `budget`.
// async fn chosen_host(
//     budget: &RuntimeBudget,
//     pins: &HostPins,
// ) -> impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation> {
//     let mut stores = InMemoryStoreManager::new();
//     let space = ParRSpace::create(
//         stores.r_space_stores().await.expect("in-memory stores"),
//         Arc::new(Box::new(ChosenMatcher { pins: pins.clone() })),
//     )
//     .expect("an in-memory space");
//     let host: RhoISpace = Arc::new(Box::new(space));
//     ChargingRSpace::charging_rspace(host, budget.clone())
// }

/// A fresh in-memory space with the [`ChosenMatcher`] of `pins`, whose accounting observer is
/// `recorder`.
async fn chosen_host(recorder: &Arc<Recorder>, pins: &HostPins) -> ParRSpace {
    let mut stores = InMemoryStoreManager::new();
    let space = ParRSpace::create(
        stores.r_space_stores().await.expect("in-memory stores"),
        Arc::new(Box::new(ChosenMatcher { pins: pins.clone() })),
    )
    .expect("an in-memory space");
    observe(&space, recorder);
    space
}

// Changed by decision 102 (plan v2 pins): the host program reads the record of each consume from
// the receipt of the agent ([`receipt_datum`]), so it makes no canonical choice of a record.
//
// /// The stored record that a consume of `receive` binds on `host` under the canonical order of
// /// decision 41: the first record in canonical order whose payload the pattern binds. The guard
// /// is checked once on it (decision 37). `None` means that no record binds.
// async fn record_choice(
//     host: &impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
//     receive: &ConsumeOp,
// ) -> Option<Datum<ListParWithRandom>> {
//     let records = host.get_data(&data()).await;
//     canonical_data_order(&records)
//         .into_iter()
//         .map(|index| &records[index])
//         .find(|stored| EnvelopeMatcher.get(&receive.pattern, &stored.a).is_some())
//         .cloned()
// }

/// Added by decision 102 (plan v2 section 5): what [`compare_with_host`] compared besides the
/// observer calls and the stores.
#[derive(Debug, Default)]
struct Compared {
    /// The persist flag of the record that each fire of a receive binds, in order.
    receive_binds: Vec<bool>,
    /// The fires of each operation of the agent, as `fired` reports them, in order.
    fires: Vec<Vec<(String, Vec<Par>)>>,
}

/// Added by decision 102 (plan v2 section 5): the operations of the agent that the host program
/// has not met yet, each with its observer calls from its start to its finish ([`operations`]).
type Receipts<'a> = std::slice::Iter<'a, Vec<Call>>;

/// Added by decision 102 (plan v2 section 5): the receipt of the next operation of the agent.
fn next_receipt<'a>(receipts: &mut Receipts<'a>) -> Result<&'a [Call], TestCaseError> {
    receipts
        .next()
        .map(Vec::as_slice)
        .ok_or_else(|| failure("the host program makes more operations than the agent"))
}

/// Added by decision 102 (plan v2 section 5): the waiter that the COMM of the operation `receipt`
/// of the agent fires, as a waiter pin. It is `WaiterPin::Nothing` when the operation makes no
/// COMM, because the produce stores its record.
fn receipt_waiter(receipt: &[Call]) -> WaiterPin {
    receipt
        .iter()
        .find_map(|call| match call {
            Call::Comm { continuation, .. } => {
                Some(WaiterPin::Only(Box::new(continuation.clone())))
            }
            _ => None,
        })
        .unwrap_or(WaiterPin::Nothing)
}

/// Added by decision 102 (plan v2 section 5): the stored record and its persist flag that the
/// COMM of the operation `receipt` of the agent binds. It is `None` when the operation makes no
/// COMM. A COMM of a bag binds one record, and play names it as a participant.
fn receipt_datum(receipt: &[Call]) -> Result<Option<(ListParWithRandom, bool)>, TestCaseError> {
    match receipt
        .iter()
        .find(|call| matches!(call, Call::Comm { .. }))
    {
        None => Ok(None),
        Some(Call::Comm {
            participants: Some(participants),
            ..
        }) => match participants.as_slice() {
            [participant] => Ok(Some((participant.stored.clone(), participant.persistent))),
            other => Err(failure(format!("a bag COMM binds one record: {other:?}"))),
        },
        Some(other) => Err(failure(format!(
            "play names the participants of a COMM: {other:?}"
        ))),
    }
}

/// Added by decision 102 (plan v2 section 5): the stored records of `host` that the pattern of
/// `receive` binds and whose bound values its guard refuses. The receive of the agent parked, so
/// its pick was such a record, or no record binds its pattern (decision 37). The function fails
/// when records bind and the guard refuses none of them.
async fn refused_records(
    host: &impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    receive: &ConsumeOp,
) -> Result<Vec<ListParWithRandom>, TestCaseError> {
    let records = host.get_data(&data()).await;
    let mut refused = Vec::with_capacity(records.len());
    let mut bound = 0;
    for stored in &records {
        let payload = record::unwrap_datum(&stored.a)
            .expect("a datum record")
            .datum;
        if let Some(matched) = Matcher.get(&receive.pattern, &payload) {
            bound += 1;
            if !Matcher.check_commit(&parked(receive), std::slice::from_ref(&matched)) {
                refused.push(stored.a.clone());
            }
        }
    }
    prop_assert!(
        bound == 0 || !refused.is_empty(),
        "the receive of the agent parks, so no record binds its pattern or its guard refuses one"
    );
    Ok(refused)
}

/// One call of a host program.
enum HostCall {
    /// A produce of `record` on the data channel.
    Send {
        record: ListParWithRandom,
        persist: bool,
    },
    /// The consume of a receive.
    Receive(Box<ConsumeOp>),
}

/// The fire of a host call as `fired` reports a fire: the body name and the bound values.
fn host_fire(
    continuation: &TaggedContinuation,
    results: &[support::legacy::ParResult],
) -> (String, Vec<Par>) {
    let original = record::unwrap_continuation(continuation).expect("a continuation record");
    (
        body_name(&original.continuation),
        results
            .iter()
            .flat_map(|result| result.matched_datum.pars.clone())
            .collect(),
    )
}

/// Runs the host program of `op` on `host`: the host call of the operation, then the calls of
/// the reducer after each fire. A persistent send produces again, a persistent receive consumes
/// again, and a fire of a peek sends each removed non-persistent record again
/// (`produce_peeks`).
///
/// Each call takes the next receipt of `receipts`: the observer calls of the same operation of
/// the agent ([`operations`]). The program pins the call to the pick that the receipt names (plan
/// v2 section 5, DR-129). It also checks that the plans allow each pick:
///
/// - A send that fires nothing meets no waiter that binds its record and admits it.
/// - A receive that parks meets no record, or a record that its guard refuses.
/// - A fire of a receive binds the record that the receipt names. The host matcher checks the
///   pattern and the guard of the fire, so the host fire fails when the plans refuse the pick.
///
/// The program returns the observer calls of the calls that it keeps, from the recorder
/// `recorder` of the host, and the fires, in order (decision 90, decision 69). It adds the
/// persist flags of the records that the fires of receives bind to `compared`.
///
/// Changed by decision 102 (plan v2 pins): the program follows the receipts of the agent. The
/// text before: "Each call is pinned first to the canonical choice on the host records (decision
/// 89, DR-129)."
// Changed by decision 90 (decision 69): the program returns the observer calls of its host calls
// instead of their legacy log entries.
// async fn host_program(
//     host: &impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
//     budget: &RuntimeBudget,
//     pins: &HostPins,
//     op: &SpaceOp,
// ) -> Result<(Vec<Cost>, Vec<(String, Vec<Par>)>), TestCaseError> {
// Changed by decision 102 (plan v2 pins): the program reads the receipts of the agent.
// async fn host_program(
//     host: &impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
//     budget: &RuntimeBudget,
//     pins: &HostPins,
//     op: &SpaceOp,
// ) -> Result<(Vec<Cost>, Vec<(String, Vec<Par>)>), TestCaseError> {
async fn host_program(
    host: &impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    recorder: &Recorder,
    pins: &HostPins,
    op: &SpaceOp,
    receipts: &mut Receipts<'_>,
    compared: &mut Compared,
) -> Result<(Vec<Call>, Vec<(String, Vec<Par>)>), TestCaseError> {
    let mut calls = VecDeque::from([match op {
        SpaceOp::Produce(send) => HostCall::Send {
            record: record::wrap_datum(0, &send.data),
            persist: send.persistent,
        },
        SpaceOp::Consume(receive) => HostCall::Receive(Box::new(receive.clone())),
        SpaceOp::Settle(_) => panic!("a bag issues no settle"),
        SpaceOp::Join(_) => panic!("the bag programs of this suite issue no join"),
    }]);
    // Changed by decision 90 (decision 69): the program keeps the observer calls of its host calls.
    // let mut entries = Vec::new();
    let mut observed_calls = Vec::new();
    let mut fires = Vec::new();
    while let Some(call) = calls.pop_front() {
        // Added by decision 102 (plan v2 pins): each host call follows the receipt of the same
        // operation of the agent.
        let receipt = next_receipt(receipts)?;
        let (fired, again, reproduced) = match call {
            HostCall::Send { record, persist } => {
                // Changed by decision 102 (plan v2 pins): the produce fires the waiter that the
                // COMM of the agent fires, or no waiter.
                // let waiter = waiter_choice(host, &data(), &record).await;
                // set_pins(pins, None, waiter);
                let waiter = receipt_waiter(receipt);
                if let WaiterPin::Nothing = waiter {
                    prop_assert_eq!(
                        waiter_choice(host, &data(), &record).await,
                        WaiterPin::Nothing,
                        "the agent stores the record, so no waiter binds it and admits it"
                    );
                }
                set_pins(pins, None, waiter);
                // let run = host_produce(host, budget, data(), record.clone(), persist).await;
                // entries.extend(run.entries);
                let run = host_produce(host, recorder, data(), record.clone(), persist).await;
                set_pins(pins, None, WaiterPin::Any);
                observed_calls.extend(run.calls);
                let again = match (&run.fired, persist) {
                    (Some(_), true) => Some(HostCall::Send { record, persist }),
                    _ => None,
                };
                (run.fired, again, run.reproduced)
            }
            // Changed by decision 102 (plan v2 pins): the consume binds the record that the COMM
            // of the agent binds, or it parks as the receive of the agent parks.
            // HostCall::Receive(receive) => {
            //     // Changed by decision 89 (DR-129): the consume binds the canonical record.
            //     // let run = host_consume(
            //     //     host,
            //     //     budget,
            //     //     vec![data()],
            //     //     vec![receive.pattern.clone()],
            //     //     parked(&receive),
            //     //     receive.persistent,
            //     //     receive.peek,
            //     // )
            //     // .await;
            //     let choice = record_choice(host, &receive).await;
            //     let expected: Vec<bool> = choice.iter().map(|stored| stored.persist).collect();
            //     let pinned = choice.into_iter().map(|stored| stored.a).collect();
            //     set_pins(pins, Some(pinned), WaiterPin::Any);
            //     let run = drawn_consume(host, &expected, || {
            //         host_consume(
            //             host,
            //             budget,
            //             vec![data()],
            //             vec![receive.pattern.clone()],
            //             parked(&receive),
            //             receive.persistent,
            //             receive.peek,
            //         )
            //     })
            //     .await
            //     .map_err(TestCaseError::fail)?;
            //     entries.extend(run.entries);
            //     let again = match (&run.fired, receive.persistent) {
            //         (Some(_), true) => Some(HostCall::Receive(receive)),
            //         _ => None,
            //     };
            //     (run.fired, again, run.reproduced)
            // }
            HostCall::Receive(receive) => match receipt_datum(receipt)? {
                None => {
                    // The receive of the agent parks. The host receive meets only records that
                    // its guard refuses.
                    let refused = refused_records(host, &receive).await?;
                    set_pins(pins, Some(refused), WaiterPin::Any);
                    let run = host_consume(
                        host,
                        recorder,
                        vec![data()],
                        vec![receive.pattern.clone()],
                        parked(&receive),
                        receive.persistent,
                        receive.peek,
                    )
                    .await;
                    set_pins(pins, None, WaiterPin::Any);
                    prop_assert!(run.fired.is_none(), "the host receive parks");
                    observed_calls.extend(run.calls);
                    (run.fired, None, run.reproduced)
                }
                Some((bound, persistent)) => {
                    set_pins(pins, Some(vec![bound]), WaiterPin::Any);
                    let run = drawn_consume(host, &[persistent], || {
                        host_consume(
                            host,
                            recorder,
                            vec![data()],
                            vec![receive.pattern.clone()],
                            parked(&receive),
                            receive.persistent,
                            receive.peek,
                        )
                    })
                    .await
                    .map_err(TestCaseError::fail)?;
                    set_pins(pins, None, WaiterPin::Any);
                    prop_assert!(
                        run.fired.is_some(),
                        "the host fire binds the record of the agent"
                    );
                    compared.receive_binds.push(persistent);
                    observed_calls.extend(run.calls);
                    let again = match (&run.fired, receive.persistent) {
                        (Some(_), true) => Some(HostCall::Receive(receive)),
                        _ => None,
                    };
                    (run.fired, again, run.reproduced)
                }
            },
        };
        if let Some((continuation, results)) = &fired {
            fires.push(host_fire(&continuation.continuation, results));
        }
        let mut next: Vec<HostCall> = reproduced
            .into_iter()
            .map(|(_, record)| HostCall::Send {
                record,
                persist: false,
            })
            .collect();
        next.extend(again);
        for call in next.into_iter().rev() {
            calls.push_front(call);
        }
    }
    // Ok((entries, fires))
    Ok((observed_calls, fires))
}

/// The records and the waiting continuations on the data channel of `space`, each as its
/// bincode bytes, sorted. Two stores with equal views hold the same entries.
async fn store_bytes(
    space: &impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
) -> (Vec<Vec<u8>>, Vec<Vec<u8>>) {
    let mut records: Vec<Vec<u8>> = space
        .get_data(&data())
        .await
        .iter()
        .map(|datum| bincode::serialize(datum).expect("a record serializes"))
        .collect();
    records.sort();
    let mut waiters: Vec<Vec<u8>> = space
        .get_waiting_continuations(vec![data()])
        .await
        .iter()
        .map(|waiter| bincode::serialize(waiter).expect("a waiter serializes"))
        .collect();
    waiters.sort();
    (records, waiters)
}

/// The agent and the host program of the operations `ops`, compared operation by operation:
/// the observer calls with their arguments, the fires, and at the end the stores (decision 90,
/// decision 69). Each side records the observer calls of its own host RSpace. The observer calls
/// carry the COMMs, so the two sides make the same COMMs. The host program pins each call to the
/// pick of the agent, which it reads from the receipt of the same operation of the agent (plan v2
/// section 5). So the comparison stays exact under every pick. The function returns what it
/// compared ([`Compared`]). Each row of the space names the footprint group of decision 92, which
/// the host program does not have. So the comparison removes that group from the calls of the
/// agent (`without_footprint`).
///
/// Changed by decision 102 (plan v2 pins): the host program follows the receipts of the agent.
/// The text before: "The host program pins each call to the canonical choice (decision 89,
/// DR-129), so the comparison stays exact."
// Changed by decision 102 (plan v2 pins): the comparison returns what it compared.
// async fn compare_with_host(ops: &[SpaceOp]) -> Result<(), TestCaseError> {
async fn compare_with_host(ops: &[SpaceOp]) -> Result<Compared, TestCaseError> {
    // Changed by decision 90 (decision 69): each side records the observer calls of its host
    // RSpace, because DR-134 deleted the legacy meter and its scopes.
    // let agent_budget = RuntimeBudget::new(Cost::create(LEGACY_PHLO, "rrs bag agent"));
    // let _agent_scope = agent_budget.enter_legacy_scope();
    // let rig = Rig::with_budget(&agent_budget).await;
    // let host_budget = RuntimeBudget::new(Cost::create(LEGACY_PHLO, "rrs bag host"));
    // let _host_scope = host_budget.enter_legacy_scope();
    let rig = Rig::new().await;
    let agent_recorder = observed(&rig.host);
    let host_recorder = Recorder::new();
    // Changed by decision 89 (DR-129): the host matches under the pins of the canonical choice.
    // let host = envelope_host(&host_budget).await;
    let pins: HostPins = Arc::new(Mutex::new(open_pins()));
    // Changed by decision 90 (decision 69): the host records its observer calls.
    // let host = chosen_host(&host_budget, &pins).await;
    let host = chosen_host(&host_recorder, &pins).await;
    // Added by decision 102 (plan v2 pins).
    let mut compared = Compared {
        receive_binds: Vec::new(),
        fires: Vec::with_capacity(ops.len()),
    };
    for (index, op) in ops.iter().enumerate() {
        // let agent_before = agent_budget.get_log().len();
        let agent_before = agent_recorder.count();
        let outcome = rig
            .run(op.clone())
            .await
            .map_err(|error| TestCaseError::fail(format!("operation {index} fails: {error}")))?;
        // let agent_entries = agent_budget.get_log()[agent_before..].to_vec();
        // Changed by decision 92: each row of the space names the footprint group, which the
        // host program does not have.
        let agent_calls = without_footprint(
            &agent_recorder.since(agent_before),
            &space_counter_channel(SPACE),
        );
        // Changed by decision 89 (DR-129): a reverted draw leaves its charges in the log of the
        // budget, so the host program returns the entries of the calls that it keeps.
        // let host_before = host_budget.get_log().len();
        // let host_fires = host_program(&host, &host_budget, op).await;
        // let host_entries = host_budget.get_log()[host_before..].to_vec();
        // Changed by decision 90 (decision 69): the program returns the observer calls of the
        // calls that it keeps.
        // let (host_entries, host_fires) = host_program(&host, &host_budget, &pins, op).await?;
        // prop_assert!(!agent_entries.is_empty(), "operation {} charges", index);
        // prop_assert_eq!(
        //     &agent_entries,
        //     &host_entries,
        //     "the legacy charges of operation {}: {:?}",
        //     index,
        //     op
        // );
        // Changed by decision 102 (plan v2 pins): the host program follows the receipts of the
        // agent.
        let receipts = operations(&agent_calls);
        let mut unread = receipts.iter();
        let (host_calls, host_fires) =
            host_program(&host, &host_recorder, &pins, op, &mut unread, &mut compared).await?;
        prop_assert!(
            unread.next().is_none(),
            "the host program makes every operation of the agent: {:?}",
            op
        );
        prop_assert!(
            !agent_calls.is_empty(),
            "operation {} makes observer calls",
            index
        );
        prop_assert_eq!(
            &agent_calls,
            &host_calls,
            "the observer calls of operation {}: {:?}",
            index,
            op
        );
        // Changed by decision 102 (plan v2 pins): the comparison keeps the fires of the agent.
        // prop_assert_eq!(
        //     fired(&outcome),
        //     host_fires,
        //     "the fires of operation {}",
        //     index
        // );
        let agent_fires = fired(&outcome);
        prop_assert_eq!(
            &agent_fires,
            &host_fires,
            "the fires of operation {}",
            index
        );
        prop_assert_eq!(&outcome.reissue, &None, "operation {} re-issues", index);
        compared.fires.push(agent_fires);
    }
    prop_assert_eq!(store_bytes(&rig.host).await, store_bytes(&host).await);
    // Changed by decision 102 (plan v2 pins): the comparison returns what it compared.
    // Ok(())
    Ok(compared)
}

// ─── Example tests ────────────────────────────────────────────────────────────────────────

/// Two equal sends store two equal records (the hot store keeps duplicates), and each receive
/// takes exactly one of them (F3, F4).
#[tokio::test]
async fn space_disciplines_bag_duplicate_multiplicity() {
    let rig = Rig::new().await;
    let twin = send_on(SPACE, &channel(), 5, false);
    rig.send(twin.clone()).await;
    rig.send(twin.clone()).await;
    rig.send(salted(6, 1, false)).await;
    let log = rig.log().await;
    assert_eq!(labels(&log), vec!["produce D", "produce D", "produce D"]);
    let stored = produces(&log);
    assert_eq!(
        stored[0], stored[1],
        "the two equal sends have one identity"
    );
    assert_eq!(rig.values().await, vec![(5, false), (5, false), (6, false)]);
    assert_eq!(fired(&rig.receive(receiving("a", exactly(5))).await), vec![
        delivery("a", &[])
    ]);
    assert_eq!(
        rig.values().await,
        vec![(5, false), (6, false)],
        "the removal of one of two equal records keeps the other"
    );
    assert_eq!(fired(&rig.receive(receiving("b", exactly(5))).await), vec![
        delivery("b", &[])
    ]);
    assert_eq!(rig.values().await, vec![(6, false)]);
    assert!(rig
        .receive(receiving("c", exactly(5)))
        .await
        .fires
        .is_empty());
    assert_eq!(rig.waiters().await, vec!["c".to_string()]);
}

/// The choices of the agent do not depend on the list order of the store (F1, F15). Play picks at
/// random among the candidates of each search (decision 102, rule CR-1). Each run on a fresh rig
/// stores the records 3, 1 and 2, and the receives `a`, `b` and `c` take each record once. Each
/// run also parks the waiters `x`, `y` and `z`, and the sends 7, 8 and 9 fire each waiter once.
/// The play store lists its entries newest first. Over [`PICK_RUNS`] runs, `a` takes each of the
/// three records, and the send 7 fires each of the three waiters. One of the three outcomes of
/// `a` is missing with a chance of `3·(2/3)^64`, which is less than `2·10^-11`, and the same
/// bound holds for the send 7. The unit test
/// `space_pick_helpers_shuffle_in_play_and_keep_dr75_in_the_restage` of `spaces/generic.rs`
/// checks that the restage of R-A keeps the DR-75 order, and
/// `space_pick_helpers_put_the_named_candidates_first_in_replay` that replay takes the recorded
/// candidates first. The replay half of this test retired with the rigged replay modes (decision
/// 90, C6, step D).
///
/// Changed by decision 102 (plan v2 pins): play picks at random, so the claim on the list order
/// is a claim on the replay order. The text before: "A receive takes the first record in the
/// canonical order of DR-75, and a send fires the first waiter in canonical order: by the source
/// hash, whatever the list order of the store (decision 41, F1, F15)."
#[tokio::test]
async fn space_disciplines_bag_first_match_canonical_order() {
    // Changed by decision 102 (plan v2 pins): play picks at random, so the test checks the
    // allowed outcomes, their coverage and the replay over another list order. The body before:
    // let rig = Rig::new().await;
    // let sends = ordered_sends(&[(3, false), (1, false), (2, false)]);
    // for send in &sends {
    //     rig.send(send.clone()).await;
    // }
    // let listed: Vec<i64> = rig
    //     .host
    //     .get_data(&data())
    //     .await
    //     .iter()
    //     .map(|datum| value_of(&record::unwrap_datum(&datum.a).expect("a record").datum.pars))
    //     .collect();
    // assert_eq!(
    //     listed,
    //     vec![2, 1, 3],
    //     "the store lists the records newest first, not in canonical order"
    // );
    // let mut delivered = Vec::new();
    // for name in ["a", "b", "c"] {
    //     delivered.extend(fired(&rig.receive(receiving(name, any())).await));
    // }
    // assert_eq!(delivered, vec![
    //     delivery("a", &[3]),
    //     delivery("b", &[1]),
    //     delivery("c", &[2])
    // ]);
    //
    // let rig = Rig::new().await;
    // let receives = ordered_receives(vec![
    //     receiving("x", any()),
    //     receiving("y", any()),
    //     receiving("z", any()),
    // ]);
    // for receive in &receives {
    //     rig.receive(receive.clone()).await;
    // }
    // assert_eq!(
    //     rig.host
    //         .get_waiting_continuations(vec![data()])
    //         .await
    //         .iter()
    //         .map(|waiter| body_name(
    //             &record::unwrap_continuation(&waiter.continuation)
    //                 .expect("a continuation record")
    //                 .continuation
    //         ))
    //         .collect::<Vec<String>>(),
    //     vec!["z".to_string(), "y".to_string(), "x".to_string()],
    //     "the store lists the waiters newest first, not in canonical order"
    // );
    // let mut delivered = Vec::new();
    // for value in [7, 8, 9] {
    //     delivered.extend(fired(&rig.send(salted(value, 1, false)).await));
    // }
    // assert_eq!(delivered, vec![
    //     delivery("x", &[7]),
    //     delivery("y", &[8]),
    //     delivery("z", &[9])
    // ]);
    let sends = ordered_sends(&[(3, false), (1, false), (2, false)]);
    let stores: Vec<SpaceOp> = sends.iter().cloned().map(SpaceOp::Produce).collect();
    // Retired by decision 90 (C6, step D): the replay half ran through the rigged replay modes,
    // which retire (decision 125). `space_pick_helpers_put_the_named_candidates_first_in_replay`,
    // `space_native_replay_equal_hashmap`.
    let takes: Vec<SpaceOp> = ["a", "b", "c"]
        .into_iter()
        .map(|name| SpaceOp::Consume(receiving(name, any())))
        .collect();
    let newest_records: Vec<Blake2b256Hash> = sends.iter().rev().map(record_hash).collect();
    let receives = ordered_receives(vec![
        receiving("x", any()),
        receiving("y", any()),
        receiving("z", any()),
    ]);
    let parks: Vec<SpaceOp> = receives.iter().cloned().map(SpaceOp::Consume).collect();
    // Retired by decision 90 (C6, step D): the replay half ran through the rigged replay modes,
    // which retire (decision 125). `space_pick_helpers_put_the_named_candidates_first_in_replay`,
    // `space_native_replay_equal_hashmap`.
    let offers: Vec<SpaceOp> = [7, 8, 9]
        .into_iter()
        .map(|value| SpaceOp::Produce(salted(value, 1, false)))
        .collect();
    let newest_waiters: Vec<Blake2b256Hash> = receives.iter().rev().map(waiter_hash).collect();
    let mut first_records: BTreeSet<i64> = BTreeSet::new();
    let mut first_waiters: BTreeSet<String> = BTreeSet::new();
    for _ in 0..PICK_RUNS {
        let played = played_order(&stores, &takes)
            .await
            .expect("the play of the records");
        assert_eq!(
            played.listed.records, newest_records,
            "the play store lists the records newest first"
        );
        let delivered: Vec<(String, Vec<Par>)> = played.outcomes.iter().flat_map(fired).collect();
        let names: Vec<&str> = delivered.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, vec!["a", "b", "c"], "each receive takes one record");
        let mut values: Vec<i64> = delivered
            .iter()
            .map(|(_, values)| value_of(values))
            .collect();
        first_records.insert(values[0]);
        values.sort();
        assert_eq!(values, vec![1, 2, 3], "the receives take each record once");
        assert!(played.rig.values().await.is_empty());
        // Retired by decision 90 (C6, step D): the replay half ran through the rigged replay modes,
        // which retire (decision 125).
        // `space_pick_helpers_put_the_named_candidates_first_in_replay`,
        // `space_native_replay_equal_hashmap`.

        let played = played_order(&parks, &offers)
            .await
            .expect("the play of the waiters");
        assert_eq!(
            played.listed.waiters, newest_waiters,
            "the play store lists the waiters newest first"
        );
        let delivered: Vec<(String, Vec<Par>)> = played.outcomes.iter().flat_map(fired).collect();
        let values: Vec<i64> = delivered
            .iter()
            .map(|(_, values)| value_of(values))
            .collect();
        assert_eq!(values, vec![7, 8, 9], "each send fires one waiter");
        let mut names: Vec<String> = delivered.into_iter().map(|(name, _)| name).collect();
        first_waiters.insert(names[0].clone());
        names.sort();
        assert_eq!(
            names,
            vec!["x", "y", "z"],
            "the sends fire each waiter once"
        );
        assert!(played.rig.waiters().await.is_empty());
        // Retired by decision 90 (C6, step D): the replay half ran through the rigged replay modes,
        // which retire (decision 125).
        // `space_pick_helpers_put_the_named_candidates_first_in_replay`,
        // `space_native_replay_equal_hashmap`.
    }
    assert_eq!(
        first_records,
        BTreeSet::from([1, 2, 3]),
        "the receive a takes each record in some run"
    );
    assert_eq!(
        first_waiters,
        BTreeSet::from(["x".to_string(), "y".to_string(), "z".to_string()]),
        "the send 7 fires each waiter in some run"
    );
}

/// A receive checks its guard once, on its pick. A refusal parks it, although another record
/// would pass the guard (decision 37, F2). Play picks the record 1 or the record 2 at random, each
/// with a chance of 1/2 (decision 102, rule CR-1). A pick of 2 fires `w`, which is the other
/// allowed outcome. So the test tries fresh rigs until the pick is 1
/// ([`parked_by_a_refused_pick`]). All [`PARK_TRIES`] tries pick 2 with a chance of `2^-128`.
///
/// Changed by decision 102 (plan v2 pins): play picks at random. The text before: "A receive
/// checks its guard once, on its first spatial match. A refusal parks it, although a later record
/// would pass the guard (decision 37, F2)."
#[tokio::test]
async fn space_disciplines_bag_guard_checked_once_on_first_spatial_match() {
    // Changed by decision 102 (plan v2 pins): the pick of 1 is one of two allowed outcomes, so
    // the setup tries fresh rigs until the receive parks. The setup before:
    // let rig = Rig::new().await;
    // let sends = ordered_sends(&[(1, false), (2, false)]);
    // for send in &sends {
    //     rig.send(send.clone()).await;
    // }
    // let parked = rig.receive(guarded(receiving("w", any()), equals(2))).await;
    // assert!(
    //     parked.fires.is_empty(),
    //     "the guard refuses the first match 1"
    // );
    let sends = ordered_sends(&[(1, false), (2, false)]);
    let rig = parked_by_a_refused_pick(
        &sends,
        &guarded(receiving("w", any()), equals(2)),
        &delivery("w", &[2]),
    )
    .await;
    assert_eq!(rig.waiters().await, vec!["w".to_string()]);
    assert_eq!(rig.values().await, vec![(1, false), (2, false)]);
    assert_eq!(
        fired(&rig.receive(receiving("v", exactly(2))).await),
        vec![delivery("v", &[])],
        "negative control: the record 2 matches a receive that meets it first"
    );
    assert_eq!(rig.values().await, vec![(1, false)]);
}

/// A send skips a waiter whose guard refuses and fires the next waiter in canonical order
/// (decision 37, the produce side).
#[tokio::test]
async fn space_disciplines_bag_produce_guard_refusal_moves_to_next_waiter() {
    let rig = Rig::new().await;
    let receives = ordered_receives(vec![
        guarded(receiving("refuses", any()), equals(7)),
        receiving("takes", any()),
    ]);
    for receive in &receives {
        rig.receive(receive.clone()).await;
    }
    assert_eq!(fired(&rig.send(salted(5, 1, false)).await), vec![delivery(
        "takes",
        &[5]
    )]);
    assert_eq!(rig.waiters().await, vec!["refuses".to_string()]);
    assert_eq!(fired(&rig.send(salted(7, 2, false)).await), vec![delivery(
        "refuses",
        &[7]
    )]);
    assert!(rig.waiters().await.is_empty());
    assert!(rig.values().await.is_empty());
}

/// A peek fire removes the record, as the host COMM does, and the same operation sends the
/// record again with its identity and its own authority (decision 45, Rule E). A persistent
/// record stays, so nothing is sent again.
#[tokio::test]
async fn space_disciplines_bag_peek_restores_record() {
    let budget = budget();
    let _scope = budget.enter_comm_accounting_scope();
    let rig = Rig::with_budget(&budget).await;
    let mut sending = salted(5, 1, false);
    sending.authority = region(2);
    sending.data.cost_authority = Some(region(2));
    rig.send(sending).await;
    let stored = produces(&rig.log().await);
    let peek = ConsumeOp {
        introduction_authority: region(4),
        ..peeking(receiving("p", any()))
    };
    let outcome = rig.receive(peek).await;
    assert_eq!(fired(&outcome), vec![delivery("p", &[5])]);
    assert!(
        !outcome.fires[0].results[0].persistent,
        "the flag of the host"
    );
    assert_eq!(outcome.reissue, None);
    let log = rig.log().await;
    assert_eq!(labels(&log), vec!["consume D", "comm D peek", "produce D"]);
    assert_eq!(
        produces(&log),
        stored,
        "the restore has the identity of the record"
    );
    assert_eq!(
        registered(&budget, &log[2]),
        Some(region(2)),
        "the restore registers the authority of the record, not of the peek"
    );
    assert_eq!(registered(&budget, &log[0]), Some(region(4)));
    assert_eq!(rig.values().await, vec![(5, false)]);

    let rig = Rig::new().await;
    rig.send(salted(6, 1, true)).await;
    rig.log().await;
    let outcome = rig.receive(peeking(receiving("q", any()))).await;
    assert!(outcome.fires[0].results[0].persistent);
    assert_eq!(labels(&rig.log().await), vec!["consume D", "comm D peek"]);
    assert_eq!(rig.values().await, vec![(6, true)]);
}

/// The restore of a peeked record is the host's second send: it is offered to the waiters
/// first, so a guarded waiter that admits the record fires (decision 45, Rule E). The waiter `w`
/// must park first. Play picks the record 1 or the record 2 for `w` at random, each with a chance
/// of 1/2 (decision 102, rule CR-1), and only a pick of 1 parks `w`. So the test tries fresh rigs
/// until `w` parks ([`parked_by_a_refused_pick`]). All [`PARK_TRIES`] tries pick 2 with a chance
/// of `2^-128`. The steps after the park have one allowed outcome each.
#[tokio::test]
async fn space_disciplines_bag_peek_restore_fires_guarded_waiter() {
    // Changed by decision 102 (plan v2 pins): the park of `w` is one of two allowed outcomes, so
    // the setup tries fresh rigs until `w` parks. The setup before:
    // let rig = Rig::new().await;
    // let sends = ordered_sends(&[(1, false), (2, false)]);
    // for send in &sends {
    //     rig.send(send.clone()).await;
    // }
    // rig.receive(guarded(receiving("w", any()), equals(2))).await;
    let sends = ordered_sends(&[(1, false), (2, false)]);
    let rig = parked_by_a_refused_pick(
        &sends,
        &guarded(receiving("w", any()), equals(2)),
        &delivery("w", &[2]),
    )
    .await;
    assert_eq!(
        rig.waiters().await,
        vec!["w".to_string()],
        "w meets 1 first"
    );
    rig.log().await;
    let outcome = rig.receive(peeking(receiving("p", exactly(2)))).await;
    assert_eq!(fired(&outcome), vec![
        delivery("p", &[]),
        delivery("w", &[2])
    ]);
    assert_eq!(outcome.reissue, None);
    let log = rig.log().await;
    assert_eq!(labels(&log), vec![
        "consume D",
        "comm D peek",
        "produce D",
        "comm D"
    ]);
    assert_eq!(
        produces(&log)[0].hash,
        record_hash(&sends[1]),
        "the second send has the identity of the record"
    );
    assert_eq!(rig.values().await, vec![(1, false)]);
    assert!(rig.waiters().await.is_empty());
}

/// A send fires a peek waiter and keeps its datum in flight: the datum goes to the next waiter,
/// and it is stored when no waiter takes it (decision 30). Play picks the first waiter of the
/// send 3 at random among `p` and `t`, each with a chance of 1/2 (decision 102, rule CR-1). A
/// pick of `t` spends the datum, and `p` stays, which is the other allowed outcome. So the test
/// tries fresh rigs until the pick is `p`. All [`PARK_TRIES`] tries pick `t` with a chance of
/// `2^-128`. The steps after that pick have one allowed outcome each.
#[tokio::test]
async fn space_disciplines_bag_peek_waiter_reoffers_the_datum() {
    // Changed by decision 102 (plan v2 pins): the pick of `p` is one of two allowed outcomes, so
    // the setup tries fresh rigs until the send picks `p` first. The setup before:
    // let rig = Rig::new().await;
    // let receives = ordered_receives(vec![peeking(receiving("p", any())), receiving("t", any())]);
    // for receive in &receives {
    //     rig.receive(receive.clone()).await;
    // }
    // rig.log().await;
    // let outcome = rig.send(salted(3, 1, false)).await;
    let receives = ordered_receives(vec![peeking(receiving("p", any())), receiving("t", any())]);
    let mut picked = None;
    for _ in 0..PARK_TRIES {
        let rig = Rig::new().await;
        for receive in &receives {
            rig.receive(receive.clone()).await;
        }
        rig.log().await;
        let outcome = rig.send(salted(3, 1, false)).await;
        let fires = fired(&outcome);
        match fires.as_slice() {
            [(first, _), _] if first.as_str() == "p" => {
                picked = Some((rig, outcome));
                break;
            }
            [only] => {
                assert_eq!(only, &delivery("t", &[3]), "the pick t spends the datum");
                assert_eq!(rig.waiters().await, vec!["p".to_string()]);
                assert!(rig.values().await.is_empty());
            }
            more => panic!("the send fires p and then t, or t only: {more:?}"),
        }
    }
    let Some((rig, outcome)) = picked else {
        panic!("no try of {PARK_TRIES} picks the peek waiter p first");
    };
    assert_eq!(fired(&outcome), vec![
        delivery("p", &[3]),
        delivery("t", &[3])
    ]);
    assert!(!outcome.fires[0].results[0].persistent);
    let log = rig.log().await;
    assert_eq!(labels(&log), vec![
        "produce D",
        "comm D peek",
        "produce D",
        "comm D"
    ]);
    assert_eq!(log[0], log[2], "the re-offer has the identity of the send");
    assert!(rig.values().await.is_empty());
    assert!(rig.waiters().await.is_empty());

    rig.receive(peeking(receiving("q", any()))).await;
    rig.log().await;
    assert_eq!(fired(&rig.send(salted(4, 1, false)).await), vec![delivery(
        "q",
        &[4]
    )]);
    assert_eq!(labels(&rig.log().await), vec![
        "produce D",
        "comm D peek",
        "produce D"
    ]);
    assert_eq!(rig.values().await, vec![(4, false)]);
}

/// A persistent send fires every waiter that takes it, once each, and is then stored. Play picks
/// the order of the fires at random (decision 102, rule CR-1), so each run on a fresh rig fires
/// `w1` and `w2` in one of the two orders. Over [`PICK_RUNS`] runs both orders occur. One of the
/// two orders is missing with a chance of `2^-63`.
///
/// Changed by decision 102 (plan v2 pins): play picks at random. The text before: "A persistent
/// send fires every waiter that takes it, in canonical order, and is then stored."
#[tokio::test]
async fn space_disciplines_bag_persistent_send_fires_each_waiter_then_stores() {
    // Changed by decision 102 (plan v2 pins): the order of the fires is one of two allowed
    // outcomes, so the test runs PICK_RUNS times and checks both orders. The body before:
    // let rig = Rig::new().await;
    // let receives = ordered_receives(vec![receiving("w1", any()), receiving("w2", any())]);
    // for receive in &receives {
    //     rig.receive(receive.clone()).await;
    // }
    // rig.log().await;
    // let outcome = rig.send(salted(4, 1, true)).await;
    // assert_eq!(fired(&outcome), vec![
    //     delivery("w1", &[4]),
    //     delivery("w2", &[4])
    // ]);
    // assert_eq!(outcome.reissue, None);
    // assert_eq!(labels(&rig.log().await), vec![
    //     "produce D",
    //     "comm D",
    //     "produce D",
    //     "comm D",
    //     "produce D"
    // ]);
    // assert_eq!(rig.values().await, vec![(4, true)]);
    // assert!(rig.waiters().await.is_empty());
    let receives = ordered_receives(vec![receiving("w1", any()), receiving("w2", any())]);
    let mut orders: BTreeSet<Vec<String>> = BTreeSet::new();
    for _ in 0..PICK_RUNS {
        let rig = Rig::new().await;
        for receive in &receives {
            rig.receive(receive.clone()).await;
        }
        rig.log().await;
        let outcome = rig.send(salted(4, 1, true)).await;
        let mut fires = fired(&outcome);
        orders.insert(fires.iter().map(|(name, _)| name.clone()).collect());
        fires.sort_by(|left, right| left.0.cmp(&right.0));
        assert_eq!(fires, vec![delivery("w1", &[4]), delivery("w2", &[4])]);
        assert_eq!(outcome.reissue, None);
        assert_eq!(labels(&rig.log().await), vec![
            "produce D",
            "comm D",
            "produce D",
            "comm D",
            "produce D"
        ]);
        assert_eq!(rig.values().await, vec![(4, true)]);
        assert!(rig.waiters().await.is_empty());
    }
    assert_eq!(
        orders,
        BTreeSet::from([vec!["w1".to_string(), "w2".to_string()], vec![
            "w2".to_string(),
            "w1".to_string()
        ],]),
        "the send fires the waiters in both orders"
    );
}

/// A persistent receive drains every record that it binds, in canonical order, and then parks.
#[tokio::test]
async fn space_disciplines_bag_persistent_receive_drains_then_parks() {
    let rig = Rig::new().await;
    for send in ordered_sends(&[(1, false), (2, false)]) {
        rig.send(send).await;
    }
    rig.send(salted(9, 1, false)).await;
    rig.log().await;
    let outcome = rig.receive(persisting(receiving("r", any()))).await;
    let mut delivered: Vec<i64> = fired(&outcome)
        .iter()
        .map(|(_, values)| value_of(values))
        .collect();
    assert_eq!(delivered.len(), 3);
    delivered.sort();
    assert_eq!(delivered, vec![1, 2, 9]);
    assert_eq!(outcome.reissue, None);
    assert_eq!(labels(&rig.log().await), vec![
        "consume D",
        "comm D",
        "consume D",
        "comm D",
        "consume D",
        "comm D",
        "consume D"
    ]);
    assert!(rig.values().await.is_empty());
    assert_eq!(rig.waiters().await, vec!["r".to_string()]);
    assert_eq!(fired(&rig.send(salted(3, 1, false)).await), vec![delivery(
        "r",
        &[3]
    )]);
    assert_eq!(rig.waiters().await, vec!["r".to_string()]);
    assert!(rig.values().await.is_empty());
}

/// At `FIRE_ROUND_CAP` fires, and after one round of a divergent pair, the operation commits
/// its fires and re-issues the rest with no carried order (A10, I-21, decision 43).
#[tokio::test]
async fn space_disciplines_bag_round_cap_and_divergent_pair_reissue() {
    let rig = Rig::new().await;
    for index in 0..=FIRE_ROUND_CAP {
        rig.receive(receiving(&format!("w{index}"), any())).await;
    }
    rig.log().await;
    let forever = salted(1, 1, true);
    let outcome = rig.send(forever.clone()).await;
    assert_eq!(outcome.fires.len(), FIRE_ROUND_CAP);
    assert_eq!(
        outcome.reissue,
        Some(SpaceReissue {
            op: SpaceOp::Produce(ProduceOp {
                reissued_ord: None,
                ..forever.clone()
            }),
            cause: ReissueCause::RoundCap,
        })
    );
    assert_eq!(rig.log().await.len(), 2 * FIRE_ROUND_CAP);
    assert!(rig.values().await.is_empty(), "the datum is in flight");
    assert_eq!(rig.waiters().await.len(), 1);
    let rest = rig
        .run(outcome.reissue.expect("the cap re-issues").op)
        .await
        .expect("the re-issue commits");
    assert_eq!(rest.fires.len(), 1);
    assert_eq!(rest.reissue, None);
    assert_eq!(rig.values().await, vec![(1, true)]);
    assert!(rig.waiters().await.is_empty());

    let rig = Rig::new().await;
    rig.receive(persisting(receiving("r", any()))).await;
    let outcome = rig.send(salted(9, 1, true)).await;
    assert_eq!(fired(&outcome), vec![delivery("r", &[9])]);
    assert_eq!(
        outcome.reissue.as_ref().map(|reissue| reissue.cause),
        Some(ReissueCause::DivergentPair)
    );
    assert!(rig.values().await.is_empty());
    assert_eq!(rig.waiters().await, vec!["r".to_string()]);

    let rig = Rig::new().await;
    rig.send(salted(8, 1, true)).await;
    let receive = persisting(receiving("s", any()));
    let outcome = rig.receive(receive.clone()).await;
    assert_eq!(fired(&outcome), vec![delivery("s", &[8])]);
    assert_eq!(
        outcome.reissue,
        Some(SpaceReissue {
            op: SpaceOp::Consume(receive),
            cause: ReissueCause::DivergentPair,
        })
    );
    assert_eq!(rig.values().await, vec![(8, true)]);
    assert!(rig.waiters().await.is_empty(), "the receive is in flight");
}

/// A re-issued persistent operation keeps the record and the identity of its first round,
/// because every `ord` of a bag is 0 (decision 43).
#[tokio::test]
async fn space_disciplines_bag_persistent_reissue_keeps_identity() {
    let rig = Rig::new().await;
    for index in 0..=FIRE_ROUND_CAP {
        rig.receive(receiving(&format!("w{index}"), any())).await;
    }
    rig.log().await;
    let forever = salted(1, 1, true);
    let first = rig.send(forever.clone()).await;
    let round_one = produces(&rig.log().await);
    assert_eq!(round_one.len(), FIRE_ROUND_CAP);
    rig.run(first.reissue.expect("the cap re-issues").op)
        .await
        .expect("the re-issue commits");
    let round_two = produces(&rig.log().await);
    assert_eq!(round_two.len(), 2, "the fire and the store");
    assert!(
        round_one
            .iter()
            .chain(&round_two)
            .all(|produce| produce.hash == record_hash(&forever)),
        "every round stages the same record, so the same identity"
    );

    let rig = Rig::new().await;
    rig.receive(persisting(receiving("r", any()))).await;
    rig.log().await;
    let mut next = Some(SpaceOp::Produce(salted(9, 1, true)));
    let mut rounds = Vec::new();
    for _ in 0..3 {
        let op = next.take().expect("a divergent pair re-issues");
        let outcome = rig.run(op).await.expect("the round commits");
        next = outcome.reissue.map(|reissue| reissue.op);
        rounds.push(comms(&rig.log().await));
    }
    assert!(
        rounds
            .iter()
            .all(|round| round.len() == 1 && round[0] == rounds[0][0]),
        "the rounds of a divergent pair repeat one COMM"
    );
}

/// The restore of a peek at the round cap re-issues the produce of the payload of the record,
/// which has the identity of the record (decision 45, Rule E, decision 43). The guarded peek
/// waiters must park first. Play picks the record 1 or the record 2 for each of them at random,
/// each with a chance of 1/2 (decision 102, rule CR-1), and only a pick of 1 parks. So each
/// receive retries from a checkpoint until it parks ([`parked_from_a_checkpoint`]). One of the 64
/// receives fails all [`PARK_TRIES`] tries with a chance of `64·2^-128 = 2^-122` or less. The
/// steps after the parks have one allowed outcome each.
#[tokio::test]
async fn space_disciplines_bag_peek_restore_reissue_keeps_identity() {
    let rig = Rig::new().await;
    let sends = ordered_sends(&[(1, false), (2, false)]);
    for send in &sends {
        rig.send(send.clone()).await;
    }
    for index in 0..FIRE_ROUND_CAP {
        // Changed by decision 102 (plan v2 pins): the park is one of two allowed outcomes, so the
        // receive retries from a checkpoint until it parks. The receive before:
        // rig.receive(peeking(guarded(
        //     receiving(&format!("p{index}"), any()),
        //     equals(2),
        // )))
        // .await;
        parked_from_a_checkpoint(
            &rig,
            &peeking(guarded(receiving(&format!("p{index}"), any()), equals(2))),
        )
        .await;
    }
    assert_eq!(
        rig.waiters().await.len(),
        FIRE_ROUND_CAP,
        "each meets 1 first"
    );
    rig.log().await;
    let outcome = rig.receive(peeking(receiving("peek", exactly(2)))).await;
    assert_eq!(
        outcome.fires.len(),
        FIRE_ROUND_CAP,
        "the peek and the restore share one cap"
    );
    assert_eq!(
        outcome.reissue,
        Some(SpaceReissue {
            op: SpaceOp::Produce(sends[1].clone()),
            cause: ReissueCause::RoundCap,
        }),
        "the restore re-issues the produce of the payload, not persistent, with no order"
    );
    let round_one = produces(&rig.log().await);
    assert_eq!(
        rig.values().await,
        vec![(1, false)],
        "the record is in flight"
    );
    let rest = rig
        .run(outcome.reissue.expect("the cap re-issues").op)
        .await
        .expect("the re-issue commits");
    assert_eq!(rest.fires.len(), 1);
    assert_eq!(rest.reissue, None);
    let round_two = produces(&rig.log().await);
    assert!(
        round_one
            .iter()
            .chain(&round_two)
            .all(|produce| produce.hash == record_hash(&sends[1])),
        "every send of the restored record has its identity"
    );
    assert_eq!(rig.values().await, vec![(1, false), (2, false)]);
    assert!(rig.waiters().await.is_empty());
}

/// A bag has no counter: no event touches a counter, the collection has no counter record, the
/// space counter stays 0, and every entry has the `ord` 0. The peek binds only the record 1, so
/// each step has one allowed outcome under the random picks of play (decision 102, rule CR-1).
#[tokio::test]
async fn space_disciplines_bag_no_counter_events() {
    let rig = Rig::new().await;
    for send in ordered_sends(&[(1, false), (2, true)]) {
        rig.send(send).await;
    }
    // Changed by decision 102 (plan v2 pins): a peek over `any()` picks 1 or the persistent 2 at
    // random, and only a peek of 1 restores a record. The peek before:
    // rig.receive(peeking(receiving("p", any()))).await;
    rig.receive(peeking(receiving("p", exactly(1)))).await;
    rig.receive(receiving("r", exactly(1))).await;
    rig.receive(receiving("w", exactly(3))).await;
    let log = rig.log().await;
    // Two stores, the peek fire and its restore, one fire, and one park.
    assert_eq!(log.len(), 2 + 3 + 2 + 1);
    assert!(
        labels(&log).iter().all(|label| label.contains(" D")),
        "every event is on the data channel: {:?}",
        labels(&log)
    );
    assert!(counter(&rig.host, &counter_channel(SPACE, &channel()))
        .await
        .is_empty());
    assert_eq!(
        counter(&rig.host, &space_counter_channel(SPACE)).await,
        vec![0]
    );
    assert!(records_on(&rig.host, &data())
        .await
        .iter()
        .all(|(ord, _, _)| *ord == 0));
    assert_eq!(rig.values().await.len(), 1);
    assert_eq!(rig.waiters().await, vec!["w".to_string()]);
}

/// After a checkpoint and a reset, the store holds the records of the history. For each of the
/// four records, a receive that only that record matches makes the same fire after the reset as
/// before it, and it reaches the same root. A receive over `any()` then picks one of the four
/// records at random (decision 102, rule CR-1), and it reaches the root of the removal of its
/// pick. Each step has one allowed outcome or checks membership, so the test has no miss
/// chance. The claim that the choice does not depend on the list order is a claim on the replay
/// order (rule CR-4), which `space_disciplines_bag_first_match_canonical_order` checks.
///
/// Changed by decision 102 (plan v2 pins): play picks at random, so two receives over `any()` can
/// make two choices. The text before: "After a checkpoint and a reset, the store holds the
/// records in the order of the history, and the agent makes the same choice and reaches the same
/// root."
#[tokio::test]
async fn space_disciplines_bag_checkpoint_restart_order_and_root() {
    let rig = Rig::new().await;
    for value in 1..=4 {
        rig.send(salted(value, 1, false)).await;
    }
    let start = rig
        .host
        .create_checkpoint()
        .await
        .expect("a checkpoint")
        .root;
    // Changed by decision 102 (plan v2 pins): a receive over `any()` picks at random, so the test
    // checks each record as the only match, and then the membership of a pick. The body before:
    // let first = fired(&rig.receive(receiving("a", any())).await);
    // let after = rig
    //     .host
    //     .create_checkpoint()
    //     .await
    //     .expect("a checkpoint")
    //     .root;
    // rig.host
    //     .reset(&start)
    //     .await
    //     .expect("the reset to the start");
    // assert_eq!(rig.values().await.len(), 4);
    // let again = fired(&rig.receive(receiving("a", any())).await);
    // assert_eq!(again, first, "the choice does not depend on the list order");
    // assert_eq!(
    //     rig.host
    //         .create_checkpoint()
    //         .await
    //         .expect("a checkpoint")
    //         .root,
    //     after
    // );
    let mut roots: BTreeMap<i64, Blake2b256Hash> = BTreeMap::new();
    for value in 1..=4 {
        let first = fired(&rig.receive(receiving("a", exactly(value))).await);
        assert_eq!(
            first,
            vec![delivery("a", &[])],
            "a takes the record {value}"
        );
        let after = rig
            .host
            .create_checkpoint()
            .await
            .expect("a checkpoint")
            .root;
        rig.host
            .reset(&start)
            .await
            .expect("the reset to the start");
        assert_eq!(rig.values().await.len(), 4);
        let again = fired(&rig.receive(receiving("a", exactly(value))).await);
        assert_eq!(
            again, first,
            "the fire after the reset is the fire before it"
        );
        assert_eq!(
            rig.host
                .create_checkpoint()
                .await
                .expect("a checkpoint")
                .root,
            after
        );
        rig.host
            .reset(&start)
            .await
            .expect("the reset to the start");
        roots.insert(value, after);
    }
    let picked = fired(&rig.receive(receiving("a", any())).await);
    let [(name, values)] = picked.as_slice() else {
        panic!("a receive over any() fires once: {picked:?}");
    };
    assert_eq!(name.as_str(), "a");
    let value = value_of(values);
    assert_eq!(
        Some(
            rig.host
                .create_checkpoint()
                .await
                .expect("a checkpoint")
                .root
        ),
        roots.get(&value).cloned(),
        "the pick {value} reaches the root of the removal of its record"
    );
}

// Retired by decision 90 (C6, step D): `space_disciplines_bag_replay_equal` and its replay script
// ran through the rigged replay modes, which retire (decision 125). Native replay: the family
// matrix `space_native_replay_equal_{hashmap,pathmap,array,vector}`. Its negative controls:
// `space_native_forged_predecessor_fails_closed`,
// `native_space_replay_rejects_a_tampered_candidate`,
// `native_space_replay_rejects_an_outcome_that_does_not_fit`. The text before:
// ```
// /// The script of the replay test: stores, equal records, fires on both sides, guard refusals,
// /// peeks with their restores, a restore that fires a guarded waiter, persistent operations and,
// /// at the end, a divergent pair. The records follow each other in canonical order, so every
// /// operation makes a known number of fires ([`REPLAY_FIRES`]).
// fn replay_script() -> Vec<SpaceOp> {
//     let sends = ordered_sends(&[(1, false), (2, false), (5, false)]);
//     vec![
//         SpaceOp::Produce(sends[0].clone()),
//         SpaceOp::Produce(sends[1].clone()),
//         SpaceOp::Produce(sends[2].clone()),
//         SpaceOp::Produce(sends[2].clone()),
//         SpaceOp::Consume(guarded(receiving("g", any()), equals(2))),
//         SpaceOp::Consume(peeking(receiving("p", exactly(2)))),
//         SpaceOp::Consume(receiving("a", exactly(5))),
//         SpaceOp::Consume(peeking(receiving("q", any()))),
//         SpaceOp::Consume(receiving("t", exactly(7))),
//         SpaceOp::Consume(guarded(receiving("refuses", any()), refusing())),
//         SpaceOp::Produce(salted(7, 1, true)),
//         SpaceOp::Consume(persisting(receiving("drain", exactly(5)))),
//         SpaceOp::Produce(salted(8, 1, false)),
//         SpaceOp::Produce(salted(5, 9, true)),
//     ]
// }
//
// /// The fires of each operation of [`replay_script`]: the guarded waiter parks on 1, the peek of
// /// 2 and its restore fire `p` and `g`, `a` takes one 5, `q` peeks 1 and restores it, the
// /// persistent 7 fires `t` and is stored, the drain takes the other 5 and parks, and the last
// /// persistent 5 fires the drain as a divergent pair.
// const REPLAY_FIRES: [usize; 14] = [0, 0, 0, 0, 0, 2, 1, 1, 0, 0, 1, 1, 0, 1];
//
// /// Runs the replay script on `rig`.
// async fn run_replay_script(rig: &Rig) -> Result<Vec<SpaceOutcome>, SpaceError> {
//     let mut outcomes = Vec::new();
//     for op in replay_script() {
//         outcomes.push(rig.run(op).await?);
//     }
//     Ok(outcomes)
// }
//
// /// Replay stages the transactions of play: equal outcomes, every recorded COMM paired, and the
// /// root of play. A replay without the log diverges.
// #[tokio::test]
// async fn space_disciplines_bag_replay_equal() {
//     let (play, replay) = play_and_replay().await;
//     let budget = budget();
//     let play_rig = Rig::over(play.clone(), &budget).await;
//     let start = play
//         .create_checkpoint()
//         .await
//         .expect("a checkpoint after the creation")
//         .root;
//     let played = run_replay_script(&play_rig)
//         .await
//         .expect("the script plays");
//     assert_eq!(
//         played
//             .iter()
//             .map(|outcome| outcome.fires.len())
//             .collect::<Vec<usize>>(),
//         REPLAY_FIRES.to_vec()
//     );
//     assert_eq!(
//         played
//             .last()
//             .and_then(|outcome| outcome.reissue.as_ref())
//             .map(|reissue| reissue.cause),
//         Some(ReissueCause::DivergentPair)
//     );
//     let Checkpoint { root, log } = play.create_checkpoint().await.expect("a checkpoint");
//     replay
//         .rig_and_reset(start.clone(), log)
//         .await
//         .expect("the replay rigs the play log");
//     let replay_rig = Rig {
//         host: play.clone(),
//         env: env_over(replay.clone(), &budget),
//     };
//     let replayed = run_replay_script(&replay_rig)
//         .await
//         .expect("the script replays");
//     assert_eq!(replayed, played);
//     replay
//         .check_replay_data()
//         .await
//         .expect("every recorded COMM is paired");
//     assert_eq!(
//         replay.create_checkpoint().await.expect("a checkpoint").root,
//         root
//     );
//     replay
//         .rig_and_reset(start, Vec::new())
//         .await
//         .expect("the replay rigs an empty log");
//     replay_rig
//         .run(replay_script().remove(0))
//         .await
//         .expect("a store makes no COMM, so it replays without the log");
//     match replay_rig
//         .run(SpaceOp::Consume(receiving("x", any())))
//         .await
//     {
//         Err(error @ SpaceError::ReplayDivergence(_)) => {
//             assert_eq!(error.code(), "ReplayDivergence")
//         }
//         other => panic!("negative control: a log without the COMMs diverges: {other:?}"),
//     }
// }
// ```

// Disabled by decision 90 (decision 69): Option P (the legacy charges of decision 45) is removed with the legacy meter (DR-134).
// Its host differential continues in `space_disciplines_bag_observer_calls_equal_host`.
//
// /// In a legacy deploy, each bag operation pays what `ChargingRSpace` charges for its host
// /// program, entry by entry, the second send of a peek and its fire included (decision 45,
// /// Option P and Rule E).
// #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// async fn space_disciplines_bag_legacy_charges_equal_host() {
//     let sends = ordered_sends(&[(1, false), (2, false)]);
//     // No persistent receive meets a persistent record, so the host program ends.
//     let ops = vec![
//         SpaceOp::Produce(sends[0].clone()),
//         SpaceOp::Produce(sends[1].clone()),
//         SpaceOp::Consume(guarded(receiving("g", any()), equals(2))),
//         SpaceOp::Consume(peeking(receiving("p", exactly(2)))),
//         SpaceOp::Consume(peeking(receiving("q", any()))),
//         SpaceOp::Consume(receiving("w", exactly(4))),
//         SpaceOp::Consume(peeking(receiving("o", exactly(4)))),
//         SpaceOp::Produce(salted(4, 1, true)),
//         SpaceOp::Consume(receiving("r", exactly(4))),
//         SpaceOp::Produce(salted(5, 1, false)),
//         SpaceOp::Consume(persisting(receiving("drain", exactly(5)))),
//         SpaceOp::Produce(salted(5, 2, false)),
//         SpaceOp::Consume(guarded(receiving("refuses", any()), refusing())),
//         SpaceOp::Produce(salted(6, 1, false)),
//     ];
//     compare_with_host(&ops)
//         .await
//         .unwrap_or_else(|failure| panic!("{failure}"));
// }

/// Added by decision 90 (decision 69): the host differential of the retired
/// `space_disciplines_bag_legacy_charges_equal_host`. Each bag operation makes the observer calls
/// of its host program, call by call and with the same arguments, the second send of a peek and
/// its fire included (Rule E). The fires and the stores are equal too. Play picks at random
/// (decision 102, rule CR-1), and the host program follows the receipts of the agent (plan v2
/// section 5). So each run checks the picks of that run, and the test runs [`PICK_RUNS`] times on
/// fresh spaces. Two picks of the program have two allowed outcomes each, with a chance of 1/2
/// each: `g` meets 1 and parks or meets 2 and fires, and the persistent 4 fires `w` first or `o`
/// first. Over the runs every outcome of both picks occurs. An outcome is missing with a chance of
/// `4·2^-64 = 2^-62` or less.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_disciplines_bag_observer_calls_equal_host() {
    let sends = ordered_sends(&[(1, false), (2, false)]);
    // No persistent receive meets a persistent record, so the host program ends.
    let ops = vec![
        SpaceOp::Produce(sends[0].clone()),
        SpaceOp::Produce(sends[1].clone()),
        SpaceOp::Consume(guarded(receiving("g", any()), equals(2))),
        SpaceOp::Consume(peeking(receiving("p", exactly(2)))),
        SpaceOp::Consume(peeking(receiving("q", any()))),
        SpaceOp::Consume(receiving("w", exactly(4))),
        SpaceOp::Consume(peeking(receiving("o", exactly(4)))),
        SpaceOp::Produce(salted(4, 1, true)),
        SpaceOp::Consume(receiving("r", exactly(4))),
        SpaceOp::Produce(salted(5, 1, false)),
        SpaceOp::Consume(persisting(receiving("drain", exactly(5)))),
        SpaceOp::Produce(salted(5, 2, false)),
        SpaceOp::Consume(guarded(receiving("refuses", any()), refusing())),
        SpaceOp::Produce(salted(6, 1, false)),
    ];
    // Changed by decision 102 (plan v2 pins): play picks at random, so the comparison runs
    // PICK_RUNS times and checks the coverage of the picks. The comparison before:
    // compare_with_host(&ops)
    //     .await
    //     .unwrap_or_else(|failure| panic!("{failure}"));
    let mut guarded_fires: BTreeSet<usize> = BTreeSet::new();
    let mut persistent_orders: BTreeSet<Vec<String>> = BTreeSet::new();
    for _ in 0..PICK_RUNS {
        let compared = compare_with_host(&ops)
            .await
            .unwrap_or_else(|failure| panic!("{failure}"));
        guarded_fires.insert(compared.fires[2].len());
        persistent_orders.insert(
            compared.fires[7]
                .iter()
                .map(|(name, _)| name.clone())
                .collect(),
        );
    }
    assert_eq!(
        guarded_fires,
        BTreeSet::from([0, 1]),
        "g parks on 1 in some run and fires on 2 in another"
    );
    assert_eq!(
        persistent_orders,
        BTreeSet::from([vec!["o".to_string(), "w".to_string()], vec![
            "w".to_string(),
            "o".to_string()
        ],]),
        "the persistent 4 fires w and o in both orders"
    );
}

/// A persistent record and a non-persistent record with equal bytes have two source hashes, and
/// a receive binds one of them. Play picks at random among the two (decision 102, rule CR-1),
/// each with a chance of 1/2. The host program reads the persist flag of the pick from the
/// receipt of the agent (plan v2 section 5). The pin of the host program names a record only by
/// its bytes, and host play picks between the two at random (DR-129). So the program makes its
/// consume again until it binds the record with the persist flag of the agent (decision 89). All
/// [`support::legacy::HOST_DRAWS`] draws miss that flag with a chance of `2^-1024`. Over
/// [`PICK_RUNS`] runs the agent binds each of the two records. One of them is missing with a
/// chance of `2^-63`.
///
/// Changed by decision 102 (plan v2 pins): play picks at random, and the test has a new name. The
/// name before: `space_disciplines_bag_persistent_twin_binds_the_canonical_record`. The text
/// before: "A persistent record and a non-persistent record with equal bytes have two source
/// hashes, and a receive binds the first of them in canonical order (decision 41). The pin of the
/// host program names a record only by its bytes, and host play picks between the two at random
/// (DR-129). So the program makes its consume again until it binds the record with the canonical
/// persist flag (decision 89). The test runs the program `TWIN_RUNS` times."
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// async fn space_disciplines_bag_persistent_twin_binds_the_canonical_record() {
async fn space_disciplines_bag_persistent_twin_binds_the_pick() {
    let twin = salted(1, 0, true);
    let record = salted(1, 0, false);
    assert_eq!(twin.data, record.data, "the two records have equal bytes");
    let ops = vec![
        SpaceOp::Produce(twin),
        SpaceOp::Produce(record),
        SpaceOp::Consume(receiving("t", exactly(1))),
    ];
    // Changed by decision 102 (plan v2 pins): the comparison follows the pick of the agent, and
    // the runs check that the agent binds each record. The runs before:
    // for _ in 0..TWIN_RUNS {
    //     compare_with_host(&ops)
    //         .await
    //         .unwrap_or_else(|failure| panic!("{failure}"));
    // }
    let mut flags: BTreeSet<bool> = BTreeSet::new();
    for _ in 0..PICK_RUNS {
        let compared = compare_with_host(&ops)
            .await
            .unwrap_or_else(|failure| panic!("{failure}"));
        match compared.receive_binds.as_slice() {
            [persistent] => {
                flags.insert(*persistent);
            }
            other => panic!("the receive binds one record: {other:?}"),
        }
    }
    assert_eq!(
        flags,
        BTreeSet::from([false, true]),
        "the receive binds each of the two records in some run"
    );
}

/// A fire removes exactly the waiter that it fires, by its record identity. Two waiters with
/// equal patterns stay apart, where a removal by pattern equality could take the other one
/// (D10, F12, F13). Play picks the waiter of the first send at random, each with a chance of 1/2
/// (decision 102, rule CR-1). In each run on a fresh rig the first send fires one waiter, the
/// other waiter stays, and the second send fires it. Over [`PICK_RUNS`] runs each waiter fires
/// first. One of them is missing with a chance of `2^-63`.
///
/// Changed by decision 102 (plan v2 pins): play picks at random, so the test checks both picks.
#[tokio::test]
async fn space_disciplines_bag_waiter_removal_by_identity_not_pattern() {
    // Changed by decision 102 (plan v2 pins): the waiter of the first send is one of two allowed
    // outcomes. The body before:
    // let rig = Rig::new().await;
    // let receives = ordered_receives(vec![receiving("first", any()), receiving("second", any())]);
    // assert_eq!(
    //     receives[0].pattern, receives[1].pattern,
    //     "the two waiters have equal patterns"
    // );
    // for receive in receives.iter().rev() {
    //     rig.receive(receive.clone()).await;
    // }
    // assert_eq!(fired(&rig.send(salted(1, 1, false)).await), vec![delivery(
    //     "first",
    //     &[1]
    // )]);
    // assert_eq!(rig.waiters().await, vec!["second".to_string()]);
    // assert_eq!(fired(&rig.send(salted(2, 1, false)).await), vec![delivery(
    //     "second",
    //     &[2]
    // )]);
    // assert!(rig.waiters().await.is_empty());
    let receives = ordered_receives(vec![receiving("first", any()), receiving("second", any())]);
    assert_eq!(
        receives[0].pattern, receives[1].pattern,
        "the two waiters have equal patterns"
    );
    let mut first_fires: BTreeSet<String> = BTreeSet::new();
    for _ in 0..PICK_RUNS {
        let rig = Rig::new().await;
        for receive in receives.iter().rev() {
            rig.receive(receive.clone()).await;
        }
        let fires = fired(&rig.send(salted(1, 1, false)).await);
        let (name, other) = match fires.as_slice() {
            [(name, _)] => match name.as_str() {
                "first" => ("first", "second"),
                "second" => ("second", "first"),
                unknown => panic!("an unknown waiter fires: {unknown}"),
            },
            more => panic!("the send fires one waiter: {more:?}"),
        };
        assert_eq!(fires, vec![delivery(name, &[1])]);
        assert_eq!(
            rig.waiters().await,
            vec![other.to_string()],
            "the fire removes only the waiter that it fires"
        );
        assert_eq!(fired(&rig.send(salted(2, 1, false)).await), vec![delivery(
            other,
            &[2]
        )]);
        assert!(rig.waiters().await.is_empty());
        first_fires.insert(name.to_string());
    }
    assert_eq!(
        first_fires,
        BTreeSet::from(["first".to_string(), "second".to_string()]),
        "each waiter fires first in some run"
    );
}

// ─── Property tests: the invariants of DataCollection.v and ContinuationCollection.v ───────

/// A failure of a property case with `message`.
fn failure(message: impl Into<String>) -> TestCaseError { TestCaseError::fail(message.into()) }

/// Added by decision 102 (plan v2 pins): a count of the cases of a property that reached one
/// behavior. A property asserts a least count after its run, so a property that never reaches its
/// behavior fails instead of passing with no evidence.
struct Hits(AtomicUsize);

impl Hits {
    /// No hit yet.
    const fn new() -> Self { Self(AtomicUsize::new(0)) }

    /// Records one hit.
    fn hit(&self) { self.0.fetch_add(1, Ordering::Relaxed); }

    /// Fails unless the behavior `what` was reached at least `least` times.
    fn assert_at_least(&self, least: usize, what: &str) {
        let hits = self.0.load(Ordering::Relaxed);
        eprintln!("coverage: {what}: {hits} hits (at least {least})");
        assert!(
            hits >= least,
            "{what}: {hits} hits, expected at least {least}"
        );
    }
}

/// The record that the one fire of `outcome` binds, or a failure.
fn fired_record(outcome: &SpaceOutcome) -> Result<ListParWithRandom, TestCaseError> {
    match outcome.fires.as_slice() {
        [fire] => match fire.results.as_slice() {
            [result] => Ok(result.removed_datum.clone()),
            other => Err(failure(format!("one result: {other:?}"))),
        },
        other => Err(failure(format!("one fire: {other:?}"))),
    }
}

/// Records of distinct values and salts. A pair with one value and one salt is a pair of equal
/// records.
fn arb_records(max: usize) -> BoxedStrategy<Vec<(i64, u64)>> {
    vec((0i64..4, 0u64..1_000), 0..=max).boxed()
}

/// F1 `bag_visit_order_canonical`: receives that take every value drain each record once. Play
/// picks the record of each receive at random (decision 102, rule CR-1). Each case plays the
/// stores in the generated order and then the receives. The drained source hashes are the
/// stored source hashes, each once, and the store ends empty. Each assertion holds for every
/// pick, so the property has no miss chance. Changed by decision 90 (C6, step D): the replay
/// half ran through the rigged replay modes, which retire.
///
/// Changed by decision 102 (plan v2 pins): play picks at random, so the claim on the insertion
/// order is a claim on the replay order. The text before: "receives that take every value drain
/// the records in the canonical order of their source hashes, for every insertion order (decision
/// 41)."
#[test]
fn prop_bag_visit_order_canonical() {
    // Changed by decision 90 (C6, step D): the case no longer replays over a shuffled order, so
    // the strategy no longer generates one. The text before:
    // ```
    // let strategy = arb_records(6)
    //     .prop_filter("at least one record", |records| !records.is_empty())
    //     .prop_flat_map(|records| (Just(records.clone()), Just(records).prop_shuffle()));
    // check(pure_config(), strategy, |(records, shuffled)| {
    //     tokio_runtime().block_on(visit_order_case(records, shuffled))
    // });
    // ```
    let strategy = arb_records(6).prop_filter("at least one record", |records| !records.is_empty());
    check(pure_config(), strategy, |records| {
        tokio_runtime().block_on(visit_order_case(records))
    });
}

// Changed by decision 102 (plan v2 pins): play picks at random, so the case checks the drained
// multiset and the replay over the two insertion orders. The case before:
// async fn visit_order_case(
//     records: Vec<(i64, u64)>,
//     shuffled: Vec<(i64, u64)>,
// ) -> Result<(), TestCaseError> {
//     let mut drains = Vec::with_capacity(2);
//     for order in [&records, &shuffled] {
//         let rig = Rig::new().await;
//         for (value, salt) in order {
//             rig.send(salted(*value, *salt, false)).await;
//         }
//         let mut hashes = Vec::with_capacity(order.len());
//         for index in 0..order.len() {
//             let outcome = rig.receive(receiving(&format!("r{index}"), any())).await;
//             hashes.push(Produce::create(&data(), &fired_record(&outcome)?, false).hash);
//         }
//         let mut sorted = hashes.clone();
//         sorted.sort();
//         prop_assert_eq!(&hashes, &sorted, "the records leave in canonical order");
//         prop_assert!(rig.values().await.is_empty());
//         drains.push(hashes);
//     }
//     prop_assert_eq!(
//         &drains[0],
//         &drains[1],
//         "the insertion order changes nothing"
//     );
//     Ok(())
// }
async fn visit_order_case(records: Vec<(i64, u64)>) -> Result<(), TestCaseError> {
    let stores = |order: &[(i64, u64)]| -> Vec<SpaceOp> {
        order
            .iter()
            .map(|(value, salt)| SpaceOp::Produce(salted(*value, *salt, false)))
            .collect()
    };
    let generated = stores(&records);
    // Retired by decision 90 (C6, step D): only the replay half read this binding, and that half
    // retires.
    let probe: Vec<SpaceOp> = (0..records.len())
        .map(|index| SpaceOp::Consume(receiving(&format!("r{index}"), any())))
        .collect();
    let played = played_order(&generated, &probe).await?;
    let mut drained = Vec::with_capacity(played.outcomes.len());
    for outcome in &played.outcomes {
        drained.push(Produce::create(&data(), &fired_record(outcome)?, false).hash);
    }
    drained.sort();
    let mut stored: Vec<Blake2b256Hash> = records
        .iter()
        .map(|(value, salt)| record_hash(&salted(*value, *salt, false)))
        .collect();
    stored.sort();
    prop_assert_eq!(&drained, &stored, "the receives drain each record once");
    prop_assert!(played.rig.values().await.is_empty());
    // Retired by decision 90 (C6, step D): the replay half ran through the rigged replay modes,
    // which retire (decision 125). `space_pick_helpers_put_the_named_candidates_first_in_replay`,
    // `space_native_replay_equal_hashmap`.
    Ok(())
}

/// F2 `bag_consume_first_spatial_match`: a receive takes a record that its pattern binds, and it
/// checks its guard once, on that record. A refusal parks it, whatever the other records
/// (decision 37). Play picks the record at random among the records that the pattern binds
/// (decision 102, rule CR-1). An allowed outcome is a fire of a record that the pattern binds and
/// the guard admits, or a park when no record binds or the guard refuses a record that binds.
/// Each case runs the receive on fresh rigs, at most [`PARK_TRIES`] times, until every allowed
/// outcome occurs, and each outcome must be allowed. A case stores at most six records, so each
/// allowed outcome has a chance of 1/6 or more in each try. So one of the at most six outcomes of
/// a case is missing with a chance of `6·(5/6)^128`, which is less than `5·10^-10`. For the 64
/// cases that chance is less than `3·10^-8`. The test counts the cases with two or more allowed
/// outcomes, and the cases where the pick decides between a fire and a park.
///
/// Changed by decision 102 (plan v2 pins): play picks at random. The text before: "a receive
/// takes the first record in canonical order that its pattern binds, and it checks its guard
/// once, on that record. A refusal parks it, whatever the later records (decision 37)."
#[test]
fn prop_bag_consume_first_spatial_match() {
    // Added by decision 102 (plan v2 pins).
    let choices = Hits::new();
    let mixed = Hits::new();
    let strategy = (arb_records(6), arb_pat())
        .prop_flat_map(|(records, pat)| (Just(records), Just(pat), arb_guard(pat)));
    // Changed by decision 102 (plan v2 pins): the case counts its allowed outcomes. The check
    // before:
    // check(pure_config(), strategy, |(records, pat, guard)| {
    //     tokio_runtime().block_on(first_match_case(records, pat, guard))
    // });
    check(pure_config(), strategy, |(records, pat, guard)| {
        tokio_runtime().block_on(first_match_case(records, pat, guard, &choices, &mixed))
    });
    choices.assert_at_least(8, "F2 cases with two or more allowed outcomes");
    mixed.assert_at_least(
        2,
        "F2 cases where the pick decides between a fire and a park",
    );
}

// Changed by decision 102 (plan v2 pins): play picks at random, so the case runs until every
// allowed outcome occurs. The case before:
// async fn first_match_case(
//     records: Vec<(i64, u64)>,
//     pat: Pat,
//     guard: Guard,
// ) -> Result<(), TestCaseError> {
//     let rig = Rig::new().await;
//     let sends: Vec<ProduceOp> = records
//         .iter()
//         .map(|(value, salt)| salted(*value, *salt, false))
//         .collect();
//     for send in &sends {
//         rig.send(send.clone()).await;
//     }
//     let mut canonical = sends.clone();
//     canonical.sort_by_key(record_hash);
//     let first = canonical
//         .iter()
//         .find(|send| pat.fits(value_of(&send.data.pars)));
//     let outcome = rig
//         .receive(ConsumeOp {
//             guard: guard.par(),
//             ..receiving("k", pat.pattern())
//         })
//         .await;
//     match first {
//         Some(send) if guard.admits(value_of(&send.data.pars)) => {
//             prop_assert_eq!(fired_record(&outcome)?, record::wrap_datum(0, &send.data));
//             prop_assert!(rig.waiters().await.is_empty());
//             prop_assert_eq!(rig.values().await.len() + 1, sends.len());
//         }
//         _ => {
//             prop_assert!(
//                 outcome.fires.is_empty(),
//                 "no match, or the guard refuses the first one"
//             );
//             prop_assert_eq!(rig.waiters().await, vec!["k".to_string()]);
//             prop_assert_eq!(rig.values().await.len(), sends.len());
//         }
//     }
//     Ok(())
// }
async fn first_match_case(
    records: Vec<(i64, u64)>,
    pat: Pat,
    guard: Guard,
    choices: &Hits,
    mixed: &Hits,
) -> Result<(), TestCaseError> {
    let sends: Vec<ProduceOp> = records
        .iter()
        .map(|(value, salt)| salted(*value, *salt, false))
        .collect();
    // The allowed outcomes: `None` for a park, and the source hash of each record that a fire can
    // bind.
    let mut allowed: BTreeSet<Option<Blake2b256Hash>> = BTreeSet::new();
    let mut fitting = 0usize;
    for send in &sends {
        let value = value_of(&send.data.pars);
        match (pat.fits(value), guard.admits(value)) {
            (true, true) => {
                fitting += 1;
                allowed.insert(Some(record_hash(send)));
            }
            (true, false) => {
                fitting += 1;
                allowed.insert(None);
            }
            (false, _) => {}
        }
    }
    if fitting == 0 {
        allowed.insert(None);
    }
    let receive = ConsumeOp {
        guard: guard.par(),
        ..receiving("k", pat.pattern())
    };
    let mut seen: BTreeSet<Option<Blake2b256Hash>> = BTreeSet::new();
    for _ in 0..PARK_TRIES {
        let rig = Rig::new().await;
        for send in &sends {
            rig.send(send.clone()).await;
        }
        let outcome = rig.receive(receive.clone()).await;
        let pick = match outcome.fires.as_slice() {
            [] => {
                prop_assert_eq!(rig.waiters().await, vec!["k".to_string()]);
                prop_assert_eq!(rig.values().await.len(), sends.len());
                None
            }
            [_] => {
                prop_assert!(rig.waiters().await.is_empty());
                prop_assert_eq!(rig.values().await.len() + 1, sends.len());
                Some(Produce::create(&data(), &fired_record(&outcome)?, false).hash)
            }
            more => return Err(failure(format!("a receive fires once or parks: {more:?}"))),
        };
        prop_assert!(
            allowed.contains(&pick),
            "the outcome {:?} is allowed: {:?}",
            pick,
            allowed
        );
        seen.insert(pick);
        if seen.len() == allowed.len() {
            break;
        }
    }
    prop_assert_eq!(
        &seen,
        &allowed,
        "every allowed outcome occurs in {} tries",
        PARK_TRIES
    );
    if allowed.len() >= 2 {
        choices.hit();
    }
    if allowed.len() >= 2 && allowed.contains(&None) {
        mixed.hit();
    }
    Ok(())
}

/// F3 `bag_multiplicity_preserved`: over generated programs, the agent keeps the multiset of the
/// model. A store adds exactly one record, a fire removes exactly the non-persistent records
/// that it binds, and a peek sends its record again (decision 45, Rule E). The fires agree too.
/// Play picks at random (decision 102, rule CR-1), so the model follows the picks of the agent and
/// checks that the plans allow each pick ([`BagModel`]). Each generated program runs
/// [`CASE_RUNS`] times on fresh rigs, so the model meets more picks. Each assertion holds for every
/// pick. The count of the second sends that fire a waiter depends on the picks. Its miss chance is
/// an estimate and not a bound: about 30 programs of the Rule E scenario run 4 times each, and
/// one run reaches such a fire with a chance of about 1/4 or more. So all runs miss with a chance
/// of about `(3/4)^120`, which is about `10^-15`.
///
/// Changed by decision 102 (plan v2 pins): the model follows the picks of the agent, and each
/// program runs [`CASE_RUNS`] times.
#[test]
fn prop_bag_multiplicity_preserved() {
    // Changed by decision 102 (plan v2 pins): each program runs CASE_RUNS times. The check before:
    // check(pure_config(), arb_any_program(), |program| {
    //     tokio_runtime().block_on(multiplicity_case(program))
    // });
    check(pure_config(), arb_any_program(), |program| {
        let runtime = tokio_runtime();
        for _ in 0..CASE_RUNS {
            runtime.block_on(multiplicity_case(program.clone()))?;
        }
        Ok(())
    });
    assert!(
        REOFFER_FIRES.load(Ordering::Relaxed) > 0,
        "the programs never made a second send that fires a waiter"
    );
}

/// The peeks of `prop_bag_multiplicity_preserved` whose second send fired a waiter.
static REOFFER_FIRES: AtomicUsize = AtomicUsize::new(0);

async fn multiplicity_case(program: Vec<BagOp>) -> Result<(), TestCaseError> {
    let rig = Rig::new().await;
    let mut model = BagModel::default();
    for (index, op) in program.iter().enumerate() {
        // Changed by decision 102 (plan v2 pins): the model reads the picks of the agent, so the
        // operation runs first. The model step before:
        // let expected = model.apply(op);
        let outcome = rig
            .run(op.space_op())
            .await
            .map_err(|error| failure(format!("operation {index} fails: {error}")))?;
        let expected = model.apply(op, &agent_fires(&outcome)?)?;
        if let (BagOp::Receive { peek: true, .. }, 2..) = (op, outcome.fires.len()) {
            REOFFER_FIRES.fetch_add(1, Ordering::Relaxed);
        }
        prop_assert_eq!(
            fired(&outcome),
            expected,
            "the fires of operation {}",
            index
        );
        prop_assert_eq!(&outcome.reissue, &None);
        prop_assert_eq!(
            rig.values().await,
            model.values(),
            "the records after {}",
            index
        );
        prop_assert_eq!(
            rig.waiters().await,
            model.waiter_names(),
            "the waiters after {}",
            index
        );
    }
    Ok(())
}

/// F4 `bag_removal_keeps_equal_duplicate`: of several equal records, each receive removes
/// exactly one, and the others stay.
#[test]
fn prop_bag_removal_keeps_equal_duplicate() {
    let strategy = (0i64..4, 2usize..5, vec((10i64..14, 0u64..1_000), 0..4));
    check(pure_config(), strategy, |(value, copies, others)| {
        tokio_runtime().block_on(duplicate_case(value, copies, others))
    });
}

async fn duplicate_case(
    value: i64,
    copies: usize,
    others: Vec<(i64, u64)>,
) -> Result<(), TestCaseError> {
    let rig = Rig::new().await;
    let twin = salted(value, 0, false);
    for _ in 0..copies {
        rig.send(twin.clone()).await;
    }
    for (other, salt) in &others {
        rig.send(salted(*other, *salt, false)).await;
    }
    let count = |values: &[(i64, bool)]| values.iter().filter(|(v, _)| *v == value).count();
    for left in (0..copies).rev() {
        let outcome = rig
            .receive(receiving(&format!("k{left}"), exactly(value)))
            .await;
        prop_assert_eq!(fired_record(&outcome)?, record::wrap_datum(0, &twin.data));
        let values = rig.values().await;
        prop_assert_eq!(count(&values), left, "one equal record leaves");
        prop_assert_eq!(values.len(), left + others.len());
    }
    Ok(())
}

/// F5 `bag_persistent_match_not_removed`: a fire on a persistent record leaves it, and a peek
/// of it sends nothing again.
#[test]
fn prop_bag_persistent_match_not_removed() {
    let strategy = (
        0i64..4,
        vec(proptest::bool::ANY, 1..6),
        vec((10i64..14, 0u64..1_000), 0..4),
    );
    check(pure_config(), strategy, |(value, peeks, others)| {
        tokio_runtime().block_on(persistent_case(value, peeks, others))
    });
}

async fn persistent_case(
    value: i64,
    peeks: Vec<bool>,
    others: Vec<(i64, u64)>,
) -> Result<(), TestCaseError> {
    let rig = Rig::new().await;
    rig.send(salted(value, 0, true)).await;
    for (other, salt) in &others {
        rig.send(salted(*other, *salt, false)).await;
    }
    let before = rig.values().await;
    rig.log().await;
    for (index, peek) in peeks.into_iter().enumerate() {
        let receive = ConsumeOp {
            peek,
            ..receiving(&format!("k{index}"), exactly(value))
        };
        let outcome = rig.receive(receive).await;
        prop_assert!(outcome.fires.len() == 1 && outcome.fires[0].results[0].persistent);
        prop_assert_eq!(
            rig.values().await,
            before.clone(),
            "the persistent record stays"
        );
        prop_assert!(
            produces(&rig.log().await).is_empty(),
            "nothing is sent again"
        );
    }
    Ok(())
}

// Disabled by decision 90 (decision 69): Option P (the legacy charges of decision 45) is removed with the legacy meter (DR-134).
// Its host differential continues in `prop_bag_observer_calls_equal_host`.
//
// /// `bag_peek_restore_is_host_second_send` and Option P: over generated programs, in a legacy
// /// deploy, each operation stages the operations of its host program and pays its charges, the
// /// second send of each peek and its fires included. The stores end equal.
// #[test]
// fn prop_bag_legacy_charges_equal_host() {
//     check(pure_config(), arb_any_program(), |program| {
//         let ops: Vec<SpaceOp> = program.iter().map(BagOp::space_op).collect();
//         tokio_runtime().block_on(compare_with_host(&ops))
//     });
// }

/// Added by decision 90 (decision 69): the host differential of the retired
/// `prop_bag_legacy_charges_equal_host`. `bag_peek_restore_is_host_second_send`: over generated
/// programs, each operation makes the observer calls of its host program, the second send of each
/// peek and its fires included. The fires and the stores are equal too. Play picks at random
/// (decision 102, rule CR-1), and the host program follows the receipts of the agent and checks
/// that the plans allow each pick (plan v2 section 5). Each assertion holds for every pick.
#[test]
fn prop_bag_observer_calls_equal_host() {
    check(pure_config(), arb_any_program(), |program| {
        let ops: Vec<SpaceOp> = program.iter().map(BagOp::space_op).collect();
        // Changed by decision 102 (plan v2 pins): the comparison returns what it compared.
        // tokio_runtime().block_on(compare_with_host(&ops))
        tokio_runtime()
            .block_on(compare_with_host(&ops))
            .map(|_| ())
    });
}

/// The authority byte of a send of `value` with `salt`, distinct for each record.
fn send_byte(value: i64, salt: u64) -> u8 {
    u8::try_from(1 + value * 20 + i64::try_from(salt).expect("a small salt"))
        .expect("a small authority byte")
}

/// The operation of `op` with cost authorities: a send carries the authority of its record as
/// its introduction and its cost authority, and a receive has an introduction of its own.
fn authored(op: &BagOp) -> SpaceOp {
    match (op, op.space_op()) {
        (BagOp::Send { value, salt, .. }, SpaceOp::Produce(mut send)) => {
            let authority = region(send_byte(*value, *salt));
            send.authority = authority.clone();
            send.data.cost_authority = Some(authority);
            SpaceOp::Produce(send)
        }
        (BagOp::Receive { name, .. }, SpaceOp::Consume(receive)) => {
            let byte = u8::try_from(128 + name).expect("a small receive name");
            SpaceOp::Consume(ConsumeOp {
                introduction_authority: region(byte),
                ..receive
            })
        }
        (_, other) => other,
    }
}

/// `bag_peek_restore_is_host_second_send`, the authority: over generated programs of the Rule E
/// scenario, every produce of a record (its store, its fires, and the second send of a peek
/// with its fires) registers the authority of the record, as `produce_peeks` does, and every
/// consume registers the introduction of its receive.
#[test]
fn prop_bag_restore_registers_the_record_authority() {
    check(pure_config(), arb_rule_e_program(), |program| {
        tokio_runtime().block_on(authority_case(program))
    });
    assert!(
        SECOND_SENDS.load(Ordering::Relaxed) > 0,
        "the programs never sent a peeked record again"
    );
}

/// The produce events of the receives of `prop_bag_restore_registers_the_record_authority`:
/// the second sends of peeks.
static SECOND_SENDS: AtomicUsize = AtomicUsize::new(0);

async fn authority_case(program: Vec<BagOp>) -> Result<(), TestCaseError> {
    let budget = budget();
    let _scope = budget.enter_comm_accounting_scope();
    let rig = Rig::with_budget(&budget).await;
    let mut authorities: BTreeMap<Vec<u8>, CostAuthority> = BTreeMap::new();
    for (index, op) in program.iter().enumerate() {
        let space_op = authored(op);
        let introduction = match &space_op {
            SpaceOp::Produce(send) => {
                authorities.insert(record_hash(send).0, send.authority.clone());
                None
            }
            SpaceOp::Consume(receive) => Some(receive.introduction_authority.clone()),
            SpaceOp::Settle(_) => None,
            SpaceOp::Join(join) => Some(join.introduction_authority.clone()),
        };
        rig.run(space_op)
            .await
            .map_err(|error| failure(format!("operation {index} fails: {error}")))?;
        for event in rig.log().await {
            match &event {
                Event::IoEvent(IOEvent::Produce(produce)) => {
                    let expected = authorities.get(&produce.hash.0).cloned();
                    prop_assert!(expected.is_some(), "the produce of a known record");
                    prop_assert_eq!(registered(&budget, &event), expected);
                    if introduction.is_some() {
                        SECOND_SENDS.fetch_add(1, Ordering::Relaxed);
                    }
                }
                Event::IoEvent(IOEvent::Consume(_)) => {
                    prop_assert_eq!(registered(&budget, &event), introduction.clone());
                }
                Event::Comm(_) => {}
            }
        }
    }
    Ok(())
}

/// A generated waiter that takes one datum and never re-offers it: plain, not persistent.
fn arb_plain_waiters(max: usize) -> BoxedStrategy<Vec<(Pat, Guard)>> {
    vec(
        arb_pat().prop_flat_map(|pat| (Just(pat), arb_guard(pat))),
        1..=max,
    )
    .boxed()
}

/// The receive of the generated waiter `index`.
fn plain_receive(index: usize, (pat, guard): (Pat, Guard)) -> ConsumeOp {
    ConsumeOp {
        guard: guard.par(),
        ..receiving(&format!("w{index}"), pat.pattern())
    }
}

// Changed by decision 102 (plan v2 pins): play picks the waiter of a send at random among the
// admitting waiters, so the tests use the set of the admitting waiters ([`admitting_waiters`]).
// The function before:
// /// The name of the waiter that a send of `value` fires among `waiters`: the first in canonical
// /// order whose pattern fits and whose guard admits the value (decision 37, decision 41).
// fn expected_waiter(waiters: &[(Pat, Guard)], value: i64) -> Option<String> {
//     let mut candidates: Vec<(Blake2b256Hash, usize)> = waiters
//         .iter()
//         .enumerate()
//         .filter(|(_, (pat, guard))| pat.fits(value) && guard.admits(value))
//         .map(|(index, waiter)| (waiter_hash(&plain_receive(index, *waiter)), index))
//         .collect();
//     candidates.sort();
//     candidates.first().map(|(_, index)| format!("w{index}"))
// }

/// Added by decision 102 (plan v2 pins): the names of the waiters among `waiters` that a send of
/// `value` can fire: each waiter whose pattern fits the value and whose guard admits it (decision
/// 37). Play picks one of them at random (rule CR-1).
fn admitting_waiters(waiters: &[(Pat, Guard)], value: i64) -> Vec<String> {
    waiters
        .iter()
        .enumerate()
        .filter(|(_, (pat, guard))| pat.fits(value) && guard.admits(value))
        .map(|(index, _)| format!("w{index}"))
        .collect()
}

/// F12 `waiter_removal_by_record_identity`: a send fires a waiter that binds and admits its
/// datum, and it removes exactly that waiter, by its identity. Every other waiter stays. The send
/// stores its datum only when no waiter takes it. Play picks the waiter at random among the
/// admitting waiters (decision 102, rule CR-1), so each case checks that the fired waiter is one
/// of them. Each assertion holds for every pick, so the property has no miss chance. The test
/// counts the cases with two or more admitting waiters.
///
/// Changed by decision 102 (plan v2 pins): play picks at random. The text before: "a send fires
/// the first admitting waiter in canonical order and removes exactly that waiter, by its
/// identity. Every other waiter stays."
#[test]
fn prop_bag_waiter_removal_by_record_identity() {
    // Added by decision 102 (plan v2 pins).
    let choices = Hits::new();
    // Changed by decision 102 (plan v2 pins): the case counts the cases with a choice. The check
    // before:
    // check(
    //     pure_config(),
    //     (arb_plain_waiters(6), 0i64..4),
    //     |(waiters, value)| tokio_runtime().block_on(removal_case(waiters, value)),
    // );
    check(
        pure_config(),
        (arb_plain_waiters(6), 0i64..4),
        |(waiters, value)| tokio_runtime().block_on(removal_case(waiters, value, &choices)),
    );
    choices.assert_at_least(16, "F12 cases with two or more admitting waiters");
}

// Changed by decision 102 (plan v2 pins): play picks at random, so the case checks that the fired
// waiter is an admitting waiter. The case before:
// async fn removal_case(waiters: Vec<(Pat, Guard)>, value: i64) -> Result<(), TestCaseError> {
//     let rig = Rig::new().await;
//     for (index, waiter) in waiters.iter().enumerate() {
//         rig.receive(plain_receive(index, *waiter)).await;
//     }
//     let names: Vec<String> = (0..waiters.len())
//         .map(|index| format!("w{index}"))
//         .collect();
//     let outcome = rig.send(salted(value, 1, false)).await;
//     let fired_names: Vec<String> = fired(&outcome).into_iter().map(|(name, _)| name).collect();
//     let expected = expected_waiter(&waiters, value);
//     prop_assert_eq!(
//         &fired_names,
//         &expected.iter().cloned().collect::<Vec<String>>()
//     );
//     let mut remaining = names.clone();
//     remaining.retain(|name| Some(name) != expected.as_ref());
//     remaining.sort();
//     prop_assert_eq!(rig.waiters().await, remaining);
//     let stored = match expected {
//         Some(_) => Vec::new(),
//         None => vec![(value, false)],
//     };
//     prop_assert_eq!(rig.values().await, stored);
//     Ok(())
// }
async fn removal_case(
    waiters: Vec<(Pat, Guard)>,
    value: i64,
    choices: &Hits,
) -> Result<(), TestCaseError> {
    let rig = Rig::new().await;
    for (index, waiter) in waiters.iter().enumerate() {
        rig.receive(plain_receive(index, *waiter)).await;
    }
    let mut remaining: Vec<String> = (0..waiters.len())
        .map(|index| format!("w{index}"))
        .collect();
    let admitting = admitting_waiters(&waiters, value);
    let outcome = rig.send(salted(value, 1, false)).await;
    let fired_names: Vec<String> = fired(&outcome).into_iter().map(|(name, _)| name).collect();
    match fired_names.as_slice() {
        [] => {
            prop_assert!(
                admitting.is_empty(),
                "the send stores, so no waiter takes {}: {:?}",
                value,
                admitting
            );
            prop_assert_eq!(rig.values().await, vec![(value, false)]);
        }
        [name] => {
            prop_assert!(
                admitting.contains(name),
                "the fired waiter {} takes {}",
                name,
                value
            );
            remaining.retain(|other| other != name);
            prop_assert!(rig.values().await.is_empty());
        }
        more => {
            return Err(failure(format!(
                "a send fires one waiter or stores: {more:?}"
            )))
        }
    }
    remaining.sort();
    prop_assert_eq!(rig.waiters().await, remaining);
    if admitting.len() >= 2 {
        choices.hit();
    }
    Ok(())
}

/// F13 `pattern_equality_removal_can_remove_another_waiter`, the agent side: waiters with equal
/// patterns and guards stay distinct. Each send fires exactly one of the waiters that remain and
/// removes exactly that waiter, never another waiter with the same pattern. After one send for
/// each waiter, no waiter remains. Play picks the waiter of each send at random among the
/// remaining waiters (decision 102, rule CR-1). Each assertion holds for every pick, so the
/// property has no miss chance.
///
/// Changed by decision 102 (plan v2 pins): play picks at random. The text before: "Each send
/// removes exactly the waiter that it fires, in canonical order, never another waiter with the
/// same pattern."
#[test]
fn prop_bag_equal_pattern_waiters_stay_distinct() {
    let strategy = (
        arb_pat(),
        prop_oneof![Just(Guard::Free), Just(Guard::Admit)],
        2usize..5,
    );
    check(pure_config(), strategy, |(pat, guard, count)| {
        tokio_runtime().block_on(equal_pattern_case(pat, guard, count))
    });
}

async fn equal_pattern_case(pat: Pat, guard: Guard, count: usize) -> Result<(), TestCaseError> {
    let rig = Rig::new().await;
    let waiters = vec![(pat, guard); count];
    for (index, waiter) in waiters.iter().enumerate() {
        rig.receive(plain_receive(index, *waiter)).await;
    }
    let value = match pat {
        Pat::Any => 2,
        Pat::Lit(literal) => literal,
    };
    // Changed by decision 102 (plan v2 pins): play picks the waiter of each send at random, so
    // the case removes the fired waiter from the remaining waiters. The steps before:
    // let mut canonical: Vec<(Blake2b256Hash, String)> = waiters
    //     .iter()
    //     .enumerate()
    //     .map(|(index, waiter)| {
    //         (
    //             waiter_hash(&plain_receive(index, *waiter)),
    //             format!("w{index}"),
    //         )
    //     })
    //     .collect();
    // canonical.sort();
    // for (step, (_, name)) in canonical.iter().enumerate() {
    //     let outcome = rig
    //         .send(salted(
    //             value,
    //             u64::try_from(step).expect("a small step"),
    //             false,
    //         ))
    //         .await;
    //     let names: Vec<String> = fired(&outcome).into_iter().map(|(name, _)| name).collect();
    //     prop_assert_eq!(names, vec![name.clone()]);
    //     let mut remaining: Vec<String> = canonical[step + 1..]
    //         .iter()
    //         .map(|(_, name)| name.clone())
    //         .collect();
    //     remaining.sort();
    //     prop_assert_eq!(rig.waiters().await, remaining);
    // }
    let mut remaining: Vec<String> = (0..count).map(|index| format!("w{index}")).collect();
    remaining.sort();
    for step in 0..count {
        let outcome = rig
            .send(salted(
                value,
                u64::try_from(step).expect("a small step"),
                false,
            ))
            .await;
        let names: Vec<String> = fired(&outcome).into_iter().map(|(name, _)| name).collect();
        let [name] = names.as_slice() else {
            return Err(failure(format!(
                "the send {step} fires one waiter: {names:?}"
            )));
        };
        let index = remaining
            .iter()
            .position(|other| other == name)
            .ok_or_else(|| failure(format!("the fired waiter {name} remains")))?;
        remaining.remove(index);
        prop_assert_eq!(rig.waiters().await, remaining.clone());
    }
    prop_assert!(rig.waiters().await.is_empty());
    Ok(())
}

/// F14 `park_byte_identity_idempotent`: a park whose bytes equal a parked waiter changes
/// nothing, and any byte difference parks another waiter.
#[test]
fn prop_bag_park_byte_identity_idempotent() {
    let strategy = arb_pat().prop_flat_map(|pat| (Just(pat), arb_guard(pat), 0u8..3, 0u64..1_000));
    check(pure_config(), strategy, |(pat, guard, kind, salt)| {
        tokio_runtime().block_on(idempotent_case(pat, guard, kind, salt))
    });
}

async fn idempotent_case(pat: Pat, guard: Guard, kind: u8, salt: u64) -> Result<(), TestCaseError> {
    let rig = Rig::new().await;
    let receive = resalted(
        ConsumeOp {
            persistent: kind == 1,
            peek: kind == 2,
            guard: guard.par(),
            ..receiving("k", pat.pattern())
        },
        salt,
    );
    rig.receive(receive.clone()).await;
    rig.receive(receive.clone()).await;
    prop_assert_eq!(
        rig.waiters().await,
        vec!["k".to_string()],
        "the equal park adds nothing"
    );
    rig.receive(resalted(receive, salt + 1)).await;
    prop_assert_eq!(
        rig.waiters().await,
        vec!["k".to_string(), "k".to_string()],
        "another random state parks another waiter"
    );
    Ok(())
}

/// F15 `waiter_visit_order_canonical`: a send fires a waiter that binds and admits it, or it
/// stores its datum when no waiter does (decision 37). Play picks the waiter at random among the
/// admitting waiters (decision 102, rule CR-1). Each case parks the waiters in the natural order
/// and plays the send. Each assertion holds for every pick, so the property has no miss chance.
/// Changed by decision 90 (C6, step D): the replay half ran through the rigged replay modes,
/// which retire.
///
/// Changed by decision 102 (plan v2 pins): play picks at random, so the claim on the park order
/// is a claim on the replay order. The text before: "a send fires the first waiter in canonical
/// order that binds and admits it, for every park order (decision 37, decision 41)."
#[test]
fn prop_bag_waiter_visit_order_canonical() {
    // Changed by decision 90 (C6, step D): the case no longer replays over a shuffled order, so
    // the strategy no longer generates one. The text before:
    // ```
    // let strategy = (arb_plain_waiters(6), 0i64..4).prop_flat_map(|(waiters, value)| {
    //     let count = waiters.len();
    //     (
    //         Just(waiters),
    //         Just(value),
    //         Just((0..count).collect::<Vec<usize>>()).prop_shuffle(),
    //     )
    // });
    // check(pure_config(), strategy, |(waiters, value, order)| {
    //     tokio_runtime().block_on(waiter_order_case(waiters, value, order))
    // });
    // ```
    let strategy = (arb_plain_waiters(6), 0i64..4);
    check(pure_config(), strategy, |(waiters, value)| {
        tokio_runtime().block_on(waiter_order_case(waiters, value))
    });
}

// Changed by decision 102 (plan v2 pins): play picks at random, so the case checks that the fire
// is allowed and that the replay over the two park orders makes it again. The case before:
// async fn waiter_order_case(
//     waiters: Vec<(Pat, Guard)>,
//     value: i64,
//     order: Vec<usize>,
// ) -> Result<(), TestCaseError> {
//     let expected: Vec<String> = expected_waiter(&waiters, value).into_iter().collect();
//     let natural: Vec<usize> = (0..waiters.len()).collect();
//     for park_order in [&natural, &order] {
//         let rig = Rig::new().await;
//         for index in park_order {
//             rig.receive(plain_receive(*index, waiters[*index])).await;
//         }
//         let outcome = rig.send(salted(value, 1, false)).await;
//         let names: Vec<String> = fired(&outcome).into_iter().map(|(name, _)| name).collect();
//         prop_assert_eq!(&names, &expected, "the park order {:?}", park_order);
//     }
//     Ok(())
// }
async fn waiter_order_case(waiters: Vec<(Pat, Guard)>, value: i64) -> Result<(), TestCaseError> {
    let admitting = admitting_waiters(&waiters, value);
    let parks = |indices: &[usize]| -> Vec<SpaceOp> {
        indices
            .iter()
            .map(|index| SpaceOp::Consume(plain_receive(*index, waiters[*index])))
            .collect()
    };
    let natural: Vec<usize> = (0..waiters.len()).collect();
    let natural_parks = parks(&natural);
    // Retired by decision 90 (C6, step D): only the replay half read this binding, and that half
    // retires.
    let probe = [SpaceOp::Produce(salted(value, 1, false))];
    let played = played_order(&natural_parks, &probe).await?;
    let names: Vec<String> = played
        .outcomes
        .iter()
        .flat_map(fired)
        .map(|(name, _)| name)
        .collect();
    match names.as_slice() {
        [] => {
            prop_assert!(
                admitting.is_empty(),
                "the send stores, so no waiter takes {}: {:?}",
                value,
                admitting
            );
        }
        [name] => {
            prop_assert!(
                admitting.contains(name),
                "the fired waiter {} takes {}",
                name,
                value
            );
        }
        more => {
            return Err(failure(format!(
                "a send fires one waiter or stores: {more:?}"
            )))
        }
    }
    // Retired by decision 90 (C6, step D): the replay half ran through the rigged replay modes,
    // which retire (decision 125). `space_pick_helpers_put_the_named_candidates_first_in_replay`,
    // `space_native_replay_equal_hashmap`.
    Ok(())
}

/// F16 `head_waiter_unique`, the head-only contrast: on a queue, a send meets only the head
/// waiter. A head that refuses blocks, although a later waiter would take the datum. A bag
/// offers the datum to every waiter (`prop_bag_waiter_visit_order_canonical`).
#[test]
fn prop_head_waiter_unique() {
    check(
        pure_config(),
        (arb_plain_waiters(6), 0i64..4),
        |(waiters, value)| tokio_runtime().block_on(head_waiter_case(waiters, value)),
    );
}

async fn head_waiter_case(waiters: Vec<(Pat, Guard)>, value: i64) -> Result<(), TestCaseError> {
    let host = play().await;
    let env = env_over(host.clone(), &budget());
    let config = config_of(SPACE, "hashmap", "queue", SpaceQualifier::Default);
    create(
        &Queue::default(),
        &env,
        creation(SPACE, config, SpaceFactoryUrn::HASHMAP_QUEUE),
    )
    .await;
    for (index, waiter) in waiters.iter().enumerate() {
        run(
            &Queue::default(),
            &env,
            SpaceOp::Consume(plain_receive(index, *waiter)),
        )
        .await
        .map_err(|error| failure(format!("the park fails: {error}")))?;
    }
    let outcome = run(
        &Queue::default(),
        &env,
        SpaceOp::Produce(salted(value, 1, false)),
    )
    .await
    .map_err(|error| failure(format!("the send fails: {error}")))?;
    let names: Vec<String> = fired(&outcome).into_iter().map(|(name, _)| name).collect();
    let (pat, guard) = waiters[0];
    let expected = match pat.fits(value) && guard.admits(value) {
        true => vec!["w0".to_string()],
        false => Vec::new(),
    };
    prop_assert_eq!(names, expected, "only the head waiter can fire");
    prop_assert!(outcome.fires.len() <= 1);
    Ok(())
}
