//! The bag discipline of the native spaces (P5a): the agent `GenericSpace<HashMapOuter,
//! BagDiscipline>` runs directly over an in-memory play space and its replay space (P5 design
//! v3, A.7 and A.12).
//!
//! The example tests are the bag tests of the P5 design v2 (section 2.8.4) with the changes of
//! v3 A.12: `peek_keeps_record` became `space_disciplines_bag_peek_restores_record`, and the
//! suite adds `peek_restore_fires_guarded_waiter`, `legacy_charges_equal_host` and
//! `persistent_reissue_keeps_identity`. Decision 89 adds
//! `persistent_twin_binds_the_canonical_record`.
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
//! | `bag_peek_restore_is_host_second_send` | `prop_bag_legacy_charges_equal_host`, `prop_bag_restore_registers_the_record_authority` |
//! | F12 `waiter_removal_by_record_identity` | `prop_bag_waiter_removal_by_record_identity` |
//! | F13 `pattern_equality_removal_can_remove_another_waiter` | `prop_bag_equal_pattern_waiters_stay_distinct` |
//! | F14 `park_byte_identity_idempotent` | `prop_bag_park_byte_identity_idempotent` |
//! | F15 `waiter_visit_order_canonical` | `prop_bag_waiter_visit_order_canonical` |
//! | F16 `head_waiter_unique` | `prop_head_waiter_unique` |
//!
//! The host differential runs the equivalent host program on a real `ChargingRSpace`: the host
//! call of each operation, and the calls that the reducer makes after a fire (the re-produce of
//! a persistent send, the re-consume of a persistent receive, and `produce_peeks`). The host
//! space stores the same record bytes on the same channel, and its matcher matches the payload
//! of a record without its envelope, as the agent does. Host play picks at random among
//! candidates since DR-129. So before each host call, the program pins the matcher to the
//! choice of the canonical order of decision 41 on the host records. A consume binds only the
//! first record that its pattern binds, and a produce fires only the first waiter that takes
//! its record. A pin names a record by its bytes only. So the program makes a consume again
//! until it binds the record with the canonical persist flag (decision 89).

#[path = "support/mod.rs"]
mod support;

use std::collections::{BTreeMap, VecDeque};
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
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::RuntimeBudget;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::reified::costs::{ReissueCause, FIRE_ROUND_CAP};
use rholang::rust::interpreter::rho_runtime::RhoISpace;
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
use rholang::rust::interpreter::storage::charging_rspace::ChargingRSpace;
use rspace_plus_plus::rspace::checkpoint::Checkpoint;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider;
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::space_store_handle::canonical_data_order;
use rspace_plus_plus::rspace::trace::event::{Consume, Event, IOEvent, Produce, COMM};
use support::agent::{
    any, body_name, budget, config_of, counter, counter_channel, create, creation, data_channel,
    env_over, exactly, fired, int, play, play_and_replay, receive_on, records_on, run, send_on,
    space_counter_channel, string, ParRSpace,
};
use support::legacy::{
    drawn_consume, host_consume, host_produce, waiter_choice, WaiterPin, TWIN_RUNS,
};
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

/// The phlo of a legacy differential: far above every program of the suite.
const LEGACY_PHLO: i64 = 1_000_000_000_000;

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
/// `ContinuationCollection.v` over values. The candidates of each side are visited by source
/// hash (decision 41). A consume checks the guard once on its first spatial match (decision
/// 37). A produce skips a waiter whose guard refuses. A peek of a non-persistent record removes
/// it and offers it again (decision 45, Rule E). The generators never make a divergent pair.
#[derive(Clone, Debug, Default)]
struct BagModel {
    records: Vec<ModelRecord>,
    waiters: Vec<ModelWaiter>,
}

impl BagModel {
    /// Applies `op` and returns its fires, as `fired` reports them.
    fn apply(&mut self, op: &BagOp) -> Vec<(String, Vec<Par>)> {
        let mut fires = Vec::new();
        match (op, op.space_op()) {
            (
                BagOp::Send {
                    value, persistent, ..
                },
                SpaceOp::Produce(send),
            ) => self.offer(*value, *persistent, record_hash(&send), &mut fires),
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
                self.receive(waiter, &mut fires)
            }
            _ => panic!("an operation and its space operation disagree"),
        }
        fires
    }

    /// The indices of `hashes` in canonical order. Equal hashes are byte-identical entries, so
    /// their order does not matter.
    fn canonical(hashes: impl Iterator<Item = Blake2b256Hash>) -> Vec<usize> {
        let mut indexed: Vec<(Blake2b256Hash, usize)> = hashes
            .enumerate()
            .map(|(index, hash)| (hash, index))
            .collect();
        indexed.sort();
        indexed.into_iter().map(|(_, index)| index).collect()
    }

    /// OFFER: the datum fires the first waiter in canonical order that it fits and whose guard
    /// admits it, again while it is not spent, and it is stored when no waiter takes it.
    fn offer(
        &mut self,
        value: i64,
        persist: bool,
        hash: Blake2b256Hash,
        fires: &mut Vec<(String, Vec<Par>)>,
    ) {
        loop {
            let order = Self::canonical(self.waiters.iter().map(|waiter| waiter.hash.clone()));
            let found = order.into_iter().find(|index| {
                let waiter = &self.waiters[*index];
                waiter.pat.fits(value) && waiter.guard.admits(value)
            });
            let Some(index) = found else {
                self.records.push(ModelRecord {
                    value,
                    persist,
                    hash,
                });
                return;
            };
            let waiter = self.waiters[index].clone();
            fires.push((waiter.name.clone(), waiter.pat.bound(value)));
            if !waiter.persist {
                self.waiters.remove(index);
            }
            let spent = !persist && !waiter.peek;
            assert!(
                spent || !waiter.persist,
                "the generators never make a divergent pair"
            );
            if spent {
                return;
            }
        }
    }

    /// RECEIVE: the first record in canonical order that the pattern fits, the guard checked
    /// once on it, a park on a refusal, and the re-offer of a peeked record.
    fn receive(&mut self, waiter: ModelWaiter, fires: &mut Vec<(String, Vec<Par>)>) {
        loop {
            let order = Self::canonical(self.records.iter().map(|record| record.hash.clone()));
            let found = order
                .into_iter()
                .find(|index| waiter.pat.fits(self.records[*index].value));
            let admitted = found.filter(|index| waiter.guard.admits(self.records[*index].value));
            let Some(index) = admitted else {
                if !self.waiters.iter().any(|parked| parked.hash == waiter.hash) {
                    self.waiters.push(waiter);
                }
                return;
            };
            let record = self.records[index].clone();
            fires.push((waiter.name.clone(), waiter.pat.bound(record.value)));
            if !record.persist {
                self.records.remove(index);
            }
            if waiter.peek && !record.persist {
                self.offer(record.value, false, record.hash.clone(), fires);
            }
            let spent = !waiter.persist;
            assert!(
                spent || !(waiter.peek || record.persist),
                "the generators never make a divergent pair"
            );
            if spent {
                return;
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

/// The [`EnvelopeMatcher`] under the pins of the canonical choice (decision 89, DR-129). Host
/// play picks at random among candidates, so the host program pins each call to the choice of
/// the canonical order of decision 41. A consume binds only the pinned records, and a produce
/// fires only the pinned waiter.
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

/// A `ChargingRSpace` over a fresh in-memory space with the [`ChosenMatcher`] of `pins`, which
/// charges `budget`.
async fn chosen_host(
    budget: &RuntimeBudget,
    pins: &HostPins,
) -> impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation> {
    let mut stores = InMemoryStoreManager::new();
    let space = ParRSpace::create(
        stores.r_space_stores().await.expect("in-memory stores"),
        Arc::new(Box::new(ChosenMatcher { pins: pins.clone() })),
    )
    .expect("an in-memory space");
    let host: RhoISpace = Arc::new(Box::new(space));
    ChargingRSpace::charging_rspace(host, budget.clone())
}

/// The stored record that a consume of `receive` binds on `host` under the canonical order of
/// decision 41: the first record in canonical order whose payload the pattern binds. The guard
/// is checked once on it (decision 37). `None` means that no record binds.
async fn record_choice(
    host: &impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    receive: &ConsumeOp,
) -> Option<Datum<ListParWithRandom>> {
    let records = host.get_data(&data()).await;
    canonical_data_order(&records)
        .into_iter()
        .map(|index| &records[index])
        .find(|stored| EnvelopeMatcher.get(&receive.pattern, &stored.a).is_some())
        .cloned()
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
/// (`produce_peeks`). Each call is pinned first to the canonical choice on the host records
/// (decision 89, DR-129). It returns the legacy log entries of the calls and the fires, in
/// order.
async fn host_program(
    host: &impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    budget: &RuntimeBudget,
    pins: &HostPins,
    op: &SpaceOp,
) -> Result<(Vec<Cost>, Vec<(String, Vec<Par>)>), TestCaseError> {
    let mut calls = VecDeque::from([match op {
        SpaceOp::Produce(send) => HostCall::Send {
            record: record::wrap_datum(0, &send.data),
            persist: send.persistent,
        },
        SpaceOp::Consume(receive) => HostCall::Receive(Box::new(receive.clone())),
        SpaceOp::Settle(_) => panic!("a bag issues no settle"),
        SpaceOp::Join(_) => panic!("the bag programs of this suite issue no join"),
    }]);
    let mut entries = Vec::new();
    let mut fires = Vec::new();
    while let Some(call) = calls.pop_front() {
        let (fired, again, reproduced) = match call {
            HostCall::Send { record, persist } => {
                let waiter = waiter_choice(host, &data(), &record).await;
                set_pins(pins, None, waiter);
                let run = host_produce(host, budget, data(), record.clone(), persist).await;
                entries.extend(run.entries);
                let again = match (&run.fired, persist) {
                    (Some(_), true) => Some(HostCall::Send { record, persist }),
                    _ => None,
                };
                (run.fired, again, run.reproduced)
            }
            HostCall::Receive(receive) => {
                // Changed by decision 89 (DR-129): the consume binds the canonical record.
                // let run = host_consume(
                //     host,
                //     budget,
                //     vec![data()],
                //     vec![receive.pattern.clone()],
                //     parked(&receive),
                //     receive.persistent,
                //     receive.peek,
                // )
                // .await;
                let choice = record_choice(host, &receive).await;
                let expected: Vec<bool> = choice.iter().map(|stored| stored.persist).collect();
                let pinned = choice.into_iter().map(|stored| stored.a).collect();
                set_pins(pins, Some(pinned), WaiterPin::Any);
                let run = drawn_consume(host, &expected, || {
                    host_consume(
                        host,
                        budget,
                        vec![data()],
                        vec![receive.pattern.clone()],
                        parked(&receive),
                        receive.persistent,
                        receive.peek,
                    )
                })
                .await
                .map_err(TestCaseError::fail)?;
                entries.extend(run.entries);
                let again = match (&run.fired, receive.persistent) {
                    (Some(_), true) => Some(HostCall::Receive(receive)),
                    _ => None,
                };
                (run.fired, again, run.reproduced)
            }
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
    Ok((entries, fires))
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
/// the new legacy log entries (value and label), the fires, and at the end the stores. Each
/// side runs inside the legacy scope of its own budget. The host program pins each call to the
/// canonical choice (decision 89, DR-129), so the comparison stays exact.
async fn compare_with_host(ops: &[SpaceOp]) -> Result<(), TestCaseError> {
    let agent_budget = RuntimeBudget::new(Cost::create(LEGACY_PHLO, "rrs bag agent"));
    let _agent_scope = agent_budget.enter_legacy_scope();
    let rig = Rig::with_budget(&agent_budget).await;
    let host_budget = RuntimeBudget::new(Cost::create(LEGACY_PHLO, "rrs bag host"));
    let _host_scope = host_budget.enter_legacy_scope();
    // Changed by decision 89 (DR-129): the host matches under the pins of the canonical choice.
    // let host = envelope_host(&host_budget).await;
    let pins: HostPins = Arc::new(Mutex::new(open_pins()));
    let host = chosen_host(&host_budget, &pins).await;
    for (index, op) in ops.iter().enumerate() {
        let agent_before = agent_budget.get_log().len();
        let outcome = rig
            .run(op.clone())
            .await
            .map_err(|error| TestCaseError::fail(format!("operation {index} fails: {error}")))?;
        let agent_entries = agent_budget.get_log()[agent_before..].to_vec();
        // Changed by decision 89 (DR-129): a reverted draw leaves its charges in the log of the
        // budget, so the host program returns the entries of the calls that it keeps.
        // let host_before = host_budget.get_log().len();
        // let host_fires = host_program(&host, &host_budget, op).await;
        // let host_entries = host_budget.get_log()[host_before..].to_vec();
        let (host_entries, host_fires) = host_program(&host, &host_budget, &pins, op).await?;
        prop_assert!(!agent_entries.is_empty(), "operation {} charges", index);
        prop_assert_eq!(
            &agent_entries,
            &host_entries,
            "the legacy charges of operation {}: {:?}",
            index,
            op
        );
        prop_assert_eq!(
            fired(&outcome),
            host_fires,
            "the fires of operation {}",
            index
        );
        prop_assert_eq!(&outcome.reissue, &None, "operation {} re-issues", index);
    }
    prop_assert_eq!(store_bytes(&rig.host).await, store_bytes(&host).await);
    Ok(())
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

/// A receive takes the first record in the canonical order of DR-75, and a send fires the first
/// waiter in canonical order: by the source hash, whatever the list order of the store
/// (decision 41, F1, F15).
#[tokio::test]
async fn space_disciplines_bag_first_match_canonical_order() {
    let rig = Rig::new().await;
    let sends = ordered_sends(&[(3, false), (1, false), (2, false)]);
    for send in &sends {
        rig.send(send.clone()).await;
    }
    let listed: Vec<i64> = rig
        .host
        .get_data(&data())
        .await
        .iter()
        .map(|datum| value_of(&record::unwrap_datum(&datum.a).expect("a record").datum.pars))
        .collect();
    assert_eq!(
        listed,
        vec![2, 1, 3],
        "the store lists the records newest first, not in canonical order"
    );
    let mut delivered = Vec::new();
    for name in ["a", "b", "c"] {
        delivered.extend(fired(&rig.receive(receiving(name, any())).await));
    }
    assert_eq!(delivered, vec![
        delivery("a", &[3]),
        delivery("b", &[1]),
        delivery("c", &[2])
    ]);

    let rig = Rig::new().await;
    let receives = ordered_receives(vec![
        receiving("x", any()),
        receiving("y", any()),
        receiving("z", any()),
    ]);
    for receive in &receives {
        rig.receive(receive.clone()).await;
    }
    assert_eq!(
        rig.host
            .get_waiting_continuations(vec![data()])
            .await
            .iter()
            .map(|waiter| body_name(
                &record::unwrap_continuation(&waiter.continuation)
                    .expect("a continuation record")
                    .continuation
            ))
            .collect::<Vec<String>>(),
        vec!["z".to_string(), "y".to_string(), "x".to_string()],
        "the store lists the waiters newest first, not in canonical order"
    );
    let mut delivered = Vec::new();
    for value in [7, 8, 9] {
        delivered.extend(fired(&rig.send(salted(value, 1, false)).await));
    }
    assert_eq!(delivered, vec![
        delivery("x", &[7]),
        delivery("y", &[8]),
        delivery("z", &[9])
    ]);
}

/// A receive checks its guard once, on its first spatial match. A refusal parks it, although a
/// later record would pass the guard (decision 37, F2).
#[tokio::test]
async fn space_disciplines_bag_guard_checked_once_on_first_spatial_match() {
    let rig = Rig::new().await;
    let sends = ordered_sends(&[(1, false), (2, false)]);
    for send in &sends {
        rig.send(send.clone()).await;
    }
    let parked = rig.receive(guarded(receiving("w", any()), equals(2))).await;
    assert!(
        parked.fires.is_empty(),
        "the guard refuses the first match 1"
    );
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
/// first, so a guarded waiter that admits the record fires (decision 45, Rule E).
#[tokio::test]
async fn space_disciplines_bag_peek_restore_fires_guarded_waiter() {
    let rig = Rig::new().await;
    let sends = ordered_sends(&[(1, false), (2, false)]);
    for send in &sends {
        rig.send(send.clone()).await;
    }
    rig.receive(guarded(receiving("w", any()), equals(2))).await;
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
/// and it is stored when no waiter takes it (decision 30).
#[tokio::test]
async fn space_disciplines_bag_peek_waiter_reoffers_the_datum() {
    let rig = Rig::new().await;
    let receives = ordered_receives(vec![peeking(receiving("p", any())), receiving("t", any())]);
    for receive in &receives {
        rig.receive(receive.clone()).await;
    }
    rig.log().await;
    let outcome = rig.send(salted(3, 1, false)).await;
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

/// A persistent send fires every waiter that takes it, in canonical order, and is then stored.
#[tokio::test]
async fn space_disciplines_bag_persistent_send_fires_each_waiter_then_stores() {
    let rig = Rig::new().await;
    let receives = ordered_receives(vec![receiving("w1", any()), receiving("w2", any())]);
    for receive in &receives {
        rig.receive(receive.clone()).await;
    }
    rig.log().await;
    let outcome = rig.send(salted(4, 1, true)).await;
    assert_eq!(fired(&outcome), vec![
        delivery("w1", &[4]),
        delivery("w2", &[4])
    ]);
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
/// which has the identity of the record (decision 45, Rule E, decision 43).
#[tokio::test]
async fn space_disciplines_bag_peek_restore_reissue_keeps_identity() {
    let rig = Rig::new().await;
    let sends = ordered_sends(&[(1, false), (2, false)]);
    for send in &sends {
        rig.send(send.clone()).await;
    }
    for index in 0..FIRE_ROUND_CAP {
        rig.receive(peeking(guarded(
            receiving(&format!("p{index}"), any()),
            equals(2),
        )))
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
/// space counter stays 0, and every entry has the `ord` 0.
#[tokio::test]
async fn space_disciplines_bag_no_counter_events() {
    let rig = Rig::new().await;
    for send in ordered_sends(&[(1, false), (2, true)]) {
        rig.send(send).await;
    }
    rig.receive(peeking(receiving("p", any()))).await;
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

/// After a checkpoint and a reset, the store holds the records in the order of the history,
/// and the agent makes the same choice and reaches the same root.
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
    let first = fired(&rig.receive(receiving("a", any())).await);
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
    let again = fired(&rig.receive(receiving("a", any())).await);
    assert_eq!(again, first, "the choice does not depend on the list order");
    assert_eq!(
        rig.host
            .create_checkpoint()
            .await
            .expect("a checkpoint")
            .root,
        after
    );
}

/// The script of the replay test: stores, equal records, fires on both sides, guard refusals,
/// peeks with their restores, a restore that fires a guarded waiter, persistent operations and,
/// at the end, a divergent pair. The records follow each other in canonical order, so every
/// operation makes a known number of fires ([`REPLAY_FIRES`]).
fn replay_script() -> Vec<SpaceOp> {
    let sends = ordered_sends(&[(1, false), (2, false), (5, false)]);
    vec![
        SpaceOp::Produce(sends[0].clone()),
        SpaceOp::Produce(sends[1].clone()),
        SpaceOp::Produce(sends[2].clone()),
        SpaceOp::Produce(sends[2].clone()),
        SpaceOp::Consume(guarded(receiving("g", any()), equals(2))),
        SpaceOp::Consume(peeking(receiving("p", exactly(2)))),
        SpaceOp::Consume(receiving("a", exactly(5))),
        SpaceOp::Consume(peeking(receiving("q", any()))),
        SpaceOp::Consume(receiving("t", exactly(7))),
        SpaceOp::Consume(guarded(receiving("refuses", any()), refusing())),
        SpaceOp::Produce(salted(7, 1, true)),
        SpaceOp::Consume(persisting(receiving("drain", exactly(5)))),
        SpaceOp::Produce(salted(8, 1, false)),
        SpaceOp::Produce(salted(5, 9, true)),
    ]
}

/// The fires of each operation of [`replay_script`]: the guarded waiter parks on 1, the peek of
/// 2 and its restore fire `p` and `g`, `a` takes one 5, `q` peeks 1 and restores it, the
/// persistent 7 fires `t` and is stored, the drain takes the other 5 and parks, and the last
/// persistent 5 fires the drain as a divergent pair.
const REPLAY_FIRES: [usize; 14] = [0, 0, 0, 0, 0, 2, 1, 1, 0, 0, 1, 1, 0, 1];

/// Runs the replay script on `rig`.
async fn run_replay_script(rig: &Rig) -> Result<Vec<SpaceOutcome>, SpaceError> {
    let mut outcomes = Vec::new();
    for op in replay_script() {
        outcomes.push(rig.run(op).await?);
    }
    Ok(outcomes)
}

/// Replay stages the transactions of play: equal outcomes, every recorded COMM paired, and the
/// root of play. A replay without the log diverges.
#[tokio::test]
async fn space_disciplines_bag_replay_equal() {
    let (play, replay) = play_and_replay().await;
    let budget = budget();
    let play_rig = Rig::over(play.clone(), &budget).await;
    let start = play
        .create_checkpoint()
        .await
        .expect("a checkpoint after the creation")
        .root;
    let played = run_replay_script(&play_rig)
        .await
        .expect("the script plays");
    assert_eq!(
        played
            .iter()
            .map(|outcome| outcome.fires.len())
            .collect::<Vec<usize>>(),
        REPLAY_FIRES.to_vec()
    );
    assert_eq!(
        played
            .last()
            .and_then(|outcome| outcome.reissue.as_ref())
            .map(|reissue| reissue.cause),
        Some(ReissueCause::DivergentPair)
    );
    let Checkpoint { root, log } = play.create_checkpoint().await.expect("a checkpoint");
    replay
        .rig_and_reset(start.clone(), log)
        .await
        .expect("the replay rigs the play log");
    let replay_rig = Rig {
        host: play.clone(),
        env: env_over(replay.clone(), &budget),
    };
    let replayed = run_replay_script(&replay_rig)
        .await
        .expect("the script replays");
    assert_eq!(replayed, played);
    replay
        .check_replay_data()
        .await
        .expect("every recorded COMM is paired");
    assert_eq!(
        replay.create_checkpoint().await.expect("a checkpoint").root,
        root
    );
    replay
        .rig_and_reset(start, Vec::new())
        .await
        .expect("the replay rigs an empty log");
    replay_rig
        .run(replay_script().remove(0))
        .await
        .expect("a store makes no COMM, so it replays without the log");
    match replay_rig
        .run(SpaceOp::Consume(receiving("x", any())))
        .await
    {
        Err(error @ SpaceError::ReplayDivergence(_)) => {
            assert_eq!(error.code(), "ReplayDivergence")
        }
        other => panic!("negative control: a log without the COMMs diverges: {other:?}"),
    }
}

/// In a legacy deploy, each bag operation pays what `ChargingRSpace` charges for its host
/// program, entry by entry, the second send of a peek and its fire included (decision 45,
/// Option P and Rule E).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_disciplines_bag_legacy_charges_equal_host() {
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
    compare_with_host(&ops)
        .await
        .unwrap_or_else(|failure| panic!("{failure}"));
}

/// A persistent record and a non-persistent record with equal bytes have two source hashes, and
/// a receive binds the first of them in canonical order (decision 41). The pin of the host
/// program names a record only by its bytes, and host play picks between the two at random
/// (DR-129). So the program makes its consume again until it binds the record with the
/// canonical persist flag (decision 89). The test runs the program `TWIN_RUNS` times.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_disciplines_bag_persistent_twin_binds_the_canonical_record() {
    let twin = salted(1, 0, true);
    let record = salted(1, 0, false);
    assert_eq!(twin.data, record.data, "the two records have equal bytes");
    let ops = vec![
        SpaceOp::Produce(twin),
        SpaceOp::Produce(record),
        SpaceOp::Consume(receiving("t", exactly(1))),
    ];
    for _ in 0..TWIN_RUNS {
        compare_with_host(&ops)
            .await
            .unwrap_or_else(|failure| panic!("{failure}"));
    }
}

/// A fire removes exactly the waiter that it fires, by its record identity. Two waiters with
/// equal patterns stay apart, where a removal by pattern equality could take the other one
/// (D10, F12, F13).
#[tokio::test]
async fn space_disciplines_bag_waiter_removal_by_identity_not_pattern() {
    let rig = Rig::new().await;
    let receives = ordered_receives(vec![receiving("first", any()), receiving("second", any())]);
    assert_eq!(
        receives[0].pattern, receives[1].pattern,
        "the two waiters have equal patterns"
    );
    for receive in receives.iter().rev() {
        rig.receive(receive.clone()).await;
    }
    assert_eq!(fired(&rig.send(salted(1, 1, false)).await), vec![delivery(
        "first",
        &[1]
    )]);
    assert_eq!(rig.waiters().await, vec!["second".to_string()]);
    assert_eq!(fired(&rig.send(salted(2, 1, false)).await), vec![delivery(
        "second",
        &[2]
    )]);
    assert!(rig.waiters().await.is_empty());
}

// ─── Property tests: the invariants of DataCollection.v and ContinuationCollection.v ───────

/// A failure of a property case with `message`.
fn failure(message: impl Into<String>) -> TestCaseError { TestCaseError::fail(message.into()) }

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

/// F1 `bag_visit_order_canonical`: receives that take every value drain the records in the
/// canonical order of their source hashes, for every insertion order (decision 41).
#[test]
fn prop_bag_visit_order_canonical() {
    let strategy = arb_records(6)
        .prop_filter("at least one record", |records| !records.is_empty())
        .prop_flat_map(|records| (Just(records.clone()), Just(records).prop_shuffle()));
    check(pure_config(), strategy, |(records, shuffled)| {
        tokio_runtime().block_on(visit_order_case(records, shuffled))
    });
}

async fn visit_order_case(
    records: Vec<(i64, u64)>,
    shuffled: Vec<(i64, u64)>,
) -> Result<(), TestCaseError> {
    let mut drains = Vec::with_capacity(2);
    for order in [&records, &shuffled] {
        let rig = Rig::new().await;
        for (value, salt) in order {
            rig.send(salted(*value, *salt, false)).await;
        }
        let mut hashes = Vec::with_capacity(order.len());
        for index in 0..order.len() {
            let outcome = rig.receive(receiving(&format!("r{index}"), any())).await;
            hashes.push(Produce::create(&data(), &fired_record(&outcome)?, false).hash);
        }
        let mut sorted = hashes.clone();
        sorted.sort();
        prop_assert_eq!(&hashes, &sorted, "the records leave in canonical order");
        prop_assert!(rig.values().await.is_empty());
        drains.push(hashes);
    }
    prop_assert_eq!(
        &drains[0],
        &drains[1],
        "the insertion order changes nothing"
    );
    Ok(())
}

/// F2 `bag_consume_first_spatial_match`: a receive takes the first record in canonical order
/// that its pattern binds, and it checks its guard once, on that record. A refusal parks it,
/// whatever the later records (decision 37).
#[test]
fn prop_bag_consume_first_spatial_match() {
    let strategy = (arb_records(6), arb_pat())
        .prop_flat_map(|(records, pat)| (Just(records), Just(pat), arb_guard(pat)));
    check(pure_config(), strategy, |(records, pat, guard)| {
        tokio_runtime().block_on(first_match_case(records, pat, guard))
    });
}

async fn first_match_case(
    records: Vec<(i64, u64)>,
    pat: Pat,
    guard: Guard,
) -> Result<(), TestCaseError> {
    let rig = Rig::new().await;
    let sends: Vec<ProduceOp> = records
        .iter()
        .map(|(value, salt)| salted(*value, *salt, false))
        .collect();
    for send in &sends {
        rig.send(send.clone()).await;
    }
    let mut canonical = sends.clone();
    canonical.sort_by_key(record_hash);
    let first = canonical
        .iter()
        .find(|send| pat.fits(value_of(&send.data.pars)));
    let outcome = rig
        .receive(ConsumeOp {
            guard: guard.par(),
            ..receiving("k", pat.pattern())
        })
        .await;
    match first {
        Some(send) if guard.admits(value_of(&send.data.pars)) => {
            prop_assert_eq!(fired_record(&outcome)?, record::wrap_datum(0, &send.data));
            prop_assert!(rig.waiters().await.is_empty());
            prop_assert_eq!(rig.values().await.len() + 1, sends.len());
        }
        _ => {
            prop_assert!(
                outcome.fires.is_empty(),
                "no match, or the guard refuses the first one"
            );
            prop_assert_eq!(rig.waiters().await, vec!["k".to_string()]);
            prop_assert_eq!(rig.values().await.len(), sends.len());
        }
    }
    Ok(())
}

/// F3 `bag_multiplicity_preserved`: over generated programs, the agent keeps the multiset of the
/// model. A store adds exactly one record, a fire removes exactly the non-persistent records
/// that it binds, and a peek sends its record again (decision 45, Rule E). The fires agree too.
#[test]
fn prop_bag_multiplicity_preserved() {
    check(pure_config(), arb_any_program(), |program| {
        tokio_runtime().block_on(multiplicity_case(program))
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
        let expected = model.apply(op);
        let outcome = rig
            .run(op.space_op())
            .await
            .map_err(|error| failure(format!("operation {index} fails: {error}")))?;
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

/// `bag_peek_restore_is_host_second_send` and Option P: over generated programs, in a legacy
/// deploy, each operation stages the operations of its host program and pays its charges, the
/// second send of each peek and its fires included. The stores end equal.
#[test]
fn prop_bag_legacy_charges_equal_host() {
    check(pure_config(), arb_any_program(), |program| {
        let ops: Vec<SpaceOp> = program.iter().map(BagOp::space_op).collect();
        tokio_runtime().block_on(compare_with_host(&ops))
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

/// The name of the waiter that a send of `value` fires among `waiters`: the first in canonical
/// order whose pattern fits and whose guard admits the value (decision 37, decision 41).
fn expected_waiter(waiters: &[(Pat, Guard)], value: i64) -> Option<String> {
    let mut candidates: Vec<(Blake2b256Hash, usize)> = waiters
        .iter()
        .enumerate()
        .filter(|(_, (pat, guard))| pat.fits(value) && guard.admits(value))
        .map(|(index, waiter)| (waiter_hash(&plain_receive(index, *waiter)), index))
        .collect();
    candidates.sort();
    candidates.first().map(|(_, index)| format!("w{index}"))
}

/// F12 `waiter_removal_by_record_identity`: a send fires the first admitting waiter in
/// canonical order and removes exactly that waiter, by its identity. Every other waiter stays.
#[test]
fn prop_bag_waiter_removal_by_record_identity() {
    check(
        pure_config(),
        (arb_plain_waiters(6), 0i64..4),
        |(waiters, value)| tokio_runtime().block_on(removal_case(waiters, value)),
    );
}

async fn removal_case(waiters: Vec<(Pat, Guard)>, value: i64) -> Result<(), TestCaseError> {
    let rig = Rig::new().await;
    for (index, waiter) in waiters.iter().enumerate() {
        rig.receive(plain_receive(index, *waiter)).await;
    }
    let names: Vec<String> = (0..waiters.len())
        .map(|index| format!("w{index}"))
        .collect();
    let outcome = rig.send(salted(value, 1, false)).await;
    let fired_names: Vec<String> = fired(&outcome).into_iter().map(|(name, _)| name).collect();
    let expected = expected_waiter(&waiters, value);
    prop_assert_eq!(
        &fired_names,
        &expected.iter().cloned().collect::<Vec<String>>()
    );
    let mut remaining = names.clone();
    remaining.retain(|name| Some(name) != expected.as_ref());
    remaining.sort();
    prop_assert_eq!(rig.waiters().await, remaining);
    let stored = match expected {
        Some(_) => Vec::new(),
        None => vec![(value, false)],
    };
    prop_assert_eq!(rig.values().await, stored);
    Ok(())
}

/// F13 `pattern_equality_removal_can_remove_another_waiter`, the agent side: waiters with equal
/// patterns and guards stay distinct. Each send removes exactly the waiter that it fires, in
/// canonical order, never another waiter with the same pattern.
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
    let mut canonical: Vec<(Blake2b256Hash, String)> = waiters
        .iter()
        .enumerate()
        .map(|(index, waiter)| {
            (
                waiter_hash(&plain_receive(index, *waiter)),
                format!("w{index}"),
            )
        })
        .collect();
    canonical.sort();
    for (step, (_, name)) in canonical.iter().enumerate() {
        let outcome = rig
            .send(salted(
                value,
                u64::try_from(step).expect("a small step"),
                false,
            ))
            .await;
        let names: Vec<String> = fired(&outcome).into_iter().map(|(name, _)| name).collect();
        prop_assert_eq!(names, vec![name.clone()]);
        let mut remaining: Vec<String> = canonical[step + 1..]
            .iter()
            .map(|(_, name)| name.clone())
            .collect();
        remaining.sort();
        prop_assert_eq!(rig.waiters().await, remaining);
    }
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

/// F15 `waiter_visit_order_canonical`: a send fires the first waiter in canonical order that
/// binds and admits it, for every park order (decision 37, decision 41).
#[test]
fn prop_bag_waiter_visit_order_canonical() {
    let strategy = (arb_plain_waiters(6), 0i64..4).prop_flat_map(|(waiters, value)| {
        let count = waiters.len();
        (
            Just(waiters),
            Just(value),
            Just((0..count).collect::<Vec<usize>>()).prop_shuffle(),
        )
    });
    check(pure_config(), strategy, |(waiters, value, order)| {
        tokio_runtime().block_on(waiter_order_case(waiters, value, order))
    });
}

async fn waiter_order_case(
    waiters: Vec<(Pat, Guard)>,
    value: i64,
    order: Vec<usize>,
) -> Result<(), TestCaseError> {
    let expected: Vec<String> = expected_waiter(&waiters, value).into_iter().collect();
    let natural: Vec<usize> = (0..waiters.len()).collect();
    for park_order in [&natural, &order] {
        let rig = Rig::new().await;
        for index in park_order {
            rig.receive(plain_receive(*index, waiters[*index])).await;
        }
        let outcome = rig.send(salted(value, 1, false)).await;
        let names: Vec<String> = fired(&outcome).into_iter().map(|(name, _)| name).collect();
        prop_assert_eq!(&names, &expected, "the park order {:?}", park_order);
    }
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
